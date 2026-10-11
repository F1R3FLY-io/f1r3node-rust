//! G1-4a (DR-122): tests of the linear pre-execution cost estimate. They cover
//! the named programs of design §10 and §12, the completeness rules X1 to X8,
//! the narrowed evaluation of arbitration H2, the metering of arbitration H1,
//! the exactness of the arithmetic sizes, and the single application of units.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    CostAuthority, CostRegion, CostSignature, CostSignedTerm, EVar, Expr, ListParWithRandom, New,
    Par, Receive, ReceiveBind, Send, Var,
};
use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
use models::rust::phlo_schedule::PhloScheduleV1;
use proptest::prelude::*;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use super::*;
use crate::rust::interpreter::accounting::authority::{cost_region, cost_signature_to_sig};
use crate::rust::interpreter::accounting::byte_accounting::produce_introduction_charge;
use crate::rust::interpreter::accounting::cost_accounting::CostAccounting;
use crate::rust::interpreter::accounting::lexical::resolve_lexical_names_for_funding;
use crate::rust::interpreter::accounting::native_phlo_rules::native_resource_compatibility_rule;
use crate::rust::interpreter::compiler::compiler::Compiler;
use crate::rust::interpreter::matcher::r#match::Matcher;
use crate::rust::interpreter::reduce::DebruijnInterpreter;
use crate::rust::interpreter::rho_runtime::RhoISpace;

// ── Helpers shared by the estimate tests ───────────────────────────────────

/// The schedule of the tests: the four native classes in dimension order.
pub(super) fn schedule(weights: [u64; 4]) -> PhloScheduleV1<'static> {
    let identities: [&[u8]; 4] = [b"compute", b"introduction", b"transfer", b"trace"];
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip(identities)
            .zip(weights)
            .map(|((dimension, identity), weight)| dimension.resource_class(identity, weight))
            .collect(),
        actual_price: 2,
        compatibility_rule: native_resource_compatibility_rule(),
    }
}

pub(super) fn unlimited() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)))
}

pub(super) fn funding() -> Sig { Sig::Ground(vec![9, 9, 9, 9]) }

/// A reducer on an in-memory space with the given URN map. The estimate uses
/// only its evaluation and substitution, which never touch the space.
pub(super) fn evaluator(urn_map: HashMap<String, Par>) -> Arc<DebruijnInterpreter> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a test runtime")
        .block_on(async move {
            let mut stores = InMemoryStoreManager::new();
            let store = stores.r_space_stores().await.expect("in-memory stores");
            let space = RSpace::create(store, Arc::new(Box::new(Matcher))).expect("a test space");
            let space: RhoISpace = Arc::new(Box::new(space));
            DebruijnInterpreter::new(
                space,
                Arc::new(urn_map),
                Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                Arc::new(HashMap::new()),
                CostAccounting::empty_cost(),
            )
        })
}

/// The URN map of a runtime with the system processes, as play and replay
/// build it.
pub(super) fn runtime_urn_map() -> HashMap<String, Par> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a test runtime")
        .block_on(async {
            let mut stores = InMemoryStoreManager::new();
            let store = stores.r_space_stores().await.expect("in-memory stores");
            let (runtime, _, _) = crate::rust::interpreter::test_utils::resources::create_runtimes(
                store,
                false,
                &mut Vec::new(),
            )
            .await;
            (*runtime.reducer.urn_map).clone()
        })
}

pub(super) fn stdout_urn_map() -> HashMap<String, Par> {
    HashMap::from([(
        "rho:io:stdout".to_string(),
        Par::default().with_unforgeables(vec![models::rhoapi::GUnforgeable {
            unf_instance: Some(models::rhoapi::g_unforgeable::UnfInstance::GPrivateBody(
                models::rhoapi::GPrivate { id: vec![7; 32] },
            )),
        }]),
    )])
}

/// Compiles and resolves `source` as the pre-execution check does.
pub(super) fn resolved(source: &str, urn_map: &HashMap<String, Par>) -> Par {
    let program = Compiler::source_to_adt(source).expect("the test program compiles");
    let resolved = resolve_lexical_names_for_funding(
        &program,
        Blake2b512Random::create_from_bytes(b"estimate test"),
        urn_map,
    )
    .expect("the test program resolves");
    dismantle_term(program);
    resolved
}

pub(super) fn estimate_with(
    source: &str,
    weights: [u64; 4],
    urn_map: HashMap<String, Par>,
    host: &HostWorkBudget,
) -> Result<CostEstimate, InterpreterError> {
    let par = resolved(source, &urn_map);
    let reducer = evaluator(urn_map);
    let estimate = linear_cost_estimate(&par, &funding(), &schedule(weights), &reducer, host);
    dismantle_term(par);
    estimate
}

pub(super) fn estimate(source: &str) -> CostEstimate {
    estimate_with(source, [1; 4], HashMap::new(), &unlimited()).expect("the estimate runs")
}

/// The signature that the normalizer gives an unbound name in a cost
/// signature.
pub(super) fn signature_of(name: &str) -> CostSignature {
    let program =
        Compiler::source_to_adt(&format!("{{% Nil %}}[ {name} ]")).expect("a signed term");
    let signature = program.cost_signed_terms[0]
        .signature
        .clone()
        .expect("a signature");
    dismantle_term(program);
    signature
}

pub(super) fn lane(name: &str) -> SigKey {
    cost_signature_to_sig(&signature_of(name))
        .expect("a ground signature")
        .lane_hash()
}

/// The raw counts of one lane in dimension order.
pub(super) fn counts(estimate: &CostEstimate, lane: SigKey) -> [u64; 4] {
    let mut counts = [0; 4];
    for (index, slot) in counts.iter_mut().enumerate() {
        *slot = estimate
            .quantities
            .get(&(lane, index))
            .copied()
            .unwrap_or(0);
    }
    counts
}

pub(super) fn deploy_lane() -> SigKey { funding().lane_hash() }

/// The prost length of the datum that `eval_send` stores for `pars` under a
/// seal with `regions`.
fn stored_datum(pars: Vec<Par>, regions: Vec<CostRegion>) -> ListParWithRandom {
    ListParWithRandom {
        pars,
        random_state: Blake2b512Random::create_from_bytes(b"datum").to_bytes(),
        cost_authority: Some(CostAuthority { regions }),
        cost_stack: None,
    }
}

fn region(signature: &CostSignature, discriminator: u32) -> CostRegion {
    cost_region(signature, b"entropy", discriminator).expect("a canonical region")
}

fn ground(text: &str) -> CostSignature {
    CostSignature {
        value: Some(CostSignatureValue::Ground(text.as_bytes().to_vec())),
    }
}

fn deploy_signature() -> CostSignature {
    sig_to_cost_signature(&funding()).expect("a ground funding signature")
}

fn private(id: u8) -> Par {
    Par::default().with_unforgeables(vec![models::rhoapi::GUnforgeable {
        unf_instance: Some(models::rhoapi::g_unforgeable::UnfInstance::GPrivateBody(
            models::rhoapi::GPrivate { id: vec![id; 32] },
        )),
    }])
}

// ── Basic facts that the sizes rest on ─────────────────────────────────────

#[test]
fn a_random_state_serializes_to_536_bytes() {
    for seed in [b"a".as_slice(), b"another seed", &[0; 64]] {
        let mut rand = Blake2b512Random::create_from_bytes(seed);
        assert_eq!(rand.to_bytes().len(), RANDOM_STATE_BYTES);
        rand.next();
        assert_eq!(rand.split_byte(3).to_bytes().len(), RANDOM_STATE_BYTES);
    }
}

#[test]
fn the_deploy_lane_of_the_plan_is_the_lane_of_its_region() {
    for funding in [
        Sig::Ground(vec![1, 2, 3]),
        Sig::Quote(vec![4, 5]),
        Sig::And(
            Box::new(Sig::Ground(vec![1])),
            Box::new(Sig::Ground(vec![2])),
        ),
    ] {
        let signature = sig_to_cost_signature(&funding).expect("a supported funding signature");
        let region_lane = cost_signature_to_sig(&signature)
            .expect("a canonical funding signature")
            .lane_hash();
        assert_eq!(region_lane, funding.lane_hash());
    }
}

