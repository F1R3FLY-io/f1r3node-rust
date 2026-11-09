// See rspace/src/main/scala/coop/rchain/rspace/SpaceMatcher.scala

use std::collections::HashMap;
// Added by decision 123 (plan v4, V3-1): the shared search loops.
use std::convert::Infallible;
use std::marker::PhantomData;
use std::sync::Arc;

use super::r#match::Match;
// Added by G3 (DR-135, S5): the owner checks of a match.
use super::operation_context::{ItemAdmission, MatchAdmission};
use super::rspace_interface::ISpace;
use crate::rspace::internal::{ConsumeCandidate, Datum, ProduceCandidate, WaitingContinuation};
use crate::rspace::metrics_constants::{
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CALLS_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CANDIDATES_ITERATED_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_PAIR_CONSTRUCTION_NS_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_SUCCESS_METRIC, RSPACE_METRICS_SOURCE,
};
// Added by decision 123 (plan v4, V3-1): the consume of a tuple request.
use crate::rspace::trace::event::Consume;

type MatchingDataCandidate<C, A> = (ConsumeCandidate<C, A>, Vec<(Datum<A>, i32)>);

/// Added by DR-129 (E1): a join can name one channel more than once at run
/// time. The COMM records only the set of produces, not which datum went to
/// which pattern, so replay works that assignment out again in its own order,
/// which puts the source hash first (`candidate_order`). This gives the list
/// of each repeated channel one stable order by source hash, with the
/// incoming datum of a produce (index -1) first, so the greedy assignment is
/// the one that replay finds. Data with one source hash have equal content.
/// The list in the caller's order goes into `rollback`, so a failed match
/// restores it.
fn order_repeated_channels<C, P, A>(
    channel_pattern_pairs: &[(C, P)],
    channel_to_indexed_data: &mut HashMap<C, Vec<(Datum<A>, i32)>>,
    rollback: &mut Vec<(C, Vec<(Datum<A>, i32)>)>,
) where
    C: Clone + std::hash::Hash + Eq,
    A: Clone,
{
    let compare = |(left, left_index): &(Datum<A>, i32), (right, right_index): &(Datum<A>, i32)| {
        (*left_index != -1)
            .cmp(&(*right_index != -1))
            .then_with(|| left.source.hash.cmp(&right.source.hash))
    };
    for (position, (channel, _)) in channel_pattern_pairs.iter().enumerate() {
        let first = !channel_pattern_pairs[..position]
            .iter()
            .any(|(earlier, _)| earlier == channel);
        let repeated = channel_pattern_pairs[position + 1..]
            .iter()
            .any(|(later, _)| later == channel);
        if !(first && repeated) {
            continue;
        }
        if let Some(data) = channel_to_indexed_data.get_mut(channel) {
            if data
                .windows(2)
                .any(|pair| compare(&pair[0], &pair[1]) == std::cmp::Ordering::Greater)
            {
                rollback.push((channel.clone(), data.clone()));
                data.sort_by(compare);
            }
        }
    }
}

/// Added by decision 123 (plan v4, V3-1): the groups of one search, in the
/// order of the search. Native play, directive replay, native replay and the
/// Reified rows implement it, so they run the same search loops.
pub trait MatchCandidates<C, P, A, K>
where
    C: Clone,
    P: Clone,
    A: Clone,
    K: Clone,
{
    /// `Infallible` for lists that are already fetched, `RSpaceError` or
    /// `SpaceTxnError` for a source that reads a store.
    type Error;

    /// The lists of one group. They live as long as the group borrow.
    type Group<'g>: MatchGroup<C, P, A, K, Error = Self::Error>
    where
        Self: 'g,
        C: 'g;

    /// Builds the lists of the group `index` over `channels` once, as the
    /// base builds them before its continuation loop.
    fn group<'g>(
        &'g mut self,
        index: usize,
        channels: &'g [C],
    ) -> Result<Self::Group<'g>, Self::Error>;

    /// E3-a before any read: whether the commit of `continuation` can veto.
    /// The default is `Match::commit_may_veto`.
    fn may_veto(&self, matcher: &dyn Match<P, A, K>, continuation: &K) -> bool {
        matcher.commit_may_veto(continuation)
    }
}

