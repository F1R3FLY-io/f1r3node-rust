// Produce-side operation internals: the locked produce path, the matcher
// driver loop, and candidate retirement.

use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::time::Instant;

use serde::Serialize;

use super::RSpace;
use crate::rspace::candidate_order::CandidateSource;
use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::stable_hash_provider::StableHashSerialize;
use crate::rspace::internal::*;
use crate::rspace::metrics_constants::{
    LOCKED_PRODUCE_SPAN, PRODUCE_COMM_LABEL, RSPACE_METRICS_SOURCE,
};
use crate::rspace::rspace_interface::{
    MaybeConsumeResult, MaybeProduceCandidate, MaybeProduceResult, ProduceCommitGuard,
    RSpaceAccountingObserver, RSpaceOperationCompletion, RSpaceOperationSource, commit_produce,
};
use crate::rspace::serializers::serializers::CandidateOrderingBytes;
use crate::rspace::space_matcher::SpaceMatcher;
use crate::rspace::trace::event::{COMM, Produce};

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone
        + Debug
        + Default
        + Serialize
        + StableHashSerialize
        + Hash
        + Ord
        + Eq
        + 'static
        + Sync
        + Send,
    P: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + StableHashSerialize + 'static + Sync + Send,
{
    pub(super) fn locked_produce(
        &self,
        channel: C,
        data: A,
        persist: bool,
        produce_ref: &Produce,
        guard: Option<&dyn ProduceCommitGuard>,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError> {
        // Span[F].traceI("locked-produce") from Scala
        let _span = tracing::info_span!(target: "f1r3fly.rspace", LOCKED_PRODUCE_SPAN).entered();
        tracing::trace!(target: "f1r3fly.rspace.ops", mark = "started-locked-produce", "locked_produce");

        let t0 = Instant::now();
        let grouped_channels = self.get_store().get_joins(&channel)?;
        metrics::counter!("rspace.produce.get_joins_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t0.elapsed().as_nanos() as u64);

        let source = RSpaceOperationSource::Produce(produce_ref);
        let observer = self.start_accounting_operation(
            source,
            std::slice::from_ref(&channel),
            &grouped_channels,
        )?;
        let result = self.locked_produce_observed(
            channel,
            data,
            persist,
            produce_ref,
            grouped_channels,
            observer.as_deref(),
            guard,
        );
        if let Some(observer) = observer {
            observer
                .observe_operation_finish(source, RSpaceOperationCompletion::from_result(&result));
        }
        result
    }

    fn locked_produce_observed(
        &self,
        channel: C,
        data: A,
        persist: bool,
        produce_ref: &Produce,
        grouped_channels: Vec<Vec<C>>,
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        guard: Option<&dyn ProduceCommitGuard>,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError> {
        Self::observe_produce(observer, produce_ref, &channel, &data, persist)?;
        let t1 = Instant::now();
        let extracted =
            self.extract_produce_candidate(grouped_channels, channel.clone(), Datum {
                a: std::sync::Arc::new(data.clone()),
                persist,
                source: produce_ref.clone(),
            })?;
        metrics::counter!("rspace.produce.extract_candidate_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t1.elapsed().as_nanos() as u64);

        match extracted {
            Some(produce_candidate) => {
                let t2 = Instant::now();
                let result = self
                    .process_match_found(produce_candidate, produce_ref, persist, observer, guard)
                    .map(|result| {
                        result.map(|consume_result| {
                            (consume_result.0, consume_result.1, produce_ref.clone())
                        })
                    });
                metrics::counter!("rspace.produce.process_match_ns", "source" => RSPACE_METRICS_SOURCE)
                    .increment(t2.elapsed().as_nanos() as u64);
                tracing::trace!(target: "f1r3fly.rspace.ops", mark = "finished-locked-produce", "locked_produce");
                result
            }
            None => {
                let t2 = Instant::now();
                let result = commit_produce(guard, || {
                    let result = self.store_data(channel, data, persist, produce_ref.clone())?;
                    self.log_produce(produce_ref, persist);
                    Ok(result)
                })?;
                metrics::counter!("rspace.produce.store_data_ns", "source" => RSPACE_METRICS_SOURCE)
                    .increment(t2.elapsed().as_nanos() as u64);
                tracing::trace!(target: "f1r3fly.rspace.ops", mark = "finished-locked-produce", "locked_produce");
                result
            }
        }
    }

    /*
     * Find produce candidate
     *
     * NOTE: On Rust side, we are NOT passing functions through. Instead just
     * the data. And then in 'run_matcher_for_channels' we call the
     * functions defined below
     */
    fn extract_produce_candidate(
        &self,
        grouped_channels: Vec<Vec<C>>,
        bat_channel: C,
        data: Datum<A>,
    ) -> Result<MaybeProduceCandidate<C, P, A, K>, RSpaceError> {
        let fetch_matching_continuations = |channels: Vec<C>| -> Result<
            Vec<(std::sync::Arc<WaitingContinuation<P, K>>, i32)>,
            RSpaceError,
        > {
            // Arc-shared fetch: probing continuations no longer deep-clones
            // the continuation body on every produce.
            let continuations = self.get_store().get_continuations_arc(&channels)?;
            Ok(self.shuffle_with_index(continuations))
        };

        /*
         * Here, we create a cache of the data at each channel as
         * `channelToIndexedData` which is used for finding matches.  When a
         * speculative match is found, we can remove the matching datum from
         * the remaining data candidates in the cache.
         *
         * Put another way, this allows us to speculatively remove matching
         * data without affecting the actual store contents.
         *
         * In this version, we also add the produced data directly to this
         * cache.
         */
        let fetch_matching_data = |channel| -> Result<(C, Vec<(Datum<A>, i32)>), RSpaceError> {
            let data_vec = self.get_store().get_data(&channel)?;
            let mut shuffled_data = self.shuffle_with_index(data_vec);
            if channel == bat_channel {
                shuffled_data.insert(0, (data.clone(), -1));
            }
            Ok((channel, shuffled_data))
        };

        self.run_matcher_for_channels(
            grouped_channels,
            fetch_matching_continuations,
            fetch_matching_data,
        )
    }

    fn process_match_found(
        &self,
        pc: ProduceCandidate<C, P, A, K>,
        produce_ref: &Produce,
        produce_persistent: bool,
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        guard: Option<&dyn ProduceCommitGuard>,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError> {
        let ProduceCandidate {
            channels,
            continuation,
            continuation_index,
            data_candidates,
            commit,
        } = pc;

        let WaitingContinuation {
            patterns: _patterns,
            continuation: _cont,
            persist,
            peeks,
            source: consume_ref,
        } = &continuation;

        let produce_counters_closure = |produces: &[Produce]| {
            let mut counters = self.produce_counters(produces);
            if !produce_persistent && produces.contains(produce_ref) {
                *counters.entry(produce_ref.clone()).or_insert(0) += 1;
            }
            counters
        };
        let comm = COMM::new(
            &data_candidates,
            consume_ref.clone(),
            peeks.clone(),
            produce_counters_closure,
        );
        Self::observe_comm(observer, &comm, _cont, *persist, &data_candidates)?;
        let notification = self.step_observer.as_ref().map(|_| comm.clone());
        for candidate in &data_candidates {
            self.get_store().get_joins(&candidate.channel)?;
        }
        let permit = commit.unwrap_or_default();
        let result = permit.commit_with(guard, || -> Result<_, RSpaceError> {
            if !persist {
                self.get_store()
                    .remove_continuation(&channels, continuation_index)?;
            }
            self.remove_matched_datum_and_join(&channels, &data_candidates)?;
            self.log_produce(produce_ref, produce_persistent);
            self.log_comm(comm, PRODUCE_COMM_LABEL);
            Ok(self.wrap_result(&channels, &continuation, &data_candidates))
        })??;
        if let Some(comm) = notification {
            self.observe_step_comm(
                &channels,
                &continuation,
                &data_candidates,
                &comm,
                PRODUCE_COMM_LABEL,
            );
        }
        Ok(result)
    }

    fn store_data(
        &self,
        channel: C,
        data: A,
        persist: bool,
        produce_ref: Produce,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError> {
        self.get_store().put_datum(&channel, Datum {
            a: std::sync::Arc::new(data),
            persist,
            source: produce_ref,
        })?;

        Ok(None)
    }

    fn remove_matched_datum_and_join(
        &self,
        channels: &[C],
        data_candidates: &[ConsumeCandidate<C, A>],
    ) -> Result<(), RSpaceError> {
        let mut sorted_candidates: Vec<_> = data_candidates.iter().collect();
        sorted_candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.datum_index));
        let store = self.get_store();
        for consume_candidate in sorted_candidates {
            let channel = &consume_candidate.channel;
            let datum_index = consume_candidate.datum_index;

            if datum_index >= 0 && !consume_candidate.datum.persist {
                store.remove_datum(channel, datum_index)?;
            }
            store.remove_join(channel, channels)?;
        }
        Ok(())
    }

    fn run_matcher_for_channels(
        &self,
        grouped_channels: Vec<Vec<C>>,
        fetch_matching_continuations: impl Fn(
            Vec<C>,
        ) -> Result<
            Vec<(std::sync::Arc<WaitingContinuation<P, K>>, i32)>,
            RSpaceError,
        >,
        fetch_matching_data: impl Fn(C) -> Result<(C, Vec<(Datum<A>, i32)>), RSpaceError>,
    ) -> Result<MaybeProduceCandidate<C, P, A, K>, RSpaceError> {
        for channels in &grouped_channels {
            let t_cont = Instant::now();
            let match_candidates = fetch_matching_continuations(channels.to_vec())?;
            metrics::counter!("rspace.matcher.fetch_continuations_ns", "source" => RSPACE_METRICS_SOURCE)
                .increment(t_cont.elapsed().as_nanos() as u64);
            metrics::counter!("rspace.matcher.continuations_returned", "source" => RSPACE_METRICS_SOURCE)
                .increment(match_candidates.len() as u64);

            let t_data = Instant::now();
            let channel_to_indexed_data: HashMap<C, Vec<(Datum<A>, i32)>> = channels
                .iter()
                .map(|c| fetch_matching_data(c.clone()))
                .collect::<Result<_, _>>()?;
            metrics::counter!("rspace.matcher.fetch_data_ns", "source" => RSPACE_METRICS_SOURCE)
                .increment(t_data.elapsed().as_nanos() as u64);

            let t_match = Instant::now();
            let first_match = self.extract_first_match(
                &self.matcher,
                channels.to_vec(),
                match_candidates,
                channel_to_indexed_data,
            );
            metrics::counter!("rspace.matcher.extract_first_match_ns", "source" => RSPACE_METRICS_SOURCE)
                .increment(t_match.elapsed().as_nanos() as u64);

            if let Some(produce_candidate) = first_match {
                return Ok(Some(produce_candidate));
            }
        }
        Ok(None)
    }

    pub(super) fn shuffle_with_index<D: CandidateOrderingBytes + CandidateSource>(
        &self,
        t: Vec<D>,
    ) -> Vec<(D, i32)> {
        crate::rspace::candidate_order::canonical_candidates(t)
    }
}
