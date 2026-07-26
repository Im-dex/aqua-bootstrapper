mod aqua;
mod bootstrap;
mod config;
mod error;
mod fingerprint;
mod lock;
mod process;
mod process_containment;
mod state;
mod util;

use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;

use tracing_subscriber::EnvFilter;

use crate::bootstrap::Bootstrapper;
use crate::config::Config;

pub use crate::error::{Error, Result};

pub fn run_with_parent_death_signal(parent_pid: u32, command: &[OsString]) -> Result<i32> {
    process_containment::exec_with_parent_death_signal(parent_pid, command)
}

pub async fn run(config_path: PathBuf, app_args: Vec<String>) -> Result<i32> {
    init_tracing();
    process_containment::init()?;

    let config_path = accessible_absolute_path(config_path)?;
    let root = config_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let config = Config::read(&config_path)?;
    Bootstrapper::new(root, config, app_args).run().await
}

fn accessible_absolute_path(path: PathBuf) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    };

    fs::metadata(&absolute).map_err(|source| Error::BootstrapConfigInaccessible {
        path: absolute.clone(),
        source,
    })?;

    Ok(absolute)
}

fn init_tracing() {
    let Some(filter) = rust_log_filter(std::env::var_os(EnvFilter::DEFAULT_ENV)) else {
        return;
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .init();
}

fn rust_log_filter(value: Option<OsString>) -> Option<EnvFilter> {
    value.map(|value| {
        value
            .into_string()
            .ok()
            .and_then(|value| EnvFilter::try_new(value).ok())
            .unwrap_or_else(|| EnvFilter::new("warn"))
    })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::rust_log_filter;

    #[test]
    fn tracing_stays_disabled_without_rust_log() {
        assert!(rust_log_filter(None).is_none());
    }

    #[test]
    fn tracing_uses_requested_rust_log_filter() {
        let filter = rust_log_filter(Some(OsString::from("debug"))).unwrap();

        assert_eq!(filter.to_string(), "debug");
    }

    #[test]
    fn invalid_rust_log_keeps_the_previous_warn_fallback() {
        let filter = rust_log_filter(Some(OsString::from("[invalid"))).unwrap();

        assert_eq!(filter.to_string(), "warn");
    }
}
