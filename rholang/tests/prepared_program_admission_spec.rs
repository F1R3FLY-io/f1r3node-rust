use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{ENot, Expr, Par};
use prost::Message;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::frontend::{
    prepare_program, PreparationError, PreparedProgram, ProgramFrontend, PREPARED_PROGRAM_ABI_V1,
};
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::with_runtime;
use rspace_plus_plus::rspace::merger::merging_logic::MergeType;

const PROVIDER_SOURCE: &str = "this is deliberately not a Rholang program";

fn scalar(value: i64) -> Par {
    let mut process = Par::default();
    process.exprs.push(Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    });
    process
}

fn environment() -> HashMap<String, Par> { HashMap::from([("supplied".to_owned(), scalar(42))]) }

fn random() -> Blake2b512Random { Blake2b512Random::create_from_bytes(&[17; 128]) }

fn prepared(source: &str) -> PreparedProgram {
    PreparedProgram::from_normalized(
        Compiler::source_to_adt_with_normalizer_env(source, HashMap::new())
            .expect("test oracle must normalize the fixture"),
    )
}

struct CountingFrontend {
    version: u16,
    version_calls: AtomicUsize,
    preparation_calls: AtomicUsize,
    result: Mutex<Option<Result<PreparedProgram, PreparationError>>>,
}

impl CountingFrontend {
    fn new(version: u16, result: Result<PreparedProgram, PreparationError>) -> Self {
        Self {
            version,
            version_calls: AtomicUsize::new(0),
            preparation_calls: AtomicUsize::new(0),
            result: Mutex::new(Some(result)),
        }
    }

    fn assert_calls(&self, version: usize, preparation: usize) {
        assert_eq!(self.version_calls.load(Ordering::Relaxed), version);
        assert_eq!(self.preparation_calls.load(Ordering::Relaxed), preparation);
    }
}

impl ProgramFrontend for CountingFrontend {
    fn abi_version(&self) -> u16 {
        self.version_calls.fetch_add(1, Ordering::Relaxed);
        self.version
    }

    fn prepare(
        &self,
        source: &str,
        supplied_environment: HashMap<String, Par>,
    ) -> Result<PreparedProgram, PreparationError> {
        self.preparation_calls.fetch_add(1, Ordering::Relaxed);
        assert_eq!(source, PROVIDER_SOURCE);
        assert_eq!(supplied_environment, environment());
        self.result
            .lock()
            .expect("frontend result lock")
            .take()
            .expect("a source must be prepared exactly once")
    }
}

#[test]
fn artifact_borrows_then_moves_without_cloning_the_process() {
    let process = scalar(42);
    let allocation = process.exprs.as_ptr();
    let bytes = process.encode_to_vec();
    let artifact = PreparedProgram::from_normalized(process);
    assert_eq!(artifact.abi_version(), PREPARED_PROGRAM_ABI_V1);
    assert!(std::ptr::eq(artifact.as_par(), artifact.as_par()));
    assert_eq!(artifact.as_par().exprs.as_ptr(), allocation);
    assert_eq!(artifact.as_par().encode_to_vec(), bytes);
    let process = artifact.into_par();
    assert_eq!(process.exprs.as_ptr(), allocation);
    assert_eq!(process.encode_to_vec(), bytes);
}

#[test]
fn deep_artifacts_reuse_stack_safe_process_drop_on_both_ownership_paths() {
    std::thread::Builder::new()
        .name("prepared-artifact-small-stack".to_owned())
        .stack_size(128 * 1024)
        .spawn(|| {
            for consume in [false, true] {
                let mut process = Par::default();
                for _ in 0..50_000 {
                    let mut parent = Par::default();
                    parent.exprs.push(Expr {
                        expr_instance: Some(ExprInstance::ENotBody(ENot { p: Some(process) })),
                    });
                    process = parent;
                }
                let artifact = PreparedProgram::from_normalized(process);
                assert_eq!(artifact.as_par().exprs.len(), 1);
                if consume {
                    drop(artifact.into_par());
                } else {
                    drop(artifact);
                }
            }
        })
        .expect("spawn bounded-stack ownership test")
        .join()
        .expect("deep prepared ownership must not overflow the native stack");
}

#[test]
fn preparation_is_explicit_versioned_and_invoked_once() {
    let frontend = CountingFrontend::new(PREPARED_PROGRAM_ABI_V1, Ok(prepared("Nil")));
    let artifact = prepare_program(&frontend, PROVIDER_SOURCE, environment())
        .expect("compatible frontend preparation");
    assert_eq!(artifact.as_par(), &Par::default());
    frontend.assert_calls(1, 1);
}

