// See casper/src/main/scala/coop/rchain/casper/merging/DagMerger.scala

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use hex::ToHex;
use models::rust::block_hash::BlockHash;
use prost::bytes::Bytes;
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rholang::rust::interpreter::rho_runtime::RhoHistoryRepository;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::merging_logic::{self, NumberChannelsDiff};
use rspace_plus_plus::rspace::merger::state_change::StateChange;
use rspace_plus_plus::rspace::merger::state_change_merger;
use shared::rust::hashable_set::HashableSet;
use tracing::info;

use super::conflict_set_merger;
use super::deploy_chain_index::DeployChainIndex;
use crate::rust::errors::CasperError;
use crate::rust::system_deploy::{is_slash_deploy_id, is_system_deploy_id};

pub fn cost_optimal_rejection_alg() -> impl Fn(&DeployChainIndex) -> u64 {
    |deploy_chain_index: &DeployChainIndex| {
        deploy_chain_index
            .deploys_with_cost
            .0
            .iter()
            .map(|deploy| deploy.cost)
            .sum()
    }
}

/// Check #5 — Stale-removed chain detection (pre-merge filter).
///
/// Identifies chains whose `state_changes.datums_changes[channel].removed`
/// contains bytes that are NOT in the merge's init for that channel AND NOT
/// in any other chain's `added` for that channel. Such chains are "stale" —
/// they were indexed against a prior trie state that's no longer reachable
/// from the merge's LFB via the merge's own chain set. Applying their diffs
/// produces multi-Datum on the channel.
///
/// Pure function over (chain_id, &StateChange) pairs and per-channel init
/// bytes. Returns the set of chain indices to reject.
///
/// Algorithm:
/// 1. Build per-channel `global_added`: union of every chain's `added`.
/// 2. For each chain, for each channel in its `datums_changes`:
///    - For each byte `b` in chain's `removed[channel]`:
///      - `b` is satisfiable iff `init[channel]` contains `b` OR
///        `(global_added[channel] minus this_chain.added[channel])`
///        contains `b`.
///    - If ANY byte is unsatisfiable → chain is stale.
///
/// Rejected user-deploy chains go to the rejected-deploy buffer (handled
/// by `dag_merger::merge`'s existing collateral_lost_pairs flow). Rejected
/// system-deploy chains are dropped (no recovery — atomic with block).
pub fn detect_stale_chains_pure<C: Eq + std::hash::Hash + Clone>(
    chains: &[(C, &StateChange)],
    init_of: impl Fn(&Blake2b256Hash) -> Vec<Vec<u8>>,
) -> HashSet<C> {
    use std::collections::HashMap;

    // Pass 1: build global_added per channel.
    let mut global_added: HashMap<Blake2b256Hash, Vec<Vec<u8>>> = HashMap::new();
    for (_, sc) in chains {
        for entry in sc.datums_changes.iter() {
            let ch = entry.key().clone();
            let added = &entry.value().added;
            global_added.entry(ch).or_default().extend(added.iter().cloned());
        }
    }

    // Pass 2: per-chain staleness check.
    let mut stale: HashSet<C> = HashSet::new();
    for (chain_id, sc) in chains {
        let mut is_stale = false;
        for entry in sc.datums_changes.iter() {
            let channel = entry.key();
            let ch_change = entry.value();
            if ch_change.removed.is_empty() {
                continue;
            }
            let init = init_of(channel);
            // Compute satisfiable set: init ∪ (global_added[channel] − this_chain.added[channel])
            // The minus is multiset: each byte in this_chain.added consumes one
            // matching occurrence from global_added before the check.
            let mut satisfiable: Vec<&Vec<u8>> = init.iter().collect();
            if let Some(g_added) = global_added.get(channel) {
                let mut this_added_counts: HashMap<&Vec<u8>, usize> = HashMap::new();
                for a in &ch_change.added {
                    *this_added_counts.entry(a).or_insert(0) += 1;
                }
                for g in g_added {
                    if let Some(count) = this_added_counts.get_mut(g) {
                        if *count > 0 {
                            *count -= 1;
                            continue;
                        }
                    }
                    satisfiable.push(g);
                }
            }
            // Check each removed byte against satisfiable (multiset semantics).
            let mut satisfiable_counts: HashMap<&Vec<u8>, usize> = HashMap::new();
            for s in &satisfiable {
                *satisfiable_counts.entry(*s).or_insert(0) += 1;
            }
            for r in &ch_change.removed {
                match satisfiable_counts.get_mut(r) {
                    Some(c) if *c > 0 => {
                        *c -= 1;
                    }
                    _ => {
                        let byte_prefix = hex::encode(
                            &r[..std::cmp::min(r.len(), 8)],
                        );
                        tracing::info!(
                            target: "f1r3.trace.stale_chain",
                            "[TRACE-CHECK-5-STALE] channel={} stale_byte_prefix={} chain_removed_count={} init_count={} global_added_count={}",
                            hex::encode(channel.bytes()),
                            byte_prefix,
                            ch_change.removed.len(),
                            init.len(),
                            global_added.get(channel).map(|v| v.len()).unwrap_or(0)
                        );
                        is_stale = true;
                    }
                }
            }
        }
        if is_stale {
            stale.insert(chain_id.clone());
        }
    }
    stale
}

