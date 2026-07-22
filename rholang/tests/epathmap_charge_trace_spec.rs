//! EPathMap fix P0 — CHARGE-TRACE PARITY HARNESS (test-only).
//!
//! Runs fixed query programs on a counting runtime and snapshots the ORDERED
//! (operation, reservation-KIND, value) charge sequence — the consensus-side
//! derived truth at 84a0fbe4 that P2's fused method-chain evaluation must
//! replay charge-for-charge (same kinds, same weights, same ORDER; plan §1-P2
//! "CHARGE REPLAY", amendment PM-4(b)).
//!
//! Model: the 84a0fbe4 cost-invariance probe
//! (`zipper_query_methods_spec::repeated_query_costs_are_identical`) extended
//! from a total-cost scalar to the full per-charge sequence, in the ordered
//! success-log style of the `loom_cost_trace_slots.rs` family.
//!
//! TRACE SOURCE: `RuntimeBudget::get_canonical_event_log()` — the canonical
//! committed set, a schedule-independent pure function of (program, budget)
//! (see accounting/mod.rs `cost_trace_digest` and
//! `RuntimeBudgetReplay.tla`'s `RuntimeRaceDoesNotChangeReconciledDigest`).
//! Every test still runs each program on TWO fresh runtimes and asserts
//! trace equality, so the determinism the parity gates rely on is asserted
//! in-suite, not merely assumed.
//!
//! RESERVATION KINDS (amendment PM-4(b), verified at source): the event log
//! records `BillableKind` + the `Cost::operation` string; the reservation
//! entry point is recovered by the per-operation classification table
//! [`classify_kind`] — every `union` charge in the zipper-method family goes
//! through `reserve_incremental_primitive(union_cost(1))`
//! (reduce.rs:3625/3704/3889/4976/5051/5271/5327/5405/5484/5564/5670/5760/
//! 5850), `lookup` through `reserve_primitive(lookup_cost())` (:3975/:4054),
//! `method call` through `reserve_primitive(method_call_cost())`
//! (:1536/:2723), and `var eval` through `reserve_primitive(var_eval_cost())`
//! (:1217).
//!
//! RANDOMNESS (captured truth): `evaluate_with_term` seeds evaluation with
//! `Blake2b512Random::create_from_length(128)`, which draws from
//! `rand::thread_rng()` and is nondeterministic per call. The harness drives
//! `RhoRuntime::evaluate` directly with a FIXED `create_from_bytes` seed so
//! every observable — including tuplespace `random_state` — is
//! deterministic. Charge traces are rand-independent either way (no charge
//! weight derives from the random state at 84a0fbe4); the fixed seed makes
//! that a pinned fact rather than an assumption.

use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EPathMap, ETuple, Expr, Par};
use models::rust::utils::{new_elist_par, new_gstring_par};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::BillableKind;
use rholang::rust::interpreter::accounting::has_cost::HasCost;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::with_runtime;

// ─────────────────────────────────────────────────────────────────────────────
// The charge-trace utility
// ─────────────────────────────────────────────────────────────────────────────

/// One rendered charge row: `{reservation-kind}({operation})={weight}` for
/// primitives, `{kind}={weight}` for comm/reduction/substitution (whose
/// events carry no operation string — the weight is the diagnostic value).
fn render_charge(kind: &BillableKind, weight: u64) -> String {
    match kind {
        BillableKind::Primitive(operation) => {
            format!("{}({operation})={weight}", classify_kind(operation))
        }
        BillableKind::Comm => format!("comm={weight}"),
        BillableKind::Reduction => format!("reduction={weight}"),
        BillableKind::Substitution => format!("subst={weight}"),
    }
}

/// The PM-4(b) reservation-kind classification: which metering entry point
/// produced a `Primitive` charge, recovered from its operation string per
/// the source-verified table in the module docs.
///
/// CAPTURED REFINEMENT (84a0fbe4): `union_cost(1)` is
/// `Cost { value: add_cost() * 1 = 3, operation: "1 union cost" }`
/// (costs.rs:252-258) — the operation string embeds the element count and
/// the charged WEIGHT is 3, not 1. Every `"{n} union cost"` charge in
/// reduce.rs goes through `reserve_incremental_primitive` (the zipper-method
/// sites and the collection-union sites alike); every other primitive in
/// these programs goes through `reserve_primitive`.
fn classify_kind(operation: &str) -> &'static str {
    if operation.ends_with(" union cost") {
        "incr-prim"
    } else {
        "prim"
    }
}

