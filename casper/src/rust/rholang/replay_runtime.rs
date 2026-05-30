// See casper/src/main/scala/coop/rchain/casper/rholang/RuntimeReplaySyntax.scala

use std::collections::HashMap;
use std::future::Future;
use std::time::Instant;

use models::rhoapi::Par;
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    Event, ProcessedDeploy, ProcessedSystemDeploy, SystemDeployData,
};
use models::rust::validator::Validator;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::system_processes::{
    BlockData, DeployData as SystemProcessDeployData,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::merger::merging_logic::{MergeType, NumberChannelsEndVal};

use super::runtime::{RuntimeOps, SysEvalResult};
use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    BLOCK_REPLAY_DEPLOY_CHECK_REPLAY_DATA_TIME_METRIC,
    BLOCK_REPLAY_DEPLOY_DISCARD_EVENT_LOG_TIME_METRIC, BLOCK_REPLAY_DEPLOY_EVALUATE_TIME_METRIC,
    BLOCK_REPLAY_DEPLOY_PRECHARGE_TIME_METRIC, BLOCK_REPLAY_DEPLOY_REFUND_TIME_METRIC,
    BLOCK_REPLAY_DEPLOY_RIG_TIME_METRIC, BLOCK_REPLAY_PHASE_CREATE_CHECKPOINT_TIME_METRIC,
    BLOCK_REPLAY_PHASE_RESET_TIME_METRIC, BLOCK_REPLAY_PHASE_SYSTEM_DEPLOYS_TIME_METRIC,
    BLOCK_REPLAY_PHASE_USER_DEPLOYS_TIME_METRIC,
    BLOCK_REPLAY_SYSDEPLOY_CHECKPOINT_MERGEABLE_TIME_METRIC,
    BLOCK_REPLAY_SYSDEPLOY_CHECK_TIME_METRIC, BLOCK_REPLAY_SYSDEPLOY_EVAL_TIME_METRIC,
    BLOCK_REPLAY_SYSDEPLOY_RIG_TIME_METRIC, CASPER_METRICS_SOURCE,
};
use crate::rust::util::event_converter;
use crate::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use crate::rust::util::rholang::costacc::pre_charge_deploy::PreChargeDeploy;
use crate::rust::util::rholang::costacc::refund_deploy::RefundDeploy;
use crate::rust::util::rholang::costacc::slash_deploy::SlashDeploy;
use crate::rust::util::rholang::replay_failure::ReplayFailure;
use crate::rust::util::rholang::system_deploy::SystemDeployTrait;
use crate::rust::util::rholang::{interpreter_util, system_deploy_util};

pub struct ReplayRuntimeOps {
    pub runtime_ops: RuntimeOps,
}

impl ReplayRuntimeOps {
    pub fn new(runtime_ops: RuntimeOps) -> Self {
        Self { runtime_ops }
    }

    pub fn new_from_runtime(runtime: RhoRuntimeImpl) -> Self {
        Self {
            runtime_ops: RuntimeOps::new(runtime),
        }
    }

