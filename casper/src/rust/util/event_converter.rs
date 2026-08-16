// See casper/src/main/scala/coop/rchain/casper/util/EventConverter.scala

use models::rust::casper::protocol::casper_message::{
    CommEvent, ConsumeEvent, Event, Peek, ProduceEvent,
};
use prost::Message;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::trace::event::{
    Consume, Event as RspaceEvent, IOEvent, Produce, COMM as RspaceComm,
};

pub fn to_casper_event(event: RspaceEvent) -> Event {
    match event {
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
    }
}

pub fn canonicalize_casper_events(events: &mut [Event]) {
    events.sort_by_cached_key(|event| event.to_proto().encode_to_vec());
}

pub fn to_rspace_event(event: &Event) -> RspaceEvent {
    match event {
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
    }
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;

    fn produce(value: u8) -> Event {
        Event::Produce(ProduceEvent {
            channels_hash: Bytes::from(vec![value]),
            hash: Bytes::from(vec![value.wrapping_add(1)]),
            persistent: false,
            times_repeated: 0,
            is_deterministic: true,
            output_value: vec![Bytes::from(vec![value])],
            failed: false,
        })
    }

    #[test]
    fn canonical_event_order_is_permutation_invariant_and_idempotent() {
        let mut forward = vec![produce(1), produce(2), produce(3)];
        let mut reverse = forward.iter().cloned().rev().collect::<Vec<_>>();

        canonicalize_casper_events(&mut forward);
        canonicalize_casper_events(&mut reverse);
        let once = forward.clone();
        canonicalize_casper_events(&mut forward);

        assert_eq!(forward, reverse);
        assert_eq!(forward, once);
    }

    #[test]
    fn canonical_event_order_preserves_multiplicity() {
        let mut events = vec![produce(2), produce(1), produce(2)];
        canonicalize_casper_events(&mut events);

        assert_eq!(
            events.iter().filter(|event| **event == produce(2)).count(),
            2
        );
        assert_eq!(
            events.iter().filter(|event| **event == produce(1)).count(),
            1
        );
    }

    #[test]
    fn recorded_removal_round_trips_through_the_consensus_event_format() {
        let channel = b"settlement-channel".to_vec();
        let source = Produce::create(&channel, &b"stored-stack".to_vec(), false);
        let (consume, comm) =
            rspace_plus_plus::rspace::trace::event::recorded_removal(&channel, &source, b"stack-1");
        for event in [
            RspaceEvent::IoEvent(IOEvent::Consume(consume)),
            RspaceEvent::Comm(comm),
        ] {
            assert_eq!(to_rspace_event(&to_casper_event(event.clone())), event);
        }
    }
}
