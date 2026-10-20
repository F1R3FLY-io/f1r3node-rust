use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::tagged_continuation::TaggedCont;
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
    domain_identity(PRODUCE_INTRODUCTION_DOMAIN, &source.hash.0)
}

pub fn consume_introduction_identity(source: &Consume) -> [u8; 32] {
    domain_identity(CONSUME_INTRODUCTION_DOMAIN, &source.hash.0)
}

/// Added by G1-4a (DR-122): the introduction bytes of a produce whose channel
/// and datum have the given prost lengths. `produce_introduction_charge` and
/// `produce_introduction_charge_premeasured` charge this sum.
pub(crate) fn produce_introduction_bytes(
    channel: u64,
    datum: u64,
) -> Result<u64, ByteAccountingError> {
    channel
        .checked_add(datum)
        .and_then(|bytes| bytes.checked_add(HASH_BYTES.checked_mul(2)?))
        .ok_or(ByteAccountingError::Overflow)
}

/// Added by G1-4a (DR-122): the introduction bytes of a consume whose
/// channels and patterns have `binds` prost bytes in total and whose
/// continuation has `continuation` bytes.
pub(crate) fn consume_introduction_bytes(
    binds: u64,
    continuation: u64,
    channels: usize,
) -> Result<u64, ByteAccountingError> {
    checked_sum([binds, continuation, event_storage_bytes(channels)?])
}

pub fn produce_introduction_charge(
    channel: &Par,
    data: &ListParWithRandom,
) -> Result<ByteCharge, ByteAccountingError> {
    // Changed by G1-4a (DR-122): the sum moved to `produce_introduction_bytes`.
    // let introduction_bytes = message_bytes(channel)?
    //     .checked_add(message_bytes(data)?)
    //     .and_then(|bytes| bytes.checked_add(HASH_BYTES.checked_mul(2)?))
    //     .ok_or(ByteAccountingError::Overflow)?;
    let introduction_bytes =
        produce_introduction_bytes(message_bytes(channel)?, message_bytes(data)?)?;
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
    // Changed by G1-4a (DR-122): the sum moved to `consume_introduction_bytes`.
    // let introduction_bytes = sum_message_bytes(channels)?
    //     .checked_add(sum_message_bytes(patterns)?)
    //     .and_then(|bytes| bytes.checked_add(message_bytes(continuation).ok()?))
    //     .and_then(|bytes| bytes.checked_add(event_storage_bytes(channels.len()).ok()?))
    //     .ok_or(ByteAccountingError::Overflow)?;
    let binds = sum_message_bytes(channels)?
        .checked_add(sum_message_bytes(patterns)?)
        .ok_or(ByteAccountingError::Overflow)?;
    let introduction_bytes =
        consume_introduction_bytes(binds, message_bytes(continuation)?, channels.len())?;
    Ok(ByteCharge {
        introduction_bytes,
        ..ByteCharge::default()
    })
}

/// Added by D-F2 (DR-118): the prost lengths that the reducer measured when it
/// built an introduction. `pars` is the framed length of the datum's `pars`
/// field. `body` is the length of the continuation's `ParWithRandom`, and
/// `guard` is the length of its guard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntroductionMeasurement {
    Produce { pars: u64 },
    Consume { body: u64, guard: Option<u64> },
}

fn checked_sum<const N: usize>(parts: [u64; N]) -> Result<u64, ByteAccountingError> {
    parts.into_iter().try_fold(0_u64, |total, part| {
        total.checked_add(part).ok_or(ByteAccountingError::Overflow)
    })
}

/// Added by D-F2 (DR-118): the prost length of a length-delimited field with
/// `len` content bytes: its key, its length prefix and the content.
pub(crate) fn message_field_bytes(tag: u32, len: u64) -> Result<u64, ByteAccountingError> {
    let key =
        u64::try_from(prost::encoding::key_len(tag)).map_err(|_| ByteAccountingError::Overflow)?;
    let delimiter = u64::try_from(prost::encoding::encoded_len_varint(len))
        .map_err(|_| ByteAccountingError::Overflow)?;
    checked_sum([key, delimiter, len])
}

