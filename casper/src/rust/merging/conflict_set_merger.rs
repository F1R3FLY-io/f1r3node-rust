// See casper/src/main/scala/coop/rchain/casper/merging/ConflictSetMerger.scala

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use models::rhoapi::ListParWithRandom;
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rspace_plus_plus::rspace::errors::HistoryError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hot_store_trie_action::HotStoreTrieAction;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::merger::merging_logic::{
    combine_mergeable_value, MergeType, NumberChannelsDiff,
};
use rspace_plus_plus::rspace::merger::state_change::StateChange;
use shared::rust::hashable_set::HashableSet;
use tracing::{debug, info};

type Branch<R> = HashableSet<R>;

// Utility for timing operations
fn measure_time<T, F: FnOnce() -> T>(f: F) -> (T, Duration) {
    let start = Instant::now();
    let result = f();
    let duration = start.elapsed();
    (result, duration)
}

// Utility to time operations that return Result
fn measure_result_time<T, E, F: FnOnce() -> Result<T, E>>(f: F) -> Result<(T, Duration), E> {
    let start = Instant::now();
    let result = f()?;
    let duration = start.elapsed();
    Ok((result, duration))
}

/// Compare two branches for deterministic ordering.
/// Ordering for branches to ensure deterministic comparison.
fn compare_branches<R: Ord>(a: &Branch<R>, b: &Branch<R>) -> std::cmp::Ordering {
    // Compare by sorted elements
    let mut a_sorted: Vec<_> = a.0.iter().collect();
    let mut b_sorted: Vec<_> = b.0.iter().collect();
    a_sorted.sort();
    b_sorted.sort();

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        tracing::debug!(target: "f1r3fly.merge.step", step = "compare_branches.ENTER",
            a_len = a_sorted.len(),
            b_len = b_sorted.len());
    }

    let len_cmp = a_sorted.len().cmp(&b_sorted.len());
    if len_cmp != std::cmp::Ordering::Equal {
        if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
            tracing::debug!(target: "f1r3fly.merge.step", step = "compare_branches.EXIT",
                ordering = ?len_cmp);
        }
        return len_cmp;
    }

    for (a_item, b_item) in a_sorted.iter().zip(b_sorted.iter()) {
        let cmp = a_item.cmp(b_item);
        if cmp != std::cmp::Ordering::Equal {
            if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                tracing::debug!(target: "f1r3fly.merge.step", step = "compare_branches.EXIT",
                    ordering = ?cmp);
            }
            return cmp;
        }
    }

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        tracing::debug!(target: "f1r3fly.merge.step", step = "compare_branches.EXIT",
            ordering = "Equal");
    }
    std::cmp::Ordering::Equal
}

/// Result of conflict resolution. Callers that need to adjust the rejection
/// set before diffs are applied — for example, to add DAG-descendants of
/// rejected blocks whose diffs would be stale — can do so before invoking
/// `compute_merged_state`.
pub struct ResolvedConflicts<R: Clone + Eq + std::hash::Hash> {
    /// Branches surviving conflict resolution; their diffs will be applied.
    pub to_merge: Vec<HashableSet<R>>,
    /// Rejected items (late set + dependents + optimal rejection).
    pub rejected: HashableSet<R>,
    // Diagnostic counters used in the summary log.
    pub late_set_size: usize,
    pub actual_set_size: usize,
    pub branches_count: usize,
    pub rejected_as_dependents_count: usize,
    pub optimal_rejection_count: usize,
    pub conflict_map_conflicts_count: usize,
    pub rejection_options_count: usize,
    // Timings.
    pub branches_time: Duration,
    pub conflicts_map_time: Duration,
    pub rejection_options_time: Duration,
}

