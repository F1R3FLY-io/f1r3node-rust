// Play mode of the Reified RSpaces substrate: `SpaceTxnHost` for `RSpace`.
//
// `apply_op` first runs the read-only commit checks of the operation
// (`ready_produce` / `ready_consume`: unchanged join groups, counter room,
// and a fire located in the live store with its COMM recomputed from the
// live counters), so a failed check has no effect. Then it makes the host
// calls of ops_produce.rs / ops_consume.rs in the same order. The store
// steps `store_data`, `remove_matched_datum_and_join`,
// `store_waiting_continuation` and `store_persistent_data` are private to
// those modules. `space_store_handle.rs` re-states them over the public
// `HotStore` API, statement for statement, for play and native replay.
//
// Changed by decision 123 (plan v4, V3-8, R6 to R10): this file calls the
// base steps of the host. Each operation runs in the accounting frame of the
// host (`RSpace::run_observed`). A fire commits through `commit_produce_match`
// or `commit_consume_match`. A stored datum and a parked continuation go
// through the free `store_data` and `store_waiting_continuation` of
// ops_produce.rs and ops_consume.rs. So native play and Reified play change
// the hot store through one code path, and `space_store_handle.rs` re-states
// no store step.
//
// Added by decision 124 (plan v4 C7.15, C7.18, C7.20, rules CR-17, CR-22 and
// F12): under owner funding a staged fire runs the base search of its row at
// the commit (`apply_row`). Each tuple but the staged one runs the guard once,
// then the theory, and a consume runs its guard once on its pick (F12). A row
// that finds no tuple stores its datum or parks its continuation. A store
// above the bound of its channel is the host outcome, and the readers accept a
// side above its bound (CR-22).

use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;

use super::RSpace;
// Added by decision 124 (L15, L16, plan v4 C7.15): the base admissions of the
// search of a row.
use super::accounting::{ConsumeAdmission, ProduceAdmission};
// Added by decision 123 (plan v4, V3-8, R9): the base store steps.
use super::ops_consume::store_waiting_continuation;
use super::ops_produce::{run_matcher, store_data};
use crate::rspace::errors::RSpaceError;
use crate::rspace::hot_store::HotStore;
use crate::rspace::internal::WaitingContinuation;
use crate::rspace::r#match::Match;
// Added by decision 124 (L15, L16, plan v4 C7.15): the item test and the tuple
// test of the search of a row.
use crate::rspace::operation_context::{ItemAdmission, MatchAdmission};
// Changed by decision 123 (plan v4, V3-8, R6, R8): the base commit steps log
// the COMMs with their labels.
// use crate::rspace::metrics_constants::{CONSUME_COMM_LABEL, PRODUCE_COMM_LABEL};
// Changed by decision 124 (L11, L16, plan v4 C7.15): the frame of `apply_row`
// takes the completion of its row. The text before:
// ```
// use crate::rspace::rspace_interface::{RSpaceAccountingObserver, RSpaceOperationSource};
// ```
use crate::rspace::rspace_interface::{
    RSpaceAccountingObserver, RSpaceOperationCompletion, RSpaceOperationSource,
};
// Changed by decision 94: the transaction covers the exact keys of phase B,
// so the host builds no stripe coverage.
// use crate::rspace::space_store_handle::{
//     BeforeObserve, BoxedSpaceTxn, HostTxn, LockCoverage, ReadyConsume, ReadyConsumeFire,
// ReadyFire,     ReadyProduce, SharedStore, SpaceMode, SpaceOpReceipt, SpaceTxnError,
// SpaceTxnHost,     StagedConsume, StagedConsumeFire, StagedFire, StagedOperation,
// StagedProduce, SweepReport,     TxnBackend, TxnRegistration, completion, fired_receipt,
// installed_channels, ready_consume,     ready_produce, remove_matched_datum_and_join,
// root_leaf_records, run_before_observe,     store_data, store_persistent_data,
// store_waiting_continuation, sweep_channels,     sweep_volatile_channels,
// };
// use crate::rspace::striped_locks;
// Changed by decision 123 (plan v4, V3-8, R9): the store steps are the base
// functions of ops_produce.rs and ops_consume.rs.
// Added by decision 124 (L15, L16, plan v4 C7.15): the base consume search of a
// row.
use crate::rspace::space_matcher::{MatchCandidates, MatchGroup, TupleRequest, consume_match};
use crate::rspace::space_store_handle::{
    AppliedRow, BeforeObserve, BoxedSpaceTxn, HostTxn, OpSource, PlayItemRule, ReadyConsume,
    ReadyConsumeFire, ReadyFire, ReadyProduce, RowCandidates, RowLists, RowParts, RowReads,
    RowSearch, RowSearchWithReads, RowTupleRequest, SharedStore, SpaceMode, SpaceOpReceipt,
    SpaceTxnError, SpaceTxnHost, StagedConsume, StagedConsumeFire, StagedFire, StagedOperation,
    StagedProduce, StoreEffect, SweepReport, TailRead, TxnBackend, TxnRegistration,
    actual_park_effects, fired_receipt, footprint_joins, installed_channels,
    ready_actual_consume_fire, ready_actual_produce_fire, ready_consume, ready_produce,
    receipt_completion, root_leaf_records, row_enumeration, run_before_observe, skipped_row,
    sweep_channels, sweep_volatile_channels,
};
use crate::rspace::striped_locks::{self, ChannelLockGuard};
use crate::rspace::trace::event::{COMM, Event, IOEvent, Produce};

struct PlayBackend<C, P, A, K> {
    space: RSpace<C, P, A, K>,
}

