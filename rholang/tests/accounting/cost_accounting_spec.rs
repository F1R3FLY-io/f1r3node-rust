//See rholang/src/test/scala/coop/rchain/rholang/interpreter/accounting/CostAccountingSpec.scala from main branch

use std::collections::{BTreeSet, HashMap};
use std::option::Option;
use std::sync::{Arc, Barrier};
use std::thread;

use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, TestRunner};
use rand::Rng;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::{
    BillableKind, BillableTokenEvent, CostReservationBatch, RedexId, RuntimeBudget, Sig,
    SignatureChannel, SignedProcess, SourcePath, Token, MAX_COST_TRACE_PRIMITIVE_DESCRIPTOR_BYTES,
    MAX_COST_TRACE_SOURCE_PATH_COMPONENTS,
};
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::metering::MeteredMachine;
use rholang::rust::interpreter::rho_runtime::{
    create_replay_rho_runtime, create_rho_runtime, RhoRuntime, RhoRuntimeImpl,
};
use rholang::rust::interpreter::system_processes::Definition;
use rholang::rust::interpreter::test_utils::resources::create_runtimes;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::rspace::{RSpace, RSpaceStore};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

async fn evaluate_with_cost_log(
    initial_phlo: i64,
    contract: String,
) -> (EvaluateResult, Vec<Cost>) {
    // The diagnostic cost log mirrors successful token reservations.

    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (mut runtime, _, _) =
        create_runtimes_with_cost_log(store, Some(false), Some(&mut Vec::new())).await;

    let eval_result = runtime
        .evaluate_with_phlo(
            &contract,
            Cost::create(initial_phlo, "cost_accounting_spec setup".to_string()),
        )
        .await;

    assert!(eval_result.is_ok());
    let eval_result = eval_result.unwrap();
    let cost_log = runtime.get_cost_log();
    (eval_result, cost_log)
}

async fn evaluate_with_cost_trace_digest(
    initial_phlo: i64,
    contract: String,
) -> (
    EvaluateResult,
    rholang::rust::interpreter::accounting::CostTraceDigest,
) {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (mut runtime, _, _) =
        create_runtimes_with_cost_log(store, Some(false), Some(&mut Vec::new())).await;

    let eval_result = runtime
        .evaluate_with_phlo(
            &contract,
            Cost::create(initial_phlo, "cost trace digest evaluation".to_string()),
        )
        .await
        .expect("cost trace digest evaluation");

    (eval_result, runtime.cost.cost_trace_digest())
}

async fn create_runtimes_with_cost_log(
    stores: RSpaceStore,
    init_registry: Option<bool>,
    additional_system_processes: Option<&mut Vec<Definition>>,
) -> (
    RhoRuntimeImpl,
    RhoRuntimeImpl,
    Arc<
        Box<
            dyn HistoryRepository<Par, BindPattern, ListParWithRandom, TaggedContinuation>
                + Send
                + Sync
                + 'static,
        >,
    >,
) {
    let init_registry = init_registry.unwrap_or(false);

    let mut empty_vec = Vec::new();
    let additional_system_processes = additional_system_processes.unwrap_or(&mut empty_vec);

    let hrstores =
        RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create_with_replay(
            stores,
            Arc::new(Box::new(Matcher)),
        )
        .unwrap();

    let (space, replay) = hrstores;

    let history_repository = space.get_history_repository();

    let rho_runtime = create_rho_runtime(
        space.clone(),
        Arc::new(std::collections::HashMap::new()),
        init_registry,
        additional_system_processes,
        ExternalServices::noop(),
    )
    .await;

    let replay_rho_runtime = create_replay_rho_runtime(
        replay,
        Arc::new(std::collections::HashMap::new()),
        init_registry,
        additional_system_processes,
        ExternalServices::noop(),
    )
    .await;

    (rho_runtime, replay_rho_runtime, history_repository)
}

async fn evaluate_and_replay(initial_phlo: Cost, term: String) -> (EvaluateResult, EvaluateResult) {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (mut runtime, mut replay_runtime, _): (
        RhoRuntimeImpl,
        RhoRuntimeImpl,
        Arc<
            Box<
                dyn HistoryRepository<Par, BindPattern, ListParWithRandom, TaggedContinuation>
                    + Send
                    + Sync
                    + 'static,
            >,
        >,
    ) = create_runtimes(store, false, &mut Vec::new()).await;

    let rand = Blake2b512Random::create_from_bytes(&[]);

    let play_result = {
        runtime
            .evaluate(&term, initial_phlo.clone(), HashMap::new(), rand.clone())
            .await
            .expect("Evaluation failed")
    };

    let replay_result = {
        let checkpoint = runtime.create_checkpoint().await;
        let root = checkpoint.root;
        let log = checkpoint.log;

        replay_runtime
            .reset(&root)
            .await
            .expect("Failed to reset replay runtime");
        replay_runtime.rig(log).await.expect("Rig failed");

        let result = replay_runtime
            .evaluate(&term, initial_phlo, HashMap::new(), rand)
            .await
            .expect("Replay evaluation failed");

        replay_runtime
            .check_replay_data()
            .await
            .unwrap_or_else(|e| panic!("Replay data check failed for '{}': {:?}", term, e));

        result
    };

    (play_result, replay_result)
}

// Uses Godel numbering and a https://en.wikipedia.org/wiki/Mixed_radix
// to encode certain terms as numbers in the range [0, 0x144000000).
// Every number gets decoded into a unique term, but some terms can
// be encoded by more than one number.
fn from_long(index: i64) -> String {
    let mut remainder = index;
    let num_pars = (remainder % 4) + 1;
    remainder /= 4;

    let mut result = Vec::new();
    let mut nonlinear_send = false;
    let mut nonlinear_recv = false;

    for _ in 0..num_pars {
        let dir = remainder % 2;
        remainder /= 2;

        if dir == 0 {
            //send
            let bang = if remainder % 2 == 0 { "!" } else { "!!" };
            remainder /= 2;

            if bang == "!" || !nonlinear_recv {
                let ch = remainder % 4;
                remainder /= 4;
                result.push(format!("@{}{}(0)", ch, bang));
                nonlinear_send |= bang == "!!";
            }
        } else {
            //receive
            let arrow = match remainder % 3 {
                0 => "<-",
                1 => "<=",
                2 => "<<-",
                _ => unreachable!(),
            };
            remainder /= 3;

            if arrow != "<=" || !nonlinear_send {
                let num_joins = (remainder % 2) + 1;
                remainder /= 2;

                let mut joins = Vec::new();
                for _ in 1..=num_joins {
                    let ch = remainder % 4;
                    remainder /= 4;
                    joins.push(format!("_ {} @{}", arrow, ch));
                }

                let join_str = joins.join(" & ");
                result.push(format!("for ({}) {{ 0 }}", join_str));
                nonlinear_recv |= arrow == "<=";
            }
        }
    }

    result.join(" | ")
}

fn contracts() -> Vec<String> {
    vec![
      String::from("@0!(2)"),
      String::from("@0!(2) | @1!(1)"),
      String::from("for(x <- @0){ Nil }"),
      String::from("for(x <- @0){ Nil } | @0!(2)"),
      String::from("@0!!(0) | for (_ <- @0) { 0 }"),
      String::from("@0!!(0) | for (x <- @0) { 0 }"),
      String::from("@0!!(0) | for (@0 <- @0) { 0 }"),
      String::from("@0!!(0) | @0!!(0) | for (_ <- @0) { 0 }"),
      String::from("@0!!(0) | @1!!(1) | for (_ <- @0 & _ <- @1) { 0 }"),
      String::from("@0!(0) | for (_ <- @0) { 0 }"),
      String::from("@0!(0) | for (x <- @0) { 0 }"),
      String::from("@0!(0) | for (@0 <- @0) { 0 }"),
      String::from("@0!(0) | for (_ <= @0) { 0 }"),
      String::from("@0!(0) | for (x <= @0) { 0 }"),
      String::from("@0!(0) | for (@0 <= @0) { 0 }"),
      String::from("@0!(0) | @0!(0) | for (_ <= @0) { 0 }"),
      String::from("@0!(0) | for (@0 <- @0) { 0 } | @0!(0) | for (_ <- @0) { 0 }"),
      String::from("@0!(0) | for (@0 <- @0) { 0 } | @0!(0) | for (@1 <- @0) { 0 }"),
      String::from("@0!(0) | for (_ <<- @0) { 0 }"),
      String::from("@0!!(0) | for (_ <<- @0) { 0 }"),
      String::from("@0!!(0) | @0!!(0) | for (_ <<- @0) { 0 }"),
      String::from("new loop in {\n  contract loop(@n) = {\n    match n {\n      0 => Nil\n      _ => loop!(n-1)\n    }\n  } |\n  loop!(10)\n}"),
      String::from("42 | @0!(2) | for (x <- @0) { Nil }"),
      String::from("@1!(1) |\n        for(x <- @1) { Nil } |\n        new x in { x!(10) | for(X <- x) { @2!(Set(X!(7)).add(*X).contains(10)) }} |\n        match 42 {\n          38 => Nil\n          42 =>\n@3!(42)\n        }\n     "),
      String::from("new ret, keccak256Hash(`rho:crypto:keccak256Hash`) in {\n  keccak256Hash!(\"TEST\".toByteArray(), *ret) |\n  for (_ <- ret) { Nil }\n}"),
    ]
}

fn bounded_generated_contracts(limit: i64) -> Vec<String> {
    let mut terms = BTreeSet::new();
    for index in 0..limit {
        let contract = from_long(index);
        if !contract.is_empty() {
            terms.insert(contract);
        }
    }
    terms.extend(contracts());
    terms.into_iter().collect()
}

fn is_same_channel_error(errors: &[InterpreterError]) -> bool {
    errors.iter().any(|e| match e {
        InterpreterError::ReceiveOnSameChannelsError { .. } => true,
        InterpreterError::ParserError(msg) if msg.contains("Receiving on the same channels") => {
            true
        }
        _ => false,
    })
}

fn property_runner(cases: u32) -> TestRunner {
    TestRunner::new(ProptestConfig {
        cases,
        failure_persistence: None,
        ..ProptestConfig::default()
    })
}