/// Conflict detection and optimal rejection selection. Returns the set of
/// chains to merge along with those rejected. Callers can adjust the result
/// before calling `compute_merged_state`.
///
/// `conflicts` is fallible: a `MergeType` mismatch (or any other invariant
/// violation surfaced by event-log combination) is propagated as a hard error
/// so the merge is rejected rather than silently absorbed.
pub fn resolve_conflicts<R: Clone + Eq + std::hash::Hash + PartialOrd + Ord>(
    actual_seq: Vec<R>,
    late_seq: Vec<R>,
    depends: &impl Fn(&R, &R) -> bool,
    cost: &impl Fn(&R) -> u64,
    // Prior on-DAG rejections per item (issue #294). Rejection-option
    // selection minimizes this BEFORE cost: a chain that already lost
    // merges must not keep losing on the same content-deterministic
    // criteria, or it starves to expiry. Zero everywhere reproduces the
    // pure cost-optimal selection.
    prior_losses: &impl Fn(&R) -> u64,
    mergeable_channels: &impl Fn(&R) -> NumberChannelsDiff,
    get_data: &impl Fn(Blake2b256Hash) -> Result<Vec<Datum<ListParWithRandom>>, HistoryError>,
    // Splits a set of items into branches whose elements are mutually
    // dependent. Returned branches must partition the input — every item
    // appears in exactly one branch.
    compute_branches: &impl Fn(&HashableSet<R>) -> HashableSet<HashableSet<R>>,
    // Builds the conflict map between branches. Must include every branch
    // as a key — branches with no conflicts get an empty value set — so
    // `compute_rejection_options` downstream sees the full key space.
    compute_conflict_map: &impl Fn(
        &HashableSet<HashableSet<R>>,
    ) -> Result<
        HashMap<HashableSet<R>, HashableSet<HashableSet<R>>>,
        HistoryError,
    >,
    // Chains whose effects are ALREADY in the state of the block being built
    // on — its main parent's committed state. A merge may never adjudicate
    // them away: dropping content the main parent already holds makes the
    // block's state fail to contain its own spine ancestor's, which is exactly
    // how a finalized candidate ends up with no live state holding it. Empty
    // means "nothing pinned" and the selection is unconstrained.
    //
    // Both production call sites pass empty — the merge bases on the main
    // parent, so its chains never enter the conflict set to begin with. This
    // is exercised only by unit tests.
    pinned: &HashSet<R>,
) -> Result<ResolvedConflicts<R>, HistoryError> {
    tracing::debug!(target: "f1r3fly.merge.step", step = "resolve_conflicts.ENTER",
        n_actual = actual_seq.len(), n_late = late_seq.len());
    // Convert to Sets for set operations, but use Vec for ordered iteration
    let actual_set: HashSet<R> = actual_seq.iter().cloned().collect();
    let late_set: HashSet<R> = late_seq.iter().cloned().collect();

    // Split the actual_set into branches without cross dependencies
    let (rejected_as_dependents, merge_set): (HashableSet<R>, HashableSet<R>) = {
        let mut rejected = HashableSet(HashSet::new());
        let mut to_merge = HashableSet(HashSet::new());

        for item in &actual_set {
            if late_set.iter().any(|late_item| depends(item, late_item)) {
                rejected.0.insert(item.clone());
            } else {
                to_merge.0.insert(item.clone());
            }
        }

        (rejected, to_merge)
    };

    // Group items in merge_set into branches whose elements are mutually
    // dependent.
    let (branches, branches_time) = measure_time(|| compute_branches(&merge_set));
    metrics::histogram!(
        crate::rust::metrics_constants::DAG_MERGE_BRANCHES_TIME_METRIC,
        "source" => crate::rust::metrics_constants::MERGING_METRICS_SOURCE
    )
    .record(branches_time.as_secs_f64());

    tracing::debug!(target: "f1r3fly.merge.step", step = "resolve_conflicts.branches",
        n_branches = branches.0.len(),
        branch_sizes = ?branches.0.iter().map(|b| b.0.len()).collect::<Vec<_>>(),
        rejected_as_dependents = rejected_as_dependents.0.len());

    let branches_set = HashableSet(branches.0.iter().cloned().collect());
    let (conflict_map, conflicts_map_time) =
        measure_result_time(|| compute_conflict_map(&branches_set))?;
    {
        let total_edges: usize = conflict_map.values().map(|s| s.0.len()).sum();
        tracing::debug!(target: "f1r3fly.merge.step", step = "resolve_conflicts.conflict_map",
            n_keys = conflict_map.len(), total_edges = total_edges);
    }
    metrics::histogram!(
        crate::rust::metrics_constants::DAG_MERGE_CONFLICTS_MAP_TIME_METRIC,
        "source" => crate::rust::metrics_constants::MERGING_METRICS_SOURCE
    )
    .record(conflicts_map_time.as_secs_f64());

    // Compute rejection options that leave only non-conflicting branches with timing
    use rspace_plus_plus::rspace::merger::merging_logic::compute_rejection_options;
    let (rejection_options, rejection_options_time) =
        measure_time(|| compute_rejection_options(&conflict_map));
    metrics::histogram!(
        crate::rust::metrics_constants::DAG_MERGE_REJECTION_OPTIONS_TIME_METRIC,
        "source" => crate::rust::metrics_constants::MERGING_METRICS_SOURCE
    )
    .record(rejection_options_time.as_secs_f64());

    // Get base mergeable channel results
    let channel_reads_start = Instant::now();
    // Sort keys for deterministic ordering across instances
    let mut all_channel_keys_set: std::collections::HashSet<Blake2b256Hash> =
        std::collections::HashSet::new();
    for branch in &branches {
        for item in branch {
            let item_channels = mergeable_channels(item);
            for (channel_hash, _) in item_channels.iter() {
                all_channel_keys_set.insert(channel_hash.clone());
            }
        }
    }
    let mut all_channel_keys: Vec<Blake2b256Hash> = all_channel_keys_set.into_iter().collect();
    // Sort channel keys for deterministic processing order
    all_channel_keys.sort();

    let mut base_mergeable_ch_res = HashMap::new();

    // Use RholangMergingLogic to convert the data reader function
    let get_data_ref = |hash: &Blake2b256Hash| get_data(hash.clone());
    let read_number = RholangMergingLogic::convert_to_read_number(get_data_ref);

    // Read channel numbers from storage in sorted order. `read_number` distinguishes
    // three outcomes: Ok(Some(n)) = numeric value present; Ok(None) = channel doesn't
    // exist (legitimate, start from 0); Err(_) = invariant violation or I/O error
    // (propagate to reject the merge rather than silently substituting 0).
    for channel_hash in &all_channel_keys {
        let value = read_number(channel_hash)?.unwrap_or(0);
        base_mergeable_ch_res.insert(channel_hash.clone(), value);
    }

    metrics::histogram!(
        "dag.merge.channel-reads.time",
        "source" => crate::rust::metrics_constants::MERGING_METRICS_SOURCE
    )
    .record(channel_reads_start.elapsed().as_secs_f64());

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        let channel_reads: Vec<(String, i64)> = all_channel_keys
            .iter()
            .map(|h| {
                (
                    hex::encode(h.clone().bytes()),
                    *base_mergeable_ch_res.get(h).unwrap_or(&0),
                )
            })
            .collect();
        tracing::debug!(target: "f1r3fly.merge.step", step = "resolve_conflicts.base_channels",
            n_channels = all_channel_keys.len(),
            channels = ?channel_reads);
    }

    // Get merged result rejection options
    let rejection_options_with_overflow = get_merged_result_rejection(
        &branches_set,
        &rejection_options,
        base_mergeable_ch_res.clone(),
        mergeable_channels,
    );

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        let option_branch_counts: Vec<usize> = rejection_options_with_overflow
            .0
            .iter()
            .map(|o| o.0.len())
            .collect();
        tracing::debug!(target: "f1r3fly.merge.step", step = "resolve_conflicts.merged_result_rejection",
            n_options = rejection_options_with_overflow.0.len(),
            option_branch_counts = ?option_branch_counts);
    }

    // Drop any option that would adjudicate away pinned content BEFORE cost
    // selection: cost is a phlo sum with no notion of provenance, so a main
    // parent's chain is rejected whenever it happens to be the cheaper side of
    // a conflict — which is how a block ends up with a state that omits
    // content its own spine ancestor holds.
    //
    // This is a PREFERENCE, not a veto. Where a pinned-disjoint option exists
    // it is taken; where none does, the merge still completes on the
    // unconstrained selection (see below). Guaranteeing the invariant instead
    // would mean refusing to build the block, and a certain propose wedge is
    // the worse failure of the two.
    //
    // UNREACHABLE in production, including its error path: `pinned` is empty
    // on both production call sites because the merge now bases on the main
    // parent, which puts its chains in the base instead of the conflict set.
    // A zero count of the `merge.incoherence` line THIS block emits is
    // therefore vacuous. (The same target also carries `explain_merge_failure`,
    // which IS live — read the message, not just the target.) The block is
    // retained pending the decision to remove it; the tests below are its only
    // remaining exercise.
    let rejection_options_with_overflow = if pinned.is_empty() {
        rejection_options_with_overflow
    } else {
        let admissible: HashSet<HashableSet<HashableSet<R>>> = rejection_options_with_overflow
            .0
            .iter()
            .filter(|option| {
                !option
                    .0
                    .iter()
                    .any(|branch| branch.0.iter().any(|item| pinned.contains(item)))
            })
            .cloned()
            .collect();
        if admissible.is_empty() && !rejection_options_with_overflow.0.is_empty() {
            // Two chains the main parent applied SEQUENTIALLY can read as
            // conflicting when this merge re-applies them side by side from the
            // floor — `conflicts` check #2 pairs a surviving produce with a
            // surviving consume on the same channel without examining patterns,
            // so a pair that never COMM'd in the main parent's own history
            // still registers. Refusing the merge here would turn that into a
            // propose wedge, which is a worse failure than the one pinning
            // prevents. Fall back to the unconstrained selection and say so:
            // the residual is a merge whose state may omit main-parent content,
            // which the floor's containment guard still catches loudly.
            tracing::error!(
                target: "f1r3fly.merge.incoherence",
                n_pinned = pinned.len(),
                n_options = rejection_options_with_overflow.0.len(),
                "no rejection option preserves every main-parent chain; falling back \
                 to cost-optimal selection. Re-applying the main parent's chains from \
                 the floor made them conflict with each other"
            );
            rejection_options_with_overflow
        } else {
            HashableSet(admissible)
        }
    };

    // Compute optimal rejection using prior losses, then cost
    let optimal_rejection = get_optimal_rejection(
        rejection_options_with_overflow,
        |branch| branch.0.iter().map(|item| cost(item)).sum(),
        |branch| branch.0.iter().map(|item| prior_losses(item)).sum(),
    );

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        tracing::debug!(target: "f1r3fly.merge.step", step = "resolve_conflicts.optimal_rejection",
            n_rejected_branches = optimal_rejection.0.len(),
            rejected_branch_sizes = ?optimal_rejection.0.iter().map(|b| b.0.len()).collect::<Vec<_>>());
    }

    // Compute branches to merge (difference of branches and optimal_rejection)
    let to_merge: Vec<HashableSet<R>> = branches
        .into_iter()
        .filter(|branch| {
            // Check if branch is not in optimal_rejection
            !optimal_rejection.0.iter().any(|reject_branch| {
                if branch.0.len() != reject_branch.0.len() {
                    return false;
                }
                branch.0.iter().all(|item| reject_branch.0.contains(item))
            })
        })
        .collect();

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        tracing::debug!(target: "f1r3fly.merge.step", step = "resolve_conflicts.to_merge",
            n_to_merge = to_merge.len(),
            to_merge_sizes = ?to_merge.iter().map(|b| b.0.len()).collect::<Vec<_>>());
    }

    // Flatten the optimal rejection set
    let mut optimal_rejection_flattened = HashableSet(HashSet::new());
    for branch in &optimal_rejection {
        for item in branch {
            optimal_rejection_flattened.0.insert(item.clone());
        }
    }

    // Combine all rejected items
    let mut rejected = HashableSet(HashSet::new());
    for item in &late_set {
        rejected.0.insert(item.clone());
    }
    for item in &rejected_as_dependents {
        rejected.0.insert(item.clone());
    }
    for item in &optimal_rejection_flattened {
        rejected.0.insert(item.clone());
    }

    tracing::debug!(target: "f1r3fly.merge.step", step = "resolve_conflicts.rejected_composition",
        late = late_set.len(),
        rejected_as_dependents = rejected_as_dependents.0.len(),
        optimal_rejection_flattened = optimal_rejection_flattened.0.len(),
        total_rejected = rejected.0.len());

    // Detailed INFO logging for rejection breakdown (always visible)
    let conflict_map_conflicts_count = conflict_map.iter().filter(|(_, v)| !v.0.is_empty()).count();
    info!(
        "ConflictSetMerger rejection breakdown: lateSet={}, rejectedAsDependents={}, \
        optimalRejection={}, total rejected={}, branches={}, toMerge={}, \
        conflictMap entries with conflicts={}, rejectionOptions={}, rejectionOptionsWithOverflow={}",
        late_set.len(),
        rejected_as_dependents.0.len(),
        optimal_rejection_flattened.0.len(),
        rejected.0.len(),
        branches_set.0.len(),
        to_merge.len(),
        conflict_map_conflicts_count,
        rejection_options.0.len(),
        1  // rejectionOptionsWithOverflow.size - approximation
    );

    tracing::debug!(target: "f1r3fly.merge.step", step = "resolve_conflicts.EXIT",
        n_to_merge = to_merge.len(),
        n_rejected = rejected.0.len(),
        n_branches = branches_set.0.len());

    Ok(ResolvedConflicts {
        to_merge,
        rejected,
        late_set_size: late_set.len(),
        actual_set_size: actual_set.len(),
        branches_count: branches_set.0.len(),
        rejected_as_dependents_count: rejected_as_dependents.0.len(),
        optimal_rejection_count: optimal_rejection.0.len(),
        conflict_map_conflicts_count,
        rejection_options_count: rejection_options.0.len(),
        branches_time,
        conflicts_map_time,
        rejection_options_time,
    })
}

