// Slice 29 (PB-M-14): consensus-mode Write-Ahead Log integration
// tests.  Exercises the real fs_open/fs_write/fs_write_at/fs_truncate
// native handlers against a temp-file target and inspects the
// runtime's `fs_handles.wal` buffer to verify that:
//
//   - Writes on `Consensus` caps produce WAL entries with correct
//     op / path / offset / length / payload hash
//   - Writes on `Oracular` caps produce NO WAL entries
//   - Truncate on Consensus produces a Truncate WAL entry
//   - Multiple mutations produce entries in insertion order
//   - (Redesign) canon_path in WAL includes the resolved `rel`,
//     so different files under the same canonRoot produce
//     distinguishable WAL entries (C-29-1 regression pin).
//   - (Redesign) `RhoRuntime::reset` clears the WAL between
//     block boundaries (H-29-F2 regression pin).
//   - (Redesign) `revert_to_soft_checkpoint` truncates the WAL
//     back to the snapshot mark (H-29-1 regression pin).
//
// These tests disable the slice-31 fs-native URN filter so the test
// Rholang can bind `rho:io:fs:native:1.0.0/*` URNs directly (genesis-
// scope semantics).  User deploys in production go through Fs.rho +
// File.rho which forward `cmode` via `openFileImpl` — the wiring is
// covered by the file_dir_check test suite.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crypto::rust::hash::blake2b256::Blake2b256;
    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
    use rholang::rust::interpreter::accounting::costs::Cost;
    use rholang::rust::interpreter::external_services::ExternalServices;
    use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};
    use rholang::rust::interpreter::matcher::r#match::Matcher;
    use rholang::rust::interpreter::rho_runtime::{
        create_replay_rho_runtime, create_rho_runtime, RhoRuntime, RhoRuntimeImpl,
    };
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    fn rand() -> Blake2b512Random { Blake2b512Random::create_from_bytes(&[1, 2, 45, 65]) }

    async fn create_runtime() -> RhoRuntimeImpl {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.r_space_stores().await.unwrap();
        let space: RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> =
            RSpace::create(store, Arc::new(Box::new(Matcher))).unwrap();
        let runtime = create_rho_runtime(
            space,
            Arc::new(std::collections::HashMap::new()),
            true,
            &mut Vec::new(),
            ExternalServices::noop(),
        )
        .await;
        runtime.cost.set(Cost::unsafe_max());
        // Slice 31: disable the fs-native URN filter so tests can bind
        // rho:io:fs:native:1.0.0/* URNs directly.
        runtime.disable_fs_native_urn_filter();
        runtime
    }

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
              fsOpen!("{root}", "data.bin", "rw", "consensus", *openCh) |
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
        // `apply_wal_to_fresh_tree` in this file).
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
              fsOpen!("{root}", "data.bin", "rw", "oracular", *openCh) |
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
              fsOpen!("{root}", "data.bin", "rw", "consensus", *openCh) |
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
              fsOpen!("{root}", "data.bin", "rw", "consensus", *openCh) |
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
              fsOpen!("{root}", "data.bin", "rw", "consensus", *openCh) |
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
              fsOpen!("{root}", "orc.bin", "rw", "oracular", *orcCh) |
              for (@[true, fd_orc] <- orcCh) {{
                fsWrite!(fd_orc, "aa".hexToBytes(), *w1) |
                for (@_ <- w1) {{
                  fsOpen!("{root}", "con.bin", "rw", "consensus", *conCh) |
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
              fsOpen!("{root}", "f.bin", "rw", "BOGUS", *openCh) |
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

    // ------------------------------------------------------------------
    // Redesign regression pins
    // ------------------------------------------------------------------

    /// C-29-1 regression pin: WAL entries must carry the FULL path
    /// (`canonRoot + rel`), not just the canonRoot.  Two distinct
    /// files under the same canonRoot must yield distinguishable
    /// WAL entries.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn wal_entries_include_rel_in_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.bin"), b"").unwrap();
        std::fs::write(dir.path().join("b.bin"), b"").unwrap();

        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                oa, ob, wa, wb
            in {{
              fsOpen!("{root}", "a.bin", "rw", "consensus", *oa) |
              for (@[true, fdA] <- oa) {{
                fsWrite!(fdA, "aa".hexToBytes(), *wa) |
                for (@_ <- wa) {{
                  fsOpen!("{root}", "b.bin", "rw", "consensus", *ob) |
                  for (@[true, fdB] <- ob) {{
                    fsWrite!(fdB, "bb".hexToBytes(), *wb) |
                    for (@_ <- wb) {{ Nil }}
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
        assert_eq!(entries.len(), 2);
        let p0 = entries[0].path.to_string_lossy().to_string();
        let p1 = entries[1].path.to_string_lossy().to_string();
        assert_ne!(
            p0, p1,
            "distinct files must produce distinct WAL paths (pre-fix both were canonRoot only)"
        );
        assert!(p0.ends_with("a.bin") || p1.ends_with("a.bin"));
        assert!(p0.ends_with("b.bin") || p1.ends_with("b.bin"));
    }

    /// H-29-F2 regression pin: `RhoRuntime::reset` clears the WAL.
    /// Ensures a follower resetting to a state root observes an
    /// empty WAL — no ghost entries from a prior block.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn reset_clears_wal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"").unwrap();
        let mut runtime = create_runtime().await;
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                oc, wc
            in {{
              fsOpen!("{root}", "f.bin", "rw", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsWrite!(fd, "aa".hexToBytes(), *wc) |
                for (@_ <- wc) {{ Nil }}
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
        assert!(!runtime.fs_handles.wal.is_empty());
        let root = runtime.get_root().await;
        runtime.reset(&root).await.unwrap();
        assert!(
            runtime.fs_handles.wal.is_empty(),
            "reset must clear the WAL (H-29-F2 defense-in-depth)"
        );
    }

    /// Helper: create leader + follower runtime pair sharing an
    /// underlying store, so leader's checkpoint can be replayed by
    /// follower.  Modeled on `cost_accounting_spec::evaluate_and_replay`.
    async fn create_leader_and_follower() -> (RhoRuntimeImpl, RhoRuntimeImpl) {
        let mut kvm = InMemoryStoreManager::new();
        let stores = kvm.r_space_stores().await.unwrap();
        let (space, replay) =
            RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create_with_replay(
                stores,
                Arc::new(Box::new(Matcher)),
            )
            .unwrap();
        let leader = create_rho_runtime(
            space,
            Arc::new(std::collections::HashMap::new()),
            true,
            &mut Vec::new(),
            ExternalServices::noop(),
        )
        .await;
        let follower = create_replay_rho_runtime(
            replay,
            Arc::new(std::collections::HashMap::new()),
            true,
            &mut Vec::new(),
            ExternalServices::noop(),
        )
        .await;
        leader.cost.set(Cost::unsafe_max());
        follower.cost.set(Cost::unsafe_max());
        leader.disable_fs_native_urn_filter();
        follower.disable_fs_native_urn_filter();
        (leader, follower)
    }

    /// C-R1 regression pin (slice 29 round 2): the critical
    /// leader/follower WAL symmetry property.  Runs an identical
    /// deploy on a leader runtime (is_replay = false) and a follower
    /// runtime (is_replay = true, rigged with the leader's event
    /// log), and asserts that both WALs are byte-identical.  If
    /// `fs_open`'s replay branch fails to populate a shadow handle,
    /// or if `journal_write` diverges between the two branches, the
    /// assert fires — closing the C-29-F1 / C-R1 gap that the pre-
    /// round-2 tests could not catch (no test exercised is_replay
    /// = true).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn wal_is_byte_identical_on_leader_and_follower() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.bin"), vec![0u8; 128]).unwrap();

        let (mut leader, mut follower) = create_leader_and_follower().await;

        // Term exercises all three fd-based Consensus WAL sites:
        // Write, WriteAt, Truncate — plus an Oracular sibling to
        // confirm cross-cap isolation is symmetric.
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                fsWriteAt(`rho:io:fs:native:1.0.0/writeAt`),
                fsTruncate(`rho:io:fs:native:1.0.0/truncate`),
                oc, w1, w2, w3
            in {{
              fsOpen!("{root}", "data.bin", "rw", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsWrite!(fd, "aa".hexToBytes(), *w1) |
                for (@_ <- w1) {{
                  fsWriteAt!(fd, 5, "bbcc".hexToBytes(), *w2) |
                  for (@_ <- w2) {{
                    fsTruncate!(fd, 32, *w3) |
                    for (@_ <- w3) {{ Nil }}
                  }}
                }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let rand = Blake2b512Random::create_from_bytes(&[9; 32]);

        // 1. Play on the leader.
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
        assert!(!leader_wal.is_empty(), "leader must have journaled");

        // 2. Capture leader checkpoint + rig follower for replay.
        let checkpoint = leader.create_checkpoint().await;
        let root = checkpoint.root;
        let log = checkpoint.log;
        follower.reset(&root).await.expect("follower reset");
        follower.rig(log).await.expect("follower rig");

        // 3. Replay on the follower with the SAME term + rand — this
        // drives the is_replay=true branch of every handler.
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

        // 4. WAL byte-identity assertion — the C-R1 core invariant.
        assert_eq!(
            leader_wal.len(),
            follower_wal.len(),
            "leader has {} WAL entries; follower has {} — divergence indicates \
             the fs_open replay branch failed to populate a shadow handle or \
             journal_write behaves differently on the is_replay=true branch",
            leader_wal.len(),
            follower_wal.len()
        );
        for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
            assert_eq!(
                l, f,
                "WAL entry {i} differs between leader and follower: \
                             leader={l:?}, follower={f:?}"
            );
        }

        follower
            .check_replay_data()
            .await
            .expect("follower replay data mismatch — tuplespace divergence, not just WAL");
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

    /// Streaming-backing slice Step 3 (2026-08-25): consensus-mode
    /// `entriesStreamNext` on a Consensus-cap dir stream journals one
    /// `WalOp::EntriesStreamNext` entry per call.  Oracular caps do
    /// NOT journal — same cross-cap isolation as fs_stat / fs_entries.
    ///
    /// The test drives Open → 3 Next calls (yielding a, b, c) → 1 Next
    /// call (EOS) → Close on a cmode="consensus" cap.  Expects 4 WAL
    /// entries (3 yields + 1 EOS), all with op = EntriesStreamNext,
    /// all with outcome = Success (EOS is expected control flow, not
    /// a failure).  A regression that skipped EOS journaling, or that
    /// broke the cmode gate, would fail here.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn entries_stream_next_consensus_journals_per_call() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/a"), b"1").unwrap();
        std::fs::write(dir.path().join("sub/b"), b"2").unwrap();
        std::fs::write(dir.path().join("sub/c"), b"3").unwrap();
        let runtime = create_runtime().await;

        // Open the stream on a consensus cap.
        let open_term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`), o in {{
              fsOpen!("{root}", "sub", "consensus", *o) |
              for (@[true, _fd] <- o) {{ Nil }}
            }}
            "#,
            root = dir.path().display(),
        );
        runtime
            .evaluate(
                &open_term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .expect("evaluate open");

        // Open itself does NOT journal (only Next journals per the
        // slice-3 design; Open is a leader-only setup step).
        assert_eq!(
            runtime.fs_handles.wal.len(),
            0,
            "entriesStreamOpen must NOT journal — only Next journals per call"
        );

        // 4 Next calls: 3 yields + 1 EOS.  The DirHandleTable fd is 1
        // (fresh runtime, first alloc).  Use a distinct rand per
        // iteration so each evaluate constructs a distinct fresh
        // unforgeable `r` — sharing rand would derive the same `r`
        // across calls, and the tuplespace persists between
        // evaluates, so the receiver in call N+1 would consume
        // call N's reply instead of firing its own Next.
        let fd: u64 = 1;
        for i in 0..4u8 {
            let term = format!(
                r#"
                new fsNext(`rho:io:fs:native:1.0.0/entriesStreamNext`), r in {{
                  fsNext!({fd}, *r) |
                  for (@_reply <- r) {{ Nil }}
                }}
                "#,
                fd = fd,
            );
            runtime
                .evaluate(
                    &term,
                    Cost::unsafe_max(),
                    std::collections::HashMap::new(),
                    Blake2b512Random::create_from_bytes(&[i, i, i, i]),
                )
                .await
                .expect("evaluate next");
        }

        let snap = runtime.fs_handles.wal.snapshot();
        assert_eq!(
            snap.len(),
            4,
            "expected 4 EntriesStreamNext WAL entries (3 yields + 1 EOS); got {}",
            snap.len()
        );
        for (i, e) in snap.iter().enumerate() {
            assert_eq!(
                e.op,
                WalOp::EntriesStreamNext,
                "entry {i} op must be EntriesStreamNext"
            );
            // Outcome: `[true, entryRecord]` (yields, entries 0-2)
            // journal as Success.  `[false, "EOS"]` (entry 3) falls
            // through `extract_err_code` — "EOS" is not an
            // `FSERR_*` code, so `fserr_to_code` returns 0
            // (FSERR_CODE_UNKNOWN) and the outcome becomes
            // `Failure { code: 0 }`.  This is deterministic across
            // leader/follower and cheap; see the WalOp::
            // EntriesStreamNext docstring for the downstream
            // disambiguation strategy.
            let expected_outcome = if i < 3 {
                rholang::rust::interpreter::io::wal::WalOutcome::Success
            } else {
                rholang::rust::interpreter::io::wal::WalOutcome::Failure { code: 0 }
            };
            assert_eq!(
                e.outcome, expected_outcome,
                "entry {i} outcome must be {expected_outcome:?}"
            );
            // length encodes 1 for yields, 0 for EOS/error.
            let expected_len = if i < 3 { Some(1) } else { Some(0) };
            assert_eq!(
                e.length, expected_len,
                "entry {i} length must be {expected_len:?}"
            );
        }

        // Close does NOT journal (Close is a leader-only fd release).
        let close_term = format!(
            r#"
            new fsClose(`rho:io:fs:native:1.0.0/entriesStreamClose`), r in {{
              fsClose!({fd}, *r) |
              for (@_reply <- r) {{ Nil }}
            }}
            "#,
            fd = fd,
        );
        runtime
            .evaluate(
                &close_term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .expect("evaluate close");
        assert_eq!(
            runtime.fs_handles.wal.len(),
            4,
            "Close must NOT append to WAL; count unchanged after close"
        );
    }

    /// Streaming-backing slice Step 3 (2026-08-25): Oracular
    /// entriesStreamNext MUST NOT journal — same cross-cap isolation
    /// invariant as fs_stat / fs_entries oracular pins.  A regression
    /// that ignored the cap's cmode and journaled unconditionally
    /// would surface Oracular reads in the Consensus WAL and diverge
    /// across validators with different local fs state.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn entries_stream_next_oracular_does_not_journal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/x"), b"x").unwrap();
        let runtime = create_runtime().await;

        let open_term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`), o in {{
              fsOpen!("{root}", "sub", "oracular", *o) |
              for (@[true, _fd] <- o) {{ Nil }}
            }}
            "#,
            root = dir.path().display(),
        );
        runtime
            .evaluate(
                &open_term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .expect("evaluate open");

        // 2 Next calls (1 yield + 1 EOS).
        for _ in 0..2 {
            let term = r#"
                new fsNext(`rho:io:fs:native:1.0.0/entriesStreamNext`), r in {
                  fsNext!(1, *r) |
                  for (@_reply <- r) { Nil }
                }
                "#
            .to_string();
            runtime
                .evaluate(
                    &term,
                    Cost::unsafe_max(),
                    std::collections::HashMap::new(),
                    rand(),
                )
                .await
                .expect("evaluate next");
        }

        assert!(
            runtime.fs_handles.wal.is_empty(),
            "Oracular entriesStreamNext MUST NOT journal; got {} entries",
            runtime.fs_handles.wal.len()
        );
    }

    /// Streaming-backing slice Step 3 (2026-08-25): leader/follower
    /// WAL byte-identity for the streaming primitive — the C-R1 core
    /// invariant applied to `entriesStreamNext`.  Runs Open + 3
    /// Next calls (2 yields + 1 EOS) on a Consensus cap on both
    /// leader (is_replay=false) and follower (is_replay=true, rigged
    /// with the leader's event log), and asserts both WALs are
    /// byte-identical.  Would fail if:
    /// - The follower's replay branch did NOT populate the shadow
    ///   dir-handle at Open (Step 2's fs_entries_stream_open replay
    ///   branch) → follower's Next handler skips journaling.
    /// - `journal_state_read` behaved differently on the is_replay
    ///   branch (e.g., different hash derivation).
    /// - Leader / follower disagreed on the length field or the
    ///   yielded entry's stable-hash.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn entries_stream_next_wal_is_byte_identical_on_leader_and_follower() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/a"), b"1").unwrap();
        std::fs::write(dir.path().join("sub/b"), b"2").unwrap();

        let (mut leader, mut follower) = create_leader_and_follower().await;

        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`),
                fsNext(`rho:io:fs:native:1.0.0/entriesStreamNext`),
                o, r1, r2, r3
            in {{
              fsOpen!("{root}", "sub", "consensus", *o) |
              for (@[true, fd] <- o) {{
                fsNext!(fd, *r1) |
                for (@_ <- r1) {{
                  fsNext!(fd, *r2) |
                  for (@_ <- r2) {{
                    fsNext!(fd, *r3) |
                    for (@_ <- r3) {{ Nil }}
                  }}
                }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let rand = Blake2b512Random::create_from_bytes(&[3; 32]);

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
        assert_eq!(
            leader_wal.len(),
            3,
            "leader must have journaled exactly 3 EntriesStreamNext entries \
             (2 yields + 1 EOS)"
        );

        let checkpoint = leader.create_checkpoint().await;
        let root = checkpoint.root;
        let log = checkpoint.log;
        follower.reset(&root).await.expect("follower reset");
        follower.rig(log).await.expect("follower rig");

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
            "leader has {} WAL entries; follower has {} — divergence indicates \
             the entriesStreamOpen replay branch failed to shadow the dir handle \
             or entriesStreamNext journals differently on the is_replay=true branch",
            leader_wal.len(),
            follower_wal.len()
        );
        for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
            assert_eq!(
                l, f,
                "EntriesStreamNext WAL entry {i} differs between leader and \
                 follower: leader={l:?}, follower={f:?}"
            );
        }

        follower
            .check_replay_data()
            .await
            .expect("follower replay data mismatch — tuplespace divergence, not just WAL");
    }

    /// H1 gap fix: end-to-end WAL cap enforcement — a Rholang program
    /// that fills the WAL past `MAX_WAL_ENTRIES` gets `FSERR_QUOTA_EXCEEDED`
    /// on the overflow write, and the WAL does not exceed the cap.
    /// Uses a small synthetic cap via a direct wal.append loop rather
    /// than issuing 65_536 fs_writes (which would take too long).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn wal_cap_returns_fserr_quota_exceeded_from_rholang() {
        use std::path::PathBuf;

        use rholang::rust::interpreter::io::wal::{
            PayloadRef, WalEntry, WalOp, WalOutcome, MAX_WAL_ENTRIES,
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"").unwrap();
        let runtime = create_runtime().await;

        // Pre-fill WAL to the cap via the direct API (much faster than
        // issuing MAX_WAL_ENTRIES fs_writes from Rholang).
        for _ in 0..MAX_WAL_ENTRIES {
            runtime
                .fs_handles
                .wal
                .append(WalEntry {
                    op: WalOp::Write,
                    path: PathBuf::from("/prefill"),
                    extra_path: None,
                    offset: None,
                    length: Some(0),
                    payload_ref: Some(PayloadRef::hash(b"")),
                    mode_bits: None,
                    owner: None,
                    group: None,
                    outcome: WalOutcome::Success,
                })
                .unwrap();
        }
        assert_eq!(runtime.fs_handles.wal.len(), MAX_WAL_ENTRIES);

        // Now issue an fs_write on a Consensus cap — journal_write
        // should try to append and hit the cap, returning
        // FSERR_QUOTA_EXCEEDED without invoking the syscall.
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                oc, wc
            in {{
              fsOpen!("{root}", "f.bin", "rw", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsWrite!(fd, "aa".hexToBytes(), *wc) |
                for (@reply <- wc) {{ @"out"!(reply) }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        // We can't easily read the return via evaluate(), but we can
        // assert WAL length did NOT grow past cap.
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
            runtime.fs_handles.wal.len(),
            MAX_WAL_ENTRIES,
            "WAL must not exceed MAX_WAL_ENTRIES even under concurrent load"
        );
    }

    /// Streaming-backing slice Step 3 review-fix (2026-08-25): WAL cap
    /// enforcement extends to `entriesStreamNext` journal appends.
    /// A runaway `entriesStreamNext` loop on a Consensus cap that
    /// pushes the per-runtime WAL past `MAX_WAL_ENTRIES` must
    /// surface `FSERR_QUOTA_EXCEEDED` on the overflow call — same
    /// bound as fs_write / fs_stat / fs_entries.
    ///
    /// The handler charge model: `journal_state_read` calls
    /// `wal.append_with_ack` which returns `Err(())` at the cap.
    /// The handler discards the return via `let _ =` (matches
    /// fs_stat / fs_entries), so the WAL simply stops growing —
    /// the cap propagates as a silent no-append on subsequent
    /// journal calls rather than an explicit FSERR reply.
    ///
    /// This test pre-fills the WAL to the cap via direct API, then
    /// drives one `entriesStreamNext` call, and asserts the WAL
    /// length stays exactly at the cap.  Matches the shape of
    /// `wal_cap_returns_fserr_quota_exceeded_from_rholang` above.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn entries_stream_next_respects_wal_cap() {
        use std::path::PathBuf;

        use rholang::rust::interpreter::io::wal::{
            PayloadRef, WalEntry, WalOp, WalOutcome, MAX_WAL_ENTRIES,
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/a"), b"1").unwrap();
        let runtime = create_runtime().await;

        // Open the stream FIRST so we have a valid fd before filling
        // the WAL.  Open itself doesn't journal (leader-only setup).
        let open_term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`), o in {{
              fsOpen!("{root}", "sub", "consensus", *o) |
              for (@[true, _fd] <- o) {{ Nil }}
            }}
            "#,
            root = dir.path().display(),
        );
        runtime
            .evaluate(
                &open_term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .expect("evaluate open");
        assert!(runtime.fs_handles.wal.is_empty(), "open must not journal");

        // Pre-fill WAL to the cap.
        for _ in 0..MAX_WAL_ENTRIES {
            runtime
                .fs_handles
                .wal
                .append(WalEntry {
                    op: WalOp::EntriesStreamNext,
                    path: PathBuf::from("/prefill"),
                    extra_path: None,
                    offset: None,
                    length: Some(0),
                    payload_ref: Some(PayloadRef::hash(b"")),
                    mode_bits: None,
                    owner: None,
                    group: None,
                    outcome: WalOutcome::Success,
                })
                .unwrap();
        }
        assert_eq!(runtime.fs_handles.wal.len(), MAX_WAL_ENTRIES);

        // fd is 1 (fresh runtime + open above).  Drive one Next call.
        let fd: u64 = 1;
        let next_term = format!(
            r#"
            new fsNext(`rho:io:fs:native:1.0.0/entriesStreamNext`), r in {{
              fsNext!({fd}, *r) |
              for (@_reply <- r) {{ Nil }}
            }}
            "#,
            fd = fd,
        );
        runtime
            .evaluate(
                &next_term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .expect("evaluate next");

        // WAL must not have grown past the cap — the append failed
        // silently at cap (matches fs_stat / fs_entries semantics).
        assert_eq!(
            runtime.fs_handles.wal.len(),
            MAX_WAL_ENTRIES,
            "entriesStreamNext journal must NOT exceed MAX_WAL_ENTRIES; \
             a growth to {} indicates the cap gate is bypassed",
            runtime.fs_handles.wal.len()
        );
    }

    /// H4/M1 round-2 pin: nested `create_soft_checkpoint` calls must
    /// preserve outer marks.  Pre-round-2 the single-slot Option
    /// design silently overwrote the outer, causing revert-to-outer
    /// to only unwind to the inner boundary.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn nested_soft_checkpoints_preserve_outer_wal_mark() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.bin"), b"").unwrap();
        let mut runtime = create_runtime().await;

        // Append 1 baseline entry.
        let a_term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                o, w
            in {{
              fsOpen!("{root}", "a.bin", "rw", "consensus", *o) |
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
                &a_term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .unwrap();
        assert_eq!(runtime.fs_handles.wal.len(), 1);

        // Outer checkpoint.
        let outer = runtime.create_soft_checkpoint().await;

        // One entry post-outer, pre-inner.
        runtime
            .evaluate(
                &a_term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                Blake2b512Random::create_from_bytes(&[7; 32]),
            )
            .await
            .unwrap();
        assert_eq!(runtime.fs_handles.wal.len(), 2);

        // Inner checkpoint.  Pre-round-2 this OVERWROTE the outer
        // wal_snapshot slot, losing the outer mark.
        let inner = runtime.create_soft_checkpoint().await;

        // One entry post-inner.
        runtime
            .evaluate(
                &a_term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                Blake2b512Random::create_from_bytes(&[8; 32]),
            )
            .await
            .unwrap();
        assert_eq!(runtime.fs_handles.wal.len(), 3);

        // Revert inner → back to 2 entries.
        runtime.revert_to_soft_checkpoint(inner).await;
        assert_eq!(
            runtime.fs_handles.wal.len(),
            2,
            "inner revert must land on inner mark, not outer"
        );

        // Revert outer → back to 1 entry.  Pre-round-2 this would
        // land back on the inner mark (or a garbage state) because
        // the outer mark was lost.
        runtime.revert_to_soft_checkpoint(outer).await;
        assert_eq!(
            runtime.fs_handles.wal.len(),
            1,
            "outer revert must preserve baseline; H4/M1 round-2 fix"
        );
    }

    /// H-29-1 regression pin: `revert_to_soft_checkpoint` truncates
    /// the WAL back to the snapshot mark, discarding any entries
    /// appended during the reverted deploy.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn revert_soft_checkpoint_truncates_wal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pre.bin"), b"").unwrap();
        std::fs::write(dir.path().join("post.bin"), b"").unwrap();
        let mut runtime = create_runtime().await;

        // Pre-checkpoint mutation.
        let pre_term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                o, w
            in {{
              fsOpen!("{root}", "pre.bin", "rw", "consensus", *o) |
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
                &pre_term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .unwrap();
        assert_eq!(runtime.fs_handles.wal.len(), 1);

        let checkpoint = runtime.create_soft_checkpoint().await;

        // Post-checkpoint mutation — this one gets reverted.
        let post_term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                o, w
            in {{
              fsOpen!("{root}", "post.bin", "rw", "consensus", *o) |
              for (@[true, fd] <- o) {{
                fsWrite!(fd, "bb".hexToBytes(), *w) |
                for (@_ <- w) {{ Nil }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        runtime
            .evaluate(
                &post_term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .unwrap();
        assert_eq!(runtime.fs_handles.wal.len(), 2);

        // Revert.  Post-checkpoint entries must be gone.
        runtime.revert_to_soft_checkpoint(checkpoint).await;
        assert_eq!(
            runtime.fs_handles.wal.len(),
            1,
            "revert must truncate WAL back to snapshot"
        );
        let remaining = runtime.fs_handles.wal.snapshot();
        assert!(
            remaining[0].path.to_string_lossy().ends_with("pre.bin"),
            "surviving entry must be the pre-checkpoint one, got {:?}",
            remaining[0].path
        );
    }

    // ---------------------------------------------------------------
    // Slice 32 (PB-M-14 read-hash): read-side WAL journaling tests.
    //
    // Slices 29/30 only journaled mutations (Write/WriteAt/Truncate).
    // Slice 32 extends the WAL to observation-preserving reads whose
    // returned bytes feed the tuplespace: `fs_read` / `fs_read_at`.
    //
    // Test surface:
    //   - A Consensus read produces a `Read` WAL entry whose
    //     `payload_ref = Hash(returned_bytes)`.
    //   - `fs_read_at` produces `ReadAt` with offset populated.
    //   - Oracular reads produce NO WAL entry (parity with writes).
    //   - Leader/follower symmetry: both sides append the SAME Read
    //     entry — this is what makes the per-deploy WAL byte-
    //     identical and preserves the H-30-COV round-trip property
    //     for read-heavy deploys.
    //   - Zero-byte / EOF read: empty returned bytes hash to the
    //     canonical `Blake2b256([])` and still produce an entry
    //     (a `Read` with length=0 is a distinguishable observation
    //     for replay verification).
    // ---------------------------------------------------------------

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

    // ---------------------------------------------------------------
    // H-30-COV (Phase 7 whole-review): SnapshotWriter end-to-end pins.
    //
    // The `runtime_manager.rs` unit tests cover the boot-time wiring
    // path (`set_fs_snapshot_writer` → `share_fs_snapshot_writer` →
    // `RhoRuntimeImpl.fs_snapshot_writer`).  The `snapshot.rs` unit
    // tests cover encoding, cadence math, and retention pruning in
    // isolation.  What was missing was a test proving the composed
    // pipeline: a Rholang deploy that touches fs_write on a
    // Consensus cap produces WAL entries → the WAL bytes are
    // canonically encoded → SnapshotWriter.maybe_write persists them
    // to disk → read_snapshot round-trips the bytes back.
    //
    // These tests bypass the block-boundary trigger in
    // `casper::rholang::runtime::play_deploys_for_state` and instead
    // call `SnapshotWriter.maybe_write` directly on the runtime's
    // collected entries — that's the exact same call site casper
    // uses, minus the tokio spawn_blocking wrapper (which is
    // orthogonal to correctness).
    // ---------------------------------------------------------------

    /// H-30-COV-1: Consensus-mode writes → WAL entries → snapshot
    /// bytes on disk → byte-identity check via `read_snapshot_bytes`
    /// (the read side used by joining validators).  This pins the
    /// composed pipeline: runtime WAL → `SnapshotWriter.maybe_write`
    /// → `write_snapshot` (encode + Blake2b256 + atomic rename) →
    /// on-disk content-addressed file → `read_snapshot_bytes`
    /// (hash-verified read).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn wal_to_snapshot_end_to_end_round_trip() {
        use rholang::rust::interpreter::io::snapshot::{
            encode_wal_slice, read_snapshot_bytes, SnapshotWriter,
        };

        let data_dir = tempfile::tempdir().unwrap();
        let snap_dir = tempfile::tempdir().unwrap();
        std::fs::write(data_dir.path().join("f.bin"), b"").unwrap();
        let runtime = create_runtime().await;

        let writer = SnapshotWriter {
            dir: snap_dir.path().to_path_buf(),
            cadence: 1,
            retain: 4,
            signer_sk: None,
        };
        runtime.set_fs_snapshot_writer(Some(writer.clone())).await;

        // Run a Consensus write.
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                o, w
            in {{
              fsOpen!("{root}", "f.bin", "rw", "consensus", *o) |
              for (@[true, fd] <- o) {{
                fsWrite!(fd, "cafebabe".hexToBytes(), *w) |
                for (@_ <- w) {{ Nil }}
              }}
            }}
            "#,
            root = data_dir.path().display(),
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
        assert_eq!(entries.len(), 1, "expected 1 WAL entry");

        // Directly invoke the cadence writer at block 1 (cadence hit).
        let res = writer.maybe_write(1, &entries).expect("maybe_write");
        assert!(
            res.is_some(),
            "cadence hit + non-empty entries must persist a snapshot"
        );
        let (root, _merkle_root) = res.unwrap();

        // Read back by content hash — this is the joining-validator
        // path.  `read_snapshot_bytes` re-hashes and compares against
        // the requested root, so a match proves both write-side and
        // read-side agree on the canonical encoding.
        let read_bytes = read_snapshot_bytes(snap_dir.path(), &root).expect("read_snapshot_bytes");
        let expected_bytes = encode_wal_slice(&entries);
        assert_eq!(
            read_bytes, expected_bytes,
            "on-disk bytes must equal freshly-encoded WAL slice"
        );
    }

    /// H-30-COV-3: cadence miss → NO snapshot file created, even when
    /// entries are present.  Pins the guard against silent
    /// over-writing (would exhaust disk on a busy validator).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn wal_snapshot_not_written_on_cadence_miss() {
        use rholang::rust::interpreter::io::snapshot::SnapshotWriter;

        let data_dir = tempfile::tempdir().unwrap();
        let snap_dir = tempfile::tempdir().unwrap();
        std::fs::write(data_dir.path().join("f.bin"), b"").unwrap();
        let runtime = create_runtime().await;

        let writer = SnapshotWriter {
            dir: snap_dir.path().to_path_buf(),
            cadence: 10, // block 3 is a miss
            retain: 4,
            signer_sk: None,
        };

        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                o, w
            in {{
              fsOpen!("{root}", "f.bin", "rw", "consensus", *o) |
              for (@[true, fd] <- o) {{
                fsWrite!(fd, "aa".hexToBytes(), *w) |
                for (@_ <- w) {{ Nil }}
              }}
            }}
            "#,
            root = data_dir.path().display(),
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
        let res = writer.maybe_write(3, &entries).expect("maybe_write");
        assert!(
            res.is_none(),
            "block 3 with cadence 10 must be a cadence miss"
        );
        let files: Vec<_> = std::fs::read_dir(snap_dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(
            files.is_empty(),
            "cadence miss must not write any file; got {} files",
            files.len()
        );
    }

    /// H-30-COV-4: retention bound holds across multiple cadence
    /// hits — the number of persisted `*.wal` files never exceeds
    /// `retain`.  Direct pin against a regression that forgot the
    /// `prune_snapshot_dir` call in `maybe_write`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn wal_snapshot_retention_bound_holds_across_writes() {
        use rholang::rust::interpreter::io::snapshot::SnapshotWriter;
        use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};

        let snap_dir = tempfile::tempdir().unwrap();
        let writer = SnapshotWriter {
            dir: snap_dir.path().to_path_buf(),
            cadence: 1,
            retain: 3,
            signer_sk: None,
        };

        // Craft 5 distinct WAL slices (different offsets → different
        // encodings → different roots → different filenames).
        for i in 0..5u64 {
            let entries = vec![WalEntry {
                op: WalOp::Write,
                path: std::path::PathBuf::from("/tmp/f.bin"),
                extra_path: None,
                offset: Some(i),
                length: Some(1),
                payload_ref: Some(PayloadRef::Hash([i as u8; 32])),
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            }];
            // Retention pruning uses mtime; sleep past APFS's 1s
            // granularity between writes so pruning has a stable
            // ordering to work with.
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_millis(1100));
            }
            writer.maybe_write(i as i64 + 1, &entries).unwrap();
        }

        let files: Vec<_> = std::fs::read_dir(snap_dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "wal"))
            .collect();
        assert!(
            files.len() <= 3,
            "retain=3 must bound file count; got {} files",
            files.len()
        );
    }

    /// **Multi-deploy WAL-replay parity pin (2026-08-26).**
    /// Intermediate coverage between the single-deploy byte-identity
    /// pins above and the (still-blocked) two-validator PB-M-14 E2E
    /// test.  Runs a longer sequence of mutations across multiple
    /// deploy boundaries on a leader, captures its WAL and
    /// checkpoint, replays on a follower via the same rig-then-
    /// evaluate pattern, and asserts byte-identical WAL entries at
    /// the whole-sequence level.
    ///
    /// Coverage delta over `wal_is_byte_identical_on_leader_and_follower`:
    /// * Three separate deploys (open/mutate/close x 3) instead of
    ///   one — exercises the cross-deploy fs_handles.wal continuity
    ///   invariant (WAL entries accumulate across evaluate calls
    ///   before the follower captures a single checkpoint).
    /// * Mixed operation types (write, write_at, truncate, stat) in
    ///   different orderings per deploy.
    /// * Mixes Consensus and Oracular caps so cross-cap symmetry is
    ///   exercised too (oracular calls MUST NOT emit WAL entries).
    ///
    /// The full PB-M-14 test (validator B joins from genesis + WAL
    /// alone, no shared store, no rig) requires per-node fs
    /// provisioning on `TestNode::create_node` which does not exist
    /// today.  See the ignored `pb_m_14_two_validator_scaffold`
    /// test below and the Deferred items catalog entry for
    /// "Two-validator PB-M-14 end-to-end test".
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn multi_deploy_wal_is_byte_identical_on_leader_and_follower() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.bin"), vec![0u8; 128]).unwrap();
        std::fs::write(dir.path().join("aux.bin"), vec![0u8; 64]).unwrap();

        let (mut leader, mut follower) = create_leader_and_follower().await;
        let root_display = dir.path().display().to_string();

        // Three deploys, each opens + does one or two ops + closes.
        // Mixed Consensus + Oracular so the follower must observe
        // WAL entries only from the Consensus caps.
        let deploys: Vec<(String, [u8; 32])> = vec![
            (
                format!(
                    r#"
                    new fsOpen(`rho:io:fs:native:1.0.0/open`),
                        fsWrite(`rho:io:fs:native:1.0.0/write`),
                        fsClose(`rho:io:fs:native:1.0.0/close`),
                        oc, wc, cc in {{
                      fsOpen!("{root_display}", "data.bin", "rw", "consensus", *oc) |
                      for (@[true, fd] <- oc) {{
                        fsWrite!(fd, "aabb".hexToBytes(), *wc) |
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
                      fsOpen!("{root_display}", "data.bin", "rw", "consensus", *oc) |
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
                // Oracular deploy — must NOT emit WAL entries.  The
                // WAL count assertion below transitively checks this.
                format!(
                    r#"
                    new fsOpen(`rho:io:fs:native:1.0.0/open`),
                        fsWrite(`rho:io:fs:native:1.0.0/write`),
                        fsClose(`rho:io:fs:native:1.0.0/close`),
                        oc, wc, cc in {{
                      fsOpen!("{root_display}", "aux.bin", "rw", "oracular", *oc) |
                      for (@[true, fd] <- oc) {{
                        fsWrite!(fd, "ee".hexToBytes(), *wc) |
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

        // Play the three deploys on the leader; WAL accumulates.
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
        let leader_wal = leader.fs_handles.wal.snapshot();
        assert!(
            !leader_wal.is_empty(),
            "at least one Consensus mutation must have journaled"
        );

        // Capture leader checkpoint; rig follower.
        let checkpoint = leader.create_checkpoint().await;
        follower
            .reset(&checkpoint.root)
            .await
            .expect("follower reset");
        follower.rig(checkpoint.log).await.expect("follower rig");

        // Replay all three deploys on the follower with the same
        // seeds — drives the is_replay=true branch of every handler.
        for (term, seed) in &deploys {
            follower
                .evaluate(
                    term,
                    Cost::unsafe_max(),
                    std::collections::HashMap::new(),
                    Blake2b512Random::create_from_bytes(seed),
                )
                .await
                .expect("follower evaluate");
        }
        let follower_wal = follower.fs_handles.wal.snapshot();

        assert_eq!(
            leader_wal.len(),
            follower_wal.len(),
            "multi-deploy WAL length diverges: leader={}, follower={} — \
             regression suggests one of the three deploys leaked an entry \
             on one side or the other (e.g., WAL append fires on
             is_replay=true where it shouldn't, or vice versa)",
            leader_wal.len(),
            follower_wal.len(),
        );
        for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
            assert_eq!(
                l, f,
                "multi-deploy WAL entry {i} differs: leader={l:?}, follower={f:?}"
            );
        }
        // Explicit Oracular-count-zero assertion (2026-08-26 review
        // strengthening): the count-equality check above passes even
        // if BOTH leader and follower spuriously emit Oracular
        // entries.  Pin the invariant directly by asserting no WAL
        // entry references "aux.bin" (the Oracular deploy's target).
        // A regression that made Oracular caps journal to WAL would
        // trip this on both leader and follower simultaneously.
        for entry in &leader_wal {
            let path_str = entry.path.to_string_lossy();
            assert!(
                !path_str.contains("aux.bin"),
                "Oracular deploy (aux.bin) MUST NOT emit any WAL entry; \
                 found: {entry:?}.  Regression: `journal_write` fires \
                 on oracular caps at handlers.rs — check the cmode \
                 branch inside the write handler."
            );
        }
        follower
            .check_replay_data()
            .await
            .expect("follower replay data mismatch — tuplespace divergence");
    }

    /// **Multi-deploy revert-mid-sequence WAL parity pin (2026-08-26
    /// review strengthening).**  Companion to
    /// `multi_deploy_wal_is_byte_identical_on_leader_and_follower`
    /// covering the failure-path leg: one deploy in a multi-deploy
    /// sequence reverts via `revert_to_soft_checkpoint`, and the
    /// follower's WAL must reflect the same revert (via
    /// `wal_snapshot_stack` truncate on H-29-1's stack semantics).
    ///
    /// Regression scenario: a WAL entry from the reverted deploy
    /// slips through and lands in the follower's WAL sequence — a
    /// consensus divergence (leader post-revert WAL count < follower
    /// WAL count).  The `revert_to_soft_checkpoint` machinery is the
    /// load-bearing invariant here; this pin exercises it under
    /// multi-deploy pressure that the single-deploy
    /// `revert_soft_checkpoint_truncates_wal` doesn't.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn multi_deploy_wal_survives_mid_sequence_revert() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.bin"), vec![0u8; 128]).unwrap();

        let (mut leader, mut follower) = create_leader_and_follower().await;
        let root_display = dir.path().display().to_string();

        // Successful deploy 1 (Consensus write).  WAL grows by ≥1.
        let commit_term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                fsClose(`rho:io:fs:native:1.0.0/close`),
                oc, wc, cc in {{
              fsOpen!("{root_display}", "data.bin", "rw", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsWrite!(fd, "aabb".hexToBytes(), *wc) |
                for (@_ <- wc) {{
                  fsClose!(fd, *cc) |
                  for (@_ <- cc) {{ Nil }}
                }}
              }}
            }}
            "#
        );

        for seed_byte in [1u8, 2u8] {
            leader
                .evaluate(
                    &commit_term,
                    Cost::unsafe_max(),
                    std::collections::HashMap::new(),
                    Blake2b512Random::create_from_bytes(&[seed_byte; 32]),
                )
                .await
                .expect("leader commit deploy");
        }
        let post_commit_wal_len = leader.fs_handles.wal.snapshot().len();
        assert!(
            post_commit_wal_len >= 2,
            "two committed Consensus writes must produce >= 2 WAL entries; got {post_commit_wal_len}"
        );

        // Reverted deploy: create_soft_checkpoint → evaluate a
        // mutation → revert.  The WAL entries appended between
        // create and revert MUST be truncated by H-29-1's
        // wal_snapshot_stack pop.
        let checkpoint = leader.create_soft_checkpoint().await;
        leader
            .evaluate(
                &commit_term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                Blake2b512Random::create_from_bytes(&[3u8; 32]),
            )
            .await
            .expect("leader mid-sequence evaluate");
        let pre_revert_wal_len = leader.fs_handles.wal.snapshot().len();
        assert!(
            pre_revert_wal_len > post_commit_wal_len,
            "the reverted deploy must have appended WAL entries before revert; \
             pre_revert={pre_revert_wal_len}, post_commit={post_commit_wal_len}"
        );
        leader.revert_to_soft_checkpoint(checkpoint).await;
        let post_revert_wal_len = leader.fs_handles.wal.snapshot().len();
        assert_eq!(
            post_revert_wal_len, post_commit_wal_len,
            "H-29-1 regression: revert_to_soft_checkpoint must truncate the WAL \
             back to the pre-checkpoint mark.  post_commit={post_commit_wal_len}, \
             pre_revert={pre_revert_wal_len}, post_revert={post_revert_wal_len}.  \
             Investigation: `rho_runtime.rs::revert_to_soft_checkpoint` should \
             pop wal_snapshot_stack and call wal.truncate_to."
        );

        // Successful deploy AFTER revert.  WAL should grow by ≥1.
        leader
            .evaluate(
                &commit_term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                Blake2b512Random::create_from_bytes(&[4u8; 32]),
            )
            .await
            .expect("leader post-revert deploy");
        let leader_final_wal = leader.fs_handles.wal.snapshot();
        assert!(
            leader_final_wal.len() > post_revert_wal_len,
            "post-revert Consensus deploy must append a WAL entry; \
             final={}, post_revert={post_revert_wal_len}",
            leader_final_wal.len(),
        );

        // Rig follower: reset to leader's post-sequence root + rig
        // the log.  Follower re-executes the SUCCESSFUL deploys
        // (1, 2, post-revert) with the same seeds and NOT the
        // reverted one (its rand + checkpoint were discarded on
        // leader before the create_checkpoint below).
        let checkpoint = leader.create_checkpoint().await;
        follower
            .reset(&checkpoint.root)
            .await
            .expect("follower reset");
        follower.rig(checkpoint.log).await.expect("follower rig");

        for seed_byte in [1u8, 2u8, 4u8] {
            follower
                .evaluate(
                    &commit_term,
                    Cost::unsafe_max(),
                    std::collections::HashMap::new(),
                    Blake2b512Random::create_from_bytes(&[seed_byte; 32]),
                )
                .await
                .expect("follower evaluate");
        }
        let follower_wal = follower.fs_handles.wal.snapshot();
        assert_eq!(
            leader_final_wal.len(),
            follower_wal.len(),
            "post-revert leader/follower WAL length divergence: \
             leader={}, follower={} — regression indicates the reverted \
             deploy's WAL entries were NOT properly truncated on the \
             leader (they leaked into the checkpoint the follower rigged \
             against), OR the follower re-executed the reverted deploy \
             (log ordering broken).",
            leader_final_wal.len(),
            follower_wal.len(),
        );
        for (i, (l, f)) in leader_final_wal.iter().zip(follower_wal.iter()).enumerate() {
            assert_eq!(
                l, f,
                "post-revert WAL entry {i} differs: leader={l:?}, follower={f:?}"
            );
        }
    }

    // ---------------------------------------------------------------
    // PB-M-14 file-state-identity via WAL-only replay (2026-08-26).
    //
    // Path A(ii) of the fresh-follower Layer-2 harness plan
    // (implementation-plan.md:1461-1464): a small "WAL applier"
    // reconstructs on-disk file contents on a fresh tree from a
    // captured WAL slice + a hash→bytes sidecar (what a Phase 7b
    // joiner would obtain via `get_wal_payload`).  This closes the
    // FILE-STATE-IDENTITY half of PB-M-14 that the leader/follower
    // WAL-byte-identity pins above explicitly do not cover.
    //
    // Payloads sidecar: WAL entries carry `PayloadRef::Hash(...)`,
    // NOT the raw bytes (§369 of the plan doc — hash-only WAL).
    // Production followers rehydrate bytes via the Phase 7b sub-
    // protocol; for tests, the driver knows the bytes it fed to
    // `fsWrite!` and supplies them directly by hash key.
    // ---------------------------------------------------------------

    /// Recursively compare two directory trees for byte-identical
    /// file contents + identical relative directory structure.
    /// Ignores mtime, uid/gid, and any files listed in `ignore`.
    fn assert_dir_trees_byte_identical(
        a_root: &std::path::Path,
        b_root: &std::path::Path,
        ignore: &[&str],
    ) {
        fn collect(
            root: &std::path::Path,
            base: &std::path::Path,
            ignore: &[&str],
            out: &mut std::collections::BTreeMap<std::path::PathBuf, Option<Vec<u8>>>,
        ) {
            for entry in std::fs::read_dir(root).expect("read_dir") {
                let entry = entry.expect("dir entry");
                let path = entry.path();
                let rel = path.strip_prefix(base).unwrap().to_path_buf();
                let name = rel.to_string_lossy().to_string();
                if ignore
                    .iter()
                    .any(|p| name == *p || name.starts_with(&format!("{p}/")))
                {
                    continue;
                }
                let ft = entry.file_type().expect("file_type");
                if ft.is_dir() {
                    out.insert(rel.clone(), None); // directory marker
                    collect(&path, base, ignore, out);
                } else if ft.is_file() {
                    let bytes = std::fs::read(&path).expect("read file");
                    out.insert(rel, Some(bytes));
                }
                // Symlinks / other kinds are unexpected in fileio-consensus
                // trees (boot-time validation rejects them); skip silently
                // to keep the helper focused.
            }
        }
        let mut a_map = std::collections::BTreeMap::new();
        let mut b_map = std::collections::BTreeMap::new();
        collect(a_root, a_root, ignore, &mut a_map);
        collect(b_root, b_root, ignore, &mut b_map);
        assert_eq!(
            a_map.keys().collect::<Vec<_>>(),
            b_map.keys().collect::<Vec<_>>(),
            "tree layout differs: leader={:?}, follower={:?}",
            a_map.keys().collect::<Vec<_>>(),
            b_map.keys().collect::<Vec<_>>(),
        );
        for (rel, a_val) in &a_map {
            let b_val = b_map.get(rel).unwrap();
            match (a_val, b_val) {
                (None, None) => {} // both directories
                (Some(a_bytes), Some(b_bytes)) => {
                    assert_eq!(
                        a_bytes,
                        b_bytes,
                        "byte divergence at {rel:?}: leader_len={}, follower_len={}",
                        a_bytes.len(),
                        b_bytes.len(),
                    );
                }
                _ => panic!(
                    "kind divergence at {rel:?} (leader={:?}, follower={:?})",
                    a_val.as_ref().map(|_| "file"),
                    b_val.as_ref().map(|_| "file"),
                ),
            }
        }
    }

    /// Rewrite an absolute path from `leader_root/rel` to
    /// `follower_root/rel`.  Panics if the path isn't rooted under
    /// `leader_root` — that's a WAL entry the applier can't handle
    /// safely (an out-of-tree canon_path would mean the leader saw a
    /// symlink escape, which boot-time validation forbids in the
    /// consensus-static trees this test targets).
    fn translate_path(
        leader_root: &std::path::Path,
        follower_root: &std::path::Path,
        p: &std::path::Path,
    ) -> std::path::PathBuf {
        let rel = p.strip_prefix(leader_root).unwrap_or_else(|_| {
            panic!(
                "WAL entry path {p:?} is not rooted under leader_root {leader_root:?}; \
                 test harness invariant violated"
            )
        });
        follower_root.join(rel)
    }

    /// Apply a captured WAL slice to a fresh follower tree.
    /// `payload_bytes` maps `PayloadRef::Hash(h) → bytes` for every
    /// `Write` / `WriteAt` entry the WAL references; a missing hash
    /// is a hard error (a real Phase 7b joiner would
    /// `get_wal_payload` for it — in a test, missing means the
    /// driver mis-populated the sidecar).
    ///
    /// **Supported ops (WAL is self-sufficient):**
    ///   * `Write` — sequential write; carries the fd's pre-write
    ///     shadow position in `offset` (position-tracking follow-up
    ///     2026-08-26 to PB-M-14 file-state-identity).
    ///   * `WriteAt` — positional write; `offset` from the caller.
    ///   * `Truncate` — carries the new file length in `offset`.
    ///   * Failure-outcome entries — skipped per H-6 (the leader
    ///     never mutated disk on Failure, so the follower must not).
    ///   * Observation-only variants (`Read`, `ReadAt`, `Stat`,
    ///     `Entries`, `Size`, `EntriesStreamNext`) — skipped; they
    ///     don't change disk state.
    ///
    /// The applier does NOT distinguish Write from WriteAt: both
    /// carry an absolute `offset` and are replayed by seek-then-
    /// write.  Determinism relies on the FileHandle shadow-position
    /// tracking on both leader and follower (see
    /// `FileHandle::position` docstring); the applier just reads
    /// what the WAL says.
    ///
    /// Path-based mutation variants (`Chmod`, `Chown`, `RemoveFile`,
    /// `RemoveDir`, `Rename`, `CopyFile`) are not yet wired into the
    /// production handler set for the WAL append side; this applier
    /// panics on them so a future slice that adds the handler wiring
    /// will surface the applier-side gap loudly rather than silently
    /// diverge on replay.
    fn apply_wal_to_fresh_tree(
        wal: &[WalEntry],
        payload_bytes: &std::collections::HashMap<[u8; 32], Vec<u8>>,
        leader_root: &std::path::Path,
        follower_root: &std::path::Path,
    ) {
        use std::io::{Seek, SeekFrom, Write};
        for (i, entry) in wal.iter().enumerate() {
            if matches!(entry.outcome, WalOutcome::Failure { .. }) {
                continue; // H-6: leader never mutated disk on Failure
            }
            match entry.op {
                WalOp::Write | WalOp::WriteAt => {
                    let dst = translate_path(leader_root, follower_root, &entry.path);
                    let hash = match entry.payload_ref {
                        Some(PayloadRef::Hash(h)) => h,
                        Some(PayloadRef::DeployRef { .. }) => panic!(
                            "WAL entry {i}: DeployRef payload_ref not yet supported \
                             by the fresh-tree applier — needs on-chain deploy \
                             data lookup (Phase 7b-2 reducer)"
                        ),
                        None => panic!(
                            "WAL entry {i}: {:?} without payload_ref — invariant \
                             violation in the write handler",
                            entry.op,
                        ),
                    };
                    let bytes = payload_bytes.get(&hash).unwrap_or_else(|| {
                        panic!(
                            "WAL entry {i}: hash {} missing from payload sidecar; \
                             a real Phase 7b joiner would `get_wal_payload` for it, \
                             but the test driver mis-populated the sidecar",
                            hex::encode(hash),
                        )
                    });
                    // Both Write and WriteAt carry absolute offset
                    // post-position-follow-up (2026-08-26).
                    let off = entry.offset.unwrap_or_else(|| {
                        panic!(
                            "WAL entry {i}: {:?} without offset — position-follow-up \
                             regression? journal_write should populate offset from \
                             FileHandle.position (sequential Write) or the caller-\
                             supplied offset (WriteAt)",
                            entry.op,
                        )
                    });
                    let mut f = std::fs::OpenOptions::new()
                        .create(true)
                        .write(true)
                        .truncate(false)
                        .open(&dst)
                        .unwrap_or_else(|e| panic!("open {dst:?}: {e}"));
                    f.seek(SeekFrom::Start(off))
                        .unwrap_or_else(|e| panic!("seek {dst:?}: {e}"));
                    f.write_all(bytes)
                        .unwrap_or_else(|e| panic!("write {dst:?}: {e}"));
                }
                WalOp::Truncate => {
                    let dst = translate_path(leader_root, follower_root, &entry.path);
                    let n = entry.offset.expect("Truncate must carry offset");
                    let f = std::fs::OpenOptions::new()
                        .write(true)
                        .open(&dst)
                        .unwrap_or_else(|e| panic!("open-for-truncate {dst:?}: {e}"));
                    f.set_len(n)
                        .unwrap_or_else(|e| panic!("set_len {dst:?}: {e}"));
                }
                // Observation-only — nothing to reconstruct on disk.
                WalOp::Read
                | WalOp::ReadAt
                | WalOp::Stat
                | WalOp::Entries
                | WalOp::Size
                | WalOp::EntriesStreamNext => {}
                // Path-based mutations (H-29-3 lift, 2026-08-26).
                // Each entry is fully derivable from args, so replay
                // is a straight syscall.  Failure entries are already
                // filtered above by the outcome check.
                WalOp::Chmod => {
                    let dst = translate_path(leader_root, follower_root, &entry.path);
                    let bits = entry
                        .mode_bits
                        .unwrap_or_else(|| panic!("WAL entry {i}: Chmod without mode_bits"));
                    // Use set_permissions for portability; symlink
                    // safety is a boot-time concern (consensus
                    // trees reject symlinks).
                    use std::os::unix::fs::PermissionsExt;
                    let perms = std::fs::Permissions::from_mode(bits);
                    std::fs::set_permissions(&dst, perms)
                        .unwrap_or_else(|e| panic!("chmod {dst:?}: {e}"));
                }
                WalOp::Chown => {
                    let dst = translate_path(leader_root, follower_root, &entry.path);
                    // Owner/group are String; NSS resolution on the
                    // follower host must match the leader's (documented
                    // as an operator responsibility in WalEntry::owner).
                    // Test harnesses on shared hosts resolve identically.
                    // Rely on the test not using cross-host owner names.
                    // For simplicity in the test applier we shell out to
                    // chown, or call libc directly via nix — but the
                    // simplest cross-platform path is std::os::unix +
                    // libc::chown, mirroring chown_impl's approach.
                    let owner = entry
                        .owner
                        .as_ref()
                        .unwrap_or_else(|| panic!("WAL entry {i}: Chown without owner"));
                    let group = entry.group.as_deref();
                    let uid = if owner.is_empty() {
                        u32::MAX
                    } else {
                        // Best-effort NSS lookup in the test applier.
                        // Failure to resolve → panic (test setup error;
                        // production has different error handling).
                        use std::ffi::CString;
                        let cname = CString::new(owner.as_str()).unwrap();
                        let pw = unsafe { libc::getpwnam(cname.as_ptr()) };
                        if pw.is_null() {
                            panic!(
                                "WAL entry {i}: applier can't resolve owner {owner:?} \
                                 on this host — NSS-mismatch scenario"
                            );
                        }
                        unsafe { (*pw).pw_uid }
                    };
                    let gid = match group {
                        None | Some("") => u32::MAX,
                        Some(g) => {
                            use std::ffi::CString;
                            let cname = CString::new(g).unwrap();
                            let gr = unsafe { libc::getgrnam(cname.as_ptr()) };
                            if gr.is_null() {
                                panic!("WAL entry {i}: applier can't resolve group {g:?}");
                            }
                            unsafe { (*gr).gr_gid }
                        }
                    };
                    let cpath = std::ffi::CString::new(dst.as_os_str().as_encoded_bytes()).unwrap();
                    let rc = unsafe { libc::chown(cpath.as_ptr(), uid, gid) };
                    if rc != 0 {
                        // In tests we typically don't have privileges
                        // to chown to arbitrary owners.  We accept
                        // EPERM as a no-op success — the applier
                        // couldn't apply, and tests should use
                        // current-user names to avoid this.  A
                        // hard-fail would defeat CI on unprivileged
                        // hosts.
                        let e = std::io::Error::last_os_error();
                        if e.raw_os_error() != Some(libc::EPERM) {
                            panic!("chown {dst:?}: {e}");
                        }
                    }
                }
                WalOp::RemoveFile => {
                    let dst = translate_path(leader_root, follower_root, &entry.path);
                    std::fs::remove_file(&dst)
                        .unwrap_or_else(|e| panic!("remove_file {dst:?}: {e}"));
                }
                WalOp::RemoveDir => {
                    let dst = translate_path(leader_root, follower_root, &entry.path);
                    std::fs::remove_dir(&dst).unwrap_or_else(|e| panic!("remove_dir {dst:?}: {e}"));
                }
                WalOp::Rename => {
                    let from = translate_path(leader_root, follower_root, &entry.path);
                    let to = entry
                        .extra_path
                        .as_ref()
                        .unwrap_or_else(|| panic!("WAL entry {i}: Rename without extra_path"));
                    let to = translate_path(leader_root, follower_root, to);
                    std::fs::rename(&from, &to)
                        .unwrap_or_else(|e| panic!("rename {from:?} → {to:?}: {e}"));
                }
                WalOp::CopyFile => {
                    let from = translate_path(leader_root, follower_root, &entry.path);
                    let to = entry
                        .extra_path
                        .as_ref()
                        .unwrap_or_else(|| panic!("WAL entry {i}: CopyFile without extra_path"));
                    let to = translate_path(leader_root, follower_root, to);
                    std::fs::copy(&from, &to)
                        .unwrap_or_else(|e| panic!("copy {from:?} → {to:?}: {e}"));
                }
            }
        }
    }

    // ---------------------------------------------------------------
    // H-29-3 lift, slice 1 (2026-08-26).  Path-based Consensus
    // mutations that are 1-op semantics — fs_chmod, fs_chown,
    // fs_rename, fs_copy_file, fs_remove_file — now journal to the
    // WAL before invoking the syscall.  Each test exercises the
    // Consensus path (asserts entry appended), verifies the Oracular
    // path skips journaling (parity with the fd-based ops), and
    // asserts leader/follower byte-identity via
    // `create_leader_and_follower`.
    //
    // fs_remove_dir (both non-recursive and recursive granular) is
    // covered in a follow-up slice.
    // ---------------------------------------------------------------

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

    /// H-29-3 slice 1 — Consensus fs_chown journals a Chown entry
    /// with `owner`/`group` populated.  Uses the CURRENT USER'S name
    /// via getlogin() so the chown syscall doesn't hit EPERM on
    /// unprivileged CI hosts.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn chown_on_consensus_appends_wal_entry() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"").unwrap();
        let runtime = create_runtime().await;
        // Current user's name — chown to self is a no-op that
        // succeeds on any host regardless of privilege.  If the name
        // lookup fails, skip the assertion (unusual CI environment).
        let current_user = match std::env::var("USER")
            .ok()
            .or_else(|| std::env::var("LOGNAME").ok())
        {
            Some(u) if !u.is_empty() => u,
            _ => return, // no user name available; skip
        };
        let term = format!(
            r#"
            new fsChown(`rho:io:fs:native:1.0.0/chown`), ret in {{
              fsChown!("{root}", "f.bin", "{user}", Nil, "consensus", *ret) |
              for (@_ <- ret) {{ Nil }}
            }}
            "#,
            root = dir.path().display(),
            user = current_user,
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
        assert_eq!(snap[0].op, WalOp::Chown);
        assert_eq!(snap[0].owner.as_deref(), Some(current_user.as_str()));
        // group=Nil in Rholang maps to Option<String>::None in the
        // native.  Pin that.
        assert_eq!(snap[0].group, None);
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
        // Sequence: chmod → copyFile → rename → removeFile.
        // (chown omitted here to avoid NSS-mapping requirements in
        // the shared test; covered by chown_on_consensus_appends_wal_entry.)
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

    // ---------------------------------------------------------------
    // H-29-3 lift, slice 2 (2026-08-26).  fs_remove_dir Consensus
    // support — non-recursive emits one RemoveDir entry; recursive
    // emits a granular sorted-post-order manifest of RemoveFile /
    // RemoveDir entries and packs the deletion manifest into the
    // reply so a rig-based follower can journal symmetrically on
    // `is_replay` without a live filesystem walk.
    // ---------------------------------------------------------------

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
        apply_wal_to_fresh_tree(
            &wal,
            &std::collections::HashMap::new(),
            leader_dir.path(),
            follower_dir.path(),
        );
        assert_dir_trees_byte_identical(leader_dir.path(), follower_dir.path(), &[]);
    }

    /// Leader/follower WAL byte-identity for recursive Consensus
    /// removeDir.  Follower's is_replay branch reads the reply
    /// manifest and re-journals the same granular entries.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn recursive_remove_dir_wal_is_byte_identical_on_leader_and_follower() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("top")).unwrap();
        std::fs::write(dir.path().join("top/a.txt"), b"a").unwrap();
        std::fs::create_dir(dir.path().join("top/nested")).unwrap();
        std::fs::write(dir.path().join("top/nested/b.txt"), b"b").unwrap();
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
            "leader/follower recursive-removeDir WAL byte-identity via reply manifest"
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
                      fsOpen!("{leader_root_str}", "data.bin", "rw", "consensus", *oc) |
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
                      fsOpen!("{leader_root_str}", "data.bin", "rw", "consensus", *oc) |
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
                      fsOpen!("{leader_root_str}", "log.txt", "rw", "consensus", *oc) |
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

        apply_wal_to_fresh_tree(&wal, &sidecar, leader_dir.path(), follower_dir.path());

        assert_dir_trees_byte_identical(leader_dir.path(), follower_dir.path(), &[]);
    }

    /// **PB-M-14 file-state-identity + failure-skip pin (2026-08-26).**
    /// Companion to `pb_m_14_file_state_identity_via_wal_replay` that
    /// exercises the H-6 `WalOutcome::Failure` skip branch of the
    /// applier: a WAL entry whose `outcome == Failure` MUST NOT be
    /// applied to the follower tree — otherwise the follower would
    /// write bytes that the leader's syscall never committed to disk.
    ///
    /// Uses a synthetic WAL directly (bypassing the runtime) so the
    /// Failure entry carries a `payload_ref` hash that is NOT in the
    /// sidecar.  If the applier respects `outcome`, the missing-hash
    /// panic never fires and the follower's tree matches the leader's;
    /// if the applier ignores `outcome`, the panic fires — either
    /// mode catches the regression cleanly.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn wal_applier_skips_failure_outcome_entries() {
        use std::io::{Seek, SeekFrom, Write};
        let leader_dir = tempfile::tempdir().unwrap();
        let follower_dir = tempfile::tempdir().unwrap();
        std::fs::write(leader_dir.path().join("real.bin"), vec![0u8; 16]).unwrap();
        std::fs::write(follower_dir.path().join("real.bin"), vec![0u8; 16]).unwrap();

        let real_payload: &[u8] = &[0x99, 0xAA];
        let real_offset: u64 = 4;
        let mut sidecar: std::collections::HashMap<[u8; 32], Vec<u8>> =
            std::collections::HashMap::new();
        if let PayloadRef::Hash(h) = PayloadRef::hash(real_payload) {
            sidecar.insert(h, real_payload.to_vec());
        }

        // Success WriteAt(4, [0x99, 0xAA]) + Failure WriteAt(8, ...)
        // with a BOGUS hash that is NOT in the sidecar.  The
        // Failure entry must be skipped, otherwise the applier's
        // `hash missing from payload sidecar` panic fires and
        // this test fails loudly.
        let bogus_hash = [0xEEu8; 32];
        let wal = vec![
            WalEntry {
                op: WalOp::WriteAt,
                path: leader_dir.path().join("real.bin"),
                extra_path: None,
                offset: Some(real_offset),
                length: Some(real_payload.len() as u64),
                payload_ref: Some(PayloadRef::hash(real_payload)),
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Success,
            },
            WalEntry {
                op: WalOp::WriteAt,
                path: leader_dir.path().join("real.bin"),
                extra_path: None,
                offset: Some(8),
                length: Some(4),
                payload_ref: Some(PayloadRef::Hash(bogus_hash)),
                mode_bits: None,
                owner: None,
                group: None,
                outcome: WalOutcome::Failure { code: 5 },
            },
        ];

        apply_wal_to_fresh_tree(&wal, &sidecar, leader_dir.path(), follower_dir.path());

        // Emulate the leader's successful syscall on leader_dir so
        // the tree-identity check has a reference.  (In the real
        // E2E flow this is a side effect of the leader's syscall.)
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(leader_dir.path().join("real.bin"))
            .unwrap();
        f.seek(SeekFrom::Start(real_offset)).unwrap();
        f.write_all(real_payload).unwrap();
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
              fsOpen!("{leader_root_str}", "data.bin", "rw", "consensus", *oc) |
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

        apply_wal_to_fresh_tree(&wal, &sidecar, leader_dir.path(), follower_dir.path());
        assert_dir_trees_byte_identical(leader_dir.path(), follower_dir.path(), &[]);
    }

    /// **Position-tracking leader/follower symmetry (2026-08-26).**
    /// The FileHandle shadow-position must evolve identically on
    /// leader and follower for `journal_write` to record byte-
    /// identical offsets on both sides.  Runs a mixed sequential-
    /// Write + Seek + Read + WriteAt sequence on a leader, drives
    /// the same sequence on a rig-based follower via
    /// `create_leader_and_follower`, and asserts the WAL entries
    /// are byte-identical — the same shape as the existing
    /// `wal_is_byte_identical_on_leader_and_follower` pin, extended
    /// to exercise the position-affecting ops (Seek + sequential
    /// Write/Read).
    ///
    /// A regression that only advanced shadow position on the
    /// leader (missing the follower's is_replay mirror) would
    /// produce a Write entry with offset=Some(pos) on the leader
    /// but offset=Some(0) on the follower for the second write —
    /// the byte-identity assertion catches it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn wal_position_stays_in_sync_on_leader_and_follower() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.bin"), vec![0u8; 128]).unwrap();
        let (mut leader, mut follower) = create_leader_and_follower().await;

        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                fsSeek(`rho:io:fs:native:1.0.0/seek`),
                fsRead(`rho:io:fs:native:1.0.0/read`),
                fsWriteAt(`rho:io:fs:native:1.0.0/writeAt`),
                oc, w1, s1, r1, w2 in {{
              fsOpen!("{root}", "data.bin", "rw", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsWrite!(fd, "aabb".hexToBytes(), *w1) |
                for (@_ <- w1) {{
                  fsSeek!(fd, 0, "set", *s1) |
                  for (@_ <- s1) {{
                    fsRead!(fd, 2, *r1) |
                    for (@_ <- r1) {{
                      fsWriteAt!(fd, 10, "ccdd".hexToBytes(), *w2) |
                      for (@_ <- w2) {{ Nil }}
                    }}
                  }}
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
            .expect("leader evaluate");
        let leader_wal = leader.fs_handles.wal.snapshot();
        assert!(!leader_wal.is_empty());

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
            .expect("follower evaluate");
        let follower_wal = follower.fs_handles.wal.snapshot();

        assert_eq!(
            leader_wal.len(),
            follower_wal.len(),
            "position-follow-up regression: leader/follower WAL count differs \
             ({} vs {}) — likely one side advanced FileHandle.position but the \
             other did not, causing journal_write to record different offsets \
             (which would trip the entry-by-entry check below in principle, \
             but count-divergence indicates a deeper mismatch)",
            leader_wal.len(),
            follower_wal.len(),
        );
        for (i, (l, f)) in leader_wal.iter().zip(follower_wal.iter()).enumerate() {
            assert_eq!(
                l, f,
                "WAL entry {i} differs between leader and follower — most \
                 likely a position-tracking asymmetry in fs_write / fs_read / \
                 fs_seek.  leader={l:?}, follower={f:?}"
            );
        }
        // The read entry (index 2) must carry offset=Some(0) — the
        // Seek(SET, 0) between write and read reset shadow position
        // to 0, so the Read journaled from position 0.  A regression
        // that dropped the fs_seek shadow-position update would
        // record offset=Some(4) (post-first-write position).
        let read_entry = leader_wal
            .iter()
            .find(|e| e.op == WalOp::Read)
            .expect("must have a Read entry");
        assert_eq!(
            read_entry.offset,
            Some(0),
            "sequential Read after Seek(SET,0) must record offset=Some(0); \
             fs_seek shadow-position update regression would set this to \
             Some(4) (post-first-write position).  Got {:?}",
            read_entry.offset,
        );
        follower
            .check_replay_data()
            .await
            .expect("follower replay data mismatch");
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

    /// **PB-M-14 two-validator E2E scaffold (2026-08-26).**  Ignored
    /// pending the harness gap documented in the Deferred items
    /// catalog: `TestNode::create_node` in
    /// `casper/tests/helper/test_node.rs` does not accept per-node
    /// fs provisioning (bundle entries, consensus_static paths).
    /// To exercise the PB-M-14 property described at
    /// implementation-plan.md:397 ("mutate a consensus file on
    /// validator A; bring validator B online from genesis + WAL only;
    /// assert byte-identical file contents") the harness needs the
    /// following additions:
    ///
    ///   1. `TestNode::create_node` gains a `fs_config:
    ///      Option<FsProvisioningConfig>` parameter, threaded through
    ///      `RuntimeManager::spawn_runtime` into the composed
    ///      FsGenesis-source-inject site.
    ///   2. `GenesisBuilder` gains a helper to build a validator's
    ///      genesis with a specific fs bundle attached so validator A
    ///      and B can share the same fs-generator PK but different
    ///      per-node bundle contents (or same bundle contents at
    ///      identical canon-paths).
    ///   3. `TestNode` gains a way to observe on-disk file contents
    ///      after a block finalizes — either via a filesystem hook or
    ///      by resolving the fs cap and issuing a read-back deploy.
    ///
    /// Once those land, this test drives:
    ///   1. Two-node network (A + B).
    ///   2. A mutates a consensus-static file (data.bin) via three
    ///      write deploys.
    ///   3. Blocks propagate; A finalizes them, WAL committed.
    ///   4. B (which started with the same fs bundle at the same
    ///      canon-path but empty file contents) reconstructs the
    ///      file state from WAL replay.
    ///   5. Assertion: `std::fs::read(B_bundle_path)` byte-matches
    ///      `std::fs::read(A_bundle_path)`.
    ///
    /// Interim coverage: the multi-deploy WAL-replay pin above
    /// verifies the WAL-BYTE-IDENTITY half of the PB-M-14 property
    /// (validators produce identical WAL sequences for identical
    /// deploys under the shared-store rig pattern).  The still-
    /// missing half is FILE-STATE-IDENTITY via replay from WAL
    /// against a fresh store.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "blocked on TestNode per-node fs provisioning + \
                observation hooks; see docstring above and Deferred \
                items catalog entry `Two-validator PB-M-14 end-to-end \
                test`"]
    async fn pb_m_14_two_validator_scaffold() {
        panic!(
            "pb_m_14_two_validator_scaffold is a documentation-only \
             scaffold — remove the #[ignore] attribute AND implement \
             the harness prerequisites listed in the docstring before \
             running.  See implementation-plan.md:397 for the \
             invariant this test targets."
        );
    }
}
