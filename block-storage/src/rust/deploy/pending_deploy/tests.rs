use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::{CosignedSigningDomain, Cosigner};
use models::rust::deploy_id::{DeployIdV6, LegacyDeploySignature};
use proptest::prelude::*;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{
    Db, LmdbDirStoreManager, LmdbEnvConfig,
};

use super::*;
use crate::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;

#[derive(Clone, serde::Serialize)]
struct HistoricalPendingRecord {
    deploy_id: DeployLookupId,
    #[serde(with = "shared::rust::serde_bytes")]
    deploy_id_bytes: Bytes,
    envelope: Cosigned<DeployData>,
}

fn body() -> DeployData {
    DeployData {
        term: "Nil".to_string(),
        language: "rholang".to_string(),
        time_stamp: 17,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    }
}

fn record(envelope: Cosigned<DeployData>) -> HistoricalPendingRecord {
    let (deploy_id, deploy_id_bytes) = if envelope.is_envelope_bound() {
        let bytes = envelope.envelope_commitment().unwrap();
        (
            DeployLookupId::V6(DeployIdV6::try_from(bytes.as_ref()).unwrap()),
            bytes,
        )
    } else {
        let bytes = envelope.primary().sig.clone();
        (
            DeployLookupId::Legacy(LegacyDeploySignature::new(bytes.to_vec())),
            bytes,
        )
    };
    HistoricalPendingRecord {
        deploy_id,
        deploy_id_bytes,
        envelope,
    }
}

fn threshold_envelope(count: u8) -> Cosigned<DeployData> {
    let mut members: Vec<_> = (1..=count)
        .map(|i| {
            let key = PrivateKey::from_bytes(&[i; 32]);
            (
                Cosigner {
                    pk: Secp256k1.to_public(&key),
                    sig: Bytes::new(),
                    sig_algorithm: Box::new(Secp256k1) as Box<dyn SignaturesAlg>,
                },
                key,
            )
        })
        .collect();
    members.sort_by(|a, b| a.0.pk.bytes.cmp(&b.0.pk.bytes));
    let mut signers: Vec<_> = members.iter().map(|(signer, _)| signer.clone()).collect();
    for signer in signers.iter_mut().skip(1) {
        signer.sig = Bytes::from_static(&[1]);
    }
    let hash =
        Cosigned::envelope_signing_hash(&body(), &signers, u32::from(count - 1), "secp256k1")
            .unwrap();
    for (signer, (_, key)) in signers.iter_mut().zip(&members).skip(1) {
        signer.sig = Secp256k1.sign(&hash, &key.bytes).into();
    }
    Cosigned::from_envelope_signed_data_threshold(body(), signers, u32::from(count - 1)).unwrap()
}

fn compound_legacy() -> Cosigned<DeployData> {
    let signers = (1..=3)
        .map(|i| {
            let signed = Signed::create(
                body(),
                Box::new(Secp256k1),
                PrivateKey::from_bytes(&[i; 32]),
            )
            .unwrap();
            Cosigner {
                pk: signed.pk,
                sig: signed.sig,
                sig_algorithm: signed.sig_algorithm,
            }
        })
        .collect();
    Cosigned::from_signed_data(body(), signers).unwrap()
}

fn records() -> Vec<HistoricalPendingRecord> {
    let key = PrivateKey::from_bytes(&[1; 32]);
    let signed = Signed::create(body(), Box::new(Secp256k1), key.clone()).unwrap();
    let legacy = Cosigned::from_single_signer(signed).unwrap();
    let bound = Cosigned::create_single_envelope(body(), Box::new(Secp256k1), key).unwrap();
    vec![
        record(legacy),
        record(bound),
        record(threshold_envelope(3)),
        record(compound_legacy()),
    ]
}

