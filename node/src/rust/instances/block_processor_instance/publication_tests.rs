use std::any::Any;
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, CertifiedAdmissionOutcome, CertifiedSenderAuthority, InsertMode,
};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::blocks::block_processor::BlockProcessorDependencies;
use casper::rust::casper::test_helpers::TestCasperWithSnapshot;
use casper::rust::casper::CURRENT_CASPER_PROTOCOL_VERSION;
use casper::rust::engine::block_retriever::{AdmitHashReason, BlockRetriever};
use casper::rust::validate::Validate;
use casper::rust::validator_identity::ValidatorIdentity;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use comm::rust::rp::connect::{Connections, ConnectionsCell};
use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
use crypto::rust::private_key::PrivateKey;
use models::rust::block_hash::BlockHashSerde;
use models::rust::block_implicits::get_random_block_default;
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::FinalizedFloorCommitment;
use proptest::prelude::*;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use shared::rust::store::key_value_store::{
    AtomicStoreMutation, EntryReader, KeyValueStore, KvStoreError, ValueReader,
};
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::ByteBuffer;

use super::*;

mod evidence_tests;

#[derive(Clone)]
struct PublicationStore {
    inner: Arc<InMemoryKeyValueStore>,
    fail_on_write: usize,
    writes: Arc<AtomicUsize>,
    failures: Arc<AtomicUsize>,
    gate: Option<Arc<PublicationGate>>,
    read_hook: Option<Arc<dyn Fn() + Send + Sync>>,
    write_hook: Option<Arc<dyn Fn() + Send + Sync>>,
}

struct PublicationGate {
    closed: AtomicBool,
    failed: tokio::sync::Notify,
}

impl KeyValueStore for PublicationStore {
    fn as_any(&self) -> &dyn Any { self }

    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        if let Some(hook) = &self.read_hook {
            hook();
        }
        self.inner.get(keys)
    }

    fn with_value(
        &self,
        key: &ByteBuffer,
        reader: &mut ValueReader<'_>,
    ) -> Result<(), KvStoreError> {
        if let Some(hook) = &self.read_hook {
            hook();
        }
        self.inner.with_value(key, reader)
    }

    fn visit_entries(&self, reader: &mut EntryReader<'_>) -> Result<(), KvStoreError> {
        self.inner.visit_entries(reader)
    }

    fn put(&self, pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        if self
            .gate
            .as_ref()
            .is_some_and(|gate| gate.closed.load(Ordering::SeqCst))
        {
            return Err(KvStoreError::IoError(
                "injected block storage failure".into(),
            ));
        }
        self.inner.put(pairs)?;
        if let Some(hook) = &self.write_hook {
            hook();
        }
        Ok(())
    }

    fn put_one_if_absent(&self, key: ByteBuffer, value: ByteBuffer) -> Result<bool, KvStoreError> {
        self.inner.put_one_if_absent(key, value)
    }

    fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        self.inner.delete(keys)
    }

    fn iterate(&self, visitor: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
        self.inner.iterate(visitor)
    }

    fn iterate_while(
        &self,
        visitor: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        self.inner.iterate_while(visitor)
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        self.inner.to_map()
    }

    fn print_store(&self) -> Result<(), KvStoreError> { self.inner.print_store() }

    fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }

    fn size_bytes(&self) -> usize { self.inner.size_bytes() }

    fn strict_atomic_mutate(
        &self,
        mutations: &[AtomicStoreMutation<'_>],
    ) -> Result<(), KvStoreError> {
        let selected_failure = self.writes.fetch_add(1, Ordering::SeqCst) + 1 == self.fail_on_write;
        if selected_failure
            || self
                .gate
                .as_ref()
                .is_some_and(|gate| gate.closed.load(Ordering::SeqCst))
        {
            self.failures.fetch_add(1, Ordering::SeqCst);
            if let Some(gate) = &self.gate {
                gate.failed.notify_one();
            }
            return Err(KvStoreError::IoError(
                "injected parents-map commit failure".into(),
            ));
        }
        let inner = mutations
            .iter()
            .map(|mutation| AtomicStoreMutation {
                store: if std::ptr::eq(mutation.store.as_any(), self.as_any()) {
                    self.inner.as_ref()
                } else {
                    mutation.store
                },
                key: mutation.key.clone(),
                operation: mutation.operation.clone(),
            })
            .collect::<Vec<_>>();
        self.inner.strict_atomic_mutate(&inner)
    }
}

