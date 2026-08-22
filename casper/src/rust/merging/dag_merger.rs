// See casper/src/main/scala/coop/rchain/casper/merging/DagMerger.scala

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use models::rhoapi::ListParWithRandom;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::RejectedDeploy;
use prost::bytes::Bytes;
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rholang::rust::interpreter::rho_runtime::RhoHistoryRepository;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::merger::channel_change::ChannelChange;
use rspace_plus_plus::rspace::merger::merging_logic::{self, NumberChannelsDiff};
use rspace_plus_plus::rspace::merger::state_change::StateChange;
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

fn remove_from_available(available: &mut Vec<Vec<u8>>, removed: &[Vec<u8>]) -> bool {
    for item in removed {
        let Some(pos) = available.iter().position(|existing| existing == item) else {
            return false;
        };
        available.remove(pos);
    }
    true
}

fn binary_data_is_single_number(data: &[Vec<u8>]) -> bool {
    if data.len() != 1 {
        return false;
    }
    RholangMergingLogic::encoded_datum_is_number(&data[0])
}

fn numeric_cell_would_overfill(base: &[Vec<u8>], current: &[Vec<u8>], added: &[Vec<u8>]) -> bool {
    if added.is_empty() {
        return false;
    }
    if binary_data_is_single_number(base) {
        return current.len() + added.len() > 1;
    }
    // A base-empty channel only reads as a single-value cell when MULTIPLE
    // writers accumulate into it. A sole chain leaving several numeric datums
    // on a channel it created is intra-chain state that already executed
    // sequentially in PLAY — not a produce-only conflict.
    base.is_empty()
        && !current.is_empty()
        && current.len() + added.len() > 1
        && current
            .iter()
            .chain(added.iter())
            .all(|bytes| RholangMergingLogic::encoded_datum_is_number(bytes))
}

fn dependency_ordered_branch_items<'a>(
    branch: &'a HashableSet<DeployChainIndex>,
    depends: &impl Fn(&DeployChainIndex, &DeployChainIndex) -> bool,
) -> Vec<&'a DeployChainIndex> {
    let mut pending: Vec<&DeployChainIndex> = branch.0.iter().collect();
    let mut ordered = Vec::with_capacity(pending.len());

    // Earlier position claims base availability first (the walk in
    // `split_unavailable_branch_consumes` is first-come-first-served), so
    // prior on-DAG losses outrank the content ordering here for the same
    // reason as in the other adjudication sites (issue #294): the chain a
    // deterministic tie keeps rejecting must gain ground with every loss.
    let priority_cmp = |a: &DeployChainIndex, b: &DeployChainIndex| {
        b.prior_rejections
            .cmp(&a.prior_rejections)
            .then_with(|| a.cmp(b))
    };

    while !pending.is_empty() {
        let selected_idx = (0..pending.len())
            .filter(|candidate_idx| {
                !(0..pending.len()).any(|source_idx| {
                    source_idx != *candidate_idx
                        && depends(pending[*candidate_idx], pending[source_idx])
                })
            })
            .min_by(|left_idx, right_idx| priority_cmp(pending[*left_idx], pending[*right_idx]))
            .unwrap_or_else(|| {
                (0..pending.len())
                    .min_by(|left_idx, right_idx| {
                        priority_cmp(pending[*left_idx], pending[*right_idx])
                    })
                    .expect("pending is non-empty")
            });
        ordered.push(pending.remove(selected_idx));
    }

    ordered
}

fn branch_mergeable_channels(
    branch_items: &[&DeployChainIndex],
    mergeable_channels: &impl Fn(&DeployChainIndex) -> NumberChannelsDiff,
) -> Result<NumberChannelsDiff, rspace_plus_plus::rspace::errors::HistoryError> {
    let mut branch_mergeable = NumberChannelsDiff::new();
    for chain in branch_items {
        for (key, value) in mergeable_channels(chain).iter() {
            let (incoming_diff, incoming_mt) = *value;
            match branch_mergeable.get_mut(key) {
                Some(existing) => {
                    if existing.1 != incoming_mt {
                        return Err(rspace_plus_plus::rspace::errors::HistoryError::MergeError(
                            format!(
                                "MergeType mismatch on channel {:?}: {:?} vs {:?}",
                                key, existing.1, incoming_mt
                            ),
                        ));
                    }
                    existing.0 = match merging_logic::combine_mergeable_value(
                        existing.0,
                        incoming_diff,
                        incoming_mt,
                    ) {
                        Some(v) => v,
                        None => {
                            return Err(rspace_plus_plus::rspace::errors::HistoryError::MergeError(
                                format!(
                                    "IntegerAdd overflow combining mergeable channel {:?}",
                                    key,
                                ),
                            ))
                        }
                    };
                }
                None => {
                    branch_mergeable.insert(key.clone(), (incoming_diff, incoming_mt));
                }
            }
        }
    }
    Ok(branch_mergeable)
}

fn split_unavailable_branch_consumes(
    branch: HashableSet<DeployChainIndex>,
    depends: &impl Fn(&DeployChainIndex, &DeployChainIndex) -> bool,
    state_changes: &impl Fn(
        &DeployChainIndex,
    ) -> Result<
        rspace_plus_plus::rspace::merger::state_change::StateChange,
        rspace_plus_plus::rspace::errors::HistoryError,
    >,
    mergeable_channels: &impl Fn(&DeployChainIndex) -> NumberChannelsDiff,
    base_data: &impl Fn(
        &Blake2b256Hash,
    ) -> Result<Vec<Vec<u8>>, rspace_plus_plus::rspace::errors::HistoryError>,
    base_continuations: &impl Fn(
        &Vec<Blake2b256Hash>,
    )
        -> Result<Vec<Vec<u8>>, rspace_plus_plus::rspace::errors::HistoryError>,
) -> Result<
    (
        Option<HashableSet<DeployChainIndex>>,
        HashableSet<DeployChainIndex>,
    ),
    rspace_plus_plus::rspace::errors::HistoryError,
> {
    let branch_items = dependency_ordered_branch_items(&branch, depends);
    let branch_mergeable = branch_mergeable_channels(&branch_items, mergeable_channels)?;
    let mut available_data: HashMap<Blake2b256Hash, Vec<Vec<u8>>> = HashMap::new();
    let mut available_continuations: HashMap<Vec<Blake2b256Hash>, Vec<Vec<u8>>> = HashMap::new();
    // `mutable_key_type` is a false positive: DeployChainIndex's Hash/Eq use
    // only its immutable identity fields, not the interior-mutable caches.
    #[allow(clippy::mutable_key_type)]
    let mut accepted = HashSet::new();
    #[allow(clippy::mutable_key_type)]
    let mut rejected = HashableSet(HashSet::new());

    for chain in branch_items {
        if rejected
            .0
            .iter()
            .any(|rejected_chain| depends(chain, rejected_chain))
        {
            rejected.0.insert(chain.clone());
            continue;
        }

        let changes = state_changes(chain)?;
        let chain_mergeable = mergeable_channels(chain);
        let mut next_data = available_data.clone();
        let mut next_continuations = available_continuations.clone();
        let mut applicable = true;

        for entry in changes.datums_changes.iter() {
            let channel = entry.key();
            if chain_mergeable.contains_key(channel) {
                continue;
            }
            if branch_mergeable.contains_key(channel) {
                tracing::debug!(
                    target: "f1r3.trace.unavail",
                    channel = ?channel,
                    "reject: datum channel in branch_mergeable"
                );
                applicable = false;
                break;
            }
            let changed = !entry.value().removed.is_empty() || !entry.value().added.is_empty();
            if !changed {
                continue;
            }
            let removed = &entry.value().removed;
            if removed.is_empty() {
                continue;
            }
            if !next_data.contains_key(channel) {
                next_data.insert(channel.clone(), base_data(channel)?);
            }
            if !remove_from_available(next_data.get_mut(channel).unwrap(), removed) {
                tracing::debug!(
                    target: "f1r3.trace.unavail",
                    channel = ?channel,
                    removed = removed.len(),
                    available = next_data.get(channel).map(|v| v.len()),
                    "reject: datum remove not available at base"
                );
                applicable = false;
                break;
            }
        }

        if applicable {
            for entry in changes.cont_changes.iter() {
                let consume_channels = entry.key();
                let removed = &entry.value().removed;
                if removed.is_empty() {
                    continue;
                }
                if !next_continuations.contains_key(consume_channels) {
                    next_continuations.insert(
                        consume_channels.clone(),
                        base_continuations(consume_channels)?,
                    );
                }
                if !remove_from_available(
                    next_continuations.get_mut(consume_channels).unwrap(),
                    removed,
                ) {
                    tracing::debug!(
                        target: "f1r3.trace.unavail",
                        consume_channels = ?consume_channels,
                        removed = removed.len(),
                        available = next_continuations.get(consume_channels).map(|v| v.len()),
                        "reject: continuation remove not available at base"
                    );
                    applicable = false;
                    break;
                }
            }
        }

        if applicable {
            for entry in changes.datums_changes.iter() {
                let channel = entry.key();
                if chain_mergeable.contains_key(channel) {
                    continue;
                }
                let added = &entry.value().added;
                if added.is_empty() {
                    continue;
                }
                if !next_data.contains_key(channel) {
                    next_data.insert(channel.clone(), base_data(channel)?);
                }
                let base = base_data(channel)?;
                let current = next_data.get(channel).unwrap();
                if numeric_cell_would_overfill(&base, current, added) {
                    tracing::debug!(
                        target: "f1r3.trace.unavail",
                        channel = ?channel,
                        base = base.len(),
                        current = current.len(),
                        added = added.len(),
                        "reject: numeric cell would overfill"
                    );
                    applicable = false;
                    break;
                }
                next_data.get_mut(channel).unwrap().extend(added.clone());
            }
        }

        if applicable {
            for entry in changes.cont_changes.iter() {
                let consume_channels = entry.key();
                let added = &entry.value().added;
                if added.is_empty() {
                    continue;
                }
                if !next_continuations.contains_key(consume_channels) {
                    next_continuations.insert(
                        consume_channels.clone(),
                        base_continuations(consume_channels)?,
                    );
                }
                next_continuations
                    .get_mut(consume_channels)
                    .unwrap()
                    .extend(added.clone());
            }
            available_data = next_data;
            available_continuations = next_continuations;
            accepted.insert(chain.clone());
        }

        if !applicable {
            rejected.0.insert(chain.clone());
        }
    }

    let accepted = if accepted.is_empty() {
        None
    } else {
        Some(HashableSet(accepted))
    };

    Ok((accepted, rejected))
}

fn split_unavailable_resolved_branches(
    resolved: &mut conflict_set_merger::ResolvedConflicts<DeployChainIndex>,
    depends: &impl Fn(&DeployChainIndex, &DeployChainIndex) -> bool,
    state_changes: &impl Fn(
        &DeployChainIndex,
    ) -> Result<
        rspace_plus_plus::rspace::merger::state_change::StateChange,
        rspace_plus_plus::rspace::errors::HistoryError,
    >,
    mergeable_channels: &impl Fn(&DeployChainIndex) -> NumberChannelsDiff,
    base_data: &impl Fn(
        &Blake2b256Hash,
    ) -> Result<Vec<Vec<u8>>, rspace_plus_plus::rspace::errors::HistoryError>,
    base_continuations: &impl Fn(
        &Vec<Blake2b256Hash>,
    )
        -> Result<Vec<Vec<u8>>, rspace_plus_plus::rspace::errors::HistoryError>,
) -> Result<HashableSet<DeployChainIndex>, rspace_plus_plus::rspace::errors::HistoryError> {
    let mut kept_branches = Vec::new();
    let mut rejected_all = HashableSet(HashSet::new());

    for branch in std::mem::take(&mut resolved.to_merge) {
        let (kept, rejected) = split_unavailable_branch_consumes(
            branch,
            depends,
            state_changes,
            mergeable_channels,
            base_data,
            base_continuations,
        )?;
        for chain in rejected.0 {
            rejected_all.0.insert(chain.clone());
            resolved.rejected.0.insert(chain);
        }
        if let Some(kept) = kept {
            kept_branches.push(kept);
        }
    }

    resolved.to_merge = kept_branches;
    Ok(rejected_all)
}

