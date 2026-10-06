// Shared harness of the Reified RSpaces substrate tests: a capturing
// accounting observer, a pattern matcher, space constructors, and the
// host-guided agent that turns a host operation's outcome into the staged
// space operation with the same outcome. After a peek fire the agent follows
// HEAD's peek path (`head_peek_path`). The fire removes every bound
// non-persistent record, as the host COMM does. Each non-persistent result
// is then produced again as a next produce of the transaction, as the
// reducer's `produce_peeks` produces it again: the agent stores a removed
// record again (the re-store, decision 45, Rule E), and it offers a
// non-persistent incoming datum again (the re-offer, FR-10, decision 30).
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use rspace_plus_plus::rspace::checkpoint::Checkpoint;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::hot_store::{HotStore, HotStoreInstances};
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, WaitingContinuation};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::operation_context::OperationOrder;
use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
use rspace_plus_plus::rspace::reporting_rspace::ReportingRspace;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::{
    ContResult, ISpace, MaybeConsumeResult, MaybeProduceResult, RSpaceAccountingObserver,
    RSpaceOperationCompletion, RSpaceOperationSource, RSpaceResult, ReplayOperationDirective,
};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::space_store_handle::{
    BeforeObserve, BoxedSpaceTxn, ConsumeOutcome, ProduceOutcome, SpaceConsume, SpaceProduce,
    SpaceStoreHandle, SpaceTxnError, SpaceTxnReceipt, StagedOp, bind_candidate, incoming_candidate,
};
use rspace_plus_plus::rspace::trace::event::{COMM, Consume, Event, IOEvent, Produce};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Pat {
    #[default]
    Any,
    Exact(String),
}

pub struct PatMatch;

impl Match<Pat, String, String> for PatMatch {
    fn get(&self, pattern: &Pat, datum: &String) -> Option<String> {
        match pattern {
            Pat::Any => Some(datum.clone()),
            Pat::Exact(expected) => (expected == datum).then(|| datum.clone()),
        }
    }
}

pub type Space = RSpace<String, Pat, String, String>;
pub type Replay = ReplayRSpace<String, Pat, String, String>;
pub type Reporting = ReportingRspace<String, Pat, String, String>;
pub type Handle = SpaceStoreHandle<String, Pat, String, String>;
pub type Txn = BoxedSpaceTxn<String, Pat, String, String>;
pub type Receipt = SpaceTxnReceipt<String, Pat, String, String>;
pub type Candidate = ConsumeCandidate<String, String>;
pub type Commits = Vec<Result<Receipt, SpaceTxnError>>;
/// Data, continuations and joins as bytes, in request order.
pub type ChannelView = (Vec<Vec<Vec<u8>>>, Vec<Vec<Vec<u8>>>, Vec<Vec<Vec<String>>>);
/// Recorded bindings: (IO event bytes, sorted (COMM bytes, multiplicity)).
pub type Bindings = Vec<(Vec<u8>, Vec<(Vec<u8>, usize)>)>;

/// Every channel the tests use; transactions lock all of them.
pub const UNIVERSE: [&str; 6] = ["a", "b", "c", "d", "v1", "v2"];

pub fn universe() -> Vec<String> { UNIVERSE.iter().map(|channel| channel.to_string()).collect() }

pub fn s(value: &str) -> String { value.to_string() }

pub fn names(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

/// The channels of `values` in canonical order: sorted (non-decreasing) by
/// `stable_hash_provider::hash`, so a repeated channel stays repeated. In the
/// test universe that order is `b < d < c < a < v1 < v2`.
pub fn canonical(values: &[&str]) -> Vec<String> {
    let mut channels = names(values);
    channels.sort_by_key(hash);
    channels
}

/// The canonical-order rejection of a group, computed here independently of
/// the substrate (FR-1'): a group that is not sorted (non-decreasing) by
/// channel hash. A sorted group may repeat a channel.
pub fn group_rejection(channels: &[String]) -> Option<SpaceTxnError> {
    let keys: Vec<Blake2b256Hash> = channels.iter().map(hash).collect();
    match keys.windows(2).all(|pair| pair[0] <= pair[1]) {
        true => None,
        false => Some(SpaceTxnError::NonCanonicalChannelOrder),
    }
}

/// One observer or callback call, with sources as bincode bytes of their IO
/// event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    BeforeObserve {
        source: Vec<u8>,
    },
    Directive {
        source: Vec<u8>,
    },
    Start {
        source: Vec<u8>,
        channels: Vec<String>,
        joins: Vec<Vec<String>>,
    },
    Produce {
        source: Vec<u8>,
        channel: String,
        data: String,
        persistent: bool,
    },
    Consume {
        source: Vec<u8>,
        channels: Vec<String>,
        patterns: Vec<Pat>,
        continuation: String,
        persistent: bool,
        peeks: BTreeSet<i32>,
    },
    Comm {
        comm: Vec<u8>,
        continuation: String,
        persistent: bool,
        data: Vec<(String, bool)>,
    },
    Finish {
        source: Vec<u8>,
        completion: RSpaceOperationCompletion,
    },
}

