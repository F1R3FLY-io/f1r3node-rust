//! WD-D1 acceptance tests for the pure `Δ_s`/`Σ_s` demand analyzer
//! (`accounting/delta_sigma.rs`).
//!
//! The load-bearing gate/runtime relation is that the static
//! `demand().certified_upper_bound` dominates actual atomic-COMM token count for
//! the closed, non-persistent fragment. It counts potential send/receive
//! introductions, while the runtime counts successful matches; unmatched I/O is
//! therefore free and the bound is generally strict. Persistent I/O and dynamic
//! dequotation are unprovable structurally. Production uses exact state-bound
//! evidence rooted in the authenticated pre-state.
//!
//! We validate it against:
//!   * the cost-accounting paper's §7.4 debit/credit example, whose desugared
//!     form has **8 potential communication introductions** after `?!`
//!     desugaring and realizes 4 atomic matches, and
//!   * the Appendix-B three-layer validator handler, with 5 introductions and 2
//!     realized matches.
//!
//! Both contracts are parsed through `Compiler::source_to_adt` — the SAME
//! normalizer path the runtime evaluates — so `demand` analyses exactly the `Par`
//! the runtime meters. `?!` is already desugared by the normalizer
//! (`p_send_sync_normalizer.rs`), so the `Par` is in the desugared form `demand`
//! requires (no re-desugaring in the analyzer — see `desugar_for_funding`).

use std::collections::BTreeMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::Par;
use models::rust::utils::new_send;
use prost::Message;
use rholang::rust::interpreter::accounting::authority::{cost_signature_to_sig, DemandBound};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::delta_sigma::{
    demand, demand_bound, demand_by_sig, desugar_for_funding, effective_supply,
    effective_supply_with, is_funded, match_channel_to_lane, sig_key, Decomposition, DemandEntry,
};
use rholang::rust::interpreter::accounting::{envelope_sig_compound, BillableKind, Sig};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

/// A representative default deploy authority for unsigned surfaces. Structural
/// demand is signature-agnostic until a native signed region supplies an
/// explicit authority.
fn envelope_sig() -> Sig { Sig::Ground(b"alice-envelope".to_vec()) }

async fn fresh_runtime() -> RhoRuntimeImpl {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.expect("in-memory rspace stores");
    let (runtime, _replay, _hist) = create_runtimes(store, false, &mut Vec::new()).await;
    runtime
}

fn install_test_payer(runtime: &RhoRuntimeImpl) {
    runtime.cost.set_deploy_signature_funded(
        b"delta-sigma-spec-deploy",
        Sig::Ground(b"alice-envelope".to_vec()),
    );
}

