use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use minijinja::{Environment, UndefinedBehavior};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const CONFIG_SCHEMA: u32 = 3;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub schema: u32,
    pub aqua: AquaConfig,
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
    pub config: PathBuf,
    pub root: PathBuf,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum AppExecutable {
    Aqua { name: String },
    Path { path: PathBuf },
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppCommand {
    #[serde(default)]
    pub executable: Option<AppExecutable>,
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
        require_absolute_path("aqua.config", &self.aqua.config)?;
        require_absolute_path("aqua.root", &self.aqua.root)?;
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

        match &self.app.executable {
            None => require_command("app.command", &self.app.command)?,
            Some(executable) => {
                require_arguments("app.command", &self.app.command)?;
                match executable {
                    AppExecutable::Aqua { name } => {
                        require_non_empty("app.executable.name", name)?;
                    }
                    AppExecutable::Path { path } => {
                        require_absolute_path("app.executable.path", path)?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn parse_config(raw: &str, envs: &HashMap<String, String>) -> Result<Config> {
    let raw = render_template(raw, envs)?;
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
    if command.is_empty() {
        return Err(Error::InvalidConfig(format!(
            "{field} must contain non-empty arguments"
        )));
    }
    require_arguments(field, command)
}

fn require_arguments(field: &str, arguments: &[String]) -> Result<()> {
    if arguments.iter().any(|part| part.trim().is_empty()) {
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

fn render_template(value: &str, envs: &HashMap<String, String>) -> Result<String> {
    let mut environment = Environment::new();
    environment.set_undefined_behavior(UndefinedBehavior::Strict);

    let template_env = envs
        .iter()
        .map(|(name, value)| (name.as_str(), json_string_fragment(value)))
        .collect::<BTreeMap<_, _>>();
    environment.add_global("env", template_env);
    environment.add_global("os", std::env::consts::OS);

    let template = environment.template_from_str(value).map_err(|error| {
        Error::InvalidConfig(format!("failed to parse config template: {error}"))
    })?;

    template
        .render(())
        .map_err(|error| Error::InvalidConfig(format!("failed to render config template: {error}")))
}

fn json_string_fragment(value: &str) -> String {
    let encoded = serde_json::to_string(value).expect("serializing a string cannot fail");
    encoded[1..encoded.len() - 1].to_string()
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

    let path_text = path.as_os_str().to_string_lossy();
    if path_text
        .split(|character| character == '/' || (cfg!(windows) && character == '\\'))
        .any(|component| component == "." || component == "..")
    {
        return Err(Error::InvalidConfig(format!(
            "{field} must not contain `.` or `..` path components: {}",
            path.display()
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AppCommand, AppExecutable, Config, parse_config, render_template};
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
                "schema": 3,
                "aqua": {
                    "version": "v2.59.2",
                    "sha": {
                        "windows": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                        "linux": "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
                    },
                    "config": "{{ env.PROJECT_ROOT }}/aqua.yaml",
                    "root": "{{ env.PROJECT_ROOT }}/.dv/aqua"
                },
                "bootstrap_cache": "{{ env.PROJECT_ROOT }}/.dv/bootstrap",
                "tracked_files": ["{{ env.PROJECT_ROOT }}/aqua.yaml", "{{ env.PROJECT_ROOT }}/config/**/*.toml"],
                "post_install": [{"name": "sync", "command": ["uv", "sync", "--locked"]}],
                "bootstrapped_tools": {"NODE_EXE": "node"},
                "app": {"command": ["uv", "run", "dv"]}
            }"#,
            &envs,
        )
        .unwrap();

        assert_eq!(config.aqua.version, "v2.59.2");
        assert_eq!(
            config.aqua.config,
            std::path::PathBuf::from(format!("{project_root}/aqua.yaml"))
        );
        assert_eq!(
            config.aqua.root,
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
        assert_eq!(config.app.executable, None);
    }

    #[test]
    fn parses_path_app_executable_selector() {
        let executable = if cfg!(windows) {
            Path::new("C:/work/project/.venv/Scripts/dv.exe")
        } else {
            Path::new("/work/project/.venv/bin/dv")
        };
        let app: AppCommand = serde_json::from_value(json!({
            "executable": {
                "source": "path",
                "path": executable,
            },
            "command": ["status"],
        }))
        .unwrap();

        assert_eq!(
            app.executable,
            Some(AppExecutable::Path {
                path: executable.to_path_buf(),
            })
        );
        assert_eq!(app.command, ["status"]);
    }

    #[test]
    fn parses_aqua_app_executable_selector() {
        let app: AppCommand = serde_json::from_value(json!({
            "executable": {
                "source": "aqua",
                "name": "uv",
            },
            "command": ["run", "dv"],
        }))
        .unwrap();

        assert_eq!(
            app.executable,
            Some(AppExecutable::Aqua {
                name: "uv".to_string(),
            })
        );
        assert_eq!(app.command, ["run", "dv"]);
    }

    #[test]
    fn renders_environment_variables_in_config_template() {
        let envs = HashMap::from([("PROJECT_ROOT".to_string(), "C:\\work\\project".to_string())]);

        let config = render_template(
            r#"{"aqua":{"config":"{{ env.PROJECT_ROOT }}/aqua.yaml"}}"#,
            &envs,
        )
        .unwrap();

        assert_eq!(
            config,
            r#"{"aqua":{"config":"C:\\work\\project/aqua.yaml"}}"#
        );
    }

    #[test]
    fn renders_platform_conditional_in_config_template() {
        let rendered = render_template(
            r#"{"name":"{% if os == 'windows' %}windows{% else %}other{% endif %}"}"#,
            &HashMap::new(),
        )
        .unwrap();

        assert_eq!(
            rendered,
            if cfg!(windows) {
                r#"{"name":"windows"}"#
            } else {
                r#"{"name":"other"}"#
            }
        );
    }

    #[test]
    fn rejects_missing_config_template_values() {
        let error = render_template(
            r#"{"aqua":{"config":"{{ env.MISSING_ENV }}/aqua.yaml"}}"#,
            &HashMap::new(),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("failed to render config template"));
    }

    #[test]
    fn rejects_invalid_config_template() {
        let error = render_template("{{ env.PROJECT_ROOT", &HashMap::new())
            .unwrap_err()
            .to_string();

        assert!(error.contains("failed to parse config template"));
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
                "schema": 3,
                "aqua": {{
                    "version": "v2.59.2",
                    "sha": {{
                        "windows": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                        "linux": "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
                    }},
                    "config": "{project_root}/aqua.yaml",
                    "root": "{project_root}/.dv/aqua"
                }},
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
                "schema": 3,
                "aqua": {{
                    "version": "v2.59.2",
                    "sha": {{"windows": "not-a-digest", "linux": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}},
                    "config": "{project_root}/aqua.yaml",
                    "root": "{project_root}/.dv/aqua"
                }},
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
                "schema": 3,
                "aqua": aqua_config(&dir.path().join("aqua.yaml"), &dir.path().join(".dv/aqua")),
                "bootstrap_cache": json_path(&dir.path().join(".dv/bootstrap")),
                "tracked_files": [json_path(&dir.path().join("aqua.yaml"))],
                "post_install": [{"name": "sync", "command": ["uv", "sync", "--locked"]}],
                "app": {"command": ["uv", "run", "dv"]}
            })
            .to_string(),
        )
        .unwrap();

        let parsed = Config::read(&path).unwrap();

        assert_eq!(parsed.schema, 3);
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
                "schema": 3,
                "aqua": aqua_config(Path::new("aqua.yaml"), Path::new("/tmp/project/.dv/aqua")),
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
    fn rejects_relative_app_executable_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bootstrap.json");
        fs::write(
            &path,
            json!({
                "schema": 3,
                "aqua": aqua_config(
                    &dir.path().join("aqua.yaml"),
                    &dir.path().join(".dv/aqua"),
                ),
                "bootstrap_cache": dir.path().join(".dv/bootstrap"),
                "tracked_files": [dir.path().join("aqua.yaml")],
                "app": {
                    "executable": {
                        "source": "path",
                        "path": ".venv/bin/dv",
                    },
                    "command": [],
                }
            })
            .to_string(),
        )
        .unwrap();

        let error = Config::read(&path).unwrap_err().to_string();

        assert!(error.contains("app.executable.path must be absolute"));
    }

    #[test]
    fn rejects_current_dir_component_in_app_executable_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bootstrap.json");
        fs::write(
            &path,
            json!({
                "schema": 3,
                "aqua": aqua_config(
                    &dir.path().join("aqua.yaml"),
                    &dir.path().join(".dv/aqua"),
                ),
                "bootstrap_cache": dir.path().join(".dv/bootstrap"),
                "tracked_files": [dir.path().join("aqua.yaml")],
                "app": {
                    "executable": {
                        "source": "path",
                        "path": dir.path().join(".").join("dv"),
                    },
                    "command": [],
                }
            })
            .to_string(),
        )
        .unwrap();

        let error = Config::read(&path).unwrap_err().to_string();

        assert!(error.contains("must not contain `.` or `..` path components"));
    }

    #[test]
    fn rejects_legacy_schema_two_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bootstrap.json");
        fs::write(
            &path,
            json!({
                "schema": 2,
                "aqua": aqua_config(&dir.path().join("aqua.yaml"), &dir.path().join(".dv/aqua")),
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
                "schema": 3,
                "aqua": aqua_config(&dir.path().join("..").join("aqua.yaml"), &dir.path().join(".dv/aqua")),
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

    fn aqua_config(config: &Path, root: &Path) -> serde_json::Value {
        json!({
            "version": "v2.59.2",
            "sha": {
                "windows": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "linux": "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
            },
            "config": json_path(config),
            "root": json_path(root)
        })
    }
}