/// Added by decision 123 (plan v4, V3-1): the lists of one group of a search.
pub trait MatchGroup<C, P, A, K>
where
    C: Clone,
    P: Clone,
    A: Clone,
    K: Clone,
{
    type Error;

    /// The candidate continuations of the group, with their store indices,
    /// in the order of the search.
    fn continuations(&mut self) -> Vec<(Arc<WaitingContinuation<P, K>>, i32)>;

    /// The first data tuple of the request, or None. A datum that fails
    /// `request.item` is a miss of its pattern (R5).
    fn tuple(
        &mut self,
        request: TupleRequest<'_, C, P, A, K>,
    ) -> Result<Option<Vec<ConsumeCandidate<C, A>>>, Self::Error>;

    /// Undoes what the last `tuple` call took, in reverse (DR-129 R3).
    fn restore(&mut self) {}

    /// The commit check of a complete tuple. The default is the `where`
    /// guard of decision 37 over a copy of the matched data, as the base
    /// search makes it.
    fn commit_check(
        &mut self,
        request: &TupleRequest<'_, C, P, A, K>,
        tuple: &[ConsumeCandidate<C, A>],
    ) -> Result<bool, Self::Error> {
        let mut data = Vec::with_capacity(tuple.len());
        data.extend(tuple.iter().map(|candidate| candidate.datum.a.clone()));
        Ok(request.matcher.check_commit(request.continuation, &data))
    }

    /// E3-a for one continuation of the group.
    fn may_veto(&self, matcher: &dyn Match<P, A, K>, continuation: &K) -> bool {
        matcher.commit_may_veto(continuation)
    }

    /// Notes that the owners refused `tuple` (R7). The default notes nothing.
    fn refused(
        &mut self,
        request: &TupleRequest<'_, C, P, A, K>,
        tuple: &[ConsumeCandidate<C, A>],
    ) {
        let _ = (request, tuple);
    }
}

/// Added by decision 123 (plan v4, V3-1): the terms of one tuple search. The
/// continuation comes by parts, because a consume has no
/// `WaitingContinuation` before its match.
pub struct TupleRequest<'r, C, P: Clone, A: Clone, K: Clone> {
    pub matcher: &'r dyn Match<P, A, K>,
    pub group: &'r [C],
    pub patterns: &'r [P],
    pub continuation: &'r K,
    pub persistent: bool,
    /// The consume of the continuation. A refusal books its hash (CR-20).
    pub source: &'r Consume,
    /// The play item test of an owner-funded operation (R5). None in replay.
    pub item: Option<ItemAdmission<'r, C, P, A, K>>,
    /// The channel of the incoming datum of a produce (R6).
    pub own: Option<&'r C>,
}

// Added by decision 123 (plan v4, V3-7): written by hand, as for
// `ItemAdmission`, so C, P, A and K need no `Copy` bound.
impl<C, P: Clone, A: Clone, K: Clone> Clone for TupleRequest<'_, C, P, A, K> {
    fn clone(&self) -> Self { *self }
}

impl<C, P: Clone, A: Clone, K: Clone> Copy for TupleRequest<'_, C, P, A, K> {}

/// Added by decision 123 (plan v4, V3-1, V3-3): the lists of one group that a
/// store search fetched, the continuations of the group and the data of its
/// channels. Native play, directive replay and the native consume search them
/// with the extraction of `SpaceMatcher`. `E` is the error of the source:
/// `Infallible` for a produce, `RSpaceError` for a consume, whose kept
/// admission error has that type.
pub struct StoreGroup<'g, S: ?Sized, C, P: Clone, A: Clone, K: Clone, E = Infallible> {
    space: &'g S,
    matcher: &'g Box<dyn Match<P, A, K>>,
    continuations: Vec<(Arc<WaitingContinuation<P, K>>, i32)>,
    data: HashMap<C, Vec<(Datum<A>, i32)>>,
    rollback: Vec<(C, Vec<(Datum<A>, i32)>)>,
    pair_metric: bool,
    error: PhantomData<fn() -> E>,
}

