use std::collections::HashSet;
use std::mem::size_of;

use models::rhoapi::PCost;
use models::rust::casper::protocol::casper_message::Event as CasperEvent;
use models::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeFormat};
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use models::rust::native_cost_evidence::{NativeCostEvidenceV1, NativeCostFailureClass};
use models::rust::phlo_schedule::PhloGenesisPolicy;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rspace_plus_plus::rspace::trace::event::{Consume, Event, IOEvent, Produce};

use super::direct_wallet_funding::NativeOfferedPreparedResult;
use super::genesis_resource_policy::AdoptedResourcePolicy;
use super::offered_evidence::encode_committed_native_evidence;
use crate::rust::errors::CasperError;
use crate::rust::util::event_converter;
use crate::rust::util::rholang::acceptance::PreparedOfferedCandidate;

fn invalid(reason: &str) -> CasperError { CasperError::RuntimeError(reason.to_owned()) }

fn reserve(
    budget: &HostWorkBudget,
    dimension: HostWorkDimension,
    amount: usize,
) -> Result<(), CasperError> {
    let units = u64::try_from(amount).map_err(|_| invalid("offered event work overflows"))?;
    budget
        .reserve(dimension, HostWorkUnits::new(units))
        .map_err(|error| invalid(&error.to_string()))?;
    Ok(())
}

fn add(total: &mut usize, amount: usize) -> Result<(), CasperError> {
    *total = total
        .checked_add(amount)
        .ok_or_else(|| invalid("offered event size overflows"))?;
    Ok(())
}

#[derive(Default)]
struct LogShape {
    events: usize,
    items: usize,
    bytes: usize,
    clone_bytes: usize,
}

impl LogShape {
    fn event(&mut self, items: usize, bytes: usize, clone_bytes: usize) -> Result<(), CasperError> {
        add(&mut self.events, 1)?;
        add(&mut self.items, items)?;
        add(&mut self.bytes, bytes)?;
        add(&mut self.clone_bytes, clone_bytes)?;
        let limits = offered_funded_v6_limits();
        if self.events > limits.deploy_log_events
            || self.items > limits.deploy_log_items
            || self.bytes > limits.deploy_log_bytes
            || self.clone_bytes > limits.deploy_log_bytes
        {
            return Err(invalid("offered event log exceeds protocol limits"));
        }
        Ok(())
    }
}

fn produce_shape(
    produce: &Produce,
    budget: &HostWorkBudget,
) -> Result<(usize, usize), CasperError> {
    if produce.output_value.len() >= offered_funded_v6_limits().deploy_log_items {
        return Err(invalid("offered produce items exceed protocol limit"));
    }
    reserve(
        budget,
        HostWorkDimension::VerificationOperations,
        produce.output_value.len(),
    )?;
    let items = produce
        .output_value
        .len()
        .checked_add(1)
        .ok_or_else(|| invalid("offered produce item count overflows"))?;
    let payload = produce
        .output_value
        .iter()
        .try_fold(64usize, |size, value| size.checked_add(value.len()))
        .ok_or_else(|| invalid("offered produce payload size overflows"))?;
    Ok((items, payload))
}

fn consume_shape(consume: &Consume) -> Result<(usize, usize), CasperError> {
    if consume.channel_hashes.len() >= offered_funded_v6_limits().deploy_log_items {
        return Err(invalid("offered consume items exceed protocol limit"));
    }
    let items = consume
        .channel_hashes
        .len()
        .checked_add(1)
        .ok_or_else(|| invalid("offered consume item count overflows"))?;
    let payload = consume
        .channel_hashes
        .len()
        .checked_mul(32)
        .and_then(|size| size.checked_add(32))
        .ok_or_else(|| invalid("offered consume payload size overflows"))?;
    Ok((items, payload))
}