/// §3c keep-one: among the surviving (`to_merge`) branches, reject the minimal
/// set of writers that would over-fill a single-value (number) cell, keeping
/// one. This is the finer counterpart to the apply-time guard in the merge
/// override: instead of failing the whole merge, drop the losing writers (which
/// recovery re-proposes) and merge the rest.
///
/// Only the SAME channels the apply-time guard covers are considered: a channel
/// whose base is a single numeric datum AND that this merge does not fold
/// (absent from any branch's mergeable set). Folded number channels are
/// reconciled by the number-channel fold and must not be rejected here;
/// registry / TreeHashMap nodes are non-numeric and exempt. The kept writer is
/// the lowest-ordered `DeployChainIndex`, so the choice is node-deterministic.
// False positive on `pinned`: DeployChainIndex's Hash/Eq use only immutable fields.
#[allow(clippy::mutable_key_type)]
fn split_overfilled_single_value_cells(
    resolved: &mut conflict_set_merger::ResolvedConflicts<DeployChainIndex>,
    depends: &impl Fn(&DeployChainIndex, &DeployChainIndex) -> bool,
    mergeable_channels: &impl Fn(&DeployChainIndex) -> NumberChannelsDiff,
    base_datum: &impl Fn(
        &Blake2b256Hash,
    ) -> Result<
        Vec<Datum<ListParWithRandom>>,
        rspace_plus_plus::rspace::errors::HistoryError,
    >,
    base_binary: &impl Fn(
        &Blake2b256Hash,
    ) -> Result<Vec<Vec<u8>>, rspace_plus_plus::rspace::errors::HistoryError>,
    // Chains already committed in the main parent's state; preferred as the
    // survivor when a single-value cell has to keep exactly one writer.
    //
    // ALWAYS EMPTY in production: the base became the main parent, so its
    // chains are in the base rather than in the conflict set and there is
    // nothing to prefer. Only unit tests supply a non-empty set.
    pinned: &HashSet<DeployChainIndex>,
) -> Result<HashableSet<DeployChainIndex>, rspace_plus_plus::rspace::errors::HistoryError> {
    let mut all_chains: Vec<DeployChainIndex> = resolved
        .to_merge
        .iter()
        .flat_map(|b| b.0.iter().cloned())
        .collect();
    all_chains.sort();

    let mut folded: HashSet<Blake2b256Hash> = HashSet::new();
    for chain in &all_chains {
        for key in mergeable_channels(chain).keys() {
            folded.insert(key.clone());
        }
    }

    // Combined per-channel change + the ordered producers (chains that add).
    let mut combined: HashMap<Blake2b256Hash, ChannelChange<Vec<u8>>> = HashMap::new();
    let mut producers: HashMap<Blake2b256Hash, Vec<DeployChainIndex>> = HashMap::new();
    // False positive: DeployChainIndex's Hash/Eq use only immutable fields.
    #[allow(clippy::mutable_key_type)]
    let mut rejected_seed: HashSet<DeployChainIndex> = HashSet::new();
    for chain in &all_chains {
        let chain_mergeable = mergeable_channels(chain);
        for entry in chain.state_changes.datums_changes.iter() {
            let ch = entry.key().clone();
            let chg = entry.value();
            if chain_mergeable.contains_key(&ch) {
                continue;
            }
            if folded.contains(&ch) && (!chg.added.is_empty() || !chg.removed.is_empty()) {
                rejected_seed.insert(chain.clone());
                continue;
            }
            let c = combined
                .entry(ch.clone())
                .or_insert_with(ChannelChange::empty);
            c.added.extend(chg.added.clone());
            c.removed.extend(chg.removed.clone());
            if !chg.added.is_empty() {
                producers.entry(ch).or_default().push(chain.clone());
            }
        }
    }

    // Seed rejections: for each over-filled single-value cell, keep the lowest
    // producer, reject the rest.
    for (ch, chg) in &combined {
        if chg.added.is_empty() {
            continue;
        }
        let base_d = base_datum(ch)?;
        let is_single_number = base_d.len() == 1
            && RholangMergingLogic::try_get_number_with_rnd(&base_d[0].a).is_some();
        let base_b = base_binary(ch)?;
        let kept = StateChange::multiset_diff(&base_b, &chg.removed);
        let empty_numeric_cell = base_d.is_empty()
            && kept.is_empty()
            && chg
                .added
                .iter()
                .all(|bytes| RholangMergingLogic::encoded_datum_is_number(bytes));
        if !is_single_number && !empty_numeric_cell {
            continue;
        }
        if kept.len() + chg.added.len() > 1 {
            if let Some(prod) = producers.get(ch) {
                // Keep-one picks by chain order, which knows nothing about
                // provenance — so it can drop the writer whose effect the MAIN
                // PARENT already committed, leaving this block's state missing
                // content its own spine ancestor holds. Order pinned producers
                // first. `DeployChainIndex` ordering breaks all remaining
                // ties after pinning and rejection history. It is a
                // preference inside an existing keep-one, never a veto: if
                // every producer is pinned, one still loses and the merge
                // stays live.
                let mut ordered: Vec<&DeployChainIndex> = prod.iter().collect();
                // Pinned first, then the chain with the most prior on-DAG
                // rejections (issue #294: a chain the content ordering keeps
                // losing must gain priority with each loss, or it starves to
                // expiry). The chain's total order breaks remaining ties.
                ordered.sort_by(|a, b| {
                    (!pinned.contains(*a), std::cmp::Reverse(a.prior_rejections))
                        .cmp(&(!pinned.contains(*b), std::cmp::Reverse(b.prior_rejections)))
                        .then_with(|| a.cmp(b))
                });
                for loser in ordered.iter().skip(1) {
                    rejected_seed.insert((*loser).clone());
                }
            }
        }
    }

    // Expand rejections to dependents, then prune the branches.
    let mut rejected = HashableSet(HashSet::new());
    if !rejected_seed.is_empty() {
        for chain in &all_chains {
            if rejected_seed.contains(chain) || rejected_seed.iter().any(|r| depends(chain, r)) {
                rejected.0.insert(chain.clone());
            }
        }
        let mut new_to_merge = Vec::new();
        for branch in std::mem::take(&mut resolved.to_merge) {
            // False positive: DeployChainIndex's Hash/Eq use only immutable fields.
            #[allow(clippy::mutable_key_type)]
            let kept: std::collections::HashSet<DeployChainIndex> = branch
                .0
                .into_iter()
                .filter(|c| !rejected.0.contains(c))
                .collect();
            if !kept.is_empty() {
                new_to_merge.push(HashableSet(kept));
            }
        }
        resolved.to_merge = new_to_merge;
        for c in &rejected.0 {
            resolved.rejected.0.insert(c.clone());
        }
    }
    Ok(rejected)
}

/// Per-sig prior-rejection counts derived from the rejection records the
/// merge scope holds (issue #294). On-DAG data: every validator sees the
/// same records for the same scope, so the derived priority is
/// consensus-deterministic.
pub fn prior_rejection_counts<'a>(
    records: impl IntoIterator<Item = &'a RejectedDeploy>,
) -> HashMap<Bytes, u64> {
    let mut counts: HashMap<Bytes, u64> = HashMap::new();
    count_kept_records(&mut counts, records.into_iter().cloned());
    counts
}

fn count_kept_records(
    counts: &mut HashMap<Bytes, u64>,
    records: impl IntoIterator<Item = RejectedDeploy>,
) {
    for record in records {
        if !record.duplicate {
            *counts.entry(record.sig).or_insert(0) += 1;
        }
    }
}

/// Derive the per-sig prior-rejection counts a merge may use: the kept
/// (non-duplicate) records of every block the merge can see. The caller
/// assembles the visible block set — the merge scope plus the base-lineage
/// window the scope builds on. The base-lineage half is load-bearing: the
/// retry gate opens only after a rejection settles below the floor, so at
/// retry time the record lives on the base's lineage, not in the scope.
///
/// Records are counted as each block's records load, so the whole window's
/// record set is never held at once. A block the caller cannot supply is an
/// error, never an empty history: the counts feed the rejection set that
/// peers validate (`InvalidRejectedDeploy`), so every validator must derive
/// them from the identical block set or refuse to derive them at all.
pub fn scope_prior_rejection_counts(
    visible_blocks: impl IntoIterator<Item = BlockHash>,
    records_of: impl Fn(&BlockHash) -> Result<Vec<RejectedDeploy>, CasperError>,
) -> Result<HashMap<Bytes, u64>, CasperError> {
    let mut counts: HashMap<Bytes, u64> = HashMap::new();
    let mut seen = HashSet::new();
    for block in visible_blocks {
        if seen.insert(block.clone()) {
            count_kept_records(&mut counts, records_of(&block)?);
        }
    }
    Ok(counts)
}

/// Stamp each chain with the maximum prior-rejection count of its deploys.
/// Chains whose deploys carry no records keep the default of zero, so
/// adjudication is unchanged where no losses are on record.
pub fn stamp_prior_rejections(chains: &mut [DeployChainIndex], counts: &HashMap<Bytes, u64>) {
    for chain in chains.iter_mut() {
        chain.prior_rejections = chain
            .deploys_with_cost
            .0
            .iter()
            .map(|d| counts.get(&d.deploy_id).copied().unwrap_or(0))
            .max()
            .unwrap_or(0);
    }
}

