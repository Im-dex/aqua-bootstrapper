use tokio::process::Command;

use crate::Result;

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
    use std::io;
    use std::os::raw::c_int;
    use std::os::unix::process::CommandExt;

    use tokio::process::Command;

    const PR_SET_PDEATHSIG: c_int = 1;
    #[cfg(test)]
    const PR_GET_PDEATHSIG: c_int = 2;
    const SIGTERM: c_int = 15;
    const ESRCH: c_int = 3;

    unsafe extern "C" {
        fn getppid() -> c_int;
        fn prctl(option: c_int, ...) -> c_int;
    }

    pub fn configure_child(command: &mut Command) {
        let expected_parent = unsafe { getppid() };
        configure_child_for_parent(command, expected_parent);
    }

    fn configure_child_for_parent(command: &mut Command, expected_parent: c_int) {
        unsafe {
            command.as_std_mut().pre_exec(move || {
                if prctl(PR_SET_PDEATHSIG, SIGTERM) != 0 {
                    return Err(io::Error::last_os_error());
                }

                if getppid() != expected_parent {
                    return Err(io::Error::from_raw_os_error(ESRCH));
                }

                Ok(())
            });
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{
            PR_GET_PDEATHSIG, SIGTERM, configure_child, configure_child_for_parent, prctl,
        };
        use tokio::process::Command;

        const HELPER_ENV: &str = "AQUA_BOOTSTRAPPER_PDEATHSIG_TEST_HELPER";
        const TEST_NAME: &str =
            "process_containment::linux::tests::child_receives_parent_death_signal";

        #[tokio::test]
        async fn child_receives_parent_death_signal() {
            if std::env::var_os(HELPER_ENV).is_some() {
                let mut signal = 0;
                let result = unsafe { prctl(PR_GET_PDEATHSIG, &raw mut signal) };

                assert_eq!(result, 0);
                assert_eq!(signal, SIGTERM);
                return;
            }

            let mut command = Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", TEST_NAME]).env(HELPER_ENV, "1");
            configure_child(&mut command);

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