async fn establish_prior_state(runtime: &RhoRuntimeImpl) {
    runtime.cost.set_deploy_signature(&[29; 64]);
    let result = runtime
        .evaluate(
            "@\"prior-state\"!(7)",
            Cost::create(9, "prior deployment"),
            HashMap::new(),
            random(),
        )
        .await
        .expect("prior deployment completes");
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.cost.value, 1);
    runtime
        .merge_chs
        .write()
        .await
        .insert(scalar(99), MergeType::IntegerAdd);
}

#[tokio::test]
async fn preparation_and_version_failures_do_not_reset_or_charge_prior_state() {
    with_runtime("prepared-rejection-", |mut runtime| async move {
        establish_prior_state(&runtime).await;
        let prior_cost = runtime.cost.total_cost();
        let prior_remaining = runtime.cost.get();
        let prior_signature = runtime.cost.signature();
        let prior_log = runtime.get_cost_log();
        let prior_merge = runtime.merge_chs.read().await.clone();
        let prior_root = runtime.create_checkpoint().await.root;

        for (version, expected_calls, diagnostic) in [
            (PREPARED_PROGRAM_ABI_V1, 1, "rejected preparation"),
            (PREPARED_PROGRAM_ABI_V1 + 1, 0, "Unsupported frontend ABI"),
        ] {
            let frontend =
                CountingFrontend::new(version, Err(PreparationError::new("rejected preparation")));
            let result = runtime
                .evaluate_with_frontend(
                    &frontend,
                    PROVIDER_SOURCE,
                    Cost::create(77, "must not reset"),
                    environment(),
                    random(),
                )
                .await
                .expect("preparation rejection uses the normal result envelope");
            frontend.assert_calls(1, expected_calls);
            assert_eq!(result.cost, Cost::create(0, "parse failure"));
            assert!(matches!(
                result.errors.as_slice(),
                [InterpreterError::ParserError(message)] if message.contains(diagnostic)
            ));
            assert!(result.mergeable.is_empty());
            assert_eq!(runtime.cost.total_cost(), prior_cost);
            assert_eq!(runtime.cost.get(), prior_remaining);
            assert_eq!(runtime.cost.signature(), prior_signature);
            assert_eq!(runtime.get_cost_log(), prior_log);
            assert_eq!(*runtime.merge_chs.read().await, prior_merge);
            assert_eq!(runtime.create_checkpoint().await.root, prior_root);
        }
    })
    .await;
}

#[tokio::test]
async fn negative_budget_rejects_before_frontend_access_and_prepared_execution() {
    with_runtime("prepared-negative-budget-", |mut runtime| async move {
        establish_prior_state(&runtime).await;
        let prior_cost = runtime.cost.total_cost();
        let prior_remaining = runtime.cost.get();
        let prior_merge = runtime.merge_chs.read().await.clone();
        let prior_root = runtime.create_checkpoint().await.root;
        let frontend = CountingFrontend::new(0, Ok(prepared("@1!(2)")));
        for initial in [-1, i64::MIN] {
            let from_source = runtime
                .evaluate_with_frontend(
                    &frontend,
                    PROVIDER_SOURCE,
                    Cost::create(initial, "negative"),
                    environment(),
                    random(),
                )
                .await
                .expect("negative source budget rejection");
            let from_prepared = runtime
                .evaluate_prepared(
                    prepared("@1!(2)"),
                    Cost::create(initial, "negative"),
                    random(),
                )
                .await
                .expect("negative prepared budget rejection");
            for result in [from_source, from_prepared] {
                assert_eq!(result.cost, Cost::create(0, "invalid initial phlo"));
                assert!(matches!(result.errors.as_slice(),
                    [InterpreterError::IllegalArgumentError(message)]
                    if message == &format!("Initial phlo must be non-negative, got {initial}")));
                assert!(result.mergeable.is_empty());
            }
            frontend.assert_calls(0, 0);
            assert_eq!(runtime.cost.total_cost(), prior_cost);
            assert_eq!(runtime.cost.get(), prior_remaining);
            assert_eq!(*runtime.merge_chs.read().await, prior_merge);
            assert_eq!(runtime.create_checkpoint().await.root, prior_root);
        }
    })
    .await;
}

#[tokio::test]
async fn explicit_frontend_does_not_reparse_source_and_uses_real_metering() {
    with_runtime("prepared-explicit-frontend-", |runtime| async move {
        let frontend = CountingFrontend::new(PREPARED_PROGRAM_ABI_V1, Ok(prepared("@1!(2)")));
        let result = runtime
            .evaluate_with_frontend(
                &frontend,
                PROVIDER_SOURCE,
                Cost::create(1, "one COMM"),
                environment(),
                random(),
            )
            .await
            .expect("explicit frontend deployment");
        frontend.assert_calls(1, 1);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.cost.value, 1);
        assert_eq!(runtime.cost.get().value, 0);
        let contents = runtime.get_hot_changes().await;
        let row = contents
            .get(&vec![scalar(1)])
            .expect("actual channel write");
        assert_eq!(row.data.len(), 1);
        assert_eq!(row.data[0].a.pars, vec![scalar(2)]);
    })
    .await;
}