fn bytes_field_bytes(tag: u32, bytes: &[u8]) -> Result<u64, ByteAccountingError> {
    if bytes.is_empty() {
        return Ok(0);
    }
    message_field_bytes(
        tag,
        u64::try_from(bytes.len()).map_err(|_| ByteAccountingError::Overflow)?,
    )
}

// Disabled by G1-4a (DR-122): its two callers frame the measured seal and
// stack lengths through `datum_bytes` and `continuation_bytes` now, which
// give the same lengths (the D-F2 property tests compare them with prost).
// fn optional_message_field_bytes<M: Message>(
//     tag: u32,
//     message: Option<&M>,
// ) -> Result<u64, ByteAccountingError> {
//     message.map_or(Ok(0), |message| {
//         u64::try_from(prost::encoding::message::encoded_len(tag, message))
//             .map_err(|_| ByteAccountingError::Overflow)
//     })
// }

/// Added by D-F2 (DR-118): the prost length of `ParWithRandom { body:
/// Some(b), random_state }` when `b` has `body` bytes.
pub(crate) fn par_with_random_bytes(
    body: u64,
    random_state: &[u8],
) -> Result<u64, ByteAccountingError> {
    checked_sum([
        message_field_bytes(1, body)?,
        bytes_field_bytes(2, random_state)?,
    ])
}

/// Added by G1-4a (DR-122): the prost length of a `ListParWithRandom` from
/// the framed length of its `pars`, its random state and the lengths of its
/// seal and its stack. A `None` length is an absent field.
pub(crate) fn datum_bytes(
    pars: u64,
    random_state: &[u8],
    authority: Option<u64>,
    stack: Option<u64>,
) -> Result<u64, ByteAccountingError> {
    checked_sum([
        pars,
        bytes_field_bytes(2, random_state)?,
        authority.map_or(Ok(0), |len| message_field_bytes(3, len))?,
        stack.map_or(Ok(0), |len| message_field_bytes(4, len))?,
    ])
}

/// Added by G1-4a (DR-122): the prost length of a `TaggedContinuation` with
/// a `ParBody` of `body` bytes, an optional guard and an optional seal.
pub(crate) fn continuation_bytes(
    body: u64,
    guard: Option<u64>,
    authority: Option<u64>,
) -> Result<u64, ByteAccountingError> {
    checked_sum([
        message_field_bytes(1, body)?,
        guard.map_or(Ok(0), |guard| message_field_bytes(3, guard))?,
        authority.map_or(Ok(0), |len| message_field_bytes(4, len))?,
    ])
}

fn optional_message_len<M: Message>(
    message: Option<&M>,
) -> Result<Option<u64>, ByteAccountingError> {
    message.map(|message| message_bytes(message)).transpose()
}

/// Added by D-F2 (DR-118): `produce_introduction_charge` with the framed
/// length of `data.pars` measured by the reducer. It reads the channel, the
/// random state, the seal and the stack.
pub fn produce_introduction_charge_premeasured(
    channel: &Par,
    data: &ListParWithRandom,
    pars: u64,
) -> Result<ByteCharge, ByteAccountingError> {
    // Changed by G1-4a (DR-122): the sums moved to `datum_bytes` and
    // `produce_introduction_bytes`, which the pre-execution estimate shares.
    // let data_bytes = checked_sum([
    //     pars,
    //     bytes_field_bytes(2, &data.random_state)?,
    //     optional_message_field_bytes(3, data.cost_authority.as_ref())?,
    //     optional_message_field_bytes(4, data.cost_stack.as_ref())?,
    // ])?;
    // let introduction_bytes = checked_sum([
    //     message_bytes(channel)?,
    //     data_bytes,
    //     HASH_BYTES
    //         .checked_mul(2)
    //         .ok_or(ByteAccountingError::Overflow)?,
    // ])?;
    let data_bytes = datum_bytes(
        pars,
        &data.random_state,
        optional_message_len(data.cost_authority.as_ref())?,
        optional_message_len(data.cost_stack.as_ref())?,
    )?;
    let introduction_bytes = produce_introduction_bytes(message_bytes(channel)?, data_bytes)?;
    Ok(ByteCharge {
        introduction_bytes,
        ..ByteCharge::default()
    })
}

