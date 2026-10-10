//! DR-120 (gap G9): the ordered pass of the v6 rule.
//!
//! Dev resolves conflicts by an exhaustive search over rejection options
//! (conflict_set_merger.rs:103-377,754-1300). P1 gives the residual of shared
//! data channels "a natural serialization order" (P1:363-366). The ordered
//! pass replaces the search at its single call site (dag_merger.rs:1818):
//!
//! - S1 rejects the late chains and the chains that depend on them
//!   (conflict_set_merger.rs:153-166).
//! - S2 groups the other chains into branches.
//! - S3 rejects each branch that fails the pre-check, and runs dev's
//!   availability walk on each other branch. A walk's survivors form one
//!   candidate.
//! - S4 builds dev's conflict map over the candidates.
//! - S5 walks the candidates in the order K. It keeps a candidate only when
//!   the candidate conflicts with no kept candidate, mixes no folded and
//!   plain change with them, and keeps every sign split in range. The three
//!   checks are monotone, so a later removal cannot make them fail.
//! - S6 repairs until nothing changes: lineage closure, the conflict
//!   re-check after a candidate shrinks, the overfill dry run, and the final
//!   balances. Each repair drops one whole candidate, so the loop ends.
//! - S7 returns the kept candidates and the rejected chains. Dev's later
//!   lineage step then finds nothing, because S6 closed lineage over the
//!   same rejected set.
//!
//! Each rejected chain keeps the witness of the step that rejected it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use models::rust::block_hash::BlockHash;
use rspace_plus_plus::rspace::errors::HistoryError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use shared::rust::hashable_set::HashableSet;

use super::claims::FoldedClaims;
use super::fast::base_number;
use super::ledger::{self, Failure, PurseLedger};
use super::order::{last_by_k, sort_by_k};
use super::{compose, walk_branch, RhoHistoryReader};
use crate::rust::errors::CasperError;
use crate::rust::merging::conflict_set_merger::{Branch, ResolvedConflicts};
use crate::rust::merging::deploy_chain_index::DeployChainIndex;

/// The conflict map of a set of branches, as dev's merge builds it.
pub(crate) type ConflictMapFn<'a> = dyn Fn(
        &HashableSet<Branch<DeployChainIndex>>,
    ) -> Result<
        HashMap<Branch<DeployChainIndex>, HashableSet<Branch<DeployChainIndex>>>,
        HistoryError,
    > + 'a;

/// The inputs of the ordered pass. The relations are dev's closures, so the
/// pass changes how conflicts are resolved and not what counts as one.
pub(crate) struct OrderedPassInputs<'a> {
    pub dag: &'a KeyValueDagRepresentation,
    pub actual: &'a [DeployChainIndex],
    pub late: &'a [DeployChainIndex],
    /// The settled chains (dag_merger.rs:1390).
    pub pinned: &'a HashSet<DeployChainIndex>,
    /// Scope chains that the base's own content precluded.
    pub base_conflicting: &'a [DeployChainIndex],
    /// Scope chains that settled content precluded.
    pub settled_conflicting: &'a [DeployChainIndex],
    pub depends: &'a dyn Fn(&DeployChainIndex, &DeployChainIndex) -> bool,
    pub compute_branches:
        &'a dyn Fn(&HashableSet<DeployChainIndex>) -> HashableSet<Branch<DeployChainIndex>>,
    pub compute_conflict_map: &'a ConflictMapFn<'a>,
    pub history_reader: &'a RhoHistoryReader,
}

