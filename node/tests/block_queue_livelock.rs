//! A node must keep admitting blocks while some of the blocks it holds wait
//! on ancestors it does not have.
//!
//! Each test drives a node built from the production receipt path
//! (`Running`), processing queue and scheduler (`BlockProcessorInstance`),
//! `BlockProcessor`, block store, pending area, casper buffer, DAG store,
//! block retriever and casper maintenance loop. Only block execution is
//! stubbed: `StubCasper::validate` accepts every block and can be held shut.
//! Its membership, buffer and dependency answers reproduce
//! `MultiParentCasperImpl`'s over the node's own stores.
//!
//! Peers are simulated by `PeerTransport`: it records what the node sends and
//! a network task answers block requests from the blocks each peer holds.
//! Outbound block requests (`BlockRequest`, `HasBlockRequest`) can be held
//! shut, which stands for a peer that is slow to take a request.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, DeployId, InsertMode, KeyValueDagRepresentation,
};
use block_storage::rust::dag::buffer_dag_transition::{
    atomic_insert_then_buffer, BufferTransition,
};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use casper::rust::blocks::block_processor::{
    BlockProcessor, BlockProcessorDependencies, BlockQueueItem, InFlightBlocks,
};
use casper::rust::casper::test_helpers::TestCasperWithSnapshot;
use casper::rust::casper::{
    Casper, CasperShardConf, CasperSnapshot, DeployError, MultiParentCasper,
};
use casper::rust::engine::block_retriever::{AdmitHashReason, BlockRetriever};
use casper::rust::engine::casper_launch::send_buffer_pendants_to_casper;
use casper::rust::engine::engine::Engine;
use casper::rust::engine::running::Running;
use casper::rust::errors::CasperError;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::protocol::{
    extract_packet_from_protocol, to_casper_message_proto, CasperMessageProto, PacketParseResult,
};
use casper::rust::util::proto_util;
use casper::rust::util::rholang::runtime_manager::{
    ExploratoryDeployConfig, ReplayLock, RuntimeManager,
};
use casper::rust::validator_identity::ValidatorIdentity;
use comm::rust::errors::CommError;
use comm::rust::peer_node::{NodeIdentifier, PeerNode};
use comm::rust::rp::connect::ConnectionsCell;
use comm::rust::test_instances::create_rp_conf_ask;
use comm::rust::transport::transport_layer::{Blob, TransportLayer};
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Signed;
use models::routing::Protocol;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::{
    ApprovedBlock, ApprovedBlockCandidate, BlockMessage, Bond, CasperMessage, DeployData, HasBlock,
    Justification,
};
use models::rust::validator::Validator;
use node::rust::instances::block_processor_instance::BlockProcessorInstance;
use prost::bytes::Bytes;
use rholang::rust::interpreter::external_services::ExternalServices;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporter;
use serial_test::serial;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tracing::Level;
use tracing_subscriber::filter::{EnvFilter, FilterExt, Targets};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;

const SHARD: &str = "root";
const VERSION: i64 = 1;
/// The width the node derives on a four-core host.
const PIPELINE_WIDTH: usize = 3;
/// The production capacity of the channel feeding the processor.
const QUEUE_CHANNEL_CAPACITY: usize = 2_048;
/// `casper-loop-interval` and `requested-blocks-timeout` in `defaults.conf`.
const CASPER_LOOP_INTERVAL: Duration = Duration::from_millis(750);
const REQUESTED_BLOCKS_TIMEOUT: Duration = Duration::from_secs(240);
/// The retriever asks this many connected peers, in connection order, for a
/// missing dependency before it falls back to broadcasting.
const SEED_PEERS: usize = 4;
const REPORT_TARGET: &str = "block_queue_livelock";

/// Logs at `RUST_LOG` when it is set, else warnings plus this test's reports.
fn init_tracing() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let filter = match std::env::var(EnvFilter::DEFAULT_ENV) {
            Ok(directives) => FilterExt::boxed(EnvFilter::new(directives)),
            Err(_) => FilterExt::boxed(
                Targets::new()
                    .with_target(REPORT_TARGET, Level::INFO)
                    .with_default(Level::WARN),
            ),
        };
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .with_test_writer()
                .with_filter(filter),
        );
        tracing::subscriber::set_global_default(subscriber)
            .expect("this test binary installs the only global subscriber");
    });
}

// ── Gate ────────────────────────────────────────────────────────────────────

/// A switch that callers wait on until it is open.
#[derive(Clone)]
struct Gate(Arc<watch::Sender<bool>>);

impl Gate {
    fn new(open: bool) -> Self { Self(Arc::new(watch::Sender::new(open))) }

    fn open(&self) { self.0.send_replace(true); }

    async fn pass(&self) {
        self.0
            .subscribe()
            .wait_for(|open| *open)
            .await
            .expect("the gate's sender lives as long as the gate");
    }
}

// ── Blocks ──────────────────────────────────────────────────────────────────

