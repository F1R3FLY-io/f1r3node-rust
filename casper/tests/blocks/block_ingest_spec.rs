// Block ingest's owner never waits on the network: with every outbound send
// stuck and the outbound queue full, received blocks are still classified,
// ready blocks validated, and waiting children released when their parent
// is inserted.

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
use casper::rust::blocks::ingest::block_ingest::{self, IngestSetup, OUTBOUND_CAPACITY};
use casper::rust::casper::test_helpers::TestCasperWithSnapshot;
use casper::rust::casper::{
    Casper, CasperShardConf, CasperSnapshot, DeployError, MultiParentCasper,
};
use casper::rust::errors::CasperError;
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
    async fn get_snapshot(&self) -> Result<CasperSnapshot, CasperError> { Ok(self.snapshot.clone()) }

    fn dag_contains(&self, hash: &BlockHash) -> bool { self.dag.contains(hash) }

    fn get_approved_block(&self) -> Result<&BlockMessage, CasperError> { Ok(&self.approved) }

    fn deploy(&self, _deploy: Signed<DeployData>) -> Result<Either<DeployError, DeployId>, CasperError> {
        unreachable!("the node under test receives no deploys")
    }

    async fn estimator(&self, _dag: &mut KeyValueDagRepresentation) -> Result<Vec<BlockHash>, CasperError> {
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

    async fn handle_valid_block(&self, block: &BlockMessage) -> Result<KeyValueDagRepresentation, CasperError> {
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

    async fn last_finalized_block(&self) -> Result<BlockMessage, CasperError> { Ok(self.approved.clone()) }

    async fn block_dag(&self) -> Result<KeyValueDagRepresentation, CasperError> {
        Ok(self.dag.get_representation()?)
    }

    fn block_store(&self) -> &KeyValueBlockStore { &self.blocks }

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

fn block(number: i64, seq: i32, sender: Option<Validator>, parents: &[&BlockMessage], bonds: &[Bond]) -> BlockMessage {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn blocks_are_classified_validated_and_released_while_every_send_is_stuck() {
    let validator = ValidatorIdentity::new(&Secp256k1.new_key_pair().0);
    let bonds = vec![Bond {
        validator: validator.public_key.bytes.clone(),
        stake: 100,
    }];
    let mut seq = 0;
    let mut sign = |parents: &[&BlockMessage]| {
        seq += 1;
        let number = parents.iter().map(|p| p.body.state.block_number).max().unwrap_or(0) + 1;
        validator.sign_block(&block(
            number,
            seq,
            Some(validator.public_key.bytes.clone()),
            parents,
            &bonds,
        ))
    };

    let genesis = block(0, 0, None, &[], &bonds);
    let mut manager = InMemoryStoreManager::new();
    let blocks = KeyValueBlockStore::create_from_kvm(&mut manager).await.unwrap();
    let dag = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
    blocks.put_block_message(&genesis).unwrap();
    dag.insert(&genesis, InsertMode::Approved).unwrap();

    let peer = setup::peer_node("stuck-peer", 40400);
    let transport = Arc::new(StuckTransport {
        started: AtomicUsize::new(0),
    });
    let (ingest, _tasks) = block_ingest::spawn(IngestSetup {
        block_store: blocks.clone(),
        dag: dag.clone(),
        transport: transport.clone(),
        connections: ConnectionsCell {
            peers: Arc::new(std::sync::Mutex::new(Connections::from_vec(vec![peer.clone()]))),
        },
        conf: create_rp_conf_ask(setup::peer_node("ingest-local", 40401), None, None),
        validation_width: 3,
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

    // More needs than the outbound queue holds, each asked of a peer whose
    // sends never return.
    let unheld: Vec<BlockMessage> = (0..OUTBOUND_CAPACITY + 200).map(|_| sign(&[&genesis])).collect();
    let waiting: Vec<BlockMessage> = unheld.iter().map(|parent| sign(&[parent])).collect();
    for block in &waiting {
        ingest.deliver(block.clone(), peer.clone());
        assert!(
            wait_until(Duration::from_secs(10), || ingest.view().is_held(&block.block_hash)).await,
            "a block missing its parent is classified and held"
        );
    }
    assert!(
        unheld.iter().all(|parent| ingest.view().is_awaited(&parent.block_hash)),
        "every missing parent is needed"
    );
    assert!(
        wait_until(Duration::from_secs(10), || transport.started.load(Ordering::SeqCst) >= 1).await,
        "the request sender reached the stuck transport"
    );
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(
        transport.started.load(Ordering::SeqCst),
        1,
        "the request sender is stuck on its first send"
    );

    // A child arrives before its parent; both validate once the parent does.
    let parent = sign(&[&genesis]);
    let child = sign(&[&parent]);
    ingest.deliver(child.clone(), peer.clone());
    assert!(wait_until(Duration::from_secs(10), || ingest.view().is_held(&child.block_hash)).await);
    ingest.deliver(parent.clone(), peer.clone());

    assert!(
        wait_until(Duration::from_secs(30), || dag.contains(&parent.block_hash)
            && dag.contains(&child.block_hash))
        .await,
        "a ready block is validated and its waiting child released while every send is stuck"
    );
}
