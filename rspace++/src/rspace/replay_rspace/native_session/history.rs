use serde::de::DeserializeOwned;

use super::*;
// Changed by D-C2d (D-S1, DR-96): the cold fills read the history by the
// store keys, so they no longer hash a channel.
// use crate::rspace::hashing::native_source::{
//     GroupKeys, SourceMeter, StoreKey, channels_hash, hash,
// };
use crate::rspace::hashing::native_source::{GroupKeys, StoreKey, channels_hash_from_keys};
use crate::rspace::history::native_reader::{
    NativeLeafKind, NativeReadError, decode_continuation, decode_datum, decode_join,
};
use crate::rspace::hot_store::NativeDataView;
use crate::rspace::serializers::cold_store_decode::{ColdStoreDecode, ColdStoreDecodeError};

fn read_error(error: NativeReadError<RSpaceError>) -> RSpaceError {
    match error {
        NativeReadError::Consumer(error) => error,
        NativeReadError::Store(error) => error.into(),
        NativeReadError::Invalid(error) => {
            RSpaceError::InterpreterError(format!("native history: {error:?}"))
        }
    }
}

impl<C, P, A, K, E> NativeReplaySession<C, P, A, K, E>
where
    C: Clone
        + Debug
        + Default
        + Serialize
        + DeserializeOwned
        + ColdStoreDecode
        + Hash
        + Ord
        + Eq
        + 'static
        + Sync
        + Send,
    P: Clone
        + PartialEq
        + Debug
        + Default
        + Serialize
        + DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    A: Clone
        + Debug
        + Default
        + Serialize
        + DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    K: Clone
        + PartialEq
        + Debug
        + Default
        + Serialize
        + DeserializeOwned
        + ColdStoreDecode
        + 'static
        + Sync
        + Send,
    E: NativeReplayEpoch,
{
    /// Changed by DR-131 S6: the rows of one native history leaf, each
    /// decoded by `decode`, the cold-store decoder of the leaf's kind
    /// (`decode_datum`, `decode_join` or `decode_continuation`). The typed
    /// decoder, its meter and its 128-level stop are deleted (D-A). A row
    /// that the decoder rejects is a typed-record fault, as before.
    pub(super) fn read_records<T>(
        &self,
        kind: NativeLeafKind,
        // Changed by D-C2d (D-S1, DR-96): the projection is the 32 bytes of a
        // store key's digest, so it needs no conversion.
        // projection: Blake2b256Hash,
        projection: &[u8; 32],
        decode: impl Fn(&[u8]) -> Result<T, ColdStoreDecodeError>,
    ) -> Result<Vec<T>, RSpaceError> {
        let history = self.space.get_history_repository();
        // let projection = projection.0.as_slice().try_into().map_err(|_| {
        //     RSpaceError::InterpreterError("native history projection
        // length".to_owned()) })?;
        history
            .native_history_reader(self.root)
            // Changed by D-C2d (D-S1, DR-96): the projection is a reference.
            // .with_records(kind, &projection, &meter, |rows| {
            .with_records(kind, projection, |rows| {
                let mut values = Vec::with_capacity(rows.len());
                for row in rows.iter() {
                    // Changed by D-S2 (DR-95): the rows decode in History mode,
                    // which charges each node once and reserves only real
                    // allocations.
                    // values.push(decode_record(row, &decode_meter).map_err(read_error)?);
                    values.push(decode(row).map_err(|error| read_error(error.into()))?);
                }
                Ok(values)
            })
            .map_err(read_error)
            .map(Option::unwrap_or_default)
    }

    // Changed by D-C2c (D-S1, DR-96): the native session uses its
    // digest-keyed store; each reader takes the key of its channel or group.
    // pub(super) fn read_data_with(
    //     &self,
    //     channel: &C,
    //     reserve: &impl SourceMeter,
    // ) -> Result<Vec<Datum<A>>, RSpaceError> {
    //     self.space.get_store().get_data_with_reader(
    //         channel,
    //         &|| self.read_records(NativeLeafKind::Data, hash(channel, reserve)?,
    // reserve),         reserve,
    //     )
    // }
    pub(super) fn read_data_with(
        &self,
        channel: &C,
        key: StoreKey,
    ) -> Result<Vec<Datum<A>>, RSpaceError> {
        self.store.data(
            channel,
            key,
            // Changed by D-C2d (D-S1, DR-96): the cold fill reads by the key's
            // digest instead of hashing the channel again.
            // &|| self.read_records(NativeLeafKind::Data, hash(channel, reserve)?, reserve),
            &|| self.read_records(NativeLeafKind::Data, &key.0, decode_datum::<A>),
        )
    }

    // pub(super) fn read_joins(&self, channel: &C) -> Result<Vec<Vec<C>>,
    // RSpaceError> {     let reserve =
    //         |operations, scanned, backing| self.history_reserve(operations, scanned, backing);
    //     self.space.get_store().get_joins_with_reader(
    //         channel,
    //         &|| self.read_records(NativeLeafKind::Joins, hash(channel,
    // &reserve)?, &reserve),         &reserve,
    //     )
    // }
    pub(super) fn read_joins(
        &self,
        channel: &C,
        key: StoreKey,
    ) -> Result<Vec<Vec<C>>, RSpaceError> {
        self.store.joins(
            channel,
            key,
            // Changed by D-C2d (D-S1, DR-96): the cold fill reads by the key's
            // digest instead of hashing the channel again.
            // &|| self.read_records(NativeLeafKind::Joins, hash(channel, &reserve)?, &reserve),
            &|| self.read_records(NativeLeafKind::Joins, &key.0, decode_join::<C>),
        )
    }

    // pub(super) fn read_continuations(
    //     &self,
    //     channels: &[C],
    // ) -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError> {
    //     let reserve =
    //         |operations, scanned, backing| self.history_reserve(operations, scanned, backing);
    //     self.space.get_store().get_continuations_with_reader(
    //         channels,
    //         &|| {
    //             self.read_records(
    //                 NativeLeafKind::Continuations,
    //                 channels_hash(channels, &reserve)?,
    //                 &reserve,
    //             )
    //         },
    //         &reserve,
    //     )
    // }
    pub(super) fn read_continuations(
        &self,
        channels: &[C],
        keys: &GroupKeys,
    ) -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError> {
        self.store.continuations(channels, keys, &|| {
            self.read_records(
                NativeLeafKind::Continuations,
                // Changed by D-C2d (D-S1, DR-96): the projection sorts the
                // channel keys instead of hashing every channel again.
                // channels_hash(channels, &reserve)?,
                &channels_hash_from_keys(&keys.channels),
                decode_continuation::<P, K>,
            )
        })
    }

    // /// The continuations of `channels` as shared views (C1, DR-81).
    // pub(super) fn read_continuation_views(
    //     &self,
    //     channels: &[C],
    // ) -> Result<Vec<Arc<WaitingContinuation<P, K>>>, RSpaceError> {
    //     let reserve =
    //         |operations, scanned, backing| self.history_reserve(operations, scanned, backing);
    //     self.space.get_store().get_continuation_views_with_reader(
    //         channels,
    //         &|| {
    //             self.read_records(
    //                 NativeLeafKind::Continuations,
    //                 channels_hash(channels, &reserve)?,
    //                 &reserve,
    //             )
    //         },
    //         &reserve,
    //     )
    // }
    /// The continuations of `channels` as shared views (C1, DR-81).
    pub(super) fn read_continuation_views(
        &self,
        channels: &[C],
        keys: &GroupKeys,
    ) -> Result<Vec<Arc<WaitingContinuation<P, K>>>, RSpaceError> {
        self.store.continuation_views(channels, keys, &|| {
            self.read_records(
                NativeLeafKind::Continuations,
                // Changed by D-C2d (D-S1, DR-96): the projection sorts the
                // channel keys instead of hashing every channel again.
                // channels_hash(channels, &reserve)?,
                &channels_hash_from_keys(&keys.channels),
                decode_continuation::<P, K>,
            )
        })
    }

    // /// Fills the continuation cache of `channels` without copying a
    // /// continuation (C1, DR-81).
    // pub(super) fn prefetch_continuations(&self, channels: &[C]) -> Result<(),
    // RSpaceError> {     self.read_continuation_views(channels).map(drop)
    // }
    /// Fills the continuation cache of `channels` without copying a
    /// continuation (C1, DR-81).
    pub(super) fn prefetch_continuations(
        &self,
        channels: &[C],
        keys: &GroupKeys,
    ) -> Result<(), RSpaceError> {
        self.read_continuation_views(channels, keys).map(drop)
    }

    // Disabled by C2 (DR-82): the produce path prefetches with prefetch_data,
    // which copies no datum.
    // pub(super) fn prepare_data(&self, channel: &C) -> Result<(), RSpaceError> {
    //     self.read_data_with(channel, &|operations, scanned, backing| {
    //         self.history_reserve(operations, scanned, backing)
    //     })
    //     .map(|_| ())
    // }

    // Changed by D-C2c (D-S1, DR-96): the native session uses its
    // digest-keyed store.
    // /// A copy-free view of the cached data of `channel` (C2, DR-82).
    // pub(super) fn read_data_view_with(
    //     &self,
    //     channel: &C,
    //     reserve: &impl SourceMeter,
    // ) -> Result<NativeDataView<C, A>, RSpaceError> {
    //     self.space.get_store().get_data_view_with_reader(
    //         channel,
    //         &|| self.read_records(NativeLeafKind::Data, hash(channel, reserve)?,
    // reserve),         reserve,
    //     )
    // }
    /// A copy-free view of the cached data of `channel` (C2, DR-82).
    pub(super) fn read_data_view_with(
        &self,
        channel: &C,
        key: StoreKey,
    ) -> Result<NativeDataView<C, A>, RSpaceError> {
        self.store.data_view(
            channel,
            key,
            // Changed by D-C2d (D-S1, DR-96): the cold fill reads by the key's
            // digest instead of hashing the channel again.
            // &|| self.read_records(NativeLeafKind::Data, hash(channel, reserve)?, reserve),
            &|| self.read_records(NativeLeafKind::Data, &key.0, decode_datum::<A>),
        )
    }

    // /// Fills the data cache of `channel` without copying a datum
    // /// (C2, DR-82).
    // pub(super) fn prefetch_data(&self, channel: &C) -> Result<(), RSpaceError> {
    //     self.read_data_view_with(channel, &|operations, scanned, backing| {
    //         self.history_reserve(operations, scanned, backing)
    //     })
    //     .map(drop)
    // }
    /// Fills the data cache of `channel` without copying a datum
    /// (C2, DR-82).
    pub(super) fn prefetch_data(&self, channel: &C, key: StoreKey) -> Result<(), RSpaceError> {
        self.read_data_view_with(channel, key).map(drop)
    }
}
