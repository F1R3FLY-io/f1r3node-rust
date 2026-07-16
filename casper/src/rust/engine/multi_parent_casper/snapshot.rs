//! Snapshot construction — `compute_snapshot`, `get_on_chain_state`,
//! `record_dag_cardinality_metrics`, `estimator`.
//!
//! Phase 3 Step 3 — extracted from `engine::multi_parent_casper`. Each
//! function takes the casper instance as a `&MultiParentCasperImpl<T>`
//! reference (rather than `&self`) so the implementation can live in this
//! module while the trait method is a one-line delegate in `traits.rs`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{BlockMessage, Justification};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use shared::rust::dag::dag_ops;

use super::types::MultiParentCasperImpl;
use crate::rust::casper::{CasperSnapshot, OnChainCasperState};
use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    ACTIVE_VALIDATORS_CACHE_SIZE_METRIC, CASPER_METRICS_SOURCE, DAG_BLOCKS_SIZE_METRIC,
    DAG_CHILDREN_INDEX_SIZE_METRIC, DAG_FINALIZED_BLOCKS_SIZE_METRIC, DAG_HEIGHTS_SIZE_METRIC,
    DEPLOYS_IN_SCOPE_SIG_BYTES_ESTIMATE_METRIC, DEPLOYS_IN_SCOPE_SIZE_METRIC,
};
use crate::rust::util::proto_util;

/// C15 / Smell-1: byte-size estimate for a secp256k1 compact-encoded
/// deploy signature. ~64 bytes signature + 1 byte prefix. Used to
/// drive the `DEPLOYS_IN_SCOPE_SIG_BYTES_ESTIMATE_METRIC` gauge — the
/// gauge is operator-facing memory-pressure telemetry, NOT a
/// consensus-critical value, so a rounded estimate (rather than a
/// per-deploy actual-byte sum) is intentional.
const DEPLOY_SIG_BYTES_ESTIMATE: f64 = 65.0;

#[derive(Clone, Debug, PartialEq, Eq)]
struct DeployBranchScore {
    deploy_sig_count: usize,
    latest_deploy_block_number: i64,
    root_block_number: i64,
}

fn better_deploy_branch_score(
    candidate: (&DeployBranchScore, &BlockHash),
    current: (&DeployBranchScore, &BlockHash),
) -> bool {
    candidate
        .0
        .deploy_sig_count
        .cmp(&current.0.deploy_sig_count)
        .then_with(|| {
            candidate
                .0
                .latest_deploy_block_number
                .cmp(&current.0.latest_deploy_block_number)
        })
        .then_with(|| {
            candidate
                .0
                .root_block_number
                .cmp(&current.0.root_block_number)
        })
        .then_with(|| current.1.cmp(candidate.1))
        .is_gt()
}

fn branch_unfinalized_user_deploy_score(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    root_hash: &BlockHash,
    last_finalized_block: &BlockHash,
) -> Result<Option<DeployBranchScore>, CasperError> {
    let last_finalized_number = dag
        .lookup(last_finalized_block)?
        .map(|meta| meta.block_number)
        .unwrap_or(-1);
    let root_meta = dag.lookup_unsafe(root_hash)?;
    let mut stack = vec![root_hash.clone()];
    let mut seen: HashSet<BlockHash> = HashSet::new();
    let mut deploy_sigs: HashSet<Bytes> = HashSet::new();
    let mut latest_deploy_block_number: Option<i64> = None;

    while let Some(block_hash) = stack.pop() {
        if !seen.insert(block_hash.clone())
            || &block_hash == last_finalized_block
            || dag.is_finalized(&block_hash)
        {
            continue;
        }

        let block_meta = dag.lookup_unsafe(&block_hash)?;
        if block_meta.block_number <= last_finalized_number {
            continue;
        }

        if let Some(sigs) = block_store.deploy_sigs(&block_hash)? {
            if !sigs.is_empty() {
                latest_deploy_block_number = Some(
                    latest_deploy_block_number
                        .map(|current| current.max(block_meta.block_number))
                        .unwrap_or(block_meta.block_number),
                );
                for sig in sigs {
                    deploy_sigs.insert(sig.into());
                }
            }
        }

        stack.extend(block_meta.parents.iter().cloned());
    }

    Ok(latest_deploy_block_number.map(|latest| DeployBranchScore {
        deploy_sig_count: deploy_sigs.len(),
        latest_deploy_block_number: latest,
        root_block_number: root_meta.block_number,
    }))
}

