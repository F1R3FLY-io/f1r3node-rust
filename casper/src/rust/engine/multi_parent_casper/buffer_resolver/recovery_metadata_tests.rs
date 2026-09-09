use std::sync::Arc;

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use block_storage::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use comm::rust::rp::connect::{Connections, ConnectionsCell};
use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::block_metadata::BlockMetadata;
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond};
use prost::bytes::Bytes;
use rholang::rust::interpreter::external_services::ExternalServices;
use rspace_plus_plus::rspace::rspace::RSpaceStore;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;
use shared::rust::store::key_value_store::KeyValueStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use super::MultiParentCasperImpl;
use crate::rust::casper::{Casper, CasperShardConf, MultiParentCasper, RetryCandidate};
use crate::rust::engine::block_retriever::BlockRetriever;
use crate::rust::estimator::Estimator;
use crate::rust::finality::certificate::CertificateVerificationSchedule;
use crate::rust::finality::finalization_schedule::FinalizationSchedule;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

fn hash(value: u8) -> BlockHash { Bytes::from(vec![value; models::rust::block_hash::LENGTH]) }

fn block(value: u8, number: i64, parents: Vec<BlockHash>) -> BlockMessage {
    let validator = Bytes::from(vec![7; models::rust::validator::LENGTH]);
    let mut block = models::rust::block_implicits::get_random_block(
        Some(number),
        Some(i32::try_from(number).unwrap()),
        Some(hash(20)),
        Some(hash(21)),
        Some(validator.clone()),
        Some(models::rust::block_metadata::CERTIFIED_ADMISSION_PROTOCOL_VERSION),
        Some(number),
        Some(parents),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(vec![Bond {
            validator,
            stake: 1,
        }]),
        Some("root".into()),
        None,
    );
    block.block_hash = hash(value);
    block.header.sender_bond_generation = Some(BondGeneration::GENESIS);
    if let Some(certificate) = &mut block.finalized_floor_certificate {
        certificate.exact_latest_messages.clear();
        for parent in &block.header.parents_hash_list {
            certificate.exact_latest_messages.insert(
                models::rust::validator::ValidatorSerde(block.sender.clone()),
                BlockHashSerde(parent.clone()),
            );
        }
        block.header.finalized_floor =
            Some(certificate.commitment(certificate.authority_context_digest.0.clone()));
    }
    let dependencies: std::collections::BTreeSet<_> =
        crate::rust::util::proto_util::dependencies_hashes_of(&block)
            .into_iter()
            .collect();
    assert_eq!(
        dependencies,
        block.header.parents_hash_list.iter().cloned().collect()
    );
    block
}

