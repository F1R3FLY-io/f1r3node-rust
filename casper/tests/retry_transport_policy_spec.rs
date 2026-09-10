use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use casper::rust::engine::block_retriever::{AdmitHashReason, BlockRetriever, RequestState};
use comm::rust::errors::CommError;
use comm::rust::peer_node::{Endpoint, NodeIdentifier, PeerNode};
use comm::rust::rp::connect::{Connections, ConnectionsCell};
use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use proptest::prelude::*;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

async fn recorded<F: std::future::Future>(
    recorder: &metrics_util::debugging::DebuggingRecorder,
    future: F,
) -> F::Output {
    let mut future = Box::pin(future);
    std::future::poll_fn(|cx| metrics::with_local_recorder(recorder, || future.as_mut().poll(cx)))
        .await
}

fn retry_metrics(recorder: &metrics_util::debugging::DebuggingRecorder) -> (u64, u64) {
    use casper::rust::metrics_constants::{
        BLOCK_REQUESTS_RETRIES_METRIC, BLOCK_REQUESTS_RETRY_ACTION_METRIC,
    };
    recorder
        .snapshotter()
        .snapshot()
        .into_vec()
        .into_iter()
        .fold((0, 0), |mut counts, (key, _, _, value)| {
            if let metrics_util::debugging::DebugValue::Counter(value) = value {
                if key.key().name() == BLOCK_REQUESTS_RETRY_ACTION_METRIC {
                    counts.0 += value;
                }
                if key.key().name() == BLOCK_REQUESTS_RETRIES_METRIC {
                    counts.1 += value;
                }
            }
            counts
        })
}

#[tokio::test]
async fn action_metric_precedes_transport_and_cancellation_does_not_complete_it() {
    let (retriever, transport) = fixture().await;
    let hash: BlockHash = vec![30; 32].into();
    let mut request = state();
    request.waiting_list.push(peer(2));
    retriever
        .set_request_state_for_test(hash, request)
        .await
        .unwrap();
    transport.set_response_delay(Duration::from_secs(3_600));
    let recorder = metrics_util::debugging::DebuggingRecorder::new();
    {
        let future = recorded(&recorder, retriever.request_all(Duration::from_millis(1)));
        tokio::pin!(future);
        assert!(futures::poll!(&mut future).is_pending());
        assert_eq!(retry_metrics(&recorder), (1, 0));
    }
    assert_eq!(retry_metrics(&recorder), (1, 0));
    transport.reset();
}

fn peer(value: u8) -> PeerNode {
    PeerNode {
        id: NodeIdentifier {
            key: vec![value; 32].into(),
        },
        endpoint: Endpoint {
            host: format!("peer-{value}"),
            tcp_port: 40400 + u32::from(value),
            udp_port: 40400 + u32::from(value),
        },
    }
}

fn state() -> RequestState {
    RequestState {
        timestamp: 1,
        initial_timestamp: 1,
        peers: HashSet::new(),
        received: false,
        in_casper_buffer: false,
        waiting_list: Vec::new(),
        peer_requery_cursor: 0,
        retry_budget_quarantine_until: None,
        requested_as_dependency: true,
    }
}

async fn fixture() -> (BlockRetriever<TransportLayerStub>, Arc<TransportLayerStub>) {
    let mut manager = InMemoryStoreManager::new();
    let buffer = CasperBufferKeyValueStorage::new_from_kvm(&mut manager)
        .await
        .unwrap();
    let transport = Arc::new(TransportLayerStub::new());
    let retriever = BlockRetriever::new(
        buffer,
        transport.clone(),
        ConnectionsCell {
            peers: Arc::new(Mutex::new(Connections::from_vec(vec![peer(2)]))),
        },
        create_rp_conf_ask(peer(1), None, None),
    );
    (retriever, transport)
}

