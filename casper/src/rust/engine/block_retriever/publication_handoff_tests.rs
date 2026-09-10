use std::future::Future;
use std::sync::Mutex;

use comm::rust::peer_node::{Endpoint, NodeIdentifier};
use comm::rust::rp::connect::Connections;
use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
use proptest::prelude::*;
use prost::bytes::Bytes;

use super::*;

fn retriever() -> BlockRetriever<TransportLayerStub> {
    let local = PeerNode {
        id: NodeIdentifier {
            key: Bytes::from(vec![1; 32]),
        },
        endpoint: Endpoint {
            host: "localhost".into(),
            tcp_port: 40400,
            udp_port: 40400,
        },
    };
    BlockRetriever::new(
        test_buffer(),
        Arc::new(TransportLayerStub::new()),
        ConnectionsCell {
            peers: Arc::new(Mutex::new(Connections::from_vec(Vec::new()))),
        },
        create_rp_conf_ask(local, None, None),
    )
}

fn state(dependency: bool, original: u64, quarantine: Option<u64>) -> RequestState {
    RequestState {
        timestamp: original,
        initial_timestamp: original,
        peers: HashSet::new(),
        received: false,
        in_casper_buffer: false,
        waiting_list: Vec::new(),
        peer_requery_cursor: 0,
        retry_budget_quarantine_until: quarantine,
        requested_as_dependency: dependency,
    }
}

#[tokio::test]
async fn pending_recitation_starts_a_new_request_clock() {
    let retriever = retriever();
    let hash = Bytes::from(vec![41; 32]);
    retriever
        .replace_request_for_test(hash.clone(), state(true, 1, None), 3, 0)
        .expect("seed");
    retriever
        .publish_pending(hash.clone(), HashSet::new(), HashSet::new())
        .expect("pending");
    let before = BlockRetriever::<TransportLayerStub>::current_millis();
    let result = retriever
        .admit_hash(
            hash.clone(),
            Some(retriever.conf.local.clone()),
            AdmitHashReason::HashBroadcastReceived,
        )
        .await
        .expect("recite");
    assert_eq!(result.status, AdmitHashStatus::NewRequestAdded);
    let actual = retriever
        .request_state(&hash)
        .expect("state")
        .expect("owner");
    assert_eq!(actual.initial_timestamp, 1);
    assert!(
        actual.timestamp >= before,
        "a fresh pending recitation must not reuse its previous schedule clock"
    );
    assert_eq!(retriever.retry_attempt_count(&hash).expect("budget"), 3);
}

#[tokio::test]
async fn observed_retry_uses_current_budget_after_same_owner_recitation() {
    for attempts in [
        0,
        BlockRetriever::<TransportLayerStub>::MAX_RETRIES_PER_HASH,
    ] {
        let retriever = retriever();
        let hash = Bytes::from(vec![43; 32]);
        retriever
            .replace_request_for_test(hash.clone(), state(true, 1, None), attempts, 0)
            .expect("seed");
        let captured = retriever.lookup(&hash).unwrap().unwrap();
        let observed_at = BlockRetriever::<TransportLayerStub>::current_millis();
        assert!(captured.inspect(|policy, data| {
            !data.received
                && BlockRetriever::<TransportLayerStub>::retry_due(
                    observed_at,
                    policy.last_request_timestamp,
                    policy.retry_attempts,
                )
        }));
        retriever
            .publish_pending(hash.clone(), HashSet::new(), HashSet::new())
            .expect("publish between observation and selection");
        retriever
            .admit_hash(
                hash.clone(),
                Some(retriever.conf.local.clone()),
                AdmitHashReason::HashBroadcastReceived,
            )
            .await
            .expect("recite");
        let current = retriever.lookup(&hash).unwrap().unwrap();
        assert!(Arc::ptr_eq(&captured, &current));
        assert!(current.policy().last_request_timestamp >= observed_at);
        let dispatched = retriever.try_rerequest(&captured).await.expect("apply");
        assert_eq!(dispatched, attempts == 0);
        assert_eq!(
            retriever
                .retry_attempt_count(&hash)
                .expect("current budget"),
            if dispatched { 1 } else { attempts }
        );
        assert_eq!(retriever.owners.active_count(), usize::from(dispatched));
        assert!(retriever.was_requested_as_dependency(&hash).unwrap());
        assert_eq!(current.policy().initial_timestamp, 1);
    }
}

#[tokio::test]
async fn expired_recited_due_schedule_retires_before_renewal() {
    let retriever = retriever();
    let hash = Bytes::from(vec![42; 32]);
    let before = BlockRetriever::<TransportLayerStub>::current_millis();
    let mut request = state(false, 1, Some(before.saturating_sub(1)));
    request.waiting_list.push(retriever.conf.local.clone());
    retriever
        .replace_request_for_test(
            hash.clone(),
            request,
            BlockRetriever::<TransportLayerStub>::MAX_RETRIES_PER_HASH,
            0,
        )
        .expect("seed recited schedule");
    retriever
        .request_all(Duration::from_millis(1))
        .await
        .expect("maintenance");
    assert_eq!(
        retriever.transport.request_count(),
        0,
        "a due spent schedule must retire before the expiry sweep grants a new budget"
    );
    assert_eq!(
        retriever.retry_attempt_count(&hash).expect("budget"),
        BlockRetriever::<TransportLayerStub>::MAX_RETRIES_PER_HASH
    );
    assert!(
        retriever
            .owners
            .retry_budget(&hash)
            .expect("deadline")
            .1
            .expect("quarantine")
            > before
    );
    assert!(retriever.request_states().is_empty());
}

