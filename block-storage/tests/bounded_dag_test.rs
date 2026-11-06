use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::block_implicits::get_random_block;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{
    Db, LmdbDirStoreManager, LmdbEnvConfig,
};
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

#[tokio::test]
async fn failed_finality_transaction_keeps_the_previous_lfb_and_resident_roots() {
    let mut manager = InMemoryStoreManager::new();
    let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    let genesis = block(0, vec![]);
    storage.insert(&genesis, InsertMode::Approved).unwrap();
    let head = block(20, vec![genesis.block_hash.clone()]);
    storage.insert(&head, InsertMode::Normal).unwrap();
    let before = storage.get_representation().unwrap();
    let state = manager.store("dag-history-state-v1".into()).await.unwrap();
    let progress_key = b"dag.progress".to_vec();
    let original = state.get(&vec![progress_key.clone()]).unwrap()[0]
        .clone()
        .unwrap();
    state.put(vec![(progress_key.clone(), vec![255])]).unwrap();
    assert!(storage
        .record_directly_finalized(head.block_hash.clone(), 1.0, |_| async { Ok(()) })
        .await
        .is_err());
    let after = storage.get_representation().unwrap();
    assert_eq!(after.last_finalized_block(), genesis.block_hash);
    assert_eq!(
        after.resident_counts().blocks,
        before.resident_counts().blocks
    );
    assert!(!after.is_finalized(&head.block_hash).unwrap());
    state.put(vec![(progress_key, original)]).unwrap();
    storage
        .record_directly_finalized(head.block_hash.clone(), 1.0, |_| async { Ok(()) })
        .await
        .unwrap();
    let finalized = storage.get_representation().unwrap();
    assert_eq!(finalized.last_finalized_block(), head.block_hash);
    assert_eq!(finalized.resident_counts().blocks, 1);
    assert!(finalized.is_finalized(&genesis.block_hash).unwrap());
}

