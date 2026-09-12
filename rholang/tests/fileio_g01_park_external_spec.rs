//! Slice X-2 / G-01 (2026-09-11, branch-review-2026-09-11.md):
//! coverage pin for the `park_external_during` wrapper on fileio
//! `spawn_blocking` sites.
//!
//! # What this pins
//!
//! Post-X-2, every fileio handler wraps its `spawn_blocking(...).await`
//! in `park_external_during` so the reduction driver's
//! `frontier_ready` check can advance other participants while the
//! syscall runs on the blocking pool.
//!
//! # What this does NOT pin
//!
//! An adversarial multi-participant DEADLOCK reproducer.  For
//! non-lock ops like `fs_open` on a local filesystem, the
//! `spawn_blocking` body completes independently of the reduction
//! driver, so the pre-X-2 shape produced throughput starvation
//! rather than strict deadlock.  The strict-deadlock scenario is
//! `fs_lock_range(wait:true)` where the admit oneshot IS driver-
//! dependent — S4.8 already fixed that specific case (commit
//! `0f7bfb089` covered by `fileio_lockrange_wait_true_admit_after_
//! release` in `casper/tests/genesis/contracts/fileio_examples_
//! spec.rs`).
//!
//! X-2 extends the same discipline to all fileio `spawn_blocking`
//! sites so future refactors that put driver-dependent work inside
//! a blocking closure (e.g., a syscall that awaits an intra-node
//! signal) cannot silently reintroduce the deadlock class.
//!
//! # Test shape
//!
//! Each test runs a Rholang term with two parallel branches:
//! - Branch A: invokes a fileio syscall that goes through
//!   `spawn_blocking` (fs_open / fs_read / fs_stat / etc.).
//! - Branch B: emits on a channel that the main flow consumes.
//!
//! Both branches complete + the main flow observes both branch
//! completions within a bounded time budget.  Under the X-2 fix,
//! the reduction driver services Branch B while Branch A is in
//! its blocking closure.  Regression that removes
//! `park_external_during` would either time out (if the driver
//! stalls indefinitely) or complete only via scheduling luck.

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{BindPattern, Expr, ListParWithRandom, Par, TaggedContinuation};
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
        runtime.disable_fs_native_urn_filter();
        runtime
    }

    /// Assert `@"out"` carries a Par whose value is a list whose head
    /// element is `GBool(true)`.
    async fn assert_out_true(runtime: &RhoRuntimeImpl, expected_key: &str) {
        let map = runtime.get_hot_changes().await;
        let key_par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString(expected_key.to_string())),
        }]);
        let row = map.get(&vec![key_par]).unwrap_or_else(|| {
            panic!(
                "no reply on @{expected_key:?}.  Tuplespace keys: {:?}",
                map.keys().collect::<Vec<_>>()
            )
        });
        let outer = row.data[0].a.pars[0]
            .exprs
            .first()
            .and_then(|e| e.expr_instance.as_ref());
        let list = match outer {
            Some(ExprInstance::EListBody(l)) => l,
            other => panic!("expected EList on @{expected_key:?}; got {other:?}"),
        };
        let head = list
            .ps
            .first()
            .and_then(|p| p.exprs.first())
            .and_then(|e| e.expr_instance.as_ref());
        match head {
            Some(ExprInstance::GBool(true)) => {}
            other => panic!("expected [true, ...] on @{expected_key:?}; got head {other:?}"),
        }
    }

    /// X-2 / G-01 (2026-09-11): two parallel branches — Branch A
    /// invokes `fs_open` (routes through the X-2-wrapped
    /// `spawn_blocking`) and Branch B emits on a channel the main
    /// flow observes.  Under the X-2 fix, the reduction driver can
    /// service Branch B while Branch A's `openat` runs on the
    /// blocking pool; both complete within a bounded time budget.
    ///
    /// Regression that removes `park_external_during` from the
    /// wrapper would either time out (if the driver stalls) or
    /// complete only via scheduling luck.  A wall-clock timeout
    /// bounds the run so a stalled driver surfaces as a test
    /// failure rather than a hung run.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn parallel_fs_open_and_rspace_consume_both_complete_bounded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bounded.bin");
        std::fs::write(&path, vec![0u8; 64]).expect("seed file");

        let runtime = create_runtime().await;

        // Two parallel branches:
        //   - Branch A: fs_open (goes through spawn_blocking).
        //   - Branch B: emits on `syncCh`.
        // Main flow: consumes both `openReply` and `syncCh`; only then
        // writes to @"out".  Both consumes must succeed for @"out" to
        // be populated.
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                openReply, syncCh
            in {{
              fsOpen!("{root}", "bounded.bin", "r+", "oracular", *openReply) |
              syncCh!("done") |
              for (@[true, _fd] <- openReply; @_ <- syncCh) {{
                @"out"!([true, "both branches completed"])
              }}
            }}
            "#,
            root = dir.path().display(),
        );

        let start = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            runtime.evaluate(
                &term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            ),
        )
        .await
        .expect(
            "X-2 / G-01 regression: parallel fs_open + RSpace consume \
             MUST complete within 5s.  A stall here suggests \
             `spawn_blocking` sites lost their `park_external_during` \
             wrapper — the reduction driver's `frontier_ready` sees \
             the fs_open participant as `Running` and refuses to \
             service the sibling consume until fs_open's blocking \
             task completes.",
        );
        result.expect("evaluate must not error");
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "parallel fs_open + consume ran for {elapsed:?} — timeout \
             boundary hit"
        );
        assert_out_true(&runtime, "out").await;
    }

    /// Helper: open a file (synchronous within the test), returning
    /// the fd for use in subsequent parallel-branch tests.
    async fn open_fd(runtime: &RhoRuntimeImpl, root: &std::path::Path, rel: &str) -> i64 {
        let open_term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`), openCh in {{
              fsOpen!("{root}", "{rel}", "r+", "oracular", *openCh) |
              for (@[true, fd] <- openCh) {{
                @"__fd"!(fd)
              }}
            }}
            "#,
            root = root.display(),
        );
        runtime
            .evaluate(
                &open_term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .expect("open must succeed");
        let map = runtime.get_hot_changes().await;
        let fd_key = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString("__fd".to_string())),
        }]);
        let fd_row = map.get(&vec![fd_key]).expect("fd published on @\"__fd\"");
        let fd_par = fd_row.data[0].a.pars[0].clone();
        match fd_par.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
            Some(ExprInstance::GInt(n)) => *n,
            other => panic!("expected GInt fd; got {other:?}"),
        }
    }

    /// Helper: run a Rholang term under a wall-clock timeout.  A
    /// timeout expiry indicates a `park_external_during` regression
    /// on one of the participating sites — the reduction driver's
    /// `frontier_ready` check saw the fileio participant as
    /// `Running` and refused to service the sibling consume.
    async fn evaluate_bounded(runtime: &RhoRuntimeImpl, term: &str, family: &str) {
        let start = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            runtime.evaluate(
                term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            ),
        )
        .await
        .unwrap_or_else(|_| {
            panic!(
                "X-2 / G-01 regression on the {family} family: \
                 parallel fs syscall + RSpace consume MUST complete \
                 within 5s.  A stall here suggests the {family} \
                 handler's `spawn_blocking` site lost its \
                 `park_external_during` wrapper — the reduction \
                 driver's `frontier_ready` sees the fileio \
                 participant as `Running` and refuses to service \
                 the sibling consume until the blocking task \
                 completes."
            )
        });
        result.unwrap_or_else(|e| panic!("evaluate must not error on {family}: {e}"));
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "parallel fs {family} + consume ran for {elapsed:?} — \
             timeout boundary hit"
        );
    }

    /// X-2 / G-01 companion: same shape but exercises the fs_size
    /// (fstat) path.  A per-syscall pin ensures every wrapped site
    /// gets exercise independently — a regression that misses one
    /// site would leave that specific handler's test failing while
    /// the fs_open pin passes.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn parallel_fs_size_and_rspace_consume_both_complete_bounded() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("bounded_size.bin"), vec![0u8; 128]).expect("seed file");
        let runtime = create_runtime().await;
        let fd = open_fd(&runtime, dir.path(), "bounded_size.bin").await;

        let term = format!(
            r#"
            new fsSize(`rho:io:fs:native:1.0.0/size`),
                sizeReply, syncCh
            in {{
              fsSize!({fd}, *sizeReply) |
              syncCh!("done") |
              for (@[true, _n] <- sizeReply; @_ <- syncCh) {{
                @"out_size"!([true, "both branches completed"])
              }}
            }}
            "#,
        );
        evaluate_bounded(&runtime, &term, "observation/fs_size").await;
        assert_out_true(&runtime, "out_size").await;
    }

    /// X-2 / G-01 — mutation family (fs_write).  Exercises the
    /// `spawn_blocking` site inside the write path.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn parallel_fs_write_and_rspace_consume_both_complete_bounded() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("write.bin"), vec![0u8; 64]).expect("seed file");
        let runtime = create_runtime().await;
        let fd = open_fd(&runtime, dir.path(), "write.bin").await;

        let term = format!(
            r#"
            new fsWrite(`rho:io:fs:native:1.0.0/write`),
                writeReply, syncCh
            in {{
              fsWrite!({fd}, "aabb".hexToBytes(), *writeReply) |
              syncCh!("done") |
              for (@[true, _n] <- writeReply; @_ <- syncCh) {{
                @"out_write"!([true, "both branches completed"])
              }}
            }}
            "#,
        );
        evaluate_bounded(&runtime, &term, "mutation/fs_write").await;
        assert_out_true(&runtime, "out_write").await;
    }

    /// X-2 / G-01 — mutation family (fs_truncate).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn parallel_fs_truncate_and_rspace_consume_both_complete_bounded() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("trunc.bin"), vec![0u8; 128]).expect("seed file");
        let runtime = create_runtime().await;
        let fd = open_fd(&runtime, dir.path(), "trunc.bin").await;

        let term = format!(
            r#"
            new fsTruncate(`rho:io:fs:native:1.0.0/truncate`),
                truncReply, syncCh
            in {{
              fsTruncate!({fd}, 32, *truncReply) |
              syncCh!("done") |
              for (@_ <- truncReply; @_ <- syncCh) {{
                @"out_trunc"!([true, "both branches completed"])
              }}
            }}
            "#,
        );
        evaluate_bounded(&runtime, &term, "mutation/fs_truncate").await;
        assert_out_true(&runtime, "out_trunc").await;
    }

    /// X-2 / G-01 — mutation family (fs_flush).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn parallel_fs_flush_and_rspace_consume_both_complete_bounded() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("flush.bin"), vec![0u8; 32]).expect("seed file");
        let runtime = create_runtime().await;
        let fd = open_fd(&runtime, dir.path(), "flush.bin").await;

        let term = format!(
            r#"
            new fsFlush(`rho:io:fs:native:1.0.0/flush`),
                flushReply, syncCh
            in {{
              fsFlush!({fd}, *flushReply) |
              syncCh!("done") |
              for (@_ <- flushReply; @_ <- syncCh) {{
                @"out_flush"!([true, "both branches completed"])
              }}
            }}
            "#,
        );
        evaluate_bounded(&runtime, &term, "mutation/fs_flush").await;
        assert_out_true(&runtime, "out_flush").await;
    }

    /// X-2 / G-01 — observation family (fs_read).  The read path
    /// runs through spawn_blocking to service the syscall.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn parallel_fs_read_and_rspace_consume_both_complete_bounded() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("read.bin"), vec![7u8; 32]).expect("seed file");
        let runtime = create_runtime().await;
        let fd = open_fd(&runtime, dir.path(), "read.bin").await;

        let term = format!(
            r#"
            new fsRead(`rho:io:fs:native:1.0.0/read`),
                readReply, syncCh
            in {{
              fsRead!({fd}, 8, *readReply) |
              syncCh!("done") |
              for (@[true, _bytes] <- readReply; @_ <- syncCh) {{
                @"out_read"!([true, "both branches completed"])
              }}
            }}
            "#,
        );
        evaluate_bounded(&runtime, &term, "observation/fs_read").await;
        assert_out_true(&runtime, "out_read").await;
    }

    /// X-2 / G-01 — observation family (fs_stat, path-based).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn parallel_fs_stat_and_rspace_consume_both_complete_bounded() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("stat.bin"), b"hello").expect("seed file");
        let runtime = create_runtime().await;

        let term = format!(
            r#"
            new fsStat(`rho:io:fs:native:1.0.0/stat`),
                statReply, syncCh
            in {{
              fsStat!("{root}", "stat.bin", "oracular", *statReply) |
              syncCh!("done") |
              for (@[true, _rec] <- statReply; @_ <- syncCh) {{
                @"out_stat"!([true, "both branches completed"])
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        evaluate_bounded(&runtime, &term, "observation/fs_stat").await;
        assert_out_true(&runtime, "out_stat").await;
    }

    /// X-2 / G-01 — mutation family (fs_chmod, path-based).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn parallel_fs_chmod_and_rspace_consume_both_complete_bounded() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("chmod.bin"), b"").expect("seed file");
        let runtime = create_runtime().await;

        let term = format!(
            r#"
            new fsChmod(`rho:io:fs:native:1.0.0/chmod`),
                chmodReply, syncCh
            in {{
              fsChmod!("{root}", "chmod.bin", 420, "oracular", *chmodReply) |
              syncCh!("done") |
              for (@_ <- chmodReply; @_ <- syncCh) {{
                @"out_chmod"!([true, "both branches completed"])
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        evaluate_bounded(&runtime, &term, "mutation/fs_chmod").await;
        assert_out_true(&runtime, "out_chmod").await;
    }

    /// X-2 / G-01 — stream family (fs_entries_stream_open).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn parallel_fs_entries_stream_open_and_rspace_consume_both_complete_bounded() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("sub")).expect("mkdir");
        let runtime = create_runtime().await;

        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`),
                openReply, syncCh
            in {{
              fsOpen!("{root}", "sub", "oracular", *openReply) |
              syncCh!("done") |
              for (@[true, _fd] <- openReply; @_ <- syncCh) {{
                @"out_stream_open"!([true, "both branches completed"])
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        evaluate_bounded(&runtime, &term, "stream/entriesStreamOpen").await;
        assert_out_true(&runtime, "out_stream_open").await;
    }

    /// X-2 / G-01 — lifecycle family (fs_close).  Close routes
    /// through spawn_blocking for the underlying `close(2)`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn parallel_fs_close_and_rspace_consume_both_complete_bounded() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("close.bin"), vec![0u8; 16]).expect("seed file");
        let runtime = create_runtime().await;
        let fd = open_fd(&runtime, dir.path(), "close.bin").await;

        let term = format!(
            r#"
            new fsClose(`rho:io:fs:native:1.0.0/close`),
                closeReply, syncCh
            in {{
              fsClose!({fd}, *closeReply) |
              syncCh!("done") |
              for (@_ <- closeReply; @_ <- syncCh) {{
                @"out_close"!([true, "both branches completed"])
              }}
            }}
            "#,
        );
        evaluate_bounded(&runtime, &term, "lifecycle/fs_close").await;
        assert_out_true(&runtime, "out_close").await;
    }
}
