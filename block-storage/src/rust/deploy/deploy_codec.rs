use crypto::rust::signatures::signed::Signed;
use models::casper::DeployDataProto;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_parameters::validate_parameters;
use prost::Message;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::BitVector;

const PREFIX: &[u8] = b"F1R3DEP\x01";
pub(super) fn encode(deploy: &Signed<DeployData>) -> Result<BitVector, KvStoreError> {
    deploy
        .data
        .validate_amounts()
        .map_err(KvStoreError::SerializationError)?;
    validate_parameters(&deploy.data.parameters)
        .map_err(|error| KvStoreError::SerializationError(error.to_string()))?;
    let mut bytes = PREFIX.to_vec();
    DeployData::to_proto_ref(deploy)
        .encode(&mut bytes)
        .map_err(|error| KvStoreError::SerializationError(error.to_string()))?;
    Ok(bytes)
}

pub(super) fn decode(bytes: &BitVector) -> Result<Signed<DeployData>, KvStoreError> {
    if let Some(payload) = bytes.strip_prefix(PREFIX) {
        let proto = DeployDataProto::decode(payload)
            .map_err(|error| KvStoreError::SerializationError(error.to_string()))?;
        return DeployData::from_proto(proto).map_err(KvStoreError::SerializationError);
    }
    Err(KvStoreError::SerializationError(
        "Unsupported deploy storage format".into(),
    ))
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
                phlo_price: shared::rust::asi_amount::parse("340282366920938463463374607431768211456").unwrap(),
                phlo_limit: 100_000,
                valid_after_block_number: 17,
                shard_id: "root".into(),
                expiration_timestamp: Some(1000),
                parameters,
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
            let proto = DeployDataProto::decode(bytes.strip_prefix(PREFIX).unwrap()).unwrap();
            assert_eq!(proto, DeployData::to_proto_ref(&deploy));
            assert_eq!(decode(&bytes).unwrap(), deploy);
        }
    }

    #[test]
    fn deploy_storage_rejects_tampered_signed_fields() {
        for parameters in [Vec::new(), vec![DeployParameter {
            name: "input".into(),
            value: RholangValue::Int(42),
        }]] {
            let mut deploy = signed(parameters);
            assert_eq!(decode(&encode(&deploy).unwrap()).unwrap(), deploy);
            deploy.data.expiration_timestamp = Some(999);
            let bytes = encode(&deploy).unwrap();
            assert!(bytes.starts_with(PREFIX));
            assert!(matches!(
                decode(&bytes),
                Err(KvStoreError::SerializationError(error)) if error == "Invalid signature"
            ));
        }
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
    fn deploy_parameters_storage_rejects_corrupt_payloads() {
        assert!(decode(&PREFIX.to_vec()).is_err());
        assert!(decode(&vec![255]).is_err());
    }
}
