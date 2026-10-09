use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeSet;

use models::rhoapi::{BindPattern, CostAuthority, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce, COMM};
use shared::rust::clone_backing::{self, BackingError, BackingMeter, CloneBacking};
use shared::rust::collection_backing::tree_backing;

use super::authority::{self, AuthorityByteEventKind, AuthorityError};
use super::byte_accounting::{self, ByteCharge, IntroductionMeasurement};
use super::byte_receipts::ByteObservation;
#[cfg(test)]
use super::InterpreterError;
use super::IntroductionRecord;

pub(crate) struct MeasuredRSpaceObservation<'a> {
    pub(crate) event_id: [u8; 32],
    pub(crate) kind: AuthorityByteEventKind,
    pub(crate) authority: Cow<'a, CostAuthority>,
    pub(crate) measurement: ByteCharge,
}

/// Added by D-F1 (DR-117): a COMM observation. Its authority is the witness
/// that the merge and the rekey produced, so it is canonical: the merge
/// validated each region once. Neither the native observation nor the
/// producer's reservation canonicalizes it again.
pub(crate) struct MeasuredCommObservation {
    pub(crate) event_id: [u8; 32],
    pub(crate) authority: authority::CanonicalAuthority,
    pub(crate) measurement: ByteCharge,
}

#[cfg(test)]
fn error(error: impl std::fmt::Display) -> InterpreterError {
    InterpreterError::ReduceError(error.to_string())
}

fn construction_error(error: impl std::fmt::Display) -> RSpaceError {
    RSpaceError::InterpreterError(error.to_string())
}

fn inspect_metered(
    meter: &dyn SourceMeter,
    inspect: impl FnOnce(&dyn BackingMeter) -> Result<(), BackingError>,
) -> Result<(), RSpaceError> {
    let failure = RefCell::new(None);
    let backing = |operations, scanned, bytes| {
        meter.reserve(operations, scanned, bytes).map_err(|error| {
            *failure.borrow_mut() = Some(error);
            BackingError::Rejected
        })
    };
    let result = inspect(&backing);
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    result.map_err(|_| RSpaceError::HostWorkRejected)
}

// Disabled by D-O1 (DR-110): the channel inspections use the block forms
// below, and no other caller remains. The scheduler footprint reserves its
// own channel traversals (`locked_footprint`), the digest-keyed cold reads
// read no channel (DR-96), and the produce path reserves the traversals of
// its join keys (`produce_with_authority`).
// // Kept legacy by D-O1 (DR-94) for the channel inspections (the produce
// // channel and the consume channels). The RSpace source preparation reads the
// // same channels again for the scheduler footprint and the cold-read keys
// // without reservations of its own, and the legacy per-level charge of these
// // inspections is part of what pays those reads until Stage B (D-E3).
// fn inspect_value<T: CloneBacking>(value: &T, meter: &dyn SourceMeter) -> Result<(), RSpaceError> {
//     inspect_metered(meter, |backing| clone_backing::inspect(value, backing))
// }
//
// fn inspect_slice<T: CloneBacking>(
//     values: &[T],
//     meter: &dyn SourceMeter,
// ) -> Result<(), RSpaceError> {
//     inspect_metered(meter, |backing| {
//         clone_backing::inspect_slice(values, backing)
//     })
// }

// D-O1 (DR-94): the block-mode inspections of the data, the patterns, the
// continuation and the COMM data. Each caller's inspection prepays exactly
// one linear traversal: the prost length computation of the byte charge, or
// a field read and that length (DR-76). The other reads of these values have
// their own reservations, and their releases are paid by the legacy source
// preparation.
fn inspect_value_blocks<T: CloneBacking>(
    value: &T,
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    inspect_metered(meter, |backing| {
        clone_backing::inspect_blocks(value, backing)
    })
}

fn inspect_slice_blocks<T: CloneBacking>(
    values: &[T],
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    inspect_metered(meter, |backing| {
        clone_backing::inspect_blocks_slice(values, backing)
    })
}

