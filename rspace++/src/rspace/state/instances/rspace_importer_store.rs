use std::sync::Arc;

use shared::rust::ByteVector;
use shared::rust::store::key_value_store::KeyValueStore;

use crate::rspace::history::roots_store::{RootsStore, RootsStoreInstances};
use crate::rspace::shared::trie_exporter::{KeyHash, Value};
use crate::rspace::shared::trie_importer::TrieImporter;
use crate::rspace::state::rspace_importer::RSpaceImporter;

// See rspace/src/main/scala/coop/rchain/rspace/state/instances/
// RSpaceImporterStore.scala
pub struct RSpaceImporterStore;

impl RSpaceImporterStore {
    pub fn create(
        history_store: Arc<dyn KeyValueStore>,
        value_store: Arc<dyn KeyValueStore>,
        roots_store: Arc<dyn KeyValueStore>,
    ) -> impl RSpaceImporter {
        RSpaceImporterImpl {
            history_store,
            value_store,
            roots_store,
        }
    }
}

#[derive(Clone)]
pub struct RSpaceImporterImpl {
    pub history_store: Arc<dyn KeyValueStore>,
    pub value_store: Arc<dyn KeyValueStore>,
    pub roots_store: Arc<dyn KeyValueStore>,
}

impl RSpaceImporter for RSpaceImporterImpl {
    fn get_history_item(&self, hash: KeyHash) -> Option<ByteVector> {
        self.history_store
            .get(&vec![hash.bytes()])
            .expect("RSpace Importer: history store get failed")
            .into_iter()
            .next()
            .flatten()
    }
}

impl TrieImporter for RSpaceImporterImpl {
    fn set_history_items(&self, data: Vec<(KeyHash, Value)>) {
        tracing::info!(
            target: "f1r3.trace.lfs_import",
            "[TRACE-LFS-SET-HISTORY-ITEMS] count={}",
            data.len()
        );
        for (key_hash, _) in data.iter() {
            let key_hex = hex::encode(key_hash.bytes());
            tracing::info!(
                target: "f1r3.trace.lfs_import",
                "[TRACE-LFS-SET-HISTORY-ITEM] history_key={}",
                key_hex
            );
        }
        self.history_store
            .put(
                data.iter()
                    .map(|pair| (pair.0.bytes(), pair.1.clone()))
                    .collect(),
            )
            .expect("Rspace Importer: failed to put in history store");
    }

    fn set_data_items(&self, data: Vec<(KeyHash, Value)>) {
        tracing::info!(
            target: "f1r3.trace.lfs_import",
            "[TRACE-LFS-SET-DATA-ITEMS] count={}",
            data.len()
        );
        for (key_hash, value) in data.iter() {
            let key_hex = hex::encode(key_hash.bytes());
            tracing::info!(
                target: "f1r3.trace.lfs_import",
                "[TRACE-LFS-SET-DATA-ITEM] data_key={} value_bytes_len={}",
                key_hex, value.len()
            );
        }
        self.value_store
            .put(
                data.iter()
                    .map(|pair| (pair.0.bytes(), pair.1.clone()))
                    .collect(),
            )
            .expect("Rspace Importer: failed to put in value store")
    }

    fn set_root(&self, key: &KeyHash) {
        let key_hex = hex::encode(key.bytes());
        tracing::info!(
            target: "f1r3.trace.lfs_import",
            "[TRACE-LFS-SET-ROOT] root={}",
            key_hex
        );
        let roots = RootsStoreInstances::roots_store(self.roots_store.clone());
        roots
            .record_root(key)
            .expect("Rspace Importer: failed to record root")
    }
}
