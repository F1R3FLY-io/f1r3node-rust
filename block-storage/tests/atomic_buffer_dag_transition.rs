// Tests for the atomic buffer-DAG transition helper (Bug #17 / T-9.20).
//
// Validates the contract laid out in
// `block-storage/src/rust/dag/buffer_dag_transition.rs`:
//
//   atomic_insert_then_buffer:
//     - inserts the block into the DAG store
//     - performs the BufferTransition (RemoveFromBuffer | Skip)
//     - tolerates the buffer hash being already-absent (idempotence)
//
//   reconcile_buffer_against_dag:
//     - purges pendants whose hash is now in the DAG
//     - leaves pendants whose hash is NOT in the DAG intact
//     - returns the count of purged pendants

use std::collections::HashSet;

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::dag::block_dag_key_value_storage::{
    BlockDagKeyValueStorage, CertifiedAdmissionOutcome, CertifiedSenderAuthority, InsertMode,
};
use block_storage::rust::dag::buffer_dag_transition::{
    atomic_insert_then_buffer, reconcile_buffer_against_dag, BufferTransition,
};
use models::rust::block_hash::{self, BlockHashSerde};
use models::rust::block_implicits::get_random_block;
use models::rust::block_metadata::{
    AdmissionRejectionReason, CERTIFIED_ADMISSION_PROTOCOL_VERSION,
};
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::{BlockMessage, FinalizedFloorCommitment};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

fn make_block() -> BlockMessage {
    let mut block = get_random_block(
        Some(1),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(vec![]),
        None,
        None,
        None,
        Some(vec![]),
        None,
        None,
    );
    block.header.version = CERTIFIED_ADMISSION_PROTOCOL_VERSION;
    block.header.sender_bond_generation = Some(BondGeneration::GENESIS);
    block.header.finalized_floor = Some(FinalizedFloorCommitment {
        floor_hash: prost::bytes::Bytes::from(vec![3; block_hash::LENGTH]),
        floor_post_state_hash: block.body.state.pre_state_hash.clone(),
        certificate_digest: prost::bytes::Bytes::from(vec![5; block_hash::LENGTH]),
        authority_context_digest: prost::bytes::Bytes::from(vec![4; block_hash::LENGTH]),
    });
    block
}

fn certificate(block: &BlockMessage) -> CertifiedSenderAuthority {
    CertifiedSenderAuthority::new(
        block,
        block
            .header
            .parents_hash_list
            .first()
            .cloned()
            .unwrap_or_else(|| prost::bytes::Bytes::from(vec![3; block_hash::LENGTH])),
        block.body.state.pre_state_hash.clone(),
        prost::bytes::Bytes::from(vec![4; block_hash::LENGTH]),
        BondGeneration::GENESIS,
        1,
    )
    .unwrap()
}

fn accepted_outcome(block: &BlockMessage) -> CertifiedAdmissionOutcome {
    CertifiedAdmissionOutcome::accepted(block, &certificate(block)).unwrap()
}

fn rejected_outcome(block: &BlockMessage) -> CertifiedAdmissionOutcome {
    CertifiedAdmissionOutcome::rejected(
        block,
        &certificate(block),
        AdmissionRejectionReason::InvalidTransaction,
    )
    .unwrap()
}

async fn setup_stores() -> (BlockDagKeyValueStorage, CasperBufferKeyValueStorage) {
    let mut dag_kvm = InMemoryStoreManager::new();
    let dag = BlockDagKeyValueStorage::new(&mut dag_kvm).await.unwrap();
    let mut genesis = get_random_block(
        Some(0),
        None,
        None,
        None,
        None,
        None,
        None,
        Some(vec![]),
        None,
        None,
        None,
        Some(vec![]),
        None,
        None,
    );
    genesis.header.version = CERTIFIED_ADMISSION_PROTOCOL_VERSION;
    dag.insert(&genesis, InsertMode::ApprovedGenesis).unwrap();

    let mut buf_kvm = InMemoryStoreManager::new();
    let buf_store = buf_kvm.store("parents-map".to_string()).await.unwrap();
    let typed_store = KeyValueTypedStoreImpl::new(buf_store);
    let buffer = CasperBufferKeyValueStorage::new_from_kv_store(typed_store)
        .await
        .unwrap();

    (dag, buffer)
}

#[tokio::test]
async fn atomic_insert_then_buffer_inserts_into_dag_and_removes_from_buffer() {
    let (dag, buffer) = setup_stores().await;
    let block = make_block();
    let hash_serde = BlockHashSerde(block.block_hash.clone());

    // Pre-state: block in buffer as pendant, NOT in DAG.
    buffer.put_pendant(hash_serde.clone()).unwrap();
    assert!(buffer.is_pendant(&hash_serde));
    assert!(!dag
        .get_representation()
        .unwrap()
        .contains(&block.block_hash));

    // Atomic transition.
    let updated_dag = atomic_insert_then_buffer(
        &dag,
        &block,
        InsertMode::Invalid,
        &certificate(&block),
        &rejected_outcome(&block),
        &buffer,
        BufferTransition::RemoveFromBuffer(hash_serde.clone()),
    )
    .unwrap();

    // Post-state: block in DAG (steady state (a) from §9.20).
    assert!(updated_dag.contains(&block.block_hash));
    assert!(!buffer.is_pendant(&hash_serde));
}

