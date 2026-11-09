//! Added by decision 123 (plan v4, section 10.1, V3-1): the parity of the
//! shared replay selection with the base selection (D-OBL-4).
//!
//! The four `base_` methods are verbatim copies of the base selection bodies
//! before decision 123. They differ only in their names and in the call of
//! the copied consume selection. Each case runs a base body and its wrapper
//! on one store, each with a fresh reader. The case compares the results in
//! every field, and it compares the reader calls: one continuation read and
//! one view build for each group that the selection reaches.

use std::cell::RefCell;

use super::super::tests::Logical;
use super::*;
use crate::rspace::candidate_order::canonical_candidates;
use crate::rspace::hot_store::native_store::NativeHotStore;
use crate::rspace::rspace::RSpace;
use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;

impl<C, P, A, K> ReplayRSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    /// The base `prepare_consume_candidate`. Only the name differs.
    pub(in crate::rspace::replay_rspace) fn base_prepare_consume_candidate(
        &self,
        channels: &[C],
        keys: &[StoreKey],
        patterns: &[P],
        continuation: &K,
        consume: &Consume,
        peeks: &BTreeSet<i32>,
        expected: Option<&dyn NativeCandidateIdentity>,
        // Added by G3 (DR-135, S6): the owner checks of the operation and the
        // persistence of the consume (R5, R7).
        owner: Option<(&dyn NativeOwnerFilter<C, P, A, K>, bool)>,
        reader: &CandidateReader<'_, C, P, A, K>,
    ) -> Result<Option<PreparedConsumeCandidate<C, A>>> {
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // native_backing::inspect(consume, reader.meter)?;
        if let Some(identity) = expected {
            if !identity.matches_consume(consume) {
                return Ok(None);
            }
        }
        // Added by DR-129 (E3-a): with no recorded identity this call is the
        // Stored check (`native_session/operations.rs`). It skips a
        // continuation whose commit can veto, because play, in dev's random
        // order, may have tried another tuple and stored.
        if expected.is_none() && self.matcher.commit_may_veto(continuation) {
            return Ok(None);
        }
        // Added by G3 (DR-135, S6): the Stored check also skips a consume
        // whose COMM the row refused. The refusal is verified on its own
        // (R7, SA4).
        if expected.is_none() && owner.is_some_and(|(filter, _)| filter.names_refusal_for(consume))
        {
            return Ok(None);
        }
        // Changed by C2 (DR-82): the channel data borrow copy-free views.
        // let data = self.metered_channel_data(channels, None, expected, reader)?;
        let views = self.data_views(channels, keys, reader)?;
        // Changed by D-D4 (DR-106): the match reads the recorded positions.
        // let data =
        //     self.metered_channel_data(channels, &views, None, expected, reader)?;
        // let Some(data) = self.metered_match_data(
        //     channels,
        //     patterns,
        //     continuation,
        //     &data,
        //     reader.meter,
        // )?
        // else {
        //     return Ok(None);
        // };
        let (data, positions) = self.channel_data(channels, &views, None, expected);
        // Changed by G3 (DR-135, S6): with the item test of the owner checks.
        // let Some(data) =
        //     self.match_data(channels, &positions, patterns, continuation, &data)?
        // else {
        //     return Ok(None);
        // };
        let Some(data) =
            self.match_data(channels, &positions, patterns, continuation, &data, owner)?
        else {
            return Ok(None);
        };
        let comm = self.build_comm(&data, consume, peeks, None);
        if let Some(identity) = expected {
            if !identity.matches_comm(&comm) {
                return Ok(None);
            }
        }
        Ok(Some(PreparedConsumeCandidate { data, comm }))
    }

    /// The base `prepare_produce_candidate`. Only the name differs.
    pub(in crate::rspace::replay_rspace) fn base_prepare_produce_candidate(
        &self,
        channel: &C,
        incoming: Datum<A>,
        grouped_channels: Vec<Vec<C>>,
        keys: &OperationKeys,
        expected: Option<&dyn NativeCandidateIdentity>,
        // Added by G3 (DR-135, S6): the owner checks of the operation (R5,
        // R6, R7).
        owner: Option<&dyn NativeOwnerFilter<C, P, A, K>>,
        reader: &CandidateReader<'_, C, P, A, K>,
    ) -> Result<ProduceSelection<C, P, A, K>> {
        // Added by G3 (DR-135, S6): a produce keeps its own datum under owner
        // funding (R6).
        let own_datum = owner.is_some_and(|filter| filter.requires_own_datum());
        if keys.groups.len() != grouped_channels.len() {
            return Err(RSpaceError::BugFoundError(
                "native replay join keys do not match the joins".to_owned(),
            ));
        }
        // Changed by D-D5 (DR-107): the fields of the incoming datum.
        // let next = if persist {
        let next = if incoming.persist {
            None
        } else {
            // let count = self.metered_produce_count(source, reader.meter)?;
            let count = self.get_produce_count(&incoming.source);
            let Some(next) = count.checked_add(1) else {
                let message = "Native replay produce counter overflow";
                return Err(RSpaceError::InterpreterError(message.to_owned()));
            };
            Some(next)
        };
        for (group, (channels, group_keys)) in
            grouped_channels.into_iter().zip(&keys.groups).enumerate()
        {
            let continuations =
                canonical_candidates((reader.continuations)(&channels, group_keys)?);
            // Changed by C2 (DR-82): the channel data borrow copy-free views.
            let views = self.data_views(&channels, &group_keys.channels, reader)?;
            // Changed by D-D4 (DR-106): the match reads the recorded positions.
            // let data = self.metered_channel_data(
            let (data, positions) = self.channel_data(
                &channels,
                &views,
                // Changed by D-D5 (DR-107): the entries borrow the datum.
                // Some((channel, value, persist, source)),
                Some((channel, &incoming)),
                expected,
            );
            // Added by G3 (DR-135, S6): on a channel that the join names
            // once, the patterns read only the incoming datum (R6).
            let mut data = data;
            if own_datum {
                keep_only_incoming(&mut data, &channels, &positions, channel);
            }
            for (continuation, index) in continuations {
                // Changed by D-O1 (DR-108): block accounting charges inline bytes
                // once per enclosing block.
                // native_backing::inspect(&continuation.source, reader.meter)?;
                if let Some(identity) = expected {
                    if !identity.matches_consume(&continuation.source) {
                        continue;
                    }
                }
                // Added by DR-129 (E3-a): the Stored check (no recorded
                // identity) skips a continuation whose commit can veto.
                if expected.is_none() && self.matcher.commit_may_veto(&continuation.continuation) {
                    continue;
                }
                // Added by G3 (DR-135, S6): the Stored check also skips a
                // continuation whose COMM the row refused. The refusal is
                // verified on its own (R7, SA4).
                if expected.is_none() &&
                    owner.is_some_and(|filter| filter.names_refusal_for(&continuation.source))
                {
                    continue;
                }
                let Some(data_candidates) = self.match_data(
                    &channels,
                    &positions,
                    &continuation.patterns,
                    &continuation.continuation,
                    &data,
                    // Added by G3 (DR-135, S6): the item test of the owner
                    // checks (R5).
                    owner.map(|filter| (filter, continuation.persist)),
                )?
                else {
                    continue;
                };
                // Added by G3 (DR-135, S6): on a repeated channel the tuple
                // must hold the incoming datum (R6).
                if own_datum && !holds_incoming(&data_candidates) {
                    continue;
                }
                let comm = self.build_comm(
                    &data_candidates,
                    &continuation.source,
                    &continuation.peeks,
                    // Changed by D-D5 (DR-107): the source of the datum.
                    // next.map(|next| (source, next)),
                    next.map(|next| (&incoming.source, next)),
                );
                if let Some(identity) = expected {
                    if !identity.matches_comm(&comm) {
                        // Changed by D-D5 (DR-107): the datum goes back.
                        // return Ok(None);
                        return Ok(ProduceSelection::Unmatched(incoming));
                    }
                }
                // C1 (DR-81): only the selected continuation is copied.
                // Changed by D-O1 (DR-108): block accounting charges inline bytes
                // once per enclosing block.
                // native_backing::reserve_copy_and_cleanup(
                //     continuation.as_ref(),
                //     reader.meter,
                // )?;
                let continuation = continuation.as_ref().clone();
                // Changed by D-D5 (DR-107): the incoming candidates receive
                // the produce's value, and the source goes back.
                // let candidate = ProduceCandidate {
                //     channels,
                //     continuation,
                //     continuation_index: index,
                //     data_candidates,
                // };
                // return Ok(Some((PreparedProduceCandidate { candidate, comm }, group)));
                let mut data_candidates = data_candidates;
                let Datum {
                    a: value, source, ..
                } = incoming;
                fill_incoming(&mut data_candidates, value);
                let candidate = ProduceCandidate {
                    channels,
                    continuation,
                    continuation_index: index,
                    data_candidates,
                };
                return Ok(ProduceSelection::Matched {
                    prepared: PreparedProduceCandidate { candidate, comm },
                    group,
                    source,
                });
            }
        }
        // Changed by D-D5 (DR-107): the datum goes back.
        // Ok(None)
        Ok(ProduceSelection::Unmatched(incoming))
    }

    /// The base `find_refused_consume_tuple`. Only the name and the call of
    /// `base_prepare_consume_candidate` differ.
    pub(in crate::rspace::replay_rspace) fn base_find_refused_consume_tuple(
        &self,
        channels: &[C],
        keys: &[StoreKey],
        patterns: &[P],
        continuation: &K,
        consume: &Consume,
        peeks: &BTreeSet<i32>,
        identity: &dyn NativeCandidateIdentity,
        reader: &CandidateReader<'_, C, P, A, K>,
    ) -> Result<Option<PreparedConsumeCandidate<C, A>>> {
        self.base_prepare_consume_candidate(
            channels,
            keys,
            patterns,
            continuation,
            consume,
            peeks,
            Some(identity),
            None,
            reader,
        )
    }

    /// The base `find_refused_produce_tuple`. Only the name differs.
    pub(in crate::rspace::replay_rspace) fn base_find_refused_produce_tuple(
        &self,
        channel: &C,
        incoming: &Datum<A>,
        grouped_channels: &[Vec<C>],
        keys: &OperationKeys,
        identity: &dyn NativeCandidateIdentity,
        own_datum: bool,
        reader: &CandidateReader<'_, C, P, A, K>,
    ) -> Result<Option<RefusedProduceTuple<C, P, A, K>>> {
        if keys.groups.len() != grouped_channels.len() {
            return Err(RSpaceError::BugFoundError(
                "native replay join keys do not match the joins".to_owned(),
            ));
        }
        let next = if incoming.persist {
            None
        } else {
            let count = self.get_produce_count(&incoming.source);
            let Some(next) = count.checked_add(1) else {
                let message = "Native replay produce counter overflow";
                return Err(RSpaceError::InterpreterError(message.to_owned()));
            };
            Some(next)
        };
        for (channels, group_keys) in grouped_channels.iter().zip(&keys.groups) {
            let continuations = canonical_candidates((reader.continuations)(channels, group_keys)?);
            let views = self.data_views(channels, &group_keys.channels, reader)?;
            let (mut data, positions) =
                self.channel_data(channels, &views, Some((channel, incoming)), Some(identity));
            if own_datum {
                keep_only_incoming(&mut data, channels, &positions, channel);
            }
            for (continuation, _) in continuations {
                if !identity.matches_consume(&continuation.source) {
                    continue;
                }
                let Some(data_candidates) = self.match_data(
                    channels,
                    &positions,
                    &continuation.patterns,
                    &continuation.continuation,
                    &data,
                    None,
                )?
                else {
                    continue;
                };
                if own_datum && !holds_incoming(&data_candidates) {
                    continue;
                }
                let comm = self.build_comm(
                    &data_candidates,
                    &continuation.source,
                    &continuation.peeks,
                    next.map(|next| (&incoming.source, next)),
                );
                if !identity.matches_comm(&comm) {
                    return Ok(None);
                }
                return Ok(Some(RefusedProduceTuple {
                    continuation: continuation.as_ref().clone(),
                    data: data_candidates,
                    comm,
                }));
            }
        }
        Ok(None)
    }
}