fn prefer_deploy_support_main_parent(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    parents: Vec<BlockMessage>,
    last_finalized_block: &BlockHash,
) -> Result<Vec<BlockMessage>, CasperError> {
    if parents.len() <= 1 {
        return Ok(parents);
    }

    let mut scored: Vec<Option<DeployBranchScore>> = Vec::with_capacity(parents.len());
    for parent in &parents {
        scored.push(branch_unfinalized_user_deploy_score(
            dag,
            block_store,
            &parent.block_hash,
            last_finalized_block,
        )?);
    }

    let mut best: Option<(usize, &DeployBranchScore)> = None;
    for (idx, score) in scored.iter().enumerate() {
        let Some(score) = score.as_ref() else {
            continue;
        };
        let replace = best
            .as_ref()
            .map(|(best_idx, best_score)| {
                better_deploy_branch_score(
                    (score, &parents[idx].block_hash),
                    (best_score, &parents[*best_idx].block_hash),
                )
            })
            .unwrap_or(true);
        if replace {
            best = Some((idx, score));
        }
    }

    let Some((best_idx, best_score)) = best else {
        return Ok(parents);
    };
    if best_idx == 0 {
        return Ok(parents);
    }

    let original_main = parents[0].block_hash.clone();
    let promoted = parents[best_idx].block_hash.clone();
    let mut reordered = parents;
    let promoted_parent = reordered.remove(best_idx);
    reordered.insert(0, promoted_parent);
    tracing::info!(
        target: "f1r3fly.casper.deploy_support",
        "Parent selection promoted deploy-carrying branch for canonical support: original_main={}, promoted_main={}, deploy_sigs={}, latest_deploy_block={}, promoted_root_block={}",
        PrettyPrinter::build_string_bytes(&original_main),
        PrettyPrinter::build_string_bytes(&promoted),
        best_score.deploy_sig_count,
        best_score.latest_deploy_block_number,
        best_score.root_block_number
    );
    Ok(reordered)
}

fn prune_dag_covered_parents(
    dag: &KeyValueDagRepresentation,
    parents: Vec<BlockMessage>,
) -> Result<Vec<BlockMessage>, CasperError> {
    if parents.len() <= 1 {
        return Ok(parents);
    }

    for (idx, candidate) in parents.iter().enumerate() {
        if !candidate.body.deploys.is_empty() {
            continue;
        }
        let mut covers_all = true;
        for (other_idx, other) in parents.iter().enumerate() {
            if idx == other_idx {
                continue;
            }
            if !dag.is_dag_ancestor(&other.block_hash, &candidate.block_hash)? {
                covers_all = false;
                break;
            }
        }
        if covers_all {
            tracing::info!(
                target: "f1r3fly.casper.parent_selection",
                original_parents = parents.len(),
                pruned_parents = 1,
                "Parent selection collapsed to DAG-covering parent"
            );
            return Ok(vec![candidate.clone()]);
        }
    }
    Ok(parents)
}