#[tokio::test]
async fn atomic_insert_then_buffer_idempotent_on_absent_hash() {
    let (dag, buffer) = setup_stores().await;
    let block = make_block();
    let hash_serde = BlockHashSerde(block.block_hash.clone());

    // Pre-state: block NOT in buffer, NOT in DAG.
    assert!(!buffer.is_pendant(&hash_serde));

    // Atomic transition succeeds despite buffer being empty (idempotence
    // guarantee — InvalidArgument from remove_unlocked is filtered).
    let updated_dag = atomic_insert_then_buffer(
        &dag,
        &block,
        InsertMode::Invalid,
        &certificate(&block),
        &rejected_outcome(&block),
        &buffer,
        BufferTransition::RemoveFromBuffer(hash_serde.clone()),
    )
    .unwrap();

    // Post-state: block in DAG, buffer still empty for this hash.
    assert!(updated_dag.contains(&block.block_hash));
    assert!(!buffer.is_pendant(&hash_serde));
}

#[tokio::test]
async fn atomic_insert_then_buffer_skip_does_not_touch_buffer() {
    let (dag, buffer) = setup_stores().await;
    let block = make_block();
    let hash_serde = BlockHashSerde(block.block_hash.clone());

    // Pre-state: pendant in buffer.
    buffer.put_pendant(hash_serde.clone()).unwrap();
    assert!(buffer.is_pendant(&hash_serde));

    // Skip variant: helper does NOT touch the buffer.
    let updated_dag = atomic_insert_then_buffer(
        &dag,
        &block,
        InsertMode::Normal,
        &certificate(&block),
        &accepted_outcome(&block),
        &buffer,
        BufferTransition::Skip,
    )
    .unwrap();

    // Post-state: block in DAG, pendant still in buffer.
    // (Skip is used by bootstrap paths that don't participate in the
    // buffer's dependency lifecycle.)
    assert!(updated_dag.contains(&block.block_hash));
    assert!(buffer.is_pendant(&hash_serde));
}

#[tokio::test]
async fn certified_outcome_must_match_insert_mode_before_any_state_changes() {
    for mode in [InsertMode::Normal, InsertMode::Invalid] {
        let (dag, buffer) = setup_stores().await;
        let block = make_block();
        let hash = BlockHashSerde(block.block_hash.clone());
        let outcome = match mode {
            InsertMode::Normal => rejected_outcome(&block),
            InsertMode::Invalid => accepted_outcome(&block),
            InsertMode::ApprovedGenesis => unreachable!(),
            InsertMode::SettledHistory => unreachable!(),
        };
        buffer.put_pendant(hash.clone()).unwrap();
        let error = atomic_insert_then_buffer(
            &dag,
            &block,
            mode,
            &certificate(&block),
            &outcome,
            &buffer,
            BufferTransition::RemoveFromBuffer(hash.clone()),
        )
        .err()
        .expect("mismatched admission mode must fail");

        assert!(error
            .to_string()
            .contains("DAG insert mode disagrees with certified admission outcome"));
        assert!(!dag
            .get_representation()
            .unwrap()
            .contains(&block.block_hash));
        assert!(buffer.is_pendant(&hash));
    }
}

#[tokio::test]
async fn certified_reinsertion_requires_the_identical_admission_outcome() {
    let (dag, _) = setup_stores().await;
    let block = make_block();
    let certificate = certificate(&block);
    let first = rejected_outcome(&block);
    let different_reason = CertifiedAdmissionOutcome::rejected(
        &block,
        &certificate,
        AdmissionRejectionReason::InvalidParents,
    )
    .unwrap();

    dag.insert_certified(&block, InsertMode::Invalid, &certificate, &first)
        .unwrap();
    let error = dag
        .insert_certified(&block, InsertMode::Invalid, &certificate, &different_reason)
        .err()
        .expect("conflicting certified outcome must fail");

    assert!(error
        .to_string()
        .contains("stored block metadata disagrees with certified admission outcome"));
}

#[tokio::test]
async fn reconcile_buffer_against_dag_purges_drifted_pendants() {
    let (dag, buffer) = setup_stores().await;
    let block = make_block();
    let hash_serde = BlockHashSerde(block.block_hash.clone());

    // Manually construct the (c) drift state from §9.20: block in DAG
    // AND pendant in buffer. This is what would result from a crash
    // after dag.insert but before buffer.remove.
    dag.insert_certified(
        &block,
        InsertMode::Invalid,
        &certificate(&block),
        &rejected_outcome(&block),
    )
    .unwrap();
    buffer.put_pendant(hash_serde.clone()).unwrap();
    assert!(dag
        .get_representation()
        .unwrap()
        .contains(&block.block_hash));
    assert!(buffer.is_pendant(&hash_serde));

    // Reconcile.
    let dag_rep = dag.get_representation().unwrap();
    let purged = reconcile_buffer_against_dag(&buffer, &dag_rep).unwrap();

    // Post-state: drift closed.
    assert_eq!(purged, 1);
    assert!(!buffer.is_pendant(&hash_serde));
    assert!(dag
        .get_representation()
        .unwrap()
        .contains(&block.block_hash));
}

