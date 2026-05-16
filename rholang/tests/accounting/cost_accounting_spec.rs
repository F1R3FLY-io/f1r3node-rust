//See rholang/src/test/scala/coop/rchain/rholang/interpreter/accounting/CostAccountingSpec.scala from main branch

use std::collections::HashMap;
use std::option::Option;
use std::sync::Arc;

use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rand::Rng;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::{
    BillableKind, BillableTokenEvent, RedexId, RuntimeBudget, Sig, SignatureChannel, SignedProcess,
    SourcePath, Token,
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
        Par::default(),
        init_registry,
        additional_system_processes,
        ExternalServices::noop(),
    )
    .await;

    let replay_rho_runtime = create_replay_rho_runtime(
        replay,
        Par::default(),
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

async fn check_phlo_limit_exceeded(contract: String, initial_phlo: i64) -> bool {
    let (evaluate_result, cost_log) = evaluate_with_cost_log(initial_phlo, contract).await;

    assert_eq!(
        evaluate_result.errors,
        vec![InterpreterError::OutOfPhlogistonsError],
        "Expected list of OutOfPhlogistonsError"
    );

    assert_eq!(
        evaluate_result.cost.value, initial_phlo,
        "Out-of-phlo must commit exactly the exhausted token budget"
    );
    assert!(
        cost_log.iter().map(|cost| cost.value).sum::<i64>() <= initial_phlo,
        "Successful charge events may not exceed the token budget"
    );

    true
}

fn token_event(local_index: u64, weight: u64) -> BillableTokenEvent {
    BillableTokenEvent {
        deploy_id: [7; 32],
        source_path: SourcePath(vec![local_index as u32]),
        redex_id: RedexId(local_index),
        local_index,
        kind: BillableKind::SourceStep,
        weight,
    }
}

#[test]
fn runtime_budget_initializes_from_signed_token_annotation() {
    let sig = Sig::Hash(vec![1, 2, 3]);
    let annotated = SignedProcess::metered(Par::default(), sig.clone(), 7);
    let budget = RuntimeBudget::new(Cost::create(0, "empty budget"));

    budget.reset_from_signed_process(&annotated);

    assert_eq!(budget.signature(), sig);
    assert_eq!(budget.remaining().value, 7);
    assert_eq!(budget.total_cost().value, 0);
}

#[test]
fn coalesced_token_budget_refines_nested_gate_stack() {
    let sig = Sig::Hash(vec![9]);
    let nested = Token::gate(
        sig.clone(),
        Token::gate(sig.clone(), Token::coalesced(sig.clone(), 3)),
    );
    let coalesced = Token::coalesced(sig.clone(), 5);

    assert_eq!(nested.signature(), sig);
    assert_eq!(nested.remaining_units(), coalesced.remaining_units());
}

#[test]
fn runtime_budget_matches_unit_token_expansion() {
    let weighted = RuntimeBudget::new(Cost::create(10, "weighted budget"));
    weighted.reserve_canonical(token_event(0, 3)).unwrap();

    let expanded = RuntimeBudget::new(Cost::create(10, "expanded budget"));
    for index in 0..3 {
        expanded.reserve_canonical(token_event(index, 1)).unwrap();
    }

    assert_eq!(weighted.total_cost().value, expanded.total_cost().value);
    assert_eq!(weighted.remaining().value, expanded.remaining().value);
}

#[test]
fn runtime_budget_commits_to_limit_on_depletion() {
    let budget = RuntimeBudget::new(Cost::create(5, "bounded budget"));
    budget.reserve_canonical(token_event(0, 3)).unwrap();

    let err = budget.reserve_canonical(token_event(1, 3)).unwrap_err();

    assert_eq!(err, InterpreterError::OutOfPhlogistonsError);
    assert_eq!(budget.total_cost().value, 5);
    assert_eq!(budget.remaining().value, 0);
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
fn runtime_budget_records_typed_billable_events_without_legacy_compat() {
    let budget = RuntimeBudget::new(Cost::create(10, "typed event budget"));
    let machine = MeteredMachine::new(budget.clone());

    machine
        .reserve_source_step(Cost::create(1, "send eval"))
        .unwrap();
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
        BillableKind::SourceStep,
        BillableKind::Primitive("method call".to_string()),
        BillableKind::Substitution
    ]);
    assert_eq!(budget.total_cost().value, 6);
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
    assert!(event_log
        .iter()
        .any(|event| matches!(event.kind, BillableKind::SourceStep)));
    assert!(event_log
        .iter()
        .any(|event| matches!(event.kind, BillableKind::Substitution)));
}