fn authority_metered<T>(
    meter: &dyn SourceMeter,
    action: impl FnOnce(&dyn BackingMeter) -> Result<T, AuthorityError>,
) -> Result<T, RSpaceError> {
    let failure = RefCell::new(None);
    let backing = |operations, scanned, bytes| {
        meter.reserve(operations, scanned, bytes).map_err(|error| {
            *failure.borrow_mut() = Some(error);
            BackingError::Rejected
        })
    };
    let result = action(&backing);
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    result.map_err(|error| match error {
        AuthorityError::HostWorkRejected => RSpaceError::HostWorkRejected,
        other => construction_error(other),
    })
}

#[cfg(test)]
pub(super) fn canonical_observation_authority(
    authority: &CostAuthority,
) -> Result<CostAuthority, InterpreterError> {
    let canonical = authority::canonical_authority(authority).map_err(error)?;
    if canonical.regions.is_empty() {
        return Err(error(authority::AuthorityError::MissingAuthority));
    }
    Ok(canonical)
}

impl MeasuredRSpaceObservation<'_> {
    #[cfg(test)]
    pub(crate) fn into_native(self) -> Result<ByteObservation, InterpreterError> {
        Ok(ByteObservation {
            event_id: self.event_id,
            kind: self.kind,
            authority: canonical_observation_authority(&self.authority)?,
            measurement: Some(self.measurement),
            legacy_amount: None,
        })
    }

    pub(crate) fn into_native_metered(
        self,
        meter: &dyn SourceMeter,
    ) -> Result<ByteObservation, RSpaceError> {
        let authority = authority_metered(meter, |backing| {
            authority::canonical_authority_metered(&self.authority, backing)
        })?;
        if authority.regions.is_empty() {
            return Err(construction_error(AuthorityError::MissingAuthority));
        }
        Ok(ByteObservation {
            event_id: self.event_id,
            kind: self.kind,
            authority,
            measurement: Some(self.measurement),
            legacy_amount: None,
        })
    }
}

impl MeasuredCommObservation {
    #[cfg(test)]
    pub(crate) fn into_native(self) -> Result<ByteObservation, InterpreterError> {
        if self.authority.is_empty() {
            return Err(error(authority::AuthorityError::MissingAuthority));
        }
        Ok(ByteObservation {
            event_id: self.event_id,
            kind: AuthorityByteEventKind::Comm,
            authority: self.authority.into_authority(),
            measurement: Some(self.measurement),
            legacy_amount: None,
        })
    }

    /// Added by D-F1 (DR-117): the native observation moves the witness. The
    /// authority is canonical, so no canonicalization runs and no copy is
    /// made, and the empty-authority check stays.
    pub(crate) fn into_native_metered(
        self,
        _meter: &dyn SourceMeter,
    ) -> Result<ByteObservation, RSpaceError> {
        // Changed by D-F1 (DR-117): the COMM authority is a canonical
        // witness, because the merge validated each region once.
        // let authority = authority_metered(meter, |backing| {
        //     authority::canonical_authority_metered(&self.authority, backing)
        // })?;
        if self.authority.is_empty() {
            return Err(construction_error(AuthorityError::MissingAuthority));
        }
        Ok(ByteObservation {
            event_id: self.event_id,
            kind: AuthorityByteEventKind::Comm,
            authority: self.authority.into_authority(),
            measurement: Some(self.measurement),
            legacy_amount: None,
        })
    }
}

pub(crate) fn produce_introduction<'a>(
    source: &Produce,
    channel: &Par,
    data: &ListParWithRandom,
    introduction_authority: &'a CostAuthority,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    Ok(MeasuredRSpaceObservation {
        event_id: byte_accounting::produce_introduction_identity(source),
        kind: AuthorityByteEventKind::ProduceIntroduction,
        authority: Cow::Borrowed(introduction_authority),
        measurement: byte_accounting::produce_introduction_charge(channel, data)
            .map_err(construction_error)?,
    })
}