/// A pattern 0 matches every value, and another pattern matches its own
/// value. The commit check vetoes the continuation 255. The commits of 254
/// and 255 can veto (E3-a), although the commit of 254 passes.
struct Matcher;

impl Match<u8, u8, u8> for Matcher {
    fn get(&self, pattern: &u8, value: &u8) -> Option<u8> {
        (*pattern == 0 || pattern == value).then_some(*value)
    }

    fn check_commit(&self, continuation: &u8, _: &[u8]) -> bool { *continuation != 255 }

    fn commit_may_veto(&self, continuation: &u8) -> bool { *continuation >= 254 }
}

/// The owner checks of a replayed operation:
/// - a value in `unfunded` fails its item test (R5),
/// - the item test of a value in `failing` returns an error,
/// - a consume in `refused` names a refused COMM (R7),
/// - `own_datum` keeps the incoming datum of a produce (R6).
#[derive(Default)]
struct Owners {
    unfunded: BTreeSet<u8>,
    failing: BTreeSet<u8>,
    refused: Vec<Consume>,
    own_datum: bool,
}

impl NativeOwnerFilter<u8, u8, u8, u8> for Owners {
    fn admit_item(&self, _: &u8, _: bool, _: usize, datum: &u8, _: bool) -> Result<bool> {
        if self.failing.contains(datum) {
            return Err(RSpaceError::InterpreterError(format!("the item test of {datum} fails")));
        }
        Ok(!self.unfunded.contains(datum))
    }

