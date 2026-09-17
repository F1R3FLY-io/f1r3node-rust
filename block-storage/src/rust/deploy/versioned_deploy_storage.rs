use std::sync::Arc;

use bincode::Options;
use models::rust::deploy_envelope::{
    DeployEnvelope, DeployEnvelopeFormat, DeployEnvelopeLimits, StoredDeployEnvelope,
};
use models::rust::deploy_id::DeployLookupId;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use shared::rust::store::key_value_store::{
    AtomicStoreMutation, AtomicStoreOperation, KeyValueStore, KvStoreError,
};

#[derive(Clone, Copy, Debug)]
pub enum DeployEnvelopeStoreKind {
    Pending,
    Rejected,
}

impl DeployEnvelopeStoreKind {
    pub fn namespace(self) -> &'static str {
        match self {
            Self::Pending => "deploy_authenticated_envelopes_v1",
            Self::Rejected => "rejected_authenticated_envelopes_v1",
        }
    }
}

#[derive(Clone)]
pub struct VersionedDeployStorage {
    store: Arc<dyn KeyValueStore>,
    limits: DeployEnvelopeLimits,
    record_limit: u64,
}

#[derive(Clone)]
pub(super) struct FundedDeployStorage(VersionedDeployStorage);

impl FundedDeployStorage {
    pub async fn new(
        manager: &mut impl KeyValueStoreManager,
        kind: DeployEnvelopeStoreKind,
        limits: DeployEnvelopeLimits,
    ) -> Result<Self, KvStoreError> {
        let store = Self(VersionedDeployStorage::open(manager, kind, limits).await?);
        store.visit(&mut |_| Ok(()))?;
        Ok(store)
    }

    fn check(envelope: &DeployEnvelope) -> Result<(), KvStoreError> {
        envelope
            .require_format(&[
                DeployEnvelopeFormat::Funded,
                DeployEnvelopeFormat::OfferedFunded,
            ])
            .map_err(KvStoreError::InvalidArgument)
    }

    pub fn get(&self, identity: &DeployLookupId) -> Result<Option<DeployEnvelope>, KvStoreError> {
        let envelope = self.0.get(identity)?;
        if let Some(envelope) = &envelope {
            Self::check(envelope)?;
        }
        Ok(envelope)
    }

    pub fn insert_if_absent(&self, envelope: &DeployEnvelope) -> Result<bool, KvStoreError> {
        Self::check(envelope)?;
        self.0.insert_if_absent(envelope)
    }

    pub fn prepare_put(
        &self,
        envelope: &DeployEnvelope,
    ) -> Result<AtomicStoreMutation<'_>, KvStoreError> {
        Self::check(envelope)?;
        Ok(AtomicStoreMutation {
            store: self.0.store.as_ref(),
            key: self.0.encode_key(envelope.identity())?,
            operation: AtomicStoreOperation::Put(self.0.encode_envelope(envelope)?),
        })
    }

    pub fn prepare_delete(
        &self,
        identity: &DeployLookupId,
    ) -> Result<AtomicStoreMutation<'_>, KvStoreError> {
        Ok(AtomicStoreMutation {
            store: self.0.store.as_ref(),
            key: self.0.encode_key(identity)?,
            operation: AtomicStoreOperation::Delete,
        })
    }

    pub fn visit(
        &self,
        visitor: &mut dyn FnMut(DeployEnvelope) -> Result<(), KvStoreError>,
    ) -> Result<(), KvStoreError> {
        self.0.visit_envelopes(&mut |envelope| {
            Self::check(&envelope)?;
            visitor(envelope)
        })
    }

    pub fn remove(&self, identity: &DeployLookupId) -> Result<bool, KvStoreError> {
        self.get(identity)?;
        self.0.remove(identity)
    }

    pub fn non_empty(&self) -> Result<bool, KvStoreError> { self.0.non_empty() }
}

impl VersionedDeployStorage {
    pub async fn new(
        manager: &mut impl KeyValueStoreManager,
        kind: DeployEnvelopeStoreKind,
        limits: DeployEnvelopeLimits,
    ) -> Result<Self, KvStoreError> {
        let result = Self::open(manager, kind, limits).await?;
        result.visit_envelopes(&mut |_| Ok(()))?;
        Ok(result)
    }

    async fn open(
        manager: &mut impl KeyValueStoreManager,
        kind: DeployEnvelopeStoreKind,
        limits: DeployEnvelopeLimits,
    ) -> Result<Self, KvStoreError> {
        let record_limit = limits
            .payload
            .deploy_bytes
            .checked_add(128)
            .and_then(|n| u64::try_from(n).ok())
            .ok_or_else(|| {
                KvStoreError::InvalidArgument("stored deploy byte limit overflow".to_string())
            })?;
        let store = manager.store(kind.namespace().to_string()).await?;
        let result = Self {
            store,
            limits,
            record_limit,
        };
        Ok(result)
    }

    fn decode_row(&self, key: &[u8], value: &[u8]) -> Result<DeployEnvelope, KvStoreError> {
        if key.len() as u128 > u128::from(self.record_limit)
            || value.len() as u128 > u128::from(self.record_limit)
        {
            return Err(KvStoreError::InvalidArgument(
                "stored deploy row exceeds its byte limit".to_string(),
            ));
        }
        let identity: DeployLookupId = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(self.record_limit)
            .reject_trailing_bytes()
            .deserialize(key)?;
        let record: StoredDeployEnvelope = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(self.record_limit)
            .reject_trailing_bytes()
            .deserialize(value)?;
        record
            .decode(&identity, self.limits)
            .map_err(KvStoreError::InvalidArgument)
    }

    pub fn visit_envelopes(
        &self,
        visitor: &mut dyn FnMut(DeployEnvelope) -> Result<(), KvStoreError>,
    ) -> Result<(), KvStoreError> {
        self.store
            .visit_entries(&mut |key, value| visitor(self.decode_row(key, value)?))
    }

    pub fn get(&self, identity: &DeployLookupId) -> Result<Option<DeployEnvelope>, KvStoreError> {
        let key = self.encode_key(identity)?;
        let mut result = None;
        self.store.with_value(&key, &mut |value| {
            result = value
                .map(|value| self.decode_row(&key, value))
                .transpose()?;
            Ok(())
        })?;
        Ok(result)
    }

    fn encode_key(&self, identity: &DeployLookupId) -> Result<Vec<u8>, KvStoreError> {
        Ok(bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(self.record_limit)
            .serialize(identity)?)
    }

    fn encode_envelope(&self, envelope: &DeployEnvelope) -> Result<Vec<u8>, KvStoreError> {
        let record = StoredDeployEnvelope::new(envelope).map_err(KvStoreError::InvalidArgument)?;
        record
            .decode(envelope.identity(), self.limits)
            .map_err(KvStoreError::InvalidArgument)?;
        Ok(bincode::serialize(&record)?)
    }

    pub fn insert_if_absent(&self, envelope: &DeployEnvelope) -> Result<bool, KvStoreError> {
        let inserted = self.store.put_one_if_absent(
            self.encode_key(envelope.identity())?,
            self.encode_envelope(envelope)?,
        )?;
        if !inserted {
            self.get(envelope.identity())?;
        }
        Ok(inserted)
    }

    pub fn remove(&self, identity: &DeployLookupId) -> Result<bool, KvStoreError> {
        Ok(self.store.delete(vec![self.encode_key(identity)?])? != 0)
    }

    pub fn non_empty(&self) -> Result<bool, KvStoreError> { self.store.non_empty() }
}

#[cfg(test)]
mod tests;
