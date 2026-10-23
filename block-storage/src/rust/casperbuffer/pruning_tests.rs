use models::rust::block_hash::BlockHashSerde;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use shared::rust::store::key_value_store::KvStoreError;

use super::CasperBufferKeyValueStorage;

fn hash(value: u64) -> BlockHashSerde { BlockHashSerde(value.to_be_bytes().to_vec().into()) }

async fn buffer() -> Result<CasperBufferKeyValueStorage, KvStoreError> {
    CasperBufferKeyValueStorage::new_from_kvm(&mut InMemoryStoreManager::new()).await
}

#[tokio::test]
async fn pruning_drains_a_missing_dependency_chain_without_peer_replies() -> Result<(), KvStoreError>
{
    let buffer = buffer().await?;
    buffer.add_relation(hash(0), hash(1))?;
    buffer.add_relation(hash(1), hash(2))?;

    for _ in 0..3 {
        buffer.enforce_limits(0, u64::MAX, 1, 0)?;
    }

    assert!(buffer.to_doubly_linked_dag().dependency_free.is_empty());
    assert!(buffer.take_released().is_empty());
    Ok(())
}

#[tokio::test]
async fn pruning_release_preserves_the_original_age() -> Result<(), KvStoreError> {
    let buffer = buffer().await?;
    buffer.add_relation(hash(0), hash(1))?;
    buffer.first_seen_ms.insert(hash(1), 42);

    buffer.remove(hash(0))?;

    assert_eq!(buffer.first_seen_ms.get(&hash(1)).map(|age| *age), Some(42));
    assert_eq!(buffer.take_released(), vec![hash(1)]);
    Ok(())
}

#[tokio::test]
async fn pruning_dependency_cache_follows_the_buffer_lifecycle() -> Result<(), KvStoreError> {
    let buffer = buffer().await?;
    buffer.cache_block_dependencies(hash(9), vec![]);
    assert!(buffer.cached_block_dependencies(&hash(9)).is_none());

    buffer.add_relation(hash(0), hash(1))?;
    buffer.cache_block_dependencies(hash(0), vec![]);
    buffer.cache_block_dependencies(hash(1), vec![hash(0).0]);
    assert!(buffer.uncached_block_hashes().is_empty());

    buffer.remove(hash(0))?;
    assert!(buffer.cached_block_dependencies(&hash(0)).is_none());
    assert_eq!(
        buffer.cached_block_dependencies(&hash(1)).as_deref(),
        Some(&vec![hash(0).0])
    );

    buffer.enforce_limits_with_readiness(0, u64::MAX, 1, 0, |_| false)?;
    assert!(buffer.cached_block_dependencies(&hash(1)).is_none());
    buffer.cache_block_dependencies(hash(1), vec![]);
    assert!(buffer.block_dependencies.is_empty());
    Ok(())
}

#[tokio::test]
async fn pruning_restored_hashes_require_dependency_hydration() -> Result<(), KvStoreError> {
    let buffer = buffer().await?;
    buffer.add_relation(hash(0), hash(1))?;
    buffer.cache_block_dependencies(hash(1), vec![hash(0).0]);
    let restored =
        CasperBufferKeyValueStorage::new_from_kv_store(buffer.parents_store.clone()).await?;
    let uncached: std::collections::HashSet<_> =
        restored.uncached_block_hashes().into_iter().collect();
    assert_eq!(uncached, [hash(0), hash(1)].into_iter().collect());
    assert!(restored.cached_block_dependencies(&hash(1)).is_none());
    Ok(())
}
