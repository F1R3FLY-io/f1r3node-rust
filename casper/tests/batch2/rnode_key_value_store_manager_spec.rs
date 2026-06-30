use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use casper::rust::storage::rnode_key_value_store_manager::new_key_value_store_manager;
use models::rust::block_hash::{BlockHashSerde, LENGTH};
use prost::bytes::Bytes;
use tempfile::TempDir;

#[tokio::test]
async fn rnode_store_manager_initializes_block_dag_storage_on_fresh_lmdb_dir() {
    let dir = TempDir::new().unwrap();
    let mut kvm = new_key_value_store_manager(dir.path().to_path_buf(), None);
    let dag_storage = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();

    let block_hash = Bytes::from(vec![1; LENGTH]);
    let floor_hash = Bytes::from(vec![2; LENGTH]);

    dag_storage
        .floor_index
        .put_one(
            BlockHashSerde(block_hash.clone()),
            BlockHashSerde(floor_hash.clone()),
        )
        .unwrap();

    let stored = dag_storage
        .floor_index
        .get_one(&BlockHashSerde(block_hash))
        .unwrap();

    assert_eq!(stored, Some(BlockHashSerde(floor_hash)));
}