async fn fixture() -> (
    MultiParentCasperImpl<TransportLayerStub>,
    Arc<dyn KeyValueStore>,
) {
    let mut manager = InMemoryStoreManager::new();
    let buffer = CasperBufferKeyValueStorage::new_from_kvm(&mut manager)
        .await
        .unwrap();
    let block_store = KeyValueBlockStore::create_from_kvm(&mut manager)
        .await
        .unwrap();
    let block_dag_storage = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
    let approved_block = block(0, 0, Vec::new());
    block_store.put_block_message(&approved_block).unwrap();
    block_dag_storage
        .insert(&approved_block, InsertMode::ApprovedGenesis)
        .unwrap();
    let local = PeerNode {
        id: NodeIdentifier { key: hash(8) },
        endpoint: Endpoint {
            host: "localhost".into(),
            tcp_port: 40400,
            udp_port: 40400,
        },
    };
    let block_retriever = BlockRetriever::new(
        buffer.clone(),
        Arc::new(TransportLayerStub::new()),
        ConnectionsCell {
            peers: Arc::new(std::sync::Mutex::new(Connections::from_vec(Vec::new()))),
        },
        create_rp_conf_ask(local, None, None),
    );
    let runtime_manager = RuntimeManager::create_with_store(
        RSpaceStore {
            history: Arc::new(InMemoryKeyValueStore::new()),
            roots: Arc::new(InMemoryKeyValueStore::new()),
            cold: Arc::new(InMemoryKeyValueStore::new()),
        },
        KeyValueTypedStoreImpl::new(Arc::new(InMemoryKeyValueStore::new())),
        Arc::new(Default::default()),
        ExternalServices::noop(),
    );
    let casper = MultiParentCasperImpl {
        block_retriever,
        event_publisher: F1r3flyEvents::new(),
        runtime_manager: Arc::new(runtime_manager),
        estimator: Estimator::apply(2, None),
        block_store,
        block_dag_storage,
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
        approved_block,
        finalization_in_progress: Arc::new(Default::default()),
        recovery_sync_active: Arc::new(Default::default()),
        finalization_schedule: Arc::new(FinalizationSchedule::new(2)),
        certificate_verification_schedule: Arc::new(CertificateVerificationSchedule::new(2)),
        divergence_monitor: Arc::new(Default::default()),
        finalizer_task_in_progress: Arc::new(Default::default()),
        finalizer_task_queued: Arc::new(Default::default()),
        heartbeat_signal_ref: crate::rust::heartbeat_signal::new_heartbeat_signal_ref(),
        deploys_in_scope_cache: Arc::new(Default::default()),
        active_validators_cache: Arc::new(Default::default()),
    };
    (
        casper,
        manager.store("block-metadata".into()).await.unwrap(),
    )
}

#[tokio::test]
async fn actual_resolver_propagates_corruption_after_a_missing_dependency() {
    let (casper, raw) = fixture().await;
    let admitted = block(1, 1, vec![hash(0)]);
    casper
        .block_dag_storage
        .insert(&admitted, InsertMode::Normal)
        .unwrap();
    let candidate = block(3, 2, vec![hash(2), admitted.block_hash.clone()]);
    casper.block_store.put_block_message(&candidate).unwrap();
    casper
        .casper_buffer_storage
        .add_relation(
            BlockHashSerde(hash(2)),
            BlockHashSerde(candidate.block_hash.clone()),
        )
        .unwrap();
    let typed = KeyValueTypedStoreImpl::<BlockHashSerde, BlockMetadata>::new(raw.clone());
    let key = typed
        .encode_key(&BlockHashSerde(admitted.block_hash.clone()))
        .unwrap();
    let saved = raw.get_one(&key).unwrap().unwrap();
    raw.put_one(key.clone(), vec![0xff]).unwrap();
    assert!(casper.get_dependency_free_hashes_from_buffer().is_err());
    assert!(casper.get_dependency_free_from_buffer().is_err());
    assert!(casper
        .prepare_retry_candidate(&candidate.block_hash)
        .is_err());
    raw.put_one(key, saved).unwrap();
    assert!(casper
        .get_dependency_free_hashes_from_buffer()
        .unwrap()
        .is_empty());
    assert!(casper.get_dependency_free_from_buffer().unwrap().is_empty());
    assert!(matches!(
        casper
            .prepare_retry_candidate(&candidate.block_hash)
            .unwrap(),
        RetryCandidate::MissingMetadata
    ));
    let missing = block(2, 1, vec![hash(0)]);
    casper
        .block_dag_storage
        .insert(&missing, InsertMode::Normal)
        .unwrap();
    assert_eq!(
        casper.get_dependency_free_hashes_from_buffer().unwrap(),
        vec![candidate.block_hash.clone()]
    );
    let selected = casper.get_dependency_free_from_buffer().unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].block_hash, candidate.block_hash);
    let RetryCandidate::Ready(selected) = casper
        .prepare_retry_candidate(&candidate.block_hash)
        .unwrap()
    else {
        panic!("admitted dependencies must permit a single candidate");
    };
    assert_eq!(selected.block_hash, candidate.block_hash);
}

