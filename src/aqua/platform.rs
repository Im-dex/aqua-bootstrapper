use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AquaAsset {
    pub name: String,
}

pub fn asset(_version: &str) -> Result<AquaAsset> {
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "windows",
        other => return Err(Error::UnsupportedPlatform(other.to_string())),
    };

    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => return Err(Error::UnsupportedPlatform(other.to_string())),
    };

    let extension = if cfg!(windows) { "zip" } else { "tar.gz" };

    Ok(AquaAsset {
        name: format!("aqua_{os}_{arch}.{extension}"),
    })
}

pub fn executable_name() -> &'static str {
    if cfg!(windows) { "aqua.exe" } else { "aqua" }
}