impl<'g, S: ?Sized, C, P: Clone, A: Clone, K: Clone, E> StoreGroup<'g, S, C, P, A, K, E> {
    /// The lists of one produce group: its continuations and the data of its
    /// channels, of the `width` channels of the group. The pair construction
    /// of each continuation emits its metric, as in the base produce search.
    pub fn fetched(
        space: &'g S,
        matcher: &'g Box<dyn Match<P, A, K>>,
        continuations: Vec<(Arc<WaitingContinuation<P, K>>, i32)>,
        data: HashMap<C, Vec<(Datum<A>, i32)>>,
        width: usize,
    ) -> Self {
        Self {
            space,
            matcher,
            continuations,
            data,
            rollback: Vec::with_capacity(width + 1),
            pair_metric: true,
            error: PhantomData,
        }
    }

    /// The lists of one consume: the data of its `width` channels and no
    /// continuation. The pair construction emits no metric, as in the base
    /// consume search.
    pub fn fetched_consume(
        space: &'g S,
        matcher: &'g Box<dyn Match<P, A, K>>,
        data: HashMap<C, Vec<(Datum<A>, i32)>>,
        width: usize,
    ) -> Self {
        Self {
            space,
            matcher,
            continuations: Vec::new(),
            data,
            rollback: Vec::with_capacity(width + 1),
            pair_metric: false,
            error: PhantomData,
        }
    }
}

impl<S, C, P, A, K, E> MatchGroup<C, P, A, K> for StoreGroup<'_, S, C, P, A, K, E>
where
    S: SpaceMatcher<C, P, A, K> + ?Sized,
    C: Clone + std::hash::Hash + Eq + Send + Sync,
    P: Clone + Send + Sync,
    A: Clone + Send + Sync,
    K: Clone + Send + Sync,
{
    type Error = E;

    fn continuations(&mut self) -> Vec<(Arc<WaitingContinuation<P, K>>, i32)> {
        std::mem::take(&mut self.continuations)
    }

    fn tuple(
        &mut self,
        request: TupleRequest<'_, C, P, A, K>,
    ) -> Result<Option<Vec<ConsumeCandidate<C, A>>>, E> {
        self.rollback.clear();
        let pairs = || -> Vec<(C, P)> {
            request
                .group
                .iter()
                .cloned()
                .zip(request.patterns.iter().cloned())
                .collect()
        };
        let channel_pattern_pairs = if self.pair_metric {
            let pair_start = std::time::Instant::now();
            let channel_pattern_pairs = pairs();
            metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_PAIR_CONSTRUCTION_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(pair_start.elapsed().as_nanos() as u64);
            channel_pattern_pairs
        } else {
            pairs()
        };
        // Moved by decision 123 (plan v4, V3-1) from `extract_first_match`: a
        // produce keeps its own datum (R6). On a channel that the join names
        // once, the pattern takes only the incoming datum (index -1). The full
        // list goes into `rollback` first, so the reverse restore writes it
        // back last.
        if let Some(own) = request.own {
            let channels = request.group;
            if channels.iter().filter(|channel| *channel == own).count() == 1 {
                if let Some(data) = self.data.get_mut(own) {
                    if data.iter().any(|(_, index)| *index != -1) {
                        self.rollback.push((own.clone(), data.clone()));
                        data.retain(|(_, index)| *index == -1);
                    }
                }
            }
        }
        let data_candidates = self.space.extract_data_candidates_rollback(
            self.matcher,
            &channel_pattern_pairs,
            &mut self.data,
            &mut self.rollback,
            request.item,
        );
        Ok(data_candidates.into_iter().collect::<Option<Vec<_>>>())
    }

    fn restore(&mut self) {
        for (ch, original) in self.rollback.drain(..).rev() {
            self.data.insert(ch, original);
        }
    }
}

