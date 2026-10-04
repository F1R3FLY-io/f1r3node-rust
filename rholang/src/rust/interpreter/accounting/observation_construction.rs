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
use super::byte_accounting::{self, ByteCharge};
use super::byte_receipts::ByteObservation;
#[cfg(test)]
use super::InterpreterError;

pub(crate) struct MeasuredRSpaceObservation<'a> {
    pub(crate) event_id: [u8; 32],
    pub(crate) kind: AuthorityByteEventKind,
    pub(crate) authority: Cow<'a, CostAuthority>,
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

fn inspect_value<T: CloneBacking>(value: &T, meter: &dyn SourceMeter) -> Result<(), RSpaceError> {
    inspect_metered(meter, |backing| clone_backing::inspect(value, backing))
}

fn inspect_slice<T: CloneBacking>(
    values: &[T],
    meter: &dyn SourceMeter,
) -> Result<(), RSpaceError> {
    inspect_metered(meter, |backing| {
        clone_backing::inspect_slice(values, backing)
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
    inspect_value(channel, meter)?;
    inspect_value(data, meter)?;
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
    inspect_slice(channels, meter)?;
    inspect_slice(patterns, meter)?;
    inspect_value(continuation, meter)?;
    Ok(MeasuredRSpaceObservation {
        event_id: identity,
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
        None,
    )
}

pub(crate) fn comm_metered(
    comm: &COMM,
    continuation: &TaggedContinuation,
    continuation_persistent: bool,
    data: &[(&ListParWithRandom, bool)],
    meter: &dyn SourceMeter,
) -> Result<MeasuredRSpaceObservation<'static>, RSpaceError> {
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
        inspect_value(*datum, meter)?;
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

fn comm_with_identity(
    comm: &COMM,
    continuation: &TaggedContinuation,
    continuation_persistent: bool,
    data: &[(&ListParWithRandom, bool)],
    identity: [u8; 32],
    meter: Option<&dyn SourceMeter>,
) -> Result<MeasuredRSpaceObservation<'static>, RSpaceError> {
    let mut authorities = Vec::<&CostAuthority>::new();
    if let Some(meter) = meter {
        let capacity = data
            .len()
            .checked_add(1)
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(
            capacity,
            0,
            capacity
                .checked_mul(std::mem::size_of::<&CostAuthority>())
                .ok_or(RSpaceError::HostWorkRejected)?,
        )?;
        authorities
            .try_reserve_exact(capacity)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
    }
    let mut persistent_regions = BTreeSet::new();
    if let Some(authority) = continuation.cost_authority.as_ref() {
        authorities.push(authority);
        if continuation_persistent {
            extend_persistent_regions(&mut persistent_regions, authority, meter)?;
        }
    }
    for (datum, persistent) in data {
        if let Some(authority) = datum.cost_authority.as_ref() {
            authorities.push(authority);
            if *persistent {
                extend_persistent_regions(&mut persistent_regions, authority, meter)?;
            }
        }
    }
    let authority = match meter {
        Some(meter) => authority_metered(meter, |backing| {
            authority::merge_authorities_metered(authorities, backing)
        })?,
        None => authority::merge_authorities(authorities).map_err(construction_error)?,
    };
    let authority = match meter {
        Some(meter) => authority_metered(meter, |backing| {
            authority::instantiate_persistent_regions_metered(
                &authority,
                &persistent_regions,
                identity,
                backing,
            )
        })?,
        None => {
            authority::instantiate_persistent_regions(&authority, &persistent_regions, identity)
                .map_err(construction_error)?
        }
    };
    Ok(MeasuredRSpaceObservation {
        event_id: identity,
        kind: AuthorityByteEventKind::Comm,
        authority: Cow::Owned(authority),
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