    pub async fn discard_event_log(&mut self, phase: &str, error_path: bool) {
        let drained = self.runtime_ops.runtime.take_event_log().await;
        if error_path {
            tracing::warn!(
                target: "f1r3fly.casper.replay-rho-runtime",
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
        is_genesis: bool, //FIXME have a better way of knowing this. Pass the replayDeploy function maybe? - OLD
    ) -> Result<(Blake2b256Hash, Vec<NumberChannelsEndVal>), CasperError> {
        let invalid_blocks = invalid_blocks.unwrap_or_default();

        self.runtime_ops
            .runtime
            .set_block_data(block_data.clone())
            .await;
        self.runtime_ops
            .runtime
            .set_invalid_blocks(invalid_blocks)
            .await;

        self.replay_deploys(start_hash, terms, system_deploys, !is_genesis, block_data)
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
        with_cost_accounting: bool,
        block_data: &BlockData,
    ) -> Result<(Blake2b256Hash, Vec<NumberChannelsEndVal>), CasperError> {
        // Time reset phase - Span[F].traceI("reset") from Scala
        let reset_start = Instant::now();
        self.runtime_ops
            .runtime
            .reset(&Blake2b256Hash::from_bytes_prost(start_hash))
            .await?;
        metrics::histogram!(BLOCK_REPLAY_PHASE_RESET_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(reset_start.elapsed().as_secs_f64());

        // ── WD-D2 replay-side acceptance recompute (CONSENSUS-CRITICAL) ──────
        // After the reset (the live store is now at `start_hash`, the block's
        // pre-state) and BEFORE any deploy executes, recompute the settlement-
        // debit map from `terms` (= `block.body.deploys`, the ADMITTED set) and
        // re-verify admission. The debit map is fed to `post_eval_replay` so the
        // close-block settlement debit is byte-identical to the play side; the
        // re-verification asserts that for every PRESENT pool the admitted
        // cumulative Δ_s does not exceed Σ_s (an over-admitting proposer ⇒
        // double-spend, TM-CA-153). The reject direction is intentionally NOT
        // re-derived here (rejected-deploy bodies are not in the block, only
        // sigs) — a wrongly-rejected fundable deploy changes the admitted set and
        // is caught by the post-state root check (wd-d2 §D2.4(b)).
        let replay_debits = if with_cost_accounting {
            self.recompute_and_verify_admission(&terms).await?
        } else {
            // Genesis / non-cost-accounted replay: no acceptance gate ran on the
            // play side, so there is nothing to recompute or debit.
            std::collections::BTreeMap::new()
        };

        // Time user deploys phase
        let user_deploys_start = Instant::now();
        let mut deploy_results = Vec::new();
        for term in terms {
            let result = self.replay_deploy_e(with_cost_accounting, &term).await?;
            deploy_results.push(result);
        }
        metrics::histogram!(BLOCK_REPLAY_PHASE_USER_DEPLOYS_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(user_deploys_start.elapsed().as_secs_f64());

        // Time system deploys phase
        let system_deploys_start = Instant::now();
        let mut system_deploy_results = Vec::new();
        for system_deploy in system_deploys {
            let result = self
                .replay_block_system_deploy(block_data, &system_deploy, &replay_debits)
                .await?;
            system_deploy_results.push(result);
        }
        metrics::histogram!(BLOCK_REPLAY_PHASE_SYSTEM_DEPLOYS_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(system_deploys_start.elapsed().as_secs_f64());

        let mut all_mergeable = Vec::new();
        all_mergeable.extend(deploy_results);
        all_mergeable.extend(system_deploy_results);

        // Time create-checkpoint phase - Span[F].traceI("create-checkpoint") from Scala
        let checkpoint_start = Instant::now();
        tracing::debug!(target: "f1r3fly.casper.replay-rho-runtime", "create-checkpoint-started");
        let checkpoint = self.runtime_ops.runtime.create_checkpoint().await;
        tracing::debug!(target: "f1r3fly.casper.replay-rho-runtime", "create-checkpoint-finished");
        metrics::histogram!(BLOCK_REPLAY_PHASE_CREATE_CHECKPOINT_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(checkpoint_start.elapsed().as_secs_f64());

        Ok((checkpoint.root, all_mergeable))
    }

    /// WD-D2 replay-side acceptance recompute + verification (CONSENSUS-CRITICAL).
    ///
    /// Reconstructs the ADMITTED deploy envelopes from `terms`
    /// (= `block.body.deploys`) via the SAME `ProcessedDeploy::to_cosigned` the
    /// runtime install uses, recomputes the per-pool settlement-debit map from
    /// the live store (already `reset` to the block's pre-state) via
    /// [`acceptance::recompute_settlement_debits`], and re-verifies admission:
    /// for every PRESENT pool, the admitted cumulative `Σ Δ_s` (= the recomputed
    /// debit) MUST be `≤ Σ_s` (the pre-state balance). A proposer that admitted
    /// more than a pool funds (a double-spend / oversubscription, TM-CA-153)
    /// fails this check with a [`ReplayFailure::ReplayAdmissionMismatch`] — the
    /// replay-side counterpart of the play-side in-pass residual.
    ///
    /// Returns the recomputed debit map, fed to `post_eval_replay` so the
    /// close-block settlement debit is byte-identical to the play side.
    async fn recompute_and_verify_admission(
        &self,
        terms: &[ProcessedDeploy],
    ) -> Result<
        std::collections::BTreeMap<
            crate::rust::util::rholang::acceptance::SigKey,
            crate::rust::util::rholang::acceptance::SettlementDebit,
        >,
        CasperError,
    > {
        // Reconstruct the admitted envelopes (canonical pk-ascending per signer,
        // identical to the play-side reconstruction and the runtime install).
        let mut admitted = Vec::with_capacity(terms.len());
        for term in terms {
            admitted.push(term.to_cosigned().map_err(CasperError::RuntimeError)?);
        }

        let reader = crate::rust::util::rholang::acceptance::RuntimeOpsSupplyReader {
            runtime_ops: &self.runtime_ops,
        };
        let debits =
            crate::rust::util::rholang::acceptance::recompute_settlement_debits(admitted, &reader)
                .await?;

        // Re-verify admission: each PRESENT pool's admitted ΣΔ_s ≤ Σ_s. The
        // recompute already restricted `debits` to present pools, so reading the
        // present balance here and comparing catches an over-admitting proposer
        // before the close-block settlement debit's `checked_sub` would (clearer
        // diagnostic + fails the block at the gate boundary, not deep in
        // closeBlock).
        for (sig_key, debit) in &debits {
            let balance = crate::rust::util::rholang::supply::read_balance(
                &self.runtime_ops,
                &debit.channel,
            )
            .await;
            if debit.amount > balance {
                return Err(CasperError::ReplayFailure(
                    ReplayFailure::replay_admission_mismatch(
                        terms.len(),
                        terms.len(),
                        0,
                        0,
                        format!(
                            "admitted ΣΔ_s={} exceeds Σ_s={} for pool {} — proposer over-admitted \
                             (double-spend / oversubscription)",
                            debit.amount,
                            balance,
                            hex::encode(sig_key)
                        ),
                    ),
                ));
            }
        }

        Ok(debits)
    }

    /**
     * REPLAY Evaluates deploy
     */
    pub async fn replay_deploy(
        &mut self,
        with_cost_accounting: bool,
        processed_deploy: &ProcessedDeploy,
    ) -> Option<CasperError> {
        self.replay_deploy_e(with_cost_accounting, processed_deploy)
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
        with_cost_accounting: bool,
        processed_deploy: &ProcessedDeploy,
    ) -> Result<NumberChannelsEndVal, CasperError> {
        let mut mergeable_channels: HashMap<Par, MergeType> = HashMap::new();

        let rig_start = Instant::now();
        self.rig(processed_deploy).await?;
        metrics::histogram!(BLOCK_REPLAY_DEPLOY_RIG_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(rig_start.elapsed().as_secs_f64());

        let eval_successful = if with_cost_accounting {
            self.process_deploy_with_cost_accounting(processed_deploy, &mut mergeable_channels)
                .await?
        } else {
            self.process_deploy_without_cost_accounting(processed_deploy, &mut mergeable_channels)
                .await?
        };

        let check_start = Instant::now();
        self.check_replay_data_with_fix(eval_successful).await?;
        metrics::histogram!(BLOCK_REPLAY_DEPLOY_CHECK_REPLAY_DATA_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(check_start.elapsed().as_secs_f64());

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

    /// Replay path mirror of [`RuntimeOps::play_deploy_with_cost_accounting_cosigned`].
    ///
    /// Reconstructs the [`Cosigned<DeployData>`] envelope from the on-disk
    /// `ProcessedDeploy.deploy: Signed<DeployData>` shape via
    /// `Cosigned::from_single_signer`. For legacy single-sig deploys (the
    /// only on-disk shape today) the loop runs exactly once with the
    /// primary signer, producing byte-identical replay traces to the
    /// pre-multi-sig implementation. When §1.9 extends `ProcessedDeploy`
    /// to carry the cosigner list, multi-sig replay activates automatically
    /// — the canonical pk-ascending iteration order + per-signer-index
    /// seed derivation match the play path exactly.
    ///
    /// Each per-cosigner pre-charge / refund replays through
    /// `replay_system_deploy_internal` with the SAME `rand` derivation as
    /// the play path. Canonical signer order ensures both paths iterate
    /// cosigners in identical order on play and replay.
    async fn process_deploy_with_cost_accounting(
        &mut self,
        processed_deploy: &ProcessedDeploy,
        mergeable_channels: &mut HashMap<Par, MergeType>,
    ) -> Result<bool, CasperError> {
        // Reconstitute the Cosigned<DeployData> envelope from the on-disk
        // ProcessedDeploy shape. For legacy single-sig deploys
        // (`cosigners.is_empty() && primary_phlo_share == 0`), this uplifts
        // via Cosigned::from_single_signer for byte-identical replay. For
        // multi-sig deploys (per §1.9.5 ProcessedDeploy extension), the full
        // canonical cosigner envelope is reconstructed with per-signer
        // re-verification — enabling end-to-end multi-sig replay.
        let cosigned = processed_deploy
            .to_cosigned()
            .map_err(CasperError::RuntimeError)?;
        let is_compound = cosigned.is_compound();
        let phlo_price = cosigned.data.phlo_price;

        // MIRROR of the play path: per-deploy-group id scoping the PoS
        // charge-tracking channel. Derived from the SAME reconstructed
        // Cosigned envelope, so play and replay use identical group channels.
        let dgid = system_deploy_util::deploy_group_id(&cosigned);

        // (B) Pre-charge replay fan-out — canonical pk-ascending order.
        tracing::debug!(target: "f1r3fly.casper.replay-rho-runtime", "precharge-started");
        let precharge_start = Instant::now();
        for (i, signer) in cosigned.signers().iter().enumerate() {
            let charge = signer.phlo_share.saturating_mul(phlo_price);
            let rand = if is_compound {
                system_deploy_util::generate_pre_charge_deploy_random_seed_for_signer(&cosigned, i)
            } else {
                // Legacy single-sig: byte-identical seed to existing on-chain deploys.
                system_deploy_util::generate_pre_charge_deploy_random_seed(&processed_deploy.deploy)
            };
            let mut pre_charge_deploy = PreChargeDeploy {
                charge_amount: charge,
                pk: signer.pk.clone(),
                rand,
                deploy_group_id: dgid.clone(),
                is_first: i == 0,
            };
            let precharge_result = self
                .replay_system_deploy_internal(
                    &mut pre_charge_deploy,
                    // Only the FIRST cosigner's pre-charge sees the
                    // `system_deploy_error` (legacy single-sig had one
                    // pre-charge with this contract). For multi-sig, later
                    // cosigners replay against `None` because the play path
                    // already short-circuited on any earlier failure via
                    // `revert_to_soft_checkpoint` + InsufficientPhloByCosigner.
                    if i == 0 {
                        &processed_deploy.system_deploy_error
                    } else {
                        &None
                    },
                )
                .await;
            match precharge_result {
                Ok((_, mut system_eval_result)) => {
                    let discard_start = Instant::now();
                    self.discard_event_log("precharge", false).await;
                    metrics::histogram!(BLOCK_REPLAY_DEPLOY_DISCARD_EVENT_LOG_TIME_METRIC, "source" => CASPER_METRICS_SOURCE, "phase" => "precharge")
                        .record(discard_start.elapsed().as_secs_f64());
                    if system_eval_result.errors.is_empty() {
                        mergeable_channels.extend(system_eval_result.mergeable.drain());
                    }
                    tracing::debug!(target: "f1r3fly.casper.replay-rho-runtime",
                        "precharge-done cosigner_index={}", i);
                }
                Err(err) => {
                    self.discard_event_log("precharge", true).await;
                    return Err(err);
                }
            };
        }
        metrics::histogram!(BLOCK_REPLAY_DEPLOY_PRECHARGE_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(precharge_start.elapsed().as_secs_f64());

        let eval_successful = if processed_deploy.system_deploy_error.is_none() {
            // Run the user deploy in a transaction
            let evaluate_start = Instant::now();
            let (_, successful) = self
                .run_user_deploy(processed_deploy, mergeable_channels)
                .await?;
            metrics::histogram!(BLOCK_REPLAY_DEPLOY_EVALUATE_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                .record(evaluate_start.elapsed().as_secs_f64());
            tracing::debug!(target: "f1r3fly.casper.replay-rho-runtime", "deploy-eval-done");

            // (D) Refund replay fan-out — FIFO drain matching play path.
            tracing::debug!(target: "f1r3fly.casper.replay-rho-runtime", "refund-started");
            let refund_start = Instant::now();
            let total_refund = processed_deploy
                .try_refund_amount()
                .map_err(CasperError::RuntimeError)?;
            let total_charge = cosigned.total_phlo_share().saturating_mul(phlo_price);
            let total_used = total_charge.saturating_sub(total_refund);
            let mut remaining_used = total_used;
            for (i, signer) in cosigned.signers().iter().enumerate() {
                let signer_charged = signer.phlo_share.saturating_mul(phlo_price);
                let signer_consumed = signer_charged.min(remaining_used);
                remaining_used -= signer_consumed;
                let refund_amount = signer_charged - signer_consumed;
                let rand = if is_compound {
                    system_deploy_util::generate_refund_deploy_random_seed_for_signer(&cosigned, i)
                } else {
                    system_deploy_util::generate_refund_deploy_random_seed(&processed_deploy.deploy)
                };
                let mut refund_deploy = RefundDeploy {
                    refund_amount,
                    pk: signer.pk.clone(),
                    rand,
                    deploy_group_id: dgid.clone(),
                };
                let refund_result = self
                    .replay_system_deploy_internal(&mut refund_deploy, &None)
                    .await;
                match refund_result {
                    Ok((_, mut system_eval_result)) => {
                        let discard_start = Instant::now();
                        self.discard_event_log("refund", false).await;
                        metrics::histogram!(BLOCK_REPLAY_DEPLOY_DISCARD_EVENT_LOG_TIME_METRIC, "source" => CASPER_METRICS_SOURCE, "phase" => "refund")
                            .record(discard_start.elapsed().as_secs_f64());
                        if system_eval_result.errors.is_empty() {
                            mergeable_channels.extend(system_eval_result.mergeable.drain());
                        }
                        tracing::debug!(target: "f1r3fly.casper.replay-rho-runtime",
                            "refund-done cosigner_index={}", i);
                    }
                    Err(err) => {
                        self.discard_event_log("refund", true).await;
                        return Err(err);
                    }
                }
            }
            debug_assert_eq!(
                remaining_used, 0,
                "FIFO drain incomplete on replay: remaining_used={} after fan-out; \
                 total_used={} > Σ(phlo_share × phlo_price)={} — multi-payer \
                 accounting bug on replay path",
                remaining_used, total_used, total_charge
            );
            metrics::histogram!(BLOCK_REPLAY_DEPLOY_REFUND_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                .record(refund_start.elapsed().as_secs_f64());

            successful
        } else {
            // If there was an expected failure in the system deploy, skip user deploy execution
            true
        };

        tracing::debug!(target: "f1r3fly.casper.replay-rho-runtime", "deploy-done");
        Ok(eval_successful)
    }

    async fn process_deploy_without_cost_accounting(
        &mut self,
        processed_deploy: &ProcessedDeploy,
        mergeable_channels: &mut HashMap<Par, MergeType>,
    ) -> Result<bool, CasperError> {
        self.run_user_deploy(processed_deploy, mergeable_channels)
            .await
            .map(|(_, eval_successful)| eval_successful)
    }

    pub async fn run_user_deploy(
        &mut self,
        processed_deploy: &ProcessedDeploy,
        mergeable_channels: &mut HashMap<Par, MergeType>,
    ) -> Result<(EvaluateResult, bool), CasperError> {
        // Mirror RuntimeOps behavior: rollback failed user deploy via soft checkpoint
        // so pre-charge context remains available for refund replay.
        let fallback = self.runtime_ops.runtime.create_soft_checkpoint().await;

        let deploy_data = SystemProcessDeployData::from_deploy(&processed_deploy.deploy);
        self.runtime_ops.runtime.set_deploy_data(deploy_data).await;

        let mut user_eval_result = self.runtime_ops.evaluate(&processed_deploy.deploy).await?;
        let discard_start = Instant::now();
        self.discard_event_log("user-deploy", false).await;
        metrics::histogram!(BLOCK_REPLAY_DEPLOY_DISCARD_EVENT_LOG_TIME_METRIC, "source" => CASPER_METRICS_SOURCE, "phase" => "user-deploy")
            .record(discard_start.elapsed().as_secs_f64());

        let eval_successful = user_eval_result.errors.is_empty();

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
        Ok((user_eval_result, eval_successful))
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
        settlement_debits: &std::collections::BTreeMap<
            crate::rust::util::rholang::acceptance::SigKey,
            crate::rust::util::rholang::acceptance::SettlementDebit,
        >,
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
                let mut slash_deploy = SlashDeploy {
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
                let (_, eval_result) = self
                    .replay_system_deploy_internal(&mut slash_deploy, &None)
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
                let close_block_deploy = CloseBlockDeploy {
                    initial_rand:
                        system_deploy_util::generate_close_deploy_random_seed_from_validator(
                            block_data.sender.bytes.clone(),
                            block_data.seq_num,
                        ),
                    // Replay does NOT thread the play-side debit map (debits are
                    // not serialized into the block); the settlement debit is
                    // applied on replay via the RECOMPUTED map fed directly to
                    // `post_eval_replay` (WD-D2, replay symmetry). The struct
                    // field is left empty here.
                    settlement_debits: Default::default(),
                };

                // Capture the pre-close store root for the Stage-B supply
                // dual-write context (diagnostics / mismatch reporting); the
                // supply read/write themselves target the live store.
                let pre_state_hash: StateHash =
                    self.runtime_ops.runtime.get_root().await.to_bytes_prost();

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

                // Cost-Accounted Rho Stage B (Decision 2.5/6.3) + WD-D2: mirror
                // the closeBlock-published epoch/genesis-block-1 mint into `Σ⟦v⟧`
                // AND apply the WD-D2 settlement debits (`post Σ⟦s⟧ = pre − ΣΔ`),
                // SYMMETRIC with the play-side `post_eval` in
                // `RuntimeOps::play_system_deploy`. The debit map is the one
                // RECOMPUTED in `replay_deploys` from `block.body.deploys` (the
                // play-side `settlement_debits` are not serialized into the
                // block) — same map ⇒ byte-identical writes. Run AFTER the
                // rig/replay-data check so the bare supply-produce events never
                // enter the rigged event set; the writes are captured by the
                // final replay checkpoint in `replay_deploys`. The replay path
                // enables the `ReplaySupplyMismatch` write-readback guard.
                close_block_deploy
                    .post_eval_replay(
                        &mut self.runtime_ops,
                        block_data,
                        &pre_state_hash,
                        settlement_debits,
                    )
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
        let (result, eval_res) = self.runtime_ops.eval_system_deploy(system_deploy).await?;
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
