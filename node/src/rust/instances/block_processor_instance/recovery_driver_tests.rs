use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::blocks::block_processing_queue::{
    BlockProcessingQueueReceiver, BlockProcessingQueueSender,
};
use casper::rust::casper::{CasperShardConf, MultiParentCasper};
use casper::rust::engine::block_retriever::BlockRetriever;
use casper::rust::engine::engine::noop;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::engine::multi_parent_casper::MultiParentCasperImpl;
use casper::rust::estimator::Estimator;
use casper::rust::finality::certificate::CertificateVerificationSchedule;
use casper::rust::finality::finalization_schedule::FinalizationSchedule;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use comm::rust::rp::connect::{Connections, ConnectionsCell};
use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
use models::rust::block_hash::BlockHashSerde;
use models::rust::block_implicits::get_random_block_default;
use models::rust::casper::protocol::casper_message::BlockMessage;
use proptest::prelude::*;
use prost::bytes::Bytes;
use prost::Message;
use rholang::rust::interpreter::external_services::ExternalServices;
use rspace_plus_plus::rspace::rspace::RSpaceStore;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;
use shared::rust::store::key_value_store::KeyValueStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use super::*;

#[path = "acknowledgment_tests.rs"]
mod acknowledgment_tests;

#[test]
fn retry_deadlines_preserve_active_demand_without_creating_successors() {
    for startup in [false, true] {
        for ordinary in [false, true] {
            for prior in [
                RecoveryWake::Idle,
                RecoveryWake::Work { proposal: false },
                RecoveryWake::Work { proposal: true },
                RecoveryWake::Stopped,
            ] {
                let after = prior.merge(retry_demand(startup, ordinary));
                if startup || ordinary {
                    assert_eq!(after, prior);
                } else {
                    assert_eq!(after, prior.merge(RecoveryWake::Work { proposal: false }));
                }
            }
        }
    }
}

fn block(key: u8) -> BlockMessage {
    let mut block = get_random_block_default();
    block.block_hash = Bytes::from(vec![key; 32]);
    block.header.parents_hash_list.clear();
    block.justifications.clear();
    block.header.finalized_floor = None;
    block.finalized_floor_certificate = None;
    assert!(casper::rust::util::proto_util::dependencies_hashes_of(&block).is_empty());
    block
}

struct Fixture {
    pass: OrdinaryPass,
    casper: Arc<MultiParentCasperImpl<TransportLayerStub>>,
    sender: BlockProcessingQueueSender,
    receiver: BlockProcessingQueueReceiver,
    processor: Arc<BlockProcessor<TransportLayerStub>>,
    cell: EngineCell,
    pending_policy: Arc<dyn KeyValueStore>,
}

impl Fixture {
    async fn new(count: usize, byte_limit: usize) -> Self {
        let (instance, _, _) = super::super::ownership_tests::fixture().await;
        let mut manager = InMemoryStoreManager::new();
        let buffer = CasperBufferKeyValueStorage::new_from_kvm(&mut manager)
            .await
            .unwrap();
        let pending_policy = manager
            .store(CasperBufferKeyValueStorage::PENDING_POLICY_NAMESPACE.into())
            .await
            .unwrap();
        let retriever = BlockRetriever::new(
            buffer.clone(),
            Arc::new(TransportLayerStub::new()),
            ConnectionsCell {
                peers: Arc::new(std::sync::Mutex::new(Connections::from_vec(Vec::new()))),
            },
            create_rp_conf_ask(
                PeerNode {
                    id: NodeIdentifier {
                        key: Bytes::from(vec![1; 32]),
                    },
                    endpoint: Endpoint {
                        host: "localhost".into(),
                        tcp_port: 40400,
                        udp_port: 40400,
                    },
                },
                None,
                None,
            ),
        );
        let runtime = RuntimeManager::create_with_store(
            RSpaceStore {
                history: Arc::new(InMemoryKeyValueStore::new()),
                roots: Arc::new(InMemoryKeyValueStore::new()),
                cold: Arc::new(InMemoryKeyValueStore::new()),
            },
            KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
            Arc::new(Default::default()),
            ExternalServices::noop(),
        );
        let casper = Arc::new(MultiParentCasperImpl {
            block_retriever: retriever,
            event_publisher: F1r3flyEvents::new(),
            runtime_manager: Arc::new(runtime),
            estimator: Estimator::apply(2, None),
            block_store: KeyValueBlockStore::create_from_kvm(&mut manager)
                .await
                .unwrap(),
            block_dag_storage: BlockDagKeyValueStorage::new(&mut manager).await.unwrap(),
            deploy_storage: Arc::new(parking_lot::Mutex::new(
                KeyValueDeployStorage::new(&mut manager).await.unwrap(),
            )),
            rejected_deploy_buffer: Arc::new(std::sync::Mutex::new(
                KeyValueRejectedDeployBuffer::new(&mut manager)
                    .await
                    .unwrap(),
            )),
            deploy_lifecycle: Arc::new(Default::default()),
            casper_buffer_storage: buffer,
            validator_id: None,
            casper_shard_conf: CasperShardConf::new(),
            approved_block: block(0),
            finalization_in_progress: Arc::new(Default::default()),
            recovery_sync_active: Arc::new(Default::default()),
            finalization_schedule: Arc::new(FinalizationSchedule::new(2)),
            certificate_verification_schedule: Arc::new(CertificateVerificationSchedule::new(2)),
            divergence_monitor: Arc::new(Default::default()),
            finalizer_task_in_progress: Arc::new(Default::default()),
            finalizer_task_queued: Arc::new(Default::default()),
            heartbeat_signal_ref: casper::rust::heartbeat_signal::new_heartbeat_signal_ref(),
            deploys_in_scope_cache: Arc::new(Default::default()),
            active_validators_cache: Arc::new(Default::default()),
        });
        let (sender, receiver) = BlockProcessingQueueSender::channel(3, byte_limit).unwrap();
        let context: Arc<dyn MultiParentCasper + Send + Sync> = casper.clone();
        let prepared = sender.recovery().startup().prepare(&context);
        let origin = prepared.handle();
        let cell = EngineCell::init();
        cell.set_running(Arc::new(noop()), sender.recovery(), prepared)
            .await
            .unwrap();
        let mut fixture = Self {
            pass: OrdinaryPass {
                origin,
                pass: RecoveryPass::new(count, true),
                pending: None,
            },
            casper,
            sender,
            receiver,
            processor: instance.block_processor.clone(),
            cell,
            pending_policy,
        };
        fixture.processor = acknowledgment_tests::processor_with_shared_tracker(&fixture);
        fixture
    }