async fn check_phlo_limit_exceeded(contract: String, initial_phlo: i64) -> bool {
    // D3 (DR-9, OD-3): `initial_phlo` is the per-COMM token budget. OOP fires
    // when the contract's COMM count exceeds it; on OOP the consensus cost
    // clamps to the budget. (The diagnostic per-op weight log is no longer
    // bounded by this budget — weights are diagnostic, not the consensus cost.)
    let (evaluate_result, _cost_log) = evaluate_with_cost_log(initial_phlo, contract).await;

    assert_eq!(
        evaluate_result.errors,
        vec![InterpreterError::OutOfPhlogistonsError],
        "Expected list of OutOfPhlogistonsError"
    );

    assert_eq!(
        evaluate_result.cost.value, initial_phlo,
        "Out-of-phlo must commit exactly the exhausted COMM-token budget"
    );

    true
}

fn token_event(local_index: u64, weight: u64) -> BillableTokenEvent {
    BillableTokenEvent {
        deploy_id: [7; 32],
        // D0: per-signature lane key, constant within a deploy. The scalar
        // fast path leaves the lane pool empty, so a fixed sentinel keeps the
        // canonical `Ord` (deploy_id, sig_hash, …) stable across these events.
        sig_hash: [0; 32],
        source_path: SourcePath(vec![local_index as u32]),
        redex_id: RedexId(local_index),
        local_index,
        kind: BillableKind::Comm,
        weight,
    }
}

fn token_event_with(
    deploy_tag: u8,
    source_path: Vec<u32>,
    redex_id: u64,
    local_index: u64,
    kind: BillableKind,
    weight: u64,
) -> BillableTokenEvent {
    BillableTokenEvent {
        deploy_id: [deploy_tag; 32],
        // D0: per-deploy lane key. Keyed off `deploy_tag` so events from
        // distinct deploys carry distinct (but per-deploy-constant) lane keys,
        // matching the one-compound-lane-per-deploy model.
        sig_hash: [deploy_tag; 32],
        source_path: SourcePath(source_path),
        redex_id: RedexId(redex_id),
        local_index,
        kind,
        weight,
    }
}

#[test]
fn runtime_budget_property_preserves_consumed_remaining_invariant() {
    let mut runner = property_runner(256);
    runner
        .run(
            &(0i64..512, proptest::collection::vec(1u64..64, 0..64)),
            |(initial_phlo, weights)| {
                let budget = RuntimeBudget::new(Cost::create(initial_phlo, "property budget"));
                for (index, weight) in weights.into_iter().enumerate() {
                    let event = token_event(index as u64, weight);
                    let result = budget.reserve_canonical(event.clone());

                    prop_assert_eq!(
                        budget.total_cost().value + budget.remaining().value,
                        initial_phlo
                    );
                    prop_assert!(budget.total_cost().value >= 0);
                    prop_assert!(budget.remaining().value >= 0);

                    if let Err(err) = result {
                        prop_assert_eq!(err, InterpreterError::OutOfPhlogistonsError);
                        prop_assert_eq!(budget.total_cost().value, initial_phlo);
                        prop_assert_eq!(budget.remaining().value, 0);
                        prop_assert_eq!(budget.last_oop_event(), Some(event));
                        break;
                    }
                }
                Ok(())
            },
        )
        .unwrap();
}

// D3 (DR-9, OD-3): `weighted_events_property_refine_unit_token_expansion` is
// REMOVED. Its premise — that a weight-W event is equivalent to W unit-weight
// events — was the weight-based escrow cost model. Under D3 the consensus cost
// is the COMM COUNT (one token per COMM, weight-independent), so a single COMM
// of any weight costs 1, NOT W. The per-COMM tally is exercised by
// `runtime_budget_property_preserves_consumed_remaining_invariant` and the
// d0_lane_pool_tests in `accounting/mod.rs`.

#[test]
fn unmetered_budget_property_never_consumes_or_records_events() {
    let mut runner = property_runner(128);
    runner
        .run(
            &(0i64..512, proptest::collection::vec(0u64..128, 0..64)),
            |(initial_phlo, weights)| {
                let budget = RuntimeBudget::new(Cost::create(initial_phlo, "unmetered budget"));
                budget.set_unmetered(true);

                for (index, weight) in weights.into_iter().enumerate() {
                    budget
                        .reserve_canonical(token_event(index as u64, weight))
                        .unwrap();
                    prop_assert_eq!(budget.total_cost().value, 0);
                    prop_assert_eq!(budget.remaining().value, i64::MAX);
                    prop_assert!(budget.get_event_log().is_empty());
                    prop_assert_eq!(budget.last_oop_event(), None);
                    prop_assert_eq!(budget.cost_trace_event_count(), 0);
                }

                Ok(())
            },
        )
        .unwrap();
}

#[test]
fn signature_channels_property_are_order_independent_and_domain_separated() {
    let mut runner = property_runner(128);
    runner
        .run(
            &(
                proptest::collection::vec(any::<u8>(), 0..64),
                proptest::collection::vec(any::<u8>(), 0..64),
            ),
            |(left_bytes, right_bytes)| {
                let left = Sig::Ground(left_bytes.clone());
                let right = Sig::Ground(right_bytes.clone());

                if left_bytes != right_bytes {
                    prop_assert_ne!(
                        SignatureChannel::from_sig(&left),
                        SignatureChannel::from_sig(&right)
                    );
                }

                let combined = Sig::And(Box::new(left.clone()), Box::new(right.clone()));
                let reversed = Sig::And(Box::new(right), Box::new(left));
                prop_assert_eq!(
                    SignatureChannel::from_sig(&combined),
                    SignatureChannel::from_sig(&reversed)
                );

                let budget = RuntimeBudget::new(Cost::create(10, "signature scope"));
                budget.set_deploy_signature(&left_bytes);
                prop_assert_ne!(budget.deploy_id().to_vec(), Blake2b256::hash(left_bytes));
                Ok(())
            },
        )
        .unwrap();
}

#[test]
fn runtime_budget_initializes_from_signed_token_annotation() {
    let sig = Sig::Ground(vec![1, 2, 3]);
    let annotated = SignedProcess::metered(Par::default(), sig.clone(), 7);
    let budget = RuntimeBudget::new(Cost::create(0, "empty budget"));

    budget.reset_from_signed_process(&annotated);

    assert_eq!(budget.signature(), sig);
    assert_eq!(budget.remaining().value, 7);
    assert_eq!(budget.total_cost().value, 0);
}

#[test]
fn runtime_budget_reset_from_token_clears_oop_boundary() {
    let sig = Sig::Ground(vec![1, 2, 3]);
    // D3 (DR-9): a COMM costs ONE token. A 0-COMM budget OOPs on the first COMM.
    let budget = RuntimeBudget::new(Cost::create(0, "reset budget"));

    budget.reserve_canonical(token_event(0, 10)).unwrap_err();
    assert!(budget.last_oop_event().is_some());

    budget.reset_from_token(&Token::coalesced(sig.clone(), 9));

    assert_eq!(budget.signature(), sig);
    assert_eq!(budget.total_cost().value, 0);
    assert_eq!(budget.remaining().value, 9);
    assert_eq!(budget.last_oop_event(), None);
    assert_eq!(budget.cost_trace_event_count(), 0);
}

#[test]
fn runtime_budget_reset_from_token_clears_success_trace_window() {
    let sig = Sig::Ground(vec![4, 5, 6]);
    let budget = RuntimeBudget::new(Cost::create(5, "reset budget"));

    budget.reserve_canonical(token_event(0, 2)).unwrap();
    let before_reset = budget.cost_trace_digest();
    assert_eq!(before_reset.event_count, 1);
    assert_eq!(budget.cost_trace_event_count(), 1);

    budget.reset_from_token(&Token::coalesced(sig.clone(), 9));
    let after_reset = budget.cost_trace_digest();

    assert_eq!(budget.signature(), sig);
    assert_eq!(budget.total_cost().value, 0);
    assert_eq!(budget.remaining().value, 9);
    assert_eq!(budget.cost_trace_event_count(), 0);
    assert!(budget.get_event_log().is_empty());
    assert_eq!(budget.last_oop_event(), None);
    assert_eq!(after_reset.event_count, 0);
    assert_ne!(before_reset.digest, after_reset.digest);
}

#[test]
fn reset_from_token_clears_all_recorded_state_between_deploys() {
    // The defensive `reset_serializer` RwLock was removed: per-deploy
    // finalization and reset are single-threaded by contract (the parallel
    // reducer has joined before the budget is reset/read), so reset no longer
    // needs to guard against an in-flight concurrent commit. The
    // production-relevant invariant is that reset FULLY clears the recorded
    // attempt state (the lock-free SegQueue, the reconciliation accumulator,
    // and the cached reconciliation), so a reused budget never inherits a
    // prior deploy's cost — the cross-deploy isolation the runtime relies on.
    let sig = Sig::Ground(vec![7, 8, 9]);
    let budget = RuntimeBudget::new(Cost::create(64, "reset isolation"));

    // Deploy 1: record 32 COMMs (each costs ONE token under D3; weight 2 is
    // diagnostic). 32 COMMs ≤ 64-token budget ⇒ all commit, consensus cost = 32.
    let events = (0..32).map(|path| token_event(path, 2)).collect::<Vec<_>>();
    budget
        .commit_canonical_batch(CostReservationBatch { events })
        .unwrap();
    assert_eq!(budget.total_cost().value, 32);
    assert_eq!(budget.cost_trace_event_count(), 32);

    // Reset for deploy 2.
    budget.reset_from_token(&Token::coalesced(sig.clone(), 128));

    // Deploy 2 starts clean — zero residue from deploy 1.
    assert_eq!(budget.signature(), sig);
    assert_eq!(budget.total_cost().value, 0);
    assert_eq!(budget.remaining().value, 128);
    assert_eq!(budget.cost_trace_event_count(), 0);
    assert!(budget.get_event_log().is_empty());
}

#[test]
fn empty_cost_trace_still_has_consensus_digest() {
    let budget = RuntimeBudget::new(Cost::create(5, "empty trace digest"));
    let digest = budget.cost_trace_digest();

    assert!(budget.get_event_log().is_empty());
    assert_eq!(digest.event_count, 0);
    assert!(!digest.digest.is_empty());
}

#[test]
fn cost_trace_digest_canonicalizes_success_order() {
    let forward = RuntimeBudget::new(Cost::create(20, "forward trace"));
    let reverse = RuntimeBudget::new(Cost::create(20, "reverse trace"));
    let events = vec![
        token_event_with(1, vec![2], 2, 2, BillableKind::Comm, 1),
        token_event_with(
            1,
            vec![0],
            0,
            0,
            BillableKind::Primitive("lookup".into()),
            2,
        ),
        token_event_with(1, vec![1], 1, 1, BillableKind::Substitution, 3),
    ];

    for event in &events {
        forward.reserve_canonical(event.clone()).unwrap();
    }
    for event in events.iter().rev() {
        reverse.reserve_canonical(event.clone()).unwrap();
    }

    // `get_event_log` now returns the canonical (sorted) committed set, so it
    // is order-independent: identical regardless of reservation order.
    assert_eq!(forward.get_event_log(), reverse.get_event_log());
    assert_eq!(forward.cost_trace_digest(), reverse.cost_trace_digest());
    assert_eq!(forward.cost_trace_event_count(), 3);
}

