use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::blocks::block_processor::BlockProcessorDependencies;
use casper::rust::casper::test_helpers::TestCasperWithSnapshot;
use casper::rust::engine::block_retriever::BlockRetriever;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use comm::rust::rp::connect::{Connections, ConnectionsCell};
use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
use models::rust::block_implicits::get_random_block_default;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

use super::*;

pub(super) async fn fixture() -> (
    BlockProcessorInstance<TransportLayerStub>,
    BlockProcessingQueueSender,
    Arc<dyn MultiParentCasper + Send + Sync>,
) {
    let mut manager = InMemoryStoreManager::new();
    let blocks = KeyValueBlockStore::create_from_kvm(&mut manager)
        .await
        .unwrap();
    let buffer = CasperBufferKeyValueStorage::new_from_kvm(&mut manager)
        .await
        .unwrap();
    let dag = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
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
    let processor = Arc::new(BlockProcessor::new(
        BlockProcessorDependencies::new(blocks, dag, retriever, transport, connections, conf, None)
            .unwrap(),
    ));
    let (sender, receiver) = BlockProcessingQueueSender::channel(3, 16 * 1024 * 1024).unwrap();
    let instance = BlockProcessorInstance::new(
        (receiver, sender.clone()),
        processor,
        sender.identities(),
        None,
    );
    let casper = Arc::new(TestCasperWithSnapshot::new(
        TestCasperWithSnapshot::create_empty_snapshot(),
        get_random_block_default(),
    ));
    (instance, sender, casper)
}

struct DelayedPayload {
    key: u8,
    bytes: [u8; 8],
    entered: tokio::sync::mpsc::UnboundedSender<u8>,
    resume: std::sync::mpsc::Receiver<()>,
    released: Arc<AtomicUsize>,
}

impl AsRef<[u8]> for DelayedPayload {
    fn as_ref(&self) -> &[u8] { &self.bytes }
}

impl Drop for DelayedPayload {
    fn drop(&mut self) {
        self.entered.send(self.key).unwrap();
        self.resume.recv_timeout(Duration::from_secs(20)).unwrap();
        self.released.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn dropping_an_unpolled_dispatcher_closes_input_and_stops_recovery() {
    let (instance, sender, casper) = fixture().await;
    sender
        .try_enqueue(casper, get_random_block_default())
        .unwrap();
    let recovery = sender.recovery();
    assert!(!recovery.signal().is_stopped());
    drop(instance.run());
    assert!(sender.is_closed());
    assert_eq!(sender.used_bytes(), 0);
    assert!(sender.identities().is_empty());
    assert!(recovery.signal().is_stopped());
    assert!(recovery.startup().is_stopped());
}

#[tokio::test]
async fn idle_dispatcher_terminates_when_external_senders_close() {
    let (instance, sender, casper) = fixture().await;
    let recovery = sender.recovery();
    let dispatcher = tokio::spawn(instance.run());
    drop(sender);
    tokio::time::timeout(Duration::from_secs(5), dispatcher)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(recovery.signal().is_stopped());
    drop(casper);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn actual_dispatcher_runs_two_workers_and_retains_capacity_through_destruction() {
    let (mut instance, sender, casper) = fixture().await;
    instance.max_parallel_blocks = 2;
    let processor = instance.block_processor.clone();
    let (entered, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let released = Arc::new(AtomicUsize::new(0));
    let mut resumes = std::collections::BTreeMap::new();
    for key in 1u8..=3 {
        let mut block = get_random_block_default();
        block.block_hash = Bytes::from(vec![key; 32]);
        let (resume, wait) = std::sync::mpsc::channel();
        block.sig = Bytes::from_owner(DelayedPayload {
            key,
            bytes: [key; 8],
            entered: entered.clone(),
            resume: wait,
            released: released.clone(),
        });
        processor
            .note_validation_failure(&block.block_hash)
            .unwrap();
        sender.try_enqueue(casper.clone(), block).unwrap();
        resumes.insert(key, resume);
    }
    let used = sender.used_bytes();
    let dispatcher = tokio::spawn(instance.run());
    let first = tokio::time::timeout(Duration::from_secs(5), arrivals.recv())
        .await
        .unwrap()
        .unwrap();
    let second = tokio::time::timeout(Duration::from_secs(5), arrivals.recv())
        .await
        .unwrap()
        .unwrap();
    assert_ne!(first, second);
    assert_eq!(released.load(Ordering::SeqCst), 0);
    assert_eq!(sender.used_bytes(), used);
    assert_eq!(sender.identities().len(), 3);
    assert_eq!(
        arrivals.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    );
    resumes.remove(&first).unwrap().send(()).unwrap();
    let third = tokio::time::timeout(Duration::from_secs(5), arrivals.recv())
        .await
        .unwrap()
        .unwrap();
    assert_ne!(third, first);
    assert_ne!(third, second);
    assert!(!sender.identities().contains(&Bytes::from(vec![first; 32])));
    assert!(sender.used_bytes() < used);
    for resume in resumes.into_values() {
        resume.send(()).unwrap();
    }
    let identities = sender.identities();
    let recovery = sender.recovery();
    let weak = sender.downgrade();
    drop(sender);
    tokio::time::timeout(Duration::from_secs(5), dispatcher)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(released.load(Ordering::SeqCst), 3);
    assert!(identities.is_empty());
    assert!(weak.upgrade().is_none());
    assert!(recovery.signal().is_stopped());
}