fn scheduling_fixture(
    persistent: bool,
    broadcast: bool,
) -> (BlockRetriever<TransportLayerStub>, BlockHash) {
    let retriever = retriever();
    let hash = Bytes::from(vec![27; 32]);
    let mut request = state(true, 1, None);
    let first = retriever.conf.local.clone();
    let mut second = first.clone();
    second.endpoint.tcp_port += 1;
    request.peers.extend([first, second]);
    retriever
        .replace_request_for_test(hash.clone(), request, 3, if broadcast { 2 } else { 0 })
        .unwrap();
    let owner = retriever.lookup(&hash).unwrap().unwrap();
    let until = BlockRetriever::<TransportLayerStub>::current_millis().saturating_add(60_000);
    retriever
        .owners
        .update_policy(&owner, |policy, _| {
            if broadcast {
                policy.broadcast_retry_last_request = Some(until);
            } else {
                policy.peer_requery_last_request = Some(until);
            }
        })
        .unwrap();
    if persistent {
        retriever
            .publish_pending(hash.clone(), HashSet::new(), HashSet::new())
            .unwrap();
        retriever
            .owners
            .activate(hash.clone(), owner.policy(), |_| unreachable!())
            .unwrap()
            .unwrap();
    }
    (retriever, hash)
}
async fn expired_budget_after_maintenance(
    persistent: bool,
) -> (BlockRetriever<TransportLayerStub>, BlockHash) {
    let retriever = retriever();
    let hash = Bytes::from(vec![29; 32]);
    let mut request = state(true, 1, None);
    request.waiting_list.push(retriever.conf.local.clone());
    retriever
        .replace_request_for_test(
            hash.clone(),
            request,
            BlockRetriever::<TransportLayerStub>::MAX_RETRIES_PER_HASH,
            0,
        )
        .unwrap();
    let owner = retriever.lookup(&hash).unwrap().unwrap();
    if persistent {
        retriever
            .publish_pending(hash.clone(), HashSet::new(), HashSet::new())
            .unwrap();
        retriever
            .owners
            .activate(hash.clone(), owner.policy(), |_| unreachable!())
            .unwrap()
            .unwrap();
    }
    retriever
        .request_all(Duration::from_millis(1))
        .await
        .unwrap();
    assert_eq!(retriever.transport.request_count(), 0);
    assert!(owner.policy().retry_budget_quarantine_until.is_some());
    let expired = owner.policy().retry_budget_quarantine_until.unwrap();
    retriever
        .request_all_at(Duration::from_millis(1), expired)
        .await
        .unwrap();
    (retriever, hash)
}

#[tokio::test]
async fn quarantine_expiry_volatile_sweep_does_not_send_an_autonomous_probe() {
    let (retriever, _) = expired_budget_after_maintenance(false).await;
    assert_eq!(
        retriever.transport.request_count(),
        0,
        "upstream expiry sweep does not dispatch an autonomous probe"
    );
}

async fn exhausted_transport_after_maintenance() -> (BlockRetriever<TransportLayerStub>, BlockHash)
{
    let retriever = retriever();
    let hash = Bytes::from(vec![30; 32]);
    let mut request = state(true, 1, None);
    request.waiting_list.push(retriever.conf.local.clone());
    retriever
        .replace_request_for_test(
            hash.clone(),
            request,
            BlockRetriever::<TransportLayerStub>::MAX_RETRIES_PER_HASH,
            3,
        )
        .unwrap();
    retriever
        .request_all(Duration::from_millis(1))
        .await
        .unwrap();
    assert_eq!(retriever.transport.request_count(), 0);
    assert_eq!(
        retriever.retry_attempt_count(&hash).unwrap(),
        BlockRetriever::<TransportLayerStub>::MAX_RETRIES_PER_HASH
    );
    assert!(retriever
        .is_retry_budget_quarantined(
            &hash,
            BlockRetriever::<TransportLayerStub>::current_millis()
        )
        .unwrap());
    (retriever, hash)
}

#[tokio::test]
async fn quarantine_retirement_releases_ordinary_transport_schedule() {
    let (retriever, hash) = exhausted_transport_after_maintenance().await;
    assert!(
        !retriever.request_states().contains_key(&hash),
        "upstream retirement removes the active schedule but retains the spent budget"
    );
}

#[tokio::test]
async fn quarantine_retirement_does_not_retain_ordinary_transport_provenance() {
    let (retriever, hash) = exhausted_transport_after_maintenance().await;
    assert!(!retriever.was_requested_as_dependency(&hash).unwrap());
}

