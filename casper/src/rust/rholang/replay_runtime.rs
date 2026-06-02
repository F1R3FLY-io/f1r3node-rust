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
        // Task #13a: shard-genesis spec-strict acceptance-gate mode, threaded
        // verbatim into the replay-side recompute (same constant as play).
        strict_funding_enforcement: bool,
        // Task #13b: shard-genesis client funding-slot allocations, threaded
        // verbatim onto the reconstructed block-1 close deploy (same constant as
        // the play-side proposer used).
        client_fuel_allocations: &[(Vec<u8>, i64)],
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

        self.replay_deploys(
            start_hash,
            terms,
            system_deploys,
            !is_genesis,
            block_data,
            strict_funding_enforcement,
            client_fuel_allocations,
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
        with_cost_accounting: bool,
        block_data: &BlockData,
        // Task #13a: the shard-genesis spec-strict acceptance-gate mode
        // (`CasperShardConf::strict_funding_enforcement`), threaded from the
        // validation caller so the replay-side recompute + re-verification use
        // the SAME constant as the play-side gate (replay determinism).
        strict_funding_enforcement: bool,
        // Task #13b: the shard-genesis client funding-slot allocations
        // (`CasperShardConf::client_fuel_allocations`, lowered to raw pk bytes),
        // threaded from the validation caller onto the reconstructed block-1
        // `CloseBlockDeploy` so its `Σ⟦c⟧` seed is byte-identical to the play
        // side. Same shard constant as the proposer used (replay determinism);
        // empty on default shards.
        client_fuel_allocations: &[(Vec<u8>, i64)],
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
            self.recompute_and_verify_admission(&terms, strict_funding_enforcement)
                .await?
        } else {
            // Genesis / non-cost-accounted replay: no acceptance gate ran on the
            // play side, so there is nothing to recompute or debit.
            std::collections::BTreeMap::new()
        };

        // Cost-Accounted Rho Stage D: RECOMPUTE the per-block fee credit from the
        // block alone — count = `terms.len()` (= `block.body.deploys`, INCLUDING
        // failed + dummy deploys, every fed deploy is a recorded ProcessedDeploy)
        // and recipient = the proposing validator (`block_data.sender`). Same
        // recompute-from-block discipline as `replay_debits`; fed to
        // `replay_block_system_deploy` so the closeBlock F_v credit is
        // byte-identical to the play side (the fee amount is NOT serialized into
        // the block). Empty / non-cost-accounted blocks recompute to `None`.
        let replay_fee_credit = if with_cost_accounting {
            crate::rust::util::rholang::acceptance::recompute_fee_credits(
                terms.len(),
                block_data.sender.bytes.to_vec(),
            )
        } else {
            None
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
                .replay_block_system_deploy(
                    block_data,
                    &system_deploy,
                    &replay_debits,
                    &replay_fee_credit,
                    client_fuel_allocations,
                )
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
        strict_funding_enforcement: bool,
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
        // Task #13a: thread the shard-genesis strict flag (same constant as the
        // play side) into the recompute. Under `strict`, the recompute ALSO
        // re-verifies that no admitted `Δ > 0` deploy targets an absent pool
        // (a proposer that bypassed the spec-strict gate ⇒ invalid block).
        let debits = crate::rust::util::rholang::acceptance::recompute_settlement_debits(
            admitted,
            &reader,
            strict_funding_enforcement,
        )
        .await?;

        // Re-verify admission: each PRESENT pool's admitted ΣΔ_s ≤ Σ_s. The
        // recompute already restricted `debits` to present pools, so reading the
        // present balance here and comparing catches an over-admitting proposer
        // before the close-block settlement debit's `checked_sub` would (clearer
        // diagnostic + fails the block at the gate boundary, not deep in
        // closeBlock).
        for (sig_key, debit) in &debits {
            let balance =
                crate::rust::util::rholang::supply::read_balance(&self.runtime_ops, &debit.channel)
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
    /// D3 (DR-9, OD-1/OD-2): the escrow pre-charge / refund replay fan-out is
    /// REMOVED. A deploy's fundedness was settled once against Σ⟦s⟧ by the
    /// block's acceptance gate (recomputed deterministically on replay by
    /// `recompute_settlement_debits`); there is no per-cosigner charge/refund to
    /// re-run here. Replay simply re-evaluates the user deploy and asserts the
    /// per-COMM cost matches the stored `processed_deploy.cost` (the
    /// `replay_cost_mismatch` consensus check inside [`Self::run_user_deploy`]).
    /// The user deploy runs UNMETERED-FOR-LIVENESS via `evaluate` (which the
    /// play path's `evaluate_cosigned` mirrors with an `unsafe_max` budget), so
    /// play and replay observe the same per-COMM `total_cost()`.
    async fn process_deploy_with_cost_accounting(
        &mut self,
        processed_deploy: &ProcessedDeploy,
        mergeable_channels: &mut HashMap<Par, MergeType>,
    ) -> Result<bool, CasperError> {
        let eval_successful = if processed_deploy.system_deploy_error.is_none() {
            // Re-evaluate the user deploy. `run_user_deploy` owns the
            // soft-checkpoint rollback for a failed deploy and the per-COMM
            // `replay_cost_mismatch` consensus check.
            let evaluate_start = Instant::now();
            let (_, successful) = self
                .run_user_deploy(processed_deploy, mergeable_channels)
                .await?;
            metrics::histogram!(BLOCK_REPLAY_DEPLOY_EVALUATE_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                .record(evaluate_start.elapsed().as_secs_f64());
            tracing::debug!(target: "f1r3fly.casper.replay-rho-runtime", "deploy-eval-done");
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
        fee_credit: &Option<crate::rust::util::rholang::acceptance::FeeCredit>,
        // Task #13b: the genesis client funding-slot allocations
        // `[(client_pk_bytes, amount)]` reconstructed from the shard-genesis conf
        // (the SAME constant the play-side proposer used), threaded onto the
        // reconstructed `CloseBlockDeploy` so its block-1 `Σ⟦c⟧` seed is
        // byte-identical to play. Empty on default shards (back-compat) and on
        // every non-block-1 block (the credit is gated on `block_number == 1`).
        client_fuel_allocations: &[(Vec<u8>, i64)],
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

                // Capture the pre-slash store root for the Stage-C supply
                // `Σ⟦v⟧`-zero context (diagnostics / mismatch reporting); the
                // supply read/write themselves target the live store.
                let pre_state_hash: StateHash =
                    self.runtime_ops.runtime.get_root().await.to_bytes_prost();

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

                // Cost-Accounted Rho Stage-C (Decision 4 + 6.3): zero the
                // offender's supply pool `Σ⟦offender⟧`, SYMMETRIC with the
                // play-side `post_eval` auto-call in
                // `RuntimeOps::play_system_deploy`. The offender pk is the one
                // the Rholang `slash` contract published on `sys:casper:slashedPk`
                // (re-resolved from the SAME `invalid_block_hash` on replay), so
                // the zeroed datum is byte-identical play↔replay. Run AFTER the
                // rig/replay-data check so the bare supply-produce event never
                // enters the rigged event set; the write is captured by the final
                // replay checkpoint. The replay path enables the
                // `ReplaySupplyMismatch` write-readback guard.
                slash_deploy
                    .post_eval_replay(&mut self.runtime_ops, block_data, &pre_state_hash)
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
                    // Cost-Accounted Rho Stage D: the per-block fee credit
                    // RECOMPUTED in `replay_deploys` from `block.body.deploys`
                    // (count = `terms.len()`) + the proposing validator
                    // (`block_data.sender`). Threaded here so `seed_fee_count`
                    // seeds the SAME `(sender, count)` datum the play side did,
                    // making the closeBlock F_v credit byte-identical play↔replay.
                    fee_credits: fee_credit.clone(),
                    // Task #13b: the genesis client funding-slot allocations,
                    // reconstructed from the shard-genesis conf (the SAME constant
                    // the play-side proposer read). `dual_write_supply` credits
                    // each `Σ⟦c⟧` ONLY at the block-1 close (gated on
                    // `block_number == 1`); the per-allocation `random_state` is
                    // derived from this close deploy's replay-stable `initial_rand`
                    // advanced by the SORTED-pk index — so the block-1 `Σ⟦c⟧` seed
                    // is byte-identical to the play side. Empty ⇒ no credit.
                    client_fuel_allocations: client_fuel_allocations.to_vec(),
                };

                // Capture the pre-close store root for the Stage-B supply
                // dual-write context (diagnostics / mismatch reporting); the
                // supply read/write themselves target the live store.
                let pre_state_hash: StateHash =
                    self.runtime_ops.runtime.get_root().await.to_bytes_prost();

                self.rig_system_deploy(processed_system_deploy).await?;

                // (Stage D: the `fee_credits` set on `close_block_deploy` above —
                // RECOMPUTED from `block.body.deploys` — is read by `post_eval_replay`
                // below for the byte-identical FeeExtract collection credit to the
                // proposer's Rust fee pool `F_v`; the convert mirror reads the
                // closeBlock-published eligible list from the store. No pre-eval
                // Rholang seeding is involved.)
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
                // Redemption has NO supply `post_eval` (writes neither Σ⟦v⟧ nor
                // @W_v), so there is no post-eval call here.
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
                let mut redeem_deploy = RedeemDeploy {
                    validator_pk: validator_pk.to_vec(),
                    outcome,
                    pos_multi_sig_public_keys: pos_multi_sig_public_keys.clone(),
                    pos_multi_sig_quorum: *pos_multi_sig_quorum,
                    authorizations: authorizations
                        .iter()
                        .map(|a| RedemptionAuthorization {
                            public_key: a.public_key.to_vec(),
                            signature: a.signature.to_vec(),
                        })
                        .collect(),
                    initial_rand: system_deploy_util::generate_redeem_deploy_random_seed(
                        block_data.sender.bytes.clone(),
                        block_data.seq_num,
                        outcome_tag,
                    ),
                };

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
