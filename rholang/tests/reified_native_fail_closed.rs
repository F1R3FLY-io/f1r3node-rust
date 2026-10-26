// Spaces in native execution (decision 47).
//
// Since the merge of feature/cost-accounted-rho, `create_native_replay_env` is production code:
// proposer self-certification and validator replay run every offered deploy in it. Decision 46
// made spaces fail closed there, and this file tested that rule. Decision 47 runs spaces in
// native execution: native play stages and commits them as legacy play does, and native replay
// replays each staged operation against its journal row through the native backend of the
// session. So a space deploy plays natively, and native replay reproduces it: no error, a
// complete journal, the phlo usage of play, the same authority events and byte observations,
// and the post-state root of play. A program without spaces plays and replays natively as
// before.

use std::collections::HashMap;
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativeBoundSource, NativeBudgetTraceLimits,
    NativePhloDimension, NativePhloExecutionContract, NativePhloRegionLimits,
};
use rholang::rust::interpreter::accounting::phlo_controls::{
    check_phlo_controls, PhloScheduleBinding, SignedPhloControls,
};
use rholang::rust::interpreter::accounting::{
    funding_sig_compound, NativeOperationJournalLimits, NativeOperationTraceLimits,
    NativeRuntimeConfig, RuntimeBudget,
};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::external_services::ExternalServices;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rholang::rust::interpreter::merging::mergeable_tags::default_mergeable_tags;
use rholang::rust::interpreter::rho_runtime::{
    create_native_replay_env, create_rho_runtime, RhoRuntime,
};
use rholang::rust::interpreter::spaces::namespace::{self, ReifiedId};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
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

// Disabled by decision 47: native execution runs spaces, so no space operation fails with this
// error.
//
// ```
// /// The error of a space operation in native execution, as a deploy reports it.
// fn native_execution_error() -> InterpreterError {
//     InterpreterError::ReduceError(
//         "reified:SpacesUnsupportedInNativeExecution: native execution does not support Reified RSpaces"
//             .to_owned(),
//     )
// }
// ```

type RhoHistoryRepository = Arc<
    Box<
        dyn HistoryRepository<Par, BindPattern, ListParWithRandom, TaggedContinuation>
            + Send
            + Sync,
    >,
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
        NativePhloExecutionContract::new(controls, &binding, NativeBoundSource::Certificate)
            .expect("the test contract builds");
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

fn config() -> NativeRuntimeConfig {
    with_contract(|contract| NativeRuntimeConfig::new(contract, budget_limits()))
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
    /// The kinds of the channels of the hot store that belong to a native space: every Reified
    /// id but the 2-byte op channels, whose system processes the runtime installs. The hot
    /// changes (`HotStore::to_map`) list only the channels with a data entry, so a channel that
    /// holds only a continuation is absent.
    space_channels: Vec<&'static str>,
    /// The post-state root of play.
    root: Blake2b256Hash,
}

/// Plays `term` as a native deploy on a fresh runtime over the empty state. The history stays at
/// the empty root, and the system processes are installed in the hot store only, as the native
/// replay environment installs them again.
async fn play(term: &str) -> Played {
    let mut stores = InMemoryStoreManager::new();
    let (space, _) =
        RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create_with_replay(
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
        .evaluate_with_native_phlo(term, HashMap::new(), random(), None, config())
        .await
        .expect("native play evaluates");
    let log = runtime.create_soft_checkpoint().await.log;
    let mut space_channels: Vec<&'static str> = runtime
        .get_hot_changes()
        .await
        .keys()
        .flatten()
        .filter_map(|channel| match namespace::reified_id_of(channel)? {
            ReifiedId::Op(_) => None,
            ReifiedId::Config(_) => Some("C"),
            ReifiedId::SpaceCounter(_) => Some("S"),
            ReifiedId::CollectionCounter(..) => Some("S(k)"),
            ReifiedId::Data(..) => Some("D"),
            ReifiedId::PathIndex(..) => Some("I"),
            ReifiedId::Allocator(_) => Some("A"),
            ReifiedId::Handle(_) => Some("H"),
            ReifiedId::Name(..) => Some("N"),
            ReifiedId::Matcher(..) => Some("X"),
            ReifiedId::Body(..) => Some("B"),
            ReifiedId::Reserved(..) => Some("reserved"),
        })
        .collect();
    space_channels.sort_unstable();
    let root = runtime.create_checkpoint().await.root;
    Played {
        result,
        log: log.into_iter().collect(),
        history,
        space_channels,
        root,
    }
}

/// Replays the play in the native replay environment, as proposer self-certification and
/// validator replay do. Returns the replay result, the check of a complete journal, the
/// completed phlo usage and the exported post-state root.
async fn replay(
    played: &Played,
    term: &str,
) -> (EvaluateResult, Result<(), String>, u64, Blake2b256Hash) {
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
        )
    })
    .expect("the play journal checks")
    .bind_trace(played.log.clone().into(), NativeOperationTraceLimits {
        events: 100_000,
        source_entries: 1_000_000,
        source_bytes: 10_000_000,
        telemetry_items: 100_000,
        telemetry_bytes: 10_000_000,
    })
    .expect("the play log binds to the journal");
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(DEPLOY, funding());
    budget
        .reset_for_native_execution(config())
        .expect("the replay budget enters native execution");
    let mut environment = create_native_replay_env(
        trace,
        played.history.clone(),
        Arc::new(Box::new(Matcher)),
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
    let root = environment
        .export()
        .await
        .expect("the native replay environment exports its state")
        .root()
        .clone();
    (replayed, complete, usage, root)
}

