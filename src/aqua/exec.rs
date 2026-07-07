use std::path::Path;

use crate::error::Result;
use crate::process;

pub fn install_args(config: &Path, root: &Path) -> Vec<String> {
    vec![
        "--config".to_string(),
        config.display().to_string(),
        "--root-dir".to_string(),
        root.display().to_string(),
        "install".to_string(),
    ]
}

pub fn exec_args(root: &Path, command: &[String]) -> Vec<String> {
    let mut args = vec![
        "--root-dir".to_string(),
        root.display().to_string(),
        "exec".to_string(),
        "--".to_string(),
    ];
    args.extend(command.iter().cloned());
    args
}

pub async fn run_install(aqua: &Path, aqua_config: &Path, aqua_root: &Path) -> Result<()> {
    let args = install_args(aqua_config, aqua_root);
    process::run_foreground("aqua install", aqua, &args)
        .await
        .map(|_| ())
}

pub async fn run_exec(
    name: &str,
    aqua: &Path,
    aqua_root: &Path,
    command: &[String],
) -> Result<i32> {
    let args = exec_args(aqua_root, command);
    process::run_app(name, aqua, &args).await
}
