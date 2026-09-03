// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/casper/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// See casper/src/main/scala/coop/rchain/casper/blocks/proposer/BlockCreator.scala

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
use block_storage::rust::deploy::pending_deploy::PendingDeploy;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::Cosigned;
use models::rust::block_hash::BlockHash;
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::pretty_printer;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, Bond, DeployData, F1r3flyState, FinalizationCertificate, Header,
    Justification, ObjectiveEquivocationEvidence, ProcessedDeploy, ProcessedSystemDeploy,
    RejectedDeploy, StateEffectId, ValidatorBondGeneration,
};
use models::rust::deploy_id::DeployLookupId;
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use prost::Message;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::errors::HistoryError;
use tracing;

use crate::rust::blocks::proposer::propose_result::{BlockCreatorResult, RecoveryDeferralReason};
use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;
use crate::rust::finality::floor_context::{FloorContext, RetryGateBasis};
use crate::rust::slashing_authorization::{
    authorized_slash_candidates, checked_next_seq, has_slash_evidence, CanonicalSlashAuthority,
};
use crate::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use crate::rust::util::rholang::costacc::slash_deploy::SlashDeploy;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum;
use crate::rust::util::rholang::system_deploy_user_error::SystemDeployPlatformFailure;
use crate::rust::util::rholang::{interpreter_util, system_deploy_util};
use crate::rust::util::{construct_deploy, proto_util};
use crate::rust::validator_identity::ValidatorIdentity;

struct BlockCreationHeapBoundary;

