use std::borrow::Borrow;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use shared::rust::store::key_value_store::KeyValueStore;

use super::cold_store::{ContinuationsLeaf, DataLeaf, JoinsLeaf, PersistedData};
use super::history::{History, NativePreparedHistory};
use super::history_action::{DeleteAction, HistoryAction, InsertAction};
use super::history_repository::{PREFIX_DATUM, PREFIX_JOINS, PREFIX_KONT};
use super::root_repository::RootRepository;
use crate::rspace::errors::{HistoryError, RSpaceError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::native_source::sort;
use crate::rspace::hashing::stable_hash_provider::{
    EventCodecEpoch, StableHashSerialize, channel_hash_for_epoch, channel_tuple_hash_for_epoch,
    event_leg_bytes_for_epoch,
};
// Changed by D-C3 (D-S3, DR-97): the preparation matches on export views.
// use crate::rspace::hot_store_action::DeleteAction::{DeleteContinuations, DeleteData,
// DeleteJoins}; use crate::rspace::hot_store_action::HotStoreAction;
// use crate::rspace::hot_store_action::InsertAction::{InsertContinuations, InsertData,
// InsertJoins};
use crate::rspace::hot_store_action::{HotStoreAction, NativeExportAction, NativeExportView};
use crate::rspace::internal::{Datum, WaitingContinuation};

pub struct NativeCheckpoint {
    pub(crate) cold_actions: Vec<(Vec<u8>, Vec<u8>)>,
    pub(crate) history_actions: Vec<HistoryAction>,
    pub(crate) prepared_history: Option<NativePreparedHistory>,
    pub(crate) root: Option<Blake2b256Hash>,
    pub(crate) backing: Option<NativeCheckpointBacking>,
}

pub(crate) struct NativeCheckpointBacking {
    pub(crate) history: Arc<Mutex<Box<dyn History>>>,
    pub(crate) roots: Arc<Mutex<RootRepository>>,
    pub(crate) leaves: Arc<dyn KeyValueStore>,
    pub(crate) nodes: Arc<dyn KeyValueStore>,
}

/// Changed by DR-131 S6: the bincode encoding of `value`, as
/// `bincode::serialize` gives it. An encoding error keeps the interpreter
/// class that the encoder gave it before host work was removed.
fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, RSpaceError> {
    bincode::serialize(value).map_err(|error| RSpaceError::InterpreterError(error.to_string()))
}

/// Changed by DR-131 S6: the Blake2b-256 digest of the bincode encoding of
/// `value`, as `stable_hash_provider::hash` computes it.
fn digest<T: Serialize + ?Sized>(value: &T) -> Result<Blake2b256Hash, RSpaceError> {
    Ok(Blake2b256Hash::new(&encode(value)?))
}

fn records<T>(
    items: &[T],
    encode_one: impl Fn(&T) -> Result<Vec<u8>, RSpaceError>,
) -> Result<Vec<u8>, RSpaceError> {
    let mut encoded = Vec::with_capacity(items.len());
    for item in items {
        encoded.push(encode_one(item)?);
    }
    sort(&mut encoded, |bytes| bytes);
    encode(&encoded)
}

fn datum_record<A: Clone + StableHashSerialize>(
    epoch: EventCodecEpoch,
    datum: &Datum<A>,
) -> Result<Vec<u8>, RSpaceError> {
    let mut bytes = event_leg_bytes_for_epoch(epoch, datum.a.as_ref())?;
    bytes.extend_from_slice(&encode(&(datum.persist, &datum.source))?);
    Ok(bytes)
}

fn continuation_record<P: Clone + StableHashSerialize, K: Clone + StableHashSerialize>(
    epoch: EventCodecEpoch,
    waiting: &WaitingContinuation<P, K>,
) -> Result<Vec<u8>, RSpaceError> {
    let count = u64::try_from(waiting.patterns.len())
        .map_err(|_| RSpaceError::InterpreterError("record length does not fit u64".into()))?;
    let mut bytes = encode(&count)?;
    for pattern in waiting.patterns.iter() {
        bytes.extend_from_slice(&event_leg_bytes_for_epoch(epoch, pattern)?);
    }
    bytes.extend_from_slice(&event_leg_bytes_for_epoch(epoch, waiting.continuation.as_ref())?);
    bytes.extend_from_slice(&encode(&(waiting.persist, &waiting.peeks, &waiting.source))?);
    Ok(bytes)
}

fn join_record<C: StableHashSerialize>(
    epoch: EventCodecEpoch,
    join: &Vec<C>,
) -> Result<Vec<u8>, RSpaceError> {
    let count = u64::try_from(join.len())
        .map_err(|_| RSpaceError::InterpreterError("record length does not fit u64".into()))?;
    let mut bytes = encode(&count)?;
    for channel in join {
        bytes.extend_from_slice(&event_leg_bytes_for_epoch(epoch, channel)?);
    }
    Ok(bytes)
}

fn history_key(prefix: u8, hash: &Blake2b256Hash) -> Vec<u8> {
    let mut key = Vec::with_capacity(hash.0.len() + 1);
    key.push(prefix);
    key.extend_from_slice(&hash.0);
    key
}

impl NativeCheckpoint {
    // Changed by D-C3 (D-S3, DR-97): the owned actions are prepared through
    // their borrowed views; the body below moved into `prepare_views`.
    // pub(crate) fn prepare<C, P, A, K>(
    //     actions: Vec<HotStoreAction<C, P, A, K>>,
    //     meter: &dyn SourceMeter,
    // ) -> Result<Self, RSpaceError>
    // where
    //     C: Clone + StableHashSerialize,
    //     P: Clone + StableHashSerialize,
    //     A: Clone + StableHashSerialize,
    //     K: Clone + StableHashSerialize,
    // {
    //     let reserve = |operations, scanned, backing|
    // meter.reserve(operations, scanned, backing);
    // reserve.reserve(actions.len(), 0, 0)?;     let inserts = actions
    //         .iter()
    //         .filter(|action| matches!(action, HotStoreAction::Insert(_)))
    //         .count();
    //     let mut cold_actions = vector::<(Vec<u8>, Vec<u8>)>(inserts,
    // &reserve)?;     let mut history_actions =
    // vector::<HistoryAction>(actions.len(), &reserve)?;     for action in
    // actions {         let (prefix, projection, leaf) = match action {
    //             HotStoreAction::Insert(InsertData(insert)) => {
    //                 let projection = hash(&insert.channel, &reserve)?;
    //                 let leaf = DataLeaf {
    //                     bytes: records(&insert.data, &reserve)?,
    //                 };
    //                 let hash = leaf_hash(&leaf, &reserve)?;
    //                 (PREFIX_DATUM, projection, Some((hash,
    // PersistedData::Data(leaf))))             }
    //             HotStoreAction::Insert(InsertContinuations(insert)) => {
    //                 let projection = channels_hash(&insert.channels,
    // &reserve)?;                 let leaf = ContinuationsLeaf {
    //                     bytes: records(&insert.continuations, &reserve)?,
    //                 };
    //                 let hash = leaf_hash(&leaf, &reserve)?;
    //                 (PREFIX_KONT, projection, Some((hash,
    // PersistedData::Continuations(leaf))))             }
    //             HotStoreAction::Insert(InsertJoins(insert)) => {
    //                 let projection = hash(&insert.channel, &reserve)?;
    //                 let leaf = JoinsLeaf {
    //                     bytes: records(&insert.joins, &reserve)?,
    //                 };
    //                 let hash = leaf_hash(&leaf, &reserve)?;
    //                 (PREFIX_JOINS, projection, Some((hash,
    // PersistedData::Joins(leaf))))             }
    //             HotStoreAction::Delete(DeleteData(delete)) => {
    //                 (PREFIX_DATUM, hash(&delete.channel, &reserve)?, None)
    //             }
    //             HotStoreAction::Delete(DeleteContinuations(delete)) => {
    //                 (PREFIX_KONT, channels_hash(&delete.channels, &reserve)?,
    // None)             }
    //             HotStoreAction::Delete(DeleteJoins(delete)) => {
    //                 (PREFIX_JOINS, hash(&delete.channel, &reserve)?, None)
    //             }
    //         };
    //         let key = history_key(prefix, &projection, &reserve)?;
    //         if let Some((hash, data)) = leaf {
    //             cold_actions.push((encode(&hash, &reserve)?, encode(&data,
    // &reserve)?));
    // history_actions.push(HistoryAction::Insert(InsertAction { key, hash }));
    //         } else {
    //             history_actions.push(HistoryAction::Delete(DeleteAction { key
    // }));         }
    //     }
    //     let mut keys = vector::<&[u8]>(history_actions.len(), &reserve)?;
    //     for action in &history_actions {
    //         keys.push(match action {
    //             HistoryAction::Insert(insert) => &insert.key,
    //             HistoryAction::Delete(delete) => &delete.key,
    //         });
    //     }
    //     sort(&mut keys, |key| key, &reserve)?;
    //     for adjacent in keys.windows(2) {
    //         reserve.reserve(1, adjacent[0].len().min(adjacent[1].len()), 0)?;
    //         if adjacent[0] == adjacent[1] {
    //             return
    // Err(RSpaceError::HistoryError(HistoryError::ActionError(
    // "Cannot process duplicate actions on one key.".to_owned(),
    //             )));
    //         }
    //     }
    //     reserve_cold_commit(&cold_actions, &reserve)?;
    //     Ok(Self {
    //         cold_actions,
    //         history_actions,
    //         prepared_history: None,
    //         root: None,
    //         backing: None,
    //     })
    // }

    /// The checkpoint of owned actions. D-C3 (D-S3, DR-97): they are read
    /// through their views, so the owned and the borrowed exports share one
    /// preparation.
    pub(crate) fn prepare<C, P, A, K>(
        actions: Vec<HotStoreAction<C, P, A, K>>,
    ) -> Result<Self, RSpaceError>
    where
        C: Clone + Serialize + StableHashSerialize,
        P: Clone + Serialize + StableHashSerialize,
        A: Clone + Serialize + StableHashSerialize,
        K: Clone + Serialize + StableHashSerialize,
    {
        Self::prepare_views(EventCodecEpoch::Legacy, &actions)
    }

    /// D-C3 (D-S3, DR-97): the checkpoint of the native export's changes,
    /// borrowed from the native store.
    pub(crate) fn prepare_borrowed<C, P, A, K>(
        epoch: EventCodecEpoch,
        actions: &[NativeExportAction<'_, C, A, Arc<WaitingContinuation<P, K>>>],
    ) -> Result<Self, RSpaceError>
    where
        C: StableHashSerialize,
        P: Clone + StableHashSerialize,
        A: Clone + StableHashSerialize,
        K: Clone + StableHashSerialize,
    {
        Self::prepare_views(epoch, actions)
    }

    fn prepare_views<C, P, A, K, W, T>(
        epoch: EventCodecEpoch,
        actions: &[T],
    ) -> Result<Self, RSpaceError>
    where
        C: StableHashSerialize,
        P: Clone + StableHashSerialize,
        A: Clone + StableHashSerialize,
        K: Clone + StableHashSerialize,
        W: Borrow<WaitingContinuation<P, K>>,
        T: NativeExportView<C, A, W>,
    {
        let inserts = actions
            .iter()
            .filter(|action| action.view().is_insert())
            .count();
        let mut cold_actions = Vec::with_capacity(inserts);
        let mut history_actions = Vec::with_capacity(actions.len());
        for action in actions {
            let (prefix, projection, leaf) = match action.view() {
                NativeExportAction::InsertData { channel, data } => {
                    let projection = channel_hash_for_epoch(epoch, channel)?;
                    let leaf = DataLeaf {
                        bytes: records(data, |datum| datum_record(epoch, datum))?,
                    };
                    let hash = digest(&leaf)?;
                    (PREFIX_DATUM, projection, Some((hash, PersistedData::Data(leaf))))
                }
                NativeExportAction::InsertContinuations {
                    channels,
                    continuations,
                } => {
                    let projection = channel_tuple_hash_for_epoch(epoch, channels)?;
                    let leaf = ContinuationsLeaf {
                        bytes: records(continuations, |waiting| {
                            continuation_record(epoch, waiting.borrow())
                        })?,
                    };
                    let hash = digest(&leaf)?;
                    (PREFIX_KONT, projection, Some((hash, PersistedData::Continuations(leaf))))
                }
                NativeExportAction::InsertJoins { channel, joins } => {
                    let projection = channel_hash_for_epoch(epoch, channel)?;
                    let leaf = JoinsLeaf {
                        bytes: records(joins, |join| join_record(epoch, join))?,
                    };
                    let hash = digest(&leaf)?;
                    (PREFIX_JOINS, projection, Some((hash, PersistedData::Joins(leaf))))
                }
                NativeExportAction::DeleteData { channel } => {
                    (PREFIX_DATUM, channel_hash_for_epoch(epoch, channel)?, None)
                }
                NativeExportAction::DeleteContinuations { channels } => {
                    (PREFIX_KONT, channel_tuple_hash_for_epoch(epoch, channels)?, None)
                }
                NativeExportAction::DeleteJoins { channel } => {
                    (PREFIX_JOINS, channel_hash_for_epoch(epoch, channel)?, None)
                }
            };
            let key = history_key(prefix, &projection);
            if let Some((hash, data)) = leaf {
                cold_actions.push((encode(&hash)?, encode(&data)?));
                history_actions.push(HistoryAction::Insert(InsertAction { key, hash }));
            } else {
                history_actions.push(HistoryAction::Delete(DeleteAction { key }));
            }
        }
        let mut keys: Vec<&[u8]> = Vec::with_capacity(history_actions.len());
        for action in &history_actions {
            keys.push(match action {
                HistoryAction::Insert(insert) => &insert.key,
                HistoryAction::Delete(delete) => &delete.key,
            });
        }
        sort(&mut keys, |key| key);
        for adjacent in keys.windows(2) {
            if adjacent[0] == adjacent[1] {
                return Err(RSpaceError::HistoryError(HistoryError::ActionError(
                    "Cannot process duplicate actions on one key.".to_owned(),
                )));
            }
        }
        Ok(Self {
            cold_actions,
            history_actions,
            prepared_history: None,
            root: None,
            backing: None,
        })
    }
}

// Added by the merge of feature/cost-accounted-rho (plan step 1.4): HEAD's
// epoch test without its host-work meter (DR-131). It is the only test that a
// Legacy epoch refuses a cost channel before any action is staged.
#[cfg(test)]
mod epoch_tests {
    use super::*;

    #[derive(Clone, Serialize)]
    struct CostChannel(u64);

    impl StableHashSerialize for CostChannel {
        fn stable_hash_contains_cost(&self) -> Result<bool, RSpaceError> { Ok(true) }
    }

    #[test]
    fn borrowed_preparation_rejects_legacy_cost_before_staging() {
        let channel = CostChannel(17);
        let actions: [NativeExportAction<
            '_,
            CostChannel,
            String,
            Arc<WaitingContinuation<String, String>>,
        >; 1] = [NativeExportAction::DeleteData { channel: &channel }];
        assert!(NativeCheckpoint::prepare_borrowed(EventCodecEpoch::Legacy, &actions).is_err());

        let prepared = NativeCheckpoint::prepare_borrowed(EventCodecEpoch::CostV1, &actions)
            .expect("CostV1 prepares a cost channel");
        let digest = channel_hash_for_epoch(EventCodecEpoch::CostV1, &channel)
            .expect("CostV1 hashes a cost channel");
        let expected = history_key(PREFIX_DATUM, &digest);
        assert!(
            matches!(&prepared.history_actions[0], HistoryAction::Delete(delete) if delete.key == expected)
        );
        assert!(prepared.cold_actions.is_empty());
    }
}
