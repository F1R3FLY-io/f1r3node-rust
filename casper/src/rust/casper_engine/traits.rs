//! Trait dispatch — `impl Casper` and `impl MultiParentCasper` for
//! `MultiParentCasperImpl`. Each method is a thin delegate to a free
//! function in a sibling sub-module.
//!
//! Phase 3 Step 6 — final extraction. Both trait impl blocks live here
//! because Rust requires each `impl Trait for Type` block to live in a
//! single file. The delegates make the dispatch surface small (each
//! method is 2–4 lines) so the file is reviewable as a single concern
//! ("how the casper engine binds into its public protocol surface").

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use block_storage::rust::dag::block_dag_key_value_storage::{DeployId, KeyValueDagRepresentation};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::transport::transport_layer::TransportLayer;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporter;

use crate::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use crate::rust::casper::{Casper, CasperSnapshot, DeployError, MultiParentCasper};
use crate::rust::engine::block_retriever::AdmitHashReason;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::validator_identity::ValidatorIdentity;

use super::types::MultiParentCasperImpl;

#[async_trait]
impl<T: TransportLayer + Send + Sync> Casper for MultiParentCasperImpl<T> {
    async fn get_snapshot(&self) -> Result<CasperSnapshot, CasperError> {
        super::snapshot::compute_snapshot(self).await
    }

    fn contains(&self, hash: &BlockHash) -> bool {
        super::block_admission::admit_contains(self, hash)
    }

    fn dag_contains(&self, hash: &BlockHash) -> bool {
        super::block_admission::admit_dag_contains(self, hash)
    }

    fn buffer_contains(&self, hash: &BlockHash) -> bool {
        super::block_admission::admit_buffer_contains(self, hash)
    }

    fn get_approved_block(&self) -> Result<&BlockMessage, CasperError> {
        super::block_admission::admit_get_approved_block(self)
    }

    fn deploy(
        &self,
        deploy: Signed<DeployData>,
    ) -> Result<Either<DeployError, DeployId>, CasperError> {
        super::block_admission::admit_deploy(self, deploy)
    }

    async fn estimator(
        &self,
        dag: &mut KeyValueDagRepresentation,
    ) -> Result<Vec<BlockHash>, CasperError> {
        super::snapshot::estimator(self, dag).await
    }

    fn get_version(&self) -> i64 { self.casper_shard_conf.casper_version }

    async fn validate(
        &self,
        block: &BlockMessage,
        snapshot: &mut CasperSnapshot,
    ) -> Result<Either<BlockError, ValidBlock>, CasperError> {
        super::validation_dispatcher::dispatch_validate(self, block, snapshot).await
    }

    async fn validate_self_created(
        &self,
        block: &BlockMessage,
        snapshot: &mut CasperSnapshot,
        pre_state_hash: Bytes,
        post_state_hash: Bytes,
    ) -> Result<Either<BlockError, ValidBlock>, CasperError> {
        super::validation_dispatcher::dispatch_validate_self_created(
            self,
            block,
            snapshot,
            pre_state_hash,
            post_state_hash,
        )
        .await
    }

    async fn handle_valid_block(
        &self,
        block: &BlockMessage,
    ) -> Result<KeyValueDagRepresentation, CasperError> {
        super::block_admission::admit_handle_valid_block(self, block).await
    }

    fn handle_invalid_block(
        &self,
        block: &BlockMessage,
        status: &InvalidBlock,
        dag: &KeyValueDagRepresentation,
    ) -> Result<KeyValueDagRepresentation, CasperError> {
        super::validation_dispatcher::dispatch_handle_invalid_block(self, block, status, dag)
    }

    fn get_dependency_free_from_buffer(&self) -> Result<Vec<BlockMessage>, CasperError> {
        super::buffer_resolver::buffer_get_dependency_free_from_buffer(self)
    }

    fn get_all_from_buffer(&self) -> Result<Vec<BlockMessage>, CasperError> {
        super::buffer_resolver::buffer_get_all_from_buffer(self)
    }
}

#[async_trait]
impl<T: TransportLayer + Send + Sync> MultiParentCasper for MultiParentCasperImpl<T> {
    async fn fetch_dependencies(&self) -> Result<(), CasperError> {
        // Get pendants from CasperBuffer
        let pendants = self.casper_buffer_storage.get_pendants();

        // Filter to get unseen pendants (not in block store)
        let mut pendants_unseen = Vec::new();
        for pendant_serde in pendants.iter() {
            let pendant_hash = BlockHash::from(pendant_serde.0.clone());
            if self.block_store.get(&pendant_hash)?.is_none() {
                pendants_unseen.push(pendant_hash);
            }
        }

        tracing::debug!(
            "Requesting CasperBuffer pendant hashes, {} items.",
            pendants_unseen.len()
        );

        for dependency in pendants_unseen {
            tracing::debug!(
                "Sending dependency {} to BlockRetriever",
                PrettyPrinter::build_string_bytes(&dependency)
            );

            self.block_retriever
                .admit_hash(
                    dependency,
                    None,
                    AdmitHashReason::MissingDependencyRequested,
                )
                .await?;
        }

        Ok(())
    }

