use shared::rust::clone_backing::CloneBacking;

use super::*;
use crate::rspace::striped_locks::ChannelLockGuard;

impl<C, P, A, K, E> NativeReplaySession<C, P, A, K, E>
where
    C: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + serde::de::DeserializeOwned
        + shared::rust::closed_decode::ClosedDecode
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
        + CloneBacking
        + serde::de::DeserializeOwned
        + shared::rust::closed_decode::ClosedDecode
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + serde::de::DeserializeOwned
        + shared::rust::closed_decode::ClosedDecode
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + CloneBacking
        + serde::de::DeserializeOwned
        + shared::rust::closed_decode::ClosedDecode
        + 'static
        + Sync
        + Send,
    E: NativeReplayEpoch,
{
    pub(super) fn channel_hashes<'a>(
        &self,
        channels: impl IntoIterator<Item = &'a C>,
        count: usize,
    ) -> Result<Vec<u64>, RSpaceError> {
        self.epoch.reserve_work(
            count.checked_add(1).ok_or(RSpaceError::HostWorkRejected)?,
            backing::<u64>(count)?,
        )?;
        let mut hashes = Vec::new();
        hashes
            .try_reserve_exact(count)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        for channel in channels {
            if hashes.len() == count {
                return Err(RSpaceError::HostWorkRejected);
            }
            crate::rspace::native_backing::inspect_slice(
                std::slice::from_ref(channel),
                &|operations, scanned, backing| self.history_reserve(operations, scanned, backing),
            )?;
            hashes.push(striped_locks::channel_hash(channel));
        }
        Ok(hashes)
    }

    pub(super) async fn consume_lock(
        &self,
        hashes: &[u64],
    ) -> Result<(ChannelLockGuard, ChannelLockGuard), RSpaceError> {
        self.consume_lock_with(hashes, |operations, bytes| {
            self.epoch.reserve_work(operations, bytes)
        })
        .await
    }

    pub(super) async fn consume_lock_with(
        &self,
        hashes: &[u64],
        reserve: impl Fn(usize, usize) -> Result<(), RSpaceError> + Send + Sync,
    ) -> Result<(ChannelLockGuard, ChannelLockGuard), RSpaceError> {
        let first = striped_locks::native::prepare(&self.space.phase_a_locks, hashes, &reserve)?;
        let second = striped_locks::native::prepare(&self.space.phase_b_locks, hashes, &reserve)?;
        Ok((first.acquire().await, second.acquire().await))
    }

    pub(super) async fn produce_lock(
        &self,
        channel: &C,
        key: crate::rspace::hashing::native_source::StoreKey,
    ) -> Result<(ChannelLockGuard, ChannelLockGuard), RSpaceError> {
        self.epoch.reserve_work(1, 0)?;
        crate::rspace::native_backing::inspect(channel, &|operations, scanned, backing| {
            self.history_reserve(operations, scanned, backing)
        })?;
        let hash = [striped_locks::channel_hash(channel)];
        let reserve = |operations, bytes| self.epoch.reserve_work(operations, bytes);
        let first = striped_locks::native::prepare(&self.space.phase_a_locks, &hash, reserve)?
            .acquire()
            .await;
        // Changed by D-C2c (D-S1, DR-96): the store reads by the channel key.
        // let joins = self.read_joins(channel)?;
        let joins = self.read_joins(channel, key)?;
        self.epoch.reserve_work(joins.len(), 0)?;
        let count = joins
            .iter()
            .try_fold(0usize, |count, join| count.checked_add(join.len()))
            .ok_or(RSpaceError::HostWorkRejected)?;
        let hashes = self.channel_hashes(joins.iter().flatten(), count)?;
        let second = striped_locks::native::prepare(&self.space.phase_b_locks, &hashes, reserve)?
            .acquire()
            .await;
        Ok((first, second))
    }
}
