use std::ffi::OsStr;
use std::fs::{self, File};
use std::path::Path;

#[cfg(not(windows))]
use flate2::read::GzDecoder;
#[cfg(windows)]
use zip::ZipArchive;

use crate::error::{Error, Result};
use crate::util::fs::ensure_clean_dir;

pub fn extract_aqua(archive: &Path, root: &Path) -> Result<()> {
    let staging = root.with_extension("staging");
    ensure_clean_dir(&staging)?;

    extract_archive(archive, &staging)?;

    let bin = root.join("bin");
    fs::create_dir_all(&bin)?;

    let source = find_executable(&staging)?;
    let target = bin.join(crate::aqua::platform::executable_name());
    fs::copy(&source, &target)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&target)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&target, permissions)?;
    }

    fs::remove_dir_all(&staging)?;
    Ok(())
}

#[cfg(not(windows))]
fn extract_archive(archive: &Path, destination: &Path) -> Result<()> {
    extract_tar_gz(archive, destination)
}

#[cfg(windows)]
fn extract_archive(archive: &Path, destination: &Path) -> Result<()> {
    extract_zip(archive, destination)
}

#[cfg(not(windows))]
fn extract_tar_gz(archive: &Path, destination: &Path) -> Result<()> {
    let file = File::open(archive)?;
    let gz = GzDecoder::new(file);
    let mut tar = tar::Archive::new(gz);
    tar.unpack(destination)
        .map_err(|error| Error::Archive(error.to_string()))
}

#[cfg(windows)]
fn extract_zip(archive: &Path, destination: &Path) -> Result<()> {
    let file = File::open(archive)?;
    let mut archive = ZipArchive::new(file)?;
    archive.extract(destination)?;
    Ok(())
}

fn find_executable(root: &Path) -> Result<std::path::PathBuf> {
    let expected = crate::aqua::platform::executable_name();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;

            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() && path.file_name() == Some(OsStr::new(expected)) {
                return Ok(path);
            }
        }
    }

    Err(Error::Archive(format!(
        "executable {expected} was not found in extracted archive"
    )))
}