    fn names_refusal_for(&self, consume: &Consume) -> bool { self.refused.contains(consume) }

    fn requires_own_datum(&self) -> bool { self.own_datum }
}

type Space = ReplayRSpace<u8, u8, u8, u8>;

async fn space() -> Space {
    let mut stores = InMemoryStoreManager::new();
    let stores = stores.r_space_stores().await.expect("in-memory stores");
    RSpace::create_with_replay(stores, Arc::new(Box::new(Matcher)))
        .expect("a replay space")
        .1
}

fn put(space: &Space, channel: u8, value: u8) {
    let datum = Datum::create(&channel, value, false);
    space.increment_produce_counter(&datum.source, false);
    space.get_store().put_datum(&channel, datum);
}

fn waiting(space: &Space, channels: &[u8], patterns: &[u8], body: u8) -> Consume {
    let source = Consume::create(&channels.to_vec(), &patterns.to_vec(), &body, false);
    space
        .get_store()
        .put_continuation(channels, WaitingContinuation {
            patterns: patterns.to_vec(),
            continuation: body,
            persist: false,
            peeks: BTreeSet::new(),
            source: source.clone(),
        });
    source
}

fn channel_keys(channels: &[u8]) -> Vec<StoreKey> {
    channels
        .iter()
        .map(|channel| {
            crate::rspace::hashing::native_source::channel_key(channel).expect("a channel key")
        })
        .collect()
}