#[tokio::test]
async fn sparse_restore_preserves_learned_genesis_across_eviction_and_restart() {
    let mut manager = InMemoryStoreManager::new();
    let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    let learned = BlockHash::from(vec![57; 32]);
    storage.record_genesis_hash(learned.clone()).unwrap();
    let anchor = block(100, vec![BlockHash::from(vec![58; 32])]);
    storage.insert(&anchor, InsertMode::Approved).unwrap();
    let head = block(105, vec![anchor.block_hash.clone()]);
    storage.insert(&head, InsertMode::Approved).unwrap();
    assert_eq!(storage.genesis_hash().unwrap(), Some(learned.clone()));
    assert!(storage
        .record_genesis_hash(BlockHash::from(vec![59; 32]))
        .is_err());
    let restored = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    let view = restored.get_representation().unwrap();
    assert_eq!(restored.genesis_hash().unwrap(), Some(learned));
    assert!(view.contains(&anchor.block_hash).unwrap());
    assert!(view
        .lookup(&BlockHash::from(vec![58; 32]))
        .unwrap()
        .is_none());
}

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
async fn invalid_heights_outside_the_valid_frontier_stay_cold_across_finality_and_reopen() {
    for lmdb in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut manager: Box<dyn KeyValueStoreManager> = if lmdb {
            lmdb_manager(directory.path())
        } else {
            Box::new(InMemoryStoreManager::new())
        };
        let storage = BlockDagKeyValueStorage::new_with_hot_window(manager.as_mut(), 1)
            .await
            .unwrap();
        let genesis = block(0, vec![]);
        storage.insert(&genesis, InsertMode::Approved).unwrap();
        let old = storage.get_representation().unwrap();
        let mut invalid_blocks = Vec::new();
        for (index, height) in [i64::MIN, -1, i64::MAX - 1, i64::MAX]
            .into_iter()
            .enumerate()
        {
            let mut invalid = block(height, vec![genesis.block_hash.clone()]);
            invalid.sender = vec![index as u8 + 1; models::rust::validator::LENGTH].into();
            storage.insert(&invalid, InsertMode::Invalid).unwrap();
            invalid_blocks.push(invalid);
        }
        let before_finality = storage.get_representation().unwrap();
        assert_eq!(before_finality.resident_counts().blocks, 1);
        assert_eq!(before_finality.resident_counts().child_keys, 1);
        assert_eq!(before_finality.latest_block_number(), 1);
        for invalid in &invalid_blocks {
            assert!(!old.contains(&invalid.block_hash).unwrap());
            assert!(old.lookup(&invalid.block_hash).unwrap().is_none());
            assert!(
                before_finality
                    .lookup(&invalid.block_hash)
                    .unwrap()
                    .unwrap()
                    .invalid
            );
            assert_eq!(
                before_finality
                    .invalid_latest_messages()
                    .unwrap()
                    .get(&invalid.sender),
                Some(&invalid.block_hash)
            );
        }
        assert!(old
            .children(&genesis.block_hash)
            .unwrap()
            .unwrap()
            .is_empty());
        let head = block(20, vec![genesis.block_hash.clone()]);
        let current = storage.insert(&head, InsertMode::Approved).unwrap();
        assert_eq!(current.resident_counts().blocks, 1);
        assert_eq!(current.resident_counts().child_keys, 2);
        assert_eq!(current.latest_block_number(), 21);
        assert_eq!(old.last_finalized_block(), genesis.block_hash);
        assert_eq!(before_finality.resident_counts().blocks, 1);
        let children = current.children(&genesis.block_hash).unwrap().unwrap();
        assert_eq!(children.len(), invalid_blocks.len() + 1);
        let invalid_latest = current.invalid_latest_messages().unwrap();
        for invalid in &invalid_blocks {
            assert!(current.contains(&invalid.block_hash).unwrap());
            assert!(children.contains(&invalid.block_hash));
            assert_eq!(
                current.lookup(&invalid.block_hash).unwrap(),
                before_finality.lookup(&invalid.block_hash).unwrap()
            );
            assert_eq!(
                invalid_latest.get(&invalid.sender),
                Some(&invalid.block_hash)
            );
        }

        let raw = manager.store("block-metadata".into()).await.unwrap();
        let future = invalid_blocks.last().unwrap();
        let key = bincode::serialize(&BlockHashSerde(future.block_hash.clone())).unwrap();
        let original = raw.get(&vec![key.clone()]).unwrap()[0].clone().unwrap();
        raw.put(vec![(key.clone(), vec![255])]).unwrap();
        drop(raw);
        drop(old);
        drop(before_finality);
        drop(current);
        drop(storage);
        if lmdb {
            drop(manager);
            manager = lmdb_manager(directory.path());
        }
        let reopened = BlockDagKeyValueStorage::new_with_hot_window(manager.as_mut(), 1)
            .await
            .unwrap();
        let view = reopened.get_representation().unwrap();
        assert_eq!(view.resident_counts().blocks, 1);
        assert_eq!(view.resident_counts().child_keys, 2);
        assert!(view.contains(&future.block_hash).unwrap());
        assert!(view.lookup(&future.block_hash).is_err());
        let raw = manager.store("block-metadata".into()).await.unwrap();
        raw.put(vec![(key, original)]).unwrap();
        assert_eq!(view.last_finalized_block(), head.block_hash);
        assert_eq!(view.invalid_latest_messages().unwrap(), invalid_latest);
        assert_eq!(
            view.children(&genesis.block_hash).unwrap().unwrap(),
            children
        );
        for invalid in &invalid_blocks {
            let metadata = view.lookup(&invalid.block_hash).unwrap().unwrap();
            assert!(metadata.invalid);
            assert_eq!(metadata.block_number, invalid.body.state.block_number);
        }
    }
}

#[tokio::test]
async fn invalid_only_history_has_no_resident_band() {
    let mut manager = InMemoryStoreManager::new();
    let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    let invalid = block(i64::MAX, vec![]);
    let view = storage.insert(&invalid, InsertMode::Invalid).unwrap();
    assert_eq!(view.resident_counts().blocks, 0);
    assert!(view.lookup(&invalid.block_hash).unwrap().unwrap().invalid);
    let reopened = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    let view = reopened.insert(&invalid, InsertMode::Invalid).unwrap();
    assert_eq!(view.resident_counts().blocks, 0);
    assert!(view.lookup(&invalid.block_hash).unwrap().unwrap().invalid);
    assert!(matches!(
        reopened.get_representation(),
        Err(KvStoreError::LastFinalizedBlockUninitialized)
    ));
}

