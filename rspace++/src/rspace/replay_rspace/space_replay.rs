// Replay and reporting modes of the Reified RSpaces substrate: `SpaceTxnHost`
// for `ReplayRSpace`, and for `ReportingRspace`, which delegates to its inner
// replay space with mode `Reporting` (its logger feeds the report).
//
// `apply_op` first runs the read-only commit checks of the operation
// (`ready_produce` / `ready_consume`), then makes the host calls of
// `locked_produce_observed` / `locked_consume_observed` in the same order and
// reuses the private replay helpers as written. `matches`,
// `was_repeated_enough_times` and `get_comm_or_candidate` read the host
// counter without the staged changes and iterate bindings in HashMap order,
// so their staged-view forms are `TxnCore::verify`, `SpaceTxn::admits` and
// `SpaceTxn::recorded_comms` in space_store_handle.rs.
//
// A fire that staging left unpaired (the log holds no COMM for it) makes
// the observer calls of play through `observe_comm`. A call that the
// observer rejects fails it with the error of play. Otherwise it fails with
// `ReplayDivergence` before it logs, stores or unbinds anything (decision
// 36).

use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use serde::Serialize;

use super::ReplayRSpace;
use crate::rspace::errors::RSpaceError;
use crate::rspace::hot_store::HotStore;
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::r#match::Match;
use crate::rspace::reporting_rspace::ReportingRspace;
use crate::rspace::rspace_interface::{RSpaceAccountingObserver, RSpaceOperationSource};
use crate::rspace::space_store_handle::{
    BeforeObserve, BoxedSpaceTxn, HostTxn, LockCoverage, ReadyConsume, ReadyConsumeFire, ReadyFire,
    ReadyProduce, SharedStore, SpaceMode, SpaceOpReceipt, SpaceTxnError, SpaceTxnHost,
    StagedConsume, StagedConsumeFire, StagedFire, StagedOperation, StagedProduce, SweepReport,
    TxnBackend, TxnRegistration, completion, fired_receipt, installed_channels, ready_consume,
    ready_produce, root_leaf_records, run_before_observe, sweep_channels, sweep_volatile_channels,
};
use crate::rspace::striped_locks;
use crate::rspace::trace::event::{COMM, Event, IOEvent, Produce};

struct ReplayBackend<C, P, A, K> {
    space: ReplayRSpace<C, P, A, K>,
}

impl<C, P, A, K> TxnBackend<C, P, A, K> for ReplayBackend<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    fn store(&self) -> SharedStore<C, P, A, K> { self.space.get_store() }

    fn host_counter(&self, produce: &Produce) -> i32 { self.space.get_produce_count(produce) }

    fn recorded(&self, event: &IOEvent) -> Vec<(COMM, usize)> {
        let replay_data = self.space.replay_data.lock().expect("replay data lock");
        let mut bindings: Vec<(COMM, usize)> = match replay_data.map.get(event) {
            Some(bag) => bag
                .iter()
                .map(|(comm, count)| (comm.clone(), *count))
                .collect(),
            None => Vec::new(),
        };
        bindings.sort_by_cached_key(|(comm, _)| bincode::serialize(comm).unwrap_or_default());
        bindings
    }

    fn recorded_binding(&self, event: &IOEvent, comm: &COMM) -> Option<(COMM, usize)> {
        let replay_data = self.space.replay_data.lock().expect("replay data lock");
        let binding = replay_data.map.get(event).and_then(|bag| {
            bag.get_key_value(comm)
                .map(|(recorded, count)| (recorded.clone(), *count))
        });
        drop(replay_data);
        binding
    }

    fn apply_op(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        operation: StagedOperation<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        events: &mut Vec<Event>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
        match operation {
            StagedOperation::Produce(produce) => {
                self.apply_produce(store, produce, before_observe, events)
            }
            StagedOperation::Consume(consume) => {
                self.apply_consume(store, consume, before_observe, events)
            }
        }
    }
}