#[tokio::test]
async fn quarantine_expiry_volatile_sweep_restores_retry_budget() {
    let (retriever, hash) = expired_budget_after_maintenance(false).await;
    assert_eq!(
        retriever.retry_attempt_count(&hash).unwrap(),
        0,
        "upstream expiry sweep restores a fresh retry budget"
    );
    assert!(!retriever.was_requested_as_dependency(&hash).unwrap());
}

#[tokio::test]
async fn quarantine_expiry_durable_sweep_does_not_send_an_autonomous_probe() {
    let (retriever, _) = expired_budget_after_maintenance(true).await;
    assert_eq!(
        retriever.transport.request_count(),
        0,
        "upstream expiry sweep does not dispatch an autonomous probe"
    );
}

#[tokio::test]
async fn quarantine_expiry_durable_sweep_restores_retry_budget() {
    let (retriever, hash) = expired_budget_after_maintenance(true).await;
    assert_eq!(
        retriever.retry_attempt_count(&hash).unwrap(),
        0,
        "upstream expiry sweep restores a fresh retry budget"
    );
    assert!(retriever.was_requested_as_dependency(&hash).unwrap());
}

#[tokio::test]
async fn suppressed_selection_reports_exact_action_without_completion() {
    for persistent in [false, true] {
        for broadcast in [false, true] {
            let (retriever, hash) = scheduling_fixture(persistent, broadcast);
            let owner = retriever.lookup(&hash).unwrap().unwrap();
            let recorder = metrics_util::debugging::DebuggingRecorder::new();
            let mut future = Box::pin(retriever.try_rerequest(&owner));
            let result = std::future::poll_fn(|cx| {
                metrics::with_local_recorder(&recorder, || future.as_mut().poll(cx))
            })
            .await;
            assert!(!result.unwrap());
            let mut actions = std::collections::BTreeMap::new();
            for (key, _, _, value) in recorder.snapshotter().snapshot().into_vec() {
                if let metrics_util::debugging::DebugValue::Counter(value) = value {
                    assert_ne!(key.key().name(), BLOCK_REQUESTS_RETRIES_METRIC);
                    assert_eq!(key.key().name(), BLOCK_REQUESTS_RETRY_ACTION_METRIC);
                    let label = key
                        .key()
                        .labels()
                        .find(|label| label.key() == "action")
                        .unwrap();
                    assert!(actions.insert(label.value().to_owned(), value).is_none());
                }
            }
            let expected = if broadcast {
                ["broadcast_only", "broadcast_suppressed"]
            } else {
                ["peer_requery", "peer_requery_suppressed"]
            };
            assert_eq!(actions, expected.map(|label| (label.to_owned(), 1)).into());
            assert_eq!(retriever.transport.request_count(), 0);
        }
    }
}

#[tokio::test]
async fn suppressed_selection_conflict_publishes_no_partial_state_or_metrics() {
    for broadcast in [false, true] {
        let (retriever, hash) = scheduling_fixture(true, broadcast);
        let owner = retriever.lookup(&hash).unwrap().unwrap();
        let before = owner.policy();
        let states = retriever.request_states();
        let original = states.get(&hash).unwrap();
        let mut competing = before.clone();
        competing.revision = competing.revision.checked_add(1).unwrap();
        retriever
            .casper_buffer()
            .publish_pending_request(
                BlockHashSerde(hash.clone()),
                HashSet::new(),
                HashSet::new(),
                Some(&before),
                &competing,
            )
            .unwrap();
        let recorder = metrics_util::debugging::DebuggingRecorder::new();
        let mut future = Box::pin(retriever.try_rerequest(&owner));
        let result = std::future::poll_fn(|cx| {
            metrics::with_local_recorder(&recorder, || future.as_mut().poll(cx))
        })
        .await;
        assert!(result.is_err());
        assert_eq!(owner.policy(), before);
        let states = retriever.request_states();
        let after = states.get(&hash).unwrap();
        assert_eq!(after.peers, original.peers);
        assert_eq!(after.waiting_list, original.waiting_list);
        assert_eq!(after.received, original.received);
        assert_eq!(after.in_casper_buffer, original.in_casper_buffer);
        assert_eq!(
            retriever
                .casper_buffer()
                .pending_request_policy(&BlockHashSerde(hash))
                .unwrap(),
            Some(competing)
        );
        assert!(recorder.snapshotter().snapshot().into_vec().is_empty());
        assert_eq!(retriever.transport.request_count(), 0);
        assert_eq!(
            retriever.owners.available_operations(),
            BlockRetriever::<TransportLayerStub>::MAX_RETRY_OPERATIONS
        );
    }
}

#[tokio::test]
async fn unresolved_retry_clock_is_independent_of_received_expiry() {
    let retriever = retriever();
    let hash = Bytes::from(vec![28; 32]);
    let timestamp = BlockRetriever::<TransportLayerStub>::current_millis().saturating_sub(1_000);
    let mut request = state(true, timestamp, None);
    request.waiting_list.push(retriever.conf.local.clone());
    retriever
        .replace_request_for_test(hash.clone(), request, 0, 0)
        .unwrap();
    retriever
        .request_all(Duration::from_secs(3_600))
        .await
        .unwrap();
    assert_eq!(
        retriever.transport.request_count(),
        1,
        "received expiry must not delay unresolved retry"
    );
    assert_eq!(retriever.retry_attempt_count(&hash).unwrap(), 1);
}

