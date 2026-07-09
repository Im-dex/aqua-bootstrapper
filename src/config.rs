use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};

pub const CONFIG_SCHEMA: u32 = 2;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub schema: u32,
    pub aqua: AquaConfig,
    pub aqua_config: PathBuf,
    pub aqua_root: PathBuf,
    pub bootstrap_cache: PathBuf,
    pub tracked_files: Vec<PathBuf>,
    #[serde(default)]
    pub post_install: Vec<NamedCommand>,
    #[serde(default)]
    pub bootstrapped_tools: BTreeMap<String, String>,
    pub app: AppCommand,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AquaConfig {
    pub version: String,
    pub sha: AquaSha,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AquaSha {
    pub windows: String,
    pub linux: String,
}

impl AquaSha {
    pub fn for_current_platform(&self) -> Result<&str> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("windows", "x86_64") => Ok(&self.windows),
            ("linux", "x86_64") => Ok(&self.linux),
            (os, arch) => Err(Error::UnsupportedPlatform(format!("{os}/{arch}"))),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct NamedCommand {
    pub name: String,
    pub command: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppCommand {
    pub command: Vec<String>,
}

impl Config {
    pub fn read(path: &Path) -> Result<Self> {
        let raw =
            fs::read_to_string(path).map_err(|source| Error::BootstrapConfigInaccessible {
                path: path.to_path_buf(),
                source,
            })?;
        let envs = std::env::vars().collect();
        parse_config(&raw, &envs)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != CONFIG_SCHEMA {
            return Err(Error::InvalidConfig(format!(
                "unsupported schema {}, expected {CONFIG_SCHEMA}",
                self.schema
            )));
        }

        require_non_empty("aqua.version", &self.aqua.version)?;
        require_sha256("aqua.sha.windows", &self.aqua.sha.windows)?;
        require_sha256("aqua.sha.linux", &self.aqua.sha.linux)?;
        require_absolute_path("aqua_config", &self.aqua_config)?;
        require_absolute_path("aqua_root", &self.aqua_root)?;
        require_absolute_path("bootstrap_cache", &self.bootstrap_cache)?;

        if self.tracked_files.is_empty() {
            return Err(Error::InvalidConfig(
                "tracked_files must contain at least one file".to_string(),
            ));
        }

        for path in &self.tracked_files {
            require_absolute_path("tracked_files", path)?;
        }

        for command in &self.post_install {
            require_non_empty("post_install.name", &command.name)?;
            require_command("post_install.command", &command.command)?;
        }

        for (env_name, tool) in &self.bootstrapped_tools {
            require_bootstrapped_tool_env_name(env_name)?;
            require_non_empty(&format!("bootstrapped_tools.{env_name}"), tool)?;
        }

        require_command("app.command", &self.app.command)?;
        Ok(())
    }
}

fn parse_config(raw: &str, envs: &HashMap<String, String>) -> Result<Config> {
    let raw = substitute_env(raw, envs)?;
    let config: Config = serde_json::from_str(&raw)?;
    config.validate()?;
    Ok(config)
}

fn require_non_empty(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidConfig(format!("{field} must not be empty")));
    }
    Ok(())
}

fn require_command(field: &str, command: &[String]) -> Result<()> {
    if command.is_empty() || command.iter().any(|part| part.trim().is_empty()) {
        return Err(Error::InvalidConfig(format!(
            "{field} must contain non-empty arguments"
        )));
    }
    Ok(())
}

fn require_sha256(field: &str, value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::InvalidConfig(format!(
            "{field} must be a 64-character SHA-256 hex digest"
        )));
    }
    Ok(())
}

fn substitute_env<'a>(value: &'a str, envs: &HashMap<String, String>) -> Result<Cow<'a, str>> {
    let Some(mut start) = value.find("${") else {
        return Ok(Cow::Borrowed(value));
    };
    let mut output = String::with_capacity(value.len());
    let mut rest = value;

    loop {
        output.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find('}') else {
            return Err(Error::InvalidConfig(
                "config contains unclosed environment substitution".to_string(),
            ));
        };

        let name = &after_start[..end];
        require_env_name(name)?;
        let replacement = envs.get(name).ok_or_else(|| {
            Error::InvalidConfig(format!(
                "environment variable is not set for config substitution: {name}"
            ))
        })?;
        output.push_str(&json_string_fragment(replacement));
        rest = &after_start[end + 1..];

        let Some(next_start) = rest.find("${") else {
            break;
        };
        start = next_start;
    }

    output.push_str(rest);
    Ok(Cow::Owned(output))
}

