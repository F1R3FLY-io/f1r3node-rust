//! Mergeable Channels Garbage Collection
//!
//! Garbage collects mergeable channel data for blocks that are provably unreachable.
//! This is required for multi-parent mode where immediate deletion during finalization
//! can cause data races.

use std::collections::HashSet;
use std::ops::Bound;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use shared::rust::store::key_value_store::KvStoreError;

use crate::rust::casper::CasperShardConf;
use crate::rust::finality::floor::{floor_of_block, Floor};
use crate::rust::safety::clique_oracle::FtThreshold;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

/// Sweep state carried across garbage-collection passes.
///
/// Each pass enumerates only the heights that have come into range since the
/// last one, and keeps whatever it could not yet delete. Blocks are therefore
/// enumerated once and retried until they qualify, rather than the whole DAG
/// being re-derived every pass.
#[derive(Debug, Default)]
pub struct GcSweep {
    /// Highest height already enumerated. `None` before the first pass, so that
    /// genesis is included.
    swept_height: Option<i64>,
    /// Enumerated but not yet deletable. Retried on every subsequent pass; its
    /// size tracks how far finality is behind the deletion horizon.
    pending: HashSet<BlockHash>,
}

impl GcSweep {
    pub fn new() -> Self { Self::default() }

    pub fn pending_len(&self) -> usize { self.pending.len() }
}

/// Garbage collects mergeable channel data for blocks that are provably unreachable.
///
/// A block's mergeable data is safe to delete when:
/// 1. The block is finalized
/// 2. The block is deeper than maxParentDepth + depthBuffer below the floor
/// 3. Every validator's latest message sits strictly above it on the main chain
pub async fn collect_garbage(
    sweep: &mut GcSweep,
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
        &dag.last_finalized_block(),
        FtThreshold::from_ppm(casper_shard_conf.fault_tolerance_threshold_ppm),
    )
    .await
    .map_err(|e| KvStoreError::IoError(e.to_string()))?;

    // The sweep shares that anchor. A tip-anchored ceiling would enumerate the
    // whole span between the floor and the tip — blocks `is_safe_to_delete`
    // refuses on every pass, which is the work the sweep exists to avoid.
    enumerate_newly_in_range(sweep, dag, &floor, casper_shard_conf);
    metrics::gauge!("mergeable_channels_gc_pending").set(sweep.pending.len() as f64);

    let common_strict_ancestors = common_strict_main_chain_ancestors(dag);
    let mut collected = Vec::new();

    for block_hash in sweep.pending.iter() {
        if is_safe_to_delete(
            dag,
            block_hash,
            &floor,
            casper_shard_conf,
            common_strict_ancestors.as_ref(),
        )? {
            collected.push(block_hash.clone());
        }
    }

    for block_hash in collected {
        sweep.pending.remove(&block_hash);

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
    common_strict_ancestors: Option<&HashSet<BlockHash>>,
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

    // No latest messages means nothing is known to have moved past this block,
    // which is a reason to keep the data rather than to delete it.
    let Some(ancestors) = common_strict_ancestors else {
        return Ok(false);
    };

    if !ancestors.contains(block_hash) {
        return Ok(false);
    }

    Ok(true)
}

/// Add every block whose height has come into deletion range since the last
/// pass, and advance the sweep.
///
/// Enumeration deliberately does not filter on finality. A block below the
/// ceiling can still be unfinalized while finality lags, and skipping it here
/// would move the sweep past it for good; leaving it to condition 1 of
/// `is_safe_to_delete` means it is simply retried until it qualifies.
///
/// The ceiling is a coarse upper bound on what is worth looking at, and it is
/// anchored on the floor for the same reason `is_safe_to_delete` is: a
/// tip-anchored ceiling would sweep in the entire span between the floor and the
/// tip, which the predicate refuses on every pass.
///
/// It restates the `max_parent_depth + gc_depth_buffer` distance that the
/// predicate also computes, which is a duplication worth removing — that
/// distance is the LFS forward-horizon window a joiner syncs, so the two must
/// not drift. `is_safe_to_delete` stays the authority: enumeration only has to
/// over-approximate, and the boundary block it includes is one the predicate
/// then declines.
fn enumerate_newly_in_range(
    sweep: &mut GcSweep,
    dag: &KeyValueDagRepresentation,
    floor: &Floor,
    casper_shard_conf: &CasperShardConf,
) {
    let max_allowed_depth = (casper_shard_conf.max_parent_depth as i64)
        + (casper_shard_conf.mergeable_channels_gc_depth_buffer as i64);

    extend_pending_to_ceiling(
        sweep,
        floor.block_number - max_allowed_depth,
        &dag.height_map,
    );
}

