// Consume-side operation internals: the locked consume path and its
// store helpers.

use std::collections::{BTreeSet, HashMap};
use std::fmt::Debug;
use std::hash::Hash;
use std::time::Instant;

use serde::Serialize;

use super::RSpace;
// Added by G3 (DR-135, S5): the item test of an owner-funded consume.
use super::accounting::ConsumeAdmission;
use crate::rspace::errors::RSpaceError;
// Added by decision 123 (plan v4, V3-8): the free store steps.
use crate::rspace::hot_store::HotStore;
use crate::rspace::internal::*;
use crate::rspace::metrics_constants::{
    CONSUME_COMM_LABEL, LOCKED_CONSUME_SPAN, RSPACE_METRICS_SOURCE,
};
// Added by G3 (DR-135, S5): the item test of an owner-funded consume.
use crate::rspace::operation_context::ItemAdmission;
use crate::rspace::rspace_interface::{
    ContResult, MaybeConsumeResult, RSpaceAccountingObserver, RSpaceOperationCompletion,
    RSpaceOperationSource, RSpaceResult,
};
// Changed by decision 123 (plan v4, V3-4, R6): the shared consume search
// replaces the direct extraction.
// use crate::rspace::space_matcher::SpaceMatcher;
use crate::rspace::space_matcher::{StoreGroup, TupleRequest, consume_match};
// Changed by decision 123 (plan v4, R6): the event sink of the commit steps.
// use crate::rspace::trace::event::{COMM, Consume, Produce};
use crate::rspace::trace::event::{COMM, Consume, Event, IOEvent, Produce};

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

        let source = RSpaceOperationSource::Consume(consume_ref);
        // Changed by DR-132: the observer names the recorded row, and the rest
        // of the locked consume runs under that name.
        // let observer = self.start_accounting_operation(source, channels, &[])?;
        // Changed by decision 123 (plan v4, V3-8, R10): the frame after the
        // start is the shared `run_observed`.
        // let (observer, name) =
        //     self.start_accounting_operation(source, channels, &[])?;
        // crate::rspace::operation_context::named(name, move || {
        //     let result = self.locked_consume_observed(
        //         channels,
        //         patterns,
        //         continuation,
        //         persist,
        //         peeks,
        //         consume_ref,
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
        let start = self.start_accounting_operation(source, channels, &[])?;
        Self::run_observed(source, start, RSpaceOperationCompletion::from_result, move |observer| {
            self.locked_consume_observed(
                channels,
                patterns,
                continuation,
                persist,
                peeks,
                consume_ref,
                observer,
            )
        })
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
        // Changed by decision 123 (plan v4, V3-4): the lists move into the
        // group of the shared search.
        // let mut channel_to_indexed_data = self.fetch_channel_to_index_data(channels);
        let channel_to_indexed_data = self.fetch_channel_to_index_data(channels);
        metrics::counter!("rspace.consume.fetch_data_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(t1.elapsed().as_nanos() as u64);

        let t2 = Instant::now();
        // Changed by decision 123 (plan v4, V3-4, R6): the search is the
        // shared `consume_match` over the fetched lists, which Reified play
        // also runs. The base order holds: the tuple, the kept error, the
        // metric and then the guard. The old search:
        // ```
        // let zipped: Vec<(C, P)> = channels
        //     .iter()
        //     .cloned()
        //     .zip(patterns.iter().cloned())
        //     .collect();
        // // Changed by G3 (DR-135, S5): with owner funding each item passes its
        // // item test (R5). Without it the search is that of dev.
        // // let options: Option<Vec<ConsumeCandidate<C, A>>> = self
        // //     .extract_data_candidates(
        // //         &self.matcher, &zipped, &mut channel_to_indexed_data,
        // //     )
        // //     .into_iter()
        // //     .collect();
        // let candidates = match observer.filter(|observer| observer.owner_checks_active()) {
        //     Some(observer) => {
        //         let admission = ConsumeAdmission {
        //             observer,
        //             error: Default::default(),
        //         };
        //         let item = ItemAdmission {
        //             admission: &admission,
        //             continuation,
        //             persistent: persist,
        //             channel_count: channels.len(),
        //         };
        //         let mut rollback = Vec::new();
        //         let candidates = self.extract_data_candidates_rollback(
        //             &self.matcher,
        //             &zipped,
        //             &mut channel_to_indexed_data,
        //             &mut rollback,
        //             Some(item),
        //         );
        //         if let Some(error) = admission.error.into_inner() {
        //             return Err(error);
        //         }
        //         candidates
        //     }
        //     None => {
        //         self.extract_data_candidates(&self.matcher, &zipped, &mut channel_to_indexed_data)
        //     }
        // };
        // let options: Option<Vec<ConsumeCandidate<C, A>>> = candidates.into_iter().collect();
        // metrics::counter!("rspace.consume.match_ns", "source" => RSPACE_METRICS_SOURCE)
        //     .increment(t2.elapsed().as_nanos() as u64);
        //
        // let wk = WaitingContinuation {
        //     patterns: patterns.to_vec(),
        //     continuation: continuation.clone(),
        //     persist,
        //     peeks: peeks.clone(),
        //     source: consume_ref.clone(),
        // };
        //
        // let commit_ok = match &options {
        //     // Cross-channel commit hook: a `where` guard on the consume
        //     // can veto even after every spatial bind matched. On false
        //     // we install the wk and leave the data alone, just as if
        //     // the spatial match itself had failed (plan §7.12).
        //     Some(data_candidates) => {
        //         let matched: Vec<A> = data_candidates.iter().map(|c| c.datum.a.clone()).collect();
        //         self.matcher.check_commit(continuation, &matched)
        //     }
        //     None => false,
        // };
        // ```
        let admission = observer
            .filter(|observer| observer.owner_checks_active())
            .map(|observer| ConsumeAdmission {
                observer,
                error: Default::default(),
            });
        let item = admission.as_ref().map(|admission| ItemAdmission {
            admission,
            continuation,
            persistent: persist,
            channel_count: channels.len(),
        });
        let mut group = StoreGroup::<_, C, P, A, K, RSpaceError>::fetched_consume(
            self,
            &self.matcher,
            channel_to_indexed_data,
            channels.len(),
        );
        let request = TupleRequest {
            matcher: &**self.matcher,
            group: channels,
            patterns,
            continuation,
            persistent: persist,
            source: consume_ref,
            item,
            own: None,
        };
        // `take` replaces the base `into_inner`, because the item of the
        // request still borrows the admission.
        let options = consume_match(
            &mut group,
            request,
            || {
                admission
                    .as_ref()
                    .and_then(|admission| admission.error.take())
            },
            || {
                metrics::counter!("rspace.consume.match_ns", "source" => RSPACE_METRICS_SOURCE)
                    .increment(t2.elapsed().as_nanos() as u64);
            },
        )?;

        let wk = WaitingContinuation {
            patterns: patterns.to_vec(),
            continuation: continuation.clone(),
            persist,
            peeks: peeks.clone(),
            source: consume_ref.clone(),
        };

        match options {
            // Changed by decision 123 (plan v4, V3-4): `consume_match` runs the
            // guard, and a veto gives no tuple.
            // Some(data_candidates) if commit_ok => {
            Some(data_candidates) => {
                let t3 = Instant::now();
                let produce_counters_closure =
                    |produces: &[Produce]| self.produce_counters(produces);

                let comm = COMM::new(
                    &data_candidates,
                    consume_ref.clone(),
                    peeks.clone(),
                    produce_counters_closure,
                );
                // Changed by G3 (DR-135, S5): an owner that cannot pay refuses
                // the COMM. The consume then waits, as on a miss (R5).
                // Self::observe_comm(
                //     observer, &comm, continuation, persist, &data_candidates,
                // )?;
                match Self::observe_comm(observer, &comm, continuation, persist, &data_candidates) {
                    Ok(()) => {}
                    Err(RSpaceError::CommRefused) => {
                        self.log_consume(consume_ref);
                        self.store_waiting_continuation(channels.to_vec(), wk);
                        return Ok(None);
                    }
                    Err(error) => return Err(error),
                }
                // Changed by decision 123 (plan v4, R6): the commit steps are
                // the shared `commit_consume_match`, which Reified play also
                // calls.
                // self.log_consume(consume_ref);
                // self.log_comm(comm, CONSUME_COMM_LABEL);
                // self.store_persistent_data(&data_candidates);
                let store = self.get_store();
                self.commit_consume_match(&**store, consume_ref, comm, &data_candidates, None);
                metrics::counter!("rspace.consume.process_match_ns", "source" => RSPACE_METRICS_SOURCE)
                    .increment(t3.elapsed().as_nanos() as u64);
                tracing::trace!(target: "f1r3fly.rspace.ops", mark = "finished-locked-consume", "locked_consume");
                Ok(self.wrap_result(channels, &wk, &data_candidates))
            }
            _ => {
                let t3 = Instant::now();
                self.log_consume(consume_ref);
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
        // Changed by decision 123 (plan v4, V3-8): the store step is the free
        // `store_waiting_continuation` over the store, which Reified play also
        // calls. The method forwards and drops the inserted flag, as before.
        // let store = self.get_store();
        // let _ = store.put_continuation(&channels, wc);
        // for channel in channels.iter() {
        //     store.put_join(channel, &channels);
        // }
        let store = self.get_store();
        let _ = store_waiting_continuation(&**store, channels, wc);
        None
    }

    // Retired by decision 123 (plan v4, R6): `commit_consume_match` calls the
    // free `store_persistent_data` with the store of the operation, so this
    // method has no caller. The retired method:
    // ```
    // fn store_persistent_data(&self, data_candidates: &[ConsumeCandidate<C, A>]) {
    //     let mut sorted_candidates: Vec<_> = data_candidates.iter().collect();
    //     sorted_candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.datum_index));
    //     let store = self.get_store();
    //     for consume_candidate in sorted_candidates {
    //         if !consume_candidate.datum.persist {
    //             let _ =
    //                 store.remove_datum(&consume_candidate.channel, consume_candidate.datum_index);
    //         }
    //     }
    // }
    // ```

    /// Added by decision 123 (plan v4, R6): the commit steps of a consume
    /// match, the tail of the base consume fire. It logs the consume and the
    /// COMM, and it removes the matched non-persistent records of `removal`
    /// from `store`. An event sink also gets the two log events, in order.
    /// The caller wraps the result, so the base metric keeps its point.
    pub(super) fn commit_consume_match(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        consume_ref: &Consume,
        comm: COMM,
        removal: &[ConsumeCandidate<C, A>],
        events: Option<&mut Vec<Event>>,
    ) {
        self.log_consume(consume_ref);
        match events {
            Some(events) => {
                events.push(Event::IoEvent(IOEvent::Consume(consume_ref.clone())));
                self.log_comm(comm.clone(), CONSUME_COMM_LABEL);
                events.push(Event::Comm(comm));
            }
            None => self.log_comm(comm, CONSUME_COMM_LABEL),
        }
        store_persistent_data(store, removal);
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

/// Added by decision 123 (plan v4, V3-8): the base store step
/// `store_waiting_continuation` over the store `store`. It stores the waiting
/// continuation `wc` on the channels `channels` and the join of `channels` on
/// each of them. It returns whether the store took the continuation, which the
/// waiting-continuation estimate of native replay counts. Native play and
/// Reified play park a continuation through this one function.
pub(super) fn store_waiting_continuation<C, P, A, K>(
    store: &dyn HotStore<C, P, A, K>,
    channels: Vec<C>,
    wc: WaitingContinuation<P, K>,
) -> bool
where
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    let inserted = store.put_continuation(&channels, wc).unwrap_or(false);
    for channel in channels.iter() {
        store.put_join(channel, &channels);
    }
    inserted
}

/// Added by decision 123 (plan v4, V3-8): the base store step
/// `store_persistent_data` over the store `store`. In descending order of the
/// stored index, it removes each matched non-persistent datum of a consume
/// fire.
pub(super) fn store_persistent_data<C, P, A, K>(
    store: &dyn HotStore<C, P, A, K>,
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
        if !consume_candidate.datum.persist {
            let _ = store.remove_datum(&consume_candidate.channel, consume_candidate.datum_index);
        }
    }
}
