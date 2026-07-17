//! Runtime-level negative tests for the `eval_new` injection path.
//!
//! The unit tests in `interpreter::io::injections` prove the
//! compile-time shape: that the fileio bundle is copied into the
//! `New` node's `injections` `BTreeMap`, and that an empty env
//! leaves that map empty. The two tests below close the loop at
//! the *runtime* end -- driving a real `RhoRuntimeImpl` through
//! `evaluate_with_env` and confirming that:
//!
//! 1. A user-written `new x(`rho:io:fs:native:1.0.0/open`) in { ... }`
//!    compiled with an empty env fails at `eval_new` with "No value
//!    set for {urn}", because the URN is filtered out of `urn_map`
//!    by `rho_runtime::is_internal_urn` and nothing populates
//!    `new.injections` for it. This is the property the FS-agent's
//!    ocap story leans on -- verified end-to-end here rather than
//!    inferred from reading the filter + `reduce.rs`.
//!
//! 2. A `Bundle`-wrapped `Par` supplied via the injection env is
//!    rejected at `eval_new` with "invalid injection". The
//!    injection path (`reduce.rs:1297-1325`) only accepts
//!    `GUnforgeable` or `Expression` shapes; anything else lands
//!    in the fallback error branch. This is why
//!    `fileio_native_urns()` must return bare `GPrivate` Pars, not
//!    bundle-wrapped ones -- documented in the module docstring
//!    but only implied by code inspection until now.

use std::sync::Arc;

use models::rhoapi::{BindPattern, Bundle, ListParWithRandom, Par, TaggedContinuation};
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::rho_runtime::{create_rho_runtime, RhoRuntime, RhoRuntimeImpl};
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

const OPEN_URN: &str = "rho:io:fs:native:1.0.0/open";
const OPEN_SRC: &str = "new nOpen(`rho:io:fs:native:1.0.0/open`) in { Nil }";

async fn mk_runtime() -> RhoRuntimeImpl {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let space: RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> =
        RSpace::create(store, Arc::new(Box::new(Matcher))).unwrap();
    create_rho_runtime(
        space,
        Arc::new(std::collections::HashMap::new()),
        true,
        &mut Vec::new(),
        ExternalServices::noop(),
    )
    .await
}

fn error_text(errors: &[InterpreterError]) -> String {
    errors
        .iter()
        .map(|e| format!("{:?}", e))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Compile succeeds (compilation doesn't validate URN existence),
/// but at `eval_new` time the URN misses `urn_map` (filtered by
/// `is_internal_urn`) *and* the empty env means `new.injections`
/// has no fallback, so we get "No value set for
/// rho:io:fs:native:1.0.0/open".
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn user_deploy_cannot_bind_hidden_fileio_urn_with_empty_env() {
    let mut runtime = mk_runtime().await;
    let outcome = runtime
        .evaluate_with_env(OPEN_SRC, std::collections::HashMap::new())
        .await;

    match outcome {
        Ok(eval_result) => {
            assert!(
                !eval_result.errors.is_empty(),
                "expected eval to fail; got no errors"
            );
            let combined = error_text(&eval_result.errors);
            assert!(
                combined.contains(OPEN_URN) && combined.to_lowercase().contains("no value set"),
                "expected 'No value set for {OPEN_URN}' in errors, got: {combined}"
            );
        }
        Err(err) => {
            let msg = format!("{:?}", err);
            assert!(
                msg.contains(OPEN_URN) && msg.to_lowercase().contains("no value set"),
                "expected 'No value set for {OPEN_URN}' in top-level err, got: {msg}"
            );
        }
    }
}

/// The injection path only accepts `GUnforgeable` or `Expression`
/// Pars. A `Bundle` wrapper -- even one containing the correct
/// fixed-channel Par -- is rejected. This test proves the
/// docstring warning in `io/injections.rs` is enforced by the
/// runtime, not just by code review.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bundle_wrapped_injection_is_rejected_at_runtime() {
    use rholang::rust::interpreter::system_processes::FixedChannels;

    let bundle_par = Par::default().with_bundles(vec![Bundle {
        body: Some(FixedChannels::native_open()),
        write_flag: true,
        read_flag: false,
    }]);
    let mut env = std::collections::HashMap::new();
    env.insert(OPEN_URN.to_string(), bundle_par);

    let mut runtime = mk_runtime().await;
    let outcome = runtime.evaluate_with_env(OPEN_SRC, env).await;

    match outcome {
        Ok(eval_result) => {
            assert!(
                !eval_result.errors.is_empty(),
                "expected eval to fail on bundle injection; got no errors"
            );
            let combined = error_text(&eval_result.errors);
            assert!(
                combined.contains("invalid injection"),
                "expected 'invalid injection' in errors, got: {combined}"
            );
        }
        Err(err) => {
            let msg = format!("{:?}", err);
            assert!(
                msg.contains("invalid injection"),
                "expected 'invalid injection' in top-level err, got: {msg}"
            );
        }
    }
}
