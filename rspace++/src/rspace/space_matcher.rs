// See rspace/src/main/scala/coop/rchain/rspace/SpaceMatcher.scala

use std::collections::HashMap;

use super::r#match::Match;
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
        data: &[(std::sync::Arc<Datum<A>>, i32)],
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
        channel_to_indexed_data: &mut HashMap<C, IndexedData<A>>,
        rollback: &mut Vec<(C, IndexedData<A>)>,
    ) -> Vec<Option<ConsumeCandidate<C, A>>> {
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
        mut channel_to_index_data: HashMap<C, IndexedData<A>>,
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
                    restore(&mut channel_to_index_data, rollback);
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

            restore(&mut channel_to_index_data, rollback);
        }
        None
    }
}

/// Undoes speculative matches newest first, so a channel matched more than
/// once ends at the state it had before the first match.
fn restore<C: std::hash::Hash + Eq, D>(
    channel_to_index_data: &mut HashMap<C, Vec<D>>,
    rollback: Vec<(C, Vec<D>)>,
) {
    for (channel, original) in rollback.into_iter().rev() {
        channel_to_index_data.insert(channel, original);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};
    use std::sync::Arc;

    use serde::{Deserialize, Serialize};

    use super::SpaceMatcher;
    use crate::rspace::internal::{Datum, WaitingContinuation};
    use crate::rspace::r#match::Match;
    use crate::rspace::rspace::RSpace;
    use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use crate::rspace::trace::event::Consume;

    #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
    enum Pattern {
        #[default]
        Any,
        Exactly(String),
    }

    struct PatternMatch;

    impl Match<Pattern, String, String> for PatternMatch {
        fn get(&self, pattern: &Pattern, a: &String) -> Option<String> {
            match pattern {
                Pattern::Any => Some(a.clone()),
                Pattern::Exactly(expected) => (expected == a).then(|| a.clone()),
            }
        }
    }

    fn continuation(
        channels: &[String],
        patterns: Vec<Pattern>,
        body: &str,
    ) -> Arc<WaitingContinuation<Pattern, String>> {
        let body = body.to_string();
        Arc::new(WaitingContinuation {
            source: Consume::create(&channels.to_vec(), &patterns, &body, false),
            patterns,
            continuation: body,
            persist: false,
            peeks: BTreeSet::new(),
        })
    }

    fn datum(channel: &str, value: &str) -> Arc<Datum<String>> {
        Arc::new(Datum::create(&channel.to_string(), value.to_string(), false))
    }

    #[tokio::test]
    async fn a_candidate_that_fails_after_matching_one_channel_twice_leaves_its_data_for_the_next()
    {
        let mut kvm = InMemoryStoreManager::new();
        let space: RSpace<String, Pattern, String, String> =
            RSpace::create(kvm.r_space_stores().await.unwrap(), Arc::new(Box::new(PatternMatch)))
                .unwrap();
        let matcher: Box<dyn Match<Pattern, String, String>> = Box::new(PatternMatch);

        let channels = vec!["a".to_string(), "a".to_string(), "b".to_string()];
        let first = Pattern::Exactly("1".to_string());
        let second = Pattern::Exactly("2".to_string());
        let candidates = vec![
            (continuation(&channels, vec![Pattern::Any, Pattern::Any, first], "first"), 0),
            (continuation(&channels, vec![Pattern::Any, Pattern::Any, second], "second"), 1),
        ];
        let data = HashMap::from([
            ("a".to_string(), vec![(datum("a", "x"), 0), (datum("a", "y"), 1)]),
            ("b".to_string(), vec![(datum("b", "2"), 0)]),
        ]);

        let matched = space.extract_first_match(&matcher, channels, candidates, data);

        assert_eq!(
            matched.map(|candidate| candidate.continuation.continuation),
            Some("second".to_string())
        );
    }
}
