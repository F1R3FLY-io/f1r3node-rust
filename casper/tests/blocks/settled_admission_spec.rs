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
use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, CertifiedAdmissionOutcome, CertifiedSenderAuthority, DeployId,
    InsertMode, KeyValueDagRepresentation,
};
use block_storage::rust::finality::FinalizationLedger;
use casper::rust::block_status::{CertifiedBlockValidation, InvalidBlock};
use casper::rust::blocks::block_processor::{
    new_block_processor, BlockProcessor, SettledAdmissionResult,
};
use casper::rust::casper::{Casper, CasperSnapshot, DeployError};
use casper::rust::engine::block_retriever::BlockRetriever;
use casper::rust::errors::CasperError;
use casper::rust::validator_identity::ValidatorIdentity;
use comm::rust::rp::connect::{Connections, ConnectionsCell};
use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
use crypto::rust::signatures::signed::Signed;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::block_implicits::get_random_block;
use models::rust::casper::protocol::casper_message::{BlockMessage, Bond, DeployData};
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::history::Either;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::KeyValueStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use crate::engine::setup;
use crate::helper::block_dag_storage_fixture::with_storage;
use crate::util::genesis_builder::DEFAULT_VALIDATOR_SKS;

struct AnchorCasper {
    anchor: BlockMessage,
}

#[async_trait]
impl Casper for AnchorCasper {
    async fn request_block_from_peers(&self, _hash: BlockHash) -> Result<(), CasperError> {
        unimplemented!("not exercised by settled-admission tests")
    }

    async fn get_snapshot(&self) -> Result<CasperSnapshot, CasperError> {
        unimplemented!("not exercised by settled-admission tests")
    }

    fn contains(&self, _hash: &BlockHash) -> bool { false }

    fn dag_contains(&self, _hash: &BlockHash) -> bool { false }

    fn buffer_contains(&self, _hash: &BlockHash) -> bool { false }

    fn get_approved_block(&self) -> Result<&BlockMessage, CasperError> { Ok(&self.anchor) }

