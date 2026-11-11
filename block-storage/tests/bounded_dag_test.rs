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
        "dag-sequence-v1",
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

/// A block the window has evicted is still found by sender and sequence
/// number, alone and once a twin is held beside it, and again after a
/// restart.
#[tokio::test]
async fn an_evicted_block_is_found_by_sender_and_sequence_number() {
    let directory = tempfile::tempdir().unwrap();
    let sender = BlockHash::from(vec![0x5A; 65]);
    let mut early = block(1, vec![]);
    early.sender = sender.clone();
    early.seq_num = 1;
    let mut twin = block(1, vec![]);
    twin.sender = sender.clone();
    twin.seq_num = 1;
    {
        let mut manager = lmdb_manager(directory.path());
        let storage = BlockDagKeyValueStorage::new_with_hot_window(manager.as_mut(), 1)
            .await
            .unwrap();
        let genesis = block(0, vec![]);
        storage.insert(&genesis, InsertMode::Approved).unwrap();
        early.header.parents_hash_list = vec![genesis.block_hash.clone()];
        storage.insert(&early, InsertMode::Approved).unwrap();
        let mut tip = early.block_hash.clone();
        for height in 2..40 {
            let next = block(height, vec![tip]);
            storage.insert(&next, InsertMode::Approved).unwrap();
            tip = next.block_hash.clone();
        }
        let view = storage.get_representation().unwrap();
        assert_eq!(
            view.resident_counts().blocks,
            2,
            "the early block is evicted"
        );
        assert_eq!(view.blocks_at(&sender, 1).unwrap(), vec![early
            .block_hash
            .clone()]);

        twin.header.parents_hash_list = vec![genesis.block_hash.clone()];
        storage
            .insert(&twin, InsertMode::Equivocation {
                sibling: early.block_hash.clone(),
            })
            .unwrap();
    }

    let mut manager = lmdb_manager(directory.path());
    let storage = BlockDagKeyValueStorage::new_with_hot_window(manager.as_mut(), 1)
        .await
        .unwrap();
    let mut held = storage
        .get_representation()
        .unwrap()
        .blocks_at(&sender, 1)
        .unwrap();
    held.sort();
    let mut expected = vec![early.block_hash.clone(), twin.block_hash.clone()];
    expected.sort();
    assert_eq!(held, expected);
}

