use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::finality::FinalizationLedger;
use casper::rust::storage::rnode_key_value_store_manager::{
    new_key_value_store_manager, rnode_db_mapping,
};
use models::rust::block_hash::{BlockHashSerde, LENGTH};
use prost::bytes::Bytes;
use tempfile::TempDir;

#[test]
fn rnode_mapping_registers_protocol_v5_finalization_ledger() {
    for legacy_rspace_paths in [Some(false), Some(true)] {
        let matches = rnode_db_mapping(legacy_rspace_paths)
            .into_iter()
            .filter(|(db, _)| db.id() == FinalizationLedger::STORE_NAME)
            .count();
        assert_eq!(matches, 1);
    }
}

#[tokio::test]
async fn rnode_store_manager_initializes_block_dag_storage_on_fresh_lmdb_dir() {
    let dir = TempDir::new().unwrap();
    let mut kvm = new_key_value_store_manager(dir.path().to_path_buf(), None);
    let dag_storage = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();
    assert_eq!(dag_storage.finalization_head().unwrap(), None);

    let block_hash = Bytes::from(vec![1; LENGTH]);
    let floor_hash = Bytes::from(vec![2; LENGTH]);

    dag_storage
        .floor_index_for_tests()
        .put_one(
            BlockHashSerde(block_hash.clone()),
            BlockHashSerde(floor_hash.clone()),
        )
        .unwrap();

    let stored = dag_storage
        .floor_index_for_tests()
        .get_one(&BlockHashSerde(block_hash))
        .unwrap();

    assert_eq!(stored, Some(BlockHashSerde(floor_hash)));
}

#[tokio::test]
async fn rnode_store_manager_frontier_index_round_trips() {
    // The persisted per-block finalized frontier F(X) (the warm up-walk pivot)
    // must round-trip through the new `frontier-index` LMDB store, exactly like
    // the floor index. Covers the H2 cache's persistence layer.
    let dir = TempDir::new().unwrap();
    let mut kvm = new_key_value_store_manager(dir.path().to_path_buf(), None);
    let dag_storage = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();

    let block_hash = Bytes::from(vec![7; LENGTH]);
    let frontier_hash = Bytes::from(vec![9; LENGTH]);

    dag_storage
        .frontier_index_for_tests()
        .put_one(
            BlockHashSerde(block_hash.clone()),
            BlockHashSerde(frontier_hash.clone()),
        )
        .unwrap();

    let stored = dag_storage
        .frontier_index_for_tests()
        .get_one(&BlockHashSerde(block_hash))
        .unwrap();

    assert_eq!(stored, Some(BlockHashSerde(frontier_hash)));
}
