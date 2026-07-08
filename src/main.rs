mod aqua;
mod bootstrap;
mod config;
mod error;
mod fingerprint;
mod github;
mod lock;
mod process;
mod state;
mod util;

use std::fs;
use std::path::PathBuf;

use clap::Parser;
use tracing_subscriber::EnvFilter;

use crate::bootstrap::Bootstrapper;
use crate::config::Config;
use crate::error::{Error, Result};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[arg(short, long, default_value = "bootstrap.json")]
    config: PathBuf,

    #[arg(last = true)]
    app_args: Vec<String>,
}

#[tokio::main]
async fn main() {
    init_tracing();

    let exit_code = run().await.unwrap_or_else(|error| {
        eprintln!("bootstrap failed: {error}");
        1
    });

    std::process::exit(exit_code);
}

async fn run() -> Result<i32> {
    let cli = Cli::parse();
    let config_path = accessible_absolute_path(cli.config)?;
    let root = config_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let config = Config::read(&config_path)?;
    Bootstrapper::new(root, config, cli.app_args).run().await
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

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;
    use std::path::PathBuf;

    #[test]
    fn cli_collects_app_args_after_separator() {
        let cli = Cli::parse_from([
            "aqua-bootstrapper",
            "--config",
            "custom.json",
            "--",
            "status",
            "--verbose",
        ]);

        assert_eq!(cli.config, PathBuf::from("custom.json"));
        assert_eq!(cli.app_args, ["status", "--verbose"]);
    }
}
