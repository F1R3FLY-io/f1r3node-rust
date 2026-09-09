// See casper/src/test/scala/coop/rchain/casper/engine/InitializingSpec.scala

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use casper::rust::engine::engine::{transition_to_initializing, Engine};
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::engine::initializing::Initializing;
use casper::rust::engine::lfs_tuple_space_requester;
use casper::rust::errors::CasperError;
use comm::rust::errors::CommError;
use comm::rust::rp::protocol_helper::packet_with_content;
use comm::rust::test_instances::TransportLayerStub;
use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::casper::Signature;
use models::routing::protocol::Message as ProtocolMessage;
use models::rust::casper::protocol::casper_message::{
    ApprovedBlock, ApprovedBlockRequest, BlockMessage, BlockRequest, CasperMessage,
    MergeableEntryResponse, StoreItemsMessage, StoreItemsMessageRequest,
};
use prost::bytes::Bytes;
use prost::Message;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::state::exporters::rspace_exporter_items::RSpaceExporterItems;
use rspace_plus_plus::rspace::state::instances::rspace_exporter_store::RSpaceExporterStore;
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporter;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::ByteVector;
use tokio::sync::mpsc;

use crate::engine::setup::TestFixture;

struct InitializingSpec;

impl InitializingSpec {
    fn before_each(fixture: &TestFixture) {
        fixture
            .transport_layer
            .set_responses(|_peer, _protocol| Ok(()));
    }

