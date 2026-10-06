use crate::casper::{event_proto, EventProto, ProcessedDeployProto};
use crate::rhoapi::PCost;
use crate::rust::casper::protocol::casper_message::{ConsumeEvent, Event, ProduceEvent};
use crate::rust::cost_protocol_limits::offered_funded_v6_limits;
use crate::rust::deploy_envelope::{
    DeployEnvelope, DeployEnvelopeFormat, DeployEnvelopeLimits, DeployEnvelopeRef,
};
use crate::rust::native_cost_evidence::{NativeCostEvidenceV1, NativeCostFailureClass};
use crate::rust::native_wallet_receipt::{NativeWalletReceiptLimits, NativeWalletReceiptV1};
use crate::rust::phlo_wire::PhloWireLimits;

#[derive(Clone, Debug, PartialEq)]
pub struct OfferedProcessedDeploy {
    envelope: DeployEnvelope,
    cost: PCost,
    deploy_log: Vec<Event>,
    is_failed: bool,
    native_cost_evidence: Vec<u8>,
    evidence_limits: PhloWireLimits,
}

impl OfferedProcessedDeploy {
    pub fn new(
        envelope: DeployEnvelope,
        cost: PCost,
        deploy_log: Vec<Event>,
        is_failed: bool,
        native_cost_evidence: Vec<u8>,
        limits: PhloWireLimits,
    ) -> Result<Self, String> {
        let result = Self {
            envelope,
            cost,
            deploy_log,
            is_failed,
            native_cost_evidence,
            evidence_limits: limits,
        };
        result.validate(limits)?;
        Ok(result)
    }

    pub fn envelope(&self) -> &DeployEnvelope { &self.envelope }

    pub fn cost(&self) -> &PCost { &self.cost }

    pub fn deploy_log(&self) -> &[Event] { &self.deploy_log }

    pub fn native_cost_evidence_bytes(&self) -> &[u8] { &self.native_cost_evidence }

    pub fn is_failed(&self) -> bool { self.is_failed }

    pub fn identity_bytes(&self) -> &[u8] { self.envelope.identity().as_bytes() }