fn count_native_event(
    shape: &mut LogShape,
    event: &Event,
    budget: &HostWorkBudget,
) -> Result<(), CasperError> {
    reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
    let (items, payload, map_bytes) = match event {
        Event::IoEvent(IOEvent::Produce(produce)) => {
            let (items, payload) = produce_shape(produce, budget)?;
            (items, payload, 0)
        }
        Event::IoEvent(IOEvent::Consume(consume)) => {
            let (items, payload) = consume_shape(consume)?;
            (items, payload, 0)
        }
        Event::Comm(comm) => {
            if comm.produces.len() > offered_funded_v6_limits().deploy_log_items
                || comm.peeks.len() > offered_funded_v6_limits().deploy_log_items
                || comm.times_repeated.len() > offered_funded_v6_limits().deploy_log_items
            {
                return Err(invalid("offered COMM items exceed protocol limit"));
            }
            reserve(
                budget,
                HostWorkDimension::VerificationOperations,
                comm.produces
                    .len()
                    .checked_add(comm.times_repeated.len())
                    .and_then(|count| count.checked_add(comm.peeks.len()))
                    .ok_or_else(|| invalid("offered COMM scan work overflows"))?,
            )?;
            let (mut items, mut payload) = consume_shape(&comm.consume)?;
            add(&mut items, comm.peeks.len())?;
            if items > offered_funded_v6_limits().deploy_log_items {
                return Err(invalid("offered COMM items exceed protocol limit"));
            }
            for produce in &comm.produces {
                let (next_items, next_payload) = produce_shape(produce, budget)?;
                add(&mut items, next_items)?;
                add(&mut payload, next_payload)?;
                if items > offered_funded_v6_limits().deploy_log_items
                    || items
                        .checked_mul(64)
                        .and_then(|size| size.checked_add(payload))
                        .is_none_or(|size| size > offered_funded_v6_limits().deploy_log_bytes)
                {
                    return Err(invalid("offered COMM exceeds protocol log limits"));
                }
            }
            if comm.times_repeated.len() > comm.produces.len() {
                return Err(invalid(
                    "offered COMM repetition map exceeds its produce list",
                ));
            }
            reserve(
                budget,
                HostWorkDimension::SearchStateBytes,
                comm.produces
                    .len()
                    .checked_mul(size_of::<&Produce>() * 4)
                    .ok_or_else(|| invalid("offered COMM key index size overflows"))?,
            )?;
            let mut produce_keys = HashSet::new();
            produce_keys
                .try_reserve(comm.produces.len())
                .map_err(|_| invalid("offered COMM key index allocation failed"))?;
            produce_keys.extend(comm.produces.iter());
            let mut map_bytes = 0usize;
            for produce in comm.times_repeated.keys() {
                if !produce_keys.contains(produce) {
                    return Err(invalid("offered COMM repetition key has no produce"));
                }
                let (_, key_payload) = produce_shape(produce, budget)?;
                add(&mut map_bytes, key_payload)?;
                add(&mut map_bytes, 96)?;
                if map_bytes > offered_funded_v6_limits().deploy_log_bytes {
                    return Err(invalid(
                        "offered COMM repetition map exceeds protocol log limit",
                    ));
                }
            }
            (items, payload, map_bytes)
        }
    };
    let envelope = items
        .checked_mul(64)
        .and_then(|size| size.checked_add(payload))
        .ok_or_else(|| invalid("offered event envelope size overflows"))?;
    let clone_bytes = envelope
        .checked_add(map_bytes)
        .ok_or_else(|| invalid("offered event clone size overflows"))?;
    shape.event(items, envelope, clone_bytes)
}

fn casper_produce_shape(
    produce: &models::rust::casper::protocol::casper_message::ProduceEvent,
    budget: &HostWorkBudget,
) -> Result<(usize, usize), CasperError> {
    if produce.output_value.len() >= offered_funded_v6_limits().deploy_log_items {
        return Err(invalid(
            "offered settled produce items exceed protocol limit",
        ));
    }
    reserve(
        budget,
        HostWorkDimension::VerificationOperations,
        produce.output_value.len(),
    )?;
    let items = produce
        .output_value
        .len()
        .checked_add(1)
        .ok_or_else(|| invalid("offered settled produce items overflow"))?;
    let payload = produce
        .output_value
        .iter()
        .try_fold(
            produce
                .channels_hash
                .len()
                .checked_add(produce.hash.len())
                .ok_or_else(|| invalid("offered settled produce bytes overflow"))?,
            |size, value| size.checked_add(value.len()),
        )
        .ok_or_else(|| invalid("offered settled produce bytes overflow"))?;
    Ok((items, payload))
}

fn casper_consume_shape(
    consume: &models::rust::casper::protocol::casper_message::ConsumeEvent,
    budget: &HostWorkBudget,
) -> Result<(usize, usize), CasperError> {
    if consume.channels_hashes.len() >= offered_funded_v6_limits().deploy_log_items {
        return Err(invalid(
            "offered settled consume items exceed protocol limit",
        ));
    }
    reserve(
        budget,
        HostWorkDimension::VerificationOperations,
        consume.channels_hashes.len(),
    )?;
    let items = consume
        .channels_hashes
        .len()
        .checked_add(1)
        .ok_or_else(|| invalid("offered settled consume items overflow"))?;
    let payload = consume
        .channels_hashes
        .iter()
        .try_fold(consume.hash.len(), |size, hash| {
            size.checked_add(hash.len())
        })
        .ok_or_else(|| invalid("offered settled consume bytes overflow"))?;
    Ok((items, payload))
}

