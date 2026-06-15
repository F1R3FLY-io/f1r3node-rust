//! WD-D1 acceptance tests for the pure `Δ_s`/`Σ_s` demand analyzer
//! (`accounting/delta_sigma.rs`).
//!
//! The headline test is the LOAD-BEARING EQUIVALENCE (consensus-critical, the
//! gate↔runtime bridge): the static `demand().known_lower_bound` MUST equal the
//! runtime's actual consumed token count — the number of
//! `BillableTokenEvent{kind: Comm}` events the reducer emits — for a funded
//! deploy that runs to completion. D3 (DR-9, OD-3): consensus cost = ONE token
//! per COMM (send/receive ONLY); `new`/`match`/`if` are diagnostic `Reduction`s
//! that contribute ZERO. This is the spec's "consumed = Δ_s", which
//! `replay_cost_mismatch` guards as `total_cost == consumed`. If this ever
//! diverges the acceptance gate would admit deploys the runtime cannot fund (or
//! reject fundable ones), forking consensus.
//!
//! We validate it against:
//!   * the cost-accounting paper's §7.4 debit/credit example, whose desugared
//!     form has **8 token-consuming COMMs** (the "8 not 6" semantic count after
//!     `?!` desugaring); D3 re-pins consensus cost to exactly that 8 (the outer
//!     `new` no longer counts — §7.4 "9 → 8"), and
//!   * the Appendix-B three-layer validator handler (5 COMMs under D3 — its
//!     outer `new` no longer counts, so 6 → 5).
//!
//! Both contracts are parsed through `Compiler::source_to_adt` — the SAME
//! normalizer path the runtime evaluates — so `demand` analyses exactly the `Par`
//! the runtime meters. `?!` is already desugared by the normalizer
//! (`p_send_sync_normalizer.rs`), so the `Par` is in the desugared form `demand`
//! requires (no re-desugaring in the analyzer — see `desugar_for_funding`).

use std::collections::BTreeMap;

use models::rhoapi::Par;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::delta_sigma::{
    demand, desugar_for_funding, effective_supply, effective_supply_with, is_funded, sig_key,
    Decomposition, DemandEntry,
};
use rholang::rust::interpreter::accounting::{BillableKind, Sig};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

/// A representative envelope signature (a single ground atom). Under the s₀
/// collapse the demand count is signature-agnostic, so any concrete `Sig` drives
/// the same node count; we use a ground atom to mirror a single-signer deploy.
fn envelope_sig() -> Sig { Sig::Ground(b"alice-envelope".to_vec()) }

async fn fresh_runtime() -> RhoRuntimeImpl {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.expect("in-memory rspace stores");
    let (runtime, _replay, _hist) = create_runtimes(store, false, &mut Vec::new()).await;
    runtime
}

/// Run `contract` to completion on a fresh runtime with an abundant budget and
/// return the runtime's consumed TOKEN count: the number of `Comm`
/// `BillableTokenEvent`s in the finalized canonical event log (D3/DR-9
/// token-per-COMM — each COMM is ONE token; `Reduction`/`Primitive`/
/// `Substitution` events are diagnostic and excluded from the consensus tally).
async fn runtime_consumed_token_count(contract: &str) -> usize {
    let mut runtime = fresh_runtime().await;
    let result = runtime
        .evaluate_with_phlo(contract, Cost::create(50_000_000, "delta_sigma_spec"))
        .await
        .expect("evaluate must not error at the harness level");
    assert!(
        result.errors.is_empty(),
        "contract must run to completion without errors: {:?}",
        result.errors
    );
    runtime
        .get_cost_event_log()
        .iter()
        .filter(|event| event.kind == BillableKind::Comm)
        .count()
}

/// Parse a contract to the normalized `Par` the runtime evaluates.
fn normalized_par(contract: &str) -> Par {
    Compiler::source_to_adt(contract).expect("contract must parse + normalize")
}

