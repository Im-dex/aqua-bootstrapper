use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

use crate::error::{Error, Result};
use crate::fingerprint::FileFingerprint;
use crate::util::atomic;

pub const STATE_SCHEMA: u32 = 2;
pub const STATE_FILE: &str = "state.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootstrappedTool {
    pub tool: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ResolvedAppExecutable {
    Aqua { name: String, path: PathBuf },
    Path { path: PathBuf },
}

impl ResolvedAppExecutable {
    pub fn path(&self) -> &Path {
        match self {
            Self::Aqua { path, .. } | Self::Path { path } => path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapState {
    pub schema: u32,
    pub aqua_version: String,
    pub aqua_sha256: String,
    pub aqua_executable: PathBuf,
    pub tracked_files: Vec<FileFingerprint>,
    pub post_install_completed: bool,
    pub bootstrapped_tools: BTreeMap<String, BootstrappedTool>,
    #[serde(deserialize_with = "deserialize_required_option")]
    pub resolved_app_executable: Option<ResolvedAppExecutable>,
}

fn deserialize_required_option<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

impl BootstrapState {
    pub fn new(
        aqua_version: String,
        aqua_sha256: String,
        aqua_executable: PathBuf,
        tracked_files: Vec<FileFingerprint>,
        post_install_completed: bool,
        bootstrapped_tools: BTreeMap<String, BootstrappedTool>,
        resolved_app_executable: Option<ResolvedAppExecutable>,
    ) -> Self {
        Self {
            schema: STATE_SCHEMA,
            aqua_version,
            aqua_sha256,
            aqua_executable,
            tracked_files,
            post_install_completed,
            bootstrapped_tools,
            resolved_app_executable,
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
    use super::{BootstrapState, ResolvedAppExecutable, read, write_atomic};
    use crate::fingerprint::FileFingerprint;
    use std::collections::BTreeMap;

    use serde_json::{Value, json};
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
            Some(ResolvedAppExecutable::Aqua {
                name: "aqua".to_string(),
                path: ".dv/aqua/bin/aqua".into(),
            }),
        );

        write_atomic(dir.path(), &state).unwrap();
        let read_back = read(dir.path()).unwrap().unwrap();

        assert_eq!(read_back, state);
        assert!(!read_back.post_install_completed);
    }

    #[test]
    fn rejects_legacy_state_schema() {
        let mut state: BootstrapState = serde_json::from_value(current_state_json()).unwrap();
        state.schema = 1;

        let error = state.validate().unwrap_err();

        assert!(
            error
                .to_string()
                .contains("unsupported schema 1, expected 2")
        );
    }

    #[test]
    fn rejects_state_missing_current_fields() {
        for field in [
            "aqua_sha256",
            "post_install_completed",
            "bootstrapped_tools",
            "resolved_app_executable",
        ] {
            let mut state = current_state_json();
            state.as_object_mut().unwrap().remove(field);

            let error = serde_json::from_value::<BootstrapState>(state).unwrap_err();

            assert!(
                error
                    .to_string()
                    .contains(&format!("missing field `{field}`")),
                "unexpected error for {field}: {error}"
            );
        }
    }

    #[test]
    fn rejects_legacy_state_fields() {
        let mut state = current_state_json();
        state.as_object_mut().unwrap().insert(
            "app_tool".to_string(),
            json!({"tool": "uv", "path": ".dv/aqua/bin/uv"}),
        );

        let error = serde_json::from_value::<BootstrapState>(state).unwrap_err();

        assert!(error.to_string().contains("unknown field `app_tool`"));
    }

    fn current_state_json() -> Value {
        json!({
            "schema": 2,
            "aqua_version": "v2.59.2",
            "aqua_sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "aqua_executable": ".dv/aqua/bin/aqua",
            "tracked_files": [],
            "post_install_completed": false,
            "bootstrapped_tools": {},
            "resolved_app_executable": null,
        })
    }
}
