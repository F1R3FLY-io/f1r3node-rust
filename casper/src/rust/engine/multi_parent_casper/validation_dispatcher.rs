//! Validation dispatch — `validate`, `validate_self_created`,
//! `handle_invalid_block`.
//!
//! Phase 3 Step 4 — extracted from `engine::multi_parent_casper`. Each
//! function takes the casper instance as a `&MultiParentCasperImpl<T>`
//! reference (rather than `&self`) so the implementation can live in
//! this module while the trait method is a one-line delegate in
//! `traits.rs`.

use std::collections::{BTreeMap, BTreeSet};

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, CertifiedAdmissionOutcome, CertifiedSenderAuthority, InsertMode,
    KeyValueDagRepresentation,
};
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHashSerde;
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::equivocation_record::EquivocationRecord;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::Either;

use super::snapshot::record_dag_cardinality_metrics;
use super::types::MultiParentCasperImpl;
use crate::rust::block_status::{BlockError, CertifiedBlockValidation, InvalidBlock, ValidBlock};
use crate::rust::casper::CasperSnapshot;
use crate::rust::equivocation_detector::EquivocationDetector;
use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    BLOCK_VALIDATION_STEP_BLOCK_SUMMARY_TIME_METRIC, BLOCK_VALIDATION_STEP_BONDS_CACHE_TIME_METRIC,
    BLOCK_VALIDATION_STEP_CHECKPOINT_TIME_METRIC,
    BLOCK_VALIDATION_STEP_FLOOR_AUTHORITY_TIME_METRIC,
    BLOCK_VALIDATION_STEP_NEGLECTED_INVALID_BLOCK_TIME_METRIC,
    BLOCK_VALIDATION_STEP_PRE_STATE_TIME_METRIC,
    BLOCK_VALIDATION_STEP_SIMPLE_EQUIVOCATION_TIME_METRIC,
    BLOCK_VALIDATION_STEP_SLASH_AUTHORIZATION_TIME_METRIC, CASPER_METRICS_SOURCE,
};
use crate::rust::slashing_authorization::{checked_base_seq, CanonicalSlashAuthority};
use crate::rust::util::proto_util;
use crate::rust::util::rholang::interpreter_util::{
    replay_validated_block_checkpoint, validate_block_pre_state,
};
use crate::rust::validate::Validate;

async fn timed_step<A, Fut>(
    step_name: &'static str,
    metric_name: &'static str,
    future: Fut,
) -> Result<(Either<BlockError, A>, String), CasperError>
where
    Fut: std::future::Future<Output = Result<Either<BlockError, A>, CasperError>>,
{
    tracing::debug!(target: "f1r3fly.casper", "before-{}", step_name);
    let start = std::time::Instant::now();
    let result = future.await?;
    let elapsed = start.elapsed();
    let elapsed_str = format!("{:?}", elapsed);
    let step_time_seconds = elapsed.as_secs_f64();
    metrics::histogram!(metric_name, "source" => CASPER_METRICS_SOURCE).record(step_time_seconds);
    tracing::debug!(target: "f1r3fly.casper", "after-{}", step_name);
    Ok((result, elapsed_str))
}

async fn timed_operation<A, Fut>(
    step_name: &'static str,
    metric_name: &'static str,
    future: Fut,
) -> Result<(A, String), CasperError>
where
    Fut: std::future::Future<Output = Result<A, CasperError>>,
{
    tracing::debug!(target: "f1r3fly.casper", "before-{}", step_name);
    let start = std::time::Instant::now();
    let result = future.await?;
    let elapsed = start.elapsed();
    metrics::histogram!(metric_name, "source" => CASPER_METRICS_SOURCE)
        .record(elapsed.as_secs_f64());
    tracing::debug!(target: "f1r3fly.casper", "after-{}", step_name);
    Ok((result, format!("{:?}", elapsed)))
}