fn count_casper_event(
    shape: &mut LogShape,
    event: &CasperEvent,
    budget: &HostWorkBudget,
) -> Result<(), CasperError> {
    reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
    let (items, payload) = match event {
        CasperEvent::Produce(produce) => casper_produce_shape(produce, budget)?,
        CasperEvent::Consume(consume) => casper_consume_shape(consume, budget)?,
        CasperEvent::Comm(comm) => {
            if comm.produces.len() > offered_funded_v6_limits().deploy_log_items
                || comm.peeks.len() > offered_funded_v6_limits().deploy_log_items
            {
                return Err(invalid("offered settled COMM items exceed protocol limit"));
            }
            reserve(
                budget,
                HostWorkDimension::VerificationOperations,
                comm.produces
                    .len()
                    .checked_add(comm.peeks.len())
                    .ok_or_else(|| invalid("offered settled COMM scan work overflows"))?,
            )?;
            let (mut items, mut payload) = casper_consume_shape(&comm.consume, budget)?;
            add(&mut items, comm.peeks.len())?;
            if items > offered_funded_v6_limits().deploy_log_items {
                return Err(invalid("offered settled COMM items exceed protocol limit"));
            }
            for produce in &comm.produces {
                let (next_items, next_payload) = casper_produce_shape(produce, budget)?;
                add(&mut items, next_items)?;
                add(&mut payload, next_payload)?;
                if items > offered_funded_v6_limits().deploy_log_items
                    || items
                        .checked_mul(64)
                        .and_then(|size| size.checked_add(payload))
                        .is_none_or(|size| size > offered_funded_v6_limits().deploy_log_bytes)
                {
                    return Err(invalid("offered settled COMM exceeds protocol log limits"));
                }
            }
            (items, payload)
        }
    };
    let bytes = items
        .checked_mul(64)
        .and_then(|size| size.checked_add(payload))
        .ok_or_else(|| invalid("offered settled event size overflows"))?;
    shape.event(items, bytes, bytes)
}

