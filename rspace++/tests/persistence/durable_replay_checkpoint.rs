use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::future::{Future, poll_fn};
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, mpsc};
use std::task::Poll;

use proptest::prelude::*;

use super::durable_publication::directory_snapshot;
use super::durable_rspace_authority::Profile;
use super::durable_rspace_checkpoint::{
    Contents, Observed, Operation, OwnerExit, RacedProduce, Receive, admit, consume,
    durable_authority, exit_taken, expected, finish_under_the_gate_watchdog, first_poll,
    generated_traces, hard_checkpoint, matcher, observe, observe_root, owner_slot, perform,
    produce, published_root, run, seeded_traces,
};
use super::support::{PersistenceTrialDir, trial_scratch};
use crate::rspace::errors::{AuthorityBackendMismatch, HistoryError, RSpaceError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::persistence::durable::journal_owner_hooks::{self, JournalOwnerHookPoint};
use crate::rspace::replay_rspace::ReplayRSpace;
use crate::rspace::rspace::{RSpace, RSpaceStore};
use crate::rspace::rspace_interface::ISpace;
use crate::rspace::state::rspace_state_authority::DurableOwnerSlotV1;
use crate::rspace::trace::Log;
use crate::rspace::trace::event::{COMM, Event, IOEvent};

type Space = RSpace<String, String, String, String>;
type Replay = ReplayRSpace<String, String, String, String>;
type Slot = Arc<DurableOwnerSlotV1<Profile>>;

const GENERATED_TRACES: u32 = 24;
const OVERTAKING_CHANNEL: &str = "overtaking";
const OVERTAKING_VALUE: &str = "published by the play space";
const RACED_CHANNEL: &str = "c";
const RACED_VALUE: &str = "v1";

struct DurablePair {
    play: Space,
    replay: Replay,
    slot: Slot,
    scratch: PersistenceTrialDir,
}

impl DurablePair {
    fn open(prefix: &str) -> Self {
        let scratch = trial_scratch(prefix);
        let authority = durable_authority(scratch.path());
        let (play, replay) =
            Space::create_with_replay(RSpaceStore::new(Arc::clone(&authority)), matcher())
                .expect("RSpace::create_with_replay opens a play and a replay view of the store");
        let slot = owner_slot(&authority);
        Self {
            play,
            replay,
            slot,
            scratch,
        }
    }

    fn path(&self) -> &Path { self.scratch.path() }
}

struct ReplayAnswer {
    root: Blake2b256Hash,
    exit: Option<OwnerExit>,
}

#[derive(Clone, Copy, Debug, Default)]
struct ReplayCoverage {
    traces: u32,
    checkpoints: u32,
    operations: u32,
    operations_left_out: u32,
    comms_by_produce: u32,
    comms_by_consume: u32,
    comms_on_contents_of_an_earlier_checkpoint: u32,
    twin_checkpoints_that_reach_the_published_state: u32,
    publications_from_the_published_state: u32,
    publications_from_a_superseded_base: u32,
    empty_journals_at_the_published_state: u32,
    empty_journals_at_a_superseded_base: u32,
    replays_at_another_exit: u32,
}

impl ReplayCoverage {
    fn merge(&mut self, other: Self) {
        self.traces += other.traces;
        self.checkpoints += other.checkpoints;
        self.operations += other.operations;
        self.operations_left_out += other.operations_left_out;
        self.comms_by_produce += other.comms_by_produce;
        self.comms_by_consume += other.comms_by_consume;
        self.comms_on_contents_of_an_earlier_checkpoint +=
            other.comms_on_contents_of_an_earlier_checkpoint;
        self.twin_checkpoints_that_reach_the_published_state +=
            other.twin_checkpoints_that_reach_the_published_state;
        self.publications_from_the_published_state += other.publications_from_the_published_state;
        self.publications_from_a_superseded_base += other.publications_from_a_superseded_base;
        self.empty_journals_at_the_published_state += other.empty_journals_at_the_published_state;
        self.empty_journals_at_a_superseded_base += other.empty_journals_at_a_superseded_base;
        self.replays_at_another_exit += other.replays_at_another_exit;
    }

    fn count_exit(&mut self, taken: Option<OwnerExit>, implied: OwnerExit) {
        let counter = match (taken == Some(implied), implied) {
            (true, OwnerExit::CrossBaseJournalThatMakesTheBaseCurrent) => {
                &mut self.twin_checkpoints_that_reach_the_published_state
            }
            (true, OwnerExit::SameBasePublication) => {
                &mut self.publications_from_the_published_state
            }
            (true, OwnerExit::CrossBasePublication) => {
                &mut self.publications_from_a_superseded_base
            }
            (true, OwnerExit::SameBaseEmptyJournal) => {
                &mut self.empty_journals_at_the_published_state
            }
            (true, OwnerExit::CrossBaseEmptyJournal) => {
                &mut self.empty_journals_at_a_superseded_base
            }
            (true, OwnerExit::CrossBaseMemoHit) | (false, _) => &mut self.replays_at_another_exit,
        };
        *counter += 1;
    }

    fn assert_complete(&self) {
        let required = [
            ("a trace", self.traces),
            ("a replayed hard checkpoint", self.checkpoints),
            ("a replayed operation", self.operations),
            ("a replayed COMM by a produce", self.comms_by_produce),
            ("a replayed COMM by a consume", self.comms_by_consume),
            (
                "a replayed COMM on contents of an earlier checkpoint",
                self.comms_on_contents_of_an_earlier_checkpoint,
            ),
            (
                "a twin checkpoint that reaches the state its play space published",
                self.twin_checkpoints_that_reach_the_published_state,
            ),
            (
                "a replay publication from the published state",
                self.publications_from_the_published_state,
            ),
            (
                "a replay publication from a superseded base",
                self.publications_from_a_superseded_base,
            ),
            (
                "a replay checkpoint of an empty journal at the published state",
                self.empty_journals_at_the_published_state,
            ),
            (
                "a replay checkpoint of an empty journal at a superseded base",
                self.empty_journals_at_a_superseded_base,
            ),
        ];
        for (case, count) in required {
            assert!(count > 0, "the traces never covered {case}: {self:?}");
        }
    }
}

fn observe_replay(replay: &Replay) -> Observed {
    observe(
        |key| first_poll(replay.get_data(key)),
        |key| first_poll(replay.get_waiting_continuations(key.to_vec())),
        |key| first_poll(replay.get_joins(key.clone())),
    )
}

fn replay_segment(
    pair: &DurablePair,
    log: &Log,
    admitted: &[(Operation, Option<Receive>)],
    contents: &Contents,
) -> Result<ReplayAnswer, TestCaseError> {
    let base = first_poll(pair.replay.get_root());
    let current = published_root(&pair.slot);
    first_poll(pair.replay.rig(log.clone())).expect("a replay space takes the event log of a play");
    for &(operation, fired) in admitted {
        prop_assert_eq!(
            perform(&pair.replay, operation),
            fired,
            "the replay of {:?} had another effect than the play",
            operation
        );
    }
    prop_assert_eq!(
        first_poll(pair.replay.check_replay_data()),
        Ok(()),
        "the replay left a COMM of the event log unused"
    );
    let checkpoint = first_poll(pair.replay.create_checkpoint())
        .expect("a replay that used every COMM of the event log takes its hard checkpoint");
    let published = published_root(&pair.slot);
    prop_assert!(
        checkpoint.log.is_empty(),
        "the hard checkpoint of a replay space returned events: {:?}",
        checkpoint.log
    );
    prop_assert_eq!(
        first_poll(pair.replay.get_root()),
        checkpoint.root.clone(),
        "the repository of the replay space is not at the root that its hard checkpoint returned"
    );
    prop_assert_eq!(
        observe_root(&pair.slot, &checkpoint.root),
        expected(contents),
        "the contents at the root that the replay returned differ from the model"
    );
    prop_assert_eq!(
        observe_replay(&pair.replay),
        expected(contents),
        "the contents of the replay space after its hard checkpoint differ from the model"
    );
    Ok(ReplayAnswer {
        exit: exit_taken(&base, &current, &checkpoint.root, &published),
        root: checkpoint.root,
    })
}

fn overtake_the_replay_space(pair: &DurablePair) -> Result<(), TestCaseError> {
    let fired = first_poll(pair.play.produce(
        OVERTAKING_CHANNEL.to_owned(),
        OVERTAKING_VALUE.to_owned(),
        false,
    ))
    .expect("a produce on a durable store succeeds");
    prop_assert!(fired.is_none(), "the datum of the overtaking play space fired a COMM");
    hard_checkpoint(&pair.play);
    prop_assert_ne!(
        published_root(&pair.slot),
        first_poll(pair.replay.get_root()),
        "the play space of the pair did not supersede the base of its replay space"
    );
    Ok(())
}

fn check_replayed_trace(trace: &[Vec<Operation>]) -> Result<ReplayCoverage, TestCaseError> {
    let played = DurablePair::open("durable-replay-checkpoint-played-");
    let independent = DurablePair::open("durable-replay-checkpoint-independent-");
    let overtaken = DurablePair::open("durable-replay-checkpoint-overtaken-");
    let mut contents = Contents::default();
    let mut previous_root = published_root(&played.slot);
    let mut coverage = ReplayCoverage {
        traces: 1,
        ..ReplayCoverage::default()
    };
    for operations in trace {
        let segment = admit(&mut contents, operations);
        for &(operation, fired) in &segment.admitted {
            prop_assert_eq!(
                perform(&played.play, operation),
                fired,
                "the play of {:?} had another effect than the model",
                operation
            );
        }
        let checkpoint = hard_checkpoint(&played.play);
        let twin = replay_segment(&played, &checkpoint.log, &segment.admitted, &contents)?;
        let alone = replay_segment(&independent, &checkpoint.log, &segment.admitted, &contents)?;
        overtake_the_replay_space(&overtaken)?;
        let behind = replay_segment(&overtaken, &checkpoint.log, &segment.admitted, &contents)?;
        prop_assert_eq!(
            (&twin.root, &alone.root, &behind.root),
            (&checkpoint.root, &checkpoint.root, &checkpoint.root),
            "a durable replay of the event log returned another root than the play"
        );
        let implied = match checkpoint.root == previous_root {
            false => [
                OwnerExit::CrossBaseJournalThatMakesTheBaseCurrent,
                OwnerExit::SameBasePublication,
                OwnerExit::CrossBasePublication,
            ],
            true => [
                OwnerExit::SameBaseEmptyJournal,
                OwnerExit::SameBaseEmptyJournal,
                OwnerExit::CrossBaseEmptyJournal,
            ],
        };
        for (taken, implied) in [twin.exit, alone.exit, behind.exit]
            .into_iter()
            .zip(implied)
        {
            coverage.count_exit(taken, implied);
        }
        coverage.checkpoints += 1;
        coverage.operations +=
            u32::try_from(segment.admitted.len()).expect("the operations of a segment fit u32");
        coverage.operations_left_out += segment.left_out;
        coverage.comms_by_produce += segment.comms_by_produce;
        coverage.comms_by_consume += segment.comms_by_consume;
        coverage.comms_on_contents_of_an_earlier_checkpoint +=
            segment.comms_on_contents_of_an_earlier_checkpoint;
        previous_root = checkpoint.root;
    }
    Ok(coverage)
}

fn recurring_trace() -> Vec<Vec<Operation>> {
    vec![
        vec![produce("a", "v0")],
        vec![consume(&["a"], &["v0"], "k0")],
        vec![produce("a", "v0")],
        vec![produce("b", "v1")],
    ]
}

fn store_without_replay_data(replay: &Replay, contents: &mut Contents, operations: &[Operation]) {
    let segment = admit(contents, operations);
    assert_eq!(segment.left_out, 0, "the operations of a scenario are admissible");
    for &(operation, fired) in &segment.admitted {
        assert_eq!(fired, None, "a scenario without replay data names an operation that fires");
        assert_eq!(
            perform(replay, operation),
            None,
            "a replay space without replay data fired a COMM"
        );
    }
}

fn an_altered_log_that_names_a_disabled_comm_is_refused() {
    let played = DurablePair::open("durable-replay-checkpoint-altered-played-");
    let replayed = DurablePair::open("durable-replay-checkpoint-altered-replayed-");
    let operations = [consume(&["a"], &["v0"], "k0"), produce("a", "v1")];
    let mut contents = Contents::default();
    let segment = admit(&mut contents, &operations);
    assert_eq!(segment.left_out, 0, "the operations of the altered log are admissible");
    for &(operation, fired) in &segment.admitted {
        assert_eq!(fired, None, "the pattern of the stored continuation accepts the stored datum");
        assert_eq!(
            perform(&played.play, operation),
            None,
            "the play fired a COMM that is not enabled"
        );
    }
    let checkpoint = hard_checkpoint(&played.play);
    let [
        Event::IoEvent(IOEvent::Produce(stored_produce)),
        Event::IoEvent(IOEvent::Consume(stored_consume)),
    ] = checkpoint.log.as_slice()
    else {
        panic!(
            "the play logged other events than its produce before its consume: {:?}",
            checkpoint.log
        );
    };
    let mut altered = Vec::with_capacity(checkpoint.log.len() + 1);
    altered.push(Event::Comm(COMM {
        consume: stored_consume.clone(),
        produces: vec![stored_produce.clone()],
        peeks: BTreeSet::new(),
        times_repeated: BTreeMap::from([(stored_produce.clone(), 1)]),
    }));
    altered.extend(checkpoint.log.iter().cloned());

    let start = published_root(&replayed.slot);
    first_poll(replayed.replay.rig(altered)).expect("a replay space takes the altered event log");
    for &(operation, _) in &segment.admitted {
        assert_eq!(
            perform(&replayed.replay, operation),
            None,
            "the replay fired the COMM that is not enabled"
        );
    }
    let files = directory_snapshot(replayed.path());
    assert_eq!(
        first_poll(replayed.replay.create_checkpoint()).err(),
        Some(RSpaceError::BugFoundError(
            "Unused COMM event: replayData multimap has 2 elements left".to_owned()
        )),
        "a replay that left a COMM of its log unused must be refused"
    );
    assert_eq!(
        directory_snapshot(replayed.path()),
        files,
        "a refused replay checkpoint wrote to the store"
    );
    assert_eq!(
        (published_root(&replayed.slot), first_poll(replayed.replay.get_root())),
        (start.clone(), start),
        "a refused replay checkpoint moved the published state or the repository of the replay \
         space"
    );
    assert_eq!(
        observe_replay(&replayed.replay),
        expected(&contents),
        "a refused replay checkpoint changed the contents of the replay space"
    );

    first_poll(replayed.replay.rig(checkpoint.log.clone()))
        .expect("a replay space takes the unaltered event log");
    let accepted = first_poll(replayed.replay.create_checkpoint())
        .expect("the replay of the unaltered event log takes its hard checkpoint");
    assert_eq!(
        (accepted.root.clone(), published_root(&replayed.slot)),
        (checkpoint.root.clone(), checkpoint.root),
        "the replay space did not publish the root of the play after its refused checkpoint"
    );
}

#[test]
fn a_durable_replay_returns_the_plays_root_at_every_checkpoint() {
    let mut seeded = ReplayCoverage::default();
    for trace in seeded_traces() {
        seeded.merge(check_replayed_trace(&trace).expect("a seeded trace satisfies OG-32"));
    }
    println!("durable replay checkpoint seeds: {seeded:?}");
    assert_eq!(
        seeded.operations_left_out, 0,
        "a seeded trace holds a choice or stores a continuation twice"
    );
    assert_eq!(
        seeded.replays_at_another_exit, 0,
        "a replay of a seeded trace left the owner at another exit than the play implies"
    );
    seeded.assert_complete();

    let recurring =
        check_replayed_trace(&recurring_trace()).expect("the recurring trace satisfies OG-32");
    println!("durable replay checkpoint recurring trace: {recurring:?}");
    assert_eq!(recurring.operations_left_out, 0, "the recurring trace leaves out an operation");
    assert!(
        recurring.replays_at_another_exit > 0,
        "the recurring trace never repeated a completed transition: {recurring:?}"
    );

    let generated = Cell::new(ReplayCoverage::default());
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases: GENERATED_TRACES,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
    .run(&generated_traces(), |trace| {
        let mut total = generated.get();
        total.merge(check_replayed_trace(&trace)?);
        generated.set(total);
        Ok(())
    })
    .expect("generated traces satisfy OG-32");
    assert!(generated.get().traces >= GENERATED_TRACES);
    println!("durable replay checkpoint generated traces: {:?}", generated.get());

    an_altered_log_that_names_a_disabled_comm_is_refused();
}

#[derive(Debug, PartialEq)]
struct RacedReplayInstallation {
    exit: Option<OwnerExit>,
    raced: RacedProduce,
    fired: Result<bool, RSpaceError>,
    replay: Observed,
    next_log: Log,
    next_root: Observed,
}

async fn raced_replay_installation(
    ahead: &[&[Operation]],
    journal: &[Operation],
) -> RacedReplayInstallation {
    let pair = DurablePair::open("durable-replay-checkpoint-installation-");
    let base = published_root(&pair.slot);
    let mut ahead_contents = Contents::default();
    for segment in ahead {
        run(&pair.play, &mut ahead_contents, segment);
        hard_checkpoint(&pair.play);
    }
    let current = published_root(&pair.slot);
    let mut contents = Contents::default();
    store_without_replay_data(&pair.replay, &mut contents, journal);
    let race = Rc::new(RefCell::new(None));
    let hook = journal_owner_hooks::arm(JournalOwnerHookPoint::BeforeViewInstallation, {
        let racing_replay = pair.replay.clone();
        let race = Rc::clone(&race);
        move || {
            let (first_poll_sender, first_poll_receiver) = mpsc::channel();
            let finished = tokio::spawn(async move {
                let mut operation = Box::pin(tokio::task::unconstrained(async move {
                    racing_replay
                        .produce(RACED_CHANNEL.to_owned(), RACED_VALUE.to_owned(), false)
                        .await
                        .map(|fired| fired.is_some())
                }));
                let first = poll_fn(|context| Poll::Ready(operation.as_mut().poll(context))).await;
                first_poll_sender
                    .send(first.is_pending())
                    .expect("the checkpoint thread awaits the first poll of the raced produce");
                match first {
                    Poll::Ready(fired) => fired,
                    Poll::Pending => operation.await,
                }
            });
            let raced = match first_poll_receiver
                .recv()
                .expect("a worker polls the raced produce once")
            {
                true => RacedProduce::WaitedForTheGate,
                false => RacedProduce::RanOnTheSupersededStore,
            };
            race.replace(Some((raced, finished)));
        }
    });
    let checkpoint = pair
        .replay
        .create_checkpoint()
        .await
        .expect("the raced replay checkpoint returns the answer of the owner");
    drop(hook);
    let published = published_root(&pair.slot);
    let (raced, finished) = race
        .borrow_mut()
        .take()
        .expect("the replay checkpoint passes the installation hook");
    let fired = finished
        .await
        .expect("the worker finishes the raced produce");
    let replay = observe_replay(&pair.replay);
    let next = first_poll(pair.replay.create_checkpoint())
        .expect("the replay space publishes the datum of the raced produce");
    RacedReplayInstallation {
        exit: exit_taken(&base, &current, &checkpoint.root, &published),
        raced,
        fired,
        replay,
        next_log: next.log,
        next_root: observe_root(&pair.slot, &next.root),
    }
}

fn installed_replay_answer(exit: OwnerExit, journal: &[Operation]) -> RacedReplayInstallation {
    let mut contents = Contents::default();
    let stored = admit(&mut contents, journal);
    let raced = admit(&mut contents, &[produce(RACED_CHANNEL, RACED_VALUE)]);
    assert_eq!(
        stored.left_out + raced.left_out,
        0,
        "the journal of a scenario and the raced produce are admissible"
    );
    RacedReplayInstallation {
        exit: Some(exit),
        raced: RacedProduce::WaitedForTheGate,
        fired: Ok(false),
        replay: expected(&contents),
        next_log: Vec::new(),
        next_root: expected(&contents),
    }
}

async fn a_replay_produce_races_the_installation_at_every_owner_exit() {
    let one_datum = [produce("a", "v0")];
    let second_datum = [produce("b", "v1")];
    let two_data = [produce("a", "v0"), produce("b", "v1")];
    let other_datum = [produce("b", "v0")];
    let scenarios: [(OwnerExit, &[&[Operation]], &[Operation]); 6] = [
        (OwnerExit::SameBaseEmptyJournal, &[], &[]),
        (OwnerExit::SameBasePublication, &[], &one_datum),
        (OwnerExit::CrossBaseEmptyJournal, &[&one_datum], &[]),
        (
            OwnerExit::CrossBaseJournalThatMakesTheBaseCurrent,
            &[&one_datum, &second_datum],
            &two_data,
        ),
        (OwnerExit::CrossBaseMemoHit, &[&one_datum, &second_datum], &one_datum),
        (OwnerExit::CrossBasePublication, &[&one_datum], &other_datum),
    ];
    let mut observed = Vec::with_capacity(scenarios.len());
    let mut installed = Vec::with_capacity(scenarios.len());
    for (exit, ahead, journal) in scenarios {
        observed.push(raced_replay_installation(ahead, journal).await);
        installed.push(installed_replay_answer(exit, journal));
    }
    assert_eq!(
        observed, installed,
        "a produce that arrives while a replay checkpoint installs the answer of the owner must \
         wait for the view gate at every owner exit, run on the installed store and be published \
         by the next checkpoint"
    );
}

#[test]
fn a_concurrent_replay_operation_waits_until_every_owner_exit_is_installed() {
    finish_under_the_gate_watchdog(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .build()
            .expect("build a two-worker runtime")
            .block_on(a_replay_produce_races_the_installation_at_every_owner_exit());
    });
}