/// Finding-A runtime guard (docs/casper/theory/merge-algebra/merge-algebra-verification.md §6).
/// The shipped merge operator `ChannelChange::combine` (max-union) is
/// non-associative, yet the merged root is node-identical because survivors are
/// folded in canonical sorted order AND no order-dependent survivor pair reaches
/// apply. This accumulator enforces that second premise at runtime: it trips iff
/// some datum value is contributed to `added` (or `removed`) by >= 2 DISTINCT
/// survivors on the same NON-mergeable channel — the exact precondition under which
/// the max-union fold differs from the order-independent sum-union fold
/// (`ChannelNetting.v` `combine_max_eq_combine_sum_under_no_dup`). It reads only the
/// survivor set and changes no post-state; a trip means the invariant the proofs
/// assume was violated (e.g. a conflict-check regression let such a pair through).
#[derive(Default)]
struct OrderDependenceGuard {
    added: HashMap<Blake2b256Hash, HashMap<Vec<u8>, u32>>,
    removed: HashMap<Blake2b256Hash, HashMap<Vec<u8>, u32>>,
}

impl OrderDependenceGuard {
    /// Record one survivor's per-channel datum contributions. Distinct datums per
    /// channel per survivor are counted once — within-survivor multiplicity is
    /// irrelevant to the CROSS-survivor order dependence this detects.
    fn observe(&mut self, changes: &StateChange) {
        for entry in changes.datums_changes.iter() {
            let channel = entry.key();
            let change = entry.value();
            let per_channel_added = self.added.entry(channel.clone()).or_default();
            for datum in change.added.iter().collect::<HashSet<_>>() {
                *per_channel_added.entry(datum.clone()).or_insert(0) += 1;
            }
            let per_channel_removed = self.removed.entry(channel.clone()).or_default();
            for datum in change.removed.iter().collect::<HashSet<_>>() {
                *per_channel_removed.entry(datum.clone()).or_insert(0) += 1;
            }
        }
    }

    /// The lowest (by hash bytes, so node-identically) NON-mergeable channel on
    /// which some datum was contributed by >= 2 distinct survivors, or `None` when
    /// the fold is genuinely order-independent (max-fold == sum-fold). Mergeable /
    /// number channels are skipped: they merge through the commutative-group
    /// `combine_mergeable_value`, not max-union, so multiple producers are safe.
    fn first_offender(&self, mergeable_keys: &HashSet<Blake2b256Hash>) -> Option<Blake2b256Hash> {
        self.added
            .iter()
            .chain(self.removed.iter())
            .filter(|(channel, counts)| {
                !mergeable_keys.contains(*channel) && counts.values().any(|&c| c >= 2)
            })
            .map(|(channel, _)| channel.clone())
            .min_by(|a, b| a.0.cmp(&b.0))
    }
}

