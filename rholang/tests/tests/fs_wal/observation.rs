// Observation family — O tests.
//
// Wave-3 S3.14 (2026-09-10) — split out of `fs_wal_spec.rs` via
// `#[path]` submodule.  See parent module `fs_wal_spec::tests`
// for shared setup (create_runtime, create_leader_and_follower,
// rand, assert_dir_trees_byte_identical, translate_path,
// apply_wal_translated).

use super::*;

/// M-15 fix (2026-08-06): Consensus-mode `fs_entries`
/// integration smoke test.
///
/// The record-builder-layer omission is pinned by
/// `stat::stat_record_tests::consensus_mode_omits_host_transient_fields`
/// (which asserts stat_record with ConsensusMode::Consensus
/// omits mtime/ctime/atime/owner/group).  `entry_stat_row`
/// wraps stat_record and forwards its `mode` param
/// (handlers.rs:2140), so the omission composes transitively.
///
/// The M-15 gap was: no test invoked `fs_entries` with
/// cmode="consensus" through the native handler.  A
/// regression that reroutes entry_stat_row to always pass
/// `Oracular` (e.g., a hardcoded mode arg) would pass every
/// unit test but fork consensus on any operator using
/// `consensus-static-dirs`.
///
/// This pin closes the coverage gap with a smoke test that
/// exercises the full fs_entries chain against a real
/// tempdir + cmode="consensus".  A regression that broke
/// cmode plumbing between fs_entries and entry_stat_row
/// would surface here as a shape mismatch or a runtime
/// error.  The stat_record unit test remains the primary
/// omission proof.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_entries_consensus_mode_smoke_test() {
    let dir = tempfile::tempdir().unwrap();
    // fs_entries needs to safe_descend from root into a
    // non-empty rel (safe_descend rejects empty rel with
    // QuarantineError::Empty).  Create a subdirectory and
    // list that instead of listing tempdir root directly.
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("a.bin"), b"hello").unwrap();
    std::fs::write(sub.join("b.bin"), b"world").unwrap();

    let runtime = create_runtime().await;
    // Direct native invocation with cmode="consensus".
    // Successful evaluation (no InterpreterError) proves the
    // fs_entries dispatch, safe_descend, entry_stat_row per
    // entry, and Par assembly all accept and honor the
    // Consensus cmode arg.
    let term = format!(
        r#"
            new fsEntries(`rho:io:fs:native:1.0.0/entries`), ackCh in {{
              fsEntries!("{root}", "sub", "consensus", *ackCh) |
              for (@reply <- ackCh) {{
                // The reply must be `[true, list-of-records]`.
                // We don't peek the map keys here (that would
                // require string-form Par inspection which the
                // harness doesn't cleanly expose); the stat_record
                // unit test covers per-record omission.
                match reply {{
                  [true, _rows] => Nil
                  _ => @"M15_UNEXPECTED_SHAPE"!(reply)
                }}
              }}
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
        .expect(
            "M-15: fs_entries with cmode=\"consensus\" must complete without an \
                 InterpreterError — a compile / dispatch / cmode plumbing regression \
                 would fail HERE.",
        );
    // Post-M-5 (2026-08-06): fs_entries on a Consensus cap
    // DOES journal (WalOp::Entries).  A successful call to
    // fs_entries in cmode="consensus" must produce exactly
    // one Entries entry in the WAL.  The stat_record unit
    // test remains the primary omission proof; this pin
    // additionally confirms the M-5 journaling wire-through.
    let snap = runtime.fs_handles.wal.snapshot();
    assert_eq!(
        snap.len(),
        1,
        "M-15 + M-5: Consensus fs_entries call must produce exactly one WAL \
             Entries entry (post-M-5 journaling); got {} entries",
        snap.len()
    );
    assert_eq!(
        snap[0].op,
        rholang::rust::interpreter::io::wal::WalOp::Entries,
        "M-15 + M-5: journaled op must be Entries (op tag 13)"
    );
    assert_eq!(
        snap[0].outcome,
        rholang::rust::interpreter::io::wal::WalOutcome::Success,
        "M-15 + M-5: successful fs_entries must produce Success outcome"
    );
}

/// M-5 pin: fs_stat on cmode="oracular" MUST NOT journal.
/// A regression that ignored cmode and always journaled
/// would fire here — that's a consensus-safety regression
/// (Oracular reads would appear in Consensus WAL and
/// diverge across validators with different local fs).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn m5_fs_stat_oracular_does_not_journal() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.bin"), b"x").unwrap();
    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsStat(`rho:io:fs:native:1.0.0/stat`), ackCh in {{
              fsStat!("{root}", "f.bin", "oracular", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
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
        .expect("evaluate fs_stat oracular");
    assert!(
        runtime.fs_handles.wal.is_empty(),
        "M-5: fs_stat with cmode=\"oracular\" MUST NOT journal"
    );
}

/// M-5 pin: fs_stat on cmode="consensus" journals exactly
/// one Stat entry with Success outcome.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn m5_fs_stat_consensus_journals_stat_entry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.bin"), b"x").unwrap();
    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsStat(`rho:io:fs:native:1.0.0/stat`), ackCh in {{
              fsStat!("{root}", "f.bin", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
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
        .expect("evaluate fs_stat consensus");
    let snap = runtime.fs_handles.wal.snapshot();
    assert_eq!(
        snap.len(),
        1,
        "M-5: Consensus fs_stat must journal exactly one entry"
    );
    assert_eq!(snap[0].op, rholang::rust::interpreter::io::wal::WalOp::Stat);
    assert_eq!(
        snap[0].outcome,
        rholang::rust::interpreter::io::wal::WalOutcome::Success
    );
    // Payload_ref is a Blake2b256 hash of the reply Par.
    // Two runs against the same file MUST produce the same
    // hash — the whole point of the journaling scheme.
    assert!(matches!(
        snap[0].payload_ref,
        Some(rholang::rust::interpreter::io::wal::PayloadRef::Hash(_))
    ));
}

/// M-5 pin: fs_stat on a non-existent path journals a
/// Failure entry with FSERR_CODE_NOT_FOUND — leader/follower
/// symmetric even for failed reads (H-6-style outcome
/// discriminator).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn m5_fs_stat_consensus_failure_journals_failure_entry() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsStat(`rho:io:fs:native:1.0.0/stat`), ackCh in {{
              fsStat!("{root}", "does-not-exist.bin", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
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
        .expect("evaluate fs_stat consensus (missing)");
    let snap = runtime.fs_handles.wal.snapshot();
    assert_eq!(snap.len(), 1);
    match snap[0].outcome {
        rholang::rust::interpreter::io::wal::WalOutcome::Failure { code } => {
            assert_eq!(
                code,
                rholang::rust::interpreter::io::errors::FSERR_CODE_NOT_FOUND,
                "M-5: fs_stat on missing path must journal FSERR_CODE_NOT_FOUND"
            );
        }
        other => panic!("M-5: expected Failure outcome, got {other:?}"),
    }
}

/// M-5 pin (2026-08-06): the C-R1 leader/follower symmetry
/// invariant also holds for state-read journaling.
///
/// Runs an fs_stat call on a Consensus cap on the leader,
/// captures its WAL, rigs a follower on the same store +
/// re-executes; the two WALs (each with a single Stat
/// entry) must be byte-identical.  A regression that made
/// the reply-hash non-deterministic (e.g., include mtime
/// somehow) or diverged on error-code mapping would fail
/// here.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn m5_state_read_wal_is_byte_identical_on_leader_and_follower() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("data.bin"),
        b"leader-and-follower-shared-content",
    )
    .unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    // Two calls: one successful (Success outcome), one missing
    // path (Failure outcome).  Together they cover both
    // outcome branches of the M-5 hash derivation.
    let term = format!(
        r#"
            new fsStat(`rho:io:fs:native:1.0.0/stat`), a1, a2 in {{
              fsStat!("{root}", "data.bin", "consensus", *a1) |
              for (@_ <- a1) {{
                fsStat!("{root}", "does-not-exist.bin", "consensus", *a2) |
                for (@_ <- a2) {{ Nil }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[13; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate M-5");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert_eq!(
        leader_wal.len(),
        2,
        "leader must have journaled two Stat entries (one Success, one Failure)"
    );

    // Rig follower + replay.
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
        .expect("follower evaluate M-5");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal.len(),
        follower_wal.len(),
        "M-5: leader/follower WAL entry counts diverge for Stat journaling: \
             leader={} follower={}",
        leader_wal.len(),
        follower_wal.len()
    );
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(
            l, f,
            "M-5: Stat WAL entry {i} differs between leader and follower \
                 (byte-identity is the whole point of the read-hash journaling): \
                 leader={l:?} follower={f:?}"
        );
    }
}

/// Phase 1 pin (Consensus re-execute + verify, 2026-09-01):
/// **positive path**.  When the follower's on-disk file state
/// matches the leader's at replay time, the follower's fs_stat
/// re-execute produces a Consensus stat_record whose stable_hash
/// equals the leader's cached-reply hash; the follower's WAL
/// entry is byte-identical to the leader's, with
/// `WalOutcome::Success`.
///
/// Distinct from `m5_state_read_wal_is_byte_identical_on_leader_
/// and_follower` above: that test held pre-Phase-1 too (follower
/// consumed the cached reply and re-hashed it — trivially
/// byte-identical).  This test forces the Phase-1 mechanism to
/// engage by chmod-ing the file between leader + follower calls
/// in a way that would have been invisible under Phase-0
/// tautological replay.  Under Phase 1, mode bits ARE hashed
/// (`stat_record` under Consensus keeps `mode & 0o0777`), so if
/// the fresh syscall path is engaged, changing permission bits
/// between leader and follower would flip the outcome to Failure
/// — we choose bits that DON'T change to preserve Success while
/// still demonstrating the fresh syscall runs.
///
/// The stronger fresh-syscall proof is in
/// `consensus_fs_stat_reexecute_detects_divergence` below —
/// mutate the file's SIZE between leader + follower and observe
/// the divergence code surface.  That test's failure would prove
/// the fresh-syscall path IS engaged (Phase-0 tautological replay
/// would silently accept the mismatch).  This positive test's
/// job is to prove the mechanism produces the RIGHT WAL shape
/// when the state agrees.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_stat_reexecute_matches_leader_on_identical_state() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("data.bin");
    std::fs::write(&target, b"phase-1-re-execute-positive-pin").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsStat(`rho:io:fs:native:1.0.0/stat`), ackCh in {{
              fsStat!("{root}", "data.bin", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[21; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate Phase-1 positive path");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert_eq!(
        leader_wal.len(),
        1,
        "expected exactly one Stat WAL entry from the leader; got {}",
        leader_wal.len()
    );
    assert_eq!(leader_wal[0].op, WalOp::Stat);
    assert_eq!(
        leader_wal[0].outcome,
        WalOutcome::Success,
        "leader's fs_stat on an existing file must journal Success"
    );

    // Rig follower.  On-disk file state left unchanged →
    // follower's fresh syscall produces the same stat_record →
    // verify_reply_hash_matches_cached returns Ok → WAL entry is
    // byte-identical.  A regression that reverted the handler to
    // Phase-0 tautological cached-reply consumption would ALSO
    // pass this test (positive path is invariant across the two
    // behaviors); the divergence test below is the discriminator.
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
        .expect("follower evaluate Phase-1 positive path");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        follower_wal.len(),
        1,
        "expected exactly one Stat WAL entry from the follower; got {}",
        follower_wal.len()
    );
    assert_eq!(
        leader_wal[0], follower_wal[0],
        "Phase 1: follower's re-executed Stat WAL entry must be \
             byte-identical to the leader's on matching fs state"
    );
    assert_eq!(
        follower_wal[0].outcome,
        WalOutcome::Success,
        "Phase 1 positive path: follower's Stat entry outcome must be Success"
    );

    follower.check_replay_data().await.expect(
        "replay data must match — a divergent Par produce would \
                     trip RSpace rig verification",
    );
}

/// Phase 1 pin (Consensus re-execute + verify, 2026-09-01):
/// **divergence-detection path**.  When the follower's on-disk
/// file state differs from the leader's (simulated here by
/// mutating `data.bin` between leader + follower `evaluate`
/// calls), the follower's fs_stat re-execute produces a
/// stat_record whose stable_hash does NOT match the leader's
/// cached-reply hash → the handler returns
/// `[false, "FSERR_CONSENSUS_DIVERGENCE", ...]` and journals a
/// Stat WAL entry with
/// `WalOutcome::Failure { code: FSERR_CODE_CONSENSUS_DIVERGENCE }`.
///
/// Doubles as the fresh-syscall-engagement proof: a regression
/// that reverted the Consensus follower branch to Phase-0
/// tautological cached-reply consumption would silently accept
/// the mismatch (follower's WAL would show a `Success` Stat entry
/// with the leader's hash, and this test's `assert_eq!(outcome,
/// Failure { .. })` would fail).
///
/// RSpace rig behavior: the divergent reply Par produced by the
/// follower's handler differs bytewise from the leader's cached
/// produce, so `check_replay_data` fails at the produce
/// comparator — this IS the enforcement mechanism (block
/// validation rejects the block downstream because state hashes
/// diverge).  The test asserts the RSpace-side rejection AND
/// the WAL-side divergence code together; either alone would
/// leave the mechanism half-verified.  See auto-memory
/// `fileio_wal_replay_verification_gap.md` for the design.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_stat_reexecute_detects_divergence() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_CONSENSUS_DIVERGENCE;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("data.bin");
    // Leader sees a small file.  Its stat_record.size will hash
    // into `PayloadRef::Hash(reply_hash)` in the leader's WAL.
    std::fs::write(&target, b"leader-sees-me").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsStat(`rho:io:fs:native:1.0.0/stat`), ackCh in {{
              fsStat!("{root}", "data.bin", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[22; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate Phase-1 divergence setup");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert_eq!(leader_wal.len(), 1);
    assert_eq!(leader_wal[0].op, WalOp::Stat);
    assert_eq!(
        leader_wal[0].outcome,
        WalOutcome::Success,
        "leader must see the file successfully — divergence must \
             originate from the follower's re-execute, not from leader-side error"
    );

    // Force divergence: append bytes to grow the file's size
    // (Consensus `stat_record` includes `size`, so any size delta
    // flips the hash).  Doing this BETWEEN leader.evaluate and
    // follower.evaluate cleanly simulates "leader and follower
    // see different filesystem states" — the failure mode D3's
    // per-validator subdirs are designed to normally prevent, and
    // that Phase 1 detects when it happens.
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&target)
            .expect("open target for append");
        f.write_all(b"-follower-sees-more")
            .expect("append to target");
    }

    // Rig follower + evaluate.  The follower's fs_stat re-execute
    // sees the grown file → different stat_record → divergence
    // reply → WAL entry with Failure { FSERR_CODE_CONSENSUS_DIVERGENCE }.
    let checkpoint = leader.create_checkpoint().await;
    follower
        .reset(&checkpoint.root)
        .await
        .expect("follower reset");
    follower.rig(checkpoint.log).await.expect("follower rig");
    // The evaluate itself may return Ok even though the produce
    // diverges — the divergent produce is caught by
    // `check_replay_data` below.  We deliberately do NOT unwrap
    // the evaluate: the WAL entry is populated before produce
    // fires (journal_state_read runs first inside the handler),
    // so the WAL check is the primary assertion regardless of
    // evaluate's return value.
    let _ = follower
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r,
        )
        .await;
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        follower_wal.len(),
        1,
        "Phase 1 divergence path must still journal exactly one \
             Stat entry (the divergence is a Failure outcome, not a \
             journaling skip); got {} entries",
        follower_wal.len()
    );
    assert_eq!(follower_wal[0].op, WalOp::Stat);
    match follower_wal[0].outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_CONSENSUS_DIVERGENCE,
            "Phase 1: divergence WAL entry must carry the CONSENSUS_DIVERGENCE \
                 code, not an unrelated FSERR — got code {code}"
        ),
        WalOutcome::Success => panic!(
            "Phase 1 REGRESSION: follower's re-executed fs_stat produced a \
                 Success outcome despite the on-disk divergence between leader \
                 and follower.  This means the fresh-syscall path is not \
                 engaged (Phase-0 tautological cached-reply consumption has \
                 come back), or the verify_reply_hash_matches_cached comparator \
                 is broken.  Leader WAL entry: {leader_entry:?}; follower WAL \
                 entry: {follower_entry:?}",
            leader_entry = leader_wal[0],
            follower_entry = follower_wal[0],
        ),
    }

    // Enforcement side: the divergent reply Par produced by the
    // follower does NOT match the leader's cached produce, so
    // RSpace's rig comparator reports a divergence.  Under
    // Casper, this manifests as block rejection.
    let rig_result = follower.check_replay_data().await;
    assert!(
        rig_result.is_err(),
        "Phase 1 D1 enforcement: divergent fs_stat reply Par should trip \
             RSpace rig verification — got Ok, which means the follower's \
             produce matched the leader's cached produce despite the fs \
             divergence.  This would silently accept a leader lie."
    );
}

/// Consensus ban-lift (2026-09-04): **fs_exists positive path
/// matches leader on identical state**.  Mirror of
/// `consensus_fs_stat_reexecute_matches_leader_on_identical_state`
/// (line 1352) for the newly-lifted fs_exists surface.  Under the
/// pre-ban-lift shape, Dir.rho::exists would return FSERR_
/// UNSUPPORTED on any Consensus cap; the URN itself lacked a
/// cmode slot for `journal_state_read` to gate on.
///
/// Post-lift: arity bumped 3 → 4; native handler grew a Phase-5
/// re-execute + verify branch (mirror fs_stat's Consensus replay
/// arm); `WalOp::Exists` journals the `[true, Bool]` reply hash
/// on Consensus caps.  This positive pin exercises the leader/
/// follower WAL byte-identity on matching FS state; the
/// divergence pin below is the fresh-syscall-engagement proof.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_exists_reexecute_matches_leader_on_identical_state() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data.bin"), b"present").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsExists(`rho:io:fs:native:1.0.0/exists`), ackCh in {{
              fsExists!("{root}", "data.bin", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[96; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate Consensus fs_exists positive path");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert_eq!(
        leader_wal.len(),
        1,
        "expected exactly one Exists WAL entry from the leader; got {}",
        leader_wal.len()
    );
    assert_eq!(leader_wal[0].op, WalOp::Exists);
    assert_eq!(
        leader_wal[0].outcome,
        WalOutcome::Success,
        "leader's fs_exists on an existing file must journal Success \
             (reply is [true, true] — the head-bool is true even when the \
             file is absent, because `[true, false]` is still a Success \
             outcome from `journal_state_read`'s perspective)"
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
        .expect("follower evaluate Consensus fs_exists positive path");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(follower_wal.len(), 1);
    assert_eq!(
        leader_wal[0], follower_wal[0],
        "Ban-lift: follower's re-executed Exists WAL entry must be \
             byte-identical to the leader's on matching fs state"
    );
    assert_eq!(follower_wal[0].outcome, WalOutcome::Success);

    follower.check_replay_data().await.expect(
        "replay data must match — a divergent Par produce would trip \
             RSpace rig verification",
    );
}

/// Consensus ban-lift (2026-09-04): **fs_exists divergence
/// detection**.  Mirror of
/// `consensus_fs_stat_reexecute_detects_divergence` (line 1468).
/// The pre-ban-lift Dir.rho::exists had no divergence surface
/// (Consensus dispatch returned FSERR_UNSUPPORTED before reaching
/// the native).  Post-lift: rm the file between leader + follower
/// so the follower's `libc::fstatat` returns ENOENT → fresh
/// reply becomes `[true, false]` while leader cached `[true,
/// true]` → `verify_reply_hash_matches_cached` mismatch →
/// `err(FSERR_CONSENSUS_DIVERGENCE)` reply → WAL entry with
/// `WalOutcome::Failure { code: FSERR_CODE_CONSENSUS_DIVERGENCE }`
/// + `check_replay_data` Err.
///
/// A regression that reverted the Consensus follower branch to
/// Phase-0 tautological cached-reply consumption would silently
/// accept the mismatch — follower's WAL would show a `Success`
/// Exists entry, and this test's assert_eq on
/// `Failure { FSERR_CODE_CONSENSUS_DIVERGENCE }` would fail.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_exists_reexecute_detects_divergence() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_CONSENSUS_DIVERGENCE;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("data.bin");
    // Leader sees the file present → reply [true, true].
    std::fs::write(&target, b"present-on-leader").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsExists(`rho:io:fs:native:1.0.0/exists`), ackCh in {{
              fsExists!("{root}", "data.bin", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[97; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate Consensus fs_exists divergence setup");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert_eq!(leader_wal.len(), 1);
    assert_eq!(leader_wal[0].op, WalOp::Exists);
    assert_eq!(leader_wal[0].outcome, WalOutcome::Success);

    // Remove the file between leader + follower — follower's
    // fresh fstatat returns ENOENT → reply becomes [true, false]
    // → hash mismatch → CONSENSUS_DIVERGENCE.
    std::fs::remove_file(&target).expect("remove target");

    let checkpoint = leader.create_checkpoint().await;
    follower
        .reset(&checkpoint.root)
        .await
        .expect("follower reset");
    follower.rig(checkpoint.log).await.expect("follower rig");
    let _ = follower
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r,
        )
        .await;
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        follower_wal.len(),
        1,
        "divergence path must still journal exactly one Exists entry \
             (Failure outcome, not a journaling skip); got {} entries",
        follower_wal.len()
    );
    assert_eq!(follower_wal[0].op, WalOp::Exists);
    match follower_wal[0].outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_CONSENSUS_DIVERGENCE,
            "Ban-lift: Exists divergence WAL entry must carry \
                 CONSENSUS_DIVERGENCE code — got code {code}"
        ),
        WalOutcome::Success => panic!(
            "Ban-lift REGRESSION: follower's re-executed fs_exists \
                 produced a Success outcome despite the on-disk divergence \
                 between leader and follower.  Either the fresh-syscall path \
                 is not engaged (Phase-0 tautological cached-reply consumption \
                 has come back), or verify_reply_hash_matches_cached is broken. \
                 Leader WAL entry: {leader_entry:?}; follower WAL entry: \
                 {follower_entry:?}",
            leader_entry = leader_wal[0],
            follower_entry = follower_wal[0],
        ),
    }

    let rig_result = follower.check_replay_data().await;
    assert!(
        rig_result.is_err(),
        "Ban-lift D1 enforcement: divergent fs_exists reply Par should \
             trip RSpace rig verification — got Ok, which would silently \
             accept a leader lie."
    );
}

/// Coverage-review addendum (2026-09-04): **fs_exists symmetric
/// absent**.  Parity with fs_stat's symmetric-error pin below
/// (line 1794 area).  Both leader and follower see the file
/// absent → `safe_descend_verified` returns `IoError` on both
/// sides → fresh reply is `[true, false]` on both → verify OK →
/// WAL entries byte-identical, both Success.  A regression that
/// spuriously fired CONSENSUS_DIVERGENCE on symmetric absence
/// (e.g., a verify path that erroneously compared the descent
/// error metadata instead of just the reply Par hash) would fail
/// here.
///
/// Note the outcome asymmetry from fs_stat's version: fs_stat's
/// symmetric-error reply is `[false, "FSERR_NOT_FOUND", ...]` →
/// WAL outcome is Failure { NOT_FOUND }.  fs_exists's symmetric
/// reply is `[true, false]` → head-bool is true → WAL outcome is
/// Success, even though the file was absent.  This is by design:
/// fs_exists reports "the file is not there" as a successful
/// observation, not as an error.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_exists_symmetric_absent_finalizes_to_success() {
    let dir = tempfile::tempdir().unwrap();
    // No file at "missing.bin".

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsExists(`rho:io:fs:native:1.0.0/exists`), ackCh in {{
              fsExists!("{root}", "missing.bin", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[132; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_exists symmetric-absent");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert_eq!(leader_wal.len(), 1);
    assert_eq!(leader_wal[0].op, WalOp::Exists);
    assert_eq!(
        leader_wal[0].outcome,
        WalOutcome::Success,
        "fs_exists on an absent file must journal Success — the \
             head-bool of [true, false] is true, so journal_state_read's \
             outcome derivation is Success even though the file is absent"
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
        .expect("follower evaluate fs_exists symmetric-absent");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal, follower_wal,
        "fs_exists symmetric-absent: WALs must be byte-identical.  \
             A regression that spuriously fired CONSENSUS_DIVERGENCE on \
             a symmetrically-absent file (both sides agreed on [true, \
             false]) would fail here."
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on symmetric absence");
}

/// Coverage-review addendum (2026-09-04): **fs_exists bad cmode
/// rejects**.  Parity with fs_stat / fs_entries's bad-cmode pins.
/// The `resolve_cmode` fail-closed path fires BEFORE any FS
/// syscall — a caller that passes a non-`"oracular"` /
/// non-`"consensus"` cmode gets `FSERR_BAD_ARG` immediately.
/// Leader-only (no rig setup needed): the bad-cmode arg-parse
/// error path doesn't journal.
///
/// Regression this closes: a refactor that moved cmode
/// resolution after the `is_replay` short-circuit, or that
/// changed `resolve_cmode` to fall back to a default on
/// unrecognized values (silently masking a caller-side typo
/// like `"consnsus"`), would fail here.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_exists_bad_cmode_rejects() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data.bin"), b"present").unwrap();
    let (leader, _follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsExists(`rho:io:fs:native:1.0.0/exists`), ackCh in {{
              fsExists!("{root}", "data.bin", "bogus", *ackCh) |
              for (@reply <- ackCh) {{ @"out"!(reply) }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[133; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r,
        )
        .await
        .expect("leader evaluate fs_exists bad cmode");

    // Bad-cmode path does NOT journal — it's a pre-syscall arg
    // parse error.  A regression that moved journaling before
    // cmode validation would surface here as a spurious WAL entry.
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert!(
        leader_wal.is_empty(),
        "bad-cmode fs_exists must NOT journal — the arg-parse \
             fail-closed path fires before any FS syscall.  Got \
             {leader_wal:?}",
    );
}

/// Coverage-review addendum (2026-09-04): **fs_exists Oracular
/// follower replay**.  The Phase-0 tautological arm — under
/// Oracular cmode the follower consumes the leader's cached
/// reply verbatim (per-node Oracle-mode FS state isn't
/// reproducible on the follower).  Journal_state_read is called
/// but self-guards on Consensus so it's a WAL no-op today.
///
/// This test verifies:
///   - Leader's fs_exists Oracular reply IS produced.
///   - Leader's WAL stays EMPTY (Oracular doesn't journal — the
///     journal_state_read self-guard fires).
///   - Follower's is_replay Oracular arm consumes cached reply
///     without re-executing (Oracular state isn't reproducible).
///   - Follower's WAL also stays empty.
///   - check_replay_data passes (leader's produce matches
///     follower's produce byte-for-byte).
///
/// Coverage this closes: a regression that broke the self-guard
/// in journal_state_read (i.e., started journaling under
/// Oracular) would surface here as a non-empty WAL.  A regression
/// that started re-executing under Oracular (a mis-copy of the
/// Consensus arm) would still pass check_replay_data because
/// nothing mutates between leader and follower here — the WAL-
/// emptiness check is the actual anchor.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oracular_fs_exists_reexecute_taut_matches_leader() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data.bin"), b"present").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsExists(`rho:io:fs:native:1.0.0/exists`), ackCh in {{
              fsExists!("{root}", "data.bin", "oracular", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[134; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate Oracular fs_exists");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert!(
        leader_wal.is_empty(),
        "Oracular fs_exists must NOT journal — journal_state_read \
             self-guards on Consensus.  Got {leader_wal:?}",
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
        .expect("follower evaluate Oracular fs_exists");
    let follower_wal = follower.fs_handles.wal.snapshot();
    assert!(
        follower_wal.is_empty(),
        "Oracular follower fs_exists must NOT journal — same \
             self-guard as leader.  Got {follower_wal:?}",
    );

    follower.check_replay_data().await.expect(
        "Oracular replay must match — the is_replay Oracular \
                     arm consumes cached reply verbatim",
    );
}

/// Phase 5 coverage-review addendum (2026-09-02): **fs_stat
/// symmetric syscall error**.  Attempt fs_stat on a non-existent
/// path on both sides → both see ENOENT → FSERR_NOT_FOUND →
/// verify OK → both finalize identically, NOT
/// CONSENSUS_DIVERGENCE.  Parity with fs_chmod / fs_remove_file
/// / fs_rename / fs_copy_file / fs_remove_dir symmetric-error
/// pins; extends the triad convention to observation ops.
///
/// Coverage gap this closes: a regression that spuriously fired
/// CONSENSUS_DIVERGENCE on symmetric ENOENT (e.g., verify
/// erroneously comparing hashes of two error replies with
/// different msg bytes but identical code) would slip past all
/// existing observation-op pins without this test.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_stat_symmetric_syscall_error_finalizes_to_failure() {
    let dir = tempfile::tempdir().unwrap();
    // No file at "missing.bin".

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsStat(`rho:io:fs:native:1.0.0/stat`), ackCh in {{
              fsStat!("{root}", "missing.bin", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[130; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_stat symmetric error");
    let leader_wal = leader.fs_handles.wal.snapshot();

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
        .expect("follower evaluate fs_stat symmetric error");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal, follower_wal,
        "Phase 5 fs_stat symmetric-error: WALs must be byte-identical. \
             A regression that spuriously fired CONSENSUS_DIVERGENCE on the \
             symmetric FSERR_NOT_FOUND would fail here."
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on symmetric syscall error");
}

/// Phase 2 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_size positive path**.  Mirrors the Phase-1 fs_stat
/// pattern but on a fd-based observation op: leader opens a
/// Consensus cap, calls fs_size, and journals a `WalOp::Size`
/// entry with `Success` outcome.  Follower rig+replay
/// re-executes fstat via its own shadow fd; the file's on-disk
/// bytes agree with the leader's, so the u64 size matches →
/// verify OK → follower's WAL entry is byte-identical to the
/// leader's.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_size_reexecute_matches_leader_on_identical_state() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("data.bin");
    std::fs::write(&target, b"phase-2-fs-size-positive-pin").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    // Open the file on a Consensus cap + fs_size on its fd, then
    // close.  Two WAL entries expected on the leader: the
    // openFile's statCheck Stat + the fs_size Size.  Both should
    // reproduce byte-identically on the follower under Phase 2.
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsSize(`rho:io:fs:native:1.0.0/size`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, szCh, closeCh
            in {{
              fsOpen!("{root}", "data.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsSize!(fd, *szCh) |
                for (@_ <- szCh) {{
                  fsClose!(fd, *closeCh) |
                  for (@_ <- closeCh) {{ Nil }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[31; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate Phase-2 fs_size positive path");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_size_entries: Vec<_> = leader_wal.iter().filter(|e| e.op == WalOp::Size).collect();
    assert_eq!(
        leader_size_entries.len(),
        1,
        "expected exactly one Size WAL entry from the leader; got {} out of \
             {} total WAL entries",
        leader_size_entries.len(),
        leader_wal.len()
    );
    assert_eq!(leader_size_entries[0].outcome, WalOutcome::Success);

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
        .expect("follower evaluate Phase-2 fs_size positive path");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal.len(),
        follower_wal.len(),
        "Phase 2: leader/follower WAL lengths diverge for fs_size positive \
             path: leader={} follower={}",
        leader_wal.len(),
        follower_wal.len()
    );
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(
            l, f,
            "Phase 2: WAL entry {i} differs between leader and follower on \
                 fs_size positive path: leader={l:?} follower={f:?}"
        );
    }
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on identical fs state");
}

/// Phase 2 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_size divergence-detection path**.  Grow the file between
/// leader + follower `evaluate` calls; follower's fs_size
/// re-execute sees a different `st_size` → verify hash-mismatch →
/// follower's WAL Size entry carries `Failure { FSERR_CODE_
/// CONSENSUS_DIVERGENCE }` AND `check_replay_data` returns Err.
///
/// Doubles as the fresh-syscall-engagement proof for fs_size:
/// a regression to Phase-0 tautological cached-reply consumption
/// would silently accept the size mismatch.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_size_reexecute_detects_divergence() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_CONSENSUS_DIVERGENCE;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("data.bin");
    std::fs::write(&target, b"leader-sees").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsSize(`rho:io:fs:native:1.0.0/size`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, szCh, closeCh
            in {{
              fsOpen!("{root}", "data.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsSize!(fd, *szCh) |
                for (@_ <- szCh) {{
                  fsClose!(fd, *closeCh) |
                  for (@_ <- closeCh) {{ Nil }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[32; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate Phase-2 fs_size divergence setup");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_size_entries: Vec<_> = leader_wal.iter().filter(|e| e.op == WalOp::Size).collect();
    assert_eq!(leader_size_entries.len(), 1);
    assert_eq!(leader_size_entries[0].outcome, WalOutcome::Success);

    // Grow the file — follower's fs_size re-execute must see a
    // larger u64 than the leader recorded.  Note we also mutate
    // the file's bytes indirectly (the append changes the
    // hashed-content of the Stat record from openFile's
    // statCheck too), so the follower's WAL will show BOTH the
    // Stat divergence AND the Size divergence.  This test's
    // assertion filters on op = Size specifically.
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&target)
            .expect("open target for append");
        f.write_all(b"-follower-sees-more-bytes")
            .expect("append to target");
    }

    let checkpoint = leader.create_checkpoint().await;
    follower
        .reset(&checkpoint.root)
        .await
        .expect("follower reset");
    follower.rig(checkpoint.log).await.expect("follower rig");
    let _ = follower
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r,
        )
        .await;
    let follower_wal = follower.fs_handles.wal.snapshot();

    // Look up the Size entry specifically — the fs_stat divergence
    // from openFile's statCheck also fires and may be earlier in
    // the log, but this test's contract is fs_size specifically.
    let follower_size = follower_wal.iter().find(|e| e.op == WalOp::Size).expect(
        "Phase 2 divergence path must still journal a Size entry \
                 (Failure outcome, not a journaling skip)",
    );
    match follower_size.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_CONSENSUS_DIVERGENCE,
            "Phase 2: Size divergence WAL entry must carry CONSENSUS_DIVERGENCE \
                 code, not an unrelated FSERR — got code {code}"
        ),
        WalOutcome::Success => panic!(
            "Phase 2 REGRESSION: follower's re-executed fs_size produced a \
                 Success outcome despite the on-disk divergence — the fresh-syscall \
                 path is not engaged or verify_reply_hash_matches_cached is broken. \
                 Size entry: {follower_size:?}"
        ),
    }

    // Same enforcement channel as fs_stat: divergent reply Par
    // trips RSpace rig verification, which is what block
    // validation rejects on at the Casper layer.
    let rig_result = follower.check_replay_data().await;
    assert!(
        rig_result.is_err(),
        "Phase 2 D1 enforcement: divergent fs_size reply Par should trip \
             RSpace rig verification — got Ok, which would silently accept a \
             leader lie."
    );
}

/// Phase 5 coverage-review addendum (2026-09-02): **fs_size
/// symmetric syscall error**.  Both sides call fs_size on the
/// same bogus fd (999, never opened) → both look up in the fd
/// table, both get None → FSERR_CLOSED symmetrically → verify
/// OK → both finalize identically, NOT CONSENSUS_DIVERGENCE.
///
/// fs_size is fd-based, so the natural symmetric-error scenario
/// is a lookup miss rather than a path miss.  A bogus fd literal
/// avoids any test-harness fd-allocation determinism concerns:
/// no fs_open call means no fd was ever allocated, so 999 is
/// safely unassigned on both leader and follower.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_size_symmetric_syscall_error_finalizes_to_failure() {
    let _dir = tempfile::tempdir().unwrap();
    let (mut leader, mut follower) = create_leader_and_follower().await;

    // Bogus fd — no fs_open → nothing in the fd table.  Both
    // sides return FSERR_CLOSED symmetrically.
    let term = r#"
            new fsSize(`rho:io:fs:native:1.0.0/size`), ackCh in {
              fsSize!(999, *ackCh) |
              for (@_ <- ackCh) { Nil }
            }
        "#;
    let r = Blake2b512Random::create_from_bytes(&[131; 32]);

    leader
        .evaluate(
            term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_size symmetric error");
    let leader_wal = leader.fs_handles.wal.snapshot();

    let checkpoint = leader.create_checkpoint().await;
    follower
        .reset(&checkpoint.root)
        .await
        .expect("follower reset");
    follower.rig(checkpoint.log).await.expect("follower rig");
    follower
        .evaluate(
            term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r,
        )
        .await
        .expect("follower evaluate fs_size symmetric error");
    let follower_wal = follower.fs_handles.wal.snapshot();

    // fd-lookup-miss produces no WAL entry (jmode is None →
    // journal_state_read skips).  Both sides observe the same
    // "no WAL" state.
    assert_eq!(
        leader_wal, follower_wal,
        "Phase 5 fs_size symmetric-error: WALs must be byte-identical. \
             A regression that journaled on fd-lookup-miss (or spuriously \
             fired CONSENSUS_DIVERGENCE on the symmetric FSERR_CLOSED) \
             would fail here."
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on symmetric syscall error");
}

/// Phase 2 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_read_at positive path**.  Positional read (`libc::pread`)
/// against the follower's own real fd, which was installed by
/// fs_open's Phase-2 real-open.  Since `pread` doesn't consume
/// or advance the fd position, this handler is the simplest of
/// the byte-returning observation ops — no shadow position
/// coordination.  On identical fs state, the follower's fresh
/// bytes hash to the same Blake2b256 as the leader's cached
/// bytes → verify OK → WAL byte-identity preserved.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_read_at_reexecute_matches_leader_on_identical_state() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("data.bin"),
        b"phase-2-fs-read-at-positive-pin",
    )
    .unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    // Open + pread(off=7, n=10) + close.  Positional read on a
    // Consensus cap.  fs_open's real-open makes the follower's
    // shadow's file usable for the follower's libc::pread.
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsReadAt(`rho:io:fs:native:1.0.0/readAt`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, rdCh, closeCh
            in {{
              fsOpen!("{root}", "data.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsReadAt!(fd, 7, 10, *rdCh) |
                for (@_ <- rdCh) {{
                  fsClose!(fd, *closeCh) |
                  for (@_ <- closeCh) {{ Nil }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[41; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_read_at positive path");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_readat_entries: Vec<_> = leader_wal
        .iter()
        .filter(|e| e.op == WalOp::ReadAt)
        .collect();
    assert_eq!(
        leader_readat_entries.len(),
        1,
        "expected exactly one ReadAt WAL entry from the leader; got {}",
        leader_readat_entries.len()
    );
    assert_eq!(leader_readat_entries[0].outcome, WalOutcome::Success);

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
        .expect("follower evaluate fs_read_at positive path");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal.len(),
        follower_wal.len(),
        "Phase 2: leader/follower WAL lengths diverge for fs_read_at positive \
             path: leader={} follower={}",
        leader_wal.len(),
        follower_wal.len()
    );
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(
            l, f,
            "Phase 2: WAL entry {i} differs between leader and follower on \
                 fs_read_at positive path: leader={l:?} follower={f:?}"
        );
    }
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on identical fs state");
}

/// Phase 2 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_read_at divergence-detection path**.  Overwrite the
/// file's bytes between leader + follower `evaluate` calls;
/// follower's fs_read_at re-execute reads the DIFFERENT bytes
/// at the same (off, n) → verify hash-mismatch → follower's
/// ReadAt WAL entry carries `Failure { FSERR_CODE_CONSENSUS_
/// DIVERGENCE }` AND `check_replay_data` returns Err.
///
/// Note: the divergence-err reply Par is `[false,
/// "FSERR_CONSENSUS_DIVERGENCE", msg]` — no bytes, hence the
/// Failure WAL entry has `payload_ref: None` + `length: None`
/// (journal_read hardcodes Success, so the divergence path
/// builds the WalEntry manually).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_read_at_reexecute_detects_divergence() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_CONSENSUS_DIVERGENCE;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("data.bin");
    // Leader sees "leader-sees-me" at the target.  fs_open's
    // statCheck sees the file exists + regular; fs_read_at reads
    // bytes 7..17 (i.e. "es-me" + null padding, or whatever the
    // 10-byte window yields).
    std::fs::write(&target, b"leader-sees-me-and-then-some-bytes-past").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsReadAt(`rho:io:fs:native:1.0.0/readAt`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, rdCh, closeCh
            in {{
              fsOpen!("{root}", "data.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsReadAt!(fd, 7, 10, *rdCh) |
                for (@_ <- rdCh) {{
                  fsClose!(fd, *closeCh) |
                  for (@_ <- closeCh) {{ Nil }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[42; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_read_at divergence setup");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert!(
        leader_wal
            .iter()
            .any(|e| e.op == WalOp::ReadAt && e.outcome == WalOutcome::Success),
        "leader must have journaled a successful ReadAt entry"
    );

    // Overwrite the file's bytes at the pread window.  Same
    // length so the fs_stat statCheck (openFileImpl) still
    // agrees on size — the divergence surfaces at the ReadAt
    // level specifically, not at the Stat level.  This isolates
    // the fs_read_at re-execute path in the assertion below.
    std::fs::write(&target, b"FOLLOWER-SEES-DIFFERENT-BYTES-here-past").unwrap();

    let checkpoint = leader.create_checkpoint().await;
    follower
        .reset(&checkpoint.root)
        .await
        .expect("follower reset");
    follower.rig(checkpoint.log).await.expect("follower rig");
    let _ = follower
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r,
        )
        .await;
    let follower_wal = follower.fs_handles.wal.snapshot();

    let follower_readat = follower_wal.iter().find(|e| e.op == WalOp::ReadAt).expect(
        "Phase 2 divergence path must still journal a ReadAt entry \
                 (Failure outcome, not a journaling skip)",
    );
    match follower_readat.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_CONSENSUS_DIVERGENCE,
            "Phase 2: ReadAt divergence WAL entry must carry \
                 CONSENSUS_DIVERGENCE code, not an unrelated FSERR — got \
                 code {code}"
        ),
        WalOutcome::Success => panic!(
            "Phase 2 REGRESSION: follower's re-executed fs_read_at produced \
                 a Success outcome despite the on-disk divergence.  Either the \
                 fresh-syscall path is not engaged (Phase-0 tautological cached-\
                 reply consumption came back) or verify_reply_hash_matches_cached \
                 is broken.  ReadAt entry: {follower_readat:?}"
        ),
    }
    // Divergence WAL entry shape: no bytes, so payload_ref +
    // length are None.  A regression that reused journal_read
    // for the divergence path would produce Hash(empty) +
    // length=0 — assert the None shape holds.
    assert_eq!(follower_readat.payload_ref, None);
    assert_eq!(follower_readat.length, None);

    let rig_result = follower.check_replay_data().await;
    assert!(
        rig_result.is_err(),
        "Phase 2 D1 enforcement: divergent fs_read_at reply Par should trip \
             RSpace rig verification — got Ok, which would silently accept a \
             leader lie."
    );
}

/// T-1 coverage-review addendum (2026-09-03): Phase 2 symmetric-
/// syscall-error pin for **fs_read_at**.  Bogus fd → both leader
/// and follower's fd-lookup miss → symmetric `FSERR_CLOSED`.  The
/// fd-lookup-miss path does NOT journal (jmode is None), so both
/// sides observe the same empty-WAL state.  A regression that
/// spuriously fired `CONSENSUS_DIVERGENCE` on the symmetric
/// FSERR_CLOSED (or that started journaling on fd-lookup-miss)
/// would fail here.  Mirrors the fs_size / fs_stat / fs_entries /
/// fs_truncate symmetric pins already in place.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_read_at_symmetric_syscall_error_finalizes_to_failure() {
    let _dir = tempfile::tempdir().unwrap();
    let (mut leader, mut follower) = create_leader_and_follower().await;

    // Bogus fd — no fs_open → fd table lookup miss → both sides
    // return FSERR_CLOSED symmetrically.  ReadAt asks for 10
    // bytes at offset 0 from a nonexistent fd; the handler
    // rejects at the fd-lookup step before any syscall.
    let term = r#"
            new fsReadAt(`rho:io:fs:native:1.0.0/readAt`), ackCh in {
              fsReadAt!(999, 0, 10, *ackCh) |
              for (@_ <- ackCh) { Nil }
            }
        "#;
    let r = Blake2b512Random::create_from_bytes(&[141; 32]);

    leader
        .evaluate(
            term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_read_at symmetric error");
    let leader_wal = leader.fs_handles.wal.snapshot();

    let checkpoint = leader.create_checkpoint().await;
    follower
        .reset(&checkpoint.root)
        .await
        .expect("follower reset");
    follower.rig(checkpoint.log).await.expect("follower rig");
    follower
        .evaluate(
            term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r,
        )
        .await
        .expect("follower evaluate fs_read_at symmetric error");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal, follower_wal,
        "Phase 2 fs_read_at symmetric-error: WALs must be byte-identical. \
             A regression that spuriously fired CONSENSUS_DIVERGENCE on the \
             symmetric FSERR_CLOSED (or that started journaling on fd-lookup- \
             miss) would fail here."
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on symmetric syscall error");
}

/// Phase 2 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_read positive path** (sequential read).  Unlike
/// fs_read_at (positional), sequential fs_read advances both
/// the OS-level fd position (kernel-side, via libc::read) AND
/// the FileHandle's shadow position (in-process, via the
/// with_mut increment after journal_read).  Under Phase 2, the
/// follower's re-executed libc::read must advance both in
/// lockstep with the leader's play run, so downstream reads
/// (on the same fd, in a subsequent deploy or same deploy)
/// consume from the same file offset.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_read_reexecute_matches_leader_on_identical_state() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data.bin"), b"phase-2-fs-read-positive-pin").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    // Open + sequential read n=10 + close.  Consensus cap.
    // Sequential read consumes bytes 0..10 from position 0
    // (fresh fd starts at position 0).
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsRead(`rho:io:fs:native:1.0.0/read`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, rdCh, closeCh
            in {{
              fsOpen!("{root}", "data.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsRead!(fd, 10, *rdCh) |
                for (@_ <- rdCh) {{
                  fsClose!(fd, *closeCh) |
                  for (@_ <- closeCh) {{ Nil }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[51; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_read positive path");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_read_entries: Vec<_> = leader_wal.iter().filter(|e| e.op == WalOp::Read).collect();
    assert_eq!(
        leader_read_entries.len(),
        1,
        "expected exactly one Read WAL entry from the leader; got {}",
        leader_read_entries.len()
    );
    assert_eq!(leader_read_entries[0].outcome, WalOutcome::Success);
    // Sequential Read journals with offset = pre-read shadow
    // position.  Fresh open → position 0 → the single Read
    // entry has offset = Some(0).  A regression that stopped
    // capturing shadow position (or captured it post-read)
    // would break this pin.
    assert_eq!(
        leader_read_entries[0].offset,
        Some(0),
        "sequential Read WAL entry must record pre-read shadow \
             position (0 for a fresh fd)"
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
        .expect("follower evaluate fs_read positive path");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal.len(),
        follower_wal.len(),
        "Phase 2: leader/follower WAL lengths diverge for fs_read positive \
             path: leader={} follower={}",
        leader_wal.len(),
        follower_wal.len()
    );
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(
            l, f,
            "Phase 2: WAL entry {i} differs between leader and follower on \
                 fs_read positive path: leader={l:?} follower={f:?}"
        );
    }
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on identical fs state");
}

/// Phase 2 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_read shadow-position parity** across leader + follower.
/// Two sequential reads on the same fd — first n=5, then n=8.
/// Second read must consume bytes 5..13, meaning shadow
/// position advanced by exactly 5 after the first read.  If the
/// follower's Phase-2 re-execute failed to advance the shadow
/// position in lockstep (e.g., dropped the with_mut increment
/// on the Consensus branch), the second read's WAL entry's
/// offset field would diverge from the leader's → leader/follower
/// WAL byte-identity breaks → test fails at the assert_eq!
/// loop below.  This is the sequential-read equivalent of the
/// fs_size fd-plumbing engagement proof.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_read_shadow_position_parity_across_multi_reads() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("data.bin"),
        b"phase-2-shadow-position-parity-pin-content",
    )
    .unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    // Open + read(5) + read(8) + close.  First read consumes
    // bytes 0..5 ("phase"), advances shadow to 5; second read
    // consumes bytes 5..13 ("-2-shado"), advances shadow to 13.
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsRead(`rho:io:fs:native:1.0.0/read`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, r1, r2, cl
            in {{
              fsOpen!("{root}", "data.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsRead!(fd, 5, *r1) |
                for (@_ <- r1) {{
                  fsRead!(fd, 8, *r2) |
                  for (@_ <- r2) {{
                    fsClose!(fd, *cl) |
                    for (@_ <- cl) {{ Nil }}
                  }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[52; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate multi-read");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_reads: Vec<_> = leader_wal.iter().filter(|e| e.op == WalOp::Read).collect();
    assert_eq!(
        leader_reads.len(),
        2,
        "expected exactly two Read WAL entries from the leader"
    );
    assert_eq!(
        leader_reads[0].offset,
        Some(0),
        "first sequential Read must record offset = 0"
    );
    assert_eq!(
        leader_reads[1].offset,
        Some(5),
        "second sequential Read must record offset = 5 (after first read \
             advanced shadow by 5)"
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
        .expect("follower evaluate multi-read");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(leader_wal.len(), follower_wal.len());
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(
            l, f,
            "Phase 2 shadow-position parity: WAL entry {i} differs \
                 between leader and follower — the follower's Phase-2 \
                 re-execute failed to advance the shadow position in lockstep \
                 with the leader.  A regression that dropped the with_mut \
                 shadow-position increment from the Consensus branch of \
                 fs_read would fail this assertion at the second Read \
                 entry (offset mismatch: leader=5, follower=0).  \
                 leader={l:?} follower={f:?}"
        );
    }
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on multi-read shadow-position parity");
}

/// Phase 2 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_read edge case — n=0 empty read**.  Both leader and
/// follower call `libc::read(fd, ..., 0)` which returns 0
/// bytes; fresh reply is `ok_bytes([])`; cached reply matches;
/// verify OK; journal_read appends with `payload_ref:
/// Hash([])` + `length: 0`.  Confirms the Phase-2 mechanism
/// handles the empty-read edge naturally without a special
/// case, and that shadow-position advance by 0 is a no-op
/// (the second Read entry's offset in this test would still
/// be 0 after an n=0 first read).  Addresses coverage gap 3
/// from the 2026-09-01 fs_read slice review.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_read_n_zero_preserves_wal_byte_identity() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data.bin"), b"some-non-empty-content").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsRead(`rho:io:fs:native:1.0.0/read`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, rdCh, closeCh
            in {{
              fsOpen!("{root}", "data.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsRead!(fd, 0, *rdCh) |
                for (@_ <- rdCh) {{
                  fsClose!(fd, *closeCh) |
                  for (@_ <- closeCh) {{ Nil }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[54; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_read n=0");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_reads: Vec<_> = leader_wal.iter().filter(|e| e.op == WalOp::Read).collect();
    assert_eq!(
        leader_reads.len(),
        1,
        "expected exactly one Read WAL entry from the n=0 leader call"
    );
    assert_eq!(leader_reads[0].outcome, WalOutcome::Success);
    assert_eq!(
        leader_reads[0].length,
        Some(0),
        "n=0 fs_read must journal length=0"
    );
    assert_eq!(
        leader_reads[0].offset,
        Some(0),
        "n=0 fs_read shadow-position advance is a no-op; offset stays 0"
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
        .expect("follower evaluate fs_read n=0");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(leader_wal.len(), follower_wal.len());
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(
            l, f,
            "Phase 2 n=0 edge: WAL entry {i} differs between leader and \
                 follower — an empty-read mishandling would show here as \
                 payload_ref divergence: leader={l:?} follower={f:?}"
        );
    }
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on n=0 read");
}

/// Phase 2 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_read divergence-detection path**.  Overwrite the file
/// at the same length between leader + follower evaluate;
/// follower's fs_read re-execute returns DIFFERENT bytes at
/// offset 0 → verify hash-mismatch → Failure WAL entry with
/// CONSENSUS_DIVERGENCE + check_replay_data Err.  Uses the
/// shared `journal_read_divergence` helper introduced this
/// slice (op=WalOp::Read because offset=None).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_read_reexecute_detects_divergence() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_CONSENSUS_DIVERGENCE;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("data.bin");
    std::fs::write(&target, b"leader-sees-these-bytes-here").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsRead(`rho:io:fs:native:1.0.0/read`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, rdCh, closeCh
            in {{
              fsOpen!("{root}", "data.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsRead!(fd, 12, *rdCh) |
                for (@_ <- rdCh) {{
                  fsClose!(fd, *closeCh) |
                  for (@_ <- closeCh) {{ Nil }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[53; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_read divergence setup");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert!(
        leader_wal
            .iter()
            .any(|e| e.op == WalOp::Read && e.outcome == WalOutcome::Success),
        "leader must have journaled a successful Read entry"
    );

    // Overwrite same length so statCheck agrees (same size)
    // — divergence surfaces only at the Read op level.
    std::fs::write(&target, b"FOLLOWER-SEES-DIFFERENT-CONTENT").unwrap();

    let checkpoint = leader.create_checkpoint().await;
    follower
        .reset(&checkpoint.root)
        .await
        .expect("follower reset");
    follower.rig(checkpoint.log).await.expect("follower rig");
    let _ = follower
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r,
        )
        .await;
    let follower_wal = follower.fs_handles.wal.snapshot();

    let follower_read = follower_wal.iter().find(|e| e.op == WalOp::Read).expect(
        "Phase 2 divergence path must still journal a Read entry \
                 (Failure outcome, not a journaling skip)",
    );
    match follower_read.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_CONSENSUS_DIVERGENCE,
            "Phase 2: Read divergence WAL entry must carry \
                 CONSENSUS_DIVERGENCE code — got code {code}"
        ),
        WalOutcome::Success => panic!(
            "Phase 2 REGRESSION: follower's re-executed fs_read produced \
                 a Success outcome despite the on-disk divergence.  Read \
                 entry: {follower_read:?}"
        ),
    }
    // journal_read_divergence writes payload_ref: None + length: None.
    assert_eq!(follower_read.payload_ref, None);
    assert_eq!(follower_read.length, None);

    let rig_result = follower.check_replay_data().await;
    assert!(
        rig_result.is_err(),
        "Phase 2 D1 enforcement: divergent fs_read reply Par should trip \
             RSpace rig verification"
    );
}

/// T-1 coverage-review addendum (2026-09-03): Phase 2 symmetric-
/// syscall-error pin for **fs_read** (sequential).  Same fd-lookup-
/// miss shape as fs_read_at's variant: bogus fd → both sides return
/// FSERR_CLOSED symmetrically without journaling.  fs_read differs
/// from fs_read_at only in that its shadow-position increment
/// happens *after* the syscall (see `handlers.rs::fs_read`'s
/// with_mut), so a fd-lookup-miss short-circuits before the
/// position touchpoint — this pin ensures the short-circuit is
/// symmetric across leader/follower and doesn't spuriously fire
/// CONSENSUS_DIVERGENCE.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_read_symmetric_syscall_error_finalizes_to_failure() {
    let _dir = tempfile::tempdir().unwrap();
    let (mut leader, mut follower) = create_leader_and_follower().await;

    // Bogus fd — sequential read from a nonexistent fd.  Both
    // sides fd-lookup-miss → FSERR_CLOSED, no WAL entry.
    let term = r#"
            new fsRead(`rho:io:fs:native:1.0.0/read`), ackCh in {
              fsRead!(999, 10, *ackCh) |
              for (@_ <- ackCh) { Nil }
            }
        "#;
    let r = Blake2b512Random::create_from_bytes(&[142; 32]);

    leader
        .evaluate(
            term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_read symmetric error");
    let leader_wal = leader.fs_handles.wal.snapshot();

    let checkpoint = leader.create_checkpoint().await;
    follower
        .reset(&checkpoint.root)
        .await
        .expect("follower reset");
    follower.rig(checkpoint.log).await.expect("follower rig");
    follower
        .evaluate(
            term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r,
        )
        .await
        .expect("follower evaluate fs_read symmetric error");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal, follower_wal,
        "Phase 2 fs_read symmetric-error: WALs must be byte-identical. \
             A regression that fired CONSENSUS_DIVERGENCE on the symmetric \
             FSERR_CLOSED (or that journaled on fd-lookup-miss, or that \
             advanced shadow position before the syscall gate) would fail here."
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on symmetric syscall error");
}

/// Phase 2 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_entries positive path**.  Path-based readdir with sort +
/// cap.  Sort makes the reply deterministic given identical dir
/// state — the row order is bytewise stable across leader and
/// follower.  Follower re-executes readdir + sort + entry_stat_row
/// against its own subdir; hashes match → WAL byte-identity
/// preserved.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_entries_reexecute_matches_leader_on_identical_state() {
    let dir = tempfile::tempdir().unwrap();
    // Create a subdir with three files at known names so the
    // sorted readdir is deterministic.
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/alpha"), b"a-content").unwrap();
    std::fs::write(dir.path().join("sub/beta"), b"b-content-longer").unwrap();
    std::fs::write(dir.path().join("sub/gamma"), b"c").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    // fs_entries(root, "sub", "consensus", ack) — returns a sorted
    // list of stat_records for [alpha, beta, gamma].
    let term = format!(
        r#"
            new fsEntries(`rho:io:fs:native:1.0.0/entries`), ackCh in {{
              fsEntries!("{root}", "sub", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[61; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_entries positive path");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_entries_entries: Vec<_> = leader_wal
        .iter()
        .filter(|e| e.op == WalOp::Entries)
        .collect();
    assert_eq!(
        leader_entries_entries.len(),
        1,
        "expected exactly one Entries WAL entry from the leader"
    );
    assert_eq!(leader_entries_entries[0].outcome, WalOutcome::Success);

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
        .expect("follower evaluate fs_entries positive path");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal.len(),
        follower_wal.len(),
        "Phase 2: leader/follower WAL lengths diverge for fs_entries \
             positive path: leader={} follower={}",
        leader_wal.len(),
        follower_wal.len()
    );
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(
            l, f,
            "Phase 2: WAL entry {i} differs between leader and follower on \
                 fs_entries positive path: leader={l:?} follower={f:?}"
        );
    }
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on identical dir state");
}

/// Phase 2 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_entries divergence-detection path**.  Add a new file to
/// the directory between leader + follower `evaluate` calls.
/// The follower's re-executed readdir sees the extra entry →
/// row list has different length → verify hash-mismatch → Entries
/// WAL entry carries `Failure { FSERR_CODE_CONSENSUS_DIVERGENCE }`
/// AND `check_replay_data` returns Err.
///
/// Also demonstrates that the sort makes readdir-order agnostic:
/// the ADDED file could land in any position on the follower's
/// readdir but the sort places it deterministically — the
/// divergence is about the CONTENT (a new row exists), not
/// about ordering flakiness.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_entries_reexecute_detects_divergence() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_CONSENSUS_DIVERGENCE;

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub/alpha"), b"a").unwrap();
    std::fs::write(dir.path().join("sub/beta"), b"b").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsEntries(`rho:io:fs:native:1.0.0/entries`), ackCh in {{
              fsEntries!("{root}", "sub", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[62; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_entries divergence setup");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_entries = leader_wal
        .iter()
        .find(|e| e.op == WalOp::Entries && e.outcome == WalOutcome::Success)
        .expect("leader must have journaled a successful Entries entry");

    // Force divergence by adding a new file to the subdir.
    // Follower's readdir sees [alpha, beta, gamma_new] whereas
    // leader's cached reply is [alpha, beta] → row-count
    // differs, hash mismatch.
    std::fs::write(dir.path().join("sub/gamma_new"), b"added-post-leader").unwrap();

    let checkpoint = leader.create_checkpoint().await;
    follower
        .reset(&checkpoint.root)
        .await
        .expect("follower reset");
    follower.rig(checkpoint.log).await.expect("follower rig");
    let _ = follower
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r,
        )
        .await;
    let follower_wal = follower.fs_handles.wal.snapshot();

    let follower_entries = follower_wal.iter().find(|e| e.op == WalOp::Entries).expect(
        "Phase 2 divergence path must still journal an Entries entry \
                 (Failure outcome, not a journaling skip)",
    );
    match follower_entries.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_CONSENSUS_DIVERGENCE,
            "Phase 2: Entries divergence WAL entry must carry \
                 CONSENSUS_DIVERGENCE code — got code {code}"
        ),
        WalOutcome::Success => panic!(
            "Phase 2 REGRESSION: follower's re-executed fs_entries produced \
                 a Success outcome despite the on-disk divergence.  Either the \
                 fresh-syscall path is not engaged (Phase-0 tautological cached-\
                 reply consumption came back) or verify_reply_hash_matches_cached \
                 is broken.  Entries entry: {follower_entries:?}"
        ),
    }
    // WAL-layer divergence witness (2026-09-01 gap-3 fix):
    // journal_state_read on Entries op hashes the reply Par as
    // payload_ref.  Leader's Success entry carries
    // Hash(ok_list([alpha, beta])); follower's Failure entry
    // carries Hash([false, "FSERR_CONSENSUS_DIVERGENCE", msg]).
    // Distinct Pars → distinct hashes.  This assertion proves
    // the divergence is visible at the WAL layer, not just at
    // the RSpace-rig layer (check_replay_data below).
    assert_ne!(
        follower_entries.payload_ref, leader_entries.payload_ref,
        "Phase 2: follower's Entries divergence WAL entry payload_ref \
             must differ from leader's — leader hashed the ok_list reply, \
             follower hashed the divergence-err reply.  A regression that \
             either reused leader's cached hash or emitted a payload_ref: \
             None (mixing journal_state_read + journal_read_divergence \
             conventions) would fail this pin."
    );

    let rig_result = follower.check_replay_data().await;
    assert!(
        rig_result.is_err(),
        "Phase 2 D1 enforcement: divergent fs_entries reply Par should trip \
             RSpace rig verification"
    );
}

/// Phase 5 coverage-review addendum (2026-09-02): **fs_entries
/// symmetric syscall error**.  Attempt fs_entries on a
/// non-existent directory on both sides → both see ENOENT →
/// FSERR_NOT_FOUND → verify OK → both finalize identically,
/// NOT CONSENSUS_DIVERGENCE.  Parity with the fs_stat symmetric-
/// error pin.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_entries_symmetric_syscall_error_finalizes_to_failure() {
    let dir = tempfile::tempdir().unwrap();
    // No subdirectory at "missing_dir".

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsEntries(`rho:io:fs:native:1.0.0/entries`), ackCh in {{
              fsEntries!("{root}", "missing_dir", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[132; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_entries symmetric error");
    let leader_wal = leader.fs_handles.wal.snapshot();

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
        .expect("follower evaluate fs_entries symmetric error");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal, follower_wal,
        "Phase 5 fs_entries symmetric-error: WALs must be byte-identical. \
             A regression that spuriously fired CONSENSUS_DIVERGENCE on the \
             symmetric FSERR_NOT_FOUND would fail here."
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on symmetric syscall error");
}

/// Phase 2 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_entries empty-directory edge case**.  Under Phase-0, an
/// empty-dir fs_entries surfaced a `BugFoundError` when the
/// per-entry supplement charge fired at n_entries=0 via
/// `reserve_primitive` (see the 2026-08-26 metering audit fix
/// that switched both branches to `reserve_incremental_primitive`
/// with its zero-cost early-return).  Under Phase 2, the same
/// switch is preserved on the Consensus follower's re-execute
/// path.  This pin exercises the empty-dir shape end-to-end
/// (WAL byte-identity + Success outcome + verify OK on n=0)
/// so a regression that reverted either branch to
/// `reserve_primitive` — or that broke the ok_list([]) →
/// n_entries=0 extraction chain — would surface here as an
/// evaluate error or WAL divergence.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_entries_reexecute_handles_empty_directory() {
    let dir = tempfile::tempdir().unwrap();
    // Create the sub directory with NO entries.  fs_entries on
    // "empty_sub" must return ok_list([]) (n_entries = 0).
    std::fs::create_dir(dir.path().join("empty_sub")).unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsEntries(`rho:io:fs:native:1.0.0/entries`), ackCh in {{
              fsEntries!("{root}", "empty_sub", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[63; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_entries empty-dir");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_entries: Vec<_> = leader_wal
        .iter()
        .filter(|e| e.op == WalOp::Entries)
        .collect();
    assert_eq!(
        leader_entries.len(),
        1,
        "empty-dir fs_entries must journal exactly one Entries entry \
             (regression against the 2026-08-26 metering audit fix that \
             prevented BugFoundError on n=0 from suppressing the journal)"
    );
    assert_eq!(leader_entries[0].outcome, WalOutcome::Success);

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
        .expect("follower evaluate fs_entries empty-dir");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(leader_wal.len(), follower_wal.len());
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(
            l, f,
            "Phase 2 empty-dir edge: WAL entry {i} differs between leader \
                 and follower — a regression against reserve_incremental_primitive \
                 in the Consensus branch would show here as follower's Entries \
                 entry missing or diverging: leader={l:?} follower={f:?}"
        );
    }
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on empty-dir readdir");
}

/// Phase 3 pin (Consensus re-execute + verify, 2026-09-01):
/// **Dedicated fs_seek Consensus real-lseek prerequisite pin**.
/// Under Phase 3, follower's fs_seek is_replay Consensus branch
/// must call `libc::lseek` on the shadow's real fd — not just
/// update the shadow position tracker — so that subsequent
/// Phase-2/3 fd ops (fs_read / fs_write) find the OS-fd
/// position where the leader left it.
///
/// Test shape: open a Consensus cap on a 16-byte file with
/// known content ("0123456789abcdef"), fs_seek(offset=8, SET),
/// fs_read(4 bytes).  Leader reads "89ab" at OS-fd position 8.
/// Follower's fs_seek Phase-3 real-lseek moves the follower's
/// OS-fd position to 8; follower's fs_read Phase-2 re-execute
/// reads "89ab" from position 8.  verify OK → WAL byte-identity.
///
/// A regression that dropped the fs_seek real-lseek (kept only
/// the shadow-position update) would leave follower's OS-fd
/// position at 0 → fs_read reads "0123" instead of "89ab" →
/// verify hash-mismatch → CONSENSUS_DIVERGENCE.  This pin fails
/// at the byte-identity check, pointing directly at the fs_seek
/// site (Read entry's payload_ref would differ).
///
/// Coverage complement to `wal_position_stays_in_sync_on_leader_
/// and_follower` (indirect multi-op test): this pin isolates the
/// fs_seek OS-fd-position mutation specifically.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_seek_reexecute_moves_follower_os_fd_position() {
    let dir = tempfile::tempdir().unwrap();
    // Known 16-byte content: bytes at position 8..12 are
    // "89ab" (ASCII 0x38 0x39 0x61 0x62).
    std::fs::write(dir.path().join("data.bin"), b"0123456789abcdef").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsSeek(`rho:io:fs:native:1.0.0/seek`),
                fsRead(`rho:io:fs:native:1.0.0/read`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, sc, rc, cc
            in {{
              fsOpen!("{root}", "data.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsSeek!(fd, 8, "set", *sc) |
                for (@_ <- sc) {{
                  fsRead!(fd, 4, *rc) |
                  for (@_ <- rc) {{
                    fsClose!(fd, *cc) |
                    for (@_ <- cc) {{ Nil }}
                  }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[94; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_seek dedicated");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_read = leader_wal
        .iter()
        .find(|e| e.op == WalOp::Read)
        .expect("leader must have journaled a Read entry");
    assert_eq!(
        leader_read.offset,
        Some(8),
        "leader's Read WAL entry must record offset=8 (post-seek shadow \
             position)"
    );
    assert_eq!(leader_read.length, Some(4));
    assert_eq!(leader_read.outcome, WalOutcome::Success);

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
        .expect("follower evaluate fs_seek dedicated");
    let follower_wal = follower.fs_handles.wal.snapshot();

    // Load-bearing: WAL byte-identity across all entries.  The
    // Read entry's payload_ref = Hash(bytes-read).  If the
    // follower's OS-fd position hadn't been moved by the
    // real-lseek fix, follower's libc::read from position 0
    // returns "0123" → Hash("0123") differs from leader's
    // Hash("89ab") → assertion fails at the Read entry.
    assert_eq!(
        leader_wal.len(),
        follower_wal.len(),
        "Phase 3 fs_seek dedicated: leader/follower WAL count differs — \
             likely the follower's Read Consensus branch diverged after \
             fs_seek failed to move the OS-fd position"
    );
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(
            l, f,
            "Phase 3 fs_seek REGRESSION: WAL entry {i} differs between \
                 leader and follower.  Most likely fs_seek's Consensus is_replay \
                 branch stopped calling libc::lseek on the shadow's real fd \
                 (only updated the shadow-position tracker) — the follower's \
                 subsequent fs_read reads from OS-fd position 0 instead of the \
                 seeked position, returning wrong bytes and tripping \
                 CONSENSUS_DIVERGENCE.  leader={l:?} follower={f:?}"
        );
    }
    follower
        .check_replay_data()
        .await
        .expect("replay data must match — fs_seek's real-lseek fires");
}

/// B1 pin (Phase 5 branch-review addendum, 2026-09-04 —
/// **A2-F-1**): **fs_seek Consensus re-execute divergence
/// detection**.  Companion to
/// `consensus_fs_seek_reexecute_moves_follower_os_fd_position`
/// (the matches case).  Every other Phase-5 handler pairs a
/// matches test with an explicit `_reexecute_detects_divergence`
/// regression pin (14 handlers, 14 divergence tests at fs_wal_
/// spec.rs lines 1468, 1778, 2176, 2685, 2951, 3454, 3943, 4428,
/// 4737, 5010, 5274, 5544, 5801, 6061).  fs_seek was the sole
/// exception before this slice — a refactor that turned the B1
/// verify call (handlers.rs:2407 `verify_reply_hash_matches_
/// cached`) into a no-op, or that swapped back to the pre-B1
/// tautological `produce(&previous, ack)` shape (position-follow-
/// up 2026-08-26), would pass CI silently.
///
/// Test shape: 16-byte file, leader fsOpen(cons) + fsSeek(0,
/// SEEK_END) → leader records `[true, 16]` as the cached reply
/// (no WAL entry — fs_seek is not journaled).  Grow the file to
/// 32 bytes between leader.evaluate and follower.evaluate.
/// Follower's fs_seek is_replay Consensus branch calls the real
/// libc::lseek → returns 32 → builds `ok_u64(32)` → verify_reply_
/// hash_matches_cached fires mismatch → follower produces
/// `err(FSERR_CONSENSUS_DIVERGENCE, ...)`.  Enforcement side:
/// the divergent reply Par does NOT match the leader's cached
/// `[true, 16]` produce, so RSpace's rig comparator reports
/// divergence — `check_replay_data()` returns Err.  Under
/// Casper, this manifests as block rejection.
///
/// Because fs_seek is NOT journaled (no `WalOp::Seek`), the
/// primary assertion is on `check_replay_data()` rather than a
/// WAL Failure entry — differs from the fs_stat / fs_size /
/// fs_read / fs_write / fs_chmod / ... divergence tests which
/// double-check the WAL Failure code.  Both leader and follower
/// WALs stay empty for this term (fsOpen and fsClose don't
/// journal either), so a WAL-emptiness cross-check is bundled
/// in to catch a hypothetical future WalOp::Seek addition that
/// forgot to update the test's expectations.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_seek_reexecute_detects_divergence() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("data.bin");
    // Leader sees a 16-byte file → SEEK_END returns 16.
    std::fs::write(&target, b"0123456789abcdef").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsSeek(`rho:io:fs:native:1.0.0/seek`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, sc, cc
            in {{
              fsOpen!("{root}", "data.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsSeek!(fd, 0, "end", *sc) |
                for (@_ <- sc) {{
                  fsClose!(fd, *cc) |
                  for (@_ <- cc) {{ Nil }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[95; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate B1 fs_seek divergence setup");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert!(
        leader_wal.is_empty(),
        "fs_open + fs_seek + fs_close are not journaled — leader WAL \
             must stay empty for this term.  If this fails, a WalOp::Seek \
             (or Open / Close) variant was added and this test's shape \
             needs to be updated to check for the specific Failure code \
             on the divergence branch; got {leader_wal:?}",
    );

    // Grow the file between leader and follower — follower's
    // SEEK_END will now return 32 instead of 16 → verify_reply_
    // hash_matches_cached fails → FSERR_CONSENSUS_DIVERGENCE reply.
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&target)
            .expect("open target for append");
        f.write_all(b"-follower-sees-more-bytes-here-")
            .expect("append to target");
    }

    let checkpoint = leader.create_checkpoint().await;
    follower
        .reset(&checkpoint.root)
        .await
        .expect("follower reset");
    follower.rig(checkpoint.log).await.expect("follower rig");
    // The evaluate itself may return Ok even though the produce
    // diverges — the divergent produce is caught by
    // `check_replay_data` below.  Same discipline as
    // consensus_fs_stat_reexecute_detects_divergence at line 1468.
    let _ = follower
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r,
        )
        .await;
    let follower_wal = follower.fs_handles.wal.snapshot();
    assert!(
        follower_wal.is_empty(),
        "follower WAL must also stay empty — divergence surfaces via \
             the reply Par, not the WAL, for the non-journaled fs_seek op; \
             got {follower_wal:?}",
    );

    // Primary enforcement: the follower's fs_seek produces
    // `err(FSERR_CONSENSUS_DIVERGENCE, ...)` at the seek ack
    // channel; leader cached `[true, 16]` at the same channel.
    // RSpace's rig comparator flags the mismatched produce →
    // check_replay_data returns Err.  Under Casper, this
    // manifests as block rejection at the follower.
    let rig_result = follower.check_replay_data().await;
    assert!(
        rig_result.is_err(),
        "B1 enforcement: divergent fs_seek reply Par should trip \
             RSpace rig verification — got Ok, which means the \
             follower's produce matched the leader's cached [true, 16] \
             despite SEEK_END returning 32 on the grown file.  Either \
             the Consensus fresh-syscall path is not engaged (regressed \
             to the pre-B1 tautological cached-reply consumption from \
             the 2026-08-26 position-follow-up) or verify_reply_hash_ \
             matches_cached is broken.  This would silently accept a \
             leader lie."
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_on_consensus_cap_appends_read_wal_entry() {
    let dir = tempfile::tempdir().unwrap();
    let payload = b"consensus read payload";
    std::fs::write(dir.path().join("data.bin"), payload).unwrap();
    let runtime = create_runtime().await;
    assert!(runtime.fs_handles.wal.is_empty());

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsRead(`rho:io:fs:native:1.0.0/read`),
                oc, rc
            in {{
              fsOpen!("{root}", "data.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsRead!(fd, 32, *rc) |
                for (@_ <- rc) {{ Nil }}
              }}
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
        .expect("evaluate");

    let entries = runtime.fs_handles.wal.snapshot();
    assert_eq!(entries.len(), 1, "expected 1 Read WAL entry");
    let e = &entries[0];
    assert_eq!(e.op, WalOp::Read);
    // Position-follow-up (2026-08-26): sequential Read on a
    // fresh fd (opened "rw", position=0) records offset=Some(0)
    // — the pre-read shadow position, so a joining validator
    // can verify the read against reconstructed state at the
    // exact position the leader consumed bytes from.  See
    // `FileHandle::position` for the position-tracking model.
    assert_eq!(e.offset, Some(0));
    assert_eq!(e.length, Some(payload.len() as u64));
    let expected_hash: Vec<u8> = Blake2b256::hash(payload.to_vec());
    match &e.payload_ref {
        Some(PayloadRef::Hash(h)) => {
            assert_eq!(&h[..], &expected_hash[..], "read payload hash mismatch")
        }
        other => panic!("expected PayloadRef::Hash, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_at_on_consensus_cap_appends_read_at_entry() {
    let dir = tempfile::tempdir().unwrap();
    // Prepare a file where a positional read at offset 3 length 5
    // returns "world" — pins offset semantics in the WAL entry.
    std::fs::write(dir.path().join("data.bin"), b"foo world bar").unwrap();
    let runtime = create_runtime().await;

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsReadAt(`rho:io:fs:native:1.0.0/readAt`),
                oc, rc
            in {{
              fsOpen!("{root}", "data.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsReadAt!(fd, 4, 5, *rc) |
                for (@_ <- rc) {{ Nil }}
              }}
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

    let entries = runtime.fs_handles.wal.snapshot();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].op, WalOp::ReadAt);
    assert_eq!(entries[0].offset, Some(4));
    assert_eq!(entries[0].length, Some(5));
    let expected_hash: Vec<u8> = Blake2b256::hash(b"world".to_vec());
    match &entries[0].payload_ref {
        Some(PayloadRef::Hash(h)) => assert_eq!(&h[..], &expected_hash[..]),
        other => panic!("expected Hash, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_on_oracular_cap_does_not_append_wal() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data.bin"), b"anything").unwrap();
    let runtime = create_runtime().await;

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsRead(`rho:io:fs:native:1.0.0/read`),
                oc, rc
            in {{
              fsOpen!("{root}", "data.bin", "r", "oracular", *oc) |
              for (@[true, fd] <- oc) {{
                fsRead!(fd, 32, *rc) |
                for (@_ <- rc) {{ Nil }}
              }}
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
    assert!(
        runtime.fs_handles.wal.is_empty(),
        "oracular reads must not journal; got {:?}",
        runtime.fs_handles.wal.snapshot()
    );
}

/// Slice 32 core invariant: leader and follower produce byte-
/// identical WAL entries for reads.  The follower's `is_replay`
/// branch does NOT re-execute the syscall — it extracts the
/// leader's returned bytes from the tuplespace `previous` cache,
/// re-hashes them, and appends a matching `Read` entry.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_wal_is_byte_identical_on_leader_and_follower() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data.bin"), b"leader/follower read parity").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsRead(`rho:io:fs:native:1.0.0/read`),
                fsReadAt(`rho:io:fs:native:1.0.0/readAt`),
                oc, r1, r2
            in {{
              fsOpen!("{root}", "data.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsRead!(fd, 8, *r1) |
                for (@_ <- r1) {{
                  fsReadAt!(fd, 12, 4, *r2) |
                  for (@_ <- r2) {{ Nil }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let rand = Blake2b512Random::create_from_bytes(&[42; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            rand.clone(),
        )
        .await
        .expect("leader evaluate");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert!(
        !leader_wal.is_empty(),
        "leader must have journaled Read entries"
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
            rand,
        )
        .await
        .expect("follower evaluate");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal.len(),
        follower_wal.len(),
        "leader {} Read/ReadAt entries; follower {} — the follower's \
             is_replay branch must re-hash the cached bytes and append a \
             matching WAL entry, or on-chain WAL roots will diverge",
        leader_wal.len(),
        follower_wal.len()
    );
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(l, f, "WAL entry {i}: leader={l:?}, follower={f:?}");
    }

    follower
        .check_replay_data()
        .await
        .expect("tuplespace replay must match");
}

/// EOF / zero-byte read still produces a WAL entry — an
/// observation of "the file is short" is itself consensus-
/// relevant (a follower whose reconstructed file is LONGER
/// than the leader saw must fail replay).  Empty payload's
/// Blake2b256 is well-defined and canonical.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn read_returning_zero_bytes_still_appends_wal_entry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("empty.bin"), b"").unwrap();
    let runtime = create_runtime().await;

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsRead(`rho:io:fs:native:1.0.0/read`),
                oc, rc
            in {{
              fsOpen!("{root}", "empty.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsRead!(fd, 16, *rc) |
                for (@_ <- rc) {{ Nil }}
              }}
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

    let entries = runtime.fs_handles.wal.snapshot();
    assert_eq!(
        entries.len(),
        1,
        "zero-byte read must still produce a WAL entry (EOF is consensus-observable)"
    );
    assert_eq!(entries[0].op, WalOp::Read);
    assert_eq!(entries[0].length, Some(0));
    let expected_empty_hash: Vec<u8> = Blake2b256::hash(Vec::new());
    match &entries[0].payload_ref {
        Some(PayloadRef::Hash(h)) => {
            assert_eq!(&h[..], &expected_empty_hash[..], "empty-payload hash canon")
        }
        other => panic!("expected Hash of empty, got {other:?}"),
    }
}

/// Failed read (bad fd) must NOT append a WAL entry.  Mirrors
/// slice 29's `failed_write_does_not_append_wal_entry` invariant:
/// only successful observations are consensus-relevant.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failed_read_does_not_append_wal_entry() {
    let runtime = create_runtime().await;
    let term = r#"
            new fsRead(`rho:io:fs:native:1.0.0/read`), rc in {
              fsRead!(999999, 32, *rc) |
              for (@_ <- rc) { Nil }
            }
        "#;
    runtime
        .evaluate(
            term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            rand(),
        )
        .await
        .unwrap();
    assert!(
        runtime.fs_handles.wal.is_empty(),
        "failed read (unknown fd) must not journal"
    );
}
