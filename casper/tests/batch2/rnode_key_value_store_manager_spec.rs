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

#[tokio::test]
async fn rnode_store_manager_quarantines_undecodable_deploy_records_across_reopen() {
    use std::collections::HashSet;

    use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
    use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
    use casper::rust::util::construct_deploy;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    let dir = TempDir::new().unwrap();
    let valid = construct_deploy::basic_deploy_data(1, None, None).unwrap();
    let damaged_value = vec![0xff, 0x01];
    let damaged_key = {
        let mut kvm = new_key_value_store_manager(dir.path().to_path_buf(), None);
        let mut storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let mut buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
        storage.add(vec![valid.clone()]).unwrap();
        buffer.add(vec![valid.clone()]).unwrap();
        let damaged_key = storage.store.encode_key(&vec![7u8; 64]).unwrap();
        for name in ["deploy_storage", "rejected_deploy_buffer"] {
            kvm.store(name.to_string())
                .await
                .unwrap()
                .put_one(damaged_key.clone(), damaged_value.clone())
                .unwrap();
        }
        drop((storage, buffer));
        kvm.shutdown().await.unwrap();
        damaged_key
    };

    for _ in 0..2 {
        let mut kvm = new_key_value_store_manager(dir.path().to_path_buf(), None);
        let storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
        assert_eq!(storage.read_all().unwrap(), HashSet::from([valid.clone()]));
        assert!(storage.any(|deploy| Ok(deploy.sig == valid.sig)).unwrap());
        assert_eq!(buffer.read_all().unwrap(), HashSet::from([valid.clone()]));
        for name in [
            "deploy_storage_quarantine",
            "rejected_deploy_buffer_quarantine",
        ] {
            assert_eq!(
                kvm.store(name.to_string())
                    .await
                    .unwrap()
                    .get_one(&damaged_key)
                    .unwrap(),
                Some(damaged_value.clone()),
                "{name}"
            );
        }
        drop((storage, buffer));
        kvm.shutdown().await.unwrap();
    }
}