/// Bonded validators that sign blocks with increasing sequence numbers.
struct Validators {
    identities: Vec<ValidatorIdentity>,
    bonds: Vec<Bond>,
    next_seq: Vec<i32>,
}

impl Validators {
    fn new(count: usize) -> Self {
        let identities: Vec<ValidatorIdentity> = (0..count)
            .map(|_| ValidatorIdentity::new(&Secp256k1.new_key_pair().0))
            .collect();
        let bonds = identities
            .iter()
            .map(|identity| Bond {
                validator: identity.public_key.bytes.clone(),
                stake: 100,
            })
            .collect();
        Self {
            identities,
            bonds,
            next_seq: vec![1; count],
        }
    }

    fn genesis(&self) -> BlockMessage { block(0, 0, None, vec![], vec![], &self.bonds) }

    /// A block by validator `v` whose first parent is its main parent.
    fn sign(&mut self, v: usize, parents: &[&BlockMessage]) -> BlockMessage {
        let number = parents
            .iter()
            .map(|p| p.body.state.block_number)
            .max()
            .expect("every block but genesis has a parent")
            + 1;
        let seq = self.next_seq[v];
        self.next_seq[v] += 1;
        let identity = &self.identities[v];
        identity.sign_block(&block(
            number,
            seq,
            Some(identity.public_key.bytes.clone()),
            parents.iter().map(|p| p.block_hash.clone()).collect(),
            parents
                .iter()
                .map(|p| Justification {
                    validator: p.sender.clone(),
                    latest_block_hash: p.block_hash.clone(),
                })
                .collect(),
            &self.bonds,
        ))
    }
}

fn block(
    number: i64,
    seq: i32,
    sender: Option<Validator>,
    parents: Vec<BlockHash>,
    justifications: Vec<Justification>,
    bonds: &[Bond],
) -> BlockMessage {
    get_random_block(
        Some(number),
        Some(seq),
        None,
        None,
        sender,
        Some(VERSION),
        None,
        Some(parents),
        Some(justifications),
        Some(vec![]),
        Some(vec![]),
        Some(bonds.to_vec()),
        Some(SHARD.to_string()),
        None,
    )
}

// ── Peers ───────────────────────────────────────────────────────────────────

fn peer(index: usize) -> PeerNode {
    let name = format!("peer{index:02}");
    PeerNode::new(
        NodeIdentifier {
            key: Bytes::copy_from_slice(name.as_bytes()),
        },
        name,
        40400,
        40404,
    )
}

/// Which peer holds which block, and the tips peers report.
#[derive(Clone)]
struct PeerBehaviour {
    blocks: Arc<HashMap<BlockHash, BlockMessage>>,
    holds: Arc<dyn Fn(usize, &BlockHash) -> bool + Send + Sync>,
    tips: Arc<Vec<BlockHash>>,
}

impl PeerBehaviour {
    fn new(
        blocks: &[&BlockMessage],
        holds: impl Fn(usize, &BlockHash) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            blocks: Arc::new(
                blocks
                    .iter()
                    .map(|b| (b.block_hash.clone(), (*b).clone()))
                    .collect(),
            ),
            holds: Arc::new(holds),
            tips: Arc::new(Vec::new()),
        }
    }

    fn holding_nothing() -> Self { Self::new(&[], |_, _| false) }

    fn with_tips(mut self, tips: &[&BlockMessage]) -> Self {
        self.tips = Arc::new(tips.iter().map(|b| b.block_hash.clone()).collect());
        self
    }

    fn served(&self, peer: usize, hash: &BlockHash) -> Option<BlockMessage> {
        self.blocks
            .get(hash)
            .filter(|_| (self.holds)(peer, hash))
            .cloned()
    }
}

/// Block requests the node sent, by kind and by whether the peer is one of
/// the first `SEED_PEERS` it is connected to.
#[derive(Default, Debug)]
struct SentRequests {
    block_requests_to_seed_peers: AtomicUsize,
    block_requests_to_other_peers: AtomicUsize,
    has_block_requests: AtomicUsize,
}

impl std::fmt::Display for SentRequests {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the node sent {} BlockRequests to seed peers, {} to other peers and {} \
             HasBlockRequests",
            self.block_requests_to_seed_peers.load(Ordering::SeqCst),
            self.block_requests_to_other_peers.load(Ordering::SeqCst),
            self.has_block_requests.load(Ordering::SeqCst),
        )
    }
}

/// The node's transport. Every message is handed to the simulated peers;
/// block requests are counted and first wait on `block_requests`.
#[derive(Clone)]
struct PeerTransport {
    outbound: mpsc::UnboundedSender<(PeerNode, Protocol)>,
    block_requests: Gate,
    seed_peers: Arc<HashSet<PeerNode>>,
    sent: Arc<SentRequests>,
}

enum BlockRequestKind {
    BlockRequest,
    HasBlockRequest,
}

