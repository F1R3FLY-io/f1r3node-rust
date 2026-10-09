use std::collections::HashMap;
use std::time::Duration;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rust::host_work::{HostWorkDimension, HostWorkUnits, HostWorkUsages};
use proptest::prelude::*;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use tokio::sync::RwLock;

use super::*;
use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::accounting::native_runtime::tests::{
    journal_limits, priced_config, with_priced_contract,
};
use crate::rust::interpreter::accounting::native_runtime::NativeOperationTraceLimits;
use crate::rust::interpreter::accounting::phlo_execution::PhloFailure;
use crate::rust::interpreter::compiler::compiler::Compiler;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::external_services::ExternalServices;
use crate::rust::interpreter::host_work::HostWorkBudget;
use crate::rust::interpreter::interpreter::EvaluateResult;
use crate::rust::interpreter::matcher::r#match::Matcher;
use crate::rust::interpreter::merging::mergeable_tags::default_mergeable_tags;
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

async fn replay_process(term: &str, limit: u64) -> (Option<u64>, bool) {
    replay_process_with_context(term, limit, false, false, Interruption::None).await
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Interruption {
    None,
    Cancel,
    RejectHost,
    RejectCheckpoint,
    Export,
    ExportBeforeEvaluation,
}

fn assert_host_rejected(result: &EvaluateResult) {
    assert_eq!(result.errors, vec![InterpreterError::HostWorkRejected]);
    assert!(result.economic_failures.contains(PhloFailure::Platform));
    assert!(!result.economic_failures.permits_retained_charge());
    assert_eq!(result.cost.value, 0);
    assert!(result.native_phlo_usage.is_none());
    assert!(result.native_budget_recording.is_none());
    assert!(result.native_operation_recording.is_none());
    assert!(result.byte_observations.rows.is_empty());
    assert!(result.authority_events.is_empty());
    assert!(result.authority_byte_events.is_empty());
    assert!(result.authority_realized.0.is_empty());
    assert!(result.authority_stack_births.is_empty());
    assert!(result.mergeable.is_empty());
    assert_eq!(result.quantitative_byte_cost, 0);
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
    let parsed = Compiler::source_to_adt(term).unwrap();
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
    let host = settings.host_work();
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
    let played = reducer
        .inj_with_observation(parsed.clone(), rand(), Some(host))
        .await;
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
    let host = priced_config(limit, weights, funding.price).host_work();
    let trace = with_priced_contract(limit, weights, funding.price, |contract| {
        contract
            .check_operation_journal(
                recording.session,
                &recording,
                operations,
                journal_limits(),
                &host,
            )
            .unwrap()
    })
    .bind_trace(
        log.into(),
        NativeOperationTraceLimits {
            events: 100,
            source_entries: 1000,
            source_bytes: 1_000_000,
            telemetry_items: 100,
            telemetry_bytes: 100_000,
        },
        &host,
    )
    .unwrap();
    let replay_budget = RuntimeBudget::new(Cost::unsafe_max());
    replay_budget.set_deploy_signature_funded(b"native-executor-regression", funding.authority());
    let mut replay_settings = priced_config(limit, weights, funding.price);
    replay_settings.host_work = host.clone();
    replay_budget
        .reset_for_native_execution(replay_settings)
        .unwrap();
    let mut environment = create_native_replay_env(
        trace,
        history.clone(),
        Arc::new(Box::new(Matcher)),
        host.clone(),
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
    if interruption == Interruption::RejectCheckpoint {
        let dimension = HostWorkDimension::VerificationOperations;
        let remaining = host.limits().get(dimension).get() - host.usage(dimension).get();
        host.reserve(dimension, HostWorkUnits::new(remaining))
            .unwrap();
        assert!(!host.is_rejected());
        let result = environment.evaluate(parsed, rand()).await.unwrap();
        assert!(host.is_rejected());
        assert_host_rejected(&result);
        assert!(environment.completed_evidence().await.is_err());
        assert!(environment
            .accounting_budget()
            .authority_events()
            .is_empty());
        environment.restore(checkpoint).await.unwrap();
        return (comm_limit, denied_comm);
    }
    if matches!(
        interruption,
        Interruption::Cancel | Interruption::RejectHost
    ) {
        let deploy_data = environment.deploy_data.clone();
        let observed_budget = environment.accounting_budget().clone();
        let lock = deploy_data.write().await;
        let mut evaluation = Box::pin(environment.evaluate(parsed.clone(), rand()));
        tokio::select! {
            biased;
            result = &mut evaluation => panic!("locked deploy data must prevent completion: {result:?}"),
            progress = tokio::time::timeout(Duration::from_secs(10), async {
                if interruption == Interruption::RejectHost {
                    while observed_budget.authority_events().is_empty() {
                        tokio::task::yield_now().await;
                    }
                } else {
                    tokio::task::yield_now().await;
                }
            }) => progress.expect("replay must reach the interruption point"),
        }
        if interruption == Interruption::RejectHost {
            assert!(
                !observed_budget.authority_events().is_empty(),
                "rejection must occur after a published COMM"
            );
            assert!(host
                .reserve(
                    HostWorkDimension::VerificationOperations,
                    HostWorkUnits::new(u64::MAX)
                )
                .is_err());
            drop(lock);
            let result = evaluation.await.unwrap();
            assert_host_rejected(&result);
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
                .byte_observations()
                .rows
                .is_empty());
            let channel = models::rust::utils::new_gstring_par("out".to_owned(), Vec::new(), false);
            assert!(matches!(
                environment.get_data(&channel).await,
                Err(RSpaceError::HostWorkRejected)
            ));
            let inspection = HostWorkBudget::new(host.limits());
            assert!(environment
                .get_data_with_host_work(&channel, &inspection)
                .await
                .unwrap()
                .is_empty());
            assert!(host.is_rejected());
            environment.restore(checkpoint).await.unwrap();
            return (comm_limit, denied_comm);
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
        assert!(environment.evaluate(parsed, rand()).await.is_err());
        assert!(environment.completed_usage().await.is_err());
        assert!(environment.completed_evidence().await.is_err());
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
    assert_eq!(replayed.0, played.0);
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
    assert_eq!(repeated.errors, expected_errors, "host={:?}", host.usages());
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
    assert!(host
        .reserve(
            HostWorkDimension::VerificationOperations,
            HostWorkUnits::new(u64::MAX)
        )
        .is_err());
    let rejected = environment.evaluate(parsed, rand()).await.unwrap();
    assert_host_rejected(&rejected);
    assert!(environment.completed_evidence().await.is_err());
    assert!(environment
        .accounting_budget()
        .authority_events()
        .is_empty());
    assert!(matches!(
        environment.get_data(&channel).await,
        Err(RSpaceError::HostWorkRejected)
    ));
    let inspection = HostWorkBudget::new(host.limits());
    assert!(environment
        .get_data_with_host_work(&channel, &inspection)
        .await
        .unwrap()
        .is_empty());
    assert!(host.is_rejected());
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
            let (comm_limit, _) = replay_process(&term, 1_000_000).await;
            assert!(replay_process(&term, comm_limit.unwrap()).await.1);
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
        let (comm_limit, _) = replay_process(term, 1_000_000).await;
        replay_process(term, 0).await;
        assert!(
            replay_process(term, comm_limit.expect("callback COMM"))
                .await
                .1
        );
        if term.contains("rho:block:data") || term.contains("rho:deploy:data") {
            replay_process_with_context(term, 1_000_000, true, false, Interruption::None).await;
        }
    }
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

/// Host-work use of the native replay of `term`: play records the native
/// journal, the checked journal binds the trace, and the replay runs it
/// (C14, DR-79).
async fn replayed_host_usage(term: &str) -> HostWorkUsages {
    replayed_host_usage_with_schedule(term, None).await
}

/// D-C5a (DR-100): a schedule for the replay session of
/// `replayed_host_usage_with_schedule`: the seed of the detached-task yields,
/// the direct-path counter, and the pre-DR-100 joins read.
struct ReplaySchedule {
    seed: u64,
    direct_paths: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    legacy_joins_read: bool,
}

/// `replayed_host_usage`, with the test hooks of the schedule set only while
/// the replay session is built (the play session runs without them).
async fn replayed_host_usage_with_schedule(
    term: &str,
    schedule: Option<&ReplaySchedule>,
) -> HostWorkUsages {
    let funding = FundingFixture::default();
    let (weights, limit) = (funding.weights, 1_000_000);
    let parsed = Compiler::source_to_adt(term).unwrap();
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
    let play_host = settings.host_work();
    budget.reset_for_native_execution(settings).unwrap();
    let (reducer, block_data, _, deploy_data) = create_rho_env(
        play.clone(),
        Arc::new(RwLock::new(HashMap::new())),
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
    let played = reducer
        .inj_with_observation(parsed.clone(), rand(), Some(play_host))
        .await;
    assert!(played.0.is_ok(), "{played:?}");
    drop(scope);
    let recording = budget.native_budget_recording().unwrap().unwrap();
    let operations = budget.native_operation_recording().unwrap().unwrap();
    let log = play.create_soft_checkpoint().await.log;
    let host = priced_config(limit, weights, funding.price).host_work();
    let trace = with_priced_contract(limit, weights, funding.price, |contract| {
        contract
            .check_operation_journal(
                recording.session,
                &recording,
                operations,
                journal_limits(),
                &host,
            )
            .unwrap()
    })
    .bind_trace(
        log.into(),
        NativeOperationTraceLimits {
            events: 1_000,
            source_entries: 10_000,
            source_bytes: 10_000_000,
            telemetry_items: 1_000,
            telemetry_bytes: 1_000_000,
        },
        &host,
    )
    .unwrap();
    let replay_budget = RuntimeBudget::new(Cost::unsafe_max());
    replay_budget.set_deploy_signature_funded(b"native-executor-regression", funding.authority());
    let mut replay_settings = priced_config(limit, weights, funding.price);
    replay_settings.host_work = host.clone();
    replay_budget
        .reset_for_native_execution(replay_settings)
        .unwrap();
    let mut environment = create_native_replay_env(
        trace,
        history,
        Arc::new(Box::new(Matcher)),
        host.clone(),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut extra_processes(),
        replay_budget,
        ExternalServices::noop(),
    )
    .await
    .unwrap();
    environment.block_data.write().await.block_number = 123;
    environment.deploy_data.write().await.timestamp = 456;
    if let Some(schedule) = schedule {
        use crate::rust::interpreter::deterministic_reduction::{
            DIRECT_PATH_PROBE, LEGACY_NATIVE_FOOTPRINT_READ, SCHEDULE_SEED,
        };
        SCHEDULE_SEED.with(|seed| seed.set(Some(schedule.seed)));
        DIRECT_PATH_PROBE.with(|probe| *probe.borrow_mut() = Some(schedule.direct_paths.clone()));
        LEGACY_NATIVE_FOOTPRINT_READ.with(|legacy| legacy.set(schedule.legacy_joins_read));
    }
    let replayed = tokio::time::timeout(
        Duration::from_secs(60),
        environment.evaluate_raw(parsed, rand()),
    )
    .await
    .expect("native reducer must complete");
    if schedule.is_some() {
        use crate::rust::interpreter::deterministic_reduction::{
            DIRECT_PATH_PROBE, LEGACY_NATIVE_FOOTPRINT_READ, SCHEDULE_SEED,
        };
        SCHEDULE_SEED.with(|seed| seed.set(None));
        DIRECT_PATH_PROBE.with(|probe| *probe.borrow_mut() = None);
        LEGACY_NATIVE_FOOTPRINT_READ.with(|legacy| legacy.set(false));
    }
    assert_eq!(replayed.0, played.0);
    environment.check_complete().await.unwrap();
    assert_eq!(
        environment.accounting_budget().authority_events(),
        budget.authority_events()
    );
    if schedule.is_none() {
        assert!(environment.accounting_budget().authority_events().len() > 10);
    }
    host.usages()
}

/// D-C5a (DR-100): the replay usages and direct-path counts of `term` under
/// the schedule seeds `0..seeds` on a current-thread runtime.
fn seeded_replay_usages(
    term: &str,
    seeds: u64,
    legacy_joins_read: bool,
) -> Vec<(HostWorkUsages, usize)> {
    let mut results = Vec::with_capacity(usize::try_from(seeds).expect("seed count fits usize"));
    for seed in 0..seeds {
        let schedule = ReplaySchedule {
            seed,
            direct_paths: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            legacy_joins_read,
        };
        let usages = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime")
            .block_on(replayed_host_usage_with_schedule(term, Some(&schedule)));
        results.push((
            usages,
            schedule
                .direct_paths
                .load(std::sync::atomic::Ordering::Relaxed),
        ));
    }
    results
}

/// D-C5a (DR-100; `ReplayChargeScheduleInvariance.direct_and_driver_paths_charge_alike`):
/// seeded schedules decide which produces of a lone participant take the
/// direct path and which reach the driver. The replay charges the same
/// host-work use under every seed, and the seeds reach different path
/// choices, so the test is not vacuous. Negative control: with the joins read
/// of the driver path restored, the charge follows the path choice.
#[test]
fn replay_charges_are_schedule_independent() {
    let terms = [
        r#"@"a"!(1) | match [1, 2, 3] { [4, 5] => @"b"!(2) _ => Nil }"#,
        r#"new loop, a, b, c, d in {
            contract loop(@n) = { if (n > 0) { loop!(n - 1) } } | loop!(6) |
            a!(1) | b!(2) | c!(3) | d!(4) |
            for (_ <- a) { Nil } | for (_ <- b) { Nil } | for (_ <- c) { Nil } | for (_ <- d) { Nil }
        }"#,
    ];
    let mut reached_both_paths = false;
    let mut legacy_differs = false;
    for term in terms {
        let results = seeded_replay_usages(term, 16, false);
        let (first, _) = &results[0];
        for (seed, (usages, _)) in results.iter().enumerate() {
            assert_eq!(
                usages, first,
                "seed {seed} changed the replay charge of {term}"
            );
        }
        let paths = results
            .iter()
            .map(|(_, direct)| *direct)
            .collect::<std::collections::BTreeSet<_>>();
        if paths.len() > 1 {
            reached_both_paths = true;
            let legacy = seeded_replay_usages(term, 16, true);
            legacy_differs |= legacy.iter().any(|(usages, _)| usages != &legacy[0].0);
        }
    }
    assert!(
        reached_both_paths,
        "no seed changed a path choice; the test would be vacuous"
    );
    assert!(
        legacy_differs,
        "the pre-DR-100 joins read did not change the charge"
    );
}

/// C14 (DR-79): the replay authority charges use live map sizes. Native
/// execution serializes each frontier, so the replay of one trace reports
/// the same host-work use in all dimensions on a current-thread runtime and
/// on multi-thread runtimes. The deploy has parallel branches and more than
/// ten COMMs, so the event map grows past one tree level.
#[test]
fn native_replay_authority_charge_is_schedule_independent() {
    let term = r#"new loop, a, b, c, d in {
        contract loop(@n) = { if (n > 0) { loop!(n - 1) } } | loop!(12) |
        a!(1) | b!(2) | c!(3) | d!(4) |
        for (_ <- a) { Nil } | for (_ <- b) { Nil } | for (_ <- c) { Nil } | for (_ <- d) { Nil }
    }"#;
    let current = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime")
        .block_on(replayed_host_usage(term));
    for workers in [2, 4, 8] {
        let parallel = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(workers)
            .enable_all()
            .build()
            .expect("multi-thread runtime")
            .block_on(replayed_host_usage(term));
        assert_eq!(parallel, current, "{workers} workers");
    }
}

/// Plays `term` natively with the legacy or the compacted splitter and
/// returns the checkpoint root, the event log and the length of the longest
/// recorded operation path (C9, DR-84). The session reads the splitter flag
/// of this thread when it is built.
async fn played_checkpoint(
    term: &str,
    legacy: bool,
) -> (
    rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash,
    rspace_plus_plus::rspace::trace::Log,
    usize,
) {
    struct ResetSplitter;
    impl Drop for ResetSplitter {
        fn drop(&mut self) {
            crate::rust::interpreter::deterministic_reduction::LEGACY_SPLIT
                .with(|flag| flag.set(false));
        }
    }
    crate::rust::interpreter::deterministic_reduction::LEGACY_SPLIT.with(|flag| flag.set(legacy));
    let _reset = ResetSplitter;
    let funding = FundingFixture::default();
    let (weights, limit) = (funding.weights, 1_000_000);
    let parsed = Compiler::source_to_adt(term).unwrap();
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Matcher)),
    )
    .unwrap();
    play.create_checkpoint().await.unwrap();
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(b"native-executor-regression", funding.authority());
    let settings = priced_config(limit, weights, funding.price);
    let play_host = settings.host_work();
    budget.reset_for_native_execution(settings).unwrap();
    let (reducer, block_data, _, deploy_data) = create_rho_env(
        play.clone(),
        Arc::new(RwLock::new(HashMap::new())),
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
    let played = reducer
        .inj_with_observation(
            parsed,
            Blake2b512Random::create_from_bytes(b"native-executor-regression"),
            Some(play_host),
        )
        .await;
    assert!(played.0.is_ok(), "{played:?}");
    drop(scope);
    let deepest = budget
        .native_operation_recording()
        .unwrap()
        .unwrap()
        .iter()
        .map(|record| record.occurrence.path.depth())
        .max()
        .unwrap_or(0);
    let checkpoint = play.create_checkpoint().await.unwrap();
    (checkpoint.root, checkpoint.log, deepest)
}

