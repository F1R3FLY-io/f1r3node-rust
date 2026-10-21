//! G1-4a (DR-122): runtime tests of the linear pre-execution cost estimate.
//! A complete estimate bounds the native meter's measurement of every lane
//! and class (design §11 and §12), and the estimate leaves the runtime's
//! budget, cost log and event log untouched (arbitration M1).

use std::collections::{BTreeMap, HashMap};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rust::phlo_schedule::PhloGenesisPolicy;
use proptest::prelude::*;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use super::stack_safety_tests::{arb_program, render};
use super::tests::schedule;
use super::*;
use crate::rust::interpreter::accounting::authority::{
    cost_signature_to_sig, AuthorityByteEventKind,
};
use crate::rust::interpreter::accounting::lexical::resolve_lexical_names_for_funding;
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativeBoundSource, NativeBudgetTraceLimits, NativePhloExecutionContract, NativePhloRegionLimits,
};
use crate::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
};
use crate::rust::interpreter::accounting::NativeRuntimeConfig;
use crate::rust::interpreter::compiler::compiler::Compiler;
use crate::rust::interpreter::interpreter::EvaluateResult;
use crate::rust::interpreter::rho_runtime::RhoRuntimeImpl;
use crate::rust::interpreter::test_utils::resources::create_runtimes;

const WEIGHTS: [u64; 4] = [1, 1, 1, 1];
const LIMIT: u64 = 1_000_000_000_000;

fn runtime_funding() -> Sig { Sig::Ground(vec![4, 4, 4, 4]) }

/// Added by G3 (DR-135): the schedule of the tests at `price`.
fn priced_schedule(price: u64) -> PhloScheduleV1<'static> {
    let mut descriptor = schedule(WEIGHTS);
    descriptor.actual_price = price;
    descriptor
}

/// Added by G3 (DR-135): the signed limit at `price`: `LIMIT`, lowered so
/// that the signed charge ceiling `limit * price + 1` fits the machine
/// maximum.
fn priced_limit(price: u64) -> u64 { LIMIT.min((u64::MAX - 1) / price.max(1)) }

/// A native runtime configuration with the bound `LIMIT` under the signed
/// limit, so an overrun is a user failure and never a certificate failure.
///
/// Changed by G3 (DR-135): the configuration takes the price of the
/// schedule, and its limit is `priced_limit(price)`.
// fn native_config() -> NativeRuntimeConfig {
//     let descriptor = schedule(WEIGHTS);
fn priced_native_config(price: u64) -> NativeRuntimeConfig {
    let descriptor = priced_schedule(price);
    let limit = priced_limit(price);
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
            // Changed by G3 (DR-135): the limit at the price.
            // limit: LIMIT,
            limit,
            price_ceiling: descriptor.actual_price,
            required_owner_ceilings: &owner_ceilings,
            permitted_schedules: &schedules,
        },
        selected,
        // Changed by G3 (DR-135): the limit at the price.
        // LIMIT,
        limit,
    )
    .expect("the controls admit the limit");
    let contract =
        NativePhloExecutionContract::new(controls, &binding, NativeBoundSource::SignedLimit)
            .expect("a contract under the signed limit");
    NativeRuntimeConfig::new(contract, NativeBudgetTraceLimits {
        attempts: 100_000,
        path_segments: 4_096,
        regions: NativePhloRegionLimits {
            regions: 10_000,
            encoded_authority_bytes: 100_000_000,
        },
    })
}

/// The native meter's quantity of every lane and class: each region of an
/// observation's authority is charged the observation's quantity (design
/// §4.3).
fn measured(evaluation: &EvaluateResult) -> BTreeMap<(SigKey, usize), u64> {
    let mut quantities = BTreeMap::new();
    for row in &evaluation.byte_observations.rows {
        let raw = row.measurement.expect("a measured observation");
        let row_quantities = [
            u64::from(row.kind == AuthorityByteEventKind::Comm),
            raw.introduction_bytes,
            raw.transfer_bytes,
            raw.trace_bytes,
        ];
        for region in &row.authority.regions {
            let signature = region.signature.as_ref().expect("a region signature");
            let sig = cost_signature_to_sig(signature).expect("a canonical region signature");
            if sig == Sig::Unit {
                continue;
            }
            for (class, quantity) in row_quantities.into_iter().enumerate() {
                if quantity > 0 {
                    *quantities.entry((sig.lane_hash(), class)).or_insert(0) += quantity;
                }
            }
        }
    }
    quantities
}

