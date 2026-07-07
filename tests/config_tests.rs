use std::fs;

use aqua_bootstrapper::config::Config;
use tempfile::tempdir;

#[test]
fn reads_valid_config() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("bootstrap.json");
    fs::write(
        &path,
        r#"{
            "schema": 1,
            "aqua_version": "v2.59.2",
            "aqua_config": "aqua.yaml",
            "aqua_root": ".dv/aqua",
            "bootstrap_cache": ".dv/bootstrap",
            "tracked_files": ["aqua.yaml"],
            "post_install": [{"name": "sync", "command": ["uv", "sync", "--locked"]}],
            "app": {"command": ["uv", "run", "dv"]}
        }"#,
    )
    .unwrap();

    let parsed = Config::read(&path).unwrap();

    assert_eq!(parsed.schema, 1);
    assert_eq!(parsed.aqua_version, "v2.59.2");
    assert_eq!(parsed.tracked_files.len(), 1);
}

#[test]
fn rejects_parent_dir_paths() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("bootstrap.json");
    fs::write(
        &path,
        r#"{
            "schema": 1,
            "aqua_version": "v2.59.2",
            "aqua_config": "../aqua.yaml",
            "aqua_root": ".dv/aqua",
            "bootstrap_cache": ".dv/bootstrap",
            "tracked_files": ["aqua.yaml"],
            "post_install": [],
            "app": {"command": ["uv", "run", "dv"]}
        }"#,
    )
    .unwrap();

    assert!(Config::read(&path).is_err());
}