/// Evaluate `term` on a fresh runtime with `initial_phlo` and a FIXED rand,
/// returning the evaluation result plus the rendered canonical charge trace.
async fn run_with_trace(
    prefix: &str,
    term: &str,
    initial_phlo: Cost,
) -> (EvaluateResult, Vec<String>) {
    let term = term.to_string();
    with_runtime(prefix, move |runtime: RhoRuntimeImpl| async move {
        let res = runtime
            .evaluate(&term, initial_phlo, HashMap::new(), fixed_rand())
            .await
            .expect("evaluate must not fail structurally");
        let trace = runtime
            .cost()
            .get_canonical_event_log()
            .iter()
            .map(|event| render_charge(&event.kind, event.weight))
            .collect::<Vec<_>>();
        (res, trace)
    })
    .await
}

/// The fixed evaluation seed (see the module docs on
/// `create_from_length`'s thread_rng nondeterminism).
fn fixed_rand() -> Blake2b512Random {
    Blake2b512Random::create_from_bytes(&[
        0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
        0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
        0x18, 0x19,
    ])
}

/// Run twice on fresh runtimes; assert the traces agree (the determinism the
/// parity gates rely on), then return one of them with its result.
async fn deterministic_trace(prefix: &str, term: &str) -> (EvaluateResult, Vec<String>) {
    let (res_a, trace_a) = run_with_trace(&format!("{prefix}-a-"), term, Cost::unsafe_max()).await;
    let (_res_b, trace_b) = run_with_trace(&format!("{prefix}-b-"), term, Cost::unsafe_max()).await;
    assert_eq!(
        trace_a, trace_b,
        "canonical charge trace must be identical across fresh runtimes"
    );
    (res_a, trace_a)
}

// ─────────────────────────────────────────────────────────────────────────────
// The E-6a query programs (chain shapes mirrored from e6a_support.rs)
// ─────────────────────────────────────────────────────────────────────────────

/// The subject-index literal for `Pair(A, B)` at `site0` — the source-text
/// mirror of `models/tests/fixtures/mod.rs::e6a_index_epathmap`. The «v»
/// σ-carriers are 1-tuples of GStrings here (reflect-tag GPrivates are not
/// source-expressible); chain SHAPES and charge behavior are carrier-type
/// independent.
const INDEX_MAP: &str = r#"{|
    ["t.deadbeef.Pair", "site0"],
    ["v", "site0", ("Pair",)],
    ["t.deadbeef.A", "site0", "Pair.0"],
    ["v", "site0", "Pair.0", ("A",)],
    ["t.deadbeef.B", "site0", "Pair.1"],
    ["v", "site0", "Pair.1", ("B",)]
|}"#;

/// Wrap one E-6a query chain in the exact treatment COMM shape
/// (`discovery_call_par`): one persistent index publish + one receive whose
/// body publishes the chain's result.
fn e6a_program(result_channel: &str, chain: &str) -> String {
    format!(
        r#"
        @"e6a:idx:site0"!!({INDEX_MAP}) |
        for( @idx <- @"e6a:idx:site0" ) {{
            @"{result_channel}"!( {chain} )
        }}
        "#
    )
}

/// Shape 1 — DISCOVERY: `idx.readZipperAt([tag(op)]).getSubtrie()`
/// (e6a_support.rs:489-494).
fn discovery_program() -> String {
    e6a_program(
        "e6a:sites:site0/Pair",
        r#"idx.readZipperAt(["t.deadbeef.Pair"]).getSubtrie()"#,
    )
}

/// Shape 2 — TAG GUARD: `idx.readZipperAt([tag, c₀…]).pathExists()`
/// (e6a_support.rs:682-690).
fn tag_guard_program() -> String {
    e6a_program(
        "out",
        r#"idx.readZipperAt(["t.deadbeef.A", "site0", "Pair.0"]).pathExists()"#,
    )
}

/// Shape 3 — σ-EXISTENCE: `idx.readZipperAt(["v", c₀…]).pathExists()`
/// (e6a_support.rs:695-703).
fn sigma_exists_program() -> String {
    e6a_program(
        "out",
        r#"idx.readZipperAt(["v", "site0", "Pair.0"]).pathExists()"#,
    )
}

