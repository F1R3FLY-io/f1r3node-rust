use std::collections::HashMap;
use std::time::Duration;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use proptest::prelude::*;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use tokio::sync::RwLock;

use super::*;
use crate::rust::interpreter::accounting::costs::Cost;
// Added by DR-132 (T17): the failure classes of an error list.
use crate::rust::interpreter::accounting::economic_failure::classify_errors;
use crate::rust::interpreter::accounting::native_runtime::tests::{
    journal_limits, priced_config, with_priced_contract,
};
use crate::rust::interpreter::accounting::native_runtime::NativeOperationTraceLimits;
use crate::rust::interpreter::accounting::phlo_execution::PhloFailure;
use crate::rust::interpreter::compiler::compiler::Compiler;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::external_services::ExternalServices;
use crate::rust::interpreter::matcher::r#match::Matcher;
use crate::rust::interpreter::merging::mergeable_tags::{
    default_mergeable_tags, non_negative_mergeable_tag_name,
};
use crate::rust::interpreter::rho_runtime::{create_native_replay_env, create_rho_env};
use crate::rust::interpreter::system_processes::Definition;

mod funding;
// Added by DR-114: recorded external-service calls in native execution.
mod recorded_calls;
use funding::FundingFixture;

fn extra_processes() -> Vec<Definition> {
    vec![Definition {
        urn: "rho:test:nativeEcho".to_owned(),
        fixed_channel: models::rust::utils::new_gstring_par(
            "native-echo".to_owned(),
            Vec::new(),
            false,
        ),
        arity: 2,
        body_ref: 901,
        remainder: None,
        handler: Box::new(|ctx| {
            Box::new(move |args| {
                let ctx = ctx.clone();
                Box::pin(async move {
                    let call = crate::rust::interpreter::contract_call::ContractCall {
                        space: ctx.space.clone(),
                        dispatcher: ctx.dispatcher.clone(),
                    };
                    let (produce, _, _, values) = call.unapply(args).ok_or_else(|| {
                        InterpreterError::ReduceError("invalid echo call".to_owned())
                    })?;
                    let [value, ack] = values.as_slice() else {
                        return Err(InterpreterError::ReduceError(
                            "invalid echo arity".to_owned(),
                        ));
                    };
                    produce(std::slice::from_ref(value), ack).await?;
                    Ok(vec![value.clone()])
                })
            })
        }),
    }]
}

/// Added by DR-132 (T17, A8): the errors of an evaluation result, with an
/// aggregate flattened.
fn result_errors(result: &Result<(), InterpreterError>) -> Vec<InterpreterError> {
    match result {
        Ok(()) => Vec::new(),
        Err(InterpreterError::AggregateError { interpreter_errors }) => interpreter_errors.clone(),
        Err(error) => vec![error.clone()],
    }
}

/// Added by DR-132 (T17, A8): the debug forms of `errors` in a fixed order.
/// Racing evaluation drains failures in completion order, so an aggregate can
/// list its errors in any order. Casper reads the list only through
/// `is_empty` and `classify_errors`.
fn sorted_errors(errors: &[InterpreterError]) -> Vec<String> {
    let mut forms: Vec<String> = errors.iter().map(|error| format!("{error:?}")).collect();
    forms.sort();
    forms
}

/// Added by DR-132 (DR-129 treatment): the rounds of a denial test. Racing
/// evaluation can order the attempts of two plays of one term differently, so
/// the limit that one play leaves before its first COMM with transfer bytes
/// does not deny a COMM in every later play. Each round replays its own play,
/// and some round must deny a COMM.
const DENIAL_ROUNDS: usize = 32;

/// The runtime binds only the bitmask mergeable tag, as dev does. The two
/// terms with an IntegerAdd channel name its tag through this test URI, and
/// the compiler injects the tag.
const INTEGER_ADD_TAG_URI: &str = "rho:test:integerAddMergeableTag";

