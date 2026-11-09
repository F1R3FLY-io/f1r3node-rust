//! A recording accounting observer for the host differentials of the native spaces (decision 90,
//! decision 69).
//!
//! The legacy meter is deleted (DR-134), and the legacy charges of decision 45 (Option P) are
//! deleted with it. A host differential now compares the calls of the public accounting observer
//! of RSpace, `RSpaceAccountingObserver`. These calls are the start of each locked operation, its
//! introduction (`observe_produce` or `observe_consume`), each COMM with its participants, and the
//! finish. A [`Recorder`] records each call with its arguments, in call order. A test installs one
//! recorder on the host RSpace of the agent and one on the host RSpace of the host program with
//! `set_accounting_observer` ([`observe`]). Then it compares the two records.
//!
//! The recorder refuses nothing. A start names no recorded row, so the rest of the operation runs
//! without an operation name, as in racing evaluation (DR-132). An owner check admits every item.
//! The recorder reports no owner checks (`owner_checks_active` is false), so RSpace matches as it
//! matches without owner funding (DR-135). That query configures the match and is not a call of
//! the operation, so the recorder does not record it. Host play asks it in each operation, and the
//! space backends never ask it.
//!
//! Play and replay make the same calls with one difference. Replay asks for the directive of each
//! operation (`replay_operation_directive`), and it reports a COMM without its participants
//! (`observe_comm`). [`shared_view`] keeps the calls that both make.
//!
//! The design follows the recorder of the substrate tests (`Call` and `push` in
//! `rspace++/tests/reified_space_store_handle/support.rs`), which a rholang test cannot import. The
//! sources and the COMMs are kept as values, so a failed comparison prints their hashes.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::operation_context::OperationOrder;
use rspace_plus_plus::rspace::rspace_interface::{
    CommParticipant, ISpace, RSpaceAccountingObserver, RSpaceOperationCompletion,
    RSpaceOperationSource, ReplayOperationDirective,
};
use rspace_plus_plus::rspace::trace::event::{Consume, IOEvent, Produce, COMM};

/// One datum of a COMM as `observe_comm_participants` names it: its channel, the value that the
/// continuation binds, the value that the store held, and its persist flag.
#[derive(Clone, Debug, PartialEq)]
pub struct Participant {
    /// The channel of the datum.
    pub channel: Par,
    /// The value that the continuation binds.
    pub matched: ListParWithRandom,
    /// The value that the store held.
    pub stored: ListParWithRandom,
    /// True for a persistent datum.
    pub persistent: bool,
}

/// One call of the accounting observer, with its arguments.
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    /// `replay_operation_directive`: replay asks for the directive of an operation.
    Directive {
        /// The operation.
        source: IOEvent,
    },
    /// `observe_operation_start`: the start of a locked operation.
    Start {
        /// The operation.
        source: IOEvent,
        /// The channels of the operation.
        channels: Vec<Par>,
        /// The join groups of the channel of a produce. A consume has none.
        joins: Vec<Vec<Par>>,
    },
    /// `observe_produce`: the introduction of a produce.
    Produce {
        /// The produce.
        source: Produce,
        /// Its channel.
        channel: Par,
        /// Its datum.
        data: ListParWithRandom,
        /// True for a persistent produce.
        persistent: bool,
    },
    /// `observe_consume`: the introduction of a consume.
    Consume {
        /// The consume.
        source: Consume,
        /// Its channels.
        channels: Vec<Par>,
        /// Its patterns.
        patterns: Vec<BindPattern>,
        /// Its continuation.
        continuation: TaggedContinuation,
        /// True for a persistent consume.
        persistent: bool,
        /// Its peeks.
        peeks: BTreeSet<i32>,
    },
    /// `observe_comm`, or `observe_comm_participants` with the participants of the COMM.
    Comm {
        /// The COMM.
        comm: COMM,
        /// The continuation that the COMM fires.
        continuation: TaggedContinuation,
        /// True for a persistent continuation.
        persistent: bool,
        /// The bound value and the persist flag of each datum.
        data: Vec<(ListParWithRandom, bool)>,
        /// The participants that play names. Replay calls `observe_comm`, so this is `None`.
        participants: Option<Vec<Participant>>,
    },
    /// `admit_item`: an owner check of one item of a match (DR-135).
    AdmitItem {
        /// The continuation of the match.
        continuation: TaggedContinuation,
        /// True for a persistent continuation.
        continuation_persistent: bool,
        /// The number of channels of the continuation.
        channel_count: usize,
        /// The datum.
        datum: ListParWithRandom,
        /// True for a persistent datum.
        datum_persistent: bool,
    },
    /// `observe_operation_finish`: the end of a locked operation.
    Finish {
        /// The operation.
        source: IOEvent,
        /// How the operation ended.
        completion: RSpaceOperationCompletion,
    },
}

