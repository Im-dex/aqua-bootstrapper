use std::process::Command;

use tempfile::tempdir;

#[test]
fn missing_config_path_reports_clear_error() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("missing-bootstrap.json");

    let output = Command::new(env!("CARGO_BIN_EXE_aqua-bootstrapper"))
        .arg("--config")
        .arg(&path)
        .output()
        .unwrap();

    assert!(!output.status.success());

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("bootstrap failed: bootstrap config is not accessible"));
    assert!(stderr.contains(&path.display().to_string()));
}
