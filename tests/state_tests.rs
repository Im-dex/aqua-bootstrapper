use aqua_bootstrapper::fingerprint::FileFingerprint;
use aqua_bootstrapper::state::{self, BootstrapState};
use std::collections::BTreeMap;
use tempfile::tempdir;

#[test]
fn state_round_trip_is_atomic_visible() {
    let dir = tempdir().unwrap();
    let state = BootstrapState::new(
        "v2.59.2".to_string(),
        ".dv/aqua/bin/aqua".into(),
        vec![FileFingerprint {
            path: "aqua.yaml".into(),
            size: 42,
            mtime_ns: 7,
        }],
        false,
        BTreeMap::new(),
    );

    state::write_atomic(dir.path(), &state).unwrap();
    let read_back = state::read(dir.path()).unwrap().unwrap();

    assert_eq!(read_back, state);
    assert!(!read_back.post_install_completed);
}

#[test]
fn legacy_state_without_post_install_completed_is_complete() {
    let state: BootstrapState = serde_json::from_str(
        r#"{
          "schema": 1,
          "aqua_version": "v2.59.2",
          "aqua_executable": ".dv/aqua/bin/aqua",
          "tracked_files": []
        }"#,
    )
    .unwrap();

    assert!(state.post_install_completed);
    assert!(state.bootstrapped_tools.is_empty());
}
