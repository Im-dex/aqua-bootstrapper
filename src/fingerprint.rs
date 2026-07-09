use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use glob::{Pattern, glob};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFingerprint {
    pub path: PathBuf,
    pub size: u64,
    pub mtime_ns: u128,
}

pub fn fingerprint_tracked_patterns(
    root: &Path,
    patterns: &[PathBuf],
) -> Result<Vec<FileFingerprint>> {
    let mut fingerprints = BTreeMap::new();

    for pattern in patterns {
        if is_glob_pattern(pattern) {
            expand_glob_pattern(root, pattern, &mut fingerprints)?;
        } else {
            let fingerprint = fingerprint_tracked(root, pattern)?;
            fingerprints.insert(fingerprint.path.clone(), fingerprint);
        }
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

fn expand_glob_pattern(
    root: &Path,
    pattern: &Path,
    fingerprints: &mut BTreeMap<PathBuf, FileFingerprint>,
) -> Result<()> {
    require_absolute_tracked_path(pattern)?;

    let pattern_str = glob_pattern(root, pattern)?;

    let mut matched_files = 0;
    for entry in glob(&pattern_str).map_err(|error| {
        Error::InvalidConfig(format!("invalid tracked_files glob pattern: {error}"))
    })? {
        let absolute = entry.map_err(|error| {
            Error::InvalidConfig(format!(
                "tracked_files glob pattern failed for {}: {error}",
                pattern.display()
            ))
        })?;
        let state_path = state_path(root, &absolute);
        if fingerprints.contains_key(&state_path) {
            matched_files += 1;
            continue;
        }

        let metadata = fs::metadata(&absolute).map_err(|source| Error::TrackedFile {
            path: state_path.clone(),
            source,
        })?;

        if !metadata.is_file() {
            continue;
        }

        let fingerprint = fingerprint_from_metadata(state_path, &metadata)?;
        fingerprints.insert(fingerprint.path.clone(), fingerprint);
        matched_files += 1;
    }

    if matched_files == 0 {
        return Err(Error::InvalidConfig(format!(
            "tracked_files glob pattern matched no files: {}",
            pattern.display()
        )));
    }

    Ok(())
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

fn glob_pattern(root: &Path, pattern: &Path) -> Result<String> {
    if let Ok(relative) = pattern.strip_prefix(root) {
        let root = root.to_str().ok_or_else(|| {
            Error::InvalidConfig("tracked_files root path is not valid UTF-8".to_string())
        })?;
        let relative = relative.to_str().ok_or_else(|| {
            Error::InvalidConfig(format!(
                "tracked_files glob pattern is not valid UTF-8: {}",
                pattern.display()
            ))
        })?;

        if relative.is_empty() {
            return Ok(Pattern::escape(root));
        }

        return Ok(format!(
            "{}{}{}",
            Pattern::escape(root),
            std::path::MAIN_SEPARATOR,
            relative
        ));
    }

    let pattern = pattern.to_str().ok_or_else(|| {
        Error::InvalidConfig(format!(
            "tracked_files glob pattern is not valid UTF-8: {}",
            pattern.display()
        ))
    })?;

    Ok(pattern.to_string())
}

fn is_glob_pattern(path: &Path) -> bool {
    path.to_string_lossy()
        .chars()
        .any(|character| matches!(character, '*' | '?' | '['))
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
    use super::fingerprint_tracked_patterns;
    use std::fs;

    use tempfile::tempdir;

    #[test]
    fn fingerprints_plain_tracked_files() {
        let dir = tempdir().unwrap();
        let aqua_config = dir.path().join("aqua.yaml");
        fs::write(&aqua_config, "registries: []").unwrap();

        let fingerprints = fingerprint_tracked_patterns(dir.path(), &[aqua_config]).unwrap();

        assert_eq!(fingerprints.len(), 1);
        assert_eq!(fingerprints[0].path, std::path::PathBuf::from("aqua.yaml"));
    }

    #[test]
    fn expands_glob_patterns_recursively() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("config/nested")).unwrap();
        fs::write(dir.path().join("config/a.toml"), "").unwrap();
        fs::write(dir.path().join("config/nested/b.toml"), "").unwrap();
        fs::write(dir.path().join("config/nested/ignored.txt"), "").unwrap();

        let fingerprints =
            fingerprint_tracked_patterns(dir.path(), &[dir.path().join("config/**/*.toml")])
                .unwrap();
        let paths = fingerprints
            .into_iter()
            .map(|fingerprint| fingerprint.path)
            .collect::<Vec<_>>();

        assert_eq!(
            paths,
            vec![
                std::path::PathBuf::from("config/a.toml"),
                std::path::PathBuf::from("config/nested/b.toml"),
            ]
        );
    }

    #[test]
    fn deduplicates_files_matched_by_multiple_patterns() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("config")).unwrap();
        fs::write(dir.path().join("config/a.toml"), "").unwrap();

        let fingerprints = fingerprint_tracked_patterns(
            dir.path(),
            &[
                dir.path().join("config/a.toml"),
                dir.path().join("config/**/*.toml"),
            ],
        )
        .unwrap();

        assert_eq!(fingerprints.len(), 1);
        assert_eq!(
            fingerprints[0].path,
            std::path::PathBuf::from("config/a.toml")
        );
    }

    #[test]
    fn rejects_empty_glob_patterns() {
        let dir = tempdir().unwrap();

        let error =
            fingerprint_tracked_patterns(dir.path(), &[dir.path().join("missing/**/*.toml")])
                .unwrap_err();

        assert!(error.to_string().contains("matched no files"));
    }

    #[test]
    fn treats_glob_characters_in_root_as_literals() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("[project]");
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(root.join("config/a.toml"), "").unwrap();

        let fingerprints =
            fingerprint_tracked_patterns(&root, &[root.join("config/**/*.toml")]).unwrap();

        assert_eq!(fingerprints.len(), 1);
        assert_eq!(
            fingerprints[0].path,
            std::path::PathBuf::from("config/a.toml")
        );
    }
}
