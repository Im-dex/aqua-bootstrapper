use std::fs;
use std::path::Path;

use aqua_bootstrapper::config::Config;
use serde_json::json;
use tempfile::tempdir;

#[test]
fn reads_valid_config() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("bootstrap.json");
    fs::write(
        &path,
        json!({
            "schema": 1,
            "aqua_version": "v2.59.2",
            "aqua_config": json_path(&dir.path().join("aqua.yaml")),
            "aqua_root": json_path(&dir.path().join(".dv/aqua")),
            "bootstrap_cache": json_path(&dir.path().join(".dv/bootstrap")),
            "tracked_files": [json_path(&dir.path().join("aqua.yaml"))],
            "post_install": [{"name": "sync", "command": ["uv", "sync", "--locked"]}],
            "app": {"command": ["uv", "run", "dv"]}
        })
        .to_string(),
    )
    .unwrap();

    let parsed = Config::read(&path).unwrap();

    assert_eq!(parsed.schema, 1);
    assert_eq!(parsed.aqua_version, "v2.59.2");
    assert_eq!(parsed.tracked_files.len(), 1);
}

#[test]
fn rejects_relative_paths() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("bootstrap.json");
    fs::write(
        &path,
        r#"{
            "schema": 1,
            "aqua_version": "v2.59.2",
            "aqua_config": "aqua.yaml",
            "aqua_root": "/tmp/project/.dv/aqua",
            "bootstrap_cache": "/tmp/project/.dv/bootstrap",
            "tracked_files": ["/tmp/project/aqua.yaml"],
            "post_install": [],
            "app": {"command": ["uv", "run", "dv"]}
        }"#,
    )
    .unwrap();

    assert!(Config::read(&path).is_err());
}

#[test]
fn rejects_parent_dir_paths() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("bootstrap.json");
    fs::write(
        &path,
        json!({
            "schema": 1,
            "aqua_version": "v2.59.2",
            "aqua_config": json_path(&dir.path().join("..").join("aqua.yaml")),
            "aqua_root": json_path(&dir.path().join(".dv/aqua")),
            "bootstrap_cache": json_path(&dir.path().join(".dv/bootstrap")),
            "tracked_files": [json_path(&dir.path().join("aqua.yaml"))],
            "post_install": [],
            "app": {"command": ["uv", "run", "dv"]}
        })
        .to_string(),
    )
    .unwrap();

    assert!(Config::read(&path).is_err());
}

#[test]
fn read_error_includes_config_path() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("missing-bootstrap.json");

    let error = Config::read(&path).unwrap_err().to_string();

    assert!(error.contains("bootstrap config is not accessible"));
    assert!(error.contains(&path.display().to_string()));
}

fn json_path(path: &Path) -> String {
    path.display().to_string()
}
