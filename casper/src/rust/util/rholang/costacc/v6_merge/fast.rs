//! DR-120 (gap G9): the fast path of the v6 rule.
//!
//! When the predicate P holds, no chain of the scope can lose, so the merge
//! composes every chain and runs no rejection logic (P1:356-359: results of
//! different signatures are "composed without merge analysis"). P is the
//! conjunction of checks F1 to F13 below. Each check reads only consensus
//! inputs and the base state, so every validator takes the same path. When a
//! check fails, `try_compose` returns `None` and the merge takes the slow
//! path: dev's rows 9 to 11d, the ordered pass, then dev's rows 11g to 15.
//! Under P the slow path returns the same state, no records, the same applied
//! set and the same base (law L5), so the two paths agree.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use prost::bytes::Bytes;
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rholang::rust::interpreter::rho_runtime::RhoHistoryRepository;
use rspace_plus_plus::rspace::errors::HistoryError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
use rspace_plus_plus::rspace::merger::merging_logic;
use shared::rust::hashable_set::HashableSet;

use super::claims::{self, FoldedClaims};
use super::ledger::{self, PurseLedger};
use super::{chain_depends, compose, evidence, walk_branch, RhoHistoryReader};
use crate::rust::errors::CasperError;
use crate::rust::merging::conflict_set_merger::{compare_branches, Branch, ResolvedConflicts};
use crate::rust::merging::dag_merger;
use crate::rust::merging::deploy_chain_index::DeployChainIndex;
use crate::rust::system_deploy::is_system_deploy_id;
use crate::rust::util::rholang::runtime_manager::MergedPreState;

/// The inputs of the fast path: the same consensus inputs that dev's merge
/// receives, plus the anchor's floor test.
pub(crate) struct FastPathInputs<'a> {
    pub dag: &'a KeyValueDagRepresentation,
    pub block_store: &'a KeyValueBlockStore,
    pub history_repository: &'a RhoHistoryRepository,
    /// The merge base (the scope anchor).
    pub base: &'a BlockHash,
    pub base_post_state: &'a Blake2b256Hash,
    /// True when the main parent's state holds the floor, so the base is the
    /// main parent.
    pub base_holds_floor: bool,
    pub scope: &'a HashSet<BlockHash>,
    pub base_lineage_blocks: &'a HashSet<BlockHash>,
    pub floor_block_number: i64,
    pub deploy_lifespan: i64,
    pub index: &'a dyn Fn(&BlockHash) -> Result<Vec<DeployChainIndex>, CasperError>,
    pub prior_rejection_counts: &'a HashMap<Bytes, u64>,
}

/// A composed merge: the merged state and the user deploys it applied.
#[derive(Debug)]
pub(crate) struct FastPathOutcome {
    pub state: Blake2b256Hash,
    pub applied_user_sigs: HashSet<Bytes>,
}

impl FastPathOutcome {
    /// The merged pre-state of the composed scope: no rejection records, every
    /// user deploy applied from scope, and the anchor as the merge base.
    pub(crate) fn into_merged_pre_state(self, merge_base: BlockHash) -> MergedPreState {
        MergedPreState {
            state: Bytes::copy_from_slice(&self.state.bytes()),
            rejected_user: Vec::new(),
            rejected_slashes: Vec::new(),
            applied_from_scope: self.applied_user_sigs,
            merge_base: Some(merge_base),
        }
    }
}

/// Copy of the branch glue of dag_merger.rs:1739-1753: chains that depend on
/// each other, directly or through other chains, form one branch.
pub(crate) fn compute_branches(chains: &[DeployChainIndex]) -> Vec<Branch<DeployChainIndex>> {
    let event_logs: Vec<&EventLogIndex> = chains.iter().map(|c| &c.event_log_index).collect();
    let depends_map = merging_logic::compute_depends_map_event_indexed(chains, &event_logs);
    let mut branches: Vec<Branch<DeployChainIndex>> =
        merging_logic::gather_related_sets(&depends_map)
            .0
            .into_iter()
            .map(Arc::new)
            .collect();
    branches.sort_by(|a, b| compare_branches(a, b));
    branches
}