#[tokio::test]
async fn suppressed_known_peer_selection_preserves_upstream_clock_and_cursor() {
    for persistent in [false, true] {
        let (retriever, hash) = scheduling_fixture(persistent, false);
        let owner = retriever.lookup(&hash).unwrap().unwrap();
        let before = owner.policy();
        assert!(!retriever.try_rerequest(&owner).await.unwrap());
        let after = owner.policy();
        assert!(
            after.last_request_timestamp > before.last_request_timestamp,
            "suppressed selection advances upstream request clock"
        );
        assert_eq!(
            after.peer_requery_cursor, 1,
            "suppression must preserve selected cursor advance"
        );
        assert_eq!(
            after.peer_requery_last_request,
            before.peer_requery_last_request
        );
        assert_eq!(after.retry_attempts, before.retry_attempts);
        assert_eq!(after.peer_requery_attempts, before.peer_requery_attempts);
        assert_eq!(retriever.transport.request_count(), 0);
        assert_eq!(
            retriever.owners.available_operations(),
            BlockRetriever::<TransportLayerStub>::MAX_RETRY_OPERATIONS
        );
        if persistent {
            assert_eq!(
                retriever
                    .casper_buffer()
                    .pending_request_policy(&BlockHashSerde(hash))
                    .unwrap()
                    .unwrap(),
                after
            );
        }
    }
}

#[tokio::test]
async fn suppressed_broadcast_after_peer_budget_still_advances_selected_peer_cursor() {
    for persistent in [false, true] {
        let (retriever, hash) = scheduling_fixture(persistent, true);
        let owner = retriever.lookup(&hash).unwrap().unwrap();
        let before = owner.policy();
        assert!(!retriever.try_rerequest(&owner).await.unwrap());
        let after = owner.policy();
        assert!(
            after.last_request_timestamp > before.last_request_timestamp,
            "suppressed broadcast advances upstream request clock"
        );
        assert_eq!(
            after.peer_requery_cursor, 1,
            "peer selection precedes peer budget fallback"
        );
        assert_eq!(
            after.broadcast_retry_last_request,
            before.broadcast_retry_last_request
        );
        assert_eq!(after.retry_attempts, before.retry_attempts);
        assert_eq!(after.peer_requery_attempts, before.peer_requery_attempts);
        assert_eq!(retriever.transport.request_count(), 0);
        if persistent {
            assert_eq!(
                retriever
                    .casper_buffer()
                    .pending_request_policy(&BlockHashSerde(hash))
                    .unwrap()
                    .unwrap(),
                after
            );
        }
    }
}

async fn assert_retry_completion_preserves_replacement(recovery: bool, waiting: bool) {
    let retriever = retriever();
    let hash = Bytes::from(vec![19; 32]);
    let peer = retriever.conf.local.clone();
    let mut request = state(true, 1, None);
    request.peers.insert(peer.clone());
    if waiting {
        request.waiting_list.push(peer.clone());
    }
    *retriever.connections_cell.peers.lock().unwrap() = Connections::from_vec(vec![peer]);
    retriever
        .replace_request_for_test(hash.clone(), request, 0, 0)
        .unwrap();
    let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let callback_completed = completed.clone();
    let callback_retriever = retriever.clone();
    let callback_hash = hash.clone();
    retriever.transport.set_responses(move |_, _| {
        if !callback_completed.swap(true, std::sync::atomic::Ordering::SeqCst) {
            callback_retriever
                .forget_hash_tracking(&callback_hash)
                .unwrap();
            callback_retriever
                .record_received(callback_hash.clone())
                .unwrap();
            callback_retriever
                .owners
                .update_policy(
                    &callback_retriever
                        .lookup(&callback_hash.clone())
                        .unwrap()
                        .unwrap(),
                    |policy, _| policy.retry_attempts = 7,
                )
                .unwrap();
            callback_retriever
                .owners
                .update_policy(
                    &callback_retriever
                        .lookup(&callback_hash.clone())
                        .unwrap()
                        .unwrap(),
                    |policy, _| policy.peer_requery_attempts = 3,
                )
                .unwrap();
        }
        Ok(())
    });
    let result = if recovery {
        retriever.recover_dependency(hash.clone()).await
    } else {
        retriever.request_all(Duration::from_millis(1)).await
    };
    retriever.transport.reset();
    result.unwrap();
    assert!(completed.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        retriever.retry_attempt_count(&hash).unwrap(),
        7,
        "a completed retry must not charge a replacement request"
    );
    assert_eq!(
        retriever.peer_requery_attempt_count(&hash).unwrap(),
        3,
        "a completed peer retry must not charge a replacement request"
    );
    assert!(
        retriever
            .get_request_state_for_test(&hash)
            .await
            .unwrap()
            .unwrap()
            .received
    );
}

