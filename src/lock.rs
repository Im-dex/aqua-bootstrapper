use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::error::Result;

pub const LOCK_FILE: &str = "bootstrap.lock";

pub struct BootstrapLock {
    file: File,
    path: PathBuf,
}

impl BootstrapLock {
    pub fn acquire_exclusive(cache: &Path) -> Result<Self> {
        Self::acquire(cache, LockMode::Exclusive)
    }

    pub fn acquire_shared(cache: &Path) -> Result<Self> {
        Self::acquire(cache, LockMode::Shared)
    }

    fn acquire(cache: &Path, mode: LockMode) -> Result<Self> {
        fs::create_dir_all(cache)?;
        let path = cache.join(LOCK_FILE);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        match mode {
            LockMode::Exclusive => file.lock()?,
            LockMode::Shared => file.lock_shared()?,
        }
        Ok(Self { file, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug, Clone, Copy)]
enum LockMode {
    Exclusive,
    Shared,
}

impl Drop for BootstrapLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
