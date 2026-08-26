use std::sync::{Arc, Mutex};

use shared::rust::{Byte, ByteVector};

use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::cold_store::ColdLeafAddress;
use crate::rspace::shared::trie_exporter::StateExportError;
use crate::rspace::state::rspace_exporter::RSpaceExporter;

//See rspace/src/main/scala/coop/rchain/rspace/state/exporters/
// RSpaceExporterItems.scala
pub struct StoreItems<KeyHash, Value> {
    pub items: Vec<(KeyHash, Value)>,
    pub last_path: Vec<(KeyHash, Option<Byte>)>,
}

pub struct RSpaceExporterItems;

impl RSpaceExporterItems {
    pub fn get_history(
        &self,
        exporter: Arc<Mutex<Box<dyn RSpaceExporter>>>,
        start_path: Vec<(Blake2b256Hash, Option<Byte>)>,
        skip: i32,
        take: i32,
    ) -> Result<StoreItems<Blake2b256Hash, ByteVector>, StateExportError> {
        let exporter = exporter
            .lock()
            .map_err(|_| StateExportError::InvalidPath("exporter lock is poisoned".to_string()))?;

        let page = exporter.get_page(start_path, skip, take)?;

        let history_keys: Vec<_> = page.nodes.iter().filter(|node| !node.is_leaf).collect();
        let keys: Vec<Blake2b256Hash> = history_keys
            .iter()
            .map(|key| key.hash.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let items = exporter.get_history_items(keys)?;

        Ok(StoreItems {
            items,
            last_path: page.next_path,
        })
    }

    pub fn get_data(
        &self,
        exporter: Arc<Mutex<Box<dyn RSpaceExporter>>>,
        start_path: Vec<(Blake2b256Hash, Option<Byte>)>,
        skip: i32,
        take: i32,
    ) -> Result<StoreItems<Blake2b256Hash, ByteVector>, StateExportError> {
        let exporter = exporter
            .lock()
            .map_err(|_| StateExportError::InvalidPath("exporter lock is poisoned".to_string()))?;

        let page = exporter.get_page(start_path, skip, take)?;

        let data_keys: Vec<_> = page.nodes.iter().filter(|node| node.is_leaf).collect();
        let addresses: Vec<ColdLeafAddress> = data_keys
            .iter()
            .map(|node| {
                node.leaf_address
                    .clone()
                    .ok_or(StateExportError::MissingLeafAddress)
            })
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?
            .into_iter()
            .collect();
        let items = exporter.get_data_items(addresses)?;

        Ok(StoreItems {
            items,
            last_path: page.next_path,
        })
    }

    pub fn get_history_and_data(
        exporter: Arc<dyn RSpaceExporter>,
        start_path: Vec<(Blake2b256Hash, Option<Byte>)>,
        skip: i32,
        take: i32,
    ) -> Result<
        (StoreItems<Blake2b256Hash, ByteVector>, StoreItems<Blake2b256Hash, ByteVector>),
        StateExportError,
    > {
        // Traverse and collect tuple space nodes
        let page = exporter.get_page(start_path, skip, take)?;

        // Split by leaf nodes
        let (leafs, non_leafs): (Vec<_>, Vec<_>) = page.nodes.iter().partition(|node| node.is_leaf);
        let history_keys: Vec<Blake2b256Hash> = non_leafs
            .iter()
            .map(|key| key.hash.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let data_addresses: Vec<ColdLeafAddress> = leafs
            .iter()
            .map(|node| {
                node.leaf_address
                    .clone()
                    .ok_or(StateExportError::MissingLeafAddress)
            })
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?
            .into_iter()
            .collect();

        // Load all history and data items
        let history_items = exporter.get_history_items(history_keys)?;
        let data_items = exporter.get_data_items(data_addresses)?;

        // println!("\ndata_items in exporter_items: {:?}", data_items);

        let history = StoreItems {
            items: history_items,
            last_path: page.next_path.clone(),
        };

        let data = StoreItems {
            items: data_items,
            last_path: page.next_path,
        };

        Ok((history, data))
    }
}