#[tokio::test]
async fn restart_rebuilds_only_the_hot_window() {
    let directory = tempfile::tempdir().unwrap();
    let mut chain = Vec::new();
    {
        let mut manager = lmdb_manager(directory.path());
        let storage = BlockDagKeyValueStorage::new_with_hot_window(manager.as_mut(), 2)
            .await
            .unwrap();
        for height in 0..30 {
            let item = block(
                height,
                chain
                    .last()
                    .map(|b: &BlockMessage| vec![b.block_hash.clone()])
                    .unwrap_or_default(),
            );
            storage.insert(&item, InsertMode::Approved).unwrap();
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
        assert_eq!(view.latest_block_number(), 30);
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

async fn insert_chain(storage: &BlockDagKeyValueStorage, blocks: i64) -> std::time::Duration {
    let started = std::time::Instant::now();
    let mut parent: Option<BlockHash> = None;
    for height in 0..blocks {
        let mut item = block(height, parent.iter().cloned().collect());
        item.sender = vec![1u8; 65].into();
        item.seq_num = height as i32;
        storage.insert(&item, InsertMode::Approved).unwrap();
        parent = Some(item.block_hash);
    }
    started.elapsed()
}

#[tokio::test]
async fn insertion_cost_grows_linearly_with_history() {
    let mut small =
        rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager::new();
    let mut large =
        rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager::new();
    let small = BlockDagKeyValueStorage::new_with_hot_window(&mut small, 2)
        .await
        .unwrap();
    let large = BlockDagKeyValueStorage::new_with_hot_window(&mut large, 2)
        .await
        .unwrap();
    let base = insert_chain(&small, 1_000).await;
    let four_times = insert_chain(&large, 4_000).await;
    let ratio = four_times.as_secs_f64() / base.as_secs_f64();
    assert!(
        ratio < 8.0,
        "4x the inserts took {ratio:.1}x the time; a per-insert scan of history gives ~16x"
    );
}

#[tokio::test]
async fn a_deploy_whose_carrier_left_memory_keeps_its_canonical_appearance() {
    use models::rust::block_implicits::processed_deploy_gen;
    use proptest::strategy::{Strategy, ValueTree};
    use proptest::test_runner::TestRunner;

    let deploy = processed_deploy_gen()
        .new_tree(&mut TestRunner::default())
        .unwrap()
        .current();
    let mut windowed_kvm =
        rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager::new();
    let mut unbounded_kvm =
        rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager::new();
    let windowed = BlockDagKeyValueStorage::new_with_hot_window(&mut windowed_kvm, 2)
        .await
        .unwrap();
    let unbounded = BlockDagKeyValueStorage::new_with_hot_window(&mut unbounded_kvm, i64::MAX / 2)
        .await
        .unwrap();
    let genesis = block(0, vec![]);
    let mut carrier = block(1, vec![genesis.block_hash.clone()]);
    carrier.body.deploys = vec![deploy.clone()];
    let mut chain = vec![genesis, carrier.clone()];
    for height in 2..=12 {
        let parent = chain.last().unwrap().block_hash.clone();
        chain.push(block(height, vec![parent]));
    }
    for storage in [&windowed, &unbounded] {
        storage.insert(&chain[0], InsertMode::Approved).unwrap();
        for item in &chain[1..] {
            storage.insert(item, InsertMode::Normal).unwrap();
        }
        storage
            .record_directly_finalized(chain[9].block_hash.clone(), 0.9, |_| async { Ok(()) })
            .await
            .unwrap();
    }
    let windowed = windowed.get_representation().unwrap();
    let unbounded = unbounded.get_representation().unwrap();
    assert_eq!(
        windowed.resident_counts().blocks,
        6,
        "only heights 7..=12 stay in memory; the carrier at height 1 has left"
    );
    assert!(windowed.contains(&carrier.block_hash).unwrap());
    assert_eq!(
        unbounded
            .deploy_canonical_appearance(&deploy.deploy.sig)
            .unwrap(),
        Some(carrier.block_hash.clone())
    );
    assert_eq!(
        windowed
            .deploy_canonical_appearance(&deploy.deploy.sig)
            .unwrap(),
        Some(carrier.block_hash.clone())
    );
}

#[tokio::test]
async fn metadata_without_the_history_indexes_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    {
        let mut manager = lmdb_manager(directory.path());
        let raw = manager.store("block-metadata".into()).await.unwrap();
        let genesis = block(0, vec![]);
        let mut metadata = BlockMetadata::from_block(&genesis, None, None, None);
        metadata.finalized = true;
        metadata.directly_finalized = true;
        KeyValueTypedStoreImpl::<BlockHashSerde, BlockMetadata>::new(raw)
            .put_one(BlockHashSerde(genesis.block_hash.clone()), metadata)
            .unwrap();
    }
    let mut manager = lmdb_manager(directory.path());
    match BlockDagKeyValueStorage::new_with_hot_window(manager.as_mut(), 2).await {
        Err(KvStoreError::InvalidArgument(message)) => {
            assert!(message.contains("wipe"), "{message}")
        }
        Err(other) => panic!("expected a wipe instruction, got {other:?}"),
        Ok(_) => panic!("a store written without the history indexes must not open"),
    }
}

#[tokio::test]
async fn a_block_outside_the_window_is_held_and_satisfies_its_dependents() {
    let directory = tempfile::tempdir().unwrap();
    let mut manager = lmdb_manager(directory.path());
    let storage = BlockDagKeyValueStorage::new_with_hot_window(manager.as_mut(), 1)
        .await
        .unwrap();
    let genesis = block(0, vec![]);
    storage.insert(&genesis, InsertMode::Approved).unwrap();
    let mut chain = vec![genesis];
    for height in 1..10 {
        let next = block(height, vec![chain.last().unwrap().block_hash.clone()]);
        storage.insert(&next, InsertMode::Approved).unwrap();
        chain.push(next);
    }
    assert!(
        storage
            .get_representation()
            .unwrap()
            .resident_counts()
            .blocks
            < chain.len()
    );
    let evicted = &chain[1].block_hash;
    assert!(storage.contains(evicted).unwrap());
    assert!(storage
        .unsatisfied_dependencies(std::slice::from_ref(evicted))
        .unwrap()
        .is_empty());
    assert!(storage.is_dependency_satisfied(evicted).unwrap());
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
    let before = storage
        .insert(&invalid, InsertMode::Equivocation {
            sibling: genesis.block_hash.clone(),
        })
        .unwrap();
    assert_eq!(before.latest_block_number(), 1);
    assert_eq!(before.resident_counts().blocks, 1);
    assert!(before
        .lookup(&invalid.block_hash)
        .unwrap()
        .unwrap()
        .is_invalid());
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