/// Returns the outcome of `check_equivocations`. A `Left` from any
/// intermediate validator short-circuits and is returned directly.
async fn run_validation_steps<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    block: &BlockMessage,
    snapshot: &mut CasperSnapshot,
) -> Result<CertifiedBlockValidation, CasperError> {
    if let Either::Left(BlockError::Invalid(InvalidBlock::InvalidBlockHash)) =
        Validate::block_hash(block)
    {
        return Ok(CertifiedBlockValidation::unattributable(
            InvalidBlock::InvalidBlockHash,
        ));
    }

    let (baseline_authority_result, t_authority_baseline) = timed_step(
        "authority-baseline",
        BLOCK_VALIDATION_STEP_FLOOR_AUTHORITY_TIME_METRIC,
        async {
            let authority_floor =
                crate::rust::causal_equivocation::incoming_finalized_floor(
                    &snapshot.dag,
                    &block.header.parents_hash_list,
                )
                .unwrap_or_else(|_| snapshot.dag.last_finalized_block_hash.clone());
            let context = match crate::rust::causal_equivocation::CertifiedConsensusContext::for_authority_floor_baseline(
                &snapshot.dag,
                authority_floor,
            ) {
                Ok(context) => context,
                Err(error) => {
                    return Ok(Either::Left(BlockError::BlockException(error)));
                }
            };
            match context.certify_sender(block) {
                Ok(certificate) => Ok(Either::Right(certificate)),
                Err(_) => Ok(Either::Left(BlockError::Invalid(
                    InvalidBlock::InvalidSender,
                ))),
            }
        },
    )
    .await?;
    let baseline_authority = match baseline_authority_result {
        Either::Right(certificate) => certificate,
        Either::Left(BlockError::Invalid(invalid)) => {
            return Ok(CertifiedBlockValidation::unattributable(invalid));
        }
        Either::Left(error) => {
            return CertifiedBlockValidation::from_uncertified_error(error);
        }
    };

    let (block_summary_result, t1) = timed_step(
        "block-summary",
        BLOCK_VALIDATION_STEP_BLOCK_SUMMARY_TIME_METRIC,
        async {
            Ok(Validate::block_summary(
                block,
                &this.approved_block,
                snapshot,
                &this.casper_shard_conf.shard_name,
                this.casper_shard_conf.deploy_lifespan as i32,
                this.casper_shard_conf.max_number_of_parents,
                this.casper_shard_conf.max_parent_depth,
                this.casper_shard_conf.mergeable_channels_gc_depth_buffer,
                &this.block_store,
                this.casper_shard_conf.disable_validator_progress_check,
            )
            .await)
        },
    )
    .await?;
    tracing::debug!(target: "f1r3fly.casper", "post-validation-block-summary");
    if let Either::Left(block_error) = block_summary_result {
        return CertifiedBlockValidation::certified(
            block,
            Either::Left(block_error),
            baseline_authority,
        );
    }

    let (floor_authority_result, t_floor_authority) = timed_step(
        "floor-authority",
        BLOCK_VALIDATION_STEP_FLOOR_AUTHORITY_TIME_METRIC,
        async {
            let exact_latest_messages = block
                .justifications
                .iter()
                .map(|justification| {
                    (
                        justification.validator.clone(),
                        justification.latest_block_hash.clone(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if exact_latest_messages.len() != block.justifications.len() {
                return Ok(Either::Left(BlockError::Invalid(
                    InvalidBlock::InvalidFollows,
                )));
            }
            let context =
                match crate::rust::causal_equivocation::CertifiedConsensusContext::for_candidate(
                    &snapshot.dag,
                    &block.header.parents_hash_list,
                    &exact_latest_messages,
                    crate::rust::safety::clique_oracle::FtThreshold::from_ppm(
                        snapshot
                            .on_chain_state
                            .shard_conf
                            .fault_tolerance_threshold_ppm,
                    ),
                )
                .await
                {
                    Ok(context) => context,
                    Err(error) => {
                        return Ok(Either::Left(BlockError::BlockException(error)));
                    }
                };
            if !context.has_complete_latest_message_slots() {
                return Ok(Either::Left(BlockError::Invalid(
                    InvalidBlock::InvalidFollows,
                )));
            }
            match context.certify_sender(block) {
                Ok(certificate) => Ok(Either::Right((context, certificate))),
                Err(_) => Ok(Either::Left(BlockError::Invalid(
                    InvalidBlock::InvalidSender,
                ))),
            }
        },
    )
    .await?;
    let (certified_context, sender_authority) = match floor_authority_result {
        Either::Left(block_error) => {
            return CertifiedBlockValidation::certified(
                block,
                Either::Left(block_error),
                baseline_authority,
            )
        }
        Either::Right(certified) => certified,
    };

    let (validate_pre_state_result, t_pre_state) = timed_step(
        "pre-state",
        BLOCK_VALIDATION_STEP_PRE_STATE_TIME_METRIC,
        validate_block_pre_state(
            block,
            &this.block_store,
            snapshot,
            &this.runtime_manager,
            Some(&this.rejected_deploy_buffer),
        ),
    )
    .await?;
    let pre_state_hash = match validate_pre_state_result {
        Either::Left(block_error) => {
            return CertifiedBlockValidation::certified(
                block,
                Either::Left(block_error),
                sender_authority,
            )
        }
        Either::Right(None) => {
            return CertifiedBlockValidation::certified(
                block,
                Either::Left(BlockError::Invalid(InvalidBlock::InvalidTransaction)),
                sender_authority,
            )
        }
        Either::Right(Some(pre_state_hash)) => pre_state_hash,
    };

    let (slash_authorization_result, t_slash) = timed_step(
        "slash-authorization",
        BLOCK_VALIDATION_STEP_SLASH_AUTHORIZATION_TIME_METRIC,
        async {
            let authority =
                match CanonicalSlashAuthority::load(&this.runtime_manager, &pre_state_hash).await {
                    Ok(authority) => authority,
                    Err(error) => {
                        return Ok(Either::Left(BlockError::BlockException(error)));
                    }
                };
            match Validate::slash_deploy_authorization(block, snapshot, &authority) {
                Either::Left(error) => Ok(Either::Right((authority, Some(error)))),
                Either::Right(_) => Ok(Either::Right((authority, None))),
            }
        },
    )
    .await?;
    let pre_state_slash_authority = match slash_authorization_result {
        Either::Left(block_error) => {
            return CertifiedBlockValidation::certified(
                block,
                Either::Left(block_error),
                sender_authority,
            )
        }
        Either::Right((authority, slash_error)) => {
            if let Some(error) = slash_error {
                return CertifiedBlockValidation::certified(
                    block,
                    Either::Left(error),
                    sender_authority,
                );
            }
            authority
        }
    };

    if let Err(error) = certified_context.validate_certificate(block, &sender_authority) {
        return Ok(CertifiedBlockValidation::local_fault(
            CasperError::RuntimeError(error.to_string()),
        ));
    }

    match crate::rust::causal_equivocation::validate_evidence_delta(block, &snapshot.dag)? {
        crate::rust::causal_equivocation::EvidenceDeltaVerdict::Valid => {}
        crate::rust::causal_equivocation::EvidenceDeltaVerdict::Neglected => {
            return CertifiedBlockValidation::certified(
                block,
                Either::Left(BlockError::Invalid(InvalidBlock::NeglectedEquivocation)),
                sender_authority,
            )
        }
        crate::rust::causal_equivocation::EvidenceDeltaVerdict::Invalid => {
            return CertifiedBlockValidation::certified(
                block,
                Either::Left(BlockError::Invalid(
                    InvalidBlock::InvalidEquivocationEvidence,
                )),
                sender_authority,
            )
        }
    }

    let (validate_block_checkpoint_result, t2) = timed_step(
        "checkpoint-replay",
        BLOCK_VALIDATION_STEP_CHECKPOINT_TIME_METRIC,
        replay_validated_block_checkpoint(block, snapshot, &this.runtime_manager, pre_state_hash),
    )
    .await?;
    tracing::debug!(target: "f1r3fly.casper", "transactions-validated");
    if let Either::Left(block_error) = validate_block_checkpoint_result {
        return CertifiedBlockValidation::certified(
            block,
            Either::Left(block_error),
            sender_authority,
        );
    }
    if let Either::Right(None) = validate_block_checkpoint_result {
        return CertifiedBlockValidation::certified(
            block,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidTransaction)),
            sender_authority,
        );
    }

    let (bonds_cache_result, t3) = timed_step(
        "bonds-cache",
        BLOCK_VALIDATION_STEP_BONDS_CACHE_TIME_METRIC,
        async { Ok(Validate::bonds_cache(block, &this.runtime_manager).await) },
    )
    .await?;
    tracing::debug!(target: "f1r3fly.casper", "bonds-cache-validated");
    if let Either::Left(block_error) = bonds_cache_result {
        return CertifiedBlockValidation::certified(
            block,
            Either::Left(block_error),
            sender_authority,
        );
    }

    let (neglected_invalid_block_result, t4) = timed_step(
        "neglected-invalid-block",
        BLOCK_VALIDATION_STEP_NEGLECTED_INVALID_BLOCK_TIME_METRIC,
        async {
            Ok(Validate::neglected_invalid_block(
                block,
                snapshot,
                &pre_state_slash_authority,
            ))
        },
    )
    .await?;
    tracing::debug!(target: "f1r3fly.casper", "neglected-invalid-block-validated");
    if let Either::Left(block_error) = neglected_invalid_block_result {
        return CertifiedBlockValidation::certified(
            block,
            Either::Left(block_error),
            sender_authority,
        );
    }

    // D3 (DR-9, D.5): the per-block `phlo-price` validation step is REMOVED —
    // deploys carry no phlo price/limit. Per-signature funding is settled at
    // block assembly by the acceptance gate (against Σ⟦s⟧); replay re-derives
    // the same settlement debits, and an over-admitting proposer underflows the
    // pool (a detectable invalid block) — no separate price rule is needed.

    let requested_as_dependency = this
        .casper_buffer_storage
        .requested_as_dependency(&BlockHashSerde(block.block_hash.clone()));
    let (equivocation_observation, t7) = timed_operation(
        "simple-equivocation-observation",
        BLOCK_VALIDATION_STEP_SIMPLE_EQUIVOCATION_TIME_METRIC,
        async {
            EquivocationDetector::check_equivocations(requested_as_dependency, block, &snapshot.dag)
                .await
                .map_err(CasperError::from)
        },
    )
    .await?;

    let timing_breakdown = format!(
        "authority-baseline={}, summary={}, floor-authority={}, pre-state={}, slash-authorization={}, checkpoint-replay={}, bonds={}, neglected-invalid={}, \
         evidence-delta=certified-context, equivocation-observation={}",
        t_authority_baseline, t1, t_floor_authority, t_pre_state, t_slash, t2, t3, t4, t7
    );
    tracing::debug!(target: "f1r3fly.casper", "Validation timing breakdown: {}", timing_breakdown);

    CertifiedBlockValidation::certified_with_observation(
        block,
        Either::Right(ValidBlock::Valid),
        sender_authority,
        equivocation_observation,
    )
}

async fn update_mergeable_cache_after_validation<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    block: &BlockMessage,
) {
    if this.casper_shard_conf.max_number_of_parents <= 1 {
        return;
    }

    let maybe_mergeable = this.runtime_manager.load_mergeable_channels(block);

    match maybe_mergeable {
        Ok(mergeable_chs) => {
            if let Err(err) = this.runtime_manager.get_or_compute_block_index(
                &block.block_hash,
                proto_util::block_number(block),
                &block.body.deploys,
                &block.body.system_deploys,
                &Blake2b256Hash::from_bytes_prost(&block.body.state.pre_state_hash),
                &Blake2b256Hash::from_bytes_prost(&block.body.state.post_state_hash),
                &mergeable_chs,
            ) {
                tracing::warn!(
                    "Skipping block index cache update for {} {}: {}",
                    "block",
                    PrettyPrinter::build_string_bytes(&block.block_hash),
                    err
                );
            }
        }
        Err(err) => {
            tracing::warn!(
                "Skipping mergeable/index cache update for {} {}: {}",
                "block",
                PrettyPrinter::build_string_bytes(&block.block_hash),
                err
            );
        }
    }
}

pub(crate) async fn dispatch_validate<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    block: &BlockMessage,
    snapshot: &mut CasperSnapshot,
) -> Result<CertifiedBlockValidation, CasperError> {
    tracing::info!(
        "Validating block {}",
        PrettyPrinter::build_string_block_message(block, true)
    );

    let start = std::time::Instant::now();
    let val_result = run_validation_steps(this, block, snapshot).await?;
    let elapsed = start.elapsed();

    if let Either::Right(status) = val_result.status() {
        let block_info = PrettyPrinter::build_string_block_message(block, true);
        let deploy_count = block.body.deploys.len();
        tracing::info!(
            "Block replayed: {} ({}d) ({:?}) [{:?}]",
            block_info,
            deploy_count,
            status,
            elapsed
        );
        update_mergeable_cache_after_validation(this, block).await;
    }

    Ok(val_result)
}

