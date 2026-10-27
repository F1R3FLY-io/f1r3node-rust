use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::Arc;

use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use shared::rust::ByteVector;

use crate::rspace::errors::RadixTreeError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::cold_store::PersistedData;
use crate::rspace::history::history_repository::{PREFIX_DATUM, PREFIX_JOINS, PREFIX_KONT};
use crate::rspace::shared::trie_exporter::KeyHash;
use crate::rspace::shared::trie_importer::TrieImporter;
use crate::rspace::state::exporters::rspace_exporter_items::RSpaceExporterItems;
use crate::rspace::state::rspace_exporter::RSpaceExporterInstance;

// See rspace/src/main/scala/coop/rchain/rspace/state/RSpaceImporter.scala
pub trait RSpaceImporter: TrieImporter + Send + Sync {
    fn get_history_item(&self, hash: KeyHash) -> Option<ByteVector>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum StateItemsError {
    InvalidHistorySize { expected: i32, received: i32 },
    HistoryHashMismatch { key: String, decoded: String },
    Traversal(RadixTreeError),
    HistoryItemsCorrupted,
    DataItemsCorrupted,
    InvalidDataItem(String),
    LastPathMismatch,
}

impl std::fmt::Display for StateItemsError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            StateItemsError::InvalidHistorySize { expected, received } => write!(
                f,
                "RSpace Importer: Input size of history items is not valid. Expected chunk size \
                 {}, received {}.",
                expected, received
            ),
            StateItemsError::HistoryHashMismatch { key, decoded } => write!(
                f,
                "RSpace Importer: Trie hash does not match decoded trie, key: {}, decoded: {}",
                key, decoded
            ),
            StateItemsError::Traversal(err) => {
                write!(f, "RSpace Importer: Unable to traverse received history: {}", err)
            }
            StateItemsError::HistoryItemsCorrupted => {
                write!(f, "RSpace Importer: History items are corrupted")
            }
            StateItemsError::DataItemsCorrupted => {
                write!(f, "RSpace Importer: Data items are corrupted")
            }
            StateItemsError::InvalidDataItem(err) => {
                write!(f, "RSpace Importer: Unable to validate data items hashes: {}", err)
            }
            StateItemsError::LastPathMismatch => write!(
                f,
                "RSpace Importer: Last path does not match the traversal of received history"
            ),
        }
    }
}

pub struct RSpaceImporterInstance;

