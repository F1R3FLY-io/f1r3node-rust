use std::any::Any;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::AtomicBool;

use proptest::prelude::*;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::store::key_value_store::{
    AtomicStoreMutation, AtomicStoreOperation, EntryReader, KeyValueStore, ValueReader,
};
use shared::rust::ByteBuffer;

use super::*;

type Rows = HashMap<BlockHashSerde, HashSet<BlockHashSerde>>;

fn hash(value: u8) -> BlockHashSerde {
    BlockHashSerde(vec![value; models::rust::block_hash::LENGTH].into())
}

#[derive(Clone)]
struct TestStores {
    parents: KeyValueTypedStoreImpl<BlockHashSerde, HashSet<BlockHashSerde>>,
    pending: Arc<dyn KeyValueStore>,
}

impl std::ops::Deref for TestStores {
    type Target = KeyValueTypedStoreImpl<BlockHashSerde, HashSet<BlockHashSerde>>;
    fn deref(&self) -> &Self::Target { &self.parents }
}

impl TestStores {
    async fn open(&self) -> Result<CasperBufferKeyValueStorage, KvStoreError> {
        CasperBufferKeyValueStorage::new_from_kv_store(self.parents.clone(), self.pending.clone())
            .await
    }
}

fn store() -> TestStores {
    let coordinator = Arc::new(RwLock::new(()));
    TestStores {
        parents: KeyValueTypedStoreImpl::new(Arc::new(
            InMemoryKeyValueStore::new_with_coordinator(coordinator.clone()),
        )),
        pending: Arc::new(InMemoryKeyValueStore::new_with_coordinator(coordinator)),
    }
}

#[tokio::test]
async fn expiry_batches_are_bounded_and_do_not_consume_recovery_order() {
    let durable = store();
    let buffer = durable.open().await.unwrap();
    for key in 0u8..130 {
        buffer.put_pendant(hash(key)).unwrap();
    }
    assert!(buffer.next_expiry_scan_batch(0).is_empty());
    let first = buffer.next_expiry_scan_batch(130);
    assert_eq!(first, (0u8..64).map(hash).collect::<Vec<_>>());
    for key in 0u8..130 {
        assert_eq!(buffer.next_scan_candidate(), Some(hash(key)));
    }
    let second = buffer.next_expiry_scan_batch(66);
    let third = buffer.next_expiry_scan_batch(2);
    assert_eq!(second, (64u8..128).map(hash).collect::<Vec<_>>());
    assert_eq!(third, (128u8..130).map(hash).collect::<Vec<_>>());
    buffer.remove(hash(0)).unwrap();
    buffer.put_pendant(hash(130)).unwrap();
    let first_after_removal = buffer.next_expiry_scan_batch(1);
    assert_eq!(first_after_removal, vec![hash(1)]);
    assert_eq!(buffer.next_scan_candidate(), Some(hash(1)));

    let restored = durable.open().await.unwrap();
    let count = restored.scan_candidate_count();
    let mut expiry = HashSet::new();
    let mut remaining = count;
    while remaining != 0 {
        let batch = restored.next_expiry_scan_batch(remaining);
        assert!(!batch.is_empty());
        assert!(batch.len() <= 64);
        remaining -= batch.len();
        expiry.extend(batch);
    }
    let recovery: HashSet<_> = (0..count)
        .map(|_| restored.next_scan_candidate().unwrap())
        .collect();
    assert_eq!(expiry, (1u8..=130).map(hash).collect());
    assert_eq!(recovery, expiry);
}

#[tokio::test]
async fn pruning_preserves_middle_block_dependency_across_restart() {
    let durable = store();
    let buffer = durable.open().await.unwrap();
    buffer.add_relation(hash(1), hash(2)).unwrap();
    buffer.add_relation(hash(2), hash(3)).unwrap();
    buffer.enforce_limits(usize::MAX, 0, 1, 0).unwrap();
    let current = buffer.get_parents(&hash(3));
    drop(buffer);
    let restored = durable.open().await.unwrap();
    let expected = Some(HashSet::from([hash(2)]));
    assert_eq!(
        (current, restored.get_parents(&hash(3))),
        (expected.clone(), expected),
        "cache eviction must not resolve an unadmitted parent"
    );
}

#[tokio::test]
async fn pruning_preserves_isolated_retry_owner_across_restart() {
    let durable = store();
    let buffer = durable.open().await.unwrap();
    buffer.put_pendant(hash(1)).unwrap();
    buffer.enforce_limits(0, u64::MAX, 1, 0).unwrap();
    let durable_owner = durable.get_one(&hash(1)).unwrap();
    drop(buffer);
    let restored = durable.open().await.unwrap();
    assert_eq!(
        (durable_owner, restored.is_pendant(&hash(1))),
        (Some(HashSet::new()), true),
        "pressure eviction must retain a durable unresolved retry owner"
    );
}

#[tokio::test]
async fn pruning_preserves_last_certificate_waiter_across_restart() {
    let durable = store();
    let buffer = durable.open().await.unwrap();
    buffer.add_certificate_relation(hash(9), hash(1)).unwrap();
    buffer.enforce_limits(usize::MAX, 0, 1, 0).unwrap();
    let current = buffer.get_missing_certificate_dependencies();
    drop(buffer);
    let restored = durable.open().await.unwrap();
    let expected = HashSet::from([hash(9)]);
    assert_eq!(
        (current, restored.get_missing_certificate_dependencies()),
        (expected.clone(), expected),
        "request reconciliation must include the last cold certificate waiter"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn pruning_preserves_durable_graph_operation_sequences(
        operations in prop::collection::vec((0_u8..6, 0_u8..8, 0_u8..8), 1..65),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let durable = store();
            let mut buffer = durable.open()
                .await
                .unwrap();
            let mut expected = Rows::new();
            for (operation, first, second) in operations {
                let key = hash(first);
                match operation {
                    0 => {
                        buffer.put_pendant(key.clone()).unwrap();
                        expected.entry(key).or_default();
                    }
                    1 => {
                        let parent = hash(second);
                        buffer.add_relation(parent.clone(), key.clone()).unwrap();
                        expected.entry(key).or_default().insert(parent);
                    }
                    2 => {
                        buffer.enforce_limits(usize::MAX, 0, usize::from(second) + 1, 0).unwrap();
                    }
                    3 => {
                        buffer.enforce_limits(usize::from(first), u64::MAX, usize::from(second) + 1, 0).unwrap();
                    }
                    4 => {
                        drop(buffer);
                        buffer = durable.open()
                            .await
                            .unwrap();
                    }
                    _ => {
                        buffer.remove(key.clone()).unwrap();
                        expected.remove(&key);
                        for parents in expected.values_mut() {
                            parents.remove(&key);
                        }
                    }
                }
                prop_assert_eq!(
                    durable.to_map().unwrap(),
                    expected.clone(),
                    "only explicit resolution may remove durable rows or dependency edges"
                );
            }
            Ok(())
        })?;
    }
}

