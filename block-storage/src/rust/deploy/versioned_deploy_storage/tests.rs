use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Barrier;

use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::{Cosigned, Signed};
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::phlo_controls::{PhloControlsLimits, PhloControlsV1};
use models::rust::phlo_intent::{PhloFundingIntentLimits, PhloFundingIntentV1};
use models::rust::phlo_schedule::{PhloResourceClassV1, PhloScheduleV1};
use models::rust::phlo_source::{PhloSourceLimits, PhloSourcePolicyV1};
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::signed_phlo_deploy::{FundedDeploy, FundedDeployLimits, OfferedFundedDeploy};
use proptest::prelude::*;
use prost::Message;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{
    Db, LmdbDirStoreManager, LmdbEnvConfig,
};

use super::*;
use crate::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
use crate::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;
use crate::rust::deploy::pending_deploy::PendingDeploy;

fn limits() -> DeployEnvelopeLimits {
    let wire = PhloWireLimits {
        total_bytes: 65536,
        field_bytes: 32768,
    };
    DeployEnvelopeLimits {
        members: NonZeroUsize::new(16).unwrap(),
        payload: FundedDeployLimits {
            deploy_bytes: 131072,
            signing: wire,
            funding: PhloFundingIntentLimits {
                wire,
                controls: PhloControlsLimits {
                    wire,
                    owners: 16,
                    schedules: 2,
                    total_classes: 2,
                },
                sources: 16,
                resource_permissions: 16,
                authority_nodes: 64,
            },
        },
    }
}

fn body() -> DeployData {
    DeployData {
        term: "Nil".to_string(),
        language: "rholang".to_string(),
        time_stamp: 1,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
        authority_presentations: vec![],
    }
}

fn envelopes() -> [DeployEnvelope; 2] {
    let bounds = limits().payload;
    let schedule = PhloScheduleV1 {
        protocol_version: 6,
        network: b"storage-test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"phlo",
        decimal_scale: 8,
        classes: vec![PhloResourceClassV1 {
            identity: b"compute",
            measurement_unit: b"phlo",
            measurement_rule: [1; 32],
            valuation_rule: [2; 32],
            weight: 1,
        }],
        actual_price: 1,
        compatibility_rule: [3; 32],
    };
    let digest = schedule
        .digest(bounds.funding.controls().schedule(1))
        .unwrap();
    let funding = PhloFundingIntentV1 {
        controls: PhloControlsV1 {
            limit: 10,
            price_ceiling: 2,
            required_owner_ceilings: vec![2],
            permitted_schedules: vec![schedule],
        },
        schedule_commitment: digest,
        total_exposure: 10,
        sources: vec![PhloSourcePolicyV1::new(
            b"custody",
            10,
            10,
            true,
            vec![],
            PhloSourceLimits {
                wire: bounds.funding.wire,
                resource_permissions: 0,
                authority_nodes: 0,
            },
        )
        .unwrap()],
    }
    .encode(bounds.funding)
    .unwrap();
    let funded = Cosigned::create_single_envelope(
        FundedDeploy::new(body(), funding.clone(), bounds).unwrap(),
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[1; 32]),
    )
    .unwrap();
    let offered = Cosigned::create_single_envelope(
        OfferedFundedDeploy::new(body(), funding, 10, 1, bounds).unwrap(),
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[1; 32]),
    )
    .unwrap();
    [
        FundedDeploy::to_proto(&funded).unwrap(),
        OfferedFundedDeploy::to_proto(&offered).unwrap(),
    ]
    .map(|proto| DeployEnvelope::from_proto(proto, limits()).unwrap())
}

fn manager(path: PathBuf) -> impl KeyValueStoreManager {
    let config = LmdbEnvConfig::new("versioned-deploy-test".to_string(), 16 << 20).with_max_dbs(8);
    LmdbDirStoreManager::new(
        path,
        [
            "deploy_storage",
            "deploy_envelope_storage_v6",
            "rejected_deploy_buffer",
            "blocks",
            "blocks-approved",
            "finalization-certificates",
            DeployEnvelopeStoreKind::Pending.namespace(),
            DeployEnvelopeStoreKind::Rejected.namespace(),
        ]
        .into_iter()
        .map(|name| (Db::new(name.to_string(), None), config.clone()))
        .collect::<HashMap<_, _>>(),
    )
}

