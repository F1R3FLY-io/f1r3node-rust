//! Mergeable Channels Garbage Collection
//!
//! Garbage collects mergeable channel data for blocks that are provably unreachable.
//! This is required for multi-parent mode where immediate deletion during finalization
//! can cause data races.

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::casper::CasperShardConf;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

/// Garbage collects mergeable channel data for blocks that are provably unreachable.
///
/// A block's mergeable data is safe to delete when:
/// 1. The block is finalized
/// 2. All validators' latest messages are descendants of the block's children
/// 3. The block is deeper than maxParentDepth + depthBuffer from current tips
pub async fn collect_garbage(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    runtime_manager: &std::sync::Arc<RuntimeManager>,
    casper_shard_conf: &CasperShardConf,
) -> Result<usize, KvStoreError> {
    let mut deleted_count = 0;

    // Get all finalized blocks by traversing from genesis
    // Note: This could be optimized by tracking pending GC blocks
    let finalized_blocks = get_finalized_blocks(dag)?;

    for block_hash in finalized_blocks {
        if is_safe_to_delete(dag, &block_hash, casper_shard_conf)? {
            // Get block to access its state hash
            if let Some(block) = block_store.get(&block_hash)? {
                let deleted = runtime_manager
                    .delete_mergeable_channels(&block)
                    .map_err(|e| KvStoreError::IoError(e.to_string()))?;

                if deleted {
                    deleted_count += 1;
                    tracing::debug!(
                        "GC: Deleted mergeable data for block {}",
                        hex::encode(&block_hash)
                    );
                }
            }
        }
    }

    if deleted_count > 0 {
        metrics::counter!("mergeable_channels_gc_deleted").increment(deleted_count as u64);
        tracing::info!(
            "Mergeable channels GC: Deleted {} blocks' data",
            deleted_count
        );
    } else {
        tracing::debug!("Mergeable channels GC: No data to delete");
    }

    Ok(deleted_count)
}

/// Check if a block's mergeable data is safe to delete.
fn is_safe_to_delete(
    dag: &KeyValueDagRepresentation,
    block_hash: &BlockHash,
    casper_shard_conf: &CasperShardConf,
) -> Result<bool, KvStoreError> {
    // 1. Check if block is finalized
    if !dag.is_finalized(block_hash) {
        return Ok(false);
    }

    // 2. Check depth constraint
    let block_meta = dag.lookup_unsafe(block_hash)?;
    let parent_depth_boundary = dag.latest_block_number();
    let depth_from_tip = parent_depth_boundary - block_meta.block_number;
    let max_allowed_depth = (casper_shard_conf.max_parent_depth as i64)
        + (casper_shard_conf.mergeable_channels_gc_depth_buffer as i64);

    if depth_from_tip <= max_allowed_depth {
        return Ok(false);
    }

    // 3. Check if all validators have moved past this block
    let children = match dag.children(block_hash) {
        Some(children_set) => children_set,
        None => return Ok(false), // No children means no one can have moved past
    };

    if children.is_empty() {
        return Ok(false);
    }

    let latest_message_hashes = dag.latest_message_hashes();
    if latest_message_hashes.is_empty() {
        return Ok(false);
    }

    // For each validator's latest message, check if it's a DAG descendant of any child
    for (_, latest_msg_hash) in latest_message_hashes.iter() {
        if latest_msg_hash == block_hash {
            // Validator's latest is still this block
            return Ok(false);
        }

        let mut found_in_child_chain = false;
        for child_hash_ref in children.iter() {
            if dag.is_dag_ancestor(child_hash_ref, latest_msg_hash)? {
                found_in_child_chain = true;
                break;
            }
        }

        if !found_in_child_chain {
            return Ok(false);
        }
    }

    Ok(true)
}