fn assert_projection(buffer: &CasperBufferKeyValueStorage, expected: &Rows) {
    let _guard = buffer.read_guard();
    assert_eq!(&buffer.parents_store.to_map().unwrap(), expected);
    let dag = buffer.block_dependency_dag.lock().unwrap();
    assert_eq!(
        *buffer.explicit_blocks.lock().unwrap(),
        expected.keys().cloned().collect()
    );
    let mut forward: Rows = HashMap::new();
    let backward: Rows = expected
        .iter()
        .filter(|(_, parents)| !parents.is_empty())
        .map(|(key, parents)| (key.clone(), parents.clone()))
        .collect();
    let mut ready: HashSet<_> = expected.keys().cloned().collect();
    for (child, parents) in expected {
        for parent in parents {
            forward
                .entry(parent.clone())
                .or_default()
                .insert(child.clone());
            ready.insert(parent.clone());
        }
    }
    for key in &ready {
        assert!(buffer.first_seen_ms.contains_key(key));
    }
    let candidates: HashSet<_> = ready
        .iter()
        .filter(|key| {
            CasperBufferKeyValueStorage::certificate_digest_from_dependency_key(key).is_none()
        })
        .cloned()
        .collect();
    let mut index = buffer.scan_candidates.lock().unwrap();
    assert_eq!(index.len(), candidates.len());
    let scanned: HashSet<_> = (0..index.len()).map(|_| index.next().unwrap()).collect();
    assert_eq!(scanned, candidates);
    ready.retain(|key| !backward.contains_key(key));
    assert_eq!(dag.parent_to_child_adjacency_list, forward);
    assert_eq!(dag.child_to_parent_adjacency_list, backward);
    assert_eq!(
        dag.dependency_free.iter().cloned().collect::<HashSet<_>>(),
        ready
    );
    assert_eq!(
        dag.child_to_parent_adjacency_list.len() + dag.dependency_free.len(),
        expected
            .keys()
            .chain(expected.values().flatten())
            .collect::<HashSet<_>>()
            .len()
    );
}

#[tokio::test]
async fn durable_empty_rows_restore_explicit_pendants() {
    let store = store();
    store.put_one(hash(1), HashSet::new()).unwrap();
    let buffer = store.open().await.unwrap();
    assert_projection(&buffer, &HashMap::from([(hash(1), HashSet::new())]));
}

fn pending_policy() -> PendingRequestPolicy {
    PendingRequestPolicy {
        revision: 1,
        initial_timestamp: 3,
        last_request_timestamp: 4,
        requested_as_dependency: false,
        retry_attempts: 7,
        peer_requery_attempts: 2,
        peer_requery_cursor: 1,
        retry_budget_quarantine_until: Some(100),
        dependency_recovery_last_request: Some(5),
        broadcast_retry_last_request: Some(6),
        peer_requery_last_request: Some(7),
    }
}

#[tokio::test]
async fn pending_policy_update_compares_the_stored_dependency_bytes() {
    assert_raw_dependency_guard(false).await;
}

#[tokio::test]
async fn pending_policy_renewal_compares_the_stored_dependency_bytes() {
    assert_raw_dependency_guard(true).await;
}

async fn assert_raw_dependency_guard(renewal: bool) {
    let stores = store();
    let buffer = stores.open().await.unwrap();
    let original = pending_policy();
    let parents = HashSet::from([hash(1), hash(2)]);
    buffer
        .publish_pending_request(hash(3), parents.clone(), HashSet::new(), None, &original)
        .unwrap();
    let key = stores.encode_key(&hash(3)).unwrap();
    let bytes = bincode::serialize(&vec![hash(1), hash(1), hash(2)]).unwrap();
    assert_eq!(stores.decode_value(&bytes).unwrap(), parents);
    stores
        .raw_store()
        .put(vec![(key.clone(), bytes.clone())])
        .unwrap();
    let mut expected = original.clone();
    expected.advance_revision().unwrap();
    if renewal {
        expected.retry_attempts = 0;
        expected.retry_budget_quarantine_until = None;
        assert_eq!(
            buffer
                .renew_pending_request_policy(&hash(3), &original, 100)
                .unwrap(),
            expected
        );
    } else {
        expected.retry_attempts += 1;
        buffer
            .update_pending_request_policy(&hash(3), &original, &expected)
            .unwrap();
    }
    assert_eq!(stores.raw_store().get(&vec![key]).unwrap(), vec![Some(
        bytes
    )]);
    assert_eq!(
        buffer.pending_request_policy(&hash(3)).unwrap(),
        Some(expected)
    );
    assert_eq!(buffer.get_parents(&hash(3)), Some(parents));
}

fn mutate_pending_policy(
    buffer: &CasperBufferKeyValueStorage,
    original: &PendingRequestPolicy,
    renewal: bool,
) -> Result<PendingRequestPolicy, KvStoreError> {
    if renewal {
        buffer.renew_pending_request_policy(&hash(3), original, 100)
    } else {
        let mut next = original.clone();
        next.advance_revision()?;
        next.retry_attempts += 1;
        buffer.update_pending_request_policy(&hash(3), original, &next)?;
        Ok(next)
    }
}

#[tokio::test]
async fn pending_policy_guard_rejects_missing_and_malformed_dependency_rows() {
    for renewal in [false, true] {
        for replacement in [None, Some(vec![0xff])] {
            let stores = store();
            let buffer = stores.open().await.unwrap();
            let original = pending_policy();
            buffer
                .publish_pending_request(hash(3), HashSet::new(), HashSet::new(), None, &original)
                .unwrap();
            let key = stores.encode_key(&hash(3)).unwrap();
            match &replacement {
                Some(bytes) => stores
                    .raw_store()
                    .put_one(key.clone(), bytes.clone())
                    .unwrap(),
                None => {
                    stores.raw_store().delete(vec![key.clone()]).unwrap();
                }
            }
            let result = mutate_pending_policy(&buffer, &original, renewal);
            assert!(result.is_err());
            if replacement.is_none() {
                assert!(matches!(result, Err(KvStoreError::TransactionConflict(_))));
            }
            assert_eq!(stores.raw_store().get_one(&key).unwrap(), replacement);
            assert_eq!(
                stores.pending.get_one(&key).unwrap(),
                Some(original.encode().unwrap())
            );
        }
    }
}