#[test]
fn create_with_replay_builds_a_durable_pair_and_both_spaces_checkpoint() {
    let pair = DurablePair::open("durable-replay-checkpoint-pair-");
    let genesis = published_root(&pair.slot);
    assert_eq!(
        (first_poll(pair.play.get_root()), first_poll(pair.replay.get_root())),
        (genesis.clone(), genesis.clone()),
        "the two spaces of a durable pair do not start at the published state"
    );
    let segments: [&[Operation]; 2] = [
        &[produce("a", "v0"), consume(&["b", "c"], &["v0", "v1"], "k0")],
        &[produce("b", "v0"), produce("c", "v1")],
    ];
    let mut contents = Contents::default();
    let mut roots = Vec::with_capacity(segments.len() + 1);
    roots.push(genesis);
    for operations in segments {
        let segment = admit(&mut contents, operations);
        assert_eq!(segment.left_out, 0, "the segments of the pair are admissible");
        for &(operation, fired) in &segment.admitted {
            assert_eq!(
                perform(&pair.play, operation),
                fired,
                "the play of {operation:?} had another effect than the model"
            );
        }
        let played = hard_checkpoint(&pair.play);
        assert_eq!(
            published_root(&pair.slot),
            played.root,
            "the hard checkpoint of the play space did not publish its root"
        );
        let files = directory_snapshot(pair.path());
        let replayed = replay_segment(&pair, &played.log, &segment.admitted, &contents)
            .expect("the replay space of the pair replays the event log of its play space");
        assert_eq!(
            replayed.root, played.root,
            "the replay space of a pair returned another root than its play space"
        );
        assert_eq!(
            replayed.exit,
            Some(OwnerExit::CrossBaseJournalThatMakesTheBaseCurrent),
            "the replay space of a pair did not reach the state that its play space published"
        );
        assert_eq!(
            (published_root(&pair.slot), directory_snapshot(pair.path())),
            (played.root.clone(), files),
            "a replay checkpoint that repeats a publication moved the published state or wrote to \
             the store"
        );
        assert!(
            !roots.contains(&played.root),
            "a segment of the pair returned the root of an earlier state"
        );
        roots.push(played.root);
    }
}

