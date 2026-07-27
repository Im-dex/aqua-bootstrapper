use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::process;

pub fn install_args() -> Vec<String> {
    vec!["install".to_string(), "--all".to_string()]
}

pub fn exec_args(command: &[String]) -> Vec<String> {
    let mut args = vec!["exec".to_string(), "--".to_string()];
    args.extend(command.iter().cloned());
    args
}

pub fn post_install_args(command: &[String]) -> Vec<String> {
    match command.split_first() {
        Some((executable, args)) if executable == "aqua" => args.to_vec(),
        _ => exec_args(command),
    }
}

pub fn which_args(tool: &str) -> Vec<String> {
    vec!["which".to_string(), tool.to_string()]
}

pub fn aqua_envs(aqua: &Path, aqua_config: &Path, aqua_root: &Path) -> Vec<(String, String)> {
    vec![
        ("AQUA_EXE".to_string(), aqua.display().to_string()),
        ("AQUA_ROOT_DIR".to_string(), aqua_root.display().to_string()),
        ("AQUA_CONFIG".to_string(), aqua_config.display().to_string()),
    ]
}

pub async fn run_install(aqua: &Path, aqua_config: &Path, aqua_root: &Path) -> Result<()> {
    let args = install_args();
    let mut envs = aqua_envs(aqua, aqua_config, aqua_root);
    envs.extend([
        ("AQUA_PROGRESS_BAR".to_string(), "true".to_string()),
        ("AQUA_DISABLE_POLICY".to_string(), "true".to_string()),
    ]);
    process::run_foreground("aqua install", aqua, &args, Some(&envs))
        .await
        .map(|_| ())
}

pub async fn resolve_tool(
    aqua: &Path,
    aqua_config: &Path,
    aqua_root: &Path,
    tool: &str,
) -> Result<PathBuf> {
    let args = which_args(tool);
    let name = format!("aqua which {tool}");
    let output = process::run_capture_stdout(
        &name,
        aqua,
        &args,
        Some(&aqua_envs(aqua, aqua_config, aqua_root)),
    )
    .await?;
    let path = PathBuf::from(output.trim());

    if path.as_os_str().is_empty() || !path.is_absolute() {
        return Err(Error::CommandOutput {
            name,
            reason: format!("expected an absolute path, got: {}", output.trim()),
        });
    }

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::{exec_args, install_args, post_install_args, which_args};

    #[test]
    fn install_args_install_all_configured_packages() {
        assert_eq!(install_args(), ["install", "--all"]);
    }

    #[test]
    fn exec_args_pass_command_after_separator() {
        assert_eq!(
            exec_args(&["uv".to_string(), "sync".to_string()]),
            ["exec", "--", "uv", "sync"]
        );
    }

    #[test]
    fn post_install_args_pass_managed_tool_through_aqua_exec() {
        assert_eq!(
            post_install_args(&["uv".to_string(), "sync".to_string()]),
            ["exec", "--", "uv", "sync"]
        );
    }

    #[test]
    fn post_install_args_run_aqua_directly() {
        assert_eq!(
            post_install_args(&["aqua".to_string(), "-v".to_string()]),
            ["-v"]
        );
    }

    #[test]
    fn which_args_look_up_one_tool() {
        assert_eq!(which_args("node"), ["which", "node"]);
    }
}