impl RSpaceImporterInstance {
    pub fn validate_state_items(
        history_items: &[(Blake2b256Hash, ByteVector)],
        data_items: &[(Blake2b256Hash, ByteVector)],
        start_path: Vec<(Blake2b256Hash, Option<u8>)>,
        last_path: &[(Blake2b256Hash, Option<u8>)],
        chunk_size: i32,
        skip: i32,
        get_from_history: Arc<dyn RSpaceImporter>,
    ) -> Result<(), StateItemsError> {
        let received_history_size = history_items.len() as i32;
        let is_end = || received_history_size < chunk_size;

        // Validate history items size
        let validate_history_size = || {
            let size_is_valid = || received_history_size == chunk_size || is_end();
            if size_is_valid() {
                Ok(())
            } else {
                Err(StateItemsError::InvalidHistorySize {
                    expected: chunk_size,
                    received: received_history_size,
                })
            }
        };

        // Validate history hashes
        let get_and_validate_history_items = || {
            let mut validated_items = Vec::new();
            for (hash, trie_bytes) in history_items {
                let trie_hash = Blake2b256Hash::new(trie_bytes);
                if *hash == trie_hash {
                    validated_items.push((trie_hash.bytes(), trie_bytes.clone()));
                } else {
                    return Err(StateItemsError::HistoryHashMismatch {
                        key: hex::encode(hash.bytes()),
                        decoded: hex::encode(trie_hash.bytes()),
                    });
                }
            }
            Ok(validated_items)
        };

        // Validate chunk size.
        validate_history_size()?;

        // Validate tries from received history items.
        let trie_map: HashMap<ByteVector, ByteVector> =
            get_and_validate_history_items()?.into_iter().collect();

        let get_node_function: Arc<dyn Fn(&ByteVector) -> Option<ByteVector>> =
            Arc::new(move |hash: &ByteVector| {
                trie_map.get(hash).cloned().or_else(|| {
                    get_from_history.get_history_item(Blake2b256Hash::from_bytes(hash.to_vec()))
                })
            });
        // Traverse trie and extract nodes / the same as in export. Nodes must match
        // hashed keys.
        let (nodes, leaf_keys) = RSpaceExporterInstance::try_traverse_history_with_leaf_keys(
            start_path,
            skip,
            chunk_size,
            get_node_function,
        )
        .map_err(StateItemsError::Traversal)?;

        if RSpaceExporterItems::last_path(&nodes).as_deref() != Some(last_path) {
            return Err(StateItemsError::LastPathMismatch);
        }

        // Extract history and data keys.
        let (leafs, non_leafs): (Vec<_>, Vec<_>) = nodes.into_iter().partition(|node| node.is_leaf);
        let history_keys: Vec<Blake2b256Hash> = non_leafs
            .into_iter()
            .map(|non_leaf| non_leaf.hash)
            .collect();
        let data_keys: Vec<Blake2b256Hash> = leafs.into_iter().map(|leaf| leaf.hash).collect();

        // Validate keys / cryptographic proof that store chunk is not corrupted or
        // modified.
        let history_item_keys: Vec<Blake2b256Hash> =
            history_items.iter().map(|item| item.0.clone()).collect();
        // TODO: This might need to check ordering too
        if !check_same_elements(history_item_keys, history_keys) {
            return Err(StateItemsError::HistoryItemsCorrupted);
        }

        let data_item_keys: Vec<Blake2b256Hash> =
            data_items.iter().map(|item| item.0.clone()).collect();
        // TODO: This might need to check ordering too
        if !check_same_elements(data_item_keys, data_keys) {
            return Err(StateItemsError::DataItemsCorrupted);
        }

        let mut leaf_kinds: HashMap<Blake2b256Hash, HashSet<Option<u8>>> = HashMap::new();
        for (hash, key) in leaf_keys {
            leaf_kinds.entry(hash).or_default().insert(key.first().copied());
        }

        // Validate data hashes
        data_items
            .par_iter()
            .try_for_each(|(hash, value_bytes)| {
                let persisted_data: PersistedData =
                    bincode::deserialize(value_bytes).map_err(|e| {
                        format!(
                            "Data item does not decode, key: {}: {}",
                            hex::encode(hash.bytes()),
                            e
                        )
                    })?;
                let (kind, bytes) = match persisted_data {
                    PersistedData::Joins(ref leaf) => (PREFIX_JOINS, bincode::serialize(leaf)),
                    PersistedData::Data(ref leaf) => (PREFIX_DATUM, bincode::serialize(leaf)),
                    PersistedData::Continuations(ref leaf) => {
                        (PREFIX_KONT, bincode::serialize(leaf))
                    }
                };
                let bytes = bytes.map_err(|e| e.to_string())?;
                let data_hash = Blake2b256Hash::new(&bytes);
                if *hash != data_hash {
                    return Err(format!(
                        "Data hash does not match decoded data, key: {}, decoded: {}.",
                        hex::encode(hash.bytes()),
                        hex::encode(data_hash.bytes())
                    ));
                }
                if leaf_kinds.get(hash) != Some(&HashSet::from([Some(kind)])) {
                    return Err(format!(
                        "Data item kind does not match its trie key prefix, key: {}.",
                        hex::encode(hash.bytes())
                    ));
                }
                Ok(())
            })
            .map_err(StateItemsError::InvalidDataItem)
    }
}

fn check_same_elements<T: Hash + Eq>(vec1: Vec<T>, vec2: Vec<T>) -> bool {
    let set1: HashSet<_> = vec1.into_iter().collect();
    let set2: HashSet<_> = vec2.into_iter().collect();
    set1 == set2
}