impl<C, P, A, K> TxnBackend<C, P, A, K> for PlayBackend<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    type Store = Box<dyn HotStore<C, P, A, K>>;

    fn store(&self) -> SharedStore<C, P, A, K> { self.space.get_store() }

    fn host_counter(&self, produce: &Produce) -> i32 {
        self.space
            .produce_counters(std::slice::from_ref(produce))
            .get(produce)
            .copied()
            .unwrap_or(0)
    }

    // Retired by decision 90 (C6, step D): the trait no longer asks for the
    // rigged log. The text before:
    // ```
    // fn recorded(&self, _event: &IOEvent) -> Vec<(COMM, usize)> { Vec::new() }
    //
    // fn recorded_binding(&self, _event: &IOEvent, _comm: &COMM) -> Option<(COMM, usize)> { None }
    // ```

    // Changed by decision 92: each operation records the footprint key of its
    // transaction.
    // fn apply_op(
    //     &self,
    //     store: &Self::Store,
    //     operation: StagedOperation<C, P, A, K>,
    //     before_observe: Option<BeforeObserve>,
    //     events: &mut Vec<Event>,
    // ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
    //     let store: &dyn HotStore<C, P, A, K> = &**store;
    //     match operation {
    //         StagedOperation::Produce(produce) => {
    //             self.apply_produce(store, produce, before_observe, events)
    //         }
    //         StagedOperation::Consume(consume) => {
    //             self.apply_consume(store, consume, before_observe, events)
    //         }
    //     }
    // }
    fn apply_op(
        &self,
        store: &Self::Store,
        operation: StagedOperation<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        footprint_key: Option<&C>,
        events: &mut Vec<Event>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
        self.apply_row(store, operation, before_observe, footprint_key, None, events)
            .map(|applied| applied.receipt)
    }

    // Added by decision 124 (L10, L15, L16, plan v4 C7.15, errata V4-4): a
    // staged fire with a row search runs the base search of the host when its
    // frame has owner funding. Every other row applies its staged operation.
    // An introduction that its owner cannot pay skips the row (R8), as the
    // base execution space maps it to no result.
    fn apply_row(
        &self,
        store: &Self::Store,
        operation: StagedOperation<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        footprint_key: Option<&C>,
        search: Option<RowSearchWithReads<'_, C, P, A, K>>,
        events: &mut Vec<Event>,
    ) -> Result<AppliedRow<C, P, A, K>, SpaceTxnError> {
        let store: &dyn HotStore<C, P, A, K> = &**store;
        let source = match &operation {
            StagedOperation::Produce(produce) => OpSource::Produce(produce.source.clone()),
            StagedOperation::Consume(consume) => OpSource::Consume(consume.source.clone()),
        };
        let applied = match operation {
            StagedOperation::Produce(produce) => {
                self.apply_produce(store, produce, before_observe, footprint_key, search, events)
            }
            StagedOperation::Consume(consume) => {
                self.apply_consume(store, consume, before_observe, footprint_key, search, events)
            }
        };
        match applied {
            Err(SpaceTxnError::Host(RSpaceError::IntroductionRefused)) => {
                Ok(skipped_row(source, false))
            }
            other => other,
        }
    }

    // Added by decision 124 (plan v4 C7.11, CR-31, ruling RADD 1.2): the tail
    // read of a deviating unit, over the lists of the search of play
    // (`produce_searched`) on the store after the retire group of the unit.
    fn tail_read(
        &self,
        store: &Self::Store,
        search: &mut dyn RowSearch<C, P, A, K>,
        reads: RowReads<'_>,
    ) -> Result<TailRead, SpaceTxnError> {
        let store: &dyn HotStore<C, P, A, K> = &**store;
        let read_data = reads.data(store);
        let read_continuations = reads.continuations(store);
        let read_joins = reads.joins(store);
        let counter = |produce: &Produce| self.host_counter(produce);
        let matcher: &dyn Match<P, A, K> = &**self.space.matcher;
        let lists = RowLists::new(&read_data, &read_continuations, &read_joins, None, &counter);
        search.tail_read(&lists, matcher)
    }

    // Added by decision 94: phase B as the host produce and consume take it.
    async fn lock_phase_b(&self, hashes: &[u64]) -> Result<ChannelLockGuard, SpaceTxnError> {
        Ok(striped_locks::acquire_locks(&self.space.phase_b_locks, hashes).await)
    }
}