pub(crate) async fn dispatch_validate_self_created<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    block: &BlockMessage,
    snapshot: &mut CasperSnapshot,
    pre_state_hash: Bytes,
    post_state_hash: Bytes,
) -> Result<CertifiedBlockValidation, CasperError> {
    tracing::info!(
        "Validating self-created block {}",
        PrettyPrinter::build_string_block_message(block, true)
    );

    // Safety: verify the block carries the hashes we computed.
    if block.body.state.pre_state_hash != pre_state_hash {
        let msg = format!(
            "Self-created block pre_state_hash mismatch: expected={}, actual={}, block={}",
            PrettyPrinter::build_string_no_limit(&pre_state_hash),
            PrettyPrinter::build_string_no_limit(&block.body.state.pre_state_hash),
            PrettyPrinter::build_string_bytes(&block.block_hash),
        );
        tracing::error!(
            block_hash = %PrettyPrinter::build_string_bytes(&block.block_hash),
            expected = %PrettyPrinter::build_string_no_limit(&pre_state_hash),
            actual = %PrettyPrinter::build_string_no_limit(&block.body.state.pre_state_hash),
            "self-created block pre_state_hash mismatch"
        );
        return Ok(CertifiedBlockValidation::local_fault(
            CasperError::RuntimeError(msg),
        ));
    }
    if block.body.state.post_state_hash != post_state_hash {
        let msg = format!(
            "Self-created block post_state_hash mismatch: expected={}, actual={}, block={}",
            PrettyPrinter::build_string_no_limit(&post_state_hash),
            PrettyPrinter::build_string_no_limit(&block.body.state.post_state_hash),
            PrettyPrinter::build_string_bytes(&block.block_hash),
        );
        tracing::error!(
            block_hash = %PrettyPrinter::build_string_bytes(&block.block_hash),
            expected = %PrettyPrinter::build_string_no_limit(&post_state_hash),
            actual = %PrettyPrinter::build_string_no_limit(&block.body.state.post_state_hash),
            "self-created block post_state_hash mismatch"
        );
        return Ok(CertifiedBlockValidation::local_fault(
            CasperError::RuntimeError(msg),
        ));
    }

    dispatch_validate(this, block, snapshot).await
}

