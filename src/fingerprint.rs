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

pub fn fingerprint_tracked(root: &Path, path: &Path) -> Result<FileFingerprint> {
    let absolute = root.join(path);
    let metadata = fs::metadata(&absolute).map_err(|source| Error::TrackedFile {
        path: path.to_path_buf(),
        source,
    })?;

    if !metadata.is_file() {
        return Err(Error::InvalidConfig(format!(
            "tracked path is not a file: {}",
            path.display()
        )));
    }

    Ok(FileFingerprint {
        path: path.to_path_buf(),
        size: metadata.len(),
        mtime_ns: mtime_ns(&metadata)?,
    })
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
