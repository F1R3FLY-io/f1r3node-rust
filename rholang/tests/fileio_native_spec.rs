//! Direct-URN dispatch coverage for every `rho:io:fs:native:1.0.0/*`.
//!
//! Complements the per-native behavioral coverage in
//! `file_dir_check.rs` / `fs_wal_spec.rs` / `fileio_stream_spec.rs`
//! etc. by pinning that EVERY native URN is bindable + dispatchable
//! at the `disable_fs_native_urn_filter` genesis scope.  Regression
//! scenario: a URN drops off the registration list at
//! `rho_runtime.rs::install_fs_natives` OR the URN filter's
//! whitelist drifts out of sync with the registered set.  Either
//! would fail every-native binding at this layer.
//!
//! Each test binds ONE URN, dispatches it with a well-shaped arg
//! tuple against a tempfile fixture, and asserts a well-formed
//! reply arrives.  We're NOT re-testing behavior (each native has
//! its own dedicated spec); we ARE testing that the wiring holds
//! after the H-29-3 lift + streaming-backing slice reshuffled the
//! registration list.
//!
//! The `nativeSmokeCheck` helper factors out the boilerplate: bind
//! the URN, send a well-shaped tuple, read the reply on `@"out"`,
//! assert it's a list (`[true, ...]` or `[false, code, msg]`).

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
    use rholang::rust::interpreter::accounting::costs::Cost;
    use rholang::rust::interpreter::external_services::ExternalServices;
    use rholang::rust::interpreter::matcher::r#match::Matcher;
    use rholang::rust::interpreter::rho_runtime::{create_rho_runtime, RhoRuntime, RhoRuntimeImpl};
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    fn rand() -> Blake2b512Random { Blake2b512Random::create_from_bytes(&[1, 2, 45, 65]) }

    /// Runtime with the fs-native URN filter disabled so tests can
    /// bind `rho:io:fs:native:1.0.0/*` URNs directly at user scope.
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

    /// Evaluate a Rholang program and read the single Par produced
    /// on `@"out"`.  Panics if no reply arrives or if the runtime
    /// reports errors.  Uses `get_hot_changes` to inspect the
    /// tuplespace directly — the same shape as
    /// `file_dir_check.rs`'s `eval_and_read_out`.
    async fn eval_and_read_out(runtime: &RhoRuntimeImpl, term: &str) -> Par {
        use models::rhoapi::expr::ExprInstance;
        use models::rhoapi::Expr;
        let result = runtime
            .evaluate(
                term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .expect("evaluate");
        assert!(
            result.errors.is_empty(),
            "unexpected runtime errors: {:?}",
            result.errors
        );
        let map = runtime.get_hot_changes().await;
        // Key = @"out" as a channel.  A `@"out"` channel is a Par
        // with one Expr of GString("out").
        let key_par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString("out".to_string())),
        }]);
        let row = map.get(&vec![key_par]).unwrap_or_else(|| {
            panic!(
                "no reply on @\"out\".  Tuplespace keys: {:?}",
                map.keys().collect::<Vec<_>>()
            )
        });
        row.data[0].a.pars[0].clone()
    }

    /// Assert the given Par is a Rholang list with head boolean
    /// `expected_ok` (true = success reply; false = error reply).
    fn assert_reply_head_bool(reply: &Par, expected_ok: bool) {
        let expr = reply.exprs.first().expect("reply Par must have an Expr");
        let list = match &expr.expr_instance {
            Some(ExprInstance::EListBody(l)) => l,
            other => panic!("reply must be an EList; got {other:?}"),
        };
        assert!(
            !list.ps.is_empty(),
            "reply list must be non-empty; got: {reply:?}"
        );
        let head = list
            .ps
            .first()
            .and_then(|p| p.exprs.first())
            .and_then(|e| e.expr_instance.as_ref());
        let actual = match head {
            Some(ExprInstance::GBool(b)) => *b,
            other => panic!("reply head must be GBool; got {other:?}"),
        };
        assert_eq!(
            actual, expected_ok,
            "reply head-bool mismatch on: {reply:?}",
        );
    }

    // ----------------------------------------------------------------
    // fd-based natives (open / close / read / readAt / write /
    // writeAt / seek / tell / size / truncate / flush).  Each test
    // binds one URN, exercises a known-good arg tuple against a
    // tempfile fixture, and asserts a well-formed [true, ...] reply.
    // ----------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_open_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"hello").unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/open`), ret in {{
              op!("{root}", "f.bin", "rw", "oracular", *ret) |
              for (@r <- ret) {{ @"out"!(r) }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_close_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"").unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/open`),
                cl(`rho:io:fs:native:1.0.0/close`),
                oret, cret in {{
              op!("{root}", "f.bin", "rw", "oracular", *oret) |
              for (@[true, fd] <- oret) {{
                cl!(fd, *cret) |
                for (@r <- cret) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_read_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"hello").unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/open`),
                rd(`rho:io:fs:native:1.0.0/read`),
                oret, rret in {{
              op!("{root}", "f.bin", "r", "oracular", *oret) |
              for (@[true, fd] <- oret) {{
                rd!(fd, 5, *rret) |
                for (@r <- rret) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_read_at_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"hello world").unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/open`),
                rd(`rho:io:fs:native:1.0.0/readAt`),
                oret, rret in {{
              op!("{root}", "f.bin", "r", "oracular", *oret) |
              for (@[true, fd] <- oret) {{
                rd!(fd, 6, 5, *rret) |
                for (@r <- rret) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_write_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), vec![0u8; 32]).unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/open`),
                wr(`rho:io:fs:native:1.0.0/write`),
                oret, wret in {{
              op!("{root}", "f.bin", "rw", "oracular", *oret) |
              for (@[true, fd] <- oret) {{
                wr!(fd, "ab".hexToBytes(), *wret) |
                for (@r <- wret) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_write_at_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), vec![0u8; 32]).unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/open`),
                wr(`rho:io:fs:native:1.0.0/writeAt`),
                oret, wret in {{
              op!("{root}", "f.bin", "rw", "oracular", *oret) |
              for (@[true, fd] <- oret) {{
                wr!(fd, 4, "cd".hexToBytes(), *wret) |
                for (@r <- wret) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_seek_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), vec![0u8; 32]).unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/open`),
                sk(`rho:io:fs:native:1.0.0/seek`),
                oret, sret in {{
              op!("{root}", "f.bin", "rw", "oracular", *oret) |
              for (@[true, fd] <- oret) {{
                sk!(fd, 4, "set", *sret) |
                for (@r <- sret) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_tell_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"").unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/open`),
                tl(`rho:io:fs:native:1.0.0/tell`),
                oret, tret in {{
              op!("{root}", "f.bin", "rw", "oracular", *oret) |
              for (@[true, fd] <- oret) {{
                tl!(fd, *tret) |
                for (@r <- tret) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_size_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"hello").unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/open`),
                sz(`rho:io:fs:native:1.0.0/size`),
                oret, sret in {{
              op!("{root}", "f.bin", "r", "oracular", *oret) |
              for (@[true, fd] <- oret) {{
                sz!(fd, *sret) |
                for (@r <- sret) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_truncate_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), vec![0u8; 32]).unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/open`),
                tr(`rho:io:fs:native:1.0.0/truncate`),
                oret, tret in {{
              op!("{root}", "f.bin", "rw", "oracular", *oret) |
              for (@[true, fd] <- oret) {{
                tr!(fd, 8, *tret) |
                for (@r <- tret) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_flush_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"").unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/open`),
                fl(`rho:io:fs:native:1.0.0/flush`),
                oret, fret in {{
              op!("{root}", "f.bin", "rw", "oracular", *oret) |
              for (@[true, fd] <- oret) {{
                fl!(fd, *fret) |
                for (@r <- fret) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    // ----------------------------------------------------------------
    // Path-based natives (stat / exists / entries / rename / copyFile
    // / removeFile / removeDir / chmod / chown / quarantine).
    // ----------------------------------------------------------------

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_stat_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"").unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/stat`), ret in {{
              op!("{root}", "f.bin", "oracular", *ret) |
              for (@r <- ret) {{ @"out"!(r) }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_exists_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"").unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/exists`), ret in {{
              op!("{root}", "f.bin", *ret) |
              for (@r <- ret) {{ @"out"!(r) }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_entries_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/entries`), ret in {{
              op!("{root}", "sub", "oracular", *ret) |
              for (@r <- ret) {{ @"out"!(r) }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_entries_stream_open_close_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new open(`rho:io:fs:native:1.0.0/entriesStreamOpen`),
                close(`rho:io:fs:native:1.0.0/entriesStreamClose`),
                oret, cret in {{
              open!("{root}", "sub", "oracular", *oret) |
              for (@[true, fd] <- oret) {{
                close!(fd, *cret) |
                for (@r <- cret) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_entries_stream_next_urn_dispatches_eos() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new open(`rho:io:fs:native:1.0.0/entriesStreamOpen`),
                next(`rho:io:fs:native:1.0.0/entriesStreamNext`),
                oret, nret in {{
              open!("{root}", "sub", "oracular", *oret) |
              for (@[true, fd] <- oret) {{
                next!(fd, *nret) |
                for (@r <- nret) {{ @"out"!(r) }}
              }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        // Empty dir yields EOS: [false, "EOS"]; head is false but
        // the URN dispatched cleanly.  Both success and EOS are
        // acceptable; the pin is that a well-formed reply arrives.
        assert_reply_head_bool(&reply, false);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_rename_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.bin"), b"").unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/rename`), ret in {{
              op!("{root}", "a.bin", "{root}", "b.bin", "oracular", *ret) |
              for (@r <- ret) {{ @"out"!(r) }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_copy_file_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("src.bin"), b"payload").unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/copyFile`), ret in {{
              op!("{root}", "src.bin", "{root}", "dst.bin", "oracular", *ret) |
              for (@r <- ret) {{ @"out"!(r) }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_remove_file_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"").unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/removeFile`), ret in {{
              op!("{root}", "f.bin", "oracular", *ret) |
              for (@r <- ret) {{ @"out"!(r) }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_remove_dir_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/removeDir`), ret in {{
              op!("{root}", "sub", false, "oracular", *ret) |
              for (@r <- ret) {{ @"out"!(r) }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_chmod_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"").unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/chmod`), ret in {{
              op!("{root}", "f.bin", 420, "oracular", *ret) |
              for (@r <- ret) {{ @"out"!(r) }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_chown_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"").unwrap();
        let runtime = create_runtime().await;
        // Nil/Nil = no-op chown — succeeds without NSS lookup.
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/chown`), ret in {{
              op!("{root}", "f.bin", Nil, Nil, "oracular", *ret) |
              for (@r <- ret) {{ @"out"!(r) }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_quarantine_urn_dispatches() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.bin"), b"").unwrap();
        let runtime = create_runtime().await;
        let term = format!(
            r#"
            new op(`rho:io:fs:native:1.0.0/quarantine`), ret in {{
              op!("{root}", "f.bin", *ret) |
              for (@r <- ret) {{ @"out"!(r) }}
            }}
            "#,
            root = dir.path().display(),
        );
        let reply = eval_and_read_out(&runtime, &term).await;
        assert_reply_head_bool(&reply, true);
    }
}