    fn after_each(fixture: &TestFixture) { fixture.transport_layer.reset(); }
    async fn make_transition_to_running_once_approved_block_received() {
        let _event_bus = shared::rust::shared::f1r3fly_events::F1r3flyEvents::new();

        let fixture = TestFixture::new().await;

        Self::before_each(&fixture);

        let the_init = Arc::new(|| {
            Box::pin(async { Ok(()) })
                as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
        });

        let engine_cell = Arc::new(EngineCell::init());

        // interval and duration do not affect this direct approved-block transition
        let initializing_engine =
            create_initializing_engine(&fixture, the_init, engine_cell.clone())
                .await
                .expect("Failed to create Initializing engine");

        let genesis = &fixture.genesis;
        let mut approved_block_candidate = fixture.approved_block_candidate.clone();
        approved_block_candidate.required_sigs = fixture.required_sigs;
        let validator_sk = &fixture.validator_sk;
        let validator_pk = &fixture.validator_pk;

        let approved_block = {
            let candidate_proto = approved_block_candidate.clone().to_proto();
            let candidate_bytes = {
                let mut buf = Vec::new();
                Message::encode(&candidate_proto, &mut buf).expect("Failed to encode candidate");
                buf
            };
            let candidate_hash = Blake2b256::hash(candidate_bytes);
            let signature_bytes = Secp256k1.sign(&candidate_hash, &validator_sk.bytes);

            ApprovedBlock {
                candidate: approved_block_candidate,
                sigs: vec![Signature {
                    public_key: validator_pk.bytes.clone(),
                    algorithm: "secp256k1".to_string(),
                    sig: signature_bytes.into(),
                }],
                floor_seed: None,
            }
        };

        // Get exporter for genesis block
        // Note: Instead of default exported, we should use RSpaceExporterItems::get_history_and_data
        //let _genesis_exporter = &fixture.exporter;

        let chunk_size = lfs_tuple_space_requester::PAGE_SIZE;

        #[allow(clippy::type_complexity)]
        fn genesis_export(
            genesis_exporter: Arc<dyn RSpaceExporter>,
            start_path: Vec<(Blake2b256Hash, Option<u8>)>,
            exporter_params: &crate::engine::setup::ExporterParams,
        ) -> Result<
            (
                Vec<(Blake2b256Hash, ByteVector)>,
                Vec<(Blake2b256Hash, ByteVector)>,
                Vec<(Blake2b256Hash, Option<u8>)>,
            ),
            String,
        > {
            let (history_store_items, data_store_items) = RSpaceExporterItems::get_history_and_data(
                genesis_exporter,
                start_path,
                exporter_params.skip,
                exporter_params.take,
            );
            Ok((
                history_store_items.items,
                data_store_items.items,
                history_store_items.last_path,
            ))
        }

        let post_state_hash_bs = &approved_block.candidate.block.body.state.post_state_hash;
        let post_state_hash = Blake2b256Hash::from_bytes_prost(post_state_hash_bs);
        let start_path1 = vec![(post_state_hash, None::<u8>)];

        let rspace_store = &fixture.rspace_store;
        let genesis_exporter_impl = RSpaceExporterStore::create(
            rspace_store.history.clone(),
            rspace_store.cold.clone(),
            rspace_store.roots.clone(),
        );
        let genesis_exporter_arc = Arc::new(genesis_exporter_impl);

        // Build all StoreItems requests/responses dynamically until exporter indicates completion.
        // The number of chunks can change when genesis tuplespace shape changes.
        let mut store_request_messages = Vec::new();
        let mut store_response_messages = Vec::new();
        let mut next_start_path = start_path1.clone();
        let mut seen_paths = HashSet::new();

        loop {
            if !seen_paths.insert(next_start_path.clone()) {
                break;
            }

            let request = StoreItemsMessageRequest {
                start_path: next_start_path.clone(),
                skip: 0,
                take: chunk_size,
            };

            let (history_items, data_items, last_path) = genesis_export(
                genesis_exporter_arc.clone(),
                next_start_path.clone(),
                &fixture.exporter_params,
            )
            .expect("Failed to export history and data items");

            let response = StoreItemsMessage {
                start_path: next_start_path.clone(),
                last_path: last_path.clone(),
                history_items: history_items
                    .into_iter()
                    .map(|(hash, bytes)| (hash, Bytes::from(bytes)))
                    .collect(),
                data_items: data_items
                    .into_iter()
                    .map(|(hash, bytes)| (hash, Bytes::from(bytes)))
                    .collect(),
            };

            store_request_messages.push(request);
            store_response_messages.push(response);

            if last_path.is_empty() || last_path == next_start_path {
                break;
            }
            next_start_path = last_path;

            assert!(
                store_request_messages.len() < 1024,
                "Too many tuple-space chunks while preparing initializing_spec test"
            );
        }

        // Block request message
        let block_request_message = BlockRequest {
            hash: genesis.block_hash.clone(),
        };

        // Send two response messages to signal the end
        // Scala equivalent: stateResponseQueue.enqueue1(storeResponseMessage1) *>
        //                   stateResponseQueue.enqueue1(storeResponseMessage2) *>
        //                   blockResponseQueue.enqueue1(genesis)
        // IMPORTANT: Write directly to channels (NOT through handle()) like Scala test does
        let tuple_space_tx = initializing_engine
            .tuple_space_tx
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .clone();
        let block_message_tx = initializing_engine
            .block_message_tx
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .clone();
        let store_msgs_clone = store_response_messages.clone();
        let genesis_clone = genesis.clone();

        let enqueue_responses = async move {
            // Write directly to tuple space channel (equivalent to stateResponseQueue.enqueue1)
            for store_msg in store_msgs_clone {
                tuple_space_tx
                    .send(store_msg)
                    .await
                    .expect("Failed to enqueue tuple space response");
            }
            // Write directly to block message channel (equivalent to blockResponseQueue.enqueue1)
            block_message_tx
                .send(genesis_clone)
                .await
                .expect("Failed to enqueue block response");
        };

        let local_for_expected = fixture.local.clone();
        let mut expected_requests: Vec<_> = store_request_messages
            .iter()
            .map(|request| {
                packet_with_content(
                    &local_for_expected,
                    &fixture.network_id,
                    request.clone().to_proto(),
                )
            })
            .collect();
        expected_requests.push(packet_with_content(
            &local_for_expected,
            &fixture.network_id,
            block_request_message.to_proto(),
        ));
        expected_requests.push(packet_with_content(
            &local_for_expected,
            &fixture.network_id,
            models::casper::ForkChoiceTipRequestProto::default(),
        ));
        let test = async {
            engine_cell.set(initializing_engine.clone()).await;

            let enqueue_responses_with_delay = async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                enqueue_responses.await;
            };

            let approved_block_clone = approved_block.clone();
            let local_for_handle = fixture.local.clone();
            // Handle approved block (it's blocking until responses are received)
            let handle_fut = async {
                let engine = engine_cell.get().await;
                engine
                    .handle(
                        local_for_handle,
                        CasperMessage::ApprovedBlock(approved_block_clone),
                    )
                    .await
                    .expect("Failed to handle approved block");
            };
            let _ = tokio::join!(enqueue_responses_with_delay, handle_fut);

            let engine = engine_cell.get().await;

            let casper_defined = engine.with_casper().is_some();
            assert!(
                casper_defined,
                "Casper should be defined after handling approved block"
            );

            let block_option = fixture
                .block_store
                .get(&genesis.block_hash)
                .expect("Failed to get block from store");
            assert!(block_option.is_some(), "Block should be defined in store");
            assert_eq!(block_option.as_ref(), Some(genesis));

            let handler_internal = engine_cell.get().await;

            // We use with_casper().is_some() as a proxy: Running engines have casper, Initializing engines return None.
            // This is functionally equivalent since after transition_to_running(), only Running engines should be in the cell.
            assert!(
                handler_internal.with_casper().is_some(),
                "Engine should be Running (checked via casper presence)"
            );

            let requests = fixture.transport_layer.get_all_requests();
            // Assert requested messages for the state and fork choice tip
            assert_eq!(
                requests.len(),
                expected_requests.len(),
                "Transport layer should have received expected number of requests"
            );

            // Note: Since Protocol doesn't implement Hash/Eq, we compare packet contents like in original Scala code
            // which compares `_.msg.message.packet.get.content`, not the entire Protocol objects
            let request_packet_contents: HashSet<_> = requests
                .iter()
                .filter_map(|r| match &r.msg.message {
                    Some(ProtocolMessage::Packet(packet)) => Some(&packet.content),
                    _ => None,
                })
                .collect();
            let expected_packet_contents: HashSet<_> = expected_requests
                .iter()
                .filter_map(|protocol| match &protocol.message {
                    Some(ProtocolMessage::Packet(packet)) => Some(&packet.content),
                    _ => None,
                })
                .collect();
            assert_eq!(
                request_packet_contents, expected_packet_contents,
                "Request packet contents should match expected packet contents (order doesn't matter)"
            );

            fixture.transport_layer.reset();

            let last_approved_block_o = fixture.last_approved_block.lock().unwrap().clone();
            assert!(last_approved_block_o.is_some());

            {
                let engine = engine_cell.get().await;
                engine
                    .handle(
                        fixture.local.clone(),
                        CasperMessage::ApprovedBlockRequest(ApprovedBlockRequest {
                            identifier: "test".to_string(),
                            trim_state: false,
                        }),
                    )
                    .await
                    .expect("Failed to handle approved block request");
            };

            let requests_after = fixture.transport_layer.get_all_requests();
            let approved_block_bytes =
                prost::bytes::Bytes::from(approved_block.clone().to_proto().encode_to_vec());
            let found_approved_block = requests_after.iter().any(|r| match &r.msg.message {
                Some(ProtocolMessage::Packet(packet)) => packet.content == approved_block_bytes,
                _ => false,
            });
            assert!(
                found_approved_block,
                "Expected to find approved block in transport layer requests"
            );
        };

