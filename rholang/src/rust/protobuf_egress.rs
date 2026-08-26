//! Atomic protobuf egress framing and explicit Par projection budgeting.
//!
//! Rholang's FFI frames are `u32 little-endian body length || protobuf body`.
//! This module writes that representation into one private, exactly-sized
//! buffer and returns it only after encoding succeeds.  It therefore avoids the
//! former body buffer plus prefix buffer, rejects lengths that the four-byte
//! frame cannot represent, and gives durable callers one complete-or-absent
//! publication boundary.
//!
//! Production FFI publication uses a descriptor-certified bounded traversal,
//! an exact response-local owner census, and direct PathMap emission. Component
//! ranges are retained only inside the private final frame and only for owners
//! proved to recur in that frame. The older persistent-projection budget below
//! remains an explicit measurement surface; it is not on the production path.

use std::collections::HashMap;
use std::fmt;

use models::rhoapi::Par;
use models::rholang_scala_rust_types::ConsumeResultReturn;
use models::rspace_plus_plus_types::{
    SoftCheckpointProto, StoreToMapResult, WaitingContinuationProto,
};
use models::rust::par::{ParProtobufComponentRanges, ParProtobufError};
use models::rust::par_bag::{ParBagOwnerKey, ParProtobufProjectionPreparation};
use models::rust::rholang::protobuf_encoder::try_visit_par_slots;
use models::rust::rholang::protobuf_schema::{
    CertifiedEgressEncodeError, CertifiedEgressError, CertifiedEgressNode, CertifiedEgressPlan,
    CertifiedEgressReplay, ParOwnerCensus, ParOwnerCensusStatistics,
};
use prost::Message;

const FRAME_PREFIX_BYTES: usize = std::mem::size_of::<u32>();

/// Failure to construct one private length-prefixed protobuf frame.
#[derive(Debug)]
pub enum ProtobufEgressError {
    BodyLengthOverflow {
        body_bytes: usize,
    },
    FrameLengthOverflow {
        body_bytes: usize,
    },
    Encode(prost::EncodeError),
    Par(ParProtobufError),
    Certified(CertifiedEgressError),
    AccountingOverflow {
        context: &'static str,
    },
    PolicyAccountingMismatch {
        context: &'static str,
        expected: usize,
        actual: usize,
    },
    EncodedLengthMismatch {
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for ProtobufEgressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BodyLengthOverflow { body_bytes } => write!(
                formatter,
                "protobuf body has {body_bytes} bytes, which exceeds the u32 FFI frame"
            ),
            Self::FrameLengthOverflow { body_bytes } => write!(
                formatter,
                "protobuf body length {body_bytes} overflows the addressable framed buffer"
            ),
            Self::Encode(error) => write!(formatter, "{error}"),
            Self::Par(error) => write!(formatter, "{error}"),
            Self::Certified(error) => write!(formatter, "{error}"),
            Self::AccountingOverflow { context } => {
                write!(formatter, "protobuf egress accounting overflow while counting {context}")
            }
            Self::PolicyAccountingMismatch {
                context,
                expected,
                actual,
            } => write!(
                formatter,
                "protobuf egress {context} mismatch: census expected {expected}, emission observed {actual}"
            ),
            Self::EncodedLengthMismatch { expected, actual } => write!(
                formatter,
                "protobuf encoded_len predicted {expected} body bytes but emission wrote {actual}"
            ),
        }
    }
}

impl std::error::Error for ProtobufEgressError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Encode(error) => Some(error),
            Self::Par(error) => Some(error),
            Self::Certified(error) => Some(error),
            Self::BodyLengthOverflow { .. }
            | Self::FrameLengthOverflow { .. }
            | Self::AccountingOverflow { .. }
            | Self::PolicyAccountingMismatch { .. }
            | Self::EncodedLengthMismatch { .. } => None,
        }
    }
}

impl From<prost::EncodeError> for ProtobufEgressError {
    fn from(error: prost::EncodeError) -> Self { Self::Encode(error) }
}

