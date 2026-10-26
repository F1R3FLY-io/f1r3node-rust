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
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeLimits, StoredDeployEnvelope};
use models::rust::deploy_id::{DeployIdV6, DeployLookupId};
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::KvStoreError;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::ByteString;

#[derive(Clone)]
pub struct KeyValueRejectedDeployBuffer {
    pub store: KeyValueTypedStoreImpl<ByteString, Signed<DeployData>>,
    /// Added by DR-116 (gap G6): rejected protocol-6 offered envelopes, keyed
    /// by their 32-byte deploy identity. The legacy table keeps its type and
    /// its bytes, so no migration is needed.
    pub envelope_store: KeyValueTypedStoreImpl<ByteString, StoredDeployEnvelope>,
}

impl KeyValueRejectedDeployBuffer {
    pub async fn new(kvm: &mut impl KeyValueStoreManager) -> Result<Self, KvStoreError> {
        let buffer_kv_store = kvm.store("rejected_deploy_buffer".to_string()).await?;
        let buffer_db: KeyValueTypedStoreImpl<ByteString, Signed<DeployData>> =
            KeyValueTypedStoreImpl::new(buffer_kv_store);
        let envelope_kv_store = kvm.store("rejected_envelope_buffer".to_string()).await?;
        Ok(Self {
            store: buffer_db,
            envelope_store: KeyValueTypedStoreImpl::new(envelope_kv_store),
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

    pub fn remove(&mut self, deploys: Vec<Signed<DeployData>>) -> Result<(), KvStoreError> {
        self.store
            .delete(deploys.into_iter().map(|d| d.sig.clone().into()).collect())
    }

    pub fn remove_by_sig(&mut self, sig: &[u8]) -> Result<bool, KvStoreError> {
        let key: ByteString = sig.to_vec();
        let exists = self
            .store
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
        self.store.delete(vec![key])?;
        Ok(true)
    }

    pub fn contains_sig(&self, sig: &[u8]) -> Result<bool, KvStoreError> {
        let key: ByteString = sig.to_vec();
        let exists = self
            .store
            .contains(vec![key])?
            .into_iter()
            .next()
            .unwrap_or(false);
        // Changed by DR-116 (gap G6): a lookup identity can also name a
        // buffered protocol-6 envelope.
        // Ok(exists)
        if exists {
            return Ok(true);
        }
        match DeployIdV6::try_from(sig) {
            Ok(deploy_id) => Ok(self
                .envelope_store
                .contains(vec![deploy_id.as_ref().to_vec()])?
                .into_iter()
                .next()
                .unwrap_or(false)),
            Err(_) => Ok(false),
        }
    }

    pub fn get_by_sig(&self, sig: &[u8]) -> Result<Option<Signed<DeployData>>, KvStoreError> {
        let key: ByteString = sig.to_vec();
        let results = self.store.get(&vec![key])?;
        Ok(results.into_iter().next().flatten())
    }

    pub fn read_all(&self) -> Result<HashSet<Signed<DeployData>>, KvStoreError> {
        self.store.to_map().map(|map| map.into_values().collect())
    }

    pub fn non_empty(&self) -> Result<bool, KvStoreError> { self.store.non_empty() }

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