#[tokio::test]
async fn advancing_valid_frontier_keeps_future_invalid_queries_consistent_without_rehydration() {
    let mut manager = InMemoryStoreManager::new();
    let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    let genesis = block(0, vec![]);
    storage.insert(&genesis, InsertMode::Approved).unwrap();
    let invalid = block(1, vec![genesis.block_hash.clone()]);
    let before = storage.insert(&invalid, InsertMode::Invalid).unwrap();
    assert_eq!(before.resident_counts().blocks, 1);
    let head = block(2, vec![genesis.block_hash.clone()]);
    let after = storage.insert(&head, InsertMode::Normal).unwrap();
    assert_eq!(after.resident_counts().blocks, 2);
    assert_eq!(
        after.lookup(&invalid.block_hash).unwrap(),
        before.lookup(&invalid.block_hash).unwrap()
    );
    let reopened = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    let reopened = reopened.get_representation().unwrap();
    assert_eq!(reopened.resident_counts().blocks, 3);
    assert_eq!(
        reopened.lookup(&invalid.block_hash).unwrap(),
        after.lookup(&invalid.block_hash).unwrap()
    );
    assert_eq!(
        reopened.children(&genesis.block_hash).unwrap(),
        after.children(&genesis.block_hash).unwrap()
    );
    assert_eq!(
        reopened.invalid_latest_messages().unwrap(),
        after.invalid_latest_messages().unwrap()
    );
}

#[tokio::test]
async fn approved_replay_cannot_make_invalid_metadata_the_finality_root() {
    let mut manager = InMemoryStoreManager::new();
    let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    let genesis = block(0, vec![]);
    storage.insert(&genesis, InsertMode::Approved).unwrap();
    let invalid = block(50, vec![genesis.block_hash.clone()]);
    storage.insert(&invalid, InsertMode::Invalid).unwrap();
    assert!(storage.insert(&invalid, InsertMode::Approved).is_err());
    let view = storage.get_representation().unwrap();
    assert_eq!(view.last_finalized_block(), genesis.block_hash);
    let metadata = view.lookup(&invalid.block_hash).unwrap().unwrap();
    assert!(metadata.invalid);
    assert!(!metadata.finalized);
    let reopened = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    assert_eq!(
        reopened
            .get_representation()
            .unwrap()
            .last_finalized_block(),
        genesis.block_hash
    );
}

#[tokio::test]
async fn cold_queries_and_snapshots_match_a_large_window_after_reopen() {
    for window in [1, 2, 16] {
        let mut manager = InMemoryStoreManager::new();
        let bounded = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, window)
            .await
            .unwrap();
        let mut reference_manager = InMemoryStoreManager::new();
        let reference =
            BlockDagKeyValueStorage::new_with_hot_window(&mut reference_manager, i64::MAX)
                .await
                .unwrap();
        let genesis = block(0, vec![]);
        bounded.insert(&genesis, InsertMode::Approved).unwrap();
        reference.insert(&genesis, InsertMode::Approved).unwrap();
        let old = bounded.get_representation().unwrap();
        let mut chain = vec![genesis];
        for height in 1..40 {
            let next = block(height, vec![chain.last().unwrap().block_hash.clone()]);
            bounded.insert(&next, InsertMode::Approved).unwrap();
            reference.insert(&next, InsertMode::Approved).unwrap();
            chain.push(next);
        }
        let current = bounded.get_representation().unwrap();
        assert_eq!(current.resident_counts().blocks, window as usize + 1);
        assert_eq!(old.latest_block_number(), 1);
        assert!(!old.contains(&chain[1].block_hash).unwrap());
        assert!(old
            .children(&chain[0].block_hash)
            .unwrap()
            .unwrap()
            .is_empty());
        let expected = reference.get_representation().unwrap();
        for item in &chain {
            assert_eq!(
                current.lookup(&item.block_hash).unwrap(),
                expected.lookup(&item.block_hash).unwrap()
            );
            assert_eq!(
                current.children(&item.block_hash).unwrap(),
                expected.children(&item.block_hash).unwrap()
            );
            assert_eq!(
                current.main_parent(&item.block_hash).unwrap(),
                expected.main_parent(&item.block_hash).unwrap()
            );
            assert_eq!(
                current.block_number(&item.block_hash).unwrap(),
                Some(item.body.state.block_number)
            );
            assert!(current.is_finalized(&item.block_hash).unwrap());
        }
        assert_eq!(
            current.topo_sort(0, None).unwrap(),
            expected.topo_sort(0, None).unwrap()
        );
        let prefix = hex::encode(&chain[0].block_hash);
        assert_eq!(
            current.find(&prefix[..63]).unwrap(),
            Some(chain[0].block_hash.clone())
        );
        let reopened = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, window)
            .await
            .unwrap();
        let reopened_view = reopened.get_representation().unwrap();
        assert_eq!(reopened_view.resident_counts().blocks, window as usize + 1);
        assert_eq!(
            reopened_view.last_finalized_block(),
            chain.last().unwrap().block_hash
        );
        assert_eq!(
            reopened.genesis_hash().unwrap(),
            Some(chain[0].block_hash.clone())
        );
        assert_eq!(
            old.lookup(&chain[0].block_hash).unwrap(),
            current.lookup(&chain[0].block_hash).unwrap()
        );
    }
}

