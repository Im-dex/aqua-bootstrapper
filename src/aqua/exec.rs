use std::path::Path;

use crate::error::Result;
use crate::process;

pub fn install_args() -> Vec<String> {
    vec!["install".to_string()]
}

pub fn exec_args(command: &[String]) -> Vec<String> {
    let mut args = vec!["exec".to_string(), "--".to_string()];
    args.extend(command.iter().cloned());
    args
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

pub async fn run_exec(
    name: &str,
    aqua: &Path,
    aqua_root: &Path,
    aqua_config: &Path,
    command: &[String],
) -> Result<i32> {
    let args = exec_args(command);
    let mut envs = aqua_envs(aqua, aqua_config, aqua_root);
    envs.push(("AQUA_DISABLE_LAZY_INSTALL".to_string(), "true".to_string()));
    process::run_app(name, aqua, &args, Some(&envs)).await
}
