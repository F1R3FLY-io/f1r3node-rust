// See casper/src/main/scala/coop/rchain/casper/rholang/RuntimeReplaySyntax.scala

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::time::Instant;

use crypto::rust::public_key::PublicKey;
use models::rhoapi::Par;
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    Event, ProcessedDeploy, ProcessedSystemDeploy, SystemDeployData,
};
use models::rust::validator::Validator;
use rholang::rust::interpreter::accounting::authority::{DemandBound, ResourceMultiset};
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::system_processes::{
    BlockData, DeployData as SystemProcessDeployData,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::merger::merging_logic::{MergeType, NumberChannelsEndVal};
use rspace_plus_plus::rspace::trace::event::Event as RSpaceEvent;

use super::runtime::{RuntimeOps, SysEvalResult};
use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    BLOCK_REPLAY_DEPLOY_CHECK_REPLAY_DATA_TIME_METRIC,
    BLOCK_REPLAY_DEPLOY_DISCARD_EVENT_LOG_TIME_METRIC, BLOCK_REPLAY_DEPLOY_EVALUATE_TIME_METRIC,
    BLOCK_REPLAY_DEPLOY_RIG_TIME_METRIC, BLOCK_REPLAY_PHASE_CREATE_CHECKPOINT_TIME_METRIC,
    BLOCK_REPLAY_PHASE_RESET_TIME_METRIC, BLOCK_REPLAY_PHASE_SYSTEM_DEPLOYS_TIME_METRIC,
    BLOCK_REPLAY_PHASE_USER_DEPLOYS_TIME_METRIC,
    BLOCK_REPLAY_SYSDEPLOY_CHECKPOINT_MERGEABLE_TIME_METRIC,
    BLOCK_REPLAY_SYSDEPLOY_CHECK_TIME_METRIC, BLOCK_REPLAY_SYSDEPLOY_EVAL_TIME_METRIC,
    BLOCK_REPLAY_SYSDEPLOY_RIG_TIME_METRIC, CASPER_METRICS_SOURCE,
};
use crate::rust::util::event_converter;
use crate::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use crate::rust::util::rholang::costacc::redeem_deploy::{
    RedeemDeploy, RedemptionAuthorization, RedemptionOutcome,
};
use crate::rust::util::rholang::costacc::slash_deploy::SlashDeploy;
use crate::rust::util::rholang::replay_failure::ReplayFailure;
use crate::rust::util::rholang::system_deploy::SystemDeployTrait;
use crate::rust::util::rholang::{interpreter_util, system_deploy_util};

pub struct ReplayRuntimeOps {
    pub runtime_ops: RuntimeOps,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayBlockKind {
    Genesis,
    Ordinary,
}

impl ReplayBlockKind {
    fn requires_authority_settlement(self) -> bool { self == Self::Ordinary }
}

pub(crate) fn has_exactly_one_successful_terminal_close(
    system_deploys: &[ProcessedSystemDeploy],
) -> bool {
    let close_positions = system_deploys
        .iter()
        .enumerate()
        .filter_map(|(index, deploy)| {
            matches!(deploy, ProcessedSystemDeploy::Succeeded {
                system_deploy: SystemDeployData::CloseBlockSystemDeployData,
                ..
            })
            .then_some(index)
        })
        .collect::<Vec<_>>();
    matches!(close_positions.as_slice(), [index] if *index + 1 == system_deploys.len())
}

fn certified_replay_capacity(
    allocation: &ResourceMultiset<[u8; 32]>,
    byte_cost_bound: u64,
) -> Result<Cost, CasperError> {
    let authority_capacity = allocation.0.values().try_fold(0_u64, |total, amount| {
        total.checked_add(*amount).ok_or_else(|| {
            CasperError::InvalidCostSettlement(
                "certified authority replay capacity overflows u64".to_string(),
            )
        })
    })?;
    let capacity = authority_capacity
        .checked_add(byte_cost_bound)
        .ok_or_else(|| {
            CasperError::InvalidCostSettlement(
                "certified authority and byte replay capacity overflows u64".to_string(),
            )
        })?;
    let capacity = i64::try_from(capacity).map_err(|_| {
        CasperError::InvalidCostSettlement(
            "certified authority replay capacity exceeds i64".to_string(),
        )
    })?;
    Ok(Cost::create(
        capacity,
        "certified authority replay capacity",
    ))
}

impl ReplayRuntimeOps {
    pub fn new(runtime_ops: RuntimeOps) -> Self { Self { runtime_ops } }

    pub fn new_from_runtime(runtime: RhoRuntimeImpl) -> Self {
        Self {
            runtime_ops: RuntimeOps::new(runtime),
        }
    }

    pub(crate) fn validate_effect_pre_state(
        effect: &str,
        recorded_pre: &StateHash,
        recorded_post: &StateHash,
        current_root: &StateHash,
    ) -> Result<bool, CasperError> {
        match (recorded_pre.is_empty(), recorded_post.is_empty()) {
            (true, true) => Ok(false),
            (false, false) if recorded_pre == current_root => Ok(true),
            (false, false) => Err(CasperError::ReplayFailure(
                ReplayFailure::effect_state_mismatch(
                    effect.to_string(),
                    "pre".to_string(),
                    hex::encode(recorded_pre),
                    hex::encode(current_root),
                ),
            )),
            _ => Err(CasperError::ReplayFailure(
                ReplayFailure::effect_state_mismatch(
                    effect.to_string(),
                    "witness".to_string(),
                    "both pre-state and post-state hashes".to_string(),
                    format!(
                        "pre_present={}, post_present={}",
                        !recorded_pre.is_empty(),
                        !recorded_post.is_empty()
                    ),
                ),
            )),
        }
    }

    pub(crate) fn validate_effect_post_state(
        effect: &str,
        recorded_post: &StateHash,
        actual_post: &StateHash,
    ) -> Result<(), CasperError> {
        if recorded_post == actual_post {
            Ok(())
        } else {
            Err(CasperError::ReplayFailure(
                ReplayFailure::effect_state_mismatch(
                    effect.to_string(),
                    "post".to_string(),
                    hex::encode(recorded_post),
                    hex::encode(actual_post),
                ),
            ))
        }
    }

    pub async fn discard_event_log(&mut self, phase: &str, error_path: bool) {
        let drained = self.runtime_ops.runtime.take_event_log().await;
        if error_path {
            tracing::warn!(
                target: "f1r3fly.casper.replay_rho_runtime",
                "Discarded {} replay events during {} error path",
                drained.len(),
                phase
            );
        }
    }

    /* REPLAY Compute state with deploys (genesis block) and System deploys (regular block) */