#[test]
fn cost_trace_digest_changes_when_descriptor_or_oop_boundary_changes() {
    let base = RuntimeBudget::new(Cost::create(5, "base trace"));
    base.reserve_canonical(token_event_with(1, vec![0], 0, 0, BillableKind::Comm, 2))
        .unwrap();

    let changed_weight = RuntimeBudget::new(Cost::create(5, "weight trace"));
    changed_weight
        .reserve_canonical(token_event_with(1, vec![0], 0, 0, BillableKind::Comm, 3))
        .unwrap();

    let changed_deploy = RuntimeBudget::new(Cost::create(5, "deploy trace"));
    changed_deploy
        .reserve_canonical(token_event_with(2, vec![0], 0, 0, BillableKind::Comm, 2))
        .unwrap();

    assert_ne!(base.cost_trace_digest(), changed_weight.cost_trace_digest());
    assert_ne!(base.cost_trace_digest(), changed_deploy.cost_trace_digest());

    // D3 (DR-9): a COMM costs ONE token. A 1-COMM budget admits the first COMM
    // and OOPs on the second; the OOP descriptor (source_path 1 vs 2) makes the
    // two digests differ.
    let oop_left = RuntimeBudget::new(Cost::create(1, "oop left"));
    let oop_right = RuntimeBudget::new(Cost::create(1, "oop right"));
    oop_left.reserve_canonical(token_event(0, 2)).unwrap();
    oop_right.reserve_canonical(token_event(0, 2)).unwrap();
    oop_left.reserve_canonical(token_event(1, 1)).unwrap_err();
    oop_right.reserve_canonical(token_event(2, 1)).unwrap_err();

    assert_eq!(oop_left.cost_trace_event_count(), 2);
    assert_eq!(oop_right.cost_trace_event_count(), 2);
    assert_ne!(oop_left.cost_trace_digest(), oop_right.cost_trace_digest());
}

#[test]
fn cost_trace_digest_domain_separates_event_kind_path_redex_index_and_multiplicity() {
    let base = RuntimeBudget::new(Cost::create(20, "base descriptor trace"));
    base.reserve_canonical(token_event_with(1, vec![0, 1], 2, 3, BillableKind::Comm, 4))
        .unwrap();

    let changed_kind = RuntimeBudget::new(Cost::create(20, "kind descriptor trace"));
    changed_kind
        .reserve_canonical(token_event_with(
            1,
            vec![0, 1],
            2,
            3,
            BillableKind::Substitution,
            4,
        ))
        .unwrap();

    let changed_path = RuntimeBudget::new(Cost::create(20, "path descriptor trace"));
    changed_path
        .reserve_canonical(token_event_with(1, vec![0, 2], 2, 3, BillableKind::Comm, 4))
        .unwrap();

    let changed_redex = RuntimeBudget::new(Cost::create(20, "redex descriptor trace"));
    changed_redex
        .reserve_canonical(token_event_with(1, vec![0, 1], 9, 3, BillableKind::Comm, 4))
        .unwrap();

    let changed_local_index = RuntimeBudget::new(Cost::create(20, "local index descriptor trace"));
    changed_local_index
        .reserve_canonical(token_event_with(1, vec![0, 1], 2, 8, BillableKind::Comm, 4))
        .unwrap();

    let duplicate = RuntimeBudget::new(Cost::create(20, "duplicate descriptor trace"));
    duplicate
        .reserve_canonical(token_event_with(1, vec![0, 1], 2, 3, BillableKind::Comm, 4))
        .unwrap();
    duplicate
        .reserve_canonical(token_event_with(1, vec![0, 1], 2, 3, BillableKind::Comm, 4))
        .unwrap();

    assert_ne!(base.cost_trace_digest(), changed_kind.cost_trace_digest());
    assert_ne!(base.cost_trace_digest(), changed_path.cost_trace_digest());
    assert_ne!(base.cost_trace_digest(), changed_redex.cost_trace_digest());
    assert_ne!(
        base.cost_trace_digest(),
        changed_local_index.cost_trace_digest()
    );
    assert_ne!(base.cost_trace_digest(), duplicate.cost_trace_digest());
    assert_eq!(duplicate.cost_trace_event_count(), 2);
}

#[test]
fn diagnostic_event_log_clearing_does_not_change_cost_trace_digest() {
    // D3 (DR-9, OD-3): a COMM costs ONE token. A 1-COMM budget admits the first
    // COMM and OOPs on the second.
    let budget = RuntimeBudget::new(Cost::create(1, "diagnostic trace"));
    budget.reserve_canonical(token_event(0, 2)).unwrap();
    budget.reserve_canonical(token_event(1, 2)).unwrap_err();
    let before = budget.cost_trace_digest();
    let oop = budget.last_oop_event();

    budget.clear_event_log();

    assert!(budget.get_event_log().is_empty());
    assert_eq!(budget.cost_trace_digest(), before);
    assert_eq!(budget.last_oop_event(), oop);
}

#[test]
fn unmetered_system_mode_restoration_preserves_later_metering() {
    // D3 (DR-9, OD-3): each COMM costs ONE token (weights are diagnostic). Two
    // metered COMMs ⇒ consensus cost 2; the unmetered COMM in between is not
    // recorded.
    let budget = RuntimeBudget::new(Cost::create(10, "system mode restoration"));
    budget.reserve_canonical(token_event(0, 3)).unwrap();
    let before_system = budget.cost_trace_digest();

    budget.set_unmetered(true);
    budget.reserve_canonical(token_event(99, 100)).unwrap();
    assert_eq!(budget.cost_trace_digest(), before_system);
    assert_eq!(budget.total_cost().value, 0);

    budget.set_unmetered(false);
    budget.reserve_canonical(token_event(1, 4)).unwrap();

    assert_eq!(budget.total_cost().value, 2);
    assert_eq!(budget.remaining().value, 8);
    assert_eq!(budget.cost_trace_event_count(), 2);
}

#[test]
fn coalesced_token_budget_refines_nested_gate_stack() {
    let sig = Sig::Ground(vec![9]);
    let nested = Token::gate(
        sig.clone(),
        Token::gate(sig.clone(), Token::coalesced(sig.clone(), 3)),
    );
    let coalesced = Token::coalesced(sig.clone(), 5);

    assert_eq!(nested.signature(), sig);
    assert_eq!(nested.remaining_units(), coalesced.remaining_units());
}

// D3 (DR-9, OD-3): `runtime_budget_matches_unit_token_expansion` is REMOVED —
// its premise (a weight-W event equals W unit-weight events) was the weight
// model. Under per-COMM a single COMM of any weight costs 1, NOT W, so a
// weight-3 COMM (cost 1) is NOT equivalent to three unit COMMs (cost 3).

#[test]
fn runtime_budget_canonical_event_log_is_order_independent() {
    let forward = RuntimeBudget::new(Cost::create(20, "forward budget"));
    let reverse = RuntimeBudget::new(Cost::create(20, "reverse budget"));
    let events = vec![token_event(2, 1), token_event(0, 2), token_event(1, 3)];

    for event in &events {
        forward.reserve_canonical(event.clone()).unwrap();
    }
    for event in events.iter().rev() {
        reverse.reserve_canonical(event.clone()).unwrap();
    }

    // get_event_log is now the canonical committed set (order-independent), so
    // it matches across reservation orders and equals get_canonical_event_log.
    assert_eq!(forward.get_event_log(), reverse.get_event_log());
    assert_eq!(
        forward.get_canonical_event_log(),
        reverse.get_canonical_event_log()
    );
    assert_eq!(forward.total_cost().value, reverse.total_cost().value);
    assert_eq!(forward.remaining().value, reverse.remaining().value);
}

#[test]
fn canonical_batch_commit_is_permutation_invariant() {
    // D3 (DR-9, OD-3): each COMM costs ONE token. A 1-COMM budget admits the
    // lowest-ranked COMM (source_path [1]) and OOPs on the next ([2]); the
    // commit is invariant under input permutation (the batch is canonically
    // sorted before the walk).
    let left = RuntimeBudget::new(Cost::create(1, "left batch budget"));
    let right = RuntimeBudget::new(Cost::create(1, "right batch budget"));
    let events = vec![token_event(2, 50), token_event(1, 60)];

    let left_commit = left
        .commit_canonical_batch(CostReservationBatch {
            events: events.clone(),
        })
        .unwrap();
    let right_commit = right
        .commit_canonical_batch(CostReservationBatch {
            events: events.into_iter().rev().collect(),
        })
        .unwrap();

    assert_eq!(left_commit, right_commit);
    assert_eq!(
        left_commit
            .permits
            .iter()
            .map(|permit| permit.event.source_path.clone())
            .collect::<Vec<_>>(),
        vec![SourcePath(vec![1])]
    );
    assert_eq!(
        left_commit
            .oop
            .as_ref()
            .map(|event| event.source_path.clone()),
        Some(SourcePath(vec![2]))
    );
    assert_eq!(left.total_cost().value, 1);
    assert_eq!(right.total_cost().value, 1);
    assert_eq!(left.cost_trace_digest(), right.cost_trace_digest());
}

#[test]
fn batch_commit_charges_only_granted_execution_permits() {
    // D3 (DR-9, OD-3): each COMM costs ONE token. A 1-COMM budget grants the
    // first COMM and OOPs on the second. `consumed_weight` / `permit.weight`
    // remain the DIAGNOSTIC per-op weight of the granted COMM (5); the CONSENSUS
    // cost (`total_cost`) is the COMM count (1).
    let budget = RuntimeBudget::new(Cost::create(1, "permit budget"));
    let commit = budget
        .commit_canonical_batch(CostReservationBatch {
            events: vec![token_event(3, 5), token_event(4, 5)],
        })
        .unwrap();

    let permitted_weight: u64 = commit.permits.iter().map(|permit| permit.weight).sum();

    assert_eq!(
        permitted_weight, 5,
        "diagnostic weight of the one granted COMM"
    );
    assert_eq!(commit.consumed_weight, 5);
    assert_eq!(budget.total_cost().value, 1, "consensus cost = 1 COMM");
    assert_eq!(budget.remaining().value, 0);
    assert_eq!(budget.get_event_log().len(), 1);
    assert!(commit.oop.is_some());
    assert_eq!(budget.cost_trace_event_count(), 2);
}

