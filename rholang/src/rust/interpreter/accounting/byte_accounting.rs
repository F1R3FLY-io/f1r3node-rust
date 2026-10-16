use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use prost::Message;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce, RestingCOMM, COMM};
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

fn domain_identity_metered(
    domain: &[u8],
    source_hash: &[u8],
    meter: &dyn SourceMeter,
) -> Result<[u8; 32], RSpaceError> {
    let scanned = domain
        .len()
        .checked_add(source_hash.len())
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve(1, scanned, 32)?;
    Ok(domain_identity(domain, source_hash))
}

pub fn produce_introduction_identity(source: &Produce) -> [u8; 32] {
    domain_identity(PRODUCE_INTRODUCTION_DOMAIN, &source.hash.0)
}

pub(crate) fn produce_introduction_identity_metered(
    source: &Produce,
    meter: &dyn SourceMeter,
) -> Result<[u8; 32], RSpaceError> {
    domain_identity_metered(PRODUCE_INTRODUCTION_DOMAIN, &source.hash.0, meter)
}

pub fn consume_introduction_identity(source: &Consume) -> [u8; 32] {
    domain_identity(CONSUME_INTRODUCTION_DOMAIN, &source.hash.0)
}

pub(crate) fn consume_introduction_identity_metered(
    source: &Consume,
    meter: &dyn SourceMeter,
) -> Result<[u8; 32], RSpaceError> {
    domain_identity_metered(CONSUME_INTRODUCTION_DOMAIN, &source.hash.0, meter)
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

// CostV2's RestingComm has a separate protobuf arm. These are the proto3
// encoded-size laws for its *EventProto* oneof arm (tag 4), including each
// nested message and the packed repeated uint64 data-index field. They inspect
// the RSpace source directly: no temporary Casper event is constructed, and
// no encoding/allocation occurs before the host meter has reserved the work.
// Historical COMM keeps `comm_charge` and its original byte count unchanged.
fn proto_varint_len(value: u64) -> usize { ((64 - value.leading_zeros() as usize).max(1) + 6) / 7 }

fn proto_len_delimited(payload: usize) -> Result<usize, RSpaceError> {
    let length = u64::try_from(payload).map_err(|_| RSpaceError::HostWorkRejected)?;
    1_usize
        .checked_add(proto_varint_len(length))
        .and_then(|n| n.checked_add(payload))
        .ok_or(RSpaceError::HostWorkRejected)
}

fn proto_add(total: &mut usize, additional: usize) -> Result<(), RSpaceError> {
    *total = total
        .checked_add(additional)
        .ok_or(RSpaceError::HostWorkRejected)?;
    Ok(())
}

fn proto_bytes_field(
    length: usize,
    repeated: bool,
    meter: &dyn SourceMeter,
) -> Result<usize, RSpaceError> {
    meter.reserve(1, length, 0)?;
    if length == 0 && !repeated {
        Ok(0)
    } else {
        proto_len_delimited(length)
    }
}

fn proto_consume_len(source: &Consume, meter: &dyn SourceMeter) -> Result<usize, RSpaceError> {
    meter.reserve(1, 0, 0)?;
    let mut length = 0;
    for channel in &source.channel_hashes {
        if channel.0.len() != 32 {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        proto_add(
            &mut length,
            proto_bytes_field(channel.0.len(), true, meter)?,
        )?;
    }
    if source.hash.0.len() != 32 {
        return Err(RSpaceError::ProduceCommitProtocolViolation);
    }
    proto_add(
        &mut length,
        proto_bytes_field(source.hash.0.len(), false, meter)?,
    )?;
    if source.persistent {
        proto_add(&mut length, 2)?;
    }
    Ok(length)
}

fn proto_produce_len(source: &Produce, meter: &dyn SourceMeter) -> Result<usize, RSpaceError> {
    meter.reserve(1, 0, 0)?;
    if source.channel_hash.0.len() != 32 || source.hash.0.len() != 32 {
        return Err(RSpaceError::ProduceCommitProtocolViolation);
    }
    let mut length = proto_bytes_field(source.channel_hash.0.len(), false, meter)?;
    proto_add(
        &mut length,
        proto_bytes_field(source.hash.0.len(), false, meter)?,
    )?;
    if source.persistent {
        proto_add(&mut length, 2)?;
    }
    if source.is_deterministic {
        proto_add(&mut length, 2)?;
    }
    for output in &source.output_value {
        proto_add(&mut length, proto_bytes_field(output.len(), true, meter)?)?;
    }
    if source.failed {
        proto_add(&mut length, 2)?;
    }
    Ok(length)
}

/// Exact canonical v7 `EventProto.restingComm` bytes, paid for as trace bytes.
/// The checked nonnegative i32 store addresses must round-trip through the
/// Casper uint64 fields; neither ordered roster may be sorted or deduplicated.
pub fn resting_comm_charge_metered(
    event: &RestingCOMM,
    data: &[(&ListParWithRandom, bool)],
    meter: &dyn SourceMeter,
) -> Result<ByteCharge, RSpaceError> {
    meter.reserve(1, 0, 0)?;
    let count = event.ordered_produces.len();
    if event.consume.channel_hashes.len() != count
        || event.ordered_data_indices.len() != count
        || data.len() != count
        || event.continuation_index < 0
    {
        return Err(RSpaceError::ProduceCommitProtocolViolation);
    }
    let consume_len = proto_consume_len(&event.consume, meter)?;
    let mut resting_len = proto_len_delimited(consume_len)?;
    let continuation_index = u64::try_from(event.continuation_index)
        .map_err(|_| RSpaceError::ProduceCommitProtocolViolation)?;
    if continuation_index != 0 {
        proto_add(&mut resting_len, 1 + proto_varint_len(continuation_index))?;
    }
    for (channel, source) in event
        .consume
        .channel_hashes
        .iter()
        .zip(&event.ordered_produces)
    {
        meter.reserve(1, 64, 0)?;
        if channel.0.as_slice() != source.channel_hash.0.as_slice() {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        let produce_len = proto_produce_len(source, meter)?;
        proto_add(&mut resting_len, proto_len_delimited(produce_len)?)?;
    }
    let mut packed_index_len = 0;
    for index in &event.ordered_data_indices {
        meter.reserve(1, std::mem::size_of::<i32>(), 0)?;
        let index =
            u64::try_from(*index).map_err(|_| RSpaceError::ProduceCommitProtocolViolation)?;
        proto_add(&mut packed_index_len, proto_varint_len(index))?;
    }
    if packed_index_len != 0 {
        proto_add(&mut resting_len, proto_len_delimited(packed_index_len)?)?;
    }
    for index in &event.peeks {
        meter.reserve(1, std::mem::size_of::<i32>(), 0)?;
        let index =
            usize::try_from(*index).map_err(|_| RSpaceError::ProduceCommitProtocolViolation)?;
        if index >= count {
            return Err(RSpaceError::ProduceCommitProtocolViolation);
        }
        let peek_len = if index == 0 {
            0
        } else {
            1 + proto_varint_len(index as u64)
        };
        proto_add(&mut resting_len, proto_len_delimited(peek_len)?)?;
    }
    let event_len = proto_len_delimited(resting_len)?;
    let trace_bytes = u64::try_from(event_len).map_err(|_| RSpaceError::HostWorkRejected)?;
    let mut transfer_bytes = 0_u64;
    for (datum, _) in data {
        meter.reserve(1, 0, 0)?;
        transfer_bytes = transfer_bytes
            .checked_add(message_bytes(*datum).map_err(|_| RSpaceError::HostWorkRejected)?)
            .ok_or(RSpaceError::HostWorkRejected)?;
    }
    Ok(ByteCharge {
        introduction_bytes: 0,
        transfer_bytes,
        trace_bytes,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use models::rust::casper::protocol::casper_message::{
        ConsumeEvent, Event as CasperEvent, Peek, ProduceEvent, RestingCommEvent,
    };
    use proptest::prelude::*;

    use super::*;

    fn produce() -> Produce { Produce::create(&"channel", &"datum", false) }

    fn consume(channel_count: usize) -> Consume {
        Consume::create(
            &(0..channel_count)
                .map(|channel| u8::try_from(channel).expect("test channel fits u8"))
                .collect::<Vec<_>>(),
            &vec![0_u8; channel_count],
            &"continuation",
            false,
        )
    }

    #[test]
    fn resting_charge_is_exact_canonical_v7_protobuf_arm_length() {
        let waiter = consume(2);
        let mut first = produce();
        first.channel_hash = waiter.channel_hashes[0].clone();
        first.is_deterministic = true;
        first.output_value = vec![Vec::new(), vec![7; 130]];
        let mut second = produce();
        second.channel_hash = waiter.channel_hashes[1].clone();
        second.persistent = true;
        second.failed = true;
        let resting = RestingCOMM {
            consume: waiter.clone(),
            continuation_index: 128,
            ordered_produces: vec![first.clone(), second.clone()],
            ordered_data_indices: vec![0, 128],
            peeks: BTreeSet::from([0, 1]),
        };
        let datum = ListParWithRandom::default();
        let data = [(&datum, false), (&datum, true)];
        let unlimited = |_: usize, _: usize, _: usize| Ok(());
        let charge = resting_comm_charge_metered(&resting, &data, &unlimited).unwrap();
        let typed = CasperEvent::RestingComm(RestingCommEvent {
            consume: ConsumeEvent {
                channels_hashes: waiter
                    .channel_hashes
                    .iter()
                    .map(|hash| hash.to_bytes_prost())
                    .collect(),
                hash: waiter.hash.to_bytes_prost(),
                persistent: waiter.persistent,
            },
            continuation_index: 128,
            ordered_produces: [first, second]
                .into_iter()
                .map(|source| ProduceEvent {
                    channels_hash: source.channel_hash.to_bytes_prost(),
                    hash: source.hash.to_bytes_prost(),
                    persistent: source.persistent,
                    times_repeated: 0,
                    is_deterministic: source.is_deterministic,
                    output_value: source.output_value.into_iter().map(Into::into).collect(),
                    failed: source.failed,
                })
                .collect(),
            ordered_data_indices: vec![0, 128],
            peeks: vec![Peek { channel_index: 0 }, Peek { channel_index: 1 }],
        });
        assert_eq!(charge.introduction_bytes, 0);
        assert_eq!(charge.trace_bytes, typed.to_proto().encoded_len() as u64);
        assert_eq!(charge.transfer_bytes, datum.encoded_len() as u64 * 2);
    }

    #[test]
    fn resting_charge_refuses_invalid_address_and_exhausted_host_meter() {
        let mut source = produce();
        let mut waiter = consume(1);
        source.channel_hash = waiter.channel_hashes[0].clone();
        let mut resting = RestingCOMM {
            consume: waiter.clone(),
            continuation_index: -1,
            ordered_produces: vec![source],
            ordered_data_indices: vec![0],
            peeks: BTreeSet::new(),
        };
        let datum = ListParWithRandom::default();
        let data = [(&datum, false)];
        let unlimited = |_: usize, _: usize, _: usize| Ok(());
        assert!(matches!(
            resting_comm_charge_metered(&resting, &data, &unlimited),
            Err(RSpaceError::ProduceCommitProtocolViolation)
        ));
        resting.continuation_index = 0;
        resting.ordered_data_indices[0] = -1;
        assert!(matches!(
            resting_comm_charge_metered(&resting, &data, &unlimited),
            Err(RSpaceError::ProduceCommitProtocolViolation)
        ));
        resting.ordered_data_indices[0] = 0;
        waiter.channel_hashes.clear();
        resting.consume = waiter;
        assert!(matches!(
            resting_comm_charge_metered(&resting, &data, &unlimited),
            Err(RSpaceError::ProduceCommitProtocolViolation)
        ));
        resting.consume = consume(1);
        let refused = |_: usize, _: usize, _: usize| Err(RSpaceError::HostWorkRejected);
        assert!(matches!(
            resting_comm_charge_metered(&resting, &data, &refused),
            Err(RSpaceError::HostWorkRejected)
        ));
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