impl From<ParProtobufError> for ProtobufEgressError {
    fn from(error: ParProtobufError) -> Self { Self::Par(error) }
}

impl From<CertifiedEgressError> for ProtobufEgressError {
    fn from(error: CertifiedEgressError) -> Self { Self::Certified(error) }
}

/// Encode one complete FFI frame in a caller-private buffer.
///
/// No prefix or partial body is externally visible if encoding fails.  The
/// returned buffer has exactly one allocation for representable messages.
pub fn encode_frame(message: &impl Message) -> Result<Vec<u8>, ProtobufEgressError> {
    let body_bytes = message.encoded_len();
    let body_bytes_u32 = u32::try_from(body_bytes)
        .map_err(|_| ProtobufEgressError::BodyLengthOverflow { body_bytes })?;
    let frame_bytes = FRAME_PREFIX_BYTES
        .checked_add(body_bytes)
        .ok_or(ProtobufEgressError::FrameLengthOverflow { body_bytes })?;
    let mut output = Vec::with_capacity(frame_bytes);
    output.extend_from_slice(&body_bytes_u32.to_le_bytes());
    // `Message::encode` performs another root `encoded_len()` preflight.
    // Capacity and representability were already checked against this exact
    // immutable message, so call Prost's raw body emitter and retain the exact
    // postcondition below instead of measuring the complete root twice.
    message.encode_raw(&mut output);
    let actual = output.len() - FRAME_PREFIX_BYTES;
    if actual != body_bytes {
        return Err(ProtobufEgressError::EncodedLengthMismatch {
            expected: body_bytes,
            actual,
        });
    }
    Ok(output)
}

/// Exact response-local accounting for the in-frame component-range memo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameMemoStatistics {
    pub census: ParOwnerCensusStatistics,
    pub census_owner_spilled: bool,
    pub transcript_events: usize,
    pub transcript_spilled: bool,
    pub memo_owner_spilled: bool,
    pub direct_only_occurrences: usize,
    pub range_recorded_owners: usize,
    pub range_copied_occurrences: usize,
    pub direct_only_component_bytes: usize,
    pub range_recorded_component_bytes: usize,
    pub range_copied_component_bytes: usize,
}

fn validate_census_partition(
    census_statistics: ParOwnerCensusStatistics,
) -> Result<(usize, usize), ProtobufEgressError> {
    let eligible_from_owners = census_statistics
        .unique_owners
        .checked_add(census_statistics.repeated_occurrences)
        .ok_or(ProtobufEgressError::AccountingOverflow {
            context: "census eligible occurrence equation",
        })?;
    if eligible_from_owners != census_statistics.eligible_occurrences {
        return Err(ProtobufEgressError::PolicyAccountingMismatch {
            context: "census eligible occurrence equation",
            expected: census_statistics.eligible_occurrences,
            actual: eligible_from_owners,
        });
    }
    let occurrences_from_classes = census_statistics
        .component_empty_occurrences
        .checked_add(census_statistics.eligible_occurrences)
        .ok_or(ProtobufEgressError::AccountingOverflow {
            context: "census Par occurrence equation",
        })?;
    if occurrences_from_classes != census_statistics.par_occurrences {
        return Err(ProtobufEgressError::PolicyAccountingMismatch {
            context: "census Par occurrence equation",
            expected: census_statistics.par_occurrences,
            actual: occurrences_from_classes,
        });
    }
    let direct_occurrences = census_statistics
        .unique_owners
        .checked_sub(census_statistics.repeated_owners)
        .and_then(|count| count.checked_add(census_statistics.component_empty_occurrences))
        .ok_or(ProtobufEgressError::PolicyAccountingMismatch {
            context: "census occurrence partition",
            expected: census_statistics.par_occurrences,
            actual: census_statistics
                .repeated_owners
                .saturating_add(census_statistics.repeated_occurrences),
        })?;
    let direct_component_bytes = census_statistics
        .unique_component_bytes
        .checked_sub(census_statistics.reusable_component_bytes)
        .ok_or(ProtobufEgressError::PolicyAccountingMismatch {
            context: "census component-byte partition",
            expected: census_statistics.unique_component_bytes,
            actual: census_statistics.reusable_component_bytes,
        })?;
    Ok((direct_occurrences, direct_component_bytes))
}