/// Copy of the base-lineage log fold of dag_merger.rs:1332-1345: the base's
/// own content since the parents diverged, as one event log.
pub(crate) fn base_lineage_event_log(
    base_lineage_blocks: &HashSet<BlockHash>,
    index: &dyn Fn(&BlockHash) -> Result<Vec<DeployChainIndex>, CasperError>,
) -> Result<EventLogIndex, CasperError> {
    let mut base_event_log = EventLogIndex::empty();
    let mut base_lineage_sorted: Vec<&BlockHash> = base_lineage_blocks.iter().collect();
    base_lineage_sorted.sort();
    for block_hash in base_lineage_sorted {
        for chain in index(block_hash)? {
            base_event_log = EventLogIndex::combine(&base_event_log, &chain.event_log_index)
                .map_err(CasperError::HistoryError)?;
        }
    }
    Ok(base_event_log)
}

/// LOW-5 (P5'): a conflict between two chains, with chains as units. A race
/// or a potential COMM between the combined logs of two sets of chains is
/// one between two of their chains. A produce that touches a base join
/// conflicts with every log. So an empty chain-level map rules out every
/// event-log conflict between two branches. With F7, it also rules out every
/// conflict with the combined log of the settled chains
/// (dag_merger.rs:1225-1281). F7 is needed when no chain is settled: the
/// combined log is then empty, and a chain conflicts with it only through
/// its own touching produce, which also conflicts with the base log. The
/// map has no same-user-deploy-id pass (dag_merger.rs:1732-1760), so F5
/// checks repeated deploy ids.
pub(crate) fn has_chain_level_conflict(chains: &[DeployChainIndex]) -> bool {
    let units: Vec<usize> = (0..chains.len()).collect();
    let event_logs: Vec<&EventLogIndex> = chains.iter().map(|c| &c.event_log_index).collect();
    merging_logic::compute_conflict_map_event_indexed(&units, &event_logs)
        .values()
        .any(|others| !others.0.is_empty())
}

/// The base value of a number channel, as dev's resolver reads it
/// (conflict_set_merger.rs:228-239): an absent channel reads as zero.
pub(crate) fn base_number(
    reader: &RhoHistoryReader,
    channel: &Blake2b256Hash,
) -> Result<i64, HistoryError> {
    let read_number =
        RholangMergingLogic::convert_to_read_number(|hash: &Blake2b256Hash| reader.get_data(hash));
    Ok(read_number(channel)?.unwrap_or(0))
}