impl<C, P, A, K> PlayBackend<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    // ops_produce.rs `locked_produce`: start (with the channel's join groups),
    // the observed body, then finish with the completion.
    // Changed by decision 92: the start also names the footprint key
    // `footprint_key` as one more join group.
    // fn apply_produce(
    //     &self,
    //     store: &dyn HotStore<C, P, A, K>,
    //     produce: StagedProduce<C, P, A, K>,
    //     before_observe: Option<BeforeObserve>,
    //     events: &mut Vec<Event>,
    // ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
    fn apply_produce(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        produce: StagedProduce<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        footprint_key: Option<&C>,
        search: Option<RowSearchWithReads<'_, C, P, A, K>>,
        events: &mut Vec<Event>,
    ) -> Result<AppliedRow<C, P, A, K>, SpaceTxnError> {
        let host_counter = self.host_counter(&produce.source);
        let (produce, joins) = ready_produce(store, produce, host_counter, |produces| {
            self.space.produce_counters(produces)
        })?;
        let produce_ref = produce.source.clone();
        let source = RSpaceOperationSource::Produce(&produce_ref);
        run_before_observe(before_observe, source)?;
        // Changed by decision 90 (DR-132): the start names the recorded row, and the
        // observed body and the finish run under that name, as `locked_produce`
        // of the host does. Without the name, a native charge has no operation
        // context. let observer = self.space.start_accounting_operation(
        //     source,
        //     std::slice::from_ref(&produce.channel),
        //     &joins,
        // )?;
        // let result = self.produce_observed(store, produce, observer.as_deref(),
        // events); if let Some(observer) = observer {
        //     observer.observe_operation_finish(source, completion(&result));
        // }
        // result
        // Changed by decision 92: the footprint names the footprint key.
        let joins = footprint_joins(&joins, footprint_key);
        // Changed by decision 123 (plan v4, V3-8, R10): the frame is the
        // shared `run_observed` of the host.
        let start = self.space.start_accounting_operation(
            source,
            std::slice::from_ref(&produce.channel),
            &joins,
        )?;
        // Added by decision 124 (L15, L16, plan v4 C7.15, errata V4-4): under
        // owner funding a staged fire with a row search runs the base search.
        RSpace::<C, P, A, K>::run_observed(source, start, applied_completion, move |observer| {
            let funded = observer.filter(|observer| observer.owner_checks_active());
            match (funded, search) {
                (Some(funded), Some(search)) if produce.fire.is_some() => {
                    self.produce_searched(store, produce, funded, search, events)
                }
                (_, _) => self
                    .produce_observed(store, produce, observer, events)
                    .map(AppliedRow::staged),
            }
        })
    }

    // ops_produce.rs `locked_produce_observed`: observe the introduction, then
    // log and store the datum, or fire.
    fn produce_observed(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        produce: ReadyProduce<C, P, A, K>,
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        events: &mut Vec<Event>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
        let ReadyProduce {
            source,
            channel,
            data,
            persist,
            fire,
        } = produce;
        RSpace::<C, P, A, K>::observe_produce(observer, &source, &channel, &data, persist)?;
        match fire {
            None => {
                self.space.log_produce(&source, persist);
                events.push(Event::IoEvent(IOEvent::Produce(source.clone())));
                store_data(store, channel, data, persist, source.clone());
                Ok(SpaceOpReceipt::Stored { produce: source })
            }
            Some(fire) => self.produce_fire(store, observer, events, source, persist, fire),
        }
    }

    // Added by decision 124 (L15, L16, plan v4 C7.15, errata V4-4, V4-9,
    // V4-17): the base produce search of a staged fire under owner funding, in
    // the steps of `locked_produce_observed`, over the store after the rows
    // before it. The staged tuple c0 commits as staged. Each other tuple runs
    // the guard once, then the theory (F12). It commits as the actual fire of
    // the row, and with no tuple the datum is stored. A store above the bound
    // of its channel is the host outcome, and the readers accept it (CR-22).
    // Both deviate. A stored row enumerates its pairs before the store
    // changes, and a Matched row books none (CR-20, F19).
    fn produce_searched(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        produce: ReadyProduce<C, P, A, K>,
        observer: &dyn RSpaceAccountingObserver<C, P, A, K>,
        search: RowSearchWithReads<'_, C, P, A, K>,
        events: &mut Vec<Event>,
    ) -> Result<AppliedRow<C, P, A, K>, SpaceTxnError> {
        let ReadyProduce {
            source,
            channel,
            data,
            persist,
            fire,
        } = produce;
        RSpace::<C, P, A, K>::observe_produce(Some(observer), &source, &channel, &data, persist)?;
        let Some(staged) = fire else {
            return Err(SpaceTxnError::Inconsistent("a searched produce row is not a fire"));
        };
        let (mut search, reads) = search;
        let read_data = reads.data(store);
        let read_continuations = reads.continuations(store);
        let read_joins = reads.joins(store);
        let counter = |produce: &Produce| self.host_counter(produce);
        let matcher: &dyn Match<P, A, K> = &**self.space.matcher;
        let admission = ProduceAdmission::new(&self.space, observer, &source, persist, &channel);
        let admit: &dyn MatchAdmission<C, P, A, K> = &admission;
        let (found, refused) = {
            let lists = RowLists::new(&read_data, &read_continuations, &read_joins, None, &counter);
            let mut candidates = RowCandidates::new(&mut *search, lists, None);
            let groups = candidates.groups();
            let found = run_matcher(matcher, &groups, &mut candidates, Some(admit))?;
            (found, candidates.into_refused())
        };
        if let Some(error) = admission.take_error() {
            return Err(SpaceTxnError::Host(error));
        }
        let found = match found {
            Some((_, found)) => {
                let comm = self.space.produce_comm(
                    &found.data_candidates,
                    &found.continuation.source,
                    &found.continuation.peeks,
                    &source,
                    persist,
                );
                match comm == staged.comm {
                    true => {
                        let receipt =
                            self.produce_fire(store, None, events, source, persist, staged)?;
                        return Ok(AppliedRow {
                            receipt,
                            parts: RowParts {
                                refused,
                                search: Some(search),
                                ..RowParts::staged()
                            },
                        });
                    }
                    false => Some(found),
                }
            }
            None => None,
        };
        let actual = found.as_ref().map(|found| {
            let request = RowTupleRequest {
                matcher,
                group: &found.channels,
                patterns: &found.continuation.patterns,
                continuation: &found.continuation.continuation,
                persistent: found.continuation.persist,
                source: &found.continuation.source,
                own: Some(&channel),
                item: None,
            };
            search.actual_fire(&request, &found.data_candidates)
        });
        // A Matched row books no item pair (F19). A stored row enumerates its
        // pairs before the store changes (CR-20, item 2). A head row lists its
        // head only (CR-2), so it deviates only to a store or a park.
        let (item_failed, vetoing) = match &found {
            Some(_) => (Vec::new(), false),
            None => {
                let lists =
                    RowLists::new(&read_data, &read_continuations, &read_joins, None, &counter);
                row_enumeration(&mut *search, &lists, matcher, &PlayItemRule(admit), None)?
            }
        };
        let (receipt, effects) = match found {
            Some(found) => {
                let incoming = match persist {
                    true => None,
                    false => Some(&source),
                };
                let fire = (found.channels, found.continuation, found.data_candidates);
                let (fire, effects) =
                    ready_actual_produce_fire(self, store, reads, incoming, fire)?;
                let receipt = self.produce_fire(store, None, events, source, persist, fire)?;
                (receipt, effects)
            }
            None => {
                // Added by decision 124 (ruling RADD2 1.2, J-STORE, rule CR-19
                // case 3, F22): the row writes nothing when its search keeps no
                // staged record (`store_unit` holds its stores), or when the
                // store holds the persistent record already, because a re-offer
                // leaves the record in place.
                let writes = match (search.keeps_staged_record(), persist) {
                    (false, _) => false,
                    (true, false) => true,
                    (true, true) => !read_data(&channel)?
                        .iter()
                        .any(|datum| datum.source == source),
                };
                self.space.log_produce(&source, persist);
                events.push(Event::IoEvent(IOEvent::Produce(source.clone())));
                let effects = match writes {
                    true => {
                        store_data(store, channel, data, persist, source.clone());
                        vec![StoreEffect::PutDatum(source.clone())]
                    }
                    false => Vec::new(),
                };
                let receipt = SpaceOpReceipt::Stored { produce: source };
                (receipt, effects)
            }
        };
        let parts = RowParts {
            effects: Some(effects),
            deviated: true,
            refused,
            item_failed,
            actual,
            can_stop: false,
            can_continue: true,
            vetoing,
            claims: search.actual_claims(),
            search: Some(search),
        };
        Ok(AppliedRow { receipt, parts })
    }

    // ops_produce.rs `process_match_found`: observe_comm with the COMM that
    // counts the produce being fired, log the produce and the COMM, remove
    // the continuation unless persistent, and retire the candidates, also
    // for a peek (decision 45, Rule E). The incoming datum is never stored;
    // the agent offers a non-persistent incoming datum of a peek fire again
    // as the next produce (FR-10, decision 30).
    fn produce_fire(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        events: &mut Vec<Event>,
        source: Produce,
        persist: bool,
        fire: ReadyFire<C, P, A, K>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
        let ReadyFire {
            fire:
                StagedFire {
                    channels,
                    continuation,
                    candidates,
                    ..
                },
            comm,
            continuation_index,
            committed,
        } = fire;

        RSpace::<C, P, A, K>::observe_comm(
            observer,
            &comm,
            &continuation.continuation,
            continuation.persist,
            &candidates,
        )?;
        // Changed by decision 123 (plan v4, V3-8, R8): the commit steps are
        // the shared `commit_produce_match` of the host. It takes the store of
        // the transaction, the located records `committed` and the event
        // sink, and it keeps the order of the old steps:
        // ```
        // self.space.log_produce(&source, persist);
        // events.push(Event::IoEvent(IOEvent::Produce(source.clone())));
        // self.space.log_comm(comm.clone(), PRODUCE_COMM_LABEL);
        // events.push(Event::Comm(comm.clone()));
        //
        // if let Some(index) = continuation_index {
        //     store.remove_continuation(&channels, index);
        // }
        // // Disabled by decision 45 (Rule E, "Apply everywhere"): a retaining
        // // (peek) fire only unjoined its candidates. Every fire now removes its
        // // stored non-persistent records, as the host does.
        // //
        // // ```
        // // match retain {
        // //     false => remove_matched_datum_and_join(store, &channels, &committed),
        // //     true => remove_joins(store, &channels, &committed),
        // // }
        // // ```
        // remove_matched_datum_and_join(store, &channels, &committed);
        // let wrapped = self
        //     .space
        //     .wrap_result(&channels, &continuation, &candidates);
        // ```
        let wrapped = self.space.commit_produce_match(
            store,
            &source,
            persist,
            comm.clone(),
            &channels,
            &continuation,
            continuation_index,
            &candidates,
            &committed,
            Some(events),
        );
        fired_receipt(comm, wrapped, candidates.len(), Some(source))
    }

    // ops_consume.rs `locked_consume`: start with the consume channels and no
    // joins, the observed body, then finish with the completion.
    // Changed by decision 92: the start names the footprint key
    // `footprint_key` as its one join group.
    // fn apply_consume(
    //     &self,
    //     store: &dyn HotStore<C, P, A, K>,
    //     consume: StagedConsume<C, P, A, K>,
    //     before_observe: Option<BeforeObserve>,
    //     events: &mut Vec<Event>,
    // ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
    fn apply_consume(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        consume: StagedConsume<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        footprint_key: Option<&C>,
        search: Option<RowSearchWithReads<'_, C, P, A, K>>,
        events: &mut Vec<Event>,
    ) -> Result<AppliedRow<C, P, A, K>, SpaceTxnError> {
        let consume =
            ready_consume(store, consume, |produces| self.space.produce_counters(produces))?;
        let consume_ref = consume.source.clone();
        let source = RSpaceOperationSource::Consume(&consume_ref);
        run_before_observe(before_observe, source)?;
        // Changed by decision 90 (DR-132): the start names the recorded row, and the
        // observed body and the finish run under that name, as `locked_consume`
        // of the host does. let observer = self
        //     .space
        //     .start_accounting_operation(source, &consume.channels, &[])?;
        // let result = self.consume_observed(store, consume, observer.as_deref(),
        // events); if let Some(observer) = observer {
        //     observer.observe_operation_finish(source, completion(&result));
        // }
        // result
        // Changed by decision 92: the footprint names the footprint key.
        let joins = footprint_joins(&[], footprint_key);
        // Changed by decision 123 (plan v4, V3-8, R10): the frame is the
        // shared `run_observed` of the host.
        let start = self
            .space
            .start_accounting_operation(source, &consume.channels, &joins)?;
        // Added by decision 124 (L15, L16, plan v4 C7.15, errata V4-4): under
        // owner funding a staged fire with a row search runs the base search.
        RSpace::<C, P, A, K>::run_observed(source, start, applied_completion, move |observer| {
            let funded = observer.filter(|observer| observer.owner_checks_active());
            match (funded, search) {
                (Some(funded), Some(search)) if consume.fire.is_some() => {
                    self.consume_searched(store, consume, funded, search, events)
                }
                (_, _) => self
                    .consume_observed(store, consume, observer, events)
                    .map(AppliedRow::staged),
            }
        })
    }

    // ops_consume.rs `locked_consume_observed`: observe the introduction, then
    // park, or fire: observe_comm, log the consume and the COMM, and remove
    // the stored non-persistent records unless retaining.
    fn consume_observed(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        consume: ReadyConsume<C, P, A, K>,
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        events: &mut Vec<Event>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
        let ReadyConsume {
            source,
            channels,
            group: _,
            patterns,
            continuation,
            persist,
            peeks,
            fire,
        } = consume;
        RSpace::<C, P, A, K>::observe_consume(
            observer,
            &source,
            &channels,
            &patterns,
            &continuation,
            persist,
            &peeks,
        )?;
        let wk = WaitingContinuation {
            patterns,
            continuation,
            persist,
            peeks,
            source: source.clone(),
        };
        match fire {
            None => {
                self.space.log_consume(&source);
                events.push(Event::IoEvent(IOEvent::Consume(source.clone())));
                store_waiting_continuation(store, channels, wk);
                Ok(SpaceOpReceipt::Parked { consume: source })
            }
            Some(ReadyConsumeFire {
                fire: StagedConsumeFire { candidates, .. },
                comm,
                committed,
            }) => {
                RSpace::<C, P, A, K>::observe_comm(
                    observer,
                    &comm,
                    &wk.continuation,
                    persist,
                    &candidates,
                )?;
                // Changed by decision 123 (plan v4, V3-8, R6): the commit steps
                // are the shared `commit_consume_match` of the host. It takes the
                // store of the transaction, the located records `committed` and
                // the event sink, and it keeps the order of the old steps:
                // ```
                // self.space.log_consume(&source);
                // events.push(Event::IoEvent(IOEvent::Consume(source.clone())));
                // self.space.log_comm(comm.clone(), CONSUME_COMM_LABEL);
                // events.push(Event::Comm(comm.clone()));
                // // Disabled by decision 45 (Rule E, "Apply everywhere"): a
                // // retaining (peek) fire kept its records. Every fire now removes
                // // its stored non-persistent records, as the host does.
                // //
                // // ```
                // // if !retain {
                // //     store_persistent_data(store, &committed);
                // // }
                // // ```
                // store_persistent_data(store, &committed);
                // ```
                self.space.commit_consume_match(
                    store,
                    &source,
                    comm.clone(),
                    &committed,
                    Some(events),
                );
                let wrapped = self.space.wrap_result(&channels, &wk, &candidates);
                fired_receipt(comm, wrapped, candidates.len(), None)
            }
        }
    }

    // Added by decision 124 (L15, L16, plan v4 C7.15, errata V4-4, V4-5,
    // V4-17): the base consume search of a staged fire under owner funding, in
    // the steps of `locked_consume_observed`, over the store after the rows
    // before it. The staged tuple c0 commits as staged. Each other tuple runs
    // the guard once on the pick, then the theory (F12), and it commits as the
    // actual fire of the row. A refused COMM or no tuple parks the
    // continuation, and a machinery row parks inert this way (CR-16). Each row
    // but c0 deviates. A parked row enumerates its pairs before the store
    // changes, and a Matched row books none (CR-20, F19).
    fn consume_searched(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        consume: ReadyConsume<C, P, A, K>,
        observer: &dyn RSpaceAccountingObserver<C, P, A, K>,
        search: RowSearchWithReads<'_, C, P, A, K>,
        events: &mut Vec<Event>,
    ) -> Result<AppliedRow<C, P, A, K>, SpaceTxnError> {
        let ReadyConsume {
            source,
            channels,
            group: _,
            patterns,
            continuation,
            persist,
            peeks,
            fire,
        } = consume;
        RSpace::<C, P, A, K>::observe_consume(
            Some(observer),
            &source,
            &channels,
            &patterns,
            &continuation,
            persist,
            &peeks,
        )?;
        let Some(staged) = fire else {
            return Err(SpaceTxnError::Inconsistent("a searched consume row is not a fire"));
        };
        let (mut search, reads) = search;
        let read_data = reads.data(store);
        let read_continuations = reads.continuations(store);
        let read_joins = reads.joins(store);
        let counter = |produce: &Produce| self.host_counter(produce);
        let matcher: &dyn Match<P, A, K> = &**self.space.matcher;
        let admission = ConsumeAdmission {
            observer,
            error: Default::default(),
        };
        let item = ItemAdmission {
            admission: &admission,
            continuation: &continuation,
            persistent: persist,
            channel_count: channels.len(),
        };
        let request = TupleRequest {
            matcher,
            group: &channels,
            patterns: &patterns,
            continuation: &continuation,
            persistent: persist,
            source: &source,
            item: Some(item),
            own: None,
        };
        let (fired, refused) = {
            let lists = RowLists::new(&read_data, &read_continuations, &read_joins, None, &counter);
            let mut candidates = RowCandidates::new(&mut *search, lists, None);
            let mut group = candidates.group(0, &channels)?;
            let found = consume_match(
                &mut group,
                request,
                || admission.error.take().map(SpaceTxnError::Host),
                || {},
            )?;
            let fired = match found {
                Some(tuple) => {
                    let comm = COMM::new(&tuple, source.clone(), peeks.clone(), |produces| {
                        self.space.produce_counters(produces)
                    });
                    let observed = RSpace::<C, P, A, K>::observe_comm(
                        Some(observer),
                        &comm,
                        &continuation,
                        persist,
                        &tuple,
                    );
                    match observed {
                        Ok(()) => Some((tuple, comm)),
                        Err(RSpaceError::CommRefused) => {
                            group.refused(&request, &tuple);
                            None
                        }
                        Err(error) => return Err(SpaceTxnError::Host(error)),
                    }
                }
                None => None,
            };
            (fired, candidates.into_refused())
        };
        let wk = WaitingContinuation {
            patterns,
            continuation,
            persist,
            peeks,
            source: source.clone(),
        };
        let fired = match fired {
            Some((_, comm)) if comm == staged.comm => {
                let ReadyConsumeFire {
                    fire: StagedConsumeFire { candidates, .. },
                    comm,
                    committed,
                } = staged;
                self.space.commit_consume_match(
                    store,
                    &source,
                    comm.clone(),
                    &committed,
                    Some(events),
                );
                let wrapped = self.space.wrap_result(&channels, &wk, &candidates);
                let receipt = fired_receipt(comm, wrapped, candidates.len(), None)?;
                return Ok(AppliedRow {
                    receipt,
                    parts: RowParts {
                        refused,
                        search: Some(search),
                        ..RowParts::staged()
                    },
                });
            }
            other => other,
        };
        let actual = fired.as_ref().map(|(tuple, _)| {
            let request = RowTupleRequest {
                matcher,
                group: &channels,
                patterns: &wk.patterns,
                continuation: &wk.continuation,
                persistent: persist,
                source: &source,
                own: None,
                item: None,
            };
            search.actual_fire(&request, tuple)
        });
        // A Matched row books no item pair (F19). A parked row enumerates its
        // pairs before the store changes (CR-20, item 2).
        let (item_failed, vetoing) = match &fired {
            Some(_) => (Vec::new(), false),
            None => {
                let lists =
                    RowLists::new(&read_data, &read_continuations, &read_joins, None, &counter);
                let rule = PlayItemRule(&admission);
                row_enumeration(&mut *search, &lists, matcher, &rule, Some(&wk.continuation))?
            }
        };
        let (receipt, effects) = match fired {
            Some((tuple, _)) => {
                let fire = (&source, &wk.peeks);
                let (fire, effects) =
                    ready_actual_consume_fire(self, store, reads, fire, &channels, tuple)?;
                let ReadyConsumeFire {
                    fire: StagedConsumeFire { candidates, .. },
                    comm,
                    committed,
                } = fire;
                self.space.commit_consume_match(
                    store,
                    &source,
                    comm.clone(),
                    &committed,
                    Some(events),
                );
                let wrapped = self.space.wrap_result(&channels, &wk, &candidates);
                (fired_receipt(comm, wrapped, candidates.len(), None)?, effects)
            }
            None => {
                let effects = actual_park_effects(store, reads, &channels, wk.clone())?;
                self.space.log_consume(&source);
                events.push(Event::IoEvent(IOEvent::Consume(source.clone())));
                store_waiting_continuation(store, channels, wk);
                (SpaceOpReceipt::Parked { consume: source }, effects)
            }
        };
        let parts = RowParts {
            effects: Some(effects),
            deviated: true,
            refused,
            item_failed,
            actual,
            can_stop: false,
            can_continue: true,
            vetoing,
            claims: search.actual_claims(),
            search: Some(search),
        };
        Ok(AppliedRow { receipt, parts })
    }
}