/// Recursively count token-consuming COMM nodes (sends + receives) ONLY,
/// excluding `new`/`match`/`if`. This is the cost-accounting paper's Def-17 §7.4
/// SEMANTIC count (the number of for-comprehensions/sends after `?!`
/// desugaring). Used to demonstrate the "8 not 6" property independently of the
/// runtime's additional metering of `new` nodes.
fn comm_node_count(par: &Par) -> usize {
    let mut n = 0;
    for send in &par.sends {
        n += 1;
        if let Some(chan) = &send.chan {
            n += comm_node_count(chan);
        }
        for datum in &send.data {
            n += comm_node_count(datum);
        }
    }
    for receive in &par.receives {
        n += 1;
        for bind in &receive.binds {
            if let Some(source) = &bind.source {
                n += comm_node_count(source);
            }
            for pattern in &bind.patterns {
                n += comm_node_count(pattern);
            }
        }
        if let Some(body) = &receive.body {
            n += comm_node_count(body);
        }
    }
    for new in &par.news {
        if let Some(body) = &new.p {
            n += comm_node_count(body);
        }
    }
    for mat in &par.matches {
        if let Some(target) = &mat.target {
            n += comm_node_count(target);
        }
        for case in &mat.cases {
            if let Some(source) = &case.source {
                n += comm_node_count(source);
            }
        }
    }
    for conditional in &par.conditionals {
        if let Some(condition) = &conditional.condition {
            n += comm_node_count(condition);
        }
        if let Some(if_true) = &conditional.if_true {
            n += comm_node_count(if_true);
        }
        if let Some(if_false) = &conditional.if_false {
            n += comm_node_count(if_false);
        }
    }
    for bundle in &par.bundles {
        if let Some(body) = &bundle.body {
            n += comm_node_count(body);
        }
    }
    n
}

// ═══════════════════════════════════════════════════════════════════════════
// THE LOAD-BEARING EQUIVALENCE: demand() == runtime consumed token count.
// ═══════════════════════════════════════════════════════════════════════════

/// §7.4 debit/credit orchestrator. Two synchronous round-trips driven by an
/// orchestrator against two reply-emitting handlers; fully reduces to `Nil`. The
/// desugared form has exactly **8 token-consuming COMMs** (4 sends + 4
/// for-comprehensions) — the paper's semantic count (Def 17, §7.4) — plus the
/// single outer `new` that allocates the channels. D3 (DR-9, OD-3): the
/// CONSENSUS cost is the COMM count = 8 (the outer `new` is a diagnostic
/// `Reduction`, NOT a `Comm`, so it no longer counts — this is the §7.4 "9 → 8"
/// re-pin); `demand` and the runtime's `Comm`-event count both equal 8.
const SEC_7_4_DEBIT_CREDIT: &str = r#"new d, c, dr, cr in {
    for(@x, ret <= d){ ret!(x) } |
    for(@y, ret <= c){ ret!(y) } |
    d!(1, *dr) |
    for(@z <- dr){ c!(z, *cr) | for(@w <- cr){ Nil } }
}"#;

/// Appendix-B validator handler shape: a fee-gate chain that receives a
/// deployment on `dq`, then a token stack on `ac`, then performs the fee
/// extraction send on `fee`. Three nested for-comprehensions plus the
/// FeeExtract send (the paper's three `{·}_v` signed layers), driven by two
/// setup sends, all under one `new`. Fully reduces.
const APP_B_HANDLER: &str = r#"new dq, ac, fee in {
    dq!("D") | ac!("ccc") |
    for(dep <- dq){ for(tok <- ac){ fee!(*dep, *tok) } }
}"#;