pub fn source_bytes(source: RSpaceOperationSource<'_>) -> Vec<u8> {
    let event = match source {
        RSpaceOperationSource::Produce(produce) => IOEvent::Produce(produce.clone()),
        RSpaceOperationSource::Consume(consume) => IOEvent::Consume(consume.clone()),
    };
    bincode::serialize(&event).expect("serialize the operation source")
}

/// Capturing accounting observer. It can reject the n-th COMM or the n-th
/// fallible call, and can return a replay directive.
#[derive(Default)]
pub struct Recorder {
    calls: Mutex<Vec<Call>>,
    orders: Mutex<Vec<Option<OperationOrder>>>,
    comms: Mutex<usize>,
    reject_comm_at: Mutex<Option<usize>>,
    fallible: Mutex<usize>,
    reject_call_at: Mutex<Option<usize>>,
    directive: Mutex<Option<ReplayOperationDirective>>,
    directive_at: Mutex<Option<usize>>,
    directives: Mutex<usize>,
}

impl Recorder {
    pub fn new() -> Arc<Self> { Arc::new(Self::default()) }

    pub fn rejecting_comm(index: usize) -> Arc<Self> {
        let recorder = Self::default();
        *recorder.reject_comm_at.lock().expect("reject lock") = Some(index);
        Arc::new(recorder)
    }

    /// Rejects the `index`-th fallible call with `OutOfPhlogistons`, as a
    /// deploy that runs out of phlo at that call. The fallible calls are the
    /// operation starts, the introductions and the COMMs, in call order.
    /// Directive queries do not count: play makes none, and replay makes one
    /// per operation, so play and replay reject the same charge.
    pub fn rejecting_call(index: usize) -> Arc<Self> {
        let recorder = Self::default();
        *recorder.reject_call_at.lock().expect("reject lock") = Some(index);
        Arc::new(recorder)
    }

    /// The fallible calls made so far, rejected ones included.
    pub fn fallible_calls(&self) -> usize { *self.fallible.lock().expect("fallible calls lock") }

    // Counts one fallible call and rejects it when `rejecting_call` names it.
    fn charge(&self) -> Result<(), RSpaceError> {
        let index = {
            let mut fallible = self.fallible.lock().expect("fallible calls lock");
            let index = *fallible;
            *fallible += 1;
            index
        };
        match *self.reject_call_at.lock().expect("reject lock") {
            Some(reject) if reject == index => Err(RSpaceError::OutOfPhlogistons),
            _ => Ok(()),
        }
    }

    pub fn with_directive(directive: ReplayOperationDirective) -> Arc<Self> {
        let recorder = Self::default();
        *recorder.directive.lock().expect("directive lock") = Some(directive);
        Arc::new(recorder)
    }

    /// Returns `directive` only at the `index`-th directive query.
    pub fn with_directive_at(index: usize, directive: ReplayOperationDirective) -> Arc<Self> {
        let recorder = Self::default();
        *recorder.directive.lock().expect("directive lock") = Some(directive);
        *recorder.directive_at.lock().expect("directive index lock") = Some(index);
        Arc::new(recorder)
    }

    pub fn calls(&self) -> Vec<Call> { self.calls.lock().expect("calls lock").clone() }

    pub fn orders(&self) -> Vec<Option<OperationOrder>> {
        self.orders.lock().expect("orders lock").clone()
    }

    fn push(&self, call: Call) { self.calls.lock().expect("calls lock").push(call); }

    /// A callback that records its call, then returns `result`.
    pub fn before_observe(self: &Arc<Self>, result: Result<(), RSpaceError>) -> BeforeObserve {
        let recorder = Arc::clone(self);
        Box::new(move |source: RSpaceOperationSource<'_>| {
            recorder.push(Call::BeforeObserve {
                source: source_bytes(source),
            });
            result
        })
    }
}

impl RSpaceAccountingObserver<String, Pat, String, String> for Recorder {
    fn replay_operation_directive(
        &self,
        source: RSpaceOperationSource<'_>,
    ) -> Result<Option<ReplayOperationDirective>, RSpaceError> {
        self.push(Call::Directive {
            source: source_bytes(source),
        });
        let mut directives = self.directives.lock().expect("directives lock");
        let index = *directives;
        *directives += 1;
        match *self.directive_at.lock().expect("directive index lock") {
            Some(at) if at != index => Ok(None),
            _ => Ok(self.directive.lock().expect("directive lock").clone()),
        }
    }

    fn observe_operation_start(
        &self,
        source: RSpaceOperationSource<'_>,
        channels: &[String],
        joins: &[Vec<String>],
    ) -> Result<(), RSpaceError> {
        self.push(Call::Start {
            source: source_bytes(source),
            channels: channels.to_vec(),
            joins: joins.to_vec(),
        });
        self.orders
            .lock()
            .expect("orders lock")
            .push(rspace_plus_plus::rspace::operation_context::current());
        self.charge()
    }