/// An accounting observer that records each call with its arguments, in call order.
#[derive(Default)]
pub struct Recorder {
    calls: Mutex<Vec<Call>>,
}

impl Recorder {
    /// A fresh recorder.
    pub fn new() -> Arc<Recorder> { Arc::new(Recorder::default()) }

    /// Every call so far, in call order.
    pub fn calls(&self) -> Vec<Call> { self.calls.lock().expect("the calls lock").clone() }

    /// The number of calls so far. A caller keeps it as the mark of [`Recorder::since`].
    pub fn count(&self) -> usize { self.calls.lock().expect("the calls lock").len() }

    /// The calls after the first `mark` calls, in call order.
    pub fn since(&self, mark: usize) -> Vec<Call> {
        self.calls.lock().expect("the calls lock")[mark..].to_vec()
    }

    /// Every call so far, in call order. The record is empty afterwards.
    pub fn take(&self) -> Vec<Call> {
        std::mem::take(&mut *self.calls.lock().expect("the calls lock"))
    }

    fn push(&self, call: Call) { self.calls.lock().expect("the calls lock").push(call); }
}

/// The operation `source` as an IO event.
fn event_of(source: RSpaceOperationSource<'_>) -> IOEvent {
    match source {
        RSpaceOperationSource::Produce(produce) => IOEvent::Produce(produce.clone()),
        RSpaceOperationSource::Consume(consume) => IOEvent::Consume(consume.clone()),
    }
}