pub(crate) fn dispatch_handle_invalid_block<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    block: &BlockMessage,
    status: &InvalidBlock,
    dag: &KeyValueDagRepresentation,
    certificate: &CertifiedSenderAuthority,
    outcome: &CertifiedAdmissionOutcome,
) -> Result<KeyValueDagRepresentation, CasperError> {
    let handle_invalid_block_effect = |block_dag_storage: &BlockDagKeyValueStorage,
                                       casper_buffer_storage: &CasperBufferKeyValueStorage,
                                       status: &InvalidBlock,
                                       block: &BlockMessage,
                                       certificate: &CertifiedSenderAuthority,
                                       outcome: &CertifiedAdmissionOutcome|
     -> Result<KeyValueDagRepresentation, CasperError> {
        tracing::warn!(
            "Recording invalid block {} for {:?}.",
            PrettyPrinter::build_string_bytes(&block.block_hash),
            status
        );

        // Bug #17 / T-9.20: in-process atomic transition of the
        // (DAG insert, casper-buffer remove) pair via
        // `atomic_insert_then_buffer`. Distinct LMDB envs mean
        // cross-store ACID is physically impossible; the helper
        // documents the lock-order contract (DAG global_lock A,
        // buffer state_lock B) and the on-resume reconciliation
        // closes any crash-window drift. See
        // docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.20.
        let block_hash_serde = BlockHashSerde(block.block_hash.clone());
        let updated_dag =
            block_storage::rust::dag::buffer_dag_transition::atomic_insert_then_buffer(
                block_dag_storage,
                block,
                InsertMode::Invalid,
                certificate,
                outcome,
                casper_buffer_storage,
                block_storage::rust::dag::buffer_dag_transition::BufferTransition::RemoveFromBuffer(
                    block_hash_serde,
                ),
            )?;
        record_dag_cardinality_metrics(&updated_dag);
        Ok(updated_dag)
    };

    // Atomic read-modify-write on the equivocation tracker. See
    // docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.2.
    let record_evidence = |block_dag_storage: &BlockDagKeyValueStorage,
                           block: &BlockMessage,
                           bond_generation: BondGeneration|
     -> Result<(), CasperError> {
        // `checked_base_seq(block.seq_num)` returns `None` when
        // `seq_num <= 0`. The seq_num == 0 case is the genesis block: the
        // protocol disallows equivocation evidence against genesis (it has
        // no predecessor seqNum to base the EquivocationRecord on), and
        // any seq_num < 0 is a corrupted record that should not exist
        // post-validation. Skipping is correct in both cases — genesis is
        // special, and the negative case is already rejected upstream by
        // `validate_received_slash_deploys::NegativeSequenceNumber`.
        let Some(base_equivocation_block_seq_num) = checked_base_seq(block.seq_num) else {
            return Ok(());
        };
        block_dag_storage.access_equivocations_tracker(|tracker| {
            let equivocation_records = tracker.data()?;
            let record_exists = equivocation_records.iter().any(|record| {
                record.equivocator == block.sender
                    && record.equivocator_bond_generation == bond_generation
                    && record.equivocation_base_block_seq_num == base_equivocation_block_seq_num
            });
            if !record_exists {
                let new_equivocation_record = EquivocationRecord::new(
                    block.sender.clone(),
                    bond_generation,
                    base_equivocation_block_seq_num,
                    BTreeSet::new(),
                );
                tracker.add(new_equivocation_record)?;
            }
            Ok(())
        })?;
        Ok(())
    };

    match status {
        status if status.is_slashable() => {
            // Every slashable status mints an EquivocationRecord. See
            // docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.3.
            record_evidence(&this.block_dag_storage, block, certificate.generation())?;
            handle_invalid_block_effect(
                &this.block_dag_storage,
                &this.casper_buffer_storage,
                status,
                block,
                certificate,
                outcome,
            )
        }

        _ => {
            let block_hash_serde = BlockHashSerde(block.block_hash.clone());
            this.casper_buffer_storage.remove(block_hash_serde)?;
            tracing::warn!(
                "Recording invalid block {} for {:?}.",
                PrettyPrinter::build_string_bytes(&block.block_hash),
                status
            );
            Ok(dag.clone())
        }
    }
}
