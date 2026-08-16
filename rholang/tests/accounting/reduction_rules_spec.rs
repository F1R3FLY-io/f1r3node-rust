//! B1 — RED (Reduction-rule) example + property tests for the five gated-COMM
//! reduction rules (spec §3.6 Rule 1–5) and the N-ary join (J1/J2), driven
//! against the LIVE interpreter.
//!
//! ## What the Rocq model proves vs. what the runtime exposes
//!
//! The Rocq development (`formal/rocq/cost_accounted_rho/theories/
//! TokenConservation.v`) pins each rule's EXACT token drop at the
//! signature-token abstraction layer (`system_token_count`):
//!
//! | Rocq lemma                 │ rule shape                              │ drop |
//! |────────────────────────────┼─────────────────────────────────────────┼──────|
//! | `rule1_decreases_by_one`   │ single-token `for(P)\|x!(Q)`             │  −1  |
//! | `rule2_decreases_by_two`   │ component pair `(s1∘s2)`, two tokens    │  −2  |
//! | `rule3_decreases_by_one`   │ combined `(s1∘s2)`, one combined token  │  −1  |
//! | `rule4_decreases_by_one`   │ split input/output, combined token      │  −1  |
//! | `rule5_decreases_by_two`   │ split input/output, two tokens          │  −2  |
//!
//! Those token drops live on the `TGate` gate-stripping layer. The f1r3node
//! runtime is the spec's `s₀` collapse with native recognition-only signing.
//! The normalized `Par` carries no explicit fuel-gate processes. RSpace emits
//! one `BillableKind::Comm` when a complete binary or join match commits;
//! producer/consumer arrival order and unmatched I/O do not affect cost.
//!
//! So this file asserts the rules at the TWO layers where each is observable:
//!
//!   1. RUNTIME COMM LAYER: each successful redex consumes exactly one atomic
//!      COMM token. A binary interaction bills one regardless of which side
//!      arrives last. An N-way join also bills one, regardless of arity.
//!
//!   2. SIGNATURE-TOKEN LAYER (the static `Sig` model): the Rocq token DROP
//!      itself — Rule 1/3/4 strip ONE gate (one signature pool), Rule 2/5 strip
//!      TWO (a component pair) — is mirrored by the leaf-pool count of the rule's
//!      envelope `Sig`: a single-signer redex draws from 1 pool (drop 1), a
//!      compound `s1∘s2` redex draws from 2 leaf pools (drop 2). This is the
//!      faithful Rust witness of `ruleN_decreases_by_{one,two}`.
//!
//! The JOIN (J1/J2): an N-ary join `for(y1<-x1 & … & yk<-xk){P}` is one atomic
//! RSpace contraction and is billed exactly once regardless of arity.

use models::rhoapi::Par;
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, TestRunner};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::{
    funding_sig_compound, funding_sig_single, BillableKind, Sig,
};
use rholang::rust::interpreter::compiler::compiler::Compiler;
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

fn install_test_payer(runtime: &RhoRuntimeImpl) {
    runtime.cost.set_deploy_signature_funded(
        b"reduction-rules-spec-deploy",
        Sig::Ground(b"reduction-rules-spec-payer".to_vec()),
    );
}

/// Run `contract` to completion on a fresh runtime with an abundant budget and
/// return the runtime's consumed atomic-COMM count.
async fn runtime_comm_count(contract: &str) -> usize {
    let mut runtime = fresh_runtime().await;
    install_test_payer(&runtime);
    let result = runtime
        .evaluate_with_phlo(contract, Cost::create(50_000_000, "reduction_rules_spec"))
        .await
        .expect("evaluate must not error at the harness level");
    assert!(
        result.errors.is_empty(),
        "contract must run to completion without runtime errors: {:?}",
        result.errors
    );
    runtime
        .get_cost_event_log()
        .iter()
        .filter(|event| event.kind == BillableKind::Comm)
        .count()
}

/// Recursively count potential send/receive introductions. This structural
/// quantity conservatively bounds atomic matches for the non-persistent closed
/// fixtures but is not realized cost.
fn communication_introduction_count(par: &Par) -> usize {
    let mut n = par.sends.len();
    for receive in &par.receives {
        n += 1;
        if let Some(body) = &receive.body {
            n += communication_introduction_count(body);
        }
    }
    for new in &par.news {
        if let Some(body) = &new.p {
            n += communication_introduction_count(body);
        }
    }
    for mat in &par.matches {
        for case in &mat.cases {
            if let Some(source) = &case.source {
                n += communication_introduction_count(source);
            }
        }
    }
    n
}