fn block_request_kind(msg: &Protocol) -> Option<BlockRequestKind> {
    let packet = extract_packet_from_protocol(msg).ok()?;
    match to_casper_message_proto(&packet) {
        PacketParseResult::Success(CasperMessageProto::BlockRequest(_)) => {
            Some(BlockRequestKind::BlockRequest)
        }
        PacketParseResult::Success(CasperMessageProto::HasBlockRequest(_)) => {
            Some(BlockRequestKind::HasBlockRequest)
        }
        _ => None,
    }
}

#[async_trait]
impl TransportLayer for PeerTransport {
    async fn send(&self, peer: &PeerNode, msg: &Protocol) -> Result<(), CommError> {
        if let Some(kind) = block_request_kind(msg) {
            self.block_requests.pass().await;
            let counter = match kind {
                BlockRequestKind::HasBlockRequest => &self.sent.has_block_requests,
                BlockRequestKind::BlockRequest if self.seed_peers.contains(peer) => {
                    &self.sent.block_requests_to_seed_peers
                }
                BlockRequestKind::BlockRequest => &self.sent.block_requests_to_other_peers,
            };
            counter.fetch_add(1, Ordering::SeqCst);
        }
        self.outbound
            .send((peer.clone(), msg.clone()))
            .map_err(|_| {
                CommError::InternalCommunicationError("the peers have stopped".to_string())
            })?;
        Ok(())
    }

    async fn broadcast(&self, peers: &[PeerNode], msg: &Protocol) -> Result<(), CommError> {
        for peer in peers {
            self.send(peer, msg).await?;
        }
        Ok(())
    }

    async fn stream(&self, _peer: &PeerNode, _blob: &Blob) -> Result<(), CommError> { Ok(()) }

    async fn stream_mult(&self, _peers: &[PeerNode], _blob: &Blob) -> Result<(), CommError> {
        Ok(())
    }

    async fn disconnect(&self, _peer: &PeerNode) -> Result<(), CommError> { Ok(()) }

    async fn get_channeled_peers(&self) -> Result<HashSet<PeerNode>, CommError> {
        Ok(HashSet::new())
    }
}

/// Answer what the node sends: a peer that holds a block replies `HasBlock`
/// to a `HasBlockRequest` and the block to a `BlockRequest`; every peer
/// answers a fork-choice tip request with `HasBlock` for each tip.
async fn answer_requests(
    mut outbound: mpsc::UnboundedReceiver<(PeerNode, Protocol)>,
    engine: Arc<Running<PeerTransport>>,
    peers: Vec<PeerNode>,
    behaviour: PeerBehaviour,
) {
    let index_of: HashMap<PeerNode, usize> = peers
        .iter()
        .enumerate()
        .map(|(i, p)| (p.clone(), i))
        .collect();
    while let Some((to, msg)) = outbound.recv().await {
        let Some(&index) = index_of.get(&to) else {
            continue;
        };
        let Ok(packet) = extract_packet_from_protocol(&msg) else {
            continue;
        };
        let PacketParseResult::Success(message) = to_casper_message_proto(&packet) else {
            continue;
        };
        let replies: Vec<CasperMessage> = match message {
            CasperMessageProto::HasBlockRequest(request) => {
                let hash = BlockHash::from(request.hash);
                behaviour
                    .served(index, &hash)
                    .map(|_| CasperMessage::HasBlock(HasBlock { hash }))
                    .into_iter()
                    .collect()
            }
            CasperMessageProto::BlockRequest(request) => behaviour
                .served(index, &BlockHash::from(request.hash))
                .map(CasperMessage::BlockMessage)
                .into_iter()
                .collect(),
            CasperMessageProto::ForkChoiceTipRequest(_) => behaviour
                .tips
                .iter()
                .map(|hash| CasperMessage::HasBlock(HasBlock { hash: hash.clone() }))
                .collect(),
            _ => Vec::new(),
        };
        for reply in replies {
            // The node's queue closes when its test ends; the peers stop with it.
            if let Err(err) = engine.handle(to.clone(), reply).await {
                tracing::warn!(target: REPORT_TARGET, "peers stop answering: {err}");
                return;
            }
        }
    }
}

// ── The node's Casper ───────────────────────────────────────────────────────

/// Casper over the node's real stores with block execution stubbed.
struct StubCasper {
    approved: BlockMessage,
    snapshot: CasperSnapshot,
    dag: BlockDagKeyValueStorage,
    buffer: CasperBufferKeyValueStorage,
    blocks: KeyValueBlockStore,
    retriever: BlockRetriever<PeerTransport>,
    runtime_manager: Arc<RuntimeManager>,
    validation: Gate,
    validation_started: Mutex<HashSet<BlockHash>>,
}

const RELEASE_FULL_SCAN_EVERY: u64 = 32;

impl StubCasper {
    fn validation_started(&self, hash: &BlockHash) -> bool {
        self.validation_started
            .lock()
            .expect("validation_started lock")
            .contains(hash)
    }

    fn equivocation_hashes(&self) -> Result<HashSet<BlockHash>, CasperError> {
        Ok(self.dag.access_equivocations_tracker(|tracker| {
            Ok(tracker
                .data()?
                .iter()
                .flat_map(|record| record.equivocation_detected_block_hashes.iter())
                .cloned()
                .collect())
        })?)
    }
}

