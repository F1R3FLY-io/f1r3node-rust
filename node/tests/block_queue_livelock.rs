//! A node must keep admitting blocks while some of the blocks it holds wait
//! on ancestors it does not have.
//!
//! Each test drives a node built from the production receipt path
//! (`Running`) and block ingest as the node spawns it: intake, the owner of
//! what is held and needed, validation workers and the request sender, over
//! the block store, pending area and DAG store. Only block execution is
//! stubbed: `StubCasper::validate` accepts every block and can be held shut.
//!
//! Peers are simulated by `PeerTransport`: it records what the node sends and
//! a network task answers block requests from the blocks each peer holds.
//! Outbound block requests (`BlockRequest`, `HasBlockRequest`) can be held
//! shut, which stands for a peer that is slow to take a request.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, DeployId, InsertMode, KeyValueDagRepresentation,
};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::pending_blocks::PendingBlockLimits;
use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use casper::rust::blocks::ingest::block_ingest::{self, IngestHandle, IngestSetup};
use casper::rust::blocks::ingest::fetch_scheduler::SEED_PEERS;
use casper::rust::casper::test_helpers::TestCasperWithSnapshot;
use casper::rust::casper::{
    Casper, CasperShardConf, CasperSnapshot, DeployError, MultiParentCasper,
};
use casper::rust::engine::engine::Engine;
use casper::rust::engine::running::Running;
use casper::rust::errors::CasperError;
use casper::rust::genesis::genesis::Genesis;
use casper::rust::protocol::{
    extract_packet_from_protocol, to_casper_message_proto, CasperMessageProto, PacketParseResult,
};
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
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::{
    ApprovedBlock, ApprovedBlockCandidate, BlockMessage, Bond, CasperMessage, DeployData, HasBlock,
    Justification,
};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rholang::rust::interpreter::external_services::ExternalServices;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporter;
use serial_test::serial;
use tokio::sync::{mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};
use tracing::Level;
use tracing_subscriber::filter::{EnvFilter, FilterExt, Targets};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;

