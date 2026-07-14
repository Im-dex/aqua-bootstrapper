use std::ffi::OsString;

use tokio::process::Command;

use crate::Result;

pub const PROCESS_TEMPLATE_ENV: &str = "PROCESS_CONTAINMENT_TEMPLATE_JSON";
#[cfg(target_os = "linux")]
pub const PARENT_PID_PLACEHOLDER: &str = "{parent_pid}";

pub fn command_template_json() -> Result<String> {
    #[cfg(target_os = "linux")]
    let template = vec![
        std::env::current_exe()?.display().to_string(),
        "pdeathsig".to_string(),
        "--parent-pid".to_string(),
        PARENT_PID_PLACEHOLDER.to_string(),
        "--".to_string(),
    ];
    #[cfg(not(target_os = "linux"))]
    let template: Vec<String> = Vec::new();

    Ok(serde_json::to_string(&template)?)
}

#[cfg(test)]
mod command_template_tests {
    #[cfg(target_os = "linux")]
    use super::PARENT_PID_PLACEHOLDER;
    use super::command_template_json;

    #[test]
    fn command_template_is_a_json_argument_array() {
        let template: Vec<String> =
            serde_json::from_str(&command_template_json().unwrap()).unwrap();

        #[cfg(target_os = "linux")]
        assert_eq!(
            template,
            [
                std::env::current_exe().unwrap().display().to_string(),
                "pdeathsig".to_string(),
                "--parent-pid".to_string(),
                PARENT_PID_PLACEHOLDER.to_string(),
                "--".to_string(),
            ]
        );
        #[cfg(not(target_os = "linux"))]
        assert!(template.is_empty());
    }
}

#[cfg(target_os = "linux")]
pub fn exec_with_parent_death_signal(parent_pid: u32, command: &[OsString]) -> Result<i32> {
    linux::exec_with_parent_death_signal(parent_pid, command)
}

#[cfg(not(target_os = "linux"))]
pub fn exec_with_parent_death_signal(_parent_pid: u32, _command: &[OsString]) -> Result<i32> {
    Err(crate::Error::UnsupportedPlatform(
        "pdeathsig is only supported on Linux".to_string(),
    ))
}

#[cfg(windows)]
pub fn init() -> Result<()> {
    windows::init()
}

