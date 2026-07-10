use std::path::Path;
use std::process::Stdio;

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
        Some(code) => Ok(code),
        None => Err(Error::CommandTerminated {
            name: name.to_string(),
        }),
    }
}

pub async fn run_capture_stdout(
    name: &str,
    executable: &Path,
    args: &[String],
    envs: Option<&[(String, String)]>,
) -> Result<String> {
    let mut command = Command::new(executable);
    crate::process_containment::configure_child(&mut command);
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