async fn replay_process(term: &str, limit: u64) -> (Option<u64>, bool) {
    replay_process_with_context(term, limit, false, false, Interruption::None).await
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Interruption {
    None,
    Cancel,
    Export,
    ExportBeforeEvaluation,
    /// Added by DR-132 (D2-B): the changed context must end in a stall.
    Stall,
}

async fn replay_process_with_context(
    term: &str,
    limit: u64,
    change_context: bool,
    check_authority: bool,
    interruption: Interruption,
) -> (Option<u64>, bool) {
    replay_process_with_funding(
        term,
        limit,
        change_context,
        check_authority,
        interruption,
        FundingFixture::default(),
    )
    .await
}

async fn replay_process_with_funding(
    term: &str,
    limit: u64,
    change_context: bool,
    check_authority: bool,
    interruption: Interruption,
    funding: FundingFixture,
) -> (Option<u64>, bool) {
    let weights = funding.weights;
    let injection = match term.contains(INTEGER_ADD_TAG_URI) {
        true => HashMap::from([(
            INTEGER_ADD_TAG_URI.to_owned(),
            non_negative_mergeable_tag_name(),
        )]),
        false => HashMap::new(),
    };
    let parsed = Compiler::source_to_adt_with_normalizer_env(term, injection).unwrap();
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Matcher)),
    )
    .unwrap();
    play.create_checkpoint().await.unwrap();
    let history = play.get_history_repository();
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(b"native-executor-regression", funding.authority());
    let settings = priced_config(limit, weights, funding.price);
    budget.reset_for_native_execution(settings).unwrap();
    let play_mergeable = Arc::new(RwLock::new(HashMap::new()));
    let (reducer, block_data, _, deploy_data) = create_rho_env(
        play.clone(),
        play_mergeable.clone(),
        Arc::new(default_mergeable_tags()),
        &mut extra_processes(),
        budget.clone(),
        ExternalServices::noop(),
    )
    .await
    .unwrap();
    block_data.write().await.block_number = 123;
    deploy_data.write().await.timestamp = 456;
    let scope = budget.enter_comm_accounting_scope();
    let rand = || Blake2b512Random::create_from_bytes(b"native-executor-regression");
    let played = reducer.inj_with_observation(parsed.clone(), rand()).await;
    let played_observations = budget.byte_observations();
    assert!(played_observations.has_complete_measurements());
    drop(scope);
    assert!(!budget.byte_observations().has_complete_measurements());
    let recording = budget.native_budget_recording().unwrap().unwrap();
    let mut prefix_usage = 0_u64;
    let mut comm_limit = None;
    let mut denied_comm = false;
    for attempt in recording.attempts.iter() {
        let measurement = attempt.observation.measurement.as_ref().unwrap();
        if attempt.observation.kind == AuthorityByteEventKind::Comm {
            denied_comm |= !attempt.granted;
            if measurement.transfer_bytes > 0 && comm_limit.is_none() {
                comm_limit = Some(prefix_usage);
            }
        }
        if attempt.granted {
            prefix_usage += funding.charge(&attempt.observation);
        }
    }
    assert_eq!(prefix_usage, recording.used);
    let operations = budget.native_operation_recording().unwrap().unwrap();
    assert!(term == "Nil" || !operations.is_empty());
    let log = play.create_soft_checkpoint().await.log;
    let trace = with_priced_contract(limit, weights, funding.price, |contract| {
        contract
            .check_operation_journal(recording.session, &recording, operations, journal_limits())
            .unwrap()
    })
    .bind_trace(log.into(), NativeOperationTraceLimits {
        events: 100,
        source_entries: 1000,
        source_bytes: 1_000_000,
        telemetry_items: 100,
        telemetry_bytes: 100_000,
    })
    .unwrap();
    let replay_budget = RuntimeBudget::new(Cost::unsafe_max());
    replay_budget.set_deploy_signature_funded(b"native-executor-regression", funding.authority());
    let replay_settings = priced_config(limit, weights, funding.price);
    replay_budget
        .reset_for_native_execution(replay_settings)
        .unwrap();
    let mut environment = create_native_replay_env(
        trace,
        history.clone(),
        Arc::new(Box::new(Matcher)),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut extra_processes(),
        replay_budget,
        ExternalServices::noop(),
    )
    .await
    .unwrap();
    assert_eq!(environment.urn_map(), reducer.urn_map.as_ref());
    assert_eq!(
        environment.mergeable_tags(),
        reducer.mergeable_tags.as_ref()
    );
    environment.block_data.write().await.block_number = 123 + i64::from(change_context);
    environment.deploy_data.write().await.timestamp = 456 + i64::from(change_context);
    let checkpoint = environment.checkpoint().await.unwrap();
    assert!(environment.completed_usage().await.is_err());
    assert!(environment.completed_evidence().await.is_err());
    if interruption == Interruption::ExportBeforeEvaluation {
        assert!(environment.export().await.is_err());
        return (comm_limit, denied_comm);
    }
    if interruption == Interruption::Cancel {
        let deploy_data = environment.deploy_data.clone();
        let lock = deploy_data.write().await;
        let mut evaluation = Box::pin(environment.evaluate(parsed.clone(), rand()));
        tokio::select! {
            biased;
            result = &mut evaluation => panic!("locked deploy data must prevent completion: {result:?}"),
            progress = tokio::time::timeout(Duration::from_secs(10), async {
                tokio::task::yield_now().await;
            }) => progress.expect("replay must reach the interruption point"),
        }
        drop(evaluation);
        drop(lock);
        assert!(environment.checkpoint().await.is_err());
        assert!(environment.restore(checkpoint).await.is_err());
        assert!(environment.completed_evidence().await.is_err());
        assert!(environment.evaluate(parsed, rand()).await.is_err());
        return (comm_limit, denied_comm);
    }
    if change_context {
        assert!(played.0.is_ok() && played.1 == Default::default());
        // Changed by DR-132 (D2-B): racing play can record the receive on
        // `ack` after the reply. Replay of the changed reply then leaves the
        // receive waiting, and replay declares a stall. A stalled replay
        // refuses every boundary, so the environment cannot roll its budget
        // back. Casper drops the environment after a failed evaluation
        // (`replay.evaluate(..).await?`). A replay that fails without a stall
        // rolls its budget back, as before.
        // assert!(environment.evaluate(parsed, rand()).await.is_err());
        let evaluated = environment.evaluate(parsed, rand()).await;
        assert!(evaluated.is_err(), "a changed context is rejected");
        assert!(environment.completed_usage().await.is_err());
        assert!(environment.completed_evidence().await.is_err());
        let stalled = format!("{:?}", evaluated.err()).contains("native replay stalled");
        assert!(
            stalled || interruption != Interruption::Stall,
            "the changed context must end in a stall"
        );
        if stalled {
            assert!(
                environment.checkpoint().await.is_err(),
                "a stalled replay refuses every boundary"
            );
            return (comm_limit, denied_comm);
        }
        assert!(environment
            .accounting_budget()
            .authority_events()
            .is_empty());
        assert!(environment
            .accounting_budget()
            .byte_observations()
            .rows
            .is_empty());
        return (comm_limit, denied_comm);
    }
    let replayed = tokio::time::timeout(
        Duration::from_secs(10),
        environment.evaluate_raw(parsed.clone(), rand()),
    )
    .await
    .expect("native reducer must complete");
    // Changed by DR-132 (T17, A8): replay and play compare the outcome, the
    // failure classes and the sorted errors, and not the order of an error
    // list.
    // assert_eq!(replayed.0, played.0);
    assert_eq!(replayed.0.is_ok(), played.0.is_ok());
    assert_eq!(
        sorted_errors(&result_errors(&replayed.0)),
        sorted_errors(&result_errors(&played.0))
    );
    assert_eq!(
        classify_errors(&result_errors(&replayed.0)).expect("the classification allocates"),
        classify_errors(&result_errors(&played.0)).expect("the classification allocates")
    );
    assert_eq!(replayed.1, played.1);
    environment.check_complete().await.unwrap();
    assert_eq!(environment.completed_usage().await.unwrap(), recording.used);
    if check_authority {
        assert!(
            !budget.authority_events().is_empty(),
            "fixture must charge COMM authority"
        );
    }
    assert_eq!(
        environment.accounting_budget().authority_events(),
        budget.authority_events()
    );
    assert_eq!(
        environment.accounting_budget().authority_realized(),
        budget.authority_realized()
    );
    assert_eq!(
        environment.accounting_budget().authority_frontier(),
        budget.authority_frontier()
    );
    let ordered_rows = |budget: &RuntimeBudget| {
        let mut rows = budget.byte_observations().rows;
        rows.sort_by_key(|row| (row.event_id, row.kind));
        rows
    };
    assert_eq!(
        ordered_rows(environment.accounting_budget()),
        ordered_rows(&budget)
    );
    let evidence = environment.completed_evidence().await.unwrap();
    assert_eq!(evidence.usage(), recording.used);
    assert_eq!(evidence.session(), &recording.session);
    assert_eq!(evidence.recording().attempts, recording.attempts);
    assert_eq!(
        evidence.observations().rows,
        budget.byte_observations().rows
    );
    assert!(evidence.observations().has_complete_measurements());
    let expected_funding = funding.project(&played_observations, limit, recording.used, played.1);
    assert_eq!(
        funding.project(evidence.observations(), limit, evidence.usage(), replayed.1),
        expected_funding
    );
    assert_eq!(
        evidence.operations().len(),
        budget.native_operation_recording().unwrap().unwrap().len()
    );
    if limit == 1_000_000 {
        assert!(
            played.0.is_ok() && played.1 == Default::default(),
            "{played:?}"
        );
    }
    for name in ["a", "b", "out", "left", "right"] {
        let channel = models::rust::utils::new_gstring_par(name.to_owned(), Vec::new(), false);
        if name == "out" && term.contains("\"out\"") && limit == 1_000_000 {
            assert_eq!(environment.get_data(&channel).await.unwrap().len(), 1);
        }
        assert_eq!(
            environment.get_data(&channel).await.unwrap(),
            play.get_data(&channel).await
        );
        assert_eq!(
            environment.get_joins(&channel).await.unwrap(),
            play.get_joins(channel).await
        );
    }
    environment.restore(checkpoint).await.unwrap();
    assert!(environment.check_complete().await.is_err());
    assert!(environment.completed_usage().await.is_err());
    assert!(environment.completed_evidence().await.is_err());
    assert!(environment
        .accounting_budget()
        .authority_events()
        .is_empty());
    assert!(environment
        .accounting_budget()
        .authority_realized()
        .0
        .is_empty());
    assert!(environment
        .accounting_budget()
        .authority_frontier()
        .is_empty());
    assert!(environment
        .accounting_budget()
        .byte_observations()
        .rows
        .is_empty());
    assert_eq!(evidence.usage(), recording.used);
    let channel = models::rust::utils::new_gstring_par("out".to_owned(), Vec::new(), false);
    assert!(environment.get_data(&channel).await.unwrap().is_empty());
    let empty_checkpoint = environment.checkpoint().await.unwrap();
    let repeated = environment.evaluate(parsed.clone(), rand()).await.unwrap();
    let expected_errors = match replayed.0 {
        Ok(()) => Vec::new(),
        Err(InterpreterError::AggregateError { interpreter_errors }) => interpreter_errors,
        Err(error) => vec![error],
    };
    // Changed by DR-132 (T17): the repeated evaluation can list its errors in
    // another order.
    // assert_eq!(repeated.errors, expected_errors);
    assert_eq!(
        sorted_errors(&repeated.errors),
        sorted_errors(&expected_errors)
    );
    assert_eq!(repeated.economic_failures, replayed.1);
    assert_eq!(repeated.cost, budget.total_cost());
    assert_eq!(repeated.native_phlo_usage, Some(recording.used));
    let result_recording = repeated.native_budget_recording.as_ref().unwrap();
    assert_eq!(result_recording.session, recording.session);
    assert_eq!(result_recording.used, recording.used);
    assert_eq!(result_recording.attempts, recording.attempts);
    assert_eq!(result_recording.retries.len(), recording.retries.len());
    for (actual, expected) in result_recording
        .retries
        .iter()
        .zip(recording.retries.iter())
    {
        assert_eq!(actual.occurrence, expected.occurrence);
        assert_eq!(actual.observation, expected.observation);
        assert_eq!(actual.accepted_attempt, expected.accepted_attempt);
        assert_eq!(actual.fresh_before, expected.fresh_before);
    }
    assert_eq!(
        repeated.native_operation_recording.as_deref().unwrap(),
        evidence.operations()
    );
    assert_eq!(&repeated.byte_observations, evidence.observations());
    assert_eq!(
        funding.project(
            &repeated.byte_observations,
            limit,
            repeated.native_phlo_usage.unwrap(),
            repeated.economic_failures
        ),
        expected_funding
    );
    assert_eq!(repeated.authority_events, budget.authority_events());
    assert_eq!(repeated.authority_realized, budget.authority_realized());
    assert_eq!(
        repeated.authority_stack_births,
        budget.authority_stack_births()
    );
    assert!(repeated.authority_byte_events.is_empty());
    assert_eq!(repeated.quantitative_byte_cost, 0);
    if expected_errors.is_empty() {
        assert_eq!(&repeated.mergeable, &*play_mergeable.read().await);
        if term.contains("MergeableTag") {
            assert_eq!(repeated.mergeable.len(), 1);
        }
    } else {
        assert!(repeated.mergeable.is_empty());
    }
    let completed_checkpoint = environment.checkpoint().await.unwrap();
    environment.restore(completed_checkpoint).await.unwrap();
    assert!(environment.evaluate(parsed.clone(), rand()).await.is_err());
    environment.check_complete().await.unwrap();
    assert_eq!(
        environment.accounting_budget().authority_events(),
        budget.authority_events()
    );
    assert_eq!(
        environment.accounting_budget().authority_realized(),
        budget.authority_realized()
    );
    assert_eq!(
        ordered_rows(environment.accounting_budget()),
        ordered_rows(&budget)
    );
    if interruption == Interruption::Export {
        let export = environment.export().await.unwrap();
        assert_eq!(export.evidence().usage(), recording.used);
        assert_eq!(export.evidence().recording().attempts, recording.attempts);
        assert_eq!(export.evidence().operations(), evidence.operations());
        assert_eq!(export.evidence().observations(), evidence.observations());
        assert_eq!(
            funding.project(
                export.evidence().observations(),
                limit,
                export.evidence().usage(),
                repeated.economic_failures
            ),
            expected_funding
        );
        let expected_root = play.create_checkpoint().await.unwrap().root;
        assert_eq!(export.root(), &expected_root);
        assert!(history.contains_root(export.root()).unwrap());
        let reopened = history.reset(export.root()).unwrap();
        assert_eq!(reopened.root(), expected_root);
        return (comm_limit, denied_comm);
    }
    environment.restore(empty_checkpoint).await.unwrap();
    (comm_limit, denied_comm)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn native_system_callbacks_preserve_generated_payloads_and_charges(
        payload in "[a-zA-Z0-9]{0,64}",
        crypto in any::<bool>(),
    ) {
        let operation = if crypto { "sha256Hash" } else { "blake2b256Hash" };
        let term = format!(
            "new hash(`rho:crypto:{operation}`), echo(`rho:test:nativeEcho`), ack, echoed in {{ hash!(\"{payload}\".toByteArray(), *ack) | for (@digest <- ack) {{ echo!(digest, *echoed) }} | for (@result <- echoed) {{ @\"out\"!(result) }} }}"
        );
        tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap().block_on(async {
            // Changed by DR-132 (DR-129 treatment): the limit of one play
            // denies a COMM in a later play only when both order their
            // attempts alike. Each round replays its own play.
            // let (comm_limit, _) = replay_process(&term, 1_000_000).await;
            // assert!(replay_process(&term, comm_limit.unwrap()).await.1);
            let mut denied = false;
            for _ in 0..DENIAL_ROUNDS {
                let (comm_limit, _) = replay_process(&term, 1_000_000).await;
                if replay_process(&term, comm_limit.expect("play records an attempt that transfers bytes")).await.1 {
                    denied = true;
                    break;
                }
            }
            assert!(denied, "no round denied a COMM at its limit");
        });
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_environment_replays_system_callbacks_and_rejects_changed_context() {
    for term in [
        r#"new hash(`rho:crypto:sha256Hash`), ack in { hash!("hello".toByteArray(), *ack) | for (@x <- ack) { @"out"!(x) } }"#,
        r#"new hash(`rho:crypto:blake2b256Hash`), ack in { hash!("hello".toByteArray(), *ack) | for (@x <- ack) { @"out"!(x) } }"#,
        r#"new echo(`rho:test:nativeEcho`), ack in { echo!(7, *ack) | for (@x <- ack) { @"out"!(x) } }"#,
        r#"new block(`rho:block:data`), ack in { block!(*ack) | for (@number, @stamp, @sender <- ack) { @"out"!((number, stamp, sender)) } }"#,
        r#"new deploy(`rho:deploy:data`), ack in { deploy!(*ack) | for (@stamp, owner, id <- ack) { @"out"!(stamp) } }"#,
    ] {
        // Changed by DR-132 (DR-129 treatment): the limit of one play denies
        // a COMM in a later play only when both order their attempts alike.
        // Each round replays its own play.
        // let (comm_limit, _) = replay_process(term, 1_000_000).await;
        // replay_process(term, 0).await;
        // assert!(
        //     replay_process(term, comm_limit.expect("callback COMM"))
        //         .await
        //         .1
        // );
        replay_process(term, 0).await;
        let mut denied = false;
        for _ in 0..DENIAL_ROUNDS {
            let (comm_limit, _) = replay_process(term, 1_000_000).await;
            if replay_process(term, comm_limit.expect("callback COMM"))
                .await
                .1
            {
                denied = true;
                break;
            }
        }
        assert!(denied, "no round denied a COMM at its limit for {term}");
        if term.contains("rho:block:data") || term.contains("rho:deploy:data") {
            replay_process_with_context(term, 1_000_000, true, false, Interruption::None).await;
        }
    }
}

/// Added by DR-132 (D2-B, review m10): on a current-thread runtime, play
/// records the reply on `ack` before the receive. Replay of a changed block
/// context then produces a reply that no row records, so the receive waits,
/// and replay declares a stall. `evaluate` returns the stall, and the
/// stalled environment refuses every boundary.
#[tokio::test]
async fn native_environment_stalls_on_a_changed_context_after_the_reply() {
    replay_process_with_context(
        r#"new block(`rho:block:data`), ack in { block!(*ack) | for (@number, @stamp, @sender <- ack) { @"out"!((number, stamp, sender)) } }"#,
        1_000_000,
        true,
        false,
        Interruption::Stall,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_replay_authority_state_matches_recorded_execution() {
    replay_process_with_context(
        r#"@"a"!(7) | for (@x <- @"a") { @"out"!(x) }"#,
        1_000_000,
        false,
        true,
        Interruption::None,
    )
    .await;
}

/// D-F2 (DR-118): every introduction shape that the reducer measures replays
/// with the played observations: a send with several data, a receive with a
/// guard, a join, a peek, a persistent send, a persistent receive and a send
/// on a `bundle+` name. In test builds each premeasured observation also
/// checks that it equals the walked observation.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn premeasured_introductions_replay_for_every_measured_shape() {
    replay_process_with_context(
        r#"
        new a, b, c, d, e, f, g, out in {
          a!(1, "two", [3]) |
          for (@x, @y, @z <- a where x == 1) { out!(y) } |
          for (@p <- b & @q <- c) { out!(p + q) } |
          b!(4) | c!(5) |
          for (@v <<- d) { out!(v) } | d!(6) |
          e!!(7) | for (@w <- e) { out!(w) } |
          for (@u <= f) { out!(u) } | f!(8) | f!(9) |
          @"bundled"!(bundle+{*g}) | for (@h <- @"bundled") { @h!(10) } |
          for (@t <- g) { out!(t) }
        }
        "#,
        1_000_000,
        false,
        true,
        Interruption::None,
    )
    .await;
}

// Changed by DR-132 (T4): the splits went with the deterministic scheduler
// (D3-A), so the legacy and the compacted splitter no longer exist.
// /// Plays `term` natively with the legacy or the compacted splitter and
// /// returns the checkpoint root, the event log and the length of the longest
// /// recorded operation path (C9, DR-84). The session reads the splitter flag
// /// of this thread when it is built.
// async fn played_checkpoint(
//     term: &str,
//     legacy: bool,
// ) -> (
//     rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash,
//     rspace_plus_plus::rspace::trace::Log,
//     usize,
// ) {
//     struct ResetSplitter;
//     impl Drop for ResetSplitter {
//         fn drop(&mut self) {
//             crate::rust::interpreter::deterministic_reduction::LEGACY_SPLIT
//                 .with(|flag| flag.set(false));
//         }
//     }
//     crate::rust::interpreter::deterministic_reduction::LEGACY_SPLIT.with(|flag| flag.set(legacy));
//     let _reset = ResetSplitter;
//     let funding = FundingFixture::default();
//     let (weights, limit) = (funding.weights, 1_000_000);
//     let parsed = Compiler::source_to_adt(term).unwrap();
//     let mut stores = InMemoryStoreManager::new();
//     let (play, _) = RSpace::create_with_replay(
//         stores.r_space_stores().await.unwrap(),
//         Arc::new(Box::new(Matcher)),
//     )
//     .unwrap();
//     play.create_checkpoint().await.unwrap();
//     let budget = RuntimeBudget::new(Cost::unsafe_max());
//     budget.set_deploy_signature_funded(b"native-executor-regression", funding.authority());
//     let settings = priced_config(limit, weights, funding.price);
//     budget.reset_for_native_execution(settings).unwrap();
//     let (reducer, block_data, _, deploy_data) = create_rho_env(
//         play.clone(),
//         Arc::new(RwLock::new(HashMap::new())),
//         Arc::new(default_mergeable_tags()),
//         &mut extra_processes(),
//         budget.clone(),
//         ExternalServices::noop(),
//     )
//     .await
//     .unwrap();
//     block_data.write().await.block_number = 123;
//     deploy_data.write().await.timestamp = 456;
//     let scope = budget.enter_comm_accounting_scope();
//     let played = reducer
//         .inj_with_observation(
//             parsed,
//             Blake2b512Random::create_from_bytes(b"native-executor-regression"),
//         )
//         .await;
//     assert!(played.0.is_ok(), "{played:?}");
//     drop(scope);
//     let deepest = budget
//         .native_operation_recording()
//         .unwrap()
//         .unwrap()
//         .iter()
//         .map(|record| record.occurrence.path.depth())
//         .max()
//         .unwrap_or(0);
//     let checkpoint = play.create_checkpoint().await.unwrap();
//     (checkpoint.root, checkpoint.log, deepest)
// }
//
// /// C9 (DR-84): the compacted splitter keeps the forward lexicographic order
// /// of every generated path. Native play of each corpus term therefore fires
// /// the same COMMs in the same order and reaches the same root with the
// /// compacted splitter and with the legacy splitter.
// ///
// /// Changed by DR-129: restored dev's randomized candidate order (user
// /// decision 2026-10-12). Play picks at random among candidates that match
// /// equally. In the second and third corpus terms play meets such choices, so
// /// two plays can fire their COMMs in other orders or pair other candidates,
// /// with either splitter. For those terms the plays must log the same produces
// /// and consumes, fire the same number of COMMs and reach the same root. The
// /// other terms offer play no choice and still compare their whole logs.
// #[tokio::test]
// async fn compacted_schedule_equals_legacy_schedule() {
//     // Changed by DR-129: each term carries whether play meets a choice.
//     let corpus = [
//         (
//             r#"new loop, a, b, c, d in {
//             contract loop(@n) = { if (n > 0) { loop!(n - 1) } } | loop!(12) |
//             a!(1) | b!(2) | c!(3) | d!(4) |
//             for (_ <- a) { Nil } | for (_ <- b) { Nil } | for (_ <- c) { Nil } | for (_ <- d) { Nil }
//         }"#,
//             false,
//         ),
//         (
//             r#"new fork, done in {
//             contract fork(@n) = { if (n > 0) { fork!(n - 1) | fork!(n - 1) } else { done!(n) } } |
//             fork!(4) | for (@x <= done) { Nil }
//         }"#,
//             true,
//         ),
//         (
//             r#"new a, b, ack in {
//             contract a(@x) = { ack!(x) } | a!(1) | a!(2) | b!(3) | b!(4) |
//             for (@x <- ack & @y <- b) { Nil } | for (@x <- ack & @y <- b) { Nil }
//         }"#,
//             true,
//         ),
//         (
//             r#"new c, d, e in {
//             c!!(1) | for (@x <<- c) { d!(x) } | for (@y <- c) { e!(y + 1) } |
//             for (@u <- d & @v <- e) { Nil }
//         }"#,
//             false,
//         ),
//     ];
//     // Added by DR-129: the produces and consumes of a log, in a fixed order,
//     // and the number of its COMMs.
//     let operations = |log: &rspace_plus_plus::rspace::trace::Log| {
//         let mut events: Vec<String> = log
//             .iter()
//             .filter(|event| {
//                 !matches!(
//                     event,
//                     rspace_plus_plus::rspace::trace::event::Event::Comm(_)
//                 )
//             })
//             .map(|event| format!("{event:?}"))
//             .collect();
//         events.sort();
//         events
//     };
//     let comms = |log: &rspace_plus_plus::rspace::trace::Log| {
//         log.iter()
//             .filter(|event| {
//                 matches!(
//                     event,
//                     rspace_plus_plus::rspace::trace::event::Event::Comm(_)
//                 )
//             })
//             .count()
//     };
//     // for term in corpus {
//     for (term, offers_choice) in corpus {
//         let (legacy_root, legacy_log, legacy_depth) = played_checkpoint(term, true).await;
//         let (compacted_root, compacted_log, compacted_depth) = played_checkpoint(term, false).await;
//         // Changed by DR-132 (D1-A): the recorder names each row by its journal
//         // index, so every recorded path has one segment with either splitter.
//         // T4 retires this test with the splits in S3.
//         // assert!(compacted_depth < legacy_depth, "{term}");
//         assert_eq!((compacted_depth, legacy_depth), (1, 1), "{term}");
//         assert!(
//             legacy_log.iter().any(|event| matches!(
//                 event,
//                 rspace_plus_plus::rspace::trace::event::Event::Comm(_)
//             )),
//             "{term}"
//         );
//         // Changed by DR-129: whole logs only for a term without a choice.
//         // assert_eq!(compacted_log, legacy_log, "{term}");
//         match offers_choice {
//             false => assert_eq!(compacted_log, legacy_log, "{term}"),
//             true => {
//                 assert_eq!(
//                     operations(&compacted_log),
//                     operations(&legacy_log),
//                     "{term}"
//                 );
//                 assert_eq!(comms(&compacted_log), comms(&legacy_log), "{term}");
//             }
//         }
//         assert_eq!(compacted_root, legacy_root, "{term}");
//     }
// }

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_native_evaluation_cannot_export_or_restore_partial_state() {
    replay_process_with_context(
        r#"new deploy(`rho:deploy:data`), ack in { deploy!(*ack) | for (@stamp, owner, id <- ack) { @"out"!(stamp) } }"#,
        1_000_000, false, false, Interruption::Cancel,
    ).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_export_binds_completed_accounting_to_the_recorded_state_root() {
    for term in [
        "Nil",
        r#"@"out"!!(7)"#,
        r#"@"a"!(7) | @"b"!(8) | for (@x <- @"a"; @y <- @"b") { @"out"!(x + y) }"#,
        r#"new tag(`rho:test:integerAddMergeableTag`) in { @(*tag, "counter")!(1) }"#,
    ] {
        // Changed by DR-132 (DR-129 treatment): the limit of one play denies
        // a COMM in a later play only when both order their attempts alike.
        // Each round replays its own play.
        // let (comm_limit, _) =
        //     replay_process_with_context(term, 1_000_000, false, false, Interruption::Export).await;
        // replay_process_with_context(term, 0, false, false, Interruption::Export).await;
        // if let Some(limit) = comm_limit {
        //     assert!(
        //         replay_process_with_context(term, limit, false, false, Interruption::Export)
        //             .await
        //             .1
        //     );
        // }
        replay_process_with_context(term, 0, false, false, Interruption::Export).await;
        let mut transfers = false;
        let mut denied = false;
        for _ in 0..DENIAL_ROUNDS {
            let (comm_limit, _) =
                replay_process_with_context(term, 1_000_000, false, false, Interruption::Export)
                    .await;
            let Some(limit) = comm_limit else {
                break;
            };
            transfers = true;
            if replay_process_with_context(term, limit, false, false, Interruption::Export)
                .await
                .1
            {
                denied = true;
                break;
            }
        }
        assert_eq!(
            denied, transfers,
            "no round denied a COMM at its limit for {term}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_native_trace_cannot_export_before_evaluation() {
    replay_process_with_context(
        "Nil",
        1_000_000,
        false,
        false,
        Interruption::ExportBeforeEvaluation,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_executor_replays_parsed_processes_denied_comms_and_exact_usage() {
    for term in [
        "Nil",
        r#"@"out"!(7)"#,
        r#"@"a"!(7) | for (@x <- @"a") { @"out"!(x) }"#,
        r#"@"a"!(7) | @"b"!(8) | for (@x <- @"a"; @y <- @"b") { @"out"!(x + y) }"#,
        r#"@"left"!(1) | @"right"!(2)"#,
        r#"new tag(`rho:test:integerAddMergeableTag`) in { @(*tag, "counter")!(1) }"#,
        r#"new tag(`rho:system:bitmaskMergeableTag`) in { @(*tag, "counter")!(1) }"#,
    ] {
        // Changed by DR-132 (DR-129 treatment): the limit of one play denies
        // a COMM in a later play only when both order their attempts alike.
        // Each round replays its own play.
        // let (comm_limit, _) = replay_process(term, 1_000_000).await;
        // replay_process(term, 0).await;
        // if let Some(limit) = comm_limit {
        //     assert!(
        //         replay_process(term, limit).await.1,
        //         "fixture must reject a COMM after accepted introductions"
        //     );
        // }
        replay_process(term, 0).await;
        let mut transfers = false;
        let mut denied = false;
        for _ in 0..DENIAL_ROUNDS {
            let (comm_limit, _) = replay_process(term, 1_000_000).await;
            let Some(limit) = comm_limit else {
                break;
            };
            transfers = true;
            if replay_process(term, limit).await.1 {
                denied = true;
                break;
            }
        }
        assert_eq!(
            denied, transfers,
            "fixture must reject a COMM after accepted introductions: {term}"
        );
    }
}