// Test-only by D-F2 (DR-118): the replay observer uses
// `produce_introduction_recorded_metered`. The tests keep this walked form as
// the reference.
#[cfg(test)]
pub(crate) fn produce_introduction_metered<'a>(
    source: &Produce,
    channel: &Par,
    data: &ListParWithRandom,
    introduction_authority: &'a CostAuthority,
    meter: &dyn SourceMeter,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    let identity = byte_accounting::produce_introduction_identity_metered(source, meter)?;
    produce_introduction_metered_with_identity(
        identity,
        channel,
        data,
        introduction_authority,
        meter,
    )
}

pub(crate) fn produce_introduction_metered_with_identity<'a>(
    identity: [u8; 32],
    channel: &Par,
    data: &ListParWithRandom,
    introduction_authority: &'a CostAuthority,
    meter: &dyn SourceMeter,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    // Changed by D-O1 (DR-110): a block inspection prepays one traversal, the
    // prost length of the channel. The other channel reads reserve their own
    // traversals.
    // inspect_value(channel, meter)?;
    inspect_value_blocks(channel, meter)?;
    // Changed by D-O1 (DR-94): block accounting charges inline bytes once per enclosing block.
    // inspect_value(data, meter)?;
    inspect_value_blocks(data, meter)?;
    Ok(MeasuredRSpaceObservation {
        event_id: identity,
        kind: AuthorityByteEventKind::ProduceIntroduction,
        authority: Cow::Borrowed(introduction_authority),
        measurement: byte_accounting::produce_introduction_charge(channel, data)
            .map_err(construction_error)?,
    })
}

pub(crate) fn consume_introduction<'a>(
    source: &Consume,
    channels: &[Par],
    patterns: &[BindPattern],
    continuation: &TaggedContinuation,
    introduction_authority: &'a CostAuthority,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    Ok(MeasuredRSpaceObservation {
        event_id: byte_accounting::consume_introduction_identity(source),
        kind: AuthorityByteEventKind::ConsumeIntroduction,
        authority: Cow::Borrowed(introduction_authority),
        measurement: byte_accounting::consume_introduction_charge(channels, patterns, continuation)
            .map_err(construction_error)?,
    })
}

// Test-only by D-F2 (DR-118): the replay observer uses
// `consume_introduction_recorded_metered`. The tests keep this walked form as
// the reference.
#[cfg(test)]
pub(crate) fn consume_introduction_metered<'a>(
    source: &Consume,
    channels: &[Par],
    patterns: &[BindPattern],
    continuation: &TaggedContinuation,
    introduction_authority: &'a CostAuthority,
    meter: &dyn SourceMeter,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    let identity = byte_accounting::consume_introduction_identity_metered(source, meter)?;
    consume_introduction_metered_with_identity(
        identity,
        channels,
        patterns,
        continuation,
        introduction_authority,
        meter,
    )
}

pub(crate) fn consume_introduction_metered_with_identity<'a>(
    identity: [u8; 32],
    channels: &[Par],
    patterns: &[BindPattern],
    continuation: &TaggedContinuation,
    introduction_authority: &'a CostAuthority,
    meter: &dyn SourceMeter,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    // Changed by D-O1 (DR-110): a block inspection prepays one traversal, the
    // prost lengths of the channels. The other channel reads reserve their
    // own traversals.
    // inspect_slice(channels, meter)?;
    inspect_slice_blocks(channels, meter)?;
    // Changed by D-O1 (DR-94): block accounting charges inline bytes once per enclosing block.
    // inspect_slice(patterns, meter)?;
    inspect_slice_blocks(patterns, meter)?;
    // Changed by D-O1 (DR-94): block accounting charges inline bytes once per enclosing block.
    // inspect_value(continuation, meter)?;
    inspect_value_blocks(continuation, meter)?;
    Ok(MeasuredRSpaceObservation {
        event_id: identity,
        kind: AuthorityByteEventKind::ConsumeIntroduction,
        authority: Cow::Borrowed(introduction_authority),
        measurement: byte_accounting::consume_introduction_charge(channels, patterns, continuation)
            .map_err(construction_error)?,
    })
}

