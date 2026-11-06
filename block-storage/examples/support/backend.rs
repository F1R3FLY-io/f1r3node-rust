use std::path::PathBuf;
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, DagLiveResidentDiagnostics, DagResidentDiagnostics,
    KeyValueDagRepresentation,
};
use models::rust::block_hash::BlockHash;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::atomic_store::AtomicStoreGroup;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};

use super::counting_fixture::manager;

pub async fn open(
    manager: &mut impl KeyValueStoreManager,
    window: i64,
) -> Result<BlockDagKeyValueStorage, KvStoreError> {
    BlockDagKeyValueStorage::new_with_hot_window(manager, window).await
}
pub fn resident(dag: &KeyValueDagRepresentation) -> (usize, Option<i64>, usize) {
    let (height, edges) = dag.resident_bounds();
    (dag.resident_counts().blocks, height, edges)
}
pub fn diagnostics(dag: &KeyValueDagRepresentation) -> DagResidentDiagnostics {
    dag.resident_diagnostics()
}
pub fn live_diagnostics(storage: &BlockDagKeyValueStorage) -> Option<DagLiveResidentDiagnostics> {
    Some(storage.live_resident_diagnostics())
}
pub fn visible(dag: &KeyValueDagRepresentation, hash: &BlockHash) -> Result<bool, KvStoreError> {
    dag.contains(hash)
}
pub fn children(
    dag: &KeyValueDagRepresentation,
    hash: &BlockHash,
) -> Result<Option<imbl::HashSet<BlockHash>>, KvStoreError> {
    dag.children(hash)
}
pub const WINDOWED: bool = true;

pub async fn remove_new_schema(path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let mut store_manager = manager(path);
    let mut stores: Vec<Arc<dyn KeyValueStore>> = Vec::new();
    for name in [
        "dag-membership-v1",
        "dag-height-v1",
        "dag-children-v1",
        "dag-history-state-v1",
        "dag-gc-retry-v1",
        "dag-gc-work-v1",
        "dag-gc-visited-v1",
    ] {
        stores.push(store_manager.store(name.to_string()).await?);
    }
    let group = AtomicStoreGroup::new(stores)?;
    for slot in 0..7 {
        loop {
            let count = group.transaction(|tx| {
                let page = tx.scan_page(slot, None, None, None, 1024)?;
                for (key, _) in &page.rows {
                    tx.delete(slot, key)?;
                }
                Ok(page.rows.len())
            })?;
            if count == 0 {
                break;
            }
        }
    }
    Ok(())
}
