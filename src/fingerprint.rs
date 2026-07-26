use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFingerprint {
    pub path: PathBuf,
    pub size: u64,
    pub mtime_ns: u128,
}

pub fn fingerprint_tracked_files(root: &Path, paths: &[PathBuf]) -> Result<Vec<FileFingerprint>> {
    let mut fingerprints = BTreeMap::new();

    for path in paths {
        let fingerprint = fingerprint_tracked(root, path)?;
        fingerprints.insert(fingerprint.path.clone(), fingerprint);
    }

    Ok(fingerprints.into_values().collect())
}

pub fn fingerprint_tracked(root: &Path, path: &Path) -> Result<FileFingerprint> {
    require_absolute_tracked_path(path)?;

    let metadata = fs::metadata(path).map_err(|source| Error::TrackedFile {
        path: path.to_path_buf(),
        source,
    })?;

    if !metadata.is_file() {
        return Err(Error::InvalidConfig(format!(
            "tracked path is not a file: {}",
            path.display()
        )));
    }

    fingerprint_from_metadata(state_path(root, path), &metadata)
}

fn fingerprint_from_metadata(path: PathBuf, metadata: &fs::Metadata) -> Result<FileFingerprint> {
    Ok(FileFingerprint {
        path,
        size: metadata.len(),
        mtime_ns: mtime_ns(metadata)?,
    })
}

fn state_path(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

fn require_absolute_tracked_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        return Err(Error::InvalidConfig(
            "tracked_files must not be empty".to_string(),
        ));
    }

    if !path.is_absolute() {
        return Err(Error::InvalidConfig(format!(
            "tracked_files must be absolute: {}",
            path.display()
        )));
    }

    Ok(())
}

pub fn executable_exists(path: &Path) -> Result<bool> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn mtime_ns(metadata: &fs::Metadata) -> Result<u128> {
    let modified = metadata.modified()?;
    let duration = modified.duration_since(UNIX_EPOCH).map_err(|error| {
        Error::InvalidState(format!(
            "file modification time is before Unix epoch: {error}"
        ))
    })?;

    Ok(duration.as_nanos())
}

#[cfg(test)]
mod tests {
    use super::fingerprint_tracked_files;
    use std::fs;

    use tempfile::tempdir;

    #[test]
    fn fingerprints_plain_tracked_files() {
        let dir = tempdir().unwrap();
        let aqua_config = dir.path().join("aqua.yaml");
        fs::write(&aqua_config, "registries: []").unwrap();

        let fingerprints = fingerprint_tracked_files(dir.path(), &[aqua_config]).unwrap();

        assert_eq!(fingerprints.len(), 1);
        assert_eq!(fingerprints[0].path, std::path::PathBuf::from("aqua.yaml"));
    }

    #[test]
    fn deduplicates_tracked_files() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("config.toml");
        fs::write(&config, "").unwrap();

        let fingerprints =
            fingerprint_tracked_files(dir.path(), &[config.clone(), config]).unwrap();

        assert_eq!(fingerprints.len(), 1);
        assert_eq!(
            fingerprints[0].path,
            std::path::PathBuf::from("config.toml")
        );
    }

    #[test]
    fn treats_glob_metacharacters_as_literal_path() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("config[1].toml");
        fs::write(&config, "").unwrap();

        let fingerprints = fingerprint_tracked_files(dir.path(), &[config]).unwrap();

        assert_eq!(fingerprints.len(), 1);
        assert_eq!(
            fingerprints[0].path,
            std::path::PathBuf::from("config[1].toml")
        );
    }

    #[test]
    fn does_not_expand_glob_patterns() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("config.toml"), "").unwrap();
        let pattern = dir.path().join("*.toml");

        let error =
            fingerprint_tracked_files(dir.path(), std::slice::from_ref(&pattern)).unwrap_err();

        assert!(matches!(error, crate::error::Error::TrackedFile { path, .. } if path == pattern));
    }
}