/// Shape 4 — σ-CHAIN: `idx.readZipperAt(["v", c₀…]).descendFirst().getLeaf()`
/// (e6a_support.rs:708-717).
fn sigma_chain_program() -> String {
    e6a_program(
        "out",
        r#"idx.readZipperAt(["v", "site0", "Pair.0"]).descendFirst().getLeaf()"#,
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Readback helpers
// ─────────────────────────────────────────────────────────────────────────────

async fn read_single_par(runtime: &RhoRuntimeImpl, channel_name: &str) -> Par {
    let channel = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(channel_name.to_string())),
    }]);
    let data = runtime.get_data(&channel).await;
    assert_eq!(data.len(), 1, "expected one datum at @\"{channel_name}\"");
    let pars = &data[0].a.pars;
    assert_eq!(pars.len(), 1, "expected one Par at @\"{channel_name}\"");
    pars[0].clone()
}

fn gstring_par(value: &str) -> Par {
    new_gstring_par(value.to_string(), Vec::new(), false)
}

fn ground_list(elements: Vec<Par>) -> Par {
    new_elist_par(elements, Vec::new(), false, None, Vec::new(), false)
}

fn ground_tuple1(inner: Par) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::ETupleBody(ETuple {
                ps: vec![inner],
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }],
        ..Par::default()
    }
}

/// The expected σ-chain result: the ORIGINAL «v» entry list (the trie stores
/// the whole entry Par as the value; `getLeaf` returns it losslessly).
fn expected_sigma_entry() -> Par {
    ground_list(vec![
        gstring_par("v"),
        gstring_par("site0"),
        gstring_par("Pair.0"),
        ground_tuple1(gstring_par("A")),
    ])
}

// ─────────────────────────────────────────────────────────────────────────────
// Pinned charge-trace snapshots (captured at 84a0fbe4; EPM_P0_BLESS=1 prints)
// ─────────────────────────────────────────────────────────────────────────────

fn bless_enabled() -> bool { std::env::var_os("EPM_P0_BLESS").is_some() }

fn check_trace(label: &str, pinned: &[&str], actual: &[String]) {
    if bless_enabled() {
        println!("SNAPSHOT {label} = &[");
        for row in actual {
            println!("    \"{row}\",");
        }
        println!("];");
    } else {
        let actual_refs: Vec<&str> = actual.iter().map(String::as_str).collect();
        assert_eq!(
            actual_refs, pinned,
            "{label}: charge trace drifted from the pinned 84a0fbe4 sequence"
        );
    }
}

/// CAPTURED CANONICAL STRUCTURE (common to all four shapes): the canonical
/// order (BillableTokenEvent Ord over source paths) opens with the two COMM
/// charges (persistent publish `send eval`=11, receive `receive eval`=11),
/// then the publish/receive substitution charges (size-proportional
/// `substitute_and_charge` `encoded_len` weights — the §0.B mechanism;
/// `subst=186` is the index-map datum — was `subst=286` pre-wire; the
/// `encoded_len`-proportional charge dropped because a GROUND EPathMap now
/// serializes as the compact field-8 U(m) key stream instead of the field-1
/// `ps` walk (487→335 bytes for this fixture). The cost ALGORITHM is
/// unchanged; the charge follows the smaller wire automatically), then the
/// chain: per-link
/// `method call`=10 at dispatch (outermost-first recursion into the target),
/// the base `var eval`=10, per link innermost-out the link constant
/// (`incr-prim(1 union cost)=3` for the navigation links,
/// `prim(lookup)=3` for getLeaf/getSubtrie), the result send's substitution
/// + `comm`, and finally the continuation-instantiation substitution rows.
///
/// Shape 1 — discovery: `readZipperAt([tag]).getSubtrie()`
/// (subst=17/24 lead rows differ from the other shapes because the result
/// channel name `e6a:sites:site0/Pair` is longer than `out`).
const DISCOVERY_TRACE: &[&str] = &[
    "comm=11",
    "comm=11",
    "subst=17",
    "subst=24",
    "prim(method call)=10",
    "subst=186",
    "prim(method call)=10",
    "prim(var eval)=10",
    "incr-prim(1 union cost)=3",
    "prim(lookup)=3",
    // getSubtrie() RETURNS a ground map; its result substitution is
    // encoded_len-proportional and follows the compact field-8 U(m) wire
    // (was subst=44 pre-wire). Cost algorithm unchanged.
    "subst=36",
    "comm=11",
    "subst=17",
    "subst=11",
    "subst=128",
];

