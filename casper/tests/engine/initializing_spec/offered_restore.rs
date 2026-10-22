use std::time::Duration;

use block_storage::rust::deploy::key_value_deploy_storage::PendingDeployCandidate;
use casper::rust::engine::engine::Engine;
use casper::rust::protocol::{casper_message_from_proto, to_casper_message_proto};
use casper::rust::util::proto_util;
use casper::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;
use models::rust::casper::protocol::casper_message::{
    ApprovedBlockCandidate, FloorCacheResponse, MergeableEntryResponse,
};
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::state::rspace_state_manager::RSpaceStateManager;

use super::*;
use crate::helper::offered_deploy;
use crate::util::rholang::resources::{
    generate_scope_id, mk_runtime_manager_with_history_at, mk_test_rnode_store_manager_shared,
};

pub(super) async fn offered_block(fixture: &TestFixture) -> BlockMessage {
    let mut conf = fixture.casper_shard_conf.clone();
    conf.casper_version = fixture.genesis.header.version;
    let adopted = AdoptedResourcePolicy::load(&fixture.runtime_manager, &fixture.genesis, &conf)
        .await
        .expect("the producer adopts its genesis policy");
    let offer = offered_deploy::source_deploy(
        "@\"restore-offered\"!(1)".to_string(),
        1,
        None,
        None,
        None,
        None,
        None,
    )
    .expect("the funded offer builds");
    let mut block = fixture.genesis.clone();
    block.header.parents_hash_list = vec![fixture.genesis.block_hash.clone()];
    block.header.timestamp = 1;
    block.body.state.block_number = 1;
    block.body.state.pre_state_hash = fixture.genesis.body.state.post_state_hash.clone();
    block.sender = fixture.validator_pk.bytes.clone().into();
    block.seq_num = 1;
    let (post_state, deploys, system_deploys, bonds) = fixture
        .runtime_manager
        .compute_state_with_bonds_envelopes(
            &block.body.state.pre_state_hash,
            vec![PendingDeployCandidate::Envelope(offer.envelope)],
            Vec::new(),
            BlockData::from_block(&block),
            None,
            None,
            Some(&adopted),
        )
        .await
        .expect("the producer executes the offered block");
    assert_eq!(deploys.len(), 1);
    assert!(!deploys[0].is_failed());
    block.body.state.post_state_hash = post_state;
    block.body.state.bonds = bonds;
    block.body.deploys = deploys;
    block.body.system_deploys = system_deploys;
    block.block_hash = proto_util::hash_block(&block);
    block
}

