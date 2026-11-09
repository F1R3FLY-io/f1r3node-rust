// Block ingest against the network: the owner never waits on a send, and
// requests go out only for blocks the node still needs.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, DeployId, InsertMode, KeyValueDagRepresentation,
};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use casper::rust::blocks::ingest::block_ingest::{
    self, IngestHandle, IngestSetup, OUTBOUND_CAPACITY, OUTBOUND_CONCURRENCY,
};
use casper::rust::blocks::ingest::checked_block::CheckedBlock;
use casper::rust::casper::test_helpers::TestCasperWithSnapshot;
use casper::rust::casper::{
    Casper, CasperShardConf, CasperSnapshot, DeployError, MultiParentCasper,
};
use casper::rust::errors::CasperError;
use casper::rust::protocol::{
    extract_packet_from_protocol, to_casper_message_proto, CasperMessageProto, PacketParseResult,
};
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::validator_identity::ValidatorIdentity;
use comm::rust::errors::CommError;
use comm::rust::peer_node::PeerNode;
use comm::rust::rp::connect::{Connections, ConnectionsCell};
use comm::rust::test_instances::create_rp_conf_ask;
use comm::rust::transport::transport_layer::{Blob, TransportLayer};
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Signed;
use models::routing::Protocol;
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Bond, DeployData, Justification,
};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporter;
use tokio::task::JoinSet;

use crate::engine::setup;

const SHARD: &str = "root";
const VERSION: i64 = 1;

/// A transport whose every send hangs, counting the sends started.
struct StuckTransport {
    started: AtomicUsize,
}

