// See casper/src/main/scala/coop/rchain/casper/util/EventConverter.scala

use std::sync::Arc;

use models::rust::casper::protocol::casper_message::{
    CommEvent, ConsumeEvent, Event, Peek, ProduceEvent, RestingCommEvent,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::EventCodecEpoch;
use rspace_plus_plus::rspace::trace::event::{
    Consume, Event as RspaceEvent, IOEvent, Produce, RestingCOMM, COMM as RspaceComm,
};

use crate::rust::errors::CasperError;

/// Legacy conversion is intentionally unable to authorize a CostV2-only
/// RestingCOMM. Production conversion supplies the adopted epoch below.
pub fn to_casper_event(event: RspaceEvent) -> Result<Event, CasperError> {
    to_casper_event_for_epoch(event, EventCodecEpoch::Legacy)
}

/// The authenticated epoch, not the event payload, chooses whether a resting
/// rendezvous may enter the deploy log.
pub fn to_casper_event_for_epoch(
    event: RspaceEvent,
    epoch: EventCodecEpoch,
) -> Result<Event, CasperError> {
    Ok(match event {
        RspaceEvent::Comm(RspaceComm {
            consume,
            produces,
            peeks,
            times_repeated,
        }) => Event::Comm(CommEvent {
            consume: ConsumeEvent {
                channels_hashes: consume
                    .channel_hashes
                    .iter()
                    .map(|h| h.to_bytes_prost())
                    .collect(),
                hash: consume.hash.to_bytes_prost(),
                persistent: consume.persistent,
            },
            produces: produces
                .into_iter()
                .map(|p| ProduceEvent {
                    channels_hash: p.channel_hash.to_bytes_prost(),
                    hash: p.hash.to_bytes_prost(),
                    persistent: p.persistent,
                    times_repeated: *times_repeated.get(&p).unwrap_or(&0),
                    is_deterministic: p.is_deterministic,
                    output_value: p.output_value.clone().into_iter().map(Into::into).collect(),
                    failed: p.failed,
                })
                .collect(),
            peeks: peeks.iter().map(|p| Peek { channel_index: *p }).collect(),
        }),

        RspaceEvent::IoEvent(ioevent) => match ioevent {
            IOEvent::Produce(produce) => Event::Produce(ProduceEvent {
                channels_hash: produce.channel_hash.to_bytes_prost(),
                hash: produce.hash.to_bytes_prost(),
                persistent: produce.persistent,
                times_repeated: 0,
                is_deterministic: produce.is_deterministic,
                output_value: produce.output_value.into_iter().map(Into::into).collect(),
                failed: produce.failed,
            }),

            IOEvent::Consume(consume) => Event::Consume(ConsumeEvent {
                channels_hashes: consume
                    .channel_hashes
                    .iter()
                    .map(|h| h.to_bytes_prost())
                    .collect(),
                hash: consume.hash.to_bytes_prost(),
                persistent: consume.persistent,
            }),
        },
        RspaceEvent::RestingComm(resting) => {
            if epoch != EventCodecEpoch::CostV2 {
                return Err(CasperError::RuntimeError(
                    "RestingCOMM requires the versioned CostV2 deploy-log codec".to_string(),
                ));
            }
            let event = RestingCommEvent {
                consume: ConsumeEvent {
                    channels_hashes: resting
                        .consume
                        .channel_hashes
                        .iter()
                        .map(|h| h.to_bytes_prost())
                        .collect(),
                    hash: resting.consume.hash.to_bytes_prost(),
                    persistent: resting.consume.persistent,
                },
                continuation_index: u64::try_from(resting.continuation_index).map_err(|_| {
                    CasperError::RuntimeError("resting continuation index is negative".to_string())
                })?,
                ordered_produces: resting
                    .ordered_produces
                    .iter()
                    .map(|p| ProduceEvent {
                        channels_hash: p.channel_hash.to_bytes_prost(),
                        hash: p.hash.to_bytes_prost(),
                        persistent: p.persistent,
                        times_repeated: 0,
                        is_deterministic: p.is_deterministic,
                        output_value: p.output_value.iter().cloned().map(Into::into).collect(),
                        failed: p.failed,
                    })
                    .collect(),
                ordered_data_indices: resting
                    .ordered_data_indices
                    .iter()
                    .map(|index| {
                        u64::try_from(*index).map_err(|_| {
                            CasperError::RuntimeError("resting data index is negative".to_string())
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                peeks: resting
                    .peeks
                    .iter()
                    .map(|index| Peek {
                        channel_index: *index,
                    })
                    .collect(),
            };
            event.validate().map_err(CasperError::RuntimeError)?;
            Event::RestingComm(event)
        }
    })
}

pub fn to_rspace_event(event: &Event) -> Result<RspaceEvent, CasperError> {
    to_rspace_event_for_epoch(event, EventCodecEpoch::Legacy)
}

pub fn to_rspace_event_for_epoch(
    event: &Event,
    epoch: EventCodecEpoch,
) -> Result<RspaceEvent, CasperError> {
    Ok(match event {
        Event::Produce(produce_event) => RspaceEvent::IoEvent(IOEvent::Produce(Produce {
            channel_hash: Blake2b256Hash::from_bytes_prost(&produce_event.channels_hash),
            hash: Blake2b256Hash::from_bytes_prost(&produce_event.hash),
            persistent: produce_event.persistent,
            is_deterministic: produce_event.is_deterministic,
            output_value: produce_event
                .output_value
                .clone()
                .into_iter()
                .map(|v| v.into())
                .collect(),
            failed: produce_event.failed,
        })),

        Event::Consume(consume_event) => RspaceEvent::IoEvent(IOEvent::Consume(Consume {
            channel_hashes: consume_event
                .channels_hashes
                .iter()
                .map(|h| Blake2b256Hash::from_bytes_prost(h))
                .collect(),
            hash: Blake2b256Hash::from_bytes_prost(&consume_event.hash),
            persistent: consume_event.persistent,
        })),

        Event::Comm(comm_event) => {
            let rspace_consume = Consume {
                channel_hashes: comm_event
                    .consume
                    .channels_hashes
                    .iter()
                    .map(|h| Blake2b256Hash::from_bytes_prost(h))
                    .collect(),
                hash: Blake2b256Hash::from_bytes_prost(&comm_event.consume.hash),
                persistent: comm_event.consume.persistent,
            };

            let mut produces = Vec::new();
            let mut times_repeated = std::collections::BTreeMap::new();

            for produce in &comm_event.produces {
                let rspace_produce = Produce {
                    channel_hash: Blake2b256Hash::from_bytes_prost(&produce.channels_hash),
                    hash: Blake2b256Hash::from_bytes_prost(&produce.hash),
                    persistent: produce.persistent,
                    is_deterministic: produce.is_deterministic,
                    output_value: produce
                        .output_value
                        .clone()
                        .into_iter()
                        .map(Into::into)
                        .collect(),
                    failed: produce.failed,
                };
                times_repeated.insert(rspace_produce.clone(), produce.times_repeated);
                produces.push(rspace_produce);
            }

            produces.sort_by(|a, b| {
                a.channel_hash
                    .cmp(&b.channel_hash)
                    .then_with(|| a.hash.cmp(&b.hash))
                    .then_with(|| a.persistent.cmp(&b.persistent))
            });

            let peeks: std::collections::BTreeSet<_> =
                comm_event.peeks.iter().map(|p| p.channel_index).collect();

            RspaceEvent::Comm(RspaceComm {
                consume: rspace_consume,
                produces,
                peeks,
                times_repeated,
            })
        }
        Event::RestingComm(resting) => {
            if epoch != EventCodecEpoch::CostV2 {
                return Err(CasperError::RuntimeError(
                    "RestingCOMM requires the versioned CostV2 deploy-log codec".to_string(),
                ));
            }
            resting.validate().map_err(CasperError::RuntimeError)?;
            let continuation_index = i32::try_from(resting.continuation_index).map_err(|_| {
                CasperError::RuntimeError("resting continuation index overflows".to_string())
            })?;
            let ordered_data_indices = resting
                .ordered_data_indices
                .iter()
                .map(|index| {
                    i32::try_from(*index).map_err(|_| {
                        CasperError::RuntimeError("resting data index overflows".to_string())
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            RspaceEvent::RestingComm(Arc::new(RestingCOMM {
                consume: Consume {
                    channel_hashes: resting
                        .consume
                        .channels_hashes
                        .iter()
                        .map(Blake2b256Hash::from_bytes_prost)
                        .collect(),
                    hash: Blake2b256Hash::from_bytes_prost(&resting.consume.hash),
                    persistent: resting.consume.persistent,
                },
                continuation_index,
                ordered_produces: resting
                    .ordered_produces
                    .iter()
                    .map(|p| Produce {
                        channel_hash: Blake2b256Hash::from_bytes_prost(&p.channels_hash),
                        hash: Blake2b256Hash::from_bytes_prost(&p.hash),
                        persistent: p.persistent,
                        is_deterministic: p.is_deterministic,
                        output_value: p.output_value.iter().cloned().map(Into::into).collect(),
                        failed: p.failed,
                    })
                    .collect(),
                ordered_data_indices,
                peeks: resting
                    .peeks
                    .iter()
                    .map(|peek| peek.channel_index)
                    .collect(),
            }))
        }
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use prost::Message;
    use rspace_plus_plus::rspace::trace::event::RestingCOMM;

    use super::*;

    #[test]
    fn legacy_converter_refuses_resting_without_aliasing_comm() {
        let event = RspaceEvent::RestingComm(Arc::new(RestingCOMM {
            consume: Consume {
                channel_hashes: Vec::new(),
                hash: Blake2b256Hash::new(b"resting-waiter"),
                persistent: true,
            },
            continuation_index: 0,
            ordered_produces: Vec::new(),
            ordered_data_indices: Vec::new(),
            peeks: BTreeSet::new(),
        }));
        assert!(matches!(
            to_casper_event(event),
            Err(CasperError::RuntimeError(message)) if message.contains("versioned CostV2")
        ));
    }

    #[test]
    fn ordinary_produce_keeps_its_conversion_identity() {
        let produce = Produce::new(
            Blake2b256Hash::new(b"channel"),
            Blake2b256Hash::new(b"source"),
            false,
        );
        let original = RspaceEvent::IoEvent(IOEvent::Produce(produce));
        let encoded = to_casper_event(original.clone()).unwrap();
        assert_eq!(to_rspace_event(&encoded).unwrap(), original);
    }

    #[test]
    fn cost_v2_resting_roundtrip_preserves_ordered_store_address() {
        let channel = Blake2b256Hash::new(b"repeated-channel");
        let source = Produce::new(channel.clone(), Blake2b256Hash::new(b"datum"), false);
        let consume = Consume {
            channel_hashes: vec![channel.clone(), channel],
            hash: Blake2b256Hash::new(b"waiter"),
            persistent: true,
        };
        let resting = |indices: Vec<i32>| {
            RspaceEvent::RestingComm(Arc::new(RestingCOMM {
                consume: consume.clone(),
                continuation_index: 3,
                ordered_produces: vec![source.clone(), source.clone()],
                ordered_data_indices: indices,
                peeks: BTreeSet::from([1]),
            }))
        };
        let forward = resting(vec![1, 2]);
        let reversed = resting(vec![2, 1]);
        let encoded = to_casper_event_for_epoch(forward.clone(), EventCodecEpoch::CostV2).unwrap();
        assert_eq!(
            to_rspace_event_for_epoch(&encoded, EventCodecEpoch::CostV2).unwrap(),
            forward
        );
        assert_ne!(
            encoded.to_proto().encode_to_vec(),
            to_casper_event_for_epoch(reversed, EventCodecEpoch::CostV2)
                .unwrap()
                .to_proto()
                .encode_to_vec()
        );
        assert!(to_rspace_event_for_epoch(&encoded, EventCodecEpoch::CostV1).is_err());
    }
}