impl FrameMemoStatistics {
    /// Derive the complete action ledger when the exact census proves that no
    /// component owner recurs anywhere in the response.
    ///
    /// Emission still consumes the recorded Par-length transcript and every
    /// direct Par emitter checks its exact byte delta.  Consequently no
    /// response-local owner lookup or action counter is needed in pass two:
    /// the completed census is the stronger witness for the all-direct lane.
    fn for_census_proven_direct_only(census: &ParOwnerCensus) -> Result<Self, ProtobufEgressError> {
        let census_statistics = census.statistics();
        let (direct_only_occurrences, direct_only_component_bytes) =
            validate_census_partition(census_statistics)?;
        let no_reuse_observations = [
            census_statistics.repeated_occurrences,
            census_statistics.repeated_owners,
            census_statistics.reusable_component_bytes,
            census_statistics.repeated_component_bytes,
        ];
        if let Some(actual) = no_reuse_observations.into_iter().find(|value| *value != 0) {
            return Err(ProtobufEgressError::PolicyAccountingMismatch {
                context: "census-proven direct-only policy",
                expected: 0,
                actual,
            });
        }
        if direct_only_occurrences != census_statistics.par_occurrences {
            return Err(ProtobufEgressError::PolicyAccountingMismatch {
                context: "direct-only Par occurrence count",
                expected: census_statistics.par_occurrences,
                actual: direct_only_occurrences,
            });
        }
        if direct_only_component_bytes != census_statistics.unique_component_bytes {
            return Err(ProtobufEgressError::PolicyAccountingMismatch {
                context: "direct-only component byte count",
                expected: census_statistics.unique_component_bytes,
                actual: direct_only_component_bytes,
            });
        }
        Ok(Self {
            census: census_statistics,
            census_owner_spilled: census.has_owner_spill(),
            transcript_events: 0,
            transcript_spilled: false,
            memo_owner_spilled: false,
            direct_only_occurrences,
            range_recorded_owners: 0,
            range_copied_occurrences: 0,
            direct_only_component_bytes,
            range_recorded_component_bytes: 0,
            range_copied_component_bytes: 0,
        })
    }
}

/// Compile-time observation seam for response-local memo policy.
///
/// Correctness never depends on this observer: the sealed transcript, owner
/// census, owner-exact memo, per-`Par` byte deltas, final body delta, and
/// private publication boundary remain mandatory in every monomorphization.
/// The production observer is zero-sized and its methods inline away; the
/// statistics observer is an executable accounting oracle used by tests and
/// experiments.
trait FrameMemoObserver {
    fn direct(&mut self, component_bytes: usize) -> Result<(), ProtobufEgressError>;
    fn recorded(&mut self, component_bytes: usize) -> Result<(), ProtobufEgressError>;
    fn copied(&mut self, component_bytes: usize) -> Result<(), ProtobufEgressError>;
    fn finish(
        &mut self,
        census: &ParOwnerCensus,
        transcript_events: usize,
        transcript_spilled: bool,
        memo_owner_spilled: bool,
    ) -> Result<(), ProtobufEgressError>;
}

#[derive(Default)]
struct NoopFrameMemoObserver;

impl FrameMemoObserver for NoopFrameMemoObserver {
    #[inline(always)]
    fn direct(&mut self, _component_bytes: usize) -> Result<(), ProtobufEgressError> { Ok(()) }

    #[inline(always)]
    fn recorded(&mut self, _component_bytes: usize) -> Result<(), ProtobufEgressError> { Ok(()) }

    #[inline(always)]
    fn copied(&mut self, _component_bytes: usize) -> Result<(), ProtobufEgressError> { Ok(()) }

