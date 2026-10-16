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
use crate::rspace::rspace_interface::{
    ContResult, MaybeConsumeResult, RSpaceAccountingObserver, RSpaceOperationCompletion,
    RSpaceOperationSource, RSpaceResult,
};
use crate::rspace::space_matcher::SpaceMatcher;
use crate::rspace::trace::event::{COMM, Consume, Produce};

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone
        + Debug
        + Default
        + Serialize
        + crate::rspace::hashing::stable_hash_provider::StableHashSerialize
        + Hash
        + Ord
        + Eq
        + 'static
        + Sync
        + Send,
    P: Clone
        + Debug
        + Default
        + Serialize
        + crate::rspace::hashing::stable_hash_provider::StableHashSerialize
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + crate::rspace::hashing::stable_hash_provider::StableHashSerialize
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + crate::rspace::hashing::stable_hash_provider::StableHashSerialize
        + 'static
        + Sync
        + Send,
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

        let source = RSpaceOperationSource::Consume(consume_ref);
        let observer = self.start_accounting_operation(source, channels, &[])?;
        let result = self.locked_consume_observed(
            channels,
            patterns,
            continuation,
            persist,
            peeks,
            consume_ref,
            observer.as_deref(),
        );
        if let Some(observer) = observer {
            observer
                .observe_operation_finish(source, RSpaceOperationCompletion::from_result(&result));
        }
        result
    }

    fn locked_consume_observed(
        &self,
        channels: &[C],
        patterns: &[P],
        continuation: &K,
        persist: bool,
        peeks: &BTreeSet<i32>,
        consume_ref: &Consume,
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError> {
        let t0 = Instant::now();
        Self::observe_consume(
            observer,
            consume_ref,
            channels,
            patterns,
            continuation,
            persist,
            peeks,
        )?;
        metrics::counter!("rspace.consume.log_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t0.elapsed().as_nanos() as u64);

        let t1 = Instant::now();
        let mut channel_to_indexed_data = self.fetch_channel_to_index_data(channels)?;
        metrics::counter!("rspace.consume.fetch_data_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t1.elapsed().as_nanos() as u64);

        let t2 = Instant::now();
        let zipped: Vec<(&C, &P)> = channels.iter().zip(patterns.iter()).collect();
        let options = self.extract_prepared_data_candidates(
            &self.matcher,
            &zipped,
            continuation,
            &mut channel_to_indexed_data,
        );
        metrics::counter!("rspace.consume.match_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t2.elapsed().as_nanos() as u64);

        let wk = WaitingContinuation {
            patterns: std::sync::Arc::new(patterns.to_vec()),
            continuation: std::sync::Arc::new(continuation.clone()),
            persist,
            peeks: peeks.clone(),
            source: consume_ref.clone(),
        };

        match options {
            Some((data_candidates, prepared)) => {
                let t3 = Instant::now();
                let produce_counters_closure =
                    |produces: &[Produce]| self.produce_counters(produces);

                let comm = COMM::new(
                    &data_candidates,
                    consume_ref.clone(),
                    peeks.clone(),
                    produce_counters_closure,
                );
                Self::observe_comm(observer, &comm, continuation, persist, &data_candidates)?;
                let notification = self.step_observer.as_ref().map(|_| comm.clone());
                for candidate in &data_candidates {
                    self.get_store().get_joins(&candidate.channel)?;
                }
                let result = prepared.commit(|| {
                    self.log_consume(consume_ref);
                    self.log_comm(comm, CONSUME_COMM_LABEL);
                    self.store_persistent_data(&data_candidates)?;
                    Ok(self.wrap_result(channels, &wk, &data_candidates))
                })?;
                if let Some(comm) = notification {
                    self.observe_step_comm(
                        channels,
                        &wk,
                        &data_candidates,
                        &comm,
                        CONSUME_COMM_LABEL,
                    );
                }
                metrics::counter!("rspace.consume.process_match_ns", "source" => RSPACE_METRICS_SOURCE)
                    .increment(t3.elapsed().as_nanos() as u64);
                tracing::trace!(target: "f1r3fly.rspace.ops", mark = "finished-locked-consume", "locked_consume");
                result
            }
            _ => {
                let t3 = Instant::now();
                self.get_store().get_continuations(channels)?;
                for channel in channels {
                    self.get_store().get_joins(channel)?;
                }
                self.store_waiting_continuation(channels.to_vec(), wk)?;
                self.log_consume(consume_ref);
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
    ) -> Result<HashMap<C, Vec<(Datum<A>, i32)>>, RSpaceError> {
        let store = self.get_store();
        let mut map = HashMap::with_capacity(channels.len());
        for c in channels {
            let data = store.get_data(c)?;
            let shuffled_data = self.shuffle_with_index(data);
            map.insert(c.clone(), shuffled_data);
        }
        Ok(map)
    }

    fn store_waiting_continuation(
        &self,
        channels: Vec<C>,
        wc: WaitingContinuation<P, K>,
    ) -> Result<(), RSpaceError> {
        let store = self.get_store();
        store.put_continuation(&channels, wc)?;
        for channel in channels.iter() {
            store.put_join(channel, &channels)?;
        }
        Ok(())
    }

    fn store_persistent_data(
        &self,
        data_candidates: &[ConsumeCandidate<C, A>],
    ) -> Result<(), RSpaceError> {
        let mut sorted_candidates: Vec<_> = data_candidates.iter().collect();
        sorted_candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.datum_index));
        let store = self.get_store();
        for consume_candidate in sorted_candidates {
            if !consume_candidate.datum.persist {
                store.remove_datum(&consume_candidate.channel, consume_candidate.datum_index)?;
            }
        }
        Ok(())
    }

    pub(super) fn wrap_result(
        &self,
        channels: &[C],
        wk: &WaitingContinuation<P, K>,
        data_candidates: &[ConsumeCandidate<C, A>],
    ) -> MaybeConsumeResult<C, P, A, K> {
        let cont_result = ContResult {
            continuation: wk.continuation.as_ref().clone(),
            persistent: wk.persist,
            channels: channels.to_vec(),
            patterns: wk.patterns.as_ref().clone(),
            peek: !wk.peeks.is_empty(),
        };

        let rspace_results = data_candidates
            .iter()
            .map(|data_candidate| RSpaceResult {
                channel: data_candidate.channel.clone(),
                matched_datum: data_candidate.datum.a.as_ref().clone(),
                removed_datum: data_candidate.removed_datum.as_ref().clone(),
                persistent: data_candidate.datum.persist,
            })
            .collect();

        Some((cont_result, rspace_results))
    }
}
