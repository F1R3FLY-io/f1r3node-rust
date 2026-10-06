// Decision 46 (post-merge Part B, Stage 1): spaces fail closed in native execution.
//
// Since the merge of feature/cost-accounted-rho, `create_native_replay_env` is production code:
// proposer self-certification and validator replay run every offered deploy in it, and its
// Reified runtime rejects every space operation. Native play now rejects the first space
// operation of a deploy with the same error, before any space work. So the deploy fails in play
// with no space journal row, and native replay reproduces the failure: the same error, a complete
// journal, and the same phlo usage. A program without spaces plays and replays natively as
// before.

use std::collections::HashMap;
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativeBudgetTraceLimits, NativePhloDimension,
    NativePhloExecutionContract, NativePhloRegionLimits,
};
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
};
use rholang::rust::interpreter::accounting::{
    funding_sig_compound, NativeOperationJournalLimits, NativeOperationTraceLimits,
    NativeRuntimeConfig, RuntimeBudget,
};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::merging::mergeable_tags::default_mergeable_tags;
use rholang::rust::interpreter::rho_runtime::{create_native_replay_env, create_rho_runtime, RhoRuntime};
use rholang::rust::interpreter::spaces::namespace::{self, ReifiedId};
use rspace_plus_plus::rspace::history::history_repository::HistoryRepository;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::Event;
use tokio::sync::RwLock;

/// The deploy signature of every run.
const DEPLOY: &[u8] = b"rrs-native-fail-closed";
/// The native phlo limit of a deploy.
const LIMIT: u64 = 1_000_000;
/// The weights of the compute, introduction, transfer and trace classes, and the price.
const WEIGHTS: [u64; 4] = [0, 1, 1, 0];
const PRICE: u64 = 3;
/// The seed of the deploy's random state.
const SEED: &[u8] = b"rrs-native-fail-closed-random";

/// A deploy that creates a queue space: the factory call is its first space operation.
const SPACE_PROGRAM: &str =
    r#"new factory(`rho:space:hashmap:queue`) in { for (q <- factory!?("default")) { Nil } }"#;

/// A deploy without spaces, the control.
const HOST_PROGRAM: &str = r#"new ch in { ch!(1) | for (@x <- ch) { Nil } }"#;

/// The error of a space operation in native execution, as a deploy reports it.
fn native_execution_error() -> InterpreterError {
    InterpreterError::ReduceError(
        "reified:SpacesUnsupportedInNativeExecution: native execution does not support Reified RSpaces"
            .to_owned(),
    )
}

type RhoHistoryRepository = Arc<
    Box<dyn HistoryRepository<Par, BindPattern, ListParWithRandom, TaggedContinuation> + Send + Sync>,
>;

fn schedule() -> PhloScheduleV1<'static> {
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"native-test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip(WEIGHTS)
            .zip([b"c".as_slice(), b"i", b"t", b"r"])
            .map(|((dimension, weight), name)| dimension.resource_class(name, weight))
            .collect(),
        actual_price: PRICE,
        compatibility_rule: native_resource_compatibility_rule(),
    }
}

/// Runs `action` with the native phlo contract of every run of this file.
fn with_contract<T>(action: impl FnOnce(NativePhloExecutionContract<'_>) -> T) -> T {
    let descriptor = schedule();
    let binding = PhloScheduleBinding::new(&descriptor, PhloGenesisPolicy::LIMITS)
        .expect("the test schedule binds");
    let selected = binding.schedule();
    let schedules = [selected];
    let owner_ceilings = [PRICE];
    let controls = check_phlo_controls(
        selected.environment,
        0,
        u64::MAX,
        SignedPhloControls {
            limit: LIMIT,
            price_ceiling: PRICE,
            required_owner_ceilings: &owner_ceilings,
            permitted_schedules: &schedules,
        },
        selected,
        LIMIT,
    )
    .expect("the test controls check");
    let contract =
        NativePhloExecutionContract::new(controls, &binding).expect("the test contract builds");
    action(contract)
}

fn budget_limits() -> NativeBudgetTraceLimits {
    NativeBudgetTraceLimits {
        attempts: 100_000,
        path_segments: 4096,
        regions: NativePhloRegionLimits {
            regions: 1000,
            encoded_authority_bytes: 10_000_000,
        },
    }
}

fn host_work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
}

fn config(host: HostWorkBudget) -> NativeRuntimeConfig {
    with_contract(|contract| NativeRuntimeConfig::new(contract, budget_limits(), host))
}

fn random() -> Blake2b512Random { Blake2b512Random::create_from_bytes(SEED) }

fn funding() -> rholang::rust::interpreter::accounting::Sig {
    funding_sig_compound(&[&1u64.to_be_bytes()])
}

/// The play of one native deploy.
struct Played {
    result: EvaluateResult,
    log: Vec<Event>,
    history: RhoHistoryRepository,
    /// The channels of the hot store that belong to a native space: every Reified id but the
    /// 2-byte op channels, whose system processes the runtime installs.
    space_channels: usize,
}