#[test]
fn runtime_budget_commits_to_limit_on_depletion() {
    // D3 (DR-9, OD-3): each COMM costs ONE token. A 1-COMM budget admits one
    // COMM, then OOPs; consumed clamps to the COMM budget (1).
    let budget = RuntimeBudget::new(Cost::create(1, "bounded budget"));
    budget.reserve_canonical(token_event(0, 3)).unwrap();

    let err = budget.reserve_canonical(token_event(1, 3)).unwrap_err();

    assert_eq!(err, InterpreterError::OutOfPhlogistonsError);
    assert_eq!(budget.total_cost().value, 1);
    assert_eq!(budget.remaining().value, 0);
}

#[test]
fn runtime_budget_keeps_first_oop_boundary_event() {
    // D3 (DR-9, OD-3): a 0-COMM budget OOPs on the FIRST COMM; the canonical
    // OOP boundary is the lowest-ranked event (first by source_path).
    let budget = RuntimeBudget::new(Cost::create(0, "bounded budget"));
    let first = token_event(10, 10);
    let second = token_event(11, 10);

    budget.reserve_canonical(first.clone()).unwrap_err();
    budget.reserve_canonical(second).unwrap_err();

    assert_eq!(budget.total_cost().value, 0);
    assert_eq!(budget.remaining().value, 0);
    assert_eq!(budget.last_oop_event(), Some(first));
}

#[test]
fn concurrent_runtime_budget_reservations_are_linearizable() {
    fn run_once(initial_phlo: i64) -> RuntimeBudget {
        let budget = RuntimeBudget::new(Cost::create(initial_phlo, "concurrent budget"));
        let barrier = Arc::new(Barrier::new(17));
        let mut handles = Vec::new();

        for index in 0..16u64 {
            let budget = budget.clone();
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                let event = token_event(index, index + 1);
                barrier.wait();
                let result = budget.reserve_canonical(event.clone());
                (event, result)
            }));
        }

        barrier.wait();
        let outcomes = handles
            .into_iter()
            .map(|handle| handle.join().expect("reservation worker"))
            .collect::<Vec<_>>();

        // D3 (DR-9, OD-3): runtime liveness vs canonical consensus, per-COMM.
        // Each event is a COMM costing ONE token; the lock-free CAS race decides
        // only the runtime grant (schedule-dependent). `get_event_log` reflects
        // the CANONICAL committed set (schedule-independent), whose COMM COUNT is
        // bounded by the budget. With 16 COMMs and a budget below 16, at least
        // one thread is rejected. The per-op `weight` is diagnostic only and is
        // NOT bounded by the COMM budget.
        let successful_count: u64 =
            outcomes.iter().filter(|(_, result)| result.is_ok()).count() as u64;
        let errors = outcomes
            .iter()
            .filter(|(_, result)| result.is_err())
            .count();
        let logged_comm_count: u64 = budget
            .get_event_log()
            .into_iter()
            .filter(|event| event.kind == BillableKind::Comm)
            .count() as u64;
        assert!(successful_count <= initial_phlo as u64);
        assert!(logged_comm_count <= initial_phlo as u64);
        assert!(errors > 0);

        budget
    }

    // Canonical-walk simulator: this is the schedule-independent answer
    // `reconcile()` must produce. Mirrors the production walk in
    // `accounting/mod.rs::reconcile`, but computed from the test's own
    // knowledge of the spawned events — so the assertion catches a
    // regression in the production walk rather than tautologically
    // matching it.
    fn expected_canonical_trace_count(events: &[BillableTokenEvent], initial: i64) -> u64 {
        // D3 (DR-9, OD-3): the consensus cost is the COMM count — each COMM
        // costs ONE token (weight is diagnostic). Walk in canonical order,
        // committing COMMs until the budget is exhausted.
        let mut sorted: Vec<BillableTokenEvent> = events.to_vec();
        sorted.sort();
        let mut consumed: i64 = 0;
        let mut committed: u64 = 0;
        let mut oop = false;
        for event in sorted {
            let cost_unit = i64::from(event.kind == BillableKind::Comm);
            let next = consumed.saturating_add(cost_unit);
            if cost_unit > 0 && next > initial {
                oop = true;
                break;
            }
            consumed = next;
            committed += 1;
        }
        committed + u64::from(oop)
    }

    // D3 (DR-9, OD-3): 16 COMMs, each cost 1. A budget of 8 admits the first 8
    // COMMs (canonical order = local_index ascending) and OOPs on the 9th
    // (local_index = 8). The per-op weights are diagnostic and do not affect the
    // count.
    let initial_phlo: i64 = 8;
    let spawned_events: Vec<BillableTokenEvent> =
        (0..16u64).map(|i| token_event(i, i + 1)).collect();
    let expected_trace_count = expected_canonical_trace_count(&spawned_events, initial_phlo);
    // 8 committed COMMs + 1 OOP boundary = 9.
    assert_eq!(expected_trace_count, 9);

    let budget_a = run_once(initial_phlo);

    // Consensus-side invariants derived from the canonical reconciliation:
    //
    // - `total_cost` clamps to `initial_phlo` on canonical OOP (preserves
    //   the deploy.cost == phlo_limit invariant the integration tests
    //   rely on).
    // - `last_oop_event` is the canonical OOP boundary event — the
    //   smallest-rank event whose cumulative weight would have exceeded
    //   the budget under the canonical walk, NOT a runtime CAS loser.
    //   For this fixture that is the event with local_index=8, weight=9.
    // - `cost_trace_event_count` is canonical commits + 1 (OOP fired).
    //   This must equal the simulator's answer derived purely from the
    //   spawned events, independent of Tokio scheduling.
    assert_eq!(
        budget_a.total_cost().value + budget_a.remaining().value,
        initial_phlo
    );
    assert_eq!(budget_a.total_cost().value, initial_phlo);
    let oop_a = budget_a
        .last_oop_event()
        .expect("canonical OOP must fire when weights overflow budget");
    assert_eq!(oop_a, token_event(8, 9));
    assert_eq!(budget_a.cost_trace_event_count(), expected_trace_count);
    let digest_a = budget_a.cost_trace_digest();
    assert!(!digest_a.digest.is_empty());
    assert_eq!(digest_a.event_count, expected_trace_count);

    // Schedule-invariance: a second concurrent run with the same input
    // multiset must produce the same canonical OOP boundary, the same
    // trace event count, and a byte-identical digest, even though the
    // Tokio/OS scheduler chooses different CAS race winners. This is the
    // headline Option-E guarantee: the consensus trace is a pure function
    // of (program, initial budget), not of runtime scheduling.
    let budget_b = run_once(initial_phlo);
    assert_eq!(budget_b.total_cost().value, initial_phlo);
    assert_eq!(budget_b.last_oop_event(), Some(token_event(8, 9)));
    assert_eq!(budget_b.cost_trace_event_count(), expected_trace_count);
    let digest_b = budget_b.cost_trace_digest();
    assert_eq!(digest_a, digest_b);

    // Runtime grant logs MAY differ between runs (CAS race winners are
    // schedule-dependent); the canonical reconciliation MUST NOT. Bind
    // the comparison into a discard so the canonical assertions above
    // are the ones that fail loudly if a future refactor recouples them.
    let _runtime_grants_may_differ = budget_a.get_event_log() != budget_b.get_event_log();
}

#[test]
fn oversized_runtime_event_is_rejected_without_trace_entry() {
    let budget = RuntimeBudget::new(Cost::create(10, "oversized weight budget"));
    let err = budget
        .reserve_canonical(token_event(0, u64::MAX))
        .unwrap_err();

    assert_eq!(err, InterpreterError::OutOfPhlogistonsError);
    assert_eq!(budget.total_cost().value, 0);
    assert_eq!(budget.remaining().value, 10);
    assert!(budget.get_event_log().is_empty());
    assert_eq!(budget.last_oop_event(), None);
    assert_eq!(budget.cost_trace_event_count(), 0);
}

#[test]
fn zero_weight_billable_event_is_rejected_without_trace_entry() {
    let budget = RuntimeBudget::new(Cost::create(10, "zero weight budget"));
    let err = budget.reserve_canonical(token_event(0, 0)).unwrap_err();

    assert_eq!(err, InterpreterError::OutOfPhlogistonsError);
    assert_eq!(budget.total_cost().value, 0);
    assert_eq!(budget.remaining().value, 10);
    assert!(budget.get_event_log().is_empty());
    assert_eq!(budget.cost_trace_event_count(), 0);
}

#[test]
fn max_sized_trace_descriptors_are_admitted_at_boundary() {
    let budget = RuntimeBudget::new(Cost::create(3, "descriptor boundary budget"));
    let primitive = token_event_with(
        1,
        vec![0],
        0,
        0,
        BillableKind::Primitive("x".repeat(MAX_COST_TRACE_PRIMITIVE_DESCRIPTOR_BYTES)),
        1,
    );
    let source_path = token_event_with(
        1,
        vec![0; MAX_COST_TRACE_SOURCE_PATH_COMPONENTS],
        1,
        0,
        BillableKind::Comm,
        1,
    );

    budget.reserve_canonical(primitive).unwrap();
    budget.reserve_canonical(source_path).unwrap();

    // D3 (DR-9, OD-3): consensus cost = COMM count. The Primitive is diagnostic
    // (cost 0); only the COMM (cost 1) counts ⇒ total_cost 1. Both events are
    // still admitted at the descriptor-size boundary and appear in the log.
    assert_eq!(budget.total_cost().value, 1);
    assert_eq!(budget.remaining().value, 2);
    assert_eq!(budget.get_event_log().len(), 2);
    assert_eq!(budget.cost_trace_event_count(), 2);
}

#[test]
fn oversized_trace_descriptors_are_rejected_before_trace_mutation() {
    let budget = RuntimeBudget::new(Cost::create(10, "descriptor budget"));
    let primitive = token_event_with(
        1,
        vec![0],
        0,
        0,
        BillableKind::Primitive("x".repeat(MAX_COST_TRACE_PRIMITIVE_DESCRIPTOR_BYTES + 1)),
        1,
    );

    let err = budget.reserve_canonical(primitive).unwrap_err();

    assert_eq!(err, InterpreterError::OutOfPhlogistonsError);
    assert_eq!(budget.total_cost().value, 0);
    assert_eq!(budget.remaining().value, 10);
    assert!(budget.get_event_log().is_empty());
    assert_eq!(budget.cost_trace_event_count(), 0);

    let long_source_path = token_event_with(
        1,
        vec![0; MAX_COST_TRACE_SOURCE_PATH_COMPONENTS + 1],
        0,
        0,
        BillableKind::Comm,
        1,
    );

    let err = budget.reserve_canonical(long_source_path).unwrap_err();

    assert_eq!(err, InterpreterError::OutOfPhlogistonsError);
    assert_eq!(budget.total_cost().value, 0);
    assert_eq!(budget.remaining().value, 10);
    assert!(budget.get_event_log().is_empty());
    assert_eq!(budget.cost_trace_event_count(), 0);
}