#[tokio::test]
async fn single_candidate_preparation_distinguishes_membership_body_and_admission() {
    let (casper, _) = fixture().await;
    let candidate = block(1, 1, vec![hash(0)]);
    let key = BlockHashSerde(candidate.block_hash.clone());
    assert!(matches!(
        casper.prepare_retry_candidate(&key.0).unwrap(),
        RetryCandidate::Absent
    ));
    casper
        .casper_buffer_storage
        .put_pendant(key.clone())
        .unwrap();
    assert!(matches!(
        casper.prepare_retry_candidate(&key.0).unwrap(),
        RetryCandidate::MissingBody
    ));
    casper
        .block_dag_storage
        .insert(&candidate, InsertMode::Normal)
        .unwrap();
    assert!(matches!(
        casper.prepare_retry_candidate(&key.0).unwrap(),
        RetryCandidate::AlreadyAdmitted
    ));
    casper.casper_buffer_storage.remove(key.clone()).unwrap();
    assert!(matches!(
        casper.prepare_retry_candidate(&key.0).unwrap(),
        RetryCandidate::Absent
    ));
    assert_eq!(casper.retry_candidate_count(), 0);
    assert_eq!(casper.next_retry_candidate(), None);
}

#[tokio::test]
async fn single_candidate_metadata_errors_are_not_treated_as_absence() {
    let (casper, raw) = fixture().await;
    let candidate = block(1, 1, vec![hash(0)]);
    casper
        .casper_buffer_storage
        .put_pendant(BlockHashSerde(candidate.block_hash.clone()))
        .unwrap();
    casper
        .block_dag_storage
        .insert(&candidate, InsertMode::Normal)
        .unwrap();
    let typed = KeyValueTypedStoreImpl::<BlockHashSerde, BlockMetadata>::new(raw.clone());
    let key = typed
        .encode_key(&BlockHashSerde(candidate.block_hash.clone()))
        .unwrap();
    let saved = raw.get_one(&key).unwrap().unwrap();
    raw.put_one(key.clone(), vec![0xff]).unwrap();
    assert!(casper
        .prepare_retry_candidate(&candidate.block_hash)
        .is_err());
    raw.put_one(key, saved).unwrap();
    assert!(matches!(
        casper
            .prepare_retry_candidate(&candidate.block_hash)
            .unwrap(),
        RetryCandidate::AlreadyAdmitted
    ));
}

#[tokio::test]
async fn startup_preparation_preserves_captured_membership_and_worker_dependency_checks() {
    let (casper, _) = fixture().await;
    let candidate = block(1, 1, vec![hash(2)]);
    casper.block_store.put_block_message(&candidate).unwrap();
    assert!(matches!(
        casper
            .prepare_retry_candidate(&candidate.block_hash)
            .unwrap(),
        RetryCandidate::Absent
    ));
    assert!(matches!(
        casper
            .prepare_startup_candidate(&candidate.block_hash)
            .unwrap(),
        RetryCandidate::Ready(_)
    ));
    casper
        .casper_buffer_storage
        .add_certificate_relation(
            BlockHashSerde(hash(9)),
            BlockHashSerde(candidate.block_hash.clone()),
        )
        .unwrap();
    assert!(matches!(
        casper
            .prepare_retry_candidate(&candidate.block_hash)
            .unwrap(),
        RetryCandidate::WaitingCertificate
    ));
    assert!(matches!(
        casper
            .prepare_startup_candidate(&candidate.block_hash)
            .unwrap(),
        RetryCandidate::Ready(_)
    ));
}

