//! Atomic protobuf egress framing and explicit Par projection budgeting.
//!
//! Rholang's FFI frames are `u32 little-endian body length || protobuf body`.
//! This module writes that representation into one private, exactly-sized
//! buffer and returns it only after encoding succeeds.  It therefore avoids the
//! former body buffer plus prefix buffer, rejects lengths that the four-byte
//! frame cannot represent, and gives durable callers one complete-or-absent
//! publication boundary.
//!
//! Ordinary protobuf encoding remains cache-hit-or-direct.  Projection
//! preparation is an explicit owner decision governed by both a per-value cap
//! and an aggregate *new retention* budget; an already-prepared CoW clone costs
//! the current owner no additional budget.

use std::fmt;

use models::rhoapi::Par;
use models::rholang_scala_rust_types::ConsumeResultReturn;
use models::rspace_plus_plus_types::{
    SoftCheckpointProto, StoreToMapResult, WaitingContinuationProto,
};
use models::rust::par::ParProtobufError;
use models::rust::par_bag::ParProtobufProjectionPreparation;
use models::rust::rholang::protobuf_encoder::try_visit_par_slots;
use prost::Message;

const FRAME_PREFIX_BYTES: usize = std::mem::size_of::<u32>();

/// Failure to construct one private length-prefixed protobuf frame.
#[derive(Debug)]
pub enum ProtobufEgressError {
    BodyLengthOverflow { body_bytes: usize },
    FrameLengthOverflow { body_bytes: usize },
    Encode(prost::EncodeError),
    EncodedLengthMismatch { expected: usize, actual: usize },
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
            Self::Encode(error) => error.fmt(formatter),
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
            Self::BodyLengthOverflow { .. }
            | Self::FrameLengthOverflow { .. }
            | Self::EncodedLengthMismatch { .. } => None,
        }
    }
}

impl From<prost::EncodeError> for ProtobufEgressError {
    fn from(error: prost::EncodeError) -> Self { Self::Encode(error) }
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
    message.encode(&mut output)?;
    let actual = output.len() - FRAME_PREFIX_BYTES;
    if actual != body_bytes {
        return Err(ProtobufEgressError::EncodedLengthMismatch {
            expected: body_bytes,
            actual,
        });
    }
    Ok(output)
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