async fn fresh_runtime() -> RhoRuntimeImpl {
    let mut stores = InMemoryStoreManager::new();
    let store = stores.r_space_stores().await.expect("in-memory stores");
    let (runtime, _, _) = create_runtimes(store, false, &mut Vec::new()).await;
    runtime
}

/// Estimates `source` and runs it under the native meter with the same seed.
/// For a complete estimate, every measured quantity is at most its estimate
/// and the measured usage is at most `B`. The estimate is returned for
/// further checks.
async fn assert_measured_within_estimate(source: &str, seed: &[u8]) -> Option<CostEstimate> {
    // Added by G3 (DR-135): the check at the price of the schedule.
    assert_measured_within_priced_estimate(source, seed, schedule(WEIGHTS).actual_price).await
}

// Changed by G3 (DR-135): the estimate and the run take one price, which
// every region of the run records (R4).
// async fn assert_measured_within_estimate(source: &str, seed: &[u8]) -> Option<CostEstimate> {
async fn assert_measured_within_priced_estimate(
    source: &str,
    seed: &[u8],
    price: u64,
) -> Option<CostEstimate> {
    let mut runtime = fresh_runtime().await;
    let urn_map = runtime.reducer.urn_map.clone();
    let program = Compiler::source_to_adt(source).ok()?;
    let rand = Blake2b512Random::create_from_bytes(seed);
    let resolved = resolve_lexical_names_for_funding(&program, rand.clone(), &urn_map).ok()?;
    dismantle_term(program);
    let estimate = linear_cost_estimate(
        &resolved,
        &runtime_funding(),
        // Changed by G3 (DR-135): the schedule at the price.
        // &schedule(WEIGHTS),
        &priced_schedule(price),
        &runtime.reducer,
    )
    .expect("the estimate runs");
    dismantle_term(resolved);
    runtime
        .cost
        .set_deploy_signature_funded(seed, runtime_funding());
    // Changed by G3 (DR-135): the configuration at the price.
    // let evaluation = runtime
    //     .evaluate_with_native_phlo(source, HashMap::new(), rand, None, native_config())
    //     .await
    //     .expect("the native evaluation runs");
    let evaluation = runtime
        .evaluate_with_native_phlo(
            source,
            HashMap::new(),
            rand,
            None,
            priced_native_config(price),
        )
        .await
        .expect("the native evaluation runs");
    if estimate.completeness == Completeness::Complete {
        assert!(
            evaluation.errors.is_empty(),
            "{source}: {:?}",
            evaluation.errors
        );
        let measured = measured(&evaluation);
        for (key, quantity) in &measured {
            let estimated = estimate.quantities.get(key).copied().unwrap_or(0);
            assert!(
                *quantity <= estimated,
                "{source}: lane {:?} class {} measured {quantity} above the estimate {estimated}",
                &key.0[..4],
                key.1
            );
        }
        let usage = evaluation.native_phlo_usage.expect("a native usage");
        assert!(
            usage <= estimate.bound_phlo,
            "{source}: usage {usage} above the estimate {}",
            estimate.bound_phlo
        );
    }
    Some(estimate)
}

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("a test runtime")
        .block_on(future)
}

