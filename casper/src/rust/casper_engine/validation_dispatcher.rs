//! Validation dispatch — `validate`, `validate_self_created`,
//! `handle_invalid_block`.
//!
//! Phase 3 Step 4 — extracted from `multi_parent_casper_impl.rs`. Each
//! function takes the casper instance as a `&MultiParentCasperImpl<T>`
//! reference (rather than `&self`) so the implementation can live in
//! this module while the trait method is a one-line delegate in
//! `traits.rs`.

use std::collections::BTreeSet;

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, InsertMode, KeyValueDagRepresentation,
};
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::block_hash::BlockHashSerde;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::BlockMessage;
use models::rust::equivocation_record::EquivocationRecord;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::Either;

use crate::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use crate::rust::casper::CasperSnapshot;
use crate::rust::equivocation_detector::EquivocationDetector;
use crate::rust::errors::CasperError;
use crate::rust::metrics_constants::{
    BLOCK_VALIDATION_STEP_BLOCK_SUMMARY_TIME_METRIC, BLOCK_VALIDATION_STEP_BONDS_CACHE_TIME_METRIC,
    BLOCK_VALIDATION_STEP_CHECKPOINT_TIME_METRIC,
    BLOCK_VALIDATION_STEP_NEGLECTED_EQUIVOCATION_TIME_METRIC,
    BLOCK_VALIDATION_STEP_NEGLECTED_INVALID_BLOCK_TIME_METRIC,
    BLOCK_VALIDATION_STEP_PHLO_PRICE_TIME_METRIC,
    BLOCK_VALIDATION_STEP_SIMPLE_EQUIVOCATION_TIME_METRIC, CASPER_METRICS_SOURCE,
};
use crate::rust::slashing_authorization::checked_base_seq;
use crate::rust::util::rholang::interpreter_util::validate_block_checkpoint;
use crate::rust::validate::Validate;

use super::snapshot::record_dag_cardinality_metrics;
use super::types::MultiParentCasperImpl;

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
    metrics::histogram!(metric_name, "source" => CASPER_METRICS_SOURCE)
        .record(step_time_seconds);
    tracing::debug!(target: "f1r3fly.casper", "after-{}", step_name);
    Ok((result, elapsed_str))
}