#[tokio::test]
async fn late_history_is_durable_without_rehydrating_or_changing_old_views() {
    let mut manager = InMemoryStoreManager::new();
    let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    let genesis = block(0, vec![]);
    storage.insert(&genesis, InsertMode::Approved).unwrap();
    let head = block(10, vec![genesis.block_hash.clone()]);
    storage.insert(&head, InsertMode::Approved).unwrap();
    let old = storage.get_representation().unwrap();
    let late = block(1, vec![genesis.block_hash.clone()]);
    let current = storage.insert(&late, InsertMode::SettledHistory).unwrap();
    assert_eq!(current.resident_counts().blocks, 1);
    assert!(current.contains(&late.block_hash).unwrap());
    assert!(!old.contains(&late.block_hash).unwrap());
    assert_eq!(old.latest_message_hashes(), current.latest_message_hashes());
    assert_eq!(current.latest_block_number(), 11);
    assert_eq!(
        current
            .children(&genesis.block_hash)
            .unwrap()
            .unwrap()
            .len(),
        2
    );
    let duplicate = storage.insert(&late, InsertMode::Normal).unwrap();
    assert_eq!(
        duplicate.latest_message_hashes(),
        current.latest_message_hashes()
    );
    assert_eq!(
        duplicate.publication_sequence(),
        current.publication_sequence()
    );
}

#[tokio::test]
async fn genesis_identity_survives_eviction_and_new_validator_bonding() {
    let mut manager = InMemoryStoreManager::new();
    let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    let genesis = block(0, vec![]);
    storage.insert(&genesis, InsertMode::Approved).unwrap();
    let head = block(8, vec![genesis.block_hash.clone()]);
    storage.insert(&head, InsertMode::Approved).unwrap();
    let mut next = block(9, vec![head.block_hash.clone()]);
    let validator = BlockHash::from(vec![7; 32]);
    next.body.state.bonds.push(Bond {
        validator: validator.clone(),
        stake: 10,
    });
    let view = storage.insert(&next, InsertMode::Normal).unwrap();
    assert_eq!(
        view.latest_message_hash(&validator),
        Some(genesis.block_hash.clone())
    );
    assert_eq!(
        storage.genesis_hash().unwrap(),
        Some(genesis.block_hash.clone())
    );
    let conflicting = block(0, vec![]);
    assert!(storage.insert(&conflicting, InsertMode::Approved).is_err());
    assert!(!storage
        .get_representation()
        .unwrap()
        .contains(&conflicting.block_hash)
        .unwrap());
    storage.insert(&genesis, InsertMode::Approved).unwrap();
    assert_eq!(
        storage.get_representation().unwrap().last_finalized_block(),
        head.block_hash
    );
}

