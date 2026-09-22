use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crypto::rust::hash::sha_256::Sha256Hasher;
use models::rust::block_hash::{BlockHash, BlockHashSerde};
use models::rust::block_metadata::BlockMetadata;
use models::rust::validator::Validator;
use parking_lot::RwLock;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use shared::rust::store::key_value_store::KeyValueStore;
use shared::rust::store::key_value_typed_store::KeyValueTypedStore;
use shared::rust::store::key_value_typed_store_impl::KeyValueTypedStoreImpl;
use shared::rust::store::soak_snapshot::{
    decode_length_prefixed, BoundedLmdbReader, ReadLimits, ReadUsage, SnapshotError,
    TransactionIdentity,
};

use super::block_dag_key_value_storage::{BlockDagKeyValueStorage, KeyValueDagRepresentation};
use super::block_metadata_store::BlockMetadataStore;
use super::carrier_index::CarrierIndex;
use super::deploy_lifecycle_types::DeployLifecycleTables;
use crate::rust::key_value_block_store::{BlockDecodeLimits, KeyValueBlockStore};

pub const SNAPSHOT_SCHEMA_VERSION: u32 = 2;
pub const SNAPSHOT_SCOPE: &str = "batch-b1-bounded-detached-capture";
pub const EXCLUDED_STORES: [&str; 5] = [
    "blocks-approved",
    "deploy-lifecycle-events",
    "deploy-lifecycle-terminal",
    "carrier-index",
    "carrier-index-meta",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureLimits {
    pub read: ReadLimits,
    pub block_decode: BlockDecodeLimits,
    pub max_blocks: usize,
    pub max_validators: usize,
    pub max_edges: usize,
    pub max_work: usize,
    pub lock_wait: Duration,
}

impl CaptureLimits {
    pub fn validate(&self) -> Result<(), SnapshotError> {
        self.read.validate()?;
        self.block_decode.validate()?;
        if self.max_blocks == 0 || self.max_validators == 0 || self.max_edges == 0 {
            return Err(SnapshotError::InvalidLimits(
                "block, validator, and edge limits must be positive".to_string(),
            ));
        }
        if self.max_work == 0 {
            return Err(SnapshotError::InvalidLimits(
                "max_work must be positive".to_string(),
            ));
        }
        if self.lock_wait.is_zero() {
            return Err(SnapshotError::InvalidLimits(
                "lock_wait must be positive".to_string(),
            ));
        }
        self.lock_deadline()?;
        Ok(())
    }

    fn lock_deadline(&self) -> Result<Instant, SnapshotError> {
        Instant::now().checked_add(self.lock_wait).ok_or_else(|| {
            SnapshotError::InvalidLimits("lock_wait exceeds the clock range".to_string())
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability<T> {
    Present(T),
    Absent,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlockBody {
    Held(Vec<u8>),
    NotHeld,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DetachedBlock {
    pub metadata: BlockMetadata,
    pub floor: Availability<BlockHash>,
    pub frontier: Availability<BlockHash>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Coverage {
    pub held_blocks: usize,
    pub complete_held_dag: bool,
    pub requested_bodies: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapturePhase {
    GuardsHeld,
    StateCopied,
    RowsRead,
    Validated,
    GuardsReleased,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotData {
    pub schema_version: u32,
    pub scope: &'static str,
    pub limits: CaptureLimits,
    pub coverage: Coverage,
    pub insertion_generation: u64,
    pub transactions: Vec<TransactionIdentity>,
    pub usage: ReadUsage,
    pub work: usize,
    pub dag_set: BTreeSet<BlockHash>,
    pub child_map: BTreeMap<BlockHash, BTreeSet<BlockHash>>,
    pub height_map: BTreeMap<i64, BTreeSet<BlockHash>>,
    pub block_number_map: BTreeMap<BlockHash, i64>,
    pub main_parent_map: BTreeMap<BlockHash, BlockHash>,
    pub self_justification_map: BTreeMap<BlockHash, BlockHash>,
    pub last_finalized_block: Option<(BlockHash, i64)>,
    pub finalized_block_set: BTreeSet<BlockHash>,
    pub latest_messages: BTreeMap<Validator, BlockHash>,
    pub invalid_blocks: BTreeMap<BlockHash, BlockMetadata>,
    pub blocks: BTreeMap<BlockHash, DetachedBlock>,
    pub bodies: BTreeMap<BlockHash, BlockBody>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DetachedDagSnapshot {
    data: SnapshotData,
    canonical: Vec<u8>,
    digest: [u8; 32],
}

impl Deref for DetachedDagSnapshot {
    type Target = SnapshotData;

    fn deref(&self) -> &Self::Target { &self.data }
}

pub struct ScratchView {
    pub representation: KeyValueDagRepresentation,
    pub blocks: KeyValueBlockStore,
    pub excluded_stores: &'static [&'static str],
}

impl ScratchView {
    pub fn observes_durable_work(&self) -> bool { false }
}

pub struct CaptureRequest<'a> {
    pub limits: CaptureLimits,
    pub bodies: &'a [BlockHash],
}

struct WorkMeter {
    used: usize,
    limit: usize,
}

impl WorkMeter {
    fn charge(&mut self, amount: usize) -> Result<(), SnapshotError> {
        let observed = self
            .used
            .checked_add(amount)
            .ok_or(SnapshotError::CounterOverflow("capture work"))?;
        if observed > self.limit {
            return Err(SnapshotError::LimitExceeded {
                kind: "capture work",
                limit: self.limit,
                observed,
            });
        }
        self.used = observed;
        Ok(())
    }
}

fn bounded(kind: &'static str, observed: usize, limit: usize) -> Result<(), SnapshotError> {
    if observed > limit {
        return Err(SnapshotError::LimitExceeded {
            kind,
            limit,
            observed,
        });
    }
    Ok(())
}

fn preflight_metadata(
    bytes: &[u8],
    limits: &CaptureLimits,
    parent_edges: &mut usize,
    work: &mut WorkMeter,
) -> Result<(), SnapshotError> {
    struct Wire<'a>(&'a [u8]);
    impl Wire<'_> {
        fn take(&mut self, n: usize) -> Result<&[u8], SnapshotError> {
            let (head, tail) = self
                .0
                .split_at_checked(n)
                .ok_or_else(|| malformed("The metadata record is truncated."))?;
            self.0 = tail;
            Ok(head)
        }
        fn count(&mut self, kind: &'static str, limit: usize) -> Result<usize, SnapshotError> {
            let n = u64::from_le_bytes(self.take(8)?.try_into().unwrap());
            let n = usize::try_from(n).map_err(|_| SnapshotError::CounterOverflow(kind))?;
            bounded(kind, n, limit)?;
            Ok(n)
        }
        fn bytes(&mut self) -> Result<(), SnapshotError> {
            let n = self.count("metadata bytes", self.0.len())?;
            self.take(n)?;
            Ok(())
        }
    }
    work.charge(bytes.len())?;
    let mut wire = Wire(bytes);
    wire.bytes()?;
    let parents = wire.count("metadata parents", limits.max_edges)?;
    let total = parent_edges
        .checked_add(parents)
        .ok_or(SnapshotError::CounterOverflow("parent edges"))?;
    bounded("parent edges", total, limits.max_edges)?;
    *parent_edges = total;
    for _ in 0..parents {
        wire.bytes()?;
    }
    wire.bytes()?;
    for _ in 0..wire.count("metadata justifications", limits.max_edges)? {
        wire.bytes()?;
        wire.bytes()?;
    }
    for _ in 0..wire.count("metadata validators", limits.max_validators)? {
        wire.bytes()?;
        wire.take(8)?;
    }
    wire.take(19)?;
    wire.bytes()?;
    if !wire.0.is_empty() {
        return Err(malformed("The metadata record has trailing bytes."));
    }
    work.charge(bytes.len())?;
    Ok(())
}

fn prepare_read(
    reader: &mut BoundedLmdbReader,
    work: &WorkMeter,
) -> Result<ReadUsage, SnapshotError> {
    let remaining = work.limit - work.used;
    reader.restrict_remaining(remaining / 4, remaining / 2)?;
    Ok(reader.usage())
}

fn charge_read(
    reader: &BoundedLmdbReader,
    before: ReadUsage,
    work: &mut WorkMeter,
) -> Result<(), SnapshotError> {
    let after = reader.usage();
    work.charge(after.operations - before.operations)?;
    work.charge(after.bytes - before.bytes)?;
    work.charge(after.bytes - before.bytes)
}

fn read_value(
    reader: &mut BoundedLmdbReader,
    store: &Arc<dyn KeyValueStore>,
    key: &Vec<u8>,
    work: &mut WorkMeter,
) -> Result<Option<Vec<u8>>, SnapshotError> {
    work.charge(key.len())?;
    let before = prepare_read(reader, work)?;
    let result = reader.read_value(store, key)?;
    charge_read(reader, before, work)?;
    Ok(result)
}

fn scan(
    reader: &mut BoundedLmdbReader,
    store: &Arc<dyn KeyValueStore>,
    count: usize,
    work: &mut WorkMeter,
) -> Result<BTreeMap<Vec<u8>, Vec<u8>>, SnapshotError> {
    let before = prepare_read(reader, work)?;
    let result = reader.scan_limited(store, count)?;
    charge_read(reader, before, work)?;
    Ok(result)
}

fn read_failed(error: impl std::fmt::Display) -> SnapshotError {
    SnapshotError::ReadFailed(error.to_string())
}

fn malformed(error: impl std::fmt::Display) -> SnapshotError {
    SnapshotError::Malformed(error.to_string())
}

pub fn capture(
    dag: &BlockDagKeyValueStorage,
    blocks: &KeyValueBlockStore,
    request: &CaptureRequest<'_>,
) -> Result<DetachedDagSnapshot, SnapshotError> {
    capture_observed(dag, blocks, request, |_| {})
}

pub fn capture_observed(
    dag: &BlockDagKeyValueStorage,
    blocks: &KeyValueBlockStore,
    request: &CaptureRequest<'_>,
    mut observe: impl FnMut(CapturePhase),
) -> Result<DetachedDagSnapshot, SnapshotError> {
    let limits = &request.limits;
    limits.validate()?;
    let mut work = WorkMeter {
        used: 0,
        limit: limits.max_work,
    };

    bounded("requested bodies", request.bodies.len(), limits.max_blocks)?;
    for hash in request.bodies {
        bounded(
            "request hash bytes",
            hash.len(),
            limits.read.max_value_bytes,
        )?;
        work.charge(1)?;
        work.charge(hash.len())?;
    }
    work.charge(8)?;
    let deadline = limits.lock_deadline()?;
    let access = dag.soak_capture_access(deadline, limits.lock_wait)?;
    observe(CapturePhase::GuardsHeld);
    let generation_before = access.generation();
    let state = access
        .metadata_store()
        .capture_state(deadline, limits.lock_wait)?;
    observe(CapturePhase::StateCopied);

    let held_blocks = state.dag_set.len();
    if held_blocks > limits.max_blocks {
        return Err(SnapshotError::LimitExceeded {
            kind: "held blocks",
            limit: limits.max_blocks,
            observed: held_blocks,
        });
    }
    work.charge(state.height_map.len())?;
    work.charge(state.height_map.len())?;
    let mut charge_hash = |hash: &BlockHash| -> Result<(), SnapshotError> {
        bounded("state hash bytes", hash.len(), limits.read.max_value_bytes)?;
        work.charge(2)?;
        work.charge(hash.len())?;
        work.charge(hash.len())
    };
    for hash in &state.dag_set {
        charge_hash(hash)?;
    }
    let mut child_edges = 0usize;
    for (parent, children) in &state.child_map {
        charge_hash(parent)?;
        child_edges = child_edges
            .checked_add(children.len())
            .ok_or(SnapshotError::CounterOverflow("child edges"))?;
        bounded("child edges", child_edges, limits.max_edges)?;
        for child in children {
            charge_hash(child)?;
        }
    }
    for hashes in state.height_map.values() {
        for hash in hashes {
            charge_hash(hash)?;
        }
    }
    for hash in state.block_number_map.keys() {
        charge_hash(hash)?;
    }
    for (hash, parent) in &state.main_parent_map {
        charge_hash(hash)?;
        charge_hash(parent)?;
    }
    for (hash, justification) in &state.self_justification_map {
        charge_hash(hash)?;
        charge_hash(justification)?;
    }
    for hash in &state.finalized_block_set {
        charge_hash(hash)?;
    }
    if let Some((hash, _)) = &state.last_finalized_block {
        charge_hash(hash)?;
    }
    let metadata_typed = access.metadata_store().capture_typed_store();
    let latest_typed = access.latest_messages_index();
    let invalid_typed = access.invalid_blocks_index();
    let floor_typed = access.floor_index();
    let frontier_typed = access.frontier_index();
    let metadata_raw = metadata_typed.raw_store();
    let latest_raw = latest_typed.raw_store();
    let invalid_raw = invalid_typed.raw_store();
    let floor_raw = floor_typed.raw_store();
    let frontier_raw = frontier_typed.raw_store();
    let blocks_raw = blocks.soak_capture_store();

    let mut reader = BoundedLmdbReader::open(
        &[
            metadata_raw,
            latest_raw,
            invalid_raw,
            floor_raw,
            frontier_raw,
            blocks_raw,
        ],
        limits.read.clone(),
    )?;

    let dag_set: BTreeSet<BlockHash> = state.dag_set.iter().cloned().collect();
    let mut captured_blocks: BTreeMap<BlockHash, DetachedBlock> = BTreeMap::new();
    let mut parent_edges = 0usize;
    for hash in &dag_set {
        work.charge(3)?;
        work.charge(hash.len())?;
        let key = metadata_typed
            .encode_key(&BlockHashSerde(hash.clone()))
            .map_err(malformed)?;
        let metadata_bytes =
            read_value(&mut reader, metadata_raw, &key, &mut work)?.ok_or_else(|| {
                SnapshotError::Incomplete(format!(
                    "held block {} has no metadata row",
                    hex::encode(hash)
                ))
            })?;
        preflight_metadata(&metadata_bytes, limits, &mut parent_edges, &mut work)?;
        let metadata: BlockMetadata = metadata_typed
            .decode_value(&metadata_bytes)
            .map_err(malformed)?;
        if metadata.block_hash != *hash {
            return Err(SnapshotError::Malformed(format!(
                "metadata row for {} names block {}",
                hex::encode(hash),
                hex::encode(&metadata.block_hash)
            )));
        }
        let floor = match read_value(&mut reader, floor_raw, &key, &mut work)? {
            None => Availability::Absent,
            Some(bytes) => {
                work.charge(bytes.len())?;
                let value = BlockHash::from(decode_length_prefixed(&bytes)?);
                Availability::Present(value)
            }
        };
        let frontier = match read_value(&mut reader, frontier_raw, &key, &mut work)? {
            None => Availability::Absent,
            Some(bytes) => {
                work.charge(bytes.len())?;
                let value = BlockHash::from(decode_length_prefixed(&bytes)?);
                Availability::Present(value)
            }
        };
        captured_blocks.insert(hash.clone(), DetachedBlock {
            metadata,
            floor,
            frontier,
        });
    }

    let latest_rows = scan(&mut reader, latest_raw, limits.max_validators, &mut work)?;
    if latest_rows.len() > limits.max_validators {
        return Err(SnapshotError::LimitExceeded {
            kind: "validators",
            limit: limits.max_validators,
            observed: latest_rows.len(),
        });
    }
    let mut latest_messages: BTreeMap<Validator, BlockHash> = BTreeMap::new();
    for (key, value) in latest_rows {
        work.charge(1)?;
        work.charge(key.len())?;
        work.charge(value.len())?;
        let validator = Validator::from(decode_length_prefixed(&key)?);
        let hash = BlockHash::from(decode_length_prefixed(&value)?);
        latest_messages.insert(validator, hash);
    }

    let invalid_rows = scan(&mut reader, invalid_raw, limits.max_blocks, &mut work)?;
    let mut invalid_blocks: BTreeMap<BlockHash, BlockMetadata> = BTreeMap::new();
    let mut invalid_parent_edges = 0usize;
    for (key, value) in invalid_rows {
        work.charge(1)?;
        work.charge(key.len())?;
        let hash = BlockHash::from(decode_length_prefixed(&key)?);
        preflight_metadata(&value, limits, &mut invalid_parent_edges, &mut work)?;
        let metadata: BlockMetadata = invalid_typed.decode_value(&value).map_err(malformed)?;
        if metadata.block_hash != hash {
            return Err(malformed("The invalid metadata key differs."));
        }
        invalid_blocks.insert(hash, metadata);
    }

    let mut bodies: BTreeMap<BlockHash, BlockBody> = BTreeMap::new();
    for hash in request.bodies {
        work.charge(2)?;
        if !dag_set.contains(hash) {
            bodies.insert(hash.clone(), BlockBody::NotHeld);
            continue;
        }
        let key = KeyValueBlockStore::soak_block_key(hash);
        let raw = read_value(&mut reader, blocks_raw, &key, &mut work)?.ok_or_else(|| {
            SnapshotError::Incomplete(format!(
                "held block {} has no block store row",
                hex::encode(hash)
            ))
        })?;
        let mut decode_limits = limits.block_decode.clone();
        decode_limits.max_decompressed_bytes = decode_limits
            .max_decompressed_bytes
            .min((work.limit - work.used) / 2);
        if decode_limits.max_decompressed_bytes == 0 {
            work.charge(work.limit)?;
        }
        let mut encoded = raw.as_slice();
        let decoded_len =
            usize::try_from(prost::encoding::decode_varint(&mut encoded).map_err(malformed)?)
                .map_err(|_| SnapshotError::CounterOverflow("decoded block bytes"))?;
        work.charge(decoded_len)?;
        work.charge(decoded_len)?;
        let block = KeyValueBlockStore::decode_block_bounded(&raw, &decode_limits)?;
        if block.block_hash != *hash {
            return Err(SnapshotError::Malformed(format!(
                "block store row for {} decodes to block {}",
                hex::encode(hash),
                hex::encode(&block.block_hash)
            )));
        }
        bodies.insert(hash.clone(), BlockBody::Held(raw));
    }
    observe(CapturePhase::RowsRead);

    let usage = reader.usage();
    let transactions = reader.validate()?;
    observe(CapturePhase::Validated);
    let generation_after = access.generation();
    if generation_after != generation_before {
        return Err(SnapshotError::EnvironmentChanged {
            environment: "dag insertion generation".to_string(),
            opened: generation_before as usize,
            observed: generation_after as usize,
        });
    }
    drop(access);
    observe(CapturePhase::GuardsReleased);

    let data = SnapshotData {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        scope: SNAPSHOT_SCOPE,
        limits: limits.clone(),
        coverage: Coverage {
            held_blocks,
            complete_held_dag: true,
            requested_bodies: request.bodies.len(),
        },
        insertion_generation: generation_before,
        transactions,
        usage,
        work: work.used,
        dag_set,
        child_map: state
            .child_map
            .iter()
            .map(|(parent, children)| (parent.clone(), children.iter().cloned().collect()))
            .collect(),
        height_map: state
            .height_map
            .iter()
            .map(|(height, hashes)| (*height, hashes.iter().cloned().collect()))
            .collect(),
        block_number_map: state
            .block_number_map
            .iter()
            .map(|(hash, number)| (hash.clone(), *number))
            .collect(),
        main_parent_map: state
            .main_parent_map
            .iter()
            .map(|(hash, parent)| (hash.clone(), parent.clone()))
            .collect(),
        self_justification_map: state
            .self_justification_map
            .iter()
            .map(|(hash, justification)| (hash.clone(), justification.clone()))
            .collect(),
        last_finalized_block: state.last_finalized_block.clone(),
        finalized_block_set: state.finalized_block_set.iter().cloned().collect(),
        latest_messages,
        invalid_blocks,
        blocks: captured_blocks,
        bodies,
    };
    DetachedDagSnapshot::seal(data, work)
}

struct CanonicalEncoder {
    out: Vec<u8>,
    max_bytes: usize,
    work: WorkMeter,
}

impl CanonicalEncoder {
    fn append(&mut self, bytes: &[u8]) -> Result<(), SnapshotError> {
        let total = self
            .out
            .len()
            .checked_add(bytes.len())
            .ok_or(SnapshotError::CounterOverflow("snapshot bytes"))?;
        bounded("snapshot bytes", total, self.max_bytes)?;
        self.work.charge(bytes.len())?;
        self.out.extend_from_slice(bytes);
        Ok(())
    }

    fn tag(&mut self, name: &str) -> Result<(), SnapshotError> { self.bytes(name.as_bytes()) }
    fn bytes(&mut self, value: &[u8]) -> Result<(), SnapshotError> {
        self.u64(value.len() as u64)?;
        self.append(value)
    }
    fn u64(&mut self, value: u64) -> Result<(), SnapshotError> { self.append(&value.to_be_bytes()) }
    fn i64(&mut self, value: i64) -> Result<(), SnapshotError> { self.append(&value.to_be_bytes()) }
    fn u32(&mut self, value: u32) -> Result<(), SnapshotError> { self.append(&value.to_be_bytes()) }
    fn i32(&mut self, value: i32) -> Result<(), SnapshotError> { self.append(&value.to_be_bytes()) }
    fn bool(&mut self, value: bool) -> Result<(), SnapshotError> { self.append(&[u8::from(value)]) }
    fn availability(&mut self, value: &Availability<BlockHash>) -> Result<(), SnapshotError> {
        self.bool(matches!(value, Availability::Present(_)))?;
        if let Availability::Present(hash) = value {
            self.bytes(hash)?;
        }
        Ok(())
    }
    fn metadata(&mut self, metadata: &BlockMetadata) -> Result<(), SnapshotError> {
        self.bytes(&metadata.block_hash)?;
        self.u64(metadata.parents.len() as u64)?;
        for parent in &metadata.parents {
            self.bytes(parent)?;
        }
        self.bytes(&metadata.sender)?;
        self.u64(metadata.justifications.len() as u64)?;
        for justification in &metadata.justifications {
            self.bytes(&justification.validator)?;
            self.bytes(&justification.latest_block_hash)?;
        }
        self.u64(metadata.weight_map.len() as u64)?;
        for (validator, weight) in &metadata.weight_map {
            self.bytes(validator)?;
            self.i64(*weight)?;
        }
        self.i64(metadata.block_number)?;
        self.i32(metadata.sequence_number)?;
        self.bool(metadata.invalid)?;
        self.bool(metadata.directly_finalized)?;
        self.bool(metadata.finalized)?;
        self.u32(metadata.fault_tolerance_value.to_bits())?;
        self.bytes(&metadata.merge_base)
    }
}

impl SnapshotData {
    fn encode(&self, enc: &mut CanonicalEncoder) -> Result<(), SnapshotError> {
        enc.tag("schema")?;
        enc.u32(self.schema_version)?;
        enc.tag(self.scope)?;
        enc.tag("limits")?;
        enc.u64(self.limits.read.max_value_bytes as u64)?;
        enc.u64(self.limits.read.max_total_bytes as u64)?;
        enc.u64(self.limits.read.max_records as u64)?;
        enc.u64(self.limits.read.max_operations as u64)?;
        enc.u64(self.limits.block_decode.max_compressed_bytes as u64)?;
        enc.u64(self.limits.block_decode.max_decompressed_bytes as u64)?;
        enc.u64(self.limits.block_decode.max_expansion_ratio as u64)?;
        enc.u64(self.limits.max_blocks as u64)?;
        enc.u64(self.limits.max_validators as u64)?;
        enc.u64(self.limits.max_edges as u64)?;
        enc.u64(self.limits.max_work as u64)?;
        enc.u64(self.limits.lock_wait.as_secs())?;
        enc.u32(self.limits.lock_wait.subsec_nanos())?;
        enc.tag("read usage")?;
        enc.u64(self.usage.bytes as u64)?;
        enc.u64(self.usage.records as u64)?;
        enc.u64(self.usage.operations as u64)?;
        enc.tag("coverage")?;
        enc.u64(self.coverage.held_blocks as u64)?;
        enc.bool(self.coverage.complete_held_dag)?;
        enc.u64(self.coverage.requested_bodies as u64)?;
        enc.tag("generation")?;
        enc.u64(self.insertion_generation)?;
        enc.tag("transactions")?;
        enc.u64(self.transactions.len() as u64)?;
        for txn in &self.transactions {
            enc.bytes(txn.environment.as_bytes())?;
            enc.u64(txn.last_txn_id_before_open as u64)?;
            enc.u64(txn.txn_id as u64)?;
            match txn.last_txn_id_after_validation {
                None => enc.bool(false)?,
                Some(value) => {
                    enc.bool(true)?;
                    enc.u64(value as u64)?;
                }
            }
        }
        enc.tag("dag_set")?;
        enc.u64(self.dag_set.len() as u64)?;
        for hash in &self.dag_set {
            enc.bytes(hash)?;
        }
        enc.tag("child_map")?;
        enc.u64(self.child_map.len() as u64)?;
        for (parent, children) in &self.child_map {
            enc.bytes(parent)?;
            enc.u64(children.len() as u64)?;
            for child in children {
                enc.bytes(child)?;
            }
        }
        enc.tag("height_map")?;
        enc.u64(self.height_map.len() as u64)?;
        for (height, hashes) in &self.height_map {
            enc.i64(*height)?;
            enc.u64(hashes.len() as u64)?;
            for hash in hashes {
                enc.bytes(hash)?;
            }
        }
        enc.tag("block_number_map")?;
        enc.u64(self.block_number_map.len() as u64)?;
        for (hash, number) in &self.block_number_map {
            enc.bytes(hash)?;
            enc.i64(*number)?;
        }
        enc.tag("main_parent_map")?;
        enc.u64(self.main_parent_map.len() as u64)?;
        for (hash, parent) in &self.main_parent_map {
            enc.bytes(hash)?;
            enc.bytes(parent)?;
        }
        enc.tag("self_justification_map")?;
        enc.u64(self.self_justification_map.len() as u64)?;
        for (hash, justification) in &self.self_justification_map {
            enc.bytes(hash)?;
            enc.bytes(justification)?;
        }
        enc.tag("last_finalized_block")?;
        match &self.last_finalized_block {
            None => enc.bool(false)?,
            Some((hash, height)) => {
                enc.bool(true)?;
                enc.bytes(hash)?;
                enc.i64(*height)?;
            }
        }
        enc.tag("finalized_block_set")?;
        enc.u64(self.finalized_block_set.len() as u64)?;
        for hash in &self.finalized_block_set {
            enc.bytes(hash)?;
        }
        enc.tag("latest_messages")?;
        enc.u64(self.latest_messages.len() as u64)?;
        for (validator, hash) in &self.latest_messages {
            enc.bytes(validator)?;
            enc.bytes(hash)?;
        }
        enc.tag("invalid_blocks")?;
        enc.u64(self.invalid_blocks.len() as u64)?;
        for (hash, metadata) in &self.invalid_blocks {
            enc.bytes(hash)?;
            enc.metadata(metadata)?;
        }
        enc.tag("blocks")?;
        enc.u64(self.blocks.len() as u64)?;
        for (hash, block) in &self.blocks {
            enc.bytes(hash)?;
            enc.metadata(&block.metadata)?;
            enc.availability(&block.floor)?;
            enc.availability(&block.frontier)?;
        }
        enc.tag("bodies")?;
        enc.u64(self.bodies.len() as u64)?;
        for (hash, body) in &self.bodies {
            enc.bytes(hash)?;
            match body {
                BlockBody::NotHeld => enc.bool(false)?,
                BlockBody::Held(bytes) => {
                    enc.bool(true)?;
                    enc.bytes(bytes)?;
                }
            }
        }
        Ok(())
    }
}

impl DetachedDagSnapshot {
    fn seal(mut data: SnapshotData, work: WorkMeter) -> Result<Self, SnapshotError> {
        let mut enc = CanonicalEncoder {
            out: Vec::new(),
            max_bytes: data.limits.read.max_total_bytes,
            work,
        };
        data.encode(&mut enc)?;
        enc.tag("work")?;
        let size = enc
            .out
            .len()
            .checked_add(8)
            .ok_or(SnapshotError::CounterOverflow("snapshot bytes"))?;
        enc.work.charge(size)?;
        enc.work.charge(size)?;
        let total_work = enc
            .work
            .used
            .checked_add(8)
            .ok_or(SnapshotError::CounterOverflow("capture work"))?;
        enc.u64(total_work as u64)?;
        data.work = enc.work.used;
        let digest = Sha256Hasher::hash(enc.out.clone()).try_into().unwrap();
        Ok(Self {
            data,
            canonical: enc.out,
            digest,
        })
    }

    pub fn canonical_bytes(&self) -> &[u8] { &self.canonical }

    pub fn digest(&self) -> [u8; 32] { self.digest }

    pub fn digest_hex(&self) -> String { hex::encode(self.digest()) }

    pub fn scratch_view(&self) -> Result<ScratchView, SnapshotError> {
        let (last_finalized_block_hash, _) =
            self.last_finalized_block.clone().ok_or_else(|| {
                SnapshotError::Incomplete("last finalized block is uninitialized".to_string())
            })?;

        let metadata_kv: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let metadata_typed: KeyValueTypedStoreImpl<BlockHashSerde, BlockMetadata> =
            KeyValueTypedStoreImpl::new(metadata_kv);
        let rows: Vec<(BlockHashSerde, BlockMetadata)> = self
            .blocks
            .values()
            .map(|block| {
                (
                    BlockHashSerde(block.metadata.block_hash.clone()),
                    block.metadata.clone(),
                )
            })
            .collect();
        metadata_typed.put(rows).map_err(read_failed)?;
        let metadata_store = BlockMetadataStore::try_new(metadata_typed).map_err(read_failed)?;
        for hash in &self.dag_set {
            if !metadata_store.contains(hash) {
                return Err(SnapshotError::Incomplete(format!(
                    "scratch metadata store does not hold {}",
                    hex::encode(hash)
                )));
            }
        }

        let floor_kv: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let floor_index: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde> =
            KeyValueTypedStoreImpl::new(floor_kv);
        let frontier_kv: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let frontier_index: KeyValueTypedStoreImpl<BlockHashSerde, BlockHashSerde> =
            KeyValueTypedStoreImpl::new(frontier_kv);
        let mut floor_rows = Vec::new();
        let mut frontier_rows = Vec::new();
        for block in self.blocks.values() {
            if let Availability::Present(value) = &block.floor {
                floor_rows.push((
                    BlockHashSerde(block.metadata.block_hash.clone()),
                    BlockHashSerde(value.clone()),
                ));
            }
            if let Availability::Present(value) = &block.frontier {
                frontier_rows.push((
                    BlockHashSerde(block.metadata.block_hash.clone()),
                    BlockHashSerde(value.clone()),
                ));
            }
        }
        floor_index.put(floor_rows).map_err(read_failed)?;
        frontier_index.put(frontier_rows).map_err(read_failed)?;

        let representation = KeyValueDagRepresentation {
            dag_set: self.dag_set.iter().cloned().collect(),
            latest_messages_map: self
                .latest_messages
                .iter()
                .map(|(validator, hash)| (validator.clone(), hash.clone()))
                .collect(),
            child_map: self
                .child_map
                .iter()
                .map(|(parent, children)| {
                    (
                        parent.clone(),
                        children.iter().cloned().collect::<imbl::HashSet<_>>(),
                    )
                })
                .collect(),
            height_map: self
                .height_map
                .iter()
                .map(|(height, hashes)| {
                    (
                        *height,
                        hashes.iter().cloned().collect::<imbl::HashSet<_>>(),
                    )
                })
                .collect(),
            block_number_map: self
                .block_number_map
                .iter()
                .map(|(hash, number)| (hash.clone(), *number))
                .collect(),
            main_parent_map: self
                .main_parent_map
                .iter()
                .map(|(hash, parent)| (hash.clone(), parent.clone()))
                .collect(),
            self_justification_map: self
                .self_justification_map
                .iter()
                .map(|(hash, justification)| (hash.clone(), justification.clone()))
                .collect(),
            invalid_blocks_set: self.invalid_blocks.values().cloned().collect(),
            last_finalized_block_hash,
            finalized_blocks_set: self.finalized_block_set.iter().cloned().collect(),
            block_metadata_index: Arc::new(RwLock::new(metadata_store)),
            floor_index,
            frontier_index,
            lifecycle: Arc::new(RwLock::new(DeployLifecycleTables::in_memory())),
            carrier_index: Arc::new(RwLock::new(CarrierIndex::in_memory())),
        };

        let block_rows: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        for (hash, body) in &self.bodies {
            if let BlockBody::Held(bytes) = body {
                block_rows
                    .put_one(hash.to_vec(), bytes.clone())
                    .map_err(read_failed)?;
            }
        }
        let blocks = KeyValueBlockStore::new(block_rows, Arc::new(InMemoryKeyValueStore::new()));
        Ok(ScratchView {
            representation,
            blocks,
            excluded_stores: &EXCLUDED_STORES,
        })
    }
}

#[cfg(test)]
mod tests {
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;

    use super::*;

    #[test]
    fn all_capture_guards_use_the_supplied_deadline() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dag = runtime.block_on(async {
            BlockDagKeyValueStorage::new(&mut InMemoryStoreManager::new())
                .await
                .unwrap()
        });
        let deadline = Instant::now();
        let wait = Duration::MAX;
        {
            let _global = dag.global_lock.write();
            assert!(matches!(
                dag.soak_capture_access(deadline, wait),
                Err(SnapshotError::LockTimeout(value)) if value == wait
            ));
        }
        {
            let _metadata = dag.block_metadata_index.write();
            assert!(matches!(
                dag.soak_capture_access(deadline, wait),
                Err(SnapshotError::LockTimeout(value)) if value == wait
            ));
            assert!(dag.global_lock.try_write().is_some());
        }
        {
            let access = dag.soak_capture_access(deadline, wait).unwrap();
            let metadata = access.metadata_store();
            let _state = metadata.dag_state().write();
            assert!(matches!(
                metadata.capture_state(deadline, wait),
                Err(SnapshotError::LockTimeout(value)) if value == wait
            ));
        }
        let access = dag.soak_capture_access(deadline, wait).unwrap();
        assert!(access
            .metadata_store()
            .capture_state(deadline, wait)
            .is_ok());
    }
}
