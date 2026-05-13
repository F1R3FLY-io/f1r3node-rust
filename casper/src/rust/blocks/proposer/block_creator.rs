// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// See casper/src/main/scala/coop/rchain/casper/blocks/proposer/BlockCreator.scala

use std::collections::HashSet;
use std::sync::Arc;
use std::time::SystemTime;

use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::signed::Signed;
use models::rust::casper::pretty_printer;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, Bond, DeployData, F1r3flyState, Header, Justification, ProcessedDeploy,
    ProcessedSystemDeploy, RejectedDeploy,
};
use prost::bytes::Bytes;
use rholang::rust::interpreter::system_processes::BlockData;
use tracing;

use crate::rust::blocks::proposer::propose_result::BlockCreatorResult;
use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;
use crate::rust::slashing_authorization::{authorized_slash_candidates, checked_next_seq};
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
struct PreparedUserDeploys {
    deploys: HashSet<Signed<DeployData>>,
    effective_cap: usize,
    cap_hit: bool,
}

/// C15 / Smell-2: was previously a zero-arg `fn -> bool` returning a
/// hard-coded `true`. Promoted to a `const` so its always-on nature
/// is explicit and the value is folded at compile time. Kept as a
/// named constant (rather than inlined `true`) because it is a
/// feature-flag posture that may yet be moved into `CasperShardConf`
/// for per-shard control — when that happens the rename target is
/// already in place.
const DEPLOY_SELECTION_RESERVE_TAIL_ENABLED: bool = true;

/// C15 / Smell-4: extract the deploy-signature pretty-print prefix
/// used in operator-facing log messages. Previously inlined as
/// `deploy_sig_prefix(&d.sig)` at four
/// sites in `log_deploy_pool_filtering`.
fn deploy_sig_prefix(sig: &Bytes) -> String { hex::encode(&sig[..std::cmp::min(8, sig.len())]) }

