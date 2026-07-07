use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    TarGz,
    Zip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AquaAsset {
    pub name: String,
    pub kind: ArchiveKind,
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

    let kind = if os == "windows" {
        ArchiveKind::Zip
    } else {
        ArchiveKind::TarGz
    };
    let extension = match kind {
        ArchiveKind::TarGz => "tar.gz",
        ArchiveKind::Zip => "zip",
    };

    Ok(AquaAsset {
        name: format!("aqua_{os}_{arch}.{extension}"),
        kind,
    })
}

pub fn executable_name() -> &'static str {
    if cfg!(windows) { "aqua.exe" } else { "aqua" }
}
