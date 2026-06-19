//! B5 (CA-P-097/098/104) — the STUCK out-of-fuel gate, asserted behaviorally
//! against the live interpreter.
//!
//! In the cost-accounting transpiler lowering a signed process `{% P %}[s]`
//! becomes a fuel gate `for(t <- Σ⟦s⟧){ *t | P }` (the demo header,
//! `examples/cost_accounting_demo.rho`): the gate is a RECEIVER on the signer's
//! supply channel `Σ⟦s⟧`; it fires (releasing the protected body `P`) ONLY when
//! a fuel token rests on `Σ⟦s⟧`. When the signer is UNFUNDED — no token on
//! `Σ⟦s⟧` — the gate is a LONE `PInput` with no matching producer, so it is
//! STUCK: it cannot take a `rho_step`, and the body `P` it guards never runs.
//!
//! This mirrors, at the live-runtime level, the Rocq faithfulness theorems
//! (`docs/theory/cost-accounting-native-faithfulness-design.md`,
//! `workstream-e-validator-contract.md`):
//!   * `gated_translation_stuck` / `PInput_alone_stuck` — the unfunded gate is a
//!     lone receiver with NO reduction step (silent blocking, never a panic);
//!   * `fuel_gate_body_protected` — the protected body `P` does not progress and
//!     produces no effect while the gate is stuck (it consumes nothing);
//!   * `fuel_gate_stuck_isolated` — the stuck gate is ISOLATED: a concurrent,
//!     independently-fundable process runs to completion regardless (the stuck
//!     gate does not deadlock the rest of the term).
//!
//! "Insufficient stack depth (no matching token)" = the empty `Σ⟦s⟧` channel:
//! the gate needs one token to fire and there is none, so it parks. We assert
//! the three observable consequences via `evaluate_with_phlo` + `get_data`:
//! (1) no panic / no harness error, (2) the body's effect is absent (no state
//! change, body consumes nothing), (3) an isolated sibling completes.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::BillableKind;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

async fn fresh_runtime() -> RhoRuntimeImpl {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.expect("in-memory rspace stores");
    let (runtime, _replay, _hist) = create_runtimes(store, false, &mut Vec::new()).await;
    runtime
}

/// The integer value(s) resting on the named (quoted-string) channel after
/// quiescence — a side-effect probe: an empty result means the producer that
/// would have written there never ran.
async fn cell_values(runtime: &RhoRuntimeImpl, name: &str) -> Vec<i64> {
    let channel = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(name.to_string())),
    }]);
    runtime
        .get_data(&channel)
        .await
        .iter()
        .flat_map(|datum| datum.a.pars.iter())
        .flat_map(|par| par.exprs.iter())
        .filter_map(|expr| match &expr.expr_instance {
            Some(ExprInstance::GInt(n)) => Some(*n),
            _ => None,
        })
        .collect()
}

async fn eval(runtime: &mut RhoRuntimeImpl, contract: &str) -> EvaluateResult {
    runtime
        .evaluate_with_phlo(contract, Cost::create(1_000_000, "stuck_out_of_fuel_spec"))
        .await
        .expect("evaluate must not raise a harness-level error (no panic)")
}

fn comm_count(runtime: &RhoRuntimeImpl) -> usize {
    runtime
        .get_cost_event_log()
        .iter()
        .filter(|event| event.kind == BillableKind::Comm)
        .count()
}

/// A fuel gate over an EMPTY supply channel is STUCK: the gate receiver parks
/// (no matching token), its protected body never runs, and evaluation finishes
/// WITHOUT a panic or error. `gated_translation_stuck` / `PInput_alone_stuck`
/// (silent blocking) + `fuel_gate_body_protected` (body consumes nothing).
#[tokio::test]
async fn unfunded_fuel_gate_is_stuck_and_body_never_runs() {
    let mut runtime = fresh_runtime().await;
    // `{% @"moved"!(42) %}[s]` lowered: a gate reading from the (empty) supply
    // channel `sigma_s`, whose body would move 42 onto "moved". No token is ever
    // produced on `sigma_s`, so the gate is stuck and the body is protected.
    const STUCK_GATE: &str = r#"new sigma_s in {
        for(t <- sigma_s){ *t | @"moved"!(42) }
    }"#;

    let result = eval(&mut runtime, STUCK_GATE).await;

    // (1) No panic, no harness error: the stuck gate blocks SILENTLY.
    assert!(
        result.errors.is_empty(),
        "an unfunded fuel gate must block silently, not error: {:?}",
        result.errors
    );

    // (2) No state change / the body consumes nothing: the body's `@"moved"!(42)`
    // never fired, so "moved" holds NO datum.
    let moved = cell_values(&runtime, "moved").await;
    assert!(
        moved.is_empty(),
        "the protected body must NOT run while the gate is stuck (found {:?})",
        moved
    );
}

