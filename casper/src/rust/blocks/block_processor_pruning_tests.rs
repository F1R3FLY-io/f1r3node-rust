use block_storage::rust::dag::block_dag_key_value_storage::InsertMode;
use comm::rust::peer_node::{NodeIdentifier, PeerNode};
use comm::rust::rp::connect::Connections;
use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
use models::rust::block_implicits::get_random_block;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};

use super::*;
use crate::rust::engine::block_retriever::{
    AdmitHashResult, AdmitHashStatus, MAX_RETRIES_PER_HASH,
};
use crate::rust::protocol::{extract_packet_from_protocol, verify_block_request};

#[derive(Clone)]
struct ReadProbeStore {
    inner: Arc<dyn KeyValueStore>,
    tracked: Arc<HashSet<Vec<u8>>>,
    reads: Arc<std::sync::atomic::AtomicUsize>,
}

impl KeyValueStore for ReadProbeStore {
    fn as_any(&self) -> &dyn std::any::Any { self }

    fn get(&self, keys: &Vec<Vec<u8>>) -> Result<Vec<Option<Vec<u8>>>, KvStoreError> {
        self.reads.fetch_add(
            keys.iter()
                .filter(|key| self.tracked.contains(*key))
                .count(),
            Ordering::Relaxed,
        );
        self.inner.get(keys)
    }

    fn with_value(
        &self,
        key: &Vec<u8>,
        reader: &mut shared::rust::store::key_value_store::ValueReader<'_>,
    ) -> Result<(), KvStoreError> {
        if self.tracked.contains(key) {
            self.reads.fetch_add(1, Ordering::Relaxed);
        }
        self.inner.with_value(key, reader)
    }

    fn visit_entries(
        &self,
        reader: &mut shared::rust::store::key_value_store::EntryReader<'_>,
    ) -> Result<(), KvStoreError> {
        self.inner.visit_entries(reader)
    }

    fn put(&self, values: Vec<(Vec<u8>, Vec<u8>)>) -> Result<(), KvStoreError> {
        self.inner.put(values)
    }

    fn put_one_if_absent(&self, key: Vec<u8>, value: Vec<u8>) -> Result<bool, KvStoreError> {
        self.inner.put_one_if_absent(key, value)
    }

    fn delete(&self, keys: Vec<Vec<u8>>) -> Result<usize, KvStoreError> { self.inner.delete(keys) }

    fn iterate(&self, f: fn(Vec<u8>, Vec<u8>)) -> Result<(), KvStoreError> { self.inner.iterate(f) }

    fn iterate_while(
        &self,
        f: &mut dyn FnMut(Vec<u8>, Vec<u8>) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        self.inner.iterate_while(f)
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<std::collections::BTreeMap<Vec<u8>, Vec<u8>>, KvStoreError> {
        self.inner.to_map()
    }

    fn print_store(&self) -> Result<(), KvStoreError> { self.inner.print_store() }

    fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }

    fn contains(&self, keys: &Vec<Vec<u8>>) -> Result<Vec<bool>, KvStoreError> {
        self.inner.contains(keys)
    }

    fn size_bytes(&self) -> usize { self.inner.size_bytes() }
}

struct Fixture {
    dependencies: BlockProcessorDependencies<TransportLayerStub>,
    peer: PeerNode,
    parent: BlockHash,
    child: BlockHash,
}

impl Fixture {
    async fn new() -> Self {
        let mut kvm = InMemoryStoreManager::new();
        let block_store = KeyValueBlockStore::create_from_kvm(&mut kvm).await.unwrap();
        let dag = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();
        let buffer = CasperBufferKeyValueStorage::new_from_kvm(&mut kvm)
            .await
            .unwrap();
        let genesis = get_random_block(
            Some(0),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(vec![]),
            None,
            None,
            None,
            Some(vec![]),
            None,
            None,
        );
        block_store.put_block_message(&genesis).unwrap();
        dag.insert(&genesis, InsertMode::Approved).unwrap();

        let local = PeerNode::new(
            NodeIdentifier {
                key: vec![1; 20].into(),
            },
            "local".to_string(),
            40400,
            40404,
        );
        let peer = PeerNode::new(
            NodeIdentifier {
                key: vec![2; 20].into(),
            },
            "peer".to_string(),
            40400,
            40404,
        );
        let connections = ConnectionsCell {
            peers: Arc::new(Mutex::new(Connections::from_vec(vec![peer.clone()]))),
        };
        let conf = create_rp_conf_ask(local, None, None);
        let transport = Arc::new(TransportLayerStub::new());
        let retriever = BlockRetriever::new(
            Arc::new(Mutex::new(HashMap::new())),
            transport.clone(),
            connections.clone(),
            conf.clone(),
        );
        let parent = BlockHash::from(vec![3; 32]);
        let child = BlockHash::from(vec![4; 32]);
        buffer
            .add_relation(
                BlockHashSerde(parent.clone()),
                BlockHashSerde(child.clone()),
            )
            .unwrap();
        let dependencies = BlockProcessorDependencies::new(
            block_store,
            buffer,
            dag,
            retriever,
            transport,
            connections,
            conf,
            None,
        );

        Self {
            dependencies,
            peer,
            parent,
            child,
        }
    }