// Added by decision 124 (L11, L16, plan v4 C7.15): the completion of the
// frame of a row, from the receipt of the row.
fn applied_completion<C, P: Clone, A: Clone, K: Clone>(
    result: &Result<AppliedRow<C, P, A, K>, SpaceTxnError>,
) -> RSpaceOperationCompletion {
    match result {
        Ok(applied) => receipt_completion(&applied.receipt),
        Err(_) => RSpaceOperationCompletion::Rejected,
    }
}

#[async_trait]
impl<C, P, A, K> SpaceTxnHost<C, P, A, K> for RSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    fn mode(&self) -> SpaceMode { SpaceMode::Play }

    fn matcher(&self) -> Arc<Box<dyn Match<P, A, K>>> { Arc::clone(&self.matcher) }

    // Changed by decision 94: the host takes phase A at the begin, and the
    // transaction takes phase B ([`SpaceTxn::lock_phase_b`]). The default
    // `begin` takes both phases over the lock keys, as `consume_lock` does.
    // async fn begin(
    //     &self,
    //     lock_keys: &[C],
    //     registration: TxnRegistration,
    // ) -> Result<BoxedSpaceTxn<C, P, A, K>, SpaceTxnError> {
    //     if lock_keys.is_empty() {
    //         return Err(SpaceTxnError::InvalidOperation("lock keys must not be
    // empty"));     }
    //     let hashes: Vec<u64> =
    // lock_keys.iter().map(striped_locks::channel_hash).collect();
    //     let coverage =
    //         LockCoverage::new(&hashes, self.phase_a_locks.len(),
    // self.phase_b_locks.len());     let locks =
    // self.consume_lock(&hashes).await;     let store = self.get_store();
    //     let installed = installed_channels(&self.installs)?;
    //     let backend = PlayBackend {
    //         space: self.clone(),
    //     };
    //     Ok(Box::new(HostTxn::new(
    //         SpaceMode::Play,
    //         backend,
    //         store,
    //         installed,
    //         coverage,
    //         locks,
    //         registration,
    //     )))
    // }
    async fn begin_phase_a(
        &self,
        lock_keys: &[C],
        registration: TxnRegistration,
    ) -> Result<BoxedSpaceTxn<C, P, A, K>, SpaceTxnError> {
        if lock_keys.is_empty() {
            return Err(SpaceTxnError::InvalidOperation("lock keys must not be empty"));
        }
        let hashes: Vec<u64> = lock_keys.iter().map(striped_locks::channel_hash).collect();
        let phase_a = striped_locks::acquire_locks(&self.phase_a_locks, &hashes).await;
        let store = self.get_store();
        let installed = installed_channels(&self.installs)?;
        let backend = PlayBackend {
            space: self.clone(),
        };
        Ok(Box::new(HostTxn::new(
            SpaceMode::Play,
            backend,
            store,
            installed,
            lock_keys.to_vec(),
            phase_a,
            registration,
        )))
    }

    fn root_leaf_record_count(&self, channel: &C) -> Result<u64, SpaceTxnError> {
        let repository = self.get_history_repository();
        let reader = repository
            .get_history_reader(&repository.root())
            .map_err(RSpaceError::from)?;
        root_leaf_records(reader.as_ref(), channel)
    }

    async fn sweep_volatile(
        &self,
        channels: &[C],
        _registration: &TxnRegistration,
    ) -> Result<SweepReport, SpaceTxnError> {
        let channels = sweep_channels(channels)?;
        let hashes: Vec<u64> = channels.values().map(striped_locks::channel_hash).collect();
        let _guards = self.consume_lock(&hashes).await;
        let repository = self.get_history_repository();
        let reader = repository
            .get_history_reader(&repository.root())
            .map_err(RSpaceError::from)?;
        let installed = installed_channels(&self.installs)?;
        let store = self.get_store();
        sweep_volatile_channels(&**store, reader.as_ref(), &installed, &channels, || {})
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    // Added by decision 92.
    use std::task::Poll;

    use super::*;
    // Added by decision 92.
    use crate::rspace::internal::Datum;
    use crate::rspace::rspace_interface::ISpace;
    use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use crate::rspace::space_store_handle::{
        ConsumeOutcome, ProduceOutcome, SpaceConsume, SpaceProduce, SpaceStoreHandle, TxnView,
        bind_candidate, incoming_candidate,
    };

    struct AnyMatch;

    impl Match<String, String, String> for AnyMatch {
        fn get(&self, _pattern: &String, datum: &String) -> Option<String> { Some(datum.clone()) }
    }

    async fn space() -> RSpace<String, String, String, String> {
        let mut stores = InMemoryStoreManager::new();
        let store = stores
            .r_space_stores()
            .await
            .expect("create the in-memory RSpace stores");
        RSpace::create(store, Arc::new(Box::new(AnyMatch))).expect("create the RSpace")
    }

    fn s(value: &str) -> String { value.to_string() }

    fn store_op(channel: &str, data: &str) -> SpaceProduce<String, String, String, String> {
        SpaceProduce {
            channel: s(channel),
            data: s(data),
            persist: false,
            outcome: ProduceOutcome::Store,
            before_observe: None,
            own: false,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
        }
    }

    fn park_op(channel: &str, continuation: &str) -> SpaceConsume<String, String, String, String> {
        SpaceConsume {
            channels: vec![s(channel)],
            patterns: vec![s("_")],
            continuation: s(continuation),
            persist: false,
            peeks: BTreeSet::new(),
            outcome: ConsumeOutcome::Park,
            before_observe: None,
            own: false,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
        }
    }

    fn fire_op(
        channel: &str,
        data: &str,
        waiting: &WaitingContinuation<String, String>,
    ) -> SpaceProduce<String, String, String, String> {
        SpaceProduce {
            channel: s(channel),
            data: s(data),
            persist: false,
            outcome: ProduceOutcome::Fire {
                channels: vec![s(channel)],
                continuation: waiting.clone(),
                data: vec![incoming_candidate(
                    s(channel),
                    Produce::create(&s(channel), &s(data), false),
                    s(data),
                    false,
                    s(data),
                )],
            },
            before_observe: None,
            own: false,
            // Added by decision 123 (plan v4, V3-9): no row search.
            search: None,
        }
    }

    // Produce counters near i32::MAX, set with the host's own counter
    // helpers: staging refuses the increment that would overflow, the views
    // use checked arithmetic, and commit refuses an overflowing operation
    // before any of its effects.
    #[tokio::test]
    async fn space_store_handle_counter_overflow_fails_closed() {
        let space = space().await;
        let keys = vec![s("a"), s("b")];
        let x = Produce::create(&s("a"), &s("x"), false);
        let y = Produce::create(&s("b"), &s("y"), false);
        space.restore_produce_counter(BTreeMap::from([(x.clone(), i32::MAX - 1)]));
        let handle = SpaceStoreHandle::new(space.clone());

        // Staging: the last increment fits, the next one overflows.
        let mut txn = handle.begin(&keys).await.expect("begin");
        txn.produce(store_op("a", "x"))
            .expect("the last increment fits");
        assert_eq!(
            txn.produce_counters(std::slice::from_ref(&x)),
            Ok(BTreeMap::from([(x.clone(), i32::MAX)]))
        );
        assert_eq!(txn.produce(store_op("a", "x")).err(), Some(SpaceTxnError::CounterOverflow));
        assert_eq!(txn.data(&s("a")).expect("read a").len(), 1);
        txn.commit().await.expect("commit the increment that fits");
        assert_eq!(
            space.produce_counters(std::slice::from_ref(&x)),
            BTreeMap::from([(x.clone(), i32::MAX)])
        );

        // At i32::MAX a store and a produce fire of x overflow at staging;
        // a consume fire of the stored x does not increment and stages.
        let waiting = WaitingContinuation::create(
            &vec![s("a")],
            &vec![s("_")],
            &s("k"),
            false,
            BTreeSet::new(),
        );
        let mut txn = handle.begin(&keys).await.expect("begin");
        assert_eq!(txn.produce(store_op("a", "x")).err(), Some(SpaceTxnError::CounterOverflow));
        let stored = txn.data(&s("a")).expect("read a")[0].clone();
        let consumed = txn
            .consume(SpaceConsume {
                channels: vec![s("a")],
                patterns: vec![s("_")],
                continuation: s("k2"),
                persist: false,
                peeks: BTreeSet::new(),
                outcome: ConsumeOutcome::Fire {
                    data: vec![bind_candidate(s("a"), &stored, s("x"), 0)],
                },
                before_observe: None,
                own: false,
                // Added by decision 123 (plan v4, V3-9): no row search.
                search: None,
            })
            .expect("a consume fire does not increment the counter");
        assert_eq!(
            consumed
                .comm
                .and_then(|comm| comm.times_repeated.get(&x).copied()),
            Some(i32::MAX)
        );
        txn.consume(park_op("a", "k"))
            .expect("park k on the emptied channel");
        txn.commit()
            .await
            .expect("commit the consume fire and the park");
        let mut txn = handle.begin(&keys).await.expect("begin");
        assert_eq!(
            txn.produce(fire_op("a", "x", &waiting)).err(),
            Some(SpaceTxnError::CounterOverflow)
        );
        assert_eq!(txn.continuations(&[s("a")]).expect("read [a]").len(), 1);
        drop(txn);

        // The view adds the staged delta to the host counter with a check.
        let mut view: TxnView<String, String, String, String> = TxnView::default();
        view.count_delta.insert(y.clone(), 1);
        assert_eq!(view.staged_count(i32::MAX - 1, &y), Ok(i32::MAX));
        assert_eq!(view.staged_count(i32::MAX, &y), Err(SpaceTxnError::CounterOverflow));

        // Commit: a counter that reaches i32::MAX after staging refuses the
        // operation before any effect; the operation before it stays applied.
        // `take_event_log` also resets the host counters.
        assert!(!space.take_event_log().await.is_empty());
        let mut txn = handle.begin(&keys).await.expect("begin");
        txn.produce(store_op("a", "w")).expect("stage w");
        txn.produce(store_op("b", "y")).expect("stage y");
        space.restore_produce_counter(BTreeMap::from([(y.clone(), i32::MAX)]));
        assert_eq!(txn.commit().await.err(), Some(SpaceTxnError::CounterOverflow));
        assert_eq!(space.get_data(&s("a")).await.len(), 1);
        assert!(space.get_data(&s("b")).await.is_empty());
        assert_eq!(
            space.produce_counters(std::slice::from_ref(&y)),
            BTreeMap::from([(y.clone(), i32::MAX)])
        );
        assert_eq!(space.take_event_log().await, vec![Event::IoEvent(IOEvent::Produce(
            Produce::create(&s("a"), &s("w"), false)
        ))]);
    }

    // Added by decision 92.
    const NOT_OWN_FIRST: &str = "the first operation of a space transaction is not its job's own";

    // Added by decision 92.
    fn own(
        op: SpaceProduce<String, String, String, String>,
    ) -> SpaceProduce<String, String, String, String> {
        SpaceProduce { own: true, ..op }
    }

    // Added by decision 92.
    fn data_of(data: Vec<Datum<String>>) -> Vec<String> {
        data.into_iter().map(|datum| datum.a).collect()
    }

    // Added by decision 92: with a footprint key, the commit requires that the
    // first staged operation is its job's own (`SpaceReplayOrder.tla`, the
    // control `AnchorAppendedUnsafe`). A shared first operation and an empty
    // commit fail with `InvalidOperation` and change nothing. An own first
    // operation commits, and a later operation can be shared.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_store_handle_footprint_key_requires_an_own_first_operation() {
        let space = space().await;
        let handle = SpaceStoreHandle::new(space.clone());
        let keys = vec![s("a"), s("s")];

        let mut txn = handle.begin(&keys).await.expect("begin");
        txn.set_footprint_key(s("s")).expect("s is an own lock key");
        txn.produce(store_op("a", "x"))
            .expect("stage a shared store");
        assert_eq!(txn.commit().await.err(), Some(SpaceTxnError::InvalidOperation(NOT_OWN_FIRST)));
        drop(txn);
        assert!(space.get_data(&s("a")).await.is_empty());

        let mut txn = handle.begin(&keys).await.expect("begin");
        txn.set_footprint_key(s("s")).expect("s is an own lock key");
        assert_eq!(txn.commit().await.err(), Some(SpaceTxnError::InvalidOperation(NOT_OWN_FIRST)));
        drop(txn);
        assert!(space.take_event_log().await.is_empty());

        let mut txn = handle.begin(&keys).await.expect("begin");
        txn.set_footprint_key(s("s")).expect("s is an own lock key");
        txn.produce(own(store_op("a", "y")))
            .expect("stage an own store");
        txn.produce(store_op("a", "z"))
            .expect("stage a shared store");
        txn.commit().await.expect("an own first operation commits");
        assert_eq!(data_of(space.get_data(&s("a")).await).len(), 2);

        // Without a footprint key, the guard does not apply.
        let mut txn = handle.begin(&keys).await.expect("begin");
        txn.produce(store_op("s", "w"))
            .expect("stage a shared store");
        txn.commit()
            .await
            .expect("a transaction without a footprint key commits");
        assert_eq!(data_of(space.get_data(&s("s")).await), vec![s("w")]);
    }

    // Added by decision 92: the footprint key is one of the own lock keys, and it
    // is set once. A scope key of phase B is not an own lock key.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_store_handle_footprint_key_is_one_own_lock_key() {
        let space = space().await;
        let handle = SpaceStoreHandle::new(space.clone());
        let mut txn = handle
            .begin_phase_a(&[s("a"), s("s")])
            .await
            .expect("begin phase A");
        txn.lock_phase_b(&[s("b")]).await.expect("phase B");
        for key in [s("b"), s("c")] {
            assert_eq!(
                txn.set_footprint_key(key.clone()),
                Err(SpaceTxnError::InvalidOperation("the footprint key is not an own lock key")),
                "key {key}"
            );
        }
        txn.set_footprint_key(s("s")).expect("s is an own lock key");
        assert_eq!(
            txn.set_footprint_key(s("s")),
            Err(SpaceTxnError::InvalidOperation("the footprint key is already set"))
        );
    }

    // Added by decision 92: a discard removes the staged operations and their
    // effects. The transaction keeps its stripes of both phases, its coverage
    // and its footprint key, so it can stage an anchor and commit it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_store_handle_discard_keeps_the_locks_and_the_key() {
        let space = space().await;
        let handle = SpaceStoreHandle::new(space.clone());
        let keys = vec![s("a"), s("s")];
        let hashes: Vec<u64> = keys.iter().map(striped_locks::channel_hash).collect();
        let mut txn = handle.begin(&keys).await.expect("begin");
        txn.set_footprint_key(s("s")).expect("s is an own lock key");
        txn.produce(store_op("a", "x"))
            .expect("stage a shared store");
        assert_eq!(txn.data(&s("a")).expect("read a").len(), 1);

        txn.discard_staged().expect("discard");
        assert!(txn.data(&s("a")).expect("read a").is_empty());
        for stripes in [&space.phase_a_locks, &space.phase_b_locks] {
            let mut acquire = Box::pin(striped_locks::acquire_locks(stripes, &hashes));
            assert!(
                matches!(futures::poll!(&mut acquire), Poll::Pending),
                "the transaction keeps its stripes"
            );
        }
        assert_eq!(
            txn.set_footprint_key(s("s")),
            Err(SpaceTxnError::InvalidOperation("the footprint key is already set"))
        );
        txn.produce(own(store_op("a", "y")))
            .expect("the coverage stays");
        txn.commit().await.expect("commit the own operation");
        assert_eq!(data_of(space.get_data(&s("a")).await), vec![s("y")]);
        assert_eq!(space.take_event_log().await, vec![Event::IoEvent(IOEvent::Produce(
            Produce::create(&s("a"), &s("y"), false)
        ))]);
    }

    // Added by decision 94: phase A covers no channel, so staging fails with
    // `UnlockedChannel` until phase B, and reads succeed (the classification of
    // the scope). Phase B covers the own keys and the scope keys exactly, and a
    // second phase B fails with `InvalidOperation`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_store_handle_phase_b_covers_the_own_and_scope_keys_once() {
        let space = space().await;
        let handle = SpaceStoreHandle::new(space.clone());
        let mut txn = handle
            .begin_phase_a(&[s("a")])
            .await
            .expect("begin phase A");
        assert!(txn.data(&s("b")).expect("a read under phase A").is_empty());
        assert!(txn.joins(&s("a")).expect("a read under phase A").is_empty());
        assert_eq!(txn.produce(store_op("a", "x")).err(), Some(SpaceTxnError::UnlockedChannel));
        txn.lock_phase_b(&[s("b")]).await.expect("phase B");
        assert_eq!(
            txn.lock_phase_b(&[]).await,
            Err(SpaceTxnError::InvalidOperation("the transaction already holds phase B"))
        );
        txn.produce(store_op("a", "x"))
            .expect("phase B covers the own key");
        txn.produce(store_op("b", "y"))
            .expect("phase B covers the scope key");
        assert_eq!(txn.produce(store_op("c", "z")).err(), Some(SpaceTxnError::UnlockedChannel));
        txn.commit().await.expect("commit");
        assert_eq!(data_of(space.get_data(&s("a")).await), vec![s("x")]);
        assert_eq!(data_of(space.get_data(&s("b")).await), vec![s("y")]);
        assert!(space.get_data(&s("c")).await.is_empty());
    }
}