#[tokio::test]
async fn waiting_peer_retry_completion_preserves_replacement_request() {
    assert_retry_completion_preserves_replacement(false, true).await;
}

#[tokio::test]
async fn known_peer_retry_completion_preserves_replacement_request() {
    assert_retry_completion_preserves_replacement(false, false).await;
}

#[tokio::test]
async fn dependency_retry_completion_preserves_replacement_request() {
    assert_retry_completion_preserves_replacement(true, false).await;
}

#[tokio::test]
async fn dependency_recovery_at_capacity_creates_no_transport_or_orphan_policy() {
    let retriever = retriever();
    let hash = Bytes::from(vec![19; 32]);
    *retriever.connections_cell.peers.lock().unwrap() =
        Connections::from_vec(vec![retriever.conf.local.clone()]);
    for index in 0..BlockRetriever::<TransportLayerStub>::MAX_REQUESTED_BLOCKS_ENTRIES {
        retriever
            .replace_request_for_test(
                Bytes::from(index.to_be_bytes().to_vec()),
                state(true, 1, None),
                0,
                0,
            )
            .unwrap();
    }
    retriever.recover_dependency(hash.clone()).await.unwrap();
    assert_eq!(
        retriever.transport.request_count(),
        0,
        "capacity refusal must not dispatch an unowned dependency request"
    );
    assert!(!retriever.request_states().contains_key(&hash));
    assert!(retriever.lookup(&hash).unwrap().is_none());
}

#[tokio::test]
async fn dependency_request_promotes_previously_announced_hash() {
    let retriever = retriever();
    let hash = Bytes::from(vec![9; 32]);
    retriever
        .admit_hash(hash.clone(), None, AdmitHashReason::HashBroadcastReceived)
        .await
        .unwrap();
    assert!(!retriever.was_requested_as_dependency(&hash).unwrap());
    retriever
        .admit_hash(
            hash.clone(),
            None,
            AdmitHashReason::MissingDependencyRequested,
        )
        .await
        .unwrap();
    assert!(
        retriever.was_requested_as_dependency(&hash).unwrap(),
        "a real dependency request must promote an existing announcement"
    );
}

#[tokio::test]
async fn maintenance_reopening_preserves_retry_policy_and_original_age() {
    let retriever = retriever();
    let hash = Bytes::from(vec![9; 32]);
    let mut request = state(true, 1, None);
    request.received = true;
    retriever
        .replace_request_for_test(hash.clone(), request, 0, 0)
        .unwrap();
    retriever
        .owners
        .update_policy(
            &retriever.lookup(&hash.clone()).unwrap().unwrap(),
            |policy, _| policy.retry_attempts = 7,
        )
        .unwrap();
    retriever
        .owners
        .update_policy(
            &retriever.lookup(&hash.clone()).unwrap().unwrap(),
            |policy, _| policy.peer_requery_attempts = 3,
        )
        .unwrap();
    retriever
        .request_all(Duration::from_millis(1))
        .await
        .unwrap();
    let actual = retriever
        .get_request_state_for_test(&hash)
        .await
        .unwrap()
        .unwrap();
    assert!(!actual.received);
    assert_eq!(
        actual.initial_timestamp, 1,
        "local receipt expiry must not renew request age"
    );
    assert_eq!(retriever.retry_attempt_count(&hash).unwrap(), 7);
    assert_eq!(retriever.peer_requery_attempt_count(&hash).unwrap(), 3);
}