/// Added by D-F2 (DR-118): true when `continuation` has the shape that the
/// reducer measured: a `ParBody` and a guard exactly when the measurement has
/// one. It reads only the body variant and the guard presence.
pub(crate) fn consume_premeasurement_applies(
    continuation: &TaggedContinuation,
    guard: Option<u64>,
) -> bool {
    matches!(continuation.tagged_cont, Some(TaggedCont::ParBody(_)))
        && guard.is_some() == continuation.guard.is_some()
}

/// Added by D-F2 (DR-118): `consume_introduction_charge` with the body and
/// guard lengths measured by the reducer. It returns `None` when the
/// continuation does not have the shape of a reducer-built continuation, and
/// the caller then walks it.
pub fn consume_introduction_charge_premeasured(
    channels: &[Par],
    patterns: &[BindPattern],
    continuation: &TaggedContinuation,
    body: u64,
    guard: Option<u64>,
) -> Result<Option<ByteCharge>, ByteAccountingError> {
    if !consume_premeasurement_applies(continuation, guard) {
        return Ok(None);
    }
    // Changed by G1-4a (DR-122): the sums moved to `continuation_bytes` and
    // `consume_introduction_bytes`, which the pre-execution estimate shares.
    // let continuation_bytes = checked_sum([
    //     message_field_bytes(1, body)?,
    //     guard.map_or(Ok(0), |guard| message_field_bytes(3, guard))?,
    //     optional_message_field_bytes(4, continuation.cost_authority.as_ref())?,
    // ])?;
    // let introduction_bytes = checked_sum([
    //     sum_message_bytes(channels)?,
    //     sum_message_bytes(patterns)?,
    //     continuation_bytes,
    //     event_storage_bytes(channels.len())?,
    // ])?;
    let continuation = continuation_bytes(
        body,
        guard,
        optional_message_len(continuation.cost_authority.as_ref())?,
    )?;
    let binds = sum_message_bytes(channels)?
        .checked_add(sum_message_bytes(patterns)?)
        .ok_or(ByteAccountingError::Overflow)?;
    let introduction_bytes = consume_introduction_bytes(binds, continuation, channels.len())?;
    Ok(Some(ByteCharge {
        introduction_bytes,
        ..ByteCharge::default()
    }))
}

/// Added by G1-4a (DR-122): the transfer bytes of one datum that a COMM
/// delivers, its prost length.
pub fn datum_transfer_bytes(datum: &ListParWithRandom) -> Result<u64, ByteAccountingError> {
    message_bytes(datum)
}

/// Added by G1-4a (DR-122): the trace bytes of a COMM whose consume has
/// `channels` channels, `32 + 96 * channels`.
pub fn comm_trace_bytes(channels: usize) -> Result<u64, ByteAccountingError> {
    event_storage_bytes(channels)?
        .checked_add(
            u64::try_from(channels)
                .map_err(|_| ByteAccountingError::Overflow)?
                .checked_mul(event_storage_bytes(1)?)
                .ok_or(ByteAccountingError::Overflow)?,
        )
        .ok_or(ByteAccountingError::Overflow)
}