#[tokio::test]
async fn successful_prepared_admission_resets_budget_and_merge_tracking() {
    with_runtime("prepared-reset-", |mut runtime| async move {
        establish_prior_state(&runtime).await;
        let signature = runtime.cost.signature();
        let root = runtime.create_checkpoint().await.root;
        for initial in [0, 11, i64::MAX] {
            runtime
                .merge_chs
                .write()
                .await
                .insert(scalar(99), MergeType::IntegerAdd);
            let result = runtime
                .evaluate_prepared(prepared("Nil"), Cost::create(initial, "reset"), random())
                .await
                .expect("prepared Nil admission");
            assert!(result.errors.is_empty());
            assert_eq!(result.cost.value, 0);
            assert_eq!(runtime.cost.get().value, initial);
            assert_eq!(runtime.cost.signature(), signature);
            assert!(result.mergeable.is_empty());
            assert!(runtime.merge_chs.read().await.is_empty());
            assert_eq!(runtime.create_checkpoint().await.root, root);
        }
    })
    .await;
}

#[tokio::test]
async fn prepared_and_source_paths_preserve_results_costs_rng_and_state() {
    for (source, budget) in [
        ("Nil", 0),
        ("@1!(2)", 1),
        ("new name in { @0!(*name) }", 1),
        ("new x in { x!(7) | for (@value <- x) { @0!(value) } }", 10),
        ("@1!(1) | @2!(2)", 1),
        ("@2!(3.noSuchMethod())", 10),
    ] {
        let (legacy_result, legacy_root, legacy_signature) =
            with_runtime("prepared-source-oracle-", |mut runtime| async move {
                runtime.cost.set_deploy_signature(&[29; 64]);
                let result = runtime
                    .evaluate(
                        source,
                        Cost::create(budget, "fixture"),
                        HashMap::new(),
                        random(),
                    )
                    .await
                    .expect("source result");
                (
                    result,
                    runtime.create_checkpoint().await.root,
                    runtime.cost.signature(),
                )
            })
            .await;
        with_runtime("prepared-source-comparison-", |mut runtime| async move {
            runtime.cost.set_deploy_signature(&[29; 64]);
            let result = runtime
                .evaluate_prepared(prepared(source), Cost::create(budget, "fixture"), random())
                .await
                .expect("prepared result");
            assert_eq!(result.cost, legacy_result.cost, "cost for {source}");
            assert_eq!(
                result
                    .errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                legacy_result
                    .errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                "errors for {source}",
            );
            assert_eq!(
                result.mergeable, legacy_result.mergeable,
                "merge tracking for {source}"
            );
            assert_eq!(
                runtime.cost.signature(),
                legacy_signature,
                "signature for {source}"
            );
            assert_eq!(
                runtime.create_checkpoint().await.root,
                legacy_root,
                "RSpace/RNG for {source}"
            );
        })
        .await;
    }
}

#[tokio::test]
async fn raw_prepared_failure_and_source_wrapper_keep_distinct_checkpoint_ownership() {
    for raw_prepared in [false, true] {
        with_runtime("prepared-checkpoint-", |mut runtime| async move {
            establish_prior_state(&runtime).await;
            let before = runtime.create_checkpoint().await.root;
            let source = "for (_ <- @\"prior-state\") { @2!(3.noSuchMethod()) }";
            let result = if raw_prepared {
                runtime
                    .evaluate_prepared(prepared(source), Cost::create(9, "raw"), random())
                    .await
            } else {
                runtime
                    .evaluate_with_phlo(source, Cost::create(9, "wrapper"))
                    .await
            }
            .expect("evaluation error remains in the result envelope");
            assert!(!result.errors.is_empty());
            assert!(result.cost.value > 0);
            let after = runtime.create_checkpoint().await.root;
            assert_eq!(before == after, !raw_prepared);
            assert!(runtime.merge_chs.read().await.is_empty());
        })
        .await;
    }
}

#[tokio::test]
async fn prepared_admission_preserves_explicit_unmetered_mode() {
    with_runtime("prepared-mode-", |runtime| async move {
        for unmetered in [false, true] {
            runtime.cost.set_unmetered(unmetered);
            let result = runtime
                .evaluate_prepared(prepared("@1!(2)"), Cost::create(0, "mode"), random())
                .await
                .expect("mode result");
            assert_eq!(result.errors.is_empty(), unmetered);
            assert_eq!(result.cost.value, 0);
        }
    })
    .await;
}
