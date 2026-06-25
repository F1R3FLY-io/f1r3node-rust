// See casper/src/main/scala/coop/rchain/casper/blocks/proposer/BlockCreator.scala

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, Bond, DeployData, F1r3flyState, Header, Justification, ProcessedDeploy,
    ProcessedSystemDeploy, RejectedDeploy,
};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use tracing;

use crate::rust::blocks::proposer::propose_result::BlockCreatorResult;
use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use crate::rust::util::rholang::costacc::slash_deploy::SlashDeploy;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum;
use crate::rust::util::rholang::system_deploy_user_error::SystemDeployPlatformFailure;
use crate::rust::util::rholang::{interpreter_util, system_deploy_util};
use crate::rust::util::{construct_deploy, proto_util};
use crate::rust::validator_identity::ValidatorIdentity;

/*
 * Overview of createBlock
 *
 *  1. Rank each of the block cs's latest messages (blocks) via the LMD GHOST estimator.
 *  2. Let each latest message have a score of 2^(-i) where i is the index of that latest message in the ranking.
 *     Take a subset S of the latest messages such that the sum of scores is the greatest and
 *     none of the blocks in S conflicts with each other. S will become the parents of the
 *     about-to-be-created block.
 *  3. Extract all valid deploys that aren't already in all ancestors of S (the parents).
 *  4. Create a new block that contains the deploys from the previous step.
 */
pub struct PreparedUserDeploys {
    pub deploys: HashSet<Signed<DeployData>>,
    pub effective_cap: usize,
    pub cap_hit: bool,
    /// Sigs read from the recovery buffer this round (the re-proposed losers).
    /// `create` base-checks these against the execution base before re-executing;
    /// fresh deploys never had an effect in the base and skip the check.
    pub recovered_sigs: HashSet<Bytes>,
}

fn deploy_selection_reserve_tail_enabled() -> bool { true }