    fn observe_operation_finish(
        &self,
        source: RSpaceOperationSource<'_>,
        completion: RSpaceOperationCompletion,
    ) {
        self.push(Call::Finish {
            source: source_bytes(source),
            completion,
        });
    }

    fn observe_produce(
        &self,
        source: &Produce,
        channel: &String,
        data: &String,
        persistent: bool,
    ) -> Result<(), RSpaceError> {
        self.push(Call::Produce {
            source: source_bytes(RSpaceOperationSource::Produce(source)),
            channel: channel.clone(),
            data: data.clone(),
            persistent,
        });
        self.charge()
    }

    fn observe_consume(
        &self,
        source: &Consume,
        channels: &[String],
        patterns: &[Pat],
        continuation: &String,
        persistent: bool,
        peeks: &BTreeSet<i32>,
    ) -> Result<(), RSpaceError> {
        self.push(Call::Consume {
            source: source_bytes(RSpaceOperationSource::Consume(source)),
            channels: channels.to_vec(),
            patterns: patterns.to_vec(),
            continuation: continuation.clone(),
            persistent,
            peeks: peeks.clone(),
        });
        self.charge()
    }

    fn observe_comm(
        &self,
        comm: &COMM,
        continuation: &String,
        continuation_persistent: bool,
        data: &[(&String, bool)],
    ) -> Result<(), RSpaceError> {
        self.push(Call::Comm {
            comm: bincode::serialize(comm).expect("serialize the observed COMM"),
            continuation: continuation.clone(),
            persistent: continuation_persistent,
            data: data
                .iter()
                .map(|(datum, persistent)| ((*datum).clone(), *persistent))
                .collect(),
        });
        let index = {
            let mut comms = self.comms.lock().expect("comms lock");
            let index = *comms;
            *comms += 1;
            index
        };
        let charged = self.charge();
        match *self.reject_comm_at.lock().expect("reject lock") {
            Some(reject) if reject == index => Err(RSpaceError::OutOfPhlogistons),
            _ => charged,
        }
    }
}

pub fn observed<S: ISpace<String, Pat, String, String>>(space: &S, recorder: &Arc<Recorder>) {
    space.set_accounting_observer(Some(recorder.clone()));
}

pub async fn play_space() -> Space {
    let mut stores = InMemoryStoreManager::new();
    let store = stores
        .r_space_stores()
        .await
        .expect("create the in-memory RSpace stores");
    RSpace::create(store, Arc::new(Box::new(PatMatch))).expect("create the RSpace")
}

/// A play space and a replay space over one history repository.
pub async fn play_and_replay() -> (Space, Replay) {
    let mut stores = InMemoryStoreManager::new();
    let store = stores
        .r_space_stores()
        .await
        .expect("create the in-memory RSpace stores");
    RSpace::create_with_replay(store, Arc::new(Box::new(PatMatch)))
        .expect("create the play and replay spaces")
}

/// A replay space over `play`'s history, with its own hot store.
pub fn replay_over(play: &Space) -> Replay {
    let history = play.get_history_repository();
    let reader = history
        .get_history_reader(&history.root())
        .expect("read the history root");
    let store = HotStoreInstances::create_from_hr(reader.base());
    ReplayRSpace::apply(history, Arc::new(store), Arc::new(Box::new(PatMatch)))
}

/// A reporting space over `play`'s history, with its own hot store.
pub fn reporting_over(play: &Space) -> Reporting {
    let history = play.get_history_repository();
    let reader = history
        .get_history_reader(&history.root())
        .expect("read the history root");
    let store = HotStoreInstances::create_from_hr(reader.base());
    ReportingRspace::apply(history, Arc::new(store), Arc::new(Box::new(PatMatch)))
}

/// A host-level operation over the test universe.
#[derive(Clone, Debug)]
pub enum Op {
    Produce {
        channel: String,
        data: String,
        persist: bool,
    },
    Consume {
        channels: Vec<String>,
        patterns: Vec<Pat>,
        continuation: String,
        persist: bool,
        peek: bool,
    },
}

impl Op {
    pub fn produce(channel: &str, data: &str, persist: bool) -> Op {
        Op::Produce {
            channel: s(channel),
            data: s(data),
            persist,
        }
    }

    pub fn consume(channels: &[&str], patterns: Vec<Pat>, continuation: &str, persist: bool) -> Op {
        Op::Consume {
            channels: names(channels),
            patterns,
            continuation: s(continuation),
            persist,
            peek: false,
        }
    }

    pub fn peek(channels: &[&str], patterns: Vec<Pat>, continuation: &str) -> Op {
        Op::Consume {
            channels: names(channels),
            patterns,
            continuation: s(continuation),
            persist: false,
            peek: true,
        }
    }

