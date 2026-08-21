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
use crate::rust::metrics_constants::MERGEABLE_CHANNELS_GC_METRICS_SOURCE;
use crate::rust::safety::clique_oracle::FtThreshold;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

/// Sweep state carried across garbage-collection passes.
///
/// Each pass enumerates only the heights that have come into range since the
/// last one, and keeps whatever it could not yet delete. A block that never
/// clears — an orphan on a losing fork, permanently unfinalized — is retried
/// forever at the cost of one set entry; it cannot stop the sweep from moving
/// on to blocks that do clear, unlike a scheme that only advances a watermark
/// once an entire height is fully resolved.
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
    let pass_started = std::time::Instant::now();
    let mut deleted_count = 0;

    // The deletion anchor, derived ONCE per pass: the floor of the last
    // finalized block. Deletion is irreversible and the data serves merges,
    // whose scope is bounded below by the floor — see `is_safe_to_delete`.
    let floor = floor_of_block(
        dag,
        block_store,
        &dag.last_finalized_block(),
        FtThreshold::from_ppm(casper_shard_conf.fault_tolerance_threshold_ppm),
    )
    .await
    .map_err(|e| KvStoreError::IoError(e.to_string()))?;

    // The sweep shares that anchor. A tip-anchored ceiling would enumerate the
    // whole span between the floor and the tip — blocks `is_safe_to_delete`
    // refuses on every pass, which is the sweep exists to avoid doing twice.
    enumerate_newly_in_range(sweep, dag, &floor, casper_shard_conf);
    metrics::gauge!("mergeable_channels_gc_pending").set(sweep.pending.len() as f64);

    // The ancestor walk only ever needs to reach as low as the oldest
    // candidate this pass can actually test against it. A candidate that
    // fails `reaches_ancestor_check` — an orphan that will never finalize,
    // most commonly — is refused before `common_strict_ancestors` is ever
    // consulted, so its height must not drag the walk down either; only
    // candidates that clear that bar can pull `min_height` lower.
    let mut min_height: Option<i64> = None;
    for hash in sweep.pending.iter() {
        if reaches_ancestor_check(dag, hash, &floor, casper_shard_conf)? {
            if let Ok(height) = dag.block_number_unsafe(hash) {
                min_height = Some(min_height.map_or(height, |m| m.min(height)));
            }
        }
    }
    // Depth, not raw height: a stable value here means the ancestor walk's
    // window is stable. A value that grows over a soak run means some
    // eligible candidate — finalized, deep enough, has children — is stuck
    // outside `common_strict_ancestors` and is the one holding the walk back.
    metrics::histogram!("mergeable_channels_gc.oldest_eligible_pending_depth")
        .record(min_height.map_or(0, |h| floor.block_number - h) as f64);

    let common_strict_ancestors =
        min_height.and_then(|min_height| common_strict_main_chain_ancestors(dag, min_height));
    metrics::histogram!("mergeable_channels_gc.ancestor_set_size")
        .record(common_strict_ancestors.as_ref().map_or(0, |a| a.len()) as f64);
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
        // Removed from `pending` only after storage confirms the attempt —
        // not before. `sweep` is the caller's own persistent state, not a
        // local copy: an early return from a `?` below would otherwise leave
        // this entry evicted from `pending` with its deletion status
        // unknown, and nothing ever retries it again.
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
        sweep.pending.remove(&block_hash);
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

    metrics::histogram!("mergeable_channels_gc.pass.time", "source" => MERGEABLE_CHANNELS_GC_METRICS_SOURCE)
        .record(pass_started.elapsed().as_secs_f64());

    Ok(deleted_count)
}