#[tokio::test]
async fn pending_policy_guard_rejects_concurrent_dependency_row_replacement() {
    for renewal in [false, true] {
        for replacement in [None, Some(vec![hash(1), hash(1)]), Some(vec![hash(2)])] {
            let mut stores = store();
            let raw = stores.raw_store().clone();
            let pause = Arc::new(ReadPause {
                armed: AtomicBool::new(false),
                captured: std::sync::Barrier::new(2),
                resume: std::sync::Barrier::new(2),
            });
            stores.parents = KeyValueTypedStoreImpl::new(Arc::new(FaultStore {
                inner: raw.clone(),
                fail_commit: Arc::new(AtomicBool::new(false)),
                exit_after_commit: None,
                supports_atomic: true,
                read_pause: Some(pause.clone()),
            }));
            let buffer = stores.open().await.unwrap();
            let original = pending_policy();
            buffer
                .publish_pending_request(
                    hash(3),
                    HashSet::from([hash(1)]),
                    HashSet::new(),
                    None,
                    &original,
                )
                .unwrap();
            let key = stores.encode_key(&hash(3)).unwrap();
            let replacement = replacement.map(|parents| bincode::serialize(&parents).unwrap());
            pause.armed.store(true, Ordering::SeqCst);
            let result = std::thread::scope(|scope| {
                let update = scope.spawn(|| mutate_pending_policy(&buffer, &original, renewal));
                pause.captured.wait();
                match &replacement {
                    Some(bytes) => raw.put_one(key.clone(), bytes.clone()).unwrap(),
                    None => {
                        raw.delete(vec![key.clone()]).unwrap();
                    }
                }
                pause.resume.wait();
                update.join().unwrap()
            });
            assert!(matches!(result, Err(KvStoreError::TransactionConflict(_))));
            assert_eq!(raw.get_one(&key).unwrap(), replacement);
            assert_eq!(
                stores.pending.get_one(&key).unwrap(),
                Some(original.encode().unwrap())
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn pending_policy_guard_preserves_generated_dependency_representations(
        members in prop::collection::vec(0u8..16, 0..33),
        renewal in any::<bool>(),
        conflict in any::<bool>(),
        stale in any::<bool>(),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let (stores, failure) = fault_store();
            let buffer = stores.open().await.unwrap();
            let original = pending_policy();
            let sequence: Vec<_> = members.into_iter().map(hash).collect();
            let parents: HashSet<_> = sequence.iter().cloned().collect();
            buffer.publish_pending_request(hash(3), parents.clone(), HashSet::new(), None, &original).unwrap();
            let key = stores.encode_key(&hash(3)).unwrap();
            let bytes = bincode::serialize(&sequence).unwrap();
            prop_assert_eq!(stores.decode_value(&bytes).unwrap(), parents);
            stores.raw_store().put_one(key.clone(), bytes.clone()).unwrap();
            let mut expected = original.clone();
            if stale { expected.advance_revision().unwrap(); }
            failure.store(conflict, Ordering::SeqCst);
            let result = mutate_pending_policy(&buffer, &expected, renewal);
            prop_assert_eq!(result.is_err(), conflict || stale);
            prop_assert_eq!(stores.raw_store().get_one(&key).unwrap(), Some(bytes));
            let next = result.unwrap_or(original);
            prop_assert_eq!(stores.pending.get_one(&key).unwrap(), Some(next.encode().unwrap()));
            Ok(())
        })?;
    }
}

#[tokio::test]
async fn pending_policy_renewal_preserves_recitation_fields_and_reopens_exactly() {
    let stores = store();
    let buffer = stores.open().await.unwrap();
    let mut original = pending_policy();
    original.requested_as_dependency = true;
    buffer
        .publish_pending_request(
            hash(3),
            HashSet::from([hash(1)]),
            HashSet::from([hash(9)]),
            None,
            &original,
        )
        .unwrap();
    let rows = stores.to_map().unwrap();
    assert!(buffer
        .renew_pending_request_policy(&hash(3), &original, 99)
        .is_err());
    let mut expected = original.clone();
    expected.advance_revision().unwrap();
    expected.retry_attempts = 0;
    expected.retry_budget_quarantine_until = None;
    assert!(buffer
        .update_pending_request_policy(&hash(3), &original, &expected)
        .is_err());
    assert_eq!(
        buffer.pending_request_policy(&hash(3)).unwrap(),
        Some(original.clone())
    );
    assert_eq!(
        buffer
            .renew_pending_request_policy(&hash(3), &original, 100)
            .unwrap(),
        expected
    );
    assert_projection(&buffer, &rows);
    assert!(buffer
        .renew_pending_request_policy(&hash(3), &original, 100)
        .is_err());
    assert!(buffer
        .renew_pending_request_policy(&hash(3), &expected, 101)
        .is_err());
    let restored = stores.open().await.unwrap();
    assert_eq!(
        restored.pending_request_policy(&hash(3)).unwrap(),
        Some(expected)
    );
    assert_projection(&restored, &rows);
}

#[tokio::test]
async fn pending_policy_quarantine_commits_observed_total_and_clears_peer_schedule() {
    let (stores, failure) = fault_store();
    let buffer = stores.open().await.unwrap();
    let original = pending_policy();
    buffer
        .publish_pending_request(
            hash(3),
            HashSet::from([hash(1)]),
            HashSet::new(),
            None,
            &original,
        )
        .unwrap();
    let mut effective = original.clone();
    effective.retry_attempts += 2;
    effective.peer_requery_attempts += 1;
    let rows = stores.to_map().unwrap();
    let mut expected = effective.clone();
    expected.advance_revision().unwrap();
    expected.retry_budget_quarantine_until = Some(200);
    expected.peer_requery_attempts = 0;
    expected.peer_requery_cursor = 0;
    expected.dependency_recovery_last_request = None;
    expected.broadcast_retry_last_request = None;
    expected.peer_requery_last_request = None;
    assert!(buffer
        .update_pending_request_policy(&hash(3), &original, &expected)
        .is_err());
    failure.store(true, Ordering::SeqCst);
    assert!(matches!(
        buffer.quarantine_pending_request_policy(&hash(3), &original, &effective, 8, 200),
        Err(KvStoreError::TransactionConflict(_))
    ));
    assert_eq!(
        buffer.pending_request_policy(&hash(3)).unwrap(),
        Some(original.clone())
    );
    assert_projection(&buffer, &rows);
    assert_eq!(
        buffer
            .quarantine_pending_request_policy(&hash(3), &original, &effective, 8, 200)
            .unwrap(),
        expected
    );
    let restored = stores.open().await.unwrap();
    assert_eq!(
        restored.pending_request_policy(&hash(3)).unwrap(),
        Some(expected)
    );
    assert_projection(&restored, &rows);
    assert!(buffer
        .quarantine_pending_request_policy(&hash(3), &original, &effective, 8, 200)
        .is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn pending_policy_quarantine_cycles_preserve_atomic_fields(
        operations in prop::collection::vec((0u8..4, any::<bool>(), any::<bool>()), 1..65),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let (stores, failure) = fault_store();
            let mut buffer = stores.open().await.unwrap();
            let mut expected = pending_policy();
            expected.retry_attempts = 0;
            expected.peer_requery_attempts = 0;
            expected.retry_budget_quarantine_until = None;
            buffer.publish_pending_request(hash(3), HashSet::from([hash(1)]), HashSet::new(), None, &expected).unwrap();
            let original_rows = stores.to_map().unwrap();
            for (step, (operation, fails, peer)) in operations.into_iter().enumerate() {
                let now = step as u64;
                if operation == 3 {
                    buffer = stores.open().await.unwrap();
                } else {
                    let mut next = expected.clone();
                    next.advance_revision().unwrap();
                    failure.store(fails, Ordering::SeqCst);
                    let (eligible, result) = match operation {
                        0 => {
                            next.retry_attempts += 1;
                            next.peer_requery_attempts += u32::from(peer);
                            next.last_request_timestamp = now;
                            next.peer_requery_cursor = next.peer_requery_cursor.wrapping_add(1);
                            next.peer_requery_last_request = Some(now);
                            (true, buffer.update_pending_request_policy(&hash(3), &expected, &next))
                        }
                        1 => {
                            next.retry_budget_quarantine_until = Some(now + 3);
                            next.peer_requery_attempts = 0;
                            next.peer_requery_cursor = 0;
                            next.dependency_recovery_last_request = None;
                            next.broadcast_retry_last_request = None;
                            next.peer_requery_last_request = None;
                            (expected.retry_attempts >= 4,
                             buffer.quarantine_pending_request_policy(&hash(3), &expected, &expected, 4, now + 3).map(|_| ()))
                        }
                        _ => {
                            next.retry_attempts = 0;
                            next.retry_budget_quarantine_until = None;
                            (expected.retry_budget_quarantine_until.is_some_and(|deadline| deadline <= now),
                             buffer.renew_pending_request_policy(&hash(3), &expected, now).map(|_| ()))
                        }
                    };
                    prop_assert_eq!(result.is_ok(), eligible && !fails);
                    if result.is_ok() { expected = next; }
                }
                prop_assert_eq!(buffer.pending_request_policy(&hash(3)).unwrap(), Some(expected.clone()));
                prop_assert_eq!(stores.to_map().unwrap(), original_rows.clone());
            }
            Ok(())
        })?;
    }
}

