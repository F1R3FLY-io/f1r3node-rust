// See casper/src/main/scala/coop/rchain/casper/util/rholang/InterpreterUtil.scala

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::Par;
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Bond, DeployData, ProcessedDeploy, ProcessedSystemDeploy, RejectedDeploy,
    RejectedDeployReason, StateEffectId, SystemDeployData,
};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::Either;

use super::replay_failure::ReplayFailure;
use super::runtime_manager::RuntimeManager;
use crate::rust::api::deploy_finalization_status::{self, DeployFinalizationState};
use crate::rust::block_status::BlockStatus;
use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;
use crate::rust::merging::block_index::BlockIndex;
use crate::rust::merging::dag_merger;
use crate::rust::merging::deploy_chain_index::DeployChainIndex;
use crate::rust::metrics_constants::{BLOCK_PROCESSING_REPLAY_TIME_METRIC, CASPER_METRICS_SOURCE};
use crate::rust::util::proto_util;
use crate::rust::BlockProcessing;

pub const EXACT_REJECTION_PROTOCOL_VERSION: i64 = 2;

fn state_effect_encoding_matches_protocol(
    protocol_version: i64,
    encoded: &[StateEffectId],
    computed: &[StateEffectId],
) -> bool {
    if protocol_version < crate::rust::casper::STATE_EFFECT_PROVENANCE_PROTOCOL_VERSION {
        encoded.is_empty()
    } else {
        encoded.windows(2).all(|pair| pair[0] < pair[1]) && encoded == computed
    }
}

pub fn mk_term(rho: &str, normalizer_env: HashMap<String, Par>) -> Result<Par, InterpreterError> {
    Compiler::source_to_adt_with_normalizer_env(rho, normalizer_env)
}

/// Sigs with at least one active occurrence across the full parent closure.
/// Exact rejection records remove only their named source; legacy records retain
/// their historical latest-height behavior. Walking all parents catches surviving
/// co-parent occurrences that a main-parent-only walk would miss.
pub fn canonical_won_sigs(
    block_store: &KeyValueBlockStore,
    parents: &[BlockHash],
    earliest_block_number: i64,
) -> Result<HashSet<Bytes>, CasperError> {
    canonical_disposition_sets(block_store, parents, earliest_block_number).map(|sets| sets.0)
}

pub fn canonical_rejected_sigs(
    block_store: &KeyValueBlockStore,
    parents: &[BlockHash],
    earliest_block_number: i64,
) -> Result<HashSet<Bytes>, CasperError> {
    canonical_disposition_sets(block_store, parents, earliest_block_number).map(|sets| sets.1)
}

pub fn canonical_disposition_sets(
    block_store: &KeyValueBlockStore,
    parents: &[BlockHash],
    earliest_block_number: i64,
) -> Result<(HashSet<Bytes>, HashSet<Bytes>), CasperError> {
    let dispositions = canonical_dispositions(block_store, parents, earliest_block_number)?;
    let mut won = HashSet::new();
    let mut rejected = HashSet::new();
    for (sig, disposition) in dispositions {
        if disposition.won {
            won.insert(sig);
        } else {
            rejected.insert(sig);
        }
    }
    Ok((won, rejected))
}

pub fn canonical_won_sigs_at_floor(
    block_store: &KeyValueBlockStore,
    finalized_floor: &BlockHash,
    parents: &[BlockHash],
    earliest_block_number: i64,
    protocol_version: i64,
) -> Result<HashSet<Bytes>, CasperError> {
    canonical_disposition_sets_at_floor(
        block_store,
        finalized_floor,
        parents,
        earliest_block_number,
        protocol_version,
    )
    .map(|sets| sets.0)
}

pub fn canonical_disposition_sets_at_floor(
    block_store: &KeyValueBlockStore,
    finalized_floor: &BlockHash,
    parents: &[BlockHash],
    earliest_block_number: i64,
    protocol_version: i64,
) -> Result<(HashSet<Bytes>, HashSet<Bytes>), CasperError> {
    let mut visible = canonical_disposition_sets(block_store, parents, earliest_block_number)?;
    if protocol_version < EXACT_REJECTION_PROTOCOL_VERSION {
        return Ok(visible);
    }

    let floor = canonical_disposition_sets(
        block_store,
        std::slice::from_ref(finalized_floor),
        earliest_block_number,
    )?;
    visible.0.extend(floor.0.iter().cloned());
    visible.1.retain(|sig| !floor.0.contains(sig));
    Ok(visible)
}

fn block_in_floor_merge_scope(
    dag: &KeyValueDagRepresentation,
    hash: &BlockHash,
    floor_hash: &BlockHash,
    floor_block_number: i64,
) -> Result<bool, CasperError> {
    let meta = dag.lookup_unsafe(hash)?;
    if meta.block_number > floor_block_number {
        return Ok(true);
    }
    Ok(!dag.is_dag_ancestor(hash, floor_hash)?)
}

fn missing_disposition_block_error(context: &str, block_hash: &BlockHash) -> CasperError {
    CasperError::RuntimeError(format!(
        "Missing block {} while {}",
        PrettyPrinter::build_string_bytes(block_hash),
        context
    ))
}

#[derive(Default)]
struct SigEvents {
    occurrences: HashMap<BlockHash, i64>,
    exact_rejections: HashMap<BlockHash, i64>,
    latest_legacy_rejection: Option<i64>,
}

#[derive(Clone, Debug)]
struct ScopeDisposition {
    height: i64,
    won: bool,
    active_sources: HashSet<BlockHash>,
}

fn record_scope_events(
    events: &mut HashMap<Bytes, SigEvents>,
    source_hash: &BlockHash,
    block: &BlockMessage,
) -> Result<(), CasperError> {
    let exact_protocol = block.header.version >= EXACT_REJECTION_PROTOCOL_VERSION;
    let valid_encoding = block.body.rejected_deploys.iter().all(|rejected| {
        if exact_protocol {
            rejected.has_provenance() && rejected.reason != RejectedDeployReason::Unspecified
        } else {
            !rejected.has_provenance() && rejected.reason == RejectedDeployReason::Unspecified
        }
    });
    if !valid_encoding {
        return Err(CasperError::RuntimeError(format!(
            "block {} has rejected-deploy encoding incompatible with protocol {}",
            PrettyPrinter::build_string_bytes(source_hash),
            block.header.version,
        )));
    }
    let block_number = block.body.state.block_number;
    for processed in &block.body.deploys {
        events
            .entry(processed.deploy.sig.clone())
            .or_default()
            .occurrences
            .entry(source_hash.clone())
            .and_modify(|height| *height = (*height).max(block_number))
            .or_insert(block_number);
    }
    for rejected in &block.body.rejected_deploys {
        let sig_events = events.entry(rejected.sig.clone()).or_default();
        if rejected.has_provenance() {
            sig_events
                .exact_rejections
                .entry(rejected.source_block_hash.clone())
                .and_modify(|height| *height = (*height).max(block_number))
                .or_insert(block_number);
        } else {
            sig_events.latest_legacy_rejection = Some(
                sig_events
                    .latest_legacy_rejection
                    .map_or(block_number, |height| height.max(block_number)),
            );
        }
    }
    Ok(())
}

fn reduce_scope_events(events: HashMap<Bytes, SigEvents>) -> HashMap<Bytes, ScopeDisposition> {
    events
        .into_iter()
        .map(|(sig, sig_events)| {
            let active: Vec<_> = sig_events
                .occurrences
                .iter()
                .filter(|(source, height)| {
                    !sig_events.exact_rejections.contains_key(*source)
                        && sig_events
                            .latest_legacy_rejection
                            .is_none_or(|legacy_height| **height > legacy_height)
                })
                .collect();
            let disposition = if active.is_empty() {
                let height = sig_events
                    .exact_rejections
                    .values()
                    .copied()
                    .chain(sig_events.latest_legacy_rejection)
                    .max()
                    .unwrap_or(i64::MIN);
                ScopeDisposition {
                    height,
                    won: false,
                    active_sources: HashSet::new(),
                }
            } else {
                ScopeDisposition {
                    height: active
                        .iter()
                        .map(|(_, height)| **height)
                        .max()
                        .unwrap_or(i64::MIN),
                    won: true,
                    active_sources: active
                        .into_iter()
                        .map(|(source, _)| source.clone())
                        .collect(),
                }
            };
            (sig, disposition)
        })
        .collect()
}

/// BFS-walk the closure of `parents` down to `earliest_block_number` and
/// reduce exact source tombstones before projecting each deploy signature.
fn canonical_dispositions(
    block_store: &KeyValueBlockStore,
    parents: &[BlockHash],
    earliest_block_number: i64,
) -> Result<HashMap<Bytes, ScopeDisposition>, CasperError> {
    let mut events: HashMap<Bytes, SigEvents> = HashMap::new();
    let mut visited: HashSet<BlockHash> = HashSet::new();
    let mut queue: VecDeque<BlockHash> = parents.iter().cloned().collect();
    while let Some(hash) = queue.pop_front() {
        if !visited.insert(hash.clone()) {
            continue;
        }
        let block = block_store.get(&hash)?.ok_or_else(|| {
            CasperError::RuntimeError(format!(
                "Missing block {} while reducing deploy occurrence dispositions",
                PrettyPrinter::build_string_bytes(&hash)
            ))
        })?;
        let bn = block.body.state.block_number;
        if bn < earliest_block_number {
            continue;
        }
        record_scope_events(&mut events, &hash, &block)?;
        for p in &block.header.parents_hash_list {
            queue.push_back(p.clone());
        }
    }
    Ok(reduce_scope_events(events))
}

fn visible_rejections_threatening_finalized(
    block_store: &KeyValueBlockStore,
    parents: &[BlockHash],
    earliest_block_number: i64,
    finalized: &HashMap<Bytes, ScopeDisposition>,
) -> Result<HashSet<Bytes>, CasperError> {
    let mut threatened = HashSet::new();
    let mut visited: HashSet<BlockHash> = HashSet::new();
    let mut queue: VecDeque<BlockHash> = parents.iter().cloned().collect();
    while let Some(hash) = queue.pop_front() {
        if !visited.insert(hash.clone()) {
            continue;
        }
        let block = block_store.get(&hash)?.ok_or_else(|| {
            missing_disposition_block_error("checking finalized-source threats", &hash)
        })?;
        let bn = block.body.state.block_number;
        if bn < earliest_block_number {
            continue;
        }
        for rd in &block.body.rejected_deploys {
            let Some(disposition) = finalized.get(&rd.sig).filter(|value| value.won) else {
                continue;
            };
            let threatens = if rd.has_provenance() {
                disposition.active_sources.contains(&rd.source_block_hash)
            } else {
                bn >= disposition.height
            };
            if threatens {
                threatened.insert(rd.sig.clone());
            }
        }
        for p in &block.header.parents_hash_list {
            queue.push_back(p.clone());
        }
    }
    Ok(threatened)
}

/// Sigs whose rejected-buffer entry is terminal: finalized ancestry has an
/// active source and no visible exact tombstone targets that source. Legacy
/// rejection records use their historical height comparison.
pub fn finalized_won_terminal_sigs(
    block_store: &KeyValueBlockStore,
    last_finalized_block: &BlockHash,
    visible_parents: &[BlockHash],
    earliest_block_number: i64,
    protocol_version: i64,
) -> Result<HashSet<Bytes>, CasperError> {
    block_store.get(last_finalized_block)?.ok_or_else(|| {
        missing_disposition_block_error("checking finalized terminal deploys", last_finalized_block)
    })?;
    let finalized = canonical_dispositions(
        block_store,
        std::slice::from_ref(last_finalized_block),
        earliest_block_number,
    )?;
    if protocol_version >= EXACT_REJECTION_PROTOCOL_VERSION {
        return Ok(finalized
            .into_iter()
            .filter_map(|(sig, disposition)| disposition.won.then_some(sig))
            .collect());
    }
    let visible_rejections = visible_rejections_threatening_finalized(
        block_store,
        visible_parents,
        earliest_block_number,
        &finalized,
    )?;
    Ok(finalized
        .into_iter()
        .filter_map(|(sig, disposition)| {
            let terminal = disposition.won && !visible_rejections.contains(&sig);
            terminal.then_some(sig)
        })
        .collect())
}

fn rejected_sig_has_visible_non_source_win(
    block_store: &KeyValueBlockStore,
    visible_blocks: &HashSet<BlockHash>,
    sig: &Bytes,
    source_block: &BlockHash,
) -> Result<bool, CasperError> {
    let mut events = HashMap::new();
    for hash in visible_blocks {
        if hash == source_block {
            continue;
        }
        let block = block_store.get(hash)?.ok_or_else(|| {
            missing_disposition_block_error("checking visible non-source wins", hash)
        })?;
        record_scope_events(&mut events, hash, &block)?;
    }
    Ok(reduce_scope_events(events)
        .get(sig)
        .map(|disposition| disposition.won)
        .unwrap_or(false))
}

#[cfg(test)]
fn visible_rejected_deploy_sigs(
    block_store: &KeyValueBlockStore,
    visible_blocks: &HashSet<BlockHash>,
) -> Result<HashSet<Bytes>, CasperError> {
    Ok(
        visible_rejected_deploy_latest_heights(block_store, visible_blocks)?
            .into_keys()
            .collect(),
    )
}

#[cfg(test)]
fn visible_rejected_deploy_latest_heights(
    block_store: &KeyValueBlockStore,
    visible_blocks: &HashSet<BlockHash>,
) -> Result<HashMap<Bytes, i64>, CasperError> {
    let mut latest: HashMap<Bytes, i64> = HashMap::new();
    for hash in visible_blocks {
        let block = block_store.get(hash)?.ok_or_else(|| {
            missing_disposition_block_error("collecting visible rejected deploys", hash)
        })?;
        for rd in &block.body.rejected_deploys {
            latest
                .entry(rd.sig.clone())
                .and_modify(|height| *height = (*height).max(block.body.state.block_number))
                .or_insert(block.body.state.block_number);
        }
    }
    Ok(latest)
}