    pub fn evidence(&self, limits: PhloWireLimits) -> Result<NativeCostEvidenceV1<'_>, String> {
        let evidence = NativeCostEvidenceV1::decode(&self.native_cost_evidence, limits)
            .map_err(|error| error.to_string())?;
        if self.envelope.format() != DeployEnvelopeFormat::OfferedFunded {
            return Err("processed funded deploy requires an offered envelope".to_string());
        }
        let DeployEnvelopeRef::OfferedFunded(signed) = self.envelope.view() else {
            unreachable!("offered format checked")
        };
        if evidence.envelope_commitment.as_slice() != self.envelope.identity().as_bytes()
            || i64::try_from(evidence.phlo_limit).ok() != Some(signed.data.phlo_limit())
            || i64::try_from(evidence.phlo_price).ok() != Some(signed.data.phlo_price())
            || evidence.phlo_used != self.cost.cost
            || (evidence.failure_class == NativeCostFailureClass::UserFailure) != self.is_failed
        {
            return Err(
                "processed funded deploy differs from its signed offer or evidence".to_string(),
            );
        }
        let protocol = offered_funded_v6_limits();
        evidence
            .decoded_sections(protocol.funding_case, protocol.prepaid_delta)
            .map_err(|error| error.to_string())?;
        let wallet =
            NativeWalletReceiptV1::decode(evidence.wallet_settlement, NativeWalletReceiptLimits {
                wire: PhloWireLimits {
                    total_bytes: limits.field_bytes,
                    field_bytes: limits.field_bytes,
                },
                payers: protocol.envelope.payload.funding.sources,
            })
            .map_err(|error| error.to_string())?;
        let resource_rev = evidence.resource_rev().map_err(|error| error.to_string())?;
        if wallet.resource_rev != resource_rev
            || wallet.fee_rev != evidence.fee_rev
            || wallet.rev_spent().map_err(|error| error.to_string())?
                != evidence.rev_spent().map_err(|error| error.to_string())?
        {
            return Err(
                "processed funded deploy wallet receipt differs from cost evidence".to_string(),
            );
        }
        Ok(evidence)
    }

    pub fn validate(&self, limits: PhloWireLimits) -> Result<(), String> {
        check_event_log(&self.deploy_log)?;
        self.evidence(limits)?;
        Ok(())
    }

    pub fn from_proto(
        proto: ProcessedDeployProto,
        envelope_limits: DeployEnvelopeLimits,
        evidence_limits: PhloWireLimits,
    ) -> Result<Self, String> {
        if !proto.system_deploy_error.is_empty() {
            return Err("processed funded deploy cannot commit a platform failure".to_string());
        }
        check_proto_event_log(&proto.deploy_log)?;
        if proto
            .native_cost_evidence
            .as_ref()
            .is_some_and(|evidence| evidence.len() > evidence_limits.total_bytes)
        {
            return Err("processed funded deploy evidence exceeds protocol limit".to_string());
        }
        let envelope = DeployEnvelope::from_processed_proto_with_limits(
            proto
                .deploy
                .ok_or_else(|| "Missing deploy field".to_string())?,
            envelope_limits,
        )?;
        let cost = proto.cost.ok_or_else(|| "Missing cost field".to_string())?;
        let deploy_log = proto
            .deploy_log
            .into_iter()
            .map(Event::from_proto)
            .collect::<Result<Vec<_>, _>>()?;
        let native_cost_evidence = proto
            .native_cost_evidence
            .ok_or_else(|| "processed funded deploy requires native cost evidence".to_string())?;
        let native_cost_evidence = native_cost_evidence.to_vec();
        Self::new(
            envelope,
            cost,
            deploy_log,
            proto.errored,
            native_cost_evidence,
            evidence_limits,
        )
    }

    pub fn to_proto_verified(&self) -> ProcessedDeployProto {
        self.validate(self.evidence_limits)
            .expect("verified offered processed deploy");
        ProcessedDeployProto {
            deploy: Some(self.envelope.to_proto().expect("verified offered envelope")),
            cost: Some(self.cost.clone()),
            deploy_log: self.deploy_log.iter().map(Event::to_proto).collect(),
            errored: self.is_failed,
            system_deploy_error: String::new(),
            native_cost_evidence: Some(self.native_cost_evidence.clone().into()),
        }
    }

    pub fn to_proto(&self, limits: PhloWireLimits) -> Result<ProcessedDeployProto, String> {
        self.validate(limits)?;
        Ok(self.to_proto_verified())
    }
}

fn produce_shape(produce: &ProduceEvent) -> Result<(usize, usize), String> {
    if produce.output_value.len() > offered_funded_v6_limits().deploy_log_items {
        return Err("processed funded deploy event items exceed protocol limit".to_string());
    }
    let items = produce
        .output_value
        .len()
        .checked_add(1)
        .ok_or_else(|| "processed funded deploy event item count overflows".to_string())?;
    let bytes = produce.output_value.iter().try_fold(
        produce
            .channels_hash
            .len()
            .checked_add(produce.hash.len())
            .ok_or_else(|| "processed funded deploy event bytes overflow".to_string())?,
        |sum, value| {
            sum.checked_add(value.len())
                .ok_or_else(|| "processed funded deploy event bytes overflow".to_string())
        },
    )?;
    Ok((items, bytes))
}

fn consume_shape(consume: &ConsumeEvent) -> Result<(usize, usize), String> {
    if consume.channels_hashes.len() > offered_funded_v6_limits().deploy_log_items {
        return Err("processed funded deploy event items exceed protocol limit".to_string());
    }
    let items = consume
        .channels_hashes
        .len()
        .checked_add(1)
        .ok_or_else(|| "processed funded deploy event item count overflows".to_string())?;
    let bytes = consume
        .channels_hashes
        .iter()
        .try_fold(consume.hash.len(), |sum, hash| sum.checked_add(hash.len()))
        .ok_or_else(|| "processed funded deploy event bytes overflow".to_string())?;
    Ok((items, bytes))
}