async fn assert_worker_publication_ownership(
    fail_on_write: usize,
    tracker_full: bool,
    conflicting_payload: Option<(u8, u8)>,
) {
    assert_worker_publication_with_history(
        fail_on_write,
        tracker_full,
        conflicting_payload,
        false,
        false,
    )
    .await;
}

async fn assert_worker_publication_with_history(
    fail_on_write: usize,
    tracker_full: bool,
    conflicting_payload: Option<(u8, u8)>,
    requested_history: bool,
    restart_history: bool,
) {
    assert_worker_publication_with_retirement(
        fail_on_write,
        tracker_full,
        conflicting_payload,
        requested_history,
        restart_history,
        false,
    )
    .await;
}

fn publication_blocks() -> (BlockMessage, BlockMessage) {
    let mut genesis = get_random_block_default();
    genesis.header.version = CURRENT_CASPER_PROTOCOL_VERSION;
    genesis.header.parents_hash_list.clear();
    genesis.header.objective_equivocation_evidence_delta.clear();
    genesis.header.finalized_floor = None;
    genesis.body.state.block_number = 0;
    genesis.body.deploys.clear();
    genesis.body.system_deploys.clear();
    genesis.body.state.bonds.clear();
    genesis.body.state.bond_generations.clear();
    genesis.justifications.clear();
    genesis.finalized_floor_certificate = None;
    genesis.shard_id = "publication-regression".into();
    genesis.block_hash = genesis.computed_block_hash();
    let mut candidate = genesis.clone();
    candidate.body.state.block_number = 1;
    candidate.header.parents_hash_list = vec![Bytes::from(vec![21; 32])];
    candidate.header.sender_bond_generation = Some(BondGeneration::GENESIS);
    candidate.header.finalized_floor = Some(FinalizedFloorCommitment {
        floor_hash: genesis.block_hash.clone(),
        floor_post_state_hash: genesis.body.state.post_state_hash.clone(),
        certificate_digest: Bytes::from(vec![22; 32]),
        authority_context_digest: Bytes::from(vec![23; 32]),
    });
    candidate.seq_num = 1;
    let candidate =
        ValidatorIdentity::new(&PrivateKey::from_bytes(&[1; 32])).sign_block(&candidate);
    assert!(Validate::format_of_fields(&candidate));
    assert!(Validate::block_signature(&candidate));
    assert!(candidate.has_valid_content_hash());
    (genesis, candidate)
}