/// Compute `collateral_lost_pairs` for Check #5 stale-chain rejection.
///
/// When Check #5 drops a chain as stale, its user deploys would otherwise be
/// lost — the rejected-deploy buffer captures them via this function so a
/// later proposer can recover them via the exemption path.
///
/// CRITICAL invariant: a deploy present in any KEPT (non-stale) chain MUST
/// NOT appear in the result. Its effects are in canonical state via the
/// kept chain, so marking it "collateral lost" would trigger the recovery
/// exemption to re-execute it on top of state that already has its effects,
/// producing duplicate produces on tagged channels (the multi-Datum bug
/// blocking `test_bonding_validators` Liveness Phase).
///
/// System deploys are excluded — they are atomic with their containing
/// block and have no recovery semantics.
fn stale_chain_collateral_pure<F, G>(
    stale_chain_indices: &HashSet<usize>,
    chain_count: usize,
    deploys_of: F,
    source_of: G,
) -> Vec<(Bytes, BlockHash)>
where
    F: Fn(usize) -> Vec<Bytes>,
    G: Fn(usize) -> BlockHash,
{
    // Deploys present in any kept (non-stale) chain are still in canonical
    // state via that chain. Marking them "collateral lost" would trigger the
    // recovery exemption to re-execute them — see module docstring.
    let kept_chain_deploys: HashSet<Bytes> = (0..chain_count)
        .filter(|idx| !stale_chain_indices.contains(idx))
        .flat_map(|idx| deploys_of(idx))
        .collect();

    let mut collateral = Vec::new();
    for idx in 0..chain_count {
        if !stale_chain_indices.contains(&idx) {
            continue;
        }
        for deploy_id in deploys_of(idx) {
            if is_system_deploy_id(&deploy_id) {
                continue;
            }
            if kept_chain_deploys.contains(&deploy_id) {
                continue;
            }
            collateral.push((deploy_id, source_of(idx)));
        }
    }
    collateral
}

/// BFS walk of DAG descendants of `start_blocks`, restricted to `scope`.
///
/// When the merge rejects the deploy chains of a block, any descendant block
/// in scope has diffs that were computed against the rejected block's
/// post-state and are therefore stale. This walk identifies the affected
/// descendants so their chains can be rejected as well.
///
/// Returns the strict descendants; the start blocks themselves are not included.
fn descendants_within_scope(
    dag: &KeyValueDagRepresentation,
    start_blocks: &HashSet<BlockHash>,
    scope: &HashSet<BlockHash>,
) -> HashSet<BlockHash> {
    let mut result = HashSet::new();
    let mut queue: Vec<BlockHash> = start_blocks.iter().cloned().collect();
    let mut visited: HashSet<BlockHash> = start_blocks.clone();

    while let Some(current) = queue.pop() {
        if let Some(children) = dag.children(&current) {
            for child in children {
                if scope.contains(&child) && visited.insert(child.clone()) {
                    result.insert(child.clone());
                    queue.push(child);
                }
            }
        }
    }

    result
}

/// Pre-computed data for a branch — aggregated user-deploy IDs and event
/// logs split by deploy provenance.
///
/// `combined_user_event_log` aggregates the `user_event_log_index` of every
/// chain in the branch; it feeds the existing CSP-level conflict checks
/// (`compute_conflict_map_event_indexed`).
///
/// `combined_system_event_log` aggregates the `system_event_log_index` of
/// every chain; it feeds the system-deploy state-mutation checks.
///
/// Per-deploy event provenance replaces the old chain-level
/// system-deploy filter: instead of stripping any chain that carries a
/// system-deploy id (which masked user-deploy effects on tagged channels
/// when the runtime grouped them with closeBlock), each chain
/// contributes its user and system events to the respective combined
/// logs.
pub struct BranchDerived {
    pub user_deploy_ids: HashSet<Bytes>,
    pub combined_user_event_log: rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex,
    pub combined_system_event_log: rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex,
    pub combined_all_event_log: rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex,
    /// Identity-tagged channels where at least one chain in this branch has
    /// a pending produce (in `produces_linear ∪ produces_persistent` for
    /// that chain) on a channel whose `identity_tagged_channels` membership
    /// is asserted by the chain's own `EventLogIndex` AND that pending
    /// produce is NOT in `produces_mergeable` (i.e., the channel has no
    /// commutative-merge representation for this chain's contribution —
    /// the Layer-2 contract-seam case per `docs/casper/STATE_MERGING.md`).
    ///
    /// Used by Check #4 (after Step 7 rewrite) to detect cross-branch races
    /// on tagged single-value channels: intersecting two branches'
    /// `identity_tagged_pending_produces` sets identifies channels where
    /// both branches will write non-commutative pending produces. If the
    /// emitted produce hashes differ, the merge would violate the Layer-2
    /// single-value contract and the conflict must be resolved via
    /// cost-optimal rejection.
    pub identity_tagged_pending_produces: HashSet<Blake2b256Hash>,
}