#[tokio::test]
async fn legacy_migration_recovers_history_and_distinguishes_corruption_from_absence() {
    let mut manager = InMemoryStoreManager::new();
    let raw = manager.store("block-metadata".into()).await.unwrap();
    let typed = KeyValueTypedStoreImpl::<BlockHashSerde, BlockMetadata>::new(raw.clone());
    let mut chain = Vec::new();
    for height in 0..1100 {
        let item = block(
            height,
            chain
                .last()
                .map(|b: &BlockMessage| vec![b.block_hash.clone()])
                .unwrap_or_default(),
        );
        let mut metadata = BlockMetadata::from_block(&item, false, None, None);
        metadata.finalized = true;
        metadata.directly_finalized = height == 1099;
        typed
            .put_one(BlockHashSerde(item.block_hash.clone()), metadata)
            .unwrap();
        chain.push(item);
    }
    let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 2)
        .await
        .unwrap();
    let view = storage.get_representation().unwrap();
    assert_eq!(view.resident_counts().blocks, 3);
    assert!(view.contains(&chain[0].block_hash).unwrap());
    assert_eq!(
        storage.genesis_hash().unwrap(),
        Some(chain[0].block_hash.clone())
    );
    raw.delete(vec![bincode::serialize(&BlockHashSerde(
        chain[0].block_hash.clone(),
    ))
    .unwrap()])
        .unwrap();
    assert!(matches!(
        view.lookup(&chain[0].block_hash),
        Err(KvStoreError::KeyNotFound(_))
    ));
    assert!(view
        .lookup(&BlockHash::from(vec![99; 32]))
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn publication_failure_rolls_back_membership_metadata_latest_and_height() {
    let mut manager = InMemoryStoreManager::new();
    let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    let genesis = block(0, vec![]);
    storage.insert(&genesis, InsertMode::Approved).unwrap();
    let before = storage.get_representation().unwrap();
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
    assert_eq!(after.latest_block_number(), before.latest_block_number());
    assert!(!after.contains(&candidate.block_hash).unwrap());
    let metadata = manager.store("block-metadata".into()).await.unwrap();
    assert!(metadata
        .get(&vec![bincode::serialize(&BlockHashSerde(
            candidate.block_hash.clone()
        ))
        .unwrap()])
        .unwrap()[0]
        .is_none());
    assert!(after
        .children(&genesis.block_hash)
        .unwrap()
        .unwrap()
        .is_empty());
    state.delete(vec![b"gc.discovery".to_vec()]).unwrap();
    let recovered = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    let inserted = recovered.insert(&candidate, InsertMode::Normal).unwrap();
    assert!(inserted.contains(&candidate.block_hash).unwrap());
    assert_eq!(
        inserted.publication_sequence(),
        before.publication_sequence() + 1
    );
}

#[tokio::test]
async fn migration_resumes_a_committed_batch_after_a_later_decode_failure() {
    let mut manager = InMemoryStoreManager::new();
    let metadata = manager.store("block-metadata".into()).await.unwrap();
    let typed = KeyValueTypedStoreImpl::<BlockHashSerde, BlockMetadata>::new(metadata.clone());
    let mut first = None;
    for height in 0..1030 {
        let mut item = block(height, vec![]);
        let mut hash = vec![0; 32];
        hash[24..].copy_from_slice(&(height as u64).to_be_bytes());
        item.block_hash = BlockHash::from(hash);
        if height == 0 {
            first = Some(item.block_hash.clone());
        }
        let mut row = BlockMetadata::from_block(&item, false, None, None);
        row.directly_finalized = height == 1029;
        row.finalized = true;
        typed.put_one(BlockHashSerde(item.block_hash), row).unwrap();
    }
    let corrupt_key = bincode::serialize(&BlockHashSerde(BlockHash::from(vec![255; 32]))).unwrap();
    metadata
        .put(vec![(corrupt_key.clone(), vec![255])])
        .unwrap();
    assert!(
        BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 2)
            .await
            .is_err()
    );
    let membership = manager.store("dag-membership-v1".into()).await.unwrap();
    assert_eq!(membership.to_map().unwrap().len(), 1024);
    metadata.delete(vec![corrupt_key]).unwrap();
    let restored = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 2)
        .await
        .unwrap();
    let view = restored.get_representation().unwrap();
    assert_eq!(view.resident_counts().blocks, 3);
    assert_eq!(membership.to_map().unwrap().len(), 1030);
    assert!(view.contains(&first.unwrap()).unwrap());
}

