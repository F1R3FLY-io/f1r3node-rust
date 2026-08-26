//! Trait dispatch — `impl Casper` and `impl MultiParentCasper` for
//! `MultiParentCasperImpl`. Each method is a thin delegate to a free
//! function in a sibling sub-module.
//!
//! Phase 3 Step 6 — final extraction. Both trait impl blocks live here
//! because Rust requires each `impl Trait for Type` block to live in a
//! single file. The delegates make the dispatch surface small (each
//! method is 2–4 lines) so the file is reviewable as a single concern
//! ("how the casper engine binds into its public protocol surface").

use std::sync::atomic::Ordering;
use std::sync::Arc;

use async_trait::async_trait;
use block_storage::rust::dag::block_dag_key_value_storage::{
    CertifiedAdmissionOutcome, CertifiedSenderAuthority, DeployId, KeyValueDagRepresentation,
};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::transport::transport_layer::TransportLayer;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData};
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporter;

use super::types::MultiParentCasperImpl;
use crate::rust::block_status::{CertifiedBlockValidation, InvalidBlock};
use crate::rust::casper::{
    Casper, CasperShardConf, CasperSnapshot, DeployError, MultiParentCasper,
};
use crate::rust::engine::block_retriever::AdmitHashReason;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::validator_identity::ValidatorIdentity;

#[async_trait]
impl<T: TransportLayer + Send + Sync> Casper for MultiParentCasperImpl<T> {
    async fn get_snapshot(&self) -> Result<CasperSnapshot, CasperError> {
        super::snapshot::compute_snapshot(self).await
    }

    fn request_finalization(&self) -> Result<(), CasperError> {
        super::finalization_runner::request_finalization(self)
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

    fn deploy_cosigned(
        &self,
        cosigned: crypto::rust::signatures::signed::Cosigned<DeployData>,
    ) -> Result<Either<DeployError, DeployId>, CasperError> {
        super::block_admission::admit_deploy_cosigned(self, cosigned)
    }

    async fn estimator(
        &self,
        dag: &mut KeyValueDagRepresentation,
    ) -> Result<Vec<BlockHash>, CasperError> {
        super::snapshot::estimator(self, dag).await
    }

    fn get_version(&self) -> i64 { self.casper_shard_conf.casper_version }

    fn recovery_sync_active(&self) -> bool { self.recovery_sync_active.load(Ordering::Acquire) }

    fn set_recovery_sync_active(&self, active: bool) {
        self.recovery_sync_active.store(active, Ordering::Release);
    }

    #[tracing::instrument(level = "info", skip(self, block, snapshot), fields(block_hash = %PrettyPrinter::build_string_bytes(&block.block_hash)))]
    async fn validate(
        &self,
        block: &BlockMessage,
        snapshot: &mut CasperSnapshot,
    ) -> Result<CertifiedBlockValidation, CasperError> {
        super::validation_dispatcher::dispatch_validate(self, block, snapshot).await
    }

    async fn validate_self_created(
        &self,
        block: &BlockMessage,
        snapshot: &mut CasperSnapshot,
        pre_state_hash: Bytes,
        post_state_hash: Bytes,
    ) -> Result<CertifiedBlockValidation, CasperError> {
        super::validation_dispatcher::dispatch_validate_self_created(
            self,
            block,
            snapshot,
            pre_state_hash,
            post_state_hash,
        )
        .await
    }

    #[tracing::instrument(level = "info", skip(self, block), fields(block_hash = %PrettyPrinter::build_string_bytes(&block.block_hash)))]
    async fn handle_valid_block(
        &self,
        block: &BlockMessage,
        certificate: &CertifiedSenderAuthority,
        outcome: &CertifiedAdmissionOutcome,
    ) -> Result<KeyValueDagRepresentation, CasperError> {
        super::block_admission::admit_handle_valid_block(self, block, certificate, outcome).await
    }

    fn handle_invalid_block(
        &self,
        block: &BlockMessage,
        status: &InvalidBlock,
        dag: &KeyValueDagRepresentation,
        certificate: &CertifiedSenderAuthority,
        outcome: &CertifiedAdmissionOutcome,
    ) -> Result<KeyValueDagRepresentation, CasperError> {
        super::validation_dispatcher::dispatch_handle_invalid_block(
            self,
            block,
            status,
            dag,
            certificate,
            outcome,
        )
    }

    fn get_dependency_free_from_buffer(&self) -> Result<Vec<BlockMessage>, CasperError> {
        super::buffer_resolver::buffer_get_dependency_free_from_buffer(self)
    }

    fn get_dependency_free_hashes_from_buffer(&self) -> Result<Vec<BlockHash>, CasperError> {
        super::buffer_resolver::buffer_get_dependency_free_hashes_from_buffer(self)
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

    fn normalized_initial_fault(&self, target: &BlockHash) -> Result<f32, CasperError> {
        let dag = self.block_dag_storage.get_representation()?;
        let context =
            crate::rust::causal_equivocation::CertifiedConsensusContext::for_target(&dag, target)?;
        Ok(context.normalized_initial_fault())
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
        self.runtime_manager.get_history_repo().exporter()
    }

    fn runtime_manager(&self) -> Arc<RuntimeManager> { self.runtime_manager.clone() }

    fn casper_shard_conf(&self) -> &CasperShardConf { &self.casper_shard_conf }

    fn rejected_deploy_buffer_contains_sig(&self, sig: &[u8]) -> Result<bool, CasperError> {
        self.rejected_deploy_buffer
            .lock()
            .map_err(|e| CasperError::LockError(e.to_string()))?
            .contains_sig(sig)
            .map_err(Into::into)
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