    fn request_finalization(&self) -> Result<(), CasperError> {
        unimplemented!("not exercised by settled-admission tests")
    }

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
    ) -> Result<CertifiedBlockValidation, CasperError> {
        unimplemented!("not exercised by settled-admission tests")
    }

    async fn validate_self_created(
        &self,
        _block: &BlockMessage,
        _snapshot: &mut CasperSnapshot,
        _pre_state_hash: Bytes,
        _post_state_hash: Bytes,
    ) -> Result<CertifiedBlockValidation, CasperError> {
        unimplemented!("not exercised by settled-admission tests")
    }

    async fn handle_valid_block(
        &self,
        _block: &BlockMessage,
        _certificate: &CertifiedSenderAuthority,
        _outcome: &CertifiedAdmissionOutcome,
    ) -> Result<KeyValueDagRepresentation, CasperError> {
        unimplemented!("not exercised by settled-admission tests")
    }

    fn handle_invalid_block(
        &self,
        _block: &BlockMessage,
        _status: &InvalidBlock,
        _dag: &KeyValueDagRepresentation,
        _certificate: &CertifiedSenderAuthority,
        _outcome: &CertifiedAdmissionOutcome,
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
    ledger_store: Arc<dyn KeyValueStore>,
    block_store: block_storage::rust::key_value_block_store::KeyValueBlockStore,
    casper_buffer: CasperBufferKeyValueStorage,
    block_retriever: BlockRetriever<TransportLayerStub>,
    transport: Arc<TransportLayerStub>,
    connections_cell: ConnectionsCell,
    rp_conf: comm::rust::rp::rp_conf::RPConf,
    casper: Arc<dyn Casper + Send + Sync + 'static>,
    sender: Validator,
    live_head: BlockMessage,
    genesis: BlockMessage,
    bonds: Vec<Bond>,
    citer_identity: ValidatorIdentity,
    sender_identity: ValidatorIdentity,
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

        let (block_store, _indexed_dag_storage, casper_buffer) =
            with_storage(|bs, ids| async move {
                let mut kvm = InMemoryStoreManager::new();
                let store = kvm.store("parents-map".to_string()).await.unwrap();
                let typed_store = KeyValueTypedStoreImpl::new(store);
                let cb = CasperBufferKeyValueStorage::new_from_kv_store(
                    typed_store,
                    kvm.store(CasperBufferKeyValueStorage::PENDING_POLICY_NAMESPACE.into())
                        .await
                        .unwrap(),
                )
                .await
                .unwrap();
                (bs, ids, cb)
            })
            .await;

        let block_retriever = BlockRetriever::new(
            casper_buffer.clone(),
            transport.clone(),
            retriever_connections,
            rp_conf.clone(),
        );

        let mut dag_kvm = InMemoryStoreManager::new();
        let dag_storage = BlockDagKeyValueStorage::new(&mut dag_kvm).await.unwrap();
        let ledger_store = dag_kvm
            .store(FinalizationLedger::STORE_NAME.to_string())
            .await
            .unwrap();
        let dag_reader = dag_storage.clone();

        let sender_identity = ValidatorIdentity::new(&DEFAULT_VALIDATOR_SKS[1]);
        let sender = sender_identity.public_key.bytes.clone();
        let citer_identity = ValidatorIdentity::new(&DEFAULT_VALIDATOR_SKS[0]);
        let citer_sender = citer_identity.public_key.bytes.clone();
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
        dag_storage
            .insert(&genesis, InsertMode::ApprovedGenesis)
            .unwrap();

        // The node's live chain: sender's latest message sits at seq 5, one
        // below the anchor height.
        let live_head = sender_identity.sign_block(&lean_block(
            39,
            5,
            Some(sender.clone()),
            vec![genesis.block_hash.clone()],
            bonds.clone(),
        ));
        dag_storage.insert(&live_head, InsertMode::Normal).unwrap();

        let mut anchor = lean_block(40, 9, None, vec![genesis.block_hash.clone()], bonds.clone());
        anchor.block_hash = casper::rust::util::proto_util::hash_block(&anchor);
        let casper: Arc<dyn Casper + Send + Sync + 'static> = Arc::new(AnchorCasper { anchor });

        let processor = new_block_processor(
            block_store.clone(),
            dag_storage.clone(),
            block_retriever.clone(),
            transport.clone(),
            connections_cell.clone(),
            rp_conf.clone(),
            None,
        )
        .unwrap();

        Self {
            processor,
            dag_reader,
            ledger_store,
            block_store,
            casper_buffer,
            block_retriever,
            transport,
            connections_cell,
            rp_conf,
            casper,
            sender,
            live_head,
            genesis,
            bonds,
            citer_identity,
            sender_identity,
        }
    }

    /// Deliver a bonded citer naming `dep` as a missing dependency, which
    /// records the settled solicitation the door consumes on arrival.
    async fn solicit(&self, dep: &BlockHash) { self.solicit_with_sequence(dep, 1).await; }

    async fn solicit_with_sequence(&self, dep: &BlockHash, sequence: i32) {
        let citer = self.citer_identity.sign_block(&lean_block(
            41,
            sequence,
            Some(self.citer_identity.public_key.bytes.clone()),
            vec![dep.clone()],
            self.bonds.clone(),
        ));
        assert!(self
            .processor
            .check_if_well_formed_and_store(&citer)
            .await
            .unwrap());
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

    fn historic_target(&self, sequence_number: i32) -> BlockMessage {
        self.sender_identity.sign_block(&lean_block(
            6,
            sequence_number,
            Some(self.sender.clone()),
            vec![self.genesis.block_hash.clone()],
            self.bonds.clone(),
        ))
    }

    fn restarted_processor(&self) -> BlockProcessor<TransportLayerStub> {
        new_block_processor(
            self.block_store.clone(),
            self.dag_reader.clone(),
            self.block_retriever.clone(),
            self.transport.clone(),
            self.connections_cell.clone(),
            self.rp_conf.clone(),
            None,
        )
        .unwrap()
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
    let foreign = fixture.sender_identity.sign_block(&lean_block(
        6,
        40,
        Some(fixture.sender.clone()),
        vec![fixture.genesis.block_hash.clone()],
        fixture.bonds.clone(),
    ));
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

/// Deep settled history is routinely authored by validators that have since
/// unbonded and therefore hold no latest-message slot on this node. Such a
/// block must still go through the door: the fifth conjunct exists to refuse
/// live-chain material wearing a sub-anchor height, and live material always
/// HAS a live latest message. Refusing on an absent slot re-wedges exactly
/// the restore gaps the door was built to close.
#[tokio::test]
async fn settled_admission_admits_an_unbonded_historic_author() {
    let fixture = Fixture::new().await;

    let departed_identity = ValidatorIdentity::new(&DEFAULT_VALIDATOR_SKS[2]);
    let departed = departed_identity.public_key.bytes.clone();
    assert!(
        fixture
            .dag_reader
            .get_representation()
            .unwrap()
            .latest_message_hash(&departed)
            .is_none(),
        "the departed author must have no latest-message slot",
    );

    let historic = departed_identity.sign_block(&lean_block(
        6,
        3,
        Some(departed.clone()),
        vec![fixture.genesis.block_hash.clone()],
        fixture.bonds.clone(),
    ));
    fixture.solicit(&historic.block_hash).await;

    let admitted = fixture
        .processor
        .try_admit_settled(fixture.casper.clone(), &historic)
        .await
        .unwrap();

    assert_eq!(
        admitted,
        SettledAdmissionResult::Admitted,
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
    let straggler = fixture.sender_identity.sign_block(&lean_block(
        6,
        2,
        Some(fixture.sender.clone()),
        vec![fixture.genesis.block_hash.clone()],
        fixture.bonds.clone(),
    ));
    fixture.solicit(&straggler.block_hash).await;

    let admitted = fixture
        .processor
        .try_admit_settled(fixture.casper.clone(), &straggler)
        .await
        .unwrap();

    assert_eq!(
        admitted,
        SettledAdmissionResult::Admitted,
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

#[tokio::test]
async fn settled_admission_retries_after_precommit_storage_failure() {
    let fixture = Fixture::new().await;
    let target = fixture.historic_target(2);
    fixture.solicit(&target.block_hash).await;
    fixture.processor.fail_next_settled_insert();

    assert!(fixture
        .processor
        .try_admit_settled(fixture.casper.clone(), &target)
        .await
        .is_err());
    assert!(!fixture
        .dag_reader
        .get_representation()
        .unwrap()
        .contains(&target.block_hash));
    assert!(fixture
        .casper_buffer
        .get_children(&BlockHashSerde(target.block_hash.clone()))
        .is_some());

    assert_eq!(
        fixture
            .processor
            .try_admit_settled(fixture.casper.clone(), &target)
            .await
            .unwrap(),
        SettledAdmissionResult::Admitted
    );
}

#[tokio::test]
async fn settled_admission_duplicate_delivery_is_idempotent() {
    let fixture = Fixture::new().await;
    let target = fixture.historic_target(2);
    fixture.solicit(&target.block_hash).await;

    assert_eq!(
        fixture
            .processor
            .try_admit_settled(fixture.casper.clone(), &target)
            .await
            .unwrap(),
        SettledAdmissionResult::Admitted
    );
    assert_eq!(
        fixture
            .processor
            .try_admit_settled(fixture.casper.clone(), &target)
            .await
            .unwrap(),
        SettledAdmissionResult::AlreadyAdmitted
    );
}

#[tokio::test]
async fn settled_admission_equal_concurrent_deliveries_commit_once() {
    let fixture = Fixture::new().await;
    let target = fixture.historic_target(2);
    fixture.solicit(&target.block_hash).await;

    let first = fixture
        .processor
        .try_admit_settled(fixture.casper.clone(), &target);
    let second = fixture
        .processor
        .try_admit_settled(fixture.casper.clone(), &target);
    let (first, second) = tokio::join!(first, second);
    let results = [first.unwrap(), second.unwrap()];

    assert_eq!(
        results
            .iter()
            .filter(|result| **result == SettledAdmissionResult::Admitted)
            .count(),
        1
    );
    assert!(results.iter().all(|result| matches!(
        result,
        SettledAdmissionResult::Admitted
            | SettledAdmissionResult::DuplicateInFlight
            | SettledAdmissionResult::AlreadyAdmitted
    )));
}

#[tokio::test]
async fn settled_admission_restart_recognizes_durable_commit() {
    let fixture = Fixture::new().await;
    let target = fixture.historic_target(2);
    fixture.solicit(&target.block_hash).await;
    assert_eq!(
        fixture
            .processor
            .try_admit_settled(fixture.casper.clone(), &target)
            .await
            .unwrap(),
        SettledAdmissionResult::Admitted
    );

    let episode = fixture
        .dag_reader
        .current_recovery_episode(&target.shard_id, target.header.version)
        .unwrap();
    assert_eq!(
        fixture.dag_reader.settled_recovery_usage(&episode).unwrap(),
        1
    );
    fixture
        .dag_reader
        .clear_settled_recovery_state_for_tests()
        .unwrap();
    assert_eq!(
        fixture.dag_reader.settled_recovery_usage(&episode).unwrap(),
        0
    );
    assert_eq!(
        fixture
            .dag_reader
            .reconcile_settled_history_admissions(
                &fixture.block_store,
                fixture.casper.get_approved_block().unwrap(),
            )
            .unwrap(),
        1
    );
    assert_eq!(
        fixture.dag_reader.settled_recovery_usage(&episode).unwrap(),
        1
    );
    assert_eq!(
        fixture
            .dag_reader
            .reconcile_settled_history_admissions(
                &fixture.block_store,
                fixture.casper.get_approved_block().unwrap(),
            )
            .unwrap(),
        1
    );
    assert_eq!(
        fixture.dag_reader.settled_recovery_usage(&episode).unwrap(),
        1
    );

    let restarted = fixture.restarted_processor();
    assert_eq!(
        restarted
            .settled_admission_count(&target.shard_id, target.header.version)
            .unwrap(),
        1
    );
    assert_eq!(
        fixture
            .dag_reader
            .validate_settled_history_admissions(
                &fixture.block_store,
                fixture.casper.get_approved_block().unwrap(),
            )
            .unwrap(),
        1
    );
    let mut wrong_anchor = fixture.casper.get_approved_block().unwrap().clone();
    wrong_anchor.body.state.block_number += 1;
    wrong_anchor.block_hash = casper::rust::util::proto_util::hash_block(&wrong_anchor);
    assert!(fixture
        .dag_reader
        .validate_settled_history_admissions(&fixture.block_store, &wrong_anchor)
        .is_err());
    assert_eq!(
        restarted
            .try_admit_settled(fixture.casper.clone(), &target)
            .await
            .unwrap(),
        SettledAdmissionResult::AlreadyAdmitted
    );
}

#[tokio::test]
async fn settled_admission_startup_rejects_corrupt_durable_usage() {
    let fixture = Fixture::new().await;
    let target = fixture.historic_target(2);
    fixture.solicit(&target.block_hash).await;
    assert_eq!(
        fixture
            .processor
            .try_admit_settled(fixture.casper.clone(), &target)
            .await
            .unwrap(),
        SettledAdmissionResult::Admitted
    );
    let episode = fixture
        .dag_reader
        .current_recovery_episode(&target.shard_id, target.header.version)
        .unwrap();
    fixture
        .dag_reader
        .put_settled_recovery_usage_for_tests(&episode, 2)
        .unwrap();

    assert!(fixture
        .dag_reader
        .reconcile_settled_history_admissions(
            &fixture.block_store,
            fixture.casper.get_approved_block().unwrap(),
        )
        .is_err());
}

#[tokio::test]
async fn settled_admission_reconciliation_validates_all_bodies_before_any_migration_write() {
    for body_bytes in [0usize, 1024, 64 * 1024] {
        let fixture = Fixture::new().await;
        let mut targets = Vec::new();
        for sequence in 1..=4 {
            let mut target = fixture.historic_target(sequence);
            target.body.extra_bytes = Bytes::from(vec![sequence as u8; body_bytes]);
            let target = fixture.sender_identity.sign_block(&target);
            fixture
                .solicit_with_sequence(&target.block_hash, sequence)
                .await;
            assert_eq!(
                fixture
                    .processor
                    .try_admit_settled(fixture.casper.clone(), &target)
                    .await
                    .unwrap(),
                SettledAdmissionResult::Admitted,
            );
            targets.push(target);
        }
        fixture
            .dag_reader
            .clear_settled_recovery_state_for_tests()
            .unwrap();
        let before = fixture.ledger_store.to_map().unwrap();
        for target in &targets {
            let mut corrupt = target.clone();
            corrupt.body.extra_bytes = Bytes::from_static(b"corrupt durable body");
            fixture
                .block_store
                .put(target.block_hash.clone(), &corrupt)
                .unwrap();
            assert!(fixture
                .dag_reader
                .reconcile_settled_history_admissions(
                    &fixture.block_store,
                    fixture.casper.get_approved_block().unwrap(),
                )
                .is_err());
            assert_eq!(fixture.ledger_store.to_map().unwrap(), before);
            fixture
                .block_store
                .put(target.block_hash.clone(), target)
                .unwrap();
        }
        assert_eq!(
            fixture
                .dag_reader
                .validate_settled_history_admissions(
                    &fixture.block_store,
                    fixture.casper.get_approved_block().unwrap(),
                )
                .unwrap(),
            4
        );
        for _ in 0..2 {
            assert_eq!(
                fixture
                    .dag_reader
                    .reconcile_settled_history_admissions(
                        &fixture.block_store,
                        fixture.casper.get_approved_block().unwrap(),
                    )
                    .unwrap(),
                4
            );
            let episode = fixture
                .dag_reader
                .current_recovery_episode(&targets[0].shard_id, targets[0].header.version)
                .unwrap();
            assert_eq!(
                fixture.dag_reader.settled_recovery_usage(&episode).unwrap(),
                4
            );
        }
    }
}

#[tokio::test]
async fn settled_admission_rejects_an_unbonded_citer() {
    let fixture = Fixture::new().await;
    let target = fixture.historic_target(2);
    let unbonded = ValidatorIdentity::new(&DEFAULT_VALIDATOR_SKS[2]);
    let citer = unbonded.sign_block(&lean_block(
        41,
        1,
        Some(unbonded.public_key.bytes.clone()),
        vec![target.block_hash.clone()],
        fixture.bonds.clone(),
    ));
    assert!(fixture
        .processor
        .check_if_well_formed_and_store(&citer)
        .await
        .unwrap());
    assert!(!fixture
        .processor
        .check_dependencies_with_effects(fixture.casper.clone(), &citer)
        .await
        .unwrap());

    assert_eq!(
        fixture
            .processor
            .try_admit_settled(fixture.casper.clone(), &target)
            .await
            .unwrap(),
        SettledAdmissionResult::NotSolicited
    );
}