    pub fn peeks(&self) -> BTreeSet<i32> {
        match self {
            Op::Consume {
                channels,
                peek: true,
                ..
            } => (0..channels.len() as i32).collect(),
            _ => BTreeSet::new(),
        }
    }

    pub fn is_peek(&self) -> bool { matches!(self, Op::Consume { peek: true, .. }) }

    /// The error with which the substrate must reject this operation: a
    /// consume on a group that is not sorted (a permutation, or a repeat out
    /// of order).
    pub fn rejection(&self) -> Option<SpaceTxnError> {
        match self {
            Op::Consume { channels, .. } => group_rejection(channels),
            Op::Produce { .. } => None,
        }
    }

    /// The operation staged as a park (the outcome of a rejected group).
    pub fn park(&self) -> Planned {
        match self {
            Op::Consume {
                channels,
                patterns,
                continuation,
                persist,
                ..
            } => Planned::Consume {
                channels: channels.clone(),
                patterns: patterns.clone(),
                continuation: continuation.clone(),
                persist: *persist,
                peeks: self.peeks(),
                outcome: ConsumeOutcome::Park,
            },
            Op::Produce {
                channel,
                data,
                persist,
            } => Planned::Produce {
                channel: channel.clone(),
                data: data.clone(),
                persist: *persist,
                outcome: ProduceOutcome::Store,
            },
        }
    }
}

/// Outcome of a host operation.
#[derive(Clone, Debug)]
pub enum HostOutcome {
    Produced(MaybeProduceResult<String, Pat, String, String>),
    Consumed(MaybeConsumeResult<String, Pat, String, String>),
}

pub async fn run_host<S: ISpace<String, Pat, String, String>>(
    space: &S,
    op: &Op,
) -> Result<HostOutcome, RSpaceError> {
    match op {
        Op::Produce {
            channel,
            data,
            persist,
        } => space
            .produce(channel.clone(), data.clone(), *persist)
            .await
            .map(HostOutcome::Produced),
        Op::Consume {
            channels,
            patterns,
            continuation,
            persist,
            ..
        } => space
            .consume(channels.clone(), patterns.clone(), continuation.clone(), *persist, op.peeks())
            .await
            .map(HostOutcome::Consumed),
    }
}

/// A space operation with its outcome decided; clonable so one plan can
/// run in play, replay and reporting.
#[derive(Clone, Debug)]
pub enum Planned {
    Produce {
        channel: String,
        data: String,
        persist: bool,
        outcome: ProduceOutcome<String, Pat, String, String>,
    },
    Consume {
        channels: Vec<String>,
        patterns: Vec<Pat>,
        continuation: String,
        persist: bool,
        peeks: BTreeSet<i32>,
        outcome: ConsumeOutcome<String, String>,
    },
    /// An operation that the substrate must reject with `error`; staging it
    /// has no effect.
    Rejected {
        plan: Box<Planned>,
        error: SpaceTxnError,
    },
}

impl Planned {
    pub fn stage(
        &self,
        txn: &mut Txn,
        before_observe: Option<BeforeObserve>,
    ) -> Result<StagedOp, SpaceTxnError> {
        match self.clone() {
            Planned::Produce {
                channel,
                data,
                persist,
                outcome,
            } => txn.produce(SpaceProduce {
                channel,
                data,
                persist,
                outcome,
                before_observe,
            }),
            Planned::Consume {
                channels,
                patterns,
                continuation,
                persist,
                peeks,
                outcome,
            } => txn.consume(SpaceConsume {
                channels,
                patterns,
                continuation,
                persist,
                peeks,
                outcome,
                before_observe,
            }),
            Planned::Rejected { plan, .. } => plan.stage(txn, before_observe),
        }
    }

    /// Stages the plan and checks the expected result: success, or the
    /// rejection a `Rejected` plan names.
    pub fn stage_expected(&self, txn: &mut Txn) -> Result<(), String> {
        match self {
            Planned::Rejected { plan, error } => match plan.stage(txn, None) {
                Err(actual) if actual == *error => Ok(()),
                other => Err(format!("expected {:?}, got {:?}", error, other)),
            },
            plan => plan
                .stage(txn, None)
                .map(|_| ())
                .map_err(|error| format!("stage {:?}: {:?}", plan, error)),
        }
    }

    pub fn is_rejected(&self) -> bool { matches!(self, Planned::Rejected { .. }) }

    pub fn is_fire(&self) -> bool {
        matches!(
            self,
            Planned::Produce {
                outcome: ProduceOutcome::Fire { .. },
                ..
            } | Planned::Consume {
                outcome: ConsumeOutcome::Fire { .. },
                ..
            }
        )
    }
}

/// The waiting continuation the host fired, rebuilt from its result.
pub fn fired_continuation(
    cont: &ContResult<String, Pat, String>,
) -> WaitingContinuation<Pat, String> {
    let peeks = match cont.peek {
        true => (0..cont.channels.len() as i32).collect(),
        false => BTreeSet::new(),
    };
    WaitingContinuation::create(
        &cont.channels,
        &cont.patterns,
        &cont.continuation,
        cont.persistent,
        peeks,
    )
}

