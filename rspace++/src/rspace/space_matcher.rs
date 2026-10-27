// See rspace/src/main/scala/coop/rchain/rspace/SpaceMatcher.scala

use std::collections::HashMap;

use super::r#match::Match;
// Added by G3 (DR-135, S5): the owner checks of a match.
use super::operation_context::{ItemAdmission, MatchAdmission};
use super::rspace_interface::ISpace;
use crate::rspace::internal::{
    ConsumeCandidate, Datum, IndexedData, ProduceCandidate, WaitingContinuation,
};
use crate::rspace::metrics_constants::{
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CALLS_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CANDIDATES_ITERATED_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_PAIR_CONSTRUCTION_NS_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_SUCCESS_METRIC, RSPACE_METRICS_SOURCE,
};

type MatchingDataCandidate<C, A> = (ConsumeCandidate<C, A>, IndexedData<A>);

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
    channel_to_indexed_data: &mut HashMap<C, IndexedData<A>>,
    rollback: &mut Vec<(C, IndexedData<A>)>,
) where
    C: Clone + std::hash::Hash + Eq,
    A: Clone,
{
    let compare = |(left, left_index): &(std::sync::Arc<Datum<A>>, i32),
                   (right, right_index): &(std::sync::Arc<Datum<A>>, i32)| {
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
        data: &[(std::sync::Arc<Datum<A>>, i32)],
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
    //     channel_to_indexed_data: &mut HashMap<C, IndexedData<A>>,
    //     rollback: &mut Vec<(C, IndexedData<A>)>,
    // ) -> Vec<Option<ConsumeCandidate<C, A>>> {
    fn extract_data_candidates_rollback(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channel_pattern_pairs: &[(C, P)],
        channel_to_indexed_data: &mut HashMap<C, IndexedData<A>>,
        rollback: &mut Vec<(C, IndexedData<A>)>,
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
                    if let Some(original) = channel_to_indexed_data.insert(channel.clone(), rem) {
                        rollback.push((channel.clone(), original));
                    }
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
        channel_to_indexed_data: &mut HashMap<C, IndexedData<A>>,
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
    //     mut channel_to_index_data: HashMap<C, IndexedData<A>>,
    // ) -> Option<ProduceCandidate<C, P, A, K>> {
    fn extract_first_match(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channels: Vec<C>,
        match_candidates: Vec<(std::sync::Arc<WaitingContinuation<P, K>>, i32)>,
        mut channel_to_index_data: HashMap<C, IndexedData<A>>,
        admission: Option<&dyn MatchAdmission<C, P, A, K>>,
    ) -> Option<ProduceCandidate<C, P, A, K>> {
        metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        for (cont, index) in &match_candidates {
            metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CANDIDATES_ITERATED_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            let __pair_start = std::time::Instant::now();
            let channel_pattern_pairs: Vec<(C, P)> = channels
                .iter()
                .cloned()
                .zip(cont.patterns.iter().cloned())
                .collect();
            metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_PAIR_CONSTRUCTION_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(__pair_start.elapsed().as_nanos() as u64);

            let mut rollback = Vec::new();
            // Added by G3 (DR-135, S5): the item test of this continuation
            // (R5).
            let item = admission.map(|admission| ItemAdmission {
                admission,
                continuation: &cont.continuation,
                persistent: cont.persist,
                channel_count: channels.len(),
            });
            // Added by G3 (DR-135, S5): a produce keeps its own datum (R6).
            // On a channel that the join names once, the pattern takes only
            // the incoming datum (index -1). The full list goes into
            // `rollback` first, so the reverse restore writes it back last.
            let own_channel = admission.and_then(|admission| admission.own_datum_channel());
            if let Some(own) = own_channel {
                if channels.iter().filter(|channel| *channel == own).count() == 1 {
                    if let Some(data) = channel_to_index_data.get_mut(own) {
                        if data.iter().any(|(_, index)| *index != -1) {
                            rollback.push((own.clone(), data.clone()));
                            data.retain(|(_, index)| *index == -1);
                        }
                    }
                }
            }
            // Changed by G3 (DR-135, S5): the extraction runs the item test.
            // let data_candidates = self.extract_data_candidates_rollback(
            //     matcher,
            //     &channel_pattern_pairs,
            //     &mut channel_to_index_data,
            //     &mut rollback,
            // );
            let data_candidates = self.extract_data_candidates_rollback(
                matcher,
                &channel_pattern_pairs,
                &mut channel_to_index_data,
                &mut rollback,
                item,
            );
            // Added by G3 (DR-135, S5): on a repeated channel the tuple must
            // hold the incoming datum (R6).
            let holds_own_datum = own_channel.is_none() ||
                data_candidates
                    .iter()
                    .flatten()
                    .any(|candidate| candidate.datum_index == -1);

            // Changed by G3 (DR-135, S5): the tuple must hold the incoming
            // datum under owner funding (R6).
            // if data_candidates.iter().all(|x| x.is_some()) {
            if data_candidates.iter().all(|x| x.is_some()) && holds_own_datum {
                // Cross-channel commit hook: gives the matcher a chance
                // to veto a commit even after every spatial bind has
                // matched. Used for `where`-clause guards that mention
                // bindings from multiple channels (plan §7.12).
                let matched_data: Vec<A> = data_candidates
                    .iter()
                    .map(|c| c.as_ref().unwrap().datum.a.clone())
                    .collect();
                if !matcher.check_commit(&cont.continuation, &matched_data) {
                    // Guard rejected: roll back and try the next
                    // waiting continuation, just like a spatial miss.
                    // Changed by DR-129 (R3): a channel that the join names
                    // more than once is saved once for each match. The
                    // restore runs in reverse, so the first saved list, the
                    // whole one, is the last one written back.
                    // for (ch, original) in rollback {
                    for (ch, original) in rollback.into_iter().rev() {
                        channel_to_index_data.insert(ch, original);
                    }
                    continue;
                }
                // Added by G3 (DR-135, S5): the tuple test (R5). The owners of
                // the tuple pay its COMM, or the tuple is refused: it rolls
                // back, and the next continuation is tried, as on a veto.
                let candidates: Vec<ConsumeCandidate<C, A>> =
                    data_candidates.into_iter().flatten().collect();
                if let Some(admission) = admission {
                    if !admission.admit_tuple(cont, &candidates) {
                        for (ch, original) in rollback.into_iter().rev() {
                            channel_to_index_data.insert(ch, original);
                        }
                        continue;
                    }
                }
                metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_SUCCESS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(1);
                // Changed by G3 (DR-135, S5): the candidates moved out for the
                // tuple test.
                // data_candidates: data_candidates.into_iter().flatten().collect(),
                return Some(ProduceCandidate {
                    channels,
                    // Deref-clone the inner continuation only on a successful
                    // match — failed probes never deep-clone.
                    continuation: (**cont).clone(),
                    continuation_index: *index,
                    data_candidates: candidates,
                });
            }

            // Restore only the channels that were actually mutated
            // Changed by DR-129 (R3): in reverse, as above.
            // for (ch, original) in rollback {
            for (ch, original) in rollback.into_iter().rev() {
                channel_to_index_data.insert(ch, original);
            }
        }
        None
    }
}
