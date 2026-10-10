//! DR-120: composition of a resolved v6 merge (MEDIUM-3, LOW-3).
//!
//! The guarded trie actions and the multi-writer set are copied from
//! `dag_merger::merge`, not moved, so dev's merge keeps its code. The copies
//! call only public primitives. The test
//! `fast_path_equals_dev_merge_when_dev_rejects_nothing` fails if dev's
//! originals drift.

use std::collections::{BTreeSet, HashMap, HashSet};

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rholang::rust::interpreter::rho_runtime::RhoHistoryRepository;
use rspace_plus_plus::rspace::errors::HistoryError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hot_store_trie_action::HotStoreTrieAction;
use rspace_plus_plus::rspace::merger::channel_change::ChannelChange;
use rspace_plus_plus::rspace::merger::merging_logic::NumberChannelsDiff;
use rspace_plus_plus::rspace::merger::state_change::StateChange;
use rspace_plus_plus::rspace::merger::state_change_merger;
use shared::rust::hashable_set::HashableSet;

use super::RhoHistoryReader;
use crate::rust::errors::CasperError;
use crate::rust::merging::conflict_set_merger::{self, compare_branches, ResolvedConflicts};
use crate::rust::merging::deploy_chain_index::DeployChainIndex;

type RhoTrieAction = HotStoreTrieAction<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

/// Copy of the apply callback of dag_merger.rs:1539-1576: the number-channel
/// override, and the single-value-cell guard for every other channel.
pub(crate) fn guarded_channel_action(
    reader: &RhoHistoryReader,
    hash: &Blake2b256Hash,
    channel_changes: &ChannelChange<Vec<u8>>,
    number_chs: &NumberChannelsDiff,
    multi_writer_channels: &HashSet<Blake2b256Hash>,
) -> Result<Option<RhoTrieAction>, HistoryError> {
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
}

/// Copy of dag_merger.rs:2007-2023: the channels where more than one kept
/// chain adds datums.
pub(crate) fn multi_writer_channel_set<'a>(
    to_merge: impl IntoIterator<Item = &'a HashableSet<DeployChainIndex>>,
) -> HashSet<Blake2b256Hash> {
    let mut writer_counts: HashMap<Blake2b256Hash, usize> = HashMap::new();
    for branch in to_merge {
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
}

/// Applies the kept branches of `resolved` to the base state, as
/// dag_merger.rs:2024-2051 does with the copies above, the reader of
/// dag_merger.rs:1447-1451 and the apply closure of dag_merger.rs:1595-1601.
pub(crate) fn compose(
    history_repository: &RhoHistoryRepository,
    base_post_state: &Blake2b256Hash,
    resolved: &ResolvedConflicts<DeployChainIndex>,
) -> Result<Blake2b256Hash, CasperError> {
    let history_reader = history_repository
        .get_history_reader(base_post_state)
        .map_err(CasperError::HistoryError)?;
    let multi_writer_channels = multi_writer_channel_set(&resolved.to_merge);
    let compute_trie_actions_fn = |changes: StateChange, mergeable_chs: NumberChannelsDiff| {
        state_change_merger::compute_trie_actions(
            &changes,
            &history_reader,
            &mergeable_chs,
            |hash: &Blake2b256Hash, channel_changes, number_chs: &NumberChannelsDiff| {
                guarded_channel_action(
                    &history_reader,
                    hash,
                    channel_changes,
                    number_chs,
                    &multi_writer_channels,
                )
            },
        )
    };
    let apply_trie_actions_fn = |actions| {
        history_repository
            .reset(base_post_state)
            .map(|reset_repo| reset_repo.do_checkpoint(actions))
            .map(|checkpoint| checkpoint.root())
            .map_err(|e| e.into())
    };
    conflict_set_merger::compute_merged_state(
        resolved,
        &|chain: &DeployChainIndex| Ok(chain.state_changes.clone()),
        &|chain: &DeployChainIndex| chain.event_log_index.number_channels_data.clone(),
        &compute_trie_actions_fn,
        &apply_trie_actions_fn,
    )
    .map_err(CasperError::HistoryError)
}

/// LOW-3: a dry run of the apply-time guard over `kept`. It folds the kept
/// chains' changes with `StateChange::combine` in compose's canonical order
/// (conflict_set_merger.rs:458-468), which normalizes each channel through
/// channel_change.rs:17-41, and then applies the guard branch of
/// `guarded_channel_action`. It returns the first channel, in channel order,
/// on which compose would fail the guard.
pub(crate) fn first_overfill(
    kept: &[&HashableSet<DeployChainIndex>],
    reader: &RhoHistoryReader,
) -> Result<Option<Blake2b256Hash>, HistoryError> {
    let mut sorted: Vec<&HashableSet<DeployChainIndex>> = kept.to_vec();
    sorted.sort_by(|a, b| compare_branches(a, b));
    let mut combined = StateChange::empty();
    let mut folded: BTreeSet<&Blake2b256Hash> = BTreeSet::new();
    for branch in sorted {
        let mut items: Vec<&DeployChainIndex> = branch.0.iter().collect();
        items.sort();
        for item in items {
            combined = combined.combine(item.state_changes.clone());
            folded.extend(item.event_log_index.number_channels_data.keys());
        }
    }
    let multi_writer_channels = multi_writer_channel_set(kept.iter().copied());
    for entry in combined.datums_changes.iter() {
        let (hash, changes) = (entry.key(), entry.value());
        if folded.contains(hash) || changes.added.is_empty() {
            continue;
        }
        let base = reader.get_data(hash)?;
        if !base.is_empty() || multi_writer_channels.contains(hash) {
            let base_bin = reader.get_data_proj_binary(hash)?;
            let guard = RholangMergingLogic::check_single_value_cell_not_overfilled(
                hash, &base, &base_bin, changes,
            );
            if guard.is_err() {
                return Ok(Some(hash.clone()));
            }
        }
    }
    Ok(None)
}