/// Why the ordered pass rejected a chain.
#[derive(Clone, Debug)]
pub(crate) enum Witness {
    /// S1: the chain's validity window is closed at the floor.
    Late,
    /// S1: the chain depends on a late chain.
    DependsOnLate,
    /// S3: the chain's branch fails the pre-check.
    InvalidBranch,
    /// S3: dev's availability walk rejected the chain.
    Unavailable,
    /// S5: the chain's candidate conflicts with this kept candidate.
    Conflict(Branch<DeployChainIndex>),
    /// S5: the chain's candidate mixes folded and plain changes with the kept
    /// set.
    Mixing,
    /// S5: the chain's candidate breaks a sign split or a merge type.
    Ledger,
    /// S6.1: the chain's source block descends from this rejected block.
    StaleLineage(BlockHash),
    /// S6.2: after a shrink, the chain's candidate conflicts with this
    /// earlier kept candidate.
    LineageConflict(Branch<DeployChainIndex>),
    /// S6.3: the chain's candidate was the last adder by K on this
    /// overfilled channel.
    Overfill(Blake2b256Hash),
    /// S6.4: the chain's candidate was the last contributor by K that this
    /// final balance needed to drop.
    Balance(Blake2b256Hash, Failure),
}

fn short_hex(bytes: &[u8]) -> String { hex::encode(&bytes[..8.min(bytes.len())]) }

fn describe_candidate(candidate: &Branch<DeployChainIndex>) -> String {
    let mut ids: Vec<String> = candidate
        .0
        .iter()
        .flat_map(|chain| chain.deploys_with_cost.0.iter())
        .map(|deploy| short_hex(&deploy.deploy_id))
        .collect();
    ids.sort();
    ids.join(",")
}

impl Witness {
    /// The witness as one log field.
    pub(crate) fn describe(&self) -> String {
        match self {
            Witness::Late => "late".to_string(),
            Witness::DependsOnLate => "depends on a late chain".to_string(),
            Witness::InvalidBranch => "branch fails the pre-check".to_string(),
            Witness::Unavailable => "removal unavailable at the base".to_string(),
            Witness::Conflict(kept) => {
                format!("conflicts with kept [{}]", describe_candidate(kept))
            }
            Witness::Mixing => "mixes folded and plain changes".to_string(),
            Witness::Ledger => "breaks a sign split or a merge type".to_string(),
            Witness::StaleLineage(ancestor) => {
                format!("descends from rejected block {}", short_hex(ancestor))
            }
            Witness::LineageConflict(earlier) => {
                format!(
                    "conflicts after a shrink with [{}]",
                    describe_candidate(earlier)
                )
            }
            Witness::Overfill(channel) => {
                format!("last adder on overfilled {}", short_hex(&channel.bytes()))
            }
            Witness::Balance(channel, failure) => {
                format!(
                    "dropped for the {failure:?} balance of {}",
                    short_hex(&channel.bytes())
                )
            }
        }
    }
}

/// The rejected chains and their witnesses.
pub(crate) type Rejections = HashMap<DeployChainIndex, Witness>;

fn unwrap_branch(branch: Branch<DeployChainIndex>) -> HashableSet<DeployChainIndex> {
    Arc::try_unwrap(branch).unwrap_or_else(|shared| (*shared).clone())
}

fn reject_chains<'c>(
    rejected: &mut Rejections,
    chains: impl IntoIterator<Item = &'c DeployChainIndex>,
    witness: &Witness,
) {
    for chain in chains {
        rejected
            .entry(chain.clone())
            .or_insert_with(|| witness.clone());
    }
}

/// The ordered pass. Returns the resolved conflicts and the number of chains
/// that the availability walk rejected.
pub(crate) fn ordered_pass(
    i: &OrderedPassInputs<'_>,
) -> Result<(ResolvedConflicts<DeployChainIndex>, usize), CasperError> {
    let (resolved, unavailable_count, witnesses) = ordered_pass_witnessed(i)?;
    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        for (chain, witness) in &witnesses {
            let sigs: Vec<String> = chain
                .deploys_with_cost
                .0
                .iter()
                .map(|deploy| short_hex(&deploy.deploy_id))
                .collect();
            tracing::debug!(
                target: "f1r3fly.merge.step",
                step = "v6_merge.REJECTED",
                src = %short_hex(&chain.source_block_hash),
                sigs = ?sigs,
                witness = %witness.describe(),
                "ordered pass rejected a chain"
            );
        }
    }
    Ok((resolved, unavailable_count))
}