        test.await;

        Self::after_each(&fixture);
    }
}

// Creates an Initializing engine using TestFixture's shared stores and managers.
// This matches Scala's approach where InitializingSpec extends Setup and uses
// implicit vals from the Setup trait (blockStore, blockDagStorage, runtimeManager, etc.).
//
// CRITICAL: Using fixture's stores ensures genesis data exported from fixture.rspace_store
// is imported into the SAME rspace_store instance, preventing storage isolation bugs.
#[allow(clippy::type_complexity)]
async fn create_initializing_engine(
    fixture: &TestFixture,
    the_init: Arc<
        dyn Fn() -> Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>> + Send + Sync,
    >,
    engine_cell: Arc<EngineCell>,
) -> Result<Arc<Initializing<TransportLayerStub>>, String> {
    // Create engine-specific channels (each Initializing instance needs its own)
    let (block_tx, block_rx) = mpsc::channel::<BlockMessage>(50);
    let (tuple_tx, tuple_rx) = mpsc::channel::<StoreItemsMessage>(50);
    let (block_processing_queue_tx, _block_processing_queue_rx) =
        casper::rust::blocks::block_processing_queue::BlockProcessingQueueSender::channel(
            1024,
            64 * 1024 * 1024,
        )
        .expect("block processing queue");

    // Use all stores and managers from fixture (matching Scala's Setup pattern)
    Ok(Arc::new(Initializing::new(
        fixture.transport_layer.as_ref().clone(),
        fixture.rp_conf_ask.clone(),
        fixture.connections_cell.clone(),
        fixture.last_approved_block.clone(),
        fixture.block_store.clone(),
        fixture.block_dag_storage.clone(),
        fixture.deploy_storage.clone(),
        fixture.rejected_deploy_buffer.clone(),
        fixture.casper_buffer_storage.clone(),
        fixture.rspace_state_manager.clone(),
        block_processing_queue_tx,
        fixture.blocks_in_processing.clone(),
        fixture.casper_shard_conf.clone(),
        fixture.required_sigs,
        Some(fixture.validator_id.clone()),
        the_init,
        block_tx,
        block_rx,
        tuple_tx,
        tuple_rx,
        true,
        false,
        fixture.event_publisher.clone(),
        fixture.block_retriever.clone(),
        engine_cell.clone(),
        fixture.runtime_manager.clone(),
        fixture.estimator.clone(),
        casper::rust::heartbeat_signal::new_heartbeat_signal_ref(),
        None,
    )))
}

