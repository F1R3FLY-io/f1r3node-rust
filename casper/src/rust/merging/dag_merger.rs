// See casper/src/main/scala/coop/rchain/casper/merging/DagMerger.scala

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use models::rust::block_hash::BlockHash;
use prost::bytes::Bytes;
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rholang::rust::interpreter::rho_runtime::RhoHistoryRepository;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::merging_logic::{self, NumberChannelsDiff};
use rspace_plus_plus::rspace::merger::state_change_merger;
use shared::rust::hashable_set::HashableSet;

use super::conflict_set_merger;
use super::deploy_chain_index::DeployChainIndex;
use crate::rust::errors::CasperError;
use crate::rust::system_deploy::{is_slash_deploy_id, is_system_deploy_id};

pub fn cost_optimal_rejection_alg() -> impl Fn(&DeployChainIndex) -> u64 {
    |deploy_chain_index: &DeployChainIndex| {
        let cost: u64 = deploy_chain_index
            .deploys_with_cost
            .0
            .iter()
            .map(|deploy| deploy.cost)
            .sum();
        if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
            tracing::debug!(target: "f1r3fly.merge.step", step = "cost_optimal_rejection_alg.RESULT",
                src_block = %hex::encode(&deploy_chain_index.source_block_hash[..]),
                src_block_number = deploy_chain_index.source_block_number,
                n_deploys = deploy_chain_index.deploys_with_cost.0.len(),
                cost = cost);
        }
        cost
    }
}

/// True iff every datum in `sub` (with multiplicity) is present in `sup` (with
/// multiplicity). The multiplicity-aware containment check the single-value-cell
/// keep-one needs: a chain may consume a datum only as many times as it remains
/// available in the running cell state.
pub(crate) fn is_sub_multiset(sub: &[Vec<u8>], sup: &[Vec<u8>]) -> bool {
    let mut sup_counts: HashMap<&Vec<u8>, usize> = HashMap::new();
    for d in sup {
        *sup_counts.entry(d).or_insert(0) += 1;
    }
    let mut sub_counts: HashMap<&Vec<u8>, usize> = HashMap::new();
    for d in sub {
        *sub_counts.entry(d).or_insert(0) += 1;
    }
    sub_counts
        .iter()
        .all(|(d, &need)| sup_counts.get(*d).copied().unwrap_or(0) >= need)
}

/// Deterministic order for the single-value-cell serialization keep-one. Block
/// number is PRIMARY so a producer precedes its consumers (a consumer of another
/// chain's produce is in a strictly higher block), keeping the available-multiset
/// seeding valid without a topological sort. Among same-height siblings — where a
/// single-value-cell fork actually is — higher TOTAL COST wins (pay-more-wins),
/// with (block hash, sorted sigs) as the final node-identical tiebreak.
pub(crate) fn serialize_keep_one_order(
    a: &DeployChainIndex,
    b: &DeployChainIndex,
) -> std::cmp::Ordering {
    fn cost(c: &DeployChainIndex) -> u64 { c.deploys_with_cost.0.iter().map(|d| d.cost).sum() }
    fn sig_key(c: &DeployChainIndex) -> Vec<Vec<u8>> {
        let mut sigs: Vec<Vec<u8>> = c
            .deploys_with_cost
            .0
            .iter()
            .map(|d| d.deploy_id.to_vec())
            .collect();
        sigs.sort();
        sigs
    }
    a.source_block_number
        .cmp(&b.source_block_number)
        .then_with(|| cost(b).cmp(&cost(a))) // higher cost first (pay-more-wins)
        .then_with(|| a.source_block_hash.cmp(&b.source_block_hash))
        .then_with(|| sig_key(a).cmp(&sig_key(b)))
}

pub fn merge(
    dag: &KeyValueDagRepresentation,
    lfb: &BlockHash,
    lfb_post_state: &Blake2b256Hash,
    index: impl Fn(&BlockHash) -> Result<Vec<DeployChainIndex>, CasperError>,
    history_repository: &RhoHistoryRepository,
    rejection_cost_f: impl Fn(&DeployChainIndex) -> u64,
    scope: Option<HashSet<BlockHash>>,
    disable_late_block_filtering: bool,
) -> Result<
    (
        Blake2b256Hash,
        Vec<(Bytes, BlockHash)>,
        Vec<(Bytes, BlockHash)>,
    ),
    CasperError,
