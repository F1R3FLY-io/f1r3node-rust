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

use super::types::MultiParentCasperImpl;
use crate::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use crate::rust::casper::{Casper, CasperSnapshot, DeployError, MultiParentCasper};
use crate::rust::engine::block_retriever::AdmitHashReason;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::validator_identity::ValidatorIdentity;

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
            super::finalization_runner::build_finalization_context(self),
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
        // C15 / Arch-3: body extracted to
        // `block_admission::admit_has_pending_deploys_in_storage_for_snapshot`.
        // `dispatch.rs` hosts only thin trait delegates per the
        // module-level doc-comment.
        super::block_admission::admit_has_pending_deploys_in_storage_for_snapshot(self, snapshot)
            .await
    }
}
