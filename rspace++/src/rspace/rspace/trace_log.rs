// Event-log and produce-counter bookkeeping (the deploy trace).

use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use std::hash::Hash;
use std::mem::size_of;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use shared::rust::collection_backing::{tree_backing, tree_search_bound};

use super::RSpace;
use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::native_source::SourceMeter;
use crate::rspace::internal::{ConsumeCandidate, WaitingContinuation};
use crate::rspace::metrics_constants::RSPACE_METRICS_SOURCE;
use crate::rspace::operation_context::{self, OperationOrder, PathSegment};
use crate::rspace::striped_locks;
use crate::rspace::trace::Log;
use crate::rspace::trace::event::{COMM, Consume, Event, IOEvent, Produce};

/// Outstanding slots are counted per destination. Every ordinary log append
/// preserves these slots, so a resting commit can publish after retirement
/// without allocation or a SourceMeter call. The reservation mutex is always
/// acquired before the selected log mutex; no log mutex is held while HotStore
/// retirement holds shard locks.
#[derive(Default)]
pub(super) struct EventLogReservations {
    pub(super) plain: usize,
    pub(super) ordered: BTreeMap<OperationOrder, usize>,
}

pub(super) struct EventSlotLease {
    reservations: Arc<Mutex<EventLogReservations>>,
    plain: Arc<Mutex<Log>>,
    ordered: Arc<Mutex<BTreeMap<OperationOrder, Log>>>,
    order: Option<OperationOrder>,
    committed: bool,
}

impl EventSlotLease {
    pub(super) fn publish(mut self, event: Event) {
        let mut reservations = self.reservations.lock().expect("event reservations lock");
        if let Some(order) = &self.order {
            let mut ordered = self.ordered.lock().expect("ordered event log lock");
            let slots = reservations
                .ordered
                .get_mut(order)
                .expect("ordered slot reserved");
            *slots -= 1;
            if *slots == 0 {
                reservations.ordered.remove(order);
            }
            let log = ordered.get_mut(order).expect("ordered log slot exists");
            assert!(log.len() < log.capacity(), "reserved ordered event slot was lost");
            log.push(event);
        } else {
            let mut log = self.plain.lock().expect("event log lock");
            reservations.plain -= 1;
            assert!(log.len() < log.capacity(), "reserved event slot was lost");
            log.push(event);
        }
        self.committed = true;
    }
}

