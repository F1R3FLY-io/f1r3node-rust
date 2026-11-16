// See casper/src/main/scala/coop/rchain/casper/engine/GenesisCeremonyMaster.scala

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::deploy::key_value_deploy_storage::{
    KeyValueDeployStorage, KeyValueRejectedDeployBuffer,
};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::peer_node::PeerNode;
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::rp::rp_conf::RPConf;
use comm::rust::transport::transport_layer::TransportLayer;
use models::rust::casper::protocol::casper_message::{ApprovedBlock, BlockMessage, CasperMessage};
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;
use tokio::time::sleep;

use crate::rust::blocks::ingest::block_ingest::IngestHandle;
use crate::rust::casper::{hash_set_casper, CasperShardConf, MultiParentCasper};
use crate::rust::engine::approve_block_protocol::ApproveBlockProtocolImpl;
use crate::rust::engine::engine::{
    insert_into_block_and_dag_store, log_no_approved_block_available,
    record_direct_to_running_init_metrics, send_no_approved_block_available, transition_to_running,
    Engine,
};
use crate::rust::engine::engine_cell::EngineCell;
use crate::rust::errors::CasperError;
use crate::rust::estimator::Estimator;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;
use crate::rust::validator_identity::ValidatorIdentity;

pub struct GenesisCeremonyMaster<T: TransportLayer + Send + Sync + Clone + 'static> {
    approve_protocol: Arc<ApproveBlockProtocolImpl<T>>,
    transport_layer: Arc<T>,
    rp_conf_ask: RPConf,
}