#[test]
fn units_count_the_ground_name_and_quote_leaves() {
    let compound = CostSignature {
        value: Some(CostSignatureValue::Compound(
            models::rhoapi::CostSignatureCompound {
                elements: vec![ground("a"), ground("b"), ground("c")],
            },
        )),
    };
    assert_eq!(signature_units(&ground("a")), 1);
    assert_eq!(signature_units(&compound), 3);
    assert_eq!(
        signature_units(&CostSignature {
            value: Some(CostSignatureValue::Unit(true)),
        }),
        0
    );
}

// ── Named programs (design §10 and §12) ────────────────────────────────────

/// Design §10.1: the term of the 64-source test is complete. Its send and its
/// receive both carry the region of `payer`, so compute is 2, transfer is
/// twice the datum and trace is twice 128 bytes.
#[test]
fn the_term_of_the_sixty_four_source_test_is_complete() {
    let estimate = estimate("{% new x in { x!(0) | for (@n <- x) { Nil } } %}[ payer ]");
    assert_eq!(estimate.completeness, Completeness::Complete);
    let payer = counts(&estimate, lane("payer"));
    assert_eq!(payer[0], 2, "one COMM count for each participant");
    assert_eq!(
        payer[3], 256,
        "one trace of 32 + 96 bytes for each participant"
    );
    let datum = stored_datum(
        vec![Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(0)),
        }])],
        vec![region(&signature_of("payer"), 0)],
    );
    assert_eq!(
        payer[2],
        2 * u64::try_from(datum.encoded_len()).expect("a small datum")
    );
    assert_eq!(counts(&estimate, deploy_lane()), [0; 4]);
    assert_eq!(
        estimate.bound_phlo,
        payer.iter().sum::<u64>(),
        "every weight and the units of payer are 1"
    );
}

/// Design §10.2: a branch on a received value keeps the estimate complete,
/// and the vector of the branch that does not run is the maximum with 0.
#[test]
fn a_branch_on_a_received_value_is_complete() {
    let estimate = estimate(
        "new x, y in { x!(5) | for (@n <- x) { if (n > 3) { y!(1) | for (_ <- y) { Nil } } else { Nil } } }",
    );
    assert_eq!(estimate.completeness, Completeness::Complete);
    assert_eq!(counts(&estimate, deploy_lane())[0], 4);
}

/// Design §10.3: a join with two binds signed by one signature charges two
/// regions of that lane, and its introduction charges the deploy lane.
#[test]
fn a_join_with_two_signed_binds_counts_two_regions() {
    let estimate = estimate(
        "new x, y in { x!(0) | y!(0) | for ({% @a <- x %}[ s ] & {% @b <- y %}[ s ]) { Nil } }",
    );
    assert_eq!(estimate.completeness, Completeness::Complete);
    let signed = counts(&estimate, lane("s"));
    assert_eq!(signed[0], 2);
    assert_eq!(
        signed[1], 0,
        "the introduction does not charge the bind regions"
    );
    assert_eq!(signed[3], 2 * (32 + 96 * 2));
    let datum = |seed: u8| {
        stored_datum(
            vec![Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GInt(0)),
            }])],
            vec![region(&deploy_signature(), u32::from(seed))],
        )
        .encoded_len()
    };
    assert_eq!(
        signed[2],
        2 * u64::try_from(datum(0) + datum(0)).expect("small data")
    );
    let deploy = counts(&estimate, deploy_lane());
    assert_eq!(deploy[0], 2, "each send opens a deploy region");
    assert!(
        deploy[1] > 0,
        "the receive introduces itself under the deploy lane"
    );
}

/// Design §10.4: the datum is sized after the reducer's evaluation, so the
/// interpolation counts the 40-character string.
#[test]
fn an_interpolated_datum_is_sized_after_evaluation() {
    let estimate = estimate(
        r#"new x in { x!("${a}${a}${a}${a}" %% {"a": "abcdefghij"}) | for (@n <- x) { Nil } }"#,
    );
    assert_eq!(estimate.completeness, Completeness::Complete);
    let evaluated = stored_datum(
        vec![Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString("abcdefghij".repeat(4))),
        }])],
        vec![region(&deploy_signature(), 0)],
    );
    let transfer = counts(&estimate, deploy_lane())[2];
    assert_eq!(
        transfer,
        2 * u64::try_from(evaluated.encoded_len()).expect("a small datum"),
        "the send and the receive each count the evaluated datum"
    );
}

/// Design §10.5: each incomplete program reports the first failed rule.
#[test]
fn incomplete_programs_report_their_rule() {
    let cases: [(&str, IncompleteReason); 6] = [
        (
            r#"{% @"pub"!(0) %}[ payer ]"#,
            IncompleteReason::NonFreshChannel,
        ),
        (
            r#"{% new x in { x!(0) | for (@n <- x) { Nil } | [ @"pub"!(0) ].nth(0) } %}[ payer ]"#,
            IncompleteReason::MethodInProcessPosition,
        ),
        (
            "new x in { contract x(@n) = { x!(n) } | x!(0) }",
            IncompleteReason::StaticPlan(UnprovableDemand::UnboundedControlFlow),
        ),
        (
            "new x, y in { x!(0) | for (@n <- x) { y!(n) } | for (_ <- y) { Nil } }",
            IncompleteReason::OpenData,
        ),
        (
            "new x in { x!(0) | for (@n <<- x) { Nil } }",
            IncompleteReason::PeekOrStack,
        ),
        (
            "new x in { x!(0) | for (@n <- x) { match n { 1 => { x!(n) } _ => Nil } } }",
            IncompleteReason::OpenData,
        ),
    ];
    for (source, reason) in cases {
        assert_eq!(
            estimate(source).completeness,
            Completeness::Incomplete(reason),
            "{source}"
        );
    }
}

/// Design §10.5: a URN inside a receive body keeps its URN kind, so its
/// channel is not fresh.
#[test]
fn a_urn_inside_a_receive_body_is_not_a_fresh_channel() {
    let source = "new x in { x!(0) | for (@n <- x) { new out(`rho:io:stdout`) in { out!(0) } } }";
    let estimate =
        estimate_with(source, [1; 4], stdout_urn_map(), &unlimited()).expect("the estimate runs");
    assert_eq!(
        estimate.completeness,
        Completeness::Incomplete(IncompleteReason::NonFreshChannel)
    );
}

/// A nested signed scope inside a term with several parallel parts: each
/// participant counts every enclosing region. Uniform signing wraps the
/// receive's continuation in `{% Nil %}[ inner ]`, and the plan counts one more
/// unit for that empty signed body (design §5.5).
#[test]
fn a_nested_signed_scope_counts_every_enclosing_region() {
    let estimate =
        estimate("new x in { {% x!(0) | {% for (@n <- x) { Nil } %}[ inner ] %}[ outer ] | Nil }");
    assert_eq!(estimate.completeness, Completeness::Complete);
    assert_eq!(counts(&estimate, lane("outer"))[0], 2);
    assert_eq!(counts(&estimate, lane("inner"))[0], 2);
    assert_eq!(counts(&estimate, deploy_lane()), [0; 4]);
}

/// A receive with a signed bind: its COMM charges the bind region and no
/// deploy region, and its introduction charges the deploy lane.
#[test]
fn a_signed_bind_charges_its_comm_to_its_own_region() {
    let estimate = estimate("new x in { x!(0) | for ({% @n <- x %}[ s ]) { Nil } }");
    assert_eq!(estimate.completeness, Completeness::Complete);
    let signed = counts(&estimate, lane("s"));
    assert_eq!(signed[0], 1);
    assert_eq!(signed[1], 0);
    assert_eq!(signed[3], 128);
    let deploy = counts(&estimate, deploy_lane());
    assert_eq!(deploy[0], 1, "the send opens one deploy region");
    assert!(deploy[1] > 0);
}

