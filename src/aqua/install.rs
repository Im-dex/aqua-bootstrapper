use std::path::{Path, PathBuf};

use reqwest::Client;

use crate::aqua::archive::extract_aqua;
use crate::aqua::download::download_release_asset;
use crate::aqua::platform;
use crate::error::Result;
use crate::util::progress;
use crate::util::sha256;

pub async fn ensure_installed(
    client: &Client,
    version: &str,
    expected_sha256: &str,
    aqua_root: &Path,
    cache: &Path,
) -> Result<PathBuf> {
    let asset = platform::asset(version)?;
    let download_dir = cache.join("downloads").join(version);
    let archive = download_release_asset(client, version, &asset.name, &download_dir).await?;

    let digest = tokio::task::spawn_blocking({
        let archive = archive.clone();
        let asset_name = asset.name.clone();
        move || {
            sha256::file_hex_with_progress(
                &archive,
                format!("Computing SHA-256 for Aqua release asset {asset_name}"),
                format!("Computed SHA-256 for Aqua release asset {asset_name}"),
            )
        }
    })
    .await??;

    verify_checksum(&asset.name, expected_sha256, &digest)?;
    progress::step(format!(
        "Verified SHA-256 for Aqua release asset {}",
        asset.name
    ));

    let root = aqua_root.to_path_buf();
    let archive_for_extract = archive.clone();
    progress::step(format!("Extracting Aqua to {}...", aqua_root.display()));
    tokio::task::spawn_blocking(move || extract_aqua(&archive_for_extract, &root)).await??;
    let executable = crate::aqua::executable_path(aqua_root);
    progress::step(format!("Aqua is ready at {}", executable.display()));

    Ok(executable)
}

fn verify_checksum(asset: &str, expected: &str, actual: &str) -> Result<()> {
    if expected.eq_ignore_ascii_case(actual) {
        return Ok(());
    }

    Err(crate::error::Error::ChecksumMismatch {
        asset: asset.to_string(),
        expected: expected.to_string(),
        actual: actual.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::verify_checksum;

    const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn accepts_matching_checksum_case_insensitively() {
        assert!(verify_checksum("aqua.zip", &DIGEST.to_uppercase(), DIGEST).is_ok());
    }

    #[test]
    fn rejects_checksum_mismatch() {
        let error = verify_checksum("aqua.zip", DIGEST, "different").unwrap_err();

        assert!(error.to_string().contains("checksum mismatch"));
    }
}
