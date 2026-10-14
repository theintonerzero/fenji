//! The dummy password hash is ready soon after the server starts (#132's review, M1b), so the first
//! sign-in for an unknown username takes no longer than any other. Its own test binary: the hash is
//! process-wide, and another test in the same process could have made it first.
mod common;

use std::time::{Duration, Instant};

use common::*;
use fenji_server::server::GateLimits;

#[tokio::test]
async fn the_dummy_hash_is_made_at_start_not_at_the_first_unknown_username() {
    assert!(
        !fenji_auth::password::dummy_ready(),
        "nothing in this process made it yet"
    );
    let dir = tempfile::tempdir().unwrap();
    let (s, _c) = server(dir.path(), "", GateLimits::default()).await;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !fenji_auth::password::dummy_ready() {
        assert!(
            Instant::now() < deadline,
            "the dummy hash wasn't made at start"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    s.shutdown().await.unwrap();
}
