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
    BlockMessage, Bond, DeployData, ProcessedDeploy, ProcessedSystemDeploy, SystemDeployData,
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
                    Some(sig)
                } else {
                    tracing::debug!(
                        "RejectedDeployBuffer populate: skipping sig {} (state={:?}) — already resolved in canonical view",
                        hex::encode(&sig),
                        status.state
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
            let rejected_deploy_ids: HashSet<_> = rejected_deploys.iter().cloned().collect();
            let block_rejected_deploy_sigs: HashSet<_> = block
                .body
                .rejected_deploys
                .iter()
                .map(|d| d.sig.clone())
                .collect();

            if incoming_pre_state_hash != computed_pre_state_hash {
                tracing::debug!(target: "f1r3fly.casper.block_validation", block = %hex::encode(&block.block_hash[..8.min(block.block_hash.len())]), computed = %hex::encode(&computed_pre_state_hash[..8.min(computed_pre_state_hash.len())]), incoming = %hex::encode(&incoming_pre_state_hash[..8.min(incoming_pre_state_hash.len())]), "validate.block_checkpoint: PRE-STATE MISMATCH (recomputed merge != block's recorded pre-state) -> reject, NO replay");
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
    rejected_deploy_buffer: Option<&std::sync::Arc<std::sync::Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>>,
) -> Result<
    (
        StateHash,
        StateHash,
        Vec<ProcessedDeploy>,
        Vec<prost::bytes::Bytes>,
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
    let computed_parents_info = compute_parents_post_state(
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

/// Compute the merged post-state from multiple parent blocks.
///
/// For exploratory deploy, pass `disable_late_block_filtering_override = Some(true)` to
/// always disable late block filtering (see full merged state).
/// For normal block creation, pass `None` to use the shard config value.
pub async fn compute_parents_post_state(
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
) -> Result<
    (
        StateHash,
        Vec<Bytes>,
        Vec<crate::rust::merging::rejected_slash::RejectedSlash>,
    ),
    CasperError,
> {
    let total_started = std::time::Instant::now();
    const MAX_PARENT_MERGE_SCOPE_BLOCKS: usize = 512;
    const MAX_LCA_DISTANCE_BLOCKS: i64 = 256;
    const MAX_FULL_ANCESTOR_SCAN_NODES: usize = 8_192;

    // No entered span guard here: the function is async (it awaits floor
    // derivation), and an `.entered()` guard is not `Send` across an await.
    // The individual tracing events below carry their own targets.
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

            let mut parent_hashes_for_key: Vec<BlockHash> =
                parents.iter().map(|p| p.block_hash.clone()).collect();
            parent_hashes_for_key.sort();
            let disable_late_block_filtering = disable_late_block_filtering_override
                .unwrap_or(s.on_chain_state.shard_conf.disable_late_block_filtering);
            let cache_key = super::runtime_manager::ParentsPostStateCacheKey {
                sorted_parent_hashes: parent_hashes_for_key,
                snapshot_lfb_hash: s.last_finalized_block.clone(),
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
                // Try cache first
                if let Some(cached) = runtime_manager.block_index_cache.get(v) {
                    return Ok((*cached.value()).clone());
                }

                // Cache miss - compute the BlockIndex
                let b = block_store.get_unsafe(v);
                let pre_state = &b.body.state.pre_state_hash;
                let post_state = &b.body.state.post_state_hash;
                let sender = b.sender.clone();
                let seq_num = b.seq_num;

                let mergeable_chs =
                    runtime_manager.load_mergeable_channels(post_state, sender, seq_num)?;

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

                // Cache the result
                runtime_manager
                    .block_index_cache
                    .insert(v.clone(), block_index.clone());

                Ok(block_index)
            };

            // Compute scope: all ancestors of parents (blocks visible from these parents)
            // bounded by max-parent-depth configured for the shard to avoid
            // expensive ancestry walks through finalized history.
            let parent_hashes: Vec<BlockHash> =
                parents.iter().map(|p| p.block_hash.clone()).collect();
            let max_parent_block_number = parents
                .iter()
                .map(|p| p.body.state.block_number)
                .max()
                .unwrap_or(0);
            let max_parent_depth = s.on_chain_state.shard_conf.max_parent_depth;
            let ancestor_min_block_number = if max_parent_depth <= 0 || max_parent_depth == i32::MAX
            {
                i64::MIN
            } else {
                max_parent_block_number.saturating_sub(max_parent_depth as i64)
            };
            let include_visible_ancestor =
                |hash: &BlockHash, dag: &KeyValueDagRepresentation| -> bool {
                    // IMPORTANT: do not use local finalized status as a merge-scope filter.
                    // Different validators can have temporarily different finalized views, and
                    // filtering by `is_finalized` causes non-deterministic parent post-state
                    // computation for the same parent set.
                    if ancestor_min_block_number == i64::MIN {
                        return true;
                    }

                    match dag.lookup(hash) {
                        Ok(Some(meta)) => meta.block_number >= ancestor_min_block_number,
                        Ok(None) => false,
                        Err(_) => false,
                    }
                };
            // Get all ancestors of all parents (including the parents themselves)
            // Use bounded traversal that stops at finalized blocks to prevent O(chain_length) growth
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

            // Node-deterministic finalized floor, derived from the block's frozen
            // justification snapshot — the cut the merge builds on. Replaces the
            // LCA base: the floor is finalization-aware and advance-only, so the
            // merged state is MONOTONE, where the LCA base let it churn
            // (path-dependent FS). The `lfb_*` names are kept so the downstream
            // scope filter, fallback, and merge call are unchanged (now
            // floor-anchored rather than LCA-anchored).
            let lca_started = std::time::Instant::now();
            let floor = crate::rust::finality::floor::finalized_floor(
                &s.dag,
                &parent_hashes,
                latest_messages,
                s.on_chain_state.shard_conf.fault_tolerance_threshold,
            )
            .await?;
            let lca_ms = lca_started.elapsed().as_millis();
            let lfb_for_descendants = floor.hash.clone();

            // The floor block's committed post-state is FS(floor) — the merge base.
            let lfb_block = block_store.get_unsafe(&lfb_for_descendants);
            let lfb_state = Blake2b256Hash::from_bytes_prost(&lfb_block.body.state.post_state_hash);

            // Scope visible_blocks to blocks at or above the floor: the
            // unfinalized band the merge resolves. Blocks at or below the floor
            // are already sealed into the floor's post-state, so merging them is
            // redundant. Deterministic — the floor and block numbers are pure
            // DAG/justification facts.
            let floor_block_number = lfb_block.body.state.block_number;
            let pre_filter_count = visible_blocks.len();
            visible_blocks.retain(|bh| match s.dag.lookup_unsafe(bh) {
                Ok(meta) => meta.block_number >= floor_block_number,
                Err(_) => true, // keep on lookup error (conservative)
            });
            tracing::debug!(target: "f1r3fly.casper.compute_parents_post_state", floor = %hex::encode(&lfb_for_descendants[..8.min(lfb_for_descendants.len())]), floor_block = floor_block_number, base_state = %hex::encode(&lfb_block.body.state.post_state_hash[..8.min(lfb_block.body.state.post_state_hash.len())]), scope_blocks = visible_blocks.len(), n_parents = parents.len(), "merge.compute_parents_post_state: floor+base+scope computed");
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
                    hex::encode(
                        &lfb_for_descendants[..std::cmp::min(10, lfb_for_descendants.len())]
                    ),
                    floor_block_number,
                    visible_blocks.len()
                );
            }

            let max_parent_block_number = parents
                .iter()
                .map(|p| p.body.state.block_number)
                .max()
                .unwrap_or(lfb_block.body.state.block_number);
            let lca_distance = max_parent_block_number - lfb_block.body.state.block_number;
            let visible_blocks_len = visible_blocks.len();
            if visible_blocks.len() > MAX_PARENT_MERGE_SCOPE_BLOCKS
                || lca_distance > MAX_LCA_DISTANCE_BLOCKS
            {
                let fallback_parent = parents
                    .iter()
                    .max_by(|a, b| {
                        a.body
                            .state
                            .block_number
                            .cmp(&b.body.state.block_number)
                            .then_with(|| a.block_hash.cmp(&b.block_hash))
                    })
                    .expect("parents is non-empty in multi-parent branch");
                tracing::warn!(
                    target: "f1r3fly.casper.compute_parents_post_state.fallback",
                    "compute_parents_post_state fallback: visibleBlocks={}, lca_distance={}, chosen_parent={} (block {}), reason=merge_scope_too_large",
                    visible_blocks.len(),
                    lca_distance,
                    PrettyPrinter::build_string_bytes(&fallback_parent.block_hash),
                    fallback_parent.body.state.block_number
                );
                metrics::counter!(
                    crate::rust::metrics_constants::MERGE_SCOPE_TOO_LARGE_FALLBACK_FIRED_METRIC,
                    "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
                )
                .increment(1);
                let fallback_state = proto_util::post_state_hash(fallback_parent);
                runtime_manager.put_cached_parents_post_state(
                    cache_key,
                    (fallback_state.clone(), Vec::new(), Vec::new()),
                );
                tracing::debug!(
                    target: "f1r3fly.casper.compute_parents_post_state.timing",
                    "compute_parents_post_state timing: path=fallback_latest_parent, parents={}, cache_lookup_ms={}, collect_ancestors_ms={}, flatten_visible_ms={}, lca_ms={}, visible_blocks={}, lca_distance={}, total_ms={}",
                    parents.len(),
                    cache_lookup_ms,
                    collect_ancestors_ms,
                    flatten_visible_ms,
                    lca_ms,
                    visible_blocks_len,
                    lca_distance,
                    total_started.elapsed().as_millis()
                );
                return Ok((fallback_state, Vec::new(), Vec::new()));
            }

            // Use DagMerger to merge parent states with scope
            let merge_started = std::time::Instant::now();
            let merger_result = dag_merger::merge(
                &s.dag,
                &lfb_for_descendants,
                &lfb_state,
                |hash: &BlockHash| -> Result<Vec<DeployChainIndex>, CasperError> {
                    let block_index = block_index_f(hash)?;
                    Ok(block_index.deploy_chains)
                },
                &runtime_manager.history_repo,
                dag_merger::cost_optimal_rejection_alg(),
                Some(visible_blocks),
                disable_late_block_filtering,
            )?;
            let merge_ms = merge_started.elapsed().as_millis();

            let (state, rejected_user_pairs, rejected_slash_pairs) = merger_result;
            tracing::debug!(target: "f1r3fly.casper.compute_parents_post_state", merged_state = %hex::encode(&state.bytes()[..8.min(state.bytes().len())]), n_rejected_user = rejected_user_pairs.len(), n_rejected_slash = rejected_slash_pairs.len(), merge_ms, "merge.compute_parents_post_state: DagMerger produced merged state");

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

            // Strip block hashes; the cache and callers only need the deploy sigs.
            let rejected: Vec<Bytes> = rejected_user_pairs
                .into_iter()
                .map(|(sig, _)| sig)
                .collect();

            let computed_state = prost::bytes::Bytes::copy_from_slice(&state.bytes());
            // The floor is a deterministic function of the block's justifications,
            // so the merged state is always cacheable (no snapshot-LFB fallback).
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
                "compute_parents_post_state timing: path=merged, parents={}, cache_lookup_ms={}, collect_ancestors_ms={}, flatten_visible_ms={}, lca_ms={}, merge_ms={}, visible_blocks={}, rejected_deploys={}, rejected_slashes={}, total_ms={}",
                parents.len(),
                cache_lookup_ms,
                collect_ancestors_ms,
                flatten_visible_ms,
                lca_ms,
                merge_ms,
                visible_blocks_len,
                rejected.len(),
                rejected_slashes.len(),
                total_started.elapsed().as_millis()
            );
            Ok((computed_state, rejected, rejected_slashes))
        }
    }
}
