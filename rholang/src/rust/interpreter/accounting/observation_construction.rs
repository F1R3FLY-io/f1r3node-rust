use std::borrow::Cow;
use std::collections::BTreeSet;

use models::rhoapi::{BindPattern, CostAuthority, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce, COMM};

use super::authority::{self, AuthorityByteEventKind};
use super::byte_accounting::{self, ByteCharge};
use super::byte_receipts::ByteObservation;
use super::InterpreterError;

pub(crate) struct MeasuredRSpaceObservation<'a> {
    pub(crate) event_id: [u8; 32],
    pub(crate) kind: AuthorityByteEventKind,
    pub(crate) authority: Cow<'a, CostAuthority>,
    pub(crate) measurement: ByteCharge,
}

fn error(error: impl std::fmt::Display) -> InterpreterError {
    InterpreterError::ReduceError(error.to_string())
}

fn construction_error(error: impl std::fmt::Display) -> RSpaceError {
    RSpaceError::InterpreterError(error.to_string())
}

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
    pub(crate) fn into_native(self) -> Result<ByteObservation, InterpreterError> {
        Ok(ByteObservation {
            event_id: self.event_id,
            kind: self.kind,
            authority: canonical_observation_authority(&self.authority)?,
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

pub(crate) fn comm(
    comm: &COMM,
    continuation: &TaggedContinuation,
    continuation_persistent: bool,
    data: &[(&ListParWithRandom, bool)],
) -> Result<MeasuredRSpaceObservation<'static>, RSpaceError> {
    let identity: [u8; 32] = comm
        .cost_identity()
        .bytes()
        .try_into()
        .map_err(|_| RSpaceError::BugFoundError("invalid COMM identity length".to_owned()))?;
    let mut authorities = Vec::<&CostAuthority>::new();
    let mut persistent_regions = BTreeSet::new();
    if let Some(authority) = continuation.cost_authority.as_ref() {
        authorities.push(authority);
        if continuation_persistent {
            persistent_regions.extend(
                authority::authority_regions(authority)
                    .map_err(construction_error)?
                    .into_keys(),
            );
        }
    }
    for (datum, persistent) in data {
        if let Some(authority) = datum.cost_authority.as_ref() {
            authorities.push(authority);
            if *persistent {
                persistent_regions.extend(
                    authority::authority_regions(authority)
                        .map_err(construction_error)?
                        .into_keys(),
                );
            }
        }
    }
    let authority = authority::merge_authorities(authorities).map_err(construction_error)?;
    let authority =
        authority::instantiate_persistent_regions(&authority, &persistent_regions, identity)
            .map_err(construction_error)?;
    Ok(MeasuredRSpaceObservation {
        event_id: identity,
        kind: AuthorityByteEventKind::Comm,
        authority: Cow::Owned(authority),
        measurement: byte_accounting::comm_charge(comm, data).map_err(construction_error)?,
    })
}
