//! Where the stub writes when `--out` isn't given.
use fenji_stub::default_out_dir;

#[test]
fn the_default_is_re_stub_logs_in_the_checkout_above() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    let sub = root.path().join("tools/stub");
    std::fs::create_dir_all(&sub).unwrap();
    let want = root.path().join("re").join("stub-logs");
    assert_eq!(
        default_out_dir(&sub),
        Some(want.clone()),
        "from a subdirectory"
    );
    assert_eq!(default_out_dir(root.path()), Some(want), "from the root");
}

#[test]
fn there_is_no_default_outside_a_checkout() {
    let outside = tempfile::tempdir().unwrap();
    assert_eq!(default_out_dir(outside.path()), None);
}
