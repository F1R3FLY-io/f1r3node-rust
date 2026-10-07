use crypto::rust::signatures::signed::Signed;
use models::casper::DeployDataProto;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_parameters::validate_parameters;
use prost::Message;
use serde::Deserialize;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::BitVector;

const PREFIX: &[u8] = b"F1R3DEP\x01";
const LEGACY_DECODE_METRIC: &str = "deploy.storage.legacy_decode";

#[derive(Deserialize)]
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
    metrics::counter!(LEGACY_DECODE_METRIC).increment(1);
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
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};
    use models::rust::deploy_parameters::{DeployParameter, RholangValue};
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;
    use crate::rust::deploy::key_value_deploy_storage::KeyValueDeployStorage;
    use crate::rust::deploy::key_value_rejected_deploy_buffer::KeyValueRejectedDeployBuffer;

    const LEGACY_DEPLOY_369DBFCC4: &str = concat!(
        "03000000000000004e696c7b000000000000000700000000000000a086010000",
        "00000011000000000000000400000000000000726f6f7401e803000000000000",
        "4100000000000000041b84c5567b126440995d3ed5aaba0565d71e1834604819",
        "ff9c17f5e9d5dd078f70beaf8f588b541507fed6a642c5ab42dfdf8120a7f639",
        "de5122d47a69a8e8d14700000000000000304502210092526253b63faa274731",
        "a1907908ded5bb5e46fea2d28464985ca4bf3ec3c4b802206f05828da0310cfd",
        "c5e7da0cd34385559b5f6d4cf3b25ca78176416ecf6fadb50900000000000000",
        "736563703235366b31",
    );

    fn signed(parameters: Vec<DeployParameter>) -> Signed<DeployData> {
        Signed::create(
            DeployData {
                term: "Nil".into(),
                time_stamp: 123,
                phlo_price: 7,
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
    fn deploy_storage_decodes_pre_parameters_fixture() {
        let legacy_bytes = hex::decode(LEGACY_DEPLOY_369DBFCC4).unwrap();
        let restored = decode(&legacy_bytes).unwrap();
        assert_eq!(restored, signed(Vec::new()));
        assert!(restored.data.parameters.is_empty());
        let encoded = encode(&restored).unwrap();
        assert!(encoded.starts_with(PREFIX));
        assert_eq!(decode(&encoded).unwrap(), restored);
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
    fn deploy_storage_counts_only_legacy_decode_attempts() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let legacy_reads = || {
            snapshotter
                .snapshot()
                .into_vec()
                .into_iter()
                .filter_map(|(key, _, _, value)| {
                    if key.key().name() == "deploy.storage.legacy_decode" {
                        match value {
                            DebugValue::Counter(count) => Some(count),
                            _ => None,
                        }
                    } else {
                        None
                    }
                })
                .sum::<u64>()
        };
        metrics::with_local_recorder(&recorder, || {
            for parameters in [Vec::new(), vec![DeployParameter {
                name: "input".into(),
                value: RholangValue::Nil,
            }]] {
                let deploy = signed(parameters);
                assert_eq!(decode(&encode(&deploy).unwrap()).unwrap(), deploy);
            }
            assert!(decode(&PREFIX.to_vec()).is_err());
            assert_eq!(legacy_reads(), 0);

            let legacy_bytes = hex::decode(LEGACY_DEPLOY_369DBFCC4).unwrap();
            for count in 1..=2 {
                assert_eq!(decode(&legacy_bytes).unwrap(), signed(Vec::new()));
                assert_eq!(legacy_reads(), count);
            }
            assert!(decode(&vec![255]).is_err());
            assert_eq!(legacy_reads(), 3);
        });
    }

    #[tokio::test]
    async fn deploy_parameters_survive_storage_and_rejected_buffer_reopen() {
        let mut kvm = InMemoryStoreManager::new();
        let parameterized = signed(vec![DeployParameter {
            name: "items".into(),
            value: RholangValue::Tuple(vec![RholangValue::Nil, RholangValue::Bytes(vec![0, 255])]),
        }]);
        let old = signed(Vec::new());
        let legacy_bytes = hex::decode(LEGACY_DEPLOY_369DBFCC4).unwrap();
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
        let mut storage = KeyValueDeployStorage::new(&mut kvm).await.unwrap();
        let mut buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
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