#[tokio::test]
async fn pending_policy_renewal_conflict_preserves_both_stores_and_allows_retry() {
    let (stores, failure) = fault_store();
    let buffer = stores.open().await.unwrap();
    let original = pending_policy();
    buffer
        .publish_pending_request(
            hash(3),
            HashSet::from([hash(1)]),
            HashSet::new(),
            None,
            &original,
        )
        .unwrap();
    let rows = stores.to_map().unwrap();
    failure.store(true, Ordering::SeqCst);
    assert!(matches!(
        buffer.renew_pending_request_policy(&hash(3), &original, 100),
        Err(KvStoreError::TransactionConflict(_))
    ));
    assert_eq!(
        buffer.pending_request_policy(&hash(3)).unwrap(),
        Some(original.clone())
    );
    assert_projection(&buffer, &rows);
    let restored = stores.open().await.unwrap();
    assert_eq!(
        restored.pending_request_policy(&hash(3)).unwrap(),
        Some(original.clone())
    );
    let updated = restored
        .renew_pending_request_policy(&hash(3), &original, 100)
        .unwrap();
    assert_eq!(updated.retry_attempts, 0);
    assert_projection(&restored, &rows);
}

#[tokio::test]
async fn pending_policy_renewal_has_one_concurrent_winner_and_preserves_other_hashes() {
    let stores = store();
    let buffer = stores.open().await.unwrap();
    let original = pending_policy();
    for key in [3, 4] {
        buffer
            .publish_pending_request(
                hash(key),
                HashSet::from([hash(1)]),
                HashSet::new(),
                None,
                &original,
            )
            .unwrap();
    }
    let rows = stores.to_map().unwrap();
    let barrier = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let handles = [0, 1].map(|_| {
            scope.spawn(|| {
                barrier.wait();
                buffer.renew_pending_request_policy(&hash(3), &original, 100)
            })
        });
        handles.map(|handle| handle.join().unwrap())
    });
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(KvStoreError::TransactionConflict(_))))
            .count(),
        1
    );
    assert_eq!(
        buffer.pending_request_policy(&hash(4)).unwrap(),
        Some(original)
    );
    assert_projection(&buffer, &rows);
    let updated = results.into_iter().find_map(Result::ok).unwrap();
    assert_eq!(
        stores
            .open()
            .await
            .unwrap()
            .pending_request_policy(&hash(3))
            .unwrap(),
        Some(updated)
    );
}

#[tokio::test]
async fn pending_policy_commit_and_subset_update_survive_reopen() {
    let store = store();
    let buffer = store.open().await.unwrap();
    let policy = pending_policy();
    buffer
        .publish_pending_request(
            hash(3),
            HashSet::from([hash(1)]),
            HashSet::from([hash(9)]),
            None,
            &policy,
        )
        .unwrap();
    let rows = store.to_map().unwrap();
    let mut promoted = policy.clone();
    promoted.advance_revision().unwrap();
    promoted.requested_as_dependency = true;
    buffer
        .publish_pending_request(
            hash(3),
            HashSet::from([hash(1)]),
            HashSet::new(),
            Some(&policy),
            &promoted,
        )
        .unwrap();
    assert_eq!(store.to_map().unwrap(), rows);
    assert_eq!(
        buffer.pending_request_policy(&hash(3)).unwrap(),
        Some(promoted.clone())
    );
    drop(buffer);
    let restored = store.open().await.unwrap();
    assert_eq!(
        restored.pending_request_policy(&hash(3)).unwrap(),
        Some(promoted)
    );
    assert_eq!(store.to_map().unwrap(), rows);
}

#[tokio::test]
async fn pending_policy_failure_and_stale_update_preserve_both_stores() {
    let (store, failure) = fault_store();
    let buffer = store.open().await.unwrap();
    let policy = pending_policy();
    buffer.add_relation(hash(1), hash(3)).unwrap();
    let original = store.to_map().unwrap();
    failure.store(true, Ordering::SeqCst);
    assert!(matches!(
        buffer.publish_pending_request(
            hash(3),
            HashSet::from([hash(2)]),
            HashSet::from([hash(9)]),
            None,
            &policy
        ),
        Err(KvStoreError::TransactionConflict(_))
    ));
    assert_projection(&buffer, &original);
    assert!(buffer.pending_request_policy(&hash(3)).unwrap().is_none());
    buffer
        .publish_pending_request(hash(3), HashSet::new(), HashSet::new(), None, &policy)
        .unwrap();
    let mut next = policy.clone();
    next.advance_revision().unwrap();
    next.retry_attempts += 1;
    buffer
        .update_pending_request_policy(&hash(3), &policy, &next)
        .unwrap();
    assert!(matches!(
        buffer.publish_pending_request(
            hash(3),
            HashSet::from([hash(2)]),
            HashSet::new(),
            Some(&policy),
            &next
        ),
        Err(KvStoreError::TransactionConflict(_))
    ));
    assert_projection(&buffer, &original);
    assert_eq!(
        buffer.pending_request_policy(&hash(3)).unwrap(),
        Some(next.clone())
    );
    failure.store(true, Ordering::SeqCst);
    assert!(buffer.remove(hash(3)).is_err());
    assert_projection(&buffer, &original);
    assert_eq!(
        buffer.pending_request_policy(&hash(3)).unwrap(),
        Some(next.clone())
    );
    buffer.remove(hash(3)).unwrap();
    assert!(buffer.pending_request_policy(&hash(3)).unwrap().is_none());
    let mut later = next.clone();
    later.advance_revision().unwrap();
    assert!(buffer
        .update_pending_request_policy(&hash(3), &next, &later)
        .is_err());
    assert!(store.to_map().unwrap().is_empty());
    assert!(store.pending.to_map().unwrap().is_empty());
}

#[tokio::test]
async fn pending_policy_concurrent_revisions_commit_exactly_one_complete_update() {
    let store = store();
    let buffer = store.open().await.unwrap();
    let original = pending_policy();
    buffer
        .publish_pending_request(hash(3), HashSet::new(), HashSet::new(), None, &original)
        .unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let results = std::thread::scope(|scope| {
        let handles = [1u8, 2].map(|candidate| {
            let buffer = buffer.clone();
            let barrier = barrier.clone();
            let original = original.clone();
            scope.spawn(move || {
                let mut next = original.clone();
                next.advance_revision().unwrap();
                next.retry_attempts += u32::from(candidate);
                barrier.wait();
                let result = buffer.publish_pending_request(
                    hash(3),
                    HashSet::from([hash(candidate)]),
                    HashSet::new(),
                    Some(&original),
                    &next,
                );
                (candidate, next, result)
            })
        });
        handles.map(|handle| handle.join().unwrap())
    });
    assert_eq!(
        results
            .iter()
            .filter(|(_, _, result)| result.is_ok())
            .count(),
        1
    );
    let (winner, expected, _) = results
        .iter()
        .find(|(_, _, result)| result.is_ok())
        .unwrap();
    assert_eq!(
        buffer.get_parents(&hash(3)),
        Some(HashSet::from([hash(*winner)]))
    );
    assert_eq!(
        buffer.pending_request_policy(&hash(3)).unwrap(),
        Some(expected.clone())
    );
}

#[tokio::test]
async fn pending_policy_legacy_and_corrupt_records_do_not_invent_authority() {
    let store = store();
    let buffer = store.open().await.unwrap();
    buffer.add_relation(hash(1), hash(3)).unwrap();
    assert!(buffer.pending_request_policy(&hash(3)).unwrap().is_none());
    let key = store.encode_key(&hash(3)).unwrap();
    let mut corrupt = pending_policy().encode().unwrap();
    corrupt[..2].copy_from_slice(&2u16.to_le_bytes());
    store.pending.put_one(key.clone(), corrupt).unwrap();
    assert!(buffer.pending_request_policy(&hash(3)).is_err());
    store
        .pending
        .put_one(key, pending_policy().encode().unwrap())
        .unwrap();
    store.parents.delete(vec![hash(3)]).unwrap();
    assert!(buffer.pending_request_policy(&hash(3)).is_err());
}