pub(crate) async fn dispatch_validate<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    block: &BlockMessage,
    snapshot: &mut CasperSnapshot,
) -> Result<Either<BlockError, ValidBlock>, CasperError> {
    tracing::info!(
        "Validating block {}",
        PrettyPrinter::build_string_block_message(block, true)
    );

    let start = std::time::Instant::now();
    let val_result = {
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
                    &this.block_store,
                    this.casper_shard_conf.disable_validator_progress_check,
                )
                .await)
            },
        )
        .await?;
        tracing::debug!(target: "f1r3fly.casper", "post-validation-block-summary");
        if let Either::Left(block_error) = block_summary_result {
            return Ok(Either::Left(block_error));
        }

        let (validate_block_checkpoint_result, t2) = timed_step(
            "checkpoint",
            BLOCK_VALIDATION_STEP_CHECKPOINT_TIME_METRIC,
            validate_block_checkpoint(
                block,
                &this.block_store,
                snapshot,
                &mut *this.runtime_manager.lock().await,
            ),
        )
        .await?;
        tracing::debug!(target: "f1r3fly.casper", "transactions-validated");
        if let Either::Left(block_error) = validate_block_checkpoint_result {
            return Ok(Either::Left(block_error));
        }
        if let Either::Right(None) = validate_block_checkpoint_result {
            return Ok(Either::Left(BlockError::Invalid(
                InvalidBlock::InvalidTransaction,
            )));
        }

        let (bonds_cache_result, t3) = timed_step(
            "bonds-cache",
            BLOCK_VALIDATION_STEP_BONDS_CACHE_TIME_METRIC,
            async { Ok(Validate::bonds_cache(block, &*this.runtime_manager.lock().await).await) },
        )
        .await?;
        tracing::debug!(target: "f1r3fly.casper", "bonds-cache-validated");
        if let Either::Left(block_error) = bonds_cache_result {
            return Ok(Either::Left(block_error));
        }

        let (neglected_invalid_block_result, t4) = timed_step(
            "neglected-invalid-block",
            BLOCK_VALIDATION_STEP_NEGLECTED_INVALID_BLOCK_TIME_METRIC,
            async { Ok(Validate::neglected_invalid_block(block, snapshot)) },
        )
        .await?;
        tracing::debug!(target: "f1r3fly.casper", "neglected-invalid-block-validated");
        if let Either::Left(block_error) = neglected_invalid_block_result {
            return Ok(Either::Left(block_error));
        }

        let (equivocation_detector_result, t5) = timed_step(
            "neglected-equivocation",
            BLOCK_VALIDATION_STEP_NEGLECTED_EQUIVOCATION_TIME_METRIC,
            async {
                EquivocationDetector::check_neglected_equivocations_with_update(
                    block,
                    &snapshot.dag,
                    &this.block_store,
                    &this.approved_block,
                    &this.block_dag_storage,
                )
                .await
                .map_err(CasperError::from)
            },
        )
        .await?;
        tracing::debug!(target: "f1r3fly.casper", "neglected-equivocation-validated");
        if let Either::Left(block_error) = equivocation_detector_result {
            return Ok(Either::Left(block_error));
        }

        let (phlo_price_result, t6) = timed_step(
            "phlo-price",
            BLOCK_VALIDATION_STEP_PHLO_PRICE_TIME_METRIC,
            async {
                Ok(Validate::phlo_price(
                    block,
                    this.casper_shard_conf.min_phlo_price,
                ))
            },
        )
        .await?;
        tracing::debug!(target: "f1r3fly.casper", "phlogiston-price-validated");
        if let Either::Left(_) = phlo_price_result {
            tracing::warn!(
                "One or more deploys has phloPrice lower than {}",
                this.casper_shard_conf.min_phlo_price
            );
        }

        let requested_as_dependency = this
            .casper_buffer_storage
            .requested_as_dependency(&BlockHashSerde(block.block_hash.clone()));

        let (equivocation_result, t7) = timed_step(
            "simple-equivocation",
            BLOCK_VALIDATION_STEP_SIMPLE_EQUIVOCATION_TIME_METRIC,
            async {
                EquivocationDetector::check_equivocations(
                    requested_as_dependency,
                    block,
                    &snapshot.dag,
                )
                .await
                .map_err(CasperError::from)
            },
        )
        .await?;
        tracing::debug!(target: "f1r3fly.casper", "equivocation-validated");

        tracing::debug!(
            target: "f1r3fly.casper",
            "Validation timing breakdown: summary={}, checkpoint={}, bonds={}, neglected-invalid={}, neglected-equiv={}, phlo={}, simple-equiv={}",
            t1, t2, t3, t4, t5, t6, t7
        );

        equivocation_result
    };

    let elapsed = start.elapsed();

    if let Either::Right(ref status) = val_result {
        let block_info = PrettyPrinter::build_string_block_message(block, true);
        let deploy_count = block.body.deploys.len();
        tracing::info!(
            "Block replayed: {} ({}d) ({:?}) [{:?}]",
            block_info,
            deploy_count,
            status,
            elapsed
        );

        if this.casper_shard_conf.max_number_of_parents > 1 {
            let maybe_mergeable = this.runtime_manager.lock().await.load_mergeable_channels(
                &block.body.state.post_state_hash,
                block.sender.clone(),
                block.seq_num,
            );

            match maybe_mergeable {
                Ok(mergeable_chs) => {
                    if let Err(err) = this
                        .runtime_manager
                        .lock()
                        .await
                        .get_or_compute_block_index(
                            &block.block_hash,
                            &block.body.deploys,
                            &block.body.system_deploys,
                            &Blake2b256Hash::from_bytes_prost(&block.body.state.pre_state_hash),
                            &Blake2b256Hash::from_bytes_prost(&block.body.state.post_state_hash),
                            &mergeable_chs,
                        )
                    {
                        tracing::warn!(
                            "Skipping block index cache update for block {}: {}",
                            PrettyPrinter::build_string_bytes(&block.block_hash),
                            err
                        );
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        "Skipping mergeable/index cache update for block {}: {}",
                        PrettyPrinter::build_string_bytes(&block.block_hash),
                        err
                    );
                }
            }
        }
    }

    Ok(val_result)
}

