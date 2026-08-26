use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::validator::ValidatorSerde;
use parking_lot::Mutex;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use serde::{Deserialize, Serialize};
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
enum FinalizationLedgerKey {
    Head,
    Round(u64),
    Effect(FinalizationEffectId),
    ProjectionCursor,
    EffectsCursor,
    EffectsComplete(u64),
    EffectsCompactionCursor,
    Genesis,
    Witness(BlockHashSerde),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum FinalizationLedgerValue {
    Head(FinalizationHead),
    Round(FinalizationRecord),
    Effect,
    ProjectionCursor(u64),
    EffectsCursor(u64),
    EffectsComplete,
    EffectsCompactionCursor(u64),
    Genesis(FinalizationGenesisAnchor),
    Witness(LocalFinalizationWitness),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FinalizationHead {
    pub revision: u64,
    pub block_hash: BlockHashSerde,
    pub block_number: i64,
    pub record_digest: BlockHashSerde,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FinalizationGenesisAnchor {
    pub block_hash: BlockHashSerde,
    pub block_number: i64,
    pub record_digest: BlockHashSerde,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnsureGenesisOutcome {
    Initialized,
    AlreadyCanonical,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FinalizationRecord {
    pub revision: u64,
    pub predecessor_hash: BlockHashSerde,
    pub predecessor_digest: BlockHashSerde,
    pub directly_finalized: BlockHashSerde,
    pub block_number: i64,
    pub fault_tolerance_bits: u32,
    pub finalized: BTreeSet<BlockHashSerde>,
    pub manifest_digest: BlockHashSerde,
    pub witness_digest: BlockHashSerde,
    pub record_digest: BlockHashSerde,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LocalFinalizationWitness {
    pub schema_version: u32,
    pub genesis_hash: BlockHashSerde,
    pub predecessor_hash: BlockHashSerde,
    pub predecessor_digest: BlockHashSerde,
    pub target_block_hash: BlockHashSerde,
    pub target_block_number: i64,
    pub target_post_state_hash: BlockHashSerde,
    pub fault_tolerance_numerator: i64,
    pub fault_tolerance_denominator: i64,
    pub latest_messages: BTreeMap<ValidatorSerde, BlockHashSerde>,
    pub supporting_block_hashes: BTreeSet<BlockHashSerde>,
    pub authority_context_digest: BlockHashSerde,
    pub finalized: BTreeSet<BlockHashSerde>,
    pub witness_digest: BlockHashSerde,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum FinalizationEffectKind {
    DeployRemoval,
    CosignerRemoval,
    RuntimeCacheEviction,
    FinalizedEvent,
}

impl FinalizationEffectKind {
    const ALL: [Self; 4] = [
        Self::DeployRemoval,
        Self::CosignerRemoval,
        Self::RuntimeCacheEviction,
        Self::FinalizedEvent,
    ];
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct FinalizationEffectId {
    pub revision: u64,
    pub block_hash: BlockHashSerde,
    pub kind: FinalizationEffectKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FinalizationAppendOutcome {
    Committed(FinalizationHead),
    AlreadyCommitted(FinalizationHead),
    Stale(FinalizationHead),
}

#[derive(Clone)]
pub struct FinalizationLedger {
    store: KeyValueTypedStoreImpl<FinalizationLedgerKey, FinalizationLedgerValue>,
    append_lock: Arc<Mutex<()>>,
}

impl FinalizationLedger {
    pub const STORE_NAME: &'static str = "finalization-ledger-v6";
    pub const WITNESS_SCHEMA_VERSION: u32 = 1;

    pub async fn create_from_kvm(
        kvm: &mut impl KeyValueStoreManager,
    ) -> Result<Self, KvStoreError> {
        let store = kvm.store(Self::STORE_NAME.to_string()).await?;
        Ok(Self::from_store(store))
    }

    fn new(store: KeyValueTypedStoreImpl<FinalizationLedgerKey, FinalizationLedgerValue>) -> Self {
        Self {
            store,
            append_lock: Arc::new(Mutex::new(())),
        }
    }

    pub(crate) fn from_store(store: Arc<dyn KeyValueStore>) -> Self {
        Self::new(KeyValueTypedStoreImpl::new(store))
    }

    fn digest(parts: impl IntoIterator<Item = Vec<u8>>) -> BlockHashSerde {
        let parts = parts.into_iter().collect::<Vec<_>>();
        BlockHashSerde(Bytes::from(Blake2b256::hash_parts(
            parts.iter().map(Vec::as_slice),
        )))
    }

    fn genesis_digest(block_hash: &BlockHash, block_number: i64) -> BlockHashSerde {
        Self::digest([
            b"f1r3-finalization-genesis-v6".to_vec(),
            block_hash.to_vec(),
            block_number.to_be_bytes().to_vec(),
        ])
    }

    fn manifest_digest(finalized: &BTreeSet<BlockHashSerde>) -> BlockHashSerde {
        let mut parts = Vec::with_capacity(finalized.len() + 2);
        parts.push(b"f1r3-finalization-manifest-v6".to_vec());
        parts.push((finalized.len() as u64).to_be_bytes().to_vec());
        parts.extend(finalized.iter().map(|hash| hash.0.to_vec()));
        Self::digest(parts)
    }

    fn record_digest(record: &FinalizationRecord) -> BlockHashSerde {
        let mut parts = Vec::with_capacity(record.finalized.len() + 11);
        parts.push(b"f1r3-finalization-record-v6".to_vec());
        parts.push(record.revision.to_be_bytes().to_vec());
        parts.push(record.predecessor_hash.0.to_vec());
        parts.push(record.predecessor_digest.0.to_vec());
        parts.push(record.directly_finalized.0.to_vec());
        parts.push(record.block_number.to_be_bytes().to_vec());
        parts.push(record.fault_tolerance_bits.to_be_bytes().to_vec());
        parts.push(record.manifest_digest.0.to_vec());
        parts.push(record.witness_digest.0.to_vec());
        parts.extend(record.finalized.iter().map(|hash| hash.0.to_vec()));
        Self::digest(parts)
    }

    fn witness_digest(witness: &LocalFinalizationWitness) -> BlockHashSerde {
        let mut parts = Vec::with_capacity(
            witness.latest_messages.len()
                + witness.supporting_block_hashes.len()
                + witness.finalized.len()
                + 16,
        );
        parts.push(b"f1r3-local-finalization-witness-v1".to_vec());
        parts.push(witness.schema_version.to_be_bytes().to_vec());
        parts.push(witness.genesis_hash.0.to_vec());
        parts.push(witness.predecessor_hash.0.to_vec());
        parts.push(witness.predecessor_digest.0.to_vec());
        parts.push(witness.target_block_hash.0.to_vec());
        parts.push(witness.target_block_number.to_be_bytes().to_vec());
        parts.push(witness.target_post_state_hash.0.to_vec());
        parts.push(witness.fault_tolerance_numerator.to_be_bytes().to_vec());
        parts.push(witness.fault_tolerance_denominator.to_be_bytes().to_vec());
        parts.push(
            (witness.latest_messages.len() as u64)
                .to_be_bytes()
                .to_vec(),
        );
        for (validator, block_hash) in &witness.latest_messages {
            parts.push(validator.0.to_vec());
            parts.push(block_hash.0.to_vec());
        }
        parts.push(
            (witness.supporting_block_hashes.len() as u64)
                .to_be_bytes()
                .to_vec(),
        );
        parts.extend(
            witness
                .supporting_block_hashes
                .iter()
                .map(|hash| hash.0.to_vec()),
        );
        parts.push(witness.authority_context_digest.0.to_vec());
        parts.push((witness.finalized.len() as u64).to_be_bytes().to_vec());
        parts.extend(witness.finalized.iter().map(|hash| hash.0.to_vec()));
        Self::digest(parts)
    }

    pub fn prepare_witness(
        genesis_hash: BlockHash,
        expected: &FinalizationHead,
        target_block_hash: BlockHash,
        target_block_number: i64,
        target_post_state_hash: BlockHash,
        fault_tolerance_numerator: i64,
        fault_tolerance_denominator: i64,
        latest_messages: BTreeMap<ValidatorSerde, BlockHashSerde>,
        supporting_block_hashes: BTreeSet<BlockHashSerde>,
        authority_context_digest: BlockHashSerde,
        finalized: BTreeSet<BlockHashSerde>,
    ) -> Result<LocalFinalizationWitness, KvStoreError> {
        if target_block_number <= expected.block_number
            || fault_tolerance_denominator <= 0
            || !supporting_block_hashes.contains(&BlockHashSerde(target_block_hash.clone()))
            || !finalized.contains(&BlockHashSerde(target_block_hash.clone()))
            || finalized
                .iter()
                .any(|hash| !supporting_block_hashes.contains(hash))
            || latest_messages
                .values()
                .any(|hash| !supporting_block_hashes.contains(hash))
            || authority_context_digest.0.len() != models::rust::block_hash::LENGTH
        {
            return Err(KvStoreError::InvalidArgument(
                "local finalization witness is structurally incomplete".to_string(),
            ));
        }
        let mut witness = LocalFinalizationWitness {
            schema_version: Self::WITNESS_SCHEMA_VERSION,
            genesis_hash: BlockHashSerde(genesis_hash),
            predecessor_hash: expected.block_hash.clone(),
            predecessor_digest: expected.record_digest.clone(),
            target_block_hash: BlockHashSerde(target_block_hash),
            target_block_number,
            target_post_state_hash: BlockHashSerde(target_post_state_hash),
            fault_tolerance_numerator,
            fault_tolerance_denominator,
            latest_messages,
            supporting_block_hashes,
            authority_context_digest,
            finalized,
            witness_digest: BlockHashSerde(Bytes::new()),
        };
        witness.witness_digest = Self::witness_digest(&witness);
        Ok(witness)
    }

    fn validate_witness(
        expected: &FinalizationHead,
        witness: &LocalFinalizationWitness,
    ) -> Result<(), KvStoreError> {
        Self::validate_witness_standalone(witness)?;
        if witness.predecessor_hash != expected.block_hash
            || witness.predecessor_digest != expected.record_digest
            || witness.target_block_number <= expected.block_number
        {
            return Err(KvStoreError::InvalidArgument(
                "local finalization witness does not refine its expected durable head".to_string(),
            ));
        }
        Ok(())
    }

    pub fn validate_witness_standalone(
        witness: &LocalFinalizationWitness,
    ) -> Result<(), KvStoreError> {
        let hash_length = models::rust::block_hash::LENGTH;
        if witness.schema_version != Self::WITNESS_SCHEMA_VERSION
            || witness.genesis_hash.0.len() != hash_length
            || witness.predecessor_hash.0.len() != hash_length
            || witness.predecessor_digest.0.len() != hash_length
            || witness.target_block_hash.0.len() != hash_length
            || witness.target_post_state_hash.0.len() != hash_length
            || witness.authority_context_digest.0.len() != hash_length
            || witness.witness_digest.0.len() != hash_length
            || witness.fault_tolerance_denominator <= 0
            || !witness
                .supporting_block_hashes
                .contains(&witness.target_block_hash)
            || !witness.finalized.contains(&witness.target_block_hash)
            || witness
                .finalized
                .iter()
                .any(|hash| !witness.supporting_block_hashes.contains(hash))
            || witness
                .latest_messages
                .values()
                .any(|hash| !witness.supporting_block_hashes.contains(hash))
            || witness
                .supporting_block_hashes
                .iter()
                .chain(witness.finalized.iter())
                .any(|hash| hash.0.len() != hash_length)
            || witness.witness_digest != Self::witness_digest(witness)
        {
            return Err(KvStoreError::InvalidArgument(
                "local finalization witness standalone validation failed".to_string(),
            ));
        }
        Ok(())
    }

    pub fn persist_witness(
        &self,
        expected: &FinalizationHead,
        witness: &LocalFinalizationWitness,
    ) -> Result<(), KvStoreError> {
        Self::validate_witness(expected, witness)?;
        let _guard = self.append_lock.lock();
        let genesis = self.genesis()?.ok_or_else(|| {
            KvStoreError::InvalidArgument(
                "finalization ledger has no initialized genesis anchor".to_string(),
            )
        })?;
        if witness.genesis_hash != genesis.block_hash {
            return Err(KvStoreError::InvalidArgument(
                "local finalization witness is bound to a different genesis".to_string(),
            ));
        }
        let key = FinalizationLedgerKey::Witness(witness.witness_digest.clone());
        match self.store.get_one(&key)? {
            Some(FinalizationLedgerValue::Witness(existing)) if existing == *witness => Ok(()),
            Some(_) => Err(KvStoreError::SerializationError(
                "local finalization witness digest collision".to_string(),
            )),
            None => self
                .store
                .put_one(key, FinalizationLedgerValue::Witness(witness.clone())),
        }
    }

    pub fn witness(
        &self,
        digest: &BlockHashSerde,
    ) -> Result<Option<LocalFinalizationWitness>, KvStoreError> {
        match self
            .store
            .get_one(&FinalizationLedgerKey::Witness(digest.clone()))?
        {
            Some(FinalizationLedgerValue::Witness(witness)) => Ok(Some(witness)),
            Some(_) => Err(KvStoreError::SerializationError(
                "local finalization witness key contains a different value".to_string(),
            )),
            None => Ok(None),
        }
    }

    pub fn head(&self) -> Result<Option<FinalizationHead>, KvStoreError> {
        match self.store.get_one(&FinalizationLedgerKey::Head)? {
            Some(FinalizationLedgerValue::Head(head)) => Ok(Some(head)),
            Some(_) => Err(KvStoreError::SerializationError(
                "finalization ledger head key contains a non-head value".to_string(),
            )),
            None => Ok(None),
        }
    }

    fn genesis(&self) -> Result<Option<FinalizationGenesisAnchor>, KvStoreError> {
        match self.store.get_one(&FinalizationLedgerKey::Genesis)? {
            Some(FinalizationLedgerValue::Genesis(genesis)) => Ok(Some(genesis)),
            Some(_) => Err(KvStoreError::SerializationError(
                "finalization ledger genesis key contains a non-genesis value".to_string(),
            )),
            None => Ok(None),
        }
    }

    pub fn genesis_anchor(&self) -> Result<Option<FinalizationGenesisAnchor>, KvStoreError> {
        self.genesis()
    }

    fn expected_genesis(block_hash: BlockHash, block_number: i64) -> FinalizationGenesisAnchor {
        FinalizationGenesisAnchor {
            record_digest: Self::genesis_digest(&block_hash, block_number),
            block_hash: BlockHashSerde(block_hash),
            block_number,
        }
    }

    fn genesis_head(genesis: &FinalizationGenesisAnchor) -> FinalizationHead {
        FinalizationHead {
            revision: 0,
            block_hash: genesis.block_hash.clone(),
            block_number: genesis.block_number,
            record_digest: genesis.record_digest.clone(),
        }
    }

    fn record_head(record: &FinalizationRecord) -> FinalizationHead {
        FinalizationHead {
            revision: record.revision,
            block_hash: record.directly_finalized.clone(),
            block_number: record.block_number,
            record_digest: record.record_digest.clone(),
        }
    }

    fn validate_cursor_bounds(&self, head_revision: u64) -> Result<(), KvStoreError> {
        let projection = self.projection_cursor()?;
        let effects = self.effects_cursor()?;
        let compaction = self.effects_compaction_cursor()?;
        if projection > head_revision || effects > head_revision || compaction > effects {
            return Err(KvStoreError::SerializationError(format!(
                "finalization cursor bounds are invalid: projection={projection}, effects={effects}, compaction={compaction}, head={head_revision}"
            )));
        }
        Ok(())
    }

    fn validate_initialized_endpoints(
        &self,
        genesis: &FinalizationGenesisAnchor,
        head: &FinalizationHead,
    ) -> Result<(), KvStoreError> {
        let expected_digest = Self::genesis_digest(&genesis.block_hash.0, genesis.block_number);
        if genesis.record_digest != expected_digest {
            return Err(KvStoreError::SerializationError(
                "finalization genesis anchor digest is invalid".to_string(),
            ));
        }
        self.validate_cursor_bounds(head.revision)?;
        let genesis_head = Self::genesis_head(genesis);
        if head.revision == 0 {
            if *head != genesis_head {
                return Err(KvStoreError::SerializationError(
                    "revision-zero finalization head disagrees with genesis anchor".to_string(),
                ));
            }
            return Ok(());
        }
        let first = self.record(1)?.ok_or_else(|| {
            KvStoreError::SerializationError(
                "advanced finalization ledger is missing round 1".to_string(),
            )
        })?;
        Self::validate_record(&genesis_head, &first)?;
        let current = self.record(head.revision)?.ok_or_else(|| {
            KvStoreError::SerializationError(format!(
                "finalization ledger head references missing round {}",
                head.revision
            ))
        })?;
        if Self::record_head(&current) != *head {
            return Err(KvStoreError::SerializationError(
                "finalization ledger head disagrees with its immutable round record".to_string(),
            ));
        }
        Ok(())
    }

    pub fn validate_integrity(&self) -> Result<(), KvStoreError> {
        let _guard = self.append_lock.lock();
        let genesis = self.genesis()?;
        let head = self.head()?;
        match (genesis, head) {
            (None, None) if !self.store.non_empty()? => Ok(()),
            (None, None) => Err(KvStoreError::SerializationError(
                "finalization ledger contains partial bootstrap data".to_string(),
            )),
            (None, Some(_)) => Err(KvStoreError::SerializationError(
                "finalization ledger head exists without an immutable genesis anchor".to_string(),
            )),
            (Some(_), None) => Err(KvStoreError::SerializationError(
                "finalization genesis anchor exists without a durable head".to_string(),
            )),
            (Some(genesis), Some(head)) => {
                self.validate_initialized_endpoints(&genesis, &head)?;
                let mut expected = Self::genesis_head(&genesis);
                for revision in 1..=head.revision {
                    let record = self.record(revision)?.ok_or_else(|| {
                        KvStoreError::SerializationError(format!(
                            "finalization ledger head references missing round {revision}"
                        ))
                    })?;
                    Self::validate_record(&expected, &record)?;
                    let witness = self.witness(&record.witness_digest)?.ok_or_else(|| {
                        KvStoreError::SerializationError(format!(
                            "finalization round {revision} has no portable witness"
                        ))
                    })?;
                    Self::validate_witness(&expected, &witness)?;
                    if witness.target_block_hash != record.directly_finalized
                        || witness.target_block_number != record.block_number
                        || witness.finalized != record.finalized
                    {
                        return Err(KvStoreError::SerializationError(format!(
                            "finalization round {revision} witness does not match its record"
                        )));
                    }
                    expected = Self::record_head(&record);
                }
                if expected != head {
                    return Err(KvStoreError::SerializationError(
                        "finalization ledger chain does not terminate at its durable head"
                            .to_string(),
                    ));
                }
                Ok(())
            }
        }
    }

    pub fn ensure_genesis(
        &self,
        block_hash: BlockHash,
        block_number: i64,
    ) -> Result<EnsureGenesisOutcome, KvStoreError> {
        let _guard = self.append_lock.lock();
        let expected = Self::expected_genesis(block_hash, block_number);
        match (self.genesis()?, self.head()?) {
            (Some(existing), Some(head)) if existing == expected => {
                self.validate_initialized_endpoints(&existing, &head)?;
                Ok(EnsureGenesisOutcome::AlreadyCanonical)
            }
            (Some(_), Some(_)) => Err(KvStoreError::InvalidArgument(
                "finalization ledger genesis conflicts with its immutable anchor".to_string(),
            )),
            (None, None) if !self.store.non_empty()? => {
                let head = Self::genesis_head(&expected);
                self.store.put(vec![
                    (
                        FinalizationLedgerKey::Genesis,
                        FinalizationLedgerValue::Genesis(expected),
                    ),
                    (
                        FinalizationLedgerKey::Head,
                        FinalizationLedgerValue::Head(head),
                    ),
                    (
                        FinalizationLedgerKey::ProjectionCursor,
                        FinalizationLedgerValue::ProjectionCursor(0),
                    ),
                    (
                        FinalizationLedgerKey::EffectsCursor,
                        FinalizationLedgerValue::EffectsCursor(0),
                    ),
                    (
                        FinalizationLedgerKey::EffectsCompactionCursor,
                        FinalizationLedgerValue::EffectsCompactionCursor(0),
                    ),
                ])?;
                Ok(EnsureGenesisOutcome::Initialized)
            }
            (None, None) => Err(KvStoreError::SerializationError(
                "refusing to bootstrap a non-empty finalization ledger".to_string(),
            )),
            (None, Some(_)) => Err(KvStoreError::SerializationError(
                "refusing to backfill a missing finalization genesis anchor".to_string(),
            )),
            (Some(_), None) => Err(KvStoreError::SerializationError(
                "finalization genesis anchor exists without a durable head".to_string(),
            )),
        }
    }

    pub fn prepare_record(
        expected: &FinalizationHead,
        directly_finalized: BlockHash,
        block_number: i64,
        fault_tolerance: f32,
        finalized: BTreeSet<BlockHashSerde>,
        witness: &LocalFinalizationWitness,
    ) -> Result<FinalizationRecord, KvStoreError> {
        if block_number <= expected.block_number {
            return Err(KvStoreError::InvalidArgument(
                "finalization candidate does not strictly advance block height".to_string(),
            ));
        }
        if !finalized.contains(&BlockHashSerde(directly_finalized.clone())) {
            return Err(KvStoreError::InvalidArgument(
                "finalization manifest omits directly finalized block".to_string(),
            ));
        }
        Self::validate_witness(expected, witness)?;
        if witness.target_block_hash.0 != directly_finalized
            || witness.target_block_number != block_number
            || witness.finalized != finalized
        {
            return Err(KvStoreError::InvalidArgument(
                "finalization record does not match its portable witness".to_string(),
            ));
        }
        let manifest_digest = Self::manifest_digest(&finalized);
        let mut record = FinalizationRecord {
            revision: expected.revision.checked_add(1).ok_or_else(|| {
                KvStoreError::InvalidArgument("finalization revision exhausted".to_string())
            })?,
            predecessor_hash: expected.block_hash.clone(),
            predecessor_digest: expected.record_digest.clone(),
            directly_finalized: BlockHashSerde(directly_finalized),
            block_number,
            fault_tolerance_bits: fault_tolerance.to_bits(),
            finalized,
            manifest_digest,
            witness_digest: witness.witness_digest.clone(),
            record_digest: BlockHashSerde(Bytes::new()),
        };
        record.record_digest = Self::record_digest(&record);
        Ok(record)
    }

    fn validate_record(
        expected: &FinalizationHead,
        record: &FinalizationRecord,
    ) -> Result<(), KvStoreError> {
        if record.revision
            != expected.revision.checked_add(1).ok_or_else(|| {
                KvStoreError::InvalidArgument("finalization revision exhausted".to_string())
            })?
            || record.predecessor_hash != expected.block_hash
            || record.predecessor_digest != expected.record_digest
            || record.block_number <= expected.block_number
            || !record.finalized.contains(&record.directly_finalized)
            || record.manifest_digest != Self::manifest_digest(&record.finalized)
            || record.record_digest != Self::record_digest(record)
        {
            return Err(KvStoreError::InvalidArgument(
                "finalization record does not refine its expected durable head".to_string(),
            ));
        }
        Ok(())
    }

    pub fn try_append(
        &self,
        expected: &FinalizationHead,
        record: &FinalizationRecord,
    ) -> Result<FinalizationAppendOutcome, KvStoreError> {
        Self::validate_record(expected, record)?;
        let _guard = self.append_lock.lock();
        let current = self.head()?.ok_or_else(|| {
            KvStoreError::InvalidArgument(
                "finalization ledger has no initialized genesis head".to_string(),
            )
        })?;
        let witness = self.witness(&record.witness_digest)?.ok_or_else(|| {
            KvStoreError::InvalidArgument(
                "finalization witness must be durably persisted before ledger publication"
                    .to_string(),
            )
        })?;
        Self::validate_witness(expected, &witness)?;
        if witness.target_block_hash != record.directly_finalized
            || witness.target_block_number != record.block_number
            || witness.finalized != record.finalized
        {
            return Err(KvStoreError::InvalidArgument(
                "persisted finality witness does not match finalization record".to_string(),
            ));
        }
        if current == *expected {
            let next = FinalizationHead {
                revision: record.revision,
                block_hash: record.directly_finalized.clone(),
                block_number: record.block_number,
                record_digest: record.record_digest.clone(),
            };
            self.store.put(vec![
                (
                    FinalizationLedgerKey::Round(record.revision),
                    FinalizationLedgerValue::Round(record.clone()),
                ),
                (
                    FinalizationLedgerKey::Head,
                    FinalizationLedgerValue::Head(next.clone()),
                ),
            ])?;
            return Ok(FinalizationAppendOutcome::Committed(next));
        }
        if current.revision == record.revision
            && current.block_hash == record.directly_finalized
            && current.record_digest == record.record_digest
        {
            let stored = self.record(record.revision)?;
            if stored.as_ref() == Some(record) {
                return Ok(FinalizationAppendOutcome::AlreadyCommitted(current));
            }
            return Err(KvStoreError::SerializationError(
                "finalization head exists without its exact immutable round record".to_string(),
            ));
        }
        Ok(FinalizationAppendOutcome::Stale(current))
    }

    pub fn record(&self, revision: u64) -> Result<Option<FinalizationRecord>, KvStoreError> {
        match self
            .store
            .get_one(&FinalizationLedgerKey::Round(revision))?
        {
            Some(FinalizationLedgerValue::Round(record)) => Ok(Some(record)),
            Some(_) => Err(KvStoreError::SerializationError(format!(
                "finalization round key {revision} contains a non-round value"
            ))),
            None => Ok(None),
        }
    }

    pub fn records_through_head(&self) -> Result<Vec<FinalizationRecord>, KvStoreError> {
        let Some(head) = self.head()? else {
            return Ok(Vec::new());
        };
        (1..=head.revision)
            .map(|revision| {
                self.record(revision)?.ok_or_else(|| {
                    KvStoreError::SerializationError(format!(
                        "finalization ledger head references missing round {revision}"
                    ))
                })
            })
            .collect()
    }

    fn projection_cursor(&self) -> Result<u64, KvStoreError> {
        match self
            .store
            .get_one(&FinalizationLedgerKey::ProjectionCursor)?
        {
            Some(FinalizationLedgerValue::ProjectionCursor(revision)) => Ok(revision),
            Some(_) => Err(KvStoreError::SerializationError(
                "finalization projection cursor key contains a non-cursor value".to_string(),
            )),
            None => Err(KvStoreError::SerializationError(
                "finalization ledger is missing its projection cursor".to_string(),
            )),
        }
    }

    fn effects_cursor(&self) -> Result<u64, KvStoreError> {
        match self.store.get_one(&FinalizationLedgerKey::EffectsCursor)? {
            Some(FinalizationLedgerValue::EffectsCursor(revision)) => Ok(revision),
            Some(_) => Err(KvStoreError::SerializationError(
                "finalization effects cursor key contains a non-cursor value".to_string(),
            )),
            None => Err(KvStoreError::SerializationError(
                "finalization ledger is missing its effects cursor".to_string(),
            )),
        }
    }

    fn effects_compaction_cursor(&self) -> Result<u64, KvStoreError> {
        match self
            .store
            .get_one(&FinalizationLedgerKey::EffectsCompactionCursor)?
        {
            Some(FinalizationLedgerValue::EffectsCompactionCursor(revision)) => Ok(revision),
            Some(_) => Err(KvStoreError::SerializationError(
                "finalization effects compaction cursor key contains a non-cursor value"
                    .to_string(),
            )),
            None => Err(KvStoreError::SerializationError(
                "finalization ledger is missing its effects compaction cursor".to_string(),
            )),
        }
    }

    fn records_after(&self, revision: u64) -> Result<Vec<FinalizationRecord>, KvStoreError> {
        let head = self.head()?.ok_or_else(|| {
            KvStoreError::InvalidArgument(
                "finalization ledger has no initialized genesis head".to_string(),
            )
        })?;
        if revision > head.revision {
            return Err(KvStoreError::SerializationError(format!(
                "finalization cursor {revision} exceeds durable head {}",
                head.revision
            )));
        }
        if revision == head.revision {
            return Ok(Vec::new());
        }
        let first = revision.checked_add(1).ok_or_else(|| {
            KvStoreError::SerializationError("finalization cursor exhausted".to_string())
        })?;
        (first..=head.revision)
            .map(|next| {
                self.record(next)?.ok_or_else(|| {
                    KvStoreError::SerializationError(format!(
                        "finalization ledger head references missing round {next}"
                    ))
                })
            })
            .collect()
    }

    pub fn pending_projection_records(&self) -> Result<Vec<FinalizationRecord>, KvStoreError> {
        self.records_after(self.projection_cursor()?)
    }

    pub fn record_projection_completed(&self, revision: u64) -> Result<(), KvStoreError> {
        let _guard = self.append_lock.lock();
        let cursor = self.projection_cursor()?;
        if revision <= cursor {
            return Ok(());
        }
        let expected = cursor.checked_add(1).ok_or_else(|| {
            KvStoreError::InvalidArgument("finalization projection cursor exhausted".to_string())
        })?;
        if revision != expected || self.record(revision)?.is_none() {
            return Err(KvStoreError::InvalidArgument(format!(
                "cannot advance finalization projection cursor from {cursor} to {revision}"
            )));
        }
        self.store.put_one(
            FinalizationLedgerKey::ProjectionCursor,
            FinalizationLedgerValue::ProjectionCursor(revision),
        )
    }

    pub fn pending_effect_records(&self) -> Result<Vec<FinalizationRecord>, KvStoreError> {
        self.records_after(self.effects_cursor()?)
    }

    fn round_effects_complete(&self, revision: u64) -> Result<bool, KvStoreError> {
        match self
            .store
            .get_one(&FinalizationLedgerKey::EffectsComplete(revision))?
        {
            Some(FinalizationLedgerValue::EffectsComplete) => Ok(true),
            Some(_) => Err(KvStoreError::SerializationError(format!(
                "finalization effects-complete key {revision} contains an invalid value"
            ))),
            None => Ok(false),
        }
    }

    pub fn effect_completed(&self, id: &FinalizationEffectId) -> Result<bool, KvStoreError> {
        if id.revision <= self.effects_cursor()? {
            return Ok(true);
        }
        match self
            .store
            .get_one(&FinalizationLedgerKey::Effect(id.clone()))?
        {
            Some(FinalizationLedgerValue::Effect) => Ok(true),
            Some(_) => Err(KvStoreError::SerializationError(
                "finalization effect key contains a non-effect value".to_string(),
            )),
            None => Ok(false),
        }
    }

    pub fn record_effect(&self, id: FinalizationEffectId) -> Result<(), KvStoreError> {
        let _guard = self.append_lock.lock();
        let record = self.record(id.revision)?.ok_or_else(|| {
            KvStoreError::InvalidArgument(format!(
                "cannot receipt effect for uncommitted finalization round {}",
                id.revision
            ))
        })?;
        if !record.finalized.contains(&id.block_hash) {
            return Err(KvStoreError::InvalidArgument(
                "cannot receipt effect for a block outside the committed manifest".to_string(),
            ));
        }
        if self.effect_completed(&id)? {
            return Ok(());
        }
        self.store.put_one(
            FinalizationLedgerKey::Effect(id),
            FinalizationLedgerValue::Effect,
        )
    }

    pub fn record_round_effects_completed(&self, revision: u64) -> Result<u64, KvStoreError> {
        let _guard = self.append_lock.lock();
        let record = self.record(revision)?.ok_or_else(|| {
            KvStoreError::InvalidArgument(format!(
                "cannot complete effects for uncommitted finalization round {revision}"
            ))
        })?;
        let old_cursor = self.effects_cursor()?;
        if revision <= old_cursor {
            return Ok(old_cursor);
        }
        for block_hash in &record.finalized {
            for kind in FinalizationEffectKind::ALL {
                let id = FinalizationEffectId {
                    revision,
                    block_hash: block_hash.clone(),
                    kind,
                };
                if !self.effect_completed(&id)? {
                    return Err(KvStoreError::InvalidArgument(format!(
                        "cannot complete finalization round {revision} with a missing {kind:?} receipt"
                    )));
                }
            }
        }

        let head = self.head()?.ok_or_else(|| {
            KvStoreError::InvalidArgument(
                "finalization ledger has no initialized genesis head".to_string(),
            )
        })?;
        if revision > head.revision {
            return Err(KvStoreError::InvalidArgument(format!(
                "effect round {revision} exceeds durable head {}",
                head.revision
            )));
        }
        let mut new_cursor = old_cursor;
        while new_cursor < head.revision {
            let next = new_cursor.checked_add(1).ok_or_else(|| {
                KvStoreError::SerializationError(
                    "finalization effects cursor exhausted".to_string(),
                )
            })?;
            if next != revision && !self.round_effects_complete(next)? {
                break;
            }
            new_cursor = next;
        }

        let mut updates = vec![(
            FinalizationLedgerKey::EffectsComplete(revision),
            FinalizationLedgerValue::EffectsComplete,
        )];
        if new_cursor != old_cursor {
            updates.push((
                FinalizationLedgerKey::EffectsCursor,
                FinalizationLedgerValue::EffectsCursor(new_cursor),
            ));
        }
        self.store.put(updates)?;
        self.compact_effect_receipts()?;
        Ok(new_cursor)
    }

    fn compact_effect_receipts(&self) -> Result<(), KvStoreError> {
        let compacted = self.effects_compaction_cursor()?;
        let completed = self.effects_cursor()?;
        if compacted > completed {
            return Err(KvStoreError::SerializationError(format!(
                "finalization effects compaction cursor {compacted} exceeds completed cursor {completed}"
            )));
        }
        if compacted == completed {
            return Ok(());
        }
        let mut keys = Vec::new();
        for revision in (compacted + 1)..=completed {
            let record = self.record(revision)?.ok_or_else(|| {
                KvStoreError::SerializationError(format!(
                    "cannot compact effects for missing finalization round {revision}"
                ))
            })?;
            for block_hash in record.finalized {
                for kind in FinalizationEffectKind::ALL {
                    keys.push(FinalizationLedgerKey::Effect(FinalizationEffectId {
                        revision,
                        block_hash: block_hash.clone(),
                        kind,
                    }));
                }
            }
            keys.push(FinalizationLedgerKey::EffectsComplete(revision));
        }
        self.store.delete(keys)?;
        self.store.put_one(
            FinalizationLedgerKey::EffectsCompactionCursor,
            FinalizationLedgerValue::EffectsCompactionCursor(completed),
        )
    }

    pub fn reconcile_effect_compaction(&self) -> Result<(), KvStoreError> {
        let _guard = self.append_lock.lock();
        self.compact_effect_receipts()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Barrier;

    use proptest::prelude::*;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

    use super::*;

    fn hash(byte: u8) -> BlockHash { Bytes::from(vec![byte; 32]) }

    fn ledger() -> FinalizationLedger {
        FinalizationLedger::new(KeyValueTypedStoreImpl::new(Arc::new(
            InMemoryKeyValueStore::new(),
        )))
    }

    fn initialize(
        ledger: &FinalizationLedger,
        block_hash: BlockHash,
        block_number: i64,
    ) -> FinalizationHead {
        assert_eq!(
            ledger.ensure_genesis(block_hash, block_number).unwrap(),
            EnsureGenesisOutcome::Initialized
        );
        ledger.head().unwrap().unwrap()
    }

    fn prepare_record(
        ledger: &FinalizationLedger,
        expected: &FinalizationHead,
        directly_finalized: BlockHash,
        block_number: i64,
        fault_tolerance: f32,
        finalized: BTreeSet<BlockHashSerde>,
    ) -> Result<FinalizationRecord, KvStoreError> {
        let genesis = ledger.genesis_anchor()?.unwrap();
        let witness = FinalizationLedger::prepare_witness(
            genesis.block_hash.0,
            expected,
            directly_finalized.clone(),
            block_number,
            hash(200),
            1,
            1,
            BTreeMap::new(),
            BTreeSet::from([BlockHashSerde(directly_finalized.clone())]),
            BlockHashSerde(hash(201)),
            finalized.clone(),
        )?;
        ledger.persist_witness(expected, &witness)?;
        FinalizationLedger::prepare_record(
            expected,
            directly_finalized,
            block_number,
            fault_tolerance,
            finalized,
            &witness,
        )
    }

    #[test]
    fn witness_requires_authority_context_and_finalized_target() {
        let ledger = ledger();
        let genesis = initialize(&ledger, hash(0), 0);
        let target = BlockHashSerde(hash(1));
        assert!(FinalizationLedger::prepare_witness(
            hash(0),
            &genesis,
            target.0.clone(),
            1,
            hash(2),
            1,
            10,
            BTreeMap::new(),
            BTreeSet::from([target.clone()]),
            BlockHashSerde(Bytes::new()),
            BTreeSet::from([target.clone()]),
        )
        .is_err());
        assert!(FinalizationLedger::prepare_witness(
            hash(0),
            &genesis,
            target.0,
            1,
            hash(2),
            1,
            10,
            BTreeMap::new(),
            BTreeSet::from([BlockHashSerde(hash(1))]),
            BlockHashSerde(hash(3)),
            BTreeSet::from([BlockHashSerde(hash(1)), BlockHashSerde(hash(3))]),
        )
        .is_err());
    }

    #[test]
    fn equal_finalized_target_can_have_different_local_ledger_identity() {
        let stepped = ledger();
        let direct = ledger();
        let stepped_genesis = initialize(&stepped, hash(0), 0);
        let direct_genesis = initialize(&direct, hash(0), 0);
        let intermediate = prepare_record(
            &stepped,
            &stepped_genesis,
            hash(5),
            5,
            0.75,
            BTreeSet::from([BlockHashSerde(hash(5))]),
        )
        .unwrap();
        let stepped_five = match stepped.try_append(&stepped_genesis, &intermediate).unwrap() {
            FinalizationAppendOutcome::Committed(head) => head,
            outcome => panic!("unexpected intermediate append outcome: {outcome:?}"),
        };
        let stepped_ten = prepare_record(
            &stepped,
            &stepped_five,
            hash(10),
            10,
            0.75,
            BTreeSet::from([BlockHashSerde(hash(10))]),
        )
        .unwrap();
        let stepped_head = match stepped.try_append(&stepped_five, &stepped_ten).unwrap() {
            FinalizationAppendOutcome::Committed(head) => head,
            outcome => panic!("unexpected stepped append outcome: {outcome:?}"),
        };
        let direct_ten = prepare_record(
            &direct,
            &direct_genesis,
            hash(10),
            10,
            0.75,
            BTreeSet::from([BlockHashSerde(hash(10))]),
        )
        .unwrap();
        let direct_head = match direct.try_append(&direct_genesis, &direct_ten).unwrap() {
            FinalizationAppendOutcome::Committed(head) => head,
            outcome => panic!("unexpected direct append outcome: {outcome:?}"),
        };

        assert_eq!(stepped_head.block_hash, direct_head.block_hash);
        assert_eq!(stepped_head.block_number, direct_head.block_number);
        assert_ne!(stepped_head.revision, direct_head.revision);
        assert_ne!(stepped_head.record_digest, direct_head.record_digest);
    }

    proptest! {
        #[test]
        fn arbitrary_local_round_history_is_not_cross_node_identity(
            intermediate_rounds in 1u8..16,
        ) {
            let stepped = ledger();
            let direct = ledger();
            let mut stepped_head = initialize(&stepped, hash(0), 0);
            let direct_genesis = initialize(&direct, hash(0), 0);
            for round in 1..=intermediate_rounds {
                let record = prepare_record(
                    &stepped,
                    &stepped_head,
                    hash(round),
                    i64::from(round),
                    0.75,
                    BTreeSet::from([BlockHashSerde(hash(round))]),
                )
                .unwrap();
                stepped_head = match stepped.try_append(&stepped_head, &record).unwrap() {
                    FinalizationAppendOutcome::Committed(head) => head,
                    outcome => panic!("unexpected intermediate append outcome: {outcome:?}"),
                };
            }
            let target = hash(250);
            let target_height = 100;
            let stepped_record = prepare_record(
                &stepped,
                &stepped_head,
                target.clone(),
                target_height,
                0.75,
                BTreeSet::from([BlockHashSerde(target.clone())]),
            )
            .unwrap();
            stepped_head = match stepped.try_append(&stepped_head, &stepped_record).unwrap() {
                FinalizationAppendOutcome::Committed(head) => head,
                outcome => panic!("unexpected stepped target outcome: {outcome:?}"),
            };
            let direct_record = prepare_record(
                &direct,
                &direct_genesis,
                target,
                target_height,
                0.75,
                BTreeSet::from([BlockHashSerde(hash(250))]),
            )
            .unwrap();
            let direct_head = match direct.try_append(&direct_genesis, &direct_record).unwrap() {
                FinalizationAppendOutcome::Committed(head) => head,
                outcome => panic!("unexpected direct target outcome: {outcome:?}"),
            };

            prop_assert_eq!(stepped_head.block_hash, direct_head.block_hash);
            prop_assert_eq!(stepped_head.block_number, direct_head.block_number);
            prop_assert_ne!(stepped_head.revision, direct_head.revision);
            prop_assert_ne!(stepped_head.record_digest, direct_head.record_digest);
        }
    }

    #[test]
    fn one_successor_wins_parallel_same_head_append() {
        let ledger = ledger();
        let genesis = initialize(&ledger, hash(0), 0);
        let left = prepare_record(
            &ledger,
            &genesis,
            hash(1),
            1,
            0.5,
            BTreeSet::from([BlockHashSerde(hash(1))]),
        )
        .unwrap();
        let right = prepare_record(
            &ledger,
            &genesis,
            hash(2),
            1,
            0.5,
            BTreeSet::from([BlockHashSerde(hash(2))]),
        )
        .unwrap();

        let barrier = Arc::new(Barrier::new(3));
        let left_worker = {
            let ledger = ledger.clone();
            let genesis = genesis.clone();
            let left = left.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                ledger.try_append(&genesis, &left).unwrap()
            })
        };
        let right_worker = {
            let ledger = ledger.clone();
            let genesis = genesis.clone();
            let right = right.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                ledger.try_append(&genesis, &right).unwrap()
            })
        };
        barrier.wait();
        let outcomes = [left_worker.join().unwrap(), right_worker.join().unwrap()];
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, FinalizationAppendOutcome::Committed(_)))
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, FinalizationAppendOutcome::Stale(_)))
                .count(),
            1
        );
        assert_eq!(ledger.records_through_head().unwrap().len(), 1);
    }

    #[test]
    fn exact_retry_is_idempotent_and_restart_recovers_head() {
        let store = Arc::new(InMemoryKeyValueStore::new());
        let ledger = FinalizationLedger::new(KeyValueTypedStoreImpl::new(store.clone()));
        let genesis = initialize(&ledger, hash(0), 0);
        let record = prepare_record(
            &ledger,
            &genesis,
            hash(1),
            1,
            0.75,
            BTreeSet::from([BlockHashSerde(hash(1))]),
        )
        .unwrap();
        let committed = ledger.try_append(&genesis, &record).unwrap();
        assert!(matches!(committed, FinalizationAppendOutcome::Committed(_)));
        assert!(matches!(
            ledger.try_append(&genesis, &record).unwrap(),
            FinalizationAppendOutcome::AlreadyCommitted(_)
        ));

        let restarted = FinalizationLedger::new(KeyValueTypedStoreImpl::new(store));
        assert_eq!(restarted.head().unwrap(), ledger.head().unwrap());
        assert_eq!(restarted.records_through_head().unwrap(), vec![record]);
    }

    #[test]
    fn duplicate_genesis_after_advanced_head_is_a_write_free_identity_assertion() {
        let store = Arc::new(InMemoryKeyValueStore::new());
        let ledger = FinalizationLedger::new(KeyValueTypedStoreImpl::new(store.clone()));
        let genesis = initialize(&ledger, hash(0), 0);
        let record = prepare_record(
            &ledger,
            &genesis,
            hash(1),
            1,
            0.75,
            BTreeSet::from([BlockHashSerde(hash(1))]),
        )
        .unwrap();
        ledger.try_append(&genesis, &record).unwrap();
        let before = store.to_map().unwrap();

        assert_eq!(
            ledger.ensure_genesis(hash(0), 0).unwrap(),
            EnsureGenesisOutcome::AlreadyCanonical
        );
        assert_eq!(store.to_map().unwrap(), before);
        assert_eq!(ledger.head().unwrap().unwrap().revision, 1);
        assert!(ledger.ensure_genesis(hash(9), 0).is_err());
        assert_eq!(store.to_map().unwrap(), before);
    }

    #[test]
    fn restarted_advanced_ledger_accepts_only_its_immutable_genesis() {
        let store = Arc::new(InMemoryKeyValueStore::new());
        let ledger = FinalizationLedger::new(KeyValueTypedStoreImpl::new(store.clone()));
        let genesis = initialize(&ledger, hash(0), 0);
        let record = prepare_record(
            &ledger,
            &genesis,
            hash(1),
            1,
            0.75,
            BTreeSet::from([BlockHashSerde(hash(1))]),
        )
        .unwrap();
        ledger.try_append(&genesis, &record).unwrap();

        let restarted = FinalizationLedger::new(KeyValueTypedStoreImpl::new(store.clone()));
        restarted.validate_integrity().unwrap();
        let before = store.to_map().unwrap();
        assert_eq!(
            restarted.ensure_genesis(hash(0), 0).unwrap(),
            EnsureGenesisOutcome::AlreadyCanonical
        );
        assert_eq!(store.to_map().unwrap(), before);
    }

    #[test]
    fn partial_or_unrooted_bootstrap_state_fails_closed() {
        let head_only = ledger();
        head_only
            .store
            .put_one(
                FinalizationLedgerKey::Head,
                FinalizationLedgerValue::Head(FinalizationLedger::genesis_head(
                    &FinalizationLedger::expected_genesis(hash(0), 0),
                )),
            )
            .unwrap();
        assert!(head_only.validate_integrity().is_err());
        assert!(head_only.ensure_genesis(hash(0), 0).is_err());

        let anchor_only = ledger();
        anchor_only
            .store
            .put_one(
                FinalizationLedgerKey::Genesis,
                FinalizationLedgerValue::Genesis(FinalizationLedger::expected_genesis(hash(0), 0)),
            )
            .unwrap();
        assert!(anchor_only.validate_integrity().is_err());
        assert!(anchor_only.ensure_genesis(hash(0), 0).is_err());

        let cursor_only = ledger();
        cursor_only
            .store
            .put_one(
                FinalizationLedgerKey::ProjectionCursor,
                FinalizationLedgerValue::ProjectionCursor(0),
            )
            .unwrap();
        assert!(cursor_only.validate_integrity().is_err());
        assert!(cursor_only.ensure_genesis(hash(0), 0).is_err());
    }

    #[test]
    fn corrupt_chain_endpoint_or_cursor_fails_reopen_and_duplicate_assertion() {
        let missing_round = ledger();
        let genesis = initialize(&missing_round, hash(0), 0);
        missing_round
            .store
            .put_one(
                FinalizationLedgerKey::Head,
                FinalizationLedgerValue::Head(FinalizationHead {
                    revision: 1,
                    block_hash: BlockHashSerde(hash(1)),
                    block_number: 1,
                    record_digest: BlockHashSerde(hash(2)),
                }),
            )
            .unwrap();
        assert!(missing_round.validate_integrity().is_err());
        assert!(missing_round.ensure_genesis(hash(0), 0).is_err());

        let invalid_cursor = ledger();
        assert_eq!(initialize(&invalid_cursor, hash(0), 0), genesis);
        invalid_cursor
            .store
            .put_one(
                FinalizationLedgerKey::ProjectionCursor,
                FinalizationLedgerValue::ProjectionCursor(1),
            )
            .unwrap();
        assert!(invalid_cursor.validate_integrity().is_err());
        assert!(invalid_cursor.ensure_genesis(hash(0), 0).is_err());
    }

    #[test]
    fn effect_receipts_are_idempotent_and_manifest_scoped() {
        let ledger = ledger();
        let genesis = initialize(&ledger, hash(0), 0);
        let record = prepare_record(
            &ledger,
            &genesis,
            hash(1),
            1,
            0.75,
            BTreeSet::from([BlockHashSerde(hash(1))]),
        )
        .unwrap();
        ledger.try_append(&genesis, &record).unwrap();
        let id = FinalizationEffectId {
            revision: 1,
            block_hash: BlockHashSerde(hash(1)),
            kind: FinalizationEffectKind::DeployRemoval,
        };
        ledger.record_effect(id.clone()).unwrap();
        ledger.record_effect(id.clone()).unwrap();
        assert!(ledger.effect_completed(&id).unwrap());

        let outside = FinalizationEffectId {
            revision: 1,
            block_hash: BlockHashSerde(hash(9)),
            kind: FinalizationEffectKind::DeployRemoval,
        };
        assert!(ledger.record_effect(outside).is_err());
    }

    fn receipt_all_effects(ledger: &FinalizationLedger, revision: u64, block_hash: BlockHash) {
        for kind in FinalizationEffectKind::ALL {
            ledger
                .record_effect(FinalizationEffectId {
                    revision,
                    block_hash: BlockHashSerde(block_hash.clone()),
                    kind,
                })
                .unwrap();
        }
    }

    #[test]
    fn projection_cursor_advances_only_in_committed_order() {
        let ledger = ledger();
        let genesis = initialize(&ledger, hash(0), 0);
        let first = prepare_record(
            &ledger,
            &genesis,
            hash(1),
            1,
            0.75,
            BTreeSet::from([BlockHashSerde(hash(1))]),
        )
        .unwrap();
        let first_head = match ledger.try_append(&genesis, &first).unwrap() {
            FinalizationAppendOutcome::Committed(head) => head,
            outcome => panic!("unexpected append outcome: {outcome:?}"),
        };
        let second = prepare_record(
            &ledger,
            &first_head,
            hash(2),
            2,
            0.8,
            BTreeSet::from([BlockHashSerde(hash(2))]),
        )
        .unwrap();
        ledger.try_append(&first_head, &second).unwrap();

        assert_eq!(ledger.pending_projection_records().unwrap(), vec![
            first.clone(),
            second.clone()
        ]);
        assert!(ledger.record_projection_completed(2).is_err());
        ledger.record_projection_completed(1).unwrap();
        ledger.record_projection_completed(1).unwrap();
        assert_eq!(ledger.pending_projection_records().unwrap(), vec![
            second.clone()
        ]);
        ledger.record_projection_completed(2).unwrap();
        assert!(ledger.pending_projection_records().unwrap().is_empty());
    }

    #[test]
    fn effects_cursor_coalesces_out_of_order_completed_rounds() {
        let ledger = ledger();
        let genesis = initialize(&ledger, hash(0), 0);
        let first = prepare_record(
            &ledger,
            &genesis,
            hash(1),
            1,
            0.75,
            BTreeSet::from([BlockHashSerde(hash(1))]),
        )
        .unwrap();
        let first_head = match ledger.try_append(&genesis, &first).unwrap() {
            FinalizationAppendOutcome::Committed(head) => head,
            outcome => panic!("unexpected append outcome: {outcome:?}"),
        };
        let second = prepare_record(
            &ledger,
            &first_head,
            hash(2),
            2,
            0.8,
            BTreeSet::from([BlockHashSerde(hash(2))]),
        )
        .unwrap();
        ledger.try_append(&first_head, &second).unwrap();

        assert!(ledger.record_round_effects_completed(1).is_err());
        receipt_all_effects(&ledger, 2, hash(2));
        assert_eq!(ledger.record_round_effects_completed(2).unwrap(), 0);
        receipt_all_effects(&ledger, 1, hash(1));
        assert_eq!(ledger.record_round_effects_completed(1).unwrap(), 2);
        assert!(ledger.pending_effect_records().unwrap().is_empty());
    }

    #[test]
    fn restart_recovers_each_projection_and_effect_boundary() {
        let store = Arc::new(InMemoryKeyValueStore::new());
        let ledger = FinalizationLedger::new(KeyValueTypedStoreImpl::new(store.clone()));
        let genesis = initialize(&ledger, hash(0), 0);
        let record = prepare_record(
            &ledger,
            &genesis,
            hash(1),
            1,
            0.75,
            BTreeSet::from([BlockHashSerde(hash(1))]),
        )
        .unwrap();
        ledger.try_append(&genesis, &record).unwrap();

        let restarted = FinalizationLedger::new(KeyValueTypedStoreImpl::new(store.clone()));
        assert_eq!(restarted.pending_projection_records().unwrap(), vec![
            record.clone()
        ]);
        restarted.record_projection_completed(1).unwrap();

        for kind in [
            FinalizationEffectKind::DeployRemoval,
            FinalizationEffectKind::CosignerRemoval,
        ] {
            restarted
                .record_effect(FinalizationEffectId {
                    revision: 1,
                    block_hash: BlockHashSerde(hash(1)),
                    kind,
                })
                .unwrap();
        }
        let restarted = FinalizationLedger::new(KeyValueTypedStoreImpl::new(store.clone()));
        assert_eq!(restarted.pending_effect_records().unwrap(), vec![record]);
        assert!(restarted.record_round_effects_completed(1).is_err());
        for kind in [
            FinalizationEffectKind::RuntimeCacheEviction,
            FinalizationEffectKind::FinalizedEvent,
        ] {
            restarted
                .record_effect(FinalizationEffectId {
                    revision: 1,
                    block_hash: BlockHashSerde(hash(1)),
                    kind,
                })
                .unwrap();
        }
        restarted.record_round_effects_completed(1).unwrap();

        let restarted = FinalizationLedger::new(KeyValueTypedStoreImpl::new(store));
        assert!(restarted.pending_projection_records().unwrap().is_empty());
        assert!(restarted.pending_effect_records().unwrap().is_empty());
        for kind in FinalizationEffectKind::ALL {
            assert!(restarted
                .effect_completed(&FinalizationEffectId {
                    revision: 1,
                    block_hash: BlockHashSerde(hash(1)),
                    kind,
                })
                .unwrap());
        }
    }

    proptest! {
        #[test]
        fn arbitrary_effect_completion_order_preserves_the_contiguous_cursor(
            priorities in proptest::collection::vec(any::<u8>(), 1..7)
        ) {
            let ledger = ledger();
            let mut head = initialize(&ledger, hash(0), 0);
            for revision in 1..=priorities.len() {
                let block_hash = hash(revision as u8);
                let record = prepare_record(
                    &ledger,
                    &head,
                    block_hash.clone(),
                    revision as i64,
                    0.75,
                    BTreeSet::from([BlockHashSerde(block_hash)]),
                )
                .unwrap();
                head = match ledger.try_append(&head, &record).unwrap() {
                    FinalizationAppendOutcome::Committed(next) => next,
                    outcome => panic!("unexpected append outcome: {outcome:?}"),
                };
            }

            let mut order = priorities.iter().copied().enumerate().collect::<Vec<_>>();
            order.sort_by_key(|(revision, priority)| (*priority, *revision));
            for (zero_based_revision, _) in order {
                let revision = zero_based_revision + 1;
                receipt_all_effects(&ledger, revision as u64, hash(revision as u8));
                let cursor = ledger
                    .record_round_effects_completed(revision as u64)
                    .unwrap();
                prop_assert!(cursor <= priorities.len() as u64);
                prop_assert!(ledger
                    .pending_effect_records()
                    .unwrap()
                    .iter()
                    .all(|record| record.revision > cursor));
            }
            prop_assert_eq!(ledger.effects_cursor().unwrap(), priorities.len() as u64);
            prop_assert!(ledger.pending_effect_records().unwrap().is_empty());
        }
    }
}
