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
    BlockMessage, Bond, DeployData, ProcessedDeploy, ProcessedSystemDeploy,
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
use crate::rust::block_status::BlockStatus;
use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;
use crate::rust::merging::block_index::BlockIndex;
use crate::rust::merging::dag_merger;
use crate::rust::merging::deploy_chain_index::DeployChainIndex;
use crate::rust::metrics_constants::{BLOCK_PROCESSING_REPLAY_TIME_METRIC, CASPER_METRICS_SOURCE};
use crate::rust::util::proto_util;
use crate::rust::BlockProcessing;

pub fn mk_term(rho: &str, normalizer_env: HashMap<String, Par>) -> Result<Par, InterpreterError> {
    Compiler::source_to_adt_with_normalizer_env(rho, normalizer_env)
}

/// Pre-compute admit decisions for a batch of rejected-deploy sigs in a
/// single canonical-chain scan. The returned set contains the sigs that
/// *should* be admitted to the rejected-deploy buffer — those whose
/// current finalization state is `Pending`. Sigs whose state is terminal
/// (`Finalized` / `Failed` / `Expired`) are absent from the returned
/// set: they have already been resolved in the local canonical view and
/// must not be re-proposed (re-proposal would either waste a slot or,
/// in the catchup case, cause re-execution of already-canonical work
/// against a different pre-state).
///
/// Catastrophic resolver failures (LFB lookup, block-store IO during
/// the prelude) are treated conservatively as "do not admit" for any
/// sig in the batch — transient failures must not open the
/// re-execution hazard; sigs will be retried on the next merge
/// rejection if still live.
///
/// This is the batched replacement for the previous per-sig
/// `should_admit_to_rejected_buffer`. Cost: one BFS over the
/// `deploy_lifespan` window regardless of sig count, instead of one
/// BFS per sig. For an N-rejected merge with M-block window, this is
/// O(M + N) block fetches versus O(N · M).
fn compute_rejected_buffer_admits(
    dag: &block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    deploy_lifespan: i64,
    sigs: &HashSet<Bytes>,
) -> HashSet<Bytes> {
    use crate::rust::api::deploy_finalization_status::{resolve_batch, DeployFinalizationState};
    let __admits_start = std::time::Instant::now();
    if sigs.is_empty() {
        metrics::histogram!(
            crate::rust::metrics_constants::COMPUTE_REJECTED_BUFFER_ADMITS_TIME_METRIC,
            "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
        )
        .record(__admits_start.elapsed().as_secs_f64());
        return HashSet::new();
    }
    let result = match resolve_batch(dag, block_store, deploy_lifespan, sigs) {
        Ok(statuses) => statuses
            .into_iter()
            .filter_map(|(sig, status)| {
                if status.state == DeployFinalizationState::Pending {
                    tracing::debug!(
                        target: "f1r3.trace.recovery_gate",
                        event = "gate_admit",
                        sig = %hex::encode(&sig[..sig.len().min(16)]),
                        "rejected deploy admitted to recovery buffer (status=Pending)"
                    );
                    Some(sig)
                } else {
                    tracing::debug!(
                        target: "f1r3.trace.recovery_gate",
                        event = "gate_skip",
                        sig = %hex::encode(&sig[..sig.len().min(16)]),
                        state = ?status.state,
                        "rejected deploy NOT admitted to recovery buffer (terminal status — dropped from recovery)"
                    );
                    None
                }
            })
            .collect(),
        Err(err) => {
            tracing::warn!(
                "RejectedDeployBuffer populate: batched status check failed: {} — admitting nothing for this merge",
                err
            );
            HashSet::new()
        }
    };
    metrics::histogram!(
        crate::rust::metrics_constants::COMPUTE_REJECTED_BUFFER_ADMITS_TIME_METRIC,
        "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
    )
    .record(__admits_start.elapsed().as_secs_f64());
    result
}

fn with_ancestors_capped(
    dag: &KeyValueDagRepresentation,
    block_hash: &BlockHash,
    max_nodes: usize,
) -> Result<Option<HashSet<BlockHash>>, CasperError> {
    if max_nodes == 0 {
        return Ok(None);
    }

    let mut visited: HashSet<BlockHash> = HashSet::new();
    let mut queue: VecDeque<BlockHash> = VecDeque::from([block_hash.clone()]);

    while let Some(current_hash) = queue.pop_front() {
        if !visited.insert(current_hash.clone()) {
            continue;
        }
        if visited.len() >= max_nodes {
            return Ok(None);
        }

        let metadata = dag.lookup_unsafe(&current_hash)?;
        for parent in metadata.parents {
            if !visited.contains(&parent) {
                queue.push_back(parent);
            }
        }
    }

    Ok(Some(visited))
}