/// The fast path. `Ok(None)` sends the merge to the slow path. An error is
/// one that dev's merge raises on the same inputs too.
pub(crate) fn try_compose(i: &FastPathInputs<'_>) -> Result<Option<FastPathOutcome>, CasperError> {
    // F1 (P1): the base is the main parent.
    if !i.base_holds_floor {
        return Ok(None);
    }
    // F2: the scope blocks that are not on the base's main chain, sorted
    // (dag_merger.rs:889-897,951-952).
    let mut actual_blocks: Vec<BlockHash> = Vec::with_capacity(i.scope.len());
    for candidate in i.scope {
        if !i.dag.is_in_main_chain(candidate, i.base)? {
            actual_blocks.push(candidate.clone());
        }
    }
    actual_blocks.sort();
    // F3 (P2): every user deploy carries a fee-cursor transition.
    for block_hash in &actual_blocks {
        let block = i.block_store.get(block_hash)?.ok_or_else(|| {
            CasperError::RuntimeError(format!(
                "v6 merge scope block {} not in block store (DAG/store desync)",
                hex::encode(&block_hash[..8.min(block_hash.len())])
            ))
        })?;
        if !evidence::every_deploy_has_fee_transition(&block)? {
            return Ok(None);
        }
    }
    // F4: the raw chains, stamped with their prior losses like the slow path
    // stamps them (dag_merger.rs:965), so both paths walk in one order.
    let mut chains: Vec<DeployChainIndex> = Vec::new();
    for block_hash in &actual_blocks {
        chains.extend((i.index)(block_hash)?);
    }
    dag_merger::stamp_prior_rejections(&mut chains, i.prior_rejection_counts);
    // F5 (C2): no user deploy id repeats in the raw list.
    if claims::has_repeated_user_id(&chains) {
        return Ok(None);
    }
    // F6 (P3): no chain's validity window is closed at the floor.
    let (chains, window_closed) =
        dag_merger::split_window_closed_chains(chains, i.floor_block_number, i.deploy_lifespan);
    if !window_closed.is_empty() {
        return Ok(None);
    }
    // F7 (P4): no chain conflicts with the base's own content.
    let base_event_log = base_lineage_event_log(i.base_lineage_blocks, i.index)?;
    let (chains, base_conflicting) = dag_merger::partition_base_conflicts(chains, &base_event_log);
    if !base_conflicting.is_empty() {
        return Ok(None);
    }
    // F8: the branches.
    let branches = compute_branches(&chains);
    let branches_count = branches.len();
    // F9 (C4): every branch passes the pre-check, so no fold below errors.
    if !branches.iter().all(|branch| ledger::branch_valid(branch)) {
        return Ok(None);
    }
    // F10 (P5'): no two chains conflict.
    if has_chain_level_conflict(&chains) {
        return Ok(None);
    }
    // F11 (P6): dev's availability walk rejects nothing.
    let history_reader = i
        .history_repository
        .get_history_reader(i.base_post_state)
        .map_err(CasperError::HistoryError)?;
    for branch in &branches {
        let (_, unavailable) = walk_branch((**branch).clone(), &chain_depends, &history_reader)
            .map_err(CasperError::HistoryError)?;
        if !unavailable.0.is_empty() {
            return Ok(None);
        }
    }
    // F12 (P7): no branch mixes folded and plain changes with another, and
    // the dry run of compose's guard flags no channel.
    let mut folded_claims = FoldedClaims::default();
    for branch in &branches {
        if folded_claims.mixes(branch) {
            return Ok(None);
        }
        folded_claims.add(branch);
    }
    let kept: Vec<&HashableSet<DeployChainIndex>> = branches.iter().map(|b| &**b).collect();
    if compose::first_overfill(&kept, &history_reader)
        .map_err(CasperError::HistoryError)?
        .is_some()
    {
        return Ok(None);
    }
    // F13 (P8): the full set's sign splits fit, one merge type per channel,
    // and every contributor channel ends in [0, i64::MAX].
    let mut purse_ledger = PurseLedger::default();
    if !branches.iter().all(|branch| purse_ledger.try_add(branch)) {
        return Ok(None);
    }
    if purse_ledger
        .first_failure(&mut |channel| base_number(&history_reader, channel))
        .map_err(CasperError::HistoryError)?
        .is_some()
    {
        return Ok(None);
    }
    // F14: compose every chain.
    let applied_user_sigs: HashSet<Bytes> = chains
        .iter()
        .flat_map(|chain| chain.deploys_with_cost.0.iter())
        .filter(|deploy| !is_system_deploy_id(&deploy.deploy_id))
        .map(|deploy| deploy.deploy_id.clone())
        .collect();
    let resolved = ResolvedConflicts {
        to_merge: branches
            .into_iter()
            .map(|branch| Arc::try_unwrap(branch).unwrap_or_else(|shared| (*shared).clone()))
            .collect(),
        rejected: HashableSet(HashSet::new()),
        late_set_size: 0,
        actual_set_size: chains.len(),
        branches_count,
        rejected_as_dependents_count: 0,
        optimal_rejection_count: 0,
        conflict_map_conflicts_count: 0,
        rejection_options_count: 0,
        branches_time: std::time::Duration::ZERO,
        conflicts_map_time: std::time::Duration::ZERO,
        rejection_options_time: std::time::Duration::ZERO,
    };
    let state = compose::compose(i.history_repository, i.base_post_state, &resolved)?;
    tracing::debug!(
        target: "f1r3fly.merge.cpps",
        step = "v6_merge.FAST_PATH",
        base = %hex::encode(&i.base[..8.min(i.base.len())]),
        n_chains = chains.len(),
        n_applied = applied_user_sigs.len(),
        "merge.cpps: v6 scope composed without merge analysis"
    );
    Ok(Some(FastPathOutcome {
        state,
        applied_user_sigs,
    }))
}