#[tokio::test]
async fn failed_eviction_keeps_old_roots_and_retries_after_metadata_repair() {
    let mut manager = InMemoryStoreManager::new();
    let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    let genesis = block(0, vec![]);
    storage.insert(&genesis, InsertMode::Approved).unwrap();
    let old = storage.get_representation().unwrap();
    let raw = manager.store("block-metadata".into()).await.unwrap();
    let key = bincode::serialize(&BlockHashSerde(genesis.block_hash.clone())).unwrap();
    let bytes = raw.get(&vec![key.clone()]).unwrap()[0].clone().unwrap();
    raw.delete(vec![key.clone()]).unwrap();
    let head = block(9, vec![genesis.block_hash.clone()]);
    assert!(storage.insert(&head, InsertMode::Approved).is_err());
    assert_eq!(old.resident_counts().blocks, 1);
    assert!(old.contains(&genesis.block_hash).unwrap());
    assert!(storage.get_representation().is_err());
    raw.put(vec![(key, bytes)]).unwrap();
    let repaired = storage.get_representation().unwrap();
    assert_eq!(repaired.resident_counts().blocks, 1);
    assert!(repaired.contains(&genesis.block_hash).unwrap());
    assert_eq!(repaired.last_finalized_block(), head.block_hash);
    assert_eq!(
        repaired
            .children(&genesis.block_hash)
            .unwrap()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn eviction_removes_invalid_reverse_edges_without_assuming_parent_height_order() {
    let mut manager = InMemoryStoreManager::new();
    let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    let genesis = block(0, vec![]);
    storage.insert(&genesis, InsertMode::Approved).unwrap();
    let parent = block(5, vec![genesis.block_hash.clone()]);
    storage.insert(&parent, InsertMode::Normal).unwrap();
    let invalid = block(1, vec![
        parent.block_hash.clone(),
        genesis.block_hash.clone(),
    ]);
    storage.insert(&invalid, InsertMode::Invalid).unwrap();
    let before = storage.get_representation().unwrap();
    let head = block(10, vec![genesis.block_hash.clone()]);
    let after = storage.insert(&head, InsertMode::Approved).unwrap();
    assert_eq!(after.resident_counts().blocks, 1);
    assert_eq!(after.resident_counts().child_keys, 2);
    assert_eq!(before.resident_counts().blocks, 3);
    assert!(after.contains(&invalid.block_hash).unwrap());
    assert!(after.lookup(&invalid.block_hash).unwrap().unwrap().invalid);
    assert_eq!(
        after
            .children(&parent.block_hash)
            .unwrap()
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        vec![invalid.block_hash.clone()]
    );
    assert_eq!(
        after
            .topo_sort(0, Some(5))
            .unwrap()
            .into_iter()
            .flatten()
            .collect::<std::collections::HashSet<_>>(),
        std::collections::HashSet::from([genesis.block_hash.clone(), parent.block_hash.clone()])
    );
    let reopened = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    let reopened = reopened.get_representation().unwrap();
    assert_eq!(reopened.resident_counts().blocks, 1);
    assert_eq!(reopened.resident_counts().child_keys, 2);
    assert_eq!(
        reopened.children(&parent.block_hash).unwrap(),
        after.children(&parent.block_hash).unwrap()
    );
}

#[tokio::test]
async fn ready_schema_rejects_missing_historical_secondary_rows() {
    for table in ["dag-membership-v1", "dag-height-v1", "dag-children-v1"] {
        let mut manager = InMemoryStoreManager::new();
        let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
            .await
            .unwrap();
        let genesis = block(0, vec![]);
        storage.insert(&genesis, InsertMode::Approved).unwrap();
        let cold = block(1, vec![genesis.block_hash.clone()]);
        storage.insert(&cold, InsertMode::Normal).unwrap();
        let head = block(10, vec![genesis.block_hash.clone()]);
        storage.insert(&head, InsertMode::Approved).unwrap();
        let raw = manager.store(table.into()).await.unwrap();
        let key = match table {
            "dag-membership-v1" => cold.block_hash.to_vec(),
            "dag-height-v1" => {
                block_storage::rust::dag::dag_history_index::encode_height_key(1, &cold.block_hash)
                    .unwrap()
            }
            _ => [genesis.block_hash.as_ref(), cold.block_hash.as_ref()].concat(),
        };
        assert_eq!(raw.delete(vec![key]).unwrap(), 1);
        let opened = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1).await;
        assert!(
            opened.is_err(),
            "ready schema accepted missing row in {table}"
        );
    }
}

#[tokio::test]
async fn cold_metadata_identity_height_and_invalid_status_must_match_membership() {
    for corrupt_field in ["identity", "height", "invalid"] {
        let mut manager = InMemoryStoreManager::new();
        let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
            .await
            .unwrap();
        let genesis = block(0, vec![]);
        storage.insert(&genesis, InsertMode::Approved).unwrap();
        let head = block(10, vec![]);
        storage.insert(&head, InsertMode::Approved).unwrap();
        let view = storage.get_representation().unwrap();
        let mut metadata = view.lookup(&genesis.block_hash).unwrap().unwrap();
        match corrupt_field {
            "identity" => metadata.block_hash = BlockHash::from(vec![99; 32]),
            "height" => metadata.block_number = 42,
            _ => metadata.invalid = true,
        }
        let raw = manager.store("block-metadata".into()).await.unwrap();
        raw.put(vec![(
            bincode::serialize(&BlockHashSerde(genesis.block_hash.clone())).unwrap(),
            bincode::serialize(&metadata).unwrap(),
        )])
        .unwrap();
        assert!(
            view.lookup(&genesis.block_hash).is_err(),
            "accepted corrupt cold metadata {corrupt_field}"
        );
        assert!(matches!(
            storage.record_directly_finalized(head.block_hash.clone(), 0.75, |_| async { Ok(()) }).await,
            Err(KvStoreError::InvalidArgument(message)) if message == "finalized FT metadata disagrees with membership"
        ));
    }
}

#[tokio::test]
async fn lifecycle_appearances_keep_cold_membership_and_ignore_future_publication() {
    use block_storage::rust::dag::deploy_lifecycle_types::{LifecycleEvent, LifecycleEventKind};

    let mut manager = InMemoryStoreManager::new();
    let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    let genesis = block(0, vec![]);
    storage.insert(&genesis, InsertMode::Approved).unwrap();
    let head = block(10, vec![genesis.block_hash.clone()]);
    storage.insert(&head, InsertMode::Approved).unwrap();
    let old = storage.get_representation().unwrap();
    let late = block(1, vec![genesis.block_hash.clone()]);
    let sig = vec![42; 64];
    for item in [&genesis, &late] {
        old.lifecycle
            .write()
            .append_event_once(&sig, Some(0), LifecycleEvent {
                height: item.body.state.block_number,
                block_hash: item.block_hash.to_vec(),
                kind: LifecycleEventKind::Included { is_failed: false },
            })
            .unwrap();
    }
    assert_eq!(
        old.deploy_canonical_appearance(&sig).unwrap(),
        Some(genesis.block_hash.clone())
    );
    assert_eq!(
        storage.deploy_canonical_appearance(&sig).unwrap(),
        Some(genesis.block_hash.clone())
    );
    let current = storage.insert(&late, InsertMode::SettledHistory).unwrap();
    assert_eq!(
        current.deploy_canonical_appearance(&sig).unwrap(),
        Some(late.block_hash.clone())
    );
    assert_eq!(
        old.deploy_canonical_appearance(&sig).unwrap(),
        Some(genesis.block_hash)
    );
}

#[tokio::test]
async fn terminal_appearances_respect_view_sequence_and_propagate_membership_decode_errors() {
    use block_storage::rust::dag::deploy_lifecycle_types::{TerminalRecord, TerminalState};

    let mut manager = InMemoryStoreManager::new();
    let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
        .await
        .unwrap();
    let genesis = block(0, vec![]);
    storage.insert(&genesis, InsertMode::Approved).unwrap();
    let head = block(10, vec![genesis.block_hash.clone()]);
    storage.insert(&head, InsertMode::Approved).unwrap();
    let old = storage.get_representation().unwrap();
    let late = block(1, vec![genesis.block_hash.clone()]);
    let current = storage.insert(&late, InsertMode::SettledHistory).unwrap();
    let sig = vec![77; 64];
    current
        .put_deploy_terminal_if_absent(&sig, TerminalRecord {
            state: TerminalState::Finalized,
            rejection_count: 0,
            latest_height: 1,
            latest_block_hash: late.block_hash.to_vec(),
        })
        .unwrap();
    assert_eq!(
        current.deploy_canonical_appearance(&sig).unwrap(),
        Some(late.block_hash.clone())
    );
    assert_eq!(old.deploy_canonical_appearance(&sig).unwrap(), None);
    let membership = manager.store("dag-membership-v1".into()).await.unwrap();
    membership
        .put(vec![(late.block_hash.to_vec(), vec![255])])
        .unwrap();
    assert!(current.deploy_canonical_appearance(&sig).is_err());
    assert!(storage.deploy_canonical_appearance(&sig).is_err());
}

#[tokio::test]
async fn corrupt_latest_metadata_prevents_new_sender_publication_until_repaired() {
    for corrupt_field in ["identity", "height", "invalid", "missing"] {
        let mut manager = InMemoryStoreManager::new();
        let storage = BlockDagKeyValueStorage::new_with_hot_window(&mut manager, 1)
            .await
            .unwrap();
        let genesis = block(0, vec![]);
        storage.insert(&genesis, InsertMode::Approved).unwrap();
        let sender = BlockHash::from(vec![7; 65]);
        let mut previous = block(1, vec![genesis.block_hash.clone()]);
        previous.sender = sender.clone();
        previous.seq_num = 1;
        storage.insert(&previous, InsertMode::Normal).unwrap();
        let mut head = block(10, vec![genesis.block_hash.clone()]);
        head.sender = BlockHash::from(vec![8; 65]);
        storage.insert(&head, InsertMode::Approved).unwrap();
        let before = storage.get_representation().unwrap();
        assert_eq!(
            before.latest_message_hash(&sender),
            Some(previous.block_hash.clone())
        );
        let raw = manager.store("block-metadata".into()).await.unwrap();
        let key = bincode::serialize(&BlockHashSerde(previous.block_hash.clone())).unwrap();
        let original = raw.get(&vec![key.clone()]).unwrap()[0].clone().unwrap();
        let mut metadata: BlockMetadata = bincode::deserialize(&original).unwrap();
        match corrupt_field {
            "identity" => metadata.block_hash = BlockHash::from(vec![99; 32]),
            "height" => metadata.block_number = 42,
            "invalid" => metadata.invalid = true,
            _ => (),
        }
        if corrupt_field == "missing" {
            raw.delete(vec![key.clone()]).unwrap();
        } else {
            raw.put(vec![(key.clone(), bincode::serialize(&metadata).unwrap())])
                .unwrap();
        }
        let mut candidate = block(11, vec![head.block_hash.clone()]);
        candidate.sender = sender.clone();
        candidate.seq_num = 2;
        assert!(
            storage.insert(&candidate, InsertMode::Normal).is_err(),
            "accepted corrupt previous latest {corrupt_field}"
        );
        let after = storage.get_representation().unwrap();
        assert_eq!(after.publication_sequence(), before.publication_sequence());
        assert_eq!(
            after.latest_message_hash(&sender),
            Some(previous.block_hash.clone())
        );
        assert!(!after.contains(&candidate.block_hash).unwrap());
        assert!(raw
            .get(&vec![bincode::serialize(&BlockHashSerde(
                candidate.block_hash.clone()
            ))
            .unwrap()])
            .unwrap()[0]
            .is_none());
        raw.put(vec![(key, original)]).unwrap();
        let repaired = storage.insert(&candidate, InsertMode::Normal).unwrap();
        assert_eq!(
            repaired.latest_message_hash(&sender),
            Some(candidate.block_hash)
        );
        assert_eq!(
            repaired.publication_sequence(),
            before.publication_sequence() + 1
        );
    }
}