#[test]
fn historical_pending_bincode_layout_is_unchanged() {
    for record in records() {
        let bytes = bincode::serialize(&record).unwrap();
        let decoded: PendingDeploy = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.typed_deploy_id(), &record.deploy_id);
        assert_eq!(decoded.deploy_id(), &record.deploy_id_bytes);
        assert_eq!(decoded.data(), record.envelope.data());
        assert_eq!(
            decoded.envelope().body_envelope().unwrap(),
            &record.envelope
        );
        assert_eq!(
            decoded.encoded_len(),
            DeployData::to_proto_cosigned(&record.envelope).encoded_len()
        );
        assert_eq!(bincode::serialize(&decoded).unwrap(), bytes);
    }
}

#[test]
fn legacy_pending_fixture_digests() {
    let expected = [
        "9c5b16092797160f6250d2e569b180b66fbedc0dcd47d0b8871c4caf09b6d259",
        "20cc6233587d3d89a6f7cff487e9566f882d470ef2cd01ddcd80110a1824c814",
        "970f2809403e2b6a5f06c10ab3f663a2988948bc057738c61fdf44ccf29d4562",
        "38d8cc66921c0b94cefb122240d9a049954db2070ab094d852961087dfcdd1bf",
    ];
    let records = records();
    assert_eq!(records.len(), expected.len());
    for (record, expected) in records.into_iter().zip(expected) {
        assert_eq!(
            hex::encode(crypto::rust::hash::blake2b256::Blake2b256::hash(
                bincode::serialize(&record).unwrap()
            )),
            expected
        );
    }
}

#[test]
fn nonfirst_legacy_primary_cannot_enter_a_lossy_body_or_serde_adapter() {
    let legacy = compound_legacy();
    let mut signers = legacy.signers().to_vec();
    signers.rotate_left(1);
    let original_primary = signers[0].sig.clone();
    let wire =
        bincode::serialize(&(body(), signers, 0u32, CosignedSigningDomain::LegacyPayload)).unwrap();
    let reordered: Cosigned<DeployData> = bincode::deserialize(&wire).unwrap();
    let envelope = DeployEnvelope::from_body_envelope(reordered).unwrap();
    assert_eq!(envelope.identity().as_bytes(), original_primary.as_ref());
    assert_eq!(envelope.primary().sig, original_primary);
    assert!(envelope.body_envelope().is_err());
    let pending = PendingDeploy::from_envelope(envelope).unwrap();
    assert!(pending.clone().into_body_envelope().is_err());
    assert!(bincode::serialize(&pending).is_err());
    assert_eq!(
        pending.into_envelope().to_proto().unwrap().sig,
        original_primary
    );
}

#[test]
fn many_member_pending_records_reverify_on_repeated_reads() {
    for count in [3, 17, 65] {
        let original = record(threshold_envelope(count));
        let bytes = bincode::serialize(&original).unwrap();
        let started = std::time::Instant::now();
        for _ in 0..8 {
            let pending: PendingDeploy = bincode::deserialize(&bytes).unwrap();
            assert_eq!(pending.envelope().signers().len(), usize::from(count));
            assert!(pending.envelope().signers()[0].sig.is_empty());
            assert_eq!(
                pending.envelope().body_envelope().unwrap(),
                &original.envelope
            );
            assert_eq!(bincode::serialize(&pending).unwrap(), bytes);
        }
        eprintln!(
            "pending_decode members={count} reads=8 encoded_bytes={} elapsed_ms={}",
            bytes.len(),
            started.elapsed().as_millis()
        );
    }
}

