// Settled-history door vs. consensus state — pinned to CI run 32588262605
// (amd64-docker session 35b31728, observer2 18:14:16): five solicited foreign
// blocks below the anchor were admitted unjudged, and the insert's
// seq-monotone latest-message update moved the shared sender key's latest
// message onto the foreign chain (seq 40 over the live seq-5 head), feeding
// the estimator a frontier this node does not hold.
//
//   settled_admission_never_advances_a_latest_message — the defect pin: a
//   solicited at-or-below-anchor block whose sender already has a live latest
//   message must leave that latest message untouched, admitted or not.
//
//   settled_admission_still_admits_a_genuine_straggler — the door's real job
//   (shard1 joiner2, own-history #4 at anchor #4): a straggler seq-below the
//   sender's latest message is admitted, and the latest message stays put.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, DeployId, InsertMode, KeyValueDagRepresentation,
};
use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use casper::rust::blocks::block_processor::{new_block_processor, BlockProcessor};
use casper::rust::casper::{Casper, CasperSnapshot, DeployError};
use casper::rust::engine::block_retriever::BlockRetriever;
use casper::rust::errors::CasperError;
use comm::rust::rp::connect::{Connections, ConnectionsCell};
use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond, DeployData};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use crate::engine::setup;
use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_util::generate_validator;

struct AnchorCasper {
    anchor: BlockMessage,
}

#[async_trait]
impl Casper for AnchorCasper {
    async fn get_snapshot(&self) -> Result<CasperSnapshot, CasperError> {
        unimplemented!("not exercised by settled-admission tests")
    }

    fn contains(&self, _hash: &BlockHash) -> bool { false }

    fn dag_contains(&self, _hash: &BlockHash) -> bool { false }

    fn buffer_contains(&self, _hash: &BlockHash) -> bool { false }

    fn get_approved_block(&self) -> Result<&BlockMessage, CasperError> { Ok(&self.anchor) }

    fn deploy(
        &self,
        _deploy: Signed<DeployData>,
    ) -> Result<Either<DeployError, DeployId>, CasperError> {
        unimplemented!("not exercised by settled-admission tests")
    }

    async fn estimator(
        &self,
        _dag: &mut KeyValueDagRepresentation,
    ) -> Result<Vec<BlockHash>, CasperError> {
        unimplemented!("not exercised by settled-admission tests")
    }

    fn get_version(&self) -> i64 { 1 }

    async fn validate(
        &self,
        _block: &BlockMessage,
        _snapshot: &mut CasperSnapshot,
    ) -> Result<Either<BlockError, ValidBlock>, CasperError> {
        unimplemented!("not exercised by settled-admission tests")
    }

    async fn validate_self_created(
        &self,
        _block: &BlockMessage,
        _snapshot: &mut CasperSnapshot,
        _pre_state_hash: Bytes,
        _post_state_hash: Bytes,
    ) -> Result<Either<BlockError, ValidBlock>, CasperError> {
        unimplemented!("not exercised by settled-admission tests")
    }

    async fn handle_valid_block(
        &self,
        _block: &BlockMessage,
    ) -> Result<KeyValueDagRepresentation, CasperError> {
        unimplemented!("not exercised by settled-admission tests")
    }

    fn handle_invalid_block(
        &self,
        _block: &BlockMessage,
        _status: &InvalidBlock,
        _dag: &KeyValueDagRepresentation,
    ) -> Result<KeyValueDagRepresentation, CasperError> {
        unimplemented!("not exercised by settled-admission tests")
    }

    fn get_dependency_free_from_buffer(&self) -> Result<Vec<BlockMessage>, CasperError> {
        Ok(Vec::new())
    }

    fn get_all_from_buffer(&self) -> Result<Vec<BlockMessage>, CasperError> { Ok(Vec::new()) }
}

struct Fixture {
    processor: BlockProcessor<TransportLayerStub>,
    dag_reader: BlockDagKeyValueStorage,
    casper: Arc<dyn Casper + Send + Sync + 'static>,
    sender: Validator,
    live_head: BlockMessage,
    genesis: BlockMessage,
    bonds: Vec<Bond>,
    citer_sender: Validator,
}

fn lean_block(
    number: i64,
    seq: i32,
    sender: Option<Validator>,
    parents: Vec<BlockHash>,
    bonds: Vec<Bond>,
) -> BlockMessage {
    get_random_block(
        Some(number),
        Some(seq),
        None,
        None,
        sender,
        None,
        None,
        Some(parents),
        None,
        Some(vec![]),
        Some(vec![]),
        Some(bonds),
        None,
        None,
    )
}