/// Attribute a merge failure to the chains that could have caused it, and log
/// it as ONE error line carrying the merge context.
///
/// Reports, for the channel the error names: every surviving chain that
/// removes on it (the candidates for the offending removal) with its source
/// block and deploy sigs, every rejected chain that ADDS on it (a rejected
/// producer is how a survivor's removal loses its backing), and the merge
/// coordinates — base, base state, floor height, scope size,
/// survivor/rejected counts.
///
/// Best-effort by construction: it runs only when the merge already failed, so
/// a probe that cannot answer degrades the report rather than the error.
fn explain_merge_failure(
    err: &rspace_plus_plus::rspace::errors::HistoryError,
    resolved: &conflict_set_merger::ResolvedConflicts<DeployChainIndex>,
    base: &BlockHash,
    base_post_state: &Blake2b256Hash,
    floor_block_number: i64,
    scope: &Option<HashSet<BlockHash>>,
) {
    let message = err.to_string();
    // The channel is the one datum the error always carries; use it to select
    // the chains worth naming instead of dumping the whole survivor set.
    let channel_hex: Option<String> = message
        .split("channel ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .map(|s| s.trim_end_matches(':').to_string());

    let describe = |chain: &DeployChainIndex| -> String {
        let sigs: Vec<String> = chain
            .deploys_with_cost
            .0
            .iter()
            .map(|d| hex::encode(&d.deploy_id[..8.min(d.deploy_id.len())]))
            .collect();
        format!(
            "src={}#{} sigs={:?}",
            hex::encode(&chain.source_block_hash[..8.min(chain.source_block_hash.len())]),
            chain.source_block_number,
            sigs
        )
    };
    let touches = |chain: &DeployChainIndex, want_added: bool| -> bool {
        let Some(ref ch) = channel_hex else {
            return false;
        };
        chain.state_changes.datums_changes.iter().any(|entry| {
            hex::encode(entry.key().clone().bytes()) == *ch
                && if want_added {
                    !entry.value().added.is_empty()
                } else {
                    !entry.value().removed.is_empty()
                }
        })
    };

    let surviving_removers: Vec<String> = resolved
        .to_merge
        .iter()
        .flat_map(|b| b.0.iter())
        .filter(|c| touches(c, false))
        .take(8)
        .map(describe)
        .collect();
    let rejected_producers: Vec<String> = resolved
        .rejected
        .0
        .iter()
        .filter(|c| touches(c, true))
        .take(8)
        .map(describe)
        .collect();

    tracing::error!(
        target: "f1r3fly.merge.incoherence",
        error = %message,
        channel = channel_hex.as_deref().unwrap_or("unparsed"),
        floor_block = floor_block_number,
        base = %hex::encode(&base[..8.min(base.len())]),
        base_state = %hex::encode(&base_post_state.clone().bytes()[..8]),
        scope_blocks = scope.as_ref().map(|s| s.len()).unwrap_or(0),
        surviving_chains = resolved.to_merge.iter().map(|b| b.0.len()).sum::<usize>(),
        rejected_chains = resolved.rejected.0.len(),
        surviving_removers_on_channel = ?surviving_removers,
        rejected_producers_on_channel = ?rejected_producers,
        "merge failed: applied diffs incoherent with the base — every propose over \
         this scope will fail identically until the scope or the floor changes"
    );
}

fn resolve_conflicts_with_unavailable_retry(
    actual_seq_all: &[DeployChainIndex],
    late_seq_all: &[DeployChainIndex],
    resolve_once: &impl Fn(
        Vec<DeployChainIndex>,
        Vec<DeployChainIndex>,
    ) -> Result<
        conflict_set_merger::ResolvedConflicts<DeployChainIndex>,
        rspace_plus_plus::rspace::errors::HistoryError,
    >,
    split_unavailable: &impl Fn(
        &mut conflict_set_merger::ResolvedConflicts<DeployChainIndex>,
    ) -> Result<
        HashableSet<DeployChainIndex>,
        rspace_plus_plus::rspace::errors::HistoryError,
    >,
) -> Result<
    (
        conflict_set_merger::ResolvedConflicts<DeployChainIndex>,
        usize,
    ),
    rspace_plus_plus::rspace::errors::HistoryError,
> {
    let mut forced_rejected = HashableSet(HashSet::new());

    loop {
        let actual_seq: Vec<_> = actual_seq_all
            .iter()
            .filter(|chain| !forced_rejected.0.contains(*chain))
            .cloned()
            .collect();
        let mut late_seq = late_seq_all.to_vec();
        late_seq.extend(forced_rejected.0.iter().cloned());

        let mut resolved = resolve_once(actual_seq, late_seq)?;
        let unavailable = split_unavailable(&mut resolved)?;
        let mut added = 0usize;
        for chain in unavailable.0 {
            if forced_rejected.0.insert(chain) {
                added += 1;
            }
        }

        if added == 0 {
            return Ok((resolved, forced_rejected.0.len()));
        }
    }
}

/// Merge-time validity-window rule, keyed on the merging block's FLOOR: a
/// chain carrying a USER deploy whose window is closed at the floor
/// (`valid_after <= floor_block_number - deploy_lifespan`) must not merge.
/// A silent validator's stale tip stays mergeable indefinitely (below-floor
/// sibling), so a within-window carrier can arrive arbitrarily late;
/// executing it would land effects after the deploy's validity window
/// closed and reopen a settled Expired verdict.
///
/// The floor — never the merge height — is the correct clock: for any
/// VALIDLY included chain, inclusion height `h <= valid_after + lifespan`,
/// so if the rule fires (`floor > valid_after + lifespan >= h`) the
/// chain's block lies below the floor — and the base sits at or above the
/// floor, so an in-scope chain of ordinary standing can never be hit. The rule
/// fires exactly on the below-floor-sibling (late-carrier) class. The
/// floor is a pure function of the block's parents and justifications, and
/// justification-regression validation stops a proposer from faking a
/// lower one, so once any canonical block's floor passes a deploy's
/// window, every future canonical merge rejects its late carriers — which
/// is what makes a floor-keyed `Expired` verdict terminal.
///
/// Rejected chains are recorded like any other loser; the block-expired
/// selection filter uses the same bound, so recovery never re-proposes
/// them. Chains with no window entries (system-only) are exempt.
fn split_window_closed_chains(
    chains: Vec<DeployChainIndex>,
    floor_block_number: i64,
    deploy_lifespan: i64,
) -> (Vec<DeployChainIndex>, Vec<DeployChainIndex>) {
    let earliest_valid_after = floor_block_number - deploy_lifespan;
    chains.into_iter().partition(|chain| {
        chain
            .deploy_windows
            .values()
            .all(|valid_after| *valid_after > earliest_valid_after)
    })
}

/// Merge the scope onto `base`.
///
/// `base` is the merging block's state parent — its main parent, or the
/// finalized floor when that parent's state does not hold the floor's settled
/// content. It is NOT the LFB, and has not been since the base moved off the
/// floor; the merge only ever needed a committed state to build on and the
/// block hash that names it.
pub fn merge(
    dag: &KeyValueDagRepresentation,
    base: &BlockHash,
    base_post_state: &Blake2b256Hash,
    index: impl Fn(&BlockHash) -> Result<Vec<DeployChainIndex>, CasperError>,
    history_repository: &RhoHistoryRepository,
    rejection_cost_f: impl Fn(&DeployChainIndex) -> u64,
    scope: Option<HashSet<BlockHash>>,
    disable_late_block_filtering: bool,
    floor_block_number: i64,
    deploy_lifespan: i64,
    // True iff the sig's effect is already present in the BASE state. The
    // dedup below seeds such sigs with an unbeatable freshest-copy
    // sentinel so every scope copy drops: the settled copy lives in the
    // base where scope-level dedup cannot see it, and without the seed the
    // scope copy re-applies and doubles the deploy's cells.
    sig_settled_in_base: &dyn Fn(&Bytes) -> Result<bool, CasperError>,
    // Blocks on the BASE's own lineage that at least one other parent has not
    // seen — the base's contribution since the parents diverged. Bounded by
    // branch divergence, not by finality lag. Empty for a single-parent block,
    // which has nothing to merge against.
    base_lineage_blocks: &HashSet<BlockHash>,
    // Per-sig prior-rejection counts derived from the records every block in
    // the merge's view carries (issue #294). The caller assembles them from
    // on-DAG data (see `scope_prior_rejection_counts`), so proposer and
    // validators derive identical counts and adjudication stays
    // consensus-deterministic. Losses outrank content ordering in both
    // adjudication sites, so a repeatedly rejected deploy gains priority
    // instead of starving to expiry.
    prior_rejection_counts: &HashMap<Bytes, u64>,
) -> Result<
    (
        Blake2b256Hash,
        // Rejected user deploys as full records: each names the CARRIER it
        // adjudicated (the rejected chain's source block) and carries the
        // formation-time duplicate flag. The record is consensus content —
        // it travels to the block body as-is.
        Vec<RejectedDeploy>,
        Vec<(Bytes, BlockHash)>,
        // User sigs whose chains this merge APPLIED from scope: their
        // effects are in the returned state, so none of them may also be
        // executed fresh on top of it. The merge is the only place this
        // set is known.
        HashSet<Bytes>,
    ),
    CasperError,
> {
    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        tracing::debug!(target: "f1r3fly.merge.step", step = "merge.ENTER",
            base = %hex::encode(&base[..]),
            base_post_state = %hex::encode(base_post_state.clone().bytes()),
            scope = %scope
                .as_ref()
                .map_or("ALL".to_string(), |s| format!("{} blocks", s.len())),
            disable_late_block_filtering = disable_late_block_filtering);
    }

    // Blocks to merge are all blocks in scope that are NOT the base or on its
    // main-parent chain. This includes:
    // 1. Descendants of the base (blocks built on top of it)
    // 2. Siblings of the base (same height, different branch) that are ancestors of the tips
    // Previously we only included descendants, which missed deploy effects from sibling branches.
    let actual_blocks: HashSet<BlockHash> = match &scope {
        Some(scope_blocks) => {
            // Avoid unbounded full-DAG ancestor scans. Check each scope block against the base directly.
            let mut result = HashSet::new();
            for candidate in scope_blocks {
                if !dag.is_in_main_chain(candidate, base)? {
                    result.insert(candidate.clone());
                }
            }
            if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                let included: Vec<String> = result.iter().map(|b| hex::encode(&b[..])).collect();
                // Scope blocks excluded from the merge set because they ARE on the
                // base's main chain (the base itself or one of its main-parent ancestors).
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
            // Legacy behavior: use descendants of the base
            let descendants = dag.descendants(base)?;
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
        "DagMerger.merge: base={}, scope={}, actualBlocks (above base)={}, lateBlocks={}",
        hex::encode(&base[..std::cmp::min(8, base.len())]),
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

    // Both sets are stamped: `resolve_conflicts` rejects every late chain
    // outright today, but it receives both sequences and ranks on
    // `prior_rejections`, so a late chain must never reach a loss-aware
    // comparison carrying the constructor's zero instead of its record.
    stamp_prior_rejections(&mut actual_set_vec, prior_rejection_counts);
    stamp_prior_rejections(&mut late_set_vec, prior_rejection_counts);

    // Accumulator for deploys that lose their chain via dedup but have no
    // fresher copy elsewhere. These are treated the same as conflict-rejected
    // deploys downstream — added to the rejected-deploy buffer so the
    // recovery path can re-propose them in a subsequent block.
    let mut collateral_lost_pairs: Vec<(Bytes, BlockHash)> = Vec::new();

    // Memoized settled-in-base results, one probe per unique sig per merge.
    let mut settled_checked: HashSet<Bytes> = HashSet::new();
    let mut settled_sigs: HashSet<Bytes> = HashSet::new();

    // Deploy de-duplication. When the same deploy ID appears in chains from
    // multiple blocks in scope — for example, because a previously-rejected
    // deploy was re-proposed in a later block — keep the copy from the freshest
    // source: higher block number first, then lexicographically-smaller block
    // hash as a deterministic tiebreak. A chain containing any deploy whose
    // freshest source is a different chain is dropped; its diffs were computed
    // against a pre-state that the fresh execution replaces.
    if !actual_set_vec.is_empty() {
        // Find the freshest source for each deploy_id across all chains.
        // A sig whose effect is already SETTLED IN THE BASE is seeded with
        // an unbeatable sentinel: the base itself is the freshest "copy",
        // so every scope copy is stale and its chain drops. The settled
        // copy sits in the base where scope-level dedup cannot see it —
        // without the seed the scope copy re-applies and doubles the
        // per-deploy cells. (The sentinel hash is empty, which no real
        // chain source can equal, so the retain below never keeps a
        // settled copy and the collateral pass correctly treats settled
        // sigs as not-lost.)
        let mut latest_for_deploy: HashMap<Bytes, (i64, BlockHash)> = HashMap::new();
        for chain in &actual_set_vec {
            for deploy in &chain.deploys_with_cost.0 {
                if is_system_deploy_id(&deploy.deploy_id) {
                    continue;
                }
                if settled_checked.insert(deploy.deploy_id.clone())
                    && sig_settled_in_base(&deploy.deploy_id)?
                {
                    tracing::info!(
                        "DagMerger dedup: sig {} already settled in the base; dropping all scope copies",
                        hex::encode(&deploy.deploy_id[..8.min(deploy.deploy_id.len())]),
                    );
                    settled_sigs.insert(deploy.deploy_id.clone());
                    latest_for_deploy
                        .insert(deploy.deploy_id.clone(), (i64::MAX, BlockHash::new()));
                }
            }
        }
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

    // Merge-time validity-window rule (see split_window_closed_chains):
    // closed-window chains join the LATE set — `resolve_conflicts` rejects
    // late chains unconditionally (they reach the block's rejection record
    // through the standard pair assembly) and rejects actual chains that
    // depend on them; the stale-diff lineage expansion afterward covers
    // state-lineage descendants. The floor-relative window is the
    // deterministic lateness definition the legacy (nondeterministic,
    // disabled) late-block query lacked. A chain both settled-in-base and
    // window-closed was already dropped record-less by the dedup sentinel
    // above — fine: its effect stands, a record would be dup-flagged
    // testimony.
    let (in_window, window_rejected) =
        split_window_closed_chains(actual_set_vec, floor_block_number, deploy_lifespan);
    actual_set_vec = in_window;
    if !window_rejected.is_empty() {
        tracing::info!(
            target: "f1r3fly.merge.step",
            "DagMerger window rule: rejected {} late chain(s) whose deploy validity window is closed at floor #{}",
            window_rejected.len(),
            floor_block_number,
        );
        late_set_vec.extend(window_rejected);
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
    let actual_seq_all = actual_set_vec;
    let late_seq_all = late_set_vec;

    // The base's OWN contribution since the parents diverged, as one combined
    // event log. `conflicts` compares two chains' event logs, and the base is
    // not one of them — so once the base carries content of its own, nothing
    // in conflict resolution can see it. An incoming chain's log was computed
    // against a state that work is not in, so its surviving produce and the
    // base's matching consume can land side by side un-COMM'd: a state no
    // sequential execution reaches, and one that a later deploy can observe.
    //
    // A scope chain conflicting with the base loses by construction. The base
    // is committed — it cannot be adjudicated away — so this is a decision, not
    // a preference, and it needs no fallback.
    let mut base_event_log =
        rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex::empty();
    let mut base_lineage_sorted: Vec<&BlockHash> = base_lineage_blocks.iter().collect();
    base_lineage_sorted.sort();
    for block_hash in base_lineage_sorted {
        for chain in index(block_hash)? {
            base_event_log =
                rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex::combine(
                    &base_event_log,
                    &chain.event_log_index,
                )
                .map_err(CasperError::HistoryError)?;
        }
    }
    let (actual_seq_all, base_conflicting): (Vec<DeployChainIndex>, Vec<DeployChainIndex>) =
        actual_seq_all.into_iter().partition(|chain| {
            !merging_logic::are_conflicting(&chain.event_log_index, &base_event_log)
        });
    if !base_conflicting.is_empty() {
        tracing::debug!(
            target: "f1r3fly.merge.cpps",
            step = "merge.reject_conflicts_with_base",
            n_rejected = base_conflicting.len(),
            n_base_blocks = base_lineage_blocks.len(),
            "scope chains conflicting with the base's own committed content"
        );
    }

    // Nothing to pin. Pinning existed to keep the main parent's chains out of
    // the rejection set while the merge rebuilt state from the floor and those
    // chains were in scope competing on cost. The base is the main parent now,
    // so its content is committed under the merge rather than adjudicated by
    // it, and every consumer below runs its empty-set path.
    #[allow(clippy::mutable_key_type)]
    let pinned: HashSet<DeployChainIndex> = HashSet::new();

    struct BranchDerived {
        user_deploy_ids: HashSet<Bytes>,
        combined_all_event_log: rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex,
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
            combined_all_event_log,
        })
    }

    // Create history reader for base state
    let history_reader = std::sync::Arc::new(
        history_repository
            .get_history_reader(base_post_state)
            .map_err(|e| CasperError::HistoryError(e))?,
    );

    // Bind merge-logic closures to named variables so both resolve_conflicts
    // and compute_merged_state can take them by reference, with the rejection
    // expansion step interposed between the two calls.
    let depends_fn = |target: &DeployChainIndex, source: &DeployChainIndex| -> bool {
        let produces_created =
            merging_logic::produces_created_and_not_destroyed(&source.event_log_index);
        let consumes_created =
            merging_logic::consumes_created_and_not_destroyed(&source.event_log_index);

        let produces_source: HashSet<_> = produces_created
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

        let consume_dep = consumes_created
            .0
            .intersection(&target.event_log_index.consumes_produced.0)
            .next()
            .is_some();
        if consume_dep && tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
            tracing::debug!(target: "f1r3fly.merge.step", step = "depends_fn.TRUE_CONSUME_INTERSECTION",
                target_src = %hex::encode(&target.source_block_hash[..]),
                source_src = %hex::encode(&source.source_block_hash[..]),
                n_consume_overlap = consumes_created
                    .0
                    .intersection(&target.event_log_index.consumes_produced.0)
                    .count());
        }
        consume_dep
    };

    let state_changes_fn = |chain: &DeployChainIndex| Ok(chain.state_changes.clone());

    let mergeable_channels_fn =
        |chain: &DeployChainIndex| chain.event_log_index.number_channels_data.clone();

    let mk_compute_trie_actions_fn = |multi_writer_channels: HashSet<Blake2b256Hash>| {
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
                        // §3c single-value-cell discriminator: reject a merge that would
                        // over-fill a single-value (number) cell via a write this merge did
                        // not fold. Registry / TreeHashMap nodes are non-numeric and exempt.
                        // Prevents the RhoVM IntegerAdd single-value invariant tripping at
                        // read time (RCA-asi-devnet-finality-halt). `added`-empty changes
                        // skip the base read inside the helper.
                        //
                        // Provenance gate for the base-empty arm: a channel absent at
                        // base whose adds all came from a SINGLE accepted chain is that
                        // branch's own internal state — it executed sequentially in PLAY
                        // and validated in REPLAY, so multiple numeric datums prove the
                        // channel is not a single-value cell. Overfill on a base-empty
                        // channel is only a cross-writer accumulation phenomenon.
                        if !channel_changes.added.is_empty() {
                            let base = reader.get_data(hash)?;
                            if !base.is_empty() || multi_writer_channels.contains(hash) {
                                let base_bin = reader.get_data_proj_binary(hash)?;
                                RholangMergingLogic::check_single_value_cell_not_overfilled(
                                    hash,
                                    &base,
                                    &base_bin,
                                    channel_changes,
                                )?;
                            }
                        }
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
            .reset(base_post_state)
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
    // Branch event-log combination is fallible —
    // a MergeType mismatch propagates as a hard error so the merge is
    // rejected rather than silently absorbing the invariant violation.
    let compute_conflict_map_fn = |branches_set: &HashableSet<HashableSet<DeployChainIndex>>| -> Result<
        HashMap<HashableSet<DeployChainIndex>, HashableSet<HashableSet<DeployChainIndex>>>,
        rspace_plus_plus::rspace::errors::HistoryError,
    > {
        // Snapshot branch references in a stable order so the parallel
        // arrays passed into the indexed map and the deploy-id pass below
        // line up.
        let branches_refs: Vec<&HashableSet<DeployChainIndex>> = branches_set.0.iter().collect();
        let branches_owned: Vec<HashableSet<DeployChainIndex>> =
            branches_refs.iter().map(|b| (*b).clone()).collect();
        let branch_derived: Vec<BranchDerived> = branches_refs
            .iter()
            .map(|branch| compute_branch_derived(branch))
            .collect::<Result<Vec<_>, _>>()?;

        let event_logs: Vec<&rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex> =
            branch_derived
                .iter()
                .map(|derived| &derived.combined_all_event_log)
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
        for (idx, derived) in branch_derived.iter().enumerate() {
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

    let resolve_once = |actual_seq: Vec<DeployChainIndex>, late_seq: Vec<DeployChainIndex>| {
        conflict_set_merger::resolve_conflicts(
            actual_seq,
            late_seq,
            &depends_fn,
            &rejection_cost_f,
            &|chain: &DeployChainIndex| chain.prior_rejections,
            &mergeable_channels_fn,
            &get_data_fn,
            &compute_branches_fn,
            &compute_conflict_map_fn,
            &pinned,
        )
    };

    let split_unavailable =
        |resolved: &mut conflict_set_merger::ResolvedConflicts<DeployChainIndex>| {
            let mut rejected = split_unavailable_resolved_branches(
                resolved,
                &depends_fn,
                &state_changes_fn,
                &mergeable_channels_fn,
                &|channel| history_reader.get_data_proj_binary(channel),
                &|consume_channels| {
                    let history_pointer =
                        rspace_plus_plus::rspace::hashing::stable_hash_provider::hash_from_hashes(
                            consume_channels,
                        );
                    history_reader.get_continuations_proj_binary(&history_pointer)
                },
            )?;
            // §3c keep-one: drop losing writers to an over-filled single-value
            // cell (recovery re-proposes them) instead of failing the merge.
            let overfilled = split_overfilled_single_value_cells(
                resolved,
                &depends_fn,
                &mergeable_channels_fn,
                &|channel| history_reader.get_data(channel),
                &|channel| history_reader.get_data_proj_binary(channel),
                &pinned,
            )?;
            for chain in overfilled.0 {
                rejected.0.insert(chain);
            }
            Ok(rejected)
        };

    let (mut resolved, unavailable_rejected_count) = resolve_conflicts_with_unavailable_retry(
        &actual_seq_all,
        &late_seq_all,
        &resolve_once,
        &split_unavailable,
    )
    .map_err(CasperError::HistoryError)?;

    // Chains the base's own content precluded. Folded in here so they travel
    // the ordinary rejection path — record, buffer, recovery — like any other
    // adjudicated loser.
    for chain in base_conflicting {
        resolved.rejected.0.insert(chain);
    }

    if unavailable_rejected_count > 0 {
        tracing::debug!(
            target: "f1r3fly.merge.step",
            step = "merge.reject_unavailable_floor_consumes",
            rejected_chains = unavailable_rejected_count,
            remaining_branches = resolved.to_merge.len()
        );
    }

    // Rejection expansion over block lineage: a surviving chain whose source
    // block DAG-descends from a rejected chain's source block was computed
    // against a pre-state that materializes the rejected work. Applying its
    // pre-computed diffs on a merge base WITHOUT that work is a stale-diff
    // application — the descendant's effects appear while its ancestor's are
    // absent, an internally inconsistent post-state. Reject the descendants'
    // chains as well; recovery re-proposes them against the actual merged
    // base. Event-log dependency expansion (inside conflict resolution)
    // cannot catch this: the descendant may touch disjoint channels and still
    // be state-lineage-dependent. Ancestry is transitive, so one pass covers
    // deeper descendants.
    {
        let rejected_blocks: HashSet<BlockHash> = resolved
            .rejected
            .0
            .iter()
            .map(|chain| chain.source_block_hash.clone())
            .collect();
        if !rejected_blocks.is_empty() {
            let mut descends_cache: HashMap<BlockHash, bool> = HashMap::new();
            let mut descends_rejected = |block_hash: &BlockHash| -> Result<bool, CasperError> {
                if let Some(cached) = descends_cache.get(block_hash) {
                    return Ok(*cached);
                }
                let mut result = false;
                for rejected_block in &rejected_blocks {
                    if rejected_block != block_hash
                        && dag.is_dag_ancestor(rejected_block, block_hash)?
                    {
                        result = true;
                        break;
                    }
                }
                descends_cache.insert(block_hash.clone(), result);
                Ok(result)
            };
            let mut expanded = 0usize;
            let mut new_to_merge = Vec::new();
            for branch in std::mem::take(&mut resolved.to_merge) {
                // False positive: DeployChainIndex's Hash/Eq use only immutable fields.
                #[allow(clippy::mutable_key_type)]
                let mut kept: HashSet<DeployChainIndex> = HashSet::new();
                for chain in branch.0 {
                    if descends_rejected(&chain.source_block_hash)? {
                        expanded += 1;
                        resolved.rejected.0.insert(chain);
                    } else {
                        kept.insert(chain);
                    }
                }
                if !kept.is_empty() {
                    new_to_merge.push(HashableSet(kept));
                }
            }
            resolved.to_merge = new_to_merge;
            if expanded > 0 {
                tracing::info!(
                    target: "f1r3fly.merge.step",
                    step = "merge.reject_stale_diff_descendants",
                    expanded_chains = expanded,
                    rejected_source_blocks = rejected_blocks.len(),
                    remaining_branches = resolved.to_merge.len(),
                    "rejection expanded over block lineage to prevent stale-diff application"
                );
            }
        }
    }

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

    // The user sigs whose chains survived every rejection pass and are
    // about to be APPLIED into the merged state. Returned to the caller:
    // these effects are in the merged pre-state, so a deploy among them
    // must not ALSO be executed fresh on top of it.
    let applied_user_sigs: HashSet<Bytes> = resolved
        .to_merge
        .iter()
        .flat_map(|branch| branch.0.iter())
        .flat_map(|chain| chain.deploys_with_cost.0.iter())
        .filter(|deploy| !is_system_deploy_id(&deploy.deploy_id))
        .map(|deploy| deploy.deploy_id.clone())
        .collect();

    // Channels where MORE THAN ONE accepted chain contributes datum adds:
    // only these can exhibit cross-writer accumulation, so only these are
    // subject to the base-empty single-value-cell overfill guard.
    let multi_writer_channels: HashSet<Blake2b256Hash> = {
        let mut writer_counts: HashMap<Blake2b256Hash, usize> = HashMap::new();
        for branch in &resolved.to_merge {
            for chain in &branch.0 {
                for entry in chain.state_changes.datums_changes.iter() {
                    if !entry.value().added.is_empty() {
                        *writer_counts.entry(entry.key().clone()).or_insert(0) += 1;
                    }
                }
            }
        }
        writer_counts
            .into_iter()
            .filter(|(_, writers)| *writers > 1)
            .map(|(channel, _)| channel)
            .collect()
    };
    let compute_trie_actions_fn = mk_compute_trie_actions_fn(multi_writer_channels);

    // Combine surviving diffs and apply to the LFB post-state.
    let new_state = conflict_set_merger::compute_merged_state(
        &resolved,
        &state_changes_fn,
        &mergeable_channels_fn,
        &compute_trie_actions_fn,
        &apply_trie_actions_fn,
    )
    .map_err(|e| {
        // A failure here is applied-diff incoherence: it repeats on every
        // propose over the same scope, so the shard stops producing blocks.
        // The error names a channel; everything needed to act on it — which
        // surviving chain carries the offending removal, where that chain came
        // from, and what the merge context was — is in scope RIGHT HERE and
        // was previously recoverable only by re-running under a debug stream
        // that produces gigabytes per minute. Emit it once, on the error path.
        explain_merge_failure(
            &e,
            &resolved,
            base,
            base_post_state,
            floor_block_number,
            &scope,
        );
        CasperError::HistoryError(e)
    })?;

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

    // Duplicate flag: the record does not dispute the sig's standing win
    // when the effect is present in THIS merge's own post-state — either a
    // kept chain in the same merge carries a copy, or the effect is
    // settled in the base. Both are frozen, validator-recomputable facts
    // of this merge; readers discard duplicate-flagged records from the
    // disposition ordering.
    let mut duplicate_of = |sig: &Bytes| -> Result<bool, CasperError> {
        if applied_user_sigs.contains(sig) || settled_sigs.contains(sig) {
            return Ok(true);
        }
        if settled_checked.insert(sig.clone()) && sig_settled_in_base(sig)? {
            settled_sigs.insert(sig.clone());
            return Ok(true);
        }
        Ok(false)
    };

    let mut rejected_user_deploys: Vec<RejectedDeploy> = Vec::new();
    for (id, src) in &all_pairs {
        if !is_system_deploy_id(id) {
            rejected_user_deploys.push(RejectedDeploy {
                sig: id.clone(),
                duplicate: duplicate_of(id)?,
                carrier: src.clone(),
            });
        }
    }
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
            .map(|record| record.sig.clone())
            .collect();
        for (id, src) in collateral_lost_pairs {
            if !existing_ids.contains(&id) {
                let duplicate = duplicate_of(&id)?;
                rejected_user_deploys.push(RejectedDeploy {
                    sig: id,
                    duplicate,
                    carrier: src,
                });
            }
        }
    }

    // Deterministic ordering across validators.
    rejected_user_deploys.sort();
    rejected_slashes.sort();

    tracing::debug!(
        "DagMerger.merge: base={}, scope={}, actual={}, late={}, rejected_user={}, rejected_slash={}",
        hex::encode(&base[..std::cmp::min(8, base.len())]),
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
            .map(|record| {
                format!(
                    "{}{}",
                    hex::encode(&record.sig[..std::cmp::min(8, record.sig.len())]),
                    if record.duplicate { "(dup)" } else { "" }
                )
            })
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

    Ok((
        new_state,
        rejected_user_deploys,
        rejected_slashes,
        applied_user_sigs,
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use dashmap::DashMap;
    use rholang::rust::interpreter::rho_type::RhoNumber;
    use rspace_plus_plus::rspace::hashing::stable_hash_provider;
    use rspace_plus_plus::rspace::merger::channel_change::ChannelChange;
    use rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex;
    use rspace_plus_plus::rspace::merger::state_change::StateChange;
    use rspace_plus_plus::rspace::serializers::serializers;
    use rspace_plus_plus::rspace::trace::event::Produce;

    use super::*;
    use crate::rust::merging::deploy_chain_index::{DeployChainIndex, DeployIdWithCost};

    fn datum_change(
        channel: Blake2b256Hash,
        removed: Vec<Vec<u8>>,
        added: Vec<Vec<u8>>,
    ) -> StateChange {
        let datums_changes = DashMap::new();
        datums_changes.insert(channel, ChannelChange { added, removed });
        StateChange {
            datums_changes,
            cont_changes: DashMap::new(),
            consume_channels_to_join_serialized_map: DashMap::new(),
        }
    }

    fn chain(
        deploy_id: u8,
        cost: u64,
        source_block_number: i64,
        state_changes: StateChange,
    ) -> DeployChainIndex {
        chain_with_event_log(
            deploy_id,
            cost,
            source_block_number,
            state_changes,
            EventLogIndex::empty(),
        )
    }

    fn chain_with_event_log(
        deploy_id: u8,
        cost: u64,
        source_block_number: i64,
        state_changes: StateChange,
        event_log_index: EventLogIndex,
    ) -> DeployChainIndex {
        let deploys_with_cost = HashableSet(HashSet::from([DeployIdWithCost {
            deploy_id: Bytes::from(vec![deploy_id]),
            cost,
        }]));
        DeployChainIndex::from_parts(
            deploys_with_cost,
            Blake2b256Hash::from_bytes(vec![deploy_id; 32]),
            event_log_index,
            state_changes,
            Bytes::from(vec![deploy_id; 32]),
            source_block_number,
        )
    }

    fn chain_with_window(
        deploy_id: u8,
        cost: u64,
        source_block_number: i64,
        valid_after: i64,
        state_changes: StateChange,
    ) -> DeployChainIndex {
        let mut c = chain(deploy_id, cost, source_block_number, state_changes);
        c.deploy_windows =
            std::collections::HashMap::from([(Bytes::from(vec![deploy_id]), valid_after)]);
        c
    }

    /// The window rule keys on `valid_after <= floor - lifespan` — the
    /// proposer's block-expired bound evaluated at the merging block's
    /// FLOOR (never the merge height: an in-scope chain of ordinary
    /// standing must be unreachable by this rule, which the floor key
    /// guarantees arithmetically). Closed-window chains are split out
    /// for rejection-with-record; the boundary (valid_after == floor -
    /// lifespan) is CLOSED (matches the selection filter); window-less
    /// (system-only) chains are exempt.
    #[test]
    fn window_rule_splits_closed_window_chains_only() {
        let closed = chain_with_window(1, 10, 3, 0, StateChange::empty());
        let at_boundary = chain_with_window(2, 10, 3, 5, StateChange::empty());
        let open = chain_with_window(3, 10, 3, 6, StateChange::empty());
        let system_only = {
            let mut c = chain(4, 10, 3, StateChange::empty());
            c.deploy_windows.clear();
            c
        };

        // floor #55, lifespan 50 → earliest_valid_after = 5.
        let (kept, rejected) = split_window_closed_chains(
            vec![
                closed.clone(),
                at_boundary.clone(),
                open.clone(),
                system_only.clone(),
            ],
            55,
            50,
        );

        let kept_ids: Vec<u8> = kept
            .iter()
            .flat_map(|c| c.deploys_with_cost.0.iter())
            .map(|d| d.deploy_id[0])
            .collect();
        let rejected_ids: Vec<u8> = rejected
            .iter()
            .flat_map(|c| c.deploys_with_cost.0.iter())
            .map(|d| d.deploy_id[0])
            .collect();

        assert!(
            rejected_ids.contains(&1) && rejected_ids.contains(&2),
            "closed and boundary windows must be split out (got rejected={:?})",
            rejected_ids
        );
        assert!(
            kept_ids.contains(&3) && kept_ids.contains(&4),
            "open-window and window-less chains must be kept (got kept={:?})",
            kept_ids
        );
    }

    fn mergeable_chain(
        deploy_id: u8,
        cost: u64,
        source_block_number: i64,
        channel: Blake2b256Hash,
        state_changes: StateChange,
    ) -> DeployChainIndex {
        let mut event_log = EventLogIndex::empty();
        event_log
            .number_channels_data
            .insert(channel, (0, merging_logic::MergeType::IntegerAdd));
        chain_with_event_log(
            deploy_id,
            cost,
            source_block_number,
            state_changes,
            event_log,
        )
    }

    fn encoded_number(channel_hash: &Blake2b256Hash, num: i64) -> Vec<u8> {
        let rnd = Blake2b512Random::create_from_bytes(&[num as u8; 32]);
        let par_with_rnd = ListParWithRandom {
            pars: vec![RhoNumber::create_par(num)],
            random_state: rnd.to_bytes(),
        };
        let data_hash =
            stable_hash_provider::hash_produce(channel_hash.bytes(), &par_with_rnd, false);
        let produce = Produce {
            channel_hash: channel_hash.clone(),
            hash: data_hash,
            persistent: false,
            is_deterministic: true,
            output_value: vec![],
            failed: false,
        };
        serializers::encode_datum(&Datum {
            a: par_with_rnd,
            persist: false,
            source: produce,
        })
    }

    fn split_branch(
        branch: HashableSet<DeployChainIndex>,
        base_channel: Blake2b256Hash,
        base_data_value: Vec<u8>,
        depends: impl Fn(&DeployChainIndex, &DeployChainIndex) -> bool,
    ) -> (
        Option<HashableSet<DeployChainIndex>>,
        HashableSet<DeployChainIndex>,
    ) {
        split_branch_with_base(branch, base_channel, vec![base_data_value], depends)
    }

    fn split_branch_with_base(
        branch: HashableSet<DeployChainIndex>,
        base_channel: Blake2b256Hash,
        base_data_values: Vec<Vec<u8>>,
        depends: impl Fn(&DeployChainIndex, &DeployChainIndex) -> bool,
    ) -> (
        Option<HashableSet<DeployChainIndex>>,
        HashableSet<DeployChainIndex>,
    ) {
        split_unavailable_branch_consumes(
            branch,
            &depends,
            &|chain| Ok(chain.state_changes.clone()),
            &|chain| chain.event_log_index.number_channels_data.clone(),
            &|channel| {
                if channel == &base_channel {
                    Ok(base_data_values.clone())
                } else {
                    Ok(Vec::new())
                }
            },
            &|_| Ok(Vec::new()),
        )
        .expect("split unavailable branch consumes")
    }

    #[test]
    fn unavailable_floor_consumes_reject_lower_priority_same_cell_writer() {
        let channel = Blake2b256Hash::from_bytes(vec![0x11; 32]);
        let datum_a = vec![0xaa; 32];
        let datum_b = vec![0xbb; 32];
        let datum_c = vec![0xcc; 32];
        let high_cost = chain(
            1,
            10,
            1,
            datum_change(channel.clone(), vec![datum_a.clone()], vec![datum_b]),
        );
        let low_cost = chain(
            2,
            1,
            1,
            datum_change(channel.clone(), vec![datum_a.clone()], vec![datum_c]),
        );
        let branch = HashableSet(HashSet::from([high_cost.clone(), low_cost.clone()]));

        let (kept, rejected) = split_branch(branch, channel, datum_a, |_, _| false);

        let kept = kept.expect("one writer must survive");
        assert!(kept.0.contains(&high_cost));
        assert!(!kept.0.contains(&low_cost));
        assert!(rejected.0.contains(&low_cost));
        assert!(!rejected.0.contains(&high_cost));
    }

    #[test]
    fn unavailable_floor_consumes_reject_produce_only_single_value_overfill() {
        let channel = Blake2b256Hash::from_bytes(vec![0x12; 32]);
        let base = encoded_number(&channel, 0);
        let added_a = encoded_number(&channel, 1);
        let added_b = encoded_number(&channel, 2);
        let first = chain(
            1,
            10,
            1,
            datum_change(channel.clone(), Vec::new(), vec![added_a]),
        );
        let second = chain(
            2,
            1,
            1,
            datum_change(channel.clone(), Vec::new(), vec![added_b]),
        );
        let branch = HashableSet(HashSet::from([first.clone(), second.clone()]));

        let (kept, rejected) = split_branch(branch, channel, base, |_, _| false);

        assert!(kept.is_none());
        assert!(rejected.0.contains(&first));
        assert!(rejected.0.contains(&second));
    }

    #[test]
    fn unavailable_floor_consumes_keep_one_empty_numeric_cell_creator() {
        let channel = Blake2b256Hash::from_bytes(vec![0x14; 32]);
        let added_a = encoded_number(&channel, 0);
        let added_b = encoded_number(&channel, 0);
        let first = chain(
            1,
            10,
            1,
            datum_change(channel.clone(), Vec::new(), vec![added_a]),
        );
        let second = chain(
            2,
            1,
            1,
            datum_change(channel.clone(), Vec::new(), vec![added_b]),
        );
        let branch = HashableSet(HashSet::from([first.clone(), second.clone()]));

        let (kept, rejected) = split_branch_with_base(branch, channel, Vec::new(), |_, _| false);

        let kept = kept.expect("one numeric creator must survive");
        assert_eq!(kept.0.len(), 1);
        assert_eq!(rejected.0.len(), 1);
        assert_ne!(
            kept.0.iter().next().unwrap().source_block_hash,
            rejected.0.iter().next().unwrap().source_block_hash
        );
    }

    #[test]
    fn overfilled_splitter_keep_one_empty_numeric_cell_creator() {
        let channel = Blake2b256Hash::from_bytes(vec![0x15; 32]);
        let added_a = encoded_number(&channel, 0);
        let added_b = encoded_number(&channel, 0);
        let first = chain(
            1,
            10,
            1,
            datum_change(channel.clone(), Vec::new(), vec![added_a]),
        );
        let second = chain(
            2,
            1,
            1,
            datum_change(channel.clone(), Vec::new(), vec![added_b]),
        );
        let mut resolved = conflict_set_merger::ResolvedConflicts {
            to_merge: vec![
                HashableSet(HashSet::from([first.clone()])),
                HashableSet(HashSet::from([second.clone()])),
            ],
            rejected: HashableSet(HashSet::new()),
            late_set_size: 0,
            actual_set_size: 2,
            branches_count: 2,
            rejected_as_dependents_count: 0,
            optimal_rejection_count: 0,
            conflict_map_conflicts_count: 0,
            rejection_options_count: 0,
            branches_time: std::time::Duration::ZERO,
            conflicts_map_time: std::time::Duration::ZERO,
            rejection_options_time: std::time::Duration::ZERO,
        };

        let rejected = split_overfilled_single_value_cells(
            &mut resolved,
            &|_, _| false,
            &|_| BTreeMap::new(),
            &|_| Ok(Vec::new()),
            &|_| Ok(Vec::new()),
            &HashSet::new(),
        )
        .expect("split overfilled cells");

        assert_eq!(rejected.0.len(), 1);
        assert_eq!(resolved.to_merge.len(), 1);
        assert_eq!(resolved.to_merge[0].0.len(), 1);
    }

    /// Issue #294 (B1): a chain that already lost prior merges — its deploys
    /// carry kept rejection records in the scope — must win keep-one against
    /// an otherwise-equal contender. Without loss-aware adjudication the
    /// content ordering alone decides, the same chain loses every merge, and
    /// recovery re-proposes it into the same loss until its window closes.
    #[test]
    fn overfilled_splitter_prefers_previously_rejected_contender() {
        let channel = Blake2b256Hash::from_bytes(vec![0x18; 32]);
        let added_fresh = encoded_number(&channel, 0);
        let added_veteran = encoded_number(&channel, 0);
        // Equal cost; the fresh contender's deploy id is lex-smaller, so the
        // content ordering alone keeps it. The veteran carries one prior loss.
        let fresh = chain(
            1,
            10,
            1,
            datum_change(channel.clone(), Vec::new(), vec![added_fresh]),
        );
        let mut veteran = chain(
            2,
            10,
            1,
            datum_change(channel.clone(), Vec::new(), vec![added_veteran]),
        );
        veteran.prior_rejections = 1;
        let mut resolved = conflict_set_merger::ResolvedConflicts {
            to_merge: vec![
                HashableSet(HashSet::from([fresh.clone()])),
                HashableSet(HashSet::from([veteran.clone()])),
            ],
            rejected: HashableSet(HashSet::new()),
            late_set_size: 0,
            actual_set_size: 2,
            branches_count: 2,
            rejected_as_dependents_count: 0,
            optimal_rejection_count: 0,
            conflict_map_conflicts_count: 0,
            rejection_options_count: 0,
            branches_time: std::time::Duration::ZERO,
            conflicts_map_time: std::time::Duration::ZERO,
            rejection_options_time: std::time::Duration::ZERO,
        };

        let rejected = split_overfilled_single_value_cells(
            &mut resolved,
            &|_, _| false,
            &|_| BTreeMap::new(),
            &|_| Ok(Vec::new()),
            &|_| Ok(Vec::new()),
            &HashSet::new(),
        )
        .expect("split overfilled cells");

        assert!(
            rejected.0.contains(&fresh),
            "the contender with no prior losses must be the one rejected"
        );
        assert_eq!(resolved.to_merge.len(), 1);
        assert!(
            resolved.to_merge[0].0.contains(&veteran),
            "the previously rejected chain must survive keep-one"
        );
    }

    /// Issue #294 (B2): equal prior-rejection counts must leave adjudication
    /// exactly where the content ordering puts it — loss-awareness is a
    /// tie-breaker on losses, never a perturbation of the deterministic
    /// baseline every validator must reproduce.
    #[test]
    fn overfilled_splitter_equal_prior_losses_keep_content_order() {
        let channel = Blake2b256Hash::from_bytes(vec![0x19; 32]);
        let added_a = encoded_number(&channel, 0);
        let added_b = encoded_number(&channel, 0);
        let mut first = chain(
            1,
            10,
            1,
            datum_change(channel.clone(), Vec::new(), vec![added_a]),
        );
        let mut second = chain(
            2,
            10,
            1,
            datum_change(channel.clone(), Vec::new(), vec![added_b]),
        );
        first.prior_rejections = 1;
        second.prior_rejections = 1;
        let mut resolved = conflict_set_merger::ResolvedConflicts {
            to_merge: vec![
                HashableSet(HashSet::from([first.clone()])),
                HashableSet(HashSet::from([second.clone()])),
            ],
            rejected: HashableSet(HashSet::new()),
            late_set_size: 0,
            actual_set_size: 2,
            branches_count: 2,
            rejected_as_dependents_count: 0,
            optimal_rejection_count: 0,
            conflict_map_conflicts_count: 0,
            rejection_options_count: 0,
            branches_time: std::time::Duration::ZERO,
            conflicts_map_time: std::time::Duration::ZERO,
            rejection_options_time: std::time::Duration::ZERO,
        };

        let rejected = split_overfilled_single_value_cells(
            &mut resolved,
            &|_, _| false,
            &|_| BTreeMap::new(),
            &|_| Ok(Vec::new()),
            &|_| Ok(Vec::new()),
            &HashSet::new(),
        )
        .expect("split overfilled cells");

        assert!(
            rejected.0.contains(&second),
            "with equal prior losses the content ordering must still decide"
        );
        assert_eq!(resolved.to_merge.len(), 1);
        assert!(
            resolved.to_merge[0].0.contains(&first),
            "the content-order winner must survive when losses are equal"
        );
    }

    /// Issue #294 (B3): the losing-every-merge composition. One deploy meets
    /// a FRESH equal-cost contender on the same key every round; each round's
    /// loser leaves a rejection record in the scope, exactly as the merge
    /// records rejections in block bodies. A deploy the content ordering
    /// always ranks last must still land before `deploy_lifespan` rounds
    /// close its validity window — otherwise it terminates Expired with no
    /// error surfaced, which is the defect.
    #[test]
    fn sustained_same_key_contention_lands_before_window_closes() {
        let channel = Blake2b256Hash::from_bytes(vec![0x1a; 32]);
        // Lex-largest deploy id: the content ordering alone rejects it in
        // every round's tie.
        let starved_id: u8 = 0xff;
        let lifespan_rounds: u8 = 50;
        let mut scope_records: Vec<RejectedDeploy> = Vec::new();
        let mut landed_round: Option<u8> = None;

        for round in 1..=lifespan_rounds {
            let added_fresh = encoded_number(&channel, 0);
            let added_starved = encoded_number(&channel, 0);
            let mut contenders = [
                chain(
                    round,
                    10,
                    1,
                    datum_change(channel.clone(), Vec::new(), vec![added_fresh]),
                ),
                chain(
                    starved_id,
                    10,
                    1,
                    datum_change(channel.clone(), Vec::new(), vec![added_starved]),
                ),
            ];
            let counts = prior_rejection_counts(scope_records.iter());
            stamp_prior_rejections(&mut contenders, &counts);
            let [fresh, starved] = contenders;

            let mut resolved = conflict_set_merger::ResolvedConflicts {
                to_merge: vec![
                    HashableSet(HashSet::from([fresh.clone()])),
                    HashableSet(HashSet::from([starved.clone()])),
                ],
                rejected: HashableSet(HashSet::new()),
                late_set_size: 0,
                actual_set_size: 2,
                branches_count: 2,
                rejected_as_dependents_count: 0,
                optimal_rejection_count: 0,
                conflict_map_conflicts_count: 0,
                rejection_options_count: 0,
                branches_time: std::time::Duration::ZERO,
                conflicts_map_time: std::time::Duration::ZERO,
                rejection_options_time: std::time::Duration::ZERO,
            };
            let rejected = split_overfilled_single_value_cells(
                &mut resolved,
                &|_, _| false,
                &|_| BTreeMap::new(),
                &|_| Ok(Vec::new()),
                &|_| Ok(Vec::new()),
                &HashSet::new(),
            )
            .expect("split overfilled cells");

            if !rejected.0.contains(&starved) {
                landed_round = Some(round);
                break;
            }
            scope_records.push(RejectedDeploy {
                sig: Bytes::from(vec![starved_id]),
                duplicate: false,
                carrier: Bytes::from(vec![round; 32]),
            });
        }

        assert!(
            landed_round.is_some(),
            "starved deploy lost every merge for {} rounds and expired",
            lifespan_rounds
        );
    }

    /// Issue #294 (B5): a merge's counts come from the records of every block
    /// it can see — scope blocks AND the base-lineage window (the retry gate
    /// settles a rejection below the floor before the retry, so the record
    /// that must raise priority is on the base's lineage). Duplicate-flagged
    /// records do not dispute a standing win and must not count.
    #[test]
    fn scope_counts_aggregate_kept_records_across_visible_blocks() {
        let sig = Bytes::from(vec![0x2a]);
        let scope_block: BlockHash = Bytes::from(vec![1u8; 32]);
        let lineage_block: BlockHash = Bytes::from(vec![2u8; 32]);
        let records_of = |hash: &BlockHash| -> Result<Vec<RejectedDeploy>, CasperError> {
            if *hash == scope_block {
                Ok(vec![RejectedDeploy {
                    sig: sig.clone(),
                    duplicate: false,
                    carrier: Bytes::from(vec![3u8; 32]),
                }])
            } else {
                Ok(vec![
                    RejectedDeploy {
                        sig: sig.clone(),
                        duplicate: false,
                        carrier: Bytes::from(vec![4u8; 32]),
                    },
                    RejectedDeploy {
                        sig: sig.clone(),
                        duplicate: true,
                        carrier: Bytes::from(vec![5u8; 32]),
                    },
                ])
            }
        };

        let counts = scope_prior_rejection_counts(
            vec![scope_block.clone(), lineage_block.clone()],
            records_of,
        )
        .expect("derive scope counts");

        assert_eq!(
            counts.get(&sig).copied(),
            Some(2),
            "one kept record per visible block must count; the duplicate must not"
        );
    }

    /// Issue #294 (B6): a visible block this node cannot supply must fail
    /// the derivation, not count as an empty history. The counts shape the
    /// rejection set peers validate, so a node that silently derived them
    /// from fewer blocks would propose (or reject) a different rejection
    /// set than every peer holding the full window.
    #[test]
    fn scope_counts_deduplicate_block_identifiers() {
        let sig = Bytes::from(vec![0x2a]);
        let block: BlockHash = Bytes::from(vec![1u8; 32]);
        let counts = scope_prior_rejection_counts(vec![block.clone(), block], |_hash| {
            Ok(vec![RejectedDeploy {
                sig: sig.clone(),
                duplicate: false,
                carrier: Bytes::from(vec![2u8; 32]),
            }])
        })
        .expect("derive scope counts");

        assert_eq!(counts.get(&sig).copied(), Some(1));
    }

    #[test]
    fn chain_priority_uses_maximum_member_loss() {
        let first = Bytes::from(vec![1u8]);
        let second = Bytes::from(vec![2u8]);
        let mut contender = chain(1, 1, 1, StateChange::empty());
        contender.deploys_with_cost.0.insert(DeployIdWithCost {
            deploy_id: second.clone(),
            cost: 1,
        });
        let counts = HashMap::from([(first, 3), (second, 5)]);

        stamp_prior_rejections(std::slice::from_mut(&mut contender), &counts);

        assert_eq!(contender.prior_rejections, 5);
    }

    #[test]
    fn scope_counts_fail_on_missing_visible_block() {
        let held: BlockHash = Bytes::from(vec![1u8; 32]);
        let missing: BlockHash = Bytes::from(vec![2u8; 32]);
        let records_of = |hash: &BlockHash| -> Result<Vec<RejectedDeploy>, CasperError> {
            if *hash == held {
                Ok(Vec::new())
            } else {
                Err(CasperError::BlockNotHeld(hash.clone()))
            }
        };

        let result = scope_prior_rejection_counts(vec![held.clone(), missing.clone()], records_of);

        match result {
            Err(CasperError::BlockNotHeld(hash)) => assert_eq!(hash, missing),
            other => panic!(
                "a missing visible block must surface as BlockNotHeld, got {:?}",
                other.map(|c| c.len())
            ),
        }
    }

    /// Issue #294 (B7): the adversarial bound of phase 1. A contender that
    /// arrives with a manufactured lead of N prior losses outranks a fresh
    /// victim on the same key — but only for N rounds. Every round the
    /// victim loses it gains one loss of its own, so the lead is consumed
    /// one merge at a time. With the content ordering also against the
    /// victim (the worst case, and the one round a lead-less rival already
    /// costs it), the victim lands at round N + 2: the lead buys exactly N
    /// extra merges of delay. The attacker cannot extend the delay without
    /// manufacturing fresh losses, each of which costs a charged winner on
    /// the same key.
    #[test]
    fn manufactured_loss_lead_delays_victim_by_exactly_lead_rounds() {
        let channel = Blake2b256Hash::from_bytes(vec![0x1b; 32]);
        // Lex-smaller id: the content ordering favors the attacker in a tie.
        let attacker_id: u8 = 0x01;
        let victim_id: u8 = 0x02;
        let lead: u64 = 3;
        let mut scope_records: Vec<RejectedDeploy> = (0..lead)
            .map(|i| RejectedDeploy {
                sig: Bytes::from(vec![attacker_id]),
                duplicate: false,
                carrier: Bytes::from(vec![0x80 + i as u8; 32]),
            })
            .collect();
        let mut victim_landed_round: Option<u64> = None;

        for round in 1..=(lead + 2) {
            let mut contenders = [
                chain(
                    attacker_id,
                    10,
                    1,
                    datum_change(channel.clone(), Vec::new(), vec![encoded_number(
                        &channel, 0,
                    )]),
                ),
                chain(
                    victim_id,
                    10,
                    1,
                    datum_change(channel.clone(), Vec::new(), vec![encoded_number(
                        &channel, 0,
                    )]),
                ),
            ];
            let counts = prior_rejection_counts(scope_records.iter());
            stamp_prior_rejections(&mut contenders, &counts);
            let [attacker, victim] = contenders;

            let mut resolved = conflict_set_merger::ResolvedConflicts {
                to_merge: vec![
                    HashableSet(HashSet::from([attacker.clone()])),
                    HashableSet(HashSet::from([victim.clone()])),
                ],
                rejected: HashableSet(HashSet::new()),
                late_set_size: 0,
                actual_set_size: 2,
                branches_count: 2,
                rejected_as_dependents_count: 0,
                optimal_rejection_count: 0,
                conflict_map_conflicts_count: 0,
                rejection_options_count: 0,
                branches_time: std::time::Duration::ZERO,
                conflicts_map_time: std::time::Duration::ZERO,
                rejection_options_time: std::time::Duration::ZERO,
            };
            let rejected = split_overfilled_single_value_cells(
                &mut resolved,
                &|_, _| false,
                &|_| BTreeMap::new(),
                &|_| Ok(Vec::new()),
                &|_| Ok(Vec::new()),
                &HashSet::new(),
            )
            .expect("split overfilled cells");

            if !rejected.0.contains(&victim) {
                victim_landed_round = Some(round);
                break;
            }
            assert!(
                round <= lead + 1,
                "the victim must lose no more than {} rounds against a lead of {}",
                lead + 1,
                lead
            );
            // The attacker's win is charged and lands; only the victim's loss
            // is recorded. The attacker re-enters the next round with its
            // original lead (a fresh deploy on the same key, same sig
            // history).
            scope_records.push(RejectedDeploy {
                sig: Bytes::from(vec![victim_id]),
                duplicate: false,
                carrier: Bytes::from(vec![round as u8; 32]),
            });
        }

        assert_eq!(
            victim_landed_round,
            Some(lead + 2),
            "a lead of {} must buy exactly {} extra merges of delay over the one tie round",
            lead,
            lead
        );
    }

    #[test]
    fn unavailable_split_rejects_untagged_touch_to_folded_number_channel() {
        let channel = Blake2b256Hash::from_bytes(vec![0x16; 32]);
        let tagged_added = encoded_number(&channel, 0);
        let untagged_added = encoded_number(&channel, 0);
        let tagged = mergeable_chain(
            1,
            10,
            1,
            channel.clone(),
            datum_change(channel.clone(), Vec::new(), vec![tagged_added]),
        );
        let untagged = chain(
            2,
            1,
            1,
            datum_change(channel.clone(), Vec::new(), vec![untagged_added]),
        );
        let branch = HashableSet(HashSet::from([tagged.clone(), untagged.clone()]));

        let (kept, rejected) = split_branch_with_base(branch, channel, Vec::new(), |_, _| false);

        let kept = kept.expect("tagged writer must survive");
        assert!(kept.0.contains(&tagged));
        assert!(!kept.0.contains(&untagged));
        assert!(rejected.0.contains(&untagged));
        assert!(!rejected.0.contains(&tagged));
    }

    #[test]
    fn overfilled_splitter_rejects_untagged_touch_to_folded_number_channel() {
        let channel = Blake2b256Hash::from_bytes(vec![0x17; 32]);
        let tagged_added = encoded_number(&channel, 0);
        let untagged_added = encoded_number(&channel, 0);
        let tagged = mergeable_chain(
            1,
            10,
            1,
            channel.clone(),
            datum_change(channel.clone(), Vec::new(), vec![tagged_added]),
        );
        let untagged = chain(
            2,
            1,
            1,
            datum_change(channel.clone(), Vec::new(), vec![untagged_added]),
        );
        let mut resolved = conflict_set_merger::ResolvedConflicts {
            to_merge: vec![
                HashableSet(HashSet::from([tagged.clone()])),
                HashableSet(HashSet::from([untagged.clone()])),
            ],
            rejected: HashableSet(HashSet::new()),
            late_set_size: 0,
            actual_set_size: 2,
            branches_count: 2,
            rejected_as_dependents_count: 0,
            optimal_rejection_count: 0,
            conflict_map_conflicts_count: 0,
            rejection_options_count: 0,
            branches_time: std::time::Duration::ZERO,
            conflicts_map_time: std::time::Duration::ZERO,
            rejection_options_time: std::time::Duration::ZERO,
        };

        let rejected = split_overfilled_single_value_cells(
            &mut resolved,
            &|_, _| false,
            &|chain| chain.event_log_index.number_channels_data.clone(),
            &|_| Ok(Vec::new()),
            &|_| Ok(Vec::new()),
            &HashSet::new(),
        )
        .expect("split mixed folded cells");

        assert!(rejected.0.contains(&untagged));
        assert!(!rejected.0.contains(&tagged));
        assert_eq!(resolved.to_merge.len(), 1);
        assert!(resolved.to_merge[0].0.contains(&tagged));
    }

    #[test]
    fn unavailable_floor_consumes_reject_internal_single_value_overfill() {
        let channel = Blake2b256Hash::from_bytes(vec![0x13; 32]);
        let base = encoded_number(&channel, 0);
        let added_a = encoded_number(&channel, 1);
        let added_b = encoded_number(&channel, 2);
        let overfilled = chain(
            1,
            10,
            1,
            datum_change(channel.clone(), vec![base.clone()], vec![added_a, added_b]),
        );
        let branch = HashableSet(HashSet::from([overfilled.clone()]));

        let (kept, rejected) = split_branch(branch, channel, base, |_, _| false);

        assert!(kept.is_none());
        assert!(rejected.0.contains(&overfilled));
    }

    #[test]
    fn unavailable_floor_consumes_allow_dependent_chain_intermediate() {
        let channel = Blake2b256Hash::from_bytes(vec![0x22; 32]);
        let datum_a = vec![0xaa; 32];
        let datum_b = vec![0xbb; 32];
        let datum_c = vec![0xcc; 32];
        let first = chain(
            1,
            1,
            1,
            datum_change(
                channel.clone(),
                vec![datum_a.clone()],
                vec![datum_b.clone()],
            ),
        );
        let second = chain(
            2,
            10,
            2,
            datum_change(channel.clone(), vec![datum_b], vec![datum_c]),
        );
        let branch = HashableSet(HashSet::from([first.clone(), second.clone()]));

        let (kept, rejected) = split_branch(branch, channel, datum_a, |target, source| {
            target == &second && source == &first
        });

        let kept = kept.expect("dependent chain must survive");
        assert!(kept.0.contains(&first));
        assert!(kept.0.contains(&second));
        assert!(rejected.0.is_empty());
    }

    #[test]
    fn unavailable_floor_consumes_reject_dependents_of_rejected_chain() {
        let channel = Blake2b256Hash::from_bytes(vec![0x33; 32]);
        let datum_a = vec![0xaa; 32];
        let datum_b = vec![0xbb; 32];
        let datum_c = vec![0xcc; 32];
        let datum_d = vec![0xdd; 32];
        let high_cost = chain(
            1,
            10,
            1,
            datum_change(channel.clone(), vec![datum_a.clone()], vec![datum_d]),
        );
        let losing_source = chain(
            2,
            1,
            1,
            datum_change(
                channel.clone(),
                vec![datum_a.clone()],
                vec![datum_b.clone()],
            ),
        );
        let dependent = chain(
            3,
            20,
            2,
            datum_change(channel.clone(), vec![datum_b], vec![datum_c]),
        );
        let branch = HashableSet(HashSet::from([
            high_cost.clone(),
            losing_source.clone(),
            dependent.clone(),
        ]));

        let (kept, rejected) = split_branch(branch, channel, datum_a, |target, source| {
            target == &dependent && source == &losing_source
        });

        let kept = kept.expect("high-cost writer must survive");
        assert!(kept.0.contains(&high_cost));
        assert!(!kept.0.contains(&losing_source));
        assert!(!kept.0.contains(&dependent));
        assert!(rejected.0.contains(&losing_source));
        assert!(rejected.0.contains(&dependent));
    }

    /// A merge must never adjudicate away content its own MAIN PARENT already
    /// holds. Cost-optimal rejection is a phlo sum with no notion of
    /// provenance, so whenever a main-parent chain is the cheaper side of a
    /// conflict it is the one dropped — which is exactly what block #537 did to
    /// #536's deploy `304402206624dd87` on shard `bc35a3ad`, while #536 was
    /// #537's own `parents[0]`. The result is a block whose state omits its
    /// spine ancestor's content: the oracle certifies #536 because every latest
    /// message descends from it, and no live state holds it.
    ///
    /// Here the main parent's chain is deliberately the CHEAPER side, so an
    /// unpinned selection rejects it.
    ///
    /// Pins a path the node no longer takes: production passes an empty
    /// `pinned` set because the base is the main parent, which keeps its
    /// content out of the conflict set entirely. Kept until the pinning
    /// machinery is removed or restored.
    #[test]
    fn conflict_resolution_never_rejects_main_parent_content() {
        let channel = Blake2b256Hash::from_bytes(vec![0x55; 32]);
        // Already committed in the main parent's state; cheap.
        let main_parent_chain = chain(
            1,
            5,
            1,
            datum_change(channel.clone(), vec![vec![0x01; 32]], vec![vec![0x02; 32]]),
        );
        // A rival from another parent; expensive, so cost prefers to keep it.
        let rival = chain(
            2,
            500,
            1,
            datum_change(channel.clone(), vec![vec![0x01; 32]], vec![vec![0x03; 32]]),
        );

        let compute_branches = |merge_set: &HashableSet<DeployChainIndex>| {
            HashableSet(
                merge_set
                    .0
                    .iter()
                    .map(|chain| HashableSet(HashSet::from([chain.clone()])))
                    .collect(),
            )
        };
        // The two branches conflict: both consume the same base datum.
        let compute_conflict_map = |branches: &HashableSet<HashableSet<DeployChainIndex>>| {
            #[allow(clippy::mutable_key_type)]
            let mut map = HashMap::new();
            let all: Vec<HashableSet<DeployChainIndex>> = branches.0.iter().cloned().collect();
            for branch in &all {
                #[allow(clippy::mutable_key_type)]
                let others: HashSet<HashableSet<DeployChainIndex>> = all
                    .iter()
                    .filter(|other| *other != branch)
                    .cloned()
                    .collect();
                map.insert(branch.clone(), HashableSet(others));
            }
            Ok(map)
        };

        #[allow(clippy::mutable_key_type)]
        let pinned: HashSet<DeployChainIndex> = HashSet::from([main_parent_chain.clone()]);

        let resolved = conflict_set_merger::resolve_conflicts(
            vec![main_parent_chain.clone(), rival.clone()],
            Vec::new(),
            &|_, _| false,
            &cost_optimal_rejection_alg(),
            &|chain: &DeployChainIndex| chain.prior_rejections,
            &|_| BTreeMap::new(),
            &|_| Ok(Vec::new()),
            &compute_branches,
            &compute_conflict_map,
            &pinned,
        )
        .expect("conflict resolution");

        assert!(
            !resolved.rejected.0.contains(&main_parent_chain),
            "the main parent's own chain was rejected — its content is committed in \
             the state this block extends, so dropping it makes the block's state fail \
             to contain its spine ancestor's (the bc35a3ad fork)"
        );
        assert!(
            resolved.rejected.0.contains(&rival),
            "the conflict must still be resolved, by rejecting the side the main \
             parent does not already hold"
        );
    }

    /// Pinning must never cost liveness. Two chains the main parent applied in
    /// SEQUENCE can read as conflicting when a later merge re-applies them side
    /// by side from the floor: `conflicts` check #2 pairs a surviving produce
    /// against a surviving consume on the same channel without comparing
    /// patterns, so a pair that never actually COMM'd in the main parent's own
    /// history still registers. Both are pinned, so every rejection option
    /// touches pinned content.
    ///
    /// Refusing the merge there would convert a rare state-containment residual
    /// into a certain propose wedge — strictly the worse failure, and the one
    /// class this whole effort exists to remove. Resolution must therefore
    /// complete, logging the fallback, and leave the floor's containment guard
    /// to catch the residual loudly.
    ///
    /// Unreachable from the node for the same reason as
    /// `conflict_resolution_never_rejects_main_parent_content`: production
    /// pins nothing.
    #[test]
    fn pinning_falls_back_rather_than_wedging_when_pinned_chains_conflict() {
        let channel = Blake2b256Hash::from_bytes(vec![0x66; 32]);
        let first = chain(
            1,
            5,
            1,
            datum_change(channel.clone(), vec![vec![0x01; 32]], vec![vec![0x02; 32]]),
        );
        let second = chain(
            2,
            7,
            1,
            datum_change(channel.clone(), vec![vec![0x01; 32]], vec![vec![0x03; 32]]),
        );

        let compute_branches = |merge_set: &HashableSet<DeployChainIndex>| {
            HashableSet(
                merge_set
                    .0
                    .iter()
                    .map(|chain| HashableSet(HashSet::from([chain.clone()])))
                    .collect(),
            )
        };
        let compute_conflict_map = |branches: &HashableSet<HashableSet<DeployChainIndex>>| {
            #[allow(clippy::mutable_key_type)]
            let mut map = HashMap::new();
            let all: Vec<HashableSet<DeployChainIndex>> = branches.0.iter().cloned().collect();
            for branch in &all {
                #[allow(clippy::mutable_key_type)]
                let others: HashSet<HashableSet<DeployChainIndex>> = all
                    .iter()
                    .filter(|other| *other != branch)
                    .cloned()
                    .collect();
                map.insert(branch.clone(), HashableSet(others));
            }
            Ok(map)
        };

        // BOTH sides pinned: no rejection option leaves the pinned set intact.
        #[allow(clippy::mutable_key_type)]
        let pinned: HashSet<DeployChainIndex> = HashSet::from([first.clone(), second.clone()]);

        let resolved = conflict_set_merger::resolve_conflicts(
            vec![first.clone(), second.clone()],
            Vec::new(),
            &|_, _| false,
            &cost_optimal_rejection_alg(),
            &|chain: &DeployChainIndex| chain.prior_rejections,
            &|_| BTreeMap::new(),
            &|_| Ok(Vec::new()),
            &compute_branches,
            &compute_conflict_map,
            &pinned,
        )
        .expect("resolution must still succeed — refusing here is a propose wedge");

        assert_eq!(
            resolved.rejected.0.len(),
            1,
            "the conflict is still resolved by rejecting exactly one side; pinning \
             expresses a preference, never a veto that can strand the proposer"
        );
    }

    /// Conflict resolution is not the only place main-parent content can be
    /// dropped: §3c keep-one picks the survivor for an over-filled single-value
    /// cell by CHAIN ORDER, which knows nothing about provenance. Two
    /// validators racing an RMW on one cell is the ucc suite's core shape, so
    /// a main-parent writer landing later in the order is ordinary, not exotic
    /// — and dropping it leaves the block's state missing content its own
    /// spine ancestor holds, the same defect by a different route.
    ///
    /// Also unreachable from the node: `split_overfilled_single_value_cells`
    /// is called with an empty `pinned` set, so the ordering preference this
    /// asserts never applies in production.
    #[test]
    fn overfill_keep_one_prefers_main_parent_content() {
        let counter = Blake2b256Hash::from_bytes(vec![0x71; 32]);
        // Sorts FIRST, so unpinned keep-one would retain this one.
        let rival = chain(
            0,
            10,
            1,
            datum_change(counter.clone(), Vec::new(), vec![encoded_number(
                &counter, 3,
            )]),
        );
        // Sorts LAST, but its effect is already committed in the main parent.
        let main_parent_chain = chain(
            9,
            10,
            1,
            datum_change(counter.clone(), Vec::new(), vec![encoded_number(
                &counter, 4,
            )]),
        );

        let mut resolved = conflict_set_merger::ResolvedConflicts {
            to_merge: vec![
                HashableSet(HashSet::from([rival.clone()])),
                HashableSet(HashSet::from([main_parent_chain.clone()])),
            ],
            rejected: HashableSet(HashSet::new()),
            late_set_size: 0,
            actual_set_size: 2,
            branches_count: 2,
            rejected_as_dependents_count: 0,
            optimal_rejection_count: 0,
            conflict_map_conflicts_count: 0,
            rejection_options_count: 0,
            branches_time: std::time::Duration::ZERO,
            conflicts_map_time: std::time::Duration::ZERO,
            rejection_options_time: std::time::Duration::ZERO,
        };

        #[allow(clippy::mutable_key_type)]
        let pinned: HashSet<DeployChainIndex> = HashSet::from([main_parent_chain.clone()]);

        let rejected = split_overfilled_single_value_cells(
            &mut resolved,
            &|_, _| false,
            &|_| BTreeMap::new(),
            &|_| Ok(Vec::new()),
            &|_| Ok(Vec::new()),
            &pinned,
        )
        .expect("overfill split");

        assert!(
            !rejected.0.contains(&main_parent_chain),
            "keep-one dropped the writer whose effect the main parent already \
             committed, purely because it sorts later"
        );
        assert!(
            rejected.0.contains(&rival),
            "the cell must still keep exactly one writer — the preference \
             reorders the choice, it does not suppress the keep-one"
        );
    }

    #[test]
    fn unavailable_retry_reconsiders_conflicts_after_winning_branch_is_removed() {
        let channel = Blake2b256Hash::from_bytes(vec![0x44; 32]);
        let unavailable_winner = chain(
            1,
            10,
            1,
            datum_change(channel.clone(), vec![vec![0xaa; 32]], vec![vec![0xbb; 32]]),
        );
        let rescued_branch = chain(2, 1, 1, StateChange::empty());
        let actual_seq_all = vec![unavailable_winner.clone(), rescued_branch.clone()];
        let late_seq_all = Vec::new();

        let compute_branches = |merge_set: &HashableSet<DeployChainIndex>| {
            HashableSet(
                merge_set
                    .0
                    .iter()
                    .map(|chain| HashableSet(HashSet::from([chain.clone()])))
                    .collect(),
            )
        };
        let compute_conflict_map = |branches: &HashableSet<HashableSet<DeployChainIndex>>| {
            // False positive: DeployChainIndex's Hash/Eq use only immutable fields.
            #[allow(clippy::mutable_key_type)]
            let mut map: HashMap<
                HashableSet<DeployChainIndex>,
                HashableSet<HashableSet<DeployChainIndex>>,
            > = branches
                .0
                .iter()
                .map(|branch| (branch.clone(), HashableSet(HashSet::new())))
                .collect();
            let winner_branch = branches
                .0
                .iter()
                .find(|branch| branch.0.contains(&unavailable_winner))
                .cloned();
            let rescued = branches
                .0
                .iter()
                .find(|branch| branch.0.contains(&rescued_branch))
                .cloned();
            if let (Some(winner), Some(rescued)) = (winner_branch, rescued) {
                map.get_mut(&winner).unwrap().0.insert(rescued.clone());
                map.get_mut(&rescued).unwrap().0.insert(winner);
            }
            Ok(map)
        };
        let resolve_once = |actual_seq: Vec<DeployChainIndex>, late_seq: Vec<DeployChainIndex>| {
            conflict_set_merger::resolve_conflicts(
                actual_seq,
                late_seq,
                &|_, _| false,
                &cost_optimal_rejection_alg(),
                &|chain: &DeployChainIndex| chain.prior_rejections,
                &|_| BTreeMap::new(),
                &|_| Ok(Vec::new()),
                &compute_branches,
                &compute_conflict_map,
                &HashSet::new(),
            )
        };
        let split_unavailable =
            |resolved: &mut conflict_set_merger::ResolvedConflicts<DeployChainIndex>| {
                split_unavailable_resolved_branches(
                    resolved,
                    &|_, _| false,
                    &|chain| Ok(chain.state_changes.clone()),
                    &|_| BTreeMap::new(),
                    &|_| Ok(Vec::new()),
                    &|_| Ok(Vec::new()),
                )
            };

        let (resolved, unavailable_count) = resolve_conflicts_with_unavailable_retry(
            &actual_seq_all,
            &late_seq_all,
            &resolve_once,
            &split_unavailable,
        )
        .expect("retrying unavailable conflicts should succeed");

        assert_eq!(unavailable_count, 1);
        assert!(resolved.rejected.0.contains(&unavailable_winner));
        assert!(!resolved.rejected.0.contains(&rescued_branch));
        assert!(resolved
            .to_merge
            .iter()
            .any(|branch| branch.0.contains(&rescued_branch)));
    }

    /// The stranded-consumer hazard — a chain reaching the fold with a datum
    /// removal the base cannot match, which `make_trie_action` hard-errors on
    /// (the propose-wedge signature seen on shard `ad584769`) — is NOT
    /// reachable through `merge`'s resolution pipeline. Stranding needs a
    /// producer and its consumer inside ONE branch with the producer
    /// droppable on its own, and the pipeline forecloses both halves:
    ///
    /// - **No event-log dependency** ⇒ the two chains are DIFFERENT branches
    ///   (`compute_branches` groups by `depends`), and availability walks
    ///   branches separately carrying base plus that branch's own accepted
    ///   adds — so the consumer's unbacked removal is rejected on the first
    ///   pass, before §3c runs at all.
    /// - **With the dependency** ⇒ they share a branch and both pass
    ///   availability, but §3c's rejection of the producer re-enters
    ///   `resolve_conflicts_with_unavailable_retry`, which puts the producer
    ///   in the LATE set; `resolve_conflicts` then drops the consumer as its
    ///   dependent before the fold sees it.
    ///
    /// Both arms assert the same end state: whatever survives the loop passes
    /// the availability check that `make_trie_action` would otherwise fail.
    #[test]
    fn the_resolution_pipeline_converges_on_an_availability_coherent_set() {
        let cell = Blake2b256Hash::from_bytes(vec![0x91; 32]);
        let counter = Blake2b256Hash::from_bytes(vec![0x92; 32]);
        let base_value: Vec<u8> = vec![0x01; 32];
        let produced: Vec<u8> = vec![0x02; 32];
        let final_value: Vec<u8> = vec![0x03; 32];

        // The produce the consumer consumes — present only in the dependent
        // arm, which is what puts the two chains in one branch.
        let carried = Produce {
            channel_hash: cell.clone(),
            hash: Blake2b256Hash::from_bytes(vec![0xaa; 32]),
            persistent: false,
            is_deterministic: true,
            output_value: vec![],
            failed: false,
        };

        let run = |dependent: bool| -> Vec<DeployChainIndex> {
            // Producer: RMWs the cell (base -> produced) AND writes the counter.
            let producer_changes = datum_change(cell.clone(), vec![base_value.clone()], vec![
                produced.clone(),
            ]);
            producer_changes
                .datums_changes
                .insert(counter.clone(), ChannelChange {
                    added: vec![encoded_number(&counter, 7)],
                    removed: Vec::new(),
                });
            let (producer_log, consumer_log) = if dependent {
                let mut p = EventLogIndex::empty();
                p.produces_linear = HashableSet(HashSet::from([carried.clone()]));
                let mut c = EventLogIndex::empty();
                c.produces_consumed = HashableSet(HashSet::from([carried.clone()]));
                (p, c)
            } else {
                (EventLogIndex::empty(), EventLogIndex::empty())
            };
            let producer = chain_with_event_log(1, 10, 1, producer_changes, producer_log);
            // Consumer: computed against the producer's post-state — removes
            // `produced`, which the BASE does not hold.
            let consumer = chain_with_event_log(
                2,
                10,
                1,
                datum_change(cell.clone(), vec![produced.clone()], vec![
                    final_value.clone()
                ]),
                consumer_log,
            );
            // A second, lower-ordered writer to the same counter. Neither
            // chain overfills the counter on its own.
            let rival = chain(
                0,
                10,
                1,
                datum_change(counter.clone(), Vec::new(), vec![encoded_number(
                    &counter, 9,
                )]),
            );

            let base_data = |ch: &Blake2b256Hash| {
                Ok(if *ch == cell {
                    vec![base_value.clone()]
                } else {
                    Vec::new()
                })
            };
            let depends_fn = |a: &DeployChainIndex, b: &DeployChainIndex| {
                merging_logic::depends(&a.event_log_index, &b.event_log_index)
            };
            let mergeable_fn =
                |c: &DeployChainIndex| c.event_log_index.number_channels_data.clone();
            let compute_branches = |merge_set: &HashableSet<DeployChainIndex>| {
                let chains_vec: Vec<DeployChainIndex> = merge_set.0.iter().cloned().collect();
                let event_logs: Vec<&EventLogIndex> =
                    chains_vec.iter().map(|c| &c.event_log_index).collect();
                #[allow(clippy::mutable_key_type)]
                let depends_map =
                    merging_logic::compute_depends_map_event_indexed(&chains_vec, &event_logs);
                merging_logic::gather_related_sets(&depends_map)
            };
            let compute_conflict_map = |branches: &HashableSet<HashableSet<DeployChainIndex>>| {
                #[allow(clippy::mutable_key_type)]
                let mut map = HashMap::new();
                for branch in branches.0.iter() {
                    map.insert(branch.clone(), HashableSet(HashSet::new()));
                }
                Ok(map)
            };
            let resolve_once = |actual_seq: Vec<DeployChainIndex>,
                                late_seq: Vec<DeployChainIndex>| {
                conflict_set_merger::resolve_conflicts(
                    actual_seq,
                    late_seq,
                    &depends_fn,
                    &cost_optimal_rejection_alg(),
                    &|chain: &DeployChainIndex| chain.prior_rejections,
                    &mergeable_fn,
                    &|_| Ok(Vec::new()),
                    &compute_branches,
                    &compute_conflict_map,
                    &HashSet::new(),
                )
            };
            // Exactly what `merge` runs per iteration: availability, then §3c,
            // unioned into one rejection set that feeds the retry.
            let split_unavailable =
                |resolved: &mut conflict_set_merger::ResolvedConflicts<DeployChainIndex>| {
                    let mut rejected = split_unavailable_resolved_branches(
                        resolved,
                        &depends_fn,
                        &|c| Ok(c.state_changes.clone()),
                        &mergeable_fn,
                        &base_data,
                        &|_| Ok(Vec::new()),
                    )?;
                    let overfilled = split_overfilled_single_value_cells(
                        resolved,
                        &depends_fn,
                        &mergeable_fn,
                        &|_| Ok(Vec::new()),
                        &base_data,
                        &HashSet::new(),
                    )?;
                    for chain in overfilled.0 {
                        rejected.0.insert(chain);
                    }
                    Ok(rejected)
                };

            let (resolved, _) = resolve_conflicts_with_unavailable_retry(
                &[producer, consumer, rival],
                &[],
                &resolve_once,
                &split_unavailable,
            )
            .expect("resolution pipeline");

            let survivors: Vec<DeployChainIndex> = resolved
                .to_merge
                .iter()
                .flat_map(|b| b.0.iter().cloned())
                .collect();

            // The property under test: what reaches the fold must pass the
            // availability check `make_trie_action` implicitly relies on.
            for chain in &survivors {
                let (kept, rejected) = split_unavailable_branch_consumes(
                    HashableSet(HashSet::from([chain.clone()])),
                    &depends_fn,
                    &|c| Ok(c.state_changes.clone()),
                    &mergeable_fn,
                    &base_data,
                    &|_| Ok(Vec::new()),
                )
                .expect("availability re-check");
                assert!(
                    kept.is_some() && rejected.0.is_empty(),
                    "a survivor carries a removal the base cannot match — this is the \
                     unbacked-removal state that hard-errors in make_trie_action"
                );
            }
            survivors
        };

        // Arm 1 — independent chains: availability rejects the consumer on the
        // first pass, because it is alone in its branch with an unbacked removal.
        let consumer_id = Bytes::from(vec![2u8]);
        let independent = run(false);
        assert!(
            !independent.iter().any(|c| c
                .deploys_with_cost
                .0
                .iter()
                .any(|d| d.deploy_id == consumer_id)),
            "the consumer must not survive: its removal is unbacked at the base and \
             nothing in its own branch produces it"
        );

        // Arm 2 — genuinely dependent chains: both pass availability together,
        // §3c drops the producer, and the retry drops the consumer as its
        // dependent. Convergence is asserted by the per-survivor check above.
        let dependent = run(true);
        assert!(
            dependent.len() < 3,
            "the producer's §3c rejection must carry its dependent consumer out too"
        );
    }

    /// Availability verdicts are RELATIVE TO THE SURVIVOR SET they were
    /// computed on. The splitter walks a branch in dependency order carrying
    /// base plus the adds of chains accepted before it, so a chain's removal
    /// can be available ONLY because an earlier accepted chain produced that
    /// datum — remove that producer and the same chain is unavailable.
    ///
    /// This pins the invariant, not a live defect: `merge` does reject further
    /// chains after the splitters run (the stale-diff lineage expansion) and
    /// re-validates nothing, so any future rejection pass that can drop a
    /// producer while keeping its consumer would hand an unbacked removal to
    /// `make_trie_action`. The expansion itself does not: it rejects whole
    /// DAG-descendant lineages, and event-log dependency covers same-block
    /// pairs.
    #[test]
    fn availability_verdicts_are_relative_to_the_survivor_set() {
        let channel = Blake2b256Hash::from_bytes(vec![0x77; 32]);
        let base_datum: Vec<u8> = vec![0x01; 32];
        let produced: Vec<u8> = vec![0x02; 32];
        let final_datum: Vec<u8> = vec![0x03; 32];

        // Producer consumes the base datum and writes `produced`.
        let producer = chain(
            1,
            10,
            1,
            datum_change(channel.clone(), vec![base_datum.clone()], vec![
                produced.clone()
            ]),
        );
        // Consumer was computed against the producer's post-state: it removes
        // `produced`, a datum the BASE does not hold.
        let consumer = chain(
            2,
            10,
            2,
            datum_change(channel.clone(), vec![produced.clone()], vec![final_datum]),
        );

        let base_data = |ch: &Blake2b256Hash| {
            Ok(if *ch == channel {
                vec![base_datum.clone()]
            } else {
                Vec::new()
            })
        };
        let state_changes_fn = |c: &DeployChainIndex| Ok(c.state_changes.clone());
        let no_mergeable = |_: &DeployChainIndex| BTreeMap::new();
        let no_continuations = |_: &Vec<Blake2b256Hash>| Ok(Vec::new());
        let one_branch = |merge_set: &HashableSet<DeployChainIndex>| {
            HashableSet(HashSet::from([HashableSet(merge_set.0.clone())]))
        };
        let no_conflicts = |branches: &HashableSet<HashableSet<DeployChainIndex>>| {
            Ok(branches
                .0
                .iter()
                .map(|branch| (branch.clone(), HashableSet(HashSet::new())))
                .collect())
        };
        let resolve = |chains: Vec<DeployChainIndex>| {
            conflict_set_merger::resolve_conflicts(
                chains,
                Vec::new(),
                &|_: &DeployChainIndex, _: &DeployChainIndex| false,
                &cost_optimal_rejection_alg(),
                &|chain: &DeployChainIndex| chain.prior_rejections,
                &no_mergeable,
                &|_| Ok(Vec::new()),
                &one_branch,
                &no_conflicts,
                &HashSet::new(),
            )
            .expect("conflict resolution should succeed")
        };

        // Together, both chains pass availability: the consumer's removal is
        // backed by the producer accepted immediately before it.
        let mut together = resolve(vec![producer.clone(), consumer.clone()]);
        split_unavailable_resolved_branches(
            &mut together,
            &|_, _| false,
            &state_changes_fn,
            &no_mergeable,
            &base_data,
            &no_continuations,
        )
        .expect("availability split should succeed");
        assert!(
            together.rejected.0.is_empty(),
            "both chains must pass availability together — the consumer's removal is \
             backed by the producer's add (rejected={})",
            together.rejected.0.len()
        );

        // The lineage expansion now drops the producer (its source block
        // DAG-descends from a rejected chain's source block). Re-running the
        // SAME availability check on what survives shows the survivor set is no
        // longer valid: the consumer's removal is unbacked at the base.
        let mut after_expansion = resolve(vec![consumer.clone()]);
        split_unavailable_resolved_branches(
            &mut after_expansion,
            &|_, _| false,
            &state_changes_fn,
            &no_mergeable,
            &base_data,
            &no_continuations,
        )
        .expect("availability split should succeed");
        assert!(
            after_expansion.rejected.0.contains(&consumer),
            "the consumer must be unavailable once the producer is gone — if it is not, \
             this reproduction no longer stages the post-expansion hazard"
        );

        // The defect: `merge` hands the post-expansion set to the fold without
        // repeating that check, so this unbacked removal reaches
        // `make_trie_action`.
        assert!(
            together
                .to_merge
                .iter()
                .any(|branch| branch.0.contains(&consumer)),
            "the consumer survives the pre-expansion split and is carried into the fold"
        );
    }

    #[test]
    fn unavailable_retry_rejects_dependents_of_removed_winner() {
        let channel = Blake2b256Hash::from_bytes(vec![0x55; 32]);
        let unavailable_winner = chain(
            1,
            10,
            1,
            datum_change(channel.clone(), vec![vec![0xaa; 32]], vec![vec![0xbb; 32]]),
        );
        let dependent = chain(2, 9, 2, StateChange::empty());
        let rescued_branch = chain(3, 1, 1, StateChange::empty());
        let actual_seq_all = vec![
            unavailable_winner.clone(),
            dependent.clone(),
            rescued_branch.clone(),
        ];
        let late_seq_all = Vec::new();

        let compute_branches = |merge_set: &HashableSet<DeployChainIndex>| {
            HashableSet(
                merge_set
                    .0
                    .iter()
                    .map(|chain| HashableSet(HashSet::from([chain.clone()])))
                    .collect(),
            )
        };
        let compute_conflict_map = |branches: &HashableSet<HashableSet<DeployChainIndex>>| {
            // False positive: DeployChainIndex's Hash/Eq use only immutable fields.
            #[allow(clippy::mutable_key_type)]
            let mut map: HashMap<
                HashableSet<DeployChainIndex>,
                HashableSet<HashableSet<DeployChainIndex>>,
            > = branches
                .0
                .iter()
                .map(|branch| (branch.clone(), HashableSet(HashSet::new())))
                .collect();
            let winner_branch = branches
                .0
                .iter()
                .find(|branch| branch.0.contains(&unavailable_winner))
                .cloned();
            let rescued = branches
                .0
                .iter()
                .find(|branch| branch.0.contains(&rescued_branch))
                .cloned();
            if let (Some(winner), Some(rescued)) = (winner_branch, rescued) {
                map.get_mut(&winner).unwrap().0.insert(rescued.clone());
                map.get_mut(&rescued).unwrap().0.insert(winner);
            }
            Ok(map)
        };
        let depends = |target: &DeployChainIndex, source: &DeployChainIndex| {
            target == &dependent && source == &unavailable_winner
        };
        let resolve_once = |actual_seq: Vec<DeployChainIndex>, late_seq: Vec<DeployChainIndex>| {
            conflict_set_merger::resolve_conflicts(
                actual_seq,
                late_seq,
                &depends,
                &cost_optimal_rejection_alg(),
                &|chain: &DeployChainIndex| chain.prior_rejections,
                &|_| BTreeMap::new(),
                &|_| Ok(Vec::new()),
                &compute_branches,
                &compute_conflict_map,
                &HashSet::new(),
            )
        };
        let split_unavailable =
            |resolved: &mut conflict_set_merger::ResolvedConflicts<DeployChainIndex>| {
                split_unavailable_resolved_branches(
                    resolved,
                    &depends,
                    &|chain| Ok(chain.state_changes.clone()),
                    &|_| BTreeMap::new(),
                    &|_| Ok(Vec::new()),
                    &|_| Ok(Vec::new()),
                )
            };

        let (resolved, unavailable_count) = resolve_conflicts_with_unavailable_retry(
            &actual_seq_all,
            &late_seq_all,
            &resolve_once,
            &split_unavailable,
        )
        .expect("retrying unavailable conflicts should reject dependents");

        assert_eq!(unavailable_count, 1);
        assert!(resolved.rejected.0.contains(&unavailable_winner));
        assert!(resolved.rejected.0.contains(&dependent));
        assert!(!resolved.rejected.0.contains(&rescued_branch));
        assert!(resolved
            .to_merge
            .iter()
            .any(|branch| branch.0.contains(&rescued_branch)));
    }
}