pub(crate) async fn dispatch_validate_self_created<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    block: &BlockMessage,
    snapshot: &mut CasperSnapshot,
    pre_state_hash: Bytes,
    post_state_hash: Bytes,
) -> Result<Either<BlockError, ValidBlock>, CasperError> {
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
        tracing::error!("{}", msg);
        return Ok(Either::Left(BlockError::BlockException(
            CasperError::RuntimeError(msg),
        )));
    }
    if block.body.state.post_state_hash != post_state_hash {
        let msg = format!(
            "Self-created block post_state_hash mismatch: expected={}, actual={}, block={}",
            PrettyPrinter::build_string_no_limit(&post_state_hash),
            PrettyPrinter::build_string_no_limit(&block.body.state.post_state_hash),
            PrettyPrinter::build_string_bytes(&block.block_hash),
        );
        tracing::error!("{}", msg);
        return Ok(Either::Left(BlockError::BlockException(
            CasperError::RuntimeError(msg),
        )));
    }

    let start = std::time::Instant::now();
    let val_result = {
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
                    &this.block_store,
                    this.casper_shard_conf.disable_validator_progress_check,
                )
                .await)
            },
        )
        .await?;
        tracing::debug!(target: "f1r3fly.casper", "post-validation-block-summary");
        if let Either::Left(block_error) = block_summary_result {
            return Ok(Either::Left(block_error));
        }

        // SKIP validate_block_checkpoint and bonds_cache for self-created.

        let (neglected_invalid_block_result, t4) = timed_step(
            "neglected-invalid-block",
            BLOCK_VALIDATION_STEP_NEGLECTED_INVALID_BLOCK_TIME_METRIC,
            async { Ok(Validate::neglected_invalid_block(block, snapshot)) },
        )
        .await?;
        tracing::debug!(target: "f1r3fly.casper", "neglected-invalid-block-validated");
        if let Either::Left(block_error) = neglected_invalid_block_result {
            return Ok(Either::Left(block_error));
        }

        let (equivocation_detector_result, t5) = timed_step(
            "neglected-equivocation",
            BLOCK_VALIDATION_STEP_NEGLECTED_EQUIVOCATION_TIME_METRIC,
            async {
                EquivocationDetector::check_neglected_equivocations_with_update(
                    block,
                    &snapshot.dag,
                    &this.block_store,
                    &this.approved_block,
                    &this.block_dag_storage,
                )
                .await
                .map_err(CasperError::from)
            },
        )
        .await?;
        tracing::debug!(target: "f1r3fly.casper", "neglected-equivocation-validated");
        if let Either::Left(block_error) = equivocation_detector_result {
            return Ok(Either::Left(block_error));
        }

        let (phlo_price_result, t6) = timed_step(
            "phlo-price",
            BLOCK_VALIDATION_STEP_PHLO_PRICE_TIME_METRIC,
            async {
                Ok(Validate::phlo_price(
                    block,
                    this.casper_shard_conf.min_phlo_price,
                ))
            },
        )
        .await?;
        tracing::debug!(target: "f1r3fly.casper", "phlogiston-price-validated");
        if let Either::Left(_) = phlo_price_result {
            tracing::warn!(
                "One or more deploys has phloPrice lower than {}",
                this.casper_shard_conf.min_phlo_price
            );
        }

        let requested_as_dependency = this
            .casper_buffer_storage
            .requested_as_dependency(&BlockHashSerde(block.block_hash.clone()));

        let (equivocation_result, t7) = timed_step(
            "simple-equivocation",
            BLOCK_VALIDATION_STEP_SIMPLE_EQUIVOCATION_TIME_METRIC,
            async {
                EquivocationDetector::check_equivocations(
                    requested_as_dependency,
                    block,
                    &snapshot.dag,
                )
                .await
                .map_err(CasperError::from)
            },
        )
        .await?;
        tracing::debug!(target: "f1r3fly.casper", "equivocation-validated");

        tracing::debug!(
            target: "f1r3fly.casper",
            "Self-validation timing breakdown: summary={}, neglected-invalid={}, neglected-equiv={}, phlo={}, simple-equiv={} (checkpoint and bonds-cache skipped)",
            t1, t4, t5, t6, t7
        );

        equivocation_result
    };

    let elapsed = start.elapsed();

    if let Either::Right(ref status) = val_result {
        let block_info = PrettyPrinter::build_string_block_message(block, true);
        let deploy_count = block.body.deploys.len();
        tracing::info!(
            "Self-created block validated: {} ({}d) ({:?}) [{:?}]",
            block_info,
            deploy_count,
            status,
            elapsed
        );

        if this.casper_shard_conf.max_number_of_parents > 1 {
            let maybe_mergeable = this.runtime_manager.lock().await.load_mergeable_channels(
                &block.body.state.post_state_hash,
                block.sender.clone(),
                block.seq_num,
            );

            match maybe_mergeable {
                Ok(mergeable_chs) => {
                    if let Err(err) = this
                        .runtime_manager
                        .lock()
                        .await
                        .get_or_compute_block_index(
                            &block.block_hash,
                            &block.body.deploys,
                            &block.body.system_deploys,
                            &Blake2b256Hash::from_bytes_prost(&block.body.state.pre_state_hash),
                            &Blake2b256Hash::from_bytes_prost(&block.body.state.post_state_hash),
                            &mergeable_chs,
                        )
                    {
                        tracing::warn!(
                            "Skipping block index cache update for self-created block {}: {}",
                            PrettyPrinter::build_string_bytes(&block.block_hash),
                            err
                        );
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        "Skipping mergeable/index cache update for self-created block {}: {}",
                        PrettyPrinter::build_string_bytes(&block.block_hash),
                        err
                    );
                }
            }
        }
    }

    Ok(val_result)
}