fn operation_keys(joins: &[Vec<u8>]) -> OperationKeys {
    OperationKeys::build(joins).expect("join keys")
}

/// One call of the reader: the continuations of a group, or the data view of
/// a channel.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Read {
    Continuations(Vec<u8>),
    Data(u8),
}

/// Runs `action` with a fresh reader over the store of `space`, and returns
/// its result with the calls of the reader.
fn with_reader<T>(
    space: &Space,
    action: impl FnOnce(&CandidateReader<'_, u8, u8, u8, u8>) -> T,
) -> (T, Vec<Read>) {
    let reads = RefCell::new(Vec::new());
    let store: NativeHotStore<u8, u8, u8, u8> = NativeHotStore::new();
    let result = {
        let data = |channel: &u8, key: StoreKey| {
            reads.borrow_mut().push(Read::Data(*channel));
            store.data_view(channel, key, &|| Ok(space.get_store().get_data(channel)))
        };
        let continuations = |channels: &[u8], keys: &GroupKeys| {
            reads
                .borrow_mut()
                .push(Read::Continuations(channels.to_vec()));
            store.continuation_views(channels, keys, &|| {
                Ok(space.get_store().get_continuations(channels))
            })
        };
        action(&CandidateReader {
            data: &data,
            continuations: &continuations,
        })
    };
    (result, reads.into_inner())
}

/// The reads of a consume that reaches its lists: one view build, which
/// reads each channel occurrence.
fn consume_reads(channels: &[u8]) -> Vec<Read> {
    channels
        .iter()
        .map(|channel| Read::Data(*channel))
        .collect()
}

/// The reads of a produce that reaches the first `visited` join groups: for
/// each group one continuation read and one view build.
fn produce_reads(joins: &[Vec<u8>], visited: usize) -> Vec<Read> {
    let groups = &joins[..visited];
    let mut reads = Vec::with_capacity(groups.iter().map(|group| 1 + group.len()).sum());
    for channels in groups {
        reads.push(Read::Continuations(channels.clone()));
        reads.extend(channels.iter().map(|channel| Read::Data(*channel)));
    }
    reads
}

/// Asserts that a base body and its wrapper give one result in every field
/// and make the same reader calls, and that the calls are `reads`.
fn assert_parity<T: Debug>(
    case: &str,
    base: &(T, Vec<Read>),
    shared: &(T, Vec<Read>),
    reads: &[Read],
) {
    assert_eq!(format!("{:?}", shared.0), format!("{:?}", base.0), "{case}: the result");
    assert_eq!(shared.1, base.1, "{case}: the reader calls");
    assert_eq!(shared.1, reads, "{case}: one continuation read and one view build for each group");
}

/// A consume selection: the data and the COMM, nothing, or an error.
type Consumed = std::result::Result<Option<(Vec<ConsumeCandidate<u8, u8>>, COMM)>, String>;

fn consumed(result: Result<Option<PreparedConsumeCandidate<u8, u8>>>) -> Consumed {
    result
        .map(|prepared| prepared.map(|prepared| (prepared.data, prepared.comm)))
        .map_err(|error| format!("{error:?}"))
}

/// A produce selection: the candidate, the COMM, the group and the source of
/// a match, or the incoming datum.
#[derive(Debug)]
enum Produced {
    Matched {
        candidate: ProduceCandidate<u8, u8, u8, u8>,
        comm: COMM,
        group: usize,
        source: Produce,
    },
    Unmatched(Datum<u8>),
}

fn produced(
    result: Result<ProduceSelection<u8, u8, u8, u8>>,
) -> std::result::Result<Produced, String> {
    result
        .map(|selection| match selection {
            ProduceSelection::Matched {
                prepared,
                group,
                source,
            } => Produced::Matched {
                candidate: prepared.candidate,
                comm: prepared.comm,
                group,
                source,
            },
            ProduceSelection::Unmatched(datum) => Produced::Unmatched(datum),
        })
        .map_err(|error| format!("{error:?}"))
}

/// A refused produce tuple: the continuation, the data and the COMM.
type Refused = std::result::Result<
    Option<(WaitingContinuation<u8, u8>, Vec<ConsumeCandidate<u8, u8>>, COMM)>,
    String,
>;

fn refused(result: Result<Option<RefusedProduceTuple<u8, u8, u8, u8>>>) -> Refused {
    result
        .map(|tuple| tuple.map(|tuple| (tuple.continuation, tuple.data, tuple.comm)))
        .map_err(|error| format!("{error:?}"))
}

/// The consume selection of the continuation `body` on `channels`, by the
/// base body and by its wrapper.
fn consume_case(
    space: &Space,
    case: &str,
    channels: &[u8],
    keys: &[StoreKey],
    patterns: &[u8],
    body: u8,
    expected: Option<&dyn NativeCandidateIdentity>,
    owner: Option<(&dyn NativeOwnerFilter<u8, u8, u8, u8>, bool)>,
    reads: &[Read],
) -> Consumed {
    let consume = Consume::create(&channels.to_vec(), &patterns.to_vec(), &body, false);
    let peeks = BTreeSet::new();
    let base = with_reader(space, |reader| {
        consumed(space.base_prepare_consume_candidate(
            channels, keys, patterns, &body, &consume, &peeks, expected, owner, reader,
        ))
    });
    let shared = with_reader(space, |reader| {
        consumed(space.prepare_consume_candidate(
            channels, keys, patterns, &body, &consume, &peeks, expected, owner, reader,
        ))
    });
    assert_parity(case, &base, &shared, reads);
    shared.0
}

/// The refused tuple of the consume of `body` that `identity` names, by the
/// base body and by its wrapper.
fn refused_consume_case(
    space: &Space,
    case: &str,
    channels: &[u8],
    patterns: &[u8],
    body: u8,
    identity: &dyn NativeCandidateIdentity,
    reads: &[Read],
) -> Consumed {
    let consume = Consume::create(&channels.to_vec(), &patterns.to_vec(), &body, false);
    let keys = channel_keys(channels);
    let peeks = BTreeSet::new();
    let base = with_reader(space, |reader| {
        consumed(space.base_find_refused_consume_tuple(
            channels, &keys, patterns, &body, &consume, &peeks, identity, reader,
        ))
    });
    let shared = with_reader(space, |reader| {
        consumed(space.find_refused_consume_tuple(
            channels, &keys, patterns, &body, &consume, &peeks, identity, reader,
        ))
    });
    assert_parity(case, &base, &shared, reads);
    shared.0
}

/// The produce selection of `incoming` on the channel 1, by the base body
/// and by its wrapper.
fn produce_case(
    space: &Space,
    case: &str,
    incoming: &Datum<u8>,
    joins: &[Vec<u8>],
    keys: &OperationKeys,
    expected: Option<&dyn NativeCandidateIdentity>,
    owner: Option<&dyn NativeOwnerFilter<u8, u8, u8, u8>>,
    reads: &[Read],
) -> std::result::Result<Produced, String> {
    let base = with_reader(space, |reader| {
        produced(space.base_prepare_produce_candidate(
            &1,
            incoming.clone(),
            joins.to_vec(),
            keys,
            expected,
            owner,
            reader,
        ))
    });
    let shared = with_reader(space, |reader| {
        produced(space.prepare_produce_candidate(
            &1,
            incoming.clone(),
            joins.to_vec(),
            keys,
            expected,
            owner,
            reader,
        ))
    });
    assert_parity(case, &base, &shared, reads);
    shared.0
}

/// The refused tuple of the produce of `incoming` on the channel 1 that
/// `identity` names, by the base body and by its wrapper.
fn refused_produce_case(
    space: &Space,
    case: &str,
    incoming: &Datum<u8>,
    joins: &[Vec<u8>],
    keys: &OperationKeys,
    identity: &dyn NativeCandidateIdentity,
    own_datum: bool,
    reads: &[Read],
) -> Refused {
    let base = with_reader(space, |reader| {
        refused(space.base_find_refused_produce_tuple(
            &1, incoming, joins, keys, identity, own_datum, reader,
        ))
    });
    let shared = with_reader(space, |reader| {
        refused(
            space
                .find_refused_produce_tuple(&1, incoming, joins, keys, identity, own_datum, reader),
        )
    });
    assert_parity(case, &base, &shared, reads);
    shared.0
}

/// The COMM of a consume selection that matched.
fn consume_comm(result: &Consumed) -> COMM {
    let Ok(Some((_, comm))) = result else {
        panic!("a matched consume selection, not {result:?}");
    };
    comm.clone()
}

/// The COMM and the group of a produce selection that matched. The
/// selection gives back the source of the incoming datum.
fn produce_match(
    result: &std::result::Result<Produced, String>,
    incoming: &Datum<u8>,
) -> (COMM, usize) {
    let Ok(Produced::Matched {
        comm,
        group,
        source,
        ..
    }) = result
    else {
        panic!("a matched produce selection, not {result:?}");
    };
    assert_eq!(source, &incoming.source, "the matched selection gives back the source");
    (comm.clone(), *group)
}

/// Whether a produce selection matched nothing and gave back the incoming
/// datum.
fn unmatched(result: &std::result::Result<Produced, String>, incoming: &Datum<u8>) -> bool {
    matches!(result, Ok(Produced::Unmatched(datum)) if datum == incoming)
}

/// `comm` with a peek that the selection never records, so that the COMM of
/// a match differs from it.
fn with_peek(comm: &COMM) -> COMM {
    let mut changed = comm.clone();
    changed.peeks.insert(0);
    changed
}

/// The consume cases on a store with the data 5 and 6 on the channel 1 and
/// the data 7 and 8 on the channel 2.
fn consume_cases(space: &Space) {
    let pair = [1, 2];
    let pair_keys = channel_keys(&pair);
    let stored = consume_case(
        space,
        "a Stored check of a consume matches",
        &pair,
        &pair_keys,
        &[0, 7],
        9,
        None,
        None,
        &consume_reads(&pair),
    );
    let comm = consume_comm(&stored);
    let partial = consume_case(
        space,
        "a partial miss gives nothing",
        &pair,
        &pair_keys,
        &[0, 9],
        9,
        None,
        None,
        &consume_reads(&pair),
    );
    assert!(matches!(partial, Ok(None)), "a partial miss: {partial:?}");
    let repeated = [1, 1];
    consume_comm(&consume_case(
        space,
        "a repeated channel takes one datum for each pattern",
        &repeated,
        &channel_keys(&repeated),
        &[0, 0],
        9,
        None,
        None,
        &consume_reads(&repeated),
    ));
    let vetoing = consume_case(
        space,
        "the Stored check skips a vetoing continuation before any read",
        &pair,
        &pair_keys,
        &[0, 7],
        255,
        None,
        None,
        &[],
    );
    assert!(matches!(vetoing, Ok(None)), "a vetoing continuation: {vetoing:?}");
    let refusing = Owners {
        refused: vec![Consume::create(&pair.to_vec(), &vec![0u8, 7], &9u8, false)],
        ..Owners::default()
    };
    let skipped = consume_case(
        space,
        "the Stored check skips a refused consume before any read",
        &pair,
        &pair_keys,
        &[0, 7],
        9,
        None,
        Some((&refusing, false)),
        &[],
    );
    assert!(matches!(skipped, Ok(None)), "a refused consume: {skipped:?}");
    let unfunded = Owners {
        unfunded: BTreeSet::from([5, 7]),
        ..Owners::default()
    };
    let funded = consume_case(
        space,
        "the item test skips an unfunded datum",
        &pair,
        &pair_keys,
        &[0, 0],
        9,
        None,
        Some((&unfunded, true)),
        &consume_reads(&pair),
    );
    let Ok(Some((data, _))) = &funded else {
        panic!("a funded tuple, not {funded:?}");
    };
    assert!(
        data.iter()
            .all(|candidate| !unfunded.unfunded.contains(&candidate.datum.a))
    );
    consume_comm(&consume_case(
        space,
        "a Matched check selects the recorded tuple",
        &pair,
        &pair_keys,
        &[0, 7],
        9,
        Some(&Logical(comm.clone())),
        None,
        &consume_reads(&pair),
    ));
    let other = consume_case(
        space,
        "the identity of another consume stops before any read",
        &pair,
        &pair_keys,
        &[0, 8],
        9,
        Some(&Logical(comm.clone())),
        None,
        &[],
    );
    assert!(matches!(other, Ok(None)), "another consume: {other:?}");
    let differing = consume_case(
        space,
        "a COMM that differs from the identity gives nothing",
        &pair,
        &pair_keys,
        &[0, 7],
        9,
        Some(&Logical(with_peek(&comm))),
        None,
        &consume_reads(&pair),
    );
    assert!(matches!(differing, Ok(None)), "a differing COMM: {differing:?}");
    let mismatched = consume_case(
        space,
        "keys that do not match the channels fail before any read",
        &pair,
        &channel_keys(&[1]),
        &[0, 7],
        9,
        None,
        None,
        &[],
    );
    assert!(mismatched.is_err(), "mismatched keys: {mismatched:?}");
    consume_comm(&refused_consume_case(
        space,
        "the finder of a refused consume finds the named tuple",
        &pair,
        &[0, 7],
        9,
        &Logical(comm.clone()),
        &consume_reads(&pair),
    ));
    let not_found = refused_consume_case(
        space,
        "the finder of a refused consume finds nothing for a differing COMM",
        &pair,
        &[0, 7],
        9,
        &Logical(with_peek(&comm)),
        &consume_reads(&pair),
    );
    assert!(matches!(not_found, Ok(None)), "a differing refusal: {not_found:?}");
}

/// The produce cases of the incoming datum 4 on the channel 1, on the store
/// of the consume cases with the continuation 3 on the channel 1 and the
/// continuations 4 and 255 on the channels 1 and 2.
fn produce_cases(space: &Space, incoming: &Datum<u8>) {
    let pair = vec![1, 2];
    waiting(space, &[1], &[9], 3);
    waiting(space, &pair, &[0, 7], 4);
    waiting(space, &pair, &[0, 7], 255);
    let later = vec![vec![1], pair.clone()];
    let later_keys = operation_keys(&later);
    let first = vec![pair.clone(), vec![1]];
    let first_keys = operation_keys(&first);
    let single = vec![vec![1]];

    let selected = produce_case(
        space,
        "a later group is read only after the earlier groups fail",
        incoming,
        &later,
        &later_keys,
        None,
        None,
        &produce_reads(&later, 2),
    );
    let (later_comm, group) = produce_match(&selected, incoming);
    assert_eq!(group, 1);
    let selected = produce_case(
        space,
        "a match in the first group reads no later group",
        incoming,
        &first,
        &first_keys,
        None,
        None,
        &produce_reads(&first, 1),
    );
    let (first_comm, group) = produce_match(&selected, incoming);
    assert_eq!(group, 0);
    let none = produce_case(
        space,
        "a produce that matches no group gets its datum back",
        incoming,
        &single,
        &operation_keys(&single),
        None,
        None,
        &produce_reads(&single, 1),
    );
    assert!(unmatched(&none, incoming), "no group matches: {none:?}");
    let (_, group) = produce_match(
        &produce_case(
            space,
            "a Matched check selects the recorded continuation",
            incoming,
            &later,
            &later_keys,
            Some(&Logical(later_comm.clone())),
            None,
            &produce_reads(&later, 2),
        ),
        incoming,
    );
    assert_eq!(group, 1);
    let rejected = produce_case(
        space,
        "a COMM that differs from the identity returns the datum at once",
        incoming,
        &first,
        &first_keys,
        Some(&Logical(with_peek(&first_comm))),
        None,
        &produce_reads(&first, 1),
    );
    assert!(unmatched(&rejected, incoming), "a differing COMM: {rejected:?}");
    let refusing = Owners {
        refused: vec![Consume::create(&pair, &vec![0u8, 7], &4u8, false)],
        ..Owners::default()
    };
    let skipped = produce_case(
        space,
        "the Stored check skips a refused continuation",
        incoming,
        &first,
        &first_keys,
        None,
        Some(&refusing),
        &produce_reads(&first, 2),
    );
    assert!(unmatched(&skipped, incoming), "a refused continuation: {skipped:?}");
    let unfunded = Owners {
        unfunded: BTreeSet::from([4]),
        ..Owners::default()
    };
    let funded = produce_case(
        space,
        "the item test skips the unfunded incoming datum",
        incoming,
        &first,
        &first_keys,
        None,
        Some(&unfunded),
        &produce_reads(&first, 1),
    );
    let Ok(Produced::Matched { candidate, .. }) = &funded else {
        panic!("a funded tuple, not {funded:?}");
    };
    assert!(
        candidate
            .data_candidates
            .iter()
            .all(|data| data.datum_index != -1)
    );
    let mismatched = produce_case(
        space,
        "keys that do not match the joins fail before any read",
        incoming,
        &later,
        &operation_keys(&single),
        None,
        None,
        &[],
    );
    assert!(mismatched.is_err(), "mismatched keys: {mismatched:?}");
    refused_produce_case(
        space,
        "the finder of a refused produce finds the named tuple",
        incoming,
        &later,
        &later_keys,
        &Logical(later_comm.clone()),
        false,
        &produce_reads(&later, 2),
    )
    .expect("the finder runs")
    .expect("the named tuple");
    refused_produce_case(
        space,
        "the finder of a refused produce keeps the own datum",
        incoming,
        &later,
        &later_keys,
        &Logical(later_comm),
        true,
        &produce_reads(&later, 2),
    )
    .expect("the finder runs")
    .expect("the named tuple");
    let not_found = refused_produce_case(
        space,
        "the finder of a refused produce stops at a differing COMM",
        incoming,
        &first,
        &first_keys,
        &Logical(with_peek(&first_comm)),
        false,
        &produce_reads(&first, 1),
    )
    .expect("the finder runs");
    assert!(not_found.is_none(), "a differing refusal");
    let mismatched = refused_produce_case(
        space,
        "the finder of a refused produce fails on keys that do not match",
        incoming,
        &later,
        &operation_keys(&single),
        &Logical(first_comm),
        false,
        &[],
    );
    assert!(mismatched.is_err(), "mismatched keys");
}

/// Added by decision 123 (plan v4, section 10.1, V3-1): each wrapper of the
/// replay selection gives the result of its base body, with one
/// continuation read and one view build for each group that it reaches. The
/// cases cover the Stored check and the Matched check, the early skips, the
/// owner checks, the own datum, the identity rejection and the errors.
#[tokio::test]
async fn replay_selection_equals_the_old_reader_selection() {
    let incoming = Datum::create(&1u8, 4u8, false);
    let own = Owners {
        own_datum: true,
        ..Owners::default()
    };

    let pair_space = space().await;
    put(&pair_space, 1, 5);
    put(&pair_space, 1, 6);
    put(&pair_space, 2, 7);
    put(&pair_space, 2, 8);
    consume_cases(&pair_space);
    produce_cases(&pair_space, &incoming);

    // The own datum leaves only the incoming datum 4 on the channel 1, which
    // the pattern 6 does not take (R6).
    let own_space = space().await;
    put(&own_space, 1, 6);
    put(&own_space, 2, 8);
    waiting(&own_space, &[1, 2], &[6, 8], 12);
    let joins = vec![vec![1, 2]];
    let keys = operation_keys(&joins);
    let reads = produce_reads(&joins, 1);
    let open = produce_case(
        &own_space,
        "a stored datum matches without owner checks",
        &incoming,
        &joins,
        &keys,
        None,
        None,
        &reads,
    );
    produce_match(&open, &incoming);
    let kept = produce_case(
        &own_space,
        "the own datum keeps only the incoming datum on a channel named once",
        &incoming,
        &joins,
        &keys,
        None,
        Some(&own),
        &reads,
    );
    assert!(unmatched(&kept, &incoming), "the own datum: {kept:?}");

    // The item test of the stored datum 6 fails. The restriction removes
    // that datum before the tuple search, so its item test does not run (R6).
    let failing = Owners {
        failing: BTreeSet::from([6]),
        own_datum: true,
        ..Owners::default()
    };
    let untested = produce_case(
        &own_space,
        "the own datum runs no item test on a stored datum of a channel named once",
        &incoming,
        &joins,
        &keys,
        None,
        Some(&failing),
        &reads,
    );
    assert!(unmatched(&untested, &incoming), "the untested stored datum: {untested:?}");

    // On a channel that the join names twice the list stays whole, and the
    // tuple must hold the incoming datum (R6).
    let repeated_space = space().await;
    put(&repeated_space, 1, 6);
    put(&repeated_space, 1, 6);
    waiting(&repeated_space, &[1, 1], &[6, 6], 11);
    let joins = vec![vec![1, 1]];
    let keys = operation_keys(&joins);
    let reads = produce_reads(&joins, 1);
    produce_match(
        &produce_case(
            &repeated_space,
            "a repeated channel matches two stored data without owner checks",
            &incoming,
            &joins,
            &keys,
            None,
            None,
            &reads,
        ),
        &incoming,
    );
    let without = produce_case(
        &repeated_space,
        "a repeated own channel needs the incoming datum",
        &incoming,
        &joins,
        &keys,
        None,
        Some(&own),
        &reads,
    );
    assert!(unmatched(&without, &incoming), "the repeated own channel: {without:?}");

    // The commit of 254 passes, but it can veto, so the Stored check skips
    // it (E3-a).
    let veto_space = space().await;
    waiting(&veto_space, &[1], &[0], 254);
    let single = vec![vec![1]];
    let keys = operation_keys(&single);
    let vetoing = produce_case(
        &veto_space,
        "the Stored check skips a continuation whose commit can veto",
        &incoming,
        &single,
        &keys,
        None,
        None,
        &produce_reads(&single, 1),
    );
    assert!(unmatched(&vetoing, &incoming), "a vetoing continuation: {vetoing:?}");

    let counted_space = space().await;
    waiting(&counted_space, &[1], &[0], 3);
    counted_space
        .produce_counter
        .lock()
        .expect("the produce counter")
        .insert(incoming.source.clone(), i32::MAX);
    let overflow = produce_case(
        &counted_space,
        "the counter overflow fails before any read",
        &incoming,
        &single,
        &keys,
        None,
        None,
        &[],
    );
    assert!(overflow.is_err(), "the counter overflow: {overflow:?}");
}