/// Combine the surviving chains' diffs into trie actions and apply them to
/// the merged base state. Reads `resolved.to_merge` and returns the new state
/// root; `resolved.rejected` is not read or modified.
pub fn compute_merged_state<R, C, P, A, K>(
    resolved: &ResolvedConflicts<R>,
    state_changes: &impl Fn(&R) -> Result<StateChange, HistoryError>,
    mergeable_channels: &impl Fn(&R) -> NumberChannelsDiff,
    compute_trie_actions: &impl Fn(
        StateChange,
        NumberChannelsDiff,
    ) -> Result<Vec<HotStoreTrieAction<C, P, A, K>>, HistoryError>,
    apply_trie_actions: &impl Fn(
        Vec<HotStoreTrieAction<C, P, A, K>>,
    ) -> Result<Blake2b256Hash, HistoryError>,
) -> Result<Blake2b256Hash, HistoryError>
where
    R: Clone + Eq + std::hash::Hash + PartialOrd + Ord,
    C: Clone,
    P: Clone,
    A: Clone,
    K: Clone,
{
    tracing::debug!(target: "f1r3fly.merge.step", step = "compute_merged_state.ENTER",
        n_to_merge = resolved.to_merge.len(),
        n_total_items = resolved.to_merge.iter().map(|b| b.0.len()).sum::<usize>());

    // Sort toMerge for deterministic processing order
    let mut to_merge_sorted: Vec<&HashableSet<R>> = resolved.to_merge.iter().collect();
    to_merge_sorted.sort_by(|a, b| compare_branches(a, b));

    // Flatten and sort items within each branch
    let mut to_merge_items: Vec<&R> = Vec::new();
    for branch in to_merge_sorted {
        let mut branch_items: Vec<_> = branch.0.iter().collect();
        branch_items.sort();
        to_merge_items.extend(branch_items);
    }

    tracing::debug!(target: "f1r3fly.merge.step", step = "compute_merged_state.items_sorted",
        n_items = to_merge_items.len());

    // Combine state changes from all items to be merged with timing
    let mut order_guard = OrderDependenceGuard::default();
    let (all_changes, combine_all_changes_time) =
        measure_result_time(|| -> Result<StateChange, HistoryError> {
            let mut combined = StateChange::empty();
            for (idx, item) in to_merge_items.iter().enumerate() {
                let item_changes = state_changes(item)?;
                if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                    for entry in item_changes.datums_changes.iter() {
                        let ch = entry.key();
                        let change = entry.value();
                        let removed_bytes: usize = change.removed.iter().map(|d| d.len()).sum();
                        let added_bytes: usize = change.added.iter().map(|d| d.len()).sum();
                        tracing::debug!(target: "f1r3fly.merge.step",
                            step = "compute_merged_state.item_datums",
                            item_idx = idx,
                            channel = %hex::encode(ch.clone().bytes()),
                            removed = change.removed.len(),
                            removed_bytes = removed_bytes,
                            added = change.added.len(),
                            added_bytes = added_bytes);
                    }
                    tracing::debug!(target: "f1r3fly.merge.step",
                        step = "compute_merged_state.item_summary",
                        item_idx = idx,
                        datums = item_changes.datums_changes.len(),
                        conts = item_changes.cont_changes.len(),
                        joins = item_changes.consume_channels_to_join_serialized_map.len());
                }
                // Finding-A guard: record this survivor's datum contributions
                // before `combine` consumes it (detection only, post-state unchanged).
                order_guard.observe(&item_changes);
                combined = combined.combine(item_changes);
                if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                    for entry in combined.datums_changes.iter() {
                        let ch = entry.key();
                        let change = entry.value();
                        let removed_bytes: usize = change.removed.iter().map(|d| d.len()).sum();
                        let added_bytes: usize = change.added.iter().map(|d| d.len()).sum();
                        tracing::debug!(target: "f1r3fly.merge.step",
                            step = "compute_merged_state.combined_running",
                            after_item_idx = idx,
                            channel = %hex::encode(ch.clone().bytes()),
                            removed = change.removed.len(),
                            removed_bytes = removed_bytes,
                            added = change.added.len(),
                            added_bytes = added_bytes);
                    }
                }
            }
            Ok(combined)
        })?;

    metrics::histogram!(
        "dag.merge.combine-changes.time",
        "source" => crate::rust::metrics_constants::MERGING_METRICS_SOURCE
    )
    .record(combine_all_changes_time.as_secs_f64());

    let combined_datums_count = all_changes.datums_changes.len();
    let combined_conts_count = all_changes.cont_changes.len();
    let combined_joins_count = all_changes.consume_channels_to_join_serialized_map.len();

    tracing::debug!(target: "f1r3fly.merge.step", step = "compute_merged_state.combined_total",
        datums = combined_datums_count,
        conts = combined_conts_count,
        joins = combined_joins_count);

    // Combine all mergeable channels (in sorted order). Per-channel `MergeType`
    // determines how diffs combine: integer-add uses wrapping addition; bitmask-OR
    // uses bitwise OR through u64. Branches must agree on merge_type for a given
    // channel; disagreement yields a tagged error so callers reject the merge
    // rather than crashing the validator.
    let mut all_mergeable_channels = NumberChannelsDiff::new();
    for item in &to_merge_items {
        let item_channels = mergeable_channels(item);
        for (key, value) in item_channels.iter() {
            let (incoming_diff, incoming_mt) = *value;
            match all_mergeable_channels.get_mut(key) {
                Some(existing) => {
                    if existing.1 != incoming_mt {
                        return Err(HistoryError::MergeError(format!(
                            "MergeType mismatch on channel {:?}: {:?} vs {:?}",
                            key, existing.1, incoming_mt,
                        )));
                    }
                    existing.0 =
                        match combine_mergeable_value(existing.0, incoming_diff, incoming_mt) {
                            Some(v) => v,
                            // Survivors already passed the per-branch overflow gate, so
                            // this should be unreachable; error rather than write a
                            // wrapped value if it ever is.
                            None => {
                                return Err(HistoryError::MergeError(format!(
                                    "IntegerAdd overflow combining mergeable channel {:?}",
                                    key,
                                )))
                            }
                        };
                }
                None => {
                    all_mergeable_channels.insert(key.clone(), (incoming_diff, incoming_mt));
                }
            }
        }
    }

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        let merged_channels: Vec<(String, i64)> = all_mergeable_channels
            .iter()
            .map(|(k, v)| (hex::encode(k.clone().bytes()), v.0))
            .collect();
        tracing::debug!(target: "f1r3fly.merge.step", step = "compute_merged_state.mergeable_channels",
            n_channels = all_mergeable_channels.len(),
            channels = ?merged_channels);
    }

    // Finding-A guard check (merge-algebra-verification.md §6): trip iff an
    // order-dependent survivor pair reached apply — a datum contributed by >= 2
    // distinct survivors on the same NON-mergeable channel, the precondition under
    // which the non-associative max-union fold differs from the order-independent
    // sum-union fold. Mergeable/number channels merge via the commutative-group
    // operator and are excluded. Detection only: post-state, trie actions, and the
    // merged root are byte-identical whether or not this fires (ChannelNetting.v §5
    // + `combine_max_eq_combine_sum_under_no_dup`).
    let mergeable_keys: HashSet<Blake2b256Hash> = all_mergeable_channels.keys().cloned().collect();
    if let Some(channel) = order_guard.first_offender(&mergeable_keys) {
        debug_assert!(
            false,
            "order-dependent survivor pair reached apply on channel {} — the \
             max-union merge fold is not order-independent here (Finding A; \
             docs/casper/theory/merge-algebra/merge-algebra-verification.md §6)",
            hex::encode(channel.bytes())
        );
        tracing::error!(target: "f1r3fly.merge.step",
            step = "apply.order_dependent_survivor_pair",
            channel = %hex::encode(channel.bytes()),
            "order-dependent survivor pair reached apply; merged post-state may be \
             association-dependent (Finding-A guard, non-fatal)");
        metrics::counter!(
            "dag.merge.order-dependent-survivor-pair",
            "source" => crate::rust::metrics_constants::MERGING_METRICS_SOURCE
        )
        .increment(1);
    }

    tracing::debug!(target: "f1r3fly.merge.step", step = "compute_merged_state.compute_trie_actions.ENTER",
        datums = combined_datums_count,
        conts = combined_conts_count,
        joins = combined_joins_count,
        n_mergeable_channels = all_mergeable_channels.len());

    // Compute and apply trie actions with timing
    let (trie_actions, compute_actions_time) =
        measure_result_time(|| compute_trie_actions(all_changes, all_mergeable_channels.clone()))?;

    tracing::debug!(target: "f1r3fly.merge.step", step = "compute_merged_state.compute_trie_actions.EXIT",
        n_trie_actions = trie_actions.len(),
        elapsed = ?compute_actions_time);

    let (new_state, apply_actions_time) =
        measure_result_time(|| apply_trie_actions(trie_actions.clone()))?;

    tracing::debug!(target: "f1r3fly.merge.step", step = "compute_merged_state.apply_trie_actions.EXIT",
        new_state = %hex::encode(new_state.clone().bytes()),
        elapsed = ?apply_actions_time);

    // Prepare log message
    let log_str = format!(
        "Merging done: late set size {}; actual set size {}; computed branches ({}) in {:?}; \
        conflicts map in {:?}; rejection options ({}) in {:?}; optimal rejection set size {}; \
        rejected as late dependency {}; changes combined (datums={}, conts={}, joins={}) in {:?}; \
        trie actions ({}) in {:?}; actions applied in {:?}",
        resolved.late_set_size,
        resolved.actual_set_size,
        resolved.branches_count,
        resolved.branches_time,
        resolved.conflicts_map_time,
        resolved.rejection_options_count,
        resolved.rejection_options_time,
        resolved.optimal_rejection_count,
        resolved.rejected_as_dependents_count,
        combined_datums_count,
        combined_conts_count,
        combined_joins_count,
        combine_all_changes_time,
        trie_actions.len(),
        compute_actions_time,
        apply_actions_time
    );

    debug!("{}", log_str);

    tracing::debug!(target: "f1r3fly.merge.step", step = "compute_merged_state.EXIT",
        new_state = %hex::encode(new_state.clone().bytes()),
        n_trie_actions = trie_actions.len());

    Ok(new_state)
}