fn check_event_log(events: &[Event]) -> Result<(), String> {
    let limits = offered_funded_v6_limits();
    if events.len() > limits.deploy_log_events {
        return Err("processed funded deploy event count exceeds protocol limit".to_string());
    }
    let mut items = 0usize;
    let mut bytes = 0usize;
    for event in events {
        let (event_items, event_bytes) = match event {
            Event::Produce(produce) => produce_shape(produce)?,
            Event::Consume(consume) => consume_shape(consume)?,
            Event::Comm(comm) => {
                if comm.produces.len() > limits.deploy_log_items
                    || comm.peeks.len() > limits.deploy_log_items
                {
                    return Err(
                        "processed funded deploy event items exceed protocol limit".to_string()
                    );
                }
                let (mut count, mut size) = consume_shape(&comm.consume)?;
                count = count.checked_add(comm.peeks.len()).ok_or_else(|| {
                    "processed funded deploy event item count overflows".to_string()
                })?;
                if count > limits.deploy_log_items {
                    return Err(
                        "processed funded deploy event items exceed protocol limit".to_string()
                    );
                }
                for produce in &comm.produces {
                    let (next_count, next_size) = produce_shape(produce)?;
                    count = count.checked_add(next_count).ok_or_else(|| {
                        "processed funded deploy event item count overflows".to_string()
                    })?;
                    size = size.checked_add(next_size).ok_or_else(|| {
                        "processed funded deploy event bytes overflow".to_string()
                    })?;
                    if count > limits.deploy_log_items
                        || count
                            .checked_mul(64)
                            .and_then(|overhead| overhead.checked_add(size))
                            .is_none_or(|total| total > limits.deploy_log_bytes)
                    {
                        return Err(
                            "processed funded deploy event exceeds protocol limit".to_string()
                        );
                    }
                }
                (count, size)
            }
        };
        items = items
            .checked_add(event_items)
            .filter(|count| *count <= limits.deploy_log_items)
            .ok_or_else(|| {
                "processed funded deploy event items exceed protocol limit".to_string()
            })?;
        bytes = bytes
            .checked_add(event_bytes)
            .and_then(|count| count.checked_add(event_items.checked_mul(64)?))
            .filter(|count| *count <= limits.deploy_log_bytes)
            .ok_or_else(|| {
                "processed funded deploy event bytes exceed protocol limit".to_string()
            })?;
    }
    Ok(())
}