    fn add(&self, block: &BlockMessage) {
        self.casper.block_store.put_block_message(block).unwrap();
        self.casper
            .casper_buffer_storage
            .add_relation(
                BlockHashSerde(block.block_hash.clone()),
                BlockHashSerde(block.block_hash.clone()),
            )
            .unwrap();
    }

    fn step(&mut self) -> Result<Step, CasperError> {
        let before = self.pass.pass.remaining();
        let retrying = self.pass.pending.is_some();
        let result = self.pass.step(&self.sender.downgrade(), &self.processor);
        self.pass.finish_attempt(result, before, retrying)
    }
}

#[tokio::test]
async fn final_candidate_survives_real_byte_rejection_and_is_reloaded() {
    let candidate = block(1);
    let mut occupying = candidate.clone();
    occupying.block_hash = Bytes::from(vec![2; 32]);
    let bytes = candidate.to_proto().encoded_len();
    let mut fixture = Fixture::new(1, bytes).await;
    fixture.add(&candidate);
    fixture
        .sender
        .try_enqueue(fixture.casper.clone(), occupying)
        .unwrap();
    let held = fixture.receiver.try_recv().unwrap();
    assert_eq!(fixture.sender.used_bytes(), bytes);
    for _ in 0..3 {
        assert!(matches!(fixture.step().unwrap(), Step::Parked));
        assert_eq!(fixture.pass.pass.remaining(), 0);
        assert_eq!(fixture.pass.pending.as_ref(), Some(&candidate.block_hash));
        assert!(!fixture.pass.complete());
        assert_eq!(fixture.sender.used_bytes(), bytes);
        assert!(!fixture.sender.identities().contains(&candidate.block_hash));
    }
    drop(held);
    assert!(matches!(fixture.step().unwrap(), Step::Advanced));
    assert!(fixture.pass.complete());
    assert!(fixture.pass.pass.proposal_ready());
    let admitted = fixture.receiver.try_recv().unwrap();
    assert_eq!(admitted.block.block_hash, candidate.block_hash);
    drop(admitted);
    assert_eq!(fixture.sender.used_bytes(), 0);
    assert!(matches!(fixture.step().unwrap(), Step::Complete));
}

#[tokio::test]
async fn candidate_removal_during_capacity_wait_finishes_without_false_admission() {
    let candidate = block(1);
    let bytes = candidate.to_proto().encoded_len();
    let mut fixture = Fixture::new(1, bytes).await;
    fixture.add(&candidate);
    let mut occupying = candidate.clone();
    occupying.block_hash = Bytes::from(vec![2; 32]);
    fixture
        .sender
        .try_enqueue(fixture.casper.clone(), occupying)
        .unwrap();
    let held = fixture.receiver.try_recv().unwrap();
    assert!(matches!(fixture.step().unwrap(), Step::Parked));
    fixture
        .casper
        .casper_buffer_storage
        .remove(BlockHashSerde(candidate.block_hash.clone()))
        .unwrap();
    drop(held);
    assert!(matches!(fixture.step().unwrap(), Step::Advanced));
    assert!(fixture.pass.complete());
    assert!(fixture.receiver.try_recv().is_err());
}