/// R is a type for minimal rejection unit.
/// IMPORTANT: actual_seq and late_seq must be passed in sorted order to ensure
/// deterministic processing across all validators.
///
/// Convenience wrapper that runs `resolve_conflicts` followed by
/// `compute_merged_state`. Callers that need to inspect or adjust the rejection
/// set between the two steps should call them directly instead.
pub fn merge<
    R: Clone + Eq + std::hash::Hash + PartialOrd + Ord,
    C: Clone,
    P: Clone,
    A: Clone,
    K: Clone,
>(
    actual_seq: Vec<R>,
    late_seq: Vec<R>,
    depends: impl Fn(&R, &R) -> bool,
    cost: impl Fn(&R) -> u64,
    state_changes: impl Fn(&R) -> Result<StateChange, HistoryError>,
    mergeable_channels: impl Fn(&R) -> NumberChannelsDiff,
    compute_trie_actions: impl Fn(
        StateChange,
        NumberChannelsDiff,
    ) -> Result<Vec<HotStoreTrieAction<C, P, A, K>>, HistoryError>,
    apply_trie_actions: impl Fn(
        Vec<HotStoreTrieAction<C, P, A, K>>,
    ) -> Result<Blake2b256Hash, HistoryError>,
    get_data: impl Fn(Blake2b256Hash) -> Result<Vec<Datum<ListParWithRandom>>, HistoryError>,
    compute_branches: impl Fn(&HashableSet<R>) -> HashableSet<HashableSet<R>>,
    compute_conflict_map: impl Fn(
        &HashableSet<HashableSet<R>>,
    ) -> Result<
        HashMap<HashableSet<R>, HashableSet<HashableSet<R>>>,
        HistoryError,
    >,
) -> Result<(Blake2b256Hash, HashableSet<R>), HistoryError> {
    tracing::debug!(target: "f1r3fly.merge.step", step = "merge.ENTER",
        n_actual = actual_seq.len(),
        n_late = late_seq.len());

    let resolved = resolve_conflicts(
        actual_seq,
        late_seq,
        &depends,
        &cost,
        // This wrapper merges a bare chain set with no block context, so no
        // prior-loss records exist to consult.
        &|_| 0,
        &mergeable_channels,
        &get_data,
        &compute_branches,
        &compute_conflict_map,
        // This wrapper has no main parent to speak of — it merges a bare chain
        // set with no block context — so nothing is pinned. The node's merge
        // path (`dag_merger::merge`) calls `resolve_conflicts` directly and
        // passes an empty set too, for its own reason: its base IS the main
        // parent, so that parent's chains are never candidates for rejection.
        &HashSet::new(),
    )?;
    let new_state = compute_merged_state(
        &resolved,
        &state_changes,
        &mergeable_channels,
        &compute_trie_actions,
        &apply_trie_actions,
    )?;

    tracing::debug!(target: "f1r3fly.merge.step", step = "merge.EXIT",
        new_state = %hex::encode(new_state.clone().bytes()),
        n_rejected = resolved.rejected.0.len());

    Ok((new_state, resolved.rejected))
}

/// Compute optimal rejection configuration.
/// Find the optimal rejection set from conflicting branches.
fn get_optimal_rejection<R: Eq + std::hash::Hash + Clone + Ord>(
    options: HashableSet<HashableSet<Branch<R>>>,
    target_f: impl Fn(&Branch<R>) -> u64,
    losses_f: impl Fn(&Branch<R>) -> u64,
) -> HashableSet<Branch<R>> {
    assert!(
        options
            .0
            .iter()
            .map(|b| {
                let mut heads = HashSet::new();
                for branch in &b.0 {
                    if let Some(head) = branch.0.iter().min() {
                        // Use min() for determinism
                        heads.insert(head);
                    }
                }
                heads
            })
            .collect::<Vec<_>>()
            .len()
            == options.0.len(),
        "Same rejection unit is found in two rejection options. Please report this to code maintainer."
    );

    tracing::debug!(target: "f1r3fly.merge.step", step = "get_optimal_rejection.ENTER",
        n_options = options.0.len());

    // Convert to sorted list for deterministic processing
    let mut options_vec: Vec<_> = options.0.into_iter().collect();
    options_vec.sort_by(|a, b| {
        // Zeroth criterion (issue #294): total prior losses of the rejected
        // set, ascending — reject the branches with the FEWEST recorded
        // losses, so a chain that already lost gains priority with every
        // loss instead of losing the same tie forever. All-zero counts fall
        // through to the cost criterion unchanged.
        let a_losses: u64 = a.0.iter().map(|branch| losses_f(branch)).sum();
        let b_losses: u64 = b.0.iter().map(|branch| losses_f(branch)).sum();

        if a_losses != b_losses {
            return a_losses.cmp(&b_losses);
        }

        // First criterion: sum of target function values
        let a_sum: u64 = a.0.iter().map(|branch| target_f(branch)).sum();
        let b_sum: u64 = b.0.iter().map(|branch| target_f(branch)).sum();

        if a_sum != b_sum {
            return a_sum.cmp(&b_sum);
        }

        // Second criterion: total size of branches
        let a_size: usize = a.0.iter().map(|branch| branch.0.len()).sum();
        let b_size: usize = b.0.iter().map(|branch| branch.0.len()).sum();

        if a_size != b_size {
            return a_size.cmp(&b_size);
        }

        let mut a_branches: Vec<_> = a.0.iter().collect();
        let mut b_branches: Vec<_> = b.0.iter().collect();
        a_branches.sort_by(|x, y| compare_branches(x, y));
        b_branches.sort_by(|x, y| compare_branches(x, y));

        for (a_branch, b_branch) in a_branches.iter().zip(b_branches.iter()) {
            let ord = compare_branches(a_branch, b_branch);
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        a_branches.len().cmp(&b_branches.len())
    });

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        let candidates: Vec<(usize, u64, usize)> = options_vec
            .iter()
            .map(|o| {
                let cost: u64 = o.0.iter().map(|branch| target_f(branch)).sum();
                let size: usize = o.0.iter().map(|branch| branch.0.len()).sum();
                (o.0.len(), cost, size)
            })
            .collect();
        tracing::debug!(target: "f1r3fly.merge.step", step = "get_optimal_rejection.candidates",
            candidates_n_branches_cost_size = ?candidates);
    }

    let chosen = options_vec
        .into_iter()
        .next()
        .unwrap_or_else(|| HashableSet(HashSet::new()));

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        let chosen_cost: u64 = chosen.0.iter().map(|branch| target_f(branch)).sum();
        tracing::debug!(target: "f1r3fly.merge.step", step = "get_optimal_rejection.EXIT",
            n_chosen_branches = chosen.0.len(),
            chosen_cost = chosen_cost,
            chosen_sizes = ?chosen.0.iter().map(|b| b.0.len()).collect::<Vec<_>>());
    }

    chosen
}