#[async_trait]
impl TransportLayer for StuckTransport {
    async fn send(&self, _peer: &PeerNode, _msg: &Protocol) -> Result<(), CommError> {
        self.started.fetch_add(1, Ordering::SeqCst);
        futures::future::pending::<()>().await;
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

/// A transport whose every send takes `per_send`, counting the block requests
/// (`BlockRequest` and `HasBlockRequest`) it sends.
struct SlowTransport {
    per_send: Duration,
    requests_sent: AtomicUsize,
}

fn is_block_request(msg: &Protocol) -> bool {
    extract_packet_from_protocol(msg).is_ok_and(|packet| {
        matches!(
            to_casper_message_proto(&packet),
            PacketParseResult::Success(
                CasperMessageProto::BlockRequest(_) | CasperMessageProto::HasBlockRequest(_)
            )
        )
    })
}

#[async_trait]
impl TransportLayer for SlowTransport {
    async fn send(&self, _peer: &PeerNode, msg: &Protocol) -> Result<(), CommError> {
        tokio::time::sleep(self.per_send).await;
        if is_block_request(msg) {
            self.requests_sent.fetch_add(1, Ordering::SeqCst);
        }
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

/// Casper over the test's stores: every block validates, a valid block is
/// persisted and inserted.
struct AcceptingCasper {
    approved: BlockMessage,
    snapshot: CasperSnapshot,
    dag: BlockDagKeyValueStorage,
    blocks: KeyValueBlockStore,
}

#[async_trait]
impl Casper for AcceptingCasper {
    async fn get_snapshot(&self) -> Result<CasperSnapshot, CasperError> {
        Ok(self.snapshot.clone())
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
        _block: &BlockMessage,
        _snapshot: &mut CasperSnapshot,
    ) -> Result<Either<BlockError, ValidBlock>, CasperError> {
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
        unreachable!("every block in this test is valid")
    }
}

#[async_trait]
impl MultiParentCasper for AcceptingCasper {
    fn normalized_initial_fault(
        &self,
        _weights: std::collections::HashMap<Validator, u64>,
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

    fn get_validator(&self) -> Option<ValidatorIdentity> { None }

    fn is_local_validator_bonded(&self) -> bool { false }

    async fn get_history_exporter(&self) -> Arc<dyn RSpaceExporter> {
        unreachable!("the node under test serves no state")
    }

    fn runtime_manager(&self) -> Arc<RuntimeManager> {
        unreachable!("the node under test runs no state")
    }

    async fn has_pending_deploys_in_storage(&self) -> Result<bool, CasperError> { Ok(false) }
}

fn block(
    number: i64,
    seq: i32,
    sender: Option<Validator>,
    parents: &[&BlockMessage],
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
        Some(parents.iter().map(|p| p.block_hash.clone()).collect()),
        Some(
            parents
                .iter()
                .map(|p| Justification {
                    validator: p.sender.clone(),
                    latest_block_hash: p.block_hash.clone(),
                })
                .collect(),
        ),
        Some(vec![]),
        Some(vec![]),
        Some(bonds.to_vec()),
        Some(SHARD.to_string()),
        None,
    )
}

fn checked(block: &BlockMessage) -> CheckedBlock {
    CheckedBlock::check(block.clone(), "block-ingest-spec").expect("the test signs its blocks")
}

async fn wait_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while !done() {
        if started.elapsed() >= timeout {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    true
}

/// One node's block ingest over in-memory stores holding genesis, with one
/// connected peer and a validator that signs the test's blocks.
struct Node<T> {
    ingest: IngestHandle,
    _tasks: JoinSet<()>,
    dag: BlockDagKeyValueStorage,
    blocks: KeyValueBlockStore,
    genesis: BlockMessage,
    peer: PeerNode,
    transport: Arc<T>,
    validator: ValidatorIdentity,
    bonds: Vec<Bond>,
    seq: i32,
}

impl<T: TransportLayer + Send + Sync + 'static> Node<T> {
    async fn start(transport: T) -> Self {
        let validator = ValidatorIdentity::new(&Secp256k1.new_key_pair().0);
        let bonds = vec![Bond {
            validator: validator.public_key.bytes.clone(),
            stake: 100,
        }];
        let genesis = block(0, 0, None, &[], &bonds);
        let mut manager = InMemoryStoreManager::new();
        let blocks = KeyValueBlockStore::create_from_kvm(&mut manager)
            .await
            .unwrap();
        let dag = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
        blocks.put_block_message(&genesis).unwrap();
        dag.insert(&genesis, InsertMode::Approved).unwrap();

        let peer = setup::peer_node("remote-peer", 40400);
        let transport = Arc::new(transport);
        let (ingest, tasks) = block_ingest::spawn(IngestSetup {
            block_store: blocks.clone(),
            dag: dag.clone(),
            transport: transport.clone(),
            connections: ConnectionsCell {
                peers: Arc::new(std::sync::Mutex::new(Connections::from_vec(vec![
                    peer.clone()
                ]))),
            },
            conf: create_rp_conf_ask(setup::peer_node("ingest-local", 40401), None, None),
            validation_width: 3,
            max_number_of_parents: casper::rust::casper::UNLIMITED_PARENTS,
            state_root_fetch_tx: None,
            state_root_results: None,
        })
        .unwrap();
        ingest
            .attach_casper(Arc::new(AcceptingCasper {
                approved: genesis.clone(),
                snapshot: TestCasperWithSnapshot::create_empty_snapshot(),
                dag: dag.clone(),
                blocks: blocks.clone(),
            }))
            .await
            .unwrap();
        Self {
            ingest,
            _tasks: tasks,
            dag,
            blocks,
            genesis,
            peer,
            transport,
            validator,
            bonds,
            seq: 0,
        }
    }

    fn sign(&mut self, parents: &[&BlockMessage]) -> BlockMessage {
        self.seq += 1;
        let number = parents
            .iter()
            .map(|p| p.body.state.block_number)
            .max()
            .unwrap_or(0)
            + 1;
        self.validator.sign_block(&block(
            number,
            self.seq,
            Some(self.validator.public_key.bytes.clone()),
            parents,
            &self.bonds,
        ))
    }

    /// `count` blocks missing their parents, delivered and held: their
    /// parents are needed. Returns the parents.
    async fn hold_children_of_unheld_parents(&mut self, count: usize) -> Vec<BlockMessage> {
        let genesis = self.genesis.clone();
        let unheld: Vec<BlockMessage> = (0..count).map(|_| self.sign(&[&genesis])).collect();
        let waiting: Vec<BlockMessage> = unheld.iter().map(|parent| self.sign(&[parent])).collect();
        for block in &waiting {
            self.ingest.deliver(checked(block), self.peer.clone());
            assert!(
                wait_until(Duration::from_secs(10), || self
                    .ingest
                    .view()
                    .is_held(&block.block_hash))
                .await,
                "a block missing its parent is classified and held"
            );
        }
        assert!(
            unheld
                .iter()
                .all(|parent| self.ingest.view().is_awaited(&parent.block_hash)),
            "every missing parent is needed"
        );
        unheld
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn blocks_are_classified_validated_and_released_while_every_send_is_stuck() {
    let mut node = Node::start(StuckTransport {
        started: AtomicUsize::new(0),
    })
    .await;
    let transport = node.transport.clone();

    // More needs than the outbound queue holds, each asked of a peer whose
    // sends never return.
    node.hold_children_of_unheld_parents(OUTBOUND_CAPACITY + 200)
        .await;
    assert!(
        wait_until(Duration::from_secs(10), || {
            transport.started.load(Ordering::SeqCst) >= OUTBOUND_CONCURRENCY
        })
        .await,
        "the request sender reached the stuck transport"
    );
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(
        transport.started.load(Ordering::SeqCst),
        OUTBOUND_CONCURRENCY,
        "every concurrent send is stuck and no more are started"
    );

    // A child arrives before its parent; both validate once the parent does.
    let genesis = node.genesis.clone();
    let parent = node.sign(&[&genesis]);
    let child = node.sign(&[&parent]);
    node.ingest.deliver(checked(&child), node.peer.clone());
    assert!(
        wait_until(Duration::from_secs(10), || node
            .ingest
            .view()
            .is_held(&child.block_hash))
        .await
    );
    node.ingest.deliver(checked(&parent), node.peer.clone());

    assert!(
        wait_until(Duration::from_secs(30), || node
            .dag
            .contains(&parent.block_hash)
            && node.dag.contains(&child.block_hash))
        .await,
        "a ready block is validated and its waiting child released while every send is stuck"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_request_is_sent_once_every_needed_block_has_arrived() {
    let mut node = Node::start(SlowTransport {
        per_send: Duration::from_millis(20),
        requests_sent: AtomicUsize::new(0),
    })
    .await;
    let transport = node.transport.clone();

    let parents = node.hold_children_of_unheld_parents(300).await;
    for parent in &parents {
        node.ingest.deliver(checked(parent), node.peer.clone());
    }
    assert!(
        wait_until(Duration::from_secs(30), || parents
            .iter()
            .all(|parent| node.dag.contains(&parent.block_hash)))
        .await,
        "every needed block is delivered and validated"
    );
    assert!(parents
        .iter()
        .all(|parent| !node.ingest.view().is_awaited(&parent.block_hash)));

    let sent_when_nothing_was_needed = transport.requests_sent.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        transport.requests_sent.load(Ordering::SeqCst) - sent_when_nothing_was_needed,
        0,
        "requests kept going out for blocks the node already holds"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_bad_copy_this_node_stored_is_deleted_and_the_block_asked_for() {
    let mut node = Node::start(SlowTransport {
        per_send: Duration::from_millis(1),
        requests_sent: AtomicUsize::new(0),
    })
    .await;
    let transport = node.transport.clone();
    let genesis = node.genesis.clone();
    let honest = node.sign(&[&genesis]);
    let mut bad_copy = honest.clone();
    bad_copy.seq_num += 1;
    node.blocks
        .put(honest.block_hash.clone(), &bad_copy)
        .unwrap();

    node.ingest.need(honest.block_hash.clone()).await.unwrap();

    assert!(
        wait_until(Duration::from_secs(10), || node
            .blocks
            .get(&honest.block_hash)
            .unwrap()
            .is_none()
            && transport.requests_sent.load(Ordering::SeqCst) >= 1)
        .await,
        "the stored copy that fails the integrity check is deleted and the block asked for"
    );
    assert!(node.ingest.view().is_awaited(&honest.block_hash));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_valid_block_found_in_the_store_is_checked_and_validated() {
    let mut node = Node::start(StuckTransport {
        started: AtomicUsize::new(0),
    })
    .await;
    let genesis = node.genesis.clone();
    let block = node.sign(&[&genesis]);
    node.blocks.put_block_message(&block).unwrap();

    node.ingest.need(block.block_hash.clone()).await.unwrap();

    assert!(
        wait_until(Duration::from_secs(10), || node
            .dag
            .contains(&block.block_hash))
        .await,
        "a block read from the store is admitted and validated"
    );
}