fn signed_approved_block(fixture: &TestFixture) -> ApprovedBlock {
    let mut candidate = fixture.approved_block_candidate.clone();
    candidate.required_sigs = fixture.required_sigs;
    let candidate_hash = Blake2b256::hash(candidate.clone().to_proto().encode_to_vec());
    let signature_bytes = Secp256k1.sign(&candidate_hash, &fixture.validator_sk.bytes);
    ApprovedBlock {
        candidate,
        sigs: vec![Signature {
            public_key: fixture.validator_pk.bytes.clone(),
            algorithm: "secp256k1".to_string(),
            sig: signature_bytes.into(),
        }],
        floor_seed: None,
    }
}

fn exported_store_responses(
    fixture: &TestFixture,
    approved_block: &ApprovedBlock,
) -> Vec<StoreItemsMessage> {
    let post_state_hash = Blake2b256Hash::from_bytes_prost(
        &approved_block.candidate.block.body.state.post_state_hash,
    );
    let exporter = Arc::new(RSpaceExporterStore::create(
        fixture.rspace_store.history.clone(),
        fixture.rspace_store.cold.clone(),
        fixture.rspace_store.roots.clone(),
    ));
    let mut responses = Vec::new();
    let mut start_path = vec![(post_state_hash, None::<u8>)];
    let mut seen_paths = HashSet::new();

    loop {
        assert!(seen_paths.insert(start_path.clone()));
        let (history_items, data_items) = RSpaceExporterItems::get_history_and_data(
            exporter.clone(),
            start_path.clone(),
            fixture.exporter_params.skip,
            fixture.exporter_params.take,
        );
        let last_path = history_items.last_path.clone();
        responses.push(StoreItemsMessage {
            start_path: start_path.clone(),
            last_path: last_path.clone(),
            history_items: history_items
                .items
                .into_iter()
                .map(|(hash, bytes)| (hash, Bytes::from(bytes)))
                .collect(),
            data_items: data_items
                .items
                .into_iter()
                .map(|(hash, bytes)| (hash, Bytes::from(bytes)))
                .collect(),
        });
        if last_path.is_empty() || last_path == start_path {
            break;
        }
        start_path = last_path;
        assert!(responses.len() < 1024);
    }
    responses
}

fn corrupt_store_response(approved_block: &ApprovedBlock) -> StoreItemsMessage {
    let post_state_hash = Blake2b256Hash::from_bytes_prost(
        &approved_block.candidate.block.body.state.post_state_hash,
    );
    StoreItemsMessage {
        start_path: vec![(post_state_hash, None)],
        last_path: Vec::new(),
        history_items: vec![(
            Blake2b256Hash::from_bytes(vec![7; 32]),
            Bytes::from_static(b"corrupt-state-chunk"),
        )],
        data_items: Vec::new(),
    }
}