impl Drop for BlockCreationHeapBoundary {
    fn drop(&mut self) {
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        {
            RuntimeManager::trim_allocator();
            metrics::counter!(
                crate::rust::metrics_constants::ALLOCATOR_TRIM_TOTAL_METRIC,
                "source" => crate::rust::metrics_constants::CASPER_METRICS_SOURCE
            )
            .increment(1);
        }
    }
}

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
    pub deploys: HashSet<PendingDeploy>,
    pub effective_cap: usize,
    pub cap_hit: bool,
    pub selected_retry_count: usize,
    pub selected_ordinary_count: usize,
    pub selected_in_scope_recovery_count: usize,
    pub selected_retry_sigs: HashSet<DeployLookupId>,
    pub selected_in_scope_recovery_sigs: HashSet<DeployLookupId>,
    pub already_in_scope_count: usize,
    pub selected_user_deploy_bytes: usize,
    pub deferred_user_deploy_bytes: usize,
    pub byte_cap_hit: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DeployAdmissionPolicy {
    allow_ordinary: bool,
    ordinary_cap: usize,
    allow_in_scope_recovery: bool,
    in_scope_recovery_cap: usize,
    reserve_tail: bool,
    fallback: bool,
    backpressure: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BranchDeployInfo {
    block_hash: BlockHash,
    sender: Validator,
    block_number: i64,
    timestamp: i64,
    deploy_sig_count: usize,
    new_sig_count: usize,
    recycled_sig_count: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DeployInclusionProgress {
    leader: Option<Validator>,
    latest_deploy: Option<BranchDeployInfo>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FreshLocalDeployStats {
    count: usize,
    oldest_age_millis: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct InScopeLocalDeployStats {
    count: usize,
    oldest_age_millis: i64,
    stranded_count: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct DeployInclusionStaleness {
    stale: bool,
    block_or_time_stale: bool,
    signature_stale: bool,
    missing_deploy_metadata: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FreshAdmissionFallback {
    allowed: bool,
    cap: usize,
    backpressure: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DeploySelection {
    deploys: HashSet<PendingDeploy>,
    strategy: &'static str,
    selected_bytes: usize,
    deferred_bytes: usize,
    count_capped: bool,
    byte_capped: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct FinalityLagStats {
    dag_tip: i64,
    last_finalized_block: i64,
    lag: i64,
}

/// C15 / Smell-2: was previously a zero-arg `fn -> bool` returning a
/// hard-coded `true`. Promoted to a `const` so its always-on nature
/// is explicit and the value is folded at compile time. Kept as a
/// named constant (rather than inlined `true`) because it is a
/// feature-flag posture that may yet be moved into `CasperShardConf`
/// for per-shard control — when that happens the rename target is
/// already in place.
const DEPLOY_SELECTION_RESERVE_TAIL_ENABLED: bool = true;
const ORDINARY_DEPLOY_PROPOSAL_CAP: usize = 128;
const USER_DEPLOY_BYTE_PROPOSAL_BUDGET: usize = 2 * 1024 * 1024;
const USER_DEPLOY_BACKPRESSURE_BYTE_PROPOSAL_BUDGET: usize = 512 * 1024;
const RETRY_DEPLOY_REPROPOSAL_CAP: usize = 32;
const RETRY_FRONTIER_DEFERRAL_LEASE_BLOCKS: i64 = 3;
const NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP: usize = 8;
const NON_LEADER_FALLBACK_MIN_ORDINARY_DEPLOY_CAP: usize = 4;
const NON_LEADER_FALLBACK_MEDIUM_ORDINARY_DEPLOY_CAP: usize = 16;
const NON_LEADER_FALLBACK_MAX_ORDINARY_DEPLOY_CAP: usize = 32;
const DEPLOY_INCLUSION_LEASE_BLOCKS: i64 = 3;
const DEPLOY_INCLUSION_LEASE_MILLIS: i64 = 30_000;
const FRESH_DEPLOY_MAX_ADMISSION_DELAY_MILLIS: i64 = 60_000;
const FRESH_DEPLOY_ESCALATED_ADMISSION_DELAY_MILLIS: i64 = 120_000;
const FRESH_DEPLOY_MAX_ESCALATED_ADMISSION_DELAY_MILLIS: i64 = 300_000;
const FINALITY_LAG_SOFT_BACKPRESSURE_BLOCKS: i64 = 4;
const FINALITY_LAG_HARD_BACKPRESSURE_BLOCKS: i64 = 8;
const DEPLOY_LOG_SAMPLE_LIMIT: usize = 8;

/// C15 / Smell-4: extract the deploy-signature pretty-print prefix
/// used in operator-facing log messages. Previously inlined as
/// `deploy_sig_prefix(&d.sig)` at four
/// sites in `log_deploy_pool_filtering`.
fn deploy_sig_prefix(sig: &Bytes) -> String { hex::encode(&sig[..std::cmp::min(8, sig.len())]) }

fn bounded_deploy_id_sample<'a>(
    deploy_ids: impl IntoIterator<Item = &'a [u8]>,
) -> (Vec<String>, usize, usize) {
    let mut sample = Vec::with_capacity(DEPLOY_LOG_SAMPLE_LIMIT);
    let mut count = 0usize;
    for deploy_id in deploy_ids {
        count = count.saturating_add(1);
        if sample.len() < DEPLOY_LOG_SAMPLE_LIMIT {
            sample.push(hex::encode(&deploy_id[..std::cmp::min(8, deploy_id.len())]));
        }
    }
    let omitted = count.saturating_sub(sample.len());
    (sample, count, omitted)
}

fn trace_deploy_filter<'a>(reason: &'static str, deploy_ids: impl IntoIterator<Item = &'a [u8]>) {
    let (sample, count, omitted) = bounded_deploy_id_sample(deploy_ids);
    if count > 0 {
        tracing::debug!(
            target: "f1r3fly.merge.cpps",
            step = "prepare_user_deploys.FILTER",
            decision = "filtered",
            reason,
            count,
            omitted,
            deploy_sample = ?sample,
            "merge.cpps: deploy filter summary"
        );
    }
}

fn retry_frontier_deferral_lease_expired(next_block: i64, rejection_height: i64) -> bool {
    next_block.saturating_sub(rejection_height) > RETRY_FRONTIER_DEFERRAL_LEASE_BLOCKS
}

fn selected_parents_collectively_cover_latest_messages(
    dag: &KeyValueDagRepresentation,
    parents: &[BlockMessage],
    justifications: &[Justification],
    invalid_blocks: &HashMap<BlockHash, Validator>,
) -> Result<bool, CasperError> {
    for justification in justifications {
        if invalid_blocks.contains_key(&justification.latest_block_hash) {
            continue;
        }
        let mut covered = false;
        for parent in parents {
            if dag.is_dag_ancestor(&justification.latest_block_hash, &parent.block_hash)? {
                covered = true;
                break;
            }
        }
        if !covered {
            return Ok(false);
        }
    }
    Ok(true)
}

/// One line per deferred retry with the gate's basis — a recurring deferral
/// for one sig is the starvation tripwire, and the basis says which of the
/// gate's closed conditions is holding it.
fn trace_retry_gate_deferral(
    deploy_id: &DeployLookupId,
    basis: &RetryGateBasis,
    ctx: &FloorContext,
) {
    let (basis_name, record_block) = match basis {
        RetryGateBasis::Open => return,
        RetryGateBasis::NoDisposition => ("no-disposition", String::new()),
        RetryGateBasis::NoKeptRejection => ("no-kept-rejection", String::new()),
        RetryGateBasis::RecordAboveFloor(block) => (
            "record-above-floor",
            hex::encode(&block[..std::cmp::min(8, block.len())]),
        ),
    };
    tracing::debug!(
        target: "f1r3fly.casper.recovery",
        sig = %deploy_sig_prefix(&Bytes::copy_from_slice(deploy_id.as_bytes())),
        basis = basis_name,
        record_block = %record_block,
        floor_number = ctx.floor.block_number,
        "retry deferred by the gate"
    );
}

fn ordered_user_deploys(deploys: &HashSet<PendingDeploy>) -> Vec<PendingDeploy> {
    let mut ordered: Vec<PendingDeploy> = deploys.iter().cloned().collect();
    ordered.sort_by(|a, b| {
        a.data()
            .valid_after_block_number
            .cmp(&b.data().valid_after_block_number)
            .then_with(|| a.data().time_stamp.cmp(&b.data().time_stamp))
            .then_with(|| a.deploy_id().cmp(b.deploy_id()))
    });
    ordered
}

#[cfg(test)]
fn select_recovered_deploys_for_block(
    deploys: &HashSet<PendingDeploy>,
    _block_number: i64,
    cap: usize,
) -> DeploySelection {
    select_deploys_for_block(deploys, cap, false, USER_DEPLOY_BYTE_PROPOSAL_BUDGET)
}

fn deploy_encoded_len(deploy: &PendingDeploy) -> usize { deploy.encoded_len() }

fn select_deploys_for_block(
    deploys: &HashSet<PendingDeploy>,
    cap: usize,
    reserve_tail: bool,
    byte_budget: usize,
) -> DeploySelection {
    // One proto encoding per deploy per selection call: sizes are computed
    // here and reused for both the total and the budget loop below.
    let sizes: HashMap<Bytes, usize> = deploys
        .iter()
        .map(|d| (d.deploy_id().clone(), deploy_encoded_len(d)))
        .collect();
    let total_bytes: usize = sizes.values().sum();
    let ordered = ordered_user_deploys(deploys);
    if ordered.is_empty() || cap == 0 || byte_budget == 0 {
        return DeploySelection {
            deploys: HashSet::new(),
            strategy: "none",
            selected_bytes: 0,
            deferred_bytes: total_bytes,
            count_capped: cap == 0 && !ordered.is_empty(),
            byte_capped: byte_budget == 0 && !ordered.is_empty(),
        };
    }
    let count_capped = ordered.len() > cap;
    let (candidates, strategy): (Vec<PendingDeploy>, &'static str) = if ordered.len() <= cap {
        (ordered, "uncapped")
    } else if reserve_tail && DEPLOY_SELECTION_RESERVE_TAIL_ENABLED && cap > 1 {
        let oldest_take = cap.saturating_sub(1);
        let mut candidates: Vec<PendingDeploy> =
            ordered.iter().take(oldest_take).cloned().collect();
        candidates.extend(ordered.iter().last().cloned());
        (candidates, "oldest-plus-newest")
    } else {
        (ordered.into_iter().take(cap).collect(), "oldest-only")
    };
    let mut selected = HashSet::new();
    let mut selected_bytes = 0usize;
    let mut byte_capped = false;
    // Skip-and-continue is starvation-free, not just best-effort packing: a
    // budget-skipped deploy keeps its position in the (valid_after, timestamp,
    // sig) order while everything ahead of it gets selected, included, and
    // removed from storage — new deploys always sort behind it — so it
    // strictly advances to the front, where the `selected.is_empty()`
    // carve-out admits it alone even when it alone exceeds the budget.
    for deploy in candidates {
        let deploy_bytes = sizes.get(deploy.deploy_id()).copied().unwrap_or_default();
        let next_bytes = selected_bytes.saturating_add(deploy_bytes);
        if next_bytes <= byte_budget || selected.is_empty() {
            selected_bytes = next_bytes;
            selected.insert(deploy);
        } else {
            byte_capped = true;
        }
    }
    if selected_bytes > byte_budget {
        byte_capped = true;
    }
    DeploySelection {
        deploys: selected,
        strategy,
        selected_bytes,
        deferred_bytes: total_bytes.saturating_sub(selected_bytes),
        count_capped,
        byte_capped,
    }
}

fn user_deploy_byte_budget(admission_policy: DeployAdmissionPolicy) -> usize {
    if admission_policy.backpressure {
        USER_DEPLOY_BACKPRESSURE_BYTE_PROPOSAL_BUDGET
    } else {
        USER_DEPLOY_BYTE_PROPOSAL_BUDGET
    }
}

fn normal_ordinary_deploy_cap(casper_snapshot: &CasperSnapshot) -> usize {
    (casper_snapshot
        .on_chain_state
        .shard_conf
        .max_user_deploys_per_block as usize)
        .min(ORDINARY_DEPLOY_PROPOSAL_CAP)
}

fn is_retryable_single_value_batch_error(err: &CasperError) -> bool {
    match err {
        CasperError::HistoryError(HistoryError::MergeError(msg)) => {
            msg.contains("single-value cell")
                && msg.contains("would hold")
                && msg.contains("after merge")
        }
        CasperError::RuntimeError(msg) => {
            (msg.contains("number channel")
                && msg.contains("holds")
                && msg.contains("IntegerAdd single-value invariant violated"))
                || msg.contains("Expected at most one value for number channel")
        }
        _ => false,
    }
}

fn next_single_value_retry_limit(current: usize) -> Option<usize> {
    if current > 1 {
        Some(std::cmp::max(1, current / 2))
    } else {
        None
    }
}

fn next_checkpoint_retry_limit(err: &CasperError, current: usize) -> Option<usize> {
    if is_retryable_single_value_batch_error(err)
        || matches!(
            err,
            CasperError::SystemRuntimeError(
                SystemDeployPlatformFailure::GasPaymentFailure(_)
                    | SystemDeployPlatformFailure::GasRefundFailure(_)
            )
        )
    {
        next_single_value_retry_limit(current)
    } else {
        None
    }
}

pub async fn prepare_user_deploys(
    casper_snapshot: &CasperSnapshot,
    block_number: i64,
    current_time_millis: i64,
    deploy_storage: Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    rejected_deploy_buffer: Arc<
        Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>,
    >,
    block_store: &KeyValueBlockStore,
    allow_recovered_deploys: bool,
    allow_ordinary_deploys: bool,
) -> Result<PreparedUserDeploys, CasperError> {
    let floor_ctx = derive_floor_context(casper_snapshot, block_store).await?;
    prepare_user_deploys_with_policy(
        casper_snapshot,
        block_number,
        current_time_millis,
        deploy_storage,
        rejected_deploy_buffer,
        block_store,
        allow_recovered_deploys,
        DeployAdmissionPolicy {
            allow_ordinary: allow_ordinary_deploys,
            ordinary_cap: normal_ordinary_deploy_cap(casper_snapshot),
            allow_in_scope_recovery: false,
            in_scope_recovery_cap: 0,
            reserve_tail: true,
            fallback: false,
            backpressure: false,
        },
        floor_ctx.as_ref(),
    )
    .await
}

/// One [`FloorContext`] per block operation, from the snapshot's frozen
/// (parents, justifications) pair. `create` derives it once and threads it;
/// entry points callable outside `create` derive their own. `None` iff the
/// snapshot has no parents (parentless fixtures and the pre-genesis shape) —
/// there is no floor to derive, and every consumer's walk over zero parents
/// is empty anyway.
pub(crate) async fn derive_floor_context(
    casper_snapshot: &CasperSnapshot,
    block_store: &KeyValueBlockStore,
) -> Result<Option<FloorContext>, CasperError> {
    if casper_snapshot.parents.is_empty() {
        return Ok(None);
    }
    let parent_hashes: Vec<BlockHash> = casper_snapshot
        .parents
        .iter()
        .map(|p| p.block_hash.clone())
        .collect();
    let latest_messages: BTreeMap<Validator, BlockHash> = casper_snapshot
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    FloorContext::derive(
        &casper_snapshot.dag,
        block_store,
        &parent_hashes,
        &latest_messages,
        crate::rust::safety::clique_oracle::FtThreshold::from_ppm(
            casper_snapshot
                .on_chain_state
                .shard_conf
                .fault_tolerance_threshold_ppm,
        ),
        casper_snapshot.on_chain_state.shard_conf.casper_version,
    )
    .await
    .map(Some)
}

/// The parents-rooted canonical-won walk, through the operation context's
/// memo when one exists (context-less entry points walk directly — over an
/// empty parent set the walk is empty either way).
fn canonical_won_over_parents(
    floor_ctx: Option<&FloorContext>,
    casper_snapshot: &CasperSnapshot,
    block_store: &KeyValueBlockStore,
    earliest_block_number: i64,
) -> Result<HashSet<DeployLookupId>, CasperError> {
    match floor_ctx {
        Some(ctx) => ctx.won_sigs(block_store, earliest_block_number),
        None => {
            let parent_hashes: Vec<BlockHash> = casper_snapshot
                .parents
                .iter()
                .map(|p| p.block_hash.clone())
                .collect();
            interpreter_util::canonical_won_sigs(block_store, &parent_hashes, earliest_block_number)
        }
    }
}

async fn prepare_user_deploys_with_policy(
    casper_snapshot: &CasperSnapshot,
    block_number: i64,
    current_time_millis: i64,
    deploy_storage: Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    rejected_deploy_buffer: Arc<
        Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>,
    >,
    block_store: &KeyValueBlockStore,
    allow_recovered_deploys: bool,
    admission_policy: DeployAdmissionPolicy,
    floor_ctx: Option<&FloorContext>,
) -> Result<PreparedUserDeploys, CasperError> {
    let max_user_deploys = normal_ordinary_deploy_cap(casper_snapshot);
    let ordinary_cap = admission_policy.ordinary_cap.min(max_user_deploys);
    let in_scope_recovery_cap = admission_policy.in_scope_recovery_cap.min(max_user_deploys);
    let allow_ordinary_deploys = admission_policy.allow_ordinary && ordinary_cap > 0;
    let allow_in_scope_recovery =
        admission_policy.allow_in_scope_recovery && in_scope_recovery_cap > 0;
    let mut deploy_storage_guard = deploy_storage.lock();

    let stored_unfinalized: HashSet<PendingDeploy> =
        if allow_ordinary_deploys || allow_in_scope_recovery {
            deploy_storage_guard
                .read_all_for_protocol(casper_snapshot.on_chain_state.shard_conf.casper_version)?
        } else {
            HashSet::new()
        };

    let mut buffered_deploys: HashSet<PendingDeploy> =
        if allow_ordinary_deploys || allow_in_scope_recovery || allow_recovered_deploys {
            let buffer_guard = rejected_deploy_buffer
                .lock()
                .map_err(|e| CasperError::LockError(e.to_string()))?;
            buffer_guard.read_all()?
        } else {
            HashSet::new()
        };
    let earliest_block_number = crate::rust::util::deploy_window::earliest_valid_after(
        block_number,
        casper_snapshot.on_chain_state.shard_conf.deploy_lifespan,
    )?;
    // The FLOOR-clock window bound for retry work. The floor is the only
    // clock that closes a validity window irreversibly (the merge window
    // rule and the buffer retain read the same bound); the tip clock runs
    // ahead of it, so tip-expired floor-live retries must stay admissible
    // and must never be deleted. `None` (no derivable floor) defers every
    // irreversible removal of retry work — delay, never loss.
    let floor_expiry_bound = floor_ctx
        .map(|ctx| {
            crate::rust::util::deploy_window::earliest_valid_after(
                ctx.floor.block_number,
                casper_snapshot.on_chain_state.shard_conf.deploy_lifespan,
            )
        })
        .transpose()?;
    // Both expiry kinds are terminal for buffered work: a floor-window-closed
    // deploy can never again pass the merge window rule, so holding it
    // "recoverable" only re-offers it to a proposer that must reject it.
    let expired_buffered: Vec<PendingDeploy> = buffered_deploys
        .iter()
        .filter(|deploy| {
            deploy.data().is_expired_at(current_time_millis)
                || floor_expiry_bound.is_some_and(|bound| !not_expired_deploy(bound, deploy.data()))
        })
        .cloned()
        .collect();
    if !expired_buffered.is_empty() {
        for deploy in &expired_buffered {
            tracing::info!(
                target: "f1r3fly.casper.deploy_lifecycle",
                event = "buffer_removed",
                deploy_sig = %hex::encode(deploy.deploy_id()),
                reason = "expired",
                valid_after_block = deploy.data().valid_after_block_number,
                floor_expiry_bound = ?floor_expiry_bound,
                current_time_millis,
                "deploy lifecycle"
            );
        }
        tracing::info!(
            target: "f1r3fly.casper.recovery",
            "Removing {} expired rejected-buffer deploy(s) from storage and rejected-deploy buffer",
            expired_buffered.len()
        );
        for deploy in &expired_buffered {
            deploy_storage_guard.remove_envelope_by_id(deploy.deploy_id())?;
        }
        rejected_deploy_buffer
            .lock()
            .map_err(|e| CasperError::LockError(e.to_string()))?
            .remove(expired_buffered.clone())?;
        let expired_sigs: HashSet<DeployLookupId> = expired_buffered
            .into_iter()
            .map(|deploy| deploy.typed_deploy_id().clone())
            .collect();
        buffered_deploys.retain(|deploy| !expired_sigs.contains(deploy.typed_deploy_id()));
    }
    let mut buffered_sigs: HashSet<DeployLookupId> = buffered_deploys
        .iter()
        .map(|d| d.typed_deploy_id().clone())
        .collect();

    let skipped_buffered_ordinary = if allow_ordinary_deploys && !allow_recovered_deploys {
        stored_unfinalized
            .iter()
            .filter(|deploy| buffered_sigs.contains(deploy.typed_deploy_id()))
            .count()
    } else {
        0
    };
    if skipped_buffered_ordinary > 0 {
        tracing::info!(
            target: "f1r3fly.casper.recovery",
            "Prepare user deploys: skipped {} ordinary deploy(s) already parked in rejected-deploy buffer",
            skipped_buffered_ordinary
        );
    }

    let buffer_scan_floor = buffered_deploys
        .iter()
        .map(|d| d.data().valid_after_block_number)
        .min()
        .map(|h| h.min(earliest_block_number))
        .unwrap_or(earliest_block_number);

    // Terminal purge: eviction is irreversible, so it keys on the one
    // irreversible fact — the deploy's effect present in the FLOOR block's
    // committed post-state, read from the recorded construction facts.
    // Floor coverage is monotone (floor-covered effects are in every
    // future merge base), so a purged entry can never be needed again. No
    // node-local finality marker may evict: a win the finalizer marked
    // final can still sit above the justification-derived floor, where a
    // later merge can reject it, and the buffer holds the only
    // re-proposable copy. Without floor facts (parentless shapes) the
    // purge defers — delay, never loss.
    let settled_buffered: Vec<PendingDeploy> = match floor_ctx {
        Some(ctx) if !buffered_deploys.is_empty() => {
            let mut settled = Vec::new();
            for deploy in &buffered_deploys {
                if ctx.effect_settled_in_floor(
                    block_store,
                    deploy.data().valid_after_block_number,
                    deploy.typed_deploy_id(),
                )? {
                    settled.push(deploy.clone());
                }
            }
            settled
        }
        _ => Vec::new(),
    };
    if !settled_buffered.is_empty() {
        rejected_deploy_buffer
            .lock()
            .map_err(|e| CasperError::LockError(e.to_string()))?
            .remove(settled_buffered.clone())?;
        tracing::info!(
            target: "f1r3fly.casper.recovery",
            "Purged {} rejected-buffer entr(y/ies) with floor-settled effects before block #{}",
            settled_buffered.len(),
            block_number
        );
        for deploy in &settled_buffered {
            buffered_deploys.remove(deploy);
            buffered_sigs.remove(deploy.typed_deploy_id());
        }
    }

    let canonical_won_buffer_sigs = if allow_recovered_deploys && !buffered_deploys.is_empty() {
        canonical_won_over_parents(floor_ctx, casper_snapshot, block_store, buffer_scan_floor)?
    } else {
        HashSet::new()
    };

    // The retry gate, proposer side — the SAME predicate validation runs
    // (`FloorContext::retry_gate_open`), so a proposal the gate admits is a
    // proposal every validator accepts. A buffered retry stays parked until
    // its latest kept rejection settles into the floor closure; re-proposal
    // against a live contest regenerated same-sig sibling copies faster
    // than merges could adjudicate them. No derivable floor (`None`) defers
    // every retry — delay, never loss: the buffer's floor-window retain
    // keeps custody.
    let recovered: HashSet<PendingDeploy> = if allow_recovered_deploys {
        let mut kept: HashSet<PendingDeploy> = HashSet::new();
        let mut gated_count = 0usize;
        for deploy in buffered_deploys {
            let candidate = !canonical_won_buffer_sigs.contains(deploy.typed_deploy_id())
                && (!casper_snapshot
                    .deploys_in_scope
                    .contains(deploy.typed_deploy_id())
                    || casper_snapshot
                        .rejected_in_scope
                        .contains(deploy.typed_deploy_id()));
            if !candidate {
                continue;
            }
            match floor_ctx {
                Some(ctx) => {
                    match ctx.retry_gate_basis(
                        &casper_snapshot.dag,
                        block_store,
                        earliest_block_number,
                        deploy.typed_deploy_id(),
                    )? {
                        RetryGateBasis::Open => {
                            kept.insert(deploy);
                        }
                        basis => {
                            trace_retry_gate_deferral(deploy.typed_deploy_id(), &basis, ctx);
                            gated_count += 1;
                        }
                    }
                }
                None => gated_count += 1,
            }
        }
        if gated_count > 0 {
            tracing::info!(
                target: "f1r3fly.casper.recovery",
                "Prepare user deploys: {} buffered retr(y/ies) deferred by the retry gate",
                gated_count
            );
        }
        kept
    } else {
        HashSet::new()
    };
    let recovered_sigs: HashSet<DeployLookupId> = recovered
        .iter()
        .map(|d| d.typed_deploy_id().clone())
        .collect();
    let recovery_backlog = allow_recovered_deploys && !recovered.is_empty();
    let storage_scan_allowed_now = allow_ordinary_deploys || allow_in_scope_recovery;

    let ordinary_kept_with_recovery = if recovery_backlog && allow_ordinary_deploys {
        stored_unfinalized
            .iter()
            .filter(|deploy| !buffered_sigs.contains(deploy.typed_deploy_id()))
            .count()
    } else {
        0
    };
    if ordinary_kept_with_recovery > 0 {
        tracing::info!(
            target: "f1r3fly.casper.recovery",
            "Prepare user deploys: kept {} ordinary deploy(s) eligible while rejected deploy recovery backlog drains",
            ordinary_kept_with_recovery
        );
    }

    let unfinalized: HashSet<PendingDeploy> = if storage_scan_allowed_now {
        stored_unfinalized
            .into_iter()
            .filter(|deploy| !buffered_sigs.contains(deploy.typed_deploy_id()))
            .collect()
    } else {
        HashSet::new()
    };

    let suppressed_recovered_in_scope = if allow_recovered_deploys {
        buffered_sigs
            .iter()
            .filter(|sig| {
                canonical_won_buffer_sigs.contains(*sig)
                    || (casper_snapshot.deploys_in_scope.contains(*sig)
                        && !casper_snapshot.rejected_in_scope.contains(*sig))
            })
            .count()
    } else {
        0
    };
    if suppressed_recovered_in_scope > 0 {
        for sig in buffered_sigs.iter().filter(|sig| {
            canonical_won_buffer_sigs.contains(*sig)
                || (casper_snapshot.deploys_in_scope.contains(*sig)
                    && !casper_snapshot.rejected_in_scope.contains(*sig))
        }) {
            tracing::info!(
                target: "f1r3fly.casper.deploy_lifecycle",
                event = "recovery_suppressed",
                deploy_sig = %hex::encode(sig.as_bytes()),
                canonical_won = canonical_won_buffer_sigs.contains(sig),
                in_scope = casper_snapshot.deploys_in_scope.contains(sig),
                rejected_in_scope = casper_snapshot.rejected_in_scope.contains(sig),
                next_block = block_number,
                "deploy lifecycle"
            );
        }
        tracing::info!(
            target: "f1r3fly.casper.recovery",
            "Prepare user deploys: suppressed {} recovered deploy(s) still visible in unresolved scope",
            suppressed_recovered_in_scope
        );
    }

    let recovered_count = recovered.len();
    if recovered_count > 0 {
        for deploy in &recovered {
            tracing::info!(
                target: "f1r3fly.casper.deploy_lifecycle",
                event = "recovery_candidate",
                deploy_sig = %hex::encode(deploy.deploy_id()),
                valid_after_block = deploy.data().valid_after_block_number,
                in_scope = casper_snapshot.deploys_in_scope.contains(deploy.typed_deploy_id()),
                rejected_in_scope = casper_snapshot.rejected_in_scope.contains(deploy.typed_deploy_id()),
                next_block = block_number,
                "deploy lifecycle"
            );
        }
        let recovered_sigs: Vec<String> = recovered
            .iter()
            .map(|d| hex::encode(&d.deploy_id()[..d.deploy_id().len().min(8)]))
            .collect();
        tracing::info!(
            target: "f1r3fly.casper.recovery",
            "Prepare user deploys: {} recovered from rejected-deploy buffer; sigs={:?}",
            recovered_count,
            recovered_sigs
        );
    }
    let unfinalized: HashSet<PendingDeploy> = unfinalized
        .into_iter()
        .chain(recovered.into_iter())
        .collect();

    tracing::debug!(
        target: "f1r3fly.merge.cpps",
        step = "prepare_user_deploys.POOL",
        block_number,
        unfinalized_pool = unfinalized.len(),
        recovered = recovered_count,
        "merge.cpps: deploy pool assembled (unfinalized + recovered re-admits)"
    );

    let canonical_scan_floor = unfinalized
        .iter()
        .filter(|d| {
            recovered_sigs.contains(d.typed_deploy_id())
                || casper_snapshot
                    .rejected_in_scope
                    .contains(d.typed_deploy_id())
        })
        .map(|d| d.data().valid_after_block_number)
        .min()
        .map(|h| h.min(earliest_block_number))
        .unwrap_or(earliest_block_number);

    // Retry work (buffered or rejected-in-scope) reads the FLOOR-clock
    // window for block expiry; ordinary deploys keep the tip clock, which
    // is never looser than the floor's, so nothing leaks back. Absent a
    // derivable floor, retry work also falls back to the tip clock for
    // ADMISSION only (removal below defers instead — deletion is
    // irreversible, admission is retried next round).
    let is_retry_sig = |deploy_id: &DeployLookupId| {
        buffered_sigs.contains(deploy_id) || casper_snapshot.rejected_in_scope.contains(deploy_id)
    };
    let block_expiry_bound = |deploy: &PendingDeploy| {
        if is_retry_sig(deploy.typed_deploy_id()) {
            floor_expiry_bound.unwrap_or(earliest_block_number)
        } else {
            earliest_block_number
        }
    };

    // Categorize deploys for logging
    let future_deploys: Vec<_> = unfinalized
        .iter()
        .filter(|d| !not_future_deploy(block_number, d.data()))
        .collect();
    let block_expired_deploys: Vec<_> = unfinalized
        .iter()
        .filter(|d| !not_expired_deploy(block_expiry_bound(d), d.data()))
        .collect();
    let time_expired_deploys: Vec<_> = unfinalized
        .iter()
        .filter(|d| d.data().is_expired_at(current_time_millis))
        .collect();

    // Filter valid deploys (not expired by block, not expired by time, and
    // not future). Block expiry applies to recovered and rejected-retry
    // deploys too — on the floor clock: the merge window rule and expiry
    // validity read the same bound, so a floor-window-closed deploy
    // admitted here could only yield a block that fails its own
    // validation, rebuilt every propose (the permanent finalization
    // wedge). Expiry is a chain-level invariant; recovery cannot outlive
    // it — but the clock that closes it is the floor's, never the tip's.
    let valid: HashSet<PendingDeploy> = unfinalized
        .iter()
        .filter(|deploy| {
            not_future_deploy(block_number, deploy.data())
                && not_expired_deploy(block_expiry_bound(deploy), deploy.data())
                && !deploy.data().is_expired_at(current_time_millis)
        })
        .cloned()
        .collect();

    let valid_count = valid.len();

    let canonical_won = canonical_won_over_parents(
        floor_ctx,
        casper_snapshot,
        block_store,
        canonical_scan_floor,
    )?;

    let recovered_canonical_wins: Vec<PendingDeploy> = valid
        .iter()
        .filter(|deploy| {
            recovered_sigs.contains(deploy.typed_deploy_id())
                && canonical_won.contains(deploy.typed_deploy_id())
        })
        .cloned()
        .collect();
    let retry_scope_exempt = |deploy: &PendingDeploy| {
        recovered_sigs.contains(deploy.typed_deploy_id())
            && casper_snapshot
                .rejected_in_scope
                .contains(deploy.typed_deploy_id())
    };
    let blocked_by_scope = |deploy: &PendingDeploy| {
        casper_snapshot
            .deploys_in_scope
            .contains(deploy.typed_deploy_id())
            && !retry_scope_exempt(deploy)
    };
    let already_in_scope: Vec<PendingDeploy> = valid
        .iter()
        .filter(|deploy| {
            canonical_won.contains(deploy.typed_deploy_id()) || blocked_by_scope(deploy)
        })
        .map(|deploy| (*deploy).clone())
        .collect();
    let valid_unique: HashSet<PendingDeploy> = valid
        .into_iter()
        .filter(|deploy| {
            !canonical_won.contains(deploy.typed_deploy_id()) && !blocked_by_scope(deploy)
        })
        .collect();

    let already_in_scope_count = already_in_scope.len();
    for deploy in &recovered_canonical_wins {
        tracing::info!(
            target: "f1r3fly.casper.deploy_lifecycle",
            event = "storage_removed",
            deploy_sig = %hex::encode(deploy.deploy_id()),
            reason = "canonical_parent_win",
            next_block = block_number,
            "deploy lifecycle"
        );
    }
    let purged_recovered_already_in_scope = purge_recovered_already_in_scope(
        &mut deploy_storage_guard,
        &recovered_canonical_wins,
        &recovered_sigs,
    )?;
    if purged_recovered_already_in_scope > 0 {
        tracing::info!(
            target: "f1r3fly.casper.recovery",
            "Purged {} recovered deploy(s) with canonical wins before proposing block #{}",
            purged_recovered_already_in_scope,
            block_number
        );
    }
    let is_retry_candidate = |deploy: &PendingDeploy| {
        recovered_sigs.contains(deploy.typed_deploy_id())
            || casper_snapshot
                .rejected_in_scope
                .contains(deploy.typed_deploy_id())
    };
    // The gate covers EVERY retry route. `recovered_sigs` already passed it
    // above; the pool route (rejected-in-scope, not buffered — reachable
    // under deep floor lag, where the record is in the walk window but its
    // carrier is below it) must pass the same predicate, or the proposer
    // mints a block every validator rejects as `PrematureDeployRetry`.
    let mut retry_candidates: HashSet<PendingDeploy> = HashSet::new();
    let mut gated_pool_retries = 0usize;
    for deploy in valid_unique.iter().filter(|d| is_retry_candidate(d)) {
        if recovered_sigs.contains(deploy.typed_deploy_id()) {
            retry_candidates.insert(deploy.clone());
            continue;
        }
        match floor_ctx {
            Some(ctx) => {
                match ctx.retry_gate_basis(
                    &casper_snapshot.dag,
                    block_store,
                    earliest_block_number,
                    deploy.typed_deploy_id(),
                )? {
                    RetryGateBasis::Open => {
                        retry_candidates.insert(deploy.clone());
                    }
                    basis => {
                        trace_retry_gate_deferral(deploy.typed_deploy_id(), &basis, ctx);
                        gated_pool_retries += 1;
                    }
                }
            }
            None => gated_pool_retries += 1,
        }
    }
    if gated_pool_retries > 0 {
        tracing::info!(
            target: "f1r3fly.casper.recovery",
            "Prepare user deploys: {} pool retr(y/ies) deferred by the retry gate",
            gated_pool_retries
        );
    }
    if !retry_candidates.is_empty() {
        let retry_frontier_merged = selected_parents_collectively_cover_latest_messages(
            &casper_snapshot.dag,
            &casper_snapshot.parents,
            &casper_snapshot.justifications,
            &casper_snapshot.invalid_blocks,
        )?;
        if !retry_frontier_merged {
            let latest_message_count = casper_snapshot
                .justifications
                .iter()
                .filter(|justification| {
                    !casper_snapshot
                        .invalid_blocks
                        .contains_key(&justification.latest_block_hash)
                })
                .count();
            let rejection_heights = match floor_ctx {
                Some(ctx) => ctx.latest_kept_rejection_heights(
                    block_store,
                    earliest_block_number,
                    retry_candidates
                        .iter()
                        .map(|deploy| deploy.typed_deploy_id()),
                )?,
                None => HashMap::new(),
            };
            let mut deferred_sigs = HashSet::new();
            for deploy in &retry_candidates {
                let rejection_height = rejection_heights
                    .get(deploy.typed_deploy_id())
                    .copied()
                    .flatten();
                // Deferral is bounded ONLY by the lease. A candidate whose
                // rejection height cannot be resolved has no lease clock, so
                // it escapes now: the gate (proposer and validators alike)
                // still bounds validity, and an unbounded packaging deferral
                // is the starvation the lease exists to stop.
                let escape_reason = match rejection_height {
                    None => Some("rejection_height_unknown"),
                    Some(height) if retry_frontier_deferral_lease_expired(block_number, height) => {
                        Some("deferral_lease_expired")
                    }
                    Some(_) => None,
                };
                if let Some(reason) = escape_reason {
                    tracing::info!(
                        target: "f1r3fly.casper.deploy_lifecycle",
                        event = "retry_frontier_escape",
                        deploy_sig = %hex::encode(deploy.deploy_id()),
                        reason,
                        next_block = block_number,
                        rejection_height,
                        deferral_lease_blocks = RETRY_FRONTIER_DEFERRAL_LEASE_BLOCKS,
                        selected_parent_count = casper_snapshot.parents.len(),
                        latest_message_count,
                        "deploy lifecycle"
                    );
                } else {
                    tracing::info!(
                        target: "f1r3fly.casper.deploy_lifecycle",
                        event = "retry_frontier_deferred",
                        deploy_sig = %hex::encode(deploy.deploy_id()),
                        reason = "no_covering_parent",
                        next_block = block_number,
                        rejection_height,
                        deferral_lease_blocks = RETRY_FRONTIER_DEFERRAL_LEASE_BLOCKS,
                        selected_parent_count = casper_snapshot.parents.len(),
                        latest_message_count,
                        "deploy lifecycle"
                    );
                    deferred_sigs.insert(deploy.deploy_id().clone());
                }
            }
            retry_candidates.retain(|deploy| !deferred_sigs.contains(deploy.deploy_id()));
        }
    }
    let ordinary_candidates: HashSet<PendingDeploy> = valid_unique
        .iter()
        .filter(|deploy| !is_retry_candidate(deploy))
        .cloned()
        .collect();
    // #194: deliberately does NOT exclude `rejected_in_scope` deploys here.
    // `retry_scope_exempt` (above) only exempts a rejected-in-scope deploy from
    // `blocked_by_scope` when THIS validator also holds it in its own local
    // `recovered_sigs` buffer. A validator that only learned of the deploy via
    // gossip/another block's inclusion never populates that buffer, so without
    // this candidate route such a deploy is excluded from both recovery paths
    // and gets filtered "already in scope" forever.
    let in_scope_recovery_candidates: HashSet<PendingDeploy> = if allow_in_scope_recovery {
        already_in_scope
            .iter()
            .filter(|deploy| {
                !canonical_won.contains(deploy.typed_deploy_id())
                    && casper_snapshot
                        .deploys_in_scope
                        .contains(deploy.typed_deploy_id())
            })
            .cloned()
            .collect()
    } else {
        HashSet::new()
    };
    let retry_candidate_count = retry_candidates.len();
    let total_byte_budget = user_deploy_byte_budget(admission_policy);
    let mut remaining_byte_budget = total_byte_budget;
    let retry_selection = select_deploys_for_block(
        &retry_candidates,
        RETRY_DEPLOY_REPROPOSAL_CAP,
        false,
        remaining_byte_budget,
    );
    remaining_byte_budget = remaining_byte_budget.saturating_sub(retry_selection.selected_bytes);
    let retry_capped = retry_selection.count_capped || retry_selection.byte_capped;
    if retry_capped {
        let deferred_retries = retry_candidate_count.saturating_sub(retry_selection.deploys.len());
        tracing::info!(
            target: "f1r3fly.casper.recovery",
            "Retry deploy selection capped for block #{}: selected={}, deferred={}, cap={}, selected_bytes={}, deferred_bytes={}, byte_budget={}",
            block_number,
            retry_selection.deploys.len(),
            deferred_retries,
            RETRY_DEPLOY_REPROPOSAL_CAP,
            retry_selection.selected_bytes,
            retry_selection.deferred_bytes,
            total_byte_budget
        );
    }
    let ordinary_selection = select_deploys_for_block(
        &ordinary_candidates,
        ordinary_cap,
        admission_policy.reserve_tail,
        remaining_byte_budget,
    );
    remaining_byte_budget = remaining_byte_budget.saturating_sub(ordinary_selection.selected_bytes);
    let ordinary_capped = ordinary_selection.count_capped || ordinary_selection.byte_capped;
    let in_scope_recovery_selection = select_deploys_for_block(
        &in_scope_recovery_candidates,
        in_scope_recovery_cap,
        false,
        remaining_byte_budget,
    );
    let in_scope_recovery_capped =
        in_scope_recovery_selection.count_capped || in_scope_recovery_selection.byte_capped;
    let selected_in_scope_recovery = in_scope_recovery_selection.deploys;
    let selected_in_scope_recovery_sigs: HashSet<DeployLookupId> = selected_in_scope_recovery
        .iter()
        .map(|deploy| deploy.typed_deploy_id().clone())
        .collect();
    let selected: HashSet<PendingDeploy> = retry_selection
        .deploys
        .into_iter()
        .chain(ordinary_selection.deploys.into_iter())
        .chain(selected_in_scope_recovery.into_iter())
        .collect();
    for deploy in &selected {
        tracing::info!(
            target: "f1r3fly.casper.deploy_lifecycle",
            event = "selected",
            deploy_sig = %hex::encode(deploy.deploy_id()),
            next_block = block_number,
            retry = is_retry_candidate(deploy),
            in_scope_recovery = selected_in_scope_recovery_sigs.contains(deploy.typed_deploy_id()),
            valid_after_block = deploy.data().valid_after_block_number,
            "deploy lifecycle"
        );
    }
    let selected_user_deploy_bytes = retry_selection
        .selected_bytes
        .saturating_add(ordinary_selection.selected_bytes)
        .saturating_add(in_scope_recovery_selection.selected_bytes);
    let deferred_user_deploy_bytes = retry_selection
        .deferred_bytes
        .saturating_add(ordinary_selection.deferred_bytes)
        .saturating_add(in_scope_recovery_selection.deferred_bytes);
    let byte_cap_hit = retry_selection.byte_capped
        || ordinary_selection.byte_capped
        || in_scope_recovery_selection.byte_capped;
    let selected_retry_sigs: HashSet<DeployLookupId> = selected
        .iter()
        .filter(|deploy| is_retry_candidate(deploy))
        .map(|deploy| deploy.typed_deploy_id().clone())
        .collect();
    let selected_retry_count = selected_retry_sigs.len();
    let selected_in_scope_recovery_count = selected
        .iter()
        .filter(|deploy| selected_in_scope_recovery_sigs.contains(deploy.typed_deploy_id()))
        .count();
    let selected_ordinary_count = selected
        .iter()
        .filter(|deploy| {
            !is_retry_candidate(deploy)
                && !selected_in_scope_recovery_sigs.contains(deploy.typed_deploy_id())
        })
        .count();
    let deferred = retry_candidate_count.saturating_sub(selected_retry_count)
        + ordinary_candidates
            .len()
            .saturating_sub(selected_ordinary_count)
        + in_scope_recovery_candidates
            .len()
            .saturating_sub(selected_in_scope_recovery_count);
    let cap_hit = retry_capped || ordinary_capped || in_scope_recovery_capped;
    if ordinary_capped {
        tracing::info!(
            "Ordinary deploy selection capped for block #{}: selected={}, deferred={}, cap={}, strategy={}, selected_bytes={}, deferred_bytes={}, remaining_byte_budget={}",
            block_number,
            selected_ordinary_count,
            ordinary_candidates.len().saturating_sub(selected_ordinary_count),
            ordinary_cap,
            ordinary_selection.strategy,
            ordinary_selection.selected_bytes,
            ordinary_selection.deferred_bytes,
            total_byte_budget.saturating_sub(retry_selection.selected_bytes)
        );
    }
    if !selected_in_scope_recovery_sigs.is_empty() || in_scope_recovery_capped {
        tracing::info!(
            target: "f1r3fly.casper.recovery",
            "In-scope deploy recovery selection for block #{}: selected={}, deferred={}, cap={}, strategy={}, selected_bytes={}, deferred_bytes={}, remaining_byte_budget={}",
            block_number,
            selected_in_scope_recovery_count,
            in_scope_recovery_candidates.len().saturating_sub(selected_in_scope_recovery_count),
            in_scope_recovery_cap,
            in_scope_recovery_selection.strategy,
            in_scope_recovery_selection.selected_bytes,
            in_scope_recovery_selection.deferred_bytes,
            total_byte_budget
                .saturating_sub(retry_selection.selected_bytes)
                .saturating_sub(ordinary_selection.selected_bytes)
        );
    }
    if byte_cap_hit {
        tracing::info!(
            target: "f1r3fly.casper.recovery",
            "Deploy selection byte budget capped block #{}: selected_bytes={}, deferred_bytes={}, byte_budget={}, selected={}, deferred={}",
            block_number,
            selected_user_deploy_bytes,
            deferred_user_deploy_bytes,
            total_byte_budget,
            selected.len(),
            deferred
        );
    }

    if tracing::enabled!(target: "f1r3fly.merge.cpps", tracing::Level::DEBUG) {
        trace_deploy_filter(
            "future",
            future_deploys.iter().map(|d| d.deploy_id().as_ref()),
        );
        trace_deploy_filter(
            "block-expired",
            block_expired_deploys.iter().map(|d| d.deploy_id().as_ref()),
        );
        trace_deploy_filter(
            "time-expired",
            time_expired_deploys.iter().map(|d| d.deploy_id().as_ref()),
        );
        trace_deploy_filter(
            "already-in-scope",
            already_in_scope
                .iter()
                .filter(|d| !selected_in_scope_recovery_sigs.contains(d.typed_deploy_id()))
                .map(|d| d.deploy_id().as_ref()),
        );
        let (sample, count, omitted) =
            bounded_deploy_id_sample(valid_unique.iter().map(|d| d.deploy_id().as_ref()));
        if count > 0 {
            tracing::debug!(
                target: "f1r3fly.merge.cpps",
                step = "prepare_user_deploys.FILTER",
                decision = "selected-candidate",
                reason = "passed-expiry-and-scope-filters",
                count,
                omitted,
                deploy_sample = ?sample,
                "merge.cpps: deploy filter summary"
            );
        }
    }

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
            selected.len()
        );
    }

    // Remove all expired deploys from storage to prevent them from triggering
    // future proposals. Combine block-expired and time-expired, avoiding
    // duplicates. Removal is irreversible, so block-expiry removal of RETRY
    // work requires the floor bound — with no derivable floor, retry work is
    // excluded here and re-judged next round (delay, never loss); its
    // admission-side filter above already deferred on the same fact.
    let all_expired: HashSet<&PendingDeploy> = block_expired_deploys
        .iter()
        .filter(|d| floor_expiry_bound.is_some() || !is_retry_sig(d.typed_deploy_id()))
        .chain(time_expired_deploys.iter())
        .cloned()
        .collect();
    if !all_expired.is_empty() {
        for deploy in &all_expired {
            tracing::info!(
                target: "f1r3fly.casper.deploy_lifecycle",
                event = "storage_and_buffer_removed",
                deploy_sig = %hex::encode(deploy.deploy_id()),
                reason = "expired",
                block_expired = block_expired_deploys.iter().any(|item| item.deploy_id() == deploy.deploy_id()),
                time_expired = time_expired_deploys.iter().any(|item| item.deploy_id() == deploy.deploy_id()),
                valid_after_block = deploy.data().valid_after_block_number,
                floor_expiry_bound = ?floor_expiry_bound,
                next_block = block_number,
                "deploy lifecycle"
            );
        }
        tracing::info!(
            "Removing {} expired deploy(s) from storage and rejected-deploy buffer",
            all_expired.len()
        );
        let expired_list: Vec<PendingDeploy> = all_expired.into_iter().cloned().collect();
        for deploy in &expired_list {
            deploy_storage_guard.remove_envelope_by_id(deploy.deploy_id())?;
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

    if tracing::enabled!(target: "f1r3fly.merge.cpps", tracing::Level::DEBUG) {
        let (chosen, _, omitted) =
            bounded_deploy_id_sample(selected.iter().map(|d| d.deploy_id().as_ref()));
        tracing::debug!(
            target: "f1r3fly.merge.cpps",
            step = "prepare_user_deploys.CHOSEN",
            block_number,
            count = selected.len(),
            cap_hit,
            ordinary_cap,
            in_scope_recovery_cap,
            ordinary_strategy = ordinary_selection.strategy,
            in_scope_recovery_strategy = in_scope_recovery_selection.strategy,
            deferred,
            selected_user_deploy_bytes,
            deferred_user_deploy_bytes,
            byte_cap_hit,
            omitted,
            chosen = ?chosen,
            "merge.cpps: final deploy set chosen for block"
        );
    }

    Ok(PreparedUserDeploys {
        deploys: selected,
        effective_cap: ordinary_cap,
        cap_hit,
        selected_retry_count,
        selected_ordinary_count,
        selected_in_scope_recovery_count,
        selected_retry_sigs,
        selected_in_scope_recovery_sigs,
        already_in_scope_count,
        selected_user_deploy_bytes,
        deferred_user_deploy_bytes,
        byte_cap_hit,
    })
}

fn collect_self_chain_deploy_sigs(
    casper_snapshot: &CasperSnapshot,
    validator_identity: &ValidatorIdentity,
    block_store: &KeyValueBlockStore,
) -> Result<HashSet<DeployLookupId>, CasperError> {
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
    if casper_snapshot.dag.canonical_genesis_hash() == Some(&current_hash) {
        return Ok(HashSet::new());
    }

    let mut deploy_sigs: HashSet<DeployLookupId> = HashSet::new();
    let max_depth = std::cmp::max(casper_snapshot.on_chain_state.shard_conf.deploy_lifespan, 1);

    for _ in 0..(max_depth as usize) {
        if casper_snapshot.dag.canonical_genesis_hash() == Some(&current_hash) {
            break;
        }
        let block = block_store.get(&current_hash)?.ok_or_else(|| {
            missing_block_error("collecting self-chain deploy signatures", &current_hash)
        })?;

        for processed in &block.body.deploys {
            deploy_sigs.insert(
                processed
                    .deploy_id_for_protocol(block.header.version)
                    .map_err(CasperError::RuntimeError)?,
            );
        }

        let Some(main_parent) = block.header.parents_hash_list.first().cloned() else {
            break;
        };
        current_hash = main_parent;
    }

    Ok(deploy_sigs)
}

fn self_chain_has_unfinalized_user_deploys(
    casper_snapshot: &CasperSnapshot,
    validator_identity: &ValidatorIdentity,
    block_store: &KeyValueBlockStore,
) -> Result<bool, CasperError> {
    let self_validator = validator_identity.public_key.bytes.clone();
    let current_hash_from_justifications = casper_snapshot
        .justifications
        .iter()
        .find(|j| j.validator == self_validator)
        .map(|j| j.latest_block_hash.clone());
    let current_hash_from_dag = casper_snapshot.dag.latest_message_hash(&self_validator);

    let Some(mut current_hash) = current_hash_from_dag.or(current_hash_from_justifications) else {
        return Ok(false);
    };
    if casper_snapshot.dag.canonical_genesis_hash() == Some(&current_hash) {
        return Ok(false);
    }

    let last_finalized_block_number = block_store
        .get(&casper_snapshot.last_finalized_block)?
        .ok_or_else(|| {
            missing_block_error(
                "reading the finalized boundary for self-chain recovery",
                &casper_snapshot.last_finalized_block,
            )
        })?
        .body
        .state
        .block_number;
    let max_depth = std::cmp::max(casper_snapshot.on_chain_state.shard_conf.deploy_lifespan, 1);

    for _ in 0..(max_depth as usize) {
        if casper_snapshot.dag.canonical_genesis_hash() == Some(&current_hash) {
            break;
        }
        if current_hash == casper_snapshot.last_finalized_block
            || casper_snapshot.dag.is_finalized(&current_hash)
        {
            break;
        }

        let block = block_store.get(&current_hash)?.ok_or_else(|| {
            missing_block_error("scanning self-chain recovery state", &current_hash)
        })?;

        if block.body.state.block_number <= last_finalized_block_number {
            break;
        }

        if !block.body.deploys.is_empty() {
            return Ok(true);
        }

        let Some(main_parent) = block.header.parents_hash_list.first().cloned() else {
            break;
        };
        current_hash = main_parent;
    }

    Ok(false)
}

fn scope_has_unfinalized_user_deploys(
    casper_snapshot: &CasperSnapshot,
    block_store: &KeyValueBlockStore,
) -> Result<bool, CasperError> {
    let last_finalized_block_number = block_store
        .get(&casper_snapshot.last_finalized_block)?
        .ok_or_else(|| {
            missing_block_error(
                "reading the finalized boundary for parent-scope recovery",
                &casper_snapshot.last_finalized_block,
            )
        })?
        .body
        .state
        .block_number;
    let current_block_number = casper_snapshot
        .max_block_num
        .checked_add(1)
        .ok_or_else(|| {
            CasperError::RuntimeError(format!(
                "max_block_num overflow: {} + 1 wraps i64",
                casper_snapshot.max_block_num
            ))
        })?;
    let earliest_block_number = current_block_number
        .saturating_sub(casper_snapshot.on_chain_state.shard_conf.deploy_lifespan);
    let mut stack: Vec<BlockHash> = casper_snapshot
        .parents
        .iter()
        .map(|parent| parent.block_hash.clone())
        .collect();
    let mut seen: HashSet<BlockHash> = HashSet::new();

    while let Some(block_hash) = stack.pop() {
        if !seen.insert(block_hash.clone())
            || block_hash == casper_snapshot.last_finalized_block
            || casper_snapshot.dag.is_finalized(&block_hash)
        {
            continue;
        }

        let block = block_store.get(&block_hash)?.ok_or_else(|| {
            missing_block_error("scanning parent-scope recovery state", &block_hash)
        })?;

        if block.body.state.block_number <= last_finalized_block_number
            || block.body.state.block_number <= earliest_block_number
        {
            continue;
        }

        if !block.body.deploys.is_empty() {
            return Ok(true);
        }

        stack.extend(block.header.parents_hash_list.iter().cloned());
    }

    Ok(false)
}

fn newer_branch_deploy_info(
    current: Option<BranchDeployInfo>,
    candidate: BranchDeployInfo,
) -> Option<BranchDeployInfo> {
    match current {
        Some(existing)
            if existing.block_number > candidate.block_number
                || (existing.block_number == candidate.block_number
                    && (existing.timestamp > candidate.timestamp
                        || (existing.timestamp == candidate.timestamp
                            && existing.sender.as_ref() >= candidate.sender.as_ref()))) =>
        {
            Some(existing)
        }
        _ => Some(candidate),
    }
}

fn collect_branch_user_deploy_sigs(
    casper_snapshot: &CasperSnapshot,
    block_store: &KeyValueBlockStore,
    roots: &[BlockHash],
    earliest_block_number: i64,
    last_finalized_block_number: Option<i64>,
    excluded_block_hash: Option<&BlockHash>,
) -> Result<HashSet<DeployLookupId>, CasperError> {
    let mut stack: Vec<BlockHash> = roots.to_vec();
    let mut seen: HashSet<BlockHash> = HashSet::new();
    let mut sigs = HashSet::new();

    while let Some(block_hash) = stack.pop() {
        if !seen.insert(block_hash.clone())
            || block_hash == casper_snapshot.last_finalized_block
            || casper_snapshot.dag.is_finalized(&block_hash)
        {
            continue;
        }

        let block = block_store.get(&block_hash)?.ok_or_else(|| {
            missing_block_error("collecting visible branch deploy signatures", &block_hash)
        })?;

        if last_finalized_block_number
            .is_some_and(|lfb_number| block.body.state.block_number <= lfb_number)
            || block.body.state.block_number <= earliest_block_number
        {
            continue;
        }

        let excluded = excluded_block_hash
            .map(|excluded_hash| excluded_hash == &block_hash)
            .unwrap_or(false);
        if !excluded {
            for deploy in &block.body.deploys {
                sigs.insert(
                    deploy
                        .deploy_id_for_protocol(block.header.version)
                        .map_err(CasperError::RuntimeError)?,
                );
            }
        }

        stack.extend(block.header.parents_hash_list.iter().cloned());
    }

    Ok(sigs)
}

fn classify_branch_deploy_info(
    mut info: BranchDeployInfo,
    casper_snapshot: &CasperSnapshot,
    block_store: &KeyValueBlockStore,
    earliest_block_number: i64,
    last_finalized_block_number: Option<i64>,
) -> Result<BranchDeployInfo, CasperError> {
    let block = block_store.get(&info.block_hash)?.ok_or_else(|| {
        missing_block_error("classifying branch deploy progress", &info.block_hash)
    })?;
    let deploy_sigs: HashSet<DeployLookupId> = block
        .body
        .deploys
        .iter()
        .map(|deploy| {
            deploy
                .deploy_id_for_protocol(block.header.version)
                .map_err(CasperError::RuntimeError)
        })
        .collect::<Result<_, _>>()?;
    let parent_frontier: Vec<BlockHash> = casper_snapshot
        .parents
        .iter()
        .map(|parent| parent.block_hash.clone())
        .collect();
    let visible_sigs = collect_branch_user_deploy_sigs(
        casper_snapshot,
        block_store,
        &parent_frontier,
        earliest_block_number,
        last_finalized_block_number,
        Some(&info.block_hash),
    )?;
    let new_sig_count = deploy_sigs
        .iter()
        .filter(|sig| !visible_sigs.contains(*sig))
        .count();
    info.deploy_sig_count = deploy_sigs.len();
    info.new_sig_count = new_sig_count;
    info.recycled_sig_count = deploy_sigs.len().saturating_sub(new_sig_count);
    Ok(info)
}

fn branch_unfinalized_user_deploy_info(
    casper_snapshot: &CasperSnapshot,
    block_store: &KeyValueBlockStore,
    root_hash: &BlockHash,
) -> Result<Option<BranchDeployInfo>, CasperError> {
    let last_finalized_block_number = block_store
        .get(&casper_snapshot.last_finalized_block)?
        .ok_or_else(|| {
            missing_block_error(
                "reading the finalized boundary for branch deploy progress",
                &casper_snapshot.last_finalized_block,
            )
        })?
        .body
        .state
        .block_number;
    let current_block_number = casper_snapshot
        .max_block_num
        .checked_add(1)
        .ok_or_else(|| {
            CasperError::RuntimeError(format!(
                "max_block_num overflow: {} + 1 wraps i64",
                casper_snapshot.max_block_num
            ))
        })?;
    let earliest_block_number = current_block_number
        .saturating_sub(casper_snapshot.on_chain_state.shard_conf.deploy_lifespan);
    let mut stack = vec![root_hash.clone()];
    let mut seen: HashSet<BlockHash> = HashSet::new();
    let mut latest = None;

    while let Some(block_hash) = stack.pop() {
        if !seen.insert(block_hash.clone())
            || block_hash == casper_snapshot.last_finalized_block
            || casper_snapshot.dag.is_finalized(&block_hash)
        {
            continue;
        }

        let block = block_store
            .get(&block_hash)?
            .ok_or_else(|| missing_block_error("scanning branch deploy progress", &block_hash))?;

        if block.body.state.block_number <= last_finalized_block_number
            || block.body.state.block_number <= earliest_block_number
        {
            continue;
        }

        if !block.body.deploys.is_empty() {
            latest = newer_branch_deploy_info(latest, BranchDeployInfo {
                block_hash: block_hash.clone(),
                sender: block.sender.clone(),
                block_number: block.body.state.block_number,
                timestamp: block.header.timestamp,
                deploy_sig_count: block.body.deploys.len(),
                new_sig_count: 0,
                recycled_sig_count: 0,
            });
        }

        stack.extend(block.header.parents_hash_list.iter().cloned());
    }

    latest
        .map(|info| {
            classify_branch_deploy_info(
                info,
                casper_snapshot,
                block_store,
                earliest_block_number,
                Some(last_finalized_block_number),
            )
        })
        .transpose()
}

fn storage_has_unresolved_in_scope_deploys(
    casper_snapshot: &CasperSnapshot,
    deploy_storage: &Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    _rejected_deploy_buffer: &Arc<Mutex<KeyValueRejectedDeployBuffer>>,
) -> Result<bool, CasperError> {
    let stored_deploys = deploy_storage
        .lock()
        .read_all_for_protocol(casper_snapshot.on_chain_state.shard_conf.casper_version)?;
    for deploy in stored_deploys {
        if casper_snapshot
            .deploys_in_scope
            .contains(deploy.typed_deploy_id())
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Build one `SlashDeploy` from its canonical offender evidence. The deploy's
/// `initial_rand` seed is a pure function of `(proposer pubkey, seq_num,
/// invalid_block_hash)`, allowing every node and the replay path to recompute
/// identical randomness for the candidate selected by `prepare_slashing_deploys`.
fn build_slash_deploy(
    invalid_block_hash: &BlockHash,
    equivocation_block_hash: Option<&BlockHash>,
    proposer_public_key: &PublicKey,
    target_activation_epoch: i64,
    target_bond_generation: BondGeneration,
    seq_num: i32,
) -> SlashDeploy {
    let self_id = Bytes::copy_from_slice(&proposer_public_key.bytes);
    SlashDeploy {
        invalid_block_hash: invalid_block_hash.clone(),
        equivocation_block_hash: equivocation_block_hash.cloned(),
        pk: proposer_public_key.clone(),
        target_activation_epoch,
        target_bond_generation,
        initial_rand: system_deploy_util::generate_slash_evidence_random_seed(
            self_id,
            seq_num,
            invalid_block_hash,
            equivocation_block_hash,
        ),
    }
}

fn prepare_slashing_deploys(
    casper_snapshot: &CasperSnapshot,
    validator_identity: &ValidatorIdentity,
    proposed_block_num: i64,
    seq_num: i32,
    authority: &CanonicalSlashAuthority,
) -> Result<Vec<SlashDeploy>, CasperError> {
    let self_id = Bytes::copy_from_slice(&validator_identity.public_key.bytes);

    // An unbonded proposer cannot effect a slash (the PoS contract rejects
    // the deploy at replay time). Skip emission to avoid wasted work and to
    // satisfy the proven-correct theorem T-9.8 — see
    // docs/casper/theory/slashing/design/09-bug-fixes-and-rationale.md §9.8.
    //
    // Symmetry note: the receive-side predicate
    // `validate_received_slash_deploys` does NOT require the block sender to
    // be bonded — it only checks the slash *target* is bonded (rule 6). The
    // block-sender-bonded invariant is enforced upstream by
    // `block_sender_has_weight` (validate.rs); this proposer-side filter is
    // an optimization, not an authorization predicate. The two cannot
    // diverge in a way that admits unauthorized slashes.
    //
    // Subsumption over dev's `filter_slashable_invalid_messages`:
    // `authorized_slash_candidates` is the T-9.8 conjunctive predicate.
    // Each candidate it returns already satisfies the bonded-target +
    // active-validator conditions that dev's simpler filter checked, PLUS
    // the epoch/evidence-epoch matches that dev's filter omitted. The
    // proposer-side authorization here therefore strictly extends, not
    // replaces, dev's filter.
    let proposer_bond = authority.bond(&self_id);
    if proposer_bond <= 0 {
        return Ok(Vec::new());
    }

    let slash_candidates =
        authorized_slash_candidates(casper_snapshot, proposed_block_num, authority)?;

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
    // See docs/casper/theory/slashing/design/06-proposing-and-effect.md for
    // the full rationale.

    // Create SlashDeploy objects
    let mut slashing_deploys = Vec::new();
    for slash_candidate in slash_candidates {
        // Phase 10 (C-5): `.get()` converts the typed Epoch back to the protobuf i64.
        let slash_deploy = build_slash_deploy(
            &slash_candidate.invalid_block_hash,
            slash_candidate.equivocation_block_hash.as_ref(),
            &validator_identity.public_key,
            slash_candidate.target_activation_epoch.get(),
            slash_candidate.target_bond_generation,
            seq_num,
        );

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
) -> Result<Vec<Cosigned<DeployData>>, CasperError> {
    match dummy_deploy_opt {
        Some((private_key, term)) => {
            let deploy = construct_deploy::source_deploy_now(
                term,
                Some(private_key.clone()),
                Some(block_number - 1),
                Some(shard_id),
            )
            .map_err(|e| {
                CasperError::RuntimeError(format!("Failed to create dummy deploy: {}", e))
            })?;
            let envelope =
                Cosigned::create_single_envelope(deploy.data, Box::new(Secp256k1), private_key)
                    .map_err(|error| {
                        CasperError::RuntimeError(format!(
                            "Failed to create dummy envelope: {error}"
                        ))
                    })?;
            Ok(vec![envelope])
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
    rejected_deploy_buffer: Arc<Mutex<KeyValueRejectedDeployBuffer>>,
    protocol_version: i64,
    failure_msg: &str,
) -> Result<(bool, bool), CasperError> {
    let Some(sig) = extract_deploy_sig_from_refund_failure(failure_msg) else {
        return Ok((false, false));
    };

    // Phase 9 (A-3): deploy_storage is a parking_lot::Mutex (no poison) → `.lock()` yields
    // the guard directly; rejected_deploy_buffer is a std Mutex (map_err the poison).
    let deploy_id = DeployLookupId::from_protocol_bytes(protocol_version, &sig)
        .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
    let removed_from_deploy_storage = {
        let mut storage = deploy_storage.lock();
        match &deploy_id {
            DeployLookupId::Legacy(signature) => storage.remove_by_sig(signature.as_bytes())?,
            DeployLookupId::V6(deploy_id) => storage.remove_envelope_by_id(deploy_id.as_ref())?,
        }
    };
    let removed_from_rejected_buffer = rejected_deploy_buffer
        .lock()
        .map_err(|e| CasperError::LockError(e.to_string()))?
        .remove_by_id(&deploy_id)
        .map_err(CasperError::from)?;

    Ok((removed_from_deploy_storage, removed_from_rejected_buffer))
}

#[cfg(test)]
fn drain_selected_deploys_from_rejected_buffer(
    rejected_deploy_buffer: &Arc<Mutex<KeyValueRejectedDeployBuffer>>,
    deploys: &[PendingDeploy],
) -> Result<usize, CasperError> {
    let mut guard = rejected_deploy_buffer
        .lock()
        .map_err(|e| CasperError::LockError(e.to_string()))?;
    let mut removed = 0usize;
    for deploy in deploys {
        if guard
            .remove_by_id(deploy.typed_deploy_id())
            .map_err(CasperError::from)?
        {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Packaging a recovered deploy removes its ORDINARY deploy-storage copy
/// (the buffer is the tracking home for merge-rejected work) but deliberately
/// leaves the rejected-buffer entry in place. The packaged block is not yet
/// canonical — if fork choice leaves it behind, the buffer entry is the only
/// re-proposable copy. Buffer entries are purged only when their sig is
/// finalized-won (see the terminal purge in `prepare_user_deploys_with_policy`)
/// or expired.
fn drain_selected_recovered_deploys_from_deploy_storage(
    deploy_storage: &Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    rejected_deploy_buffer: &Arc<Mutex<KeyValueRejectedDeployBuffer>>,
    deploys: &[PendingDeploy],
) -> Result<usize, CasperError> {
    let selected_recovered: Vec<PendingDeploy> = {
        let guard = rejected_deploy_buffer
            .lock()
            .map_err(|e| CasperError::LockError(e.to_string()))?;
        let mut out = Vec::new();
        for deploy in deploys {
            if guard
                .contains_id(deploy.typed_deploy_id())
                .map_err(CasperError::from)?
            {
                out.push(deploy.clone());
            }
        }
        out
    };
    if selected_recovered.is_empty() {
        return Ok(0);
    }

    let mut removed_from_storage = 0usize;
    {
        let mut storage = deploy_storage.lock();
        for deploy in &selected_recovered {
            if storage.remove_envelope_by_id(deploy.deploy_id())? {
                tracing::info!(
                    target: "f1r3fly.casper.deploy_lifecycle",
                    event = "storage_removed",
                    deploy_sig = %hex::encode(deploy.deploy_id()),
                    reason = "recovery_carrier_packaged",
                    buffer_retained = true,
                    "deploy lifecycle"
                );
                removed_from_storage += 1;
            }
        }
    }

    Ok(removed_from_storage)
}

/// Removes the ordinary deploy-storage copies of recovered deploys whose sig
/// already shows a canonical win in the parent scope. The rejected-buffer
/// entry is NOT touched here: a canonical-but-unfinalized win can still be
/// orphaned, and the buffer entry must remain re-proposable until the win is
/// finalized (terminal purge in `prepare_user_deploys_with_policy`).
fn purge_recovered_already_in_scope(
    deploy_storage: &mut KeyValueDeployStorage,
    deploys: &[PendingDeploy],
    recovered_sigs: &HashSet<DeployLookupId>,
) -> Result<usize, CasperError> {
    let recovered_done: Vec<PendingDeploy> = deploys
        .iter()
        .filter(|deploy| recovered_sigs.contains(deploy.typed_deploy_id()))
        .cloned()
        .collect();
    if recovered_done.is_empty() {
        return Ok(0);
    }

    for deploy in &recovered_done {
        deploy_storage.remove_envelope_by_id(deploy.deploy_id())?;
    }
    Ok(recovered_done.len())
}

fn recovered_deploy_leader(
    casper_snapshot: &CasperSnapshot,
) -> Result<Option<Validator>, CasperError> {
    let validators = casper_snapshot.finalized_floor_validators();
    if validators.is_empty() {
        return Ok(None);
    }
    let finalized_height = casper_snapshot
        .dag
        .lookup_unsafe(&casper_snapshot.last_finalized_block)?
        .block_number
        .max(0) as usize;
    Ok(validators.get(finalized_height % validators.len()).cloned())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CandidateSelfChainDisposition {
    NotOnSelfChain,
    ActiveDuplicate,
    ExcludedBranchRehome,
    SelectedRecovery,
}

impl CandidateSelfChainDisposition {
    const fn should_package(self) -> bool { !matches!(self, Self::ActiveDuplicate) }
}

fn candidate_self_chain_disposition(
    on_self_chain: bool,
    active_in_candidate_scope: bool,
    selected_recovery: bool,
) -> CandidateSelfChainDisposition {
    if !on_self_chain {
        CandidateSelfChainDisposition::NotOnSelfChain
    } else if selected_recovery {
        CandidateSelfChainDisposition::SelectedRecovery
    } else if active_in_candidate_scope {
        CandidateSelfChainDisposition::ActiveDuplicate
    } else {
        CandidateSelfChainDisposition::ExcludedBranchRehome
    }
}

fn filter_self_chain_deploys(
    deploys: &mut HashSet<PendingDeploy>,
    self_chain_deploy_sigs: &HashSet<DeployLookupId>,
    candidate_scope_deploy_sigs: &dashmap::DashSet<DeployLookupId>,
    selected_recovery_sigs: &HashSet<DeployLookupId>,
) -> usize {
    let before = deploys.len();
    deploys.retain(|deploy| {
        candidate_self_chain_disposition(
            self_chain_deploy_sigs.contains(deploy.typed_deploy_id()),
            candidate_scope_deploy_sigs.contains(deploy.typed_deploy_id()),
            selected_recovery_sigs.contains(deploy.typed_deploy_id()),
        )
        .should_package()
    });
    before.saturating_sub(deploys.len())
}

fn missing_block_error(context: &str, block_hash: &BlockHash) -> CasperError {
    CasperError::RuntimeError(format!(
        "Missing block {} while {}",
        pretty_printer::PrettyPrinter::build_string_bytes(block_hash),
        context
    ))
}

fn deploy_inclusion_progress(
    casper_snapshot: &CasperSnapshot,
    block_store: &KeyValueBlockStore,
) -> Result<DeployInclusionProgress, CasperError> {
    let validators = casper_snapshot.finalized_floor_validators();
    let mut latest = None;
    for parent in &casper_snapshot.parents {
        if parent.sender.is_empty()
            || (!validators.is_empty() && !validators.iter().any(|v| v == &parent.sender))
        {
            continue;
        }

        if let Some(info) =
            branch_unfinalized_user_deploy_info(casper_snapshot, block_store, &parent.block_hash)?
        {
            latest = newer_branch_deploy_info(latest, info);
        }
    }

    if let Some(info) = latest {
        let leader = if !info.sender.is_empty()
            && (validators.is_empty() || validators.iter().any(|v| v == &info.sender))
        {
            Some(info.sender.clone())
        } else {
            recovered_deploy_leader(casper_snapshot)?
        };
        return Ok(DeployInclusionProgress {
            leader,
            latest_deploy: Some(info),
        });
    }

    if scope_has_unfinalized_user_deploys(casper_snapshot, block_store)? {
        return Ok(DeployInclusionProgress {
            leader: recovered_deploy_leader(casper_snapshot)?,
            latest_deploy: None,
        });
    }

    Ok(DeployInclusionProgress::default())
}

#[cfg(test)]
fn deploy_inclusion_progress_is_stale(
    progress: &DeployInclusionProgress,
    next_block_num: i64,
    now_millis: i64,
) -> bool {
    deploy_inclusion_progress_staleness(progress, next_block_num, now_millis).stale
}

fn deploy_inclusion_progress_staleness(
    progress: &DeployInclusionProgress,
    next_block_num: i64,
    now_millis: i64,
) -> DeployInclusionStaleness {
    match &progress.latest_deploy {
        Some(info) => {
            let block_or_time_stale = next_block_num.saturating_sub(info.block_number)
                >= DEPLOY_INCLUSION_LEASE_BLOCKS
                || now_millis.saturating_sub(info.timestamp) >= DEPLOY_INCLUSION_LEASE_MILLIS;
            let signature_stale = info.deploy_sig_count > 0 && info.new_sig_count == 0;
            DeployInclusionStaleness {
                stale: block_or_time_stale || signature_stale,
                block_or_time_stale,
                signature_stale,
                missing_deploy_metadata: false,
            }
        }
        None => DeployInclusionStaleness {
            stale: progress.leader.is_some(),
            block_or_time_stale: false,
            signature_stale: false,
            missing_deploy_metadata: progress.leader.is_some(),
        },
    }
}

fn fresh_local_deploy_stats(
    casper_snapshot: &CasperSnapshot,
    block_number: i64,
    current_time_millis: i64,
    deploy_storage: &Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    rejected_deploy_buffer: &Arc<Mutex<KeyValueRejectedDeployBuffer>>,
    block_store: &KeyValueBlockStore,
    floor_ctx: Option<&FloorContext>,
) -> Result<FreshLocalDeployStats, CasperError> {
    let stored_deploys = deploy_storage
        .lock()
        .read_all_for_protocol(casper_snapshot.on_chain_state.shard_conf.casper_version)?;
    if stored_deploys.is_empty() {
        return Ok(FreshLocalDeployStats::default());
    }
    let buffered_sigs: HashSet<DeployLookupId> = rejected_deploy_buffer
        .lock()
        .map_err(|e| CasperError::LockError(e.to_string()))?
        .read_all()?
        .into_iter()
        .map(|deploy| deploy.typed_deploy_id().clone())
        .collect();
    let earliest_block_number = crate::rust::util::deploy_window::earliest_valid_after(
        block_number,
        casper_snapshot.on_chain_state.shard_conf.deploy_lifespan,
    )?;
    let candidates: HashSet<PendingDeploy> = stored_deploys
        .into_iter()
        .filter(|deploy| {
            !buffered_sigs.contains(deploy.typed_deploy_id())
                && !casper_snapshot
                    .deploys_in_scope
                    .contains(deploy.typed_deploy_id())
                && not_future_deploy(block_number, deploy.data())
                && not_expired_deploy(earliest_block_number, deploy.data())
                && !deploy.data().is_expired_at(current_time_millis)
        })
        .collect();
    if candidates.is_empty() {
        return Ok(FreshLocalDeployStats::default());
    }
    let canonical_scan_floor = candidates
        .iter()
        .map(|d| d.data().valid_after_block_number)
        .min()
        .map(|h| h.min(earliest_block_number))
        .unwrap_or(earliest_block_number);
    let canonical_won = canonical_won_over_parents(
        floor_ctx,
        casper_snapshot,
        block_store,
        canonical_scan_floor,
    )?;
    let mut count = 0usize;
    let mut oldest_time = None;
    for deploy in candidates {
        if canonical_won.contains(deploy.typed_deploy_id()) {
            continue;
        }
        count += 1;
        oldest_time = Some(
            oldest_time
                .map(|current: i64| current.min(deploy.data().time_stamp))
                .unwrap_or(deploy.data().time_stamp),
        );
    }
    Ok(FreshLocalDeployStats {
        count,
        oldest_age_millis: oldest_time
            .map(|time_stamp| current_time_millis.saturating_sub(time_stamp))
            .unwrap_or(0),
    })
}

fn in_scope_local_deploy_stats(
    casper_snapshot: &CasperSnapshot,
    block_number: i64,
    current_time_millis: i64,
    deploy_storage: &Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    rejected_deploy_buffer: &Arc<Mutex<KeyValueRejectedDeployBuffer>>,
    block_store: &KeyValueBlockStore,
    floor_ctx: Option<&FloorContext>,
) -> Result<InScopeLocalDeployStats, CasperError> {
    let stored_deploys = deploy_storage
        .lock()
        .read_all_for_protocol(casper_snapshot.on_chain_state.shard_conf.casper_version)?;
    if stored_deploys.is_empty() {
        return Ok(InScopeLocalDeployStats::default());
    }
    let buffered_sigs: HashSet<DeployLookupId> = rejected_deploy_buffer
        .lock()
        .map_err(|e| CasperError::LockError(e.to_string()))?
        .read_all()?
        .into_iter()
        .map(|deploy| deploy.typed_deploy_id().clone())
        .collect();
    let earliest_block_number = crate::rust::util::deploy_window::earliest_valid_after(
        block_number,
        casper_snapshot.on_chain_state.shard_conf.deploy_lifespan,
    )?;
    let candidates: HashSet<PendingDeploy> = stored_deploys
        .into_iter()
        .filter(|deploy| {
            !buffered_sigs.contains(deploy.typed_deploy_id())
                && casper_snapshot
                    .deploys_in_scope
                    .contains(deploy.typed_deploy_id())
                && !casper_snapshot
                    .rejected_in_scope
                    .contains(deploy.typed_deploy_id())
                && not_future_deploy(block_number, deploy.data())
                && not_expired_deploy(earliest_block_number, deploy.data())
                && !deploy.data().is_expired_at(current_time_millis)
        })
        .collect();
    if candidates.is_empty() {
        return Ok(InScopeLocalDeployStats::default());
    }
    let canonical_scan_floor = candidates
        .iter()
        .map(|d| d.data().valid_after_block_number)
        .min()
        .map(|h| h.min(earliest_block_number))
        .unwrap_or(earliest_block_number);
    let canonical_won = canonical_won_over_parents(
        floor_ctx,
        casper_snapshot,
        block_store,
        canonical_scan_floor,
    )?;
    let mut count = 0usize;
    let mut stranded_count = 0usize;
    let mut oldest_time = None;
    for deploy in candidates {
        if canonical_won.contains(deploy.typed_deploy_id()) {
            continue;
        }
        count += 1;
        stranded_count += 1;
        oldest_time = Some(
            oldest_time
                .map(|current: i64| current.min(deploy.data().time_stamp))
                .unwrap_or(deploy.data().time_stamp),
        );
    }
    Ok(InScopeLocalDeployStats {
        count,
        oldest_age_millis: oldest_time
            .map(|time_stamp| current_time_millis.saturating_sub(time_stamp))
            .unwrap_or(0),
        stranded_count,
    })
}

/// Admission-time recoverability check: does the rejected-deploy buffer hold
/// at least one deploy worth re-proposing from THIS proposer's perspective?
/// Runs after snapshot completion, so it refines the block-number window with
/// the scope sets (clean-in-scope deploys excluded; rejected-in-scope deploys
/// keep recovery eligibility past the window).
///
/// Cost: O(1) when the buffer is empty (the common steady state); otherwise
/// one canonical-won scan bounded below by `scan_floor` (never deeper than
/// the deploy-lifespan window).
fn retry_candidate_is_ready(
    clean_in_scope: bool,
    future: bool,
    time_expired: bool,
    floor_window_expired: bool,
    terminal: bool,
) -> bool {
    !terminal && !clean_in_scope && !future && !time_expired && !floor_window_expired
}

pub(crate) fn rejected_buffer_has_recoverable_deploys(
    casper_snapshot: &CasperSnapshot,
    block_number: i64,
    current_time_millis: i64,
    rejected_deploy_buffer: &Arc<Mutex<KeyValueRejectedDeployBuffer>>,
    block_store: &KeyValueBlockStore,
    floor_ctx: Option<&FloorContext>,
) -> Result<bool, CasperError> {
    let buffered_deploys = {
        let buffer_guard = rejected_deploy_buffer
            .lock()
            .map_err(|e| CasperError::LockError(e.to_string()))?;
        if !buffer_guard.non_empty()? {
            return Ok(false);
        }
        buffer_guard.read_all()?
    };
    if buffered_deploys.is_empty() {
        return Ok(false);
    }
    let earliest_block_number = crate::rust::util::deploy_window::earliest_valid_after(
        block_number,
        casper_snapshot.on_chain_state.shard_conf.deploy_lifespan,
    )?;
    // Buffered work is retry work: its window reads the FLOOR clock, so a
    // floor-window-closed entry no longer counts as a recovery backlog and
    // cannot hold admission in recovery mode.
    let window_bound = floor_ctx
        .map(|ctx| {
            crate::rust::util::deploy_window::earliest_valid_after(
                ctx.floor.block_number,
                casper_snapshot.on_chain_state.shard_conf.deploy_lifespan,
            )
        })
        .transpose()?
        .unwrap_or(earliest_block_number);
    let mut candidates = Vec::new();
    for deploy in &buffered_deploys {
        let terminal = casper_snapshot
            .dag
            .deploy_terminal(deploy.typed_deploy_id())?
            .is_some();
        let eligible = {
            let rejected_in_scope = casper_snapshot
                .rejected_in_scope
                .contains(deploy.typed_deploy_id());
            let clean_in_scope = casper_snapshot
                .deploys_in_scope
                .contains(deploy.typed_deploy_id())
                && !rejected_in_scope;
            retry_candidate_is_ready(
                clean_in_scope,
                !not_future_deploy(block_number, deploy.data()),
                deploy.data().is_expired_at(current_time_millis),
                !not_expired_deploy(window_bound, deploy.data()),
                terminal,
            )
        };
        if eligible {
            candidates.push(deploy);
        }
    }
    if candidates.is_empty() {
        return Ok(false);
    }
    let scan_floor = candidates
        .iter()
        .map(|d| d.data().valid_after_block_number)
        .min()
        .map(|h| h.min(earliest_block_number))
        .unwrap_or(earliest_block_number);
    let canonical_won =
        canonical_won_over_parents(floor_ctx, casper_snapshot, block_store, scan_floor)?;

    Ok(candidates
        .iter()
        .any(|deploy| !canonical_won.contains(deploy.typed_deploy_id())))
}

fn finality_lag_stats(
    casper_snapshot: &CasperSnapshot,
    block_store: &KeyValueBlockStore,
) -> Result<FinalityLagStats, CasperError> {
    let last_finalized_block = block_store
        .get(&casper_snapshot.last_finalized_block)?
        .ok_or_else(|| {
            missing_block_error(
                "computing finality-lag admission state",
                &casper_snapshot.last_finalized_block,
            )
        })?
        .body
        .state
        .block_number;
    let dag_tip = casper_snapshot.max_block_num;
    Ok(FinalityLagStats {
        dag_tip,
        last_finalized_block,
        lag: dag_tip.saturating_sub(last_finalized_block).max(0),
    })
}

fn adaptive_fallback_ordinary_deploy_cap(
    casper_snapshot: &CasperSnapshot,
    fresh_local_stats: FreshLocalDeployStats,
    finality_lag_stats: FinalityLagStats,
) -> (usize, bool) {
    let normal_cap = normal_ordinary_deploy_cap(casper_snapshot);
    if normal_cap == 0 {
        return (0, false);
    }
    let backpressure = finality_lag_stats.lag >= FINALITY_LAG_SOFT_BACKPRESSURE_BLOCKS;
    let cap = if finality_lag_stats.lag >= FINALITY_LAG_HARD_BACKPRESSURE_BLOCKS {
        NON_LEADER_FALLBACK_MIN_ORDINARY_DEPLOY_CAP
    } else if backpressure {
        NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP
    } else if fresh_local_stats.oldest_age_millis
        >= FRESH_DEPLOY_MAX_ESCALATED_ADMISSION_DELAY_MILLIS
    {
        NON_LEADER_FALLBACK_MAX_ORDINARY_DEPLOY_CAP
    } else if fresh_local_stats.oldest_age_millis >= FRESH_DEPLOY_ESCALATED_ADMISSION_DELAY_MILLIS {
        NON_LEADER_FALLBACK_MEDIUM_ORDINARY_DEPLOY_CAP
    } else {
        NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP
    };
    (normal_cap.min(cap), backpressure)
}

fn adaptive_normal_ordinary_deploy_cap(
    casper_snapshot: &CasperSnapshot,
    stale_in_scope_work: bool,
    deploy_inclusion_staleness: DeployInclusionStaleness,
    finality_lag_stats: FinalityLagStats,
) -> (usize, bool) {
    let normal_cap = normal_ordinary_deploy_cap(casper_snapshot);
    if normal_cap == 0 {
        return (0, false);
    }
    let cap = if finality_lag_stats.lag >= FINALITY_LAG_HARD_BACKPRESSURE_BLOCKS
        || (stale_in_scope_work && deploy_inclusion_staleness.signature_stale)
    {
        NON_LEADER_FALLBACK_MIN_ORDINARY_DEPLOY_CAP
    } else if finality_lag_stats.lag >= FINALITY_LAG_SOFT_BACKPRESSURE_BLOCKS
        || (stale_in_scope_work && deploy_inclusion_staleness.stale)
    {
        NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP
    } else {
        normal_cap
    };
    let effective = normal_cap.min(cap);
    (effective, effective < normal_cap)
}

fn fresh_admission_fallback(
    casper_snapshot: &CasperSnapshot,
    _stale_in_scope_work: bool,
    _deploy_inclusion_staleness: DeployInclusionStaleness,
    fresh_local_stats: FreshLocalDeployStats,
    finality_lag_stats: FinalityLagStats,
) -> FreshAdmissionFallback {
    if fresh_local_stats.count == 0 {
        return FreshAdmissionFallback::default();
    }
    let (cap, backpressure) = adaptive_fallback_ordinary_deploy_cap(
        casper_snapshot,
        fresh_local_stats,
        finality_lag_stats,
    );
    FreshAdmissionFallback {
        allowed: cap > 0,
        cap,
        backpressure,
    }
}

fn in_scope_recovery_fallback(
    casper_snapshot: &CasperSnapshot,
    stale_in_scope_work: bool,
    deploy_inclusion_staleness: DeployInclusionStaleness,
    in_scope_local_stats: InScopeLocalDeployStats,
    finality_lag_stats: FinalityLagStats,
) -> FreshAdmissionFallback {
    let has_stranded_work = in_scope_local_stats.stranded_count > 0;
    if !stale_in_scope_work
        || in_scope_local_stats.count == 0
        || (!has_stranded_work
            && in_scope_local_stats.oldest_age_millis < FRESH_DEPLOY_MAX_ADMISSION_DELAY_MILLIS)
        || (!has_stranded_work
            && !deploy_inclusion_staleness.stale
            && finality_lag_stats.lag < FINALITY_LAG_HARD_BACKPRESSURE_BLOCKS)
    {
        return FreshAdmissionFallback::default();
    }
    let (cap, backpressure) = adaptive_fallback_ordinary_deploy_cap(
        casper_snapshot,
        FreshLocalDeployStats {
            count: in_scope_local_stats.count,
            oldest_age_millis: in_scope_local_stats.oldest_age_millis,
        },
        finality_lag_stats,
    );
    FreshAdmissionFallback {
        allowed: cap > 0,
        cap,
        backpressure,
    }
}

fn ordinary_admission_policy(
    casper_snapshot: &CasperSnapshot,
    rejected_buffer_non_empty: bool,
    allow_recovered_deploys: bool,
    stale_in_scope_work: bool,
    allow_deploy_inclusion: bool,
    fallback: FreshAdmissionFallback,
    in_scope_recovery: FreshAdmissionFallback,
    deploy_inclusion_staleness: DeployInclusionStaleness,
    finality_lag_stats: FinalityLagStats,
) -> DeployAdmissionPolicy {
    let (normal_cap, normal_backpressure) = adaptive_normal_ordinary_deploy_cap(
        casper_snapshot,
        stale_in_scope_work,
        deploy_inclusion_staleness,
        finality_lag_stats,
    );
    let requires_fallback = (rejected_buffer_non_empty && !allow_recovered_deploys)
        || (stale_in_scope_work && !allow_deploy_inclusion);
    if requires_fallback {
        return DeployAdmissionPolicy {
            allow_ordinary: fallback.allowed,
            ordinary_cap: fallback.cap,
            allow_in_scope_recovery: in_scope_recovery.allowed,
            in_scope_recovery_cap: in_scope_recovery.cap,
            reserve_tail: false,
            fallback: fallback.allowed || in_scope_recovery.allowed,
            backpressure: fallback.backpressure || in_scope_recovery.backpressure,
        };
    }

    DeployAdmissionPolicy {
        allow_ordinary: true,
        ordinary_cap: normal_cap,
        allow_in_scope_recovery: in_scope_recovery.allowed,
        in_scope_recovery_cap: in_scope_recovery.cap,
        reserve_tail: !normal_backpressure,
        fallback: in_scope_recovery.allowed,
        backpressure: normal_backpressure || in_scope_recovery.backpressure,
    }
}

fn metric_bool(value: bool) -> f64 {
    if value {
        1.0
    } else {
        0.0
    }
}

fn record_deploy_admission_metrics(
    fresh_local_stats: FreshLocalDeployStats,
    in_scope_local_stats: InScopeLocalDeployStats,
    finality_lag_stats: FinalityLagStats,
    inclusion_progress: &DeployInclusionProgress,
    inclusion_staleness: DeployInclusionStaleness,
    admission_policy: DeployAdmissionPolicy,
    prepared: &PreparedUserDeploys,
) {
    use crate::rust::metrics_constants::{
        BLOCK_CREATOR_DEPLOY_ADMISSION_ALREADY_IN_SCOPE_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_BACKPRESSURE_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_BLOCK_TIME_STALE_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_BYTE_CAP_HIT_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_DAG_TIP_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_DEFERRED_USER_BYTES_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_FALLBACK_CAP_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_FALLBACK_ENABLED_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_FRESH_LOCAL_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_IN_SCOPE_LOCAL_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_LFB_LAG_METRIC, BLOCK_CREATOR_DEPLOY_ADMISSION_LFB_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_MISSING_PROGRESS_METADATA_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_OLDEST_FRESH_AGE_MS_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_OLDEST_IN_SCOPE_AGE_MS_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_PROGRESS_NEW_SIGS_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_PROGRESS_RECYCLED_SIGS_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_SELECTED_IN_SCOPE_RECOVERY_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_SELECTED_ORDINARY_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_SELECTED_RETRY_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_SELECTED_USER_BYTES_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_SIGNATURE_STALE_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_STRANDED_IN_SCOPE_METRIC,
        BLOCK_CREATOR_DEPLOY_ADMISSION_USER_BYTE_BUDGET_METRIC, CASPER_METRICS_SOURCE,
    };
    let (progress_new_sigs, progress_recycled_sigs) = inclusion_progress
        .latest_deploy
        .as_ref()
        .map(|info| (info.new_sig_count, info.recycled_sig_count))
        .unwrap_or((0, 0));
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_FRESH_LOCAL_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(fresh_local_stats.count as f64);
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_OLDEST_FRESH_AGE_MS_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(fresh_local_stats.oldest_age_millis as f64);
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_IN_SCOPE_LOCAL_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(in_scope_local_stats.count as f64);
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_STRANDED_IN_SCOPE_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(in_scope_local_stats.stranded_count as f64);
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_OLDEST_IN_SCOPE_AGE_MS_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(in_scope_local_stats.oldest_age_millis as f64);
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_ALREADY_IN_SCOPE_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(prepared.already_in_scope_count as f64);
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_SELECTED_ORDINARY_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(prepared.selected_ordinary_count as f64);
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_SELECTED_RETRY_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(prepared.selected_retry_count as f64);
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_SELECTED_IN_SCOPE_RECOVERY_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(prepared.selected_in_scope_recovery_count as f64);
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_SELECTED_USER_BYTES_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(prepared.selected_user_deploy_bytes as f64);
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_DEFERRED_USER_BYTES_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(prepared.deferred_user_deploy_bytes as f64);
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_USER_BYTE_BUDGET_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(user_deploy_byte_budget(admission_policy) as f64);
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_BYTE_CAP_HIT_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(metric_bool(prepared.byte_cap_hit));
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_FALLBACK_ENABLED_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(metric_bool(admission_policy.fallback));
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_FALLBACK_CAP_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(if admission_policy.fallback {
        admission_policy
            .ordinary_cap
            .max(admission_policy.in_scope_recovery_cap) as f64
    } else {
        0.0
    });
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_BACKPRESSURE_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(metric_bool(admission_policy.backpressure));
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_DAG_TIP_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(finality_lag_stats.dag_tip as f64);
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_LFB_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(finality_lag_stats.last_finalized_block as f64);
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_LFB_LAG_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(finality_lag_stats.lag as f64);
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_PROGRESS_NEW_SIGS_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(progress_new_sigs as f64);
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_PROGRESS_RECYCLED_SIGS_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(progress_recycled_sigs as f64);
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_SIGNATURE_STALE_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(metric_bool(inclusion_staleness.signature_stale));
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_BLOCK_TIME_STALE_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(metric_bool(inclusion_staleness.block_or_time_stale));
    metrics::gauge!(
        BLOCK_CREATOR_DEPLOY_ADMISSION_MISSING_PROGRESS_METADATA_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .set(metric_bool(inclusion_staleness.missing_deploy_metadata));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CertifiedContextRelation {
    Ready,
    MaterializationPending,
    FloorRegression,
    FloorConflict,
    ContextMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CertifiedContextComparison {
    relation: CertifiedContextRelation,
    candidate_descends_from_materialized: bool,
    materialized_descends_from_candidate: bool,
    candidate_preserves_materialized_state: Option<bool>,
}

fn compare_certified_contexts(
    dag: &block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation,
    block_store: &KeyValueBlockStore,
    materialized: &crate::rust::causal_equivocation::CertifiedConsensusContext,
    candidate: &crate::rust::causal_equivocation::CertifiedConsensusContext,
) -> Result<CertifiedContextComparison, CasperError> {
    if materialized == candidate {
        return Ok(CertifiedContextComparison {
            relation: CertifiedContextRelation::Ready,
            candidate_descends_from_materialized: true,
            materialized_descends_from_candidate: true,
            candidate_preserves_materialized_state: Some(true),
        });
    }

    let materialized_floor = materialized.incoming_finalized_floor();
    let candidate_floor = candidate.incoming_finalized_floor();
    if materialized_floor == candidate_floor {
        return Ok(CertifiedContextComparison {
            relation: CertifiedContextRelation::ContextMismatch,
            candidate_descends_from_materialized: true,
            materialized_descends_from_candidate: true,
            candidate_preserves_materialized_state: Some(
                materialized.incoming_finalized_floor_post_state_hash()
                    == candidate.incoming_finalized_floor_post_state_hash(),
            ),
        });
    }

    let candidate_descends_from_materialized =
        dag.is_dag_ancestor(materialized_floor, candidate_floor)?;
    let materialized_descends_from_candidate =
        dag.is_dag_ancestor(candidate_floor, materialized_floor)?;
    let candidate_preserves_materialized_state = if candidate_descends_from_materialized {
        Some(crate::rust::finality::floor::state_contains(
            dag,
            block_store,
            &crate::rust::finality::floor::Floor {
                hash: candidate_floor.clone(),
                block_number: dag.lookup_unsafe(candidate_floor)?.block_number,
            },
            &crate::rust::finality::floor::Floor {
                hash: materialized_floor.clone(),
                block_number: dag.lookup_unsafe(materialized_floor)?.block_number,
            },
            &mut crate::rust::finality::floor::StateContainmentMemo::new(),
        )?)
    } else {
        None
    };
    let relation = if candidate_descends_from_materialized
        && candidate_preserves_materialized_state == Some(true)
    {
        CertifiedContextRelation::MaterializationPending
    } else if materialized_descends_from_candidate {
        CertifiedContextRelation::FloorRegression
    } else {
        CertifiedContextRelation::FloorConflict
    };
    Ok(CertifiedContextComparison {
        relation,
        candidate_descends_from_materialized,
        materialized_descends_from_candidate,
        candidate_preserves_materialized_state,
    })
}

fn proposal_recovery_deferral_reason(
    context_relation: CertifiedContextRelation,
    candidate_slots_complete: bool,
    proposer_active: bool,
) -> Option<RecoveryDeferralReason> {
    match context_relation {
        CertifiedContextRelation::Ready if !candidate_slots_complete => {
            Some(RecoveryDeferralReason::IncompleteCandidateCommitteeSlots)
        }
        CertifiedContextRelation::Ready if !proposer_active => {
            Some(RecoveryDeferralReason::InactiveCandidateValidator)
        }
        CertifiedContextRelation::Ready => None,
        CertifiedContextRelation::MaterializationPending => {
            Some(RecoveryDeferralReason::FinalizedFloorMaterializationPending)
        }
        CertifiedContextRelation::FloorRegression => {
            Some(RecoveryDeferralReason::CandidateFloorRegression)
        }
        CertifiedContextRelation::FloorConflict => {
            Some(RecoveryDeferralReason::CandidateFloorConflict)
        }
        CertifiedContextRelation::ContextMismatch => {
            Some(RecoveryDeferralReason::CertifiedContextMismatch)
        }
    }
}

trait CheckpointAttemptObserver {
    fn before_attempt(&mut self, _user_deploy_limit: usize, _deploys: &[Cosigned<DeployData>]) {}

    fn complete_attempt(
        &mut self,
        result: Result<interpreter_util::DeploysCheckpoint, CasperError>,
    ) -> Result<interpreter_util::DeploysCheckpoint, CasperError> {
        result
    }
}

struct LiveCheckpointAttemptObserver;

impl CheckpointAttemptObserver for LiveCheckpointAttemptObserver {}

#[cfg(feature = "test-utils")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointAttemptTrace {
    pub user_deploy_limit: usize,
    pub deploy_ids: Vec<DeployLookupId>,
}

#[cfg(feature = "test-utils")]
#[derive(Default)]
struct ForcedCheckpointRetryObserver {
    forced: bool,
    attempts: Vec<CheckpointAttemptTrace>,
}

#[cfg(feature = "test-utils")]
impl CheckpointAttemptObserver for ForcedCheckpointRetryObserver {
    fn before_attempt(&mut self, user_deploy_limit: usize, deploys: &[Cosigned<DeployData>]) {
        self.attempts.push(CheckpointAttemptTrace {
            user_deploy_limit,
            deploy_ids: deploys
                .iter()
                .map(crate::rust::util::rholang::acceptance::admission_deploy_id)
                .collect(),
        });
    }

    fn complete_attempt(
        &mut self,
        result: Result<interpreter_util::DeploysCheckpoint, CasperError>,
    ) -> Result<interpreter_util::DeploysCheckpoint, CasperError> {
        if !self.forced && result.is_ok() {
            self.forced = true;
            Err(CasperError::RuntimeError(
                "number channel forced-checkpoint-retry holds 2 values [0, 0]; IntegerAdd single-value invariant violated"
                    .to_string(),
            ))
        } else {
            result
        }
    }
}

pub async fn create(
    casper_snapshot: &CasperSnapshot,
    validator_identity: &ValidatorIdentity,
    dummy_deploy_opt: Option<(PrivateKey, String)>,
    deploy_storage: Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    rejected_deploy_buffer: Arc<Mutex<block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer>>,
    runtime_manager: &RuntimeManager,
    block_store: &mut KeyValueBlockStore,
    allow_empty_blocks: bool,
) -> Result<BlockCreatorResult, CasperError> {
    let mut observer = LiveCheckpointAttemptObserver;
    create_with_checkpoint_attempt_observer(
        casper_snapshot,
        validator_identity,
        dummy_deploy_opt,
        deploy_storage,
        rejected_deploy_buffer,
        runtime_manager,
        block_store,
        allow_empty_blocks,
        &mut observer,
    )
    .await
}

#[cfg(feature = "test-utils")]
#[allow(clippy::too_many_arguments)]
pub async fn create_with_forced_checkpoint_retry(
    casper_snapshot: &CasperSnapshot,
    validator_identity: &ValidatorIdentity,
    dummy_deploy_opt: Option<(PrivateKey, String)>,
    deploy_storage: Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    rejected_deploy_buffer: Arc<Mutex<KeyValueRejectedDeployBuffer>>,
    runtime_manager: &RuntimeManager,
    block_store: &mut KeyValueBlockStore,
    allow_empty_blocks: bool,
) -> Result<(BlockCreatorResult, Vec<CheckpointAttemptTrace>), CasperError> {
    let mut observer = ForcedCheckpointRetryObserver::default();
    let result = create_with_checkpoint_attempt_observer(
        casper_snapshot,
        validator_identity,
        dummy_deploy_opt,
        deploy_storage,
        rejected_deploy_buffer,
        runtime_manager,
        block_store,
        allow_empty_blocks,
        &mut observer,
    )
    .await?;
    Ok((result, observer.attempts))
}

#[allow(clippy::too_many_arguments)]
async fn create_with_checkpoint_attempt_observer<O: CheckpointAttemptObserver>(
    casper_snapshot: &CasperSnapshot,
    validator_identity: &ValidatorIdentity,
    dummy_deploy_opt: Option<(PrivateKey, String)>,
    deploy_storage: Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    rejected_deploy_buffer: Arc<Mutex<KeyValueRejectedDeployBuffer>>,
    runtime_manager: &RuntimeManager,
    block_store: &mut KeyValueBlockStore,
    allow_empty_blocks: bool,
    checkpoint_attempt_observer: &mut O,
) -> Result<BlockCreatorResult, CasperError> {
    let _heap_boundary = BlockCreationHeapBoundary;
    if casper_snapshot.on_chain_state.shard_conf.casper_version
        >= crate::rust::casper::CERTIFIED_FINALIZED_FLOOR_PROTOCOL_VERSION
        && casper_snapshot.finalized_floor_certificate.is_none()
    {
        return Err(CasperError::RuntimeError(
            "protocol-v6 proposal snapshot has no finalized-floor certificate".to_string(),
        ));
    }
    use crate::rust::metrics_constants::{
        BLOCK_CREATOR_COMPUTE_DEPLOYS_CHECKPOINT_TIME_METRIC,
        BLOCK_CREATOR_COMPUTE_PARENTS_POST_STATE_TIME_METRIC,
        BLOCK_CREATOR_PACKAGE_BLOCK_TIME_METRIC, BLOCK_CREATOR_PACKED_BLOCK_BYTES_METRIC,
        BLOCK_CREATOR_PREPARE_USER_DEPLOYS_TIME_METRIC, BLOCK_CREATOR_TOTAL_TIME_METRIC,
        CASPER_METRICS_SOURCE,
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
    if !parents.is_empty() {
        let latest_messages = justifications
            .iter()
            .map(|justification| {
                (
                    justification.validator.clone(),
                    justification.latest_block_hash.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let candidate_context =
            crate::rust::causal_equivocation::CertifiedConsensusContext::for_frozen_floor(
                &casper_snapshot.dag,
                casper_snapshot.last_finalized_block.clone(),
                &latest_messages,
            )?;
        let context_comparison = compare_certified_contexts(
            &casper_snapshot.dag,
            block_store,
            &casper_snapshot.consensus_context,
            &candidate_context,
        )?;
        if context_comparison.relation != CertifiedContextRelation::Ready {
            tracing::warn!(
                relation = ?context_comparison.relation,
                materialized_floor = %hex::encode(casper_snapshot.consensus_context.incoming_finalized_floor()),
                candidate_floor = %hex::encode(candidate_context.incoming_finalized_floor()),
                materialized_context = %hex::encode(casper_snapshot.consensus_context.digest()),
                candidate_context = %hex::encode(candidate_context.digest()),
                candidate_descends_from_materialized = context_comparison.candidate_descends_from_materialized,
                materialized_descends_from_candidate = context_comparison.materialized_descends_from_candidate,
                candidate_preserves_materialized_state = ?context_comparison.candidate_preserves_materialized_state,
                "candidate consensus context is not proposal-ready"
            );
        }
        if let Some(reason) = proposal_recovery_deferral_reason(
            context_comparison.relation,
            candidate_context.has_complete_latest_message_slots(),
            candidate_context
                .active_validators()
                .contains(&validator_identity.public_key.bytes),
        ) {
            return Ok(BlockCreatorResult::recovery_deferred(reason));
        }
    }
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

    // The one derivation of the floor (and its post-state) for this whole
    // propose; every walk and probe below reads it. `None` only for
    // parentless fixture shapes, where the floor requirement surfaces at
    // bonds packaging exactly as before.
    let floor_ctx = derive_floor_context(casper_snapshot, block_store).await?;

    // Prepare deploys
    let user_deploys = {
        let t = std::time::Instant::now();
        let user_deploys_in_scope =
            scope_has_unfinalized_user_deploys(casper_snapshot, block_store)?;
        let storage_deploys_in_scope = storage_has_unresolved_in_scope_deploys(
            casper_snapshot,
            &deploy_storage,
            &rejected_deploy_buffer,
        )?;
        let self_chain_user_deploys = self_chain_has_unfinalized_user_deploys(
            casper_snapshot,
            validator_identity,
            block_store,
        )?;
        let stale_in_scope_work =
            user_deploys_in_scope || storage_deploys_in_scope || self_chain_user_deploys;
        let user_work_in_flight = stale_in_scope_work;
        let self_chain_deploy_sigs =
            collect_self_chain_deploy_sigs(casper_snapshot, validator_identity, block_store)?;
        let inclusion_progress = deploy_inclusion_progress(casper_snapshot, block_store)?;
        let allow_deploy_inclusion = inclusion_progress
            .leader
            .as_ref()
            .map(|leader| leader == &validator_identity.public_key.bytes)
            .unwrap_or(true);
        let inclusion_staleness =
            deploy_inclusion_progress_staleness(&inclusion_progress, next_block_num, now_millis);
        let finality_lag_stats = finality_lag_stats(casper_snapshot, block_store)?;
        let fresh_local_stats = fresh_local_deploy_stats(
            casper_snapshot,
            next_block_num,
            now_millis,
            &deploy_storage,
            &rejected_deploy_buffer,
            block_store,
            floor_ctx.as_ref(),
        )?;
        let in_scope_local_stats = in_scope_local_deploy_stats(
            casper_snapshot,
            next_block_num,
            now_millis,
            &deploy_storage,
            &rejected_deploy_buffer,
            block_store,
            floor_ctx.as_ref(),
        )?;
        let fallback = fresh_admission_fallback(
            casper_snapshot,
            stale_in_scope_work,
            inclusion_staleness,
            fresh_local_stats,
            finality_lag_stats,
        );
        let in_scope_recovery = in_scope_recovery_fallback(
            casper_snapshot,
            stale_in_scope_work,
            inclusion_staleness,
            in_scope_local_stats,
            finality_lag_stats,
        );
        let rejected_buffer_non_empty = rejected_buffer_has_recoverable_deploys(
            casper_snapshot,
            next_block_num,
            now_millis,
            &rejected_deploy_buffer,
            block_store,
            floor_ctx.as_ref(),
        )?;
        let allow_recovered_deploys = rejected_buffer_non_empty;
        if rejected_buffer_non_empty {
            tracing::info!(
                target: "f1r3fly.casper.deploy_lifecycle",
                event = "recovery_custody",
                proposer = %hex::encode(&validator_identity.public_key.bytes),
                held = allow_recovered_deploys,
                next_block = next_block_num,
                "deploy lifecycle"
            );
        }
        let admission_policy = ordinary_admission_policy(
            casper_snapshot,
            rejected_buffer_non_empty,
            allow_recovered_deploys,
            stale_in_scope_work,
            allow_deploy_inclusion,
            fallback,
            in_scope_recovery,
            inclusion_staleness,
            finality_lag_stats,
        );
        if user_work_in_flight && !allow_deploy_inclusion && !admission_policy.allow_ordinary {
            tracing::info!(
                target: "f1r3fly.casper.recovery",
                "Ordinary user deploy selection deferred to deploy-inclusion leader for block #{}; proposing finality support only: scope={}, storage_scope={}, self_chain={}",
                next_block_num,
                user_deploys_in_scope,
                storage_deploys_in_scope,
                self_chain_user_deploys
            );
        }
        if (stale_in_scope_work || rejected_buffer_non_empty)
            && admission_policy.allow_ordinary
            && !admission_policy.reserve_tail
        {
            tracing::info!(
                target: "f1r3fly.casper.recovery",
                "Ordinary user deploy fallback enabled for block #{}: cap={}, fresh_local={}, oldest_fresh_age_ms={}, in_scope_local={}, stranded_in_scope={}, oldest_in_scope_age_ms={}, inclusion_progress_stale={}, signature_stale={}, lfb_lag={}, backpressure={}",
                next_block_num,
                admission_policy.ordinary_cap,
                fresh_local_stats.count,
                fresh_local_stats.oldest_age_millis,
                in_scope_local_stats.count,
                in_scope_local_stats.stranded_count,
                in_scope_local_stats.oldest_age_millis,
                inclusion_staleness.stale,
                inclusion_staleness.signature_stale,
                finality_lag_stats.lag,
                admission_policy.backpressure
            );
        }
        if admission_policy.allow_in_scope_recovery {
            tracing::info!(
                target: "f1r3fly.casper.recovery",
                "In-scope deploy recovery enabled for block #{}: cap={}, in_scope_local={}, stranded_in_scope={}, oldest_in_scope_age_ms={}, inclusion_progress_stale={}, signature_stale={}, lfb_lag={}, backpressure={}",
                next_block_num,
                admission_policy.in_scope_recovery_cap,
                in_scope_local_stats.count,
                in_scope_local_stats.stranded_count,
                in_scope_local_stats.oldest_age_millis,
                inclusion_staleness.stale,
                inclusion_staleness.signature_stale,
                finality_lag_stats.lag,
                admission_policy.backpressure
            );
        }
        if user_work_in_flight && admission_policy.allow_ordinary && allow_deploy_inclusion {
            tracing::info!(
                target: "f1r3fly.casper.recovery",
                "Ordinary user deploy selection remains enabled for block #{} while user deploy work is in flight; per-deploy scope filters will suppress duplicates: scope={}, storage_scope={}",
                next_block_num,
                user_deploys_in_scope,
                storage_deploys_in_scope
            );
        }
        let prepared = prepare_user_deploys_with_policy(
            casper_snapshot,
            next_block_num,
            now_millis,
            deploy_storage.clone(),
            rejected_deploy_buffer.clone(),
            block_store,
            allow_recovered_deploys,
            admission_policy,
            floor_ctx.as_ref(),
        )
        .await?;
        record_deploy_admission_metrics(
            fresh_local_stats,
            in_scope_local_stats,
            finality_lag_stats,
            &inclusion_progress,
            inclusion_staleness,
            admission_policy,
            &prepared,
        );
        let selected_recovery_sigs: HashSet<DeployLookupId> = prepared
            .selected_retry_sigs
            .iter()
            .chain(prepared.selected_in_scope_recovery_sigs.iter())
            .cloned()
            .collect();
        let mut v = prepared.deploys;
        if !self_chain_deploy_sigs.is_empty() {
            let skipped = filter_self_chain_deploys(
                &mut v,
                &self_chain_deploy_sigs,
                &casper_snapshot.deploys_in_scope,
                &selected_recovery_sigs,
            );
            if skipped > 0 {
                tracing::info!(
                    "Filtered {} deploy(s) already active in the selected-parent closure and self latest-message chain",
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
        v
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
    // The user deploys (gated by the WD-D2 acceptance gate below) are kept
    // `prepare_user_deploys` already removed deploys in scope. User and
    // validator-heartbeat deploys enter the same funding gate below.
    let __merge_pre_t = std::time::Instant::now();
    let latest_messages: BTreeMap<Validator, BlockHash> = casper_snapshot
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
        floor_ctx.as_ref(),
        Some(&validator_identity.public_key.bytes),
    )
    .await?;
    metrics::histogram!(
        BLOCK_CREATOR_COMPUTE_PARENTS_POST_STATE_TIME_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .record(__merge_pre_t.elapsed().as_secs_f64());
    let pre_state = merge_pre_info.state;

    let slash_authority = if has_slash_evidence(casper_snapshot) {
        Some(CanonicalSlashAuthority::load(runtime_manager, &pre_state).await?)
    } else {
        None
    };
    let slashing_deploys = {
        let t = std::time::Instant::now();
        let v = match slash_authority.as_ref() {
            Some(authority) => prepare_slashing_deploys(
                casper_snapshot,
                validator_identity,
                next_block_num,
                next_seq_num,
                authority,
            )?,
            None => Vec::new(),
        };
        tracing::debug!(
            target: "f1r3fly.block_creator.timing",
            "prepare_slashing_deploys_ms={}, slashing_deploys_count={}",
            t.elapsed().as_millis(),
            v.len()
        );
        v
    };

    let has_slashing_deploys = !slashing_deploys.is_empty();
    let system_deploys_converted: Vec<SystemDeployEnum> = slashing_deploys
        .into_iter()
        .map(SystemDeployEnum::Slash)
        .collect();
    let slashed_hashes: Vec<models::rust::block_hash::BlockHash> = system_deploys_converted
        .iter()
        .filter_map(|sd| sd.as_slash().map(|s| s.invalid_block_hash.clone()))
        .collect();
    let invalid_blocks = crate::rust::util::proto_util::slashed_block_senders(
        &casper_snapshot.dag,
        &slashed_hashes,
    )?;
    let block_data = BlockData {
        time_stamp: now_millis,
        block_number: next_block_num,
        sender: validator_identity.public_key.clone(),
        seq_num: next_seq_num,
    };

    // ── WD-D2 acceptance gate (cost-accounted-rho §7.6/§7.7) ─────────────────
    let user_deploys_for_gate: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>> =
        user_deploys
            .into_iter()
            .map(PendingDeploy::into_envelope)
            .collect();
    let mut deploys_for_gate = user_deploys_for_gate.clone();
    let dummy_sigs: std::collections::BTreeSet<DeployLookupId> = dummy_deploys
        .iter()
        .map(crate::rust::util::rholang::acceptance::admission_deploy_id)
        .collect();
    deploys_for_gate.extend(dummy_deploys.iter().cloned());

    // Run the state-bound funding proof on every signed deploy against the merged
    // pre-state before settlement. The bounded execution re-imposes canonical
    // order, records the exact root/cost chain, rejects capacity exhaustion,
    // and iterates after underfunded removals until its evidence describes the
    // exact retained sequence. The final retained execution is the block's user
    // transition; checkpoint construction continues from its witnessed root.
    let gate_admission = {
        let t = std::time::Instant::now();
        let admission = runtime_manager
            .certify_state_bound_admission(
                &pre_state,
                deploys_for_gate,
                &block_data,
                &invalid_blocks,
            )
            .await?;
        tracing::debug!(
            target: "f1r3fly.block_creator.timing",
            "acceptance_gate_ms={}, admitted={}, gate_rejected={}, debit_pools={}",
            t.elapsed().as_millis(),
            admission.outcome().admitted.len(),
            admission.outcome().rejected.len(),
            admission.outcome().debits.len()
        );
        admission
    };
    let gate_outcome = gate_admission.outcome().clone();
    let gate_rejected_sigs: Vec<DeployLookupId> = gate_outcome
        .rejected
        .iter()
        .filter(|sig| !dummy_sigs.contains(*sig))
        .cloned()
        .collect();
    let gate_rejected_sig_set: HashSet<DeployLookupId> =
        gate_rejected_sigs.iter().cloned().collect();
    let gate_rejected_user_cosigned: Vec<_> = user_deploys_for_gate
        .iter()
        .filter(|deploy| {
            gate_rejected_sig_set
                .contains(&crate::rust::util::rholang::acceptance::admission_deploy_id(deploy))
        })
        .cloned()
        .collect();
    let (admitted_dummy_cosigned, admitted_user_cosigned): (Vec<_>, Vec<_>) =
        gate_outcome.admitted.into_iter().partition(|deploy| {
            dummy_sigs
                .contains(&crate::rust::util::rholang::acceptance::admission_deploy_id(deploy))
        });
    // Whether there is any user work surviving the gate (drives the post-gate
    // empty-block skip below).
    let has_admitted_user_deploys = !admitted_user_cosigned.is_empty();

    // Check if we have any new work to process.
    // If empty blocks are disabled, skip closeBlock-only proposals to avoid no-op checkpoint cost.
    // If empty blocks are enabled (heartbeat/liveness mode), continue and emit closeBlock.
    // POST-GATE empty-block skip: a terminal funding-rejection record is user
    // work even when no deploy executes, so clients can observe a finalized
    // rejection instead of polling Pending until expiry.
    let has_user_or_dummy_deploys = has_admitted_user_deploys
        || !gate_rejected_user_cosigned.is_empty()
        || !admitted_dummy_cosigned.is_empty();
    if !has_user_or_dummy_deploys && !has_slashing_deploys && !allow_empty_blocks {
        tracing::info!(
            "Skipping empty block creation: no funded user deploys (gate-admitted={}, gate-rejected={}), no dummy deploys, no authorized slashing deploys",
            admitted_user_cosigned.len(),
            gate_rejected_sigs.len()
        );
        return Ok(BlockCreatorResult::NoNewDeploys);
    }

    // Use the adjusted `now_millis` captured at the start of create for block timestamp.
    // The value is clamped to the max parent timestamp to avoid InvalidTimestamp from clock skew.
    // This ensures the same time is used for deploy filtering and block creation.
    // Compute checkpoint data — route through the multi-sig-aware path
    // (compute_deploys_checkpoint_cosigned) so cosigner data survives from
    // submission through execution. The deploys fed to execution are the
    // WD-D2-GATE-ADMITTED envelopes (already reconstructed as
    // Cosigned<DeployData> by `admit_by_funding`, in canonical order — only
    // funded deploys execute, gate-before-execute per tex 1726-1729).
    let checkpoint_started = std::time::Instant::now();
    let original_admitted_user_deploys = admitted_user_cosigned.len();
    let mut user_deploy_limit = original_admitted_user_deploys;
    let mut retry_count = 0usize;
    let checkpoint_data = loop {
        let attempt_admitted: Vec<crypto::rust::signatures::signed::Cosigned<DeployData>> =
            admitted_user_cosigned
                .iter()
                .take(user_deploy_limit)
                .cloned()
                .collect();
        let (attempt_deploys, attempt_admission) = if user_deploy_limit
            == original_admitted_user_deploys
        {
            let mut deploys = attempt_admitted.clone();
            deploys.extend(admitted_dummy_cosigned.iter().cloned());
            crate::rust::util::rholang::acceptance::canonical_sort(&mut deploys);
            (deploys, gate_admission.clone())
        } else {
            let mut prefix_deploys = attempt_admitted.clone();
            prefix_deploys.extend(admitted_dummy_cosigned.iter().cloned());
            let prefix_admission = runtime_manager
                .certify_state_bound_admission(
                    &pre_state,
                    prefix_deploys,
                    &block_data,
                    &invalid_blocks,
                )
                .await?;
            let prefix_outcome = prefix_admission.outcome();
            if prefix_outcome.admitted.len()
                != attempt_admitted.len() + admitted_dummy_cosigned.len()
                || !prefix_outcome.rejected.is_empty()
            {
                return Err(CasperError::RuntimeError(format!(
                        "prefix re-admission diverged while shrinking the checkpoint batch: prefix_len={}, re-admitted={}, re-rejected={} (admission must be prefix-closed)",
                        attempt_admitted.len(),
                        prefix_outcome.admitted.len(),
                        prefix_outcome.rejected.len()
                    )));
            }
            (prefix_outcome.admitted.clone(), prefix_admission)
        };
        let mut attempt_system_deploys = system_deploys_converted.clone();
        attempt_system_deploys.push(SystemDeployEnum::Close(CloseBlockDeploy::new(
            system_deploy_util::generate_close_deploy_random_seed_from_pk(
                validator_identity.public_key.clone(),
                next_seq_num,
            ),
        )));
        let cosigned_deploys = attempt_deploys;
        let attempted_user_deploys = user_deploy_limit;
        let attempted_total_deploys = cosigned_deploys.len();
        checkpoint_attempt_observer.before_attempt(user_deploy_limit, &cosigned_deploys);

        let checkpoint_attempt =
            interpreter_util::compute_deploys_checkpoint_cosigned_admitted_with_effects(
                block_store,
                parents.clone(),
                cosigned_deploys,
                attempt_system_deploys,
                casper_snapshot,
                runtime_manager,
                block_data.clone(),
                invalid_blocks.clone(),
                Some(&rejected_deploy_buffer),
                floor_ctx.as_ref(),
                Some(&validator_identity.public_key.bytes),
                attempt_admission,
            )
            .await;
        match checkpoint_attempt_observer.complete_attempt(checkpoint_attempt) {
            Ok(data) => {
                if attempted_user_deploys < original_admitted_user_deploys {
                    tracing::warn!(
                        "Checkpoint merge recovered by reducing selected user deploys for block #{}: original_user_deploys={}, included_user_deploys={}, dummy_deploys={}, retries={}",
                        next_block_num,
                        original_admitted_user_deploys,
                        attempted_user_deploys,
                        dummy_deploys.len(),
                        retry_count
                    );
                }
                break data;
            }
            Err(CasperError::SystemRuntimeError(
                SystemDeployPlatformFailure::GasPaymentFailure(msg)
                | SystemDeployPlatformFailure::GasRefundFailure(msg),
            )) => {
                let retry_err = CasperError::SystemRuntimeError(
                    SystemDeployPlatformFailure::GasRefundFailure(msg.clone()),
                );
                if let Some(next_limit) = next_checkpoint_retry_limit(&retry_err, user_deploy_limit)
                {
                    retry_count += 1;
                    tracing::warn!(
                        "Checkpoint payment accounting failed for selected deploy batch in block #{}; retrying with fewer user deploys: attempted_user_deploys={}, attempted_total_deploys={}, next_user_deploys={}, error={}",
                        next_block_num,
                        attempted_user_deploys,
                        attempted_total_deploys,
                        next_limit,
                        msg
                    );
                    user_deploy_limit = next_limit;
                    continue;
                }
                let (removed_from_deploy_storage, removed_from_rejected_buffer) =
                    quarantine_refund_failure_deploy(
                        deploy_storage.clone(),
                        rejected_deploy_buffer.clone(),
                        casper_snapshot.on_chain_state.shard_conf.casper_version,
                        &msg,
                    )?;
                tracing::warn!(
                    "Gas payment accounting failure during checkpoint; quarantined_toxic_deploy_storage={} quarantined_toxic_rejected_buffer={} error={}",
                    removed_from_deploy_storage,
                    removed_from_rejected_buffer,
                    msg
                );
                return Ok(BlockCreatorResult::NoNewDeploys);
            }
            Err(err) if is_retryable_single_value_batch_error(&err) => {
                let Some(next_limit) = next_checkpoint_retry_limit(&err, user_deploy_limit) else {
                    return Err(err);
                };
                retry_count += 1;
                tracing::warn!(
                    "Checkpoint merge rejected selected deploy batch for block #{}; retrying with fewer user deploys: attempted_user_deploys={}, attempted_total_deploys={}, next_user_deploys={}, error={}",
                    next_block_num,
                    attempted_user_deploys,
                    attempted_total_deploys,
                    next_limit,
                    err
                );
                user_deploy_limit = next_limit;
            }
            Err(err) => return Err(err),
        }
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

    let interpreter_util::DeploysCheckpoint {
        pre_state_hash,
        post_state_hash,
        deploys: mut processed_deploys,
        rejected_deploys,
        rejected_state_effects,
        applied_state_effects,
        system_deploys: processed_system_deploys,
        bonds: new_bonds,
        applied_from_scope,
        merge_base,
    } = checkpoint_data;
    let packaged_gate_rejections = if user_deploy_limit == original_admitted_user_deploys {
        gate_rejected_user_cosigned
    } else {
        Vec::new()
    };
    processed_deploys.extend(
        packaged_gate_rejections
            .iter()
            .map(|deploy| ProcessedDeploy::admission_rejected(deploy, pre_state_hash.clone())),
    );
    let block_bonds = new_bonds;
    let mut bond_generations = runtime_manager
        .compute_bond_generations(&post_state_hash)
        .await?
        .into_iter()
        .map(|(validator, generation)| {
            Ok(ValidatorBondGeneration {
                validator,
                generation: BondGeneration::try_from(generation).map_err(|error| {
                    CasperError::RuntimeError(format!(
                        "PoS returned an invalid bond generation while packaging a block: {error}"
                    ))
                })?,
            })
        })
        .collect::<Result<Vec<_>, CasperError>>()?;
    bond_generations.sort_unstable();
    let mut active_validators = runtime_manager
        .get_active_validators(&post_state_hash)
        .await?;
    active_validators.sort_unstable();
    active_validators.dedup();
    let sender_bond_generation = casper_snapshot
        .consensus_context
        .authority_generations()
        .get(&validator_identity.public_key.bytes)
        .copied()
        .ok_or_else(|| {
            CasperError::RuntimeError(
                "proposer is absent from the certified finalized-floor generation map".to_string(),
            )
        })?;
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
    let parent_hashes = parents
        .iter()
        .map(|parent| parent.block_hash.clone())
        .collect::<Vec<_>>();
    let evidence_roots = parent_hashes
        .iter()
        .chain(
            justifications
                .iter()
                .map(|justification| &justification.latest_block_hash),
        )
        .cloned()
        .collect::<Vec<_>>();
    let objective_equivocation_evidence_delta =
        crate::rust::causal_equivocation::proposer_evidence_delta(
            &evidence_roots,
            &casper_snapshot.dag,
        )?;
    let unsigned_block = package_block(
        &block_data,
        parent_hashes,
        justifications.clone(),
        pre_state_hash,
        post_state_hash,
        processed_deploys,
        rejected_deploys,
        rejected_state_effects,
        applied_state_effects,
        processed_system_deploys,
        block_bonds,
        applied_from_scope,
        merge_base,
        bond_generations,
        active_validators,
        sender_bond_generation,
        objective_equivocation_evidence_delta,
        casper_snapshot.consensus_context.digest().clone(),
        casper_snapshot.finalized_floor_certificate.clone(),
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
    let signed_block_bytes = signed_block.to_proto().encoded_len();
    metrics::gauge!(BLOCK_CREATOR_PACKED_BLOCK_BYTES_METRIC, "source" => CASPER_METRICS_SOURCE)
        .set(signed_block_bytes as f64);

    let selected_user_deploys_for_buffer_drain: Vec<PendingDeploy> = admitted_user_cosigned
        .iter()
        .take(user_deploy_limit)
        .chain(packaged_gate_rejections.iter())
        .cloned()
        .map(PendingDeploy::from_envelope_v6)
        .collect::<Result<Vec<_>, _>>()
        .map_err(CasperError::RuntimeError)?;
    let removed_recovered_from_storage = drain_selected_recovered_deploys_from_deploy_storage(
        &deploy_storage,
        &rejected_deploy_buffer,
        &selected_user_deploys_for_buffer_drain,
    )?;
    if removed_recovered_from_storage > 0 {
        tracing::info!(
            target: "f1r3fly.casper.recovery",
            "Removed {} selected recovered deploy(s) from ordinary deploy storage after packaging block #{}; rejected-buffer entries retained until finalized-won",
            removed_recovered_from_storage,
            next_block_num
        );
    }
    tracing::event!(tracing::Level::DEBUG, mark = "block-signed");

    let block_info = pretty_printer::PrettyPrinter::build_string_block_message(&signed_block, true);
    let deploy_count = signed_block.body.deploys.len();
    tracing::debug!("Block created: {} ({}d)", block_info, deploy_count);
    let total_create_block_ms = create_started.elapsed().as_millis();

    tracing::debug!(
        target: "f1r3fly.block_creator.timing",
        "Block creator timing: package_ms={}, sign_ms={}, packed_block_bytes={}, total_create_block_ms={}",
        package_ms,
        sign_ms,
        signed_block_bytes,
        total_create_block_ms
    );
    metrics::histogram!(
        BLOCK_CREATOR_TOTAL_TIME_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .record(create_started.elapsed().as_secs_f64());

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
    rejected_deploys: Vec<RejectedDeploy>,
    rejected_state_effects: Vec<StateEffectId>,
    applied_state_effects: Vec<StateEffectId>,
    system_deploys: Vec<ProcessedSystemDeploy>,
    bonds_map: Vec<Bond>,
    applied_from_scope: Vec<Bytes>,
    merge_base: Option<BlockHash>,
    bond_generations: Vec<ValidatorBondGeneration>,
    active_validators: Vec<Validator>,
    sender_bond_generation: BondGeneration,
    objective_equivocation_evidence_delta: Vec<ObjectiveEquivocationEvidence>,
    candidate_authority_context_digest: Bytes,
    finalized_floor_certificate: Option<FinalizationCertificate>,
    shard_id: String,
    version: i64,
) -> BlockMessage {
    let state = F1r3flyState {
        pre_state_hash,
        post_state_hash,
        bonds: bonds_map,
        bond_generations,
        active_validators,
        block_number: block_data.block_number,
    };

    let body = Body {
        state,
        deploys,
        rejected_deploys,
        rejected_state_effects,
        applied_state_effects,
        system_deploys,
        extra_bytes: Bytes::new(),
        applied_from_scope,
        // None = header-derivable (single-parent, genesis): recorded as
        // empty bytes per the field contract.
        merge_base: merge_base.unwrap_or_default(),
    };

    let finalized_floor = finalized_floor_certificate
        .as_ref()
        .map(|certificate| certificate.commitment(candidate_authority_context_digest));
    let header = Header {
        parents_hash_list: parents,
        timestamp: block_data.time_stamp,
        version,
        extra_bytes: Bytes::new(),
        sender_bond_generation: Some(sender_bond_generation),
        objective_equivocation_evidence_delta,
        finalized_floor,
    };
    let mut block = proto_util::unsigned_block_proto(
        body,
        header,
        justifications,
        shard_id,
        Some(block_data.seq_num),
    );
    block.finalized_floor_certificate = finalized_floor_certificate;
    block
}

fn not_expired_deploy(earliest_block_number: i64, deploy_data: &DeployData) -> bool {
    deploy_data.valid_after_block_number > earliest_block_number
}

fn not_future_deploy(current_block_number: i64, deploy_data: &DeployData) -> bool {
    deploy_data.valid_after_block_number < current_block_number
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::*;

    #[test]
    fn deploy_log_samples_are_bounded_and_report_omissions() {
        let ids = (0u8..16).map(|value| vec![value; 32]).collect::<Vec<_>>();
        let (sample, count, omitted) = bounded_deploy_id_sample(ids.iter().map(Vec::as_slice));

        assert_eq!(count, 16);
        assert_eq!(sample.len(), DEPLOY_LOG_SAMPLE_LIMIT);
        assert_eq!(omitted, 8);
        assert_eq!(sample[0], "0000000000000000");
        assert_eq!(sample[7], "0707070707070707");
    }

    fn current_envelope(
        deploy: &crypto::rust::signatures::signed::Signed<DeployData>,
    ) -> Cosigned<DeployData> {
        construct_deploy::envelope_from_deploy_data(deploy.data.clone(), None)
            .expect("protocol-v6 envelope")
    }

    fn pending(deploy: crypto::rust::signatures::signed::Signed<DeployData>) -> PendingDeploy {
        PendingDeploy::from_envelope_v6(current_envelope(&deploy)).expect("pending deploy")
    }

    fn processed(deploy: crypto::rust::signatures::signed::Signed<DeployData>) -> ProcessedDeploy {
        ProcessedDeploy::empty_from_cosigned(&current_envelope(&deploy))
    }

    fn current_id(deploy: &crypto::rust::signatures::signed::Signed<DeployData>) -> DeployLookupId {
        pending(deploy.clone()).typed_deploy_id().clone()
    }

    fn current_id_bytes(deploy: &crypto::rust::signatures::signed::Signed<DeployData>) -> Bytes {
        pending(deploy.clone()).deploy_id().clone()
    }

    fn seed_current_deploys<'a>(
        deploy_storage: &Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
        deploys: impl IntoIterator<Item = &'a crypto::rust::signatures::signed::Signed<DeployData>>,
    ) {
        let mut storage = deploy_storage.lock();
        for deploy in deploys {
            storage
                .add_envelope_if_absent(current_envelope(deploy))
                .expect("seed protocol-v6 deploy storage");
        }
    }

    fn legacy_pending(
        deploy: crypto::rust::signatures::signed::Signed<DeployData>,
    ) -> PendingDeploy {
        PendingDeploy::from_legacy(deploy).expect("legacy pending deploy")
    }

    fn legacy_id(deploy: &crypto::rust::signatures::signed::Signed<DeployData>) -> DeployLookupId {
        legacy_sig_id(&deploy.sig)
    }

    fn legacy_sig_id(sig: &Bytes) -> DeployLookupId {
        DeployLookupId::Legacy(models::rust::deploy_id::LegacyDeploySignature::new(
            sig.to_vec(),
        ))
    }

    fn current_rejected(
        deploy: &crypto::rust::signatures::signed::Signed<DeployData>,
        source: BlockHash,
        reason: models::rust::casper::protocol::casper_message::RejectedDeployReason,
    ) -> RejectedDeploy {
        let DeployLookupId::V6(deploy_id) = current_id(deploy) else {
            unreachable!("current deploy identity")
        };
        RejectedDeploy::occurrence_v6(deploy_id, source, reason)
    }

    fn validator(byte: u8) -> Validator { Bytes::from(vec![byte; models::rust::validator::LENGTH]) }

    fn invalid_block_hash(byte: u8) -> BlockHash { Bytes::from(vec![byte; 32]) }

    fn validator_identity(byte: u8) -> ValidatorIdentity {
        ValidatorIdentity {
            public_key: PublicKey::new(validator(byte)),
            private_key: PrivateKey::from_bytes(&[byte; 32]),
            signature_algorithm: "test".to_string(),
        }
    }

    fn is_recovered_deploy_leader(
        casper_snapshot: &CasperSnapshot,
        validator_identity: &ValidatorIdentity,
    ) -> Result<bool, CasperError> {
        Ok(recovered_deploy_leader(casper_snapshot)?
            .map(|leader| leader == validator_identity.public_key.bytes)
            .unwrap_or(false))
    }

    #[test]
    fn proposal_recovery_deferral_classification_is_total_and_precedence_ordered() {
        use RecoveryDeferralReason::{
            CandidateFloorConflict, CandidateFloorRegression, CertifiedContextMismatch,
            FinalizedFloorMaterializationPending, InactiveCandidateValidator,
            IncompleteCandidateCommitteeSlots,
        };

        let cases = [
            (
                CertifiedContextRelation::MaterializationPending,
                false,
                false,
                Some(FinalizedFloorMaterializationPending),
            ),
            (
                CertifiedContextRelation::FloorRegression,
                true,
                true,
                Some(CandidateFloorRegression),
            ),
            (
                CertifiedContextRelation::FloorConflict,
                true,
                true,
                Some(CandidateFloorConflict),
            ),
            (
                CertifiedContextRelation::ContextMismatch,
                true,
                true,
                Some(CertifiedContextMismatch),
            ),
            (
                CertifiedContextRelation::Ready,
                false,
                false,
                Some(IncompleteCandidateCommitteeSlots),
            ),
            (
                CertifiedContextRelation::Ready,
                false,
                true,
                Some(IncompleteCandidateCommitteeSlots),
            ),
            (
                CertifiedContextRelation::Ready,
                true,
                false,
                Some(InactiveCandidateValidator),
            ),
            (CertifiedContextRelation::Ready, true, true, None),
        ];

        for (context_relation, slots_complete, proposer_active, expected) in cases {
            assert_eq!(
                proposal_recovery_deferral_reason(
                    context_relation,
                    slots_complete,
                    proposer_active,
                ),
                expected
            );
        }
    }

    fn set_test_committee(snapshot: &mut CasperSnapshot, validators: Vec<Validator>) {
        snapshot.finalized_floor_bonds = validators
            .iter()
            .cloned()
            .map(|validator| Bond {
                validator,
                stake: 1,
            })
            .collect();
        snapshot.on_chain_state.active_validators = validators;
    }

    fn set_last_finalized_height(snapshot: &mut CasperSnapshot, height: i64) {
        let hash = invalid_block_hash(height as u8);
        let active_validator_set = snapshot
            .finalized_floor_bonds
            .iter()
            .map(|bond| bond.validator.clone())
            .collect();
        snapshot.dag.dag_set.insert(hash.clone());
        snapshot
            .dag
            .block_metadata_index
            .write()
            .add(crate::rust::test_metadata::certify(
                models::rust::block_metadata::BlockMetadata {
                    block_hash: hash.clone(),
                    post_state_hash: invalid_block_hash(height as u8),
                    parents: Vec::new(),
                    sender: validator(0),
                    justifications: Vec::new(),
                    weight_map: BTreeMap::new(),
                    bond_generation_map: BTreeMap::new(),
                    active_validator_set,
                    block_number: height,
                    sequence_number: height as i32,
                    admission_outcome: None,
                    directly_finalized: true,
                    finalized: true,
                    fault_tolerance_value: 1.0,
                    successful_state_effect_indices: Default::default(),
                    rejected_state_effects: Default::default(),
                    applied_state_effects: Default::default(),
                    protocol_version: crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
                    objective_equivocation_evidence_delta: Vec::new(),
                    sender_authority: None,
                    finalized_floor_commitment: None,
                    admission_schema_version:
                        models::rust::block_metadata::ADMISSION_SCHEMA_VERSION,
                    approved_genesis: false,
                    merge_base: Bytes::new(),
                },
                models::rust::bond_generation::BondGeneration::GENESIS,
            ))
            .expect("insert finalized metadata");
        snapshot.last_finalized_block = hash;
    }

    fn seed_empty_last_finalized_block(
        snapshot: &mut CasperSnapshot,
        block_store: &KeyValueBlockStore,
    ) {
        let block = test_block(
            invalid_block_hash(0xf0),
            validator(1),
            Vec::new(),
            0,
            Vec::new(),
        );
        block_store
            .put_block_message(&block)
            .expect("store last finalized block");
        snapshot.last_finalized_block = block.block_hash;
    }

    fn test_block(
        hash: BlockHash,
        sender: Validator,
        parents: Vec<BlockHash>,
        block_number: i64,
        deploys: Vec<ProcessedDeploy>,
    ) -> BlockMessage {
        let mut block = package_block(
            &BlockData {
                time_stamp: block_number,
                block_number,
                sender: PublicKey::new(sender.clone()),
                seq_num: block_number as i32,
            },
            parents,
            Vec::new(),
            Bytes::from(vec![0; models::rust::block_hash::LENGTH]),
            hash.clone(),
            deploys,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            Vec::new(),
            Vec::new(),
            BondGeneration::GENESIS,
            Vec::new(),
            Bytes::from(vec![0; models::rust::block_hash::LENGTH]),
            None,
            "test".to_string(),
            crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
        );
        block.sender = sender;
        block.block_hash = hash;
        let floor_hash = block
            .header
            .parents_hash_list
            .first()
            .cloned()
            .unwrap_or_else(|| block.block_hash.clone());
        let target = models::rust::block_hash::BlockHashSerde(floor_hash.clone());
        let zero = models::rust::block_hash::BlockHashSerde(Bytes::from(vec![
            0;
            models::rust::block_hash::LENGTH
        ]));
        let manifest = BTreeSet::from([target.clone()]);
        let certificate = FinalizationCertificate {
            schema_version: FinalizationCertificate::SCHEMA_VERSION,
            protocol_version: crate::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION,
            shard_id: "test".to_string(),
            genesis_hash: target.clone(),
            predecessor_floor_hash: target.clone(),
            predecessor_certificate_digest: zero.clone(),
            predecessor_certificate_block_hash: zero,
            target_floor_hash: target.clone(),
            target_post_state_hash: target.clone(),
            target_block_number: block_number.saturating_sub(1).max(0),
            fault_tolerance_numerator: 0,
            fault_tolerance_denominator: 1,
            exact_latest_messages: BTreeMap::from([(
                models::rust::validator::ValidatorSerde(block.sender.clone()),
                target,
            )]),
            authority_context_digest: models::rust::block_hash::BlockHashSerde(Bytes::from(
                vec![2; models::rust::block_hash::LENGTH],
            )),
            supporting_manifest_digest: FinalizationCertificate::supporting_digest(&manifest),
            finalized_manifest_digest: FinalizationCertificate::finalized_digest(&manifest),
            supporting_block_count: 1,
            finalized_block_count: 1,
        };
        block.header.finalized_floor =
            Some(certificate.commitment(Bytes::from(vec![3; models::rust::block_hash::LENGTH])));
        block.finalized_floor_certificate = Some(certificate);
        block
    }

    fn fallback(allowed: bool, cap: usize) -> FreshAdmissionFallback {
        FreshAdmissionFallback {
            allowed,
            cap,
            backpressure: false,
        }
    }

    fn lag(dag_tip: i64, last_finalized_block: i64) -> FinalityLagStats {
        FinalityLagStats {
            dag_tip,
            last_finalized_block,
            lag: dag_tip.saturating_sub(last_finalized_block).max(0),
        }
    }

    fn branch_info(byte: u8, sender: Validator, block_number: i64) -> BranchDeployInfo {
        BranchDeployInfo {
            block_hash: invalid_block_hash(byte),
            sender,
            block_number,
            timestamp: block_number,
            deploy_sig_count: 1,
            new_sig_count: 1,
            recycled_sig_count: 0,
        }
    }

    #[tokio::test]
    async fn self_chain_user_deploy_gate_tracks_unfinalized_user_block_behind_empty_latest() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        let validator_id = validator(1);
        let identity = validator_identity(1);
        set_test_committee(&mut snapshot, vec![validator_id.clone()]);
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 10;
        snapshot
            .on_chain_state
            .bonds_map
            .insert(validator_id.clone(), 1);

        let lfb_hash = invalid_block_hash(0x11);
        let user_hash = invalid_block_hash(0x22);
        let empty_hash = invalid_block_hash(0x33);
        let advanced_lfb_hash = invalid_block_hash(0x44);
        let lfb = test_block(
            lfb_hash.clone(),
            validator_id.clone(),
            Vec::new(),
            1,
            Vec::new(),
        );
        let deploy = crate::rust::util::construct_deploy::basic_deploy_data(
            1,
            None,
            Some("test".to_string()),
        )
        .expect("deploy");
        let user = test_block(
            user_hash.clone(),
            validator_id.clone(),
            vec![lfb_hash.clone()],
            2,
            vec![processed(deploy)],
        );
        let empty = test_block(
            empty_hash.clone(),
            validator_id,
            vec![user_hash.clone()],
            3,
            Vec::new(),
        );
        let advanced_lfb = test_block(
            advanced_lfb_hash.clone(),
            identity.public_key.bytes.clone(),
            vec![lfb_hash.clone()],
            4,
            Vec::new(),
        );

        block_store.put_block_message(&lfb).expect("put lfb");
        block_store.put_block_message(&user).expect("put user");
        block_store.put_block_message(&empty).expect("put empty");
        block_store
            .put_block_message(&advanced_lfb)
            .expect("put advanced lfb");
        snapshot.dag.latest_messages_map = snapshot
            .dag
            .latest_messages_map
            .update(identity.public_key.bytes.clone(), empty_hash.clone());
        snapshot.last_finalized_block = lfb_hash.clone();

        assert_eq!(
            snapshot.dag.latest_message_hash(&identity.public_key.bytes),
            Some(empty_hash.clone())
        );
        assert_eq!(
            block_store
                .get(&user_hash)
                .expect("read user block")
                .expect("user block")
                .body
                .deploys
                .len(),
            1
        );
        assert_eq!(
            block_store
                .get(&empty_hash)
                .expect("read empty block")
                .expect("empty block")
                .header
                .parents_hash_list
                .first()
                .cloned(),
            Some(user_hash.clone())
        );

        assert!(
            self_chain_has_unfinalized_user_deploys(&snapshot, &identity, &block_store)
                .expect("detect unfinalized user deploy")
        );

        let user_sig = block_store
            .get(&user_hash)
            .expect("read user block")
            .expect("user block")
            .body
            .deploys
            .first()
            .expect("deploy")
            .deploy
            .sig
            .clone();
        snapshot.rejected_in_scope.insert(legacy_sig_id(&user_sig));
        assert!(
            self_chain_has_unfinalized_user_deploys(&snapshot, &identity, &block_store)
                .expect("treat rejected self-chain user deploy as unresolved")
        );
        snapshot.rejected_in_scope.remove(&legacy_sig_id(&user_sig));

        snapshot.last_finalized_block = advanced_lfb_hash;
        assert!(
            !self_chain_has_unfinalized_user_deploys(&snapshot, &identity, &block_store)
                .expect("self-chain user deploy below lfb height")
        );

        snapshot.last_finalized_block = lfb_hash;
        snapshot.dag.finalized_blocks_set.insert(user_hash.clone());
        assert!(
            !self_chain_has_unfinalized_user_deploys(&snapshot, &identity, &block_store)
                .expect("side-branch user deploy finalized")
        );

        snapshot.dag.finalized_blocks_set.remove(&user_hash);
        snapshot.last_finalized_block = user_hash;
        assert!(
            !self_chain_has_unfinalized_user_deploys(&snapshot, &identity, &block_store)
                .expect("user deploy finalized")
        );
    }

    #[tokio::test]
    async fn scope_user_deploy_gate_tracks_unfinalized_non_rejected_parent_deploys() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        let validator_id = validator(1);
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 10;

        let lfb_hash = invalid_block_hash(0x51);
        let user_hash = invalid_block_hash(0x52);
        let empty_hash = invalid_block_hash(0x53);
        let lfb = test_block(
            lfb_hash.clone(),
            validator_id.clone(),
            Vec::new(),
            1,
            Vec::new(),
        );
        let deploy = crate::rust::util::construct_deploy::basic_deploy_data(
            1,
            None,
            Some("test".to_string()),
        )
        .expect("deploy");
        let user_sig = deploy.sig.clone();
        let user = test_block(
            user_hash.clone(),
            validator_id.clone(),
            vec![lfb_hash.clone()],
            2,
            vec![processed(deploy)],
        );
        let empty = test_block(
            empty_hash.clone(),
            validator_id,
            vec![user_hash.clone()],
            3,
            Vec::new(),
        );

        block_store.put_block_message(&lfb).expect("put lfb");
        block_store.put_block_message(&user).expect("put user");
        block_store.put_block_message(&empty).expect("put empty");
        snapshot.last_finalized_block = lfb_hash.clone();
        snapshot.max_block_num = 3;
        snapshot.parents = vec![empty.clone()];

        assert!(scope_has_unfinalized_user_deploys(&snapshot, &block_store)
            .expect("detect parent-scope user deploy"));

        snapshot.rejected_in_scope.insert(legacy_sig_id(&user_sig));
        assert!(scope_has_unfinalized_user_deploys(&snapshot, &block_store)
            .expect("treat rejected parent-scope user deploy as unresolved"));
        snapshot.rejected_in_scope.remove(&legacy_sig_id(&user_sig));

        snapshot.dag.finalized_blocks_set.insert(user_hash.clone());
        assert!(!scope_has_unfinalized_user_deploys(&snapshot, &block_store)
            .expect("ignore finalized parent-scope user deploy"));
        snapshot.dag.finalized_blocks_set.remove(&user_hash);

        snapshot.last_finalized_block = user_hash;
        assert!(!scope_has_unfinalized_user_deploys(&snapshot, &block_store)
            .expect("ignore deploy at last finalized boundary"));
    }

    #[tokio::test]
    async fn storage_user_deploy_gate_tracks_unresolved_in_scope_storage_deploys() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        let deploy = crate::rust::util::construct_deploy::basic_deploy_data(
            1,
            None,
            Some("test".to_string()),
        )
        .expect("deploy");

        assert!(!storage_has_unresolved_in_scope_deploys(
            &snapshot,
            &deploy_storage,
            &rejected_deploy_buffer
        )
        .expect("empty storage"));

        seed_current_deploys(&deploy_storage, [&deploy]);
        assert!(!storage_has_unresolved_in_scope_deploys(
            &snapshot,
            &deploy_storage,
            &rejected_deploy_buffer
        )
        .expect("stored deploy not yet in scope"));

        snapshot.deploys_in_scope.insert(current_id(&deploy));
        assert!(storage_has_unresolved_in_scope_deploys(
            &snapshot,
            &deploy_storage,
            &rejected_deploy_buffer
        )
        .expect("stored deploy in scope"));

        let pending_deploy = pending(deploy.clone());
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![pending_deploy.clone()])
            .expect("park deploy in rejected buffer");
        assert!(storage_has_unresolved_in_scope_deploys(
            &snapshot,
            &deploy_storage,
            &rejected_deploy_buffer
        )
        .expect("stored deploy parked in rejected buffer remains unresolved while in scope"));
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .remove(vec![pending_deploy])
            .expect("remove parked deploy");

        snapshot.rejected_in_scope.insert(current_id(&deploy));
        assert!(storage_has_unresolved_in_scope_deploys(
            &snapshot,
            &deploy_storage,
            &rejected_deploy_buffer
        )
        .expect("stored rejected deploy in scope remains unresolved"));
    }

    #[test]
    fn recovered_deploy_leader_is_independent_of_parent_order_and_sender() {
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        set_test_committee(&mut snapshot, vec![
            validator(3),
            validator(1),
            validator(2),
        ]);
        set_last_finalized_height(&mut snapshot, 0);
        snapshot.parents = vec![test_block(
            invalid_block_hash(9),
            validator(3),
            Vec::new(),
            9,
            Vec::new(),
        )];

        assert!(is_recovered_deploy_leader(&snapshot, &validator_identity(1)).expect("leader"));
        assert!(!is_recovered_deploy_leader(&snapshot, &validator_identity(2)).expect("leader"));
        assert!(!is_recovered_deploy_leader(&snapshot, &validator_identity(3)).expect("leader"));

        snapshot.parents[0].sender = validator(9);
        snapshot.on_chain_state.active_validators = vec![validator(8), validator(9)];
        assert!(is_recovered_deploy_leader(&snapshot, &validator_identity(1)).expect("leader"));
        assert!(!is_recovered_deploy_leader(&snapshot, &validator_identity(3)).expect("leader"));
    }

    #[test]
    fn recovered_deploy_leader_rotates_by_finalized_height() {
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        set_test_committee(&mut snapshot, vec![
            validator(3),
            validator(1),
            validator(2),
        ]);
        set_last_finalized_height(&mut snapshot, 0);

        assert!(is_recovered_deploy_leader(&snapshot, &validator_identity(1)).expect("leader"));
        assert!(!is_recovered_deploy_leader(&snapshot, &validator_identity(2)).expect("leader"));
        assert!(!is_recovered_deploy_leader(&snapshot, &validator_identity(3)).expect("leader"));

        set_last_finalized_height(&mut snapshot, 0);
        assert!(is_recovered_deploy_leader(&snapshot, &validator_identity(1)).expect("leader"));

        set_last_finalized_height(&mut snapshot, 1);
        assert!(is_recovered_deploy_leader(&snapshot, &validator_identity(2)).expect("leader"));
        assert!(!is_recovered_deploy_leader(&snapshot, &validator_identity(1)).expect("leader"));

        set_last_finalized_height(&mut snapshot, 2);
        assert!(is_recovered_deploy_leader(&snapshot, &validator_identity(3)).expect("leader"));
        assert!(!is_recovered_deploy_leader(&snapshot, &validator_identity(1)).expect("leader"));

        set_last_finalized_height(&mut snapshot, 3);
        assert!(is_recovered_deploy_leader(&snapshot, &validator_identity(1)).expect("leader"));
    }

    #[test]
    fn recovered_deploy_leader_uses_the_finalized_floor_committee() {
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot.on_chain_state.active_validators.clear();
        snapshot.on_chain_state.bonds_map.insert(validator(1), 0);
        snapshot.on_chain_state.bonds_map.insert(validator(2), 10);
        snapshot.on_chain_state.bonds_map.insert(validator(3), 10);
        snapshot.finalized_floor_bonds = vec![
            Bond {
                validator: validator(2),
                stake: 10,
            },
            Bond {
                validator: validator(3),
                stake: 10,
            },
        ];
        set_last_finalized_height(&mut snapshot, 0);

        assert!(is_recovered_deploy_leader(&snapshot, &validator_identity(2)).expect("leader"));
        assert!(!is_recovered_deploy_leader(&snapshot, &validator_identity(1)).expect("leader"));

        set_last_finalized_height(&mut snapshot, 1);
        assert!(is_recovered_deploy_leader(&snapshot, &validator_identity(3)).expect("leader"));
        assert!(!is_recovered_deploy_leader(&snapshot, &validator_identity(2)).expect("leader"));
    }

    #[test]
    fn recovered_deploy_leader_fails_closed_without_an_eligible_validator() {
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        set_last_finalized_height(&mut snapshot, 0);

        assert_eq!(recovered_deploy_leader(&snapshot).expect("leader"), None);
        assert!(!is_recovered_deploy_leader(&snapshot, &validator_identity(1)).expect("leader"));
    }

    #[test]
    fn recovered_deploy_leadership_is_unique_within_each_finalized_view() {
        let validators = vec![validator(3), validator(1), validator(2), validator(2)];
        for height in 0..12 {
            let mut snapshot =
                crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
            set_test_committee(&mut snapshot, validators.clone());
            set_last_finalized_height(&mut snapshot, height);

            let elected: Vec<_> = (1..=3)
                .filter(|byte| {
                    is_recovered_deploy_leader(&snapshot, &validator_identity(*byte))
                        .expect("leader")
                })
                .collect();
            assert_eq!(elected.len(), 1, "finalized view {height}");
            assert_eq!(elected[0], (height % 3 + 1) as u8);
        }
    }

    #[test]
    fn self_chain_filter_is_candidate_scope_relative() {
        let recovered = construct_deploy::basic_deploy_data(51, None, Some("test".to_string()))
            .expect("recovered deploy");
        let active = construct_deploy::basic_deploy_data(52, None, Some("test".to_string()))
            .expect("active deploy");
        let rehome = construct_deploy::basic_deploy_data(54, None, Some("test".to_string()))
            .expect("rehome deploy");
        let unrelated = construct_deploy::basic_deploy_data(53, None, Some("test".to_string()))
            .expect("unrelated deploy");
        let mut deploys = HashSet::from([
            pending(recovered.clone()),
            pending(active.clone()),
            pending(rehome.clone()),
            pending(unrelated.clone()),
        ]);
        let self_chain = HashSet::from([
            current_id(&recovered),
            current_id(&active),
            current_id(&rehome),
        ]);
        let candidate_scope = dashmap::DashSet::from_iter([current_id(&active)]);
        let selected_recoveries = HashSet::from([current_id(&recovered)]);

        let removed = filter_self_chain_deploys(
            &mut deploys,
            &self_chain,
            &candidate_scope,
            &selected_recoveries,
        );

        assert_eq!(removed, 1);
        assert!(deploys.contains(&pending(recovered)));
        assert!(!deploys.contains(&pending(active)));
        assert!(deploys.contains(&pending(rehome)));
        assert!(deploys.contains(&pending(unrelated)));
    }

    #[test]
    fn self_chain_candidate_disposition_covers_every_predicate_combination() {
        use CandidateSelfChainDisposition::{
            ActiveDuplicate, ExcludedBranchRehome, NotOnSelfChain, SelectedRecovery,
        };

        let cases = [
            (false, false, false, NotOnSelfChain),
            (false, false, true, NotOnSelfChain),
            (false, true, false, NotOnSelfChain),
            (false, true, true, NotOnSelfChain),
            (true, false, false, ExcludedBranchRehome),
            (true, false, true, SelectedRecovery),
            (true, true, false, ActiveDuplicate),
            (true, true, true, SelectedRecovery),
        ];
        for (on_self_chain, in_scope, selected_recovery, expected) in cases {
            let actual =
                candidate_self_chain_disposition(on_self_chain, in_scope, selected_recovery);
            assert_eq!(actual, expected);
            assert_eq!(actual.should_package(), actual != ActiveDuplicate);
        }
    }

    proptest::proptest! {
        #[test]
        fn candidate_scope_packaging_matches_the_captured_authorization(
            on_self_chain in proptest::bool::ANY,
            active_in_candidate_scope in proptest::bool::ANY,
            selected_recovery in proptest::bool::ANY,
        ) {
            let disposition = candidate_self_chain_disposition(
                on_self_chain,
                active_in_candidate_scope,
                selected_recovery,
            );
            let expected = !on_self_chain || selected_recovery || !active_in_candidate_scope;
            proptest::prop_assert_eq!(disposition.should_package(), expected);
        }

        #[test]
        fn retry_readiness_matches_all_exclusion_invariants(
            clean_in_scope in proptest::bool::ANY,
            future in proptest::bool::ANY,
            time_expired in proptest::bool::ANY,
            floor_window_expired in proptest::bool::ANY,
            terminal in proptest::bool::ANY,
        ) {
            let ready = retry_candidate_is_ready(
                clean_in_scope,
                future,
                time_expired,
                floor_window_expired,
                terminal,
            );
            proptest::prop_assert_eq!(
                ready,
                !(terminal || clean_in_scope || future || time_expired || floor_window_expired),
            );
            proptest::prop_assert!(!ready || !terminal);
            proptest::prop_assert!(!ready || !clean_in_scope);
            proptest::prop_assert!(!ready || !future);
            proptest::prop_assert!(!ready || !time_expired);
            proptest::prop_assert!(!ready || !floor_window_expired);
        }
    }

    #[tokio::test]
    async fn recovery_ancestry_scans_fail_closed_on_missing_committed_bodies() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot.last_finalized_block = invalid_block_hash(0xa1);

        let scope_error = scope_has_unfinalized_user_deploys(&snapshot, &block_store)
            .expect_err("missing finalized boundary body");
        assert!(scope_error
            .to_string()
            .contains("reading the finalized boundary for parent-scope recovery"));

        let lag_error = finality_lag_stats(&snapshot, &block_store)
            .expect_err("missing finalized body for finality lag");
        assert!(lag_error
            .to_string()
            .contains("computing finality-lag admission state"));
    }

    #[tokio::test]
    async fn deploy_inclusion_leader_tracks_user_deploy_sender_below_support_block() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        set_test_committee(&mut snapshot, vec![
            validator(1),
            validator(2),
            validator(3),
        ]);
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 10;

        let lfb_hash = invalid_block_hash(0x71);
        let user_hash = invalid_block_hash(0x72);
        let support_hash = invalid_block_hash(0x73);
        let lfb = test_block(lfb_hash.clone(), validator(1), Vec::new(), 1, Vec::new());
        let deploy = crate::rust::util::construct_deploy::basic_deploy_data(
            1,
            None,
            Some("test".to_string()),
        )
        .expect("deploy");
        let user = test_block(
            user_hash.clone(),
            validator(2),
            vec![lfb_hash.clone()],
            2,
            vec![processed(deploy)],
        );
        let support = test_block(
            support_hash.clone(),
            validator(3),
            vec![user_hash.clone()],
            3,
            Vec::new(),
        );

        block_store.put_block_message(&lfb).expect("put lfb");
        block_store.put_block_message(&user).expect("put user");
        block_store
            .put_block_message(&support)
            .expect("put support");
        snapshot.last_finalized_block = lfb_hash;
        snapshot.max_block_num = 3;
        snapshot.parents = vec![support];

        let progress = deploy_inclusion_progress(&snapshot, &block_store).expect("progress");

        assert_eq!(progress.leader, Some(validator(2)));
        assert_eq!(
            progress.latest_deploy,
            Some(BranchDeployInfo {
                block_hash: user_hash,
                sender: validator(2),
                block_number: 2,
                timestamp: 2,
                deploy_sig_count: 1,
                new_sig_count: 1,
                recycled_sig_count: 0,
            })
        );
        assert_ne!(
            progress.leader,
            Some(validator_identity(3).public_key.bytes)
        );
    }

    #[test]
    fn non_leader_recovery_backlog_disables_ordinary_deploy_selection() {
        let snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        assert!(
            !ordinary_admission_policy(
                &snapshot,
                true,
                false,
                false,
                true,
                FreshAdmissionFallback::default(),
                FreshAdmissionFallback::default(),
                DeployInclusionStaleness::default(),
                lag(1, 1)
            )
            .allow_ordinary
        );
        assert!(
            ordinary_admission_policy(
                &snapshot,
                true,
                true,
                false,
                true,
                FreshAdmissionFallback::default(),
                FreshAdmissionFallback::default(),
                DeployInclusionStaleness::default(),
                lag(1, 1)
            )
            .allow_ordinary
        );
        assert!(
            ordinary_admission_policy(
                &snapshot,
                false,
                false,
                false,
                false,
                FreshAdmissionFallback::default(),
                FreshAdmissionFallback::default(),
                DeployInclusionStaleness::default(),
                lag(1, 1)
            )
            .allow_ordinary
        );
    }

    #[test]
    fn deploy_inclusion_leadership_gates_ordinary_selection() {
        let snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        assert!(
            ordinary_admission_policy(
                &snapshot,
                false,
                false,
                false,
                false,
                FreshAdmissionFallback::default(),
                FreshAdmissionFallback::default(),
                DeployInclusionStaleness::default(),
                lag(1, 1)
            )
            .allow_ordinary
        );
        assert!(
            ordinary_admission_policy(
                &snapshot,
                false,
                false,
                true,
                true,
                FreshAdmissionFallback::default(),
                FreshAdmissionFallback::default(),
                DeployInclusionStaleness::default(),
                lag(1, 1)
            )
            .allow_ordinary
        );
        assert!(
            !ordinary_admission_policy(
                &snapshot,
                false,
                false,
                true,
                false,
                FreshAdmissionFallback::default(),
                FreshAdmissionFallback::default(),
                DeployInclusionStaleness::default(),
                lag(1, 1)
            )
            .allow_ordinary
        );
        assert!(
            !ordinary_admission_policy(
                &snapshot,
                true,
                false,
                true,
                true,
                FreshAdmissionFallback::default(),
                FreshAdmissionFallback::default(),
                DeployInclusionStaleness::default(),
                lag(1, 1)
            )
            .allow_ordinary
        );
        assert!(
            ordinary_admission_policy(
                &snapshot,
                true,
                true,
                true,
                true,
                FreshAdmissionFallback::default(),
                FreshAdmissionFallback::default(),
                DeployInclusionStaleness::default(),
                lag(1, 1)
            )
            .allow_ordinary
        );
    }

    #[test]
    fn bounded_fresh_fallback_uses_small_oldest_first_policy() {
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 32;
        let policy = ordinary_admission_policy(
            &snapshot,
            false,
            false,
            true,
            false,
            fallback(true, NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP),
            FreshAdmissionFallback::default(),
            DeployInclusionStaleness::default(),
            lag(1, 1),
        );

        assert!(policy.allow_ordinary);
        assert_eq!(policy.ordinary_cap, NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP);
        assert!(!policy.reserve_tail);
        assert!(policy.fallback);
    }

    #[test]
    fn stale_progress_backpressures_normal_leader_cap() {
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 128;
        let stale = DeployInclusionStaleness {
            stale: true,
            block_or_time_stale: true,
            signature_stale: false,
            missing_deploy_metadata: false,
        };
        let recycled = DeployInclusionStaleness {
            signature_stale: true,
            ..stale
        };

        let policy = ordinary_admission_policy(
            &snapshot,
            false,
            false,
            false,
            true,
            FreshAdmissionFallback::default(),
            FreshAdmissionFallback::default(),
            stale,
            lag(3, 2),
        );
        assert_eq!(policy.ordinary_cap, 128);
        assert!(policy.reserve_tail);
        assert!(!policy.backpressure);

        let policy = ordinary_admission_policy(
            &snapshot,
            false,
            false,
            true,
            true,
            FreshAdmissionFallback::default(),
            FreshAdmissionFallback::default(),
            stale,
            lag(3, 2),
        );
        assert!(policy.allow_ordinary);
        assert_eq!(policy.ordinary_cap, NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP);
        assert!(!policy.reserve_tail);
        assert!(policy.backpressure);

        let policy = ordinary_admission_policy(
            &snapshot,
            false,
            false,
            true,
            true,
            FreshAdmissionFallback::default(),
            FreshAdmissionFallback::default(),
            recycled,
            lag(3, 2),
        );
        assert_eq!(
            policy.ordinary_cap,
            NON_LEADER_FALLBACK_MIN_ORDINARY_DEPLOY_CAP
        );
    }

    #[test]
    fn in_scope_recovery_fallback_waits_for_age_and_stale_progress() {
        let snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        let old = InScopeLocalDeployStats {
            count: 150,
            oldest_age_millis: FRESH_DEPLOY_MAX_ADMISSION_DELAY_MILLIS,
            stranded_count: 0,
        };
        let young = InScopeLocalDeployStats {
            count: 150,
            oldest_age_millis: FRESH_DEPLOY_MAX_ADMISSION_DELAY_MILLIS - 1,
            stranded_count: 0,
        };
        let young_stranded = InScopeLocalDeployStats {
            count: 1,
            oldest_age_millis: 0,
            stranded_count: 1,
        };
        let stale = DeployInclusionStaleness {
            stale: true,
            block_or_time_stale: true,
            signature_stale: false,
            missing_deploy_metadata: false,
        };

        assert!(!in_scope_recovery_fallback(&snapshot, false, stale, old, lag(3, 2)).allowed);
        assert!(
            !in_scope_recovery_fallback(
                &snapshot,
                true,
                DeployInclusionStaleness::default(),
                old,
                lag(3, 2)
            )
            .allowed
        );
        assert!(!in_scope_recovery_fallback(&snapshot, true, stale, young, lag(3, 2)).allowed);
        let fallback = in_scope_recovery_fallback(&snapshot, true, stale, old, lag(3, 2));
        assert!(fallback.allowed);
        assert_eq!(fallback.cap, NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP);
        let fallback = in_scope_recovery_fallback(
            &snapshot,
            true,
            DeployInclusionStaleness::default(),
            young_stranded,
            lag(3, 2),
        );
        assert!(fallback.allowed);
        assert_eq!(fallback.cap, NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP);
    }

    #[test]
    fn fresh_fallback_uses_bounded_cap_before_age_or_progress_lease() {
        let snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        let stats = FreshLocalDeployStats {
            count: 1,
            oldest_age_millis: FRESH_DEPLOY_MAX_ADMISSION_DELAY_MILLIS,
        };
        let fresh = deploy_inclusion_progress_staleness(
            &DeployInclusionProgress {
                leader: Some(validator(1)),
                latest_deploy: Some(branch_info(0x81, validator(1), 10)),
            },
            11,
            10,
        );
        let stale = DeployInclusionStaleness {
            stale: true,
            block_or_time_stale: true,
            signature_stale: false,
            missing_deploy_metadata: false,
        };

        let fallback = fresh_admission_fallback(&snapshot, true, fresh, stats, lag(3, 1));
        assert!(fallback.allowed);
        assert_eq!(fallback.cap, NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP);
        assert!(fresh_admission_fallback(&snapshot, true, stale, stats, lag(3, 1)).allowed);
        assert!(
            fresh_admission_fallback(
                &snapshot,
                false,
                DeployInclusionStaleness::default(),
                stats,
                lag(3, 1)
            )
            .allowed
        );

        let young = FreshLocalDeployStats {
            count: 1,
            oldest_age_millis: FRESH_DEPLOY_MAX_ADMISSION_DELAY_MILLIS - 1,
        };
        let fallback = fresh_admission_fallback(&snapshot, true, stale, young, lag(3, 1));
        assert!(fallback.allowed);
        assert_eq!(fallback.cap, NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP);
        assert!(
            !fresh_admission_fallback(
                &snapshot,
                true,
                stale,
                FreshLocalDeployStats::default(),
                lag(3, 1)
            )
            .allowed
        );
    }

    #[test]
    fn adaptive_fallback_scales_with_age_and_finality_backpressure() {
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 128;

        let medium = fresh_admission_fallback(
            &snapshot,
            false,
            DeployInclusionStaleness::default(),
            FreshLocalDeployStats {
                count: 10,
                oldest_age_millis: FRESH_DEPLOY_ESCALATED_ADMISSION_DELAY_MILLIS,
            },
            lag(10, 9),
        );
        assert_eq!(medium.cap, NON_LEADER_FALLBACK_MEDIUM_ORDINARY_DEPLOY_CAP);
        assert!(!medium.backpressure);

        let max = fresh_admission_fallback(
            &snapshot,
            false,
            DeployInclusionStaleness::default(),
            FreshLocalDeployStats {
                count: 10,
                oldest_age_millis: FRESH_DEPLOY_MAX_ESCALATED_ADMISSION_DELAY_MILLIS,
            },
            lag(10, 9),
        );
        assert_eq!(max.cap, NON_LEADER_FALLBACK_MAX_ORDINARY_DEPLOY_CAP);
        assert!(!max.backpressure);

        let soft = fresh_admission_fallback(
            &snapshot,
            false,
            DeployInclusionStaleness::default(),
            FreshLocalDeployStats {
                count: 10,
                oldest_age_millis: FRESH_DEPLOY_MAX_ESCALATED_ADMISSION_DELAY_MILLIS,
            },
            lag(10, 6),
        );
        assert_eq!(soft.cap, NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP);
        assert!(soft.backpressure);

        let hard = fresh_admission_fallback(
            &snapshot,
            false,
            DeployInclusionStaleness::default(),
            FreshLocalDeployStats {
                count: 10,
                oldest_age_millis: FRESH_DEPLOY_MAX_ESCALATED_ADMISSION_DELAY_MILLIS,
            },
            lag(10, 2),
        );
        assert_eq!(hard.cap, NON_LEADER_FALLBACK_MIN_ORDINARY_DEPLOY_CAP);
        assert!(hard.backpressure);
    }

    #[test]
    fn newer_branch_deploy_info_breaks_ties_by_sender() {
        let mut existing = branch_info(0x82, validator(1), 10);
        existing.timestamp = 1_000;
        let mut candidate = branch_info(0x83, validator(2), 10);
        candidate.timestamp = 1_000;

        assert_eq!(
            newer_branch_deploy_info(Some(existing), candidate.clone()),
            Some(candidate)
        );
    }

    #[test]
    fn deploy_inclusion_progress_stale_after_block_or_time_lease() {
        let progress = DeployInclusionProgress {
            leader: Some(validator(1)),
            latest_deploy: Some(BranchDeployInfo {
                timestamp: 1_000,
                ..branch_info(0x84, validator(1), 10)
            }),
        };

        assert!(!deploy_inclusion_progress_is_stale(
            &progress,
            12,
            1_000 + DEPLOY_INCLUSION_LEASE_MILLIS - 1
        ));
        assert!(deploy_inclusion_progress_is_stale(&progress, 13, 1_000));
        assert!(deploy_inclusion_progress_is_stale(
            &progress,
            11,
            1_000 + DEPLOY_INCLUSION_LEASE_MILLIS
        ));
        assert!(deploy_inclusion_progress_is_stale(
            &DeployInclusionProgress {
                leader: Some(validator(1)),
                latest_deploy: None,
            },
            11,
            1_000
        ));
        assert!(!deploy_inclusion_progress_is_stale(
            &DeployInclusionProgress::default(),
            11,
            1_000
        ));
    }

    #[test]
    fn deploy_inclusion_progress_is_stale_when_latest_deploy_block_recycles_signatures() {
        let mut recycled = branch_info(0x85, validator(1), 10);
        recycled.new_sig_count = 0;
        recycled.recycled_sig_count = 1;
        let progress = DeployInclusionProgress {
            leader: Some(validator(1)),
            latest_deploy: Some(recycled),
        };

        let staleness = deploy_inclusion_progress_staleness(&progress, 11, 1_000);

        assert!(staleness.stale);
        assert!(!staleness.block_or_time_stale);
        assert!(staleness.signature_stale);
    }

    #[tokio::test]
    async fn deploy_inclusion_progress_counts_recycled_deploy_signatures() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        set_test_committee(&mut snapshot, vec![
            validator(1),
            validator(2),
            validator(3),
        ]);
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 10;

        let lfb_hash = invalid_block_hash(0x86);
        let first_hash = invalid_block_hash(0x87);
        let recycled_hash = invalid_block_hash(0x88);
        let lfb = test_block(lfb_hash.clone(), validator(1), Vec::new(), 1, Vec::new());
        let deploy = crate::rust::util::construct_deploy::basic_deploy_data(
            2,
            None,
            Some("test".to_string()),
        )
        .expect("deploy");
        let first = test_block(
            first_hash.clone(),
            validator(2),
            vec![lfb_hash.clone()],
            2,
            vec![processed(deploy.clone())],
        );
        let recycled = test_block(
            recycled_hash.clone(),
            validator(2),
            vec![first_hash.clone()],
            3,
            vec![processed(deploy)],
        );

        block_store.put_block_message(&lfb).expect("put lfb");
        block_store.put_block_message(&first).expect("put first");
        block_store
            .put_block_message(&recycled)
            .expect("put recycled");
        snapshot.last_finalized_block = lfb_hash;
        snapshot.max_block_num = 3;
        snapshot.parents = vec![recycled];

        let progress = deploy_inclusion_progress(&snapshot, &block_store).expect("progress");

        assert_eq!(progress.leader, Some(validator(2)));
        assert_eq!(
            progress.latest_deploy,
            Some(BranchDeployInfo {
                block_hash: recycled_hash,
                sender: validator(2),
                block_number: 3,
                timestamp: 3,
                deploy_sig_count: 1,
                new_sig_count: 0,
                recycled_sig_count: 1,
            })
        );
    }

    #[tokio::test]
    async fn deploy_inclusion_progress_treats_sibling_branch_signatures_as_in_scope() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        set_test_committee(&mut snapshot, vec![
            validator(1),
            validator(2),
            validator(3),
        ]);
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 10;

        let lfb_hash = invalid_block_hash(0x89);
        let sibling_hash = invalid_block_hash(0x8a);
        let duplicate_hash = invalid_block_hash(0x8b);
        let lfb = test_block(lfb_hash.clone(), validator(1), Vec::new(), 1, Vec::new());
        let deploy = crate::rust::util::construct_deploy::basic_deploy_data(
            3,
            None,
            Some("test".to_string()),
        )
        .expect("deploy");
        let sibling = test_block(
            sibling_hash.clone(),
            validator(3),
            vec![lfb_hash.clone()],
            2,
            vec![processed(deploy.clone())],
        );
        let duplicate = test_block(
            duplicate_hash.clone(),
            validator(2),
            vec![lfb_hash.clone()],
            3,
            vec![processed(deploy)],
        );

        block_store.put_block_message(&lfb).expect("put lfb");
        block_store
            .put_block_message(&sibling)
            .expect("put sibling");
        block_store
            .put_block_message(&duplicate)
            .expect("put duplicate");
        snapshot.last_finalized_block = lfb_hash;
        snapshot.max_block_num = 3;
        snapshot.parents = vec![duplicate, sibling];

        let progress = deploy_inclusion_progress(&snapshot, &block_store).expect("progress");

        assert_eq!(progress.leader, Some(validator(2)));
        assert_eq!(
            progress.latest_deploy,
            Some(BranchDeployInfo {
                block_hash: duplicate_hash,
                sender: validator(2),
                block_number: 3,
                timestamp: 3,
                deploy_sig_count: 1,
                new_sig_count: 0,
                recycled_sig_count: 1,
            })
        );
    }

    /// T-Slash seed-wiring (MainTheorem.v:302, `main_TSlash_deploy_seed_uses_invalid_block_hash`).
    ///
    /// The emitted `SlashDeploy`'s `initial_rand` MUST derive from the offender's OWN
    /// `invalid_block_hash` (plus the proposer pubkey and seq), so every node — and the
    /// replay path — recomputes the identical randomness. A regression wiring the seed from a
    /// DIFFERENT input (the offender pubkey, a constant, the proposer's own block hash) would
    /// still pass every candidate-FILTERING test above yet silently fork replay.
    /// `build_slash_deploy` is the single construction seam both proposer slash paths use.
    #[test]
    fn build_slash_deploy_wires_seed_from_invalid_block_hash() {
        let invalid_block = invalid_block_hash(0xD5);
        let proposer_pk = PublicKey::from_bytes(&[0x07u8; 32]);
        let seq_num = 42;
        let target_epoch = 7i64;

        let deploy = build_slash_deploy(
            &invalid_block,
            None,
            &proposer_pk,
            target_epoch,
            BondGeneration::GENESIS,
            seq_num,
        );

        // Straight-through fields.
        assert_eq!(
            deploy.invalid_block_hash, invalid_block,
            "invalid_block_hash passes through"
        );
        assert_eq!(deploy.pk, proposer_pk, "proposer pubkey passes through");
        assert_eq!(
            deploy.target_activation_epoch, target_epoch,
            "target epoch passes through"
        );

        // The load-bearing wiring: the seed recomputes from THIS deploy's own invalid_block_hash.
        let self_id = Bytes::copy_from_slice(&proposer_pk.bytes);
        let expected = system_deploy_util::generate_slash_deploy_random_seed(
            self_id.clone(),
            seq_num,
            &deploy.invalid_block_hash,
        );
        assert_eq!(
            deploy.initial_rand, expected,
            "initial_rand must be generate_slash_deploy_random_seed(proposer, seq, invalid_block_hash)"
        );

        // Negative control — a DIFFERENT invalid_block_hash yields a DIFFERENT seed, so the
        // assertion above is discriminating (not vacuously true for any hash).
        let other_block = invalid_block_hash(0xE6);
        let seed_other =
            system_deploy_util::generate_slash_deploy_random_seed(self_id, seq_num, &other_block);
        assert_ne!(
            deploy.initial_rand, seed_other,
            "a different invalid_block_hash must change the seed (wrong-hash regression must be caught)"
        );
    }

    #[test]
    fn detects_retryable_single_value_batch_errors() {
        let err = CasperError::HistoryError(HistoryError::MergeError(
            "single-value cell abc would hold 2 values after merge".to_string(),
        ));

        assert!(is_retryable_single_value_batch_error(&err));

        let runtime = CasperError::RuntimeError(
            "number channel abc holds 2 values [0, 0]; IntegerAdd single-value invariant violated"
                .to_string(),
        );
        assert!(is_retryable_single_value_batch_error(&runtime));

        let diff_conversion = CasperError::RuntimeError(
            "Expected at most one value for number channel abc, found 2".to_string(),
        );
        assert!(is_retryable_single_value_batch_error(&diff_conversion));

        let other = CasperError::HistoryError(HistoryError::MergeError(
            "MergeType mismatch on channel abc".to_string(),
        ));
        assert!(!is_retryable_single_value_batch_error(&other));

        let unrelated_runtime = CasperError::RuntimeError(
            "single-value cell abc would hold 2 values after merge".to_string(),
        );
        assert!(!is_retryable_single_value_batch_error(&unrelated_runtime));
    }

    #[test]
    fn single_value_retry_limit_reaches_one() {
        let mut limit = 100;
        let mut limits = Vec::new();
        while let Some(next) = next_single_value_retry_limit(limit) {
            limits.push(next);
            limit = next;
        }

        assert_eq!(limits, vec![50, 25, 12, 6, 3, 1]);
        assert_eq!(next_single_value_retry_limit(1), None);
        assert_eq!(next_single_value_retry_limit(0), None);
    }

    #[test]
    fn checkpoint_retry_limit_includes_multi_deploy_refund_failures() {
        let payment = CasperError::SystemRuntimeError(
            SystemDeployPlatformFailure::GasPaymentFailure("payment failed".to_string()),
        );
        assert_eq!(next_checkpoint_retry_limit(&payment, 16), Some(8));
        assert_eq!(next_checkpoint_retry_limit(&payment, 1), None);

        let refund = CasperError::SystemRuntimeError(
            SystemDeployPlatformFailure::GasRefundFailure("refund failed".to_string()),
        );
        assert_eq!(next_checkpoint_retry_limit(&refund, 16), Some(8));
        assert_eq!(next_checkpoint_retry_limit(&refund, 1), None);

        let single_value = CasperError::RuntimeError(
            "number channel abc holds 2 values [0, 0]; IntegerAdd single-value invariant violated"
                .to_string(),
        );
        assert_eq!(next_checkpoint_retry_limit(&single_value, 16), Some(8));

        let unrelated = CasperError::RuntimeError("other".to_string());
        assert_eq!(next_checkpoint_retry_limit(&unrelated, 16), None);
    }

    proptest::proptest! {
        #[test]
        fn checkpoint_retry_sequences_are_deterministic_strict_and_bounded(
            initial in 1usize..=1_000_000usize,
            error_kind in 0u8..3,
        ) {
            let retryable = match error_kind {
                0 => CasperError::SystemRuntimeError(
                    SystemDeployPlatformFailure::GasPaymentFailure("payment failed".to_string()),
                ),
                1 => CasperError::SystemRuntimeError(
                    SystemDeployPlatformFailure::GasRefundFailure("refund failed".to_string()),
                ),
                _ => CasperError::RuntimeError(
                    "number channel property holds 2 values [0, 0]; IntegerAdd single-value invariant violated"
                        .to_string(),
                ),
            };
            let sequence = |mut current: usize| {
                let mut limits = Vec::new();
                while let Some(next) = next_checkpoint_retry_limit(&retryable, current) {
                    limits.push(next);
                    current = next;
                }
                limits
            };

            let first = sequence(initial);
            let second = sequence(initial);
            proptest::prop_assert_eq!(&first, &second);
            proptest::prop_assert!(first.len() <= usize::BITS as usize);
            let mut previous = initial;
            for next in &first {
                proptest::prop_assert!(*next < previous);
                proptest::prop_assert!(*next >= 1);
                previous = *next;
            }
            if initial > 1 {
                proptest::prop_assert_eq!(first.last(), Some(&1));
            } else {
                proptest::prop_assert!(first.is_empty());
            }
        }

        #[test]
        fn unrelated_checkpoint_errors_never_start_prefix_retries(
            current in 0usize..=1_000_000usize,
            message in "[a-zA-Z0-9 _-]{0,64}",
        ) {
            let unrelated = CasperError::RuntimeError(format!("unrelated {message}"));
            proptest::prop_assert_eq!(next_checkpoint_retry_limit(&unrelated, current), None);
        }
    }

    #[tokio::test]
    async fn selected_recovered_deploys_are_drained_from_rejected_buffer() {
        let mut kvm = InMemoryStoreManager::new();
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let recovered = construct_deploy::basic_deploy_data(41, None, Some("test".to_string()))
            .expect("recovered deploy");
        let unrelated = construct_deploy::basic_deploy_data(42, None, Some("test".to_string()))
            .expect("unrelated deploy");
        let never_buffered =
            construct_deploy::basic_deploy_data(43, None, Some("test".to_string()))
                .expect("never buffered deploy");

        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![pending(recovered.clone()), pending(unrelated.clone())])
            .expect("seed rejected buffer");

        let removed = drain_selected_deploys_from_rejected_buffer(&rejected_deploy_buffer, &[
            pending(recovered.clone()),
            pending(never_buffered),
        ])
        .expect("drain selected deploys");

        assert_eq!(removed, 1);
        let guard = rejected_deploy_buffer.lock().expect("rejected buffer lock");
        assert!(!guard
            .contains_id(&current_id(&recovered))
            .expect("recovered contains"));
        assert!(guard
            .contains_id(&current_id(&unrelated))
            .expect("unrelated contains"));
    }

    // Packaging drains a recovered deploy's ordinary-storage copy only; the
    // rejected-buffer entry must survive packaging (the block may be
    // orphaned) and is purged only when its sig is finalized-won or expired.
    #[tokio::test]
    async fn selected_recovered_deploys_drain_storage_but_keep_buffer_entry() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let recovered = construct_deploy::basic_deploy_data(61, None, Some("test".to_string()))
            .expect("recovered deploy");
        let ordinary = construct_deploy::basic_deploy_data(62, None, Some("test".to_string()))
            .expect("ordinary deploy");
        let unselected_recovered =
            construct_deploy::basic_deploy_data(63, None, Some("test".to_string()))
                .expect("unselected recovered deploy");

        seed_current_deploys(&deploy_storage, [
            &recovered,
            &ordinary,
            &unselected_recovered,
        ]);
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![
                pending(recovered.clone()),
                pending(unselected_recovered.clone()),
            ])
            .expect("seed rejected buffer");

        let removed = drain_selected_recovered_deploys_from_deploy_storage(
            &deploy_storage,
            &rejected_deploy_buffer,
            &[pending(recovered.clone()), pending(ordinary.clone())],
        )
        .expect("drain selected recovered deploys");

        assert_eq!(removed, 1);
        let storage = deploy_storage.lock();
        assert!(!storage
            .contains_envelope(current_id(&recovered).as_bytes())
            .expect("recovered storage membership"));
        assert!(storage
            .contains_envelope(current_id(&ordinary).as_bytes())
            .expect("ordinary storage membership"));
        assert!(storage
            .contains_envelope(current_id(&unselected_recovered).as_bytes())
            .expect("unselected recovered storage membership"));
        let buffer = rejected_deploy_buffer.lock().expect("rejected buffer lock");
        assert!(buffer
            .contains_id(&current_id(&recovered))
            .expect("recovered contains"));
        assert!(buffer
            .contains_id(&current_id(&unselected_recovered))
            .expect("unselected recovered contains"));
    }

    #[tokio::test]
    async fn non_leader_skips_storage_deploys_parked_in_rejected_buffer() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot.parents.clear();
        seed_empty_last_finalized_block(&mut snapshot, &block_store);
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 10;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 50;
        let buffered = construct_deploy::basic_deploy_data(71, None, Some("test".to_string()))
            .expect("buffered deploy");
        let ordinary = construct_deploy::basic_deploy_data(72, None, Some("test".to_string()))
            .expect("ordinary deploy");

        seed_current_deploys(&deploy_storage, [&buffered, &ordinary]);
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![pending(buffered.clone())])
            .expect("seed rejected buffer");

        let prepared = prepare_user_deploys(
            &snapshot,
            20,
            ordinary.data.time_stamp,
            deploy_storage,
            rejected_deploy_buffer,
            &block_store,
            false,
            true,
        )
        .await
        .expect("prepare deploys");

        assert_eq!(prepared.deploys.len(), 1);
        assert!(prepared
            .deploys
            .iter()
            .any(|deploy| deploy.deploy_id() == &current_id_bytes(&ordinary)));
        assert!(!prepared
            .deploys
            .iter()
            .any(|deploy| deploy.deploy_id() == &current_id_bytes(&buffered)));
    }

    #[tokio::test]
    async fn rejected_buffer_backlog_requires_selectable_deploy() {
        let mut kvm = InMemoryStoreManager::new();
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        seed_empty_last_finalized_block(&mut snapshot, &block_store);
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 50;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("time")
            .as_millis() as i64;
        let mut expired = construct_deploy::source_deploy_now(
            "@expired-buffer!(0)".to_string(),
            None,
            Some(10),
            Some("test".to_string()),
        )
        .expect("expired deploy");
        expired.data.expiration_timestamp = Some(now - 1);
        let old = construct_deploy::source_deploy_now(
            "@old-buffer!(0)".to_string(),
            None,
            Some(40),
            Some("test".to_string()),
        )
        .expect("old deploy");
        let future = construct_deploy::source_deploy_now(
            "@future-buffer!(0)".to_string(),
            None,
            Some(100),
            Some("test".to_string()),
        )
        .expect("future deploy");
        let boundary = construct_deploy::source_deploy_now(
            "@boundary-buffer!(0)".to_string(),
            None,
            Some(50),
            Some("test".to_string()),
        )
        .expect("boundary deploy");
        snapshot.rejected_in_scope.insert(current_id(&boundary));
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![
                pending(expired),
                pending(old),
                pending(future),
                pending(boundary),
            ])
            .expect("seed rejected buffer");

        assert!(!rejected_buffer_has_recoverable_deploys(
            &snapshot,
            100,
            now,
            &rejected_deploy_buffer,
            &block_store,
            None
        )
        .expect("check unselectable buffer"));

        let fresh = construct_deploy::source_deploy_now(
            "@fresh-buffer!(0)".to_string(),
            None,
            Some(99),
            Some("test".to_string()),
        )
        .expect("fresh deploy");
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![pending(fresh)])
            .expect("seed fresh buffer");

        assert!(rejected_buffer_has_recoverable_deploys(
            &snapshot,
            100,
            now,
            &rejected_deploy_buffer,
            &block_store,
            None
        )
        .expect("check selectable buffer"));
    }

    #[tokio::test]
    async fn prepare_user_deploys_purges_expired_rejected_buffer_entries() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        seed_empty_last_finalized_block(&mut snapshot, &block_store);
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 10;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 50;
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("time")
            .as_millis() as i64;
        let mut expired = construct_deploy::source_deploy_now(
            "@expired-purge!(0)".to_string(),
            None,
            Some(10),
            Some("test".to_string()),
        )
        .expect("expired deploy");
        expired.data.expiration_timestamp = Some(now - 1);
        seed_current_deploys(&deploy_storage, [&expired]);
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![pending(expired.clone())])
            .expect("seed rejected buffer");

        let prepared = prepare_user_deploys(
            &snapshot,
            20,
            now,
            deploy_storage.clone(),
            rejected_deploy_buffer.clone(),
            &block_store,
            true,
            true,
        )
        .await
        .expect("prepare deploys");

        assert!(prepared.deploys.is_empty());
        assert!(!deploy_storage
            .lock()
            .contains_envelope(current_id(&expired).as_bytes())
            .expect("expired storage membership"));
        assert!(!rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .contains_id(&current_id(&expired))
            .expect("buffer contains"));
    }

    #[tokio::test]
    async fn ordinary_storage_selection_uses_ordinary_throughput_cap() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        seed_empty_last_finalized_block(&mut snapshot, &block_store);
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 32;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 50;
        let deploys: Vec<_> = (91..=130)
            .map(|id| {
                construct_deploy::basic_deploy_data(id, None, Some("test".to_string()))
                    .expect("deploy")
            })
            .collect();
        let first = deploys[0].clone();
        let current_time = deploys[2].data.time_stamp;

        seed_current_deploys(&deploy_storage, deploys.iter());

        let prepared = prepare_user_deploys(
            &snapshot,
            20,
            current_time,
            deploy_storage,
            rejected_deploy_buffer,
            &block_store,
            true,
            true,
        )
        .await
        .expect("prepare deploys");

        assert_eq!(prepared.deploys.len(), 32);
        assert_eq!(prepared.effective_cap, 32);
        assert!(prepared.cap_hit);
        assert!(prepared
            .deploys
            .iter()
            .any(|deploy| deploy.deploy_id() == &current_id_bytes(&first)));
    }

    #[tokio::test]
    async fn ordinary_storage_selection_uses_byte_budget_for_large_stored_event_batches() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        seed_empty_last_finalized_block(&mut snapshot, &block_store);
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 128;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 500;
        let payload = "x".repeat(USER_DEPLOY_BYTE_PROPOSAL_BUDGET / 8);
        let deploys: Vec<_> = (1..=40)
            .map(|id| {
                construct_deploy::source_deploy(
                    format!("@\"{}{}\"!(Nil)", payload, id),
                    1_000 + id as i64,
                    None,
                    None,
                    None,
                    Some(id as i64),
                    Some("test".to_string()),
                )
                .expect("deploy")
            })
            .collect();

        seed_current_deploys(&deploy_storage, deploys.iter());

        let prepared = prepare_user_deploys_with_policy(
            &snapshot,
            300,
            10_000,
            deploy_storage,
            rejected_deploy_buffer,
            &block_store,
            false,
            DeployAdmissionPolicy {
                allow_ordinary: true,
                ordinary_cap: ORDINARY_DEPLOY_PROPOSAL_CAP,
                allow_in_scope_recovery: false,
                in_scope_recovery_cap: 0,
                reserve_tail: false,
                fallback: false,
                backpressure: false,
            },
            None,
        )
        .await
        .expect("prepare deploys");

        assert!(!prepared.deploys.is_empty());
        assert!(prepared.deploys.len() < ORDINARY_DEPLOY_PROPOSAL_CAP);
        assert!(prepared.byte_cap_hit);
        assert!(prepared.cap_hit);
        assert!(prepared.selected_user_deploy_bytes <= USER_DEPLOY_BYTE_PROPOSAL_BUDGET);
        assert!(prepared.deferred_user_deploy_bytes > 0);
    }

    #[tokio::test]
    async fn fallback_ordinary_selection_uses_bounded_oldest_first_cap() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        seed_empty_last_finalized_block(&mut snapshot, &block_store);
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 32;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 500;
        let deploys: Vec<_> = (1..=20)
            .map(|id| {
                construct_deploy::source_deploy(
                    format!("@{}!({})", id, id),
                    1_000 + id as i64,
                    None,
                    None,
                    None,
                    Some(id as i64),
                    Some("test".to_string()),
                )
                .expect("deploy")
            })
            .collect();

        seed_current_deploys(&deploy_storage, deploys.iter());

        let prepared = prepare_user_deploys_with_policy(
            &snapshot,
            200,
            10_000,
            deploy_storage,
            rejected_deploy_buffer,
            &block_store,
            false,
            DeployAdmissionPolicy {
                allow_ordinary: true,
                ordinary_cap: NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP,
                allow_in_scope_recovery: false,
                in_scope_recovery_cap: 0,
                reserve_tail: false,
                fallback: true,
                backpressure: false,
            },
            None,
        )
        .await
        .expect("prepare deploys");

        let selected_valid_after: HashSet<i64> = prepared
            .deploys
            .iter()
            .map(|deploy| deploy.data().valid_after_block_number)
            .collect();
        let expected: HashSet<i64> = (1..=NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP as i64).collect();
        assert_eq!(
            prepared.deploys.len(),
            NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP
        );
        assert_eq!(selected_valid_after, expected);
        assert_eq!(
            prepared.effective_cap,
            NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP
        );
        assert!(prepared.cap_hit);
    }

    #[tokio::test]
    async fn already_in_scope_deploys_do_not_consume_fallback_fresh_cap() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        seed_empty_last_finalized_block(&mut snapshot, &block_store);
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 32;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 500;
        let in_scope: Vec<_> = (1..=95)
            .map(|id| {
                construct_deploy::source_deploy(
                    format!("@{}!({})", id, id),
                    1_000 + id as i64,
                    None,
                    None,
                    None,
                    Some(id as i64),
                    Some("test".to_string()),
                )
                .expect("deploy")
            })
            .collect();
        let fresh: Vec<_> = (100..=109)
            .map(|id| {
                construct_deploy::source_deploy(
                    format!("@{}!({})", id, id),
                    1_000 + id as i64,
                    None,
                    None,
                    None,
                    Some(id as i64),
                    Some("test".to_string()),
                )
                .expect("deploy")
            })
            .collect();
        for deploy in &in_scope {
            snapshot.deploys_in_scope.insert(current_id(deploy));
        }
        let fresh_sigs: HashSet<Bytes> = fresh.iter().map(current_id_bytes).collect();
        seed_current_deploys(&deploy_storage, in_scope.iter().chain(fresh.iter()));

        let prepared = prepare_user_deploys_with_policy(
            &snapshot,
            300,
            10_000,
            deploy_storage,
            rejected_deploy_buffer,
            &block_store,
            false,
            DeployAdmissionPolicy {
                allow_ordinary: true,
                ordinary_cap: NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP,
                allow_in_scope_recovery: false,
                in_scope_recovery_cap: 0,
                reserve_tail: false,
                fallback: true,
                backpressure: false,
            },
            None,
        )
        .await
        .expect("prepare deploys");

        assert_eq!(
            prepared.deploys.len(),
            NON_LEADER_FALLBACK_ORDINARY_DEPLOY_CAP
        );
        assert!(prepared
            .deploys
            .iter()
            .all(|deploy| fresh_sigs.contains(deploy.deploy_id())));
    }

    // Regression for #194: a deploy the DAG-wide scan marks `rejected_in_scope`
    // (a descendant merge rejected it) is only exempt from the scope block when
    // THIS validator also holds it in its own local recovery buffer
    // (`retry_scope_exempt`, line ~845). A validator that only saw the deploy
    // via gossip/another block's inclusion never populates that local buffer,
    // so before this fix the deploy fell into a gap between both recovery
    // routes: excluded from `in_scope_recovery_candidates` because it IS
    // `rejected_in_scope`, and excluded from the local-recovery retry path
    // because it is NOT locally recovered. It was filtered "already in scope"
    // every round with no route back in, matching the testbed audit's
    // asymmetric per-validator stall (some validators stuck, others fine).
    #[tokio::test]
    async fn rejected_in_scope_deploy_without_local_recovery_is_still_selected_for_recovery() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 32;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 500;
        let deploy = construct_deploy::source_deploy(
            "@stranded_no_local_buffer!(0)".to_string(),
            1_000,
            None,
            None,
            None,
            Some(1),
            Some("test".to_string()),
        )
        .expect("deploy");
        // In scope (some block in the DAG carries it) AND rejected_in_scope
        // (a descendant merge already rejected it) — but the rejected-deploy
        // buffer (below) stays empty, so this validator never locally
        // "recovered" it. No parents are set, so `canonical_won` is empty:
        // the deploy did not land via any canonical parent either.
        snapshot.deploys_in_scope.insert(current_id(&deploy));
        snapshot.rejected_in_scope.insert(current_id(&deploy));
        seed_current_deploys(&deploy_storage, [&deploy]);

        let prepared = prepare_user_deploys_with_policy(
            &snapshot,
            300,
            10_000,
            deploy_storage,
            rejected_deploy_buffer,
            &block_store,
            false,
            DeployAdmissionPolicy {
                allow_ordinary: false,
                ordinary_cap: 0,
                allow_in_scope_recovery: true,
                in_scope_recovery_cap: NON_LEADER_FALLBACK_MIN_ORDINARY_DEPLOY_CAP,
                reserve_tail: false,
                fallback: true,
                backpressure: true,
            },
            None,
        )
        .await
        .expect("prepare deploys");

        assert_eq!(prepared.already_in_scope_count, 1);
        assert_eq!(prepared.selected_in_scope_recovery_count, 1);
        assert_eq!(prepared.deploys.len(), 1);
        assert_eq!(
            prepared.deploys.iter().next().unwrap().deploy_id(),
            &current_id_bytes(&deploy)
        );
    }

    #[tokio::test]
    async fn stale_in_scope_recovery_selects_bounded_oldest_first_batch() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        seed_empty_last_finalized_block(&mut snapshot, &block_store);
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 128;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 500;
        let deploys: Vec<_> = (1..=20)
            .map(|id| {
                construct_deploy::source_deploy(
                    format!("@{}!({})", id, id),
                    1_000 + id as i64,
                    None,
                    None,
                    None,
                    Some(id as i64),
                    Some("test".to_string()),
                )
                .expect("deploy")
            })
            .collect();
        for deploy in &deploys {
            snapshot.deploys_in_scope.insert(current_id(deploy));
        }
        let lfb_hash = invalid_block_hash(0x91);
        let user_hash = invalid_block_hash(0x92);
        let lfb = test_block(lfb_hash.clone(), validator(1), Vec::new(), 250, Vec::new());
        let user = test_block(
            user_hash,
            validator(2),
            vec![lfb_hash.clone()],
            299,
            deploys.iter().cloned().map(processed).collect(),
        );
        block_store.put_block_message(&lfb).expect("put lfb");
        block_store.put_block_message(&user).expect("put user");
        snapshot.last_finalized_block = lfb_hash;
        snapshot.max_block_num = 299;
        snapshot.parents = vec![lfb];
        seed_current_deploys(&deploy_storage, deploys.iter());

        let stats = in_scope_local_deploy_stats(
            &snapshot,
            300,
            10_000,
            &deploy_storage,
            &rejected_deploy_buffer,
            &block_store,
            None,
        )
        .expect("in-scope stats");
        assert_eq!(stats.count, 20);

        let prepared = prepare_user_deploys_with_policy(
            &snapshot,
            300,
            10_000,
            deploy_storage,
            rejected_deploy_buffer,
            &block_store,
            false,
            DeployAdmissionPolicy {
                allow_ordinary: false,
                ordinary_cap: 0,
                allow_in_scope_recovery: true,
                in_scope_recovery_cap: NON_LEADER_FALLBACK_MIN_ORDINARY_DEPLOY_CAP,
                reserve_tail: false,
                fallback: true,
                backpressure: true,
            },
            None,
        )
        .await
        .expect("prepare deploys");

        let selected_valid_after: HashSet<i64> = prepared
            .deploys
            .iter()
            .map(|deploy| deploy.data().valid_after_block_number)
            .collect();
        let expected: HashSet<i64> =
            (1..=NON_LEADER_FALLBACK_MIN_ORDINARY_DEPLOY_CAP as i64).collect();
        assert_eq!(
            prepared.deploys.len(),
            NON_LEADER_FALLBACK_MIN_ORDINARY_DEPLOY_CAP
        );
        assert_eq!(selected_valid_after, expected);
        assert_eq!(prepared.already_in_scope_count, 20);
        assert_eq!(
            prepared.selected_in_scope_recovery_count,
            NON_LEADER_FALLBACK_MIN_ORDINARY_DEPLOY_CAP
        );
        assert_eq!(prepared.selected_ordinary_count, 0);
    }

    #[tokio::test]
    async fn parent_chain_in_scope_deploys_are_not_selected_for_recovery() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 128;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 500;
        let deploy = construct_deploy::source_deploy(
            "@parent_chain!(0)".to_string(),
            1_000,
            None,
            None,
            None,
            Some(1),
            Some("test".to_string()),
        )
        .expect("deploy");
        snapshot.deploys_in_scope.insert(current_id(&deploy));
        seed_current_deploys(&deploy_storage, [&deploy]);
        let lfb_hash = invalid_block_hash(0x94);
        let user_hash = invalid_block_hash(0x95);
        let lfb = test_block(lfb_hash.clone(), validator(1), Vec::new(), 250, Vec::new());
        let user = test_block(user_hash, validator(2), vec![lfb_hash.clone()], 299, vec![
            processed(deploy.clone()),
        ]);
        block_store.put_block_message(&lfb).expect("put lfb");
        block_store.put_block_message(&user).expect("put user");
        snapshot.last_finalized_block = lfb_hash;
        snapshot.max_block_num = 299;
        snapshot.parents = vec![user];

        let stats = in_scope_local_deploy_stats(
            &snapshot,
            300,
            10_000,
            &deploy_storage,
            &rejected_deploy_buffer,
            &block_store,
            None,
        )
        .expect("in-scope stats");
        assert_eq!(stats.count, 0);

        let prepared = prepare_user_deploys_with_policy(
            &snapshot,
            300,
            10_000,
            deploy_storage,
            rejected_deploy_buffer,
            &block_store,
            false,
            DeployAdmissionPolicy {
                allow_ordinary: false,
                ordinary_cap: 0,
                allow_in_scope_recovery: true,
                in_scope_recovery_cap: NON_LEADER_FALLBACK_MIN_ORDINARY_DEPLOY_CAP,
                reserve_tail: false,
                fallback: true,
                backpressure: true,
            },
            None,
        )
        .await
        .expect("prepare deploys");

        assert!(prepared.deploys.is_empty());
        assert_eq!(prepared.already_in_scope_count, 1);
        assert_eq!(prepared.selected_in_scope_recovery_count, 0);
    }

    #[tokio::test]
    async fn finalized_sibling_stranded_in_scope_deploy_is_selected_for_recovery() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 128;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 500;
        let deploy = construct_deploy::source_deploy(
            "@stranded!(0)".to_string(),
            1_000,
            None,
            None,
            None,
            Some(1),
            Some("test".to_string()),
        )
        .expect("deploy");
        snapshot.deploys_in_scope.insert(current_id(&deploy));
        seed_current_deploys(&deploy_storage, [&deploy]);
        let base_hash = invalid_block_hash(0x96);
        let finalized_hash = invalid_block_hash(0x97);
        let losing_hash = invalid_block_hash(0x98);
        let base = test_block(base_hash.clone(), validator(1), Vec::new(), 250, Vec::new());
        let finalized = test_block(
            finalized_hash.clone(),
            validator(1),
            vec![base_hash.clone()],
            300,
            Vec::new(),
        );
        let losing = test_block(
            losing_hash.clone(),
            validator(2),
            vec![base_hash],
            300,
            vec![processed(deploy.clone())],
        );
        block_store.put_block_message(&base).expect("put base");
        block_store
            .put_block_message(&finalized)
            .expect("put finalized");
        block_store.put_block_message(&losing).expect("put losing");
        snapshot.last_finalized_block = finalized_hash.clone();
        snapshot.max_block_num = 300;
        snapshot.parents = vec![finalized];

        let parent_hashes = vec![finalized_hash];
        let canonical_won =
            interpreter_util::canonical_won_sigs(&block_store, &parent_hashes, -200)
                .expect("canonical wins");
        assert!(!canonical_won.contains(&current_id(&deploy)));

        let stats = in_scope_local_deploy_stats(
            &snapshot,
            301,
            10_000,
            &deploy_storage,
            &rejected_deploy_buffer,
            &block_store,
            None,
        )
        .expect("in-scope stats");
        assert_eq!(stats.count, 1);
        assert_eq!(stats.stranded_count, 1);

        let prepared = prepare_user_deploys_with_policy(
            &snapshot,
            301,
            10_000,
            deploy_storage,
            rejected_deploy_buffer,
            &block_store,
            false,
            DeployAdmissionPolicy {
                allow_ordinary: false,
                ordinary_cap: 0,
                allow_in_scope_recovery: true,
                in_scope_recovery_cap: NON_LEADER_FALLBACK_MIN_ORDINARY_DEPLOY_CAP,
                reserve_tail: false,
                fallback: true,
                backpressure: false,
            },
            None,
        )
        .await
        .expect("prepare deploys");

        assert_eq!(prepared.deploys.len(), 1);
        assert!(prepared
            .deploys
            .iter()
            .any(|d| d.deploy_id() == &current_id_bytes(&deploy)));
        assert_eq!(prepared.already_in_scope_count, 1);
        assert_eq!(prepared.selected_in_scope_recovery_count, 1);
    }

    #[tokio::test]
    async fn finalized_in_scope_deploys_are_not_selected_for_recovery() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 128;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 500;
        let deploy = construct_deploy::source_deploy(
            "@finalized!(0)".to_string(),
            1_000,
            None,
            None,
            None,
            Some(1),
            Some("test".to_string()),
        )
        .expect("deploy");
        snapshot.deploys_in_scope.insert(current_id(&deploy));
        seed_current_deploys(&deploy_storage, [&deploy]);
        // Real geometry: the winning inclusion rides a block the parents
        // descend from (here: the parent itself), so the CANONICAL walk
        // over the parents excludes the sig — no finality marker consulted.
        let lfb_hash = invalid_block_hash(0x93);
        let lfb = test_block(lfb_hash.clone(), validator(1), Vec::new(), 250, vec![
            processed(deploy.clone()),
        ]);
        block_store.put_block_message(&lfb).expect("put lfb");
        snapshot.last_finalized_block = lfb_hash;
        snapshot.max_block_num = 250;
        snapshot.parents = vec![lfb];

        let stats = in_scope_local_deploy_stats(
            &snapshot,
            300,
            10_000,
            &deploy_storage,
            &rejected_deploy_buffer,
            &block_store,
            None,
        )
        .expect("in-scope stats");
        assert_eq!(stats.count, 0);

        let prepared = prepare_user_deploys_with_policy(
            &snapshot,
            300,
            10_000,
            deploy_storage,
            rejected_deploy_buffer,
            &block_store,
            false,
            DeployAdmissionPolicy {
                allow_ordinary: false,
                ordinary_cap: 0,
                allow_in_scope_recovery: true,
                in_scope_recovery_cap: NON_LEADER_FALLBACK_MIN_ORDINARY_DEPLOY_CAP,
                reserve_tail: false,
                fallback: true,
                backpressure: true,
            },
            None,
        )
        .await
        .expect("prepare deploys");

        assert!(prepared.deploys.is_empty());
        assert_eq!(prepared.already_in_scope_count, 1);
        assert_eq!(prepared.selected_in_scope_recovery_count, 0);
    }

    #[tokio::test]
    async fn ordinary_selection_stays_eligible_while_a_retry_is_gated() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot.parents.clear();
        seed_empty_last_finalized_block(&mut snapshot, &block_store);
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 10;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 50;
        let recovered = construct_deploy::basic_deploy_data(81, None, Some("test".to_string()))
            .expect("recovered deploy");
        let ordinary = construct_deploy::basic_deploy_data(82, None, Some("test".to_string()))
            .expect("ordinary deploy");

        seed_current_deploys(&deploy_storage, [&recovered, &ordinary]);
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![pending(recovered.clone())])
            .expect("seed rejected buffer");

        let prepared = prepare_user_deploys(
            &snapshot,
            20,
            ordinary.data.time_stamp,
            deploy_storage.clone(),
            rejected_deploy_buffer.clone(),
            &block_store,
            true,
            true,
        )
        .await
        .expect("prepare deploys");

        // The gated retry (no derivable floor here) must not block ordinary
        // selection: the ordinary deploy rides, the buffered one waits.
        assert_eq!(prepared.deploys.len(), 1);
        assert!(prepared
            .deploys
            .iter()
            .any(|deploy| deploy.deploy_id() == &current_id_bytes(&ordinary)));
        assert!(!prepared
            .deploys
            .iter()
            .any(|deploy| deploy.deploy_id() == &current_id_bytes(&recovered)));

        deploy_storage
            .lock()
            .remove_envelope_by_id(current_id(&recovered).as_bytes())
            .expect("drain recovered deploy storage");
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .remove(vec![pending(recovered.clone())])
            .expect("drain rejected buffer");

        let prepared = prepare_user_deploys(
            &snapshot,
            21,
            ordinary.data.time_stamp,
            deploy_storage,
            rejected_deploy_buffer,
            &block_store,
            true,
            true,
        )
        .await
        .expect("prepare deploys after recovery drain");

        assert_eq!(prepared.deploys.len(), 1);
        assert!(prepared
            .deploys
            .iter()
            .any(|deploy| deploy.deploy_id() == &current_id_bytes(&ordinary)));
    }

    #[test]
    fn recovered_retry_selection_is_stable_across_block_numbers() {
        let deploys: HashSet<_> = (1..=20)
            .map(|id| {
                construct_deploy::basic_deploy_data(id, None, Some("test".to_string()))
                    .map(pending)
                    .expect("deploy")
            })
            .collect();

        let selected_a = select_recovered_deploys_for_block(&deploys, 10, 4);
        let selected_b = select_recovered_deploys_for_block(&deploys, 11, 4);

        assert_eq!(selected_a.deploys.len(), 4);
        assert_eq!(selected_a.deploys, selected_b.deploys);
    }

    #[test]
    fn byte_budget_still_selects_one_oversize_deploy() {
        let deploy = construct_deploy::source_deploy(
            format!(
                "@\"{}\"!(Nil)",
                "x".repeat(USER_DEPLOY_BYTE_PROPOSAL_BUDGET + 1)
            ),
            1_000,
            None,
            None,
            None,
            Some(1),
            Some("test".to_string()),
        )
        .expect("deploy");
        let deploys = HashSet::from([pending(deploy)]);
        let selected = select_deploys_for_block(
            &deploys,
            ORDINARY_DEPLOY_PROPOSAL_CAP,
            false,
            USER_DEPLOY_BYTE_PROPOSAL_BUDGET,
        );

        assert_eq!(selected.deploys.len(), 1);
        assert!(selected.selected_bytes > USER_DEPLOY_BYTE_PROPOSAL_BUDGET);
        assert!(selected.byte_capped);
    }

    #[tokio::test]
    async fn recovered_buffered_deploy_waits_when_seen_in_scope() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot.parents.clear();
        seed_empty_last_finalized_block(&mut snapshot, &block_store);
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 10;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 50;
        let recovered = construct_deploy::basic_deploy_data(91, None, Some("test".to_string()))
            .expect("recovered deploy");

        seed_current_deploys(&deploy_storage, [&recovered]);
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![pending(recovered.clone())])
            .expect("seed rejected buffer");
        snapshot.deploys_in_scope.insert(current_id(&recovered));

        let prepared = prepare_user_deploys(
            &snapshot,
            20,
            recovered.data.time_stamp,
            deploy_storage,
            rejected_deploy_buffer,
            &block_store,
            true,
            true,
        )
        .await
        .expect("prepare deploys");

        assert!(
            prepared.deploys.is_empty(),
            "recovered deploys must wait while a prior clean inclusion is still in unresolved scope"
        );
    }

    #[tokio::test]
    async fn buffered_retry_without_derivable_floor_is_deferred_not_lost() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot.parents.clear();
        seed_empty_last_finalized_block(&mut snapshot, &block_store);
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 10;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 50;
        let recovered = construct_deploy::basic_deploy_data(92, None, Some("test".to_string()))
            .expect("recovered deploy");

        seed_current_deploys(&deploy_storage, [&recovered]);
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![pending(recovered.clone())])
            .expect("seed rejected buffer");
        snapshot.deploys_in_scope.insert(current_id(&recovered));
        snapshot.rejected_in_scope.insert(current_id(&recovered));

        let prepared = prepare_user_deploys(
            &snapshot,
            20,
            recovered.data.time_stamp,
            deploy_storage,
            rejected_deploy_buffer.clone(),
            &block_store,
            true,
            true,
        )
        .await
        .expect("prepare deploys");

        // A parentless snapshot derives no floor, and the retry gate defers
        // every re-proposal it cannot prove settled — delay, never loss: the
        // buffer keeps custody, and nothing is selected.
        assert_eq!(prepared.deploys.len(), 0);
        assert!(rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .contains_id(&current_id(&recovered))
            .expect("contains sig"));
    }

    // Inverted contract (issue #197): a tip-expired recovered deploy must be
    // excluded, but without a derivable finalized floor its irreversible purge
    // is deferred. A later floor can still prove the deploy live.
    #[tokio::test]
    async fn recovered_buffered_deploy_is_retained_without_floor_expiry_evidence() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        seed_empty_last_finalized_block(&mut snapshot, &block_store);
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 10;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 10;
        let recovered = construct_deploy::source_deploy_now(
            "@101!(101)".to_string(),
            None,
            Some(0),
            Some("test".to_string()),
        )
        .expect("recovered deploy");

        seed_current_deploys(&deploy_storage, [&recovered]);
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![pending(recovered.clone())])
            .expect("seed rejected buffer");

        let prepared = prepare_user_deploys(
            &snapshot,
            20,
            recovered.data.time_stamp,
            deploy_storage.clone(),
            rejected_deploy_buffer.clone(),
            &block_store,
            true,
            true,
        )
        .await
        .expect("prepare deploys");

        assert!(prepared.deploys.is_empty());
        assert!(deploy_storage
            .lock()
            .contains_envelope(current_id(&recovered).as_bytes())
            .expect("recovered storage membership"));
        assert!(rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .contains_id(&current_id(&recovered))
            .expect("rejected buffer contains"));
    }

    // A canonical-but-unfinalized win purges only the ordinary-storage copy;
    // the rejected-buffer entry survives (the winning block could still be
    // orphaned) until the finalized-won terminal purge removes it.
    #[tokio::test]
    async fn recovered_canonical_wins_are_purged_from_storage_only() {
        let mut kvm = InMemoryStoreManager::new();
        let mut deploy_storage = KeyValueDeployStorage::new(&mut kvm)
            .await
            .expect("deploy storage");
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let recovered_done =
            construct_deploy::basic_deploy_data(51, None, Some("test".to_string()))
                .expect("recovered done deploy");
        let recovered_pending =
            construct_deploy::basic_deploy_data(52, None, Some("test".to_string()))
                .expect("recovered pending deploy");
        let ordinary_done = construct_deploy::basic_deploy_data(53, None, Some("test".to_string()))
            .expect("ordinary done deploy");

        for deploy in [&recovered_done, &recovered_pending, &ordinary_done] {
            deploy_storage
                .add_envelope_if_absent(current_envelope(deploy))
                .expect("seed protocol-v6 deploy storage");
        }
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![
                pending(recovered_done.clone()),
                pending(recovered_pending.clone()),
            ])
            .expect("seed rejected buffer");

        let recovered_sigs: HashSet<DeployLookupId> =
            [current_id(&recovered_done), current_id(&recovered_pending)]
                .into_iter()
                .collect();
        let removed = purge_recovered_already_in_scope(
            &mut deploy_storage,
            &[
                pending(recovered_done.clone()),
                pending(ordinary_done.clone()),
            ],
            &recovered_sigs,
        )
        .expect("purge recovered already in scope");

        assert_eq!(removed, 1);
        assert!(!deploy_storage
            .contains_envelope(current_id(&recovered_done).as_bytes())
            .expect("recovered done storage membership"));
        assert!(deploy_storage
            .contains_envelope(current_id(&recovered_pending).as_bytes())
            .expect("recovered pending storage membership"));
        assert!(deploy_storage
            .contains_envelope(current_id(&ordinary_done).as_bytes())
            .expect("ordinary done storage membership"));

        let buffer_guard = rejected_deploy_buffer.lock().expect("rejected buffer lock");
        assert!(buffer_guard
            .contains_id(&current_id(&recovered_done))
            .expect("recovered done contains"));
        assert!(buffer_guard
            .contains_id(&current_id(&recovered_pending))
            .expect("recovered pending contains"));
    }

    #[tokio::test]
    async fn refund_failure_quarantine_removes_recovered_deploy_from_both_stores() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let deploy = construct_deploy::basic_deploy_data(42, None, Some("test".to_string()))
            .expect("deploy");

        deploy_storage
            .lock()
            .add(vec![deploy.clone()])
            .expect("add deploy");
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![legacy_pending(deploy.clone())])
            .expect("add recovered deploy");

        let msg = format!(
            "(Bug found) Deploy refund failed: Insufficient funds, deploy_sig={}, deployer_pk=04ffc016, refund_amount=4999911287",
            hex::encode(&deploy.sig)
        );
        let removed = quarantine_refund_failure_deploy(
            deploy_storage.clone(),
            rejected_deploy_buffer.clone(),
            crate::rust::casper::CERTIFIED_FINALIZED_FLOOR_PROTOCOL_VERSION - 1,
            &msg,
        )
        .expect("quarantine");

        assert_eq!(removed, (true, true));
        assert!(!deploy_storage
            .lock()
            .read_all()
            .expect("read deploy storage")
            .contains(&deploy));
        assert!(!rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .contains_id(&legacy_id(&deploy))
            .expect("contains sig"));
    }

    /// The terminal purge is irreversible, so it may key only on the one
    /// irreversible fact — the deploy's effect present in the FLOOR block's
    /// committed post-state. A win merely marked finalized by this node's
    /// finalizer can still sit above the justification-derived floor, where
    /// a later merge can reject it; evicting on that marker loses the only
    /// re-proposable copy. Absent floor-state evidence, the entry stays.
    #[tokio::test]
    async fn buffer_entry_is_kept_without_floor_state_evidence_of_its_effect() {
        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 10;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 50;
        let buffered = construct_deploy::basic_deploy_data(95, None, Some("test".to_string()))
            .expect("buffered deploy");

        // The node-local finalizer marks the winning block finalized; the
        // floor derivable from this snapshot never covers its effect.
        let won_block = test_block(invalid_block_hash(0x99), validator(1), Vec::new(), 1, vec![
            processed(buffered.clone()),
        ]);
        block_store
            .put_block_message(&won_block)
            .expect("store won block");
        snapshot.last_finalized_block = won_block.block_hash.clone();

        seed_current_deploys(&deploy_storage, [&buffered]);
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![pending(buffered.clone())])
            .expect("seed rejected buffer");

        let _prepared = prepare_user_deploys(
            &snapshot,
            20,
            buffered.data.time_stamp,
            deploy_storage,
            rejected_deploy_buffer.clone(),
            &block_store,
            true,
            true,
        )
        .await
        .expect("prepare deploys");

        assert!(
            rejected_deploy_buffer
                .lock()
                .expect("rejected buffer lock")
                .contains_id(&current_id(&buffered))
                .expect("contains sig"),
            "a buffer entry may be evicted only on floor-state evidence of \
             its effect; a node-local finality marker is not that evidence"
        );
    }

    /// Retry admission and removal read the FLOOR-clock validity window,
    /// not the tip clock. A rejected deploy whose window is closed at the
    /// tip but open at the floor can still land — the merge window and
    /// expiry validity both key on the floor — so selection must keep
    /// offering it and removal (irreversible) must not delete the only
    /// re-proposable copy on the faster clock. The retry gate is OPEN in
    /// this staging: the deploy's kept rejection rides in the floor block
    /// itself, a settled adjudication.
    #[tokio::test]
    async fn tip_expired_floor_live_rejected_deploy_stays_retryable() {
        use block_storage::rust::dag::block_dag_key_value_storage::{
            BlockDagKeyValueStorage, InsertMode,
        };

        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 10;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 50;

        let retry = construct_deploy::source_deploy(
            "@71!(71)".to_string(),
            1_000,
            None,
            None,
            None,
            Some(6),
            Some("test".to_string()),
        )
        .expect("retry deploy");

        // Floor pinned at #55: the parentless root reads as genesis to the
        // cold frontier walk (finalized by definition), so the derived floor
        // is the root. The tip sits at #60 and proposes #61, so the
        // tip-clock window (edge 6+50 = 56) is closed for the valid_after-6
        // deploy while the floor-clock window (bound 55-50 = 5) is open.
        // The floor block carries the retry's kept rejection record — a
        // settled adjudication inside the walk window (#11..) — so the
        // retry gate is open.
        let mut genesis_block = test_block(
            invalid_block_hash(0xA0),
            validator(1),
            Vec::new(),
            55,
            Vec::new(),
        );
        genesis_block.body.rejected_deploys = vec![current_rejected(
            &retry,
            genesis_block.block_hash.clone(),
            models::rust::casper::protocol::casper_message::RejectedDeployReason::MergeConflict,
        )];
        let parent_block = test_block(
            invalid_block_hash(0xA1),
            validator(1),
            vec![genesis_block.block_hash.clone()],
            60,
            Vec::new(),
        );
        block_store
            .put_block_message(&genesis_block)
            .expect("store genesis");
        block_store
            .put_block_message(&parent_block)
            .expect("store parent");
        dag_storage
            .insert(&genesis_block, InsertMode::SettledHistory)
            .expect("insert genesis");
        dag_storage
            .insert(&parent_block, InsertMode::Normal)
            .expect("insert parent");
        snapshot.dag = dag_storage.get_representation().expect("dag");
        snapshot.parents = vec![parent_block];

        seed_current_deploys(&deploy_storage, [&retry]);
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![pending(retry.clone())])
            .expect("seed rejected buffer");

        let prepared = prepare_user_deploys(
            &snapshot,
            61,
            10_000,
            deploy_storage,
            rejected_deploy_buffer.clone(),
            &block_store,
            true,
            true,
        )
        .await
        .expect("prepare deploys");

        assert!(
            rejected_deploy_buffer
                .lock()
                .expect("rejected buffer lock")
                .contains_id(&current_id(&retry))
                .expect("contains sig"),
            "removal is floor-clock: a floor-live entry must not be deleted \
             on the tip clock"
        );
        assert!(
            prepared
                .deploys
                .iter()
                .any(|d| d.deploy_id() == &current_id_bytes(&retry)),
            "retry admission is floor-clock: a tip-expired floor-live \
             rejected deploy stays selectable"
        );
    }

    #[tokio::test]
    async fn retry_frontier_uses_collective_parent_coverage_and_bounded_lease() {
        use block_storage::rust::dag::block_dag_key_value_storage::{
            BlockDagKeyValueStorage, InsertMode,
        };

        let mut kvm = InMemoryStoreManager::new();
        let deploy_storage = Arc::new(parking_lot::Mutex::new(
            KeyValueDeployStorage::new(&mut kvm)
                .await
                .expect("deploy storage"),
        ));
        let rejected_deploy_buffer = Arc::new(Mutex::new(
            KeyValueRejectedDeployBuffer::new(&mut kvm)
                .await
                .expect("rejected deploy buffer"),
        ));
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let dag_storage = BlockDagKeyValueStorage::new(&mut kvm)
            .await
            .expect("dag storage");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 10;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 50;

        let retry = construct_deploy::source_deploy(
            "@71!(71)".to_string(),
            1_000,
            None,
            None,
            None,
            Some(20),
            Some("test".to_string()),
        )
        .expect("retry deploy");
        let mut floor = test_block(
            invalid_block_hash(0xB0),
            validator(1),
            Vec::new(),
            59,
            Vec::new(),
        );
        floor.body.rejected_deploys = vec![current_rejected(
            &retry,
            floor.block_hash.clone(),
            models::rust::casper::protocol::casper_message::RejectedDeployReason::MergeConflict,
        )];
        let left = test_block(
            invalid_block_hash(0xB1),
            validator(1),
            vec![floor.block_hash.clone()],
            60,
            Vec::new(),
        );
        let right = test_block(
            invalid_block_hash(0xB2),
            validator(2),
            vec![floor.block_hash.clone()],
            60,
            Vec::new(),
        );
        for block in [&floor, &left, &right] {
            block_store.put_block_message(block).expect("store block");
        }
        dag_storage
            .insert(&floor, InsertMode::SettledHistory)
            .expect("insert floor");
        dag_storage
            .insert(&left, InsertMode::Normal)
            .expect("insert left parent");
        dag_storage
            .insert(&right, InsertMode::Normal)
            .expect("insert right parent");
        snapshot.dag = dag_storage.get_representation().expect("dag");
        snapshot.justifications = [
            Justification {
                validator: validator(1),
                latest_block_hash: left.block_hash.clone(),
            },
            Justification {
                validator: validator(2),
                latest_block_hash: right.block_hash.clone(),
            },
        ]
        .into_iter()
        .collect();
        snapshot.parents = vec![left.clone(), right.clone()];

        seed_current_deploys(&deploy_storage, [&retry]);
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![pending(retry.clone())])
            .expect("seed rejected buffer");

        let prepared = prepare_user_deploys(
            &snapshot,
            61,
            10_000,
            deploy_storage.clone(),
            rejected_deploy_buffer.clone(),
            &block_store,
            true,
            true,
        )
        .await
        .expect("prepare deploys with collectively covering parents");

        assert!(
            prepared
                .deploys
                .iter()
                .any(|deploy| deploy.deploy_id() == &current_id_bytes(&retry)),
            "split selected parents collectively cover their visible latest messages"
        );

        snapshot.parents.reverse();
        let mut reversed_justifications = snapshot.justifications.to_vec();
        reversed_justifications.reverse();
        snapshot.justifications = reversed_justifications.into_iter().collect();
        let prepared = prepare_user_deploys(
            &snapshot,
            61,
            10_000,
            deploy_storage.clone(),
            rejected_deploy_buffer.clone(),
            &block_store,
            true,
            true,
        )
        .await
        .expect("prepare deploys with permuted collective coverage");
        assert!(
            prepared
                .deploys
                .iter()
                .any(|deploy| deploy.deploy_id() == &current_id_bytes(&retry)),
            "parent and latest-message order must not change collective coverage"
        );

        let missing = test_block(
            invalid_block_hash(0xB3),
            validator(3),
            vec![floor.block_hash.clone()],
            60,
            Vec::new(),
        );
        block_store
            .put_block_message(&missing)
            .expect("store uncovered latest message");
        dag_storage
            .insert(&missing, InsertMode::Normal)
            .expect("insert uncovered latest message");
        snapshot.dag = dag_storage.get_representation().expect("updated dag");
        snapshot.justifications = [
            Justification {
                validator: validator(1),
                latest_block_hash: left.block_hash.clone(),
            },
            Justification {
                validator: validator(2),
                latest_block_hash: right.block_hash.clone(),
            },
            Justification {
                validator: validator(3),
                latest_block_hash: missing.block_hash.clone(),
            },
        ]
        .into_iter()
        .collect();
        snapshot.parents = vec![left.clone(), right.clone()];

        let prepared = prepare_user_deploys(
            &snapshot,
            61,
            10_000,
            deploy_storage.clone(),
            rejected_deploy_buffer.clone(),
            &block_store,
            true,
            true,
        )
        .await
        .expect("prepare deploys with an incomplete selected frontier");

        assert!(
            prepared.deploys.is_empty(),
            "an uncovered valid latest message must defer retry before lease expiry"
        );
        assert!(
            rejected_deploy_buffer
                .lock()
                .expect("rejected buffer lock")
                .contains_id(&current_id(&retry))
                .expect("contains retry after deferral"),
            "frontier deferral must retain owner custody"
        );

        snapshot.parents = vec![left.clone(), right.clone(), missing.clone()];
        let prepared = prepare_user_deploys(
            &snapshot,
            62,
            10_000,
            deploy_storage.clone(),
            rejected_deploy_buffer.clone(),
            &block_store,
            true,
            true,
        )
        .await
        .expect("prepare deploys after collective frontier completion");
        assert!(
            prepared
                .deploys
                .iter()
                .any(|deploy| deploy.deploy_id() == &current_id_bytes(&retry)),
            "collective coverage must admit retry without a serial coalescing parent"
        );

        // Reflexive cover: a selected parent that IS the sole valid latest
        // message covers the frontier by itself, inside the lease.
        snapshot.justifications = [Justification {
            validator: validator(1),
            latest_block_hash: left.block_hash.clone(),
        }]
        .into_iter()
        .collect();
        snapshot.parents = vec![left.clone()];
        let prepared = prepare_user_deploys(
            &snapshot,
            61,
            10_000,
            deploy_storage.clone(),
            rejected_deploy_buffer.clone(),
            &block_store,
            true,
            true,
        )
        .await
        .expect("prepare deploys with a parent that is a latest message");

        assert!(
            prepared
                .deploys
                .iter()
                .any(|deploy| deploy.deploy_id() == &current_id_bytes(&retry)),
            "a parent that is itself the latest message must count as covering it"
        );

        snapshot.justifications = [
            Justification {
                validator: validator(1),
                latest_block_hash: left.block_hash.clone(),
            },
            Justification {
                validator: validator(2),
                latest_block_hash: right.block_hash.clone(),
            },
            Justification {
                validator: validator(3),
                latest_block_hash: missing.block_hash.clone(),
            },
        ]
        .into_iter()
        .collect();
        snapshot.parents = vec![left, right];
        let prepared = prepare_user_deploys(
            &snapshot,
            63,
            10_000,
            deploy_storage,
            rejected_deploy_buffer,
            &block_store,
            true,
            true,
        )
        .await
        .expect("prepare deploys after frontier deferral lease");

        assert!(
            prepared
                .deploys
                .iter()
                .any(|deploy| deploy.deploy_id() == &current_id_bytes(&retry)),
            "the bounded lease must prevent frontier deferral from consuming the validity window"
        );
    }
}