#[tokio::test]
async fn pending_policy_lmdb_crash_keeps_row_and_policy_atomic() {
    const DIRECTORY: &str = "F1R3_PENDING_POLICY_CRASH_DIRECTORY";
    const AFTER: &str = "F1R3_PENDING_POLICY_CRASH_AFTER_COMMIT";
    if let Some(directory) = std::env::var_os(DIRECTORY) {
        let mut store = lmdb_store(std::path::Path::new(&directory));
        store.parents = KeyValueTypedStoreImpl::new(Arc::new(FaultStore {
            inner: store.parents.raw_store().clone(),
            fail_commit: Arc::new(AtomicBool::new(false)),
            exit_after_commit: Some(std::env::var(AFTER).unwrap() == "true"),
            supports_atomic: true,
            read_pause: None,
        }));
        let buffer = store.open().await.unwrap();
        buffer
            .publish_pending_request(
                hash(3),
                HashSet::from([hash(2)]),
                HashSet::from([hash(9)]),
                None,
                &pending_policy(),
            )
            .unwrap();
        panic!("child did not reach the atomic pending publication boundary");
    }
    for after_commit in [false, true] {
        let directory = tempfile::Builder::new()
            .prefix("pr216-pending-policy-crash-")
            .tempdir()
            .unwrap();
        let store = lmdb_store(directory.path());
        let buffer = store.open().await.unwrap();
        buffer.add_relation(hash(1), hash(3)).unwrap();
        buffer
            .publish_pending_request(
                hash(4),
                HashSet::new(),
                HashSet::new(),
                None,
                &pending_policy(),
            )
            .unwrap();
        let mut expected = store.to_map().unwrap();
        drop(buffer);
        drop(store);
        let test = format!(
            "{}::pending_policy_lmdb_crash_keeps_row_and_policy_atomic",
            module_path!()
        );
        let test = test.strip_prefix("block_storage::").unwrap_or(&test);
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(DIRECTORY, directory.path())
            .env(AFTER, after_commit.to_string())
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(77),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reopened = lmdb_store(directory.path());
        let restored = reopened.open().await.unwrap();
        if after_commit {
            expected.get_mut(&hash(3)).unwrap().extend([
                hash(2),
                CasperBufferKeyValueStorage::certificate_dependency_key(&hash(9)).unwrap(),
            ]);
        }
        assert_projection(&restored, &expected);
        assert_eq!(
            restored.pending_request_policy(&hash(3)).unwrap(),
            after_commit.then(pending_policy)
        );
        assert_eq!(
            restored.pending_request_policy(&hash(4)).unwrap(),
            Some(pending_policy())
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn pending_policy_histories_preserve_atomic_ownership(
        operations in prop::collection::vec((0u8..5, any::<bool>()), 1..65),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let (store, failure) = fault_store();
            let mut buffer = store.open().await.unwrap();
            let mut expected: Option<PendingRequestPolicy> = None;
            for (operation, fails) in operations {
                if operation == 4 {
                    buffer = store.open().await.unwrap();
                } else {
                    failure.store(fails, Ordering::SeqCst);
                    let mut next = expected.clone().unwrap_or_else(pending_policy);
                    if expected.is_some() {
                        next.advance_revision().unwrap();
                        next.retry_attempts += 1;
                    }
                    next.requested_as_dependency |= operation == 1;
                    let result = if operation == 3 {
                        buffer.remove(hash(3))
                    } else {
                        buffer.publish_pending_request(hash(3), HashSet::from([hash(1)]), HashSet::new(), expected.as_ref(), &next)
                    };
                    prop_assert_eq!(result.is_err(), fails);
                    if result.is_ok() {
                        expected = if operation == 3 { None } else { Some(next) };
                    }
                }
                prop_assert_eq!(buffer.pending_request_policy(&hash(3)).unwrap(), expected.clone());
                prop_assert_eq!(buffer.contains_durable_row(&hash(3)).unwrap(), expected.is_some());
                let rows = if expected.is_some() {
                    HashMap::from([(hash(3), HashSet::from([hash(1)]))])
                } else { HashMap::new() };
                assert_projection(&buffer, &rows);
            }
            Ok(())
        })?;
    }
}

#[tokio::test]
async fn startup_snapshot_preserves_hashes_across_mutation_and_certificate_resolution() {
    let buffer = store().open().await.unwrap();
    buffer.put_pendant(hash(1)).unwrap();
    buffer.put_pendant(hash(2)).unwrap();
    buffer.add_certificate_relation(hash(9), hash(3)).unwrap();
    let snapshot = buffer.snapshot_pendant_candidates();
    assert_eq!(snapshot.original_len(), 3);
    buffer.remove(hash(1)).unwrap();
    buffer.put_pendant(hash(1)).unwrap();
    buffer.add_relation(hash(4), hash(2)).unwrap();
    buffer.resolve_certificate_dependency(hash(9)).unwrap();
    let mut certificate_keys = 0;
    let mut block_keys = Vec::new();
    for key in snapshot {
        if CasperBufferKeyValueStorage::certificate_digest_from_dependency_key(&key).is_some() {
            certificate_keys += 1;
        } else {
            block_keys.push(key);
        }
    }
    assert_eq!(block_keys, vec![hash(1), hash(2)]);
    assert_eq!(certificate_keys, 1);
    assert_eq!(
        buffer.get_pendants(),
        HashSet::from([hash(1), hash(3), hash(4)])
    );
}

#[tokio::test]
async fn durable_put_pendant_survives_restart_without_synthetic_parent() {
    let store = store();
    let buffer = store.open().await.unwrap();
    buffer.put_pendant(hash(1)).unwrap();
    drop(buffer);
    let restored = store.open().await.unwrap();
    assert_projection(&restored, &HashMap::from([(hash(1), HashSet::new())]));
}

#[tokio::test]
async fn durable_last_dependency_resolution_preserves_ready_child() {
    let store = store();
    let buffer = store.open().await.unwrap();
    buffer.add_relation(hash(1), hash(2)).unwrap();
    buffer.remove(hash(1)).unwrap();
    assert_projection(&buffer, &HashMap::from([(hash(2), HashSet::new())]));
    drop(buffer);
    let restored = store.open().await.unwrap();
    assert_projection(&restored, &HashMap::from([(hash(2), HashSet::new())]));
}

#[tokio::test]
async fn durable_orphan_parent_keeps_its_own_dependency_row() {
    let store = store();
    let buffer = store.open().await.unwrap();
    buffer.add_relation(hash(1), hash(2)).unwrap();
    buffer.add_relation(hash(2), hash(3)).unwrap();
    buffer.remove(hash(3)).unwrap();
    let expected = HashMap::from([(hash(2), HashSet::from([hash(1)]))]);
    assert_projection(&buffer, &expected);
    drop(buffer);
    let restored = store.open().await.unwrap();
    assert_projection(&restored, &expected);
}

#[tokio::test]
async fn durable_explicit_parent_survives_its_last_child_removal() {
    let store = store();
    let buffer = store.open().await.unwrap();
    buffer.put_pendant(hash(1)).unwrap();
    buffer.add_relation(hash(1), hash(2)).unwrap();
    buffer.remove(hash(2)).unwrap();
    assert_projection(&buffer, &HashMap::from([(hash(1), HashSet::new())]));
    drop(buffer);
    let restored = store.open().await.unwrap();
    assert_projection(&restored, &HashMap::from([(hash(1), HashSet::new())]));
}