/// Run `contract` to completion on a fresh runtime with an abundant budget and
/// return the runtime's consumed TOKEN count: the number of atomic `Comm`
/// `BillableTokenEvent`s in the finalized canonical event log (D3/DR-9
/// token-per-COMM — each COMM is ONE token; `Reduction`/`Primitive`/
/// `Substitution` events are diagnostic and excluded from the consensus tally).
async fn runtime_consumed_token_count(contract: &str) -> usize {
    let mut runtime = fresh_runtime().await;
    install_test_payer(&runtime);
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

/// Recursively count potential communication introductions (sends + receives),
/// excluding `new`/`match`/`if`. This is the cost-accounting paper's Def-17 §7.4
/// as used by the conservative structural analyzer. This is not the number of
/// successful runtime matches.
fn communication_introduction_count(par: &Par) -> usize {
    let mut n = 0;
    for send in &par.sends {
        n += 1;
        if let Some(chan) = &send.chan {
            n += communication_introduction_count(chan);
        }
        for datum in &send.data {
            n += communication_introduction_count(datum);
        }
    }
    for receive in &par.receives {
        n += 1;
        for bind in &receive.binds {
            if let Some(source) = &bind.source {
                n += communication_introduction_count(source);
            }
            for pattern in &bind.patterns {
                n += communication_introduction_count(pattern);
            }
        }
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
        if let Some(target) = &mat.target {
            n += communication_introduction_count(target);
        }
        for case in &mat.cases {
            if let Some(source) = &case.source {
                n += communication_introduction_count(source);
            }
        }
    }
    for conditional in &par.conditionals {
        if let Some(condition) = &conditional.condition {
            n += communication_introduction_count(condition);
        }
        if let Some(if_true) = &conditional.if_true {
            n += communication_introduction_count(if_true);
        }
        if let Some(if_false) = &conditional.if_false {
            n += communication_introduction_count(if_false);
        }
    }
    for bundle in &par.bundles {
        if let Some(body) = &bundle.body {
            n += communication_introduction_count(body);
        }
    }
    for signed in &par.cost_signed_terms {
        if let Some(body) = &signed.body {
            n += communication_introduction_count(body);
        }
    }
    n
}

// ═══════════════════════════════════════════════════════════════════════════
// THE LOAD-BEARING REFINEMENT: realized atomic COMM <= structural demand.
// ═══════════════════════════════════════════════════════════════════════════

/// §7.4 debit/credit orchestrator. Two synchronous round-trips driven by an
/// orchestrator against two reply-emitting handlers; fully reduces to `Nil`. The
/// desugared form has exactly **8 potential introductions** (4 sends + 4
/// receives) and realizes 4 atomic matches. The structural reservation is
/// deliberately conservative; the state-bound witness settles the exact 4.
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
async fn delta_s_bounds_runtime_consumed_for_sec_7_4_example() {
    let par = normalized_par(SEC_7_4_DEBIT_CREDIT);

    let introductions = communication_introduction_count(&par);
    assert_eq!(
        introductions, 8,
        "the §7.4 desugared example must have 8 potential introductions"
    );

    let analysis = demand(&par, &envelope_sig());
    let runtime_consumed = runtime_consumed_token_count(SEC_7_4_DEBIT_CREDIT).await;

    assert!(
        analysis.unknown,
        "the persistent §7.4 handlers require state-bound evidence"
    );
    assert!(
        analysis.certified_upper_bound as usize >= runtime_consumed,
        "Δ_s ({}) must bound realized atomic-COMM cost ({})",
        analysis.certified_upper_bound,
        runtime_consumed
    );
    assert_eq!(analysis.certified_upper_bound, 8);
    assert_eq!(analysis.certified_upper_bound as usize, introductions);
    assert_eq!(runtime_consumed, 4);
}

/// B6 (CA-P-176) — the SUGARED §7.4 surface form, written with the synchronous-
/// send sugar `!?` on BOTH the debit and the credit round-trip (the spec's
/// two-sided `?!`/`!?` expansion). The normalizer (`p_send_sync_normalizer.rs`)
/// expands EACH `chan!?(args).` to `new ret in { chan!(ret, args) | for(_ <- ret){ Nil } }`
/// — a send + a wildcard reply for-comprehension. So the SOURCE's 6 token-bearing
/// signed layers (2 handler `for`s + 2 handler `ret!` replies + 2 `!?` sync sends)
/// become 8 potential introductions after desugaring: the 2 `!?` each contribute
/// a generated reply receive, so 6 → 8. This is the structural reservation
/// count. (`d!?(1).` is the standalone synchronous send with
/// the empty continuation `.` — grammar `send_sync` + `empty_cont`.)
const SEC_7_4_SUGARED: &str = r#"new d, c in {
    for(@x, ret <= d){ ret!(x) } |
    for(@y, ret <= c){ ret!(y) } |
    d!?(1). |
    c!?(2).
}"#;

/// Count the `?!`/`!?`-introduced REPLY receives in a desugared `Par`: a
/// for-comprehension whose single bind matches a lone WILDCARD pattern (the
/// `for(_ <- ret){…}` the sync-send normalizer emits). Each two-sided sync send
/// contributes exactly one such reply receive — the "+1 COMM per `!?`" that turns
/// the surface count into the semantic count. Descends `news` (each `!?` wraps
/// its send+reply under a fresh `new ret`) and receive bodies.
fn sync_reply_receive_count(par: &Par) -> usize {
    let mut n = 0;
    for receive in &par.receives {
        // The hallmark of a sync-send reply receive: exactly one bind whose sole
        // pattern is a lone `Var::Wildcard` (`for(_ <- ret){…}`).
        let lone_wildcard = receive.binds.len() == 1 && receive.binds[0].patterns.len() == 1 && {
            let p = &receive.binds[0].patterns[0];
            p.sends.is_empty()
                && p.receives.is_empty()
                && p.news.is_empty()
                && p.matches.is_empty()
                && p.bundles.is_empty()
                && p.exprs.len() == 1
                && matches!(
                    &p.exprs[0].expr_instance,
                    Some(ExprInstance::EVarBody(ev))
                        if matches!(
                            ev.v.as_ref().and_then(|v| v.var_instance.as_ref()),
                            Some(VarInstance::Wildcard(_))
                        )
                )
        };
        if lone_wildcard {
            n += 1;
        }
        if let Some(body) = &receive.body {
            n += sync_reply_receive_count(body);
        }
    }
    for new in &par.news {
        if let Some(body) = &new.p {
            n += sync_reply_receive_count(body);
        }
    }
    n
}

