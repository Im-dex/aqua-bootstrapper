use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("bootstrap config is not accessible: {path}: {source}")]
    BootstrapConfigInaccessible {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),

    #[error("tracked file is not accessible: {path}")]
    TrackedFile {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("state is invalid: {0}")]
    InvalidState(String),

    #[error("download failed: {0}")]
    Download(String),

    #[error("Aqua release asset checksum mismatch for {asset}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        asset: String,
        expected: String,
        actual: String,
    },

    #[error("archive extraction failed: {0}")]
    Archive(String),

    #[error("command failed: {name} exited with {code}")]
    CommandFailed { name: String, code: i32 },

    #[error("command was terminated by signal: {name}")]
    CommandTerminated { name: String },

    #[error("command returned invalid output: {name}: {reason}")]
    CommandOutput { name: String, reason: String },

    #[error("application executable is not accessible after post-install: {path}: {source}")]
    ApplicationExecutableInaccessible {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("application executable is not a regular file after post-install: {path}")]
    ApplicationExecutableNotRegularFile { path: PathBuf },

    #[cfg(unix)]
    #[error("application executable does not have an executable permission bit: {path}")]
    ApplicationExecutableNotExecutable { path: PathBuf },

    #[error("process containment setup failed during {operation}: {source}")]
    ProcessContainment {
        operation: &'static str,
        source: std::io::Error,
    },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("task join error: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[cfg(windows)]
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
}

pub type Result<T> = std::result::Result<T, Error>;
