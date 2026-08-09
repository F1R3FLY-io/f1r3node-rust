// Fixed-size striped-lock scheme shared by `RSpace` and `ReplayRSpace`.
//
// 256 pre-allocated mutexes, channel hash % NUM_LOCK_STRIPES -> stripe
// index, replacing a growing DashMap<u64, Mutex> (PR #72 fixed this for
// RSpace's phase_a_locks/phase_b_locks; issue #43 ported the same fix to
// ReplayRSpace, which had been left on the pre-#72 DashMap). Extracted here
// because both types otherwise carried verbatim copies of this machinery.

use std::hash::Hash;
use std::sync::Arc;

pub(crate) const NUM_LOCK_STRIPES: usize = 256;

pub(crate) struct HeldLock {
    _guard: tokio::sync::OwnedMutexGuard<()>,
}

pub(crate) struct ChannelLockGuard {
    _held: Vec<HeldLock>,
}

pub(crate) fn channel_hash<C: Hash>(channel: &C) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    channel.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn new_striped_locks() -> Arc<Vec<Arc<tokio::sync::Mutex<()>>>> {
    Arc::new(
        (0..NUM_LOCK_STRIPES)
            .map(|_| Arc::new(tokio::sync::Mutex::new(())))
            .collect(),
    )
}

pub(crate) async fn acquire_locks(
    stripes: &[Arc<tokio::sync::Mutex<()>>],
    keys: &[u64],
) -> ChannelLockGuard {
    // Map channel hashes to stripe indices, sort to prevent deadlocks,
    // dedup so two channels in the same stripe are only locked once.
    let mut indices: Vec<usize> = keys.iter().map(|k| (*k as usize) % stripes.len()).collect();
    indices.sort();
    indices.dedup();

    let mut held: Vec<HeldLock> = Vec::with_capacity(indices.len());
    for idx in indices {
        let guard = stripes[idx].clone().lock_owned().await;
        held.push(HeldLock { _guard: guard });
    }

    ChannelLockGuard { _held: held }
}

pub(crate) async fn consume_lock(
    phase_a_locks: &[Arc<tokio::sync::Mutex<()>>],
    phase_b_locks: &[Arc<tokio::sync::Mutex<()>>],
    channel_hashes: &[u64],
) -> (ChannelLockGuard, ChannelLockGuard) {
    let phase_a = acquire_locks(phase_a_locks, channel_hashes).await;
    let phase_b = acquire_locks(phase_b_locks, channel_hashes).await;
    (phase_a, phase_b)
}