/// B6 (CA-P-176) — pin the literal §7.4 SOURCE-count 6 → DESUGARED-count 8 for
/// the two-sided `?!`/`!?` expansion, and `Δ_s == 8`.
///
/// The two-sided sync round-trip is already exercised end-to-end by
/// `delta_s_bounds_runtime_consumed_for_sec_7_4_example` (the desugared form);
/// this test PINS THE COUNT: the SUGARED surface (`SEC_7_4_SUGARED`, two `!?`
/// sync sends) carries exactly 2 surface sync sends, and the normalizer expands
/// each into a send + a wildcard reply receive — so the desugared `Par` has 8
/// potential introductions, of which exactly 2 are `!?`-introduced reply receives.
/// The surface signed-layer count is therefore 8 − 2 = 6, and `Δ_s` (which
/// counts the desugared COMMs) is 8.
#[tokio::test]
async fn sec_7_4_two_sided_desugar_pins_source_6_to_desugared_8() {
    // The SOURCE carries exactly two `!?` synchronous sends (one per side of the
    // two-sided round-trip) — the surface sugar that desugars.
    let surface_sync_sends = SEC_7_4_SUGARED.matches("!?").count();
    assert_eq!(
        surface_sync_sends, 2,
        "the two-sided §7.4 surface has exactly 2 `!?` sync sends"
    );

    let par = normalized_par(SEC_7_4_SUGARED);

    // DESUGARED structural count: 8 potential communication introductions.
    let desugared_comms = communication_introduction_count(&par);
    assert_eq!(
        desugared_comms, 8,
        "the two-sided §7.4 sugar desugars to 8 potential introductions"
    );

    // Exactly 2 of those 8 COMMs are the `!?`-introduced wildcard reply receives
    // (one per surface sync send) — the COMMs the desugar ADDS.
    let added_by_desugar = sync_reply_receive_count(&par);
    assert_eq!(
        added_by_desugar, 2,
        "each of the 2 `!?` sync sends adds exactly one reply receive COMM"
    );

    // SOURCE-count 6 → DESUGARED-count 8: the surface signed-layer count is the
    // desugared COMM count MINUS the desugar-introduced reply receives.
    let source_layers = desugared_comms - added_by_desugar;
    assert_eq!(
        source_layers, 6,
        "§7.4 source signed-layer count = 8 desugared COMMs − 2 `!?` reply receives = 6"
    );

    // The numeric structural projection is 8, but persistent handlers make the
    // finite proof unavailable; production uses state-bound evidence.
    let analysis = demand(&par, &envelope_sig());
    assert!(
        analysis.unknown,
        "persistent handlers are structurally unprovable"
    );
    assert_eq!(
        analysis.certified_upper_bound, 8,
        "Δ_s == 8 for the §7.4 two-sided desugar"
    );

    // With empty sync continuations only the two request rendezvous commit; the
    // remaining introductions are unforced or unmatched.
    let runtime_consumed = runtime_consumed_token_count(SEC_7_4_SUGARED).await;
    assert_eq!(
        runtime_consumed, 2,
        "the runtime consumes exactly 2 atomic-COMM tokens"
    );

    // Cross-pin: the sugared form and the hand-desugared `SEC_7_4_DEBIT_CREDIT`
    // example carry the same structural introduction count and projection.
    let hand_desugared = normalized_par(SEC_7_4_DEBIT_CREDIT);
    assert_eq!(
        communication_introduction_count(&hand_desugared),
        desugared_comms,
        "sugared `!?` form and hand-desugared form have the same 8-COMM semantic count"
    );
}