    /**
     * Evaluates (and validates) deploys and System deploys with checkpoint to valiate final state hash
     */
    #[tracing::instrument(
        name = "replay-compute-state",
        target = "f1r3fly.casper.replay-rho-runtime",
        skip_all
    )]
    pub async fn replay_compute_state(
        &mut self,
        start_hash: &StateHash,
        terms: Vec<ProcessedDeploy>,
        system_deploys: Vec<ProcessedSystemDeploy>,
        block_data: &BlockData,
        invalid_blocks: Option<HashMap<BlockHash, Validator>>,
        is_genesis: bool,
        runtime_manager: Option<&crate::rust::util::rholang::runtime_manager::RuntimeManager>,
    ) -> Result<(Blake2b256Hash, Vec<NumberChannelsEndVal>), CasperError> {
        let invalid_blocks = invalid_blocks.unwrap_or_default();
        if tracing::enabled!(target: "f1r3fly.casper.invalid_blocks", tracing::Level::DEBUG) {
            let entries: Vec<String> = invalid_blocks
                .iter()
                .map(|(bh, v)| {
                    format!(
                        "{}=>{}",
                        hex::encode(&bh[..8.min(bh.len())]),
                        hex::encode(&v[..8.min(v.len())])
                    )
                })
                .collect();
            tracing::debug!(target: "f1r3fly.casper.invalid_blocks", n = invalid_blocks.len(), seq = block_data.seq_num, "REPLAY compute_state invalid_blocks: [{}]", entries.join(", "));
        }

        self.runtime_ops
            .runtime
            .set_block_data(block_data.clone())
            .await;
        self.runtime_ops
            .runtime
            .set_invalid_blocks(invalid_blocks)
            .await;

        let block_kind = if is_genesis {
            ReplayBlockKind::Genesis
        } else {
            ReplayBlockKind::Ordinary
        };
        self.replay_deploys(
            start_hash,
            terms,
            system_deploys,
            block_kind,
            block_data,
            runtime_manager,
        )
        .await
    }

    /* REPLAY Deploy evaluators */

    /**
     * Evaluates (and validates) deploys on root hash with checkpoint to validate final state hash
     */
    pub async fn replay_deploys(
        &mut self,
        start_hash: &StateHash,
        terms: Vec<ProcessedDeploy>,
        system_deploys: Vec<ProcessedSystemDeploy>,
        block_kind: ReplayBlockKind,
        block_data: &BlockData,
        runtime_manager: Option<&crate::rust::util::rholang::runtime_manager::RuntimeManager>,
    ) -> Result<(Blake2b256Hash, Vec<NumberChannelsEndVal>), CasperError> {
        tracing::debug!(target: "f1r3fly.casper.replay_rho_runtime", start_hash = %hex::encode(&start_hash[..8.min(start_hash.len())]), n_user = terms.len(), n_system = system_deploys.len(), "replay.replay_deploys ENTER (reset to pre-state, then replay deploys vs recorded COMMs)");
        // Time reset phase - Span[F].traceI("reset") from Scala
        let reset_start = Instant::now();
        let start_root = Blake2b256Hash::from_bytes_prost(start_hash);
        self.runtime_ops.runtime.reset(&start_root).await?;
        metrics::histogram!(BLOCK_REPLAY_PHASE_RESET_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(reset_start.elapsed().as_secs_f64());

        let result = async {

        if block_kind.requires_authority_settlement()
            && !has_exactly_one_successful_terminal_close(&system_deploys)
        {
            return Err(CasperError::ReplayFailure(
                ReplayFailure::replay_admission_mismatch(
                    terms.len(),
                    terms.len(),
                    0,
                    0,
                    "ordinary block must contain exactly one successful terminal close deploy"
                        .to_string(),
                ),
            ));
        }
        // ── WD-D2 replay-side acceptance recompute (CONSENSUS-CRITICAL) ──────
        // After the reset (the live store is now at `start_hash`, the block's
        // pre-state) and BEFORE any deploy executes, recompute the certified
        // reservation from `terms` (= the executed subset of `block.body.deploys`) and
        // re-verify admission. The realized debit is derived from each
        // replay-checked `ProcessedDeploy.cost`; the static check asserts that every
        // purse dominates cumulative
        // Δ_s^max (an over-admitting proposer ⇒
        // double-spend, TM-CA-153). RuntimeManager has already re-derived the
        // full executed/rejected partition before this replay begins.
        // Time user deploys phase
        tracing::debug!(target: "f1r3fly.casper.replay_rho_runtime", n_user = terms.len(), "replay.replay_deploys: USER-deploy phase");
        let user_deploys_start = Instant::now();
        // Slice 31 gap fix + H-P7-5 round-2: RAII exemption for the
        // URN filter around genesis replay.  Genesis replay
        // re-executes the FsGenesis ProcessedDeploy which binds
        // `rho:io:fs:native:*` URNs — the play side disables the
        // filter around genesis, and we mirror on replay so the
        // block-approver / validate-checkpoint paths don't fail
        // with `ReplayStatusMismatch`.  Drop guarantees re-enable
        // on every exit path including panic; pre-fix bare toggle
        // could leak the exemption if the async block panicked.
        // Cost-accounted merge: use `block_kind == ReplayBlockKind::Genesis`
        // as the genesis-mode signal (replaces fileio's earlier
        // `!with_cost_accounting` param — that param was removed on
        // the cost-accounted branch; block_kind carries the same signal).
        let _filter_exemption = if block_kind == ReplayBlockKind::Genesis {
            Some(self.runtime_ops.runtime.exempt_fs_native_urn_filter())
        } else {
            None
        };
        let mut deploy_results = Vec::new();
        let mut current_root = start_hash.clone();
        for term in terms {
            let effect = format!("user:{}", hex::encode(&term.deploy.sig));
            let validate_witness = Self::validate_effect_pre_state(
                &effect,
                &term.pre_state_hash,
                &term.post_state_hash,
                &current_root,
            )?;
            let purse_snapshot = if block_kind.requires_authority_settlement() {
                let runtime_manager = runtime_manager.ok_or_else(|| {
                    CasperError::InvalidCostSettlement(
                        "ordinary replay requires a committed-state purse reader".to_string(),
                    )
                })?;
                let reader = crate::rust::util::rholang::acceptance::RuntimeManagerSupplyReader {
                    runtime_manager,
                    pre_state_hash: current_root.clone(),
                };
                Some(
                    crate::rust::util::rholang::acceptance::replay_purse_snapshot(&term, &reader)
                        .await?,
                )
            } else {
                None
            };
            let result = self
                .replay_deploy_e_with_snapshot(block_kind, &term, purse_snapshot.as_ref())
                .await?;
            let checkpoint = self.runtime_ops.runtime.create_checkpoint().await;
            let actual_post = checkpoint.root.to_bytes_prost();
            if validate_witness {
                Self::validate_effect_post_state(&effect, &term.post_state_hash, &actual_post)?;
            }
            current_root = actual_post;
            deploy_results.push(result);
        }
        drop(_filter_exemption); // Explicit drop before subsequent runtime ops.
        metrics::histogram!(BLOCK_REPLAY_PHASE_USER_DEPLOYS_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(user_deploys_start.elapsed().as_secs_f64());

        // Time system deploys phase
        tracing::debug!(target: "f1r3fly.casper.replay_rho_runtime", n_system = system_deploys.len(), "replay.replay_deploys: SYSTEM-deploy phase (closeBlock etc.)");
        let system_deploys_start = Instant::now();
        let mut system_deploy_results = Vec::new();
        for (index, system_deploy) in system_deploys.into_iter().enumerate() {
            let effect = format!("system:{}", index);
            let (recorded_pre, recorded_post) = system_deploy.state_hashes();
            let validate_witness = Self::validate_effect_pre_state(
                &effect,
                recorded_pre,
                recorded_post,
                &current_root,
            )?;
            let result = self
                .replay_block_system_deploy(block_data, &system_deploy)
                .await?;
            let checkpoint = self.runtime_ops.runtime.create_checkpoint().await;
            let actual_post = checkpoint.root.to_bytes_prost();
            if validate_witness {
                Self::validate_effect_post_state(&effect, recorded_post, &actual_post)?;
            }
            current_root = actual_post;
            system_deploy_results.push(result);
        }
        metrics::histogram!(BLOCK_REPLAY_PHASE_SYSTEM_DEPLOYS_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(system_deploys_start.elapsed().as_secs_f64());

        let mut all_mergeable = Vec::new();
        all_mergeable.extend(deploy_results);
        all_mergeable.extend(system_deploy_results);

        // Time create-checkpoint phase - Span[F].traceI("create-checkpoint") from Scala
        let checkpoint_start = Instant::now();
        tracing::debug!(target: "f1r3fly.casper.replay_rho_runtime", "create-checkpoint-started");
        let checkpoint = self.runtime_ops.runtime.create_checkpoint().await;
        tracing::debug!(target: "f1r3fly.casper.replay_rho_runtime", "create-checkpoint-finished");
        metrics::histogram!(BLOCK_REPLAY_PHASE_CREATE_CHECKPOINT_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(checkpoint_start.elapsed().as_secs_f64());

        tracing::debug!(target: "f1r3fly.casper.replay_rho_runtime", computed_root = %hex::encode(&checkpoint.root.bytes()[..8.min(checkpoint.root.bytes().len())]), "replay.replay_deploys DONE (computed final replay root)");
        Ok((checkpoint.root, all_mergeable))
        }
        .await;

        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                self.runtime_ops.runtime.reset(&start_root).await.map_err(
                    |rollback_error| {
                        CasperError::RuntimeError(format!(
                            "replay failed ({error}); restoring the block pre-state failed: {rollback_error}"
                        ))
                    },
                )?;
                Err(error)
            }
        }
    }

    /**
     * REPLAY Evaluates deploy
     */
    pub async fn replay_deploy(
        &mut self,
        block_kind: ReplayBlockKind,
        processed_deploy: &ProcessedDeploy,
    ) -> Option<CasperError> {
        self.replay_deploy_e(block_kind, processed_deploy)
            .await
            .err()
    }

    #[tracing::instrument(
        name = "replay-deploy",
        target = "f1r3fly.casper.replay-rho-runtime",
        skip_all
    )]
    pub async fn replay_deploy_e(
        &mut self,
        block_kind: ReplayBlockKind,
        processed_deploy: &ProcessedDeploy,
    ) -> Result<NumberChannelsEndVal, CasperError> {
        self.replay_deploy_e_with_snapshot(block_kind, processed_deploy, None)
            .await
    }

    pub(crate) async fn replay_deploy_e_with_snapshot(
        &mut self,
        block_kind: ReplayBlockKind,
        processed_deploy: &ProcessedDeploy,
        purse_snapshot: Option<&crate::rust::util::rholang::acceptance::ReplayPurseSnapshot>,
    ) -> Result<NumberChannelsEndVal, CasperError> {
        let fallback = self.runtime_ops.runtime.create_soft_checkpoint().await;
        let result = self
            .replay_deploy_e_with_snapshot_transaction(block_kind, processed_deploy, purse_snapshot)
            .await;
        if result.is_err() {
            self.runtime_ops
                .runtime
                .revert_to_soft_checkpoint(fallback)
                .await;
        }
        result
    }

    async fn replay_deploy_e_with_snapshot_transaction(
        &mut self,
        block_kind: ReplayBlockKind,
        processed_deploy: &ProcessedDeploy,
        purse_snapshot: Option<&crate::rust::util::rholang::acceptance::ReplayPurseSnapshot>,
    ) -> Result<NumberChannelsEndVal, CasperError> {
        let mut mergeable_channels: HashMap<Par, MergeType> = HashMap::new();
        let execution_authority = if block_kind.requires_authority_settlement() {
            let certificate =
                crate::rust::util::rholang::acceptance::authority_certificate_from_proto(
                    processed_deploy
                        .authority_funding_certificate
                        .as_ref()
                        .ok_or_else(|| {
                            CasperError::InvalidCostSettlement(
                                "replay deploy is missing its authority certificate".to_string(),
                            )
                        })?,
                )?;
            let allocation = match certificate.demand {
                DemandBound::Exact(allocation) => allocation,
                DemandBound::FiniteUpperBound { bound, .. } => bound,
                DemandBound::Unprovable(_) => {
                    return Err(CasperError::InvalidCostSettlement(
                        "replay deploy carries an unprovable authority demand".to_string(),
                    ));
                }
            };
            let capacity = certified_replay_capacity(&allocation, certificate.byte_cost_bound)?;
            Some((capacity, allocation))
        } else {
            None
        };

        let dsig = if tracing::enabled!(target: "f1r3fly.casper.replay_rho_runtime", tracing::Level::DEBUG)
        {
            hex::encode(&processed_deploy.deploy.sig[..8.min(processed_deploy.deploy.sig.len())])
        } else {
            String::new()
        };
        tracing::debug!(target: "f1r3fly.casper.replay_rho_runtime", deploy = %dsig, "replay.deploy ENTER (rig recorded COMMs)");
        let rig_start = Instant::now();
        self.rig(processed_deploy).await?;
        metrics::histogram!(BLOCK_REPLAY_DEPLOY_RIG_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(rig_start.elapsed().as_secs_f64());

        // H-2 fix (2026-08-06): wrap the replay-branch deploy in
        // `WalDeployScope` so `journal_read` / `journal_write` /
        // `journal_truncate` appends on the follower drain per-
        // deploy at end-of-scope.  Same WalDeployScope pattern the
        // leader uses; keeps follower's per-runtime WAL clean and
        // enables byte-for-byte leader-vs-follower WAL comparison.
        // See runtime.rs for the full rationale.
        let deploy_scope: rholang::rust::interpreter::io::lock::DeployScope = {
            let h = crypto::rust::hash::blake2b256::Blake2b256::hash(
                processed_deploy.deploy.sig.to_vec(),
            );
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&h);
            arr
        };
        let mut wal_scope = crate::rust::rholang::runtime::WalDeployScope::new_with_lock_sweep(
            self.runtime_ops.runtime.fs_handles.wal.clone(),
            self.runtime_ops.runtime.fs_handles.lock_registry.clone(),
            deploy_scope,
            self.runtime_ops
                .runtime
                .fs_handles
                .current_deploy_scope
                .clone(),
        );

        // Cost-accounted merge: `process_deploy_with_cost_accounting`
        // was renamed to `process_ordinary_deploy` and gained
        // `execution_authority` + `purse_snapshot` args.  Genesis
        // deploys keep the same `process_genesis_deploy` path.
        let eval_successful = if block_kind.requires_authority_settlement() {
            self.process_ordinary_deploy(
                processed_deploy,
                &mut mergeable_channels,
                execution_authority,
                purse_snapshot.ok_or_else(|| {
                    CasperError::InvalidCostSettlement(
                        "ordinary replay is missing its verified purse snapshot".to_string(),
                    )
                })?,
            )
            .await?
        } else {
            self.process_genesis_deploy(processed_deploy, &mut mergeable_channels)
                .await?
        };
        tracing::debug!(target: "f1r3fly.casper.replay_rho_runtime", deploy = %dsig, eval_successful, "replay.deploy eval done");

        let check_start = Instant::now();
        if let Err(e) = self.check_replay_data_with_fix(eval_successful).await {
            tracing::debug!(target: "f1r3fly.casper.replay_rho_runtime", deploy = %dsig, "replay.deploy check_replay_data FAILED -> {}", e);
            // wal_scope's Drop is the discard-drain path — no
            // committing on error keeps the follower's per-runtime
            // WAL clean of this failed deploy's contributions.
            return Err(e.into());
        }
        metrics::histogram!(BLOCK_REPLAY_DEPLOY_CHECK_REPLAY_DATA_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(check_start.elapsed().as_secs_f64());

        // H-2: commit-drain the WAL slice with the leader's frozen
        // event_log so the follower processes byte-identical
        // entry order (H-R3's log-order-derived drain applies to
        // replay too).  Result is intentionally discarded — no
        // downstream consumer on the replay side today; the
        // side effect (releasing entries from the shared WAL) is
        // the H-2 fix's purpose.
        let _replay_slice = wal_scope.take_and_commit(&processed_deploy.deploy_log);

        // Time checkpoint-mergeable operation (matches Scala RuntimeReplaySyntax.scala:L322)
        let checkpoint_mergeable_start = Instant::now();
        let channels_data = self
            .runtime_ops
            .get_number_channels_data(&mergeable_channels)
            .await?;
        metrics::histogram!(BLOCK_REPLAY_SYSDEPLOY_CHECKPOINT_MERGEABLE_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(checkpoint_mergeable_start.elapsed().as_secs_f64());

        Ok(channels_data)
    }

    /// Replay path mirror of [`RuntimeOps::play_ordinary_deploy_cosigned`].
    ///
    /// D3 (DR-9, OD-1/OD-2): the escrow pre-charge/refund replay fan-out is
    /// removed. Replay derives the finite execution capacity from the same
    /// authenticated authority pre-state, reconstructs the complete cosigned
    /// envelope, and rejects exhaustion. It then requires the canonical weighted
    /// RSpace cost, status, event log, post-state root, settlement, and fee carve
    /// to match the state-bound evidence committed by the block.
    async fn process_ordinary_deploy(
        &mut self,
        processed_deploy: &ProcessedDeploy,
        mergeable_channels: &mut HashMap<Par, MergeType>,
        execution_authority: Option<(Cost, ResourceMultiset<[u8; 32]>)>,
        purse_snapshot: &crate::rust::util::rholang::acceptance::ReplayPurseSnapshot,
    ) -> Result<bool, CasperError> {
        if processed_deploy.system_deploy_error.is_some() {
            return Err(CasperError::InvalidCostSettlement(
                "admitted cost-accounted deploy carries a system-deploy error".to_string(),
            ));
        }
        let cosigned = processed_deploy
            .to_cosigned()
            .map_err(CasperError::InvalidCostSettlement)?;
        let certificate = crate::rust::util::rholang::acceptance::authority_certificate_from_proto(
            processed_deploy
                .authority_funding_certificate
                .as_ref()
                .ok_or_else(|| {
                    CasperError::InvalidCostSettlement(
                        "replay deploy is missing its authority certificate".to_string(),
                    )
                })?,
        )?;
        let witness = crate::rust::util::rholang::acceptance::authority_witness_from_proto(
            processed_deploy
                .authority_cost_witness
                .as_ref()
                .ok_or_else(|| {
                    CasperError::InvalidCostSettlement(
                        "replay deploy is missing its authority witness".to_string(),
                    )
                })?,
            false,
        )?;
        if witness.certificate_id != certificate.certificate_id() {
            return Err(CasperError::InvalidCostSettlement(
                "replay authority witness is bound to a different certificate".to_string(),
            ));
        }
        PublicKey::validate_secp256k1_bytes(&certificate.fee_recipient).map_err(|error| {
            CasperError::InvalidCostSettlement(format!(
                "authority certificate fee recipient is invalid: {error}"
            ))
        })?;
        let fee_address =
            rholang::rust::interpreter::util::vault_address::VaultAddress::from_public_key(
                &PublicKey::from_bytes(&certificate.fee_recipient),
            )
            .ok_or_else(|| {
                CasperError::InvalidCostSettlement(
                    "authority certificate fee recipient has no canonical vault".to_string(),
                )
            })?
            .to_base58();
        let fee_event = crate::rust::util::rholang::acceptance::fee_authority_event(&cosigned)?;
        let signatures = crate::rust::util::rholang::acceptance::authority_purse_signatures(
            &cosigned, &witness,
        )?;
        let reserved_resources = certificate
            .allocation
            .checked_add(&certificate.byte_allocation)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?
            .checked_add(&certificate.fee_allocation)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        let mut reserve_allocations = Vec::new();
        for (key, amount) in &reserved_resources.0 {
            let signature = signatures.get(key).ok_or_else(|| {
                CasperError::InvalidCostSettlement(
                    "vault reservation references an unresolved signature".to_string(),
                )
            })?;
            let payer = crate::rust::util::rholang::costacc::vault_payer::vault_payer(signature)
                .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
            reserve_allocations.push(
                crate::rust::util::rholang::costacc::vault_cost_deploy::VaultAllocation::new(
                    payer.address.to_base58(),
                    i64::try_from(*amount).map_err(|_| {
                        CasperError::InvalidCostSettlement(
                            "vault reservation exceeds the platform range".to_string(),
                        )
                    })?,
                )?,
            );
        }
        reserve_allocations.push(
            crate::rust::util::rholang::costacc::vault_cost_deploy::VaultAllocation::new(
                fee_address.clone(),
                crate::rust::util::rholang::costacc::VALIDATOR_HANDLER_COST_PER_DEPLOY,
            )?,
        );

        let mut inventory =
            rholang::rust::interpreter::accounting::authority::AuthorityPhysicalInventory::default(
            );
        let mut purse_stacks = BTreeMap::new();
        for key in signatures.keys() {
            let purse = purse_snapshot.get(key).ok_or_else(|| {
                CasperError::InvalidCostSettlement(
                    "verified replay purse snapshot is missing an authority lane".to_string(),
                )
            })?;
            let balance = purse.balance.unwrap_or(0);
            if balance < 0 {
                return Err(CasperError::InvalidCostSettlement(
                    "authority purse balance cannot be negative".to_string(),
                ));
            }
            if balance > 0 {
                inventory.balances.0.insert(
                    *key,
                    u64::try_from(balance).expect("non-negative authority balance"),
                );
            }
            for stack in &purse.stacks {
                if inventory
                    .stacks
                    .insert(stack.instance_id, stack.stack.cells.clone())
                    .is_some()
                    || purse_stacks
                        .insert(stack.instance_id, stack.clone())
                        .is_some()
                {
                    return Err(CasperError::InvalidCostSettlement(
                        "authority inventory contains a duplicate stack identity".to_string(),
                    ));
                }
            }
        }
        if purse_snapshot.len() != signatures.len() {
            return Err(CasperError::InvalidCostSettlement(
                "verified replay purse snapshot contains unexpected authority lanes".to_string(),
            ));
        }
        let evaluate_start = Instant::now();
        let (eval_result, successful, _user_log) = self
            .run_user_deploy(processed_deploy, mergeable_channels, execution_authority)
            .await?;
        metrics::histogram!(BLOCK_REPLAY_DEPLOY_EVALUATE_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(evaluate_start.elapsed().as_secs_f64());
        let lifecycle_log = processed_deploy
            .deploy_log
            .iter()
            .map(event_converter::to_rspace_event)
            .collect::<Vec<_>>();
        let actual_events = super::runtime::causal_authority_events_from_lifecycle_trace(
            &lifecycle_log,
            &eval_result.authority_events,
        )?;
        if actual_events != witness.events || eval_result.authority_realized != witness.realized {
            return Err(CasperError::InvalidCostSettlement(
                "replay authority trace differs from the committed witness".to_string(),
            ));
        }
        if eval_result.authority_byte_events != witness.byte_events
            || eval_result.quantitative_byte_cost != witness.byte_cost
        {
            return Err(CasperError::InvalidCostSettlement(
                "replay quantitative byte trace differs from the committed witness".to_string(),
            ));
        }
        let actual_born_stacks = self
            .runtime_ops
            .resolve_authority_stack_births(&eval_result.authority_stack_births)
            .await?;
        if actual_born_stacks != witness.born_stacks {
            return Err(CasperError::InvalidCostSettlement(
                "replay authority stack births differ from the committed witness".to_string(),
            ));
        }
        for stack in self
            .runtime_ops
            .resolve_authority_born_purse_stacks(&witness.born_stacks)
            .await?
        {
            let birth = witness
                .born_stacks
                .iter()
                .find(|birth| birth.stack_id == stack.instance_id)
                .ok_or_else(|| {
                    CasperError::InvalidCostSettlement(
                        "replay born stack is missing its witness presentation".to_string(),
                    )
                })?;
            if inventory
                .stacks
                .insert(stack.instance_id, stack.stack.cells.clone())
                .is_some()
                || inventory
                    .born_stacks
                    .insert(stack.instance_id, birth.produce_hash)
                    .is_some()
                || purse_stacks.insert(stack.instance_id, stack).is_some()
            {
                return Err(CasperError::InvalidCostSettlement(
                    "replay born stack collides with reserved inventory".to_string(),
                ));
            }
        }
        let physical_settlement =
            rholang::rust::interpreter::accounting::authority::verify_physical_settlement(
                &witness.events,
                &signatures,
                &inventory,
                &witness.physical_draws,
            )
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        if physical_settlement.balance_debit != witness.settlement
            || !certificate
                .allocation
                .dominates(&physical_settlement.balance_debit)
        {
            return Err(CasperError::InvalidCostSettlement(
                "replay physical settlement differs from its vault reservation".to_string(),
            ));
        }
        let after_cost = inventory
            .balances
            .checked_sub(&physical_settlement.balance_debit)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        let recomputed_byte =
            rholang::rust::interpreter::accounting::authority::allocate_quantitative_events(
                &witness.byte_events,
                &after_cost,
            )
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        if recomputed_byte != witness.byte_settlement
            || recomputed_byte != certificate.byte_allocation
        {
            return Err(CasperError::InvalidCostSettlement(
                "replay quantitative byte allocation differs from its witness or certificate"
                    .to_string(),
            ));
        }
        let after_byte = after_cost
            .checked_sub(&recomputed_byte)
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        let recomputed_fee =
            rholang::rust::interpreter::accounting::authority::allocate_authority_events(
                std::slice::from_ref(&fee_event),
                &after_byte,
            )
            .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
        if recomputed_fee != certificate.fee_allocation {
            return Err(CasperError::InvalidCostSettlement(
                "replay fee allocation differs from its certificate".to_string(),
            ));
        }

        crate::rust::util::rholang::supply::apply_stack_pops(
            &mut self.runtime_ops,
            &purse_stacks.into_values().collect::<Vec<_>>(),
            &physical_settlement.stack_pops,
        )
        .await?;
        self.runtime_ops.runtime.take_event_log().await;

        let mut settlements = Vec::new();
        for (key, reserved_amount) in &reserved_resources.0 {
            let signature = signatures.get(key).ok_or_else(|| {
                CasperError::InvalidCostSettlement(
                    "vault settlement references an unresolved signature".to_string(),
                )
            })?;
            let payer = crate::rust::util::rholang::costacc::vault_payer::vault_payer(signature)
                .map_err(|error| CasperError::InvalidCostSettlement(error.to_string()))?;
            let burn = physical_settlement.balance_debit.get(key);
            let byte_burn = recomputed_byte.get(key);
            let fee = certificate.fee_allocation.get(key);
            let total_burn = burn.checked_add(byte_burn).ok_or_else(|| {
                CasperError::InvalidCostSettlement("replay vault burn overflows u64".to_string())
            })?;
            if total_burn
                .checked_add(fee)
                .is_none_or(|total| total > *reserved_amount)
            {
                return Err(CasperError::InvalidCostSettlement(
                    "replay vault settlement exceeds its reservation".to_string(),
                ));
            }
            settlements.push(
                crate::rust::util::rholang::costacc::vault_cost_deploy::VaultSettlement::new(
                    payer.address.to_base58(),
                    i64::try_from(total_burn).map_err(|_| {
                        CasperError::InvalidCostSettlement(
                            "vault burn exceeds the platform range".to_string(),
                        )
                    })?,
                    i64::try_from(fee).map_err(|_| {
                        CasperError::InvalidCostSettlement(
                            "vault fee exceeds the platform range".to_string(),
                        )
                    })?,
                )?,
            );
        }
        settlements.push(
            crate::rust::util::rholang::costacc::vault_cost_deploy::VaultSettlement::new(
                fee_address.clone(),
                crate::rust::util::rholang::costacc::VALIDATOR_HANDLER_COST_PER_DEPLOY,
                0,
            )?,
        );
        let mut apply =
            crate::rust::util::rholang::costacc::vault_cost_deploy::ApplyCostDeploy::new(
                certificate.reservation_id,
                reserve_allocations,
                settlements,
                fee_address,
                crate::rust::util::rholang::costacc::vault_cost_deploy::lifecycle_random(
                    &certificate.reservation_id,
                    1,
                ),
            )?;
        let (_, mut apply_eval) = self
            .replay_system_deploy_internal(&mut apply, &None)
            .await?;
        mergeable_channels.extend(apply_eval.mergeable.drain());
        self.runtime_ops.runtime.take_event_log().await;

        tracing::debug!(target: "f1r3fly.casper.replay_rho_runtime", "deploy-done");
        Ok(successful)
    }

    async fn process_genesis_deploy(
        &mut self,
        processed_deploy: &ProcessedDeploy,
        mergeable_channels: &mut HashMap<Par, MergeType>,
    ) -> Result<bool, CasperError> {
        self.run_user_deploy(processed_deploy, mergeable_channels, None)
            .await
            .map(|(_, eval_successful, _)| eval_successful)
    }

    pub async fn run_user_deploy(
        &mut self,
        processed_deploy: &ProcessedDeploy,
        mergeable_channels: &mut HashMap<Par, MergeType>,
        execution_authority: Option<(Cost, ResourceMultiset<[u8; 32]>)>,
    ) -> Result<(EvaluateResult, bool, Vec<RSpaceEvent>), CasperError> {
        // Mirror RuntimeOps behavior: rollback a failed user deploy while
        // preserving the block-level authority reservation for settlement.
        let fallback = self.runtime_ops.runtime.create_soft_checkpoint().await;

        let deploy_data = SystemProcessDeployData::from_deploy(&processed_deploy.deploy);
        self.runtime_ops.runtime.set_deploy_data(deploy_data).await;

        let mut user_eval_result = match execution_authority {
            Some((budget, authority_allocation)) => {
                let cosigned = processed_deploy
                    .to_cosigned()
                    .map_err(CasperError::InvalidCostSettlement)?;
                self.runtime_ops
                    .evaluate_cosigned_with_budget_and_authority(
                        &cosigned,
                        budget,
                        Some(authority_allocation),
                    )
                    .await?
            }
            None => {
                self.runtime_ops
                    .evaluate_genesis(&processed_deploy.deploy)
                    .await?
            }
        };
        let discard_start = Instant::now();
        let user_log = self.runtime_ops.runtime.take_event_log().await;
        metrics::histogram!(BLOCK_REPLAY_DEPLOY_DISCARD_EVENT_LOG_TIME_METRIC, "source" => CASPER_METRICS_SOURCE, "phase" => "user-deploy")
            .record(discard_start.elapsed().as_secs_f64());

        let eval_successful = user_eval_result.errors.is_empty();
        if user_eval_result
            .errors
            .iter()
            .any(|error| matches!(error, InterpreterError::OutOfPhlogistonsError))
        {
            return Err(CasperError::ReplayFailure(
                ReplayFailure::replay_admission_mismatch(
                    1,
                    1,
                    0,
                    0,
                    "admitted deploy exhausted its state-bound replay execution capacity"
                        .to_string(),
                ),
            ));
        }

        if !eval_successful {
            interpreter_util::print_deploy_errors(
                &processed_deploy.deploy.sig,
                &user_eval_result.errors,
            );
            self.runtime_ops
                .runtime
                .revert_to_soft_checkpoint(fallback)
                .await;
        } else {
            mergeable_channels.extend(user_eval_result.mergeable.drain());
        }

        // Verify that our execution matches the expected result
        if processed_deploy.is_failed != !eval_successful {
            return Err(CasperError::ReplayFailure(
                ReplayFailure::replay_status_mismatch(processed_deploy.is_failed, !eval_successful),
            ));
        }

        if processed_deploy.cost.cost != user_eval_result.cost.value as u64 {
            return Err(CasperError::ReplayFailure(
                ReplayFailure::replay_cost_mismatch(
                    processed_deploy.cost.cost,
                    user_eval_result.cost.value as u64,
                ),
            ));
        }

        // The per-operation cost-trace digest is intentionally NOT compared
        // in replay: it is diagnostic-only, not a consensus quantity. Consensus
        // cost integrity is the conserved total cost (compared above) plus the
        // failed/OOP status (compared above) plus the post-state hash. See the
        // cost-accounting threat model (TM-CA-151) and the design doc.
        Ok((user_eval_result, eval_successful, user_log))
    }

    /* REPLAY System deploy evaluators */

    /**
     * Evaluates System deploy with checkpoint to get final state hash
     */
    #[tracing::instrument(
        name = "replay-sys-deploy",
        target = "f1r3fly.casper.replay-rho-runtime",
        skip_all
    )]
    pub async fn replay_block_system_deploy(
        &mut self,
        block_data: &BlockData,
        processed_system_deploy: &ProcessedSystemDeploy,
    ) -> Result<NumberChannelsEndVal, CasperError> {
        let system_deploy = match processed_system_deploy {
            ProcessedSystemDeploy::Succeeded {
                ref system_deploy, ..
            } => system_deploy,
            ProcessedSystemDeploy::Failed { .. } => &SystemDeployData::Empty,
        };

        match system_deploy {
            SystemDeployData::Slash {
                invalid_block_hash,
                issuer_public_key,
                target_activation_epoch,
            } => {
                let slash_deploy = SlashDeploy {
                    invalid_block_hash: invalid_block_hash.clone(),
                    pk: issuer_public_key.clone(),
                    target_activation_epoch: *target_activation_epoch,
                    initial_rand: system_deploy_util::generate_slash_deploy_random_seed(
                        block_data.sender.bytes.clone(),
                        block_data.seq_num,
                        invalid_block_hash,
                    ),
                };

                self.rig_system_deploy(processed_system_deploy).await?;
                let mut slash_deploy_mut = slash_deploy.clone();
                let (_, eval_result) = self
                    .replay_system_deploy_internal(&mut slash_deploy_mut, &None)
                    .await?;

                self.discard_event_log("slash-system-deploy", false).await;

                // Time checkpoint-mergeable operation for slash deploy
                let checkpoint_mergeable_start = Instant::now();
                let map = self
                    .runtime_ops
                    .get_number_channels_data(&eval_result.mergeable)
                    .await?;
                metrics::histogram!(BLOCK_REPLAY_SYSDEPLOY_CHECKPOINT_MERGEABLE_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .record(checkpoint_mergeable_start.elapsed().as_secs_f64());

                self.check_replay_data_with_fix(eval_result.errors.is_empty())
                    .await?;

                Ok(map)
            }

            SystemDeployData::CloseBlockSystemDeployData => {
                let close_block_deploy = CloseBlockDeploy::new(
                    system_deploy_util::generate_close_deploy_random_seed_from_validator(
                        block_data.sender.bytes.clone(),
                        block_data.seq_num,
                    ),
                );

                self.rig_system_deploy(processed_system_deploy).await?;

                let mut close_block_deploy_mut = close_block_deploy.clone();
                let (_, eval_result) = self
                    .replay_system_deploy_internal(&mut close_block_deploy_mut, &None)
                    .await?;

                self.discard_event_log("close-block-system-deploy", false)
                    .await;

                // Time checkpoint-mergeable operation for close block deploy
                let checkpoint_mergeable_start = Instant::now();
                let map = self
                    .runtime_ops
                    .get_number_channels_data(&eval_result.mergeable)
                    .await?;
                metrics::histogram!(BLOCK_REPLAY_SYSDEPLOY_CHECKPOINT_MERGEABLE_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .record(checkpoint_mergeable_start.elapsed().as_secs_f64());

                self.check_replay_data_with_fix(eval_result.errors.is_empty())
                    .await?;

                Ok(map)
            }

            SystemDeployData::Redeem {
                validator_pk,
                outcome_tag,
                penalty,
                pos_multi_sig_public_keys,
                pos_multi_sig_quorum,
                authorizations,
            } => {
                // Cost-Accounted Rho Stage-C redemption replay (DR-7/DR-12).
                // Reconstruct the RedeemDeploy from the block-body authorization
                // material and re-run it. The DR-12 multisig-quorum verification
                // (RedeemDeploy::verify_multisig_quorum, invoked from `env()`) is a
                // DETERMINISTIC pure function of these fields, so replay re-derives
                // the SAME `multiSigVerified` verdict as play — and the Rholang
                // state transition replays via `replay_system_deploy_internal`.
                let outcome = match outcome_tag.as_str() {
                    "Vindicated" => RedemptionOutcome::Vindicated,
                    "Guilty" => RedemptionOutcome::Guilty { penalty: *penalty },
                    "Burned" => RedemptionOutcome::Burned,
                    other => {
                        return Err(CasperError::ReplayFailure(ReplayFailure::internal_error(
                            format!("unknown redemption outcome tag on replay: {}", other),
                        )));
                    }
                };
                let mut redeem_deploy = RedeemDeploy::new(
                    validator_pk.to_vec(),
                    outcome,
                    pos_multi_sig_public_keys.clone(),
                    *pos_multi_sig_quorum,
                    block_data.sender.bytes.clone(),
                    block_data.seq_num,
                );
                redeem_deploy.authorizations = authorizations
                    .iter()
                    .map(|a| RedemptionAuthorization {
                        public_key: a.public_key.to_vec(),
                        signature: a.signature.to_vec(),
                    })
                    .collect();

                self.rig_system_deploy(processed_system_deploy).await?;
                let (_, eval_result) = self
                    .replay_system_deploy_internal(&mut redeem_deploy, &None)
                    .await?;

                self.discard_event_log("redeem-system-deploy", false).await;

                let checkpoint_mergeable_start = Instant::now();
                let map = self
                    .runtime_ops
                    .get_number_channels_data(&eval_result.mergeable)
                    .await?;
                metrics::histogram!(BLOCK_REPLAY_SYSDEPLOY_CHECKPOINT_MERGEABLE_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .record(checkpoint_mergeable_start.elapsed().as_secs_f64());

                self.check_replay_data_with_fix(eval_result.errors.is_empty())
                    .await?;
                Ok(map)
            }

            SystemDeployData::Empty => Err(CasperError::ReplayFailure(
                ReplayFailure::internal_error("Expected system deploy".to_string()),
            )),
        }
    }

    #[tracing::instrument(
        name = "replay-system-deploy",
        target = "f1r3fly.casper.replay-rho-runtime",
        skip_all
    )]
    pub async fn replay_system_deploy_internal<S: SystemDeployTrait>(
        &mut self,
        system_deploy: &mut S,
        expected_failure_msg: &Option<String>,
    ) -> Result<SysEvalResult<S>, CasperError> {
        // Time system deploy evaluation
        let eval_start = Instant::now();
        let (result, eval_res) = match self.runtime_ops.eval_system_deploy(system_deploy).await {
            Err(CasperError::SystemRuntimeError(
                crate::rust::util::rholang::system_deploy_user_error::SystemDeployPlatformFailure::ConsumeFailed,
            )) => {
                let detail = self
                    .runtime_ops
                    .runtime
                    .check_replay_data()
                    .await
                    .err()
                    .map(|error| error.to_string())
                    .unwrap_or_else(|| "replay data was fully consumed".to_string());
                return Err(CasperError::ReplayFailure(ReplayFailure::internal_error(
                    format!("system deploy result was not produced; {detail}"),
                )));
            }
            result => result?,
        };
        metrics::histogram!(BLOCK_REPLAY_SYSDEPLOY_EVAL_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(eval_start.elapsed().as_secs_f64());

        // Compare evaluation from play and replay, successful or failed
        match (expected_failure_msg, &result) {
            // Valid replay
            (None, Either::Right(_)) => {
                // Replayed successful execution
                Ok((result, eval_res))
            }
            (Some(expected_error), Either::Left(error)) => {
                let actual_error = &error.error_message;
                if expected_error == actual_error {
                    // Replayed failed execution - error messages match
                    Ok((result, eval_res))
                } else {
                    // Error messages different
                    Err(CasperError::ReplayFailure(
                        ReplayFailure::system_deploy_error_mismatch(
                            expected_error.clone(),
                            actual_error.clone(),
                        ),
                    ))
                }
            }
            // Invalid replay
            (Some(_), Either::Right(_)) => {
                // Error expected, replay successful
                Err(CasperError::ReplayFailure(
                    ReplayFailure::replay_status_mismatch(true, false),
                ))
            }
            (None, Either::Left(_)) => {
                // No error expected, replay failed
                Err(CasperError::ReplayFailure(
                    ReplayFailure::replay_status_mismatch(false, true),
                ))
            }
        }
    }

    /* Helper functions */

    pub async fn rig_with_check<A, F, Fut>(
        &self,
        processed_deploy: &ProcessedDeploy,
        action: F,
    ) -> Result<(A, bool), CasperError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(A, bool), CasperError>>,
    {
        // Rig the events first
        self.rig(processed_deploy).await?;

        // Execute the provided async action
        let action_result = action().await;

        match action_result {
            Ok((value, eval_successful)) => {
                match self.check_replay_data_with_fix(eval_successful).await {
                    Ok(_) => Ok((value, eval_successful)),
                    Err(replay_failure) => Err(CasperError::ReplayFailure(replay_failure)),
                }
            }
            Err(e) => Err(e),
        }
    }

    pub async fn rig_with_check_system_deploy<A, F, Fut>(
        &self,
        processed_system_deploy: &ProcessedSystemDeploy,
        action: F,
    ) -> Result<(A, EvaluateResult), CasperError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(A, EvaluateResult), CasperError>>,
    {
        self.rig_system_deploy(processed_system_deploy).await?;
        let (value, eval_res) = action().await?;
        self.check_replay_data_with_fix(eval_res.errors.is_empty())
            .await?;
        Ok((value, eval_res))
    }

    pub async fn rig(&self, processed_deploy: &ProcessedDeploy) -> Result<(), CasperError> {
        let rig_start = Instant::now();
        self.runtime_ops
            .runtime
            .rig(
                processed_deploy
                    .deploy_log
                    .iter()
                    .map(event_converter::to_rspace_event)
                    .collect(),
            )
            .await?;
        metrics::histogram!(BLOCK_REPLAY_SYSDEPLOY_RIG_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(rig_start.elapsed().as_secs_f64());
        Ok(())
    }

    pub async fn rig_system_deploy(
        &self,
        processed_system_deploy: &ProcessedSystemDeploy,
    ) -> Result<(), CasperError> {
        let event_list = match processed_system_deploy {
            ProcessedSystemDeploy::Succeeded { event_list, .. } => event_list,
            ProcessedSystemDeploy::Failed { event_list, .. } => event_list,
        };

        Ok(self
            .runtime_ops
            .runtime
            .rig(
                event_list
                    .iter()
                    .map(|event: &Event| event_converter::to_rspace_event(event))
                    .collect(),
            )
            .await?)
    }

    pub async fn check_replay_data_with_fix(
        &self,
        // https://f1r3fly.atlassian.net/browse/RCHAIN-3505
        eval_successful: bool,
    ) -> Result<(), ReplayFailure> {
        let check_start = Instant::now();
        let result = match self.runtime_ops.runtime.check_replay_data().await {
            Ok(()) => Ok(()),
            Err(err) => {
                let err_msg = err.to_string();
                if err_msg.contains("unused") && err_msg.contains("COMM") {
                    if !eval_successful {
                        // Suppress UnusedCOMMEvent when eval was not successful
                        Ok(())
                    } else {
                        Err(ReplayFailure::unused_comm_event(err_msg))
                    }
                } else {
                    Err(ReplayFailure::internal_error(format!(
                        "Replay check failed: {}",
                        err
                    )))
                }
            }
        };
        metrics::histogram!(BLOCK_REPLAY_SYSDEPLOY_CHECK_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(check_start.elapsed().as_secs_f64());
        result
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn succeeded(system_deploy: SystemDeployData) -> ProcessedSystemDeploy {
        ProcessedSystemDeploy::Succeeded {
            event_list: Vec::new(),
            system_deploy,
            pre_state_hash: StateHash::new(),
            post_state_hash: StateHash::new(),
        }
    }

    fn failed() -> ProcessedSystemDeploy {
        ProcessedSystemDeploy::Failed {
            event_list: Vec::new(),
            error_msg: "failed".to_string(),
            pre_state_hash: StateHash::new(),
            post_state_hash: StateHash::new(),
        }
    }

    #[test]
    fn ordinary_replay_requires_one_successful_terminal_close() {
        let close = || succeeded(SystemDeployData::CloseBlockSystemDeployData);
        let other = || succeeded(SystemDeployData::Empty);

        assert!(has_exactly_one_successful_terminal_close(&[close()]));
        assert!(has_exactly_one_successful_terminal_close(&[
            other(),
            close()
        ]));
        assert!(!has_exactly_one_successful_terminal_close(&[]));
        assert!(!has_exactly_one_successful_terminal_close(&[failed()]));
        assert!(!has_exactly_one_successful_terminal_close(&[
            close(),
            other()
        ]));
        assert!(!has_exactly_one_successful_terminal_close(&[
            close(),
            close()
        ]));
    }

    #[test]
    fn effect_state_witness_requires_complete_contiguous_boundaries() {
        let empty = StateHash::new();
        let pre = StateHash::from(vec![1; 32]);
        let post = StateHash::from(vec![2; 32]);

        assert!(
            !ReplayRuntimeOps::validate_effect_pre_state("legacy", &empty, &empty, &pre).unwrap()
        );
        assert!(ReplayRuntimeOps::validate_effect_pre_state("exact", &pre, &post, &pre).unwrap());
        assert!(
            ReplayRuntimeOps::validate_effect_pre_state("partial", &pre, &empty, &pre).is_err()
        );
        assert!(ReplayRuntimeOps::validate_effect_pre_state("gap", &post, &pre, &pre).is_err());
        assert!(ReplayRuntimeOps::validate_effect_post_state("exact", &post, &post).is_ok());
        assert!(ReplayRuntimeOps::validate_effect_post_state("forged", &pre, &post).is_err());
    }

    #[test]
    fn replay_capacity_is_the_checked_sum_of_certified_authority_and_bytes() {
        let allocation = ResourceMultiset(BTreeMap::from([([1; 32], 2), ([2; 32], 3)]));

        assert_eq!(certified_replay_capacity(&allocation, 7).unwrap().value, 12);
        assert_eq!(
            certified_replay_capacity(&ResourceMultiset::default(), 0)
                .unwrap()
                .value,
            0
        );
    }

    #[test]
    fn replay_capacity_rejects_overflow() {
        let allocation = ResourceMultiset(BTreeMap::from([([1; 32], u64::MAX), ([2; 32], 1)]));

        assert!(certified_replay_capacity(&allocation, 0).is_err());
        assert!(certified_replay_capacity(
            &ResourceMultiset(BTreeMap::from([([1; 32], 1)])),
            u64::MAX
        )
        .is_err());
    }
}
