use crypto::rust::signatures::signed::Signed;
use models::casper::DeployDataProto;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_parameters::validate_parameters;
use prost::Message;
use serde::{Deserialize, Serialize};
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::BitVector;

const PREFIX: &[u8] = b"F1R3DEP\x01";

#[derive(Serialize, Deserialize)]
struct LegacyDeployData {
    term: String,
    time_stamp: i64,
    phlo_price: i64,
    phlo_limit: i64,
    valid_after_block_number: i64,
    shard_id: String,
    expiration_timestamp: Option<i64>,
}

pub(super) fn encode(deploy: &Signed<DeployData>) -> Result<BitVector, KvStoreError> {
    validate_parameters(&deploy.data.parameters)
        .map_err(|error| KvStoreError::SerializationError(error.to_string()))?;
    if deploy.data.parameters.is_empty() {
        let data = &deploy.data;
        return Ok(bincode::serialize(&Signed {
            data: LegacyDeployData {
                term: data.term.clone(),
                time_stamp: data.time_stamp,
                phlo_price: data.phlo_price,
                phlo_limit: data.phlo_limit,
                valid_after_block_number: data.valid_after_block_number,
                shard_id: data.shard_id.clone(),
                expiration_timestamp: data.expiration_timestamp,
            },
            pk: deploy.pk.clone(),
            sig: deploy.sig.clone(),
            sig_algorithm: deploy.sig_algorithm.clone(),
        })?);
    }
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
    let legacy: Signed<LegacyDeployData> = bincode::deserialize(bytes)?;
    Ok(Signed {
        data: DeployData {
            term: legacy.data.term,
            time_stamp: legacy.data.time_stamp,
            phlo_price: legacy.data.phlo_price,
            phlo_limit: legacy.data.phlo_limit,
            valid_after_block_number: legacy.data.valid_after_block_number,
            shard_id: legacy.data.shard_id,
            expiration_timestamp: legacy.data.expiration_timestamp,
            parameters: Vec::new(),
        },
        pk: legacy.pk,
        sig: legacy.sig,
        sig_algorithm: legacy.sig_algorithm,
    })
}

#[cfg(test)]
mod tests {
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use models::rust::deploy_parameters::{DeployParameter, RholangValue};
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;
    use crate::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
    use crate::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;

    fn signed(parameters: Vec<DeployParameter>) -> Signed<DeployData> {
        Signed::create(
            DeployData {
                term: "Nil".into(),
                time_stamp: 1,
                phlo_price: 1,
                phlo_limit: 100_000,
                valid_after_block_number: 0,
                shard_id: "root".into(),
                expiration_timestamp: Some(1000),
                parameters,
            },
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn deploy_parameters_survive_storage_and_rejected_buffer_reopen() {
        let mut kvm = InMemoryStoreManager::new();
        let parameterized = signed(vec![DeployParameter {
            name: "items".into(),
            value: RholangValue::Tuple(vec![RholangValue::Nil, RholangValue::Bytes(vec![0, 255])]),
        }]);
        let old = signed(Vec::new());
        let legacy_bytes = bincode::serialize(&Signed {
            data: LegacyDeployData {
                term: "Nil".into(),
                time_stamp: 1,
                phlo_price: 1,
                phlo_limit: 100_000,
                valid_after_block_number: 0,
                shard_id: "root".into(),
                expiration_timestamp: Some(1000),
            },
            pk: old.pk.clone(),
            sig: old.sig.clone(),
            sig_algorithm: old.sig_algorithm.clone(),
        })
        .unwrap();
        assert_eq!(encode(&old).unwrap(), legacy_bytes);
        let key = bincode::serialize(&old.sig.to_vec()).unwrap();
        for name in ["deploy_storage", "rejected_deploy_buffer"] {
            kvm.store(name.into())
                .await
                .unwrap()
                .put_one(key.clone(), legacy_bytes.clone())
                .unwrap();
        }
        let mut storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let mut buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
        storage.add(vec![parameterized.clone()]).unwrap();
        buffer.add(vec![parameterized.clone()]).unwrap();
        drop(storage);
        drop(buffer);
        let storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
        let expected = std::collections::HashSet::from([old, parameterized]);
        assert_eq!(storage.read_all().unwrap(), expected);
        assert_eq!(buffer.read_all().unwrap(), expected);
    }

    #[test]
    fn deploy_parameters_storage_rejects_corrupt_payloads() {
        assert!(decode(&PREFIX.to_vec()).is_err());
        assert!(decode(&vec![255]).is_err());
    }
}