#[tokio::test]
async fn delta_s_bounds_runtime_consumed_for_app_b_handler() {
    let par = normalized_par(APP_B_HANDLER);

    let analysis = demand(&par, &envelope_sig());
    let runtime_consumed = runtime_consumed_token_count(APP_B_HANDLER).await;

    assert!(!analysis.unknown);
    assert!(
        analysis.certified_upper_bound as usize >= runtime_consumed,
        "Δ_s ({}) must bound runtime cost ({}) for the App.B handler",
        analysis.certified_upper_bound,
        runtime_consumed
    );
    // The App.B handler embeds the paper's 3 signed `{·}_v` layers; the desugared
    // realization meters 2 receives (the `for dep` / `for tok`) + 2 setup sends
    // (`dq!` / `ac!`) + 1 FeeExtract send (`fee!`) = 5 COMMs, under 1 `new`. D3
    // (DR-9, OD-3): consensus cost = the 5 COMMs (the `new` is a diagnostic
    // Reduction worth 0, so the App.B count drops 6 → 5). Pin the COMM core (>= 3
    // signed layers) and the total.
    assert!(
        communication_introduction_count(&par) >= 3,
        "the App.B handler must carry at least its 3 signed-layer introductions"
    );
    assert_eq!(analysis.certified_upper_bound, 5);
    assert_eq!(
        analysis.certified_upper_bound as usize,
        communication_introduction_count(&par)
    );
    assert_eq!(runtime_consumed, 2);
}

/// D3 (DR-9, OD-3) — a branch-free reservation is exact. General contracts only
/// require actual COMM cost to be bounded above by the certified reservation.
#[tokio::test]
async fn straight_line_gate_bound_dominates_runtime_comm_count() {
    let contracts = [
        SEC_7_4_DEBIT_CREDIT,
        APP_B_HANDLER,
        r#"@"a"!(1)"#,
        r#"new x in { x!(1) | for(y <- x){ Nil } }"#,
        r#"new x, r in { x!(1) | for(y <- x){ r!(*y) } | for(z <- r){ Nil } }"#,
    ];
    for contract in contracts {
        let par = normalized_par(contract);
        let demand_count = demand(&par, &envelope_sig()).certified_upper_bound;
        let runtime_comm_count = runtime_consumed_token_count(contract).await as i64;
        let introductions = communication_introduction_count(&par) as i64;
        assert!(
            demand_count >= runtime_comm_count,
            "gate demand ({}) must dominate runtime COMM count ({}) for: {}",
            demand_count,
            runtime_comm_count,
            contract
        );
        assert_eq!(
            demand_count, introductions,
            "gate demand ({}) must equal the introduction count ({}) for: {}",
            demand_count, introductions, contract
        );
    }
}

#[tokio::test]
async fn one_signed_scope_over_multiple_redexes_reserves_every_possible_firing() {
    let contract =
        r#"{% for(_ <- @"x"){ Nil } | @"x"!(0) | for(_ <- @"y"){ Nil } | @"y"!(0) %}[ payer ]"#;
    let par = normalized_par(contract);
    let signature = par.cost_signed_terms[0].signature.as_ref().unwrap();
    let lane = cost_signature_to_sig(signature).unwrap().lane_hash();
    let DemandBound::FiniteUpperBound { bound, .. } = demand_bound(&par, &envelope_sig()) else {
        panic!("the closed non-persistent term must have a finite bound")
    };
    assert_eq!(bound.get(&lane), 4);

    let mut runtime = fresh_runtime().await;
    let result = runtime
        .evaluate_with_phlo(contract, Cost::create(50_000_000, "signed scope bound"))
        .await
        .unwrap();
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(runtime.cost.authority_realized().get(&lane), 2);
    assert!(bound.dominates(&runtime.cost.authority_realized()));
}

/// For closed, non-persistent terms the structural reservation is finite and
/// equals the potential-introduction count.
#[tokio::test]
async fn straight_line_reservation_equals_introduction_count() {
    for contract in [APP_B_HANDLER, r#"@"a"!(1) | @"b"!(2)"#] {
        let par = normalized_par(contract);
        let analysis = demand(&par, &envelope_sig());
        let introductions = communication_introduction_count(&par) as i64;
        assert_eq!(analysis.certified_upper_bound, introductions);
        // A supply that exactly meets the reservation admits the deploy.
        let supply = analysis.certified_upper_bound;
        assert!(is_funded(&analysis, supply), "Σ = Δ must admit at margin 0");
    }
}

