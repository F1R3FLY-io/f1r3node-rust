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