async fn returned_error_counts_action(waiting: bool, known: bool, recovery: bool) {
    let (retriever, transport) = fixture().await;
    let hash: BlockHash = vec![9; 32].into();
    let mut request = state();
    if waiting {
        request.waiting_list.push(peer(2));
    }
    if known {
        request.peers.insert(peer(2));
    }
    retriever
        .set_request_state_for_test(hash.clone(), request)
        .await
        .unwrap();
    transport.set_responses(|_, _| Err(CommError::TimeOut));
    let result = if recovery {
        retriever.recover_dependency(hash.clone()).await
    } else {
        retriever.request_all(Duration::from_millis(1)).await
    };
    retriever
        .publish_pending(hash.clone(), HashSet::new(), HashSet::new())
        .unwrap();
    let policy = retriever
        .casper_buffer()
        .pending_request_policy(&BlockHashSerde(hash))
        .unwrap()
        .unwrap();
    assert_eq!(
        policy.retry_attempts, 1,
        "a returned transport error completes one upstream retry action"
    );
    assert_eq!(policy.peer_requery_attempts, u32::from(known && !recovery));
    assert!(
        result.is_ok(),
        "block transport errors must not abort retry maintenance: {result:?}"
    );
    if waiting && !recovery {
        assert_eq!(
            transport.request_count(),
            2,
            "a failed last-peer request still triggers its fallback broadcast"
        );
    }
}

#[tokio::test]
async fn waiting_peer_error_still_broadcasts_and_counts_once() {
    returned_error_counts_action(true, false, false).await;
}

#[tokio::test]
async fn known_peer_error_counts_both_counters() {
    returned_error_counts_action(false, true, false).await;
}

#[tokio::test]
async fn broadcast_error_counts_once() { returned_error_counts_action(false, false, false).await; }

#[tokio::test]
async fn recovery_error_counts_once() { returned_error_counts_action(false, false, true).await; }

#[tokio::test]
async fn initial_transport_error_retains_tracking_without_counting_a_retry() {
    let (retriever, transport) = fixture().await;
    let hash: BlockHash = vec![9; 32].into();
    transport.set_responses(|_, _| Err(CommError::TimeOut));
    let result = retriever
        .admit_hash(
            hash.clone(),
            Some(peer(2)),
            AdmitHashReason::MissingDependencyRequested,
        )
        .await;
    assert!(
        result.is_ok(),
        "initial transport errors leave retry ownership intact"
    );
    assert!(retriever.was_requested_as_dependency(&hash).unwrap());
    retriever
        .publish_pending(hash.clone(), HashSet::new(), HashSet::new())
        .unwrap();
    assert_eq!(
        retriever
            .casper_buffer()
            .pending_request_policy(&BlockHashSerde(hash))
            .unwrap()
            .unwrap()
            .retry_attempts,
        0
    );
}

#[tokio::test]
async fn recovery_peer_append_to_nonempty_queue_counts_without_dispatch() {
    let (retriever, transport) = fixture().await;
    let hash: BlockHash = vec![9; 32].into();
    let mut request = state();
    request.waiting_list.push(peer(3));
    retriever
        .set_request_state_for_test(hash.clone(), request)
        .await
        .unwrap();
    retriever.recover_dependency(hash.clone()).await.unwrap();
    assert_eq!(
        transport.request_count(),
        0,
        "upstream does not broadcast after appending a peer to a nonempty queue"
    );
    assert_eq!(retriever.get_waiting_list_size(&hash).await.unwrap(), 2);
    retriever
        .publish_pending(hash.clone(), HashSet::new(), HashSet::new())
        .unwrap();
    assert_eq!(
        retriever
            .casper_buffer()
            .pending_request_policy(&BlockHashSerde(hash))
            .unwrap()
            .unwrap()
            .retry_attempts,
        1
    );
}

#[tokio::test]
async fn cancelled_waiting_action_does_not_count_at_either_await() {
    for cancel_at_broadcast in [false, true] {
        let (retriever, transport) = fixture().await;
        let hash: BlockHash = vec![9; 32].into();
        let mut request = state();
        request.waiting_list.push(peer(2));
        retriever
            .set_request_state_for_test(hash.clone(), request)
            .await
            .unwrap();
        if cancel_at_broadcast {
            let delayed = transport.clone();
            transport.set_responses(move |_, _| {
                delayed.set_response_delay(Duration::from_secs(3600));
                Err(CommError::TimeOut)
            });
        } else {
            transport.set_response_delay(Duration::from_secs(3600));
        }
        {
            let retry = retriever.request_all(Duration::from_millis(1));
            tokio::pin!(retry);
            assert!(futures::poll!(retry.as_mut()).is_pending());
        }
        assert_eq!(transport.request_count(), usize::from(cancel_at_broadcast));
        transport.reset();
        retriever
            .publish_pending(hash.clone(), HashSet::new(), HashSet::new())
            .unwrap();
        assert_eq!(
            retriever
                .casper_buffer()
                .pending_request_policy(&BlockHashSerde(hash))
                .unwrap()
                .unwrap()
                .retry_attempts,
            0
        );
    }
}