/// Position of the result that binds the incoming datum of a produce: the
/// first result with the produced channel, value and persistence. A stored
/// record that equals the incoming datum has the same bytes and source, so
/// either one may take this position.
pub fn incoming_position(
    results: &[RSpaceResult<String, String>],
    channel: &String,
    data: &String,
    persist: bool,
) -> Option<usize> {
    results.iter().position(|result| {
        result.channel == *channel && result.removed_datum == *data && result.persistent == persist
    })
}

/// Candidates for the host's results, bound to the staged view of `txn`.
/// `incoming` is the produced record of a produce-triggered fire.
pub fn candidates_from_results(
    txn: &mut Txn,
    results: &[RSpaceResult<String, String>],
    incoming: Option<(&String, &String, bool)>,
) -> Vec<Candidate> {
    let incoming_at = incoming
        .and_then(|(channel, data, persist)| incoming_position(results, channel, data, persist));
    let mut claimed: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    let mut candidates = Vec::with_capacity(results.len());
    for (index, result) in results.iter().enumerate() {
        if let (Some((channel, data, persist)), true) = (incoming, incoming_at == Some(index)) {
            candidates.push(incoming_candidate(
                result.channel.clone(),
                Produce::create(channel, data, persist),
                data.clone(),
                persist,
                result.matched_datum.clone(),
            ));
            continue;
        }
        let source = Produce::create(&result.channel, &result.removed_datum, result.persistent);
        let records = txn.data(&result.channel).expect("read the staged data");
        let claims = claimed.entry(result.channel.clone()).or_default();
        let position = records
            .iter()
            .enumerate()
            .position(|(index, record)| {
                record.a == result.removed_datum &&
                    record.persist == result.persistent &&
                    record.source == source &&
                    (result.persistent || !claims.contains(&index))
            })
            .expect("the record the host matched is in the staged view");
        let stored = records[position].clone();
        if !result.persistent {
            claims.insert(position);
        }
        candidates.push(bind_candidate(
            result.channel.clone(),
            &stored,
            result.matched_datum.clone(),
            position as i32,
        ));
    }
    candidates
}

/// The space operation with the host's outcome, bound to `txn`'s staged view.
pub fn plan_from_host(txn: &mut Txn, op: &Op, outcome: &HostOutcome) -> Planned {
    match (op, outcome) {
        (
            Op::Produce {
                channel,
                data,
                persist,
            },
            HostOutcome::Produced(result),
        ) => {
            let outcome = match result {
                None => ProduceOutcome::Store,
                Some((cont, results, _)) => ProduceOutcome::Fire {
                    channels: cont.channels.clone(),
                    continuation: fired_continuation(cont),
                    data: candidates_from_results(txn, results, Some((channel, data, *persist))),
                },
            };
            Planned::Produce {
                channel: channel.clone(),
                data: data.clone(),
                persist: *persist,
                outcome,
            }
        }
        (
            Op::Consume {
                channels,
                patterns,
                continuation,
                persist,
                ..
            },
            HostOutcome::Consumed(result),
        ) => {
            let outcome = match result {
                None => ConsumeOutcome::Park,
                Some((_, results)) => ConsumeOutcome::Fire {
                    data: candidates_from_results(txn, results, None),
                },
            };
            Planned::Consume {
                channels: channels.clone(),
                patterns: patterns.clone(),
                continuation: continuation.clone(),
                persist: *persist,
                peeks: op.peeks(),
                outcome,
            }
        }
        _ => panic!("the host outcome does not belong to the operation"),
    }
}

/// The most host operations that one operation may start: itself and its
/// re-produces. A re-produce follows a peek fire. That fire is the operation
/// itself (a consume) or removes a non-persistent peek waiter (a produce),
/// and it binds at most three records (two channels, one of them repeated).
/// A run of the generated groups has at most 32 operations, so at most 32
/// parked peek waiters, and one operation starts at most
/// 1 + 3 * (1 + 32) = 100 host operations, below this bound.
pub const REPRODUCE_ROUNDS: usize = 128;