pub fn compute_branch_derived(
    branch: &HashableSet<DeployChainIndex>,
) -> Result<BranchDerived, rspace_plus_plus::rspace::errors::HistoryError> {
    let user_deploy_ids: HashSet<_> = branch
        .0
        .iter()
        .flat_map(|chain| chain.deploys_with_cost.0.iter())
        .filter(|deploy| !is_system_deploy_id(&deploy.deploy_id))
        .map(|deploy| deploy.deploy_id.clone())
        .collect();

    // Identity-tagged pending produces (used by Check #4).
    // Per-chain: a channel qualifies on chain C iff:
    //   1. The channel hash is in C's event_log_index.identity_tagged_channels
    //      (the runtime-detected tag membership from is_mergeable_channel),
    //   2. C has at least one pending produce on the channel (in
    //      produces_linear ∪ produces_persistent), AND
    //   3. At least one of those pending produces is NOT in produces_mergeable
    //      (i.e., the channel is tagged but lacks a commutative-merge
    //      representation in this deploy — contract-seam case).
    // Union over chains gives the branch's set.
    let identity_tagged_pending_produces: HashSet<Blake2b256Hash> = branch
        .0
        .iter()
        .flat_map(|chain| {
            let eli = &chain.event_log_index;
            eli.identity_tagged_channels
                .0
                .iter()
                .filter(|ch| {
                    let pending_produces_on_ch: Vec<_> = eli
                        .produces_linear
                        .0
                        .iter()
                        .chain(eli.produces_persistent.0.iter())
                        .filter(|p| &p.channel_hash == *ch)
                        .collect();
                    if pending_produces_on_ch.is_empty() {
                        return false;
                    }
                    // At least one pending produce on this channel is not
                    // in the commutative-mergeable set — Layer-2 contract
                    // applies but no commutative representation available.
                    pending_produces_on_ch
                        .iter()
                        .any(|p| !eli.produces_mergeable.0.contains(*p))
                })
                .cloned()
                .collect::<HashSet<Blake2b256Hash>>()
        })
        .collect();

    let combined_user_event_log = branch
        .0
        .iter()
        .map(|chain| &chain.user_event_log_index)
        .try_fold(
            rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex::empty(),
            |acc, index| {
                rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex::combine(
                    &acc, index,
                )
            },
        )?;

    let combined_system_event_log = branch
        .0
        .iter()
        .map(|chain| &chain.system_event_log_index)
        .try_fold(
            rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex::empty(),
            |acc, index| {
                rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex::combine(
                    &acc, index,
                )
            },
        )?;

    let combined_all_event_log =
        rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex::combine(
            &combined_user_event_log,
            &combined_system_event_log,
        )?;

    Ok(BranchDerived {
        user_deploy_ids,
        combined_user_event_log,
        combined_system_event_log,
        combined_all_event_log,
        identity_tagged_pending_produces,
    })
}

/// Group `merge_set` chains into branches whose elements depend on each other.
/// Builds inverted indexes over each chain's `EventLogIndex` and emits depends
/// pairs in a single pass, then groups via `gather_related_sets`.
pub fn compute_branches(
    merge_set: &HashableSet<DeployChainIndex>,
) -> HashableSet<HashableSet<DeployChainIndex>> {
    let chains_vec: Vec<DeployChainIndex> = merge_set.0.iter().cloned().collect();
    let event_logs: Vec<&rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex> =
        chains_vec.iter().map(|c| &c.event_log_index).collect();
    #[allow(clippy::mutable_key_type)]
    let depends_map = merging_logic::compute_depends_map_event_indexed(&chains_vec, &event_logs);
    merging_logic::gather_related_sets(&depends_map)
}

pub fn compute_conflict_map(
    branches_set: &HashableSet<HashableSet<DeployChainIndex>>,
) -> Result<
    HashMap<HashableSet<DeployChainIndex>, HashableSet<HashableSet<DeployChainIndex>>>,
    rspace_plus_plus::rspace::errors::HistoryError,
