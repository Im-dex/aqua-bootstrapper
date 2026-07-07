pub mod archive;
pub mod download;
pub mod exec;
pub mod install;
pub mod platform;

use std::path::{Path, PathBuf};

pub const OWNER: &str = "aquaproj";
pub const REPO: &str = "aqua";

pub fn executable_path(root: &Path) -> PathBuf {
    root.join("bin").join(platform::executable_name())
}
