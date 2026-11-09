use std::time::Duration;

use block_storage::rust::dag::block_dag_key_value_storage::{BlockDagKeyValueStorage, InsertMode};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use casper::rust::engine::engine::Engine;
use casper::rust::protocol::{casper_message_from_proto, to_casper_message_proto};
use casper::rust::util::proto_util;
use models::rust::casper::protocol::casper_message::{
    ApprovedBlockCandidate, FloorCacheEntry, FloorCacheResponse, MergeableEntryResponse,
};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::state::rspace_state_manager::RSpaceStateManager;

use super::*;
use crate::util::rholang::resources::{
    generate_scope_id, mk_runtime_manager_with_history_at, mk_test_rnode_store_manager_shared,
};

async fn restore_child(max_parent_depth: i32, preinstall_genesis: bool) {
    crate::init_logger();
    let mut fixture = TestFixture::new().await;
    let genesis = fixture.genesis.clone();
    let genesis_entry = fixture
        .runtime_manager
        .get_mergeable_entry_bytes(&genesis)
        .unwrap();
    let mut anchor = super::offered_restore::offered_block(&fixture).await;
    anchor.justifications.clear();
    anchor.block_hash = proto_util::hash_block(&anchor);
    let exporter: Arc<dyn RSpaceExporter> = Arc::new(RSpaceExporterStore::create(
        fixture.rspace_store.history.clone(),
        fixture.rspace_store.cold.clone(),
        fixture.rspace_store.roots.clone(),
    ));
    let mut stores = mk_test_rnode_store_manager_shared(generate_scope_id());
    let (runtime, history) = mk_runtime_manager_with_history_at(&mut *stores).await;
    fixture.runtime_manager = Arc::new(runtime);
    fixture
        .runtime_manager
        .put_mergeable_entry_bytes(
            genesis_entry.0,
            genesis_entry
                .1
                .expect("the producer has the genesis mergeable entry"),
        )
        .unwrap();
    fixture.rspace_store = stores.r_space_stores().await.unwrap();
    fixture.rspace_state_manager = RSpaceStateManager::new(history.exporter(), history.importer());
    let mut dag_stores = InMemoryStoreManager::new();
    fixture.block_dag_storage = BlockDagKeyValueStorage::new(&mut dag_stores).await.unwrap();
    fixture.block_store = KeyValueBlockStore::create_from_kvm(&mut dag_stores)
        .await
        .unwrap();
    fixture.casper_shard_conf.max_parent_depth = max_parent_depth;
    assert!(!fixture
        .runtime_manager
        .has_root(&Blake2b256Hash::from_bytes_prost(
            &genesis.body.state.post_state_hash,
        ))
        .unwrap());
    assert!(fixture.block_dag_storage.genesis_hash().unwrap().is_none());
    if preinstall_genesis {
        fixture.block_store.put_block_message(&genesis).unwrap();
        fixture
            .block_dag_storage
            .insert(&genesis, InsertMode::Approved)
            .unwrap();
    }
    let engine_cell = Arc::new(EngineCell::init());
    let initializing = create_initializing_engine(
        &fixture,
        Arc::new(|| Box::pin(async { Ok(()) })),
        engine_cell.clone(),
    )
    .await
    .unwrap();
    engine_cell.set(initializing.clone()).await;
    let respond = async {
        let mut next_request = 0;
        loop {
            let Some((_, protocol)) = fixture.transport_layer.get_request(next_request) else {
                tokio::time::sleep(Duration::from_millis(10)).await;
                continue;
            };
            next_request += 1;
            let Some(ProtocolMessage::Packet(packet)) = protocol.message else {
                continue;
            };
            let request =
                casper_message_from_proto(to_casper_message_proto(&packet).get().unwrap()).unwrap();
            let response = match request {
                CasperMessage::BlockRequest(request) => {
                    let block = if request.hash == anchor.block_hash {
                        anchor.clone()
                    } else {
                        assert_eq!(request.hash, genesis.block_hash);
                        genesis.clone()
                    };
                    CasperMessage::BlockMessage(block)
                }
                CasperMessage::StoreItemsMessageRequest(request) => {
                    let (history, data) = RSpaceExporterItems::get_history_and_data(
                        exporter.clone(),
                        request.start_path.clone(),
                        request.skip,
                        request.take,
                    )
                    .unwrap();
                    CasperMessage::StoreItemsMessage(StoreItemsMessage {
                        start_path: request.start_path,
                        last_path: history.last_path,
                        history_items: history
                            .items
                            .into_iter()
                            .map(|(k, v)| (k, v.into()))
                            .collect(),
                        data_items: data.items.into_iter().map(|(k, v)| (k, v.into())).collect(),
                    })
                }
                CasperMessage::MergeableEntryRequest(request) => {
                    CasperMessage::MergeableEntryResponse(MergeableEntryResponse {
                        block_hash: request.block_hash,
                        serialized_entry: Vec::new().into(),
                    })
                }
                CasperMessage::FloorCacheRequest(request) => {
                    assert!(request.hashes.contains(&anchor.block_hash));
                    CasperMessage::FloorCacheResponse(FloorCacheResponse {
                        entries: vec![FloorCacheEntry {
                            block_hash: anchor.block_hash.clone(),
                            floor_hash: genesis.block_hash.clone(),
                            frontier_hash: genesis.block_hash.clone(),
                        }],
                        genesis_hash: genesis.block_hash.clone(),
                        genesis_block: Some(genesis.clone()),
                    })
                }
                _ => continue,
            };
            initializing
                .handle(fixture.local.clone(), response)
                .await
                .unwrap();
        }
    };
    let restore = async {
        initializing
            .handle(
                fixture.local.clone(),
                CasperMessage::ApprovedBlock(ApprovedBlock {
                    candidate: ApprovedBlockCandidate {
                        block: anchor.clone(),
                        required_sigs: 0,
                    },
                    floor_seed: None,
                    sigs: Vec::new(),
                }),
            )
            .await
            .unwrap();
        let running = engine_cell.get().await;
        let casper = running.with_casper().expect("the joiner reaches Running");
        assert_eq!(
            casper.last_finalized_block().await.unwrap().block_hash,
            anchor.block_hash
        );
        assert!(fixture
            .runtime_manager
            .has_root(&Blake2b256Hash::from_bytes_prost(
                &genesis.body.state.post_state_hash,
            ))
            .unwrap());
        assert!(fixture
            .runtime_manager
            .has_mergeable_entry(&anchor)
            .unwrap());
        let dag = fixture.block_dag_storage.get_representation().unwrap();
        assert!(dag.contains(&genesis.block_hash));
        assert_eq!(
            dag.get_cached_floor(&anchor.block_hash).unwrap(),
            Some(genesis.block_hash.clone())
        );
        assert_eq!(
            dag.get_cached_frontier(&anchor.block_hash).unwrap(),
            Some(genesis.block_hash.clone())
        );
        for bond in &anchor.body.state.bonds {
            if bond.validator != anchor.sender {
                assert_eq!(
                    dag.latest_message_hash(&bond.validator),
                    Some(genesis.block_hash.clone())
                );
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(90), async {
        tokio::select! {
            _ = respond => panic!("the bootstrap responder stopped"),
            _ = restore => {},
        }
    })
    .await
    .expect("restore completes within the test deadline");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn restore_receives_genesis_before_inserting_bonded_blocks() {
    restore_child(10, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unlimited_depth_restore_imports_history_before_replay() {
    restore_child(i32::MAX, true).await;
}
