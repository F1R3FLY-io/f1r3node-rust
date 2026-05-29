// See casper/src/main/scala/coop/rchain/casper/merging/DagMerger.scala

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use hex::ToHex;
use models::rhoapi::ListParWithRandom;
use models::rust::block_hash::BlockHash;
use prost::bytes::Bytes;
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rholang::rust::interpreter::pretty_printer::PrettyPrinter as RholangPrettyPrinter;
use rholang::rust::interpreter::rho_runtime::RhoHistoryRepository;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::Datum;
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
/// Rejected chains are dropped from `actual_seq` so their diffs are not
/// applied. Their deploys are NOT propagated to the rejected-deploy buffer:
/// each stale chain's source block is a valid in-scope DAG block whose
/// `body.deploys` carries the user-deploy commitment forward; treating it
/// as "rejected, needs recovery" causes multi-Datum (the chain rejection
/// is about the diff being unsafe, not about the commitment).
pub fn detect_stale_chains_pure<C: Eq + std::hash::Hash + Clone + std::fmt::Debug>(
    chains: &[(C, &StateChange)],
    init_of: impl Fn(&Blake2b256Hash) -> Vec<Vec<u8>>,
    is_mergeable: impl Fn(&C, &Blake2b256Hash) -> bool,
) -> HashSet<C> {
    use std::collections::HashMap;

    // Pass 1: build global_added per channel.
    let mut global_added: HashMap<Blake2b256Hash, Vec<Vec<u8>>> = HashMap::new();
    for (_, sc) in chains {
        for entry in sc.datums_changes.iter() {
            let ch = entry.key().clone();
            let added = &entry.value().added;
            global_added
                .entry(ch)
                .or_default()
                .extend(added.iter().cloned());
        }
    }

    // Pass 2: per-chain staleness check.
    let mut stale: HashSet<C> = HashSet::new();
    for (chain_id, sc) in chains {
        let mut is_stale = false;
        for entry in sc.datums_changes.iter() {
            let channel = entry.key();
            // Number/mergeable channels are merged by value-fold
            // (`calculate_number_channel_merge`), never via
            // `make_trie_action`'s multiset-diff. Their datum-level
            // remove/add is just the materialization of a delta and is
            // irrelevant to staleness — the fold reconciles by summing
            // diffs. Flagging them stale is a false positive that drops the
            // whole chain (and any bond/vault effect bundled in it). When
            // `chain_id` survives conflict resolution, this channel is in
            // `all_mergeable_channels` (fold path); if it's rejected, its
            // diff never applies — so skipping its staleness here cannot
            // produce a multi-Datum.
            if is_mergeable(chain_id, channel) {
                continue;
            }
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
                        let removed_hash = Blake2b256Hash::new(r);
                        let removed_hash_bytes = removed_hash.bytes();
                        let in_init = init.iter().any(|d| d == r);
                        let in_global_added = global_added
                            .get(channel)
                            .map(|v| v.iter().any(|d| d == r))
                            .unwrap_or(false);
                        tracing::info!(
                            target: "f1r3.trace.stale_chain",
                            "[TRACE-CHECK-5-STALE] chain_idx={:?} channel={} removed_hash={} removed_len={} in_init={} in_global_added={} chain_removed_count={} init_count={} global_added_count={}",
                            chain_id,
                            hex::encode(channel.bytes()),
                            hex::encode(&removed_hash_bytes),
                            r.len(),
                            in_init,
                            in_global_added,
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

/// Deploy sigs whose effects are committed in `base`'s pre-state: clean
/// (non-failed) inclusions in `base`'s ancestry that were NOT later rejected
/// within that ancestry, bounded by `deploy_lifespan` blocks below `base`.
///
/// Used to detect recovery double-execution at the merge layer. The walk is
/// anchored to the fixed merge base and reads content-addressed block bodies,
/// so the result is identical across nodes — unlike the validator-side
/// LFB-anchored recovery gate, which depends on local finalization progress.
///
/// `clean − rejected`: a deploy that is clean in one ancestor but rejected in
/// another within the window is treated as NOT committed (its effects may have
/// been removed), so a legitimate recovery of it is not wrongly dropped.
pub fn base_committed_deploy_sigs(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    base: &BlockHash,
    deploy_lifespan: i64,
) -> HashSet<Bytes> {
    let floor = dag
        .block_number(base)
        .map(|h| (h - deploy_lifespan).max(0))
        .unwrap_or(0);
    let mut clean: HashSet<Bytes> = HashSet::new();
    let mut rejected: HashSet<Bytes> = HashSet::new();
    let mut visited: HashSet<BlockHash> = HashSet::new();
    let mut frontier: Vec<BlockHash> = vec![base.clone()];
    while let Some(h) = frontier.pop() {
        if !visited.insert(h.clone()) {
            continue;
        }
        match dag.block_number(&h) {
            Some(height) if height < floor => continue,
            None => continue,
            _ => {}
        }
        let block = match block_store.get(&h) {
            Ok(Some(b)) => b,
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!(
                    "base_committed_deploy_sigs: block_store.get failed for {}: {} — continuing",
                    hex::encode(&h),
                    e
                );
                continue;
            }
        };
        for pd in &block.body.deploys {
            if !pd.is_failed {
                clean.insert(pd.deploy.sig.clone());
            }
        }
        for rd in &block.body.rejected_deploys {
            rejected.insert(rd.sig.clone());
        }
        for parent in &block.header.parents_hash_list {
            if !visited.contains(parent) {
                frontier.push(parent.clone());
            }
        }
    }
    clean.retain(|sig| !rejected.contains(sig));
    clean
}

/// Detect chains that re-execute a deploy already committed in the base
/// pre-state ("recovery double-execution"). A chain whose deploy IDs include
/// any sig for which `base_committed` returns true ran on top of a state that
/// already holds that deploy's effects — re-applying its diff double-executes
/// the deploy (a second NN/vault-init datum → multi-Datum). Such chains must
/// be dropped from the merge, and their deploys must NOT be queued for
/// recovery (the base commitment already carries them forward).
///
/// `base_committed(sig)` reports whether `sig`'s effects are already in the
/// base, computed deterministically from the fixed merge base's ancestry —
/// unlike the validator-side LFB anchor, this does not depend on the local
/// node's finalization progress, so all nodes agree.
fn detect_base_recovery_chains_pure(
    chains: &[(usize, Vec<Bytes>)],
    base_committed: impl Fn(&Bytes) -> bool,
) -> HashSet<usize> {
    let mut flagged = HashSet::new();
    for (idx, deploy_ids) in chains {
        if deploy_ids.iter().any(|sig| base_committed(sig)) {
            flagged.insert(*idx);
        }
    }
    flagged
}


#[cfg(test)]
mod base_recovery_dedup_tests {
    use std::collections::HashSet;

    use prost::bytes::Bytes;

    use super::detect_base_recovery_chains_pure;

    fn sig(b: u8) -> Bytes { Bytes::from(vec![b; 8]) }

    /// A chain whose deploy is already committed in the base re-executes it on
    /// a state that already holds its effects → double-execution. It must be
    /// flagged so the chain is dropped and the deploy is NOT queued for
    /// recovery. FAILS until `detect_base_recovery_chains_pure` implements the
    /// base check (proves the fix engages).
    #[test]
    fn chain_reexecuting_base_committed_deploy_is_detected() {
        let committed = sig(0xaa);
        let fresh = sig(0xbb);
        let chains = vec![
            (0usize, vec![committed.clone()]), // re-executes a base-committed deploy
            (1usize, vec![fresh.clone()]),     // genuinely new
        ];
        let base: HashSet<Bytes> = HashSet::from([committed.clone()]);

        let flagged = detect_base_recovery_chains_pure(&chains, |s| base.contains(s));

        assert!(
            flagged.contains(&0),
            "chain re-executing a base-committed deploy must be flagged as a \
             recovery double-execution; got {:?}",
            flagged
        );
        assert!(
            !flagged.contains(&1),
            "chain with only fresh deploys must NOT be flagged; got {:?}",
            flagged
        );
    }

    /// Control: an empty base flags nothing (no false positives — genuinely
    /// new deploys still merge and, if lost, recover normally).
    #[test]
    fn no_chains_flagged_when_base_is_empty() {
        let chains = vec![(0usize, vec![sig(0xaa)]), (1usize, vec![sig(0xbb)])];
        let flagged = detect_base_recovery_chains_pure(&chains, |_| false);
        assert!(
            flagged.is_empty(),
            "empty base must flag nothing; got {:?}",
            flagged
        );
    }
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
    // Deploy sigs already committed (clean) in the base pre-state's ancestry.
    // A chain re-executing one of these is a recovery double-execution: it is
    // dropped from the merge and excluded from the rejected-deploy buffer.
    base_committed_sigs: &HashSet<Bytes>,
) -> Result<
    (
        Blake2b256Hash,
        Vec<(Bytes, BlockHash)>,
        Vec<(Bytes, BlockHash)>,
        // kept_chain_sigs: sigs from kept (`to_merge`) chains paired with
        // their source block's height. This is the merge's CANONICAL view
        // of "deploys whose effects are in the merged pre-state from this
        // round" — the kept-chain semantics that the naive
        // `rejected_deploys` subtract cannot capture (a sig in both a kept
        // and a rejected chain is correctly marked applied here).
        // See applied-sigs-design.md §3.
        HashMap<Bytes, i64>,
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
                tracing::info!(
                    target: "f1r3.trace.merge_reject",
                    "[MERGE-DEDUP-DROP] sig={} dropped_from_block={} dropped_block_num={} is_collateral={} fresher_block={} fresher_num={}",
                    hex::encode(&deploy.deploy_id),
                    hex::encode(&chain.source_block_hash),
                    chain.source_block_number,
                    is_collateral,
                    best.map(|(_, h)| hex::encode(&h))
                        .unwrap_or_else(|| "none".to_string()),
                    best.map(|(n, _)| n.to_string())
                        .unwrap_or_else(|| "none".to_string()),
                );
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

    // Base-recovery dedup: drop any chain that re-executes a deploy already
    // committed (clean) in the base pre-state's ancestry. Such a chain ran on a
    // state that already holds the deploy's effects, so re-applying its diff
    // double-executes it (a second NN/vault-init datum → multi-Datum). The base
    // set is derived from the fixed merge base, so this decision is identical
    // across all nodes — unlike the validator-side LFB-anchored gate. A
    // base-committed deploy is NOT queued for recovery (the base commitment
    // already carries it forward); only genuinely-new deploys in a dropped
    // chain become recovery collateral.
    if !base_committed_sigs.is_empty() && !actual_set_vec.is_empty() {
        let chain_deploy_ids: Vec<(usize, Vec<Bytes>)> = actual_set_vec
            .iter()
            .enumerate()
            .map(|(i, chain)| {
                let ids = chain
                    .deploys_with_cost
                    .0
                    .iter()
                    .map(|d| d.deploy_id.clone())
                    .collect();
                (i, ids)
            })
            .collect();
        let flagged = detect_base_recovery_chains_pure(&chain_deploy_ids, |sig| {
            base_committed_sigs.contains(sig)
        });
        if !flagged.is_empty() {
            for (i, chain) in actual_set_vec.iter().enumerate() {
                if !flagged.contains(&i) {
                    continue;
                }
                for deploy in chain.deploys_with_cost.0.iter() {
                    // Skip system deploys and the base-committed deploys
                    // themselves; only the chain's genuinely-new deploys recover.
                    if is_system_deploy_id(&deploy.deploy_id)
                        || base_committed_sigs.contains(&deploy.deploy_id)
                    {
                        continue;
                    }
                    collateral_lost_pairs
                        .push((deploy.deploy_id.clone(), chain.source_block_hash.clone()));
                }
            }
            tracing::info!(
                target: "f1r3.trace.merge_reject",
                "[MERGE-BASE-RECOVERY-DROP] dropped {} chain(s) re-executing base-committed deploys",
                flagged.len()
            );
            let mut idx = 0usize;
            actual_set_vec.retain(|_| {
                let keep = !flagged.contains(&idx);
                idx += 1;
                keep
            });
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
    //   * Deploys are NOT propagated to the rejected-deploy buffer: each
    //     stale chain's source block is a valid in-scope DAG block whose
    //     body.deploys carries the commitment forward. Treating these as
    //     "rejected, needs recovery" would re-execute on a hot store
    //     already holding the prior write → multi-Datum on tagged channels.
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
        // A channel is exempt from the staleness check for a given chain
        // when that chain carries it as a number/mergeable channel — those
        // are value-folded at merge time, not Datum-replaced, so a
        // "stale removed" on them is a false positive (see detect_stale_chains_pure).
        let is_mergeable = |idx: &usize, channel: &Blake2b256Hash| -> bool {
            actual_seq[*idx]
                .event_log_index
                .number_channels_data
                .contains_key(channel)
        };
        detect_stale_chains_pure(&chains_for_check, init_of, is_mergeable)
    };
    if !stale_chain_indices.is_empty() {
        tracing::info!(
            target: "f1r3.trace.stale_chain",
            "[TRACE-CHECK-5-REJECTED] count={} of {} chains rejected as stale",
            stale_chain_indices.len(),
            actual_seq.len()
        );
        for (idx, chain) in actual_seq.iter().enumerate() {
            let role = if stale_chain_indices.contains(&idx) {
                "STALE"
            } else {
                "KEPT"
            };
            let sigs: String = chain
                .deploys_with_cost
                .0
                .iter()
                .map(|d| hex::encode(&d.deploy_id))
                .collect::<Vec<_>>()
                .join(",");
            tracing::info!(
                target: "f1r3.trace.stale_chain",
                "[TRACE-CHECK-5-CHAIN-SOURCE] role={} chain_idx={} source_block={} block_num={} deploys_count={} sigs=[{}]",
                role,
                idx,
                hex::encode(&chain.source_block_hash),
                chain.source_block_number,
                chain.deploys_with_cost.0.len(),
                sigs,
            );
            // Dump per-channel datums_changes for this chain so we can see
            // the actual diff content (which Datum bytes the chain adds/
            // removes per channel) and correlate with multi-Datum and
            // bonds-degeneration investigations. Logs byte length + a
            // Blake2b256 content hash of each Datum so different values
            // are distinguishable (raw bytes can be megabytes).
            for entry in chain.state_changes.datums_changes.iter() {
                let channel_hex =
                    hex::encode(&entry.key().bytes()[..8.min(entry.key().bytes().len())]);
                let fingerprint = |d: &Vec<u8>| -> String {
                    let h = Blake2b256Hash::new(d);
                    let h_bytes = h.bytes();
                    format!("len={}/h={}", d.len(), hex::encode(&h_bytes))
                };
                let added_fps: String = entry
                    .value()
                    .added
                    .iter()
                    .map(fingerprint)
                    .collect::<Vec<_>>()
                    .join(",");
                let removed_fps: String = entry
                    .value()
                    .removed
                    .iter()
                    .map(fingerprint)
                    .collect::<Vec<_>>()
                    .join(",");
                tracing::info!(
                    target: "f1r3.trace.stale_chain",
                    "[TRACE-CHAIN-DIFF] role={} chain_idx={} source_block={} channel={} added_count={} removed_count={} added=[{}] removed=[{}]",
                    role,
                    idx,
                    hex::encode(&chain.source_block_hash),
                    channel_hex,
                    entry.value().added.len(),
                    entry.value().removed.len(),
                    added_fps,
                    removed_fps,
                );
                // For register-modify channels (non-empty removed), decode the
                // raw Datum bytes into Pars so we can read the actual value
                // (additive accumulator vs map vs scalar) and decide the
                // proper merge semantics. bincode(Datum<ListParWithRandom>).
                if !entry.value().removed.is_empty() {
                    let decode = |d: &Vec<u8>| -> String {
                        match bincode::deserialize::<Datum<ListParWithRandom>>(d) {
                            Ok(datum) => {
                                let mut pp = RholangPrettyPrinter::new();
                                let body = datum
                                    .a
                                    .pars
                                    .iter()
                                    .map(|p| pp.build_string_from_message(p))
                                    .collect::<Vec<_>>()
                                    .join(" | ");
                                let truncated: String = body.chars().take(400).collect();
                                format!("persist={} par={}", datum.persist, truncated)
                            }
                            Err(e) => format!("<decode-failed: {}>", e),
                        }
                    };
                    for (i, a) in entry.value().added.iter().enumerate() {
                        tracing::info!(
                            target: "f1r3.trace.stale_chain",
                            "[TRACE-CHAIN-DIFF-DECODED] chain_idx={} channel={} kind=added i={} {}",
                            idx, channel_hex, i, decode(a),
                        );
                    }
                    for (i, r) in entry.value().removed.iter().enumerate() {
                        tracing::info!(
                            target: "f1r3.trace.stale_chain",
                            "[TRACE-CHAIN-DIFF-DECODED] chain_idx={} channel={} kind=removed i={} {}",
                            idx, channel_hex, i, decode(r),
                        );
                    }
                }
            }
        }
        // Drop stale chains from the merge set. We do NOT propagate the
        // dropped chains' user deploys as recovery collateral: each stale
        // chain's `source_block` is a valid in-scope DAG block whose
        // `body.deploys` records the deploy as committed. Treating those
        // deploys as "rejected, needs recovery" would trigger the Bug B
        // admit-back path and re-execute the deploy on a hot store that
        // already holds its prior write — multi-Datum on tagged channels.
        // The block's commitment carries the deploy forward without need
        // of the rejected-deploy buffer.
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

    // DIAG: order-stable per-chain fingerprint (sorts deploy_ids + datums_changes
    // + cont_changes) so resolve_conflicts' stage fps are deterministic across
    // node processes — unlike raw Debug, which hits DashMap iteration order.
    let diag_chain_fp = |c: &DeployChainIndex| -> String {
        let mut dids: Vec<String> = c
            .deploys_with_cost
            .0
            .iter()
            .map(|d| hex::encode(&d.deploy_id))
            .collect();
        dids.sort();
        let mut dch: Vec<String> = c
            .state_changes
            .datums_changes
            .iter()
            .map(|e| {
                let mut a: Vec<String> = e.value().added.iter().map(|x| hex::encode(x)).collect();
                let mut r: Vec<String> = e.value().removed.iter().map(|x| hex::encode(x)).collect();
                a.sort();
                r.sort();
                format!(
                    "{}|{}|{}",
                    hex::encode(e.key().bytes()),
                    a.join(";"),
                    r.join(";")
                )
            })
            .collect();
        dch.sort();
        let mut cch: Vec<String> = c
            .state_changes
            .cont_changes
            .iter()
            .map(|e| {
                let k: String = e
                    .key()
                    .iter()
                    .map(|h| hex::encode(h.bytes()))
                    .collect::<Vec<_>>()
                    .join("+");
                let mut a: Vec<String> = e.value().added.iter().map(|x| hex::encode(x)).collect();
                let mut r: Vec<String> = e.value().removed.iter().map(|x| hex::encode(x)).collect();
                a.sort();
                r.sort();
                format!("{}|{}|{}", k, a.join(";"), r.join(";"))
            })
            .collect();
        cch.sort();
        hex::encode(
            Blake2b256Hash::new(
                format!(
                    "src={}#dids={}#D={}#C={}",
                    hex::encode(&c.source_block_hash),
                    dids.join(","),
                    dch.join(" "),
                    cch.join(" ")
                )
                .as_bytes(),
            )
            .bytes(),
        )
    };

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
        &diag_chain_fp,
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

    // Bug-A principle extended to the conflict-rejection path: collect the
    // deploy sigs that survive in KEPT (to_merge) chains. A rejected deploy
    // whose sig also appears in a kept chain has its effects applied via that
    // chain, so queuing it for recovery would double-execute it (re-running its
    // vault/NN init produces a second datum -> multi-Datum -> the refund's
    // `sub` reads a stale value -> "Insufficient funds"). Such sigs are excluded
    // from the rejected-deploy buffer below; genuinely-lost deploys (sig not in
    // any kept chain) are still buffered for legitimate recovery.
    let kept_sigs: HashSet<Bytes> = resolved
        .to_merge
        .iter()
        .flat_map(|branch| branch.0.iter())
        .flat_map(|chain| chain.deploys_with_cost.0.iter())
        .map(|deploy| deploy.deploy_id.clone())
        .collect();

    // Build the per-sig height map for applied_sigs (Phase 1 merge-integrated
    // computation). Each kept chain contributes its deploys at the chain's
    // source block number. Same sig in multiple kept chains takes min-height
    // (earliest application is canonical).
    let kept_chain_sigs: HashMap<Bytes, i64> = {
        let mut acc: HashMap<Bytes, i64> = HashMap::new();
        for branch in resolved.to_merge.iter() {
            for chain in branch.0.iter() {
                let h = chain.source_block_number;
                for deploy in chain.deploys_with_cost.0.iter() {
                    if is_system_deploy_id(&deploy.deploy_id) {
                        continue;
                    }
                    acc.entry(deploy.deploy_id.clone())
                        .and_modify(|existing| {
                            if h < *existing {
                                *existing = h;
                            }
                        })
                        .or_insert(h);
                }
            }
        }
        acc
    };

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

    // Anti-double-execute: drop any rejected user deploy whose sig is applied
    // via a kept chain (see kept_sigs above). This prevents recovery from
    // re-executing a deploy whose effect is already in the merged result.
    let __before_kept_dedup = rejected_user_deploys.len();
    rejected_user_deploys
        .retain(|(sig, _)| !kept_sigs.contains(sig) && !base_committed_sigs.contains(sig));
    if rejected_user_deploys.len() < __before_kept_dedup {
        tracing::info!(
            "DagMerger: excluded {} rejected user deploys also present in kept chains (anti-double-execute)",
            __before_kept_dedup - rejected_user_deploys.len(),
        );
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
            .map(|(sig, src)| {
                format!(
                    "{}@{}",
                    hex::encode(&sig[..std::cmp::min(8, sig.len())]),
                    hex::encode(&src[..std::cmp::min(8, src.len())])
                )
            })
            .collect();
        tracing::info!(
            "DagMerger rejected {} user deploys (sig@source_block): {}",
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

    let new_state_hex = hex::encode(new_state.bytes());
    let lfb_hex = hex::encode(lfb_post_state.bytes());
    let equal_to_lfb = new_state_hex == lfb_hex;
    let to_merge_chains: usize = resolved.to_merge.iter().map(|b| b.0.len()).sum();
    let rejected_chains: usize = rejected.0.len();
    let rejected_user_sigs: Vec<String> = rejected_user_deploys
        .iter()
        .map(|(sig, src)| {
            format!(
                "{}@{}",
                hex::encode(&sig[..std::cmp::min(8, sig.len())]),
                hex::encode(&src[..std::cmp::min(8, src.len())])
            )
        })
        .collect();
    tracing::info!(
        target: "f1r3.trace.merge_provenance",
        "[TRACE-MERGE-RESULT-PROVENANCE] new_state={} lfb_post_state={} equal_to_lfb={} to_merge_chains={} rejected_chains={} rejected_user_sigs=[{}]",
        new_state_hex,
        lfb_hex,
        equal_to_lfb,
        to_merge_chains,
        rejected_chains,
        rejected_user_sigs.join(","),
    );

    Ok((
        new_state,
        rejected_user_deploys,
        rejected_slashes,
        kept_chain_sigs,
    ))
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
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
    use rspace_plus_plus::rspace::merger::channel_change::ChannelChange;
    use rspace_plus_plus::rspace::merger::state_change::StateChange;

    use super::detect_stale_chains_pure;

    fn channel(byte: u8) -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![byte; 32]) }

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

        let sc = state_change_with_one_channel(&ch, vec![chain_added], vec![stale_removed]);
        let chains: Vec<(usize, &StateChange)> = vec![(0, &sc)];
        let init_of = |c: &Blake2b256Hash| -> Vec<Vec<u8>> {
            if c == &ch {
                vec![init_value.clone()]
            } else {
                Vec::new()
            }
        };

        let stale = detect_stale_chains_pure(&chains, init_of, |_, _| false);

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

        let sc = state_change_with_one_channel(&ch, vec![vec![0x21, 0x73]], vec![value.clone()]);
        let chains: Vec<(usize, &StateChange)> = vec![(0, &sc)];
        let init_of = |c: &Blake2b256Hash| -> Vec<Vec<u8>> {
            if c == &ch {
                vec![value.clone()]
            } else {
                Vec::new()
            }
        };

        let stale = detect_stale_chains_pure(&chains, init_of, |_, _| false);

        assert!(
            stale.is_empty(),
            "Aligned chain must not be flagged: {:?}",
            stale
        );
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
        let sc_a =
            state_change_with_one_channel(&ch, vec![intermediate.clone()], vec![init_val.clone()]);
        // Chain B: removed intermediate (covered by A's added), added final
        let sc_b =
            state_change_with_one_channel(&ch, vec![final_val.clone()], vec![intermediate.clone()]);
        let chains: Vec<(usize, &StateChange)> = vec![(0, &sc_a), (1, &sc_b)];
        let init_of = |c: &Blake2b256Hash| -> Vec<Vec<u8>> {
            if c == &ch {
                vec![init_val.clone()]
            } else {
                Vec::new()
            }
        };

        let stale = detect_stale_chains_pure(&chains, init_of, |_, _| false);

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

        let stale = detect_stale_chains_pure(&chains, init_of, |_, _| false);

        assert!(stale.is_empty());
    }
}