async fn assert_worker_publication_with_retirement(
    fail_on_write: usize,
    tracker_full: bool,
    conflicting_payload: Option<(u8, u8)>,
    requested_history: bool,
    restart_history: bool,
    exercise_retirement: bool,
) {
    let mut manager = InMemoryStoreManager::new();
    let blocks = KeyValueBlockStore::create_from_kvm(&mut manager)
        .await
        .unwrap();
    let dag = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
    let (mut genesis, candidate) = publication_blocks();
    dag.insert(&genesis, InsertMode::ApprovedGenesis).unwrap();
    blocks.put_block_message(&genesis).unwrap();

    let failures = Arc::new(AtomicUsize::new(0));
    let gate = exercise_retirement.then(|| {
        Arc::new(PublicationGate {
            closed: AtomicBool::new(true),
            failed: tokio::sync::Notify::new(),
        })
    });
    let coordinator = Arc::new(std::sync::RwLock::new(()));
    let pending: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new_with_coordinator(
        coordinator.clone(),
    ));
    let durable: KeyValueTypedStoreImpl<BlockHashSerde, HashSet<BlockHashSerde>> =
        KeyValueTypedStoreImpl::new(Arc::new(PublicationStore {
            inner: Arc::new(InMemoryKeyValueStore::new_with_coordinator(coordinator)),
            fail_on_write,
            writes: Arc::new(AtomicUsize::new(0)),
            failures: failures.clone(),
            gate: gate.clone(),
            read_hook: None,
            write_hook: None,
        }));
    let buffer = CasperBufferKeyValueStorage::new_from_kv_store(durable.clone(), pending.clone())
        .await
        .unwrap();
    let transport = Arc::new(TransportLayerStub::new());
    let connections = ConnectionsCell {
        peers: Arc::new(std::sync::Mutex::new(Connections::from_vec(Vec::new()))),
    };
    let conf = create_rp_conf_ask(
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
    );
    let retriever = BlockRetriever::new(
        buffer.clone(),
        transport.clone(),
        connections.clone(),
        conf.clone(),
    );
    if tracker_full {
        for index in 0u32..2048 {
            let mut bytes = vec![0; 32];
            bytes[..4].copy_from_slice(&index.to_be_bytes());
            retriever.ack_receive(Bytes::from(bytes)).await.unwrap();
        }
    }
    let dependencies = BlockProcessorDependencies::new(
        blocks.clone(),
        dag.clone(),
        retriever.clone(),
        transport.clone(),
        connections.clone(),
        conf.clone(),
        None,
    )
    .unwrap();
    let processor = Arc::new(BlockProcessor::new(dependencies.clone()));
    let hash = candidate.block_hash.clone();
    if requested_history {
        genesis.body.state.block_number = 2;
        genesis.block_hash = genesis.computed_block_hash();
    }
    let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(TestCasperWithSnapshot::new(
        TestCasperWithSnapshot::create_empty_snapshot(),
        genesis,
    ));
    if requested_history {
        assert!(!processor
            .check_if_of_interest(casper.clone(), &candidate)
            .unwrap());
        retriever
            .admit_hash(
                hash.clone(),
                None,
                AdmitHashReason::MissingDependencyRequested,
            )
            .await
            .unwrap();
        assert!(retriever.was_requested_as_dependency(&hash).unwrap());
    }
    assert!(processor
        .check_if_of_interest(casper.clone(), &candidate)
        .unwrap());
    let key = BlockHashSerde(hash.clone());
    assert!(durable.get_one(&key).unwrap().is_none());
    assert!(!processor
        .restore_stored_buffer_ownership(casper.clone(), &hash)
        .await
        .unwrap());
    assert!(durable.get_one(&key).unwrap().is_none());
    retriever.ack_receive(hash.clone()).await.unwrap();
    assert_eq!(
        retriever.request_states().contains_key(&hash),
        !tracker_full
    );

    if fail_on_write == 2 || fail_on_write == 3 {
        processor
            .check_if_well_formed_and_store(&candidate)
            .await
            .unwrap();
        if fail_on_write == 3 {
            buffer
                .add_relation(BlockHashSerde(Bytes::from(vec![21; 32])), key.clone())
                .unwrap();
        } else {
            let _result = dependencies.commit_to_buffer(&candidate, Some(HashSet::from([
            casper::rust::blocks::block_processor::CasperDependency::Block(Bytes::from(vec![21; 32])),
            casper::rust::blocks::block_processor::CasperDependency::FinalizationCertificate(Bytes::from(vec![22; 32])),
        ]))).await;
        }
        processor.note_validation_failure(&hash).unwrap();
        assert!(processor.is_validation_failure_quarantined(&hash).unwrap());
    }

    let mut incoming = candidate.clone();
    if let Some((parent, certificate)) = conflicting_payload {
        processor
            .check_if_well_formed_and_store(&candidate)
            .await
            .unwrap();
        processor.note_validation_failure(&hash).unwrap();
        incoming.header.parents_hash_list = vec![Bytes::from(vec![parent; 32])];
        incoming
            .header
            .finalized_floor
            .as_mut()
            .unwrap()
            .certificate_digest = Bytes::from(vec![certificate; 32]);
        assert_eq!(incoming.block_hash, hash);
        assert!(!incoming.has_valid_content_hash());
    }

    let (sender, mut receiver) = BlockProcessingQueueSender::channel(1, 1024 * 1024).unwrap();
    sender
        .try_enqueue_with_receipt(casper.clone(), incoming, |hash| {
            retriever.record_received(hash.clone()).map(|_| ())
        })
        .unwrap();
    let item = receiver.try_recv().unwrap();
    if let Some(gate) = &gate {
        let worker = tokio::spawn(process_owned_block(
            processor.clone(),
            item,
            sender.identities(),
            sender.recovery().signal(),
        ));
        tokio::time::timeout(Duration::from_secs(10), gate.failed.notified())
            .await
            .unwrap();
        let mut peer = conf.local.clone();
        peer.endpoint.tcp_port += 1;
        *connections.peers.lock().unwrap() = Connections::from_vec(vec![peer]);
        transport.set_response_delay(Duration::from_millis(501));
        let before_requests = transport.request_count();
        for _ in 0..32 {
            retriever.recover_dependency(hash.clone()).await.unwrap();
        }
        assert_eq!(transport.request_count() - before_requests, 32);
        retriever.recover_dependency(hash.clone()).await.unwrap();
        assert_eq!(transport.request_count() - before_requests, 32);
        if let Some(state) = retriever.request_state(&hash).unwrap() {
            assert!(state.retry_budget_quarantine_until.is_some());
        }
        assert!(blocks.contains_stored_block(&hash).unwrap());
        let pending_owner = matches!(
            processor.retry_ownership(&hash).unwrap(),
            casper::rust::blocks::block_processor::RetryOwnership::Pending
        );
        assert!(
            !sender.identities().is_empty()
                || pending_owner
                || retriever.request_states().contains_key(&hash),
            "retry retirement must not leave an accepted stored block without an owner"
        );
        gate.closed.store(false, Ordering::SeqCst);
        tokio::time::timeout(Duration::from_secs(10), worker)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            processor.retry_ownership(&hash).unwrap(),
            casper::rust::blocks::block_processor::RetryOwnership::Pending
        ));
    } else {
        process_owned_block(
            processor.clone(),
            item,
            sender.identities(),
            sender.recovery().signal(),
        )
        .await;
    }

    if !exercise_retirement && fail_on_write <= 1 {
        assert_eq!(
            failures.load(Ordering::SeqCst),
            usize::from(fail_on_write == 1)
        );
    }
    assert!(blocks.contains_stored_block(&hash).unwrap());
    assert!(!dag.get_representation().unwrap().contains(&hash));
    assert_eq!(sender.used_bytes(), 0);
    assert!(sender.identities().is_empty());
    let durable_owner = durable.get_one(&key).unwrap().is_some();
    let requested_owner = retriever.request_states().contains_key(&hash);
    {
        assert!(
            durable_owner,
            "an untracked worker must retry failed publication before releasing its last owner"
        );
        let dependencies = durable.get_one(&key).unwrap().unwrap();
        let mut certificate_key = vec![0xff];
        certificate_key.extend_from_slice(&[22; 32]);
        assert_eq!(
            dependencies,
            HashSet::from([
                BlockHashSerde(Bytes::from(vec![21; 32])),
                BlockHashSerde(Bytes::from(certificate_key)),
            ]),
            "worker release must preserve every missing dependency"
        );
        if requested_history {
            let processor = if restart_history {
                let restored_buffer = CasperBufferKeyValueStorage::new_from_kv_store(
                    durable.clone(),
                    pending.clone(),
                )
                .await
                .unwrap();
                let restored_retriever = BlockRetriever::new(
                    restored_buffer.clone(),
                    transport.clone(),
                    connections.clone(),
                    conf.clone(),
                );
                assert!(restored_retriever
                    .was_requested_as_dependency(&hash)
                    .unwrap());
                Arc::new(BlockProcessor::new(
                    BlockProcessorDependencies::new(
                        blocks,
                        dag,
                        restored_retriever,
                        transport,
                        connections,
                        conf,
                        None,
                    )
                    .unwrap(),
                ))
            } else {
                processor
            };
            assert!(
                processor.check_if_of_interest(casper, &candidate).unwrap(),
                "a pending solicited block must remain eligible below the approved height"
            );
        } else {
            assert!(
                !requested_owner,
                "successful publication permits acknowledgement"
            );
        }
    }
}