impl Drop for EventSlotLease {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let mut reservations = self.reservations.lock().expect("event reservations lock");
        if let Some(order) = &self.order {
            let mut ordered = self.ordered.lock().expect("ordered event log lock");
            let slots = reservations
                .ordered
                .get_mut(order)
                .expect("ordered slot reserved");
            *slots -= 1;
            if *slots == 0 {
                reservations.ordered.remove(order);
                if ordered.get(order).is_some_and(Vec::is_empty) {
                    ordered.remove(order);
                }
            }
        } else {
            reservations.plain -= 1;
        }
    }
}

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
    /// Reserve one destination-specific event slot before a resting store
    /// transaction is authorized. Ordinary writers preserve every live slot;
    /// the lease's `publish` therefore has no allocation or fallible work.
    pub(super) fn reserve_resting_event_slot(
        &self,
        meter: &dyn SourceMeter,
    ) -> Result<EventSlotLease, RSpaceError> {
        let order = operation_context::current();
        let mut reservations = self
            .event_log_reservations
            .lock()
            .expect("event reservations lock");
        match order.as_ref() {
            None => {
                let mut log = self.event_log.lock().expect("event log lock");
                let needed = reservations
                    .plain
                    .checked_add(1)
                    .ok_or(RSpaceError::HostWorkRejected)?;
                let target = log
                    .len()
                    .checked_add(needed)
                    .ok_or(RSpaceError::HostWorkRejected)?;
                let bytes = target
                    .checked_mul(size_of::<Event>())
                    .ok_or(RSpaceError::HostWorkRejected)?;
                let copied = log
                    .len()
                    .checked_mul(size_of::<Event>())
                    .ok_or(RSpaceError::HostWorkRejected)?;
                meter.reserve(1, copied, bytes)?;
                if log.capacity() < target {
                    log.try_reserve_exact(needed)
                        .map_err(|_| RSpaceError::HostWorkRejected)?;
                }
                reservations.plain = needed;
            }
            Some(order) => {
                let mut logs = self
                    .ordered_event_log
                    .lock()
                    .expect("ordered event log lock");
                let entries = logs
                    .len()
                    .max(reservations.ordered.len())
                    .checked_add(1)
                    .ok_or(RSpaceError::HostWorkRejected)?;
                let max_depth = logs
                    .keys()
                    .chain(reservations.ordered.keys())
                    .map(|key| key.path.len())
                    .max()
                    .unwrap_or(0)
                    .max(order.path.len());
                // At most eight ordered lookups/inserts/removals cover both
                // reservation and publication (or refusal cleanup). The
                // allocation-free causal-path comparator may lift at every
                // depth, with at most `usize::BITS` pointer steps per lift.
                let comparisons = tree_search_bound(entries)
                    .checked_mul(8)
                    .ok_or(RSpaceError::HostWorkRejected)?;
                let path_bytes = max_depth
                    .checked_mul(size_of::<PathSegment>())
                    .and_then(|bytes| bytes.checked_mul(2))
                    .and_then(|bytes| bytes.checked_mul(comparisons))
                    .ok_or(RSpaceError::HostWorkRejected)?;
                let path_steps = max_depth
                    .checked_mul(usize::BITS as usize)
                    .and_then(|steps| steps.checked_mul(4))
                    .and_then(|steps| steps.checked_mul(comparisons))
                    .ok_or(RSpaceError::HostWorkRejected)?;
                let (log_ops, log_backing) = tree_backing::<OperationOrder, Log>(entries)
                    .ok_or(RSpaceError::HostWorkRejected)?;
                let (reservation_ops, reservation_backing) =
                    tree_backing::<OperationOrder, usize>(entries)
                        .ok_or(RSpaceError::HostWorkRejected)?;
                meter.reserve(
                    log_ops
                        .checked_add(reservation_ops)
                        .and_then(|ops| ops.checked_add(path_steps))
                        .ok_or(RSpaceError::HostWorkRejected)?,
                    path_bytes,
                    log_backing
                        .checked_add(reservation_backing)
                        .and_then(|bytes| bytes.checked_add(path_bytes))
                        .ok_or(RSpaceError::HostWorkRejected)?,
                )?;
                let needed = reservations
                    .ordered
                    .get(order)
                    .copied()
                    .unwrap_or(0)
                    .checked_add(1)
                    .ok_or(RSpaceError::HostWorkRejected)?;
                let present_len = logs.get(order).map_or(0, Vec::len);
                let target = present_len
                    .checked_add(needed)
                    .ok_or(RSpaceError::HostWorkRejected)?;
                let bytes = target
                    .checked_mul(size_of::<Event>())
                    .ok_or(RSpaceError::HostWorkRejected)?;
                let copied = present_len
                    .checked_mul(size_of::<Event>())
                    .ok_or(RSpaceError::HostWorkRejected)?;
                meter.reserve(1, copied, bytes)?;
                let log = logs.entry(order.clone()).or_default();
                if log.capacity() < target && log.try_reserve_exact(needed).is_err() {
                    if log.is_empty() {
                        logs.remove(order);
                    }
                    return Err(RSpaceError::HostWorkRejected);
                }
                reservations.ordered.insert(order.clone(), needed);
            }
        }
        Ok(EventSlotLease {
            reservations: Arc::clone(&self.event_log_reservations),
            plain: Arc::clone(&self.event_log),
            ordered: Arc::clone(&self.ordered_event_log),
            order,
            committed: false,
        })
    }

    pub(super) fn push_event(&self, event: Event) {
        let reservations = self
            .event_log_reservations
            .lock()
            .expect("event reservations lock");
        if let Some(order) = crate::rspace::operation_context::current() {
            let needed = reservations.ordered.get(&order).copied().unwrap_or(0) + 1;
            let mut ordered = self
                .ordered_event_log
                .lock()
                .expect("ordered event log lock");
            let log = ordered.entry(order).or_default();
            log.reserve(needed);
            log.push(event);
        } else {
            let mut log = self.event_log.lock().expect("event log lock");
            log.reserve(reservations.plain + 1);
            log.push(event);
        }
    }

    pub(super) fn take_ordered_event_log(&self) -> Log {
        let reservations = self
            .event_log_reservations
            .lock()
            .expect("event reservations lock");
        assert!(
            reservations.plain == 0 && reservations.ordered.is_empty(),
            "checkpoint drained an active resting event lease"
        );
        drop(reservations);
        let mut log = std::mem::take(&mut *self.event_log.lock().expect("event log lock"));
        let ordered = std::mem::take(
            &mut *self
                .ordered_event_log
                .lock()
                .expect("ordered event log lock"),
        );
        for events in ordered.into_values() {
            log.extend(events);
        }
        log
    }

    pub(super) fn clear_ordered_event_log(&self) {
        let reservations = self
            .event_log_reservations
            .lock()
            .expect("event reservations lock");
        assert!(
            reservations.plain == 0 && reservations.ordered.is_empty(),
            "reset cleared an active resting event lease"
        );
        drop(reservations);
        self.ordered_event_log
            .lock()
            .expect("ordered event log lock")
            .clear();
    }

    // Single-shard lookup for the hot path (log_produce): a given key always
    // hashes to the same shard, so per-key correctness holds regardless of
    // what any other shard is doing concurrently.
    fn produce_counter_shard(
        &self,
        produce_ref: &Produce,
    ) -> &std::sync::Mutex<BTreeMap<Produce, i32>> {
        let idx = (striped_locks::channel_hash(produce_ref) as usize) % self.produce_counter.len();
        &self.produce_counter[idx]
    }

    // Groups by shard first so a slice with repeats or same-shard keys locks
    // each needed shard only once, instead of once per produce_ref.
    pub(super) fn produce_counters(&self, produce_refs: &[Produce]) -> BTreeMap<Produce, i32> {
        let mut by_shard: HashMap<usize, Vec<&Produce>> = HashMap::new();
        for p in produce_refs {
            let idx = (striped_locks::channel_hash(p) as usize) % self.produce_counter.len();
            by_shard.entry(idx).or_default().push(p);
        }

        let mut result = BTreeMap::new();
        for (idx, refs) in by_shard {
            let guard = self.produce_counter[idx]
                .lock()
                .expect("produce counter shard lock");
            for p in refs {
                result.insert(p.clone(), guard.get(p).copied().unwrap_or(0));
            }
        }
        result
    }

    // Locks every shard, in the same fixed index order every helper below
    // uses, before touching any of them, and holds all guards for the whole
    // operation. This is the equivalent of the former single global lock's
    // linearization point, restricted to this field: any produce() racing
    // take/reset/restore blocks on whichever shard it hashes to until the
    // full boundary operation finishes on every shard, so the result is a
    // consistent point-in-time snapshot across shards, not just per-key.
    //
    // (This does not make produce_counter atomic *with* event_log — those
    // remain two separate locks taken sequentially, same as before this
    // change; that cross-field ordering gap is pre-existing, not introduced
    // here, and is out of scope until event_log itself is addressed.)
    fn all_produce_counter_shards(&self) -> Vec<std::sync::MutexGuard<'_, BTreeMap<Produce, i32>>> {
        self.produce_counter
            .iter()
            .map(|shard| shard.lock().expect("produce counter shard lock"))
            .collect()
    }

    // Drains all shards into one combined map and leaves every shard empty,
    // atomically across the whole field (see all_produce_counter_shards).
    pub(super) fn take_produce_counter(&self) -> BTreeMap<Produce, i32> {
        let mut guards = self.all_produce_counter_shards();
        let mut combined = BTreeMap::new();
        for guard in guards.iter_mut() {
            combined.extend(std::mem::take(&mut **guard));
        }
        combined
    }

    // Empties every shard atomically across the whole field.
    pub(super) fn reset_produce_counter(&self) {
        let mut guards = self.all_produce_counter_shards();
        for guard in guards.iter_mut() {
            **guard = BTreeMap::new();
        }
    }

    // Partitions the incoming map by shard before taking any locks, then
    // installs each partition with one assignment per shard under all
    // guards held together — avoids the reset-then-insert window a
    // concurrent produce() could otherwise land in between.
    pub(super) fn restore_produce_counter(&self, map: BTreeMap<Produce, i32>) {
        let mut partitioned: Vec<BTreeMap<Produce, i32>> = (0..self.produce_counter.len())
            .map(|_| BTreeMap::new())
            .collect();
        for (k, v) in map {
            let idx = (striped_locks::channel_hash(&k) as usize) % self.produce_counter.len();
            partitioned[idx].insert(k, v);
        }

        let mut guards = self.all_produce_counter_shards();
        for (guard, part) in guards.iter_mut().zip(partitioned) {
            **guard = part;
        }
    }

    pub(super) fn log_comm(&self, comm: COMM, comm_metric_label: &'static str) {
        // Increment counter FIRST (matching Scala) using constants to avoid
        // memory leaks. The label is CONSUME_COMM_LABEL or PRODUCE_COMM_LABEL.
        metrics::counter!(comm_metric_label, "source" => RSPACE_METRICS_SOURCE).increment(1);

        // Then update event log (RSpace-specific behavior)
        self.push_event(Event::Comm(comm));
    }

    pub(super) fn observe_step_comm(
        &self,
        channels: &[C],
        continuation: &WaitingContinuation<P, K>,
        data_candidates: &[ConsumeCandidate<C, A>],
        comm: &COMM,
        label: &'static str,
    ) {
        if let Some(observer) = &self.step_observer {
            let consumed: Vec<A> = data_candidates
                .iter()
                .map(|candidate| (*candidate.datum.a).clone())
                .collect();
            observer.observe_comm(
                channels,
                &consumed,
                &continuation.patterns,
                &continuation.continuation,
                comm,
                label,
            );
        }
    }

    pub(super) fn log_consume(&self, consume_ref: &Consume) {
        self.push_event(Event::IoEvent(IOEvent::Consume(consume_ref.clone())));
    }

    pub(super) fn log_produce(&self, produce_ref: &Produce, persist: bool) {
        self.push_event(Event::IoEvent(IOEvent::Produce(produce_ref.clone())));
        if !persist {
            let mut counter = self
                .produce_counter_shard(produce_ref)
                .lock()
                .expect("produce counter shard lock");
            match counter.get_mut(produce_ref) {
                Some(count) => *count += 1,
                None => {
                    counter.insert(produce_ref.clone(), 1);
                }
            }
        }
    }
}