/// Conditions 1 and 2 of `is_safe_to_delete`, plus the "has children" half of
/// condition 3 — everything that can be decided about a candidate WITHOUT
/// consulting `common_strict_ancestors`. Shared with the caller that bounds
/// the ancestor walk: a candidate that fails here never reaches the
/// ancestor-membership test, so it must never be allowed to pull that walk's
/// depth down either. One predicate, so the two can't drift apart.
fn reaches_ancestor_check(
    dag: &KeyValueDagRepresentation,
    block_hash: &BlockHash,
    floor: &Floor,
    casper_shard_conf: &CasperShardConf,
) -> Result<bool, KvStoreError> {
    // 1. Check if block is finalized. An orphan on a losing fork never
    //    satisfies this — permanently, not just for this pass — and that is
    //    fine: it stays in `pending` and is retried, but never blocks any
    //    other block's own check from succeeding.
    if !dag.is_finalized(block_hash) {
        return Ok(false);
    }

    // 2. Depth is measured from the FLOOR, not from the tip. The data being
    //    deleted is what merges need to compute number-channel diffs, and the
    //    floor is what bounds how deep a merge reaches: the base's own lineage
    //    walk stops at it, and a base never sits below it. The floor trails the
    //    tip by the time mutual citation takes (~3 blocks healthy, >100 during
    //    the observed pacification stall), so a tip-anchored bound put the
    //    whole span between the floor and `tip - max_allowed_depth` inside the
    //    deletable set while merges still needed it. Anchoring on the floor is
    //    strictly more conservative: the floor never leads the tip, so this can
    //    only delete less than before, never more.
    let block_meta = dag.lookup_unsafe(block_hash)?;
    let depth_from_floor = floor.block_number - block_meta.block_number;
    let max_allowed_depth = (casper_shard_conf.max_parent_depth as i64)
        + (casper_shard_conf.mergeable_channels_gc_depth_buffer as i64);

    if depth_from_floor <= max_allowed_depth {
        return Ok(false);
    }

    // Every validator must have moved past this block on their own main
    // chain — but that is only checkable once it has children at all.
    match dag.children(block_hash) {
        Some(children_set) => Ok(!children_set.is_empty()),
        None => Ok(false), // No children means no one can have moved past
    }
}

/// Check if a block's mergeable data is safe to delete.
fn is_safe_to_delete(
    dag: &KeyValueDagRepresentation,
    block_hash: &BlockHash,
    floor: &Floor,
    casper_shard_conf: &CasperShardConf,
    common_strict_ancestors: Option<&HashSet<BlockHash>>,
) -> Result<bool, KvStoreError> {
    if !reaches_ancestor_check(dag, block_hash, floor, casper_shard_conf)? {
        return Ok(false);
    }

    // Every validator's latest message must sit strictly above this block on
    // their own main chain. `common_strict_ancestors` is the intersection,
    // over every validator's latest message, of that validator's strict
    // main-parent lineage — computed once per pass, not once per candidate.
    // No latest messages means nothing is known to have moved past this
    // block, which is a reason to keep the data rather than to delete it.
    let Some(ancestors) = common_strict_ancestors else {
        return Ok(false);
    };

    Ok(ancestors.contains(block_hash))
}

/// Add every block whose height has come into deletion range since the last
/// pass, and advance the sweep.
///
/// Enumeration deliberately does not filter on finality. A block below the
/// ceiling can still be unfinalized while finality lags, and skipping it here
/// would move the sweep past it for good; leaving it to condition 1 of
/// `is_safe_to_delete` means it is simply retried until it qualifies — or, for
/// an orphan that never finalizes, retried forever at the cost of one entry,
/// never blocking anything else.
///
/// The ceiling is a coarse upper bound on what is worth looking at, and it is
/// anchored on the floor for the same reason `is_safe_to_delete` is: a
/// tip-anchored ceiling would sweep in the entire span between the floor and the
/// tip, which the predicate refuses on every pass.
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