pub async fn prepare_user_deploys(
    casper_snapshot: &CasperSnapshot,
    block_number: i64,
    current_time_millis: i64,
    deploy_storage: Arc<Mutex<KeyValueDeployStorage>>,
    rejected_deploy_buffer: Arc<
        Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>,
    >,
    self_validator: Option<&Bytes>,
) -> Result<PreparedUserDeploys, CasperError> {
    // The guard must not be held across the await points below (floor
    // resolution); it is re-acquired for the expired-deploy removal at the end.
    let unfinalized: HashSet<Signed<DeployData>> = {
        let deploy_storage_guard = deploy_storage
            .lock()
            .map_err(|e| CasperError::LockError(e.to_string()))?;
        deploy_storage_guard.read_all()?
    };

    // Read recovered deploys from the rejected-deploy buffer. These were dropped
    // by a prior merge's conflict resolution and are now candidates for
    // re-inclusion (fresh execution against the current merged base).
    let recovered: HashSet<Signed<DeployData>> = {
        let buffer_guard = rejected_deploy_buffer
            .lock()
            .map_err(|e| CasperError::LockError(e.to_string()))?;
        buffer_guard.read_all()?
    };

    let recovered_count = recovered.len();
    let recovered_sigs: HashSet<Bytes> = recovered.iter().map(|d| d.sig.clone()).collect();
    let unfinalized: HashSet<Signed<DeployData>> = unfinalized
        .into_iter()
        .chain(recovered.into_iter())
        .collect();
    if recovered_count > 0 {
        tracing::info!(
            "Prepare user deploys: {} recovered from rejected-deploy buffer",
            recovered_count
        );
    }

    let earliest_block_number =
        block_number - casper_snapshot.on_chain_state.shard_conf.deploy_lifespan;

    // Categorize deploys for logging
    let future_deploys: Vec<_> = unfinalized
        .iter()
        .filter(|d| !not_future_deploy(block_number, &d.data))
        .collect();
    let block_expired_deploys: Vec<_> = unfinalized
        .iter()
        .filter(|d| !not_expired_deploy(earliest_block_number, &d.data))
        .collect();
    let time_expired_deploys: Vec<_> = unfinalized
        .iter()
        .filter(|d| d.data.is_expired_at(current_time_millis))
        .collect();

    // Filter valid deploys (not expired by block, not expired by time, and not future)
    let valid: HashSet<Signed<DeployData>> = unfinalized
        .iter()
        .filter(|deploy| {
            not_future_deploy(block_number, &deploy.data)
                && not_expired_deploy(earliest_block_number, &deploy.data)
                && !deploy.data.is_expired_at(current_time_millis)
        })
        .cloned()
        .collect();

    let valid_count = valid.len();

    // Selection gates:
    //
    //  - ordinary deploys: skip when already included within the ancestry
    //    scope (the baseline duplicate guard);
    //
    //  - recovered (merge-rejected) deploys: re-proposed by their single owner.
    //    The recovery buffer holds only deploys whose latest outcome was
    //    rejection — `handle_valid_block` purges a deploy on block acceptance
    //    and `compute_parents_post_state` re-adds it when a merge rejects it —
    //    so a buffered deploy is by construction NOT in the execution base.
    //    Re-executing one lands a genuine merge loser, never an already-applied
    //    deploy, so no proposal-time liveness check is needed.
    //    `Validate::repeat_deploy` remains the consensus backstop.
    let has_recovery_candidates = valid
        .iter()
        .any(|deploy| recovered_sigs.contains(&deploy.sig));

    let mut valid_unique: HashSet<Signed<DeployData>> = HashSet::new();
    let mut already_in_scope: Vec<Signed<DeployData>> = Vec::new();
    let mut recoveries_admitted = 0usize;
    let mut recoveries_not_owned = 0usize;
    for deploy in valid {
        if recovered_sigs.contains(&deploy.sig) {
            // Single-owner recovery: only the validator that proposed this
            // deploy's indexed inclusion re-proposes it. Without this, every
            // node holding the rejected sig would re-propose it concurrently,
            // producing duplicate-conflict storms that re-reject the recovery.
            let owner = match casper_snapshot
                .dag
                .lookup_by_deploy_id(&deploy.sig.to_vec())
                .map_err(CasperError::KvStoreError)?
            {
                Some(host) => casper_snapshot
                    .dag
                    .lookup(&host)
                    .map_err(CasperError::KvStoreError)?
                    .map(|meta| meta.sender),
                None => None,
            };
            let is_owner = match (self_validator, owner.as_ref()) {
                (Some(me), Some(o)) => o == me,
                _ => false,
            };
            if !is_owner {
                recoveries_not_owned += 1;
                tracing::debug!(
                    target: "f1r3.trace.recovery_gate",
                    event = "recovery_not_owned",
                    block = block_number,
                    sig = %hex::encode(&deploy.sig[..deploy.sig.len().min(16)]),
                    owner = ?owner.as_ref().map(|o| hex::encode(&o[..o.len().min(8)])),
                    self_validator = ?self_validator.map(|me| hex::encode(&me[..me.len().min(8)])),
                    "recovered loser NOT re-proposed this round (not owner / owner unknown)"
                );
                continue;
            }
            recoveries_admitted += 1;
            tracing::debug!(
                target: "f1r3.trace.recovery_gate",
                event = "recovery_admitted",
                block = block_number,
                sig = %hex::encode(&deploy.sig[..deploy.sig.len().min(16)]),
                owner = ?owner.as_ref().map(|o| hex::encode(&o[..o.len().min(8)])),
                "recovered loser admitted for re-execution (owned, not in base)"
            );
            valid_unique.insert(deploy);
        } else if casper_snapshot.deploys_in_scope.contains(&deploy.sig) {
            already_in_scope.push(deploy);
        } else {
            valid_unique.insert(deploy);
        }
    }

    if has_recovery_candidates {
        tracing::info!(
            "Prepare user deploys: recovery gate: admitted={}, not-owned={}",
            recoveries_admitted,
            recoveries_not_owned
        );
    }

    let already_in_scope_count = already_in_scope.len();

    // Log deploy selection details when there are any deploys in the pool
    if !unfinalized.is_empty() || !casper_snapshot.deploys_in_scope.is_empty() {
        tracing::info!(
            "Deploy selection for block #{}: pool={}, future={} (validAfterBlockNumber >= {}), \
             blockExpired={} (validAfterBlockNumber <= {}), timeExpired={} (expirationTimestamp <= {}), \
             valid={}, alreadyInScope={}, selected={}",
            block_number,
            unfinalized.len(),
            future_deploys.len(),
            block_number,
            block_expired_deploys.len(),
            earliest_block_number,
            time_expired_deploys.len(),
            current_time_millis,
            valid_count,
            already_in_scope_count,
            valid_unique.len()
        );
    }

    // Log details for filtered-out deploys (to help debug why deploys aren't included)
    for d in &future_deploys {
        tracing::warn!(
            "Deploy {}... FILTERED (future): validAfterBlockNumber={} >= currentBlock={}",
            hex::encode(&d.sig[..std::cmp::min(8, d.sig.len())]),
            d.data.valid_after_block_number,
            block_number
        );
    }
    for d in &block_expired_deploys {
        tracing::warn!(
            "Deploy {}... FILTERED (block-expired): validAfterBlockNumber={} <= earliestBlock={}",
            hex::encode(&d.sig[..std::cmp::min(8, d.sig.len())]),
            d.data.valid_after_block_number,
            earliest_block_number
        );
    }
    for d in &time_expired_deploys {
        tracing::warn!(
            "Deploy {}... FILTERED (time-expired): expirationTimestamp={:?} <= currentTime={}",
            hex::encode(&d.sig[..std::cmp::min(8, d.sig.len())]),
            d.data.expiration_timestamp,
            current_time_millis
        );
    }
    for d in &already_in_scope {
        tracing::warn!(
            "Deploy {}... FILTERED (already in scope): deploy already exists in DAG within lifespan window",
            hex::encode(&d.sig[..std::cmp::min(8, d.sig.len())])
        );
    }

    // Remove all expired deploys from storage to prevent them from triggering future proposals
    // Combine block-expired and time-expired, avoiding duplicates
    let all_expired: HashSet<&Signed<DeployData>> = block_expired_deploys
        .iter()
        .chain(time_expired_deploys.iter())
        .cloned()
        .collect();
    if !all_expired.is_empty() {
        tracing::info!(
            "Removing {} expired deploy(s) from storage and rejected-deploy buffer",
            all_expired.len()
        );
        let expired_list: Vec<Signed<DeployData>> = all_expired.into_iter().cloned().collect();
        {
            let mut deploy_storage_guard = deploy_storage
                .lock()
                .map_err(|e| CasperError::LockError(e.to_string()))?;
            deploy_storage_guard.remove(expired_list.clone())?;
        }

        // Also purge expired sigs from the rejected-deploy buffer.
        // Reads above already filter expired sigs out of `valid_unique`, so
        // they don't get re-proposed, but on-disk LMDB entries persist
        // unless explicitly removed. Without this, a sustained-load
        // adversary that keeps generating conflicts can grow the buffer
        // unbounded.
        let mut buffer_guard = rejected_deploy_buffer
            .lock()
            .map_err(|e| CasperError::LockError(e.to_string()))?;
        buffer_guard.remove(expired_list)?;
    }

    let max_deploys = casper_snapshot
        .on_chain_state
        .shard_conf
        .max_user_deploys_per_block as usize;
    let max_user_deploys = max_deploys;
    if valid_unique.len() <= max_user_deploys {
        tracing::debug!(
            target: "f1r3.trace.recovery_gate",
            event = "block_selected",
            block = block_number,
            cap_hit = false,
            n_selected = valid_unique.len(),
            selected = ?valid_unique.iter().map(|d| hex::encode(&d.sig[..d.sig.len().min(16)])).collect::<Vec<_>>(),
            "deploys selected for this block (uncapped)"
        );
        return Ok(PreparedUserDeploys {
            deploys: valid_unique,
            effective_cap: max_user_deploys,
            cap_hit: false,
            recovered_sigs: recovered_sigs.clone(),
        });
    }

    // Deterministically order deploys by age so selection remains stable across validators.
    let mut ordered: Vec<Signed<DeployData>> = valid_unique.into_iter().collect();
    ordered.sort_by(|a, b| {
        a.data
            .valid_after_block_number
            .cmp(&b.data.valid_after_block_number)
            .then_with(|| a.data.time_stamp.cmp(&b.data.time_stamp))
            .then_with(|| {
                // Stable deterministic tie-breaker for identical timestamps/windows.
                a.sig.cmp(&b.sig)
            })
    });

    // To avoid head-of-line blocking after stress bursts, reserve one slot for
    // the freshest deploy when capping is active. The remaining slots still drain
    // oldest deploys first to preserve fairness.
    let (selected, selection_strategy): (HashSet<Signed<DeployData>>, &'static str) =
        if deploy_selection_reserve_tail_enabled() {
            if max_user_deploys == 1 {
                (
                    ordered.iter().last().cloned().into_iter().collect(),
                    "newest-only",
                )
            } else {
                let oldest_take = max_user_deploys.saturating_sub(1);
                let mut picked: HashSet<Signed<DeployData>> =
                    ordered.iter().take(oldest_take).cloned().collect();
                if let Some(newest) = ordered.iter().last().cloned() {
                    picked.insert(newest);
                }
                if max_user_deploys <= ordered.len() {
                    debug_assert_eq!(picked.len(), max_user_deploys);
                }
                (picked, "oldest-plus-newest")
            }
        } else {
            (
                ordered.iter().take(max_user_deploys).cloned().collect(),
                "oldest-only",
            )
        };
    let deferred = valid_count
        .saturating_sub(already_in_scope_count)
        .saturating_sub(selected.len());

    tracing::info!(
        "Deploy selection capped for block #{}: selected={}, deferred={}, cap={}, strategy={}",
        block_number,
        selected.len(),
        deferred,
        max_user_deploys,
        selection_strategy
    );

    tracing::debug!(
        target: "f1r3.trace.recovery_gate",
        event = "block_selected",
        block = block_number,
        cap_hit = true,
        n_selected = selected.len(),
        selected = ?selected.iter().map(|d| hex::encode(&d.sig[..d.sig.len().min(16)])).collect::<Vec<_>>(),
        "deploys selected for this block (capped)"
    );
    Ok(PreparedUserDeploys {
        deploys: selected,
        effective_cap: max_user_deploys,
        cap_hit: true,
        recovered_sigs,
    })
}

fn collect_self_chain_deploy_sigs(
    casper_snapshot: &CasperSnapshot,
    validator_identity: &ValidatorIdentity,
    block_store: &KeyValueBlockStore,
) -> Result<HashSet<Bytes>, CasperError> {
    let self_validator = validator_identity.public_key.bytes.clone();
    let current_hash_from_justifications = casper_snapshot
        .justifications
        .iter()
        .find(|j| j.validator == self_validator)
        .map(|j| j.latest_block_hash.clone());
    let current_hash_from_dag = casper_snapshot.dag.latest_message_hash(&self_validator);

    let Some(mut current_hash) = current_hash_from_justifications.or(current_hash_from_dag) else {
        return Ok(HashSet::new());
    };

    let mut deploy_sigs: HashSet<Bytes> = HashSet::new();
    let max_depth = std::cmp::max(casper_snapshot.on_chain_state.shard_conf.deploy_lifespan, 1);

    for _ in 0..(max_depth as usize) {
        let Some(block) = block_store.get(&current_hash)? else {
            break;
        };

        for processed in &block.body.deploys {
            deploy_sigs.insert(processed.deploy.sig.clone());
        }

        let Some(main_parent) = block.header.parents_hash_list.first().cloned() else {
            break;
        };
        current_hash = main_parent;
    }

    Ok(deploy_sigs)
}

/// Pure-function filter extracted for unit testing. Keeps an
/// invalid-latest-message entry only if the equivocator is still
/// slashable in the parent post-state — i.e., bonded with positive
/// stake AND in the PoS active-validator set. The active-validator
/// check matters when bond floor > 0: a validator slashed in a parent
/// retains stake at the floor, satisfying the bonded check, but PoS
/// has removed them from active_validators so they shouldn't be
/// re-slashed. Without this, the proposer emits a redundant SlashDeploy
/// every block until the equivocator's invalid latest message ages
/// out of the DAG view, saved by PoS slash idempotency but inflating
/// body and wasting execution.
fn filter_slashable_invalid_messages(
    invalid_latest_messages: HashMap<Validator, BlockHash>,
    bonds_map: &HashMap<Validator, i64>,
    active_validators: &[Validator],
) -> Vec<(Validator, BlockHash)> {
    invalid_latest_messages
        .into_iter()
        .filter(|(validator, _)| {
            bonds_map.get(validator).copied().unwrap_or(0) > 0
                && active_validators.contains(validator)
        })
        .collect()
}

async fn prepare_slashing_deploys(
    casper_snapshot: &CasperSnapshot,
    validator_identity: &ValidatorIdentity,
    seq_num: i32,
) -> Result<Vec<SlashDeploy>, CasperError> {
    let self_id = Bytes::copy_from_slice(&validator_identity.public_key.bytes);

    let invalid_latest_messages = casper_snapshot.dag.invalid_latest_messages()?;
    let slashable_invalid_messages = filter_slashable_invalid_messages(
        invalid_latest_messages,
        &casper_snapshot.on_chain_state.bonds_map,
        &casper_snapshot.on_chain_state.active_validators,
    );

    let mut slashing_deploys = Vec::new();
    for (_, invalid_block_hash) in slashable_invalid_messages {
        let slash_deploy = SlashDeploy {
            invalid_block_hash: invalid_block_hash.clone(),
            pk: validator_identity.public_key.clone(),
            initial_rand: system_deploy_util::generate_slash_deploy_random_seed(
                self_id.clone(),
                seq_num,
                &invalid_block_hash,
            ),
        };

        tracing::info!(
            "Issuing slashing deploy justified by block {}",
            pretty_printer::PrettyPrinter::build_string_bytes(&invalid_block_hash)
        );

        slashing_deploys.push(slash_deploy);
    }

    Ok(slashing_deploys)
}

fn prepare_dummy_deploy(
    block_number: i64,
    shard_id: String,
    dummy_deploy_opt: Option<(PrivateKey, String)>,
) -> Result<Vec<Signed<DeployData>>, CasperError> {
    match dummy_deploy_opt {
        Some((private_key, term)) => {
            let deploy = construct_deploy::source_deploy_now(
                term,
                Some(private_key),
                Some(block_number - 1),
                Some(shard_id),
            )
            .map_err(|e| {
                CasperError::RuntimeError(format!("Failed to create dummy deploy: {}", e))
            })?;
            Ok(vec![deploy])
        }
        None => Ok(Vec::new()),
    }
}

fn extract_deploy_sig_from_refund_failure(msg: &str) -> Option<Vec<u8>> {
    let marker = "deploy_sig=";
    let start = msg.find(marker)? + marker.len();
    let tail = &msg[start..];
    let end = tail.find(',').unwrap_or(tail.len());
    let sig_hex = tail[..end].trim();
    hex::decode(sig_hex).ok()
}

fn quarantine_refund_failure_deploy(
    deploy_storage: Arc<Mutex<KeyValueDeployStorage>>,
    failure_msg: &str,
) -> Result<bool, CasperError> {
    let Some(sig) = extract_deploy_sig_from_refund_failure(failure_msg) else {
        return Ok(false);
    };

    let mut guard = deploy_storage
        .lock()
        .map_err(|e| CasperError::LockError(e.to_string()))?;
    guard.remove_by_sig(&sig).map_err(CasperError::KvStoreError)
}

pub async fn create(
    casper_snapshot: &CasperSnapshot,
    validator_identity: &ValidatorIdentity,
    dummy_deploy_opt: Option<(PrivateKey, String)>,
    deploy_storage: Arc<Mutex<KeyValueDeployStorage>>,
    rejected_deploy_buffer: Arc<Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>,
    runtime_manager: &RuntimeManager,
    block_store: &mut KeyValueBlockStore,
    allow_empty_blocks: bool,
) -> Result<BlockCreatorResult, CasperError> {
    use crate::rust::metrics_constants::{
        BLOCK_CREATOR_COMPUTE_DEPLOYS_CHECKPOINT_TIME_METRIC,
        BLOCK_CREATOR_COMPUTE_PARENTS_POST_STATE_TIME_METRIC,
        BLOCK_CREATOR_PACKAGE_BLOCK_TIME_METRIC, BLOCK_CREATOR_PREPARE_USER_DEPLOYS_TIME_METRIC,
        BLOCK_CREATOR_TOTAL_TIME_METRIC, CASPER_METRICS_SOURCE,
    };
    let create_started = std::time::Instant::now();
    // Capture current time once to ensure consistency between deploy filtering and block timestamp.
    // This prevents race condition where a deploy could pass filtering but expire before block creation.
    let now_u128 = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|e| CasperError::RuntimeError(format!("Failed to get current time: {}", e)))?
        .as_millis();
    let mut now_millis = i64::try_from(now_u128).map_err(|_| {
        CasperError::RuntimeError(format!(
            "Current timestamp millis {} exceeds i64::MAX",
            now_u128
        ))
    })?;

    let next_seq_num = casper_snapshot
        .max_seq_nums
        .get(&validator_identity.public_key.bytes)
        .map(|seq| *seq + 1)
        .unwrap_or(1) as i32;
    let next_block_num = casper_snapshot.max_block_num + 1;
    let parents = &casper_snapshot.parents;
    let justifications = &casper_snapshot.justifications;
    if let Some(max_parent_ts) = parents.iter().map(|p| p.header.timestamp).max() {
        if now_millis < max_parent_ts {
            tracing::debug!(
                "Adjusting block timestamp from {} to parent timestamp {} to avoid clock-skew regressions",
                now_millis,
                max_parent_ts
            );
            now_millis = max_parent_ts;
        }
    }

    tracing::info!(
        "Creating block #{} (seqNum {})",
        next_block_num,
        next_seq_num
    );

    let shard_id = casper_snapshot.on_chain_state.shard_conf.shard_name.clone();

    // Prepare deploys
    let (user_deploys, recovered_sigs) = {
        let t = std::time::Instant::now();
        let prepared = prepare_user_deploys(
            casper_snapshot,
            next_block_num,
            now_millis,
            deploy_storage.clone(),
            rejected_deploy_buffer.clone(),
            Some(&validator_identity.public_key.bytes),
        )
        .await?;
        let mut v = prepared.deploys;
        let self_chain_deploy_sigs =
            collect_self_chain_deploy_sigs(casper_snapshot, validator_identity, block_store)?;
        if !self_chain_deploy_sigs.is_empty() {
            let before = v.len();
            // A sig in the proposer's self-chain is normally a duplicate and
            // must be filtered out. The exception is a sig in
            // `rejected_in_scope`: the merge engine conflict-rejected it, so
            // its effects never landed in canonical state and re-proposing
            // it is correct. Mirror the same exemption that
            // `prepare_user_deploys` applies upstream.
            v.retain(|deploy| {
                !self_chain_deploy_sigs.contains(&deploy.sig)
                    || casper_snapshot.rejected_in_scope.contains(&deploy.sig)
            });
            let skipped = before.saturating_sub(v.len());
            if skipped > 0 {
                tracing::info!(
                    "Filtered {} deploy(s) already present in self latest-message chain",
                    skipped
                );
            }
        }
        tracing::debug!(
            target: "f1r3fly.block_creator.timing",
            "prepare_user_deploys_ms={}, user_deploys_count={}, user_deploy_cap={}, user_deploy_cap_hit={}",
            t.elapsed().as_millis(),
            v.len(),
            prepared.effective_cap,
            prepared.cap_hit
        );
        metrics::histogram!(BLOCK_CREATOR_PREPARE_USER_DEPLOYS_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(t.elapsed().as_secs_f64());
        (v, prepared.recovered_sigs)
    };
    let dummy_deploys = {
        let t = std::time::Instant::now();
        let v = prepare_dummy_deploy(next_block_num, shard_id.clone(), dummy_deploy_opt)?;
        tracing::debug!(
            target: "f1r3fly.block_creator.timing",
            "prepare_dummy_deploys_ms={}, dummy_deploys_count={}",
            t.elapsed().as_millis(),
            v.len()
        );
        v
    };
    let slashing_deploys = {
        let t = std::time::Instant::now();
        let v = prepare_slashing_deploys(casper_snapshot, validator_identity, next_seq_num).await?;
        tracing::debug!(
            target: "f1r3fly.block_creator.timing",
            "prepare_slashing_deploys_ms={}, slashing_deploys_count={}",
            t.elapsed().as_millis(),
            v.len()
        );
        v
    };

    // Combine all deploys. prepare_user_deploys already removed deploys in scope.
    let mut all_deploys: HashSet<Signed<DeployData>> = user_deploys;

    // Add dummy deploys
    all_deploys.extend(dummy_deploys);

    // Merge the parents once up front. Two reasons to do this before the
    // empty-block skip check below:
    //   1. To discover slashes that were rejected by cost-optimal merge
    //      resolution — those slashes must be re-issued by this proposer
    //      so the slash effect lands in the merge block regardless of the
    //      merge's rejection decision.
    //   2. To include rejected-slash recovery in the "do we have work?"
    //      decision. A heartbeat-disabled proposer that wakes with no user
    //      deploys and no own-detected slashes would otherwise skip,
    //      stranding any merge-rejected slashes from parent merging.
    // The merge result is cached so the downstream compute_deploys_checkpoint
    // call hits the cache.
    let __merge_pre_t = std::time::Instant::now();
    // Floor snapshot = the proposer's justifications — exactly the set
    // packaged into this block's header, so the floor the proposer builds on
    // equals the floor every validator re-derives from the signed block.
    let latest_messages: std::collections::BTreeMap<Validator, BlockHash> = casper_snapshot
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let merge_pre_info = interpreter_util::compute_parents_post_state(
        block_store,
        parents.clone(),
        casper_snapshot,
        runtime_manager,
        &latest_messages,
        None,
        Some(&rejected_deploy_buffer),
    )
    .await?;
    metrics::histogram!(
        BLOCK_CREATOR_COMPUTE_PARENTS_POST_STATE_TIME_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .record(__merge_pre_t.elapsed().as_secs_f64());
    let (pre_state, _rejected_user_sigs, rejected_slashes) = merge_pre_info;

    // Base-check: drop any recovered deploy whose effect is already in the
    // execution base. A merge can KEEP a deploy by absorbing its effect from a
    // parent's body — without that deploy appearing in the merge's own
    // body.deploys, so the recovery buffer is never purged of it — and a
    // fast-pathed block then builds directly on such a base. Re-executing it
    // would re-create the deploy's per-deploy number cells (content-twin) and
    // re-charge its vault. Reading the actual pre-state catches this where the
    // buffer-membership and ancestry paper-trail are blind, for both merged and
    // fast-pathed bases. On a check error we skip (conservative: never risk the
    // twin); the deploy stays buffered and is retried.
    if !recovered_sigs.is_empty() {
        let base_state = Blake2b256Hash::from_bytes_prost(&pre_state);
        let block_store_ref: &KeyValueBlockStore = &*block_store;
        let before = all_deploys.len();
        let mut skipped: usize = 0;
        all_deploys.retain(|deploy| {
            if !recovered_sigs.contains(&deploy.sig) {
                return true;
            }
            match interpreter_util::recovered_deploy_effect_in_base(
                &casper_snapshot.dag,
                block_store_ref,
                runtime_manager,
                &base_state,
                &deploy.sig,
            ) {
                Ok(false) => true,
                Ok(true) => {
                    skipped += 1;
                    false
                }
                Err(err) => {
                    tracing::warn!(
                        "Recovery base-check failed for {}: {} — skipping re-execution to avoid content-twin",
                        hex::encode(&deploy.sig[..std::cmp::min(8, deploy.sig.len())]),
                        err
                    );
                    skipped += 1;
                    false
                }
            }
        });
        if skipped > 0 {
            tracing::info!(
                "Recovery base-check: skipped {} of {} candidate deploy(s) already applied in the base",
                skipped,
                before
            );
        }
    }

    // Union own slashes with merge-rejected slashes, dedup by
    // `invalid_block_hash`. Own detections take priority — any
    // merge-rejected slash for an equivocator already covered by
    // prepare_slashing_deploys is dropped. `filter_recoverable` also
    // collapses multiple rejected slashes for the same equivocator
    // (e.g., from different original issuers) down to a single entry.
    let own_invalid_block_hashes = slashing_deploys
        .iter()
        .map(|sd| sd.invalid_block_hash.clone());
    let recovered_rejected_slashes = crate::rust::merging::rejected_slash::filter_recoverable(
        rejected_slashes,
        own_invalid_block_hashes,
    );

    // Check if we have any new work to process.
    // If empty blocks are disabled, skip closeBlock-only proposals to avoid no-op checkpoint cost.
    // If empty blocks are enabled (heartbeat/liveness mode), continue and emit closeBlock.
    // Recovered rejected slashes count as work — without this check, a
    // heartbeat-disabled proposer would silently drop merge-rejected slashes
    // on a wake with no other pending work.
    let has_slashing_deploys = !slashing_deploys.is_empty();
    let has_recovered_rejected_slashes = !recovered_rejected_slashes.is_empty();
    if all_deploys.is_empty()
        && !has_slashing_deploys
        && !has_recovered_rejected_slashes
        && !allow_empty_blocks
    {
        tracing::info!(
            "Skipping empty block creation: no new user deploys, no slashing deploys, no merge-rejected slashes to recover"
        );
        return Ok(BlockCreatorResult::NoNewDeploys);
    }

    // Make sure closeBlock is the last system Deploy
    let mut system_deploys_converted: Vec<SystemDeployEnum> = Vec::new();

    // Add own-detected slashes
    for slash_deploy in slashing_deploys {
        system_deploys_converted.push(SystemDeployEnum::Slash(slash_deploy));
    }

    // Re-issue slashes that the merge dropped. The proposer signs these
    // under its own identity, matching the existing slashing convention.
    let self_id = Bytes::copy_from_slice(&validator_identity.public_key.bytes);
    for rs in &recovered_rejected_slashes {
        let slash_deploy = SlashDeploy {
            invalid_block_hash: rs.invalid_block_hash.clone(),
            pk: validator_identity.public_key.clone(),
            initial_rand: system_deploy_util::generate_slash_deploy_random_seed(
                self_id.clone(),
                next_seq_num,
                &rs.invalid_block_hash,
            ),
        };
        tracing::info!(
            "Recovering merge-rejected slash: invalid_block={}, original_issuer={}",
            pretty_printer::PrettyPrinter::build_string_bytes(&rs.invalid_block_hash),
            hex::encode(&rs.issuer_public_key.bytes)
        );
        system_deploys_converted.push(SystemDeployEnum::Slash(slash_deploy));
    }

    // Add the actual close block deploy
    system_deploys_converted.push(SystemDeployEnum::Close(CloseBlockDeploy {
        initial_rand: system_deploy_util::generate_close_deploy_random_seed_from_pk(
            validator_identity.public_key.clone(),
            next_seq_num,
        ),
    }));

    // Use the adjusted `now_millis` captured at the start of create for block timestamp.
    // The value is clamped to the max parent timestamp to avoid InvalidTimestamp from clock skew.
    // This ensures the same time is used for deploy filtering and block creation.
    let invalid_blocks = casper_snapshot.invalid_blocks.clone();
    let block_data = BlockData {
        time_stamp: now_millis,
        block_number: next_block_num,
        sender: validator_identity.public_key.clone(),
        seq_num: next_seq_num,
    };

    // Compute checkpoint data
    let checkpoint_started = std::time::Instant::now();
    let checkpoint_data = match interpreter_util::compute_deploys_checkpoint(
        block_store,
        parents.clone(),
        all_deploys.into_iter().collect(),
        system_deploys_converted,
        casper_snapshot,
        runtime_manager,
        block_data.clone(),
        invalid_blocks,
        &latest_messages,
        Some(&rejected_deploy_buffer),
    )
    .await
    {
        Ok(data) => data,
        Err(CasperError::SystemRuntimeError(SystemDeployPlatformFailure::GasRefundFailure(
            msg,
        ))) => {
            let removed = quarantine_refund_failure_deploy(deploy_storage.clone(), &msg)?;
            tracing::warn!(
                "Gas refund failure during checkpoint; quarantined_toxic_deploy={} error={}",
                removed,
                msg
            );
            return Ok(BlockCreatorResult::NoNewDeploys);
        }
        Err(err) => return Err(err),
    };
    tracing::debug!(
        target: "f1r3fly.block_creator.timing",
        "compute_deploys_checkpoint_ms={}",
        checkpoint_started.elapsed().as_millis()
    );
    metrics::histogram!(
        BLOCK_CREATOR_COMPUTE_DEPLOYS_CHECKPOINT_TIME_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .record(checkpoint_started.elapsed().as_secs_f64());

    let (
        pre_state_hash,
        post_state_hash,
        processed_deploys,
        rejected_deploys,
        processed_system_deploys,
        new_bonds,
    ) = checkpoint_data;

    // Finality = FS: the block's bonds FIELD carries the finalized-floor bonds
    // (the sealed cut this block builds on), not the speculative post-state
    // bonds. This mirrors RChain's `BlockCreator: bondsMap = fringeBondsMap`:
    // consensus weight (clique oracle) and the justification bonded-set derive
    // from this field, so it must reflect sealed truth. `new_bonds` (the
    // post-state PoS read) stays the execution result inside post_state_hash;
    // only the consensus-facing field moves to FS.
    let floor_bonds = {
        let ft = casper_snapshot
            .on_chain_state
            .shard_conf
            .fault_tolerance_threshold;
        let parent_hashes: Vec<_> = parents.iter().map(|p| p.block_hash.clone()).collect();
        let latest_messages: std::collections::BTreeMap<_, _> = justifications
            .iter()
            .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
            .collect();
        let floor = crate::rust::finality::floor::finalized_floor(
            &casper_snapshot.dag,
            &parent_hashes,
            &latest_messages,
            ft,
        )
        .await?;
        let fs = crate::rust::finality::floor_seal::floor_state_get_or_compute(
            &casper_snapshot.dag,
            block_store,
            runtime_manager,
            &floor.hash,
            ft,
        )
        .await?;
        let fs_bonds = runtime_manager.compute_bonds(&fs.state_hash.0).await?;
        // The bonds FIELD is the consensus committee, not the full economic bonded set.
        // Every consensus reader keys off it — the clique oracle's weight map, fork-choice
        // weighting, the `justification_follows` bonded-set, and the proposer/synchrony
        // gates. So it must be the validators that actually validate this epoch:
        // `activeValidators` (the capped, epoch-rotated committee) intersected with the
        // finalized-floor bonds. The intersection drops both bonded-not-active validators
        // (capped out or not yet activated) and active-not-bonded validators (e.g. a
        // withdrawing validator still in the frozen active set). Economic state lives in
        // PoS `allBonds`, read separately. At numberOfActiveValidators=10000 the committee
        // is every bonded validator, so this is a no-op today; it becomes the real cap when
        // N < bonded count (Phase 2 / Issue J).
        let active = runtime_manager
            .get_active_validators(&fs.state_hash.0)
            .await?;
        let committee: Vec<Bond> = fs_bonds
            .into_iter()
            .filter(|b| active.contains(&b.validator))
            .collect();
        if committee.len() != new_bonds.len() {
            tracing::info!(
                target: "f1r3fly.casper.bonds_validation",
                floor_number = floor.block_number,
                committee = committee.len(),
                post_state_bonds = new_bonds.len(),
                "block bonds field = active committee ∩ FS(floor) bonds; differs from post-state bonds (finality delay / inactive bonded)"
            );
        }
        committee
    };

    let casper_version = casper_snapshot.on_chain_state.shard_conf.casper_version;

    // Span[F].trace(ProcessDeploysAndCreateBlockMetricsSource) from Scala
    let _span =
        tracing::info_span!(target: "f1r3fly.casper.create_block", "process-deploys-and-create-block")
            .entered();

    tracing::event!(tracing::Level::DEBUG, mark = "before-packing-block");
    // Create unsigned block
    let package_started = std::time::Instant::now();
    let pre_state_hash_for_result = pre_state_hash.clone();
    let post_state_hash_for_result = post_state_hash.clone();
    let unsigned_block = package_block(
        &block_data,
        parents.iter().map(|p| p.block_hash.clone()).collect(),
        justifications.iter().map(|j| j.clone()).collect(),
        pre_state_hash,
        post_state_hash,
        processed_deploys,
        rejected_deploys,
        processed_system_deploys,
        floor_bonds,
        shard_id,
        casper_version,
    );
    let package_ms = package_started.elapsed().as_millis();
    metrics::histogram!(
        BLOCK_CREATOR_PACKAGE_BLOCK_TIME_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .record(package_started.elapsed().as_secs_f64());

    tracing::event!(tracing::Level::DEBUG, mark = "block-created");
    // Sign the block
    let sign_started = std::time::Instant::now();
    let signed_block = validator_identity.sign_block(&unsigned_block);
    let sign_ms = sign_started.elapsed().as_millis();

    tracing::event!(tracing::Level::DEBUG, mark = "block-signed");

    let block_info = pretty_printer::PrettyPrinter::build_string_block_message(&signed_block, true);
    let deploy_count = signed_block.body.deploys.len();
    tracing::debug!("Block created: {} ({}d)", block_info, deploy_count);
    let total_create_block_ms = create_started.elapsed().as_millis();

    tracing::debug!(
        target: "f1r3fly.block_creator.timing",
        "Block creator timing: package_ms={}, sign_ms={}, total_create_block_ms={}",
        package_ms,
        sign_ms,
        total_create_block_ms
    );
    metrics::histogram!(
        BLOCK_CREATOR_TOTAL_TIME_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .record(create_started.elapsed().as_secs_f64());

    RuntimeManager::trim_allocator();

    Ok(BlockCreatorResult::Created(
        signed_block,
        pre_state_hash_for_result,
        post_state_hash_for_result,
    ))
}

fn package_block(
    block_data: &BlockData,
    parents: Vec<Bytes>,
    justifications: Vec<Justification>,
    pre_state_hash: Bytes,
    post_state_hash: Bytes,
    deploys: Vec<ProcessedDeploy>,
    rejected_deploys: Vec<(Bytes, BlockHash)>,
    system_deploys: Vec<ProcessedSystemDeploy>,
    bonds_map: Vec<Bond>,
    shard_id: String,
    version: i64,
) -> BlockMessage {
    let state = F1r3flyState {
        pre_state_hash,
        post_state_hash,
        bonds: bonds_map,
        block_number: block_data.block_number,
    };

    let rejected_deploys_wrapped: Vec<RejectedDeploy> = rejected_deploys
        .into_iter()
        .map(|(sig, host)| RejectedDeploy { sig, host })
        .collect();

    let body = Body {
        state,
        deploys,
        rejected_deploys: rejected_deploys_wrapped,
        system_deploys,
        extra_bytes: Bytes::new(),
    };

    let header = Header {
        parents_hash_list: parents,
        timestamp: block_data.time_stamp,
        version,
        extra_bytes: Bytes::new(),
    };

    proto_util::unsigned_block_proto(
        body,
        header,
        justifications,
        shard_id,
        Some(block_data.seq_num),
    )
}

fn not_expired_deploy(earliest_block_number: i64, deploy_data: &DeployData) -> bool {
    deploy_data.valid_after_block_number > earliest_block_number
}

fn not_future_deploy(current_block_number: i64, deploy_data: &DeployData) -> bool {
    deploy_data.valid_after_block_number < current_block_number
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validator(byte: u8) -> Validator { Bytes::from(vec![byte; 32]) }

    fn invalid_block_hash(byte: u8) -> BlockHash { Bytes::from(vec![byte; 32]) }

    /// A bonded validator that PoS still considers active is slashable
    /// when their latest message is invalid. Baseline behavior.
    #[test]
    fn bonded_active_equivocator_is_slashable() {
        let equivocator = validator(0xAA);
        let invalid_block = invalid_block_hash(0x11);

        let mut invalid_latest_messages = HashMap::new();
        invalid_latest_messages.insert(equivocator.clone(), invalid_block.clone());

        let mut bonds_map = HashMap::new();
        bonds_map.insert(equivocator.clone(), 5);

        let active_validators = vec![equivocator.clone()];

        let out = filter_slashable_invalid_messages(
            invalid_latest_messages,
            &bonds_map,
            &active_validators,
        );

        assert_eq!(out.len(), 1, "bonded active equivocator must be slashable");
        assert_eq!(out[0].0, equivocator);
        assert_eq!(out[0].1, invalid_block);
    }

    /// An equivocator with stake 0 is excluded by the bonded check,
    /// regardless of active-validator membership. Existing behavior.
    #[test]
    fn unbonded_equivocator_filtered_out() {
        let equivocator = validator(0xBB);
        let invalid_block = invalid_block_hash(0x22);

        let mut invalid_latest_messages = HashMap::new();
        invalid_latest_messages.insert(equivocator.clone(), invalid_block);

        let mut bonds_map = HashMap::new();
        bonds_map.insert(equivocator.clone(), 0);

        let active_validators = vec![equivocator];

        let out = filter_slashable_invalid_messages(
            invalid_latest_messages,
            &bonds_map,
            &active_validators,
        );

        assert!(out.is_empty(), "stake-0 equivocator must not be slashable");
    }

    /// An equivocator already slashed in a parent block retains stake
    /// at the bond floor (e.g., 1 in production), satisfying the
    /// stake > 0 check, but PoS removes them from active_validators.
    /// The active-validator filter is what stops the proposer from
    /// emitting redundant SlashDeploys block after block.
    #[test]
    fn bonded_but_already_slashed_equivocator_filtered_out() {
        let equivocator = validator(0xCC);
        let invalid_block = invalid_block_hash(0x33);

        let mut invalid_latest_messages = HashMap::new();
        invalid_latest_messages.insert(equivocator.clone(), invalid_block);

        // Bond floor > 0 — equivocator's stake stays at 1 after slash.
        let mut bonds_map = HashMap::new();
        bonds_map.insert(equivocator.clone(), 1);

        // PoS has removed the slashed validator from the active set.
        let active_validators: Vec<Validator> = vec![];

        let out = filter_slashable_invalid_messages(
            invalid_latest_messages,
            &bonds_map,
            &active_validators,
        );

        assert!(
            out.is_empty(),
            "already-slashed equivocator (not in active_validators) must not be \
             re-slashed even when bond floor > 0 keeps their stake nonzero. If this \
             fires, prepare_slashing_deploys will emit redundant SlashDeploys every \
             block until the invalid latest message ages out of the DAG view."
        );
    }
}
