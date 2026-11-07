use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::block_implicits::get_random_block;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::BlockMessage;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{
    Db, LmdbDirStoreManager, LmdbEnvConfig,
};
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

fn block(height: i64, parents: Vec<BlockHash>) -> BlockMessage {
    get_random_block(
        Some(height),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(parents),
        Some(vec![]),
        None,
        None,
        Some(vec![]),
        None,
        None,
    )
}

fn lmdb_manager(directory: &std::path::Path) -> Box<dyn KeyValueStoreManager> {
    let mapping = [
        "block-metadata",
        "equivocation-tracker",
        "latest-messages",
        "invalid-blocks",
        "floor-index",
        "frontier-index",
        "deploy-lifecycle-events",
        "deploy-lifecycle-terminal",
        "carrier-index",
        "carrier-index-meta",
        "genesis-hash",
        "dag-membership-v1",
        "dag-height-v1",
        "dag-children-v1",
        "dag-history-state-v1",
        "dag-gc-retry-v1",
        "dag-gc-work-v1",
        "dag-gc-visited-v1",
    ]
    .into_iter()
    .map(|name| {
        (Db::new(name.into(), None), LmdbEnvConfig {
            name: "dagstorage".into(),
            max_env_size: 64 * 1024 * 1024,
            max_dbs: 32,
        })
    })
    .collect();
    Box::new(LmdbDirStoreManager::new(directory.to_path_buf(), mapping))
}

#[tokio::test]
async fn eviction_preserves_historical_queries_and_captured_views() {
    let directory = tempfile::tempdir().unwrap();
    let mut manager = lmdb_manager(directory.path());
    let storage = BlockDagKeyValueStorage::new_with_hot_window(manager.as_mut(), 1)
        .await
        .unwrap();
    let genesis = block(0, vec![]);
    storage.insert(&genesis, InsertMode::Approved).unwrap();
    let old = storage.get_representation().unwrap();
    let mut chain = vec![genesis];
    for height in 1..40 {
        let next = block(height, vec![chain.last().unwrap().block_hash.clone()]);
        storage.insert(&next, InsertMode::Approved).unwrap();
        chain.push(next);
    }
    let current = storage.get_representation().unwrap();
    assert_eq!(current.resident_counts().blocks, 2);
    assert_eq!(current.latest_block_number(), 40);
    assert_eq!(old.latest_block_number(), 1);
    assert!(!old.contains(&chain[1].block_hash).unwrap());
    assert!(old
        .children(&chain[0].block_hash)
        .unwrap()
        .unwrap()
        .is_empty());
    for (height, item) in chain.iter().enumerate() {
        assert!(current.contains(&item.block_hash).unwrap());
        assert_eq!(
            current
                .lookup(&item.block_hash)
                .unwrap()
                .unwrap()
                .block_hash,
            item.block_hash
        );
        assert_eq!(
            current.block_number(&item.block_hash).unwrap(),
            Some(height as i64)
        );
        assert_eq!(
            current.main_parent(&item.block_hash).unwrap(),
            item.header.parents_hash_list.first().cloned()
        );
        assert!(current.is_finalized(&item.block_hash).unwrap());
        let children = current.children(&item.block_hash).unwrap().unwrap();
        assert_eq!(children.len(), usize::from(height + 1 < chain.len()));
        if let Some(child) = chain.get(height + 1) {
            assert!(children.contains(&child.block_hash));
        }
    }
    assert_eq!(
        current.topo_sort(0, None).unwrap(),
        chain
            .iter()
            .map(|b| vec![b.block_hash.clone()])
            .collect::<Vec<_>>()
    );
    let prefix = hex::encode(&chain[0].block_hash);
    assert_eq!(
        current.find(&prefix[..63]).unwrap(),
        Some(chain[0].block_hash.clone())
    );
    assert_eq!(current.resident_counts().blocks, 2);
}