#[tokio::test]
async fn returned_error_after_pending_publication_updates_original_policy() {
    let (retriever, transport) = fixture().await;
    let hash: BlockHash = vec![9; 32].into();
    let mut request = state();
    request.peers.insert(peer(2));
    retriever
        .set_request_state_for_test(hash.clone(), request)
        .await
        .unwrap();
    let callback = retriever.clone();
    let callback_hash = hash.clone();
    transport.set_responses(move |_, _| {
        callback
            .publish_pending(callback_hash.clone(), HashSet::new(), HashSet::new())
            .unwrap();
        Err(CommError::TimeOut)
    });
    retriever
        .request_all(Duration::from_millis(1))
        .await
        .unwrap();
    transport.reset();
    let policy = retriever
        .casper_buffer()
        .pending_request_policy(&BlockHashSerde(hash))
        .unwrap()
        .unwrap();
    assert_eq!(
        (policy.retry_attempts, policy.peer_requery_attempts),
        (1, 1)
    );
    assert_eq!(retriever.get_requested_blocks_count().await.unwrap(), 0);
}

#[tokio::test]
async fn returned_error_does_not_charge_replacement_policy() {
    let (retriever, transport) = fixture().await;
    let hash: BlockHash = vec![9; 32].into();
    let mut request = state();
    request.peers.insert(peer(2));
    retriever
        .set_request_state_for_test(hash.clone(), request)
        .await
        .unwrap();
    let callback = retriever.clone();
    let callback_hash = hash.clone();
    transport.set_responses(move |_, _| {
        callback
            .replace_request_for_test(callback_hash.clone(), state(), 7, 3)
            .unwrap();
        Err(CommError::TimeOut)
    });
    retriever
        .request_all(Duration::from_millis(1))
        .await
        .unwrap();
    transport.reset();
    retriever
        .publish_pending(hash.clone(), HashSet::new(), HashSet::new())
        .unwrap();
    let policy = retriever
        .casper_buffer()
        .pending_request_policy(&BlockHashSerde(hash))
        .unwrap()
        .unwrap();
    assert_eq!(
        (policy.retry_attempts, policy.peer_requery_attempts),
        (7, 3)
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn last_peer_action_counts_once_for_all_transport_outcomes(first_error: bool, second_error: bool) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let (retriever, transport) = fixture().await;
            let hash: BlockHash = vec![9; 32].into();
            let mut request = state();
            request.waiting_list.push(peer(2));
            retriever.set_request_state_for_test(hash.clone(), request).await.unwrap();
            let calls = Arc::new(AtomicUsize::new(0));
            let observed = calls.clone();
            transport.set_responses(move |_, _| {
                let error = if observed.fetch_add(1, Ordering::SeqCst) == 0 { first_error } else { second_error };
                if error { Err(CommError::TimeOut) } else { Ok(()) }
            });
            retriever.request_all(Duration::from_millis(1)).await.unwrap();
            prop_assert_eq!(calls.load(Ordering::SeqCst), 2);
            retriever.publish_pending(hash.clone(), HashSet::new(), HashSet::new()).unwrap();
            let policy = retriever.casper_buffer().pending_request_policy(&BlockHashSerde(hash)).unwrap().unwrap();
            prop_assert_eq!((policy.retry_attempts, policy.peer_requery_attempts), (1, 0));
            Ok(())
        })?;
    }
}

