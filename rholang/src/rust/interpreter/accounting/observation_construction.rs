use std::borrow::Cow;
use std::collections::BTreeSet;

use models::rhoapi::{BindPattern, CostAuthority, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce, COMM};

use super::authority::{self, AuthorityByteEventKind, AuthorityError};
use super::byte_accounting::{self, ByteCharge, IntroductionMeasurement};
use super::byte_receipts::ByteObservation;
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
    pub(crate) kind: AuthorityByteEventKind,
    pub(crate) authority: authority::CanonicalAuthority,
    pub(crate) measurement: ByteCharge,
}

fn construction_error(error: impl std::fmt::Display) -> RSpaceError {
    RSpaceError::InterpreterError(error.to_string())
}

impl MeasuredRSpaceObservation<'_> {
    pub(crate) fn into_native(self) -> Result<ByteObservation, RSpaceError> {
        let authority =
            authority::canonical_authority(&self.authority).map_err(construction_error)?;
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
    /// Added by D-F1 (DR-117): the native observation moves the witness. The
    /// authority is canonical, so no canonicalization runs and no copy is
    /// made, and the empty-authority check stays.
    pub(crate) fn into_native(self) -> Result<ByteObservation, RSpaceError> {
        if self.authority.is_empty() {
            return Err(construction_error(AuthorityError::MissingAuthority));
        }
        Ok(ByteObservation {
            event_id: self.event_id,
            kind: self.kind,
            authority: self.authority.into_authority(),
            measurement: Some(self.measurement),
            legacy_amount: None,
        })
    }
}

// Test-only by DR-131: the runtime and the replay observer use the recorded
// forms below. The tests keep this form as the reference.
#[cfg(test)]
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

pub(crate) fn produce_introduction_with_identity<'a>(
    identity: [u8; 32],
    channel: &Par,
    data: &ListParWithRandom,
    introduction_authority: &'a CostAuthority,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    Ok(MeasuredRSpaceObservation {
        event_id: identity,
        kind: AuthorityByteEventKind::ProduceIntroduction,
        authority: Cow::Borrowed(introduction_authority),
        measurement: byte_accounting::produce_introduction_charge(channel, data)
            .map_err(construction_error)?,
    })
}

// Test-only by DR-131: as `produce_introduction`.
#[cfg(test)]
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

pub(crate) fn consume_introduction_with_identity<'a>(
    identity: [u8; 32],
    channels: &[Par],
    patterns: &[BindPattern],
    continuation: &TaggedContinuation,
    introduction_authority: &'a CostAuthority,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
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
pub(crate) fn produce_introduction_premeasured_with_identity<'a>(
    identity: [u8; 32],
    channel: &Par,
    data: &ListParWithRandom,
    pars: u64,
    introduction_authority: &'a CostAuthority,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
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
pub(crate) fn consume_introduction_premeasured_with_identity<'a>(
    identity: [u8; 32],
    channels: &[Par],
    patterns: &[BindPattern],
    continuation: &TaggedContinuation,
    body: u64,
    guard: Option<u64>,
    introduction_authority: &'a CostAuthority,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    if !byte_accounting::consume_premeasurement_applies(continuation, guard) {
        return consume_introduction_with_identity(
            identity,
            channels,
            patterns,
            continuation,
            introduction_authority,
        );
    }
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
pub(crate) fn produce_introduction_recorded_with_identity<'a>(
    identity: [u8; 32],
    channel: &Par,
    data: &ListParWithRandom,
    record: &'a IntroductionRecord,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    match record.measurement {
        Some(IntroductionMeasurement::Produce { pars }) => {
            produce_introduction_premeasured_with_identity(
                identity,
                channel,
                data,
                pars,
                &record.authority,
            )
        }
        _ => produce_introduction_with_identity(identity, channel, data, &record.authority),
    }
}

pub(crate) fn produce_introduction_recorded<'a>(
    source: &Produce,
    channel: &Par,
    data: &ListParWithRandom,
    record: &'a IntroductionRecord,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    let identity = byte_accounting::produce_introduction_identity(source);
    produce_introduction_recorded_with_identity(identity, channel, data, record)
}

/// Added by D-F2 (DR-118): the consume introduction of a registered record. It
/// reuses the reducer's measurement when the record has one, and it walks the
/// continuation otherwise.
pub(crate) fn consume_introduction_recorded_with_identity<'a>(
    identity: [u8; 32],
    channels: &[Par],
    patterns: &[BindPattern],
    continuation: &TaggedContinuation,
    record: &'a IntroductionRecord,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    match record.measurement {
        Some(IntroductionMeasurement::Consume { body, guard }) => {
            consume_introduction_premeasured_with_identity(
                identity,
                channels,
                patterns,
                continuation,
                body,
                guard,
                &record.authority,
            )
        }
        _ => consume_introduction_with_identity(
            identity,
            channels,
            patterns,
            continuation,
            &record.authority,
        ),
    }
}