    fn normalized_initial_fault(
        &self,
        weights: HashMap<Validator, u64>,
    ) -> Result<f32, CasperError> {
        let equivocating_weight =
            self.block_dag_storage
                .access_equivocations_tracker(|tracker| {
                    let equivocation_records = tracker.data()?;
                    let equivocating_weight: u64 = equivocation_records
                        .iter()
                        .map(|record| &record.equivocator)
                        .filter_map(|equivocator| weights.get(equivocator))
                        .sum();
                    Ok(equivocating_weight)
                })?;

        let total_weight: u64 = weights.values().sum();
        if total_weight == 0 {
            Ok(0.0)
        } else {
            Ok(equivocating_weight as f32 / total_weight as f32)
        }
    }

    async fn last_finalized_block(&self) -> Result<BlockMessage, CasperError> {
        super::finalization_runner::compute_last_finalized_block(
            super::finalization_runner::FinalizationContext {
                block_dag_storage: self.block_dag_storage.clone(),
                block_store: self.block_store.clone(),
                deploy_storage: self.deploy_storage.clone(),
                runtime_manager: self.runtime_manager.clone(),
                event_publisher: self.event_publisher.clone(),
                finalization_in_progress: self.finalization_in_progress.clone(),
                enable_mergeable_channel_gc: self.casper_shard_conf.enable_mergeable_channel_gc,
                fault_tolerance_threshold: self.casper_shard_conf.fault_tolerance_threshold,
                finalizer_conf: self.casper_shard_conf.finalizer_conf.clone(),
                finalizer_blocking_timeout: self.casper_shard_conf.finalizer_blocking_timeout,
            },
        )
        .await
    }

    async fn block_dag(&self) -> Result<KeyValueDagRepresentation, CasperError> {
        self.block_dag_storage
            .get_representation()
            .map_err(Into::into)
    }

    fn block_store(&self) -> &KeyValueBlockStore { &self.block_store }

    fn get_validator(&self) -> Option<ValidatorIdentity> { self.validator_id.clone() }

    async fn get_history_exporter(&self) -> Arc<dyn RSpaceExporter> {
        self.runtime_manager
            .lock()
            .await
            .get_history_repo()
            .exporter()
    }

    fn runtime_manager(&self) -> Arc<tokio::sync::Mutex<RuntimeManager>> {
        self.runtime_manager.clone()
    }

    async fn has_pending_deploys_in_storage(&self) -> Result<bool, CasperError> {
        let snapshot = self.get_snapshot().await?;
        self.has_pending_deploys_in_storage_for_snapshot(&snapshot)
            .await
    }

    async fn has_pending_deploys_in_storage_for_snapshot(
        &self,
        snapshot: &CasperSnapshot,
    ) -> Result<bool, CasperError> {
        let latest_block_number = snapshot.dag.latest_block_number();
        let earliest_block_number =
            latest_block_number - snapshot.on_chain_state.shard_conf.deploy_lifespan;
        let current_time_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        // Phase 9 (A-3): `deploy_storage` is `parking_lot::Mutex`.
        let storage = self.deploy_storage.lock();
        if !storage.non_empty().map_err(|e| {
            CasperError::RuntimeError(format!("Failed to query deploy storage: {:?}", e))
        })? {
            return Ok(false);
        }

        storage
            .any(|deploy| {
                let block_expired = deploy.data.valid_after_block_number <= earliest_block_number;
                let time_expired = deploy.data.is_expired_at(current_time_millis);
                if block_expired || time_expired {
                    return Ok(false);
                }

                // `pending_deploy_is_future_for_next_block` is `pub(super)`
                // in `events`; the call resolves because `traits` is a
                // sibling sub-module of `events` under `casper_engine`.
                let is_future = super::events::pending_deploy_is_future_for_next_block(
                    latest_block_number,
                    deploy.data.valid_after_block_number,
                );
                let already_in_scope = snapshot.deploys_in_scope.contains(&deploy.sig);
                Ok(!is_future && !already_in_scope)
            })
            .map_err(|e| {
                CasperError::RuntimeError(format!("Failed to scan deploy storage: {:?}", e))
            })
    }
}
