//! Snapshot construction — `compute_snapshot`, `get_on_chain_state`,
//! `record_dag_cardinality_metrics`, `estimator`.
//!
//! Phase 3 Step 3 — extracted from `engine::multi_parent_casper`. Each
//! function takes the casper instance as a `&MultiParentCasperImpl<T>`
//! reference (rather than `&self`) so the implementation can live in this
//! module while the trait method is a one-line delegate in `traits.rs`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::transport::transport_layer::TransportLayer;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData, Justification};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use shared::rust::dag::dag_ops;

use super::types::MultiParentCasperImpl;
use crate::rust::casper::{CasperSnapshot, OnChainCasperState};
use crate::rust::errors::CasperError;
use crate::rust::estimator::retained_parent_indices;
use crate::rust::finality::floor::canonical_floor_committee;
use crate::rust::metrics_constants::{
    CASPER_METRICS_SOURCE, DAG_BLOCKS_SIZE_METRIC, DAG_CHILDREN_INDEX_SIZE_METRIC,
    DAG_FINALIZED_BLOCKS_SIZE_METRIC, DAG_HEIGHTS_SIZE_METRIC,
    DEPLOYS_IN_SCOPE_SIG_BYTES_ESTIMATE_METRIC, DEPLOYS_IN_SCOPE_SIZE_METRIC,
    PARENT_FRONTIER_CAPACITY_DEFERRED_TOTAL_METRIC,
};
use crate::rust::util::proto_util;
use crate::rust::util::rholang::interpreter_util;

/// C15 / Smell-1: byte-size estimate for a secp256k1 compact-encoded
/// deploy signature. ~64 bytes signature + 1 byte prefix. Used to
/// drive the `DEPLOYS_IN_SCOPE_SIG_BYTES_ESTIMATE_METRIC` gauge — the
/// gauge is operator-facing memory-pressure telemetry, NOT a
/// consensus-critical value, so a rounded estimate (rather than a
/// per-deploy actual-byte sum) is intentional.
const DEPLOY_SIG_BYTES_ESTIMATE: f64 = 65.0;

fn order_parents_by_ghost_head(
    mut parents: Vec<BlockMessage>,
    ghost_main_parent: &BlockHash,
) -> Result<Vec<BlockMessage>, CasperError> {
    if !parents
        .iter()
        .any(|parent| parent.block_hash == ghost_main_parent)
    {
        return Err(CasperError::RuntimeError(format!(
            "LMD-GHOST head {} is absent from the candidate parent set",
            hex::encode(ghost_main_parent)
        )));
    }
    parents.sort_by(|a, b| {
        let a_main = a.block_hash == ghost_main_parent;
        let b_main = b.block_hash == ghost_main_parent;
        b_main
            .cmp(&a_main)
            .then_with(|| a.block_hash.cmp(&b.block_hash))
    });
    Ok(parents)
}

fn prune_dag_covered_parents(
    dag: &KeyValueDagRepresentation,
    parents: Vec<BlockMessage>,
) -> Result<Vec<BlockMessage>, CasperError> {
    if parents.len() <= 1 {
        return Ok(parents);
    }

    let retained_indices = reachability_maximal_indices(parents.len(), |candidate, other| {
        dag.is_dag_ancestor(&parents[candidate].block_hash, &parents[other].block_hash)
            .map_err(CasperError::from)
    })?;
    let retained = retained_indices
        .into_iter()
        .map(|index| parents[index].clone())
        .collect::<Vec<_>>();
    tracing::debug!(
        target: "f1r3fly.casper.parent_selection",
        original_parents = parents.len(),
        retained_parents = retained.len(),
        "Parent selection retained the reachability-maximal antichain"
    );
    Ok(retained)
}

fn reachability_maximal_indices<E>(
    len: usize,
    mut is_ancestor: impl FnMut(usize, usize) -> Result<bool, E>,
) -> Result<Vec<usize>, E> {
    let mut retained = Vec::with_capacity(len);
    for candidate in 0..len {
        let mut covered = false;
        for other in 0..len {
            if candidate != other && is_ancestor(candidate, other)? {
                covered = true;
                break;
            }
        }
        if !covered {
            retained.push(candidate);
        }
    }
    Ok(retained)
}

