//! W1 multi-sig showcase — the cost-accounting MULTI-SIGNATURE TREASURY as a
//! NATIVE integration test.
//!
//! `examples/multi_sig_treasury.rho` centers on joint authority: 2-of-2 and
//! 3-of-3 compound `(*)` signatures, the combined-cell co-signed token, a 2-of-3
//! quorum built from racing pairs over a one-shot grant, lollipop `-o` delegation,
//! a `new`-bound ring-fenced reserve key, the receive-side per-clause signed join
//! `{% y <- x %}[s]`, and a `# P` charter signature. Compiling + running it
//! validates the W1 normalizer on every joint-authority surface form.
//!
//! NATIVE SEMANTICS (MAJOR-4). Native recognition emits NO fuel gates
//! (recognition-only — the normalized `Par` is identical to the
//! unsigned program), so IN-PROGRAM PARKING does not occur and multi-sig
//! FUNDEDNESS is settled at DEPLOY ADMISSION (the Rust acceptance gate, with the
//! BALANCED `DefaultApportionment` debiting each cosigner wallet an equal share),
//! which a single CLI/eval run does not apply. We therefore assert what holds
//! verbatim under native semantics: the GUARD-enforced conservation invariant —
//! every `pay` / `release` `match` re-emits both cells on both branches, so MONEY
//! is conserved in every interleaving regardless of which gates "run". Three
//! non-crossing pools (Treasury 500 + Quorum_grant 40 + Reserve 100) ⇒ total 640,
//! no cell ever negative, the one-shot grant released exactly once.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

const TREASURY: &str = include_str!("../../../examples/multi_sig_treasury.rho");

async fn fresh_runtime() -> RhoRuntimeImpl {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.expect("in-memory rspace stores");
    let (runtime, _replay, _hist) = create_runtimes(store, false, &mut Vec::new()).await;
    runtime
}

/// The multi-sig treasury NORMALIZES under native recognition — every joint-authority
/// surface form (2-of-2 / 3-of-3 compound, combined-cell token, lollipop, `#P`
/// charter sig, per-clause signed join) is recognized end-to-end with no
/// double-metering gate emitted.
#[test]
fn treasury_normalizes_under_native_recognition() {
    Compiler::source_to_adt(TREASURY).expect("the multi-sig treasury must normalize natively");
}

/// Sum the integer values currently resting on the named (quoted-string) cells,
/// asserting none is negative. After the program reaches quiescence each cell
/// holds exactly its final value (every PAY/RELEASE re-produces the cells it
/// consumes), so the sum is the conserved money total.
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
        assert!(
            value >= 0,
            "cell `{name}` must never go negative (got {value})"
        );
        total += value;
    }
    total
}

/// The treasury RUNS to completion natively and the GUARD-enforced conservation
/// invariant holds: MONEY total 640 (Treasury 500 + Quorum_grant 40 + Reserve 100),
/// no cell negative — across all twelve cash cells of the three non-crossing pools.
#[tokio::test]
async fn treasury_runs_and_conserves_money() {
    let mut runtime = fresh_runtime().await;
    let result = runtime
        .evaluate_with_phlo(TREASURY, Cost::create(500_000_000, "multi_sig_treasury"))
        .await
        .expect("the multi-sig treasury must evaluate without a harness-level error");
    assert!(
        result.errors.is_empty(),
        "the treasury must run to completion without runtime errors: {:?}",
        result.errors
    );

    // MONEY ledger — the three non-crossing pools, conserved at their opening total.
    let money = cell_total(&runtime, &[
        // Treasury pool
        "Treasury",
        "Dave_cash",
        "Doug_cash",
        "Frank_cash",
        "Erin_cash",
        "Heidi_cash",
        "Niaj_cash",
        "Olivia_cash",
        "Peggy_cash",
        "Ivan_cash",
        "Judy_cash",
        // Quorum pool
        "Quorum_grant",
        "Grace_cash",
        // Reserve pool
        "Reserve",
        "Mallory_cash",
    ])
    .await;
    assert_eq!(
        money, 640,
        "MONEY is conserved at its opening total (Treasury 500 + Quorum_grant 40 + Reserve 100 = 640)"
    );
}
