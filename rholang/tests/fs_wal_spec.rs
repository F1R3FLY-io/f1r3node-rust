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
    use fileio_test_fixtures::{apply_wal_translated, assert_dir_trees_byte_identical};
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

    // Wave-3 S3.14 (2026-09-10) — per-family test submodules via
    // `#[path]`.  Each submodule inherits parent scope via
    // `use super::*;` for access to `create_runtime`, `rand`, and
    // the shared imports at the top of this `mod tests` block.
    // All submodule tests run as part of the single `fs_wal_spec`
    // integration binary — no proliferation of test binaries.
    #[path = "fs_wal/lifecycle.rs"]
    mod lifecycle;
    #[path = "fs_wal/mutation.rs"]
    mod mutation;
    #[path = "fs_wal/observation.rs"]
    mod observation;
    #[path = "fs_wal/stream.rs"]
    mod stream;

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
              fsOpen!("{root}", "a.bin", "r+", "consensus", *oa) |
              for (@[true, fdA] <- oa) {{
                fsWrite!(fdA, "aa".hexToBytes(), *wa) |
                for (@_ <- wa) {{
                  fsOpen!("{root}", "b.bin", "r+", "consensus", *ob) |
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
              fsOpen!("{root}", "f.bin", "r+", "consensus", *oc) |
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
              fsOpen!("{root}", "data.bin", "r+", "consensus", *oc) |
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

    /// X-3 / SEC-3 (2026-09-12, branch-review-2026-09-11.md):
    /// pin the load-bearing "cost pre-charge runs BEFORE WAL-cap
    /// check" ordering.  When the WAL is full, the pre-charge is
    /// consumed even though the handler returns
    /// `FSERR_QUOTA_EXCEEDED` and appends no entry.  This IS
    /// intentional (per user 2026-09-11: Item 1 explicitly
    /// deferred; refund would emit an unaccounted BillableToken
    /// Event that shifts the `authority_cost_witness` fold bytes
    /// → hard-fork surface).
    ///
    /// This regression pin prevents a future refactor from
    /// accidentally introducing a "charge-then-refund" pattern:
    /// - Pre-fill the WAL to `MAX_WAL_ENTRIES`.
    /// - Snapshot the runtime's remaining cost budget.
    /// - Issue an `fs_write` that will trigger the WAL-cap
    ///   FSERR_QUOTA_EXCEEDED path.
    /// - Assert the budget DECREASED (proving the pre-charge
    ///   fired) — a refund would leave the budget unchanged or
    ///   only partially decreased.
    ///
    /// See `handler_trait.rs::dispatch_via_trait` step 3 for the
    /// full ordering documentation.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn sec3_wal_cap_full_pre_charge_burns_cost() {
        use rholang::rust::interpreter::io::wal::{
            PayloadRef, WalEntry, WalOp, WalOutcome, MAX_WAL_ENTRIES,
        };

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"").unwrap();
        let runtime = create_runtime().await;

        // Pre-fill WAL to the cap.
        for _ in 0..MAX_WAL_ENTRIES {
            runtime
                .fs_handles
                .wal
                .append(WalEntry {
                    op: WalOp::Write,
                    path: std::path::PathBuf::from("/prefill"),
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
        // Pass a bounded initial budget to `evaluate` so we can
        // measure cost consumption precisely.  1M phlo is well
        // above the fs_open + fs_write attempt cost but well below
        // `Cost::unsafe_max`.  (Setting `runtime.cost` directly
        // doesn't work — evaluate reinitializes it from its
        // `initial_phlos` argument.)
        let initial_budget = Cost::create(1_000_000, "sec3_test");
        // Issue an fs_write on a Consensus cap — journal_write
        // attempts to append and hits the cap.
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                oc, wc
            in {{
              fsOpen!("{root}", "f.bin", "r+", "consensus", *oc) |
              for (@[true, fd] <- oc) {{
                fsWrite!(fd, "aa".hexToBytes(), *wc) |
                for (@_reply <- wc) {{ Nil }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let initial_value = initial_budget.value;
        runtime
            .evaluate(
                &term,
                initial_budget,
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .unwrap();
        let budget_after = runtime.cost.get();
        assert!(
            budget_after.value < initial_value,
            "SEC-3: cost budget MUST decrease after fs_write hit \
             WAL-cap-full (proving the pre-charge fired).  A \
             refund would leave the budget at or near \
             `initial_value` = {}; got after={:?}",
            initial_value,
            budget_after,
        );
        // WAL must NOT have grown past cap (unchanged from the
        // sister H1 pin).
        assert_eq!(
            runtime.fs_handles.wal.len(),
            MAX_WAL_ENTRIES,
            "WAL cap must hold even after failed append attempt"
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
              fsOpen!("{root}", "f.bin", "r+", "consensus", *oc) |
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
              fsOpen!("{root}", "a.bin", "r+", "consensus", *o) |
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
              fsOpen!("{root}", "pre.bin", "r+", "consensus", *o) |
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
              fsOpen!("{root}", "post.bin", "r+", "consensus", *o) |
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
              fsOpen!("{root}", "f.bin", "r+", "consensus", *o) |
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
              fsOpen!("{root}", "f.bin", "r+", "consensus", *o) |
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
            // M-31 (2026-09-06, A6-F-3): retention pruning
            // (`prune_snapshot_dir`) sorts by filesystem mtime.  On
            // APFS the mtime granularity is 1 second, so consecutive
            // writes can tie and leave the retention ordering
            // implementation-defined.  On ext4 / xfs / zfs the
            // granularity is nanosecond and consecutive writes yield
            // distinct mtimes without any sleep.  Gate the 1.1s
            // per-iteration sleep behind `target_os = "macos"` so
            // Linux CI drops ~4.4s of pure wall time; macOS keeps
            // its coverage identical.
            //
            // Not folded into a monotone-counter arg to `maybe_write`
            // because the retention policy's mtime semantics are a
            // consensus surface (peer-fetch tier reads the same
            // directory + retention convention) and changing the
            // ordering key is out of scope for this test-suite fix.
            #[cfg(target_os = "macos")]
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
                      fsOpen!("{root_display}", "data.bin", "r+", "consensus", *oc) |
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
                      fsOpen!("{root_display}", "data.bin", "r+", "consensus", *oc) |
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
                      fsOpen!("{root_display}", "aux.bin", "r+", "oracular", *oc) |
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
              fsOpen!("{root_display}", "data.bin", "r+", "consensus", *oc) |
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

    // T-06 (2026-09-11, wave-4 Cluster D+E Phase 5): the previously
    // inline test helpers `assert_dir_trees_byte_identical`,
    // `translate_path`, and `apply_wal_translated` moved to the
    // sibling `fileio-test-fixtures` crate.  Imported at the top of
    // this `mod tests` block; propagated to `#[path]`-included
    // submodules (`mutation.rs`, `observation.rs`, etc.) via the
    // existing `use super::*;` re-export chain.

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
              fsOpen!("{root}", "data.bin", "r+", "consensus", *oc) |
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