// Returns (None, checkpoints) if the block's tuplespace hash
// does not match the computed hash based on the deploys
pub async fn validate_block_checkpoint(
    block: &BlockMessage,
    block_store: &KeyValueBlockStore,
    s: &mut CasperSnapshot,
    runtime_manager: &RuntimeManager,
    rejected_deploy_buffer: Option<&std::sync::Arc<std::sync::Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>>,
) -> Result<BlockProcessing<Option<StateHash>>, CasperError> {
    tracing::trace!(target: "f1r3fly.casper.block_validation", "before-unsafe-get-parents");
    let incoming_pre_state_hash = proto_util::pre_state_hash(block);
    let parents = proto_util::get_parents(block_store, block);
    // Floor snapshot = the block's SIGNED justifications: node-identical at
    // validate. The snapshot's live justification view is the proposer's view
    // and would be wrong here.
    let latest_messages: BTreeMap<Validator, BlockHash> = block
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    tracing::trace!(target: "f1r3fly.casper.block_validation", parent_count = parents.len(), "before-compute-parents-post-state");
    let parents_post_state_start = std::time::Instant::now();
    let computed_parents_info = compute_parents_post_state(
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
        Ok((computed_pre_state_hash, rejected_deploys, _rejected_slashes)) => {
            // (sig, host) pairs: validate the host too, so a proposer cannot forge
            // which inclusion a rejection blames (the per-inclusion guard relies on it).
            let rejected_deploy_ids: HashSet<_> = rejected_deploys.iter().cloned().collect();
            let block_rejected_deploy_sigs: HashSet<_> = block
                .body
                .rejected_deploys
                .iter()
                .map(|d| (d.sig.clone(), d.host.clone()))
                .collect();

            if incoming_pre_state_hash != computed_pre_state_hash {
                // TODO: at this point we may just as well terminate the replay, there's no way it will succeed.
                tracing::warn!(
                    "Computed pre-state hash {} does not equal block's pre-state hash {}.",
                    PrettyPrinter::build_string_bytes(&computed_pre_state_hash),
                    PrettyPrinter::build_string_bytes(&incoming_pre_state_hash)
                );

                Ok(Either::Right(None))
            } else if rejected_deploy_ids != block_rejected_deploy_sigs {
                // Detailed logging for InvalidRejectedDeploy mismatch
                let extra_in_computed: Vec<_> = rejected_deploy_ids
                    .difference(&block_rejected_deploy_sigs)
                    .cloned()
                    .collect();
                let missing_in_computed: Vec<_> = block_rejected_deploy_sigs
                    .difference(&rejected_deploy_ids)
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
                    validator_rejected = rejected_deploy_ids.len(),
                    block_rejected = block_rejected_deploy_sigs.len(),
                    extra_count = extra_in_computed.len(),
                    missing_count = missing_in_computed.len(),
                    duplicate_count,
                    "rejected deploy mismatch: validator and block creator disagree on rejected deploys"
                );

                Ok(Either::Left(BlockStatus::invalid_rejected_deploy()))
            } else {
                tracing::debug!(target: "f1r3fly.casper.replay_block", "before-process-pre-state-hash");
                // Using tracing events for async - Span[F] equivalent from Scala
                tracing::debug!(target: "f1r3fly.casper.replay_block", "replay-block-started");
                let replay_start = std::time::Instant::now();
                let replay_result =
                    replay_block(incoming_pre_state_hash, block, &mut s.dag, runtime_manager)
                        .await?;
                metrics::histogram!(BLOCK_PROCESSING_REPLAY_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .record(replay_start.elapsed().as_secs_f64());
                tracing::debug!(target: "f1r3fly.casper.replay_block", "replay-block-finished");

                handle_errors(proto_util::post_state_hash(block), replay_result)
            }
        }
        Err(ex) => Ok(Either::Left(BlockStatus::exception(ex))),
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
    let internal_system_deploys = proto_util::system_deploys(block);

    // Check for duplicate deploys in the block before replay
    let mut all_deploy_sigs: Vec<Bytes> = internal_deploys
        .iter()
        .map(|pd| pd.deploy.sig.clone())
        .collect();
    all_deploy_sigs.extend(block.body.rejected_deploys.iter().map(|rd| rd.sig.clone()));

    let mut sig_counts: HashMap<Bytes, usize> = HashMap::new();
    for sig in &all_deploy_sigs {
        *sig_counts.entry(sig.clone()).or_insert(0) += 1;
    }
    let deploy_duplicates: HashMap<Bytes, usize> = sig_counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .collect();

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

    // Get invalid blocks set from DAG
    let invalid_blocks_set = dag.invalid_blocks();

    // Get unseen block hashes
    let unseen_blocks_set = proto_util::unseen_block_hashes(dag, block)?;

    // Filter out invalid blocks that are unseen
    let seen_invalid_blocks_set: Vec<_> = invalid_blocks_set
        .iter()
        .filter(|invalid_block| !unseen_blocks_set.contains(&invalid_block.block_hash))
        .cloned()
        .collect();
    // TODO: Write test in which switching this to .filter makes it fail

    // Convert to invalid blocks map
    let invalid_blocks: HashMap<BlockHash, Validator> = seen_invalid_blocks_set
        .into_iter()
        .map(|invalid_block| (invalid_block.block_hash, invalid_block.sender))
        .collect();

    // Create block data and check if genesis
    let block_data = BlockData::from_block(block);
    let is_genesis = block.header.parents_hash_list.is_empty();

    // Implement retry logic with limit of 3 retries
    let mut attempts = 0;
    const MAX_RETRIES: usize = 3;

    loop {
        // Call the async replay_compute_state method
        let replay_result = runtime_manager
            .replay_compute_state(
                &initial_state_hash,
                internal_deploys.clone(),
                internal_system_deploys.clone(),
                &block_data,
                Some(invalid_blocks.clone()),
                is_genesis,
            )
            .await;

        match replay_result {
            Ok(computed_state_hash) => {
                // Check if computed hash matches expected hash
                if computed_state_hash == block.body.state.post_state_hash {
                    // Success - hashes match
                    return Ok(Either::Right(computed_state_hash));
                } else if attempts >= MAX_RETRIES {
                    // Give up after max retries
                    tracing::error!(
                        block_hash = %PrettyPrinter::build_string_no_limit(&block.block_hash),
                        expected = %PrettyPrinter::build_string_no_limit(&block.body.state.post_state_hash),
                        computed = %PrettyPrinter::build_string_no_limit(&computed_state_hash),
                        attempts,
                        "replay tuple space mismatch: giving up after max retries"
                    );
                    return Ok(Either::Right(computed_state_hash));
                } else {
                    // Retry - log error and continue
                    tracing::error!(
                        block_hash = %PrettyPrinter::build_string_no_limit(&block.block_hash),
                        expected = %PrettyPrinter::build_string_no_limit(&block.body.state.post_state_hash),
                        computed = %PrettyPrinter::build_string_no_limit(&computed_state_hash),
                        attempt = attempts + 1,
                        "replay tuple space mismatch: retrying"
                    );
                    attempts += 1;
                }
            }
            Err(replay_error) => {
                if attempts >= MAX_RETRIES {
                    // Give up after max retries
                    tracing::error!(
                        block_hash = %PrettyPrinter::build_string_no_limit(&block.block_hash),
                        error = ?replay_error,
                        attempts,
                        "replay failed: giving up after max retries"
                    );
                    // Convert CasperError to ReplayFailure::InternalError
                    return Ok(Either::Left(ReplayFailure::internal_error(
                        replay_error.to_string(),
                    )));
                } else {
                    // Retry - log error and continue
                    tracing::error!(
                        block_hash = %PrettyPrinter::build_string_no_limit(&block.block_hash),
                        error = ?replay_error,
                        attempt = attempts + 1,
                        "replay failed: retrying"
                    );
                    attempts += 1;
                }
            }
        }
    }
}

fn handle_errors(
    ts_hash: StateHash,
    result: Either<ReplayFailure, StateHash>,
) -> Result<BlockProcessing<Option<StateHash>>, CasperError> {
    match result {
        Either::Left(replay_failure) => match replay_failure {
            ReplayFailure::InternalError { msg } => {
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

pub async fn compute_deploys_checkpoint(
    block_store: &mut KeyValueBlockStore,
    parents: Vec<BlockMessage>,
    deploys: Vec<Signed<DeployData>>,
    system_deploys: Vec<super::system_deploy_enum::SystemDeployEnum>,
    s: &CasperSnapshot,
    runtime_manager: &RuntimeManager,
    block_data: BlockData,
    invalid_blocks: HashMap<BlockHash, Validator>,
    latest_messages: &BTreeMap<Validator, BlockHash>,
    rejected_deploy_buffer: Option<&std::sync::Arc<std::sync::Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>>,
) -> Result<
    (
        StateHash,
        StateHash,
        Vec<ProcessedDeploy>,
        // Rejected user deploys as (sig, host) — carried into the block body.
        Vec<(prost::bytes::Bytes, BlockHash)>,
        Vec<ProcessedSystemDeploy>,
        Vec<Bond>,
    ),
    CasperError,
> {
    let checkpoint_started = std::time::Instant::now();
    // Using tracing events for async - Span[F] equivalent from Scala
    tracing::debug!(target: "f1r3fly.casper.compute_deploys_checkpoint", "compute-deploys-checkpoint-started");
    // Ensure parents are not empty
    if parents.is_empty() {
        return Err(CasperError::RuntimeError(
            "Parents must not be empty".to_string(),
        ));
    }

    // Compute parents post state
    let parents_started = std::time::Instant::now();
    let computed_parents_info = compute_parents_post_state(
        block_store,
        parents,
        s,
        runtime_manager,
        latest_messages,
        None,
        rejected_deploy_buffer,
    )
    .await?;
    let parents_ms = parents_started.elapsed().as_millis();
    let (pre_state_hash, rejected_deploys, _rejected_slashes) = computed_parents_info;

    // Compute state and bonds using one spawned runtime
    let compute_state_started = std::time::Instant::now();
    let result = runtime_manager
        .compute_state_with_bonds(
            &pre_state_hash,
            deploys,
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
        processed_system_deploys,
        bonds,
    ))
}

/// Build (or fetch from cache) the [`BlockIndex`] for a block — the per-block
/// deploy-chain / mergeable index consumed by both the parent-state merge and
/// the floor-state seal, so the two merge paths share one definition.
/// Ensure every block in the merge `scope` has its mergeable-channels entry
/// materialized before `dag_merger::merge` reads them through the (synchronous)
/// `block_index_f` closure.
///
/// The merge requires the entry for every scope block, but its presence is a
/// byproduct of local execution: a block imported via LFS without replay — or
/// rejected locally and never replayed — lacks it and `load_mergeable_channels`
/// hard-errors `KeyNotFound`. That made merge validity node-local — the same
/// block could finalize on nodes that replayed it and be rejected on nodes
/// that did not. Recomputing the missing entries here (a deterministic full
/// replay) makes mergeable presence a function of block content on every node.
/// Healthy nodes pay only a cached-presence check per scope block.
pub(crate) async fn ensure_scope_mergeable_present(
    block_store: &KeyValueBlockStore,
    runtime_manager: &RuntimeManager,
    dag: &KeyValueDagRepresentation,
    scope: &HashSet<BlockHash>,
) -> Result<(), CasperError> {
    for hash in scope {
        // Fast path: a cached BlockIndex implies load_mergeable_channels already
        // succeeded for this block, so its (persistent) entry is present.
        if runtime_manager.block_index_cache.contains_key(hash) {
            continue;
        }
        let block = block_store.get_unsafe(hash);
        if runtime_manager.has_mergeable_entry(&block)? {
            continue;
        }

        // Reproduce the block's *seen* invalid-block set (the set its original
        // validation used) so the replay reproduces its post_state_hash and the
        // entry is stored under the correct key. `unseen_block_hashes` needs a
        // mutable representation handle; the clone is cheap (imbl/Arc-backed)
        // and taken only on this recompute path.
        let mut dag_scratch = dag.clone();
        let invalid = dag_scratch.invalid_blocks();
        let unseen = proto_util::unseen_block_hashes(&mut dag_scratch, &block)?;
        let invalid_blocks: HashMap<BlockHash, Validator> = invalid
            .iter()
            .filter(|ib| !unseen.contains(&ib.block_hash))
            .map(|ib| (ib.block_hash.clone(), ib.sender.clone()))
            .collect();

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

pub fn build_block_index(
    runtime_manager: &RuntimeManager,
    block_store: &KeyValueBlockStore,
    hash: &BlockHash,
) -> Result<BlockIndex, CasperError> {
    if let Some(cached) = runtime_manager.block_index_cache.get(hash) {
        return Ok((*cached.value()).clone());
    }

    let b = block_store.get_unsafe(hash);
    let pre_state = &b.body.state.pre_state_hash;
    let post_state = &b.body.state.post_state_hash;
    let sender = b.sender.clone();
    let seq_num = b.seq_num;

    let mergeable_chs = runtime_manager.load_mergeable_channels(post_state, sender, seq_num)?;

    let block_index = crate::rust::merging::block_index::new(
        &b.block_hash,
        b.body.state.block_number,
        &b.body.deploys,
        &b.body.system_deploys,
        &Blake2b256Hash::from_bytes_prost(pre_state),
        &Blake2b256Hash::from_bytes_prost(post_state),
        &runtime_manager.history_repo,
        &mergeable_chs,
    )?;

    runtime_manager
        .block_index_cache
        .insert(hash.clone(), block_index.clone());

    Ok(block_index)
}

/// True iff a recovered deploy's effect is already present in `base_state` — i.e.
/// the deploy already executed in the lineage this block builds on (the "flip":
/// kept on a branch the base descends from, while a sibling merge rejected it
/// into the recovery buffer). Re-executing such a deploy re-creates its per-deploy
/// number cells (content-twin) and re-charges its vault, so the proposer must skip
/// it. The check reads the actual pre-state, so it is correct for both merged and
/// fast-pathed bases — exactly where buffer membership and the ancestry
/// paper-trail are blind.
///
/// Signal: the deploy's own number-cell produces. Each carries a sig-derived rnd,
/// so its `Produce` identity (channel + datum + persist) is unique to this deploy
/// and a match is false-positive-free even on a shared channel. We rebuild the
/// deploy's per-deploy event-log index from the block that executed it, take its
/// created-and-not-destroyed produces on number channels, and test each against
/// the base with the same `Datum.source == Produce` match the block index uses.
/// Any present ⇒ the deploy ran in this base.
pub fn recovered_deploy_effect_in_base(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    runtime_manager: &RuntimeManager,
    base_state: &Blake2b256Hash,
    sig: &Bytes,
) -> Result<bool, CasperError> {
    // The block that executed the deploy. The deploy index records `body.deploys`
    // inclusions only, so the resolved block carries the deploy's `deploy_log` and
    // a per-index mergeable map. A deploy with no such block never ran -> not in base.
    let Some(origin_hash) = dag
        .lookup_by_deploy_id(&sig.to_vec())
        .map_err(CasperError::KvStoreError)?
    else {
        return Ok(false);
    };
    let Some(origin) = block_store.get(&origin_hash)? else {
        return Ok(false);
    };
    let Some(idx) = origin
        .body
        .deploys
        .iter()
        .position(|pd| pd.deploy.sig == *sig)
    else {
        return Ok(false);
    };

    // The deploy's own number channels, aligned per-deploy by index.
    let mergeable_chs = runtime_manager.load_mergeable_channels(
        &origin.body.state.post_state_hash,
        origin.sender.clone(),
        origin.seq_num,
    )?;
    let Some(merge_chs) = mergeable_chs.get(idx) else {
        return Ok(false);
    };
    if merge_chs.is_empty() {
        return Ok(false);
    }

    // Value-agnostic channel check. The collision channel is sig-derived, so its
    // NAME is stable across executions even when its VALUE is not (gas/PoS amounts,
    // map contents differ by base) — content matching the datum is therefore
    // unreliable; channel presence is not.
    //
    // Restrict to the deploy's OWN per-deploy cells: a channel this deploy touched
    // (in its mergeable set) that did NOT exist in the block-that-executed-it's
    // pre-state is one this deploy created (its gas / `new`-site cells, sig-unique).
    // Channels that pre-existed (the shared PoS / vault state the pre-charge reads)
    // carry data unrelated to this deploy and are excluded — checking them would
    // false-positive. If one of the deploy's own created cells is already populated
    // in the base, the deploy already executed in this base's lineage, so
    // re-executing it would double its cell (content-twin); skip it.
    let origin_pre = Blake2b256Hash::from_bytes_prost(&origin.body.state.pre_state_hash);
    let origin_pre_reader = runtime_manager
        .history_repo
        .get_history_reader(&origin_pre)
        .map_err(CasperError::HistoryError)?;
    let base_reader = runtime_manager
        .history_repo
        .get_history_reader(base_state)
        .map_err(CasperError::HistoryError)?;
    for (ch, _) in merge_chs.iter() {
        let created_by_deploy = origin_pre_reader
            .get_data(ch)
            .map_err(CasperError::HistoryError)?
            .is_empty();
        if !created_by_deploy {
            continue;
        }
        let in_base = !base_reader
            .get_data(ch)
            .map_err(CasperError::HistoryError)?
            .is_empty();
        tracing::debug!(
            target: "f1r3.trace.basecheck",
            sig = %hex::encode(&sig[..sig.len().min(8)]),
            channel = %hex::encode(&ch.bytes()[..6]),
            in_base,
            "base-check per-deploy created cell"
        );
        if in_base {
            return Ok(true);
        }
    }
    tracing::debug!(
        target: "f1r3.trace.basecheck",
        sig = %hex::encode(&sig[..sig.len().min(8)]),
        merge_chs = merge_chs.len(),
        "base-check: deploy effect NOT found in base (will re-execute)"
    );
    Ok(false)
}

/// Upper bound on how far below the floor a straddling parent's cone may fork.
/// Purely anti-DoS: a tip forked pathologically deep would otherwise force
/// every merge to walk and conflict-check history back to its fork point. The
/// retention twin of the FloorData store — both can be generous.
const MAX_STRADDLE_DEPTH_BLOCKS: i64 = 1024;

/// Compute the merged post-state from multiple parent blocks.
///
/// Multi-parent pre-states are built on the sealed finalized state: base =
/// `FS(floor)` where `floor` is derived from `latest_messages` (the block's
/// justification snapshot), and the conflict scope is the closure difference
/// `closure(parents) \ closure(floor)` — straddler cones included, no parent
/// filtering. Every input is in-block or sealed, so the result is a pure
/// function of (parents, justifications).
///
/// For exploratory deploy, pass `disable_late_block_filtering_override = Some(true)` to
/// always disable late block filtering (see full merged state).
/// For normal block creation, pass `None` to use the shard config value.
#[tracing::instrument(
    target = "f1r3fly.casper.compute_parents_post_state",
    name = "compute-parents-post-state",
    level = "debug",
    skip_all
)]
pub async fn compute_parents_post_state(
    block_store: &KeyValueBlockStore,
    parents: Vec<BlockMessage>,
    s: &CasperSnapshot,
    runtime_manager: &RuntimeManager,
    latest_messages: &BTreeMap<Validator, BlockHash>,
    disable_late_block_filtering_override: Option<bool>,
    rejected_deploy_buffer: Option<&std::sync::Arc<std::sync::Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>>,
) -> Result<
    (
        StateHash,
        // Rejected user deploys as (sig, host): host = the rejected inclusion's
        // source block, carried into the body for the per-inclusion guard.
        Vec<(Bytes, BlockHash)>,
        Vec<crate::rust::merging::rejected_slash::RejectedSlash>,
    ),
    CasperError,
> {
    let total_started = std::time::Instant::now();
    const MAX_FULL_ANCESTOR_SCAN_NODES: usize = 8_192;

    match parents.len() {
        // For genesis, use empty trie's root hash
        0 => {
            let state = RuntimeManager::empty_state_hash_fixed();
            tracing::debug!(
                target: "f1r3fly.casper.compute_parents_post_state.timing",
                "compute_parents_post_state timing: path=genesis, parents=0, total_ms={}",
                total_started.elapsed().as_millis()
            );
            Ok((state, Vec::new(), Vec::new()))
        }

        // For single parent, get its post state hash
        1 => {
            let parent = &parents[0];
            let state = proto_util::post_state_hash(parent);
            tracing::debug!(
                target: "f1r3fly.casper.compute_parents_post_state.timing",
                "compute_parents_post_state timing: path=single_parent, parents=1, total_ms={}",
                total_started.elapsed().as_millis()
            );
            Ok((state, Vec::new(), Vec::new()))
        }

        // Multiple parents - we might want to take some data from the parent with the most stake,
        // e.g. bonds map, slashing deploys, bonding deploys.
        // such system deploys are not mergeable, so take them from one of the parents.
        _ => {
            let cache_lookup_started = std::time::Instant::now();
            // Fast path: if one parent is descendant of all others, its post-state already
            // includes all effects from the remaining parents and we can skip DAG merge.
            for candidate in &parents {
                let covers_all = parents
                    .iter()
                    .filter(|p| p.block_hash != candidate.block_hash)
                    .all(|p| {
                        s.dag
                            .is_in_main_chain(&p.block_hash, &candidate.block_hash)
                            .unwrap_or(false)
                    });
                if covers_all {
                    tracing::debug!(
                        target: "f1r3fly.casper.compute_parents_post_state.fast_path",
                        "compute_parents_post_state fast path: descendant parent {} covers all {} parents",
                        PrettyPrinter::build_string_bytes(&candidate.block_hash),
                        parents.len()
                    );
                    let state = proto_util::post_state_hash(candidate);
                    tracing::debug!(
                        target: "f1r3fly.casper.compute_parents_post_state.timing",
                        "compute_parents_post_state timing: path=descendant_fast_path, parents={}, cache_lookup_ms={}, total_ms={}",
                        parents.len(),
                        cache_lookup_started.elapsed().as_millis(),
                        total_started.elapsed().as_millis()
                    );
                    return Ok((state, Vec::new(), Vec::new()));
                }
            }

            // Broader fast path: if one parent is an ancestor-descendant cover in DAG
            // (not only on the main-parent chain), its post-state already subsumes the
            // remaining parents and merge can be skipped safely.
            if parents.len() <= 8 {
                let parent_hashes: HashSet<BlockHash> =
                    parents.iter().map(|p| p.block_hash.clone()).collect();
                for candidate in &parents {
                    let Ok(Some(candidate_closure)) = with_ancestors_capped(
                        &s.dag,
                        &candidate.block_hash,
                        MAX_FULL_ANCESTOR_SCAN_NODES,
                    ) else {
                        continue;
                    };

                    let covers_all = parent_hashes
                        .iter()
                        .filter(|hash| **hash != candidate.block_hash)
                        .all(|hash| candidate_closure.contains(hash));

                    if covers_all {
                        tracing::debug!(
                            target: "f1r3fly.casper.compute_parents_post_state.fast_path",
                            "compute_parents_post_state fast path: dag-descendant parent {} covers all {} parents",
                            PrettyPrinter::build_string_bytes(&candidate.block_hash),
                            parents.len()
                        );
                        let state = proto_util::post_state_hash(candidate);
                        tracing::debug!(
                            target: "f1r3fly.casper.compute_parents_post_state.timing",
                            "compute_parents_post_state timing: path=dag_descendant_fast_path, parents={}, cache_lookup_ms={}, total_ms={}",
                            parents.len(),
                            cache_lookup_started.elapsed().as_millis(),
                            total_started.elapsed().as_millis()
                        );
                        return Ok((state, Vec::new(), Vec::new()));
                    }
                }
            }

            let parent_hashes: Vec<BlockHash> =
                parents.iter().map(|p| p.block_hash.clone()).collect();

            // floor(B): the justification-derived finalized cut this block
            // builds on — a pure function of (parents, latest_messages).
            let ft_threshold = s.on_chain_state.shard_conf.fault_tolerance_threshold;
            let floor_compute_start = std::time::Instant::now();
            let floor = crate::rust::finality::floor::finalized_floor(
                &s.dag,
                &parent_hashes,
                latest_messages,
                ft_threshold,
            )
            .await?;
            metrics::histogram!(
                crate::rust::metrics_constants::BLOCK_PROCESSING_PPS_FLOOR_COMPUTE_TIME_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .record(floor_compute_start.elapsed().as_secs_f64());

            let mut parent_hashes_for_key = parent_hashes.clone();
            parent_hashes_for_key.sort();
            let disable_late_block_filtering = disable_late_block_filtering_override
                .unwrap_or(s.on_chain_state.shard_conf.disable_late_block_filtering);
            let cache_key = super::runtime_manager::ParentsPostStateCacheKey {
                sorted_parent_hashes: parent_hashes_for_key,
                floor_hash: floor.hash.clone(),
                disable_late_block_filtering,
            };
            if let Some((cached_state, cached_rejected, cached_slashes)) =
                runtime_manager.get_cached_parents_post_state(&cache_key)
            {
                tracing::debug!(
                    target: "f1r3fly.casper.compute_parents_post_state.cache",
                    "compute_parents_post_state cache hit: parents={}, rejected_deploys={}, rejected_slashes={}",
                    cache_key.sorted_parent_hashes.len(),
                    cached_rejected.len(),
                    cached_slashes.len()
                );
                tracing::debug!(
                    target: "f1r3fly.casper.compute_parents_post_state.timing",
                    "compute_parents_post_state timing: path=cache_hit, parents={}, cache_lookup_ms={}, total_ms={}",
                    cache_key.sorted_parent_hashes.len(),
                    cache_lookup_started.elapsed().as_millis(),
                    total_started.elapsed().as_millis()
                );
                return Ok((cached_state, cached_rejected, cached_slashes));
            }
            let cache_lookup_ms = cache_lookup_started.elapsed().as_millis();

            // Function to get or compute BlockIndex for each parent block hash
            let block_index_f = |v: &BlockHash| -> Result<BlockIndex, CasperError> {
                build_block_index(runtime_manager, block_store, v)
            };

            // FS(floor): the sealed merge base. Finalized effects live in the
            // base and never re-enter the conflict scope.
            let fs_seal_start = std::time::Instant::now();
            let fs = crate::rust::finality::floor_seal::floor_state_get_or_compute(
                &s.dag,
                block_store,
                runtime_manager,
                &floor.hash,
                ft_threshold,
            )
            .await?;
            metrics::histogram!(
                crate::rust::metrics_constants::BLOCK_PROCESSING_PPS_FS_SEAL_TIME_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .record(fs_seal_start.elapsed().as_secs_f64());
            let base_state = Blake2b256Hash::from_bytes_prost(&fs.state_hash.0);

            // Conflict scope = closure(parents) \ closure(floor). Straddler
            // cones (parents forked below the floor) are absorbed into the
            // scope rather than filtered; the walk is depth-bounded purely as
            // an anti-DoS cap, and a truncated cone is an explicit error, not
            // a silently narrower merge.
            let collect_scope_started = std::time::Instant::now();
            let closure = crate::rust::finality::floor_seal::floor_closure(
                &s.dag,
                runtime_manager,
                &floor.hash,
            )?;
            let depth_bound = floor.block_number - MAX_STRADDLE_DEPTH_BLOCKS;
            let in_scope = |hash: &BlockHash| -> bool {
                !closure.contains(hash)
                    && s.dag
                        .block_number(hash)
                        .is_some_and(|number| number >= depth_bound)
            };
            let mut scope: HashSet<BlockHash> = HashSet::new();
            for parent_hash in &parent_hashes {
                scope.extend(s.dag.ancestors(parent_hash.clone(), |h| in_scope(h))?);
                scope.insert(parent_hash.clone());
            }
            // A parent at or below the floor contributes nothing: its effects
            // are already sealed into the base.
            scope.retain(|h| in_scope(h));

            // Truncation check: every parent edge leaving the scope must land
            // in the floor closure. An edge that was cut by the depth bound
            // means a straddler forked deeper than the cap — reject loudly.
            let mut straddler_blocks: usize = 0;
            for hash in &scope {
                let meta = s.dag.lookup_unsafe(hash)?;
                if meta.block_number <= floor.block_number {
                    straddler_blocks += 1;
                }
                for parent_edge in &meta.parents {
                    if !scope.contains(parent_edge) && !closure.contains(parent_edge) {
                        return Err(CasperError::Other(format!(
                            "straddling parent cone exceeds depth cap: block {} (#{}) reaches {} below floor {} (#{}) - {} blocks",
                            PrettyPrinter::build_string_bytes(hash),
                            meta.block_number,
                            PrettyPrinter::build_string_bytes(parent_edge),
                            PrettyPrinter::build_string_bytes(&floor.hash),
                            floor.block_number,
                            MAX_STRADDLE_DEPTH_BLOCKS,
                        )));
                    }
                }
            }
            let scope_elapsed = collect_scope_started.elapsed();
            let collect_scope_ms = scope_elapsed.as_millis();
            metrics::histogram!(
                crate::rust::metrics_constants::BLOCK_PROCESSING_PPS_SCOPE_BUILD_TIME_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .record(scope_elapsed.as_secs_f64());

            // Diagnostic digest of the block's justification snapshot. Gated so the
            // map/format/sort only runs when a consuming trace is enabled — zero cost
            // at production log levels (f1r3.trace=warn).
            let lm_key: Vec<String> = if tracing::enabled!(target: "f1r3.trace.fs_floor", tracing::Level::DEBUG)
                || tracing::enabled!(target: "f1r3.trace.merge_verdict", tracing::Level::INFO)
            {
                let mut v: Vec<String> = latest_messages
                    .iter()
                    .map(|(val, h)| {
                        format!(
                            "{}:{}",
                            hex::encode(&val[..val.len().min(3)]),
                            hex::encode(&h[..h.len().min(6)])
                        )
                    })
                    .collect();
                v.sort();
                v
            } else {
                Vec::new()
            };
            tracing::debug!(
                target: "f1r3.trace.fs_floor",
                event = "stage2_base",
                floor = %PrettyPrinter::build_string_bytes(&floor.hash),
                floor_number = floor.block_number,
                base_state = %PrettyPrinter::build_string_bytes(&fs.state_hash.0),
                parents = parent_hashes.len(),
                scope = scope.len(),
                straddler_blocks,
                lm_key = ?lm_key,
                "multi-parent merge based on sealed floor state"
            );

            // Every scope block's mergeable entry must be loadable before the
            // merge builds indices; recompute any the node never replayed.
            let ensure_mergeable_start = std::time::Instant::now();
            ensure_scope_mergeable_present(block_store, runtime_manager, &s.dag, &scope).await?;
            metrics::histogram!(
                crate::rust::metrics_constants::BLOCK_PROCESSING_PPS_ENSURE_MERGEABLE_TIME_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .record(ensure_mergeable_start.elapsed().as_secs_f64());

            // Construction merge resolves the cone by keep-one + recovery on the
            // sealed FS(floor) base. The enforcement window is intentionally not
            // applied here: it force-rejected channel-disjoint writes that merely
            // DAG-descend from a finalized-rejected boundary CloseBlock.
            let merge_started = std::time::Instant::now();
            let merger_result = dag_merger::merge(
                &s.dag,
                &floor.hash,
                &base_state,
                |hash: &BlockHash| -> Result<Vec<DeployChainIndex>, CasperError> {
                    let block_index = block_index_f(hash)?;
                    Ok(block_index.deploy_chains)
                },
                &runtime_manager.history_repo,
                dag_merger::cost_optimal_rejection_alg(),
                Some(scope),
                disable_late_block_filtering,
                None,
            )?;
            let merge_elapsed = merge_started.elapsed();
            let merge_ms = merge_elapsed.as_millis();
            metrics::histogram!(
                crate::rust::metrics_constants::BLOCK_PROCESSING_PPS_MERGE_TIME_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .record(merge_elapsed.as_secs_f64());

            // `_applied_user` is consumed only by the seal (FloorData accepted
            // ledger); the proposer's pre-state merge does not need it.
            let (state, _applied_user, rejected_user_pairs, rejected_slash_pairs) = merger_result;

            tracing::debug!(
                target: "f1r3fly.casper.parent_selection",
                new_state = %hex::encode(state.bytes()),
                merge_ms,
                rejected_user_count = rejected_user_pairs.len(),
                rejected_slash_count = rejected_slash_pairs.len(),
                "multi-parent merge result",
            );
            tracing::info!(
                target: "f1r3.trace.merge_verdict",
                floor_number = floor.block_number,
                parents = parent_hashes.len(),
                parent_hashes = ?parent_hashes.iter().map(|h| hex::encode(&h[..h.len().min(6)])).collect::<Vec<_>>(),
                lm_key = ?lm_key,
                new_state = %hex::encode(&state.bytes()[..state.bytes().len().min(6)]),
                rejected_sigs = ?rejected_user_pairs.iter().map(|(s, _)| hex::encode(&s[..s.len().min(8)])).collect::<Vec<_>>(),
                "MERGE_VERDICT",
            );

            // Populate the rejected-deploy buffer from (sig, source_block_hash) pairs.
            // Looking up the `Signed<DeployData>` from the block store lets the block
            // creator re-propose these deploys in a subsequent block. Fetching each
            // source block at most once keeps the cost proportional to the number of
            // distinct rejected-from blocks.
            //
            // Catchup gate: before admitting a deploy to the buffer, check its
            // current finalization status against the local DAG view. Skip any
            // sig whose state is terminal (Finalized / Failed / Expired) — such
            // sigs have been resolved elsewhere, and re-proposing them would at
            // best waste proposal slots and at worst cause a catching-up
            // validator to re-execute already-canonical work against a
            // different pre-state.
            if let Some(buffer) = rejected_deploy_buffer {
                if !rejected_user_pairs.is_empty() {
                    // Pre-compute admit decisions for all rejected sigs in
                    // one batched canonical-chain scan, before the
                    // per-block iteration below. Without batching, the
                    // catchup hot path was O(rejected_count × DAG_size)
                    // — a 50-rejected merge with a 200-block deploy-
                    // lifespan window would do 10 000 block fetches.
                    // After batching: one BFS regardless of N, then
                    // dictionary lookups.
                    let candidate_sigs: HashSet<Bytes> = rejected_user_pairs
                        .iter()
                        .map(|(sig, _)| sig.clone())
                        .collect();
                    let admit_set: HashSet<Bytes> = compute_rejected_buffer_admits(
                        &s.dag,
                        block_store,
                        s.on_chain_state.shard_conf.deploy_lifespan,
                        &candidate_sigs,
                    );

                    let mut by_block: HashMap<BlockHash, Vec<Bytes>> = HashMap::new();
                    for (sig, src_block) in &rejected_user_pairs {
                        by_block
                            .entry(src_block.clone())
                            .or_default()
                            .push(sig.clone());
                    }
                    let mut deploys_to_buffer: Vec<Signed<DeployData>> = Vec::new();
                    for (src_block, sigs) in by_block {
                        let sig_set: HashSet<Bytes> = sigs.into_iter().collect();
                        match block_store.get(&src_block) {
                            Ok(Some(block)) => {
                                for pd in &block.body.deploys {
                                    if sig_set.contains(&pd.deploy.sig)
                                        && admit_set.contains(&pd.deploy.sig)
                                    {
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

            // Seal keep-one losers: the finalized seal ledger's rejected deploys (concurrent
            // single-value-cell writers the seal did NOT fold). Unlike the construction
            // rejections above, these are NOT in any block's body.rejected_deploys, so
            // `resolve_batch` would report them Finalized (clean inclusion) and the deploy-status
            // admit-gate would drop them. Gate instead on the effect-in-FS base-check — the same
            // oracle `repeat_deploy` and the proposer's recovery filter use: admit a loser iff its
            // effect is NOT in FS and it is NOT superseded by an accepted inclusion (accepted-wins).
            // Re-feeding the cumulative ledger each call is absorbed by the gate (an already-
            // recovered loser reads in-FS → skipped), so the loser re-executes against the updated
            // FS exactly once and lands a cut later — monotone, no orphan.
            if let Some(buffer) = rejected_deploy_buffer {
                if !fs.rejected_deploys.is_empty() {
                    let mut by_block: HashMap<BlockHash, Vec<Bytes>> = HashMap::new();
                    for r in &fs.rejected_deploys {
                        let sig = r.sig.clone();
                        // accepted-wins: a loser also accepted at some inclusion is already in FS.
                        if fs.accepted_deploys.iter().any(|a| a.sig == sig) {
                            continue;
                        }
                        // effect-in-FS base-check: skip already-recovered (or otherwise in-FS) losers.
                        if recovered_deploy_effect_in_base(
                            &s.dag,
                            block_store,
                            runtime_manager,
                            &base_state,
                            &sig,
                        )
                        .unwrap_or(false)
                        {
                            continue;
                        }
                        by_block.entry(r.host.0.clone()).or_default().push(sig);
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
                                    "RejectedDeployBuffer (seal losers): source block {} not in store",
                                    PrettyPrinter::build_string_bytes(&src_block)
                                );
                            }
                            Err(err) => {
                                tracing::warn!(
                                    "RejectedDeployBuffer (seal losers): failed to load {}: {}",
                                    PrettyPrinter::build_string_bytes(&src_block),
                                    err
                                );
                            }
                        }
                    }
                    if !deploys_to_buffer.is_empty() {
                        match buffer.lock() {
                            Ok(mut guard) => {
                                if let Err(err) = guard.add(deploys_to_buffer) {
                                    tracing::warn!(
                                        "RejectedDeployBuffer (seal losers) add failed: {}",
                                        err
                                    );
                                }
                            }
                            Err(_) => {
                                tracing::warn!(
                                    "RejectedDeployBuffer (seal losers): lock poisoned; skipping populate"
                                );
                            }
                        }
                    }
                }
            }

            // Recover rejected-slash metadata by reading each source block's
            // system_deploys once. The block creator uses these to dedup
            // slashes into the merge block's body; without this the slash
            // effect would be lost to cost-optimal rejection.
            let rejected_slashes: Vec<crate::rust::merging::rejected_slash::RejectedSlash> =
                if rejected_slash_pairs.is_empty() {
                    Vec::new()
                } else {
                    let mut by_block: HashMap<BlockHash, Vec<Bytes>> = HashMap::new();
                    for (sig, src_block) in &rejected_slash_pairs {
                        by_block
                            .entry(src_block.clone())
                            .or_default()
                            .push(sig.clone());
                    }
                    let mut out = Vec::new();
                    for (src_block, _sigs) in by_block {
                        match block_store.get(&src_block) {
                            Ok(Some(block)) => {
                                for psd in &block.body.system_deploys {
                                    if let models::rust::casper::protocol::casper_message::ProcessedSystemDeploy::Succeeded {
                                    system_deploy:
                                        models::rust::casper::protocol::casper_message::SystemDeployData::Slash {
                                            invalid_block_hash,
                                            issuer_public_key,
                                        },
                                    ..
                                } = psd
                                {
                                    out.push(
                                        crate::rust::merging::rejected_slash::RejectedSlash {
                                            invalid_block_hash: invalid_block_hash.clone(),
                                            issuer_public_key: issuer_public_key.clone(),
                                            source_block_hash: src_block.clone(),
                                        },
                                    );
                                }
                                }
                            }
                            Ok(None) => {
                                tracing::warn!(
                                    "RejectedSlash extract: source block {} not in store",
                                    PrettyPrinter::build_string_bytes(&src_block)
                                );
                            }
                            Err(err) => {
                                tracing::warn!(
                                    "RejectedSlash extract: failed to load {}: {}",
                                    PrettyPrinter::build_string_bytes(&src_block),
                                    err
                                );
                            }
                        }
                    }
                    out
                };

            // Keep (sig, host): the body's rejected_deploys carry the host so the
            // per-inclusion content-twin guard can tell which inclusion was rejected.
            let rejected: Vec<(Bytes, BlockHash)> = rejected_user_pairs;

            let computed_state = prost::bytes::Bytes::copy_from_slice(&state.bytes());
            runtime_manager.put_cached_parents_post_state(
                cache_key,
                (
                    computed_state.clone(),
                    rejected.clone(),
                    rejected_slashes.clone(),
                ),
            );
            tracing::debug!(
                target: "f1r3fly.casper.compute_parents_post_state.timing",
                "compute_parents_post_state timing: path=merged, parents={}, cache_lookup_ms={}, collect_scope_ms={}, merge_ms={}, rejected_deploys={}, rejected_slashes={}, total_ms={}",
                parents.len(),
                cache_lookup_ms,
                collect_scope_ms,
                merge_ms,
                rejected.len(),
                rejected_slashes.len(),
                total_started.elapsed().as_millis()
            );
            Ok((computed_state, rejected, rejected_slashes))
        }
    }
}
