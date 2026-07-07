use std::fs;
use std::path::Path;

use crate::error::Result;

pub fn ensure_clean_dir(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::create_dir_all(path)?;
    Ok(())
}