/// The named programs of design §12: the term of §10.1, a two-signer offer, a
/// join, a branch on a received value, the join and the interpolation of
/// §10.3 and §10.4, nested scopes and a dequoted name. The normalizer rejects
/// a join with a repeated channel, so `tests.rs` builds that join in code.
/// The G1-4a fix adds a guard on the received value (review M-1), a nested
/// guard on the pattern of the enclosing receive, and the quote of a closed
/// process and of a resolved name (review H-1).
#[test]
fn named_complete_programs_bound_their_measurement() {
    let programs = [
        "{% new x in { x!(0) | for (@n <- x) { Nil } } %}[ payer ]",
        "{% new x in { x!(0) | for (@n <- x) { Nil } } %}[ a (*) b ]",
        "new x, y in { x!(1) | y!(2) | for (@a <- x & @b <- y) { Nil } }",
        "new x, y in { x!(5) | for (@n <- x) { if (n > 3) { y!(1) | for (_ <- y) { Nil } } else { Nil } } }",
        "new x, y in { x!(2) | for (@n <- x) { if (n > 3) { y!(1) | for (_ <- y) { Nil } } else { Nil } } }",
        "new x, y in { x!(0) | y!(0) | for ({% @a <- x %}[ s ] & {% @b <- y %}[ s ]) { Nil } }",
        r#"new x in { x!("${a}${a}${a}${a}" %% {"a": "abcdefghij"}) | for (@n <- x) { Nil } }"#,
        "new x in { {% x!(0) | {% for (@n <- x) { Nil } %}[ inner ] %}[ outer ] | Nil }",
        "new x in { x!(0) | for ({% @n <- x %}[ s ]) { Nil } }",
        "new x in { x!(0) | for (@n <- x) { *x } }",
        "new x, y in { x!([1, \"a\"] ++ [2]) | for (@n <- x) { y!(\"a\" ++ \"b\") | for (@m <- y) { Nil } } }",
        "new x in { {% {% {% x!(0) | for (@n <- x) { Nil } %}[ a ] %}[ b ] %}[ c ] }",
        "new x in { x!(5) | for (@n <- x where n > 3) { Nil } }",
        "new x, y in { x!(1) | for (@n <- x) { y!(2) | for (@m <- y where m > n) { Nil } } }",
        r#"new y in { {% y!(0) | for (_ <- y) { Nil } %}[ #"AAAA" ] }"#,
        "new y in { {% y!(0) | for (_ <- y) { Nil } %}[ #*y ] }",
    ];
    block_on(async {
        for (index, source) in programs.iter().enumerate() {
            let estimate = assert_measured_within_estimate(source, &[index as u8; 8])
                .await
                .expect("the named program compiles and resolves");
            assert_eq!(estimate.completeness, Completeness::Complete, "{source}");
        }
    });
}

