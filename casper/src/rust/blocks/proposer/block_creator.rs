// References below to `formal/{rocq,tlaplus,sage}/slashing/`,
// `FINDINGS.md`, `slashing-search-horizon.{md,sh}`, `slashing-traceability.md`,
// `docs/casper/theory/slashing/methodology/`, and `.mutants.toml` point at
// audit-corpus artifacts preserved on the `analysis/slashing` branch.
//
// See casper/src/main/scala/coop/rchain/casper/blocks/proposer/BlockCreator.scala

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, Bond, DeployData, F1r3flyState, Header, Justification, ProcessedDeploy,
    ProcessedSystemDeploy, RejectedDeploy,
};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use prost::Message;
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::errors::HistoryError;
use tracing;

use crate::rust::blocks::proposer::propose_result::BlockCreatorResult;
use crate::rust::casper::CasperSnapshot;
use crate::rust::errors::CasperError;
use crate::rust::finality::floor_context::{FloorContext, RetryGateBasis};
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
pub struct PreparedUserDeploys {
    pub deploys: HashSet<Signed<DeployData>>,
    pub effective_cap: usize,
    pub cap_hit: bool,
    pub selected_retry_count: usize,
    pub selected_ordinary_count: usize,
    pub selected_in_scope_recovery_count: usize,
    pub selected_in_scope_recovery_sigs: HashSet<Bytes>,
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
    deploys: HashSet<Signed<DeployData>>,
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

/// C15 / Smell-4: extract the deploy-signature pretty-print prefix
/// used in operator-facing log messages. Previously inlined as
/// `deploy_sig_prefix(&d.sig)` at four
/// sites in `log_deploy_pool_filtering`.
fn deploy_sig_prefix(sig: &Bytes) -> String { hex::encode(&sig[..std::cmp::min(8, sig.len())]) }

/// One line per deferred retry with the gate's basis — a recurring deferral
/// for one sig is the starvation tripwire, and the basis says which of the
/// gate's closed conditions is holding it.
fn trace_retry_gate_deferral(sig: &Bytes, basis: &RetryGateBasis, ctx: &FloorContext) {
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
        sig = %deploy_sig_prefix(sig),
        basis = basis_name,
        record_block = %record_block,
        floor_number = ctx.floor.block_number,
        "retry deferred by the gate"
    );
}

fn ordered_user_deploys(deploys: &HashSet<Signed<DeployData>>) -> Vec<Signed<DeployData>> {
    let mut ordered: Vec<Signed<DeployData>> = deploys.iter().cloned().collect();
    ordered.sort_by(|a, b| {
        a.data
            .valid_after_block_number
            .cmp(&b.data.valid_after_block_number)
            .then_with(|| a.data.time_stamp.cmp(&b.data.time_stamp))
            .then_with(|| a.sig.cmp(&b.sig))
    });
    ordered
}

#[cfg(test)]
fn select_recovered_deploys_for_block(
    deploys: &HashSet<Signed<DeployData>>,
    _block_number: i64,
    cap: usize,
) -> DeploySelection {
    select_deploys_for_block(deploys, cap, false, USER_DEPLOY_BYTE_PROPOSAL_BUDGET)
}

fn deploy_encoded_len(deploy: &Signed<DeployData>) -> usize {
    DeployData::to_proto_ref(deploy).encoded_len()
}