    #[inline(always)]
    fn finish(
        &mut self,
        _census: &ParOwnerCensus,
        _transcript_events: usize,
        _transcript_spilled: bool,
        _memo_owner_spilled: bool,
    ) -> Result<(), ProtobufEgressError> {
        Ok(())
    }
}

#[derive(Default)]
struct StatisticsFrameMemoObserver {
    statistics: FrameMemoStatistics,
}

impl StatisticsFrameMemoObserver {
    #[inline]
    fn add(
        value: &mut usize,
        amount: usize,
        context: &'static str,
    ) -> Result<(), ProtobufEgressError> {
        *value = value
            .checked_add(amount)
            .ok_or(ProtobufEgressError::AccountingOverflow { context })?;
        Ok(())
    }

    fn into_statistics(self) -> FrameMemoStatistics { self.statistics }
}

impl FrameMemoObserver for StatisticsFrameMemoObserver {
    #[inline]
    fn direct(&mut self, component_bytes: usize) -> Result<(), ProtobufEgressError> {
        Self::add(
            &mut self.statistics.direct_only_occurrences,
            1,
            "direct Par occurrences",
        )?;
        Self::add(
            &mut self.statistics.direct_only_component_bytes,
            component_bytes,
            "direct-only component bytes",
        )
    }

    #[inline]
    fn recorded(&mut self, component_bytes: usize) -> Result<(), ProtobufEgressError> {
        Self::add(
            &mut self.statistics.range_recorded_owners,
            1,
            "range-recorded Par owners",
        )?;
        Self::add(
            &mut self.statistics.range_recorded_component_bytes,
            component_bytes,
            "range-recorded component bytes",
        )
    }

    #[inline]
    fn copied(&mut self, component_bytes: usize) -> Result<(), ProtobufEgressError> {
        Self::add(
            &mut self.statistics.range_copied_occurrences,
            1,
            "range-copied Par occurrences",
        )?;
        Self::add(
            &mut self.statistics.range_copied_component_bytes,
            component_bytes,
            "range-copied component bytes",
        )
    }

    fn finish(
        &mut self,
        census: &ParOwnerCensus,
        transcript_events: usize,
        transcript_spilled: bool,
        memo_owner_spilled: bool,
    ) -> Result<(), ProtobufEgressError> {
        if !census.statistics().has_reuse() {
            self.statistics = FrameMemoStatistics::for_census_proven_direct_only(census)?;
        } else {
            let census_statistics = census.statistics();
            let (direct_occurrences, direct_component_bytes) =
                validate_census_partition(census_statistics)?;
            let expected = [
                (
                    "direct occurrence count",
                    direct_occurrences,
                    self.statistics.direct_only_occurrences,
                ),
                (
                    "recorded owner count",
                    census_statistics.repeated_owners,
                    self.statistics.range_recorded_owners,
                ),
                (
                    "range-copied occurrence count",
                    census_statistics.repeated_occurrences,
                    self.statistics.range_copied_occurrences,
                ),
                (
                    "direct-only component byte count",
                    direct_component_bytes,
                    self.statistics.direct_only_component_bytes,
                ),
                (
                    "range-recorded component byte count",
                    census_statistics.reusable_component_bytes,
                    self.statistics.range_recorded_component_bytes,
                ),
                (
                    "range-copied component byte count",
                    census_statistics.repeated_component_bytes,
                    self.statistics.range_copied_component_bytes,
                ),
            ];
            for (context, expected, actual) in expected {
                if actual != expected {
                    return Err(ProtobufEgressError::PolicyAccountingMismatch {
                        context,
                        expected,
                        actual,
                    });
                }
            }
            self.statistics.census = census_statistics;
            self.statistics.census_owner_spilled = census.has_owner_spill();
        }
        self.statistics.transcript_events = transcript_events;
        self.statistics.transcript_spilled = transcript_spilled;
        self.statistics.memo_owner_spilled = memo_owner_spilled;
        Ok(())
    }
}

