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
    let config_path =
        cli.config
            .canonicalize()
            .map_err(|source| Error::BootstrapConfigInaccessible {
                path: cli.config,
                source,
            })?;
    let root = config_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let config = Config::read(&config_path)?;
    Bootstrapper::new(root, config).run().await
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .init();
}
