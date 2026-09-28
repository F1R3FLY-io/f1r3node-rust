// Consume-side operation internals: the locked consume path and its
// store helpers.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Debug;
use std::hash::Hash;
use std::time::Instant;

use serde::Serialize;

use super::RSpace;
use crate::rspace::errors::RSpaceError;
use crate::rspace::internal::*;
use crate::rspace::metrics_constants::{
    CONSUME_COMM_LABEL, LOCKED_CONSUME_SPAN, RSPACE_METRICS_SOURCE,
};
use crate::rspace::rspace_interface::{ContResult, MaybeConsumeResult, RSpaceResult};
use crate::rspace::space_matcher::SpaceMatcher;
use crate::rspace::trace::event::{COMM, Consume, Produce};

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    pub(super) fn locked_consume(
        &self,
        channels: &[C],
        patterns: &[P],
        continuation: &K,
        persist: bool,
        peeks: &BTreeSet<i32>,
        consume_ref: &Consume,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError> {
        // Span[F].traceI("locked-consume") from Scala
        let _span = tracing::info_span!(target: "f1r3fly.rspace", LOCKED_CONSUME_SPAN).entered();
        tracing::trace!(target: "f1r3fly.rspace.ops", mark = "started-locked-consume", "locked_consume");

        let t0 = Instant::now();
        self.log_consume(consume_ref);
        metrics::counter!("rspace.consume.log_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t0.elapsed().as_nanos() as u64);

        let t1 = Instant::now();
        let mut channel_to_indexed_data = self.fetch_channel_to_index_data(channels);
        metrics::counter!("rspace.consume.fetch_data_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t1.elapsed().as_nanos() as u64);

        let t2 = Instant::now();
        let zipped: Vec<(C, P)> = channels
            .iter()
            .cloned()
            .zip(patterns.iter().cloned())
            .collect();
        let options: Option<Vec<ConsumeCandidate<C, A>>> = self
            .extract_data_candidates(&self.matcher, &zipped, &mut channel_to_indexed_data)
            .into_iter()
            .collect();
        metrics::counter!("rspace.consume.match_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t2.elapsed().as_nanos() as u64);

        let wk = WaitingContinuation {
            patterns: patterns.to_vec(),
            continuation: continuation.clone(),
            persist,
            peeks: peeks.clone(),
            source: consume_ref.clone(),
        };

        let commit_ok = match &options {
            // Cross-channel commit hook: a `where` guard on the consume
            // can veto even after every spatial bind matched. On false
            // we install the wk and leave the data alone, just as if
            // the spatial match itself had failed (plan §7.12).
            Some(data_candidates) => {
                let matched: Vec<A> = data_candidates.iter().map(|c| c.datum.a.clone()).collect();
                self.matcher.check_commit(continuation, &matched)
            }
            None => false,
        };

        match options {
            Some(data_candidates) if commit_ok => {
                let t3 = Instant::now();
                let produce_counters_closure =
                    |produces: &[Produce]| self.produce_counters(produces);

                self.log_comm(
                    COMM::new(
                        &data_candidates,
                        consume_ref.clone(),
                        peeks.clone(),
                        produce_counters_closure,
                    ),
                    CONSUME_COMM_LABEL,
                );
                self.store_persistent_data(&data_candidates);
                metrics::counter!("rspace.consume.process_match_ns", "source" => RSPACE_METRICS_SOURCE)
                    .increment(t3.elapsed().as_nanos() as u64);
                tracing::trace!(target: "f1r3fly.rspace.ops", mark = "finished-locked-consume", "locked_consume");
                Ok(self.wrap_result(channels, &wk, &data_candidates))
            }
            _ => {
                let t3 = Instant::now();
                self.store_waiting_continuation(channels.to_vec(), wk);
                metrics::counter!("rspace.consume.store_continuation_ns", "source" => RSPACE_METRICS_SOURCE)
                    .increment(t3.elapsed().as_nanos() as u64);
                tracing::trace!(target: "f1r3fly.rspace.ops", mark = "finished-locked-consume", "locked_consume");
                Ok(None)
            }
        }
    }

    /*
     * Here, we create a cache of the data at each channel as
     * `channelToIndexedData` which is used for finding matches.  When a
     * speculative match is found, we can remove the matching datum from the
     * remaining data candidates in the cache.
     *
     * Put another way, this allows us to speculatively remove matching data
     * without affecting the actual store contents.
     */
    pub(super) fn fetch_channel_to_index_data(
        &self,
        channels: &[C],
    ) -> HashMap<C, Vec<(Datum<A>, i32)>> {
        let store = self.get_store();
        let mut map = HashMap::with_capacity(channels.len());
        for c in channels {
            let data = store.get_data(c);
            let shuffled_data = self.shuffle_with_index(data);
            map.insert(c.clone(), shuffled_data);
        }
        map
    }

    fn store_waiting_continuation(
        &self,
        channels: Vec<C>,
        wc: WaitingContinuation<P, K>,
    ) -> MaybeConsumeResult<C, P, A, K> {
        let store = self.get_store();
        let _ = store.put_continuation(&channels, wc);
        for channel in channels.iter() {
            store.put_join(channel, &channels);
        }
        None
    }

    fn store_persistent_data(&self, data_candidates: &[ConsumeCandidate<C, A>]) {
        let mut sorted_candidates: Vec<_> = data_candidates.iter().collect();
        sorted_candidates.sort_by_key(|candidate| candidate.datum_index);
        let store = self.get_store();
        for consume_candidate in sorted_candidates {
            if !consume_candidate.datum.persist {
                let _ =
                    store.remove_datum(&consume_candidate.channel, consume_candidate.datum_index);
            }
        }
    }

    pub(super) fn wrap_result(
        &self,
        channels: &[C],
        wk: &WaitingContinuation<P, K>,
        data_candidates: &[ConsumeCandidate<C, A>],
    ) -> MaybeConsumeResult<C, P, A, K> {
        let cont_result = ContResult {
            continuation: wk.continuation.clone(),
            persistent: wk.persist,
            channels: channels.to_vec(),
            patterns: wk.patterns.clone(),
            peek: !wk.peeks.is_empty(),
        };

        let rspace_results = data_candidates
            .iter()
            .map(|data_candidate| RSpaceResult {
                channel: data_candidate.channel.clone(),
                matched_datum: data_candidate.datum.a.clone(),
                removed_datum: data_candidate.removed_datum.clone(),
                persistent: data_candidate.datum.persist,
            })
            .collect();

        Some((cont_result, rspace_results))
    }
}