    async fn request_parent(&self) -> AdmitHashResult {
        self.dependencies
            .block_retriever
            .admit_hash(
                self.parent.clone(),
                Some(self.peer.clone()),
                AdmitHashReason::MissingDependencyRequested,
            )
            .await
            .unwrap()
    }

    fn prune(&self) {
        self.dependencies.enforce_casper_buffer_limit(0).unwrap();
        assert_eq!(self.dependencies.casper_buffer.approx_node_count(), 0);
        assert!(!self
            .dependencies
            .casper_buffer
            .is_pendant(&BlockHashSerde(self.parent.clone())));
        assert!(!self
            .dependencies
            .casper_buffer
            .contains(&BlockHashSerde(self.child.clone())));
    }
}

#[tokio::test]
async fn overflow_prune_keeps_an_unreceived_dependency_retryable() {
    let fixture = Fixture::new().await;
    fixture.request_parent().await;
    let retriever = &fixture.dependencies.block_retriever;
    retriever
        .set_retry_attempts_for_test(&fixture.parent, 3)
        .unwrap();

    fixture.prune();

    let mut request = retriever
        .get_request_state_for_test(&fixture.parent)
        .await
        .unwrap()
        .expect("Buffer pruning must keep the pending parent request");
    assert!(!request.received);
    assert!(request.requested_as_dependency);
    assert_eq!(request.waiting_list, vec![fixture.peer.clone()]);
    assert_eq!(
        retriever.retry_attempts_for_test(&fixture.parent).unwrap(),
        3
    );

    request.timestamp = 0;
    retriever
        .set_request_state_for_test(fixture.parent.clone(), request)
        .await
        .unwrap();
    fixture.dependencies.transport.reset();
    retriever.request_all(Duration::ZERO).await.unwrap();

    let parent_requested = (0..fixture.dependencies.transport.request_count())
        .filter_map(|index| fixture.dependencies.transport.get_request(index))
        .any(|(peer, protocol)| {
            peer == fixture.peer
                && extract_packet_from_protocol(&protocol)
                    .is_ok_and(|packet| verify_block_request(&packet, &fixture.parent).is_ok())
        });
    assert!(
        parent_requested,
        "The retry must request the missing parent from its peer"
    );
}

#[tokio::test]
async fn overflow_prune_reopens_a_received_block_for_another_request() {
    let fixture = Fixture::new().await;
    let retriever = &fixture.dependencies.block_retriever;
    retriever.ack_receive(fixture.parent.clone()).await.unwrap();
    assert!(retriever.is_received(fixture.parent.clone()).await.unwrap());

    fixture.prune();

    assert!(retriever
        .get_request_state_for_test(&fixture.parent)
        .await
        .unwrap()
        .is_none());
    fixture.dependencies.transport.reset();
    let result = fixture.request_parent().await;
    assert_eq!(result.status, AdmitHashStatus::NewRequestAdded);
    assert_eq!(fixture.dependencies.transport.request_count(), 1);
}

#[tokio::test]
async fn overflow_prune_preserves_the_retry_quarantine() {
    let fixture = Fixture::new().await;
    fixture.request_parent().await;
    let retriever = &fixture.dependencies.block_retriever;
    retriever
        .set_retry_attempts_for_test(&fixture.parent, MAX_RETRIES_PER_HASH)
        .unwrap();
    retriever
        .recover_dependency(fixture.parent.clone())
        .await
        .unwrap();
    assert!(retriever
        .get_request_state_for_test(&fixture.parent)
        .await
        .unwrap()
        .is_none());
    fixture.dependencies.transport.reset();
    assert_eq!(
        fixture.request_parent().await.status,
        AdmitHashStatus::Ignore
    );

    fixture.prune();

    assert_eq!(
        fixture.request_parent().await.status,
        AdmitHashStatus::Ignore
    );
    assert_eq!(fixture.dependencies.transport.request_count(), 0);
    assert_eq!(
        retriever.retry_attempts_for_test(&fixture.parent).unwrap(),
        MAX_RETRIES_PER_HASH
    );
}