#[cfg(not(windows))]
pub fn init() -> Result<()> {
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn configure_child(command: &mut Command) {
    linux::configure_child(command);
}

#[cfg(not(target_os = "linux"))]
pub fn configure_child(_command: &mut Command) {}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::OsString;
    use std::io;
    use std::os::raw::c_int;
    use std::os::unix::process::CommandExt;
    use std::process::Command as StdCommand;

    use tokio::process::Command;

    use crate::{Error, Result};

    const PR_SET_PDEATHSIG: c_int = 1;
    #[cfg(test)]
    const PR_GET_PDEATHSIG: c_int = 2;
    const SIGTERM: c_int = 15;
    const ESRCH: c_int = 3;

    unsafe extern "C" {
        fn getpid() -> c_int;
        fn getppid() -> c_int;
        fn prctl(option: c_int, ...) -> c_int;
    }

    pub fn configure_child(command: &mut Command) {
        let expected_parent = unsafe { getpid() };
        configure_child_for_parent(command, expected_parent);
    }

    pub fn exec_with_parent_death_signal(parent_pid: u32, command: &[OsString]) -> Result<i32> {
        let Some((program, args)) = command.split_first() else {
            return Err(Error::ProcessContainment {
                operation: "executing the pdeathsig command",
                source: io::Error::new(io::ErrorKind::InvalidInput, "command is empty"),
            });
        };

        let expected_parent = c_int::try_from(parent_pid)
            .ok()
            .filter(|pid| *pid > 0)
            .ok_or_else(|| Error::ProcessContainment {
                operation: "configuring the pdeathsig process",
                source: io::Error::new(io::ErrorKind::InvalidInput, "parent PID is invalid"),
            })?;
        configure_current_process(expected_parent).map_err(|source| Error::ProcessContainment {
            operation: "configuring the pdeathsig process",
            source,
        })?;

        let mut child = StdCommand::new(program);
        child.args(args);
        let source = child.exec();
        Err(Error::ProcessContainment {
            operation: "executing the pdeathsig command",
            source,
        })
    }

    fn configure_child_for_parent(command: &mut Command, expected_parent: c_int) {
        unsafe {
            command
                .as_std_mut()
                .pre_exec(move || configure_current_process(expected_parent));
        }
    }

    fn configure_current_process(expected_parent: c_int) -> io::Result<()> {
        unsafe {
            if prctl(PR_SET_PDEATHSIG, SIGTERM) != 0 {
                return Err(io::Error::last_os_error());
            }

            if getppid() != expected_parent {
                return Err(io::Error::from_raw_os_error(ESRCH));
            }
        }

        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use std::io;
        use std::os::unix::process::CommandExt;

        use super::{
            PR_GET_PDEATHSIG, SIGTERM, configure_child, configure_child_for_parent, prctl,
        };
        use tokio::process::Command;

        #[tokio::test]
        async fn child_receives_parent_death_signal() {
            let mut command = Command::new("/bin/true");
            configure_child(&mut command);
            unsafe {
                command.as_std_mut().pre_exec(|| {
                    let mut signal = 0;
                    if prctl(PR_GET_PDEATHSIG, &raw mut signal) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if signal != SIGTERM {
                        return Err(io::Error::other(format!(
                            "expected parent death signal {SIGTERM}, got {signal}"
                        )));
                    }

                    Ok(())
                });
            }

            assert!(command.status().await.unwrap().success());
        }

        #[tokio::test]
        async fn child_does_not_exec_after_parent_changes() {
            let mut command = Command::new(std::env::current_exe().unwrap());
            configure_child_for_parent(&mut command, -1);

            let error = command.status().await.unwrap_err();
            assert_eq!(error.raw_os_error(), Some(super::ESRCH));
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::io;
    use std::mem::size_of;
    use std::ptr;
    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    use crate::{Error, Result};

    static JOB_HANDLE: OnceLock<isize> = OnceLock::new();

    pub fn init() -> Result<()> {
        if JOB_HANDLE.get().is_some() {
            return Ok(());
        }

        let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if job.is_null() {
            return Err(containment_error(
                "creating the Windows Job Object",
                io::Error::last_os_error(),
            ));
        }

        if let Err(error) = set_kill_on_close(job).and_then(|()| assign_current_process(job)) {
            unsafe {
                CloseHandle(job);
            }
            return Err(error);
        }

        match JOB_HANDLE.set(job as isize) {
            Ok(()) => Ok(()),
            Err(_) => {
                unsafe {
                    CloseHandle(job);
                }
                Ok(())
            }
        }
    }

    fn set_kill_on_close(job: HANDLE) -> Result<()> {
        let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        let configured = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const information).cast::<c_void>(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            return Err(containment_error(
                "configuring the Windows Job Object",
                io::Error::last_os_error(),
            ));
        }

        Ok(())
    }

    fn assign_current_process(job: HANDLE) -> Result<()> {
        let assigned = unsafe { AssignProcessToJobObject(job, GetCurrentProcess()) };
        if assigned == 0 {
            return Err(containment_error(
                "assigning the bootstrapper to the Windows Job Object",
                io::Error::last_os_error(),
            ));
        }

        Ok(())
    }

    fn containment_error(operation: &'static str, source: io::Error) -> Error {
        Error::ProcessContainment { operation, source }
    }

    #[cfg(test)]
    mod tests {
        use std::ffi::c_void;
        use std::mem::size_of;
        use std::ptr;

        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            QueryInformationJobObject,
        };

        use super::set_kill_on_close;

        #[test]
        fn job_is_configured_to_kill_processes_when_closed() {
            let job = TestJob::new();
            set_kill_on_close(job.0).unwrap();

            let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            let queried = unsafe {
                QueryInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&raw mut information).cast::<c_void>(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                    ptr::null_mut(),
                )
            };

            assert_ne!(queried, 0);
            assert_ne!(
                information.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                0
            );
        }

        struct TestJob(HANDLE);

        impl TestJob {
            fn new() -> Self {
                let handle = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
                assert!(!handle.is_null());
                Self(handle)
            }
        }

        impl Drop for TestJob {
            fn drop(&mut self) {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }
}
