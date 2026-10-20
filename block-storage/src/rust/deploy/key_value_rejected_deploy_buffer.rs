// Per-node local buffer of deploys rejected during multi-parent merge.
//
// When the merge algorithm drops a deploy from the canonical merged state,
// its data is placed here so the block creator can re-propose it in a
// subsequent block. Each validator maintains its own buffer; there is no
// cross-validator coordination.
//
// Mirrors KeyValueDeployStorage in shape and storage backing.

use std::collections::HashSet;
use std::sync::Arc;

use crypto::rust::signatures::signed::Signed;
use models::casper::DeployDataProto;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::casper::protocol::legacy_checked_deploy::CheckedLegacyDeploy;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeLimits, StoredDeployEnvelope};
use models::rust::deploy_id::{DeployIdV6, DeployLookupId};
use prost::Message;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::ByteString;

#[derive(Clone)]
pub struct KeyValueRejectedDeployBuffer {
    pub store: KeyValueTypedStoreImpl<ByteString, Signed<DeployData>>,
    checked_store: KeyValueTypedStoreImpl<ByteString, StoredCheckedLegacyDeploy>,
    pub envelope_store: KeyValueTypedStoreImpl<ByteString, StoredDeployEnvelope>,
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
        let envelope_kv_store = kvm.store("rejected_envelope_buffer".to_string()).await?;
        Ok(Self {
            store: buffer_db,
            checked_store: KeyValueTypedStoreImpl::new(checked_kv_store),
            envelope_store: KeyValueTypedStoreImpl::new(envelope_kv_store),
        })
    }

    /// Added by the merge of feature/cost-accounted-rho (finding R3-12): the
    /// buffer over three existing stores, for a caller that has stores and no
    /// store manager. The checked store stays private, so the caller passes
    /// untyped stores.
    pub fn from_stores(
        store: Arc<dyn KeyValueStore>,
        checked_store: Arc<dyn KeyValueStore>,
        envelope_store: Arc<dyn KeyValueStore>,
    ) -> Self {
        Self {
            store: KeyValueTypedStoreImpl::new(store),
            checked_store: KeyValueTypedStoreImpl::new(checked_store),
            envelope_store: KeyValueTypedStoreImpl::new(envelope_store),
        }
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
        let exists = self
            .store
            .contains(vec![key.clone()])?
            .into_iter()
            .next()
            .unwrap_or(false)
            || self
                .checked_store
                .contains(vec![key.clone()])?
                .into_iter()
                .next()
                .unwrap_or(false);
        if !exists {
            // Changed by DR-116 (gap G6): a lookup identity can also name a
            // buffered protocol-6 envelope.
            // return Ok(false);
            return match DeployIdV6::try_from(sig) {
                Ok(deploy_id) => self.remove_envelope_by_id(&deploy_id),
                Err(_) => Ok(false),
            };
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
                .unwrap_or(false)
            || match DeployIdV6::try_from(sig) {
                Ok(deploy_id) => self
                    .envelope_store
                    .contains(vec![deploy_id.as_ref().to_vec()])?
                    .into_iter()
                    .next()
                    .unwrap_or(false),
                Err(_) => false,
            })
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
        Ok(self.store.non_empty()?
            || self.checked_store.non_empty()?
            || self.envelope_store.non_empty()?)
    }

    /// Added by DR-116 (gap G6): buffers rejected protocol-6 offered envelopes
    /// under their deploy identity. A put overwrites, so populate stays
    /// idempotent.
    pub fn add_envelopes(&mut self, envelopes: &[DeployEnvelope]) -> Result<(), KvStoreError> {
        let mut entries = Vec::with_capacity(envelopes.len());
        for envelope in envelopes {
            let DeployLookupId::V6(deploy_id) = envelope.identity() else {
                return Err(KvStoreError::InvalidArgument(
                    "legacy deploys belong in the legacy rejected-deploy table".to_string(),
                ));
            };
            let stored =
                StoredDeployEnvelope::new(envelope).map_err(KvStoreError::SerializationError)?;
            entries.push((deploy_id.as_ref().to_vec(), stored));
        }
        self.envelope_store.put(entries)
    }

    /// Added by DR-116: every buffered offered envelope, sorted by identity. A
    /// row whose key differs from its envelope identity fails closed.
    pub fn read_all_envelopes(
        &self,
        limits: DeployEnvelopeLimits,
    ) -> Result<Vec<DeployEnvelope>, KvStoreError> {
        let rows = self.envelope_store.to_map()?;
        let mut envelopes = Vec::with_capacity(rows.len());
        for (key, stored) in rows {
            let deploy_id = DeployIdV6::try_from(key.as_slice())
                .map_err(|error| KvStoreError::SerializationError(error.to_string()))?;
            let envelope = stored
                .decode(&DeployLookupId::V6(deploy_id), limits)
                .map_err(KvStoreError::SerializationError)?;
            envelopes.push(envelope);
        }
        envelopes
            .sort_by(|left, right| left.identity().as_bytes().cmp(right.identity().as_bytes()));
        Ok(envelopes)
    }

    /// Added by DR-116: removes one buffered offered envelope and reports
    /// whether it was present.
    pub fn remove_envelope_by_id(&mut self, deploy_id: &DeployIdV6) -> Result<bool, KvStoreError> {
        let key: ByteString = deploy_id.as_ref().to_vec();
        let exists = self
            .envelope_store
            .contains(vec![key.clone()])?
            .into_iter()
            .next()
            .unwrap_or(false);
        if !exists {
            return Ok(false);
        }
        self.envelope_store.delete(vec![key])?;
        Ok(true)
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
        // Changed by the merge of feature/cost-accounted-rho (clippy gate):
        // the binding is never mutated.
        // let mut buffer = buffer().await;
        let buffer = buffer().await;
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

    // DR-116 (gap G6): the envelope table of the buffer.
    use crate::rust::deploy::key_value_deploy_storage::tests::{body_envelope, envelope_limits};

    #[tokio::test]
    async fn envelope_table_keeps_legacy_rows_and_round_trips_v6_envelopes() {
        let mut kvm = InMemoryStoreManager::new();
        let mut buffer = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
        let legacy = deploy(5);
        buffer.add(vec![legacy.clone()]).unwrap();
        let envelope = body_envelope();
        let DeployLookupId::V6(id) = envelope.identity() else {
            panic!("a body envelope has a v6 identity")
        };
        let id = *id;

        buffer
            .add_envelopes(std::slice::from_ref(&envelope))
            .unwrap();
        buffer
            .add_envelopes(std::slice::from_ref(&envelope))
            .unwrap();
        assert_eq!(buffer.read_all_envelopes(envelope_limits()).unwrap(), vec![
            envelope.clone()
        ]);
        assert_eq!(buffer.read_all().unwrap(), HashSet::from([legacy.clone()]));

        let reopened = KeyValueRejectedDeployBuffer::new(&mut kvm).await.unwrap();
        assert_eq!(
            reopened.read_all_envelopes(envelope_limits()).unwrap(),
            vec![envelope]
        );
        assert_eq!(
            reopened.read_all().unwrap(),
            HashSet::from([legacy.clone()])
        );

        assert!(buffer.remove_envelope_by_id(&id).unwrap());
        assert!(!buffer.remove_envelope_by_id(&id).unwrap());
        assert!(buffer
            .read_all_envelopes(envelope_limits())
            .unwrap()
            .is_empty());
        assert!(buffer.contains_sig(&legacy.sig).unwrap());
    }

    /// DR-116 (gap G6): the identity lookups take a deploy identity. A 32-byte
    /// key names the envelope table.
    #[tokio::test]
    async fn the_identity_lookups_name_a_buffered_envelope_by_its_deploy_id() {
        let mut buffer = buffer().await;
        let legacy = deploy(5);
        buffer.add(vec![legacy.clone()]).unwrap();
        let envelope = body_envelope();
        let DeployLookupId::V6(id) = envelope.identity() else {
            panic!("a body envelope has a v6 identity")
        };
        let id = *id;
        buffer
            .add_envelopes(std::slice::from_ref(&envelope))
            .unwrap();
        assert!(buffer.contains_sig(id.as_ref()).unwrap());
        assert!(!buffer.contains_sig(&[7; 32]).unwrap());
        assert!(buffer.remove_by_sig(id.as_ref()).unwrap());
        assert!(!buffer.contains_sig(id.as_ref()).unwrap());
        assert!(!buffer.remove_by_sig(id.as_ref()).unwrap());
        assert!(buffer.contains_sig(&legacy.sig).unwrap());
    }

    #[tokio::test]
    async fn an_envelope_stored_under_another_identity_fails_closed() {
        let buffer = buffer().await;
        let stored = StoredDeployEnvelope::new(&body_envelope()).unwrap();
        buffer
            .envelope_store
            .put(vec![(vec![7; 32], stored)])
            .unwrap();
        assert!(buffer.read_all_envelopes(envelope_limits()).is_err());
    }
}
