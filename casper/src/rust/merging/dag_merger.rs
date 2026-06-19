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
        deploy_chain_index
            .deploys_with_cost
            .0
            .iter()
            .map(|deploy| deploy.cost)
            .sum()
    }
}

/// True iff `sub` is a sub-multiset of `sup`: every datum appears in `sup` at
/// least as many times as in `sub`. The single-value-cell serialization uses
/// this to decide whether a chain's consumed (`removed`) datums are still
/// available in the running cell state.
fn is_sub_multiset(sub: &[Vec<u8>], sup: &[Vec<u8>]) -> bool {
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

/// Finalized counterparties for a merge — the enforcement window between the
/// base floor and the fork-point cover of the conflict scope. Currently unused:
/// its only producer (`floor_seal::enforcement_window`) was removed in the
/// buffer-drain change, so every live caller passes `None`. Retained (dead-but-
/// wired through `merge`/`resolve_conflicts`) pending the follow-up that strips
/// the `final_context` parameter end-to-end.
pub struct FinalContext {
    /// Seal-accepted chains in the window: conflict counterparties whose
    /// effects are in the base. Never merged, never re-litigated.
    pub accepted_chains: Vec<DeployChainIndex>,
    /// Seal-rejected chains in the window: depends-sources for force-rejection.
    pub rejected_chains: Vec<DeployChainIndex>,
    /// User-deploy sigs of accepted window chains: a conflict chain carrying
    /// one is a duplicate of finalized work and is force-rejected. Deliberately
    /// NOT the cumulative finalization-rejected set — a rejected deploy must
    /// stay re-proposable above the floor (the recovery path).
    pub enforce_sigs: HashSet<Bytes>,
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
    final_context: Option<&FinalContext>,
) -> Result<
    (
        Blake2b256Hash,
        // Applied (kept-chain) user-deploy (sig, source block) pairs — the
        // deploys whose effect IS in this merged state. The seal records these
        // as FS's accepted ledger so a deploy finalized-accepted at any
        // inclusion is never re-proposed (the content-twin fix).
        Vec<(Bytes, BlockHash)>,
        Vec<(Bytes, BlockHash)>,
        Vec<(Bytes, BlockHash)>,
    ),
    CasperError,
> {
    let lfb_hex = hex::encode(lfb_post_state.bytes());
    tracing::debug!(
        target: "f1r3fly.merge.dag",
        lfb_post_state = %lfb_hex,
        "dag merge lfb post-state"
    );

    let lfb_hex = hex::encode(lfb_post_state.bytes());
    tracing::debug!(
        target: "f1r3fly.merge.dag",
        lfb_post_state = %lfb_hex,
        "dag merge lfb post-state"
    );

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

        // ALL chains contribute to the branch's combined event log, system-deploy
        // chains included. CloseBlock writes the whole PoS state cell at epoch
        // boundaries; excluding system chains here made two sibling boundary
        // writes invisible to conflict detection, so both whole-cell deltas were
        // applied and the single-value cell went multi-datum (then getBonds reads
        // diverged per node -> InvalidBondsCache). RChain indexes system deploys
        // as first-class conflict participants for the same reason.
        let combined_event_log = branch
            .0
            .iter()
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
                    let ch_hex = hex::encode(hash.bytes());
                    tracing::trace!(
                        target: "f1r3fly.rholang.dispatcher",
                        channel = %ch_hex,
                        in_mergeable_chs = number_chs.get(hash).is_some(),
                        number_chs_size = number_chs.len(),
                        "merge dispatcher channel"
                    );
                    if let Some(number_ch_val) = number_chs.get(hash) {
                        let (diff, merge_type) = *number_ch_val;
                        tracing::trace!(
                            target: "f1r3fly.rholang.dispatcher",
                            channel = %ch_hex,
                            merge_type = ?merge_type,
                            diff,
                            "merge dispatcher fold path"
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
                        tracing::trace!(
                            target: "f1r3fly.rholang.dispatcher",
                            channel = %ch_hex,
                            "merge dispatcher fallback path"
                        );
                        // DIAG: identify the non-mergeable channel by decoding its
                        // base value(s) — numeric (a vault/number cell) vs a Par
                        // (a coordination/lock channel).
                        if let Ok(base) = reader.get_data(hash) {
                            let vals: Vec<String> = base
                                .iter()
                                .map(
                                    |d| match RholangMergingLogic::try_get_number_with_rnd(&d.a) {
                                        Some((n, _)) => format!("num:{}", n),
                                        None => "par".to_string(),
                                    },
                                )
                                .collect();
                            tracing::debug!(
                                target: "f1r3fly.merge.channel_id",
                                channel = %ch_hex,
                                base_count = base.len(),
                                base_values = %vals.join(","),
                                added = channel_changes.added.len(),
                                removed = channel_changes.removed.len(),
                                "non-mergeable channel base values"
                            );
                        }
                        Ok(None)
                    }
                },
            )
        }
    };

    let apply_trie_actions_fn = |actions: Vec<_>| {
        tracing::debug!(
            target: "f1r3fly.merge.dag",
            actions_count = actions.len(),
            "dag merge apply actions"
        );
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

        // Event-log conflicts: races, potential COMMs, base-join touches.
        // `mutable_key_type` is a false positive here: prost::bytes::Bytes uses an
        // internal Arc, not interior mutability, but clippy can't distinguish.
        #[allow(clippy::mutable_key_type)]
        let mut conflict_map =
            merging_logic::compute_conflict_map_event_indexed(&branches_owned, &event_logs);

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
            merging_logic::gather_related_sets(&depends_map)
        };

    // Enforcement inputs: finalized window chains as counterparties + the
    // duplicate-of-finalized sig predicate.
    let empty_final_set = conflict_set_merger::FinalSet::empty();
    let final_set = final_context.map_or(empty_final_set, |ctx| conflict_set_merger::FinalSet {
        accepted: ctx.accepted_chains.clone(),
        rejected: ctx.rejected_chains.clone(),
    });
    let carries_enforced_sig = |chain: &DeployChainIndex| -> bool {
        final_context.is_some_and(|ctx| {
            !ctx.enforce_sigs.is_empty()
                && chain.deploys_with_cost.0.iter().any(|deploy| {
                    !is_system_deploy_id(&deploy.deploy_id)
                        && ctx.enforce_sigs.contains(&deploy.deploy_id)
                })
        })
    };

    // DIAG: attribute enforcement-window force-rejections. For each force-
    // rejected branch, classify its relationship to the finalized window chains:
    //   succ_acc  — branch CONSUMED an accepted final chain's OUTPUT produce
    //               (a legitimate RMW successor building on finalized state);
    //   race_acc  — branch CONSUMED the SAME INPUT produce an accepted final
    //               chain consumed (a genuine double-spend race);
    //   succ_rej / race_rej — same vs the rejected window chains.
    // This discriminates "successor force-rejected (the bug)" from "genuine race
    // / stale-base recovery (force-rejection correct, bug elsewhere)".
    let force_reject_logger = |branch: &HashableSet<DeployChainIndex>,
                               conflicts_final: bool,
                               enforced_sig: bool,
                               depends_rej: bool| {
        use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
        let branch_log = branch
            .0
            .iter()
            .map(|c| &c.event_log_index)
            .try_fold(EventLogIndex::empty(), |acc, x| {
                EventLogIndex::combine(&acc, x)
            })
            .ok();
        let consumed: std::collections::HashSet<_> = branch_log
            .as_ref()
            .map(|bl| bl.produces_consumed.0.iter().cloned().collect())
            .unwrap_or_default();
        let fc_sig = |fc: &DeployChainIndex| -> String {
            fc.deploys_with_cost
                .0
                .iter()
                .filter(|d| !is_system_deploy_id(&d.deploy_id))
                .map(|d| hex::encode(&d.deploy_id[..d.deploy_id.len().min(8)]))
                .collect::<Vec<_>>()
                .join("/")
        };
        let classify = |chains: &[DeployChainIndex]| -> (Vec<String>, Vec<String>) {
            let (mut succ, mut race) = (Vec::new(), Vec::new());
            for fc in chains {
                let out = merging_logic::produces_created_and_not_destroyed(&fc.event_log_index);
                let inp: std::collections::HashSet<_> =
                    fc.event_log_index.produces_consumed.0.iter().collect();
                let label = fc_sig(fc);
                if label.is_empty() {
                    continue;
                }
                if consumed.iter().any(|p| out.0.contains(p)) {
                    succ.push(label.clone());
                }
                if consumed.iter().any(|p| inp.contains(p)) {
                    race.push(label);
                }
            }
            (succ, race)
        };
        let (succ_acc, race_acc) = classify(&final_set.accepted);
        let (succ_rej, race_rej) = classify(&final_set.rejected);
        for chain in branch.0.iter() {
            let sigs: Vec<String> = chain
                .deploys_with_cost
                .0
                .iter()
                .filter(|d| !is_system_deploy_id(&d.deploy_id))
                .map(|d| hex::encode(&d.deploy_id[..d.deploy_id.len().min(8)]))
                .collect();
            if sigs.is_empty() {
                continue;
            }
            let mut nonfold_chans: Vec<String> = Vec::new();
            for e in chain.state_changes.datums_changes.iter() {
                let ch = e.key();
                if !chain.event_log_index.number_channels_data.contains_key(ch) {
                    nonfold_chans.push(hex::encode(&ch.bytes()[..6]));
                }
            }
            tracing::info!(
                target: "f1r3.trace.fs_floor",
                event = "force_rejected_branch",
                sigs = %sigs.join(","),
                conflicts_final,
                enforced_sig,
                depends_rej,
                nonfold_channels = %nonfold_chans.join(" "),
                succ_acc = %succ_acc.join(","),
                race_acc = %race_acc.join(","),
                succ_rej = %succ_rej.join(","),
                race_rej = %race_rej.join(","),
                "enforcement force-rejected branch"
            );
        }
    };

    // Resolve conflicts: enforce finalized decisions, then select the
    // cost-optimal rejection set among the survivors.
    let mut resolved = conflict_set_merger::resolve_conflicts(
        actual_seq,
        late_seq,
        &depends_fn,
        &rejection_cost_f,
        &mergeable_channels_fn,
        &get_data_fn,
        &compute_branches_fn,
        &compute_conflict_map_fn,
        &final_set,
        &carries_enforced_sig,
        &force_reject_logger,
    )
    .map_err(|e| CasperError::HistoryError(e))?;

    // DIAG: per-pass attribution of resolve_conflicts. Lets a run tell whether
    // the user writes were dropped here (and by which sub-pass: force-rejection /
    // cost-optimal keep-one / late-dependency) versus a later pass (serialize /
    // expansion).
    {
        let resolve_rejected_user: Vec<String> = resolved
            .rejected
            .0
            .iter()
            .flat_map(|c| c.deploys_with_cost.0.iter())
            .filter(|d| !is_system_deploy_id(&d.deploy_id))
            .map(|d| hex::encode(&d.deploy_id[..d.deploy_id.len().min(8)]))
            .collect();
        tracing::info!(
            target: "f1r3.trace.fs_floor",
            event = "resolve_breakdown",
            late = resolved.late_set_size,
            dependents = resolved.rejected_as_dependents_count,
            forced = resolved.force_rejected_final_count,
            optimal = resolved.optimal_rejection_count,
            branches = resolved.branches_count,
            to_merge = resolved.to_merge.len(),
            rejected_user = %resolve_rejected_user.join(","),
            "resolve_conflicts result (before serialize/expand)"
        );
    }

    if resolved.force_rejected_final_count > 0 {
        tracing::info!(
            target: "f1r3.trace.fs_floor",
            event = "enforce_rejected",
            forced_branches = resolved.force_rejected_final_count,
            window_accepted = final_set.accepted.len(),
            window_rejected = final_set.rejected.len(),
            "finalized decisions force-rejected conflict branches"
        );
    }

    // Single-value-cell serialization. After conflict resolution the surviving
    // chains may still contain CONCURRENT writes to one non-foldable single-value
    // cell — a fork where two chains consumed the same base datum. Conflict
    // detection misses this when both writers share a dependency on the producer
    // (they land in one branch, and the combined event log hides the double
    // consume as internal COMMs). A single-value cell cannot hold both writes;
    // keep one linear write path and reject the rest to recovery, where they
    // re-execute against the updated base. Generalizes the prior stale-consume
    // pass: a stale rebased consume is the degenerate fork whose consumed datum
    // was never available. Foldable (mergeable number) channels compose via the
    // dispatcher fold and are exempt.
    {
        let mut foldable: HashSet<Blake2b256Hash> = HashSet::new();
        for branch in resolved.to_merge.iter() {
            for chain in branch.0.iter() {
                for (ch, _) in chain.event_log_index.number_channels_data.iter() {
                    foldable.insert(ch.clone());
                }
            }
        }

        // Decide which chains to reject. Order all surviving chains so a producer
        // always precedes its consumers: deploy_chains are dependency-grouped per
        // block, so a chain consuming another's produce is in a strictly higher
        // block; ordering by block number — with (block hash, sorted sigs) as a
        // total-order tiebreak — respects producer-before-consumer without a
        // topological sort and is identical on every node.
        // `mutable_key_type`: DeployChainIndex contains a DashMap (interior
        // mutability) but is hashed by its stable content, as elsewhere in this
        // module — the keys are never mutated while in the set.
        #[allow(clippy::mutable_key_type)]
        let rejected: HashSet<DeployChainIndex> = {
            let mut ordered: Vec<&DeployChainIndex> = resolved
                .to_merge
                .iter()
                .flat_map(|branch| branch.0.iter())
                .collect();
            let sig_key = |chain: &DeployChainIndex| -> Vec<Vec<u8>> {
                let mut sigs: Vec<Vec<u8>> = chain
                    .deploys_with_cost
                    .0
                    .iter()
                    .map(|d| d.deploy_id.to_vec())
                    .collect();
                sigs.sort();
                sigs
            };
            ordered.sort_by(|a, b| {
                a.source_block_number
                    .cmp(&b.source_block_number)
                    .then_with(|| a.source_block_hash.cmp(&b.source_block_hash))
                    .then_with(|| sig_key(a).cmp(&sig_key(b)))
            });

            // Running available-datum multiset per non-foldable channel, seeded
            // lazily from the merge base.
            let mut available: HashMap<Blake2b256Hash, Vec<Vec<u8>>> = HashMap::new();
            #[allow(clippy::mutable_key_type)]
            let mut rejected: HashSet<DeployChainIndex> = HashSet::new();

            for chain in ordered.iter() {
                let chain: &DeployChainIndex = chain;
                // Serializable iff every non-foldable channel it consumes from
                // still has those datums available in the running cell state.
                let mut serializable = true;
                let mut detail: Option<(String, String)> = None;
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
                        detail = Some((
                            hex::encode(ch.bytes()),
                            removed
                                .first()
                                .map(|d| hex::encode(&d[..d.len().min(8)]))
                                .unwrap_or_default(),
                        ));
                        break;
                    }
                }
                if !serializable {
                    if let Some((ch_hex, removed_hash)) = detail {
                        let sigs: Vec<String> = chain
                            .deploys_with_cost
                            .0
                            .iter()
                            .filter(|dp| !is_system_deploy_id(&dp.deploy_id))
                            .map(|dp| hex::encode(&dp.deploy_id[..dp.deploy_id.len().min(8)]))
                            .collect();
                        tracing::info!(
                            target: "f1r3.trace.fs_floor",
                            event = "serialize_reject_chain",
                            channel = %ch_hex,
                            removed_hash = %removed_hash,
                            sigs = %sigs.join(","),
                            "single-value-cell serialization: chain's consumed datum no longer available (fork sibling or stale rebase) — rejecting to recovery"
                        );
                    }
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
            tracing::info!(
                target: "f1r3.trace.fs_floor",
                event = "serialize_rejected",
                chains = rejected.len(),
                branches_before = pre,
                branches_after = resolved.to_merge.len(),
                "single-value-cell serialization rejected fork/stale chains; one linear path kept per cell"
            );
        }
    }

    // Source blocks of the rejected chains, classified system/user for merge
    // tracing only. The former rejection-expansion pass that consumed this set
    // (rejecting any chain whose block DAG-descended from a rejected chain) was
    // removed: it dropped channel-disjoint descendants — keep-one's survivor —
    // on block-descent rather than data dependency.
    let rejected_source_blocks: HashSet<BlockHash> = resolved
        .rejected
        .0
        .iter()
        .map(|chain| chain.source_block_hash.clone())
        .collect();

    // DIAG: dump the rejected source blocks that feed the descendant walk, with
    // block number and whether each carries a system (S, e.g. CloseBlock) and/or
    // user (U) chain. Lets a run confirm whether a dropped user write descends
    // from a rejected SYSTEM chain's block (the suspected expansion over-reach)
    // rather than from a conflicting user write.
    if !rejected_source_blocks.is_empty() {
        use std::collections::BTreeMap;
        let mut by_block: BTreeMap<(i64, String), (bool, bool)> = BTreeMap::new();
        for chain in resolved.rejected.0.iter() {
            let key = (
                chain.source_block_number,
                hex::encode(&chain.source_block_hash[..chain.source_block_hash.len().min(6)]),
            );
            let entry = by_block.entry(key).or_insert((false, false));
            for d in chain.deploys_with_cost.0.iter() {
                if is_system_deploy_id(&d.deploy_id) {
                    entry.0 = true;
                } else {
                    entry.1 = true;
                }
            }
        }
        let summary: Vec<String> = by_block
            .iter()
            .map(|((num, hash), (sys, usr))| {
                format!(
                    "#{}:{}:{}{}",
                    num,
                    hash,
                    if *sys { "S" } else { "" },
                    if *usr { "U" } else { "" }
                )
            })
            .collect();
        tracing::info!(
            target: "f1r3.trace.fs_floor",
            event = "rejected_source_blocks",
            blocks = %summary.join(" "),
            "rejected chains' source blocks (S=system, U=user)"
        );
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

    tracing::debug!(
        target: "f1r3fly.merge.dag",
        new_root = %hex::encode(new_state.bytes()),
        "dag merge apply result"
    );

    // Applied (kept-chain) user-deploy (sig, source block) pairs — the deploys
    // whose effect IS in this merged state. Mirrors the rejected extraction but
    // reads `resolved.to_merge` (the survivors). The seal accumulates these into
    // FloorData.accepted_deploys so a deploy finalized-accepted at any inclusion
    // is never re-proposed, even if a later re-inclusion is rejected.
    let mut applied_user: Vec<(Bytes, BlockHash)> = resolved
        .to_merge
        .iter()
        .flat_map(|branch| branch.0.iter())
        .flat_map(|chain| {
            let src = chain.source_block_hash.clone();
            chain
                .deploys_with_cost
                .0
                .iter()
                .map(move |deploy| (deploy.deploy_id.clone(), src.clone()))
        })
        .filter(|(id, _)| !is_system_deploy_id(id))
        .collect();
    applied_user.sort();
    applied_user.dedup();

    // DIAG: kept (applied) chains' channels + system status — symmetric to
    // rejected_chain_channels but INCLUDES system chains, so a run reveals
    // whether the chain CREATING a doubled cell is a user pre-charge chain (and
    // which deployer sig owns it) or a system (CloseBlock/Slash) chain.
    for branch in resolved.to_merge.iter() {
        for chain in branch.0.iter() {
            let user_sigs: Vec<String> = chain
                .deploys_with_cost
                .0
                .iter()
                .filter(|d| !is_system_deploy_id(&d.deploy_id))
                .map(|d| hex::encode(&d.deploy_id[..d.deploy_id.len().min(8)]))
                .collect();
            let is_sys = chain
                .deploys_with_cost
                .0
                .iter()
                .any(|d| is_system_deploy_id(&d.deploy_id));
            let mut chans: Vec<String> = Vec::new();
            for e in chain.state_changes.datums_changes.iter() {
                let ch = e.key();
                let foldable = chain.event_log_index.number_channels_data.contains_key(ch);
                chans.push(format!(
                    "{}:{}r{}a{}",
                    hex::encode(&ch.bytes()[..6]),
                    if foldable { "F" } else { "N" },
                    e.value().removed.len(),
                    e.value().added.len(),
                ));
            }
            if chans.is_empty() {
                continue;
            }
            tracing::info!(
                target: "f1r3.trace.fs_floor",
                event = "kept_chain_channels",
                is_sys,
                sigs = %user_sigs.join(","),
                channels = %chans.join(" "),
                "kept chain touched channels (is_sys = system deploy)"
            );
        }
    }

    let rejected = resolved.rejected;

    // DIAG: per rejected user chain, dump the datum channels it touched (N =
    // non-foldable, F = foldable/number) with removed/added counts. Lets a
    // post-run trace attribute WHY a user deploy lost the merge — i.e. which
    // cell its chain conflicted on (the map cell vs a vault/PoS cell).
    for chain in rejected.0.iter() {
        let sigs: Vec<String> = chain
            .deploys_with_cost
            .0
            .iter()
            .filter(|d| !is_system_deploy_id(&d.deploy_id))
            .map(|d| hex::encode(&d.deploy_id[..d.deploy_id.len().min(8)]))
            .collect();
        if sigs.is_empty() {
            continue;
        }
        let mut chans: Vec<String> = Vec::new();
        for e in chain.state_changes.datums_changes.iter() {
            let ch = e.key();
            let foldable = chain.event_log_index.number_channels_data.contains_key(ch);
            chans.push(format!(
                "{}:{}r{}a{}",
                hex::encode(&ch.bytes()[..6]),
                if foldable { "F" } else { "N" },
                e.value().removed.len(),
                e.value().added.len(),
            ));
        }
        tracing::info!(
            target: "f1r3.trace.fs_floor",
            event = "rejected_chain_channels",
            sigs = %sigs.join(","),
            channels = %chans.join(" "),
            "rejected user chain touched channels"
        );
    }

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
            target: "f1r3.trace.fs_floor",
            event = "merge_rejected_user",
            count = rejected_user_deploys.len(),
            sigs = %rejected_str.join(","),
            "DagMerger rejected user deploys (span distinguishes construction vs seal merge)"
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

    Ok((
        new_state,
        applied_user,
        rejected_user_deploys,
        rejected_slashes,
    ))
}