async fn prepare_user_deploys(
    casper_snapshot: &CasperSnapshot,
    block_number: i64,
    current_time_millis: i64,
    deploy_storage: Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
) -> Result<PreparedUserDeploys, CasperError> {
    // Phase 9 (A-3): parking_lot::Mutex — no poison.
    let mut deploy_storage_guard = deploy_storage.lock();

    // Read all unfinalized deploys from storage
    let unfinalized: HashSet<Signed<DeployData>> = deploy_storage_guard.read_all()?;

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

    // Remove deploys that are already in scope to prevent resending
    let already_in_scope: Vec<Signed<DeployData>> = valid
        .iter()
        .filter(|deploy| casper_snapshot.deploys_in_scope.contains(&deploy.sig))
        .map(|deploy| (*deploy).clone())
        .collect();
    let valid_unique: HashSet<Signed<DeployData>> = valid
        .into_iter()
        .filter(|deploy| !casper_snapshot.deploys_in_scope.contains(&deploy.sig))
        .collect();

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
            deploy_sig_prefix(&d.sig),
            d.data.valid_after_block_number,
            block_number
        );
    }
    for d in &block_expired_deploys {
        tracing::warn!(
            "Deploy {}... FILTERED (block-expired): validAfterBlockNumber={} <= earliestBlock={}",
            deploy_sig_prefix(&d.sig),
            d.data.valid_after_block_number,
            earliest_block_number
        );
    }
    for d in &time_expired_deploys {
        tracing::warn!(
            "Deploy {}... FILTERED (time-expired): expirationTimestamp={:?} <= currentTime={}",
            deploy_sig_prefix(&d.sig),
            d.data.expiration_timestamp,
            current_time_millis
        );
    }
    for d in &already_in_scope {
        tracing::warn!(
            "Deploy {}... FILTERED (already in scope): deploy already exists in DAG within lifespan window",
            deploy_sig_prefix(&d.sig)
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
            "Removing {} expired deploy(s) from storage",
            all_expired.len()
        );
        let expired_list: Vec<Signed<DeployData>> = all_expired.into_iter().cloned().collect();
        deploy_storage_guard.remove(expired_list)?;
    }

    let max_deploys = casper_snapshot
        .on_chain_state
        .shard_conf
        .max_user_deploys_per_block as usize;
    let max_user_deploys = max_deploys;
    if valid_unique.len() <= max_user_deploys {
        return Ok(PreparedUserDeploys {
            deploys: valid_unique,
            effective_cap: max_user_deploys,
            cap_hit: false,
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
        if DEPLOY_SELECTION_RESERVE_TAIL_ENABLED {
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

    Ok(PreparedUserDeploys {
        deploys: selected,
        effective_cap: max_user_deploys,
        cap_hit: true,
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

async fn prepare_slashing_deploys(
    casper_snapshot: &CasperSnapshot,
    validator_identity: &ValidatorIdentity,
    seq_num: i32,
) -> Result<Vec<SlashDeploy>, CasperError> {
    let self_id = Bytes::copy_from_slice(&validator_identity.public_key.bytes);

    // An unbonded proposer cannot effect a slash (the PoS contract rejects
    // the deploy at replay time). Skip emission to avoid wasted work and to
    // satisfy the proven-correct theorem T-9.8 — see
    // docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.8.
    //
    // Symmetry note: the receive-side predicate
    // `validate_received_slash_deploys` does NOT require the block sender to
    // be bonded — it only checks the slash *target* is bonded (rule 6). The
    // block-sender-bonded invariant is enforced upstream by
    // `block_sender_has_weight` (validate.rs); this proposer-side filter is
    // an optimization, not an authorization predicate. The two cannot
    // diverge in a way that admits unauthorized slashes.
    let proposer_bond = casper_snapshot
        .on_chain_state
        .bonds_map
        .get(&self_id)
        .copied()
        .unwrap_or(0);
    if proposer_bond <= 0 {
        return Ok(Vec::new());
    }

    let slash_candidates = authorized_slash_candidates(casper_snapshot)?;

    // `authorized_slash_candidates` documents an at-most-one-per-offender
    // invariant via its `BTreeMap<Validator, …>` accumulator
    // (slashing_authorization.rs:253-317). Pin the contract at the boundary
    // so a future refactor of that helper can't silently produce duplicates.
    debug_assert!(
        {
            let mut offenders: Vec<&prost::bytes::Bytes> =
                slash_candidates.iter().map(|c| &c.offender).collect();
            offenders.sort();
            let original_len = offenders.len();
            offenders.dedup();
            offenders.len() == original_len
        },
        "authorized_slash_candidates must produce unique offenders; got duplicates"
    );

    // Slash deploys are NOT persisted in `KeyValueDeployStorage` and
    // this is correct by design (not a TODO).
    //
    // (1) Structural reason: `KeyValueDeployStorage` is keyed on the
    //     user-deploy signature `(sig → Signed<DeployData>)`. Slash
    //     deploys are unsigned `SystemDeployEnum::Slash(SlashDeploy
    //     { invalid_block_hash, pk, target_activation_epoch, initial_rand })` — they have no
    //     `Signed<DeployData>` shape and cannot be inserted.
    //
    // (2) Determinism reason: slash deploys are pure functions of
    //     `(authorized invalid-block evidence, validator_identity,
    //      target_activation_epoch, seq_num,
    //      generate_slash_deploy_random_seed)`. The invalid-block
    //     evidence is persisted via `BlockMetadataStore`. On node
    //     restart, `prepare_slashing_deploys` deterministically
    //     reconstructs the same slash-deploy set.
    //
    // (3) Theorem citations: T-4 (record monotonicity) +
    //     T-9.3 (catch-all dispatcher mints record per slashable
    //     block) jointly guarantee that the set of bonded current-epoch
    //     invalid-block evidence is exactly the input domain of
    //     `prepare_slashing_deploys`. See
    //     formal/rocq/slashing/theories/EquivocationRecord.v
    //     (`record_monotone`) and
    //     formal/rocq/slashing/theories/BugFixDispatcher.v
    //     (`t_9_3_catchall_mints_record`).
    //
    // (4) Symmetric reasoning: `CloseBlockDeploy` is also a system
    //     deploy and is not persisted in `KeyValueDeployStorage`
    //     for the same reason. The asymmetry is intentional: user
    //     deploys are crash-recovery state; system deploys are
    //     deterministically replayable from the persisted DAG.
    //
    // See docs/theory/slashing/design/06-proposing-and-effect.md for
    // the full rationale.

    // Create SlashDeploy objects
    let mut slashing_deploys = Vec::new();
    for slash_candidate in slash_candidates {
        let slash_deploy = SlashDeploy {
            invalid_block_hash: slash_candidate.invalid_block_hash.clone(),
            pk: validator_identity.public_key.clone(),
            // Phase 10 (C-5): convert typed Epoch back to the protobuf
            // i64 at the wire boundary.
            target_activation_epoch: slash_candidate.target_activation_epoch.get(),
            initial_rand: system_deploy_util::generate_slash_deploy_random_seed(
                self_id.clone(),
                seq_num,
            ),
        };

        tracing::info!(
            "Issuing slashing deploy justified by block {}",
            pretty_printer::PrettyPrinter::build_string_bytes(&slash_candidate.invalid_block_hash)
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
    deploy_storage: Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    failure_msg: &str,
) -> Result<bool, CasperError> {
    let Some(sig) = extract_deploy_sig_from_refund_failure(failure_msg) else {
        return Ok(false);
    };

    // Phase 9 (A-3): parking_lot::Mutex — no poison.
    let mut guard = deploy_storage.lock();
    guard.remove_by_sig(&sig).map_err(CasperError::KvStoreError)
}

pub async fn create(
    casper_snapshot: &CasperSnapshot,
    validator_identity: &ValidatorIdentity,
    dummy_deploy_opt: Option<(PrivateKey, String)>,
    deploy_storage: Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    runtime_manager: &mut RuntimeManager,
    block_store: &mut KeyValueBlockStore,
    allow_empty_blocks: bool,
) -> Result<BlockCreatorResult, CasperError> {
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

    // Sequence numbers are wire-protocol i32. Use a checked successor here
    // rather than `+ 1` so a hostile snapshot can't roll the local validator
    // past i32::MAX silently — overflow surfaces as a `CasperError` and the
    // proposer refuses to mint the block. Mirrors the receiver-side
    // `checked_base_seq` check.
    let next_seq_num = casper_snapshot
        .max_seq_nums
        .get(&validator_identity.public_key.bytes)
        .map(|seq| {
            checked_next_seq(*seq).ok_or_else(|| {
                CasperError::RuntimeError(format!("next sequence number overflows i32: {}", *seq))
            })
        })
        .transpose()?
        .unwrap_or(1);
    // P2-9: align with T-9.14's checked-arithmetic discipline; surface
    // overflow as an error instead of silently wrapping around.
    let next_block_num = casper_snapshot
        .max_block_num
        .checked_add(1)
        .ok_or_else(|| {
            CasperError::RuntimeError(format!(
                "max_block_num overflow: {} + 1 wraps i64",
                casper_snapshot.max_block_num
            ))
        })?;
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
    let (user_deploys, _, _) = {
        let t = std::time::Instant::now();
        let prepared = prepare_user_deploys(
            casper_snapshot,
            next_block_num,
            now_millis,
            deploy_storage.clone(),
        )
        .await?;
        let mut v = prepared.deploys;
        let self_chain_deploy_sigs =
            collect_self_chain_deploy_sigs(casper_snapshot, validator_identity, block_store)?;
        if !self_chain_deploy_sigs.is_empty() {
            let before = v.len();
            v.retain(|deploy| !self_chain_deploy_sigs.contains(&deploy.sig));
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
        (v, prepared.effective_cap, prepared.cap_hit)
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

    // Check if we have any new work to process.
    // If empty blocks are disabled, skip closeBlock-only proposals to avoid no-op checkpoint cost.
    // If empty blocks are enabled (heartbeat/liveness mode), continue and emit closeBlock.
    let has_slashing_deploys = !slashing_deploys.is_empty();
    if all_deploys.is_empty() && !has_slashing_deploys && !allow_empty_blocks {
        tracing::info!(
            "Skipping empty block creation: no new user deploys and no slashing deploys"
        );
        return Ok(BlockCreatorResult::NoNewDeploys);
    }

    // Make sure closeBlock is the last system Deploy
    let mut system_deploys_converted: Vec<SystemDeployEnum> = Vec::new();

    // Add slashing deploys
    for slash_deploy in slashing_deploys {
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

    let (
        pre_state_hash,
        post_state_hash,
        processed_deploys,
        rejected_deploys,
        processed_system_deploys,
        new_bonds,
    ) = checkpoint_data;

    let casper_version = casper_snapshot.on_chain_state.shard_conf.casper_version;

    // Span[F].trace(ProcessDeploysAndCreateBlockMetricsSource) from Scala
    let _span =
        tracing::info_span!(target: "f1r3fly.create-block", "process-deploys-and-create-block")
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
        new_bonds,
        shard_id,
        casper_version,
    );
    let package_ms = package_started.elapsed().as_millis();

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
    rejected_deploys: Vec<Bytes>,
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
        .map(|r| RejectedDeploy { sig: r })
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
