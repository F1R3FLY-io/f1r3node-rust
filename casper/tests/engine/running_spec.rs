// See casper/src/test/scala/coop/rchain/casper/engine/RunningSpec.scala

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use block_storage::rust::dag::block_dag_key_value_storage::{
    CertifiedAdmissionOutcome, CertifiedSenderAuthority,
};
use casper::rust::block_status::{CertifiedBlockValidation, InvalidBlock};
use casper::rust::casper::{
    Casper, CasperShardConf, CasperSnapshot, DeployError, MultiParentCasper,
};
use casper::rust::engine::engine::Engine;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::engine::running::{
    update_fork_choice_tips_if_stuck, Running, RunningRecoveryContext,
};
use casper::rust::errors::CasperError;
use casper::rust::validator_identity::ValidatorIdentity;
use models::routing::protocol::Message as ProtocolMessage;
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::{
    ApprovedBlock, ApprovedBlockCandidate, BlockMessage, BlockRequest, CasperMessage, DeployData,
    ForkChoiceTipRequest, HasBlock, MergeableEntryRequest,
};
use prost::bytes::Bytes;
use prost::Message;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporter;

use crate::engine::setup::{to_casper_message, TestFixture};

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct ValidatorAwareNoOpsCasper {
        inner: crate::helper::no_ops_casper_effect::NoOpsCasperEffect,
        validator_id: ValidatorIdentity,
        finalization_requests: Arc<AtomicUsize>,
        recovery_sync_active: Arc<std::sync::atomic::AtomicBool>,
    }

    #[async_trait]
    impl MultiParentCasper for ValidatorAwareNoOpsCasper {
        async fn fetch_dependencies(&self) -> Result<(), CasperError> {
            self.inner.fetch_dependencies().await
        }

        fn normalized_initial_fault(
            &self,
            target: &models::rust::block_hash::BlockHash,
        ) -> Result<f32, CasperError> {
            self.inner.normalized_initial_fault(target)
        }

        async fn last_finalized_block(&self) -> Result<BlockMessage, CasperError> {
            self.inner.last_finalized_block().await
        }

        async fn block_dag(
            &self,
        ) -> Result<
            block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation,
            CasperError,
        > {
            self.inner.block_dag().await
        }

        fn block_store(&self) -> &block_storage::rust::key_value_block_store::KeyValueBlockStore {
            self.inner.block_store()
        }

        fn casper_shard_conf(&self) -> &CasperShardConf { self.inner.casper_shard_conf() }

        fn get_validator(&self) -> Option<ValidatorIdentity> { Some(self.validator_id.clone()) }

        async fn get_history_exporter(&self) -> Arc<dyn RSpaceExporter> {
            self.inner.get_history_exporter().await
        }

        fn runtime_manager(
            &self,
        ) -> Arc<casper::rust::util::rholang::runtime_manager::RuntimeManager> {
            self.inner.runtime_manager()
        }

        async fn has_pending_deploys_in_storage(&self) -> Result<bool, CasperError> {
            self.inner.has_pending_deploys_in_storage().await
        }
    }

    #[async_trait]
    impl Casper for ValidatorAwareNoOpsCasper {
        async fn request_block_from_peers(&self, hash: BlockHash) -> Result<(), CasperError> {
            self.inner.request_block_from_peers(hash).await
        }

        async fn get_snapshot(&self) -> Result<CasperSnapshot, CasperError> {
            self.inner.get_snapshot().await
        }

        fn request_finalization(&self) -> Result<(), CasperError> {
            self.finalization_requests.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn recovery_sync_active(&self) -> bool { self.recovery_sync_active.load(Ordering::SeqCst) }

        fn set_recovery_sync_active(&self, active: bool) {
            self.recovery_sync_active.store(active, Ordering::SeqCst);
        }

        fn contains(&self, hash: &BlockHash) -> bool { self.inner.contains(hash) }

        fn dag_contains(&self, hash: &BlockHash) -> bool { self.inner.dag_contains(hash) }

        fn buffer_contains(&self, hash: &BlockHash) -> bool { self.inner.buffer_contains(hash) }

        fn get_approved_block(&self) -> Result<&BlockMessage, CasperError> {
            self.inner.get_approved_block()
        }

        fn deploy(
            &self,
            deploy: crypto::rust::signatures::signed::Signed<DeployData>,
        ) -> Result<
            Either<DeployError, block_storage::rust::dag::block_dag_key_value_storage::DeployId>,
            CasperError,
        > {
            self.inner.deploy(deploy)
        }

        async fn estimator(
            &self,
            dag: &mut block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation,
        ) -> Result<Vec<BlockHash>, CasperError> {
            self.inner.estimator(dag).await
        }

        fn get_version(&self) -> i64 { self.inner.get_version() }

        async fn validate(
            &self,
            block: &BlockMessage,
            snapshot: &mut CasperSnapshot,
        ) -> Result<CertifiedBlockValidation, CasperError> {
            self.inner.validate(block, snapshot).await
        }

        async fn validate_self_created(
            &self,
            block: &BlockMessage,
            snapshot: &mut CasperSnapshot,
            pre_state_hash: Bytes,
            post_state_hash: Bytes,
        ) -> Result<CertifiedBlockValidation, CasperError> {
            self.inner
                .validate_self_created(block, snapshot, pre_state_hash, post_state_hash)
                .await
        }

        async fn handle_valid_block(
            &self,
            block: &BlockMessage,
            certificate: &CertifiedSenderAuthority,
            outcome: &CertifiedAdmissionOutcome,
        ) -> Result<
            block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation,
            CasperError,
        > {
            self.inner
                .handle_valid_block(block, certificate, outcome)
                .await
        }

        fn handle_invalid_block(
            &self,
            block: &BlockMessage,
            status: &InvalidBlock,
            dag: &block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation,
            certificate: &CertifiedSenderAuthority,
            outcome: &CertifiedAdmissionOutcome,
        ) -> Result<
            block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation,
            CasperError,
        > {
            self.inner
                .handle_invalid_block(block, status, dag, certificate, outcome)
        }

        fn get_dependency_free_from_buffer(&self) -> Result<Vec<BlockMessage>, CasperError> {
            self.inner.get_dependency_free_from_buffer()
        }

        fn get_all_from_buffer(&self) -> Result<Vec<BlockMessage>, CasperError> {
            self.inner.get_all_from_buffer()
        }
    }

    #[tokio::test]
    async fn engine_should_enqueue_block_message_for_processing() {
        let fixture = TestFixture::new().await;
        let block_message = get_random_block(
            None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        );

        let signed_block = fixture.validator_id.sign_block(&block_message);

        fixture
            .engine
            .handle(
                fixture.local.clone(),
                CasperMessage::BlockMessage(signed_block.clone()),
            )
            .await
            .unwrap();

        // Verify the block was enqueued for processing (following Scala test behavior)
        // This matches the Scala test pattern: getRandomBlock() -> signBlock() -> handle() -> check queue
        assert!(
            fixture
                .is_block_in_processing_queue(&signed_block.block_hash)
                .await,
            "Block should be enqueued in processing queue after being handled"
        );
    }

    #[tokio::test]
    async fn engine_should_respond_to_block_request() {
        let fixture = TestFixture::new().await;

        // Scala: blockStore.put(genesis.blockHash, genesis) (line 79)
        // Insert genesis block into store before testing BlockRequest response
        let genesis = fixture.genesis.clone();
        fixture
            .block_store
            .put(genesis.block_hash.clone(), &genesis)
            .expect("Failed to put genesis block");

        let block_request = BlockRequest {
            hash: genesis.block_hash.clone(),
        };

        fixture
            .engine
            .handle(
                fixture.local.clone(),
                CasperMessage::BlockRequest(block_request),
            )
            .await
            .unwrap();

        assert_eq!(fixture.transport_layer.request_count(), 1);
        let sent_request = fixture.transport_layer.pop_request().unwrap();
        assert_eq!(sent_request.peer, fixture.local);
        if let CasperMessage::BlockMessage(sent_msg) = to_casper_message(sent_request.msg) {
            assert_eq!(sent_msg, genesis);
        } else {
            panic!("Expected BlockMessage");
        }
    }

    #[tokio::test]
    async fn engine_refuses_to_export_mergeable_evidence() {
        let fixture = TestFixture::new().await;
        let genesis = fixture.genesis.clone();
        fixture
            .block_store
            .put(genesis.block_hash.clone(), &genesis)
            .expect("Failed to put genesis block");

        fixture
            .engine
            .handle(
                fixture.local.clone(),
                CasperMessage::MergeableEntryRequest(MergeableEntryRequest {
                    block_hash: genesis.block_hash.clone(),
                }),
            )
            .await
            .unwrap();

        assert_eq!(fixture.transport_layer.request_count(), 1);
        let sent_request = fixture.transport_layer.pop_request().unwrap();
        assert_eq!(sent_request.peer, fixture.local);
        let packet = match sent_request.msg.message {
            Some(ProtocolMessage::Packet(packet)) => packet,
            _ => panic!("Expected packet response"),
        };
        assert_eq!(packet.type_id, "MergeableEntryResponse");
        let response = models::casper::MergeableEntryResponseProto::decode(packet.content)
            .expect("MergeableEntryResponse payload");
        assert_eq!(response.block_hash, genesis.block_hash);
        assert!(response.serialized_entry.is_empty());
    }

    #[tokio::test]
    async fn engine_should_respond_to_approved_block_request() {
        let fixture = TestFixture::new().await;

        // Scala: Similar to BlockRequest test, genesis needs to be in store
        // Insert genesis block into store before testing ApprovedBlockRequest response
        let genesis_block = fixture.genesis.clone();
        fixture
            .block_store
            .put(genesis_block.block_hash.clone(), &genesis_block)
            .expect("Failed to put genesis block");

        let approved_block_request =
            models::rust::casper::protocol::casper_message::ApprovedBlockRequest {
                identifier: "test".to_string(),
                trim_state: false,
            };
        let expected_approved_block =
            models::rust::casper::protocol::casper_message::ApprovedBlock {
                candidate: models::rust::casper::protocol::casper_message::ApprovedBlockCandidate {
                    block: genesis_block,
                    required_sigs: 0,
                },
                sigs: Vec::new(),
                floor_seed: None,
            };

        fixture
            .engine
            .handle(
                fixture.local.clone(),
                CasperMessage::ApprovedBlockRequest(approved_block_request),
            )
            .await
            .unwrap();

        assert_eq!(fixture.transport_layer.request_count(), 1);
        let sent_request = fixture.transport_layer.pop_request().unwrap();
        assert_eq!(sent_request.peer, fixture.local);
        if let CasperMessage::ApprovedBlock(sent_msg) = to_casper_message(sent_request.msg) {
            assert_eq!(sent_msg, expected_approved_block);
        } else {
            panic!("Expected ApprovedBlock");
        }
    }

    #[tokio::test]
    async fn engine_should_respond_to_fork_choice_tip_request() {
        let mut fixture = TestFixture::new().await;

        // Step 1: Create a request object
        let request = ForkChoiceTipRequest {};

        // Step 2: Create 2 blocks with distinct senders so both can be tips.
        let mut block1 = get_random_block(
            None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        );
        block1.sender = Bytes::from_static(b"sender-1");

        let mut block2 = get_random_block(
            None, None, None, None, None, None, None, None, None, None, None, None, None, None,
        );
        block2.sender = Bytes::from_static(b"sender-2");

        // Step 3: Insert blocks in blockDagStorage (following Scala implementation)
        // This matches the Scala pattern: blockDagStorage.insert(block1, false)
        fixture.casper.insert_block(block1.clone(), false);
        fixture.casper.insert_block(block2.clone(), false);

        // Step 5: Call engine.handle with local peer and request object
        fixture
            .engine
            .handle(
                fixture.local.clone(),
                CasperMessage::ForkChoiceTipRequest(request),
            )
            .await
            .unwrap();

        let engine_casper = fixture
            .engine
            .with_casper()
            .expect("Running engine should expose a casper instance");
        let expected_tips: HashSet<Bytes> = engine_casper
            .block_dag()
            .await
            .expect("Failed to load block DAG")
            .latest_message_hashes()
            .into_iter()
            .map(|(_, hash)| hash)
            .collect();

        // Step 6: Get requests from transportLayer
        let requests = fixture.transport_layer.get_all_requests();
        assert_eq!(
            requests.len(),
            expected_tips.len(),
            "Expected one HasBlock response per fork-choice tip"
        );

        // Step 8: Assert all transport-layer requests target local peer.
        for request in &requests {
            assert_eq!(request.peer, fixture.local);
        }

        // Step 9: Assert all responses are HasBlock messages with at least one tip hash.
        let mut received_tips: HashSet<Bytes> = HashSet::new();
        let mut has_block_count = 0usize;
        for request in &requests {
            if let CasperMessage::HasBlock(HasBlock { hash }) =
                to_casper_message(request.msg.clone())
            {
                has_block_count += 1;
                received_tips.insert(hash);
            } else {
                panic!("Expected HasBlock response for fork-choice tip request");
            }
        }

        assert_eq!(has_block_count, requests.len());
        assert_eq!(received_tips, expected_tips);
    }

    #[tokio::test]
    async fn stale_validator_should_stay_running_and_request_tips_and_local_finalization() {
        let fixture = TestFixture::new().await;
        let engine_cell = Arc::new(EngineCell::init());

        fixture.transport_layer.reset();
        fixture
            .transport_layer
            .set_responses(|_peer, _protocol| Ok(()));

        let mut stale_block = fixture.genesis.clone();
        stale_block.block_hash = Bytes::from_static(b"stale-validator-block");
        stale_block.sender = fixture.validator_id.public_key.bytes.clone();
        stale_block.header.timestamp = 0;

        let mut casper = fixture.casper.clone();
        casper.insert_block(stale_block, false);

        let approved_block = ApprovedBlock {
            candidate: ApprovedBlockCandidate {
                block: fixture.genesis.clone(),
                required_sigs: 0,
            },
            sigs: Vec::new(),
            floor_seed: None,
        };

        let finalization_requests = Arc::new(AtomicUsize::new(0));
        let recovery_sync_active = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let running = Running::new(
            fixture.block_processing_queue_tx.clone(),
            fixture.blocks_in_processing.clone(),
            Arc::new(ValidatorAwareNoOpsCasper {
                inner: casper,
                validator_id: fixture.validator_id.clone(),
                finalization_requests: finalization_requests.clone(),
                recovery_sync_active: recovery_sync_active.clone(),
            }) as Arc<dyn MultiParentCasper + Send + Sync>,
            approved_block,
            Arc::new(|| {
                Box::pin(async { Ok(()) })
                    as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
            }),
            false,
            fixture.transport_layer.clone(),
            fixture.rp_conf_ask.clone(),
            fixture.block_retriever.clone(),
            Some(RunningRecoveryContext {
                connections_cell: fixture.connections_cell.clone(),
            }),
            None,
        );
        engine_cell.set(Arc::new(running)).await;

        update_fork_choice_tips_if_stuck(
            &engine_cell,
            &fixture.transport_layer,
            &fixture.connections_cell,
            &fixture.rp_conf_ask,
            Duration::from_secs(1),
        )
        .await
        .unwrap();

        let engine = engine_cell.get().await;
        assert!(
            engine.with_casper().is_some(),
            "stale validator recovery must preserve its live Casper instance"
        );
        assert_eq!(finalization_requests.load(Ordering::SeqCst), 1);
        assert!(recovery_sync_active.load(Ordering::SeqCst));

        let requests = fixture.transport_layer.get_all_requests();
        assert!(
            requests.iter().any(|request| {
                matches!(
                    &request.msg.message,
                    Some(models::routing::protocol::Message::Packet(packet))
                        if packet.type_id == "ForkChoiceTipRequest"
                )
            }),
            "recovery should request ordinary DAG tips from peers; requests: {:?}",
            requests.iter().map(|r| &r.msg).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn fresh_validator_should_stay_running_without_approved_block_request() {
        let fixture = TestFixture::new().await;
        let engine_cell = Arc::new(EngineCell::init());

        fixture.transport_layer.reset();
        fixture
            .transport_layer
            .set_responses(|_peer, _protocol| Ok(()));

        let mut fresh_block = fixture.genesis.clone();
        fresh_block.block_hash = Bytes::from_static(b"fresh-validator-block");
        fresh_block.sender = fixture.validator_id.public_key.bytes.clone();
        fresh_block.header.timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let mut casper = fixture.casper.clone();
        casper.insert_block(fresh_block, false);

        let approved_block = ApprovedBlock {
            candidate: ApprovedBlockCandidate {
                block: fixture.genesis.clone(),
                required_sigs: 0,
            },
            sigs: Vec::new(),
            floor_seed: None,
        };

        let finalization_requests = Arc::new(AtomicUsize::new(0));
        let recovery_sync_active = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let running = Running::new(
            fixture.block_processing_queue_tx.clone(),
            fixture.blocks_in_processing.clone(),
            Arc::new(ValidatorAwareNoOpsCasper {
                inner: casper,
                validator_id: fixture.validator_id.clone(),
                finalization_requests: finalization_requests.clone(),
                recovery_sync_active: recovery_sync_active.clone(),
            }) as Arc<dyn MultiParentCasper + Send + Sync>,
            approved_block,
            Arc::new(|| {
                Box::pin(async { Ok(()) })
                    as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
            }),
            false,
            fixture.transport_layer.clone(),
            fixture.rp_conf_ask.clone(),
            fixture.block_retriever.clone(),
            Some(RunningRecoveryContext {
                connections_cell: fixture.connections_cell.clone(),
            }),
            None,
        );
        engine_cell.set(Arc::new(running)).await;

        update_fork_choice_tips_if_stuck(
            &engine_cell,
            &fixture.transport_layer,
            &fixture.connections_cell,
            &fixture.rp_conf_ask,
            Duration::from_secs(60),
        )
        .await
        .unwrap();

        let engine = engine_cell.get().await;
        assert!(
            engine.with_casper().is_some(),
            "fresh validator should remain in Running"
        );

        let requests = fixture.transport_layer.get_all_requests();
        assert!(
            !requests.iter().any(|request| {
                matches!(
                    &request.msg.message,
                    Some(models::routing::protocol::Message::Packet(packet))
                        if packet.type_id == "ForkChoiceTipRequest"
                )
            }),
            "fresh validator should not request recovery tips; requests: {:?}",
            requests.iter().map(|r| &r.msg).collect::<Vec<_>>()
        );
        assert_eq!(finalization_requests.load(Ordering::SeqCst), 0);
        assert!(!recovery_sync_active.load(Ordering::SeqCst));
    }
}