#[async_trait]
impl Casper for StubCasper {
    async fn get_snapshot(&self) -> Result<CasperSnapshot, CasperError> {
        Ok(self.snapshot.clone())
    }

    async fn request_block_from_peers(&self, hash: BlockHash) -> Result<(), CasperError> {
        self.retriever
            .admit_hash(hash, None, AdmitHashReason::MissingDependencyRequested)
            .await
            .map(|_| ())
    }

    fn contains(&self, hash: &BlockHash) -> bool {
        self.buffer_contains(hash) || self.dag_contains(hash)
    }

    fn dag_contains(&self, hash: &BlockHash) -> bool {
        self.dag
            .get_representation()
            .expect("the DAG holds genesis from the node's start")
            .contains(hash)
    }

    fn buffer_contains(&self, hash: &BlockHash) -> bool {
        self.buffer.contains(&BlockHashSerde(hash.clone()))
    }

    fn buffer_waits_on_dependency(&self, hash: &BlockHash) -> bool {
        self.buffer
            .get_parents(&BlockHashSerde(hash.clone()))
            .is_some_and(|parents| {
                parents
                    .iter()
                    .any(|parent| !self.dag_contains(&BlockHash::from(parent.0.clone())))
            })
    }

    fn get_approved_block(&self) -> Result<&BlockMessage, CasperError> { Ok(&self.approved) }

    fn deploy(
        &self,
        _deploy: Signed<DeployData>,
    ) -> Result<Either<DeployError, DeployId>, CasperError> {
        unreachable!("the node under test receives no deploys")
    }

    async fn estimator(
        &self,
        _dag: &mut KeyValueDagRepresentation,
    ) -> Result<Vec<BlockHash>, CasperError> {
        unreachable!("the node under test does not propose")
    }

    fn get_version(&self) -> i64 { VERSION }

    async fn validate(
        &self,
        block: &BlockMessage,
        _snapshot: &mut CasperSnapshot,
    ) -> Result<Either<BlockError, ValidBlock>, CasperError> {
        self.validation_started
            .lock()
            .expect("validation_started lock")
            .insert(block.block_hash.clone());
        self.validation.pass().await;
        Ok(Either::Right(ValidBlock::Valid))
    }

    async fn validate_self_created(
        &self,
        _block: &BlockMessage,
        _snapshot: &mut CasperSnapshot,
        _pre_state_hash: Bytes,
        _post_state_hash: Bytes,
    ) -> Result<Either<BlockError, ValidBlock>, CasperError> {
        unreachable!("the node under test does not propose")
    }

    async fn handle_valid_block(
        &self,
        block: &BlockMessage,
    ) -> Result<KeyValueDagRepresentation, CasperError> {
        self.blocks.persist_block(block)?;
        Ok(atomic_insert_then_buffer(
            &self.dag,
            block,
            InsertMode::Normal,
            &self.buffer,
            BufferTransition::RemoveFromBuffer(BlockHashSerde(block.block_hash.clone())),
        )?)
    }

    fn handle_invalid_block(
        &self,
        _block: &BlockMessage,
        _status: &InvalidBlock,
        _dag: &KeyValueDagRepresentation,
    ) -> Result<KeyValueDagRepresentation, CasperError> {
        unreachable!("every block in these tests is valid")
    }

    fn get_dependency_free_from_buffer(&self) -> Result<Vec<BlockMessage>, CasperError> {
        let equivocations = self.equivocation_hashes()?;
        let invalid: HashSet<BlockHash> = self
            .dag
            .get_representation()?
            .invalid_blocks_map()?
            .into_keys()
            .collect();

        let released = self.buffer.take_released();
        let full_scan = self
            .buffer
            .count_release_scan()
            .is_multiple_of(RELEASE_FULL_SCAN_EVERY);
        let mut candidates: HashSet<BlockHash> = HashSet::new();
        if full_scan {
            for pendant in self.buffer.get_pendants() {
                candidates.insert(BlockHash::from(pendant.0.clone()));
            }
            for child in self
                .buffer
                .to_doubly_linked_dag()
                .child_to_parent_adjacency_list
                .keys()
            {
                candidates.insert(BlockHash::from(child.0.clone()));
            }
        } else {
            for hash in released {
                if self.buffer.is_pendant(&hash) || self.buffer.contains(&hash) {
                    candidates.insert(BlockHash::from(hash.0.clone()));
                }
            }
        }

        let mut ready = Vec::new();
        for hash in candidates {
            let Some(block) = self.blocks.get(&hash)? else {
                continue;
            };
            let dependencies_ready =
                proto_util::dependencies_hashes_of(&block)
                    .iter()
                    .all(|dependency| {
                        self.dag_contains(dependency)
                            || equivocations.contains(dependency)
                            || invalid.contains(dependency)
                    });
            if dependencies_ready {
                ready.push(block);
            }
        }
        Ok(ready)
    }