impl RSpaceAccountingObserver<Par, BindPattern, ListParWithRandom, TaggedContinuation>
    for Recorder
{
    fn replay_operation_directive(
        &self,
        source: RSpaceOperationSource<'_>,
    ) -> Result<Option<ReplayOperationDirective>, RSpaceError> {
        self.push(Call::Directive {
            source: event_of(source),
        });
        Ok(None)
    }

    fn observe_operation_start(
        &self,
        source: RSpaceOperationSource<'_>,
        channels: &[Par],
        joins: &[Vec<Par>],
    ) -> Result<Option<OperationOrder>, RSpaceError> {
        self.push(Call::Start {
            source: event_of(source),
            channels: channels.to_vec(),
            joins: joins.to_vec(),
        });
        Ok(None)
    }

    fn observe_operation_finish(
        &self,
        source: RSpaceOperationSource<'_>,
        completion: RSpaceOperationCompletion,
    ) {
        self.push(Call::Finish {
            source: event_of(source),
            completion,
        });
    }

    fn observe_produce(
        &self,
        source: &Produce,
        channel: &Par,
        data: &ListParWithRandom,
        persistent: bool,
    ) -> Result<(), RSpaceError> {
        self.push(Call::Produce {
            source: source.clone(),
            channel: channel.clone(),
            data: data.clone(),
            persistent,
        });
        Ok(())
    }

    fn observe_consume(
        &self,
        source: &Consume,
        channels: &[Par],
        patterns: &[BindPattern],
        continuation: &TaggedContinuation,
        persistent: bool,
        peeks: &BTreeSet<i32>,
    ) -> Result<(), RSpaceError> {
        self.push(Call::Consume {
            source: source.clone(),
            channels: channels.to_vec(),
            patterns: patterns.to_vec(),
            continuation: continuation.clone(),
            persistent,
            peeks: peeks.clone(),
        });
        Ok(())
    }

    fn observe_comm(
        &self,
        comm: &COMM,
        continuation: &TaggedContinuation,
        continuation_persistent: bool,
        data: &[(&ListParWithRandom, bool)],
    ) -> Result<(), RSpaceError> {
        let mut values = Vec::with_capacity(data.len());
        values.extend(
            data.iter()
                .map(|(datum, persistent)| ((*datum).clone(), *persistent)),
        );
        self.push(Call::Comm {
            comm: comm.clone(),
            continuation: continuation.clone(),
            persistent: continuation_persistent,
            data: values,
            participants: None,
        });
        Ok(())
    }

    fn owner_checks_active(&self) -> bool { false }

    fn admit_item(
        &self,
        continuation: &TaggedContinuation,
        continuation_persistent: bool,
        channel_count: usize,
        datum: &ListParWithRandom,
        datum_persistent: bool,
    ) -> Result<bool, RSpaceError> {
        self.push(Call::AdmitItem {
            continuation: continuation.clone(),
            continuation_persistent,
            channel_count,
            datum: datum.clone(),
            datum_persistent,
        });
        Ok(true)
    }

    fn observe_comm_participants(
        &self,
        comm: &COMM,
        continuation: &TaggedContinuation,
        continuation_persistent: bool,
        participants: &[CommParticipant<'_, Par, ListParWithRandom>],
    ) -> Result<(), RSpaceError> {
        let mut values = Vec::with_capacity(participants.len());
        values.extend(
            participants
                .iter()
                .map(|participant| (participant.matched.clone(), participant.persistent)),
        );
        let mut named = Vec::with_capacity(participants.len());
        named.extend(participants.iter().map(|participant| Participant {
            channel: participant.channel.clone(),
            matched: participant.matched.clone(),
            stored: participant.stored.clone(),
            persistent: participant.persistent,
        }));
        self.push(Call::Comm {
            comm: comm.clone(),
            continuation: continuation.clone(),
            persistent: continuation_persistent,
            data: values,
            participants: Some(named),
        });
        Ok(())
    }
}

/// Installs `recorder` as the accounting observer of `space`.
pub fn observe<S>(space: &S, recorder: &Arc<Recorder>)
where S: ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
    space.set_accounting_observer(Some(recorder.clone()));
}

/// A fresh recorder, installed as the accounting observer of `space`.
pub fn observed<S>(space: &S) -> Arc<Recorder>
where S: ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
    let recorder = Recorder::new();
    observe(space, &recorder);
    recorder
}

/// Removes the accounting observer of `space`.
pub fn unobserve<S>(space: &S)
where S: ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
    space.set_accounting_observer(None);
}

/// The operations of `calls`, in call order. Each operation holds the calls from its start to its
/// finish. It panics when a call lies outside an operation or an operation does not finish.
pub fn operations(calls: &[Call]) -> Vec<Vec<Call>> {
    let mut operations: Vec<Vec<Call>> = Vec::new();
    let mut open = false;
    for call in calls {
        match (call, open) {
            (Call::Start { .. }, false) => {
                operations.push(vec![call.clone()]);
                open = true;
            }
            (Call::Start { .. }, true) => panic!("an operation starts inside another: {call:?}"),
            (Call::Finish { .. }, true) => {
                operations
                    .last_mut()
                    .expect("an open operation")
                    .push(call.clone());
                open = false;
            }
            (_, true) => operations
                .last_mut()
                .expect("an open operation")
                .push(call.clone()),
            (_, false) => panic!("a call lies outside an operation: {call:?}"),
        }
    }
    assert!(!open, "every operation finishes");
    operations
}

