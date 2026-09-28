use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use block_storage::rust::dag::soak_snapshot::{
    self, BlockBody, CaptureLimits, CaptureRequest, DetachedDagSnapshot,
};
use block_storage::rust::key_value_block_store::BlockDecodeLimits;
use crypto::rust::hash::sha_256::Sha256Hasher;
use models::rust::block_hash::BlockHash;
use serde::{Deserialize, Serialize};
use shared::rust::dag::observation_work::{
    CheckedWork, WorkKind, WorkLimits, WorkMeter, WorkUsage,
};
use shared::rust::store::soak_snapshot::ReadLimits;

use super::reference::Reference;
use super::{
    AuthorityInputs, CaptureEndpoint, Coverage, EventKind, FloorOutcome, ObservationEvent,
    ObserverBinding,
};
use crate::rust::finality::floor::{self, Floor, FloorOfView};
use crate::rust::safety::clique_oracle::{CliqueOracle, ExactOracleResult, FtThreshold};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureOptions {
    pub max_value_bytes: usize,
    pub max_total_bytes: usize,
    pub max_records: usize,
    pub max_operations: usize,
    pub max_compressed_bytes: usize,
    pub max_decompressed_bytes: usize,
    pub max_expansion_ratio: usize,
    pub max_blocks: usize,
    pub max_validators: usize,
    pub max_edges: usize,
    pub max_work: usize,
    pub lock_wait_ms: u64,
}

impl CaptureOptions {
    fn limits(&self, remaining: Duration) -> CaptureLimits {
        CaptureLimits {
            read: ReadLimits {
                max_value_bytes: self.max_value_bytes,
                max_total_bytes: self.max_total_bytes,
                max_records: self.max_records,
                max_operations: self.max_operations,
            },
            block_decode: BlockDecodeLimits {
                max_compressed_bytes: self.max_compressed_bytes,
                max_decompressed_bytes: self.max_decompressed_bytes,
                max_expansion_ratio: self.max_expansion_ratio,
            },
            max_blocks: self.max_blocks,
            max_validators: self.max_validators,
            max_edges: self.max_edges,
            max_work: self.max_work,
            lock_wait: Duration::from_millis(self.lock_wait_ms).min(remaining),
        }
    }