/// `fuel_gate_stuck_isolated`: the stuck gate is ISOLATED — a concurrent,
/// independently-runnable process completes to its full effect regardless. The
/// stuck gate does not deadlock or starve the rest of the term.
#[tokio::test]
async fn stuck_gate_is_isolated_from_a_concurrent_runnable_process() {
    let mut runtime = fresh_runtime().await;
    // LEFT: the stuck unfunded gate (body would write "blocked"). RIGHT: an
    // ordinary funded interaction that writes "ran". They share no channels.
    const ISOLATED: &str = r#"new sigma_s, ch in {
        for(t <- sigma_s){ *t | @"blocked"!(1) }
        | ch!(7) | for(@v <- ch){ @"ran"!(v) }
    }"#;

    let result = eval(&mut runtime, ISOLATED).await;
    assert!(
        result.errors.is_empty(),
        "isolated evaluation must not error: {:?}",
        result.errors
    );

    // The stuck gate's body did NOT run.
    let blocked = cell_values(&runtime, "blocked").await;
    assert!(
        blocked.is_empty(),
        "the stuck gate's body must not run (found {:?})",
        blocked
    );
    // The independent process DID run to completion (its effect is present).
    let ran = cell_values(&runtime, "ran").await;
    assert_eq!(
        ran,
        vec![7],
        "the concurrent runnable process must complete despite the stuck gate"
    );
}

/// The body protected by a stuck gate CONSUMES NOTHING: a gate whose body itself
/// contains a full COMM interaction contributes ZERO completed body interactions
/// while parked. We compare the gate's COMM tally to the SAME body run UNGATED:
/// ungated the body's interaction completes (its extra COMMs fire + its effect
/// lands); gated-and-stuck it does not. The only COMM the stuck term bills is the
/// gate receiver's own install — the body's interaction never advances.
#[tokio::test]
async fn stuck_gate_body_consumes_nothing_versus_ungated_baseline() {
    // BASELINE: the body run UNGATED — a self-contained interaction that moves 5
    // onto "out". It runs to completion: `inner!(5)` send + `for(@v <- inner)`
    // receive + the body's `@"out"!(v)` send = 3 COMMs, and "out" ends holding 5.
    let mut baseline_rt = fresh_runtime().await;
    const UNGATED_BODY: &str = r#"new inner in { inner!(5) | for(@v <- inner){ @"out"!(v) } }"#;
    let baseline = eval(&mut baseline_rt, UNGATED_BODY).await;
    assert!(baseline.errors.is_empty(), "baseline must not error");
    let baseline_comms = comm_count(&baseline_rt);
    let baseline_out = cell_values(&baseline_rt, "out").await;
    assert_eq!(baseline_comms, 3, "the ungated body completes its full interaction (3 COMMs)");
    assert_eq!(baseline_out, vec![5], "ungated, the body's effect lands on \"out\"");

    // STUCK: the SAME body, now GATED behind an empty supply channel. The gate
    // receiver installs (1 COMM) but never fires, so the body's inner
    // interaction never advances — it consumes none of its COMMs and "out" stays
    // empty.
    let mut stuck_rt = fresh_runtime().await;
    const GATED_BODY: &str = r#"new sigma_s in {
        for(t <- sigma_s){ *t | new inner in { inner!(5) | for(@v <- inner){ @"out"!(v) } } }
    }"#;
    let stuck = eval(&mut stuck_rt, GATED_BODY).await;
    assert!(
        stuck.errors.is_empty(),
        "the stuck gate must block silently: {:?}",
        stuck.errors
    );
    let stuck_comms = comm_count(&stuck_rt);
    let stuck_out = cell_values(&stuck_rt, "out").await;

    // The body consumed NOTHING: "out" is empty (no effect, no state change).
    assert!(
        stuck_out.is_empty(),
        "the gated body must consume nothing while stuck (found {:?})",
        stuck_out
    );
    // The stuck term bills strictly FEWER COMMs than the completed baseline — the
    // body's interaction (the baseline's 2 COMMs) never advanced; only the gate
    // receiver's own install is billed.
    assert!(
        stuck_comms < baseline_comms,
        "the stuck body must advance strictly fewer COMMs than the completed baseline \
         (stuck={stuck_comms}, baseline={baseline_comms})"
    );
    assert_eq!(
        stuck_comms, 1,
        "only the stuck gate receiver's own install bills a COMM; the body advances none"
    );
}
