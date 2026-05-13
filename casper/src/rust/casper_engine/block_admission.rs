//! Block admission, deploy intake, valid-block handling.
//!
//! Phase 3 Step 5 — extracted from `multi_parent_casper_impl.rs`. Each
//! function takes the casper instance as a `&MultiParentCasperImpl<T>`
//! reference; the trait method is a one-line delegate in `traits.rs`.

use comm::rust::transport::transport_layer::TransportLayer;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use models::rust::normalizer_env::normalizer_env_from_deploy;
use rspace_plus_plus::rspace::history::Either;
use block_storage::rust::dag::block_dag_key_value_storage::{
    DeployId, InsertMode, KeyValueDagRepresentation,
};

use crate::rust::casper::DeployError;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::interpreter_util;

use super::snapshot::record_dag_cardinality_metrics;
use super::types::MultiParentCasperImpl;

pub(crate) fn admit_contains<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    hash: &BlockHash,
) -> bool {
    admit_buffer_contains(this, hash) || admit_dag_contains(this, hash)
}

pub(crate) fn admit_dag_contains<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    hash: &BlockHash,
) -> bool {
    // Bootstrap-window safety (P1-2): if the DAG representation is not yet
    // initialized (no approved/last-finalized block), report `false` rather
    // than panicking. Returning `false` here matches the trait's
    // pre-existing semantics for "block not present".
    match this.block_dag_storage.get_representation() {
        Ok(dag) => dag.contains(hash),
        Err(_) => false,
    }
}

pub(crate) fn admit_buffer_contains<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    hash: &BlockHash,
) -> bool {
    let block_hash_serde = BlockHashSerde(hash.clone());
    this.casper_buffer_storage.contains(&block_hash_serde)
}

pub(crate) fn admit_get_approved_block<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
) -> Result<&BlockMessage, CasperError> {
    Ok(&this.approved_block)
}

pub(crate) fn admit_deploy<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    deploy: Signed<DeployData>,
) -> Result<Either<DeployError, DeployId>, CasperError> {
    // Create normalizer environment from deploy
    let normalizer_env = normalizer_env_from_deploy(&deploy);
    let parse_started_at = std::time::Instant::now();

    // Try to parse the deploy term
    match interpreter_util::mk_term(&deploy.data.term, normalizer_env) {
        Err(interpreter_error) => {
            tracing::debug!(
                target: "f1r3fly.deploy.latency",
                parse_ms = parse_started_at.elapsed().as_millis(),
                "Deploy parse failed"
            );
            Ok(Either::Left(DeployError::parsing_error(format!(
                "Error in parsing term: \n{}",
                interpreter_error
            ))))
        }
        Ok(_parsed_term) => {
            let parse_elapsed_ms = parse_started_at.elapsed().as_millis();
            let add_started_at = std::time::Instant::now();
            let deploy_id = add_deploy(this, deploy)?;
            tracing::debug!(
                target: "f1r3fly.deploy.latency",
                parse_ms = parse_elapsed_ms,
                add_deploy_ms = add_started_at.elapsed().as_millis(),
                "Deploy parse/add completed"
            );
            Ok(Either::Right(deploy_id))
        }
    }
}

pub(crate) async fn admit_handle_valid_block<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    block: &BlockMessage,
) -> Result<KeyValueDagRepresentation, CasperError> {
    // Insert block as valid into DAG storage
    let updated_dag = this.block_dag_storage.insert(block, InsertMode::Normal)?;
    record_dag_cardinality_metrics(&updated_dag);

    // Remove user deploys from pending deploy storage as soon as the block is
    // accepted into the DAG.
    let deploys: Vec<_> = block
        .body
        .deploys
        .iter()
        .map(|pd| pd.deploy.clone())
        .collect();
    if !deploys.is_empty() {
        let deploys_count = deploys.len();
        let block_hash = PrettyPrinter::build_string_bytes(&block.block_hash);
        let block_number = block.body.state.block_number;
        // Phase 9 (A-3): `deploy_storage` is `parking_lot::Mutex` — no
        // poison propagation, so `.lock()` returns the guard directly.
        this.deploy_storage.lock().remove(deploys)?;

        tracing::debug!(
            "Removed {} deploys from pending pool for accepted block {} at {}.",
            deploys_count,
            block_hash,
            block_number
        );
    }

    // Remove block from casper buffer
    let block_hash_serde = BlockHashSerde(block.block_hash.clone());
    this.casper_buffer_storage.remove(block_hash_serde)?;

    // Publish BlockAdded event
    this.event_publisher
        .publish(super::events::added_event(block))?;

    // Update last finalized block if needed
    super::finalization_runner::update_last_finalized_block(this, block).await?;

    // Wake heartbeat immediately when a new peer block is accepted.
    if let Some(validator_id) = &this.validator_id {
        if block.sender != validator_id.public_key.bytes {
            if let Some(signal) = this.heartbeat_signal_ref.get() {
                tracing::debug!(
                    "Triggering heartbeat wake for accepted peer block {}",
                    PrettyPrinter::build_string_bytes(&block.block_hash)
                );
                signal.trigger_wake();
            }
        }
    }

    Ok(updated_dag)
}

pub(crate) fn add_deploy<T: TransportLayer + Send + Sync>(
    this: &MultiParentCasperImpl<T>,
    deploy: Signed<DeployData>,
) -> Result<DeployId, CasperError> {
    // Add deploy to storage. Phase 9 (A-3): parking_lot::Mutex.
    this.deploy_storage.lock().add(vec![deploy.clone()])?;

    // Log the received deploy
    let deploy_info = PrettyPrinter::build_string_signed_deploy_data(&deploy);
    tracing::info!("Received {}", deploy_info);

    // Wake the heartbeat immediately so it picks up the new deploy without
    // waiting for the next timer tick (up to check_interval seconds).
    // Phase 8 (C-4): operator-controlled via CasperShardConf rather than a
    // hardcoded predicate.
    if this.casper_shard_conf.deploy_heartbeat_wake_enabled {
        if let Some(signal) = this.heartbeat_signal_ref.get() {
            tracing::debug!("Triggering heartbeat wake for immediate block proposal");
            signal.trigger_wake();
        } else {
            tracing::debug!("No heartbeat signal available (heartbeat may be disabled)");
        }
    }

    // Return deploy signature as DeployId
    Ok(deploy.sig.to_vec())
}
