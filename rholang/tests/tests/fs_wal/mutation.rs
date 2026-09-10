// Mutation family — M tests.
//
// Wave-3 S3.14 (2026-09-10) — split out of `fs_wal_spec.rs` via
// `#[path]` submodule.  See parent module `fs_wal_spec::tests`
// for shared setup (create_runtime, create_leader_and_follower,
// rand, assert_dir_trees_byte_identical, translate_path,
// apply_wal_translated).

use super::*;

/// A Consensus-cap write must append a `Write` WAL entry whose
/// payload_ref is `Hash(blake2b256(payload))`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_on_consensus_cap_appends_wal_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.bin");
    std::fs::write(&path, b"").unwrap();

    let runtime = create_runtime().await;
    assert!(runtime.fs_handles.wal.is_empty());

    let payload = b"hello world";
    // Rholang: open with cmode="consensus", write payload, capture reply.
    // The bytes literal below must match `payload` exactly for the
    // hash assertion below.
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                openCh, writeCh, closeCh
            in {{
              fsOpen!("{root}", "data.bin", "r+", "consensus", *openCh) |
              for (@[true, fd] <- openCh) {{
                fsWrite!(fd, "68656c6c6f20776f726c64".hexToBytes(), *writeCh) |
                for (@_ <- writeCh) {{
                  fsClose!(fd, *closeCh)
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
        .expect("evaluate must succeed");

    // Inspect WAL.
    let entries = runtime.fs_handles.wal.snapshot();
    assert_eq!(
        entries.len(),
        1,
        "expected 1 WAL entry, got {}",
        entries.len()
    );
    let e = &entries[0];
    assert_eq!(e.op, WalOp::Write);
    // Position-follow-up (2026-08-26): sequential Write on a
    // fresh fd (opened "rw", position=0) records offset=Some(0)
    // — the pre-write shadow position pulled from FileHandle
    // in journal_write.  Pre-position-follow-up this was None;
    // the change unblocks the fresh-tree WAL applier from
    // reconstructing sequential-write file state (see
    // `apply_wal_to_fresh_tree` in
    // `rholang::interpreter::io::wal_applier`).
    assert_eq!(e.offset, Some(0));
    assert_eq!(e.length, Some(payload.len() as u64));
    // Payload hash must match Blake2b256 of the actual bytes.
    let expected_hash: Vec<u8> = Blake2b256::hash(payload.to_vec());
    match &e.payload_ref {
        Some(PayloadRef::Hash(h)) => {
            assert_eq!(&h[..], &expected_hash[..], "payload hash mismatch")
        }
        other => panic!("expected PayloadRef::Hash, got {other:?}"),
    }
}

/// Phase 7b-2 (2026-08-27): a Consensus-cap write with an
/// attached `PayloadPersistence` MUST persist the bytes
/// content-addressed under the WAL entry's `Hash(...)` key,
/// so joining validators can fetch them via the wire protocol.
/// Oracular caps must NOT persist (they never journal, so
/// there's no hash to key off of).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_write_persists_bytes_to_attached_payload_store() {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use rholang::rust::interpreter::io::wal::PayloadPersistence;

    /// Minimal test double — records every persist call and
    /// echoes the computed hash back like the real store.
    #[derive(Debug, Default)]
    struct RecordingStore {
        calls: Mutex<HashMap<[u8; 32], Vec<u8>>>,
    }
    impl PayloadPersistence for RecordingStore {
        fn persist(&self, bytes: &[u8]) -> Result<[u8; 32], String> {
            let h: Vec<u8> = Blake2b256::hash(bytes.to_vec());
            let mut buf = [0u8; 32];
            buf.copy_from_slice(&h);
            self.calls.lock().unwrap().insert(buf, bytes.to_vec());
            Ok(buf)
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.bin");
    std::fs::write(&path, b"").unwrap();

    let runtime = create_runtime().await;
    let store = Arc::new(RecordingStore::default());
    runtime
        .fs_handles
        .share_payload_store(Some(store.clone() as Arc<dyn PayloadPersistence>));

    let payload = b"hello world";
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                openCh, writeCh, closeCh
            in {{
              fsOpen!("{root}", "data.bin", "r+", "consensus", *openCh) |
              for (@[true, fd] <- openCh) {{
                fsWrite!(fd, "68656c6c6f20776f726c64".hexToBytes(), *writeCh) |
                for (@_ <- writeCh) {{
                  fsClose!(fd, *closeCh)
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
        .expect("evaluate must succeed");

    // The store must contain the exact bytes under the
    // Blake2b256 hash the WAL entry references.
    let expected_hash: Vec<u8> = Blake2b256::hash(payload.to_vec());
    let mut expected = [0u8; 32];
    expected.copy_from_slice(&expected_hash);
    let calls = store.calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "expected exactly one persist call");
    assert_eq!(
        calls.get(&expected).map(|v| v.as_slice()),
        Some(payload.as_ref())
    );
    // Sanity: WAL entry keys off the same hash.
    let entries = runtime.fs_handles.wal.snapshot();
    assert_eq!(entries.len(), 1);
    match &entries[0].payload_ref {
        Some(PayloadRef::Hash(h)) => assert_eq!(*h, expected),
        other => panic!("expected PayloadRef::Hash, got {other:?}"),
    }
}

/// Phase 7b-2 review-pin (2026-08-27): a Consensus-cap write
/// on a runtime with NO payload store attached MUST still
/// journal the WAL entry.  Sanity check that the persist hook
/// is a no-op when the field is None (test harnesses and
/// observer nodes hit this path).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_write_without_payload_store_still_journals() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.bin");
    std::fs::write(&path, b"").unwrap();

    let runtime = create_runtime().await;
    // No share_payload_store call — the field stays None.
    assert!(runtime.fs_handles.payload_store().is_none());

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                openCh, writeCh
            in {{
              fsOpen!("{root}", "data.bin", "r+", "consensus", *openCh) |
              for (@[true, fd] <- openCh) {{
                fsWrite!(fd, "68656c6c6f".hexToBytes(), *writeCh) |
                for (@_ <- writeCh) {{ Nil }}
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
        .expect("evaluate must succeed even without a payload store");

    // WAL entry MUST land regardless of persistence.
    let entries = runtime.fs_handles.wal.snapshot();
    assert_eq!(
        entries.len(),
        1,
        "Consensus write must journal even without a payload store"
    );
    assert_eq!(entries[0].op, WalOp::Write);
}

/// Phase 7b-2 review-pin (2026-08-27): a `persist(bytes)` call
/// that returns `Err(...)` MUST be logged and swallowed — the
/// deploy continues, the WAL entry lands.  Otherwise a
/// transient disk-full or permission-denied error on the
/// payload dir would abort the leader mid-deploy, forking
/// consensus with peers whose disks are still healthy.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn persist_error_does_not_abort_consensus_write() {
    use std::sync::{Arc, Mutex};

    use rholang::rust::interpreter::io::wal::PayloadPersistence;

    #[derive(Debug, Default)]
    struct FailingStore {
        calls: Mutex<usize>,
    }
    impl PayloadPersistence for FailingStore {
        fn persist(&self, _bytes: &[u8]) -> Result<[u8; 32], String> {
            *self.calls.lock().unwrap() += 1;
            Err("simulated disk failure".to_string())
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.bin");
    std::fs::write(&path, b"").unwrap();

    let runtime = create_runtime().await;
    let store = Arc::new(FailingStore::default());
    runtime
        .fs_handles
        .share_payload_store(Some(store.clone() as Arc<dyn PayloadPersistence>));

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                openCh, writeCh
            in {{
              fsOpen!("{root}", "data.bin", "r+", "consensus", *openCh) |
              for (@[true, fd] <- openCh) {{
                fsWrite!(fd, "ff00".hexToBytes(), *writeCh) |
                for (@_ <- writeCh) {{ Nil }}
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
            "persist Err MUST NOT abort the deploy — that would fork consensus with \
                 peers whose payload dir is healthy",
        );

    // persist was called at least once (may be 2 if partial-write finalize also fires).
    assert!(
        *store.calls.lock().unwrap() >= 1,
        "persist was expected to be called on the Consensus write"
    );
    // WAL entry still lands — the deploy proceeded past the
    // failed persist.
    let entries = runtime.fs_handles.wal.snapshot();
    assert_eq!(entries.len(), 1, "WAL entry MUST land even on persist Err");
    assert_eq!(entries[0].op, WalOp::Write);
}

/// Phase 7b-2 (2026-08-27): an Oracular-cap write on a
/// runtime with an attached payload store must NOT persist —
/// Oracular caps never journal (no WAL entry, no hash to key
/// off of, no fetchable payload for joiners).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn oracular_write_does_not_persist_to_attached_payload_store() {
    use std::sync::{Arc, Mutex};

    use rholang::rust::interpreter::io::wal::PayloadPersistence;

    #[derive(Debug, Default)]
    struct CountingStore {
        n: Mutex<usize>,
    }
    impl PayloadPersistence for CountingStore {
        fn persist(&self, _bytes: &[u8]) -> Result<[u8; 32], String> {
            *self.n.lock().unwrap() += 1;
            Ok([0u8; 32])
        }
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.bin");
    std::fs::write(&path, b"").unwrap();

    let runtime = create_runtime().await;
    let store = Arc::new(CountingStore::default());
    runtime
        .fs_handles
        .share_payload_store(Some(store.clone() as Arc<dyn PayloadPersistence>));

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                openCh, writeCh
            in {{
              fsOpen!("{root}", "data.bin", "r+", "oracular", *openCh) |
              for (@[true, fd] <- openCh) {{
                fsWrite!(fd, "abcd".hexToBytes(), *writeCh) |
                for (@_ <- writeCh) {{ Nil }}
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

    assert_eq!(
        *store.n.lock().unwrap(),
        0,
        "oracular cap must not call persist; got {} calls",
        *store.n.lock().unwrap()
    );
}

/// An Oracular-cap write must NOT produce any WAL entry.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_on_oracular_cap_does_not_append_wal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.bin");
    std::fs::write(&path, b"").unwrap();

    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                openCh, writeCh
            in {{
              fsOpen!("{root}", "data.bin", "r+", "oracular", *openCh) |
              for (@[true, fd] <- openCh) {{
                fsWrite!(fd, "abcd".hexToBytes(), *writeCh) |
                for (@_ <- writeCh) {{ Nil }}
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
        "oracular cap must not append WAL entries; got {:?}",
        runtime.fs_handles.wal.snapshot()
    );
}

/// writeAt on Consensus produces a `WriteAt` entry with the
/// offset populated.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn write_at_on_consensus_cap_appends_write_at_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.bin");
    std::fs::write(&path, vec![0u8; 100]).unwrap();

    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWriteAt(`rho:io:fs:native:1.0.0/writeAt`),
                openCh, writeCh
            in {{
              fsOpen!("{root}", "data.bin", "r+", "consensus", *openCh) |
              for (@[true, fd] <- openCh) {{
                fsWriteAt!(fd, 42, "cafe".hexToBytes(), *writeCh) |
                for (@_ <- writeCh) {{ Nil }}
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
    assert_eq!(entries[0].op, WalOp::WriteAt);
    assert_eq!(entries[0].offset, Some(42));
    assert_eq!(entries[0].length, Some(2)); // 0xCAFE = 2 bytes
}

/// Truncate on Consensus produces a `Truncate` entry with offset
/// = new length.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn truncate_on_consensus_cap_appends_truncate_entry() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.bin");
    std::fs::write(&path, vec![0u8; 100]).unwrap();

    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsTruncate(`rho:io:fs:native:1.0.0/truncate`),
                openCh, truncCh
            in {{
              fsOpen!("{root}", "data.bin", "r+", "consensus", *openCh) |
              for (@[true, fd] <- openCh) {{
                fsTruncate!(fd, 10, *truncCh) |
                for (@_ <- truncCh) {{ Nil }}
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
    assert_eq!(entries[0].op, WalOp::Truncate);
    assert_eq!(entries[0].offset, Some(10));
    assert_eq!(entries[0].length, None);
    assert!(entries[0].payload_ref.is_none());
}

/// Multiple mutations produce WAL entries in insertion order.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn multiple_mutations_append_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.bin");
    std::fs::write(&path, vec![0u8; 100]).unwrap();

    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                fsWriteAt(`rho:io:fs:native:1.0.0/writeAt`),
                fsTruncate(`rho:io:fs:native:1.0.0/truncate`),
                openCh, w1, w2, w3
            in {{
              fsOpen!("{root}", "data.bin", "r+", "consensus", *openCh) |
              for (@[true, fd] <- openCh) {{
                fsWrite!(fd, "aa".hexToBytes(), *w1) |
                for (@_ <- w1) {{
                  fsWriteAt!(fd, 5, "bb".hexToBytes(), *w2) |
                  for (@_ <- w2) {{
                    fsTruncate!(fd, 20, *w3) |
                    for (@_ <- w3) {{ Nil }}
                  }}
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
        .unwrap();
    let entries = runtime.fs_handles.wal.snapshot();
    assert_eq!(
        entries.len(),
        3,
        "expected 3 entries, got {}",
        entries.len()
    );
    assert_eq!(entries[0].op, WalOp::Write);
    assert_eq!(entries[1].op, WalOp::WriteAt);
    assert_eq!(entries[2].op, WalOp::Truncate);
}

/// Mixing Oracular and Consensus caps in one runtime — only the
/// Consensus caps produce WAL entries.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_consensus_caps_produce_wal_entries() {
    let dir = tempfile::tempdir().unwrap();
    let path_orc = dir.path().join("orc.bin");
    let path_con = dir.path().join("con.bin");
    std::fs::write(&path_orc, b"").unwrap();
    std::fs::write(&path_con, b"").unwrap();

    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                orcCh, conCh, w1, w2
            in {{
              fsOpen!("{root}", "orc.bin", "r+", "oracular", *orcCh) |
              for (@[true, fd_orc] <- orcCh) {{
                fsWrite!(fd_orc, "aa".hexToBytes(), *w1) |
                for (@_ <- w1) {{
                  fsOpen!("{root}", "con.bin", "r+", "consensus", *conCh) |
                  for (@[true, fd_con] <- conCh) {{
                    fsWrite!(fd_con, "bb".hexToBytes(), *w2) |
                    for (@_ <- w2) {{ Nil }}
                  }}
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
        .unwrap();
    let entries = runtime.fs_handles.wal.snapshot();
    assert_eq!(entries.len(), 1, "only consensus-cap write should journal");
    assert_eq!(entries[0].op, WalOp::Write);
    // Path from the consensus cap.
    assert!(
        entries[0]
            .path
            .to_string_lossy()
            .contains(&*dir.path().display().to_string()),
        "path should contain the tempdir root; got {:?}",
        entries[0].path
    );
}

/// A failed write (bad fd) must not append a WAL entry —
/// journaling only happens on successful syscall completion.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failed_write_does_not_append_wal_entry() {
    let runtime = create_runtime().await;
    let term = r#"
            new fsWrite(`rho:io:fs:native:1.0.0/write`), w
            in {
              fsWrite!(999999, "aa".hexToBytes(), *w) |
              for (@_ <- w) { Nil }
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
    assert!(runtime.fs_handles.wal.is_empty());
}

/// H-6 fix regression pin (2026-08-06) — a Consensus-cap
/// write to an fd opened READ-ONLY produces a WAL entry with
/// `outcome = Failure { code = FSERR_CODE_IO }`, NOT a
/// missing entry and NOT a `Success` entry.
///
/// The pre-syscall placeholder pattern (C-29-F1 fix) means
/// every Consensus write appends a WAL entry BEFORE the
/// syscall runs.  H-6's threat model: the syscall fails
/// (EIO/ENOSPC/EROFS or here EBADF from writing to an
/// O_RDONLY fd) — followers must not replay a Write against
/// their own filesystem based on a leader event that never
/// actually happened.  The `outcome = Failure` mark tells
/// them to skip the mutation.
///
/// Bypasses the File.rho mode-cap by binding the
/// `rho:io:fs:native:1.0.0/*` URNs directly (this test file's
/// established pattern; see the top-level module doc).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failed_consensus_write_appends_wal_entry_marked_failure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.bin");
    std::fs::write(&path, b"existing").unwrap();

    let runtime = create_runtime().await;
    assert!(runtime.fs_handles.wal.is_empty());

    // Open the file O_RDONLY (mode = "r") with cmode="consensus".
    // Attempt a write on the resulting fd — libc::write returns
    // -1 with EBADF (write on read-only fd), so the syscall
    // fails while the pre-syscall WAL placeholder is already
    // in place.  H-6 requires that placeholder to be flipped
    // to Failure via finalize_failure_journal.
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                o, w
            in {{
              fsOpen!("{root}", "f.bin", "r", "consensus", *o) |
              for (@[true, fd] <- o) {{
                fsWrite!(fd, "aa".hexToBytes(), *w) |
                for (@_ <- w) {{ Nil }}
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

    let snap = runtime.fs_handles.wal.snapshot();
    assert_eq!(
        snap.len(),
        1,
        "Consensus write on r-only fd MUST leave a WAL entry \
             (pre-syscall placeholder pattern) — got {} entries",
        snap.len()
    );
    // Outcome must be Failure — this is the H-6 assertion.
    // Any Failure code satisfies the invariant; the specific
    // code (EBADF → FSERR_IO via io_err_code's default arm)
    // is a platform detail we don't want to over-pin.
    match snap[0].outcome {
        rholang::rust::interpreter::io::wal::WalOutcome::Failure { code: _ } => {}
        other => panic!(
            "H-6: failed syscall must mark WAL entry as Failure; got {:?}",
            other
        ),
    }
    // Op / path / length preserved so replayers can see
    // WHAT the leader tried to do and diagnostics survive.
    // "aa".hexToBytes() decodes to a single byte 0xAA.
    assert_eq!(snap[0].op, WalOp::Write);
    assert_eq!(snap[0].length, Some(1));
}

/// Phase 3 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_truncate positive path**.  Under Phase-0 the follower's
/// is_replay branch consumed the leader's cached reply and
/// finalized the pre-appended WAL placeholder via the H-6
/// pattern; no real `libc::ftruncate` fired on the follower.
/// Under Phase 3, the follower's is_replay Consensus branch
/// re-executes `libc::ftruncate` against its own shadow fd
/// (installed by fs_open's Phase-2 real-open), verifies the
/// fresh reply's stable_hash matches the leader's cached, and
/// keeps the pre-appended Success WAL entry.
///
/// Truncate's reply is `[true]` — an `ok_bare` with no bytes or
/// numeric payload — so its stable_hash is trivially invariant
/// under identical syscall success on both sides.  A regression
/// that dropped Phase-3 re-execute would leave the follower's
/// file untruncated on its own subdir (Phase-0 tautological
/// path); the file-size check after evaluate proves the real
/// syscall actually fired on the follower.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_truncate_reexecute_matches_leader_on_identical_state() {
    let dir = tempfile::tempdir().unwrap();
    // Both leader and follower see the same file at start:
    // 20 bytes.  Truncate to 8 bytes.  Under Phase 3, the
    // follower's fresh libc::ftruncate reduces its file to 8
    // bytes too — proving the syscall actually ran.
    let target = dir.path().join("data.bin");
    std::fs::write(&target, b"twenty-bytes-of-data").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsTruncate(`rho:io:fs:native:1.0.0/truncate`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, tc, cc
            in {{
              fsOpen!("{root}", "data.bin", "r+", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsTruncate!(fd, 8, *tc) |
                for (@_ <- tc) {{
                  fsClose!(fd, *cc) |
                  for (@_ <- cc) {{ Nil }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[81; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_truncate positive");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_truncates: Vec<_> = leader_wal
        .iter()
        .filter(|e| e.op == WalOp::Truncate)
        .collect();
    assert_eq!(
        leader_truncates.len(),
        1,
        "expected exactly one Truncate WAL entry from the leader"
    );
    assert_eq!(leader_truncates[0].outcome, WalOutcome::Success);
    assert_eq!(
        leader_truncates[0].offset,
        Some(8),
        "Truncate WAL entry records target size in `offset`"
    );
    // Leader's file got truncated by the real ftruncate.
    let leader_bytes = std::fs::read(&target).unwrap();
    assert_eq!(
        leader_bytes.len(),
        8,
        "leader must have truncated the file to 8 bytes"
    );

    // Restore file to pre-play state (leader/follower share the
    // same tempdir under this test-harness pattern; without the
    // restore, follower's re-execute would see a pre-truncated
    // file rather than the pre-play 20-byte state).  Mirrors
    // the fs_size/fs_read_at pattern.
    std::fs::write(&target, b"twenty-bytes-of-data").unwrap();

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
        .expect("follower evaluate fs_truncate positive");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal.len(),
        follower_wal.len(),
        "Phase 3: leader/follower WAL lengths diverge on fs_truncate positive"
    );
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(
            l, f,
            "Phase 3: WAL entry {i} differs on fs_truncate positive: \
                 leader={l:?} follower={f:?}"
        );
    }
    // The load-bearing Phase-3 assertion: the follower's real
    // ftruncate fired.  Post-restore the file was 20 bytes; the
    // follower's re-execute must have truncated it back to 8.
    // A regression that kept the follower on Phase-0 tautological
    // (no real ftruncate) would leave this at 20 → assertion fails.
    let follower_bytes = std::fs::read(&target).unwrap();
    assert_eq!(
        follower_bytes.len(),
        8,
        "Phase 3 REGRESSION: follower's ftruncate did not fire — file is \
             still {} bytes (expected 8).  The follower's is_replay Consensus \
             branch must have reverted to Phase-0 tautological cached-reply \
             consumption.",
        follower_bytes.len()
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on identical truncate state");
}

/// Phase 3 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_truncate divergence-detection path**.  Force a divergence
/// by closing the follower's shadow fd out from under it (via a
/// mid-evaluation `fs_close` on the leader that RSpace preserves
/// as-is on the follower).  Actually simpler: change the file's
/// mode on disk between leader + follower evaluate so the
/// follower's ftruncate returns a different error than the
/// leader.
///
/// Simplest: make the target file read-only between leader (which
/// completed the truncate successfully) and follower (which now
/// hits EACCES on ftruncate).  Follower's reply is
/// `[false, FSERR_PERM, msg]` vs leader's cached `[true]` → hash
/// mismatch → divergence-err fires with FSERR_CONSENSUS_DIVERGENCE
/// + `check_replay_data` Err.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_truncate_reexecute_detects_divergence() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_CONSENSUS_DIVERGENCE;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("data.bin");
    std::fs::write(&target, b"twenty-bytes-of-data").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsTruncate(`rho:io:fs:native:1.0.0/truncate`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, tc, cc
            in {{
              fsOpen!("{root}", "data.bin", "r+", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsTruncate!(fd, 8, *tc) |
                for (@_ <- tc) {{
                  fsClose!(fd, *cc) |
                  for (@_ <- cc) {{ Nil }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[82; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_truncate divergence setup");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert!(
        leader_wal
            .iter()
            .any(|e| e.op == WalOp::Truncate && e.outcome == WalOutcome::Success),
        "leader must have journaled a successful Truncate entry"
    );

    // Force divergence: make the file read-only between leader
    // and follower.  Restore contents first (leader truncated
    // to 8 bytes); then chmod to 0o444 so open("rw") itself
    // fails on the follower.
    std::fs::write(&target, b"twenty-bytes-of-data").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o444)).unwrap();

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

    // Restore permissions so tempdir cleanup doesn't fail.
    let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644));

    // The follower's fs_open re-execute (Phase-2 real-open under
    // Consensus) sees the read-only file and fails with FSERR_PERM.
    // Rholang's `for (@[true, fd] <- oc)` pattern doesn't match on
    // the error reply, so the fsTruncate call downstream never
    // fires — no Truncate WAL entry from the follower's is_replay
    // branch.  But the fsOpen leader-cached-hash comparison would
    // still divergence — actually let me think.  Under Phase-2
    // fs_open real-open, the follower opens against its own subdir
    // (permission-denied); shadow install falls back to file: None.
    // Then downstream fs_truncate sees raw_fd None → FSERR_CLOSED.
    //
    // But the Rholang pattern `[true, fd]` doesn't match the error
    // reply from fsOpen on the follower.  So the sub-continuation
    // (fsTruncate) never fires — no divergence surfaces at the
    // Truncate WAL layer.  We instead assert divergence at the
    // fsOpen layer via the RSpace rig failure below.
    //
    // Simpler path: assert `check_replay_data` returns Err.  The
    // divergent fsOpen produce (leader's [true, fd] vs follower's
    // [false, FSERR_PERM, msg]) trips the rig comparator.
    let rig_result = follower.check_replay_data().await;
    assert!(
        rig_result.is_err(),
        "Phase 3 D1 enforcement: divergent state (leader saw rw file, \
             follower saw ro file) must trip RSpace rig verification — got Ok"
    );

    // WAL-layer divergence witness: the follower's pre-appended
    // Truncate entry got flipped from Success to Failure {
    // FSERR_CODE_CONSENSUS_DIVERGENCE } via finalize_failure_journal
    // on the divergence path.  Note the follower's shadow (opened
    // as file: None on the ro-file fallback in fs_open's Phase-2
    // real-open) makes fs_truncate's fresh syscall return
    // FSERR_CLOSED; that fresh reply doesn't match leader's cached
    // `[true]` → verify hash-mismatch → CONSENSUS_DIVERGENCE code
    // fires via my finalize_failure_journal call.
    let follower_truncate = follower_wal
        .iter()
        .find(|e| e.op == WalOp::Truncate)
        .expect(
            "Phase 3 divergence path must still journal a Truncate entry \
                 (Failure outcome, not a journaling skip — the pre-append is \
                 unconditional under the H-6 pattern)",
        );
    match follower_truncate.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_CONSENSUS_DIVERGENCE,
            "Phase 3: Truncate divergence WAL entry must carry \
                 CONSENSUS_DIVERGENCE code (not an unrelated FSERR from the \
                 raw syscall like FSERR_PERM or FSERR_CLOSED — the whole point \
                 of the Phase-3 mechanism is to surface divergences \
                 specifically as CONSENSUS_DIVERGENCE rather than leaking the \
                 raw syscall FSERR).  Got code {code}"
        ),
        WalOutcome::Success => panic!(
            "Phase 3 REGRESSION: follower's Truncate WAL entry stayed at \
                 Success despite fs drift (ro-file).  Either fresh-syscall \
                 path is not engaged (Phase-0 tautological path back) or \
                 verify_reply_hash_matches_cached is broken.  Truncate entry: \
                 {follower_truncate:?}"
        ),
    }
}

/// Phase 3 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_truncate FSERR_QUOTA_EXCEEDED edge**.  When
/// `n > MAX_TRUNCATE_BYTES = 16 GiB`, the handler short-circuits
/// with `FSERR_QUOTA_EXCEEDED` on both leader and follower BEFORE
/// the pre-append `journal_truncate` call.  This test proves:
///   1. No Truncate WAL entry is journaled on either side
///      (pre-append guard on `n <= MAX_TRUNCATE_BYTES` gates it).
///   2. Same error reply on both sides → verify OK → no divergence.
///   3. WAL byte-identity holds trivially (no WalOp::Truncate).
///
/// A regression that dropped the pre-append gate — journaling
/// Truncate entries for oversized-n calls — would trip this pin
/// on the first assertion.  A regression that swapped the
/// FSERR_QUOTA_EXCEEDED short-circuit for a different code would
/// change the fresh reply on the follower's Consensus branch,
/// potentially triggering CONSENSUS_DIVERGENCE spuriously.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_truncate_quota_exceeded_preserves_wal_symmetry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data.bin"), b"content").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    // n = MAX_TRUNCATE_BYTES + 1 = 16 GiB + 1.  Both sides
    // short-circuit with FSERR_QUOTA_EXCEEDED before the pre-append.
    let oversized_n: u64 = 16 * 1024 * 1024 * 1024 + 1;
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsTruncate(`rho:io:fs:native:1.0.0/truncate`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, tc, cc
            in {{
              fsOpen!("{root}", "data.bin", "r+", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsTruncate!(fd, {n}, *tc) |
                for (@_ <- tc) {{
                  fsClose!(fd, *cc) |
                  for (@_ <- cc) {{ Nil }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
        n = oversized_n,
    );
    let r = Blake2b512Random::create_from_bytes(&[83; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_truncate quota-exceeded");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_truncates: Vec<_> = leader_wal
        .iter()
        .filter(|e| e.op == WalOp::Truncate)
        .collect();
    assert!(
        leader_truncates.is_empty(),
        "leader must NOT journal a Truncate entry for n > MAX_TRUNCATE_BYTES \
             — the pre-append is gated on n <= MAX_TRUNCATE_BYTES.  Got {} \
             Truncate entries",
        leader_truncates.len()
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
        .expect("follower evaluate fs_truncate quota-exceeded");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal, follower_wal,
        "Phase 3 quota-exceeded edge: WALs must be byte-identical across \
             leader and follower.  A regression that gated the pre-append \
             differently on either side, or that spuriously fired \
             CONSENSUS_DIVERGENCE on the QUOTA_EXCEEDED symmetric error, \
             would fail here."
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on quota-exceeded symmetric error");
}

/// Phase 3 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_truncate symmetric syscall error**.  Open the file
/// read-only ("r"), then attempt ftruncate.  Both leader and
/// follower's `libc::ftruncate` on a read-only fd returns
/// EINVAL → the handler maps that to `FSERR_BAD_ARG` (see
/// `io_err_code` in errors.rs).  Same fresh reply on both
/// sides:
///   - Pre-append fires (n <= MAX_TRUNCATE_BYTES).
///   - Fresh reply is `[false, FSERR_BAD_ARG, msg]` on both.
///   - verify_reply_hash_matches_cached returns Ok (same Par).
///   - Under the verify-OK branch, the Consensus follower's
///     `finalize_failure_journal` fires with `FSERR_CODE_BAD_ARG`
///     (not CONSENSUS_DIVERGENCE) — flipping the pre-append
///     placeholder to `Failure { FSERR_CODE_BAD_ARG }` on both
///     sides.
///   - WAL byte-identity holds; check_replay_data OK.
///
/// Proves the H-6 finalize path works symmetrically under
/// Phase-3 Consensus re-execute: fresh syscall errors that agree
/// leader vs follower do NOT trigger CONSENSUS_DIVERGENCE; they
/// flow through the same syscall-error finalize as the pre-refactor
/// leader path.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_truncate_symmetric_syscall_error_finalizes_to_failure() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_BAD_ARG;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data.bin"), b"twenty-bytes-of-data").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    // Open the file read-only, then attempt truncate.  On both
    // Linux and macOS, ftruncate on a read-only fd returns EINVAL
    // (POSIX-specified: fd must be writable).  The handler's
    // io_err_code maps EINVAL → FSERR_BAD_ARG.
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsTruncate(`rho:io:fs:native:1.0.0/truncate`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, tc, cc
            in {{
              fsOpen!("{root}", "data.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsTruncate!(fd, 8, *tc) |
                for (@_ <- tc) {{
                  fsClose!(fd, *cc) |
                  for (@_ <- cc) {{ Nil }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[84; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_truncate symmetric error");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_truncate = leader_wal
        .iter()
        .find(|e| e.op == WalOp::Truncate)
        .expect("leader must journal a Truncate entry (pre-append fires)");
    match leader_truncate.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_BAD_ARG,
            "leader's Truncate entry must be finalized to Failure {{ \
                 FSERR_CODE_BAD_ARG }} for ftruncate-on-readonly-fd (EINVAL); \
                 got code {code}"
        ),
        WalOutcome::Success => panic!(
            "leader's Truncate entry stayed at Success despite EINVAL from \
                 ftruncate on a read-only fd.  H-6 finalize_failure_journal path \
                 is broken.  Truncate entry: {leader_truncate:?}"
        ),
    }

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
        .expect("follower evaluate fs_truncate symmetric error");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal, follower_wal,
        "Phase 3 symmetric-error: WALs must be byte-identical.  A regression \
             that fired CONSENSUS_DIVERGENCE on the symmetric FSERR_BAD_ARG \
             (both sides agreed on the error) would fail here — the Consensus \
             re-execute's verify-OK branch MUST use the fresh syscall's error \
             code, not CONSENSUS_DIVERGENCE."
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on symmetric syscall error");
}

/// Phase 3 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_write positive path with load-bearing on-disk check**.
/// Under Phase 0 tautological replay, the follower's is_replay
/// branch consumed the leader's cached reply and NEVER fired
/// `libc::write` on the follower's own fd — the follower's
/// subdir file (or shared tempdir file, in this test) stayed
/// unmutated by the follower's play run.  Under Phase 3, the
/// follower's Consensus re-execute does a real `libc::write` via
/// its shadow's real fd (installed by fs_open's Phase-2 real-
/// open) using the same bytes the reducer re-evaluates from the
/// deploy source (D2).
///
/// Uses the same test-harness pattern as fs_truncate's positive
/// pin: same term on both sides (RSpace rig hashes produce
/// contents including arg values — different paths would fail
/// rig at the fs_open produce comparator).  Restore file to
/// empty between leader + follower evaluate so the follower's
/// real libc::write has to re-do the leader's work.  Load-
/// bearing post-follower on-disk check: file contains PAYLOAD.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_write_reexecute_writes_to_follower_file() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("data.bin");
    std::fs::write(&target, b"").unwrap();

    let payload = b"phase-3-fs_write-payload";
    let payload_hex = hex::encode(payload);

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, wc, cc
            in {{
              fsOpen!("{root}", "data.bin", "r+", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsWrite!(fd, "{payload_hex}".hexToBytes(), *wc) |
                for (@_ <- wc) {{
                  fsClose!(fd, *cc) |
                  for (@_ <- cc) {{ Nil }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[91; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_write positive");

    // Leader's file contains PAYLOAD post-play.
    let leader_bytes = std::fs::read(&target).unwrap();
    assert_eq!(
        leader_bytes, payload,
        "leader's fs_write must have written PAYLOAD"
    );

    // Restore file to empty pre-play state.  Under D3 per-
    // validator subdirs (production), the follower's own subdir
    // is at pre-play state naturally; this restore emulates
    // that under the shared-tempdir test harness.  See
    // fs_truncate's positive pin for the same pattern.
    std::fs::write(&target, b"").unwrap();

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
        .expect("follower evaluate fs_write positive");

    // LOAD-BEARING Phase-3 assertion: post-restore, the file was
    // empty; if the follower's fs_write re-execute fires, the
    // file contains PAYLOAD.  A regression to Phase-0 tautological
    // (no real libc::write on the follower) would leave the file
    // empty → this assertion fails.
    let follower_post = std::fs::read(&target).unwrap();
    assert_eq!(
        follower_post,
        payload,
        "Phase 3 REGRESSION: follower's fs_write did NOT fire — the file \
             (restored to empty pre-follower-evaluate) is still empty.  Either \
             the fresh-syscall path is not engaged (Phase-0 tautological came \
             back), or fs_open's Phase-2 real-open failed to install a real fd \
             on the follower.  Expected {} bytes; got {} bytes.",
        payload.len(),
        follower_post.len(),
    );

    // WAL byte-identity across both sides.
    let leader_wal = leader.fs_handles.wal.snapshot();
    let follower_wal = follower.fs_handles.wal.snapshot();
    assert_eq!(leader_wal.len(), follower_wal.len());
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(
            l, f,
            "Phase 3: WAL entry {i} differs on fs_write positive: \
                 leader={l:?} follower={f:?}"
        );
    }
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on identical write state");
}

/// Phase 3 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_write divergence-detection path**.  Same shape as the
/// fs_truncate divergence pin — chmod file to 0o444 between
/// leader and follower.  fs_open on follower fails EACCES →
/// shadow file: None → fs_write's write_impl returns FSERR_CLOSED
/// → verify hash-mismatch vs cached `[true, n]` → finalize flips
/// the pre-append Write entry to Failure { FSERR_CODE_CONSENSUS_
/// DIVERGENCE } + check_replay_data Err.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_write_reexecute_detects_divergence() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_CONSENSUS_DIVERGENCE;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("data.bin");
    std::fs::write(&target, b"").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, wc, cc
            in {{
              fsOpen!("{root}", "data.bin", "r+", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsWrite!(fd, "68656c6c6f".hexToBytes(), *wc) |
                for (@_ <- wc) {{
                  fsClose!(fd, *cc) |
                  for (@_ <- cc) {{ Nil }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[92; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_write divergence setup");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert!(
        leader_wal
            .iter()
            .any(|e| e.op == WalOp::Write && e.outcome == WalOutcome::Success),
        "leader must have journaled a successful Write entry"
    );

    // Force divergence: restore, then chmod ro.
    std::fs::write(&target, b"").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o444)).unwrap();

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

    // Restore permissions for cleanup.
    let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644));

    let rig_result = follower.check_replay_data().await;
    assert!(
        rig_result.is_err(),
        "Phase 3 D1 enforcement: divergent state (ro-file on follower's \
             fs_open) must trip RSpace rig verification"
    );

    let follower_write = follower_wal
        .iter()
        .find(|e| e.op == WalOp::Write)
        .expect("follower must have a pre-appended Write entry (unconditional)");
    match follower_write.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_CONSENSUS_DIVERGENCE,
            "Phase 3: Write divergence WAL entry must carry \
                 CONSENSUS_DIVERGENCE code — got code {code}"
        ),
        WalOutcome::Success => panic!(
            "Phase 3 REGRESSION: follower's Write WAL entry stayed at Success \
                 despite fs drift.  Either fresh-syscall path not engaged or \
                 verify broken.  Write entry: {follower_write:?}"
        ),
    }
}

/// Phase 3 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_write symmetric syscall error**.  Analog of the
/// fs_truncate symmetric-error pin.  Open file "r" (read-only)
/// under a Consensus cap, then attempt fs_write.  Both leader
/// and follower's `libc::write` on a read-only fd returns
/// EBADF → the handler maps that to `FSERR_IO` (via
/// `io_err_code`).  Same fresh reply on both sides:
///   - Pre-append fires (bytes.len() <= MAX_WRITE_BYTES).
///   - Fresh reply is `[false, FSERR_IO, msg]` on both.
///   - verify_reply_hash_matches_cached returns Ok (same Par).
///   - Both sides finalize to `Failure { FSERR_CODE_IO }` via
///     the H-6 finalize_failure_journal path — NOT
///     CONSENSUS_DIVERGENCE.
///   - WAL byte-identity holds.
///
/// Proves the Phase-3 verify-OK branch uses the fresh syscall's
/// error code — a regression that spuriously fired
/// CONSENSUS_DIVERGENCE on symmetric errors would fail here.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_write_symmetric_syscall_error_finalizes_to_failure() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data.bin"), b"initial-content").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    // Open the file read-only, then attempt fs_write.  libc::write
    // on a read-only fd returns EBADF; io_err_code maps other-
    // kind errors to FSERR_IO.  Both leader and follower agree on
    // this error → verify OK → finalize to Failure { FSERR_CODE_IO }.
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, wc, cc
            in {{
              fsOpen!("{root}", "data.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsWrite!(fd, "68656c6c6f".hexToBytes(), *wc) |
                for (@_ <- wc) {{
                  fsClose!(fd, *cc) |
                  for (@_ <- cc) {{ Nil }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[93; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_write symmetric error");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_write = leader_wal
        .iter()
        .find(|e| e.op == WalOp::Write)
        .expect("leader must journal a Write entry (pre-append fires)");
    match leader_write.outcome {
        WalOutcome::Failure { code } => assert_ne!(
            code, 0,
            "leader's Write entry must be finalized to Failure with a \
                 valid FSERR code (not UNKNOWN=0) — H-6 finalize_failure_journal \
                 should map the EBADF from libc::write on a read-only fd.  Got \
                 code {code}"
        ),
        WalOutcome::Success => panic!(
            "leader's Write entry stayed at Success despite EBADF from \
                 libc::write on a read-only fd.  H-6 finalize_failure_journal \
                 path is broken.  Write entry: {leader_write:?}"
        ),
    }

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
        .expect("follower evaluate fs_write symmetric error");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal, follower_wal,
        "Phase 3 symmetric-error: WALs must be byte-identical.  A regression \
             that spuriously fired CONSENSUS_DIVERGENCE on the symmetric error \
             (both sides agreed on FSERR_IO) would fail here — the Consensus \
             re-execute's verify-OK branch MUST use the fresh syscall's error \
             code, not CONSENSUS_DIVERGENCE."
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on symmetric syscall error");
}

/// Phase 3 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_write_at positive path with load-bearing on-disk offset
/// check**.  Positional write via `libc::pwrite` — analog of
/// fs_write's positive pin but at a specific offset instead of
/// sequential.  Under Phase 3, follower's Consensus re-execute
/// does a real libc::pwrite via shadow's real fd (installed by
/// fs_open's Phase-2 real-open) at the specified offset.  pwrite
/// does NOT advance OS-fd position (POSIX guarantee), so no
/// shadow position update on either side.
///
/// Uses the same restore-file-between pattern as fs_write's
/// positive pin.  Post-follower on-disk assertion: bytes at the
/// specified offset match PAYLOAD; bytes outside the write
/// window are unchanged from the pre-play state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_write_at_reexecute_writes_to_follower_file_at_offset() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("data.bin");
    // Pre-play file: 32 bytes of 0xAA (marker to distinguish
    // written region from unwritten).
    let pre_bytes = vec![0xAA; 32];
    std::fs::write(&target, &pre_bytes).unwrap();

    let payload = b"phase-3-write_at";
    let payload_hex = hex::encode(payload);
    let offset: u64 = 8;

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWriteAt(`rho:io:fs:native:1.0.0/writeAt`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, wc, cc
            in {{
              fsOpen!("{root}", "data.bin", "r+", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsWriteAt!(fd, {offset}, "{payload_hex}".hexToBytes(), *wc) |
                for (@_ <- wc) {{
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
        .expect("leader evaluate fs_write_at positive");

    // Leader wrote payload at offset — verify pre + payload + post shape.
    let leader_bytes = std::fs::read(&target).unwrap();
    assert_eq!(
        &leader_bytes[..offset as usize],
        &[0xAA; 8],
        "leader's pre-offset bytes must be unchanged (pwrite doesn't touch \
             them)"
    );
    assert_eq!(
        &leader_bytes[offset as usize..offset as usize + payload.len()],
        payload,
        "leader's on-disk bytes at offset must equal PAYLOAD"
    );
    assert_eq!(
        &leader_bytes[offset as usize + payload.len()..],
        &[0xAA; 32 - 8 - 16],
        "leader's post-offset bytes must be unchanged"
    );

    // Restore file to pre-play state for follower's re-execute.
    std::fs::write(&target, &pre_bytes).unwrap();

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
        .expect("follower evaluate fs_write_at positive");

    // LOAD-BEARING: follower's real libc::pwrite must have hit
    // the offset.  Post-restore file was all 0xAA; if the
    // follower's fs_write_at re-execute fires, bytes at offset
    // become PAYLOAD (bytes outside the window stay 0xAA).  A
    // regression to Phase-0 tautological leaves the file at
    // all-0xAA.
    let follower_bytes = std::fs::read(&target).unwrap();
    assert_eq!(
        &follower_bytes[offset as usize..offset as usize + payload.len()],
        payload,
        "Phase 3 REGRESSION: follower's fs_write_at did NOT fire at offset \
             — bytes at offset are still 0xAA (pre-play).  Either fresh-syscall \
             path not engaged or fs_open Phase-2 real-open failed."
    );
    assert_eq!(
        &follower_bytes[..offset as usize],
        &[0xAA; 8],
        "follower's pre-offset bytes must be unchanged"
    );

    // WAL byte-identity across all entries.
    let leader_wal = leader.fs_handles.wal.snapshot();
    let follower_wal = follower.fs_handles.wal.snapshot();
    assert_eq!(leader_wal.len(), follower_wal.len());
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(
            l, f,
            "Phase 3: WAL entry {i} differs on fs_write_at positive: \
                 leader={l:?} follower={f:?}"
        );
    }
    // WriteAt entry must record offset in the WAL entry.
    let leader_writeat = leader_wal
        .iter()
        .find(|e| e.op == WalOp::WriteAt)
        .expect("leader must journal a WriteAt entry");
    assert_eq!(leader_writeat.offset, Some(offset));
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on identical fs_write_at state");
}

/// Phase 3 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_write_at divergence-detection**.  Same chmod-cascade
/// shape as fs_write / fs_truncate divergence pins.  Follower's
/// fs_open sees ro-file → shadow file: None → fs_write_at's
/// write_impl returns FSERR_CLOSED → verify hash-mismatch vs
/// cached [true, n] → Failure { FSERR_CODE_CONSENSUS_DIVERGENCE }
/// + check_replay_data Err.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_write_at_reexecute_detects_divergence() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_CONSENSUS_DIVERGENCE;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("data.bin");
    std::fs::write(&target, vec![0xAA; 32]).unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWriteAt(`rho:io:fs:native:1.0.0/writeAt`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, wc, cc
            in {{
              fsOpen!("{root}", "data.bin", "r+", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsWriteAt!(fd, 8, "68656c6c6f".hexToBytes(), *wc) |
                for (@_ <- wc) {{
                  fsClose!(fd, *cc) |
                  for (@_ <- cc) {{ Nil }}
                }}
              }}
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
        .expect("leader evaluate fs_write_at divergence setup");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert!(
        leader_wal
            .iter()
            .any(|e| e.op == WalOp::WriteAt && e.outcome == WalOutcome::Success),
        "leader must have journaled a successful WriteAt entry"
    );

    std::fs::write(&target, vec![0xAA; 32]).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o444)).unwrap();

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
    let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644));

    let rig_result = follower.check_replay_data().await;
    assert!(
        rig_result.is_err(),
        "Phase 3 D1 enforcement: divergent state (ro-file) must trip \
             RSpace rig on the fs_open produce"
    );

    let follower_writeat = follower_wal
        .iter()
        .find(|e| e.op == WalOp::WriteAt)
        .expect("follower must have a pre-appended WriteAt entry");
    match follower_writeat.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_CONSENSUS_DIVERGENCE,
            "Phase 3: WriteAt divergence WAL entry must carry \
                 CONSENSUS_DIVERGENCE code — got {code}"
        ),
        WalOutcome::Success => panic!(
            "Phase 3 REGRESSION: follower's WriteAt entry stayed at \
                 Success despite fs drift.  WriteAt entry: {follower_writeat:?}"
        ),
    }
}

/// Phase 3 pin (Consensus re-execute + verify, 2026-09-01):
/// **fs_write_at symmetric syscall error**.  Analog of fs_write /
/// fs_truncate symmetric-error pins.  Open file "r" (read-only)
/// under Consensus, then attempt fs_write_at.  libc::pwrite on
/// a read-only fd returns EBADF → both sides map to FSERR_IO →
/// same fresh reply → verify OK → both finalize to Failure {
/// FSERR_CODE_IO }, NOT CONSENSUS_DIVERGENCE.  WAL byte-identity.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_write_at_symmetric_syscall_error_finalizes_to_failure() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data.bin"), vec![0xAA; 32]).unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWriteAt(`rho:io:fs:native:1.0.0/writeAt`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, wc, cc
            in {{
              fsOpen!("{root}", "data.bin", "r", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsWriteAt!(fd, 8, "68656c6c6f".hexToBytes(), *wc) |
                for (@_ <- wc) {{
                  fsClose!(fd, *cc) |
                  for (@_ <- cc) {{ Nil }}
                }}
              }}
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
        .expect("leader evaluate fs_write_at symmetric error");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_writeat = leader_wal
        .iter()
        .find(|e| e.op == WalOp::WriteAt)
        .expect("leader must journal a WriteAt entry");
    match leader_writeat.outcome {
        WalOutcome::Failure { code } => assert_ne!(
            code, 0,
            "leader's WriteAt entry must finalize to a valid FSERR code \
                 (not UNKNOWN=0) for pwrite-on-readonly-fd (EBADF).  Got {code}"
        ),
        WalOutcome::Success => panic!(
            "leader's WriteAt entry stayed at Success despite EBADF from \
                 pwrite on ro fd.  H-6 finalize_failure_journal path broken.  \
                 WriteAt entry: {leader_writeat:?}"
        ),
    }

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
        .expect("follower evaluate fs_write_at symmetric error");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal, follower_wal,
        "Phase 3 fs_write_at symmetric-error: WALs must be byte-identical. \
             A regression that spuriously fired CONSENSUS_DIVERGENCE on the \
             symmetric FSERR_IO would fail here."
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on symmetric syscall error");
}

/// Phase 4 pin (Consensus re-execute + verify, 2026-09-02):
/// **fs_chmod positive path** with load-bearing on-disk mode-
/// bits assertion.  Path-based mutation via `fchmodat`.  Under
/// Phase-0 tautological replay, the follower's is_replay branch
/// consumed the leader's cached reply and NEVER fired the
/// syscall — the follower's on-disk file mode stayed unchanged.
/// Under Phase 4, the follower's Consensus re-execute does a
/// real `fchmodat` against its own subdir via the Shape A
/// resolver.
///
/// Uses same restore-mode pattern as fs_truncate's positive pin:
/// same term on both sides (RSpace rig hashes produce content
/// including arg values); restore file mode to a KNOWN-different
/// value (0o644) between leader + follower evaluate so the
/// follower's real fchmodat has to re-do the leader's work
/// (setting mode to 0o444).  Load-bearing post-follower on-disk
/// mode-bits check.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_chmod_reexecute_changes_follower_file_mode() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("data.bin");
    std::fs::write(&target, b"content").unwrap();
    // Pre-play mode = 0o644.
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    // Target mode = 0o444 (read-only).
    let target_mode: u32 = 0o444;
    let term = format!(
        r#"
            new fsChmod(`rho:io:fs:native:1.0.0/chmod`), ackCh in {{
              fsChmod!("{root}", "data.bin", {mode}, "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
        mode = target_mode,
    );
    let r = Blake2b512Random::create_from_bytes(&[101; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_chmod positive");
    let leader_mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o7777;
    assert_eq!(
        leader_mode, target_mode,
        "leader's fchmodat must have set the file's mode to 0o444"
    );

    // Restore mode to 0o644 pre-follower — proves follower's own
    // real fchmodat re-did the change.
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();

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
        .expect("follower evaluate fs_chmod positive");

    // LOAD-BEARING: file mode must be 0o444 post-follower-evaluate.
    // Regression to Phase-0 tautological leaves it at 0o644.
    let follower_mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o7777;
    assert_eq!(
        follower_mode, target_mode,
        "Phase 4 REGRESSION: follower's fs_chmod did NOT fire — file mode \
             is still 0o{follower_mode:o} (expected 0o{target_mode:o}).  Either \
             fresh-syscall path not engaged (Phase-0 tautological came back) or \
             the Shape A resolver failed to route to the on-disk root.",
    );

    // Restore for cleanup.
    let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644));

    // WAL byte-identity.
    let leader_wal = leader.fs_handles.wal.snapshot();
    let follower_wal = follower.fs_handles.wal.snapshot();
    assert_eq!(leader_wal.len(), follower_wal.len());
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(
            l, f,
            "Phase 4: WAL entry {i} differs on fs_chmod positive: \
                 leader={l:?} follower={f:?}"
        );
    }
    let chmod_entry = leader_wal
        .iter()
        .find(|e| e.op == WalOp::Chmod)
        .expect("leader must journal a Chmod entry");
    assert_eq!(chmod_entry.mode_bits, Some(target_mode));
    assert_eq!(chmod_entry.outcome, WalOutcome::Success);

    follower
        .check_replay_data()
        .await
        .expect("replay data must match on identical fs_chmod");
}

/// Phase 4 pin (Consensus re-execute + verify, 2026-09-02):
/// **fs_chmod divergence-detection**.  Same chmod-cascade pattern
/// as other Phase-3 divergence pins.  Between leader + follower
/// evaluate, replace the file with a DIRECTORY at the same name;
/// follower's fchmodat on a directory (with AT_SYMLINK_NOFOLLOW)
/// still succeeds on most systems but may fail differently
/// depending on FS.  Simplest: remove the file entirely →
/// fchmodat returns ENOENT → FSERR_NOT_FOUND.  verify sees
/// fresh err vs cached ok → CONSENSUS_DIVERGENCE fires.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_chmod_reexecute_detects_divergence() {
    use std::os::unix::fs::PermissionsExt;

    use rholang::rust::interpreter::io::errors::FSERR_CODE_CONSENSUS_DIVERGENCE;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("data.bin");
    std::fs::write(&target, b"content").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsChmod(`rho:io:fs:native:1.0.0/chmod`), ackCh in {{
              fsChmod!("{root}", "data.bin", {mode}, "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
        mode = 0o444u32,
    );
    let r = Blake2b512Random::create_from_bytes(&[102; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_chmod divergence setup");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert!(
        leader_wal
            .iter()
            .any(|e| e.op == WalOp::Chmod && e.outcome == WalOutcome::Success),
        "leader must journal a successful Chmod entry"
    );

    // Force divergence: remove the file entirely.  Follower's
    // fchmodat sees ENOENT → FSERR_NOT_FOUND, which differs from
    // leader's cached [true].
    std::fs::remove_file(&target).unwrap();

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

    let rig_result = follower.check_replay_data().await;
    assert!(
        rig_result.is_err(),
        "Phase 4 D1 enforcement: divergent state (file removed pre-follower) \
             must trip RSpace rig verification on the fs_chmod produce"
    );

    let follower_chmod = follower_wal
        .iter()
        .find(|e| e.op == WalOp::Chmod)
        .expect("follower must have a pre-appended Chmod entry");
    match follower_chmod.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_CONSENSUS_DIVERGENCE,
            "Phase 4: Chmod divergence WAL entry must carry \
                 CONSENSUS_DIVERGENCE code — got {code}"
        ),
        WalOutcome::Success => panic!(
            "Phase 4 REGRESSION: follower's Chmod entry stayed at Success \
                 despite fs drift.  Chmod entry: {follower_chmod:?}"
        ),
    }
}

/// Phase 4 pin (Consensus re-execute + verify, 2026-09-02):
/// **fs_chmod symmetric syscall error**.  Analog of fs_truncate /
/// fs_write / fs_write_at symmetric-error pins.  Attempt fchmodat
/// on a non-existent file — both leader and follower see the
/// same ENOENT → FSERR_NOT_FOUND → verify OK → both finalize to
/// Failure { FSERR_CODE_NOT_FOUND }, NOT CONSENSUS_DIVERGENCE.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_chmod_symmetric_syscall_error_finalizes_to_failure() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_NOT_FOUND;

    let dir = tempfile::tempdir().unwrap();
    // No file at "does-not-exist.bin".

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsChmod(`rho:io:fs:native:1.0.0/chmod`), ackCh in {{
              fsChmod!("{root}", "does-not-exist.bin", {mode}, "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
        mode = 0o644u32,
    );
    let r = Blake2b512Random::create_from_bytes(&[103; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_chmod symmetric error");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_chmod = leader_wal
        .iter()
        .find(|e| e.op == WalOp::Chmod)
        .expect("leader must journal a Chmod entry");
    match leader_chmod.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_NOT_FOUND,
            "leader's Chmod entry must finalize to Failure with NOT_FOUND \
                 for fchmodat on missing file (ENOENT); got {code}"
        ),
        WalOutcome::Success => panic!(
            "leader's Chmod entry stayed at Success despite ENOENT.  H-6 \
                 finalize_failure_journal broken.  Chmod entry: {leader_chmod:?}"
        ),
    }

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
        .expect("follower evaluate fs_chmod symmetric error");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal, follower_wal,
        "Phase 4 fs_chmod symmetric-error: WALs must be byte-identical. \
             A regression that spuriously fired CONSENSUS_DIVERGENCE on the \
             symmetric FSERR_NOT_FOUND would fail here."
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on symmetric syscall error");
}

/// Phase 4 pin (Consensus re-execute + verify, 2026-09-02):
/// **fs_remove_file positive path** with load-bearing on-disk
/// file-existence check.  Path-based mutation via `unlinkat`.
/// Under Phase-0 tautological replay, the follower's is_replay
/// branch consumed cached reply and never fired unlinkat — the
/// follower's on-disk file stayed present.  Under Phase 4, the
/// follower's Consensus re-execute does a real unlinkat via
/// the Shape A resolver.
///
/// Uses the restore-file-between pattern (like fs_truncate /
/// fs_write / fs_chmod pins): recreate the file pre-follower
/// so the follower's real unlinkat has to re-remove it.  Load-
/// bearing post-follower file-existence check: file must be
/// gone (not present).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_remove_file_reexecute_removes_follower_file() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("removable.bin");
    std::fs::write(&target, b"content").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsRemove(`rho:io:fs:native:1.0.0/removeFile`), ackCh in {{
              fsRemove!("{root}", "removable.bin", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[111; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_remove_file positive");
    assert!(
        !target.exists(),
        "leader's unlinkat must have removed the file"
    );

    // Restore file pre-follower — proves follower's own real
    // unlinkat re-did the removal.
    std::fs::write(&target, b"content").unwrap();

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
        .expect("follower evaluate fs_remove_file positive");

    // LOAD-BEARING: file must be gone post-follower-evaluate.
    // Regression to Phase-0 tautological leaves the file present.
    assert!(
        !target.exists(),
        "Phase 4 REGRESSION: follower's fs_remove_file did NOT fire — file \
             still exists after follower.evaluate.  Either fresh-syscall path \
             not engaged (Phase-0 tautological came back) or Shape A resolver \
             failed to route."
    );

    let leader_wal = leader.fs_handles.wal.snapshot();
    let follower_wal = follower.fs_handles.wal.snapshot();
    assert_eq!(leader_wal.len(), follower_wal.len());
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(
            l, f,
            "Phase 4: WAL entry {i} differs on fs_remove_file positive: \
                 leader={l:?} follower={f:?}"
        );
    }
    let rf_entry = leader_wal
        .iter()
        .find(|e| e.op == WalOp::RemoveFile)
        .expect("leader must journal a RemoveFile entry");
    assert_eq!(rf_entry.outcome, WalOutcome::Success);

    follower
        .check_replay_data()
        .await
        .expect("replay data must match on identical fs_remove_file");
}

/// Phase 4 pin (Consensus re-execute + verify, 2026-09-02):
/// **fs_remove_file divergence-detection**.  Between leader +
/// follower evaluate, do NOT restore the file — follower's
/// unlinkat sees ENOENT (leader already removed it) → fresh err
/// vs cached [true] → CONSENSUS_DIVERGENCE fires.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_remove_file_reexecute_detects_divergence() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_CONSENSUS_DIVERGENCE;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("removable.bin"), b"content").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsRemove(`rho:io:fs:native:1.0.0/removeFile`), ackCh in {{
              fsRemove!("{root}", "removable.bin", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[112; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_remove_file divergence setup");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert!(
        leader_wal
            .iter()
            .any(|e| e.op == WalOp::RemoveFile && e.outcome == WalOutcome::Success),
        "leader must journal a successful RemoveFile entry"
    );

    // DO NOT restore the file — follower's re-execute sees
    // leader's post-play state (file gone).  Follower's fresh
    // unlinkat returns ENOENT → CONSENSUS_DIVERGENCE.
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

    let rig_result = follower.check_replay_data().await;
    assert!(
        rig_result.is_err(),
        "Phase 4 D1 enforcement: follower's fresh ENOENT vs leader's cached \
             [true] must trip RSpace rig verification"
    );

    let follower_rf = follower_wal
        .iter()
        .find(|e| e.op == WalOp::RemoveFile)
        .expect("follower must have a pre-appended RemoveFile entry");
    match follower_rf.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_CONSENSUS_DIVERGENCE,
            "Phase 4: RemoveFile divergence WAL entry must carry \
                 CONSENSUS_DIVERGENCE code — got {code}"
        ),
        WalOutcome::Success => panic!(
            "Phase 4 REGRESSION: follower's RemoveFile entry stayed at \
                 Success despite fs drift.  Entry: {follower_rf:?}"
        ),
    }
}

/// Phase 4 pin (Consensus re-execute + verify, 2026-09-02):
/// **fs_remove_file symmetric syscall error**.  Attempt unlinkat
/// on a non-existent file on both sides → both see ENOENT →
/// FSERR_NOT_FOUND → verify OK → both finalize to Failure {
/// FSERR_CODE_NOT_FOUND }, NOT CONSENSUS_DIVERGENCE.  Parity
/// with fs_chmod / fs_truncate / fs_write symmetric-error pins.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_remove_file_symmetric_syscall_error_finalizes_to_failure() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_NOT_FOUND;

    let dir = tempfile::tempdir().unwrap();
    // No file at "does-not-exist.bin".

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsRemove(`rho:io:fs:native:1.0.0/removeFile`), ackCh in {{
              fsRemove!("{root}", "does-not-exist.bin", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[113; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_remove_file symmetric error");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_rf = leader_wal
        .iter()
        .find(|e| e.op == WalOp::RemoveFile)
        .expect("leader must journal a RemoveFile entry");
    match leader_rf.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_NOT_FOUND,
            "leader's RemoveFile entry must finalize to Failure with \
                 NOT_FOUND for unlinkat on missing file (ENOENT); got {code}"
        ),
        WalOutcome::Success => panic!(
            "leader's RemoveFile entry stayed at Success despite ENOENT.  \
                 H-6 finalize broken.  Entry: {leader_rf:?}"
        ),
    }

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
        .expect("follower evaluate fs_remove_file symmetric error");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal, follower_wal,
        "Phase 4 fs_remove_file symmetric-error: WALs must be byte-identical. \
             A regression that spuriously fired CONSENSUS_DIVERGENCE on the \
             symmetric FSERR_NOT_FOUND would fail here."
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on symmetric syscall error");
}

/// Phase 4 pin (Consensus re-execute + verify, 2026-09-02):
/// **fs_rename positive re-execute**.  Leader renames a.bin →
/// b.bin (success).  Restore the pre-play state between leader
/// and follower evaluate (delete b.bin, recreate a.bin) so
/// follower's own renameat can succeed against its own file.
/// Post-follower-evaluate: b.bin exists, a.bin does not.
/// Regression to Phase-0 tautological leaves a.bin present +
/// b.bin absent (follower never fired the syscall).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_rename_reexecute_renames_follower_file() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("a.bin");
    let dst = dir.path().join("b.bin");
    std::fs::write(&src, b"content").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsRename(`rho:io:fs:native:1.0.0/rename`), ackCh in {{
              fsRename!("{root}", "a.bin", "{root}", "b.bin", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[114; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_rename positive");
    assert!(!src.exists(), "leader's renameat must remove source");
    assert!(dst.exists(), "leader's renameat must create dest");

    // Restore pre-play state — proves follower's own real
    // renameat re-did the operation.
    std::fs::remove_file(&dst).unwrap();
    std::fs::write(&src, b"content").unwrap();

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
        .expect("follower evaluate fs_rename positive");

    // LOAD-BEARING: source gone + dest present post-follower.
    // Regression to Phase-0 tautological leaves src present + dst absent.
    assert!(
        !src.exists(),
        "Phase 4 REGRESSION: follower's fs_rename did NOT fire — source \
             still exists after follower.evaluate.  Either fresh-syscall path \
             not engaged (Phase-0 tautological came back) or Shape A resolver \
             failed to route."
    );
    assert!(
        dst.exists(),
        "Phase 4 REGRESSION: follower's fs_rename did NOT fire — dest \
             not present after follower.evaluate."
    );

    let leader_wal = leader.fs_handles.wal.snapshot();
    let follower_wal = follower.fs_handles.wal.snapshot();
    assert_eq!(leader_wal.len(), follower_wal.len());
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(
            l, f,
            "Phase 4: WAL entry {i} differs on fs_rename positive: \
                 leader={l:?} follower={f:?}"
        );
    }
    let rn_entry = leader_wal
        .iter()
        .find(|e| e.op == WalOp::Rename)
        .expect("leader must journal a Rename entry");
    assert_eq!(rn_entry.outcome, WalOutcome::Success);

    follower
        .check_replay_data()
        .await
        .expect("replay data must match on identical fs_rename");
}

/// Phase 4 pin (Consensus re-execute + verify, 2026-09-02):
/// **fs_rename divergence-detection**.  Leader renames a.bin →
/// b.bin successfully; do NOT restore between evaluate.  Follower's
/// renameat sees ENOENT (source already moved by leader) → fresh
/// err vs cached [true] → CONSENSUS_DIVERGENCE fires.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_rename_reexecute_detects_divergence() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_CONSENSUS_DIVERGENCE;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.bin"), b"content").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsRename(`rho:io:fs:native:1.0.0/rename`), ackCh in {{
              fsRename!("{root}", "a.bin", "{root}", "b.bin", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[115; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_rename divergence setup");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert!(
        leader_wal
            .iter()
            .any(|e| e.op == WalOp::Rename && e.outcome == WalOutcome::Success),
        "leader must journal a successful Rename entry"
    );

    // DO NOT restore — follower sees leader's post-play state
    // (a.bin gone, b.bin present).  Follower's fresh renameat
    // returns ENOENT → CONSENSUS_DIVERGENCE.
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

    let rig_result = follower.check_replay_data().await;
    assert!(
        rig_result.is_err(),
        "Phase 4 D1 enforcement: follower's fresh ENOENT vs leader's cached \
             [true] must trip RSpace rig verification"
    );

    let follower_rn = follower_wal
        .iter()
        .find(|e| e.op == WalOp::Rename)
        .expect("follower must have a pre-appended Rename entry");
    match follower_rn.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_CONSENSUS_DIVERGENCE,
            "Phase 4: Rename divergence WAL entry must carry \
                 CONSENSUS_DIVERGENCE code — got {code}"
        ),
        WalOutcome::Success => panic!(
            "Phase 4 REGRESSION: follower's Rename entry stayed at \
                 Success despite fs drift.  Entry: {follower_rn:?}"
        ),
    }
}

/// Phase 4 pin (Consensus re-execute + verify, 2026-09-02):
/// **fs_rename symmetric syscall error**.  Attempt renameat on
/// a non-existent source on both sides → both see ENOENT →
/// FSERR_NOT_FOUND → verify OK → both finalize to Failure {
/// FSERR_CODE_NOT_FOUND }, NOT CONSENSUS_DIVERGENCE.  Parity
/// with fs_chmod / fs_remove_file symmetric-error pins.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_rename_symmetric_syscall_error_finalizes_to_failure() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_NOT_FOUND;

    let dir = tempfile::tempdir().unwrap();
    // No file at "a.bin".

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsRename(`rho:io:fs:native:1.0.0/rename`), ackCh in {{
              fsRename!("{root}", "a.bin", "{root}", "b.bin", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[116; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_rename symmetric error");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_rn = leader_wal
        .iter()
        .find(|e| e.op == WalOp::Rename)
        .expect("leader must journal a Rename entry");
    match leader_rn.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_NOT_FOUND,
            "leader's Rename entry must finalize to Failure with NOT_FOUND \
                 for renameat on missing source (ENOENT); got {code}"
        ),
        WalOutcome::Success => panic!(
            "leader's Rename entry stayed at Success despite ENOENT.  \
                 H-6 finalize broken.  Entry: {leader_rn:?}"
        ),
    }

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
        .expect("follower evaluate fs_rename symmetric error");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal, follower_wal,
        "Phase 4 fs_rename symmetric-error: WALs must be byte-identical. \
             A regression that spuriously fired CONSENSUS_DIVERGENCE on the \
             symmetric FSERR_NOT_FOUND would fail here."
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on symmetric syscall error");
}

/// Phase 4 pin (Consensus re-execute + verify, 2026-09-02):
/// **fs_copy_file positive re-execute**.  Leader copies src.bin
/// → dst.bin (success).  Delete dst.bin between leader + follower
/// evaluate so follower's own real copy re-creates it via
/// safe_open_verified + std::io::copy.  Post-follower: dst.bin
/// exists with identical bytes.  A regression to Phase-0
/// tautological leaves dst.bin missing (follower never fired
/// the syscall).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_copy_file_reexecute_copies_to_follower_file() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.bin");
    let dst = dir.path().join("dst.bin");
    let payload: &[u8] = b"payload bytes for copy";
    std::fs::write(&src, payload).unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsCopy(`rho:io:fs:native:1.0.0/copyFile`), ackCh in {{
              fsCopy!("{root}", "src.bin", "{root}", "dst.bin", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[117; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_copy_file positive");
    assert!(dst.exists(), "leader's copy must create dest");
    assert_eq!(std::fs::read(&dst).unwrap(), payload);

    // Remove dst pre-follower — proves follower's own real
    // std::io::copy re-created it.
    std::fs::remove_file(&dst).unwrap();

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
        .expect("follower evaluate fs_copy_file positive");

    // LOAD-BEARING: dst present with correct bytes post-follower.
    // Regression to Phase-0 tautological leaves dst absent.
    assert!(
        dst.exists(),
        "Phase 4 REGRESSION: follower's fs_copy_file did NOT fire — dst \
             not present after follower.evaluate.  Either fresh-syscall path \
             not engaged (Phase-0 tautological came back) or Shape A resolver \
             failed to route."
    );
    assert_eq!(
        std::fs::read(&dst).unwrap(),
        payload,
        "Phase 4 REGRESSION: follower's fs_copy_file wrote different bytes \
             than the source; std::io::copy through safe_open_verified must \
             produce byte-identical output."
    );

    let leader_wal = leader.fs_handles.wal.snapshot();
    let follower_wal = follower.fs_handles.wal.snapshot();
    assert_eq!(leader_wal.len(), follower_wal.len());
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(
            l, f,
            "Phase 4: WAL entry {i} differs on fs_copy_file positive: \
                 leader={l:?} follower={f:?}"
        );
    }
    let cf_entry = leader_wal
        .iter()
        .find(|e| e.op == WalOp::CopyFile)
        .expect("leader must journal a CopyFile entry");
    assert_eq!(cf_entry.outcome, WalOutcome::Success);

    follower
        .check_replay_data()
        .await
        .expect("replay data must match on identical fs_copy_file");
}

/// Phase 4 pin (Consensus re-execute + verify, 2026-09-02):
/// **fs_copy_file byte-count divergence-detection**.  Leader
/// copies N bytes (cached reply [true, N]).  Between leader +
/// follower evaluate, truncate the source to a shorter length —
/// follower's copy produces M < N bytes → fresh reply [true, M]
/// hashes differently than cached [true, N] → CONSENSUS_DIVERGENCE
/// fires.  This pin specifically exercises the reply's u64
/// payload (byte count) as part of the hash, catching regressions
/// where verify was hashing only the boolean portion.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_copy_file_reexecute_detects_byte_count_divergence() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_CONSENSUS_DIVERGENCE;

    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.bin");
    let dst = dir.path().join("dst.bin");
    std::fs::write(&src, b"twenty-two payload bytes").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsCopy(`rho:io:fs:native:1.0.0/copyFile`), ackCh in {{
              fsCopy!("{root}", "src.bin", "{root}", "dst.bin", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[118; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_copy_file divergence setup");
    let leader_n = std::fs::metadata(&dst).unwrap().len();
    assert!(leader_n > 0, "leader must have copied bytes");

    // Truncate source to a shorter length between evaluate.  We
    // also delete dst so follower's O_CREAT|O_TRUNC re-creates
    // it, but reads a smaller source → different n.
    std::fs::write(&src, b"short").unwrap();
    std::fs::remove_file(&dst).unwrap();

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

    let rig_result = follower.check_replay_data().await;
    assert!(
        rig_result.is_err(),
        "Phase 4 D1 enforcement: follower's fresh copy with different n \
             must trip RSpace rig verification"
    );

    let follower_cf = follower_wal
        .iter()
        .find(|e| e.op == WalOp::CopyFile)
        .expect("follower must have a pre-appended CopyFile entry");
    match follower_cf.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_CONSENSUS_DIVERGENCE,
            "Phase 4: CopyFile divergence WAL entry must carry \
                 CONSENSUS_DIVERGENCE code — got {code}"
        ),
        WalOutcome::Success => panic!(
            "Phase 4 REGRESSION: follower's CopyFile entry stayed at \
                 Success despite byte-count drift.  Entry: {follower_cf:?}"
        ),
    }
}

/// Phase 4 pin (Consensus re-execute + verify, 2026-09-02):
/// **fs_copy_file symmetric syscall error**.  Attempt copyFile
/// with a non-existent source on both sides → both see ENOENT
/// → FSERR_NOT_FOUND → verify OK → both finalize to Failure {
/// FSERR_CODE_NOT_FOUND }, NOT CONSENSUS_DIVERGENCE.  Parity
/// with fs_chmod / fs_remove_file / fs_rename symmetric-error
/// pins.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_copy_file_symmetric_syscall_error_finalizes_to_failure() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_NOT_FOUND;

    let dir = tempfile::tempdir().unwrap();
    // No file at "missing.bin".

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsCopy(`rho:io:fs:native:1.0.0/copyFile`), ackCh in {{
              fsCopy!("{root}", "missing.bin", "{root}", "dst.bin", "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[119; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_copy_file symmetric error");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_cf = leader_wal
        .iter()
        .find(|e| e.op == WalOp::CopyFile)
        .expect("leader must journal a CopyFile entry");
    match leader_cf.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_NOT_FOUND,
            "leader's CopyFile entry must finalize to Failure with NOT_FOUND \
                 for safe_open_verified on missing source (ENOENT); got {code}"
        ),
        WalOutcome::Success => panic!(
            "leader's CopyFile entry stayed at Success despite ENOENT.  \
                 H-6 finalize broken.  Entry: {leader_cf:?}"
        ),
    }

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
        .expect("follower evaluate fs_copy_file symmetric error");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal, follower_wal,
        "Phase 4 fs_copy_file symmetric-error: WALs must be byte-identical. \
             A regression that spuriously fired CONSENSUS_DIVERGENCE on the \
             symmetric FSERR_NOT_FOUND would fail here."
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on symmetric syscall error");
}

/// Phase 4 pin (Consensus re-execute + verify, 2026-09-02):
/// **fs_remove_dir non-recursive positive re-execute**.  Leader
/// removes an empty directory via unlinkat(AT_REMOVEDIR).
/// Restore pre-play state (recreate the directory) between
/// leader + follower evaluate so follower's own real syscall
/// can succeed against its own directory.  Post-follower:
/// directory is gone.  Regression to Phase-0 tautological
/// leaves the directory present.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_remove_dir_non_recursive_reexecute_removes_follower_dir() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("empty-dir");
    std::fs::create_dir(&target).unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsRemoveDir(`rho:io:fs:native:1.0.0/removeDir`), ackCh in {{
              fsRemoveDir!("{root}", "empty-dir", false, "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[120; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_remove_dir non-recursive positive");
    assert!(!target.exists(), "leader's unlinkat must have removed dir");

    // Restore dir pre-follower — proves follower's own real
    // unlinkat re-did the removal.
    std::fs::create_dir(&target).unwrap();

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
        .expect("follower evaluate fs_remove_dir non-recursive positive");

    // LOAD-BEARING: directory must be gone post-follower-evaluate.
    // Regression to Phase-0 tautological leaves it present.
    assert!(
        !target.exists(),
        "Phase 4 REGRESSION: follower's fs_remove_dir non-recursive did NOT \
             fire — dir still exists after follower.evaluate.  Either fresh-syscall \
             path not engaged (Phase-0 tautological came back) or Shape A resolver \
             failed to route."
    );

    let leader_wal = leader.fs_handles.wal.snapshot();
    let follower_wal = follower.fs_handles.wal.snapshot();
    assert_eq!(leader_wal.len(), follower_wal.len());
    for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
        assert_eq!(
            l, f,
            "Phase 4: WAL entry {i} differs on fs_remove_dir non-recursive \
                 positive: leader={l:?} follower={f:?}"
        );
    }
    let rd_entry = leader_wal
        .iter()
        .find(|e| e.op == WalOp::RemoveDir)
        .expect("leader must journal a RemoveDir entry");
    assert_eq!(rd_entry.outcome, WalOutcome::Success);

    follower
        .check_replay_data()
        .await
        .expect("replay data must match on identical fs_remove_dir");
}

/// Phase 4 pin (Consensus re-execute + verify, 2026-09-02):
/// **fs_remove_dir non-recursive divergence-detection**.  Leader
/// removes the directory successfully; do NOT restore between
/// evaluate — follower's unlinkat returns ENOENT → fresh err vs
/// cached [true] → CONSENSUS_DIVERGENCE fires.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_remove_dir_non_recursive_reexecute_detects_divergence() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_CONSENSUS_DIVERGENCE;

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("empty-dir")).unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsRemoveDir(`rho:io:fs:native:1.0.0/removeDir`), ackCh in {{
              fsRemoveDir!("{root}", "empty-dir", false, "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[121; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_remove_dir non-recursive divergence setup");
    let leader_wal = leader.fs_handles.wal.snapshot();
    assert!(
        leader_wal
            .iter()
            .any(|e| e.op == WalOp::RemoveDir && e.outcome == WalOutcome::Success),
        "leader must journal a successful RemoveDir entry"
    );

    // DO NOT restore — follower's unlinkat sees ENOENT.
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

    let rig_result = follower.check_replay_data().await;
    assert!(
        rig_result.is_err(),
        "Phase 4 D1 enforcement: follower's fresh ENOENT vs leader's cached \
             [true] must trip RSpace rig verification"
    );

    let follower_rd = follower_wal
        .iter()
        .find(|e| e.op == WalOp::RemoveDir)
        .expect("follower must have a pre-appended RemoveDir entry");
    match follower_rd.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_CONSENSUS_DIVERGENCE,
            "Phase 4: RemoveDir divergence WAL entry must carry \
                 CONSENSUS_DIVERGENCE code — got {code}"
        ),
        WalOutcome::Success => panic!(
            "Phase 4 REGRESSION: follower's RemoveDir entry stayed at \
                 Success despite fs drift.  Entry: {follower_rd:?}"
        ),
    }
}

/// Phase 4 pin (Consensus re-execute + verify, 2026-09-02):
/// **fs_remove_dir non-recursive symmetric syscall error**.
/// Attempt to remove a non-existent directory on both sides →
/// both see ENOENT → FSERR_NOT_FOUND → verify OK → both finalize
/// to Failure { FSERR_CODE_NOT_FOUND }, NOT CONSENSUS_DIVERGENCE.
/// Parity with fs_chmod / fs_remove_file / fs_rename /
/// fs_copy_file symmetric-error pins.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_remove_dir_non_recursive_symmetric_syscall_error_finalizes_to_failure() {
    use rholang::rust::interpreter::io::errors::FSERR_CODE_NOT_FOUND;

    let dir = tempfile::tempdir().unwrap();
    // No directory at "missing-dir".

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsRemoveDir(`rho:io:fs:native:1.0.0/removeDir`), ackCh in {{
              fsRemoveDir!("{root}", "missing-dir", false, "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[122; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_remove_dir non-recursive symmetric error");
    let leader_wal = leader.fs_handles.wal.snapshot();
    let leader_rd = leader_wal
        .iter()
        .find(|e| e.op == WalOp::RemoveDir)
        .expect("leader must journal a RemoveDir entry");
    match leader_rd.outcome {
        WalOutcome::Failure { code } => assert_eq!(
            code, FSERR_CODE_NOT_FOUND,
            "leader's RemoveDir entry must finalize to Failure with NOT_FOUND \
                 for unlinkat(AT_REMOVEDIR) on missing dir (ENOENT); got {code}"
        ),
        WalOutcome::Success => panic!(
            "leader's RemoveDir entry stayed at Success despite ENOENT.  \
                 Leader H-6 finalize broken.  Entry: {leader_rd:?}"
        ),
    }

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
        .expect("follower evaluate fs_remove_dir non-recursive symmetric error");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal, follower_wal,
        "Phase 4 fs_remove_dir non-recursive symmetric-error: WALs must be \
             byte-identical. A regression that spuriously fired \
             CONSENSUS_DIVERGENCE on the symmetric FSERR_NOT_FOUND would \
             fail here."
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on symmetric syscall error");
}

/// Phase 4 R5(b) pin (Consensus recursive re-execute + verify,
/// 2026-09-02): **fs_remove_dir recursive positive re-execute**.
/// Leader recursively removes a top/nested tree (4 granular WAL
/// entries).  Restore pre-play tree between leader + follower
/// evaluate so follower's own real walk yields the same
/// manifest.  Post-follower: tree is gone.  Load-bearing:
/// follower's WAL has 4 entries with byte-identical paths, and
/// on-disk verification (`!top.exists()`) proves the follower
/// really unlinked.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_remove_dir_recursive_reexecute_removes_follower_tree() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("top");
    let seed_tree = |base: &std::path::Path| {
        std::fs::create_dir(base.join("top")).unwrap();
        std::fs::write(base.join("top/a.txt"), b"a").unwrap();
        std::fs::create_dir(base.join("top/nested")).unwrap();
        std::fs::write(base.join("top/nested/b.txt"), b"b").unwrap();
    };
    seed_tree(dir.path());

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsRemoveDir(`rho:io:fs:native:1.0.0/removeDir`), ackCh in {{
              fsRemoveDir!("{root}", "top", true, "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[123; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_remove_dir recursive positive");
    assert!(
        !target.exists(),
        "leader's recursive removeDir must delete tree"
    );
    assert_eq!(leader.fs_handles.wal.snapshot().len(), 4);

    // Restore pre-play tree — proves follower's own walk +
    // unlinks re-executed the deletion under R5(b).
    seed_tree(dir.path());

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
        .expect("follower evaluate fs_remove_dir recursive positive");

    // LOAD-BEARING: tree gone post-follower.  Regression to
    // the pre-R5(b) mirror-from-cached behavior leaves the
    // tree intact — follower didn't actually run unlink.
    assert!(
        !target.exists(),
        "Phase 4 R5(b) REGRESSION: follower's recursive removeDir did NOT \
             fire — tree still exists after follower.evaluate.  Either \
             fresh-syscall path not engaged (pre-R5(b) mirror-from-cached \
             came back) or Shape A resolver failed to route."
    );

    let leader_wal = leader.fs_handles.wal.snapshot();
    let follower_wal = follower.fs_handles.wal.snapshot();
    assert_eq!(
        leader_wal, follower_wal,
        "Phase 4 R5(b): recursive removeDir WAL must be byte-identical \
             under identical trees on both sides"
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on identical recursive removeDir");
}

/// Phase 4 R5(b) pin (2026-09-02): **fs_remove_dir recursive
/// divergence-detection**.  Leader recursively removes tree.
/// Between evaluate, seed the follower's tempdir with a DIFFERENT
/// tree (one extra file).  Follower's walk yields a different
/// manifest → different reply → verify hash-mismatch →
/// CONSENSUS_DIVERGENCE fires.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_remove_dir_recursive_reexecute_detects_divergence() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("top")).unwrap();
    std::fs::write(dir.path().join("top/a.txt"), b"a").unwrap();

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsRemoveDir(`rho:io:fs:native:1.0.0/removeDir`), ackCh in {{
              fsRemoveDir!("{root}", "top", true, "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[124; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_remove_dir recursive divergence setup");

    // Seed a DIFFERENT tree for the follower — extra file
    // means the follower's walk produces a longer manifest
    // than the leader's cached reply.
    std::fs::create_dir(dir.path().join("top")).unwrap();
    std::fs::write(dir.path().join("top/a.txt"), b"a").unwrap();
    std::fs::write(dir.path().join("top/EXTRA.txt"), b"extra").unwrap();

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

    let rig_result = follower.check_replay_data().await;
    assert!(
        rig_result.is_err(),
        "Phase 4 R5(b): follower's fresh manifest with extra entry \
             must trip RSpace rig verification"
    );

    // The follower's WAL should reflect ITS actual walk
    // (including the EXTRA.txt unlink), giving a different
    // length from the leader's snapshot — proves the follower
    // ran its own walk under R5(b) rather than mirroring
    // leader's cached manifest.
    let leader_wal_len = leader.fs_handles.wal.snapshot().len();
    let follower_wal_len = follower.fs_handles.wal.snapshot().len();
    assert_ne!(
        leader_wal_len, follower_wal_len,
        "Phase 4 R5(b): follower's WAL length must differ from leader's \
             — leader={leader_wal_len}, follower={follower_wal_len}.  A \
             regression to pre-R5(b) mirror-from-cached would produce \
             identical lengths and no divergence signal."
    );
}

/// Phase 4 R5(b) pin (2026-09-02): **fs_remove_dir recursive
/// symmetric syscall error**.  Attempt recursive removeDir on
/// a non-existent target on both sides → both fail identically
/// at safe_descend_verified → same fresh reply → verify OK →
/// no CONSENSUS_DIVERGENCE.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_fs_remove_dir_recursive_symmetric_syscall_error_finalizes_to_failure() {
    let dir = tempfile::tempdir().unwrap();
    // No directory at "missing-tree".

    let (mut leader, mut follower) = create_leader_and_follower().await;

    let term = format!(
        r#"
            new fsRemoveDir(`rho:io:fs:native:1.0.0/removeDir`), ackCh in {{
              fsRemoveDir!("{root}", "missing-tree", true, "consensus", *ackCh) |
              for (@_ <- ackCh) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[125; 32]);

    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .expect("leader evaluate fs_remove_dir recursive symmetric error");
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
        .expect("follower evaluate fs_remove_dir recursive symmetric error");
    let follower_wal = follower.fs_handles.wal.snapshot();

    assert_eq!(
        leader_wal, follower_wal,
        "Phase 4 R5(b) recursive symmetric-error: WALs must be \
             byte-identical when both sides see the same ENOENT on \
             safe_descend_verified"
    );
    follower
        .check_replay_data()
        .await
        .expect("replay data must match on symmetric syscall error");
}

/// H-29-3 slice 1 — Consensus fs_chmod journals a Chmod entry.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chmod_on_consensus_appends_wal_entry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.bin"), b"x").unwrap();
    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsChmod(`rho:io:fs:native:1.0.0/chmod`), ret in {{
              fsChmod!("{root}", "f.bin", 420, "consensus", *ret) |
              for (@_ <- ret) {{ Nil }}
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
    let snap = runtime.fs_handles.wal.snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].op, WalOp::Chmod);
    assert_eq!(snap[0].mode_bits, Some(0o644));
    assert!(snap[0].path.to_string_lossy().ends_with("f.bin"));
}

/// Oracular fs_chmod skips journaling (parity with fd-based
/// mutations).  Even if the syscall succeeds or fails, no WAL
/// entry appears.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chmod_on_oracular_does_not_append_wal() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.bin"), b"x").unwrap();
    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsChmod(`rho:io:fs:native:1.0.0/chmod`), ret in {{
              fsChmod!("{root}", "f.bin", 420, "oracular", *ret) |
              for (@_ <- ret) {{ Nil }}
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

/// Leader/follower WAL byte-identity for Consensus fs_chmod.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chmod_wal_is_byte_identical_on_leader_and_follower() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.bin"), b"x").unwrap();
    let (mut leader, mut follower) = create_leader_and_follower().await;
    let term = format!(
        r#"
            new fsChmod(`rho:io:fs:native:1.0.0/chmod`), ret in {{
              fsChmod!("{root}", "f.bin", 420, "consensus", *ret) |
              for (@_ <- ret) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[91; 32]);
    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .unwrap();
    let l = leader.fs_handles.wal.snapshot();
    let checkpoint = leader.create_checkpoint().await;
    follower.reset(&checkpoint.root).await.unwrap();
    follower.rig(checkpoint.log).await.unwrap();
    follower
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r,
        )
        .await
        .unwrap();
    let f = follower.fs_handles.wal.snapshot();
    assert_eq!(l, f);
    follower.check_replay_data().await.unwrap();
}

/// H-29-3 slice 1 — Consensus fs_remove_file journals a
/// RemoveFile entry.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remove_file_on_consensus_appends_wal_entry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("victim.bin"), b"").unwrap();
    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsRemoveFile(`rho:io:fs:native:1.0.0/removeFile`), ret in {{
              fsRemoveFile!("{root}", "victim.bin", "consensus", *ret) |
              for (@_ <- ret) {{ Nil }}
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
    let snap = runtime.fs_handles.wal.snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].op, WalOp::RemoveFile);
    assert!(snap[0].path.to_string_lossy().ends_with("victim.bin"));
    // Actually deleted from disk.
    assert!(!dir.path().join("victim.bin").exists());
}

/// H-29-3 slice 1 — Consensus fs_rename journals a Rename entry
/// with `path` = from-canon-path and `extra_path` = to-canon-path.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rename_on_consensus_appends_wal_entry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.bin"), b"").unwrap();
    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsRename(`rho:io:fs:native:1.0.0/rename`), ret in {{
              fsRename!("{root}", "a.bin", "{root}", "b.bin", "consensus", *ret) |
              for (@_ <- ret) {{ Nil }}
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
    let snap = runtime.fs_handles.wal.snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].op, WalOp::Rename);
    assert!(snap[0].path.to_string_lossy().ends_with("a.bin"));
    let extra = snap[0]
        .extra_path
        .as_ref()
        .expect("Rename must carry extra_path");
    assert!(extra.to_string_lossy().ends_with("b.bin"));
    assert!(dir.path().join("b.bin").exists());
    assert!(!dir.path().join("a.bin").exists());
}

/// H-29-3 slice 1 — Consensus fs_copy_file journals a CopyFile
/// entry.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn copy_file_on_consensus_appends_wal_entry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("src.bin"), b"payload").unwrap();
    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsCopyFile(`rho:io:fs:native:1.0.0/copyFile`), ret in {{
              fsCopyFile!("{root}", "src.bin", "{root}", "dst.bin", "consensus", *ret) |
              for (@_ <- ret) {{ Nil }}
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
    let snap = runtime.fs_handles.wal.snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].op, WalOp::CopyFile);
    assert_eq!(
        std::fs::read(dir.path().join("dst.bin")).unwrap(),
        b"payload"
    );
}

/// Phase 4 ban pin (2026-09-02, post-security-review S-2):
/// **fs_chown with cmode="consensus" MUST reject with
/// `FSERR_UNSUPPORTED`.**  See handlers.rs::fs_chown for the
/// design rationale: WAL captures owner/group as caller-supplied
/// String values (e.g., "bob"), NSS-mapping ("bob" → uid) is
/// host-local, and two validators with different /etc/passwd
/// entries would land different uids on-disk without any
/// signal to the consensus layer.  The Consensus verify pattern
/// (compare fresh vs cached reply hash) doesn't catch this
/// because fchownat's reply is `[true]` regardless of the uid
/// it actually stamped.
///
/// Pre-2026-09-02 this test asserted the opposite — that
/// Consensus fs_chown journals a Chown WAL entry.  That
/// behavior masked NSS divergence silently under the Phase-0
/// tautological cached-reply consumption path.  Rewritten to
/// pin the ban.
///
/// A regression that dropped the ban would let Consensus chown
/// through; the WAL would journal string-typed owner/group,
/// and two validators with divergent NSS would produce
/// divergent on-disk uids while their WALs stayed byte-identical.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chown_on_consensus_rejects_with_fserr_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.bin"), b"").unwrap();
    let runtime = create_runtime().await;

    let term = format!(
        r#"
            new fsChown(`rho:io:fs:native:1.0.0/chown`), ret in {{
              fsChown!("{root}", "f.bin", "someuser", Nil, "consensus", *ret) |
              for (@reply <- ret) {{
                @"result"!(reply)
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
        .expect("evaluate Consensus fs_chown");

    // Assert the reply is specifically FSERR_UNSUPPORTED.  A
    // regression that returned FSERR_BAD_ARG / FSERR_IO / etc.
    // would still produce no WAL entry (and the wal.is_empty()
    // check below would still pass), so we need the code-slot
    // check to lock the specific FSERR down.
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::Expr;
    use rholang::rust::interpreter::io::errors::FSERR_UNSUPPORTED;
    use rholang::rust::interpreter::io::response::extract_err_code;
    use rholang::rust::interpreter::rho_runtime::RhoRuntime;
    let result_channel = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString("result".to_string())),
    }]);
    let datums = runtime.get_data(&result_channel).await;
    let reply_par = datums
        .first()
        .and_then(|d| d.a.pars.first())
        .cloned()
        .expect(
            "no reply on @\"result\" — the ban's early-return produce didn't \
                 land, or the term shape changed",
        );
    let code = extract_err_code(std::slice::from_ref(&reply_par)).expect(
        "reply must be an [false, code, msg] error shape from the ban's \
             early-return; got a non-error reply",
    );
    assert_eq!(
        code, FSERR_UNSUPPORTED,
        "Consensus fs_chown rejection must use FSERR_UNSUPPORTED specifically \
             (see handlers.rs::fs_chown ban comment).  Got code: {code}"
    );

    // No WAL entry should be journaled since the ban fires
    // before journal_path_mutation_single is called.
    assert!(
        runtime.fs_handles.wal.is_empty(),
        "Consensus fs_chown rejection must NOT journal — the handler errored \
             out before any WAL append.  Got WAL: {:?}",
        runtime.fs_handles.wal.snapshot()
    );
}

/// H-29-3 slice 1 — leader/follower WAL byte-identity for all
/// five lifted single-op path mutations.  Runs a mixed sequence
/// on the leader, replays on the follower, asserts identical
/// WAL entries.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn h_29_3_slice_1_wal_byte_identity_across_all_five_ops() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.bin"), b"payload").unwrap();
    std::fs::write(dir.path().join("g.bin"), b"other").unwrap();
    let (mut leader, mut follower) = create_leader_and_follower().await;
    // Sequence: chmod → copyFile → rename → removeFile.  (chown
    // omitted: post-2026-09-02 S-2 ban, Consensus fs_chown returns
    // FSERR_UNSUPPORTED without journaling — the ban is pinned by
    // chown_on_consensus_rejects_with_fserr_unsupported.)
    let term = format!(
        r#"
            new fsChmod(`rho:io:fs:native:1.0.0/chmod`),
                fsCopyFile(`rho:io:fs:native:1.0.0/copyFile`),
                fsRename(`rho:io:fs:native:1.0.0/rename`),
                fsRemoveFile(`rho:io:fs:native:1.0.0/removeFile`),
                c1, c2, c3, c4 in {{
              fsChmod!("{root}", "f.bin", 420, "consensus", *c1) |
              for (@_ <- c1) {{
                fsCopyFile!("{root}", "f.bin", "{root}", "h.bin", "consensus", *c2) |
                for (@_ <- c2) {{
                  fsRename!("{root}", "h.bin", "{root}", "i.bin", "consensus", *c3) |
                  for (@_ <- c3) {{
                    fsRemoveFile!("{root}", "g.bin", "consensus", *c4) |
                    for (@_ <- c4) {{ Nil }}
                  }}
                }}
              }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[123; 32]);
    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .unwrap();
    let l = leader.fs_handles.wal.snapshot();
    assert_eq!(l.len(), 4);
    assert_eq!(l[0].op, WalOp::Chmod);
    assert_eq!(l[1].op, WalOp::CopyFile);
    assert_eq!(l[2].op, WalOp::Rename);
    assert_eq!(l[3].op, WalOp::RemoveFile);
    // Phase 4 (2026-09-02): under path-mutation re-execute, the
    // follower's real syscalls run against the shared tempdir
    // files that leader already mutated.  Restore pre-play
    // state so follower's re-executes succeed symmetrically:
    //   - fs_chmod: idempotent (re-chmoding to the same mode
    //     succeeds regardless of current mode); no restore
    //     needed.
    //   - fs_copy_file: real re-execute (Phase 4) opens dest
    //     with O_CREAT|O_TRUNC and rewrites bytes from source.
    //     h.bin's post-restore state (below) gives the follower
    //     the same source-of-truth as the leader had; the
    //     O_TRUNC semantics mean the presence/absence of h.bin
    //     pre-follower doesn't matter for this op, but restore
    //     below covers the fs_rename step's precondition.
    //   - fs_rename: leader moved h.bin → i.bin.  Follower's
    //     renameat needs h.bin present and i.bin absent.
    //   - fs_remove_file: leader removed g.bin.  Follower's
    //     unlinkat needs g.bin present.
    std::fs::write(dir.path().join("g.bin"), b"other").unwrap();
    std::fs::remove_file(dir.path().join("i.bin")).unwrap();
    std::fs::write(dir.path().join("h.bin"), b"payload").unwrap();
    let checkpoint = leader.create_checkpoint().await;
    follower.reset(&checkpoint.root).await.unwrap();
    follower.rig(checkpoint.log).await.unwrap();
    follower
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r,
        )
        .await
        .unwrap();
    let f = follower.fs_handles.wal.snapshot();
    assert_eq!(
        l, f,
        "leader/follower WAL byte-identity across the 4-op mixed sequence"
    );
    follower.check_replay_data().await.unwrap();
}

/// Non-recursive Consensus removeDir emits a single RemoveDir
/// entry (fully derivable from args).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remove_dir_non_recursive_on_consensus_appends_wal_entry() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("empty")).unwrap();
    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsRemoveDir(`rho:io:fs:native:1.0.0/removeDir`), ret in {{
              fsRemoveDir!("{root}", "empty", false, "consensus", *ret) |
              for (@_ <- ret) {{ Nil }}
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
    let snap = runtime.fs_handles.wal.snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].op, WalOp::RemoveDir);
    assert!(snap[0].path.to_string_lossy().ends_with("empty"));
    assert!(!dir.path().join("empty").exists());
}

/// Recursive Consensus removeDir emits granular RemoveFile /
/// RemoveDir entries in sorted post-order against a three-level
/// tree:
///
///     top/
///       a.txt
///       nested/
///         b.txt
///
/// Sorted per-directory + post-order (children before parents)
/// yields: RemoveFile(top/a.txt), RemoveFile(top/nested/b.txt),
/// RemoveDir(top/nested), RemoveDir(top).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remove_dir_recursive_on_consensus_emits_granular_manifest() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("top")).unwrap();
    std::fs::write(dir.path().join("top/a.txt"), b"").unwrap();
    std::fs::create_dir(dir.path().join("top/nested")).unwrap();
    std::fs::write(dir.path().join("top/nested/b.txt"), b"").unwrap();
    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsRemoveDir(`rho:io:fs:native:1.0.0/removeDir`), ret in {{
              fsRemoveDir!("{root}", "top", true, "consensus", *ret) |
              for (@_ <- ret) {{ Nil }}
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
    let snap = runtime.fs_handles.wal.snapshot();
    assert_eq!(snap.len(), 4, "expected 4 granular entries, got {snap:#?}");
    assert_eq!(snap[0].op, WalOp::RemoveFile);
    assert!(snap[0].path.to_string_lossy().ends_with("top/a.txt"));
    assert_eq!(snap[1].op, WalOp::RemoveFile);
    assert!(snap[1].path.to_string_lossy().ends_with("top/nested/b.txt"));
    assert_eq!(snap[2].op, WalOp::RemoveDir);
    assert!(snap[2].path.to_string_lossy().ends_with("top/nested"));
    assert_eq!(snap[3].op, WalOp::RemoveDir);
    assert!(snap[3].path.to_string_lossy().ends_with("top"));
    assert!(!dir.path().join("top").exists());
}

/// Oracular recursive removeDir does NOT journal.  Reply shape
/// unchanged (`[true]`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remove_dir_recursive_on_oracular_does_not_journal() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("top")).unwrap();
    std::fs::write(dir.path().join("top/a.txt"), b"").unwrap();
    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsRemoveDir(`rho:io:fs:native:1.0.0/removeDir`), ret in {{
              fsRemoveDir!("{root}", "top", true, "oracular", *ret) |
              for (@_ <- ret) {{ Nil }}
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
    assert!(!dir.path().join("top").exists());
}

/// Fresh-tree applier can reconstruct the tree state from a
/// granular removeDir manifest.  Closes the file-state-identity
/// loop for the recursive case (sibling files outside the
/// removed subtree must survive).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pb_m_14_file_state_identity_recursive_remove_dir() {
    let leader_dir = tempfile::tempdir().unwrap();
    let follower_dir = tempfile::tempdir().unwrap();
    for base in [leader_dir.path(), follower_dir.path()] {
        std::fs::create_dir(base.join("top")).unwrap();
        std::fs::write(base.join("top/a.txt"), b"aa").unwrap();
        std::fs::create_dir(base.join("top/nested")).unwrap();
        std::fs::write(base.join("top/nested/b.txt"), b"bb").unwrap();
        std::fs::write(base.join("survivor.bin"), b"keep").unwrap();
    }
    let leader = create_runtime().await;
    let term = format!(
        r#"
            new fsRemoveDir(`rho:io:fs:native:1.0.0/removeDir`), ret in {{
              fsRemoveDir!("{root}", "top", true, "consensus", *ret) |
              for (@_ <- ret) {{ Nil }}
            }}
            "#,
        root = leader_dir.path().display(),
    );
    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            Blake2b512Random::create_from_bytes(&[77u8; 32]),
        )
        .await
        .unwrap();
    let wal = leader.fs_handles.wal.snapshot();
    apply_wal_translated(
        &wal,
        &std::collections::HashMap::new(),
        leader_dir.path(),
        follower_dir.path(),
    );
    assert_dir_trees_byte_identical(leader_dir.path(), follower_dir.path(), &[]);
}

/// Leader/follower WAL byte-identity for recursive Consensus
/// removeDir.  Under R5(b) (2026-09-02) the follower does REAL
/// per-entry syscalls against its own subdir (rather than
/// mirroring the leader's cached manifest), so the shared-
/// tempdir test-harness needs to restore the tree between
/// leader + follower evaluate to give the follower an
/// equivalent walk.  The invariant proved is: given identical
/// on-disk trees, leader and follower produce byte-identical
/// WAL entries via the relative-path manifest.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recursive_remove_dir_wal_is_byte_identical_on_leader_and_follower() {
    let dir = tempfile::tempdir().unwrap();
    let seed_tree = |base: &std::path::Path| {
        std::fs::create_dir(base.join("top")).unwrap();
        std::fs::write(base.join("top/a.txt"), b"a").unwrap();
        std::fs::create_dir(base.join("top/nested")).unwrap();
        std::fs::write(base.join("top/nested/b.txt"), b"b").unwrap();
    };
    seed_tree(dir.path());
    let (mut leader, mut follower) = create_leader_and_follower().await;
    let term = format!(
        r#"
            new fsRemoveDir(`rho:io:fs:native:1.0.0/removeDir`), ret in {{
              fsRemoveDir!("{root}", "top", true, "consensus", *ret) |
              for (@_ <- ret) {{ Nil }}
            }}
            "#,
        root = dir.path().display(),
    );
    let r = Blake2b512Random::create_from_bytes(&[55u8; 32]);
    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r.clone(),
        )
        .await
        .unwrap();
    let l = leader.fs_handles.wal.snapshot();
    assert_eq!(l.len(), 4, "leader produced 4 granular entries");
    // R5(b): restore pre-play state so follower's real walk
    // yields the same manifest the leader produced.
    seed_tree(dir.path());
    let checkpoint = leader.create_checkpoint().await;
    follower.reset(&checkpoint.root).await.unwrap();
    follower.rig(checkpoint.log).await.unwrap();
    follower
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            r,
        )
        .await
        .unwrap();
    let f = follower.fs_handles.wal.snapshot();
    assert_eq!(
        l, f,
        "leader/follower recursive-removeDir WAL byte-identity via \
             R5(b) relative-path manifest"
    );
    follower.check_replay_data().await.unwrap();
}

/// H-29-3 slice 1 — a failed Consensus mutation appends a
/// Failure-outcome entry (H-6 pattern).  Uses fs_remove_file on
/// a nonexistent path so the syscall reliably fails with ENOENT.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failed_remove_file_on_consensus_appends_failure_entry() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = create_runtime().await;
    let term = format!(
        r#"
            new fsRemoveFile(`rho:io:fs:native:1.0.0/removeFile`), ret in {{
              fsRemoveFile!("{root}", "does-not-exist.bin", "consensus", *ret) |
              for (@_ <- ret) {{ Nil }}
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
    let snap = runtime.fs_handles.wal.snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].op, WalOp::RemoveFile);
    match snap[0].outcome {
        WalOutcome::Failure { .. } => {}
        other => panic!("expected Failure outcome after ENOENT, got {other:?}"),
    }
}

/// **PB-M-14 file-state-identity pin (2026-08-26).**  Closes the
/// second half of the PB-M-14 property — the first being WAL-byte-
/// identity between leader and follower (covered by
/// `multi_deploy_wal_is_byte_identical_on_leader_and_follower`).
///
/// Structure:
///   1. Two identical base trees at different temp dirs
///      (leader_dir + follower_dir, each seeded with the same
///      initial file bytes at the same relative paths).
///   2. A leader runtime evaluates a sequence of Consensus
///      `fsWriteAt` / `fsTruncate` deploys against leader_dir.
///      The driver knows the exact bytes it passes to `fsWriteAt!`
///      and hashes them into a payload sidecar keyed by
///      `PayloadRef::hash(bytes)` — the same key the WAL entries
///      carry (a real Phase 7b joiner obtains this sidecar from
///      peers via `get_wal_payload`).
///   3. `apply_wal_to_fresh_tree` replays the WAL onto follower_dir
///      using ONLY the WAL entries + sidecar (no rig, no shared
///      store, no Rholang re-execution).
///   4. `assert_dir_trees_byte_identical` verifies leader_dir and
///      follower_dir are byte-identical after replay.
///
/// Regression scenarios this pin catches:
/// * A WAL entry mis-records `offset` for a `WriteAt` (follower
///   writes at the wrong place → byte divergence).
/// * A `Failure` outcome that the handler wrongly marked
///   `Success` on the leader (follower attempts a write that
///   the leader never performed → byte divergence).
/// * `payload_ref` computed over a different byte slice than
///   the one actually written (follower's sidecar lookup finds
///   no matching hash → applier panics).
/// * `canon_path` on the WAL entry loses the `rel` component
///   (all writes collapse onto canon_root itself → follower's
///   `rel` file never gets touched → byte divergence).
/// * `Truncate` mis-records the target length (byte divergence
///   on file size / tail contents).
///
/// Not covered here (still Path B or a follow-up slice):
/// * Full E2E through Casper block-processing on a two-node
///   network (`pb_m_14_two_validator_scaffold` docstring).
/// * Path-based mutations (chmod/chown/remove/rename/copy) — the
///   handler-side WAL append for those is not yet wired.
///
/// Sequential `Write` reconstruction: covered by the sibling
/// test `pb_m_14_file_state_identity_sequential_write` after the
/// position-follow-up (2026-08-26) — sequential Write now
/// records absolute offset in the WAL from the FileHandle's
/// shadow position, so the applier handles it identically to
/// WriteAt.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pb_m_14_file_state_identity_via_wal_replay() {
    // Two dirs with IDENTICAL base contents at IDENTICAL relative
    // paths.  The leader mutates leader_dir; the applier
    // reconstructs the same state on follower_dir purely from the
    // WAL + payload sidecar.
    let leader_dir = tempfile::tempdir().unwrap();
    let follower_dir = tempfile::tempdir().unwrap();
    for base in [leader_dir.path(), follower_dir.path()] {
        std::fs::write(base.join("data.bin"), vec![0u8; 128]).unwrap();
        std::fs::write(base.join("log.txt"), vec![0u8; 64]).unwrap();
    }

    let leader = create_runtime().await;
    let leader_root_str = leader_dir.path().display().to_string();

    // Deploy 1: WriteAt(0, "aabb") to data.bin (Consensus).
    // Deploy 2: WriteAt(10, "ccdd") + Truncate(32) on data.bin.
    // Deploy 3: WriteAt(3, "ff") to log.txt.
    //
    // Payload sidecar is populated by hashing the exact bytes the
    // Rholang literal is decoded to.  Any mismatch between the
    // sidecar keys and the WAL's payload_ref hashes would trip
    // the applier's `missing hash` panic — closing that loop is
    // part of the point of this test.
    let mut sidecar: std::collections::HashMap<[u8; 32], Vec<u8>> =
        std::collections::HashMap::new();
    let mut record = |bytes: &[u8]| {
        if let PayloadRef::Hash(h) = PayloadRef::hash(bytes) {
            sidecar.insert(h, bytes.to_vec());
        }
    };
    record(&[0xaa, 0xbb]);
    record(&[0xcc, 0xdd]);
    record(&[0xff]);

    let deploys: Vec<(String, [u8; 32])> = vec![
        (
            format!(
                r#"
                    new fsOpen(`rho:io:fs:native:1.0.0/open`),
                        fsWriteAt(`rho:io:fs:native:1.0.0/writeAt`),
                        fsClose(`rho:io:fs:native:1.0.0/close`),
                        oc, wc, cc in {{
                      fsOpen!("{leader_root_str}", "data.bin", "r+", "consensus", *oc) |
                      for (@[true, fd] <- oc) {{
                        fsWriteAt!(fd, 0, "aabb".hexToBytes(), *wc) |
                        for (@_ <- wc) {{
                          fsClose!(fd, *cc) |
                          for (@_ <- cc) {{ Nil }}
                        }}
                      }}
                    }}
                    "#
            ),
            [1u8; 32],
        ),
        (
            format!(
                r#"
                    new fsOpen(`rho:io:fs:native:1.0.0/open`),
                        fsWriteAt(`rho:io:fs:native:1.0.0/writeAt`),
                        fsTruncate(`rho:io:fs:native:1.0.0/truncate`),
                        fsClose(`rho:io:fs:native:1.0.0/close`),
                        oc, wc, tc, cc in {{
                      fsOpen!("{leader_root_str}", "data.bin", "r+", "consensus", *oc) |
                      for (@[true, fd] <- oc) {{
                        fsWriteAt!(fd, 10, "ccdd".hexToBytes(), *wc) |
                        for (@_ <- wc) {{
                          fsTruncate!(fd, 32, *tc) |
                          for (@_ <- tc) {{
                            fsClose!(fd, *cc) |
                            for (@_ <- cc) {{ Nil }}
                          }}
                        }}
                      }}
                    }}
                    "#
            ),
            [2u8; 32],
        ),
        (
            format!(
                r#"
                    new fsOpen(`rho:io:fs:native:1.0.0/open`),
                        fsWriteAt(`rho:io:fs:native:1.0.0/writeAt`),
                        fsClose(`rho:io:fs:native:1.0.0/close`),
                        oc, wc, cc in {{
                      fsOpen!("{leader_root_str}", "log.txt", "r+", "consensus", *oc) |
                      for (@[true, fd] <- oc) {{
                        fsWriteAt!(fd, 3, "ff".hexToBytes(), *wc) |
                        for (@_ <- wc) {{
                          fsClose!(fd, *cc) |
                          for (@_ <- cc) {{ Nil }}
                        }}
                      }}
                    }}
                    "#
            ),
            [3u8; 32],
        ),
    ];

    for (term, seed) in &deploys {
        leader
            .evaluate(
                term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                Blake2b512Random::create_from_bytes(seed),
            )
            .await
            .expect("leader evaluate");
    }

    let wal = leader.fs_handles.wal.snapshot();
    assert!(
        !wal.is_empty(),
        "leader must have journaled at least one Consensus write"
    );

    // Pre-condition sanity: every WriteAt entry's payload_ref
    // MUST be a key in the sidecar.  A miss here means the
    // driver's hashed-bytes don't match the WAL's — a bug in
    // this test, or a regression in payload_ref computation.
    for (i, entry) in wal.iter().enumerate() {
        if entry.op == WalOp::WriteAt && matches!(entry.outcome, WalOutcome::Success) {
            if let Some(PayloadRef::Hash(h)) = entry.payload_ref {
                assert!(
                    sidecar.contains_key(&h),
                    "WAL entry {i} references hash {} not in the driver's \
                         sidecar — either the driver didn't record the bytes it \
                         fed to fsWriteAt, or the handler's payload_ref hash \
                         diverged from the actual bytes written",
                    hex::encode(h),
                );
            }
        }
    }

    apply_wal_translated(&wal, &sidecar, leader_dir.path(), follower_dir.path());

    assert_dir_trees_byte_identical(leader_dir.path(), follower_dir.path(), &[]);
}

/// **PB-M-14 file-state-identity — sequential-Write leg
/// (position-follow-up, 2026-08-26).**  Companion to
/// `pb_m_14_file_state_identity_via_wal_replay` covering the
/// path the earlier test explicitly did not: sequential
/// `fsWrite` (offset absent from Rholang, filled in by the
/// handler from `FileHandle.position`).
///
/// After the position-follow-up, sequential Write records
/// absolute offset in the WAL derived from the fd's shadow
/// position at journal time (which both leader and follower
/// evolve deterministically).  The applier then handles it
/// identically to WriteAt.  This test drives a three-write
/// sequence on a single fd (positions 0 → 4 → 12 across the
/// three writes) so the shadow-position update between writes
/// is exercised, plus a Seek and a Write-after-Seek to
/// exercise the seek-position sync.
///
/// Regression scenarios this pin catches (on top of the
/// existing WriteAt/Truncate coverage):
/// * `journal_write` records `offset=None` for sequential
///   Write (regression to the pre-follow-up shape) → applier's
///   `{:?} without offset` panic fires.
/// * Shadow position not advanced after successful write →
///   subsequent sequential Write records offset=0 (same as
///   first write) → applier overwrites earlier bytes → byte
///   divergence.
/// * `fs_seek` doesn't update shadow position → subsequent
///   sequential Write records wrong offset → byte divergence.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pb_m_14_file_state_identity_sequential_write() {
    let leader_dir = tempfile::tempdir().unwrap();
    let follower_dir = tempfile::tempdir().unwrap();
    // Base file: 64 zero bytes.  Sequential writes will land
    // at offsets 0, 4, 12 (via a mid-sequence Seek); then
    // truncate to 20 bytes so tail-comparison is meaningful.
    for base in [leader_dir.path(), follower_dir.path()] {
        std::fs::write(base.join("data.bin"), vec![0u8; 64]).unwrap();
    }

    let leader = create_runtime().await;
    let leader_root_str = leader_dir.path().display().to_string();

    // Sidecar for every byte-payload the driver expects to hash.
    let mut sidecar: std::collections::HashMap<[u8; 32], Vec<u8>> =
        std::collections::HashMap::new();
    let mut record = |bytes: &[u8]| {
        if let PayloadRef::Hash(h) = PayloadRef::hash(bytes) {
            sidecar.insert(h, bytes.to_vec());
        }
    };
    record(&[0x11, 0x22, 0x33, 0x44]); // write 1 @ pos 0 → advances to 4
    record(&[0x55, 0x66, 0x77, 0x88]); // write 2 @ pos 4 → advances to 8
    record(&[0x99, 0xAA, 0xBB, 0xCC]); // write 3 @ pos 12 (after seek)

    // Single deploy that opens once, writes three times (with
    // a seek in the middle), truncates, and closes.  Runs
    // multiple sequential writes on the SAME fd so the shadow-
    // position advance between writes is on the critical
    // path.
    let term = format!(
        r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                fsSeek(`rho:io:fs:native:1.0.0/seek`),
                fsTruncate(`rho:io:fs:native:1.0.0/truncate`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, w1, w2, sk, w3, tc, cc in {{
              fsOpen!("{leader_root_str}", "data.bin", "r+", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsWrite!(fd, "11223344".hexToBytes(), *w1) |
                for (@_ <- w1) {{
                  fsWrite!(fd, "55667788".hexToBytes(), *w2) |
                  for (@_ <- w2) {{
                    fsSeek!(fd, 12, "set", *sk) |
                    for (@_ <- sk) {{
                      fsWrite!(fd, "99aabbcc".hexToBytes(), *w3) |
                      for (@_ <- w3) {{
                        fsTruncate!(fd, 20, *tc) |
                        for (@_ <- tc) {{
                          fsClose!(fd, *cc) |
                          for (@_ <- cc) {{ Nil }}
                        }}
                      }}
                    }}
                  }}
                }}
              }}
            }}
            "#
    );
    leader
        .evaluate(
            &term,
            Cost::unsafe_max(),
            std::collections::HashMap::new(),
            Blake2b512Random::create_from_bytes(&[7u8; 32]),
        )
        .await
        .expect("leader evaluate");

    let wal = leader.fs_handles.wal.snapshot();
    // Expect: three Write entries + one Truncate = 4 WAL
    // entries.  fs_seek is NOT journaled (no WalOp::Seek).
    assert_eq!(
        wal.len(),
        4,
        "expected 3 Write + 1 Truncate entries, got {} entries: {wal:?}",
        wal.len(),
    );
    // Verify the three Writes carry the expected shadow-position
    // offsets: 0, 4, 12 (post-seek).
    assert_eq!(wal[0].op, WalOp::Write);
    assert_eq!(wal[0].offset, Some(0), "first sequential write @ pos 0");
    assert_eq!(wal[1].op, WalOp::Write);
    assert_eq!(
        wal[1].offset,
        Some(4),
        "second sequential write @ pos 4 (after 4-byte first write)",
    );
    assert_eq!(wal[2].op, WalOp::Write);
    assert_eq!(
        wal[2].offset,
        Some(12),
        "third sequential write @ pos 12 (post-Seek SET to 12)",
    );
    assert_eq!(wal[3].op, WalOp::Truncate);
    assert_eq!(wal[3].offset, Some(20));

    // Sanity: every write's payload_ref is in the sidecar.
    for (i, entry) in wal.iter().enumerate() {
        if entry.op == WalOp::Write {
            if let Some(PayloadRef::Hash(h)) = entry.payload_ref {
                assert!(
                    sidecar.contains_key(&h),
                    "WAL entry {i} references hash {} not in sidecar",
                    hex::encode(h),
                );
            }
        }
    }

    apply_wal_translated(&wal, &sidecar, leader_dir.path(), follower_dir.path());
    assert_dir_trees_byte_identical(leader_dir.path(), follower_dir.path(), &[]);
}