> {
    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        tracing::debug!(target: "f1r3fly.merge.step", step = "merge.ENTER",
            lfb = %hex::encode(&lfb[..]),
            lfb_post_state = %hex::encode(lfb_post_state.clone().bytes()),
            scope = %scope
                .as_ref()
                .map_or("ALL".to_string(), |s| format!("{} blocks", s.len())),
            disable_late_block_filtering = disable_late_block_filtering);
    }

    // Blocks to merge are all blocks in scope that are NOT the LFB or its ancestors.
    // This includes:
    // 1. Descendants of LFB (blocks built on top of LFB)
    // 2. Siblings of LFB (blocks at same height but different branch) that are ancestors of the tips
    // Previously we only included descendants, which missed deploy effects from sibling branches.
    let actual_blocks: HashSet<BlockHash> = match &scope {
        Some(scope_blocks) => {
            // Avoid unbounded full-DAG ancestor scans. Check each scope block against LFB directly.
            let mut result = HashSet::new();
            for candidate in scope_blocks {
                if !dag.is_in_main_chain(candidate, lfb)? {
                    result.insert(candidate.clone());
                }
            }
            if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                let included: Vec<String> = result.iter().map(|b| hex::encode(&b[..])).collect();
                // Scope blocks excluded from the merge set because they ARE in the
                // LFB main chain (i.e. the LFB or its ancestors).
                let excluded_in_main: Vec<String> = scope_blocks
                    .iter()
                    .filter(|b| !result.contains(*b))
                    .map(|b| hex::encode(&b[..]))
                    .collect();
                tracing::debug!(target: "f1r3fly.merge.step", step = "merge.actual_blocks.SCOPED",
                    n_scope = scope_blocks.len(),
                    n_included = result.len(),
                    n_excluded_in_main_chain = scope_blocks.len() - result.len(),
                    included = ?included,
                    excluded_in_main_chain = ?excluded_in_main);
            }
            result
        }
        None => {
            // Legacy behavior: use descendants of LFB
            let descendants = dag.descendants(lfb)?;
            if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                tracing::debug!(target: "f1r3fly.merge.step", step = "merge.actual_blocks.LEGACY_DESCENDANTS",
                    n_descendants = descendants.len());
            }
            descendants
        }
    };

    // Late blocks: With the new actualBlocks definition that includes sibling branches,
    // there are no "late" blocks when scope is provided - all non-ancestor blocks are in actualBlocks.
    // Late block filtering is now only relevant for legacy code paths without scope.
    let late_blocks: HashSet<BlockHash> = if disable_late_block_filtering || scope.is_some() {
        // No late blocks when scope is provided (all relevant blocks are in actualBlocks)
        HashSet::new()
    } else {
        // Legacy: query nonFinalizedBlocks (non-deterministic, but no scope means
        // this is not a multi-parent merge validation)
        let non_finalized_blocks = dag.non_finalized_blocks()?;
        non_finalized_blocks
            .difference(&actual_blocks)
            .cloned()
            .collect()
    };

    // Log the block sets for debugging
    tracing::info!(
        "DagMerger.merge: LFB={}, scope={}, actualBlocks (above LFB)={}, lateBlocks={}",
        hex::encode(&lfb[..std::cmp::min(8, lfb.len())]),
        scope
            .as_ref()
            .map_or("ALL".to_string(), |s| format!("{} blocks", s.len())),
        actual_blocks.len(),
        late_blocks.len()
    );

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        let actual: Vec<String> = actual_blocks.iter().map(|b| hex::encode(&b[..])).collect();
        let late: Vec<String> = late_blocks.iter().map(|b| hex::encode(&b[..])).collect();
        tracing::debug!(target: "f1r3fly.merge.step", step = "merge.block_sets",
            n_actual = actual_blocks.len(), n_late = late_blocks.len(),
            actual_blocks = ?actual, late_blocks = ?late);
    }

    // Get indices for actual and late blocks, converting to sorted vectors for determinism
    let mut actual_set_vec = Vec::new();
    let mut late_set_vec = Vec::new();

    // Process actual blocks (sorted for determinism)
    let mut actual_blocks_sorted: Vec<_> = actual_blocks.iter().collect();
    actual_blocks_sorted.sort();
    for block_hash in actual_blocks_sorted {
        let indices = index(block_hash)?;
        actual_set_vec.extend(indices);
    }

    // Process late blocks (sorted for determinism)
    let mut late_blocks_sorted: Vec<_> = late_blocks.iter().collect();
    late_blocks_sorted.sort();
    for block_hash in late_blocks_sorted {
        let indices = index(block_hash)?;
        late_set_vec.extend(indices);
    }

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        tracing::debug!(target: "f1r3fly.merge.step", step = "merge.indices_loaded",
            n_actual_chains = actual_set_vec.len(), n_late_chains = late_set_vec.len());
    }

    // Accumulator for deploys that lose their chain via dedup but have no
    // fresher copy elsewhere. These are treated the same as conflict-rejected
    // deploys downstream — added to the rejected-deploy buffer so the
    // recovery path can re-propose them in a subsequent block.
    let mut collateral_lost_pairs: Vec<(Bytes, BlockHash)> = Vec::new();

    // Deploy de-duplication. When the same deploy ID appears in chains from
    // multiple blocks in scope — for example, because a previously-rejected
    // deploy was re-proposed in a later block — keep the copy from the freshest
    // source: higher block number first, then lexicographically-smaller block
    // hash as a deterministic tiebreak. A chain containing any deploy whose
    // freshest source is a different chain is dropped; its diffs were computed
    // against a pre-state that the fresh execution replaces.
    if !actual_set_vec.is_empty() {
        // Find the freshest source for each deploy_id across all chains.
        let mut latest_for_deploy: HashMap<Bytes, (i64, BlockHash)> = HashMap::new();
        for chain in &actual_set_vec {
            for deploy in &chain.deploys_with_cost.0 {
                let candidate = (chain.source_block_number, chain.source_block_hash.clone());
                match latest_for_deploy.get(&deploy.deploy_id) {
                    Some((best_num, best_hash)) => {
                        // Fresher = higher block number, or byte-lex smaller hash at tie.
                        let is_fresher = candidate.0 > *best_num
                            || (candidate.0 == *best_num && candidate.1 < *best_hash);
                        if is_fresher {
                            latest_for_deploy.insert(deploy.deploy_id.clone(), candidate);
                        }
                    }
                    None => {
                        latest_for_deploy.insert(deploy.deploy_id.clone(), candidate);
                    }
                }
            }
        }

        if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
            tracing::debug!(target: "f1r3fly.merge.step", step = "merge.dedup.latest_for_deploy.ENTER",
                n_deploy_ids = latest_for_deploy.len(), n_chains = actual_set_vec.len());
            for (deploy_id, (num, hash)) in latest_for_deploy.iter() {
                tracing::debug!(target: "f1r3fly.merge.step", step = "merge.dedup.latest_for_deploy",
                    deploy_id = %hex::encode(&deploy_id[..8.min(deploy_id.len())]),
                    freshest_block_number = *num,
                    freshest_block_hash = %hex::encode(&hash[..]));
            }
        }

        // Retain chains only if every deploy in the chain points back to THIS chain
        // as the freshest source. A chain with even one stale deploy is discarded —
        // its diffs are against a pre-state that includes the stale deploy's effects,
        // which are being dropped.
        //
        // Dropping a chain with multiple deploys can cost "collateral": deploys in
        // the dropped chain whose IDs have no fresher copy elsewhere are effectively
        // lost. Collect those sigs so the rejected-deploy buffer can re-propose
        // them in a later block, mirroring how conflict-rejected deploys recover.
        let pre_dedup_count = actual_set_vec.len();
        let (retained, dropped): (Vec<_>, Vec<_>) = std::mem::take(&mut actual_set_vec)
            .into_iter()
            .partition(|chain| {
                chain.deploys_with_cost.0.iter().all(|deploy| {
                    match latest_for_deploy.get(&deploy.deploy_id) {
                        Some((best_num, best_hash)) => {
                            chain.source_block_number == *best_num
                                && chain.source_block_hash == *best_hash
                        }
                        None => true,
                    }
                })
            });
        actual_set_vec = retained;
        let post_dedup_count = actual_set_vec.len();

        if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
            tracing::debug!(target: "f1r3fly.merge.step", step = "merge.dedup.partition",
                pre_dedup = pre_dedup_count, retained = post_dedup_count, dropped = dropped.len());
            for chain in &actual_set_vec {
                let sigs: Vec<String> = chain
                    .deploys_with_cost
                    .0
                    .iter()
                    .map(|d| hex::encode(&d.deploy_id[..8.min(d.deploy_id.len())]))
                    .collect();
                tracing::debug!(target: "f1r3fly.merge.step", step = "merge.dedup.retained_chain",
                    src_block = %hex::encode(&chain.source_block_hash[..]),
                    src_block_number = chain.source_block_number,
                    n_deploys = chain.deploys_with_cost.0.len(),
                    sigs = ?sigs);
            }
        }

        for chain in &dropped {
            for deploy in chain.deploys_with_cost.0.iter() {
                if is_system_deploy_id(&deploy.deploy_id) {
                    continue;
                }
                let best = latest_for_deploy.get(&deploy.deploy_id);
                let is_collateral = match best {
                    Some((best_num, best_hash)) => {
                        chain.source_block_number == *best_num
                            && chain.source_block_hash == *best_hash
                    }
                    None => true,
                };
                if is_collateral {
                    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                        tracing::debug!(target: "f1r3fly.merge.step", step = "merge.dedup.collateral_lost",
                            deploy_id = %hex::encode(&deploy.deploy_id[..8.min(deploy.deploy_id.len())]),
                            src_block = %hex::encode(&chain.source_block_hash[..]));
                    }
                    collateral_lost_pairs
                        .push((deploy.deploy_id.clone(), chain.source_block_hash.clone()));
                }
            }
        }

        if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
            for chain in &dropped {
                let sigs: Vec<String> = chain
                    .deploys_with_cost
                    .0
                    .iter()
                    .map(|d| hex::encode(&d.deploy_id[..8.min(d.deploy_id.len())]))
                    .collect();
                let channels: Vec<String> = chain
                    .state_changes
                    .datums_changes
                    .iter()
                    .map(|e| {
                        let chg = e.value();
                        format!(
                            "{}:r{}a{}",
                            hex::encode(e.key().clone().bytes()),
                            chg.removed.len(),
                            chg.added.len()
                        )
                    })
                    .collect();
                tracing::debug!(target: "f1r3fly.merge.step", step = "merge.dedup_dropped_chain",
                    src_block = %hex::encode(&chain.source_block_hash[..8.min(chain.source_block_hash.len())]),
                    sigs = ?sigs, channels = ?channels);
            }
        }

        if post_dedup_count < pre_dedup_count {
            tracing::info!(
                "DagMerger dedup: dropped {} stale chain(s) ({} -> {}), collateral deploys={}",
                pre_dedup_count - post_dedup_count,
                pre_dedup_count,
                post_dedup_count,
                collateral_lost_pairs.len(),
            );
        }
    }

    // Sort the deploy chain indices for deterministic iteration order
    actual_set_vec.sort();
    late_set_vec.sort();

    // Log state change details for debugging merge issues
    for (i, chain) in actual_set_vec.iter().enumerate() {
        tracing::debug!(
            target: "f1r3fly.merge.dag_merger.state_changes",
            "deploy_chain[{}]: datums={}, conts={}, joins={}, deploys={}, cost={}",
            i,
            chain.state_changes.datums_changes.len(),
            chain.state_changes.cont_changes.len(),
            chain.state_changes.consume_channels_to_join_serialized_map.len(),
            chain.deploys_with_cost.0.len(),
            chain.deploys_with_cost.0.iter().map(|d| d.cost).sum::<u64>(),
        );
    }

    // STEP TRACE: per-chain inputs to conflict resolution — each chain's deploy
    // sigs and its per-channel datum delta (removed/added counts + bytes). Lets
    // the @"m" cell be followed by size across chains. Gated, zero cost when off.
    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        tracing::debug!(target: "f1r3fly.merge.step", step = "merge.resolve_inputs",
            n_actual_chains = actual_set_vec.len(), n_late = late_set_vec.len(),
            collateral_lost = collateral_lost_pairs.len());
        for (i, chain) in actual_set_vec.iter().enumerate() {
            let sigs: Vec<String> = chain
                .deploys_with_cost
                .0
                .iter()
                .map(|d| hex::encode(&d.deploy_id[..8.min(d.deploy_id.len())]))
                .collect();
            for entry in chain.state_changes.datums_changes.iter() {
                let chg = entry.value();
                let rb: usize = chg.removed.iter().map(|d| d.len()).sum();
                let ab: usize = chg.added.iter().map(|d| d.len()).sum();
                tracing::debug!(target: "f1r3fly.merge.step", step = "merge.chain_channel",
                    chain = i, sigs = ?sigs,
                    channel = %hex::encode(entry.key().clone().bytes()),
                    removed = chg.removed.len(), added = chg.added.len(),
                    removed_bytes = rb, added_bytes = ab);
            }
        }
    }

    // Keep as Vec for deterministic processing (ConflictSetMerger expects sorted Vecs)
    let actual_seq = actual_set_vec;
    let late_seq = late_set_vec;

    // Pre-computed data for a single DeployChainIndex, cached by pointer address
    // to avoid recomputing on every O(D²) depends() call.
    struct ChainDerived {
        produces_created: HashableSet<rspace_plus_plus::rspace::trace::event::Produce>,
        consumes_created: HashableSet<rspace_plus_plus::rspace::trace::event::Consume>,
    }

    // Pre-computed data for a branch (HashableSet<DeployChainIndex>), cached by
    // pointer address to avoid recomputing on every O(B²) conflicts() call.
    struct BranchDerived {
        user_deploy_ids: HashSet<Bytes>,
        combined_event_log: rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex,
    }

    fn compute_branch_derived(
        branch: &HashableSet<DeployChainIndex>,
    ) -> Result<BranchDerived, rspace_plus_plus::rspace::errors::HistoryError> {
        let user_deploy_ids: HashSet<_> = branch
            .0
            .iter()
            .flat_map(|chain| chain.deploys_with_cost.0.iter())
            .filter(|deploy| !is_system_deploy_id(&deploy.deploy_id))
            .map(|deploy| deploy.deploy_id.clone())
            .collect();

        let combined_event_log = branch
            .0
            .iter()
            .filter(|idx| {
                idx.deploys_with_cost
                    .0
                    .iter()
                    .all(|d| !is_system_deploy_id(&d.deploy_id))
            })
            .map(|chain| &chain.event_log_index)
            .try_fold(
                rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex::empty(),
                |acc, index| {
                    rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex::combine(
                        &acc, index,
                    )
                },
            )?;

        Ok(BranchDerived {
            user_deploy_ids,
            combined_event_log,
        })
    }

    // Lazy caches keyed by pointer address. Safe because:
    // - References come from HashSet iteration, addresses stable during iteration
    // - DerivedSets/BranchDerived are pure functions of the item
    let chain_cache: RefCell<HashMap<usize, ChainDerived>> = RefCell::new(HashMap::new());
    let branch_cache: RefCell<HashMap<usize, BranchDerived>> = RefCell::new(HashMap::new());

    let get_chain_derived = |chain: &DeployChainIndex| -> usize {
        let addr = std::ptr::addr_of!(*chain) as usize;
        let mut cache = chain_cache.borrow_mut();
        cache.entry(addr).or_insert_with(|| ChainDerived {
            produces_created: merging_logic::produces_created_and_not_destroyed(
                &chain.event_log_index,
            ),
            consumes_created: merging_logic::consumes_created_and_not_destroyed(
                &chain.event_log_index,
            ),
        });
        addr
    };

    let get_branch_derived =
        |branch: &HashableSet<DeployChainIndex>| -> Result<usize, rspace_plus_plus::rspace::errors::HistoryError> {
            let addr = std::ptr::addr_of!(*branch) as usize;
            let mut cache = branch_cache.borrow_mut();
            if !cache.contains_key(&addr) {
                let derived = compute_branch_derived(branch)?;
                cache.insert(addr, derived);
            }
            Ok(addr)
        };

    // Create history reader for base state
    let history_reader = std::sync::Arc::new(
        history_repository
            .get_history_reader(lfb_post_state)
            .map_err(|e| CasperError::HistoryError(e))?,
    );

    // Bind merge-logic closures to named variables so both resolve_conflicts
    // and compute_merged_state can take them by reference, with the rejection
    // expansion step interposed between the two calls.
    let depends_fn = |target: &DeployChainIndex, source: &DeployChainIndex| -> bool {
        // Cached depends: pre-computes source's derived sets on first access
        let source_addr = get_chain_derived(source);
        let cache = chain_cache.borrow();
        let derived = cache.get(&source_addr).unwrap();

        let produces_source: HashSet<_> = derived
            .produces_created
            .0
            .difference(&source.event_log_index.produces_mergeable.0)
            .collect();
        let produces_target: HashSet<_> = target
            .event_log_index
            .produces_consumed
            .0
            .difference(&source.event_log_index.produces_mergeable.0)
            .collect();

        if produces_source
            .intersection(&produces_target)
            .next()
            .is_some()
        {
            if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                tracing::debug!(target: "f1r3fly.merge.step", step = "depends_fn.TRUE_PRODUCE_INTERSECTION",
                    target_src = %hex::encode(&target.source_block_hash[..]),
                    source_src = %hex::encode(&source.source_block_hash[..]),
                    n_produce_overlap = produces_source.intersection(&produces_target).count());
            }
            return true;
        }

        let consume_dep = derived
            .consumes_created
            .0
            .intersection(&target.event_log_index.consumes_produced.0)
            .next()
            .is_some();
        if consume_dep && tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
            tracing::debug!(target: "f1r3fly.merge.step", step = "depends_fn.TRUE_CONSUME_INTERSECTION",
                target_src = %hex::encode(&target.source_block_hash[..]),
                source_src = %hex::encode(&source.source_block_hash[..]),
                n_consume_overlap = derived
                    .consumes_created
                    .0
                    .intersection(&target.event_log_index.consumes_produced.0)
                    .count());
        }
        consume_dep
    };

    let state_changes_fn = |chain: &DeployChainIndex| Ok(chain.state_changes.clone());

    let mergeable_channels_fn =
        |chain: &DeployChainIndex| chain.event_log_index.number_channels_data.clone();

    let compute_trie_actions_fn = {
        let reader = Arc::clone(&history_reader);
        move |changes: rspace_plus_plus::rspace::merger::state_change::StateChange,
              mergeable_chs| {
            // Per-channel datum-delta trace: surfaces a clobber (a base datum
            // removed with nothing added) and a multi-datum land (added > 1) in
            // the merged state change, gated so it costs nothing when disabled.
            if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                tracing::debug!(target: "f1r3fly.merge.step", step = "merge.merged_state.ENTER",
                    n_channels = changes.datums_changes.len());
                for entry in changes.datums_changes.iter() {
                    let chg = entry.value();
                    let removed_bytes: usize = chg.removed.iter().map(|d| d.len()).sum();
                    let added_bytes: usize = chg.added.iter().map(|d| d.len()).sum();
                    tracing::debug!(
                        target: "f1r3fly.merge.step",
                        step = "merge.merged_result_channel",
                        channel = %hex::encode(entry.key().clone().bytes()),
                        removed = chg.removed.len(),
                        added = chg.added.len(),
                        removed_bytes,
                        added_bytes,
                    );
                }
            }
            let trie_actions = state_change_merger::compute_trie_actions(
                &changes,
                &*reader,
                &mergeable_chs,
                |hash: &Blake2b256Hash, channel_changes, number_chs: &NumberChannelsDiff| {
                    if let Some(number_ch_val) = number_chs.get(hash) {
                        let (diff, merge_type) = *number_ch_val;
                        let base_get_data = |h: &Blake2b256Hash| reader.get_data(h);
                        Ok(Some(RholangMergingLogic::calculate_number_channel_merge(
                            hash,
                            diff,
                            merge_type,
                            channel_changes,
                            base_get_data,
                        )?))
                    } else {
                        Ok(None)
                    }
                },
            );
            if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                match &trie_actions {
                    Ok(actions) => {
                        tracing::debug!(target: "f1r3fly.merge.step", step = "merge.merged_state.EXIT",
                        n_trie_actions = actions.len())
                    }
                    Err(e) => {
                        tracing::debug!(target: "f1r3fly.merge.step", step = "merge.merged_state.ERROR",
                        error = ?e)
                    }
                }
            }
            trie_actions
        }
    };

    let apply_trie_actions_fn = |actions| {
        history_repository
            .reset(lfb_post_state)
            .map(|reset_repo| reset_repo.do_checkpoint(actions))
            .map(|checkpoint| checkpoint.root())
            .map_err(|e| e.into())
    };

    let get_data_fn = |hash| history_reader.get_data(&hash).map_err(|e| e.into());

    // Build the conflict map for branches. Combines event-log conflicts
    // (races, potential COMMs, produces touching base joins) with the
    // same-user-deploy-id check: two branches that share any user deploy
    // ID must be flagged as conflicting regardless of their event logs.
    //
    // `EventLogIndex::combine` inside `get_branch_derived` is fallible —
    // a MergeType mismatch propagates as a hard error so the merge is
    // rejected rather than silently absorbing the invariant violation.
    let compute_conflict_map_fn = |branches_set: &HashableSet<HashableSet<DeployChainIndex>>| -> Result<
        HashMap<HashableSet<DeployChainIndex>, HashableSet<HashableSet<DeployChainIndex>>>,
        rspace_plus_plus::rspace::errors::HistoryError,
    > {
        // Populate `branch_cache` for every branch so the borrow below can
        // read combined event logs without recomputing, and any combine
        // failure surfaces here before we read.
        for branch in branches_set.0.iter() {
            get_branch_derived(branch)?;
        }

        // Snapshot branch references in a stable order so the parallel
        // arrays passed into the indexed map and the deploy-id pass below
        // line up.
        let branches_refs: Vec<&HashableSet<DeployChainIndex>> = branches_set.0.iter().collect();
        let branches_owned: Vec<HashableSet<DeployChainIndex>> =
            branches_refs.iter().map(|b| (*b).clone()).collect();

        let cache = branch_cache.borrow();
        let event_logs: Vec<&rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex> =
            branches_refs
                .iter()
                .map(|b| {
                    let addr = std::ptr::addr_of!(**b) as usize;
                    &cache.get(&addr).unwrap().combined_event_log
                })
                .collect();

        // MSTACK merge-trace: per-branch deploy sigs + event-log sizes. Gated
        // behind the `f1r3fly.merge.mstack` target so the sig/format work (and the
        // loop) is skipped entirely unless that target is enabled — zero cost in
        // normal operation.
        if tracing::enabled!(target: "f1r3fly.merge.mstack", tracing::Level::DEBUG) {
            for (idx, e) in event_logs.iter().enumerate() {
                let sigs: Vec<String> = branches_owned[idx]
                    .0
                    .iter()
                    .flat_map(|dci| dci.deploys_with_cost.0.iter())
                    .map(|d| hex::encode(&d.deploy_id[..20.min(d.deploy_id.len())]))
                    .collect();
                tracing::debug!(
                    target: "f1r3fly.merge.mstack",
                    "branch[{}] sigs={:?} |produces_consumed|={} |consumes_produced|={}",
                    idx,
                    sigs,
                    e.produces_consumed.0.len(),
                    e.consumes_produced.0.len()
                );
            }
        }

        // Event-log conflicts: races, potential COMMs, base-join touches.
        // `mutable_key_type` is a false positive here: prost::bytes::Bytes uses an
        // internal Arc, not interior mutability, but clippy can't distinguish.
        #[allow(clippy::mutable_key_type)]
        let mut conflict_map =
            merging_logic::compute_conflict_map_event_indexed(&branches_owned, &event_logs);

        // STEP TRACE: event-log conflict edges by branch index, before the
        // same-user-deploy-id pass augments them below.
        if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
            tracing::debug!(target: "f1r3fly.merge.step", step = "compute_conflict_map_fn.ENTER",
                n_branches = branches_refs.len());
            for (idx, b) in branches_refs.iter().enumerate() {
                let n = conflict_map.get(*b).map(|s| s.0.len()).unwrap_or(0);
                tracing::debug!(target: "f1r3fly.merge.step", step = "compute_conflict_map_fn.EVENT_LOG_EDGE",
                    branch = idx, conflicts_with_n_branches = n);
            }
        }

        // MSTACK merge-trace: the resulting conflict map by branch idx (gated).
        if tracing::enabled!(target: "f1r3fly.merge.mstack", tracing::Level::DEBUG) {
            for (idx, b) in branches_refs.iter().enumerate() {
                let n = conflict_map.get(*b).map(|s| s.0.len()).unwrap_or(0);
                tracing::debug!(
                    target: "f1r3fly.merge.mstack",
                    "conflict_map branch[{}] conflicts_with_n_branches={}",
                    idx, n
                );
            }
        }

        // Same-user-deploy-id pass: for any user deploy ID appearing in
        // multiple branches, mark all such branches as mutual conflicts.
        let mut deploy_to_branches: HashMap<prost::bytes::Bytes, Vec<usize>> = HashMap::new();
        for (idx, b) in branches_refs.iter().enumerate() {
            let addr = std::ptr::addr_of!(**b) as usize;
            let derived = cache.get(&addr).unwrap();
            for d in &derived.user_deploy_ids {
                deploy_to_branches.entry(d.clone()).or_default().push(idx);
            }
        }
        if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
            for (deploy_id, branch_ids) in deploy_to_branches.iter() {
                if branch_ids.len() >= 2 {
                    tracing::debug!(target: "f1r3fly.merge.step", step = "compute_conflict_map_fn.SHARED_DEPLOY_ID",
                        deploy_id = %hex::encode(&deploy_id[..8.min(deploy_id.len())]),
                        branch_indices = ?branch_ids);
                }
            }
        }

        for branch_ids in deploy_to_branches.values() {
            if branch_ids.len() < 2 {
                continue;
            }
            for i in 0..branch_ids.len() {
                for j in (i + 1)..branch_ids.len() {
                    let a = branches_owned[branch_ids[i]].clone();
                    let b = branches_owned[branch_ids[j]].clone();
                    if let Some(set_a) = conflict_map.get_mut(&a) {
                        set_a.0.insert(b.clone());
                    }
                    if let Some(set_b) = conflict_map.get_mut(&b) {
                        set_b.0.insert(a.clone());
                    }
                }
            }
        }

        if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
            for (idx, b) in branches_refs.iter().enumerate() {
                let n = conflict_map.get(*b).map(|s| s.0.len()).unwrap_or(0);
                tracing::debug!(target: "f1r3fly.merge.step", step = "compute_conflict_map_fn.EXIT",
                    branch = idx, conflicts_with_n_branches_final = n);
            }
        }

        Ok(conflict_map)
    };

    // Group chains in merge_set into branches whose elements depend on each
    // other. Builds inverted indexes over each chain's `EventLogIndex` and
    // emits depends pairs in a single pass, then groups via
    // `gather_related_sets`.
    let compute_branches_fn =
        |merge_set: &HashableSet<DeployChainIndex>| -> HashableSet<HashableSet<DeployChainIndex>> {
            let chains_vec: Vec<DeployChainIndex> = merge_set.0.iter().cloned().collect();
            let event_logs: Vec<&rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex> =
                chains_vec.iter().map(|c| &c.event_log_index).collect();
            #[allow(clippy::mutable_key_type)]
            let depends_map =
                merging_logic::compute_depends_map_event_indexed(&chains_vec, &event_logs);
            let branches = merging_logic::gather_related_sets(&depends_map);
            if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                tracing::debug!(target: "f1r3fly.merge.step", step = "compute_branches_fn.ENTER",
                    n_chains = chains_vec.len(), n_branches = branches.0.len());
                for (idx, branch) in branches.0.iter().enumerate() {
                    let sigs: Vec<String> = branch
                        .0
                        .iter()
                        .flat_map(|dci| dci.deploys_with_cost.0.iter())
                        .map(|d| hex::encode(&d.deploy_id[..8.min(d.deploy_id.len())]))
                        .collect();
                    tracing::debug!(target: "f1r3fly.merge.step", step = "compute_branches_fn.BRANCH",
                        branch = idx, n_chains = branch.0.len(), sigs = ?sigs);
                }
            }
            branches
        };

    // Resolve conflicts: detect conflicts and select the cost-optimal rejection set.
    let mut resolved = conflict_set_merger::resolve_conflicts(
        actual_seq,
        late_seq,
        &depends_fn,
        &rejection_cost_f,
        &mergeable_channels_fn,
        &get_data_fn,
        &compute_branches_fn,
        &compute_conflict_map_fn,
    )
    .map_err(|e| CasperError::HistoryError(e))?;

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        let log_chain = |tag: &str, c: &DeployChainIndex| {
            let sigs: Vec<String> = c
                .deploys_with_cost
                .0
                .iter()
                .map(|d| hex::encode(&d.deploy_id[..8.min(d.deploy_id.len())]))
                .collect();
            let chans: Vec<String> = c
                .state_changes
                .datums_changes
                .iter()
                .map(|e| {
                    let chg = e.value();
                    format!(
                        "{}:r{}a{}",
                        hex::encode(e.key().clone().bytes()),
                        chg.removed.len(),
                        chg.added.len()
                    )
                })
                .collect();
            tracing::debug!(target: "f1r3fly.merge.step", step = "resolve_conflicts.RESULT",
                verdict = tag,
                src = %hex::encode(&c.source_block_hash[..8.min(c.source_block_hash.len())]),
                sigs = ?sigs, chans = ?chans);
        };
        for branch in &resolved.to_merge {
            for c in &branch.0 {
                log_chain("KEEP", c);
            }
        }
        for c in &resolved.rejected.0 {
            log_chain("REJECT", c);
        }
    }

    // Single-value-cell serialization (keep-one). After conflict resolution the
    // surviving chains may still contain CONCURRENT writes to one non-foldable
    // single-value cell — a fork where two chains consumed the same base datum.
    // Conflict detection misses this when both writers share a dependency on the
    // producer (they land in one branch, and the combined event log hides the
    // double consume as internal COMMs). A single-value cell cannot hold both
    // writes; keep one linear write path and reject the rest to recovery, where
    // they re-execute against the updated base. A stale rebased consume is the
    // degenerate fork whose consumed datum was never available. Foldable
    // (mergeable number) channels compose via the dispatcher fold and are exempt.
    //
    // This replaces the prior block-descendant "rejection expansion", which
    // dropped channel-disjoint descendants (keep-one's own survivor) on block
    // descent rather than data dependency.
    let __keep_one_start = std::time::Instant::now();
    {
        let mut foldable: HashSet<Blake2b256Hash> = HashSet::new();
        for branch in resolved.to_merge.iter() {
            for chain in branch.0.iter() {
                for (ch, _) in chain.event_log_index.number_channels_data.iter() {
                    foldable.insert(ch.clone());
                }
            }
        }

        #[allow(clippy::mutable_key_type)]
        let rejected: HashSet<DeployChainIndex> = {
            let mut ordered: Vec<&DeployChainIndex> = resolved
                .to_merge
                .iter()
                .flat_map(|branch| branch.0.iter())
                .collect();
            ordered.sort_by(|a, b| serialize_keep_one_order(a, b));

            // Running available-datum multiset per non-foldable channel, seeded
            // lazily from the merge (floor) base.
            let mut available: HashMap<Blake2b256Hash, Vec<Vec<u8>>> = HashMap::new();
            #[allow(clippy::mutable_key_type)]
            let mut rejected: HashSet<DeployChainIndex> = HashSet::new();

            for chain in ordered.iter() {
                let chain: &DeployChainIndex = chain;
                // Serializable iff every non-foldable channel it consumes from
                // still has those datums available in the running cell state.
                let mut serializable = true;
                for e in chain.state_changes.datums_changes.iter() {
                    let ch = e.key();
                    if foldable.contains(ch) {
                        continue;
                    }
                    let removed = &e.value().removed;
                    if removed.is_empty() {
                        continue;
                    }
                    if !available.contains_key(ch) {
                        let base = history_reader
                            .get_data_proj_binary(ch)
                            .map_err(CasperError::HistoryError)?;
                        available.insert(ch.clone(), base);
                    }
                    if !is_sub_multiset(removed, available.get(ch).expect("seeded above")) {
                        serializable = false;
                        break;
                    }
                }
                if !serializable {
                    rejected.insert(DeployChainIndex::clone(chain));
                    continue;
                }
                // Apply this chain's writes so later chains see them:
                // available = (available -- removed) ++ added, per touched channel.
                for e in chain.state_changes.datums_changes.iter() {
                    let ch = e.key();
                    if foldable.contains(ch) {
                        continue;
                    }
                    if !available.contains_key(ch) {
                        let base = history_reader
                            .get_data_proj_binary(ch)
                            .map_err(CasperError::HistoryError)?;
                        available.insert(ch.clone(), base);
                    }
                    let avail = available.get_mut(ch).expect("seeded above");
                    let mut next =
                        rspace_plus_plus::rspace::merger::state_change::StateChange::multiset_diff(
                            avail,
                            &e.value().removed,
                        );
                    next.extend(e.value().added.iter().cloned());
                    *avail = next;
                }
            }
            rejected
        };

        if !rejected.is_empty() {
            let pre = resolved.to_merge.len();
            let mut kept_branches: Vec<HashableSet<DeployChainIndex>> = Vec::new();
            for branch in std::mem::take(&mut resolved.to_merge) {
                #[allow(clippy::mutable_key_type)]
                let mut kept: HashSet<DeployChainIndex> = HashSet::new();
                for chain in branch.0 {
                    if rejected.contains(&chain) {
                        resolved.rejected.0.insert(chain);
                    } else {
                        kept.insert(chain);
                    }
                }
                if !kept.is_empty() {
                    kept_branches.push(HashableSet(kept));
                }
            }
            resolved.to_merge = kept_branches;
            tracing::debug!(target: "f1r3fly.merge.step", step = "merge.serialize_keep_one",
                rejected_chains = rejected.len(),
                branches_before = pre,
                branches_after = resolved.to_merge.len());
        }
    }
    metrics::histogram!(
        crate::rust::metrics_constants::DAG_MERGE_REJECTION_EXPANSION_TIME_METRIC,
        "source" => crate::rust::metrics_constants::MERGING_METRICS_SOURCE
    )
    .record(__keep_one_start.elapsed().as_secs_f64());

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        let log_final = |verdict: &str, c: &DeployChainIndex| {
            let sigs: Vec<String> = c
                .deploys_with_cost
                .0
                .iter()
                .map(|d| hex::encode(&d.deploy_id[..8.min(d.deploy_id.len())]))
                .collect();
            let chans: Vec<String> = c
                .state_changes
                .datums_changes
                .iter()
                .map(|e| {
                    let chg = e.value();
                    format!(
                        "{}:r{}a{}",
                        hex::encode(e.key().clone().bytes()),
                        chg.removed.len(),
                        chg.added.len()
                    )
                })
                .collect();
            tracing::debug!(target: "f1r3fly.merge.step", step = "merge.final_verdict",
                verdict = verdict,
                src = %hex::encode(&c.source_block_hash[..8.min(c.source_block_hash.len())]),
                sigs = ?sigs, chans = ?chans);
        };
        for branch in &resolved.to_merge {
            for c in &branch.0 {
                log_final("KEEP", c);
            }
        }
        for c in &resolved.rejected.0 {
            log_final("REJECT", c);
        }
    }

    // Combine surviving diffs and apply to the LFB post-state.
    let new_state = conflict_set_merger::compute_merged_state(
        &resolved,
        &state_changes_fn,
        &mergeable_channels_fn,
        &compute_trie_actions_fn,
        &apply_trie_actions_fn,
    )
    .map_err(|e| CasperError::HistoryError(e))?;

    let rejected = resolved.rejected;

    // Extract (rejected deploy ID, source block hash) pairs, split by kind.
    // User deploys feed the rejected-deploy buffer for re-proposal. Slash
    // deploys feed the block creator's dedup step so that the slash effect
    // persists in the merge block's body regardless of cost-optimal rejection
    // of the source chain. Non-slash system deploys (close block, heartbeat)
    // are intentionally dropped here — they are atomic with their containing
    // block and have no recovery semantics.
    let all_pairs: Vec<(Bytes, BlockHash)> = rejected
        .0
        .iter()
        .flat_map(|chain| {
            let src = chain.source_block_hash.clone();
            chain
                .deploys_with_cost
                .0
                .iter()
                .map(move |deploy| (deploy.deploy_id.clone(), src.clone()))
        })
        .collect();

    let mut rejected_user_deploys: Vec<(Bytes, BlockHash)> = all_pairs
        .iter()
        .filter(|(id, _)| !is_system_deploy_id(id))
        .cloned()
        .collect();
    let mut rejected_slashes: Vec<(Bytes, BlockHash)> = all_pairs
        .into_iter()
        .filter(|(id, _)| is_slash_deploy_id(id))
        .collect();

    // Fold dedup collateral into the rejected-user list so the buffer can
    // recover deploys whose chain was dropped for reasons other than
    // cost-optimal rejection. Keep the list unique per deploy_id — a deploy
    // already present from conflict rejection takes precedence.
    if !collateral_lost_pairs.is_empty() {
        let existing_ids: HashSet<Bytes> = rejected_user_deploys
            .iter()
            .map(|(id, _)| id.clone())
            .collect();
        for pair in collateral_lost_pairs {
            if !existing_ids.contains(&pair.0) {
                rejected_user_deploys.push(pair);
            }
        }
    }

    // Deterministic ordering across validators.
    rejected_user_deploys.sort();
    rejected_slashes.sort();

    tracing::debug!(
        "DagMerger.merge: LFB={}, scope={}, actual={}, late={}, rejected_user={}, rejected_slash={}",
        hex::encode(&lfb[..std::cmp::min(8, lfb.len())]),
        scope
            .as_ref()
            .map_or("ALL".to_string(), |s| s.len().to_string()),
        actual_blocks.len(),
        late_blocks.len(),
        rejected_user_deploys.len(),
        rejected_slashes.len(),
    );

    if !rejected_user_deploys.is_empty() {
        let rejected_str: Vec<_> = rejected_user_deploys
            .iter()
            .map(|(sig, _)| hex::encode(&sig[..std::cmp::min(8, sig.len())]))
            .collect();
        tracing::info!(
            "DagMerger rejected {} user deploys: {}",
            rejected_user_deploys.len(),
            rejected_str.join(", ")
        );
    }
    if !rejected_slashes.is_empty() {
        let rejected_str: Vec<_> = rejected_slashes
            .iter()
            .map(|(sig, _)| hex::encode(&sig[..std::cmp::min(8, sig.len())]))
            .collect();
        tracing::info!(
            "DagMerger rejected {} slashes: {}",
            rejected_slashes.len(),
            rejected_str.join(", ")
        );
    }

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        tracing::debug!(target: "f1r3fly.merge.step", step = "merge.EXIT",
            new_state = %hex::encode(new_state.clone().bytes()),
            n_rejected_user = rejected_user_deploys.len(),
            n_rejected_slash = rejected_slashes.len());
    }

    Ok((new_state, rejected_user_deploys, rejected_slashes))
}
