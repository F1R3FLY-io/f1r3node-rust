// Play mode of the Reified RSpaces substrate: `SpaceTxnHost` for `RSpace`.
//
// `apply_op` first runs the read-only commit checks of the operation
// (`ready_produce` / `ready_consume`: unchanged join groups, counter room,
// and a fire located in the live store with its COMM recomputed from the
// live counters), so a failed check has no effect. Then it makes the host
// calls of ops_produce.rs / ops_consume.rs in the same order. The store
// steps `store_data`, `remove_matched_datum_and_join`,
// `store_waiting_continuation` and `store_persistent_data` are private to
// those modules, so they are re-stated at the end of this file over the
// public `HotStore` API, statement for statement.

use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;

use super::RSpace;
use crate::rspace::errors::RSpaceError;
use crate::rspace::hot_store::HotStore;
use crate::rspace::internal::{ConsumeCandidate, Datum, WaitingContinuation};
use crate::rspace::r#match::Match;
use crate::rspace::metrics_constants::{CONSUME_COMM_LABEL, PRODUCE_COMM_LABEL};
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
    fn store(&self) -> SharedStore<C, P, A, K> { self.space.get_store() }

    fn host_counter(&self, produce: &Produce) -> i32 {
        self.space
            .produce_counters(std::slice::from_ref(produce))
            .get(produce)
            .copied()
            .unwrap_or(0)
    }

    fn recorded(&self, _event: &IOEvent) -> Vec<(COMM, usize)> { Vec::new() }

    fn recorded_binding(&self, _event: &IOEvent, _comm: &COMM) -> Option<(COMM, usize)> { None }

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

impl<C, P, A, K> PlayBackend<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    // ops_produce.rs `locked_produce`: start (with the channel's join groups),
    // the observed body, then finish with the completion.
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
        let result = self.produce_observed(store, produce, observer.as_deref(), events);
        if let Some(observer) = observer {
            observer.observe_operation_finish(source, completion(&result));
        }
        result
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
        self.space.log_produce(&source, persist);
        events.push(Event::IoEvent(IOEvent::Produce(source.clone())));
        self.space.log_comm(comm.clone(), PRODUCE_COMM_LABEL);
        events.push(Event::Comm(comm.clone()));

        if let Some(index) = continuation_index {
            store.remove_continuation(&channels, index);
        }
        // Disabled by decision 45 (Rule E, "Apply everywhere"): a retaining
        // (peek) fire only unjoined its candidates. Every fire now removes its
        // stored non-persistent records, as the host does.
        //
        // ```
        // match retain {
        //     false => remove_matched_datum_and_join(store, &channels, &committed),
        //     true => remove_joins(store, &channels, &committed),
        // }
        // ```
        remove_matched_datum_and_join(store, &channels, &committed);
        let wrapped = self
            .space
            .wrap_result(&channels, &continuation, &candidates);
        fired_receipt(comm, wrapped, candidates.len(), Some(source))
    }

    // ops_consume.rs `locked_consume`: start with the consume channels and no
    // joins, the observed body, then finish with the completion.
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
        let result = self.consume_observed(store, consume, observer.as_deref(), events);
        if let Some(observer) = observer {
            observer.observe_operation_finish(source, completion(&result));
        }
        result
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
                self.space.log_consume(&source);
                events.push(Event::IoEvent(IOEvent::Consume(source.clone())));
                self.space.log_comm(comm.clone(), CONSUME_COMM_LABEL);
                events.push(Event::Comm(comm.clone()));
                // Disabled by decision 45 (Rule E, "Apply everywhere"): a
                // retaining (peek) fire kept its records. Every fire now removes
                // its stored non-persistent records, as the host does.
                //
                // ```
                // if !retain {
                //     store_persistent_data(store, &committed);
                // }
                // ```
                store_persistent_data(store, &committed);
                let wrapped = self.space.wrap_result(&channels, &wk, &candidates);
                fired_receipt(comm, wrapped, candidates.len(), None)
            }
        }
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

    async fn begin(
        &self,
        lock_keys: &[C],
        registration: TxnRegistration,
    ) -> Result<BoxedSpaceTxn<C, P, A, K>, SpaceTxnError> {
        if lock_keys.is_empty() {
            return Err(SpaceTxnError::InvalidOperation("lock keys must not be empty"));
        }
        let hashes: Vec<u64> = lock_keys.iter().map(striped_locks::channel_hash).collect();
        let coverage =
            LockCoverage::new(&hashes, self.phase_a_locks.len(), self.phase_b_locks.len());
        let locks = self.consume_lock(&hashes).await;
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
            coverage,
            locks,
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

// ops_produce.rs `store_data`.
fn store_data<C, P, A, K>(
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

// ops_produce.rs `remove_matched_datum_and_join`.
fn remove_matched_datum_and_join<C, P, A, K>(
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

// Disabled by decision 45 (Rule E, "Apply everywhere"): no fire keeps its
// records, so the retained form of `remove_matched_datum_and_join` has no
// caller.
//
// ```
// // The retained (peek) form of `remove_matched_datum_and_join`: the records
// // stay and only the joins of the fired group follow the continuation.
// fn remove_joins<C, P, A, K>(
//     store: &dyn HotStore<C, P, A, K>,
//     channels: &[C],
//     data_candidates: &[ConsumeCandidate<C, A>],
// ) where
//     C: Clone + Hash + Eq,
//     P: Clone,
//     A: Clone,
//     K: Clone,
// {
//     for consume_candidate in by_descending_index(data_candidates) {
//         store.remove_join(&consume_candidate.channel, channels);
//     }
// }
// ```

// ops_consume.rs `store_waiting_continuation`.
fn store_waiting_continuation<C, P, A, K>(
    store: &dyn HotStore<C, P, A, K>,
    channels: Vec<C>,
    wc: WaitingContinuation<P, K>,
) where
    C: Clone + Hash + Eq,
    P: Clone,
    A: Clone,
    K: Clone,
{
    let _ = store.put_continuation(&channels, wc);
    for channel in channels.iter() {
        store.put_join(channel, &channels);
    }
}

// ops_consume.rs `store_persistent_data`.
fn store_persistent_data<C, P, A, K>(
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

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
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
}