// Spaces fail closed on a native replay directive (replay_rspace.rs
// `locked_*_observed` would hand the operation to the native session).
fn reject_directive<C, P, A, K>(
    observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
    source: RSpaceOperationSource<'_>,
) -> Result<(), SpaceTxnError> {
    match observer
        .map(|observer| observer.replay_operation_directive(source))
        .transpose()?
        .flatten()
    {
        Some(_) => Err(SpaceTxnError::NativeReplayDirectiveUnsupported),
        None => Ok(()),
    }
}

impl<C, P, A, K> ReplayBackend<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    // replay_rspace.rs `locked_produce`: start (with the channel's join
    // groups), the observed body, then finish with the completion.
    fn apply_produce(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        produce: StagedProduce<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        events: &mut Vec<Event>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
        let host_counter = self.host_counter(&produce.source);
        let (produce, joins) = ready_produce(store, produce, host_counter, |produces| {
            self.space.produce_counters(produces)
        })?;
        let produce_ref = produce.source.clone();
        let source = RSpaceOperationSource::Produce(&produce_ref);
        run_before_observe(before_observe, source)?;
        let observer = self.space.start_accounting_operation(
            source,
            std::slice::from_ref(&produce.channel),
            &joins,
        )?;
        let result = self.produce_observed(store, produce, source, observer.as_deref(), events);
        if let Some(observer) = observer {
            observer.observe_operation_finish(source, completion(&result));
        }
        result
    }

    // replay_rspace.rs `locked_produce_observed`: refuse a directive, observe
    // the introduction, increment the counter, then store or fire. A failed
    // fire rolls the counter back.
    fn produce_observed(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        produce: ReadyProduce<C, P, A, K>,
        source: RSpaceOperationSource<'_>,
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        events: &mut Vec<Event>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
        reject_directive(observer, source)?;
        let ReadyProduce {
            source: produce_ref,
            channel,
            data,
            persist,
            fire,
        } = produce;
        ReplayRSpace::<C, P, A, K>::observe_produce(
            observer,
            &produce_ref,
            &channel,
            &data,
            persist,
        )?;
        self.space.increment_produce_counter(&produce_ref, persist);
        match fire {
            None => {
                self.space
                    .log_produce(produce_ref.clone(), &channel, &data, persist);
                events.push(Event::IoEvent(IOEvent::Produce(produce_ref.clone())));
                let _ = self
                    .space
                    .store_data(channel, data, persist, produce_ref.clone());
                Ok(SpaceOpReceipt::Stored {
                    produce: produce_ref,
                })
            }
            Some(fire) => {
                let incoming = Datum {
                    a: data,
                    persist,
                    source: produce_ref.clone(),
                };
                let result = self.produce_fire(store, observer, events, channel, incoming, fire);
                if result.is_err() {
                    self.space.rollback_produce_counter(&produce_ref, persist);
                }
                result
            }
        }
    }

    // replay_rspace.rs `handle_match`: the counter was incremented before
    // matching, then observe_comm. An unpaired fire stops there with
    // `ReplayDivergence`, and `produce_observed` rolls the counter back. A
    // paired fire logs the produce and the COMM, removes the continuation
    // unless persistent, retires the candidates (also for a peek, decision
    // 45, Rule E), and removes the COMM's bindings. The incoming datum is
    // never stored; the agent offers a non-persistent incoming datum of a
    // peek fire again as the next produce (FR-10, decision 30). The receipt
    // carries the produce as recorded in the paired COMM.
    fn produce_fire(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        events: &mut Vec<Event>,
        channel: C,
        incoming: Datum<A>,
        fire: ReadyFire<C, P, A, K>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
        let ReadyFire {
            fire:
                StagedFire {
                    channels,
                    continuation,
                    candidates,
                    recorded,
                    ..
                },
            comm,
            continuation_index,
            committed,
        } = fire;
        let Datum {
            a: data,
            persist,
            source,
        } = incoming;

        ReplayRSpace::<C, P, A, K>::observe_comm(
            observer,
            &comm,
            &continuation.continuation,
            continuation.persist,
            &candidates,
        )?;
        let Some(recorded) = recorded else {
            return Err(SpaceTxnError::ReplayDivergence(Box::new(comm)));
        };
        self.space
            .log_produce(source.clone(), &channel, &data, persist);
        events.push(Event::IoEvent(IOEvent::Produce(source.clone())));
        self.space.log_comm(
            &candidates,
            &channels,
            continuation.clone(),
            comm.clone(),
            "comm.produce",
        );
        events.push(Event::Comm(comm.clone()));

        match continuation_index {
            Some(index) => {
                store.remove_continuation(&channels, index);
                self.space.mark_replay_waiting_continuation_match();
            }
            None => self.space.mark_replay_waiting_continuation_match(),
        }
        // Disabled by decision 45 (Rule E, "Apply everywhere"): a retaining
        // (peek) fire only unjoined its candidates. Every fire now removes its
        // stored non-persistent records, as the host does.
        //
        // ```
        // match retain {
        //     false => self
        //         .space
        //         .remove_matched_datum_and_join(&channels, &committed),
        //     true => {
        //         for candidate in by_descending_index(&committed) {
        //             store.remove_join(&candidate.channel, &channels);
        //         }
        //     }
        // }
        // ```
        self.space
            .remove_matched_datum_and_join(&channels, &committed);
        self.space.remove_bindings_for(comm.clone());

        let produce = recorded
            .produces
            .into_iter()
            .find(|produce| produce.hash == source.hash)
            .unwrap_or(source);
        let consume_ref = continuation.source.clone();
        let fired = candidates.len();
        let wrapped = self
            .space
            .wrap_result(channels, continuation, consume_ref, candidates);
        fired_receipt(comm, wrapped, fired, Some(produce))
    }

    // replay_rspace.rs `locked_consume`: start with the consume channels and
    // no joins, the observed body, then finish with the completion.
    fn apply_consume(
        &self,
        store: &dyn HotStore<C, P, A, K>,
        consume: StagedConsume<C, P, A, K>,
        before_observe: Option<BeforeObserve>,
        events: &mut Vec<Event>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
        let consume =
            ready_consume(store, consume, |produces| self.space.produce_counters(produces))?;
        let consume_ref = consume.source.clone();
        let source = RSpaceOperationSource::Consume(&consume_ref);
        run_before_observe(before_observe, source)?;
        let observer = self
            .space
            .start_accounting_operation(source, &consume.channels, &[])?;
        let result = self.consume_observed(consume, source, observer.as_deref(), events);
        if let Some(observer) = observer {
            observer.observe_operation_finish(source, completion(&result));
        }
        result
    }

    // replay_rspace.rs `locked_consume_observed`: refuse a directive, observe
    // the introduction, then park, or fire: observe_comm, after which an
    // unpaired fire stops with `ReplayDivergence`, and a paired fire logs
    // the consume and the COMM, removes the stored non-persistent records
    // (also for a peek, decision 45, Rule E), and removes the COMM's
    // bindings.
    fn consume_observed(
        &self,
        consume: ReadyConsume<C, P, A, K>,
        source: RSpaceOperationSource<'_>,
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        events: &mut Vec<Event>,
    ) -> Result<SpaceOpReceipt<C, P, A, K>, SpaceTxnError> {
        reject_directive(observer, source)?;
        let ReadyConsume {
            source: consume_ref,
            channels,
            patterns,
            continuation,
            persist,
            peeks,
            fire,
        } = consume;
        ReplayRSpace::<C, P, A, K>::observe_consume(
            observer,
            &consume_ref,
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
            source: consume_ref.clone(),
        };
        match fire {
            None => {
                self.space.log_consume(
                    consume_ref.clone(),
                    &channels,
                    &wk.patterns,
                    &wk.continuation,
                    persist,
                    &wk.peeks,
                );
                events.push(Event::IoEvent(IOEvent::Consume(consume_ref.clone())));
                let _ = self.space.store_waiting_continuation(channels, wk);
                Ok(SpaceOpReceipt::Parked {
                    consume: consume_ref,
                })
            }
            Some(ReadyConsumeFire {
                fire:
                    StagedConsumeFire {
                        candidates,
                        recorded,
                        ..
                    },
                comm,
                committed,
            }) => {
                ReplayRSpace::<C, P, A, K>::observe_comm(
                    observer,
                    &comm,
                    &wk.continuation,
                    wk.persist,
                    &candidates,
                )?;
                if recorded.is_none() {
                    return Err(SpaceTxnError::ReplayDivergence(Box::new(comm)));
                }
                self.space.log_consume(
                    consume_ref.clone(),
                    &channels,
                    &wk.patterns,
                    &wk.continuation,
                    persist,
                    &wk.peeks,
                );
                events.push(Event::IoEvent(IOEvent::Consume(consume_ref.clone())));
                self.space.log_comm(
                    &candidates,
                    &channels,
                    wk.clone(),
                    comm.clone(),
                    "comm.consume",
                );
                events.push(Event::Comm(comm.clone()));
                // Disabled by decision 45 (Rule E, "Apply everywhere"): a
                // retaining (peek) fire kept its records. Every fire now removes
                // its stored non-persistent records, as the host does.
                //
                // ```
                // if !retain {
                //     self.space.store_persistent_data(&committed);
                // }
                // ```
                self.space.store_persistent_data(&committed);
                self.space.remove_bindings_for(comm.clone());
                let fired = candidates.len();
                let wrapped = self
                    .space
                    .wrap_result(channels, wk, consume_ref, candidates);
                fired_receipt(comm, wrapped, fired, None)
            }
        }
    }
}

