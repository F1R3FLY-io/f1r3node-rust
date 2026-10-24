use std::sync::Arc;

use shared::rust::{Byte, ByteVector};

use crate::rspace::errors::RadixTreeError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::state::rspace_exporter::RSpaceExporter;

//See rspace/src/main/scala/coop/rchain/rspace/state/exporters/
// RSpaceExporterItems.scala
pub struct StoreItems<KeyHash, Value> {
    pub items: Vec<(KeyHash, Value)>,
    pub last_path: Vec<(KeyHash, Option<Byte>)>,
}

pub struct RSpaceExporterItems;

impl RSpaceExporterItems {
    pub fn get_history_and_data(
        exporter: Arc<dyn RSpaceExporter>,
        start_path: Vec<(Blake2b256Hash, Option<Byte>)>,
        skip: i32,
        take: i32,
    ) -> Result<
        (StoreItems<Blake2b256Hash, ByteVector>, StoreItems<Blake2b256Hash, ByteVector>),
        RadixTreeError,
    > {
        // Traverse and collect tuple space nodes
        let nodes = exporter.get_nodes(start_path.clone(), skip, take)?;

        let Some(last_entry) = nodes.last() else {
            return Err(RadixTreeError::KeyNotFound(
                "RSpace Exporter: empty history for the requested start path".to_string(),
            ));
        };

        // Split by leaf nodes
        let (leafs, non_leafs): (Vec<_>, Vec<_>) = nodes.iter().partition(|node| node.is_leaf);
        let history_keys: Vec<Blake2b256Hash> = non_leafs
            .iter()
            .map(|key| key.hash.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        let data_keys: Vec<Blake2b256Hash> = leafs
            .iter()
            .map(|key| key.hash.clone())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        // Load all history and data items
        let history_items = exporter
            .get_history_items(history_keys)
            .map_err(RadixTreeError::KvStoreError)?;
        let data_items = exporter
            .get_data_items(data_keys)
            .map_err(RadixTreeError::KvStoreError)?;

        let last_path = if last_entry.is_leaf {
            start_path
        } else {
            let mut path = last_entry.path.clone();
            path.push((last_entry.hash.clone(), None));
            path
        };
        let history = StoreItems {
            items: history_items,
            last_path: last_path.clone(),
        };

        let data = StoreItems {
            items: data_items,
            last_path,
        };

        Ok((history, data))
    }
}