async fn enqueue_restore_responses(
    engine: Arc<Initializing<TransportLayerStub>>,
    transport_layer: Arc<TransportLayerStub>,
    send_genesis: bool,
    store_responses: Vec<StoreItemsMessage>,
    genesis: BlockMessage,
) {
    let tuple_tx = engine
        .tuple_space_tx
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .clone();
    let block_tx = engine
        .block_message_tx
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .clone();
    for response in store_responses {
        tuple_tx.send(response).await.unwrap();
    }
    if !send_genesis {
        return;
    }
    let expected_request = BlockRequest {
        hash: genesis.block_hash.clone(),
    }
    .to_proto()
    .encode_to_vec();
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let request_count = transport_layer
                .get_all_requests()
                .iter()
                .filter(|request| {
                    matches!(
                        &request.msg.message,
                        Some(ProtocolMessage::Packet(packet))
                            if packet.content.as_ref() == expected_request
                    )
                })
                .count();
            if request_count >= 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("the block requester must request the genesis block");
    block_tx.send(genesis).await.unwrap();
}

#[tokio::test]
async fn make_transition_to_running_once_approved_block_received() {
    InitializingSpec::make_transition_to_running_once_approved_block_received().await;
}

#[tokio::test]
async fn corrupt_first_restore_retries_and_correct_second_restore_runs() {
    let fixture = TestFixture::new().await;
    InitializingSpec::before_each(&fixture);
    let engine_cell = Arc::new(EngineCell::init());
    let the_init = Arc::new(|| {
        Box::pin(async { Ok(()) }) as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
    });
    let engine = create_initializing_engine(&fixture, the_init, engine_cell.clone())
        .await
        .unwrap();
    engine_cell.set(engine.clone()).await;
    let approved_block = signed_approved_block(&fixture);

    fixture.transport_layer.reset();
    fixture
        .transport_layer
        .set_responses(|_peer, _protocol| Ok(()));
    let enqueue = enqueue_restore_responses(
        engine.clone(),
        fixture.transport_layer.clone(),
        true,
        vec![corrupt_store_response(&approved_block)],
        fixture.genesis.clone(),
    );
    let restore = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        engine.handle(
            fixture.local.clone(),
            CasperMessage::ApprovedBlock(approved_block.clone()),
        ),
    );
    let (_, restore_result) = tokio::join!(enqueue, restore);
    restore_result
        .expect("the first restoration attempt must terminate")
        .expect("the first restoration failure must schedule a retry");
    assert!(engine_cell.get().await.with_casper().is_none());

    let retry_request = models::casper::ApprovedBlockRequestProto {
        identifier: String::new(),
        trim_state: true,
    }
    .encode_to_vec();
    assert!(fixture
        .transport_layer
        .get_all_requests()
        .iter()
        .any(|request| {
            matches!(
                &request.msg.message,
                Some(ProtocolMessage::Packet(packet)) if packet.content.as_ref() == retry_request
            )
        }));

    let correct_responses = exported_store_responses(&fixture, &approved_block);
    let enqueue = enqueue_restore_responses(
        engine.clone(),
        fixture.transport_layer.clone(),
        false,
        correct_responses,
        fixture.genesis.clone(),
    );
    let restore = engine.handle(
        fixture.local.clone(),
        CasperMessage::ApprovedBlock(approved_block),
    );
    let (_, restore_result) = tokio::time::timeout(std::time::Duration::from_secs(120), async {
        tokio::join!(enqueue, restore)
    })
    .await
    .expect("the second restoration attempt must terminate");
    restore_result.expect("the second restoration attempt must reach Running");
    assert!(engine_cell.get().await.with_casper().is_some());
    InitializingSpec::after_each(&fixture);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stale_retry_request_failure_cannot_terminate_newer_restore() {
    let fixture = TestFixture::new().await;
    InitializingSpec::before_each(&fixture);
    let engine_cell = Arc::new(EngineCell::init());
    let startup_failure = engine_cell.subscribe_startup_failure();
    let the_init = Arc::new(|| {
        Box::pin(async { Ok(()) }) as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
    });
    let engine = create_initializing_engine(&fixture, the_init, engine_cell.clone())
        .await
        .unwrap();
    engine_cell.set(engine.clone()).await;
    let approved_block = signed_approved_block(&fixture);
    let retry_request = models::casper::ApprovedBlockRequestProto {
        identifier: String::new(),
        trim_state: true,
    }
    .encode_to_vec();
    let retry_entered = Arc::new(AtomicBool::new(false));
    let retry_gate = Arc::new((Mutex::new(false), Condvar::new()));
    let release_retry = || {
        let (released, ready) = &*retry_gate;
        *released.lock().unwrap() = true;
        ready.notify_all();
    };

    fixture.transport_layer.reset();
    fixture.transport_layer.set_responses({
        let retry_entered = retry_entered.clone();
        let retry_gate = retry_gate.clone();
        move |_peer, protocol| match &protocol.message {
            Some(ProtocolMessage::Packet(packet))
                if packet.content.as_ref() == retry_request
                    && !retry_entered.swap(true, Ordering::SeqCst) =>
            {
                let (released, ready) = &*retry_gate;
                let mut released = released.lock().unwrap();
                while !*released {
                    released = ready.wait(released).unwrap();
                }
                Err(CommError::TimeOut)
            }
            _ => Ok(()),
        }
    });

    let first_enqueue = tokio::spawn(enqueue_restore_responses(
        engine.clone(),
        fixture.transport_layer.clone(),
        true,
        vec![corrupt_store_response(&approved_block)],
        fixture.genesis.clone(),
    ));
    let first_restore = tokio::spawn({
        let engine = engine.clone();
        let local = fixture.local.clone();
        let approved_block = approved_block.clone();
        async move {
            engine
                .handle(local, CasperMessage::ApprovedBlock(approved_block))
                .await
        }
    });

    let first_request_started = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while !retry_entered.load(Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await;
    if first_request_started.is_err() {
        release_retry();
    }
    first_request_started.expect("the first generation must begin its replacement request");
    let first_enqueue_result = first_enqueue.await;
    if first_enqueue_result.is_err() {
        release_retry();
    }
    first_enqueue_result.unwrap();
    let request_count_before_second_restore = fixture.transport_layer.request_count();

    let second_restore = tokio::spawn({
        let engine = engine.clone();
        let local = fixture.local.clone();
        let approved_block = approved_block.clone();
        async move {
            engine
                .handle(local, CasperMessage::ApprovedBlock(approved_block))
                .await
        }
    });
    let second_restore_started = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while fixture.transport_layer.request_count() <= request_count_before_second_restore {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await;
    release_retry();
    second_restore_started.expect("the second generation must start state restoration");
    first_restore
        .await
        .unwrap()
        .expect("the stale request failure must not terminate the newer restoration");

    enqueue_restore_responses(
        engine.clone(),
        fixture.transport_layer.clone(),
        false,
        exported_store_responses(&fixture, &approved_block),
        fixture.genesis.clone(),
    )
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(120), second_restore)
        .await
        .expect("the second restoration attempt must terminate")
        .unwrap()
        .expect("the second restoration attempt must reach Running");

    assert!(startup_failure.borrow().is_none());
    assert!(engine_cell.get().await.with_casper().is_some());
    InitializingSpec::after_each(&fixture);
}

#[tokio::test]
async fn restore_failure_budget_reports_terminal_startup_failure() {
    let fixture = TestFixture::new().await;
    InitializingSpec::before_each(&fixture);
    let engine_cell = Arc::new(EngineCell::init());
    let startup_failure = engine_cell.subscribe_startup_failure();
    let the_init = Arc::new(|| {
        Box::pin(async { Ok(()) }) as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
    });
    let engine = create_initializing_engine(&fixture, the_init, engine_cell)
        .await
        .unwrap();
    let approved_block = signed_approved_block(&fixture);

    for attempt in 1..=3 {
        let enqueue = enqueue_restore_responses(
            engine.clone(),
            fixture.transport_layer.clone(),
            attempt == 1,
            vec![corrupt_store_response(&approved_block)],
            fixture.genesis.clone(),
        );
        let restore = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            engine.handle(
                fixture.local.clone(),
                CasperMessage::ApprovedBlock(approved_block.clone()),
            ),
        );
        let (_, result) = tokio::join!(enqueue, restore);
        let result =
            result.unwrap_or_else(|_| panic!("restoration attempt {attempt} did not terminate"));
        assert_eq!(result.is_err(), attempt == 3);
    }

    assert!(startup_failure.borrow().is_some());
    let duplicate = engine
        .handle(
            fixture.local.clone(),
            CasperMessage::ApprovedBlock(approved_block),
        )
        .await;
    assert!(duplicate.is_ok());
    InitializingSpec::after_each(&fixture);
}

