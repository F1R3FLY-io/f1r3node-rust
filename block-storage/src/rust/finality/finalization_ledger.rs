use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::num::NonZeroUsize;
use std::sync::Arc;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::casper::protocol::casper_message::FinalizationCertificate;
use models::rust::validator::ValidatorSerde;
use parking_lot::Mutex;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use serde::{Deserialize, Serialize};
use shared::rust::store::key_value_store::{
    strict_atomic_mutate, AtomicStoreMutation, AtomicStoreOperation, KeyValueStore, KvStoreError,
};
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;

mod bounded_decode;
mod effect_observation;
mod integrity_pages;
mod recovery_pages;

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
    SettledRecoveryCharge(BlockHashSerde),
    SettledRecoveryUsage(BlockHashSerde),
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
    SettledRecoveryCharge(SettledRecoveryCharge),
    SettledRecoveryUsage(u64),
}

pub const RECOVERY_STATE_SCHEMA_VERSION: u32 = 1;
pub const SETTLED_RECOVERY_EPISODE_CAPACITY: u64 = 512;

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct RecoveryEpisodeId {
    pub schema_version: u32,
    pub revision: u64,
    pub shard_id: String,
    pub protocol_version: i64,
    pub floor_hash: BlockHashSerde,
    pub floor_post_state_hash: BlockHashSerde,
    pub certificate_digest: BlockHashSerde,
}

impl RecoveryEpisodeId {
    pub fn validate(&self) -> Result<(), KvStoreError> {
        let hash_length = models::rust::block_hash::LENGTH;
        let zero_certificate = self.certificate_digest.0.iter().all(|byte| *byte == 0);
        if self.schema_version != RECOVERY_STATE_SCHEMA_VERSION
            || self.shard_id.is_empty()
            || self.protocol_version <= 0
            || self.floor_hash.0.len() != hash_length
            || self.floor_post_state_hash.0.len() != hash_length
            || self.certificate_digest.0.len() != hash_length
            || ((self.revision == 0) != zero_certificate)
        {
            return Err(KvStoreError::InvalidArgument(
                "recovery episode is structurally invalid".to_string(),
            ));
        }
        Ok(())
    }

    pub fn digest(&self) -> BlockHashSerde {
        FinalizationLedger::digest([
            b"f1r3-recovery-episode-v1".to_vec(),
            self.schema_version.to_be_bytes().to_vec(),
            self.revision.to_be_bytes().to_vec(),
            self.shard_id.as_bytes().to_vec(),
            self.protocol_version.to_be_bytes().to_vec(),
            self.floor_hash.0.to_vec(),
            self.floor_post_state_hash.0.to_vec(),
            self.certificate_digest.0.to_vec(),
        ])
    }