/// A join with two binds on one channel consumes two data, so its transfer
/// counts the channel's datum twice (design §5.5). The normalizer rejects such
/// a join in source (`ReceiveOnSameChannelsError`), so the test builds the
/// term in code: `new x in { x!(1) | x!(2) | for (_ <- x & _ <- x) { Nil } }`.
#[test]
fn a_join_with_a_repeated_channel_counts_the_datum_for_each_bind() {
    let channel = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(VarInstance::BoundVar(0)),
            }),
        })),
    }]);
    let send = |value: i64| Send {
        chan: Some(channel.clone()),
        data: vec![Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }])],
        persistent: false,
        locally_free: vec![1],
        connective_used: false,
    };
    let bind = || ReceiveBind {
        patterns: vec![Par::default()],
        source: Some(channel.clone()),
        remainder: None,
        free_count: 0,
        cost_signature: None,
    };
    let body = Par {
        sends: vec![send(1), send(2)],
        receives: vec![Receive {
            binds: vec![bind(), bind()],
            body: Some(Par::default()),
            persistent: false,
            peek: false,
            bind_count: 0,
            locally_free: vec![1],
            connective_used: false,
            condition: None,
        }],
        ..Default::default()
    };
    let mut par = Par::default();
    par.news.push(New {
        bind_count: 1,
        p: Some(body),
        uri: Vec::new(),
        injections: BTreeMap::new(),
        locally_free: Vec::new(),
    });
    let reducer = evaluator(HashMap::new());
    let estimate =
        linear_cost_estimate(&par, &funding(), &schedule([1; 4]), &reducer, &unlimited())
            .expect("the estimate runs");
    assert_eq!(estimate.completeness, Completeness::Complete);
    let datum = stored_datum(
        vec![Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(1)),
        }])],
        vec![region(&deploy_signature(), 0)],
    );
    let message = u64::try_from(datum.encoded_len()).expect("a small datum");
    let deploy = counts(&estimate, deploy_lane());
    assert_eq!(
        deploy[2],
        6 * message,
        "the receive and each send count two data of the channel"
    );
    assert_eq!(deploy[3], 3 * (32 + 96 * 2));
}

/// The resolver merges the value of a dequoted resolved name into the receive
/// body, so the plan sees no dequotation and the estimate stays complete.
#[test]
fn a_dequoted_fresh_name_inside_a_receive_body_is_merged() {
    let estimate = estimate("new x in { x!(0) | for (@n <- x) { *x } }");
    assert_eq!(estimate.completeness, Completeness::Complete);
}

/// A name created inside a receive body and used in a signature is dynamic
/// authority (DR-121), so rule X1 fails.
#[test]
fn a_signature_on_a_receive_body_name_is_dynamic() {
    let estimate = estimate("new x in { x!(0) | for (@n <- x) { new y in { {% y!(0) %}[ y ] } } }");
    assert_eq!(
        estimate.completeness,
        Completeness::Incomplete(IncompleteReason::StaticPlan(
            UnprovableDemand::DynamicAuthority
        ))
    );
}

// ── The completeness rules: no violating program is complete ──────────────