#[tokio::test]
async fn fork_choice_notice_failure_after_running_does_not_reopen_restore() {
    let fixture = TestFixture::new().await;
    InitializingSpec::before_each(&fixture);
    let engine_cell = Arc::new(EngineCell::init());
    let the_init = Arc::new(|| {
        Box::pin(async { Ok(()) }) as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
    });
    let engine = create_initializing_engine(&fixture, the_init, engine_cell.clone())
        .await
        .unwrap();
    engine_cell.set(engine.clone()).await;
    let approved_block = signed_approved_block(&fixture);
    let fork_choice_request = models::casper::ForkChoiceTipRequestProto::default().encode_to_vec();
    fixture.transport_layer.reset();
    fixture
        .transport_layer
        .set_responses(move |_peer, protocol| match &protocol.message {
            Some(ProtocolMessage::Packet(packet))
                if packet.content.as_ref() == fork_choice_request =>
            {
                Err(CommError::TimeOut)
            }
            _ => Ok(()),
        });

    let enqueue = enqueue_restore_responses(
        engine.clone(),
        fixture.transport_layer.clone(),
        true,
        exported_store_responses(&fixture, &approved_block),
        fixture.genesis.clone(),
    );
    let restore = engine.handle(
        fixture.local.clone(),
        CasperMessage::ApprovedBlock(approved_block.clone()),
    );
    let (_, restore_result) = tokio::join!(enqueue, restore);
    restore_result.expect("post-commit notification failure must not fail restoration");
    assert!(engine_cell.get().await.with_casper().is_some());

    engine
        .handle(
            fixture.local.clone(),
            CasperMessage::ApprovedBlock(approved_block),
        )
        .await
        .expect("a duplicate cannot reopen a committed restoration");
    assert!(engine_cell.get().await.with_casper().is_some());
    InitializingSpec::after_each(&fixture);
}

