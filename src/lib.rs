mod aqua;
mod bootstrap;
mod config;
mod error;
mod fingerprint;
mod lock;
mod process;
mod state;
mod util;

use std::fs;
use std::path::PathBuf;

use tracing_subscriber::EnvFilter;

use crate::bootstrap::Bootstrapper;
use crate::config::Config;

pub use crate::error::{Error, Result};

pub async fn run(config_path: PathBuf, app_args: Vec<String>) -> Result<i32> {
    init_tracing();

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
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .init();
}