#[test]
fn a_refused_durable_replay_checkpoint_is_a_typed_error_and_changes_nothing() {
    let pair = DurablePair::open("durable-replay-checkpoint-refusal-");
    let journal = [produce("a", "v0"), consume(&["b", "c"], &["v0", "v1"], "k0")];
    let mut contents = Contents::default();
    store_without_replay_data(&pair.replay, &mut contents, &journal);
    let files = directory_snapshot(pair.path());
    let root = published_root(&pair.slot);
    let mut owner = pair
        .slot
        .owner()
        .try_lock()
        .expect("the owner mutex is free");
    owner
        .close()
        .expect("an owner without a pending publication closes");
    drop(owner);

    assert_eq!(
        first_poll(pair.replay.create_checkpoint()).err(),
        Some(RSpaceError::HistoryError(HistoryError::ActionError(
            "the durable owner refused the view checkpoint: Closed".to_owned()
        ))),
        "a closed owner refuses the hard checkpoint of the replay space with a typed error"
    );
    assert_eq!(
        directory_snapshot(pair.path()),
        files,
        "a refused replay checkpoint wrote to the store"
    );
    assert_eq!(
        (published_root(&pair.slot), first_poll(pair.replay.get_root())),
        (root.clone(), root),
        "a refused replay checkpoint moved the published state or the repository of the replay \
         space"
    );
    assert_eq!(
        observe_replay(&pair.replay),
        expected(&contents),
        "a refused replay checkpoint changed the contents of the replay space"
    );

    let detached_scratch = trial_scratch("durable-replay-checkpoint-detached-");
    let (repository, hot_store) =
        Space::create_history_repo(RSpaceStore::new(durable_authority(detached_scratch.path())))
            .expect("create_history_repo opens the durable store");
    let detached = Replay::apply(Arc::new(repository), Arc::new(hot_store), matcher());
    assert_eq!(perform(&detached, produce("a", "v0")), None);
    assert_eq!(
        first_poll(detached.create_checkpoint()).err(),
        Some(RSpaceError::HistoryError(HistoryError::BackendMismatch(
            AuthorityBackendMismatch::History
        ))),
        "a durable replay space that RSpace::create_with_replay did not build keeps the typed \
         backend refusal"
    );
}