/// Reviews M-5 and LOW 2: the guard of a nested receive names a URN level
/// outside the enclosing receive. The body substitution of the enclosing
/// receive gives that level its value. Before the fix the estimate was
/// complete, and the run measured 4 introduction bytes more than its
/// estimate. The estimate is now incomplete.
#[test]
fn a_nested_guard_on_a_urn_level_is_incomplete() {
    let source = "new x, y, out(`rho:io:stdout`) in { x!(1) | y!(2) | for (@n <- x) { for (@m <- y where m == *out) { Nil } } }";
    let estimate = block_on(assert_measured_within_estimate(source, &[40; 8]))
        .expect("the program compiles and resolves");
    assert_eq!(
        estimate.completeness,
        Completeness::Incomplete(IncompleteReason::UnsizedValue)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Generated programs of the complete profile: whenever the estimate is
    /// complete, the native meter measures at most the estimate.
    #[test]
    fn generated_complete_programs_bound_their_measurement(program in arb_program(true), seed in any::<[u8; 8]>()) {
        let source = render(&program);
        block_on(assert_measured_within_estimate(&source, &seed));
    }

    /// Added by G3 (DR-135): every region of a run records the run's price,
    /// so its stored seal grows by the price's varint. The estimate sizes each
    /// region at that price, so a complete estimate still bounds every
    /// measured quantity. The prices cover each varint length from zero bytes
    /// (price 0, which prost omits) to six bytes.
    #[test]
    fn the_estimate_still_bounds_measured_bytes(
        program in arb_program(true),
        seed in any::<[u8; 8]>(),
        price in prop_oneof![
            Just(0_u64),
            Just(1_u64),
            Just(127_u64),
            Just(128_u64),
            Just(16_383_u64),
            Just(16_384_u64),
            Just((1_u64 << 21) - 1),
            Just(1_u64 << 21),
            Just((1_u64 << 28) - 1),
            Just(1_u64 << 28),
            Just(1_u64 << 35),
            1_u64..(1_u64 << 35),
        ],
    ) {
        let source = render(&program);
        block_on(assert_measured_within_priced_estimate(&source, &seed, price));
    }
}

/// Added by G3 (DR-135): the price property is not vacuous. A priced region
/// is longer than an unpriced one, and the named programs keep a complete
/// estimate that bounds their measurement at a six-byte price.
#[test]
fn a_priced_region_is_longer_and_still_bounded() {
    let signature = sig_to_cost_signature(&runtime_funding()).expect("a ground funding signature");
    let unpriced = region_field_bytes(&signature, 0, 0).expect("a sized region");
    let priced = region_field_bytes(&signature, 0, 1 << 35).expect("a sized region");
    assert_eq!(
        priced,
        unpriced + 1 + 6,
        "a field tag and a six-byte varint"
    );
    block_on(async {
        for (index, source) in [
            "{% new x in { x!(0) | for (@n <- x) { Nil } } %}[ payer ]",
            "new x, y in { x!(0) | y!(0) | for ({% @a <- x %}[ s ] & {% @b <- y %}[ s ]) { Nil } }",
        ]
        .iter()
        .enumerate()
        {
            let estimate =
                assert_measured_within_priced_estimate(source, &[index as u8; 8], 1 << 35)
                    .await
                    .expect("the named program compiles and resolves");
            assert_eq!(estimate.completeness, Completeness::Complete, "{source}");
        }
    });
}

/// The estimate evaluates with a copy of the reducer whose two meters are
/// fresh, so the runtime's cost, cost log, event log and evaluation counters
/// stay as they were (arbitration M1).
#[test]
fn the_estimate_leaves_the_runtime_budget_untouched() {
    block_on(async {
        let runtime = fresh_runtime().await;
        let source = r#"new x, y in { x!("${a}" %% {"a": "b"}, [1] ++ [2], 3 + 4) | y!(5) | for (@n, @k, @j <- x & @m <- y) { if (n == 1) { Nil } else { Nil } } }"#;
        let program = Compiler::source_to_adt(source).expect("the program compiles");
        let resolved = resolve_lexical_names_for_funding(
            &program,
            Blake2b512Random::create_from_bytes(b"isolation"),
            &runtime.reducer.urn_map,
        )
        .expect("the program resolves");
        dismantle_term(program);
        let cost = runtime.cost.get();
        let cost_log = runtime.get_cost_log();
        let event_log = runtime.cost.get_event_log();
        let stats = runtime.reducer.eval_work_stats();
        let estimate = linear_cost_estimate(
            &resolved,
            &runtime_funding(),
            &schedule(WEIGHTS),
            &runtime.reducer,
        )
        .expect("the estimate runs");
        dismantle_term(resolved);
        assert_eq!(estimate.completeness, Completeness::Complete);
        assert_eq!(runtime.cost.get(), cost);
        assert_eq!(runtime.get_cost_log(), cost_log);
        assert_eq!(runtime.cost.get_event_log(), event_log);
        assert_eq!(runtime.reducer.eval_work_stats(), stats);
    });
}

/// The differential property is not vacuous: most generated programs of the
/// complete profile compile, get a complete estimate and run under the
/// native meter.
#[test]
fn generated_complete_programs_reach_the_runtime() {
    let sources = super::stack_safety_tests::sample_programs(true, 32);
    let complete = block_on(async {
        let mut complete = 0usize;
        for (index, source) in sources.iter().enumerate() {
            let estimate = assert_measured_within_estimate(source, &[index as u8; 8]).await;
            if estimate.is_some_and(|estimate| estimate.completeness == Completeness::Complete) {
                complete += 1;
            }
        }
        complete
    });
    assert!(
        complete >= 24,
        "only {complete} of 32 programs ran with a complete estimate"
    );
}