async fn begin_replay<C, P, A, K>(
    space: &ReplayRSpace<C, P, A, K>,
    mode: SpaceMode,
    lock_keys: &[C],
    registration: TxnRegistration,
) -> Result<BoxedSpaceTxn<C, P, A, K>, SpaceTxnError>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    if lock_keys.is_empty() {
        return Err(SpaceTxnError::InvalidOperation("lock keys must not be empty"));
    }
    let hashes: Vec<u64> = lock_keys.iter().map(striped_locks::channel_hash).collect();
    let coverage = LockCoverage::new(&hashes, space.phase_a_locks.len(), space.phase_b_locks.len());
    let locks = space.consume_lock(&hashes).await;
    let store = space.get_store();
    let installed = installed_channels(&space.installs)?;
    let backend = ReplayBackend {
        space: space.clone(),
    };
    Ok(Box::new(HostTxn::new(mode, backend, store, installed, coverage, locks, registration)))
}

fn replay_root_leaf_record_count<C, P, A, K>(
    space: &ReplayRSpace<C, P, A, K>,
    channel: &C,
) -> Result<u64, SpaceTxnError>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    let repository = space.get_history_repository();
    let reader = repository
        .get_history_reader(&repository.root())
        .map_err(RSpaceError::from)?;
    root_leaf_records(reader.as_ref(), channel)
}

