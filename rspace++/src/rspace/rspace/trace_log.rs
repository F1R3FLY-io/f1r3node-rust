// Event-log and produce-counter bookkeeping (the deploy trace).

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::hash::Hash;

use serde::Serialize;

use super::RSpace;
use crate::rspace::metrics_constants::RSPACE_METRICS_SOURCE;
use crate::rspace::trace::event::{COMM, Consume, Event, IOEvent, Produce};

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    pub(super) fn produce_counters(&self, produce_refs: &[Produce]) -> BTreeMap<Produce, i32> {
        let counter = self.produce_counter.lock().expect("produce counter lock");
        produce_refs
            .iter()
            .map(|p| (p.clone(), counter.get(p).copied().unwrap_or(0)))
            .collect()
    }

    pub(super) fn log_comm(&self, comm: COMM, comm_metric_label: &'static str) {
        // Increment counter FIRST (matching Scala) using constants to avoid
        // memory leaks. The label is CONSUME_COMM_LABEL or PRODUCE_COMM_LABEL.
        metrics::counter!(comm_metric_label, "source" => RSPACE_METRICS_SOURCE).increment(1);

        // Then update event log (RSpace-specific behavior)
        self.event_log
            .lock()
            .expect("event log lock")
            .push(Event::Comm(comm));
    }

    pub(super) fn log_consume(&self, consume_ref: &Consume) {
        self.event_log
            .lock()
            .expect("event log lock")
            .push(Event::IoEvent(IOEvent::Consume(consume_ref.clone())));
    }

    pub(super) fn log_produce(&self, produce_ref: &Produce, persist: bool) {
        self.event_log
            .lock()
            .expect("event log lock")
            .push(Event::IoEvent(IOEvent::Produce(produce_ref.clone())));
        if !persist {
            let mut counter = self.produce_counter.lock().expect("produce counter lock");
            match counter.get_mut(produce_ref) {
                Some(count) => *count += 1,
                None => {
                    counter.insert(produce_ref.clone(), 1);
                }
            }
        }
    }
}