/// The ordered pass with the witness of every rejected chain.
pub(crate) fn ordered_pass_witnessed(
    i: &OrderedPassInputs<'_>,
) -> Result<(ResolvedConflicts<DeployChainIndex>, usize, Rejections), CasperError> {
    // S1: the late chains and their dependents.
    let late: HashSet<DeployChainIndex> = i.late.iter().cloned().collect();
    let mut rejected: Rejections = HashMap::with_capacity(i.actual.len() + late.len());
    reject_chains(&mut rejected, &late, &Witness::Late);
    let mut merge_set: HashSet<DeployChainIndex> = HashSet::with_capacity(i.actual.len());
    let mut rejected_as_dependents = 0usize;
    for chain in i.actual {
        match late.iter().any(|late_chain| (i.depends)(chain, late_chain)) {
            true => {
                rejected_as_dependents += 1;
                reject_chains(&mut rejected, [chain], &Witness::DependsOnLate);
            }
            false => {
                merge_set.insert(chain.clone());
            }
        }
    }
    // S2: the branches.
    let branches: Vec<Branch<DeployChainIndex>> = (i.compute_branches)(&HashableSet(merge_set))
        .0
        .into_iter()
        .collect();
    let branches_count = branches.len();
    // S3: the pre-check and the availability walk, per branch.
    let mut candidates: Vec<Branch<DeployChainIndex>> = Vec::with_capacity(branches_count);
    let mut unavailable_count = 0usize;
    for branch in branches {
        if !ledger::branch_valid(&branch) {
            reject_chains(&mut rejected, branch.0.iter(), &Witness::InvalidBranch);
            continue;
        }
        let (survivors, unavailable) =
            walk_branch(unwrap_branch(branch), i.depends, i.history_reader)
                .map_err(CasperError::HistoryError)?;
        unavailable_count += unavailable.0.len();
        reject_chains(&mut rejected, unavailable.0.iter(), &Witness::Unavailable);
        if let Some(survivors) = survivors {
            candidates.push(Arc::new(survivors));
        }
    }
    sort_by_k(&mut candidates, i.pinned);
    // S4: dev's conflict map over the candidates.
    let conflict_map = (i.compute_conflict_map)(&HashableSet(candidates.iter().cloned().collect()))
        .map_err(CasperError::HistoryError)?;
    let conflict_map_conflicts_count = conflict_map
        .values()
        .filter(|others| !others.0.is_empty())
        .count();
    // S5: one walk in K order with the monotone checks.
    let mut kept: Vec<Branch<DeployChainIndex>> = Vec::with_capacity(candidates.len());
    let mut folded_claims = FoldedClaims::default();
    let mut purse_ledger = PurseLedger::default();
    for candidate in candidates {
        let conflicting = conflict_map.get(&candidate).and_then(|others| {
            kept.iter()
                .find(|kept_candidate| others.0.contains(*kept_candidate))
                .cloned()
        });
        let witness = match conflicting {
            Some(kept_candidate) => Some(Witness::Conflict(kept_candidate)),
            None if folded_claims.mixes(&candidate) => Some(Witness::Mixing),
            None if !purse_ledger.try_add(&candidate) => Some(Witness::Ledger),
            None => None,
        };
        match witness {
            Some(witness) => reject_chains(&mut rejected, candidate.0.iter(), &witness),
            None => {
                folded_claims.add(&candidate);
                kept.push(candidate);
            }
        }
    }
    // S6: the repair loop for the checks that are not monotone.
    repair(i, &mut kept, &mut rejected)?;
    tracing::debug!(
        target: "f1r3fly.merge.step",
        step = "v6_merge.ORDERED_PASS",
        n_actual = i.actual.len(),
        n_late = late.len(),
        n_branches = branches_count,
        n_unavailable = unavailable_count,
        n_kept = kept.len(),
        n_rejected = rejected.len(),
        "ordered pass resolved the v6 merge"
    );
    // S7: the result.
    let rejected_chains = HashableSet(rejected.keys().cloned().collect());
    Ok((
        ResolvedConflicts {
            to_merge: kept.into_iter().map(unwrap_branch).collect(),
            rejected: rejected_chains,
            late_set_size: late.len(),
            actual_set_size: i.actual.len(),
            branches_count,
            rejected_as_dependents_count: rejected_as_dependents,
            optimal_rejection_count: 0,
            conflict_map_conflicts_count,
            rejection_options_count: 0,
            branches_time: std::time::Duration::ZERO,
            conflicts_map_time: std::time::Duration::ZERO,
            rejection_options_time: std::time::Duration::ZERO,
        },
        unavailable_count,
        rejected,
    ))
}