> {
    // Snapshot branch references in a stable order so the parallel arrays
    // passed into the indexed map and the deploy-id pass below line up.
    let branches_refs: Vec<&HashableSet<DeployChainIndex>> = branches_set.0.iter().collect();
    let branches_owned: Vec<HashableSet<DeployChainIndex>> =
        branches_refs.iter().map(|b| (*b).clone()).collect();

    // Compute branch-derived data fresh for each branch. The original closure
    // form cached this in a RefCell, but `resolve_conflicts` calls this
    // exactly once per merge, so the cache was effectively unused — dropping
    // it has zero perf impact on production.
    let derived: Vec<BranchDerived> = branches_refs
        .iter()
        .map(|b| compute_branch_derived(b))
        .collect::<Result<_, _>>()?;

    // Option 2 experiment: feed BOTH user and system events into Check #1.
    // Hypothesis: heartbeat closeBlocks read stateCh via peek (`<<-`), so
    // they populate `produces_peeked`, not `produces_consumed`. Only true
    // state-mutating runMVar consumes (regular `<-` inside `runMVar`)
    // populate `produces_consumed`. Feeding system events should surface
    // bonding-bug conflicts without re-triggering the bf3d274a flood —
    // because that flood was about consume/produce matching at the
    // potential-comms layer, not produces_consumed races.
    let all_event_logs: Vec<&rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex> =
        derived.iter().map(|d| &d.combined_all_event_log).collect();

    // Per-branch identity-tagged channels with non-commutative pending writes
    // (computed per-chain inside compute_branch_derived). Used by Check #4 to
    // detect cross-branch races on Layer-2 single-value contract slots.
    let identity_tagged_pending_produces: Vec<&HashSet<Blake2b256Hash>> = derived
        .iter()
        .map(|d| &d.identity_tagged_pending_produces)
        .collect();

    // Event-log conflicts: races, potential COMMs, base-join touches.
    // `mutable_key_type` is a false positive here: prost::bytes::Bytes uses an
    // internal Arc, not interior mutability, but clippy can't distinguish.
    #[allow(clippy::mutable_key_type)]
    let mut conflict_map = merging_logic::compute_conflict_map_event_indexed(
        &branches_owned,
        &all_event_logs,
        &identity_tagged_pending_produces,
    );

    // Same-user-deploy-id pass: for any user deploy ID appearing in multiple
    // branches, mark all such branches as mutual conflicts.
    let mut deploy_to_branches: HashMap<prost::bytes::Bytes, Vec<usize>> = HashMap::new();
    for (idx, d) in derived.iter().enumerate() {
        for id in &d.user_deploy_ids {
            deploy_to_branches.entry(id.clone()).or_default().push(idx);
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

    Ok(conflict_map)
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
            result
        }
        None => {
            // Legacy behavior: use descendants of LFB
            dag.descendants(lfb)?
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
                    collateral_lost_pairs
                        .push((deploy.deploy_id.clone(), chain.source_block_hash.clone()));
                }
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
            target: "f1r3fly.dag_merger.state_changes",
            "deploy_chain[{}]: datums={}, conts={}, joins={}, deploys={}, cost={}",
            i,
            chain.state_changes.datums_changes.len(),
            chain.state_changes.cont_changes.len(),
            chain.state_changes.consume_channels_to_join_serialized_map.len(),
            chain.deploys_with_cost.0.len(),
            chain.deploys_with_cost.0.iter().map(|d| d.cost).sum::<u64>(),
        );
    }

    // Keep as Vec for deterministic processing (ConflictSetMerger expects sorted Vecs)
    let mut actual_seq = actual_set_vec;
    let late_seq = late_set_vec;

    // Pre-computed data for a single DeployChainIndex, cached by pointer address
    // to avoid recomputing on every O(D²) depends() call.
    struct ChainDerived {
        produces_created: HashableSet<rspace_plus_plus::rspace::trace::event::Produce>,
        consumes_created: HashableSet<rspace_plus_plus::rspace::trace::event::Consume>,
    }

    // Lazy chain-derived cache keyed by pointer address. Safe because:
    // - References come from HashSet iteration, addresses stable during iteration
    // - DerivedSets is a pure function of the item
    let chain_cache: RefCell<HashMap<usize, ChainDerived>> = RefCell::new(HashMap::new());

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

    // Create history reader for base state
    let history_reader = std::sync::Arc::new(
        history_repository
            .get_history_reader(lfb_post_state)
            .map_err(|e| CasperError::HistoryError(e))?,
    );

    // Check #5 — Stale-removed chain detection (pre-merge filter).
    //
    // A chain is stale if its `state_changes.datums_changes[channel].removed`
    // contains bytes that are NOT in init for that channel AND NOT in any
    // other chain's `added` for that channel. Such chains were indexed
    // against a prior trie state that's no longer reachable through the
    // merge's chain set. Applying their diffs produces multi-Datum
    // (`make_trie_action` does `multiset_diff(init, removed) ++ added` —
    // stale removed survives the diff, added is appended on top).
    //
    // Reject stale chains BEFORE resolve_conflicts:
    //   * Removes them from actual_seq (no further consideration)
    //   * User deploys go to collateral_lost_pairs → rejected-deploy buffer
    //     → next proposer re-includes (cost-optimal user-deploy recovery path)
    //   * System deploys are silently dropped (atomic with block, no recovery)
    //
    // Uses a transient cache for init lookups to avoid repeated history reads
    // across multiple chains touching the same channel.
    let stale_chain_indices: HashSet<usize> = {
        let init_cache: RefCell<HashMap<Blake2b256Hash, Vec<Vec<u8>>>> =
            RefCell::new(HashMap::new());
        let init_of = |channel: &Blake2b256Hash| -> Vec<Vec<u8>> {
            let mut cache = init_cache.borrow_mut();
            if let Some(existing) = cache.get(channel) {
                return existing.clone();
            }
            let data = history_reader
                .get_data_proj_binary(channel)
                .unwrap_or_default();
            cache.insert(channel.clone(), data.clone());
            data
        };
        let chains_for_check: Vec<(usize, &StateChange)> = actual_seq
            .iter()
            .enumerate()
            .map(|(idx, c)| (idx, &c.state_changes))
            .collect();
        detect_stale_chains_pure(&chains_for_check, init_of)
    };
    if !stale_chain_indices.is_empty() {
        tracing::info!(
            target: "f1r3.trace.stale_chain",
            "[TRACE-CHECK-5-REJECTED] count={} of {} chains rejected as stale",
            stale_chain_indices.len(),
            actual_seq.len()
        );
        // Capture user-deploy IDs from stale chains for recovery buffer before dropping.
        let new_collateral = stale_chain_collateral_pure(
            &stale_chain_indices,
            actual_seq.len(),
            |idx| {
                actual_seq[idx]
                    .deploys_with_cost
                    .0
                    .iter()
                    .map(|d| d.deploy_id.clone())
                    .collect()
            },
            |idx| actual_seq[idx].source_block_hash.clone(),
        );
        collateral_lost_pairs.extend(new_collateral);
        // Drop stale chains from the merge set.
        let mut idx_counter = 0usize;
        actual_seq.retain(|_| {
            let keep = !stale_chain_indices.contains(&idx_counter);
            idx_counter += 1;
            keep
        });
    }

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
            return true;
        }

        derived
            .consumes_created
            .0
            .intersection(&target.event_log_index.consumes_produced.0)
            .next()
            .is_some()
    };

    let state_changes_fn = |chain: &DeployChainIndex| Ok(chain.state_changes.clone());

    let mergeable_channels_fn =
        |chain: &DeployChainIndex| chain.event_log_index.number_channels_data.clone();

    let compute_trie_actions_fn = {
        let reader = Arc::clone(&history_reader);
        move |changes, mergeable_chs| {
            state_change_merger::compute_trie_actions(
                &changes,
                &*reader,
                &mergeable_chs,
                |hash: &Blake2b256Hash, channel_changes, number_chs: &NumberChannelsDiff| {
                    let ch_hex: String = hash.encode_hex();
                    let in_mergeable = number_chs.contains_key(hash);
                    info!(
                        target: "f1r3.trace.dispatcher",
                        "[TRACE-DISPATCHER] channel={} in_mergeable_chs={} number_chs_size={}",
                        ch_hex, in_mergeable, number_chs.len()
                    );
                    if let Some(number_ch_val) = number_chs.get(hash) {
                        let (diff, merge_type) = *number_ch_val;
                        info!(
                            target: "f1r3.trace.dispatcher",
                            "[TRACE-DISPATCHER-FOLD] channel={} merge_type={:?} diff={}",
                            ch_hex, merge_type, diff
                        );
                        let base_get_data = |h: &Blake2b256Hash| reader.get_data(h);
                        Ok(Some(RholangMergingLogic::calculate_number_channel_merge(
                            hash,
                            diff,
                            merge_type,
                            channel_changes,
                            base_get_data,
                        )?))
                    } else {
                        info!(
                            target: "f1r3.trace.dispatcher",
                            "[TRACE-DISPATCHER-FALLBACK] channel={} (not in mergeable_chs)",
                            ch_hex
                        );
                        Ok(None)
                    }
                },
            )
        }
    };

    let lfb_hex: String = lfb_post_state.encode_hex();
    info!(
        target: "f1r3.trace.dag_merger",
        "[TRACE-DAG-MERGE-LFB-POST-STATE] lfb_post_state={}",
        lfb_hex
    );
    let apply_trie_actions_fn = |actions: Vec<_>| {
        info!(
            target: "f1r3.trace.dag_merger",
            "[TRACE-DAG-MERGE-APPLY-ACTIONS] actions_count={}",
            actions.len()
        );
        history_repository
            .reset(lfb_post_state)
            .map(|reset_repo| reset_repo.do_checkpoint(actions))
            .map(|checkpoint| {
                let root = checkpoint.root();
                let root_hex: String = root.encode_hex();
                info!(
                    target: "f1r3.trace.dag_merger",
                    "[TRACE-DAG-MERGE-APPLY-RESULT] new_root={}",
                    root_hex
                );
                root
            })
            .map_err(|e| e.into())
    };

    let get_data_fn = |hash| history_reader.get_data(&hash).map_err(|e| e.into());

    // Resolve conflicts: detect conflicts and select the cost-optimal rejection set.
    // Both branch-grouping and conflict-mapping are module-level pub functions
    // (see `compute_branches`, `compute_conflict_map` above) so the test suite
    // can exercise the exact production call path.
    let mut resolved = conflict_set_merger::resolve_conflicts(
        actual_seq,
        late_seq,
        &depends_fn,
        &rejection_cost_f,
        &mergeable_channels_fn,
        &get_data_fn,
        &compute_branches,
        &compute_conflict_map,
    )
    .map_err(|e| CasperError::HistoryError(e))?;

    // Rejection expansion. Any chain whose source block is a DAG descendant
    // of a rejected chain's source block (within merge scope) has pre-computed
    // diffs against a pre-state that the merge is about to drop. Expand the
    // rejection set to include those chains before applying diffs.
    //
    // All descendant chains are rejected unconditionally; an event-log
    // read/write analysis could narrow this, but event logs miss indirect
    // dependencies through system contracts (the very condition the expansion
    // exists to catch), so we prefer conservative correctness.
    let rejected_source_blocks: HashSet<BlockHash> = resolved
        .rejected
        .0
        .iter()
        .map(|chain| chain.source_block_hash.clone())
        .collect();

    let pre_expansion_rejected = resolved.rejected.0.len();

    let __exp_start = std::time::Instant::now();
    if !rejected_source_blocks.is_empty() {
        let descendant_blocks =
            descendants_within_scope(dag, &rejected_source_blocks, &actual_blocks);

        if !descendant_blocks.is_empty() {
            // Rebuild to_merge: any branch containing a chain from a descendant
            // block gets moved whole into rejected. Branches are dependency
            // clusters — rejecting partial branches would leave internally
            // inconsistent diffs.
            let mut new_to_merge: Vec<HashableSet<DeployChainIndex>> = Vec::new();
            for branch in resolved.to_merge.drain(..) {
                let has_stale = branch
                    .0
                    .iter()
                    .any(|chain| descendant_blocks.contains(&chain.source_block_hash));
                if has_stale {
                    for chain in branch.0 {
                        resolved.rejected.0.insert(chain);
                    }
                } else {
                    new_to_merge.push(branch);
                }
            }
            resolved.to_merge = new_to_merge;

            tracing::info!(
                "DagMerger rejection expansion: {} descendant blocks, rejected grew from {} to {} chains",
                descendant_blocks.len(),
                pre_expansion_rejected,
                resolved.rejected.0.len()
            );
            metrics::counter!(
                crate::rust::metrics_constants::DAG_MERGE_REJECTION_EXPANSION_FIRED_METRIC,
                "source" => crate::rust::metrics_constants::MERGING_METRICS_SOURCE
            )
            .increment(1);
        }
    }
    metrics::histogram!(
        crate::rust::metrics_constants::DAG_MERGE_REJECTION_EXPANSION_TIME_METRIC,
        "source" => crate::rust::metrics_constants::MERGING_METRICS_SOURCE
    )
    .record(__exp_start.elapsed().as_secs_f64());

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

    Ok((new_state, rejected_user_deploys, rejected_slashes))
}

