use std::collections::HashMap;
use std::path::Path;

use crate::error::{Error, Result};
use crate::process;

pub fn install_args() -> Vec<String> {
    vec!["install".to_string(), "--all".to_string()]
}

pub fn exec_args_with_env(command: &[String], envs: &[(String, String)]) -> Result<Vec<String>> {
    let mut args = vec!["exec".to_string(), "--".to_string()];
    let mut env_lookup = None;
    for part in command {
        args.push(match substitute_env(part, envs, &mut env_lookup)? {
            Some(value) => value,
            None => part.clone(),
        });
    }
    Ok(args)
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
    let mut envs = aqua_envs(aqua, aqua_config, aqua_root);
    envs.push(("AQUA_DISABLE_LAZY_INSTALL".to_string(), "true".to_string()));
    let args = exec_args_with_env(command, &envs)?;
    process::run_app(name, aqua, &args, Some(&envs)).await
}

fn substitute_env(
    value: &str,
    envs: &[(String, String)],
    env_lookup: &mut Option<HashMap<String, String>>,
) -> Result<Option<String>> {
    let Some(mut start) = value.find("${") else {
        return Ok(None);
    };
    let mut output = String::with_capacity(value.len());
    let mut rest = value;

    loop {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find('}') else {
            return Err(Error::InvalidConfig(format!(
                "command contains unclosed environment substitution: {value}"
            )));
        };

        let name = &after_start[..end];
        require_env_name(name)?;
        let replacement = resolve_env(name, envs, env_lookup).ok_or_else(|| {
            Error::InvalidConfig(format!(
                "environment variable is not set for command substitution: {name}"
            ))
        })?;
        output.push_str(replacement);
        rest = &after_start[end + 1..];

        let Some(next_start) = rest.find("${") else {
            break;
        };
        start = next_start;
    }

    output.push_str(rest);
    Ok(Some(output))
}

fn require_env_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(Error::InvalidConfig(
            "environment substitution variable name must not be empty".to_string(),
        ));
    };

    if !(first == '_' || first.is_ascii_alphabetic())
        || chars.any(|ch| !(ch == '_' || ch.is_ascii_alphanumeric()))
    {
        return Err(Error::InvalidConfig(format!(
            "invalid environment substitution variable name: {name}"
        )));
    }

    Ok(())
}

fn resolve_env<'a>(
    name: &str,
    envs: &[(String, String)],
    env_lookup: &'a mut Option<HashMap<String, String>>,
) -> Option<&'a str> {
    let lookup = env_lookup.get_or_insert_with(|| {
        let mut lookup: HashMap<String, String> = std::env::vars().collect();
        lookup.extend(envs.iter().cloned());
        lookup
    });
    lookup.get(name).map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::{exec_args_with_env, install_args};

    #[test]
    fn install_args_install_all_configured_packages() {
        assert_eq!(install_args(), ["install", "--all"]);
    }

    #[test]
    fn exec_args_pass_command_after_separator() {
        assert_eq!(
            exec_args_with_env(
                &["uv".to_string(), "run".to_string(), "dv".to_string()],
                &[]
            )
            .unwrap(),
            ["exec", "--", "uv", "run", "dv"]
        );
    }

    #[test]
    fn exec_args_substitute_env_values() {
        let envs = vec![
            ("AQUA_EXE".to_string(), "C:/tools/aqua.exe".to_string()),
            ("AQUA_CONFIG".to_string(), "config/aqua.yaml".to_string()),
        ];

        assert_eq!(
            exec_args_with_env(
                &[
                    "${AQUA_EXE}".to_string(),
                    "--config=${AQUA_CONFIG}".to_string(),
                ],
                &envs,
            )
            .unwrap(),
            [
                "exec",
                "--",
                "C:/tools/aqua.exe",
                "--config=config/aqua.yaml"
            ]
        );
    }

    #[test]
    fn exec_args_reject_missing_env_values() {
        let error = exec_args_with_env(&["${MISSING_ENV}".to_string()], &[])
            .unwrap_err()
            .to_string();

        assert!(error.contains("environment variable is not set"));
        assert!(error.contains("MISSING_ENV"));
    }

    #[test]
    fn exec_args_reject_unclosed_env_substitution() {
        let error = exec_args_with_env(&["${AQUA_CONFIG".to_string()], &[])
            .unwrap_err()
            .to_string();

        assert!(error.contains("unclosed environment substitution"));
    }
}