fn suppress_rejected_pairs_with_later_visible_rejection(
    block_store: &KeyValueBlockStore,
    visible_blocks: &HashSet<BlockHash>,
    rejected_user_pairs: &mut Vec<RejectedDeploy>,
) -> Result<usize, CasperError> {
    if rejected_user_pairs.is_empty() {
        return Ok(0);
    }

    let mut visible_rejections: HashSet<(Bytes, BlockHash)> = HashSet::new();
    for hash in visible_blocks {
        let block = block_store.get(hash)?.ok_or_else(|| {
            missing_disposition_block_error("suppressing repeated exact rejections", hash)
        })?;
        for rejected in &block.body.rejected_deploys {
            if rejected.has_provenance() {
                visible_rejections
                    .insert((rejected.sig.clone(), rejected.source_block_hash.clone()));
            }
        }
    }
    if visible_rejections.is_empty() {
        return Ok(0);
    }

    let before = rejected_user_pairs.len();
    rejected_user_pairs.retain(|rejected| {
        !visible_rejections.contains(&(rejected.sig.clone(), rejected.source_block_hash.clone()))
    });
    Ok(before.saturating_sub(rejected_user_pairs.len()))
}

fn merge_occurrence_context(
    block_store: &KeyValueBlockStore,
    dag: &KeyValueDagRepresentation,
    floor_hash: &BlockHash,
    visible_blocks: &HashSet<BlockHash>,
    protocol_version: i64,
) -> Result<dag_merger::MergeOccurrenceContext, CasperError> {
    if protocol_version < EXACT_REJECTION_PROTOCOL_VERSION {
        return Ok(dag_merger::MergeOccurrenceContext::default());
    }

    let base_committed_sigs =
        canonical_won_sigs(block_store, std::slice::from_ref(floor_hash), i64::MIN)?;
    let mut scope_tombstones: BTreeMap<(Bytes, BlockHash), RejectedDeployReason> = BTreeMap::new();
    let mut recording_blocks: Vec<_> = visible_blocks.iter().cloned().collect();
    recording_blocks.sort();
    for recording_hash in recording_blocks {
        let recording_block = block_store.get(&recording_hash)?.ok_or_else(|| {
            missing_disposition_block_error(
                "deriving authoritative merge tombstones",
                &recording_hash,
            )
        })?;
        if recording_block.header.version != protocol_version {
            return Err(CasperError::RuntimeError(format!(
                "merge scope mixes protocol versions {} and {} at block {}",
                protocol_version,
                recording_block.header.version,
                PrettyPrinter::build_string_bytes(&recording_hash),
            )));
        }
        for rejected in &recording_block.body.rejected_deploys {
            if !rejected.has_provenance() || rejected.reason == RejectedDeployReason::Unspecified {
                return Err(CasperError::RuntimeError(format!(
                    "protocol {} block {} contains a legacy or reasonless rejected-deploy record",
                    protocol_version,
                    PrettyPrinter::build_string_bytes(&recording_hash),
                )));
            }
            if !visible_blocks.contains(&rejected.source_block_hash) {
                continue;
            }
            if !dag.is_dag_ancestor(&rejected.source_block_hash, &recording_hash)? {
                return Err(CasperError::RuntimeError(format!(
                    "rejected-deploy record in {} is not causally descended from source {}",
                    PrettyPrinter::build_string_bytes(&recording_hash),
                    PrettyPrinter::build_string_bytes(&rejected.source_block_hash),
                )));
            }
            let source_block = block_store
                .get(&rejected.source_block_hash)?
                .ok_or_else(|| {
                    missing_disposition_block_error(
                        "validating a merge tombstone source",
                        &rejected.source_block_hash,
                    )
                })?;
            let source_contains_sig = source_block
                .body
                .deploys
                .iter()
                .any(|processed| processed.deploy.sig == rejected.sig);
            if !source_contains_sig {
                return Err(CasperError::RuntimeError(format!(
                    "rejected-deploy record in {} names signature {} absent from source {}",
                    PrettyPrinter::build_string_bytes(&recording_hash),
                    PrettyPrinter::build_string_bytes(&rejected.sig),
                    PrettyPrinter::build_string_bytes(&rejected.source_block_hash),
                )));
            }
            let key = (rejected.sig.clone(), rejected.source_block_hash.clone());
            match scope_tombstones.entry(key) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let reason = entry.get().canonical_join(rejected.reason);
                    entry.insert(reason);
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(rejected.reason);
                }
            }
        }
    }

    Ok(dag_merger::MergeOccurrenceContext {
        base_committed_sigs,
        scope_tombstones,
        require_exact_effects: true,
    })
}

fn retain_pending_rejected_deploys_for_buffer(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    deploy_lifespan: i64,
    deploys: &mut Vec<Signed<DeployData>>,
    rejecting_block_number: i64,
    protocol_version: i64,
) -> Result<usize, CasperError> {
    if deploys.is_empty() {
        return Ok(0);
    }

    let sigs: HashSet<Bytes> = deploys.iter().map(|deploy| deploy.sig.clone()).collect();
    let statuses =
        deploy_finalization_status::resolve_batch(dag, block_store, deploy_lifespan, &sigs)
            .map_err(|err| {
                CasperError::RuntimeError(format!(
                    "RejectedDeployBuffer finalization-status filter failed: {}",
                    err
                ))
            })?;

    let finalized_floor = dag.last_finalized_block();
    block_store.get(&finalized_floor)?.ok_or_else(|| {
        missing_disposition_block_error(
            "filtering finalized rejected-buffer entries",
            &finalized_floor,
        )
    })?;
    let exact_protocol = protocol_version >= EXACT_REJECTION_PROTOCOL_VERSION;
    let scan_floor = deploys
        .iter()
        .map(|d| d.data.valid_after_block_number)
        .min()
        .unwrap_or(rejecting_block_number)
        .min(rejecting_block_number);
    let finalized_dispositions = canonical_dispositions(
        block_store,
        std::slice::from_ref(&finalized_floor),
        scan_floor,
    )?;

    let before = deploys.len();
    deploys.retain(|deploy| {
        let state = statuses.get(&deploy.sig).map(|status| status.state);
        match state {
            None | Some(DeployFinalizationState::Pending) => true,
            Some(DeployFinalizationState::Finalized) => {
                match finalized_dispositions.get(&deploy.sig) {
                    Some(disposition) if disposition.won => {
                        !exact_protocol && disposition.height <= rejecting_block_number
                    }
                    disagreement => {
                        // The resolver and the finalized-chain disposition scan
                        // disagree: `Finalized` without a winning finalized
                        // disposition (or none at all). Retaining is the safe
                        // direction — the buffer holds the only re-proposable
                        // copy — but the disagreement itself is the resolver's
                        // known blindness to rejections above the LFB, so make
                        // it visible rather than silent.
                        tracing::warn!(
                            target: "f1r3fly.casper.recovery",
                            "RejectedDeployBuffer populate: resolver reports Finalized for {} but the \
                             finalized-chain disposition is {:?} (rejecting block #{}); retaining",
                            PrettyPrinter::build_string_bytes(&deploy.sig),
                            disagreement,
                            rejecting_block_number
                        );
                        true
                    }
                }
            }
            Some(_) => false,
        }
    });
    Ok(before - deploys.len())
}

// Returns (None, checkpoints) if the block's tuplespace hash
// does not match the computed hash based on the deploys
pub async fn validate_block_pre_state(
    block: &BlockMessage,
    block_store: &KeyValueBlockStore,
    s: &mut CasperSnapshot,
    runtime_manager: &RuntimeManager,
    rejected_deploy_buffer: Option<&std::sync::Arc<std::sync::Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>>,
) -> Result<BlockProcessing<Option<StateHash>>, CasperError> {
    tracing::trace!(target: "f1r3fly.casper.block_validation", "before-unsafe-get-parents");
    let incoming_pre_state_hash = proto_util::pre_state_hash(block);
    let parents = proto_util::get_parents(block_store, block);
    tracing::debug!(target: "f1r3fly.casper.block_validation", block = %hex::encode(&block.block_hash[..8.min(block.block_hash.len())]), seq = block.seq_num, n_parents = parents.len(), "validate.block_checkpoint ENTER (recompute parents post-state, then replay)");
    tracing::trace!(target: "f1r3fly.casper.block_validation", parent_count = parents.len(), "before-compute-parents-post-state");
    let parents_post_state_start = std::time::Instant::now();
    // Validate: the floor must be derived from the BLOCK's own recorded
    // justifications (node-identical), not the validating node's live view.
    let latest_messages: BTreeMap<Validator, BlockHash> = block
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let computed_parents_info = compute_parents_post_state_with_effects(
        block_store,
        parents.clone(),
        s,
        runtime_manager,
        &latest_messages,
        None,
        rejected_deploy_buffer,
    )
    .await;
    metrics::histogram!(
        crate::rust::metrics_constants::BLOCK_PROCESSING_PARENTS_POST_STATE_TIME_METRIC,
        "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
    )
    .record(parents_post_state_start.elapsed().as_secs_f64());

    tracing::info!(
        "Computed parents post state for {}.",
        PrettyPrinter::build_string_block_message(block, false)
    );

    match computed_parents_info {
        Ok((computed_pre_state_hash, rejected_deploys, rejected_state_effects)) => {
            let computed_rejections: std::collections::BTreeSet<_> =
                rejected_deploys.iter().cloned().collect();
            let block_rejections: std::collections::BTreeSet<_> =
                block.body.rejected_deploys.iter().cloned().collect();
            let exact_protocol = block.header.version >= EXACT_REJECTION_PROTOCOL_VERSION;
            let encoding_matches_protocol = if exact_protocol {
                block_rejections.iter().all(|rejected| {
                    rejected.has_provenance()
                        && rejected.reason != RejectedDeployReason::Unspecified
                })
            } else {
                block_rejections.iter().all(|rejected| {
                    !rejected.has_provenance()
                        && rejected.reason == RejectedDeployReason::Unspecified
                })
            };
            let rejections_match = encoding_matches_protocol
                && if !exact_protocol {
                    let block_deploy_sigs: HashSet<_> = block
                        .body
                        .deploys
                        .iter()
                        .map(|pd| pd.deploy.sig.clone())
                        .collect();
                    let computed_sigs: HashSet<_> = computed_rejections
                        .iter()
                        .filter(|rejected| !block_deploy_sigs.contains(&rejected.sig))
                        .map(|rejected| rejected.sig.clone())
                        .collect();
                    let block_sigs: HashSet<_> =
                        block_rejections.iter().map(|r| r.sig.clone()).collect();
                    computed_sigs == block_sigs
                } else {
                    computed_rejections == block_rejections
                };
            let state_effects_match = state_effect_encoding_matches_protocol(
                block.header.version,
                &block.body.rejected_state_effects,
                &rejected_state_effects,
            );

            if incoming_pre_state_hash != computed_pre_state_hash {
                tracing::debug!(target: "f1r3fly.casper.block_validation", block = %hex::encode(&block.block_hash[..8.min(block.block_hash.len())]), computed = %hex::encode(&computed_pre_state_hash[..8.min(computed_pre_state_hash.len())]), incoming = %hex::encode(&incoming_pre_state_hash[..8.min(incoming_pre_state_hash.len())]), "validate.block_checkpoint: PRE-STATE MISMATCH (recomputed merge != block's recorded pre-state) -> reject, NO replay");
                tracing::warn!(
                    "Computed pre-state hash {} does not equal block's pre-state hash {}.",
                    PrettyPrinter::build_string_bytes(&computed_pre_state_hash),
                    PrettyPrinter::build_string_bytes(&incoming_pre_state_hash)
                );

                Ok(Either::Right(None))
            } else if !rejections_match || !state_effects_match {
                // Detailed logging for InvalidRejectedDeploy mismatch
                let extra_in_computed: Vec<_> = computed_rejections
                    .difference(&block_rejections)
                    .cloned()
                    .collect();
                let missing_in_computed: Vec<_> = block_rejections
                    .difference(&computed_rejections)
                    .cloned()
                    .collect();

                // Find duplicates across all deploy sigs in the block
                let mut sig_counts: HashMap<Bytes, usize> = HashMap::new();
                for pd in &block.body.deploys {
                    *sig_counts.entry(pd.deploy.sig.clone()).or_insert(0) += 1;
                }
                for rd in &block.body.rejected_deploys {
                    *sig_counts.entry(rd.sig.clone()).or_insert(0) += 1;
                }
                let duplicate_count = sig_counts.values().filter(|&&c| c > 1).count();

                tracing::error!(
                    block_num = block.body.state.block_number,
                    block_hash = %PrettyPrinter::build_string_bytes(&block.block_hash),
                    sender = %PrettyPrinter::build_string_bytes(&block.sender),
                    validator_rejected = computed_rejections.len(),
                    block_rejected = block_rejections.len(),
                    extra_count = extra_in_computed.len(),
                    missing_count = missing_in_computed.len(),
                    duplicate_count,
                    computed_rejected_state_effects = rejected_state_effects.len(),
                    block_rejected_state_effects = block.body.rejected_state_effects.len(),
                    state_effects_match,
                    "merge-disposition mismatch: validator and block creator disagree on rejected deploys or state effects"
                );

                Ok(Either::Left(BlockStatus::invalid_rejected_deploy()))
            } else {
                Ok(Either::Right(Some(incoming_pre_state_hash)))
            }
        }
        Err(ex) => Ok(Either::Left(BlockStatus::exception(ex))),
    }
}