/// Component regions already present in one private final frame.
///
/// The first reusable owner is stored inline. A table with census-derived
/// capacity is allocated only when a second reusable owner is first emitted;
/// any number of singleton owners therefore leaves this memo allocation-free.
#[derive(Default)]
struct ResponseFrameMemo {
    first: Option<ParProtobufComponentRanges>,
    additional: Option<HashMap<ParBagOwnerKey, ParProtobufComponentRanges>>,
    additional_capacity: usize,
}

impl ResponseFrameMemo {
    fn new(census: &ParOwnerCensus) -> Self {
        Self {
            first: None,
            additional: None,
            additional_capacity: census.statistics().repeated_owners.saturating_sub(1),
        }
    }

    #[inline(never)]
    fn emit<O: FrameMemoObserver>(
        &mut self,
        census: &ParOwnerCensus,
        value: &Par,
        expected: usize,
        output: &mut Vec<u8>,
        observer: &mut O,
    ) -> Result<(), ProtobufEgressError> {
        let owner = value.protobuf_owner_key();
        let owner_statistics = census.owner_reuse_statistics(&owner);

        // Component-empty values have no owner observation, and owners seen
        // exactly once cannot repay range bookkeeping. Both lanes stream from
        // the authoritative PathMap directly and leave no memo entry behind.
        if value.components_is_empty() {
            if owner_statistics.is_some() {
                return Err(ProtobufEgressError::PolicyAccountingMismatch {
                    context: "component-empty owner census exclusion",
                    expected: 0,
                    actual: 1,
                });
            }
            value.encode_protobuf_direct_to_vec_prevalidated(output, expected)?;
            observer.direct(0)?;
            return Ok(());
        }

        let owner_statistics =
            owner_statistics.ok_or(ProtobufEgressError::PolicyAccountingMismatch {
                context: "eligible owner census membership",
                expected: 1,
                actual: 0,
            })?;
        let component_bytes = owner_statistics.component_bytes;
        if owner_statistics.occurrences == 1 {
            value.encode_protobuf_direct_to_vec_prevalidated(output, expected)?;
            observer.direct(component_bytes)?;
            return Ok(());
        }

        if let Some(ranges) = self
            .first
            .as_ref()
            .filter(|ranges| ranges.owner_key() == &owner)
        {
            value.encode_protobuf_from_component_ranges_prevalidated(output, ranges, expected)?;
            observer.copied(component_bytes)?;
            return Ok(());
        }
        if let Some(ranges) = self
            .additional
            .as_ref()
            .and_then(|additional| additional.get(&owner))
        {
            value.encode_protobuf_from_component_ranges_prevalidated(output, ranges, expected)?;
            observer.copied(component_bytes)?;
            return Ok(());
        }

        let ranges =
            value.encode_protobuf_recording_component_ranges_prevalidated(output, expected)?;
        observer.recorded(component_bytes)?;
        if self.first.is_none() {
            self.first = Some(ranges);
        } else {
            let additional = self
                .additional
                .get_or_insert_with(|| HashMap::with_capacity(self.additional_capacity));
            let previous = additional.insert(owner, ranges);
            if previous.is_some() {
                return Err(ProtobufEgressError::PolicyAccountingMismatch {
                    context: "memoized owner uniqueness",
                    expected: 0,
                    actual: 1,
                });
            }
        }
        Ok(())
    }

    /// Validate the correctness-critical memo shape independently of optional
    /// telemetry. Every census-proven repeated owner must have exactly one
    /// recorded range set when emission completes.
    fn finish(self, census: &ParOwnerCensus) -> Result<bool, ProtobufEgressError> {
        let recorded_owners = usize::from(self.first.is_some())
            .checked_add(self.additional.as_ref().map_or(0, HashMap::len))
            .ok_or(ProtobufEgressError::AccountingOverflow {
                context: "memoized owner cardinality",
            })?;
        let expected = census.statistics().repeated_owners;
        if recorded_owners != expected {
            return Err(ProtobufEgressError::PolicyAccountingMismatch {
                context: "memoized owner cardinality",
                expected,
                actual: recorded_owners,
            });
        }
        Ok(self.additional.is_some())
    }
}