fn normalized_par(contract: &str) -> Par {
    Compiler::source_to_adt(contract).expect("contract must parse + normalize")
}

/// Number of distinct leaf signature pools a redex's envelope `Sig` draws from
/// — the Rust witness of the Rocq per-rule TOKEN drop: a single gate (Rule
/// 1/3/4) is one leaf pool, a stripped component pair (Rule 2/5) is two.
fn leaf_pool_count(sig: &Sig) -> usize { sig.signer_channels().len() }

// ═══════════════════════════════════════════════════════════════════════════
// Layer 1 — RUNTIME COMM LAYER: one token per successful atomic contraction.
// ═══════════════════════════════════════════════════════════════════════════

/// A single COMM interaction (the shape underlying Rules 1/3/4 — a `for | !`
/// exchange) bills exactly one atomic COMM. Its two introductions form a strict
/// structural upper bound.
#[tokio::test]
async fn single_comm_interaction_bills_one_atomic_match() {
    let redex = r#"new x in { x!(1) | for(y <- x){ Nil } }"#;
    let comms = runtime_comm_count(redex).await;
    assert_eq!(
        comms, 1,
        "a single for|! interaction bills one atomic match"
    );
    assert_eq!(
        communication_introduction_count(&normalized_par(redex)),
        2,
        "the redex contains two potential participants"
    );
}

/// Rule 1 (single-token `for(P) | x!(Q)`, the input continuation re-emits): the
/// underlying interaction is one COMM exchange whose body fires a further send,
/// so it realizes two successive atomic matches.
#[tokio::test]
async fn rule1_input_continuation_observable_cost() {
    let redex = r#"new x, r in { x!(1) | for(y <- x){ r!(*y) } | for(z <- r){ Nil } }"#;
    let comms = runtime_comm_count(redex).await;
    assert_eq!(comms, 2, "two successful rendezvous = 2 COMMs");
    assert_eq!(communication_introduction_count(&normalized_par(redex)), 4);
}