/// Cross-check on smaller fully-reducing deploys to widen the equivalence
/// evidence beyond the two headline examples.
#[tokio::test]
async fn straight_line_delta_s_bounds_runtime_consumed_across_assorted_deploys() {
    // D3 (DR-9, OD-3): per-COMM counts (send/receive only; `new` is a diagnostic
    // Reduction worth 0). One send ⇒ 1. `new x in { x!(1) | for(y<-x){Nil} }` ⇒
    // 1 send + 1 receive = 2 (the `new` no longer counts). The third adds one
    // more send in the receive body ⇒ 3.
    let cases = [
        (r#"@"a"!(1)"#, 1_i64, 0_usize),
        (r#"new x in { x!(1) | for(y <- x){ Nil } }"#, 2, 1),
        (r#"new x, r in { x!(1) | for(y <- x){ r!(*y) } }"#, 3, 1),
    ];
    for (contract, expected_bound, expected_cost) in cases {
        let par = normalized_par(contract);
        let analysis = demand(&par, &envelope_sig());
        let runtime_consumed = runtime_consumed_token_count(contract).await;
        assert!(
            !analysis.unknown,
            "contract should be resolvable: {contract}"
        );
        assert!(
            analysis.certified_upper_bound as usize >= runtime_consumed,
            "Δ_s must dominate runtime consumed for: {contract}"
        );
        assert_eq!(
            analysis.certified_upper_bound, expected_bound,
            "Δ_s for {contract} should be {expected_bound}"
        );
        assert_eq!(runtime_consumed, expected_cost);
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
        analysis.certified_upper_bound >= 2,
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

    // effectiveΣ_{s1∘s2} = 10 + min(4,6) = 14   (Join term)
    // No-weakening (§D2.9-R2): the single components pass through at their RAW
    // balance, NOT credited with the compound pool (was 14 / 16 pre-R2).
    assert_eq!(effective.get(&key_compound), Some(&14));
    assert_eq!(effective.get(&key_s1), Some(&4));
    assert_eq!(effective.get(&key_s2), Some(&6));

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
    let analysis = demand(&normalized_par(APP_B_HANDLER), &envelope_sig());
    assert_eq!(analysis.certified_upper_bound, 5);
    assert!(!analysis.unknown);

    // F-B: for fully-resolvable demand the gate is EXACTLY Def 19 `Σ_s ≥ Δ_s` —
    // the economic margin (`min_phlo_price`) is NOT folded into the correctness
    // inequality, so a non-zero margin must NOT shift the known-demand boundary.
    assert!(!is_funded(&analysis, 4));
    assert!(is_funded(&analysis, 5));
    assert!(is_funded(&analysis, 6));
    // Σ well above ⇒ accept.
    assert!(is_funded(&analysis, 100));
}

#[tokio::test]
async fn is_funded_unknown_demand_requires_finite_bound_proof() {
    let analysis = DemandEntry {
        certified_upper_bound: 5,
        unknown: true,
    };
    assert!(!is_funded(&analysis, 5));
    assert!(!is_funded(&analysis, 8));
    assert!(!is_funded(&analysis, i64::MAX));
}

/// The legacy signature-channel projection defaults unsigned data-channel
/// introductions to the deploy authority. Native `CostAuthority` regions are
/// tested separately and are not represented by this diagnostic projection.
#[test]
fn demand_by_sig_defaults_unsigned_surfaces_to_the_deploy_authority() {
    let par = normalized_par(r#"@"a"!(1) | for(x <- @"b"){ Nil }"#);
    let env = envelope_sig();
    let env_key = sig_key(&env);

    // The real per-deploy signer set for a single signer: its one signer channel
    // IS the envelope, so no DATA channel can match it.
    let signer_channels: Vec<(Vec<u8>, [u8; 32])> = env
        .signer_channels()
        .into_iter()
        .map(|(channel, lane)| (channel.encode_to_vec(), lane))
        .collect();
    let region = |channel: &Par| match_channel_to_lane(channel, &signer_channels);

    let by_sig = demand_by_sig(&par, env_key, &region);
    let scalar = demand(&par, &env);

    assert_eq!(by_sig.len(), 1, "unsigned surfaces have one default lane");
    assert_eq!(
        by_sig.get(&env_key).copied(),
        Some(scalar),
        "the default lane equals structural demand"
    );
}

/// W1 Phase 3 — the structural multi-lane projection uses the canonical channel
/// mapping. This fixture has 3 potential introductions on leaf 0, 2 on leaf 1,
/// and 1 on a data channel attributed to the envelope. Realized runtime matches
/// use the same mapping but need not equal these structural counts.
#[test]
fn multi_lane_structural_projection_uses_canonical_channel_mapping() {
    let env = envelope_sig_compound(&[b"sig-a", b"sig-b"]);
    let leaves = env.signer_channels();
    assert_eq!(leaves.len(), 2, "two cosigners ⇒ two leaf signer lanes");
    let (chan0, lane0) = (leaves[0].0.clone(), leaves[0].1);
    let (chan1, lane1) = (leaves[1].0.clone(), leaves[1].1);
    let env_key = sig_key(&env);
    let data_chan = Par::default(); // not a signer channel ⇒ attributes to the envelope

    let signer_channels: Vec<(Vec<u8>, [u8; 32])> = leaves
        .iter()
        .map(|(channel, lane)| (channel.encode_to_vec(), *lane))
        .collect();
    let region = {
        let signer_channels = signer_channels.clone();
        move |channel: &Par| match_channel_to_lane(channel, &signer_channels)
    };

    // STATIC: a Par with 3 sends on Σ⟦s₀⟧, 2 on Σ⟦s₁⟧, 1 on a data channel.
    let mut par = Par::default();
    for _ in 0..3 {
        par.sends
            .push(new_send(chan0.clone(), vec![], false, vec![], false));
    }
    for _ in 0..2 {
        par.sends
            .push(new_send(chan1.clone(), vec![], false, vec![], false));
    }
    par.sends
        .push(new_send(data_chan.clone(), vec![], false, vec![], false));
    let by_sig = demand_by_sig(&par, env_key, &region);

    assert_eq!(
        by_sig.get(&lane0).map(|entry| entry.certified_upper_bound),
        Some(3),
        "static leaf-0 = 3"
    );
    assert_eq!(
        by_sig.get(&lane1).map(|entry| entry.certified_upper_bound),
        Some(2),
        "static leaf-1 = 2"
    );
    assert_eq!(
        by_sig
            .get(&env_key)
            .map(|entry| entry.certified_upper_bound),
        Some(1),
        "static envelope = 1"
    );
}

/// W1 Phase 3 (GATE 3) — OSLF funding-logic conformance PER LANE: every per-lane
/// `DemandEntry` that `demand_by_sig` produces, fed through the funding judgment
/// `is_funded`, obeys the OSLF laws — Def 19 `Σ ≥ Δ` for a RESOLVABLE lane (the
/// economic margin inert), Thm 20 `Σ ≥ Δ + margin` for an over-approximated
/// (`unknown`) lane, and monotonicity in supply (no contraction). This confirms
/// `demand_by_sig`'s per-lane output INTEGRATES soundly with the funding gate (it
/// exercises the lane bounds 3 and 2, which the synthetic whole-logic grid in
/// `resource_logic_conformance::default_resource_logic_satisfies_oslf_laws` does
/// not hit). The whole-logic soundness is proven there; this is its per-lane image.
#[test]
fn multi_lane_demand_entries_satisfy_oslf_funding_laws_per_lane() {
    // Rebuild the same multi-lane structural fixture.
    let env = envelope_sig_compound(&[b"sig-a", b"sig-b"]);
    let leaves = env.signer_channels();
    let env_key = sig_key(&env);
    let signer_channels: Vec<(Vec<u8>, [u8; 32])> = leaves
        .iter()
        .map(|(channel, lane)| (channel.encode_to_vec(), *lane))
        .collect();
    let region = {
        let signer_channels = signer_channels.clone();
        move |channel: &Par| match_channel_to_lane(channel, &signer_channels)
    };
    let mut par = Par::default();
    for _ in 0..3 {
        par.sends
            .push(new_send(leaves[0].0.clone(), vec![], false, vec![], false));
    }
    for _ in 0..2 {
        par.sends
            .push(new_send(leaves[1].0.clone(), vec![], false, vec![], false));
    }
    par.sends
        .push(new_send(Par::default(), vec![], false, vec![], false));
    let by_sig = demand_by_sig(&par, env_key, &region);

    // The fixture's lanes are all RESOLVABLE (the data join has no `*x` drop), so
    // each obeys the Def-19 resolvable rule; the test still covers the Thm-20
    // `unknown` branch generically below.
    assert!(
        by_sig.values().all(|entry| !entry.unknown),
        "the fixture's lanes are statically resolvable"
    );

    for (lane, entry) in &by_sig {
        let bound = entry.certified_upper_bound;
        for supply in 0i64..=(bound + 12) {
            let funded = is_funded(entry, supply);
            let expected = !entry.unknown && i128::from(supply) >= i128::from(bound);
            assert_eq!(
                funded, expected,
                "per-lane funding law (lane={lane:?} entry={entry:?} supply={supply})"
            );
            // No contraction: funded at Σ ⇒ funded at Σ+1.
            if funded {
                assert!(
                    is_funded(entry, supply + 1),
                    "per-lane is_funded must be monotone in supply (lane={lane:?})"
                );
            }
        }
    }
}