async fn restore_offered_block(cached: bool, invalid_post_state: bool) {
    crate::init_logger();
    let mut joiner = TestFixture::new().await;
    let mut anchor = offered_block(&joiner).await;
    let genesis = joiner.genesis.clone();
    let expected_entry = joiner
        .runtime_manager
        .get_mergeable_entry_bytes(&anchor)
        .expect("the producer has the offered entry");
    let exporter: Arc<dyn RSpaceExporter> = Arc::new(RSpaceExporterStore::create(
        joiner.rspace_store.history.clone(),
        joiner.rspace_store.cold.clone(),
        joiner.rspace_store.roots.clone(),
    ));
    let mut stores = mk_test_rnode_store_manager_shared(generate_scope_id());
    let (runtime_manager, history) = mk_runtime_manager_with_history_at(&mut *stores).await;
    joiner.runtime_manager = Arc::new(runtime_manager);
    joiner.rspace_store = stores
        .r_space_stores()
        .await
        .expect("the joiner has its own store");
    joiner.rspace_state_manager = RSpaceStateManager::new(history.exporter(), history.importer());
    joiner.casper_shard_conf.max_parent_depth = 10;
    assert!(!joiner
        .runtime_manager
        .has_root(&Blake2b256Hash::from_bytes_prost(
            &anchor.body.state.post_state_hash,
        ))
        .expect("the fresh joiner's history is readable"));
    assert!(!joiner
        .runtime_manager
        .has_mergeable_entry(&anchor)
        .expect("the fresh joiner's mergeable store is readable"));
    assert!(expected_entry.1.is_some());
    if cached {
        joiner
            .runtime_manager
            .put_mergeable_entry_bytes(
                expected_entry.0.clone(),
                expected_entry.1.clone().expect("the produced entry exists"),
            )
            .expect("the joiner holds the transferred entry");
    }
    if invalid_post_state {
        anchor.body.state.post_state_hash = genesis.body.state.post_state_hash.clone();
        anchor.block_hash = proto_util::hash_block(&anchor);
    }
    joiner.casper_shard_conf.casper_version = 1;
    joiner.casper_shard_conf.min_phlo_price = 999;
    let engine_cell = Arc::new(EngineCell::init());
    let initializing = create_initializing_engine(
        &joiner,
        Arc::new(|| Box::pin(async { Ok(()) })),
        engine_cell.clone(),
    )
    .await
    .expect("the joining engine builds");
    engine_cell.set(initializing.clone()).await;
    let respond = async {
        let mut next_request = 0;
        loop {
            let Some((_, protocol)) = joiner.transport_layer.get_request(next_request) else {
                tokio::time::sleep(Duration::from_millis(10)).await;
                continue;
            };
            next_request += 1;
            let Some(ProtocolMessage::Packet(packet)) = protocol.message else {
                continue;
            };
            let request = casper_message_from_proto(
                to_casper_message_proto(&packet)
                    .get()
                    .expect("the request packet decodes"),
            )
            .expect("the request message decodes");
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
                    .expect("the requested producer state exports");
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
                CasperMessage::FloorCacheRequest(_) => {
                    CasperMessage::FloorCacheResponse(FloorCacheResponse {
                        entries: Vec::new(),
                        genesis_hash: genesis.block_hash.clone(),
                        genesis_block: Some(genesis.clone()),
                    })
                }
                _ => continue,
            };
            initializing
                .handle(joiner.local.clone(), response)
                .await
                .expect("the joining engine receives the bootstrap response");
        }
    };
    let restore = async {
        let attempts = if invalid_post_state { 3 } else { 1 };
        for attempt in 1..=attempts {
            let result = initializing
                .handle(
                    joiner.local.clone(),
                    CasperMessage::ApprovedBlock(ApprovedBlock {
                        candidate: ApprovedBlockCandidate {
                            block: anchor.clone(),
                            required_sigs: 0,
                        },
                        floor_seed: None,
                        sigs: Vec::new(),
                    }),
                )
                .await;
            if invalid_post_state {
                assert!(engine_cell.get().await.with_casper().is_none());
                assert!(!joiner.runtime_manager.has_mergeable_entry(&anchor).unwrap());
                if attempt == 3 {
                    let error = result.expect_err("three invalid restores exhaust the retries");
                    assert!(
                        error
                            .to_string()
                            .contains("post-state differs from commitment"),
                        "{error}"
                    );
                } else {
                    result.expect("an invalid restore permits another attempt");
                }
            } else {
                result.expect("the offered restore succeeds");
                let running = engine_cell.get().await;
                let casper = running.with_casper().expect("restore reaches Running");
                assert_eq!(
                    casper
                        .last_finalized_block()
                        .await
                        .expect("the restored LFB is available")
                        .block_hash,
                    anchor.block_hash
                );
                assert_eq!(casper.get_version(), genesis.header.version);
                let policy = casper
                    .adopted_resource_policy()
                    .expect("Running retains the adopted policy");
                assert_eq!(
                    policy.genesis().genesis_root(),
                    &genesis.body.state.post_state_hash
                );
                assert_eq!(
                    joiner
                        .runtime_manager
                        .get_mergeable_entry_bytes(&anchor)
                        .unwrap(),
                    expected_entry
                );
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(90), async {
        tokio::select! {
            _ = respond => panic!("the bootstrap responder stopped before restore"),
            _ = restore => {},
        }
    })
    .await
    .expect("offered restore completes within the test deadline");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_fresh_joiner_reconstructs_missing_offered_mergeable_data() {
    restore_offered_block(false, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_joiner_preserves_cached_offered_mergeable_data() {
    restore_offered_block(true, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn offered_restore_rejects_a_wrong_post_state_on_every_retry() {
    restore_offered_block(false, true).await;
}