    pub fn strictly_advances(&self, previous: &Self) -> bool {
        self.shard_id == previous.shard_id
            && self.protocol_version == previous.protocol_version
            && self.revision > previous.revision
            && self.floor_hash != previous.floor_hash
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct SettledRecoveryCharge {
    pub schema_version: u32,
    pub episode: RecoveryEpisodeId,
    pub target_block_hash: BlockHashSerde,
    pub citer_validator: ValidatorSerde,
    pub citer_bond_generation: i64,
}

pub(crate) struct SettledRecoveryChargePlan {
    pub store: Arc<dyn KeyValueStore>,
    pub mutations: Vec<(Vec<u8>, AtomicStoreOperation)>,
    pub committed: u64,
}

pub(crate) struct SettledRecoveryState {
    pub charges: Vec<SettledRecoveryCharge>,
    pub usage: HashMap<BlockHashSerde, u64>,
}

impl SettledRecoveryCharge {
    pub fn validate(&self) -> Result<(), KvStoreError> {
        self.episode.validate()?;
        if self.schema_version != RECOVERY_STATE_SCHEMA_VERSION
            || self.target_block_hash.0.len() != models::rust::block_hash::LENGTH
            || self.citer_validator.0.len() != models::rust::validator::LENGTH
            || self.citer_bond_generation < 0
        {
            return Err(KvStoreError::InvalidArgument(
                "settled recovery charge is structurally invalid".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FinalizationHead {
    pub revision: u64,
    pub block_hash: BlockHashSerde,
    pub block_number: i64,
    pub record_digest: BlockHashSerde,
    pub certificate_digest: BlockHashSerde,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FinalizationProjectionEndpoint {
    pub head: FinalizationHead,
    pub projection_revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FinalizationGenesisAnchor {
    pub block_hash: BlockHashSerde,
    pub block_number: i64,
    pub record_digest: BlockHashSerde,
    pub certificate_digest: BlockHashSerde,
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
    pub protocol_version: i64,
    pub shard_id: String,
    pub genesis_hash: BlockHashSerde,
    pub predecessor_hash: BlockHashSerde,
    pub predecessor_digest: BlockHashSerde,
    pub predecessor_certificate_digest: BlockHashSerde,
    pub predecessor_certificate_block_hash: BlockHashSerde,
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

impl LocalFinalizationWitness {
    pub fn to_certificate(&self) -> FinalizationCertificate {
        FinalizationCertificate {
            schema_version: self.schema_version,
            protocol_version: self.protocol_version,
            shard_id: self.shard_id.clone(),
            genesis_hash: self.genesis_hash.clone(),
            predecessor_floor_hash: self.predecessor_hash.clone(),
            predecessor_certificate_digest: self.predecessor_certificate_digest.clone(),
            predecessor_certificate_block_hash: self.predecessor_certificate_block_hash.clone(),
            target_floor_hash: self.target_block_hash.clone(),
            target_post_state_hash: self.target_post_state_hash.clone(),
            target_block_number: self.target_block_number,
            fault_tolerance_numerator: self.fault_tolerance_numerator,
            fault_tolerance_denominator: self.fault_tolerance_denominator,
            exact_latest_messages: self.latest_messages.clone(),
            authority_context_digest: self.authority_context_digest.clone(),
            supporting_manifest_digest: FinalizationCertificate::supporting_digest(
                &self.supporting_block_hashes,
            ),
            finalized_manifest_digest: FinalizationCertificate::finalized_digest(&self.finalized),
            supporting_block_count: u32::try_from(self.supporting_block_hashes.len())
                .unwrap_or(u32::MAX),
            finalized_block_count: u32::try_from(self.finalized.len()).unwrap_or(u32::MAX),
        }
    }
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

pub struct FinalizationRecordScan {
    ledger: FinalizationLedger,
    cursor: u64,
    through_revision: u64,
}

pub struct FinalizationEffectsAdvance {
    ledger: FinalizationLedger,
    through_revision: u64,
    cursor: u64,
    finished: bool,
    failure: Option<KvStoreError>,
}

impl FinalizationEffectsAdvance {
    pub fn through_revision(&self) -> u64 { self.through_revision }

    pub fn cursor(&self) -> u64 { self.cursor }

    pub fn advance_next_page(&mut self, max_rounds: NonZeroUsize) -> Result<bool, KvStoreError> {
        recovery_pages::advance_page(
            &self.ledger,
            recovery_pages::Advancement {
                target: self.through_revision,
                cursor: &mut self.cursor,
                finished: &mut self.finished,
                failure: &mut self.failure,
            },
            max_rounds,
        )
    }

    fn finish(mut self) -> Result<u64, KvStoreError> {
        while !self.advance_next_page(NonZeroUsize::new(32).unwrap())? {}
        Ok(self.cursor)
    }

    async fn finish_async(mut self) -> Result<u64, KvStoreError> {
        while !self.finished {
            self = tokio::task::spawn_blocking(move || {
                self.advance_next_page(NonZeroUsize::new(32).unwrap())?;
                Ok::<_, KvStoreError>(self)
            })
            .await
            .map_err(|error| {
                KvStoreError::IoError(format!("finalization cursor worker failed: {error}"))
            })??;
        }
        Ok(self.cursor)
    }
}

pub struct FinalizationIntegrityScan {
    ledger: FinalizationLedger,
    snapshot: Option<(FinalizationGenesisAnchor, FinalizationHead)>,
    validated: Option<FinalizationHead>,
    failure: Option<KvStoreError>,
    complete: bool,
}

struct LedgerReceiptFactory;

impl recovery_pages::ReceiptFactory for LedgerReceiptFactory {
    type Block = BlockHashSerde;
    type Key = FinalizationLedgerKey;
    const KINDS: usize = FinalizationEffectKind::ALL.len();

    fn effect(revision: u64, block: &BlockHashSerde, kind: usize) -> Self::Key {
        FinalizationLedgerKey::Effect(FinalizationEffectId {
            revision,
            block_hash: block.clone(),
            kind: FinalizationEffectKind::ALL[kind],
        })
    }

    fn complete(revision: u64) -> Self::Key { FinalizationLedgerKey::EffectsComplete(revision) }
}

type FinalizationReceiptKeys = recovery_pages::ReceiptKeys<
    std::collections::btree_set::IntoIter<BlockHashSerde>,
    LedgerReceiptFactory,
>;

pub struct FinalizationReceiptCompaction {
    ledger: FinalizationLedger,
    through_revision: u64,
    cursor: u64,
    pending: Option<std::iter::Peekable<FinalizationReceiptKeys>>,
    failure: Option<KvStoreError>,
}

impl FinalizationReceiptCompaction {
    pub fn through_revision(&self) -> u64 { self.through_revision }

    pub fn delete_next_page(&mut self, max_keys: NonZeroUsize) -> Result<bool, KvStoreError> {
        recovery_pages::compact_page(
            &self.ledger,
            recovery_pages::Compaction {
                target: self.through_revision,
                cursor: &mut self.cursor,
                pending: &mut self.pending,
                failure: &mut self.failure,
            },
            max_keys,
        )
    }
}

impl FinalizationIntegrityScan {
    pub fn captured_head(&self) -> Option<&FinalizationHead> {
        self.snapshot.as_ref().map(|(_, head)| head)
    }

    pub fn validated_revision(&self) -> u64 {
        self.validated.as_ref().map_or(0, |head| head.revision)
    }

    pub fn is_complete(&self) -> bool { self.complete }

    pub fn validate_next_page(&mut self, max_records: NonZeroUsize) -> Result<bool, KvStoreError> {
        integrity_pages::validate_page(
            &self.ledger,
            integrity_pages::Integrity {
                snapshot: &self.snapshot,
                validated: &mut self.validated,
                failure: &mut self.failure,
                complete: &mut self.complete,
            },
            max_records,
        )
    }
}

impl FinalizationRecordScan {
    pub fn through_revision(&self) -> u64 { self.through_revision }

    pub fn next_record(&mut self) -> Result<Option<FinalizationRecord>, KvStoreError> {
        if self.cursor == self.through_revision {
            return Ok(None);
        }
        let next = self.cursor.checked_add(1).ok_or_else(|| {
            KvStoreError::SerializationError("finalization scan cursor exhausted".to_string())
        })?;
        let record = self.ledger.record(next)?.ok_or_else(|| {
            KvStoreError::SerializationError(format!(
                "finalization ledger scan references missing round {next}"
            ))
        })?;
        self.cursor = next;
        Ok(Some(record))
    }

    fn collect(mut self) -> Result<Vec<FinalizationRecord>, KvStoreError> {
        let mut records = Vec::new();
        while let Some(record) = self.next_record()? {
            records.push(record);
        }
        Ok(records)
    }
}

impl FinalizationLedger {
    pub const STORE_NAME: &'static str = "finalization-ledger-v7";
    pub const WITNESS_SCHEMA_VERSION: u32 = FinalizationCertificate::SCHEMA_VERSION;
    const INTEGRITY_PAGE_RECORDS: NonZeroUsize = NonZeroUsize::new(32).unwrap();

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

    fn read_value(
        &self,
        key: &FinalizationLedgerKey,
    ) -> Result<Option<FinalizationLedgerValue>, KvStoreError> {
        let encoded_key = self.store.encode_key(key)?;
        let mut value = None;
        self.store
            .raw_store()
            .with_value(&encoded_key, &mut |encoded| {
                value = encoded
                    .map(|bytes| bounded_decode::decode(key, bytes))
                    .transpose()?;
                Ok(())
            })?;
        Ok(value)
    }

    fn digest(parts: impl IntoIterator<Item = Vec<u8>>) -> BlockHashSerde {
        let parts = parts.into_iter().collect::<Vec<_>>();
        BlockHashSerde(Bytes::from(Blake2b256::hash_parts(
            parts.iter().map(Vec::as_slice),
        )))
    }

    fn genesis_digest(block_hash: &BlockHash, block_number: i64) -> BlockHashSerde {
        Self::digest([
            b"f1r3-finalization-genesis-v7".to_vec(),
            block_hash.to_vec(),
            block_number.to_be_bytes().to_vec(),
        ])
    }

    fn manifest_digest(finalized: &BTreeSet<BlockHashSerde>) -> BlockHashSerde {
        let mut parts = Vec::with_capacity(finalized.len() + 2);
        parts.push(b"f1r3-finalization-manifest-v7".to_vec());
        parts.push((finalized.len() as u64).to_be_bytes().to_vec());
        parts.extend(finalized.iter().map(|hash| hash.0.to_vec()));
        Self::digest(parts)
    }

    fn record_digest(record: &FinalizationRecord) -> BlockHashSerde {
        let mut parts = Vec::with_capacity(record.finalized.len() + 11);
        parts.push(b"f1r3-finalization-record-v7".to_vec());
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
        BlockHashSerde(witness.to_certificate().digest())
    }

    pub fn prepare_witness(
        protocol_version: i64,
        shard_id: String,
        genesis_hash: BlockHash,
        expected: &FinalizationHead,
        target_block_hash: BlockHash,
        predecessor_certificate_digest: BlockHash,
        predecessor_certificate_block_hash: BlockHash,
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
            || protocol_version <= 0
            || shard_id.is_empty()
            || fault_tolerance_denominator <= 0
            || !supporting_block_hashes.contains(&BlockHashSerde(target_block_hash.clone()))
            || !finalized.contains(&BlockHashSerde(target_block_hash.clone()))
            || finalized
                .iter()
                .any(|hash| !supporting_block_hashes.contains(hash))
            || latest_messages
                .values()
                .any(|hash| !supporting_block_hashes.contains(hash))
            || ((expected.revision == 0)
                != predecessor_certificate_digest.iter().all(|byte| *byte == 0))
            || (predecessor_certificate_digest.iter().all(|byte| *byte == 0)
                != predecessor_certificate_block_hash
                    .iter()
                    .all(|byte| *byte == 0))
            || (!predecessor_certificate_digest.iter().all(|byte| *byte == 0)
                && !supporting_block_hashes
                    .contains(&BlockHashSerde(predecessor_certificate_block_hash.clone())))
            || supporting_block_hashes.len() > FinalizationCertificate::MAX_SUPPORTING_BLOCKS
            || finalized.len() > FinalizationCertificate::MAX_FINALIZED_BLOCKS
            || authority_context_digest.0.len() != models::rust::block_hash::LENGTH
        {
            return Err(KvStoreError::InvalidArgument(
                "local finalization witness is structurally incomplete".to_string(),
            ));
        }
        let mut witness = LocalFinalizationWitness {
            schema_version: Self::WITNESS_SCHEMA_VERSION,
            protocol_version,
            shard_id,
            genesis_hash: BlockHashSerde(genesis_hash),
            predecessor_hash: expected.block_hash.clone(),
            predecessor_digest: expected.record_digest.clone(),
            predecessor_certificate_digest: BlockHashSerde(predecessor_certificate_digest),
            predecessor_certificate_block_hash: BlockHashSerde(predecessor_certificate_block_hash),
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
        Self::validate_witness_standalone(&witness)?;
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
            || witness.protocol_version <= 0
            || witness.shard_id.is_empty()
            || witness.genesis_hash.0.len() != hash_length
            || witness.predecessor_hash.0.len() != hash_length
            || witness.predecessor_digest.0.len() != hash_length
            || witness.predecessor_certificate_digest.0.len() != hash_length
            || witness.predecessor_certificate_block_hash.0.len() != hash_length
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
            || ((witness.predecessor_hash == witness.genesis_hash)
                != witness
                    .predecessor_certificate_digest
                    .0
                    .iter()
                    .all(|byte| *byte == 0))
            || (witness
                .predecessor_certificate_digest
                .0
                .iter()
                .all(|byte| *byte == 0)
                != witness
                    .predecessor_certificate_block_hash
                    .0
                    .iter()
                    .all(|byte| *byte == 0))
            || (!witness
                .predecessor_certificate_digest
                .0
                .iter()
                .all(|byte| *byte == 0)
                && !witness
                    .supporting_block_hashes
                    .contains(&witness.predecessor_certificate_block_hash))
            || witness
                .supporting_block_hashes
                .iter()
                .chain(witness.finalized.iter())
                .any(|hash| hash.0.len() != hash_length)
            || witness.supporting_block_hashes.len()
                > FinalizationCertificate::MAX_SUPPORTING_BLOCKS
            || witness.finalized.len() > FinalizationCertificate::MAX_FINALIZED_BLOCKS
            || witness.witness_digest != Self::witness_digest(witness)
            || witness.to_certificate().validate_shape().is_err()
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
        match self.read_value(&key)? {
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
        match self.read_value(&FinalizationLedgerKey::Witness(digest.clone()))? {
            Some(FinalizationLedgerValue::Witness(witness)) => Ok(Some(witness)),
            Some(_) => Err(KvStoreError::SerializationError(
                "local finalization witness key contains a different value".to_string(),
            )),
            None => Ok(None),
        }
    }

    pub fn head(&self) -> Result<Option<FinalizationHead>, KvStoreError> {
        match self.read_value(&FinalizationLedgerKey::Head)? {
            Some(FinalizationLedgerValue::Head(head)) => Ok(Some(head)),
            Some(_) => Err(KvStoreError::SerializationError(
                "finalization ledger head key contains a non-head value".to_string(),
            )),
            None => Ok(None),
        }
    }

    pub fn settled_recovery_charge(
        &self,
        charge: &SettledRecoveryCharge,
    ) -> Result<Option<SettledRecoveryCharge>, KvStoreError> {
        charge.validate()?;
        match self.read_value(&FinalizationLedgerKey::SettledRecoveryCharge(
            Self::settled_recovery_charge_digest(charge),
        ))? {
            Some(FinalizationLedgerValue::SettledRecoveryCharge(charge)) => Ok(Some(charge)),
            Some(_) => Err(KvStoreError::SerializationError(
                "settled recovery charge key contains a different value".to_string(),
            )),
            None => Ok(None),
        }
    }

    fn settled_recovery_charge_digest(charge: &SettledRecoveryCharge) -> BlockHashSerde {
        Self::digest([
            b"f1r3-settled-recovery-charge-v1".to_vec(),
            charge.episode.digest().0.to_vec(),
            charge.target_block_hash.0.to_vec(),
            charge.citer_validator.0.to_vec(),
            charge.citer_bond_generation.to_be_bytes().to_vec(),
        ])
    }

    pub fn settled_recovery_usage(&self, episode: &RecoveryEpisodeId) -> Result<u64, KvStoreError> {
        episode.validate()?;
        match self.read_value(&FinalizationLedgerKey::SettledRecoveryUsage(
            episode.digest(),
        ))? {
            Some(FinalizationLedgerValue::SettledRecoveryUsage(usage)) => Ok(usage),
            Some(_) => Err(KvStoreError::SerializationError(
                "settled recovery usage key contains a different value".to_string(),
            )),
            None => Ok(0),
        }
    }

    pub(crate) fn prepare_settled_recovery_charge(
        &self,
        charge: &SettledRecoveryCharge,
        capacity: u64,
    ) -> Result<SettledRecoveryChargePlan, KvStoreError> {
        charge.validate()?;
        let charge_key = FinalizationLedgerKey::SettledRecoveryCharge(
            Self::settled_recovery_charge_digest(charge),
        );
        match self.read_value(&charge_key)? {
            Some(FinalizationLedgerValue::SettledRecoveryCharge(existing))
                if existing == *charge =>
            {
                return Ok(SettledRecoveryChargePlan {
                    store: self.store.raw_store().clone(),
                    mutations: Vec::new(),
                    committed: self.settled_recovery_usage(&charge.episode)?,
                });
            }
            Some(FinalizationLedgerValue::SettledRecoveryCharge(_)) => {
                return Err(KvStoreError::InvalidArgument(
                    "settled recovery target has a conflicting durable charge".to_string(),
                ));
            }
            Some(_) => {
                return Err(KvStoreError::SerializationError(
                    "settled recovery charge key contains a different value".to_string(),
                ));
            }
            None => {}
        }
        let usage_key = FinalizationLedgerKey::SettledRecoveryUsage(charge.episode.digest());
        let current_value = self.read_value(&usage_key)?;
        let current = match current_value.as_ref() {
            Some(FinalizationLedgerValue::SettledRecoveryUsage(usage)) => *usage,
            Some(_) => {
                return Err(KvStoreError::SerializationError(
                    "settled recovery usage key contains a different value".to_string(),
                ));
            }
            None => 0,
        };
        let committed = current.checked_add(1).ok_or_else(|| {
            KvStoreError::InvalidArgument("settled recovery usage exceeds u64".to_string())
        })?;
        if committed > capacity {
            return Err(KvStoreError::RecoveryBudgetExhausted {
                domain: "settled-history admission",
                capacity,
            });
        }
        Ok(SettledRecoveryChargePlan {
            store: self.store.raw_store().clone(),
            mutations: vec![
                (
                    self.store.encode_key(&charge_key)?,
                    AtomicStoreOperation::PutIfAbsentOrEqual(self.store.encode_value(
                        &FinalizationLedgerValue::SettledRecoveryCharge(charge.clone()),
                    )?),
                ),
                (
                    self.store.encode_key(&usage_key)?,
                    AtomicStoreOperation::CompareAndSwap {
                        expected: current_value
                            .map(|value| self.store.encode_value(&value))
                            .transpose()?,
                        replacement: Some(self.store.encode_value(
                            &FinalizationLedgerValue::SettledRecoveryUsage(committed),
                        )?),
                    },
                ),
            ],
            committed,
        })
    }

    pub(crate) fn settled_recovery_state(&self) -> Result<SettledRecoveryState, KvStoreError> {
        let mut charges = Vec::new();
        let mut usage = HashMap::new();
        self.store
            .raw_store()
            .visit_entries(&mut |encoded_key, encoded_value| {
                let Some((key, value)) = bounded_decode::recovery_row(encoded_key, encoded_value)?
                else {
                    return Ok(());
                };
                match (key, value) {
                    (
                        FinalizationLedgerKey::SettledRecoveryCharge(digest),
                        FinalizationLedgerValue::SettledRecoveryCharge(charge),
                    ) => {
                        charge.validate()?;
                        if digest != Self::settled_recovery_charge_digest(&charge) {
                            return Err(KvStoreError::SerializationError(
                                "settled recovery charge key does not match its value".to_string(),
                            ));
                        }
                        charges.push(charge);
                    }
                    (
                        FinalizationLedgerKey::SettledRecoveryUsage(episode),
                        FinalizationLedgerValue::SettledRecoveryUsage(count),
                    ) => {
                        if usage.insert(episode, count).is_some() {
                            return Err(KvStoreError::SerializationError(
                                "settled recovery usage contains a duplicate episode".to_string(),
                            ));
                        }
                    }
                    (
                        FinalizationLedgerKey::SettledRecoveryCharge(_)
                        | FinalizationLedgerKey::SettledRecoveryUsage(_),
                        _,
                    )
                    | (
                        _,
                        FinalizationLedgerValue::SettledRecoveryCharge(_)
                        | FinalizationLedgerValue::SettledRecoveryUsage(_),
                    ) => {
                        return Err(KvStoreError::SerializationError(
                            "settled recovery ledger key and value types differ".to_string(),
                        ));
                    }
                    _ => {}
                }
                Ok(())
            })?;
        Ok(SettledRecoveryState { charges, usage })
    }

    #[cfg(any(test, feature = "test-internals"))]
    pub(crate) fn clear_settled_recovery_state_for_tests(&self) -> Result<(), KvStoreError> {
        let keys = self
            .store
            .to_map()?
            .into_keys()
            .filter(|key| {
                matches!(
                    key,
                    FinalizationLedgerKey::SettledRecoveryCharge(_)
                        | FinalizationLedgerKey::SettledRecoveryUsage(_)
                )
            })
            .collect::<Vec<_>>();
        self.store.delete(keys)
    }

    #[cfg(any(test, feature = "test-internals"))]
    pub(crate) fn put_settled_recovery_usage_for_tests(
        &self,
        episode: &RecoveryEpisodeId,
        usage: u64,
    ) -> Result<(), KvStoreError> {
        self.store.put_one(
            FinalizationLedgerKey::SettledRecoveryUsage(episode.digest()),
            FinalizationLedgerValue::SettledRecoveryUsage(usage),
        )
    }

    pub(crate) fn commit_settled_recovery_migration(
        &self,
        episode: &RecoveryEpisodeId,
        missing: &[SettledRecoveryCharge],
        expected_usage: u64,
    ) -> Result<u64, KvStoreError> {
        episode.validate()?;
        if missing.is_empty() {
            return Ok(expected_usage);
        }
        let _guard = self.append_lock.lock();
        let usage_key = FinalizationLedgerKey::SettledRecoveryUsage(episode.digest());
        let current_value = self.read_value(&usage_key)?;
        let current_usage = match current_value.as_ref() {
            Some(FinalizationLedgerValue::SettledRecoveryUsage(count)) => *count,
            Some(_) => {
                return Err(KvStoreError::SerializationError(
                    "settled recovery usage key contains a different value".to_string(),
                ));
            }
            None => 0,
        };
        if current_usage != expected_usage {
            return Err(KvStoreError::TransactionConflict(
                "settled recovery usage changed during migration".to_string(),
            ));
        }

        let mut encoded = Vec::with_capacity(missing.len() + 1);
        let mut unique = BTreeSet::new();
        for charge in missing {
            charge.validate()?;
            if &charge.episode != episode {
                return Err(KvStoreError::InvalidArgument(
                    "settled recovery migration spans multiple episodes".to_string(),
                ));
            }
            let digest = Self::settled_recovery_charge_digest(charge);
            if !unique.insert(digest.clone()) {
                return Err(KvStoreError::InvalidArgument(
                    "settled recovery migration contains duplicate charges".to_string(),
                ));
            }
            let key = FinalizationLedgerKey::SettledRecoveryCharge(digest);
            match self.read_value(&key)? {
                Some(FinalizationLedgerValue::SettledRecoveryCharge(existing))
                    if existing == *charge =>
                {
                    continue;
                }
                Some(FinalizationLedgerValue::SettledRecoveryCharge(_)) => {
                    return Err(KvStoreError::InvalidArgument(
                        "settled recovery migration conflicts with a durable charge".to_string(),
                    ));
                }
                Some(_) => {
                    return Err(KvStoreError::SerializationError(
                        "settled recovery charge key contains a different value".to_string(),
                    ));
                }
                None => {}
            }
            encoded.push((
                self.store.encode_key(&key)?,
                AtomicStoreOperation::PutIfAbsentOrEqual(self.store.encode_value(
                    &FinalizationLedgerValue::SettledRecoveryCharge(charge.clone()),
                )?),
            ));
        }
        let increment = u64::try_from(encoded.len()).map_err(|_| {
            KvStoreError::InvalidArgument(
                "settled recovery migration count exceeds u64".to_string(),
            )
        })?;
        if increment == 0 {
            return Ok(current_usage);
        }
        let replacement_usage = current_usage.checked_add(increment).ok_or_else(|| {
            KvStoreError::InvalidArgument("settled recovery usage exceeds u64".to_string())
        })?;
        encoded.push((
            self.store.encode_key(&usage_key)?,
            AtomicStoreOperation::CompareAndSwap {
                expected: current_value
                    .map(|value| self.store.encode_value(&value))
                    .transpose()?,
                replacement: Some(self.store.encode_value(
                    &FinalizationLedgerValue::SettledRecoveryUsage(replacement_usage),
                )?),
            },
        ));
        let mutations = encoded
            .iter()
            .map(|(key, operation)| AtomicStoreMutation {
                store: self.store.raw_store().as_ref(),
                key: key.clone(),
                operation: operation.clone(),
            })
            .collect::<Vec<_>>();
        #[cfg(test)]
        crate::allocation_probe::mark(0);
        strict_atomic_mutate(&mutations)?;
        #[cfg(test)]
        crate::allocation_probe::mark(1);
        Ok(replacement_usage)
    }

    pub(crate) fn projection_endpoint(
        &self,
    ) -> Result<Option<FinalizationProjectionEndpoint>, KvStoreError> {
        let _guard = self.append_lock.lock();
        let Some(head) = self.head()? else {
            return Ok(None);
        };
        let projection_revision = self.projection_cursor()?;
        if projection_revision > head.revision {
            return Err(KvStoreError::SerializationError(format!(
                "finalization projection cursor {projection_revision} exceeds durable head {}",
                head.revision
            )));
        }
        Ok(Some(FinalizationProjectionEndpoint {
            head,
            projection_revision,
        }))
    }

    fn genesis(&self) -> Result<Option<FinalizationGenesisAnchor>, KvStoreError> {
        match self.read_value(&FinalizationLedgerKey::Genesis)? {
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
            certificate_digest: BlockHashSerde(Bytes::from(vec![
                0;
                models::rust::block_hash::LENGTH
            ])),
        }
    }

    fn genesis_head(genesis: &FinalizationGenesisAnchor) -> FinalizationHead {
        FinalizationHead {
            revision: 0,
            block_hash: genesis.block_hash.clone(),
            block_number: genesis.block_number,
            record_digest: genesis.record_digest.clone(),
            certificate_digest: genesis.certificate_digest.clone(),
        }
    }

    fn record_head(record: &FinalizationRecord) -> FinalizationHead {
        FinalizationHead {
            revision: record.revision,
            block_hash: record.directly_finalized.clone(),
            block_number: record.block_number,
            record_digest: record.record_digest.clone(),
            certificate_digest: record.witness_digest.clone(),
        }
    }

    fn validate_cursor_bounds(&self, head_revision: u64) -> Result<(), KvStoreError> {
        recovery_pages::validate_cursor_bounds(self, head_revision)
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

    pub fn begin_integrity_scan(&self) -> Result<FinalizationIntegrityScan, KvStoreError> {
        let captured = integrity_pages::capture(self)?;
        Ok(FinalizationIntegrityScan {
            ledger: self.clone(),
            snapshot: captured.snapshot,
            validated: captured.validated,
            failure: None,
            complete: captured.complete,
        })
    }

    pub fn validate_integrity(&self) -> Result<(), KvStoreError> {
        let mut scan = self.begin_integrity_scan()?;
        while !scan.validate_next_page(Self::INTEGRITY_PAGE_RECORDS)? {}
        Ok(())
    }

    pub async fn validate_integrity_async(&self) -> Result<(), KvStoreError> {
        let ledger = self.clone();
        let mut scan = tokio::task::spawn_blocking(move || ledger.begin_integrity_scan())
            .await
            .map_err(|error| {
                KvStoreError::IoError(format!("finalization audit worker failed: {error}"))
            })??;
        while !scan.is_complete() {
            scan = tokio::task::spawn_blocking(move || {
                scan.validate_next_page(Self::INTEGRITY_PAGE_RECORDS)?;
                Ok::<_, KvStoreError>(scan)
            })
            .await
            .map_err(|error| {
                KvStoreError::IoError(format!("finalization audit worker failed: {error}"))
            })??;
        }
        Ok(())
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
                certificate_digest: record.witness_digest.clone(),
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
        match self.read_value(&FinalizationLedgerKey::Round(revision))? {
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
        match self.read_value(&FinalizationLedgerKey::ProjectionCursor)? {
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
        match self.read_value(&FinalizationLedgerKey::EffectsCursor)? {
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
        match self.read_value(&FinalizationLedgerKey::EffectsCompactionCursor)? {
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

    fn record_scan(
        &self,
        after_revision: u64,
        through_revision: u64,
    ) -> Result<FinalizationRecordScan, KvStoreError> {
        if after_revision > through_revision {
            return Err(KvStoreError::SerializationError(format!(
                "finalization cursor {after_revision} exceeds scan target {through_revision}"
            )));
        }
        Ok(FinalizationRecordScan {
            ledger: self.clone(),
            cursor: after_revision,
            through_revision,
        })
    }

    pub fn pending_projection_scan(&self) -> Result<FinalizationRecordScan, KvStoreError> {
        let _guard = self.append_lock.lock();
        let head = self.head()?.ok_or_else(|| {
            KvStoreError::InvalidArgument(
                "finalization ledger has no initialized genesis head".to_string(),
            )
        })?;
        self.validate_cursor_bounds(head.revision)?;
        self.record_scan(self.projection_cursor()?, head.revision)
    }

    pub fn pending_projection_records(&self) -> Result<Vec<FinalizationRecord>, KvStoreError> {
        self.pending_projection_scan()?.collect()
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
        self.pending_effect_scan()?.collect()
    }

    pub fn pending_effect_scan(&self) -> Result<FinalizationRecordScan, KvStoreError> {
        let window = recovery_pages::select_effects(self)?;
        self.record_scan(window.cursor, window.target)
    }

    fn require_effect_projection_locked(&self, revision: u64) -> Result<(), KvStoreError> {
        recovery_pages::require_projection_locked(self, revision)
    }

    pub fn require_effect_projection(&self, revision: u64) -> Result<(), KvStoreError> {
        recovery_pages::require_projection(self, revision)
    }

    fn round_effects_complete(&self, revision: u64) -> Result<bool, KvStoreError> {
        match self.read_value(&FinalizationLedgerKey::EffectsComplete(revision))? {
            Some(FinalizationLedgerValue::EffectsComplete) => Ok(true),
            Some(_) => Err(KvStoreError::SerializationError(format!(
                "finalization effects-complete key {revision} contains an invalid value"
            ))),
            None => Ok(false),
        }
    }

    pub fn effect_completed(&self, id: &FinalizationEffectId) -> Result<bool, KvStoreError> {
        effect_observation::effect_is_complete(
            id.revision,
            || self.effects_cursor(),
            || match self.read_value(&FinalizationLedgerKey::Effect(id.clone()))? {
                Some(FinalizationLedgerValue::Effect) => Ok(true),
                Some(_) => Err(KvStoreError::SerializationError(
                    "finalization effect key contains a non-effect value".to_string(),
                )),
                None => Ok(false),
            },
        )
    }

    pub fn record_effect(&self, id: FinalizationEffectId) -> Result<(), KvStoreError> {
        let _guard = self.append_lock.lock();
        self.require_effect_projection_locked(id.revision)?;
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

    fn commit_round_effects_completed(
        &self,
        revision: u64,
    ) -> Result<FinalizationEffectsAdvance, KvStoreError> {
        let _guard = self.append_lock.lock();
        self.require_effect_projection_locked(revision)?;
        let record = self.record(revision)?.ok_or_else(|| {
            KvStoreError::InvalidArgument(format!(
                "cannot complete effects for uncommitted finalization round {revision}"
            ))
        })?;
        let old_cursor = self.effects_cursor()?;
        if revision <= old_cursor {
            return self.effects_advance_scan_locked();
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
        self.store.put_one(
            FinalizationLedgerKey::EffectsComplete(revision),
            FinalizationLedgerValue::EffectsComplete,
        )?;
        self.effects_advance_scan_locked()
    }

    fn effects_advance_scan_locked(&self) -> Result<FinalizationEffectsAdvance, KvStoreError> {
        Ok(self.effects_advance_from_window(recovery_pages::advancement_snapshot_locked(self)?))
    }

    fn effects_advance_from_window(
        &self,
        window: recovery_pages::Window,
    ) -> FinalizationEffectsAdvance {
        FinalizationEffectsAdvance {
            ledger: self.clone(),
            through_revision: window.target,
            cursor: window.cursor,
            finished: window.cursor == window.target,
            failure: None,
        }
    }

    pub fn begin_effects_cursor_advance(&self) -> Result<FinalizationEffectsAdvance, KvStoreError> {
        Ok(self.effects_advance_from_window(recovery_pages::begin_advancement(self)?))
    }

    pub async fn reconcile_effects_cursor_async(&self) -> Result<u64, KvStoreError> {
        let ledger = self.clone();
        let scan = tokio::task::spawn_blocking(move || ledger.begin_effects_cursor_advance())
            .await
            .map_err(|error| {
                KvStoreError::IoError(format!("finalization cursor worker failed: {error}"))
            })??;
        scan.finish_async().await
    }

    pub fn record_round_effects_completed(&self, revision: u64) -> Result<u64, KvStoreError> {
        let cursor = self.commit_round_effects_completed(revision)?.finish()?;
        self.reconcile_effect_compaction()?;
        Ok(cursor)
    }

    pub async fn record_round_effects_completed_async(
        &self,
        revision: u64,
    ) -> Result<u64, KvStoreError> {
        let ledger = self.clone();
        let scan =
            tokio::task::spawn_blocking(move || ledger.commit_round_effects_completed(revision))
                .await
                .map_err(|error| {
                    KvStoreError::IoError(format!("finalization completion worker failed: {error}"))
                })??;
        let cursor = scan.finish_async().await?;
        self.reconcile_effect_compaction_async().await?;
        Ok(cursor)
    }

    pub fn begin_effect_compaction(&self) -> Result<FinalizationReceiptCompaction, KvStoreError> {
        let window = recovery_pages::begin_compaction(self)?;
        Ok(FinalizationReceiptCompaction {
            ledger: self.clone(),
            through_revision: window.target,
            cursor: window.cursor,
            pending: None,
            failure: None,
        })
    }

    pub fn reconcile_effect_compaction(&self) -> Result<(), KvStoreError> {
        let mut scan = self.begin_effect_compaction()?;
        while !scan.delete_next_page(NonZeroUsize::new(256).unwrap())? {}
        Ok(())
    }

    pub async fn reconcile_effect_compaction_async(&self) -> Result<(), KvStoreError> {
        let ledger = self.clone();
        let mut scan = tokio::task::spawn_blocking(move || ledger.begin_effect_compaction())
            .await
            .map_err(|error| {
                KvStoreError::IoError(format!("finalization compaction worker failed: {error}"))
            })??;
        while scan.cursor < scan.through_revision {
            scan = tokio::task::spawn_blocking(move || {
                scan.delete_next_page(NonZeroUsize::new(256).unwrap())?;
                Ok::<_, KvStoreError>(scan)
            })
            .await
            .map_err(|error| {
                KvStoreError::IoError(format!("finalization compaction worker failed: {error}"))
            })??;
        }
        Ok(())
    }
}

impl recovery_pages::ExclusiveGate for Mutex<()> {
    type Guard<'a> = parking_lot::MutexGuard<'a, ()>;

    fn lock(&self) -> Self::Guard<'_> { Mutex::lock(self) }
}

impl recovery_pages::RecoveryStore for FinalizationLedger {
    type Error = KvStoreError;
    type Gate = Mutex<()>;
    type Keys = FinalizationReceiptKeys;

    fn gate(&self) -> &Self::Gate { &self.append_lock }

    fn head_revision(&self) -> Result<Option<u64>, Self::Error> {
        Ok(self.head()?.map(|head| head.revision))
    }

    fn projection_cursor(&self) -> Result<u64, Self::Error> {
        FinalizationLedger::projection_cursor(self)
    }

    fn effects_cursor(&self) -> Result<u64, Self::Error> {
        FinalizationLedger::effects_cursor(self)
    }

    fn compaction_cursor(&self) -> Result<u64, Self::Error> { self.effects_compaction_cursor() }

    fn round_complete(&self, revision: u64) -> Result<bool, Self::Error> {
        self.round_effects_complete(revision)
    }

    fn write_effects_cursor(&self, revision: u64) -> Result<(), Self::Error> {
        self.store.put_one(
            FinalizationLedgerKey::EffectsCursor,
            FinalizationLedgerValue::EffectsCursor(revision),
        )
    }

    fn receipt_keys(&self, revision: u64) -> Result<Self::Keys, Self::Error> {
        let record = self.record(revision)?.ok_or_else(|| {
            KvStoreError::SerializationError(format!(
                "cannot compact effects for missing finalization round {revision}"
            ))
        })?;
        Ok(FinalizationReceiptKeys::new(
            revision,
            record.finalized.into_iter(),
        ))
    }

    fn delete_receipts(&self, keys: Vec<FinalizationLedgerKey>) -> Result<(), Self::Error> {
        self.store.delete(keys).map(|_| ())
    }

    fn write_compaction_cursor(&self, revision: u64) -> Result<(), Self::Error> {
        self.store.put_one(
            FinalizationLedgerKey::EffectsCompactionCursor,
            FinalizationLedgerValue::EffectsCompactionCursor(revision),
        )
    }

    fn serialization(message: String) -> Self::Error { KvStoreError::SerializationError(message) }

    fn invalid(message: String) -> Self::Error { KvStoreError::InvalidArgument(message) }

    fn projection_pending(revision: u64, projected_revision: u64) -> Self::Error {
        KvStoreError::FinalizationProjectionPending {
            revision,
            projected_revision,
        }
    }
}

impl integrity_pages::IntegrityStore for FinalizationLedger {
    type Genesis = FinalizationGenesisAnchor;
    type Head = FinalizationHead;

    fn genesis(&self) -> Result<Option<Self::Genesis>, Self::Error> {
        FinalizationLedger::genesis(self)
    }

    fn head(&self) -> Result<Option<Self::Head>, Self::Error> { FinalizationLedger::head(self) }

    fn non_empty(&self) -> Result<bool, Self::Error> { self.store.non_empty() }

    fn genesis_head(genesis: &Self::Genesis) -> Self::Head {
        FinalizationLedger::genesis_head(genesis)
    }

    fn revision(head: &Self::Head) -> u64 { head.revision }

    fn validate_initialized_endpoints(
        &self,
        genesis: &Self::Genesis,
        head: &Self::Head,
    ) -> Result<(), Self::Error> {
        FinalizationLedger::validate_initialized_endpoints(self, genesis, head)
    }

    fn record_head(&self, revision: u64) -> Result<Option<Self::Head>, Self::Error> {
        Ok(self
            .record(revision)?
            .as_ref()
            .map(FinalizationLedger::record_head))
    }

    fn validate_next(
        &self,
        expected: &Self::Head,
        revision: u64,
    ) -> Result<Self::Head, Self::Error> {
        let record = self.record(revision)?.ok_or_else(|| {
            KvStoreError::SerializationError(format!(
                "finalization ledger head references missing round {revision}"
            ))
        })?;
        FinalizationLedger::validate_record(expected, &record)?;
        let witness = self.witness(&record.witness_digest)?.ok_or_else(|| {
            KvStoreError::SerializationError(format!(
                "finalization round {revision} has no portable witness"
            ))
        })?;
        FinalizationLedger::validate_witness(expected, &witness)?;
        if witness.target_block_hash != record.directly_finalized
            || witness.target_block_number != record.block_number
            || witness.finalized != record.finalized
        {
            return Err(KvStoreError::SerializationError(format!(
                "finalization round {revision} witness does not match its record"
            )));
        }
        Ok(FinalizationLedger::record_head(&record))
    }
}

#[cfg(test)]
mod tests {
    mod completion_observation;
    #[cfg(unix)]
    mod durability;
    mod startup_readiness;
    mod resource_measurements;

    use std::collections::HashSet;
    use std::sync::Barrier;
    use std::time::Duration;

    use proptest::prelude::*;
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

    use super::*;

    struct AuditStoreLifetime(Option<tokio::sync::oneshot::Sender<()>>);

    impl Drop for AuditStoreLifetime {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    type ReadObserver = dyn Fn(&[Vec<u8>]) -> Result<(), KvStoreError> + Send + Sync;
    type WriteObserver = dyn Fn(&[(Vec<u8>, Vec<u8>)]) -> Result<(), KvStoreError> + Send + Sync;

    #[derive(Clone)]
    struct ObservedAuditStore {
        inner: Arc<dyn KeyValueStore>,
        on_read: Arc<ReadObserver>,
        on_delete: Option<Arc<ReadObserver>>,
        on_put: Option<Arc<WriteObserver>>,
        _lifetime: Arc<AuditStoreLifetime>,
    }

    impl KeyValueStore for ObservedAuditStore {
        fn as_any(&self) -> &dyn std::any::Any { self }

        fn with_value(
            &self,
            key: &Vec<u8>,
            reader: &mut shared::rust::store::key_value_store::ValueReader<'_>,
        ) -> Result<(), KvStoreError> {
            (self.on_read)(std::slice::from_ref(key))?;
            self.inner.with_value(key, reader)
        }

        fn visit_entries(
            &self,
            reader: &mut shared::rust::store::key_value_store::EntryReader<'_>,
        ) -> Result<(), KvStoreError> {
            self.inner.visit_entries(&mut |key, value| {
                (self.on_read)(&[key.to_vec()])?;
                reader(key, value)
            })
        }

        fn get(&self, keys: &Vec<Vec<u8>>) -> Result<Vec<Option<Vec<u8>>>, KvStoreError> {
            (self.on_read)(keys)?;
            self.inner.get(keys)
        }

        fn put(&self, pairs: Vec<(Vec<u8>, Vec<u8>)>) -> Result<(), KvStoreError> {
            if let Some(observer) = &self.on_put {
                observer(&pairs)?;
            }
            self.inner.put(pairs)
        }

        fn put_one_if_absent(&self, key: Vec<u8>, value: Vec<u8>) -> Result<bool, KvStoreError> {
            self.inner.put_one_if_absent(key, value)
        }

        fn delete(&self, keys: Vec<Vec<u8>>) -> Result<usize, KvStoreError> {
            if let Some(observer) = &self.on_delete {
                observer(&keys)?;
            }
            self.inner.delete(keys)
        }

        fn iterate(&self, _f: fn(Vec<u8>, Vec<u8>)) -> Result<(), KvStoreError> {
            Err(KvStoreError::InvalidArgument(
                "audit attempted a whole-store scan".to_string(),
            ))
        }

        fn iterate_while(
            &self,
            _f: &mut dyn FnMut(Vec<u8>, Vec<u8>) -> Result<bool, KvStoreError>,
        ) -> Result<(), KvStoreError> {
            Err(KvStoreError::InvalidArgument(
                "audit attempted a whole-store scan".to_string(),
            ))
        }

        fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

        fn to_map(&self) -> Result<BTreeMap<Vec<u8>, Vec<u8>>, KvStoreError> {
            Err(KvStoreError::InvalidArgument(
                "audit attempted a whole-store scan".to_string(),
            ))
        }

        fn print_store(&self) -> Result<(), KvStoreError> { self.inner.print_store() }

        fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }

        fn size_bytes(&self) -> usize { self.inner.size_bytes() }
    }

    fn hash(byte: u8) -> BlockHash { Bytes::from(vec![byte; 32]) }

    fn ledger_with_committed_rounds(rounds: u8) -> FinalizationLedger {
        let ledger = ledger();
        let mut head = initialize(&ledger, hash(0), 0);
        for revision in 1..=rounds {
            let record = prepare_record(
                &ledger,
                &head,
                hash(revision),
                i64::from(revision),
                0.75,
                BTreeSet::from([BlockHashSerde(hash(revision))]),
            )
            .unwrap();
            head = match ledger.try_append(&head, &record).unwrap() {
                FinalizationAppendOutcome::Committed(head) => head,
                outcome => panic!("unexpected append outcome: {outcome:?}"),
            };
        }
        ledger
    }

    fn all_writer_rows() -> HashMap<FinalizationLedgerKey, FinalizationLedgerValue> {
        let ledger = ledger_with_committed_rounds(2);
        let mut rows = ledger.store.to_map().unwrap();
        let id = FinalizationEffectId {
            revision: 1,
            block_hash: BlockHashSerde(hash(1)),
            kind: FinalizationEffectKind::FinalizedEvent,
        };
        rows.insert(
            FinalizationLedgerKey::Effect(id),
            FinalizationLedgerValue::Effect,
        );
        rows.insert(
            FinalizationLedgerKey::EffectsComplete(1),
            FinalizationLedgerValue::EffectsComplete,
        );
        let charge = recovery_charge(&recovery_episode(1, 10), 20, 30, 1);
        rows.insert(
            FinalizationLedgerKey::SettledRecoveryCharge(
                FinalizationLedger::settled_recovery_charge_digest(&charge),
            ),
            FinalizationLedgerValue::SettledRecoveryCharge(charge),
        );
        rows.insert(
            FinalizationLedgerKey::SettledRecoveryUsage(BlockHashSerde(hash(10))),
            FinalizationLedgerValue::SettledRecoveryUsage(1),
        );
        rows
    }

    #[test]
    fn bounded_decoder_preserves_all_writer_row_variants_and_rejects_truncation() {
        let rows = all_writer_rows();
        let tags = rows
            .values()
            .map(|value| {
                let encoded = bincode::serialize(value).unwrap();
                u32::from_le_bytes(encoded[..4].try_into().unwrap())
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(tags, (0..=10).collect());
        for (key, value) in rows {
            let encoded = bincode::serialize(&value).unwrap();
            assert_eq!(bounded_decode::decode(&key, &encoded).unwrap(), value);
            for cut in 0..encoded.len() {
                assert!(
                    bounded_decode::decode(&key, &encoded[..cut]).is_err(),
                    "accepted truncation {cut} for {key:?}"
                );
            }
            let mut trailing = encoded;
            trailing.push(0);
            assert!(bounded_decode::decode(&key, &trailing).is_err());
        }
    }

    #[test]
    fn recovery_scan_preserves_all_key_value_classifications() {
        let rows = all_writer_rows();
        for key in rows.keys() {
            for value in rows.values() {
                let encoded_key = bincode::serialize(key).unwrap();
                let encoded_value = bincode::serialize(value).unwrap();
                let key_tag = u32::from_le_bytes(encoded_key[..4].try_into().unwrap());
                let value_tag = u32::from_le_bytes(encoded_value[..4].try_into().unwrap());
                let result = bounded_decode::recovery_row(&encoded_key, &encoded_value);
                if key_tag >= 9 || value_tag >= 9 {
                    if key_tag == value_tag {
                        assert_eq!(result.unwrap(), Some((key.clone(), value.clone())));
                    } else {
                        assert!(
                            result.is_err(),
                            "accepted recovery mismatch {key_tag}/{value_tag}"
                        );
                    }
                } else {
                    assert_eq!(result.unwrap(), None);
                }
            }
        }
    }

    #[test]
    fn recovery_scan_checks_unselected_rows_and_rejects_all_truncations() {
        for (key, value) in all_writer_rows() {
            let encoded_key = bincode::serialize(&key).unwrap();
            let encoded_value = bincode::serialize(&value).unwrap();
            for cut in 0..encoded_key.len() {
                assert!(bounded_decode::recovery_row(&encoded_key[..cut], &encoded_value).is_err());
            }
            for cut in 0..encoded_value.len() {
                assert!(bounded_decode::recovery_row(&encoded_key, &encoded_value[..cut]).is_err());
            }
            let mut trailing_key = encoded_key.clone();
            trailing_key.push(0);
            assert!(bounded_decode::recovery_row(&trailing_key, &encoded_value).is_err());
            let mut trailing_value = encoded_value.clone();
            trailing_value.push(0);
            assert!(bounded_decode::recovery_row(&encoded_key, &trailing_value).is_err());
            let corrupt = vec![255u8; 4];
            assert!(bounded_decode::recovery_row(&corrupt, &encoded_value).is_err());
            assert!(bounded_decode::recovery_row(&encoded_key, &corrupt).is_err());
        }
    }

    #[test]
    fn recovery_scan_visits_every_row_without_owned_enumeration() {
        let original = ledger();
        let rows = all_writer_rows();
        let mut expected_charges = Vec::new();
        let mut expected_usage = HashMap::new();
        for (key, value) in &rows {
            match (key, value) {
                (_, FinalizationLedgerValue::SettledRecoveryCharge(charge)) => {
                    expected_charges.push(charge.clone())
                }
                (
                    FinalizationLedgerKey::SettledRecoveryUsage(episode),
                    FinalizationLedgerValue::SettledRecoveryUsage(count),
                ) => {
                    expected_usage.insert(episode.clone(), *count);
                }
                _ => {}
            }
            original.store.put_one(key.clone(), value.clone()).unwrap();
        }
        let visits = Arc::new(Mutex::new(HashSet::new()));
        let observer_visits = visits.clone();
        let observed = FinalizationLedger::from_store(Arc::new(ObservedAuditStore {
            inner: original.store.raw_store().clone(),
            on_read: Arc::new(move |keys| {
                assert_eq!(keys.len(), 1);
                assert!(observer_visits.lock().insert(keys[0].clone()));
                Ok(())
            }),
            on_delete: None,
            on_put: None,
            _lifetime: Arc::new(AuditStoreLifetime(None)),
        }));
        let state = observed.settled_recovery_state().unwrap();
        assert_eq!(state.charges, expected_charges);
        assert_eq!(state.usage, expected_usage);
        assert_eq!(visits.lock().len(), rows.len());
        let bad_key = original
            .store
            .encode_key(&FinalizationLedgerKey::Round(999))
            .unwrap();
        original
            .store
            .raw_store()
            .put_one(bad_key, vec![255; 4])
            .unwrap();
        visits.lock().clear();
        assert!(observed.settled_recovery_state().is_err());
    }

    proptest! {
        #[test]
        fn recovery_scan_rejects_forged_key_lengths_before_allocation(
            declared in any::<u64>(),
            tag in 8u32..11,
        ) {
            let mut key = tag.to_le_bytes().to_vec();
            key.extend_from_slice(&declared.to_le_bytes());
            key.extend_from_slice(&[1; 32]);
            let value = bincode::serialize(&FinalizationLedgerValue::Effect).unwrap();
            let result = bounded_decode::recovery_row(&key, &value);
            if declared == 32 && tag == 8 {
                prop_assert_eq!(result.unwrap(), None);
            } else {
                prop_assert!(result.is_err());
            }
        }

        #[test]
        fn recovery_scan_matches_materialized_selection_across_insertion_orders(
            targets in proptest::collection::btree_set(1u8..100, 0..32),
            rounds in 0u8..5,
            reverse in any::<bool>(),
        ) {
            let ledger = ledger_with_committed_rounds(rounds);
            let episode = recovery_episode(1, 150);
            let mut charges = targets.iter().map(|target| recovery_charge(&episode, *target, 200, 1)).collect::<Vec<_>>();
            if reverse { charges.reverse(); }
            ledger.commit_settled_recovery_migration(&episode, &charges, 0).unwrap();
            let reference = ledger.store.to_map().unwrap();
            let expected_charges = reference.values().filter_map(|value| match value {
                FinalizationLedgerValue::SettledRecoveryCharge(charge) => Some((FinalizationLedger::settled_recovery_charge_digest(charge), charge.clone())),
                _ => None,
            }).collect::<HashMap<_, _>>();
            let expected_usage = reference.into_iter().filter_map(|(key, value)| match (key, value) {
                (FinalizationLedgerKey::SettledRecoveryUsage(episode), FinalizationLedgerValue::SettledRecoveryUsage(count)) => Some((episode, count)),
                _ => None,
            }).collect::<HashMap<_, _>>();
            let state = ledger.settled_recovery_state().unwrap();
            let actual = state.charges.into_iter().map(|charge| (FinalizationLedger::settled_recovery_charge_digest(&charge), charge)).collect::<HashMap<_, _>>();
            prop_assert_eq!(actual, expected_charges);
            prop_assert_eq!(state.usage, expected_usage);
        }
    }

    #[test]
    fn bounded_decoder_preserves_long_recovery_shards_without_a_new_protocol_cap() {
        let ledger = ledger();
        let mut episode = recovery_episode(1, 10);
        episode.shard_id = "long-shard".repeat(1024);
        let charge = recovery_charge(&episode, 20, 30, 1);
        let key = FinalizationLedgerKey::SettledRecoveryCharge(
            FinalizationLedger::settled_recovery_charge_digest(&charge),
        );
        let value = FinalizationLedgerValue::SettledRecoveryCharge(charge.clone());
        ledger.store.put_one(key, value).unwrap();
        assert_eq!(
            ledger.settled_recovery_charge(&charge).unwrap(),
            Some(charge)
        );
    }

    #[test]
    fn bounded_decoder_accepts_the_maximum_writer_witness_and_checks_all_collection_limits() {
        let ledger = ledger_with_committed_rounds(1);
        let record = ledger.record(1).unwrap().unwrap();
        let mut witness = ledger.witness(&record.witness_digest).unwrap().unwrap();
        witness.shard_id = "s".repeat(FinalizationCertificate::MAX_SHARD_ID_BYTES);
        witness.latest_messages = (0..FinalizationCertificate::MAX_EXACT_LATEST_MESSAGES)
            .map(|index| {
                let mut validator = vec![0u8; models::rust::validator::LENGTH];
                validator[..4].copy_from_slice(&u32::try_from(index).unwrap().to_le_bytes());
                (
                    ValidatorSerde(Bytes::from(validator)),
                    witness.target_block_hash.clone(),
                )
            })
            .collect();
        witness.supporting_block_hashes = (0..FinalizationCertificate::MAX_SUPPORTING_BLOCKS)
            .map(|index| {
                if index == 0 {
                    witness.target_block_hash.clone()
                } else {
                    let mut bytes = vec![0u8; models::rust::block_hash::LENGTH];
                    bytes[..4].copy_from_slice(&u32::try_from(index).unwrap().to_le_bytes());
                    BlockHashSerde(Bytes::from(bytes))
                }
            })
            .collect();
        witness.finalized = witness
            .supporting_block_hashes
            .iter()
            .take(FinalizationCertificate::MAX_FINALIZED_BLOCKS)
            .cloned()
            .collect();
        witness.witness_digest = FinalizationLedger::witness_digest(&witness);
        let (_, validation) = crate::allocation_probe::measure(|| {
            FinalizationLedger::validate_witness_standalone(&witness).unwrap();
        });
        assert_eq!(validation.live_delta, 0);
        crate::allocation_probe::report(
            "maximum_witness_validation",
            witness.supporting_block_hashes.len(),
            22_102_208,
            validation,
        );
        let key = FinalizationLedgerKey::Witness(witness.witness_digest.clone());
        let latest_offset = 328 + witness.shard_id.len();
        let support_offset = latest_offset + 8 + 113 * witness.latest_messages.len();
        let finalized_offset = support_offset + 8 + 40 * witness.supporting_block_hashes.len() + 40;
        let value = FinalizationLedgerValue::Witness(witness);
        let mut encoded = bincode::serialize(&value).unwrap();
        assert_eq!(encoded.len(), 22_102_208);
        let (decoded, decoding) =
            crate::allocation_probe::measure(|| bounded_decode::decode(&key, &encoded).unwrap());
        assert_eq!(decoded, value);
        crate::allocation_probe::report(
            "maximum_witness_decode",
            FinalizationCertificate::MAX_SUPPORTING_BLOCKS,
            encoded.len(),
            decoding,
        );
        let (_, released) = crate::allocation_probe::measure(|| drop(decoded));
        assert_eq!(released.live_delta, -decoding.live_delta);
        for (offset, max) in [
            (16, FinalizationCertificate::MAX_SHARD_ID_BYTES),
            (
                latest_offset,
                FinalizationCertificate::MAX_EXACT_LATEST_MESSAGES,
            ),
            (
                support_offset,
                FinalizationCertificate::MAX_SUPPORTING_BLOCKS,
            ),
            (
                finalized_offset,
                FinalizationCertificate::MAX_FINALIZED_BLOCKS,
            ),
        ] {
            let original: [u8; 8] = encoded[offset..offset + 8].try_into().unwrap();
            for length in [u64::try_from(max).unwrap() + 1, u64::MAX] {
                encoded[offset..offset + 8].copy_from_slice(&length.to_le_bytes());
                assert!(bounded_decode::decode(&key, &encoded).is_err());
            }
            encoded[offset..offset + 8].copy_from_slice(&original);
        }
    }

    proptest! {
        #[test]
        fn bounded_decoder_rejects_forged_record_lengths_before_deserialization(
            length in any::<u64>(),
            field in prop_oneof![Just(12usize), Just(52), Just(92), Just(144)],
        ) {
            let ledger = ledger_with_committed_rounds(1);
            let key = FinalizationLedgerKey::Round(1);
            let value = ledger.store.get_one(&key).unwrap().unwrap();
            let mut encoded = bincode::serialize(&value).unwrap();
            let original = u64::from_le_bytes(encoded[field..field + 8].try_into().unwrap());
            encoded[field..field + 8].copy_from_slice(&length.to_le_bytes());
            if length != original {
                prop_assert!(bounded_decode::decode(&key, &encoded).is_err());
            } else {
                prop_assert_eq!(bounded_decode::decode(&key, &encoded).unwrap(), value);
            }
        }

        #[test]
        fn bounded_decoder_acceptance_agrees_with_the_existing_serializer(
            encoded in proptest::collection::vec(any::<u8>(), 0..1024),
            tag in 0u8..11,
        ) {
            let key = match tag {
                0 => FinalizationLedgerKey::Head,
                1 => FinalizationLedgerKey::Round(1),
                2 => FinalizationLedgerKey::Effect(FinalizationEffectId { revision: 1, block_hash: BlockHashSerde(hash(1)), kind: FinalizationEffectKind::DeployRemoval }),
                3 => FinalizationLedgerKey::ProjectionCursor,
                4 => FinalizationLedgerKey::EffectsCursor,
                5 => FinalizationLedgerKey::EffectsComplete(1),
                6 => FinalizationLedgerKey::EffectsCompactionCursor,
                7 => FinalizationLedgerKey::Genesis,
                8 => FinalizationLedgerKey::Witness(BlockHashSerde(hash(1))),
                9 => FinalizationLedgerKey::SettledRecoveryCharge(BlockHashSerde(hash(1))),
                _ => FinalizationLedgerKey::SettledRecoveryUsage(BlockHashSerde(hash(1))),
            };
            if let Ok(value) = bounded_decode::decode(&key, &encoded) {
                prop_assert_eq!(&value, &bincode::deserialize::<FinalizationLedgerValue>(&encoded).unwrap());
                prop_assert_eq!(bincode::serialize(&value).unwrap(), encoded);
            }
        }
    }

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
        let carrier = if expected.revision == 0 {
            hash(0)
        } else {
            expected.block_hash.0.clone()
        };
        let mut supporting = BTreeSet::from([BlockHashSerde(directly_finalized.clone())]);
        if expected.revision > 0 {
            supporting.insert(BlockHashSerde(carrier.clone()));
        }
        let latest_messages = BTreeMap::from([(
            ValidatorSerde(Bytes::from(vec![1; models::rust::validator::LENGTH])),
            BlockHashSerde(directly_finalized.clone()),
        )]);
        let witness = FinalizationLedger::prepare_witness(
            6,
            "root".to_string(),
            genesis.block_hash.0,
            expected,
            directly_finalized.clone(),
            expected.certificate_digest.0.clone(),
            carrier,
            block_number,
            hash(200),
            1,
            1,
            latest_messages,
            supporting,
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

    fn recovery_episode(revision: u64, byte: u8) -> RecoveryEpisodeId {
        RecoveryEpisodeId {
            schema_version: RECOVERY_STATE_SCHEMA_VERSION,
            revision,
            shard_id: "root".to_string(),
            protocol_version: 6,
            floor_hash: BlockHashSerde(hash(byte)),
            floor_post_state_hash: BlockHashSerde(hash(byte.wrapping_add(1))),
            certificate_digest: BlockHashSerde(if revision == 0 {
                hash(0)
            } else {
                hash(byte.wrapping_add(2))
            }),
        }
    }

    fn recovery_charge(
        episode: &RecoveryEpisodeId,
        target: u8,
        citer: u8,
        generation: i64,
    ) -> SettledRecoveryCharge {
        SettledRecoveryCharge {
            schema_version: RECOVERY_STATE_SCHEMA_VERSION,
            episode: episode.clone(),
            target_block_hash: BlockHashSerde(hash(target)),
            citer_validator: ValidatorSerde(Bytes::from(vec![
                citer;
                models::rust::validator::LENGTH
            ])),
            citer_bond_generation: generation,
        }
    }

    fn commit_charge_plan(plan: SettledRecoveryChargePlan) -> Result<(), KvStoreError> {
        let mutations = plan
            .mutations
            .iter()
            .map(|(key, operation)| AtomicStoreMutation {
                store: plan.store.as_ref(),
                key: key.clone(),
                operation: operation.clone(),
            })
            .collect::<Vec<_>>();
        strict_atomic_mutate(&mutations)
    }

    #[test]
    fn settled_recovery_migration_is_atomic_and_idempotent() {
        let ledger = ledger();
        let episode = recovery_episode(1, 1);
        let first = recovery_charge(&episode, 10, 20, 0);
        let second = recovery_charge(&episode, 11, 20, 0);

        assert_eq!(
            ledger
                .commit_settled_recovery_migration(&episode, &[first.clone(), second.clone()], 0,)
                .unwrap(),
            2
        );
        assert_eq!(
            ledger
                .commit_settled_recovery_migration(&episode, &[first.clone(), second.clone()], 2,)
                .unwrap(),
            2
        );

        let state = ledger.settled_recovery_state().unwrap();
        assert_eq!(state.usage.get(&episode.digest()), Some(&2));
        assert_eq!(state.charges.len(), 2);
        assert!(state.charges.contains(&first));
        assert!(state.charges.contains(&second));
    }

    #[test]
    fn empty_legacy_migration_is_zero_usage_without_storage_mutation() {
        let ledger = ledger();
        let episode = recovery_episode(1, 1);
        let before = ledger.store.raw_store().to_map().unwrap();

        for _ in 0..2 {
            assert_eq!(
                ledger
                    .commit_settled_recovery_migration(&episode, &[], 0)
                    .unwrap(),
                0
            );
            assert_eq!(ledger.settled_recovery_usage(&episode).unwrap(), 0);
            assert_eq!(ledger.store.raw_store().to_map().unwrap(), before);
            let state = ledger.settled_recovery_state().unwrap();
            assert!(state.charges.is_empty());
            assert!(state.usage.is_empty());
        }
    }

    #[test]
    fn settled_recovery_charge_identity_includes_citer_and_generation() {
        let ledger = ledger();
        let episode = recovery_episode(1, 1);
        let charges = [
            recovery_charge(&episode, 10, 20, 0),
            recovery_charge(&episode, 10, 21, 0),
            recovery_charge(&episode, 10, 20, 1),
        ];

        assert_eq!(
            ledger
                .commit_settled_recovery_migration(&episode, &charges, 0)
                .unwrap(),
            3
        );
        assert_eq!(ledger.settled_recovery_state().unwrap().charges.len(), 3);
    }

    #[test]
    fn settled_recovery_migration_fails_on_usage_or_charge_conflict() {
        let ledger = ledger();
        let episode = recovery_episode(1, 1);
        let charge = recovery_charge(&episode, 10, 20, 0);
        ledger
            .store
            .put_one(
                FinalizationLedgerKey::SettledRecoveryUsage(episode.digest()),
                FinalizationLedgerValue::SettledRecoveryUsage(1),
            )
            .unwrap();
        assert!(matches!(
            ledger.commit_settled_recovery_migration(&episode, &[charge.clone()], 0),
            Err(KvStoreError::TransactionConflict(_))
        ));

        let conflicting = recovery_charge(&episode, 10, 21, 0);
        ledger
            .store
            .put_one(
                FinalizationLedgerKey::SettledRecoveryCharge(
                    FinalizationLedger::settled_recovery_charge_digest(&charge),
                ),
                FinalizationLedgerValue::SettledRecoveryCharge(conflicting),
            )
            .unwrap();
        assert!(matches!(
            ledger.commit_settled_recovery_migration(&episode, &[charge], 1),
            Err(KvStoreError::InvalidArgument(_))
        ));
    }

    #[test]
    fn settled_recovery_capacity_is_per_episode() {
        let ledger = ledger();
        let first_episode = recovery_episode(1, 1);
        for target in [10, 11] {
            let plan = ledger
                .prepare_settled_recovery_charge(&recovery_charge(&first_episode, target, 20, 0), 2)
                .unwrap();
            commit_charge_plan(plan).unwrap();
        }
        assert!(matches!(
            ledger.prepare_settled_recovery_charge(&recovery_charge(&first_episode, 12, 20, 0), 2,),
            Err(KvStoreError::RecoveryBudgetExhausted { capacity: 2, .. })
        ));

        let next_episode = recovery_episode(2, 2);
        let plan = ledger
            .prepare_settled_recovery_charge(&recovery_charge(&next_episode, 12, 20, 0), 2)
            .unwrap();
        commit_charge_plan(plan).unwrap();
        assert_eq!(ledger.settled_recovery_usage(&first_episode).unwrap(), 2);
        assert_eq!(ledger.settled_recovery_usage(&next_episode).unwrap(), 1);
    }

    #[test]
    fn legacy_migration_over_capacity_blocks_new_work_until_the_next_episode() {
        let ledger = ledger();
        let first_episode = recovery_episode(1, 1);
        let legacy = (0..=SETTLED_RECOVERY_EPISODE_CAPACITY)
            .map(|generation| recovery_charge(&first_episode, 10, 20, generation as i64))
            .collect::<Vec<_>>();
        assert_eq!(
            ledger
                .commit_settled_recovery_migration(&first_episode, &legacy, 0)
                .unwrap(),
            SETTLED_RECOVERY_EPISODE_CAPACITY + 1
        );
        assert!(matches!(
            ledger.prepare_settled_recovery_charge(
                &recovery_charge(
                    &first_episode,
                    11,
                    20,
                    SETTLED_RECOVERY_EPISODE_CAPACITY as i64 + 1,
                ),
                SETTLED_RECOVERY_EPISODE_CAPACITY,
            ),
            Err(KvStoreError::RecoveryBudgetExhausted { .. })
        ));

        let next_episode = recovery_episode(2, 2);
        let plan = ledger
            .prepare_settled_recovery_charge(
                &recovery_charge(&next_episode, 11, 20, 0),
                SETTLED_RECOVERY_EPISODE_CAPACITY,
            )
            .unwrap();
        commit_charge_plan(plan).unwrap();
        assert_eq!(ledger.settled_recovery_usage(&next_episode).unwrap(), 1);
    }

    proptest! {
        #[test]
        fn arbitrary_recovery_charge_sequences_preserve_exact_usage(
            episode_tag in 1u8..200,
            raw in prop::collection::vec((any::<u8>(), any::<u8>(), 0i64..8), 0..128),
        ) {
            let ledger = ledger();
            let episode = recovery_episode(1, episode_tag);
            let capacity = 16u64;
            let mut accepted = HashSet::new();

            for (target, citer, generation) in raw {
                let charge = recovery_charge(&episode, target, citer, generation);
                if accepted.contains(&charge) {
                    let plan = ledger
                        .prepare_settled_recovery_charge(&charge, capacity)
                        .unwrap();
                    prop_assert!(plan.mutations.is_empty());
                    prop_assert_eq!(plan.committed, accepted.len() as u64);
                } else if accepted.len() < capacity as usize {
                    let plan = ledger
                        .prepare_settled_recovery_charge(&charge, capacity)
                        .unwrap();
                    prop_assert_eq!(plan.committed, accepted.len() as u64 + 1);
                    commit_charge_plan(plan).unwrap();
                    accepted.insert(charge);
                } else {
                    prop_assert!(matches!(
                        ledger.prepare_settled_recovery_charge(&charge, capacity),
                        Err(KvStoreError::RecoveryBudgetExhausted { capacity: 16, .. })
                    ), "a new charge beyond capacity must fail");
                }
            }

            let state = ledger.settled_recovery_state().unwrap();
            prop_assert_eq!(
                state.usage.get(&episode.digest()).copied().unwrap_or(0),
                accepted.len() as u64,
            );
            prop_assert_eq!(state.charges.into_iter().collect::<HashSet<_>>(), accepted);
        }

        #[test]
        fn arbitrary_legacy_migration_is_exact_and_idempotent(
            episode_tag in 1u8..200,
            raw in prop::collection::vec((any::<u8>(), any::<u8>(), 0i64..8), 0..128),
        ) {
            let ledger = ledger();
            let episode = recovery_episode(1, episode_tag);
            let missing = raw
                .into_iter()
                .map(|(target, citer, generation)| {
                    recovery_charge(&episode, target, citer, generation)
                })
                .collect::<HashSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let expected = missing.len() as u64;

            prop_assert_eq!(
                ledger
                    .commit_settled_recovery_migration(&episode, &missing, 0)
                    .unwrap(),
                expected,
            );
            prop_assert_eq!(
                ledger
                    .commit_settled_recovery_migration(&episode, &missing, expected)
                    .unwrap(),
                expected,
            );
            let state = ledger.settled_recovery_state().unwrap();
            prop_assert_eq!(ledger.settled_recovery_usage(&episode).unwrap(), expected);
            if missing.is_empty() {
                prop_assert!(state.usage.is_empty());
            } else {
                prop_assert_eq!(state.usage.get(&episode.digest()), Some(&expected));
            }
            prop_assert_eq!(state.charges.len(), missing.len());
        }
    }

    #[test]
    fn witness_requires_authority_context_and_finalized_target() {
        let ledger = ledger();
        let genesis = initialize(&ledger, hash(0), 0);
        let target = BlockHashSerde(hash(1));
        assert!(FinalizationLedger::prepare_witness(
            6,
            "root".to_string(),
            hash(0),
            &genesis,
            target.0.clone(),
            hash(0),
            hash(0),
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
            6,
            "root".to_string(),
            hash(0),
            &genesis,
            target.0,
            hash(0),
            hash(0),
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
        assert!(FinalizationLedger::prepare_witness(
            6,
            "root".to_string(),
            hash(0),
            &genesis,
            hash(1),
            hash(0),
            hash(0),
            1,
            hash(2),
            1,
            10,
            BTreeMap::new(),
            BTreeSet::from([BlockHashSerde(hash(1))]),
            BlockHashSerde(hash(3)),
            BTreeSet::from([BlockHashSerde(hash(1))]),
        )
        .is_err());
    }

    #[test]
    fn non_genesis_predecessor_carrier_must_be_in_the_support_manifest() {
        let ledger = ledger();
        let genesis = initialize(&ledger, hash(0), 0);
        let first = prepare_record(
            &ledger,
            &genesis,
            hash(1),
            1,
            1.0,
            BTreeSet::from([BlockHashSerde(hash(1))]),
        )
        .unwrap();
        let head = match ledger.try_append(&genesis, &first).unwrap() {
            FinalizationAppendOutcome::Committed(head) => head,
            outcome => panic!("unexpected append outcome: {outcome:?}"),
        };
        let target = BlockHashSerde(hash(2));
        assert!(FinalizationLedger::prepare_witness(
            6,
            "root".to_string(),
            hash(0),
            &head,
            target.0.clone(),
            head.certificate_digest.0.clone(),
            head.block_hash.0.clone(),
            2,
            hash(3),
            1,
            10,
            BTreeMap::from([(
                ValidatorSerde(Bytes::from(vec![1; models::rust::validator::LENGTH])),
                target.clone(),
            )]),
            BTreeSet::from([target.clone()]),
            BlockHashSerde(hash(4)),
            BTreeSet::from([target]),
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
                    certificate_digest: BlockHashSerde(hash(3)),
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
    fn startup_audit_rejects_effects_ahead_of_projection() {
        let ledger = ledger_with_committed_rounds(1);
        ledger
            .store
            .put_one(
                FinalizationLedgerKey::EffectsCursor,
                FinalizationLedgerValue::EffectsCursor(1),
            )
            .unwrap();
        assert!(ledger.validate_integrity().is_err());
        assert!(ledger.ensure_genesis(hash(0), 0).is_err());
    }

    #[test]
    fn integrity_scan_releases_its_lock_and_keeps_its_captured_head() {
        let ledger = ledger_with_committed_rounds(3);
        let mut scan = ledger.begin_integrity_scan().unwrap();
        let captured = scan.captured_head().unwrap().clone();
        assert!(!scan.validate_next_page(NonZeroUsize::MIN).unwrap());
        assert_eq!(scan.validated_revision(), 1);
        assert!(ledger.append_lock.try_lock().is_some());

        let writer = ledger.clone();
        std::thread::spawn(move || {
            let head = writer.head().unwrap().unwrap();
            let record = prepare_record(
                &writer,
                &head,
                hash(4),
                4,
                0.75,
                BTreeSet::from([BlockHashSerde(hash(4))]),
            )
            .unwrap();
            assert!(matches!(
                writer.try_append(&head, &record).unwrap(),
                FinalizationAppendOutcome::Committed(_)
            ));
        })
        .join()
        .unwrap();

        assert!(!scan.validate_next_page(NonZeroUsize::MIN).unwrap());
        assert!(scan.validate_next_page(NonZeroUsize::MIN).unwrap());
        assert!(scan.is_complete());
        assert_eq!(scan.validated_revision(), 3);
        assert_eq!(scan.captured_head(), Some(&captured));
        assert_eq!(ledger.head().unwrap().unwrap().revision, 4);
        ledger.validate_integrity().unwrap();
    }

    #[test]
    fn integrity_scan_failure_is_terminal_and_does_not_publish_partial_page_progress() {
        let ledger = ledger_with_committed_rounds(3);
        let record = ledger.record(2).unwrap().unwrap();
        let witness = ledger.witness(&record.witness_digest).unwrap().unwrap();
        let mut scan = ledger.begin_integrity_scan().unwrap();
        ledger
            .store
            .delete(vec![FinalizationLedgerKey::Witness(
                record.witness_digest.clone(),
            )])
            .unwrap();
        let before = ledger.store.raw_store().to_map().unwrap();
        let first_error = scan
            .validate_next_page(NonZeroUsize::new(2).unwrap())
            .unwrap_err();
        assert_eq!(scan.validated_revision(), 0);
        assert!(!scan.is_complete());
        assert_eq!(ledger.store.raw_store().to_map().unwrap(), before);

        ledger
            .store
            .put_one(
                FinalizationLedgerKey::Witness(record.witness_digest),
                FinalizationLedgerValue::Witness(witness),
            )
            .unwrap();
        assert_eq!(scan.validate_next_page(NonZeroUsize::MIN), Err(first_error));
        assert_eq!(scan.validated_revision(), 0);
        assert!(!scan.is_complete());
        ledger.validate_integrity().unwrap();
    }

    #[test]
    fn restarted_integrity_scan_rechecks_the_previous_validated_prefix() {
        let ledger = ledger_with_committed_rounds(4);
        let mut scan = ledger.begin_integrity_scan().unwrap();
        assert!(!scan
            .validate_next_page(NonZeroUsize::new(2).unwrap())
            .unwrap());
        assert_eq!(scan.validated_revision(), 2);
        drop(scan);

        let record = ledger.record(2).unwrap().unwrap();
        ledger
            .store
            .delete(vec![FinalizationLedgerKey::Witness(record.witness_digest)])
            .unwrap();
        let mut restarted = ledger.begin_integrity_scan().unwrap();
        assert_eq!(restarted.validated_revision(), 0);
        assert!(!restarted.validate_next_page(NonZeroUsize::MIN).unwrap());
        assert!(restarted.validate_next_page(NonZeroUsize::MIN).is_err());
        assert!(!restarted.is_complete());
    }

    #[test]
    fn integrity_scan_rejects_genesis_head_and_endpoint_changes() {
        for mutation in 0..4 {
            let ledger = ledger_with_committed_rounds(3);
            let mut scan = ledger.begin_integrity_scan().unwrap();
            match mutation {
                0 => {
                    let mut genesis = ledger.genesis().unwrap().unwrap();
                    genesis.block_hash = BlockHashSerde(hash(9));
                    ledger
                        .store
                        .put_one(
                            FinalizationLedgerKey::Genesis,
                            FinalizationLedgerValue::Genesis(genesis),
                        )
                        .unwrap();
                }
                1 => {
                    let record = ledger.record(2).unwrap().unwrap();
                    ledger
                        .store
                        .put_one(
                            FinalizationLedgerKey::Head,
                            FinalizationLedgerValue::Head(FinalizationLedger::record_head(&record)),
                        )
                        .unwrap();
                }
                2 => {
                    let mut head = ledger.head().unwrap().unwrap();
                    head.block_hash = BlockHashSerde(hash(9));
                    ledger
                        .store
                        .put_one(
                            FinalizationLedgerKey::Head,
                            FinalizationLedgerValue::Head(head),
                        )
                        .unwrap();
                }
                _ => {
                    ledger
                        .store
                        .delete(vec![FinalizationLedgerKey::Round(3)])
                        .unwrap();
                }
            }
            let before = ledger.store.raw_store().to_map().unwrap();
            assert!(scan.validate_next_page(NonZeroUsize::MIN).is_err());
            assert_eq!(scan.validated_revision(), 0);
            assert!(!scan.is_complete());
            assert_eq!(ledger.store.raw_store().to_map().unwrap(), before);
        }
    }

    #[tokio::test]
    async fn async_integrity_audit_preserves_complete_validation_and_errors() {
        let empty = ledger();
        empty.validate_integrity_async().await.unwrap();
        let valid = ledger_with_committed_rounds(35);
        valid.validate_integrity_async().await.unwrap();
        let record = valid.record(33).unwrap().unwrap();
        valid
            .store
            .delete(vec![FinalizationLedgerKey::Witness(record.witness_digest)])
            .unwrap();
        assert!(valid.validate_integrity_async().await.is_err());
    }

    #[test]
    fn integrity_pages_bound_storage_reads_without_whole_store_scans() {
        let original = ledger_with_committed_rounds(8);
        let reads = Arc::new(Mutex::new(Vec::new()));
        let observer_reads = reads.clone();
        let observed = FinalizationLedger::from_store(Arc::new(ObservedAuditStore {
            inner: original.store.raw_store().clone(),
            on_read: Arc::new(move |keys| {
                observer_reads.lock().extend(keys.iter().cloned());
                Ok(())
            }),
            on_delete: None,
            on_put: None,
            _lifetime: Arc::new(AuditStoreLifetime(None)),
        }));
        let mut scan = observed.begin_integrity_scan().unwrap();
        while !scan.is_complete() {
            reads.lock().clear();
            let before = scan.validated_revision();
            scan.validate_next_page(NonZeroUsize::new(3).unwrap())
                .unwrap();
            let after = scan.validated_revision();
            let read_keys = reads
                .lock()
                .iter()
                .map(|key| observed.store.decode_key(key).unwrap())
                .collect::<Vec<_>>();
            assert_eq!(read_keys.len(), 6 + 2 * (after - before) as usize);
            let round_reads = read_keys
                .iter()
                .filter_map(|key| match key {
                    FinalizationLedgerKey::Round(revision) => Some(*revision),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let expected = std::iter::once(8)
                .chain((before + 1)..=after)
                .collect::<Vec<_>>();
            assert_eq!(round_reads, expected);
            assert!(after - before <= 3);
        }
    }

    #[tokio::test]
    async fn cancelling_async_integrity_audit_releases_the_active_page_without_scheduling_another()
    {
        let original = ledger_with_committed_rounds(65);
        let first = original.record(1).unwrap().unwrap();
        let pause_key = original
            .store
            .encode_key(&FinalizationLedgerKey::Witness(first.witness_digest))
            .unwrap();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let entered = Mutex::new(Some(entered_tx));
        let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
        let resume = Mutex::new(resume_rx);
        let (released_tx, released_rx) = tokio::sync::oneshot::channel();
        let read_keys = Arc::new(Mutex::new(Vec::new()));
        let observed_keys = read_keys.clone();
        let observed = FinalizationLedger::from_store(Arc::new(ObservedAuditStore {
            inner: original.store.raw_store().clone(),
            on_read: Arc::new(move |keys| {
                observed_keys.lock().extend(keys.iter().cloned());
                if keys.contains(&pause_key) {
                    if let Some(sender) = entered.lock().take() {
                        let _ = sender.send(());
                        resume
                            .lock()
                            .recv_timeout(Duration::from_secs(10))
                            .map_err(|error| KvStoreError::IoError(error.to_string()))?;
                    }
                }
                Ok(())
            }),
            on_delete: None,
            on_put: None,
            _lifetime: Arc::new(AuditStoreLifetime(Some(released_tx))),
        }));
        let worker = tokio::spawn(async move { observed.validate_integrity_async().await });
        tokio::time::timeout(Duration::from_secs(10), entered_rx)
            .await
            .unwrap()
            .unwrap();
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
        resume_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), released_rx)
            .await
            .unwrap()
            .unwrap();

        let witness_reads = read_keys
            .lock()
            .iter()
            .map(|key| original.store.decode_key(key).unwrap())
            .filter(|key| matches!(key, FinalizationLedgerKey::Witness(_)))
            .count();
        assert_eq!(
            witness_reads,
            FinalizationLedger::INTEGRITY_PAGE_RECORDS.get()
        );
        original.validate_integrity_async().await.unwrap();
    }

    #[test]
    fn pending_effects_exclude_a_round_appended_after_projection_capture() {
        let ledger = ledger_with_committed_rounds(0);
        let projection_captured = Arc::new(Barrier::new(2));
        let round_appended = Arc::new(Barrier::new(2));
        let reader = {
            let ledger = ledger.clone();
            let projection_captured = projection_captured.clone();
            let round_appended = round_appended.clone();
            std::thread::spawn(move || {
                assert!(ledger.pending_projection_records().unwrap().is_empty());
                projection_captured.wait();
                round_appended.wait();
                ledger.pending_effect_records().unwrap()
            })
        };
        projection_captured.wait();
        let head = ledger.head().unwrap().unwrap();
        let record = prepare_record(
            &ledger,
            &head,
            hash(1),
            1,
            0.75,
            BTreeSet::from([BlockHashSerde(hash(1))]),
        )
        .unwrap();
        ledger.try_append(&head, &record).unwrap();
        round_appended.wait();
        assert!(reader.join().unwrap().is_empty());
        ledger.record_projection_completed(1).unwrap();
        assert_eq!(ledger.pending_effect_records().unwrap(), vec![record]);
    }

    #[test]
    fn projection_scan_keeps_its_target_after_a_later_append() {
        let ledger = ledger_with_committed_rounds(1);
        let mut scan = ledger.pending_projection_scan().unwrap();
        let head = ledger.head().unwrap().unwrap();
        let second = prepare_record(
            &ledger,
            &head,
            hash(2),
            2,
            0.75,
            BTreeSet::from([BlockHashSerde(hash(2))]),
        )
        .unwrap();
        ledger.try_append(&head, &second).unwrap();

        assert_eq!(scan.through_revision(), 1);
        assert_eq!(scan.next_record().unwrap().unwrap().revision, 1);
        assert_eq!(scan.next_record().unwrap(), None);
        assert_eq!(scan.next_record().unwrap(), None);
        assert_eq!(
            ledger.pending_projection_scan().unwrap().through_revision(),
            2
        );
    }

    #[test]
    fn effect_scan_keeps_its_projected_target_as_projection_advances() {
        let ledger = ledger_with_committed_rounds(2);
        ledger.record_projection_completed(1).unwrap();
        let mut scan = ledger.pending_effect_scan().unwrap();
        ledger.record_projection_completed(2).unwrap();

        assert_eq!(scan.through_revision(), 1);
        assert_eq!(scan.next_record().unwrap().unwrap().revision, 1);
        assert_eq!(scan.next_record().unwrap(), None);
        assert_eq!(ledger.pending_effect_scan().unwrap().through_revision(), 2);
        ledger.require_effect_projection(1).unwrap();
        ledger.require_effect_projection(2).unwrap();
    }

    #[test]
    fn record_scan_does_not_advance_after_missing_or_malformed_records() {
        let ledger = ledger_with_committed_rounds(2);
        let first = ledger.record(1).unwrap().unwrap();
        let mut scan = ledger.pending_projection_scan().unwrap();
        ledger
            .store
            .delete(vec![FinalizationLedgerKey::Round(1)])
            .unwrap();
        assert!(scan.next_record().is_err());
        ledger
            .store
            .put_one(
                FinalizationLedgerKey::Round(1),
                FinalizationLedgerValue::EffectsComplete,
            )
            .unwrap();
        assert!(scan.next_record().is_err());
        ledger
            .store
            .put_one(
                FinalizationLedgerKey::Round(1),
                FinalizationLedgerValue::Round(first.clone()),
            )
            .unwrap();

        assert_eq!(scan.next_record().unwrap(), Some(first));
        assert_eq!(scan.next_record().unwrap().unwrap().revision, 2);
        assert_eq!(scan.next_record().unwrap(), None);
    }

    #[test]
    fn early_effect_receipt_cannot_mutate_the_ledger() {
        let ledger = ledger_with_committed_rounds(1);
        let id = FinalizationEffectId {
            revision: 1,
            block_hash: BlockHashSerde(hash(1)),
            kind: FinalizationEffectKind::DeployRemoval,
        };
        let before = ledger.store.raw_store().to_map().unwrap();
        assert_eq!(
            ledger.record_effect(id.clone()),
            Err(KvStoreError::FinalizationProjectionPending {
                revision: 1,
                projected_revision: 0,
            }),
        );
        assert_eq!(ledger.store.raw_store().to_map().unwrap(), before);
        ledger.record_projection_completed(1).unwrap();
        ledger.record_effect(id.clone()).unwrap();
        assert!(ledger.effect_completed(&id).unwrap());
    }

    #[test]
    fn early_round_completion_cannot_mutate_the_ledger() {
        let ledger = ledger_with_committed_rounds(1);
        for kind in FinalizationEffectKind::ALL {
            ledger
                .store
                .put_one(
                    FinalizationLedgerKey::Effect(FinalizationEffectId {
                        revision: 1,
                        block_hash: BlockHashSerde(hash(1)),
                        kind,
                    }),
                    FinalizationLedgerValue::Effect,
                )
                .unwrap();
        }
        let before = ledger.store.raw_store().to_map().unwrap();
        assert_eq!(
            ledger.record_round_effects_completed(1),
            Err(KvStoreError::FinalizationProjectionPending {
                revision: 1,
                projected_revision: 0,
            }),
        );
        assert_eq!(ledger.store.raw_store().to_map().unwrap(), before);
        ledger.record_projection_completed(1).unwrap();
        assert_eq!(ledger.record_round_effects_completed(1).unwrap(), 1);
        assert!(ledger.validate_integrity().is_ok());
    }

    proptest! {
        #[test]
        fn integrity_page_partitions_preserve_full_validation(
            rounds in 0u8..9,
            page_limits in proptest::collection::vec(1usize..6, 1..6),
            missing_witness in proptest::option::of(any::<u8>()),
        ) {
            let ledger = ledger_with_committed_rounds(rounds);
            let missing_revision = missing_witness.filter(|_| rounds > 0)
                .map(|seed| 1 + u64::from(seed % rounds));
            if let Some(revision) = missing_revision {
                let record = ledger.record(revision).unwrap().unwrap();
                ledger.store.delete(vec![FinalizationLedgerKey::Witness(record.witness_digest)]).unwrap();
            }
            let mut scan = ledger.begin_integrity_scan().unwrap();
            let mut page = 0;
            let result = loop {
                let before = scan.validated_revision();
                let limit = page_limits[page % page_limits.len()];
                let result = scan.validate_next_page(NonZeroUsize::new(limit).unwrap());
                prop_assert!(scan.validated_revision() >= before);
                prop_assert!(scan.validated_revision() - before <= limit as u64);
                prop_assert!(scan.validated_revision() <= u64::from(rounds));
                page += 1;
                match result {
                    Ok(false) => {
                        prop_assert!(scan.validated_revision() > before);
                        prop_assert!(page <= usize::from(rounds));
                    }
                    result => break result,
                }
            };
            prop_assert_eq!(result.is_ok(), missing_revision.is_none());
            prop_assert_eq!(scan.is_complete(), missing_revision.is_none());
            prop_assert_eq!(ledger.validate_integrity().is_ok(), missing_revision.is_none());
            if result.is_ok() {
                prop_assert_eq!(scan.validated_revision(), u64::from(rounds));
                let head = ledger.head().unwrap();
                prop_assert_eq!(scan.captured_head(), head.as_ref());
            }
        }

        #[test]
        fn effect_readiness_matches_the_projected_prefix(
            rounds in 1u8..8,
            projection_seed in any::<u8>(),
        ) {
            let projected = projection_seed % (rounds + 1);
            let ledger = ledger_with_committed_rounds(rounds);
            for revision in 1..=u64::from(projected) {
                ledger.record_projection_completed(revision).unwrap();
            }
            let before = ledger.store.raw_store().to_map().unwrap();
            for revision in 0..=u64::from(rounds) + 1 {
                let readiness = ledger.require_effect_projection(revision);
                if revision == 0 || revision > u64::from(rounds) {
                    prop_assert!(matches!(readiness, Err(KvStoreError::InvalidArgument(_))));
                } else if revision > u64::from(projected) {
                    prop_assert_eq!(readiness, Err(KvStoreError::FinalizationProjectionPending {
                        revision,
                        projected_revision: u64::from(projected),
                    }));
                } else {
                    prop_assert_eq!(readiness, Ok(()));
                }
            }
            prop_assert_eq!(ledger.store.raw_store().to_map().unwrap(), before);
            let mut scan = ledger.pending_effect_scan().unwrap();
            prop_assert_eq!(scan.through_revision(), u64::from(projected));
            for revision in 1..=u64::from(projected) {
                prop_assert_eq!(scan.next_record().unwrap().unwrap().revision, revision);
            }
            prop_assert_eq!(scan.next_record().unwrap(), None);
        }

        #[test]
        fn startup_audit_enforces_the_complete_cursor_order(
            rounds in 0u8..8,
            projection in 0u64..10,
            effects in 0u64..10,
            compaction in 0u64..10,
        ) {
            let ledger = ledger_with_committed_rounds(rounds);
            ledger.store.put(vec![
                (FinalizationLedgerKey::ProjectionCursor,
                 FinalizationLedgerValue::ProjectionCursor(projection)),
                (FinalizationLedgerKey::EffectsCursor,
                 FinalizationLedgerValue::EffectsCursor(effects)),
                (FinalizationLedgerKey::EffectsCompactionCursor,
                 FinalizationLedgerValue::EffectsCompactionCursor(compaction)),
            ]).unwrap();
            let ordered = compaction <= effects
                && effects <= projection
                && projection <= u64::from(rounds);
            prop_assert_eq!(ledger.validate_integrity().is_ok(), ordered);
            prop_assert_eq!(ledger.ensure_genesis(hash(0), 0).is_ok(), ordered);
        }

        #[test]
        fn startup_audit_checks_every_record_field_at_every_position(rounds in 1u8..8) {
            let ledger = ledger_with_committed_rounds(rounds);
            for revision in 1..=u64::from(rounds) {
                let original = ledger.record(revision).unwrap().unwrap();
                let FinalizationRecord {
                    revision: _, predecessor_hash: _, predecessor_digest: _,
                    directly_finalized: _, block_number: _, fault_tolerance_bits: _,
                    finalized: _, manifest_digest: _, witness_digest: _, record_digest: _,
                } = &original;
                let mutations: [fn(&mut FinalizationRecord); 10] = [
                    |record| record.revision += 1,
                    |record| record.predecessor_hash = BlockHashSerde(hash(254)),
                    |record| record.predecessor_digest = BlockHashSerde(hash(254)),
                    |record| record.directly_finalized = BlockHashSerde(hash(254)),
                    |record| record.block_number += 1,
                    |record| record.fault_tolerance_bits ^= 1,
                    |record| { record.finalized.insert(BlockHashSerde(hash(254))); },
                    |record| record.manifest_digest = BlockHashSerde(hash(254)),
                    |record| record.witness_digest = BlockHashSerde(hash(254)),
                    |record| record.record_digest = BlockHashSerde(hash(254)),
                ];
                for (field, mutate) in mutations.into_iter().enumerate() {
                    let mut altered = original.clone();
                    mutate(&mut altered);
                    ledger.store.put_one(
                        FinalizationLedgerKey::Round(revision),
                        FinalizationLedgerValue::Round(altered),
                    ).unwrap();
                    prop_assert!(ledger.validate_integrity().is_err(),
                        "unchecked round {revision}, field {field}");
                    ledger.store.put_one(
                        FinalizationLedgerKey::Round(revision),
                        FinalizationLedgerValue::Round(original.clone()),
                    ).unwrap();
                    prop_assert!(ledger.validate_integrity().is_ok());
                }
            }
        }

        #[test]
        fn startup_audit_requires_every_record_and_witness(rounds in 1u8..8) {
            let ledger = ledger_with_committed_rounds(rounds);
            for revision in 1..=u64::from(rounds) {
                let record = ledger.record(revision).unwrap().unwrap();
                let witness = ledger.witness(&record.witness_digest).unwrap().unwrap();
                ledger.store.delete(vec![FinalizationLedgerKey::Round(revision)]).unwrap();
                prop_assert!(ledger.validate_integrity().is_err());
                ledger.store.put_one(
                    FinalizationLedgerKey::Round(revision),
                    FinalizationLedgerValue::EffectsCursor(0),
                ).unwrap();
                prop_assert!(ledger.validate_integrity().is_err());
                ledger.store.put_one(
                    FinalizationLedgerKey::Round(revision),
                    FinalizationLedgerValue::Round(record.clone()),
                ).unwrap();
                ledger.store.delete(vec![
                    FinalizationLedgerKey::Witness(record.witness_digest.clone())
                ]).unwrap();
                prop_assert!(ledger.validate_integrity().is_err());
                ledger.store.put_one(
                    FinalizationLedgerKey::Witness(record.witness_digest.clone()),
                    FinalizationLedgerValue::Round(record.clone()),
                ).unwrap();
                prop_assert!(ledger.validate_integrity().is_err());
                ledger.store.put_one(
                    FinalizationLedgerKey::Witness(record.witness_digest),
                    FinalizationLedgerValue::Witness(witness),
                ).unwrap();
                prop_assert!(ledger.validate_integrity().is_ok());
            }
        }

        #[test]
        fn startup_audit_checks_every_witness_field_at_every_position(rounds in 1u8..8) {
            let ledger = ledger_with_committed_rounds(rounds);
            for revision in 1..=u64::from(rounds) {
                let record = ledger.record(revision).unwrap().unwrap();
                let original = ledger.witness(&record.witness_digest).unwrap().unwrap();
                let LocalFinalizationWitness {
                    schema_version: _, protocol_version: _, shard_id: _, genesis_hash: _,
                    predecessor_hash: _, predecessor_digest: _, predecessor_certificate_digest: _,
                    predecessor_certificate_block_hash: _, target_block_hash: _,
                    target_block_number: _, target_post_state_hash: _, fault_tolerance_numerator: _,
                    fault_tolerance_denominator: _, latest_messages: _, supporting_block_hashes: _,
                    authority_context_digest: _, finalized: _, witness_digest: _,
                } = &original;
                let mutations: [fn(&mut LocalFinalizationWitness); 18] = [
                    |witness| witness.schema_version += 1,
                    |witness| witness.protocol_version += 1,
                    |witness| witness.shard_id.push('x'),
                    |witness| witness.genesis_hash = BlockHashSerde(hash(254)),
                    |witness| witness.predecessor_hash = BlockHashSerde(hash(254)),
                    |witness| witness.predecessor_digest = BlockHashSerde(hash(254)),
                    |witness| witness.predecessor_certificate_digest = BlockHashSerde(hash(254)),
                    |witness| witness.predecessor_certificate_block_hash = BlockHashSerde(hash(254)),
                    |witness| witness.target_block_hash = BlockHashSerde(hash(254)),
                    |witness| witness.target_block_number += 1,
                    |witness| witness.target_post_state_hash = BlockHashSerde(hash(254)),
                    |witness| witness.fault_tolerance_numerator += 1,
                    |witness| witness.fault_tolerance_denominator += 1,
                    |witness| {
                        witness.latest_messages.insert(
                            ValidatorSerde(Bytes::from(vec![254; models::rust::validator::LENGTH])),
                            BlockHashSerde(hash(254)),
                        );
                    },
                    |witness| { witness.supporting_block_hashes.insert(BlockHashSerde(hash(254))); },
                    |witness| witness.authority_context_digest = BlockHashSerde(hash(254)),
                    |witness| { witness.finalized.insert(BlockHashSerde(hash(254))); },
                    |witness| witness.witness_digest = BlockHashSerde(hash(254)),
                ];
                for (field, mutate) in mutations.into_iter().enumerate() {
                    let mut altered = original.clone();
                    mutate(&mut altered);
                    ledger.store.put_one(
                        FinalizationLedgerKey::Witness(record.witness_digest.clone()),
                        FinalizationLedgerValue::Witness(altered),
                    ).unwrap();
                    prop_assert!(ledger.validate_integrity().is_err(),
                        "unchecked witness at round {revision}, field {field}");
                    ledger.store.put_one(
                        FinalizationLedgerKey::Witness(record.witness_digest.clone()),
                        FinalizationLedgerValue::Witness(original.clone()),
                    ).unwrap();
                    prop_assert!(ledger.validate_integrity().is_ok());
                }
            }
        }
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
        ledger.record_projection_completed(1).unwrap();
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

    fn ledger_with_uncompacted_effects(rounds: u8) -> FinalizationLedger {
        let ledger = ledger_with_committed_rounds(rounds);
        for revision in 1..=rounds {
            ledger
                .record_projection_completed(u64::from(revision))
                .unwrap();
            receipt_all_effects(&ledger, u64::from(revision), hash(revision));
            ledger
                .store
                .put_one(
                    FinalizationLedgerKey::EffectsComplete(u64::from(revision)),
                    FinalizationLedgerValue::EffectsComplete,
                )
                .unwrap();
        }
        ledger
            .store
            .put_one(
                FinalizationLedgerKey::EffectsCursor,
                FinalizationLedgerValue::EffectsCursor(u64::from(rounds)),
            )
            .unwrap();
        ledger
    }

    #[test]
    fn effect_completed_remains_true_when_compaction_removes_the_observed_receipt() {
        check_completion_during_receipt_compaction(ledger_with_committed_rounds(1));
    }

    #[test]
    fn lmdb_effect_completed_remains_true_during_receipt_compaction() {
        use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;

        let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("target/verification/pr216/ledger-startup");
        std::fs::create_dir_all(&scratch).unwrap();
        let directory = tempfile::Builder::new()
            .prefix("completion-lmdb-")
            .tempdir_in(scratch)
            .unwrap();
        let mut options = heed::EnvOpenOptions::new();
        options.map_size(10 * 1024 * 1024).max_dbs(1);
        let environment = Arc::new(unsafe { options.open(directory.path()).unwrap() });
        let database = {
            let mut transaction = environment.write_txn().unwrap();
            let database = environment
                .create_database(&mut transaction, Some(FinalizationLedger::STORE_NAME))
                .unwrap();
            transaction.commit().unwrap();
            database
        };
        let store = Arc::new(LmdbKeyValueStore::new(environment.clone(), database));
        let fixture = ledger_with_committed_rounds(1);
        store
            .put(
                fixture
                    .store
                    .raw_store()
                    .to_map()
                    .unwrap()
                    .into_iter()
                    .collect(),
            )
            .unwrap();
        check_completion_during_receipt_compaction(FinalizationLedger::from_store(store));
        drop(environment);
        directory.close().unwrap();
    }

    fn check_completion_during_receipt_compaction(original: FinalizationLedger) {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::mpsc;

        original.record_projection_completed(1).unwrap();
        receipt_all_effects(&original, 1, hash(1));
        let id = FinalizationEffectId {
            revision: 1,
            block_hash: BlockHashSerde(hash(1)),
            kind: FinalizationEffectKind::FinalizedEvent,
        };
        let receipt_key = original
            .store
            .encode_key(&FinalizationLedgerKey::Effect(id.clone()))
            .unwrap();
        let receipt_for_observer = receipt_key.clone();
        let paused = Arc::new(AtomicBool::new(false));
        let (at_receipt, reached_receipt) = mpsc::channel();
        let (resume, resumed) = mpsc::channel();
        let resumed = Mutex::new(resumed);
        let observed = FinalizationLedger::from_store(Arc::new(ObservedAuditStore {
            inner: original.store.raw_store().clone(),
            on_read: Arc::new(move |keys| {
                if keys.contains(&receipt_for_observer) && !paused.swap(true, Ordering::SeqCst) {
                    at_receipt.send(()).unwrap();
                    resumed
                        .lock()
                        .recv_timeout(Duration::from_secs(10))
                        .unwrap();
                }
                Ok(())
            }),
            on_delete: None,
            on_put: None,
            _lifetime: Arc::new(AuditStoreLifetime(None)),
        }));
        let reader = observed.clone();
        let query = std::thread::spawn(move || reader.effect_completed(&id));
        let reached = reached_receipt.recv_timeout(Duration::from_secs(10));
        let completed = if reached.is_ok() {
            observed.record_round_effects_completed(1)
        } else {
            Err(KvStoreError::InvalidArgument(
                "query did not reach its receipt read".to_string(),
            ))
        };
        let removed = original.store.raw_store().get(&vec![receipt_key]).unwrap();
        let _ = resume.send(());
        let result = query.join().unwrap();
        reached.unwrap();
        assert_eq!(completed.unwrap(), 1);
        assert_eq!(removed, vec![None]);
        assert!(
            result.unwrap(),
            "compaction must preserve logical effect completion"
        );
    }

    #[test]
    fn effects_cursor_pages_bound_reads_and_keep_the_projected_target() {
        let original = ledger_with_uncompacted_effects(9);
        original
            .store
            .put(vec![
                (
                    FinalizationLedgerKey::EffectsCursor,
                    FinalizationLedgerValue::EffectsCursor(0),
                ),
                (
                    FinalizationLedgerKey::ProjectionCursor,
                    FinalizationLedgerValue::ProjectionCursor(6),
                ),
            ])
            .unwrap();
        let reads = Arc::new(Mutex::new(Vec::new()));
        let observer_reads = reads.clone();
        let observed = FinalizationLedger::from_store(Arc::new(ObservedAuditStore {
            inner: original.store.raw_store().clone(),
            on_read: Arc::new(move |keys| {
                observer_reads.lock().extend(keys.iter().cloned());
                Ok(())
            }),
            on_delete: None,
            on_put: None,
            _lifetime: Arc::new(AuditStoreLifetime(None)),
        }));
        let mut scan = observed.begin_effects_cursor_advance().unwrap();
        assert_eq!(scan.through_revision(), 6);
        reads.lock().clear();
        assert!(!scan
            .advance_next_page(NonZeroUsize::new(2).unwrap())
            .unwrap());
        assert_eq!(scan.cursor(), 2);
        assert!(observed.append_lock.try_lock().is_some());
        let keys = reads
            .lock()
            .iter()
            .map(|key| observed.store.decode_key(key).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(keys.len(), 6);
        assert_eq!(
            keys.iter()
                .filter_map(|key| match key {
                    FinalizationLedgerKey::EffectsComplete(revision) => Some(*revision),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        original
            .store
            .put_one(
                FinalizationLedgerKey::ProjectionCursor,
                FinalizationLedgerValue::ProjectionCursor(9),
            )
            .unwrap();
        while !scan
            .advance_next_page(NonZeroUsize::new(2).unwrap())
            .unwrap()
        {}
        assert_eq!(scan.cursor(), 6);
        assert_eq!(observed.effects_cursor().unwrap(), 6);
        assert_eq!(
            observed
                .begin_effects_cursor_advance()
                .unwrap()
                .finish()
                .unwrap(),
            9
        );
    }

    #[test]
    fn effects_cursor_failure_cannot_publish_partial_page_progress() {
        let original = ledger_with_uncompacted_effects(6);
        original
            .store
            .put_one(
                FinalizationLedgerKey::EffectsCursor,
                FinalizationLedgerValue::EffectsCursor(0),
            )
            .unwrap();
        let once = std::sync::atomic::AtomicBool::new(true);
        let observed = FinalizationLedger::from_store(Arc::new(ObservedAuditStore {
            inner: original.store.raw_store().clone(),
            on_read: Arc::new(|_| Ok(())),
            on_delete: None,
            on_put: Some(Arc::new(move |_| {
                if once.swap(false, std::sync::atomic::Ordering::SeqCst) {
                    return Err(KvStoreError::IoError(
                        "injected cursor write failure".to_string(),
                    ));
                }
                Ok(())
            })),
            _lifetime: Arc::new(AuditStoreLifetime(None)),
        }));
        let before = original.store.raw_store().to_map().unwrap();
        let mut scan = observed.begin_effects_cursor_advance().unwrap();
        assert!(scan
            .advance_next_page(NonZeroUsize::new(3).unwrap())
            .is_err());
        assert_eq!(scan.cursor(), 0);
        assert_eq!(original.store.raw_store().to_map().unwrap(), before);
        assert!(scan
            .advance_next_page(NonZeroUsize::new(3).unwrap())
            .is_err());
        assert_eq!(original.store.raw_store().to_map().unwrap(), before);
        assert_eq!(
            observed
                .begin_effects_cursor_advance()
                .unwrap()
                .finish()
                .unwrap(),
            6
        );
    }

    #[test]
    fn concurrent_effects_cursor_scans_use_current_durable_progress() {
        let ledger = ledger_with_uncompacted_effects(9);
        ledger
            .store
            .put_one(
                FinalizationLedgerKey::EffectsCursor,
                FinalizationLedgerValue::EffectsCursor(0),
            )
            .unwrap();
        let first_page = Arc::new(Barrier::new(2));
        let second_page = Arc::new(Barrier::new(2));
        let mut first = ledger.begin_effects_cursor_advance().unwrap();
        let mut second = ledger.begin_effects_cursor_advance().unwrap();
        let worker_first_page = first_page.clone();
        let worker_second_page = second_page.clone();
        let worker = std::thread::spawn(move || {
            worker_first_page.wait();
            assert!(!second
                .advance_next_page(NonZeroUsize::new(3).unwrap())
                .unwrap());
            assert_eq!(second.cursor(), 5);
            worker_second_page.wait();
        });
        assert!(!first
            .advance_next_page(NonZeroUsize::new(2).unwrap())
            .unwrap());
        first_page.wait();
        second_page.wait();
        assert!(!first
            .advance_next_page(NonZeroUsize::new(2).unwrap())
            .unwrap());
        assert_eq!(first.cursor(), 7);
        worker.join().unwrap();
        assert_eq!(first.finish().unwrap(), 9);
    }

    #[tokio::test]
    async fn cancelled_effects_cursor_worker_commits_only_its_active_page() {
        let original = ledger_with_uncompacted_effects(65);
        original
            .store
            .put_one(
                FinalizationLedgerKey::EffectsCursor,
                FinalizationLedgerValue::EffectsCursor(0),
            )
            .unwrap();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let entered = Mutex::new(Some(entered_tx));
        let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
        let resume = Mutex::new(resume_rx);
        let (released_tx, released_rx) = tokio::sync::oneshot::channel();
        let writes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed_writes = writes.clone();
        let observed = FinalizationLedger::from_store(Arc::new(ObservedAuditStore {
            inner: original.store.raw_store().clone(),
            on_read: Arc::new(|_| Ok(())),
            on_delete: None,
            on_put: Some(Arc::new(move |_| {
                observed_writes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if let Some(sender) = entered.lock().take() {
                    let _ = sender.send(());
                    resume
                        .lock()
                        .recv_timeout(Duration::from_secs(10))
                        .map_err(|error| KvStoreError::IoError(error.to_string()))?;
                }
                Ok(())
            })),
            _lifetime: Arc::new(AuditStoreLifetime(Some(released_tx))),
        }));
        let worker = tokio::spawn(async move { observed.reconcile_effects_cursor_async().await });
        tokio::time::timeout(Duration::from_secs(10), entered_rx)
            .await
            .unwrap()
            .unwrap();
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
        resume_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), released_rx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(original.effects_cursor().unwrap(), 32);
        assert_eq!(original.effects_compaction_cursor().unwrap(), 0);
        assert_eq!(original.reconcile_effects_cursor_async().await.unwrap(), 65);
        original.reconcile_effect_compaction_async().await.unwrap();
        assert_eq!(original.effects_compaction_cursor().unwrap(), 65);
    }

    proptest! {
        #[test]
        fn effects_cursor_page_partitions_preserve_the_exact_contiguous_prefix(
            complete in prop::collection::vec(any::<bool>(), 0..16),
            budget in 1usize..9,
            restart_after in 0usize..18,
        ) {
            let ledger = ledger_with_uncompacted_effects(complete.len() as u8);
            ledger.store.put_one(FinalizationLedgerKey::EffectsCursor, FinalizationLedgerValue::EffectsCursor(0)).unwrap();
            for (index, done) in complete.iter().enumerate() {
                if !done {
                    ledger.store.delete(vec![FinalizationLedgerKey::EffectsComplete(index as u64 + 1)]).unwrap();
                }
            }
            let expected = complete.iter().take_while(|done| **done).count() as u64;
            let mut scan = ledger.begin_effects_cursor_advance().unwrap();
            let mut finished = false;
            for page in 0..=complete.len() {
                let before = scan.cursor();
                finished = scan.advance_next_page(NonZeroUsize::new(budget).unwrap()).unwrap();
                prop_assert!(before <= scan.cursor());
                prop_assert!(scan.cursor() - before <= budget as u64);
                prop_assert!(scan.cursor() <= expected);
                if finished { break; }
                if page == restart_after {
                    drop(scan);
                    scan = ledger.begin_effects_cursor_advance().unwrap();
                }
            }
            prop_assert!(finished);
            prop_assert_eq!(scan.cursor(), expected);
            prop_assert_eq!(ledger.effects_cursor().unwrap(), expected);
        }
    }

    #[test]
    fn receipt_compaction_bounds_deletion_batches_and_preserves_later_rounds() {
        let original = ledger_with_uncompacted_effects(3);
        original
            .store
            .put_one(
                FinalizationLedgerKey::EffectsCursor,
                FinalizationLedgerValue::EffectsCursor(2),
            )
            .unwrap();
        let deletions = Arc::new(Mutex::new(Vec::new()));
        let observer_deletions = deletions.clone();
        let observed = FinalizationLedger::from_store(Arc::new(ObservedAuditStore {
            inner: original.store.raw_store().clone(),
            on_read: Arc::new(|_| Ok(())),
            on_delete: Some(Arc::new(move |keys| {
                observer_deletions.lock().push(keys.to_vec());
                Ok(())
            })),
            on_put: None,
            _lifetime: Arc::new(AuditStoreLifetime(None)),
        }));
        let mut scan = observed.begin_effect_compaction().unwrap();
        assert_eq!(scan.through_revision(), 2);
        assert!(!scan
            .delete_next_page(NonZeroUsize::new(2).unwrap())
            .unwrap());
        assert!(observed.append_lock.try_lock().is_some());
        assert_eq!(observed.effects_compaction_cursor().unwrap(), 0);
        original
            .store
            .put_one(
                FinalizationLedgerKey::EffectsCursor,
                FinalizationLedgerValue::EffectsCursor(3),
            )
            .unwrap();
        while !scan
            .delete_next_page(NonZeroUsize::new(2).unwrap())
            .unwrap()
        {}
        assert_eq!(observed.effects_compaction_cursor().unwrap(), 2);
        let batches = deletions.lock();
        assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 10);
        assert!(batches
            .iter()
            .all(|batch| !batch.is_empty() && batch.len() <= 2));
        for key in batches.iter().flatten() {
            match observed.store.decode_key(key).unwrap() {
                FinalizationLedgerKey::Effect(id) => assert!(id.revision <= 2),
                FinalizationLedgerKey::EffectsComplete(revision) => assert!(revision <= 2),
                key => panic!("unexpected compaction key {key:?}"),
            }
        }
        drop(batches);
        for kind in FinalizationEffectKind::ALL {
            assert_eq!(
                original
                    .store
                    .get_one(&FinalizationLedgerKey::Effect(FinalizationEffectId {
                        revision: 3,
                        block_hash: BlockHashSerde(hash(3)),
                        kind,
                    }))
                    .unwrap(),
                Some(FinalizationLedgerValue::Effect)
            );
        }
        observed.reconcile_effect_compaction().unwrap();
        assert_eq!(observed.effects_compaction_cursor().unwrap(), 3);
    }

    #[test]
    fn receipt_compaction_restarts_after_partial_delete_or_cursor_write_failure() {
        for fail_cursor in [false, true] {
            let original = ledger_with_uncompacted_effects(2);
            let once = Arc::new(std::sync::atomic::AtomicBool::new(true));
            let delete_once = once.clone();
            let write_once = once.clone();
            let raw = original.store.raw_store().clone();
            let observed = FinalizationLedger::from_store(Arc::new(ObservedAuditStore {
                inner: raw.clone(),
                on_read: Arc::new(|_| Ok(())),
                on_delete: Some(Arc::new(move |keys| {
                    if !fail_cursor && delete_once.swap(false, std::sync::atomic::Ordering::SeqCst)
                    {
                        raw.delete(keys.iter().take(1).cloned().collect())?;
                        return Err(KvStoreError::IoError(
                            "injected partial deletion".to_string(),
                        ));
                    }
                    Ok(())
                })),
                on_put: Some(Arc::new(move |_| {
                    if fail_cursor && write_once.swap(false, std::sync::atomic::Ordering::SeqCst) {
                        return Err(KvStoreError::IoError(
                            "injected cursor write failure".to_string(),
                        ));
                    }
                    Ok(())
                })),
                _lifetime: Arc::new(AuditStoreLifetime(None)),
            }));
            let mut scan = observed.begin_effect_compaction().unwrap();
            assert!(scan
                .delete_next_page(NonZeroUsize::new(8).unwrap())
                .is_err());
            assert_eq!(observed.effects_compaction_cursor().unwrap(), 0);
            let after_failure = original.store.raw_store().to_map().unwrap();
            assert!(scan
                .delete_next_page(NonZeroUsize::new(8).unwrap())
                .is_err());
            assert_eq!(original.store.raw_store().to_map().unwrap(), after_failure);
            observed.reconcile_effect_compaction().unwrap();
            assert_eq!(observed.effects_compaction_cursor().unwrap(), 2);
            for revision in 1..=2 {
                for kind in FinalizationEffectKind::ALL {
                    let id = FinalizationEffectId {
                        revision,
                        block_hash: BlockHashSerde(hash(revision as u8)),
                        kind,
                    };
                    assert!(observed.effect_completed(&id).unwrap());
                    assert_eq!(
                        original
                            .store
                            .get_one(&FinalizationLedgerKey::Effect(id))
                            .unwrap(),
                        None
                    );
                }
            }
        }
    }

    #[test]
    fn concurrent_receipt_compactors_preserve_monotonic_completion() {
        let ledger = ledger_with_uncompacted_effects(6);
        let start = Arc::new(Barrier::new(2));
        let workers = [1, 7]
            .into_iter()
            .map(|budget| {
                let ledger = ledger.clone();
                let start = start.clone();
                std::thread::spawn(move || {
                    let mut scan = ledger.begin_effect_compaction().unwrap();
                    start.wait();
                    while !scan
                        .delete_next_page(NonZeroUsize::new(budget).unwrap())
                        .unwrap()
                    {
                        std::thread::yield_now();
                    }
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(ledger.effects_compaction_cursor().unwrap(), 6);
        for revision in 1..=6 {
            assert_eq!(
                ledger
                    .store
                    .get_one(&FinalizationLedgerKey::EffectsComplete(revision))
                    .unwrap(),
                None
            );
        }
    }

    #[tokio::test]
    async fn asynchronous_completion_matches_synchronous_storage() {
        let synchronous = ledger_with_committed_rounds(3);
        let asynchronous = ledger_with_committed_rounds(3);
        for ledger in [&synchronous, &asynchronous] {
            for revision in 1..=3 {
                ledger.record_projection_completed(revision).unwrap();
                receipt_all_effects(ledger, revision, hash(revision as u8));
            }
        }
        for revision in [3, 1, 2, 1] {
            assert_eq!(
                synchronous
                    .record_round_effects_completed(revision)
                    .unwrap(),
                asynchronous
                    .record_round_effects_completed_async(revision)
                    .await
                    .unwrap(),
            );
            assert_eq!(
                synchronous.store.raw_store().to_map().unwrap(),
                asynchronous.store.raw_store().to_map().unwrap()
            );
        }
        let uninitialized = ledger();
        let before = uninitialized.store.raw_store().to_map().unwrap();
        assert!(matches!(
            uninitialized.reconcile_effect_compaction(),
            Err(KvStoreError::SerializationError(_))
        ));
        assert!(matches!(
            uninitialized.reconcile_effect_compaction_async().await,
            Err(KvStoreError::SerializationError(_))
        ));
        assert_eq!(uninitialized.store.raw_store().to_map().unwrap(), before);

        let genesis_only = ledger_with_committed_rounds(0);
        let before = genesis_only.store.raw_store().to_map().unwrap();
        genesis_only
            .reconcile_effect_compaction_async()
            .await
            .unwrap();
        assert_eq!(genesis_only.store.raw_store().to_map().unwrap(), before);
    }

    #[tokio::test]
    async fn cancelled_compaction_finishes_only_its_active_page_and_releases_storage() {
        let original = ledger_with_uncompacted_effects(3);
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let entered = Mutex::new(Some(entered_tx));
        let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
        let resume = Mutex::new(resume_rx);
        let (released_tx, released_rx) = tokio::sync::oneshot::channel();
        let deletes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed_deletes = deletes.clone();
        let observed = FinalizationLedger::from_store(Arc::new(ObservedAuditStore {
            inner: original.store.raw_store().clone(),
            on_read: Arc::new(|_| Ok(())),
            on_delete: Some(Arc::new(move |_| {
                observed_deletes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if let Some(sender) = entered.lock().take() {
                    let _ = sender.send(());
                    resume
                        .lock()
                        .recv_timeout(Duration::from_secs(10))
                        .map_err(|error| KvStoreError::IoError(error.to_string()))?;
                }
                Ok(())
            })),
            on_put: None,
            _lifetime: Arc::new(AuditStoreLifetime(Some(released_tx))),
        }));
        let worker =
            tokio::spawn(async move { observed.reconcile_effect_compaction_async().await });
        tokio::time::timeout(Duration::from_secs(10), entered_rx)
            .await
            .unwrap()
            .unwrap();
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
        resume_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), released_rx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(deletes.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(original.effects_compaction_cursor().unwrap(), 1);
        assert_eq!(original.effects_cursor().unwrap(), 3);
        assert_eq!(
            original
                .store
                .get_one(&FinalizationLedgerKey::EffectsComplete(2))
                .unwrap(),
            Some(FinalizationLedgerValue::EffectsComplete)
        );
        original.reconcile_effect_compaction_async().await.unwrap();
        assert_eq!(original.effects_compaction_cursor().unwrap(), 3);
    }

    proptest! {
        #[test]
        fn receipt_key_iterator_covers_every_block_and_effect_once(
            blocks in prop::collection::btree_set(any::<u8>(), 0..80),
            revision in 1u64..1000,
        ) {
            let expected = blocks.iter().flat_map(|byte| {
                FinalizationEffectKind::ALL.into_iter().map(move |kind| {
                    FinalizationLedgerKey::Effect(FinalizationEffectId {
                        revision, block_hash: BlockHashSerde(hash(*byte)), kind,
                    })
                })
            }).chain(std::iter::once(FinalizationLedgerKey::EffectsComplete(revision)))
                .collect::<Vec<_>>();
            let mut actual = FinalizationReceiptKeys::new(
                revision,
                blocks.into_iter().map(|byte| BlockHashSerde(hash(byte)))
                    .collect::<BTreeSet<_>>().into_iter(),
            );
            prop_assert_eq!(actual.by_ref().collect::<Vec<_>>(), expected);
            prop_assert_eq!(actual.next(), None);
            prop_assert_eq!(actual.next(), None);
        }

        #[test]
        fn receipt_page_partitions_and_restart_preserve_completed_effects(
            rounds in 0u8..7,
            budget in 1usize..18,
            crash_page in 0usize..40,
        ) {
            let ledger = ledger_with_uncompacted_effects(rounds);
            let before = ledger.store.raw_store().to_map().unwrap();
            let mut scan = ledger.begin_effect_compaction().unwrap();
            let mut previous = 0;
            let mut complete = false;
            for page in 0..100 {
                complete = scan.delete_next_page(NonZeroUsize::new(budget).unwrap()).unwrap();
                let cursor = ledger.effects_compaction_cursor().unwrap();
                prop_assert!(previous <= cursor && cursor <= u64::from(rounds));
                for revision in 1..=u64::from(rounds) {
                    for kind in FinalizationEffectKind::ALL {
                        let id = FinalizationEffectId {
                            revision, block_hash: BlockHashSerde(hash(revision as u8)), kind,
                        };
                        prop_assert!(ledger.effect_completed(&id).unwrap());
                        if revision <= cursor {
                            prop_assert_eq!(ledger.store.get_one(&FinalizationLedgerKey::Effect(id)).unwrap(), None);
                        }
                    }
                }
                previous = cursor;
                if complete { break; }
                if page == crash_page {
                    drop(scan);
                    scan = ledger.begin_effect_compaction().unwrap();
                }
            }
            prop_assert!(complete);
            prop_assert_eq!(previous, u64::from(rounds));
            let unrelated = |map: BTreeMap<Vec<u8>, Vec<u8>>| {
                map.into_iter().filter(|(key, _)| !matches!(
                    ledger.store.decode_key(key).unwrap(),
                    FinalizationLedgerKey::Effect(_)
                        | FinalizationLedgerKey::EffectsComplete(_)
                        | FinalizationLedgerKey::EffectsCompactionCursor
                )).collect::<BTreeMap<_, _>>()
            };
            prop_assert_eq!(unrelated(before), unrelated(ledger.store.raw_store().to_map().unwrap()));
        }
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
    fn projection_endpoint_reports_lag_until_the_durable_head_is_projected() {
        let ledger = ledger();
        let genesis = initialize(&ledger, hash(0), 0);
        assert_eq!(
            ledger.projection_endpoint().unwrap(),
            Some(FinalizationProjectionEndpoint {
                head: genesis.clone(),
                projection_revision: 0,
            })
        );

        let record = prepare_record(
            &ledger,
            &genesis,
            hash(1),
            1,
            0.75,
            BTreeSet::from([BlockHashSerde(hash(1))]),
        )
        .unwrap();
        let head = match ledger.try_append(&genesis, &record).unwrap() {
            FinalizationAppendOutcome::Committed(head) => head,
            outcome => panic!("unexpected append outcome: {outcome:?}"),
        };
        assert_eq!(
            ledger.projection_endpoint().unwrap(),
            Some(FinalizationProjectionEndpoint {
                head: head.clone(),
                projection_revision: 0,
            })
        );

        ledger.record_projection_completed(1).unwrap();
        assert_eq!(
            ledger.projection_endpoint().unwrap(),
            Some(FinalizationProjectionEndpoint {
                head,
                projection_revision: 1,
            })
        );
    }

    proptest! {
        #[test]
        fn projection_endpoint_preserves_exact_head_and_projection_prefix(
            (rounds, projected) in
                (1u8..8).prop_flat_map(|rounds| (Just(rounds), 0u8..=rounds)),
        ) {
            let ledger = ledger();
            let mut head = initialize(&ledger, hash(0), 0);
            for round in 1..=rounds {
                let record = prepare_record(
                    &ledger,
                    &head,
                    hash(round),
                    i64::from(round),
                    0.75,
                    BTreeSet::from([BlockHashSerde(hash(round))]),
                )
                .unwrap();
                head = match ledger.try_append(&head, &record).unwrap() {
                    FinalizationAppendOutcome::Committed(head) => head,
                    outcome => panic!("unexpected append outcome: {outcome:?}"),
                };
            }
            for revision in 1..=u64::from(projected) {
                ledger.record_projection_completed(revision).unwrap();
            }

            let endpoint = ledger.projection_endpoint().unwrap().unwrap();
            prop_assert_eq!(endpoint.head, head);
            prop_assert_eq!(endpoint.projection_revision, u64::from(projected));
            prop_assert_eq!(
                ledger.pending_projection_records().unwrap().len(),
                usize::from(rounds - projected),
            );
        }
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
        ledger.record_projection_completed(1).unwrap();
        ledger.record_projection_completed(2).unwrap();

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
                ledger.record_projection_completed(revision as u64).unwrap();
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
