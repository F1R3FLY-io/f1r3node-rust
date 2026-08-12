// Slice 31: phase-scoped fs-native URN visibility.
//
// The reducer's `filter_fs_native_urns` flag is ON by default (state
// execution).  Toggled OFF around `play_deploys_for_genesis` in
// `casper::rholang::runtime` so the FsGenesis composed source can
// bind `rho:io:fs:native:1.0.0/*` URNs.  When ON, `eval_new` returns
// a `ReduceError` for any URN starting with the fs-native prefix —
// closing MVP simplification #5 (H-26-F4 / H-27-3): user deploys can
// no longer bind raw fs syscalls and bypass Fs.rho's sandbox.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
    use rholang::rust::interpreter::accounting::costs::Cost;
    use rholang::rust::interpreter::errors::InterpreterError;
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
        runtime
    }

    /// Slice 31 default: fs-native URN filter is ON at runtime
    /// construction — protects state-execution deploys by default.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn filter_is_enabled_by_default() {
        let runtime = create_runtime().await;
        assert!(
            runtime.fs_native_urn_filter_enabled(),
            "filter must be ON by default so state deploys are protected"
        );
    }

    /// H-P7-5 review fix (Phase 7 whole-review round): the
    /// `exempt_fs_native_urn_filter` RAII guard re-enables the
    /// filter on Drop.  Pins the normal-path semantic.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn raii_guard_reenables_filter_on_drop() {
        let runtime = create_runtime().await;
        assert!(runtime.fs_native_urn_filter_enabled());
        {
            let _guard = runtime.exempt_fs_native_urn_filter();
            assert!(
                !runtime.fs_native_urn_filter_enabled(),
                "filter is OFF while guard is alive"
            );
            // Guard drops at scope end.
        }
        assert!(
            runtime.fs_native_urn_filter_enabled(),
            "filter MUST be re-enabled after guard drops"
        );
    }

    /// H-P7-5 core: guard re-enables on panic unwind.  This is the
    /// pre-fix bug — bare `disable/enable` pair could leak the
    /// exemption if the async block between them panicked.  RAII
    /// closes that gap.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn raii_guard_reenables_filter_on_panic() {
        let runtime = create_runtime().await;
        assert!(runtime.fs_native_urn_filter_enabled());
        let flag_before = runtime.fs_native_urn_filter_enabled();
        assert!(flag_before);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = runtime.exempt_fs_native_urn_filter();
            assert!(!runtime.fs_native_urn_filter_enabled());
            panic!("simulated deploy panic");
            // Guard's Drop MUST run despite the panic (panic=unwind).
        }));
        assert!(result.is_err(), "panic must propagate");
        assert!(
            runtime.fs_native_urn_filter_enabled(),
            "filter MUST be re-enabled by guard's Drop even on panic — \
             pre-fix bare-toggle design would leave it OFF here"
        );
    }

    /// H-31-COV-2 whole-review pin: exemption held across
    /// SEVERAL async awaits still re-enables on drop.  Simulates
    /// the play_deploys_for_genesis + replay_deploys pattern where
    /// the guard spans a loop of async deploy processing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn raii_guard_spans_multiple_awaits_and_reenables() {
        let runtime = create_runtime().await;
        {
            let _guard = runtime.exempt_fs_native_urn_filter();
            for _ in 0..3 {
                tokio::task::yield_now().await;
                assert!(
                    !runtime.fs_native_urn_filter_enabled(),
                    "filter must stay OFF across yields while guard held"
                );
            }
        }
        assert!(runtime.fs_native_urn_filter_enabled());
    }

    /// H-P7-5 companion: explicit `drop(guard)` also re-enables
    /// (mirrors the play_deploys_for_genesis + replay_deploys
    /// usage pattern).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn raii_guard_explicit_drop_reenables_filter() {
        let runtime = create_runtime().await;
        let guard = runtime.exempt_fs_native_urn_filter();
        assert!(!runtime.fs_native_urn_filter_enabled());
        drop(guard);
        assert!(runtime.fs_native_urn_filter_enabled());
    }

    /// Slice 31: enable/disable are idempotent and observable via
    /// the introspection helper.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn toggle_methods_flip_the_filter() {
        let runtime = create_runtime().await;
        assert!(runtime.fs_native_urn_filter_enabled());
        runtime.disable_fs_native_urn_filter();
        assert!(!runtime.fs_native_urn_filter_enabled());
        runtime.disable_fs_native_urn_filter(); // idempotent
        assert!(!runtime.fs_native_urn_filter_enabled());
        runtime.enable_fs_native_urn_filter();
        assert!(runtime.fs_native_urn_filter_enabled());
        runtime.enable_fs_native_urn_filter(); // idempotent
        assert!(runtime.fs_native_urn_filter_enabled());
    }

    /// Slice 31 core: with the filter ON, a user-scope deploy that
    /// binds ANY rho:io:fs:native:* URN gets a `ReduceError`.  This
    /// is the MVP-#5 gap closure: previously the URN was globally
    /// lookupable, letting user code hit raw syscalls and bypass
    /// Fs.rho's sandbox / mode-cap / bundle checks.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn state_deploy_binding_fs_native_urn_fails_with_reduce_error() {
        let runtime = create_runtime().await;
        assert!(runtime.fs_native_urn_filter_enabled());
        let term = r#"
            new fsOpen(`rho:io:fs:native:1.0.0/open`) in { Nil }
        "#;
        let result = runtime
            .evaluate(
                term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .expect("evaluate should return an EvaluateResult");
        assert!(
            !result.errors.is_empty(),
            "filter ON: user-scope binding of fs-native URN must fail"
        );
        // The specific error is `ReduceError` with a message
        // referring to the URN.  Match on the string to keep the
        // test robust against error-enum-variant refactors.
        let msgs: Vec<String> = result.errors.iter().map(|e| format!("{e}")).collect();
        assert!(
            msgs.iter().any(|m| m.contains("rho:io:fs:native")),
            "error must reference the rejected URN; got {msgs:?}"
        );
    }

    /// Slice 31: every one of the 22 fs-native suffixes is rejected
    /// (prefix-based check catches the whole family in one filter).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn all_fs_native_suffixes_are_rejected() {
        let runtime = create_runtime().await;
        for suffix in [
            "open",
            "close",
            "read",
            "readAt",
            "write",
            "writeAt",
            "seek",
            "tell",
            "size",
            "flush",
            "stat",
            "exists",
            "truncate",
            "chmod",
            "chown",
            "removeFile",
            "removeDir",
            "rename",
            "copyFile",
            "entries",
            // Phase 8 slice 8a — range-lock natives.
            "lockRange",
            "lockSequential",
            "releaseLock",
            // Phase 8 slice 8a step-4 — File.close sweep native.
            "releaseAllForHolder",
        ] {
            let term = format!(r#"new bad(`rho:io:fs:native:1.0.0/{suffix}`) in {{ bad!(0) }}"#);
            let result = runtime
                .evaluate(
                    &term,
                    Cost::unsafe_max(),
                    std::collections::HashMap::new(),
                    rand(),
                )
                .await
                .expect("evaluate should return an EvaluateResult");
            assert!(
                !result.errors.is_empty(),
                "suffix `{suffix}` must be filtered but slipped through"
            );
        }
    }

    /// Slice 31 negative: URNs NOT starting with the fs-native
    /// prefix are unaffected by the filter.  Rebind a well-known
    /// non-fs URN and confirm it still resolves.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn non_fs_urns_pass_through_the_filter() {
        let runtime = create_runtime().await;
        // rho:registry:lookup is a legitimate URN every deploy uses;
        // must remain resolvable regardless of filter state.
        let term = r#"new rl(`rho:registry:lookup`) in { Nil }"#;
        let result = runtime
            .evaluate(
                term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .expect("evaluate should return an EvaluateResult");
        assert!(
            result.errors.is_empty(),
            "non-fs URN must not be affected by fs-native filter; got {:?}",
            result.errors
        );
    }

    /// Slice 31: with the filter OFF, the same fs-native URN binding
    /// that failed above now RESOLVES (the URN is in urn_map because
    /// every runtime registers all 22 fs-native handlers at setup).
    /// The `send` itself may fail at dispatch time (wrong arity /
    /// non-String args), but the `eval_new` binding must succeed —
    /// no ReduceError referring to the URN.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn genesis_scope_binding_fs_native_urn_resolves() {
        let runtime = create_runtime().await;
        runtime.disable_fs_native_urn_filter();
        assert!(!runtime.fs_native_urn_filter_enabled());
        let term = r#"new fsOpen(`rho:io:fs:native:1.0.0/open`) in { Nil }"#;
        let result = runtime
            .evaluate(
                term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .expect("evaluate should return an EvaluateResult");
        // With the filter off, eval_new resolves the URN via urn_map.
        // The body is `Nil` so the deploy runs cleanly with no
        // errors.  A previous filter-on run would have surfaced a
        // ReduceError instead.
        assert!(
            result.errors.is_empty(),
            "filter OFF: genesis-scope binding must resolve; got errors: {:?}",
            result.errors
        );
    }

    /// Slice 31: re-enabling the filter after a genesis batch
    /// restores protection for subsequent state deploys.  Mirrors
    /// the `play_deploys_for_genesis` wrapper in casper's runtime.rs
    /// which toggles off, runs, toggles back on.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn re_enabling_filter_restores_protection() {
        let runtime = create_runtime().await;
        runtime.disable_fs_native_urn_filter();
        // "Genesis deploy" — succeeds.
        let genesis_term = r#"new fsOpen(`rho:io:fs:native:1.0.0/open`) in { Nil }"#;
        let genesis_result = runtime
            .evaluate(
                genesis_term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .unwrap();
        assert!(genesis_result.errors.is_empty(), "genesis batch succeeds");
        // Re-enable.  Subsequent user deploy must fail.
        runtime.enable_fs_native_urn_filter();
        let user_term = r#"new fsOpen(`rho:io:fs:native:1.0.0/open`) in { Nil }"#;
        let user_result = runtime
            .evaluate(
                user_term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .unwrap();
        assert!(
            !user_result.errors.is_empty(),
            "state deploy after re-enable must be blocked"
        );
    }

    /// Slice 31: prefix-based filter catches near-neighbor URNs even
    /// if they don't exist in urn_map (defense-in-depth against a
    /// future URN suffix rename or addition).  A hypothetical
    /// `rho:io:fs:native:1.0.0/futureOp` is filtered before the
    /// urn_map lookup step, so no need to update the filter when a
    /// new suffix lands.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn filter_catches_unknown_fs_native_urn_prefix() {
        let runtime = create_runtime().await;
        let term = r#"new x(`rho:io:fs:native:1.0.0/futureOp`) in { Nil }"#;
        let result = runtime
            .evaluate(
                term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .unwrap();
        assert!(
            !result.errors.is_empty(),
            "unknown-suffix fs-native URN must be filtered (prefix-based)"
        );
        // Distinguish filter-rejection from urn_map-miss: the
        // filter's error explicitly mentions the URN family.
        let msgs: Vec<String> = result.errors.iter().map(|e| format!("{e}")).collect();
        assert!(
            msgs.iter().any(|m| m.contains("rho:io:fs:native")),
            "filter-rejection error should reference the fs-native family; got {msgs:?}"
        );
    }

    /// L-31-COV-1 (Phase 7 whole-review): positive-pin that the
    /// `rho:system:*` URN family is UNAFFECTED by the fs-native
    /// filter.  System URNs (deployerId, deployId, blockData,
    /// invalidBlocks) are structurally distinct from
    /// `rho:io:fs:native:*` and must remain resolvable in state
    /// deploys.  Regression guard: if a future refactor broadens the
    /// prefix check (e.g. accidentally matches `rho:` or `rho:sys*`),
    /// this test will fail with a filter-rejection error.
    ///
    /// Note: bare test runtimes don't wire the per-deploy injections
    /// that supply `rho:system:*` values, so binding these URNs here
    /// legitimately produces a `BugFoundError` from the injection
    /// path.  What we're pinning is that the fs-native filter itself
    /// does NOT reject them — none of the errors mention the
    /// `rho:io:fs:native` filter message.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn system_urns_pass_through_the_filter() {
        let runtime = create_runtime().await;
        assert!(runtime.fs_native_urn_filter_enabled());
        for urn in ["rho:system:deployerId", "rho:system:deployId"] {
            let term = format!(r#"new x(`{urn}`) in {{ Nil }}"#);
            let result = runtime
                .evaluate(
                    &term,
                    Cost::unsafe_max(),
                    std::collections::HashMap::new(),
                    rand(),
                )
                .await
                .expect("evaluate should return an EvaluateResult");
            let msgs: Vec<String> = result.errors.iter().map(|e| format!("{e}")).collect();
            assert!(
                !msgs.iter().any(|m| m.contains("rho:io:fs:native")),
                "fs-native filter must NOT reject system URN `{urn}`; got: {msgs:?}"
            );
            assert!(
                !msgs
                    .iter()
                    .any(|m| m.contains("not resolvable in this phase")),
                "system URN `{urn}` must not trip the phase-scope filter; got: {msgs:?}"
            );
        }
    }

    /// Slice 31: introspection ergonomics — verify the error type is
    /// `ReduceError`, not the generic `BugFoundError` that would
    /// arise from missing-in-urn_map + missing-from-injections.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn filter_rejection_is_a_reduce_error_not_a_bug_error() {
        let runtime = create_runtime().await;
        let term = r#"new x(`rho:io:fs:native:1.0.0/open`) in { Nil }"#;
        let result = runtime
            .evaluate(
                term,
                Cost::unsafe_max(),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .unwrap();
        let e = result.errors.first().expect("expected at least one error");
        assert!(
            matches!(e, InterpreterError::ReduceError(_)),
            "filter rejection must produce ReduceError, got {e:?}"
        );
    }
}
