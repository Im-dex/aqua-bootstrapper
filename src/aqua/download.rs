use std::path::{Path, PathBuf};

use reqwest::Client;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::aqua::{OWNER, REPO};
use crate::error::{Error, Result};

pub async fn download_release_asset(
    client: &Client,
    version: &str,
    asset_name: &str,
    download_dir: &Path,
) -> Result<PathBuf> {
    fs::create_dir_all(download_dir).await?;

    let url = format!("https://github.com/{OWNER}/{REPO}/releases/download/{version}/{asset_name}");
    let response = client
        .get(&url)
        .header("User-Agent", "aqua-bootstrapper")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(Error::Download(format!(
            "GET {url} returned {}",
            response.status()
        )));
    }

    let target = download_dir.join(asset_name);
    let temp = target.with_extension("download");
    let mut file = fs::File::create(&temp).await?;
    let bytes = response.bytes().await?;
    file.write_all(&bytes).await?;
    file.sync_all().await?;
    drop(file);
    fs::rename(&temp, &target).await?;

    Ok(target)
}