/// Added by D-F2 (DR-118): the produce introduction of a datum whose `pars`
/// length the reducer measured. It walks the channel, the seal and the stack,
/// and it reads the length of the random state. It does not walk `pars`.
pub(crate) fn produce_introduction_premeasured_metered_with_identity<'a>(
    identity: [u8; 32],
    channel: &Par,
    data: &ListParWithRandom,
    pars: u64,
    introduction_authority: &'a CostAuthority,
    meter: &dyn SourceMeter,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    inspect_value_blocks(channel, meter)?;
    inspect_value_blocks(&data.cost_authority, meter)?;
    inspect_value_blocks(&data.cost_stack, meter)?;
    meter.reserve(3, clone_backing::BLOCK_FIELD_SCANNED, 0)?;
    let measurement = byte_accounting::produce_introduction_charge_premeasured(channel, data, pars)
        .map_err(construction_error)?;
    #[cfg(any(test, debug_assertions))]
    assert_eq!(
        Ok(measurement),
        byte_accounting::produce_introduction_charge(channel, data),
        "a premeasured produce introduction differs from the walked one"
    );
    Ok(MeasuredRSpaceObservation {
        event_id: identity,
        kind: AuthorityByteEventKind::ProduceIntroduction,
        authority: Cow::Borrowed(introduction_authority),
        measurement,
    })
}

/// Added by D-F2 (DR-118): the consume introduction of a continuation whose
/// body and guard lengths the reducer measured. It reads the continuation's
/// body variant and guard presence. When they match the measurement, it walks
/// the channels, the patterns and the authority, but not the body or the
/// guard. Otherwise it walks the whole continuation.
pub(crate) fn consume_introduction_premeasured_metered_with_identity<'a>(
    identity: [u8; 32],
    channels: &[Par],
    patterns: &[BindPattern],
    continuation: &TaggedContinuation,
    body: u64,
    guard: Option<u64>,
    introduction_authority: &'a CostAuthority,
    meter: &dyn SourceMeter,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    meter.reserve(6, 2 * clone_backing::BLOCK_FIELD_SCANNED, 0)?;
    if !byte_accounting::consume_premeasurement_applies(continuation, guard) {
        return consume_introduction_metered_with_identity(
            identity,
            channels,
            patterns,
            continuation,
            introduction_authority,
            meter,
        );
    }
    inspect_slice_blocks(channels, meter)?;
    inspect_slice_blocks(patterns, meter)?;
    inspect_value_blocks(&continuation.cost_authority, meter)?;
    let measurement = byte_accounting::consume_introduction_charge_premeasured(
        channels,
        patterns,
        continuation,
        body,
        guard,
    )
    .map_err(construction_error)?
    .ok_or_else(|| {
        RSpaceError::BugFoundError("premeasured continuation lost its shape".to_owned())
    })?;
    #[cfg(any(test, debug_assertions))]
    assert_eq!(
        Ok(measurement),
        byte_accounting::consume_introduction_charge(channels, patterns, continuation),
        "a premeasured consume introduction differs from the walked one"
    );
    Ok(MeasuredRSpaceObservation {
        event_id: identity,
        kind: AuthorityByteEventKind::ConsumeIntroduction,
        authority: Cow::Borrowed(introduction_authority),
        measurement,
    })
}

/// Added by D-F2 (DR-118): the produce introduction of a registered record. It
/// reuses the reducer's measurement when the record has one, and it walks the
/// datum otherwise.
pub(crate) fn produce_introduction_recorded_metered_with_identity<'a>(
    identity: [u8; 32],
    channel: &Par,
    data: &ListParWithRandom,
    record: &'a IntroductionRecord,
    meter: &dyn SourceMeter,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    match record.measurement {
        Some(IntroductionMeasurement::Produce { pars }) => {
            produce_introduction_premeasured_metered_with_identity(
                identity,
                channel,
                data,
                pars,
                &record.authority,
                meter,
            )
        }
        _ => produce_introduction_metered_with_identity(
            identity,
            channel,
            data,
            &record.authority,
            meter,
        ),
    }
}

pub(crate) fn produce_introduction_recorded_metered<'a>(
    source: &Produce,
    channel: &Par,
    data: &ListParWithRandom,
    record: &'a IntroductionRecord,
    meter: &dyn SourceMeter,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    let identity = byte_accounting::produce_introduction_identity_metered(source, meter)?;
    produce_introduction_recorded_metered_with_identity(identity, channel, data, record, meter)
}