pub fn assemble_offered_processed_deploy(
    envelope: DeployEnvelope,
    preflight: &PreparedOfferedCandidate<'_>,
    adopted: &AdoptedResourcePolicy,
    prepared: NativeOfferedPreparedResult,
    budget: &HostWorkBudget,
) -> Result<OfferedProcessedDeploy, CasperError> {
    if envelope.format() != DeployEnvelopeFormat::OfferedFunded
        || envelope.identity().as_bytes() != preflight.envelope_identity
        || prepared.funding_root != preflight.funding_root
        || prepared.execution_root != preflight.execution_root
        || prepared
            .phlo_used
            .checked_add(prepared.retained_phlo)
            .filter(|used| *used <= preflight.phlo_limit)
            .is_none()
        || prepared.fresh_phlo > prepared.phlo_used
        || prepared.fee_rev != preflight.fee_rev
    {
        return Err(invalid(
            "offered processed candidate differs from authenticated preflight",
        ));
    }
    let resource_rev = u128::from(prepared.fresh_phlo)
        .checked_add(u128::from(prepared.retained_phlo))
        .and_then(|used| used.checked_mul(u128::from(preflight.phlo_price)))
        .ok_or_else(|| invalid("offered resource REV overflows"))?;
    if prepared.resource_rev != resource_rev
        || resource_rev.checked_add(prepared.fee_rev) != Some(prepared.rev_spent)
        || prepared.rev_spent > preflight.rev_ceiling
    {
        return Err(invalid("offered settlement differs from signed REV charge"));
    }
    let selected = preflight.selected_terms(adopted)?;
    let schedule_commitment = selected
        .schedule()
        .digest(PhloGenesisPolicy::LIMITS)
        .map_err(|error| invalid(&error.to_string()))?;
    if prepared.wallet_settlement_log_events > prepared.settlement_log.len()
        || prepared.wallet_settlement_log_events > offered_funded_v6_limits().deploy_log_events
    {
        return Err(invalid(
            "offered wallet log boundary exceeds its settlement log",
        ));
    }
    let wallet_settlement_log_events = u64::try_from(prepared.wallet_settlement_log_events)
        .map_err(|_| invalid("offered wallet log boundary overflows"))?;
    let evidence = NativeCostEvidenceV1 {
        envelope_commitment: preflight.envelope_identity,
        genesis_policy_commitment: adopted.genesis_policy_commitment()?,
        schedule_commitment,
        original_funding_root: prepared.funding_root,
        settlement_runtime_root: prepared.settlement_root,
        post_state_root: prepared.final_root,
        phlo_used: prepared.phlo_used,
        fresh_phlo: prepared.fresh_phlo,
        retained_phlo: prepared.retained_phlo,
        phlo_limit: preflight.phlo_limit,
        phlo_price: preflight.phlo_price,
        fee_rev: prepared.fee_rev,
        failure_class: prepared.failure_class,
        wallet_settlement_log_events,
        budget_recording: &prepared.recording.budget,
        operation_journal: &prepared.recording.journal,
        funding_case: &prepared.funding_case,
        prepaid_delta: &prepared.prepaid_delta,
        wallet_settlement: &prepared.wallet_receipt,
    };
    let limits = offered_funded_v6_limits();
    let native_cost_evidence =
        encode_committed_native_evidence(&evidence, limits.evidence, budget)?;
    let mut shape = LogShape::default();
    for event in prepared
        .user_log
        .iter()
        .chain(prepared.grant_drain_log.iter())
    {
        count_native_event(&mut shape, event, budget)?;
    }
    for event in &prepared.settlement_log {
        count_casper_event(&mut shape, event, budget)?;
    }
    reserve(
        budget,
        HostWorkDimension::VerificationOperations,
        shape
            .items
            .checked_add(shape.events)
            .ok_or_else(|| invalid("offered event work overflows"))?,
    )?;
    reserve(
        budget,
        HostWorkDimension::VerificationBytes,
        shape.clone_bytes,
    )?;
    reserve(
        budget,
        HostWorkDimension::SearchStateBytes,
        shape
            .clone_bytes
            .checked_mul(3)
            .ok_or_else(|| invalid("offered event allocation size overflows"))?
            .checked_add(
                shape
                    .events
                    .checked_mul(size_of::<CasperEvent>())
                    .ok_or_else(|| invalid("offered event vector size overflows"))?,
            )
            .ok_or_else(|| invalid("offered event allocation size overflows"))?,
    )?;
    let mut deploy_log = Vec::new();
    deploy_log
        .try_reserve_exact(shape.events)
        .map_err(|_| invalid("offered event log allocation failed"))?;
    for event in prepared
        .user_log
        .iter()
        .chain(prepared.grant_drain_log.iter())
    {
        deploy_log.push(event_converter::to_casper_event(event.clone()));
    }
    deploy_log.extend(prepared.settlement_log);
    OfferedProcessedDeploy::new(
        envelope,
        PCost {
            cost: prepared.phlo_used,
        },
        deploy_log,
        prepared.failure_class == NativeCostFailureClass::UserFailure,
        native_cost_evidence,
        limits.evidence,
    )
    .map_err(|error| invalid(&error))
}

#[cfg(test)]
mod tests {
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
    use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

    use super::*;

    #[test]
    fn offered_log_exhaustion_rejects_before_event_conversion() {
        let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1)));
        let mut produce = Produce::new(
            Blake2b256Hash::new(b"channel"),
            Blake2b256Hash::new(b"event"),
            false,
        );
        produce.output_value = vec![vec![7; 256]];
        let event = Event::IoEvent(IOEvent::Produce(produce));
        let mut shape = LogShape::default();
        assert!(count_native_event(&mut shape, &event, &budget).is_err());
        assert_eq!(shape.events, 0);
        assert!(
            matches!(event, Event::IoEvent(IOEvent::Produce(ref value)) if value.output_value[0].len() == 256)
        );
    }

    #[test]
    fn native_and_committed_event_shapes_match_after_conversion() {
        let mut produce = Produce::new(
            Blake2b256Hash::new(b"channel"),
            Blake2b256Hash::new(b"event"),
            false,
        );
        produce.output_value = vec![vec![1, 2, 3], vec![4, 5]];
        let native_event = Event::IoEvent(IOEvent::Produce(produce));
        let mut native = LogShape::default();
        let mut committed = LogShape::default();
        let limits = HostWorkLimits::uniform(HostWorkLimit::new(10_000));
        count_native_event(&mut native, &native_event, &HostWorkBudget::new(limits)).unwrap();
        let converted = event_converter::to_casper_event(native_event);
        count_casper_event(&mut committed, &converted, &HostWorkBudget::new(limits)).unwrap();
        assert_eq!(
            (native.events, native.items, native.bytes),
            (committed.events, committed.items, committed.bytes)
        );
    }
}