/// The intersection, over every validator's latest message, of that
/// validator's own strict main-parent lineage. Computed once per pass — the
/// per-candidate cost this replaces was the quadratic term in the original
/// O(chain × validators × depth) scan. Walks each validator's main-parent
/// chain in memory (`imbl` lookups, not LMDB reads).
///
/// Stops each validator's walk once it reaches `min_height` — the lowest
/// height any candidate in `pending` can have. A candidate's own height is
/// always >= that minimum by construction, so nothing that could ever be
/// tested against this set sits below where the walk stops.
///
/// This is NOT the retention-window ceiling: unlike the old watermark, which
/// only ever pointed at heights already fully resolved, `pending` here can
/// hold a block arbitrarily far below the current window — an orphan that
/// has sat there for the chain's whole life. Bounding by the window instead
/// of by `pending`'s own floor would make such a block un-checkable forever,
/// silently defeating the sweep's entire reason for retrying it. `min_height`
/// is derived by the caller from `reaches_ancestor_check`-eligible candidates
/// only, so a permanently-unfinalized orphan — refused before this set is
/// ever consulted — does not drag the walk down either.
fn common_strict_main_chain_ancestors(
    dag: &KeyValueDagRepresentation,
    min_height: i64,
) -> Option<HashSet<BlockHash>> {
    // Validators sharing the same latest message (common on a healthy,
    // synchronized chain) would otherwise walk that same lineage once per
    // validator instead of once total.
    let latest_messages: HashSet<BlockHash> =
        dag.latest_message_hashes().values().cloned().collect();

    common_strict_ancestors(latest_messages, |block_hash| {
        let parent = dag.main_parent(block_hash)?;
        // An unknown height is treated as "stop here" rather than
        // propagating an error — the same forgiving posture as `main_parent`
        // returning `None`, and for the same reason: one validator's
        // incomplete lineage must not fail the whole pass for every
        // candidate the intersection is checked against.
        (dag.block_number_unsafe(&parent).ok()? >= min_height).then_some(parent)
    })
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
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use block_storage::rust::dag::block_metadata_store::BlockMetadataStore;
    use models::rust::block_metadata::BlockMetadata;
    use parking_lot::RwLock as PlRwLock;
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    use super::*;

    // -- enumeration / sweep bookkeeping -----------------------------------

    fn at_height(n: i64) -> BlockHash { BlockHash::from(format!("h{}", n).into_bytes()) }

    fn height_map(top: i64) -> imbl::OrdMap<i64, imbl::HashSet<BlockHash>> {
        (0..=top)
            .map(|n| {
                let mut set = imbl::HashSet::new();
                set.insert(at_height(n));
                (n, set)
            })
            .collect()
    }

    #[test]
    fn enumerates_everything_up_to_the_ceiling_on_the_first_pass() {
        let mut sweep = GcSweep::new();
        extend_pending_to_ceiling(&mut sweep, 3, &height_map(10));

        assert_eq!(sweep.pending.len(), 4);
        assert!(sweep.pending.contains(&at_height(0)));
        assert!(sweep.pending.contains(&at_height(3)));
        assert!(!sweep.pending.contains(&at_height(4)));
    }

    #[test]
    fn second_pass_enumerates_only_what_newly_came_into_range() {
        let map = height_map(10);
        let mut sweep = GcSweep::new();
        extend_pending_to_ceiling(&mut sweep, 3, &map);
        sweep.pending.clear(); // stand in for the first pass having deleted them

        extend_pending_to_ceiling(&mut sweep, 5, &map);

        assert_eq!(sweep.pending.len(), 2);
        assert!(sweep.pending.contains(&at_height(4)));
        assert!(sweep.pending.contains(&at_height(5)));
    }

    #[test]
    fn a_pass_that_adds_no_range_leaves_pending_untouched() {
        let map = height_map(10);
        let mut sweep = GcSweep::new();
        extend_pending_to_ceiling(&mut sweep, 5, &map);
        let after_first = sweep.pending.clone();

        extend_pending_to_ceiling(&mut sweep, 5, &map);
        assert_eq!(sweep.pending, after_first);

        extend_pending_to_ceiling(&mut sweep, 2, &map);
        assert_eq!(sweep.pending, after_first);
    }

    /// THE regression this sweep design exists for: a block a pass can't yet
    /// clear (here, standing in for an orphan that never finalizes) does not
    /// stop that same pass, or any later one, from enumerating and retrying
    /// everything else. The old level/watermark design advanced only when an
    /// entire height was fully resolved — one permanently-unresolvable block
    /// froze it there forever.
    #[test]
    fn a_block_refused_this_pass_stays_pending_for_the_next() {
        let map = height_map(10);
        let mut sweep = GcSweep::new();
        extend_pending_to_ceiling(&mut sweep, 2, &map);

        // Only block 0 clears this pass; block 1 and 2 stay pending — one of
        // them standing in for a block that will never clear.
        sweep.pending.remove(&at_height(0));

        extend_pending_to_ceiling(&mut sweep, 4, &map);

        assert!(
            !sweep.pending.contains(&at_height(0)),
            "already-cleared block stays cleared"
        );
        assert!(
            sweep.pending.contains(&at_height(1)),
            "never-cleared block is retried, not dropped"
        );
        assert!(
            sweep.pending.contains(&at_height(2)),
            "never-cleared block is retried, not dropped"
        );
        assert!(
            sweep.pending.contains(&at_height(3)) && sweep.pending.contains(&at_height(4)),
            "enumeration still advanced past the stuck blocks"
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

    // -- common_strict_ancestors --------------------------------------------

    fn named_hash(value: &'static [u8]) -> BlockHash { BlockHash::from_static(value) }

    #[test]
    fn intersects_strict_main_chain_ancestors() {
        use std::collections::HashMap;

        let a = named_hash(b"a");
        let b = named_hash(b"b");
        let c = named_hash(b"c");
        let genesis = named_hash(b"genesis");

        // validator 1's lineage: c -> b -> a -> genesis
        // validator 2's lineage: c -> b -> genesis (diverges below b)
        let parents: HashMap<BlockHash, BlockHash> =
            [(c.clone(), b.clone()), (b.clone(), genesis.clone())]
                .into_iter()
                .collect();
        let parents2: HashMap<BlockHash, BlockHash> =
            [(a.clone(), genesis.clone())].into_iter().collect();

        let ancestors = common_strict_ancestors([c.clone(), c.clone()], |h| {
            parents.get(h).or_else(|| parents2.get(h)).cloned()
        })
        .expect("non-empty input yields a set");

        assert!(ancestors.contains(&b));
        assert!(ancestors.contains(&genesis));
    }

    #[test]
    fn excludes_latest_messages_from_strict_ancestors() {
        let tip = named_hash(b"tip");
        let parent = named_hash(b"parent");
        let parents: std::collections::HashMap<BlockHash, BlockHash> =
            [(tip.clone(), parent.clone())].into_iter().collect();

        let ancestors = common_strict_ancestors([tip.clone()], |h| parents.get(h).cloned())
            .expect("non-empty input yields a set");

        assert!(
            !ancestors.contains(&tip),
            "the latest message itself is not its own ancestor"
        );
        assert!(ancestors.contains(&parent));
    }

    #[test]
    fn returns_none_without_latest_messages() {
        assert!(common_strict_ancestors(Vec::new(), |_: &BlockHash| None).is_none());
    }

    // -- is_safe_to_delete ----------------------------------------------------

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

    fn is_safe_to_delete_at_floor(
        dag: &KeyValueDagRepresentation,
        block_hash: &BlockHash,
        floor: &Floor,
        conf: &CasperShardConf,
    ) -> Result<bool, KvStoreError> {
        // Derived from the DAG exactly as `collect_garbage` does, so the depth
        // clause is exercised against a real citation set rather than a stub
        // that could refuse for the wrong reason. Bounded at this single
        // candidate's own height — the same rule `collect_garbage` applies
        // to a whole `pending` set collapses to "this one height" here.
        let candidate_height = dag.lookup_unsafe(block_hash)?.block_number;
        let common_strict_ancestors = common_strict_main_chain_ancestors(dag, candidate_height);
        is_safe_to_delete(
            dag,
            block_hash,
            floor,
            conf,
            common_strict_ancestors.as_ref(),
        )
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
            !is_safe_to_delete_at_floor(&dag, &hash(12), &floor_at(10), &conf)
                .expect("safety check"),
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
            is_safe_to_delete_at_floor(&dag, &hash(5), &floor_at(10), &conf).expect("safety check"),
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
            !is_safe_to_delete_at_floor(&dag, &hash(6), &floor_at(10), &conf)
                .expect("safety check"),
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
            !is_safe_to_delete_at_floor(&dag, &hash(5), &floor_at(10), &conf)
                .expect("safety check"),
            "an unfinalized block's data may still be needed by a branch that wins",
        );
    }
}
