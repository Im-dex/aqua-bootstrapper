use std::path::Path;
use std::process::{ExitStatus, Stdio};

use tokio::process::Command;

use crate::error::{Error, Result};

pub async fn run_foreground(
    name: &str,
    executable: &Path,
    args: &[String],
    envs: Option<&[(String, String)]>,
) -> Result<i32> {
    let mut command = Command::new(executable);
    crate::process_containment::configure_child(&mut command);
    command.kill_on_drop(true);
    command
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(envs) = envs {
        command.envs(envs.iter().map(|(key, value)| (key, value)));
    }

    let status = command.status().await?;
    match status.code() {
        Some(code) if code == 0 => Ok(code),
        Some(code) => Err(Error::CommandFailed {
            name: name.to_string(),
            code,
        }),
        None => Err(Error::CommandTerminated {
            name: name.to_string(),
        }),
    }
}

pub async fn run_app(
    name: &str,
    executable: &Path,
    args: &[String],
    envs: Option<&[(String, String)]>,
) -> Result<i32> {
    let mut command = Command::new(executable);
    crate::process_containment::configure_child(&mut command);
    command.kill_on_drop(true);
    command
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(envs) = envs {
        command.envs(envs.iter().map(|(key, value)| (key, value)));
    }

    let status = command.status().await?;
    application_exit_code(name, status)
}

pub async fn run_capture_stdout(
    name: &str,
    executable: &Path,
    args: &[String],
    envs: Option<&[(String, String)]>,
) -> Result<String> {
    let mut command = Command::new(executable);
    crate::process_containment::configure_child(&mut command);
    command.kill_on_drop(true);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if let Some(envs) = envs {
        command.envs(envs.iter().map(|(key, value)| (key, value)));
    }

    let output = command.output().await?;
    match output.status.code() {
        Some(0) => String::from_utf8(output.stdout).map_err(|_| Error::CommandOutput {
            name: name.to_string(),
            reason: "stdout is not valid UTF-8".to_string(),
        }),
        Some(code) => Err(Error::CommandFailed {
            name: name.to_string(),
            code,
        }),
        None => Err(Error::CommandTerminated {
            name: name.to_string(),
        }),
    }
}

fn application_exit_code(name: &str, status: ExitStatus) -> Result<i32> {
    if let Some(code) = status.code() {
        return Ok(code);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        if let Some(signal) = status.signal() {
            return Ok(128 + signal);
        }
    }

    Err(Error::CommandTerminated {
        name: name.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    #[cfg(unix)]
    use std::path::Path;
    use std::path::PathBuf;
    use std::thread;
    use std::time::{Duration, Instant};

    use tempfile::tempdir;
    use tokio::task::JoinHandle;

    use super::{run_app, run_capture_stdout, run_foreground};
    use crate::Result;

    const STARTED_ENV: &str = "AQUA_BOOTSTRAPPER_CANCEL_TEST_STARTED";
    const FINISHED_ENV: &str = "AQUA_BOOTSTRAPPER_CANCEL_TEST_FINISHED";
    const CHILD_DELAY: Duration = Duration::from_secs(2);

    #[derive(Clone, Copy)]
    enum Runner {
        Foreground,
        App,
        CaptureStdout,
    }

    impl Runner {
        fn name(self) -> &'static str {
            match self {
                Self::Foreground => "foreground",
                Self::App => "app",
                Self::CaptureStdout => "capture-stdout",
            }
        }
    }

    #[tokio::test]
    async fn cancelling_command_future_kills_direct_child() {
        let directory = tempdir().unwrap();
        let executable = env::current_exe().unwrap();
        let mut children = Vec::new();

        for runner in [Runner::Foreground, Runner::App, Runner::CaptureStdout] {
            let started = directory.path().join(format!("{}.started", runner.name()));
            let finished = directory.path().join(format!("{}.finished", runner.name()));
            let args = vec![
                "--exact".to_string(),
                "process::tests::cancellation_child".to_string(),
                "--quiet".to_string(),
            ];
            let envs = vec![
                (
                    STARTED_ENV.to_string(),
                    started.as_os_str().to_string_lossy().into_owned(),
                ),
                (
                    FINISHED_ENV.to_string(),
                    finished.as_os_str().to_string_lossy().into_owned(),
                ),
            ];
            let executable = executable.clone();
            let name = runner.name();

            let task: JoinHandle<Result<()>> = match runner {
                Runner::Foreground => tokio::spawn(async move {
                    run_foreground(name, &executable, &args, Some(&envs))
                        .await
                        .map(|_| ())
                }),
                Runner::App => tokio::spawn(async move {
                    run_app(name, &executable, &args, Some(&envs))
                        .await
                        .map(|_| ())
                }),
                Runner::CaptureStdout => tokio::spawn(async move {
                    run_capture_stdout(name, &executable, &args, Some(&envs))
                        .await
                        .map(|_| ())
                }),
            };

            children.push((runner, task, started, finished));
        }

        for (_, _, started, _) in &children {
            wait_for_file(started.clone()).await;
        }

        for (_, task, _, _) in &children {
            task.abort();
        }
        for (runner, task, _, _) in children {
            let error = task.await.expect_err("cancelled task completed");
            assert!(
                error.is_cancelled(),
                "{} task was not cancelled: {error}",
                runner.name()
            );
        }

        wait_past_child_delay().await;
        for runner in [Runner::Foreground, Runner::App, Runner::CaptureStdout] {
            let finished = directory.path().join(format!("{}.finished", runner.name()));
            assert!(
                !finished.exists(),
                "{} child survived cancellation",
                runner.name()
            );
        }
    }

    #[test]
    fn cancellation_child() {
        let Some(started) = env::var_os(STARTED_ENV) else {
            return;
        };
        let Some(finished) = env::var_os(FINISHED_ENV) else {
            return;
        };

        fs::write(started, b"started").unwrap();
        thread::sleep(CHILD_DELAY);
        fs::write(finished, b"finished").unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn app_signal_uses_conventional_shell_exit_code() {
        let args = vec!["-c".to_string(), "kill -TERM $$".to_string()];

        let code = run_app("signal test", Path::new("/bin/sh"), &args, None)
            .await
            .unwrap();

        assert_eq!(code, 128 + 15);
    }

    async fn wait_for_file(path: PathBuf) {
        tokio::task::spawn_blocking(move || {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !path.exists() {
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for {}",
                    path.display()
                );
                thread::sleep(Duration::from_millis(10));
            }
        })
        .await
        .unwrap();
    }

    async fn wait_past_child_delay() {
        tokio::task::spawn_blocking(|| {
            thread::sleep(CHILD_DELAY + Duration::from_millis(500));
        })
        .await
        .unwrap();
    }
}
