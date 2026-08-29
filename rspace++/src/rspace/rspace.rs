// See rspace/src/main/scala/coop/rchain/rspace/RSpace.scala

// NOTE: Manual marks are used instead of trace_i()/with_marks() because
// the functions are not async-compatible with Span trait's closure pattern.
// This matches Scala's Span[F].traceI() and withMarks() semantics.
//
// Module layout (child modules of this file):
//   locks.rs       — two-phase striped lock acquisition + LOCK_SEQUENCE
//   setup.rs       — construction/wiring (apply, create, create_with_replay,
//                    create_history_repo, spawn, hot-store rebuild)
//   ispace_impl.rs — the ISpace trait impl (operation entry points,
//                    checkpointing, state accessors)
//   trace_log.rs   — event-log + produce-counter bookkeeping
//   ops_consume.rs — locked consume path and its store helpers
//   ops_produce.rs — locked produce path, matcher driver, candidate
//                    retirement
//   ops_install.rs — install registry and restore

use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

use serde::Serialize;
use shared::rust::store::key_value_store::KeyValueStore;

use crate::rspace::history::history_repository::HistoryRepository;
use crate::rspace::hot_store::HotStore;
use crate::rspace::internal::Install;
use crate::rspace::r#match::Match;
use crate::rspace::space_matcher::SpaceMatcher;
use crate::rspace::trace::Log;
use crate::rspace::trace::event::Produce;

mod ispace_impl;
mod locks;
mod ops_consume;
mod ops_install;
mod ops_produce;
mod setup;
#[cfg(test)]
mod tests;
mod trace_log;

pub use locks::LOCK_SEQUENCE;

#[derive(Clone)]
pub struct RSpaceStore {
    pub history: Arc<dyn KeyValueStore>,
    pub roots: Arc<dyn KeyValueStore>,
    pub cold: Arc<dyn KeyValueStore>,
}

#[repr(C)]
#[derive(Clone)]
pub struct RSpace<C, P, A, K> {
    // Left as RwLock, unlike store below: its only reader is spawn() (rare,
    // not on the produce/consume hot path), so it doesn't have that field's
    // problem.
    pub history_repository:
        Arc<std::sync::RwLock<Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>>>>,
    // get_store() is called multiple times per produce()/consume() and never
    // contends a writer on that path — writes only happen at checkpoint/spawn
    // boundaries, replacing the whole pointer wholesale. ArcSwap is lock-free
    // for this "read-mostly, rare wholesale swap" pattern: std::sync::RwLock
    // does atomic RMW on shared reader-count state per read-lock/unlock,
    // which scales badly under many concurrent readers (confirmed: isolated
    // read-lock-only benchmark measured negative scaling — see issue #50
    // follow-up).
    pub store: Arc<arc_swap::ArcSwap<Box<dyn HotStore<C, P, A, K>>>>,
    installs: Arc<std::sync::Mutex<HashMap<Vec<C>, Install<P, K>>>>,
    event_log: Arc<std::sync::Mutex<Log>>,
    // Striped like phase_a/phase_b_locks below: NUM_LOCK_STRIPES independent
    // shards keyed by channel_hash(produce) % NUM_LOCK_STRIPES, instead of
    // one global std::sync::Mutex<BTreeMap>. The hot path (log_produce/
    // produce_counters) only ever contends the one shard its key hashes to.
    // Checkpoint boundaries (take/reset/restore_produce_counter in
    // trace_log.rs) lock
    // every shard for the whole operation, in the same fixed index order
    // every time, restoring the single global lock's point-in-time
    // linearization for this field specifically — not across it and
    // event_log, which remain two separate locks taken sequentially, same
    // as before this change (pre-existing, out of scope here).
    produce_counter: Arc<Vec<std::sync::Mutex<BTreeMap<Produce, i32>>>>,
    matcher: Arc<Box<dyn Match<P, A, K>>>,
    // Fixed-size striped locks replace the growing DashMap<u64, Mutex>.
    // See striped_locks.rs for the stripe count and hashing/lock scheme.
    // No DashMap entry() → no parking_lot shard contention per produce/consume.
    phase_a_locks: Arc<Vec<Arc<tokio::sync::Mutex<()>>>>,
    phase_b_locks: Arc<Vec<Arc<tokio::sync::Mutex<()>>>>,
}

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    pub fn get_store(&self) -> Arc<Box<dyn HotStore<C, P, A, K>>> { self.store.load_full() }

    pub fn get_history_repository(
        &self,
    ) -> Arc<Box<dyn HistoryRepository<C, P, A, K> + Send + Sync + 'static>> {
        self.history_repository
            .read()
            .expect("history read lock")
            .clone()
    }
}

impl<C, P, A, K> SpaceMatcher<C, P, A, K> for RSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
}