fn extend_pending_to_ceiling(
    sweep: &mut GcSweep,
    ceiling: i64,
    height_map: &imbl::OrdMap<i64, imbl::HashSet<BlockHash>>,
) {
    if sweep.swept_height.is_some_and(|swept| ceiling <= swept) {
        return;
    }

    let lower = match sweep.swept_height {
        Some(swept) => Bound::Excluded(swept),
        None => Bound::Unbounded,
    };

    for (_, hashes) in height_map.range((lower, Bound::Included(ceiling))) {
        sweep.pending.extend(hashes.iter().cloned());
    }

    sweep.swept_height = Some(ceiling);
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    /// Named hashes for the ancestor tests, which care about chain shape rather
    /// than height; the height-keyed fixtures below use `hash(n)` instead.
    fn named_hash(value: &'static [u8]) -> BlockHash { BlockHash::from_static(value) }

    #[test]
    fn intersects_strict_main_chain_ancestors() {
        let genesis = named_hash(b"genesis");
        let common = named_hash(b"common");
        let left = named_hash(b"left");
        let right = named_hash(b"right");
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
        let genesis = named_hash(b"genesis");
        let latest = named_hash(b"latest");
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

    /// One block per height, hash `b"h<n>"`, for heights `0..=top`.
    fn height_map(top: i64) -> imbl::OrdMap<i64, imbl::HashSet<BlockHash>> {
        (0..=top)
            .map(|n| {
                let hash = BlockHash::from(format!("h{}", n).into_bytes());
                (n, imbl::HashSet::unit(hash))
            })
            .collect()
    }

    fn at_height(n: i64) -> BlockHash { BlockHash::from(format!("h{}", n).into_bytes()) }

    #[test]
    fn enumerates_everything_up_to_the_ceiling_on_the_first_pass() {
        let mut sweep = GcSweep::new();

        extend_pending_to_ceiling(&mut sweep, 3, &height_map(10));

        // Genesis is included: the sweep has no lower bound before its first pass.
        assert_eq!(sweep.pending.len(), 4);
        assert!(sweep.pending.contains(&at_height(0)));
        assert!(sweep.pending.contains(&at_height(3)));
        assert!(!sweep.pending.contains(&at_height(4)));
        assert_eq!(sweep.swept_height, Some(3));
    }

    #[test]
    fn second_pass_enumerates_only_what_newly_came_into_range() {
        let map = height_map(10);
        let mut sweep = GcSweep::new();
        extend_pending_to_ceiling(&mut sweep, 3, &map);
        sweep.pending.clear(); // stand in for the first pass having deleted them

        extend_pending_to_ceiling(&mut sweep, 5, &map);

        // Heights 0..=3 are not re-enumerated; only 4 and 5 are new.
        assert_eq!(sweep.pending.len(), 2);
        assert!(sweep.pending.contains(&at_height(4)));
        assert!(sweep.pending.contains(&at_height(5)));
        assert_eq!(sweep.swept_height, Some(5));
    }

    #[test]
    fn a_pass_that_adds_no_range_leaves_pending_untouched() {
        let map = height_map(10);
        let mut sweep = GcSweep::new();
        extend_pending_to_ceiling(&mut sweep, 5, &map);
        let after_first = sweep.pending.clone();

        // The tip has not advanced, so the ceiling has not moved.
        extend_pending_to_ceiling(&mut sweep, 5, &map);
        assert_eq!(sweep.pending, after_first);

        // A ceiling that moved backwards must not rewind the sweep either.
        extend_pending_to_ceiling(&mut sweep, 2, &map);
        assert_eq!(sweep.pending, after_first);
        assert_eq!(sweep.swept_height, Some(5));
    }

    #[test]
    fn a_block_refused_this_pass_stays_pending_for_the_next() {
        let map = height_map(10);
        let mut sweep = GcSweep::new();
        extend_pending_to_ceiling(&mut sweep, 2, &map);

        // `collect_garbage` removes only what it deleted; a refusal leaves the
        // entry in place, so the block is retried rather than enumerated again.
        sweep.pending.remove(&at_height(0));

        extend_pending_to_ceiling(&mut sweep, 4, &map);

        assert!(
            !sweep.pending.contains(&at_height(0)),
            "deleted block is gone"
        );
        assert!(
            sweep.pending.contains(&at_height(1)),
            "refused block is retried"
        );
        assert!(
            sweep.pending.contains(&at_height(2)),
            "refused block is retried"
        );
    }

    #[test]
    fn enumeration_does_not_depend_on_the_finalized_block_cache() {
        // Heights are the only input: `finalized_blocks_set` is a bounded cache
        // that evicts, so a block missing from it must still be enumerated and
        // left for condition 1 of `is_safe_to_delete` to judge.
        let mut sweep = GcSweep::new();

        extend_pending_to_ceiling(&mut sweep, 6, &height_map(20));

        assert_eq!(sweep.pending.len(), 7);
    }
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
    use models::rust::block_metadata::BlockMetadata;
    use parking_lot::RwLock as PlRwLock;
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    use super::*;
    use crate::rust::finality::floor::Floor;

    const TOP: u8 = 20;

    fn hash(n: u8) -> Bytes { Bytes::from(vec![n; 32]) }

    fn linear_chain_dag() -> KeyValueDagRepresentation {
        let store = KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new()));
        let mut metadata_store = BlockMetadataStore::new(store);
        let validator = Bytes::from(vec![0xee; 65]);

        let mut dag_set = imbl::HashSet::new();
        let mut block_number_map = imbl::HashMap::new();
        let mut main_parent_map = imbl::HashMap::new();
        let mut child_map: imbl::HashMap<Bytes, imbl::HashSet<Bytes>> = imbl::HashMap::new();
        let mut height_map: imbl::OrdMap<i64, imbl::HashSet<Bytes>> = imbl::OrdMap::new();
        let mut finalized_blocks_set = imbl::HashSet::new();

        for n in 0..=TOP {
            let block_hash = hash(n);
            dag_set.insert(block_hash.clone());
            block_number_map.insert(block_hash.clone(), n as i64);
            finalized_blocks_set.insert(block_hash.clone());
            height_map.insert(n as i64, imbl::HashSet::unit(block_hash.clone()));

            let parents = if n == 0 {
                Vec::new()
            } else {
                let parent = hash(n - 1);
                main_parent_map.insert(block_hash.clone(), parent.clone());
                child_map
                    .entry(parent.clone())
                    .or_default()
                    .insert(block_hash.clone());
                vec![parent]
            };

            metadata_store
                .add(BlockMetadata {
                    block_hash: block_hash.clone(),
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
                })
                .expect("add metadata");
        }

        KeyValueDagRepresentation {
            dag_set,
            latest_messages_map: imbl::HashMap::unit(validator, hash(TOP)),
            child_map,
            height_map,
            block_number_map,
            main_parent_map,
            self_justification_map: imbl::HashMap::new(),
            invalid_blocks_set: imbl::HashSet::new(),
            last_finalized_block_hash: hash(TOP),
            finalized_blocks_set,
            block_metadata_index: Arc::new(PlRwLock::new(metadata_store)),
            deploy_index: Arc::new(PlRwLock::new(KeyValueTypedStoreImpl::new(Arc::new(
                InMemoryKeyValueStore::new(),
            )))),
            floor_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            frontier_index: KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        }
    }

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

    fn is_safe_to_delete_at_floor(
        dag: &KeyValueDagRepresentation,
        block_hash: &BlockHash,
        floor: &Floor,
        conf: &CasperShardConf,
    ) -> Result<bool, KvStoreError> {
        // Derived from the DAG exactly as `collect_garbage` does, so the depth
        // clause is exercised against a real citation set rather than a stub
        // that could refuse for the wrong reason.
        let common_strict_ancestors = common_strict_main_chain_ancestors(dag);
        is_safe_to_delete(
            dag,
            block_hash,
            floor,
            conf,
            common_strict_ancestors.as_ref(),
        )
    }

    #[test]
    fn a_block_above_the_floor_is_never_collected_however_far_the_tip_has_run() {
        let dag = linear_chain_dag();
        let conf = conf();
        assert_eq!(dag.latest_block_number(), TOP as i64 + 1);
        assert!(
            !is_safe_to_delete_at_floor(&dag, &hash(12), &floor_at(10), &conf)
                .expect("safety check"),
            "a block above the floor must retain its mergeable-channel data"
        );
    }
}