/// A context that wraps a core program: parallel parts, a `new`, a signed
/// scope or a match. None of them changes the rule that the core breaks.
fn wrap(core: &str, context: u8, filler: u8) -> String {
    let filler = match filler % 3 {
        0 => "Nil".to_string(),
        1 => "new z in { z!(1) | for (@w <- z) { Nil } }".to_string(),
        _ => "{% new z in { z!(2) | for (_ <- z) { Nil } } %}[ f ]".to_string(),
    };
    match context % 4 {
        0 => format!("{core} | {filler}"),
        1 => format!("new outer in {{ {core} | {filler} }}"),
        2 => format!("{{% {core} | {filler} %}}[ ctx ]"),
        _ => format!("match 7 {{ 7 => {{ {core} }} _ => {{ {filler} }} }}"),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// One generated program for each rule X1 to X8 (design §12). Each core
    /// breaks exactly one rule, and every context keeps it the first failure.
    #[test]
    fn no_program_that_breaks_a_rule_is_complete(rule in 0u8..10, context in any::<u8>(), filler in any::<u8>()) {
        let (core, reason) = match rule {
            0 => ("new x in { x!(0) | for (@n <= x) { Nil } }", IncompleteReason::StaticPlan(UnprovableDemand::UnboundedControlFlow)),
            1 => ("new x in { x!(0) | for (n <- x) { *n } }", IncompleteReason::StaticPlan(UnprovableDemand::RecursiveDequotation)),
            2 => (r#"new x in { @"public"!(0) | for (@n <- x) { Nil } }"#, IncompleteReason::NonFreshChannel),
            3 => ("new x, y in { x!(0) | for (@n <- x) { match *y { _ => Nil } } }", IncompleteReason::EscapingName),
            4 => ("new x, y in { x!(0) | for (@n <- x) { y!(n) } | for (_ <- y) { Nil } }", IncompleteReason::OpenData),
            5 => ("new x, y in { x!(0) | for (@{=*y} <- x) { Nil } }", IncompleteReason::OpenPattern),
            6 => ("new x, y in { x!(0) | y!(1) | for (@n <- x) { for (_ <- y) { if (n > 1) { Nil } else { Nil } } } }", IncompleteReason::ReceivedValueInIntroduction),
            7 => ("new x in { x!(0) | for (@n <<- x) { Nil } }", IncompleteReason::PeekOrStack),
            8 => ("new x in { x!(0) | for (@n <- x) { Nil } | [Nil].nth(0) }", IncompleteReason::MethodInProcessPosition),
            _ => ("new x in { x!(1 matches _) | for (@n <- x) { Nil } }", IncompleteReason::DataEvaluation),
        };
        let estimate = estimate(&wrap(core, context, filler));
        prop_assert_eq!(estimate.completeness, Completeness::Incomplete(reason));
    }
}

// ── The narrowed evaluation (arbitration H2) ───────────────────────────────

/// A datum with a costly `matches` is never evaluated: the participant has
/// no size, and the estimate reserves no primitive call for it.
#[test]
fn a_costly_matches_is_never_evaluated() {
    let host = unlimited();
    let estimate = estimate_with(
        "new x in { x!([1, 2, 3] matches [1, _, 3]) | for (@n <- x) { Nil } }",
        [1; 4],
        HashMap::new(),
        &host,
    )
    .expect("the estimate runs");
    assert_eq!(
        estimate.completeness,
        Completeness::Incomplete(IncompleteReason::DataEvaluation)
    );
    assert_eq!(
        host.usage(HostWorkDimension::PrimitiveCalls).get(),
        2,
        "only the channel lookups of the send and of the receive are evaluated"
    );
}

/// A chain of big-integer multiplications has no known work limit.
#[test]
fn a_chain_of_big_integer_multiplications_is_never_evaluated() {
    let estimate =
        estimate("new x in { x!(10n * 10n * 10n * 10n * 10n * 10n) | for (@n <- x) { Nil } }");
    assert_eq!(
        estimate.completeness,
        Completeness::Incomplete(IncompleteReason::DataEvaluation)
    );
}

/// A long `%%` template is charged `n * (n + K * (k + 3))` before the
/// interpolation runs, so the offered host-work limits reject it.
#[test]
fn a_long_interpolation_template_is_rejected_before_it_runs() {
    let template = "${a}".repeat(5_000);
    let source =
        format!(r#"new x in {{ x!("{template}" %% {{"a": "b"}}) | for (@n <- x) {{ Nil }} }}"#);
    let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(268_435_456));
    limits.set(
        HostWorkDimension::SearchStateBytes,
        HostWorkLimit::new(134_217_728),
    );
    let host = HostWorkBudget::new(limits);
    assert_eq!(
        estimate_with(&source, [1; 4], HashMap::new(), &host),
        Err(InterpreterError::HostWorkRejected)
    );
    assert_eq!(
        host.rejection().map(|rejection| rejection.dimension()),
        Some(HostWorkDimension::PrimitiveInputBytes)
    );
}

/// A nested `%%` chain doubles its output at each level. The output bound of
/// each level is the template length of the next, so the charge grows with
/// the depth of the chain, and the offered host-work limits reject a deep
/// chain before any level runs. A shallow chain stays complete under the same
/// limits.
#[test]
fn a_nested_interpolation_chain_is_rejected_before_it_runs() {
    fn chain(levels: usize) -> String {
        let mut datum = String::from(r#""${a}${a}""#);
        for _ in 0..levels {
            datum = format!(r#"({datum} %% {{"a": "${{a}}${{a}}"}})"#);
        }
        format!("new x in {{ x!({datum}) | for (@n <- x) {{ Nil }} }}")
    }
    fn offered() -> HostWorkBudget {
        let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(268_435_456));
        limits.set(
            HostWorkDimension::SearchStateBytes,
            HostWorkLimit::new(134_217_728),
        );
        HostWorkBudget::new(limits)
    }
    let shallow = estimate_with(&chain(4), [1; 4], HashMap::new(), &offered())
        .expect("the offered limits admit a shallow chain");
    assert_eq!(shallow.completeness, Completeness::Complete);
    let host = offered();
    assert_eq!(
        estimate_with(&chain(24), [1; 4], HashMap::new(), &host),
        Err(InterpreterError::HostWorkRejected)
    );
    assert_eq!(
        host.rejection().map(|rejection| rejection.dimension()),
        Some(HostWorkDimension::PrimitiveInputBytes)
    );
}

/// The interpolation charge is the formula of arbitration H2 plus the bound
/// of the input and output bytes.
#[test]
fn the_interpolation_charge_follows_the_formula() {
    let reducer = evaluator(HashMap::new());
    let core = isolated_evaluator(&reducer);
    let host = unlimited();
    let datum = Compiler::source_to_adt(r#""${ab}${ab}xyz" %% {"ab": "0123456789", "c": "d"}"#)
        .expect("the datum compiles");
    let mut estimator = Estimator::new(&funding(), &core, &host).expect("an estimator");
    estimator.measure(&datum).expect("the datum is measured");
    let bound = estimator
        .evaluation_bound(&datum)
        .expect("the bound is counted")
        .expect("an interpolation has a known limit");
    let template = 13u64;
    let keys = 2u64;
    let key_bytes = 2u64;
    let loops = template * (template + keys * (key_bytes + 3));
    assert!(bound.work >= loops);
    let without = Compiler::source_to_adt(r#""${ab}${ab}xyz" ++ "0123456789""#)
        .expect("the concatenation compiles");
    estimator
        .measure(&without)
        .expect("the concatenation is measured");
    let concatenation = estimator
        .evaluation_bound(&without)
        .expect("the bound is counted")
        .expect("a concatenation has a known limit");
    assert!(concatenation.work < bound.work);
    dismantle_term(datum);
    dismantle_term(without);
}

// ── Exactness of the arithmetic sizes ──────────────────────────────────────

/// The seal of a send inside nested signed scopes has the length of the
/// authority that the reducer builds, because the running total of the scope
/// adds the field length of each region. Each signed body has two parallel
/// parts, so the normalizer keeps the three scopes apart.
#[test]
fn seal_lengths_come_from_running_totals() {
    let estimate = estimate("new x in { {% x!(0) | {% x!(1) | {% x!(2) %}[ c ] %}[ b ] %}[ a ] }");
    assert_eq!(estimate.completeness, Completeness::Complete);
    let introduction = |value: i64, regions: Vec<CostRegion>| {
        let datum = ListParWithRandom {
            pars: vec![Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::GInt(value)),
            }])],
            random_state: vec![0; RANDOM_STATE_BYTES],
            cost_authority: Some(CostAuthority { regions }),
            cost_stack: None,
        };
        produce_introduction_charge(&private(1), &datum)
            .expect("a measured produce")
            .introduction_bytes
    };
    let a = || region(&signature_of("a"), 0);
    let b = || region(&signature_of("b"), 0);
    let c = || region(&signature_of("c"), 0);
    let outer = introduction(0, vec![a()]);
    let middle = introduction(1, vec![a(), b()]);
    let inner = introduction(2, vec![a(), b(), c()]);
    assert_eq!(counts(&estimate, lane("a"))[1], outer + middle + inner);
    assert_eq!(counts(&estimate, lane("b"))[1], middle + inner);
    assert_eq!(counts(&estimate, lane("c"))[1], inner);
}

/// Directly nested signed terms collapse into one scope with a compound
/// signature: one region of a lane with two units.
#[test]
fn directly_nested_signed_terms_form_one_compound_scope() {
    let estimate = estimate("new x in { {% {% x!(0) %}[ a ] %}[ b ] }");
    assert_eq!(estimate.completeness, Completeness::Complete);
    let mut lanes = estimate
        .signatures
        .iter()
        .filter(|(lane, _)| **lane != deploy_lane());
    let (compound, signature) = lanes.next().expect("one compound lane");
    assert!(lanes.next().is_none());
    assert_eq!(signature_units(signature), 2);
    assert_eq!(counts(&estimate, *compound)[0], 1);
}

/// The program of the vector fold follows `compile_term` in the reducer's
/// term order, where the match comes before the `if`: each folds its branches
/// with an alternative, and every child combines.
#[test]
fn pass_one_emits_the_program_of_compile_term() {
    let par = resolved(
        "new x in { x!(0) | if (true) { x!(1) } else { Nil } | match 1 { 1 => { x!(2) } _ => { x!(3) } } }",
        &HashMap::new(),
    );
    let reducer = evaluator(HashMap::new());
    let core = isolated_evaluator(&reducer);
    let host = unlimited();
    let mut estimator = Estimator::new(&funding(), &core, &host).expect("an estimator");
    estimator.measure(&par).expect("the program is measured");
    estimator.walk(&par).expect("pass 1 runs");
    use Instruction::{Alternative, Combine, Local, Push};
    assert_eq!(estimator.program, vec![
        Push,
        Push,
        Local(0),
        Push,
        Local(1),
        Push,
        Local(2),
        Alternative,
        Combine,
        Push,
        Local(3),
        Push,
        Alternative,
        Combine,
        Combine,
    ]);
    dismantle_term(par);
}

// ── The metering of arbitration H1 ─────────────────────────────────────────

/// `depth` nested signed scopes, each with its own signature, around `sends`
/// sends on one fresh name. The loop builds the term from the inside out.
fn signed_tower(depth: usize, sends: usize) -> Par {
    let channel = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(VarInstance::BoundVar(0)),
            }),
        })),
    }]);
    let mut body = Par::default();
    for _ in 0..sends {
        body.sends.push(Send {
            chan: Some(channel.clone()),
            data: Vec::new(),
            persistent: false,
            locally_free: vec![1],
            connective_used: false,
        });
    }
    for level in 0..depth {
        let mut outer = Par::default();
        outer.cost_signed_terms.push(CostSignedTerm {
            body: Some(body),
            signature: Some(ground(&format!("tower-{level}"))),
        });
        body = outer;
    }
    let mut root = Par::default();
    root.news.push(New {
        bind_count: 1,
        p: Some(body),
        uri: Vec::new(),
        injections: BTreeMap::new(),
        locally_free: Vec::new(),
    });
    root
}

fn usages_of(par: &Par) -> [u64; 16] {
    let host = unlimited();
    let reducer = evaluator(HashMap::new());
    linear_cost_estimate(par, &funding(), &schedule([1; 4]), &reducer, &host)
        .expect("the estimate runs");
    let usages = host.usages().into_array();
    std::array::from_fn(|index| usages[index].get())
}

fn dimension_index(dimension: HostWorkDimension) -> usize {
    HostWorkDimension::ALL
        .iter()
        .position(|candidate| *candidate == dimension)
        .expect("a host-work dimension")
}

/// The chain walks of the plan and of the fold visit one region link for
/// each enclosing scope of each send. The plan walks each link once. The
/// fold charges three walks of each link, because `contribute` can walk the
/// chain of a receive three times (review LOW 3). The authority-node charge
/// is therefore four times the sends times the depth plus one.
#[test]
fn chain_links_are_charged_for_each_enclosing_scope() {
    for (depth, sends) in [(4usize, 3usize), (8, 3), (8, 6)] {
        let par = signed_tower(depth, sends);
        let usages = usages_of(&par);
        let links = u64::try_from(sends * (depth + 1)).expect("a small count");
        // Changed by the G1-4a fix (review LOW 3): the fold charges three
        // walks of each link.
        // assert_eq!(
        //     usages[dimension_index(HostWorkDimension::AuthorityNodes)],
        //     2 * links,
        //     "depth {depth}, sends {sends}"
        // );
        assert_eq!(
            usages[dimension_index(HostWorkDimension::AuthorityNodes)],
            4 * links,
            "depth {depth}, sends {sends}"
        );
        dismantle_term(par);
    }
}