// Each swept continuation lowers the waiting-continuations estimate as host
// continuation removal (`handle_match`) does: one decrement while the
// estimate is positive. A sweep matches nothing, so it counts no match.
async fn sweep_replay<C, P, A, K>(
    space: &ReplayRSpace<C, P, A, K>,
    channels: &[C],
) -> Result<SweepReport, SpaceTxnError>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    let channels = sweep_channels(channels)?;
    let hashes: Vec<u64> = channels.values().map(striped_locks::channel_hash).collect();
    let _guards = space.consume_lock(&hashes).await;
    let repository = space.get_history_repository();
    let reader = repository
        .get_history_reader(&repository.root())
        .map_err(RSpaceError::from)?;
    let installed = installed_channels(&space.installs)?;
    let store = space.get_store();
    sweep_volatile_channels(&**store, reader.as_ref(), &installed, &channels, || {
        if space
            .replay_waiting_continuations_estimate
            .load(Ordering::Relaxed) >
            0
        {
            space.dec_replay_waiting_continuations();
        }
    })
}

#[async_trait]
impl<C, P, A, K> SpaceTxnHost<C, P, A, K> for ReplayRSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    fn mode(&self) -> SpaceMode { SpaceMode::Replay }

    fn matcher(&self) -> Arc<Box<dyn Match<P, A, K>>> { Arc::clone(&self.matcher) }

    async fn begin(
        &self,
        lock_keys: &[C],
        registration: TxnRegistration,
    ) -> Result<BoxedSpaceTxn<C, P, A, K>, SpaceTxnError> {
        begin_replay(self, SpaceMode::Replay, lock_keys, registration).await
    }

    fn root_leaf_record_count(&self, channel: &C) -> Result<u64, SpaceTxnError> {
        replay_root_leaf_record_count(self, channel)
    }

    async fn sweep_volatile(
        &self,
        channels: &[C],
        _registration: &TxnRegistration,
    ) -> Result<SweepReport, SpaceTxnError> {
        sweep_replay(self, channels).await
    }
}