#[tokio::test]
async fn replacing_context_discards_pending_candidate_and_blocks_old_proposal() {
    let candidate = block(1);
    let bytes = candidate.to_proto().encoded_len();
    let mut fixture = Fixture::new(1, bytes).await;
    fixture.add(&candidate);
    let mut occupying = candidate.clone();
    occupying.block_hash = Bytes::from(vec![2; 32]);
    fixture
        .sender
        .try_enqueue(fixture.casper.clone(), occupying)
        .unwrap();
    let held = fixture.receiver.try_recv().unwrap();
    assert!(matches!(fixture.step().unwrap(), Step::Parked));
    fixture.cell.set(Arc::new(noop())).await;
    drop(held);
    assert!(matches!(fixture.step().unwrap(), Step::Complete));
    assert!(!fixture.pass.pass.proposal_ready());
    assert!(fixture.pass.pending.is_none());
    assert!(fixture.receiver.try_recv().is_err());
}

#[tokio::test]
async fn retry_error_does_not_consume_the_next_candidate_selection() {
    let mut fixture = Fixture::new(2, 1024 * 1024).await;
    let first = Bytes::from(vec![1; 32]);
    assert_eq!(fixture.pass.candidate(|| Some(first.clone())), Some(&first));
    assert_eq!(fixture.pass.pass.remaining(), 1);
    assert!(fixture
        .pass
        .finish_attempt(
            Err(CasperError::RuntimeError("candidate read failed".into())),
            1,
            true,
        )
        .is_err());
    assert_eq!(fixture.pass.pass.remaining(), 1);
    assert!(fixture.pass.pending.is_none());
    let second = Bytes::from(vec![2; 32]);
    assert_eq!(
        fixture.pass.candidate(|| Some(second.clone())),
        Some(&second)
    );
    fixture
        .pass
        .finish_attempt(Ok(Step::Advanced), 1, false)
        .unwrap();
    assert!(fixture.pass.complete());
    assert!(!fixture.pass.pass.proposal_ready());
}

#[tokio::test]
async fn generated_retry_histories_preserve_selection_and_completion_invariants() {
    let fixture = Fixture::new(0, 1024 * 1024).await;
    let origin = fixture.pass.origin.clone();
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases: 128,
        ..Default::default()
    });
    runner
        .run(
            &(
                0usize..128,
                any::<bool>(),
                prop::collection::vec(0u8..6, 0..512),
            ),
            |(budget, proposal, operations)| {
                let mut pass = OrdinaryPass {
                    origin: origin.clone(),
                    pass: RecoveryPass::new(budget, proposal),
                    pending: None,
                };
                let mut remaining = budget;
                let mut pending = None;
                let mut failed = false;
                let mut selections = 0usize;
                for operation in operations {
                    match operation {
                        0 => {
                            let hash = Bytes::from(selections.to_be_bytes().to_vec());
                            let expected = pending
                                .clone()
                                .or_else(|| (remaining > 0).then(|| hash.clone()));
                            let mut calls = 0;
                            let actual = pass
                                .candidate(|| {
                                    calls += 1;
                                    Some(hash.clone())
                                })
                                .cloned();
                            prop_assert_eq!(actual, expected);
                            if pending.is_none() && remaining > 0 {
                                remaining -= 1;
                                selections += 1;
                                pending = Some(hash);
                                prop_assert_eq!(calls, 1);
                            } else {
                                prop_assert_eq!(calls, 0);
                            }
                        }
                        1 => {
                            pass.finish_attempt(Ok(Step::Parked), remaining, pending.is_some())
                                .unwrap();
                        }
                        2 => {
                            pass.finish_attempt(Ok(Step::Advanced), remaining, pending.is_some())
                                .unwrap();
                            pending = None;
                        }
                        3 => {
                            prop_assert!(pass
                                .finish_attempt(
                                    Err(CasperError::RuntimeError("read failed".into())),
                                    remaining,
                                    pending.is_some(),
                                )
                                .is_err());
                            if pending.is_none() && remaining > 0 {
                                remaining -= 1;
                            }
                            pending = None;
                            failed = true;
                        }
                        4 => {
                            prop_assert_eq!(pass.complete(), remaining == 0 && pending.is_none());
                        }
                        _ => {
                            let mut called = false;
                            let actual = pass
                                .candidate(|| {
                                    called = true;
                                    None
                                })
                                .cloned();
                            prop_assert_eq!(actual, pending.clone());
                            if pending.is_none() && remaining > 0 {
                                remaining -= 1;
                                selections += 1;
                                prop_assert!(called);
                            } else {
                                prop_assert!(!called);
                            }
                        }
                    }
                    prop_assert_eq!(pass.pass.remaining(), remaining);
                    prop_assert_eq!(&pass.pending, &pending);
                    prop_assert_eq!(pass.complete(), remaining == 0 && pending.is_none());
                    prop_assert_eq!(
                        pass.complete() && pass.pass.proposal_ready(),
                        remaining == 0 && pending.is_none() && proposal && !failed
                    );
                    prop_assert!(selections <= budget);
                }
                Ok(())
            },
        )
        .unwrap();
}
