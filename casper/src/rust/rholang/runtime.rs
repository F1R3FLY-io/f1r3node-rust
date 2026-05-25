// See casper/src/main/scala/coop/rchain/casper/rholang/RuntimeSyntax.scala

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::mem;
use std::sync::OnceLock;
use std::time::Instant;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, GPrivate, GUnforgeable, ListParWithRandom, Par, TaggedContinuation,
};
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{
    Bond, DeployData, Event, ProcessedDeploy, ProcessedSystemDeploy, SystemDeployData,
};
// `normalizer_env_from_deploy` is replaced by `normalizer_env_from_cosigned_deploy`
// at the only remaining call site (inside `evaluate_cosigned`). The legacy `evaluate`
// path uplifts `Signed<DeployData>` to `Cosigned<DeployData>` via
// `Cosigned::from_single_signer` and delegates, so the legacy env builder is no
// longer reached from runtime.rs.
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::sorted_par_hash_set::SortedParHashSet;
use models::rust::sorted_par_map::SortedParMap;
use models::rust::utils::new_freevar_par;
use models::rust::validator::Validator;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::has_cost::HasCost;
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::env::Env;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::interpreter::EvaluateResult;
use rholang::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
use rholang::rust::interpreter::rho_runtime::{bootstrap_registry, RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::system_processes::{
    BlockData, DeployData as SystemProcessDeployData,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::history::instances::radix_history::RadixHistory;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::merger::merging_logic::{MergeType, NumberChannelsEndVal};

use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    BLOCK_REPLAY_SYSDEPLOY_EVAL_CONSUME_RESULT_TIME_METRIC,
    BLOCK_REPLAY_SYSDEPLOY_EVAL_EVALUATE_SOURCE_TIME_METRIC, CASPER_METRICS_SOURCE,
    EVALUATE_SOURCE_WRAPPER_CALLS_METRIC, EVALUATE_SOURCE_WRAPPER_TIME_NS_METRIC,
    EVAL_SYSTEM_DEPLOY_WRAPPER_CALLS_METRIC, EVAL_SYSTEM_DEPLOY_WRAPPER_TIME_NS_METRIC,
};
use crate::rust::rholang::types::eval_collector::EvalCollector;
use crate::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use crate::rust::util::rholang::costacc::pre_charge_deploy::PreChargeDeploy;
use crate::rust::util::rholang::costacc::refund_deploy::RefundDeploy;
use crate::rust::util::rholang::costacc::slash_deploy::SlashDeploy;
use crate::rust::util::rholang::system_deploy::SystemDeployTrait;
use crate::rust::util::rholang::system_deploy_result::SystemDeployResult;
use crate::rust::util::rholang::system_deploy_user_error::{
    SystemDeployPlatformFailure, SystemDeployUserError,
};
use crate::rust::util::rholang::tools::Tools;
use crate::rust::util::rholang::{interpreter_util, system_deploy_util};
use crate::rust::util::{construct_deploy, event_converter};

pub struct RuntimeOps {
    pub runtime: RhoRuntimeImpl,
}

impl RuntimeOps {
    pub fn new(runtime: RhoRuntimeImpl) -> Self { Self { runtime } }
}

#[allow(type_alias_bounds)]
pub type SysEvalResult<S: SystemDeployTrait> =
    (Either<SystemDeployUserError, S::Result>, EvaluateResult);

fn system_deploy_consume_all_pattern() -> BindPattern {
    BindPattern {
        patterns: vec![new_freevar_par(0, Vec::new())],
        remainder: None,
        free_count: 1,
    }
}

impl RuntimeOps {
    /**
     * Because of the history legacy, the emptyStateHash does not really represent an empty trie.
     * The `emptyStateHash` is used as genesis block pre state which the state only contains registry
     * fixed channels in the state.
     */
    pub async fn empty_state_hash(&mut self) -> Result<StateHash, CasperError> {
        self.runtime
            .reset(&RadixHistory::empty_root_node_hash())
            .await?;

        bootstrap_registry(&self.runtime).await;
        let checkpoint = self.runtime.create_checkpoint().await;
        Ok(checkpoint.root.bytes().into())
    }

    /* Compute state with deploys (genesis block) and System deploys (regular block) */

    /**
     * Evaluates deploys and System deploys with checkpoint to get final state hash
     */
    pub async fn compute_state(
        &mut self,
        start_hash: &StateHash,
        terms: Vec<Signed<DeployData>>,
        system_deploys: Vec<crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum>,
        block_data: BlockData,
        invalid_blocks: HashMap<BlockHash, Validator>,
    ) -> Result<
        (
            StateHash,
            Vec<(ProcessedDeploy, NumberChannelsEndVal)>,
            Vec<(ProcessedSystemDeploy, NumberChannelsEndVal)>,
        ),
        CasperError,
    > {
        let mem_profile_enabled = crate::rust::util::rholang::mem_profiler::mem_profile_enabled();
        let read_vm_rss_kb =
            || -> Option<usize> { crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() };
        let mut rss_baseline = if mem_profile_enabled {
            read_vm_rss_kb()
        } else {
            None
        };
        let mut rss_prev = rss_baseline;
        let mut log_mem_step = |step: &str| {
            if !mem_profile_enabled {
                return;
            }
            if let Some(curr) = read_vm_rss_kb() {
                let prev = rss_prev.unwrap_or(curr);
                let baseline = rss_baseline.unwrap_or(curr);
                eprintln!(
                    "compute_state.mem step={} rss_kb={} delta_prev_kb={} delta_total_kb={}",
                    step,
                    curr,
                    curr as i64 - prev as i64,
                    curr as i64 - baseline as i64
                );
                rss_prev = Some(curr);
                if rss_baseline.is_none() {
                    rss_baseline = Some(curr);
                }
            }
        };

        // Using tracing events instead of spans for async context
        // Span[F].traceI("compute-state") equivalent from Scala
        tracing::info!(target: "f1r3fly.casper.runtime", "compute-state-started");
        log_mem_step("start");
        self.runtime.set_block_data(block_data).await;
        log_mem_step("after_set_block_data");
        self.runtime.set_invalid_blocks(invalid_blocks).await;
        log_mem_step("after_set_invalid_blocks");

        let (start_hash, processed_deploys) =
            self.play_deploys_for_state(start_hash, terms).await?;
        log_mem_step("after_play_deploys_for_state");

        let mut current_hash = start_hash;
        let mut processed_system_deploys = Vec::with_capacity(system_deploys.len());

        for (idx, system_deploy_enum) in system_deploys.into_iter().enumerate() {
            if mem_profile_enabled {
                let before_step = format!("before_system_deploy_{}", idx + 1);
                log_mem_step(&before_step);
            }
            // Match on the enum and call appropriate generic method
            let result = match system_deploy_enum {
                crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Slash(
                    mut slash_deploy,
                ) => {
                    self.play_system_deploy(&current_hash, &mut slash_deploy)
                        .await?
                }
                crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                    mut close_deploy,
                ) => {
                    self.play_system_deploy(&current_hash, &mut close_deploy)
                        .await?
                }
            };
            if mem_profile_enabled {
                let after_step = format!("after_system_deploy_{}", idx + 1);
                log_mem_step(&after_step);
            }

            match result {
                SystemDeployResult::PlaySucceeded {
                    state_hash,
                    processed_system_deploy,
                    mergeable_channels,
                    result: _,
                } => {
                    processed_system_deploys.push((processed_system_deploy, mergeable_channels));
                    current_hash = state_hash;
                }
                SystemDeployResult::PlayFailed {
                    processed_system_deploy: ProcessedSystemDeploy::Failed { error_msg, .. },
                } => {
                    return Err(CasperError::RuntimeError(format!(
                        "Unexpected system error during play of system deploy: {}",
                        error_msg
                    )))
                }
                SystemDeployResult::PlayFailed {
                    processed_system_deploy: ProcessedSystemDeploy::Succeeded { .. },
                } => {
                    return Err(CasperError::RuntimeError(
                        "Unreachable code path. This is likely caused by a bug in the runtime."
                            .to_string(),
                    ))
                }
            }
        }

        let post_state_hash = current_hash;
        log_mem_step("finish");

        tracing::info!(target: "f1r3fly.casper.runtime", "compute-state-finished");
        Ok((post_state_hash, processed_deploys, processed_system_deploys))
    }

    /**
     * Evaluates genesis deploys with checkpoint to get final state hash
     */
    pub async fn compute_genesis(
        &mut self,
        terms: Vec<Signed<DeployData>>,
        block_time: i64,
        block_number: i64,
    ) -> Result<
        (
            StateHash,
            StateHash,
            Vec<(ProcessedDeploy, NumberChannelsEndVal)>,
        ),
        CasperError,
    > {
        // Using tracing events instead of spans for async context
        // Span[F].traceI("compute-genesis") equivalent from Scala
        tracing::info!(target: "f1r3fly.casper.runtime", "compute-genesis-started");
        self.runtime
            .set_block_data(BlockData {
                time_stamp: block_time,
                block_number,
                sender: PublicKey::from_bytes(&Vec::new()),
                seq_num: 0,
            })
            .await;

        let genesis_pre_state_hash = self.empty_state_hash().await?;
        let play_result = self
            .play_deploys_for_genesis(&genesis_pre_state_hash, terms)
            .await?;

        let (post_state_hash, processed_deploys) = play_result;
        tracing::info!(target: "f1r3fly.casper.runtime", "compute-genesis-finished");
        Ok((genesis_pre_state_hash, post_state_hash, processed_deploys))
    }

    /* Deploy evaluators */

    /**
     * Evaluates deploys on root hash with checkpoint to get final state hash
     */
    pub async fn play_deploys_for_state(
        &mut self,
        start_hash: &StateHash,
        terms: Vec<Signed<DeployData>>,
    ) -> Result<(StateHash, Vec<(ProcessedDeploy, NumberChannelsEndVal)>), CasperError> {
        let mem_profile_enabled = crate::rust::util::rholang::mem_profiler::mem_profile_enabled();
        let read_vm_rss_kb =
            || -> Option<usize> { crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() };
        let mut rss_baseline = if mem_profile_enabled {
            read_vm_rss_kb()
        } else {
            None
        };
        let mut rss_prev = rss_baseline;
        let mut log_mem_step = |step: &str| {
            if !mem_profile_enabled {
                return;
            }
            if let Some(curr) = read_vm_rss_kb() {
                let prev = rss_prev.unwrap_or(curr);
                let baseline = rss_baseline.unwrap_or(curr);
                eprintln!(
                    "play_deploys_for_state.mem step={} rss_kb={} delta_prev_kb={} delta_total_kb={}",
                    step,
                    curr,
                    curr as i64 - prev as i64,
                    curr as i64 - baseline as i64
                );
                rss_prev = Some(curr);
                if rss_baseline.is_none() {
                    rss_baseline = Some(curr);
                }
            }
        };

        // Using tracing events for async - Span[F].withMarks("play-deploys") from Scala
        tracing::info!(target: "f1r3fly.casper.play-deploys", "play-deploys-started");
        log_mem_step("start");
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(start_hash))
            .await?;
        log_mem_step("after_reset");

        let mut res = Vec::with_capacity(terms.len());
        for (idx, deploy) in terms.into_iter().enumerate() {
            if mem_profile_enabled {
                let before = format!("before_deploy_{}", idx + 1);
                log_mem_step(&before);
            }
            res.push(self.play_deploy_with_cost_accounting(deploy).await?);
            if mem_profile_enabled {
                let after = format!("after_deploy_{}", idx + 1);
                log_mem_step(&after);
            }
        }

        log_mem_step("before_final_checkpoint");
        log_mem_step("before_final_checkpoint_create_checkpoint");
        let final_checkpoint = self.runtime.create_checkpoint().await;
        log_mem_step("after_final_checkpoint_create_checkpoint");
        log_mem_step("before_final_checkpoint_root_to_bytes");
        let final_root = final_checkpoint.root.to_bytes_prost();
        log_mem_step("after_final_checkpoint_root_to_bytes");
        log_mem_step("after_final_checkpoint");
        Ok((final_root, res))
    }

    /**
     * Evaluates deploys on root hash with checkpoint to get final state hash
     */
    pub async fn play_deploys_for_genesis(
        &mut self,
        start_hash: &StateHash,
        terms: Vec<Signed<DeployData>>,
    ) -> Result<(StateHash, Vec<(ProcessedDeploy, NumberChannelsEndVal)>), CasperError> {
        // Using tracing events for async - Span[F].withMarks("play-deploys") from Scala
        tracing::info!(target: "f1r3fly.casper.play-deploys-genesis", "play-deploys-genesis-started");
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(start_hash))
            .await?;

        let mut res = Vec::with_capacity(terms.len());
        for deploy in terms {
            res.push(self.process_deploy_with_mergeable_data(deploy).await?);
        }

        let final_checkpoint = self.runtime.create_checkpoint().await;
        Ok((final_checkpoint.root.to_bytes_prost(), res))
    }

    /**
     * Evaluates deploy with cost accounting (PoS Pre-charge and Refund calls).
     *
     * Legacy single-signature adapter. Byte-identical observable behavior to
     * the pre-multi-signature implementation — same `deploy_id`, same vault
     * deltas, same cost-trace digests, same `ProcessedDeploy::empty` failure
     * envelope on pre-charge failure. Achieved by uplifting `Signed<DeployData>`
     * to a single-signer `Cosigned<DeployData>` envelope (via
     * `Cosigned::from_single_signer`) and delegating to the canonical
     * `play_deploy_with_cost_accounting_cosigned` implementation. The legacy
     * seed-derivation path (via `as_legacy_signed_ref` in the cosigned method)
     * preserves replay determinism for existing on-chain deploys.
     */
    pub async fn play_deploy_with_cost_accounting(
        &mut self,
        deploy: Signed<DeployData>,
    ) -> Result<(ProcessedDeploy, NumberChannelsEndVal), CasperError> {
        let phlo_limit = deploy.data.phlo_limit;
        let cosigned = crypto::rust::signatures::signed::Cosigned::from_single_signer(
            deploy, phlo_limit,
        )
        .map_err(|e| {
            CasperError::RuntimeError(format!("legacy uplift to Cosigned failed: {e}"))
        })?;
        self.play_deploy_with_cost_accounting_cosigned(cosigned).await
    }

    /// Multi-signature aware deploy execution with cost accounting.
    ///
    /// Realizes the cost-accounted rho-calculus `σ₁ & σ₂` operational
    /// semantics at the protocol level: each cosigner pre-charges their
    /// share of `phlo_limit * phlo_price` to the PoS vault under a single
    /// atomic soft-checkpoint scope; the user deploy evaluates against the
    /// shared budget; unused phlo refunds FIFO in canonical pk-ascending
    /// order (leftmost-by-pk consumed first, refunded last).
    ///
    /// Soft-checkpoint architecture (per §1.7.5):
    /// - **OUTER** scope (this method) wraps all per-cosigner pre-charges.
    ///   Revert on pre-charge failure rolls back ALL preceding cosigner
    ///   debits AND the PoS Map state atomically.
    /// - **INNER** scope (at `process_deploy_cosigned`) wraps the USER
    ///   DEPLOY only. Revert on user-deploy errors keeps pre-charge entries
    ///   in the PoS Map so refunds can drain them.
    /// - Refund-side failure is platform-level (existing
    ///   `SystemDeployPlatformFailure::GasRefundFailure` semantics).
    ///
    /// Legacy single-sig (`!cosigned.is_compound()`) routes through the
    /// legacy `generate_{pre_charge,refund}_deploy_random_seed` helpers and
    /// the legacy `DEPLOY_SIGNATURE_DOMAIN` (via `set_deploy_signature` in
    /// `evaluate_cosigned`), preserving byte-identical on-chain effects.
    pub async fn play_deploy_with_cost_accounting_cosigned(
        &mut self,
        cosigned: crypto::rust::signatures::signed::Cosigned<DeployData>,
    ) -> Result<(ProcessedDeploy, NumberChannelsEndVal), CasperError> {
        let mem_profile_enabled = crate::rust::util::rholang::mem_profiler::mem_profile_enabled();
        let read_vm_rss_kb =
            || -> Option<usize> { crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() };
        let mut rss_baseline = if mem_profile_enabled {
            read_vm_rss_kb()
        } else {
            None
        };
        let mut rss_prev = rss_baseline;
        let mut log_mem_step = |step: &str| {
            if !mem_profile_enabled {
                return;
            }
            if let Some(curr) = read_vm_rss_kb() {
                let prev = rss_prev.unwrap_or(curr);
                let baseline = rss_baseline.unwrap_or(curr);
                eprintln!(
                    "play_deploy_with_cost_accounting.mem step={} rss_kb={} delta_prev_kb={} delta_total_kb={}",
                    step,
                    curr,
                    curr as i64 - prev as i64,
                    curr as i64 - baseline as i64
                );
                rss_prev = Some(curr);
                if rss_baseline.is_none() {
                    rss_baseline = Some(curr);
                }
            }
        };

        tracing::debug!(target: "f1r3fly.casper.play-deploy", "play-deploy-started");
        log_mem_step("start");
        let mut eval_collector_state = EvalCollector::new();

        let primary_pk_hex = hex::encode(&cosigned.primary().pk.bytes);
        let primary_sig_hex = hex::encode(&cosigned.primary().sig);
        let phlo_price = cosigned.data.phlo_price;
        let is_compound = cosigned.is_compound();

        // OUTER soft-checkpoint — covers per-cosigner pre-charge fan-out and
        // the user deploy. INNER scope at process_deploy_cosigned wraps the
        // user deploy only (existing behavior preserved). Pre-charge revert
        // rolls back ALL preceding cosigner debits + PoS Map state.
        let outer_fallback = self.runtime.create_soft_checkpoint().await;

        // (B) Pre-charge fan-out — canonical pk-ascending order.
        // For single-signer cosigned (legacy uplift), the loop runs exactly
        // once with the primary signer.
        log_mem_step("before_precharge_fanout");
        for (i, signer) in cosigned.signers().iter().enumerate() {
            let charge = signer.phlo_share.saturating_mul(phlo_price);
            let rand = if is_compound {
                system_deploy_util::generate_pre_charge_deploy_random_seed_for_signer(
                    &cosigned, i,
                )
            } else {
                // Legacy single-sig: byte-identical seed to existing on-chain
                // deploys (preserves replay determinism for legacy state).
                let legacy = cosigned.as_legacy_signed_ref();
                system_deploy_util::generate_pre_charge_deploy_random_seed(&legacy)
            };
            tracing::debug!(target: "f1r3fly.casper.precharge",
                "precharge-started cosigner_index={} pk={}", i, hex::encode(&signer.pk.bytes));
            tracing::debug!(
                "PreCharging {} for {} (cosigner {} of {})",
                hex::encode(&signer.pk.bytes),
                charge,
                i,
                cosigned.signers().len()
            );
            let (event_log, result, mergeable_channels) = self
                .play_system_deploy_internal(&mut PreChargeDeploy {
                    charge_amount: charge,
                    pk: signer.pk.clone(),
                    rand,
                })
                .await?;
            eval_collector_state.add(event_log, mergeable_channels);
            if let Either::Left(error) = result {
                // Atomic rollback of ALL preceding pre-charges + PoS Map state.
                self.runtime.revert_to_soft_checkpoint(outer_fallback).await;
                tracing::error!(
                    "Pre-charge failure for cosigner {} (pk={}): {}",
                    i, hex::encode(&signer.pk.bytes), error.error_message
                );
                if !is_compound {
                    // Legacy single-sig path: preserve byte-identical
                    // ProcessedDeploy::empty + system_deploy_error envelope
                    // so block validators see the same error envelope as
                    // existing on-chain deploys.
                    let legacy_signed = cosigned.into_legacy_signed_unchecked();
                    let mut empty_pd = ProcessedDeploy::empty(legacy_signed);
                    empty_pd.system_deploy_error = Some(error.error_message);
                    let mergeable_channels_data = self
                        .get_number_channels_data(&eval_collector_state.mergeable_channels)
                        .await?;
                    let deploy_log = mem::take(&mut eval_collector_state.event_log);
                    return Ok((
                        ProcessedDeploy {
                            deploy_log,
                            ..empty_pd
                        },
                        mergeable_channels_data,
                    ));
                }
                // Multi-sig: surface precise per-cosigner error.
                return Err(CasperError::InsufficientPhloByCosigner {
                    signer_index: i,
                    pk_hex: hex::encode(&signer.pk.bytes),
                    message: error.error_message,
                });
            }
        }
        log_mem_step("after_precharge_fanout");

        // (C) USER DEPLOY (has its own inner soft-checkpoint).
        tracing::debug!(target: "f1r3fly.casper.user-deploy",
            "user-deploy-started primary_pk={}", primary_pk_hex);
        let (mut pd, mc) = self.process_deploy_cosigned(cosigned.clone()).await?;
        let deploy_log = mem::take(&mut pd.deploy_log);
        eval_collector_state.add(deploy_log, mc);
        log_mem_step("after_user_deploy");

        // (D) Refund fan-out — FIFO drain in canonical pk-ascending order.
        // Leftmost-by-pk cosigner's tokens consumed first (refunded last);
        // rightmost cosigner's tokens consumed last (refunded most of any
        // unused phlo). Matches the operational reading of
        // left-associated Sig::And and Token::Gate.
        let total_refund = pd
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
                system_deploy_util::generate_refund_deploy_random_seed_for_signer(
                    &cosigned, i,
                )
            } else {
                let legacy = cosigned.as_legacy_signed_ref();
                system_deploy_util::generate_refund_deploy_random_seed(&legacy)
            };
            tracing::debug!(target: "f1r3fly.casper.refund",
                "refund-started cosigner_index={} pk={}", i, hex::encode(&signer.pk.bytes));
            tracing::debug!(
                "Refunding {} with {} (cosigner {} of {}; consumed={}, charged={})",
                hex::encode(&signer.pk.bytes),
                refund_amount,
                i,
                cosigned.signers().len(),
                signer_consumed,
                signer_charged
            );
            let (event_log, result, mergeable_channels) = self
                .play_system_deploy_internal(&mut RefundDeploy {
                    refund_amount,
                    pk: signer.pk.clone(),
                    rand,
                })
                .await?;
            eval_collector_state.add(event_log, mergeable_channels);
            if let Either::Left(error) = result {
                // Refund failure is platform-level — existing semantics
                // (SystemDeployPlatformFailure::GasRefundFailure). The outer
                // soft-checkpoint is NOT reverted; the block will be rejected
                // by the validator and operators investigate.
                let failure_context = format!(
                    "{}, primary_sig={}, primary_pk={}, cosigner_index={}, \
                     cosigner_pk={}, refund_amount={}",
                    error.error_message,
                    primary_sig_hex,
                    primary_pk_hex,
                    i,
                    hex::encode(&signer.pk.bytes),
                    refund_amount
                );
                metrics::counter!(
                    "casper_runtime_refund_failures_total",
                    "source" => CASPER_METRICS_SOURCE
                )
                .increment(1);
                tracing::warn!("Refund failure '{}'", failure_context);
                return Err(CasperError::SystemRuntimeError(
                    SystemDeployPlatformFailure::GasRefundFailure(failure_context),
                ));
            }
        }
        log_mem_step("after_refund_fanout");
        debug_assert_eq!(
            remaining_used, 0,
            "FIFO drain incomplete: remaining_used={} after fan-out; \
             total_used={} > Σ(phlo_share × phlo_price)={} — multi-payer \
             accounting bug",
            remaining_used, total_used, total_charge
        );

        let mergeable_channels_data = self
            .get_number_channels_data(&eval_collector_state.mergeable_channels)
            .await?;
        let deploy_log = mem::take(&mut eval_collector_state.event_log);
        log_mem_step("after_collect_result");
        Ok((
            ProcessedDeploy { deploy_log, ..pd },
            mergeable_channels_data,
        ))
    }

    /// Legacy single-signature user-deploy execution. Uplifts to
    /// `Cosigned<DeployData>` and delegates to [`Self::process_deploy_cosigned`]
    /// for byte-identical observable behavior.
    pub async fn process_deploy(
        &mut self,
        deploy: Signed<DeployData>,
    ) -> Result<(ProcessedDeploy, HashMap<Par, MergeType>), CasperError> {
        let phlo_limit = deploy.data.phlo_limit;
        let cosigned = crypto::rust::signatures::signed::Cosigned::from_single_signer(
            deploy, phlo_limit,
        )
        .map_err(|e| {
            CasperError::RuntimeError(format!(
                "legacy uplift to Cosigned failed in process_deploy: {e}"
            ))
        })?;
        self.process_deploy_cosigned(cosigned).await
    }

    /// Multi-signature aware user-deploy execution. Keeps the INNER
    /// soft-checkpoint (line 560 in the legacy implementation) that wraps
    /// the user deploy ONLY — on user-deploy errors the inner scope
    /// reverts user-deploy effects but pre-charge state remains intact so
    /// refunds can drain the PoS Map.
    ///
    /// The `ProcessedDeploy.deploy: Signed<DeployData>` storage shape is
    /// preserved by reconstituting the primary signer's `Signed<DeployData>`
    /// envelope via `Cosigned::into_legacy_signed_unchecked` — invariants
    /// were already enforced at `Cosigned::from_signed_data` construction so
    /// no re-verification is needed.
    pub async fn process_deploy_cosigned(
        &mut self,
        cosigned: crypto::rust::signatures::signed::Cosigned<DeployData>,
    ) -> Result<(ProcessedDeploy, HashMap<Par, MergeType>), CasperError> {
        // INNER soft-checkpoint — wraps USER DEPLOY only. Back-compat
        // semantics preserved exactly: pre-charge state (in the outer scope
        // at `play_deploy_with_cost_accounting_cosigned`) is NOT reverted by
        // user-deploy errors, so refunds can drain the PoS Map.
        let fallback = self.runtime.create_soft_checkpoint().await;

        let eval_result = self.evaluate_cosigned(&cosigned).await?;

        let deploy_log = self.runtime.take_event_log().await;
        let cost_trace = self.runtime.cost.cost_trace_digest();

        let eval_succeeded = eval_result.errors.is_empty();
        let primary_sig = cosigned.primary().sig.clone();
        // Reconstitute the legacy Signed<DeployData> shape for ProcessedDeploy
        // storage. For single-sig (legacy uplift), this returns a byte-identical
        // legacy envelope. For multi-sig, the additional cosigners are dropped
        // from the storage shape — when §1.9 lands, ProcessedDeploy will be
        // extended to carry the cosigner list; for now back-compat with the
        // existing on-disk shape is preserved.
        let legacy_signed = cosigned.into_legacy_signed_unchecked();

        let deploy_result = ProcessedDeploy {
            deploy: legacy_signed,
            cost: Cost::to_proto(eval_result.cost),
            deploy_log: deploy_log
                .into_iter()
                .map(|event| event_converter::to_casper_event(event))
                .collect(),
            is_failed: !eval_succeeded,
            system_deploy_error: None,
            cost_trace_digest: cost_trace.digest.into(),
            cost_trace_event_count: cost_trace.event_count,
        };

        if !eval_succeeded {
            self.runtime.revert_to_soft_checkpoint(fallback).await;
            interpreter_util::print_deploy_errors(&primary_sig, &eval_result.errors);
        }

        Ok((deploy_result, eval_result.mergeable))
    }

    /// Legacy single-signature variant. Thin wrapper around
    /// [`Self::process_deploy_with_mergeable_data_cosigned`].
    pub async fn process_deploy_with_mergeable_data(
        &mut self,
        deploy: Signed<DeployData>,
    ) -> Result<(ProcessedDeploy, NumberChannelsEndVal), CasperError> {
        let phlo_limit = deploy.data.phlo_limit;
        let cosigned = crypto::rust::signatures::signed::Cosigned::from_single_signer(
            deploy, phlo_limit,
        )
        .map_err(|e| {
            CasperError::RuntimeError(format!(
                "legacy uplift to Cosigned failed in process_deploy_with_mergeable_data: {e}"
            ))
        })?;
        self.process_deploy_with_mergeable_data_cosigned(cosigned).await
    }

    pub async fn process_deploy_with_mergeable_data_cosigned(
        &mut self,
        cosigned: crypto::rust::signatures::signed::Cosigned<DeployData>,
    ) -> Result<(ProcessedDeploy, NumberChannelsEndVal), CasperError> {
        let (pd, merge_chs) = self.process_deploy_cosigned(cosigned).await?;
        let data = self.get_number_channels_data(&merge_chs).await?;
        Ok((pd, data))
    }

    pub async fn get_number_channels_data(
        &self,
        channels: &std::collections::HashMap<
            Par,
            rspace_plus_plus::rspace::merger::merging_logic::MergeType,
        >,
    ) -> Result<NumberChannelsEndVal, CasperError> {
        let mut result = BTreeMap::new();
        for (channel, merge_type) in channels {
            if let Some((hash, value)) = self.get_number_channel(channel, *merge_type).await? {
                result.insert(hash, (value, *merge_type));
            }
        }
        Ok(result)
    }

    /// Deterministic multi-value fold for a mergeable channel that holds more
    /// than one numeric Datum at observation time. Dispatches by `MergeType`:
    /// `IntegerAdd` picks the max (conservative for vault balances);
    /// `BitmaskOr` OR-folds all bitmaps (no set bit is lost). Returns `None`
    /// for an empty input.
    pub fn fold_multi_value(values: &[i64], merge_type: MergeType) -> Option<i64> {
        if values.is_empty() {
            return None;
        }
        let folded = match merge_type {
            MergeType::IntegerAdd => *values.iter().max().unwrap(),
            MergeType::BitmaskOr => values
                .iter()
                .fold(0i64, |acc, v| ((acc as u64) | (*v as u64)) as i64),
        };
        Some(folded)
    }

    pub async fn get_number_channel(
        &self,
        channel: &Par,
        merge_type: MergeType,
    ) -> Result<Option<(Blake2b256Hash, i64)>, CasperError> {
        let ch_values = self.runtime.get_data(channel).await;

        if ch_values.is_empty() {
            Ok(None)
        } else {
            let ch_hash = stable_hash_provider::hash(channel);
            if ch_values.len() != 1 {
                // Liveness-first fallback: ambiguous mergeable channel values should not wedge
                // proposing. Non-numeric values are skipped — they aren't candidates for the
                // numeric merge path and fall through to existing conflict handling.
                let nums: Vec<i64> = ch_values
                    .iter()
                    .filter_map(|datum| {
                        RholangMergingLogic::try_get_number_with_rnd(&datum.a).map(|(n, _)| n)
                    })
                    .collect();

                let num = match Self::fold_multi_value(&nums, merge_type) {
                    Some(n) => n,
                    None => return Ok(None),
                };

                tracing::warn!(
                    target: "f1r3fly.mergeable_channel.sanitize",
                    "NumberChannel has {} values; merge_type={:?} dispatched value={} for channel {}",
                    ch_values.len(),
                    merge_type,
                    num,
                    hex::encode(ch_hash.clone().bytes()),
                );
                metrics::counter!(
                    "mergeable_channel_number_sanitized_total",
                    "source" => "casper_runtime"
                )
                .increment(1);

                return Ok(Some((ch_hash, num)));
            }

            // Single value: opportunistic numeric read. Non-numeric values
            // (e.g., TreeHashMap leaf Maps tagged with the bitmask tag) are
            // skipped here and fall through to the existing conflict path.
            let num_par = &ch_values[0].a;
            match RholangMergingLogic::try_get_number_with_rnd(num_par) {
                Some((num, _)) => Ok(Some((ch_hash, num))),
                None => Ok(None),
            }
        }
    }

    /* System deploy evaluators */

    /**
     * Evaluates System deploy with checkpoint to get final state hash
     */
    pub async fn play_system_deploy<S: SystemDeployTrait>(
        &mut self,
        state_hash: &StateHash,
        system_deploy: &mut S,
    ) -> Result<SystemDeployResult<S::Result>, CasperError> {
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(state_hash))
            .await?;

        let (event_log, result, mergeable_channels) =
            self.play_system_deploy_internal(system_deploy).await?;

        let final_state_hash = {
            let checkpoint = self.runtime.create_checkpoint().await;
            checkpoint.root.to_bytes_prost()
        };

        match result {
            Either::Right(system_deploy_result) => {
                let mcl = self.get_number_channels_data(&mergeable_channels).await?;
                if let Some(SlashDeploy {
                    invalid_block_hash,
                    pk,
                    target_activation_epoch,
                    initial_rand: _,
                }) = system_deploy.as_any().downcast_ref::<SlashDeploy>()
                {
                    Ok(SystemDeployResult::play_succeeded(
                        final_state_hash,
                        event_log,
                        SystemDeployData::create_slash(
                            invalid_block_hash.clone(),
                            pk.clone(),
                            *target_activation_epoch,
                        ),
                        mcl,
                        system_deploy_result,
                    ))
                } else if let Some(CloseBlockDeploy { .. }) =
                    system_deploy.as_any().downcast_ref::<CloseBlockDeploy>()
                {
                    Ok(SystemDeployResult::play_succeeded(
                        final_state_hash,
                        event_log,
                        SystemDeployData::create_close(),
                        mcl,
                        system_deploy_result,
                    ))
                } else {
                    Ok(SystemDeployResult::play_succeeded(
                        final_state_hash,
                        event_log,
                        SystemDeployData::Empty,
                        mcl,
                        system_deploy_result,
                    ))
                }
            }

            Either::Left(usr_err) => Ok(SystemDeployResult::play_failed(event_log, usr_err)),
        }
    }

    pub async fn play_system_deploy_internal<S: SystemDeployTrait>(
        &mut self,
        system_deploy: &mut S,
    ) -> Result<
        (
            Vec<Event>,
            Either<SystemDeployUserError, S::Result>,
            HashMap<Par, MergeType>,
        ),
        CasperError,
    > {
        let mem_profile_enabled = crate::rust::util::rholang::mem_profiler::mem_profile_enabled();
        let read_vm_rss_kb =
            || -> Option<usize> { crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() };
        let deploy_type = std::any::type_name::<S>();
        let mut rss_baseline = if mem_profile_enabled {
            read_vm_rss_kb()
        } else {
            None
        };
        let mut rss_prev = rss_baseline;
        let mut log_mem_step = |step: &str| {
            if !mem_profile_enabled {
                return;
            }
            if let Some(curr) = read_vm_rss_kb() {
                let prev = rss_prev.unwrap_or(curr);
                let baseline = rss_baseline.unwrap_or(curr);
                eprintln!(
                    "play_system_deploy_internal.mem deploy_type={} step={} rss_kb={} delta_prev_kb={} delta_total_kb={}",
                    deploy_type,
                    step,
                    curr,
                    curr as i64 - prev as i64,
                    curr as i64 - baseline as i64
                );
                rss_prev = Some(curr);
                if rss_baseline.is_none() {
                    rss_baseline = Some(curr);
                }
            }
        };
        log_mem_step("start");

        // Get System deploy result / throw fatal errors for unexpected results
        let (result_or_system_deploy_error, eval_result) =
            self.eval_system_deploy(system_deploy).await?;
        log_mem_step("after_eval_system_deploy");

        let log = self.runtime.take_event_log().await;
        log_mem_step("after_take_event_log");
        let log = log
            .into_iter()
            .map(event_converter::to_casper_event)
            .collect();
        log_mem_step("after_convert_event_log");

        Ok((log, result_or_system_deploy_error, eval_result.mergeable))
    }

    /**
     * Evaluates System deploy (applicative errors are fatal)
     */
    pub async fn eval_system_deploy<S: SystemDeployTrait>(
        &mut self,
        system_deploy: &mut S,
    ) -> Result<SysEvalResult<S>, CasperError> {
        let mem_profile_enabled = crate::rust::util::rholang::mem_profiler::mem_profile_enabled();
        let read_vm_rss_kb =
            || -> Option<usize> { crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() };
        let deploy_type = std::any::type_name::<S>();
        let mut rss_baseline = if mem_profile_enabled {
            read_vm_rss_kb()
        } else {
            None
        };
        let mut rss_prev = rss_baseline;
        let mut log_mem_step = |step: &str| {
            if !mem_profile_enabled {
                return;
            }
            if let Some(curr) = read_vm_rss_kb() {
                let prev = rss_prev.unwrap_or(curr);
                let baseline = rss_baseline.unwrap_or(curr);
                eprintln!(
                    "eval_system_deploy.mem deploy_type={} step={} rss_kb={} delta_prev_kb={} delta_total_kb={}",
                    deploy_type,
                    step,
                    curr,
                    curr as i64 - prev as i64,
                    curr as i64 - baseline as i64
                );
                rss_prev = Some(curr);
                if rss_baseline.is_none() {
                    rss_baseline = Some(curr);
                }
            }
        };
        let wrapper_pre_start = Instant::now();
        log_mem_step("start");

        // println!("\nEvaluating system deploy, {:?}", S::source());
        let wrapper_pre = wrapper_pre_start.elapsed();
        let eval_result = self.evaluate_system_source(system_deploy).await?;
        log_mem_step("after_evaluate_system_source");

        // println!("\nEval result: {:?}", eval_result);

        let wrapper_mid_start = Instant::now();
        if !eval_result.errors.is_empty() {
            return Err(CasperError::SystemRuntimeError(
                SystemDeployPlatformFailure::UnexpectedSystemErrors(eval_result.errors),
            ));
        }
        log_mem_step("after_error_check");

        log_mem_step("before_consume_system_result");
        let wrapper_mid = wrapper_mid_start.elapsed();
        let consumed = self.consume_system_result(system_deploy).await?;
        let wrapper_post_start = Instant::now();
        log_mem_step("after_consume_system_result");
        let r = match consumed {
            Some((_, vec_list)) => match vec_list.as_slice() {
                [ListParWithRandom { pars, .. }] if pars.len() == 1 => {
                    let extracted = system_deploy.extract_result(&pars[0]);
                    log_mem_step("after_extract_result");
                    Ok(extracted)
                }
                _ => Err(CasperError::SystemRuntimeError(
                    SystemDeployPlatformFailure::UnexpectedResult(
                        vec_list.iter().flat_map(|lp| lp.pars.clone()).collect(),
                    ),
                )),
            },
            None => Err(CasperError::SystemRuntimeError(
                SystemDeployPlatformFailure::ConsumeFailed,
            )),
        }?;
        log_mem_step("after_match_result");
        metrics::counter!(EVAL_SYSTEM_DEPLOY_WRAPPER_CALLS_METRIC, "source" => CASPER_METRICS_SOURCE)
            .increment(1);
        metrics::counter!(EVAL_SYSTEM_DEPLOY_WRAPPER_TIME_NS_METRIC, "source" => CASPER_METRICS_SOURCE)
            .increment(
                (wrapper_pre + wrapper_mid + wrapper_post_start.elapsed()).as_nanos() as u64,
            );

        Ok((r, eval_result))
    }

    /**
     * Evaluates exploratory (read-only) deploy
     */
    pub async fn play_exploratory_deploy(
        &mut self,
        term: String,
        hash: &StateHash,
    ) -> Result<(Vec<Par>, u64), CasperError> {
        let deploy_result = async {
            let deploy = construct_deploy::source_deploy(
                term,
                0,
                // Hardcoded phlogiston limit / 1 REV if phloPrice=1
                Some(100 * 1000 * 1000),
                None,
                Some(construct_deploy::DEFAULT_SEC.clone()),
                None,
                None,
            )?;

            // Create return channel as first private name created in deploy term
            let mut rand = Tools::unforgeable_name_rng(&deploy.pk, deploy.data.time_stamp);
            let return_name = Par::default().with_unforgeables(vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                    id: rand.next().into_iter().map(|b| b as u8).collect(),
                })),
            }]);

            // Execute deploy on top of specified block hash
            self.capture_results_with_name(hash, &deploy, &return_name)
                .await
        };

        deploy_result.await
    }

    async fn play_exploratory_par(
        &mut self,
        par: Par,
        hash: &StateHash,
    ) -> Result<Vec<Par>, CasperError> {
        use crate::rust::metrics_constants::{
            BONDS_CACHE_GET_DATA_TIME_METRIC, BONDS_CACHE_INJ_TIME_METRIC,
            BONDS_CACHE_RESET_TIME_METRIC, CASPER_METRICS_SOURCE,
        };
        let __reset_start = std::time::Instant::now();
        let mem_profile_enabled = crate::rust::util::rholang::mem_profiler::mem_profile_enabled();
        let read_vm_rss_kb =
            || -> Option<usize> { crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() };
        let mut rss_baseline = if mem_profile_enabled {
            read_vm_rss_kb()
        } else {
            None
        };
        let mut rss_prev = rss_baseline;
        let mut log_mem_step = |step: &str| {
            if !mem_profile_enabled {
                return;
            }
            if let Some(curr) = read_vm_rss_kb() {
                let prev = rss_prev.unwrap_or(curr);
                let baseline = rss_baseline.unwrap_or(curr);
                eprintln!(
                    "play_exploratory_par.mem step={} rss_kb={} delta_prev_kb={} delta_total_kb={}",
                    step,
                    curr,
                    curr as i64 - prev as i64,
                    curr as i64 - baseline as i64
                );
                rss_prev = Some(curr);
                if rss_baseline.is_none() {
                    rss_baseline = Some(curr);
                }
            }
        };
        log_mem_step("start");

        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(hash))
            .await?;
        log_mem_step("after_reset");
        self.runtime.cost().set(Cost::unsafe_max());
        log_mem_step("after_set_cost");
        metrics::histogram!(BONDS_CACHE_RESET_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(__reset_start.elapsed().as_secs_f64());

        let rand = Blake2b512Random::create_from_bytes(&[0u8; 128]);
        let mut return_rand = rand.clone();
        let return_name = Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                id: return_rand.next().into_iter().map(|b| b as u8).collect(),
            })),
        }]);
        log_mem_step("after_build_return_name");

        let __inj_start = std::time::Instant::now();
        let result = match self.runtime.inj(par, Env::new(), rand).await {
            Ok(()) => {
                log_mem_step("after_inj_ok");
                metrics::histogram!(BONDS_CACHE_INJ_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .record(__inj_start.elapsed().as_secs_f64());
                let __get_data_start = std::time::Instant::now();
                let data = self.get_data_par(&return_name).await;
                metrics::histogram!(BONDS_CACHE_GET_DATA_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .record(__get_data_start.elapsed().as_secs_f64());
                log_mem_step("after_get_data_par");
                Ok(data)
            }
            Err(err) => {
                metrics::histogram!(BONDS_CACHE_INJ_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
                    .record(__inj_start.elapsed().as_secs_f64());
                log_mem_step("after_inj_err");
                tracing::error!("Error in play_exploratory_par: {:?}", err);
                Ok(Vec::new())
            }
        };

        let _ = self.runtime.take_event_log().await;
        log_mem_step("after_take_event_log");
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(hash))
            .await?;
        log_mem_step("after_post_query_reset");

        result
    }

    /* Checkpoints */

    /**
     * Creates soft checkpoint with rollback if result is false.
     */
    pub async fn with_soft_transaction<A, F, Fut>(&mut self, action: F) -> Result<A, CasperError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(A, bool), CasperError>>,
    {
        let fallback = self.runtime.create_soft_checkpoint().await;

        // Execute action
        let (a, success) = action().await?;

        // Revert the state if failed
        if !success {
            self.runtime.revert_to_soft_checkpoint(fallback).await;
        }

        Ok(a)
    }

    /* Evaluates and captures results */

    // Return channel on which result is captured is the first name
    // in the deploy term `new return in { return!(42) }`
    pub async fn capture_results(
        &mut self,
        start: &StateHash,
        deploy: &Signed<DeployData>,
    ) -> Result<Vec<Par>, CasperError> {
        // Create return channel as first unforgeable name created in deploy term
        let mut rand = Tools::unforgeable_name_rng(&deploy.pk, deploy.data.time_stamp);
        let return_name = Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                id: rand.next().into_iter().map(|b| b as u8).collect(),
            })),
        }]);

        let (data, _token_cost) = self
            .capture_results_with_name(start, deploy, &return_name)
            .await?;
        Ok(data)
    }

    pub async fn capture_results_with_name(
        &mut self,
        start: &StateHash,
        deploy: &Signed<DeployData>,
        name: &Par,
    ) -> Result<(Vec<Par>, u64), CasperError> {
        match self.capture_results_with_errors(start, deploy, name).await {
            Ok(result) => Ok(result),
            Err(err) => Err(CasperError::InterpreterError(
                InterpreterError::BugFoundError(format!(
                    "Unexpected error while capturing results from Rholang: {}",
                    err
                )),
            )),
        }
    }

    pub async fn capture_results_with_errors(
        &mut self,
        start: &StateHash,
        deploy: &Signed<DeployData>,
        name: &Par,
    ) -> Result<(Vec<Par>, u64), CasperError> {
        self.runtime
            .reset(&Blake2b256Hash::from_bytes_prost(start))
            .await?;

        let eval_res = self.evaluate(deploy).await?;
        if !eval_res.errors.is_empty() {
            return Err(CasperError::InterpreterError(eval_res.errors[0].clone()));
        }

        let cost = eval_res.cost.value.max(0) as u64;
        Ok((self.get_data_par(name).await, cost))
    }

    /* Evaluates Rholang source code */

    /// Legacy single-signature evaluate. Preserves byte-identical
    /// observable behavior for existing on-chain deploys (same `deploy_id`,
    /// same `Sig::Hash` value, same normalizer env). Multi-signature
    /// dispatch happens in [`Self::evaluate_cosigned`] which this
    /// method delegates to via legacy uplift.
    pub async fn evaluate(
        &mut self,
        deploy: &Signed<DeployData>,
    ) -> Result<EvaluateResult, CasperError> {
        let cosigned = crypto::rust::signatures::signed::Cosigned::from_single_signer(
            deploy.clone(),
            deploy.data.phlo_limit,
        )
        .map_err(|e| {
            CasperError::RuntimeError(format!("legacy uplift to Cosigned failed in evaluate: {e}"))
        })?;
        self.evaluate_cosigned(&cosigned).await
    }

    /// Multi-signature aware deploy evaluation. Single source of truth for
    /// the signature install + normalizer-env construction logic.
    ///
    /// Single-sig deploys (`!cosigned.is_compound()`) route through the
    /// legacy `set_deploy_signature` (legacy `DEPLOY_SIGNATURE_DOMAIN`) so
    /// existing on-chain deploy_ids are preserved bit-for-bit. Multi-sig
    /// deploys route through `set_deploy_signatures` (compound domain
    /// separator) folding all signers into a left-associated `Sig::And` tree.
    ///
    /// The normalizer env is built via `normalizer_env_from_cosigned_deploy`
    /// in both cases — for single-sig that produces a one-element
    /// `rho:system:cosigners` list, observably equivalent to the legacy
    /// `normalizer_env_from_deploy(signed)` output (Cosigned uplift
    /// equivalence verified by
    /// `cosigned_envelope_legacy_uplift_yields_single_element_cosigners`).
    pub async fn evaluate_cosigned(
        &mut self,
        cosigned: &crypto::rust::signatures::signed::Cosigned<DeployData>,
    ) -> Result<EvaluateResult, CasperError> {
        let deploy_data = SystemProcessDeployData::from_cosigned(cosigned);
        self.runtime.set_deploy_data(deploy_data).await;
        self.runtime.cost.set_unmetered(false);

        if cosigned.is_compound() {
            // Multi-sig: fold all signatures into Sig::And, derive
            // compound-domain deploy_id from canonical-order signer set.
            let sigs: Vec<&[u8]> =
                cosigned.signers().iter().map(|s| s.sig.as_ref()).collect();
            self.runtime.cost.set_deploy_signatures(&sigs);
        } else {
            // Legacy single-sig path — byte-identical deploy_id to existing
            // on-chain deploys (legacy DEPLOY_SIGNATURE_DOMAIN).
            self.runtime
                .cost
                .set_deploy_signature(&cosigned.primary().sig);
        }

        let primary = cosigned.primary();
        let result = self
            .runtime
            .evaluate(
                &cosigned.data.term,
                Cost::create(cosigned.data.phlo_limit, "Evaluate deploy".to_string()),
                models::rust::normalizer_env::normalizer_env_from_cosigned_deploy(cosigned),
                Tools::unforgeable_name_rng(&primary.pk, cosigned.data.time_stamp),
            )
            .await;

        match result {
            Ok(eval_result) => Ok(eval_result),
            Err(e) => Err(CasperError::InterpreterError(e)),
        }
    }

    pub async fn evaluate_system_source<S: SystemDeployTrait>(
        &mut self,
        system_deploy: &mut S,
    ) -> Result<EvaluateResult, CasperError> {
        let mem_profile_enabled = crate::rust::util::rholang::mem_profiler::mem_profile_enabled();
        let read_vm_rss_kb =
            || -> Option<usize> { crate::rust::util::rholang::mem_profiler::read_vm_rss_kb() };
        let deploy_type = std::any::type_name::<S>();
        let mut rss_baseline = if mem_profile_enabled {
            read_vm_rss_kb()
        } else {
            None
        };
        let mut rss_prev = rss_baseline;
        let mut log_mem_step = |step: &str| {
            if !mem_profile_enabled {
                return;
            }
            if let Some(curr) = read_vm_rss_kb() {
                let prev = rss_prev.unwrap_or(curr);
                let baseline = rss_baseline.unwrap_or(curr);
                eprintln!(
                    "evaluate_system_source.mem deploy_type={} step={} rss_kb={} delta_prev_kb={} delta_total_kb={}",
                    deploy_type,
                    step,
                    curr,
                    curr as i64 - prev as i64,
                    curr as i64 - baseline as i64
                );
                rss_prev = Some(curr);
                if rss_baseline.is_none() {
                    rss_baseline = Some(curr);
                }
            }
        };
        log_mem_step("start");

        // Using tracing events for async - Span[F].traceI("evaluate-system-source") from Scala
        tracing::debug!(target: "f1r3fly.casper.evaluate-system-source", "evaluate-system-source-started");
        let eval_start = Instant::now();
        let wrapper_pre_start = eval_start;
        log_mem_step("before_build_env");
        let env = system_deploy.env();
        log_mem_step("after_build_env");
        let rand = system_deploy.rand().clone();
        log_mem_step("after_clone_rand");
        log_mem_step("before_runtime_evaluate");
        let wrapper_pre = wrapper_pre_start.elapsed();
        let result = {
            // System deploys perform protocol maintenance and settlement work
            // outside user-runtime metering. The scoped guard is deliberately
            // used here so panics, early returns, and async errors cannot leak
            // unmetered mode into the next user deploy.
            let _unmetered_scope = self.runtime.cost.enter_unmetered_scope();
            self.runtime
                .evaluate(
                    S::source(),
                    Cost::unsafe_max(),
                    env,
                    // `evaluate` owns the random seed state for this run, so the
                    // cloned deploy seed is passed by value with the rest of the
                    // immutable system-deploy inputs.
                    rand,
                )
                .await
        };
        let result = result?;
        let wrapper_post_start = Instant::now();
        log_mem_step("after_runtime_evaluate");
        metrics::histogram!(BLOCK_REPLAY_SYSDEPLOY_EVAL_EVALUATE_SOURCE_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(eval_start.elapsed().as_secs_f64());
        metrics::counter!(EVALUATE_SOURCE_WRAPPER_CALLS_METRIC, "source" => CASPER_METRICS_SOURCE)
            .increment(1);
        metrics::counter!(EVALUATE_SOURCE_WRAPPER_TIME_NS_METRIC, "source" => CASPER_METRICS_SOURCE)
            .increment((wrapper_pre + wrapper_post_start.elapsed()).as_nanos() as u64);
        Ok(result)
    }

    pub async fn get_data_par(&self, channel: &Par) -> Vec<Par> {
        self.runtime
            .get_data(channel)
            .await
            .into_iter()
            .flat_map(|datum| datum.a.pars)
            .collect()
    }

    pub async fn get_continuation_par(&self, channels: Vec<Par>) -> Vec<(Vec<BindPattern>, Par)> {
        self.runtime
            .get_continuations(channels)
            .await
            .into_iter()
            .filter_map(|wk| {
                if let Some(TaggedCont::ParBody(par_body)) = wk.continuation.tagged_cont {
                    Some((wk.patterns, par_body.body.unwrap()))
                } else {
                    None
                }
            })
            .collect()
    }

    pub async fn consume_result(
        &mut self,
        channel: Par,
        pattern: BindPattern,
    ) -> Result<Option<(TaggedContinuation, Vec<ListParWithRandom>)>, CasperError> {
        Ok(self
            .runtime
            .consume_result(vec![channel], vec![pattern])
            .await?)
    }

    pub async fn consume_system_result<S: SystemDeployTrait>(
        &mut self,
        system_deploy: &mut S,
    ) -> Result<Option<(TaggedContinuation, Vec<ListParWithRandom>)>, CasperError> {
        let consume_start = Instant::now();
        let return_channel = system_deploy.return_channel()?;
        let result = self
            .consume_result(return_channel, system_deploy_consume_all_pattern())
            .await;
        metrics::histogram!(BLOCK_REPLAY_SYSDEPLOY_EVAL_CONSUME_RESULT_TIME_METRIC, "source" => CASPER_METRICS_SOURCE)
            .record(consume_start.elapsed().as_secs_f64());
        result
    }

    /* Read only Rholang evaluator helpers */

    pub async fn get_active_validators(
        &mut self,
        start_hash: &StateHash,
    ) -> Result<Vec<Validator>, CasperError> {
        let validators_pars = self
            .play_exploratory_par(Self::activate_validator_query_par().clone(), start_hash)
            .await?;

        if validators_pars.is_empty() {
            tracing::warn!(
                "No result from getActiveValidators query for state {}; treating as no active validators",
                PrettyPrinter::build_string_bytes(start_hash)
            );
            return Ok(Vec::new());
        }

        if validators_pars.len() != 1 {
            return Err(CasperError::RuntimeError(format!(
                "Incorrect number of results from query of current bonds in state {}: {}",
                PrettyPrinter::build_string_bytes(start_hash),
                validators_pars.len()
            )));
        }

        let validators = Self::to_validator_vec(validators_pars[0].to_owned())?;
        let vlds: Vec<String> = validators.iter().map(|v| hex::encode(v)).collect();
        tracing::info!(
            "*** ACTIVE VALIDATORS FOR StateHash {}: {}",
            hex::encode(start_hash),
            vlds.join("\n")
        );

        Ok(validators)
    }

    pub async fn compute_bonds(&mut self, hash: &StateHash) -> Result<Vec<Bond>, CasperError> {
        let bonds_pars = self
            .play_exploratory_par(Self::bonds_query_par().clone(), hash)
            .await?;

        if bonds_pars.is_empty() {
            tracing::warn!(
                "No result from getBonds query for state {}; treating as empty bonds",
                PrettyPrinter::build_string_bytes(hash)
            );
            return Ok(Vec::new());
        }

        if bonds_pars.len() != 1 {
            return Err(CasperError::RuntimeError(format!(
                "Incorrect number of results from query of current bonds in state {}: {}",
                PrettyPrinter::build_string_bytes(hash),
                bonds_pars.len()
            )));
        }

        Self::to_bond_vec(bonds_pars[0].to_owned())
    }

    fn activate_validator_query_source() -> String {
        r#"
          new return, rl(`rho:registry:lookup`), poSCh in {
          rl!(`rho:system:pos`, *poSCh) |
          for(@(_, PoS) <- poSCh) {
            @PoS!("getActiveValidators", *return)
          }
        }
      "#
        .to_string()
    }

    fn activate_validator_query_par() -> &'static Par {
        static QUERY: OnceLock<Par> = OnceLock::new();
        QUERY.get_or_init(|| {
            Compiler::source_to_adt(&Self::activate_validator_query_source())
                .expect("Failed to compile active validator query source")
        })
    }

    fn bonds_query_source() -> String {
        r#"
        new return, rl(`rho:registry:lookup`), poSCh in {
          rl!(`rho:system:pos`, *poSCh) |
          for(@(_, PoS) <- poSCh) {
            @PoS!("getBonds", *return)
          }
        }
      "#
        .to_string()
    }

    fn bonds_query_par() -> &'static Par {
        static QUERY: OnceLock<Par> = OnceLock::new();
        QUERY.get_or_init(|| {
            Compiler::source_to_adt(&Self::bonds_query_source())
                .expect("Failed to compile bonds query source")
        })
    }

    fn to_validator_vec(validators_par: Par) -> Result<Vec<Validator>, CasperError> {
        if validators_par.exprs.is_empty() {
            return Ok(Vec::new());
        }

        let ps = match validators_par.exprs[0].expr_instance.as_ref().unwrap() {
            ExprInstance::ESetBody(set) => ParSetTypeMapper::eset_to_par_set(set.clone()).ps,
            _ => SortedParHashSet::create_from_empty(),
        };

        ps.map_iter(|v| {
            if v.exprs.len() != 1 {
                Err(CasperError::RuntimeError(
                    "Validator in bonds map wasn't a single string.".to_string(),
                ))
            } else {
                match v.exprs[0].expr_instance.as_ref().unwrap() {
                    ExprInstance::GByteArray(g_byte_array) => Ok(g_byte_array.clone().into()),
                    _ => Err(CasperError::RuntimeError(
                        "Expected GByteArray in validator data".to_string(),
                    )),
                }
            }
        })
        .collect::<Result<Vec<_>, _>>()
    }

    fn to_bond_vec(bonds_map: Par) -> Result<Vec<Bond>, CasperError> {
        if bonds_map.exprs.is_empty() {
            return Ok(Vec::new());
        }

        let ps = match bonds_map.exprs[0].expr_instance.as_ref().unwrap() {
            ExprInstance::EMapBody(map) => ParMapTypeMapper::emap_to_par_map(map.clone()).ps,
            _ => SortedParMap::create_from_empty(),
        };

        ps.map_iter(|(validator, bond)| {
            if validator.exprs.len() != 1 {
                Err(CasperError::RuntimeError(
                    "Validator in bonds map wasn't a single string.".to_string(),
                ))
            } else if bond.exprs.len() != 1 {
                Err(CasperError::RuntimeError(
                    "Stake in bonds map wasn't a single string.".to_string(),
                ))
            } else {
                let validator_name = match validator.exprs[0].expr_instance.as_ref().unwrap() {
                    ExprInstance::GByteArray(g_byte_array) => Ok(g_byte_array.clone().into()),
                    _ => Err(CasperError::RuntimeError(
                        "Expected GByteArray in validator data".to_string(),
                    )),
                }?;

                let stake_amount = match bond.exprs[0].expr_instance.as_ref().unwrap() {
                    ExprInstance::GInt(g_int) => Ok(*g_int),
                    _ => Err(CasperError::RuntimeError(
                        "Expected GInt in stake data".to_string(),
                    )),
                }?;

                Ok(Bond {
                    validator: validator_name,
                    stake: stake_amount,
                })
            }
        })
        .collect::<Result<Vec<_>, _>>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_multi_value_empty_returns_none() {
        assert_eq!(
            RuntimeOps::fold_multi_value(&[], MergeType::IntegerAdd),
            None
        );
        assert_eq!(
            RuntimeOps::fold_multi_value(&[], MergeType::BitmaskOr),
            None
        );
    }

    #[test]
    fn fold_multi_value_single_returns_value() {
        assert_eq!(
            RuntimeOps::fold_multi_value(&[42], MergeType::IntegerAdd),
            Some(42)
        );
        assert_eq!(
            RuntimeOps::fold_multi_value(&[42], MergeType::BitmaskOr),
            Some(42)
        );
    }

    #[test]
    fn fold_multi_value_integer_add_returns_max() {
        // Vault-balance semantics: pick the largest observed value.
        assert_eq!(
            RuntimeOps::fold_multi_value(&[10, 5, 20, 15], MergeType::IntegerAdd),
            Some(20)
        );
    }

    #[test]
    fn fold_multi_value_bitmask_or_returns_or_fold_not_max() {
        // BitmaskOr must OR-fold all bitmaps; using max() would silently lose
        // bits set only in non-max values.
        let a = 0b00010001i64; // bits {0, 4}
        let b = 0b00100010i64; // bits {1, 5}
                               // max(a, b) = b = 0b00100010 — would lose bits {0, 4}.
                               // OR fold = 0b00110011 — bits {0, 1, 4, 5}. Correct.
        assert_eq!(
            RuntimeOps::fold_multi_value(&[a, b], MergeType::BitmaskOr),
            Some(0b00110011),
        );
        // Three-way fold sanity.
        let c = 0b01000000i64;
        assert_eq!(
            RuntimeOps::fold_multi_value(&[a, b, c], MergeType::BitmaskOr),
            Some(0b01110011),
        );
    }

    #[test]
    fn fold_multi_value_bitmask_or_commutes() {
        // Result must not depend on observation order.
        let xs = [0b0001_0001i64, 0b0010_0010, 0b0100_0100, 0b1000_1000];
        let mut ys = xs;
        ys.reverse();
        assert_eq!(
            RuntimeOps::fold_multi_value(&xs, MergeType::BitmaskOr),
            RuntimeOps::fold_multi_value(&ys, MergeType::BitmaskOr),
        );
    }

    #[test]
    fn fold_multi_value_bitmask_or_negative_high_bits_preserved() {
        // i64::MIN sets only the sign bit (bit 63). OR with a positive bitmap
        // must keep bit 63 set — no narrowing or sign-extension surprise.
        let neg = i64::MIN;
        let pos = 0b1010i64;
        let folded = RuntimeOps::fold_multi_value(&[neg, pos], MergeType::BitmaskOr).unwrap();
        assert_eq!(folded as u64, (neg as u64) | (pos as u64));
        assert_ne!(folded & i64::MIN, 0, "sign bit must remain set");
    }
}
