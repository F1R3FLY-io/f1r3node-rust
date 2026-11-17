use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::future::{Future, poll_fn};
use std::path::Path;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use proptest::prelude::*;
use tokio::sync::RwLock;

use super::durable_backend_selection::manager_over;
use super::durable_publication::directory_snapshot;
use super::durable_rspace_authority::Profile;
use super::owner_cross_base_checkpoint::view_limits;
use super::support::{PersistenceTrialDir, trial_scratch};
use crate::rspace::checkpoint::Checkpoint;
use crate::rspace::errors::{
    AuthorityBackendMismatch, HistoryError, HistoryRepositoryError, RSpaceError,
};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::r#match::Match;
use crate::rspace::persistence::durable::journal_owner_hooks::{self, JournalOwnerHookPoint};
use crate::rspace::persistence::durable::{
    DurableAdmissionErrorV1, DurableIoError, DurableStartupErrorV1,
};
use crate::rspace::persistence::{DurableStateRootV1, DurableViewReaderV1};
use crate::rspace::replay_rspace::ReplayRSpace;
use crate::rspace::rspace::{RSpace, RSpaceStore};
use crate::rspace::rspace_interface::{ContResult, ISpace, RSpaceResult};
use crate::rspace::shared::key_value_store_manager::{
    KeyValueStoreManager, RSpaceBackendSelection,
};
use crate::rspace::state::rspace_state_authority::{
    AuthorityRouteV1, DurableBackendErrorV1, DurableOwnerSlotV1, RSpaceStateAuthority,
};
use crate::rspace::trace::Log;
use crate::rspace::trace::event::{Event, IOEvent, Produce};

type Space = RSpace<String, String, String, String>;
type Replay = ReplayRSpace<String, String, String, String>;
type Slot = Arc<DurableOwnerSlotV1<Profile>>;
type Patterns = [usize; LONGEST_GROUP];
type ParkedProduce = Pin<Box<dyn Future<Output = Result<bool, RSpaceError>>>>;

const CHANNELS: [&str; 3] = ["a", "b", "c"];
const VALUES: [&str; 2] = ["v0", "v1"];
const BODIES: [&str; 2] = ["k0", "k1"];
const GROUPS: [&[usize]; 5] = [&[0], &[2], &[0, 1], &[1, 0], &[1, 2]];
const UNUSED_ORDERS: [&[usize]; 3] = [&[1], &[2, 1], &[0, 2]];
const LONGEST_GROUP: usize = 2;
const PAIR_IN_ORDER: usize = 2;
const PAIR_REVERSED: usize = 3;
const INSTALLED_CHANNEL: &str = "i";
const INSTALLED_BODY: &str = "installed";
const GENERATED_TRACES: u32 = 24;
const MAX_SEGMENTS: usize = 4;
const MAX_OPERATIONS: usize = 6;
const UNCONTENDED: &str = "an operation of an uncontended durable view completes on its first poll";
const GATE_WATCHDOG: &str =
    "a durable hard checkpoint under its held view gate did not complete within 30 s";
const RACED_CHANNEL: &str = "c";
const RACED_VALUE: &str = "v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Receive {
    group: usize,
    patterns: Patterns,
    body: usize,
}

impl Receive {
    fn new(group: usize, mut patterns: Patterns, body: usize) -> Self {
        patterns[GROUPS[group].len()..].fill(0);
        Self {
            group,
            patterns,
            body,
        }
    }

    fn needs(self) -> impl Iterator<Item = (usize, usize)> {
        GROUPS[self.group].iter().copied().zip(self.patterns)
    }

    fn channels(self) -> Vec<String> { channels_of(GROUPS[self.group]) }