/// Keep the generated bounded traversal out of the frame-planning body. This
/// boundary prevents link-context-dependent whole-graph inlining while still
/// allowing each generated message method to optimize its local schema loop.
#[inline(never)]
fn emit_certified_direct<T: CertifiedEgressNode>(
    message: &T,
    output: &mut Vec<u8>,
    replay: &mut CertifiedEgressReplay<'_>,
) -> Result<(), ProtobufEgressError> {
    match message.try_encode_certified_egress(output, replay, &mut |value, expected, output| {
        value
            .encode_protobuf_direct_to_vec_prevalidated(output, expected)
            .map_err(ProtobufEgressError::from)
    }) {
        Ok(()) => Ok(()),
        Err(CertifiedEgressEncodeError::Length(error)) => Err(error.into()),
        Err(CertifiedEgressEncodeError::Par(error)) => Err(error),
    }
}

#[inline(never)]
fn emit_certified_with_memo<T: CertifiedEgressNode, O: FrameMemoObserver>(
    message: &T,
    output: &mut Vec<u8>,
    replay: &mut CertifiedEgressReplay<'_>,
    census: &ParOwnerCensus,
    memo: &mut ResponseFrameMemo,
    observer: &mut O,
) -> Result<(), ProtobufEgressError> {
    match message.try_encode_certified_egress(output, replay, &mut |value, expected, output| {
        memo.emit(census, value, expected, output, observer)
    }) {
        Ok(()) => Ok(()),
        Err(CertifiedEgressEncodeError::Length(error)) => Err(error.into()),
        Err(CertifiedEgressEncodeError::Par(error)) => Err(error),
    }
}

/// Encode a complete generated protobuf graph with a response-local in-frame
/// component memo.
///
/// The first occurrence of each copy-on-write ParBag owner streams directly
/// from PathMap into the final frame. Later occurrences copy the two component
/// regions already in that frame and emit their own root metadata between
/// them. Nothing is published into a live `ParBagAux`, and all memo state dies
/// with this call.
fn encode_frame_native_core<T: CertifiedEgressNode, O: FrameMemoObserver>(
    message: &T,
    observer: &mut O,
) -> Result<Vec<u8>, ProtobufEgressError> {
    // Pass one is both the exact body preflight and the complete reuse-policy
    // census. It completes before a frame buffer exists, so malformed values
    // cannot publish even a length prefix.
    let plan = CertifiedEgressPlan::for_root(message)?;
    let body_bytes = plan.body_len();
    let body_bytes_u32 = u32::try_from(body_bytes)
        .map_err(|_| ProtobufEgressError::BodyLengthOverflow { body_bytes })?;
    let frame_bytes = FRAME_PREFIX_BYTES
        .checked_add(body_bytes)
        .ok_or(ProtobufEgressError::FrameLengthOverflow { body_bytes })?;
    let mut output = Vec::with_capacity(frame_bytes);
    output.extend_from_slice(&body_bytes_u32.to_le_bytes());
    let census = plan.owner_census();
    let mut replay = plan.replay();

    // A completed no-reuse census admits a strictly smaller second-pass
    // machine: every Par is direct, so owner lookup, component-size lookup,
    // range state, and per-action accounting would all re-prove facts already
    // established by pass one. Exact replay and exact emitted byte deltas remain
    // mandatory, and the private frame is still the sole publication boundary.
    let memo_owner_spilled = if !census.statistics().has_reuse() {
        emit_certified_direct(message, &mut output, &mut replay)?;
        replay.finish()?;
        let actual = output.len() - FRAME_PREFIX_BYTES;
        if actual != body_bytes {
            return Err(ProtobufEgressError::EncodedLengthMismatch {
                expected: body_bytes,
                actual,
            });
        }
        false
    } else {
        let mut memo = ResponseFrameMemo::new(census);
        emit_certified_with_memo(
            message,
            &mut output,
            &mut replay,
            census,
            &mut memo,
            observer,
        )?;
        replay.finish()?;
        let actual = output.len() - FRAME_PREFIX_BYTES;
        if actual != body_bytes {
            return Err(ProtobufEgressError::EncodedLengthMismatch {
                expected: body_bytes,
                actual,
            });
        }
        memo.finish(census)?
    };

    observer.finish(
        census,
        plan.transcript_event_count(),
        plan.has_transcript_spill(),
        memo_owner_spilled,
    )?;
    Ok(output)
}

