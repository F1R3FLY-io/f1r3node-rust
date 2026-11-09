// Produce-side operation internals: the locked produce path, the matcher
// driver loop, and candidate retirement.

use std::collections::HashMap;
// Added by decision 123 (plan v4, V3-2): the lists of native play.
use std::convert::Infallible;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Instant;

// Changed by decision 123 (plan v4, V3-8): the play shuffle moved to
// `candidate_order.rs`, so this file no longer shuffles.
// use rand::seq::SliceRandom;
use serde::Serialize;

use super::RSpace;
// Added by G3 (DR-135, S5): the owner checks of an owner-funded produce.
use super::accounting::ProduceAdmission;
// Changed by DR-129: restored dev's randomized candidate order (user decision
// 2026-10-12). Play no longer calls the canonical order.
// use crate::rspace::candidate_order::CandidateSource;
use crate::rspace::errors::RSpaceError;
// Added by decision 123 (plan v4, V3-8): the free store steps.
use crate::rspace::hot_store::HotStore;
use crate::rspace::internal::*;
// Added by decision 123 (plan v4, V3-2): the matcher of the shared search.
use crate::rspace::r#match::Match;
use crate::rspace::metrics_constants::{
    LOCKED_PRODUCE_SPAN, PRODUCE_COMM_LABEL, RSPACE_METRICS_SOURCE,
};
// Added by G3 (DR-135, S5): the owner checks of a match.
use crate::rspace::operation_context::MatchAdmission;
use crate::rspace::rspace_interface::{
    MaybeConsumeResult, MaybeProduceCandidate, MaybeProduceResult, RSpaceAccountingObserver,
    RSpaceOperationCompletion, RSpaceOperationSource,
};
// Changed by decision 123 (plan v4, V3-2): the shared produce search.
// use crate::rspace::space_matcher::SpaceMatcher;
use crate::rspace::space_matcher::{MatchCandidates, SpaceMatcher, StoreGroup, first_match};
// Changed by decision 123 (plan v4, R8): the event sink of the commit steps.
// use crate::rspace::trace::event::{COMM, Produce};
use crate::rspace::trace::event::{COMM, Event, IOEvent, Produce};

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    pub(super) fn locked_produce(
        &self,
        channel: C,
        data: A,
        persist: bool,
        produce_ref: &Produce,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError> {
        // Span[F].traceI("locked-produce") from Scala
        let _span = tracing::info_span!(target: "f1r3fly.rspace", LOCKED_PRODUCE_SPAN).entered();
        tracing::trace!(target: "f1r3fly.rspace.ops", mark = "started-locked-produce", "locked_produce");

        let t0 = Instant::now();
        let grouped_channels = self.get_store().get_joins(&channel);
        metrics::counter!("rspace.produce.get_joins_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t0.elapsed().as_nanos() as u64);

        let source = RSpaceOperationSource::Produce(produce_ref);
        // Changed by DR-132: the observer names the recorded row, and the rest
        // of the locked produce runs under that name.
        // let observer = self.start_accounting_operation(
        //     source,
        //     std::slice::from_ref(&channel),
        //     &grouped_channels,
        // )?;
        // Changed by decision 123 (plan v4, V3-8, R10): the frame after the
        // start is the shared `run_observed`.
        // let (observer, name) = self.start_accounting_operation(
        //     source,
        //     std::slice::from_ref(&channel),
        //     &grouped_channels,
        // )?;
        // crate::rspace::operation_context::named(name, move || {
        //     let result = self.locked_produce_observed(
        //         channel,
        //         data,
        //         persist,
        //         produce_ref,
        //         grouped_channels,
        //         observer.as_deref(),
        //     );
        //     if let Some(observer) = observer {
        //         observer.observe_operation_finish(
        //             source,
        //             RSpaceOperationCompletion::from_result(&result),
        //         );
        //     }
        //     result
        // })
        let start = self.start_accounting_operation(
            source,
            std::slice::from_ref(&channel),
            &grouped_channels,
        )?;
        Self::run_observed(source, start, RSpaceOperationCompletion::from_result, move |observer| {
            self.locked_produce_observed(
                channel,
                data,
                persist,
                produce_ref,
                grouped_channels,
                observer,
            )
        })
    }

    fn locked_produce_observed(
        &self,
        channel: C,
        data: A,
        persist: bool,
        produce_ref: &Produce,
        grouped_channels: Vec<Vec<C>>,
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError> {
        Self::observe_produce(observer, produce_ref, &channel, &data, persist)?;
        // Added by G3 (DR-135, S5): with owner funding the matcher runs the
        // owner checks (R5, R6), and the tuple test observes the COMM.
        let admission = observer
            .filter(|observer| observer.owner_checks_active())
            .map(|observer| ProduceAdmission::new(self, observer, produce_ref, persist, &channel));
        let t1 = Instant::now();
        // Changed by G3 (DR-135, S5): the extraction takes the owner checks.
        // let extracted = self.extract_produce_candidate(
        //     grouped_channels, channel.clone(), Datum {
        //     a: data.clone(),
        //     persist,
        //     source: produce_ref.clone(),
        // });
        let extracted = self.extract_produce_candidate(
            grouped_channels,
            channel.clone(),
            Datum {
                a: data.clone(),
                persist,
                source: produce_ref.clone(),
            },
            admission
                .as_ref()
                .map(|admission| admission as &dyn MatchAdmission<C, P, A, K>),
        );
        metrics::counter!("rspace.produce.extract_candidate_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t1.elapsed().as_nanos() as u64);
        // Added by G3 (DR-135, S5): a hard error that an owner check kept.
        if let Some(error) = admission.as_ref().and_then(ProduceAdmission::take_error) {
            return Err(error);
        }

        match extracted {
            Some(produce_candidate) => {
                let t2 = Instant::now();
                // Changed by G3 (DR-135, S5): the tuple test observed the COMM
                // of an owner-funded match.
                // .process_match_found(produce_candidate, produce_ref, persist, observer)
                let result = self
                    .process_match_found(
                        produce_candidate,
                        produce_ref,
                        persist,
                        observer,
                        admission.is_some(),
                    )
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
                self.log_produce(produce_ref, persist);
                let result = Ok(self.store_data(channel, data, persist, produce_ref.clone()));
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
     * NOTE: On Rust side, we are NOT passing functions through. Instead just the
     * data. And then in 'run_matcher_for_channels' we call the functions
     * defined below
     */
    // Changed by G3 (DR-135, S5): the search takes the owner checks of an
    // owner-funded produce.
    // fn extract_produce_candidate(
    //     &self,
    //     grouped_channels: Vec<Vec<C>>,
    //     bat_channel: C,
    //     data: Datum<A>,
    // ) -> MaybeProduceCandidate<C, P, A, K> {
    fn extract_produce_candidate(
        &self,
        grouped_channels: Vec<Vec<C>>,
        bat_channel: C,
        data: Datum<A>,
        admission: Option<&dyn MatchAdmission<C, P, A, K>>,
    ) -> MaybeProduceCandidate<C, P, A, K> {
        let fetch_matching_continuations =
            |channels: Vec<C>| -> Vec<(std::sync::Arc<WaitingContinuation<P, K>>, i32)> {
                // Arc-shared fetch: probing continuations no longer deep-clones
                // the continuation body on every produce.
                let continuations = self.get_store().get_continuations_arc(&channels);
                self.shuffle_with_index(continuations)
            };

        /*
         * Here, we create a cache of the data at each channel as
         * `channelToIndexedData` which is used for finding matches.  When a
         * speculative match is found, we can remove the matching datum from
         * the remaining data candidates in the cache.
         *
         * Put another way, this allows us to speculatively remove matching data
         * without affecting the actual store contents.
         *
         * In this version, we also add the produced data directly to this cache.
         */
        let fetch_matching_data = |channel| -> (C, Vec<(Datum<A>, i32)>) {
            let data_vec = self.get_store().get_data(&channel);
            let mut shuffled_data = self.shuffle_with_index(data_vec);
            if channel == bat_channel {
                shuffled_data.insert(0, (data.clone(), -1));
            }
            (channel, shuffled_data)
        };

        // Changed by G3 (DR-135, S5): with the owner checks.
        // self.run_matcher_for_channels(
        //     grouped_channels,
        //     fetch_matching_continuations,
        //     fetch_matching_data,
        // )
        self.run_matcher_for_channels(
            grouped_channels,
            fetch_matching_continuations,
            fetch_matching_data,
            admission,
        )
    }

    // Changed by G3 (DR-135, S5): the tuple test of an owner-funded match
    // already observed the COMM.
    // fn process_match_found(
    //     &self,
    //     pc: ProduceCandidate<C, P, A, K>,
    //     produce_ref: &Produce,
    //     produce_persistent: bool,
    //     observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
    // ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError> {
    fn process_match_found(
        &self,
        pc: ProduceCandidate<C, P, A, K>,
        produce_ref: &Produce,
        produce_persistent: bool,
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        comm_observed: bool,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError> {
        let ProduceCandidate {
            channels,
            continuation,
            continuation_index,
            data_candidates,
        } = pc;

        let WaitingContinuation {
            patterns: _patterns,
            continuation: _cont,
            persist,
            peeks,
            source: consume_ref,
        } = &continuation;

        // Changed by G3 (DR-135, S5): the COMM is built as the tuple test
        // builds it.
        // let produce_counters_closure = |produces: &[Produce]| {
        //     let mut counters = self.produce_counters(produces);
        //     if !produce_persistent && produces.contains(produce_ref) {
        //         *counters.entry(produce_ref.clone()).or_insert(0) += 1;
        //     }
        //     counters
        // };
        // let comm = COMM::new(
        //     &data_candidates,
        //     consume_ref.clone(),
        //     peeks.clone(),
        //     produce_counters_closure,
        // );
        // Self::observe_comm(observer, &comm, _cont, *persist, &data_candidates)?;
        let comm = self.produce_comm(
            &data_candidates,
            consume_ref,
            peeks,
            produce_ref,
            produce_persistent,
        );
        if !comm_observed {
            Self::observe_comm(observer, &comm, _cont, *persist, &data_candidates)?;
        }
        // Changed by decision 123 (plan v4, R8): the commit steps are the
        // shared `commit_produce_match`, which Reified play also calls.
        // self.log_produce(produce_ref, produce_persistent);
        // self.log_comm(comm, PRODUCE_COMM_LABEL);
        //
        // if !persist {
        //     self.get_store()
        //         .remove_continuation(&channels, continuation_index);
        // }
        //
        // self.remove_matched_datum_and_join(&channels, &data_candidates);
        //
        // Ok(self.wrap_result(&channels, &continuation, &data_candidates))
        let store = self.get_store();
        Ok(self.commit_produce_match(
            &**store,
            produce_ref,
            produce_persistent,
            comm,
            &channels,
            &continuation,
            (!*persist).then_some(continuation_index),
            &data_candidates,
            &data_candidates,
            None,
        ))
    }

    /// Added by decision 123 (plan v4, R8): the commit steps of a produce
    /// match, the tail of the base `process_match_found`. It logs the produce
    /// and the COMM. It removes the continuation at `removed_continuation`
    /// from `store`, which is None for a persistent continuation. Then it
    /// removes the matched records of `removal` and their joins. The result
    /// wraps `candidates`. An event sink also gets the two log events, in
    /// order.
    pub(super) fn commit_produce_match(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        produce_ref: &Produce,
        produce_persistent: bool,
        comm: COMM,
        channels: &[C],
        continuation: &WaitingContinuation<P, K>,
        removed_continuation: Option<i32>,
        candidates: &[ConsumeCandidate<C, A>],
        removal: &[ConsumeCandidate<C, A>],
        events: Option<&mut Vec<Event>>,
    ) -> MaybeConsumeResult<C, P, A, K> {
        self.log_produce(produce_ref, produce_persistent);
        match events {
            Some(events) => {
                events.push(Event::IoEvent(IOEvent::Produce(produce_ref.clone())));
                self.log_comm(comm.clone(), PRODUCE_COMM_LABEL);
                events.push(Event::Comm(comm));
            }
            None => self.log_comm(comm, PRODUCE_COMM_LABEL),
        }
        if let Some(index) = removed_continuation {
            store.remove_continuation(channels, index);
        }
        remove_matched_datum_and_join(store, channels, removal);
        self.wrap_result(channels, continuation, candidates)
    }

    /// Added by G3 (DR-135, S5): the COMM of a produce that matched
    /// `data_candidates`. The tuple test and the match build it in one way:
    /// a non-persistent incoming produce counts once more.
    pub(super) fn produce_comm(
        &self,
        data_candidates: &[ConsumeCandidate<C, A>],
        consume_ref: &crate::rspace::trace::event::Consume,
        peeks: &std::collections::BTreeSet<i32>,
        produce_ref: &Produce,
        produce_persistent: bool,
    ) -> COMM {
        // Changed by decision 123 (plan v4, V3-8): the body moved to the free
        // `produce_comm_with`, which the Reified space transactions also call.
        // The method forwards with the produce counters of this space.
        // let produce_counters_closure = |produces: &[Produce]| {
        //     let mut counters = self.produce_counters(produces);
        //     if !produce_persistent && produces.contains(produce_ref) {
        //         *counters.entry(produce_ref.clone()).or_insert(0) += 1;
        //     }
        //     counters
        // };
        // COMM::new(
        //     data_candidates,
        //     consume_ref.clone(),
        //     peeks.clone(),
        //     produce_counters_closure,
        // )
        produce_comm_with(
            data_candidates,
            consume_ref,
            peeks,
            Some((produce_ref, produce_persistent)),
            |produces| self.produce_counters(produces),
        )
    }

    fn store_data(
        &self,
        channel: C,
        data: A,
        persist: bool,
        produce_ref: Produce,
    ) -> MaybeProduceResult<C, P, A, K> {
        // Changed by decision 123 (plan v4, V3-8): the store step is the free
        // `store_data` over the store, which Reified play also calls. The
        // method forwards.
        // self.get_store().put_datum(&channel, Datum {
        //     a: data,
        //     persist,
        //     source: produce_ref,
        // });
        let store = self.get_store();
        store_data(&**store, channel, data, persist, produce_ref);

        None
    }

    // Retired by decision 123 (plan v4, R8): `commit_produce_match` calls the
    // free `remove_matched_datum_and_join` with the store of the operation,
    // so this method has no caller. The retired method:
    // ```
    // fn remove_matched_datum_and_join(
    //     &self,
    //     channels: &[C],
    //     data_candidates: &[ConsumeCandidate<C, A>],
    // ) {
    //     let mut sorted_candidates: Vec<_> = data_candidates.iter().collect();
    //     sorted_candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.datum_index));
    //     let store = self.get_store();
    //     for consume_candidate in sorted_candidates {
    //         let channel = &consume_candidate.channel;
    //         let datum_index = consume_candidate.datum_index;
    //
    //         if datum_index >= 0 &&
    //             !consume_candidate.datum.persist &&
    //             store.remove_datum(channel, datum_index).is_err()
    //         {
    //             continue;
    //         }
    //         store.remove_join(channel, channels);
    //     }
    // }
    // ```

    // Changed by G3 (DR-135, S5): with the owner checks of an owner-funded
    // produce.
    // fn run_matcher_for_channels(
    //     &self,
    //     grouped_channels: Vec<Vec<C>>,
    //     fetch_matching_continuations: impl Fn(
    //         Vec<C>,
    //     )
    //         -> Vec<(std::sync::Arc<WaitingContinuation<P, K>>, i32)>,
    //     fetch_matching_data: impl Fn(C) -> (C, Vec<(Datum<A>, i32)>),
    // ) -> MaybeProduceCandidate<C, P, A, K> {
    // Changed by decision 123 (plan v4, V3-2, N5): the body moved to the free
    // `run_matcher`, which runs over the `StoreLists` of the two fetch
    // closures here and over the runs of a Reified row elsewhere. The old
    // body:
    // ```
    // fn run_matcher_for_channels(
    //     &self,
    //     grouped_channels: Vec<Vec<C>>,
    //     fetch_matching_continuations: impl Fn(
    //         Vec<C>,
    //     )
    //         -> Vec<(std::sync::Arc<WaitingContinuation<P, K>>, i32)>,
    //     fetch_matching_data: impl Fn(C) -> (C, Vec<(Datum<A>, i32)>),
    //     admission: Option<&dyn MatchAdmission<C, P, A, K>>,
    // ) -> MaybeProduceCandidate<C, P, A, K> {
    //     for channels in &grouped_channels {
    //         let t_cont = Instant::now();
    //         let match_candidates = fetch_matching_continuations(channels.to_vec());
    //         metrics::counter!("rspace.matcher.fetch_continuations_ns", "source" => RSPACE_METRICS_SOURCE)
    //             .increment(t_cont.elapsed().as_nanos() as u64);
    //         metrics::counter!("rspace.matcher.continuations_returned", "source" => RSPACE_METRICS_SOURCE)
    //             .increment(match_candidates.len() as u64);
    //
    //         let t_data = Instant::now();
    //         let channel_to_indexed_data: HashMap<C, Vec<(Datum<A>, i32)>> = channels
    //             .iter()
    //             .map(|c| fetch_matching_data(c.clone()))
    //             .collect();
    //         metrics::counter!("rspace.matcher.fetch_data_ns", "source" => RSPACE_METRICS_SOURCE)
    //             .increment(t_data.elapsed().as_nanos() as u64);
    //
    //         let t_match = Instant::now();
    //         // Changed by G3 (DR-135, S5): with the owner checks.
    //         // let first_match = self.extract_first_match(
    //         //     &self.matcher,
    //         //     channels.to_vec(),
    //         //     match_candidates,
    //         //     channel_to_indexed_data,
    //         // );
    //         let first_match = self.extract_first_match(
    //             &self.matcher,
    //             channels.to_vec(),
    //             match_candidates,
    //             channel_to_indexed_data,
    //             admission,
    //         );
    //         metrics::counter!("rspace.matcher.extract_first_match_ns", "source" => RSPACE_METRICS_SOURCE)
    //             .increment(t_match.elapsed().as_nanos() as u64);
    //
    //         if let Some(produce_candidate) = first_match {
    //             return Some(produce_candidate);
    //         }
    //     }
    //     None
    // }
    // ```
    fn run_matcher_for_channels(
        &self,
        grouped_channels: Vec<Vec<C>>,
        fetch_matching_continuations: impl Fn(
            Vec<C>,
        )
            -> Vec<(std::sync::Arc<WaitingContinuation<P, K>>, i32)>,
        fetch_matching_data: impl Fn(C) -> (C, Vec<(Datum<A>, i32)>),
        admission: Option<&dyn MatchAdmission<C, P, A, K>>,
    ) -> MaybeProduceCandidate<C, P, A, K> {
        let mut lists = StoreLists {
            space: self,
            matcher: &self.matcher,
            fetch_matching_continuations,
            fetch_matching_data,
        };
        let Ok(found) = run_matcher(&**self.matcher, &grouped_channels, &mut lists, admission);
        found.map(|(_, produce_candidate)| produce_candidate)
    }

    // Changed by DR-129: restored dev's randomized candidate order (user
    // decision 2026-10-12). Native replay follows the recorded pick, so play
    // does not need the canonical order of DR-75. Replay keeps that order as its
    // own internal order (`native_candidate.rs`, `native_candidate/selection.rs`).
    // pub(super) fn shuffle_with_index<D: Serialize + CandidateSource>(
    //     &self,
    //     t: Vec<D>,
    // ) -> Vec<(D, i32)> {
    //     crate::rspace::candidate_order::canonical_candidates(t)
    // }
    // Changed by decision 123 (plan v4, V3-8): the body moved to the free
    // `candidate_order::shuffle_with_index`, which the Reified disciplines
    // also call (through `space_store_handle::play_order`). The method
    // forwards, so native play draws the same random numbers in the same
    // order.
    // pub(super) fn shuffle_with_index<D>(&self, t: Vec<D>) -> Vec<(D, i32)> {
    //     let mut indexed_vec = t
    //         .into_iter()
    //         .enumerate()
    //         .map(|(i, d)| (d, i as i32))
    //         .collect::<Vec<_>>();
    //     if indexed_vec.len() >= 2 {
    //         indexed_vec.shuffle(&mut rand::rng());
    //     }
    //     indexed_vec
    // }
    pub(super) fn shuffle_with_index<D>(&self, t: Vec<D>) -> Vec<(D, i32)> {
        crate::rspace::candidate_order::shuffle_with_index(t)
    }
}

/// Added by decision 123 (plan v4, V3-2): the lists of native play. A group
/// reads its lists only when the search reaches it, in the base order. The
/// continuations come first, and then the data of each channel occurrence,
/// each with its base metric. A channel that the group names twice is fetched
/// and shuffled twice, and the map keeps the last fetch, as in the base.
pub(super) struct StoreLists<'s, S: ?Sized, P, A, K, FC, FD> {
    space: &'s S,
    matcher: &'s Box<dyn Match<P, A, K>>,
    fetch_matching_continuations: FC,
    fetch_matching_data: FD,
}

impl<S, C, P, A, K, FC, FD> MatchCandidates<C, P, A, K> for StoreLists<'_, S, P, A, K, FC, FD>
where
    S: SpaceMatcher<C, P, A, K> + ?Sized,
    C: Clone + Hash + Eq + Send + Sync,
    P: Clone + Send + Sync,
    A: Clone + Send + Sync,
    K: Clone + Send + Sync,
    FC: Fn(Vec<C>) -> Vec<(Arc<WaitingContinuation<P, K>>, i32)>,
    FD: Fn(C) -> (C, Vec<(Datum<A>, i32)>),
{
    type Error = Infallible;
    type Group<'g>
        = StoreGroup<'g, S, C, P, A, K>
    where
        Self: 'g,
        C: 'g;

    fn group<'g>(
        &'g mut self,
        _index: usize,
        channels: &'g [C],
    ) -> Result<StoreGroup<'g, S, C, P, A, K>, Infallible> {
        let t_cont = Instant::now();
        let match_candidates = (self.fetch_matching_continuations)(channels.to_vec());
        metrics::counter!("rspace.matcher.fetch_continuations_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t_cont.elapsed().as_nanos() as u64);
        metrics::counter!("rspace.matcher.continuations_returned", "source" => RSPACE_METRICS_SOURCE)
            .increment(match_candidates.len() as u64);

        let t_data = Instant::now();
        let channel_to_indexed_data: HashMap<C, Vec<(Datum<A>, i32)>> = channels
            .iter()
            .map(|c| (self.fetch_matching_data)(c.clone()))
            .collect();
        metrics::counter!("rspace.matcher.fetch_data_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t_data.elapsed().as_nanos() as u64);

        Ok(StoreGroup::fetched(
            self.space,
            self.matcher,
            match_candidates,
            channel_to_indexed_data,
            channels.len(),
        ))
    }
}

/// Added by decision 123 (plan v4, V3-2, N5): the produce search over the
/// groups of any source, the body of the base `run_matcher_for_channels`.
/// The groups are visited in order, and each group builds its lists only when
/// the search reaches it. The first match returns with the index of its
/// group. For native play each join group is one run, so the visit order and
/// the metrics are those of the base.
pub(super) fn run_matcher<C, P, A, K, S>(
    matcher: &dyn Match<P, A, K>,
    groups: &[Vec<C>],
    source: &mut S,
    admission: Option<&dyn MatchAdmission<C, P, A, K>>,
) -> Result<Option<(usize, ProduceCandidate<C, P, A, K>)>, S::Error>
where
    C: Clone,
    P: Clone,
    A: Clone,
    K: Clone,
    S: MatchCandidates<C, P, A, K> + ?Sized,
{
    for (index, channels) in groups.iter().enumerate() {
        let mut group = source.group(index, channels)?;
        let t_match = Instant::now();
        let found = first_match(matcher, channels.to_vec(), &mut group, admission)?;
        metrics::counter!("rspace.matcher.extract_first_match_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t_match.elapsed().as_nanos() as u64);

        if let Some(produce_candidate) = found {
            return Ok(Some((index, produce_candidate)));
        }
    }
    Ok(None)
}

/// Added by decision 123 (plan v4, V3-8): the COMM of a produce fire that
/// matched `data_candidates`, the body of the base `RSpace::produce_comm`.
/// `incoming` is the incoming produce of the fire and its persistence, when
/// the fire has one. `counters` gives the stored produce counts. A
/// non-persistent incoming produce that the COMM lists counts once more, so
/// the tuple test and the match build the COMM in one way.
pub(crate) fn produce_comm_with<C, A: Clone>(
    data_candidates: &[ConsumeCandidate<C, A>],
    consume_ref: &crate::rspace::trace::event::Consume,
    peeks: &std::collections::BTreeSet<i32>,
    incoming: Option<(&Produce, bool)>,
    counters: impl Fn(&[Produce]) -> std::collections::BTreeMap<Produce, i32>,
) -> COMM {
    let produce_counters_closure = |produces: &[Produce]| {
        let mut produce_counts = counters(produces);
        match incoming {
            Some((produce_ref, false)) if produces.contains(produce_ref) => {
                *produce_counts.entry(produce_ref.clone()).or_insert(0) += 1;
            }
            _ => {}
        }
        produce_counts
    };
    COMM::new(data_candidates, consume_ref.clone(), peeks.clone(), produce_counters_closure)
}

/// Added by decision 123 (plan v4, V3-8): the base store step `store_data`
/// over the store `store`. It stores the datum `data` of the produce
/// `produce_ref` on `channel`. Native play and Reified play store a datum
/// through this one function.
pub(super) fn store_data<C, P, A, K>(
    store: &dyn HotStore<C, P, A, K>,
    channel: C,
    data: A,
    persist: bool,
    produce_ref: Produce,
) where
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    store.put_datum(&channel, Datum {
        a: data,
        persist,
        source: produce_ref,
    });
}

/// Added by decision 123 (plan v4, V3-8): the base store step
/// `remove_matched_datum_and_join` over the store `store`. In descending
/// order of the stored index, it removes each matched non-persistent datum
/// and the join of the fired channels `channels` on the channel of each
/// candidate. A candidate whose removal fails keeps its join.
pub(super) fn remove_matched_datum_and_join<C, P, A, K>(
    store: &dyn HotStore<C, P, A, K>,
    channels: &[C],
    data_candidates: &[ConsumeCandidate<C, A>],
) where
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    let mut sorted_candidates: Vec<_> = data_candidates.iter().collect();
    sorted_candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.datum_index));
    for consume_candidate in sorted_candidates {
        let channel = &consume_candidate.channel;
        let datum_index = consume_candidate.datum_index;

        if datum_index >= 0 &&
            !consume_candidate.datum.persist &&
            store.remove_datum(channel, datum_index).is_err()
        {
            continue;
        }
        store.remove_join(channel, channels);
    }
}

// Added by decision 123 (plan v4, section 10.1): the parity of the shared
// produce search with the base search.
#[cfg(test)]
mod parity_tests;