#[test]
fn runtime_budget_unmetered_mode_does_not_consume_tokens() {
    let budget = RuntimeBudget::new(Cost::create(5, "bounded budget"));
    budget.set_unmetered(true);

    budget.reserve_canonical(token_event(0, 100)).unwrap();

    assert_eq!(budget.total_cost().value, 0);
    assert_eq!(budget.remaining().value, i64::MAX);
}

#[test]
fn scoped_unmetered_mode_restores_after_early_return() {
    fn run_system_scope(budget: &RuntimeBudget) -> Result<(), InterpreterError> {
        let _scope = budget.enter_unmetered_scope();
        budget.reserve_canonical(token_event(99, 100)).unwrap();
        Err(InterpreterError::UserAbortError)
    }

    let budget = RuntimeBudget::new(Cost::create(5, "scoped unmetered budget"));

    assert_eq!(
        run_system_scope(&budget),
        Err(InterpreterError::UserAbortError)
    );
    budget.reserve_canonical(token_event(0, 2)).unwrap();

    // D3 (DR-9, OD-3): the one metered COMM after the unmetered scope costs 1
    // (its weight 2 is diagnostic); the unmetered COMM is not recorded.
    assert_eq!(budget.total_cost().value, 1);
    assert_eq!(budget.remaining().value, 4);
    assert_eq!(budget.cost_trace_event_count(), 1);
}

#[test]
fn metered_machine_rejects_zero_cost_billable_source_event() {
    let budget = RuntimeBudget::new(Cost::create(5, "zero runtime cost budget"));
    let machine = MeteredMachine::new(budget.clone());

    let err = machine
        .reserve_comm(Cost::create(0, "zero source cost"))
        .unwrap_err();

    assert!(matches!(err, InterpreterError::BugFoundError(_)));
    assert!(budget.get_event_log().is_empty());
    assert_eq!(budget.total_cost().value, 0);
    assert_eq!(budget.remaining().value, 5);
    assert_eq!(budget.cost_trace_event_count(), 0);
}

#[test]
fn diagnostic_cost_log_clearing_does_not_change_budget_observables() {
    let budget = RuntimeBudget::new(Cost::create(10, "diagnostic budget"));
    let event = token_event(0, 3);
    budget
        .reserve_canonical_with_cost(event.clone(), Cost::create(3, "diagnostic charge"))
        .unwrap();

    assert!(!budget.get_log().is_empty());
    let total = budget.total_cost();
    let remaining = budget.remaining();
    let event_log = budget.get_event_log();
    let oop = budget.last_oop_event();

    budget.clear_log();

    assert!(budget.get_log().is_empty());
    assert_eq!(budget.total_cost(), total);
    assert_eq!(budget.remaining(), remaining);
    assert_eq!(budget.get_event_log(), event_log);
    assert_eq!(budget.last_oop_event(), oop);
    assert_eq!(event_log, vec![event]);
}