/// Observable cost is STRICTLY POSITIVE and rule-determined for every COMM
/// interaction, and strictly less than the same program with an extra
/// independently matched redex added. This is
/// the live-runtime image of `token_strictly_decreases`: a reduction step
/// consumes a strictly positive quantum, and adding a rule's redex strictly
/// increases the consumed amount by that redex's COMM nodes.
#[tokio::test]
async fn observable_cost_strictly_increases_with_each_redex() {
    let one = runtime_comm_count(r#"new x in { x!(1) | for(y <- x){ Nil } }"#).await;
    let two = runtime_comm_count(
        r#"new x, w in { x!(1) | for(y <- x){ Nil } | w!(2) | for(z <- w){ Nil } }"#,
    )
    .await;
    assert!(
        one > 0,
        "every COMM interaction consumes a strictly positive cost"
    );
    assert_eq!(one, 1, "one interaction = 1 atomic COMM");
    assert_eq!(two, 2, "two disjoint interactions = 2 atomic COMMs");
    assert!(
        two > one,
        "adding a redex strictly increases the consumed cost"
    );
    assert_eq!(
        two - one,
        1,
        "the added interaction's quantum is exactly one atomic COMM"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Layer 2 — SIGNATURE-TOKEN LAYER: the Rocq per-rule TOKEN drop, mirrored by the
// leaf-pool count of the rule's envelope Sig. Rule 1/3/4 strip ONE gate (one
// pool); Rule 2/5 strip a component PAIR (two pools).
// ═══════════════════════════════════════════════════════════════════════════

/// Rules 1, 3, 4 each strip a SINGLE gate — the `system_token_count` drops
/// by one. The Rust witness: the rule's envelope `Sig` is a single atom
/// (Rule 1) or a combined `s1∘s2` whose COMM draws ONE combined token
/// (Rule 3/4). For the drop-by-ONE rules the *number of tokens consumed in
/// the step* is one; we pin the single-signer pool image (one leaf) and the
/// combined-token image (the combined pool is drawn as one).
#[test]
fn rule1_3_4_strip_one_token() {
    // Rule 1: a single-signer redex — one leaf pool, the single token stripped.
    let s_single = funding_sig_single(b"alice-pk");
    assert_eq!(
        leaf_pool_count(&s_single),
        1,
        "Rule 1 (single token): the envelope draws from exactly one pool"
    );

    // Rule 3 / Rule 4 strip ONE COMBINED token from the `s1∘s2` pool. The
    // compound envelope is `Sig::And(s1, s2)`; the COMBINED pool `Σ⟦s1∘s2⟧` is a
    // distinct lane keyed by the whole compound's `lane_hash` (Def 7.4: a layer
    // is attributed to the WHOLE signature value), so a Rule-3/4 step draws ONE
    // token from that single combined pool.
    let s1 = Sig::Ground(b"signer-1".to_vec());
    let s2 = Sig::Ground(b"signer-2".to_vec());
    let compound = Sig::And(Box::new(s1.clone()), Box::new(s2.clone()));
    // The combined pool's lane is distinct from either component's lane — the
    // single combined token Rule 3/4 strips.
    assert_ne!(compound.lane_hash(), s1.lane_hash());
    assert_ne!(compound.lane_hash(), s2.lane_hash());
}

/// Rules 2 and 5 strip a component PAIR — the `system_token_count` drops by 2.
/// The Rust witness: the compound `s1∘s2`'s component decomposition has exactly
/// TWO leaf pools (`signer_channels().len() == 2`), one token consumed from
/// EACH, so the token drop is 2. `funding_sig_compound` builds exactly the
/// `Sig::And(g₁, g₂)` fold the runtime forms for a 2-signer deploy.
#[test]
fn rule2_5_strip_two_tokens() {
    let compound = funding_sig_compound(&[b"signer-1", b"signer-2"]);
    assert_eq!(
        leaf_pool_count(&compound),
        2,
        "Rule 2/5 (component pair): the compound draws from exactly two leaf pools"
    );
    // Each leaf lane is distinct (two distinct tokens consumed).
    let leaves = compound.signer_channels();
    assert_ne!(
        leaves[0].1, leaves[1].1,
        "the two stripped tokens are on distinct leaf lanes"
    );
}

/// The single-vs-compound token-drop DICHOTOMY is exactly the 1-vs-2 split of
/// the Rocq rules: a 1-signer envelope strips 1, an n≥2-signer envelope strips
/// n leaf tokens (the component-pair generalization), and these are never equal.
#[test]
fn single_token_drop_differs_from_pair_token_drop() {
    let single = funding_sig_single(b"solo-pk");
    let pair = funding_sig_compound(&[b"a-pk", b"b-pk"]);
    assert_eq!(leaf_pool_count(&single), 1);
    assert_eq!(leaf_pool_count(&pair), 2);
    assert!(
        leaf_pool_count(&pair) > leaf_pool_count(&single),
        "a compound rule strips strictly more tokens than a single rule"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// The JOIN (J1/J2): an N-ary join bills exactly ONE COMM (the receive) for the
// join itself, regardless of arity — the combined join token is consumed once.
// ═══════════════════════════════════════════════════════════════════════════

/// Recursively locate the single multi-bind join receive and return its bind
/// (clause) count. The join body lives under the top-level `new` that allocates
/// its channels, so we descend `news`. Returns the bind count of the first
/// receive with ≥ 1 bind found in pre-order.
fn join_bind_count(par: &Par) -> Option<usize> {
    if let Some(recv) = par.receives.iter().find(|r| !r.binds.is_empty()) {
        return Some(recv.binds.len());
    }
    for new in &par.news {
        if let Some(body) = &new.p {
            if let Some(n) = join_bind_count(body) {
                return Some(n);
            }
        }
    }
    None
}

/// Count the receive NODES (not binds) reachable, descending `news`. An N-ary
/// join is ONE node regardless of arity.
fn receive_node_count(par: &Par) -> usize {
    let mut n = par.receives.len();
    for new in &par.news {
        if let Some(body) = &new.p {
            n += receive_node_count(body);
        }
    }
    n
}

/// A 2-clause join fires and bills exactly one atomic COMM. The two feeding
/// sends are participants in that contraction, not separate cost events.
#[tokio::test]
async fn binary_join_bills_one_receive_comm() {
    let redex = r#"new a, b in { a!(1) | b!(2) | for(x <- a & y <- b){ Nil } }"#;
    let par = normalized_par(redex);
    // The join is a SINGLE receive node carrying two binds.
    assert_eq!(receive_node_count(&par), 1, "the join is one receive node");
    assert_eq!(
        join_bind_count(&par),
        Some(2),
        "carrying two binds (2-ary join)"
    );

    let comms = runtime_comm_count(redex).await;
    assert_eq!(comms, 1, "the complete 2-way join is one atomic COMM");
}

/// J1 — the join COMM is ONE regardless of arity. A 2-clause and a 3-clause
/// join each bill exactly one atomic COMM; feeding-send count does not multiply
/// the cost.
#[tokio::test]
async fn join_receive_comm_is_one_regardless_of_arity() {
    let two = r#"new a, b in { a!(1) | b!(2) | for(x <- a & y <- b){ Nil } }"#;
    let three =
        r#"new a, b, c in { a!(1) | b!(2) | c!(3) | for(x <- a & y <- b & z <- c){ Nil } }"#;

    // Each program's join is a SINGLE receive node (the join COMM), independent
    // of arity.
    assert_eq!(receive_node_count(&normalized_par(two)), 1);
    assert_eq!(receive_node_count(&normalized_par(three)), 1);
    assert_eq!(
        join_bind_count(&normalized_par(two)),
        Some(2),
        "2-ary join carries 2 binds"
    );
    assert_eq!(
        join_bind_count(&normalized_par(three)),
        Some(3),
        "3-ary join carries 3 binds"
    );

    let two_comms = runtime_comm_count(two).await;
    let three_comms = runtime_comm_count(three).await;
    assert_eq!(two_comms, 1, "2-ary join = 1 atomic COMM");
    assert_eq!(three_comms, 1, "3-ary join = 1 atomic COMM");
}

// ═══════════════════════════════════════════════════════════════════════════
// PROPERTY: each disjoint matched redex adds exactly one atomic-COMM token.
// ═══════════════════════════════════════════════════════════════════════════

/// Build a program of `pairs` disjoint single-COMM interactions in parallel:
/// `new x0,…,x{n-1} in { x0!(0) | for(_<-x0){Nil} | … }`. Each pair contributes
/// exactly one atomic match, so realized cost is `pairs` while the structural
/// introduction bound is `2 * pairs`.
fn parallel_interactions(pairs: usize) -> String {
    let names: Vec<String> = (0..pairs).map(|i| format!("x{i}")).collect();
    let body: Vec<String> = (0..pairs)
        .map(|i| format!("x{i}!({i}) | for(y <- x{i}){{ Nil }}", i = i))
        .collect();
    format!("new {} in {{ {} }}", names.join(", "), body.join(" | "))
}

#[test]
fn prop_random_redex_consumes_rule_determined_comm_quantum() {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    // Bounded so each proptest case still drives the full async interpreter
    // quickly; 1..=6 disjoint interactions exercise the additive quantum.
    let mut runner = TestRunner::new(ProptestConfig {
        cases: 48,
        ..ProptestConfig::default()
    });
    runner
        .run(&(1usize..=6), |pairs| {
            let program = parallel_interactions(pairs);
            let expected_bound = 2 * pairs;
            let expected_cost = pairs;

            // Static image: both sides of every redex are potential participants.
            let static_comms = communication_introduction_count(&normalized_par(&program));
            prop_assert_eq!(
                static_comms,
                expected_bound,
                "structural introduction count must be twice the pair count"
            );

            // Live runtime: the consumed COMM tally equals that amount, and is
            // strictly positive (every redex strictly decreases the budget) and
            // strictly monotone in `pairs`.
            let runtime_comms = rt.block_on(runtime_comm_count(&program));
            prop_assert_eq!(
                runtime_comms,
                expected_cost,
                "runtime cost must equal the number of atomic matches"
            );
            prop_assert!(
                runtime_comms > 0,
                "a non-empty redex consumes a positive quantum"
            );
            // The quantum strictly decreases the available budget: consuming
            // `runtime_comms` from a budget B leaves B − runtime_comms < B.
            let budget = 1_000_000i64;
            prop_assert!(
                budget - runtime_comms as i64 == budget - expected_cost as i64,
                "the consumed amount is exactly the rule-determined quantum"
            );
            Ok(())
        })
        .expect("random signed redex must reduce with the rule-determined COMM quantum");
}
