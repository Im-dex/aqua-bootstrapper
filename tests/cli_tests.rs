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

#[cfg(target_os = "linux")]
#[test]
fn pdeathsig_executes_command_and_preserves_its_exit_code() {
    let status = Command::new(env!("CARGO_BIN_EXE_aqua-bootstrapper"))
        .args(["pdeathsig", "--parent-pid"])
        .arg(std::process::id().to_string())
        .args(["--", "/bin/sh", "-c", "exit 23"])
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(23));
}

#[cfg(target_os = "linux")]
#[test]
fn pdeathsig_does_not_exec_after_expected_parent_is_gone() {
    let output = Command::new(env!("CARGO_BIN_EXE_aqua-bootstrapper"))
        .args([
            "pdeathsig",
            "--parent-pid",
            "1",
            "--",
            "/bin/sh",
            "-c",
            "echo payload-executed",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}