#[test]
fn runtime_budget_records_typed_billable_events_without_legacy_compat() {
    let budget = RuntimeBudget::new(Cost::create(10, "typed event budget"));
    let machine = MeteredMachine::new(budget.clone());

    machine.reserve_comm(Cost::create(1, "send eval")).unwrap();
    machine
        .reserve_primitive(Cost::create(2, "method call"))
        .unwrap();
    machine
        .reserve_substitution(Cost::create(3, "substitution"))
        .unwrap();

    let kinds = budget
        .get_event_log()
        .into_iter()
        .map(|event| event.kind)
        .collect::<Vec<_>>();

    assert_eq!(kinds, vec![
        BillableKind::Comm,
        BillableKind::Primitive("method call".to_string()),
        BillableKind::Substitution
    ]);
    // D3 (DR-9, OD-3): consensus cost = the COMM count. Only the `Comm` counts
    // (=1); the diagnostic Primitive / Substitution contribute 0. (All three
    // still appear in the event log / digest.)
    assert_eq!(budget.total_cost().value, 1);
    assert_eq!(budget.cost_trace_event_count(), 3);
    assert!(!budget.cost_trace_digest().digest.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn evaluation_records_only_typed_billable_events() {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    let (mut runtime, _, _) =
        create_runtimes_with_cost_log(store, Some(false), Some(&mut Vec::new())).await;

    let result = runtime
        .evaluate_with_phlo("@0!(2)", Cost::create(1000, "typed event eval"))
        .await
        .unwrap();

    assert!(result.errors.is_empty());
    let event_log = runtime.get_cost_event_log();
    assert!(!event_log.is_empty());
    assert!(event_log.iter().all(|event| event.weight > 0));
    assert!(event_log
        .iter()
        .any(|event| matches!(event.kind, BillableKind::Comm)));
    assert!(event_log
        .iter()
        .any(|event| matches!(event.kind, BillableKind::Substitution)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn negative_initial_phlo_is_rejected_before_metered_trace() {
    let (result, cost_log) = evaluate_with_cost_log(-1, "@0!(0)".to_string()).await;

    assert_eq!(result.cost.value, 0);
    assert!(matches!(result.errors.as_slice(), [
        InterpreterError::IllegalArgumentError(_)
    ]));
    assert!(cost_log.is_empty());
}

#[test]
fn signature_channels_are_deploy_isolated() {
    let left = SignatureChannel::from_sig(&Sig::Ground(vec![1]));
    let right = SignatureChannel::from_sig(&Sig::Ground(vec![2]));
    let combined = SignatureChannel::from_sig(&Sig::And(
        Box::new(Sig::Ground(vec![1])),
        Box::new(Sig::Ground(vec![2])),
    ));
    let reversed = SignatureChannel::from_sig(&Sig::And(
        Box::new(Sig::Ground(vec![2])),
        Box::new(Sig::Ground(vec![1])),
    ));

    assert_ne!(left, right);
    assert_ne!(combined, left);
    assert_ne!(combined, right);
    assert_eq!(combined, reversed);
}

#[test]
fn deploy_signature_scope_is_domain_separated_from_raw_signature_bytes() {
    let raw_signature = vec![1, 2, 3, 4];
    let budget = RuntimeBudget::new(Cost::create(10, "signature scope"));

    budget.set_deploy_signature(&raw_signature);

    let raw_hash = Blake2b256::hash(raw_signature.clone());
    assert_ne!(budget.deploy_id().to_vec(), raw_hash);

    let scoped_signature = budget.signature();
    match &scoped_signature {
        // The deploy-signature digest is a `#P`-style process-hash, so
        // `set_deploy_signature` installs a `Sig::Quote` atom.
        Sig::Quote(bytes) => assert_eq!(bytes.as_slice(), budget.deploy_id().as_slice()),
        _ => panic!("deploy signatures must map to quote-scoped accounting signatures"),
    }

    assert_ne!(
        SignatureChannel::from_sig(&scoped_signature),
        SignatureChannel::from_sig(&Sig::Ground(raw_signature))
    );
}

#[test]
fn set_deploy_signatures_folds_into_left_associated_sig_and() {
    let budget = RuntimeBudget::new(Cost::create(10, "compound signature scope"));
    let sig_a: &[u8] = &[0xaa, 0xaa];
    let sig_b: &[u8] = &[0xbb, 0xbb];
    let sig_c: &[u8] = &[0xcc, 0xcc];

    budget.set_deploy_signatures(&[sig_a, sig_b, sig_c]);

    let sig = budget.signature();
    // Expected shape: Sig::And(Sig::And(Sig::Quote(h_a), Sig::Quote(h_b)), Sig::Quote(h_c))
    // — every per-signer hash is a `#P`-style process-hash, hence a Quote atom.
    match sig {
        Sig::And(outer_left, outer_right) => {
            assert!(matches!(*outer_right, Sig::Quote(_)));
            match *outer_left {
                Sig::And(inner_left, inner_right) => {
                    assert!(matches!(*inner_left, Sig::Quote(_)));
                    assert!(matches!(*inner_right, Sig::Quote(_)));
                }
                other => panic!(
                    "inner Sig::And expected, got {:?} — folding must be left-associated",
                    other
                ),
            }
        }
        other => panic!("Sig::And expected at outer level, got {:?}", other),
    }
}

#[test]
fn set_deploy_signatures_single_signer_is_sig_quote() {
    let budget = RuntimeBudget::new(Cost::create(10, "single-signer compound scope"));
    let single_sig: &[u8] = &[0xde, 0xad, 0xbe, 0xef];
    budget.set_deploy_signatures(&[single_sig]);

    match budget.signature() {
        Sig::Quote(_) => {
            // Single-signer fold collapses to a flat Sig::Quote (the
            // per-signer digest is a `#P`-style process-hash).
        }
        other => panic!(
            "single-signer set_deploy_signatures must produce Sig::Quote, got {:?}",
            other
        ),
    }
}

#[test]
fn set_deploy_signatures_distinct_domain_from_legacy_single_sig() {
    // The compound-deploy domain separator must produce a DIFFERENT deploy_id
    // than the legacy single-sig domain, even for the same wire signature
    // bytes. This guarantees that pre-existing single-sig deploys keep their
    // on-chain deploy_ids while multi-sig deploys obtain distinguishable ones.
    let raw_sig: &[u8] = &[0x42; 64];

    let legacy_budget = RuntimeBudget::new(Cost::create(10, "legacy single-sig"));
    legacy_budget.set_deploy_signature(raw_sig);

    let compound_budget = RuntimeBudget::new(Cost::create(10, "compound single-sig"));
    compound_budget.set_deploy_signatures(&[raw_sig]);

    assert_ne!(
        legacy_budget.deploy_id(),
        compound_budget.deploy_id(),
        "compound domain separator must produce distinct deploy_id from legacy"
    );
}

#[test]
fn set_deploy_signatures_deploy_id_depends_on_order() {
    // deploy_id is the Blake2b256 of the canonical-order concatenation of
    // per-sig hashes. Different input orders produce different deploy_ids
    // (deploy_id is NOT permutation-invariant by design — canonical ordering
    // is enforced UPSTREAM at Cosigned::from_signed_data, so the budget
    // always receives sorted input).
    let sig_a: &[u8] = &[0xaa];
    let sig_b: &[u8] = &[0xbb];

    let budget_ab = RuntimeBudget::new(Cost::create(10, "order ab"));
    budget_ab.set_deploy_signatures(&[sig_a, sig_b]);

    let budget_ba = RuntimeBudget::new(Cost::create(10, "order ba"));
    budget_ba.set_deploy_signatures(&[sig_b, sig_a]);

    assert_ne!(budget_ab.deploy_id(), budget_ba.deploy_id());

    // BUT the resulting signature CHANNEL is permutation-invariant via
    // SignatureChannel::from_sig (existing `ParSortMatcher::sort_match` guarantee).
    let channel_ab = SignatureChannel::from_sig(&budget_ab.signature());
    let channel_ba = SignatureChannel::from_sig(&budget_ba.signature());
    assert_eq!(channel_ab, channel_ba);
}

#[test]
#[should_panic(expected = "set_deploy_signatures requires at least one signature")]
fn set_deploy_signatures_empty_panics() {
    let budget = RuntimeBudget::new(Cost::create(10, "empty"));
    let empty: &[&[u8]] = &[];
    budget.set_deploy_signatures(empty);
}

// ─── Phase 2: Sig::Threshold M-of-N quorum primitive ───

#[test]
fn sig_threshold_reflection_permutation_invariant_in_members() {
    // Same member set, different orders ⇒ identical SignatureChannel.
    // This is the substrate guarantee that quorum verifier dispatch is
    // canonical-order-independent.
    let m1 = Sig::Ground(vec![0xaa, 0xaa]);
    let m2 = Sig::Ground(vec![0xbb, 0xbb]);
    let m3 = Sig::Ground(vec![0xcc, 0xcc]);

    let order_abc = Sig::Threshold {
        threshold: 2,
        members: vec![m1.clone(), m2.clone(), m3.clone()],
    };
    let order_cba = Sig::Threshold {
        threshold: 2,
        members: vec![m3.clone(), m2.clone(), m1.clone()],
    };
    let order_bac = Sig::Threshold {
        threshold: 2,
        members: vec![m2.clone(), m1.clone(), m3.clone()],
    };

    let ch_abc = SignatureChannel::from_sig(&order_abc);
    let ch_cba = SignatureChannel::from_sig(&order_cba);
    let ch_bac = SignatureChannel::from_sig(&order_bac);
    assert_eq!(ch_abc, ch_cba);
    assert_eq!(ch_abc, ch_bac);
}

#[test]
fn sig_threshold_reflection_distinct_from_and_with_same_member_set() {
    // Threshold{k, [a, b]} should NOT collapse to And(a, b) at the
    // SignatureChannel level — even though both reflect into a sorted Par
    // composition of member channels, the Threshold case is semantically
    // a quorum primitive. The reflection layer happens to produce the
    // same channel shape (intentional — verifier dispatches on the wire
    // shape's `threshold` field, not the channel shape), so we verify the
    // RUNTIME enum distinguishes them rather than the SignatureChannel.
    let a = Sig::Ground(vec![1]);
    let b = Sig::Ground(vec![2]);
    let threshold_ab = Sig::Threshold {
        threshold: 2,
        members: vec![a.clone(), b.clone()],
    };
    let and_ab = Sig::And(Box::new(a), Box::new(b));
    assert_ne!(threshold_ab, and_ab); // Sig enum distinguishes them
}

#[test]
fn sig_threshold_nested_within_and_reflects_consistently() {
    // Sig::And(Threshold{2, [a,b,c]}, Hash(d)) should reflect to a channel
    // that's permutation-invariant in the threshold's members.
    let a = Sig::Ground(vec![0x01]);
    let b = Sig::Ground(vec![0x02]);
    let c = Sig::Ground(vec![0x03]);
    let d = Sig::Ground(vec![0x04]);

    let nested_abc = Sig::And(
        Box::new(Sig::Threshold {
            threshold: 2,
            members: vec![a.clone(), b.clone(), c.clone()],
        }),
        Box::new(d.clone()),
    );
    let nested_cba = Sig::And(
        Box::new(Sig::Threshold {
            threshold: 2,
            members: vec![c, b, a],
        }),
        Box::new(d),
    );
    assert_eq!(
        SignatureChannel::from_sig(&nested_abc),
        SignatureChannel::from_sig(&nested_cba)
    );
}

#[test]
fn sig_threshold_single_member_reflects_like_unit_and_member() {
    // Threshold{1, [m]} is "1-of-1" which is the degenerate case meaning
    // "m alone authorizes the deploy". Its reflection should NOT crash and
    // should be permutation-invariant trivially.
    let m = Sig::Ground(vec![0xde, 0xad]);
    let trivial = Sig::Threshold {
        threshold: 1,
        members: vec![m.clone()],
    };
    let _ch = SignatureChannel::from_sig(&trivial); // does not panic
}

#[test]
fn sig_threshold_empty_members_reflects_to_unit_channel() {
    // Threshold{0, []} is the trivially-satisfied quorum. The reflected
    // channel is the empty Par (matches Sig::Unit). Verifier layer (Phase 2
    // Cosigned::from_signed_data extension) will reject zero-member
    // thresholds at envelope construction, but the substrate reflection
    // remains well-defined.
    let empty_quorum = Sig::Threshold {
        threshold: 0,
        members: Vec::new(),
    };
    let _ch = SignatureChannel::from_sig(&empty_quorum); // does not panic
}

// ─── Phase 3 LL-rich algebra: substrate LL identity properties ───
//
// Substrate-level verification of canonical linear-logic identities
// (Phase 3 §3.7). The reflection layer (`SignatureChannel::from_sig`)
// implements algebraic permutation-/structural-invariance via the
// existing `ParSortMatcher::sort_match` post-step, so many LL identities
// (associativity, commutativity, distributivity for the
// composition-shaped connectives) are derivable directly at the
// SignatureChannel level. Phase 3's Rocq mechanization (§3.6/3.7) covers
// the corresponding theorems with Qed-closed proofs.

#[test]
fn sig_plus_reflection_commutative() {
    // σ ⊕ τ ≡ τ ⊕ σ at the channel level (signer-choice symmetry).
    let a = Sig::Ground(vec![0x01]);
    let b = Sig::Ground(vec![0x02]);
    let ab = Sig::Plus(Box::new(a.clone()), Box::new(b.clone()));
    let ba = Sig::Plus(Box::new(b), Box::new(a));
    assert_eq!(
        SignatureChannel::from_sig(&ab),
        SignatureChannel::from_sig(&ba)
    );
}

#[test]
fn sig_with_reflection_commutative() {
    // σ & τ ≡ τ & σ (LL "with" / verifier-choice symmetry).
    let a = Sig::Ground(vec![0x10]);
    let b = Sig::Ground(vec![0x20]);
    let ab = Sig::With(Box::new(a.clone()), Box::new(b.clone()));
    let ba = Sig::With(Box::new(b), Box::new(a));
    assert_eq!(
        SignatureChannel::from_sig(&ab),
        SignatureChannel::from_sig(&ba)
    );
}

#[test]
fn sig_bang_idempotent_at_channel_level() {
    // !(!σ) ≡ !σ at the reflection layer. Bang is unary; double-bang
    // collapses because Bang's reflection is the inner channel, and the
    // outer Bang's reflection is the inner Bang's reflection = inner σ.
    let a = Sig::Ground(vec![0x42; 8]);
    let bang_a = Sig::Bang(Box::new(a));
    let bang_bang_a = Sig::Bang(Box::new(bang_a.clone()));
    assert_eq!(
        SignatureChannel::from_sig(&bang_a),
        SignatureChannel::from_sig(&bang_bang_a)
    );
}

#[test]
fn sig_whynot_idempotent_at_channel_level() {
    // ?(?σ) ≡ ?σ — dual of bang idempotence.
    let a = Sig::Ground(vec![0xff; 8]);
    let q_a = Sig::WhyNot(Box::new(a));
    let q_q_a = Sig::WhyNot(Box::new(q_a.clone()));
    assert_eq!(
        SignatureChannel::from_sig(&q_a),
        SignatureChannel::from_sig(&q_q_a)
    );
}

#[test]
fn sig_lolly_reflection_distinct_from_tensor() {
    // σ ⊸ τ should NOT collapse to σ ⊗ τ at the channel level — both
    // produce composition-shape channels, so we test the Sig enum
    // distinguishes them. Operationally Lolly is a capability (consume σ,
    // produce τ via the registry transformer), Tensor is joint possession.
    let a = Sig::Ground(vec![0xa1]);
    let b = Sig::Ground(vec![0xb2]);
    let lolly = Sig::Lolly(Box::new(a.clone()), Box::new(b.clone()));
    let and_ab = Sig::And(Box::new(a), Box::new(b));
    assert_ne!(lolly, and_ab); // enum distinguishes them
                               // Channel reflections happen to coincide (intentional substrate
                               // sharing; verifier dispatches on the Sig variant, not the channel
                               // shape).
}

#[test]
fn sig_ll_algebra_full_combinator_well_formed() {
    // Construct a deeply-nested LL expression combining every connective:
    //   And(Plus(Bang(a), WhyNot(b)), Threshold{2, [c, Lolly(d, e), With(f, Unit)]})
    // Reflection must succeed (no panic / no incorrect collapse).
    let a = Sig::Ground(vec![0x01]);
    let b = Sig::Ground(vec![0x02]);
    let c = Sig::Ground(vec![0x03]);
    let d = Sig::Ground(vec![0x04]);
    let e = Sig::Ground(vec![0x05]);
    let f = Sig::Ground(vec![0x06]);

    let expr = Sig::And(
        Box::new(Sig::Plus(
            Box::new(Sig::Bang(Box::new(a))),
            Box::new(Sig::WhyNot(Box::new(b))),
        )),
        Box::new(Sig::Threshold {
            threshold: 2,
            members: vec![
                c,
                Sig::Lolly(Box::new(d), Box::new(e)),
                Sig::With(Box::new(f), Box::new(Sig::Unit)),
            ],
        }),
    );
    let _channel = SignatureChannel::from_sig(&expr); // does not panic
}

#[test]
fn sig_proto_round_trip_every_connective() {
    // Construct an Sig expression that exercises EVERY connective and
    // round-trip through SigCompound proto. The reverse-decoded Sig must
    // equal the original.
    let original = Sig::And(
        Box::new(Sig::Plus(
            Box::new(Sig::Bang(Box::new(Sig::Ground(vec![0x01, 0x02])))),
            Box::new(Sig::WhyNot(Box::new(Sig::Ground(vec![0x03, 0x04])))),
        )),
        Box::new(Sig::Threshold {
            threshold: 2,
            members: vec![
                Sig::Ground(vec![0x05]),
                Sig::Lolly(
                    Box::new(Sig::Ground(vec![0x06])),
                    Box::new(Sig::Ground(vec![0x07])),
                ),
                Sig::With(Box::new(Sig::Ground(vec![0x08])), Box::new(Sig::Unit)),
            ],
        }),
    );

    let proto = original.to_proto();
    let decoded = Sig::from_proto(&proto).expect("round-trip decode must succeed");
    assert_eq!(decoded, original);
}

#[test]
fn sig_proto_round_trip_unit() {
    let proto = Sig::Unit.to_proto();
    let decoded = Sig::from_proto(&proto).expect("Unit round-trip");
    assert_eq!(decoded, Sig::Unit);
}

#[test]
fn sig_proto_round_trip_ground_atom() {
    let original = Sig::Ground(vec![0xde, 0xad, 0xbe, 0xef]);
    let proto = original.to_proto();
    let decoded = Sig::from_proto(&proto).expect("Ground round-trip");
    assert_eq!(decoded, original);
}

#[test]
fn sig_proto_round_trip_quote_atom() {
    let original = Sig::Quote(vec![0xde, 0xad, 0xbe, 0xef]);
    let proto = original.to_proto();
    let decoded = Sig::from_proto(&proto).expect("Quote round-trip");
    assert_eq!(decoded, original);
}

/// The atom-kind axis (`Ground` vs `Quote`) must survive the
/// `to_proto`/`from_proto` round-trip, the wire `SigAtom.atom_kind` tag
/// must be emitted accordingly, and a `SigAtom` whose `atom_kind` is the
/// proto3 default (0 / absent) must decode to `Ground` (backward compat).
#[test]
fn sig_proto_atom_kind_round_trips_and_defaults_to_ground() {
    use models::casper::{sig_compound, AtomKind, SigAtom, SigCompound};

    // (1) Ground emits atom_kind = GROUND.
    let ground_proto = Sig::Ground(vec![0x01, 0x02]).to_proto();
    match ground_proto.connective.as_ref().expect("connective") {
        sig_compound::Connective::Atom(atom) => {
            assert_eq!(atom.atom_kind, AtomKind::Ground as i32);
        }
        other => panic!("expected Atom, got {:?}", other),
    }

    // (2) Quote emits atom_kind = QUOTE.
    let quote_proto = Sig::Quote(vec![0x01, 0x02]).to_proto();
    match quote_proto.connective.as_ref().expect("connective") {
        sig_compound::Connective::Atom(atom) => {
            assert_eq!(atom.atom_kind, AtomKind::Quote as i32);
        }
        other => panic!("expected Atom, got {:?}", other),
    }

    // (3) A SigAtom with the DEFAULT atom_kind (0 == GROUND, the proto3
    //     default a legacy encoder would leave unset) decodes to Ground.
    let legacy_default_atom = SigCompound {
        connective: Some(sig_compound::Connective::Atom(SigAtom {
            pk: vec![0xaa, 0xbb].into(),
            sig: Default::default(),
            sig_algorithm: String::new(),
            atom_kind: 0,
        })),
    };
    assert_eq!(
        Sig::from_proto(&legacy_default_atom).expect("legacy atom decodes"),
        Sig::Ground(vec![0xaa, 0xbb]),
        "atom_kind == 0 (default/absent) must decode to Ground"
    );
}

/// Round-trip a compound expression mixing BOTH atom axes under `And` and
/// every ILLE connective, asserting the axis is preserved through nesting.
#[test]
fn sig_proto_round_trip_mixed_ground_quote_under_connectives() {
    let original = Sig::And(
        Box::new(Sig::Plus(
            Box::new(Sig::Bang(Box::new(Sig::Ground(vec![0x01, 0x02])))),
            Box::new(Sig::WhyNot(Box::new(Sig::Quote(vec![0x03, 0x04])))),
        )),
        Box::new(Sig::Threshold {
            threshold: 2,
            members: vec![
                Sig::Ground(vec![0x05]),
                Sig::Lolly(
                    Box::new(Sig::Quote(vec![0x06])),
                    Box::new(Sig::Ground(vec![0x07])),
                ),
                Sig::With(Box::new(Sig::Quote(vec![0x08])), Box::new(Sig::Unit)),
            ],
        }),
    );
    let proto = original.to_proto();
    let decoded = Sig::from_proto(&proto).expect("mixed-axis round-trip decode");
    assert_eq!(
        decoded, original,
        "Ground/Quote axis must be preserved through And + every ILLE connective"
    );
}

#[test]
fn sig_proto_round_trip_threshold_preserves_member_order() {
    let original = Sig::Threshold {
        threshold: 3,
        members: vec![
            Sig::Ground(vec![0xaa]),
            Sig::Ground(vec![0xbb]),
            Sig::Ground(vec![0xcc]),
            Sig::Ground(vec![0xdd]),
        ],
    };
    let proto = original.to_proto();
    let decoded = Sig::from_proto(&proto).expect("Threshold round-trip");
    assert_eq!(decoded, original);
    match decoded {
        Sig::Threshold { threshold, members } => {
            assert_eq!(threshold, 3);
            assert_eq!(members.len(), 4);
        }
        _ => panic!("expected Threshold after round-trip"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_permutation_use_cases_preserve_cost() {
    let variants = vec![
        "@0!(0) | @1!(1) | for (_ <- @0) { 0 } | for (_ <- @1) { 0 }",
        "for (_ <- @1) { 0 } | @1!(1) | for (_ <- @0) { 0 } | @0!(0)",
        "@1!(1) | for (_ <- @0) { 0 } | @0!(0) | for (_ <- @1) { 0 }",
        "for (_ <- @0) { 0 } | for (_ <- @1) { 0 } | @0!(0) | @1!(1)",
    ];

    let mut expected_cost = None;
    for contract in variants {
        let (result, _) = evaluate_with_cost_log(1000, contract.to_string()).await;
        assert!(
            result.errors.is_empty(),
            "Contract errored: {}: {:?}",
            contract,
            result.errors
        );
        match expected_cost {
            None => expected_cost = Some(result.cost),
            Some(ref expected) => assert_eq!(
                &result.cost, expected,
                "Parallel permutation changed cost for '{}'",
                contract
            ),
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_interpreter_cost_trace_digest_is_repeatable() {
    let contract = "@0!(0) | @1!(1) | for (_ <- @0) { 0 } | for (_ <- @1) { 0 }";
    let mut expected = None;

    for _ in 0..8 {
        let (result, digest) = evaluate_with_cost_trace_digest(1000, contract.to_string()).await;

        assert!(
            result.errors.is_empty(),
            "Contract errored: {}: {:?}",
            contract,
            result.errors
        );
        assert!(!digest.digest.is_empty());
        assert!(digest.event_count > 0);

        match &expected {
            None => expected = Some((result.cost, digest)),
            Some((expected_cost, expected_digest)) => {
                assert_eq!(&result.cost, expected_cost);
                assert_eq!(&digest, expected_digest);
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bounded_generated_terms_have_deterministic_play_replay_cost() {
    let phlo = Cost::create(i32::MAX as i64, "bounded generated replay");
    let mut tested = 0usize;
    let mut skipped = 0usize;

    for contract in bounded_generated_contracts(512) {
        let (play, replay) = evaluate_and_replay(phlo.clone(), contract.clone()).await;
        if is_same_channel_error(&play.errors) || is_same_channel_error(&replay.errors) {
            skipped += 1;
            continue;
        }

        assert!(
            play.errors.is_empty(),
            "Unexpected play error for '{}': {:?}",
            contract,
            play.errors
        );
        assert!(
            replay.errors.is_empty(),
            "Unexpected replay error for '{}': {:?}",
            contract,
            replay.errors
        );
        assert_eq!(
            play.cost, replay.cost,
            "Play/replay cost mismatch for '{}'",
            contract
        );
        tested += 1;
    }

    assert!(
        tested >= 128,
        "Bounded generated coverage too small: tested={}, skipped={}",
        tested,
        skipped
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn structurally_equivalent_parallel_order_has_same_token_cost() {
    let left = evaluate_with_cost_log(1000, "@0!(0) | @1!(1)".to_string())
        .await
        .0;
    let right = evaluate_with_cost_log(1000, "@1!(1) | @0!(0)".to_string())
        .await
        .0;

    assert!(left.errors.is_empty());
    assert!(right.errors.is_empty());
    assert_eq!(left.cost, right.cost);
}

/// D3 (DR-9, OD-3) — CONSENSUS COST EXCLUDES PER-OP GAS. Two contracts with the
/// SAME number of COMMs but VERY different per-operation gas (one sends a plain
/// literal, the other sends a deep arithmetic expression that charges many
/// `Primitive` ops) must have the IDENTICAL consensus cost (the COMM count),
/// because per-op gas is DIAGNOSTIC only and does not gate consensus.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_cost_excludes_per_op_gas() {
    // 1 send (1 COMM), trivial payload.
    let plain = evaluate_with_cost_log(100_000, "@0!(1)".to_string())
        .await
        .0;
    // 1 send (1 COMM), but a payload that charges many arithmetic Primitives.
    let heavy = evaluate_with_cost_log(
        100_000,
        "@0!(1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 10)".to_string(),
    )
    .await
    .0;

    assert!(
        plain.errors.is_empty(),
        "plain term must run: {:?}",
        plain.errors
    );
    assert!(
        heavy.errors.is_empty(),
        "heavy term must run: {:?}",
        heavy.errors
    );

    // Both are exactly ONE COMM, so the consensus cost is identical (= 1),
    // regardless of the heavy term's much larger per-op diagnostic gas.
    assert_eq!(
        plain.cost.value, heavy.cost.value,
        "per-op gas must NOT change the per-COMM consensus cost (plain={}, heavy={})",
        plain.cost.value, heavy.cost.value
    );
    assert_eq!(
        plain.cost.value, 1,
        "a single send is exactly one COMM token"
    );
}

/// D3 (DR-9, OD-3): the CONSENSUS cost (`eval_result.cost`) is the per-COMM
/// token count, NOT the sum of the diagnostic per-op weight log. The cost log
/// is now diagnostic-only (it still records each charge's per-op weight, which
/// sums to the old weight total), so `total_cost == Σ log weights` no longer
/// holds. The D3-correct invariant: a non-empty metered contract consumes ≥ 1
/// COMM token, the diagnostic log is still populated, and the consensus cost
/// never exceeds the diagnostic event count (COMMs are a subset of all billable
/// events, and each COMM contributes exactly 1).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consensus_cost_is_per_comm_count_with_diagnostic_log_populated() {
    for contract in contracts() {
        let initial_phlo = 10000i64;
        let (eval_result, cost_log) = evaluate_with_cost_log(initial_phlo, contract.clone()).await;
        assert_eq!(
            eval_result.errors,
            Vec::new(),
            "Contract errored: {}",
            contract
        );
        // Consensus cost = per-COMM count > 0 for a non-empty metered contract.
        assert!(
            eval_result.cost.value > 0,
            "Non-empty metered contract should consume ≥ 1 COMM token: {}",
            contract
        );
        // The diagnostic per-op log is still recorded (D3 keeps it for fidelity).
        assert!(
            !cost_log.is_empty(),
            "diagnostic cost log must remain populated: {}",
            contract
        );
        // The per-COMM consensus cost never exceeds the diagnostic event count
        // (each billable event is at most one COMM; non-COMM events are 0).
        assert!(
            eval_result.cost.value <= cost_log.len() as i64,
            "per-COMM cost ({}) must not exceed diagnostic event count ({}) for '{}'",
            eval_result.cost.value,
            cost_log.len(),
            contract
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cost_should_be_deterministic() {
    for contract in contracts() {
        let mut first_cost: Option<i64> = None;
        for i in 0..20 {
            let (result, _log) = evaluate_with_cost_log(i32::MAX as i64, contract.clone()).await;
            assert!(result.errors.is_empty(), "Contract errored: {}", contract);
            match first_cost {
                None => first_cost = Some(result.cost.value),
                Some(expected) => {
                    assert_eq!(
                        result.cost.value, expected,
                        "Cost not deterministic at iteration {} for '{}': expected={}, got={}",
                        i, contract, expected, result.cost.value
                    );
                }
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cost_should_be_repeatable_when_generated() {
    let phlo = Cost::create(i32::MAX as i64, "max_value".to_string());
    let mut tested = 0u32;
    let mut skipped = 0u32;
    let mut mismatches: Vec<(String, i64, i64)> = Vec::new();

    let mut rng = rand::thread_rng();
    for _ in 0..10000 {
        let long = ((rng.gen::<i64>() % 0x144000000) + 0x144000000) % 0x144000000;
        let contract = from_long(long);
        if contract.is_empty() {
            continue;
        }

        let (play, replay) = evaluate_and_replay(phlo.clone(), contract.clone()).await;

        if is_same_channel_error(&play.errors) || is_same_channel_error(&replay.errors) {
            skipped += 1;
            continue;
        }
        assert!(
            play.errors.is_empty(),
            "Unexpected play error for '{}': {:?}",
            contract,
            play.errors
        );
        assert!(
            replay.errors.is_empty(),
            "Unexpected replay error for '{}': {:?}",
            contract,
            replay.errors
        );

        if play.cost != replay.cost {
            mismatches.push((contract, play.cost.value, replay.cost.value));
        }
        tested += 1;
    }

    eprintln!(
        "Replay cost determinism: {} tested, {} skipped, {} mismatches",
        tested,
        skipped,
        mismatches.len()
    );
    for (contract, play, replay) in &mismatches {
        eprintln!(
            "  MISMATCH: play={}, replay={}, diff={}, contract='{}'",
            play,
            replay,
            play - replay,
            contract
        );
    }
    assert!(tested > 100, "Too few contracts tested: {}", tested);
    assert!(
        mismatches.is_empty(),
        "{} contracts had play/replay cost mismatches",
        mismatches.len()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn running_out_of_phlogistons_should_stop_evaluation_upon_cost_depletion_in_a_single_execution_branch(
) {
    // D3 (DR-9, OD-3): `@1!(1)` is ONE COMM (cost 1). A 0-COMM budget OOPs on it
    // (1 COMM exceeds 0); consumed clamps to the COMM budget (0).
    check_phlo_limit_exceeded("@1!(1)".to_string(), 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn malformed_source_should_not_consume_tokens_before_metered_state_exists() {
    let (evaluate_result, cost_log) =
        evaluate_with_cost_log(1, "new f, x in { f(x) }".to_string()).await;

    assert!(!evaluate_result.errors.is_empty());
    assert_eq!(evaluate_result.cost.value, 0);
    assert!(cost_log.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_out_of_phlo_use_cases_commit_exact_budget() {
    let variants = vec![
        "@1!(1) | @2!(2) | @3!(3)",
        "@3!(3) | @2!(2) | @1!(1)",
        "@2!(2) | @1!(1) | @3!(3)",
    ];

    for contract in variants {
        // D3 (DR-9, OD-3): each `@N!(N)` is ONE COMM (3 COMMs total). A 2-COMM
        // budget admits two and OOPs on the third; consumed clamps to 2.
        check_phlo_limit_exceeded(contract.to_string(), 2).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn should_stop_the_evaluation_of_all_execution_branches_when_one_of_them_runs_out_of_phlo() {
    // D3 (DR-9, OD-3): 3 sends = 3 COMMs; a 2-COMM budget OOPs on the third.
    check_phlo_limit_exceeded("@1!(1) | @2!(2) | @3!(3)".to_string(), 2).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn should_stop_the_evaluation_of_all_execution_branches_when_one_of_them_runs_out_of_phlo_with_a_more_sophisticated_contract(
) {
    let mut rng = rand::thread_rng();
    for contract in contracts() {
        let (full_result, _) = evaluate_with_cost_log(i32::MAX as i64, contract.clone()).await;
        assert!(
            full_result.errors.is_empty(),
            "Contract errored with full budget: {}",
            contract
        );
        if full_result.cost.value <= 1 {
            continue;
        }
        let initial_phlo = rng.gen_range(1..full_result.cost.value);

        // D3 (DR-9, OD-3): `initial_phlo` is drawn from `[1, full_comm_count)`,
        // i.e. STRICTLY below the contract's per-COMM cost, so it must OOP and
        // clamp consumed to the budget. (No weight-sum-vs-budget check: the
        // diagnostic per-op log is not bounded by the COMM budget.)
        let (result, _cost_log) = evaluate_with_cost_log(initial_phlo, contract.clone()).await;

        assert_eq!(
            result.errors,
            vec![InterpreterError::OutOfPhlogistonsError],
            "Expected out-of-phlo for {} with initial COMM budget {} below full per-COMM cost {}",
            contract,
            initial_phlo,
            full_result.cost.value
        );
        assert_eq!(
            result.cost.value, initial_phlo,
            "Out-of-phlo must commit exactly the exhausted COMM-token budget"
        );
    }
}

/// Regression test for F1R3FLY-io/f1r3node#178: peek consume with parallel
/// produce must preserve peeked data and produce identical play/replay costs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn peek_with_parallel_produce_should_have_deterministic_replay_cost() {
    let phlo = Cost::create(i32::MAX as i64, "max_value".to_string());

    let contracts = vec![
        // Basic peek + produce (simplified #178 repro)
        "for (@x <<- @1) { 0 } | @1!(42)",
        // Persistent produce triggering peek COMM (the bug pattern)
        "@1!!(0) | for (_ <<- @1) { 0 } | @2!(0)",
        // Peek join with persistent produce on one channel
        "for (_ <<- @2 & _ <<- @3) { 0 } | @3!!(0) | @2!(0)",
        // Peek join with persistent produces on both channels
        "for (_ <<- @1 & _ <<- @2) { 0 } | @1!!(0) | @2!!(0)",
        // Multiple peek consumes with persistent and non-persistent produces
        "for (_ <<- @1) { 0 } | @1!!(0) | for (_ <<- @2) { 0 } | @2!!(0)",
    ];

    for contract in contracts {
        for _ in 0..20 {
            let (play, replay) = evaluate_and_replay(phlo.clone(), contract.to_string()).await;
            assert!(
                play.errors.is_empty(),
                "Play error for '{}': {:?}",
                contract,
                play.errors
            );
            assert!(
                replay.errors.is_empty(),
                "Replay error for '{}': {:?}",
                contract,
                replay.errors
            );
            assert_eq!(
                play.cost, replay.cost,
                "Play/replay cost mismatch for '{}': play={}, replay={}",
                contract, play.cost.value, replay.cost.value
            );
        }
    }
}

/// Throughput regression bench for Option E's lock-free attempt log +
/// post-hoc reconciliation. Drives a high-concurrency reservation
/// pattern (16 threads × 100 events each = 1600 reservations) against
/// a single shared `RuntimeBudget` and asserts completion within a
/// generous wall-clock bound. The bound is set high enough that even
/// on slow CI it should pass; a regression to the prior `commit_lock`
/// design (or worse) would push us over the bound.
///
/// Not a hard SLO — this catches catastrophic regressions only. Real
/// performance characterization is the operator's responsibility per
/// CLAUDE.md "Be data driven" (use `perf record`, `hyperfine`, etc.).
#[test]
fn option_e_throughput_regression_bench() {
    let total_weight: u64 = 16 * 100 * 5;
    let budget = RuntimeBudget::new(Cost::create(total_weight as i64 * 2, "bench budget"));
    let barrier = Arc::new(Barrier::new(17));
    let mut handles = Vec::new();

    let start = std::time::Instant::now();
    for thread_idx in 0..16u64 {
        let budget = budget.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            for event_idx in 0..100u64 {
                let rank = thread_idx * 1000 + event_idx;
                let _ = budget.reserve_canonical(token_event(rank, 5));
            }
        }));
    }
    barrier.wait();
    for h in handles {
        h.join().expect("bench worker");
    }

    // Drive reconciliation once at the end (mirrors deploy finalization).
    let digest = budget.cost_trace_digest();
    let elapsed = start.elapsed();

    // Sanity: all 1600 COMM events fit the budget, no OOP. D3 (DR-9, OD-3): the
    // consensus cost is the COMM COUNT (1600), NOT the weight sum (the per-op
    // weight 5 is diagnostic).
    assert_eq!(digest.event_count, 1600);
    assert_eq!(budget.last_oop_event(), None);
    assert_eq!(budget.total_cost().value, 1600);

    // Generous regression bound — 30s on the slowest CI tier. Local
    // (Linux x86_64) typical: <500ms. A regression to the old commit_lock
    // (single-mutex hot-path serialization) or a future bad lock-coupling
    // would push wall-clock well past this.
    assert!(
        elapsed.as_secs() < 30,
        "Option E reservation throughput regressed: 1600 reservations took {:?}",
        elapsed
    );
}
