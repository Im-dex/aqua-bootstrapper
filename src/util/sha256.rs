use std::fs::File;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::Result;
use crate::util::progress::Progress;

pub fn file_hex_with_progress(
    path: &Path,
    label: impl Into<String>,
    complete_message: impl AsRef<str>,
) -> Result<String> {
    let mut file = File::open(path)?;
    let mut progress = Progress::new(label, Some(file.metadata()?.len()));
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        progress.advance(read as u64);
    }

    progress.finish(complete_message);

    Ok(hex::encode(hasher.finalize()))
}
