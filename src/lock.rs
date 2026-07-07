use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::error::Result;

pub const LOCK_FILE: &str = "bootstrap.lock";

pub struct BootstrapLock {
    file: File,
    path: PathBuf,
}

impl BootstrapLock {
    pub fn acquire(cache: &Path) -> Result<Self> {
        fs::create_dir_all(cache)?;
        let path = cache.join(LOCK_FILE);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        file.lock()?;
        Ok(Self { file, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for BootstrapLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