/// A combine at each level of a tower touches the lanes of the enclosing
/// scopes, so the merge charge grows with the square of the depth.
#[test]
fn merge_entries_grow_with_the_enclosing_lanes() {
    let small = usages_of(&signed_tower(16, 1));
    let large = usages_of(&signed_tower(32, 1));
    let index = dimension_index(HostWorkDimension::VerificationOperations);
    assert!(
        large[index] > 3 * small[index],
        "doubling the depth more than triples the merge charge: {} and {}",
        small[index],
        large[index]
    );
}

/// Nested receive bodies: the body of each receive is substituted again at
/// each enclosing receive, and each substitution is charged before it runs.
#[test]
fn nested_receive_bodies_are_charged_for_each_substitution() {
    let nested = |depth: usize| {
        let mut source = String::from("Nil");
        for _ in 0..depth {
            source = format!("x!(0) | for (@n <- x) {{ {source} }}");
        }
        format!("new x in {{ {source} }}")
    };
    let index = dimension_index(HostWorkDimension::SubstitutionBytes);
    let charge = |depth: usize| {
        let host = unlimited();
        estimate_with(&nested(depth), [1; 4], HashMap::new(), &host).expect("the estimate runs");
        host.usages().into_array()[index].get()
    };
    let (shallow, deep) = (charge(8), charge(16));
    assert!(
        deep > 3 * shallow,
        "the substituted bytes grow with the square of the receive depth: {shallow} and {deep}"
    );
}

/// `Env::shift` copies the whole environment at each binder of a substituted
/// term. One more `new` inside a sized receive body adds one copy, so the
/// binding charge grows by the environment size, which holds every outer
/// name. The `new` itself binds one more level. Changed by the G1-4a fix
/// (review M-4): a copy costs the capacity of the environment map, which is
/// at least its length, so the difference is at least the 32 added names.
/// Changed by the second G1-4a fix (re-review HIGH-1): a copy costs the fixed
/// bucket count of the map, which the pre-pass sizes from the peak binder
/// count. A sibling `new` of eight names fixes the peak at the outer names
/// plus eight, so each nested `new` adds that bucket count plus its own
/// binding.
#[test]
fn each_environment_copy_of_a_sizing_substitution_is_charged() {
    // let program = |outer: usize, nested: usize| {
    //     let names = (0..outer)
    //         .map(|index| format!("a{index}"))
    //         .collect::<Vec<_>>()
    //         .join(", ");
    //     let mut body = String::from("Nil");
    //     for index in 0..nested {
    //         body = format!("new b{index} in {{ {body} }}");
    //     }
    //     format!("new {names} in {{ new x in {{ x!(0) | for (@n <- x) {{ {body} }} }} }}")
    // };
    let program = |outer: usize, nested: usize| {
        let names = (0..outer)
            .map(|index| format!("a{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut body = String::from("Nil");
        for index in 0..nested {
            body = format!("new b{index} in {{ {body} }}");
        }
        format!(
            "new {names} in {{ new c0, c1, c2, c3, c4, c5, c6, c7 in {{ Nil }} | new x in {{ x!(0) | for (@n <- x) {{ {body} }} }} }}"
        )
    };
    let index = dimension_index(HostWorkDimension::SubstitutionBindings);
    let bindings = |outer: usize, nested: usize| {
        let host = unlimited();
        estimate_with(&program(outer, nested), [1; 4], HashMap::new(), &host)
            .expect("the estimate runs");
        host.usages().into_array()[index].get()
    };
    let copy = |outer: usize| {
        let (none, one, two) = (bindings(outer, 0), bindings(outer, 1), bindings(outer, 2));
        assert_eq!(
            two - one,
            one - none,
            "each nested `new` adds the same charge"
        );
        one - none
    };
    let (small, large) = (copy(4), copy(36));
    // Changed by the G1-4a fix (review M-4): the copy charge is the capacity
    // of the map, and `environment_copies_are_charged_the_capacity_of_the_map`
    // checks its exact value.
    // assert_eq!(large - small, 32, "each copy holds every outer name");
    // Changed by the second G1-4a fix (re-review HIGH-1): the exact charge
    // follows from the peak binder count.
    // assert!(
    //     large - small >= 32,
    //     "each copy holds every outer name: {small} and {large}"
    // );
    for (outer, copy) in [(4usize, small), (36, large)] {
        let peak = outer + 8;
        assert_eq!(
            copy,
            environment_buckets(2 * peak + 2) + 1,
            "{outer} outer names"
        );
    }
}

/// For every charged dimension, a budget equal to the charge gives the same
/// estimate, and a budget one unit below it rejects the program.
#[test]
fn an_exhausted_budget_rejects_the_estimate_in_every_dimension() {
    let source = r#"new x, y, out(`rho:io:stdout`) in {
        {% x!("${a}" %% {"a": "bc"}, [1, 2] ++ [3]) | for (@n <- x & @m <- y) { if (n == 1) { Nil } else { Nil } } %}[ s ] |
        y!(true) | for ({% @k <- y %}[ t ]) { match k { true => { Nil } _ => { Nil } } } | out!(0)
    }"#;
    let urn_map = stdout_urn_map();
    let par = resolved(source, &urn_map);
    let reducer = evaluator(urn_map);
    let host = unlimited();
    let expected = linear_cost_estimate(&par, &funding(), &schedule([1; 4]), &reducer, &host)
        .expect("an unlimited budget estimates the program");
    let mut charged = 0;
    for dimension in HostWorkDimension::ALL {
        let usage = host.usage(dimension).get();
        if usage == 0 {
            continue;
        }
        charged += 1;
        let mut short = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
        short.set(dimension, HostWorkLimit::new(usage - 1));
        assert_eq!(
            linear_cost_estimate(
                &par,
                &funding(),
                &schedule([1; 4]),
                &reducer,
                &HostWorkBudget::new(short)
            ),
            Err(InterpreterError::HostWorkRejected),
            "{dimension} one unit short"
        );
        let mut exact = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
        exact.set(dimension, HostWorkLimit::new(usage));
        assert_eq!(
            linear_cost_estimate(
                &par,
                &funding(),
                &schedule([1; 4]),
                &reducer,
                &HostWorkBudget::new(exact)
            ),
            Ok(expected.clone()),
            "{dimension} exactly enough"
        );
    }
    assert!(charged >= 8, "the estimate charges {charged} dimensions");
    dismantle_term(par);
}

// ── The estimate applies units once (the M1 control of the map) ────────────

/// The estimate's raw counts form a counted witness whose checked usage is
/// the estimate itself. Units applied twice would exceed it for a lane with
/// two units.
#[test]
fn the_counted_witness_of_the_raw_counts_has_the_estimate_as_its_usage() {
    use models::rust::phlo_schedule::PhloGenesisPolicy;

    use crate::rust::interpreter::accounting::phlo_controls::{
        check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
    };
    use crate::rust::interpreter::accounting::phlo_execution::{
        check_counted_phlo_execution, CountedPhloExecutionWitness, PhloExecutionLimits,
        PhloResource, PhloResourceAmount,
    };

    let weights = [3, 1, 2, 5];
    let estimate = estimate_with(
        "{% new x in { x!(0) | for (@n <- x) { Nil } } %}[ a (*) b ]",
        weights,
        HashMap::new(),
        &unlimited(),
    )
    .expect("the estimate runs");
    assert_eq!(estimate.completeness, Completeness::Complete);
    let descriptor = schedule(weights);
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS)
        .expect("a schedule binding");
    let selected = binding.schedule();
    let schedules = [selected];
    let owner_ceilings = [descriptor.actual_price];
    let controls = check_phlo_controls(
        selected.environment,
        0,
        u64::MAX,
        SignedPhloControls {
            limit: 1_000_000_000_000,
            price_ceiling: descriptor.actual_price,
            required_owner_ceilings: &owner_ceilings,
            permitted_schedules: &schedules,
        },
        selected,
        estimate.bound_phlo,
    )
    .expect("the controls admit the estimate");
    let sigs = estimate
        .quantities
        .keys()
        .map(|(lane, _)| {
            cost_signature_to_sig(estimate.signatures.get(lane).expect("a lane signature"))
                .expect("a static lane")
        })
        .collect::<Vec<_>>();
    let locations = estimate
        .quantities
        .keys()
        .map(|(lane, _)| lane.to_vec())
        .collect::<Vec<_>>();
    let required = estimate
        .quantities
        .iter()
        .zip(sigs.iter().zip(locations.iter()))
        .map(
            |(((_, class), quantity), (sig, location))| PhloResourceAmount {
                resource: PhloResource {
                    location,
                    class: *class,
                    acquisition_terms: b"estimate",
                    authority: sig,
                },
                quantity: *quantity,
            },
        )
        .collect::<Vec<_>>();
    let checked = check_counted_phlo_execution(
        controls,
        CountedPhloExecutionWitness {
            available: &[],
            required: &required,
            used: &[],
            unused: &[],
            fresh: &required,
        },
        PhloExecutionLimits {
            resource_entries: 64,
            authority_nodes: 4_096,
            key_bytes: 1 << 20,
        },
    )
    .expect("the witness of the estimate is admissible");
    assert_eq!(checked.usage(), estimate.bound_phlo);
    let compound = lane_of_compound();
    let twice = estimate
        .quantities
        .iter()
        .filter(|((lane, _), _)| *lane == compound)
        .map(|((_, class), quantity)| weights[*class] * 2 * 2 * quantity)
        .sum::<u64>();
    assert!(
        twice > estimate.bound_phlo,
        "units applied twice exceed the estimate"
    );
}

