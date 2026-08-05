//! Mergeable Channels Garbage Collection
//!
//! Garbage collects mergeable channel data for blocks that are provably unreachable.
//! This is required for multi-parent mode where immediate deletion during finalization
//! can cause data races.

use std::collections::HashSet;

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

    let finalized_blocks = get_finalized_blocks(dag)?;
    let common_strict_ancestors = common_strict_main_chain_ancestors(dag);

    for block_hash in finalized_blocks {
        if is_safe_to_delete(
            dag,
            &block_hash,
            casper_shard_conf,
            common_strict_ancestors.as_ref(),
        )? {
            // Get block to access its state hash
            if let Some(block) = block_store.get(&block_hash)? {
                let deleted = runtime_manager
                    .delete_mergeable_channels(
                        &block.body.state.post_state_hash,
                        block.sender.clone(),
                        block.seq_num,
                    )
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
    common_strict_ancestors: Option<&HashSet<BlockHash>>,
) -> Result<bool, KvStoreError> {
    // 1. Check if block is finalized
    if !dag.is_finalized(block_hash) {
        return Ok(false);
    }

    // 2. Check depth constraint
    let block_meta = dag.lookup_unsafe(block_hash)?;
    let max_block_number = dag.latest_block_number();
    let depth_from_tip = max_block_number - block_meta.block_number;
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

    if common_strict_ancestors.is_some_and(|ancestors| !ancestors.contains(block_hash)) {
        return Ok(false);
    }

    Ok(true)
}

fn common_strict_main_chain_ancestors(
    dag: &KeyValueDagRepresentation,
) -> Option<HashSet<BlockHash>> {
    common_strict_ancestors(
        dag.latest_message_hashes().values().cloned(),
        |block_hash| dag.main_parent(block_hash),
    )
}

fn common_strict_ancestors(
    latest_messages: impl IntoIterator<Item = BlockHash>,
    main_parent: impl Fn(&BlockHash) -> Option<BlockHash>,
) -> Option<HashSet<BlockHash>> {
    latest_messages
        .into_iter()
        .map(|latest_message| {
            let mut ancestors = HashSet::new();
            let mut current = latest_message;
            while let Some(parent) = main_parent(&current) {
                if !ancestors.insert(parent.clone()) {
                    break;
                }
                current = parent;
            }
            ancestors
        })
        .reduce(|common, ancestors| common.intersection(&ancestors).cloned().collect())
}

/// Get all finalized blocks from the DAG.
/// Note: This is a simple implementation that could be optimized.
fn get_finalized_blocks(dag: &KeyValueDagRepresentation) -> Result<Vec<BlockHash>, KvStoreError> {
    Ok(dag.finalized_blocks_set.iter().cloned().collect())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn hash(value: &'static [u8]) -> BlockHash { BlockHash::from_static(value) }

    #[test]
    fn intersects_strict_main_chain_ancestors() {
        let genesis = hash(b"genesis");
        let common = hash(b"common");
        let left = hash(b"left");
        let right = hash(b"right");
        let parents = HashMap::from([
            (common.clone(), genesis.clone()),
            (left.clone(), common.clone()),
            (right.clone(), common.clone()),
        ]);

        let ancestors =
            common_strict_ancestors([left, right], |block_hash| parents.get(block_hash).cloned());

        assert_eq!(ancestors, Some(HashSet::from([common, genesis])));
    }

    #[test]
    fn excludes_latest_messages_from_strict_ancestors() {
        let genesis = hash(b"genesis");
        let latest = hash(b"latest");
        let parents = HashMap::from([(latest.clone(), genesis.clone())]);

        let ancestors =
            common_strict_ancestors([latest], |block_hash| parents.get(block_hash).cloned());

        assert_eq!(ancestors, Some(HashSet::from([genesis])));
    }

    #[test]
    fn returns_none_without_latest_messages() {
        let latest_messages: [BlockHash; 0] = [];
        let ancestors = common_strict_ancestors(latest_messages, |_| None);

        assert_eq!(ancestors, None);
    }
}
