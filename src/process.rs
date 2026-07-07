use std::path::Path;
use std::process::Stdio;

use tokio::process::Command;

use crate::error::{Error, Result};

pub async fn run_foreground(name: &str, executable: &Path, args: &[String]) -> Result<i32> {
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

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

pub async fn run_app(name: &str, executable: &Path, args: &[String]) -> Result<i32> {
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = command.status().await?;
    match status.code() {
        Some(code) => Ok(code),
        None => Err(Error::CommandTerminated {
            name: name.to_string(),
        }),
    }
}