    fn pattern_values(self) -> Vec<String> {
        self.needs()
            .map(|(_, pattern)| VALUES[pattern].to_owned())
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Produce { channel: usize, value: usize },
    Consume(Receive),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refusal {
    Choice,
    StoredTwice,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Contents {
    data: [[u64; VALUES.len()]; CHANNELS.len()],
    receives: BTreeSet<Receive>,
}

impl Contents {
    fn joins(&self) -> BTreeSet<(usize, usize)> {
        self.receives
            .iter()
            .flat_map(|receive| {
                GROUPS[receive.group]
                    .iter()
                    .map(move |&member| (member, receive.group))
            })
            .collect()
    }

    fn enables(&self, receive: Receive, channel: usize, value: usize) -> bool {
        GROUPS[receive.group].contains(&channel) &&
            receive.needs().all(|(member, pattern)| {
                if member == channel {
                    pattern == value
                } else {
                    self.data[member][pattern] > 0
                }
            })
    }

    fn apply(&mut self, operation: Operation) -> Result<Option<Receive>, Refusal> {
        match operation {
            Operation::Produce { channel, value } => {
                let mut enabled = self
                    .receives
                    .iter()
                    .copied()
                    .filter(|&receive| self.enables(receive, channel, value));
                match (enabled.next(), enabled.next()) {
                    (None, _) => {
                        self.data[channel][value] += 1;
                        Ok(None)
                    }
                    (Some(fired), None) => {
                        self.receives.remove(&fired);
                        for (member, pattern) in fired.needs() {
                            if member != channel {
                                self.data[member][pattern] -= 1;
                            }
                        }
                        Ok(Some(fired))
                    }
                    (Some(_), Some(_)) => Err(Refusal::Choice),
                }
            }
            Operation::Consume(receive) => {
                if receive
                    .needs()
                    .all(|(member, pattern)| self.data[member][pattern] > 0)
                {
                    for (member, pattern) in receive.needs() {
                        self.data[member][pattern] -= 1;
                    }
                    Ok(Some(receive))
                } else if self.receives.insert(receive) {
                    Ok(None)
                } else {
                    Err(Refusal::StoredTwice)
                }
            }
        }
    }

    fn take_stored_here(&mut self, needs: impl Iterator<Item = (usize, usize)>) -> bool {
        let mut stored_earlier = false;
        for (member, pattern) in needs {
            if self.data[member][pattern] > 0 {
                self.data[member][pattern] -= 1;
            } else {
                stored_earlier = true;
            }
        }
        stored_earlier
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Observed {
    contents: Contents,
    joins: BTreeSet<(usize, usize)>,
}

#[derive(Default)]
struct Segment {
    admitted: Vec<(Operation, Option<Receive>)>,
    left_out: u32,
    comms_by_produce: u32,
    comms_by_consume: u32,
    comms_on_contents_of_an_earlier_checkpoint: u32,
}

#[derive(Clone, Copy, Debug, Default)]
struct Coverage {
    traces: u32,
    checkpoints: u32,
    operations: u32,
    operations_left_out: u32,
    comms_by_produce: u32,
    comms_by_consume: u32,
    comms_on_contents_of_an_earlier_checkpoint: u32,
    checkpoints_holding_both_orders_of_a_channel_pair: u32,
    checkpoints_of_an_empty_journal: u32,
    checkpoints_of_a_journal_without_effect: u32,
    segments_run_in_another_order: u32,
    replayed_comms: u32,
}

impl Coverage {
    fn merge(&mut self, other: Self) {
        self.traces += other.traces;
        self.checkpoints += other.checkpoints;
        self.operations += other.operations;
        self.operations_left_out += other.operations_left_out;
        self.comms_by_produce += other.comms_by_produce;
        self.comms_by_consume += other.comms_by_consume;
        self.comms_on_contents_of_an_earlier_checkpoint +=
            other.comms_on_contents_of_an_earlier_checkpoint;
        self.checkpoints_holding_both_orders_of_a_channel_pair +=
            other.checkpoints_holding_both_orders_of_a_channel_pair;
        self.checkpoints_of_an_empty_journal += other.checkpoints_of_an_empty_journal;
        self.checkpoints_of_a_journal_without_effect +=
            other.checkpoints_of_a_journal_without_effect;
        self.segments_run_in_another_order += other.segments_run_in_another_order;
        self.replayed_comms += other.replayed_comms;
    }

    fn assert_complete(&self) {
        let required = [
            ("a trace", self.traces),
            ("a hard checkpoint", self.checkpoints),
            ("an operation", self.operations),
            ("a COMM by a produce", self.comms_by_produce),
            ("a COMM by a consume", self.comms_by_consume),
            (
                "a COMM on contents of an earlier checkpoint",
                self.comms_on_contents_of_an_earlier_checkpoint,
            ),
            (
                "a checkpoint that holds both orders of one channel pair",
                self.checkpoints_holding_both_orders_of_a_channel_pair,
            ),
            ("a checkpoint of an empty journal", self.checkpoints_of_an_empty_journal),
            (
                "a checkpoint of a journal without effect",
                self.checkpoints_of_a_journal_without_effect,
            ),
            ("a segment run in another order", self.segments_run_in_another_order),
            ("a replayed COMM", self.replayed_comms),
        ];
        for (case, count) in required {
            assert!(count > 0, "the traces never covered {case}: {self:?}");
        }
    }
}

struct EqualStrings;

impl Match<String, String, String> for EqualStrings {
    fn get(&self, pattern: &String, datum: &String) -> Option<String> {
        (pattern == datum).then(|| datum.clone())
    }
}

pub(super) struct DurableStore {
    pub(super) view: Space,
    pub(super) slot: Slot,
    pub(super) authority: Arc<RSpaceStateAuthority>,
    scratch: PersistenceTrialDir,
}

impl DurableStore {
    pub(super) fn open(prefix: &str) -> Self {
        let scratch = trial_scratch(prefix);
        let authority = durable_authority(scratch.path());
        let view = open_view(&authority);
        let slot = owner_slot(&authority);
        Self {
            view,
            slot,
            authority,
            scratch,
        }
    }

    pub(super) fn path(&self) -> &Path { self.scratch.path() }
}

fn position_of(names: &[&str], name: &str) -> usize {
    names
        .iter()
        .position(|known| *known == name)
        .expect("a name belongs to the test universe")
}

fn channels_of(members: &[usize]) -> Vec<String> {
    members
        .iter()
        .map(|&member| CHANNELS[member].to_owned())
        .collect()
}

fn group_of<T: AsRef<str>>(channels: &[T]) -> usize {
    GROUPS
        .iter()
        .position(|members| {
            members.len() == channels.len() &&
                members
                    .iter()
                    .zip(channels)
                    .all(|(&member, named)| CHANNELS[member] == named.as_ref())
        })
        .expect("a channel group belongs to the test universe")
}

fn patterns_of<T: AsRef<str>>(values: &[T]) -> Patterns {
    let mut patterns = [0; LONGEST_GROUP];
    for (position, named) in values.iter().enumerate() {
        patterns[position] = position_of(&VALUES, named.as_ref());
    }
    patterns
}

pub(super) fn produce(channel_name: &str, value_name: &str) -> Operation {
    Operation::Produce {
        channel: position_of(&CHANNELS, channel_name),
        value: position_of(&VALUES, value_name),
    }
}

pub fn consume(channel_names: &[&str], pattern_names: &[&str], body_name: &str) -> Operation {
    assert_eq!(channel_names.len(), pattern_names.len(), "a receive names one pattern per channel");
    Operation::Consume(Receive::new(
        group_of(channel_names),
        patterns_of(pattern_names),
        position_of(&BODIES, body_name),
    ))
}

fn matcher() -> Arc<Box<dyn Match<String, String, String>>> { Arc::new(Box::new(EqualStrings)) }

pub(super) fn first_poll<T>(operation: impl Future<Output = T>) -> T {
    let mut operation = std::pin::pin!(tokio::task::unconstrained(operation));
    match operation
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("{UNCONTENDED}"),
    }
}

pub(super) fn durable_authority(directory: &Path) -> Arc<RSpaceStateAuthority> {
    RSpaceStateAuthority::durable(directory, view_limits())
}

pub(super) fn open_view(authority: &Arc<RSpaceStateAuthority>) -> Space {
    Space::create(RSpaceStore::new(Arc::clone(authority)), matcher())
        .expect("RSpace::create opens a view of the durable store")
}

pub(super) fn owner_slot(authority: &RSpaceStateAuthority) -> Slot {
    match authority.route() {
        AuthorityRouteV1::Durable(backend) => backend
            .owner_slot::<String, String, String, String>()
            .expect("the durable owner slot serves the string profile"),
        AuthorityRouteV1::Legacy => panic!("the trial authority uses the durable backend"),
    }
}

pub(super) fn published_root(slot: &Slot) -> Blake2b256Hash {
    Blake2b256Hash::from(slot.published_core().current_root())
}

pub(super) fn hard_checkpoint(view: &Space) -> Checkpoint {
    first_poll(view.create_checkpoint()).expect("the durable view's hard checkpoint publishes")
}

fn comms_in(log: &Log) -> usize {
    log.iter()
        .filter(|event| matches!(event, Event::Comm(_)))
        .count()
}

fn fired_receive(
    continuation: &ContResult<String, String, String>,
    consumed: &[RSpaceResult<String, String>],
) -> Receive {
    assert_eq!(
        continuation.channels.len(),
        continuation.patterns.len(),
        "a COMM returned a continuation without one pattern per channel"
    );
    assert!(
        !continuation.persistent && !continuation.peek,
        "a COMM returned a persistent or peeking continuation: {continuation:?}"
    );
    let fired = Receive::new(
        group_of(&continuation.channels),
        patterns_of(&continuation.patterns),
        position_of(&BODIES, &continuation.continuation),
    );
    let mut taken: Vec<(&str, &str, &str, bool)> = consumed
        .iter()
        .map(|datum| {
            (
                datum.channel.as_str(),
                datum.matched_datum.as_str(),
                datum.removed_datum.as_str(),
                datum.persistent,
            )
        })
        .collect();
    taken.sort_unstable();
    let mut named: Vec<(&str, &str, &str, bool)> = fired
        .needs()
        .map(|(member, pattern)| (CHANNELS[member], VALUES[pattern], VALUES[pattern], false))
        .collect();
    named.sort_unstable();
    assert_eq!(taken, named, "a COMM consumed other data than its patterns name");
    fired
}

fn perform<S: ISpace<String, String, String, String>>(
    space: &S,
    operation: Operation,
) -> Option<Receive> {
    match operation {
        Operation::Produce { channel, value } => {
            first_poll(space.produce(CHANNELS[channel].to_owned(), VALUES[value].to_owned(), false))
                .expect("a produce on a durable store succeeds")
                .map(|(continuation, consumed, _)| fired_receive(&continuation, &consumed))
        }
        Operation::Consume(receive) => first_poll(space.consume(
            receive.channels(),
            receive.pattern_values(),
            BODIES[receive.body].to_owned(),
            false,
            BTreeSet::new(),
        ))
        .expect("a consume on a durable store succeeds")
        .map(|(continuation, consumed)| fired_receive(&continuation, &consumed)),
    }
}

pub(super) fn run(view: &Space, contents: &mut Contents, operations: &[Operation]) {
    for &operation in operations {
        let modelled = contents
            .apply(operation)
            .expect("a seeded operation is choice-free and stores no continuation twice");
        assert_eq!(
            perform(view, operation),
            modelled,
            "the view's effect of {operation:?} differs from the model"
        );
    }
}

fn observe(
    data_on: impl Fn(&String) -> Vec<Datum<String>>,
    receives_on: impl Fn(&[String]) -> Vec<WaitingContinuation<String, String>>,
    joins_of: impl Fn(&String) -> Vec<Vec<String>>,
) -> Observed {
    let mut contents = Contents::default();
    let mut joins = BTreeSet::new();
    for (member, name) in CHANNELS.into_iter().enumerate() {
        let key = name.to_owned();
        for stored in data_on(&key) {
            assert!(!stored.persist, "a stored datum became persistent: {stored:?}");
            contents.data[member][position_of(&VALUES, stored.a.as_str())] += 1;
        }
        for joined in joins_of(&key) {
            assert!(
                joins.insert((member, group_of(&joined))),
                "a join was read twice: {name} {joined:?}"
            );
        }
    }
    for (group, members) in GROUPS.into_iter().enumerate() {
        for stored in receives_on(&channels_of(members)) {
            assert!(
                !stored.persist && stored.peeks.is_empty(),
                "a stored continuation became persistent or peeking: {stored:?}"
            );
            assert_eq!(
                stored.patterns.len(),
                members.len(),
                "a stored continuation lost a pattern: {stored:?}"
            );
            assert!(
                contents.receives.insert(Receive::new(
                    group,
                    patterns_of(stored.patterns.as_slice()),
                    position_of(&BODIES, stored.continuation.as_str()),
                )),
                "a continuation was read twice: {stored:?}"
            );
        }
    }
    for members in UNUSED_ORDERS {
        assert!(
            receives_on(&channels_of(members)).is_empty(),
            "a continuation was read under the unused channel order {members:?}"
        );
    }
    Observed { contents, joins }
}

pub(super) fn expected(contents: &Contents) -> Observed {
    Observed {
        contents: contents.clone(),
        joins: contents.joins(),
    }
}

pub(super) fn observe_root(slot: &Slot, root: &Blake2b256Hash) -> Observed {
    let target =
        DurableStateRootV1::try_from(root).expect("a checkpoint root is a durable state root");
    let (resolved, handle) = slot
        .published_core()
        .read_handle(Some(target))
        .expect("the store retains the state of a returned root");
    let reader = DurableViewReaderV1::new(handle, resolved.roots());
    observe(
        |key| reader.get_data_canonical(key),
        |key| reader.get_continuations_canonical(key),
        |key| reader.get_joins_canonical(key),
    )
}

pub(super) fn observe_view(view: &Space) -> Observed {
    observe(
        |key| first_poll(view.get_data(key)),
        |key| first_poll(view.get_waiting_continuations(key.to_vec())),
        |key| first_poll(view.get_joins(key.clone())),
    )
}

fn holds_the_installed_continuation(view: &Space) -> bool {
    first_poll(view.get_waiting_continuations(vec![INSTALLED_CHANNEL.to_owned()]))
        .iter()
        .any(|stored| stored.persist && stored.continuation.as_str() == INSTALLED_BODY)
}

fn single_view_roots(segments: &[&[Operation]]) -> Vec<Blake2b256Hash> {
    let twin = DurableStore::open("durable-rspace-checkpoint-twin-");
    let mut contents = Contents::default();
    segments
        .iter()
        .map(|segment| {
            run(&twin.view, &mut contents, segment);
            hard_checkpoint(&twin.view).root
        })
        .collect()
}

fn admit(contents: &mut Contents, operations: &[Operation]) -> Segment {
    let mut segment = Segment {
        admitted: Vec::with_capacity(operations.len()),
        ..Segment::default()
    };
    let mut stored_here = Contents::default();
    for &operation in operations {
        let Ok(fired) = contents.apply(operation) else {
            segment.left_out += 1;
            continue;
        };
        segment.admitted.push((operation, fired));
        match (operation, fired) {
            (Operation::Produce { channel, value }, None) => {
                stored_here.data[channel][value] += 1;
            }
            (Operation::Consume(receive), None) => {
                stored_here.receives.insert(receive);
            }
            (Operation::Produce { channel, .. }, Some(receive)) => {
                segment.comms_by_produce += 1;
                let continuation_stored_earlier = !stored_here.receives.remove(&receive);
                let datum_stored_earlier = stored_here
                    .take_stored_here(receive.needs().filter(|&(member, _)| member != channel));
                segment.comms_on_contents_of_an_earlier_checkpoint +=
                    u32::from(continuation_stored_earlier || datum_stored_earlier);
            }
            (Operation::Consume(_), Some(receive)) => {
                segment.comms_by_consume += 1;
                segment.comms_on_contents_of_an_earlier_checkpoint +=
                    u32::from(stored_here.take_stored_here(receive.needs()));
            }
        }
    }
    segment
}

fn another_order(
    start: &Contents,
    admitted: &[(Operation, Option<Receive>)],
    reached: &Contents,
) -> Option<Vec<(Operation, Option<Receive>)>> {
    let mut contents = start.clone();
    let mut reordered = Vec::with_capacity(admitted.len());
    for &(operation, _) in admitted.iter().rev() {
        reordered.push((operation, contents.apply(operation).ok()?));
    }
    let differs = reordered
        .iter()
        .zip(admitted)
        .any(|(moved, original)| moved.0 != original.0);
    (differs && contents == *reached).then_some(reordered)
}

fn replayed_root(
    twin: &DurableStore,
    log: &Log,
    operations: &[(Operation, Option<Receive>)],
) -> Blake2b256Hash {
    let (repository, _) = Space::create_history_repo(RSpaceStore::new(Arc::clone(&twin.authority)))
        .expect("the replay twin opens a repository at its current state");
    let (base, store) = twin
        .slot
        .view_at(&repository.root())
        .expect("the replay twin pins its current state");
    let replay = Replay::apply(Arc::new(repository), Arc::new(store), matcher());
    first_poll(replay.rig(log.clone())).expect("the replay takes the play's event log");
    for &(operation, fired) in operations {
        assert_eq!(
            perform(&replay, operation),
            fired,
            "the replay of {operation:?} differs from the play"
        );
    }
    first_poll(replay.check_replay_data())
        .expect("the replay uses every COMM of the play's event log");
    let gate = Arc::new(RwLock::new(()));
    let permit = Arc::clone(&gate)
        .try_write_owned()
        .expect("the replay twin's gate is free");
    let mut owner = twin
        .slot
        .owner()
        .try_lock()
        .expect("the replay twin's owner mutex is free");
    let (reached, _) = first_poll(owner.checkpoint_view_from_base(
        permit,
        base,
        &gate,
        &replay.get_store(),
        view_limits(),
    ))
    .expect("the replay twin's owner publishes the replayed journal");
    Blake2b256Hash::from(reached.root())
}

fn check_trace(trace: &[Vec<Operation>]) -> Result<Coverage, TestCaseError> {
    let play = DurableStore::open("durable-rspace-checkpoint-play-");
    let replay_twin = DurableStore::open("durable-rspace-checkpoint-replay-");
    let order_twin = DurableStore::open("durable-rspace-checkpoint-order-");
    let mut contents = Contents::default();
    let mut states = Vec::with_capacity(trace.len() + 1);
    states.push((published_root(&play.slot), contents.clone()));
    let mut coverage = Coverage {
        traces: 1,
        ..Coverage::default()
    };
    for operations in trace {
        let start = contents.clone();
        let segment = admit(&mut contents, operations);
        for &(operation, fired) in &segment.admitted {
            prop_assert_eq!(
                perform(&play.view, operation),
                fired,
                "the view's effect of {:?} differs from the model",
                operation
            );
        }
        let checkpoint = hard_checkpoint(&play.view);
        let comms = segment
            .admitted
            .iter()
            .filter(|(_, fired)| fired.is_some())
            .count();
        prop_assert_eq!(
            (checkpoint.log.len(), comms_in(&checkpoint.log)),
            (segment.admitted.len() + comms, comms),
            "a hard checkpoint returns one event per operation and one per COMM"
        );
        prop_assert_eq!(
            observe_root(&play.slot, &checkpoint.root),
            expected(&contents),
            "the contents at the returned root differ from the model"
        );
        prop_assert_eq!(
            observe_view(&play.view),
            expected(&contents),
            "the view's contents after its hard checkpoint differ from the model"
        );
        prop_assert_eq!(
            (published_root(&play.slot), first_poll(play.view.get_root())),
            (checkpoint.root.clone(), checkpoint.root.clone()),
            "the returned root is not the published root and the root of the view's repository"
        );
        prop_assert_eq!(
            replayed_root(&replay_twin, &checkpoint.log, &segment.admitted),
            checkpoint.root.clone(),
            "a durable replay of the event log from the same start reached another root"
        );
        let reordered = another_order(&start, &segment.admitted, &contents);
        for &(operation, fired) in reordered.as_deref().unwrap_or(&segment.admitted) {
            prop_assert_eq!(
                perform(&order_twin.view, operation),
                fired,
                "the reordered view's effect of {:?} differs from the model",
                operation
            );
        }
        prop_assert_eq!(
            hard_checkpoint(&order_twin.view).root,
            checkpoint.root.clone(),
            "two operation orders that reach equal contents returned different roots"
        );
        coverage.checkpoints += 1;
        coverage.operations +=
            u32::try_from(segment.admitted.len()).expect("the operations of a segment fit u32");
        coverage.operations_left_out += segment.left_out;
        coverage.comms_by_produce += segment.comms_by_produce;
        coverage.comms_by_consume += segment.comms_by_consume;
        coverage.comms_on_contents_of_an_earlier_checkpoint +=
            segment.comms_on_contents_of_an_earlier_checkpoint;
        coverage.checkpoints_holding_both_orders_of_a_channel_pair +=
            u32::from([PAIR_IN_ORDER, PAIR_REVERSED].into_iter().all(|group| {
                contents
                    .receives
                    .iter()
                    .any(|receive| receive.group == group)
            }));
        coverage.checkpoints_of_an_empty_journal += u32::from(segment.admitted.is_empty());
        coverage.checkpoints_of_a_journal_without_effect +=
            u32::from(!segment.admitted.is_empty() && contents == start);
        coverage.segments_run_in_another_order += u32::from(reordered.is_some());
        coverage.replayed_comms += u32::try_from(comms).expect("the COMMs of a segment fit u32");
        states.push((checkpoint.root, contents.clone()));
    }
    for (left_root, left_contents) in &states {
        for (right_root, right_contents) in &states {
            prop_assert_eq!(
                left_root == right_root,
                left_contents == right_contents,
                "a durable state root must name exactly one modelled content"
            );
        }
    }
    Ok(coverage)
}

fn seeded_traces() -> [Vec<Vec<Operation>>; 3] {
    [
        vec![
            vec![
                consume(&["a", "b"], &["v0", "v1"], "k0"),
                consume(&["b", "a"], &["v0", "v0"], "k1"),
                produce("c", "v0"),
            ],
            vec![produce("a", "v0"), produce("b", "v1")],
            vec![
                produce("c", "v1"),
                consume(&["c"], &["v1"], "k0"),
                consume(&["c"], &["v0"], "k1"),
            ],
            Vec::new(),
            vec![produce("a", "v0"), consume(&["a"], &["v0"], "k0")],
            vec![produce("b", "v0"), produce("a", "v0"), consume(&["b", "a"], &["v0", "v0"], "k1")],
        ],
        vec![
            vec![
                produce("a", "v0"),
                produce("a", "v0"),
                produce("b", "v1"),
                consume(&["b", "c"], &["v1", "v0"], "k1"),
            ],
            vec![consume(&["a"], &["v0"], "k0"), produce("c", "v0")],
            vec![
                consume(&["a", "b"], &["v0", "v0"], "k0"),
                consume(&["a", "b"], &["v0", "v0"], "k1"),
            ],
        ],
        vec![Vec::new()],
    ]
}

fn generated_operations() -> impl Strategy<Value = Operation> {
    prop_oneof![
        (0..CHANNELS.len(), 0..VALUES.len())
            .prop_map(|(channel, value)| Operation::Produce { channel, value }),
        (0..GROUPS.len(), prop::array::uniform2(0..VALUES.len()), 0..BODIES.len()).prop_map(
            |(group, patterns, body)| Operation::Consume(Receive::new(group, patterns, body))
        ),
    ]
}

fn generated_traces() -> impl Strategy<Value = Vec<Vec<Operation>>> {
    prop::collection::vec(
        prop::collection::vec(generated_operations(), 0..=MAX_OPERATIONS),
        1..=MAX_SEGMENTS,
    )
}

#[test]
fn hard_checkpoints_of_choice_free_traces_return_the_expected_contents_and_replayable_roots() {
    let mut seeded = Coverage::default();
    for trace in seeded_traces() {
        seeded.merge(check_trace(&trace).expect("a seeded trace satisfies OG-30"));
    }
    println!("durable hard checkpoint seeds: {seeded:?}");
    assert_eq!(
        seeded.operations_left_out, 0,
        "a seeded trace holds a choice or stores a continuation twice"
    );
    seeded.assert_complete();
    let generated = Cell::new(Coverage::default());
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases: GENERATED_TRACES,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
    .run(&generated_traces(), |trace| {
        let mut total = generated.get();
        total.merge(check_trace(&trace)?);
        generated.set(total);
        Ok(())
    })
    .expect("generated traces satisfy OG-30");
    assert!(generated.get().traces >= GENERATED_TRACES);
    println!("durable hard checkpoint generated traces: {:?}", generated.get());
}

async fn checkpoint_under_the_held_view_gate(authority: Arc<RSpaceStateAuthority>) {
    let view = open_view(&authority);
    let slot = owner_slot(&authority);
    let mut contents = Contents::default();
    run(&view, &mut contents, &[produce("a", "v0")]);
    let held_at_capture = Rc::new(Cell::new(None));
    let parked: Rc<RefCell<Option<ParkedProduce>>> = Rc::new(RefCell::new(None));
    let hook = journal_owner_hooks::arm(JournalOwnerHookPoint::AfterCapture, {
        let waiting_view = view.clone();
        let slot = Arc::clone(&slot);
        let held_at_capture = Rc::clone(&held_at_capture);
        let parked = Rc::clone(&parked);
        move || {
            let mut waiting: ParkedProduce = Box::pin(tokio::task::unconstrained(async move {
                waiting_view
                    .produce("b".to_owned(), "v1".to_owned(), false)
                    .await
                    .map(|fired| fired.is_some())
            }));
            let gate_refuses_a_reader = waiting
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending();
            let owner_is_locked = slot.owner().try_lock().is_err();
            held_at_capture.set(Some((gate_refuses_a_reader, owner_is_locked)));
            *parked.borrow_mut() = Some(waiting);
        }
    });
    let checkpoint = view
        .create_checkpoint()
        .await
        .expect("the hard checkpoint under the held view gate publishes");
    drop(hook);
    assert_eq!(
        held_at_capture.get(),
        Some((true, true)),
        "at capture the view gate refuses a reader and the owner mutex is locked"
    );
    assert_eq!(published_root(&slot), checkpoint.root);
    assert_eq!(observe_root(&slot, &checkpoint.root), expected(&contents));
    assert!(slot.owner().try_lock().is_ok(), "the hard checkpoint kept the owner mutex");
    let waiting = parked
        .borrow_mut()
        .take()
        .expect("the capture hook parked a produce at the view gate");
    assert_eq!(
        waiting.await,
        Ok(false),
        "the produce that waited for the view gate stores its datum"
    );
    contents
        .apply(produce("b", "v1"))
        .expect("the waiting produce is choice-free");
    assert_eq!(
        observe_view(&view),
        expected(&contents),
        "the datum of the produce that waited for the view gate is not on the view"
    );
    let next = view
        .create_checkpoint()
        .await
        .expect("the view publishes the datum of the produce that waited");
    assert_eq!(observe_root(&slot, &next.root), expected(&contents));
}

fn finish_under_the_gate_watchdog(trial: impl FnOnce() + Send + 'static) {
    let (finished_sender, finished_receiver) = mpsc::channel();
    let runtime_thread = std::thread::spawn(move || {
        trial();
        finished_sender
            .send(())
            .expect("the test thread awaits the hard checkpoint");
    });
    match finished_receiver.recv_timeout(Duration::from_secs(30)) {
        Ok(()) => runtime_thread.join().expect("the runtime thread finishes"),
        Err(RecvTimeoutError::Timeout) => panic!("{GATE_WATCHDOG}"),
        Err(RecvTimeoutError::Disconnected) => match runtime_thread.join() {
            Err(panic) => std::panic::resume_unwind(panic),
            Ok(()) => panic!("the runtime thread ended without reporting the hard checkpoint"),
        },
    }
}

#[test]
fn a_hard_checkpoint_under_the_held_view_gate_and_owner_mutex_completes() {
    let scratch = trial_scratch("durable-rspace-checkpoint-gate-");
    let authority = durable_authority(scratch.path());
    finish_under_the_gate_watchdog(move || {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("build a current-thread runtime")
            .block_on(checkpoint_under_the_held_view_gate(authority));
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OwnerExit {
    SameBaseEmptyJournal,
    SameBasePublication,
    CrossBaseEmptyJournal,
    CrossBaseJournalThatMakesTheBaseCurrent,
    CrossBaseMemoHit,
    CrossBasePublication,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RacedProduce {
    WaitedForTheGate,
    RanOnTheSupersededStore,
}

#[derive(Debug, PartialEq)]
struct RacedInstallation {
    exit: Option<OwnerExit>,
    raced: RacedProduce,
    fired: Result<bool, RSpaceError>,
    view: Observed,
    next_log: Log,
    next_root: Observed,
}

fn exit_taken(
    base: &Blake2b256Hash,
    current: &Blake2b256Hash,
    answered: &Blake2b256Hash,
    published: &Blake2b256Hash,
) -> Option<OwnerExit> {
    match (base == current, answered == base, answered == current, answered == published) {
        (true, true, true, true) => Some(OwnerExit::SameBaseEmptyJournal),
        (true, false, false, true) => Some(OwnerExit::SameBasePublication),
        (false, true, false, false) => Some(OwnerExit::CrossBaseEmptyJournal),
        (false, false, true, true) => Some(OwnerExit::CrossBaseJournalThatMakesTheBaseCurrent),
        (false, false, false, false) => Some(OwnerExit::CrossBaseMemoHit),
        (false, false, false, true) => Some(OwnerExit::CrossBasePublication),
        _ => None,
    }
}

async fn raced_installation(ahead: &[&[Operation]], journal: &[Operation]) -> RacedInstallation {
    let store = DurableStore::open("durable-rspace-checkpoint-installation-");
    let racer = open_view(&store.authority);
    let base = published_root(&store.slot);
    let mut ahead_contents = Contents::default();
    for segment in ahead {
        run(&store.view, &mut ahead_contents, segment);
        hard_checkpoint(&store.view);
    }
    let current = published_root(&store.slot);
    let mut contents = Contents::default();
    run(&racer, &mut contents, journal);
    let race = Rc::new(RefCell::new(None));
    let hook = journal_owner_hooks::arm(JournalOwnerHookPoint::BeforeViewInstallation, {
        let racing_view = racer.clone();
        let race = Rc::clone(&race);
        move || {
            let (first_poll_sender, first_poll_receiver) = mpsc::channel();
            let finished = tokio::spawn(async move {
                let mut operation = Box::pin(tokio::task::unconstrained(async move {
                    racing_view
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
    let checkpoint = racer
        .create_checkpoint()
        .await
        .expect("the raced hard checkpoint returns the owner's answer");
    drop(hook);
    let published = published_root(&store.slot);
    let (raced, finished) = race
        .borrow_mut()
        .take()
        .expect("the hard checkpoint passes the installation hook");
    let fired = finished
        .await
        .expect("the worker finishes the raced produce");
    let view = observe_view(&racer);
    let next = hard_checkpoint(&racer);
    RacedInstallation {
        exit: exit_taken(&base, &current, &checkpoint.root, &published),
        raced,
        fired,
        view,
        next_log: next.log,
        next_root: observe_root(&store.slot, &next.root),
    }
}

fn installed_answer(exit: OwnerExit, journal: &[Operation]) -> RacedInstallation {
    let mut contents = Contents::default();
    for &operation in journal {
        contents
            .apply(operation)
            .expect("the journal of a scenario is choice-free");
    }
    contents
        .apply(produce(RACED_CHANNEL, RACED_VALUE))
        .expect("the raced produce is choice-free");
    RacedInstallation {
        exit: Some(exit),
        raced: RacedProduce::WaitedForTheGate,
        fired: Ok(false),
        view: expected(&contents),
        next_log: vec![Event::IoEvent(IOEvent::Produce(Produce::create(
            &RACED_CHANNEL.to_owned(),
            &RACED_VALUE.to_owned(),
            false,
        )))],
        next_root: expected(&contents),
    }
}

async fn a_produce_races_the_installation_at_every_owner_exit() {
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
        observed.push(raced_installation(ahead, journal).await);
        installed.push(installed_answer(exit, journal));
    }
    assert_eq!(
        observed, installed,
        "a produce that arrives while a hard checkpoint installs the owner's answer must wait for \
         the view gate at every owner exit, run on the installed store, be logged for the next \
         checkpoint and be published by it"
    );
}

#[test]
fn a_concurrent_operation_waits_until_every_owner_exit_is_installed() {
    finish_under_the_gate_watchdog(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .build()
            .expect("build a two-worker runtime")
            .block_on(a_produce_races_the_installation_at_every_owner_exit());
    });
}

#[test]
fn a_view_behind_the_current_state_publishes_the_successor_of_its_own_base() {
    let store = DurableStore::open("durable-rspace-checkpoint-behind-");
    let shared = [produce("a", "v0"), consume(&["b", "c"], &["v0", "v1"], "k0")];
    let ahead_journal = [produce("c", "v1"), consume(&["a"], &["v0"], "k1")];
    let behind_journal = [produce("b", "v0"), consume(&["a", "b"], &["v1", "v1"], "k1")];
    let overtaken_journal = [produce("b", "v0"), produce("a", "v1")];
    let mut shared_contents = Contents::default();
    run(&store.view, &mut shared_contents, &shared);
    let shared_root = hard_checkpoint(&store.view).root;
    let ahead = open_view(&store.authority);
    let behind = open_view(&store.authority);
    let mut ahead_contents = shared_contents.clone();
    let mut behind_contents = shared_contents.clone();

    run(&ahead, &mut ahead_contents, &ahead_journal);
    let ahead_root = hard_checkpoint(&ahead).root;
    assert_eq!(published_root(&store.slot), ahead_root);

    run(&behind, &mut behind_contents, &behind_journal);
    let behind_checkpoint = hard_checkpoint(&behind);
    let current_base_roots = single_view_roots(&[&shared, &behind_journal]);
    assert_eq!(current_base_roots[0], shared_root);
    assert_eq!(
        behind_checkpoint.root, current_base_roots[1],
        "a view behind the current state returned another root than the same operations give from \
         its base while that base is current"
    );
    assert_ne!(behind_checkpoint.root, ahead_root);
    assert_eq!(behind_checkpoint.log.len(), behind_journal.len());
    assert_eq!(
        observe_root(&store.slot, &behind_checkpoint.root),
        expected(&behind_contents),
        "the successor of the view's own base holds other contents than its base and its journal"
    );
    assert_eq!(observe_view(&behind), expected(&behind_contents));
    assert_eq!(published_root(&store.slot), behind_checkpoint.root);
    assert_eq!(
        observe_root(&store.slot, &ahead_root),
        expected(&ahead_contents),
        "the state that the other view published changed"
    );

    run(&ahead, &mut ahead_contents, &overtaken_journal);
    let overtaken_root = hard_checkpoint(&ahead).root;
    let overtaken_base_roots = single_view_roots(&[&shared, &ahead_journal, &overtaken_journal]);
    assert_eq!(overtaken_base_roots[1], ahead_root);
    assert_eq!(
        overtaken_root, overtaken_base_roots[2],
        "a view that was overtaken returned another root than the same operations give from its \
         base while that base is current"
    );
    assert_eq!(observe_root(&store.slot, &overtaken_root), expected(&ahead_contents));
    assert_eq!(observe_view(&ahead), expected(&ahead_contents));
    assert_eq!(published_root(&store.slot), overtaken_root);
}

#[test]
fn a_view_continues_from_the_state_every_owner_exit_returns() {
    let store = DurableStore::open("durable-rspace-checkpoint-exits-");
    let second = open_view(&store.authority);
    let third = open_view(&store.authority);
    let genesis_root = published_root(&store.slot);
    assert_eq!(
        first_poll(store.view.install(
            vec![INSTALLED_CHANNEL.to_owned()],
            vec!["installed-pattern".to_owned()],
            INSTALLED_BODY.to_owned(),
        )),
        Ok(None),
        "a continuation is installed on a channel without data"
    );

    let files = directory_snapshot(store.path());
    let unchanged = hard_checkpoint(&store.view);
    assert_eq!(
        unchanged.root, genesis_root,
        "an empty journal at the current state moved the root"
    );
    assert!(unchanged.log.is_empty());
    assert_eq!(directory_snapshot(store.path()), files, "an empty journal wrote to the store");
    assert!(holds_the_installed_continuation(&store.view));

    let mut first_contents = Contents::default();
    run(&store.view, &mut first_contents, &[produce("a", "v0")]);
    let one_datum_root = hard_checkpoint(&store.view).root;
    run(&store.view, &mut first_contents, &[produce("b", "v1")]);
    let two_data_root = hard_checkpoint(&store.view).root;
    assert_eq!(observe_root(&store.slot, &two_data_root), expected(&first_contents));
    assert_eq!(observe_view(&store.view), expected(&first_contents));
    assert!(holds_the_installed_continuation(&store.view));
    assert!(
        first_poll(second.get_waiting_continuations(vec![INSTALLED_CHANNEL.to_owned()])).is_empty(),
        "an installed continuation of one view was published to another view"
    );

    let files = directory_snapshot(store.path());
    let own_base = hard_checkpoint(&second);
    assert_eq!(
        own_base.root, genesis_root,
        "an empty journal at a superseded base did not return the view's own base"
    );
    assert_eq!(observe_view(&second), expected(&Contents::default()));

    let mut second_contents = Contents::default();
    run(&second, &mut second_contents, &[produce("a", "v0"), produce("b", "v1")]);
    let already_current = hard_checkpoint(&second);
    assert_eq!(
        already_current.root, two_data_root,
        "a journal that turns a superseded base into the current state did not return it"
    );
    assert_eq!(already_current.log.len(), 2);
    assert_eq!(observe_view(&second), expected(&second_contents));

    let mut third_contents = Contents::default();
    run(&third, &mut third_contents, &[produce("a", "v0")]);
    let memoised = hard_checkpoint(&third);
    assert_eq!(
        memoised.root, one_datum_root,
        "a completed transition repeated from its base did not return its successor"
    );
    assert_eq!(observe_view(&third), expected(&third_contents));
    assert_eq!(
        (published_root(&store.slot), directory_snapshot(store.path())),
        (two_data_root, files),
        "an owner exit without a publication moved the published state or wrote to the store"
    );

    run(&third, &mut third_contents, &[produce("c", "v1")]);
    let from_memoised = hard_checkpoint(&third).root;
    assert_eq!(
        from_memoised,
        single_view_roots(&[&[produce("a", "v0")], &[produce("c", "v1")]])[1],
        "a view did not continue from the memoised successor it was answered with"
    );
    assert_eq!(observe_root(&store.slot, &from_memoised), expected(&third_contents));
    assert_eq!(published_root(&store.slot), from_memoised);

    run(&second, &mut second_contents, &[consume(&["a"], &["v0"], "k0")]);
    let from_current = hard_checkpoint(&second).root;
    assert_eq!(
        from_current,
        single_view_roots(&[&[produce("a", "v0"), produce("b", "v1")], &[consume(
            &["a"],
            &["v0"],
            "k0"
        )]])[1],
        "a view did not continue from the current state it was answered with"
    );
    assert_eq!(observe_root(&store.slot, &from_current), expected(&second_contents));
    assert_eq!(observe_view(&second), expected(&second_contents));
    assert!(holds_the_installed_continuation(&store.view));
}

#[tokio::test]
async fn the_store_managers_limits_admit_a_same_base_and_a_cross_base_checkpoint() {
    let scratch = trial_scratch("durable-rspace-checkpoint-manager-");
    let mut manager = manager_over(scratch.path());
    manager
        .select_rspace_backend("rspace", RSpaceBackendSelection::Durable)
        .await
        .expect("the durable backend is selected before the prefix is used");
    let stores = manager
        .r_space_stores()
        .await
        .expect("the manager builds the RSpace store of the durable prefix");
    let slot = owner_slot(&stores.authority());
    let ahead = Space::create(stores.clone(), matcher())
        .expect("RSpace::create opens the first view of the manager's durable store");
    let behind = Space::create(stores, matcher())
        .expect("RSpace::create opens the second view of the manager's durable store");
    let ahead_journal = [produce("a", "v0"), consume(&["b", "c"], &["v0", "v1"], "k0")];
    let behind_journal = [produce("b", "v1"), consume(&["a", "b"], &["v1", "v0"], "k1")];
    let mut ahead_contents = Contents::default();
    let mut behind_contents = Contents::default();

    run(&ahead, &mut ahead_contents, &ahead_journal);
    let ahead_root = hard_checkpoint(&ahead).root;
    assert_eq!(ahead_root, single_view_roots(&[&ahead_journal])[0]);
    assert_eq!(observe_root(&slot, &ahead_root), expected(&ahead_contents));

    run(&behind, &mut behind_contents, &behind_journal);
    let behind_root = hard_checkpoint(&behind).root;
    assert_eq!(
        behind_root,
        single_view_roots(&[&behind_journal])[0],
        "the store manager's limits changed the successor of a superseded base"
    );
    assert_eq!(observe_root(&slot, &behind_root), expected(&behind_contents));
    assert_eq!(published_root(&slot), behind_root);
}

#[test]
fn a_refused_durable_checkpoint_is_a_typed_error_and_changes_nothing() {
    let store = DurableStore::open("durable-rspace-checkpoint-refusal-");
    let journal = [produce("a", "v0"), consume(&["b", "c"], &["v0", "v1"], "k0")];
    let mut contents = Contents::default();
    run(&store.view, &mut contents, &journal);
    let files = directory_snapshot(store.path());
    let root = published_root(&store.slot);
    let mut owner = store
        .slot
        .owner()
        .try_lock()
        .expect("the owner mutex is free");
    owner
        .close()
        .expect("an owner without a pending publication closes");
    drop(owner);

    assert_eq!(
        first_poll(store.view.create_checkpoint()).err(),
        Some(RSpaceError::HistoryError(HistoryError::ActionError(
            "the durable owner refused the view checkpoint: Closed".to_owned()
        ))),
        "a closed owner refuses the view's hard checkpoint with a typed error"
    );
    assert_eq!(
        directory_snapshot(store.path()),
        files,
        "a refused hard checkpoint wrote to the store"
    );
    assert_eq!(
        (published_root(&store.slot), first_poll(store.view.get_root())),
        (root.clone(), root),
        "a refused hard checkpoint moved the published state or the view's repository"
    );
    assert_eq!(
        observe_view(&store.view),
        expected(&contents),
        "a refused hard checkpoint changed the view's contents"
    );
    run(&store.view, &mut contents, &[produce("c", "v0")]);
    assert_eq!(
        first_poll(store.view.take_event_log()).len(),
        journal.len() + 1,
        "a refused hard checkpoint took the view's event log"
    );

    let detached_scratch = trial_scratch("durable-rspace-checkpoint-detached-");
    let (repository, hot_store) =
        Space::create_history_repo(RSpaceStore::new(durable_authority(detached_scratch.path())))
            .expect("create_history_repo opens the durable store");
    let detached = Space::apply(Arc::new(repository), hot_store, matcher());
    assert_eq!(perform(&detached, produce("a", "v0")), None);
    assert_eq!(
        first_poll(detached.create_checkpoint()).err(),
        Some(RSpaceError::HistoryError(HistoryError::BackendMismatch(
            AuthorityBackendMismatch::History
        ))),
        "a durable space that RSpace::create did not build keeps the typed backend refusal"
    );
}

#[test]
fn a_view_built_by_create_keeps_the_owner_slot_until_it_is_dropped() {
    let scratch = trial_scratch("durable-rspace-checkpoint-slot-");
    let authority = durable_authority(scratch.path());
    let view = open_view(&authority);
    drop(authority);
    let mut contents = Contents::default();
    run(&view, &mut contents, &[produce("a", "v0")]);
    let first_root = hard_checkpoint(&view).root;

    let successor = durable_authority(scratch.path());
    let refused = Space::create(RSpaceStore::new(Arc::clone(&successor)), matcher())
        .err()
        .expect("a second authority is refused while a view of the first authority lives");
    match &refused {
        HistoryRepositoryError::DurableBackend(error) => {
            match error.downcast_ref::<DurableBackendErrorV1>() {
                Some(DurableBackendErrorV1::Recovery(refusal)) => assert!(
                    matches!(
                        **refusal,
                        DurableAdmissionErrorV1::Startup(DurableStartupErrorV1::Io(
                            DurableIoError::WriterLockContended { .. }
                        ))
                    ),
                    "the second authority was refused with {refusal:?}"
                ),
                other => panic!("the second authority was refused with {other:?}"),
            }
        }
        other => panic!("the second authority was refused outside the durable backend: {other:?}"),
    }

    run(&view, &mut contents, &[produce("b", "v1")]);
    let last_root = hard_checkpoint(&view).root;
    assert_ne!(last_root, first_root);
    drop(view);
    let reopened = open_view(&successor);
    assert_eq!(
        first_poll(reopened.get_root()),
        last_root,
        "the store did not reopen at the last root that the dropped view published"
    );
    assert_eq!(observe_view(&reopened), expected(&contents));
}
