// Lifecycle family — file/cap creation + close tests.
//
// 3 tests: bad-cmode open + Consensus fs_close fd-release + Consensus
// append-open rejection.  See parent module `fs_wal_spec::tests` for
// shared setup (create_runtime, create_leader_and_follower, rand).
//
// Wave-3 S3.14 (2026-09-10) — split out of `fs_wal_spec.rs` via
// `#[path]` submodule.

use super::*;

/// A bad cmode arg to fs_open must reject the open AND not
/// populate any FileHandle — subsequent writes fail with
/// FSERR_CLOSED (unknown fd) and no WAL entry is produced.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bad_cmode_open_produces_no_handle_no_wal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.bin");
    std::fs::write(&path, b"").unwrap();
    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`), openCh in {{
              fsOpen!("{root}", "f.bin", "r+", "BOGUS", *openCh) |
              for (@r <- openCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    runtime
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            rand(),
        )
        .await
        .unwrap();
    assert!(runtime.fs_handles.wal.is_empty());
}

/// Phase 2 regression pin (fd-release fix, 2026-09-01):
/// **follower's Consensus fs_close is_replay branch MUST release
/// the shadow's real OS fd** — pre-fix, the branch produced the
/// cached reply without calling `handles.remove(fd)`, so the
/// shadow's `File` wrapper (installed by fs_open's Phase-2
/// real-open) stayed alive until runtime drop.  A validator
/// processing many blocks with Consensus fs traffic would
/// accumulate OS fds up to `MAX_OPEN_FDS = 1024` and then hit
/// `FSERR_QUOTA_EXCEEDED` on the next fs_open replay.
///
/// Direct probe: snapshot the leader's next_fd watermark before
/// and after the deploy to bound the allocated-fd range, then
/// scan the follower's fd table via `raw_fd` for each fd in
/// that range and assert `None`.  A regression that removed the
/// `handles.remove` call from fs_close's is_replay branch would
/// leave the follower's shadow alive at the allocated fd →
/// `raw_fd` returns `Some` → assertion fires.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_close_replay_releases_follower_shadow_fd() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data.bin"), b"phase-2-close-release-pin").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    // Term: open + close on a Consensus cap.  Under Phase 2, the
    // follower's fs_open replay installs a real File-backed
    // shadow; the fs_close replay must remove it.
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, closeCh
            in {{
              fsOpen!("{root}", "data.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsClose!(fd, *closeCh) |
                for (@_ <- closeCh) {{ Nil }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[33; 32]);

    // Snapshot the leader's next_fd watermark to bound the range
    // the deploy will allocate into.  fs_open advances the
    // atomic each time it inserts; the delta before → after is
    // the count of fds Rholang allocated in this run.
    let leader_fd_lo = leader.fs_handles.snapshot_next_fd();
    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fd-release setup");
    let leader_fd_hi = leader.fs_handles.snapshot_next_fd();
    assert!(
        leader_fd_hi > leader_fd_lo,
        "leader must have allocated at least one fd during open+close"
    );

    let checkpoint = leader.create_checkpoint().await;
    follower
        .reset(&checkpoint.root)
        .await
        .expect("follower reset");
    follower.rig(checkpoint.log).await.expect("follower rig");
    follower
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r,
        )
        .await
        .expect("follower evaluate fd-release check");

    // Direct assertion: every fd the leader allocated must have
    // been released on the follower's side too.  A regression
    // that dropped the `handles.remove` call from fs_close's
    // is_replay branch would leave the shadow alive → raw_fd
    // returns Some → the assertion below fires with the
    // specific leaked fd.
    for fd in leader_fd_lo..leader_fd_hi {
        assert!(
            follower.fs_handles.raw_fd(fd).await.is_none(),
            "Phase 2 fd-release regression: follower's shadow at fd {fd} \
                 was NOT released post-close.  fs_close's is_replay branch \
                 stopped calling handles.remove(fd) — the shadow's real OS \
                 fd (installed by fs_open's Phase-2 real-open under Consensus) \
                 stays alive across runtime lifetime, accumulating to \
                 MAX_OPEN_FDS on production validators."
        );
    }
}
/// **Consensus + O_APPEND rejection (2026-08-26).**  The
/// position-follow-up rejects `fsOpen` with mode `a` or `a+`
/// when the cap is Consensus, because O_APPEND semantics don't
/// fit the shadow-position model (kernel-retargets writes to
/// file-end atomically; the follower can't fstat).  Regression
/// scenario: a future refactor removes the guard → Consensus
/// append writes journal offset from stale shadow position →
/// follower replays writes to the wrong place → byte
/// divergence.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_append_open_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data.bin"), b"").unwrap();
    let runtime = create_runtime().await;

    // Try mode "a" on a Consensus cap — must return FSERR_BAD_ARG.
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`), oc in {{
              fsOpen!("{root}", "data.bin", "a", "consensus", *oc) |
              for (@reply <- oc) {{
                match reply {{
                  [false, "FSERR_BAD_ARG", _] => Nil
                  _ => @"UNEXPECTED_REPLY"!(reply)
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let result = runtime
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            rand(),
        )
        .await
        .expect("evaluate");
    assert!(
        result.errors.is_empty(),
        "consensus + append should reply cleanly (not raise); got errors: {:?}",
        result.errors,
    );
    // WAL stays empty — the open was rejected before any
    // journal-eligible op ran.
    assert!(
        runtime.fs_handles.wal.is_empty(),
        "consensus + append rejection must not journal any WAL entry",
    );
}
