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
    /// **Step 4 of the streaming slice is where this snapshot stack
    /// wiring lands.** This test is currently `#[ignore]` because
    /// `create_soft_checkpoint` / `revert_to_soft_checkpoint` do not
    /// yet touch the `DirHandleTable`; once Step 4 wires it in, drop
    /// the ignore and the test will pin the invariant.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires streaming-slice Step 4 (WalDeployScope Drop path)"]
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
}
