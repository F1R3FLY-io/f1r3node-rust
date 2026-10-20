// See rspace/src/main/scala/coop/rchain/rspace/SpaceMatcher.scala

use std::collections::HashMap;

use super::r#match::Match;
use super::rspace_interface::ISpace;
use crate::rspace::internal::{ConsumeCandidate, Datum, ProduceCandidate, WaitingContinuation};
use crate::rspace::metrics_constants::{
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CALLS_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CANDIDATES_ITERATED_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_PAIR_CONSTRUCTION_NS_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_SUCCESS_METRIC, RSPACE_METRICS_SOURCE,
};

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

pub trait SpaceMatcher<C, P, A, K>: ISpace<C, P, A, K>
where
    C: Clone + std::hash::Hash + Eq + Send + Sync,
    P: Clone + Send + Sync,
    A: Clone + Send + Sync,
    K: Clone + Send + Sync,
{
    fn find_matching_data_candidate(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channel: C,
        data: &[(Datum<A>, i32)],
        pattern: &P,
    ) -> Option<MatchingDataCandidate<C, A>> {
        for (idx, (datum, data_index)) in data.iter().enumerate() {
            metrics::counter!("rspace.matcher.get_calls", "source" => "rspace").increment(1);
            // Probe by reference — no whole-pattern/whole-datum clone per attempt.
            let t_match = std::time::Instant::now();
            let match_result = matcher.get(pattern, &datum.a);
            metrics::counter!("rspace.matcher.fold_match_ns", "source" => "rspace")
                .increment(t_match.elapsed().as_nanos() as u64);

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
    fn extract_data_candidates_rollback(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channel_pattern_pairs: &[(C, P)],
        channel_to_indexed_data: &mut HashMap<C, Vec<(Datum<A>, i32)>>,
        rollback: &mut Vec<(C, Vec<(Datum<A>, i32)>)>,
    ) -> Vec<Option<ConsumeCandidate<C, A>>> {
        // Added by DR-129 (E1): a stable order for a channel that the join
        // names more than once.
        order_repeated_channels(channel_pattern_pairs, channel_to_indexed_data, rollback);
        let mut acc = Vec::with_capacity(channel_pattern_pairs.len());

        for (channel, pattern) in channel_pattern_pairs {
            let maybe_tuple: Option<MatchingDataCandidate<C, A>> =
                match channel_to_indexed_data.get(channel) {
                    Some(indexed_data) => self.find_matching_data_candidate(
                        matcher,
                        channel.clone(),
                        indexed_data,
                        pattern,
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
        self.extract_data_candidates_rollback(
            matcher,
            channel_pattern_pairs,
            channel_to_indexed_data,
            &mut rollback,
        )
    }

    fn extract_first_match(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channels: Vec<C>,
        match_candidates: Vec<(std::sync::Arc<WaitingContinuation<P, K>>, i32)>,
        mut channel_to_index_data: HashMap<C, Vec<(Datum<A>, i32)>>,
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
            let data_candidates = self.extract_data_candidates_rollback(
                matcher,
                &channel_pattern_pairs,
                &mut channel_to_index_data,
                &mut rollback,
            );

            if data_candidates.iter().all(|x| x.is_some()) {
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
                metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_SUCCESS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                    .increment(1);
                return Some(ProduceCandidate {
                    channels,
                    // Deref-clone the inner continuation only on a successful
                    // match — failed probes never deep-clone.
                    continuation: (**cont).clone(),
                    continuation_index: *index,
                    data_candidates: data_candidates.into_iter().flatten().collect(),
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