#[tokio::test]
async fn durable_certificate_resolution_preserves_child_across_restart() {
    let store = store();
    let buffer = store.open().await.unwrap();
    buffer.add_certificate_relation(hash(1), hash(2)).unwrap();
    assert!(buffer.get_pendants().is_empty());
    buffer.resolve_certificate_dependency(hash(1)).unwrap();
    drop(buffer);
    let restored = store.open().await.unwrap();
    assert_projection(&restored, &HashMap::from([(hash(2), HashSet::new())]));
    assert!(restored.get_missing_certificate_dependencies().is_empty());
}

#[tokio::test]
async fn durable_put_pendant_does_not_erase_existing_dependencies() {
    let store = store();
    let buffer = store.open().await.unwrap();
    buffer.add_relation(hash(1), hash(2)).unwrap();
    buffer.put_pendant(hash(2)).unwrap();
    assert_projection(
        &buffer,
        &HashMap::from([(hash(2), HashSet::from([hash(1)]))]),
    );
}

#[derive(Clone)]
struct FaultStore {
    inner: Arc<dyn KeyValueStore>,
    fail_commit: Arc<AtomicBool>,
    exit_after_commit: Option<bool>,
    supports_atomic: bool,
    read_pause: Option<Arc<ReadPause>>,
}

struct ReadPause {
    armed: AtomicBool,
    captured: std::sync::Barrier,
    resume: std::sync::Barrier,
}

impl KeyValueStore for FaultStore {
    fn as_any(&self) -> &dyn Any { self }
    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        self.inner.get(keys)
    }
    fn with_value(
        &self,
        key: &ByteBuffer,
        reader: &mut ValueReader<'_>,
    ) -> Result<(), KvStoreError> {
        self.inner.with_value(key, reader)?;
        if let Some(pause) = &self.read_pause {
            if pause.armed.swap(false, Ordering::SeqCst) {
                pause.captured.wait();
                pause.resume.wait();
            }
        }
        Ok(())
    }
    fn visit_entries(&self, reader: &mut EntryReader<'_>) -> Result<(), KvStoreError> {
        self.inner.visit_entries(reader)
    }
    fn put(&self, pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        self.inner.put(pairs)
    }
    fn put_one_if_absent(&self, key: ByteBuffer, value: ByteBuffer) -> Result<bool, KvStoreError> {
        self.inner.put_one_if_absent(key, value)
    }
    fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        self.inner.delete(keys)
    }
    fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
        self.inner.iterate(f)
    }
    fn iterate_while(
        &self,
        f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        self.inner.iterate_while(f)
    }
    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }
    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        self.inner.to_map()
    }
    fn print_store(&self) -> Result<(), KvStoreError> { self.inner.print_store() }
    fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }
    fn size_bytes(&self) -> usize { self.inner.size_bytes() }

    fn strict_atomic_mutate(
        &self,
        mutations: &[AtomicStoreMutation<'_>],
    ) -> Result<(), KvStoreError> {
        if !self.supports_atomic {
            return Err(KvStoreError::AtomicityUnavailable(
                "injected unsupported backend".to_string(),
            ));
        }
        if self.exit_after_commit == Some(false) {
            std::process::exit(77);
        }
        let mut inner = mutations
            .iter()
            .map(|mutation| AtomicStoreMutation {
                store: if std::ptr::eq(mutation.store.as_any(), self.as_any()) {
                    self.inner.as_ref()
                } else {
                    mutation.store
                },
                key: mutation.key.clone(),
                operation: mutation.operation.clone(),
            })
            .collect::<Vec<_>>();
        if self.fail_commit.swap(false, Ordering::SeqCst) {
            inner.push(AtomicStoreMutation {
                store: self.inner.as_ref(),
                key: b"buffer-injected-late-conflict".to_vec(),
                operation: AtomicStoreOperation::CompareAndSwap {
                    expected: Some(vec![255]),
                    replacement: None,
                },
            });
        }
        let result = self.inner.strict_atomic_mutate(&inner);
        if result.is_ok() && self.exit_after_commit == Some(true) {
            std::process::exit(77);
        }
        result
    }
}

fn fault_store() -> (TestStores, Arc<AtomicBool>) {
    let fail_commit = Arc::new(AtomicBool::new(false));
    let mut stores = store();
    stores.parents = KeyValueTypedStoreImpl::new(Arc::new(FaultStore {
        inner: stores.parents.raw_store().clone(),
        fail_commit: fail_commit.clone(),
        exit_after_commit: None,
        supports_atomic: true,
        read_pause: None,
    }));
    (stores, fail_commit)
}

#[tokio::test]
async fn publication_is_atomic_for_mixed_dependencies_and_preserves_existing_rows() {
    let (store, failure) = fault_store();
    let buffer = store.open().await.unwrap();
    buffer
        .add_dependencies(hash(3), HashSet::from([hash(1)]), HashSet::new())
        .unwrap();
    let before = store.to_map().unwrap();
    failure.store(true, Ordering::SeqCst);
    assert!(buffer
        .add_dependencies(hash(3), HashSet::from([hash(2)]), HashSet::from([hash(9)]))
        .is_err());
    assert_projection(&buffer, &before);
    buffer
        .add_dependencies(hash(3), HashSet::from([hash(2)]), HashSet::from([hash(9)]))
        .unwrap();
    let expected = HashMap::from([(
        hash(3),
        HashSet::from([
            hash(1),
            hash(2),
            CasperBufferKeyValueStorage::certificate_dependency_key(&hash(9)).unwrap(),
        ]),
    )]);
    assert_projection(&buffer, &expected);
    let restored = store.open().await.unwrap();
    assert_projection(&restored, &expected);
}

#[tokio::test]
async fn publication_distinguishes_empty_rows_from_implicit_parents() {
    let durable = store();
    let buffer = durable.open().await.unwrap();
    buffer
        .add_dependencies(hash(3), HashSet::from([hash(1)]), HashSet::new())
        .unwrap();
    assert!(buffer.is_pendant(&hash(1)));
    assert!(!buffer.contains_durable_row(&hash(1)).unwrap());
    buffer
        .add_dependencies(hash(1), HashSet::new(), HashSet::new())
        .unwrap();
    assert!(buffer.contains_durable_row(&hash(1)).unwrap());
    assert!(!buffer.contains(&hash(1)));
    buffer
        .add_dependencies(hash(3), HashSet::new(), HashSet::new())
        .unwrap();
    assert_projection(
        &buffer,
        &HashMap::from([
            (hash(1), HashSet::new()),
            (hash(3), HashSet::from([hash(1)])),
        ]),
    );
    let restored = durable.open().await.unwrap();
    assert!(restored.contains_durable_row(&hash(1)).unwrap());
}

#[tokio::test]
async fn publication_rejects_malformed_certificate_before_any_write() {
    let durable = store();
    let buffer = durable.open().await.unwrap();
    assert!(buffer
        .add_dependencies(
            hash(3),
            HashSet::from([hash(1)]),
            HashSet::from([BlockHashSerde(vec![9; 31].into())])
        )
        .is_err());
    assert_projection(&buffer, &HashMap::new());
}