    fn get_all_from_buffer(&self) -> Result<Vec<BlockMessage>, CasperError> {
        let mut blocks = Vec::new();
        for hash in self
            .buffer
            .to_doubly_linked_dag()
            .child_to_parent_adjacency_list
            .keys()
        {
            if let Some(block) = self.blocks.get(&BlockHash::from(hash.clone()))? {
                blocks.push(block);
            }
        }
        Ok(blocks)
    }
}

#[async_trait]
impl MultiParentCasper for StubCasper {
    async fn fetch_dependencies(&self) -> Result<(), CasperError> {
        let mut unseen = Vec::new();
        for pendant in self.buffer.get_pendants() {
            let hash = BlockHash::from(pendant.0.clone());
            if self.blocks.get(&hash)?.is_none() {
                unseen.push(hash);
            }
        }
        for dependency in unseen {
            self.retriever
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
        _weights: HashMap<Validator, u64>,
    ) -> Result<f32, CasperError> {
        unreachable!("the node under test does not finalize")
    }

    async fn last_finalized_block(&self) -> Result<BlockMessage, CasperError> {
        Ok(self.approved.clone())
    }

    async fn block_dag(&self) -> Result<KeyValueDagRepresentation, CasperError> {
        Ok(self.dag.get_representation()?)
    }

    fn block_store(&self) -> &KeyValueBlockStore { &self.blocks }

    fn casper_shard_conf(&self) -> &CasperShardConf { &self.snapshot.on_chain_state.shard_conf }

    fn runtime_manager(&self) -> Arc<RuntimeManager> { self.runtime_manager.clone() }

    fn get_validator(&self) -> Option<ValidatorIdentity> { None }

    fn is_local_validator_bonded(&self) -> bool { false }

    async fn get_history_exporter(&self) -> Arc<dyn RSpaceExporter> {
        unreachable!("the node under test serves no state")
    }

    async fn has_pending_deploys_in_storage(&self) -> Result<bool, CasperError> { Ok(false) }
}

// ── The node ────────────────────────────────────────────────────────────────

/// Stores that outlive a node, so a node can be restarted over them.
struct Stores {
    manager: InMemoryStoreManager,
    genesis: BlockMessage,
}

impl Stores {
    async fn with_genesis(genesis: BlockMessage) -> Self {
        let mut manager = InMemoryStoreManager::new();
        let blocks = KeyValueBlockStore::create_from_kvm(&mut manager)
            .await
            .unwrap();
        let dag = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
        blocks.put_block_message(&genesis).unwrap();
        dag.insert(&genesis, InsertMode::Approved).unwrap();
        Self { manager, genesis }
    }
}

struct NodeSetup {
    peers: usize,
    behaviour: PeerBehaviour,
    block_requests: Gate,
    validation: Gate,
}

struct Node {
    engine: Arc<Running<PeerTransport>>,
    casper: Arc<StubCasper>,
    in_flight: Arc<InFlightBlocks>,
    peers: Vec<PeerNode>,
    sent: Arc<SentRequests>,
    tasks: Vec<JoinHandle<()>>,
}

impl Node {
    /// Start the node over `stores` and run its startup: the fork-choice tip
    /// request and the re-enqueue of stored buffer pendants.
    async fn start(stores: &mut Stores, setup: NodeSetup) -> Self {
        let manager = &mut stores.manager;
        let blocks = KeyValueBlockStore::create_from_kvm(manager).await.unwrap();
        let dag = BlockDagKeyValueStorage::new(manager).await.unwrap();
        let buffer = CasperBufferKeyValueStorage::new_from_kv_store(KeyValueTypedStoreImpl::new(
            manager.store("parents-map".to_string()).await.unwrap(),
        ))
        .await
        .unwrap();
        let (runtime_manager, _) = RuntimeManager::create_with_history_config(
            manager.r_space_stores().await.unwrap(),
            RuntimeManager::mergeable_store(manager).await.unwrap(),
            Genesis::default_mergeable_tags_arc(),
            ExternalServices::noop(),
            ExploratoryDeployConfig::for_tests(),
            Arc::new(ReplayLock::with_permits(PIPELINE_WIDTH + 2)),
        );

        let peers: Vec<PeerNode> = (0..setup.peers).map(peer).collect();
        let connections = ConnectionsCell::new();
        for p in &peers {
            connections.flat_modify(|c| c.add_conn(p.clone())).unwrap();
        }
        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();
        let transport = Arc::new(PeerTransport {
            outbound: outbound_tx,
            block_requests: setup.block_requests,
            seed_peers: Arc::new(peers.iter().take(SEED_PEERS).cloned().collect()),
            sent: Arc::new(SentRequests::default()),
        });
        let sent = transport.sent.clone();
        let conf = create_rp_conf_ask(peer(99), None, None);
        let retriever = BlockRetriever::new(
            Arc::new(Mutex::new(HashMap::new())),
            transport.clone(),
            connections.clone(),
            conf.clone(),
        );

        let casper = Arc::new(StubCasper {
            approved: stores.genesis.clone(),
            snapshot: TestCasperWithSnapshot::create_empty_snapshot(),
            dag: dag.clone(),
            buffer: buffer.clone(),
            blocks: blocks.clone(),
            retriever: retriever.clone(),
            runtime_manager: Arc::new(runtime_manager),
            validation: setup.validation,
            validation_started: Mutex::new(HashSet::new()),
        });
        let processor = Arc::new(BlockProcessor::new(BlockProcessorDependencies::new(
            blocks.clone(),
            buffer.clone(),
            dag,
            retriever.clone(),
            transport.clone(),
            connections.clone(),
            conf.clone(),
            None,
        )));

        let in_flight = Arc::new(InFlightBlocks::new());
        let (queue_tx, queue_rx) = mpsc::channel::<BlockQueueItem>(QUEUE_CHANNEL_CAPACITY);
        let mut results = BlockProcessorInstance::new(
            (queue_rx, queue_tx.clone()),
            processor,
            in_flight.clone(),
            PIPELINE_WIDTH,
        )
        .create()
        .unwrap();
        let mut tasks = vec![tokio::spawn(async move {
            while results.recv().await.is_some() {}
        })];

        let the_init = {
            let transport = transport.clone();
            let connections = connections.clone();
            let conf = conf.clone();
            let casper = casper.clone();
            let retriever = retriever.clone();
            let in_flight = in_flight.clone();
            let queue_tx = queue_tx.clone();
            Arc::new(move || {
                let transport = transport.clone();
                let connections = connections.clone();
                let conf = conf.clone();
                let casper = casper.clone();
                let retriever = retriever.clone();
                let in_flight = in_flight.clone();
                let queue_tx = queue_tx.clone();
                Box::pin(async move {
                    transport
                        .send_fork_choice_tip_request(&connections, &conf)
                        .await?;
                    let buffer = casper.buffer.clone();
                    let blocks = casper.blocks.clone();
                    send_buffer_pendants_to_casper(
                        casper, &buffer, &blocks, &retriever, &in_flight, &queue_tx,
                    )
                    .await
                }) as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
            })
        };

        let engine = Arc::new(Running::new(
            queue_tx,
            in_flight.clone(),
            casper.clone(),
            ApprovedBlock {
                candidate: ApprovedBlockCandidate {
                    block: stores.genesis.clone(),
                    required_sigs: 0,
                },
                sigs: vec![],
                floor_seed: None,
            },
            the_init,
            false,
            transport,
            conf,
            retriever.clone(),
            None,
        ));

        tasks.push(tokio::spawn(answer_requests(
            outbound_rx,
            engine.clone(),
            peers.clone(),
            setup.behaviour,
        )));
        {
            let casper = casper.clone();
            tasks.push(tokio::spawn(async move {
                loop {
                    if let Err(err) = casper.fetch_dependencies().await {
                        tracing::warn!(target: REPORT_TARGET, "fetch_dependencies failed: {err}");
                    }
                    if let Err(err) = retriever.request_all(REQUESTED_BLOCKS_TIMEOUT).await {
                        tracing::warn!(target: REPORT_TARGET, "request_all failed: {err}");
                    }
                    tokio::time::sleep(CASPER_LOOP_INTERVAL).await;
                }
            }));
        }

        engine.init().await.expect("node startup");
        Self {
            engine,
            casper,
            in_flight,
            peers,
            sent,
            tasks,
        }
    }

