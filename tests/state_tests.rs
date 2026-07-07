use aqua_bootstrapper::fingerprint::FileFingerprint;
use aqua_bootstrapper::state::{self, BootstrapState};
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
    );

    state::write_atomic(dir.path(), &state).unwrap();
    let read_back = state::read(dir.path()).unwrap().unwrap();

    assert_eq!(read_back, state);
}
