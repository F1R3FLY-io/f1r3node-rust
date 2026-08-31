// Two-phase striped channel locking for RSpace operations.
// See striped_locks.rs for the stripe count and hashing/lock scheme.

use std::fmt::Debug;
use std::hash::Hash;
use std::sync::atomic::AtomicU64;
use std::time::Instant;

use serde::Serialize;

use super::RSpace;
use crate::rspace::metrics_constants::RSPACE_METRICS_SOURCE;
use crate::rspace::striped_locks::{self, ChannelLockGuard};

pub static LOCK_SEQUENCE: AtomicU64 = AtomicU64::new(0);

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    pub(super) async fn consume_lock(
        &self,
        channel_hashes: &[u64],
    ) -> (ChannelLockGuard, ChannelLockGuard) {
        striped_locks::consume_lock(&self.phase_a_locks, &self.phase_b_locks, channel_hashes).await
    }

    // Sub-phase timing (issue #50) — see
    // issues/02-intra-deploy-par-mutex-rspace.md.
    pub(super) async fn produce_lock(&self, channel: &C) -> (ChannelLockGuard, ChannelLockGuard) {
        let phase_a_start = Instant::now();
        let channel_hash = striped_locks::channel_hash(channel);
        let phase_a = striped_locks::acquire_locks(&self.phase_a_locks, &[channel_hash]).await;
        metrics::counter!("rspace.produce.phase_a_wait_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(phase_a_start.elapsed().as_nanos() as u64);

        let joins_start = Instant::now();
        let store = self.get_store();
        let join_hashes: Vec<u64> = store
            .get_joins(channel)
            .into_iter()
            .flatten()
            .map(|ch| striped_locks::channel_hash(&ch))
            .collect();
        metrics::counter!("rspace.produce.get_joins_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(joins_start.elapsed().as_nanos() as u64);

        let phase_b_start = Instant::now();
        let phase_b = striped_locks::acquire_locks(&self.phase_b_locks, &join_hashes).await;
        metrics::counter!("rspace.produce.phase_b_wait_ns", "source" => RSPACE_METRICS_SOURCE)
            .increment(phase_b_start.elapsed().as_nanos() as u64);

        (phase_a, phase_b)
    }
}