#[tokio::test]
async fn delta_s_equals_runtime_consumed_for_sec_7_4_example() {
    let par = normalized_par(SEC_7_4_DEBIT_CREDIT);

    // "8 not 6": the desugared §7.4 example has exactly 8 token-consuming COMMs
    // (sends + receives), the SEMANTIC count after `?!` desugaring — strictly
    // more than the 6 syntactic signed layers of the sugared surface form.
    let comms = comm_node_count(&par);
    assert_eq!(
        comms, 8,
        "the §7.4 desugared example must have 8 token-consuming COMMs (semantic count)"
    );

    // D3 (DR-9, OD-3): the static demand (per-COMM) must equal the runtime's
    // consumed COMM-event count exactly.
    let analysis = demand(&par, &envelope_sig());
    let runtime_consumed = runtime_consumed_token_count(SEC_7_4_DEBIT_CREDIT).await;

    assert!(
        !analysis.unknown,
        "the §7.4 example is fully statically resolvable (no unresolved *x)"
    );
    assert_eq!(
        analysis.known_lower_bound as usize, runtime_consumed,
        "Δ_s ({}) must equal the runtime consumed COMM count ({}) for the §7.4 example",
        analysis.known_lower_bound, runtime_consumed
    );
    // §7.4 "9 → 8": consensus cost = the 8 COMMs (the outer `new` no longer
    // counts). gate demand == runtime consumed == 8 == comm_node_count.
    assert_eq!(analysis.known_lower_bound, 8);
    assert_eq!(analysis.known_lower_bound as usize, comms);
}

#[tokio::test]
async fn delta_s_equals_runtime_consumed_for_app_b_handler() {
    let par = normalized_par(APP_B_HANDLER);

    let analysis = demand(&par, &envelope_sig());
    let runtime_consumed = runtime_consumed_token_count(APP_B_HANDLER).await;

    assert!(!analysis.unknown);
    assert_eq!(
        analysis.known_lower_bound as usize, runtime_consumed,
        "Δ_s ({}) must equal the runtime consumed token count ({}) for the App.B handler",
        analysis.known_lower_bound, runtime_consumed
    );
    // The App.B handler embeds the paper's 3 signed `{·}_v` layers; the desugared
    // realization meters 2 receives (the `for dep` / `for tok`) + 2 setup sends
    // (`dq!` / `ac!`) + 1 FeeExtract send (`fee!`) = 5 COMMs, under 1 `new`. D3
    // (DR-9, OD-3): consensus cost = the 5 COMMs (the `new` is a diagnostic
    // Reduction worth 0, so the App.B count drops 6 → 5). Pin the COMM core (>= 3
    // signed layers) and the total.
    assert!(
        comm_node_count(&par) >= 3,
        "the App.B handler must carry at least its 3 signed-layer COMMs"
    );
    assert_eq!(analysis.known_lower_bound, 5);
    // gate demand == runtime consumed == comm_node_count, all per-COMM.
    assert_eq!(analysis.known_lower_bound as usize, comm_node_count(&par));
}

/// D3 (DR-9, OD-3) — GATE DEMAND == RUNTIME COMM COUNT. The block-assembly
/// gate's static `demand().known_lower_bound` MUST equal the runtime's actual
/// consumed COMM-event count for every funded, fully-reducing deploy. This is
/// the consensus-critical D1→D3 bridge (the gate admits exactly what the runtime
/// consumes); the explicit pin complements the §7.4 / App.B headline examples.
#[tokio::test]
async fn gate_demand_equals_runtime_comm_count() {
    let contracts = [
        SEC_7_4_DEBIT_CREDIT,
        APP_B_HANDLER,
        r#"@"a"!(1)"#,
        r#"new x in { x!(1) | for(y <- x){ Nil } }"#,
        r#"new x, r in { x!(1) | for(y <- x){ r!(*y) } | for(z <- r){ Nil } }"#,
    ];
    for contract in contracts {
        let par = normalized_par(contract);
        let demand_count = demand(&par, &envelope_sig()).known_lower_bound;
        let runtime_comm_count = runtime_consumed_token_count(contract).await as i64;
        let comm_nodes = comm_node_count(&par) as i64;
        assert_eq!(
            demand_count, runtime_comm_count,
            "gate demand ({}) must equal runtime COMM count ({}) for: {}",
            demand_count, runtime_comm_count, contract
        );
        assert_eq!(
            demand_count, comm_nodes,
            "gate demand ({}) must equal the static COMM-node count ({}) for: {}",
            demand_count, comm_nodes, contract
        );
    }
}