#[tokio::test]
async fn receipt_preserves_exhausted_budget_during_active_quarantine() {
    let retriever = retriever();
    let hash = Bytes::from(vec![9; 32]);
    let now = BlockRetriever::<TransportLayerStub>::current_millis();
    let deadline = now.saturating_add(60_000);
    retriever
        .replace_request_for_test(hash.clone(), state(true, now, Some(deadline)), 0, 0)
        .unwrap();
    retriever
        .owners
        .update_policy(
            &retriever.lookup(&hash.clone()).unwrap().unwrap(),
            |policy, _| {
                policy.retry_attempts = BlockRetriever::<TransportLayerStub>::MAX_RETRIES_PER_HASH
            },
        )
        .unwrap();
    assert!(retriever.has_exceeded_retry_budget(&hash).unwrap());
    assert!(retriever.is_retry_budget_quarantined(&hash, now).unwrap());
    retriever.ack_receive(hash.clone()).await.unwrap();
    assert!(
        retriever.has_exceeded_retry_budget(&hash).unwrap(),
        "receipt must not replenish an exhausted retry budget"
    );
    assert!(
        retriever.is_retry_budget_quarantined(&hash, now).unwrap(),
        "receipt must not bypass active retry quarantine"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn retry_clock_matches_upstream_ladder(now in any::<u64>(), timestamp in any::<u64>(), attempts in any::<u32>()) {
        let interval = 500 * (1 + u64::from(attempts.saturating_sub(4) / 4).min(7));
        prop_assert_eq!(BlockRetriever::<TransportLayerStub>::retry_due(now, timestamp, attempts), now.saturating_sub(timestamp) > interval);
        if let Some(boundary) = timestamp.checked_add(interval) {
            prop_assert!(!BlockRetriever::<TransportLayerStub>::retry_due(boundary, timestamp, attempts));
            if let Some(after) = boundary.checked_add(1) {
                prop_assert!(BlockRetriever::<TransportLayerStub>::retry_due(after, timestamp, attempts));
            }
        }
    }

    #[test]
    fn suppressed_selection_preserves_policy_for_generated_cursors(
        cursor in prop_oneof![Just(u32::MAX), any::<u32>()],
        persistent in any::<bool>(), broadcast in any::<bool>(),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let (retriever, hash) = scheduling_fixture(persistent, broadcast);
        let owner = retriever.lookup(&hash).unwrap().unwrap();
        retriever.owners.update_policy(&owner, |policy, _| policy.peer_requery_cursor = cursor).unwrap();
        let before = owner.policy();
        prop_assert!(!runtime.block_on(retriever.try_rerequest(&owner)).unwrap());
        let after = owner.policy();
        prop_assert_eq!(after.peer_requery_cursor, cursor.wrapping_add(1));
        prop_assert!(after.last_request_timestamp > before.last_request_timestamp);
        let mut normalized = after.clone();
        normalized.revision = before.revision;
        normalized.last_request_timestamp = before.last_request_timestamp;
        normalized.peer_requery_cursor = before.peer_requery_cursor;
        prop_assert_eq!(normalized, before);
        prop_assert_eq!(retriever.transport.request_count(), 0);
        prop_assert_eq!(retriever.owners.available_operations(), BlockRetriever::<TransportLayerStub>::MAX_RETRY_OPERATIONS);
        if persistent {
            prop_assert_eq!(retriever.casper_buffer().pending_request_policy(&BlockHashSerde(hash)).unwrap().unwrap(), after);
        }
    }

    #[test]
    fn received_owner_has_no_selection_side_effects(attempts in 0u32..32, cursor in any::<u32>(), expired in any::<bool>()) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let retriever = retriever();
        let hash = Bytes::from(vec![29; 32]);
        let mut request = state(true, 1, if expired { Some(1) } else { None });
        request.received = true;
        request.peer_requery_cursor = cursor;
        retriever.replace_request_for_test(hash.clone(), request, attempts, 0).unwrap();
        let owner = retriever.lookup(&hash).unwrap().unwrap();
        let before = owner.policy();
        let recorder = metrics_util::debugging::DebuggingRecorder::new();
        prop_assert!(!metrics::with_local_recorder(&recorder, || runtime.block_on(retriever.try_rerequest(&owner))).unwrap());
        prop_assert_eq!(owner.policy(), before);
        prop_assert!(recorder.snapshotter().snapshot().into_vec().is_empty());
        prop_assert_eq!(retriever.transport.request_count(), 0);
        prop_assert_eq!(retriever.owners.available_operations(), BlockRetriever::<TransportLayerStub>::MAX_RETRY_OPERATIONS);
    }

    #[test]
    fn dependency_provenance_matches_genuine_request_history(
        operations in prop::collection::vec(0u8..3, 1..65),
        initial_authority in any::<bool>(),
        initially_received in any::<bool>(),
        quarantined in any::<bool>(),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let retriever = retriever();
        let hash = Bytes::from(vec![9; 32]);
        let deadline = quarantined.then_some(u64::MAX);
        let mut request = state(initial_authority, 1, deadline);
        request.received = initially_received;
        retriever.replace_request_for_test(hash.clone(), request, 0, 0).unwrap();
        retriever.owners.update_policy(&retriever.lookup(&hash.clone()).unwrap().unwrap(), |policy, _| policy.retry_attempts = 7).unwrap();
        let mut expected = initial_authority;
        for operation in operations {
            match operation {
                0 => {
                    runtime.block_on(retriever.admit_hash(hash.clone(), None, AdmitHashReason::HashBroadcastReceived)).unwrap();
                }
                1 => {
                    runtime.block_on(retriever.admit_hash(hash.clone(), None, AdmitHashReason::MissingDependencyRequested)).unwrap();
                    expected = true;
                }
                _ => { retriever.record_received(hash.clone()).unwrap(); }
            }
            let requests = retriever.request_states();
            let current = &requests[&hash];
            prop_assert_eq!(current.requested_as_dependency, expected);
            prop_assert_eq!(current.initial_timestamp, 1);
            prop_assert_eq!(current.retry_budget_quarantine_until, deadline);
            prop_assert_eq!(retriever.retry_attempt_count(&hash).unwrap(), 7);
            prop_assert_eq!(requests.len(), 1);
        }
    }

    #[test]
    fn local_handoff_histories_preserve_policy(
        operations in prop::collection::vec(0u8..3, 1..129),
        dependency in any::<bool>(),
        initial in any::<u64>(),
        attempts in any::<u32>(),
        deadline in any::<u64>(),
    ) {
        let retriever = retriever();
        let hash = Bytes::from(vec![9; 32]);
        retriever.replace_request_for_test(hash.clone(), state(dependency, initial, Some(deadline)), 0, 0).unwrap();
        retriever.owners.update_policy(&retriever.lookup(&hash.clone()).unwrap().unwrap(), |policy, _| policy.retry_attempts = attempts).unwrap();
        retriever.owners.update_policy(&retriever.lookup(&hash.clone()).unwrap().unwrap(), |policy, _| policy.peer_requery_attempts = attempts).unwrap();
        retriever.owners.update_policy(&retriever.lookup(&hash).unwrap().unwrap(), |policy, _| {
            policy.dependency_recovery_last_request = Some(deadline);
            policy.broadcast_retry_last_request = Some(deadline);
            policy.peer_requery_last_request = Some(deadline);
        }).unwrap();
        for operation in operations {
            match operation {
                0 => { prop_assert_eq!(retriever.record_received(hash.clone()).unwrap(), RequestTracking::Tracked); }
                1 => { prop_assert_eq!(retriever.reopen_after_local_failure(hash.clone()).unwrap(), RequestTracking::Tracked); }
                _ => { retriever.reopen_stale_receipt(&hash, u64::MAX, 0).unwrap(); }
            }
            let requests = retriever.request_states();
            let actual = &requests[&hash];
            prop_assert_eq!(actual.initial_timestamp, initial);
            prop_assert_eq!(actual.requested_as_dependency, dependency);
            prop_assert_eq!(actual.retry_budget_quarantine_until, Some(deadline));
            prop_assert_eq!(requests.len(), 1);
            prop_assert_eq!(retriever.retry_attempt_count(&hash).unwrap(), attempts);
            prop_assert_eq!(retriever.peer_requery_attempt_count(&hash).unwrap(), attempts);
            let policy = retriever.lookup(&hash).unwrap().unwrap().policy();
            for value in [policy.dependency_recovery_last_request, policy.broadcast_retry_last_request, policy.peer_requery_last_request] {
                prop_assert_eq!(value, Some(deadline));
            }
        }
    }

    #[test]
    fn receipt_keeps_existing_retry_budget(
        attempts in 1u32..=u32::MAX,
        peer_attempts in 1u32..=u32::MAX,
        receipts in 1usize..17,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let retriever = retriever();
        let hash = Bytes::from(vec![9; 32]);
        retriever.replace_request_for_test(hash.clone(), state(false, 1, None), 0, 0).unwrap();
        retriever.owners.update_policy(&retriever.lookup(&hash.clone()).unwrap().unwrap(), |policy, _| policy.retry_attempts = attempts).unwrap();
        retriever.owners.update_policy(&retriever.lookup(&hash.clone()).unwrap().unwrap(), |policy, _| policy.peer_requery_attempts = peer_attempts).unwrap();
        for _ in 0..receipts {
            runtime.block_on(retriever.ack_receive(hash.clone())).unwrap();
            prop_assert_eq!(retriever.retry_attempt_count(&hash).unwrap(), attempts);
            prop_assert_eq!(retriever.peer_requery_attempt_count(&hash).unwrap(), peer_attempts);
        }
    }

    #[test]
    fn receipt_keeps_quarantine_and_cooldowns(
        deadline in 1u64..=u64::MAX,
        cooldown in 1u64..=u64::MAX,
        receipts in 1usize..17,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let retriever = retriever();
        let hash = Bytes::from(vec![9; 32]);
        retriever.replace_request_for_test(hash.clone(), state(true, 1, Some(deadline)), 0, 0).unwrap();
        let owner = retriever.lookup(&hash).unwrap().unwrap();
        retriever.owners.update_policy(&owner, |policy, _| {
            policy.dependency_recovery_last_request = Some(cooldown);
            policy.broadcast_retry_last_request = Some(cooldown);
            policy.peer_requery_last_request = Some(cooldown);
        }).unwrap();
        for _ in 0..receipts {
            runtime.block_on(retriever.ack_receive(hash.clone())).unwrap();
            let actual = runtime.block_on(retriever.get_request_state_for_test(&hash)).unwrap().unwrap();
            prop_assert_eq!(actual.retry_budget_quarantine_until, Some(deadline));
            let policy = owner.policy();
            for actual in [policy.dependency_recovery_last_request, policy.broadcast_retry_last_request,
                policy.peer_requery_last_request] {
                prop_assert_eq!(actual, Some(cooldown));
            }
        }
    }

    #[test]
    fn receipt_keeps_dependency_and_original_timestamp(
        dependency in any::<bool>(),
        original in any::<u64>(),
        receipts in 1usize..17,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let retriever = retriever();
        let hash = Bytes::from(vec![9; 32]);
        retriever.replace_request_for_test(hash.clone(), state(dependency, original, None), 0, 0).unwrap();
        for _ in 0..receipts {
            runtime.block_on(retriever.ack_receive(hash.clone())).unwrap();
            let actual = runtime.block_on(retriever.get_request_state_for_test(&hash)).unwrap().unwrap();
            prop_assert_eq!(actual.requested_as_dependency, dependency);
            prop_assert_eq!(actual.initial_timestamp, original);
            prop_assert!(actual.received);
            prop_assert!(!actual.in_casper_buffer);
        }
    }
}