#[tokio::test]
async fn publication_concurrent_unions_preserve_each_submitted_dependency() {
    let durable = store();
    let buffer = durable.open().await.unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(3));
    std::thread::scope(|scope| {
        for first in [1, 2] {
            let buffer = &buffer;
            let barrier = &barrier;
            scope.spawn(move || {
                barrier.wait();
                buffer
                    .add_dependencies(
                        hash(3),
                        HashSet::from([hash(first)]),
                        HashSet::from([hash(first + 5)]),
                    )
                    .unwrap();
            });
        }
        barrier.wait();
    });
    let expected = HashMap::from([(
        hash(3),
        HashSet::from([
            hash(1),
            hash(2),
            CasperBufferKeyValueStorage::certificate_dependency_key(&hash(6)).unwrap(),
            CasperBufferKeyValueStorage::certificate_dependency_key(&hash(7)).unwrap(),
        ]),
    )]);
    assert_projection(&buffer, &expected);
    let restored = durable.open().await.unwrap();
    assert_projection(&restored, &expected);
}

#[tokio::test]
async fn publication_and_resolution_orders_keep_conservative_edges_reconcilable() {
    for resolve_first in [false, true] {
        let durable = store();
        let buffer = durable.open().await.unwrap();
        buffer
            .add_dependencies(hash(3), HashSet::from([hash(1)]), HashSet::from([hash(9)]))
            .unwrap();
        if resolve_first {
            buffer.remove(hash(1)).unwrap();
            buffer.resolve_certificate_dependency(hash(9)).unwrap();
        }
        buffer
            .add_dependencies(hash(3), HashSet::from([hash(1)]), HashSet::from([hash(9)]))
            .unwrap();
        assert!(buffer.is_waiting_on_certificate(&hash(3)));
        buffer.remove(hash(1)).unwrap();
        buffer.resolve_certificate_dependency(hash(9)).unwrap();
        assert_projection(&buffer, &HashMap::from([(hash(3), HashSet::new())]));
        let restored = durable.open().await.unwrap();
        assert!(restored.is_pendant(&hash(3)));
        assert!(restored.contains_durable_row(&hash(3)).unwrap());
    }
}

#[tokio::test]
async fn durable_failed_removal_preserves_rows_memory_and_timestamps() {
    let (store, failure) = fault_store();
    let buffer = store.open().await.unwrap();
    buffer.add_relation(hash(1), hash(2)).unwrap();
    buffer.add_relation(hash(1), hash(3)).unwrap();
    let before = store.to_map().unwrap();
    let times: HashMap<_, _> = buffer
        .first_seen_ms
        .iter()
        .map(|entry| (entry.key().clone(), *entry.value()))
        .collect();
    failure.store(true, Ordering::SeqCst);
    assert!(matches!(
        buffer.remove(hash(1)),
        Err(KvStoreError::TransactionConflict(_))
    ));
    assert_projection(&buffer, &before);
    let after_times: HashMap<_, _> = buffer
        .first_seen_ms
        .iter()
        .map(|entry| (entry.key().clone(), *entry.value()))
        .collect();
    assert_eq!(times, after_times);
    drop(buffer);
    let restored = store.open().await.unwrap();
    assert_projection(&restored, &before);
    restored.remove(hash(1)).unwrap();
    restored.remove(hash(1)).unwrap();
    assert_projection(
        &restored,
        &HashMap::from([(hash(2), HashSet::new()), (hash(3), HashSet::new())]),
    );
}

proptest! {
    #[test]
    fn operation_histories_match_durable_projection(
        operations in proptest::collection::vec((0u8..6, 0u8..8, 0u8..8, any::<bool>()), 0..200)
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let (store, failure) = fault_store();
            let mut buffer = store.open().await.unwrap();
            let mut expected: Rows = HashMap::new();
            for (operation, first, second, fails) in operations {
                let key = hash(first);
                let child = hash(second);
                if operation == 4 {
                    drop(buffer);
                    buffer = store.open().await.unwrap();
                } else {
                    failure.store(fails, Ordering::SeqCst);
                    let result = match operation {
                        0 => buffer.put_pendant(key.clone()),
                        1 => buffer.add_relation(key.clone(), child.clone()),
                        2 => buffer.remove(key.clone()),
                        5 => buffer.add_dependencies(child.clone(), HashSet::from([key.clone()]), HashSet::from([key.clone()])),
                        _ => buffer.add_certificate_relation(key.clone(), child.clone()),
                    };
                    let unused_failure = failure.swap(false, Ordering::SeqCst);
                    if fails && !unused_failure {
                        assert!(matches!(result, Err(KvStoreError::TransactionConflict(_))));
                    } else {
                        result.unwrap();
                        match operation {
                            0 => { expected.entry(key).or_default(); }
                            1 | 3 | 5 => {
                                let parent = if operation == 3 {
                                    CasperBufferKeyValueStorage::certificate_dependency_key(&key).unwrap()
                                } else { key };
                                if operation == 5 {
                                    expected.entry(child.clone()).or_default().insert(
                                        CasperBufferKeyValueStorage::certificate_dependency_key(&parent).unwrap()
                                    );
                                }
                                expected.entry(child).or_default().insert(parent);
                            }
                            _ => {
                                expected.remove(&key);
                                for parents in expected.values_mut() { parents.remove(&key); }
                            }
                        }
                    }
                }
                assert_projection(&buffer, &expected);
            }
        });
    }
}

#[tokio::test]
async fn concurrent_mutations_publish_only_complete_durable_projections() {
    let store = store();
    let buffer = store.open().await.unwrap();
    std::thread::scope(|scope| {
        for offset in 0..2 {
            let buffer = &buffer;
            scope.spawn(move || {
                for round in 0..64 {
                    let key = hash((round + offset) % 8);
                    buffer.put_pendant(key.clone()).unwrap();
                    buffer
                        .add_relation(key.clone(), hash((round + 1) % 8))
                        .unwrap();
                    buffer.remove(key).unwrap();
                }
            });
        }
        for _ in 0..128 {
            let expected = {
                let _guard = buffer.read_guard();
                let rows = store.to_map().unwrap();
                let dag = buffer.block_dependency_dag.lock().unwrap();
                for (key, parents) in &rows {
                    if parents.is_empty() {
                        assert!(dag.dependency_free.contains(key));
                    } else {
                        assert_eq!(dag.child_to_parent_adjacency_list.get(key), Some(parents));
                    }
                }
                rows
            };
            assert!(expected.len() <= 8);
        }
    });
    let expected = store.to_map().unwrap();
    assert_projection(&buffer, &expected);
    drop(buffer);
    let restored = store.open().await.unwrap();
    assert_projection(&restored, &expected);
}

fn lmdb_store(directory: &std::path::Path) -> TestStores {
    let mut options = heed::EnvOpenOptions::new();
    options.map_size(10 * 1024 * 1024).max_dbs(2);
    let env = Arc::new(unsafe { options.open(directory).unwrap() });
    let (database, pending) = {
        let mut transaction = env.write_txn().unwrap();
        let database = env
            .create_database(&mut transaction, Some("parents-map"))
            .unwrap();
        let pending = env
            .create_database(
                &mut transaction,
                Some(CasperBufferKeyValueStorage::PENDING_POLICY_NAMESPACE),
            )
            .unwrap();
        transaction.commit().unwrap();
        (database, pending)
    };
    TestStores {
        parents: KeyValueTypedStoreImpl::new(Arc::new(
            shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore::new(
                env.clone(),
                database,
            ),
        )),
        pending: Arc::new(
            shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore::new(env, pending),
        ),
    }
}

