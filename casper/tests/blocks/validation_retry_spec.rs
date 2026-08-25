// Bounded validation-error retry — pinned to CI run 32588262605 (arm64-docker
// session 2724a5cf, joiner3): five buffered blocks each re-harvested ~2,770
// times on the same hard estimator-walk error ("weight_from_validator_by_dag:
// main-parent metadata missing"), because a validation `Err` acked but never
// purged, and nothing excluded the still-buffered block from the next
// dependency-free harvest.
//
//   validation_failures_are_bounded_and_end_in_purge — every failure below
//   the cap quarantines the hash (pacing the harvest); the cap'th failure
//   demands purge, and acting on it empties the buffer.
//
//   a_success_clears_the_failure_ledger — a settled verdict resets both the
//   attempt count and the quarantine.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use casper::rust::blocks::block_processor::{
    validation_error_attempts_max, BlockProcessorDependencies, ValidationFailureDisposition,
};
use casper::rust::engine::block_retriever::BlockRetriever;
use comm::rust::rp::connect::{Connections, ConnectionsCell};
use comm::rust::test_instances::{create_rp_conf_ask, TransportLayerStub};
use models::rust::block_hash::BlockHashSerde;
use models::rust::block_implicits::get_random_block_default;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

use crate::engine::setup;
use crate::helper::block_dag_storage_fixture::with_storage;

async fn dependencies() -> BlockProcessorDependencies<TransportLayerStub> {
    let local_peer = setup::peer_node("test-peer", 40400);
    let connections_cell = ConnectionsCell {
        peers: Arc::new(Mutex::new(Connections::from_vec(vec![local_peer.clone()]))),
    };
    let rp_conf = create_rp_conf_ask(local_peer.clone(), None, None);
    let transport = Arc::new(TransportLayerStub::new());
    let retriever_connections = ConnectionsCell {
        peers: Arc::new(Mutex::new(Connections::from_vec(vec![local_peer.clone()]))),
    };
    let block_retriever = BlockRetriever::new(
        Arc::new(Mutex::new(HashMap::new())),
        transport.clone(),
        retriever_connections,
        rp_conf.clone(),
    );

    let (block_store, _indexed_dag_storage, casper_buffer) = with_storage(|bs, ids| async move {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.store("parents-map".to_string()).await.unwrap();
        let typed_store = KeyValueTypedStoreImpl::new(store);
        let cb = CasperBufferKeyValueStorage::new_from_kv_store(typed_store)
            .await
            .unwrap();
        (bs, ids, cb)
    })
    .await;

    let mut dag_kvm = InMemoryStoreManager::new();
    let dag_storage = BlockDagKeyValueStorage::new(&mut dag_kvm).await.unwrap();

    BlockProcessorDependencies::new(
        block_store,
        casper_buffer,
        dag_storage,
        block_retriever,
        transport,
        connections_cell,
        rp_conf,
        None,
    )
}

#[tokio::test]
async fn validation_failures_are_bounded_and_end_in_purge() {
    let deps = dependencies().await;
    let block = get_random_block_default();
    deps.commit_to_buffer(&block, None).await.unwrap();
    let serde_hash = BlockHashSerde(block.block_hash.clone());
    assert!(deps.casper_buffer().is_pendant(&serde_hash));

    let max = validation_error_attempts_max();
    for attempt in 1..max {
        assert_eq!(
            deps.note_validation_failure(&block.block_hash).unwrap(),
            ValidationFailureDisposition::Retry,
            "attempt {attempt} of {max} stays retryable",
        );
        assert!(
            deps.is_validation_failure_quarantined(&block.block_hash)
                .unwrap(),
            "a failed attempt must quarantine the hash so the pendant \
             harvest stops hot-looping it",
        );
    }
    assert_eq!(
        deps.note_validation_failure(&block.block_hash).unwrap(),
        ValidationFailureDisposition::PurgeAndQuarantine,
        "the cap'th consecutive failure must end the retry loop",
    );

    deps.remove_from_buffer(&block).await.unwrap();
    deps.ack_processed(&block).await.unwrap();
    assert!(
        !deps.casper_buffer().is_pendant(&serde_hash),
        "after the demanded purge the block is no longer harvestable",
    );
}

#[tokio::test]
async fn a_success_clears_the_failure_ledger() {
    let deps = dependencies().await;
    let block = get_random_block_default();

    deps.note_validation_failure(&block.block_hash).unwrap();
    deps.note_validation_failure(&block.block_hash).unwrap();
    assert!(deps
        .is_validation_failure_quarantined(&block.block_hash)
        .unwrap());

    deps.clear_validation_failures(&block.block_hash).unwrap();
    assert!(
        !deps
            .is_validation_failure_quarantined(&block.block_hash)
            .unwrap(),
        "a settled verdict lifts the quarantine",
    );
    assert_eq!(
        deps.note_validation_failure(&block.block_hash).unwrap(),
        ValidationFailureDisposition::Retry,
        "the attempt ledger restarts after a settled verdict",
    );
}
