use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::casper::protocol::casper_message::Event;
use prost::bytes::Bytes;
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rholang::rust::interpreter::rho_runtime::RhoHistoryRepository;
use rspace_plus_plus::rspace::errors::HistoryError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::history_reader::HistoryReader;
use rspace_plus_plus::rspace::merger::merging_logic::{
    self, NumberChannelsDiff, NumberChannelsEndVal,
};
use rspace_plus_plus::rspace::merger::state_change::StateChange;
use rspace_plus_plus::rspace::merger::state_change_merger;
use shared::rust::hashable_set::HashableSet;

use super::body::StateHash;
use crate::rust::errors::CasperError;
use crate::rust::merging::block_index::create_event_log_index;
use crate::rust::merging::conflict_set_merger::{compute_merged_state, ResolvedConflicts};
use crate::rust::merging::deploy_chain_index::DeployChainIndex;
use crate::rust::merging::deploy_index::DeployIndex;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

type HistoryReaderOf = Arc<
    Box<dyn HistoryReader<Blake2b256Hash, Par, BindPattern, ListParWithRandom, TaggedContinuation>>,
>;

pub struct RunEffect<'a> {
    pub envelope_id: Bytes,
    pub user_log: &'a [Event],
    pub post_root: [u8; 32],
    pub number_channels: &'a NumberChannelsEndVal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Merged {
    pub mid_root: StateHash,
    pub merged: Vec<bool>,
}

fn history(error: HistoryError) -> CasperError { CasperError::HistoryError(error) }

pub fn merge_runs(
    runtime_manager: &RuntimeManager,
    pre_root: &StateHash,
    effects: &[RunEffect<'_>],
) -> Result<Merged, CasperError> {
    let base = Blake2b256Hash::from_bytes_prost(pre_root);
    let repository = runtime_manager.history_repo.clone();
    let diffs = runtime_manager.convert_number_channels_to_diff(
        effects
            .iter()
            .map(|effect| effect.number_channels.clone())
            .collect(),
        &base,
    )?;
    if diffs.len() != effects.len() {
        return Err(CasperError::RuntimeError(
            "the number-channel diffs do not match the runs".to_string(),
        ));
    }
    let reader = Arc::new(repository.get_history_reader(&base).map_err(history)?);
    let mut survivors: Vec<DeployChainIndex> = Vec::new();
    let mut merged = Vec::with_capacity(effects.len());
    for (effect, diff) in effects.iter().zip(diffs) {
        let index = DeployIndex {
            deploy_id: effect.envelope_id.clone(),
            cost: 0,
            event_log_index: create_event_log_index(
                effect.user_log,
                repository.clone(),
                &base,
                diff,
            ),
        };
        let chain = DeployChainIndex::new(
            &HashableSet(HashSet::from([index])),
            &base,
            &Blake2b256Hash::from_bytes(effect.post_root.to_vec()),
            repository.clone(),
            Bytes::new(),
            0,
            HashMap::new(),
        )
        .map_err(history)?;
        let conflicts = survivors.iter().any(|kept| {
            merging_logic::are_conflicting(&kept.event_log_index, &chain.event_log_index)
        });
        if conflicts {
            merged.push(false);
            continue;
        }
        survivors.push(chain);
        match merged_root(&survivors, &reader, &repository, &base, false) {
            Ok(_) => merged.push(true),
            Err(HistoryError::MergeError(_)) => {
                survivors.pop();
                merged.push(false);
            }
            Err(error) => return Err(history(error)),
        }
    }
    if survivors.is_empty() {
        return Ok(Merged {
            mid_root: pre_root.clone(),
            merged,
        });
    }
    let root = merged_root(&survivors, &reader, &repository, &base, true).map_err(history)?;
    Ok(Merged {
        mid_root: root.to_bytes_prost(),
        merged,
    })
}

fn merged_root(
    survivors: &[DeployChainIndex],
    reader: &HistoryReaderOf,
    repository: &RhoHistoryRepository,
    base: &Blake2b256Hash,
    apply: bool,
) -> Result<Blake2b256Hash, HistoryError> {
    let multi_writer: HashSet<Blake2b256Hash> = {
        let mut writers: HashMap<Blake2b256Hash, usize> = HashMap::new();
        for chain in survivors {
            for entry in chain.state_changes.datums_changes.iter() {
                if !entry.value().added.is_empty() {
                    *writers.entry(entry.key().clone()).or_insert(0) += 1;
                }
            }
        }
        writers
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(channel, _)| channel)
            .collect()
    };
    let resolved = ResolvedConflicts {
        to_merge: survivors
            .iter()
            .cloned()
            .map(|chain| HashableSet(HashSet::from([chain])))
            .collect(),
        rejected: HashableSet(HashSet::new()),
        late_set_size: 0,
        actual_set_size: 0,
        branches_count: 0,
        rejected_as_dependents_count: 0,
        optimal_rejection_count: 0,
        conflict_map_conflicts_count: 0,
        rejection_options_count: 0,
        branches_time: Duration::ZERO,
        conflicts_map_time: Duration::ZERO,
        rejection_options_time: Duration::ZERO,
    };
    let state_changes = |chain: &DeployChainIndex| Ok((*chain.state_changes).clone());
    let source_of = |chain: &DeployChainIndex| {
        chain
            .deploys_with_cost
            .0
            .iter()
            .map(|deploy| deploy.deploy_id.clone())
            .next()
            .unwrap_or_default()
    };
    let mergeable = |chain: &DeployChainIndex| chain.event_log_index.number_channels_data.clone();
    let trie_actions = |changes: StateChange, number_channels: NumberChannelsDiff| {
        state_change_merger::compute_trie_actions(
            &changes,
            &**reader,
            &number_channels,
            |hash: &Blake2b256Hash, channel_changes, number_chs: &NumberChannelsDiff| {
                if let Some((diff, merge_type)) = number_chs.get(hash) {
                    return Ok(Some(RholangMergingLogic::calculate_number_channel_merge(
                        hash,
                        diff,
                        *merge_type,
                        channel_changes,
                        |h: &Blake2b256Hash| reader.get_data(h),
                    )?));
                }
                if !channel_changes.added.is_empty() {
                    let base_data = reader.get_data(hash)?;
                    if !base_data.is_empty() || multi_writer.contains(hash) {
                        let base_binary = reader.get_data_proj_binary(hash)?;
                        RholangMergingLogic::check_single_value_cell_not_overfilled(
                            hash,
                            &base_data,
                            &base_binary,
                            channel_changes,
                        )?;
                    }
                }
                Ok(None)
            },
        )
    };
    let write = |actions| match apply {
        true => repository
            .reset(base)
            .map(|reset| reset.do_checkpoint(actions))
            .map(|checkpoint| checkpoint.root()),
        false => Ok(base.clone()),
    };
    compute_merged_state(
        &resolved,
        &state_changes,
        &source_of,
        &mergeable,
        &trie_actions,
        &write,
    )
}
