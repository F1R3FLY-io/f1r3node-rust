//! Phase 10 fd-lifecycle regression spec.
//!
//! The plan doc (implementation-plan.md §Test-infrastructure) calls
//! for a `fileio_lifecycle_spec.rs` that exercises fd-table rollback
//! on deploy error against the *production* deploy path — the
//! `RhoRuntimeImpl::create_soft_checkpoint` / `revert_to_soft_checkpoint`
//! pair — not the direct `FileHandleTable::truncate_to` unit path.
//! The unit-level rollback is already pinned in
//! `handle_table::truncate_to_snapshot_leaves_pre_snapshot_intact`;
//! this spec verifies the deploy-boundary wiring in `rho_runtime.rs`
//! (`fs_snapshot_stack.push` on create_soft_checkpoint, matching
//! `pop` + `truncate_to` on revert).
//!
//! ## What a regression looks like
//!
//! A refactor that removes the `fs_snapshot_stack.push(snapshot_next_fd())`
//! from `create_soft_checkpoint`, or the matching `pop + truncate_to`
//! from `revert_to_soft_checkpoint`, would leak fds allocated during
//! a failed deploy: the fd remains in `FileHandleTable` past the
//! deploy boundary, next-fd advances monotonically past it, and
//! subsequent deploys can never observe / reuse it — a slow leak
//! that eventually trips `MAX_OPEN_FDS` under sustained load with
//! erroring deploys.  This spec catches the removal.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
    use rholang::rust::interpreter::accounting::costs::Cost;
    use rholang::rust::interpreter::external_services::ExternalServices;
    use rholang::rust::interpreter::matcher::r#match::Matcher;
    use rholang::rust::interpreter::rho_runtime::{create_rho_runtime, RhoRuntime, RhoRuntimeImpl};
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
        // Test uses the native URN directly for deterministic fd
        // observation; the production Fs.rho / File.rho path forwards
        // via the same handler and is covered by other specs.
        runtime.disable_fs_native_urn_filter();
        runtime
    }

    /// Production-path fd rollback: a deploy that opens files AFTER a
    /// soft checkpoint has its fds swept when the checkpoint is
    /// reverted.  The pre-checkpoint fd survives.
    ///
    /// Sequence:
    ///   1. Open pre-checkpoint file → fd_pre allocated at counter
    ///      value P (post-open, counter = P+1).
    ///   2. `create_soft_checkpoint()` pushes P+1 onto the fs snapshot
    ///      stack.
    ///   3. Open post-checkpoint file → fd_post allocated at counter
    ///      value P+1 (post-open, counter = P+2).
    ///   4. `revert_to_soft_checkpoint()` pops P+1 and calls
    ///      `truncate_to(P+1)` — every fd < P+1 (i.e., fd_pre)
    ///      survives; every fd >= P+1 (i.e., fd_post) is removed.
    ///
    /// Verification uses `runtime.fs_handles.raw_fd(fd)` to probe
    /// specific fd presence — Some(_) for surviving fds, None for
    /// swept ones.  Counter itself does NOT rewind (monotonic
    /// invariant preserved by `truncate_to`); this preserves the
    /// aliasing-protection property that a stale reference to
    /// fd_post can never later resolve to a freshly-allocated
    /// unrelated file.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn deploy_error_rollback_sweeps_post_checkpoint_fds() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pre.bin"), b"pre").unwrap();
        std::fs::write(dir.path().join("post.bin"), b"post").unwrap();

        let mut runtime = create_runtime().await;

        // Sanity: fd counter starts fresh, no fds present.
        let counter_initial = runtime.fs_handles.snapshot_next_fd();

        // (1) Pre-checkpoint open.  Uses "oracular" cmode so no WAL
        // entries are appended (this spec focuses on the fd table,
        // not the WAL — WAL rollback is covered by fs_wal_spec).
        let pre_term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`), o in {{
              fsOpen!("{root}", "pre.bin", "r", "oracular", *o) |
              for (@[true, _fd] <- o) {{ Nil }}
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

        // The pre-open advanced the counter by exactly one; the
        // allocated fd equals `counter_initial`.
        let fd_pre = counter_initial;
        assert!(
            runtime.fs_handles.raw_fd(fd_pre).await.is_some(),
            "pre-checkpoint fd {fd_pre} must be present after open",
        );
        let counter_after_pre = runtime.fs_handles.snapshot_next_fd();
        assert_eq!(
            counter_after_pre,
            counter_initial + 1,
            "counter advanced by one after pre-open (initial={counter_initial}, \
             after_pre={counter_after_pre})",
        );

        // (2) Snapshot the checkpoint.  The fs snapshot stack now
        // holds `counter_after_pre` (= P+1).
        let checkpoint = runtime.create_soft_checkpoint().await;

        // (3) Post-checkpoint open — this is the fd that MUST be
        // swept by the subsequent revert.
        let post_term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`), o in {{
              fsOpen!("{root}", "post.bin", "r", "oracular", *o) |
              for (@[true, _fd] <- o) {{ Nil }}
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
        let fd_post = counter_after_pre;
        assert!(
            runtime.fs_handles.raw_fd(fd_post).await.is_some(),
            "post-checkpoint fd {fd_post} must be present after open",
        );
        let counter_after_post = runtime.fs_handles.snapshot_next_fd();
        assert_eq!(
            counter_after_post,
            counter_after_pre + 1,
            "counter advanced by one after post-open",
        );

        // (4) Revert.  Pre-checkpoint fd survives; post-checkpoint fd
        // is swept.  Counter does NOT rewind — monotonicity is the
        // aliasing-protection invariant.
        runtime.revert_to_soft_checkpoint(checkpoint).await;
        assert!(
            runtime.fs_handles.raw_fd(fd_pre).await.is_some(),
            "revert must NOT sweep pre-checkpoint fd {fd_pre}",
        );
        assert!(
            runtime.fs_handles.raw_fd(fd_post).await.is_none(),
            "revert MUST sweep post-checkpoint fd {fd_post} \
             (regression: fs_snapshot_stack push/pop broken, or \
             truncate_to skipped in revert_to_soft_checkpoint)",
        );
        let counter_post_revert = runtime.fs_handles.snapshot_next_fd();
        assert_eq!(
            counter_post_revert, counter_after_post,
            "counter must be monotonic across revert — a rewound counter \
             would break the aliasing-protection property (a subsequent \
             open at fd_post could serve a stale reference)",
        );
    }

    /// Nested checkpoints: an inner revert must sweep only fds
    /// opened between the inner checkpoint and now, leaving fds
    /// opened between the outer checkpoint and the inner one
    /// intact.  Companion to `nested_soft_checkpoints_preserve_outer_wal_mark`
    /// in `fs_wal_spec.rs` (H4/M1 round-2 fix — the stack pattern
    /// is the fix, and it applies to both the fd snapshot and the
    /// WAL snapshot).  A regression that reverts stack semantics
    /// back to a single-slot cell would trip the WAL pin OR this
    /// pin (or both) depending on which slot the refactor picks.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn nested_soft_checkpoints_preserve_outer_fd_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.bin"), b"a").unwrap();
        std::fs::write(dir.path().join("b.bin"), b"b").unwrap();
        std::fs::write(dir.path().join("c.bin"), b"c").unwrap();

        let mut runtime = create_runtime().await;
        let counter_initial = runtime.fs_handles.snapshot_next_fd();

        // Open A before any checkpoint.
        run_open(&mut runtime, dir.path(), "a.bin").await;
        let fd_a = counter_initial;

        // Outer checkpoint.  Snapshot = counter_after_a.
        let outer = runtime.create_soft_checkpoint().await;

        // Open B between outer and inner.
        run_open(&mut runtime, dir.path(), "b.bin").await;
        let fd_b = counter_initial + 1;

        // Inner checkpoint.  Snapshot = counter_after_b.
        let inner = runtime.create_soft_checkpoint().await;

        // Open C after inner.
        run_open(&mut runtime, dir.path(), "c.bin").await;
        let fd_c = counter_initial + 2;

        // Revert INNER: sweep fd_c only; fd_a and fd_b survive.
        runtime.revert_to_soft_checkpoint(inner).await;
        assert!(
            runtime.fs_handles.raw_fd(fd_a).await.is_some(),
            "fd_a survives inner revert"
        );
        assert!(
            runtime.fs_handles.raw_fd(fd_b).await.is_some(),
            "fd_b survives inner revert"
        );
        assert!(
            runtime.fs_handles.raw_fd(fd_c).await.is_none(),
            "fd_c swept by inner revert"
        );

        // Revert OUTER: sweep fd_b too; fd_a survives.  Regression
        // scenario the stack fixes: pre-fix a single-slot fs snapshot
        // cell would have been overwritten by the inner
        // create_soft_checkpoint, so the outer revert would find no
        // snapshot and sweep nothing — fd_b would leak past its
        // deploy boundary.
        runtime.revert_to_soft_checkpoint(outer).await;
        assert!(
            runtime.fs_handles.raw_fd(fd_a).await.is_some(),
            "fd_a survives outer revert"
        );
        assert!(
            runtime.fs_handles.raw_fd(fd_b).await.is_none(),
            "fd_b swept by outer revert (regression: nested-snapshot stack \
             semantics broken — see rho_runtime.rs::create_soft_checkpoint \
             H4/M1 round-2 fix)",
        );
    }

    /// **Streaming-slice Step 4 review-fixup pin (2026-08-26).**
    /// `reset()` must clear `dir_fs_snapshot_stack` so a subsequent
    /// `revert_to_soft_checkpoint` without a matching `create` cannot
    /// pop a stale mark and truncate the dir table to a pre-reset
    /// watermark.  Companion invariant to the M6 round-2 fix for the
    /// file-fd + WAL stacks documented at rho_runtime.rs::reset.
    ///
    /// Regression scenario: if `reset()` forgets to clear
    /// `dir_fs_snapshot_stack`, an unbalanced revert pops the stale
    /// mark (=post_open_1_value, some small number) and calls
    /// `truncate_to(mark)`, which sweeps every dir fd >= mark.  Post-
    /// reset, the fd counter is re-seeded from state hash to a much
    /// larger watermark, so newly-allocated fds fall above the stale
    /// mark and get incorrectly swept.  This test opens a stream
    /// post-reset and confirms it survives the unbalanced revert.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn reset_clears_dir_fs_snapshot_stack() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let mut runtime = create_runtime().await;

        // (1) Open a dir stream pre-reset to seed a mark.
        run_open_dir_stream(&mut runtime, dir.path(), "sub").await;

        // (2) Capture a checkpoint — pushes a mark on
        // `dir_fs_snapshot_stack` (private field; observed indirectly
        // via the revert behavior below).
        let stale_checkpoint = runtime.create_soft_checkpoint().await;

        // (3) Reset to the current root — MUST clear the stack.
        let root = runtime.get_root().await;
        runtime.reset(&root).await.unwrap();

        // (4) Open a new dir stream — allocated at a post-reset
        // watermark (state-hash-seeded, much larger than the pre-
        // reset mark).
        run_open_dir_stream(&mut runtime, dir.path(), "sub").await;
        let post_reset_fd = runtime.fs_handles.dir_handles.snapshot_next_fd() - 1;

        // (5) Unbalanced revert — the stack was cleared by reset, so
        // this must be a no-op.  If reset forgot to clear the stack,
        // the pop would find the stale mark and truncate every fd >=
        // that mark, including `post_reset_fd`.
        runtime.revert_to_soft_checkpoint(stale_checkpoint).await;

        assert!(
            runtime
                .fs_handles
                .dir_handles
                .get(post_reset_fd)
                .await
                .is_some(),
            "post-reset dir stream fd {post_reset_fd} must survive an \
             unbalanced revert (reset must have cleared \
             dir_fs_snapshot_stack).  A regression that drops the \
             `self.dir_fs_snapshot_stack.lock().unwrap().clear()` line \
             in rho_runtime.rs::reset would trip this pin: the pre-reset \
             mark (small) would be popped and truncate_to would sweep \
             the post-reset fd (large)."
        );
    }

    /// **Streaming-slice Step 4 review-fixup pin (2026-08-26).**
    /// `revert_to_soft_checkpoint` with no matching `create` is a
    /// defensive no-op on the dir stack — the docstring at
    /// rho_runtime.rs pins this contract.  Mirror pin exists
    /// implicitly for the file-fd stack (would hit the same
    /// `if let Some(s) = snap` guard), but no companion test
    /// exercises the dir stack path.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn unbalanced_dir_revert_without_matching_create_is_no_op() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let mut runtime = create_runtime().await;

        // Open a dir stream and capture a checkpoint.
        run_open_dir_stream(&mut runtime, dir.path(), "sub").await;
        let checkpoint = runtime.create_soft_checkpoint().await;
        let fd_before = runtime.fs_handles.dir_handles.snapshot_next_fd() - 1;

        // Balanced revert — sweeps nothing new (the fd was opened
        // before the checkpoint).
        runtime.revert_to_soft_checkpoint(checkpoint).await;
        assert!(
            runtime
                .fs_handles
                .dir_handles
                .get(fd_before)
                .await
                .is_some(),
            "pre-checkpoint dir fd survives balanced revert"
        );

        // Second (unbalanced) revert — no matching create exists, so
        // the stack is empty; the revert must be a no-op.  Get a
        // fresh checkpoint value to hand in (any SoftCheckpoint from
        // reducer.space.create_soft_checkpoint works — the WalOp
        // stacks are what matter for our pin).
        let unbalanced = runtime.reducer.space.create_soft_checkpoint().await;
        runtime.revert_to_soft_checkpoint(unbalanced).await;
        assert!(
            runtime
                .fs_handles
                .dir_handles
                .get(fd_before)
                .await
                .is_some(),
            "unbalanced revert must be a no-op: the fd must survive \
             (regression: `if let Some(s) = dir_snap` guard removed)"
        );
    }

    async fn run_open_dir_stream(runtime: &mut RhoRuntimeImpl, root: &std::path::Path, rel: &str) {
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`), o in {{
              fsOpen!("{root}", "{rel}", "oracular", *o) |
              for (@[true, _fd] <- o) {{ Nil }}
            }}
            "#,
            root = root.display(),
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
    }

    async fn run_open(runtime: &mut RhoRuntimeImpl, root: &std::path::Path, rel: &str) {
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`), o in {{
              fsOpen!("{root}", "{rel}", "r", "oracular", *o) |
              for (@[true, _fd] <- o) {{ Nil }}
            }}
            "#,
            root = root.display(),
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
    }
}