fn select_deploys_for_block(
    deploys: &HashSet<Signed<DeployData>>,
    cap: usize,
    reserve_tail: bool,
    byte_budget: usize,
) -> DeploySelection {
    // One proto encoding per deploy per selection call: sizes are computed
    // here and reused for both the total and the budget loop below.
    let sizes: HashMap<Bytes, usize> = deploys
        .iter()
        .map(|d| (d.sig.clone(), deploy_encoded_len(d)))
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
    let (candidates, strategy): (Vec<Signed<DeployData>>, &'static str) = if ordered.len() <= cap {
        (ordered, "uncapped")
    } else if reserve_tail && DEPLOY_SELECTION_RESERVE_TAIL_ENABLED && cap > 1 {
        let oldest_take = cap.saturating_sub(1);
        let mut candidates: Vec<Signed<DeployData>> =
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
        let deploy_bytes = sizes.get(&deploy.sig).copied().unwrap_or_default();
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
async fn derive_floor_context(
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
) -> Result<HashSet<Bytes>, CasperError> {
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

    let stored_unfinalized: HashSet<Signed<DeployData>> =
        if allow_ordinary_deploys || allow_in_scope_recovery {
            deploy_storage_guard.read_all()?
        } else {
            HashSet::new()
        };

    let mut buffered_deploys: HashSet<Signed<DeployData>> =
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
    let expired_buffered: Vec<Signed<DeployData>> = buffered_deploys
        .iter()
        .filter(|deploy| {
            deploy.data.is_expired_at(current_time_millis)
                || floor_expiry_bound.is_some_and(|bound| !not_expired_deploy(bound, &deploy.data))
        })
        .cloned()
        .collect();
    if !expired_buffered.is_empty() {
        for deploy in &expired_buffered {
            tracing::info!(
                target: "f1r3fly.casper.deploy_lifecycle",
                event = "buffer_removed",
                deploy_sig = %hex::encode(&deploy.sig),
                reason = "expired",
                valid_after_block = deploy.data.valid_after_block_number,
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
        deploy_storage_guard.remove(expired_buffered.clone())?;
        rejected_deploy_buffer
            .lock()
            .map_err(|e| CasperError::LockError(e.to_string()))?
            .remove(expired_buffered.clone())?;
        let expired_sigs: HashSet<Bytes> = expired_buffered
            .into_iter()
            .map(|deploy| deploy.sig)
            .collect();
        buffered_deploys.retain(|deploy| !expired_sigs.contains(&deploy.sig));
    }
    let mut buffered_sigs: HashSet<Bytes> =
        buffered_deploys.iter().map(|d| d.sig.clone()).collect();

    let skipped_buffered_ordinary = if allow_ordinary_deploys && !allow_recovered_deploys {
        stored_unfinalized
            .iter()
            .filter(|deploy| buffered_sigs.contains(&deploy.sig))
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
        .map(|d| d.data.valid_after_block_number)
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
    let settled_buffered: Vec<Signed<DeployData>> = match floor_ctx {
        Some(ctx) if !buffered_deploys.is_empty() => {
            let mut settled = Vec::new();
            for deploy in &buffered_deploys {
                if ctx.effect_settled_in_floor(
                    block_store,
                    deploy.data.valid_after_block_number,
                    &deploy.sig,
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
            buffered_sigs.remove(&deploy.sig);
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
    let recovered: HashSet<Signed<DeployData>> = if allow_recovered_deploys {
        let mut kept: HashSet<Signed<DeployData>> = HashSet::new();
        let mut gated_count = 0usize;
        for deploy in buffered_deploys {
            let candidate = !canonical_won_buffer_sigs.contains(&deploy.sig)
                && (!casper_snapshot.deploys_in_scope.contains(&deploy.sig)
                    || casper_snapshot.rejected_in_scope.contains(&deploy.sig));
            if !candidate {
                continue;
            }
            match floor_ctx {
                Some(ctx) => {
                    match ctx.retry_gate_basis(
                        &casper_snapshot.dag,
                        block_store,
                        earliest_block_number,
                        &deploy.sig,
                    )? {
                        RetryGateBasis::Open => {
                            kept.insert(deploy);
                        }
                        basis => {
                            trace_retry_gate_deferral(&deploy.sig, &basis, ctx);
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
    let recovered_sigs: HashSet<Bytes> = recovered.iter().map(|d| d.sig.clone()).collect();
    let recovery_backlog = allow_recovered_deploys && !recovered.is_empty();
    let storage_scan_allowed_now = allow_ordinary_deploys || allow_in_scope_recovery;

    let ordinary_kept_with_recovery = if recovery_backlog && allow_ordinary_deploys {
        stored_unfinalized
            .iter()
            .filter(|deploy| !buffered_sigs.contains(&deploy.sig))
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

    let unfinalized: HashSet<Signed<DeployData>> = if storage_scan_allowed_now {
        stored_unfinalized
            .into_iter()
            .filter(|deploy| !buffered_sigs.contains(&deploy.sig))
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
                deploy_sig = %hex::encode(sig),
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
                deploy_sig = %hex::encode(&deploy.sig),
                valid_after_block = deploy.data.valid_after_block_number,
                in_scope = casper_snapshot.deploys_in_scope.contains(&deploy.sig),
                rejected_in_scope = casper_snapshot.rejected_in_scope.contains(&deploy.sig),
                next_block = block_number,
                "deploy lifecycle"
            );
        }
        let recovered_sigs: Vec<String> = recovered
            .iter()
            .map(|d| hex::encode(&d.sig[..d.sig.len().min(8)]))
            .collect();
        tracing::info!(
            target: "f1r3fly.casper.recovery",
            "Prepare user deploys: {} recovered from rejected-deploy buffer; sigs={:?}",
            recovered_count,
            recovered_sigs
        );
    }
    let unfinalized: HashSet<Signed<DeployData>> = unfinalized
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
            recovered_sigs.contains(&d.sig) || casper_snapshot.rejected_in_scope.contains(&d.sig)
        })
        .map(|d| d.data.valid_after_block_number)
        .min()
        .map(|h| h.min(earliest_block_number))
        .unwrap_or(earliest_block_number);

    // Retry work (buffered or rejected-in-scope) reads the FLOOR-clock
    // window for block expiry; ordinary deploys keep the tip clock, which
    // is never looser than the floor's, so nothing leaks back. Absent a
    // derivable floor, retry work also falls back to the tip clock for
    // ADMISSION only (removal below defers instead — deletion is
    // irreversible, admission is retried next round).
    let is_retry_sig = |sig: &Bytes| {
        buffered_sigs.contains(sig) || casper_snapshot.rejected_in_scope.contains(sig)
    };
    let block_expiry_bound = |deploy: &Signed<DeployData>| {
        if is_retry_sig(&deploy.sig) {
            floor_expiry_bound.unwrap_or(earliest_block_number)
        } else {
            earliest_block_number
        }
    };

    // Categorize deploys for logging
    let future_deploys: Vec<_> = unfinalized
        .iter()
        .filter(|d| !not_future_deploy(block_number, &d.data))
        .collect();
    let block_expired_deploys: Vec<_> = unfinalized
        .iter()
        .filter(|d| !not_expired_deploy(block_expiry_bound(d), &d.data))
        .collect();
    let time_expired_deploys: Vec<_> = unfinalized
        .iter()
        .filter(|d| d.data.is_expired_at(current_time_millis))
        .collect();

    // Filter valid deploys (not expired by block, not expired by time, and
    // not future). Block expiry applies to recovered and rejected-retry
    // deploys too — on the floor clock: the merge window rule and expiry
    // validity read the same bound, so a floor-window-closed deploy
    // admitted here could only yield a block that fails its own
    // validation, rebuilt every propose (the permanent finalization
    // wedge). Expiry is a chain-level invariant; recovery cannot outlive
    // it — but the clock that closes it is the floor's, never the tip's.
    let valid: HashSet<Signed<DeployData>> = unfinalized
        .iter()
        .filter(|deploy| {
            not_future_deploy(block_number, &deploy.data)
                && not_expired_deploy(block_expiry_bound(deploy), &deploy.data)
                && !deploy.data.is_expired_at(current_time_millis)
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

    let recovered_canonical_wins: Vec<Signed<DeployData>> = valid
        .iter()
        .filter(|deploy| {
            recovered_sigs.contains(&deploy.sig) && canonical_won.contains(&deploy.sig)
        })
        .cloned()
        .collect();
    let retry_scope_exempt = |deploy: &Signed<DeployData>| {
        recovered_sigs.contains(&deploy.sig)
            && casper_snapshot.rejected_in_scope.contains(&deploy.sig)
    };
    let blocked_by_scope = |deploy: &Signed<DeployData>| {
        casper_snapshot.deploys_in_scope.contains(&deploy.sig) && !retry_scope_exempt(deploy)
    };
    let already_in_scope: Vec<Signed<DeployData>> = valid
        .iter()
        .filter(|deploy| canonical_won.contains(&deploy.sig) || blocked_by_scope(deploy))
        .map(|deploy| (*deploy).clone())
        .collect();
    let valid_unique: HashSet<Signed<DeployData>> = valid
        .into_iter()
        .filter(|deploy| !canonical_won.contains(&deploy.sig) && !blocked_by_scope(deploy))
        .collect();

    let already_in_scope_count = already_in_scope.len();
    for deploy in &recovered_canonical_wins {
        tracing::info!(
            target: "f1r3fly.casper.deploy_lifecycle",
            event = "storage_removed",
            deploy_sig = %hex::encode(&deploy.sig),
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
    let is_retry_candidate = |deploy: &Signed<DeployData>| {
        recovered_sigs.contains(&deploy.sig)
            || casper_snapshot.rejected_in_scope.contains(&deploy.sig)
    };
    // The gate covers EVERY retry route. `recovered_sigs` already passed it
    // above; the pool route (rejected-in-scope, not buffered — reachable
    // under deep floor lag, where the record is in the walk window but its
    // carrier is below it) must pass the same predicate, or the proposer
    // mints a block every validator rejects as `PrematureDeployRetry`.
    let mut retry_candidates: HashSet<Signed<DeployData>> = HashSet::new();
    let mut gated_pool_retries = 0usize;
    for deploy in valid_unique.iter().filter(|d| is_retry_candidate(d)) {
        if recovered_sigs.contains(&deploy.sig) {
            retry_candidates.insert(deploy.clone());
            continue;
        }
        match floor_ctx {
            Some(ctx) => {
                match ctx.retry_gate_basis(
                    &casper_snapshot.dag,
                    block_store,
                    earliest_block_number,
                    &deploy.sig,
                )? {
                    RetryGateBasis::Open => {
                        retry_candidates.insert(deploy.clone());
                    }
                    basis => {
                        trace_retry_gate_deferral(&deploy.sig, &basis, ctx);
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
    let mut retry_frontier_merged = false;
    'parents: for parent in &casper_snapshot.parents {
        for justification in &casper_snapshot.justifications {
            if !casper_snapshot
                .invalid_blocks
                .contains_key(&justification.latest_block_hash)
                && !casper_snapshot
                    .dag
                    .is_dag_ancestor(&justification.latest_block_hash, &parent.block_hash)?
            {
                continue 'parents;
            }
        }
        retry_frontier_merged = true;
        break;
    }
    if !retry_frontier_merged {
        retry_candidates.clear();
    }
    let ordinary_candidates: HashSet<Signed<DeployData>> = valid_unique
        .iter()
        .filter(|deploy| !is_retry_candidate(deploy))
        .cloned()
        .collect();
    let in_scope_recovery_candidates: HashSet<Signed<DeployData>> = if allow_in_scope_recovery {
        already_in_scope
            .iter()
            .filter(|deploy| {
                !canonical_won.contains(&deploy.sig)
                    && casper_snapshot.deploys_in_scope.contains(&deploy.sig)
                    && !casper_snapshot.rejected_in_scope.contains(&deploy.sig)
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
    let selected_in_scope_recovery_sigs: HashSet<Bytes> = selected_in_scope_recovery
        .iter()
        .map(|deploy| deploy.sig.clone())
        .collect();
    let selected: HashSet<Signed<DeployData>> = retry_selection
        .deploys
        .into_iter()
        .chain(ordinary_selection.deploys.into_iter())
        .chain(selected_in_scope_recovery.into_iter())
        .collect();
    for deploy in &selected {
        tracing::info!(
            target: "f1r3fly.casper.deploy_lifecycle",
            event = "selected",
            deploy_sig = %hex::encode(&deploy.sig),
            next_block = block_number,
            retry = is_retry_candidate(deploy),
            in_scope_recovery = selected_in_scope_recovery_sigs.contains(&deploy.sig),
            valid_after_block = deploy.data.valid_after_block_number,
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
    let selected_retry_count = selected
        .iter()
        .filter(|deploy| is_retry_candidate(deploy))
        .count();
    let selected_in_scope_recovery_count = selected
        .iter()
        .filter(|deploy| selected_in_scope_recovery_sigs.contains(&deploy.sig))
        .count();
    let selected_ordinary_count = selected
        .iter()
        .filter(|deploy| {
            !is_retry_candidate(deploy) && !selected_in_scope_recovery_sigs.contains(&deploy.sig)
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
        for d in &future_deploys {
            tracing::debug!(
                target: "f1r3fly.merge.cpps",
                step = "prepare_user_deploys.FILTER",
                deploy = %hex::encode(&d.sig[..8.min(d.sig.len())]),
                decision = "filtered",
                reason = "future",
                "merge.cpps: deploy filter decision"
            );
        }
        for d in &block_expired_deploys {
            tracing::debug!(
                target: "f1r3fly.merge.cpps",
                step = "prepare_user_deploys.FILTER",
                deploy = %hex::encode(&d.sig[..8.min(d.sig.len())]),
                decision = "filtered",
                reason = "block-expired",
                "merge.cpps: deploy filter decision"
            );
        }
        for d in &time_expired_deploys {
            tracing::debug!(
                target: "f1r3fly.merge.cpps",
                step = "prepare_user_deploys.FILTER",
                deploy = %hex::encode(&d.sig[..8.min(d.sig.len())]),
                decision = "filtered",
                reason = "time-expired",
                "merge.cpps: deploy filter decision"
            );
        }
        for d in already_in_scope
            .iter()
            .filter(|d| !selected_in_scope_recovery_sigs.contains(&d.sig))
        {
            tracing::debug!(
                target: "f1r3fly.merge.cpps",
                step = "prepare_user_deploys.FILTER",
                deploy = %hex::encode(&d.sig[..8.min(d.sig.len())]),
                decision = "filtered",
                reason = "already-in-scope (repeat_deploy / deploys_in_scope, non-stale)",
                "merge.cpps: deploy filter decision"
            );
        }
        for d in &valid_unique {
            tracing::debug!(
                target: "f1r3fly.merge.cpps",
                step = "prepare_user_deploys.FILTER",
                deploy = %hex::encode(&d.sig[..8.min(d.sig.len())]),
                decision = "selected-candidate",
                reason = "passed expiry + scope filters",
                "merge.cpps: deploy filter decision"
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
    for d in already_in_scope
        .iter()
        .filter(|d| !selected_in_scope_recovery_sigs.contains(&d.sig))
    {
        tracing::warn!(
            "Deploy {}... FILTERED (already in scope): deploy already exists in DAG within lifespan window",
            deploy_sig_prefix(&d.sig)
        );
    }

    // Remove all expired deploys from storage to prevent them from triggering
    // future proposals. Combine block-expired and time-expired, avoiding
    // duplicates. Removal is irreversible, so block-expiry removal of RETRY
    // work requires the floor bound — with no derivable floor, retry work is
    // excluded here and re-judged next round (delay, never loss); its
    // admission-side filter above already deferred on the same fact.
    let all_expired: HashSet<&Signed<DeployData>> = block_expired_deploys
        .iter()
        .filter(|d| floor_expiry_bound.is_some() || !is_retry_sig(&d.sig))
        .chain(time_expired_deploys.iter())
        .cloned()
        .collect();
    if !all_expired.is_empty() {
        for deploy in &all_expired {
            tracing::info!(
                target: "f1r3fly.casper.deploy_lifecycle",
                event = "storage_and_buffer_removed",
                deploy_sig = %hex::encode(&deploy.sig),
                reason = "expired",
                block_expired = block_expired_deploys.iter().any(|item| item.sig == deploy.sig),
                time_expired = time_expired_deploys.iter().any(|item| item.sig == deploy.sig),
                valid_after_block = deploy.data.valid_after_block_number,
                floor_expiry_bound = ?floor_expiry_bound,
                next_block = block_number,
                "deploy lifecycle"
            );
        }
        tracing::info!(
            "Removing {} expired deploy(s) from storage and rejected-deploy buffer",
            all_expired.len()
        );
        let expired_list: Vec<Signed<DeployData>> = all_expired.into_iter().cloned().collect();
        deploy_storage_guard.remove(expired_list.clone())?;

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
        let chosen: Vec<String> = selected
            .iter()
            .map(|d| hex::encode(&d.sig[..8.min(d.sig.len())]))
            .collect();
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
        selected_in_scope_recovery_sigs,
        already_in_scope_count,
        selected_user_deploy_bytes,
        deferred_user_deploy_bytes,
        byte_cap_hit,
    })
}

fn scope_has_unfinalized_user_deploys(
    casper_snapshot: &CasperSnapshot,
    block_store: &KeyValueBlockStore,
) -> Result<bool, CasperError> {
    let last_finalized_block_number = block_store
        .get(&casper_snapshot.last_finalized_block)?
        .map(|block| block.body.state.block_number);
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

        let Some(block) = block_store.get(&block_hash)? else {
            continue;
        };

        if last_finalized_block_number
            .map(|lfb_number| block.body.state.block_number <= lfb_number)
            .unwrap_or(false)
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
) -> Result<HashSet<Bytes>, CasperError> {
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

        let Some(block) = block_store.get(&block_hash)? else {
            continue;
        };

        if last_finalized_block_number
            .map(|lfb_number| block.body.state.block_number <= lfb_number)
            .unwrap_or(false)
            || block.body.state.block_number <= earliest_block_number
        {
            continue;
        }

        let excluded = excluded_block_hash
            .map(|excluded_hash| excluded_hash == &block_hash)
            .unwrap_or(false);
        if !excluded {
            for deploy in &block.body.deploys {
                sigs.insert(deploy.deploy.sig.clone());
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
    let Some(block) = block_store.get(&info.block_hash)? else {
        return Ok(info);
    };
    let deploy_sigs: HashSet<Bytes> = block
        .body
        .deploys
        .iter()
        .map(|deploy| deploy.deploy.sig.clone())
        .collect();
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
        .map(|block| block.body.state.block_number);
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

        let Some(block) = block_store.get(&block_hash)? else {
            continue;
        };

        if last_finalized_block_number
            .map(|lfb_number| block.body.state.block_number <= lfb_number)
            .unwrap_or(false)
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
                last_finalized_block_number,
            )
        })
        .transpose()
}

fn storage_has_unresolved_in_scope_deploys(
    casper_snapshot: &CasperSnapshot,
    deploy_storage: &Arc<parking_lot::Mutex<KeyValueDeployStorage>>,
    _rejected_deploy_buffer: &Arc<Mutex<KeyValueRejectedDeployBuffer>>,
) -> Result<bool, CasperError> {
    let stored_deploys = deploy_storage.lock().read_all()?;
    for deploy in stored_deploys {
        if casper_snapshot.deploys_in_scope.contains(&deploy.sig) {
            return Ok(true);
        }
    }
    Ok(false)
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
///
/// Merge of dev (EPOCH-004) into feature/slashing: production callers
/// of this filter were replaced by `slashing_authorization::
/// authorized_slash_candidates`, which is the full T-9.8 conjunctive
/// predicate (bonded-target ∧ active-validator ∧ epoch-match ∧
/// evidence-epoch-match). This helper is retained under
/// `#[cfg(test)]` because the test suite below pins the
/// `bonded ∧ active` subset of T-9.8 directly — a regression catch for
/// any future refactor of `authorized_slash_candidates` that drops one
/// of those clauses.
#[cfg(test)]
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

/// Build one `SlashDeploy` from its offender evidence. BOTH proposer-side slash
/// paths — the freshly-detected `prepare_slashing_deploys` and the merge-rejected
/// `recovered_rejected_slashes` recovery in `create_block` — previously constructed the
/// deploy with two byte-identical inline copies. This single seam is where the deploy's
/// `initial_rand` seed is wired, and by MainTheorem T-Slash
/// (`main_TSlash_deploy_seed_uses_invalid_block_hash`,
/// formal/rocq/slashing/theories/MainTheorem.v:302) that seed MUST be a pure function of
/// `(proposer pubkey, seq_num, invalid_block_hash)` — deriving it from the offender's OWN
/// `invalid_block_hash` is what lets every node and the replay path recompute the identical
/// randomness. Extracting the copies removes the standing risk that a future edit to one
/// silently diverges the seed-wiring (mirrors `finality::floor::floor_committee`).
fn build_slash_deploy(
    invalid_block_hash: &BlockHash,
    proposer_public_key: &PublicKey,
    target_activation_epoch: i64,
    seq_num: i32,
) -> SlashDeploy {
    let self_id = Bytes::copy_from_slice(&proposer_public_key.bytes);
    SlashDeploy {
        invalid_block_hash: invalid_block_hash.clone(),
        pk: proposer_public_key.clone(),
        target_activation_epoch,
        initial_rand: system_deploy_util::generate_slash_deploy_random_seed(
            self_id,
            seq_num,
            invalid_block_hash,
        ),
    }
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
    // See docs/casper/theory/slashing/design/06-proposing-and-effect.md for
    // the full rationale.

    // Create SlashDeploy objects
    let mut slashing_deploys = Vec::new();
    for slash_candidate in slash_candidates {
        // Phase 10 (C-5): `.get()` converts the typed Epoch back to the protobuf i64.
        let slash_deploy = build_slash_deploy(
            &slash_candidate.invalid_block_hash,
            &validator_identity.public_key,
            slash_candidate.target_activation_epoch.get(),
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
    rejected_deploy_buffer: Arc<Mutex<KeyValueRejectedDeployBuffer>>,
    failure_msg: &str,
) -> Result<(bool, bool), CasperError> {
    let Some(sig) = extract_deploy_sig_from_refund_failure(failure_msg) else {
        return Ok((false, false));
    };

    // Phase 9 (A-3): deploy_storage is a parking_lot::Mutex (no poison) → `.lock()` yields
    // the guard directly; rejected_deploy_buffer is a std Mutex (map_err the poison).
    let removed_from_deploy_storage = deploy_storage
        .lock()
        .remove_by_sig(&sig)
        .map_err(CasperError::from)?;
    let removed_from_rejected_buffer = rejected_deploy_buffer
        .lock()
        .map_err(|e| CasperError::LockError(e.to_string()))?
        .remove_by_sig(&sig)
        .map_err(CasperError::from)?;

    Ok((removed_from_deploy_storage, removed_from_rejected_buffer))
}

#[cfg(test)]
fn drain_selected_deploys_from_rejected_buffer(
    rejected_deploy_buffer: &Arc<Mutex<KeyValueRejectedDeployBuffer>>,
    deploys: &[Signed<DeployData>],
) -> Result<usize, CasperError> {
    let mut guard = rejected_deploy_buffer
        .lock()
        .map_err(|e| CasperError::LockError(e.to_string()))?;
    let mut removed = 0usize;
    for deploy in deploys {
        if guard
            .remove_by_sig(&deploy.sig)
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
    deploys: &[Signed<DeployData>],
) -> Result<usize, CasperError> {
    let selected_recovered: Vec<Signed<DeployData>> = {
        let guard = rejected_deploy_buffer
            .lock()
            .map_err(|e| CasperError::LockError(e.to_string()))?;
        let mut out = Vec::new();
        for deploy in deploys {
            if guard.contains_sig(&deploy.sig).map_err(CasperError::from)? {
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
            if storage
                .remove_by_sig(&deploy.sig)
                .map_err(CasperError::from)?
            {
                tracing::info!(
                    target: "f1r3fly.casper.deploy_lifecycle",
                    event = "storage_removed",
                    deploy_sig = %hex::encode(&deploy.sig),
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
    deploys: &[Signed<DeployData>],
    recovered_sigs: &HashSet<Bytes>,
) -> Result<usize, CasperError> {
    let recovered_done: Vec<Signed<DeployData>> = deploys
        .iter()
        .filter(|deploy| recovered_sigs.contains(&deploy.sig))
        .cloned()
        .collect();
    if recovered_done.is_empty() {
        return Ok(0);
    }

    deploy_storage
        .remove(recovered_done.clone())
        .map_err(CasperError::from)?;
    Ok(recovered_done.len())
}

fn current_proposal_validators(casper_snapshot: &CasperSnapshot) -> Vec<Validator> {
    let mut validators: Vec<Validator> =
        if !casper_snapshot.on_chain_state.active_validators.is_empty() {
            casper_snapshot.on_chain_state.active_validators.clone()
        } else {
            casper_snapshot
                .on_chain_state
                .bonds_map
                .iter()
                .filter(|(_, stake)| **stake > 0)
                .map(|(validator, _)| validator.clone())
                .collect()
        };
    validators.sort();
    validators
}

fn deploy_inclusion_progress(
    casper_snapshot: &CasperSnapshot,
    block_store: &KeyValueBlockStore,
) -> Result<DeployInclusionProgress, CasperError> {
    let validators = current_proposal_validators(casper_snapshot);
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
            current_proposal_validators(casper_snapshot)
                .first()
                .cloned()
        };
        return Ok(DeployInclusionProgress {
            leader,
            latest_deploy: Some(info),
        });
    }

    if scope_has_unfinalized_user_deploys(casper_snapshot, block_store)? {
        return Ok(DeployInclusionProgress {
            leader: current_proposal_validators(casper_snapshot)
                .first()
                .cloned(),
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
    let stored_deploys = deploy_storage.lock().read_all()?;
    if stored_deploys.is_empty() {
        return Ok(FreshLocalDeployStats::default());
    }
    let buffered_sigs: HashSet<Bytes> = rejected_deploy_buffer
        .lock()
        .map_err(|e| CasperError::LockError(e.to_string()))?
        .read_all()?
        .into_iter()
        .map(|deploy| deploy.sig)
        .collect();
    let earliest_block_number = crate::rust::util::deploy_window::earliest_valid_after(
        block_number,
        casper_snapshot.on_chain_state.shard_conf.deploy_lifespan,
    )?;
    let candidates: HashSet<Signed<DeployData>> = stored_deploys
        .into_iter()
        .filter(|deploy| {
            !buffered_sigs.contains(&deploy.sig)
                && !casper_snapshot.deploys_in_scope.contains(&deploy.sig)
                && not_future_deploy(block_number, &deploy.data)
                && not_expired_deploy(earliest_block_number, &deploy.data)
                && !deploy.data.is_expired_at(current_time_millis)
        })
        .collect();
    if candidates.is_empty() {
        return Ok(FreshLocalDeployStats::default());
    }
    let canonical_scan_floor = candidates
        .iter()
        .map(|d| d.data.valid_after_block_number)
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
        if canonical_won.contains(&deploy.sig) {
            continue;
        }
        count += 1;
        oldest_time = Some(
            oldest_time
                .map(|current: i64| current.min(deploy.data.time_stamp))
                .unwrap_or(deploy.data.time_stamp),
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
    let stored_deploys = deploy_storage.lock().read_all()?;
    if stored_deploys.is_empty() {
        return Ok(InScopeLocalDeployStats::default());
    }
    let buffered_sigs: HashSet<Bytes> = rejected_deploy_buffer
        .lock()
        .map_err(|e| CasperError::LockError(e.to_string()))?
        .read_all()?
        .into_iter()
        .map(|deploy| deploy.sig)
        .collect();
    let earliest_block_number = crate::rust::util::deploy_window::earliest_valid_after(
        block_number,
        casper_snapshot.on_chain_state.shard_conf.deploy_lifespan,
    )?;
    let candidates: HashSet<Signed<DeployData>> = stored_deploys
        .into_iter()
        .filter(|deploy| {
            !buffered_sigs.contains(&deploy.sig)
                && casper_snapshot.deploys_in_scope.contains(&deploy.sig)
                && !casper_snapshot.rejected_in_scope.contains(&deploy.sig)
                && not_future_deploy(block_number, &deploy.data)
                && not_expired_deploy(earliest_block_number, &deploy.data)
                && !deploy.data.is_expired_at(current_time_millis)
        })
        .collect();
    if candidates.is_empty() {
        return Ok(InScopeLocalDeployStats::default());
    }
    let canonical_scan_floor = candidates
        .iter()
        .map(|d| d.data.valid_after_block_number)
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
        if canonical_won.contains(&deploy.sig) {
            continue;
        }
        count += 1;
        stranded_count += 1;
        oldest_time = Some(
            oldest_time
                .map(|current: i64| current.min(deploy.data.time_stamp))
                .unwrap_or(deploy.data.time_stamp),
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
fn rejected_buffer_has_recoverable_deploys(
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
    let candidates: Vec<_> = buffered_deploys
        .iter()
        .filter(|deploy| {
            let rejected_in_scope = casper_snapshot.rejected_in_scope.contains(&deploy.sig);
            let clean_in_scope =
                casper_snapshot.deploys_in_scope.contains(&deploy.sig) && !rejected_in_scope;
            !clean_in_scope
                && not_future_deploy(block_number, &deploy.data)
                && !deploy.data.is_expired_at(current_time_millis)
                && not_expired_deploy(window_bound, &deploy.data)
        })
        .collect();
    if candidates.is_empty() {
        return Ok(false);
    }
    let scan_floor = candidates
        .iter()
        .map(|d| d.data.valid_after_block_number)
        .min()
        .map(|h| h.min(earliest_block_number))
        .unwrap_or(earliest_block_number);
    let canonical_won =
        canonical_won_over_parents(floor_ctx, casper_snapshot, block_store, scan_floor)?;

    Ok(candidates
        .iter()
        .any(|deploy| !canonical_won.contains(&deploy.sig)))
}

fn finality_lag_stats(
    casper_snapshot: &CasperSnapshot,
    block_store: &KeyValueBlockStore,
) -> Result<FinalityLagStats, CasperError> {
    let last_finalized_block = block_store
        .get(&casper_snapshot.last_finalized_block)?
        .map(|block| block.body.state.block_number)
        .unwrap_or(-1);
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
    let (user_deploys, _, _) = {
        let t = std::time::Instant::now();
        let user_deploys_in_scope =
            scope_has_unfinalized_user_deploys(casper_snapshot, block_store)?;
        let storage_deploys_in_scope = storage_has_unresolved_in_scope_deploys(
            casper_snapshot,
            &deploy_storage,
            &rejected_deploy_buffer,
        )?;
        let stale_in_scope_work = user_deploys_in_scope || storage_deploys_in_scope;
        let user_work_in_flight = stale_in_scope_work;
        // Recovery admission is unconditional: the buffer is OWNER-SCOPED
        // (only the sender of a rejected copy's carrier holds its retry) and
        // the retry gate paces every re-proposal on the floor, so there is
        // nothing left for a validator-set leader election to serialize.
        let allow_recovered_deploys = true;
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
        if rejected_buffer_non_empty {
            tracing::info!(
                target: "f1r3fly.casper.deploy_lifecycle",
                event = "recovery_leadership",
                proposer = %hex::encode(&validator_identity.public_key.bytes),
                selected = allow_recovered_deploys,
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
                "Ordinary user deploy selection deferred to deploy-inclusion leader for block #{}; proposing finality support only: scope={}, storage_scope={}",
                next_block_num,
                user_deploys_in_scope,
                storage_deploys_in_scope
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
        // The create-side self-chain retain is deliberately GONE: suppressing
        // re-inclusion by own-chain topology stranded deploys whose only
        // copies lived in own unmerged blocks. The record gates govern
        // re-inclusion now — the in-scope filter suppresses genuine
        // duplicates, and the merge's keep-one dedup reconciles the
        // transient two-copy window on-record.
        let v = prepared.deploys;
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
    let latest_messages: BTreeMap<Validator, BlockHash> = casper_snapshot
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let local_validator: Validator =
        prost::bytes::Bytes::copy_from_slice(&validator_identity.public_key.bytes);
    let merge_pre_info = interpreter_util::compute_parents_post_state(
        block_store,
        parents.clone(),
        casper_snapshot,
        runtime_manager,
        &latest_messages,
        None,
        Some(&rejected_deploy_buffer),
        floor_ctx.as_ref(),
        Some(&local_validator),
    )
    .await?;
    metrics::histogram!(
        BLOCK_CREATOR_COMPUTE_PARENTS_POST_STATE_TIME_METRIC,
        "source" => CASPER_METRICS_SOURCE
    )
    .record(__merge_pre_t.elapsed().as_secs_f64());
    let rejected_slashes = merge_pre_info.rejected_slashes.clone();

    // NEVER EXECUTE A DEPLOY WHOSE EFFECT IS ALREADY IN THE PRE-STATE.
    //
    // Selection ran BEFORE the parents were merged, so it cannot know what
    // the block will actually be built on. Two independent mechanisms return
    // a rejected deploy's work: recovery re-selects the deploy for fresh
    // re-proposal, and a merge reinstates the original copy from scope.
    // Neither can see the other, so both can deliver — and the second
    // execution is not a harmless duplicate. The deploy's cells are keyed by
    // a sig-derived rnd, so both executions write the SAME channel and
    // neither consumes the other's datum: the cell ends up holding two
    // values, the vault read trips the IntegerAdd single-value invariant,
    // and the toxic-deploy quarantine then deletes the deploy from BOTH the
    // pool and the buffer. A double-apply destroys the work rather than
    // duplicating it.
    //
    // The test is the INVARIANT, not a route. `applied_from_scope` alone
    // misses paths: it is empty on the short-circuit shapes
    // (`single_parent`, `descendant_fast_path`, cache hit) where the effect
    // arrives via a parent's post-state instead. The membership walk over
    // the pre-state's recorded lineage — the merge base where one is
    // recorded, the sole parent otherwise — is provenance-independent and
    // therefore complete. `applied_from_scope` is still consulted first:
    // it is exact and needs no I/O, so the walk only runs for what it
    // does not cover.
    //
    // Only the FRESH copy is dropped, never the merge's, so reinstatement
    // stays intact. A false positive costs a round — the deploy stays in
    // storage and in the buffer — so this is delay, never loss.
    let user_deploys: HashSet<Signed<DeployData>> = {
        let base_lineage_root: BlockHash = merge_pre_info
            .merge_base
            .clone()
            .unwrap_or_else(|| parents[0].block_hash.clone());
        let mut kept: HashSet<Signed<DeployData>> = HashSet::with_capacity(user_deploys.len());
        let mut dropped: Vec<String> = Vec::new();
        for deploy in user_deploys {
            let already_applied = merge_pre_info.applied_from_scope.contains(&deploy.sig)
                || crate::rust::finality::deploy_lifecycle::effect_in_state_of(
                    block_store,
                    &base_lineage_root,
                    &deploy.sig,
                    deploy.data.valid_after_block_number,
                )?;
            if already_applied {
                dropped.push(hex::encode(&deploy.sig[..deploy.sig.len().min(8)]));
            } else {
                kept.insert(deploy);
            }
        }
        if !dropped.is_empty() {
            tracing::info!(
                target: "f1r3fly.casper.recovery",
                "Dropped {} selected deploy(s) from block #{}: their effects are already in the \
                 pre-state this block executes against, so re-executing would double-apply; \
                 sigs={:?}",
                dropped.len(),
                next_block_num,
                dropped,
            );
        }
        kept
    };
    let has_user_or_dummy_deploys = !user_deploys.is_empty() || !dummy_deploys.is_empty();

    // Union own slashes with merge-rejected slashes, dedup by
    // `invalid_block_hash`. Own detections take priority — any
    // merge-rejected slash for an equivocator already covered by
    // prepare_slashing_deploys is dropped. `filter_recoverable` also
    // collapses multiple rejected slashes for the same equivocator
    // (e.g., from different original issuers) down to a single entry,
    // then the evidence filter drops stale or no-longer-invalid hashes.
    let own_invalid_block_hashes = slashing_deploys
        .iter()
        .map(|sd| sd.invalid_block_hash.clone());
    let epoch_length = casper_snapshot.on_chain_state.shard_conf.epoch_length;
    let candidate_recovered_rejected_slashes =
        crate::rust::merging::rejected_slash::filter_recoverable(
            rejected_slashes,
            own_invalid_block_hashes,
        );
    let (recovered_target_activation_epoch, recovered_rejected_slashes) =
        if candidate_recovered_rejected_slashes.is_empty() {
            (None, Vec::new())
        } else {
            let recovered_target_activation_epoch =
                crate::rust::slashing_authorization::epoch_for_block_number(
                    next_block_num,
                    epoch_length,
                )
                .map_err(|e| {
                    CasperError::RuntimeError(format!(
                        "Failed to compute current epoch for recovered slash deploy: {:?}",
                        e
                    ))
                })?
                .get();
            let recovered_rejected_slashes =
                crate::rust::merging::rejected_slash::filter_recoverable_with_evidence(
                    candidate_recovered_rejected_slashes,
                    Vec::<BlockHash>::new(),
                    |invalid_block_hash| {
                        let Some(metadata) = casper_snapshot
                            .dag
                            .lookup(invalid_block_hash)
                            .map_err(CasperError::from)?
                        else {
                            return Ok::<bool, CasperError>(false);
                        };
                        if !metadata.invalid {
                            return Ok::<bool, CasperError>(false);
                        }
                        let evidence_epoch =
                            crate::rust::slashing_authorization::epoch_for_block_number(
                                metadata.block_number,
                                epoch_length,
                            )
                            .map_err(|e| {
                                CasperError::from(
                                    crate::rust::slashing_authorization::SlashAuthError::from(e),
                                )
                            })?;
                        Ok::<bool, CasperError>(
                            evidence_epoch.get() == recovered_target_activation_epoch,
                        )
                    },
                )?;
            (
                Some(recovered_target_activation_epoch),
                recovered_rejected_slashes,
            )
        };

    // Check if we have any new work to process.
    // If empty blocks are disabled, skip closeBlock-only proposals to avoid no-op checkpoint cost.
    // If empty blocks are enabled (heartbeat/liveness mode), continue and emit closeBlock.
    // Recovered rejected slashes count as work — without this check, a
    // heartbeat-disabled proposer would silently drop merge-rejected slashes
    // on a wake with no other pending work.
    let has_slashing_deploys = !slashing_deploys.is_empty();
    let has_recovered_rejected_slashes = !recovered_rejected_slashes.is_empty();
    if !has_user_or_dummy_deploys
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
    // Per T-9.8, `target_activation_epoch` must equal the *current* epoch
    // of the block carrying the slash — for recovered slashes the current
    // epoch is the one that will be assigned to the block we are creating,
    // i.e. `epoch_for_block_number(next_block_num, epoch_length)`.
    if let Some(recovered_target_activation_epoch) = recovered_target_activation_epoch {
        for rs in &recovered_rejected_slashes {
            let slash_deploy = build_slash_deploy(
                &rs.invalid_block_hash,
                &validator_identity.public_key,
                recovered_target_activation_epoch,
                next_seq_num,
            );
            tracing::info!(
                "Recovering merge-rejected slash: invalid_block={}, original_issuer={}, target_activation_epoch={}",
                pretty_printer::PrettyPrinter::build_string_bytes(&rs.invalid_block_hash),
                hex::encode(&rs.issuer_public_key.bytes),
                recovered_target_activation_epoch
            );
            system_deploys_converted.push(SystemDeployEnum::Slash(slash_deploy));
        }
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
    // Invalid-blocks map (hash -> sender) for the PoS slash deploys: derived from
    // this block's own slash targets so it is byte-identical at creation and
    // replay (see proto_util::slashed_block_senders). A DAG-derived view is
    // node-view-dependent and makes the slash deploy fail replay (ConsumeFailed).
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

    // Compute checkpoint data
    let checkpoint_started = std::time::Instant::now();
    let ordered_user_deploys = ordered_user_deploys(&user_deploys);
    let original_user_deploys = ordered_user_deploys.len();
    let mut user_deploy_limit = original_user_deploys;
    let mut retry_count = 0usize;
    let checkpoint_data = loop {
        let mut deploys: Vec<Signed<DeployData>> = ordered_user_deploys
            .iter()
            .take(user_deploy_limit)
            .cloned()
            .collect();
        deploys.extend(dummy_deploys.iter().cloned());
        let attempted_user_deploys = user_deploy_limit;
        let attempted_total_deploys = deploys.len();

        match interpreter_util::compute_deploys_checkpoint(
            block_store,
            parents.clone(),
            deploys,
            system_deploys_converted.clone(),
            casper_snapshot,
            runtime_manager,
            block_data.clone(),
            invalid_blocks.clone(),
            Some(&rejected_deploy_buffer),
            floor_ctx.as_ref(),
            Some(&local_validator),
        )
        .await
        {
            Ok(data) => {
                if attempted_user_deploys < original_user_deploys {
                    tracing::warn!(
                        "Checkpoint merge recovered by reducing selected user deploys for block #{}: original_user_deploys={}, included_user_deploys={}, dummy_deploys={}, retries={}",
                        next_block_num,
                        original_user_deploys,
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
        deploys: processed_deploys,
        rejected_deploys,
        system_deploys: processed_system_deploys,
        bonds: new_bonds,
        applied_from_scope,
        merge_base,
    } = checkpoint_data;

    let block_bonds = {
        // The floor requirement is real here even for parentless fixture
        // shapes: a block's bonds are the floor committee, so a snapshot
        // with no derivable floor cannot package bonds.
        let ctx = floor_ctx.as_ref().ok_or_else(|| {
            CasperError::Other(
                "finalized_floor requires a non-empty parent set; genesis pre-state comes from \
                 config"
                    .to_string(),
            )
        })?;
        let committee: Vec<Bond> =
            crate::rust::finality::floor::floor_committee(runtime_manager, &ctx.floor_state)
                .await?;
        if committee.len() != new_bonds.len() {
            tracing::info!(
                target: "f1r3fly.casper.bonds_validation",
                floor_number = ctx.floor.block_number,
                committee = committee.len(),
                post_state_bonds = new_bonds.len(),
                "block bonds field differs from post-state bonds"
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
        justifications.iter().cloned().collect(),
        pre_state_hash,
        post_state_hash,
        processed_deploys,
        rejected_deploys,
        processed_system_deploys,
        block_bonds,
        applied_from_scope,
        merge_base,
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

    for processed in &signed_block.body.deploys {
        tracing::info!(
            target: "f1r3fly.casper.deploy_lifecycle",
            event = "carrier_created",
            deploy_sig = %hex::encode(&processed.deploy.sig),
            block_hash = %hex::encode(&signed_block.block_hash),
            block_number = signed_block.body.state.block_number,
            sender = %hex::encode(&signed_block.sender),
            failed = processed.is_failed,
            parents = ?signed_block.header.parents_hash_list.iter().map(hex::encode).collect::<Vec<_>>(),
            "deploy lifecycle"
        );
    }
    for rejected in &signed_block.body.rejected_deploys {
        tracing::info!(
            target: "f1r3fly.casper.deploy_lifecycle",
            event = "rejection_recorded",
            deploy_sig = %hex::encode(&rejected.sig),
            block_hash = %hex::encode(&signed_block.block_hash),
            block_number = signed_block.body.state.block_number,
            carrier = %hex::encode(&rejected.carrier),
            duplicate = rejected.duplicate,
            "deploy lifecycle"
        );
    }

    let selected_user_deploys_for_buffer_drain: Vec<Signed<DeployData>> = ordered_user_deploys
        .iter()
        .take(user_deploy_limit)
        .cloned()
        .collect();
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
    rejected_deploys: Vec<RejectedDeploy>,
    system_deploys: Vec<ProcessedSystemDeploy>,
    bonds_map: Vec<Bond>,
    applied_from_scope: Vec<Bytes>,
    merge_base: Option<BlockHash>,
    shard_id: String,
    version: i64,
) -> BlockMessage {
    let state = F1r3flyState {
        pre_state_hash,
        post_state_hash,
        bonds: bonds_map,
        block_number: block_data.block_number,
    };

    let body = Body {
        state,
        deploys,
        rejected_deploys,
        system_deploys,
        extra_bytes: Bytes::new(),
        applied_from_scope,
        // None = header-derivable (single-parent, genesis): recorded as
        // empty bytes per the field contract.
        merge_base: merge_base.unwrap_or_default(),
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
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::*;

    fn validator(byte: u8) -> Validator { Bytes::from(vec![byte; models::rust::validator::LENGTH]) }

    fn invalid_block_hash(byte: u8) -> BlockHash { Bytes::from(vec![byte; 32]) }

    fn validator_identity(byte: u8) -> ValidatorIdentity {
        ValidatorIdentity {
            public_key: PublicKey::new(validator(byte)),
            private_key: PrivateKey::from_bytes(&[byte; 32]),
            signature_algorithm: "test".to_string(),
        }
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
            Bytes::new(),
            Bytes::new(),
            deploys,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            None,
            "test".to_string(),
            1,
        );
        block.sender = sender;
        block.block_hash = hash;
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
            vec![ProcessedDeploy::empty(deploy)],
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

        snapshot.rejected_in_scope.insert(user_sig.clone());
        assert!(scope_has_unfinalized_user_deploys(&snapshot, &block_store)
            .expect("treat rejected parent-scope user deploy as unresolved"));
        snapshot.rejected_in_scope.remove(&user_sig);

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

        deploy_storage
            .lock()
            .add(vec![deploy.clone()])
            .expect("seed storage deploy");
        assert!(!storage_has_unresolved_in_scope_deploys(
            &snapshot,
            &deploy_storage,
            &rejected_deploy_buffer
        )
        .expect("stored deploy not yet in scope"));

        snapshot.deploys_in_scope.insert(deploy.sig.clone());
        assert!(storage_has_unresolved_in_scope_deploys(
            &snapshot,
            &deploy_storage,
            &rejected_deploy_buffer
        )
        .expect("stored deploy in scope"));

        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![deploy.clone()])
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
            .remove(vec![deploy.clone()])
            .expect("remove parked deploy");

        snapshot.rejected_in_scope.insert(deploy.sig.clone());
        assert!(storage_has_unresolved_in_scope_deploys(
            &snapshot,
            &deploy_storage,
            &rejected_deploy_buffer
        )
        .expect("stored rejected deploy in scope remains unresolved"));
    }

    #[tokio::test]
    async fn deploy_inclusion_leader_tracks_user_deploy_sender_below_support_block() {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm)
            .await
            .expect("block store");
        let mut snapshot =
            crate::rust::casper::test_helpers::TestCasperWithSnapshot::create_empty_snapshot();
        snapshot.on_chain_state.active_validators = vec![validator(1), validator(2), validator(3)];
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
            vec![ProcessedDeploy::empty(deploy)],
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
        snapshot.on_chain_state.active_validators = vec![validator(1), validator(2), validator(3)];
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
            vec![ProcessedDeploy::empty(deploy.clone())],
        );
        let recycled = test_block(
            recycled_hash.clone(),
            validator(2),
            vec![first_hash.clone()],
            3,
            vec![ProcessedDeploy::empty(deploy)],
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
        snapshot.on_chain_state.active_validators = vec![validator(1), validator(2), validator(3)];
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
            vec![ProcessedDeploy::empty(deploy.clone())],
        );
        let duplicate = test_block(
            duplicate_hash.clone(),
            validator(2),
            vec![lfb_hash.clone()],
            3,
            vec![ProcessedDeploy::empty(deploy)],
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

        let deploy = build_slash_deploy(&invalid_block, &proposer_pk, target_epoch, seq_num);

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
            .add(vec![recovered.clone(), unrelated.clone()])
            .expect("seed rejected buffer");

        let removed = drain_selected_deploys_from_rejected_buffer(&rejected_deploy_buffer, &[
            recovered.clone(),
            never_buffered,
        ])
        .expect("drain selected deploys");

        assert_eq!(removed, 1);
        let guard = rejected_deploy_buffer.lock().expect("rejected buffer lock");
        assert!(!guard
            .contains_sig(&recovered.sig)
            .expect("recovered contains"));
        assert!(guard
            .contains_sig(&unrelated.sig)
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

        deploy_storage
            .lock()
            .add(vec![
                recovered.clone(),
                ordinary.clone(),
                unselected_recovered.clone(),
            ])
            .expect("seed deploy storage");
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![recovered.clone(), unselected_recovered.clone()])
            .expect("seed rejected buffer");

        let removed = drain_selected_recovered_deploys_from_deploy_storage(
            &deploy_storage,
            &rejected_deploy_buffer,
            &[recovered.clone(), ordinary.clone()],
        )
        .expect("drain selected recovered deploys");

        assert_eq!(removed, 1);
        let storage = deploy_storage.lock().read_all().expect("read storage");
        assert!(!storage.iter().any(|deploy| deploy.sig == recovered.sig));
        assert!(storage.iter().any(|deploy| deploy.sig == ordinary.sig));
        assert!(storage
            .iter()
            .any(|deploy| deploy.sig == unselected_recovered.sig));
        let buffer = rejected_deploy_buffer.lock().expect("rejected buffer lock");
        assert!(buffer
            .contains_sig(&recovered.sig)
            .expect("recovered contains"));
        assert!(buffer
            .contains_sig(&unselected_recovered.sig)
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
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 10;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 50;
        let buffered = construct_deploy::basic_deploy_data(71, None, Some("test".to_string()))
            .expect("buffered deploy");
        let ordinary = construct_deploy::basic_deploy_data(72, None, Some("test".to_string()))
            .expect("ordinary deploy");

        deploy_storage
            .lock()
            .add(vec![buffered.clone(), ordinary.clone()])
            .expect("seed deploy storage");
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![buffered.clone()])
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
            .any(|deploy| deploy.sig == ordinary.sig));
        assert!(!prepared
            .deploys
            .iter()
            .any(|deploy| deploy.sig == buffered.sig));
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
            Some(-40),
            Some("test".to_string()),
        )
        .expect("old deploy");
        let future = construct_deploy::source_deploy_now(
            "@future-buffer!(0)".to_string(),
            None,
            Some(20),
            Some("test".to_string()),
        )
        .expect("future deploy");
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![expired, old, future])
            .expect("seed rejected buffer");

        assert!(!rejected_buffer_has_recoverable_deploys(
            &snapshot,
            20,
            now,
            &rejected_deploy_buffer,
            &block_store,
            None
        )
        .expect("check unselectable buffer"));

        let fresh = construct_deploy::source_deploy_now(
            "@fresh-buffer!(0)".to_string(),
            None,
            Some(19),
            Some("test".to_string()),
        )
        .expect("fresh deploy");
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![fresh])
            .expect("seed fresh buffer");

        assert!(rejected_buffer_has_recoverable_deploys(
            &snapshot,
            20,
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
        deploy_storage
            .lock()
            .add(vec![expired.clone()])
            .expect("seed deploy storage");
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![expired.clone()])
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
            .read_all()
            .expect("read storage")
            .iter()
            .any(|deploy| deploy.sig == expired.sig));
        assert!(!rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .contains_sig(&expired.sig)
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

        deploy_storage
            .lock()
            .add(deploys)
            .expect("seed deploy storage");

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
            .any(|deploy| deploy.sig == first.sig));
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

        deploy_storage
            .lock()
            .add(deploys)
            .expect("seed deploy storage");

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

        deploy_storage
            .lock()
            .add(deploys)
            .expect("seed deploy storage");

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
            .map(|deploy| deploy.data.valid_after_block_number)
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
            snapshot.deploys_in_scope.insert(deploy.sig.clone());
        }
        let fresh_sigs: HashSet<Bytes> = fresh.iter().map(|deploy| deploy.sig.clone()).collect();
        deploy_storage
            .lock()
            .add(in_scope.into_iter().chain(fresh.into_iter()).collect())
            .expect("seed deploy storage");

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
            .all(|deploy| fresh_sigs.contains(&deploy.sig)));
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
            snapshot.deploys_in_scope.insert(deploy.sig.clone());
        }
        let lfb_hash = invalid_block_hash(0x91);
        let user_hash = invalid_block_hash(0x92);
        let lfb = test_block(lfb_hash.clone(), validator(1), Vec::new(), 250, Vec::new());
        let user = test_block(
            user_hash,
            validator(2),
            vec![lfb_hash.clone()],
            299,
            deploys
                .iter()
                .cloned()
                .map(ProcessedDeploy::empty)
                .collect(),
        );
        block_store.put_block_message(&lfb).expect("put lfb");
        block_store.put_block_message(&user).expect("put user");
        snapshot.last_finalized_block = lfb_hash;
        snapshot.max_block_num = 299;
        snapshot.parents = vec![lfb];
        deploy_storage
            .lock()
            .add(deploys)
            .expect("seed deploy storage");

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
            .map(|deploy| deploy.data.valid_after_block_number)
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
        snapshot.deploys_in_scope.insert(deploy.sig.clone());
        deploy_storage
            .lock()
            .add(vec![deploy.clone()])
            .expect("seed deploy storage");
        let lfb_hash = invalid_block_hash(0x94);
        let user_hash = invalid_block_hash(0x95);
        let lfb = test_block(lfb_hash.clone(), validator(1), Vec::new(), 250, Vec::new());
        let user = test_block(user_hash, validator(2), vec![lfb_hash.clone()], 299, vec![
            ProcessedDeploy::empty(deploy.clone()),
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
        snapshot.deploys_in_scope.insert(deploy.sig.clone());
        deploy_storage
            .lock()
            .add(vec![deploy.clone()])
            .expect("seed deploy storage");
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
            vec![ProcessedDeploy::empty(deploy.clone())],
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
        assert!(!canonical_won.contains(&deploy.sig));

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
        assert!(prepared.deploys.iter().any(|d| d.sig == deploy.sig));
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
        snapshot.deploys_in_scope.insert(deploy.sig.clone());
        deploy_storage
            .lock()
            .add(vec![deploy.clone()])
            .expect("seed deploy storage");
        // Real geometry: the winning inclusion rides a block the parents
        // descend from (here: the parent itself), so the CANONICAL walk
        // over the parents excludes the sig — no finality marker consulted.
        let lfb_hash = invalid_block_hash(0x93);
        let lfb = test_block(lfb_hash.clone(), validator(1), Vec::new(), 250, vec![
            ProcessedDeploy::empty(deploy.clone()),
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
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 10;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 50;
        let recovered = construct_deploy::basic_deploy_data(81, None, Some("test".to_string()))
            .expect("recovered deploy");
        let ordinary = construct_deploy::basic_deploy_data(82, None, Some("test".to_string()))
            .expect("ordinary deploy");

        deploy_storage
            .lock()
            .add(vec![recovered.clone(), ordinary.clone()])
            .expect("seed deploy storage");
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![recovered.clone()])
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
            .any(|deploy| deploy.sig == ordinary.sig));
        assert!(!prepared
            .deploys
            .iter()
            .any(|deploy| deploy.sig == recovered.sig));

        deploy_storage
            .lock()
            .remove(vec![recovered.clone()])
            .expect("drain recovered deploy storage");
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .remove(vec![recovered.clone()])
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
            .any(|deploy| deploy.sig == ordinary.sig));
    }

    #[test]
    fn recovered_retry_selection_is_stable_across_block_numbers() {
        let deploys: HashSet<_> = (1..=20)
            .map(|id| {
                construct_deploy::basic_deploy_data(id, None, Some("test".to_string()))
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
        let deploys = HashSet::from([deploy]);
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
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 10;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 50;
        let recovered = construct_deploy::basic_deploy_data(91, None, Some("test".to_string()))
            .expect("recovered deploy");

        deploy_storage
            .lock()
            .add(vec![recovered.clone()])
            .expect("seed deploy storage");
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![recovered.clone()])
            .expect("seed rejected buffer");
        snapshot.deploys_in_scope.insert(recovered.sig.clone());

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
        snapshot
            .on_chain_state
            .shard_conf
            .max_user_deploys_per_block = 10;
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 50;
        let recovered = construct_deploy::basic_deploy_data(92, None, Some("test".to_string()))
            .expect("recovered deploy");

        deploy_storage
            .lock()
            .add(vec![recovered.clone()])
            .expect("seed deploy storage");
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![recovered.clone()])
            .expect("seed rejected buffer");
        snapshot.deploys_in_scope.insert(recovered.sig.clone());
        snapshot.rejected_in_scope.insert(recovered.sig.clone());

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
            .contains_sig(&recovered.sig)
            .expect("contains sig"));
    }

    // Inverted contract (issue #197): a block-expired recovered deploy must be
    // excluded and purged, not selected — validation has no recovery carve-out,
    // so selecting it could only wedge the proposer on its own invalid block.
    #[tokio::test]
    async fn recovered_buffered_deploy_is_purged_after_block_expiry() {
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
        snapshot.on_chain_state.shard_conf.deploy_lifespan = 10;
        let recovered = construct_deploy::source_deploy_now(
            "@101!(101)".to_string(),
            None,
            Some(0),
            Some("test".to_string()),
        )
        .expect("recovered deploy");

        deploy_storage
            .lock()
            .add(vec![recovered.clone()])
            .expect("seed deploy storage");
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![recovered.clone()])
            .expect("seed rejected buffer");

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

        assert!(prepared.deploys.is_empty());
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

        deploy_storage
            .add(vec![
                recovered_done.clone(),
                recovered_pending.clone(),
                ordinary_done.clone(),
            ])
            .expect("seed deploy storage");
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![recovered_done.clone(), recovered_pending.clone()])
            .expect("seed rejected buffer");

        let recovered_sigs: HashSet<Bytes> =
            [recovered_done.sig.clone(), recovered_pending.sig.clone()]
                .into_iter()
                .collect();
        let removed = purge_recovered_already_in_scope(
            &mut deploy_storage,
            &[recovered_done.clone(), ordinary_done.clone()],
            &recovered_sigs,
        )
        .expect("purge recovered already in scope");

        assert_eq!(removed, 1);
        let storage_remaining = deploy_storage.read_all().expect("read deploy storage");
        assert!(!storage_remaining
            .iter()
            .any(|deploy| deploy.sig == recovered_done.sig));
        assert!(storage_remaining
            .iter()
            .any(|deploy| deploy.sig == recovered_pending.sig));
        assert!(storage_remaining
            .iter()
            .any(|deploy| deploy.sig == ordinary_done.sig));

        let buffer_guard = rejected_deploy_buffer.lock().expect("rejected buffer lock");
        assert!(buffer_guard
            .contains_sig(&recovered_done.sig)
            .expect("recovered done contains"));
        assert!(buffer_guard
            .contains_sig(&recovered_pending.sig)
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
            .add(vec![deploy.clone()])
            .expect("add recovered deploy");

        let msg = format!(
            "(Bug found) Deploy refund failed: Insufficient funds, deploy_sig={}, deployer_pk=04ffc016, refund_amount=4999911287",
            hex::encode(&deploy.sig)
        );
        let removed = quarantine_refund_failure_deploy(
            deploy_storage.clone(),
            rejected_deploy_buffer.clone(),
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
            .contains_sig(&deploy.sig)
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
            models::rust::casper::protocol::casper_message::ProcessedDeploy::empty(
                buffered.clone(),
            ),
        ]);
        block_store
            .put_block_message(&won_block)
            .expect("store won block");
        snapshot.last_finalized_block = won_block.block_hash.clone();

        deploy_storage
            .lock()
            .add(vec![buffered.clone()])
            .expect("seed deploy storage");
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![buffered.clone()])
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
                .contains_sig(&buffered.sig)
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
        genesis_block.body.rejected_deploys = vec![
            models::rust::casper::protocol::casper_message::RejectedDeploy {
                sig: retry.sig.clone(),
                duplicate: false,
                carrier: genesis_block.block_hash.clone(),
            },
        ];
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
            .insert(&genesis_block, InsertMode::Approved)
            .expect("insert genesis");
        dag_storage
            .insert(&parent_block, InsertMode::Normal)
            .expect("insert parent");
        snapshot.dag = dag_storage.get_representation().expect("dag");
        snapshot.parents = vec![parent_block];

        deploy_storage
            .lock()
            .add(vec![retry.clone()])
            .expect("seed deploy storage");
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![retry.clone()])
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
                .contains_sig(&retry.sig)
                .expect("contains sig"),
            "removal is floor-clock: a floor-live entry must not be deleted \
             on the tip clock"
        );
        assert!(
            prepared.deploys.iter().any(|d| d.sig == retry.sig),
            "retry admission is floor-clock: a tip-expired floor-live \
             rejected deploy stays selectable"
        );
    }

    #[tokio::test]
    async fn retry_waits_until_visible_parent_frontier_is_merged() {
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
            55,
            Vec::new(),
        );
        floor.body.rejected_deploys = vec![
            models::rust::casper::protocol::casper_message::RejectedDeploy {
                sig: retry.sig.clone(),
                duplicate: false,
                carrier: floor.block_hash.clone(),
            },
        ];
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
            .insert(&floor, InsertMode::Approved)
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
        snapshot.parents = vec![left, right];

        deploy_storage
            .lock()
            .add(vec![retry.clone()])
            .expect("seed deploy storage");
        rejected_deploy_buffer
            .lock()
            .expect("rejected buffer lock")
            .add(vec![retry])
            .expect("seed rejected buffer");

        let prepared = prepare_user_deploys(
            &snapshot,
            61,
            10_000,
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
            "merged-frontier retry packaging must defer a retry over visible sibling parents"
        );
    }
}
