use std::mem::size_of;
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
use crate::rspace::hashing::native_source::{SourceMeter, channels_hash, encode, hash, sort};
// Changed by D-C3 (D-S3, DR-97): the preparation matches on export views.
// use crate::rspace::hot_store_action::DeleteAction::{DeleteContinuations, DeleteData,
// DeleteJoins}; use crate::rspace::hot_store_action::HotStoreAction;
// use crate::rspace::hot_store_action::InsertAction::{InsertContinuations, InsertData,
// InsertJoins};
use crate::rspace::hot_store_action::{HotStoreAction, NativeExportAction, NativeExportView};

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

fn vector<T>(count: usize, meter: &impl SourceMeter) -> Result<Vec<T>, RSpaceError> {
    let backing = count
        .checked_mul(size_of::<T>())
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(count.checked_add(1).ok_or(RSpaceError::HostWorkRejected)?, 0, backing)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| RSpaceError::HostWorkRejected)?;
    Ok(values)
}

fn records<T: Serialize>(items: &[T], meter: &impl SourceMeter) -> Result<Vec<u8>, RSpaceError> {
    let mut encoded = vector::<Vec<u8>>(items.len(), meter)?;
    for item in items {
        encoded.push(encode(item, meter)?);
    }
    sort(&mut encoded, |bytes| bytes, meter)?;
    encode(&encoded, meter)
}

fn leaf_hash<T: Serialize>(
    leaf: &T,
    meter: &impl SourceMeter,
) -> Result<Blake2b256Hash, RSpaceError> {
    let bytes = encode(leaf, meter)?;
    meter.reserve(1, bytes.len(), 32)?;
    Ok(Blake2b256Hash::new(&bytes))
}

fn history_key(
    prefix: u8,
    hash: &Blake2b256Hash,
    meter: &impl SourceMeter,
) -> Result<Vec<u8>, RSpaceError> {
    let size = hash
        .0
        .len()
        .checked_add(1)
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(1, hash.0.len(), size)?;
    let mut key = Vec::new();
    key.try_reserve_exact(size)
        .map_err(|_| RSpaceError::HostWorkRejected)?;
    key.push(prefix);
    key.extend_from_slice(&hash.0);
    Ok(key)
}

fn reserve_cold_commit(
    actions: &[(Vec<u8>, Vec<u8>)],
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    let count = actions.len();
    let metadata = size_of::<Vec<u8>>()
        .checked_add(size_of::<bool>())
        .and_then(|bytes| bytes.checked_add(2 * size_of::<((Vec<u8>, Vec<u8>), bool)>()))
        .and_then(|bytes| bytes.checked_add(size_of::<(Vec<u8>, Vec<u8>)>()))
        .and_then(|bytes| bytes.checked_mul(count))
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(
        count
            .checked_mul(6)
            .and_then(|n| n.checked_add(1))
            .ok_or(RSpaceError::HostWorkRejected)?,
        0,
        metadata,
    )?;
    for (key, value) in actions {
        let key_bytes = key
            .len()
            .checked_mul(2)
            .ok_or(RSpaceError::HostWorkRejected)?;
        let backing = key_bytes
            .checked_add(value.len())
            .ok_or(RSpaceError::HostWorkRejected)?;
        let scanned = key
            .len()
            .checked_mul(4)
            .and_then(|bytes| {
                value
                    .len()
                    .checked_mul(2)
                    .and_then(|value| bytes.checked_add(value))
            })
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(5, scanned, backing)?;
    }
    Ok(())
}

impl NativeCheckpoint {
    // Changed by D-C3 (D-S3, DR-97): the owned actions are prepared through
    // their borrowed views; the body below moved into `prepare_views`.
    // pub(crate) fn prepare<C, P, A, K>(
    //     actions: Vec<HotStoreAction<C, P, A, K>>,
    //     meter: &dyn SourceMeter,
    // ) -> Result<Self, RSpaceError>
    // where
    //     C: Clone + Serialize,
    //     P: Clone + Serialize,
    //     A: Clone + Serialize,
    //     K: Clone + Serialize,
    // {
    //     let reserve = |operations, scanned, backing| meter.reserve(operations,
    // scanned, backing);     reserve.reserve(actions.len(), 0, 0)?;
    //     let inserts = actions
    //         .iter()
    //         .filter(|action| matches!(action, HotStoreAction::Insert(_)))
    //         .count();
    //     let mut cold_actions = vector::<(Vec<u8>, Vec<u8>)>(inserts, &reserve)?;
    //     let mut history_actions = vector::<HistoryAction>(actions.len(),
    // &reserve)?;     for action in actions {
    //         let (prefix, projection, leaf) = match action {
    //             HotStoreAction::Insert(InsertData(insert)) => {
    //                 let projection = hash(&insert.channel, &reserve)?;
    //                 let leaf = DataLeaf {
    //                     bytes: records(&insert.data, &reserve)?,
    //                 };
    //                 let hash = leaf_hash(&leaf, &reserve)?;
    //                 (PREFIX_DATUM, projection, Some((hash,
    // PersistedData::Data(leaf))))             }
    //             HotStoreAction::Insert(InsertContinuations(insert)) => {
    //                 let projection = channels_hash(&insert.channels, &reserve)?;
    //                 let leaf = ContinuationsLeaf {
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
    //             return Err(RSpaceError::HistoryError(HistoryError::ActionError(
    //                 "Cannot process duplicate actions on one key.".to_owned(),
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
    /// preparation, with the same reservations as before.
    pub(crate) fn prepare<C, P, A, K>(
        actions: Vec<HotStoreAction<C, P, A, K>>,
        meter: &dyn SourceMeter,
    ) -> Result<Self, RSpaceError>
    where
        C: Clone + Serialize,
        P: Clone + Serialize,
        A: Clone + Serialize,
        K: Clone + Serialize,
    {
        Self::prepare_views(&actions, meter)
    }

