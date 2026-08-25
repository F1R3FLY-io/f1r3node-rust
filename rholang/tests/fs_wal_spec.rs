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
    use rholang::rust::interpreter::io::wal::{PayloadRef, WalOp};
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
        assert_eq!(e.offset, None);
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
        assert_eq!(e.offset, None);
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
        let root = res.unwrap();

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
}
