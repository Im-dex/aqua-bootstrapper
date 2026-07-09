use std::path::PathBuf;

use clap::Parser;

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
    let cli = Cli::parse();
    let exit_code = aqua_bootstrapper::run(cli.config, cli.app_args)
        .await
        .unwrap_or_else(|error| {
            eprintln!("bootstrap failed: {error}");
            1
        });

    std::process::exit(exit_code);
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