#[tokio::test]
async fn retention_restored_pendants_keep_a_stable_age_epoch() {
    let store = store();
    store.put_one(hash(1), HashSet::new()).unwrap();
    let buffer = store.open().await.unwrap();
    let first = buffer
        .first_seen_ms
        .get(&hash(1))
        .map(|seen| *seen)
        .unwrap_or(1000);
    assert_eq!(
        buffer.dependency_free_nodes_with_age_ms(first + 1000),
        vec![(1000, hash(1))]
    );
    assert_eq!(
        buffer.dependency_free_nodes_with_age_ms(first + 2000),
        vec![(2000, hash(1))]
    );
}

#[tokio::test]
async fn retention_isolated_pendant_contributes_to_pressure_pruning() {
    let store = store();
    store.put_one(hash(1), HashSet::new()).unwrap();
    let buffer = store.open().await.unwrap();
    assert_eq!(buffer.size(), 0);
    assert_eq!(buffer.approx_node_count(), 1);
    assert_eq!(buffer.enforce_limits(0, u64::MAX, 1, 0).unwrap(), (0, 1));
    assert_eq!(buffer.approx_node_count(), 0);
    assert!(store.to_map().unwrap().is_empty());
}

#[tokio::test]
async fn unsupported_backend_cannot_publish_buffer_mutations() {
    let mut store = store();
    store.parents = KeyValueTypedStoreImpl::new(Arc::new(FaultStore {
        inner: store.parents.raw_store().clone(),
        fail_commit: Arc::new(AtomicBool::new(false)),
        exit_after_commit: None,
        supports_atomic: false,
        read_pause: None,
    }));
    store.put_one(hash(2), HashSet::from([hash(1)])).unwrap();
    let expected = store.to_map().unwrap();
    let buffer = store.open().await.unwrap();
    let before_times: HashMap<_, _> = buffer
        .first_seen_ms
        .iter()
        .map(|entry| (entry.key().clone(), *entry.value()))
        .collect();
    for result in [
        buffer.put_pendant(hash(3)),
        buffer.add_relation(hash(3), hash(2)),
        buffer.remove(hash(1)),
    ] {
        assert!(matches!(result, Err(KvStoreError::AtomicityUnavailable(_))));
        assert_projection(&buffer, &expected);
    }
    let after_times: HashMap<_, _> = buffer
        .first_seen_ms
        .iter()
        .map(|entry| (entry.key().clone(), *entry.value()))
        .collect();
    assert_eq!(before_times, after_times);
}

#[tokio::test]
async fn failed_mixed_certificate_resolution_keeps_block_obligation_after_retry() {
    let (store, failure) = fault_store();
    let buffer = store.open().await.unwrap();
    buffer.add_relation(hash(1), hash(2)).unwrap();
    buffer.add_certificate_relation(hash(3), hash(2)).unwrap();
    let before = store.to_map().unwrap();
    failure.store(true, Ordering::SeqCst);
    assert!(matches!(
        buffer.resolve_certificate_dependency(hash(3)),
        Err(KvStoreError::TransactionConflict(_))
    ));
    assert_projection(&buffer, &before);
    buffer.resolve_certificate_dependency(hash(3)).unwrap();
    drop(buffer);
    let restored = store.open().await.unwrap();
    assert_projection(
        &restored,
        &HashMap::from([(hash(2), HashSet::from([hash(1)]))]),
    );
    assert!(!restored.is_pendant(&hash(2)));
    assert!(!restored.requested_as_certificate_dependency(&hash(3)));
}

#[tokio::test]
async fn pruning_the_last_child_preserves_its_explicit_parent() {
    let store = store();
    let buffer = store.open().await.unwrap();
    buffer.put_pendant(hash(1)).unwrap();
    buffer.add_relation(hash(1), hash(2)).unwrap();
    assert_eq!(buffer.enforce_limits(usize::MAX, 0, 1, 0).unwrap(), (1, 0));
    assert_projection(&buffer, &HashMap::from([(hash(1), HashSet::new())]));
}

#[tokio::test]
async fn scan_rotation_preserves_order_across_capacity_exits_and_duplicate_dependencies() {
    let store = store();
    let buffer = store.open().await.unwrap();
    buffer.put_pendant(hash(1)).unwrap();
    buffer.put_pendant(hash(2)).unwrap();
    assert_eq!(buffer.scan_candidate_count(), 2);
    assert_eq!(buffer.next_scan_candidate(), Some(hash(1)));
    buffer.put_pendant(hash(1)).unwrap();
    buffer.add_certificate_relation(hash(9), hash(2)).unwrap();
    assert_eq!(buffer.scan_candidate_count(), 2);
    assert_eq!(buffer.next_scan_candidate(), Some(hash(2)));
    buffer.resolve_certificate_dependency(hash(9)).unwrap();
    assert_eq!(buffer.next_scan_candidate(), Some(hash(1)));
    assert_eq!(buffer.next_scan_candidate(), Some(hash(2)));
}

#[tokio::test]
async fn scan_namespace_accepts_ff_block_hash_but_excludes_certificate_keys() {
    let store = store();
    let buffer = store.open().await.unwrap();
    buffer.put_pendant(hash(255)).unwrap();
    buffer.add_certificate_relation(hash(255), hash(2)).unwrap();
    assert_eq!(buffer.scan_candidate_count(), 2);
    assert_eq!(buffer.next_scan_candidate(), Some(hash(255)));
    assert_eq!(buffer.next_scan_candidate(), Some(hash(2)));
    assert_eq!(buffer.approx_node_count(), 3);
}

#[tokio::test]
async fn durable_lmdb_restart_at_atomic_publication_boundary() {
    const DIRECTORY: &str = "F1R3_BUFFER_CRASH_DIRECTORY";
    const AFTER: &str = "F1R3_BUFFER_CRASH_AFTER_COMMIT";
    if let Some(directory) = std::env::var_os(DIRECTORY) {
        let mut store = lmdb_store(std::path::Path::new(&directory));
        store.parents = KeyValueTypedStoreImpl::new(Arc::new(FaultStore {
            inner: store.parents.raw_store().clone(),
            fail_commit: Arc::new(AtomicBool::new(false)),
            exit_after_commit: Some(std::env::var(AFTER).unwrap() == "true"),
            supports_atomic: true,
            read_pause: None,
        }));
        let buffer = store.open().await.unwrap();
        buffer.remove(hash(1)).unwrap();
        panic!("child returned without reaching its transaction exit boundary");
    }
    for after_commit in [false, true] {
        let directory = tempfile::Builder::new()
            .prefix("pr216-buffer-crash-")
            .tempdir()
            .unwrap();
        let store = lmdb_store(directory.path());
        let buffer = store.open().await.unwrap();
        buffer.put_pendant(hash(1)).unwrap();
        buffer.put_pendant(hash(4)).unwrap();
        buffer.add_relation(hash(1), hash(2)).unwrap();
        buffer.add_relation(hash(1), hash(3)).unwrap();
        buffer.add_relation(hash(4), hash(3)).unwrap();
        let mut expected = store.to_map().unwrap();
        drop(buffer);
        drop(store);
        let test = format!(
            "{}::durable_lmdb_restart_at_atomic_publication_boundary",
            module_path!()
        );
        let test = test.strip_prefix("block_storage::").unwrap_or(&test);
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture", "--test-threads=1"])
            .env(DIRECTORY, directory.path())
            .env(AFTER, after_commit.to_string())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(77));
        if after_commit {
            expected.remove(&hash(1));
            for parents in expected.values_mut() {
                parents.remove(&hash(1));
            }
        }
        let reopened = lmdb_store(directory.path());
        let restored = reopened.open().await.unwrap();
        assert_projection(&restored, &expected);
    }
}