fn scratch() -> tempfile::TempDir {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/block-storage-test-scratch");
    std::fs::create_dir_all(&path).unwrap();
    tempfile::Builder::new()
        .prefix("versioned-deploy-")
        .tempdir_in(path)
        .unwrap()
}

fn pending_formats() -> Vec<PendingDeploy> {
    let key = PrivateKey::from_bytes(&[2; 32]);
    let signed = Signed::create(body(), Box::new(Secp256k1), key.clone()).unwrap();
    let plain = Cosigned::create_single_envelope(body(), Box::new(Secp256k1), key).unwrap();
    let mut records = vec![
        PendingDeploy::from_legacy(signed).unwrap(),
        PendingDeploy::from_envelope_v6(plain).unwrap(),
    ];
    records.extend(
        envelopes()
            .into_iter()
            .map(|envelope| PendingDeploy::from_envelope(envelope).unwrap()),
    );
    records
}

fn funded_block() -> models::rust::casper::protocol::casper_message::BlockMessage {
    use models::casper::{
        BlockMessageProto, BodyProto, HeaderProto, ProcessedDeployProto, RChainStateProto,
    };
    let proto = BlockMessageProto {
        block_hash: vec![1; 32].into(),
        header: Some(HeaderProto {
            version: 6,
            ..Default::default()
        }),
        body: Some(BodyProto {
            state: Some(RChainStateProto::default()),
            deploys: envelopes()
                .into_iter()
                .map(|envelope| ProcessedDeployProto {
                    deploy: Some(envelope.to_proto().unwrap()),
                    cost: Some(models::rhoapi::PCost { cost: 7 }),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }),
        ..Default::default()
    };
    models::rust::casper::protocol::casper_message::BlockMessage::from_proto_with_limits(
        proto,
        limits(),
    )
    .unwrap()
}

#[tokio::test]
async fn historical_block_store_rejects_unreadable_funded_writes() {
    use models::rust::casper::protocol::casper_message::{ApprovedBlock, ApprovedBlockCandidate};

    use crate::rust::key_value_block_store::KeyValueBlockStore;
    let mut kvm = InMemoryStoreManager::new();
    let store = KeyValueBlockStore::create_from_kvm(&mut kvm).await.unwrap();
    let block = funded_block();
    assert!(store.put(block.block_hash.clone(), &block).is_err());
    assert!(!store.contains_key(&block.block_hash).unwrap());
    let approved = ApprovedBlock {
        candidate: ApprovedBlockCandidate {
            block,
            required_sigs: 0,
        },
        sigs: vec![],
        floor_seed: None,
    };
    assert!(store.put_approved_block(&approved).is_err());
    assert!(store.get_approved_block().unwrap().is_none());
}

#[tokio::test]
async fn bounded_block_store_retains_funding_across_restart_and_rejects_smaller_limits() {
    use models::rust::casper::protocol::casper_message::{ApprovedBlock, ApprovedBlockCandidate};

    use crate::rust::key_value_block_store::KeyValueBlockStore;
    let directory = scratch();
    let mut kvm = manager(directory.path().to_path_buf());
    let store = KeyValueBlockStore::create_from_kvm_with_limits(&mut kvm, limits())
        .await
        .unwrap();
    let block = funded_block();
    let approved = ApprovedBlock {
        candidate: ApprovedBlockCandidate {
            block: block.clone(),
            required_sigs: 0,
        },
        sigs: vec![],
        floor_seed: None,
    };
    store.put(block.block_hash.clone(), &block).unwrap();
    store.put_approved_block(&approved).unwrap();
    assert_eq!(
        store.get_detached(&block.block_hash).unwrap(),
        Some(block.clone())
    );
    for deploy in &block.body.deploys {
        assert!(store
            .has_any_deploy_id_strict(
                &block.block_hash,
                &std::collections::HashSet::from([deploy.envelope().identity().clone()])
            )
            .unwrap());
    }
    let original = kvm
        .store("blocks".to_string())
        .await
        .unwrap()
        .to_map()
        .unwrap();
    let original_approved = kvm
        .store("blocks-approved".to_string())
        .await
        .unwrap()
        .to_map()
        .unwrap();
    let mut smaller = limits();
    smaller.payload.deploy_bytes = 1;
    let strict = store.clone().with_deploy_envelope_limits(smaller);
    assert!(strict.get(&block.block_hash).is_err());
    assert!(strict.get_approved_block().is_err());
    assert!(strict.put(block.block_hash.clone(), &block).is_err());
    assert!(strict.put_approved_block(&approved).is_err());
    assert_eq!(
        kvm.store("blocks".to_string())
            .await
            .unwrap()
            .to_map()
            .unwrap(),
        original
    );
    assert_eq!(
        kvm.store("blocks-approved".to_string())
            .await
            .unwrap()
            .to_map()
            .unwrap(),
        original_approved
    );
    drop(strict);
    drop(store);
    kvm.shutdown().await.unwrap();
    drop(kvm);
    let mut kvm = manager(directory.path().to_path_buf());
    let reopened = KeyValueBlockStore::create_from_kvm_with_limits(&mut kvm, limits())
        .await
        .unwrap();
    assert_eq!(reopened.get(&block.block_hash).unwrap(), Some(block));
    assert_eq!(reopened.get_approved_block().unwrap(), Some(approved));
    drop(reopened);
    kvm.shutdown().await.unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn bounded_block_store_preserves_funded_sequence_and_receipts(
        selection in prop::collection::vec((0usize..2, 0u64..10_000), 0..16)
    ) {
        use crate::rust::key_value_block_store::KeyValueBlockStore;
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let mut kvm = InMemoryStoreManager::new();
        let store = runtime.block_on(KeyValueBlockStore::create_from_kvm_with_limits(&mut kvm, limits())).unwrap();
        let mut block = funded_block();
        let templates = block.body.deploys.clone();
        block.body.deploys = selection.into_iter().map(|(index, cost)| {
            let mut processed = templates[index].clone();
            processed.cost = models::rhoapi::PCost { cost };
            processed
        }).collect();
        store.put(block.block_hash.clone(), &block).unwrap();
        let restored = store.get(&block.block_hash).unwrap().unwrap();
        prop_assert_eq!(&restored, &block);
        let mut wrong_limits = limits();
        wrong_limits.payload.deploy_bytes = 1;
        let strict = store.clone().with_deploy_envelope_limits(wrong_limits);
        prop_assert_eq!(strict.put(block.block_hash.clone(), &block).is_ok(), block.body.deploys.is_empty());
        prop_assert_eq!(store.get(&block.block_hash).unwrap(), Some(block));
    }
}

#[tokio::test]
async fn rejected_facade_mixed_batches_survive_restart_without_changing_legacy_bytes() {
    let directory = scratch();
    let records = pending_formats();
    let mut kvm = manager(directory.path().to_path_buf());
    let mut buffer = KeyValueRejectedDeployBuffer::new_with_limits(&mut kvm, limits())
        .await
        .unwrap();
    buffer.add(records[..2].to_vec()).unwrap();
    let historical = buffer.store.raw_store().to_map().unwrap();
    buffer.add(records.clone()).unwrap();
    assert_eq!(buffer.store.raw_store().to_map().unwrap(), historical);
    assert_eq!(buffer.read_all().unwrap().len(), 4);
    drop(buffer);
    kvm.shutdown().await.unwrap();
    drop(kvm);
    let mut kvm = manager(directory.path().to_path_buf());
    let mut buffer = KeyValueRejectedDeployBuffer::new_with_limits(&mut kvm, limits())
        .await
        .unwrap();
    for record in &records {
        let restored = buffer.get_by_id(record.typed_deploy_id()).unwrap().unwrap();
        assert_eq!(restored.envelope(), record.envelope());
        assert!(buffer.contains_id(record.typed_deploy_id()).unwrap());
    }
    buffer.remove(records.clone()).unwrap();
    assert!(!buffer.non_empty().unwrap());
    buffer
        .add(vec![records[3].clone(), records[3].clone()])
        .unwrap();
    assert!(buffer.remove_by_id(records[3].typed_deploy_id()).unwrap());
    assert!(!buffer.remove_by_id(records[3].typed_deploy_id()).unwrap());
    drop(buffer);
    kvm.shutdown().await.unwrap();
}

#[tokio::test]
async fn rejected_facade_prepares_entire_batch_before_writing() {
    let records = pending_formats();
    let mut kvm = InMemoryStoreManager::new();
    let mut bounds = limits();
    bounds.payload.deploy_bytes = 1;
    let mut buffer = KeyValueRejectedDeployBuffer::new_with_limits(&mut kvm, bounds)
        .await
        .unwrap();
    buffer.add(vec![records[0].clone()]).unwrap();
    let before = buffer.store.raw_store().to_map().unwrap();
    assert!(buffer
        .add(vec![records[1].clone(), records[2].clone()])
        .is_err());
    assert_eq!(buffer.store.raw_store().to_map().unwrap(), before);
    assert_eq!(buffer.read_all().unwrap().len(), 1);
    let mut historical = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
    assert!(historical.add(records.clone()).is_err());
    assert!(historical.remove(records).is_err());
    assert_eq!(historical.store.raw_store().to_map().unwrap(), before);
}

#[tokio::test]
async fn rejected_facade_rejects_cross_environment_batches_before_any_write() {
    let directory = scratch();
    let mut kvm = LmdbDirStoreManager::new(
        directory.path().to_path_buf(),
        [
            "rejected_deploy_buffer",
            DeployEnvelopeStoreKind::Rejected.namespace(),
        ]
        .into_iter()
        .map(|name| {
            (
                Db::new(name.to_string(), None),
                LmdbEnvConfig::new(name.to_string(), 16 << 20).with_max_dbs(2),
            )
        })
        .collect(),
    );
    let mut buffer = KeyValueRejectedDeployBuffer::new_with_limits(&mut kvm, limits())
        .await
        .unwrap();
    assert!(matches!(
        buffer.add(pending_formats()),
        Err(KvStoreError::AtomicityUnavailable(_))
    ));
    assert!(buffer.read_all().unwrap().is_empty());
    assert!(!buffer.non_empty().unwrap());
    drop(buffer);
    kvm.shutdown().await.unwrap();
}

#[tokio::test]
async fn rejected_facade_concurrent_batches_have_a_complete_serial_outcome() {
    let directory = scratch();
    let mut kvm = manager(directory.path().to_path_buf());
    let buffer = KeyValueRejectedDeployBuffer::new_with_limits(&mut kvm, limits())
        .await
        .unwrap();
    let records = pending_formats();
    let barrier = Barrier::new(8);
    std::thread::scope(|scope| {
        let threads: Vec<_> = (0..8)
            .map(|worker| {
                let mut buffer = buffer.clone();
                let records = &records;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    for _ in 0..8 {
                        if worker % 2 == 0 {
                            buffer.add(records.clone()).unwrap();
                        } else {
                            buffer.remove(records.clone()).unwrap();
                        }
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
    });
    let result = buffer.read_all().unwrap();
    assert!(result.is_empty() || result.len() == records.len());
    for record in result {
        assert_eq!(
            record.envelope(),
            records
                .iter()
                .find(|original| original.typed_deploy_id() == record.typed_deploy_id())
                .unwrap()
                .envelope()
        );
    }
    drop(buffer);
    kvm.shutdown().await.unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn rejected_facade_batch_histories_match_atomic_reference(
        operations in prop::collection::vec((any::<bool>(), prop::collection::vec(0usize..4, 0..8)), 0..32)
    ) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let mut kvm = InMemoryStoreManager::new();
        let mut buffer = runtime.block_on(KeyValueRejectedDeployBuffer::new_with_limits(&mut kvm, limits())).unwrap();
        let records = pending_formats();
        let mut expected = HashMap::new();
        for (insert, indices) in operations {
            let batch: Vec<_> = indices.iter().map(|&index| records[index].clone()).collect();
            if insert { buffer.add(batch).unwrap(); } else { buffer.remove(batch).unwrap(); }
            for index in indices {
                if insert {
                    expected.insert(records[index].typed_deploy_id().clone(), records[index].clone());
                } else {
                    expected.remove(records[index].typed_deploy_id());
                }
            }
            let observed = buffer.read_all().unwrap();
            prop_assert_eq!(observed.len(), expected.len());
            prop_assert_eq!(buffer.non_empty().unwrap(), !expected.is_empty());
            for record in &records {
                let restored = buffer.get_by_id(record.typed_deploy_id()).unwrap();
                prop_assert_eq!(restored.as_ref().map(PendingDeploy::envelope),
                    expected.get(record.typed_deploy_id()).map(PendingDeploy::envelope));
            }
        }
    }
}

#[tokio::test]
async fn pending_facade_retains_all_formats_across_restart_and_preserves_legacy_rows() {
    let directory = scratch();
    let records = pending_formats();
    let mut kvm = manager(directory.path().to_path_buf());
    let mut store = KeyValueDeployStorage::new_with_limits(&mut kvm, limits())
        .await
        .unwrap();
    for record in &records[..2] {
        assert!(store.add_pending_if_absent(record).unwrap());
    }
    let legacy = store.store.raw_store().to_map().unwrap();
    let plain = store.envelope_store.raw_store().to_map().unwrap();
    for record in &records[2..] {
        assert!(store.add_pending_if_absent(record).unwrap());
        assert!(!store.add_pending_if_absent(record).unwrap());
    }
    assert_eq!(store.store.raw_store().to_map().unwrap(), legacy);
    assert_eq!(store.envelope_store.raw_store().to_map().unwrap(), plain);
    assert_eq!(store.read_all_pending().unwrap().len(), 4);
    assert!(store.read_all_for_protocol(6).is_err());
    drop(store);
    kvm.shutdown().await.unwrap();
    drop(kvm);
    let mut reopened = manager(directory.path().to_path_buf());
    let mut store = KeyValueDeployStorage::new_with_limits(&mut reopened, limits())
        .await
        .unwrap();
    for record in &records {
        let restored = store
            .get_pending(record.typed_deploy_id())
            .unwrap()
            .unwrap();
        assert_eq!(restored.envelope(), record.envelope());
    }
    assert!(store.remove_pending(&records[2]).unwrap());
    assert!(!store.remove_pending(&records[2]).unwrap());
    assert_eq!(store.read_all_pending().unwrap().len(), 3);
    assert_eq!(store.store.raw_store().to_map().unwrap(), legacy);
    assert_eq!(store.envelope_store.raw_store().to_map().unwrap(), plain);
    drop(store);
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn pending_facade_requires_explicit_limits_and_rejects_wrong_namespace_rows() {
    let mut kvm = InMemoryStoreManager::new();
    let records = pending_formats();
    let mut historical = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
    assert!(historical.add_pending_if_absent(&records[2]).is_err());
    assert!(!historical.non_empty().unwrap());
    let generic = VersionedDeployStorage::new(&mut kvm, DeployEnvelopeStoreKind::Pending, limits())
        .await
        .unwrap();
    generic.insert_if_absent(records[1].envelope()).unwrap();
    assert!(KeyValueDeployStorage::new_with_limits(&mut kvm, limits())
        .await
        .is_err());
    assert!(!historical.non_empty().unwrap());
}

#[tokio::test]
async fn pending_facade_insertion_is_atomic_across_handles() {
    let mut kvm = InMemoryStoreManager::new();
    let store = KeyValueDeployStorage::new_with_limits(&mut kvm, limits())
        .await
        .unwrap();
    let pending = pending_formats().remove(3);
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let mut handle = store.clone();
            let pending = pending.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                handle.add_pending_if_absent(&pending).unwrap()
            })
        })
        .collect();
    let inserted = threads
        .into_iter()
        .map(|thread| usize::from(thread.join().unwrap()))
        .sum::<usize>();
    assert_eq!(inserted, 1);
    assert_eq!(
        store
            .get_pending(pending.typed_deploy_id())
            .unwrap()
            .unwrap()
            .envelope(),
        pending.envelope()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn pending_facade_histories_preserve_complete_envelopes_and_namespace_isolation(
        operations in proptest::collection::vec((0u8..3, 0usize..4), 0..40),
    ) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let mut manager = InMemoryStoreManager::new();
        let mut store = runtime.block_on(KeyValueDeployStorage::new_with_limits(&mut manager, limits())).unwrap();
        let records = pending_formats();
        let mut expected = HashMap::new();
        for (operation, index) in operations {
            let record = &records[index];
            let legacy = store.store.raw_store().to_map().unwrap();
            let body = store.envelope_store.raw_store().to_map().unwrap();
            match operation {
                0 => {
                    let absent = !expected.contains_key(record.typed_deploy_id());
                    prop_assert_eq!(store.add_pending_if_absent(record).unwrap(), absent);
                    expected.insert(record.typed_deploy_id().clone(), record.envelope().clone());
                }
                1 => prop_assert_eq!(store.remove_pending(record).unwrap(), expected.remove(record.typed_deploy_id()).is_some()),
                _ => prop_assert_eq!(store.get_pending(record.typed_deploy_id()).unwrap().map(PendingDeploy::into_envelope), expected.get(record.typed_deploy_id()).cloned()),
            }
            if index >= 2 {
                prop_assert_eq!(store.store.raw_store().to_map().unwrap(), legacy);
                prop_assert_eq!(store.envelope_store.raw_store().to_map().unwrap(), body);
            }
            let actual: HashMap<_, _> = store.read_all_pending().unwrap().into_iter().map(|pending| (pending.typed_deploy_id().clone(), pending.into_envelope())).collect();
            prop_assert_eq!(&actual, &expected);
            prop_assert_eq!(store.non_empty().unwrap(), !expected.is_empty());
        }
    }
}

#[tokio::test]
async fn both_stores_survive_restart_without_modifying_legacy_bincode_rows() {
    let directory = scratch();
    let [funded, offered] = envelopes();
    let signed = Signed::create(
        body(),
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[2; 32]),
    )
    .unwrap();
    let plain = Cosigned::create_single_envelope(
        body(),
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[3; 32]),
    )
    .unwrap();
    let pending = PendingDeploy::from_envelope_v6(plain.clone()).unwrap();
    let mut first = manager(directory.path().to_path_buf());
    let mut old = KeyValueDeployStorage::new(&mut first).await.unwrap();
    old.add(vec![signed.clone()]).unwrap();
    old.add_envelope_if_absent(plain.clone()).unwrap();
    let mut rejected = KeyValueRejectedDeployBuffer::new(&mut first).await.unwrap();
    rejected.add(vec![pending.clone()]).unwrap();
    let old_bytes = old.store.raw_store().to_map().unwrap();
    let plain_bytes = old.envelope_store.raw_store().to_map().unwrap();
    let rejected_bytes = rejected.store.raw_store().to_map().unwrap();
    for kind in [
        DeployEnvelopeStoreKind::Pending,
        DeployEnvelopeStoreKind::Rejected,
    ] {
        let storage = VersionedDeployStorage::new(&mut first, kind, limits())
            .await
            .unwrap();
        for envelope in [&funded, &offered] {
            assert!(storage.insert_if_absent(envelope).unwrap());
            assert!(!storage.insert_if_absent(envelope).unwrap());
            assert_eq!(
                storage.get(envelope.identity()).unwrap().as_ref(),
                Some(envelope)
            );
        }
        let mut count = 0;
        storage
            .visit_envelopes(&mut |_| {
                count += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(count, 2);
        assert!(storage.remove(funded.identity()).unwrap());
        assert!(!storage.remove(funded.identity()).unwrap());
    }
    drop(old);
    drop(rejected);
    first.shutdown().await.unwrap();
    drop(first);
    let mut reopened = manager(directory.path().to_path_buf());
    for kind in [
        DeployEnvelopeStoreKind::Pending,
        DeployEnvelopeStoreKind::Rejected,
    ] {
        let storage = VersionedDeployStorage::new(&mut reopened, kind, limits())
            .await
            .unwrap();
        assert!(storage.get(funded.identity()).unwrap().is_none());
        assert_eq!(
            storage.get(offered.identity()).unwrap(),
            Some(offered.clone())
        );
        assert!(storage.non_empty().unwrap());
    }
    let old = KeyValueDeployStorage::new(&mut reopened).await.unwrap();
    let rejected = KeyValueRejectedDeployBuffer::new(&mut reopened)
        .await
        .unwrap();
    assert_eq!(old.store.raw_store().to_map().unwrap(), old_bytes);
    assert_eq!(
        old.envelope_store.raw_store().to_map().unwrap(),
        plain_bytes
    );
    assert_eq!(rejected.store.raw_store().to_map().unwrap(), rejected_bytes);
    assert_eq!(
        old.get_envelope(plain.envelope_commitment().unwrap().as_ref())
            .unwrap(),
        Some(plain)
    );
    assert_eq!(
        rejected.get_by_id(pending.typed_deploy_id()).unwrap(),
        Some(pending)
    );
    drop(old);
    drop(rejected);
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn reads_and_reopen_reject_corrupted_rows_without_deleting_them() {
    let mut manager = InMemoryStoreManager::new();
    let envelope = envelopes()[1].clone();
    let storage =
        VersionedDeployStorage::new(&mut manager, DeployEnvelopeStoreKind::Pending, limits())
            .await
            .unwrap();
    let key = bincode::serialize(envelope.identity()).unwrap();
    let record = StoredDeployEnvelope::new(&envelope).unwrap();
    let valid = bincode::serialize(&record).unwrap();
    let mut proto = envelope.to_proto().unwrap();
    proto.phlo_price += 1;
    let bad_offer = bincode::serialize(&(1u32, Some(0x60003u32), proto.encode_to_vec())).unwrap();
    let mut trailing = valid.clone();
    trailing.push(0);
    let mut corrupt_schema = valid.clone();
    corrupt_schema[0] = 0;
    let excessive = vec![0; storage.record_limit as usize + 1];
    for malformed in [bad_offer, trailing, corrupt_schema, excessive] {
        storage
            .store
            .put_one(key.clone(), malformed.clone())
            .unwrap();
        assert!(storage.get(envelope.identity()).is_err());
        assert!(storage.visit_envelopes(&mut |_| Ok(())).is_err());
        assert!(storage.insert_if_absent(&envelope).is_err());
        assert!(VersionedDeployStorage::new(
            &mut manager,
            DeployEnvelopeStoreKind::Pending,
            limits()
        )
        .await
        .is_err());
        assert_eq!(storage.store.get_one(&key).unwrap(), Some(malformed));
    }
    storage.store.put_one(key.clone(), valid.clone()).unwrap();
    let other = envelopes()[0].identity().clone();
    storage
        .store
        .put_one(bincode::serialize(&other).unwrap(), valid)
        .unwrap();
    assert!(storage.get(&other).is_err());
    assert!(
        VersionedDeployStorage::new(&mut manager, DeployEnvelopeStoreKind::Pending, limits())
            .await
            .is_err()
    );
    storage
        .store
        .delete(vec![bincode::serialize(&other).unwrap()])
        .unwrap();
    assert_eq!(storage.get(envelope.identity()).unwrap(), Some(envelope));
}

#[tokio::test]
async fn concurrent_duplicate_inserts_have_one_winner() {
    let mut manager = InMemoryStoreManager::new();
    let storage =
        VersionedDeployStorage::new(&mut manager, DeployEnvelopeStoreKind::Pending, limits())
            .await
            .unwrap();
    let envelope = envelopes()[1].clone();
    let barrier = Barrier::new(8);
    std::thread::scope(|scope| {
        let threads: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    storage.insert_if_absent(&envelope).unwrap()
                })
            })
            .collect();
        assert_eq!(
            threads
                .into_iter()
                .map(|thread| usize::from(thread.join().unwrap()))
                .sum::<usize>(),
            1
        );
    });
    assert_eq!(
        storage.get(envelope.identity()).unwrap(),
        Some(envelope.clone())
    );
    assert!(storage.remove(envelope.identity()).unwrap());
    assert!(!storage.remove(envelope.identity()).unwrap());
    assert!(!storage.non_empty().unwrap());
}