#[tokio::test]
async fn historical_many_member_records_survive_lmdb_reopen_and_repeated_reads() {
    let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/block-storage-test-scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let directory = tempfile::Builder::new()
        .prefix("pending-many-members-")
        .tempdir_in(scratch)
        .unwrap();
    let manager = || {
        LmdbDirStoreManager::new(
            directory.path().to_path_buf(),
            std::collections::HashMap::from([(
                Db::new("rejected_deploy_buffer".to_string(), None),
                LmdbEnvConfig::new("pending-members".to_string(), 16 << 20).with_max_dbs(2),
            )]),
        )
    };
    let records: Vec<_> = [3, 17, 65]
        .into_iter()
        .map(|count| record(threshold_envelope(count)))
        .collect();
    let original: Vec<_> = records
        .iter()
        .map(|record| {
            (
                bincode::serialize(&record.deploy_id).unwrap(),
                bincode::serialize(record).unwrap(),
            )
        })
        .collect();
    {
        let mut manager = manager();
        let store = manager
            .store("rejected_deploy_buffer".to_string())
            .await
            .unwrap();
        store.put(original.clone()).unwrap();
        drop(store);
        manager.shutdown().await.unwrap();
    }
    for _ in 0..2 {
        let mut manager = manager();
        let started = std::time::Instant::now();
        let buffer = KeyValueRejectedDeployBuffer::new(&mut manager)
            .await
            .unwrap();
        let reopen_ms = started.elapsed().as_millis();
        assert_eq!(buffer.read_all().unwrap().len(), records.len());
        for record in &records {
            let started = std::time::Instant::now();
            for _ in 0..8 {
                let pending = buffer.get_by_id(&record.deploy_id).unwrap().unwrap();
                assert_eq!(pending.typed_deploy_id(), &record.deploy_id);
                assert_eq!(
                    pending.envelope().body_envelope().unwrap(),
                    &record.envelope
                );
                assert_eq!(
                    bincode::serialize(&pending).unwrap(),
                    bincode::serialize(record).unwrap()
                );
            }
            eprintln!(
                "pending_lmdb members={} reads=8 elapsed_ms={} reopen_all_ms={reopen_ms}",
                record.envelope.signers().len(),
                started.elapsed().as_millis(),
            );
        }
        assert_eq!(
            buffer.store.raw_store().to_map().unwrap(),
            original.iter().cloned().collect(),
        );
        drop(buffer);
        manager.shutdown().await.unwrap();
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn pending_repeated_restore_preserves_immutable_identity_and_body(
        timestamp in 0i64..i64::MAX,
        valid_after in 0i64..i64::MAX,
        term in "[a-zA-Z0-9 ]{0,64}",
        bound in any::<bool>(),
        restores in 1usize..6,
    ) {
        let mut data = body();
        data.time_stamp = timestamp;
        data.valid_after_block_number = valid_after;
        data.term = term;
        let key = PrivateKey::from_bytes(&[1; 32]);
        let envelope = if bound {
            Cosigned::create_single_envelope(data, Box::new(Secp256k1), key).unwrap()
        } else {
            Cosigned::from_single_signer(Signed::create(data, Box::new(Secp256k1), key).unwrap()).unwrap()
        };
        let original = record(envelope);
        let bytes = bincode::serialize(&original).unwrap();
        for _ in 0..restores {
            let pending: PendingDeploy = bincode::deserialize(&bytes).unwrap();
            prop_assert_eq!(pending.typed_deploy_id(), &original.deploy_id);
            prop_assert_eq!(pending.deploy_id(), &original.deploy_id_bytes);
            prop_assert_eq!(pending.envelope().body_envelope().unwrap(), &original.envelope);
            prop_assert_eq!(bincode::serialize(&pending).unwrap(), bytes.clone());
            let active_protocol = if bound { 6 } else { 5 };
            let other_protocol = if bound { 5 } else { 6 };
            prop_assert!(pending.validate_for_protocol(active_protocol).is_ok());
            prop_assert!(pending.validate_for_protocol(other_protocol).is_err());
        }
    }
}

#[test]
fn historical_pending_decode_rejects_mismatched_identity_and_signature() {
    for record in records() {
        let mut bad_key = record.clone();
        bad_key.deploy_id = DeployLookupId::Legacy(LegacyDeploySignature::new(vec![7; 32]));
        let mut bad_bytes = record.clone();
        bad_bytes.deploy_id_bytes = Bytes::from_static(b"wrong identity");
        let mut bad_signature = record;
        bad_signature.envelope.data.term = "new invalid in { invalid!(0) }".to_string();
        for malformed in [bad_key, bad_bytes, bad_signature] {
            let bytes = bincode::serialize(&malformed).unwrap();
            assert!(bincode::deserialize::<PendingDeploy>(&bytes).is_err());
        }
    }
}
