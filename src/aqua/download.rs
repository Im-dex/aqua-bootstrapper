use std::path::{Path, PathBuf};

use reqwest::Client;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::aqua::{OWNER, REPO};
use crate::error::{Error, Result};
use crate::util::progress::{self, Progress};

pub async fn download_release_asset(
    client: &Client,
    version: &str,
    asset_name: &str,
    download_dir: &Path,
) -> Result<PathBuf> {
    fs::create_dir_all(download_dir).await?;

    let url = format!("https://github.com/{OWNER}/{REPO}/releases/download/{version}/{asset_name}");
    let mut response = client
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
    let mut progress = Progress::new(
        format!("Downloading Aqua release asset {asset_name}"),
        response.content_length(),
    );
    let mut downloaded = 0;

    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        progress.advance(chunk.len() as u64);
    }

    file.sync_all().await?;
    drop(file);
    fs::rename(&temp, &target).await?;
    progress.finish(format!(
        "Downloaded Aqua release asset {asset_name} ({})",
        progress::format_bytes(downloaded)
    ));

    Ok(target)
}
