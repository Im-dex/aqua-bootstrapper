use std::fs;

use aqua_bootstrapper::fingerprint::fingerprint_tracked_patterns;
use tempfile::tempdir;

#[test]
fn fingerprints_plain_tracked_files() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("aqua.yaml"), "registries: []").unwrap();

    let fingerprints = fingerprint_tracked_patterns(dir.path(), &["aqua.yaml".into()]).unwrap();

    assert_eq!(fingerprints.len(), 1);
    assert_eq!(fingerprints[0].path, std::path::PathBuf::from("aqua.yaml"));
}

#[test]
fn expands_glob_patterns_recursively() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("config/nested")).unwrap();
    fs::write(dir.path().join("config/a.toml"), "").unwrap();
    fs::write(dir.path().join("config/nested/b.toml"), "").unwrap();
    fs::write(dir.path().join("config/nested/ignored.txt"), "").unwrap();

    let fingerprints =
        fingerprint_tracked_patterns(dir.path(), &["config/**/*.toml".into()]).unwrap();
    let paths = fingerprints
        .into_iter()
        .map(|fingerprint| fingerprint.path)
        .collect::<Vec<_>>();

    assert_eq!(
        paths,
        vec![
            std::path::PathBuf::from("config/a.toml"),
            std::path::PathBuf::from("config/nested/b.toml"),
        ]
    );
}

#[test]
fn deduplicates_files_matched_by_multiple_patterns() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join("config")).unwrap();
    fs::write(dir.path().join("config/a.toml"), "").unwrap();

    let fingerprints = fingerprint_tracked_patterns(
        dir.path(),
        &["config/a.toml".into(), "config/**/*.toml".into()],
    )
    .unwrap();

    assert_eq!(fingerprints.len(), 1);
    assert_eq!(
        fingerprints[0].path,
        std::path::PathBuf::from("config/a.toml")
    );
}

#[test]
fn rejects_empty_glob_patterns() {
    let dir = tempdir().unwrap();

    let error =
        fingerprint_tracked_patterns(dir.path(), &["missing/**/*.toml".into()]).unwrap_err();

    assert!(error.to_string().contains("matched no files"));
}

#[test]
fn treats_glob_characters_in_root_as_literals() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("[project]");
    fs::create_dir_all(root.join("config")).unwrap();
    fs::write(root.join("config/a.toml"), "").unwrap();

    let fingerprints = fingerprint_tracked_patterns(&root, &["config/**/*.toml".into()]).unwrap();

    assert_eq!(fingerprints.len(), 1);
    assert_eq!(
        fingerprints[0].path,
        std::path::PathBuf::from("config/a.toml")
    );
}
