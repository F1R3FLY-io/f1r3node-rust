use std::collections::HashSet;

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use models::rust::block_hash::{BlockHashSerde, LENGTH};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

async fn assert_missing_parent_preserved(limits: Option<(usize, u64, usize, u64)>) {
    let mut manager = InMemoryStoreManager::new();
    let store = manager.store("parents-map".to_string()).await.unwrap();
    let durable = KeyValueTypedStoreImpl::new(store);
    let pending = manager
        .store(CasperBufferKeyValueStorage::PENDING_POLICY_NAMESPACE.into())
        .await
        .unwrap();
    let buffer = CasperBufferKeyValueStorage::new_from_kv_store(durable.clone(), pending.clone())
        .await
        .unwrap();
    let parent = BlockHashSerde(vec![1; LENGTH].into());
    let child = BlockHashSerde(vec![2; LENGTH].into());
    let expected = Some(HashSet::from([parent.clone()]));

    buffer.add_relation(parent.clone(), child.clone()).unwrap();
    assert_eq!(durable.get_one(&child).unwrap(), expected);
    assert_eq!(buffer.get_parents(&child), expected);
    assert!(buffer.requested_as_dependency(&parent));

    if let Some((capacity, ttl, batch, interval)) = limits {
        let (aged, pressure) = buffer
            .enforce_limits(capacity, ttl, batch, interval)
            .unwrap();
        assert_eq!(aged + pressure, 1, "the fixture must exercise pruning");
    }

    let persisted = durable.get_one(&child).unwrap();
    let current = (
        buffer.get_parents(&child),
        buffer.requested_as_dependency(&parent),
    );
    drop(buffer);
    let reopened = CasperBufferKeyValueStorage::new_from_kv_store(durable, pending)
        .await
        .unwrap();
    let restored = (
        reopened.get_parents(&child),
        reopened.requested_as_dependency(&parent),
    );

    assert_eq!(
        (persisted, current, restored),
        (expected.clone(), (expected.clone(), true), (expected, true),),
        "pruning must preserve the unresolved durable row and both dependency projections"
    );
}

#[tokio::test]
async fn unpruned_missing_parent_survives_reopen() { assert_missing_parent_preserved(None).await; }

#[tokio::test]
async fn age_pruning_preserves_missing_parent_across_reopen() {
    assert_missing_parent_preserved(Some((usize::MAX, 0, 1, 0))).await;
}

#[tokio::test]
async fn pressure_pruning_preserves_missing_parent_across_reopen() {
    assert_missing_parent_preserved(Some((0, u64::MAX, 1, 0))).await;
}
