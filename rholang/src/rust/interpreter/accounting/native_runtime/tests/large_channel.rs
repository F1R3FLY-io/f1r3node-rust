//! Added for bugs 11451 to 11453 (DR-131): native play and native replay of
//! one deploy that touches a host channel with many stored records. Before
//! DR-131, replay charged the cold read of the whole channel, a candidate
//! selection that grew with the square of the records, and a scan of every
//! parked receive. Replay then rejected deploys that play accepted, so the
//! producer's own replay kept them out of every block.

use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use tokio::sync::RwLock;

use super::*;
use crate::rust::interpreter::compiler::compiler::Compiler;
use crate::rust::interpreter::external_services::ExternalServices;
use crate::rust::interpreter::matcher::r#match::Matcher;
use crate::rust::interpreter::merging::mergeable_tags::default_mergeable_tags;
use crate::rust::interpreter::rho_runtime::{create_native_replay_env, create_rho_env};

const LIMIT: u64 = 1_000_000_000;
const WEIGHTS: [u64; 4] = [2, 3, 5, 7];
const SIGNATURE_SEED: &[u8] = b"large-channel";
const FILL_CHUNK: usize = 1_000;
const SMALL: usize = 10;

const TRACE_LIMITS: NativeOperationTraceLimits = NativeOperationTraceLimits {
    events: 100_000,
    source_entries: 1_000_000,
    source_bytes: 100_000_000,
    telemetry_items: 100_000,
    telemetry_bytes: 10_000_000,
};

#[derive(Clone, Copy)]
enum Shape {
    /// `@"h"!(-1)` onto `n` stored records.
    Send,
    /// `for (_ <- @"h") { Nil }` from `n` stored records.
    Receive,
    /// `@"w"!(-1)`, which fires one of `n` parked receives.
    Fire,
}

impl Shape {
    fn fill_term(self, index: usize) -> String {
        match self {
            Shape::Fire => format!("for (@x <- @\"w\") {{ @\"o\"!([x, {index}]) }}"),
            Shape::Send | Shape::Receive => format!("@\"h\"!({index})"),
        }
    }

    fn term(self) -> &'static str {
        match self {
            Shape::Send => "@\"h\"!(-1)",
            Shape::Receive => "for (_ <- @\"h\") { Nil }",
            Shape::Fire => "@\"w\"!(-1)",
        }
    }
}

struct Played {
    recording: NativeBudgetRecording,
    operations: Arc<[NativeOperationRecord]>,
    log: Vec<rspace_plus_plus::rspace::trace::event::Event>,
}

/// One native play of `term` on `play`, then its checkpoint.
async fn play_native(
    play: &RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>,
    term: &str,
    seed: &[u8],
) -> Played {
    let parsed = Compiler::source_to_adt(term).expect("the term parses");
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget.set_deploy_signature_funded(SIGNATURE_SEED, Sig::Ground(vec![7]));
    // Changed by the merge of feature/cost-accounted-rho (finding R1-33): the
    // budget binds the codec epoch of the play space.
    // let settings = config(LIMIT, WEIGHTS);
    let settings = native_config(LIMIT, WEIGHTS);
    budget
        .reset_for_native_execution(settings)
        .expect("the play budget resets");
    let (reducer, _, _, _) = create_rho_env(
        play.clone(),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut Vec::new(),
        budget.clone(),
        ExternalServices::noop(),
    )
    .await
    .expect("the play environment starts");
    let scope = budget.enter_comm_accounting_scope();
    let played = reducer
        .inj_with_observation(parsed, Blake2b512Random::create_from_bytes(seed))
        .await;
    drop(scope);
    played.0.expect("the native play succeeds");
    let recording = budget
        .native_budget_recording()
        .expect("the play recording reads")
        .expect("the play has a budget recording");
    let operations = budget
        .native_operation_recording()
        .expect("the play journal reads")
        .expect("the play has an operation journal");
    let log = play.create_soft_checkpoint().await.log;
    play.create_checkpoint().await.expect("the post-state");
    Played {
        recording,
        operations,
        log: log.into_iter().collect(),
    }
}

/// The phlo that play recorded and the phlo that replay used.
#[derive(Debug, PartialEq, Eq)]
struct Phlo {
    play: u64,
    replay: u64,
}