fn diff_on(chain: &DeployChainIndex, channel: &Blake2b256Hash) -> Option<i64> {
    chain
        .event_log_index
        .number_channels_data
        .get(channel)
        .map(|(diff, _)| *diff)
}

fn adds_on(chain: &DeployChainIndex, channel: &Blake2b256Hash) -> bool {
    chain
        .state_changes
        .datums_changes
        .get(channel)
        .is_some_and(|change| !change.added.is_empty())
}

fn reject_candidate(
    kept: &mut Vec<Branch<DeployChainIndex>>,
    rejected: &mut Rejections,
    index: usize,
    witness: Witness,
) {
    let candidate = kept.remove(index);
    reject_chains(rejected, candidate.0.iter(), &witness);
}

fn repair_error(what: &str, channel: &Blake2b256Hash) -> CasperError {
    CasperError::RuntimeError(format!(
        "v6 repair found no {what} on channel {}",
        hex::encode(channel.bytes())
    ))
}

/// S6: repeats lineage closure, the conflict re-check, the overfill dry run
/// and the final-balance check until none of them changes the kept set.
fn repair(
    i: &OrderedPassInputs<'_>,
    kept: &mut Vec<Branch<DeployChainIndex>>,
    rejected: &mut Rejections,
) -> Result<(), CasperError> {
    let mut base_values: BTreeMap<Blake2b256Hash, i64> = BTreeMap::new();
    let mut recheck_conflicts = false;
    loop {
        if close_lineage(i, kept, rejected)? {
            recheck_conflicts = true;
        }
        sort_by_k(kept, i.pinned);
        if recheck_conflicts {
            match first_conflict_victim(i, kept)? {
                Some((victim, earlier)) => {
                    reject_candidate(kept, rejected, victim, Witness::LineageConflict(earlier));
                    continue;
                }
                None => recheck_conflicts = false,
            }
        }
        let kept_sets: Vec<&HashableSet<DeployChainIndex>> =
            kept.iter().map(|candidate| &**candidate).collect();
        let overfill = compose::first_overfill(&kept_sets, i.history_reader)
            .map_err(CasperError::HistoryError)?;
        if let Some(channel) = overfill {
            let victim = last_by_k(kept, i.pinned, |chain| adds_on(chain, &channel))
                .ok_or_else(|| repair_error("adder", &channel))?;
            reject_candidate(kept, rejected, victim, Witness::Overfill(channel));
            continue;
        }
        let failure = PurseLedger::from_branches(kept_sets.iter().copied())
            .first_failure(&mut |channel| match base_values.get(channel) {
                Some(value) => Ok(*value),
                None => {
                    let value = base_number(i.history_reader, channel)?;
                    base_values.insert(channel.clone(), value);
                    Ok(value)
                }
            })
            .map_err(CasperError::HistoryError)?;
        let Some((channel, failure)) = failure else {
            return Ok(());
        };
        let victim = match failure {
            Failure::Negative => last_by_k(kept, i.pinned, |chain| {
                diff_on(chain, &channel).is_some_and(|diff| diff < 0)
            })
            .or_else(|| last_by_k(kept, i.pinned, |chain| diff_on(chain, &channel).is_some())),
            Failure::Overflow => last_by_k(kept, i.pinned, |chain| {
                diff_on(chain, &channel).is_some_and(|diff| diff > 0)
            }),
        }
        .ok_or_else(|| repair_error("contributor", &channel))?;
        reject_candidate(kept, rejected, victim, Witness::Balance(channel, failure));
    }
}