/// Calculate merged result for a branch with the origin result map.
/// Calculate the merged result from base and branches.
///
/// Note: the non-negative-result check applies only to `IntegerAdd` channels
/// (vault balances). `BitmaskOr` channels are bitmaps, where any value is
/// representable; we OR the diff into the existing value without overflow
/// concerns.
fn cal_merged_result<R: Clone + Eq + std::hash::Hash>(
    branch: &Branch<R>,
    origin_result: HashMap<Blake2b256Hash, i64>,
    mergeable_channels: impl Fn(&R) -> NumberChannelsDiff,
) -> Option<HashMap<Blake2b256Hash, i64>> {
    tracing::debug!(target: "f1r3fly.merge.step", step = "cal_merged_result.ENTER",
        n_branch_items = branch.0.len(),
        n_origin_channels = origin_result.len());

    // Combine all channel diffs from the branch using per-channel merge strategy.
    // IntegerAdd overflow HERE means the branch's per-channel diffs sum out of
    // i64 range: reject the branch (fail loudly, return None) rather than fold a
    // silently-wrapped value that could then pass the apply-time
    // `checked_add >= 0` gate below with a wrong result (the overflow-launder;
    // see IntegerAdd.v). BitmaskOr never overflows.
    let mut diff = NumberChannelsDiff::new();
    for r in branch.0.iter() {
        for (k, v) in mergeable_channels(r) {
            let (incoming_diff, incoming_mt) = v;
            match diff.get_mut(&k) {
                Some(existing) => {
                    match combine_mergeable_value(existing.0, incoming_diff, incoming_mt) {
                        Some(combined) => existing.0 = combined,
                        None => {
                            tracing::debug!(target: "f1r3fly.merge.step",
                                step = "cal_merged_result.COMBINE_OVERFLOW",
                                channel = %hex::encode(k.clone().bytes()),
                                existing = existing.0,
                                incoming = incoming_diff);
                            return None;
                        }
                    }
                }
                None => {
                    diff.insert(k, (incoming_diff, incoming_mt));
                }
            }
        }
    }

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        let diff_channels: Vec<(String, i64)> = diff
            .iter()
            .map(|(k, v)| (hex::encode(k.clone().bytes()), v.0))
            .collect();
        tracing::debug!(target: "f1r3fly.merge.step", step = "cal_merged_result.diff",
            n_channels = diff.len(),
            channels = ?diff_channels);
    }

    // Start with Some(origin_result) and fold over the diffs
    let out = diff
        .iter()
        .fold(Some(origin_result), |ba_opt, (channel, value)| {
            ba_opt.and_then(|mut ba| {
                let (diff_val, merge_type) = *value;
                let current = *ba.get(channel).unwrap_or(&0);
                match merge_type {
                    MergeType::IntegerAdd => {
                        // Vault balance: overflow or negative result rejects the branch
                        match current.checked_add(diff_val) {
                            Some(result) if result >= 0 => {
                                ba.insert(channel.clone(), result);
                                Some(ba)
                            }
                            _ => {
                                tracing::debug!(target: "f1r3fly.merge.step",
                                    step = "cal_merged_result.REJECT",
                                    channel = %hex::encode(channel.clone().bytes()),
                                    current = current,
                                    diff = diff_val);
                                None
                            }
                        }
                    }
                    MergeType::BitmaskOr => {
                        // Bitmap: OR the new bits in; no overflow concern
                        let result = ((current as u64) | (diff_val as u64)) as i64;
                        ba.insert(channel.clone(), result);
                        Some(ba)
                    }
                }
            })
        });

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        tracing::debug!(target: "f1r3fly.merge.step", step = "cal_merged_result.EXIT",
            accepted = out.is_some(),
            n_result_channels = out.as_ref().map(|m| m.len()).unwrap_or(0));
    }

    out
}

/// Evaluate branches and return the set of branches that should be rejected.
/// Fold over branches and compute rejections.
fn fold_rejection<R: Clone + Eq + std::hash::Hash + Ord>(
    base_balance: HashMap<Blake2b256Hash, i64>,
    branches: &HashableSet<Branch<R>>,
    mergeable_channels: impl Fn(&R) -> NumberChannelsDiff,
) -> HashableSet<Branch<R>> {
    tracing::debug!(target: "f1r3fly.merge.step", step = "fold_rejection.ENTER",
        n_branches = branches.0.len(),
        n_base_channels = base_balance.len());

    // Sort branches to ensure deterministic processing order
    let mut sorted_branches: Vec<&Branch<R>> = branches.0.iter().collect();
    sorted_branches.sort_by(|a, b| compare_branches(a, b));

    // Fold branches to find which ones would result in negative or overflow balances
    let (_, rejected) = sorted_branches.iter().fold(
        (base_balance, HashableSet(HashSet::new())),
        |(balances, mut rejected), branch| {
            // Check if the branch can be merged without overflow or negative results
            match cal_merged_result(branch, balances.clone(), &mergeable_channels) {
                Some(new_balances) => {
                    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                        tracing::debug!(target: "f1r3fly.merge.step", step = "fold_rejection.accept",
                            branch_size = branch.0.len());
                    }
                    (new_balances, rejected)
                }
                None => {
                    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                        tracing::debug!(target: "f1r3fly.merge.step", step = "fold_rejection.reject",
                            branch_size = branch.0.len());
                    }
                    // If merge calculation returns None, reject this branch
                    rejected.0.insert((*branch).clone());
                    (balances, rejected)
                }
            }
        },
    );

    tracing::debug!(target: "f1r3fly.merge.step", step = "fold_rejection.EXIT",
        n_rejected = rejected.0.len());

    rejected
}

