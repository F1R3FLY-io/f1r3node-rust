//! Slice X-5 / D-04 (2026-09-12, branch-review-2026-09-11.md
//! Track D): edge case tests — zero-byte writes + extreme offsets.
//!
//! # Coverage gap this closes
//!
//! Existing tests cover happy-path counts (fs_write 4, 64, 1024,
//! 4096 bytes) but skip the interesting edge cases:
//! - `fs_write(fd, b"", ...)` — semantics were unclear (noop vs
//!   error).  This pins them.
//! - `fs_write_at(off, ...)` at `i64::MAX` — the raw Rholang wire
//!   is a signed i64 that bit-preserve-reinterprets to u64 at the
//!   handler boundary.  A regression on the reject-negative path
//!   would let a wrapped-to-u64 huge offset land silently.
//! - `fs_seek(off, "set")` at `i64::MAX` — parse_content routes
//!   this through the whence-specific off ≥ 0 gate.
//! - Negative offset on fs_write_at — reject-negative pin.
//!
//! Each test pins reply-shape consistency: either `[true, n]` or
//! `[false, code, msg]`.  A regression that produced a malformed
//! reply (empty list, wrong-type head, etc.) surfaces regardless
//! of the exact numeric result.

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

    /// Assert the reply is a well-formed EList of length ≥ 2 with a
    /// GBool head — pins the shape invariant regardless of the
    /// specific ok/err discriminator.
    fn assert_well_formed_reply(reply: &Par) {
        let list = match reply.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
            Some(ExprInstance::EListBody(l)) => l,
            other => panic!("expected EList reply; got {other:?}"),
        };
        assert!(!list.ps.is_empty(), "reply list must be non-empty");
        match list.ps[0]
            .exprs
            .first()
            .and_then(|e| e.expr_instance.as_ref())
        {
            Some(ExprInstance::GBool(_)) => {}
            other => panic!("reply head must be GBool; got {other:?}"),
        }
    }

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

    fn ok_int(reply: &Par) -> i64 {
        let list = match reply.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
            Some(ExprInstance::EListBody(l)) => l,
            other => panic!("expected EList reply, got {other:?}"),
        };
        match list.ps[1]
            .exprs
            .first()
            .and_then(|e| e.expr_instance.as_ref())
        {
            Some(ExprInstance::GInt(n)) => *n,
            other => panic!("expected GInt quantity; got {other:?}"),
        }
    }

    /// D-04 edge case 1: fs_write with a zero-byte payload MUST
    /// succeed with `[true, 0]` — a no-op write.  POSIX libc::write
    /// with n=0 is unspecified (may return 0 or error); the handler
    /// must present a consistent Rholang surface regardless.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn d04_fs_write_zero_bytes_is_noop_returning_ok_zero() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"seed").unwrap();
        let runtime = create_runtime().await;

        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWrite(`rho:io:fs:native:1.0.0/write`),
                oc, wr in {{
              fsOpen!("{root}", "f.bin", "r+", "oracular", *oc) |
              for (@[true, fd] <- oc) {{
                fsWrite!(fd, "".hexToBytes(), *wr) |
                for (@r <- wr) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_well_formed_reply(&reply);
        assert!(
            head_bool(&reply),
            "D-04 regression: zero-byte fs_write must succeed with [true, 0].  \
             Got: {reply:?}"
        );
        assert_eq!(
            ok_int(&reply),
            0,
            "D-04 regression: zero-byte fs_write must report 0 bytes written"
        );
    }

    /// D-04 edge case 2: fs_write_at at i64::MAX offset MUST produce
    /// a well-formed reply — either succeed (sparse-file semantics,
    /// filesystem-dependent) or reject cleanly.  What must NOT happen:
    /// panic, malformed reply, or wrap-to-negative confusion.
    ///
    /// The exact outcome depends on filesystem: ext4/apfs typically
    /// return EFBIG or ENOSPC via pwrite for offsets that exceed
    /// the filesystem's max file size.  We DON'T pin the specific
    /// error code — we pin the shape invariant.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn d04_fs_write_at_i64_max_offset_is_well_formed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"seed").unwrap();
        let runtime = create_runtime().await;

        // 9223372036854775807 = i64::MAX — the maximum Rholang GInt
        // can carry into the u64 offset slot.
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWriteAt(`rho:io:fs:native:1.0.0/writeAt`),
                oc, wr in {{
              fsOpen!("{root}", "f.bin", "r+", "oracular", *oc) |
              for (@[true, fd] <- oc) {{
                fsWriteAt!(fd, 9223372036854775807, "aa".hexToBytes(), *wr) |
                for (@r <- wr) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        // Shape invariant is the load-bearing pin.  A regression that
        // panicked on the offset cast (or returned an empty reply)
        // would surface here.
        assert_well_formed_reply(&reply);
        // Diagnostic: if OK, log the byte count; if error, log the code.
        if head_bool(&reply) {
            eprintln!(
                "d04_fs_write_at_i64_max_offset: wrote {} bytes (sparse-file \
                 semantics accepted by filesystem)",
                ok_int(&reply)
            );
        } else {
            eprintln!(
                "d04_fs_write_at_i64_max_offset: rejected with code {}",
                err_code(&reply)
            );
        }
    }

    /// D-04 edge case 3: fs_seek to `i64::MAX` with "set" whence
    /// (the only whence that gates `off >= 0`).  Must produce a
    /// well-formed reply — libc::lseek may accept the seek even
    /// past EOF (POSIX allows this) or reject with EOVERFLOW /
    /// EFBIG depending on filesystem.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn d04_fs_seek_i64_max_offset_is_well_formed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"seed").unwrap();
        let runtime = create_runtime().await;

        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsSeek(`rho:io:fs:native:1.0.0/seek`),
                oc, sk in {{
              fsOpen!("{root}", "f.bin", "r+", "oracular", *oc) |
              for (@[true, fd] <- oc) {{
                fsSeek!(fd, 9223372036854775807, "set", *sk) |
                for (@r <- sk) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_well_formed_reply(&reply);
        if head_bool(&reply) {
            eprintln!(
                "d04_fs_seek_i64_max: seek reported pos {} (POSIX past-EOF \
                 accepted)",
                ok_int(&reply)
            );
        } else {
            eprintln!(
                "d04_fs_seek_i64_max: rejected with code {}",
                err_code(&reply)
            );
        }
    }

    /// D-04 edge case 4: fs_write_at with a negative offset MUST
    /// reject with FSERR_BAD_ARG at parse time.  The offset slot is
    /// declared u64 at the handler boundary; the parse gate
    /// (`off >= 0`) is what enforces this discipline against a
    /// Rholang caller that supplies a negative GInt.
    ///
    /// A regression that dropped the `off >= 0` guard (or moved
    /// it after the reinterpret cast) would silently wrap the
    /// negative i64 to a huge u64 — landing at a sparse-file
    /// offset near u64::MAX.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn d04_fs_write_at_negative_offset_rejects_with_bad_arg() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"seed").unwrap();
        let runtime = create_runtime().await;

        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`),
                fsWriteAt(`rho:io:fs:native:1.0.0/writeAt`),
                oc, wr in {{
              fsOpen!("{root}", "f.bin", "r+", "oracular", *oc) |
              for (@[true, fd] <- oc) {{
                fsWriteAt!(fd, -1, "aa".hexToBytes(), *wr) |
                for (@r <- wr) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_well_formed_reply(&reply);
        assert!(
            !head_bool(&reply),
            "D-04 regression: negative offset MUST reject; a passing \
             reply here indicates the `off >= 0` guard was dropped and \
             the i64 wrapped to a huge u64."
        );
        assert_eq!(
            err_code(&reply),
            "FSERR_BAD_ARG",
            "D-04 regression: negative offset must reject with FSERR_BAD_ARG"
        );
    }
}