/// Shape 2 — tag guard (2-link: readZipperAt + pathExists; BOTH links charge
/// `union_cost(1)` → `incr-prim(1 union cost)=3` — pathExists' constant is
/// the union charge at reduce.rs:5051, weight 3 per the captured
/// `union_cost` refinement).
const TAG_GUARD_TRACE: &[&str] = &[
    "comm=11",
    "comm=11",
    "subst=7",
    "subst=17",
    "prim(method call)=10",
    "subst=186",
    "prim(method call)=10",
    "prim(var eval)=10",
    "incr-prim(1 union cost)=3",
    "incr-prim(1 union cost)=3",
    "subst=4",
    "comm=11",
    "subst=17",
    "subst=11",
    "subst=131",
];

/// Shape 3 — σ-existence (same 2-link shape as the tag guard, «v» prefix —
/// identical charge KINDS and weights except the final continuation
/// substitution, whose encoded_len reflects the shorter «v» path list).
const SIGMA_EXISTS_TRACE: &[&str] = &[
    "comm=11",
    "comm=11",
    "subst=7",
    "subst=17",
    "prim(method call)=10",
    "subst=186",
    "prim(method call)=10",
    "prim(var eval)=10",
    "incr-prim(1 union cost)=3",
    "incr-prim(1 union cost)=3",
    "subst=4",
    "comm=11",
    "subst=17",
    "subst=11",
    "subst=120",
];

/// Shape 4 — σ-chain (3-link: readZipperAt + descendFirst + getLeaf: three
/// `method call` dispatches, two navigation `union` constants, one `lookup`;
/// `subst=49` is the returned «v» entry Par's substitution).
const SIGMA_CHAIN_TRACE: &[&str] = &[
    "comm=11",
    "comm=11",
    "subst=7",
    "subst=17",
    "prim(method call)=10",
    "subst=186",
    "prim(method call)=10",
    "prim(method call)=10",
    "prim(var eval)=10",
    "incr-prim(1 union cost)=3",
    "incr-prim(1 union cost)=3",
    "prim(lookup)=3",
    "subst=49",
    "comm=11",
    "subst=17",
    "subst=11",
    "subst=145",
];