/// Get all finalized blocks from the DAG.
/// Note: This is a simple implementation that could be optimized.
fn get_finalized_blocks(dag: &KeyValueDagRepresentation) -> Result<Vec<BlockHash>, KvStoreError> {
    // Get all blocks via topo_sort and filter for finalized ones
    let all_blocks = dag.topo_sort(0, None)?;

    let finalized: Vec<BlockHash> = all_blocks
        .into_iter()
        .flatten()
        .filter(|hash| dag.is_finalized(hash))
        .collect();

    Ok(finalized)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use block_storage::rust::dag::block_dag_key_value_storage::{
        BlockDagKeyValueStorage, InsertMode,
    };
    use models::rust::block_implicits::get_random_block;
    use models::rust::casper::protocol::casper_message::{BlockMessage, Bond};
    use models::rust::validator::Validator;
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::{get_finalized_blocks, is_safe_to_delete};
    use crate::rust::casper::CasperShardConf;

    fn block(
        block_number: i64,
        sequence_number: i32,
        validator: Validator,
        parent: Option<&BlockMessage>,
    ) -> BlockMessage {
        block_with_parents(
            block_number,
            sequence_number,
            validator,
            parent
                .map(|block| vec![block.block_hash.clone()])
                .unwrap_or_default(),
        )
    }

    fn block_with_parents(
        block_number: i64,
        sequence_number: i32,
        validator: Validator,
        parents: Vec<Bytes>,
    ) -> BlockMessage {
        get_random_block(
            Some(block_number),
            Some(sequence_number),
            None,
            None,
            Some(validator.clone()),
            None,
            None,
            Some(parents),
            Some(vec![]),
            None,
            None,
            Some(vec![Bond {
                validator,
                stake: 100,
            }]),
            Some("root".to_string()),
            None,
        )
    }

    async fn finalized_linear_dag() -> (
        block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation,
        Vec<BlockMessage>,
        Validator,
    ) {
        let validator = Bytes::from(vec![7; models::rust::validator::LENGTH]);
        let genesis = block(0, 0, validator.clone(), None);
        let one = block(1, 1, validator.clone(), Some(&genesis));
        let two = block(2, 2, validator.clone(), Some(&one));
        let three = block(3, 3, validator.clone(), Some(&two));
        let four = block(4, 4, validator.clone(), Some(&three));
        let blocks = vec![genesis, one, two, three, four];

        let mut manager = InMemoryStoreManager::new();
        let storage = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
        for (index, block) in blocks.iter().enumerate() {
            let mode = if index == 0 {
                InsertMode::ApprovedGenesis
            } else {
                InsertMode::Normal
            };
            storage.insert(block, mode).unwrap();
        }
        storage
            .record_directly_finalized(blocks[1].block_hash.clone(), 1.0, |_| async { Ok(()) })
            .await
            .unwrap();

        (storage.get_representation().unwrap(), blocks, validator)
    }

    #[tokio::test]
    async fn finalized_enumeration_excludes_non_finalized_blocks() {
        let (dag, blocks, _) = finalized_linear_dag().await;
        let finalized = get_finalized_blocks(&dag)
            .unwrap()
            .into_iter()
            .collect::<HashSet<_>>();

        assert_eq!(
            finalized,
            HashSet::from([blocks[0].block_hash.clone(), blocks[1].block_hash.clone()])
        );
    }

    #[tokio::test]
    async fn deletion_safety_requires_every_finality_depth_and_reachability_guard() {
        let (dag, blocks, validator) = finalized_linear_dag().await;
        let target = blocks[1].block_hash.clone();
        let mut conf = CasperShardConf::new();
        conf.max_parent_depth = 1;
        conf.mergeable_channels_gc_depth_buffer = 1;

        assert_eq!(dag.latest_block_number(), 5);
        assert_eq!(dag.lookup_unsafe(&target).unwrap().block_number, 1);
        assert!(dag.is_finalized(&target));
        let children = dag.children(&target).unwrap();
        assert!(!children.is_empty());
        let latest_messages = dag.latest_message_hashes();
        assert!(!latest_messages.is_empty());
        for latest_message in latest_messages.values() {
            assert_ne!(latest_message, &target);
            assert!(children
                .iter()
                .any(|child| dag.is_in_main_chain(child, latest_message).unwrap()));
        }
        assert!(is_safe_to_delete(&dag, &target, &conf).unwrap());
        assert!(!is_safe_to_delete(&dag, &blocks[2].block_hash, &conf).unwrap());

        let mut within_horizon = conf.clone();
        within_horizon.max_parent_depth = 3;
        assert!(!is_safe_to_delete(&dag, &target, &within_horizon).unwrap());

        let mut missing_children = dag.clone();
        missing_children.child_map.remove(&target);
        assert!(!is_safe_to_delete(&missing_children, &target, &conf).unwrap());

        let mut empty_children = dag.clone();
        empty_children
            .child_map
            .insert(target.clone(), Default::default());
        assert!(!is_safe_to_delete(&empty_children, &target, &conf).unwrap());

        let mut latest_at_target = dag.clone();
        latest_at_target
            .latest_messages_map
            .insert(validator.clone(), target.clone());
        assert!(!is_safe_to_delete(&latest_at_target, &target, &conf).unwrap());

        let mut no_latest_witness = dag.clone();
        no_latest_witness.latest_messages_map.clear();
        assert!(!is_safe_to_delete(&no_latest_witness, &target, &conf).unwrap());

        let mut latest_outside_child_chain = dag.clone();
        latest_outside_child_chain
            .latest_messages_map
            .insert(validator.clone(), blocks[0].block_hash.clone());
        assert!(!is_safe_to_delete(&latest_outside_child_chain, &target, &conf).unwrap());

        let mut missing_latest = dag;
        missing_latest
            .latest_messages_map
            .insert(validator, Bytes::from(vec![0xff; 32]));
        assert!(is_safe_to_delete(&missing_latest, &target, &conf).is_err());
    }

    #[tokio::test]
    async fn secondary_parent_advancement_is_sufficient_for_retirement() {
        let validator = Bytes::from(vec![9; models::rust::validator::LENGTH]);
        let genesis = block(0, 0, validator.clone(), None);
        let target = block(1, 1, validator.clone(), Some(&genesis));
        let target_child = block(2, 2, validator.clone(), Some(&target));
        let main_sibling = block(2, 3, validator.clone(), Some(&genesis));
        let merged = block_with_parents(3, 4, validator, vec![
            main_sibling.block_hash.clone(),
            target_child.block_hash.clone(),
        ]);

        let mut manager = InMemoryStoreManager::new();
        let storage = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
        for (index, block) in [&genesis, &target, &target_child, &main_sibling, &merged]
            .into_iter()
            .enumerate()
        {
            let mode = if index == 0 {
                InsertMode::ApprovedGenesis
            } else {
                InsertMode::Normal
            };
            storage.insert(block, mode).unwrap();
        }
        storage
            .record_directly_finalized(target.block_hash.clone(), 1.0, |_| async { Ok(()) })
            .await
            .unwrap();

        let dag = storage.get_representation().unwrap();
        let mut conf = CasperShardConf::new();
        conf.max_parent_depth = 1;
        conf.mergeable_channels_gc_depth_buffer = 1;

        assert!(!dag
            .is_in_main_chain(&target_child.block_hash, &merged.block_hash)
            .unwrap());
        assert!(dag
            .is_dag_ancestor(&target_child.block_hash, &merged.block_hash)
            .unwrap());
        assert!(is_safe_to_delete(&dag, &target.block_hash, &conf).unwrap());
    }
}