/// Test that verifies the fix for the race condition where a slow validator
/// misses the ApprovedBlock during genesis ceremony. The fix is to proactively
/// request the ApprovedBlock when entering Initializing state, rather than
/// waiting for it to arrive (which may never happen if it was already broadcast
/// and dropped while the node was still in GenesisValidator state).
#[tokio::test]
async fn proactively_request_approved_block_on_init() {
    use models::casper::ApprovedBlockRequestProto;
    use models::routing::protocol::Message as ProtocolMessage;
    use prost::Message;

    let fixture = TestFixture::new().await;

    InitializingSpec::before_each(&fixture);

    let the_init = Arc::new(|| {
        Box::pin(async { Ok(()) }) as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
    });

    let engine_cell = Arc::new(EngineCell::init());

    let initializing_engine = create_initializing_engine(&fixture, the_init, engine_cell.clone())
        .await
        .expect("Failed to create Initializing engine");

    // Clear any previous transport requests
    fixture.transport_layer.reset();
    fixture
        .transport_layer
        .set_responses(|_peer, _protocol| Ok(()));

    // Call init - this should proactively request ApprovedBlock
    initializing_engine
        .init()
        .await
        .expect("init should succeed");

    // Verify that an ApprovedBlockRequest was sent to bootstrap
    let requests = fixture.transport_layer.get_all_requests();

    // Build expected content for comparison
    let expected_proto = ApprovedBlockRequestProto {
        identifier: "".to_string(),
        trim_state: true,
    };
    let expected_content = prost::bytes::Bytes::from(expected_proto.encode_to_vec());

    assert!(
        !requests.is_empty(),
        "Initializing.init should send a request to bootstrap"
    );

    let found_approved_block_request = requests.iter().any(|req| {
        if let Some(ProtocolMessage::Packet(packet)) = &req.msg.message {
            packet.content == expected_content
        } else {
            false
        }
    });

    assert!(
        found_approved_block_request,
        "Initializing.init should send ApprovedBlockRequest. Requests sent: {:?}",
        requests.iter().map(|r| &r.msg).collect::<Vec<_>>()
    );

    InitializingSpec::after_each(&fixture);
}