const SHARD: &str = "root";
const VERSION: i64 = 1;
/// The width the node derives on a four-core host.
const VALIDATION_WIDTH: usize = 3;
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
                stake: 100.into(),
            })
            .collect();
        Self {
            identities,
            bonds,
            next_seq: vec![1; count],
        }
    }

    fn genesis(&self) -> BlockMessage { block(0, 0, None, vec![], vec![], &self.bonds) }

    /// A block by validator `v` whose first parent is its main parent. It
    /// justifies each parent sender's latest parent, in validator order.
    fn sign(&mut self, v: usize, parents: &[&BlockMessage]) -> BlockMessage {
        let number = parents
            .iter()
            .map(|p| p.body.state.block_number)
            .max()
            .expect("every block but genesis has a parent")
            + 1;
        let seq = self.next_seq[v];
        self.next_seq[v] += 1;
        let mut latest: BTreeMap<Validator, &BlockMessage> = BTreeMap::new();
        for p in parents {
            latest
                .entry(p.sender.clone())
                .and_modify(|held| {
                    if p.body.state.block_number > held.body.state.block_number {
                        *held = p;
                    }
                })
                .or_insert(p);
        }
        let identity = &self.identities[v];
        identity.sign_block(&block(
            number,
            seq,
            Some(identity.public_key.bytes.clone()),
            parents.iter().map(|p| p.block_hash.clone()).collect(),
            latest
                .into_iter()
                .map(|(validator, p)| Justification {
                    validator,
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

/// Whether peer `index` holds a block.
type Holds = dyn Fn(usize, &BlockHash) -> bool + Send + Sync;

/// Which peer holds which block, and the tips peers report.
#[derive(Clone)]
struct PeerBehaviour {
    blocks: Arc<HashMap<BlockHash, BlockMessage>>,
    holds: Arc<Holds>,
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
    /// Block requests of either kind the node began to send, counted before
    /// they wait on `block_requests`.
    attempted: AtomicUsize,
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
            self.sent.attempted.fetch_add(1, Ordering::SeqCst);
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
    blocks: KeyValueBlockStore,
    ingest: IngestHandle,
    runtime_manager: Arc<RuntimeManager>,
    validation: Gate,
    validation_started: Mutex<HashSet<BlockHash>>,
    /// Validation of this block panics.
    panic_on: Mutex<Option<BlockHash>>,
}

impl StubCasper {
    fn validation_started(&self, hash: &BlockHash) -> bool {
        self.validation_started
            .lock()
            .expect("validation_started lock")
            .contains(hash)
    }
}

#[async_trait]
impl Casper for StubCasper {
    async fn get_snapshot(&self) -> Result<CasperSnapshot, CasperError> {
        Ok(self.snapshot.clone())
    }

    async fn request_block_from_peers(&self, hash: BlockHash) -> Result<(), CasperError> {
        self.ingest.need(hash).await
    }

    fn dag_contains(&self, hash: &BlockHash) -> bool { self.dag.contains(hash) }

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
        if self.panic_on.lock().expect("panic_on lock").as_ref() == Some(&block.block_hash) {
            panic!("validation of this block panics");
        }
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
        Ok(self.dag.insert(block, InsertMode::Normal)?)
    }

    fn handle_invalid_block(
        &self,
        _block: &BlockMessage,
        _status: &InvalidBlock,
        _dag: &KeyValueDagRepresentation,
    ) -> Result<KeyValueDagRepresentation, CasperError> {
        unreachable!("every block in these tests is valid")
    }
}

#[async_trait]
impl MultiParentCasper for StubCasper {
    fn normalized_initial_fault(
        &self,
        _weights: HashMap<Validator, num_bigint::BigInt>,
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

    fn genesis_block_hash(&self) -> Result<Option<BlockHash>, CasperError> {
        Ok(Some(self.approved.block_hash.clone()))
    }

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
    ingest: IngestHandle,
    peers: Vec<PeerNode>,
    sent: Arc<SentRequests>,
    tasks: Vec<JoinHandle<()>>,
    ingest_tasks: JoinSet<()>,
}

impl Node {
    /// Start the node over `stores` and run its startup: the fork-choice tip
    /// request.
    async fn start(stores: &mut Stores, setup: NodeSetup) -> Self {
        let manager = &mut stores.manager;
        let blocks = KeyValueBlockStore::create_from_kvm(manager).await.unwrap();
        let dag = BlockDagKeyValueStorage::new(manager).await.unwrap();
        let (runtime_manager, _) = RuntimeManager::create_with_history_config(
            manager.r_space_stores().await.unwrap(),
            RuntimeManager::mergeable_store(manager).await.unwrap(),
            Genesis::default_mergeable_tags_arc(),
            ExternalServices::noop(),
            ExploratoryDeployConfig::for_tests(),
            Arc::new(ReplayLock::with_permits(VALIDATION_WIDTH + 2)),
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

        let (ingest, ingest_tasks) = block_ingest::spawn(IngestSetup {
            block_store: blocks.clone(),
            dag: dag.clone(),
            transport: transport.clone(),
            connections: connections.clone(),
            conf: conf.clone(),
            validation_width: VALIDATION_WIDTH,
            max_number_of_parents: casper::rust::casper::UNLIMITED_PARENTS,
            state_root_fetch_tx: None,
            state_root_results: None,
        })
        .expect("block ingest starts");

        let casper = Arc::new(StubCasper {
            approved: stores.genesis.clone(),
            snapshot: TestCasperWithSnapshot::create_empty_snapshot(),
            dag,
            blocks,
            ingest: ingest.clone(),
            runtime_manager: Arc::new(runtime_manager),
            validation: setup.validation,
            validation_started: Mutex::new(HashSet::new()),
            panic_on: Mutex::new(None),
        });
        ingest
            .attach_casper(casper.clone())
            .await
            .expect("block ingest attaches to casper");

        let the_init = {
            let transport = transport.clone();
            let connections = connections.clone();
            let conf = conf.clone();
            Arc::new(move || {
                let transport = transport.clone();
                let connections = connections.clone();
                let conf = conf.clone();
                Box::pin(async move {
                    transport
                        .send_fork_choice_tip_request(&connections, &conf)
                        .await
                        .map_err(CasperError::from)
                }) as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
            })
        };

        let engine = Arc::new(Running::new(
            ingest.clone(),
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
            None,
        ));

        let tasks = vec![tokio::spawn(answer_requests(
            outbound_rx,
            engine.clone(),
            peers.clone(),
            setup.behaviour,
        ))];

        engine.init().await.expect("node startup");
        Self {
            engine,
            casper,
            ingest,
            peers,
            sent,
            tasks,
            ingest_tasks,
        }
    }

    /// Stop the node: its peers and block ingest's tasks end.
    async fn stop(mut self) {
        for task in &self.tasks {
            task.abort();
        }
        self.ingest_tasks.shutdown().await;
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

    /// Gossip `b` and wait until block ingest has taken it in.
    async fn gossip_and_settle(&self, from: usize, b: &BlockMessage) {
        self.gossip(from, b).await;
        wait_until(Duration::from_secs(30), || self.knows(b))
            .await
            .expect("block ingest takes in a gossiped block");
    }

    /// Block ingest holds `b`, or it is in the DAG.
    fn knows(&self, b: &BlockMessage) -> bool {
        self.ingest.view().is_held(&b.block_hash) || self.in_dag(b)
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

// ── A block missing a parent must not hold a processing slot ───────────────

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

    let missing_parent_count = 2 * VALIDATION_WIDTH;
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
         {missing_parent_count} blocks missing a parent waited on requests for their parents",
        outcome.unwrap_err(),
    );
}

// ── Arriving blocks must not be dropped while validation is busy ───────────

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

// ── A restart must not put unready stored blocks into slots ────────────────

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
    let waiting_count = 2 * VALIDATION_WIDTH + 2;
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
        before_restart.gossip_and_settle(0, p).await;
        before_restart.gossip_and_settle(0, s).await;
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
         pending area",
        ready_outcome.unwrap_err(),
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

// ── A long-withheld ancestor must not lose its waiting chain ───────────────

/// Long past the time re-requests take to reach their slowest interval.
const WITHHELD_FOR: Duration = Duration::from_secs(190);
/// How long the node gets once the ancestor is served: many times the
/// slowest re-request interval.
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

    let withheld_for = WITHHELD_FOR;
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

// ── Many missing ancestors must each still reach a peer holding it ─────────

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

    // Each waiting block is taken in before the next arrives, so no
    // unsolicited lane overflows: nothing would ask for a dropped tip again.
    for (i, w) in waiting.iter().enumerate() {
        node.gossip_and_settle(SEED_PEERS + i % 6, w).await;
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

// ── A parent validated while its child waits on a paused peer ─────────────

/// Three validators. The third is paused, so a request for a missing block
/// does not complete until it times out. The first validator's block arrives
/// before its parent; while the node waits on the paused peer for that
/// parent, the parent arrives from its author and is validated. Once the
/// paused peer's request ends, the first validator keeps extending its chain.
/// Every block must reach the DAG without being announced again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn a_chain_is_validated_when_its_parent_arrives_while_a_paused_peer_is_asked_for_it() {
    init_tracing();
    let mut validators = Validators::new(3);
    let genesis = validators.genesis();
    let mut stores = Stores::with_genesis(genesis.clone()).await;
    let paused_peer = Gate::new(false);
    let node = Node::start(&mut stores, NodeSetup {
        peers: 3,
        behaviour: PeerBehaviour::holding_nothing(),
        block_requests: paused_peer.clone(),
        validation: Gate::new(true),
    })
    .await;
    let author = 0;

    let parent = validators.sign(0, &[&genesis]);
    let child = validators.sign(0, &[&parent]);
    node.gossip(author, &child).await;
    wait_until(Duration::from_secs(10), || {
        node.sent.attempted.load(Ordering::SeqCst) > 0
    })
    .await
    .expect("the node asks for the missing parent");

    node.gossip(author, &parent).await;
    wait_until(Duration::from_secs(10), || node.in_dag(&parent))
        .await
        .expect("the parent, whose own parent is validated, is validated");
    paused_peer.open();

    let mut chain = vec![child];
    for _ in 0..6 {
        let next = validators.sign(0, &[chain.last().expect("the chain starts at the child")]);
        node.gossip(author, &next).await;
        chain.push(next);
    }

    let outcome = wait_until(Duration::from_secs(30), || {
        node.count_in_dag(&chain) == chain.len()
    })
    .await;
    assert!(
        outcome.is_ok(),
        "{} of {} blocks of a chain whose first block's parent arrived while a paused peer was \
         asked for it were validated after {:?}",
        node.count_in_dag(&chain),
        chain.len(),
        outcome.unwrap_err(),
    );
}

// ── A child received while its parent is being validated ───────────────────

/// Three validators. The first validator's block is held in validation. While
/// it is, the second validator's block on it arrives, then a block of the
/// first validator that cites that one. Every block request is held shut, so
/// nothing the node lacks can be fetched: once validation opens, every block
/// must reach the DAG from what the node already received.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn a_child_received_while_its_parent_is_validated_is_validated_without_a_request() {
    init_tracing();
    let mut validators = Validators::new(3);
    let genesis = validators.genesis();
    let mut stores = Stores::with_genesis(genesis.clone()).await;
    let paused_peer = Gate::new(false);
    let validation = Gate::new(false);
    let node = Node::start(&mut stores, NodeSetup {
        peers: 3,
        behaviour: PeerBehaviour::holding_nothing(),
        block_requests: paused_peer,
        validation: validation.clone(),
    })
    .await;

    let parent = validators.sign(0, &[&genesis]);
    node.gossip(0, &parent).await;
    wait_until(Duration::from_secs(10), || {
        node.casper.validation_started(&parent.block_hash)
    })
    .await
    .expect("the parent is held in validation");

    let child = validators.sign(1, &[&parent]);
    node.gossip_and_settle(1, &child).await;
    let grandchild = validators.sign(0, &[&child]);
    node.gossip_and_settle(0, &grandchild).await;
    assert!(!node.in_dag(&parent), "the parent is still in validation");

    validation.open();
    let chain = [parent, child, grandchild];
    let outcome = wait_until(Duration::from_secs(30), || {
        node.count_in_dag(&chain) == chain.len()
    })
    .await;
    assert!(
        outcome.is_ok(),
        "{} of {} blocks received while the first was in validation were validated after {:?}",
        node.count_in_dag(&chain),
        chain.len(),
        outcome.unwrap_err(),
    );
}

// ── Requested blocks competing for a nearly full pending area ──────────────

/// A chain Q ← P ← A ← B ← C where only Q's sender is known to be bonded in
/// its main parent, so P, A, B and C each need a slot in a pending area of two
/// blocks. C is pushed; the node asks for B, A, P and Q itself, more blocks
/// than the area holds. Ancestors must win the slots and every requested block
/// must reach the DAG.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn requested_blocks_competing_for_a_full_pending_area_all_reach_the_dag() {
    init_tracing();
    let mut validators = Validators::new(3);
    let genesis = validators.genesis();
    let q = validators.sign(0, &[&genesis]);
    let p = validators.sign(1, &[&q]);
    let a = validators.sign(1, &[&p]);
    let b = validators.sign(1, &[&a]);
    let c = validators.sign(2, &[&b]);
    let mut stores = Stores::with_genesis(genesis.clone()).await;
    let node = Node::start(&mut stores, NodeSetup {
        peers: 3,
        behaviour: PeerBehaviour::new(&[&q, &p, &a, &b], |_, _| true),
        block_requests: Gate::new(true),
        validation: Gate::new(true),
    })
    .await;
    node.casper.blocks.set_pending_limits(PendingBlockLimits {
        max_blocks: 2,
        ..PendingBlockLimits::default()
    });

    node.gossip(2, &c).await;
    let requested = [q, p, a, b];
    let outcome = wait_until(Duration::from_secs(30), || {
        node.count_in_dag(&requested) == requested.len()
    })
    .await;
    assert!(
        outcome.is_ok(),
        "{} of {} requested blocks reached the DAG through a two-block pending area after \
         {:?}; {}",
        node.count_in_dag(&requested),
        requested.len(),
        outcome.unwrap_err(),
        node.sent,
    );
}

// ── A flood of announcements must not hold off block requests ──────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn block_requests_keep_going_out_during_an_announcement_flood() {
    init_tracing();
    let mut validators = Validators::new(4);
    let genesis = validators.genesis();
    let mut stores = Stores::with_genesis(genesis.clone()).await;
    let node = Node::start(&mut stores, NodeSetup {
        peers: 4,
        behaviour: PeerBehaviour::holding_nothing(),
        block_requests: Gate::new(true),
        validation: Gate::new(true),
    })
    .await;
    let unheld_parent = validators.sign(1, &[&genesis]);
    let waiting = validators.sign(0, &[&unheld_parent]);
    node.gossip_and_settle(0, &waiting).await;

    let total = |sent: &SentRequests| {
        sent.block_requests_to_seed_peers.load(Ordering::SeqCst)
            + sent.block_requests_to_other_peers.load(Ordering::SeqCst)
            + sent.has_block_requests.load(Ordering::SeqCst)
    };
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flooders: Vec<JoinHandle<()>> = (0..3u64)
        .map(|t| {
            let ingest = node.ingest.clone();
            let from = node.peers[(t as usize) % node.peers.len()].clone();
            let stop = stop.clone();
            tokio::spawn(async move {
                let mut i: u64 = 0;
                while !stop.load(Ordering::Relaxed) {
                    let mut bytes = t.to_be_bytes().to_vec();
                    bytes.extend_from_slice(&i.to_be_bytes());
                    bytes.resize(32, 0);
                    ingest.announce(Bytes::from(bytes), from.clone());
                    i += 1;
                    if i.is_multiple_of(1024) {
                        tokio::task::yield_now().await;
                    }
                }
            })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let before_flood = total(&node.sent);
    tokio::time::sleep(Duration::from_secs(3)).await;
    let during_flood = total(&node.sent) - before_flood;
    stop.store(true, Ordering::Relaxed);
    for f in flooders {
        f.await.unwrap();
    }
    node.stop().await;
    assert!(
        during_flood > 0,
        "no block request went out during 3 s of announcement flood"
    );
}

// ── A panic validating one block must not stop block ingest ────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial]
async fn a_panic_validating_one_block_leaves_block_ingest_running() {
    init_tracing();
    let mut validators = Validators::new(4);
    let genesis = validators.genesis();
    let mut stores = Stores::with_genesis(genesis.clone()).await;
    let mut node = Node::start(&mut stores, NodeSetup {
        peers: 4,
        behaviour: PeerBehaviour::holding_nothing(),
        block_requests: Gate::new(true),
        validation: Gate::new(true),
    })
    .await;
    let poison = validators.sign(1, &[&genesis]);
    *node.casper.panic_on.lock().unwrap() = Some(poison.block_hash.clone());
    node.gossip(0, &poison).await;
    wait_until(Duration::from_secs(10), || {
        node.casper.validation_started(&poison.block_hash)
    })
    .await
    .expect("the poisoned block reaches validation");

    let healthy = validators.sign(2, &[&genesis]);
    node.gossip(1, &healthy).await;
    let validated = wait_until(Duration::from_secs(10), || node.in_dag(&healthy)).await;
    let ended = tokio::time::timeout(Duration::from_millis(100), node.ingest_tasks.join_next())
        .await
        .is_ok();
    node.stop().await;
    assert!(!ended, "an ingest task ended after a validation panic");
    assert!(
        validated.is_ok(),
        "a block received after the panic was not validated"
    );
}