fn candidate_scope_has_rejected_deploys(
    dag: &KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    parent_metas: Vec<BlockMetadata>,
    current_block_number: i64,
    deploy_lifespan: i64,
) -> Result<bool, CasperError> {
    let earliest_block_number = current_block_number - deploy_lifespan;
    let neighbor_fn = |block_metadata: &BlockMetadata| {
        proto_util::get_parent_metadatas_above_block_number(
            block_metadata,
            earliest_block_number,
            dag,
        )
    };
    let traversal_result = dag_ops::try_bf_traverse(parent_metas, neighbor_fn)?;
    for block_metadata in traversal_result {
        if block_store
            .rejected_deploy_sigs(&block_metadata.block_hash)?
            .map(|sigs| !sigs.is_empty())
            .unwrap_or(false)
        {
            return Ok(true);
        }
    }
    Ok(false)
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

pub(crate) async fn compute_snapshot<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
) -> Result<CasperSnapshot, CasperError> {
    if this
        .finalization_in_progress
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        tracing::debug!(
            "Finalization in progress while creating snapshot; using best-effort snapshot"
        );
    }

    let mut dag = this.block_dag_storage.get_representation()?;

    // Parent selection: Use latest block from EACH bonded validator.
    // Phase 12 (PERF-5): `latest_message_hashes()` returns an owned
    // `imbl::HashMap` already (refcount-bump clone). Use `into_iter` to
    // collect by ownership rather than re-cloning every key/value.
    let latest_msgs_hashes: HashMap<Validator, BlockHash> =
        dag.latest_message_hashes().into_iter().collect();
    let validator_capacity = latest_msgs_hashes.len();
    let invalid_latest_msgs = dag.invalid_latest_messages_from_hashes(&latest_msgs_hashes)?;
    // Phase 12 (PERF-7): each subsequent collection is bounded by the
    // current validator-set cardinality. Preallocating avoids
    // power-of-two HashMap/HashSet/Vec growth and the rehashes that come
    // with it on every snapshot.
    let mut valid_latest_msgs: HashMap<Validator, BlockHash> =
        HashMap::with_capacity(validator_capacity);
    for (validator, hash) in latest_msgs_hashes.iter() {
        if invalid_latest_msgs.contains_key(validator) {
            continue;
        }
        valid_latest_msgs.insert(validator.clone(), hash.clone());
    }
    // Storage errors during snapshot construction must propagate: a
    // silent empty `valid_latest_metas` would feed wrong fork-choice on
    // the consensus hot path. Bug #17 / T-9.20 hardened this contract
    // for crash-window drift; same discipline applies to general
    // storage I/O.
    let mut valid_latest_metas: HashMap<Validator, models::rust::block_metadata::BlockMetadata> =
        HashMap::with_capacity(valid_latest_msgs.len());
    for (validator, hash) in valid_latest_msgs.iter() {
        let meta = dag.lookup_unsafe(hash)?;
        valid_latest_metas.insert(validator.clone(), meta);
    }
    let mut unique_parent_hashes: HashSet<BlockHash> =
        HashSet::with_capacity(valid_latest_msgs.len());
    for hash in valid_latest_msgs.values() {
        unique_parent_hashes.insert(hash.clone());
    }
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

    // Sealed-floor: LMD-GHOST main-parent selection. Compute the ghost
    // main-parent from the fork-choice tips over this block's justification
    // snapshot, then order parents so the ghost main-parent sorts first
    // (then by block hash for determinism).
    //
    // The bonded-set ("most-slashed" intersection) parent filter that
    // previously occupied the else-arm below was removed. Committee/bonds
    // are now validated against the finalized floor
    // (`Validate::bonds_cache_from_floor`) together with
    // `neglected_invalid_block` — both validator-side and finalization-
    // anchored. A proposer-side parent filter cannot be a consensus-safety
    // mechanism (validators replay declared parents, not fork-choice), so it
    // was redundant. See docs/sealed-floor-merge-v2-status.md.
    let ghost_main_parent = this
        .estimator
        .tips_with_latest_messages(&mut dag, &this.approved_block, valid_latest_msgs.clone())
        .await?
        .tips
        .into_iter()
        .next();
    let mut sorted_parents_list = parent_blocks_list;
    sorted_parents_list.sort_by(|a, b| {
        let a_main = ghost_main_parent.as_ref() == Some(&a.block_hash);
        let b_main = ghost_main_parent.as_ref() == Some(&b.block_hash);
        b_main
            .cmp(&a_main)
            .then_with(|| a.block_hash.cmp(&b.block_hash))
    });
    let sorted_parents_list = prefer_deploy_support_main_parent(
        &dag,
        &this.block_store,
        sorted_parents_list,
        &dag.last_finalized_block(),
    )?;

    let rejected_deploys_in_candidate_scope = if sorted_parents_list.is_empty() {
        false
    } else {
        let sorted_parent_hashes: Vec<BlockHash> = sorted_parents_list
            .iter()
            .map(|block| block.block_hash.clone())
            .collect();
        let sorted_parent_metas = dag.lookups_unsafe(sorted_parent_hashes)?;
        let candidate_block_number = proto_util::max_block_number_metadata(&sorted_parent_metas)
            .checked_add(1)
            .ok_or_else(|| {
                CasperError::RuntimeError(
                    "candidate max_block_num overflow while checking recovery context".to_string(),
                )
            })?;
        candidate_scope_has_rejected_deploys(
            &dag,
            &this.block_store,
            sorted_parent_metas,
            candidate_block_number,
            this.casper_shard_conf.deploy_lifespan,
        )?
    };

    let recovery_backlog = {
        let buffer_guard = this
            .rejected_deploy_buffer
            .lock()
            .map_err(|err| CasperError::LockError(err.to_string()))?;
        buffer_guard.non_empty().map_err(CasperError::from)?
    };
    let recovery_context = recovery_backlog || rejected_deploys_in_candidate_scope;

    let unfiltered_parents = if sorted_parents_list.is_empty() {
        vec![this.approved_block.clone()]
    } else if recovery_context && sorted_parents_list.len() > 1 {
        tracing::info!(
            target: "f1r3fly.casper.recovery",
            "Parent selection narrowed for deploy recovery: original_parents={}, selected_main={}, local_buffer={}, rejected_in_scope={}",
            sorted_parents_list.len(),
            PrettyPrinter::build_string_bytes(&sorted_parents_list[0].block_hash),
            recovery_backlog,
            rejected_deploys_in_candidate_scope
        );
        vec![sorted_parents_list[0].clone()]
    } else {
        sorted_parents_list
    };

    let unfiltered_parents_count = unfiltered_parents.len();
    let compacted_parents = prune_dag_covered_parents(&dag, unfiltered_parents)?;

    // C15 / Smell-3: shared wire-convention constant — see
    // `crate::rust::casper::UNLIMITED_PARENTS`.
    let mut parents_after_count_limit = compacted_parents;
    if this.casper_shard_conf.max_number_of_parents != crate::rust::casper::UNLIMITED_PARENTS {
        parents_after_count_limit.truncate(this.casper_shard_conf.max_number_of_parents as usize);
    }

    let parents = if this.casper_shard_conf.max_parent_depth != i32::MAX
        && parents_after_count_limit.len() > 1
    {
        // C13 / Perf-2: collapse the build-then-max-then-filter triple
        // pass into a single forward iteration that maintains
        // `max_block_num` incrementally, followed by an in-place
        // `retain` on the vector. Eliminates one intermediate Vec
        // allocation per snapshot and a redundant `.iter()` walk for
        // the max computation.
        let mut parents_with_meta: Vec<(
            BlockMessage,
            models::rust::block_metadata::BlockMetadata,
        )> = Vec::with_capacity(parents_after_count_limit.len());
        let mut max_block_num: i64 = 0;
        for b in parents_after_count_limit {
            let meta = dag.lookup_unsafe(&b.block_hash)?;
            if meta.block_number > max_block_num {
                max_block_num = meta.block_number;
            }
            parents_with_meta.push((b, meta));
        }

        let depth = this.casper_shard_conf.max_parent_depth as i64;
        parents_with_meta.retain(|(_, meta)| max_block_num - meta.block_number <= depth);
        parents_with_meta.into_iter().map(|(b, _)| b).collect()
    } else {
        parents_after_count_limit
    };

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
        "Parent selection: {} validators, {} invalid, {} valid, {} unfiltered, {} parents",
        latest_msgs_hashes.len(),
        invalid_latest_msgs.len(),
        valid_latest_msgs.len(),
        unfiltered_parents_count,
        parents.len()
    );

    let on_chain_state = get_on_chain_state(
        this,
        parents
            .first()
            .expect("parents should never be empty after approved block"),
    )
    .await?;

    let justifications = {
        let bonded_validators = &on_chain_state.bonds_map;

        // Include justifications for ALL bonded validators based on their
        // *unfiltered* latest_messages, valid OR invalid. The proposer must
        // satisfy `justification_follows` (T-9.7), which requires every
        // bonded validator to appear in the block's justifications.
        // Filtering to only `valid_latest_metas` here would drop the
        // equivocator's slot from the snapshot, so the proposer's resulting
        // block would lack the equivocator's justification and be flagged
        // `InvalidFollows` downstream — even though parent-selection /
        // fork-choice correctly use `valid_latest_metas` only.
        //
        // This pairs with the LMM-for-invalid-blocks invariant documented
        // at `block-storage/src/rust/dag/block_dag_key_value_storage.rs`'s
        // `new_latest_messages` closure: invalid blocks advance LMM
        // precisely so the equivocator's slot is reachable here, and
        // `justification_follows` (validator-side) plus
        // `check_neglected_equivocations_with_update` (T-9.7 detection)
        // can both work.
        latest_msgs_hashes
            .iter()
            .filter(|(validator, _)| bonded_validators.contains_key(*validator))
            .map(|(validator, hash)| Justification {
                validator: validator.clone(),
                latest_block_hash: hash.clone(),
            })
            .collect::<HashSet<_>>()
    };

    // C13 / Perf-3: `parent_metas` is reused from the hoisted lookup
    // above — no second pass of `dag.lookups_unsafe`.
    let max_block_num = proto_util::max_block_number_metadata(&parent_metas);

    let max_seq_nums = valid_latest_metas
        .iter()
        .map(
            |(validator, block_metadata): (
                &Validator,
                &models::rust::block_metadata::BlockMetadata,
            )| (validator.clone(), block_metadata.sequence_number as u64),
        )
        .collect::<HashMap<_, _>>();

    let (deploys_in_scope, rejected_in_scope) = {
        let current_dag_generation = this.block_dag_storage.current_generation();
        let snapshot_lfb_hash = dag.last_finalized_block();

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
            let all_rejected = Arc::new(dashmap::DashSet::new());
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
                // Merge of dev: a deploy that was executed in this block
                // and rejected by a descendant merge contributes to the
                // rejected_in_scope set. The block creator and validator
                // intersect this with deploys_in_scope to decide
                // re-inclusion eligibility for merge-rejected deploys.
                if let Some(rejected_sigs) = this
                    .block_store
                    .rejected_deploy_sigs(&block_metadata.block_hash)?
                {
                    for sig in rejected_sigs {
                        all_rejected.insert(sig.into());
                    }
                }
            }

            // C16: parking_lot::Mutex — no poison propagation.
            let mut cache_guard = this.deploys_in_scope_cache.lock();
            *cache_guard = Some((
                current_dag_generation,
                snapshot_lfb_hash,
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
    let last_finalized_block = dag.last_finalized_block();
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
        on_chain_state,
    })
}