// Disabled by decision 45 (Rule E): the space keeps no peeked record, so the
// oracle produces every non-persistent result again instead of putting the
// stored records back into the host without events.
//
// ```
// /// The most produces that one operation may start: itself and its
// /// re-offers. Every re-offer follows a fire that removed a non-persistent
// /// peek waiter, so the generated operations stay far below this bound.
// pub const REOFFER_ROUNDS: usize = 64;
//
// /// HEAD's peek path after `op` ran on `host` with `outcome`
// /// (reduce.rs:776-803 and 973-1008). The host removed every bound
// /// non-persistent record of a peek fire, and the reducer produces each one
// /// again (`produce_peeks`). The space keeps the stored records (retention),
// /// so the oracle puts them back into the host store without events or
// /// observer calls. It returns the re-produce of a non-persistent incoming
// /// datum: the host runs it as the reducer does, and the space agent stages
// /// it as the next produce of the transaction, with the same identity
// /// (FR-10). A persistent incoming datum is produced again by the
// /// persistent-send loop, as after every persistent fire, which no oracle
// /// of these tests models.
// pub fn head_peek_path(host: &Space, op: &Op, outcome: &HostOutcome) -> Option<Op> {
//     let (results, incoming_at) = match (op, outcome) {
//         (
//             Op::Produce {
//                 channel,
//                 data,
//                 persist,
//             },
//             HostOutcome::Produced(Some((cont, results, _))),
//         ) if cont.peek => (results, incoming_position(results, channel, data, *persist)),
//         (Op::Consume { .. }, HostOutcome::Consumed(Some((cont, results)))) if cont.peek => {
//             (results, None)
//         }
//         _ => return None,
//     };
//     let store = host.get_store();
//     let mut reproduce = None;
//     for (index, result) in results.iter().enumerate() {
//         match (incoming_at == Some(index), result.persistent) {
//             (true, false) => {
//                 reproduce = Some(Op::produce(&result.channel, &result.removed_datum, false));
//             }
//             (false, false) => store.put_datum(&result.channel, Datum {
//                 a: result.removed_datum.clone(),
//                 persist: false,
//                 source: Produce::create(&result.channel, &result.removed_datum, false),
//             }),
//             (_, true) => {}
//         }
//     }
//     reproduce
// }
// ```

/// HEAD's peek path after `op` ran with `outcome` (reduce.rs
/// `continue_produce_process` and `continue_consume_process`, which call
/// `produce_peeks`, reduce.rs:981-1016). The host COMM of a peek fire
/// removed every bound non-persistent record, and the reducer produces each
/// non-persistent result again, in result order, with the removed data and
/// `persist = false`. Returns these re-produces. The host runs each one as
/// the reducer does. The space agent stages each one as a next produce of
/// the transaction, with the same identity: the re-store of a removed record
/// (decision 45, Rule E) or the re-offer of a non-persistent incoming datum
/// (FR-10, decision 30). The guided agent stages the host's outcome of each
/// re-produce: a store, or a fire when the re-produce completes a match. A
/// persistent result is not produced again: the fire does not remove a
/// persistent record, and the persistent-send loop produces a persistent
/// incoming datum again, as after every persistent fire, which no oracle of
/// these tests models.
pub fn head_peek_path(op: &Op, outcome: &HostOutcome) -> Vec<Op> {
    let results = match (op, outcome) {
        (Op::Produce { .. }, HostOutcome::Produced(Some((cont, results, _)))) if cont.peek => {
            results
        }
        (Op::Consume { .. }, HostOutcome::Consumed(Some((cont, results)))) if cont.peek => results,
        _ => return Vec::new(),
    };
    results
        .iter()
        .filter(|result| !result.persistent)
        .map(|result| Op::produce(&result.channel, &result.removed_datum, false))
        .collect()
}

/// Runs `op` on `host` and stages the same outcome in `txn`, then follows
/// HEAD's peek path: each re-produce runs on the host and is staged with the
/// host's outcome as a next produce of `txn`. The re-produces of an
/// operation come right after it, in result order, each one followed by its
/// own peek path (depth first).
pub async fn guide_op(host: &Space, txn: &mut Txn, op: &Op, planned: &mut Vec<Planned>) {
    let mut pending = Vec::with_capacity(REPRODUCE_ROUNDS);
    pending.push(op.clone());
    let mut rounds = 0usize;
    while let Some(op) = pending.pop() {
        rounds += 1;
        assert!(rounds <= REPRODUCE_ROUNDS, "the re-produces of one operation do not end");
        let outcome = run_host(host, &op).await.expect("run the host operation");
        let plan = plan_from_host(txn, &op, &outcome);
        plan.stage(txn, None).expect("stage the planned operation");
        planned.push(plan);
        pending.extend(head_peek_path(&op, &outcome).into_iter().rev());
    }
}

/// Runs `groups` on `host` one host operation at a time and on `txn_space`
/// as one transaction per group, staged from the host outcomes. A peek fire
/// follows HEAD's peek path (`head_peek_path`), so a group's plans include
/// the re-produces: the re-stores and the re-offers. Returns the plans and
/// each group's commit result.
pub async fn run_guided(
    host: &Space,
    txn_space: &Space,
    groups: &[Vec<Op>],
) -> (Vec<Vec<Planned>>, Commits) {
    let handle = SpaceStoreHandle::new(txn_space.clone());
    let lock_keys = universe();
    let mut plans = Vec::with_capacity(groups.len());
    let mut receipts = Vec::with_capacity(groups.len());
    for group in groups {
        let mut txn = handle.begin(&lock_keys).await.expect("begin a transaction");
        let mut planned = Vec::with_capacity(group.len());
        for op in group {
            if let Some(error) = op.rejection() {
                let plan = Planned::Rejected {
                    plan: Box::new(op.park()),
                    error,
                };
                plan.stage_expected(&mut txn)
                    .expect("the substrate rejects the operation");
                planned.push(plan);
                continue;
            }
            guide_op(host, &mut txn, op, &mut planned).await;
        }
        receipts.push(txn.commit().await);
        plans.push(planned);
    }
    (plans, receipts)
}