fn lane_of_compound() -> SigKey {
    let program = Compiler::source_to_adt("{% Nil %}[ a (*) b ]").expect("a signed term");
    let signature = program.cost_signed_terms[0]
        .signature
        .clone()
        .expect("a signature");
    dismantle_term(program);
    cost_signature_to_sig(&signature)
        .expect("a static compound")
        .lane_hash()
}

/// `dismantle_term` drops a term nested through every field that holds a
/// `Par` or a `CostSignature` on a small stack.
#[test]
fn dismantle_drops_terms_nested_through_every_field_on_a_small_stack() {
    let mut par = Par::default();
    for level in 0..20_000usize {
        let mut outer = Par::default();
        match level % 6 {
            0 => outer.exprs.push(Expr {
                expr_instance: Some(ExprInstance::EListBody(models::rhoapi::EList {
                    ps: vec![par],
                    ..Default::default()
                })),
            }),
            1 => outer.receives.push(Receive {
                binds: vec![ReceiveBind {
                    patterns: vec![par],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            2 => outer.sends.push(Send {
                chan: Some(par),
                ..Default::default()
            }),
            3 => outer.cost_signed_terms.push(CostSignedTerm {
                body: None,
                signature: Some(CostSignature {
                    value: Some(CostSignatureValue::Quote(par)),
                }),
            }),
            4 => outer.news.push(New {
                injections: BTreeMap::from([("rho:test".to_string(), par)]),
                ..Default::default()
            }),
            _ => outer.exprs.push(Expr {
                expr_instance: Some(ExprInstance::EPlusBody(models::rhoapi::EPlus {
                    p1: Some(par),
                    p2: None,
                })),
            }),
        }
        par = outer;
    }
    std::thread::Builder::new()
        .name("estimate-teardown".to_string())
        .stack_size(256 * 1024)
        .spawn(move || dismantle_term(par))
        .expect("spawn the teardown thread")
        .join()
        .expect("the iterative teardown stays within a small stack");
}

// ── The fixes of the G1-4a review (H-1, H-2, H-3, M-1, M-2, M-4) ───────────

fn integer(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

/// The body of the outermost `new` of a resolved skeleton, which a test
/// widens in code.
fn new_body(par: &mut Par) -> &mut Par {
    par.news
        .first_mut()
        .and_then(|new| new.p.as_mut())
        .expect("a `new` with a body")
}

fn offered_limits() -> HostWorkBudget {
    HostWorkBudget::new(models::rust::cost_protocol_limits::offered_funded_v6_host_work_limits())
}

fn estimate_par(par: &Par, host: &HostWorkBudget) -> Result<CostEstimate, InterpreterError> {
    let reducer = evaluator(HashMap::new());
    linear_cost_estimate(par, &funding(), &schedule([1; 4]), &reducer, host)
}

/// Review H-1: a `#P` signature whose process still holds a variable names
/// another lane at run time, because the reducer substitutes inside the
/// quote. Its lane is dynamic. The five forms are a matched variable, a
/// received variable, the dereference of a fresh hole, a received variable
/// in the signature of a bind, and the dereference of a resolved name in a
/// receive body, which the resolver leaves unchanged.
#[test]
fn a_quoted_signature_with_a_variable_is_dynamic() {
    let text = "A".repeat(10_000);
    let programs = [
        [
            r#"new y in { match ""#,
            &text,
            r#"" { s => { {% y!(0) | for (_ <- y) { Nil } %}[ #s ] } } }"#,
        ]
        .concat(),
        "new x, y in { x!(0) | for (@n <- x) { {% y!(0) | for (_ <- y) { Nil } %}[ #n ] } }"
            .to_string(),
        "new x in { x!(0) | for (@n <- x) { new z in { {% z!(0) | for (_ <- z) { Nil } %}[ #*z ] } } }"
            .to_string(),
        "new x, y in { x!(0) | y!(1) | for (@n <- x) { for ({% @a <- y %}[ #n ]) { Nil } } }"
            .to_string(),
        "new x, y in { x!(0) | for (@n <- x) { {% y!(0) | for (_ <- y) { Nil } %}[ #*y ] } }"
            .to_string(),
    ];
    for source in &programs {
        assert_eq!(
            estimate(source).completeness,
            Completeness::Incomplete(IncompleteReason::StaticPlan(
                UnprovableDemand::DynamicAuthority
            )),
            "{}",
            &source[..source.len().min(120)]
        );
    }
}

/// Review H-1, the control: the quote of a closed process and the
/// dereference of a name that the resolver binds keep static lanes.
#[test]
fn a_quoted_signature_without_a_variable_is_static() {
    let closed = estimate(r#"new y in { {% y!(0) | for (_ <- y) { Nil } %}[ #"AAAA" ] }"#);
    assert_eq!(closed.completeness, Completeness::Complete);
    assert_eq!(counts(&closed, lane(r#"#"AAAA""#))[0], 2);
    let resolved_name = estimate("new y in { {% y!(0) | for (_ <- y) { Nil } %}[ #*y ] }");
    assert_eq!(resolved_name.completeness, Completeness::Complete);
    assert!(
        resolved_name
            .signatures
            .keys()
            .any(|lane| *lane != deploy_lane()),
        "the resolved name has a static lane"
    );
}

/// Review H-1 in the plan: `signature_lane` makes a signature that holds a
/// variable dynamic authority, and a ground signature at the same position
/// stays static.
#[test]
fn the_plan_treats_a_quoted_signature_with_a_variable_as_dynamic() {
    let source = |signature: &str| {
        format!("new x, y in {{ x!(0) | for (@n <- x) {{ {{% y!(0) %}}[ {signature} ] }} }}")
    };
    let dynamic = resolved(&source("#n"), &HashMap::new());
    let ground = resolved(&source("s"), &HashMap::new());
    let plans = (
        static_authority_plan(&dynamic, &funding()).err(),
        static_authority_plan(&ground, &funding()).err(),
    );
    dismantle_term(dynamic);
    dismantle_term(ground);
    assert_eq!(plans, (Some(UnprovableDemand::DynamicAuthority), None));
}

/// Review H-2: 30,000 parallel sends on one fresh name fit the offered
/// host-work limits. A combine costs the lanes that it merges, not the
/// nodes of the merged subtree.
#[test]
fn thirty_thousand_parallel_sends_fit_the_offered_limits() {
    let mut par = resolved("new x in { x!(0) }", &HashMap::new());
    let body = new_body(&mut par);
    let send = body.sends.pop().expect("one send");
    body.sends = (0..30_000i64)
        .map(|value| Send {
            data: vec![integer(value)],
            ..send.clone()
        })
        .collect();
    let host = offered_limits();
    let estimate = estimate_par(&par, &host);
    dismantle_term(par);
    let estimate = estimate.expect("the offered limits admit 30,000 parallel sends");
    assert_eq!(estimate.completeness, Completeness::Complete);
    assert_eq!(counts(&estimate, deploy_lane())[0], 30_000);
}

/// Review H-2: a match with 2,000 cases fits the offered host-work limits.
#[test]
fn a_two_thousand_case_match_fits_the_offered_limits() {
    let mut par = resolved(
        "new x in { match 7 { 0 => { x!(0) } _ => { Nil } } }",
        &HashMap::new(),
    );
    let body = new_body(&mut par);
    let matched = body.matches.first_mut().expect("one match");
    let first = matched.cases.first().cloned().expect("a first case");
    matched.cases = (0..2_000i64)
        .map(|value| {
            let mut case = first.clone();
            case.pattern = Some(integer(value));
            if let Some(send) = case
                .source
                .as_mut()
                .and_then(|source| source.sends.first_mut())
            {
                send.data = vec![integer(value)];
            }
            case
        })
        .collect();
    let host = offered_limits();
    let estimate = estimate_par(&par, &host);
    dismantle_term(par);
    let estimate = estimate.expect("the offered limits admit a match with 2,000 cases");
    assert_eq!(estimate.completeness, Completeness::Complete);
    assert_eq!(
        counts(&estimate, deploy_lane())[0],
        1,
        "one case runs, so the branch maximum is one send"
    );
}

/// Review H-3: a list of 100,000 integers fits the offered host-work limits.
/// Its evaluation is bounded by its exact length, because the evaluation of
/// a list sorts nothing.
#[test]
fn a_list_of_one_hundred_thousand_elements_fits_the_offered_limits() {
    let mut par = resolved(
        "new x in { x!([0]) | for (@l <- x) { Nil } }",
        &HashMap::new(),
    );
    let body = new_body(&mut par);
    let datum = body
        .sends
        .first_mut()
        .and_then(|send| send.data.first_mut())
        .and_then(|datum| datum.exprs.first_mut())
        .and_then(|expr| expr.expr_instance.as_mut());
    let Some(ExprInstance::EListBody(list)) = datum else {
        panic!("the skeleton sends one list");
    };
    list.ps = (0..100_000i64).map(integer).collect();
    let host = offered_limits();
    let estimate = estimate_par(&par, &host);
    dismantle_term(par);
    let estimate = estimate.expect("the offered limits admit a list of 100,000 elements");
    assert_eq!(estimate.completeness, Completeness::Complete);
}

/// Review M-1: the normalizer builds a receive guard in the scope of the
/// receive's binders, so the check of rule X3 skips those binders. A guard
/// on the received values is complete. A guard that names a fresh name lets
/// the name escape.
#[test]
fn a_receive_guard_on_the_received_values_is_complete() {
    for source in [
        "new x in { x!(5) | for (@n <- x where n > 3) { Nil } }",
        "new x, y in { x!(5) | y!(2) | for (@n <- x & @m <- y where n > m) { Nil } }",
    ] {
        assert_eq!(
            estimate(source).completeness,
            Completeness::Complete,
            "{source}"
        );
    }
    assert_eq!(
        estimate("new x, y in { x!(5) | for (@n <- x where *y == n) { Nil } }").completeness,
        Completeness::Incomplete(IncompleteReason::EscapingName)
    );
}

/// Reviews M-5 and LOW 2: the body substitution of an enclosing receive gives
/// a URN level of a nested guard its value, which the guard sizing does not
/// see. So a guard that names a URN level has no size, also outside every
/// receive. A pattern level from outside the nearest enclosing receive
/// reaches the marker scan of that receive's body. A guard on the pattern of
/// the nearest enclosing receive stays complete.
#[test]
fn a_guard_that_names_a_urn_level_has_no_size() {
    let with_stdout = |source: &str| {
        estimate_with(source, [1; 4], stdout_urn_map(), &unlimited())
            .expect("the estimate runs")
            .completeness
    };
    for source in [
        "new x, y, out(`rho:io:stdout`) in { x!(1) | y!(2) | for (@n <- x) { for (@m <- y where m == *out) { Nil } } }",
        "new y, out(`rho:io:stdout`) in { y!(2) | for (@m <- y where m == *out) { Nil } }",
    ] {
        assert_eq!(
            with_stdout(source),
            Completeness::Incomplete(IncompleteReason::UnsizedValue),
            "{source}"
        );
    }
    assert_eq!(
        estimate(
            r#"new x, y, z in { z!("A") | x!(1) | for (@k <- z) { for (@n <- x) { y!(2) | for (@m <- y where m == k) { Nil } } } }"#
        )
        .completeness,
        Completeness::Incomplete(IncompleteReason::ReceivedValueInIntroduction)
    );
    assert_eq!(
        estimate(
            "new x, y in { x!(1) | for (@n <- x) { y!(2) | for (@m <- y where m > n) { Nil } } }"
        )
        .completeness,
        Completeness::Complete
    );
}

/// Review M-2: a signature taller than the sizing bound never reaches the
/// recursive canonicalization of the plan. The estimate is incomplete with
/// the reason `UnsizedValue` and returns no deep signature. The estimate and
/// the term drop on a thread with a 256 KiB stack.
#[test]
fn a_deep_quoted_signature_is_unsized_on_a_small_stack() {
    let mut quoted = integer(7);
    for _ in 0..1_000 {
        quoted = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(models::rhoapi::EList {
                ps: vec![quoted],
                ..Default::default()
            })),
        }]);
    }
    let mut par = resolved("new y in { {% y!(0) %}[ s ] }", &HashMap::new());
    new_body(&mut par)
        .cost_signed_terms
        .first_mut()
        .expect("one signed term")
        .signature = Some(CostSignature {
        value: Some(CostSignatureValue::Quote(quoted)),
    });
    let reducer = evaluator(HashMap::new());
    let outcome = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("estimate-deep-signature".to_string())
            .stack_size(256 * 1024)
            .spawn_scoped(scope, || {
                let estimate = linear_cost_estimate(
                    &par,
                    &funding(),
                    &schedule([1; 4]),
                    &reducer,
                    &unlimited(),
                )
                .expect("the estimate runs");
                let quoted = estimate
                    .signatures
                    .values()
                    .filter(|signature| {
                        matches!(signature.value, Some(CostSignatureValue::Quote(_)))
                    })
                    .count();
                (estimate.completeness, quoted)
            })
            .expect("spawn the deep-signature thread")
            .join()
            .expect("the estimate stays within a small stack")
    });
    std::thread::Builder::new()
        .name("estimate-deep-signature-teardown".to_string())
        .stack_size(256 * 1024)
        .spawn(move || dismantle_term(par))
        .expect("spawn the teardown thread")
        .join()
        .expect("the iterative teardown stays within a small stack");
    assert_eq!(
        outcome,
        (Completeness::Incomplete(IncompleteReason::UnsizedValue), 0)
    );
}

// Disabled by the second G1-4a fix (re-review HIGH-1): the charge no longer
// reads the capacity of the live map, which depends on the hash seed and on
// the group width after removals.
// `environment_copies_are_charged_the_fixed_bucket_count` replaces it.
// /// Review M-4: `pop` never shrinks the environment map, and `Env::shift`
// /// clones every bucket of it. A substitution is therefore charged the
// /// capacity of the map for the call and for each copy, not its length.
// #[test]
// fn environment_copies_are_charged_the_capacity_of_the_map() {
//     let reducer = evaluator(HashMap::new());
//     let core = isolated_evaluator(&reducer);
//     let host = unlimited();
//     let mut estimator = Estimator::new(&funding(), &core, &host).expect("an estimator");
//     estimator
//         .push_holes(1_000, BinderKind::ReceivePattern)
//         .expect("the holes are bound");
//     estimator.pop(999).expect("the holes are popped");
//     let capacity = u64::try_from(estimator.env.env_map.capacity()).expect("a small capacity");
//     assert_eq!(estimator.env.env_map.len(), 1);
//     assert!(capacity >= 1_000, "pop keeps the capacity: {capacity}");
//     let before = host.usage(HostWorkDimension::SubstitutionBindings).get();
//     estimator
//         .charge_substitution(
//             Metrics {
//                 height: 1,
//                 weight: 10,
//                 vars: 0,
//                 shifts: 3,
//             },
//             false,
//         )
//         .expect("an unlimited budget admits the charge");
//     assert_eq!(
//         host.usage(HostWorkDimension::SubstitutionBindings).get() - before,
//         4 * capacity,
//         "the call and three copies each cost the capacity"
//     );
// }

/// The capacity that `bucket_mask_to_capacity` of hashbrown gives a table of
/// `buckets` buckets.
fn table_capacity(buckets: u64) -> usize {
    let mask = usize::try_from(buckets).expect("a small bucket count") - 1;
    match mask {
        0..=7 => mask,
        _ => (mask + 1) / 8 * 7,
    }
}

/// Re-review HIGH-1: `environment_buckets` gives the bucket count of the
/// standard map for every capacity that the pre-pass requests, so the fixed
/// charge is the work of a copy. A change of the standard map makes this test
/// fail.
#[test]
fn environment_buckets_follow_the_standard_map() {
    for capacity in (1..=300).chain([1_000, 2_002, 4_096, 100_002, 200_002]) {
        let map = HashMap::<i32, Par>::with_capacity(capacity);
        assert_eq!(
            map.capacity(),
            table_capacity(environment_buckets(capacity)),
            "capacity {capacity}"
        );
    }
}

/// Re-review HIGH-1: the environment map is sized once from the peak binder
/// count. Pushes and pops never grow it, and a substitution costs the fixed
/// bucket count for the call and for each copy, whatever the removals left.
#[test]
fn environment_copies_are_charged_the_fixed_bucket_count() {
    let reducer = evaluator(HashMap::new());
    let core = isolated_evaluator(&reducer);
    let host = unlimited();
    let mut estimator = Estimator::new(&funding(), &core, &host).expect("an estimator");
    estimator.size_environment(1_000).expect("the map is sized");
    let sized = estimator.env.env_map.capacity();
    let buckets = environment_buckets(2_002);
    for round in 0..64usize {
        let count = 1 + (round * 37) % 1_000;
        estimator
            .push_holes(count, BinderKind::ReceivePattern)
            .expect("the holes are bound");
        let keep = count / 3;
        estimator.pop(count - keep).expect("the holes are popped");
        estimator.pop(keep).expect("the rest is popped");
        assert!(
            estimator.env.env_map.capacity() <= sized,
            "the map never grows: {} above {sized}",
            estimator.env.env_map.capacity()
        );
    }
    estimator
        .push_holes(1_000, BinderKind::ReceivePattern)
        .expect("the peak is bound");
    assert!(
        estimator.push_holes(1, BinderKind::ReceivePattern).is_err(),
        "the walk cannot pass the peak of the pre-pass"
    );
    estimator.pop(999).expect("the holes are popped");
    let before = host.usage(HostWorkDimension::SubstitutionBindings).get();
    estimator
        .charge_substitution(
            Metrics {
                height: 1,
                weight: 10,
                vars: 0,
                shifts: 3,
            },
            false,
        )
        .expect("an unlimited budget admits the charge");
    assert_eq!(
        host.usage(HostWorkDimension::SubstitutionBindings).get() - before,
        4 * buckets,
        "the call and three copies each cost the fixed bucket count"
    );
}

/// A program with 64 outer names and 40 receives of up to 12 binders, each
/// with a nested `new`, so pass 1 pushes and pops many binders.
fn churning_program() -> String {
    let names = (0..64)
        .map(|index| format!("a{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut parts = Vec::with_capacity(81);
    for index in 0..40usize {
        let channel = index % 64;
        let patterns = (0..1 + index % 12)
            .map(|position| format!("@p{position}"))
            .collect::<Vec<_>>()
            .join(", ");
        let data = (0..1 + index % 12)
            .map(|position| position.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!("a{channel}!({data})"));
        parts.push(format!(
            "for ({patterns} <- a{channel}) {{ new c in {{ c!({index}) | for (@q <- c) {{ Nil }} }} }}"
        ));
    }
    parts.push("Nil".to_string());
    format!("new {names} in {{ {} }}", parts.join(" | "))
}

/// Re-review HIGH-1: each estimate creates its maps with a fresh random hash
/// seed. A program with 64 outer names and many removals gets the same usage
/// in every host-work dimension in every run.
#[test]
fn the_charges_do_not_depend_on_the_hash_seed() {
    let source = churning_program();
    let urn_map = HashMap::new();
    let par = resolved(&source, &urn_map);
    let reducer = evaluator(urn_map);
    let usages = (0..32)
        .map(|_| {
            let host = unlimited();
            let estimate =
                linear_cost_estimate(&par, &funding(), &schedule([1; 4]), &reducer, &host)
                    .expect("the estimate runs");
            (estimate, host.usages().into_array())
        })
        .collect::<Vec<_>>();
    dismantle_term(par);
    let (first_estimate, first_usage) = &usages[0];
    assert_eq!(first_estimate.completeness, Completeness::Complete);
    assert!(
        first_usage[dimension_index(HostWorkDimension::SubstitutionBindings)].get() > 0,
        "the program charges environment copies"
    );
    for (estimate, usage) in &usages[1..] {
        assert_eq!(estimate, first_estimate);
        assert_eq!(usage, first_usage);
    }
}

/// Re-review MEDIUM-1: at each nesting level the reducer copies or scans the
/// subtree of each compound expression. A string nested in 60 lists is
/// charged at least 60 times its length, and the same string in one list
/// much less.
#[test]
fn a_deeply_nested_list_is_charged_for_the_copies_of_each_level() {
    let text = "A".repeat(10_000);
    let charge = |depth: usize| {
        let mut datum = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString(text.clone())),
        }]);
        for _ in 0..depth {
            datum = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(models::rhoapi::EList {
                    ps: vec![datum],
                    ..Default::default()
                })),
            }]);
        }
        let mut par = resolved(
            "new x in { x!([0]) | for (@l <- x) { Nil } }",
            &HashMap::new(),
        );
        new_body(&mut par).sends.first_mut().expect("one send").data = vec![datum];
        let host = unlimited();
        let estimate = estimate_par(&par, &host);
        dismantle_term(par);
        assert_eq!(
            estimate.expect("the estimate runs").completeness,
            Completeness::Complete,
            "{depth} lists"
        );
        host.usage(HostWorkDimension::PrimitiveInputBytes).get()
    };
    let length = u64::try_from(text.len()).expect("a small length");
    let (shallow, deep) = (charge(1), charge(60));
    assert!(
        deep >= 60 * length,
        "60 levels cost at least 60 copies: {deep}"
    );
    assert!(
        shallow < 10 * length,
        "one level costs a few copies: {shallow}"
    );
}