#[tokio::test]
async fn initialization_ignores_unauthenticated_mergeable_evidence() {
    let fixture = TestFixture::new().await;
    let the_init = Arc::new(|| {
        Box::pin(async { Ok(()) }) as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
    });
    let engine_cell = Arc::new(EngineCell::init());
    let initializing_engine = create_initializing_engine(&fixture, the_init, engine_cell)
        .await
        .expect("Failed to create Initializing engine");
    let mut before = fixture
        .runtime_manager
        .mergeable_store
        .collect(|(key, value)| Some((key.clone(), bincode::serialize(value).unwrap())))
        .expect("mergeable store snapshot");
    before.sort();

    initializing_engine
        .handle(
            fixture.local.clone(),
            CasperMessage::MergeableEntryResponse(MergeableEntryResponse {
                block_hash: vec![9; 32].into(),
                serialized_entry: vec![7; 256].into(),
            }),
        )
        .await
        .expect("unauthenticated response must be handled fail-closed");

    let mut after = fixture
        .runtime_manager
        .mergeable_store
        .collect(|(key, value)| Some((key.clone(), bincode::serialize(value).unwrap())))
        .expect("mergeable store snapshot");
    after.sort();
    assert_eq!(before, after);
}

#[test]
fn transition_to_initializing_invokes_init_immediately() {
    use models::casper::ApprovedBlockRequestProto;
    use models::routing::protocol::Message as ProtocolMessage;
    use prost::Message;

    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let fixture = TestFixture::new().await;

                InitializingSpec::before_each(&fixture);

                let init_called = Arc::new(AtomicBool::new(false));
                let init_called_ref = init_called.clone();
                let the_init = Arc::new(move || {
                    let init_called_ref = init_called_ref.clone();
                    Box::pin(async move {
                        init_called_ref.store(true, Ordering::SeqCst);
                        Ok(())
                    }) as Pin<Box<dyn Future<Output = Result<(), CasperError>> + Send>>
                });

                let engine_cell = Arc::new(EngineCell::init());
                let heartbeat_signal_ref = casper::rust::heartbeat_signal::new_heartbeat_signal_ref();

                fixture.transport_layer.reset();
                fixture
                    .transport_layer
                    .set_responses(|_peer, _protocol| Ok(()));

                transition_to_initializing(
                    &fixture.block_processing_queue_tx,
                    &fixture.blocks_in_processing,
                    &fixture.casper_shard_conf,
                    fixture.required_sigs,
                    &Some(fixture.validator_id.clone()),
                    the_init,
                    true,
                    false,
                    &fixture.transport_layer,
                    &fixture.rp_conf_ask,
                    &fixture.connections_cell,
                    &fixture.last_approved_block,
                    &fixture.block_store,
                    &fixture.block_dag_storage,
                    &fixture.deploy_storage,
                    &fixture.rejected_deploy_buffer,
                    &fixture.casper_buffer_storage,
                    &fixture.rspace_state_manager,
                    fixture.event_publisher.clone(),
                    fixture.block_retriever.clone(),
                    &engine_cell,
                    &fixture.runtime_manager,
                    &fixture.estimator,
                    &heartbeat_signal_ref,
                    None,
                )
                .await
                .expect("transition_to_initializing should succeed");

                assert!(
                    init_called.load(Ordering::SeqCst),
                    "transition_to_initializing should call init() immediately"
                );

                let requests = fixture.transport_layer.get_all_requests();
                let expected_proto = ApprovedBlockRequestProto {
                    identifier: "".to_string(),
                    trim_state: true,
                };
                let expected_content = prost::bytes::Bytes::from(expected_proto.encode_to_vec());

                let found_approved_block_request = requests.iter().any(|req| {
                    if let Some(ProtocolMessage::Packet(packet)) = &req.msg.message {
                        packet.content == expected_content
                    } else {
                        false
                    }
                });

                assert!(
                    found_approved_block_request,
                    "transition_to_initializing should trigger ApprovedBlockRequest via immediate init; requests: {:?}",
                    requests.iter().map(|r| &r.msg).collect::<Vec<_>>()
                );

                InitializingSpec::after_each(&fixture);
            })
        })
        .unwrap()
        .join()
        .unwrap();
}