/// Plays `term` as a native deploy on a fresh runtime over the empty state. The history stays at
/// the empty root, and the system processes are installed in the hot store only, as the native
/// replay environment installs them again.
async fn play(term: &str) -> Played {
    let mut stores = InMemoryStoreManager::new();
    let (space, _) = RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create_with_replay(
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("an in-memory play and replay space");
    space.create_checkpoint().await.expect("the empty root");
    let history = space.get_history_repository();
    let mut runtime = create_rho_runtime(
        space,
        Arc::new(default_mergeable_tags()),
        false,
        &mut Vec::new(),
        ExternalServices::noop(),
    )
    .await
    .expect("a native play runtime");
    runtime.cost.set_deploy_signature_funded(DEPLOY, funding());
    let result = runtime
        .evaluate_with_native_phlo(term, HashMap::new(), random(), None, config(host_work()))
        .await
        .expect("native play evaluates");
    let log = runtime.create_soft_checkpoint().await.log;
    let space_channels = runtime
        .get_hot_changes()
        .await
        .keys()
        .flatten()
        .filter(|channel| {
            matches!(
                namespace::reified_id_of(channel),
                Some(id) if !matches!(id, ReifiedId::Op(_))
            )
        })
        .count();
    Played {
        result,
        log: log.into_iter().collect(),
        history,
        space_channels,
    }
}

/// Replays the play in the native replay environment, as proposer self-certification and
/// validator replay do, and returns the replay result.
async fn replay(played: &Played, term: &str) -> (EvaluateResult, Result<(), String>, u64) {
    let recording = played
        .result
        .native_budget_recording
        .clone()
        .expect("native play records its budget");
    let operations = played
        .result
        .native_operation_recording
        .clone()
        .expect("native play records its operations");
    let host = host_work();
    let trace = with_contract(|contract| {
        contract.check_operation_journal(
            recording.session,
            &recording,
            operations,
            NativeOperationJournalLimits {
                budget: budget_limits(),
                operations: 100_000,
                total_path_segments: 1_000_000,
                source_entries: 1_000_000,
                footprint_entries: 1_000_000,
                footprint_bytes: 10_000_000,
                predecessor_edges: 1_000_000,
            },
            &host,
        )
    })
    .expect("the play journal checks")
    .bind_trace(
        played.log.clone().into(),
        NativeOperationTraceLimits {
            events: 100_000,
            source_entries: 1_000_000,
            source_bytes: 10_000_000,
            telemetry_items: 100_000,
            telemetry_bytes: 10_000_000,
        },
        &host,
    )
    .expect("the play log binds to the journal");
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(DEPLOY, funding());
    budget
        .reset_for_native_execution(config(host.clone()))
        .expect("the replay budget enters native execution");
    let mut environment = create_native_replay_env(
        trace,
        played.history.clone(),
        Arc::new(Box::new(Matcher)),
        host,
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut Vec::new(),
        budget,
        ExternalServices::noop(),
    )
    .await
    .expect("the native replay environment builds");
    let parsed = Compiler::source_to_adt(term).expect("the program parses");
    let replayed = environment
        .evaluate(parsed, random())
        .await
        .expect("native replay evaluates");
    let complete = environment
        .check_complete()
        .await
        .map_err(|error| error.to_string());
    let usage = environment.completed_usage().await.unwrap_or(u64::MAX);
    (replayed, complete, usage)
}

#[tokio::test]
async fn reified_native_play_rejects_a_space_deploy_before_any_space_work() {
    let played = play(SPACE_PROGRAM).await;
    assert_eq!(played.result.errors, vec![native_execution_error()]);
    assert_eq!(
        played.space_channels, 0,
        "native play staged no record on a channel of a space"
    );
}

#[tokio::test]
async fn reified_native_replay_reproduces_the_rejection_of_play() {
    let played = play(SPACE_PROGRAM).await;
    let (replayed, complete, usage) = replay(&played, SPACE_PROGRAM).await;
    assert_eq!(replayed.errors, played.result.errors);
    assert_eq!(complete, Ok(()), "replay consumed every journal row of play");
    let recording = played
        .result
        .native_budget_recording
        .as_ref()
        .expect("native play records its budget");
    assert_eq!(usage, recording.used, "replay used the phlo of play");
    assert_eq!(replayed.authority_events, played.result.authority_events);
    assert_eq!(
        replayed.byte_observations.rows,
        played.result.byte_observations.rows
    );
}

#[tokio::test]
async fn reified_native_host_program_plays_and_replays_unchanged() {
    let played = play(HOST_PROGRAM).await;
    assert!(played.result.errors.is_empty(), "{:?}", played.result.errors);
    let (replayed, complete, usage) = replay(&played, HOST_PROGRAM).await;
    assert!(replayed.errors.is_empty(), "{:?}", replayed.errors);
    assert_eq!(complete, Ok(()));
    let recording = played
        .result
        .native_budget_recording
        .as_ref()
        .expect("native play records its budget");
    assert_eq!(usage, recording.used);
}
