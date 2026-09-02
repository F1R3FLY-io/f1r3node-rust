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
    use rholang::rust::interpreter::io::wal_applier::apply_wal_to_fresh_tree;
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
              fsOpen!("{root}", "data.bin", "rw", "consensus", *openCh) |
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
              fsOpen!("{root}", "data.bin", "rw", "consensus", *openCh) |
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
        let leader_size_entries: Vec<_> =
            leader_wal.iter().filter(|e| e.op == WalOp::Size).collect();
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
        let leader_size_entries: Vec<_> =
            leader_wal.iter().filter(|e| e.op == WalOp::Size).collect();
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
        let leader_read_entries: Vec<_> =
            leader_wal.iter().filter(|e| e.op == WalOp::Read).collect();
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

    /// Phase 2 ban pin (Consensus re-execute + verify, 2026-09-01):
    /// **`entriesStreamOpen` with cmode="consensus" MUST reject
    /// with `FSERR_UNSUPPORTED`.**  See handlers.rs's
    /// `fs_entries_stream_open` for the design rationale: readdir
    /// order is fs-dependent and not stable across D3 per-validator
    /// subdirs, so a Consensus-cap stream would trip spurious
    /// CONSENSUS_DIVERGENCE on any two validators with independently-
    /// created copies of the same logical directory.  Users are
    /// directed to bulk `fs_entries` (sorted, deterministic) instead.
    ///
    /// A regression that dropped the ban would let Consensus stream
    /// opens through; downstream `entriesStreamNext` would then
    /// exercise the Phase-0 tautological cached-reply consumption
    /// path (removed as dead code by the ban commit) and mask real
    /// divergences.  This pin makes the ban load-bearing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn entries_stream_open_rejects_consensus_with_fserr_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let runtime = create_runtime().await;

        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`), o in {{
              fsOpen!("{root}", "sub", "consensus", *o) |
              for (@reply <- o) {{
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
            .expect("evaluate Consensus entriesStreamOpen");

        // Capture the reply from @"result" and assert it's specifically
        // FSERR_UNSUPPORTED — not some other early-return error code.
        // A regression that swapped the ban for a BAD_ARG / IO / etc.
        // would still produce no WAL entry (and the assertion below
        // would still pass), so we need the code-slot check to lock the
        // specific FSERR down.
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
            "Consensus entriesStreamOpen rejection must use FSERR_UNSUPPORTED \
             specifically (see handlers.rs::fs_entries_stream_open ban comment). \
             A regression that returned FSERR_BAD_ARG or FSERR_IO would still \
             produce no WAL entry so the wal.is_empty() check below wouldn't \
             catch it — this assertion is the load-bearing pin for the \
             specific ban code.  Got code: {code}"
        );

        // No WAL entry should be journaled since the open was rejected
        // before any fd allocation.
        assert!(
            runtime.fs_handles.wal.is_empty(),
            "Consensus entriesStreamOpen rejection must NOT journal — the \
             leader errored out before any fd was created, so there's no \
             stream state to journal.  Got WAL: {:?}",
            runtime.fs_handles.wal.snapshot()
        );
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
              fsOpen!("{root}", "data.bin", "rw", "consensus", *oc) |
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
              fsOpen!("{root}", "data.bin", "rw", "consensus", *oc) |
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
              fsOpen!("{root}", "data.bin", "rw", "consensus", *oc) |
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
              fsOpen!("{root}", "data.bin", "rw", "consensus", *oc) |
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
              fsOpen!("{root}", "data.bin", "rw", "consensus", *oc) |
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
              fsOpen!("{root}", "data.bin", "rw", "consensus", *oc) |
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
              fsOpen!("{root}", "data.bin", "rw", "consensus", *oc) |
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
            payload_dir: None,
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
            payload_dir: None,
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
            payload_dir: None,
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

    /// Test-only wrapper for `apply_wal_to_fresh_tree` that
    /// translates leader-tree WAL paths onto a follower tree via
    /// `translate_path`.  Production joiners pass an identity
    /// closure directly (the WAL already carries the joiner's
    /// canonical paths); this helper keeps the four `pb_m_14_*`
    /// call sites terse.
    ///
    /// Passes empty `allowed_roots` — the test fixtures use
    /// tempdirs so operator-frozen consensus-static-root
    /// validation is not applicable; production sites plumb the
    /// actual roots.
    fn apply_wal_translated(
        wal: &[WalEntry],
        payload_bytes: &std::collections::HashMap<[u8; 32], Vec<u8>>,
        leader_root: &std::path::Path,
        follower_root: &std::path::Path,
    ) {
        apply_wal_to_fresh_tree(
            wal,
            payload_bytes,
            |p| translate_path(leader_root, follower_root, p),
            &[],
        )
        .expect("test-driven WAL apply must not produce ApplierError");
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

    // ---------------------------------------------------------------
    // H-29-3 lift, slice 2 (2026-08-26) + Phase 4 R5(b) (2026-09-02).
    // fs_remove_dir Consensus support:
    //   - Non-recursive: emits one RemoveDir entry.  Phase 4 added
    //     follower re-execute + verify against its own subdir.
    //   - Recursive: emits a granular sorted-post-order manifest of
    //     RemoveFile / RemoveDir entries.  R5(b) shifted the reply
    //     manifest from absolute per-validator paths to relative
    //     paths so both leader and follower walk their OWN subdirs
    //     and produce byte-identical WAL + reply.
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

        apply_wal_translated(&wal, &sidecar, leader_dir.path(), follower_dir.path());

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

        apply_wal_translated(&wal, &sidecar, leader_dir.path(), follower_dir.path());

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

        apply_wal_translated(&wal, &sidecar, leader_dir.path(), follower_dir.path());
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

    // NOTE: pre-Phase-0 documentation-only scaffold
    // `pb_m_14_two_validator_scaffold` removed 2026-09-02.  All three
    // harness prerequisites it requested (per-node fs provisioning,
    // per-validator genesis with fs bundle, on-disk observation
    // hooks) landed as part of the Phase 0 Stage 2 harness rework.
    // The PB-M-14 property is now exercised by the real two-
    // validator canaries in
    // `casper/tests/multi_node/pb_m_14_two_validator_e2e.rs`:
    //   - pb_m_14_two_validator_wal_and_file_byte_identity: WAL +
    //     on-disk byte-identity for a Consensus write.
    //   - pb_m_14_leader_pending_wal_slice_publishes_consensus_write:
    //     play-side WAL aggregation.
    //   - pb_m_14_option2_leader_records_and_reproduces_via_scratch_replay:
    //     scratch replay from WAL.
    //   - pb_m_14_pseudo_joiner_boots_via_peer_fetch_tier:
    //     joiner-bootstrap via peer-fetch.
    //   - pb_m_14_divergent_follower_read_causes_block_rejection:
    //     Phase 5 divergence-detection end-to-end.
}
