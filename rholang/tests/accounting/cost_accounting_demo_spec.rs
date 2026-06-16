//! W1 Phase 5 — the cost-accounting demo as a NATIVE integration test.
//!
//! `examples/cost_accounting_demo.rho` is the full feature showcase; it exercises
//! EVERY surface form W1 recognizes: ground `[s]`, compound `(*)`, lollipop `-o`,
//! section `# P`, depth-N budget stacks `s :: s :: ()`, ring-fenced `new`-bound
//! signatures, per-clause signed binds `{% y <- x %}[s]`, and the N-ary atomic
//! join. Compiling + running it end-to-end validates the W1 normalizer on the
//! most complex real program.
//!
//! NATIVE SEMANTICS (MAJOR-4 re-scope). Native recognition emits NO fuel gates
//! (recognition-only — the normalized `Par` has the same COMM count as the
//! unsigned program), so IN-PROGRAM PARKING does not occur: the `eve` / `Zed` /
//! free-`diSig` "thief" processes RUN rather than blocking on an unfunded gate.
//! The native model replaces in-program parking with DEPLOY-LEVEL
//! admission-rejection — an unfunded signer's deploy is rejected at the F-A
//! acceptance gate (`admit_by_funding` / `is_funded`), tested separately. We
//! therefore DROP the old "drop-in native integration test" claim and assert what
//! holds verbatim under native semantics: the GUARD-enforced conservation
//! invariants (the SWAP/MOVE `match` on funds+stock conserves both resources in
//! every interleaving), namely MONEY total 410 and WIDGET total 83 (67 opening +
//! 16 produced), with no cell ever negative.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

const DEMO: &str = include_str!("../../../examples/cost_accounting_demo.rho");

async fn fresh_runtime() -> RhoRuntimeImpl {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.expect("in-memory rspace stores");
    let (runtime, _replay, _hist) = create_runtimes(store, false, &mut Vec::new()).await;
    runtime
}

/// The whole feature-rich demo NORMALIZES under native recognition — every
/// surface form (ground / compound / lollipop / `#P` / budget stack / ring-fenced
/// `new`-bound sig / per-clause bind / N-ary join) is recognized end-to-end with
/// no double-metering gate emitted.
#[test]
fn demo_normalizes_under_native_recognition() {
    Compiler::source_to_adt(DEMO).expect("the cost-accounting demo must normalize natively");
}

/// Sum the integer values currently resting on the named (quoted-string) cells,
/// asserting none is negative. After the program reaches quiescence each cell
/// holds exactly its final value (every SWAP/MOVE re-produces the cells it
/// consumes), so the sum is the conserved resource total.
async fn cell_total(runtime: &RhoRuntimeImpl, names: &[&str]) -> i64 {
    let mut total = 0_i64;
    for name in names {
        let channel = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString(name.to_string())),
        }]);
        let data = runtime.get_data(&channel).await;
        let value: i64 = data
            .iter()
            .flat_map(|datum| datum.a.pars.iter())
            .flat_map(|par| par.exprs.iter())
            .filter_map(|expr| match &expr.expr_instance {
                Some(ExprInstance::GInt(n)) => Some(*n),
                _ => None,
            })
            .sum();
        assert!(value >= 0, "cell `{name}` must never go negative (got {value})");
        total += value;
    }
    total
}

/// The demo RUNS to completion natively and the GUARD-enforced conservation
/// invariants hold: MONEY total 410, WIDGET total 83, no cell negative.
#[tokio::test]
async fn demo_runs_and_conserves_money_and_inventory() {
    let mut runtime = fresh_runtime().await;
    let result = runtime
        .evaluate_with_phlo(DEMO, Cost::create(500_000_000, "cost_accounting_demo"))
        .await
        .expect("the demo must evaluate without a harness-level error");
    assert!(
        result.errors.is_empty(),
        "the demo must run to completion without runtime errors: {:?}",
        result.errors
    );

    // MONEY ledger (cash cells) — conserved at its opening total.
    let money = cell_total(
        &runtime,
        &[
            "Ada_cash", "Ben_cash", "Cy_cash", "Di_cash", "Sue_cash", "Sam_cash", "Whse_cash",
            "Fae_cash", "Gus_cash",
        ],
    )
    .await;
    assert_eq!(money, 410, "MONEY is conserved at its opening total (410)");

    // INVENTORY ledger (stock/home cells) — opening 67 + produced 16 = 83.
    let widgets = cell_total(
        &runtime,
        &[
            "Fab1_out", "Fab2_out", "Whse_stk", "Sue_stk", "Sam_stk", "Flash_stk", "Ada_home",
            "Ben_home", "Cy_home", "Fae_home", "Gus_home",
        ],
    )
    .await;
    assert_eq!(
        widgets, 83,
        "WIDGETs are conserved: 67 opening + 16 produced = 83"
    );
}