/// Dev's ancestry test of dag_merger.rs:1863-1878: the first rejected block,
/// other than `block_hash` itself, that is a DAG ancestor of `block_hash`.
fn rejected_ancestor(
    dag: &KeyValueDagRepresentation,
    rejected_blocks: &HashSet<BlockHash>,
    block_hash: &BlockHash,
    ancestors: &mut HashMap<BlockHash, Option<BlockHash>>,
) -> Result<Option<BlockHash>, CasperError> {
    if let Some(cached) = ancestors.get(block_hash) {
        return Ok(cached.clone());
    }
    let mut result = None;
    for rejected_block in rejected_blocks {
        if rejected_block != block_hash && dag.is_dag_ancestor(rejected_block, block_hash)? {
            result = Some(rejected_block.clone());
            break;
        }
    }
    ancestors.insert(block_hash.clone(), result.clone());
    Ok(result)
}

/// S6.1: dev's stale-diff lineage rule (dag_merger.rs:1855-1898), per chain.
/// A kept chain whose source block descends from the source block of a
/// rejected chain, of `base_conflicting` or of `settled_conflicting` is
/// rejected, unless it is settled. Returns true when it removed a chain.
fn close_lineage(
    i: &OrderedPassInputs<'_>,
    kept: &mut Vec<Branch<DeployChainIndex>>,
    rejected: &mut Rejections,
) -> Result<bool, CasperError> {
    let rejected_blocks: HashSet<BlockHash> = rejected
        .keys()
        .chain(i.base_conflicting)
        .chain(i.settled_conflicting)
        .map(|chain| chain.source_block_hash.clone())
        .collect();
    if rejected_blocks.is_empty() {
        return Ok(false);
    }
    let mut ancestors: HashMap<BlockHash, Option<BlockHash>> = HashMap::new();
    let mut removed_any = false;
    let mut next: Vec<Branch<DeployChainIndex>> = Vec::with_capacity(kept.len());
    for candidate in kept.drain(..) {
        let mut remaining: HashSet<DeployChainIndex> = HashSet::with_capacity(candidate.0.len());
        let mut shrunk = false;
        for chain in candidate.0.iter() {
            let stale = match i.pinned.contains(chain) {
                true => None,
                false => rejected_ancestor(
                    i.dag,
                    &rejected_blocks,
                    &chain.source_block_hash,
                    &mut ancestors,
                )?,
            };
            match stale {
                Some(ancestor) => {
                    reject_chains(rejected, [chain], &Witness::StaleLineage(ancestor));
                    shrunk = true;
                }
                None => {
                    remaining.insert(chain.clone());
                }
            }
        }
        match (shrunk, remaining.is_empty()) {
            (false, _) => next.push(candidate),
            (true, true) => removed_any = true,
            (true, false) => {
                removed_any = true;
                next.push(Arc::new(HashableSet(remaining)));
            }
        }
    }
    *kept = next;
    Ok(removed_any)
}

/// S6.2 (MEDIUM-1): removing a chain from a candidate can grow the
/// candidate's created-and-unconsumed sets (merging_logic.rs:526-575) and so
/// create a conflict with another kept candidate. Dev's conflict map over the
/// kept set finds it. The first edge in K order loses its later endpoint, and
/// the result names the earlier one. K puts every pinned candidate first, so
/// the later endpoint is pinned only when both are.
fn first_conflict_victim(
    i: &OrderedPassInputs<'_>,
    kept: &[Branch<DeployChainIndex>],
) -> Result<Option<(usize, Branch<DeployChainIndex>)>, CasperError> {
    let conflict_map = (i.compute_conflict_map)(&HashableSet(kept.iter().cloned().collect()))
        .map_err(CasperError::HistoryError)?;
    for (earlier, candidate) in kept.iter().enumerate() {
        let Some(others) = conflict_map.get(candidate) else {
            continue;
        };
        if let Some(offset) = kept[earlier + 1..]
            .iter()
            .position(|other| others.0.contains(other))
        {
            return Ok(Some((earlier + 1 + offset, candidate.clone())));
        }
    }
    Ok(None)
}