/// The calls of `calls` with the join groups of each start in canonical order: sorted by their
/// bincode bytes. The hot store lists the join groups of a channel in the order of its own
/// history. The accounting observer of a deploy reads them only as the sorted set of their
/// channels (`locked_footprint` in `accounting/native_runtime/operations.rs`). So a test that
/// changes only the list order of a store compares the starts in this form.
pub fn canonical_join_order(calls: &[Call]) -> Vec<Call> {
    let mut canonical = Vec::with_capacity(calls.len());
    canonical.extend(calls.iter().map(|call| match call {
        Call::Start {
            source,
            channels,
            joins,
        } => {
            let mut sorted = joins.clone();
            sorted.sort_by_cached_key(|group| {
                bincode::serialize(group).expect("a join group serializes")
            });
            Call::Start {
                source: source.clone(),
                channels: channels.clone(),
                joins: sorted,
            }
        }
        other => other.clone(),
    }));
    canonical
}

/// Added by decision 92: true when `calls` are exactly the observer calls of `count` anchors
/// (`spaces::anchor`). The config channel of the space is `config`, and its space counter is
/// `footprint`. Each anchor is one consume operation: a start on `[config]` with the footprint
/// group, the introduction of a non-persistent consume without peeks, the COMM of the config
/// record, and a matched finish. A job that fails at a position of the state makes these calls
/// only.
pub fn anchor_calls_only(calls: &[Call], config: &Par, footprint: &Par, count: usize) -> bool {
    let anchors = operations(calls);
    anchors.len() == count
        && anchors.iter().all(|anchor| match anchor.as_slice() {
            [Call::Start {
                source: IOEvent::Consume(_),
                channels,
                joins,
            }, Call::Consume {
                channels: introduced,
                persistent: false,
                peeks,
                ..
            }, Call::Comm { .. }, Call::Finish {
                completion: RSpaceOperationCompletion::Matched,
                ..
            }] => {
                channels.as_slice() == std::slice::from_ref(config)
                    && joins.as_slice() == [vec![footprint.clone()]]
                    && introduced.as_slice() == std::slice::from_ref(config)
                    && peeks.is_empty()
            }
            _ => false,
        })
}

/// Added by decision 92: the calls `calls` of a space without the footprint group of each start.
/// Every row of a space transaction names the space counter `S(sid)` as one more join group, the
/// last one, of its start (`footprint_joins` in `space_store_handle.rs`). So the rows of one
/// space form one chain in play order (`SpaceReplayOrder.tla`). A host program has no such group.
/// The function panics when a start does not end with the group `[footprint]`, so a comparison
/// with the host checks that group too.
pub fn without_footprint(calls: &[Call], footprint: &Par) -> Vec<Call> {
    let mut plain = Vec::with_capacity(calls.len());
    plain.extend(calls.iter().map(|call| match call {
        Call::Start {
            source,
            channels,
            joins,
        } => match joins.split_last() {
            Some((last, rest)) if last.as_slice() == std::slice::from_ref(footprint) => {
                Call::Start {
                    source: source.clone(),
                    channels: channels.clone(),
                    joins: rest.to_vec(),
                }
            }
            _ => panic!("a start of a space row names the footprint group last: {call:?}"),
        },
        other => other.clone(),
    }));
    plain
}

/// The calls of `calls` that play and replay both make, with the arguments that both pass: every
/// call except the directive queries of replay, and each COMM without its participants.
pub fn shared_view(calls: &[Call]) -> Vec<Call> {
    let mut shared = Vec::with_capacity(calls.len());
    shared.extend(calls.iter().filter_map(|call| match call {
        Call::Directive { .. } => None,
        Call::Comm {
            comm,
            continuation,
            persistent,
            data,
            participants: _,
        } => Some(Call::Comm {
            comm: comm.clone(),
            continuation: continuation.clone(),
            persistent: *persistent,
            data: data.clone(),
            participants: None,
        }),
        other => Some(other.clone()),
    }));
    shared
}
