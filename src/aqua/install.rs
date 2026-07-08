use std::path::{Path, PathBuf};

use reqwest::Client;

use crate::aqua::archive::extract_aqua;
use crate::aqua::download::download_release_asset;
use crate::aqua::platform;
use crate::error::Result;
use crate::github::attestation;
use crate::util::progress;
use crate::util::sha256;

pub async fn ensure_installed(
    client: &Client,
    version: &str,
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

    progress::step(format!(
        "Verifying Aqua GitHub attestation for {}...",
        asset.name
    ));
    attestation::verify_aqua_release_asset(&archive, version, &digest).await?;
    progress::step(format!(
        "Verified Aqua GitHub attestation for {}",
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