/// Get merged result rejection options.
/// Get the merged result along with rejected deploys.
fn get_merged_result_rejection<R: Clone + Eq + std::hash::Hash + Ord>(
    branches: &HashableSet<Branch<R>>,
    reject_options: &HashableSet<HashableSet<Branch<R>>>,
    base: HashMap<Blake2b256Hash, i64>,
    mergeable_channels: impl Fn(&R) -> NumberChannelsDiff,
) -> HashableSet<HashableSet<Branch<R>>> {
    tracing::debug!(target: "f1r3fly.merge.step", step = "get_merged_result_rejection.ENTER",
        n_branches = branches.0.len(),
        n_reject_options = reject_options.0.len(),
        n_base_channels = base.len());

    let out = if reject_options.0.is_empty() {
        tracing::debug!(target: "f1r3fly.merge.step", step = "get_merged_result_rejection.no_options",
            n_branches = branches.0.len());
        // If no rejection options, fold the branches and return as single option
        let rejected = fold_rejection(base, branches, &mergeable_channels);
        let mut result = HashSet::new();
        result.insert(rejected);
        HashableSet(result)
    } else {
        // For each reject option, compute the difference and fold
        let result: HashSet<HashableSet<Branch<R>>> = reject_options
            .0
            .iter()
            .map(|normal_reject_options| {
                // Find branches that aren't in normal_reject_options
                let diff = HashableSet(
                    branches
                        .0
                        .iter()
                        .filter(|branch| {
                            // Check if branch is not in normal_reject_options
                            !normal_reject_options.0.iter().any(|reject_branch| {
                                if branch.0.len() != reject_branch.0.len() {
                                    return false;
                                }
                                branch.0.iter().all(|item| reject_branch.0.contains(item))
                            })
                        })
                        .cloned()
                        .collect(),
                );

                // Get branches that should be rejected from the diff
                let rejected = fold_rejection(base.clone(), &diff, &mergeable_channels);

                if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                    tracing::debug!(target: "f1r3fly.merge.step", step = "get_merged_result_rejection.option",
                        n_in_diff = diff.0.len(),
                        n_extra_rejected = rejected.0.len(),
                        n_normal_reject = normal_reject_options.0.len());
                }

                // Combine rejected with normal_reject_options
                let mut result = HashableSet(normal_reject_options.0.clone());
                for reject in &rejected.0 {
                    result.0.insert(reject.clone());
                }

                result
            })
            .collect();

        HashableSet(result)
    };

    if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
        tracing::debug!(target: "f1r3fly.merge.step", step = "get_merged_result_rejection.EXIT",
            n_options = out.0.len(),
            option_sizes = ?out.0.iter().map(|o| o.0.len()).collect::<Vec<_>>());
    }

    out
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};

    use super::*;

    fn branch(items: &[i32]) -> Branch<i32> {
        HashableSet(items.iter().copied().collect::<HashSet<i32>>())
    }

    fn rejection_option(branches: &[Branch<i32>]) -> HashableSet<Branch<i32>> {
        HashableSet(branches.iter().cloned().collect::<HashSet<Branch<i32>>>())
    }

    #[test]
    fn compare_branches_is_deterministic() {
        let a = branch(&[1, 2]);
        let b = branch(&[2, 1]);
        let c = branch(&[1, 3]);
        let d = branch(&[1, 4]);
        let short = branch(&[1]);

        assert_eq!(compare_branches(&a, &b), std::cmp::Ordering::Equal);
        assert_eq!(compare_branches(&short, &a), std::cmp::Ordering::Less);
        assert_eq!(compare_branches(&c, &d), std::cmp::Ordering::Less);
    }

    #[test]
    fn optimal_rejection_tie_break_is_stable() {
        // Both options have equal target sum (5) and equal branch count.
        // Deterministic tie-break should pick option_a because its first branch
        // starts with lower element (1 < 2).
        let option_a = rejection_option(&[branch(&[1]), branch(&[4])]);
        let option_b = rejection_option(&[branch(&[2]), branch(&[3])]);
        let options = HashableSet(HashSet::from([option_b.clone(), option_a.clone()]));

        let chosen = get_optimal_rejection(
            options,
            |branch| branch.0.iter().map(|value| *value as u64).sum(),
            |_branch| 0u64,
        );

        assert_eq!(chosen, option_a);
    }

    #[test]
    fn optimal_rejection_is_total_order_when_options_share_first_branch() {
        let shared = branch(&[1, 2]);
        let option_a = rejection_option(&[shared.clone(), branch(&[3, 4])]);
        let option_b = rejection_option(&[shared, branch(&[3, 5])]);

        for options in [
            HashableSet(HashSet::from([option_a.clone(), option_b.clone()])),
            HashableSet(HashSet::from([option_b.clone(), option_a.clone()])),
        ] {
            let chosen = get_optimal_rejection(options, |_branch| 0u64, |_branch| 0u64);
            assert_eq!(chosen, option_a);
        }
    }

    #[test]
    fn optimal_rejection_keeps_higher_loss_branch_despite_cost() {
        let high_loss = branch(&[1]);
        let low_loss = branch(&[2]);
        let reject_high_loss = rejection_option(std::slice::from_ref(&high_loss));
        let reject_low_loss = rejection_option(std::slice::from_ref(&low_loss));
        let options = HashableSet(HashSet::from([reject_high_loss, reject_low_loss.clone()]));

        let chosen = get_optimal_rejection(
            options,
            |branch| if branch == &high_loss { 1 } else { 100 },
            |branch| if branch == &high_loss { 3 } else { 0 },
        );

        assert_eq!(chosen, reject_low_loss);
    }

    #[test]
    fn optimal_rejection_equal_nonzero_losses_fall_back_to_cost_and_order() {
        let lower = branch(&[1]);
        let higher = branch(&[2]);
        let reject_lower = rejection_option(std::slice::from_ref(&lower));
        let reject_higher = rejection_option(std::slice::from_ref(&higher));

        let cost_choice = get_optimal_rejection(
            HashableSet(HashSet::from([reject_lower.clone(), reject_higher.clone()])),
            |branch| if branch == &lower { 10 } else { 1 },
            |_branch| 4,
        );
        assert_eq!(cost_choice, reject_higher);

        let order_choice = get_optimal_rejection(
            HashableSet(HashSet::from([reject_lower.clone(), reject_higher])),
            |_branch| 1,
            |_branch| 4,
        );
        assert_eq!(order_choice, reject_lower);
    }

    #[test]
    fn optimal_rejection_sums_losses_across_branches() {
        let first = branch(&[1]);
        let second = branch(&[2]);
        let third = branch(&[3]);
        let reject_pair = rejection_option(&[first.clone(), second.clone()]);
        let reject_single = rejection_option(std::slice::from_ref(&third));
        let options = HashableSet(HashSet::from([reject_pair, reject_single.clone()]));

        let chosen = get_optimal_rejection(
            options,
            |branch| if branch == &third { 100 } else { 1 },
            |branch| if branch == &third { 3 } else { 2 },
        );

        assert_eq!(chosen, reject_single);
    }

    #[test]
    fn merge_rejects_negative_channel_balance() {
        let actual_seq = vec![1, 2];
        let late_seq = Vec::<i32>::new();
        let base_channel = Blake2b256Hash::from_bytes(vec![7u8; 32]);

        let result = merge(
            actual_seq,
            late_seq,
            |_a, _b| false, // depends
            |_r| 1,         // cost
            |_r| Ok(StateChange::empty()),
            |r| {
                let mut diff = BTreeMap::new();
                // item 1 decrements channel, item 2 increments channel
                let delta = if *r == 1 { -1 } else { 1 };
                diff.insert(base_channel.clone(), (delta, MergeType::IntegerAdd));
                diff
            },
            |_state_change, _channels| Ok(Vec::<HotStoreTrieAction<i32, i32, i32, i32>>::new()),
            |_actions: Vec<HotStoreTrieAction<i32, i32, i32, i32>>| {
                Ok(Blake2b256Hash::from_bytes(vec![9u8; 32]))
            },
            |_hash| Ok(Vec::new()),
            // Each item is its own singleton branch.
            |merge_set: &HashableSet<i32>| {
                HashableSet(
                    merge_set
                        .0
                        .iter()
                        .map(|i| {
                            let mut s = HashSet::new();
                            s.insert(*i);
                            HashableSet(s)
                        })
                        .collect(),
                )
            },
            // Empty conflict map — every branch as a key with no conflicts.
            // This test exercises only the rejection-via-mergeable-overflow
            // path; the conflict-detection path is covered elsewhere.
            |branches: &HashableSet<HashableSet<i32>>| {
                Ok(branches
                    .0
                    .iter()
                    .map(|b| (b.clone(), HashableSet(HashSet::new())))
                    .collect())
            },
        );

        assert!(result.is_ok());
        let (_new_state, rejected) = result.unwrap();
        assert!(!rejected.0.is_empty());
    }

    // ---- IntegerAdd overflow-launder regression (Phase 6 W3/W4) --------------
    // Two chains in the SAME branch contribute IntegerAdd diffs to one channel
    // whose sum overflows i64. The intra-branch combine must REJECT the branch
    // (return None) — "fail loudly" — rather than wrap the value and let it pass
    // the apply-time checked_add >= 0 gate with a wrong result (the launder).

    #[test]
    fn cal_merged_result_rejects_integer_add_overflow_launder() {
        let ch = Blake2b256Hash::from_bytes(vec![7u8; 32]);
        let br = branch(&[1, 2]); // both items in ONE branch
        let mergeable = |r: &i32| {
            let mut d = NumberChannelsDiff::new();
            let v = if *r == 1 { i64::MAX } else { 1 }; // MAX + 1 overflows
            d.insert(ch.clone(), (v, MergeType::IntegerAdd));
            d
        };
        assert_eq!(
            cal_merged_result(&br, HashMap::new(), mergeable),
            None,
            "combine overflow must reject the branch (no silent wrap / launder)"
        );
    }

    #[test]
    fn cal_merged_result_rejects_integer_add_true_launder_wraps_nonnegative() {
        // A DISCRIMINATING launder witness: three IntegerAdd diffs whose sum is 2^64,
        // which wraps to 0 — a NON-NEGATIVE value that would sail through the apply-time
        // `checked_add >= 0` gate if the combine used wrapping. Only the checked_add in
        // the combine fold (which overflows on MAX + MAX) rejects it. Contrast the
        // [MAX, 1] case above, whose wrap to i64::MIN is caught by the `>= 0` gate anyway
        // and so does NOT isolate the overflow check.
        let ch = Blake2b256Hash::from_bytes(vec![7u8; 32]);
        let br = branch(&[1, 2, 3]); // three chains in ONE branch
        let mergeable = |r: &i32| {
            let mut d = NumberChannelsDiff::new();
            let v = match *r {
                1 => i64::MAX,
                2 => i64::MAX,
                _ => 2, // MAX + MAX + 2 == 2^64 ≡ 0 (mod 2^64): wraps NON-NEGATIVE
            };
            d.insert(ch.clone(), (v, MergeType::IntegerAdd));
            d
        };
        assert_eq!(
            cal_merged_result(&br, HashMap::new(), mergeable),
            None,
            "a sum that wraps to a NON-NEGATIVE value must still be rejected by checked_add \
             in the combine (the >= 0 gate alone would not catch it)"
        );
    }

    #[test]
    fn cal_merged_result_accepts_non_overflowing_integer_add() {
        let ch = Blake2b256Hash::from_bytes(vec![7u8; 32]);
        let br = branch(&[1, 2]);
        let mergeable = |r: &i32| {
            let mut d = NumberChannelsDiff::new();
            let v = if *r == 1 { 100 } else { 23 };
            d.insert(ch.clone(), (v, MergeType::IntegerAdd));
            d
        };
        // 100 + 23 = 123, applied to base 0, >= 0 -> accepted with the TRUE sum.
        assert_eq!(
            cal_merged_result(&br, HashMap::new(), mergeable),
            Some(HashMap::from([(ch, 123)]))
        );
    }

    #[test]
    fn cal_merged_result_bitmask_or_never_rejects() {
        let ch = Blake2b256Hash::from_bytes(vec![7u8; 32]);
        let br = branch(&[1, 2]);
        let mergeable = |r: &i32| {
            let mut d = NumberChannelsDiff::new();
            let v = if *r == 1 { i64::MAX } else { 1 };
            d.insert(ch.clone(), (v, MergeType::BitmaskOr)); // OR never overflows
            d
        };
        let out = cal_merged_result(&br, HashMap::new(), mergeable);
        assert_eq!(out, Some(HashMap::from([(ch, i64::MAX)])));
    }

    // ---- Finding-A order-dependence guard (merge-algebra-verification.md §6) ----

    fn datum_state_change(channel: &[u8], added: &[&[u8]], removed: &[&[u8]]) -> StateChange {
        use rspace_plus_plus::rspace::merger::channel_change::ChannelChange;
        let sc = StateChange::empty();
        sc.datums_changes
            .insert(Blake2b256Hash(channel.to_vec()), ChannelChange {
                added: added.iter().map(|d| d.to_vec()).collect(),
                removed: removed.iter().map(|d| d.to_vec()).collect(),
            });
        sc
    }

    #[test]
    fn order_guard_silent_when_survivors_add_distinct_datums() {
        // Two survivors add DIFFERENT datums to one channel: max-fold == sum-fold.
        let mut guard = OrderDependenceGuard::default();
        guard.observe(&datum_state_change(b"chan", &[b"x"], &[]));
        guard.observe(&datum_state_change(b"chan", &[b"y"], &[]));
        assert_eq!(guard.first_offender(&HashSet::new()), None);
    }

    #[test]
    fn order_guard_silent_on_single_add_single_remove_netting() {
        // One survivor produces x, another consumes the base x on the same channel.
        // `cancel_common` nets them symmetrically — associative-safe, must NOT trip
        // (the guard checks distinct-survivor duplication, not "did cancel fire").
        let mut guard = OrderDependenceGuard::default();
        guard.observe(&datum_state_change(b"chan", &[b"x"], &[]));
        guard.observe(&datum_state_change(b"chan", &[], &[b"x"]));
        assert_eq!(guard.first_offender(&HashSet::new()), None);
    }

    #[test]
    fn order_guard_trips_when_two_survivors_add_same_datum() {
        // The same datum contributed to `added` by two distinct survivors — the
        // precondition under which the max-union fold != the sum-union fold.
        let mut guard = OrderDependenceGuard::default();
        guard.observe(&datum_state_change(b"chan", &[b"x"], &[]));
        guard.observe(&datum_state_change(b"chan", &[b"x"], &[]));
        assert_eq!(
            guard.first_offender(&HashSet::new()),
            Some(Blake2b256Hash(b"chan".to_vec()))
        );
    }

    #[test]
    fn order_guard_skips_mergeable_channels() {
        // Two producers on a NUMBER channel are safe: mergeable channels merge via
        // the commutative-group operator, not max-union. Must be excluded.
        let channel = Blake2b256Hash(b"num".to_vec());
        let mut guard = OrderDependenceGuard::default();
        guard.observe(&datum_state_change(b"num", &[b"x"], &[]));
        guard.observe(&datum_state_change(b"num", &[b"x"], &[]));
        let mergeable: HashSet<Blake2b256Hash> = HashSet::from([channel]);
        assert_eq!(guard.first_offender(&mergeable), None);
    }

    #[test]
    fn order_guard_is_permutation_invariant() {
        // The trip decision is a pure function of the survivor SET, independent of
        // observation order (locks guard (c): canonical fold order).
        let a = datum_state_change(b"chan", &[b"x"], &[]);
        let b = datum_state_change(b"chan", &[b"x"], &[]);
        let c = datum_state_change(b"other", &[b"z"], &[]);
        let mut forward = OrderDependenceGuard::default();
        forward.observe(&a);
        forward.observe(&b);
        forward.observe(&c);
        let mut reverse = OrderDependenceGuard::default();
        reverse.observe(&c);
        reverse.observe(&b);
        reverse.observe(&a);
        assert_eq!(
            forward.first_offender(&HashSet::new()),
            reverse.first_offender(&HashSet::new())
        );
        assert_eq!(
            forward.first_offender(&HashSet::new()),
            Some(Blake2b256Hash(b"chan".to_vec()))
        );
    }
}
