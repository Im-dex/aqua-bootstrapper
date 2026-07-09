use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::fingerprint::FileFingerprint;
use crate::util::atomic;

pub const STATE_SCHEMA: u32 = 1;
pub const STATE_FILE: &str = "state.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootstrappedTool {
    pub tool: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootstrapState {
    pub schema: u32,
    pub aqua_version: String,
    pub aqua_executable: PathBuf,
    pub tracked_files: Vec<FileFingerprint>,
    #[serde(default = "default_post_install_completed")]
    pub post_install_completed: bool,
    #[serde(default)]
    pub bootstrapped_tools: BTreeMap<String, BootstrappedTool>,
}

impl BootstrapState {
    pub fn new(
        aqua_version: String,
        aqua_executable: PathBuf,
        tracked_files: Vec<FileFingerprint>,
        post_install_completed: bool,
        bootstrapped_tools: BTreeMap<String, BootstrappedTool>,
    ) -> Self {
        Self {
            schema: STATE_SCHEMA,
            aqua_version,
            aqua_executable,
            tracked_files,
            post_install_completed,
            bootstrapped_tools,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != STATE_SCHEMA {
            return Err(Error::InvalidState(format!(
                "unsupported schema {}, expected {STATE_SCHEMA}",
                self.schema
            )));
        }
        Ok(())
    }
}

fn default_post_install_completed() -> bool {
    true
}

pub fn state_path(cache: &Path) -> PathBuf {
    cache.join(STATE_FILE)
}

pub fn read(cache: &Path) -> Result<Option<BootstrapState>> {
    let path = state_path(cache);
    match fs::read(&path) {
        Ok(bytes) => {
            let state: BootstrapState = serde_json::from_slice(&bytes)?;
            state.validate()?;
            Ok(Some(state))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn write_atomic(cache: &Path, state: &BootstrapState) -> Result<()> {
    fs::create_dir_all(cache)?;
    let bytes = serde_json::to_vec_pretty(state)?;
    atomic::write(cache, STATE_FILE, &bytes)
}