pub async fn replay_validated_block_checkpoint(
    block: &BlockMessage,
    s: &mut CasperSnapshot,
    runtime_manager: &RuntimeManager,
    pre_state_hash: StateHash,
) -> Result<BlockProcessing<Option<StateHash>>, CasperError> {
    tracing::debug!(target: "f1r3fly.casper.replay_block", "before-process-pre-state-hash");
    tracing::debug!(target: "f1r3fly.casper.replay_block", "replay-block-started");
    let replay_start = std::time::Instant::now();
    let replay_result = replay_block(pre_state_hash, block, &mut s.dag, runtime_manager).await?;
    metrics::histogram!(BLOCK_PROCESSING_REPLAY_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
        .record(replay_start.elapsed().as_secs_f64());
    tracing::debug!(target: "f1r3fly.casper.replay_block", "replay-block-finished");

    handle_errors(proto_util::post_state_hash(block), replay_result)
}

pub async fn validate_block_checkpoint(
    block: &BlockMessage,
    block_store: &KeyValueBlockStore,
    s: &mut CasperSnapshot,
    runtime_manager: &RuntimeManager,
    rejected_deploy_buffer: Option<&std::sync::Arc<std::sync::Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>>,
) -> Result<BlockProcessing<Option<StateHash>>, CasperError> {
    match validate_block_pre_state(
        block,
        block_store,
        s,
        runtime_manager,
        rejected_deploy_buffer,
    )
    .await?
    {
        Either::Left(error) => Ok(Either::Left(error)),
        Either::Right(None) => Ok(Either::Right(None)),
        Either::Right(Some(pre_state_hash)) => {
            replay_validated_block_checkpoint(block, s, runtime_manager, pre_state_hash).await
        }
    }
}

async fn replay_block(
    initial_state_hash: StateHash,
    block: &BlockMessage,
    dag: &mut KeyValueDagRepresentation,
    runtime_manager: &RuntimeManager,
) -> Result<Either<ReplayFailure, StateHash>, CasperError> {
    // Extract deploys and system deploys from the block
    let internal_deploys = proto_util::deploys(block);

    // Check for duplicate deploys in the block before replay.
    let mut sig_counts: HashMap<Bytes, usize> = HashMap::new();
    for processed in &internal_deploys {
        *sig_counts.entry(processed.deploy.sig.clone()).or_insert(0) += 1;
    }
    let mut exact_rejection_counts: HashMap<(Bytes, BlockHash), usize> = HashMap::new();
    for rejected in &block.body.rejected_deploys {
        if rejected.has_provenance() {
            *exact_rejection_counts
                .entry((rejected.sig.clone(), rejected.source_block_hash.clone()))
                .or_insert(0) += 1;
        } else {
            *sig_counts.entry(rejected.sig.clone()).or_insert(0) += 1;
        }
    }
    let mut deploy_duplicates: HashMap<Bytes, usize> = sig_counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .collect();
    for ((sig, _), count) in exact_rejection_counts {
        if count > 1 {
            deploy_duplicates
                .entry(sig)
                .and_modify(|current| *current = (*current).max(count))
                .or_insert(count);
        }
    }

    if !deploy_duplicates.is_empty() {
        let duplicates_str: String = deploy_duplicates
            .iter()
            .map(|(sig, count)| {
                format!(
                    "  {} (appears {} times)",
                    PrettyPrinter::build_string_bytes(sig),
                    count
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        tracing::warn!(
            "\n=== Duplicate Deploys Detected in Block ===\n\
            Block #{} ({})\n\
            Found {} duplicate deploy signatures:\n{}\n\
            Total deploys: {}\n\
            Total rejected: {}\n\
            ============================================",
            block.body.state.block_number,
            PrettyPrinter::build_string_bytes(&block.block_hash),
            deploy_duplicates.len(),
            duplicates_str,
            internal_deploys.len(),
            block.body.rejected_deploys.len()
        );
    } else {
        tracing::debug!(
            "Block #{}: replaying {} deploys, {} rejected",
            block.body.state.block_number,
            internal_deploys.len(),
            block.body.rejected_deploys.len()
        );
    }

    // Invalid-blocks map (hash -> sender) for the PoS slash deploys: derived from
    // this block's own recorded slash targets so it is byte-identical at block
    // creation and replay (see proto_util::slashed_block_senders). A DAG-derived
    // view is node-view-dependent and makes the slash deploy fail replay
    // (ConsumeFailed) because the map is produced into a content-addressed COMM.
    let slashed_hashes: Vec<BlockHash> = block
        .body
        .system_deploys
        .iter()
        .filter_map(|psd| match psd {
            ProcessedSystemDeploy::Succeeded {
                system_deploy:
                    SystemDeployData::Slash {
                        invalid_block_hash, ..
                    },
                ..
            } => Some(invalid_block_hash.clone()),
            _ => None,
        })
        .collect();
    let invalid_blocks: HashMap<BlockHash, Validator> =
        proto_util::slashed_block_senders(dag, &slashed_hashes)?;

    match runtime_manager
        .replay_block_from_consensus_data(&initial_state_hash, block, Some(invalid_blocks))
        .await
    {
        Ok(computed_state_hash) => Ok(Either::Right(computed_state_hash)),
        Err(CasperError::ReplayFailure(replay_failure)) => Ok(Either::Left(replay_failure)),
        Err(local_fault) => Ok(Either::Left(ReplayFailure::internal_error(
            local_fault.to_string(),
        ))),
    }
}

fn handle_errors(
    ts_hash: StateHash,
    result: Either<ReplayFailure, StateHash>,
) -> Result<BlockProcessing<Option<StateHash>>, CasperError> {
    match result {
        Either::Left(replay_failure) => match replay_failure {
            ReplayFailure::InternalError { msg } => {
                tracing::error!(error = %msg, "replay failed with an internal error");
                let exception = CasperError::RuntimeError(format!(
                    "Internal errors encountered while processing deploy: {}",
                    msg
                ));
                Ok(Either::Left(BlockStatus::exception(exception)))
            }

            ReplayFailure::ReplayStatusMismatch {
                initial_failed,
                replay_failed,
            } => {
                tracing::warn!(
                    "Found replay status mismatch; replay failure is {} and orig failure is {}",
                    replay_failed,
                    initial_failed
                );
                Ok(Either::Right(None))
            }

            ReplayFailure::UnusedCOMMEvent { msg } => {
                tracing::warn!("Found replay exception: {}", msg);
                Ok(Either::Right(None))
            }

            ReplayFailure::ReplayCostMismatch {
                initial_cost,
                replay_cost,
            } => {
                tracing::warn!(
                    "Found replay cost mismatch: initial deploy cost = {}, replay deploy cost = {}",
                    initial_cost,
                    replay_cost
                );
                Ok(Either::Right(None))
            }

            ReplayFailure::EffectStateMismatch {
                effect,
                boundary,
                expected,
                actual,
            } => {
                tracing::warn!(
                    effect,
                    boundary,
                    expected,
                    actual,
                    "replay effect-state witness mismatch"
                );
                Ok(Either::Right(None))
            }

            ReplayFailure::ReplayAdmissionMismatch {
                expected_admitted,
                replay_admitted,
                expected_rejected,
                replay_rejected,
                detail,
            } => {
                // WD-D2: the per-signature acceptance gate recomputed on replay
                // disagreed with the block (over-admission / double-spend, or a
                // settlement-debit total mismatch). The block is INVALID.
                println!(
                    "Found replay admission mismatch (expected_admitted={}, replay_admitted={}, expected_rejected={}, replay_rejected={}): {}",
                    expected_admitted, replay_admitted, expected_rejected, replay_rejected, detail
                );
                tracing::warn!(
                    "Found replay admission mismatch (expected_admitted={}, replay_admitted={}, expected_rejected={}, replay_rejected={}): {}",
                    expected_admitted,
                    replay_admitted,
                    expected_rejected,
                    replay_rejected,
                    detail
                );
                Ok(Either::Right(None))
            }

            ReplayFailure::SystemDeployErrorMismatch {
                play_error,
                replay_error,
            } => {
                tracing::warn!(
                        "Found system deploy error mismatch: initial deploy error message = {}, replay deploy error message = {}",
                        play_error, replay_error
                    );
                Ok(Either::Right(None))
            }
        },

        Either::Right(computed_state_hash) => {
            if ts_hash == computed_state_hash {
                // State hash in block matches computed hash!
                Ok(Either::Right(Some(computed_state_hash)))
            } else {
                // State hash in block does not match computed hash -- invalid!
                // return no state hash, do not update the state hash set
                tracing::warn!(
                    "Tuplespace hash {} does not match computed hash {}.",
                    PrettyPrinter::build_string_bytes(&ts_hash),
                    PrettyPrinter::build_string_bytes(&computed_state_hash)
                );
                Ok(Either::Right(None))
            }
        }
    }
}

pub fn print_deploy_errors(deploy_sig: &Bytes, errors: &[InterpreterError]) {
    let deploy_info = PrettyPrinter::build_string_sig(deploy_sig);
    let error_messages: String = errors
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join(", ");

    tracing::warn!("Deploy ({}) errors: {}", deploy_info, error_messages);
}

/// Multi-signature-aware variant of [`compute_deploys_checkpoint`]. Accepts
/// `Vec<Cosigned<DeployData>>` so multi-signature deploys execute through
/// certified reservation and realized-cost settlement at the runtime layer.
/// For legacy single-signature deploys (1-element Cosigned envelopes —
/// produced via `Cosigned::from_single_signer` when no sidecar metadata
/// exists), behavior is byte-identical to `compute_deploys_checkpoint`.
pub async fn compute_deploys_checkpoint_cosigned(
    block_store: &mut KeyValueBlockStore,
    parents: Vec<BlockMessage>,
    deploys: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
    system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
    s: &CasperSnapshot,
    runtime_manager: &RuntimeManager,
    block_data: BlockData,
    invalid_blocks: HashMap<BlockHash, Validator>,
    rejected_deploy_buffer: Option<&std::sync::Arc<std::sync::Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>>,
) -> Result<
    (
        StateHash,
        StateHash,
        Vec<ProcessedDeploy>,
        Vec<RejectedDeploy>,
        Vec<ProcessedSystemDeploy>,
        Vec<Bond>,
    ),
    CasperError,
> {
    let (
        pre_state,
        post_state,
        processed_deploys,
        rejected_deploys,
        _,
        processed_system_deploys,
        bonds,
    ) = compute_deploys_checkpoint_cosigned_internal(
        block_store,
        parents,
        deploys,
        system_deploys,
        s,
        runtime_manager,
        block_data,
        invalid_blocks,
        rejected_deploy_buffer,
        None,
    )
    .await?;
    Ok((
        pre_state,
        post_state,
        processed_deploys,
        rejected_deploys,
        processed_system_deploys,
        bonds,
    ))
}

pub async fn compute_deploys_checkpoint_cosigned_admitted(
    block_store: &mut KeyValueBlockStore,
    parents: Vec<BlockMessage>,
    deploys: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
    system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
    s: &CasperSnapshot,
    runtime_manager: &RuntimeManager,
    block_data: BlockData,
    invalid_blocks: HashMap<BlockHash, Validator>,
    rejected_deploy_buffer: Option<&std::sync::Arc<std::sync::Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>>,
    admission: crate::rust::util::rholang::runtime_manager::StateBoundAdmission,
) -> Result<
    (
        StateHash,
        StateHash,
        Vec<ProcessedDeploy>,
        Vec<RejectedDeploy>,
        Vec<ProcessedSystemDeploy>,
        Vec<Bond>,
    ),
    CasperError,
> {
    let (
        pre_state,
        post_state,
        processed_deploys,
        rejected_deploys,
        _,
        processed_system_deploys,
        bonds,
    ) = compute_deploys_checkpoint_cosigned_internal(
        block_store,
        parents,
        deploys,
        system_deploys,
        s,
        runtime_manager,
        block_data,
        invalid_blocks,
        rejected_deploy_buffer,
        Some(admission),
    )
    .await?;
    Ok((
        pre_state,
        post_state,
        processed_deploys,
        rejected_deploys,
        processed_system_deploys,
        bonds,
    ))
}

pub async fn compute_deploys_checkpoint_cosigned_admitted_with_effects(
    block_store: &mut KeyValueBlockStore,
    parents: Vec<BlockMessage>,
    deploys: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
    system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
    s: &CasperSnapshot,
    runtime_manager: &RuntimeManager,
    block_data: BlockData,
    invalid_blocks: HashMap<BlockHash, Validator>,
    rejected_deploy_buffer: Option<&std::sync::Arc<std::sync::Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>>,
    admission: crate::rust::util::rholang::runtime_manager::StateBoundAdmission,
) -> Result<
    (
        StateHash,
        StateHash,
        Vec<ProcessedDeploy>,
        Vec<RejectedDeploy>,
        Vec<StateEffectId>,
        Vec<ProcessedSystemDeploy>,
        Vec<Bond>,
    ),
    CasperError,
> {
    compute_deploys_checkpoint_cosigned_internal(
        block_store,
        parents,
        deploys,
        system_deploys,
        s,
        runtime_manager,
        block_data,
        invalid_blocks,
        rejected_deploy_buffer,
        Some(admission),
    )
    .await
}

async fn compute_deploys_checkpoint_cosigned_internal(
    block_store: &mut KeyValueBlockStore,
    parents: Vec<BlockMessage>,
    deploys: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>>,
    system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
    s: &CasperSnapshot,
    runtime_manager: &RuntimeManager,
    block_data: BlockData,
    invalid_blocks: HashMap<BlockHash, Validator>,
    rejected_deploy_buffer: Option<&std::sync::Arc<std::sync::Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>>,
    admission: Option<crate::rust::util::rholang::runtime_manager::StateBoundAdmission>,
) -> Result<
    (
        StateHash,
        StateHash,
        Vec<ProcessedDeploy>,
        Vec<RejectedDeploy>,
        Vec<StateEffectId>,
        Vec<ProcessedSystemDeploy>,
        Vec<Bond>,
    ),
    CasperError,
> {
    tracing::debug!(target: "f1r3fly.casper.compute-deploys-checkpoint-cosigned",
        "compute-deploys-checkpoint-cosigned-started");
    if parents.is_empty() {
        return Err(CasperError::RuntimeError(
            "Parents must not be empty".to_string(),
        ));
    }
    // Propose (cosigned): the floor is derived from the proposer's justification
    // snapshot, which is exactly what gets packaged into this block's header — so
    // the floor the proposer builds on equals the floor every validator re-derives.
    let latest_messages: BTreeMap<Validator, BlockHash> = s
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let computed_parents_info = compute_parents_post_state_with_effects(
        block_store,
        parents,
        s,
        runtime_manager,
        &latest_messages,
        None,
        rejected_deploy_buffer,
    )
    .await?;
    let (pre_state_hash, rejected_deploys, rejected_state_effects) = computed_parents_info;

    let result = if let Some(admission) = admission {
        if admission.pre_state() != &pre_state_hash
            || admission.outcome().admitted != deploys
            || !admission.matches_context(&block_data, &invalid_blocks)
        {
            return Err(CasperError::InvalidCostSettlement(
                "state-bound admission token does not match checkpoint inputs".to_string(),
            ));
        }
        runtime_manager
            .compute_state_with_bonds_cosigned_admitted(admission, system_deploys)
            .await?
    } else {
        runtime_manager
            .compute_state_with_bonds_cosigned(
                &pre_state_hash,
                deploys,
                system_deploys,
                block_data,
                Some(invalid_blocks),
            )
            .await?
    };
    let (post_state_hash, processed_deploys, processed_system_deploys, bonds) = result;
    Ok((
        pre_state_hash,
        post_state_hash,
        processed_deploys,
        rejected_deploys,
        rejected_state_effects,
        processed_system_deploys,
        bonds,
    ))
}

pub async fn compute_deploys_checkpoint(
    block_store: &mut KeyValueBlockStore,
    parents: Vec<BlockMessage>,
    deploys: Vec<Signed<DeployData>>,
    system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
    s: &CasperSnapshot,
    runtime_manager: &RuntimeManager,
    block_data: BlockData,
    invalid_blocks: HashMap<BlockHash, Validator>,
    rejected_deploy_buffer: Option<&std::sync::Arc<std::sync::Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>>,
) -> Result<
    (
        StateHash,
        StateHash,
        Vec<ProcessedDeploy>,
        Vec<RejectedDeploy>,
        Vec<ProcessedSystemDeploy>,
        Vec<Bond>,
    ),
    CasperError,
> {
    let (
        pre_state,
        post_state,
        processed_deploys,
        rejected_deploys,
        _,
        processed_system_deploys,
        bonds,
    ) = compute_deploys_checkpoint_with_effects(
        block_store,
        parents,
        deploys,
        system_deploys,
        s,
        runtime_manager,
        block_data,
        invalid_blocks,
        rejected_deploy_buffer,
    )
    .await?;
    Ok((
        pre_state,
        post_state,
        processed_deploys,
        rejected_deploys,
        processed_system_deploys,
        bonds,
    ))
}

pub async fn compute_deploys_checkpoint_with_effects(
    block_store: &mut KeyValueBlockStore,
    parents: Vec<BlockMessage>,
    deploys: Vec<Signed<DeployData>>,
    system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
    s: &CasperSnapshot,
    runtime_manager: &RuntimeManager,
    block_data: BlockData,
    invalid_blocks: HashMap<BlockHash, Validator>,
    rejected_deploy_buffer: Option<&std::sync::Arc<std::sync::Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>>,
) -> Result<
    (
        StateHash,
        StateHash,
        Vec<ProcessedDeploy>,
        Vec<RejectedDeploy>,
        Vec<StateEffectId>,
        Vec<ProcessedSystemDeploy>,
        Vec<Bond>,
    ),
    CasperError,
> {
    let checkpoint_started = std::time::Instant::now();
    // Using tracing events for async - Span[F] equivalent from Scala
    tracing::debug!(target: "f1r3fly.casper.compute_deploys_checkpoint", "compute-deploys-checkpoint-started");
    tracing::debug!(target: "f1r3fly.casper.compute_deploys_checkpoint", n_deploys = deploys.len(), n_parents = parents.len(), "propose.compute_deploys_checkpoint ENTER (merge parents, then run deploys)");
    // Ensure parents are not empty
    if parents.is_empty() {
        return Err(CasperError::RuntimeError(
            "Parents must not be empty".to_string(),
        ));
    }

    // Compute parents post state
    let parents_started = std::time::Instant::now();
    // Propose: the floor is derived from the proposer's justification snapshot,
    // which is exactly what gets packaged into this block's header — so the floor
    // the proposer builds on equals the floor every validator re-derives.
    let latest_messages: BTreeMap<Validator, BlockHash> = s
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let computed_parents_info = compute_parents_post_state_with_effects(
        block_store,
        parents,
        s,
        runtime_manager,
        &latest_messages,
        None,
        rejected_deploy_buffer,
    )
    .await?;
    let parents_ms = parents_started.elapsed().as_millis();
    let (pre_state_hash, rejected_deploys, rejected_state_effects) = computed_parents_info;

    // Compute state and bonds using one spawned runtime
    let compute_state_started = std::time::Instant::now();
    let cosigned_deploys = deploys
        .into_iter()
        .map(crypto::rust::signatures::signed::Cosigned::from_single_signer)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
    let result = runtime_manager
        .compute_state_with_bonds_cosigned(
            &pre_state_hash,
            cosigned_deploys,
            system_deploys,
            block_data,
            Some(invalid_blocks),
        )
        .await?;
    let compute_state_ms = compute_state_started.elapsed().as_millis();

    let (post_state_hash, processed_deploys, processed_system_deploys, bonds) = result;
    tracing::debug!(
        target: "f1r3fly.casper.compute_deploys_checkpoint.timing",
        "compute_deploys_checkpoint timing: parents_post_state_ms={}, compute_state_ms={}, total_ms={}, processed_deploys={}, processed_system_deploys={}, rejected_deploys={}",
        parents_ms,
        compute_state_ms,
        checkpoint_started.elapsed().as_millis(),
        processed_deploys.len(),
        processed_system_deploys.len(),
        rejected_deploys.len()
    );

    Ok((
        pre_state_hash,
        post_state_hash,
        processed_deploys,
        rejected_deploys,
        rejected_state_effects,
        processed_system_deploys,
        bonds,
    ))
}

/// Ensure every block in the merge `scope` has its mergeable-channels entry
/// materialized before `dag_merger::merge` reads them through the (synchronous)
/// `block_index_f` closure. A block imported via LFS without replay — or rejected
/// locally and never replayed — lacks it, and `load_mergeable_channels`
/// hard-errors `KeyNotFound`, which made merge validity node-local (the same
/// block could finalize on nodes that replayed it and be rejected on nodes that
/// did not — forking the DAG). Recomputing the missing entries here (a
/// deterministic full replay) makes mergeable presence a function of block
/// content on every node. Healthy nodes pay only a cached-presence check.
pub(crate) async fn ensure_scope_mergeable_present(
    block_store: &KeyValueBlockStore,
    runtime_manager: &RuntimeManager,
    dag: &KeyValueDagRepresentation,
    scope: &HashSet<BlockHash>,
) -> Result<(), CasperError> {
    for hash in scope {
        // Fast path: a cached BlockIndex implies load_mergeable_channels already
        // succeeded for this block, so its persistent entry is present.
        if runtime_manager.has_cached_block_index(hash) {
            continue;
        }
        let block = block_store.get_unsafe(hash);
        if runtime_manager.has_mergeable_entry(&block)? {
            continue;
        }

        // The recompute replays the block; its invalid-blocks map must match the
        // one validation/creation used (slashed_block_senders — the block's own
        // slash targets) so the replay reproduces the block's post_state_hash and
        // the entry is stored under the correct key.
        let slashed_hashes: Vec<BlockHash> = block
            .body
            .system_deploys
            .iter()
            .filter_map(|psd| match psd {
                ProcessedSystemDeploy::Succeeded {
                    system_deploy:
                        SystemDeployData::Slash {
                            invalid_block_hash, ..
                        },
                    ..
                } => Some(invalid_block_hash.clone()),
                _ => None,
            })
            .collect();
        let invalid_blocks: HashMap<BlockHash, Validator> =
            proto_util::slashed_block_senders(dag, &slashed_hashes)?;

        runtime_manager
            .ensure_mergeable_entry(&block, invalid_blocks)
            .await?;

        tracing::info!(
            target: "f1r3fly.casper.mergeable_recompute",
            block_hash = %hex::encode(&hash[..hash.len().min(8)]),
            seq_num = block.seq_num,
            "recomputed missing mergeable entry for merge-scope block",
        );
    }

    Ok(())
}

/// Cap on the finalized-floor distance Δ = num(maxParent) − num(floor) that a
/// single multi-parent merge may span. The DETERMINISTIC backstop: Δ is a pure
/// function of the block's frozen justifications, so every honest node computes
/// the same Δ. Beyond the cap, `compute_parents_post_state` returns `Err` (the
/// proposer parks; a validator deterministically rejects) instead of silently
/// substituting one parent's post-state and dropping the others' writes.
pub(crate) const MAX_FLOOR_DISTANCE_BLOCKS: i64 = 256;

/// Advisory cap on the merge scope `|visible_blocks|`. This is NOT node-
/// deterministic (branch width differs across each node's view), so it MUST NOT
/// gate admission — a divergent reject would fork. Recorded as a metric/warning
/// only; the deterministic Δ backstop above is what bounds the merge.
pub(crate) const MAX_PARENT_MERGE_SCOPE_BLOCKS: usize = 512;

/// The deterministic merge-scope backstop decision: refuse (`Err`) iff the floor
/// distance exceeds the cap. A pure function of Δ ONLY — the scope size never
/// gates (see `MAX_PARENT_MERGE_SCOPE_BLOCKS`). Extracted for direct unit testing
/// of the "reject on Δ only, never on scope" property.
pub(crate) fn merge_scope_backstop_exceeded(floor_distance: i64) -> bool {
    floor_distance > MAX_FLOOR_DISTANCE_BLOCKS
}

pub async fn compute_parents_post_state(
    block_store: &KeyValueBlockStore,
    parents: Vec<BlockMessage>,
    s: &CasperSnapshot,
    runtime_manager: &RuntimeManager,
    latest_messages: &BTreeMap<Validator, BlockHash>,
    disable_late_block_filtering_override: Option<bool>,
    rejected_deploy_buffer: Option<&std::sync::Arc<std::sync::Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>>,
) -> Result<(StateHash, Vec<RejectedDeploy>), CasperError> {
    let (state, rejected_deploys, _) = compute_parents_post_state_with_effects(
        block_store,
        parents,
        s,
        runtime_manager,
        latest_messages,
        disable_late_block_filtering_override,
        rejected_deploy_buffer,
    )
    .await?;
    Ok((state, rejected_deploys))
}

/// Compute the merged post-state and its exact rejected-effect provenance from multiple parent blocks.
///
/// For exploratory deploy, pass `disable_late_block_filtering_override = Some(true)` to
/// always disable late block filtering (see full merged state).
/// For normal block creation, pass `None` to use the shard config value.
pub async fn compute_parents_post_state_with_effects(
    block_store: &KeyValueBlockStore,
    parents: Vec<BlockMessage>,
    s: &CasperSnapshot,
    runtime_manager: &RuntimeManager,
    // The block's frozen justification snapshot (proposer's justifications at
    // propose, the block's recorded justifications at validate) — NEVER the live
    // DAG view. The finalized floor is derived from this so it is node-identical.
    latest_messages: &BTreeMap<Validator, BlockHash>,
    disable_late_block_filtering_override: Option<bool>,
    rejected_deploy_buffer: Option<&std::sync::Arc<std::sync::Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>>,
) -> Result<(StateHash, Vec<RejectedDeploy>, Vec<StateEffectId>), CasperError> {
    let total_started = std::time::Instant::now();

    // No entered span guard here: the function is async (it awaits floor
    // derivation), and an `.entered()` guard is not `Send` across an await.
    // The individual tracing events below carry their own targets.
    tracing::debug!(
        target: "f1r3fly.merge.step",
        step = "compute_parents_post_state.ENTER",
        n_parents = parents.len(),
        latest_messages = latest_messages.len(),
        "merge.step: enter compute_parents_post_state"
    );
    match parents.len() {
        // For genesis, use empty trie's root hash
        0 => {
            let state = RuntimeManager::empty_state_hash_fixed();
            tracing::debug!(
                target: "f1r3fly.casper.compute_parents_post_state.timing",
                "compute_parents_post_state timing: path=genesis, parents=0, total_ms={}",
                total_started.elapsed().as_millis()
            );
            tracing::debug!(
                target: "f1r3fly.merge.step",
                step = "compute_parents_post_state.EXIT",
                path = "genesis",
                post_state = %hex::encode(&state[..8.min(state.len())]),
                "merge.step: exit compute_parents_post_state"
            );
            Ok((state, Vec::new(), Vec::new()))
        }

        _ => {
            let cache_lookup_started = std::time::Instant::now();
            let mut parent_hashes_for_key: Vec<BlockHash> =
                parents.iter().map(|p| p.block_hash.clone()).collect();
            parent_hashes_for_key.sort();
            let disable_late_block_filtering = disable_late_block_filtering_override
                .unwrap_or(s.on_chain_state.shard_conf.disable_late_block_filtering);
            let cache_key = super::runtime_manager::ParentsPostStateCacheKey {
                sorted_parent_hashes: parent_hashes_for_key,
                snapshot_lfb_hash: s.last_finalized_block.clone(),
                // BTreeMap iteration is key-ordered, so this is deterministic.
                sorted_latest_messages: latest_messages
                    .iter()
                    .map(|(v, h)| (v.clone(), h.clone()))
                    .collect(),
                disable_late_block_filtering,
            };
            if let Some((cached_state, cached_rejected, cached_rejected_state_effects)) =
                runtime_manager.get_cached_parents_post_state(&cache_key)
            {
                tracing::debug!(
                    target: "f1r3fly.casper.compute_parents_post_state.cache",
                    "compute_parents_post_state cache hit: parents={}, rejected_deploys={}",
                    cache_key.sorted_parent_hashes.len(),
                    cached_rejected.len()
                );
                tracing::debug!(
                    target: "f1r3fly.casper.compute_parents_post_state.timing",
                    "compute_parents_post_state timing: path=cache_hit, parents={}, cache_lookup_ms={}, total_ms={}",
                    cache_key.sorted_parent_hashes.len(),
                    cache_lookup_started.elapsed().as_millis(),
                    total_started.elapsed().as_millis()
                );
                tracing::debug!(
                    target: "f1r3fly.merge.step",
                    step = "compute_parents_post_state.CACHE_SKIP",
                    path = "cache_hit",
                    post_state = %hex::encode(&cached_state[..8.min(cached_state.len())]),
                    n_rejected = cached_rejected.len(),
                    "merge.step: cache hit, merge skipped"
                );
                return Ok((cached_state, cached_rejected, cached_rejected_state_effects));
            }
            let cache_lookup_ms = cache_lookup_started.elapsed().as_millis();

            // Function to get or compute BlockIndex for each parent block hash
            let block_index_f = |v: &BlockHash| -> Result<BlockIndex, CasperError> {
                let b = block_store.get_unsafe(v);
                let pre_state = &b.body.state.pre_state_hash;
                let post_state = &b.body.state.post_state_hash;
                let mergeable_chs = runtime_manager.load_mergeable_channels(&b)?;

                runtime_manager.get_or_compute_block_index(
                    &b.block_hash,
                    b.body.state.block_number,
                    &b.body.deploys,
                    &b.body.system_deploys,
                    &Blake2b256Hash::from_bytes_prost(pre_state),
                    &Blake2b256Hash::from_bytes_prost(post_state),
                    &mergeable_chs,
                )
            };

            // Compute scope: the unsealed band not represented by the finalized floor.
            //
            // H3: derive the floor BEFORE collecting the merge scope so the
            // ancestor walk can drop only blocks represented by the floor's
            // DAG past. A below-floor sibling can still be a direct parent, and
            // its effects are not in the floor's post-state.
            let parent_hashes: Vec<BlockHash> =
                parents.iter().map(|p| p.block_hash.clone()).collect();
            let max_parent_block_number = parents
                .iter()
                .map(|p| p.body.state.block_number)
                .max()
                .unwrap_or(0);

            // Node-deterministic finalized floor, derived from the block's frozen
            // justification snapshot — the cut the merge builds on. Replaces the
            // LCA base: the floor is finalization-aware and advance-only, so the
            // merged state is MONOTONE, where the LCA base let it churn
            // (path-dependent FS).
            let floor_derive_started = std::time::Instant::now();
            let floor = crate::rust::finality::floor::finalized_floor(
                &s.dag,
                &parent_hashes,
                latest_messages,
                crate::rust::safety::clique_oracle::FtThreshold::from_ppm(
                    s.on_chain_state.shard_conf.fault_tolerance_threshold_ppm,
                ),
            )
            .await?;
            let floor_derive_ms = floor_derive_started.elapsed().as_millis();
            let floor_hash = floor.hash.clone();

            // The floor block's committed post-state is FS(floor) — the merge base.
            // The floor is an ancestor of the parents (which are stored), so this is
            // present in normal operation; surface a clear error instead of panicking
            // if the block store and DAG ever desynchronize.
            let floor_block = block_store.get(&floor_hash)?.ok_or_else(|| {
                CasperError::RuntimeError(format!(
                    "finalized-floor block {} not in block store (DAG/store desync)",
                    hex::encode(&floor_hash[..8.min(floor_hash.len())])
                ))
            })?;
            let floor_state =
                Blake2b256Hash::from_bytes_prost(&floor_block.body.state.post_state_hash);
            let floor_block_number = floor_block.body.state.block_number;

            for candidate in &parents {
                let covers_all = parents
                    .iter()
                    .filter(|parent| parent.block_hash != candidate.block_hash)
                    .all(|parent| {
                        s.dag
                            .is_dag_ancestor(&parent.block_hash, &candidate.block_hash)
                            .unwrap_or(false)
                    });
                if covers_all
                    && crate::rust::finality::floor::is_state_preserved(
                        &s.dag,
                        &floor_hash,
                        &candidate.block_hash,
                    )?
                {
                    let state = proto_util::post_state_hash(candidate);
                    tracing::debug!(
                        target: "f1r3fly.casper.compute_parents_post_state.timing",
                        "compute_parents_post_state timing: path=state_safe_covering_parent, parents={}, cache_lookup_ms={}, floor_derive_ms={}, total_ms={}",
                        parents.len(),
                        cache_lookup_ms,
                        floor_derive_ms,
                        total_started.elapsed().as_millis()
                    );
                    tracing::debug!(
                        target: "f1r3fly.merge.step",
                        step = "compute_parents_post_state.EXIT",
                        path = "state_safe_covering_parent",
                        post_state = %hex::encode(&state[..8.min(state.len())]),
                        "merge.step: exit compute_parents_post_state"
                    );
                    return Ok((state, Vec::new(), Vec::new()));
                }
            }

            let include_visible_ancestor =
                |hash: &BlockHash, dag: &KeyValueDagRepresentation| -> bool {
                    block_in_floor_merge_scope(dag, hash, &floor_hash, floor_block_number)
                        .unwrap_or(false)
                };
            // Get all ancestors of all parents (including the parents themselves)
            // Use bounded traversal that stops once the floor's represented past is reached.
            let collect_ancestors_started = std::time::Instant::now();
            let mut visible_ancestor_sets_with_parents: Vec<HashSet<BlockHash>> = Vec::new();
            for parent_hash in &parent_hashes {
                let visible_ancestors = s.dag.with_ancestors(parent_hash.clone(), |bh| {
                    include_visible_ancestor(bh, &s.dag)
                })?;
                let mut visible_ancestors_with_parent = visible_ancestors;
                visible_ancestors_with_parent.insert(parent_hash.clone());
                visible_ancestor_sets_with_parents.push(visible_ancestors_with_parent);
            }
            let collect_ancestors_ms = collect_ancestors_started.elapsed().as_millis();

            // Flatten all ancestor sets to get visible blocks
            let flatten_visible_started = std::time::Instant::now();
            let mut visible_blocks: HashSet<BlockHash> = visible_ancestor_sets_with_parents
                .iter()
                .flat_map(|s| s.iter().cloned())
                .collect();
            let flatten_visible_ms = flatten_visible_started.elapsed().as_millis();

            // Scope visible_blocks to blocks not represented by the floor.
            let pre_filter_count = visible_blocks.len();
            let pre_filter_blocks: Option<Vec<BlockHash>> = if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG)
            {
                Some(visible_blocks.iter().cloned().collect())
            } else {
                None
            };
            visible_blocks.retain(|bh| {
                block_in_floor_merge_scope(&s.dag, bh, &floor_hash, floor_block_number)
                    .unwrap_or(true)
            });
            if tracing::enabled!(target: "f1r3fly.merge.step", tracing::Level::DEBUG) {
                let dropped: Vec<String> = match &pre_filter_blocks {
                    Some(pre) => pre
                        .iter()
                        .filter(|bh| !visible_blocks.contains(*bh))
                        .map(|bh| hex::encode(&bh[..8.min(bh.len())]))
                        .collect(),
                    None => Vec::new(),
                };
                tracing::debug!(
                    target: "f1r3fly.merge.step",
                    step = "compute_parents_post_state.FLOOR",
                    floor = %hex::encode(&floor_hash[..8.min(floor_hash.len())]),
                    floor_block = floor_block_number,
                    base_state = %hex::encode(&floor_block.body.state.post_state_hash[..8.min(floor_block.body.state.post_state_hash.len())]),
                    scope_before = pre_filter_count,
                    scope_after = visible_blocks.len(),
                    n_dropped = dropped.len(),
                    dropped = ?dropped,
                    n_parents = parents.len(),
                    "merge.step: floor derived; base=floor.post_state; scope filtered to >= floor block"
                );
            }
            tracing::debug!(target: "f1r3fly.casper.compute_parents_post_state", floor = %hex::encode(&floor_hash[..8.min(floor_hash.len())]), floor_block = floor_block_number, base_state = %hex::encode(&floor_block.body.state.post_state_hash[..8.min(floor_block.body.state.post_state_hash.len())]), scope_blocks = visible_blocks.len(), n_parents = parents.len(), "merge.compute_parents_post_state: floor+base+scope computed");
            if visible_blocks.len() < pre_filter_count {
                tracing::debug!(
                    target: "f1r3fly.casper.compute_parents_post_state",
                    "floor-scoped merge: reduced visible_blocks from {} to {} (floor at block #{})",
                    pre_filter_count,
                    visible_blocks.len(),
                    floor_block_number,
                );
            }

            if tracing::enabled!(tracing::Level::DEBUG) {
                let parent_hash_str: Vec<String> = parent_hashes
                    .iter()
                    .map(|h| hex::encode(&h[..std::cmp::min(10, h.len())]))
                    .collect();
                tracing::debug!(
                    "computeParentsPostState: parents=[{}], floor={} (block {}), visibleBlocks={}",
                    parent_hash_str.join(", "),
                    hex::encode(&floor_hash[..std::cmp::min(10, floor_hash.len())]),
                    floor_block_number,
                    visible_blocks.len()
                );
            }

            let floor_distance = max_parent_block_number - floor_block_number;
            let visible_blocks_len = visible_blocks.len();
            // Observability: the (node-deterministic) floor distance Δ and the
            // (NOT node-deterministic) scope size. Only Δ gates admission.
            metrics::histogram!(
                crate::rust::metrics_constants::FLOOR_DISTANCE_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .record(floor_distance as f64);
            metrics::histogram!(
                crate::rust::metrics_constants::MERGE_SCOPE_SIZE_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .record(visible_blocks_len as f64);
            if visible_blocks_len > MAX_PARENT_MERGE_SCOPE_BLOCKS {
                // The scope size is NOT node-deterministic (branch width differs
                // across each node's view), so it must NEVER gate admission — a
                // reject that differs across nodes would fork. Log as an anomaly
                // only; the deterministic floor-distance backstop below is what
                // actually bounds the merge.
                tracing::warn!(
                    target: "f1r3fly.casper.compute_parents_post_state",
                    visible_blocks = visible_blocks_len,
                    floor_distance,
                    max_scope = MAX_PARENT_MERGE_SCOPE_BLOCKS,
                    "merge scope unusually large; NOT rejecting (scope size is not node-deterministic) — watch floor distance"
                );
            }
            // Deterministic backstop: the floor distance Δ is a pure function of
            // the block's frozen justifications, so every honest node computes the
            // same Δ. When Δ exceeds the cap we REFUSE to build a merge rather than
            // silently substituting the single highest parent's post-state — the
            // former behaviour dropped every other parent's committed writes and
            // could strand a dropped co-parent's deploys as non-re-proposable
            // (the ~400-block "fails under load" bug: S5/¬T-K1/¬T-NDA). On propose
            // this `Err` parks the round (retried once finality advances and Δ
            // shrinks); on validate an over-Δ block is deterministically invalid —
            // both sides compute the same Δ, so there is no fork.
            if merge_scope_backstop_exceeded(floor_distance) {
                metrics::counter!(
                    crate::rust::metrics_constants::MERGE_SCOPE_BACKSTOP_ERROR_METRIC,
                    "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
                )
                .increment(1);
                tracing::warn!(
                    target: "f1r3fly.casper.compute_parents_post_state.backstop",
                    floor_distance,
                    max_floor_distance = MAX_FLOOR_DISTANCE_BLOCKS,
                    visible_blocks = visible_blocks_len,
                    floor = %hex::encode(&floor_hash[..8.min(floor_hash.len())]),
                    floor_block = floor_block_number,
                    n_parents = parents.len(),
                    "compute_parents_post_state backstop: floor distance exceeds cap; refusing lossy merge"
                );
                return Err(CasperError::RuntimeError(format!(
                    "finalized-floor merge backstop: floor distance {} exceeds cap {} (parents={}, floor block #{}); \
                     refusing to build a lossy single-parent merge — finality must advance first",
                    floor_distance,
                    MAX_FLOOR_DISTANCE_BLOCKS,
                    parents.len(),
                    floor_block_number,
                )));
            }

            // Every scope block's mergeable entry must be loadable before the
            // merge builds indices; recompute any this node never replayed
            // (otherwise a missing entry makes the merge fail node-locally → fork).
            tracing::debug!(
                target: "f1r3fly.merge.step",
                step = "compute_parents_post_state.ENSURE_MERGEABLE_PRE",
                scope_blocks = visible_blocks_len,
                "merge.step: ensure_scope_mergeable_present begin"
            );
            ensure_scope_mergeable_present(block_store, runtime_manager, &s.dag, &visible_blocks)
                .await?;
            tracing::debug!(
                target: "f1r3fly.merge.step",
                step = "compute_parents_post_state.ENSURE_MERGEABLE_POST",
                scope_blocks = visible_blocks_len,
                "merge.step: ensure_scope_mergeable_present done"
            );

            // Use DagMerger to merge parent states with scope
            tracing::debug!(
                target: "f1r3fly.merge.step",
                step = "compute_parents_post_state.MERGE_PRE",
                floor = %hex::encode(&floor_hash[..8.min(floor_hash.len())]),
                base_state = %hex::encode(&floor_state.bytes()[..8.min(floor_state.bytes().len())]),
                scope_blocks = visible_blocks_len,
                disable_late_block_filtering,
                "merge.step: dag_merger::merge begin"
            );
            let merge_started = std::time::Instant::now();
            let occurrence_context = merge_occurrence_context(
                block_store,
                &s.dag,
                &floor_hash,
                &visible_blocks,
                s.on_chain_state.shard_conf.casper_version,
            )?;
            let merger_result = dag_merger::merge(
                &s.dag,
                &floor_hash,
                &floor_state,
                |hash: &BlockHash| -> Result<Vec<DeployChainIndex>, CasperError> {
                    let block_index = block_index_f(hash)?;
                    Ok(block_index.deploy_chains)
                },
                &runtime_manager.history_repo,
                dag_merger::cost_optimal_rejection_alg(),
                Some(visible_blocks.clone()),
                disable_late_block_filtering,
                occurrence_context,
            )?;
            let merge_ms = merge_started.elapsed().as_millis();

            let state = merger_result.post_state;
            let mut rejected_user_pairs = merger_result.rejected_deploys;
            let rejected_state_effects = merger_result.rejected_state_effects;
            let suppressed = suppress_rejected_pairs_with_later_visible_rejection(
                block_store,
                &visible_blocks,
                &mut rejected_user_pairs,
            )?;
            if suppressed > 0 {
                tracing::debug!(
                    target: "f1r3fly.casper.recovery",
                    "compute_parents_post_state: suppressed {} already-visible rejected deploy(s)",
                    suppressed
                );
            }
            tracing::debug!(
                target: "f1r3fly.merge.step",
                step = "compute_parents_post_state.MERGE_POST",
                new_state = %hex::encode(&state.bytes()[..8.min(state.bytes().len())]),
                n_rejected_user = rejected_user_pairs.len(),
                merge_ms,
                "merge.step: dag_merger::merge returned"
            );
            tracing::debug!(target: "f1r3fly.casper.compute_parents_post_state", merged_state = %hex::encode(&state.bytes()[..8.min(state.bytes().len())]), n_rejected_user = rejected_user_pairs.len(), merge_ms, "merge.compute_parents_post_state: DagMerger produced merged state");

            // Populate the rejected-deploy buffer from (sig, source_block_hash) pairs.
            // Looking up the `Signed<DeployData>` from the block store lets the block
            // creator re-propose these deploys in a subsequent block. Fetching each
            // source block at most once keeps the cost proportional to the number of
            // distinct rejected-from blocks.
            //
            if let Some(buffer) = rejected_deploy_buffer {
                if !rejected_user_pairs.is_empty() {
                    let floor_won = canonical_won_sigs(
                        block_store,
                        std::slice::from_ref(&floor_hash),
                        i64::MIN,
                    )?;
                    let mut by_block: HashMap<BlockHash, Vec<Bytes>> = HashMap::new();
                    for rejected in &rejected_user_pairs {
                        if floor_won.contains(&rejected.sig)
                            || rejected_sig_has_visible_non_source_win(
                                block_store,
                                &visible_blocks,
                                &rejected.sig,
                                &rejected.source_block_hash,
                            )?
                        {
                            tracing::debug!(
                                target: "f1r3fly.casper.recovery",
                                "RejectedDeployBuffer populate: skipped already-won sig {} from {}",
                                PrettyPrinter::build_string_bytes(&rejected.sig),
                                PrettyPrinter::build_string_bytes(&rejected.source_block_hash)
                            );
                            continue;
                        }
                        by_block
                            .entry(rejected.source_block_hash.clone())
                            .or_default()
                            .push(rejected.sig.clone());
                    }
                    let mut deploys_to_buffer: Vec<Signed<DeployData>> = Vec::new();
                    for (src_block, sigs) in by_block {
                        let sig_set: HashSet<Bytes> = sigs.into_iter().collect();
                        match block_store.get(&src_block) {
                            Ok(Some(block)) => {
                                for pd in &block.body.deploys {
                                    if sig_set.contains(&pd.deploy.sig) {
                                        deploys_to_buffer.push(pd.deploy.clone());
                                    }
                                }
                            }
                            Ok(None) => {
                                tracing::warn!(
                                    "RejectedDeployBuffer populate: source block {} not in store",
                                    PrettyPrinter::build_string_bytes(&src_block)
                                );
                            }
                            Err(err) => {
                                tracing::warn!(
                                    "RejectedDeployBuffer populate: failed to load {}: {}",
                                    PrettyPrinter::build_string_bytes(&src_block),
                                    err
                                );
                            }
                        }
                    }
                    match retain_pending_rejected_deploys_for_buffer(
                        &s.dag,
                        block_store,
                        s.on_chain_state.shard_conf.deploy_lifespan,
                        &mut deploys_to_buffer,
                        max_parent_block_number.saturating_add(1),
                        s.on_chain_state.shard_conf.casper_version,
                    ) {
                        Ok(skipped) if skipped > 0 => {
                            tracing::info!(
                                target: "f1r3fly.casper.recovery",
                                "RejectedDeployBuffer populate: skipped {} terminal deploy(s)",
                                skipped
                            );
                        }
                        Ok(_) => {}
                        Err(err) => {
                            tracing::warn!(
                                "RejectedDeployBuffer populate: finalization-status filter failed; skipping populate: {}",
                                err
                            );
                            deploys_to_buffer.clear();
                        }
                    }
                    if !deploys_to_buffer.is_empty() {
                        match buffer.lock() {
                            Ok(mut guard) => {
                                if let Err(err) = guard.add(deploys_to_buffer) {
                                    tracing::warn!("RejectedDeployBuffer add failed: {}", err);
                                }
                            }
                            Err(_) => {
                                tracing::warn!(
                                    "RejectedDeployBuffer lock poisoned; skipping populate"
                                );
                            }
                        }
                    }
                }
            }

            let rejected = rejected_user_pairs;

            let computed_state = prost::bytes::Bytes::copy_from_slice(&state.bytes());
            // The floor is a deterministic function of the block's justifications,
            // so the merged state is always cacheable (no snapshot-LFB fallback).
            tracing::debug!(
                target: "f1r3fly.merge.step",
                step = "compute_parents_post_state.CACHE_PUT",
                path = "merged",
                post_state = %hex::encode(&computed_state[..8.min(computed_state.len())]),
                n_rejected = rejected.len(),
                "merge.step: cache put merged parents-post-state"
            );
            runtime_manager.put_cached_parents_post_state(
                cache_key,
                (
                    computed_state.clone(),
                    rejected.clone(),
                    rejected_state_effects.clone(),
                ),
            );
            tracing::debug!(
                target: "f1r3fly.casper.compute_parents_post_state.timing",
                "compute_parents_post_state timing: path=merged, parents={}, cache_lookup_ms={}, collect_ancestors_ms={}, flatten_visible_ms={}, floor_derive_ms={}, merge_ms={}, visible_blocks={}, rejected_deploys={}, total_ms={}",
                parents.len(),
                cache_lookup_ms,
                collect_ancestors_ms,
                flatten_visible_ms,
                floor_derive_ms,
                merge_ms,
                visible_blocks_len,
                rejected.len(),
                total_started.elapsed().as_millis()
            );
            tracing::debug!(
                target: "f1r3fly.merge.step",
                step = "compute_parents_post_state.EXIT",
                path = "merged",
                post_state = %hex::encode(&computed_state[..8.min(computed_state.len())]),
                n_rejected = rejected.len(),
                "merge.step: exit compute_parents_post_state"
            );
            Ok((computed_state, rejected, rejected_state_effects))
        }
    }
}

#[cfg(test)]
mod backstop_tests {
    use std::collections::{HashMap, HashSet};

    use block_storage::rust::dag::block_dag_key_value_storage::{
        BlockDagKeyValueStorage, InsertMode,
    };
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use models::rust::block_hash::BlockHash;
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::{
        BlockMessage, ProcessedDeploy, RejectedDeploy, RejectedDeployReason, StateEffectId,
    };
    use proptest::prelude::*;
    use prost::bytes::Bytes;
    use rspace_plus_plus::rspace::history::Either;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::{
        block_in_floor_merge_scope, canonical_rejected_sigs, canonical_won_sigs, handle_errors,
        merge_scope_backstop_exceeded, reduce_scope_events,
        rejected_sig_has_visible_non_source_win, retain_pending_rejected_deploys_for_buffer,
        state_effect_encoding_matches_protocol,
        suppress_rejected_pairs_with_later_visible_rejection, visible_rejected_deploy_sigs,
        SigEvents, EXACT_REJECTION_PROTOCOL_VERSION, MAX_FLOOR_DISTANCE_BLOCKS,
    };
    use crate::rust::block_status::BlockError;
    use crate::rust::util::construct_deploy;
    use crate::rust::util::rholang::replay_failure::ReplayFailure;

    #[test]
    fn state_effect_encoding_activates_only_at_protocol_three() {
        let first = StateEffectId {
            source_block_hash: Bytes::from_static(b"a"),
            execution_index: 0,
        };
        let second = StateEffectId {
            source_block_hash: Bytes::from_static(b"b"),
            execution_index: 0,
        };
        let canonical = vec![first.clone(), second.clone()];

        assert!(state_effect_encoding_matches_protocol(
            EXACT_REJECTION_PROTOCOL_VERSION,
            &[],
            &canonical,
        ));
        assert!(!state_effect_encoding_matches_protocol(
            EXACT_REJECTION_PROTOCOL_VERSION,
            std::slice::from_ref(&first),
            &canonical,
        ));
        let current = crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION;
        assert!(state_effect_encoding_matches_protocol(
            current, &canonical, &canonical,
        ));
        assert!(!state_effect_encoding_matches_protocol(
            current,
            &[second.clone(), first.clone()],
            &canonical,
        ));
        assert!(!state_effect_encoding_matches_protocol(
            current,
            &[first.clone(), first],
            &canonical,
        ));
        assert!(!state_effect_encoding_matches_protocol(
            current,
            std::slice::from_ref(&second),
            &canonical,
        ));
    }

    #[test]
    fn replay_admission_mismatch_is_objective_invalidity() {
        let result = handle_errors(
            Bytes::new(),
            Either::Left(ReplayFailure::replay_admission_mismatch(
                1,
                1,
                0,
                0,
                "realized cost exceeds certified bound".to_string(),
            )),
        )
        .expect("classification");

        assert!(matches!(result, Either::Right(None)));
    }

    #[test]
    fn internal_replay_error_is_local_fault() {
        let result = handle_errors(
            Bytes::new(),
            Either::Left(ReplayFailure::internal_error(
                "unknown local root".to_string(),
            )),
        )
        .expect("classification");

        assert!(matches!(
            result,
            Either::Left(BlockError::BlockException(_))
        ));
    }

    #[test]
    fn backstop_rejects_only_on_floor_distance_over_cap() {
        // At or below the cap: proceed (no backstop, build the merge).
        assert!(!merge_scope_backstop_exceeded(0));
        assert!(!merge_scope_backstop_exceeded(MAX_FLOOR_DISTANCE_BLOCKS));
        // Strictly above the cap: refuse — deterministic Err (propose parks;
        // validate rejects), never a silent lossy single-parent substitution.
        assert!(merge_scope_backstop_exceeded(MAX_FLOOR_DISTANCE_BLOCKS + 1));
        assert!(merge_scope_backstop_exceeded(i64::MAX));
    }

    #[test]
    fn backstop_decision_depends_on_floor_distance_only() {
        // The backstop is a pure function of the (node-deterministic) floor
        // distance Δ. The scope size |visible_blocks| is NOT a parameter — it
        // cannot influence the decision by construction, which is exactly the
        // anti-fork property (scope size is not node-deterministic, so gating on
        // it could reject differently across nodes and fork). This test pins that
        // the signature carries Δ only.
        let below = merge_scope_backstop_exceeded(MAX_FLOOR_DISTANCE_BLOCKS);
        let above = merge_scope_backstop_exceeded(MAX_FLOOR_DISTANCE_BLOCKS + 1);
        assert!(!below && above);
    }

    proptest! {
        #[test]
        fn recovery_projection_preserves_every_untombstoned_source(
            sources in prop::collection::btree_set(any::<u8>(), 1..16),
            rejected in prop::collection::btree_set(any::<u8>(), 0..16),
        ) {
            let sig = Bytes::from_static(b"sig");
            let occurrences: HashMap<_, _> = sources
                .iter()
                .map(|source| (Bytes::from(vec![*source]), i64::from(*source)))
                .collect();
            let exact_rejections: HashMap<_, _> = rejected
                .iter()
                .map(|source| (Bytes::from(vec![*source]), i64::from(*source) + 1))
                .collect();
            let expected: HashSet<_> = sources
                .difference(&rejected)
                .map(|source| Bytes::from(vec![*source]))
                .collect();
            let mut projection = reduce_scope_events(HashMap::from([(
                sig.clone(),
                SigEvents {
                    occurrences,
                    exact_rejections,
                    latest_legacy_rejection: None,
                },
            )]));
            let disposition = projection.remove(&sig).expect("sig disposition");

            prop_assert_eq!(disposition.won, !expected.is_empty());
            prop_assert_eq!(disposition.active_sources, expected);
        }
    }

    #[test]
    fn recovery_projection_rejects_only_after_every_source_is_tombstoned() {
        let sig = Bytes::from_static(b"sig");
        let source_a = Bytes::from_static(b"source-a");
        let source_b = Bytes::from_static(b"source-b");
        let mut projection = reduce_scope_events(HashMap::from([(sig.clone(), SigEvents {
            occurrences: HashMap::from([(source_a.clone(), 1), (source_b.clone(), 1)]),
            exact_rejections: HashMap::from([(source_a, 2), (source_b, 2)]),
            latest_legacy_rejection: None,
        })]));
        let disposition = projection.remove(&sig).expect("sig disposition");

        assert!(!disposition.won);
        assert!(disposition.active_sources.is_empty());
    }

    #[tokio::test]
    async fn canonical_disposition_reduction_fails_closed_on_missing_parent_body() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let missing = Bytes::from_static(b"missing-parent");

        let error = canonical_won_sigs(&block_store, &[missing], i64::MIN)
            .expect_err("missing parent body must fail closed");

        assert!(error
            .to_string()
            .contains("while reducing deploy occurrence dispositions"));
    }

    #[tokio::test]
    async fn recovery_disposition_helpers_fail_closed_on_missing_visible_bodies() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let finalized = block_implicits::get_random_block(
            Some(crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION),
            Some(1),
            None,
            None,
            None,
            None,
            Some(1),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        block_store
            .put_block_message(&finalized)
            .expect("store finalized block");
        let missing = Bytes::from_static(b"missing-visible");
        let visible_blocks = HashSet::from([missing.clone()]);

        let terminal_error = super::finalized_won_terminal_sigs(
            &block_store,
            &finalized.block_hash,
            std::slice::from_ref(&missing),
            i64::MIN,
            1,
        )
        .expect_err("missing visible threat body");
        assert!(terminal_error
            .to_string()
            .contains("checking finalized-source threats"));

        let non_source_error = rejected_sig_has_visible_non_source_win(
            &block_store,
            &visible_blocks,
            &Bytes::from_static(b"sig"),
            &finalized.block_hash,
        )
        .expect_err("missing visible non-source body");
        assert!(non_source_error
            .to_string()
            .contains("checking visible non-source wins"));

        let rejected_error = visible_rejected_deploy_sigs(&block_store, &visible_blocks)
            .expect_err("missing visible rejection body");
        assert!(rejected_error
            .to_string()
            .contains("collecting visible rejected deploys"));

        let mut pairs = vec![RejectedDeploy::occurrence(
            Bytes::from_static(b"sig"),
            finalized.block_hash,
            RejectedDeployReason::MergeConflict,
        )];
        let suppression_error = suppress_rejected_pairs_with_later_visible_rejection(
            &block_store,
            &visible_blocks,
            &mut pairs,
        )
        .expect_err("missing repeated-rejection body");
        assert!(suppression_error
            .to_string()
            .contains("suppressing repeated exact rejections"));
    }

    fn scope_test_block(
        block_number: i64,
        seq_num: i32,
        sender: Bytes,
        parents: Vec<BlockHash>,
    ) -> BlockMessage {
        block_implicits::get_random_block(
            Some(block_number),
            Some(seq_num),
            None,
            None,
            Some(sender),
            Some(crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION),
            Some(0),
            Some(parents),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        )
    }

    fn occurrence_test_block(
        block_number: i64,
        seq_num: i32,
        version: i64,
        parents: Vec<BlockHash>,
        deploys: Vec<ProcessedDeploy>,
    ) -> BlockMessage {
        block_implicits::get_random_block(
            Some(block_number),
            Some(seq_num),
            None,
            None,
            Some(Bytes::from(vec![version as u8; 65])),
            Some(version),
            Some(block_number),
            Some(parents),
            Some(Vec::new()),
            Some(deploys),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        )
    }

    #[tokio::test]
    async fn active_protocol_preserves_finalized_receipt_against_visible_tombstone() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");
        let version = crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION;
        let deploy = construct_deploy::source_deploy_now_full(
            "@1!(1)".to_string(),
            None,
            None,
            None,
            Some(0),
            None,
        )
        .expect("deploy");
        let sig = deploy.sig.clone();
        let genesis = occurrence_test_block(0, 0, version, Vec::new(), Vec::new());
        let floor = occurrence_test_block(1, 1, version, vec![genesis.block_hash.clone()], vec![
            ProcessedDeploy::empty(deploy),
        ]);
        let mut masking =
            occurrence_test_block(2, 2, version, vec![floor.block_hash.clone()], Vec::new());
        masking.body.rejected_deploys = vec![RejectedDeploy::occurrence(
            sig.clone(),
            floor.block_hash.clone(),
            RejectedDeployReason::DuplicateOccurrence,
        )];
        for block in [&genesis, &floor, &masking] {
            block_store.put_block_message(block).expect("store block");
        }
        dag_storage
            .insert(&genesis, InsertMode::ApprovedGenesis)
            .expect("insert genesis");
        dag_storage
            .insert(&floor, InsertMode::Normal)
            .expect("insert floor");
        dag_storage
            .insert(&masking, InsertMode::Normal)
            .expect("insert masking block");
        let dag = dag_storage.get_representation().expect("dag");
        let visible = HashSet::from([masking.block_hash.clone()]);

        let won = super::canonical_won_sigs_at_floor(
            &block_store,
            &floor.block_hash,
            std::slice::from_ref(&masking.block_hash),
            i64::MIN,
            version,
        )
        .expect("base-aware canonical wins");
        let terminal = super::finalized_won_terminal_sigs(
            &block_store,
            &floor.block_hash,
            std::slice::from_ref(&masking.block_hash),
            i64::MIN,
            version,
        )
        .expect("terminal finalized wins");
        let context = super::merge_occurrence_context(
            &block_store,
            &dag,
            &floor.block_hash,
            &visible,
            version,
        )
        .expect("merge occurrence context");

        assert!(won.contains(&sig));
        assert!(terminal.contains(&sig));
        assert!(context.base_committed_sigs.contains(&sig));
        assert!(context.scope_tombstones.is_empty());
    }

    #[tokio::test]
    async fn rejected_before_floor_remains_recoverable_after_floor() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");
        let version = crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION;
        let deploy = construct_deploy::source_deploy_now_full(
            "@2!(2)".to_string(),
            None,
            None,
            None,
            Some(0),
            None,
        )
        .expect("deploy");
        let sig = deploy.sig.clone();
        let genesis = occurrence_test_block(0, 0, version, Vec::new(), Vec::new());
        let source = occurrence_test_block(1, 1, version, vec![genesis.block_hash.clone()], vec![
            ProcessedDeploy::empty(deploy.clone()),
        ]);
        let mut rejected =
            occurrence_test_block(2, 2, version, vec![source.block_hash.clone()], Vec::new());
        rejected.body.rejected_deploys = vec![RejectedDeploy::occurrence(
            sig.clone(),
            source.block_hash.clone(),
            RejectedDeployReason::MergeConflict,
        )];
        let floor =
            occurrence_test_block(3, 3, version, vec![rejected.block_hash.clone()], Vec::new());
        let retry = occurrence_test_block(4, 4, version, vec![floor.block_hash.clone()], vec![
            ProcessedDeploy::empty(deploy),
        ]);
        for block in [&genesis, &source, &rejected, &floor, &retry] {
            block_store.put_block_message(block).expect("store block");
        }
        for (block, mode) in [
            (&genesis, InsertMode::ApprovedGenesis),
            (&source, InsertMode::Normal),
            (&rejected, InsertMode::Normal),
            (&floor, InsertMode::Normal),
            (&retry, InsertMode::Normal),
        ] {
            dag_storage.insert(block, mode).expect("insert block");
        }
        let dag = dag_storage.get_representation().expect("dag");
        let visible = HashSet::from([retry.block_hash.clone()]);
        let context = super::merge_occurrence_context(
            &block_store,
            &dag,
            &floor.block_hash,
            &visible,
            version,
        )
        .expect("merge occurrence context");
        let won = super::canonical_won_sigs_at_floor(
            &block_store,
            &floor.block_hash,
            std::slice::from_ref(&retry.block_hash),
            i64::MIN,
            version,
        )
        .expect("base-aware canonical wins");

        assert!(!context.base_committed_sigs.contains(&sig));
        assert!(context.scope_tombstones.is_empty());
        assert!(won.contains(&sig));
    }

    #[tokio::test]
    async fn merge_context_rejects_noncausal_tombstone() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");
        let version = crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION;
        let deploy = construct_deploy::source_deploy_now_full(
            "@3!(3)".to_string(),
            None,
            None,
            None,
            Some(0),
            None,
        )
        .expect("deploy");
        let sig = deploy.sig.clone();
        let floor = occurrence_test_block(0, 0, version, Vec::new(), Vec::new());
        let source = occurrence_test_block(1, 1, version, vec![floor.block_hash.clone()], vec![
            ProcessedDeploy::empty(deploy),
        ]);
        let mut sibling =
            occurrence_test_block(1, 1, version, vec![floor.block_hash.clone()], Vec::new());
        sibling.body.rejected_deploys = vec![RejectedDeploy::occurrence(
            sig,
            source.block_hash.clone(),
            RejectedDeployReason::MergeConflict,
        )];
        for block in [&floor, &source, &sibling] {
            block_store.put_block_message(block).expect("store block");
        }
        dag_storage
            .insert(&floor, InsertMode::ApprovedGenesis)
            .expect("insert floor");
        dag_storage
            .insert(&source, InsertMode::Normal)
            .expect("insert source");
        dag_storage
            .insert(&sibling, InsertMode::Normal)
            .expect("insert sibling");
        let dag = dag_storage.get_representation().expect("dag");
        let visible = HashSet::from([source.block_hash.clone(), sibling.block_hash.clone()]);

        let error = super::merge_occurrence_context(
            &block_store,
            &dag,
            &floor.block_hash,
            &visible,
            version,
        )
        .expect_err("noncausal tombstone must fail closed");
        assert!(error.to_string().contains("not causally descended"));
    }

    #[tokio::test]
    async fn merge_context_rejects_tombstone_for_absent_source_signature() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");
        let version = crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION;
        let floor = occurrence_test_block(0, 0, version, Vec::new(), Vec::new());
        let source =
            occurrence_test_block(1, 1, version, vec![floor.block_hash.clone()], Vec::new());
        let mut recording =
            occurrence_test_block(2, 2, version, vec![source.block_hash.clone()], Vec::new());
        recording.body.rejected_deploys = vec![RejectedDeploy::occurrence(
            Bytes::from_static(b"absent"),
            source.block_hash.clone(),
            RejectedDeployReason::MergeConflict,
        )];
        for block in [&floor, &source, &recording] {
            block_store.put_block_message(block).expect("store block");
        }
        dag_storage
            .insert(&floor, InsertMode::ApprovedGenesis)
            .expect("insert floor");
        dag_storage
            .insert(&source, InsertMode::Normal)
            .expect("insert source");
        dag_storage
            .insert(&recording, InsertMode::Normal)
            .expect("insert recording block");
        let dag = dag_storage.get_representation().expect("dag");
        let visible = HashSet::from([source.block_hash.clone(), recording.block_hash.clone()]);

        let error = super::merge_occurrence_context(
            &block_store,
            &dag,
            &floor.block_hash,
            &visible,
            version,
        )
        .expect_err("missing source signature must fail closed");
        assert!(error.to_string().contains("absent from source"));
    }

    #[tokio::test]
    async fn merge_context_canonically_joins_concurrent_rejection_reasons() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");
        let version = crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION;
        let deploy = construct_deploy::source_deploy_now_full(
            "@4!(4)".to_string(),
            None,
            None,
            None,
            Some(0),
            None,
        )
        .expect("deploy");
        let sig = deploy.sig.clone();
        let floor = occurrence_test_block(0, 0, version, Vec::new(), Vec::new());
        let source = occurrence_test_block(1, 1, version, vec![floor.block_hash.clone()], vec![
            ProcessedDeploy::empty(deploy),
        ]);
        let mut collateral =
            occurrence_test_block(2, 2, version, vec![source.block_hash.clone()], Vec::new());
        collateral.body.rejected_deploys = vec![RejectedDeploy::occurrence(
            sig.clone(),
            source.block_hash.clone(),
            RejectedDeployReason::CollateralChainDrop,
        )];
        let mut duplicate =
            occurrence_test_block(2, 3, version, vec![source.block_hash.clone()], Vec::new());
        duplicate.body.rejected_deploys = vec![RejectedDeploy::occurrence(
            sig.clone(),
            source.block_hash.clone(),
            RejectedDeployReason::DuplicateOccurrence,
        )];
        for block in [&floor, &source, &collateral, &duplicate] {
            block_store.put_block_message(block).expect("store block");
        }
        for (block, mode) in [
            (&floor, InsertMode::ApprovedGenesis),
            (&source, InsertMode::Normal),
            (&collateral, InsertMode::Normal),
            (&duplicate, InsertMode::Normal),
        ] {
            dag_storage.insert(block, mode).expect("insert block");
        }
        let dag = dag_storage.get_representation().expect("dag");
        let visible = HashSet::from([
            source.block_hash.clone(),
            collateral.block_hash.clone(),
            duplicate.block_hash.clone(),
        ]);

        let context = super::merge_occurrence_context(
            &block_store,
            &dag,
            &floor.block_hash,
            &visible,
            version,
        )
        .expect("concurrent causal reasons must converge");

        assert_eq!(
            context
                .scope_tombstones
                .get(&(sig, source.block_hash.clone())),
            Some(&RejectedDeployReason::DuplicateOccurrence)
        );
    }

    #[tokio::test]
    async fn protocol_v5_storage_rejects_legacy_scope_before_merge() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");
        let version = crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION;
        let floor = occurrence_test_block(0, 0, version, Vec::new(), Vec::new());
        let current =
            occurrence_test_block(1, 1, version, vec![floor.block_hash.clone()], Vec::new());
        let mut legacy = occurrence_test_block(1, 1, 1, vec![floor.block_hash.clone()], Vec::new());
        legacy.header.sender_bond_generation =
            Some(models::rust::bond_generation::BondGeneration::GENESIS);
        for block in [&floor, &current, &legacy] {
            block_store.put_block_message(block).expect("store block");
        }
        dag_storage
            .insert(&floor, InsertMode::ApprovedGenesis)
            .expect("insert floor");
        dag_storage
            .insert(&current, InsertMode::Normal)
            .expect("insert current block");
        let error = dag_storage
            .insert(&legacy, InsertMode::Normal)
            .err()
            .expect("legacy protocol block must not enter protocol-v5 storage");
        assert_eq!(
            error,
            shared::rust::store::key_value_store::KvStoreError::InvalidArgument(
                "admission outcome uses unsupported protocol version 1".to_string()
            )
        );
    }

    #[tokio::test]
    async fn disposition_reduction_rejects_protocol_incompatible_encoding() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let version = crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION;
        let mut current_with_legacy = occurrence_test_block(1, 1, version, Vec::new(), Vec::new());
        current_with_legacy.body.rejected_deploys =
            vec![RejectedDeploy::legacy(Bytes::from_static(b"legacy"))];
        let mut legacy_with_exact = occurrence_test_block(1, 1, 1, Vec::new(), Vec::new());
        legacy_with_exact.body.rejected_deploys = vec![RejectedDeploy::occurrence(
            Bytes::from_static(b"exact"),
            Bytes::from_static(b"source"),
            RejectedDeployReason::MergeConflict,
        )];
        for block in [&current_with_legacy, &legacy_with_exact] {
            block_store.put_block_message(block).expect("store block");
            let error = canonical_won_sigs(
                &block_store,
                std::slice::from_ref(&block.block_hash),
                i64::MIN,
            )
            .expect_err("incompatible encoding must fail closed");
            assert!(error.to_string().contains("encoding incompatible"));
        }
    }

    #[tokio::test]
    async fn floor_scope_drops_finalized_off_main_dag_ancestors() {
        let mut kvm = InMemoryStoreManager::new();
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");
        let secp = Secp256k1;
        let (_, v1_pk) = secp.new_key_pair();
        let (_, v2_pk) = secp.new_key_pair();
        let (_, v3_pk) = secp.new_key_pair();
        let v1 = v1_pk.bytes;
        let v2 = v2_pk.bytes;
        let v3 = v3_pk.bytes;
        let genesis = scope_test_block(0, 0, v1.clone(), Vec::new());
        let main = scope_test_block(1, 1, v1.clone(), vec![genesis.block_hash.clone()]);
        let off_main = scope_test_block(1, 1, v2.clone(), vec![genesis.block_hash.clone()]);
        let floor = scope_test_block(2, 2, v1.clone(), vec![
            main.block_hash.clone(),
            off_main.block_hash.clone(),
        ]);
        let outside = scope_test_block(1, 1, v3.clone(), vec![genesis.block_hash.clone()]);
        let outside_same_height =
            scope_test_block(2, 2, v3.clone(), vec![outside.block_hash.clone()]);
        let descendant = scope_test_block(3, 3, v1, vec![floor.block_hash.clone()]);

        for (block, mode) in [
            (&genesis, InsertMode::ApprovedGenesis),
            (&main, InsertMode::Normal),
            (&off_main, InsertMode::Normal),
            (&floor, InsertMode::Normal),
            (&outside, InsertMode::Normal),
            (&outside_same_height, InsertMode::Normal),
            (&descendant, InsertMode::Normal),
        ] {
            dag_storage.insert(block, mode).expect("insert block");
        }
        dag_storage
            .record_directly_finalized(floor.block_hash.clone(), 1.0, |_| async { Ok(()) })
            .await
            .expect("record floor finalized");
        let dag = dag_storage
            .get_representation()
            .expect("dag representation");
        let floor_block_number = dag
            .lookup_unsafe(&floor.block_hash)
            .expect("floor metadata")
            .block_number;

        assert!(dag.is_finalized(&off_main.block_hash));
        assert!(!dag
            .is_in_main_chain(&off_main.block_hash, &floor.block_hash)
            .expect("main-chain query"));
        assert!(dag
            .is_dag_ancestor(&off_main.block_hash, &floor.block_hash)
            .expect("dag ancestor query"));

        let mut visible_blocks: HashSet<_> = [
            genesis.block_hash.clone(),
            main.block_hash.clone(),
            off_main.block_hash.clone(),
            floor.block_hash.clone(),
            outside.block_hash.clone(),
            outside_same_height.block_hash.clone(),
            descendant.block_hash.clone(),
        ]
        .into_iter()
        .collect();
        visible_blocks.retain(|hash| {
            block_in_floor_merge_scope(&dag, hash, &floor.block_hash, floor_block_number)
                .expect("scope predicate")
        });

        assert!(!visible_blocks.contains(&genesis.block_hash));
        assert!(!visible_blocks.contains(&main.block_hash));
        assert!(!visible_blocks.contains(&off_main.block_hash));
        assert!(!visible_blocks.contains(&floor.block_hash));
        assert!(visible_blocks.contains(&outside.block_hash));
        assert!(visible_blocks.contains(&outside_same_height.block_hash));
        assert!(visible_blocks.contains(&descendant.block_hash));
    }

    #[tokio::test]
    async fn visible_non_source_win_in_lower_block_still_blocks_recovery_admission() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let deploy = construct_deploy::source_deploy_now_full(
            "@9!(9)".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("deploy");
        let sig = deploy.sig.clone();
        let lower_clean = block_implicits::get_random_block(
            Some(14),
            Some(1),
            None,
            None,
            None,
            None,
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy.clone())]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        let higher_source = block_implicits::get_random_block(
            Some(23),
            Some(1),
            None,
            None,
            None,
            None,
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy)]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        block_store
            .put_block_message(&lower_clean)
            .expect("store lower clean");
        block_store
            .put_block_message(&higher_source)
            .expect("store source");

        let visible_blocks: HashSet<_> = [
            lower_clean.block_hash.clone(),
            higher_source.block_hash.clone(),
        ]
        .into_iter()
        .collect();

        assert!(rejected_sig_has_visible_non_source_win(
            &block_store,
            &visible_blocks,
            &sig,
            &higher_source.block_hash,
        )
        .expect("visible win check"));
    }

    #[tokio::test]
    async fn later_visible_non_source_rejection_reopens_recovery_admission() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let deploy = construct_deploy::source_deploy_now_full(
            "@9!(9)".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("deploy");
        let sig = deploy.sig.clone();
        let lower_clean = block_implicits::get_random_block(
            Some(14),
            Some(1),
            None,
            None,
            None,
            None,
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy.clone())]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        let higher_source = block_implicits::get_random_block(
            Some(23),
            Some(1),
            None,
            None,
            None,
            None,
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy)]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        let mut later_rejection = block_implicits::get_random_block(
            Some(24),
            Some(1),
            None,
            None,
            None,
            Some(1),
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        later_rejection.body.rejected_deploys = vec![RejectedDeploy::legacy(sig.clone())];
        block_store
            .put_block_message(&lower_clean)
            .expect("store lower clean");
        block_store
            .put_block_message(&higher_source)
            .expect("store source");
        block_store
            .put_block_message(&later_rejection)
            .expect("store later rejection");

        let visible_blocks: HashSet<_> = [
            lower_clean.block_hash.clone(),
            higher_source.block_hash.clone(),
            later_rejection.block_hash.clone(),
        ]
        .into_iter()
        .collect();

        assert!(!rejected_sig_has_visible_non_source_win(
            &block_store,
            &visible_blocks,
            &sig,
            &higher_source.block_hash,
        )
        .expect("visible win check"));
    }

    #[tokio::test]
    async fn exact_rejection_preserves_another_source_as_canonical_win() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let deploy = construct_deploy::source_deploy_now_full(
            "@9!(9)".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("deploy");
        let sig = deploy.sig.clone();
        let survivor = block_implicits::get_random_block(
            Some(14),
            Some(1),
            None,
            None,
            None,
            None,
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy.clone())]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        let rejected_source = block_implicits::get_random_block(
            Some(14),
            Some(1),
            None,
            None,
            None,
            None,
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy)]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        let mut merge = block_implicits::get_random_block(
            Some(15),
            Some(1),
            None,
            None,
            None,
            Some(crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION),
            Some(0),
            Some(vec![
                survivor.block_hash.clone(),
                rejected_source.block_hash.clone(),
            ]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        merge.body.rejected_deploys = vec![RejectedDeploy::occurrence(
            sig.clone(),
            rejected_source.block_hash.clone(),
            RejectedDeployReason::DuplicateOccurrence,
        )];
        for block in [&survivor, &rejected_source, &merge] {
            block_store.put_block_message(block).expect("store block");
        }

        let parents = [merge.block_hash.clone()];
        assert!(canonical_won_sigs(&block_store, &parents, i64::MIN)
            .expect("canonical wins")
            .contains(&sig));
        assert!(!canonical_rejected_sigs(&block_store, &parents, i64::MIN)
            .expect("canonical rejections")
            .contains(&sig));
        let visible_blocks = HashSet::from([
            survivor.block_hash.clone(),
            rejected_source.block_hash.clone(),
            merge.block_hash.clone(),
        ]);
        assert!(rejected_sig_has_visible_non_source_win(
            &block_store,
            &visible_blocks,
            &sig,
            &rejected_source.block_hash,
        )
        .expect("visible win check"));
    }

    #[tokio::test]
    async fn floor_rejection_is_not_canonical_win() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let rejected_sig = Bytes::from_static(b"floor-rejected");
        let deploy = construct_deploy::source_deploy_now_full(
            "@7!(7)".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("deploy");
        let mut rejected_floor = block_implicits::get_random_block(
            Some(10),
            Some(1),
            None,
            None,
            None,
            Some(1),
            Some(10),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        rejected_floor.body.rejected_deploys = vec![RejectedDeploy::legacy(rejected_sig.clone())];
        let clean_floor = block_implicits::get_random_block(
            Some(11),
            Some(1),
            None,
            None,
            None,
            None,
            Some(11),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy.clone())]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        block_store
            .put_block_message(&rejected_floor)
            .expect("store rejected floor");
        block_store
            .put_block_message(&clean_floor)
            .expect("store clean floor");

        let rejected_floor_wins = canonical_won_sigs(
            &block_store,
            std::slice::from_ref(&rejected_floor.block_hash),
            i64::MIN,
        )
        .expect("canonical wins for rejected floor");
        let clean_floor_wins = canonical_won_sigs(
            &block_store,
            std::slice::from_ref(&clean_floor.block_hash),
            i64::MIN,
        )
        .expect("canonical wins for clean floor");

        assert!(!rejected_floor_wins.contains(&rejected_sig));
        assert!(clean_floor_wins.contains(&deploy.sig));
    }

    #[tokio::test]
    async fn visible_rejected_deploys_are_detected_from_visible_blocks() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let rejected_sig = Bytes::from_static(b"rejected");
        let unseen_sig = Bytes::from_static(b"unseen");
        let mut rejected_block = block_implicits::get_random_block(
            Some(1),
            Some(1),
            None,
            None,
            None,
            Some(1),
            Some(1),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        rejected_block.body.rejected_deploys = vec![RejectedDeploy::legacy(rejected_sig.clone())];
        let unseen_block = block_implicits::get_random_block(
            Some(1),
            Some(1),
            None,
            None,
            None,
            None,
            Some(1),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        block_store
            .put_block_message(&rejected_block)
            .expect("store rejected block");
        block_store
            .put_block_message(&unseen_block)
            .expect("store unseen block");

        let visible_blocks = [rejected_block.block_hash.clone()].into_iter().collect();
        let detected =
            visible_rejected_deploy_sigs(&block_store, &visible_blocks).expect("detect rejected");

        assert!(detected.contains(&rejected_sig));
        assert!(!detected.contains(&unseen_sig));
    }

    #[tokio::test]
    async fn older_visible_rejection_does_not_suppress_recovery_source_rejection() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let recovered_sig = Bytes::from_static(b"recovered");
        let duplicate_sig = Bytes::from_static(b"duplicate");
        let mut old_rejection = block_implicits::get_random_block(
            Some(10),
            Some(1),
            None,
            None,
            None,
            Some(1),
            Some(10),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        old_rejection.body.rejected_deploys = vec![RejectedDeploy::legacy(recovered_sig.clone())];
        let source = block_implicits::get_random_block(
            Some(20),
            Some(1),
            None,
            None,
            None,
            None,
            Some(20),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        let mut later_rejection = block_implicits::get_random_block(
            Some(21),
            Some(1),
            None,
            None,
            None,
            Some(crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION),
            Some(21),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        later_rejection.body.rejected_deploys = vec![RejectedDeploy::occurrence(
            duplicate_sig.clone(),
            source.block_hash.clone(),
            RejectedDeployReason::MergeConflict,
        )];
        block_store
            .put_block_message(&old_rejection)
            .expect("store old rejection");
        block_store
            .put_block_message(&source)
            .expect("store source");
        block_store
            .put_block_message(&later_rejection)
            .expect("store later rejection");

        let visible_blocks: HashSet<_> = [
            old_rejection.block_hash.clone(),
            source.block_hash.clone(),
            later_rejection.block_hash.clone(),
        ]
        .into_iter()
        .collect();
        let recovered = RejectedDeploy::occurrence(
            recovered_sig.clone(),
            source.block_hash.clone(),
            RejectedDeployReason::MergeConflict,
        );
        let duplicate = RejectedDeploy::occurrence(
            duplicate_sig.clone(),
            source.block_hash.clone(),
            RejectedDeployReason::MergeConflict,
        );
        let mut rejected_pairs = vec![recovered.clone(), duplicate];

        let suppressed = suppress_rejected_pairs_with_later_visible_rejection(
            &block_store,
            &visible_blocks,
            &mut rejected_pairs,
        )
        .expect("suppress rejected pairs");

        assert_eq!(suppressed, 1);
        assert_eq!(rejected_pairs, vec![recovered]);
    }

    #[tokio::test]
    async fn legacy_finalized_rejected_deploys_follow_height_ordering() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");

        let genesis = block_implicits::get_random_block(
            Some(0),
            Some(0),
            None,
            None,
            None,
            None,
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        let finalized = construct_deploy::source_deploy_now_full(
            "@1!(1)".to_string(),
            None,
            None,
            None,
            Some(0),
            None,
        )
        .expect("finalized deploy");
        let pending = construct_deploy::source_deploy_now_full(
            "@2!(2)".to_string(),
            None,
            None,
            None,
            Some(0),
            None,
        )
        .expect("pending deploy");
        let finalized_block = block_implicits::get_random_block(
            Some(1),
            Some(1),
            None,
            None,
            None,
            None,
            Some(1),
            Some(vec![genesis.block_hash.clone()]),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(finalized.clone())]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );

        block_store
            .put_block_message(&genesis)
            .expect("store genesis");
        block_store
            .put_block_message(&finalized_block)
            .expect("store finalized");
        dag_storage
            .insert(&genesis, InsertMode::ApprovedGenesis)
            .expect("insert genesis");
        dag_storage
            .insert(&finalized_block, InsertMode::Normal)
            .expect("insert finalized");

        let mut dag = dag_storage.get_representation().expect("dag");
        dag.last_finalized_block_hash = finalized_block.block_hash.clone();

        // Rejection below the finalized win (late validation of an
        // already-recovered rejection): the finalized sig is terminal.
        let mut deploys = vec![finalized.clone(), pending.clone()];
        let skipped =
            retain_pending_rejected_deploys_for_buffer(&dag, &block_store, 50, &mut deploys, 0, 1)
                .expect("retain pending");

        assert_eq!(skipped, 1);
        assert_eq!(deploys.len(), 1);
        assert_eq!(deploys[0].sig, pending.sig);

        // Rejection ABOVE the finalized win: `Finalized` is not terminal —
        // the pending rejection can finalize and drop the win's effects, so
        // the deploy must stay buffered (finalized-win blindness fix).
        let mut deploys = vec![finalized.clone(), pending.clone()];
        let skipped =
            retain_pending_rejected_deploys_for_buffer(&dag, &block_store, 50, &mut deploys, 2, 1)
                .expect("retain pending with rejection above win");

        assert_eq!(skipped, 0);
        assert_eq!(deploys.len(), 2);
    }

    #[tokio::test]
    async fn exact_protocol_finalized_receipt_is_terminal_at_every_rejection_height() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");
        let version = crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION;
        let genesis = block_implicits::get_random_block(
            Some(0),
            Some(0),
            None,
            None,
            None,
            Some(version),
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        let finalized = construct_deploy::source_deploy_now_full(
            "@1!(1)".to_string(),
            None,
            None,
            None,
            Some(0),
            None,
        )
        .expect("finalized deploy");
        let finalized_block = block_implicits::get_random_block(
            Some(1),
            Some(1),
            None,
            None,
            None,
            Some(version),
            Some(1),
            Some(vec![genesis.block_hash.clone()]),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(finalized.clone())]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some("root".to_string()),
            None,
        );
        block_store
            .put_block_message(&genesis)
            .expect("store genesis");
        block_store
            .put_block_message(&finalized_block)
            .expect("store finalized");
        dag_storage
            .insert(&genesis, InsertMode::ApprovedGenesis)
            .expect("insert genesis");
        dag_storage
            .insert(&finalized_block, InsertMode::Normal)
            .expect("insert finalized");
        let mut dag = dag_storage.get_representation().expect("dag");
        dag.last_finalized_block_hash = finalized_block.block_hash.clone();

        for rejecting_height in [0, 2, i64::MAX] {
            let mut deploys = vec![finalized.clone()];
            let skipped = retain_pending_rejected_deploys_for_buffer(
                &dag,
                &block_store,
                50,
                &mut deploys,
                rejecting_height,
                version,
            )
            .expect("filter finalized receipt");
            assert_eq!(skipped, 1);
            assert!(deploys.is_empty());
        }
    }
}
