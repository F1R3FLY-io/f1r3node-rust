use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use casper::rust::blocks::block_processor::{BlockProcessor, BlockProcessorDependencies};
use casper::rust::casper::test_helpers::TestCasperWithSnapshot;
use casper::rust::casper::{
    Casper, CURRENT_CASPER_PROTOCOL_VERSION, LEGACY_CASPER_PROTOCOL_VERSION,
};
use casper::rust::engine::block_retriever::BlockRetriever;
use comm::rust::errors::CommError;
use comm::rust::rp::connect::{Connections, ConnectionsCell};
use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
use crypto::rust::public_key::PublicKey;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Bond, Header, ObjectiveEquivocationEvidence, ProcessedSystemDeploy,
    SystemDeployData,
};
use models::rust::equivocation_record::EquivocationRecord;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use crate::engine::setup;
use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::block_generator::create_genesis_block;
use crate::helper::block_util::generate_validator;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

struct TestFixture {
    dependencies: BlockProcessorDependencies<TransportLayerStub>,
    genesis: BlockMessage,
    test_block: BlockMessage,
}

impl TestFixture {
    async fn new() -> Self {
        let local_peer = setup::peer_node("test-peer", 40400);
        let connections_cell = ConnectionsCell {
            peers: Arc::new(Mutex::new(Connections::from_vec(vec![local_peer.clone()]))),
        };
        let rp_conf = create_rp_conf_ask(local_peer.clone(), None, None);
        let transport_layer = Arc::new(TransportLayerStub::new());

        // Create a new ConnectionsCell for BlockRetriever instead of cloning
        let connections_cell_for_retriever = ConnectionsCell {
            peers: Arc::new(Mutex::new(Connections::from_vec(vec![local_peer.clone()]))),
        };

        let requested_blocks = Arc::new(Mutex::new(HashMap::new()));
        let block_retriever = BlockRetriever::new(
            requested_blocks,
            transport_layer.clone(),
            connections_cell_for_retriever,
            rp_conf.clone(),
        );

        let (mut block_store, mut indexed_dag_storage, casper_buffer) =
            with_storage(|bs, ids| async move {
                // Create CasperBuffer from in-memory store
                let mut kvm = InMemoryStoreManager::new();
                let store = kvm.store("parents-map".to_string()).await.unwrap();
                let typed_store = KeyValueTypedStoreImpl::new(store);
                let cb = CasperBufferKeyValueStorage::new_from_kv_store(typed_store)
                    .await
                    .unwrap();

                (bs, ids, cb)
            })
            .await;

        // Get underlying BlockDagKeyValueStorage for CasperDependencyAnalyzer
        let block_dag_storage = {
            // We need to extract underlying storage for the dependency analyzer
            // For now, create a separate one since IndexedBlockDagStorage doesn't expose underlying
            let mut dag_kvm = InMemoryStoreManager::new();
            BlockDagKeyValueStorage::new(&mut dag_kvm).await.unwrap()
        };

        let v1 = generate_validator(Some("Test Validator"));
        let bonds = vec![Bond {
            validator: v1.clone(),
            stake: 100,
        }];

        let genesis = create_genesis_block(
            &mut block_store,
            &mut indexed_dag_storage,
            None,
            Some(bonds.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        // Create test block
        let test_block = crate::helper::block_generator::create_block(
            &mut block_store,
            &mut indexed_dag_storage,
            vec![genesis.block_hash.clone()],
            &genesis,
            Some(v1),
            Some(bonds),
            Some(HashMap::new()),
            None,
            None,
            None,
            None,
            None,
            None,
        );

        // Create unified dependencies
        let dependencies = BlockProcessorDependencies::new(
            block_store,
            casper_buffer,
            block_dag_storage,
            block_retriever,
            transport_layer,
            connections_cell,
            rp_conf,
        );

        Self {
            dependencies,
            genesis,
            test_block,
        }
    }

    fn reset_transport(&self) { self.dependencies.transport().reset(); }
}

#[tokio::test]
async fn peer_admission_uses_the_running_protocol_version() {
    let fixture = TestFixture::new().await;
    let mut approved = fixture.genesis.clone();
    approved.header.version = LEGACY_CASPER_PROTOCOL_VERSION;
    let mut current = fixture.test_block.clone();
    current.header.version = CURRENT_CASPER_PROTOCOL_VERSION;
    current.shard_id = approved.shard_id.clone();
    let mut legacy = current.clone();
    legacy.header.version = LEGACY_CASPER_PROTOCOL_VERSION;
    let mut snapshot = TestCasperWithSnapshot::create_empty_snapshot();
    snapshot.on_chain_state.shard_conf.casper_version = CURRENT_CASPER_PROTOCOL_VERSION;
    let casper = Arc::new(TestCasperWithSnapshot::new(snapshot, approved));
    let processor = BlockProcessor::new(fixture.dependencies);

    assert!(processor
        .check_if_of_interest(casper.clone(), &current)
        .unwrap());
    assert!(!processor.check_if_of_interest(casper, &legacy).unwrap());
}

#[tokio::test]
async fn request_missing_dependencies_should_call_admit_hash_for_each_dependency() {
    let fixture = TestFixture::new().await;
    fixture.reset_transport();

    // Create test dependencies
    let dep1 = BlockHash::from(b"dependency1".to_vec());
    let dep2 = BlockHash::from(b"dependency2".to_vec());
    let deps = HashSet::from([dep1.clone(), dep2.clone()]);

    // Call request_missing_dependencies using new architecture
    let result = fixture
        .dependencies
        .request_missing_dependencies(&deps)
        .await;
    assert!(result.is_ok());

    // Verify that both dependencies were requested
    let request_count = fixture.dependencies.transport().request_count();
    assert_eq!(
        request_count, 2,
        "Should have made 2 requests for 2 dependencies"
    );
}

#[tokio::test]
async fn request_missing_dependencies_should_handle_empty_set() {
    let fixture = TestFixture::new().await;
    fixture.reset_transport();

    let empty_deps = HashSet::new();

    let result = fixture
        .dependencies
        .request_missing_dependencies(&empty_deps)
        .await;
    assert!(result.is_ok());

    // No requests should be made for empty dependency set
    let request_count = fixture.dependencies.transport().request_count();
    assert_eq!(
        request_count, 0,
        "Should not make any requests for empty dependency set"
    );
}

#[tokio::test]
async fn commit_to_buffer_should_add_pendant_when_no_dependencies() {
    let fixture = TestFixture::new().await;

    // Commit block without dependencies (should become pendant)
    let result = fixture
        .dependencies
        .commit_to_buffer(&fixture.test_block, None)
        .await;
    assert!(result.is_ok());

    // Verify block was added as pendant
    let buffer = fixture.dependencies.casper_buffer();
    let pendants = buffer.get_pendants();
    assert!(
        pendants
            .iter()
            .any(|p| p.0 == fixture.test_block.block_hash),
        "Block should be added as pendant when no dependencies provided"
    );
}

#[tokio::test]
async fn commit_to_buffer_should_add_relations_when_dependencies_provided() {
    let fixture = TestFixture::new().await;

    // Create dependency set
    let deps = HashSet::from([fixture.genesis.block_hash.clone()]);

    // Commit block with dependencies
    let result = fixture
        .dependencies
        .commit_to_buffer(&fixture.test_block, Some(deps))
        .await;
    assert!(result.is_ok());

    // Verify block was added with relations
    let buffer = fixture.dependencies.casper_buffer();
    let block_hash_serde =
        models::rust::block_hash::BlockHashSerde(fixture.test_block.block_hash.clone());
    assert!(
        buffer.contains(&block_hash_serde),
        "Block should be added with relations when dependencies provided"
    );
}

#[tokio::test]
async fn remove_from_buffer_should_remove_block() {
    let fixture = TestFixture::new().await;

    // First add block to buffer
    let result = fixture
        .dependencies
        .commit_to_buffer(&fixture.test_block, None)
        .await;
    assert!(result.is_ok());

    // Verify block is in buffer
    let buffer = fixture.dependencies.casper_buffer();
    let pendants = buffer.get_pendants();
    assert!(
        pendants
            .iter()
            .any(|p| p.0 == fixture.test_block.block_hash),
        "Block should be in buffer before removal"
    );

    // Remove block from buffer
    let result = fixture
        .dependencies
        .remove_from_buffer(&fixture.test_block)
        .await;
    assert!(result.is_ok());

    // Verify block was removed
    let buffer = fixture.dependencies.casper_buffer();
    let pendants = buffer.get_pendants();
    assert!(
        !pendants
            .iter()
            .any(|p| p.0 == fixture.test_block.block_hash),
        "Block should be removed from buffer"
    );
}

#[tokio::test]
async fn local_validation_fault_recovery_removes_pendant_before_rerequest() {
    let fixture = TestFixture::new().await;
    fixture
        .dependencies
        .commit_to_buffer(&fixture.test_block, None)
        .await
        .expect("commit pendant");
    fixture.reset_transport();

    fixture
        .dependencies
        .recover_after_local_validation_fault(&fixture.test_block.block_hash)
        .await
        .expect("defer local fault");

    let block_hash =
        models::rust::block_hash::BlockHashSerde(fixture.test_block.block_hash.clone());
    assert!(!fixture.dependencies.casper_buffer().contains(&block_hash));
    assert!(!fixture.dependencies.casper_buffer().is_pendant(&block_hash));
    assert_eq!(fixture.dependencies.transport().request_count(), 1);
}

#[tokio::test]
async fn local_validation_fault_recovery_never_restores_ready_pendant_after_transport_failure() {
    let fixture = TestFixture::new().await;
    fixture
        .dependencies
        .commit_to_buffer(&fixture.test_block, None)
        .await
        .expect("commit pendant");
    fixture.reset_transport();
    fixture
        .dependencies
        .transport()
        .set_responses(|_, _| Err(CommError::TimeOut));

    let result = fixture
        .dependencies
        .recover_after_local_validation_fault(&fixture.test_block.block_hash)
        .await;

    let block_hash =
        models::rust::block_hash::BlockHashSerde(fixture.test_block.block_hash.clone());
    assert!(result.is_err());
    assert!(!fixture.dependencies.casper_buffer().contains(&block_hash));
    assert!(!fixture.dependencies.casper_buffer().is_pendant(&block_hash));
    assert_eq!(fixture.dependencies.transport().request_count(), 1);
}

#[tokio::test]
async fn descendant_remains_blocked_after_locally_faulted_parent_leaves_ready_queue() {
    let mut genesis_builder = GenesisBuilder::new();
    let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let genesis = genesis_builder
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let node = TestNode::standalone(genesis).await.expect("node");
    let mut parent = node.genesis.clone();
    parent.block_hash = Bytes::from(vec![0xd1; 32]);
    parent.header.parents_hash_list = vec![node.genesis.block_hash.clone()];
    let mut child = parent.clone();
    child.block_hash = Bytes::from(vec![0xd2; 32]);
    child.header.parents_hash_list = vec![parent.block_hash.clone()];
    node.block_store
        .put_block_message(&parent)
        .expect("store parent");
    node.block_store
        .put_block_message(&child)
        .expect("store child");

    let parent_hash = models::rust::block_hash::BlockHashSerde(parent.block_hash.clone());
    let child_hash = models::rust::block_hash::BlockHashSerde(child.block_hash.clone());
    node.casper
        .casper_buffer_storage
        .put_pendant(parent_hash.clone())
        .expect("buffer parent");
    node.casper
        .casper_buffer_storage
        .add_relation(parent_hash.clone(), child_hash)
        .expect("buffer child");
    node.casper
        .casper_buffer_storage
        .remove(parent_hash)
        .expect("defer parent");

    let ready = node
        .casper
        .get_dependency_free_from_buffer()
        .expect("resolve buffer");
    assert!(ready
        .iter()
        .all(|block| block.block_hash != child.block_hash));
}

#[tokio::test]
async fn buffer_manager_should_handle_concurrent_operations() {
    use tokio::task;

    let fixture = TestFixture::new().await;

    // Create multiple tasks that operate on the buffer concurrently
    let mut tasks = Vec::new();
    for _i in 0..10 {
        let casper_buffer = fixture.dependencies.casper_buffer().clone();

        let task = task::spawn(async move {
            let buffer = casper_buffer;
            // Simulate concurrent buffer operations
            let pendants = buffer.get_pendants();
            pendants.len() // Return some value to verify task completed
        });

        tasks.push(task);
    }

    // Wait for all tasks to complete
    let results = futures::future::join_all(tasks).await;

    // Verify all tasks completed successfully
    assert_eq!(results.len(), 10);
    for result in results {
        assert!(result.is_ok());
    }
}

#[tokio::test]
#[allow(clippy::assertions_on_constants)]
async fn block_processor_components_should_work_together() {
    let fixture = TestFixture::new().await;

    // Test CasperBuffer logic correctly:
    // 1. Add block as pendant (no dependencies)
    // 2. Add another block that depends on the first one
    // 3. Remove the first block (which is now a parent)

    // 1. Add test_block as pendant (no dependencies)
    let result = fixture
        .dependencies
        .commit_to_buffer(&fixture.test_block, None)
        .await;
    assert!(result.is_ok());

    // Verify test_block is pendant
    let buffer = fixture.dependencies.casper_buffer();
    let block_hash_serde =
        models::rust::block_hash::BlockHashSerde(fixture.test_block.block_hash.clone());
    assert!(buffer.is_pendant(&block_hash_serde));

    // 2. Create another block that depends on test_block
    let dependent_block = BlockMessage {
        block_hash: prost::bytes::Bytes::from(b"dependent_block".to_vec()),
        header: Header {
            parents_hash_list: vec![fixture.test_block.block_hash.clone()],
            timestamp: 0,
            version: 1,
            extra_bytes: prost::bytes::Bytes::new(),
            sender_bond_generation: None,
            objective_equivocation_evidence_delta: vec![],
        },
        body: fixture.test_block.body.clone(), // Use same body as test block
        justifications: vec![],
        sender: prost::bytes::Bytes::new(),
        seq_num: 0,
        sig: prost::bytes::Bytes::new(),
        sig_algorithm: String::new(),
        shard_id: String::new(),
        extra_bytes: prost::bytes::Bytes::new(),
    };

    let deps = HashSet::from([fixture.test_block.block_hash.clone()]);
    let result = fixture
        .dependencies
        .commit_to_buffer(&dependent_block, Some(deps))
        .await;
    assert!(result.is_ok());

    // 3. Now test_block is a parent, so we can remove it
    let result = fixture
        .dependencies
        .remove_from_buffer(&fixture.test_block)
        .await;
    assert!(result.is_ok());

    // 4. Test other operations
    let deps = HashSet::from([fixture.genesis.block_hash.clone()]);
    let result = fixture
        .dependencies
        .request_missing_dependencies(&deps)
        .await;
    assert!(result.is_ok());

    // 5. Acknowledge processing
    let result = fixture
        .dependencies
        .ack_processed(&fixture.test_block)
        .await;
    assert!(result.is_ok());

    // All operations should complete successfully
    assert!(true, "All components should work together");
}

#[tokio::test]
async fn slash_evidence_is_fetched_before_block_validation() {
    let mut genesis_builder = GenesisBuilder::new();
    let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let genesis = genesis_builder
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let node = TestNode::standalone(genesis).await.expect("node");
    let evidence_hash = Bytes::from(vec![0xa5; 32]);
    let mut incoming = node.genesis.clone();
    incoming.block_hash = Bytes::from(vec![0xb6; 32]);
    incoming.header.parents_hash_list = vec![node.genesis.block_hash.clone()];
    incoming.body.system_deploys = vec![ProcessedSystemDeploy::Succeeded {
        event_list: vec![],
        system_deploy: SystemDeployData::Slash {
            invalid_block_hash: evidence_hash.clone(),
            equivocation_block_hash: None,
            issuer_public_key: PublicKey::from_bytes(&incoming.sender),
            target_activation_epoch: 0,
            target_bond_generation: models::rust::bond_generation::BondGeneration::GENESIS,
        },
        pre_state_hash: Bytes::new(),
        post_state_hash: Bytes::new(),
    }];
    node.block_dag_storage
        .access_equivocations_tracker(|tracker| {
            tracker.add(EquivocationRecord::new(
                incoming.sender.clone(),
                models::rust::bond_generation::BondGeneration::GENESIS,
                0,
                BTreeSet::from([evidence_hash.clone()]),
            ))
        })
        .expect("tracker evidence");

    let ready = node
        .block_processor
        .check_dependencies_with_effects(node.casper.clone(), &incoming)
        .await
        .expect("dependency check");
    assert!(!ready);
    assert!(node
        .requested_blocks
        .lock()
        .expect("requested blocks")
        .contains_key(&evidence_hash));

    let mut evidence = node.genesis.clone();
    evidence.block_hash = evidence_hash;
    evidence.header.parents_hash_list = vec![node.genesis.block_hash.clone()];
    evidence.header.sender_bond_generation =
        Some(models::rust::bond_generation::BondGeneration::GENESIS);
    evidence.sender = Bytes::from(vec![0xa6; models::rust::validator::LENGTH]);
    evidence.seq_num = 1;
    evidence.body.state.bonds = vec![Bond {
        validator: evidence.sender.clone(),
        stake: 1,
    }];
    node.block_store
        .put_block_message(&evidence)
        .expect("store evidence");
    node.block_dag_storage
        .insert(&evidence, InsertMode::Invalid)
        .expect("index invalid evidence");

    let ready = node
        .block_processor
        .check_dependencies_with_effects(node.casper.clone(), &incoming)
        .await
        .expect("dependency check after evidence");
    assert!(ready);
}

#[tokio::test]
async fn tracker_witness_alone_does_not_suppress_block_admission() {
    let mut genesis_builder = GenesisBuilder::new();
    let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let genesis = genesis_builder
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let node = TestNode::standalone(genesis).await.expect("node");
    let tracker_only_hash = Bytes::from(vec![0xc7; 32]);
    let mut incoming = node.genesis.clone();
    incoming.block_hash = tracker_only_hash.clone();
    node.block_dag_storage
        .access_equivocations_tracker(|tracker| {
            tracker.add(EquivocationRecord::new(
                incoming.sender.clone(),
                models::rust::bond_generation::BondGeneration::GENESIS,
                0,
                BTreeSet::from([tracker_only_hash.clone()]),
            ))
        })
        .expect("tracker witness");

    assert!(!node.casper.contains(&tracker_only_hash));
    assert!(node
        .block_processor
        .check_if_of_interest(node.casper.clone(), &incoming)
        .expect("interest check"));
}

#[tokio::test]
async fn tracker_witness_cannot_satisfy_a_certified_block_dependency() {
    let mut genesis_builder = GenesisBuilder::new();
    let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let genesis = genesis_builder
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let node = TestNode::standalone(genesis).await.expect("node");
    let tracker_only_hash = Bytes::from(vec![0xc8; 32]);
    let mut incoming = node.genesis.clone();
    incoming.block_hash = Bytes::from(vec![0xc9; 32]);
    incoming.header.parents_hash_list = vec![tracker_only_hash.clone()];
    node.block_dag_storage
        .access_equivocations_tracker(|tracker| {
            tracker.add(EquivocationRecord::new(
                incoming.sender.clone(),
                models::rust::bond_generation::BondGeneration::GENESIS,
                0,
                BTreeSet::from([tracker_only_hash.clone()]),
            ))
        })
        .expect("tracker witness");

    let ready = node
        .block_processor
        .check_dependencies_with_effects(node.casper.clone(), &incoming)
        .await
        .expect("dependency check");

    assert!(!ready);
    assert!(node
        .requested_blocks
        .lock()
        .expect("requested blocks")
        .contains_key(&tracker_only_hash));
}

#[tokio::test]
async fn objective_pair_requires_both_admitted_metadata_records() {
    let mut genesis_builder = GenesisBuilder::new();
    let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let genesis = genesis_builder
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let node = TestNode::standalone(genesis).await.expect("node");
    let first_hash = Bytes::from(vec![0xd1; models::rust::block_hash::LENGTH]);
    let second_hash = Bytes::from(vec![0xd2; models::rust::block_hash::LENGTH]);
    let evidence_validator = Bytes::from(vec![0xd4; models::rust::validator::LENGTH]);
    let mut incoming = node.genesis.clone();
    incoming.block_hash = Bytes::from(vec![0xd3; models::rust::block_hash::LENGTH]);
    incoming.header.parents_hash_list = vec![node.genesis.block_hash.clone()];
    incoming.body.system_deploys = vec![ProcessedSystemDeploy::Succeeded {
        event_list: vec![],
        system_deploy: SystemDeployData::Slash {
            invalid_block_hash: first_hash.clone(),
            equivocation_block_hash: Some(second_hash.clone()),
            issuer_public_key: PublicKey::from_bytes(&incoming.sender),
            target_activation_epoch: 0,
            target_bond_generation: models::rust::bond_generation::BondGeneration::GENESIS,
        },
        pre_state_hash: Bytes::new(),
        post_state_hash: Bytes::new(),
    }];
    incoming.header.objective_equivocation_evidence_delta =
        vec![ObjectiveEquivocationEvidence::new(
            evidence_validator.clone(),
            models::rust::bond_generation::BondGeneration::GENESIS,
            incoming.seq_num,
            first_hash.clone(),
            second_hash.clone(),
        )
        .expect("objective evidence")];

    let mut first = node.genesis.clone();
    first.block_hash = first_hash.clone();
    first.header.parents_hash_list = vec![node.genesis.block_hash.clone()];
    first.header.sender_bond_generation =
        Some(models::rust::bond_generation::BondGeneration::GENESIS);
    first.sender = evidence_validator.clone();
    first.seq_num = 1;
    first.body.state.bonds = vec![Bond {
        validator: evidence_validator.clone(),
        stake: 1,
    }];
    node.block_store
        .put_block_message(&first)
        .expect("store first evidence block");
    node.block_dag_storage
        .insert(&first, InsertMode::Invalid)
        .expect("admit first evidence metadata");
    node.block_dag_storage
        .access_equivocations_tracker(|tracker| {
            tracker.add(EquivocationRecord::new(
                evidence_validator,
                models::rust::bond_generation::BondGeneration::GENESIS,
                incoming.seq_num,
                BTreeSet::from([first_hash.clone(), second_hash.clone()]),
            ))
        })
        .expect("tracker hints");

    let ready = node
        .block_processor
        .check_dependencies_with_effects(node.casper.clone(), &incoming)
        .await
        .expect("partial objective dependency check");
    assert!(!ready);
    {
        let requested = node.requested_blocks.lock().expect("requested blocks");
        assert!(!requested.contains_key(&first_hash));
        assert!(requested.contains_key(&second_hash));
    }

    let mut second = node.genesis.clone();
    second.block_hash = second_hash;
    second.header.parents_hash_list = vec![node.genesis.block_hash.clone()];
    second.header.sender_bond_generation =
        Some(models::rust::bond_generation::BondGeneration::GENESIS);
    second.sender = Bytes::from(vec![0xd5; models::rust::validator::LENGTH]);
    second.seq_num = 1;
    second.body.state.bonds = vec![Bond {
        validator: second.sender.clone(),
        stake: 1,
    }];
    node.block_store
        .put_block_message(&second)
        .expect("store second evidence block");
    node.block_dag_storage
        .insert(&second, InsertMode::Invalid)
        .expect("admit second evidence metadata");

    assert!(node
        .block_processor
        .check_dependencies_with_effects(node.casper.clone(), &incoming)
        .await
        .expect("complete objective dependency check"));
}