#[tokio::test]
async fn legacy_migration_and_restart_rebuild_only_the_hot_window() {
    let directory = tempfile::tempdir().unwrap();
    let mut chain = Vec::new();
    {
        let mut manager = lmdb_manager(directory.path());
        let raw = manager.store("block-metadata".into()).await.unwrap();
        let typed = KeyValueTypedStoreImpl::<BlockHashSerde, BlockMetadata>::new(raw);
        for height in 0..1030 {
            let item = block(
                height,
                chain
                    .last()
                    .map(|b: &BlockMessage| vec![b.block_hash.clone()])
                    .unwrap_or_default(),
            );
            let mut metadata = BlockMetadata::from_block(&item, false, None, None);
            metadata.finalized = true;
            metadata.directly_finalized = height == 1029;
            typed
                .put_one(BlockHashSerde(item.block_hash.clone()), metadata)
                .unwrap();
            chain.push(item);
        }
    }
    for _ in 0..2 {
        let mut manager = lmdb_manager(directory.path());
        let storage = BlockDagKeyValueStorage::new_with_hot_window(manager.as_mut(), 2)
            .await
            .unwrap();
        let view = storage.get_representation().unwrap();
        assert_eq!(view.resident_counts().blocks, 3);
        assert_eq!(view.latest_block_number(), 1030);
        assert_eq!(
            view.last_finalized_block(),
            chain.last().unwrap().block_hash
        );
        assert_eq!(
            storage.genesis_hash().unwrap(),
            Some(chain[0].block_hash.clone())
        );
        assert_eq!(
            view.lookup(&chain[0].block_hash)
                .unwrap()
                .unwrap()
                .block_hash,
            chain[0].block_hash
        );
        assert!(view
            .children(&chain[0].block_hash)
            .unwrap()
            .unwrap()
            .contains(&chain[1].block_hash));
        assert_eq!(view.topo_sort(0, None).unwrap().len(), chain.len());
        assert_eq!(view.resident_counts().blocks, 3);
    }
}

#[tokio::test]
async fn invalid_future_and_failed_publication_preserve_the_valid_frontier() {
    let directory = tempfile::tempdir().unwrap();
    let mut manager = lmdb_manager(directory.path());
    let storage = BlockDagKeyValueStorage::new_with_hot_window(manager.as_mut(), 1)
        .await
        .unwrap();
    let genesis = block(0, vec![]);
    storage.insert(&genesis, InsertMode::Approved).unwrap();
    let invalid = block(i64::MAX, vec![genesis.block_hash.clone()]);
    let before = storage.insert(&invalid, InsertMode::Invalid).unwrap();
    assert_eq!(before.latest_block_number(), 1);
    assert_eq!(before.resident_counts().blocks, 1);
    assert!(before.lookup(&invalid.block_hash).unwrap().unwrap().invalid);
    let state = manager.store("dag-history-state-v1".into()).await.unwrap();
    state
        .put(vec![(b"gc.discovery".to_vec(), vec![255])])
        .unwrap();
    let candidate = block(1, vec![genesis.block_hash.clone()]);
    assert!(storage.insert(&candidate, InsertMode::Normal).is_err());
    let after = storage.get_representation().unwrap();
    assert_eq!(after.publication_sequence(), before.publication_sequence());
    assert_eq!(
        after.latest_message_hashes(),
        before.latest_message_hashes()
    );
    assert_eq!(after.latest_block_number(), 1);
    assert_eq!(
        after.children(&genesis.block_hash).unwrap(),
        before.children(&genesis.block_hash).unwrap()
    );
    assert!(!after.contains(&candidate.block_hash).unwrap());
    let metadata = manager.store("block-metadata".into()).await.unwrap();
    let candidate_key = bincode::serialize(&BlockHashSerde(candidate.block_hash.clone())).unwrap();
    assert!(metadata.get(&vec![candidate_key]).unwrap()[0].is_none());
    state.delete(vec![b"gc.discovery".to_vec()]).unwrap();
    let recovered = storage.insert(&candidate, InsertMode::Normal).unwrap();
    assert_eq!(
        recovered.publication_sequence(),
        before.publication_sequence() + 1
    );
    assert!(recovered.contains(&candidate.block_hash).unwrap());
    let invalid_key = bincode::serialize(&BlockHashSerde(invalid.block_hash.clone())).unwrap();
    metadata.delete(vec![invalid_key]).unwrap();
    assert!(matches!(
        recovered.lookup(&invalid.block_hash),
        Err(KvStoreError::KeyNotFound(_))
    ));
    assert!(recovered
        .lookup(&BlockHash::from(vec![99; 32]))
        .unwrap()
        .is_none());
}