pub(crate) async fn estimator<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    dag: &mut KeyValueDagRepresentation,
) -> Result<Vec<BlockHash>, CasperError> {
    // Phase 12 (PERF-5): use `into_iter` to consume the already-owned
    // `imbl::HashMap` rather than re-cloning every pair.
    let latest_message_hashes: HashMap<Validator, BlockHash> =
        dag.latest_message_hashes().into_iter().collect();
    let invalid_latest_messages =
        dag.invalid_latest_messages_from_hashes(&latest_message_hashes)?;

    let valid_latest: HashMap<Validator, BlockHash> = latest_message_hashes
        .iter()
        .filter(|(validator, _)| !invalid_latest_messages.contains_key(*validator))
        .map(|(validator, hash): (&Validator, &BlockHash)| (validator.clone(), hash.clone()))
        .collect();

    if valid_latest.is_empty() {
        Ok(vec![this.approved_block.block_hash.clone()])
    } else {
        let unique_hashes: HashSet<BlockHash> = valid_latest.values().cloned().collect();
        Ok(unique_hashes.into_iter().collect())
    }
}

pub(crate) async fn get_on_chain_state<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    block: &BlockMessage,
) -> Result<OnChainCasperState, CasperError> {
    let cache_key = block.body.state.post_state_hash.to_vec();
    let (cached_hit, cache_len) = {
        let cache = this.active_validators_cache.lock().await;
        (cache.get(&cache_key).cloned(), cache.len())
    };
    if let Some(cached) = cached_hit {
        metrics::gauge!(ACTIVE_VALIDATORS_CACHE_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(cache_len as f64);
        let bm = &block.body.state.bonds;
        return Ok(OnChainCasperState {
            shard_conf: this.casper_shard_conf.clone(),
            bonds_map: bm
                .iter()
                .map(|v| (v.validator.clone(), v.stake))
                .collect::<HashMap<_, _>>(),
            active_validators: cached,
        });
    }

    let fetched = this
        .runtime_manager
        .get_active_validators(&block.body.state.post_state_hash)
        .await?;

    let av = {
        let mut cache = this.active_validators_cache.lock().await;
        if cache.len() >= this.casper_shard_conf.active_validators_cache_max_entries {
            if let Some(first_key) = cache.keys().next().cloned() {
                cache.remove(&first_key);
            }
        }
        let entry = cache
            .entry(cache_key)
            .or_insert_with(|| fetched.clone())
            .clone();
        let cache_len = cache.len();
        metrics::gauge!(ACTIVE_VALIDATORS_CACHE_SIZE_METRIC, "source" => CASPER_METRICS_SOURCE)
            .set(cache_len as f64);
        entry
    };

    let bm = &block.body.state.bonds;

    Ok(OnChainCasperState {
        shard_conf: this.casper_shard_conf.clone(),
        bonds_map: bm
            .iter()
            .map(|v| (v.validator.clone(), v.stake))
            .collect::<HashMap<_, _>>(),
        active_validators: av,
    })
}

#[cfg(test)]
mod tests {
    use block_storage::rust::dag::block_dag_key_value_storage::{
        BlockDagKeyValueStorage, InsertMode,
    };
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use models::rust::block_implicits;
    use models::rust::casper::protocol::casper_message::{ProcessedDeploy, RejectedDeploy};
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::{
        candidate_scope_has_rejected_deploys, deploy_scope_cache_key_matches,
        prefer_deploy_support_main_parent, prune_dag_covered_parents,
    };

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
        let left = block_implicits::get_random_block(
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
        let right = block_implicits::get_random_block(
            Some(1),
            Some(2),
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
        let seal = block_implicits::get_random_block(
            Some(2),
            Some(3),
            None,
            None,
            None,
            None,
            Some(2),
            Some(vec![left.block_hash.clone(), right.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let left_child = block_implicits::get_random_block(
            Some(3),
            Some(4),
            None,
            None,
            None,
            None,
            Some(3),
            Some(vec![seal.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let right_child = block_implicits::get_random_block(
            Some(3),
            Some(5),
            None,
            None,
            None,
            None,
            Some(3),
            Some(vec![seal.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );

        dag_storage
            .insert(&genesis, InsertMode::Approved)
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

        let siblings = prune_dag_covered_parents(&dag, vec![left.clone(), right.clone()])
            .expect("keep siblings");
        assert_eq!(
            siblings
                .iter()
                .map(|block| block.block_hash.clone())
                .collect::<Vec<_>>(),
            vec![left.block_hash, right.block_hash]
        );

        let diverged_from_seal =
            prune_dag_covered_parents(&dag, vec![left_child.clone(), right_child.clone(), seal])
                .expect("keep common anchor without single covering parent");
        assert_eq!(diverged_from_seal.len(), 3);
        assert_eq!(diverged_from_seal[0].block_hash, left_child.block_hash);
        assert_eq!(diverged_from_seal[1].block_hash, right_child.block_hash);
    }

    #[tokio::test]
    async fn candidate_scope_detects_rejected_deploys_without_local_buffer() {
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
        rejected.body.rejected_deploys = vec![RejectedDeploy {
            sig: prost::bytes::Bytes::from_static(b"sig"),
        }];

        block_store
            .put_block_message(&genesis)
            .expect("store genesis");
        block_store
            .put_block_message(&rejected)
            .expect("store rejected");
        dag_storage
            .insert(&genesis, InsertMode::Approved)
            .expect("insert genesis");
        dag_storage
            .insert(&rejected, InsertMode::Normal)
            .expect("insert rejected");

        let dag = dag_storage.get_representation().expect("dag");
        let parent_meta = dag
            .lookup(&rejected.block_hash)
            .expect("lookup rejected")
            .expect("rejected metadata");

        assert!(
            candidate_scope_has_rejected_deploys(&dag, &block_store, vec![parent_meta], 2, 50)
                .expect("candidate scope")
        );
    }

    #[tokio::test]
    async fn deploy_support_promotes_nonfinal_deploy_branch_over_empty_main_parent() {
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
        let deploy_block = block_implicits::get_random_block(
            Some(1),
            Some(1),
            None,
            None,
            None,
            None,
            Some(1),
            Some(vec![genesis.block_hash.clone()]),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy)]),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let deploy_support = block_implicits::get_random_block(
            Some(2),
            Some(2),
            None,
            None,
            None,
            None,
            Some(2),
            Some(vec![deploy_block.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let empty = block_implicits::get_random_block(
            Some(2),
            Some(3),
            None,
            None,
            None,
            None,
            Some(2),
            Some(vec![genesis.block_hash.clone()]),
            Some(Vec::new()),
            Some(Vec::new()),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );

        for block in [&genesis, &deploy_block, &deploy_support, &empty] {
            block_store.put_block_message(block).expect("store block");
        }
        dag_storage
            .insert(&genesis, InsertMode::Approved)
            .expect("insert genesis");
        for block in [&deploy_block, &deploy_support, &empty] {
            dag_storage
                .insert(block, InsertMode::Normal)
                .expect("insert block");
        }

        let dag = dag_storage.get_representation().expect("dag");
        let reordered = prefer_deploy_support_main_parent(
            &dag,
            &block_store,
            vec![empty.clone(), deploy_support.clone()],
            &genesis.block_hash,
        )
        .expect("prefer deploy support");

        assert_eq!(reordered[0].block_hash, deploy_support.block_hash);
        assert_eq!(reordered[1].block_hash, empty.block_hash);
    }

    #[tokio::test]
    async fn deploy_support_prefers_larger_unfinalized_deploy_branch() {
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
            None,
            Some("test".to_string()),
            None,
        );
        let heavy_a = crate::rust::util::construct_deploy::basic_deploy_data(
            1,
            None,
            Some("test".to_string()),
        )
        .expect("deploy a");
        let heavy_b = crate::rust::util::construct_deploy::basic_deploy_data(
            2,
            None,
            Some("test".to_string()),
        )
        .expect("deploy b");
        let light = crate::rust::util::construct_deploy::basic_deploy_data(
            3,
            None,
            Some("test".to_string()),
        )
        .expect("deploy c");
        let heavy_parent = block_implicits::get_random_block(
            Some(1),
            Some(1),
            None,
            None,
            None,
            None,
            Some(1),
            Some(vec![genesis.block_hash.clone()]),
            Some(Vec::new()),
            Some(vec![
                ProcessedDeploy::empty(heavy_a),
                ProcessedDeploy::empty(heavy_b),
            ]),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let light_parent = block_implicits::get_random_block(
            Some(3),
            Some(2),
            None,
            None,
            None,
            None,
            Some(3),
            Some(vec![genesis.block_hash.clone()]),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(light)]),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );

        for block in [&genesis, &heavy_parent, &light_parent] {
            block_store.put_block_message(block).expect("store block");
        }
        dag_storage
            .insert(&genesis, InsertMode::Approved)
            .expect("insert genesis");
        for block in [&heavy_parent, &light_parent] {
            dag_storage
                .insert(block, InsertMode::Normal)
                .expect("insert block");
        }

        let dag = dag_storage.get_representation().expect("dag");
        let reordered = prefer_deploy_support_main_parent(
            &dag,
            &block_store,
            vec![light_parent.clone(), heavy_parent.clone()],
            &genesis.block_hash,
        )
        .expect("prefer deploy support");

        assert_eq!(reordered[0].block_hash, heavy_parent.block_hash);
        assert_eq!(reordered[1].block_hash, light_parent.block_hash);
    }

    #[tokio::test]
    async fn deploy_support_keeps_existing_deploy_main_parent() {
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
            Some(1),
            Some(1),
            None,
            None,
            None,
            None,
            Some(1),
            Some(vec![genesis.block_hash.clone()]),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy)]),
            Some(Vec::new()),
            None,
            Some("test".to_string()),
            None,
        );
        let empty = block_implicits::get_random_block(
            Some(1),
            Some(2),
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

        for block in [&genesis, &deploy_parent, &empty] {
            block_store.put_block_message(block).expect("store block");
        }
        dag_storage
            .insert(&genesis, InsertMode::Approved)
            .expect("insert genesis");
        for block in [&deploy_parent, &empty] {
            dag_storage
                .insert(block, InsertMode::Normal)
                .expect("insert block");
        }

        let dag = dag_storage.get_representation().expect("dag");
        let reordered = prefer_deploy_support_main_parent(
            &dag,
            &block_store,
            vec![deploy_parent.clone(), empty.clone()],
            &genesis.block_hash,
        )
        .expect("prefer deploy support");

        assert_eq!(reordered[0].block_hash, deploy_parent.block_hash);
        assert_eq!(reordered[1].block_hash, empty.block_hash);
    }
}