fn check_proto_event_log(events: &[EventProto]) -> Result<(), String> {
    let limits = offered_funded_v6_limits();
    if events.len() > limits.deploy_log_events {
        return Err("processed funded deploy event count exceeds protocol limit".to_string());
    }
    let mut items = 0usize;
    let mut bytes = 0usize;
    for event in events {
        let (event_items, event_payload) = match &event.event_instance {
            Some(event_proto::EventInstance::Produce(produce)) => {
                if produce.output_value.len() >= limits.deploy_log_items {
                    return Err(
                        "processed funded deploy event items exceed protocol limit".to_string()
                    );
                }
                let items =
                    produce.output_value.len().checked_add(1).ok_or_else(|| {
                        "processed funded deploy event items overflow".to_string()
                    })?;
                let payload = produce
                    .output_value
                    .iter()
                    .try_fold(
                        produce
                            .channels_hash
                            .len()
                            .checked_add(produce.hash.len())
                            .ok_or_else(|| {
                                "processed funded deploy event bytes overflow".to_string()
                            })?,
                        |size, value| size.checked_add(value.len()),
                    )
                    .ok_or_else(|| "processed funded deploy event bytes overflow".to_string())?;
                (items, payload)
            }
            Some(event_proto::EventInstance::Consume(consume)) => {
                if consume.channels_hashes.len() >= limits.deploy_log_items {
                    return Err(
                        "processed funded deploy event items exceed protocol limit".to_string()
                    );
                }
                let items = consume
                    .channels_hashes
                    .len()
                    .checked_add(1)
                    .ok_or_else(|| "processed funded deploy event items overflow".to_string())?;
                let payload = consume
                    .channels_hashes
                    .iter()
                    .try_fold(consume.hash.len(), |size, hash| {
                        size.checked_add(hash.len())
                    })
                    .ok_or_else(|| "processed funded deploy event bytes overflow".to_string())?;
                (items, payload)
            }
            Some(event_proto::EventInstance::Comm(comm)) => {
                let consume = comm
                    .consume
                    .as_ref()
                    .ok_or_else(|| "processed funded COMM lacks consume".to_string())?;
                if consume.channels_hashes.len() >= limits.deploy_log_items
                    || comm.produces.len() > limits.deploy_log_items
                    || comm.peeks.len() > limits.deploy_log_items
                {
                    return Err(
                        "processed funded deploy event items exceed protocol limit".to_string()
                    );
                }
                let mut count = consume
                    .channels_hashes
                    .len()
                    .checked_add(1)
                    .and_then(|count| count.checked_add(comm.peeks.len()))
                    .ok_or_else(|| "processed funded deploy event items overflow".to_string())?;
                let mut payload = consume
                    .channels_hashes
                    .iter()
                    .try_fold(consume.hash.len(), |size, hash| {
                        size.checked_add(hash.len())
                    })
                    .ok_or_else(|| "processed funded deploy event bytes overflow".to_string())?;
                if count > limits.deploy_log_items {
                    return Err(
                        "processed funded deploy event items exceed protocol limit".to_string()
                    );
                }
                for produce in &comm.produces {
                    if produce.output_value.len() >= limits.deploy_log_items {
                        return Err(
                            "processed funded deploy event items exceed protocol limit".to_string()
                        );
                    }
                    count = count
                        .checked_add(produce.output_value.len().checked_add(1).ok_or_else(
                            || "processed funded deploy event items overflow".to_string(),
                        )?)
                        .ok_or_else(|| {
                            "processed funded deploy event items overflow".to_string()
                        })?;
                    payload = produce
                        .output_value
                        .iter()
                        .try_fold(
                            payload
                                .checked_add(produce.channels_hash.len())
                                .and_then(|size| size.checked_add(produce.hash.len()))
                                .ok_or_else(|| {
                                    "processed funded deploy event bytes overflow".to_string()
                                })?,
                            |size, value| size.checked_add(value.len()),
                        )
                        .ok_or_else(|| {
                            "processed funded deploy event bytes overflow".to_string()
                        })?;
                    if count > limits.deploy_log_items
                        || count
                            .checked_mul(64)
                            .and_then(|overhead| overhead.checked_add(payload))
                            .is_none_or(|total| total > limits.deploy_log_bytes)
                    {
                        return Err(
                            "processed funded deploy event exceeds protocol limit".to_string()
                        );
                    }
                }
                (count, payload)
            }
            None => return Err("processed funded deploy has malformed event".to_string()),
        };
        items = items
            .checked_add(event_items)
            .filter(|count| *count <= limits.deploy_log_items)
            .ok_or_else(|| {
                "processed funded deploy event items exceed protocol limit".to_string()
            })?;
        bytes = bytes
            .checked_add(event_payload)
            .and_then(|count| count.checked_add(event_items.checked_mul(64)?))
            .filter(|count| *count <= limits.deploy_log_bytes)
            .ok_or_else(|| {
                "processed funded deploy event bytes exceed protocol limit".to_string()
            })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offered_log_rejects_large_payload_before_processed_record_construction() {
        let limit = offered_funded_v6_limits().deploy_log_bytes;
        let event = Event::Produce(ProduceEvent {
            channels_hash: vec![1; 32].into(),
            hash: vec![2; 32].into(),
            persistent: false,
            times_repeated: 0,
            is_deterministic: true,
            output_value: vec![vec![3; limit].into()],
            failed: false,
        });
        assert!(check_event_log(&[event]).is_err());
    }

    #[test]
    fn offered_proto_log_rejects_large_payload_before_model_conversion() {
        let limit = offered_funded_v6_limits().deploy_log_bytes;
        let produce = crate::casper::ProduceEventProto {
            output_value: vec![vec![3; limit].into()],
            ..Default::default()
        };
        let event = EventProto {
            event_instance: Some(event_proto::EventInstance::Produce(produce)),
        };
        assert!(check_proto_event_log(&[event]).is_err());
    }
}
