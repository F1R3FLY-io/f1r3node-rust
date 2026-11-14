use crypto::rust::signatures::signatures_alg::SignaturesAlgFactory;
use crypto::rust::signatures::signed::Signed;
use models::casper::DeployDataProto;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_parameters::validate_parameters;
use prost::Message;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::BitVector;

use super::key_value_deploy_storage::DeployPreparationError;

const PREFIX: &[u8] = b"F1R3DEP\x02";
const FINGERPRINT_LEN: usize = 32;

pub(super) fn encode(deploy: &Signed<DeployData>) -> Result<BitVector, KvStoreError> {
    encode_checked(deploy).map_err(Into::into)
}

pub(super) fn encode_checked(
    deploy: &Signed<DeployData>,
) -> Result<BitVector, DeployPreparationError> {
    deploy.data.validate_amounts().map_err(|error| {
        DeployPreparationError::Serialization(KvStoreError::SerializationError(error))
    })?;
    validate_parameters(&deploy.data.parameters).map_err(|error| {
        DeployPreparationError::Serialization(KvStoreError::SerializationError(error.to_string()))
    })?;
    let algorithm = deploy.sig_algorithm.name();
    if SignaturesAlgFactory::apply(&algorithm).is_none() {
        return Err(DeployPreparationError::UnsupportedAlgorithm(algorithm));
    }
    verify_signature(deploy).map_err(|_| DeployPreparationError::InvalidSignature)?;
    Ok(encode_verified(deploy))
}

fn encode_verified(deploy: &Signed<DeployData>) -> BitVector {
    let payload = DeployData::to_proto_ref(deploy).encode_to_vec();
    let fingerprint = Blake2b256Hash::new(&payload);
    let mut bytes = Vec::with_capacity(PREFIX.len() + FINGERPRINT_LEN + payload.len());
    bytes.extend_from_slice(PREFIX);
    bytes.extend_from_slice(&fingerprint.0);
    bytes.extend_from_slice(&payload);
    bytes
}

pub(super) fn is_current(bytes: &[u8]) -> bool { bytes.starts_with(PREFIX) }

pub(super) fn checked_payload(bytes: &[u8]) -> Result<&[u8], KvStoreError> {
    let body = bytes.strip_prefix(PREFIX).ok_or_else(|| {
        KvStoreError::SerializationError("Unsupported deploy record format".into())
    })?;
    if body.len() < FINGERPRINT_LEN {
        return Err(KvStoreError::SerializationError(
            "Truncated deploy record fingerprint".into(),
        ));
    }
    let (fingerprint, payload) = body.split_at(FINGERPRINT_LEN);
    if fingerprint != Blake2b256Hash::new(payload).0 {
        return Err(KvStoreError::SerializationError(
            "Deploy record fingerprint mismatch".into(),
        ));
    }
    Ok(payload)
}

pub(super) fn verify_signature(deploy: &Signed<DeployData>) -> Result<(), KvStoreError> {
    if !deploy.verify() {
        return Err(KvStoreError::SerializationError(
            "Invalid deploy signature".to_string(),
        ));
    }
    DeployData::with_complete_cosignatures(deploy.clone())
        .map(|_| ())
        .map_err(KvStoreError::SerializationError)
}

pub(super) fn decode(bytes: &BitVector) -> Result<Signed<DeployData>, KvStoreError> {
    let proto = DeployDataProto::decode(checked_payload(bytes)?)
        .map_err(|error| KvStoreError::SerializationError(error.to_string()))?;
    let deploy =
        DeployData::from_proto_unverified(proto).map_err(KvStoreError::SerializationError)?;
    DeployData::with_complete_cosignatures(deploy).map_err(KvStoreError::SerializationError)
}

#[cfg(test)]
mod tests {
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use models::rust::deploy_parameters::{DeployParameter, RholangValue};
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;
    use crate::rust::deploy::key_value_deploy_storage::{
        KeyValueDeployStorage, KeyValueRejectedDeployBuffer,
    };