/// Added by D-F2 (DR-118): the consume introduction of a registered record. It
/// reuses the reducer's measurement when the record has one, and it walks the
/// continuation otherwise.
pub(crate) fn consume_introduction_recorded_metered_with_identity<'a>(
    identity: [u8; 32],
    channels: &[Par],
    patterns: &[BindPattern],
    continuation: &TaggedContinuation,
    record: &'a IntroductionRecord,
    meter: &dyn SourceMeter,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    match record.measurement {
        Some(IntroductionMeasurement::Consume { body, guard }) => {
            consume_introduction_premeasured_metered_with_identity(
                identity,
                channels,
                patterns,
                continuation,
                body,
                guard,
                &record.authority,
                meter,
            )
        }
        _ => consume_introduction_metered_with_identity(
            identity,
            channels,
            patterns,
            continuation,
            &record.authority,
            meter,
        ),
    }
}

pub(crate) fn consume_introduction_recorded_metered<'a>(
    source: &Consume,
    channels: &[Par],
    patterns: &[BindPattern],
    continuation: &TaggedContinuation,
    record: &'a IntroductionRecord,
    meter: &dyn SourceMeter,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    let identity = byte_accounting::consume_introduction_identity_metered(source, meter)?;
    consume_introduction_recorded_metered_with_identity(
        identity,
        channels,
        patterns,
        continuation,
        record,
        meter,
    )
}

pub(crate) fn comm(
    comm: &COMM,
    continuation: &TaggedContinuation,
    continuation_persistent: bool,
    data: &[(&ListParWithRandom, bool)],
    residue: &authority::ResidueContext,
) -> Result<MeasuredCommObservation, RSpaceError> {
    let identity: [u8; 32] = comm
        .cost_identity()
        .0
        .as_slice()
        .try_into()
        .map_err(|_| RSpaceError::BugFoundError("invalid COMM identity length".to_owned()))?;
    comm_with_identity(
        comm,
        continuation,
        continuation_persistent,
        data,
        identity,
        residue,
        None,
    )
}

pub(crate) fn comm_metered(
    comm: &COMM,
    continuation: &TaggedContinuation,
    continuation_persistent: bool,
    data: &[(&ListParWithRandom, bool)],
    residue: &authority::ResidueContext,
    meter: &dyn SourceMeter,
) -> Result<MeasuredCommObservation, RSpaceError> {
    // Disabled by C12 (DR-76): cost_identity_metered inspects the consume,
    // peeks and repetition counts and reserves its produce work itself, and
    // comm_charge reads only the channel count. This inspection charged the
    // COMM a second time.
    // inspect_value(comm, meter)?;
    // Disabled by C12 (DR-76): the construction reads only
    // continuation.cost_authority, and merge_authorities_metered and
    // authority_regions_metered meter that read. The continuation body is
    // never read, so walking it charged work that no step performs.
    // inspect_value(continuation, meter)?;
    for (datum, _) in data {
        // Changed by D-O1 (DR-94): block accounting charges inline bytes once per enclosing block.
        // inspect_value(*datum, meter)?;
        inspect_value_blocks(*datum, meter)?;
    }
    let identity: [u8; 32] = comm
        .cost_identity_metered(meter)?
        .0
        .as_slice()
        .try_into()
        .map_err(|_| RSpaceError::BugFoundError("invalid COMM identity length".to_owned()))?;
    comm_with_identity(
        comm,
        continuation,
        continuation_persistent,
        data,
        identity,
        residue,
        Some(meter),
    )
}

