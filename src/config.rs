use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};

pub const CONFIG_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub schema: u32,
    pub aqua_version: String,
    pub aqua_config: PathBuf,
    pub aqua_root: PathBuf,
    pub bootstrap_cache: PathBuf,
    pub tracked_files: Vec<PathBuf>,
    #[serde(default)]
    pub post_install: Vec<NamedCommand>,
    pub app: AppCommand,
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
        let bytes = fs::read(path)?;
        let config: Config = serde_json::from_slice(&bytes)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != CONFIG_SCHEMA {
            return Err(Error::InvalidConfig(format!(
                "unsupported schema {}, expected {CONFIG_SCHEMA}",
                self.schema
            )));
        }

        require_non_empty("aqua_version", &self.aqua_version)?;
        require_relative_path("aqua_config", &self.aqua_config)?;
        require_relative_path("aqua_root", &self.aqua_root)?;
        require_relative_path("bootstrap_cache", &self.bootstrap_cache)?;

        if self.tracked_files.is_empty() {
            return Err(Error::InvalidConfig(
                "tracked_files must contain at least one file".to_string(),
            ));
        }

        for path in &self.tracked_files {
            require_relative_path("tracked_files", path)?;
        }

        for command in &self.post_install {
            require_non_empty("post_install.name", &command.name)?;
            require_command("post_install.command", &command.command)?;
        }

        require_command("app.command", &self.app.command)?;
        Ok(())
    }
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

fn require_relative_path(field: &str, path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        return Err(Error::InvalidConfig(format!("{field} must not be empty")));
    }

    if path.is_absolute() {
        return Err(Error::InvalidConfig(format!(
            "{field} must be relative: {}",
            path.display()
        )));
    }

    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::Prefix(_)
        )
    }) {
        return Err(Error::InvalidConfig(format!(
            "{field} must not contain parent directory components: {}",
            path.display()
        )));
    }

    Ok(())
}