#[cfg(test)]
mod check_5_stale_removed_tests {
    //! Regression tests for the multi-Datum-via-stale-removed bug observed
    //! in `test_bonding_validators` on channel `f104d43c...`.
    //!
    //! A chain whose `state_changes.datums_changes[channel].removed` contains
    //! bytes not present in `init` AND not present in any OTHER chain's
    //! `added` is stale — the chain ran against a prior trie state that's
    //! no longer reachable through the merge's chain set. Without detection,
    //! `make_trie_action` does `multiset_diff(init, removed) ++ added`, the
    //! stale `removed` survives the diff (no match in init), and the added
    //! Datum is appended on top → multi-Datum on a channel that should hold
    //! one value.
    //!
    //! `detect_stale_chains_pure` is the pre-merge filter. These tests
    //! verify it correctly flags the bug pattern and DOESN'T over-reject
    //! aligned or cross-chain-dependent cases.
    use super::detect_stale_chains_pure;
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    use rspace_plus_plus::rspace::merger::channel_change::ChannelChange;
    use rspace_plus_plus::rspace::merger::state_change::StateChange;

    fn channel(byte: u8) -> Blake2b256Hash {
        Blake2b256Hash::from_bytes(vec![byte; 32])
    }

    fn state_change_with_one_channel(
        ch: &Blake2b256Hash,
        added: Vec<Vec<u8>>,
        removed: Vec<Vec<u8>>,
    ) -> StateChange {
        let sc = StateChange::empty();
        sc.datums_changes
            .insert(ch.clone(), ChannelChange { added, removed });
        sc
    }