    fn signed(parameters: Vec<DeployParameter>) -> Signed<DeployData> {
        Signed::create(
            DeployData {
                term: "Nil".into(),
                time_stamp: 123,
                phlo_price: shared::rust::token_amount::TokenAmount::parse(
                    "340282366920938463463374607431768211456",
                )
                .unwrap(),
                phlo_limit: 100_000,
                valid_after_block_number: 17,
                shard_id: "root".into(),
                expiration_timestamp: Some(1000),
                parameters,
                cosignatures: Vec::new(),
                required_cosigners: Vec::new(),
            },
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap()
    }

    #[test]
    fn deploy_storage_always_writes_versioned_protobuf() {
        for parameters in [Vec::new(), vec![DeployParameter {
            name: "input".into(),
            value: RholangValue::Int(42),
        }]] {
            let deploy = signed(parameters);
            let bytes = encode(&deploy).unwrap();
            let proto = DeployDataProto::decode(checked_payload(&bytes).unwrap()).unwrap();
            assert_eq!(proto, DeployData::to_proto_ref(&deploy));
            assert_eq!(decode(&bytes).unwrap(), deploy);
        }
    }

    #[test]
    fn current_records_reject_changed_payloads_checksums_and_truncation() {
        let deploy = signed(Vec::new());
        let bytes = encode(&deploy).unwrap();
        for index in [PREFIX.len(), bytes.len() - 1] {
            let mut changed = bytes.clone();
            changed[index] ^= 1;
            assert!(decode(&changed).is_err());
        }
        for length in 0..bytes.len() {
            assert!(decode(&bytes[..length].to_vec()).is_err());
        }
        let mut unknown = bytes;
        unknown[PREFIX.len() - 1] = 3;
        assert!(decode(&unknown).is_err());
    }

    #[test]
    fn deploy_storage_verifies_signatures_on_write_and_not_on_read() {
        for parameters in [Vec::new(), vec![DeployParameter {
            name: "input".into(),
            value: RholangValue::Int(42),
        }]] {
            let mut deploy = signed(parameters);
            assert_eq!(decode(&encode(&deploy).unwrap()).unwrap(), deploy);
            deploy.data.expiration_timestamp = Some(999);
            assert!(encode(&deploy).is_err());
            let bytes = encode_verified(&deploy);
            assert_eq!(decode(&bytes).unwrap(), deploy);
        }
    }

    #[test]
    fn encode_rejects_a_valid_signature_with_an_unreadable_algorithm() {
        let deploy = Signed::create(
            signed(Vec::new()).data,
            Box::new(crypto::rust::signatures::ed25519::Ed25519),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap();
        assert!(deploy.verify());
        assert!(matches!(
            encode(&deploy),
            Err(KvStoreError::SerializationError(message))
                if message == "Unknown signature algorithm: ed25519"
        ));
    }

    #[tokio::test]
    async fn deploy_parameters_survive_storage_and_rejected_buffer_reopen() {
        let mut kvm = InMemoryStoreManager::new();
        let parameterized = signed(vec![DeployParameter {
            name: "items".into(),
            value: RholangValue::Tuple(vec![RholangValue::Nil, RholangValue::Bytes(vec![0, 255])]),
        }]);
        let old = signed(Vec::new());
        let stored_bytes = encode(&old).unwrap();
        let key = bincode::serialize(&old.sig.to_vec()).unwrap();
        for name in ["deploy_storage", "rejected_deploy_buffer"] {
            kvm.store(name.into())
                .await
                .unwrap()
                .put_one(key.clone(), stored_bytes.clone())
                .unwrap();
        }
        let storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
        storage.add(vec![parameterized.clone()]).unwrap();
        buffer.add(vec![parameterized.clone()]).unwrap();
        drop(storage);
        drop(buffer);
        let storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
        let expected = std::collections::HashSet::from([old.clone(), parameterized]);
        assert_eq!(storage.read_all().unwrap(), expected);
        assert_eq!(buffer.read_all().unwrap(), expected);
        storage.add(vec![old.clone()]).unwrap();
        buffer.add(vec![old]).unwrap();
        for name in ["deploy_storage", "rejected_deploy_buffer"] {
            let values = kvm
                .store(name.into())
                .await
                .unwrap()
                .get(&vec![key.clone()])
                .unwrap();
            assert!(values[0].as_ref().unwrap().starts_with(PREFIX));
        }
        drop(storage);
        drop(buffer);
        let storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
        assert_eq!(storage.read_all().unwrap(), expected);
        assert_eq!(buffer.read_all().unwrap(), expected);
    }

    #[test]
    fn records_from_nodes_with_int64_prices_are_unreadable() {
        let fingerprinted = |payload: &[u8]| {
            let mut bytes = PREFIX.to_vec();
            bytes.extend_from_slice(&Blake2b256Hash::new(payload).0);
            bytes.extend_from_slice(payload);
            bytes
        };
        let deploy = signed(Vec::new());
        let mut proto = DeployData::to_proto_ref(&deploy);
        assert_eq!(
            decode(&fingerprinted(&proto.encode_to_vec())).unwrap(),
            deploy
        );
        proto.phlo_price = Default::default();
        let mut payload = proto.encode_to_vec();
        payload.extend_from_slice(&[7 << 3, 1]);
        let mut unfingerprinted = b"F1R3DEP\x01".to_vec();
        unfingerprinted.extend_from_slice(&payload);
        assert!(decode(&fingerprinted(&payload)).is_err());
        assert!(decode(&unfingerprinted).is_err());
    }

    #[test]
    fn deploy_parameters_storage_rejects_corrupt_payloads() {
        assert!(decode(&PREFIX.to_vec()).is_err());
        assert!(decode(&vec![255]).is_err());
    }
}
