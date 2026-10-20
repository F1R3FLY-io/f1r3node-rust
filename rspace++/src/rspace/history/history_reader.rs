use crate::rspace::errors::{HistoryError, RSpaceError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::stable_hash_provider::{
    EventCodecEpoch, StableHashSerialize, channel_hash_for_epoch, channel_tuple_hash_for_epoch,
};
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::serializers::cold_store_decode::{ColdStoreDecode, ColdStoreDecodeError};
use crate::rspace::serializers::serializers::{
    decode_continuation_for_epoch, decode_datum_for_epoch,
};

/**
 * Reader for particular history (state verified on blockchain)
 *
 * @tparam Key type for hash of a channel
 * @tparam C type for Channel => this is Par
 * @tparam P type for Pattern => this is BindPattern
 * @tparam A type for Abstraction => this is ListParWithRandom
 * @tparam K type for Continuation => this is TaggedContinuation
 *
 * See rspace/src/main/scala/coop/rchain/rspace/history/HistoryReader.scala
 */
pub trait HistoryReader<Key, C: Clone, P: Clone, A: Clone, K: Clone>: Send + Sync {
    // Get current root which reader reads from
    fn root(&self) -> Key;

    fn get_data_proj(&self, key: &Key) -> Result<Vec<Datum<A>>, HistoryError>;

    fn get_data_proj_binary(&self, key: &Key) -> Result<Vec<Vec<u8>>, HistoryError>;

    fn get_continuations_proj(
        &self,
        key: &Key,
    ) -> Result<Vec<WaitingContinuation<P, K>>, HistoryError>;

    fn get_continuations_proj_binary(&self, key: &Key) -> Result<Vec<Vec<u8>>, HistoryError>;

    fn get_joins_proj(&self, key: &Key) -> Result<Vec<Vec<C>>, HistoryError>;

    fn get_joins_proj_binary(&self, key: &Key) -> Result<Vec<Vec<u8>>, HistoryError>;

    /**                                                                      
     *                                                                      
     * Defaults
     *                                                                      
     *                                                                      
     *
     */
    fn get_data(&self, key: &Key) -> Result<Vec<Datum<A>>, HistoryError> { self.get_data_proj(key) }

    fn get_continuations(&self, key: &Key) -> Result<Vec<WaitingContinuation<P, K>>, HistoryError> {
        self.get_continuations_proj(key)
    }

    fn get_joins(&self, key: &Key) -> Result<Vec<Vec<C>>, HistoryError> { self.get_joins_proj(key) }

    /**
     * Get reader which accepts non-serialized and hashed keys
     */
    fn base(&self) -> Box<dyn HistoryReaderBase<C, P, A, K>>;

    fn get_data_proj_generic(&self, key: &C) -> Vec<Datum<A>>;

    fn get_continuations_proj_generic(&self, key: &Vec<C>) -> Vec<WaitingContinuation<P, K>>;

    fn get_joins_proj_generic(&self, key: &C) -> Vec<Vec<C>>;
}

/**
 * History reader base, version of a reader which accepts non-serialized and
 * hashed keys
 */
pub trait HistoryReaderBase<C: Clone, P: Clone, A: Clone, K: Clone>: Sync + Send {
    fn get_data_proj(&self, key: &C) -> Result<Vec<Datum<A>>, RSpaceError>;

    fn get_continuations_proj(
        &self,
        key: &Vec<C>,
    ) -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError>;

    fn get_joins_proj(&self, key: &C) -> Result<Vec<Vec<C>>, RSpaceError>;

    /**                                                                      
     *                                                                      
     * Defaults
     *                                                                      
     *                                                                      
     *
     */
    fn get_data(&self, key: &C) -> Result<Vec<Datum<A>>, RSpaceError> { self.get_data_proj(key) }

    fn get_continuations(
        &self,
        key: &Vec<C>,
    ) -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError> {
        self.get_continuations_proj(key)
    }

    fn get_joins(&self, key: &C) -> Result<Vec<Vec<C>>, RSpaceError> { self.get_joins_proj(key) }
}

/// A history reader bound to one authenticated codec epoch for its lifetime.
/// All typed-key lookups use the same Route A key namespace as event creation
/// and fall closed before storage access if Legacy is given a cost-bearing key.
/// The underlying hash-keyed reader stays generic and retains its old API for
/// historical callers; activated sessions use this wrapper instead of `base`.
pub struct EpochHistoryReader<C: Clone, P: Clone, A: Clone, K: Clone> {
    reader: Box<dyn HistoryReader<Blake2b256Hash, C, P, A, K>>,
    epoch: EventCodecEpoch,
}

impl<C: Clone, P: Clone, A: Clone, K: Clone> EpochHistoryReader<C, P, A, K> {
    pub fn new(
        reader: Box<dyn HistoryReader<Blake2b256Hash, C, P, A, K>>,
        epoch: EventCodecEpoch,
    ) -> Self {
        Self { reader, epoch }
    }

    pub fn epoch(&self) -> EventCodecEpoch { self.epoch }

    pub fn root(&self) -> Blake2b256Hash { self.reader.root() }
}

impl<C, P, A, K> EpochHistoryReader<C, P, A, K>
where
    C: Clone + StableHashSerialize + ColdStoreDecode + Send + Sync + 'static,
    P: Clone + ColdStoreDecode + Send + Sync + 'static,
    A: Clone + ColdStoreDecode + Send + Sync + 'static,
    K: Clone + ColdStoreDecode + Send + Sync + 'static,
{
    fn decode_error(error: ColdStoreDecodeError) -> HistoryError {
        HistoryError::DecodeError(error.to_string())
    }

    pub fn get_data(&self, channel: &C) -> Result<Vec<Datum<A>>, RSpaceError> {
        let key = channel_hash_for_epoch(self.epoch, channel)?;
        self.get_data_by_hash(&key)
            .map_err(RSpaceError::HistoryError)
    }

    /// Decode a value at an event-carried key already authenticated as hashed
    /// under this reader's epoch. The caller must establish that provenance;
    /// this method cannot recover it from a hash. It does not hash again.
    pub fn get_data_by_hash(&self, key: &Blake2b256Hash) -> Result<Vec<Datum<A>>, HistoryError> {
        self.reader
            .get_data_proj_binary(key)?
            .iter()
            .map(|bytes| decode_datum_for_epoch(self.epoch, bytes).map_err(Self::decode_error))
            .collect()
    }

    pub fn get_continuations(
        &self,
        channels: &[C],
    ) -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError> {
        let key = channel_tuple_hash_for_epoch(self.epoch, channels)?;
        self.get_continuations_by_hash(&key)
            .map_err(RSpaceError::HistoryError)
    }

    /// Decode continuations at an already-authenticated tuple hash without
    /// changing the codec epoch selected for this reader.
    pub fn get_continuations_by_hash(
        &self,
        key: &Blake2b256Hash,
    ) -> Result<Vec<WaitingContinuation<P, K>>, HistoryError> {
        self.reader
            .get_continuations_proj_binary(key)?
            .iter()
            .map(|bytes| {
                decode_continuation_for_epoch(self.epoch, bytes).map_err(Self::decode_error)
            })
            .collect()
    }

    pub fn get_joins(&self, channel: &C) -> Result<Vec<Vec<C>>, RSpaceError> {
        let key = channel_hash_for_epoch(self.epoch, channel)?;
        self.get_joins_by_hash(&key)
            .map_err(RSpaceError::HistoryError)
    }

    /// Decode joins at an already-authenticated channel hash under the bound
    /// epoch.
    pub fn get_joins_by_hash(&self, key: &Blake2b256Hash) -> Result<Vec<Vec<C>>, HistoryError> {
        self.reader
            .get_joins_proj_binary(key)?
            .iter()
            .map(|bytes| {
                crate::rspace::serializers::cold_store_decode::cold_decode_vec_prefix_for_epoch::<C>(
                    self.epoch, bytes,
                )
                .map(|(join, _)| join)
                .map_err(Self::decode_error)
            })
            .collect()
    }

    pub fn base(self) -> Box<dyn HistoryReaderBase<C, P, A, K>> { Box::new(self) }
}

impl<C, P, A, K> HistoryReaderBase<C, P, A, K> for EpochHistoryReader<C, P, A, K>
where
    C: Clone + StableHashSerialize + ColdStoreDecode + Send + Sync + 'static,
    P: Clone + ColdStoreDecode + Send + Sync + 'static,
    A: Clone + ColdStoreDecode + Send + Sync + 'static,
    K: Clone + ColdStoreDecode + Send + Sync + 'static,
{
    fn get_data_proj(&self, key: &C) -> Result<Vec<Datum<A>>, RSpaceError> { self.get_data(key) }

    fn get_continuations_proj(
        &self,
        key: &Vec<C>,
    ) -> Result<Vec<WaitingContinuation<P, K>>, RSpaceError> {
        self.get_continuations(key)
    }

    fn get_joins_proj(&self, key: &C) -> Result<Vec<Vec<C>>, RSpaceError> { self.get_joins(key) }
}
