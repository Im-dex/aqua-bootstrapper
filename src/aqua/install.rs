use std::path::{Path, PathBuf};

use reqwest::Client;
use tracing::info;

use crate::aqua::archive::extract_aqua;
use crate::aqua::download::download_release_asset;
use crate::aqua::platform;
use crate::error::Result;
use crate::github::attestation;
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
        move || sha256::file_hex(&archive)
    })
    .await??;

    info!("verifying Aqua GitHub attestation for {}", asset.name);
    attestation::verify_aqua_release_asset(&archive, version, &digest).await?;

    let root = aqua_root.to_path_buf();
    let archive_for_extract = archive.clone();
    tokio::task::spawn_blocking(move || extract_aqua(&archive_for_extract, asset.kind, &root))
        .await??;

    Ok(crate::aqua::executable_path(aqua_root))
}