/// D3 (DR-9, OD-3) — SETTLEMENT DEBIT == COMM COUNT. The per-pool settlement
/// debit the gate accumulates (`acceptance.rs`: `Σ demand.known_lower_bound`
/// over the admitted prefix) is, for a single admitted deploy, exactly that
/// deploy's per-COMM demand. With a zero safety margin and a supply that exactly
/// meets the demand, the admitted deploy's debit equals its COMM count — so
/// `post Σ⟦s⟧ = pre − COMM_count`. We pin this debit==COMM identity directly on
/// the analyzer (the gate's `is_funded` admits iff `Σ ≥ Δ` for resolvable demand
/// — the margin applies only to over-approximated `unknown` demand — and the
/// debit it then subtracts is `Δ`, the COMM count).
#[tokio::test]
async fn settlement_debit_equals_comm_count() {
    for contract in [
        SEC_7_4_DEBIT_CREDIT,
        APP_B_HANDLER,
        r#"@"a"!(1) | @"b"!(2)"#,
    ] {
        let par = normalized_par(contract);
        let analysis = demand(&par, &envelope_sig());
        let comm_count = comm_node_count(&par) as i64;
        // The demand (== the debit the gate subtracts) is the COMM count.
        assert_eq!(analysis.known_lower_bound, comm_count);
        // A supply that exactly meets the demand (margin 0) admits the deploy;
        // the debit then drives `post = pre − Δ = pre − comm_count`.
        let supply = analysis.known_lower_bound;
        assert!(
            is_funded(&analysis, supply, 0),
            "Σ = Δ must admit at margin 0"
        );
        let post = supply - analysis.known_lower_bound; // the settlement write.
        assert_eq!(post, 0, "post Σ⟦s⟧ = pre − COMM_count must be exact");
    }
}