/// Plans `groups` on scratch spaces from the empty state.
pub async fn plan_groups(groups: &[Vec<Op>]) -> Vec<Vec<Planned>> {
    let host = play_space().await;
    let txn_space = play_space().await;
    run_guided(&host, &txn_space, groups).await.0
}

/// Runs planned transactions through `handle`; each group is one transaction.
pub async fn run_plans(handle: &Handle, groups: &[Vec<Planned>]) -> Commits {
    let lock_keys = universe();
    let mut receipts = Vec::with_capacity(groups.len());
    for group in groups {
        let mut txn = handle.begin(&lock_keys).await.expect("begin a transaction");
        for plan in group {
            plan.stage_expected(&mut txn)
                .expect("stage the planned operation");
        }
        receipts.push(txn.commit().await);
    }
    receipts
}

/// Runs planned transactions through `handle` until the first failure, as a
/// deploy ends at its first error: a staging error or a commit error ends
/// the run. Each group is one transaction. A `Rejected` plan must fail with
/// its error, which has no effect.
pub async fn run_until_failure(handle: &Handle, groups: &[Vec<Planned>]) -> Commits {
    let lock_keys = universe();
    let mut commits = Vec::with_capacity(groups.len());
    for group in groups {
        let mut txn = handle.begin(&lock_keys).await.expect("begin a transaction");
        let mut staged = Ok(());
        for plan in group {
            staged = match plan.is_rejected() {
                true => {
                    plan.stage_expected(&mut txn)
                        .expect("the substrate rejects the operation");
                    Ok(())
                }
                false => plan.stage(&mut txn, None).map(|_| ()),
            };
            if staged.is_err() {
                break;
            }
        }
        let commit = match staged {
            Ok(()) => txn.commit().await,
            Err(error) => Err(error),
        };
        let failed = commit.is_err();
        commits.push(commit);
        if failed {
            break;
        }
    }
    commits
}

/// The calls of `calls` without the directive queries, which only replay
/// makes.
pub fn without_directives(calls: Vec<Call>) -> Vec<Call> {
    calls
        .into_iter()
        .filter(|call| !matches!(call, Call::Directive { .. }))
        .collect()
}

/// The `index`-th fallible call of `calls` (see `Recorder::rejecting_call`).
pub fn fallible_call(calls: &[Call], index: usize) -> Option<&Call> {
    calls
        .iter()
        .filter(|call| match call {
            Call::Start { .. } |
            Call::Produce { .. } |
            Call::Consume { .. } |
            Call::Comm { .. } => true,
            Call::BeforeObserve { .. } | Call::Directive { .. } | Call::Finish { .. } => false,
        })
        .nth(index)
}

/// Runs `ops` as host operations.
pub async fn run_host_ops<S: ISpace<String, Pat, String, String>>(space: &S, ops: &[Op]) {
    for op in ops {
        run_host(space, op).await.expect("run the host operation");
    }
}

/// The host-executable operations of `groups`: every operation the
/// substrate does not reject, in order.
pub fn flatten(groups: &[Vec<Op>]) -> Vec<Op> {
    groups
        .iter()
        .flatten()
        .filter(|op| op.rejection().is_none())
        .cloned()
        .collect()
}

/// Events as tagged bincode bytes (`Event` itself is not serializable).
pub fn event_bytes(log: &[Event]) -> Vec<Vec<u8>> {
    log.iter()
        .map(|event| match event {
            Event::IoEvent(io_event) => {
                let mut bytes = vec![0u8];
                bytes.extend(bincode::serialize(io_event).expect("serialize an IO event"));
                bytes
            }
            Event::Comm(comm) => {
                let mut bytes = vec![1u8];
                bytes.extend(bincode::serialize(comm).expect("serialize a COMM"));
                bytes
            }
        })
        .collect()
}

pub fn comms_of(log: &[Event]) -> Vec<COMM> {
    log.iter()
        .filter_map(|event| match event {
            Event::Comm(comm) => Some(comm.clone()),
            Event::IoEvent(_) => None,
        })
        .collect()
}

/// Non-empty hot-store entries, records as bincode bytes in list order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogicalState {
    pub data: BTreeMap<String, Vec<Vec<u8>>>,
    pub continuations: BTreeMap<Vec<String>, Vec<Vec<u8>>>,
    pub joins: BTreeMap<String, Vec<Vec<String>>>,
}

