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

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, DeployId, InsertMode, KeyValueDagRepresentation,
};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use block_storage::rust::pending_blocks::PendingClass;
use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use casper::rust::blocks::ingest::ingest_state::WorkOutcome;
use casper::rust::blocks::ingest::validation_worker::Validation;
use casper::rust::casper::{Casper, CasperSnapshot, DeployError};
use casper::rust::errors::CasperError;
use casper::rust::validator_identity::ValidatorIdentity;
use comm::rust::rp::connect::{Connections, ConnectionsCell};
use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::BlockHash;
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond, DeployData};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

use crate::engine::setup;
use crate::helper::block_dag_storage_fixture::with_storage;

struct AnchorCasper {
    anchor: BlockMessage,
}

#[async_trait]
impl Casper for AnchorCasper {
    async fn get_snapshot(&self) -> Result<CasperSnapshot, CasperError> {
        unimplemented!("not exercised by settled-admission tests")
    }

    fn bucket_view(&self) -> Arc<casper::rust::economics::bucket_view::BucketView> {
        unimplemented!("not exercised by settled-admission tests")
    }

    fn dag_contains(&self, _hash: &BlockHash) -> Result<bool, CasperError> { Ok(false) }

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
        _parents: &[models::rust::block_metadata::BlockMetadata],
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
}

struct Fixture {
    validation: Validation<TransportLayerStub>,
    block_store: KeyValueBlockStore,
    dag_reader: BlockDagKeyValueStorage,
    casper: Arc<dyn Casper + Send + Sync + 'static>,
    sender_id: ValidatorIdentity,
    sender: Validator,
    live_head: BlockMessage,
    genesis: BlockMessage,
    bonds: Vec<Bond>,
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

fn identity() -> ValidatorIdentity { ValidatorIdentity::new(&Secp256k1.new_key_pair().0) }

fn signed_block(
    signer: &ValidatorIdentity,
    number: i64,
    seq: i32,
    parents: Vec<BlockHash>,
    bonds: Vec<Bond>,
) -> BlockMessage {
    signer.sign_block(&lean_block(number, seq, None, parents, bonds))
}

impl Fixture {
    async fn new() -> Self {
        let local_peer = setup::peer_node("test-peer", 40400);
        let connections_cell = ConnectionsCell {
            peers: Arc::new(Mutex::new(Connections::from_vec(vec![local_peer.clone()]))),
        };
        let rp_conf = create_rp_conf_ask(local_peer.clone(), None, None);
        let transport = Arc::new(TransportLayerStub::new());

        let block_store = with_storage(|bs, _ids| async move { bs }).await;

        let mut dag_kvm = InMemoryStoreManager::new();
        let dag_storage = BlockDagKeyValueStorage::new(&mut dag_kvm).await.unwrap();
        let dag_reader = dag_storage.clone();

        let sender_id = identity();
        let sender = sender_id.public_key.bytes.clone();
        let bonds = vec![Bond {
            validator: sender.clone(),
            stake: 100.into(),
        }];

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

        let validation = Validation::new(
            block_store.clone(),
            dag_storage,
            transport,
            connections_cell,
            rp_conf,
            None,
        );

        Self {
            validation,
            block_store,
            dag_reader,
            casper,
            sender_id,
            sender,
            live_head,
            genesis,
            bonds,
        }
    }

    /// Run the settled-history door on `block`, as block ingest dispatches
    /// it for a block a bonded validator cites at or below the anchor.
    async fn admit_settled(&self, block: &BlockMessage) -> bool {
        self.validation
            .admit_settled(self.casper.clone(), block)
            .await
            == WorkOutcome::Settled
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
    let foreign = signed_block(
        &fixture.sender_id,
        6,
        40,
        vec![fixture.genesis.block_hash.clone()],
        fixture.bonds.clone(),
    );
    fixture.admit_settled(&foreign).await;

    assert_eq!(
        fixture.latest_message_of_sender(),
        Some(fixture.live_head.block_hash.clone()),
        "settled-history admission moved the sender's latest message onto \
         the solicited block's chain",
    );
}

/// Deep settled history is routinely authored by validators that have since
/// unbonded and therefore hold no latest-message slot on this node. Such a
/// block must still go through the door: the fifth conjunct exists to refuse
/// live-chain material wearing a sub-anchor height, and live material always
/// HAS a live latest message. Refusing on an absent slot re-wedges exactly
/// the restore gaps the door was built to close.
#[tokio::test]
async fn settled_admission_admits_an_unbonded_historic_author() {
    let fixture = Fixture::new().await;

    let departed_id = identity();
    let departed = departed_id.public_key.bytes.clone();
    assert!(
        fixture
            .dag_reader
            .get_representation()
            .unwrap()
            .latest_message_hash(&departed)
            .is_none(),
        "the departed author must have no latest-message slot",
    );

    let historic = signed_block(
        &departed_id,
        6,
        3,
        vec![fixture.genesis.block_hash.clone()],
        fixture.bonds.clone(),
    );
    let admitted = fixture.admit_settled(&historic).await;

    assert!(
        admitted,
        "a solicited sub-anchor block from a sender with no latest-message \
         slot is settled history and must be admitted",
    );
    assert!(
        fixture
            .dag_reader
            .get_representation()
            .unwrap()
            .latest_message_hash(&departed)
            .is_none(),
        "settled-history admission must not create a latest-message slot \
         for the departed author",
    );
}

#[tokio::test]
async fn settled_admission_still_admits_a_genuine_straggler() {
    let fixture = Fixture::new().await;

    // Straggler: same sender, seq strictly below the live head's, height
    // below the anchor — settled history the restore closure missed.
    let straggler = signed_block(
        &fixture.sender_id,
        6,
        2,
        vec![fixture.genesis.block_hash.clone()],
        fixture.bonds.clone(),
    );
    let admitted = fixture.admit_settled(&straggler).await;

    assert!(
        admitted,
        "a genuine settled straggler must go through the door"
    );
    assert!(
        fixture
            .dag_reader
            .get_representation()
            .unwrap()
            .contains(&straggler.block_hash)
            .unwrap(),
        "admitted straggler must be in the DAG",
    );
    assert_eq!(
        fixture.latest_message_of_sender(),
        Some(fixture.live_head.block_hash.clone()),
        "a straggler below the sender's latest message must not move it",
    );
}

#[tokio::test]
async fn settled_admission_persists_a_pending_block() {
    let fixture = Fixture::new().await;

    let straggler = signed_block(
        &fixture.sender_id,
        6,
        2,
        vec![fixture.genesis.block_hash.clone()],
        fixture.bonds.clone(),
    );
    fixture
        .block_store
        .put_pending(&straggler, PendingClass::Solicited)
        .unwrap();
    assert!(fixture.block_store.is_pending(&straggler.block_hash));

    let admitted = fixture.admit_settled(&straggler).await;

    assert!(admitted);
    assert!(!fixture.block_store.is_pending(&straggler.block_hash));
    assert_eq!(
        fixture.block_store.get(&straggler.block_hash).unwrap(),
        Some(straggler.clone())
    );
}