// Disabled by decision 47: native play runs the factory call of a space deploy, so it rejects
// nothing. Its successor is `reified_native_play_runs_a_space_deploy`.
//
// ```
// #[tokio::test]
// async fn reified_native_play_rejects_a_space_deploy_before_any_space_work() {
//     let played = play(SPACE_PROGRAM).await;
//     assert_eq!(played.result.errors, vec![native_execution_error()]);
//     assert_eq!(
//         played.space_channels, 0,
//         "native play staged no record on a channel of a space"
//     );
// }
// ```

/// Decision 47: native play runs the factory call. It stages the config record C(sid), the space
/// counter S(sid) and the handle continuation on H(sid). The hot changes list the two channels
/// with a record. H(sid) holds only the continuation, which the replay test checks through the
/// complete journal and the post-state root.
#[tokio::test]
async fn reified_native_play_runs_a_space_deploy() {
    let played = play(SPACE_PROGRAM).await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    assert_eq!(
        played.space_channels,
        vec!["C", "S"],
        "native play stored the records of C(sid) and S(sid)"
    );
}

// Disabled by decision 47: native replay no longer reproduces a rejection, because play rejects
// nothing. Its successor is `reified_native_replay_reproduces_a_space_deploy`.
//
// ```
// #[tokio::test]
// async fn reified_native_replay_reproduces_the_rejection_of_play() {
//     let played = play(SPACE_PROGRAM).await;
//     let (replayed, complete, usage) = replay(&played, SPACE_PROGRAM).await;
//     assert_eq!(replayed.errors, played.result.errors);
//     assert_eq!(
//         complete,
//         Ok(()),
//         "replay consumed every journal row of play"
//     );
//     let recording = played
//         .result
//         .native_budget_recording
//         .as_ref()
//         .expect("native play records its budget");
//     assert_eq!(usage, recording.used, "replay used the phlo of play");
//     assert_eq!(replayed.authority_events, played.result.authority_events);
//     assert_eq!(
//         replayed.byte_observations.rows,
//         played.result.byte_observations.rows
//     );
// }
// ```

/// Decision 47: native replay replays each staged operation of the factory against its journal
/// row. It reproduces play: no error, a complete journal, the phlo usage of play, the same
/// authority events and byte observations, and the post-state root of play.
#[tokio::test]
async fn reified_native_replay_reproduces_a_space_deploy() {
    let played = play(SPACE_PROGRAM).await;
    let (replayed, complete, usage, root) = replay(&played, SPACE_PROGRAM).await;
    assert!(replayed.errors.is_empty(), "{:?}", replayed.errors);
    assert_eq!(replayed.errors, played.result.errors);
    assert_eq!(
        complete,
        Ok(()),
        "replay consumed every journal row of play"
    );
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
    assert_eq!(
        root, played.root,
        "replay reached the post-state root of play"
    );
}

#[tokio::test]
async fn reified_native_host_program_plays_and_replays_unchanged() {
    let played = play(HOST_PROGRAM).await;
    assert!(
        played.result.errors.is_empty(),
        "{:?}",
        played.result.errors
    );
    let (replayed, complete, usage, root) = replay(&played, HOST_PROGRAM).await;
    assert!(replayed.errors.is_empty(), "{:?}", replayed.errors);
    assert_eq!(complete, Ok(()));
    assert_eq!(root, played.root);
    let recording = played
        .result
        .native_budget_recording
        .as_ref()
        .expect("native play records its budget");
    assert_eq!(usage, recording.used);
}