#[test]
fn maintenance_snapshot_cannot_overwrite_a_completed_worker_handoff() {
    let retriever = retriever();
    let hash = Bytes::from(vec![9; 32]);
    let mut request = state(true, 1, Some(u64::MAX));
    request.received = true;
    retriever
        .replace_request_for_test(hash.clone(), request, 0, 0)
        .unwrap();
    retriever
        .owners
        .update_policy(
            &retriever.lookup(&hash.clone()).unwrap().unwrap(),
            |policy, _| policy.retry_attempts = 7,
        )
        .unwrap();
    let observer = retriever.clone();
    let observed_hash = hash.clone();
    let (observed, snapshot) = std::sync::mpsc::channel();
    let (handoff, completed) = std::sync::mpsc::channel();
    let maintenance = std::thread::spawn(move || {
        let was_received = observer.request_states()[&observed_hash].received;
        assert!(was_received);
        observed.send(()).unwrap();
        completed.recv_timeout(Duration::from_secs(5)).unwrap();
        observer
            .reopen_stale_receipt(&observed_hash, u64::MAX, 0)
            .unwrap()
    });
    snapshot.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        retriever.reopen_after_local_failure(hash.clone()).unwrap(),
        RequestTracking::Tracked
    );
    let handoff_time = retriever.request_states()[&hash].timestamp;
    handoff.send(()).unwrap();
    assert!(!maintenance.join().unwrap());
    let actual = retriever.request_states()[&hash].clone();
    assert!(!actual.received);
    assert_eq!(actual.timestamp, handoff_time);
    assert_eq!(actual.initial_timestamp, 1);
    assert_eq!(actual.retry_budget_quarantine_until, Some(u64::MAX));
    assert_eq!(retriever.retry_attempt_count(&hash).unwrap(), 7);
}

