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

#[test]
fn a_worktree_with_a_git_file_counts_as_a_checkout() {
    // A git worktree has a `.git` file, not a directory.
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".git"), "gitdir: elsewhere\n").unwrap();
    let sub = root.path().join("tools");
    std::fs::create_dir_all(&sub).unwrap();
    assert_eq!(
        default_out_dir(&sub),
        Some(root.path().join("re").join("stub-logs"))
    );
}

#[test]
fn without_out_or_a_checkout_the_stub_refuses_to_start() {
    let outside = tempfile::tempdir().unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_fenji-stub"))
        .current_dir(outside.path())
        .args(["--listen", "127.0.0.1:0"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() > deadline {
            child.kill().unwrap();
            panic!("the stub started listening outside a checkout");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let mut err = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
    assert!(!status.success());
    assert!(err.contains("pass --out"), "{err}");
    assert_eq!(
        std::fs::read_dir(outside.path()).unwrap().count(),
        0,
        "nothing written"
    );
}
