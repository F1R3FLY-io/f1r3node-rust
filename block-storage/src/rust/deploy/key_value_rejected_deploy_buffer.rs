// Per-node local buffer of deploys rejected during multi-parent merge.
//
// When the merge algorithm drops a deploy from the canonical merged state,
// its data is placed here so the block creator can re-propose it in a
// subsequent block. Each validator maintains its own buffer; there is no
// cross-validator coordination.
//
// Mirrors KeyValueDeployStorage in shape and storage backing.

use std::collections::HashSet;

use crypto::rust::signatures::signed::Signed;
use models::casper::DeployDataProto;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::casper::protocol::legacy_checked_deploy::CheckedLegacyDeploy;
use prost::Message;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::ByteString;

#[derive(Clone)]
pub struct KeyValueRejectedDeployBuffer {
    pub store: KeyValueTypedStoreImpl<ByteString, Signed<DeployData>>,
    // Separate keyspace retains the exact old Signed bincode representation.
    // A checked multi-signer retry is encoded as protobuf bytes and verified
    // again on read; never deserialize an unchecked Cosigned value directly.
    checked_store: KeyValueTypedStoreImpl<ByteString, StoredCheckedLegacyDeploy>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct StoredCheckedLegacyDeploy {
    wire: Vec<u8>,
}

impl KeyValueRejectedDeployBuffer {
    pub async fn new(kvm: &mut impl KeyValueStoreManager) -> Result<Self, KvStoreError> {
        let buffer_kv_store = kvm.store("rejected_deploy_buffer".to_string()).await?;
        let buffer_db: KeyValueTypedStoreImpl<ByteString, Signed<DeployData>> =
            KeyValueTypedStoreImpl::new(buffer_kv_store);
        let checked_kv_store = kvm
            .store("rejected_deploy_buffer_checked_v1".to_string())
            .await?;
        Ok(Self {
            store: buffer_db,
            checked_store: KeyValueTypedStoreImpl::new(checked_kv_store),
        })
    }

    pub fn add(&mut self, deploys: Vec<Signed<DeployData>>) -> Result<(), KvStoreError> {
        self.store.put(
            deploys
                .into_iter()
                .map(|d| (d.sig.clone().into(), d))
                .collect(),
        )
    }

    pub fn add_checked(&mut self, deploys: Vec<CheckedLegacyDeploy>) -> Result<(), KvStoreError> {
        self.checked_store.put(
            deploys
                .into_iter()
                .map(|deploy| {
                    (
                        deploy.identity_bytes().to_vec(),
                        StoredCheckedLegacyDeploy {
                            wire: deploy.to_proto().encode_to_vec(),
                        },
                    )
                })
                .collect(),
        )
    }

    fn decode_checked(
        stored: StoredCheckedLegacyDeploy,
    ) -> Result<CheckedLegacyDeploy, KvStoreError> {
        let wire = DeployDataProto::decode(stored.wire.as_slice())
            .map_err(|error| KvStoreError::SerializationError(error.to_string()))?;
        CheckedLegacyDeploy::from_processed_proto(wire).map_err(KvStoreError::SerializationError)
    }

    pub fn get_checked_by_sig(
        &self,
        sig: &[u8],
    ) -> Result<Option<CheckedLegacyDeploy>, KvStoreError> {
        let key: ByteString = sig.to_vec();
        let decoded = self
            .checked_store
            .get_one(&key)?
            .map(Self::decode_checked)
            .transpose()?;
        if decoded
            .as_ref()
            .is_some_and(|deploy| deploy.identity_bytes() != sig)
        {
            return Err(KvStoreError::SerializationError(
                "checked rejected-deploy key does not match verified identity".to_string(),
            ));
        }
        Ok(decoded)
    }

    pub fn read_checked_all(&self) -> Result<Vec<CheckedLegacyDeploy>, KvStoreError> {
        self.checked_store
            .to_map()?
            .into_iter()
            .map(|(key, stored)| {
                let deploy = Self::decode_checked(stored)?;
                if deploy.identity_bytes() != key.as_slice() {
                    return Err(KvStoreError::SerializationError(
                        "checked rejected-deploy key does not match verified identity".to_string(),
                    ));
                }
                Ok(deploy)
            })
            .collect()
    }

    pub fn remove(&mut self, deploys: Vec<Signed<DeployData>>) -> Result<(), KvStoreError> {
        let keys: Vec<ByteString> = deploys.into_iter().map(|d| d.sig.to_vec()).collect();
        self.store.delete(keys.clone())?;
        self.checked_store.delete(keys)
    }

    pub fn remove_by_sig(&mut self, sig: &[u8]) -> Result<bool, KvStoreError> {
        let key: ByteString = sig.to_vec();
        let exists = self.contains_sig(sig)?;
        if !exists {
            return Ok(false);
        }
        self.store.delete(vec![key.clone()])?;
        self.checked_store.delete(vec![key])?;
        Ok(true)
    }

    pub fn contains_sig(&self, sig: &[u8]) -> Result<bool, KvStoreError> {
        let key: ByteString = sig.to_vec();
        Ok(self
            .store
            .contains(vec![key.clone()])?
            .into_iter()
            .next()
            .unwrap_or(false)
            || self
                .checked_store
                .contains(vec![key])?
                .into_iter()
                .next()
                .unwrap_or(false))
    }

    pub fn get_by_sig(&self, sig: &[u8]) -> Result<Option<Signed<DeployData>>, KvStoreError> {
        let key: ByteString = sig.to_vec();
        if let Some(checked) = self.get_checked_by_sig(sig)? {
            return Ok(Some(checked.execution_projection()));
        }
        let results = self.store.get(&vec![key])?;
        Ok(results.into_iter().next().flatten())
    }