#[tokio::test]
async fn startup_reads_the_body_before_admitted_metadata() {
    let (casper, raw) = fixture().await;
    let candidate = block(1, 1, vec![hash(0)]);
    casper
        .block_dag_storage
        .insert(&candidate, InsertMode::Normal)
        .unwrap();
    assert!(matches!(
        casper
            .prepare_startup_candidate(&candidate.block_hash)
            .unwrap(),
        RetryCandidate::MissingBody
    ));
    let typed = KeyValueTypedStoreImpl::<BlockHashSerde, BlockMetadata>::new(raw.clone());
    let key = typed
        .encode_key(&BlockHashSerde(candidate.block_hash.clone()))
        .unwrap();
    raw.put_one(key, vec![0xff]).unwrap();
    assert!(matches!(
        casper
            .prepare_startup_candidate(&candidate.block_hash)
            .unwrap(),
        RetryCandidate::MissingBody
    ));
    casper.block_store.put_block_message(&candidate).unwrap();
    assert!(casper
        .prepare_startup_candidate(&candidate.block_hash)
        .is_err());
}

#[tokio::test]
async fn generated_actual_resolver_rotation_matches_live_fifo_membership() {
    use std::collections::VecDeque;

    use proptest::prelude::*;

    let (casper, _) = fixture().await;
    let mut runner = proptest::test_runner::TestRunner::new(proptest::test_runner::Config {
        cases: 64,
        failure_persistence: None,
        ..Default::default()
    });
    runner
        .run(
            &prop::collection::vec((0u8..3, 1u8..64), 0..256),
            |operations| {
                let mut expected = VecDeque::new();
                for (operation, value) in operations {
                    let key = hash(value);
                    match operation {
                        0 => {
                            casper
                                .casper_buffer_storage
                                .put_pendant(BlockHashSerde(key.clone()))
                                .unwrap();
                            if !expected.contains(&key) {
                                expected.push_back(key);
                            }
                        }
                        1 => {
                            casper
                                .casper_buffer_storage
                                .remove(BlockHashSerde(key.clone()))
                                .unwrap();
                            expected.retain(|entry| entry != &key);
                        }
                        _ => {
                            let next = expected.pop_front();
                            if let Some(key) = next.as_ref() {
                                expected.push_back(key.clone());
                            }
                            prop_assert_eq!(casper.next_retry_candidate(), next);
                        }
                    }
                    prop_assert_eq!(casper.retry_candidate_count(), expected.len());
                }
                for key in expected {
                    casper
                        .casper_buffer_storage
                        .remove(BlockHashSerde(key))
                        .unwrap();
                }
                prop_assert_eq!(casper.retry_candidate_count(), 0);
                Ok(())
            },
        )
        .unwrap();
}

#[tokio::test]
async fn actual_resolver_requires_metadata_rows_and_preserves_certificate_waiting() {
    let (casper, raw) = fixture().await;
    let candidate = block(1, 1, vec![hash(0)]);
    casper
        .casper_buffer_storage
        .put_pendant(BlockHashSerde(candidate.block_hash.clone()))
        .unwrap();
    assert!(casper
        .get_dependency_free_hashes_from_buffer()
        .unwrap()
        .is_empty());
    casper.block_store.put_block_message(&candidate).unwrap();
    let typed = KeyValueTypedStoreImpl::<BlockHashSerde, BlockMetadata>::new(raw.clone());
    let key = typed.encode_key(&BlockHashSerde(hash(0))).unwrap();
    let saved = raw.get_one(&key).unwrap().unwrap();
    raw.delete(vec![key.clone()]).unwrap();
    assert!(casper
        .get_dependency_free_hashes_from_buffer()
        .unwrap()
        .is_empty());
    raw.put_one(key, saved).unwrap();
    assert_eq!(
        casper.get_dependency_free_hashes_from_buffer().unwrap(),
        vec![candidate.block_hash.clone()]
    );
    casper
        .casper_buffer_storage
        .add_certificate_relation(
            BlockHashSerde(hash(9)),
            BlockHashSerde(candidate.block_hash),
        )
        .unwrap();
    assert!(casper
        .get_dependency_free_hashes_from_buffer()
        .unwrap()
        .is_empty());
    assert!(casper.get_dependency_free_from_buffer().unwrap().is_empty());
    assert!(matches!(
        casper.prepare_retry_candidate(&hash(1)).unwrap(),
        RetryCandidate::WaitingCertificate
    ));
}