#[async_trait]
impl<C, P, A, K> SpaceTxnHost<C, P, A, K> for ReportingRspace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    fn mode(&self) -> SpaceMode { SpaceMode::Reporting }

    fn matcher(&self) -> Arc<Box<dyn Match<P, A, K>>> { Arc::clone(&self.replay_rspace.matcher) }

    async fn begin(
        &self,
        lock_keys: &[C],
        registration: TxnRegistration,
    ) -> Result<BoxedSpaceTxn<C, P, A, K>, SpaceTxnError> {
        begin_replay(&self.replay_rspace, SpaceMode::Reporting, lock_keys, registration).await
    }

    fn root_leaf_record_count(&self, channel: &C) -> Result<u64, SpaceTxnError> {
        replay_root_leaf_record_count(&self.replay_rspace, channel)
    }

    async fn sweep_volatile(
        &self,
        channels: &[C],
        _registration: &TxnRegistration,
    ) -> Result<SweepReport, SpaceTxnError> {
        sweep_replay(&self.replay_rspace, channels).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::rspace::rspace::RSpace;
    use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use crate::rspace::space_store_handle::{ConsumeOutcome, SpaceConsume, SpaceStoreHandle};

    struct AnyMatch;

    impl Match<String, String, String> for AnyMatch {
        fn get(&self, _pattern: &String, datum: &String) -> Option<String> { Some(datum.clone()) }
    }

    fn s(value: &str) -> String { value.to_string() }

    fn park_op(
        channels: &[&str],
        continuation: &str,
    ) -> SpaceConsume<String, String, String, String> {
        SpaceConsume {
            channels: channels.iter().map(|channel| s(channel)).collect(),
            patterns: vec![s("never"); channels.len()],
            continuation: s(continuation),
            persist: false,
            peeks: BTreeSet::new(),
            outcome: ConsumeOutcome::Park,
            before_observe: None,
        }
    }

    // Every continuation the replay sweep removes lowers the waiting
    // continuations estimate as host continuation removal does: one
    // decrement while the estimate is positive, never below zero.
    #[tokio::test]
    async fn space_store_handle_replay_sweep_lowers_waiting_estimate() {
        let mut stores = InMemoryStoreManager::new();
        let (_play, replay) = RSpace::<String, String, String, String>::create_with_replay(
            stores
                .r_space_stores()
                .await
                .expect("create the in-memory RSpace stores"),
            Arc::new(Box::new(AnyMatch)),
        )
        .expect("create the play and replay spaces");
        let estimate = || {
            replay
                .replay_waiting_continuations_estimate
                .load(Ordering::Relaxed)
        };
        let handle = SpaceStoreHandle::new(replay.clone());
        let keys = vec![s("v1"), s("v2")];
        let mut txn = handle.begin(&keys).await.expect("begin");
        txn.consume(park_op(&["v1"], "k1")).expect("park k1");
        txn.consume(park_op(&["v1", "v2"], "k2")).expect("park k2");
        txn.consume(park_op(&["v2"], "k3")).expect("park k3");
        txn.commit().await.expect("commit the parks");
        assert_eq!(estimate(), 3);
        let direct = WaitingContinuation::create(
            &vec![s("v2")],
            &vec![s("never")],
            &s("direct"),
            false,
            BTreeSet::new(),
        );
        assert_eq!(replay.get_store().put_continuation(&[s("v2")], direct), Some(true));
        assert_eq!(estimate(), 3);

        let report = handle
            .sweep_volatile(&keys)
            .await
            .expect("sweep the volatile channels");
        assert_eq!(report, SweepReport {
            channels: 2,
            data: 0,
            continuations: 4,
            joins: 4,
        });
        assert_eq!(estimate(), 0);
    }
}