impl<T: TransportLayer + Send + Sync + Clone + 'static> GenesisCeremonyMaster<T> {
    pub fn new(approve_protocol: Arc<ApproveBlockProtocolImpl<T>>) -> Self {
        // In Scala these come via implicit parameters
        let transport_layer = approve_protocol.transport().clone();
        let rp_conf_ask = approve_protocol
            .conf()
            .as_ref()
            .expect("RPConf required for GenesisCeremonyMaster")
            .as_ref()
            .clone();

        Self {
            approve_protocol,
            transport_layer,
            rp_conf_ask,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn waiting_for_approved_block_loop(
        // Infrastructure dependencies (Scala implicit parameters)
        transport_layer: Arc<T>,
        rp_conf_ask: RPConf,
        connections_cell: ConnectionsCell,
        last_approved_block: Arc<Mutex<Option<ApprovedBlock>>>,
        event_publisher: &F1r3flyEvents,
        engine_cell: Arc<EngineCell>,
        mut block_store: KeyValueBlockStore,
        mut block_dag_storage: BlockDagKeyValueStorage,
        deploy_storage: KeyValueDeployStorage,
        rejected_deploy_buffer: Arc<Mutex<KeyValueRejectedDeployBuffer>>,
        runtime_manager: Arc<RuntimeManager>,
        estimator: Estimator,
        ingest: IngestHandle,
        casper_shard_conf: CasperShardConf,
        validator_id: Option<ValidatorIdentity>,
        disable_state_exporter: bool,
        heartbeat_signal_ref: crate::rust::heartbeat_signal::HeartbeatSignalRef,
    ) -> Result<(), CasperError> {
        let approved_block = loop {
            sleep(Duration::from_secs(2)).await;

            if let Some(approved_block) = last_approved_block.lock().unwrap().clone() {
                break approved_block;
            }
        };

        let ab = approved_block.candidate.block.clone();

        insert_into_block_and_dag_store(
            &mut block_store,
            &mut block_dag_storage,
            &ab,
            approved_block.clone(),
        )?;
        // History rooted at genesis: coverage is complete from 0.
        block_dag_storage.record_carrier_coverage_from(0)?;

        let casper = Self::create_casper_from_storage(
            event_publisher,
            &runtime_manager,
            &estimator,
            &block_store,
            &block_dag_storage,
            &deploy_storage,
            &rejected_deploy_buffer,
            validator_id.clone(),
            &casper_shard_conf,
            ab,
            &ingest,
            &heartbeat_signal_ref,
        )
        .await?;

        // Scala: Engine.transitionToRunning[F](..., init = ().pure[F], ...)
        let the_init = Arc::new(|| {
            Box::pin(async { Ok(()) })
                as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
        });

        // Direct-to-running path: emit init metrics that are otherwise produced in Initializing.
        record_direct_to_running_init_metrics();

        transition_to_running(
            ingest.clone(),
            Arc::new(casper),
            approved_block.clone(),
            the_init,
            disable_state_exporter,
            transport_layer.clone(),
            rp_conf_ask.clone(),
            // Phase 7b-1: ceremony-master path is genesis-rooted.
            // Snapshot chunk fetch has nothing to anchor against
            // (no prior snapshot exists), so passing `None` is
            // correct here regardless of boot pipeline state.
            None,
            // Phase 7b-2: ceremony-master also has no prior
            // WAL slice to fetch between snapshots, so the
            // payload-fetch dispatch stays disabled.
            None,
            &engine_cell,
            event_publisher,
            // The ceremony master transitions genesis-rooted: its
            // anchor is height 0, so the settled door never opens and
            // no runtime state fetch can be needed.
            None,
        )
        .await?;

        // Scala: CommUtil[F].sendForkChoiceTipRequest
        if let Err(e) = transport_layer
            .send_fork_choice_tip_request(&connections_cell, &rp_conf_ask)
            .await
        {
            tracing::warn!(error = %e, "Fork choice tip request after genesis failed");
        }

        Ok(())
    }

    /// Helper function to create MultiParentCasper from storage components
    /// Same logic as CasperLaunchImpl::create_casper but as static function
    #[allow(clippy::too_many_arguments)]
    async fn create_casper_from_storage(
        event_publisher: &F1r3flyEvents,
        runtime_manager: &Arc<RuntimeManager>,
        estimator: &Estimator,
        block_store: &KeyValueBlockStore,
        block_dag_storage: &BlockDagKeyValueStorage,
        deploy_storage: &KeyValueDeployStorage,
        rejected_deploy_buffer: &Arc<Mutex<KeyValueRejectedDeployBuffer>>,
        validator_id: Option<ValidatorIdentity>,
        casper_shard_conf: &CasperShardConf,
        ab: BlockMessage,
        ingest: &IngestHandle,
        heartbeat_signal_ref: &crate::rust::heartbeat_signal::HeartbeatSignalRef,
    ) -> Result<crate::rust::engine::multi_parent_casper::MultiParentCasperImpl, CasperError> {
        let runtime_manager_for_casper = runtime_manager.clone();

        hash_set_casper(
            ingest.clone(),
            event_publisher.clone(),
            runtime_manager_for_casper,
            estimator.clone(),
            block_store.clone(),
            block_dag_storage.clone(),
            deploy_storage.clone(),
            rejected_deploy_buffer.clone(),
            validator_id,
            casper_shard_conf.clone(),
            ab,
            heartbeat_signal_ref.clone(),
        )
        .await
    }
}

#[async_trait]
impl<T: TransportLayer + Send + Sync + Clone + 'static> Engine for GenesisCeremonyMaster<T> {
    async fn init(&self) -> Result<(), CasperError> { self.approve_protocol.run().await }

    async fn handle(&self, peer: PeerNode, msg: CasperMessage) -> Result<(), CasperError> {
        match msg {
            CasperMessage::ApprovedBlockRequest(approved_block_request) => {
                send_no_approved_block_available(
                    &self.rp_conf_ask,
                    &*self.transport_layer,
                    &approved_block_request.identifier,
                    peer,
                )
                .await
            }
            CasperMessage::BlockApproval(block_approval) => {
                self.approve_protocol.add_approval(block_approval).await
            }
            CasperMessage::NoApprovedBlockAvailable(no_approved_block_available) => {
                log_no_approved_block_available(&no_approved_block_available.node_identifier);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn with_casper(&self) -> Option<Arc<dyn MultiParentCasper + Send + Sync>> { None }
}