pub fn comm_charge(
    comm: &COMM,
    data: &[(&ListParWithRandom, bool)],
) -> Result<ByteCharge, ByteAccountingError> {
    // Changed by G1-4a (DR-122): both sums moved to `datum_transfer_bytes`
    // and `comm_trace_bytes`, which the pre-execution estimate shares.
    // let transfer_bytes = data.iter().try_fold(0_u64, |total, (datum, _)| {
    //     total
    //         .checked_add(message_bytes(*datum)?)
    //         .ok_or(ByteAccountingError::Overflow)
    // })?;
    // let channel_count = comm.consume.channel_hashes.len();
    // let trace_bytes = event_storage_bytes(channel_count)?
    //     .checked_add(
    //         u64::try_from(channel_count)
    //             .map_err(|_| ByteAccountingError::Overflow)?
    //             .checked_mul(event_storage_bytes(1)?)
    //             .ok_or(ByteAccountingError::Overflow)?,
    //     )
    //     .ok_or(ByteAccountingError::Overflow)?;
    let transfer_bytes = data.iter().try_fold(0_u64, |total, (datum, _)| {
        total
            .checked_add(datum_transfer_bytes(datum)?)
            .ok_or(ByteAccountingError::Overflow)
    })?;
    let trace_bytes = comm_trace_bytes(comm.consume.channel_hashes.len())?;
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

    fn ground(bytes: Vec<u8>) -> models::rhoapi::CostSignature {
        models::rhoapi::CostSignature {
            value: Some(models::rhoapi::cost_signature::Value::Ground(bytes)),
        }
    }

    fn optional_authority() -> impl Strategy<Value = Option<models::rhoapi::CostAuthority>> {
        prop_oneof![
            Just(None),
            Just(Some(models::rhoapi::CostAuthority::default())),
            prop::collection::vec(
                (
                    prop::collection::vec(any::<u8>(), 0..64),
                    prop::collection::vec(any::<u8>(), 0..64)
                ),
                1..4
            )
            .prop_map(|regions| Some(models::rhoapi::CostAuthority {
                regions: regions
                    .into_iter()
                    .map(|(signature, instance_id)| models::rhoapi::CostRegion {
                        instance_id,
                        signature: Some(ground(signature)),
                    })
                    .collect(),
            })),
        ]
    }

    fn optional_stack() -> impl Strategy<Value = Option<models::rhoapi::CostStack>> {
        prop_oneof![
            Just(None),
            Just(Some(models::rhoapi::CostStack::default())),
            prop::collection::vec(prop::collection::vec(any::<u8>(), 0..64), 1..4).prop_map(
                |cells| Some(models::rhoapi::CostStack {
                    cells: cells.into_iter().map(ground).collect(),
                })
            ),
        ]
    }

    fn term() -> impl Strategy<Value = Par> {
        prop_oneof![
            Just(Par::default()),
            (120_usize..17_000).prop_map(|size| models::rust::utils::new_gstring_par(
                "d".repeat(size),
                Vec::new(),
                false
            )),
            crate::rust::interpreter::accounting::random_par_term(),
        ]
    }

    fn bind_pattern() -> impl Strategy<Value = BindPattern> {
        use models::rhoapi::var::{VarInstance, WildcardMsg};
        use models::rhoapi::Var;
        (
            prop::collection::vec(term(), 0..3),
            prop_oneof![
                Just(None),
                Just(Some(Var { var_instance: None })),
                any::<i32>().prop_map(|index| Some(Var {
                    var_instance: Some(VarInstance::FreeVar(index))
                })),
                any::<i32>().prop_map(|index| Some(Var {
                    var_instance: Some(VarInstance::BoundVar(index))
                })),
                Just(Some(Var {
                    var_instance: Some(VarInstance::Wildcard(WildcardMsg {}))
                })),
            ],
            any::<i32>(),
        )
            .prop_map(|(patterns, remainder, free_count)| BindPattern {
                patterns,
                remainder,
                free_count,
            })
    }

    /// A random state that is empty in a fixed share of the cases, because
    /// prost omits an empty one (the default-field rule).
    fn random_state() -> impl Strategy<Value = Vec<u8>> {
        prop_oneof![Just(Vec::new()), prop::collection::vec(any::<u8>(), 1..300),]
    }

    /// The `pars` measurement as the reducer composes it from the substituted
    /// data.
    fn reducer_pars(pars: &[Par]) -> u64 {
        pars.iter()
            .map(|par| message_field_bytes(1, u64::try_from(par.encoded_len()).unwrap()).unwrap())
            .sum()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// D-F2 (DR-118): a produce introduction charged from the reducer's
        /// measurement equals the walked charge on every datum shape.
        #[test]
        fn measured_produce_introduction_bytes_equal_prost_encoded_len(
            channel in term(),
            pars in prop::collection::vec(term(), 0..4),
            random_state in random_state(),
            cost_authority in optional_authority(),
            cost_stack in optional_stack(),
        ) {
            let data = ListParWithRandom { pars, random_state, cost_authority, cost_stack };
            prop_assert_eq!(
                produce_introduction_charge_premeasured(&channel, &data, reducer_pars(&data.pars)),
                produce_introduction_charge(&channel, &data)
            );
        }

        /// D-F2 (DR-118): the reducer composes the length of the
        /// continuation's `ParWithRandom` from the substituted body's length.
        #[test]
        fn measured_body_bytes_equal_prost_encoded_len(
            body in term(),
            random_state in random_state(),
        ) {
            let composed = par_with_random_bytes(
                u64::try_from(body.encoded_len()).unwrap(),
                &random_state,
            );
            let body = models::rhoapi::ParWithRandom { body: Some(body), random_state };
            prop_assert_eq!(composed, Ok(u64::try_from(body.encoded_len()).unwrap()));
        }

        /// D-F2 (DR-118): a consume introduction charged from the reducer's
        /// measurement equals the walked charge on every reducer shape, and
        /// it asks for the walk on every other shape.
        #[test]
        fn measured_consume_introduction_bytes_equal_prost_encoded_len(
            channels in prop::collection::vec(term(), 1..4),
            patterns in prop::collection::vec(bind_pattern(), 1..4),
            shape in 0_u8..3,
            body in proptest::option::of(term()),
            random_state in random_state(),
            body_ref in any::<i64>(),
            guard in prop_oneof![Just(None), Just(Some(Par::default())), term().prop_map(Some)],
            measure_guard in any::<bool>(),
            cost_authority in optional_authority(),
        ) {
            let tagged_cont = match shape {
                0 => Some(TaggedCont::ParBody(models::rhoapi::ParWithRandom { body, random_state })),
                1 => Some(TaggedCont::ScalaBodyRef(body_ref)),
                _ => None,
            };
            let body = match &tagged_cont {
                Some(TaggedCont::ParBody(body)) => u64::try_from(body.encoded_len()).unwrap(),
                _ => 0,
            };
            let guard_bytes = measure_guard
                .then(|| guard.as_ref().map_or(0, |guard| u64::try_from(guard.encoded_len()).unwrap()));
            let continuation = TaggedContinuation { tagged_cont, guard, cost_authority };
            let reducer_shape = matches!(continuation.tagged_cont, Some(TaggedCont::ParBody(_)))
                && guard_bytes.is_some() == continuation.guard.is_some();
            let expected = reducer_shape
                .then(|| consume_introduction_charge(&channels, &patterns, &continuation).unwrap());
            prop_assert_eq!(
                consume_introduction_charge_premeasured(&channels, &patterns, &continuation, body, guard_bytes),
                Ok(expected)
            );
        }
    }

    /// D-F2 (DR-118): the field framing equals prost's own length-delimited
    /// framing at every varint width boundary.
    #[test]
    fn framing_matches_prost_at_varint_boundaries() {
        for len in [0_usize, 1, 127, 128, 16_383, 16_384, 2_097_151, 2_097_152] {
            let bytes = vec![0_u8; len];
            for tag in [1_u32, 2, 3, 4, 15, 16] {
                assert_eq!(
                    message_field_bytes(tag, u64::try_from(len).unwrap()),
                    Ok(u64::try_from(prost::encoding::bytes::encoded_len(tag, &bytes)).unwrap())
                );
            }
        }
    }

    /// D-F2 (DR-118), negative control: prost omits a default field, so a
    /// composition that frames every field counts bytes that the walk does
    /// not count.
    #[test]
    fn naive_framing_differs_from_prost_on_default_fields() {
        let channel = Par::default();
        let empty = ListParWithRandom::default();
        let walked = produce_introduction_charge(&channel, &empty).unwrap();
        let naive = message_bytes(&channel).unwrap() + message_field_bytes(2, 0).unwrap() + 64;
        assert_eq!(empty.encoded_len(), 0);
        assert_ne!(naive, walked.introduction_bytes);
        assert_eq!(
            produce_introduction_charge_premeasured(&channel, &empty, 0),
            Ok(walked)
        );
    }

    #[test]
    fn premeasured_charges_reject_overflow() {
        let channel = Par::default();
        let data = ListParWithRandom::default();
        assert_eq!(
            produce_introduction_charge_premeasured(&channel, &data, u64::MAX),
            Err(ByteAccountingError::Overflow)
        );
        let continuation = TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(models::rhoapi::ParWithRandom::default())),
            ..TaggedContinuation::default()
        };
        assert_eq!(
            consume_introduction_charge_premeasured(&[channel], &[], &continuation, u64::MAX, None),
            Err(ByteAccountingError::Overflow)
        );
        assert_eq!(
            message_field_bytes(1, u64::MAX),
            Err(ByteAccountingError::Overflow)
        );
    }
}