    /// Regression for the `f104d43c...` bug. A single chain claims to
    /// remove bytes that aren't in init and aren't covered by another
    /// chain's added → detected as stale.
    ///
    /// This test FAILS without `detect_stale_chains_pure` in place — proves
    /// the fix engages. Was `#[ignore]`-marked while the upstream detection
    /// was being designed; now active as a permanent regression guard.
    #[test]
    fn stale_chain_with_unmatched_removed_is_detected() {
        let ch = channel(0xf1);
        // Observed signature from session dd0c3c82:
        //   init=[ca1e8a3353dd74a2]
        //   chain.removed=[e40c3a3842739fd7]  (stale — not in init, not in other adds)
        //   chain.added=[2173751b1c07af72]
        let init_value: Vec<u8> = vec![0xca, 0x1e, 0x8a, 0x33];
        let stale_removed: Vec<u8> = vec![0xe4, 0x0c, 0x3a, 0x38];
        let chain_added: Vec<u8> = vec![0x21, 0x73, 0x75, 0x1b];

        let sc = state_change_with_one_channel(
            &ch,
            vec![chain_added],
            vec![stale_removed],
        );
        let chains: Vec<(usize, &StateChange)> = vec![(0, &sc)];
        let init_of = |c: &Blake2b256Hash| -> Vec<Vec<u8>> {
            if c == &ch {
                vec![init_value.clone()]
            } else {
                Vec::new()
            }
        };

        let stale = detect_stale_chains_pure(&chains, init_of);

        assert!(
            stale.contains(&0),
            "Chain with removed bytes not in init nor in other chains' added \
             must be detected as stale. detect_stale_chains_pure returned: {:?}",
            stale
        );
    }