pub fn encode_frame_native_with_statistics<T: CertifiedEgressNode>(
    message: &T,
) -> Result<(Vec<u8>, FrameMemoStatistics), ProtobufEgressError> {
    let mut observer = StatisticsFrameMemoObserver::default();
    let frame = encode_frame_native_core(message, &mut observer)?;
    Ok((frame, observer.into_statistics()))
}

#[inline]
pub fn encode_frame_native<T: CertifiedEgressNode>(
    message: &T,
) -> Result<Vec<u8>, ProtobufEgressError> {
    encode_frame_native_core(message, &mut NoopFrameMemoObserver)
}

/// Auditable counters for one owner-scoped preparation budget.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectionPreparationStatistics {
    pub attempts: usize,
    pub prepared: usize,
    pub already_prepared: usize,
    pub refused_too_large: usize,
    pub newly_retained_bytes: usize,
}

/// Aggregate new-retention accounting for an owner that has measured reuse.
#[derive(Debug)]
pub struct ProjectionPreparationBudget {
    initial_new_retention_bytes: usize,
    remaining_new_retention_bytes: usize,
    max_single_projection_bytes: usize,
    statistics: ProjectionPreparationStatistics,
}

impl ProjectionPreparationBudget {
    pub const fn new(
        aggregate_new_retention_bytes: usize,
        max_single_projection_bytes: usize,
    ) -> Self {
        Self {
            initial_new_retention_bytes: aggregate_new_retention_bytes,
            remaining_new_retention_bytes: aggregate_new_retention_bytes,
            max_single_projection_bytes,
            statistics: ProjectionPreparationStatistics {
                attempts: 0,
                prepared: 0,
                already_prepared: 0,
                refused_too_large: 0,
                newly_retained_bytes: 0,
            },
        }
    }

    /// Attempt to prepare one selected `Par` under both active caps.
    ///
    /// `AlreadyPrepared` consumes no budget because the projection was retained
    /// by an earlier owner sharing the same `ParBagAux`.  `TooLarge` also leaves
    /// the budget unchanged and the value cold.
    pub fn prepare(
        &mut self,
        value: &Par,
    ) -> Result<ParProtobufProjectionPreparation, ParProtobufError> {
        self.statistics.attempts = self
            .statistics
            .attempts
            .checked_add(1)
            .expect("a preparation attempt count cannot exceed addressable calls");
        let active_cap = self
            .remaining_new_retention_bytes
            .min(self.max_single_projection_bytes);
        let outcome = value.prepare_protobuf_projection(active_cap)?;
        match outcome {
            ParProtobufProjectionPreparation::Prepared { retained_bytes } => {
                self.remaining_new_retention_bytes = self
                    .remaining_new_retention_bytes
                    .checked_sub(retained_bytes)
                    .expect("Par preparation exceeded the cap it accepted");
                self.statistics.prepared = self
                    .statistics
                    .prepared
                    .checked_add(1)
                    .expect("a prepared-value count cannot exceed addressable calls");
                self.statistics.newly_retained_bytes = self
                    .statistics
                    .newly_retained_bytes
                    .checked_add(retained_bytes)
                    .expect("newly retained protobuf bytes exceeded addressable memory");
            }
            ParProtobufProjectionPreparation::AlreadyPrepared { .. } => {
                self.statistics.already_prepared = self
                    .statistics
                    .already_prepared
                    .checked_add(1)
                    .expect("an already-prepared count cannot exceed addressable calls");
            }
            ParProtobufProjectionPreparation::TooLarge { .. } => {
                self.statistics.refused_too_large = self
                    .statistics
                    .refused_too_large
                    .checked_add(1)
                    .expect("a refused-value count cannot exceed addressable calls");
            }
        }
        Ok(outcome)
    }