pub(crate) fn consume_introduction_recorded<'a>(
    source: &Consume,
    channels: &[Par],
    patterns: &[BindPattern],
    continuation: &TaggedContinuation,
    record: &'a IntroductionRecord,
) -> Result<MeasuredRSpaceObservation<'a>, RSpaceError> {
    let identity = byte_accounting::consume_introduction_identity(source);
    consume_introduction_recorded_with_identity(identity, channels, patterns, continuation, record)
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
    observation_with_identity(
        continuation,
        continuation_persistent,
        data,
        identity,
        AuthorityByteEventKind::Comm,
        byte_accounting::comm_charge(comm, data).map_err(construction_error)?,
        residue,
    )
}

pub(crate) fn resting(
    event: &rspace_plus_plus::rspace::trace::event::RestingCOMM,
    checked_identity: [u8; 32],
    continuation: &TaggedContinuation,
    continuation_persistent: bool,
    data: &[(&ListParWithRandom, bool)],
    residue: &authority::ResidueContext,
) -> Result<MeasuredCommObservation, RSpaceError> {
    let measurement = byte_accounting::resting_comm_charge(event, data)?;
    observation_with_identity(
        continuation,
        continuation_persistent,
        data,
        checked_identity,
        AuthorityByteEventKind::RestingComm,
        measurement,
        residue,
    )
}

fn extend_persistent_regions(
    regions: &mut BTreeSet<[u8; 32]>,
    authority: &CostAuthority,
) -> Result<(), RSpaceError> {
    let source = authority::authority_regions(authority).map_err(construction_error)?;
    regions.extend(source.into_keys());
    Ok(())
}

/// DR-101: the authority that a stored seal charges in the deployment of `residue`.
fn resolved_seal<'a>(
    seal: &'a CostAuthority,
    entropy: &[u8],
    residue: &authority::ResidueContext,
) -> Result<Cow<'a, CostAuthority>, RSpaceError> {
    authority::resolve_system_residue(seal, entropy, residue).map_err(construction_error)
}

fn continuation_entropy(continuation: &TaggedContinuation) -> &[u8] {
    match continuation.tagged_cont.as_ref() {
        Some(models::rhoapi::tagged_continuation::TaggedCont::ParBody(body)) => &body.random_state,
        _ => &[],
    }
}

fn observation_with_identity(
    continuation: &TaggedContinuation,
    continuation_persistent: bool,
    data: &[(&ListParWithRandom, bool)],
    identity: [u8; 32],
    kind: AuthorityByteEventKind,
    measurement: byte_accounting::ByteCharge,
    residue: &authority::ResidueContext,
) -> Result<MeasuredCommObservation, RSpaceError> {
    let mut authorities = Vec::<&CostAuthority>::with_capacity(data.len() + 1);
    let mut datum_seals = Vec::<Option<Cow<'_, CostAuthority>>>::with_capacity(data.len());
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
        .map(|seal| resolved_seal(seal, continuation_entropy(continuation), residue))
        .transpose()?;
    for (datum, _) in data {
        datum_seals.push(
            datum
                .cost_authority
                .as_ref()
                .map(|seal| resolved_seal(seal, &datum.random_state, residue))
                .transpose()?,
        );
    }
    let mut persistent_regions = BTreeSet::new();
    if let Some(authority) = continuation_seal.as_deref() {
        authorities.push(authority);
        if continuation_persistent {
            extend_persistent_regions(&mut persistent_regions, authority)?;
        }
    }
    for ((_, persistent), seal) in data.iter().zip(&datum_seals) {
        if let Some(authority) = seal.as_deref() {
            authorities.push(authority);
            if *persistent {
                extend_persistent_regions(&mut persistent_regions, authority)?;
            }
        }
    }
    // Changed by D-F1 (DR-117): the merge reads the regions in place and
    // returns the witness, and the rekey keeps it canonical without a second
    // validation of each signature.
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
    let merged = authority::merge_canonical(authorities).map_err(construction_error)?;
    let authority =
        authority::rekey(merged, &persistent_regions, identity).map_err(construction_error)?;
    Ok(MeasuredCommObservation {
        event_id: identity,
        kind,
        authority,
        measurement,
    })
}
