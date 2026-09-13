//! Slice X-5 / D-03 (2026-09-12, branch-review-2026-09-11.md
//! Track D): boundary tests for `MAX_READ_BYTES`.
//!
//! # What this pins
//!
//! `MAX_READ_BYTES` is 64 MiB (2026-07-24-File-IO §Per-call caps)
//! and is consensus-observable (folded into the fingerprint via
//! CONSENSUS_FOLD order 3).  Existing coverage samples at 0 / 1024
//! / 4096 (fileio_cost_spec) but no test exercises the boundary at
//! MAX or MAX+1.
//!
//! Regressions this catches:
//! - A refactor that changed the constant from 64 MiB to some other
//!   value would pass every happy-path test but fail this spec.
//! - A refactor that flipped the `> MAX` predicate to `>= MAX`
//!   would silently reject at-cap reads.
//!
//! # Where write-side lives
//!
//! Write-side at-max / over-max tests moved to
//! `fileio_bounds_large_spec.rs` in slice X-8 (2026-09-13, D-new-1)
//! because they each allocate ~128 MiB peak and were previously
//! `#[ignore]`-gated — CI doesn't invoke `cargo test --ignored`, so
//! the gate meant zero coverage.  The dedicated large-spec binary
//! runs by default under nextest.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

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

    /// 64 MiB — spec §Per-call caps, MAX_READ_BYTES.
    const MAX_BYTES: usize = 64 * 1024 * 1024;

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

    /// Read the reply Par from a channel.
    async fn read_out(runtime: &RhoRuntimeImpl, name: &str) -> Par {
        let map = runtime.get_hot_changes().await;
        let key = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString(name.to_string())),
        }]);
        let row = map.get(&vec![key]).unwrap_or_else(|| {
            panic!(
                "no reply on @{name:?}.  Tuplespace keys: {:?}",
                map.keys().collect::<Vec<_>>()
            )
        });
        row.data[0].a.pars[0].clone()
    }

    /// Extract head-bool from a `[ok, ...]` reply.
    fn head_bool(reply: &Par) -> bool {
        let list = match reply.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
            Some(ExprInstance::EListBody(l)) => l,
            other => panic!("expected EList reply, got {other:?}"),
        };
        match list.ps[0]
            .exprs
            .first()
            .and_then(|e| e.expr_instance.as_ref())
        {
            Some(ExprInstance::GBool(b)) => *b,
            other => panic!("expected GBool head; got {other:?}"),
        }
    }

    /// Extract error code from `[false, code, msg]`.
    fn err_code(reply: &Par) -> String {
        let list = match reply.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
            Some(ExprInstance::EListBody(l)) => l,
            other => panic!("expected EList reply, got {other:?}"),
        };
        match list.ps[1]
            .exprs
            .first()
            .and_then(|e| e.expr_instance.as_ref())
        {
            Some(ExprInstance::GString(s)) => s.clone(),
            other => panic!("expected GString code; got {other:?}"),
        }
    }

    /// D-03 read-side at-max: fs_read with n = MAX_READ_BYTES on a
    /// small file MUST succeed (returns whatever bytes are
    /// available; the cap governs the requested count, not the
    /// available count).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn d03_fs_read_at_max_bytes_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"hi").unwrap();
        let runtime = create_runtime().await;

        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsRead(`rho:io:fs:native:1.0.0/read`),
                oc, rr in {{
              fsOpen!("{root}", "f.bin", "r+", "oracular", *oc) |
              for (@[true, fd] <- oc) {{
                fsRead!(fd, {max}, *rr) |
                for (@r <- rr) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
            max = MAX_BYTES,
        );
        runtime
            .evaluate(
                &term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .expect("evaluate at-max read");

        let reply = read_out(&runtime, "out").await;
        assert!(
            head_bool(&reply),
            "D-03 regression: fs_read with n = MAX_READ_BYTES on a small \
             file MUST succeed (cap is on requested count, not available \
             count)"
        );
    }

    /// D-03 read-side over-max: fs_read with n = MAX_READ_BYTES + 1
    /// MUST reject with FSERR_QUOTA_EXCEEDED at request time
    /// (before any actual read).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn d03_fs_read_over_max_bytes_rejects() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"hi").unwrap();
        let runtime = create_runtime().await;

        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsRead(`rho:io:fs:native:1.0.0/read`),
                oc, rr in {{
              fsOpen!("{root}", "f.bin", "r+", "oracular", *oc) |
              for (@[true, fd] <- oc) {{
                fsRead!(fd, {over_max}, *rr) |
                for (@r <- rr) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
            over_max = MAX_BYTES + 1,
        );
        runtime
            .evaluate(
                &term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .expect("evaluate over-max read");

        let reply = read_out(&runtime, "out").await;
        assert!(
            !head_bool(&reply),
            "D-03 regression: fs_read with n = MAX_READ_BYTES + 1 MUST reject"
        );
        assert_eq!(
            err_code(&reply),
            "FSERR_QUOTA_EXCEEDED",
            "D-03 regression: cap-exceeded read must produce FSERR_QUOTA_EXCEEDED"
        );
    }
}