/// Added by decision 123 (plan v4, V3-1, V3-3): the produce search of one
/// group of any source, the body of the base `extract_first_match`. Each
/// continuation of the group gets one tuple search with its item test. A
/// tuple must hold the incoming datum (R6), pass the commit check of the
/// group, and pass the tuple test of the owners (R5). Otherwise the group
/// restores what the search took, and the next continuation is tried. A
/// refused tuple is noted with the group first.
pub fn first_match<C, P, A, K, G>(
    matcher: &dyn Match<P, A, K>,
    channels: Vec<C>,
    group: &mut G,
    admission: Option<&dyn MatchAdmission<C, P, A, K>>,
) -> Result<Option<ProduceCandidate<C, P, A, K>>, G::Error>
where
    C: Clone,
    P: Clone,
    A: Clone,
    K: Clone,
    G: MatchGroup<C, P, A, K> + ?Sized,
{
    metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
        .increment(1);
    let own = admission.and_then(|admission| admission.own_datum_channel());
    for (cont, index) in group.continuations() {
        metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CANDIDATES_ITERATED_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        let item = admission.map(|admission| ItemAdmission {
            admission,
            continuation: &cont.continuation,
            persistent: cont.persist,
            channel_count: channels.len(),
        });
        let request = TupleRequest {
            matcher,
            group: &channels,
            patterns: &cont.patterns,
            continuation: &cont.continuation,
            persistent: cont.persist,
            source: &cont.source,
            item,
            own,
        };
        let Some(tuple) = group.tuple(request)? else {
            group.restore();
            continue;
        };
        let holds_own_datum =
            own.is_none() || tuple.iter().any(|candidate| candidate.datum_index == -1);
        if !holds_own_datum || !group.commit_check(&request, &tuple)? {
            group.restore();
            continue;
        }
        if let Some(admission) = admission {
            if !admission.admit_tuple(&cont, &tuple) {
                group.refused(&request, &tuple);
                group.restore();
                continue;
            }
        }
        metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_SUCCESS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        return Ok(Some(ProduceCandidate {
            channels,
            continuation: (*cont).clone(),
            continuation_index: index,
            data_candidates: tuple,
        }));
    }
    Ok(None)
}

/// Added by decision 123 (plan v4, V3-4, R6): the consume search of one group
/// of any source, the search of the base consume. It tries one greedy tuple
/// with the item test of the request. Then an error that the item test kept
/// returns at once, `searched` emits the search metric, and the commit check
/// runs on a found tuple. No tuple or a veto gives `Ok(None)`, and the caller
/// parks the consume.
pub fn consume_match<C, P, A, K, G>(
    group: &mut G,
    request: TupleRequest<'_, C, P, A, K>,
    kept_error: impl FnOnce() -> Option<G::Error>,
    searched: impl FnOnce(),
) -> Result<Option<Vec<ConsumeCandidate<C, A>>>, G::Error>
where
    C: Clone,
    P: Clone,
    A: Clone,
    K: Clone,
    G: MatchGroup<C, P, A, K> + ?Sized,
{
    let tuple = group.tuple(request)?;
    if let Some(error) = kept_error() {
        return Err(error);
    }
    searched();
    let Some(tuple) = tuple else {
        return Ok(None);
    };
    match group.commit_check(&request, &tuple)? {
        true => Ok(Some(tuple)),
        false => Ok(None),
    }
}