#[test]
fn inactive_quarantine_is_not_reported_as_capacity_refusal_for_receipt() {
    assert_inactive_quarantine_refusal(false);
}

#[test]
fn inactive_quarantine_is_not_reported_as_capacity_refusal_for_local_recovery() {
    assert_inactive_quarantine_refusal(true);
}

fn assert_inactive_quarantine_refusal(reopen: bool) {
    let retriever = retriever();
    let hash = Bytes::from(vec![44; 32]);
    retriever
        .replace_request_for_test(
            hash.clone(),
            state(true, 1, None),
            BlockRetriever::<TransportLayerStub>::MAX_RETRIES_PER_HASH,
            0,
        )
        .unwrap();
    let owner = retriever.lookup(&hash).unwrap().unwrap();
    let now = BlockRetriever::<TransportLayerStub>::current_millis();
    assert!(matches!(
        retriever
            .owners
            .reserve_retry_with(
                &owner,
                BlockRetriever::<TransportLayerStub>::MAX_RETRIES_PER_HASH,
                now,
                BlockRetriever::<TransportLayerStub>::RETRY_BUDGET_QUARANTINE_MS,
                |_, _, _| RetrySelection::Dispatch(false, ()),
            )
            .unwrap(),
        PreparedRetry::None
    ));
    let before = retriever.owners.retry_budget(&hash).unwrap();
    assert_eq!(retriever.owners.active_count(), 0);
    let outcome = if reopen {
        retriever.reopen_after_local_failure(hash.clone())
    } else {
        retriever.record_received(hash.clone())
    }
    .unwrap();
    assert_eq!(retriever.owners.retry_budget(&hash).unwrap(), before);
    assert_eq!(retriever.owners.active_count(), 0);
    assert!(!retriever.was_requested_as_dependency(&hash).unwrap());
    assert_ne!(outcome, RequestTracking::Tracked);
    assert_ne!(
        outcome,
        RequestTracking::AtCapacity,
        "quarantine refusal with free capacity must identify quarantine"
    );
}

#[test]
fn local_handoff_at_capacity_neither_invents_authority_nor_discards_existing_work() {
    let retriever = retriever();
    for index in 0..BlockRetriever::<TransportLayerStub>::MAX_REQUESTED_BLOCKS_ENTRIES {
        let key = Bytes::from((index as u64).to_be_bytes().to_vec());
        retriever
            .replace_request_for_test(key, state(true, 1, None), 0, 0)
            .unwrap();
    }
    let hash = Bytes::from(vec![9; 32]);
    assert_eq!(
        retriever.reopen_after_local_failure(hash.clone()).unwrap(),
        RequestTracking::AtCapacity
    );
    assert_eq!(
        retriever.record_received(hash.clone()).unwrap(),
        RequestTracking::AtCapacity
    );
    assert_eq!(
        retriever.request_states().len(),
        BlockRetriever::<TransportLayerStub>::MAX_REQUESTED_BLOCKS_ENTRIES
    );
    assert!(!retriever.request_states().contains_key(&hash));
    retriever
        .forget_hash_tracking(&Bytes::from(0u64.to_be_bytes().to_vec()))
        .unwrap();
    assert_eq!(
        retriever.reopen_after_local_failure(hash.clone()).unwrap(),
        RequestTracking::Tracked
    );
    let request = retriever.request_states()[&hash].clone();
    assert!(!request.requested_as_dependency);
    assert!(!request.received);
}