fn block_covers_causal_tips(
    dag: &KeyValueDagRepresentation,
    block_hash: &BlockHash,
    causal_tips: &HashSet<BlockHash>,
) -> Result<bool, CasperError> {
    for tip in causal_tips {
        if !dag.is_dag_ancestor(tip, block_hash)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn recovery_main_covers_floor_and_causal_tips(
    dag: &KeyValueDagRepresentation,
    finalized_floor: &BlockHash,
    main_parent: &BlockHash,
    causal_tips: &HashSet<BlockHash>,
) -> Result<bool, CasperError> {
    Ok(dag.is_dag_ancestor(finalized_floor, main_parent)?
        && block_covers_causal_tips(dag, main_parent, causal_tips)?)
}

fn validate_causal_parent_coverage(
    dag: &KeyValueDagRepresentation,
    finalized_floor: &BlockHash,
    causal_tips: &HashSet<BlockHash>,
    parents: &[BlockMessage],
) -> Result<(), CasperError> {
    let mut descends_from_floor = false;
    for parent in parents {
        if dag.is_dag_ancestor(finalized_floor, &parent.block_hash)? {
            descends_from_floor = true;
            break;
        }
    }
    if !descends_from_floor {
        return Err(CasperError::RuntimeError(format!(
            "parent selection does not descend from captured finalized floor {}",
            hex::encode(finalized_floor)
        )));
    }
    for tip in causal_tips {
        let mut covered = false;
        for parent in parents {
            if dag.is_dag_ancestor(tip, &parent.block_hash)? {
                covered = true;
                break;
            }
        }
        if !covered {
            return Err(CasperError::RuntimeError(format!(
                "configured parent bounds omit uncovered causal tip {}",
                hex::encode(tip)
            )));
        }
    }
    Ok(())
}

fn causal_tips_covered_by_parents(
    dag: &KeyValueDagRepresentation,
    causal_tips: &HashSet<BlockHash>,
    parents: &[BlockMessage],
) -> Result<HashSet<BlockHash>, CasperError> {
    let mut covered = HashSet::with_capacity(causal_tips.len());
    for tip in causal_tips {
        for parent in parents {
            if dag.is_dag_ancestor(tip, &parent.block_hash)? {
                covered.insert(tip.clone());
                break;
            }
        }
    }
    Ok(covered)
}

fn validate_exact_parent_frontier_capacity(
    max_number_of_parents: i32,
    required_parents: usize,
    effective_committee: usize,
    unique_causal_tips: usize,
    floor_backstop_added: bool,
    expired_tip_count: usize,
) -> Result<(), CasperError> {
    if max_number_of_parents == crate::rust::casper::UNLIMITED_PARENTS {
        return Ok(());
    }
    let configured_cap = usize::try_from(max_number_of_parents).map_err(|_| {
        CasperError::RuntimeError(format!(
            "invalid max-number-of-parents at snapshot construction: {max_number_of_parents}"
        ))
    })?;
    if required_parents <= configured_cap {
        return Ok(());
    }
    metrics::counter!(
        PARENT_FRONTIER_CAPACITY_DEFERRED_TOTAL_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .increment(1);
    tracing::warn!(
        target: "f1r3fly.casper.parent_selection",
        configured_cap,
        required_parents,
        effective_committee,
        unique_causal_tips,
        floor_backstop_added,
        expired_tip_count,
        "Proposal deferred because the exact frozen parent frontier exceeds the configured cap"
    );
    Err(CasperError::ParentFrontierCapacityExceeded {
        configured_cap,
        required_parents,
        effective_committee,
        unique_causal_tips,
        floor_backstop_added,
        expired_tip_count,
    })
}

/// Snapshot-time approximation of buffer recoverability, used only to decide
/// whether `compute_snapshot` enters a recovery context (which narrows parent
/// selection). Runs BEFORE the snapshot's `deploys_in_scope` /
/// `rejected_in_scope` sets exist, so it can only filter by the block-number
/// window and wall-clock expiry — it is deliberately coarser than its
/// admission-time twin, `block_creator::rejected_buffer_has_recoverable_deploys`,
/// which refines the same tail (canonical-won exclusion) with the completed
/// snapshot's scope sets. Disagreements are benign: true-here/false-there
/// costs one narrowed-parent propose that then declines recovery;
/// false-here/true-there skips the narrowing heuristic while recovery still
/// admits at block creation. Do NOT "harmonize" the filters — this one cannot
/// use fields that do not exist yet.
fn local_rejected_buffer_has_recoverable_deploys(
    block_store: &KeyValueBlockStore,
    rejected_deploy_buffer: &Arc<Mutex<KeyValueRejectedDeployBuffer>>,
    finalized_floor: &BlockHash,
    parent_hashes: &[BlockHash],
    current_block_number: i64,
    current_time_millis: i64,
    deploy_lifespan: i64,
    protocol_version: i64,
) -> Result<bool, CasperError> {
    let buffered_deploys: HashSet<Signed<DeployData>> = {
        let buffer_guard = rejected_deploy_buffer
            .lock()
            .map_err(|err| CasperError::LockError(err.to_string()))?;
        if !buffer_guard.non_empty().map_err(CasperError::from)? {
            return Ok(false);
        }
        buffer_guard.read_all().map_err(CasperError::from)?
    };
    if buffered_deploys.is_empty() {
        return Ok(false);
    }

    let earliest_block_number = current_block_number - deploy_lifespan;
    let candidates: Vec<_> = buffered_deploys
        .iter()
        .filter(|deploy| {
            deploy.data.valid_after_block_number < current_block_number
                && deploy.data.valid_after_block_number > earliest_block_number
                && !deploy.data.is_expired_at(current_time_millis)
        })
        .collect();
    if candidates.is_empty() {
        return Ok(false);
    }
    let scan_floor = candidates
        .iter()
        .map(|deploy| deploy.data.valid_after_block_number)
        .min()
        .map(|height| height.min(earliest_block_number))
        .unwrap_or(earliest_block_number);
    let canonical_won = interpreter_util::canonical_won_sigs_at_floor(
        block_store,
        finalized_floor,
        parent_hashes,
        scan_floor,
        protocol_version,
    )?;

    Ok(candidates
        .iter()
        .any(|deploy| !canonical_won.contains(&deploy.sig)))
}

fn deploy_scope_cache_key_matches(
    cached_generation: u64,
    cached_lfb: &BlockHash,
    cached_parents: &[BlockHash],
    current_generation: u64,
    current_lfb: &BlockHash,
    current_parents: &[BlockHash],
) -> bool {
    cached_generation == current_generation
        && cached_lfb == current_lfb
        && cached_parents == current_parents
}

fn fallback_to_finalized_parent(
    block_store: &KeyValueBlockStore,
    parents: Vec<BlockMessage>,
    last_finalized_block: &BlockHash,
) -> Result<Vec<BlockMessage>, CasperError> {
    if !parents.is_empty() {
        return Ok(parents);
    }
    let finalized = block_store.get(last_finalized_block)?.ok_or_else(|| {
        shared::rust::store::key_value_store::KvStoreError::KeyNotFound(format!(
            "last finalized block missing from block store: {}",
            hex::encode(last_finalized_block)
        ))
    })?;
    Ok(vec![finalized])
}

pub(crate) fn record_dag_cardinality_metrics(dag: &KeyValueDagRepresentation) {
    metrics::gauge!(DAG_BLOCKS_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
        .set(dag.dag_set.len() as f64);
    metrics::gauge!(DAG_CHILDREN_INDEX_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
        .set(dag.child_map.len() as f64);
    metrics::gauge!(DAG_HEIGHTS_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
        .set(dag.height_map.len() as f64);
    metrics::gauge!(DAG_FINALIZED_BLOCKS_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
        .set(dag.finalized_blocks_set.len() as f64);
}

fn latest_sequence_numbers(
    latest_metadata: &HashMap<Validator, models::rust::block_metadata::BlockMetadata>,
) -> Result<HashMap<Validator, u64>, CasperError> {
    latest_metadata
        .iter()
        .map(|(validator, metadata)| {
            let sequence_number = u64::try_from(metadata.sequence_number).map_err(|_| {
                CasperError::RuntimeError(format!(
                    "latest-message sequence number is negative: {}",
                    metadata.sequence_number
                ))
            })?;
            Ok((validator.clone(), sequence_number))
        })
        .collect()
}

pub(crate) async fn compute_snapshot<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
) -> Result<CasperSnapshot, CasperError> {
    if this
        .finalization_in_progress
        .load(std::sync::atomic::Ordering::SeqCst)
        > 0
    {
        tracing::debug!(
            "Finalization in progress while creating snapshot; using best-effort snapshot"
        );
    }

    let finalization_base = this.block_dag_storage.capture_finalization_base()?;
    let snapshot_finalization_head = finalization_base.head;
    let dag = finalization_base.dag;

    // Parent selection: Use latest block from EACH bonded validator.
    // Phase 12 (PERF-5): `latest_message_hashes()` returns an owned
    // `imbl::HashMap` already (refcount-bump clone). Use `into_iter` to
    // collect by ownership rather than re-cloning every key/value.
    let latest_msgs_hashes: HashMap<Validator, BlockHash> =
        dag.latest_message_hashes().into_iter().collect();
    let validator_capacity = latest_msgs_hashes.len();
    let snapshot_lfb_hash = dag.last_finalized_block();
    let snapshot_lfb = this.block_store.get(&snapshot_lfb_hash)?.ok_or_else(|| {
        shared::rust::store::key_value_store::KvStoreError::KeyNotFound(format!(
            "last finalized block missing from block store: {}",
            hex::encode(&snapshot_lfb_hash)
        ))
    })?;
    let finalized_floor_bonds = canonical_floor_committee(
        snapshot_lfb.body.state.bonds.clone(),
        &snapshot_lfb.body.state.active_validators,
    )?;
    let mut latest_metas: HashMap<Validator, models::rust::block_metadata::BlockMetadata> =
        HashMap::with_capacity(validator_capacity);
    for (validator, hash) in latest_msgs_hashes.iter() {
        let metadata = dag.lookup_unsafe(hash)?;
        latest_metas.insert(validator.clone(), metadata);
    }
    let consensus_context =
        crate::rust::causal_equivocation::CertifiedConsensusContext::for_finalized_floor(
            &dag,
            snapshot_lfb_hash.clone(),
        )?;
    let finalized_floor_certificate = match this
        .block_dag_storage
        .finalized_floor_certificate_for_head(&snapshot_finalization_head)?
    {
        Some(certificate) => certificate,
        None if snapshot_lfb_hash == this.approved_block.block_hash
            && snapshot_lfb.body.state.block_number == 0 =>
        {
            crate::rust::finality::certificate::genesis_finalization_certificate(
                &dag,
                &snapshot_lfb,
                this.casper_shard_conf.casper_version,
                this.casper_shard_conf.shard_name.clone(),
                this.casper_shard_conf.fault_tolerance_threshold_ppm,
                1_000_000,
            )?
        }
        None => {
            return Err(CasperError::RuntimeError(
                "non-genesis finalized floor has no durable finalization certificate".to_string(),
            ));
        }
    };
    if finalized_floor_certificate.protocol_version != this.casper_shard_conf.casper_version
        || finalized_floor_certificate.shard_id != this.casper_shard_conf.shard_name
        || finalized_floor_certificate.target_floor_hash.0 != snapshot_lfb_hash
        || finalized_floor_certificate.target_block_number
            != snapshot_finalization_head.block_number
        || finalized_floor_certificate.target_post_state_hash.0
            != snapshot_lfb.body.state.post_state_hash
    {
        return Err(CasperError::RuntimeError(
            "durable finalization certificate does not bind the snapshot finalized floor"
                .to_string(),
        ));
    }
    let causal_parent_latest_msgs = consensus_context
        .causal_parent_projection()
        .eligible_latest_messages()
        .iter()
        .map(|(validator, hash)| (validator.clone(), hash.clone()))
        .collect::<HashMap<_, _>>();
    let mut unique_parent_hashes: HashSet<BlockHash> =
        HashSet::with_capacity(causal_parent_latest_msgs.len());
    for hash in causal_parent_latest_msgs.values() {
        unique_parent_hashes.insert(hash.clone());
    }
    let causal_tips = unique_parent_hashes.clone();
    let mut parent_blocks_list: Vec<BlockMessage> = Vec::with_capacity(unique_parent_hashes.len());
    for hash in unique_parent_hashes.iter() {
        // Missing parent block here is a real consensus invariant
        // violation (validator pointed at by latest_messages_map but
        // not in block_store) — surface as KvStoreError::KeyNotFound
        // rather than silently dropping the parent.
        let block = this.block_store.get(hash)?.ok_or_else(|| {
            shared::rust::store::key_value_store::KvStoreError::KeyNotFound(format!(
                "parent block referenced by latest_messages missing from block_store: {}",
                hex::encode(hash)
            ))
        })?;
        parent_blocks_list.push(block);
    }
    let mut parent_descends_from_floor = false;
    for parent in &parent_blocks_list {
        if dag.is_dag_ancestor(&snapshot_lfb_hash, &parent.block_hash)? {
            parent_descends_from_floor = true;
            break;
        }
    }
    let floor_backstop_added = !parent_descends_from_floor
        && !parent_blocks_list
            .iter()
            .any(|parent| parent.block_hash == snapshot_lfb_hash);
    if floor_backstop_added {
        parent_blocks_list.push(snapshot_lfb.clone());
    }

    // Sealed-floor: LMD-GHOST main-parent selection. Compute the ghost
    // main-parent from the fork-choice tips over this block's justification
    // snapshot, then order parents so the ghost main-parent sorts first
    // (then by block hash for determinism).
    //
    // The bonded-set ("most-slashed" intersection) parent filter that
    // previously occupied the else-arm below was removed. Sender authority,
    // exact justification membership, and synchrony weights are derived from
    // the finalized floor; the block's bonds field remains its replayed
    // post-state cache. A proposer-side parent filter cannot be a
    // consensus-safety mechanism because validators replay declared parents.
    let initial_fork_choice = this
        .estimator
        .tips_with_context(&dag, &this.approved_block, &consensus_context)
        .await?;
    let ghost_main_parent = if consensus_context
        .vote_projection()
        .eligible_latest_messages()
        .is_empty()
    {
        snapshot_lfb_hash.clone()
    } else {
        let selected = initial_fork_choice.tips.into_iter().next().ok_or_else(|| {
            CasperError::RuntimeError(
                "LMD-GHOST returned no head for a non-empty finality-vote projection".to_string(),
            )
        })?;
        if !unique_parent_hashes.contains(&selected) {
            return Err(CasperError::RuntimeError(format!(
                "LMD-GHOST head {} is absent from the causal-parent projection",
                hex::encode(&selected)
            )));
        }
        selected
    };
    let sorted_parents_list = order_parents_by_ghost_head(parent_blocks_list, &ghost_main_parent)?;
    let sorted_parents_list = prune_dag_covered_parents(&dag, sorted_parents_list)?;
    if sorted_parents_list
        .first()
        .is_none_or(|parent| parent.block_hash != ghost_main_parent)
    {
        return Err(CasperError::RuntimeError(format!(
            "reachability compaction did not preserve LMD-GHOST head {}",
            hex::encode(&ghost_main_parent)
        )));
    }
    validate_causal_parent_coverage(&dag, &snapshot_lfb_hash, &causal_tips, &sorted_parents_list)?;

    let recovery_backlog = if sorted_parents_list.is_empty() {
        false
    } else {
        let sorted_parent_hashes: Vec<BlockHash> = sorted_parents_list
            .iter()
            .map(|block| block.block_hash.clone())
            .collect();
        let candidate_block_number = sorted_parents_list
            .iter()
            .map(|block| block.body.state.block_number)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| {
                CasperError::RuntimeError(
                    "candidate max_block_num overflow while checking recovery context".to_string(),
                )
            })?;
        let now_u128 = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(CasperError::from)?
            .as_millis();
        let now_millis = i64::try_from(now_u128).map_err(|_| {
            CasperError::RuntimeError(format!(
                "Current timestamp millis {} exceeds i64::MAX",
                now_u128
            ))
        })?;
        local_rejected_buffer_has_recoverable_deploys(
            &this.block_store,
            &this.rejected_deploy_buffer,
            &snapshot_lfb_hash,
            &sorted_parent_hashes,
            candidate_block_number,
            now_millis,
            this.casper_shard_conf.deploy_lifespan,
            this.casper_shard_conf.casper_version,
        )?
    };

    let recovery_main_covers_all = if recovery_backlog && sorted_parents_list.len() > 1 {
        recovery_main_covers_floor_and_causal_tips(
            &dag,
            &snapshot_lfb_hash,
            &sorted_parents_list[0].block_hash,
            &causal_tips,
        )?
    } else {
        false
    };
    let unfiltered_parents = if recovery_main_covers_all {
        tracing::info!(
            target: "f1r3fly.casper.recovery",
            "Parent selection narrowed for selectable deploy recovery: original_parents={}, selected_main={}",
            sorted_parents_list.len(),
            PrettyPrinter::build_string_bytes(&sorted_parents_list[0].block_hash)
        );
        vec![sorted_parents_list[0].clone()]
    } else {
        fallback_to_finalized_parent(&this.block_store, sorted_parents_list, &snapshot_lfb_hash)?
    };

    let unfiltered_parents_count = unfiltered_parents.len();
    // C15 / Smell-3: shared wire-convention constant — see
    // `crate::rust::casper::UNLIMITED_PARENTS`.
    let parents =
        if this.casper_shard_conf.max_parent_depth != i32::MAX && unfiltered_parents.len() > 1 {
            // C13 / Perf-2: collapse the build-then-max-then-filter triple
            // pass into a single forward iteration that maintains
            // `max_block_num` incrementally, followed by an in-place
            // `retain` on the vector. Eliminates one intermediate Vec
            // allocation per snapshot and a redundant `.iter()` walk for
            // the max computation.
            let mut parents_with_meta: Vec<(
                BlockMessage,
                models::rust::block_metadata::BlockMetadata,
            )> = Vec::with_capacity(unfiltered_parents.len());
            for b in unfiltered_parents {
                let meta = dag.lookup_unsafe(&b.block_hash)?;
                parents_with_meta.push((b, meta));
            }

            let block_numbers = parents_with_meta
                .iter()
                .map(|(_, metadata)| metadata.block_number)
                .collect::<Vec<_>>();
            retained_parent_indices(
                &block_numbers,
                i64::from(this.casper_shard_conf.max_parent_depth),
            )?
            .into_iter()
            .map(|index| parents_with_meta[index].0.clone())
            .collect()
        } else {
            unfiltered_parents
        };

    let live_causal_tips = causal_tips_covered_by_parents(&dag, &causal_tips, &parents)?;
    let expired_causal_tips = causal_tips.len().saturating_sub(live_causal_tips.len());
    if expired_causal_tips > 0 {
        tracing::info!(
            target: "f1r3fly.casper.parent_selection",
            expired_causal_tips,
            max_parent_depth = this.casper_shard_conf.max_parent_depth,
            "Parent-depth expiry removed stale unfinalized causal tips"
        );
    }

    validate_exact_parent_frontier_capacity(
        this.casper_shard_conf.max_number_of_parents,
        parents.len(),
        finalized_floor_bonds.len(),
        causal_tips.len(),
        floor_backstop_added,
        expired_causal_tips,
    )?;
    validate_causal_parent_coverage(&dag, &snapshot_lfb_hash, &live_causal_tips, &parents)?;

    // C13 / Perf-3: hoist the parent-metadata lookup. Previously this
    // function performed two passes of `dag.lookup_unsafe` over the
    // same `parents` set — one to build `parent_metas_for_lca` and
    // another (via `lookups_unsafe`) to build `parent_metas`. The
    // batched `lookups_unsafe` is cheaper per parent, so use it once
    // up-front and borrow into the LCA call.
    let parent_hashes: Vec<BlockHash> = parents.iter().map(|b| b.block_hash.clone()).collect();
    let parent_metas = dag.lookups_unsafe(parent_hashes.clone())?;

    let lca = if parent_metas.is_empty() {
        this.approved_block.block_hash.clone()
    } else {
        crate::rust::util::dag_operations::DagOperations::lowest_universal_common_ancestor_many(
            &parent_metas,
            &dag,
        )
        .await?
        .block_hash
    };

    let tips: Vec<BlockHash> = parents.iter().map(|b| b.block_hash.clone()).collect();

    tracing::debug!(
        "Parent selection: {} validators, {} ineligible, {} valid, {} unfiltered, {} parents",
        latest_msgs_hashes.len(),
        latest_msgs_hashes
            .len()
            .saturating_sub(causal_parent_latest_msgs.len()),
        causal_parent_latest_msgs.len(),
        unfiltered_parents_count,
        parents.len()
    );

    let on_chain_state = get_on_chain_state(this, &snapshot_lfb).await?;

    let justifications = consensus_context
        .vote_projection()
        .exact_latest_messages()
        .iter()
        .map(|(validator, hash)| Justification {
            validator: validator.clone(),
            latest_block_hash: hash.clone(),
        })
        .collect();

    // C13 / Perf-3: `parent_metas` is reused from the hoisted lookup
    // above — no second pass of `dag.lookups_unsafe`.
    let max_block_num = proto_util::max_block_number_metadata(&parent_metas);

    let max_seq_nums = latest_sequence_numbers(&latest_metas)?;

    let (deploys_in_scope, rejected_in_scope) = {
        let current_dag_generation = this.block_dag_storage.current_generation();
        let cached: Option<(Arc<dashmap::DashSet<Bytes>>, Arc<dashmap::DashSet<Bytes>>)> = {
            // C16: `deploys_in_scope_cache` is a `parking_lot::Mutex` —
            // no poison propagation, `.lock()` returns the guard
            // directly. The prior `std::sync::Mutex` migration's
            // poison-handling branch has been removed.
            //
            // Merge of dev: cache tuple now carries both the deploys-in-scope
            // and the rejected-in-scope companion set so both can be served
            // out of one cache hit.
            let cache_guard = this.deploys_in_scope_cache.lock();
            cache_guard.as_ref().and_then(
                |(gen, cached_lfb, cached_parents, deploys_set, rejected_set)| {
                    if deploy_scope_cache_key_matches(
                        *gen,
                        cached_lfb,
                        cached_parents,
                        current_dag_generation,
                        &snapshot_lfb_hash,
                        &parent_hashes,
                    ) {
                        Some((deploys_set.clone(), rejected_set.clone()))
                    } else {
                        None
                    }
                },
            )
        };

        if let Some(pair) = cached {
            pair
        } else {
            // P2-9: checked arithmetic — alignment with T-9.14.
            let current_block_number = max_block_num.checked_add(1).ok_or_else(|| {
                CasperError::RuntimeError(format!(
                    "max_block_num overflow: {} + 1 wraps i64",
                    max_block_num
                ))
            })?;
            let earliest_block_number =
                current_block_number - on_chain_state.shard_conf.deploy_lifespan;

            // Propagate storage errors out of the BFS neighbor
            // expansion. Silent `.unwrap_or_default()` here is a
            // correctness bug: a transient storage failure on a
            // single parent would shrink `deploys_in_scope`, which
            // could then admit a duplicate-signature deploy past
            // `InvalidRepeatDeploy` detection.
            let neighbor_fn = |block_metadata: &models::rust::block_metadata::BlockMetadata| -> Result<
                Vec<models::rust::block_metadata::BlockMetadata>,
                shared::rust::store::key_value_store::KvStoreError,
            > {
                proto_util::get_parent_metadatas_above_block_number(
                    block_metadata,
                    earliest_block_number,
                    &dag,
                )
            };

            let traversal_result = dag_ops::try_bf_traverse(parent_metas, neighbor_fn)?;

            let all_deploys = Arc::new(dashmap::DashSet::new());
            for block_metadata in traversal_result {
                let block_deploy_sigs = this
                    .block_store
                    .deploy_sigs(&block_metadata.block_hash)?
                    .ok_or_else(|| {
                    CasperError::RuntimeError(format!(
                        "Missing block {} during deploys_in_scope traversal",
                        PrettyPrinter::build_string_bytes(&block_metadata.block_hash)
                    ))
                })?;
                for deploy_sig in block_deploy_sigs {
                    all_deploys.insert(deploy_sig.into());
                }
            }

            let (_, canonical_rejected) = interpreter_util::canonical_disposition_sets_at_floor(
                &this.block_store,
                &snapshot_lfb_hash,
                &parent_hashes,
                earliest_block_number,
                this.casper_shard_conf.casper_version,
            )?;
            let all_rejected: Arc<dashmap::DashSet<Bytes>> =
                Arc::new(canonical_rejected.into_iter().collect());

            // C16: parking_lot::Mutex — no poison propagation.
            let mut cache_guard = this.deploys_in_scope_cache.lock();
            *cache_guard = Some((
                current_dag_generation,
                snapshot_lfb_hash.clone(),
                parent_hashes.clone(),
                all_deploys.clone(),
                all_rejected.clone(),
            ));
            (all_deploys, all_rejected)
        }
    };
    let deploys_in_scope_len = deploys_in_scope.len();
    let deploys_in_scope_sig_bytes_estimate =
        (deploys_in_scope_len as f64) * DEPLOY_SIG_BYTES_ESTIMATE;
    metrics::gauge!(DEPLOYS_IN_SCOPE_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
        .set(deploys_in_scope_len as f64);
    metrics::gauge!(
        DEPLOYS_IN_SCOPE_SIG_BYTES_ESTIMATE_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(deploys_in_scope_sig_bytes_estimate);

    let invalid_blocks = dag.invalid_blocks_map()?;
    let last_finalized_block = snapshot_lfb_hash;
    record_dag_cardinality_metrics(&dag);

    Ok(CasperSnapshot {
        dag,
        last_finalized_block,
        lca,
        tips,
        parents,
        justifications,
        invalid_blocks,
        deploys_in_scope,
        rejected_in_scope,
        max_block_num,
        max_seq_nums,
        finalized_floor_bonds,
        on_chain_state,
        consensus_context,
        finalized_floor_certificate: Some(finalized_floor_certificate),
    })
}

pub(crate) async fn estimator<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    dag: &mut KeyValueDagRepresentation,
) -> Result<Vec<BlockHash>, CasperError> {
    let lfb_hash = dag.last_finalized_block();
    let context = crate::rust::causal_equivocation::CertifiedConsensusContext::for_finalized_floor(
        dag, lfb_hash,
    )?;
    if context
        .vote_projection()
        .eligible_latest_messages()
        .is_empty()
    {
        Ok(vec![this.approved_block.block_hash.clone()])
    } else {
        let unique_hashes: HashSet<BlockHash> = context
            .vote_projection()
            .eligible_latest_messages()
            .values()
            .cloned()
            .collect();
        Ok(unique_hashes.into_iter().collect())
    }
}

pub(crate) async fn get_on_chain_state<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    block: &BlockMessage,
) -> Result<OnChainCasperState, CasperError> {
    let bm = &block.body.state.bonds;

    Ok(OnChainCasperState {
        shard_conf: this.casper_shard_conf.clone(),
        bonds_map: bm
            .iter()
            .map(|v| (v.validator.clone(), v.stake))
            .collect::<HashMap<_, _>>(),
        bond_generations: block
            .body
            .state
            .bond_generations
            .iter()
            .map(|entry| (entry.validator.clone(), entry.generation))
            .collect(),
        active_validators: block.body.state.active_validators.clone(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};
    use std::time::SystemTime;

    use block_storage::rust::dag::block_dag_key_value_storage::{
        BlockDagKeyValueStorage, InsertMode,
    };
    use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use models::rust::block_implicits;
    use models::rust::block_metadata::BlockMetadata;
    use models::rust::casper::protocol::casper_message::{
        FinalizedFloorCommitment, ProcessedDeploy, RejectedDeploy,
    };
    use proptest::prelude::*;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::{
        causal_tips_covered_by_parents, deploy_scope_cache_key_matches,
        fallback_to_finalized_parent, latest_sequence_numbers,
        local_rejected_buffer_has_recoverable_deploys, order_parents_by_ghost_head,
        prune_dag_covered_parents, reachability_maximal_indices,
        recovery_main_covers_floor_and_causal_tips, validate_causal_parent_coverage,
        validate_exact_parent_frontier_capacity,
    };
    use crate::rust::errors::CasperError;

    #[test]
    fn sequence_snapshot_includes_invalid_latest_messages() {
        let valid = prost::bytes::Bytes::from_static(b"valid");
        let invalid = prost::bytes::Bytes::from_static(b"invalid");
        let mut valid_block = block_implicits::get_random_block_default();
        valid_block.seq_num = 4;
        let mut invalid_block = block_implicits::get_random_block_default();
        invalid_block.seq_num = 9;
        let latest = std::collections::HashMap::from([
            (
                valid.clone(),
                BlockMetadata::from_block(&valid_block, None, None),
            ),
            (
                invalid.clone(),
                BlockMetadata::from_block(&invalid_block, None, None),
            ),
        ]);

        let sequences = latest_sequence_numbers(&latest).expect("latest sequence snapshot");

        assert_eq!(sequences.get(&valid), Some(&4));
        assert_eq!(sequences.get(&invalid), Some(&9));
    }

    #[test]
    fn deploy_scope_cache_key_includes_selected_parents() {
        let lfb = prost::bytes::Bytes::from_static(b"lfb");
        let parent_a = prost::bytes::Bytes::from_static(b"a");
        let parent_b = prost::bytes::Bytes::from_static(b"b");

        assert!(deploy_scope_cache_key_matches(
            7,
            &lfb,
            std::slice::from_ref(&parent_a),
            7,
            &lfb,
            std::slice::from_ref(&parent_a)
        ));
        assert!(!deploy_scope_cache_key_matches(
            7,
            &lfb,
            std::slice::from_ref(&parent_a),
            7,
            &lfb,
            std::slice::from_ref(&parent_b)
        ));
        assert!(!deploy_scope_cache_key_matches(
            7,
            &lfb,
            &[parent_a.clone(), parent_b.clone()],
            7,
            &lfb,
            &[parent_b, parent_a]
        ));
    }

    #[tokio::test]
    async fn empty_valid_parent_set_falls_back_to_last_finalized_block() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let finalized = block_implicits::get_random_block(
            Some(7),
            Some(3),
            None,
            None,
            None,
            Some(crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION),
            Some(7),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        block_store
            .put_block_message(&finalized)
            .expect("store finalized block");

        let selected =
            fallback_to_finalized_parent(&block_store, Vec::new(), &finalized.block_hash)
                .expect("finalized fallback");
        assert_eq!(selected, vec![finalized.clone()]);

        let declared = vec![finalized.clone()];
        assert_eq!(
            fallback_to_finalized_parent(&block_store, declared.clone(), &finalized.block_hash)
                .expect("declared parents"),
            declared
        );
        assert!(fallback_to_finalized_parent(
            &block_store,
            Vec::new(),
            &prost::bytes::Bytes::from_static(b"missing"),
        )
        .is_err());
    }

    #[tokio::test]
    async fn parent_selection_prunes_dag_covered_parents() {
        let mut kvm = InMemoryStoreManager::new();
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");

        let genesis = block_implicits::get_random_block(
            Some(0),
            Some(0),
            None,
            None,
            None,
            Some(crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION),
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let bind_genesis_floor =
            |mut block: models::rust::casper::protocol::casper_message::BlockMessage| {
                block.header.finalized_floor = Some(FinalizedFloorCommitment {
                    floor_hash: genesis.block_hash.clone(),
                    floor_post_state_hash: genesis.body.state.post_state_hash.clone(),
                    certificate_digest: prost::bytes::Bytes::from(
                        vec![1; models::rust::block_hash::LENGTH],
                    ),
                    authority_context_digest: prost::bytes::Bytes::from(
                        vec![2; models::rust::block_hash::LENGTH],
                    ),
                });
                block.block_hash = crate::rust::util::proto_util::hash_block(&block);
                block
            };
        let left = bind_genesis_floor(block_implicits::get_random_block(
            Some(1),
            Some(1),
            None,
            None,
            None,
            Some(crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION),
            Some(1),
            Some(vec![genesis.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        ));
        let right = bind_genesis_floor(block_implicits::get_random_block(
            Some(1),
            Some(2),
            None,
            None,
            None,
            Some(crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION),
            Some(1),
            Some(vec![genesis.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        ));
        let seal = bind_genesis_floor(block_implicits::get_random_block(
            Some(2),
            Some(3),
            None,
            None,
            None,
            Some(crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION),
            Some(2),
            Some(vec![left.block_hash.clone(), right.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        ));
        let left_child = bind_genesis_floor(block_implicits::get_random_block(
            Some(3),
            Some(4),
            None,
            None,
            None,
            Some(crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION),
            Some(3),
            Some(vec![seal.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        ));
        let right_child = bind_genesis_floor(block_implicits::get_random_block(
            Some(3),
            Some(5),
            None,
            None,
            None,
            Some(crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION),
            Some(3),
            Some(vec![seal.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        ));

        dag_storage
            .insert(&genesis, InsertMode::ApprovedGenesis)
            .expect("insert genesis");
        for block in [&left, &right, &seal, &left_child, &right_child] {
            dag_storage
                .insert(block, InsertMode::Normal)
                .expect("insert block");
        }

        let dag = dag_storage.get_representation().expect("dag");
        let pruned =
            prune_dag_covered_parents(&dag, vec![left.clone(), seal.clone(), right.clone()])
                .expect("prune parents");
        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0].block_hash, seal.block_hash);
        let causal_tips = HashSet::from([left.block_hash.clone(), right.block_hash.clone()]);
        validate_causal_parent_coverage(&dag, &genesis.block_hash, &causal_tips, &pruned)
            .expect("covering merge parent preserves both causal tips");

        let siblings = prune_dag_covered_parents(&dag, vec![left.clone(), right.clone()])
            .expect("keep siblings");
        assert_eq!(
            siblings
                .iter()
                .map(|block| block.block_hash.clone())
                .collect::<Vec<_>>(),
            vec![left.block_hash.clone(), right.block_hash.clone()]
        );
        validate_causal_parent_coverage(&dag, &genesis.block_hash, &causal_tips, &siblings)
            .expect("sibling parent set preserves both causal tips");
        assert!(validate_causal_parent_coverage(
            &dag,
            &genesis.block_hash,
            &causal_tips,
            std::slice::from_ref(&siblings[0]),
        )
        .is_err());
        assert!(
            validate_causal_parent_coverage(&dag, &seal.block_hash, &causal_tips, &siblings,)
                .is_err()
        );

        let diverged_from_seal =
            prune_dag_covered_parents(&dag, vec![left_child.clone(), right_child.clone(), seal])
                .expect("remove every covered common anchor");
        assert_eq!(diverged_from_seal.len(), 2);
        assert_eq!(diverged_from_seal[0].block_hash, left_child.block_hash);
        assert_eq!(diverged_from_seal[1].block_hash, right_child.block_hash);

        let left_tip = HashSet::from([left.block_hash.clone()]);
        assert!(!recovery_main_covers_floor_and_causal_tips(
            &dag,
            &right.block_hash,
            &left.block_hash,
            &left_tip,
        )
        .expect("incompatible floor check"));
        assert!(recovery_main_covers_floor_and_causal_tips(
            &dag,
            &genesis.block_hash,
            &left.block_hash,
            &left_tip,
        )
        .expect("covered floor check"));

        let depth_candidates = HashSet::from([
            left_child.block_hash.clone(),
            right_child.block_hash.clone(),
        ]);
        let live_after_depth = causal_tips_covered_by_parents(
            &dag,
            &depth_candidates,
            std::slice::from_ref(&left_child),
        )
        .expect("derive live causal tips after depth expiry");
        assert_eq!(live_after_depth, HashSet::from([left_child.block_hash]));
    }

    #[test]
    fn exact_parent_frontier_capacity_uses_the_frozen_frontier() {
        assert!(validate_exact_parent_frontier_capacity(101, 1, 0, 0, true, 0).is_ok());
        assert!(validate_exact_parent_frontier_capacity(101, 4, 3, 3, true, 0).is_ok());
        assert!(validate_exact_parent_frontier_capacity(3, 3, 10_000, 3, false, 0).is_ok());

        let error = validate_exact_parent_frontier_capacity(2, 3, 3, 3, false, 0)
            .expect_err("an actual frontier wider than the cap must defer");
        assert_eq!(error, CasperError::ParentFrontierCapacityExceeded {
            configured_cap: 2,
            required_parents: 3,
            effective_committee: 3,
            unique_causal_tips: 3,
            floor_backstop_added: false,
            expired_tip_count: 0,
        });
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn reachability_compaction_is_a_covering_maximal_antichain(
            levels in proptest::collection::vec(0u8..32, 1..64),
        ) {
            let retained = reachability_maximal_indices(levels.len(), |left, right| {
                Ok::<bool, ()>(levels[left] < levels[right])
            })
            .unwrap();

            prop_assert!(!retained.is_empty());
            for &left in &retained {
                for &right in &retained {
                    prop_assert!(left == right || levels[left] >= levels[right]);
                }
            }
            for candidate in 0..levels.len() {
                let covered = retained.iter().any(|&parent| {
                    candidate == parent || levels[candidate] < levels[parent]
                });
                prop_assert!(covered);
            }
        }

        #[test]
        fn exact_parent_frontier_capacity_matches_the_cardinality_oracle(
            required in 1usize..128,
            cap in 1i32..128,
            committee in 0usize..256,
            unique_tips in 0usize..256,
            floor_backstop_added in any::<bool>(),
            expired_tip_count in 0usize..256,
        ) {
            let result = validate_exact_parent_frontier_capacity(
                cap,
                required,
                committee,
                unique_tips,
                floor_backstop_added,
                expired_tip_count,
            );
            prop_assert_eq!(result.is_ok(), required <= cap as usize);
        }

        #[test]
        fn ghost_parent_order_is_permutation_invariant_and_preserves_the_head(
            values in proptest::collection::btree_set(any::<u8>(), 1..32),
            reverse in any::<bool>(),
        ) {
            let mut parents = values
                .iter()
                .map(|value| {
                    let mut block = block_implicits::get_random_block_default();
                    block.block_hash = prost::bytes::Bytes::from(vec![*value]);
                    block
                })
                .collect::<Vec<_>>();
            let ghost = parents[parents.len() / 2].block_hash.clone();
            if reverse {
                parents.reverse();
            } else {
                let rotation = parents.len() / 3;
                parents.rotate_left(rotation);
            }
            let input_hashes = parents
                .iter()
                .map(|parent| parent.block_hash.clone())
                .collect::<HashSet<_>>();

            let ordered = order_parents_by_ghost_head(parents, &ghost).unwrap();
            let output_hashes = ordered
                .iter()
                .map(|parent| parent.block_hash.clone())
                .collect::<HashSet<_>>();

            prop_assert_eq!(&ordered[0].block_hash, &ghost);
            prop_assert_eq!(output_hashes, input_hashes);
            prop_assert!(ordered[1..]
                .windows(2)
                .all(|pair| pair[0].block_hash < pair[1].block_hash));
        }
    }

    #[tokio::test]
    async fn local_rejected_backlog_requires_selectable_deploy() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let floor = block_implicits::get_random_block(
            Some(0),
            Some(0),
            None,
            None,
            None,
            Some(crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION),
            Some(0),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        block_store.put_block_message(&floor).expect("store floor");
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("time")
            .as_millis() as i64;
        let mut expired = crate::rust::util::construct_deploy::source_deploy_now(
            "@expired!(0)".to_string(),
            None,
            Some(10),
            Some("test".to_string()),
        )
        .expect("expired deploy");
        expired.data.expiration_timestamp = Some(now - 1);
        let old = crate::rust::util::construct_deploy::source_deploy_now(
            "@old!(0)".to_string(),
            None,
            Some(-40),
            Some("test".to_string()),
        )
        .expect("old deploy");
        let future = crate::rust::util::construct_deploy::source_deploy_now(
            "@future!(0)".to_string(),
            None,
            Some(20),
            Some("test".to_string()),
        )
        .expect("future deploy");
        rejected_deploy_buffer
            .lock()
            .expect("buffer lock")
            .add(vec![expired, old, future])
            .expect("seed buffer");

        assert!(!local_rejected_buffer_has_recoverable_deploys(
            &block_store,
            &rejected_deploy_buffer,
            &floor.block_hash,
            &[],
            20,
            now,
            50,
            crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
        )
        .expect("check unselectable backlog"));

        let fresh = crate::rust::util::construct_deploy::source_deploy_now(
            "@fresh!(0)".to_string(),
            None,
            Some(19),
            Some("test".to_string()),
        )
        .expect("fresh deploy");
        rejected_deploy_buffer
            .lock()
            .expect("buffer lock")
            .add(vec![fresh])
            .expect("seed fresh");

        assert!(local_rejected_buffer_has_recoverable_deploys(
            &block_store,
            &rejected_deploy_buffer,
            &floor.block_hash,
            &[],
            20,
            now,
            50,
            crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
        )
        .expect("check selectable backlog"));
    }

    #[tokio::test]
    async fn local_rejected_backlog_ignores_canonical_wins() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let canonical = crate::rust::util::construct_deploy::source_deploy_now(
            "@canonical!(0)".to_string(),
            None,
            Some(10),
            Some("test".to_string()),
        )
        .expect("canonical deploy");
        let parent = block_implicits::get_random_block(
            Some(19),
            Some(19),
            None,
            None,
            None,
            None,
            Some(19),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(canonical.clone())]),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        block_store
            .put_block_message(&parent)
            .expect("store parent");
        rejected_deploy_buffer
            .lock()
            .expect("buffer lock")
            .add(vec![canonical])
            .expect("seed buffer");
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("time")
            .as_millis() as i64;

        assert!(!local_rejected_buffer_has_recoverable_deploys(
            &block_store,
            &rejected_deploy_buffer,
            &parent.block_hash,
            std::slice::from_ref(&parent.block_hash),
            20,
            now,
            50,
            crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
        )
        .expect("check canonical backlog"));
    }

    #[tokio::test]
    async fn historical_rejection_without_local_backlog_does_not_trigger_recovery() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));

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
            None,
            Some("test".to_string()),
            None,
        );
        let mut rejected = block_implicits::get_random_block(
            Some(1),
            Some(1),
            None,
            None,
            None,
            None,
            Some(1),
            Some(vec![genesis.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        rejected.body.rejected_deploys = vec![RejectedDeploy::legacy(
            prost::bytes::Bytes::from_static(b"sig"),
        )];

        block_store
            .put_block_message(&rejected)
            .expect("store rejected");

        assert!(!local_rejected_buffer_has_recoverable_deploys(
            &block_store,
            &rejected_deploy_buffer,
            &rejected.block_hash,
            std::slice::from_ref(&rejected.block_hash),
            2,
            i64::MAX,
            50,
            1,
        )
        .expect("empty backlog"));
    }

    #[test]
    fn ghost_head_remains_first_when_another_parent_carries_a_deploy() {
        let ghost = block_implicits::get_random_block(
            Some(2),
            Some(2),
            None,
            None,
            None,
            None,
            Some(2),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let deploy = crate::rust::util::construct_deploy::basic_deploy_data(
            0,
            None,
            Some("test".to_string()),
        )
        .expect("deploy");
        let deploy_parent = block_implicits::get_random_block(
            Some(3),
            Some(3),
            None,
            None,
            None,
            None,
            Some(3),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy)]),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );

        let ordered = order_parents_by_ghost_head(
            vec![deploy_parent.clone(), ghost.clone()],
            &ghost.block_hash,
        )
        .expect("order by GHOST head");

        assert_eq!(ordered[0].block_hash, ghost.block_hash);
        assert_eq!(ordered[1].block_hash, deploy_parent.block_hash);
        assert!(order_parents_by_ghost_head(vec![deploy_parent], &ghost.block_hash).is_err());
    }
}