/// Fills a fresh space with `n` records of `shape`, then plays the deploy of
/// `shape` and replays it from the root after the fill. The result is the
/// phlo of play and of replay, or the error of the replay.
async fn passes(shape: Shape, n: usize) -> Result<Phlo, String> {
    let mut stores = InMemoryStoreManager::new();
    // Changed by the merge of feature/cost-accounted-rho (finding R1-33): the
    // play space has the codec epoch of the native harnesses.
    // let (play, _) = RSpace::create_with_replay(
    //     stores.r_space_stores().await.expect("in-memory stores"),
    //     Arc::new(Box::new(Matcher)),
    // )
    // .expect("a play space");
    let (play, _) = RSpace::create_with_replay_for_epoch(
        NATIVE_EPOCH,
        stores.r_space_stores().await.expect("in-memory stores"),
        Arc::new(Box::new(Matcher)),
    )
    .expect("a play space");
    play.create_checkpoint().await.expect("the pre-state");
    let mut stored = 0;
    while stored < n {
        let count = FILL_CHUNK.min(n - stored);
        let term = (stored..stored + count)
            .map(|index| shape.fill_term(index))
            .collect::<Vec<_>>()
            .join(" | ");
        play_native(&play, &term, format!("fill-{n}-{stored}").as_bytes()).await;
        stored += count;
    }
    let history = play.get_history_repository();
    let seed = format!("measured-{n}");
    let played = play_native(&play, shape.term(), seed.as_bytes()).await;
    let play_phlo = played.recording.used;
    let trace = with_contract(LIMIT, WEIGHTS, |contract| {
        contract.check_operation_journal(
            played.recording.session,
            &played.recording,
            Arc::clone(&played.operations),
            journal_limits(),
        )
    })
    .expect("the journal checks")
    .bind_trace(played.log.clone().into(), TRACE_LIMITS)
    .expect("the trace binds");
    let replay_budget = RuntimeBudget::new(Cost::unsafe_max());
    replay_budget.set_deploy_signature_funded(SIGNATURE_SEED, Sig::Ground(vec![7]));
    // Changed by the merge of feature/cost-accounted-rho (finding R1-33): the
    // replay budget binds the codec epoch of the play space.
    // let replay_settings = config(LIMIT, WEIGHTS);
    let replay_settings = native_config(LIMIT, WEIGHTS);
    replay_budget
        .reset_for_native_execution(replay_settings)
        .expect("the replay budget resets");
    let parsed = Compiler::source_to_adt(shape.term()).expect("the term parses");
    // Changed by the merge of feature/cost-accounted-rho (finding R1-11):
    // F1R3Lang's native replay takes the codec epoch of its budget first.
    match create_native_replay_env(
        // Changed by the merge of feature/cost-accounted-rho (finding R1-33):
        // native replay has the codec epoch of the play space.
        // EventCodecEpoch::Legacy,
        NATIVE_EPOCH,
        trace,
        history,
        Arc::new(Box::new(Matcher)),
        Arc::new(RwLock::new(HashMap::new())),
        Arc::new(default_mergeable_tags()),
        &mut Vec::new(),
        replay_budget,
        ExternalServices::noop(),
    )
    .await
    {
        Err(error) => Err(format!("replay environment: {error}")),
        Ok(mut environment) => {
            match environment
                .evaluate(parsed, Blake2b512Random::create_from_bytes(seed.as_bytes()))
                .await
            {
                Ok(replayed) if replayed.errors.is_empty() => environment
                    .completed_usage()
                    .await
                    .map(|replay| Phlo {
                        play: play_phlo,
                        replay,
                    })
                    .map_err(|error| format!("replay usage: {error}")),
                Ok(replayed) => Err(format!("replay errors {:?}", replayed.errors)),
                Err(error) => Err(format!("replay failed: {error}")),
            }
        }
    }
}

/// Changed by DR-131: replay accepts the deploy on channels of `SMALL` and
/// `large` records, and replay uses the phlo that play recorded. Play phlo is
/// above zero, so the equality is not vacuous. The comparison of the
/// host-work growth of play and replay went with host work.
async fn assert_replay_follows_play(shape: Shape, large: usize) {
    for records in [SMALL, large] {
        let phlo = passes(shape, records)
            .await
            .unwrap_or_else(|error| panic!("replay accepts {records} records: {error}"));
        assert!(phlo.play > 0, "play charges phlo on {records} records");
        assert_eq!(
            phlo.replay, phlo.play,
            "replay uses the phlo of play on {records} records"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_send_onto_a_large_channel_replays_like_its_play() {
    assert_replay_follows_play(Shape::Send, 10_000).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_receive_from_a_large_channel_replays_like_its_play() {
    assert_replay_follows_play(Shape::Receive, 1_400).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_send_that_fires_one_of_many_parked_receives_replays_like_its_play() {
    assert_replay_follows_play(Shape::Fire, 4_000).await;
}