#[tokio::test]
async fn publication_failure_preserves_retry_owner_after_worker_release() {
    assert_worker_publication_ownership(1, false, None).await;
}

#[tokio::test]
async fn restoration_does_not_republish_after_interleaved_terminal_admission() {
    let mut manager = InMemoryStoreManager::new();
    let dag = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
    let buffer = CasperBufferKeyValueStorage::new_from_kvm(&mut manager)
        .await
        .unwrap();
    let (genesis, candidate) = publication_blocks();
    dag.insert(&genesis, InsertMode::ApprovedGenesis).unwrap();
    let certificate = CertifiedSenderAuthority::new(
        &candidate,
        candidate
            .header
            .finalized_floor
            .as_ref()
            .unwrap()
            .floor_hash
            .clone(),
        candidate
            .header
            .finalized_floor
            .as_ref()
            .unwrap()
            .floor_post_state_hash
            .clone(),
        candidate
            .header
            .finalized_floor
            .as_ref()
            .unwrap()
            .authority_context_digest
            .clone(),
        BondGeneration::GENESIS,
        1,
    )
    .unwrap();
    let outcome = CertifiedAdmissionOutcome::accepted(&candidate, &certificate).unwrap();
    let armed = Arc::new(AtomicBool::new(false));
    let admitted = Arc::new(AtomicBool::new(false));
    let read_hook = {
        let armed = armed.clone();
        let admitted = admitted.clone();
        let dag = dag.clone();
        let candidate = candidate.clone();
        Arc::new(move || {
            if armed.swap(false, Ordering::SeqCst) {
                dag.insert_certified(&candidate, InsertMode::Normal, &certificate, &outcome)
                    .unwrap();
                admitted.store(true, Ordering::SeqCst);
            }
        })
    };
    let blocks = KeyValueBlockStore::new(
        Arc::new(PublicationStore {
            inner: Arc::new(InMemoryKeyValueStore::new()),
            fail_on_write: 0,
            writes: Arc::new(AtomicUsize::new(0)),
            failures: Arc::new(AtomicUsize::new(0)),
            gate: None,
            read_hook: Some(read_hook),
            write_hook: None,
        }),
        Arc::new(InMemoryKeyValueStore::new()),
    );
    blocks.put_block_message(&genesis).unwrap();
    blocks
        .put_block_message_awaiting_certificate(&candidate)
        .unwrap();
    let transport = Arc::new(TransportLayerStub::new());
    let connections = ConnectionsCell {
        peers: Arc::new(std::sync::Mutex::new(Connections::from_vec(Vec::new()))),
    };
    let conf = create_rp_conf_ask(
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
    );
    let retriever = BlockRetriever::new(
        buffer.clone(),
        transport.clone(),
        connections.clone(),
        conf.clone(),
    );
    let processor = BlockProcessor::new(
        BlockProcessorDependencies::new(
            blocks,
            dag.clone(),
            retriever.clone(),
            transport,
            connections,
            conf,
            None,
        )
        .unwrap(),
    );
    let casper = Arc::new(TestCasperWithSnapshot::new(
        TestCasperWithSnapshot::create_empty_snapshot(),
        genesis,
    ));
    armed.store(true, Ordering::SeqCst);
    assert!(processor
        .restore_stored_buffer_ownership(casper, &candidate.block_hash)
        .await
        .unwrap());
    assert!(admitted.load(Ordering::SeqCst));
    assert!(dag.has_admitted_metadata(&candidate.block_hash).unwrap());
    assert!(!buffer
        .contains_durable_row(&BlockHashSerde(candidate.block_hash.clone()))
        .unwrap());
    assert!(buffer
        .pending_request_policy(&BlockHashSerde(candidate.block_hash.clone()))
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn first_publication_preserves_captured_dependency_after_tracking_removal() {
    let mut manager = InMemoryStoreManager::new();
    let dag = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
    let buffer = CasperBufferKeyValueStorage::new_from_kvm(&mut manager)
        .await
        .unwrap();
    let (mut genesis, candidate) = publication_blocks();
    dag.insert(&genesis, InsertMode::ApprovedGenesis).unwrap();
    let transport = Arc::new(TransportLayerStub::new());
    let connections = ConnectionsCell {
        peers: Arc::new(std::sync::Mutex::new(Connections::from_vec(Vec::new()))),
    };
    let conf = create_rp_conf_ask(
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
    );
    let retriever = BlockRetriever::new(
        buffer.clone(),
        transport.clone(),
        connections.clone(),
        conf.clone(),
    );
    let hash = candidate.block_hash.clone();
    retriever
        .admit_hash(
            hash.clone(),
            None,
            AdmitHashReason::MissingDependencyRequested,
        )
        .await
        .unwrap();
    let removed = Arc::new(AtomicBool::new(false));
    let write_hook = {
        let retriever = retriever.clone();
        let hash = hash.clone();
        let removed = removed.clone();
        Arc::new(move || {
            if !removed.swap(true, Ordering::SeqCst) {
                assert!(retriever.was_requested_as_dependency(&hash).unwrap());
                retriever.forget_hash_tracking(&hash).unwrap();
                assert!(!retriever.was_requested_as_dependency(&hash).unwrap());
            }
        })
    };
    let blocks = KeyValueBlockStore::new(
        Arc::new(PublicationStore {
            inner: Arc::new(InMemoryKeyValueStore::new()),
            fail_on_write: 0,
            writes: Arc::new(AtomicUsize::new(0)),
            failures: Arc::new(AtomicUsize::new(0)),
            gate: None,
            read_hook: None,
            write_hook: Some(write_hook),
        }),
        Arc::new(InMemoryKeyValueStore::new()),
    );
    let processor = Arc::new(BlockProcessor::new(
        BlockProcessorDependencies::new(
            blocks.clone(),
            dag,
            retriever.clone(),
            transport,
            connections,
            conf,
            None,
        )
        .unwrap(),
    ));
    genesis.body.state.block_number = 2;
    genesis.block_hash = genesis.computed_block_hash();
    let casper: Arc<dyn MultiParentCasper + Send + Sync> = Arc::new(TestCasperWithSnapshot::new(
        TestCasperWithSnapshot::create_empty_snapshot(),
        genesis,
    ));
    let (sender, mut receiver) = BlockProcessingQueueSender::channel(1, 1024 * 1024).unwrap();
    sender
        .try_enqueue_with_receipt(casper.clone(), candidate.clone(), |hash| {
            retriever.record_received(hash.clone()).map(|_| ())
        })
        .unwrap();
    process_owned_block(
        processor.clone(),
        receiver.try_recv().unwrap(),
        sender.identities(),
        sender.recovery().signal(),
    )
    .await;
    assert!(removed.load(Ordering::SeqCst));
    assert_eq!(blocks.get_detached(&hash).unwrap(), Some(candidate.clone()));
    assert!(buffer
        .contains_durable_row(&BlockHashSerde(hash.clone()))
        .unwrap());
    assert!(sender.identities().is_empty());
    assert_eq!(sender.used_bytes(), 0);
    assert!(
        buffer
            .pending_request_policy(&BlockHashSerde(hash))
            .unwrap()
            .unwrap()
            .requested_as_dependency,
        "first publication lost the dependency decision captured before request cleanup"
    );
    assert!(processor.check_if_of_interest(casper, &candidate).unwrap());
}

#[tokio::test]
async fn publication_failure_retains_an_owner_across_retry_budget_exhaustion() {
    tokio::time::timeout(
        Duration::from_secs(120),
        assert_worker_publication_with_retirement(0, false, None, true, false, true),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn publication_success_transfers_retry_owner_before_worker_release() {
    assert_worker_publication_ownership(0, false, None).await;
}

#[tokio::test]
async fn publication_failure_at_tracker_capacity_retries_without_another_announcement() {
    assert_worker_publication_ownership(1, true, None).await;
}

#[tokio::test]
async fn publication_success_at_tracker_capacity_transfers_worker_ownership() {
    assert_worker_publication_ownership(0, true, None).await;
}

#[tokio::test]
async fn publication_later_relation_failure_never_releases_a_partial_dependency_row() {
    assert_worker_publication_ownership(2, true, None).await;
}

#[tokio::test]
async fn publication_quarantine_repairs_a_preexisting_partial_row_before_worker_release() {
    assert_worker_publication_ownership(3, true, None).await;
}

#[tokio::test]
async fn publication_quarantine_uses_stored_identity_not_conflicting_incoming_dependencies() {
    assert_worker_publication_ownership(0, true, Some((24, 25))).await;
}

#[tokio::test]
async fn publication_quarantine_retires_tracking_only_after_stored_owner_is_complete() {
    assert_worker_publication_ownership(0, false, Some((24, 25))).await;
}

#[tokio::test]
async fn publication_pending_solicited_history_preserves_old_block_eligibility() {
    assert_worker_publication_with_history(0, false, None, true, false).await;
}

#[tokio::test]
async fn publication_quarantined_solicited_history_preserves_old_block_eligibility() {
    assert_worker_publication_with_history(0, false, Some((24, 25)), true, false).await;
}

#[tokio::test]
async fn publication_restarted_solicited_history_preserves_old_block_eligibility() {
    assert_worker_publication_with_history(0, false, None, true, true).await;
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn publication_quarantine_ignores_generated_incoming_dependencies(
        parent in 24u8..=u8::MAX,
        certificate in 24u8..=u8::MAX,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(assert_worker_publication_ownership(0, true, Some((parent, certificate))));
    }
}