/// Cross-check on smaller fully-reducing deploys to widen the equivalence
/// evidence beyond the two headline examples.
#[tokio::test]
async fn delta_s_equals_runtime_consumed_across_assorted_deploys() {
    // D3 (DR-9, OD-3): per-COMM counts (send/receive only; `new` is a diagnostic
    // Reduction worth 0). One send ⇒ 1. `new x in { x!(1) | for(y<-x){Nil} }` ⇒
    // 1 send + 1 receive = 2 (the `new` no longer counts). The third adds one
    // more send in the receive body ⇒ 3.
    let cases = [
        (r#"@"a"!(1)"#, 1_i64),
        (r#"new x in { x!(1) | for(y <- x){ Nil } }"#, 2),
        (r#"new x, r in { x!(1) | for(y <- x){ r!(*y) } }"#, 3),
    ];
    for (contract, expected) in cases {
        let par = normalized_par(contract);
        let analysis = demand(&par, &envelope_sig());
        let runtime_consumed = runtime_consumed_token_count(contract).await;
        assert!(
            !analysis.unknown,
            "contract should be resolvable: {contract}"
        );
        assert_eq!(
            analysis.known_lower_bound as usize, runtime_consumed,
            "Δ_s must equal runtime consumed for: {contract}"
        );
        assert_eq!(
            analysis.known_lower_bound, expected,
            "Δ_s for {contract} should be {expected}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// desugar_for_funding: identity on a normalized Par (the normalizer already
// desugared `?!`).
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn normalizer_already_desugars_sync_send() {
    // A `?!` synchronous send normalizes to `new ret in { chan!(ret,..) | for(..) }`
    // — so the normalized Par already contains a `new`, a send, and a receive.
    // `desugar_for_funding` is the identity on it (no double-expansion).
    let par =
        normalized_par(r#"new s in { for(@v, r <= s){ r!(v) } | for(reply <- s!?(1)){ Nil } }"#);
    assert_eq!(desugar_for_funding(&par), par);
    // The desugared `Par` must contain at least one receive (the `?!`'s reply
    // for-comprehension) AND at least one send (the `?!`'s call send) — evidence
    // the sync-send sugar was expanded to send + for by the normalizer.
    let analysis = demand(&par, &envelope_sig());
    assert!(
        analysis.known_lower_bound >= 2,
        "a desugared ?! must contribute at least a send + a for"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// effective_supply: the Split/Join closure over real Sig::lane_hash keys.
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn effective_supply_closure_over_real_lane_hashes() {
    // Build a compound `And(s1, s2)` and key the supply map by the SAME
    // canonical lane-hash basis the gate and supply channel use.
    let s1 = Sig::Ground(b"signer-1".to_vec());
    let s2 = Sig::Ground(b"signer-2".to_vec());
    let compound = Sig::And(Box::new(s1.clone()), Box::new(s2.clone()));

    let key_s1 = sig_key(&s1);
    let key_s2 = sig_key(&s2);
    let key_compound = sig_key(&compound);

    let mut raw = BTreeMap::new();
    raw.insert(key_s1, 4_i64);
    raw.insert(key_s2, 6_i64);
    raw.insert(key_compound, 10_i64);

    let effective = effective_supply_with(&raw, &[Decomposition {
        compound: key_compound,
        left: key_s1,
        right: key_s2,
    }]);

    // effectiveΣ_{s1∘s2} = 10 + min(4,6) = 14
    // effectiveΣ_{s1}    = 4 + 10        = 14
    // effectiveΣ_{s2}    = 6 + 10        = 16
    assert_eq!(effective.get(&key_compound), Some(&14));
    assert_eq!(effective.get(&key_s1), Some(&14));
    assert_eq!(effective.get(&key_s2), Some(&16));

    // The no-decomposition closure is the identity (single-signer fast path).
    assert_eq!(effective_supply(&raw), raw);
}

// ═══════════════════════════════════════════════════════════════════════════
// is_funded: Def 19 + Thm 20 over-approximation at the ±margin boundaries,
// including the unknown-reject direction. (Boundary arithmetic is also unit-
// tested in-module; here we exercise it against a real analyzed deploy so the
// integration path — demand → is_funded — is covered end to end.)
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn is_funded_gate_at_def19_boundary_for_real_demand() {
    // Analyze a real fully-reducing deploy: D3 per-COMM Δ = 8 for §7.4.
    let analysis = demand(&normalized_par(SEC_7_4_DEBIT_CREDIT), &envelope_sig());
    assert_eq!(analysis.known_lower_bound, 8);
    assert!(!analysis.unknown);

    // F-B: for fully-resolvable demand the gate is EXACTLY Def 19 `Σ_s ≥ Δ_s` —
    // the economic margin (`min_phlo_price`) is NOT folded into the correctness
    // inequality, so a non-zero margin must NOT shift the known-demand boundary.
    let margin = 2_i64;
    // Σ = Δ - 1 = 7 ⇒ reject (under the exact demand).
    assert!(!is_funded(&analysis, 7, margin));
    // Σ = Δ = 8 ⇒ accept (Def 19 boundary; the margin does NOT apply to known demand).
    assert!(is_funded(&analysis, 8, margin));
    // Σ = Δ + margin - 1 = 9 ⇒ accept (this was REJECTED before the F-B fix).
    assert!(is_funded(&analysis, 9, margin));
    // Σ well above ⇒ accept.
    assert!(is_funded(&analysis, 100, margin));
    // The margin is inert for known demand: identical verdict at margin 0.
    assert_eq!(is_funded(&analysis, 8, 0), is_funded(&analysis, 8, margin));
}

#[tokio::test]
async fn is_funded_unknown_demand_rejected_unless_lower_bound_plus_margin_met() {
    // A deploy that dequotes an unbound name (`*x` of a name received at runtime)
    // is NOT statically resolvable ⇒ unknown demand. The gate must reject it
    // unless the supply clears the KNOWN lower bound plus the margin (Thm 20 safe
    // direction). We construct an analysis with the unknown flag directly to
    // pin the boundary precisely (the in-module tests cover the AST trigger).
    let analysis = DemandEntry {
        known_lower_bound: 5,
        unknown: true,
    };
    let margin = 4_i64;

    // Σ = Δ_known = 5 (margin unmet) ⇒ reject even though Σ ≥ known lower bound.
    assert!(!is_funded(&analysis, 5, margin));
    // Σ = Δ_known + margin - 1 = 8 ⇒ still reject.
    assert!(!is_funded(&analysis, 8, margin));
    // Σ = Δ_known + margin = 9 ⇒ accept (margin headroom cleared).
    assert!(is_funded(&analysis, 9, margin));
}