    /// Control: a chain whose removed matches init is NOT stale.
    #[test]
    fn chain_aligned_with_init_is_not_stale() {
        let ch = channel(0xa1);
        let value: Vec<u8> = vec![0xca, 0x1e];

        let sc = state_change_with_one_channel(
            &ch,
            vec![vec![0x21, 0x73]],
            vec![value.clone()],
        );
        let chains: Vec<(usize, &StateChange)> = vec![(0, &sc)];
        let init_of = |c: &Blake2b256Hash| -> Vec<Vec<u8>> {
            if c == &ch {
                vec![value.clone()]
            } else {
                Vec::new()
            }
        };

        let stale = detect_stale_chains_pure(&chains, init_of);

        assert!(stale.is_empty(), "Aligned chain must not be flagged: {:?}", stale);
    }

    /// Control: chain B's removed matches chain A's added (cross-chain
    /// dependency). Neither chain is stale — A's added covers B's removed.
    #[test]
    fn cross_chain_dependency_is_not_stale() {
        let ch = channel(0xb2);
        let init_val: Vec<u8> = vec![0xca, 0x1e]; // present at init
        let intermediate: Vec<u8> = vec![0xab, 0xcd]; // A's added = B's removed
        let final_val: Vec<u8> = vec![0x21, 0x73];

        // Chain A: removed init, added intermediate
        let sc_a = state_change_with_one_channel(
            &ch,
            vec![intermediate.clone()],
            vec![init_val.clone()],
        );
        // Chain B: removed intermediate (covered by A's added), added final
        let sc_b = state_change_with_one_channel(
            &ch,
            vec![final_val.clone()],
            vec![intermediate.clone()],
        );
        let chains: Vec<(usize, &StateChange)> = vec![(0, &sc_a), (1, &sc_b)];
        let init_of = |c: &Blake2b256Hash| -> Vec<Vec<u8>> {
            if c == &ch {
                vec![init_val.clone()]
            } else {
                Vec::new()
            }
        };

        let stale = detect_stale_chains_pure(&chains, init_of);

        assert!(
            stale.is_empty(),
            "Cross-chain dependency must not be flagged stale: {:?}",
            stale
        );
    }

    /// Control: empty removed is never stale.
    #[test]
    fn empty_removed_is_not_stale() {
        let ch = channel(0xc3);
        let sc = state_change_with_one_channel(&ch, vec![vec![0x01]], Vec::new());
        let chains: Vec<(usize, &StateChange)> = vec![(0, &sc)];
        let init_of = |_: &Blake2b256Hash| Vec::new();

        let stale = detect_stale_chains_pure(&chains, init_of);

        assert!(stale.is_empty());
    }
}