    pub const fn initial_new_retention_bytes(&self) -> usize { self.initial_new_retention_bytes }

    pub const fn remaining_new_retention_bytes(&self) -> usize {
        self.remaining_new_retention_bytes
    }

    pub const fn max_single_projection_bytes(&self) -> usize { self.max_single_projection_bytes }

    pub const fn statistics(&self) -> ProjectionPreparationStatistics { self.statistics }
}

fn try_visit_waiting_continuation_par_slots<E>(
    waiting: &WaitingContinuationProto,
    visit: &mut impl FnMut(&Par) -> Result<(), E>,
) -> Result<(), E> {
    for pattern in &waiting.patterns {
        try_visit_par_slots(pattern, &mut *visit)?;
    }
    if let Some(continuation) = &waiting.continuation {
        try_visit_par_slots(continuation, &mut *visit)?;
    }
    Ok(())
}

/// Visit every protobuf-visible `Par` slot in a soft checkpoint without
/// decoding PathMap-backed values or constructing a shadow collection.
pub fn try_visit_soft_checkpoint_par_slots<E>(
    checkpoint: &SoftCheckpointProto,
    mut visit: impl FnMut(&Par) -> Result<(), E>,
) -> Result<(), E> {
    let Some(snapshot) = &checkpoint.cache_snapshot else {
        return Ok(());
    };
    for entry in &snapshot.continuations {
        for channel in &entry.key {
            visit(channel)?;
        }
        for waiting in &entry.value {
            try_visit_waiting_continuation_par_slots(waiting, &mut visit)?;
        }
    }
    for entry in &snapshot.installed_continuations {
        for channel in &entry.key {
            visit(channel)?;
        }
        if let Some(waiting) = &entry.value {
            try_visit_waiting_continuation_par_slots(waiting, &mut visit)?;
        }
    }
    for entry in &snapshot.data {
        if let Some(channel) = &entry.key {
            visit(channel)?;
        }
        for datum in &entry.value {
            if let Some(body) = &datum.a {
                try_visit_par_slots(body, &mut visit)?;
            }
        }
    }
    for entry in &snapshot.joins {
        if let Some(channel) = &entry.key {
            visit(channel)?;
        }
        for join in &entry.value {
            for channel in &join.join {
                visit(channel)?;
            }
        }
    }
    for entry in &snapshot.installed_joins {
        if let Some(channel) = &entry.key {
            visit(channel)?;
        }
        for join in &entry.value {
            for channel in &join.join {
                visit(channel)?;
            }
        }
    }
    Ok(())
}

/// Visit every protobuf-visible `Par` slot in a hot-change response.
pub fn try_visit_hot_changes_par_slots<E>(
    changes: &StoreToMapResult,
    mut visit: impl FnMut(&Par) -> Result<(), E>,
) -> Result<(), E> {
    for entry in &changes.map_entries {
        for channel in &entry.key {
            visit(channel)?;
        }
        let Some(value) = &entry.value else {
            continue;
        };
        for datum in &value.data {
            if let Some(body) = &datum.a {
                try_visit_par_slots(body, &mut visit)?;
            }
        }
        for waiting in &value.wks {
            try_visit_waiting_continuation_par_slots(waiting, &mut visit)?;
        }
    }
    Ok(())
}

/// Visit every protobuf-visible `Par` slot in a consume-result response.
pub fn try_visit_consume_result_par_slots<E>(
    result: &ConsumeResultReturn,
    mut visit: impl FnMut(&Par) -> Result<(), E>,
) -> Result<(), E> {
    if let Some(continuation) = &result.tagged_cont {
        try_visit_par_slots(continuation, &mut visit)?;
    }
    for datum in &result.datums {
        try_visit_par_slots(datum, &mut visit)?;
    }
    Ok(())
}
