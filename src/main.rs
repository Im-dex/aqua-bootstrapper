use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<CliCommand>,

    #[arg(short, long, default_value = "bootstrap.json")]
    config: PathBuf,

    #[arg(last = true)]
    app_args: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    /// Run a command with a Linux parent-death signal.
    Pdeathsig {
        /// PID of the process that is launching this wrapper.
        #[arg(long, value_name = "PID")]
        parent_pid: u32,

        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<OsString>,
    },
}

fn main() {
    let cli = Cli::parse();
    let exit_code = match cli.command {
        Some(CliCommand::Pdeathsig {
            parent_pid,
            command,
        }) => aqua_bootstrapper::run_with_parent_death_signal(parent_pid, &command).unwrap_or_else(
            |error| {
                eprintln!("pdeathsig failed: {error}");
                1
            },
        ),
        None => run_bootstrap(cli.config, cli.app_args),
    };

    std::process::exit(exit_code);
}

#[tokio::main]
async fn run_bootstrap(config: PathBuf, app_args: Vec<String>) -> i32 {
    aqua_bootstrapper::run(config, app_args)
        .await
        .unwrap_or_else(|error| {
            eprintln!("bootstrap failed: {error}");
            1
        })
}

#[cfg(test)]
mod tests {
    use super::{Cli, CliCommand};
    use clap::Parser;
    use std::ffi::OsString;
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
        assert!(cli.command.is_none());
    }

    #[test]
    fn cli_collects_pdeathsig_command_without_parsing_its_arguments() {
        let cli = Cli::parse_from([
            "aqua-bootstrapper",
            "pdeathsig",
            "--parent-pid",
            "123",
            "--",
            "command",
            "--option",
            "value",
        ]);

        assert!(matches!(
            cli.command,
            Some(CliCommand::Pdeathsig {
                parent_pid: 123,
                command,
            })
                if command
                    == [
                        OsString::from("command"),
                        OsString::from("--option"),
                        OsString::from("value"),
                    ]
        ));
    }
}