#[tokio::test]
async fn reconcile_buffer_against_dag_leaves_non_drift_intact() {
    let (dag, buffer) = setup_stores().await;
    let block_not_in_dag = make_block();
    let hash_not_in_dag = BlockHashSerde(block_not_in_dag.block_hash.clone());

    // Put a pendant in buffer whose hash is NOT in the DAG (the
    // genuine "pending dependency" steady state (b) from §9.20).
    buffer.put_pendant(hash_not_in_dag.clone()).unwrap();

    let dag_rep = dag.get_representation().unwrap();
    let purged = reconcile_buffer_against_dag(&buffer, &dag_rep).unwrap();

    // Post-state: nothing purged, the pendant remains.
    assert_eq!(purged, 0);
    assert!(buffer.is_pendant(&hash_not_in_dag));
}

#[tokio::test]
async fn reconcile_buffer_against_dag_handles_mixed_state() {
    let (dag, buffer) = setup_stores().await;

    let drift_block = make_block();
    let drift_hash = BlockHashSerde(drift_block.block_hash.clone());
    dag.insert_certified(
        &drift_block,
        InsertMode::Invalid,
        &certificate(&drift_block),
        &rejected_outcome(&drift_block),
    )
    .unwrap();
    buffer.put_pendant(drift_hash.clone()).unwrap();

    let pending_block = make_block();
    let pending_hash = BlockHashSerde(pending_block.block_hash.clone());
    buffer.put_pendant(pending_hash.clone()).unwrap();

    // Sanity: both are pendants pre-reconcile.
    let all_pendants_pre: HashSet<BlockHashSerde> = buffer.get_pendants();
    assert!(all_pendants_pre.contains(&drift_hash));
    assert!(all_pendants_pre.contains(&pending_hash));

    let dag_rep = dag.get_representation().unwrap();
    let purged = reconcile_buffer_against_dag(&buffer, &dag_rep).unwrap();

    // Only the drifted hash purged; the pending hash remains.
    assert_eq!(purged, 1);
    assert!(!buffer.is_pendant(&drift_hash));
    assert!(buffer.is_pendant(&pending_hash));
}

#[tokio::test]
async fn reconcile_buffer_against_dag_is_idempotent() {
    // Running reconcile twice on the same drifted state should produce
    // the same end state and return zero purges on the second call.
    let (dag, buffer) = setup_stores().await;
    let block = make_block();
    let hash_serde = BlockHashSerde(block.block_hash.clone());
    dag.insert_certified(
        &block,
        InsertMode::Invalid,
        &certificate(&block),
        &rejected_outcome(&block),
    )
    .unwrap();
    buffer.put_pendant(hash_serde.clone()).unwrap();

    let dag_rep = dag.get_representation().unwrap();
    let first = reconcile_buffer_against_dag(&buffer, &dag_rep).unwrap();
    let second = reconcile_buffer_against_dag(&buffer, &dag_rep).unwrap();

    assert_eq!(first, 1);
    assert_eq!(second, 0);
    assert!(!buffer.is_pendant(&hash_serde));
}

// Idempotence-of-insert: calling atomic_insert_then_buffer twice with
// the same block is safe (the second call is a no-op on DAG via
// insert_internal's short-circuit, and the buffer remove is also
// no-op because the hash is already absent).
#[tokio::test]
async fn atomic_insert_then_buffer_idempotent_on_repeat() {
    let (dag, buffer) = setup_stores().await;
    let block = make_block();
    let hash_serde = BlockHashSerde(block.block_hash.clone());

    buffer.put_pendant(hash_serde.clone()).unwrap();

    let first = atomic_insert_then_buffer(
        &dag,
        &block,
        InsertMode::Invalid,
        &certificate(&block),
        &rejected_outcome(&block),
        &buffer,
        BufferTransition::RemoveFromBuffer(hash_serde.clone()),
    );
    assert!(first.is_ok());

    let second = atomic_insert_then_buffer(
        &dag,
        &block,
        InsertMode::Invalid,
        &certificate(&block),
        &rejected_outcome(&block),
        &buffer,
        BufferTransition::RemoveFromBuffer(hash_serde.clone()),
    );
    // Second call: block already in DAG (insert_internal short-circuits),
    // hash already absent from buffer (remove_unlocked tolerates).
    assert!(
        second.is_ok(),
        "second call must succeed (idempotence); got: {:?}",
        second.err()
    );

    assert!(dag
        .get_representation()
        .unwrap()
        .contains(&block.block_hash));
    assert!(!buffer.is_pendant(&hash_serde));
}