pub trait SpaceMatcher<C, P, A, K>: ISpace<C, P, A, K>
where
    C: Clone + std::hash::Hash + Eq + Send + Sync,
    P: Clone + Send + Sync,
    A: Clone + Send + Sync,
    K: Clone + Send + Sync,
{
    // Changed by G3 (DR-135, S5): the search also runs the item test of an
    // owner-funded match (R5).
    // fn find_matching_data_candidate(
    //     &self,
    //     matcher: &Box<dyn Match<P, A, K>>,
    //     channel: C,
    //     data: &[(Datum<A>, i32)],
    //     pattern: &P,
    // ) -> Option<MatchingDataCandidate<C, A>> {
    fn find_matching_data_candidate(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channel: C,
        data: &[(Datum<A>, i32)],
        pattern: &P,
        item: Option<ItemAdmission<'_, C, P, A, K>>,
    ) -> Option<MatchingDataCandidate<C, A>> {
        for (idx, (datum, data_index)) in data.iter().enumerate() {
            metrics::counter!("rspace.matcher.get_calls", "source" => "rspace").increment(1);
            // Probe by reference — no whole-pattern/whole-datum clone per attempt.
            let t_match = std::time::Instant::now();
            let match_result = matcher.get(pattern, &datum.a);
            metrics::counter!("rspace.matcher.fold_match_ns", "source" => "rspace")
                .increment(t_match.elapsed().as_nanos() as u64);
            // Added by G3 (DR-135, S5): a datum whose owners cannot pay its
            // share of a COMM with this continuation is a miss (R5).
            let match_result = match (match_result, item) {
                (Some(mat), Some(item)) if !item.admits(&mat, datum.persist) => None,
                (result, _) => result,
            };

            if let Some(mat) = match_result {
                let indexed_datums = if datum.persist {
                    data.to_vec()
                } else {
                    let mut remaining = Vec::with_capacity(data.len() - 1);
                    remaining.extend_from_slice(&data[..idx]);
                    remaining.extend_from_slice(&data[idx + 1..]);
                    remaining
                };
                return Some((
                    ConsumeCandidate {
                        channel,
                        datum: Datum {
                            a: mat,
                            persist: datum.persist,
                            source: datum.source.clone(),
                        },
                        removed_datum: datum.a.clone(),
                        datum_index: *data_index,
                    },
                    indexed_datums,
                ));
            }
        }
        None
    }

    /// Attempts to match all channel-pattern pairs against the data map.
    /// Records mutations in `rollback` so the caller can undo them on failure.
    // Changed by G3 (DR-135, S5): the extraction passes the item test of an
    // owner-funded match on (R5).
    // fn extract_data_candidates_rollback(
    //     &self,
    //     matcher: &Box<dyn Match<P, A, K>>,
    //     channel_pattern_pairs: &[(C, P)],
    //     channel_to_indexed_data: &mut HashMap<C, Vec<(Datum<A>, i32)>>,
    //     rollback: &mut Vec<(C, Vec<(Datum<A>, i32)>)>,
    // ) -> Vec<Option<ConsumeCandidate<C, A>>> {
    fn extract_data_candidates_rollback(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channel_pattern_pairs: &[(C, P)],
        channel_to_indexed_data: &mut HashMap<C, Vec<(Datum<A>, i32)>>,
        rollback: &mut Vec<(C, Vec<(Datum<A>, i32)>)>,
        item: Option<ItemAdmission<'_, C, P, A, K>>,
    ) -> Vec<Option<ConsumeCandidate<C, A>>> {
        // Added by DR-129 (E1): a stable order for a channel that the join
        // names more than once.
        order_repeated_channels(channel_pattern_pairs, channel_to_indexed_data, rollback);
        let mut acc = Vec::with_capacity(channel_pattern_pairs.len());

        for (channel, pattern) in channel_pattern_pairs {
            let maybe_tuple: Option<MatchingDataCandidate<C, A>> =
                // Changed by G3 (DR-135, S5): with the item test.
                // match channel_to_indexed_data.get(channel) {
                //     Some(indexed_data) => self.find_matching_data_candidate(
                //         matcher,
                //         channel.clone(),
                //         indexed_data,
                //         pattern,
                //     ),
                //     None => None,
                // };
                match channel_to_indexed_data.get(channel) {
                    Some(indexed_data) => self.find_matching_data_candidate(
                        matcher,
                        channel.clone(),
                        indexed_data,
                        pattern,
                        item,
                    ),
                    None => None,
                };

            match maybe_tuple {
                Some((cand, rem)) => {
                    acc.push(Some(cand));
                    // Save the original before mutating
                    if let Some(original) = channel_to_indexed_data.get(channel) {
                        rollback.push((channel.clone(), original.clone()));
                    }
                    channel_to_indexed_data.insert(channel.clone(), rem);
                }
                None => {
                    acc.push(None);
                }
            }
        }

        acc
    }

    /// Non-rollback version for consume path (no speculative matching).
    fn extract_data_candidates(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channel_pattern_pairs: &[(C, P)],
        channel_to_indexed_data: &mut HashMap<C, Vec<(Datum<A>, i32)>>,
    ) -> Vec<Option<ConsumeCandidate<C, A>>> {
        let mut rollback = Vec::new();
        // Changed by G3 (DR-135, S5): no item test.
        // self.extract_data_candidates_rollback(
        //     matcher,
        //     channel_pattern_pairs,
        //     channel_to_indexed_data,
        //     &mut rollback,
        // )
        self.extract_data_candidates_rollback(
            matcher,
            channel_pattern_pairs,
            channel_to_indexed_data,
            &mut rollback,
            None,
        )
    }

    // Changed by G3 (DR-135, S5): a produce with owner funding runs the
    // owner checks of each continuation (R5, R6). Without them it matches as
    // on dev.
    // fn extract_first_match(
    //     &self,
    //     matcher: &Box<dyn Match<P, A, K>>,
    //     channels: Vec<C>,
    //     match_candidates: Vec<(std::sync::Arc<WaitingContinuation<P, K>>, i32)>,
    //     mut channel_to_index_data: HashMap<C, Vec<(Datum<A>, i32)>>,
    // ) -> Option<ProduceCandidate<C, P, A, K>> {
    // Changed by decision 123 (plan v4, V3-1, V3-3): the body moved to the
    // free `first_match`, which runs over a `StoreGroup` of the fetched
    // lists here and over the groups of the other sources elsewhere. The
    // old body:
    // ```
    // fn extract_first_match(
    //     &self,
    //     matcher: &Box<dyn Match<P, A, K>>,
    //     channels: Vec<C>,
    //     match_candidates: Vec<(std::sync::Arc<WaitingContinuation<P, K>>, i32)>,
    //     mut channel_to_index_data: HashMap<C, Vec<(Datum<A>, i32)>>,
    //     admission: Option<&dyn MatchAdmission<C, P, A, K>>,
    // ) -> Option<ProduceCandidate<C, P, A, K>> {
    //     metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
    //         .increment(1);
    //     for (cont, index) in &match_candidates {
    //         metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CANDIDATES_ITERATED_METRIC, "source" => RSPACE_METRICS_SOURCE)
    //             .increment(1);
    //         let __pair_start = std::time::Instant::now();
    //         let channel_pattern_pairs: Vec<(C, P)> = channels
    //             .iter()
    //             .cloned()
    //             .zip(cont.patterns.iter().cloned())
    //             .collect();
    //         metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_PAIR_CONSTRUCTION_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
    //             .increment(__pair_start.elapsed().as_nanos() as u64);
    //
    //         let mut rollback = Vec::new();
    //         // Added by G3 (DR-135, S5): the item test of this continuation
    //         // (R5).
    //         let item = admission.map(|admission| ItemAdmission {
    //             admission,
    //             continuation: &cont.continuation,
    //             persistent: cont.persist,
    //             channel_count: channels.len(),
    //         });
    //         // Added by G3 (DR-135, S5): a produce keeps its own datum (R6).
    //         // On a channel that the join names once, the pattern takes only
    //         // the incoming datum (index -1). The full list goes into
    //         // `rollback` first, so the reverse restore writes it back last.
    //         let own_channel = admission.and_then(|admission| admission.own_datum_channel());
    //         if let Some(own) = own_channel {
    //             if channels.iter().filter(|channel| *channel == own).count() == 1 {
    //                 if let Some(data) = channel_to_index_data.get_mut(own) {
    //                     if data.iter().any(|(_, index)| *index != -1) {
    //                         rollback.push((own.clone(), data.clone()));
    //                         data.retain(|(_, index)| *index == -1);
    //                     }
    //                 }
    //             }
    //         }
    //         // Changed by G3 (DR-135, S5): the extraction runs the item test.
    //         // let data_candidates = self.extract_data_candidates_rollback(
    //         //     matcher,
    //         //     &channel_pattern_pairs,
    //         //     &mut channel_to_index_data,
    //         //     &mut rollback,
    //         // );
    //         let data_candidates = self.extract_data_candidates_rollback(
    //             matcher,
    //             &channel_pattern_pairs,
    //             &mut channel_to_index_data,
    //             &mut rollback,
    //             item,
    //         );
    //         // Added by G3 (DR-135, S5): on a repeated channel the tuple must
    //         // hold the incoming datum (R6).
    //         let holds_own_datum = own_channel.is_none() ||
    //             data_candidates
    //                 .iter()
    //                 .flatten()
    //                 .any(|candidate| candidate.datum_index == -1);
    //
    //         // Changed by G3 (DR-135, S5): the tuple must hold the incoming
    //         // datum under owner funding (R6).
    //         // if data_candidates.iter().all(|x| x.is_some()) {
    //         if data_candidates.iter().all(|x| x.is_some()) && holds_own_datum {
    //             // Cross-channel commit hook: gives the matcher a chance
    //             // to veto a commit even after every spatial bind has
    //             // matched. Used for `where`-clause guards that mention
    //             // bindings from multiple channels (plan §7.12).
    //             let matched_data: Vec<A> = data_candidates
    //                 .iter()
    //                 .map(|c| c.as_ref().unwrap().datum.a.clone())
    //                 .collect();
    //             if !matcher.check_commit(&cont.continuation, &matched_data) {
    //                 // Guard rejected: roll back and try the next
    //                 // waiting continuation, just like a spatial miss.
    //                 // Changed by DR-129 (R3): a channel that the join names
    //                 // more than once is saved once for each match. The
    //                 // restore runs in reverse, so the first saved list, the
    //                 // whole one, is the last one written back.
    //                 // for (ch, original) in rollback {
    //                 for (ch, original) in rollback.into_iter().rev() {
    //                     channel_to_index_data.insert(ch, original);
    //                 }
    //                 continue;
    //             }
    //             // Added by G3 (DR-135, S5): the tuple test (R5). The owners of
    //             // the tuple pay its COMM, or the tuple is refused: it rolls
    //             // back, and the next continuation is tried, as on a veto.
    //             let candidates: Vec<ConsumeCandidate<C, A>> =
    //                 data_candidates.into_iter().flatten().collect();
    //             if let Some(admission) = admission {
    //                 if !admission.admit_tuple(cont, &candidates) {
    //                     for (ch, original) in rollback.into_iter().rev() {
    //                         channel_to_index_data.insert(ch, original);
    //                     }
    //                     continue;
    //                 }
    //             }
    //             metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_SUCCESS_METRIC, "source" => RSPACE_METRICS_SOURCE)
    //                 .increment(1);
    //             // Changed by G3 (DR-135, S5): the candidates moved out for the
    //             // tuple test.
    //             // data_candidates: data_candidates.into_iter().flatten().collect(),
    //             return Some(ProduceCandidate {
    //                 channels,
    //                 // Deref-clone the inner continuation only on a successful
    //                 // match — failed probes never deep-clone.
    //                 continuation: (**cont).clone(),
    //                 continuation_index: *index,
    //                 data_candidates: candidates,
    //             });
    //         }
    //
    //         // Restore only the channels that were actually mutated
    //         // Changed by DR-129 (R3): in reverse, as above.
    //         // for (ch, original) in rollback {
    //         for (ch, original) in rollback.into_iter().rev() {
    //             channel_to_index_data.insert(ch, original);
    //         }
    //     }
    //     None
    // }
    // ```
    fn extract_first_match(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channels: Vec<C>,
        match_candidates: Vec<(std::sync::Arc<WaitingContinuation<P, K>>, i32)>,
        channel_to_index_data: HashMap<C, Vec<(Datum<A>, i32)>>,
        admission: Option<&dyn MatchAdmission<C, P, A, K>>,
    ) -> Option<ProduceCandidate<C, P, A, K>> {
        let width = channels.len();
        let mut group = StoreGroup::<Self, C, P, A, K>::fetched(
            self,
            matcher,
            match_candidates,
            channel_to_index_data,
            width,
        );
        let Ok(found) = first_match(&**matcher, channels, &mut group, admission);
        found
    }
}