#[cfg(test)]
mod stale_chain_collateral_tests {
    //! Regression tests for `stale_chain_collateral_pure`.
    //!
    //! Root cause of `test_bonding_validators` Liveness Phase failure
    //! (`runtime_manager.rs:1271` "Expected at most one value for number
    //! channel … found 2"):
    //!
    //! Check #5 stale-rejection captures EVERY user deploy in a dropped
    //! chain into `collateral_lost_pairs` → fed to `body.rejected_deploys`
    //! → triggers the recovery exemption in `prepare_user_deploys`. When a
    //! deploy also lives in a kept chain (its effects are in canonical
    //! pre-state via that chain), exemption re-execution stacks a second
    //! produce on a tagged channel → multi-Datum.
    //!
    //! The pure function MUST omit deploys present in any kept chain.

    use super::stale_chain_collateral_pure;
    use models::rust::block_hash::BlockHash;
    use prost::bytes::Bytes;
    use std::collections::HashSet;

    fn sig(b: u8) -> Bytes {
        Bytes::from(vec![b; 8])
    }

    fn block(b: u8) -> BlockHash {
        BlockHash::from(vec![b; 8])
    }

    /// Bug A regression. Same deploy in kept chain 0 AND stale chain 1.
    /// `stale_chain_collateral_pure` must exclude the duplicate from the
    /// collateral list — otherwise the recovery exemption re-executes it
    /// on top of canonical pre-state that already has its effects.
    #[test]
    fn collateral_excludes_deploys_present_in_any_kept_chain() {
        let deploy_x = sig(0xAA);
        let deploy_y = sig(0xBB);
        let src_kept = block(0x10);
        let src_stale = block(0x20);

        let chain_deploys = vec![vec![deploy_x.clone()], vec![deploy_x.clone(), deploy_y.clone()]];
        let chain_source = vec![src_kept.clone(), src_stale.clone()];
        let stale: HashSet<usize> = [1].iter().copied().collect();

        let collateral = stale_chain_collateral_pure(
            &stale,
            chain_deploys.len(),
            |idx| chain_deploys[idx].clone(),
            |idx| chain_source[idx].clone(),
        );

        let collateral_sigs: Vec<&Bytes> = collateral.iter().map(|(s, _)| s).collect();

        assert!(
            !collateral_sigs.iter().any(|s| **s == deploy_x),
            "deploy_x is present in kept chain 0 — its effects are in canonical \
             state via that chain. Adding it to collateral_lost_pairs would \
             trigger recovery exemption re-execution → double-execution → \
             multi-Datum on tagged channels (the test_bonding_validators \
             Liveness Phase blocker). Got collateral: {:?}",
            collateral_sigs
                .iter()
                .map(|s| hex::encode(s.as_ref()))
                .collect::<Vec<_>>()
        );

        assert!(
            collateral.iter().any(|(s, src)| *s == deploy_y && *src == src_stale),
            "deploy_y is unique to the stale chain — it MUST land in collateral \
             so the recovery buffer can re-propose it (otherwise legitimate \
             orphan recovery breaks)"
        );
    }

    /// Control: when no deploys overlap between kept and stale chains, all
    /// stale-chain user deploys land in collateral.
    #[test]
    fn collateral_includes_unique_stale_deploys() {
        let deploy_x = sig(0xAA);
        let deploy_y = sig(0xBB);
        let src_kept = block(0x10);
        let src_stale = block(0x20);

        let chain_deploys = vec![vec![deploy_x.clone()], vec![deploy_y.clone()]];
        let chain_source = vec![src_kept.clone(), src_stale.clone()];
        let stale: HashSet<usize> = [1].iter().copied().collect();

        let collateral = stale_chain_collateral_pure(
            &stale,
            chain_deploys.len(),
            |idx| chain_deploys[idx].clone(),
            |idx| chain_source[idx].clone(),
        );

        assert_eq!(collateral.len(), 1);
        assert_eq!(collateral[0].0, deploy_y);
        assert_eq!(collateral[0].1, src_stale);
    }

    /// Control: no stale chains → no collateral.
    #[test]
    fn no_stale_chains_yields_empty_collateral() {
        let deploy_x = sig(0xAA);
        let src = block(0x10);
        let chain_deploys = vec![vec![deploy_x]];
        let chain_source = vec![src];
        let stale: HashSet<usize> = HashSet::new();

        let collateral = stale_chain_collateral_pure(
            &stale,
            chain_deploys.len(),
            |idx| chain_deploys[idx].clone(),
            |idx| chain_source[idx].clone(),
        );

        assert!(collateral.is_empty());
    }
}
