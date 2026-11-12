use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt::{self, Debug, Display};
use std::ops::Range;

use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::hot_store::HotStore;
use rspace_plus_plus::rspace::internal::{Datum, ProduceCandidate, Row, WaitingContinuation};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::{ContResult, ISpace, RSpaceResult};
use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError, legacy_prefix,
};
use serde::{Deserialize, Serialize};

pub trait Term: Clone + Ord + Debug {}

impl<T: Clone + Ord + Debug> Term for T {}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DatumEntry<A> {
    pub value: A,
    pub persistent: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContinuationEntry<P, K> {
    pub patterns: Vec<P>,
    pub continuation: K,
    pub persistent: bool,
    pub peeks: BTreeSet<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Contents<C, P, A, K> {
    pub data: BTreeMap<C, BTreeMap<DatumEntry<A>, usize>>,
    pub continuations: BTreeMap<Vec<C>, BTreeMap<ContinuationEntry<P, K>, usize>>,
}

impl<C, P, A, K> Default for Contents<C, P, A, K> {
    fn default() -> Self {
        Self {
            data: BTreeMap::new(),
            continuations: BTreeMap::new(),
        }
    }
}

impl<C: Term, P: Term, A: Term, K: Term> Contents<C, P, A, K> {
    pub fn add_datum(&mut self, channel: C, entry: DatumEntry<A>) {
        *self
            .data
            .entry(channel)
            .or_default()
            .entry(entry)
            .or_default() += 1;
    }

    pub fn add_continuation(&mut self, channels: Vec<C>, entry: ContinuationEntry<P, K>) {
        *self
            .continuations
            .entry(channels)
            .or_default()
            .entry(entry)
            .or_default() += 1;
    }

    pub fn has_repeated_occurrence(&self) -> bool {
        self.data
            .values()
            .flat_map(BTreeMap::values)
            .any(|count| *count > 1)
    }

    fn remove_datum(&mut self, channel: &C, entry: &DatumEntry<A>) -> bool {
        match self.data.get_mut(channel) {
            Some(entries) => {
                let removed = remove_occurrence(entries, entry);
                if entries.is_empty() {
                    self.data.remove(channel);
                }
                removed
            }
            None => false,
        }
    }

    fn remove_continuation(&mut self, channels: &[C], entry: &ContinuationEntry<P, K>) -> bool {
        match self.continuations.get_mut(channels) {
            Some(entries) => {
                let removed = remove_occurrence(entries, entry);
                if entries.is_empty() {
                    self.continuations.remove(channels);
                }
                removed
            }
            None => false,
        }
    }
}

fn remove_occurrence<T: Ord>(entries: &mut BTreeMap<T, usize>, entry: &T) -> bool {
    match entries.get_mut(entry) {
        Some(count) if *count > 1 => {
            *count -= 1;
            true
        }
        Some(_) => {
            entries.remove(entry);
            true
        }
        None => false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Operation<C, P, A, K> {
    Produce {
        channel: C,
        datum: A,
        persistent: bool,
    },
    Consume {
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
        persistent: bool,
        peeks: BTreeSet<i32>,
    },
    Install {
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
    },
}

enum Arrival<C, P, A, K> {
    Datum {
        channel: C,
        entry: DatumEntry<A>,
    },
    Continuation {
        channels: Vec<C>,
        entry: ContinuationEntry<P, K>,
    },
}

impl<C: Term, P: Term, A: Term, K: Term> Operation<C, P, A, K> {
    fn arrival(&self) -> Arrival<C, P, A, K> {
        match self {
            Operation::Produce {
                channel,
                datum,
                persistent,
            } => Arrival::Datum {
                channel: channel.clone(),
                entry: DatumEntry {
                    value: datum.clone(),
                    persistent: *persistent,
                },
            },
            Operation::Consume {
                channels,
                patterns,
                continuation,
                persistent,
                peeks,
            } => Arrival::Continuation {
                channels: channels.clone(),
                entry: ContinuationEntry {
                    patterns: patterns.clone(),
                    continuation: continuation.clone(),
                    persistent: *persistent,
                    peeks: peeks.clone(),
                },
            },
            Operation::Install {
                channels,
                patterns,
                continuation,
            } => Arrival::Continuation {
                channels: channels.clone(),
                entry: ContinuationEntry {
                    patterns: patterns.clone(),
                    continuation: continuation.clone(),
                    persistent: true,
                    peeks: BTreeSet::new(),
                },
            },
        }
    }

    fn is_persistent(&self) -> bool {
        match self {
            Operation::Produce { persistent, .. } | Operation::Consume { persistent, .. } => {
                *persistent
            }
            Operation::Install { .. } => true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Selected<C, A> {
    pub channel: C,
    pub datum: DatumEntry<A>,
    pub binding: A,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rendezvous<C, P, A, K> {
    pub channels: Vec<C>,
    pub continuation: ContinuationEntry<P, K>,
    pub selection: Vec<Selected<C, A>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedComm<C, P, A, K> {
    pub channels: Vec<C>,
    pub patterns: Vec<P>,
    pub continuation: K,
    pub persistent: bool,
    pub peek: bool,
    pub selection: Vec<Selected<C, A>>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FiredBody<A, K> {
    pub continuation: K,
    pub bindings: Vec<A>,
}

impl<C: Term, P: Term, A: Term, K: Term> Rendezvous<C, P, A, K> {
    pub fn observed(&self) -> ObservedComm<C, P, A, K> {
        ObservedComm {
            channels: self.channels.clone(),
            patterns: self.continuation.patterns.clone(),
            continuation: self.continuation.continuation.clone(),
            persistent: self.continuation.persistent,
            peek: !self.continuation.peeks.is_empty(),
            selection: self.selection.clone(),
        }
    }

    fn fired_body(&self) -> FiredBody<A, K> {
        FiredBody {
            continuation: self.continuation.continuation.clone(),
            bindings: self
                .selection
                .iter()
                .map(|selected| selected.binding.clone())
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Enumeration<C, P, A, K> {
    pub members: BTreeSet<Rendezvous<C, P, A, K>>,
    pub rejected_by_guard: usize,
}

impl<C, P, A, K> Default for Enumeration<C, P, A, K> {
    fn default() -> Self {
        Self {
            members: BTreeSet::new(),
            rejected_by_guard: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Firing<C, P, A, K> {
    pub body: FiredBody<A, K>,
    pub issued_again: Vec<Operation<C, P, A, K>>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Outcome<C, P, A, K> {
    pub contents: Contents<C, P, A, K>,
    pub fired: BTreeMap<FiredBody<A, K>, usize>,
}

type Pending<C, P, A, K> = BTreeMap<(Operation<C, P, A, K>, usize), usize>;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Configuration<C, P, A, K> {
    contents: Contents<C, P, A, K>,
    pending: Pending<C, P, A, K>,
    fired: BTreeMap<FiredBody<A, K>, usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Occurrence {
    Resting,
    Arriving,
}

struct Candidate<'w, A> {
    occurrence: Occurrence,
    entry: &'w DatumEntry<A>,
    available: Option<usize>,
    binding: A,
}

struct Shape<'w, C, P, A, K> {
    channels: &'w [C],
    continuation: &'w ContinuationEntry<P, K>,
    candidates: Vec<Vec<Candidate<'w, A>>>,
    require_arriving: bool,
}

type Usage<'w, C, A> = BTreeMap<(Occurrence, &'w C, &'w DatumEntry<A>), usize>;

pub struct Reference<'m, C, P, A, K> {
    matcher: &'m dyn Match<P, A, K>,
    contents: Contents<C, P, A, K>,
}

impl<'m, C: Term, P: Term, A: Term, K: Term> Reference<'m, C, P, A, K> {
    pub fn new(matcher: &'m dyn Match<P, A, K>) -> Self {
        Self {
            matcher,
            contents: Contents::default(),
        }
    }

    pub fn contents(&self) -> &Contents<C, P, A, K> { &self.contents }

    pub fn enabled_for(&self, operation: &Operation<C, P, A, K>) -> Enumeration<C, P, A, K> {
        let mut enumeration = Enumeration::default();
        match operation.arrival() {
            Arrival::Datum { channel, entry } => {
                for (channels, entries) in &self.contents.continuations {
                    if channels.contains(&channel) {
                        for continuation in entries.keys() {
                            self.enumerate_selections(
                                channels,
                                continuation,
                                Some((&channel, &entry)),
                                &mut enumeration,
                            );
                        }
                    }
                }
            }
            Arrival::Continuation { channels, entry } => {
                self.enumerate_selections(&channels, &entry, None, &mut enumeration)
            }
        }
        enumeration
    }

    pub fn enabled_at_rest(&self) -> Enumeration<C, P, A, K> {
        let mut enumeration = Enumeration::default();
        for (channels, entries) in &self.contents.continuations {
            for continuation in entries.keys() {
                self.enumerate_selections(channels, continuation, None, &mut enumeration);
            }
        }
        enumeration
    }

    pub fn rest(&mut self, operation: &Operation<C, P, A, K>) {
        match operation.arrival() {
            Arrival::Datum { channel, entry } => self.contents.add_datum(channel, entry),
            Arrival::Continuation { channels, entry } => {
                self.contents.add_continuation(channels, entry)
            }
        }
    }

    pub fn perform(
        &mut self,
        operation: &Operation<C, P, A, K>,
        member: &Rendezvous<C, P, A, K>,
    ) -> Firing<C, P, A, K> {
        match operation.arrival() {
            Arrival::Datum { channel, entry } => {
                if !member.continuation.persistent {
                    let removed = self
                        .contents
                        .remove_continuation(&member.channels, &member.continuation);
                    assert!(removed, "the continuation of {member:?} rests before it fires");
                }
                let mut arriving_unclaimed = !entry.persistent;
                for selected in member
                    .selection
                    .iter()
                    .filter(|selected| !selected.datum.persistent)
                {
                    if arriving_unclaimed && selected.channel == channel && selected.datum == entry
                    {
                        arriving_unclaimed = false;
                    } else {
                        let removed = self
                            .contents
                            .remove_datum(&selected.channel, &selected.datum);
                        assert!(
                            removed,
                            "the datum {selected:?} of {member:?} rests before it fires"
                        );
                    }
                }
                assert!(
                    !arriving_unclaimed,
                    "the arriving datum of {operation:?} takes part in {member:?}"
                );
            }
            Arrival::Continuation { channels, entry } => {
                assert!(
                    member.channels == channels && member.continuation == entry,
                    "{member:?} fires the arriving continuation of {operation:?}"
                );
                for selected in member
                    .selection
                    .iter()
                    .filter(|selected| !selected.datum.persistent)
                {
                    let removed = self
                        .contents
                        .remove_datum(&selected.channel, &selected.datum);
                    assert!(removed, "the datum {selected:?} of {member:?} rests before it fires");
                }
            }
        }
        Firing {
            body: member.fired_body(),
            issued_again: issued_again(operation, member),
        }
    }

    pub fn outcomes(
        &self,
        program: &[Operation<C, P, A, K>],
        reissue_depth_limit: usize,
    ) -> BTreeSet<Outcome<C, P, A, K>> {
        let mut pending = Pending::new();
        for operation in program {
            *pending.entry((operation.clone(), 0)).or_default() += 1;
        }
        let mut frontier = vec![Configuration {
            contents: self.contents.clone(),
            pending,
            fired: BTreeMap::new(),
        }];
        let mut visited = BTreeSet::new();
        let mut outcomes = BTreeSet::new();
        while let Some(configuration) = frontier.pop() {
            if !visited.insert(configuration.clone()) {
                continue;
            }
            if configuration.pending.is_empty() {
                outcomes.insert(Outcome {
                    contents: configuration.contents,
                    fired: configuration.fired,
                });
                continue;
            }
            for (operation, depth) in configuration.pending.keys() {
                let mut remaining = configuration.pending.clone();
                remove_occurrence(&mut remaining, &(operation.clone(), *depth));
                let mut successor = Reference {
                    matcher: self.matcher,
                    contents: configuration.contents.clone(),
                };
                let enumeration = successor.enabled_for(operation);
                if enumeration.members.is_empty() {
                    successor.rest(operation);
                    frontier.push(Configuration {
                        contents: successor.contents,
                        pending: remaining,
                        fired: configuration.fired.clone(),
                    });
                    continue;
                }
                for member in &enumeration.members {
                    let mut firing_reference = Reference {
                        matcher: self.matcher,
                        contents: configuration.contents.clone(),
                    };
                    let Firing { body, issued_again } = firing_reference.perform(operation, member);
                    let mut next_pending = remaining.clone();
                    if *depth < reissue_depth_limit {
                        for issued in issued_again {
                            *next_pending.entry((issued, depth + 1)).or_default() += 1;
                        }
                    }
                    let mut fired = configuration.fired.clone();
                    *fired.entry(body).or_default() += 1;
                    frontier.push(Configuration {
                        contents: firing_reference.contents,
                        pending: next_pending,
                        fired,
                    });
                }
            }
        }
        outcomes
    }

    fn enumerate_selections<'w>(
        &'w self,
        channels: &'w [C],
        continuation: &'w ContinuationEntry<P, K>,
        arriving: Option<(&'w C, &'w DatumEntry<A>)>,
        enumeration: &mut Enumeration<C, P, A, K>,
    ) {
        assert_eq!(
            channels.len(),
            continuation.patterns.len(),
            "a receive names one pattern per channel"
        );
        let shape = Shape {
            channels,
            continuation,
            candidates: channels
                .iter()
                .zip(&continuation.patterns)
                .map(|(channel, pattern)| self.candidates(channel, pattern, arriving))
                .collect(),
            require_arriving: arriving.is_some(),
        };
        let mut chosen = Vec::with_capacity(channels.len());
        let mut usage = Usage::new();
        self.descend(&shape, &mut chosen, &mut usage, enumeration);
    }

    fn candidates<'w>(
        &'w self,
        channel: &C,
        pattern: &P,
        arriving: Option<(&'w C, &'w DatumEntry<A>)>,
    ) -> Vec<Candidate<'w, A>> {
        let resting =
            self.contents
                .data
                .get(channel)
                .into_iter()
                .flatten()
                .map(|(entry, count)| {
                    (Occurrence::Resting, entry, (!entry.persistent).then_some(*count))
                });
        let arrived = arriving
            .filter(|(arriving_channel, _)| *arriving_channel == channel)
            .map(|(_, entry)| (Occurrence::Arriving, entry, (!entry.persistent).then_some(1)));
        resting
            .chain(arrived)
            .filter_map(|(occurrence, entry, available)| {
                self.matcher
                    .get(pattern, &entry.value)
                    .map(|binding| Candidate {
                        occurrence,
                        entry,
                        available,
                        binding,
                    })
            })
            .collect()
    }

    fn descend<'w>(
        &self,
        shape: &'w Shape<'w, C, P, A, K>,
        chosen: &mut Vec<&'w Candidate<'w, A>>,
        usage: &mut Usage<'w, C, A>,
        enumeration: &mut Enumeration<C, P, A, K>,
    ) {
        match (shape.channels.get(chosen.len()), shape.candidates.get(chosen.len())) {
            (Some(channel), Some(candidates)) => {
                for candidate in candidates {
                    let key = (candidate.occurrence, channel, candidate.entry);
                    let used = usage.get(&key).copied().unwrap_or_default();
                    if candidate
                        .available
                        .is_some_and(|available| used >= available)
                    {
                        continue;
                    }
                    usage.insert(key, used + 1);
                    chosen.push(candidate);
                    self.descend(shape, chosen, usage, enumeration);
                    chosen.pop();
                    usage.insert(key, used);
                }
            }
            _ => self.conclude(shape, chosen, enumeration),
        }
    }

    fn conclude(
        &self,
        shape: &Shape<'_, C, P, A, K>,
        chosen: &[&Candidate<'_, A>],
        enumeration: &mut Enumeration<C, P, A, K>,
    ) {
        let arriving_taken = chosen
            .iter()
            .any(|candidate| matches!(candidate.occurrence, Occurrence::Arriving));
        if shape.require_arriving && !arriving_taken {
            return;
        }
        let bindings: Vec<&A> = chosen.iter().map(|candidate| &candidate.binding).collect();
        if !self
            .matcher
            .check_commit(&shape.continuation.continuation, &bindings)
        {
            enumeration.rejected_by_guard += 1;
            return;
        }
        enumeration.members.insert(Rendezvous {
            channels: shape.channels.to_vec(),
            continuation: shape.continuation.clone(),
            selection: shape
                .channels
                .iter()
                .zip(chosen)
                .map(|(channel, candidate)| Selected {
                    channel: channel.clone(),
                    datum: candidate.entry.clone(),
                    binding: candidate.binding.clone(),
                })
                .collect(),
        });
    }
}

fn issued_again<C: Term, P: Term, A: Term, K: Term>(
    operation: &Operation<C, P, A, K>,
    member: &Rendezvous<C, P, A, K>,
) -> Vec<Operation<C, P, A, K>> {
    let sent_again = member
        .selection
        .iter()
        .enumerate()
        .filter(|(position, selected)| {
            !selected.datum.persistent &&
                member
                    .continuation
                    .peeks
                    .contains(&position_index(*position))
        })
        .map(|(_, selected)| Operation::Produce {
            channel: selected.channel.clone(),
            datum: selected.datum.value.clone(),
            persistent: false,
        });
    let repeated = operation.is_persistent().then(|| operation.clone());
    sent_again.chain(repeated).collect()
}

fn position_index(position: usize) -> i32 {
    i32::try_from(position).expect("a receive has fewer than 2^31 binds")
}

pub fn observed_comm<C: Term, P: Term, A: Term, K: Term>(
    continuation: ContResult<C, P, K>,
    results: Vec<RSpaceResult<C, A>>,
) -> ObservedComm<C, P, A, K> {
    ObservedComm {
        channels: continuation.channels,
        patterns: continuation.patterns,
        continuation: continuation.continuation,
        persistent: continuation.persistent,
        peek: continuation.peek,
        selection: results
            .into_iter()
            .map(|result| Selected {
                channel: result.channel,
                datum: DatumEntry {
                    value: result.removed_datum,
                    persistent: result.persistent,
                },
                binding: result.matched_datum,
            })
            .collect(),
    }
}

pub fn rendezvous_of_candidate<C: Term, P: Term, A: Term, K: Term>(
    candidate: &ProduceCandidate<C, P, A, K>,
) -> Rendezvous<C, P, A, K> {
    let waiting = candidate.continuation();
    Rendezvous {
        channels: candidate.channels().to_vec(),
        continuation: ContinuationEntry {
            patterns: waiting.patterns.as_ref().clone(),
            continuation: waiting.continuation.as_ref().clone(),
            persistent: waiting.persist,
            peeks: waiting.peeks.clone(),
        },
        selection: candidate
            .data_candidates()
            .iter()
            .map(|data| Selected {
                channel: data.channel.clone(),
                datum: DatumEntry {
                    value: data.removed_datum.as_ref().clone(),
                    persistent: data.datum.persist,
                },
                binding: data.datum.a.as_ref().clone(),
            })
            .collect(),
    }
}

pub fn contents_of_rows<C: Term, P: Term, A: Term, K: Term>(
    rows: HashMap<Vec<C>, Row<P, A, K>>,
) -> Result<Contents<C, P, A, K>, String> {
    let mut contents = Contents::default();
    for (channels, row) in rows {
        match channels.as_slice() {
            [channel] => {
                for datum in &row.data {
                    contents.add_datum(channel.clone(), DatumEntry {
                        value: datum.a.as_ref().clone(),
                        persistent: datum.persist,
                    });
                }
            }
            _ if row.data.is_empty() => {}
            _ => {
                return Err(format!(
                    "the row of the channel list {channels:?} holds data {:?}, which belong to \
                     single channels",
                    row.data
                ));
            }
        }
        for waiting in &row.wks {
            contents.add_continuation(channels.clone(), ContinuationEntry {
                patterns: waiting.patterns.as_ref().clone(),
                continuation: waiting.continuation.as_ref().clone(),
                persistent: waiting.persist,
                peeks: waiting.peeks.clone(),
            });
        }
    }
    Ok(contents)
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Pattern {
    #[default]
    Wildcard,
    StringMatch(String),
}

impl StableHashSerialize for Pattern {}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Guard {
    #[default]
    Unguarded,
    AtMost(i64),
    StrictlyIncreasing,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GuardedContinuation {
    pub name: String,
    pub guard: Guard,
}

impl StableHashSerialize for GuardedContinuation {}

impl ColdStoreDecode for Pattern {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        legacy_prefix(bytes)
    }
}

impl ColdStoreDecode for GuardedContinuation {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        legacy_prefix(bytes)
    }
}

fn as_int(datum: &str) -> i64 {
    datum
        .parse::<i64>()
        .unwrap_or_else(|error| panic!("test data are decimal integers: {datum:?}: {error}"))
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GuardingMatch;

impl Match<Pattern, String, GuardedContinuation> for GuardingMatch {
    fn get(&self, pattern: &Pattern, data: &String) -> Option<String> {
        match pattern {
            Pattern::Wildcard => Some(data.clone()),
            Pattern::StringMatch(value) if value == data => Some(data.clone()),
            Pattern::StringMatch(_) => None,
        }
    }

    fn check_commit(&self, continuation: &GuardedContinuation, matched: &[&String]) -> bool {
        match &continuation.guard {
            Guard::Unguarded => true,
            Guard::AtMost(bound) => matched.iter().all(|datum| as_int(datum) <= *bound),
            Guard::StrictlyIncreasing => matched
                .windows(2)
                .all(|pair| as_int(pair[0]) < as_int(pair[1])),
        }
    }
}

pub type TestOperation = Operation<String, Pattern, String, GuardedContinuation>;
pub type TestRendezvous = Rendezvous<String, Pattern, String, GuardedContinuation>;
pub type TestContents = Contents<String, Pattern, String, GuardedContinuation>;
pub type TestOutcome = Outcome<String, Pattern, String, GuardedContinuation>;
pub type TestReference<'m> = Reference<'m, String, Pattern, String, GuardedContinuation>;
pub type TestMatch = dyn Match<Pattern, String, GuardedContinuation>;
type TestCandidate = ProduceCandidate<String, Pattern, String, GuardedContinuation>;
type TestObservedComm = ObservedComm<String, Pattern, String, GuardedContinuation>;
type TestHotStore = dyn HotStore<String, Pattern, String, GuardedContinuation>;

pub trait SpaceUnderTest: ISpace<String, Pattern, String, GuardedContinuation> {
    fn enabled_candidates(&self) -> Vec<TestCandidate>;

    fn stage(&self, operation: &TestOperation);
}

impl SpaceUnderTest for RSpace<String, Pattern, String, GuardedContinuation> {
    fn enabled_candidates(&self) -> Vec<TestCandidate> { self.enabled_rendezvous() }

    fn stage(&self, operation: &TestOperation) { stage_into(&**self.get_store(), operation) }
}

impl SpaceUnderTest for ReplayRSpace<String, Pattern, String, GuardedContinuation> {
    fn enabled_candidates(&self) -> Vec<TestCandidate> { self.enabled_rendezvous() }

    fn stage(&self, operation: &TestOperation) { stage_into(&**self.get_store(), operation) }
}

fn stage_into(store: &TestHotStore, operation: &TestOperation) {
    match operation {
        Operation::Produce {
            channel,
            datum,
            persistent,
        } => store.put_datum(channel, Datum::create(channel, datum.clone(), *persistent)),
        Operation::Consume {
            channels,
            patterns,
            continuation,
            persistent,
            peeks,
        } => {
            store.put_continuation(
                channels,
                WaitingContinuation::create(
                    channels,
                    patterns,
                    continuation,
                    *persistent,
                    peeks.clone(),
                ),
            );
            for channel in channels {
                store.put_join(channel, channels);
            }
        }
        Operation::Install {
            channels,
            patterns,
            continuation,
        } => {
            store.install_continuation(
                channels,
                WaitingContinuation::create(
                    channels,
                    patterns,
                    continuation,
                    true,
                    BTreeSet::new(),
                ),
            );
            for channel in channels {
                store.install_join(channel, channels);
            }
        }
    }
}

pub struct SpacePair {
    pub play: Box<dyn SpaceUnderTest>,
    pub replay: Box<dyn SpaceUnderTest>,
}

#[derive(Clone, Debug)]
pub struct ConformancePlan {
    pub trace_seeds: Range<u64>,
    pub operations_per_trace: usize,
    pub staged_seeds: Range<u64>,
    pub reissue_depth_limit: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StepKind {
    Produce,
    PersistentProduce,
    SingleConsume,
    JoinedConsume,
    RepeatedChannelConsume,
    PersistentConsume,
    PeekingConsume,
    GuardedConsume,
    Install,
}

impl StepKind {
    pub const ALL: [StepKind; 9] = [
        StepKind::Produce,
        StepKind::PersistentProduce,
        StepKind::SingleConsume,
        StepKind::JoinedConsume,
        StepKind::RepeatedChannelConsume,
        StepKind::PersistentConsume,
        StepKind::PeekingConsume,
        StepKind::GuardedConsume,
        StepKind::Install,
    ];

    fn of(operation: &TestOperation) -> Vec<StepKind> {
        match operation {
            Operation::Produce {
                persistent: false, ..
            } => vec![StepKind::Produce],
            Operation::Produce {
                persistent: true, ..
            } => vec![StepKind::PersistentProduce],
            Operation::Consume {
                channels,
                continuation,
                persistent,
                peeks,
                ..
            } => {
                let arity_kind = match channels.len() {
                    1 => StepKind::SingleConsume,
                    _ => StepKind::JoinedConsume,
                };
                [
                    (true, arity_kind),
                    (has_repeated_channel(channels), StepKind::RepeatedChannelConsume),
                    (*persistent, StepKind::PersistentConsume),
                    (!peeks.is_empty(), StepKind::PeekingConsume),
                    (!matches!(continuation.guard, Guard::Unguarded), StepKind::GuardedConsume),
                ]
                .into_iter()
                .filter_map(|(applies, kind)| applies.then_some(kind))
                .collect()
            }
            Operation::Install { .. } => vec![StepKind::Install],
        }
    }
}

fn has_repeated_channel(channels: &[String]) -> bool {
    channels
        .iter()
        .enumerate()
        .any(|(position, channel)| channels[..position].contains(channel))
}

#[derive(Clone, Debug, Default)]
pub struct StepCounts {
    pub fired: usize,
    pub rested: usize,
}

const KEPT_VIOLATIONS: usize = 8;

#[derive(Clone, Debug, Default)]
pub struct PropertyTally {
    pub checks: usize,
    pub violations: usize,
    pub first_violations: Vec<String>,
}

impl PropertyTally {
    fn record(&mut self, holds: bool, describe: impl FnOnce() -> String) {
        self.checks += 1;
        if !holds {
            self.violations += 1;
            if self.first_violations.len() < KEPT_VIOLATIONS {
                self.first_violations.push(describe());
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ModeReport {
    pub traces: usize,
    pub completed_traces: usize,
    pub steps: usize,
    pub reissued_steps: usize,
    pub dropped_reissues: usize,
    pub comms: usize,
    pub rests: usize,
    pub kinds: BTreeMap<StepKind, StepCounts>,
    pub enabled_set_sizes: BTreeMap<usize, usize>,
    pub fired_installed: usize,
    pub rejected_by_guard: usize,
    pub membership: PropertyTally,
    pub enabled_sets: PropertyTally,
    pub contents: PropertyTally,
    pub failures: Vec<String>,
}

impl ModeReport {
    fn tally_kinds(&mut self, operation: &TestOperation, fired: bool) {
        for kind in StepKind::of(operation) {
            let counts = self.kinds.entry(kind).or_default();
            if fired {
                counts.fired += 1;
            } else {
                counts.rested += 1;
            }
        }
        if fired {
            self.comms += 1;
        } else {
            self.rests += 1;
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct StagedReport {
    pub states: usize,
    pub non_empty_states: usize,
    pub members: usize,
    pub repeated_occurrence_states: usize,
    pub rejected_by_guard: usize,
    pub enabled_sets: PropertyTally,
    pub contents: PropertyTally,
}

#[derive(Clone, Debug, Default)]
pub struct ConformanceReport {
    pub play: ModeReport,
    pub replay: ModeReport,
    pub staged_play: StagedReport,
    pub staged_replay: StagedReport,
}

#[derive(Clone, Copy, Debug)]
struct TraceLabel {
    mode: &'static str,
    seed: u64,
}

impl Display for TraceLabel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} seed {}", self.mode, self.seed)
    }
}

struct StepContext<'i> {
    label: TraceLabel,
    step: usize,
    installed: &'i BTreeSet<GuardedContinuation>,
}

impl Display for StepContext<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} step {}", self.label, self.step)
    }
}

enum StepVerdict {
    Continue(Vec<TestOperation>),
    Stop,
}

async fn execute(
    space: &dyn SpaceUnderTest,
    operation: &TestOperation,
) -> Result<Option<TestObservedComm>, String> {
    match operation {
        Operation::Produce {
            channel,
            datum,
            persistent,
        } => space
            .produce(channel.clone(), datum.clone(), *persistent)
            .await
            .map(|fired| {
                fired.map(|(continuation, results, _)| observed_comm(continuation, results))
            })
            .map_err(|error| format!("produce failed: {error}")),
        Operation::Consume {
            channels,
            patterns,
            continuation,
            persistent,
            peeks,
        } => space
            .consume(
                channels.clone(),
                patterns.clone(),
                continuation.clone(),
                *persistent,
                peeks.clone(),
            )
            .await
            .map(|fired| fired.map(|(continuation, results)| observed_comm(continuation, results)))
            .map_err(|error| format!("consume failed: {error}")),
        Operation::Install {
            channels,
            patterns,
            continuation,
        } => match space
            .install(channels.clone(), patterns.clone(), continuation.clone())
            .await
        {
            Ok(None) => Ok(None),
            Ok(Some(fired)) => Err(format!("install returned {fired:?} instead of resting")),
            Err(error) => Err(format!("install failed: {error}")),
        },
    }
}

async fn lockstep_step(
    space: &dyn SpaceUnderTest,
    reference: &mut TestReference<'_>,
    operation: &TestOperation,
    context: &StepContext<'_>,
    report: &mut ModeReport,
) -> StepVerdict {
    let enumeration = reference.enabled_for(operation);
    report.rejected_by_guard += enumeration.rejected_by_guard;
    *report
        .enabled_set_sizes
        .entry(enumeration.members.len())
        .or_default() += 1;
    let issued_again = match execute(space, operation).await {
        Err(error) => {
            report
                .failures
                .push(format!("{context}: {operation:?}: {error}"));
            return StepVerdict::Stop;
        }
        Ok(None) => {
            report
                .membership
                .record(enumeration.members.is_empty(), || {
                    format!(
                        "{context}: {operation:?} rested although the reference enables {:?}",
                        enumeration.members
                    )
                });
            report.tally_kinds(operation, false);
            reference.rest(operation);
            Vec::new()
        }
        Ok(Some(observed)) => {
            let matching: Vec<&TestRendezvous> = enumeration
                .members
                .iter()
                .filter(|member| member.observed() == observed)
                .collect();
            match matching.as_slice() {
                [member] => {
                    report.membership.record(true, String::new);
                    report.tally_kinds(operation, true);
                    if context
                        .installed
                        .contains(&member.continuation.continuation)
                    {
                        report.fired_installed += 1;
                    }
                    reference.perform(operation, member).issued_again
                }
                [] => {
                    report.membership.record(false, || {
                        format!(
                            "{context}: {operation:?} fired {observed:?}, which is not a member \
                             of the reference's enabled set {:?}",
                            enumeration.members
                        )
                    });
                    return StepVerdict::Stop;
                }
                several => {
                    report.failures.push(format!(
                        "{context}: {operation:?} fired {observed:?}, which matches {} members",
                        several.len()
                    ));
                    return StepVerdict::Stop;
                }
            }
        }
    };
    let production_enabled: BTreeSet<TestRendezvous> = space
        .enabled_candidates()
        .iter()
        .map(rendezvous_of_candidate)
        .collect();
    let reference_enabled = reference.enabled_at_rest().members;
    report.enabled_sets.record(
        production_enabled.is_empty() && production_enabled == reference_enabled,
        || {
            format!(
                "{context}: after {operation:?} enabled_rendezvous() is {production_enabled:?} \
                 and the reference's E(S) is {reference_enabled:?}"
            )
        },
    );
    let production_contents = contents_of_rows(space.to_map().await);
    report
        .contents
        .record(production_contents.as_ref() == Ok(reference.contents()), || {
            format!(
                "{context}: after {operation:?} the space holds {production_contents:?} and the \
                 reference holds {:?}",
                reference.contents()
            )
        });
    StepVerdict::Continue(issued_again)
}

async fn run_trace(
    space: &dyn SpaceUnderTest,
    matcher: &TestMatch,
    program: &[TestOperation],
    reissue_depth_limit: usize,
    label: TraceLabel,
    report: &mut ModeReport,
) -> bool {
    report.traces += 1;
    let installed: BTreeSet<GuardedContinuation> = program
        .iter()
        .filter_map(|operation| match operation {
            Operation::Install { continuation, .. } => Some(continuation.clone()),
            Operation::Produce { .. } | Operation::Consume { .. } => None,
        })
        .collect();
    let mut reference = TestReference::new(matcher);
    let mut queue: VecDeque<(TestOperation, usize)> = program
        .iter()
        .map(|operation| (operation.clone(), 0))
        .collect();
    let mut step = 0;
    while let Some((operation, depth)) = queue.pop_front() {
        step += 1;
        report.steps += 1;
        if depth > 0 {
            report.reissued_steps += 1;
        }
        let context = StepContext {
            label,
            step,
            installed: &installed,
        };
        match lockstep_step(space, &mut reference, &operation, &context, report).await {
            StepVerdict::Stop => return false,
            StepVerdict::Continue(issued_again) if depth < reissue_depth_limit => {
                for issued in issued_again.into_iter().rev() {
                    queue.push_front((issued, depth + 1));
                }
            }
            StepVerdict::Continue(issued_again) => report.dropped_reissues += issued_again.len(),
        }
    }
    report.completed_traces += 1;
    true
}

async fn compare_staged_state(
    space: &dyn SpaceUnderTest,
    matcher: &TestMatch,
    staged: &[TestOperation],
    label: TraceLabel,
    report: &mut StagedReport,
) {
    let mut reference = TestReference::new(matcher);
    for operation in staged {
        space.stage(operation);
        reference.rest(operation);
    }
    report.states += 1;
    let production_contents = contents_of_rows(space.to_map().await);
    report
        .contents
        .record(production_contents.as_ref() == Ok(reference.contents()), || {
            format!(
                "{label}: the staged space holds {production_contents:?} and the reference holds \
                 {:?}",
                reference.contents()
            )
        });
    let production_enabled: BTreeSet<TestRendezvous> = space
        .enabled_candidates()
        .iter()
        .map(rendezvous_of_candidate)
        .collect();
    let reference_enabled = reference.enabled_at_rest();
    report.rejected_by_guard += reference_enabled.rejected_by_guard;
    report.members += reference_enabled.members.len();
    if !reference_enabled.members.is_empty() {
        report.non_empty_states += 1;
        if reference.contents().has_repeated_occurrence() {
            report.repeated_occurrence_states += 1;
        }
    }
    report
        .enabled_sets
        .record(production_enabled == reference_enabled.members, || {
            format!(
                "{label}: enabled_rendezvous() is {production_enabled:?} and the reference's E(S) \
                 is {:?}",
                reference_enabled.members
            )
        });
}

pub async fn run_conformance<Factory>(
    factory: Factory,
    matcher: &TestMatch,
    plan: &ConformancePlan,
) -> ConformanceReport
where
    Factory: AsyncFn() -> SpacePair,
{
    let mut report = ConformanceReport::default();
    for seed in plan.trace_seeds.clone() {
        let pair = factory().await;
        let program = generate_program(seed, plan.operations_per_trace);
        let start_root = pair.play.get_root().await;
        let play_label = TraceLabel { mode: "play", seed };
        let play_completed = run_trace(
            &*pair.play,
            matcher,
            &program,
            plan.reissue_depth_limit,
            play_label,
            &mut report.play,
        )
        .await;
        if !play_completed {
            report
                .replay
                .failures
                .push(format!("replay seed {seed}: not run because play stopped"));
            continue;
        }
        let log = pair.play.take_event_log().await;
        if let Err(error) = pair.replay.rig_and_reset(start_root, log).await {
            report
                .replay
                .failures
                .push(format!("replay seed {seed}: rig_and_reset failed: {error}"));
            continue;
        }
        let replay_label = TraceLabel {
            mode: "replay",
            seed,
        };
        let replay_completed = run_trace(
            &*pair.replay,
            matcher,
            &program,
            plan.reissue_depth_limit,
            replay_label,
            &mut report.replay,
        )
        .await;
        if replay_completed {
            if let Err(error) = pair.replay.check_replay_data().await {
                report
                    .replay
                    .failures
                    .push(format!("replay seed {seed}: check_replay_data failed: {error}"));
            }
        }
    }
    for seed in plan.staged_seeds.clone() {
        let pair = factory().await;
        let staged = generate_staged_state(seed);
        let play_label = TraceLabel {
            mode: "staged play",
            seed,
        };
        compare_staged_state(&*pair.play, matcher, &staged, play_label, &mut report.staged_play)
            .await;
        let replay_label = TraceLabel {
            mode: "staged replay",
            seed,
        };
        compare_staged_state(
            &*pair.replay,
            matcher,
            &staged,
            replay_label,
            &mut report.staged_replay,
        )
        .await;
    }
    report
}

const CHANNELS: [&str; 3] = ["a", "b", "c"];
const VALUES: [&str; 4] = ["1", "2", "3", "4"];
const PROGRAM_PERSISTENCE: (u64, u64) = (3, 20);
const STAGED_PERSISTENCE: (u64, u64) = (1, 5);

struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self { Self { state: seed } }

    fn next_word(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut word = self.state;
        word = (word ^ (word >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        word = (word ^ (word >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        word ^ (word >> 31)
    }

    fn below(&mut self, bound: u64) -> u64 { self.next_word() % bound }

    fn chance(&mut self, (numerator, denominator): (u64, u64)) -> bool {
        self.below(denominator) < numerator
    }

    fn pick<'i, T>(&mut self, items: &'i [T]) -> &'i T {
        &items[self.below(items.len() as u64) as usize]
    }
}

fn random_channels(random: &mut SplitMix64, arity: usize) -> Vec<String> {
    (0..arity)
        .map(|_| random.pick(&CHANNELS).to_string())
        .collect()
}

fn random_patterns(random: &mut SplitMix64, arity: usize) -> Vec<Pattern> {
    (0..arity)
        .map(|_| match random.chance((1, 2)) {
            true => Pattern::Wildcard,
            false => Pattern::StringMatch(random.pick(&VALUES).to_string()),
        })
        .collect()
}

fn random_guard(random: &mut SplitMix64) -> Guard {
    match random.below(5) {
        0..=2 => Guard::Unguarded,
        3 => Guard::AtMost(1 + random.below(4) as i64),
        _ => Guard::StrictlyIncreasing,
    }
}

fn random_send(random: &mut SplitMix64, persistence: (u64, u64)) -> TestOperation {
    Operation::Produce {
        channel: random.pick(&CHANNELS).to_string(),
        datum: random.pick(&VALUES).to_string(),
        persistent: random.chance(persistence),
    }
}

fn random_receive(random: &mut SplitMix64, name: String, persistence: (u64, u64)) -> TestOperation {
    let arity = match random.below(20) {
        0..=9 => 1,
        10..=16 => 2,
        _ => 3,
    };
    let channels = random_channels(random, arity);
    let patterns = random_patterns(random, arity);
    let guard = random_guard(random);
    let persistent = random.chance(persistence);
    let peeks = match !persistent && random.chance((1, 5)) {
        true => (0..arity).map(position_index).collect(),
        false => BTreeSet::new(),
    };
    Operation::Consume {
        channels,
        patterns,
        continuation: GuardedContinuation { name, guard },
        persistent,
        peeks,
    }
}

fn random_install(random: &mut SplitMix64, name: String, guard: Guard) -> TestOperation {
    let arity = 1 + random.below(2) as usize;
    let channels = random_channels(random, arity);
    let patterns = random_patterns(random, arity);
    Operation::Install {
        channels,
        patterns,
        continuation: GuardedContinuation { name, guard },
    }
}

pub fn generate_program(seed: u64, operations: usize) -> Vec<TestOperation> {
    let mut random = SplitMix64::new(seed);
    let install_attempts = random.below(3) as usize;
    let mut program = Vec::with_capacity(install_attempts + operations);
    let mut installed_lists = BTreeSet::new();
    for index in 0..install_attempts {
        let install = random_install(&mut random, format!("install-{index}"), Guard::Unguarded);
        if let Operation::Install { channels, .. } = &install {
            if installed_lists.insert(channels.clone()) {
                program.push(install);
            }
        }
    }
    for index in 0..operations {
        let operation = match random.chance((1, 2)) {
            true => random_send(&mut random, PROGRAM_PERSISTENCE),
            false => random_receive(&mut random, format!("receive-{index}"), PROGRAM_PERSISTENCE),
        };
        program.push(operation);
    }
    program
}

pub fn generate_staged_state(seed: u64) -> Vec<TestOperation> {
    let mut random = SplitMix64::new(seed);
    let data = random.below(7) as usize;
    let receives = 1 + random.below(3) as usize;
    let install = random.chance((1, 4));
    let mut staged = Vec::with_capacity(data + receives + usize::from(install));
    for _ in 0..data {
        staged.push(random_send(&mut random, STAGED_PERSISTENCE));
    }
    for index in 0..receives {
        staged.push(random_receive(
            &mut random,
            format!("staged-receive-{index}"),
            STAGED_PERSISTENCE,
        ));
    }
    if install {
        let guard = random_guard(&mut random);
        staged.push(random_install(&mut random, "staged-install".to_string(), guard));
    }
    staged
}