impl Fixture {
    async fn new() -> Self {
        let local_peer = setup::peer_node("test-peer", 40400);
        let connections_cell = ConnectionsCell {
            peers: Arc::new(Mutex::new(Connections::from_vec(vec![local_peer.clone()]))),
        };
        let rp_conf = create_rp_conf_ask(local_peer.clone(), None, None);
        let transport = Arc::new(TransportLayerStub::new());
        let retriever_connections = ConnectionsCell {
            peers: Arc::new(Mutex::new(Connections::from_vec(vec![local_peer.clone()]))),
        };
        let block_retriever = BlockRetriever::new(
            Arc::new(Mutex::new(HashMap::new())),
            transport.clone(),
            retriever_connections,
            rp_conf.clone(),
        );

        let (block_store, _indexed_dag_storage, casper_buffer) =
            with_storage(|bs, ids| async move {
                let mut kvm = InMemoryStoreManager::new();
                let store = kvm.store("parents-map".to_string()).await.unwrap();
                let typed_store = KeyValueTypedStoreImpl::new(store);
                let cb = CasperBufferKeyValueStorage::new_from_kv_store(typed_store)
                    .await
                    .unwrap();
                (bs, ids, cb)
            })
            .await;

        let mut dag_kvm = InMemoryStoreManager::new();
        let dag_storage = BlockDagKeyValueStorage::new(&mut dag_kvm).await.unwrap();
        let dag_reader = dag_storage.clone();

        let sender = generate_validator(Some("SharedKeyValidator"));
        let citer_sender = generate_validator(Some("AnchorBondedCiter"));
        let bonds = vec![
            Bond {
                validator: sender.clone(),
                stake: 100,
            },
            Bond {
                validator: citer_sender.clone(),
                stake: 100,
            },
        ];

        let genesis = lean_block(0, 0, None, vec![], vec![]);
        dag_storage.insert(&genesis, InsertMode::Approved).unwrap();

        // The node's live chain: sender's latest message sits at seq 5, one
        // below the anchor height.
        let live_head = lean_block(
            39,
            5,
            Some(sender.clone()),
            vec![genesis.block_hash.clone()],
            bonds.clone(),
        );
        dag_storage.insert(&live_head, InsertMode::Normal).unwrap();

        let anchor = lean_block(40, 9, None, vec![genesis.block_hash.clone()], bonds.clone());
        let casper: Arc<dyn Casper + Send + Sync + 'static> = Arc::new(AnchorCasper { anchor });

        let processor = new_block_processor(
            block_store,
            casper_buffer,
            dag_storage,
            block_retriever,
            transport,
            connections_cell,
            rp_conf,
            None,
        );

        Self {
            processor,
            dag_reader,
            casper,
            sender,
            live_head,
            genesis,
            bonds,
            citer_sender,
        }
    }

    /// Deliver a bonded citer naming `dep` as a missing dependency, which
    /// records the settled solicitation the door consumes on arrival.
    async fn solicit(&self, dep: &BlockHash) {
        let citer = lean_block(
            41,
            1,
            Some(self.citer_sender.clone()),
            vec![dep.clone()],
            self.bonds.clone(),
        );
        let ready = self
            .processor
            .check_dependencies_with_effects(self.casper.clone(), &citer)
            .await
            .unwrap();
        assert!(!ready, "citer must be missing its solicited dependency");
    }

    fn latest_message_of_sender(&self) -> Option<BlockHash> {
        self.dag_reader
            .get_representation()
            .unwrap()
            .latest_message_hash(&self.sender)
    }
}

#[tokio::test]
async fn settled_admission_never_advances_a_latest_message() {
    let fixture = Fixture::new().await;
    assert_eq!(
        fixture.latest_message_of_sender(),
        Some(fixture.live_head.block_hash.clone()),
    );

    // Foreign block: same sender key, higher seq than the live head, height
    // below the anchor — the observer2 shape.
    let foreign = lean_block(
        6,
        40,
        Some(fixture.sender.clone()),
        vec![fixture.genesis.block_hash.clone()],
        fixture.bonds.clone(),
    );
    fixture.solicit(&foreign.block_hash).await;

    fixture
        .processor
        .try_admit_settled(fixture.casper.clone(), &foreign)
        .await
        .unwrap();

    assert_eq!(
        fixture.latest_message_of_sender(),
        Some(fixture.live_head.block_hash.clone()),
        "settled-history admission moved the sender's latest message onto \
         the solicited block's chain",
    );
}

#[tokio::test]
async fn settled_admission_still_admits_a_genuine_straggler() {
    let fixture = Fixture::new().await;

    // Straggler: same sender, seq strictly below the live head's, height
    // below the anchor — settled history the restore closure missed.
    let straggler = lean_block(
        6,
        2,
        Some(fixture.sender.clone()),
        vec![fixture.genesis.block_hash.clone()],
        fixture.bonds.clone(),
    );
    fixture.solicit(&straggler.block_hash).await;

    let admitted = fixture
        .processor
        .try_admit_settled(fixture.casper.clone(), &straggler)
        .await
        .unwrap();

    assert!(
        admitted,
        "a genuine settled straggler must go through the door"
    );
    assert!(
        fixture
            .dag_reader
            .get_representation()
            .unwrap()
            .contains(&straggler.block_hash),
        "admitted straggler must be in the DAG",
    );
    assert_eq!(
        fixture.latest_message_of_sender(),
        Some(fixture.live_head.block_hash.clone()),
        "a straggler below the sender's latest message must not move it",
    );
}