impl LogicalState {
    /// The same state with every list sorted (multiset view).
    pub fn sorted(&self) -> LogicalState {
        let mut sorted = self.clone();
        sorted.data.values_mut().for_each(|records| records.sort());
        sorted
            .continuations
            .values_mut()
            .for_each(|records| records.sort());
        sorted.joins.values_mut().for_each(|groups| groups.sort());
        sorted
    }
}

pub fn store_state(store: &dyn HotStore<String, Pat, String, String>) -> LogicalState {
    let snapshot = store.snapshot();
    LogicalState {
        data: snapshot
            .data_flat()
            .into_iter()
            .filter(|(_, records)| !records.is_empty())
            .map(|(channel, records)| {
                let bytes = records
                    .iter()
                    .map(|record| bincode::serialize(record).expect("serialize a datum"))
                    .collect();
                (channel, bytes)
            })
            .collect(),
        continuations: snapshot
            .continuations_flat()
            .into_iter()
            .filter(|(_, waiting)| !waiting.is_empty())
            .map(|(group, waiting)| {
                let bytes = waiting
                    .iter()
                    .map(|wk| bincode::serialize(wk).expect("serialize a continuation"))
                    .collect();
                (group, bytes)
            })
            .collect(),
        joins: snapshot
            .joins_flat()
            .into_iter()
            .filter(|(_, groups)| !groups.is_empty())
            .collect(),
    }
}

pub fn logical_state(space: &Space) -> LogicalState { store_state(&**space.get_store()) }

pub fn replay_state(space: &Replay) -> LogicalState { store_state(&**space.get_store()) }

/// The contents of `channels` and `groups` through the `ISpace` getters.
pub async fn channel_view<S: ISpace<String, Pat, String, String>>(
    space: &S,
    channels: &[String],
    groups: &[Vec<String>],
) -> ChannelView {
    let mut data = Vec::with_capacity(channels.len());
    let mut joins = Vec::with_capacity(channels.len());
    for channel in channels {
        data.push(
            space
                .get_data(channel)
                .await
                .iter()
                .map(|record| bincode::serialize(record).expect("serialize a datum"))
                .collect(),
        );
        joins.push(space.get_joins(channel.clone()).await);
    }
    let mut continuations = Vec::with_capacity(groups.len());
    for group in groups {
        continuations.push(
            space
                .get_waiting_continuations(group.clone())
                .await
                .iter()
                .map(|wk| bincode::serialize(wk).expect("serialize a continuation"))
                .collect(),
        );
    }
    (data, continuations, joins)
}

/// Every join group of the universe in the given spaces, plus the singletons.
pub async fn groups_of(spaces: &[&Space]) -> Vec<Vec<String>> {
    let mut groups: BTreeSet<Vec<String>> = universe().into_iter().map(|c| vec![c]).collect();
    for space in spaces {
        for channel in universe() {
            groups.extend(space.get_joins(channel).await);
        }
    }
    groups.into_iter().collect()
}

pub async fn checkpoint<S: ISpace<String, Pat, String, String>>(space: &S) -> Checkpoint {
    space
        .create_checkpoint()
        .await
        .expect("create a checkpoint")
}

/// Rigs `replay` with the bindings of `log` and resets it to `start`.
pub async fn rig(replay: &Replay, start: &Blake2b256Hash, log: &[Event]) {
    replay
        .rig_and_reset(start.clone(), log.to_vec())
        .await
        .expect("rig the replay space");
}

/// The recorded bindings of a replay space, sorted.
pub fn replay_bindings(replay: &Replay) -> Bindings {
    let replay_data = replay.replay_data.lock().expect("replay data lock");
    let mut bindings: Bindings = replay_data
        .map
        .iter()
        .map(|entry| {
            let mut comms: Vec<(Vec<u8>, usize)> = entry
                .value()
                .iter()
                .map(|(comm, count)| (bincode::serialize(comm).expect("serialize a COMM"), *count))
                .collect();
            comms.sort();
            (bincode::serialize(entry.key()).expect("serialize an IO event"), comms)
        })
        .collect();
    bindings.sort();
    bindings
}

pub fn receipts_ok(receipts: Commits) -> Vec<Receipt> {
    receipts
        .into_iter()
        .map(|receipt| receipt.expect("commit the transaction"))
        .collect()
}

/// Every continuation key of the hot store, including the keys whose list
/// is empty (a read leaves such an entry).
pub fn continuation_keys(
    store: &dyn HotStore<String, Pat, String, String>,
) -> BTreeSet<Vec<String>> {
    store.snapshot().continuations_flat().into_keys().collect()
}

/// `log` without any IO event equal to `removed`.
pub fn without_event(log: &[Event], removed: &IOEvent) -> Vec<Event> {
    log.iter()
        .filter(|event| !matches!(event, Event::IoEvent(io_event) if io_event == removed))
        .cloned()
        .collect()
}

/// The message of a caught panic.
pub fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(text) => *text,
        Err(payload) => match payload.downcast::<&'static str>() {
            Ok(text) => text.to_string(),
            Err(_) => "<a panic without a message>".to_string(),
        },
    }
}
