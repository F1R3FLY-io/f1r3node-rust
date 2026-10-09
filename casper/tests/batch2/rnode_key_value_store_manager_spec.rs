use block_storage::rust::dag::block_dag_key_value_storage::BlockDagKeyValueStorage;
use casper::rust::storage::rnode_key_value_store_manager::new_key_value_store_manager;
use models::rust::block_hash::{BlockHashSerde, LENGTH};
use prost::bytes::Bytes;
use tempfile::TempDir;

#[tokio::test]
async fn rnode_store_manager_initializes_block_dag_storage_on_fresh_lmdb_dir() {
    let dir = TempDir::new().unwrap();
    let mut kvm = new_key_value_store_manager(dir.path().to_path_buf(), None);
    let dag_storage = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();

    let block_hash = Bytes::from(vec![1; LENGTH]);
    let floor_hash = Bytes::from(vec![2; LENGTH]);

    dag_storage
        .floor_index_for_tests()
        .put_one(
            BlockHashSerde(block_hash.clone()),
            BlockHashSerde(floor_hash.clone()),
        )
        .unwrap();

    let stored = dag_storage
        .floor_index_for_tests()
        .get_one(&BlockHashSerde(block_hash))
        .unwrap();

    assert_eq!(stored, Some(BlockHashSerde(floor_hash)));
}

#[tokio::test]
async fn rnode_store_manager_frontier_index_round_trips() {
    // The persisted per-block finalized frontier F(X) (the warm up-walk pivot)
    // must round-trip through the new `frontier-index` LMDB store, exactly like
    // the floor index. Covers the H2 cache's persistence layer.
    let dir = TempDir::new().unwrap();
    let mut kvm = new_key_value_store_manager(dir.path().to_path_buf(), None);
    let dag_storage = BlockDagKeyValueStorage::new(&mut kvm).await.unwrap();

    let block_hash = Bytes::from(vec![7; LENGTH]);
    let frontier_hash = Bytes::from(vec![9; LENGTH]);

    dag_storage
        .frontier_index_for_tests()
        .put_one(
            BlockHashSerde(block_hash.clone()),
            BlockHashSerde(frontier_hash.clone()),
        )
        .unwrap();

    let stored = dag_storage
        .frontier_index_for_tests()
        .get_one(&BlockHashSerde(block_hash))
        .unwrap();

    assert_eq!(stored, Some(BlockHashSerde(frontier_hash)));
}

/// DR-116 (gap G6): the rejected-deploy buffer opens its envelope table through
/// the registered LMDB database, and the table survives a restart.
#[tokio::test]
async fn rnode_store_manager_keeps_the_rejected_envelope_buffer_across_restart() {
    use block_storage::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signed::Cosigned;
    use models::rust::cost_deploy_data::DeployData;
    use models::rust::cost_protocol_limits::offered_funded_v6_limits;
    use models::rust::deploy_envelope::DeployEnvelope;
    use models::rust::deploy_id::DeployLookupId;

    let signed = Cosigned::create_single_envelope(
        DeployData {
            term: "Nil".to_string(),
            language: "rholang".to_string(),
            time_stamp: 1,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        },
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[2; 32]),
    )
    .expect("a body envelope signs");
    let envelope = DeployEnvelope::from_body_envelope(signed).expect("a body envelope encodes");
    let DeployLookupId::V6(id) = envelope.identity() else {
        panic!("a body envelope has a v6 identity")
    };
    let id = *id;
    let limits = offered_funded_v6_limits().envelope;

    let dir = TempDir::new().unwrap();
    {
        let mut kvm = new_key_value_store_manager(dir.path().to_path_buf(), None);
        let mut buffer = KeyValueRejectedDeployBuffer::new(&mut kvm)
            .await
            .expect("the envelope table is registered");
        buffer
            .add_envelopes(std::slice::from_ref(&envelope))
            .expect("the envelope is buffered");
    }
    let mut kvm = new_key_value_store_manager(dir.path().to_path_buf(), None);
    let mut buffer = KeyValueRejectedDeployBuffer::new(&mut kvm)
        .await
        .expect("the envelope table reopens");
    assert_eq!(
        buffer
            .read_all_envelopes(limits)
            .expect("the table decodes"),
        vec![envelope]
    );
    assert!(buffer
        .remove_envelope_by_id(&id)
        .expect("the entry removes"));
}