fn extend_persistent_regions(
    regions: &mut BTreeSet<[u8; 32]>,
    authority: &CostAuthority,
    meter: Option<&dyn SourceMeter>,
) -> Result<(), RSpaceError> {
    let source = match meter {
        Some(meter) => authority_metered(meter, |backing| {
            authority::authority_regions_metered(authority, backing)
        })?,
        None => authority::authority_regions(authority).map_err(construction_error)?,
    };
    for key in source.into_keys() {
        if let Some(meter) = meter {
            let next = regions
                .len()
                .checked_add(1)
                .ok_or(RSpaceError::HostWorkRejected)?;
            let (next_ops, next_bytes) =
                tree_backing::<[u8; 32], ()>(next).ok_or(RSpaceError::HostWorkRejected)?;
            let (prior_ops, prior_bytes) =
                tree_backing::<[u8; 32], ()>(regions.len()).ok_or(RSpaceError::HostWorkRejected)?;
            meter.reserve(
                next_ops
                    .checked_sub(prior_ops)
                    .and_then(|count| count.checked_add(next))
                    .ok_or(RSpaceError::HostWorkRejected)?,
                next.checked_mul(32).ok_or(RSpaceError::HostWorkRejected)?,
                next_bytes
                    .checked_sub(prior_bytes)
                    .ok_or(RSpaceError::HostWorkRejected)?,
            )?;
        }
        regions.insert(key);
    }
    Ok(())
}

/// DR-101: the authority that a stored seal charges in the deployment of `residue`.
fn resolved_seal<'a>(
    seal: &'a CostAuthority,
    entropy: &[u8],
    residue: &authority::ResidueContext,
    meter: Option<&dyn SourceMeter>,
) -> Result<Cow<'a, CostAuthority>, RSpaceError> {
    match meter {
        Some(meter) => authority_metered(meter, |backing| {
            authority::resolve_system_residue_metered(seal, entropy, residue, backing)
        }),
        None => {
            authority::resolve_system_residue(seal, entropy, residue).map_err(construction_error)
        }
    }
}

fn continuation_entropy(continuation: &TaggedContinuation) -> &[u8] {
    match continuation.tagged_cont.as_ref() {
        Some(models::rhoapi::tagged_continuation::TaggedCont::ParBody(body)) => &body.random_state,
        _ => &[],
    }
}

fn comm_with_identity(
    comm: &COMM,
    continuation: &TaggedContinuation,
    continuation_persistent: bool,
    data: &[(&ListParWithRandom, bool)],
    identity: [u8; 32],
    residue: &authority::ResidueContext,
    meter: Option<&dyn SourceMeter>,
) -> Result<MeasuredCommObservation, RSpaceError> {
    let mut authorities = Vec::<&CostAuthority>::new();
    let mut datum_seals = Vec::<Option<Cow<'_, CostAuthority>>>::new();
    if let Some(meter) = meter {
        let capacity = data
            .len()
            .checked_add(1)
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(
            capacity
                .checked_mul(2)
                .ok_or(RSpaceError::HostWorkRejected)?,
            0,
            capacity
                .checked_mul(
                    std::mem::size_of::<&CostAuthority>()
                        + std::mem::size_of::<Option<Cow<'_, CostAuthority>>>(),
                )
                .ok_or(RSpaceError::HostWorkRejected)?,
        )?;
        authorities
            .try_reserve_exact(capacity)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        datum_seals
            .try_reserve_exact(data.len())
            .map_err(|_| RSpaceError::HostWorkRejected)?;
    }
    // Changed by DR-101: every participant's seal, the incoming one included,
    // is charged as this deployment resolves it, so a COMM never charges the
    // payer of an earlier deployment for system residue.
    // if let Some(authority) = continuation.cost_authority.as_ref() {
    //     authorities.push(authority);
    //     if continuation_persistent {
    //         extend_persistent_regions(&mut persistent_regions, authority, meter)?;
    //     }
    // }
    // for (datum, persistent) in data {
    //     if let Some(authority) = datum.cost_authority.as_ref() {
    //         authorities.push(authority);
    //         if *persistent {
    //             extend_persistent_regions(&mut persistent_regions, authority, meter)?;
    //         }
    //     }
    // }
    let continuation_seal = continuation
        .cost_authority
        .as_ref()
        .map(|seal| resolved_seal(seal, continuation_entropy(continuation), residue, meter))
        .transpose()?;
    for (datum, _) in data {
        datum_seals.push(
            datum
                .cost_authority
                .as_ref()
                .map(|seal| resolved_seal(seal, &datum.random_state, residue, meter))
                .transpose()?,
        );
    }
    let mut persistent_regions = BTreeSet::new();
    if let Some(authority) = continuation_seal.as_deref() {
        authorities.push(authority);
        if continuation_persistent {
            extend_persistent_regions(&mut persistent_regions, authority, meter)?;
        }
    }
    for ((_, persistent), seal) in data.iter().zip(&datum_seals) {
        if let Some(authority) = seal.as_deref() {
            authorities.push(authority);
            if *persistent {
                extend_persistent_regions(&mut persistent_regions, authority, meter)?;
            }
        }
    }
    // Changed by D-F1 (DR-117): the metered merge reads the regions in place
    // and returns the witness, and the rekey keeps it canonical without a
    // second validation of each signature. The unmetered path keeps the
    // legacy merge and instantiation and returns the same witness.
    // let authority = match meter {
    //     Some(meter) => authority_metered(meter, |backing| {
    //         authority::merge_authorities_metered(authorities, backing)
    //     })?,
    //     None => authority::merge_authorities(authorities).map_err(construction_error)?,
    // };
    // let authority = match meter {
    //     Some(meter) => authority_metered(meter, |backing| {
    //         authority::instantiate_persistent_regions_metered(
    //             &authority,
    //             &persistent_regions,
    //             identity,
    //             backing,
    //         )
    //     })?,
    //     None => {
    //         authority::instantiate_persistent_regions(&authority, &persistent_regions, identity)
    //             .map_err(construction_error)?
    //     }
    // };
    let authority = match meter {
        Some(meter) => {
            let merged = authority_metered(meter, |backing| {
                authority::merge_canonical_metered(authorities.iter().copied(), backing)
            })?;
            authority_metered(meter, |backing| {
                authority::rekey_metered(merged, &persistent_regions, identity, backing)
            })?
        }
        None => {
            let merged = authority::merge_authorities(authorities).map_err(construction_error)?;
            authority::instantiate_persistent_regions_canonical(
                &merged,
                &persistent_regions,
                identity,
            )
            .map_err(construction_error)?
        }
    };
    Ok(MeasuredCommObservation {
        event_id: identity,
        authority,
        measurement: byte_accounting::comm_charge(comm, data).map_err(construction_error)?,
    })
}

