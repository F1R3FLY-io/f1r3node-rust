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
use crate::rust::finality::floor::{floor_of_block, Floor};
use crate::rust::safety::clique_oracle::FtThreshold;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

/// Garbage collects mergeable channel data for blocks that are provably unreachable.
///
/// A block's mergeable data is safe to delete when:
/// 1. The block is finalized
/// 2. All validators' latest messages are descendants of the block's children
/// 3. The block is deeper than maxParentDepth + depthBuffer below the floor
pub async fn collect_garbage(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    runtime_manager: &std::sync::Arc<RuntimeManager>,
    casper_shard_conf: &CasperShardConf,
) -> Result<usize, KvStoreError> {
    let mut deleted_count = 0;

    // The deletion anchor, derived ONCE per pass: the floor of the last
    // finalized block. Deletion is irreversible and the data serves merges,
    // whose base is the floor — see `is_safe_to_delete`.
    let floor = floor_of_block(
        dag,
        block_store,
        &dag.last_finalized_block(),
        FtThreshold::from_ppm(casper_shard_conf.fault_tolerance_threshold_ppm),
    )
    .await
    .map_err(|e| KvStoreError::IoError(e.to_string()))?;

    // Get all finalized blocks by traversing from genesis
    // Note: This could be optimized by tracking pending GC blocks
    let finalized_blocks = get_finalized_blocks(dag)?;

    for block_hash in finalized_blocks {
        if is_safe_to_delete(dag, &block_hash, &floor, casper_shard_conf)? {
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
    floor: &Floor,
    casper_shard_conf: &CasperShardConf,
) -> Result<bool, KvStoreError> {
    // 1. Check if block is finalized
    if !dag.is_finalized(block_hash) {
        return Ok(false);
    }

    // 2. Depth is measured from the FLOOR, not from the tip. The data being
    //    deleted is what merges need to compute number-channel diffs, and a
    //    merge reads it for the blocks above its BASE — which is the floor, not
    //    the tip. The floor trails the tip by the time mutual citation takes
    //    (~3 blocks healthy, >100 during the observed pacification stall), so a
    //    tip-anchored bound put the whole span between the floor and
    //    `tip - max_allowed_depth` inside the deletable set while floor-based
    //    merges still needed it. Anchoring on the floor is strictly more
    //    conservative: the floor never leads the tip, so this can only delete
    //    less than before, never more.
    let block_meta = dag.lookup_unsafe(block_hash)?;
    let depth_from_floor = floor.block_number - block_meta.block_number;
    let max_allowed_depth = (casper_shard_conf.max_parent_depth as i64)
        + (casper_shard_conf.mergeable_channels_gc_depth_buffer as i64);

    if depth_from_floor <= max_allowed_depth {
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

    // For each validator's latest message, check if it's a descendant of any child (via main chain)
    for (_, latest_msg_hash) in latest_message_hashes.iter() {
        if latest_msg_hash == block_hash {
            // Validator's latest is still this block
            return Ok(false);
        }

        // Check if latest message is descendant of any child (via main chain)
        let mut found_in_child_chain = false;
        for child_hash_ref in children.iter() {
            if dag.is_in_main_chain(child_hash_ref, latest_msg_hash)? {
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
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
    use models::rust::block_metadata::BlockMetadata;
    use parking_lot::RwLock as PlRwLock;
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    use super::*;

    fn hash(n: u8) -> Bytes { Bytes::from(vec![n; 32]) }

    /// A linear finalized chain 0..=TOP by one validator, whose latest message
    /// is the tip. Everything `is_safe_to_delete` reads is populated: heights
    /// (so `latest_block_number` is real), main parents, children, the
    /// finalized set, and the latest-message map.
    const TOP: u8 = 20;

    fn linear_chain_dag() -> KeyValueDagRepresentation {
        let store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
        let mut bms = BlockMetadataStore::new(store);
        let validator = Bytes::from(vec![0xEEu8; 65]);

        let mut dag_set = imbl::HashSet::new();
        let mut block_number_map = imbl::HashMap::new();
        let mut main_parent_map = imbl::HashMap::new();
        let mut child_map: imbl::HashMap<Bytes, imbl::HashSet<Bytes>> = imbl::HashMap::new();
        let mut height_map: imbl::OrdMap<i64, imbl::HashSet<Bytes>> = imbl::OrdMap::new();
        let mut finalized_blocks_set = imbl::HashSet::new();

        for n in 0..=TOP {
            let h = hash(n);
            dag_set.insert(h.clone());
            block_number_map.insert(h.clone(), n as i64);
            finalized_blocks_set.insert(h.clone());
            let mut at_height = imbl::HashSet::new();
            at_height.insert(h.clone());
            height_map.insert(n as i64, at_height);

            let parents = if n == 0 {
                Vec::new()
            } else {
                let parent = hash(n - 1);
                main_parent_map.insert(h.clone(), parent.clone());
                let mut kids = child_map.get(&parent).cloned().unwrap_or_default();
                kids.insert(h.clone());
                child_map.insert(parent.clone(), kids);
                vec![parent]
            };

            bms.add(BlockMetadata {
                block_hash: h.clone(),
                parents,
                sender: validator.clone(),
                justifications: vec![],
                weight_map: BTreeMap::new(),
                block_number: n as i64,
                sequence_number: n as i32,
                invalid: false,
                directly_finalized: true,
                finalized: true,
                fault_tolerance_value: 1.0,
                merge_base: Bytes::new(),
                rejected_carriers: vec![],
            })
            .expect("add metadata");
        }

        let mut latest_messages_map = imbl::HashMap::new();
        latest_messages_map.insert(validator, hash(TOP));

        KeyValueDagRepresentation {
            dag_set,
            latest_messages_map,
            child_map,
            height_map,
            block_number_map,
            main_parent_map,
            self_justification_map: imbl::HashMap::new(),
            invalid_blocks_set: imbl::HashSet::new(),
            last_finalized_block_hash: hash(TOP),
            finalized_blocks_set,
            block_metadata_index: Arc::new(PlRwLock::new(bms)),
            floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            lifecycle: Arc::new(parking_lot::RwLock::new(
                block_storage::rust::dag::deploy_lifecycle_types::DeployLifecycleTables::in_memory(
                ),
            )),
        }
    }

    /// max_allowed_depth = max_parent_depth + gc buffer = 4.
    fn conf() -> CasperShardConf {
        let mut conf = CasperShardConf::new();
        conf.max_parent_depth = 3;
        conf.mergeable_channels_gc_depth_buffer = 1;
        conf
    }

    fn floor_at(n: u8) -> Floor {
        Floor {
            hash: hash(n),
            block_number: n as i64,
        }
    }

    /// THE regression. Mergeable data serves merges, and a merge reads it for
    /// the blocks above its BASE — the floor. A block ABOVE the floor is inside
    /// that span no matter how far the tip has run ahead, so its data must
    /// survive. Measuring depth from the tip deletes exactly the span between
    /// the floor and `tip - max_allowed_depth`, which is the data floor-based
    /// merges are still reading, and the floor can trail the tip by a hundred
    /// blocks during a stall.
    #[test]
    fn a_block_above_the_floor_is_never_collected_however_far_the_tip_has_run() {
        let dag = linear_chain_dag();
        let conf = conf();
        // Tip is TOP + 1 = 21, so block 12 is 9 below it — past the 4-block
        // allowance on the tip clock — but 2 ABOVE the floor at 10.
        assert_eq!(dag.latest_block_number(), TOP as i64 + 1);
        assert!(
            !is_safe_to_delete(&dag, &hash(12), &floor_at(10), &conf).expect("safety check"),
            "block 12 sits above the floor at 10, so a merge based on that floor \
             still reads its mergeable data. Anchored on the tip it reads as \
             depth 9 and is collected, and the merge then fails to find history \
             it needs",
        );
    }

    /// The discriminator against a never-collect over-correction: below the
    /// floor by more than the allowance, the data can no longer be reached by
    /// any merge and must be released.
    #[test]
    fn a_block_far_below_the_floor_is_still_collected() {
        let dag = linear_chain_dag();
        let conf = conf();
        assert!(
            is_safe_to_delete(&dag, &hash(5), &floor_at(10), &conf).expect("safety check"),
            "block 5 is 5 below the floor at 10, past the 4-block allowance, so \
             no merge can still need it and holding it forever leaks",
        );
    }

    /// The allowance is measured on the floor clock, so the boundary is exact:
    /// one block closer than the discriminator above must be retained.
    #[test]
    fn the_allowance_boundary_is_measured_from_the_floor() {
        let dag = linear_chain_dag();
        let conf = conf();
        assert!(
            !is_safe_to_delete(&dag, &hash(6), &floor_at(10), &conf).expect("safety check"),
            "block 6 is exactly the 4-block allowance below the floor — retained",
        );
    }

    /// Finality is still a precondition: an unfinalized block is never
    /// collected regardless of where the floor sits.
    #[test]
    fn an_unfinalized_block_is_never_collected() {
        let mut dag = linear_chain_dag();
        dag.finalized_blocks_set.remove(&hash(5));
        {
            let mut bms = dag.block_metadata_index.write();
            let mut meta = bms.get(&hash(5)).expect("lookup").expect("metadata");
            meta.finalized = false;
            meta.directly_finalized = false;
            bms.add(meta).expect("re-add metadata");
        }
        let conf = conf();
        assert!(
            !is_safe_to_delete(&dag, &hash(5), &floor_at(10), &conf).expect("safety check"),
            "an unfinalized block's data may still be needed by a branch that wins",
        );
    }
}