    /// Stop the node once nothing is queued or processing.
    async fn stop(self) {
        wait_until(Duration::from_secs(30), || self.in_flight.len() == 0)
            .await
            .expect("the node goes idle before it stops");
        for task in &self.tasks {
            task.abort();
        }
    }

    async fn gossip(&self, from: usize, b: &BlockMessage) {
        self.engine
            .handle(
                self.peers[from].clone(),
                CasperMessage::BlockMessage(b.clone()),
            )
            .await
            .unwrap();
    }

    fn in_dag(&self, b: &BlockMessage) -> bool { self.casper.dag_contains(&b.block_hash) }

    fn count_in_dag<'a>(&self, blocks: impl IntoIterator<Item = &'a BlockMessage>) -> usize {
        blocks.into_iter().filter(|b| self.in_dag(b)).count()
    }
}

/// Poll `done` every 10 ms until it holds or `timeout` passes.
async fn wait_until(timeout: Duration, mut done: impl FnMut() -> bool) -> Result<(), Duration> {
    let started = Instant::now();
    loop {
        if done() {
            return Ok(());
        }
        if started.elapsed() >= timeout {
            return Err(started.elapsed());
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

// ── R1 (2a): a block missing a parent must not hold a processing slot ──────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn a_ready_block_is_validated_while_blocks_missing_parents_wait_on_slow_requests() {
    init_tracing();
    let mut validators = Validators::new(4);
    let genesis = validators.genesis();
    let mut stores = Stores::with_genesis(genesis.clone()).await;
    let block_requests = Gate::new(false);
    let node = Node::start(&mut stores, NodeSetup {
        peers: 4,
        behaviour: PeerBehaviour::holding_nothing(),
        block_requests: block_requests.clone(),
        validation: Gate::new(true),
    })
    .await;

    let missing_parent_count = 2 * PIPELINE_WIDTH;
    for i in 0..missing_parent_count {
        let unheld_parent = validators.sign(1 + i % 3, &[&genesis]);
        let waiting = validators.sign(i % 4, &[&unheld_parent]);
        node.gossip(0, &waiting).await;
    }
    let ready = validators.sign(0, &[&genesis]);
    node.gossip(1, &ready).await;

    let outcome = wait_until(Duration::from_secs(10), || {
        node.casper.validation_started(&ready.block_hash)
    })
    .await;
    block_requests.open();
    assert!(
        outcome.is_ok(),
        "a block whose parents are all validated did not start validation within {:?} while \
         {missing_parent_count} blocks missing a parent waited on requests for their parents \
         ({} blocks queued or processing)",
        outcome.unwrap_err(),
        node.in_flight.len(),
    );
}

// ── R2 (2b): arriving blocks must not be dropped while validation is busy ──

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn every_ready_block_received_while_validation_is_busy_is_validated() {
    init_tracing();
    let mut validators = Validators::new(10);
    let genesis = validators.genesis();
    let mut stores = Stores::with_genesis(genesis.clone()).await;
    let validation = Gate::new(false);
    let node = Node::start(&mut stores, NodeSetup {
        peers: 4,
        behaviour: PeerBehaviour::holding_nothing(),
        block_requests: Gate::new(true),
        validation: validation.clone(),
    })
    .await;

    let tips: Vec<BlockMessage> = (0..600)
        .map(|i| validators.sign(i % 10, &[&genesis]))
        .collect();
    for (i, tip) in tips.iter().enumerate() {
        node.gossip(i % 4, tip).await;
    }
    validation.open();

    let outcome = wait_until(Duration::from_secs(60), || {
        node.count_in_dag(&tips) == tips.len()
    })
    .await;
    assert!(
        outcome.is_ok(),
        "{} of {} ready blocks received while validation was busy were validated after {:?}",
        node.count_in_dag(&tips),
        tips.len(),
        outcome.unwrap_err(),
    );
}

// ── R3 (2c): a restart must not put unready stored blocks into slots ───────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn a_restarted_node_validates_a_ready_block_while_stored_blocks_wait_on_lost_parents() {
    init_tracing();
    let mut validators = Validators::new(4);
    let genesis = validators.genesis();
    let mut stores = Stores::with_genesis(genesis.clone()).await;

    // Each waiting block is stored (its main parent, genesis, is validated)
    // and cites a second parent the node can only hold in the pending area,
    // because that parent's own main parent is withheld.
    let waiting_count = 2 * PIPELINE_WIDTH + 2;
    let mut withheld = Vec::new();
    let mut pending = Vec::new();
    let mut waiting = Vec::new();
    for _ in 0..waiting_count {
        let w = validators.sign(2, &[&genesis]);
        let p = validators.sign(1, &[&w]);
        let s = validators.sign(0, &[&genesis, &p]);
        withheld.push(w);
        pending.push(p);
        waiting.push(s);
    }

    let before_restart = Node::start(&mut stores, NodeSetup {
        peers: 4,
        behaviour: PeerBehaviour::holding_nothing(),
        block_requests: Gate::new(true),
        validation: Gate::new(true),
    })
    .await;
    for (p, s) in pending.iter().zip(&waiting) {
        before_restart.gossip(0, p).await;
        before_restart.gossip(0, s).await;
    }
    before_restart.stop().await;

    let block_requests = Gate::new(false);
    let all: Vec<&BlockMessage> = withheld.iter().chain(&pending).chain(&waiting).collect();
    let node = Node::start(&mut stores, NodeSetup {
        peers: 4,
        behaviour: PeerBehaviour::new(&all, |_, _| true)
            .with_tips(&waiting.iter().collect::<Vec<_>>()),
        block_requests: block_requests.clone(),
        validation: Gate::new(true),
    })
    .await;
    let ready = validators.sign(3, &[&genesis]);
    node.gossip(1, &ready).await;

    let ready_outcome = wait_until(Duration::from_secs(10), || {
        node.casper.validation_started(&ready.block_hash)
    })
    .await;
    block_requests.open();
    assert!(
        ready_outcome.is_ok(),
        "after a restart, a block whose parents are all validated did not start validation \
         within {:?} while {waiting_count} stored blocks waited on parents lost with the \
         pending area ({} blocks queued or processing)",
        ready_outcome.unwrap_err(),
        node.in_flight.len(),
    );

    let converged = wait_until(Duration::from_secs(60), || {
        node.count_in_dag(&waiting) == waiting_count
    })
    .await;
    assert!(
        converged.is_ok(),
        "{} of {waiting_count} stored waiting blocks were validated {:?} after their ancestors \
         became available",
        node.count_in_dag(&waiting),
        converged.unwrap_err(),
    );
}

// ── R4 (2d): a long-withheld ancestor must not lose its waiting chain ──────

/// The casper buffer prunes a root waiting this long, with its subtree.
const BUFFER_STALE_ROOT_AGE: Duration = Duration::from_secs(180);
/// How long the node gets once the ancestor is served: longer than the
/// retriever's cycle of a full retry budget plus its quarantine.
const AFTER_SERVED: Duration = Duration::from_secs(90);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn a_chain_waiting_on_a_long_withheld_ancestor_is_validated_once_it_is_served() {
    init_tracing();
    let mut validators = Validators::new(3);
    let genesis = validators.genesis();
    let mut stores = Stores::with_genesis(genesis.clone()).await;

    let ancestor = validators.sign(1, &[&genesis]);
    let child = validators.sign(0, &[&ancestor]);
    let grandchild = validators.sign(2, &[&child]);

    let withheld_for = BUFFER_STALE_ROOT_AGE + Duration::from_secs(10);
    let served_from = Instant::now() + withheld_for;
    let ancestor_hash = ancestor.block_hash.clone();
    let node = Node::start(&mut stores, NodeSetup {
        peers: 4,
        behaviour: PeerBehaviour::new(&[&ancestor], move |_, hash| {
            *hash == ancestor_hash && Instant::now() >= served_from
        }),
        block_requests: Gate::new(true),
        validation: Gate::new(true),
    })
    .await;
    node.gossip(0, &child).await;
    node.gossip(0, &grandchild).await;

    // The shard keeps producing while the ancestor is withheld: one unrelated
    // ready block a second.
    let chain = [ancestor, child, grandchild];
    let deadline = served_from + AFTER_SERVED;
    let mut traffic = 0usize;
    while node.count_in_dag(&chain) < chain.len() && Instant::now() < deadline {
        node.gossip(1 + traffic % 3, &validators.sign(traffic % 3, &[&genesis]))
            .await;
        traffic += 1;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    assert_eq!(
        node.count_in_dag(&chain),
        chain.len(),
        "a chain whose root ancestor was withheld for {withheld_for:?} was not validated within \
         {AFTER_SERVED:?} of the ancestor being served (each chain block was gossiped once; \
         {traffic} unrelated blocks arrived meanwhile)",
    );
}

// ── R5 (2e): many missing ancestors must each still reach a peer holding it ─

/// `waiting_count` blocks each cite a distinct missing parent that only the
/// peers after the first `SEED_PEERS` hold. Returns how many of the waiting
/// blocks were validated, how long that took, and the requests the node sent.
async fn missing_parents_held_beyond_the_seed_peers(
    waiting_count: usize,
) -> (usize, Result<(), Duration>, Arc<SentRequests>) {
    let mut validators = Validators::new(10);
    let genesis = validators.genesis();
    let mut stores = Stores::with_genesis(genesis.clone()).await;

    let mut parents = Vec::with_capacity(waiting_count);
    let mut waiting = Vec::with_capacity(waiting_count);
    for i in 0..waiting_count {
        let parent = validators.sign(i % 10, &[&genesis]);
        waiting.push(validators.sign((i + 1) % 10, &[&parent]));
        parents.push(parent);
    }
    let node = Node::start(&mut stores, NodeSetup {
        peers: SEED_PEERS + 6,
        behaviour: PeerBehaviour::new(&parents.iter().collect::<Vec<_>>(), |peer, _| {
            peer >= SEED_PEERS
        }),
        block_requests: Gate::new(true),
        validation: Gate::new(true),
    })
    .await;

    // Delivery is paced below the in-flight cap so no block is dropped there.
    for (i, w) in waiting.iter().enumerate() {
        wait_until(Duration::from_secs(60), || node.in_flight.len() < 256)
            .await
            .expect("the node drains its queue");
        node.gossip(SEED_PEERS + i % 6, w).await;
    }

    let outcome = wait_until(Duration::from_secs(90), || {
        node.count_in_dag(&waiting) == waiting_count
    })
    .await;
    (node.count_in_dag(&waiting), outcome, node.sent.clone())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn missing_parents_held_beyond_the_seed_peers_are_fetched() {
    init_tracing();
    let waiting_count = 1_500;
    let (validated, outcome, sent) =
        missing_parents_held_beyond_the_seed_peers(waiting_count).await;
    tracing::info!(target: REPORT_TARGET, waiting_count, "{sent}");
    assert!(
        outcome.is_ok(),
        "{validated} of {waiting_count} blocks were validated after {:?}; {sent}",
        outcome.unwrap_err(),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn missing_parents_held_beyond_the_seed_peers_are_fetched_when_thousands_are_missing() {
    init_tracing();
    let waiting_count = 4_000;
    let (validated, outcome, sent) =
        missing_parents_held_beyond_the_seed_peers(waiting_count).await;
    tracing::info!(target: REPORT_TARGET, waiting_count, "{sent}");
    assert!(
        outcome.is_ok(),
        "{validated} of {waiting_count} blocks, each missing a parent that only non-seed peers \
         hold, were validated after {:?}; {sent}",
        outcome.unwrap_err(),
    );
}