/// C9 (DR-84): the compacted splitter keeps the forward lexicographic order
/// of every generated path. Native play of each corpus term therefore fires
/// the same COMMs in the same order and reaches the same root with the
/// compacted splitter and with the legacy splitter.
#[tokio::test]
async fn compacted_schedule_equals_legacy_schedule() {
    let corpus = [
        r#"new loop, a, b, c, d in {
            contract loop(@n) = { if (n > 0) { loop!(n - 1) } } | loop!(12) |
            a!(1) | b!(2) | c!(3) | d!(4) |
            for (_ <- a) { Nil } | for (_ <- b) { Nil } | for (_ <- c) { Nil } | for (_ <- d) { Nil }
        }"#,
        r#"new fork, done in {
            contract fork(@n) = { if (n > 0) { fork!(n - 1) | fork!(n - 1) } else { done!(n) } } |
            fork!(4) | for (@x <= done) { Nil }
        }"#,
        r#"new a, b, ack in {
            contract a(@x) = { ack!(x) } | a!(1) | a!(2) | b!(3) | b!(4) |
            for (@x <- ack & @y <- b) { Nil } | for (@x <- ack & @y <- b) { Nil }
        }"#,
        r#"new c, d, e in {
            c!!(1) | for (@x <<- c) { d!(x) } | for (@y <- c) { e!(y + 1) } |
            for (@u <- d & @v <- e) { Nil }
        }"#,
    ];
    for term in corpus {
        let (legacy_root, legacy_log, legacy_depth) = played_checkpoint(term, true).await;
        let (compacted_root, compacted_log, compacted_depth) = played_checkpoint(term, false).await;
        assert!(compacted_depth < legacy_depth, "{term}");
        assert!(
            legacy_log.iter().any(|event| matches!(
                event,
                rspace_plus_plus::rspace::trace::event::Event::Comm(_)
            )),
            "{term}"
        );
        assert_eq!(compacted_log, legacy_log, "{term}");
        assert_eq!(compacted_root, legacy_root, "{term}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_native_evaluation_cannot_export_or_restore_partial_state() {
    replay_process_with_context(
        r#"new deploy(`rho:deploy:data`), ack in { deploy!(*ack) | for (@stamp, owner, id <- ack) { @"out"!(stamp) } }"#,
        1_000_000, false, false, Interruption::Cancel,
    ).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_replay_host_rejection_restores_partial_execution_without_receipts() {
    replay_process_with_context(
        r#"new deploy(`rho:deploy:data`), ack in { deploy!(*ack) | for (@stamp, owner, id <- ack) { @"out"!(stamp) } }"#,
        1_000_000, false, false, Interruption::RejectHost,
    ).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_replay_checkpoint_rejection_is_nonbillable_without_execution() {
    replay_process_with_context(
        r#"@"a"!(7) | for (@x <- @"a") { @"out"!(x) }"#,
        1_000_000,
        false,
        false,
        Interruption::RejectCheckpoint,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_export_binds_completed_accounting_to_the_recorded_state_root() {
    for term in [
        "Nil",
        r#"@"out"!!(7)"#,
        r#"@"a"!(7) | @"b"!(8) | for (@x <- @"a"; @y <- @"b") { @"out"!(x + y) }"#,
        r#"new tag(`rho:system:integerAddMergeableTag`) in { @(*tag, "counter")!(1) }"#,
    ] {
        let (comm_limit, _) =
            replay_process_with_context(term, 1_000_000, false, false, Interruption::Export).await;
        replay_process_with_context(term, 0, false, false, Interruption::Export).await;
        if let Some(limit) = comm_limit {
            assert!(
                replay_process_with_context(term, limit, false, false, Interruption::Export)
                    .await
                    .1
            );
        }
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
        r#"new tag(`rho:system:integerAddMergeableTag`) in { @(*tag, "counter")!(1) }"#,
        r#"new tag(`rho:system:bitmaskMergeableTag`) in { @(*tag, "counter")!(1) }"#,
    ] {
        let (comm_limit, _) = replay_process(term, 1_000_000).await;
        replay_process(term, 0).await;
        if let Some(limit) = comm_limit {
            assert!(
                replay_process(term, limit).await.1,
                "fixture must reject a COMM after accepted introductions"
            );
        }
    }
}
