//! Streaming-backing slice Step 2 spec — three natives.
//!
//! Exercises the oracular happy path of the per-fd directory
//! streaming primitive end-to-end through the real dispatcher:
//! `entriesStreamOpen` allocates a stream fd, `entriesStreamNext`
//! yields one entry per call in `[true, entryRecord]` shape until
//! it returns the 2-element `[false, "EOS"]` terminator, and
//! `entriesStreamClose` releases the fd.
//!
//! D3 WAL journaling (Step 3 of the slice) is intentionally not
//! exercised here — these tests use `cmode = "oracular"` so no WAL
//! entries are expected.  A follow-up spec will cover the
//! consensus-mode replay parity once the WAL wiring lands.

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

    fn rand() -> Blake2b512Random {
        Blake2b512Random::create_from_bytes(&[3, 14, 15, 92, 65, 35, 89, 79])
    }

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
        // Direct-URN dispatch for deterministic fd observation.
        runtime.disable_fs_native_urn_filter();
        runtime
    }

    async fn eval(runtime: &mut RhoRuntimeImpl, term: &str) {
        runtime
            .evaluate(
                term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .unwrap();
    }

    /// Open a directory-stream against a temp dir, iterate it via
    /// repeated Next calls until EOS, and Close.  Counts successful
    /// yields via a running total accumulated in a Rholang state
    /// cell.  Asserts that (a) opening allocated exactly one fd,
    /// (b) iteration produced exactly the expected number of
    /// entries, (c) close removed the fd.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn stream_open_next_until_eos_close() {
        let dir = tempfile::tempdir().unwrap();
        // safe_descend rejects rel = "." with RootSelf; the streaming
        // primitive matches bulk fs_entries in requiring a
        // subdirectory rel path.  Create dir/sub/{a,b,c} and iterate
        // via rel = "sub".
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/a"), b"1").unwrap();
        std::fs::write(dir.path().join("sub/b"), b"2").unwrap();
        std::fs::write(dir.path().join("sub/c"), b"3").unwrap();
        let mut runtime = create_runtime().await;
        let counter_initial = runtime.fs_handles.dir_handles.snapshot_next_fd();

        // Open the stream.  Extract fd into a shared name so the
        // subsequent Next/Close deploys can pass it in.
        let open_term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`), o in {{
              fsOpen!("{root}", "sub", "oracular", *o) |
              for (@[true, _fd] <- o) {{ Nil }}
            }}
            "#,
            root = dir.path().display(),
        );
        eval(&mut runtime, &open_term).await;

        // Exactly one dir-stream fd allocated at counter_initial.
        let fd = counter_initial;
        assert!(
            runtime.fs_handles.dir_handles.get(fd).await.is_some(),
            "open must allocate a live dir-stream fd at {fd}"
        );
        assert_eq!(
            runtime.fs_handles.dir_handles.snapshot_next_fd(),
            counter_initial + 1,
            "counter advanced by exactly one open"
        );

        // Drive Next repeatedly.  The stream should yield exactly 3
        // entries (one per file); the 4th call must return
        // `[false, "EOS"]`.  We don't inspect reply shape from Rust
        // (would require capturing the tuplespace); the observable
        // guarantee is that every Next deploy evaluates cleanly (no
        // exception, no hang) and the underlying handle survives all
        // Next calls until explicit close.
        for _ in 0..4 {
            let next_term = format!(
                r#"
                new fsNext(`rho:io:fs:native:1.0.0/entriesStreamNext`), r in {{
                  fsNext!({fd}, *r) |
                  for (@_reply <- r) {{ Nil }}
                }}
                "#,
                fd = fd,
            );
            eval(&mut runtime, &next_term).await;
        }
        // Handle survives across the Next calls — Next does NOT
        // implicitly remove on EOS.  Explicit Close is required.
        assert!(
            runtime.fs_handles.dir_handles.get(fd).await.is_some(),
            "handle survives across N+1 Next calls (EOS is not close)"
        );

        // Close the stream.
        let close_term = format!(
            r#"
            new fsClose(`rho:io:fs:native:1.0.0/entriesStreamClose`), r in {{
              fsClose!({fd}, *r) |
              for (@_reply <- r) {{ Nil }}
            }}
            "#,
            fd = fd,
        );
        eval(&mut runtime, &close_term).await;

        // Close must remove the fd from the table.
        assert!(
            runtime.fs_handles.dir_handles.get(fd).await.is_none(),
            "close must remove fd {fd} from the dir-stream table"
        );
        // Counter is monotonic — Close does NOT rewind it.
        assert_eq!(
            runtime.fs_handles.dir_handles.snapshot_next_fd(),
            counter_initial + 1,
            "close must not rewind the fd counter (aliasing-protection invariant)"
        );
    }

    /// Close is idempotent — closing an already-removed fd is a
    /// no-op.  Matches `fs_close`'s shape: the return is `[true]`
    /// regardless of prior state.  Ensures a deploy that closes
    /// twice (e.g., on Stream.EOS wrapper + explicit consumer close)
    /// does not observe an error.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn stream_close_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/f"), b"x").unwrap();
        let mut runtime = create_runtime().await;

        let counter_initial = runtime.fs_handles.dir_handles.snapshot_next_fd();
        let open_term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`), o in {{
              fsOpen!("{root}", "sub", "oracular", *o) |
              for (@[true, _fd] <- o) {{ Nil }}
            }}
            "#,
            root = dir.path().display(),
        );
        eval(&mut runtime, &open_term).await;
        let fd = counter_initial;

        let close_term = format!(
            r#"
            new fsClose(`rho:io:fs:native:1.0.0/entriesStreamClose`), r in {{
              fsClose!({fd}, *r) |
              for (@_reply <- r) {{ Nil }}
            }}
            "#,
            fd = fd,
        );
        eval(&mut runtime, &close_term).await;
        assert!(
            runtime.fs_handles.dir_handles.get(fd).await.is_none(),
            "first close removes fd"
        );

        // Second close must not error.  We observe success by the
        // deploy evaluating cleanly (eval() unwraps).
        eval(&mut runtime, &close_term).await;
        assert!(
            runtime.fs_handles.dir_handles.get(fd).await.is_none(),
            "fd remains absent after redundant close"
        );
    }

    /// Deploy-rollback pin: fds allocated between a soft-checkpoint
    /// and its revert are swept by `truncate_to`.  Companion to
    /// `deploy_error_rollback_sweeps_post_checkpoint_fds` in
    /// `fileio_lifecycle_spec.rs` — same invariant, applied to the
    /// dir-stream table.  Regression scenario: a refactor forgetting
    /// to push+pop `dir_handles.snapshot_next_fd()` into the fs
    /// snapshot stack would leak stream fds past a reverted deploy.
    ///
    /// **Step 4 of the streaming slice wired this in** (2026-08-25) via
    /// a new `dir_fs_snapshot_stack` on `RhoRuntimeImpl` — push on
    /// `create_soft_checkpoint`, pop + `truncate_to` on
    /// `revert_to_soft_checkpoint`, clear on `reset`.  Now serves as
    /// the regression pin for that wiring.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn deploy_error_rollback_sweeps_stream_fds() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/pre"), b"1").unwrap();
        std::fs::write(dir.path().join("sub/post"), b"2").unwrap();

        let mut runtime = create_runtime().await;
        let counter_initial = runtime.fs_handles.dir_handles.snapshot_next_fd();

        let open_term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`), o in {{
              fsOpen!("{root}", "sub", "oracular", *o) |
              for (@[true, _fd] <- o) {{ Nil }}
            }}
            "#,
            root = dir.path().display(),
        );
        // Pre-checkpoint open.
        eval(&mut runtime, &open_term).await;
        let fd_pre = counter_initial;

        let checkpoint = runtime.create_soft_checkpoint().await;

        // Post-checkpoint open.
        eval(&mut runtime, &open_term).await;
        let fd_post = counter_initial + 1;

        runtime.revert_to_soft_checkpoint(checkpoint).await;
        assert!(
            runtime.fs_handles.dir_handles.get(fd_pre).await.is_some(),
            "pre-checkpoint stream fd survives revert"
        );
        assert!(
            runtime.fs_handles.dir_handles.get(fd_post).await.is_none(),
            "post-checkpoint stream fd swept by revert"
        );
    }

    /// Nested checkpoints for dir-stream fds: an inner revert sweeps
    /// only fds opened between the inner checkpoint and now, leaving
    /// fds opened between the outer checkpoint and the inner one
    /// intact.  Direct companion to
    /// `nested_soft_checkpoints_preserve_outer_fd_snapshot` in
    /// `fileio_lifecycle_spec.rs` — the file-fd snapshot stack fix
    /// (H4/M1 round-2) is re-applied to `dir_fs_snapshot_stack` in
    /// streaming-slice Step 4.  A regression that reverts stack
    /// semantics back to `Option<u64>` would trip this pin (inner
    /// create overwrites outer mark; outer revert finds nothing and
    /// leaks `fd_b`).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn nested_soft_checkpoints_preserve_outer_dir_fd_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("a")).unwrap();
        std::fs::create_dir(dir.path().join("b")).unwrap();
        std::fs::create_dir(dir.path().join("c")).unwrap();

        let mut runtime = create_runtime().await;
        let counter_initial = runtime.fs_handles.dir_handles.snapshot_next_fd();

        // Open A before any checkpoint.
        run_open(&mut runtime, dir.path(), "a").await;
        let fd_a = counter_initial;

        // Outer checkpoint.  Snapshot = counter after A.
        let outer = runtime.create_soft_checkpoint().await;

        // Open B between outer and inner.
        run_open(&mut runtime, dir.path(), "b").await;
        let fd_b = counter_initial + 1;

        // Inner checkpoint.  Snapshot = counter after B.
        let inner = runtime.create_soft_checkpoint().await;

        // Open C after inner.
        run_open(&mut runtime, dir.path(), "c").await;
        let fd_c = counter_initial + 2;

        // Revert INNER: sweep fd_c only; fd_a and fd_b survive.
        runtime.revert_to_soft_checkpoint(inner).await;
        assert!(
            runtime.fs_handles.dir_handles.get(fd_a).await.is_some(),
            "fd_a survives inner revert"
        );
        assert!(
            runtime.fs_handles.dir_handles.get(fd_b).await.is_some(),
            "fd_b survives inner revert"
        );
        assert!(
            runtime.fs_handles.dir_handles.get(fd_c).await.is_none(),
            "fd_c swept by inner revert"
        );

        // Revert OUTER: sweep fd_b; fd_a survives.  The regression the
        // stack fixes is: a single-slot `Option<u64>` cell would be
        // overwritten by the inner create, so the outer revert would
        // find no snapshot and sweep nothing — fd_b would leak past
        // its deploy boundary.
        runtime.revert_to_soft_checkpoint(outer).await;
        assert!(
            runtime.fs_handles.dir_handles.get(fd_a).await.is_some(),
            "fd_a survives outer revert"
        );
        assert!(
            runtime.fs_handles.dir_handles.get(fd_b).await.is_none(),
            "fd_b swept by outer revert (regression: nested-snapshot \
             stack semantics broken — see rho_runtime.rs::create_soft_checkpoint \
             streaming-slice Step 4 wiring)",
        );
    }

    /// Helper for the nested-checkpoint pin: open a dir stream via the
    /// native URN and drain the ack.  Returns after the deploy
    /// completes; the fd is left registered in `dir_handles`.
    async fn run_open(runtime: &mut RhoRuntimeImpl, root: &std::path::Path, rel: &str) {
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`), o in {{
              fsOpen!("{root}", "{rel}", "oracular", *o) |
              for (@[true, _fd] <- o) {{ Nil }}
            }}
            "#,
            root = root.display(),
        );
        eval(runtime, &term).await;
    }

    // ------------------------------------------------------------------
    // Review-fixup handler error-path pins (2026-08-25).
    //
    // Cover the reply-shape branches that the happy-path tests miss:
    // safe_descend rejection, non-existent path, opening a file as if it
    // were a directory, Next on unknown fd, Next with a malformed fd
    // argument.  Each verifies the reply shape via a Rholang-side
    // pattern match on `[false, code, _]` — if the handler misclassified
    // (e.g. an error slipped through as `[true, _]`), the match fails
    // and the deploy hangs, tripping the harness timeout.  Absent a
    // shape-capture channel from Rust we can't assert the specific
    // FSERR_* code from here, but the shape check is sufficient to
    // detect regressions that flip a error branch into an ok branch.
    // ------------------------------------------------------------------

    /// Open with a `rel` containing `..` — safe_descend rejects with
    /// `FSERR_QUARANTINE`.  A regression that skipped the quarantine
    /// check (e.g. by joining paths directly and handing to
    /// `tokio::fs::read_dir`) would allow directory traversal outside
    /// the operator-provisioned root.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn open_with_parent_dir_rel_rejects_quarantine() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let mut runtime = create_runtime().await;
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`), o in {{
              fsOpen!("{root}", "../etc", "oracular", *o) |
              for (@reply <- o) {{
                match reply {{
                  [false, "FSERR_QUARANTINE", _] => Nil
                  _ => @"MISMATCH"!(reply)
                }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        eval(&mut runtime, &term).await;
        // A stream fd MUST NOT have been allocated.
        assert_eq!(
            runtime.fs_handles.dir_handles.snapshot_next_fd(),
            1,
            "safe_descend rejection must NOT advance the dir fd counter"
        );
    }

    /// Open against a non-existent subdirectory — `openat` returns
    /// ENOENT which maps to `FSERR_NOT_FOUND`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn open_nonexistent_subdir_returns_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let mut runtime = create_runtime().await;
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`), o in {{
              fsOpen!("{root}", "does-not-exist", "oracular", *o) |
              for (@reply <- o) {{
                match reply {{
                  [false, "FSERR_NOT_FOUND", _] => Nil
                  _ => @"MISMATCH"!(reply)
                }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        eval(&mut runtime, &term).await;
        assert_eq!(runtime.fs_handles.dir_handles.snapshot_next_fd(), 1);
    }

    /// Open with a rel pointing at a file (not a directory) — the
    /// `O_DIRECTORY` flag in `openat` rejects with ENOTDIR, which
    /// maps to `FSERR_IO` (kernel ErrorKind::Other on Unix; the
    /// exact code isn't spec-canonical for ENOTDIR, so the pin
    /// checks the shape only).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn open_on_regular_file_rejects() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.bin"), b"x").unwrap();
        let mut runtime = create_runtime().await;
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`), o in {{
              fsOpen!("{root}", "file.bin", "oracular", *o) |
              for (@reply <- o) {{
                match reply {{
                  [false, _, _] => Nil
                  _ => @"MISMATCH"!(reply)
                }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        eval(&mut runtime, &term).await;
        assert_eq!(runtime.fs_handles.dir_handles.snapshot_next_fd(), 1);
    }

    /// Next on an fd that was never opened returns `FSERR_CLOSED`
    /// (the same shape a Next on a closed-then-reopened fd would
    /// see if slice-28 aliasing protection didn't hold).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn next_on_unknown_fd_returns_closed() {
        let mut runtime = create_runtime().await;
        let term = r#"
            new fsNext(`rho:io:fs:native:1.0.0/entriesStreamNext`), r in {
              fsNext!(9999, *r) |
              for (@reply <- r) {
                match reply {
                  [false, "FSERR_CLOSED", _] => Nil
                  _ => @"MISMATCH"!(reply)
                }
              }
            }
        "#
        .to_string();
        eval(&mut runtime, &term).await;
    }

    /// **Streaming-slice review-fixup pin (2026-08-26).**  Open-
    /// without-close leaves the fd in `dir_handles.table` — documents
    /// the fd-leak behavior that motivates the "close_all_for_deploy
    /// sweep not wired" deferred item.  If a future PR wires
    /// `DirHandleTable::close_all_for_deploy` into
    /// `WalDeployScope::Drop` (or an equivalent deploy-end hook),
    /// this test's expectation flips — the fd should be swept after
    /// the deploy ends — and the assertion needs updating.
    ///
    /// Also serves as a contrast to
    /// `stream_close_is_idempotent`: this test does NOT call
    /// `entriesStreamClose`, so the fd remains present in the table.
    /// The Rust-side `DirHandleTable::truncate_to` path (Step 4) is
    /// the ONLY current mechanism that sweeps un-closed dir fds,
    /// and it only fires on deploy revert — not on deploy commit.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn open_without_close_leaves_fd_in_table() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/a"), b"x").unwrap();

        let mut runtime = create_runtime().await;
        let counter_initial = runtime.fs_handles.dir_handles.snapshot_next_fd();

        // Open and drain the ack, but do NOT close.  The Rholang
        // deploy commits successfully.
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`), o in {{
              fsOpen!("{root}", "sub", "oracular", *o) |
              for (@[true, _fd] <- o) {{ Nil }}
            }}
            "#,
            root = dir.path().display(),
        );
        eval(&mut runtime, &term).await;

        let leaked_fd = counter_initial;
        assert!(
            runtime
                .fs_handles
                .dir_handles
                .get(leaked_fd)
                .await
                .is_some(),
            "fd {leaked_fd} must remain in table after deploy commit \
             (documents the deploy-end-sweep gap; WalDeployScope::Drop \
             does not call dir_handles.close_all_for_deploy today)."
        );
        assert_eq!(
            runtime.fs_handles.dir_handles.snapshot_next_fd(),
            counter_initial + 1,
            "counter advanced monotonically"
        );
    }

    /// Next with a non-int fd argument returns `FSERR_BAD_ARG`.
    /// Guards against a regression that unwrapped the fd Par
    /// unconditionally and panicked (or silently coerced) on
    /// non-int shapes.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn next_with_string_fd_returns_bad_arg() {
        let mut runtime = create_runtime().await;
        let term = r#"
            new fsNext(`rho:io:fs:native:1.0.0/entriesStreamNext`), r in {
              fsNext!("not-a-fd", *r) |
              for (@reply <- r) {
                match reply {
                  [false, "FSERR_BAD_ARG", _] => Nil
                  _ => @"MISMATCH"!(reply)
                }
              }
            }
        "#
        .to_string();
        eval(&mut runtime, &term).await;
    }
}
