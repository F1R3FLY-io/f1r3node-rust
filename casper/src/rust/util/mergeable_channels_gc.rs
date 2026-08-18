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
/// 2. The block is deeper than maxParentDepth + depthBuffer from current tips
/// 3. Every validator's latest message sits strictly above it on the main chain
pub async fn collect_garbage(
    sweep: &mut GcSweep,
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    runtime_manager: &std::sync::Arc<RuntimeManager>,
    casper_shard_conf: &CasperShardConf,
) -> Result<usize, KvStoreError> {
    let mut deleted_count = 0;

    enumerate_newly_in_range(sweep, dag, casper_shard_conf);
    metrics::gauge!("mergeable_channels_gc_pending").set(sweep.pending.len() as f64);

    let common_strict_ancestors = common_strict_main_chain_ancestors(dag);
    let mut collected = Vec::new();

    for block_hash in sweep.pending.iter() {
        if is_safe_to_delete(
            dag,
            block_hash,
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
/// The ceiling is a coarse upper bound on what is worth looking at, not a
/// second definition of the horizon — `is_safe_to_delete` remains the only
/// place the `max_parent_depth + gc_depth_buffer` boundary is expressed, since
/// that same distance also governs the LFS forward-horizon window a joiner
/// syncs.
fn enumerate_newly_in_range(
    sweep: &mut GcSweep,
    dag: &KeyValueDagRepresentation,
    casper_shard_conf: &CasperShardConf,
) {
    let max_allowed_depth = (casper_shard_conf.max_parent_depth as i64)
        + (casper_shard_conf.mergeable_channels_gc_depth_buffer as i64);

    extend_pending_to_ceiling(
        sweep,
        dag.latest_block_number() - max_allowed_depth,
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
}