#[tokio::test]
async fn oversized_lookup_keys_fail_before_store_access() {
    let mut manager = InMemoryStoreManager::new();
    let storage =
        VersionedDeployStorage::new(&mut manager, DeployEnvelopeStoreKind::Pending, limits())
            .await
            .unwrap();
    let identity = DeployLookupId::Legacy(models::rust::deploy_id::LegacyDeploySignature::new(
        vec![0; storage.record_limit as usize],
    ));
    assert!(storage.get(&identity).is_err());
    assert!(storage.remove(&identity).is_err());
    assert!(!storage.non_empty().unwrap());
}

#[tokio::test]
async fn pending_funded_envelopes_retain_controls_without_entering_body_only_paths() {
    let mut manager = InMemoryStoreManager::new();
    let storage =
        VersionedDeployStorage::new(&mut manager, DeployEnvelopeStoreKind::Rejected, limits())
            .await
            .unwrap();
    for envelope in envelopes() {
        let original = envelope.to_proto().unwrap();
        let pending = PendingDeploy::from_envelope(envelope.clone()).unwrap();
        assert_eq!(pending.typed_deploy_id(), envelope.identity());
        assert_eq!(pending.deploy_id().as_ref(), envelope.identity().as_bytes());
        assert_eq!(pending.data(), envelope.body());
        assert_eq!(pending.encoded_len(), original.encoded_len());
        assert_eq!(pending.envelope(), &envelope);
        assert!(pending.clone().into_body_envelope().is_err());
        assert!(pending.envelope().body_envelope().is_err());
        assert!(bincode::serialize(&pending).is_err());
        for protocol in [5, 6, 7, i64::MAX] {
            assert!(pending.validate_for_protocol(protocol).is_err());
        }
        assert!(storage.insert_if_absent(&pending.into_envelope()).unwrap());
        let restored =
            PendingDeploy::from_envelope(storage.get(envelope.identity()).unwrap().unwrap())
                .unwrap();
        assert_eq!(restored.envelope().to_proto().unwrap(), original);
        assert!(restored.into_body_envelope().is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn lookup_key_limits_match_fixed_width_codec(
        identity_size in 0usize..512,
        deploy_limit in 0usize..512,
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let mut manager = InMemoryStoreManager::new();
        let mut bounds = limits();
        bounds.payload.deploy_bytes = deploy_limit;
        let storage = runtime.block_on(VersionedDeployStorage::new(
            &mut manager, DeployEnvelopeStoreKind::Pending, bounds,
        )).unwrap();
        let identity = DeployLookupId::Legacy(models::rust::deploy_id::LegacyDeploySignature::new(
            vec![0; identity_size],
        ));
        let encoded_size = bincode::serialized_size(&identity).unwrap();
        prop_assert_eq!(encoded_size, 12 + identity_size as u64);
        let fits = encoded_size <= storage.record_limit;
        prop_assert_eq!(storage.get(&identity).is_ok(), fits);
        prop_assert_eq!(storage.remove(&identity).is_ok(), fits);
        let identity = DeployLookupId::V6(
            models::rust::deploy_id::DeployIdV6::try_from([0u8; 32].as_slice()).unwrap(),
        );
        prop_assert_eq!(bincode::serialized_size(&identity).unwrap(), 36);
        prop_assert!(storage.get(&identity).unwrap().is_none());
        prop_assert!(!storage.remove(&identity).unwrap());
        prop_assert!(!storage.non_empty().unwrap());
    }

    #[test]
    fn generated_store_histories_preserve_complete_envelopes(
        operations in prop::collection::vec((0u8..5, any::<bool>()), 1..64),
        rejected in any::<bool>(),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let mut manager = InMemoryStoreManager::new();
        let kind = if rejected { DeployEnvelopeStoreKind::Rejected } else { DeployEnvelopeStoreKind::Pending };
        let mut storage = runtime.block_on(VersionedDeployStorage::new(
            &mut manager, kind, limits(),
        )).unwrap();
        let envelopes = envelopes();
        let mut expected = HashMap::new();
        for (operation, second) in operations {
            let envelope = &envelopes[usize::from(second)];
            match operation {
                0 => {
                    let vacant = !expected.contains_key(envelope.identity());
                    prop_assert_eq!(storage.insert_if_absent(envelope).unwrap(), vacant);
                    expected.entry(envelope.identity().clone()).or_insert_with(|| envelope.clone());
                }
                1 => {
                    prop_assert_eq!(storage.remove(envelope.identity()).unwrap(), expected.remove(envelope.identity()).is_some());
                }
                2 => {
                    prop_assert_eq!(storage.get(envelope.identity()).unwrap(), expected.get(envelope.identity()).cloned());
                }
                3 => {
                    storage = runtime.block_on(VersionedDeployStorage::new(
                        &mut manager, kind, limits(),
                    )).unwrap();
                }
                _ => {}
            }
            let mut actual = HashMap::new();
            storage.visit_envelopes(&mut |retained| {
                actual.insert(retained.identity().clone(), retained);
                Ok(())
            }).unwrap();
            prop_assert_eq!(&actual, &expected);
            prop_assert_eq!(storage.non_empty().unwrap(), !expected.is_empty());
        }
    }
}