    pub fn read_all(&self) -> Result<HashSet<Signed<DeployData>>, KvStoreError> {
        let mut deploys: HashSet<Signed<DeployData>> = self.store.to_map()?.into_values().collect();
        for checked in self.read_checked_all()? {
            deploys.insert(checked.execution_projection());
        }
        Ok(deploys)
    }

    pub fn non_empty(&self) -> Result<bool, KvStoreError> {
        Ok(self.store.non_empty()? || self.checked_store.non_empty()?)
    }
}

#[cfg(test)]
mod tests {
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use models::casper::CompoundSigner;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::*;

    fn deploy(time_stamp: i64) -> Signed<DeployData> {
        Signed::create(
            DeployData {
                term: "Nil".to_string(),
                time_stamp,
                phlo_price: 1,
                phlo_limit: 100_000,
                valid_after_block_number: 0,
                shard_id: "root".to_string(),
                expiration_timestamp: None,
            },
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap()
    }

    async fn buffer() -> KeyValueRejectedDeployBuffer {
        let mut kvm = InMemoryStoreManager::new();
        KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap()
    }

    #[tokio::test]
    async fn checked_retry_roundtrip_keeps_all_signers_and_old_store_entries() {
        let mut buffer = buffer().await;
        let old = deploy(1);
        let primary = deploy(2);
        let additional = Signed::create(
            primary.data.clone(),
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[2; 32]),
        )
        .unwrap();
        let mut wire = DeployData::to_proto(primary.clone());
        wire.cosigners.push(CompoundSigner {
            pk: additional.pk.bytes.into(),
            sig: additional.sig,
            sig_algorithm: additional.sig_algorithm.name(),
        });
        wire.cosigner_threshold = 2;
        let checked = CheckedLegacyDeploy::from_processed_proto(wire.clone()).unwrap();
        buffer.add(vec![old.clone()]).unwrap();
        buffer.add_checked(vec![checked.clone()]).unwrap();

        assert_eq!(buffer.read_all().unwrap().len(), 2);
        let loaded = buffer
            .get_checked_by_sig(checked.identity_bytes())
            .unwrap()
            .unwrap();
        assert_eq!(loaded.to_proto(), wire);
        assert_eq!(loaded.signers().len(), 2);
        assert_eq!(loaded.threshold(), 2);
        assert_eq!(buffer.read_checked_all().unwrap(), vec![checked.clone()]);

        assert!(buffer.remove_by_sig(checked.identity_bytes()).unwrap());
        assert!(buffer
            .get_checked_by_sig(checked.identity_bytes())
            .unwrap()
            .is_none());
        assert_eq!(buffer.read_all().unwrap(), HashSet::from([old]));
    }

    #[tokio::test]
    async fn checked_retry_store_fails_closed_on_malformed_or_mismatched_wire() {
        let mut buffer = buffer().await;
        buffer
            .checked_store
            .put_one(vec![9], StoredCheckedLegacyDeploy { wire: vec![0xff] })
            .unwrap();
        assert!(buffer.read_checked_all().is_err());
        buffer.checked_store.delete(vec![vec![9]]).unwrap();

        let signed = deploy(3);
        buffer
            .checked_store
            .put_one(vec![9], StoredCheckedLegacyDeploy {
                wire: DeployData::to_proto(signed).encode_to_vec(),
            })
            .unwrap();
        assert!(buffer.read_checked_all().is_err());
        assert!(buffer.get_checked_by_sig(&[9]).is_err());
    }

    #[tokio::test]
    async fn add_read_all_and_non_empty() {
        let mut buffer = buffer().await;
        assert!(!buffer.non_empty().unwrap());

        let (d1, d2) = (deploy(1), deploy(2));
        buffer.add(vec![d1.clone(), d2.clone()]).unwrap();

        assert!(buffer.non_empty().unwrap());
        assert_eq!(buffer.read_all().unwrap(), HashSet::from([d1, d2]));
    }

    #[tokio::test]
    async fn contains_and_get_by_sig() {
        let mut buffer = buffer().await;
        let (d1, d2) = (deploy(1), deploy(2));
        buffer.add(vec![d1.clone()]).unwrap();

        assert!(buffer.contains_sig(&d1.sig).unwrap());
        assert!(!buffer.contains_sig(&d2.sig).unwrap());
        assert_eq!(buffer.get_by_sig(&d1.sig).unwrap(), Some(d1));
        assert_eq!(buffer.get_by_sig(&d2.sig).unwrap(), None);
    }

    #[tokio::test]
    async fn remove_deletes_listed_deploys() {
        let mut buffer = buffer().await;
        let (d1, d2) = (deploy(1), deploy(2));
        buffer.add(vec![d1.clone(), d2.clone()]).unwrap();

        buffer.remove(vec![d2]).unwrap();
        assert_eq!(buffer.read_all().unwrap(), HashSet::from([d1]));
    }

    #[tokio::test]
    async fn remove_by_sig_reports_presence() {
        let mut buffer = buffer().await;
        let d1 = deploy(1);
        buffer.add(vec![d1.clone()]).unwrap();

        assert!(buffer.remove_by_sig(&d1.sig).unwrap());
        assert!(!buffer.remove_by_sig(&d1.sig).unwrap());
        assert!(!buffer.contains_sig(&d1.sig).unwrap());
        assert!(!buffer.non_empty().unwrap());
    }
}