    fn validate(&self) -> Result<(), String> {
        let bounds = [
            (self.max_value_bytes, 8 * 1024 * 1024),
            (self.max_total_bytes, 64 * 1024 * 1024),
            (self.max_records, 65_536),
            (self.max_operations, 2_000_000),
            (self.max_compressed_bytes, 8 * 1024 * 1024),
            (self.max_decompressed_bytes, 8 * 1024 * 1024),
            (self.max_expansion_ratio, 4096),
            (self.max_blocks, 4096),
            (self.max_validators, 64),
            (self.max_edges, 65_536),
            (self.max_work, 2_000_000),
        ];
        if bounds
            .iter()
            .any(|(value, ceiling)| *value == 0 || value > ceiling)
            || !(1..=30_000).contains(&self.lock_wait_ms)
        {
            return Err("invalid_capture_limits".to_string());
        }
        self.limits(Duration::from_secs(30))
            .validate()
            .map_err(|e| e.to_string())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum FloorSelection {
    View,
    Block { hash: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityRequest {
    pub capture: CaptureOptions,
    pub evaluation: WorkLimits,
    pub targets: Vec<String>,
    pub body_hashes: Vec<String>,
    pub floor: Option<FloorSelection>,
    pub original: bool,
    pub reference: bool,
    pub strict: bool,
}

pub fn parse_hash(hash: &str) -> Result<BlockHash, String> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err("invalid_block_hash".to_string());
    }
    hex::decode(hash)
        .map(Into::into)
        .map_err(|_| "invalid_block_hash".to_string())
}

impl AuthorityRequest {
    pub fn validate(&self) -> Result<(), String> {
        self.capture.validate()?;
        self.evaluation.validate().map_err(|e| e.to_string())?;
        if self.targets.len() > 16 || self.body_hashes.len() > self.capture.max_blocks {
            return Err("selection_limit".to_string());
        }
        for hash in self.targets.iter().chain(&self.body_hashes) {
            parse_hash(hash)?;
        }
        if let Some(FloorSelection::Block { hash }) = &self.floor {
            parse_hash(hash)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "availability", rename_all = "snake_case")]
pub enum Value<T> {
    Available {
        input_digest: String,
        value: T,
    },
    Unavailable {
        input_digest: Option<String>,
        reason: String,
    },
    Failed {
        input_digest: Option<String>,
        reason: String,
    },
    NotRequested,
}

impl<T> Value<T> {
    fn available(digest: &str, value: T) -> Self {
        Self::Available {
            input_digest: digest.to_string(),
            value,
        }
    }

    fn unavailable(digest: &str, reason: impl ToString) -> Self {
        Self::Unavailable {
            input_digest: Some(digest.to_string()),
            reason: reason.to_string(),
        }
    }

    fn from_result(digest: &str, result: Result<T, impl ToString>) -> Self {
        match result {
            Ok(value) => Self::available(digest, value),
            Err(error) => Self::unavailable(digest, error),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FloorValue {
    pub outcome: FloorOutcome,
    pub hash: Option<String>,
    pub block_number: Option<i64>,
}

impl FloorValue {
    pub fn empty(outcome: FloorOutcome) -> Self {
        Self {
            outcome,
            hash: None,
            block_number: None,
        }
    }

    pub fn with_hash(outcome: FloorOutcome, hash: &BlockHash, block_number: Option<i64>) -> Self {
        Self {
            outcome,
            hash: Some(hex::encode(hash)),
            block_number,
        }
    }

    fn from_view(value: FloorOfView) -> Self {
        match value {
            FloorOfView::Advance(f) => {
                Self::with_hash(FloorOutcome::Advance, &f.hash, Some(f.block_number))
            }
            FloorOfView::NoAdvance => Self::empty(FloorOutcome::NoAdvance),
            FloorOfView::ContainmentHold { derived } => Self::with_hash(
                FloorOutcome::ContainmentHold,
                &derived.hash,
                Some(derived.block_number),
            ),
            FloorOfView::AbsenceHold { missing } => {
                Self::with_hash(FloorOutcome::AbsenceHold, &missing, None)
            }
            FloorOfView::IncompatibilityHold { .. } => {
                Self::empty(FloorOutcome::IncompatibilityHold)
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct OracleComparison {
    pub algorithm: &'static str,
    pub decision_matches: bool,
    pub original_matches: Option<bool>,
    pub measured_decision: bool,
    pub reference: super::reference::ReferenceOracle,
}

#[derive(Clone, Debug, Serialize)]
pub struct FloorComparison {
    pub algorithm: &'static str,
    pub matches: bool,
    pub measured: FloorValue,
    pub reference: FloorValue,
}

#[derive(Clone, Debug, Serialize)]
pub struct TargetResult {
    pub target: String,
    pub input_scope: &'static str,
    pub comparator: &'static str,
    pub threshold_numerator: i64,
    pub threshold_denominator: i64,
    pub oracle_decision: Value<bool>,
    pub oracle_witness: Value<ExactOracleResult>,
    pub original_fault_tolerance: Value<u32>,
    pub display_projection: Value<u32>,
    pub persisted_fault_tolerance: Value<u32>,
    pub reference_comparison: Value<OracleComparison>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TransactionRecord {
    pub environment: String,
    pub before_open: usize,
    pub opened: usize,
    pub validated: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorkReport {
    pub aggregate: WorkUsage,
    pub preparation: WorkUsage,
    pub measured: WorkUsage,
    pub original: Value<WorkUsage>,
    pub reference: Value<WorkUsage>,
    pub complete: bool,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuthorityResponse {
    pub scope: &'static str,
    pub live_profile_qualified: bool,
    pub coverage: Option<Coverage>,
    pub snapshot_digest: String,
    pub authority_digest: String,
    pub authority: AuthorityInputs,
    pub request: AuthorityRequest,
    pub held_blocks: usize,
    pub requested_bodies: usize,
    pub capture_work: usize,
    pub capture_operations: usize,
    pub capture_records: usize,
    pub capture_bytes: usize,
    pub transactions: Vec<TransactionRecord>,
    pub targets: Vec<TargetResult>,
    pub floor_result: Value<FloorValue>,
    pub floor_comparison: Value<FloorComparison>,
    pub work: WorkReport,
    pub events: Vec<ObservationEvent>,
}

fn digest(value: &impl Serialize, meter: &CheckedWork) -> Result<String, String> {
    let size = bincode::serialized_size(value).map_err(|e| e.to_string())?;
    meter
        .charge(WorkKind::Allocation, 1, size)
        .map_err(|e| e.to_string())?;
    let bytes = bincode::serialize(value).map_err(|e| e.to_string())?;
    Ok(hex::encode(Sha256Hasher::hash(bytes)))
}

fn prepare(snapshot: &DetachedDagSnapshot, meter: &mut CheckedWork) -> Result<(), String> {
    let mut metadata_bytes = 0;
    for block in snapshot.blocks.values() {
        meter.step(WorkKind::Metadata).map_err(|e| e.to_string())?;
        let bytes = bincode::serialized_size(&block.metadata).map_err(|e| e.to_string())?;
        metadata_bytes = metadata_bytes.max(
            bytes
                .checked_mul(16)
                .and_then(|n| n.checked_add(1024))
                .ok_or("metadata_size_overflow")?,
        );
        let mut total = 0i64;
        for weight in block.metadata.weight_map.values() {
            meter.step(WorkKind::Oracle).map_err(|e| e.to_string())?;
            if *weight < 0 {
                return Err("negative_stake".to_string());
            }
            total = total.checked_add(*weight).ok_or("stake_overflow")?;
        }
    }
    let mut body_bytes = 0;
    for body in snapshot.bodies.values() {
        meter.step(WorkKind::Metadata).map_err(|e| e.to_string())?;
        if let BlockBody::Held(bytes) = body {
            let decoded =
                prost::encoding::decode_varint(&mut bytes.as_slice()).map_err(|e| e.to_string())?;
            body_bytes = body_bytes.max(
                decoded
                    .checked_mul(64)
                    .and_then(|n| n.checked_add(bytes.len() as u64))
                    .ok_or("body_size_overflow")?,
            );
        }
    }
    meter
        .set_storage_bounds(metadata_bytes, body_bytes)
        .map_err(|e| e.to_string())
}

fn scratch(
    snapshot: &DetachedDagSnapshot,
    meter: &CheckedWork,
) -> Result<soak_snapshot::ScratchView, String> {
    meter
        .allocate(snapshot.canonical_bytes().len(), 16)
        .map_err(|e| e.to_string())?;
    meter
        .allocate(snapshot.blocks.len(), 2048)
        .map_err(|e| e.to_string())?;
    let view = snapshot.scratch_view().map_err(|e| e.to_string())?;
    meter
        .step(WorkKind::Allocation)
        .map_err(|e| e.to_string())?;
    Ok(view)
}

#[derive(Clone, Debug, Serialize)]
pub struct PartialWork {
    pub aggregate: WorkUsage,
    pub preparation: WorkUsage,
    pub measured: WorkUsage,
    pub original: Option<WorkUsage>,
    pub reference: Option<WorkUsage>,
    pub failure: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuthorityFailure {
    pub reason: String,
    pub work: Option<PartialWork>,
}

impl From<String> for AuthorityFailure {
    fn from(reason: String) -> Self { Self { reason, work: None } }
}

impl std::fmt::Display for AuthorityFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.reason) }
}

fn work_report(meter: &CheckedWork, request: &AuthorityRequest, digest: &str) -> WorkReport {
    let (aggregate, paths, failure) = meter.usage();
    WorkReport {
        aggregate,
        preparation: paths[0].clone(),
        measured: paths[1].clone(),
        original: if request.original {
            Value::available(digest, paths[2].clone())
        } else {
            Value::NotRequested
        },
        reference: if request.reference {
            Value::available(digest, paths[3].clone())
        } else {
            Value::NotRequested
        },
        complete: failure.is_none(),
        failure,
    }
}

pub async fn evaluate(
    endpoint: &CaptureEndpoint,
    binding: &ObserverBinding,
    request: &AuthorityRequest,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
) -> Result<AuthorityResponse, AuthorityFailure> {
    let mut meter = CheckedWork::new(request.evaluation.clone(), deadline, cancelled, 0, 0)
        .map_err(|e| AuthorityFailure::from(e.to_string()))?;
    evaluate_inner(endpoint, binding, request, deadline, &mut meter)
        .await
        .map_err(|reason| {
            let (aggregate, paths, failure) = meter.usage();
            AuthorityFailure {
                reason: reason.clone(),
                work: Some(PartialWork {
                    aggregate,
                    preparation: paths[0].clone(),
                    measured: paths[1].clone(),
                    original: request.original.then(|| paths[2].clone()),
                    reference: request.reference.then(|| paths[3].clone()),
                    failure: failure.unwrap_or(reason),
                }),
            }
        })
}

async fn evaluate_inner(
    endpoint: &CaptureEndpoint,
    binding: &ObserverBinding,
    request: &AuthorityRequest,
    deadline: Instant,
    meter: &mut CheckedWork,
) -> Result<AuthorityResponse, String> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or("deadline")?;
    if !binding.is_active() {
        return Err("instance_changed".to_string());
    }
    let body_hashes = request
        .body_hashes
        .iter()
        .map(|s| parse_hash(s))
        .collect::<Result<Vec<_>, _>>()?;
    let snapshot = soak_snapshot::capture(&endpoint.dag, &endpoint.blocks, &CaptureRequest {
        limits: request.capture.limits(remaining),
        bodies: &body_hashes,
    })
    .map_err(|e| format!("capture_unavailable:{e}"))?;
    if !binding.is_active() {
        return Err("instance_changed".to_string());
    }
    prepare(&snapshot, meter)?;
    let authority_digest = digest(
        &(
            "batch-b2-authority-v1",
            snapshot.digest(),
            &endpoint.authority,
            request,
        ),
        meter,
    )?;
    let snapshot_digest = snapshot.digest_hex();
    let measured_work = meter.for_path(1).map_err(|e| e.to_string())?;
    let original_work = meter.for_path(2).map_err(|e| e.to_string())?;
    let reference_work = meter.for_path(3).map_err(|e| e.to_string())?;
    let target_digests = request
        .targets
        .iter()
        .map(|target| {
            digest(
                &(&authority_digest, "captured_latest_messages", target),
                meter,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let floor_digest = request
        .floor
        .as_ref()
        .map(|selection| digest(&(&authority_digest, "floor", selection), meter))
        .transpose()?;
    let measured = scratch(&snapshot, meter);
    let original = if request.original {
        Some(scratch(&snapshot, meter))
    } else {
        None
    };
    let threshold = FtThreshold::from_ppm(endpoint.authority.fault_tolerance_threshold_ppm);
    if !(-1_000_000..=1_000_000).contains(&threshold.num) {
        return Err("invalid_adopted_threshold".to_string());
    }
    let mut reference = Reference::new(&snapshot, reference_work, threshold.num);
    let operation = binding.operation();
    let mut targets = Vec::new();
    for (target, input_digest) in request.targets.iter().zip(target_digests) {
        let hash = parse_hash(target)?;
        let exact = match &measured {
            Ok(view) => CliqueOracle::exact_result_metered(
                &measured_work,
                &hash,
                &view.representation,
                &snapshot.latest_messages,
                threshold,
                request.strict,
            )
            .await
            .map_err(|e| e.to_string()),
            Err(error) => Err(error.clone()),
        };
        let original_bits = match &original {
            None => Value::NotRequested,
            Some(Err(error)) => Value::unavailable(&input_digest, error),
            Some(Ok(view)) => Value::from_result(
                &input_digest,
                CliqueOracle::ft_witnessed_metered(
                    &original_work,
                    &hash,
                    &view.representation,
                    &snapshot.latest_messages,
                )
                .await
                .map(f32::to_bits),
            ),
        };
        let comparison = if request.reference {
            match (
                &exact,
                reference.oracle(&hash, &snapshot.latest_messages, request.strict),
            ) {
                (Ok(exact), Ok(reference)) => {
                    let original_matches = match &original_bits {
                        Value::Available { value, .. } => Some(*value == reference.original_bits),
                        _ => None,
                    };
                    Value::available(&input_digest, OracleComparison {
                        algorithm: "immutable-subset-reference-v1",
                        decision_matches: exact.decision == reference.decision,
                        original_matches,
                        measured_decision: exact.decision,
                        reference,
                    })
                }
                (Err(error), _) => Value::unavailable(&input_digest, error),
                (_, Err(error)) => Value::unavailable(&input_digest, error),
            }
        } else {
            Value::NotRequested
        };
        let (decision, witness) = match exact {
            Ok(value) => (
                Value::available(&input_digest, value.decision),
                Value::available(&input_digest, value),
            ),
            Err(error) => (
                Value::unavailable(&input_digest, &error),
                Value::unavailable(&input_digest, error),
            ),
        };
        let persisted = match snapshot.blocks.get(&hash) {
            Some(block) if block.metadata.finalized => {
                binding.emit_snapshot(
                    EventKind::PersistedObservation,
                    None,
                    None,
                    Some(&hash),
                    Some(block.metadata.block_number),
                    None,
                    Some(snapshot.digest()),
                );
                Value::available(
                    &snapshot_digest,
                    block.metadata.fault_tolerance_value.to_bits(),
                )
            }
            Some(_) => Value::unavailable(&snapshot_digest, "not_finalized"),
            None => Value::unavailable(&snapshot_digest, "target_not_held"),
        };
        binding.emit_snapshot(
            EventKind::DetachedDerivation,
            operation,
            None,
            Some(&hash),
            None,
            Some(matches!(&decision, Value::Available { .. })),
            Some(snapshot.digest()),
        );
        targets.push(TargetResult {
            target: target.clone(),
            input_scope: "captured_latest_messages",
            comparator: if request.strict { "gt" } else { "ge" },
            threshold_numerator: threshold.num,
            threshold_denominator: threshold.den,
            oracle_decision: decision,
            oracle_witness: witness,
            original_fault_tolerance: original_bits,
            display_projection: Value::unavailable(
                &input_digest,
                "equivocation_snapshot_unavailable",
            ),
            persisted_fault_tolerance: persisted,
            reference_comparison: comparison,
        });
    }
    let (floor_result, floor_comparison) = match &request.floor {
        None => (Value::NotRequested, Value::NotRequested),
        Some(selection) => {
            let input_digest = floor_digest.as_deref().ok_or("floor_digest_unavailable")?;
            let result = match &measured {
                Err(error) => Err(error.clone()),
                Ok(view) => match selection {
                    FloorSelection::View => match &snapshot.last_finalized_block {
                        Some((hash, height)) => floor::floor_of_view_metered(
                            &measured_work,
                            &view.representation,
                            &view.blocks,
                            &Floor {
                                hash: hash.clone(),
                                block_number: *height,
                            },
                            threshold,
                        )
                        .await
                        .map(FloorValue::from_view)
                        .map_err(|e| e.to_string()),
                        None => Err("last_finalized_block_unavailable".to_string()),
                    },
                    FloorSelection::Block { hash } => floor::floor_of_block_metered(
                        &measured_work,
                        &view.representation,
                        &view.blocks,
                        &parse_hash(hash)?,
                        threshold,
                    )
                    .await
                    .map(|f| {
                        FloorValue::with_hash(FloorOutcome::Advance, &f.hash, Some(f.block_number))
                    })
                    .map_err(|e| e.to_string()),
                },
            };
            let comparison = if request.reference {
                let reference = match selection {
                    FloorSelection::View => match &snapshot.last_finalized_block {
                        Some((hash, height)) => reference
                            .view_floor(hash, *height)
                            .map_err(|e| e.to_string()),
                        None => Err("last_finalized_block_unavailable".to_string()),
                    },
                    FloorSelection::Block { hash } => reference
                        .block_floor(&parse_hash(hash)?)
                        .map_err(|e| e.to_string()),
                };
                match (&result, reference) {
                    (Ok(measured), Ok(reference)) => {
                        Value::available(input_digest, FloorComparison {
                            algorithm: "immutable-floor-reference-v1",
                            matches: *measured == reference,
                            measured: measured.clone(),
                            reference,
                        })
                    }
                    (Err(error), _) => Value::unavailable(input_digest, error),
                    (_, Err(error)) => Value::unavailable(input_digest, error),
                }
            } else {
                Value::NotRequested
            };
            (Value::from_result(input_digest, result), comparison)
        }
    };
    let work = work_report(meter, request, &authority_digest);
    Ok(AuthorityResponse {
        scope: "batch-b2-detached-authority-evaluation",
        live_profile_qualified: false,
        coverage: None,
        snapshot_digest,
        authority_digest,
        authority: endpoint.authority.clone(),
        request: request.clone(),
        held_blocks: snapshot.coverage.held_blocks,
        requested_bodies: snapshot.coverage.requested_bodies,
        capture_work: snapshot.work,
        capture_operations: snapshot.usage.operations,
        capture_records: snapshot.usage.records,
        capture_bytes: snapshot.usage.bytes,
        transactions: snapshot
            .transactions
            .iter()
            .map(|t| TransactionRecord {
                environment: t.environment.clone(),
                before_open: t.last_txn_id_before_open,
                opened: t.txn_id,
                validated: t.last_txn_id_after_validation,
            })
            .collect(),
        targets,
        floor_result,
        floor_comparison,
        work,
        events: Vec::new(),
    })
}