#[test]
fn signature_channels_are_deploy_isolated() {
    let left = SignatureChannel::from_sig(&Sig::Hash(vec![1]));
    let right = SignatureChannel::from_sig(&Sig::Hash(vec![2]));
    let combined = SignatureChannel::from_sig(&Sig::And(
        Box::new(Sig::Hash(vec![1])),
        Box::new(Sig::Hash(vec![2])),
    ));
    let reversed = SignatureChannel::from_sig(&Sig::And(
        Box::new(Sig::Hash(vec![2])),
        Box::new(Sig::Hash(vec![1])),
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
        Sig::Hash(bytes) => assert_eq!(bytes.as_slice(), budget.deploy_id().as_slice()),
        _ => panic!("deploy signatures must map to hash-scoped accounting signatures"),
    }

    assert_ne!(
        SignatureChannel::from_sig(&scoped_signature),
        SignatureChannel::from_sig(&Sig::Hash(raw_signature))
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn total_cost_of_evaluation_should_be_equal_to_the_sum_of_all_costs_in_the_log() {
    for contract in contracts() {
        let initial_phlo = 10000i64;
        let (eval_result, cost_log) = evaluate_with_cost_log(initial_phlo, contract.clone()).await;
        assert_eq!(
            eval_result.errors,
            Vec::new(),
            "Contract errored: {}",
            contract
        );
        let logged_cost = cost_log.iter().map(|c| c.value).sum::<i64>();
        assert_eq!(
            eval_result.cost.value, logged_cost,
            "Cost mismatch for '{}': logged={}, got={}",
            contract, logged_cost, eval_result.cost.value
        );
        assert!(
            eval_result.cost.value > 0,
            "Non-empty metered contract should consume tokens: {}",
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

        // Skip contracts that hit the known "same-channel join" limitation.
        // These are invalid Rholang — not a replay issue.
        let is_same_channel_error = |errors: &[InterpreterError]| {
            errors.iter().any(|e| match e {
                InterpreterError::ReceiveOnSameChannelsError { .. } => true,
                InterpreterError::ParserError(msg)
                    if msg.contains("Receiving on the same channels") =>
                {
                    true
                }
                _ => false,
            })
        };
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
    check_phlo_limit_exceeded("@1!(1)".to_string(), 1).await;
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
async fn should_stop_the_evaluation_of_all_execution_branches_when_one_of_them_runs_out_of_phlo() {
    check_phlo_limit_exceeded("@1!(1) | @2!(2) | @3!(3)".to_string(), 20).await;
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

        let (result, cost_log) = evaluate_with_cost_log(initial_phlo, contract.clone()).await;

        assert_eq!(
            result.errors,
            vec![InterpreterError::OutOfPhlogistonsError],
            "Expected out-of-phlo for {} with initial budget {} below full cost {}",
            contract,
            initial_phlo,
            full_result.cost.value
        );
        assert_eq!(
            result.cost.value, initial_phlo,
            "Out-of-phlo must commit exactly the exhausted token budget"
        );
        assert!(
            cost_log.iter().map(|cost| cost.value).sum::<i64>() <= initial_phlo,
            "Successful charge events may not exceed the token budget"
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
