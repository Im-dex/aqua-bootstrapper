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
    #[serde(default)]
    pub aqua_sha256: String,
    pub aqua_executable: PathBuf,
    pub tracked_files: Vec<FileFingerprint>,
    #[serde(default = "default_post_install_completed")]
    pub post_install_completed: bool,
    #[serde(default)]
    pub bootstrapped_tools: BTreeMap<String, BootstrappedTool>,
    #[serde(default)]
    pub app_tool: Option<BootstrappedTool>,
}

impl BootstrapState {
    pub fn new(
        aqua_version: String,
        aqua_sha256: String,
        aqua_executable: PathBuf,
        tracked_files: Vec<FileFingerprint>,
        post_install_completed: bool,
        bootstrapped_tools: BTreeMap<String, BootstrappedTool>,
        app_tool: Option<BootstrappedTool>,
    ) -> Self {
        Self {
            schema: STATE_SCHEMA,
            aqua_version,
            aqua_sha256,
            aqua_executable,
            tracked_files,
            post_install_completed,
            bootstrapped_tools,
            app_tool,
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

#[cfg(test)]
mod tests {
    use super::{BootstrapState, read, write_atomic};
    use crate::fingerprint::FileFingerprint;
    use std::collections::BTreeMap;

    use tempfile::tempdir;

    #[test]
    fn state_round_trip_is_atomic_visible() {
        let dir = tempdir().unwrap();
        let state = BootstrapState::new(
            "v2.59.2".to_string(),
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
            ".dv/aqua/bin/aqua".into(),
            vec![FileFingerprint {
                path: "aqua.yaml".into(),
                size: 42,
                mtime_ns: 7,
            }],
            false,
            BTreeMap::new(),
            None,
        );

        write_atomic(dir.path(), &state).unwrap();
        let read_back = read(dir.path()).unwrap().unwrap();

        assert_eq!(read_back, state);
        assert!(!read_back.post_install_completed);
    }

    #[test]
    fn legacy_state_without_post_install_completed_is_complete() {
        let state: BootstrapState = serde_json::from_str(
            r#"{
              "schema": 1,
              "aqua_version": "v2.59.2",
              "aqua_executable": ".dv/aqua/bin/aqua",
              "tracked_files": []
            }"#,
        )
        .unwrap();

        assert!(state.post_install_completed);
        assert!(state.bootstrapped_tools.is_empty());
        assert!(state.app_tool.is_none());
    }
}
