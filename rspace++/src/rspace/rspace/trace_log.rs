// Event-log and produce-counter bookkeeping (the deploy trace).

use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use std::hash::Hash;

use serde::Serialize;

use super::RSpace;
use crate::rspace::metrics_constants::RSPACE_METRICS_SOURCE;
use crate::rspace::striped_locks;
use crate::rspace::trace::Log;
use crate::rspace::trace::event::{COMM, Consume, Event, IOEvent, Produce};

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    pub(super) fn push_event(&self, event: Event) {
        if let Some(order) = crate::rspace::operation_context::current() {
            self.ordered_event_log
                .lock()
                .expect("ordered event log lock")
                .entry(order)
                .or_default()
                .push(event);
        } else {
            self.event_log.lock().expect("event log lock").push(event);
        }
    }

    pub(super) fn take_ordered_event_log(&self) -> Log {
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
