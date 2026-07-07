#[cfg(unix)]
use std::fs::File;

use std::fs::{self};
use std::io::Write;
use std::path::Path;

use tempfile::NamedTempFile;

use crate::error::Result;

pub fn write(dir: &Path, file_name: &str, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(dir)?;

    let mut temp = NamedTempFile::new_in(dir)?;
    temp.write_all(bytes)?;
    temp.as_file_mut().sync_all()?;

    let target = dir.join(file_name);
    temp.persist(&target).map_err(|error| error.error)?;

    sync_dir(dir)?;
    Ok(())
}

fn sync_dir(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(dir)?.sync_all()?;
    }

    #[cfg(not(unix))]
    {
        let _ = dir;
    }

    Ok(())
}
