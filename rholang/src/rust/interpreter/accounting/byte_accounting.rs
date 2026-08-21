use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use prost::Message;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce, COMM};
use thiserror::Error;

const PRODUCE_INTRODUCTION_DOMAIN: &[u8] = b"f1r3node:byte-accounting:produce-introduction:v1";
const CONSUME_INTRODUCTION_DOMAIN: &[u8] = b"f1r3node:byte-accounting:consume-introduction:v1";
const HASH_BYTES: u64 = 32;
const SCHEDULE_DOMAIN: &[u8] = b"f1r3node:byte-accounting:schedule:v1";
pub const BYTE_COST_SCHEDULE_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteCostSchedule {
    pub introduction_rate: u64,
    pub transfer_rate: u64,
    pub trace_rate: u64,
}

pub const BYTE_COST_SCHEDULE_V1: ByteCostSchedule = ByteCostSchedule {
    introduction_rate: 1,
    transfer_rate: 1,
    trace_rate: 1,
};

pub fn byte_cost_schedule_digest() -> [u8; 32] {
    Blake2b256::hash_stream(|update| {
        update(SCHEDULE_DOMAIN);
        update(&BYTE_COST_SCHEDULE_VERSION.to_le_bytes());
        update(&BYTE_COST_SCHEDULE_V1.introduction_rate.to_le_bytes());
        update(&BYTE_COST_SCHEDULE_V1.transfer_rate.to_le_bytes());
        update(&BYTE_COST_SCHEDULE_V1.trace_rate.to_le_bytes());
    })
    .try_into()
    .expect("Blake2b-256 digest length")
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ByteCharge {
    pub introduction_bytes: u64,
    pub transfer_bytes: u64,
    pub trace_bytes: u64,
}

impl ByteCharge {
    pub fn cost(self, schedule: ByteCostSchedule) -> Result<u64, ByteAccountingError> {
        self.introduction_bytes
            .checked_mul(schedule.introduction_rate)
            .and_then(|introduction| {
                self.transfer_bytes
                    .checked_mul(schedule.transfer_rate)
                    .and_then(|transfer| introduction.checked_add(transfer))
            })
            .and_then(|subtotal| {
                self.trace_bytes
                    .checked_mul(schedule.trace_rate)
                    .and_then(|trace| subtotal.checked_add(trace))
            })
            .ok_or(ByteAccountingError::Overflow)
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ByteAccountingError {
    #[error("canonical byte accounting overflow")]
    Overflow,
}

fn message_bytes<M: Message>(message: &M) -> Result<u64, ByteAccountingError> {
    u64::try_from(message.encoded_len()).map_err(|_| ByteAccountingError::Overflow)
}

fn sum_message_bytes<M: Message>(messages: &[M]) -> Result<u64, ByteAccountingError> {
    messages.iter().try_fold(0_u64, |total, message| {
        total
            .checked_add(message_bytes(message)?)
            .ok_or(ByteAccountingError::Overflow)
    })
}

fn event_storage_bytes(channels: usize) -> Result<u64, ByteAccountingError> {
    let channels = u64::try_from(channels).map_err(|_| ByteAccountingError::Overflow)?;
    HASH_BYTES
        .checked_add(
            channels
                .checked_mul(HASH_BYTES)
                .ok_or(ByteAccountingError::Overflow)?,
        )
        .ok_or(ByteAccountingError::Overflow)
}

fn domain_identity(domain: &[u8], source_hash: &[u8]) -> [u8; 32] {
    Blake2b256::hash_stream(|update| {
        update(domain);
        update(source_hash);
    })
    .try_into()
    .expect("Blake2b-256 digest length")
}

pub fn produce_introduction_identity(source: &Produce) -> [u8; 32] {
    domain_identity(PRODUCE_INTRODUCTION_DOMAIN, &source.hash.bytes())
}

pub fn consume_introduction_identity(source: &Consume) -> [u8; 32] {
    domain_identity(CONSUME_INTRODUCTION_DOMAIN, &source.hash.bytes())
}

pub fn produce_introduction_charge(
    channel: &Par,
    data: &ListParWithRandom,
) -> Result<ByteCharge, ByteAccountingError> {
    let introduction_bytes = message_bytes(channel)?
        .checked_add(message_bytes(data)?)
        .and_then(|bytes| bytes.checked_add(HASH_BYTES.checked_mul(2)?))
        .ok_or(ByteAccountingError::Overflow)?;
    Ok(ByteCharge {
        introduction_bytes,
        ..ByteCharge::default()
    })
}

pub fn consume_introduction_charge(
    channels: &[Par],
    patterns: &[BindPattern],
    continuation: &TaggedContinuation,
) -> Result<ByteCharge, ByteAccountingError> {
    let introduction_bytes = sum_message_bytes(channels)?
        .checked_add(sum_message_bytes(patterns)?)
        .and_then(|bytes| bytes.checked_add(message_bytes(continuation).ok()?))
        .and_then(|bytes| bytes.checked_add(event_storage_bytes(channels.len()).ok()?))
        .ok_or(ByteAccountingError::Overflow)?;
    Ok(ByteCharge {
        introduction_bytes,
        ..ByteCharge::default()
    })
}

pub fn comm_charge(
    comm: &COMM,
    data: &[(&ListParWithRandom, bool)],
) -> Result<ByteCharge, ByteAccountingError> {
    let transfer_bytes = data.iter().try_fold(0_u64, |total, (datum, _)| {
        total
            .checked_add(message_bytes(*datum)?)
            .ok_or(ByteAccountingError::Overflow)
    })?;
    let channel_count = comm.consume.channel_hashes.len();
    let trace_bytes = event_storage_bytes(channel_count)?
        .checked_add(
            u64::try_from(channel_count)
                .map_err(|_| ByteAccountingError::Overflow)?
                .checked_mul(event_storage_bytes(1)?)
                .ok_or(ByteAccountingError::Overflow)?,
        )
        .ok_or(ByteAccountingError::Overflow)?;
    Ok(ByteCharge {
        introduction_bytes: 0,
        transfer_bytes,
        trace_bytes,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use proptest::prelude::*;

    use super::*;

    fn produce() -> Produce { Produce::create(&"channel", &"datum", false) }

    fn consume(channel_count: usize) -> Consume {
        Consume::create(
            &(0..channel_count).collect::<Vec<_>>(),
            &vec![0_u8; channel_count],
            &"continuation",
            false,
        )
    }

    #[test]
    fn schedule_digest_is_versioned_and_stable() {
        assert_eq!(
            hex::encode(byte_cost_schedule_digest()),
            "20f7da72457c462469ffb9e9d476e203b1395cada72bf102d8af484c32a4840c"
        );
    }

    #[test]
    fn introduction_identities_are_operation_separated() {
        let produce = produce();
        let consume = consume(1);

        assert_eq!(
            produce_introduction_identity(&produce),
            produce_introduction_identity(&produce)
        );
        assert_ne!(
            produce_introduction_identity(&produce),
            consume_introduction_identity(&consume)
        );
    }

    #[test]
    fn checked_cost_rejects_every_overflow_position() {
        for charge in [
            ByteCharge {
                introduction_bytes: u64::MAX,
                ..ByteCharge::default()
            },
            ByteCharge {
                transfer_bytes: u64::MAX,
                ..ByteCharge::default()
            },
            ByteCharge {
                trace_bytes: u64::MAX,
                ..ByteCharge::default()
            },
        ] {
            assert_eq!(
                charge.cost(ByteCostSchedule {
                    introduction_rate: 2,
                    transfer_rate: 2,
                    trace_rate: 2,
                }),
                Err(ByteAccountingError::Overflow)
            );
        }
    }

    proptest! {
        #[test]
        fn produce_introduction_is_exact_canonical_footprint(
            random_state in prop::collection::vec(any::<u8>(), 0..2048),
        ) {
            let channel = Par::default();
            let data = ListParWithRandom {
                random_state,
                ..ListParWithRandom::default()
            };
            let charge = produce_introduction_charge(&channel, &data).unwrap();
            prop_assert_eq!(
                charge.introduction_bytes,
                u64::try_from(channel.encoded_len() + data.encoded_len()).unwrap() + 64
            );
            prop_assert_eq!(charge.cost(BYTE_COST_SCHEDULE_V1).unwrap(), charge.introduction_bytes);
        }

        #[test]
        fn consume_introduction_is_exact_canonical_footprint(channel_count in 1usize..33) {
            let channels = vec![Par::default(); channel_count];
            let patterns = vec![BindPattern::default(); channel_count];
            let continuation = TaggedContinuation::default();
            let charge = consume_introduction_charge(&channels, &patterns, &continuation).unwrap();
            let messages = channels.iter().map(Message::encoded_len).sum::<usize>()
                + patterns.iter().map(Message::encoded_len).sum::<usize>()
                + continuation.encoded_len();
            prop_assert_eq!(
                charge.introduction_bytes,
                u64::try_from(messages).unwrap() + 32 + 32 * u64::try_from(channel_count).unwrap()
            );
        }

        #[test]
        fn comm_charges_every_payload_and_join_trace_byte(
            channel_count in 1usize..33,
            payload_sizes in prop::collection::vec(0usize..1024, 1..33),
        ) {
            let data = payload_sizes
                .iter()
                .map(|size| ListParWithRandom {
                    random_state: vec![7; *size],
                    ..ListParWithRandom::default()
                })
                .collect::<Vec<_>>();
            let data_refs = data.iter().map(|datum| (datum, false)).collect::<Vec<_>>();
            let comm = COMM {
                consume: consume(channel_count),
                produces: vec![produce(); channel_count],
                peeks: BTreeSet::new(),
                times_repeated: BTreeMap::new(),
            };
            let charge = comm_charge(&comm, &data_refs).unwrap();
            prop_assert_eq!(
                charge.transfer_bytes,
                u64::try_from(data.iter().map(Message::encoded_len).sum::<usize>()).unwrap()
            );
            prop_assert_eq!(
                charge.trace_bytes,
                32 + 96 * u64::try_from(channel_count).unwrap()
            );
        }

        #[test]
        fn complete_interaction_cost_is_arrival_order_independent(
            producer_random_state in prop::collection::vec(any::<u8>(), 0..2048),
            channel_count in 1usize..33,
        ) {
            let channels = vec![Par::default(); channel_count];
            let patterns = vec![BindPattern::default(); channel_count];
            let continuation = TaggedContinuation::default();
            let data = ListParWithRandom {
                random_state: producer_random_state,
                ..ListParWithRandom::default()
            };
            let produce_charge = produce_introduction_charge(&channels[0], &data).unwrap();
            let consume_charge =
                consume_introduction_charge(&channels, &patterns, &continuation).unwrap();
            let comm = COMM {
                consume: consume(channel_count),
                produces: vec![produce(); channel_count],
                peeks: BTreeSet::new(),
                times_repeated: BTreeMap::new(),
            };
            let comm_charge = comm_charge(&comm, &[(&data, false)]).unwrap();
            let producer_first = produce_charge
                .cost(BYTE_COST_SCHEDULE_V1)
                .unwrap()
                .checked_add(consume_charge.cost(BYTE_COST_SCHEDULE_V1).unwrap())
                .unwrap()
                .checked_add(comm_charge.cost(BYTE_COST_SCHEDULE_V1).unwrap())
                .unwrap();
            let consumer_first = consume_charge
                .cost(BYTE_COST_SCHEDULE_V1)
                .unwrap()
                .checked_add(produce_charge.cost(BYTE_COST_SCHEDULE_V1).unwrap())
                .unwrap()
                .checked_add(comm_charge.cost(BYTE_COST_SCHEDULE_V1).unwrap())
                .unwrap();
            prop_assert_eq!(producer_first, consumer_first);
        }
    }
}
