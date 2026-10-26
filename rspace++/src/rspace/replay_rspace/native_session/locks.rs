use super::*;
use crate::rspace::striped_locks::ChannelLockGuard;

impl<C, P, A, K, E> NativeReplaySession<C, P, A, K, E>
where
    C: Clone
        + Debug
        + Default
        + Serialize
        + serde::de::DeserializeOwned
        + ColdStoreDecode
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
        + serde::de::DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + serde::de::DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    K: Clone
        + Debug
        + Default
        + Serialize
        + serde::de::DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    E: NativeReplayEpoch,
{
    pub(super) fn channel_hashes<'a>(
        &self,
        channels: impl IntoIterator<Item = &'a C>,
        count: usize,
    ) -> Vec<u64> {
        let mut hashes = Vec::with_capacity(count);
        for channel in channels {
            // Changed by D-O1 (DR-108): block accounting charges inline bytes
            // once per enclosing block. The channel is read through a
            // reference, as at the other lock sites.
            // crate::rspace::native_backing::inspect_slice(
            //     std::slice::from_ref(channel),
            //     &|operations, scanned, backing| {
            //         self.history_reserve(operations, scanned, backing)
            //     },
            // )?;
            hashes.push(striped_locks::channel_hash(channel));
        }
        hashes
    }

    pub(super) async fn consume_lock(
        &self,
        hashes: &[u64],
    ) -> Result<(ChannelLockGuard, ChannelLockGuard), RSpaceError> {
        let first = striped_locks::native::prepare(&self.space.phase_a_locks, hashes)?;
        let second = striped_locks::native::prepare(&self.space.phase_b_locks, hashes)?;
        Ok((first.acquire().await, second.acquire().await))
    }

    pub(super) async fn produce_lock(
        &self,
        channel: &C,
        key: crate::rspace::hashing::native_source::StoreKey,
    ) -> Result<(ChannelLockGuard, ChannelLockGuard), RSpaceError> {
        // Changed by D-O1 (DR-108): block accounting charges inline bytes
        // once per enclosing block.
        // crate::rspace::native_backing::inspect(
        //     channel,
        //     &|operations, scanned, backing| {
        //         self.history_reserve(operations, scanned, backing)
        //     },
        // )?;
        let hash = [striped_locks::channel_hash(channel)];
        let first = striped_locks::native::prepare(&self.space.phase_a_locks, &hash)?
            .acquire()
            .await;
        // Changed by D-C2c (D-S1, DR-96): the store reads by the channel key.
        // let joins = self.read_joins(channel)?;
        let joins = self.read_joins(channel, key)?;
        let count = joins.iter().map(Vec::len).sum();
        let hashes = self.channel_hashes(joins.iter().flatten(), count);
        let second = striped_locks::native::prepare(&self.space.phase_b_locks, &hashes)?
            .acquire()
            .await;
        Ok((first, second))
    }
}