// ─────────────────────────────────────────────────────────────────────────────
// The four chain-shape snapshots
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn discovery_chain_trace_and_result() {
    let program = discovery_program();
    let (res, trace) = deterministic_trace("epm-p0-discovery", &program).await;
    assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
    check_trace("DISCOVERY_TRACE", DISCOVERY_TRACE, &trace);

    // Result identity: the subtrie of the op prefix is exactly the «s» root
    // entry (op-first family ⇒ one candidate site).
    with_runtime("epm-p0-discovery-read-", |runtime: RhoRuntimeImpl| async move {
        let res = runtime
            .evaluate(&program, Cost::unsafe_max(), HashMap::new(), fixed_rand())
            .await
            .expect("evaluate");
        assert!(res.errors.is_empty());
        let subtrie = read_single_par(&runtime, "e6a:sites:site0/Pair").await;
        let expected_entry = ground_list(vec![
            gstring_par("t.deadbeef.Pair"),
            gstring_par("site0"),
        ]);
        match subtrie.exprs.first().and_then(|e| e.expr_instance.as_ref()) {
            Some(ExprInstance::EPathmapBody(EPathMap { ps, .. })) => {
                assert_eq!(ps.len(), 1, "op subtrie must hold exactly the root «s» entry");
                assert_eq!(ps[0], expected_entry, "subtrie value must be the original entry Par");
            }
            other => panic!("expected an EPathMap subtrie, got {other:?}"),
        }
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tag_guard_chain_trace_and_result() {
    let program = tag_guard_program();
    let (res, trace) = deterministic_trace("epm-p0-tagguard", &program).await;
    assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
    check_trace("TAG_GUARD_TRACE", TAG_GUARD_TRACE, &trace);

    with_runtime("epm-p0-tagguard-read-", |runtime: RhoRuntimeImpl| async move {
        let res = runtime
            .evaluate(&program, Cost::unsafe_max(), HashMap::new(), fixed_rand())
            .await
            .expect("evaluate");
        assert!(res.errors.is_empty());
        let out = read_single_par(&runtime, "out").await;
        assert_eq!(
            out.exprs.first().and_then(|e| e.expr_instance.clone()),
            Some(ExprInstance::GBool(true)),
            "tag guard must hold at the indexed site"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sigma_exists_chain_trace_and_result() {
    let program = sigma_exists_program();
    let (res, trace) = deterministic_trace("epm-p0-sigmaexists", &program).await;
    assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
    check_trace("SIGMA_EXISTS_TRACE", SIGMA_EXISTS_TRACE, &trace);

    with_runtime("epm-p0-sigmaexists-read-", |runtime: RhoRuntimeImpl| async move {
        let res = runtime
            .evaluate(&program, Cost::unsafe_max(), HashMap::new(), fixed_rand())
            .await
            .expect("evaluate");
        assert!(res.errors.is_empty());
        let out = read_single_par(&runtime, "out").await;
        assert_eq!(
            out.exprs.first().and_then(|e| e.expr_instance.clone()),
            Some(ExprInstance::GBool(true)),
            "σ-position existence must hold"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sigma_chain_trace_and_result() {
    let program = sigma_chain_program();
    let (res, trace) = deterministic_trace("epm-p0-sigmachain", &program).await;
    assert!(res.errors.is_empty(), "errors: {:?}", res.errors);
    check_trace("SIGMA_CHAIN_TRACE", SIGMA_CHAIN_TRACE, &trace);

    with_runtime("epm-p0-sigmachain-read-", |runtime: RhoRuntimeImpl| async move {
        let res = runtime
            .evaluate(&program, Cost::unsafe_max(), HashMap::new(), fixed_rand())
            .await
            .expect("evaluate");
        assert!(res.errors.is_empty());
        let out = read_single_par(&runtime, "out").await;
        assert_eq!(
            out, expected_sigma_entry(),
            "getLeaf must return the ORIGINAL «v» entry Par losslessly"
        );
    })
    .await;
}

// ─────────────────────────────────────────────────────────────────────────────
// PM-4(d) edge programs: Nil-mid-chain, arity-first ordering
// ─────────────────────────────────────────────────────────────────────────────

/// The exact Nil-mid-chain error (eval_single_expr's `_` arm,
/// reduce.rs:7022-7024). The (misleading) string IS the parity target —
/// P2's fused path must raise the identical error at the identical
/// point-in-sequence.
const NIL_MID_CHAIN_ERROR: &str = "Error: Multiple expressions given.";

/// A Nil-producing chain prefix + a follow-on 0-arity method. All four Nil
/// sources of amendment PM-4(d):
///   * getLeaf at a value-less path (reduce.rs:3929),
///   * descendFirst with no children (:5536),
///   * ascendOne at root (:5292),
///   * descendIndexedBranch with a negative index (:5598).
fn nil_source_programs() -> Vec<(&'static str, String)> {
    vec![
        (
            "getLeaf-no-value",
            r#"@"nil"!( {| ["a", "x"] |}.readZipperAt(["a"]).getLeaf().pathExists() )"#.to_string(),
        ),
        (
            "descendFirst-no-children",
            r#"@"nil"!( {| ["a"] |}.readZipperAt(["a"]).descendFirst().getLeaf() )"#.to_string(),
        ),
        (
            "ascendOne-at-root",
            r#"@"nil"!( {| ["a"] |}.readZipper().ascendOne().getLeaf() )"#.to_string(),
        ),
        (
            "descendIndexedBranch-negative",
            r#"@"nil"!( {| ["a"] |}.readZipper().descendIndexedBranch(-1).getLeaf() )"#.to_string(),
        ),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nil_mid_chain_raises_the_exact_reduce_error() {
    for (label, program) in nil_source_programs() {
        let (res, _trace) =
            run_with_trace(&format!("epm-p0-nil-{label}-"), &program, Cost::unsafe_max()).await;
        assert_eq!(res.errors.len(), 1, "{label}: expected exactly one error");
        match &res.errors[0] {
            InterpreterError::ReduceError(message) => assert_eq!(
                message, NIL_MID_CHAIN_ERROR,
                "{label}: the Nil-mid-chain error string is the parity target"
            ),
            other => panic!("{label}: expected ReduceError, got {other:?}"),
        }
    }
}

/// The getLeaf-no-value edge, trace pinned: the send's comm + channel
/// substitution commit, the follow-on link's `method call` IS charged
/// (dispatch precedes target evaluation), the inner links charge normally
/// (readZipperAt's union, getLeaf's lookup), and the FAILING link's constant
/// (pathExists' union) is NEVER charged — evaluation aborts inside
/// `eval_single_expr` before the reserve. No result produce follows.
const NIL_GETLEAF_TRACE: &[&str] = &[
    "comm=11",
    "subst=7",
    "prim(method call)=10",
    "prim(method call)=10",
    "prim(method call)=10",
    "incr-prim(1 union cost)=3",
    "prim(lookup)=3",
];

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nil_mid_chain_charge_prefix_is_pinned() {
    let (_label, program) = &nil_source_programs()[0];
    let (res_a, trace_a) =
        run_with_trace("epm-p0-niltrace-a-", program, Cost::unsafe_max()).await;
    let (_res_b, trace_b) =
        run_with_trace("epm-p0-niltrace-b-", program, Cost::unsafe_max()).await;
    assert_eq!(trace_a, trace_b, "Nil-mid-chain trace must be deterministic");
    assert_eq!(res_a.errors.len(), 1);
    check_trace("NIL_GETLEAF_TRACE", NIL_GETLEAF_TRACE, &trace_a);
}

/// Nil + WRONG ARITY: the arity check precedes `eval_single_expr` in every
/// method `apply` (e.g. GetLeafMethod, reduce.rs:3966-3973), so a
/// wrong-arity call on a Nil target raises `MethodArgumentNumberMismatch` —
/// NOT the Nil ReduceError. The ordering is the pinned truth.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nil_with_wrong_arity_hits_the_arity_check_first() {
    let program =
        r#"@"nil"!( {| ["a", "x"] |}.readZipperAt(["a"]).getLeaf().getLeaf("extra") )"#;
    let (res, _trace) = run_with_trace("epm-p0-arity-", program, Cost::unsafe_max()).await;
    assert_eq!(res.errors.len(), 1, "expected exactly one error");
    match &res.errors[0] {
        InterpreterError::MethodArgumentNumberMismatch {
            method,
            expected,
            actual,
        } => {
            assert_eq!(method, "getLeaf");
            assert_eq!(*expected, 0);
            assert_eq!(*actual, 1);
        }
        other => panic!("expected MethodArgumentNumberMismatch, got {other:?}"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Budget-exhaustion-at-index-k scaffold
// ─────────────────────────────────────────────────────────────────────────────

/// One exhaustion observation: drive the σ-chain program with a budget of
/// `k` tokens and record what happens.
#[derive(Debug, PartialEq, Eq)]
struct ExhaustionObservation {
    k: i64,
    /// Names of the errors in the evaluate result (variant names only).
    errors: Vec<&'static str>,
    /// The consensus consumed total (`EvaluateResult::cost.value`).
    consumed: i64,
    /// Charge rows committed before the boundary.
    committed_rows: usize,
}

fn error_name(error: &InterpreterError) -> &'static str {
    match error {
        InterpreterError::OutOfPhlogistonsError => "OutOfPhlogistons",
        InterpreterError::ReduceError(_) => "ReduceError",
        InterpreterError::MethodArgumentNumberMismatch { .. } => "MethodArgumentNumberMismatch",
        InterpreterError::AggregateError { .. } => "AggregateError",
        _ => "Other",
    }
}

async fn observe_exhaustion(k: i64) -> ExhaustionObservation {
    let program = sigma_chain_program();
    let (res, trace) =
        run_with_trace(&format!("epm-p0-exhaust-{k}-"), &program, Cost::create(k, "test budget"))
            .await;
    ExhaustionObservation {
        k,
        errors: res.errors.iter().map(error_name).collect(),
        consumed: res.cost.value,
        committed_rows: trace.len(),
    }
}

/// THE DERIVED TRUTH (D3 regime, DR-9/OD-3): at 84a0fbe4 the liveness gate
/// is PER-COMM — `consensus_cost_unit` is 1 for `Comm` and 0 for
/// `Reduction`/`Primitive`/`Substitution` (accounting/mod.rs:509-517), so
/// primitive charge indexes CANNOT exhaust the budget: exhaustion boundaries
/// walk the COMM sub-sequence only. The σ-chain program has 3 COMMs
/// (persistent publish, receive, result send): k=0/1/2 exhaust with
/// OutOfPhlogistons and consumed=k; k=3 completes with consumed=3 and the
/// full 17-row canonical committed set. P2's fused path must reproduce the
/// deterministic columns exactly: mid-chain primitive fusion can never move
/// an exhaustion boundary, because no primitive ever consumed a token to
/// begin with.
///
/// ★ CAPTURED SCHEDULE-DEPENDENCE (a P0 finding, not normalized away): at
/// the k values where PARALLEL branches race for the last token (here k=1:
/// the publish send and the receive both charge a comm against one token),
/// the diagnostic `committed_rows` count is SCHEDULE-DEPENDENT — the losing
/// branch ABORTS, truncating the attempt multiset the canonical
/// reconciliation runs over (observed: 3 rows when the publish branch wins
/// and completes its substitutions, 1 row under a loaded scheduler when the
/// receive side wins). The reconciliation itself is a pure function of the
/// recorded attempts (the TLA+ `RuntimeRaceDoesNotChangeReconciledDigest`
/// invariant); it is the attempt SET that varies under mid-deploy abort.
/// The consensus-relevant observables — the error and the consumed total —
/// are deterministic at every k (exactly one comm can win each token). The
/// harness therefore pins `errors`+`consumed` exactly at every k, pins
/// `committed_rows` exactly where the attempt multiset is complete (k=0:
/// nothing commits; k≥3: the full run), and bounds it to
/// [consumed, full-trace] at the racing ks. P2/P4 exhaustion differentials
/// must compare only the deterministic projection at those ks.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn budget_exhaustion_walks_comm_boundaries() {
    const FULL_TRACE_ROWS: usize = 17; // == SIGMA_CHAIN_TRACE.len()
    assert_eq!(SIGMA_CHAIN_TRACE.len(), FULL_TRACE_ROWS);

    // (k, errors, consumed, committed_rows: Some(exact) | None = racing k,
    //  bounded to [consumed, FULL_TRACE_ROWS]).
    let expected: [(i64, Vec<&'static str>, i64, Option<usize>); 5] = [
        (0, vec!["OutOfPhlogistons"], 0, Some(0)),
        (1, vec!["OutOfPhlogistons"], 1, None),
        (2, vec!["OutOfPhlogistons"], 2, None),
        (3, vec![], 3, Some(FULL_TRACE_ROWS)),
        (4, vec![], 3, Some(FULL_TRACE_ROWS)),
    ];

    for (k, errors, consumed, rows) in expected {
        let actual = observe_exhaustion(k).await;
        if bless_enabled() {
            println!("SNAPSHOT exhaustion {actual:?}");
            continue;
        }
        assert_eq!(actual.errors, errors, "exhaustion errors drifted (k={k})");
        assert_eq!(actual.consumed, consumed, "exhaustion consumed drifted (k={k})");
        match rows {
            Some(exact) => assert_eq!(
                actual.committed_rows, exact,
                "committed_rows drifted at a deterministic k (k={k})"
            ),
            None => assert!(
                (consumed as usize..=FULL_TRACE_ROWS).contains(&actual.committed_rows),
                "committed_rows {} outside the captured envelope at racing k={k}",
                actual.committed_rows
            ),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Cost-invariance guard (the 84a0fbe4 probe, kept close to the traces)
// ─────────────────────────────────────────────────────────────────────────────

/// The four chain shapes charge identical totals on fresh runtimes in
/// different memo-warmth states — the scalar invariance the per-charge
/// snapshots above refine (mirrors
/// `zipper_query_methods_spec::repeated_query_costs_are_identical`).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chain_shape_costs_are_memo_warmth_invariant() {
    for (label, program) in [
        ("discovery", discovery_program()),
        ("tag-guard", tag_guard_program()),
        ("sigma-exists", sigma_exists_program()),
        ("sigma-chain", sigma_chain_program()),
    ] {
        let (res_cold, _) =
            run_with_trace(&format!("epm-p0-inv-cold-{label}-"), &program, Cost::unsafe_max())
                .await;
        let (res_warm, _) =
            run_with_trace(&format!("epm-p0-inv-warm-{label}-"), &program, Cost::unsafe_max())
                .await;
        assert!(res_cold.errors.is_empty() && res_warm.errors.is_empty());
        assert_eq!(
            res_cold.cost.value, res_warm.cost.value,
            "{label}: trie-memo warmth must not change charged costs"
        );
    }
}
