//! Snapshot construction — `compute_snapshot`, `get_on_chain_state`,
//! `record_dag_cardinality_metrics`, `estimator`.
//!
//! Phase 3 Step 3 — extracted from `multi_parent_casper_impl.rs`. Each
//! function takes the casper instance as a `&MultiParentCasperImpl<T>`
//! reference (rather than `&self`) so the implementation can live in this
//! module while the trait method is a one-line delegate in `traits.rs`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHash;
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

    let dag = this.block_dag_storage.get_representation()?;

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

    let mut sorted_parents_list = parent_blocks_list;
    let max_parent_block_number = sorted_parents_list
        .iter()
        .map(|b| b.body.state.block_number)
        .max()
        .unwrap_or(0);
    let near_tip_tolerance_blocks: i64 = 0;
    sorted_parents_list.sort_by(|a, b| {
        let a_num = a.body.state.block_number;
        let b_num = b.body.state.block_number;
        let a_is_near_tip =
            max_parent_block_number.saturating_sub(a_num) <= near_tip_tolerance_blocks;
        let b_is_near_tip =
            max_parent_block_number.saturating_sub(b_num) <= near_tip_tolerance_blocks;

        if a_is_near_tip && b_is_near_tip {
            a.block_hash.cmp(&b.block_hash)
        } else {
            let block_num_cmp = b_num.cmp(&a_num);
            if block_num_cmp != std::cmp::Ordering::Equal {
                block_num_cmp
            } else {
                a.block_hash.cmp(&b.block_hash)
            }
        }
    });

    let unfiltered_parents = if sorted_parents_list.is_empty() {
        vec![this.approved_block.clone()]
    } else {
        let reference_bonds = sorted_parents_list
            .iter()
            .max_by(|a, b| {
                a.body
                    .state
                    .block_number
                    .cmp(&b.body.state.block_number)
                    .then_with(|| a.block_hash.cmp(&b.block_hash))
            })
            .expect("sorted_parents_list is non-empty after is_empty() check")
            .body
            .state
            .bonds
            .clone();

        sorted_parents_list
            .into_iter()
            .filter(|block| block.body.state.bonds == reference_bonds)
            .collect()
    };

    let unfiltered_parents_count = unfiltered_parents.len();

    // C15 / Smell-3: shared wire-convention constant — see
    // `crate::rust::casper::UNLIMITED_PARENTS`.
    let mut parents_after_count_limit = unfiltered_parents;
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
    let parent_metas = dag.lookups_unsafe(parent_hashes)?;

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
        "Parent selection: {} validators, {} invalid, {} valid, {} after bond filter, {} parents",
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

        valid_latest_metas
            .iter()
            .filter(|(validator, _)| bonded_validators.contains_key(*validator))
            .map(
                |(validator, block_metadata): (
                    &Validator,
                    &models::rust::block_metadata::BlockMetadata,
                )| Justification {
                    validator: validator.clone(),
                    latest_block_hash: block_metadata.block_hash.clone(),
                },
            )
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

    let deploys_in_scope = {
        let current_dag_generation = this.block_dag_storage.current_generation();
        let snapshot_lfb_hash = dag.last_finalized_block();

        let cached: Option<Arc<dashmap::DashSet<Bytes>>> = {
            // C16: `deploys_in_scope_cache` is a `parking_lot::Mutex` —
            // no poison propagation, `.lock()` returns the guard
            // directly. The prior `std::sync::Mutex` migration's
            // poison-handling branch has been removed.
            let cache_guard = this.deploys_in_scope_cache.lock();
            cache_guard.as_ref().and_then(|(gen, cached_lfb, set)| {
                if *gen == current_dag_generation && *cached_lfb == snapshot_lfb_hash {
                    Some(set.clone())
                } else {
                    None
                }
            })
        };

        if let Some(deploys) = cached {
            deploys
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

            // C16: parking_lot::Mutex — no poison propagation.
            let mut cache_guard = this.deploys_in_scope_cache.lock();
            *cache_guard = Some((
                current_dag_generation,
                snapshot_lfb_hash,
                all_deploys.clone(),
            ));
            all_deploys
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
        .lock()
        .await
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
