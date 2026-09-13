//! Slice X-5 / D-02 (2026-09-12, branch-review-2026-09-11.md
//! Track D): handler-boundary integration tests for the quarantine
//! (path-safety) surface.
//!
//! # What this pins
//!
//! `path.rs` has 24 unit tests covering the internals of
//! `safe_descend_verified` — TOCTOU-safe openat chain, symlink
//! detection, root-identity checks.  Those are strong, but they
//! test the helper in isolation.  This spec pins that the helper
//! is actually WIRED into the syscall handlers: a refactor that
//! moved canonicalization earlier or bypassed `safe_descend_verified`
//! in a specific handler would pass every `path.rs` unit test
//! while silently opening a quarantine bypass at the handler
//! boundary.
//!
//! # Scenarios
//!
//! 1. `fs_open_with_parent_traversal_returns_quarantine_error` —
//!    `..` in rel component; `safe_descend_verified` rejects with
//!    `EscapesRoot` → `FSERR_QUARANTINE`.
//! 2. `fs_open_with_symlink_component_returns_quarantine_error` —
//!    a symlinked directory in the path chain; the O_NOFOLLOW
//!    guard fires with `SymlinkComponent` → `FSERR_QUARANTINE`.
//! 3. `consensus_fs_open_with_path_escape_returns_quarantine` —
//!    same escape on a Consensus-mode cap; the leader's reply
//!    must be FSERR_QUARANTINE (a regression that let path-escape
//!    slip past on Consensus mode would silently escalate the
//!    quarantine bypass to a Consensus-shared surface).

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

    /// Evaluate a term and read the reply Par produced on `@"out"`.
    async fn eval_and_read_out(runtime: &RhoRuntimeImpl, term: &str) -> Par {
        runtime
            .evaluate(
                term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .expect("evaluate");
        let map = runtime.get_hot_changes().await;
        let key = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString("out".to_string())),
        }]);
        let row = map.get(&vec![key]).unwrap_or_else(|| {
            panic!(
                "no reply on @\"out\".  Tuplespace keys: {:?}",
                map.keys().collect::<Vec<_>>()
            )
        });
        row.data[0].a.pars[0].clone()
    }

    /// Extract the `[false, code, msg]` shape from a reply.  Panics
    /// if the reply is a success shape or malformed.
    fn extract_err_code(reply: &Par) -> String {
        let list = match reply.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
            Some(ExprInstance::EListBody(l)) => l,
            other => panic!("expected EList reply, got {other:?}"),
        };
        let head = list.ps[0]
            .exprs
            .first()
            .and_then(|e| e.expr_instance.as_ref());
        match head {
            Some(ExprInstance::GBool(false)) => {}
            other => panic!("expected [false, code, msg] error reply; got head {other:?}"),
        }
        let code = list
            .ps
            .get(1)
            .and_then(|p| p.exprs.first())
            .and_then(|e| e.expr_instance.as_ref());
        match code {
            Some(ExprInstance::GString(s)) => s.clone(),
            other => panic!("expected GString error code; got {other:?}"),
        }
    }

    /// D-02 scenario 1: fs_open with `..` in the rel path.
    /// `safe_descend_verified` rejects with `EscapesRoot`;
    /// `quarantine_err_reply` maps that to
    /// `[false, "FSERR_QUARANTINE", "path escapes root"]`.
    ///
    /// A regression that removed `safe_descend_verified` from
    /// `open_impl_via_table` (or moved canonicalization to a
    /// lexical-only path that doesn't reject `..`) would surface
    /// here as either a `[true, fd]` (bypass) or a non-QUARANTINE
    /// error code.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_open_with_parent_traversal_returns_quarantine_error() {
        let dir = tempfile::tempdir().unwrap();
        // Sibling directory outside the tempdir root — the attacker
        // target.
        let sibling = tempfile::tempdir().unwrap();
        std::fs::write(sibling.path().join("secret.bin"), b"attacker's target")
            .expect("seed sibling");
        let runtime = create_runtime().await;

        // rel = "../<sibling_name>/secret.bin" — a classic
        // parent-traversal attempt.  Even if the on-disk path exists
        // (which it does, since we seeded it), safe_descend_verified
        // MUST reject at component parse.
        let rel = format!(
            "../{}/secret.bin",
            sibling.path().file_name().unwrap().to_str().unwrap()
        );
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/open`), ret in {{
              op!("{root}", "{rel}", "r", "oracular", *ret) |
              for (@r <- ret) {{ @"out"!(r) }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        let code = extract_err_code(&reply);
        assert_eq!(
            code, "FSERR_QUARANTINE",
            "D-02 regression: parent-traversal path MUST reject with \
             FSERR_QUARANTINE (via safe_descend_verified → EscapesRoot).  \
             A different code suggests safe_descend_verified was bypassed \
             or canonicalization was moved earlier."
        );
    }

    /// D-02 scenario 2: fs_open through a symlinked directory
    /// component.  Even if the symlink points inside the root,
    /// `safe_descend_verified` rejects at the O_NOFOLLOW openat.
    ///
    /// A regression that dropped O_NOFOLLOW from the descent chain
    /// (Slice 30c H-P7-6) would surface here: the open would
    /// succeed on the symlink target, opening a post-boot symlink-
    /// swap attack surface.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_open_with_symlink_component_returns_quarantine_error() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        // Create a real subdir + a file inside it.
        std::fs::create_dir(dir.path().join("real")).unwrap();
        std::fs::write(dir.path().join("real/target.bin"), b"content").unwrap();
        // Symlink `link` → `real` inside the same root.  Following
        // the symlink lands at a real file inside the root, so a
        // permissive descent would succeed.  O_NOFOLLOW rejects at
        // the symlink component.
        symlink("real", dir.path().join("link")).expect("create symlink");
        let runtime = create_runtime().await;

        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/open`), ret in {{
              op!("{root}", "link/target.bin", "r", "oracular", *ret) |
              for (@r <- ret) {{ @"out"!(r) }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        let code = extract_err_code(&reply);
        assert_eq!(
            code, "FSERR_QUARANTINE",
            "D-02 regression: symlink-in-component MUST reject with \
             FSERR_QUARANTINE (via safe_descend_verified → \
             SymlinkComponent from the O_NOFOLLOW openat).  A different \
             code (or [true, fd] success) suggests O_NOFOLLOW was \
             dropped from the descent chain (Slice 30c H-P7-6 regression)."
        );
    }

    /// D-02 scenario 3: same escape but on a Consensus-mode cap.
    /// Since X-6c M-04 landed (2026-09-12), Consensus-mode dispatch
    /// with an unregistered logical root refuses UPFRONT with
    /// FSERR_UNSUPPORTED — the M-04 guard fires before
    /// safe_descend_verified is even entered.  This is strictly
    /// stronger than the pre-M-04 FSERR_QUARANTINE outcome: instead
    /// of doing path-safety work then refusing the escape, we refuse
    /// the entire dispatch because the tempdir root wasn't
    /// registered in the RootIdentityRegistry at boot.
    ///
    /// Real production Consensus caps carry `/@bundle/<X>` roots
    /// which ARE boot-registered — this test path (tempdir root)
    /// simulates the failure mode M-04 is designed to catch: a
    /// Consensus cap on an unregistered logical root.  Either
    /// FSERR_QUARANTINE (path-safety failure at descend) or
    /// FSERR_UNSUPPORTED (M-04 refusal) is a valid safety outcome;
    /// M-04 makes the refusal earlier + more explicit.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn consensus_fs_open_with_path_escape_returns_quarantine_or_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let sibling = tempfile::tempdir().unwrap();
        std::fs::write(sibling.path().join("secret.bin"), b"attacker's target")
            .expect("seed sibling");
        let runtime = create_runtime().await;

        let rel = format!(
            "../{}/secret.bin",
            sibling.path().file_name().unwrap().to_str().unwrap()
        );
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/open`), ret in {{
              op!("{root}", "{rel}", "r", "consensus", *ret) |
              for (@r <- ret) {{ @"out"!(r) }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        let code = extract_err_code(&reply);
        assert!(
            code == "FSERR_QUARANTINE" || code == "FSERR_UNSUPPORTED",
            "D-02 / X-6c M-04 regression: Consensus-mode parent-\
             traversal MUST reject with FSERR_QUARANTINE (from \
             safe_descend_verified) or FSERR_UNSUPPORTED (from the \
             M-04 unregistered-root guard).  A different code suggests \
             both the M-04 guard AND the quarantine gate were \
             bypassed.  Got: {code}"
        );
    }
}