#[cfg(test)]
mod metered_tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    #[test]
    fn metered_introductions_preserve_identity_and_charge_before_hashing() {
        let channel = Par::default();
        let data = ListParWithRandom::default();
        let continuation = TaggedContinuation::default();
        let pattern = BindPattern::default();
        let channels = vec![channel.clone()];
        let patterns = vec![pattern];
        let produce = Produce::create(&channel, &data, false);
        let consume = Consume::create(&channels, &patterns, &continuation, false);
        let authority = CostAuthority::default();
        let unlimited = |_: usize, _: usize, _: usize| Ok(());
        let ordinary_produce = produce_introduction(&produce, &channel, &data, &authority).unwrap();
        let metered_produce =
            produce_introduction_metered(&produce, &channel, &data, &authority, &unlimited)
                .unwrap();
        assert_eq!(ordinary_produce.event_id, metered_produce.event_id);
        assert_eq!(ordinary_produce.measurement, metered_produce.measurement);
        let ordinary_consume =
            consume_introduction(&consume, &channels, &patterns, &continuation, &authority)
                .unwrap();
        let metered_consume = consume_introduction_metered(
            &consume,
            &channels,
            &patterns,
            &continuation,
            &authority,
            &unlimited,
        )
        .unwrap();
        assert_eq!(ordinary_consume.event_id, metered_consume.event_id);
        assert_eq!(ordinary_consume.measurement, metered_consume.measurement);

        let rejected_hash = AtomicBool::new(false);
        let reject = |_: usize, _: usize, backing: usize| {
            if backing == 32 {
                rejected_hash.store(true, Ordering::Relaxed);
                Err(RSpaceError::HostWorkRejected)
            } else {
                Ok(())
            }
        };
        assert!(matches!(
            produce_introduction_metered(&produce, &channel, &data, &authority, &reject),
            Err(RSpaceError::HostWorkRejected)
        ));
        assert!(rejected_hash.load(Ordering::Relaxed));
    }
}