#[tokio::test]
async fn overflow_prune_keeps_a_child_released_before_the_prune_lock() {
    use block_storage::rust::dag::equivocation_tracker_store::EquivocationTrackerStore;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

    let mut fixture = Fixture::new().await;
    let original_dag = &fixture.dependencies.block_dag_storage;
    let genesis_hash = original_dag
        .get_representation()
        .unwrap()
        .last_finalized_block();
    let global_lock = Arc::new(parking_lot::RwLock::new(()));
    let mut kvm = InMemoryStoreManager::new();
    let dag = BlockDagKeyValueStorage::from_parts(
        global_lock.clone(),
        KeyValueTypedStoreImpl::new(kvm.store("latest-messages".to_string()).await.unwrap()),
        original_dag.metadata_index_for_tests(),
        KeyValueTypedStoreImpl::new(kvm.store("invalid-blocks".to_string()).await.unwrap()),
        original_dag.floor_index_for_tests().clone(),
        original_dag.frontier_index_for_tests().clone(),
        EquivocationTrackerStore::new(KeyValueTypedStoreImpl::new(
            kvm.store("equivocation-tracker".to_string()).await.unwrap(),
        )),
        Arc::new(AtomicU64::new(original_dag.current_generation())),
    );
    fixture.dependencies.block_dag_storage = dag.clone();

    let mut parent = get_random_block(
        Some(1),
        Some(1),
        None,
        None,
        None,
        None,
        None,
        Some(vec![genesis_hash]),
        Some(vec![]),
        None,
        None,
        Some(vec![]),
        None,
        None,
    );
    parent.block_hash = fixture.parent.clone();
    let mut child = parent.clone();
    child.block_hash = fixture.child.clone();
    child.header.parents_hash_list = vec![parent.block_hash.clone()];
    child.body.state.block_number = 2;
    child.seq_num = 2;
    fixture
        .dependencies
        .block_store
        .put_block_message(&parent)
        .unwrap();
    fixture
        .dependencies
        .block_store
        .put_block_message(&child)
        .unwrap();

    let metadata = dag.metadata_index_for_tests();
    let metadata_guard = metadata.write();
    let pruner = {
        let dependencies = fixture.dependencies.clone();
        std::thread::spawn(move || dependencies.enforce_casper_buffer_limit(0))
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut pruning_holds_dag_lock = false;
    while Instant::now() < deadline && !pruner.is_finished() {
        if global_lock.try_write().is_none() {
            pruning_holds_dag_lock = true;
            break;
        }
        std::thread::yield_now();
    }

    let admission = {
        let buffer = fixture.dependencies.casper_buffer.clone();
        std::thread::spawn(move || {
            use block_storage::rust::dag::buffer_dag_transition::{
                atomic_insert_then_buffer, BufferTransition,
            };

            let transition =
                BufferTransition::RemoveFromBuffer(BlockHashSerde(parent.block_hash.clone()));
            atomic_insert_then_buffer(&dag, &parent, InsertMode::Normal, &buffer, transition)
        })
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut writer_blocks_new_readers = false;
    while Instant::now() < deadline {
        if global_lock.try_read().is_none() {
            writer_blocks_new_readers = true;
            break;
        }
        std::thread::yield_now();
    }

    drop(metadata_guard);
    let admission = admission.join().unwrap();
    let pruning = pruner.join().unwrap();

    assert!(
        pruning_holds_dag_lock,
        "Pruning must acquire the DAG lock before it reads metadata"
    );
    assert!(
        writer_blocks_new_readers,
        "A DAG writer must exclude new readers"
    );
    admission.unwrap();
    pruning.unwrap();
    assert!(fixture
        .dependencies
        .casper_buffer
        .is_pendant(&BlockHashSerde(fixture.child.clone())));
    assert_eq!(fixture.dependencies.casper_buffer.take_released(), vec![
        BlockHashSerde(fixture.child)
    ]);
}

#[tokio::test]
async fn overflow_prune_does_not_decode_ready_blocks_on_later_passes() {
    let mut fixture = Fixture::new().await;
    let genesis_hash = fixture
        .dependencies
        .block_dag_storage
        .get_representation()
        .unwrap()
        .last_finalized_block();
    let mut block = get_random_block(
        Some(1),
        Some(1),
        None,
        None,
        None,
        None,
        None,
        Some(vec![genesis_hash]),
        Some(vec![]),
        None,
        None,
        Some(vec![]),
        None,
        None,
    );
    let mut hashes = HashSet::new();
    for index in 0..64_u64 {
        let mut bytes = vec![5; 32];
        bytes[..8].copy_from_slice(&index.to_be_bytes());
        block.block_hash = bytes.clone().into();
        fixture
            .dependencies
            .block_store
            .put_block_message(&block)
            .unwrap();
        fixture
            .dependencies
            .casper_buffer
            .put_pendant(BlockHashSerde(block.block_hash.clone()))
            .unwrap();
        hashes.insert(bytes);
    }
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let original = fixture
        .dependencies
        .block_store
        .soak_capture_store()
        .clone();
    fixture.dependencies.block_store = KeyValueBlockStore::new(
        Arc::new(ReadProbeStore {
            inner: original.clone(),
            tracked: Arc::new(hashes.clone()),
            reads: reads.clone(),
        }),
        original,
    );

    fixture.prune();
    assert_eq!(reads.load(Ordering::Relaxed), hashes.len());
    tokio::time::sleep(Duration::from_millis(CASPER_BUFFER_PRUNE_INTERVAL_MS + 10)).await;
    fixture.prune();
    assert_eq!(
        reads.load(Ordering::Relaxed),
        hashes.len(),
        "Repeated pruning must use cached dependencies instead of reading block bodies"
    );
    for bytes in hashes {
        assert!(fixture
            .dependencies
            .casper_buffer
            .is_pendant(&BlockHashSerde(bytes.into())));
    }
}