/// Review LOW 1: inside a live reduction session the estimate returns an
/// error in every build, because the reducer's host-work reservations would
/// charge that session.
#[test]
fn the_estimate_rejects_a_live_reduction_session() {
    let par = resolved(
        "new x in { x!(0) | for (@n <- x) { Nil } }",
        &HashMap::new(),
    );
    let reducer = evaluator(HashMap::new());
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a test runtime")
        .block_on(crate::rust::interpreter::deterministic_reduction::root(
            reducer.space.clone(),
            reducer.metering.budget(),
            reducer.reduction_coordinator.clone(),
            async {
                linear_cost_estimate(&par, &funding(), &schedule([1; 4]), &reducer, &unlimited())
            },
        ));
    dismantle_term(par);
    assert!(
        matches!(result, Err(InterpreterError::BugFoundError(_))),
        "{result:?}"
    );
}

/// Review LOW 6: a funding signature without a cost signature, here a
/// threshold, leaves the deploy lane without units. The counts of that lane
/// drop out and the estimate is incomplete, instead of an error.
#[test]
fn an_unpriced_deploy_lane_makes_the_estimate_incomplete() {
    let par = resolved(
        "new x in { x!(0) | for (@n <- x) { Nil } }",
        &HashMap::new(),
    );
    let reducer = evaluator(HashMap::new());
    let threshold = Sig::Threshold {
        threshold: 1,
        members: vec![Sig::Ground(vec![9, 9, 9, 9])],
    };
    let estimate =
        linear_cost_estimate(&par, &threshold, &schedule([1; 4]), &reducer, &unlimited());
    dismantle_term(par);
    let estimate = estimate.expect("an unpriced lane is not an error");
    assert_eq!(
        estimate.completeness,
        Completeness::Incomplete(IncompleteReason::UnsizedValue)
    );
    assert!(
        estimate
            .quantities
            .keys()
            .all(|(lane, _)| *lane != threshold.lane_hash()),
        "the unpriced deploy lane has no count"
    );
}