fn json_string_fragment(value: &str) -> String {
    let encoded = serde_json::to_string(value).expect("serializing a string cannot fail");
    encoded[1..encoded.len() - 1].to_string()
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

fn require_bootstrapped_tool_env_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte == b'_')
    {
        return Err(Error::InvalidConfig(format!(
            "bootstrapped_tools key must contain only uppercase ASCII letters and underscores: {name}"
        )));
    }

    Ok(())
}

fn require_absolute_path(field: &str, path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        return Err(Error::InvalidConfig(format!("{field} must not be empty")));
    }

    if !path.is_absolute() {
        return Err(Error::InvalidConfig(format!(
            "{field} must be absolute: {}",
            path.display()
        )));
    }

    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(Error::InvalidConfig(format!(
            "{field} must not contain parent directory components: {}",
            path.display()
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Config, parse_config, substitute_env};
    use std::collections::HashMap;
    use std::fs;
    use std::path::Path;

    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn parses_config_after_env_substitution() {
        let project_root = if cfg!(windows) {
            "C:/work/project"
        } else {
            "/work/project"
        };
        let envs = HashMap::from([("PROJECT_ROOT".to_string(), project_root.to_string())]);

        let config = parse_config(
            r#"{
                "schema": 2,
                "aqua": {
                    "version": "v2.59.2",
                    "sha": {
                        "windows": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                        "linux": "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
                    }
                },
                "aqua_config": "${PROJECT_ROOT}/aqua.yaml",
                "aqua_root": "${PROJECT_ROOT}/.dv/aqua",
                "bootstrap_cache": "${PROJECT_ROOT}/.dv/bootstrap",
                "tracked_files": ["${PROJECT_ROOT}/aqua.yaml", "${PROJECT_ROOT}/config/**/*.toml"],
                "post_install": [{"name": "sync", "command": ["uv", "sync", "--locked"]}],
                "bootstrapped_tools": {"NODE_EXE": "node"},
                "app": {"command": ["uv", "run", "dv"]}
            }"#,
            &envs,
        )
        .unwrap();

        assert_eq!(config.aqua.version, "v2.59.2");
        assert_eq!(
            config.aqua_config,
            std::path::PathBuf::from(format!("{project_root}/aqua.yaml"))
        );
        assert_eq!(
            config.aqua_root,
            std::path::PathBuf::from(format!("{project_root}/.dv/aqua"))
        );
        assert_eq!(
            config.bootstrap_cache,
            std::path::PathBuf::from(format!("{project_root}/.dv/bootstrap"))
        );
        assert_eq!(
            config.tracked_files,
            [
                std::path::PathBuf::from(format!("{project_root}/aqua.yaml")),
                std::path::PathBuf::from(format!("{project_root}/config/**/*.toml")),
            ]
        );
        assert_eq!(
            config.bootstrapped_tools,
            [("NODE_EXE".to_string(), "node".to_string())].into()
        );
    }

    #[test]
    fn substitutes_env_in_config_text() {
        let envs = HashMap::from([("PROJECT_ROOT".to_string(), "C:\\work\\project".to_string())]);

        let config =
            substitute_env(r#"{"aqua_config":"${PROJECT_ROOT}/aqua.yaml"}"#, &envs).unwrap();

        assert_eq!(config, r#"{"aqua_config":"C:\\work\\project/aqua.yaml"}"#);
    }

    #[test]
    fn rejects_missing_config_env_values() {
        let error = substitute_env(
            r#"{"aqua_config":"${MISSING_ENV}/aqua.yaml"}"#,
            &HashMap::new(),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("environment variable is not set"));
        assert!(error.contains("MISSING_ENV"));
    }

    #[test]
    fn rejects_unclosed_config_env_substitution() {
        let error = substitute_env("${PROJECT_ROOT", &HashMap::new())
            .unwrap_err()
            .to_string();

        assert!(error.contains("unclosed environment substitution"));
    }

    #[test]
    fn rejects_invalid_bootstrapped_tool_env_name() {
        let project_root = if cfg!(windows) {
            "C:/work/project"
        } else {
            "/work/project"
        };
        let config = format!(
            r#"{{
                "schema": 2,
                "aqua": {{
                    "version": "v2.59.2",
                    "sha": {{
                        "windows": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                        "linux": "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
                    }}
                }},
                "aqua_config": "{project_root}/aqua.yaml",
                "aqua_root": "{project_root}/.dv/aqua",
                "bootstrap_cache": "{project_root}/.dv/bootstrap",
                "tracked_files": ["{project_root}/aqua.yaml"],
                "bootstrapped_tools": {{"node_exe": "node"}},
                "app": {{"command": ["uv", "run", "dv"]}}
            }}"#,
        );
        let error = parse_config(&config, &HashMap::new())
            .unwrap_err()
            .to_string();

        assert!(error.contains("bootstrapped_tools key"));
    }

    #[test]
    fn rejects_invalid_aqua_sha256() {
        let project_root = if cfg!(windows) {
            "C:/work/project"
        } else {
            "/work/project"
        };
        let config = format!(
            r#"{{
                "schema": 2,
                "aqua": {{
                    "version": "v2.59.2",
                    "sha": {{"windows": "not-a-digest", "linux": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}}
                }},
                "aqua_config": "{project_root}/aqua.yaml",
                "aqua_root": "{project_root}/.dv/aqua",
                "bootstrap_cache": "{project_root}/.dv/bootstrap",
                "tracked_files": ["{project_root}/aqua.yaml"],
                "app": {{"command": ["uv", "run", "dv"]}}
            }}"#,
        );

        let error = parse_config(&config, &HashMap::new())
            .unwrap_err()
            .to_string();

        assert!(error.contains("aqua.sha.windows"));
    }

    #[test]
    fn reads_valid_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bootstrap.json");
        fs::write(
            &path,
            json!({
                "schema": 2,
                "aqua": aqua_config(),
                "aqua_config": json_path(&dir.path().join("aqua.yaml")),
                "aqua_root": json_path(&dir.path().join(".dv/aqua")),
                "bootstrap_cache": json_path(&dir.path().join(".dv/bootstrap")),
                "tracked_files": [json_path(&dir.path().join("aqua.yaml"))],
                "post_install": [{"name": "sync", "command": ["uv", "sync", "--locked"]}],
                "app": {"command": ["uv", "run", "dv"]}
            })
            .to_string(),
        )
        .unwrap();

        let parsed = Config::read(&path).unwrap();

        assert_eq!(parsed.schema, 2);
        assert_eq!(parsed.aqua.version, "v2.59.2");
        assert_eq!(parsed.tracked_files.len(), 1);
    }

    #[test]
    fn rejects_relative_paths() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bootstrap.json");
        fs::write(
            &path,
            json!({
                "schema": 2,
                "aqua": aqua_config(),
                "aqua_config": "aqua.yaml",
                "aqua_root": "/tmp/project/.dv/aqua",
                "bootstrap_cache": "/tmp/project/.dv/bootstrap",
                "tracked_files": ["/tmp/project/aqua.yaml"],
                "post_install": [],
                "app": {"command": ["uv", "run", "dv"]}
            })
            .to_string(),
        )
        .unwrap();

        assert!(Config::read(&path).is_err());
    }

    #[test]
    fn rejects_legacy_schema_one_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bootstrap.json");
        fs::write(
            &path,
            json!({
                "schema": 1,
                "aqua_version": "v2.59.2",
                "aqua_config": json_path(&dir.path().join("aqua.yaml")),
                "aqua_root": json_path(&dir.path().join(".dv/aqua")),
                "bootstrap_cache": json_path(&dir.path().join(".dv/bootstrap")),
                "tracked_files": [json_path(&dir.path().join("aqua.yaml"))],
                "post_install": [],
                "app": {"command": ["uv", "run", "dv"]}
            })
            .to_string(),
        )
        .unwrap();

        assert!(Config::read(&path).is_err());
    }

    #[test]
    fn rejects_parent_dir_paths() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bootstrap.json");
        fs::write(
            &path,
            json!({
                "schema": 2,
                "aqua": aqua_config(),
                "aqua_config": json_path(&dir.path().join("..").join("aqua.yaml")),
                "aqua_root": json_path(&dir.path().join(".dv/aqua")),
                "bootstrap_cache": json_path(&dir.path().join(".dv/bootstrap")),
                "tracked_files": [json_path(&dir.path().join("aqua.yaml"))],
                "post_install": [],
                "app": {"command": ["uv", "run", "dv"]}
            })
            .to_string(),
        )
        .unwrap();

        assert!(Config::read(&path).is_err());
    }

    #[test]
    fn read_error_includes_config_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("missing-bootstrap.json");

        let error = Config::read(&path).unwrap_err().to_string();

        assert!(error.contains("bootstrap config is not accessible"));
        assert!(error.contains(&path.display().to_string()));
    }

    fn json_path(path: &Path) -> String {
        path.display().to_string()
    }

    fn aqua_config() -> serde_json::Value {
        json!({
            "version": "v2.59.2",
            "sha": {
                "windows": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "linux": "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
            }
        })
    }
}