    /// D-C3 (D-S3, DR-97): the checkpoint of the native export's changes,
    /// borrowed from the native store.
    pub(crate) fn prepare_borrowed<C, A, W>(
        actions: &[NativeExportAction<'_, C, A, W>],
        meter: &dyn SourceMeter,
    ) -> Result<Self, RSpaceError>
    where
        C: Serialize,
        A: Clone + Serialize,
        W: Serialize,
    {
        Self::prepare_views(actions, meter)
    }

    fn prepare_views<C, A, W, T>(
        actions: &[T],
        meter: &dyn SourceMeter,
    ) -> Result<Self, RSpaceError>
    where
        C: Serialize,
        A: Clone + Serialize,
        W: Serialize,
        T: NativeExportView<C, A, W>,
    {
        let reserve = |operations, scanned, backing| meter.reserve(operations, scanned, backing);
        reserve.reserve(actions.len(), 0, 0)?;
        let inserts = actions
            .iter()
            .filter(|action| action.view().is_insert())
            .count();
        let mut cold_actions = vector::<(Vec<u8>, Vec<u8>)>(inserts, &reserve)?;
        let mut history_actions = vector::<HistoryAction>(actions.len(), &reserve)?;
        for action in actions {
            let (prefix, projection, leaf) = match action.view() {
                NativeExportAction::InsertData { channel, data } => {
                    let projection = hash(channel, &reserve)?;
                    let leaf = DataLeaf {
                        bytes: records(data, &reserve)?,
                    };
                    let hash = leaf_hash(&leaf, &reserve)?;
                    (PREFIX_DATUM, projection, Some((hash, PersistedData::Data(leaf))))
                }
                NativeExportAction::InsertContinuations {
                    channels,
                    continuations,
                } => {
                    let projection = channels_hash(channels, &reserve)?;
                    let leaf = ContinuationsLeaf {
                        bytes: records(continuations, &reserve)?,
                    };
                    let hash = leaf_hash(&leaf, &reserve)?;
                    (PREFIX_KONT, projection, Some((hash, PersistedData::Continuations(leaf))))
                }
                NativeExportAction::InsertJoins { channel, joins } => {
                    let projection = hash(channel, &reserve)?;
                    let leaf = JoinsLeaf {
                        bytes: records(joins, &reserve)?,
                    };
                    let hash = leaf_hash(&leaf, &reserve)?;
                    (PREFIX_JOINS, projection, Some((hash, PersistedData::Joins(leaf))))
                }
                NativeExportAction::DeleteData { channel } => {
                    (PREFIX_DATUM, hash(channel, &reserve)?, None)
                }
                NativeExportAction::DeleteContinuations { channels } => {
                    (PREFIX_KONT, channels_hash(channels, &reserve)?, None)
                }
                NativeExportAction::DeleteJoins { channel } => {
                    (PREFIX_JOINS, hash(channel, &reserve)?, None)
                }
            };
            let key = history_key(prefix, &projection, &reserve)?;
            if let Some((hash, data)) = leaf {
                cold_actions.push((encode(&hash, &reserve)?, encode(&data, &reserve)?));
                history_actions.push(HistoryAction::Insert(InsertAction { key, hash }));
            } else {
                history_actions.push(HistoryAction::Delete(DeleteAction { key }));
            }
        }
        let mut keys = vector::<&[u8]>(history_actions.len(), &reserve)?;
        for action in &history_actions {
            keys.push(match action {
                HistoryAction::Insert(insert) => &insert.key,
                HistoryAction::Delete(delete) => &delete.key,
            });
        }
        sort(&mut keys, |key| key, &reserve)?;
        for adjacent in keys.windows(2) {
            reserve.reserve(1, adjacent[0].len().min(adjacent[1].len()), 0)?;
            if adjacent[0] == adjacent[1] {
                return Err(RSpaceError::HistoryError(HistoryError::ActionError(
                    "Cannot process duplicate actions on one key.".to_owned(),
                )));
            }
        }
        reserve_cold_commit(&cold_actions, &reserve)?;
        Ok(Self {
            cold_actions,
            history_actions,
            prepared_history: None,
            root: None,
            backing: None,
        })
    }
}