pub(crate) fn dispatch_handle_invalid_block<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    block: &BlockMessage,
    status: &InvalidBlock,
    dag: &KeyValueDagRepresentation,
) -> Result<KeyValueDagRepresentation, CasperError> {
    let handle_invalid_block_effect =
        |block_dag_storage: &BlockDagKeyValueStorage,
         casper_buffer_storage: &CasperBufferKeyValueStorage,
         status: &InvalidBlock,
         block: &BlockMessage|
         -> Result<KeyValueDagRepresentation, CasperError> {
            tracing::warn!(
                "Recording invalid block {} for {:?}.",
                PrettyPrinter::build_string_bytes(&block.block_hash),
                status
            );

            // TODO: should be nice to have this transition of a block from casper buffer to dag storage atomic - OLD
            let updated_dag = block_dag_storage.insert(block, InsertMode::Invalid)?;
            record_dag_cardinality_metrics(&updated_dag);
            let block_hash_serde = BlockHashSerde(block.block_hash.clone());
            casper_buffer_storage.remove(block_hash_serde)?;
            Ok(updated_dag)
        };

    // Atomic read-modify-write on the equivocation tracker. See
    // docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.2.
    let record_evidence = |block_dag_storage: &BlockDagKeyValueStorage,
                           block: &BlockMessage|
     -> Result<(), CasperError> {
        let Some(base_equivocation_block_seq_num) = checked_base_seq(block.seq_num) else {
            return Ok(());
        };
        block_dag_storage
            .access_equivocations_tracker(|tracker| {
                let equivocation_records = tracker.data()?;
                let record_exists = equivocation_records.iter().any(|record| {
                    record.equivocator == block.sender
                        && record.equivocation_base_block_seq_num
                            == base_equivocation_block_seq_num
                });
                if !record_exists {
                    let new_equivocation_record = EquivocationRecord::new(
                        block.sender.clone(),
                        base_equivocation_block_seq_num,
                        BTreeSet::new(),
                    );
                    tracker.add(new_equivocation_record)?;
                }
                Ok(())
            })
            .map_err(|e| CasperError::RuntimeError(e.to_string()))?;
        Ok(())
    };

    match status {
        InvalidBlock::AdmissibleEquivocation => {
            record_evidence(&this.block_dag_storage, block)?;
            handle_invalid_block_effect(
                &this.block_dag_storage,
                &this.casper_buffer_storage,
                status,
                block,
            )
        }

        InvalidBlock::IgnorableEquivocation => {
            // Record evidence and apply the standard invalid-block effect,
            // mirroring AdmissibleEquivocation. See
            // docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.1.
            record_evidence(&this.block_dag_storage, block)?;
            handle_invalid_block_effect(
                &this.block_dag_storage,
                &this.casper_buffer_storage,
                status,
                block,
            )
        }

        status if status.is_slashable() => {
            // Every slashable status mints an EquivocationRecord. See
            // docs/theory/slashing/design/09-bug-fixes-and-rationale.md §9.3.
            record_evidence(&this.block_dag_storage, block)?;
            handle_invalid_block_effect(
                &this.block_dag_storage,
                &this.casper_buffer_storage,
                status,
                block,
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