#[tokio::test]
async fn failed_error_completion_write_retains_charge_without_network_retry() {
    let recorder = metrics_util::debugging::DebuggingRecorder::new();
    let (retriever, transport) = fixture().await;
    let hash: BlockHash = vec![9; 32].into();
    let mut request = state();
    request.peers.insert(peer(2));
    retriever
        .set_request_state_for_test(hash.clone(), request)
        .await
        .unwrap();
    let before = Arc::new(Mutex::new(None));
    let saved = before.clone();
    let callback = retriever.clone();
    let callback_hash = hash.clone();
    transport.set_responses(move |_, _| {
        callback
            .publish_pending(callback_hash.clone(), HashSet::new(), HashSet::new())
            .unwrap();
        *saved.lock().unwrap() = callback
            .casper_buffer()
            .pending_request_policy(&BlockHashSerde(callback_hash.clone()))
            .unwrap();
        callback
            .casper_buffer()
            .remove(BlockHashSerde(callback_hash.clone()))
            .unwrap();
        Err(CommError::TimeOut)
    });
    assert!(
        recorded(&recorder, retriever.request_all(Duration::from_millis(1)))
            .await
            .is_err()
    );
    assert_eq!(retry_metrics(&recorder), (1, 1));
    assert_eq!(transport.request_count(), 1);
    transport.reset();
    let previous = before.lock().unwrap().take().unwrap();
    retriever
        .casper_buffer()
        .publish_pending_request(
            BlockHashSerde(hash.clone()),
            HashSet::new(),
            HashSet::new(),
            None,
            &previous,
        )
        .unwrap();
    recorded(&recorder, retriever.request_all(Duration::from_millis(1)))
        .await
        .unwrap();
    assert_eq!(retry_metrics(&recorder), (1, 1));
    let restored = retriever
        .casper_buffer()
        .pending_request_policy(&BlockHashSerde(hash))
        .unwrap()
        .unwrap();
    assert_eq!(
        (restored.retry_attempts, restored.peer_requery_attempts),
        (1, 1)
    );
    assert_eq!(
        transport.request_count(),
        0,
        "persistence recovery must not send another network request"
    );
}

#[tokio::test]
async fn block_transport_error_does_not_skip_independent_request() {
    let (retriever, transport) = fixture().await;
    for value in [8, 9] {
        let hash: BlockHash = vec![value; 32].into();
        retriever
            .set_request_state_for_test(hash, state())
            .await
            .unwrap();
    }
    transport.set_responses(|_, _| Err(CommError::TimeOut));
    retriever
        .request_all(Duration::from_millis(1))
        .await
        .unwrap();
    assert_eq!(transport.request_count(), 2);
    for value in [8, 9] {
        let hash: BlockHash = vec![value; 32].into();
        retriever
            .publish_pending(hash.clone(), HashSet::new(), HashSet::new())
            .unwrap();
        assert_eq!(
            retriever
                .casper_buffer()
                .pending_request_policy(&BlockHashSerde(hash))
                .unwrap()
                .unwrap()
                .retry_attempts,
            1
        );
    }
}

#[tokio::test]
async fn cancellation_of_known_broadcast_and_recovery_actions_does_not_count() {
    for kind in 0..3 {
        let (retriever, transport) = fixture().await;
        let hash: BlockHash = vec![9; 32].into();
        let mut request = state();
        if kind == 0 {
            request.peers.insert(peer(2));
        }
        retriever
            .set_request_state_for_test(hash.clone(), request)
            .await
            .unwrap();
        transport.set_response_delay(Duration::from_secs(3600));
        {
            let action = async {
                if kind == 2 {
                    retriever.recover_dependency(hash.clone()).await
                } else {
                    retriever.request_all(Duration::from_millis(1)).await
                }
            };
            tokio::pin!(action);
            assert!(futures::poll!(action.as_mut()).is_pending());
        }
        transport.reset();
        retriever
            .publish_pending(hash.clone(), HashSet::new(), HashSet::new())
            .unwrap();
        let policy = retriever
            .casper_buffer()
            .pending_request_policy(&BlockHashSerde(hash))
            .unwrap()
            .unwrap();
        assert_eq!(
            (policy.retry_attempts, policy.peer_requery_attempts),
            (0, 0)
        );
    }
}
