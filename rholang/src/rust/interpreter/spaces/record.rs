//! The records of the native spaces (M-2, P4 design B.2 and B.7).
//!
//! A native space keeps its state in ordinary host records on its `0xE0` channels. Each record
//! carries a space-owned envelope inside its payload, so the host structs, serializers and
//! identities do not change.
//!
//! | Record              | Content                                                           |
//! |---------------------|-------------------------------------------------------------------|
//! | datum               | `pars = [EList[tag_D, GInt(ord), Nil, Nil], payload…]`            |
//! | parked continuation | `ParBody.body = Par{exprs: [EList[tag_K, GInt(ord), Nil, body]]}` |
//! | counter             | `pars = [EList[tag_S], GInt(n)]`                                  |
//! | config              | `pars = [EList[tag_C], config map]`                               |
//!
//! A tag is `GByteArray([0xE0, 0x52, kind, 1])`, version 1 of the envelope of its kind (A20).
//! The `Nil` slots are the P4 slots: priority and embedding of a datum, the query policy of a
//! continuation. The first par of every record is a list, so no space datum is a single number,
//! and a datum may have an empty payload (A11). A datum record keeps the random state, the cost
//! authority and the cost stack of the user datum. A parked continuation keeps its random state,
//! guard and cost authority outside the envelope.
//!
//! A priority queue (P5b, decision 38) fills two of these slots. Its own encoders and decoders
//! read and write them, and the strict P4 decoders stay as they are for every other discipline:
//!
//! | Slot                         | Value of a priority queue                                   |
//! |------------------------------|-------------------------------------------------------------|
//! | priority of a datum          | `GInt(p)` for the level `p ≥ 0`, `Nil` for the unprioritized level |
//! | policy of a continuation     | `EMap{"priority": GInt(p)}` for the waiter queue of the level `p`, `Nil` for the unrestricted queue |
//!
//! An unprioritized datum record and an unrestricted continuation of a priority queue are
//! byte-identical to the P4 records. The outcome of a collection operation decodes every fired
//! continuation with [`into_continuation`], which knows no discipline, so that decoder accepts
//! both policy forms.
//!
//! A waiter of a join (P5c, decision 29) parks with version 2 of the continuation envelope. It
//! holds one slot for each position of its group, in group order:
//!
//! | Record                    | Content                                                                 |
//! |---------------------------|-------------------------------------------------------------------------|
//! | join continuation         | `ParBody.body = Par{exprs: [EList[tag_K2, EList[slot…], body]]}`         |
//! | slot of a group position  | `EList[GInt(ord), GInt(bind), policy]`, `policy` as for a priority queue |
//!
//! The tag `tag_K2` is `GByteArray([0xE0, 0x52, b'K', 2])`. The bind indices of the slots are a
//! permutation of the binds, so [`restore_bind_order`] puts the results of a fire back into the
//! bind order of the receive. A waiter with one bind keeps the version 1 record, and the decoders
//! of each version reject the records of the other version.
//!
//! The bit sets of an envelope are the union of the bit sets of its elements. They belong to no
//! identity. The decoders are strict: a record that is not the exact image of its encoder is a
//! [`MalformedRecord`], and the operation aborts with `Inconsistent`.
//!
//! The space agents build each parked continuation from its parts, with one copy of each part
//! ([`wrap_continuation_from_parts`] and the forms for the other envelopes).

use std::collections::BTreeMap;
use std::sync::Arc;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    CostAuthority, EList, EMap, Expr, KeyValuePair, ListParWithRandom, Par, ParWithRandom,
    TaggedContinuation,
};
use models::rust::utils::union;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;
use rspace_plus_plus::rspace::space_store_handle::BeforeObserve;

use super::agent::{ConsumeOp, ProduceOp};
use super::errors::SpaceError;
// Added by decision 128 (kind routing): the registry gives the query keys of a waiter policy.
use super::families;
use super::namespace::REIFIED_TAG;
use super::types::config::{map_entries, map_par, string_par};
// Changed by decision 124 (plan v2 C7.1, decision 99): the machinery seal is the system residue
// of the payer. The text before:
// ```
// use crate::rust::interpreter::accounting::authority::{
//     resolve_system_residue, AuthorityByteEventKind, ResidueContext,
// };
// ```
use crate::rust::interpreter::accounting::authority::{
    resolve_system_residue, system_residue_authority_with_caller, AuthorityByteEventKind,
    ResidueContext,
};
use crate::rust::interpreter::accounting::byte_accounting::{
    consume_introduction_identity, produce_introduction_identity,
};
use crate::rust::interpreter::accounting::RuntimeBudget;

/// The bincode bytes of `value`, as the host encodes a channel or a datum. An encode that fails
/// is `Inconsistent`.
pub fn bincode_bytes<T: serde::Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, SpaceError> {
    bincode::serialize(value).map_err(|_| SpaceError::Inconsistent("a space value does not encode"))
}

/// The blake2b-256 hash of the bincode bytes of `value`, as the host hashes a channel
/// (`stable_hash_provider::hash`).
pub fn bincode_digest<T: serde::Serialize + ?Sized>(
    value: &T,
) -> Result<Blake2b256Hash, SpaceError> {
    Ok(Blake2b256Hash::new(&bincode_bytes(value)?))
}

/// The second byte of every record tag.
pub const RECORD_TAG_MARK: u8 = 0x52;

/// The envelope version of P4 (A20).
pub const RECORD_VERSION: u8 = 1;

/// The kind of a space record. The discriminant is the kind byte of its tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum RecordKind {
    Datum = b'D',
    Continuation = b'K',
    Counter = b'S',
    Config = b'C',
}

impl RecordKind {
    /// Every record kind.
    pub const ALL: [RecordKind; 4] = [
        RecordKind::Datum,
        RecordKind::Continuation,
        RecordKind::Counter,
        RecordKind::Config,
    ];

    /// The bytes of the tag: `[0xE0, 0x52, kind, version]`.
    pub const fn tag_bytes(self) -> [u8; 4] {
        [REIFIED_TAG, RECORD_TAG_MARK, self as u8, RECORD_VERSION]
    }

    /// The tag `GByteArray(tag_bytes)`.
    pub fn tag(self) -> Par { expr_par(ExprInstance::GByteArray(self.tag_bytes().to_vec())) }
}

/// A record that is not the exact image of its encoder. It aborts the operation with
/// `Inconsistent` (P4 design B.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("a malformed {0:?} record")]
pub struct MalformedRecord(pub RecordKind);

impl From<MalformedRecord> for SpaceError {
    fn from(error: MalformedRecord) -> Self {
        SpaceError::Inconsistent(match error.0 {
            RecordKind::Datum => "a malformed datum record",
            RecordKind::Continuation => "a malformed continuation record",
            RecordKind::Counter => "a malformed counter record",
            RecordKind::Config => "a malformed config record",
        })
    }
}

/// The expression of a `Par` that holds exactly one expression and nothing else. The bit sets
/// do not count.
pub(crate) fn sole_expr(par: &Par) -> Option<&ExprInstance> {
    let Par {
        sends,
        receives,
        news,
        exprs,
        matches,
        unforgeables,
        bundles,
        connectives,
        conditionals,
        locally_free: _,
        connective_used: _,
        cost_signed_terms,
        cost_stacks,
        use_blocks,
    } = par;
    let structural = sends.is_empty()
        && receives.is_empty()
        && news.is_empty()
        && matches.is_empty()
        && unforgeables.is_empty()
        && bundles.is_empty()
        && connectives.is_empty()
        && conditionals.is_empty()
        && cost_signed_terms.is_empty()
        && use_blocks.is_empty()
        && cost_stacks.is_empty();
    match (structural, exprs.as_slice()) {
        (
            true,
            [Expr {
                expr_instance: Some(instance),
            }],
        ) => Some(instance),
        (true, _) | (false, _) => None,
    }
}

/// A `Par` that holds exactly one ground expression.
pub(crate) fn expr_par(instance: ExprInstance) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Par::default()
    }
}

/// A list with the bit sets of the normalizer: the union of the `locally_free` sets and the
/// disjunction of the `connective_used` flags of its elements.
fn list_par(ps: Vec<Par>) -> Par {
    let (locally_free, connective_used) = ps.iter().fold(
        (Vec::new(), false),
        |(locally_free, connective_used), element| {
            (
                union(locally_free, element.locally_free.clone()),
                connective_used || element.connective_used,
            )
        },
    );
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps,
                locally_free: locally_free.clone(),
                connective_used,
                remainder: None,
            })),
        }],
        locally_free,
        connective_used,
        ..Par::default()
    }
}

/// The elements of a `Par` that is exactly one list with the flags of [`list_par`]: no
/// remainder, and the `connective_used` flags of the list and of the `Par` are the disjunction
/// of the flags of the elements.
fn list_elements(par: &Par) -> Option<&[Par]> {
    match sole_expr(par) {
        Some(ExprInstance::EListBody(EList {
            ps,
            connective_used,
            remainder: None,
            ..
        })) => {
            let expected = ps.iter().any(|element| element.connective_used);
            match (
                *connective_used == expected,
                par.connective_used == expected,
            ) {
                (true, true) => Some(ps.as_slice()),
                (true, false) | (false, _) => None,
            }
        }
        _ => None,
    }
}

fn is_tag(par: &Par, kind: RecordKind) -> bool {
    match (sole_expr(par), par.connective_used) {
        (Some(ExprInstance::GByteArray(bytes)), false) => bytes.as_slice() == kind.tag_bytes(),
        _ => false,
    }
}

fn is_nil(par: &Par) -> bool { *par == Par::default() }

fn natural_of(par: &Par) -> Option<i64> {
    match (sole_expr(par), par.connective_used) {
        (Some(ExprInstance::GInt(value)), false) if *value >= 0 => Some(*value),
        _ => None,
    }
}

fn int_par(value: i64) -> Par { expr_par(ExprInstance::GInt(value)) }

/// The envelope of a datum record with the order `ord`.
pub fn datum_envelope(ord: i64) -> Par {
    list_par(vec![
        RecordKind::Datum.tag(),
        int_par(ord),
        Par::default(),
        Par::default(),
    ])
}

/// The datum record of `datum` with the order `ord`: the envelope, then the payload. The random
/// state, the cost authority and the cost stack of `datum` stay as they are.
pub fn wrap_datum(ord: i64, datum: &ListParWithRandom) -> ListParWithRandom {
    let mut pars = Vec::with_capacity(datum.pars.len() + 1);
    pars.push(datum_envelope(ord));
    pars.extend(datum.pars.iter().cloned());
    ListParWithRandom {
        pars,
        random_state: datum.random_state.clone(),
        cost_authority: datum.cost_authority.clone(),
        cost_stack: datum.cost_stack.clone(),
    }
}

/// The order of a datum record. Only the envelope is decoded.
pub fn datum_ord(record: &ListParWithRandom) -> Result<i64, MalformedRecord> {
    let malformed = MalformedRecord(RecordKind::Datum);
    match record.pars.first().and_then(list_elements) {
        Some([tag, ord, priority, embedding])
            if is_tag(tag, RecordKind::Datum) && is_nil(priority) && is_nil(embedding) =>
        {
            natural_of(ord).ok_or(malformed)
        }
        _ => Err(malformed),
    }
}

/// A decoded datum record.
#[derive(Clone, Debug, PartialEq)]
pub struct DatumRecord {
    /// The order of the record in its collection.
    pub ord: i64,
    /// The datum as the program sent it: the payload, its random state, its cost authority and
    /// its cost stack.
    pub datum: ListParWithRandom,
}

/// Decodes a datum record (the inverse of [`wrap_datum`]).
pub fn unwrap_datum(record: &ListParWithRandom) -> Result<DatumRecord, MalformedRecord> {
    let ord = datum_ord(record)?;
    let payload = record
        .pars
        .get(1..)
        .ok_or(MalformedRecord(RecordKind::Datum))?;
    Ok(DatumRecord {
        ord,
        datum: ListParWithRandom {
            pars: payload.to_vec(),
            random_state: record.random_state.clone(),
            cost_authority: record.cost_authority.clone(),
            cost_stack: record.cost_stack.clone(),
        },
    })
}

/// The envelope of a parked continuation with the order `ord` around `body`.
pub fn continuation_envelope(ord: i64, body: Par) -> Par {
    list_par(vec![
        RecordKind::Continuation.tag(),
        int_par(ord),
        Par::default(),
        body,
    ])
}

/// The parked continuation of `continuation` with the order `ord`: its body inside the
/// envelope. The random state, the guard and the cost authority stay as they are. Only a
/// `ParBody` with a body has a continuation record.
pub fn wrap_continuation(
    ord: i64,
    continuation: &TaggedContinuation,
) -> Result<TaggedContinuation, MalformedRecord> {
    match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(body),
            random_state,
        })) => Ok(TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(continuation_envelope(ord, body.clone())),
                random_state: random_state.clone(),
            })),
            guard: continuation.guard.clone(),
            cost_authority: continuation.cost_authority.clone(),
        }),
        Some(TaggedCont::ParBody(ParWithRandom { body: None, .. }))
        | Some(TaggedCont::ScalaBodyRef(_))
        | None => Err(MalformedRecord(RecordKind::Continuation)),
    }
}

/// The order and the original body of a parked continuation.
fn continuation_parts(
    continuation: &TaggedContinuation,
) -> Result<(i64, &Par, &ParWithRandom), MalformedRecord> {
    let malformed = MalformedRecord(RecordKind::Continuation);
    let body = match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(body)) => body,
        Some(TaggedCont::ScalaBodyRef(_)) | None => return Err(malformed),
    };
    match body.body.as_ref().and_then(list_elements) {
        Some([tag, ord, policy, original])
            if is_tag(tag, RecordKind::Continuation) && is_nil(policy) =>
        {
            let ord = natural_of(ord).ok_or(malformed)?;
            Ok((ord, original, body))
        }
        _ => Err(malformed),
    }
}

/// The order of a parked continuation. Only the envelope is decoded.
pub fn continuation_ord(continuation: &TaggedContinuation) -> Result<i64, MalformedRecord> {
    continuation_parts(continuation).map(|(ord, _, _)| ord)
}

/// Added by decision 92: the random state of the `ParBody` of `continuation`, or `None` for a
/// continuation without a `ParBody`. A wrapper of a continuation keeps the random state, so a
/// record and its original continuation give the same bytes (`spaces::anchor::SpaceJob`).
pub fn continuation_random_state(continuation: &TaggedContinuation) -> Option<&[u8]> {
    match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(ParWithRandom { random_state, .. })) => Some(random_state),
        Some(TaggedCont::ScalaBodyRef(_)) | None => None,
    }
}

/// A decoded continuation record.
#[derive(Clone, Debug, PartialEq)]
pub struct ContinuationRecord {
    /// The order of the continuation in its collection.
    pub ord: i64,
    /// The continuation as the program parked it: the original body, its random state, its
    /// guard and its cost authority.
    pub continuation: TaggedContinuation,
}

/// Decodes a parked continuation (the inverse of [`wrap_continuation`]).
pub fn unwrap_continuation(
    continuation: &TaggedContinuation,
) -> Result<ContinuationRecord, MalformedRecord> {
    let (ord, original, body) = continuation_parts(continuation)?;
    Ok(ContinuationRecord {
        ord,
        continuation: TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(original.clone()),
                random_state: body.random_state.clone(),
            })),
            guard: continuation.guard.clone(),
            cost_authority: continuation.cost_authority.clone(),
        },
    })
}

/// The pars of the counter record with the value `value`: the envelope, then the value.
fn counter_pars(value: i64) -> Vec<Par> {
    vec![list_par(vec![RecordKind::Counter.tag()]), int_par(value)]
}

/// The counter record with the value `value`, the random state `random` and the cost authority
/// `cost_authority`.
pub fn counter_record(
    value: i64,
    random: &Blake2b512Random,
    cost_authority: Option<CostAuthority>,
) -> ListParWithRandom {
    ListParWithRandom {
        pars: counter_pars(value),
        random_state: random.to_bytes(),
        cost_authority,
        cost_stack: None,
    }
}

/// The value of a counter record.
pub fn counter_value(record: &ListParWithRandom) -> Result<i64, MalformedRecord> {
    let malformed = MalformedRecord(RecordKind::Counter);
    match (record.pars.as_slice(), &record.cost_stack) {
        ([envelope, value], None) => match list_elements(envelope) {
            Some([tag]) if is_tag(tag, RecordKind::Counter) => natural_of(value).ok_or(malformed),
            _ => Err(malformed),
        },
        _ => Err(malformed),
    }
}

/// The envelope of a config record.
fn config_envelope() -> Par { list_par(vec![RecordKind::Config.tag()]) }

/// The config record of the config map `config` (I-17) with the random state `random`. It has
/// no cost authority (P4 design B.7).
pub fn config_record(config: Par, random: &Blake2b512Random) -> ListParWithRandom {
    ListParWithRandom {
        pars: vec![config_envelope(), config],
        random_state: random.to_bytes(),
        cost_authority: None,
        cost_stack: None,
    }
}

/// The config map of a config record. `types::config::SpaceConfig::from_par` decodes it.
pub fn config_map(record: &ListParWithRandom) -> Result<&Par, MalformedRecord> {
    let malformed = MalformedRecord(RecordKind::Config);
    match (
        record.pars.as_slice(),
        &record.cost_authority,
        &record.cost_stack,
    ) {
        ([envelope, map], None, None) => match list_elements(envelope) {
            Some([tag]) if is_tag(tag, RecordKind::Config) => Ok(map),
            _ => Err(malformed),
        },
        _ => Err(malformed),
    }
}

/// The authority that a record carries for `authority`, by reference: the authority when it has
/// a region, as `eval_send` stamps a datum and `consume` stamps a continuation.
pub fn stamped(authority: &CostAuthority) -> Option<&CostAuthority> {
    match authority.regions.is_empty() {
        true => None,
        false => Some(authority),
    }
}

/// The cost authority that a record carries for `authority`: a copy of [`stamped`].
pub fn cost_authority(authority: &CostAuthority) -> Option<CostAuthority> {
    stamped(authority).cloned()
}

// Disabled by decision 42: the authorities were copied once for each operation, before the
// operation needed them. The borrowed form below copies an authority only where a record or a
// callback keeps it, and the meter reserves that copy.
// /// The authorities of the records that one collection operation stages (P4 design B.7).
// #[derive(Clone, Debug, Default, PartialEq)]
// pub struct OperationAuthority {
//     /// The cost authority of a counter continuation: the cost authority of the datum or of the
//     /// continuation of the operation.
//     pub continuation: Option<CostAuthority>,
//     /// The introduction authority of the operation. Every staged operation registers it, and a
//     /// new counter datum carries it.
//     pub introduction: CostAuthority,
// }
//
// impl OperationAuthority {
//     /// The authorities of a produce: the cost authority of its datum and its introduction
//     /// authority.
//     pub fn of_produce(op: &ProduceOp) -> Self {
//         Self {
//             continuation: op.data.cost_authority.clone(),
//             introduction: op.authority.clone(),
//         }
//     }
//
//     /// The authorities of a consume: its receive authority and its introduction authority.
//     pub fn of_consume(op: &ConsumeOp) -> Self {
//         Self {
//             continuation: cost_authority(&op.authority),
//             introduction: op.introduction_authority.clone(),
//         }
//     }
// }

/// The authorities of the records that one collection operation stages (P4 design B.7), by
/// reference.
#[derive(Clone, Copy)]
pub struct OperationAuthority<'a> {
    /// The cost authority of a counter continuation: the cost authority of the datum or of the
    /// continuation of the operation.
    pub continuation: Option<&'a CostAuthority>,
    /// The introduction of the operation. Every staged operation registers it, and a new counter
    /// datum carries its authority.
    pub introduction: &'a Arc<Introduction>,
}

impl<'a> OperationAuthority<'a> {
    /// The authorities of a produce: the cost authority of its datum and the introduction of
    /// the send.
    pub fn of_produce(op: &'a ProduceOp, introduction: &'a Arc<Introduction>) -> Self {
        Self {
            continuation: op.data.cost_authority.as_ref(),
            introduction,
        }
    }

    /// The authorities of a consume: its receive authority and the introduction of the receive.
    pub fn of_consume(op: &'a ConsumeOp, introduction: &'a Arc<Introduction>) -> Self {
        Self {
            continuation: stamped(&op.authority),
            introduction,
        }
    }
}

// Disabled by decision 42: each staged operation copied the budget handle and the authority.
// One `Introduction` per operation now holds them, and each callback copies one shared pointer
// (`register_introduction` below).
// /// The `before_observe` callback of a staged operation. At commit, right before the observer
// /// sees the operation, it registers `authority` as the introduction authority of the operation
// /// identity, where the reducer's `produce` and `consume` register it (P4 design B.7). A repeated
// /// identity, a re-offer or a re-fire, registers its first authority again. Another authority
// /// fails the commit at that operation.
// pub fn register_introduction(budget: &RuntimeBudget, authority: &CostAuthority) -> BeforeObserve {
//     let budget = budget.clone();
//     let authority = authority.clone();
//     Box::new(move |source: RSpaceOperationSource<'_>| {
//         let (identity, kind) = match source {
//             RSpaceOperationSource::Produce(produce) => (
//                 produce_introduction_identity(produce),
//                 AuthorityByteEventKind::ProduceIntroduction,
//             ),
//             RSpaceOperationSource::Consume(consume) => (
//                 consume_introduction_identity(consume),
//                 AuthorityByteEventKind::ConsumeIntroduction,
//             ),
//         };
//         budget
//             .register_introduction_authority(identity, kind, &authority)
//             .map_err(RSpaceError::from)
//     })
// }

// ── The builder from parts and the owned decoder of a continuation ────────────────────────

/// The parked continuation of a receive with the order `ord`, built from its parts with one
/// copy of each part: the body inside the envelope, the random state, the guard and the cost
/// authority. It equals [`wrap_continuation`] of the continuation with these parts.
pub fn wrap_continuation_from_parts(
    ord: i64,
    body: &ParWithRandom,
    guard: &Option<Par>,
    cost_authority: Option<&CostAuthority>,
) -> Result<TaggedContinuation, SpaceError> {
    let Some(original) = &body.body else {
        return Err(MalformedRecord(RecordKind::Continuation).into());
    };
    let cost_authority = cost_authority.cloned();
    Ok(TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(continuation_envelope(ord, original.clone())),
            random_state: body.random_state.clone(),
        })),
        guard: guard.clone(),
        cost_authority,
    })
}

/// Decodes an owned parked continuation (the inverse of [`wrap_continuation`], of
/// [`wrap_pq_continuation`] and of [`wrap_policy_continuation`]). It moves the original body,
/// the random state, the guard and the cost authority out of the record, so it copies none of
/// these parts.
/// The outcome of every family decodes its fired continuations here, and a fired waiter carries its
/// policy: the level of a priority queue (P5b) or the query of a VectorDB waiter (P6). So this
/// decoder accepts every policy that [`policy_of`] accepts.
pub fn into_continuation(
    continuation: TaggedContinuation,
) -> Result<ContinuationRecord, MalformedRecord> {
    let malformed = MalformedRecord(RecordKind::Continuation);
    // Replaced by P5b (decision 38): the strict decoder rejects the policy of a waiter of a
    // priority queue, so a fire of such a waiter would fail after its commit. The decoder of the
    // priority queue accepts `Nil` and that policy, and it is the strict decoder on `Nil`.
    // let ord = continuation_ord(&continuation)?;
    // Replaced by P6 (design v3 4.0): the decoder of the priority queue rejects the query policy
    // of a VectorDB waiter, so a fire of such a waiter would fail after its commit. The decoder
    // of every policy accepts `Nil`, the level of a priority queue and a VectorDB query.
    // let (ord, _) = pq_continuation_slots(&continuation)?;
    let (ord, _) = continuation_policy_slots(&continuation)?;
    let TaggedContinuation {
        tagged_cont,
        guard,
        cost_authority,
    } = continuation;
    let ParWithRandom { body, random_state } = match tagged_cont {
        Some(TaggedCont::ParBody(body)) => body,
        Some(TaggedCont::ScalaBodyRef(_)) | None => return Err(malformed),
    };
    let original = body
        .and_then(|mut envelope| match envelope.exprs.pop() {
            Some(Expr {
                expr_instance: Some(ExprInstance::EListBody(EList { mut ps, .. })),
            }) => ps.pop(),
            _ => None,
        })
        .ok_or(malformed)?;
    Ok(ContinuationRecord {
        ord,
        continuation: TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(original),
                random_state,
            })),
            guard,
            cost_authority,
        },
    })
}

// ── The slots of a priority queue (P5b, decision 38) ──────────────────────────────────────

/// The key of the policy of a waiter of a priority queue. It is also the name of the send param
/// and of the query param of a priority queue.
pub const PRIORITY_KEY: &str = "priority";

/// The priority slot of a datum envelope: `GInt(p)` for the level `p`, and `Nil` for the
/// unprioritized level.
fn priority_slot(priority: Option<i64>) -> Par {
    match priority {
        Some(level) => int_par(level),
        None => Par::default(),
    }
}

/// The level of a priority slot: `Some(None)` for `Nil`, `Some(Some(p))` for `GInt(p)` with
/// `p ≥ 0` and no connective flag, and `None` for every other value.
fn level_of_slot(slot: &Par) -> Option<Option<i64>> {
    match is_nil(slot) {
        true => Some(None),
        false => natural_of(slot).map(Some),
    }
}

/// The envelope of a datum record of a priority queue with the order `ord` and the level
/// `priority`: `Some(p)` for the level `p` and `None` for the unprioritized level. The envelope of
/// the unprioritized level is [`datum_envelope`].
pub fn pq_datum_envelope(ord: i64, priority: Option<i64>) -> Par {
    list_par(vec![
        RecordKind::Datum.tag(),
        int_par(ord),
        priority_slot(priority),
        Par::default(),
    ])
}

/// The datum record of `datum` in a priority queue, with the order `ord` and the level
/// `priority`: the envelope, then the payload. The random state, the cost authority and the cost
/// stack of `datum` stay as they are.
pub fn wrap_pq_datum(
    ord: i64,
    priority: Option<i64>,
    datum: &ListParWithRandom,
) -> ListParWithRandom {
    let mut pars = Vec::with_capacity(datum.pars.len() + 1);
    pars.push(pq_datum_envelope(ord, priority));
    pars.extend(datum.pars.iter().cloned());
    ListParWithRandom {
        pars,
        random_state: datum.random_state.clone(),
        cost_authority: datum.cost_authority.clone(),
        cost_stack: datum.cost_stack.clone(),
    }
}

/// The order and the level of a datum record of a priority queue. Only the envelope is decoded.
/// The priority slot holds `GInt(p)` with `p ≥ 0` or `Nil`, and the other slots are the slots
/// that [`datum_ord`] accepts.
pub fn pq_datum_slots(record: &ListParWithRandom) -> Result<(i64, Option<i64>), MalformedRecord> {
    let malformed = MalformedRecord(RecordKind::Datum);
    match record.pars.first().and_then(list_elements) {
        Some([tag, ord, priority, embedding])
            if is_tag(tag, RecordKind::Datum) && is_nil(embedding) =>
        {
            match (natural_of(ord), level_of_slot(priority)) {
                (Some(ord), Some(level)) => Ok((ord, level)),
                (None, _) | (_, None) => Err(malformed),
            }
        }
        _ => Err(malformed),
    }
}

/// A decoded datum record of a priority queue.
#[derive(Clone, Debug, PartialEq)]
pub struct PqDatumRecord {
    /// The order of the record in its collection.
    pub ord: i64,
    /// The level of the record: `Some(p)` for the level `p`, and `None` for the unprioritized
    /// level.
    pub priority: Option<i64>,
    /// The datum as the program sent it: the payload, its random state, its cost authority and
    /// its cost stack.
    pub datum: ListParWithRandom,
}

/// Decodes a datum record of a priority queue (the inverse of [`wrap_pq_datum`]).
pub fn unwrap_pq_datum(record: &ListParWithRandom) -> Result<PqDatumRecord, MalformedRecord> {
    let (ord, priority) = pq_datum_slots(record)?;
    let payload = record
        .pars
        .get(1..)
        .ok_or(MalformedRecord(RecordKind::Datum))?;
    Ok(PqDatumRecord {
        ord,
        priority,
        datum: ListParWithRandom {
            pars: payload.to_vec(),
            random_state: record.random_state.clone(),
            cost_authority: record.cost_authority.clone(),
            cost_stack: record.cost_stack.clone(),
        },
    })
}

/// The policy of a parked continuation of a priority queue: `EMap{"priority": GInt(p)}` for a
/// receive that waits in the waiter queue of the level `p`, and `Nil` for an unrestricted
/// receive. The map is the canonical map of one entry: a ground key and a ground value give it
/// empty bit sets, as `ParMap::create_from_vec` makes them.
pub fn pq_policy(priority: Option<i64>) -> Par {
    match priority {
        Some(level) => expr_par(ExprInstance::EMapBody(EMap {
            kvs: vec![KeyValuePair {
                key: Some(expr_par(ExprInstance::GString(PRIORITY_KEY.to_owned()))),
                value: Some(int_par(level)),
            }],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
        None => Par::default(),
    }
}

/// The level of a policy slot: `Some(None)` for `Nil`, `Some(Some(p))` for the exact image of
/// [`pq_policy`] with `p ≥ 0`, and `None` for every other value. The decoder reads the structure
/// and builds nothing: one entry, the key `"priority"` without a flag, a natural value, and no
/// remainder and no flag on the map or on its `Par`.
fn level_of_policy(policy: &Par) -> Option<Option<i64>> {
    match is_nil(policy) {
        true => Some(None),
        false => match (sole_expr(policy), policy.connective_used) {
            (
                Some(ExprInstance::EMapBody(EMap {
                    kvs,
                    connective_used: false,
                    remainder: None,
                    ..
                })),
                false,
            ) => match kvs.as_slice() {
                [KeyValuePair {
                    key: Some(key),
                    value: Some(value),
                }] => match (sole_expr(key), key.connective_used) {
                    (Some(ExprInstance::GString(name)), false) if name == PRIORITY_KEY => {
                        natural_of(value).map(Some)
                    }
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        },
    }
}

/// The envelope of a parked continuation of a priority queue with the order `ord` and the
/// policy of `priority` around `body`.
pub fn pq_continuation_envelope(ord: i64, priority: Option<i64>, body: Par) -> Par {
    list_par(vec![
        RecordKind::Continuation.tag(),
        int_par(ord),
        pq_policy(priority),
        body,
    ])
}

/// The parked continuation of `continuation` in a priority queue, with the order `ord` and the
/// policy of `priority`: its body inside the envelope. The random state, the guard and the cost
/// authority stay as they are. Only a `ParBody` with a body has a continuation record.
pub fn wrap_pq_continuation(
    ord: i64,
    priority: Option<i64>,
    continuation: &TaggedContinuation,
) -> Result<TaggedContinuation, MalformedRecord> {
    match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(body),
            random_state,
        })) => Ok(TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(pq_continuation_envelope(ord, priority, body.clone())),
                random_state: random_state.clone(),
            })),
            guard: continuation.guard.clone(),
            cost_authority: continuation.cost_authority.clone(),
        }),
        Some(TaggedCont::ParBody(ParWithRandom { body: None, .. }))
        | Some(TaggedCont::ScalaBodyRef(_))
        | None => Err(MalformedRecord(RecordKind::Continuation)),
    }
}

/// The order, the level of the policy and the original body of a parked continuation of a
/// priority queue. The policy slot holds the policy of [`pq_policy`] or `Nil`, and the other
/// slots are the slots that [`continuation_ord`] accepts.
fn pq_continuation_parts(
    continuation: &TaggedContinuation,
) -> Result<(i64, Option<i64>, &Par, &ParWithRandom), MalformedRecord> {
    let malformed = MalformedRecord(RecordKind::Continuation);
    let body = match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(body)) => body,
        Some(TaggedCont::ScalaBodyRef(_)) | None => return Err(malformed),
    };
    match body.body.as_ref().and_then(list_elements) {
        Some([tag, ord, policy, original]) if is_tag(tag, RecordKind::Continuation) => {
            match (natural_of(ord), level_of_policy(policy)) {
                (Some(ord), Some(level)) => Ok((ord, level, original, body)),
                (None, _) | (_, None) => Err(malformed),
            }
        }
        _ => Err(malformed),
    }
}

/// The order and the level of the policy of a parked continuation of a priority queue. Only the
/// envelope is decoded.
pub fn pq_continuation_slots(
    continuation: &TaggedContinuation,
) -> Result<(i64, Option<i64>), MalformedRecord> {
    pq_continuation_parts(continuation).map(|(ord, level, _, _)| (ord, level))
}

/// A decoded continuation record of a priority queue.
#[derive(Clone, Debug, PartialEq)]
pub struct PqContinuationRecord {
    /// The order of the continuation in its collection.
    pub ord: i64,
    /// The level of its waiter queue: `Some(p)` for the level `p`, and `None` for the
    /// unrestricted queue.
    pub priority: Option<i64>,
    /// The continuation as the program parked it: the original body, its random state, its
    /// guard and its cost authority.
    pub continuation: TaggedContinuation,
}

/// Decodes a parked continuation of a priority queue (the inverse of [`wrap_pq_continuation`]).
pub fn unwrap_pq_continuation(
    continuation: &TaggedContinuation,
) -> Result<PqContinuationRecord, MalformedRecord> {
    let (ord, priority, original, body) = pq_continuation_parts(continuation)?;
    Ok(PqContinuationRecord {
        ord,
        priority,
        continuation: TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(original.clone()),
                random_state: body.random_state.clone(),
            })),
            guard: continuation.guard.clone(),
            cost_authority: continuation.cost_authority.clone(),
        },
    })
}

/// The parked continuation of a receive of a priority queue with the order `ord` and the policy
/// of `priority`, built from its parts with one copy of each part. It equals
/// [`wrap_pq_continuation`] of the continuation with these parts.
pub fn wrap_pq_continuation_from_parts(
    ord: i64,
    priority: Option<i64>,
    body: &ParWithRandom,
    guard: &Option<Par>,
    cost_authority: Option<&CostAuthority>,
) -> Result<TaggedContinuation, SpaceError> {
    let Some(original) = &body.body else {
        return Err(MalformedRecord(RecordKind::Continuation).into());
    };
    let cost_authority = cost_authority.cloned();
    Ok(TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(pq_continuation_envelope(ord, priority, original.clone())),
            random_state: body.random_state.clone(),
        })),
        guard: guard.clone(),
        cost_authority,
    })
}

// ── Waiter policies and the records of a VectorDB space (P6, design v3 4.0 and 4.3) ───────
//
// The policy slot of a parked waiter (version 1) and of a join slot (version 2) holds one of
// three values. A priority queue writes `Nil` or its level map (P5b, decision 38). A VectorDB
// waiter writes `Nil` for a receive without a query, or the canonical map of its query: the
// query vector under `q` and the named params of the receive, one entry for each present key.
// The decoder of the policy is strict: it accepts `Nil`, the exact image of `pq_policy`, and a
// Changed by decision 128 (kind routing): the registry gives the policy keys. The text before:
// ```
// // non-empty map whose keys are in `VDB_QUERY_KEYS` and that equals its own re-encoding. The two
// // maps cannot be confused, because `"priority"` is not a query key.
// ```
// non-empty map whose keys are policy keys (`families::is_policy_key`) and that equals its own
// re-encoding. The two maps cannot be confused, because `"priority"` is not a query key.
//
// A datum of a VectorDB space keeps envelope version 1 and fills the embedding slot: the vector
// of the record, a ground list, or `Nil` for a record without one. The agent checks the
// elements of the vector before it stores the record, so the codec checks only the shape.

// Changed by decision 128 (kind routing): the registry gives the keys (`families::is_policy_key`):
// the query vector `q` and the query params of the families with the similarity window. Without a
// plug-in they are the 8 keys below. The text before:
// ```
// /// The keys that the query map of a VectorDB waiter may hold (P6): the query vector `q` and the
// /// named params of the receive, in bytewise order.
// pub const VDB_QUERY_KEYS: [&str; 8] = [
//     "ids",
//     "k",
//     "metric",
//     "q",
//     "radius",
//     "results",
//     "scores",
//     "threshold",
// ];
// ```

/// The policy slot of a parked waiter (version 1) and of a join slot (version 2).
///
/// The query map is boxed, so the policy has the size of the `Option<i64>` that the slot held
/// before P6.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum WaiterPolicy {
    /// `Nil`: a waiter without a policy.
    #[default]
    None,
    /// The waiter queue of the level `p` of a priority queue (P5b, decision 38): the exact image
    /// of [`pq_policy`] with `p ≥ 0`.
    Level(i64),
    /// The query of a waiter of the similarity window (P6): one value for each present policy key
    /// (`families::is_policy_key`), as the agent checked it. Changed by decision 128 (kind
    /// routing): the registry gives the keys. The text before: "The query of a VectorDB waiter
    /// (P6): one value for each present key of `VDB_QUERY_KEYS`".
    Query(Box<BTreeMap<String, Par>>),
}

impl WaiterPolicy {
    /// The policy of a priority-queue lane: `Level(p)` for `Some(p)` and `None` for `None`.
    pub fn of_level(level: Option<i64>) -> Self {
        match level {
            Some(level) => WaiterPolicy::Level(level),
            None => WaiterPolicy::None,
        }
    }

    /// The level of a priority-queue policy, and `None` for every other policy.
    pub fn level(&self) -> Option<i64> {
        match self {
            WaiterPolicy::Level(level) => Some(*level),
            WaiterPolicy::None | WaiterPolicy::Query(_) => None,
        }
    }

    /// The query map of a VectorDB policy, and `None` for every other policy.
    pub fn query(&self) -> Option<&BTreeMap<String, Par>> {
        match self {
            WaiterPolicy::Query(entries) => Some(entries),
            WaiterPolicy::None | WaiterPolicy::Level(_) => None,
        }
    }
}

/// The par of a policy: `Nil`, the map of [`pq_policy`], or the canonical query map, whose
/// entries `ParMap` sorts.
pub fn policy_par(policy: &WaiterPolicy) -> Par {
    match policy {
        WaiterPolicy::None => Par::default(),
        WaiterPolicy::Level(level) => pq_policy(Some(*level)),
        WaiterPolicy::Query(entries) => map_par(
            entries
                .iter()
                .map(|(key, value)| (string_par(key), value.clone()))
                .collect(),
        ),
    }
}

/// The policy of a policy slot: `Nil`, the exact image of [`pq_policy`] with a natural level,
/// or a non-empty map whose keys are policy keys (`families::is_policy_key`) and that equals
/// its re-encoding. Every other value is `None`. Changed by decision 128 (kind routing): the
/// registry gives the keys. The text before: "whose keys are in `VDB_QUERY_KEYS`".
pub fn policy_of(par: &Par) -> Option<WaiterPolicy> {
    match level_of_policy(par) {
        Some(Some(level)) => return Some(WaiterPolicy::Level(level)),
        Some(None) => return Some(WaiterPolicy::None),
        None => {}
    }
    let entries = map_entries(par)?;
    // Changed by decision 128 (kind routing): the registry gives the policy keys. The text before:
    // if entries.is_empty() || !entries.keys().all(|key| VDB_QUERY_KEYS.contains(key)) {
    if entries.is_empty() || !entries.keys().all(|key| families::is_policy_key(key)) {
        return None;
    }
    let policy = WaiterPolicy::Query(Box::new(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.clone()))
            .collect(),
    ));
    (policy_par(&policy) == *par).then_some(policy)
}

/// The envelope of a parked continuation with the order `ord` and the policy `policy` around
/// `body`. With `Nil` it is [`continuation_envelope`], and with a level it is
/// [`pq_continuation_envelope`].
pub fn policy_continuation_envelope(ord: i64, policy: &WaiterPolicy, body: Par) -> Par {
    list_par(vec![
        RecordKind::Continuation.tag(),
        int_par(ord),
        policy_par(policy),
        body,
    ])
}

/// The parked continuation of `continuation` with the order `ord` and the policy `policy`: its
/// body inside the envelope. The random state, the guard and the cost authority stay as they
/// are. Only a `ParBody` with a body has a continuation record.
pub fn wrap_policy_continuation(
    ord: i64,
    policy: &WaiterPolicy,
    continuation: &TaggedContinuation,
) -> Result<TaggedContinuation, MalformedRecord> {
    match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(body),
            random_state,
        })) => Ok(TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(policy_continuation_envelope(ord, policy, body.clone())),
                random_state: random_state.clone(),
            })),
            guard: continuation.guard.clone(),
            cost_authority: continuation.cost_authority.clone(),
        }),
        Some(TaggedCont::ParBody(ParWithRandom { body: None, .. }))
        | Some(TaggedCont::ScalaBodyRef(_))
        | None => Err(MalformedRecord(RecordKind::Continuation)),
    }
}

/// The order, the policy and the original body of a parked continuation of version 1. The
/// policy slot holds a value that [`policy_of`] accepts, and the other slots are the slots that
/// [`continuation_ord`] accepts.
fn policy_continuation_parts(
    continuation: &TaggedContinuation,
) -> Result<(i64, WaiterPolicy, &Par, &ParWithRandom), MalformedRecord> {
    let malformed = MalformedRecord(RecordKind::Continuation);
    let body = match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(body)) => body,
        Some(TaggedCont::ScalaBodyRef(_)) | None => return Err(malformed),
    };
    match body.body.as_ref().and_then(list_elements) {
        Some([tag, ord, policy, original]) if is_tag(tag, RecordKind::Continuation) => {
            match (natural_of(ord), policy_of(policy)) {
                (Some(ord), Some(policy)) => Ok((ord, policy, original, body)),
                (None, _) | (_, None) => Err(malformed),
            }
        }
        _ => Err(malformed),
    }
}

/// The order and the policy of a parked continuation of version 1. Only the envelope is
/// decoded.
pub fn continuation_policy_slots(
    continuation: &TaggedContinuation,
) -> Result<(i64, WaiterPolicy), MalformedRecord> {
    policy_continuation_parts(continuation).map(|(ord, policy, _, _)| (ord, policy))
}

/// A decoded parked continuation of version 1 with its policy.
#[derive(Clone, Debug, PartialEq)]
pub struct PolicyContinuationRecord {
    /// The order of the continuation in its collection.
    pub ord: i64,
    /// The policy of the waiter.
    pub policy: WaiterPolicy,
    /// The continuation as the program parked it: the original body, its random state, its
    /// guard and its cost authority.
    pub continuation: TaggedContinuation,
}

/// Decodes a parked continuation of version 1 with its policy (the inverse of
/// [`wrap_policy_continuation`]).
pub fn unwrap_policy_continuation(
    continuation: &TaggedContinuation,
) -> Result<PolicyContinuationRecord, MalformedRecord> {
    let (ord, policy, original, body) = policy_continuation_parts(continuation)?;
    Ok(PolicyContinuationRecord {
        ord,
        policy,
        continuation: TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(original.clone()),
                random_state: body.random_state.clone(),
            })),
            guard: continuation.guard.clone(),
            cost_authority: continuation.cost_authority.clone(),
        },
    })
}

/// [`wrap_policy_continuation`] built from the parts of a receive, with one copy of each part.
pub fn wrap_policy_continuation_from_parts(
    ord: i64,
    policy: &WaiterPolicy,
    body: &ParWithRandom,
    guard: &Option<Par>,
    cost_authority: Option<&CostAuthority>,
) -> Result<TaggedContinuation, SpaceError> {
    let Some(original) = &body.body else {
        return Err(MalformedRecord(RecordKind::Continuation).into());
    };
    let cost_authority = cost_authority.cloned();
    Ok(TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(policy_continuation_envelope(ord, policy, original.clone())),
            random_state: body.random_state.clone(),
        })),
        guard: guard.clone(),
        cost_authority,
    })
}

/// True when `par` can fill the embedding slot of a datum of a VectorDB space: one list without
/// a remainder, free variables or connectives. The agent checks its elements first.
fn is_embedding(par: &Par) -> bool {
    list_elements(par).is_some() && !par.connective_used && par.locally_free.is_empty()
}

/// The envelope of a datum of a VectorDB space with the order `ord` and the embedding
/// `embedding`, or `Nil` without one. Without an embedding it is [`datum_envelope`].
pub fn vdb_datum_envelope(ord: i64, embedding: Option<&Par>) -> Par {
    list_par(vec![
        RecordKind::Datum.tag(),
        int_par(ord),
        Par::default(),
        embedding.cloned().unwrap_or_default(),
    ])
}

/// The datum record of `datum` in a VectorDB space, with the order `ord` and the embedding
/// `embedding`: the envelope, then the payload. The random state, the cost authority and the
/// cost stack of `datum` stay as they are. An embedding that is not a ground list is rejected.
pub fn wrap_vdb_datum(
    ord: i64,
    embedding: Option<&Par>,
    datum: &ListParWithRandom,
) -> Result<ListParWithRandom, MalformedRecord> {
    if embedding.is_some_and(|vector| !is_embedding(vector)) {
        return Err(MalformedRecord(RecordKind::Datum));
    }
    let mut pars = Vec::with_capacity(datum.pars.len() + 1);
    pars.push(vdb_datum_envelope(ord, embedding));
    pars.extend(datum.pars.iter().cloned());
    Ok(ListParWithRandom {
        pars,
        random_state: datum.random_state.clone(),
        cost_authority: datum.cost_authority.clone(),
        cost_stack: datum.cost_stack.clone(),
    })
}

/// The order and the embedding of a datum record of a VectorDB space. Only the envelope is
/// decoded. The priority slot holds `Nil`, and the embedding slot holds `Nil` or a ground list.
pub fn vdb_datum_slots(record: &ListParWithRandom) -> Result<(i64, Option<&Par>), MalformedRecord> {
    let malformed = MalformedRecord(RecordKind::Datum);
    match record.pars.first().and_then(list_elements) {
        Some([tag, ord, priority, embedding])
            if is_tag(tag, RecordKind::Datum) && is_nil(priority) =>
        {
            let embedding = match is_nil(embedding) {
                true => None,
                false if is_embedding(embedding) => Some(embedding),
                false => return Err(malformed),
            };
            natural_of(ord).map(|ord| (ord, embedding)).ok_or(malformed)
        }
        _ => Err(malformed),
    }
}

/// A decoded datum record of a VectorDB space.
#[derive(Clone, Debug, PartialEq)]
pub struct VdbDatumRecord {
    /// The order of the record in its collection.
    pub ord: i64,
    /// The vector of the record, or `None` for a record without one.
    pub embedding: Option<Par>,
    /// The datum as the program sent it: the payload, its random state, its cost authority and
    /// its cost stack.
    pub datum: ListParWithRandom,
}

/// Decodes a datum record of a VectorDB space (the inverse of [`wrap_vdb_datum`]).
pub fn unwrap_vdb_datum(record: &ListParWithRandom) -> Result<VdbDatumRecord, MalformedRecord> {
    let (ord, embedding) = vdb_datum_slots(record)?;
    let payload = record
        .pars
        .get(1..)
        .ok_or(MalformedRecord(RecordKind::Datum))?;
    Ok(VdbDatumRecord {
        ord,
        embedding: embedding.cloned(),
        datum: ListParWithRandom {
            pars: payload.to_vec(),
            random_state: record.random_state.clone(),
            cost_authority: record.cost_authority.clone(),
            cost_stack: record.cost_stack.clone(),
        },
    })
}

// ── The continuation record of a join (P5c, decision 29) ──────────────────────────────────
//
// A waiter with two or more binds parks on the group of its channels in canonical order
// (FR-1', decision 28). Its record is version 2 of the continuation envelope:
//
//   ParBody.body = Par{exprs: [EList[tag_K2, EList[slot_0 … slot_{n-1}], body]]}
//   tag_K2       = GByteArray([0xE0, 0x52, b'K', 2])
//   slot_i       = EList[GInt(ord_i), GInt(bind_i), policy_i]
//
// The slot `i` belongs to the group position `i`. `ord_i` is the order of the waiter in the
// collection of that position, `bind_i` is the index of the bind of that position in the
// receive, and `policy_i` is `Nil` or the policy of a priority-queue lane (`pq_policy`). The
// bind indices are a permutation of `0..n`, so the outcome of a fire puts the results of its
// receipt, which follow the group order, back into the bind order of the receive. A waiter with
// one bind keeps the version 1 record. The decoders of each version reject the other version,
// because they compare the whole tag.

/// The envelope version of the continuation record of a join.
pub const JOIN_CONTINUATION_VERSION: u8 = 2;

/// The bytes of the tag of the continuation record of a join.
pub const JOIN_CONTINUATION_TAG: [u8; 4] = [
    REIFIED_TAG,
    RECORD_TAG_MARK,
    RecordKind::Continuation as u8,
    JOIN_CONTINUATION_VERSION,
];

/// The slot of one group position of a join waiter.
///
/// The slot was `Copy` with a level `priority: Option<i64>` until P6. The policy of P6 also holds
/// the query of a VectorDB bind (design v3 4.0), boxed, so the slot keeps its size.
#[derive(Clone, Debug, PartialEq)]
pub struct JoinSlot {
    /// The order of the waiter in the collection of the position. Repeated channels of one
    /// collection have equal orders, and a bag or a set gives the order 0.
    pub ord: i64,
    /// The index of the bind of the position in the receive.
    pub bind: usize,
    /// The policy of the bind: the level of its waiter queue in a priority queue, the query of a
    /// VectorDB bind, and `None` for the unrestricted queue and for every other discipline.
    pub policy: WaiterPolicy,
}

/// A decoded continuation record of a join.
#[derive(Clone, Debug, PartialEq)]
pub struct JoinContinuationRecord {
    /// The slots of the group positions, in group order.
    pub slots: Vec<JoinSlot>,
    /// The continuation as the program parked it: the original body, its random state, its
    /// guard and its cost authority.
    pub continuation: TaggedContinuation,
}

fn join_tag() -> Par { expr_par(ExprInstance::GByteArray(JOIN_CONTINUATION_TAG.to_vec())) }

fn is_join_tag(par: &Par) -> bool {
    match (sole_expr(par), par.connective_used) {
        (Some(ExprInstance::GByteArray(bytes)), false) => bytes.as_slice() == JOIN_CONTINUATION_TAG,
        _ => false,
    }
}

/// True when `slots` is the slot list of a join record: two or more slots, natural orders,
/// natural levels, and bind indices that are a permutation of `0..slots.len()`. The check of the
/// permutation allocates one flag for each slot.
fn join_slots_valid(slots: &[JoinSlot]) -> bool {
    let mut seen = vec![false; slots.len()];
    slots.len() >= 2
        && slots.iter().all(|slot| {
            let level = slot.policy.level().is_none_or(|level| level >= 0);
            let fresh = match seen.get_mut(slot.bind) {
                Some(flag @ false) => {
                    *flag = true;
                    true
                }
                Some(true) | None => false,
            };
            slot.ord >= 0 && level && fresh
        })
}

/// The par of one slot. A checked slot list has its bind indices below its length, so each
/// index fits an `i64`.
fn join_slot_par(slot: &JoinSlot) -> Result<Par, MalformedRecord> {
    let bind = i64::try_from(slot.bind).map_err(|_| MalformedRecord(RecordKind::Continuation))?;
    Ok(list_par(vec![
        int_par(slot.ord),
        int_par(bind),
        policy_par(&slot.policy),
    ]))
}

/// The envelope of a checked slot list around `body`.
fn join_envelope_of(slots: &[JoinSlot], body: Par) -> Result<Par, MalformedRecord> {
    let slot_pars = slots
        .iter()
        .map(join_slot_par)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(list_par(vec![join_tag(), list_par(slot_pars), body]))
}

/// The envelope of the continuation record of a join with the slots `slots` around `body`.
/// The encoder accepts exactly the slot lists that its decoder returns: two or more slots,
/// natural orders and levels, and bind indices that are a permutation of `0..n`.
pub fn join_continuation_envelope(slots: &[JoinSlot], body: Par) -> Result<Par, MalformedRecord> {
    match join_slots_valid(slots) {
        true => join_envelope_of(slots, body),
        false => Err(MalformedRecord(RecordKind::Continuation)),
    }
}

/// The parked continuation of `continuation` as a join waiter with the slots `slots`: its body
/// inside the envelope. The random state, the guard and the cost authority stay as they are.
/// Only a `ParBody` with a body has a continuation record.
pub fn wrap_join_continuation(
    slots: &[JoinSlot],
    continuation: &TaggedContinuation,
) -> Result<TaggedContinuation, MalformedRecord> {
    match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(body),
            random_state,
        })) => Ok(TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(join_continuation_envelope(slots, body.clone())?),
                random_state: random_state.clone(),
            })),
            guard: continuation.guard.clone(),
            cost_authority: continuation.cost_authority.clone(),
        }),
        Some(TaggedCont::ParBody(ParWithRandom { body: None, .. }))
        | Some(TaggedCont::ScalaBodyRef(_))
        | None => Err(MalformedRecord(RecordKind::Continuation)),
    }
}

/// The slot of the par `par`, for a record with `count` slots.
fn join_slot_of(par: &Par, count: usize) -> Option<JoinSlot> {
    match list_elements(par) {
        Some([ord, bind, policy]) => {
            let ord = natural_of(ord)?;
            let bind = usize::try_from(natural_of(bind)?).ok()?;
            let policy = policy_of(policy)?;
            (bind < count).then_some(JoinSlot { ord, bind, policy })
        }
        _ => None,
    }
}

/// The slots and the original body of the continuation record of a join.
fn join_continuation_parts(
    continuation: &TaggedContinuation,
) -> Result<(Vec<JoinSlot>, &Par, &ParWithRandom), MalformedRecord> {
    let malformed = MalformedRecord(RecordKind::Continuation);
    let body = match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(body)) => body,
        Some(TaggedCont::ScalaBodyRef(_)) | None => return Err(malformed),
    };
    match body.body.as_ref().and_then(list_elements) {
        Some([tag, slot_list, original]) if is_join_tag(tag) => {
            let slot_pars = list_elements(slot_list).ok_or(malformed)?;
            let slots = slot_pars
                .iter()
                .map(|par| join_slot_of(par, slot_pars.len()))
                .collect::<Option<Vec<_>>>()
                .ok_or(malformed)?;
            match join_slots_valid(&slots) {
                true => Ok((slots, original, body)),
                false => Err(malformed),
            }
        }
        _ => Err(malformed),
    }
}

/// The slots of the continuation record of a join. Only the envelope is decoded.
pub fn join_continuation_slots(
    continuation: &TaggedContinuation,
) -> Result<Vec<JoinSlot>, MalformedRecord> {
    join_continuation_parts(continuation).map(|(slots, _, _)| slots)
}

/// Decodes the continuation record of a join (the inverse of [`wrap_join_continuation`]).
pub fn unwrap_join_continuation(
    continuation: &TaggedContinuation,
) -> Result<JoinContinuationRecord, MalformedRecord> {
    let (slots, original, body) = join_continuation_parts(continuation)?;
    Ok(JoinContinuationRecord {
        slots,
        continuation: TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(original.clone()),
                random_state: body.random_state.clone(),
            })),
            guard: continuation.guard.clone(),
            cost_authority: continuation.cost_authority.clone(),
        },
    })
}

/// Decodes an owned continuation record of a join. It moves the original body, the random
/// state, the guard and the cost authority out of the record, as [`into_continuation`] does.
pub fn into_join_continuation(
    continuation: TaggedContinuation,
) -> Result<JoinContinuationRecord, MalformedRecord> {
    let malformed = MalformedRecord(RecordKind::Continuation);
    let slots = join_continuation_slots(&continuation)?;
    let TaggedContinuation {
        tagged_cont,
        guard,
        cost_authority,
    } = continuation;
    let ParWithRandom { body, random_state } = match tagged_cont {
        Some(TaggedCont::ParBody(body)) => body,
        Some(TaggedCont::ScalaBodyRef(_)) | None => return Err(malformed),
    };
    let original = body
        .and_then(|mut envelope| match envelope.exprs.pop() {
            Some(Expr {
                expr_instance: Some(ExprInstance::EListBody(EList { mut ps, .. })),
            }) => ps.pop(),
            _ => None,
        })
        .ok_or(malformed)?;
    Ok(JoinContinuationRecord {
        slots,
        continuation: TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(original),
                random_state,
            })),
            guard,
            cost_authority,
        },
    })
}

/// The results of a join fire in the bind order of its receive. The receipt lists one result
/// for each group position, in group order, and the slot of position `i` names its bind. The
/// host dispatches the results of a join in bind order (`dispatch.rs`), so the outcome puts
/// result `i` at the place `slots[i].bind`.
pub fn restore_bind_order<T>(slots: &[JoinSlot], results: Vec<T>) -> Result<Vec<T>, SpaceError> {
    if slots.len() != results.len() {
        return Err(SpaceError::Inconsistent(
            "a join fire has not one result for each slot",
        ));
    }
    let mut placed: Vec<Option<T>> = Vec::with_capacity(results.len());
    placed.resize_with(results.len(), || None);
    for (slot, result) in slots.iter().zip(results) {
        match placed.get_mut(slot.bind) {
            Some(place @ None) => *place = Some(result),
            Some(Some(_)) | None => {
                return Err(SpaceError::Inconsistent(
                    "the slots of a join fire are not a permutation of its binds",
                ))
            }
        }
    }
    placed
        .into_iter()
        .map(|result| {
            result.ok_or(SpaceError::Inconsistent(
                "the slots of a join fire are not a permutation of its binds",
            ))
        })
        .collect()
}

/// The parked continuation of a receive with two or more binds and the slots `slots`, built
/// from its parts with one copy of each part. It equals [`wrap_join_continuation`] of the
/// continuation with these parts.
pub fn wrap_join_continuation_from_parts(
    slots: &[JoinSlot],
    body: &ParWithRandom,
    guard: &Option<Par>,
    cost_authority: Option<&CostAuthority>,
) -> Result<TaggedContinuation, SpaceError> {
    let Some(original) = &body.body else {
        return Err(MalformedRecord(RecordKind::Continuation).into());
    };
    if !join_slots_valid(slots) {
        return Err(MalformedRecord(RecordKind::Continuation).into());
    }
    let cost_authority = cost_authority.cloned();
    Ok(TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(join_envelope_of(slots, original.clone())?),
            random_state: body.random_state.clone(),
        })),
        guard: guard.clone(),
        cost_authority,
    })
}

/// The introduction authority of one operation and the phlo budget of its deploy. The staged
/// operations of the operation share it, so each callback copies one shared pointer instead of
/// the budget and the authority.
pub struct Introduction {
    budget: RuntimeBudget,
    authority: CostAuthority,
}

impl Introduction {
    /// The introduction of `authority` on `budget`.
    pub fn new(budget: &RuntimeBudget, authority: &CostAuthority) -> Arc<Introduction> {
        Arc::new(Introduction {
            budget: budget.clone(),
            authority: authority.clone(),
        })
    }

    /// The introduction authority.
    pub fn authority(&self) -> &CostAuthority { &self.authority }

    /// Added by decision 124 (plan v4 C7.9 and F11, ruling D-10): true when the deployment runs
    /// with owner funding (R11). Only then does a staged row carry a row search (section 2.4).
    pub fn owner_funded(&self) -> bool { self.budget.owner_binding().is_some() }

    /// The introduction of a re-stored record (decision 45, Rule E): the budget of this
    /// introduction and the record's own cost authority as the deployment of the budget resolves
    /// it, or the empty authority when the record has none. A Unit region of system residue that
    /// this deployment derived from the random state of the record resolves to the region of
    /// the payer, and every other region stays as stored (DR-101 of `feature/cost-accounted-rho`).
    /// The resolved region of the payer carries the region price of the running native
    /// execution (DR-135 R4), so it equals the region that the deployment opened.
    /// The host reducer's `produce_peeks` registers the same authority when it sends a peeked
    /// datum again. A seal that does not resolve is `Inconsistent`.
    pub fn for_record(&self, record: &ListParWithRandom) -> Result<Arc<Introduction>, SpaceError> {
        // Changed by decision 89 (DR-101): a restored record charges its seal as this deployment
        // resolves it, so the system residue of an earlier deployment stays free.
        // match &record.cost_authority {
        //     Some(authority) => Introduction::new(&self.budget, authority),
        //     None => Introduction::new(&self.budget, &CostAuthority::default()),
        // }
        const UNRESOLVED: &str = "the seal of a re-stored record does not resolve";
        match &record.cost_authority {
            Some(seal) => {
                // Changed by decision 107 (DR-135 R4): the resolved payer region carries the
                // region price of this deployment, as in `produce_peeks`.
                // let context =
                //     ResidueContext::new(&self.budget.signature(), self.budget.deploy_id())
                //         .map_err(|_| SpaceError::Inconsistent(UNRESOLVED))?;
                let context =
                    ResidueContext::new(&self.budget.signature(), self.budget.deploy_id())
                        .map_err(|_| SpaceError::Inconsistent(UNRESOLVED))?
                        .with_price(self.budget.region_price());
                let authority = resolve_system_residue(seal, &record.random_state, &context)
                    .map_err(|_| SpaceError::Inconsistent(UNRESOLVED))?;
                Ok(Introduction::new(&self.budget, &authority))
            }
            None => Ok(Introduction::new(&self.budget, &CostAuthority::default())),
        }
    }

    /// Added by decision 124 (plan v2 C7.1, decision 99, rule CR-15): the seal of a machinery
    /// continuation or record of this deployment: the system residue R(d, P) of the authority
    /// `authority` of its payer P, as `residue_seal_of` builds it for a system body. A
    /// continuation without an authority stays without a seal.
    pub fn seal_of(
        &self,
        authority: Option<&CostAuthority>,
    ) -> Result<Option<CostAuthority>, SpaceError> {
        let Some(authority) = authority else {
            return Ok(None);
        };
        system_residue_authority_with_caller(authority, &self.budget.deploy_id(), None)
            .map(Some)
            .map_err(|_| SpaceError::Inconsistent("the seal of a machinery row does not build"))
    }

    /// Added by decision 124 (plan v2 C7.1, decision 99, rule CR-15): the introduction of a
    /// machinery row. It carries no authority, so the payer pays, as `for_record` registers a
    /// record without a seal, and the row has no owner part.
    pub fn payer(&self) -> Arc<Introduction> {
        Introduction::new(&self.budget, &CostAuthority::default())
    }
}

/// The `before_observe` callback of a staged operation of the operation of `introduction`.
/// At commit, right before the observer sees the staged operation, it registers the
/// introduction authority under the identity of the staged operation, where the reducer's
/// `produce` and `consume` register it (P4 design B.7). A repeated identity, a re-offer or a
/// re-fire, registers its first authority again. Another authority fails the commit at that
/// operation. The callback copies one shared pointer.
pub fn register_introduction(introduction: &Arc<Introduction>) -> BeforeObserve {
    let introduction = Arc::clone(introduction);
    Box::new(move |source: RSpaceOperationSource<'_>| {
        let (identity, kind) = match source {
            RSpaceOperationSource::Produce(produce) => (
                produce_introduction_identity(produce),
                AuthorityByteEventKind::ProduceIntroduction,
            ),
            RSpaceOperationSource::Consume(consume) => (
                consume_introduction_identity(consume),
                AuthorityByteEventKind::ConsumeIntroduction,
            ),
        };
        introduction
            .budget
            .register_introduction_authority(identity, kind, &introduction.authority)
            .map_err(RSpaceError::from)
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use models::rhoapi::cost_signature::Value as CostSignatureValue;
    use models::rhoapi::{BindPattern, CostRegion, CostSignature, Var};
    use models::rust::utils::{new_boundvar_par, new_freevar_par, new_gstring_par};
    use rspace_plus_plus::rspace::internal::Datum;
    use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::merging::rholang_merging_logic::RholangMergingLogic;
    use crate::rust::interpreter::spaces::namespace::{CollectionKey, Durability, NativeSpace};

    fn int(value: i64) -> Par { int_par(value) }

    fn string(value: &str) -> Par { new_gstring_par(value.to_string(), Vec::new(), false) }

    fn region(byte: u8) -> CostAuthority {
        CostAuthority {
            regions: vec![CostRegion {
                instance_id: vec![byte; 32],
                signature: Some(CostSignature {
                    value: Some(CostSignatureValue::Ground(vec![byte])),
                }),
                // Added by decision 90 (DR-135): an unpriced region, as outside native
                // execution.
                price: 0,
                caller: None,
            }],
        }
    }

    fn datum(pars: Vec<Par>) -> ListParWithRandom {
        ListParWithRandom {
            pars,
            random_state: vec![1, 2, 3],
            cost_authority: Some(region(4)),
            cost_stack: None,
        }
    }

    fn parked(body: Par) -> TaggedContinuation {
        TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(body),
                random_state: vec![5, 6],
            })),
            guard: Some(int(1)),
            cost_authority: Some(region(7)),
        }
    }

    fn with_list(par: &Par, change: impl FnOnce(&mut EList)) -> Par {
        let mut par = par.clone();
        if let Some(Expr {
            expr_instance: Some(ExprInstance::EListBody(list)),
        }) = par.exprs.first_mut()
        {
            change(list);
        }
        par
    }

    fn tag_with(bytes: [u8; 4]) -> Par { expr_par(ExprInstance::GByteArray(bytes.to_vec())) }

    fn malformed_datum(envelope: Par) -> ListParWithRandom {
        ListParWithRandom {
            pars: vec![envelope, int(1)],
            ..ListParWithRandom::default()
        }
    }

    #[test]
    fn space_record_datum_envelope_roundtrips_the_payload() {
        for payload in [Vec::new(), vec![int(1)], vec![
            string("a"),
            int(2),
            Par::default(),
        ]] {
            let original = datum(payload.clone());
            let record = wrap_datum(9, &original);
            assert_eq!(record.pars.len(), payload.len() + 1);
            assert_eq!(record.pars[0], datum_envelope(9));
            assert_eq!(&record.pars[1..], payload.as_slice());
            assert_eq!(datum_ord(&record), Ok(9));
            assert_eq!(
                unwrap_datum(&record),
                Ok(DatumRecord {
                    ord: 9,
                    datum: original.clone(),
                })
            );
        }
        let mut stacked = datum(vec![int(1)]);
        stacked.cost_stack = Some(Default::default());
        assert_eq!(
            unwrap_datum(&wrap_datum(0, &stacked)).map(|record| record.datum),
            Ok(stacked)
        );
        assert_ne!(
            wrap_datum(1, &datum(vec![int(1)])),
            wrap_datum(2, &datum(vec![int(1)]))
        );
        assert_ne!(
            wrap_datum(1, &datum(vec![int(1)])),
            wrap_datum(1, &datum(vec![int(2)]))
        );
        assert_eq!(
            datum_ord(&wrap_datum(i64::MAX, &datum(Vec::new()))),
            Ok(i64::MAX)
        );
    }

    #[test]
    fn space_record_continuation_envelope_roundtrips_the_body() {
        let mut body = new_boundvar_par(0, Vec::new(), false);
        body.exprs.extend(new_freevar_par(1, Vec::new()).exprs);
        body.connective_used = true;
        let original = parked(body.clone());
        let wrapped = wrap_continuation(4, &original).expect("a body has a continuation record");
        let envelope = match &wrapped.tagged_cont {
            Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(envelope),
                random_state,
            })) => {
                assert_eq!(random_state, &vec![5, 6]);
                envelope.clone()
            }
            other => panic!("a wrapped continuation keeps its ParBody: {other:?}"),
        };
        assert_eq!(envelope, continuation_envelope(4, body.clone()));
        assert!(envelope.connective_used);
        assert_eq!(envelope.locally_free, body.locally_free);
        assert_eq!(wrapped.guard, original.guard);
        assert_eq!(wrapped.cost_authority, original.cost_authority);
        assert_eq!(continuation_ord(&wrapped), Ok(4));
        assert_eq!(
            unwrap_continuation(&wrapped),
            Ok(ContinuationRecord {
                ord: 4,
                continuation: original.clone(),
            })
        );
        assert_ne!(
            wrap_continuation(4, &original),
            wrap_continuation(5, &original)
        );
        let malformed = Some(MalformedRecord(RecordKind::Continuation));
        let bodyless = TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: None,
                random_state: Vec::new(),
            })),
            ..original.clone()
        };
        let native = TaggedContinuation {
            tagged_cont: Some(TaggedCont::ScalaBodyRef(201)),
            ..original.clone()
        };
        for continuation in [bodyless, native, TaggedContinuation::default()] {
            assert_eq!(wrap_continuation(0, &continuation).err(), malformed);
            assert_eq!(unwrap_continuation(&continuation).err(), malformed);
        }
        assert_eq!(unwrap_continuation(&original).err(), malformed);
    }

    #[test]
    fn space_record_counter_and_config_records_roundtrip() {
        let random = Blake2b512Random::create_from_bytes(b"counter");
        for value in [0, 1, 41, i64::MAX] {
            let record = counter_record(value, &random, Some(region(3)));
            assert_eq!(counter_value(&record), Ok(value));
            assert_eq!(record.random_state, random.to_bytes());
            assert_eq!(record.cost_authority, Some(region(3)));
            assert_eq!(record.cost_stack, None);
        }
        assert_ne!(
            counter_record(1, &random, None),
            counter_record(2, &random, None)
        );
        let map = string("a config map");
        let record = config_record(map.clone(), &random);
        assert_eq!(config_map(&record), Ok(&map));
        assert_eq!(record.random_state, random.to_bytes());
        assert_eq!(record.cost_authority, None);
    }

    #[test]
    fn space_record_kinds_have_distinct_tags_and_decoders() {
        let tags: std::collections::BTreeSet<[u8; 4]> = RecordKind::ALL
            .iter()
            .map(|kind| kind.tag_bytes())
            .collect();
        assert_eq!(tags.len(), RecordKind::ALL.len());
        for kind in RecordKind::ALL {
            assert_eq!(kind.tag_bytes(), [
                REIFIED_TAG,
                RECORD_TAG_MARK,
                kind as u8,
                RECORD_VERSION
            ]);
        }
        let random = Blake2b512Random::create_from_bytes(b"kinds");
        let datum_record = wrap_datum(1, &datum(vec![int(1)]));
        let counter = counter_record(1, &random, None);
        let config = config_record(int(1), &random);
        let continuation_as_datum = malformed_datum(continuation_envelope(1, int(1)));
        assert_eq!(
            counter_value(&datum_record),
            Err(MalformedRecord(RecordKind::Counter))
        );
        assert_eq!(
            config_map(&datum_record),
            Err(MalformedRecord(RecordKind::Config))
        );
        for record in [&counter, &config, &continuation_as_datum] {
            assert_eq!(datum_ord(record), Err(MalformedRecord(RecordKind::Datum)));
        }
        assert_eq!(
            counter_value(&config),
            Err(MalformedRecord(RecordKind::Counter))
        );
        assert_eq!(
            config_map(&counter),
            Err(MalformedRecord(RecordKind::Config))
        );
        let datum_as_continuation = parked(datum_envelope(1));
        assert_eq!(
            continuation_ord(&datum_as_continuation),
            Err(MalformedRecord(RecordKind::Continuation))
        );
    }

    #[test]
    fn space_record_no_datum_is_a_single_number() {
        let random = Blake2b512Random::create_from_bytes(b"numbers");
        let records = [
            wrap_datum(0, &datum(Vec::new())),
            wrap_datum(3, &datum(vec![int(5)])),
            counter_record(7, &random, None),
            config_record(int(8), &random),
        ];
        for record in &records {
            assert!(list_elements(&record.pars[0]).is_some());
            assert!(RholangMergingLogic::try_get_number_with_rnd(record).is_none());
            let stored = Datum {
                a: record.clone(),
                persist: false,
                source: Produce::create(&int(0), record, false),
            };
            let bytes = bincode::serialize(&stored).expect("a datum serializes");
            assert!(!RholangMergingLogic::encoded_datum_is_number(&bytes));
        }
        let number = ListParWithRandom {
            pars: vec![int(5)],
            ..ListParWithRandom::default()
        };
        let bare = Datum {
            source: Produce::create(&int(0), &number, false),
            a: number,
            persist: false,
        };
        let bytes = bincode::serialize(&bare).expect("a datum serializes");
        assert!(RholangMergingLogic::encoded_datum_is_number(&bytes));
    }

    #[test]
    fn space_record_decoders_reject_malformed_envelopes() {
        let malformed = Some(MalformedRecord(RecordKind::Datum));
        let envelope = datum_envelope(2);
        let remainder = with_list(&envelope, |list| {
            list.remainder = Some(Var {
                var_instance: Some(models::rhoapi::var::VarInstance::FreeVar(0)),
            })
        });
        let flag = with_list(&envelope, |list| list.connective_used = true);
        let mut outer_flag = envelope.clone();
        outer_flag.connective_used = true;
        let mut with_send = envelope.clone();
        with_send.sends.push(Default::default());
        let mut flagged_ord = int(2);
        flagged_ord.connective_used = true;
        for bad in [
            list_par(vec![
                tag_with([REIFIED_TAG, RECORD_TAG_MARK, b'D', 2]),
                int(2),
                Par::default(),
                Par::default(),
            ]),
            list_par(vec![
                tag_with([REIFIED_TAG, 0x53, b'D', 1]),
                int(2),
                Par::default(),
                Par::default(),
            ]),
            list_par(vec![
                RecordKind::Datum.tag(),
                int(-1),
                Par::default(),
                Par::default(),
            ]),
            list_par(vec![
                RecordKind::Datum.tag(),
                string("2"),
                Par::default(),
                Par::default(),
            ]),
            list_par(vec![
                RecordKind::Datum.tag(),
                flagged_ord,
                Par::default(),
                Par::default(),
            ]),
            list_par(vec![
                RecordKind::Datum.tag(),
                int(2),
                int(0),
                Par::default(),
            ]),
            list_par(vec![
                RecordKind::Datum.tag(),
                int(2),
                Par::default(),
                int(0),
            ]),
            list_par(vec![RecordKind::Datum.tag(), int(2), Par::default()]),
            list_par(vec![
                RecordKind::Datum.tag(),
                int(2),
                Par::default(),
                Par::default(),
                Par::default(),
            ]),
            remainder,
            flag,
            outer_flag,
            with_send,
            int(2),
            Par::default(),
        ] {
            assert_eq!(
                datum_ord(&malformed_datum(bad.clone())).err(),
                malformed,
                "{bad:?}"
            );
            assert_eq!(unwrap_datum(&malformed_datum(bad)).err(), malformed);
        }
        assert_eq!(datum_ord(&ListParWithRandom::default()).err(), malformed);

        let random = Blake2b512Random::create_from_bytes(b"malformed");
        let counter = counter_record(1, &random, None);
        let mut stacked = counter.clone();
        stacked.cost_stack = Some(Default::default());
        let mut extra = counter.clone();
        extra.pars.push(Par::default());
        let mut negative = counter.clone();
        negative.pars[1] = int(-4);
        for bad in [
            stacked,
            extra,
            negative,
            wrap_datum(1, &datum(vec![int(1)])),
        ] {
            assert_eq!(
                counter_value(&bad),
                Err(MalformedRecord(RecordKind::Counter))
            );
        }

        let config = config_record(int(1), &random);
        let mut signed = config.clone();
        signed.cost_authority = Some(region(1));
        let mut stacked = config.clone();
        stacked.cost_stack = Some(Default::default());
        let mut short = config.clone();
        short.pars.pop();
        for bad in [signed, stacked, short] {
            assert_eq!(config_map(&bad), Err(MalformedRecord(RecordKind::Config)));
        }

        let policy = list_par(vec![RecordKind::Continuation.tag(), int(1), int(0), int(9)]);
        let negative = list_par(vec![
            RecordKind::Continuation.tag(),
            int(-1),
            Par::default(),
            int(9),
        ]);
        for bad in [policy, negative] {
            assert_eq!(
                continuation_ord(&parked(bad)),
                Err(MalformedRecord(RecordKind::Continuation))
            );
        }
    }

    #[test]
    fn space_record_malformed_records_abort_as_inconsistent() {
        for kind in RecordKind::ALL {
            let error = SpaceError::from(MalformedRecord(kind));
            assert_eq!(error.code(), "Inconsistent");
            assert!(matches!(error, SpaceError::Inconsistent(_)));
        }
    }

    fn space() -> NativeSpace {
        NativeSpace {
            sid: [3; 32],
            durability: Durability::Durable,
        }
    }

    /// The introduction of `authority` on a fresh budget.
    fn introduction_of(authority: &CostAuthority) -> Arc<Introduction> {
        let budget = RuntimeBudget::new(Cost::create(1_000, "record test"));
        Introduction::new(&budget, authority)
    }

    #[test]
    fn space_record_operation_authorities_follow_b7() {
        assert_eq!(cost_authority(&CostAuthority::default()), None);
        assert_eq!(cost_authority(&region(2)), Some(region(2)));
        assert_eq!(stamped(&CostAuthority::default()), None);
        assert_eq!(stamped(&region(2)), Some(&region(2)));
        let produce = ProduceOp {
            space: space(),
            key: CollectionKey::from_bytes(vec![1]).expect("a key"),
            channel: int(1),
            data: datum(vec![int(1)]),
            persistent: false,
            params: BTreeMap::new(),
            authority: region(5),
            reissued_ord: None,
        };
        let sent = introduction_of(&produce.authority);
        let authority = OperationAuthority::of_produce(&produce, &sent);
        assert_eq!(authority.continuation, Some(&region(4)));
        assert_eq!(authority.introduction.authority(), &region(5));
        let consume = ConsumeOp {
            space: space(),
            key: CollectionKey::from_bytes(vec![1]).expect("a key"),
            channel: int(1),
            pattern: BindPattern::default(),
            body: ParWithRandom::default(),
            persistent: false,
            peek: false,
            guard: None,
            query: None,
            authority: region(6),
            introduction_authority: region(8),
            reissued_ord: None,
        };
        let received = introduction_of(&consume.introduction_authority);
        let authority = OperationAuthority::of_consume(&consume, &received);
        assert_eq!(authority.continuation, Some(&region(6)));
        assert_eq!(authority.introduction.authority(), &region(8));
        let unsigned = ConsumeOp {
            authority: CostAuthority::default(),
            ..consume
        };
        assert_eq!(
            OperationAuthority::of_consume(&unsigned, &received).continuation,
            None
        );
    }

    /// The `before_observe` callback of an operation with the authority `authority`.
    fn callback(budget: &RuntimeBudget, authority: &CostAuthority) -> BeforeObserve {
        let introduction = Introduction::new(budget, authority);
        register_introduction(&introduction)
    }

    #[test]
    fn space_record_introduction_registers_the_operation_authority() {
        let budget = RuntimeBudget::new(Cost::create(1_000, "record introduction"));
        let produce = Produce::create(&int(1), &datum(vec![int(2)]), false);
        let consume = Consume::create(
            &vec![int(1)],
            &vec![BindPattern::default()],
            &parked(int(3)),
            false,
        );
        let unscoped = callback(&budget, &region(1));
        assert_eq!(unscoped(RSpaceOperationSource::Produce(&produce)), Ok(()));

        let _scope = budget.enter_comm_accounting_scope();
        let first = callback(&budget, &region(1));
        assert_eq!(first(RSpaceOperationSource::Produce(&produce)), Ok(()));
        let produce_identity = produce_introduction_identity(&produce);
        assert_eq!(
            budget.introduction_authority(
                produce_identity,
                AuthorityByteEventKind::ProduceIntroduction
            ),
            Ok(region(1))
        );
        let repeated = callback(&budget, &region(1));
        assert_eq!(repeated(RSpaceOperationSource::Produce(&produce)), Ok(()));
        let conflicting = callback(&budget, &region(2));
        assert!(conflicting(RSpaceOperationSource::Produce(&produce)).is_err());
        assert_eq!(
            budget.introduction_authority(
                produce_identity,
                AuthorityByteEventKind::ProduceIntroduction
            ),
            Ok(region(1))
        );

        let consumer = callback(&budget, &region(9));
        assert_eq!(consumer(RSpaceOperationSource::Consume(&consume)), Ok(()));
        assert_eq!(
            budget.introduction_authority(
                consume_introduction_identity(&consume),
                AuthorityByteEventKind::ConsumeIntroduction
            ),
            Ok(region(9))
        );
    }

    /// Decision 89 (DR-101): a re-stored record charges its seal as the deployment of the budget
    /// resolves it, as the host reducer's `produce_peeks` does. Residue that this deployment
    /// sealed from the random state of the record resolves to the region of the payer. Another
    /// deployment, another payer and system work keep the stored seal, and a record without a
    /// cost authority gets the empty authority.
    #[test]
    fn space_record_restore_resolves_residue_only_in_its_deployment() {
        use crate::rust::interpreter::accounting::authority::{
            canonical_authority, cost_region, sig_to_cost_signature, system_residue_region,
        };
        use crate::rust::interpreter::accounting::Sig;

        let payer = Sig::Ground(vec![1; 33]);
        let entropy = b"rrs-residue-entropy".to_vec();
        let payer_region = cost_region(
            &sig_to_cost_signature(&payer).expect("a ground payer converts"),
            &entropy,
            0,
        )
        .expect("the region of the payer");
        let stored =
            canonical_authority(&CostAuthority {
                regions: vec![
                    system_residue_region(&payer_region, &[7; 32]).expect("a residue region")
                ],
            })
            .expect("a canonical residue seal");
        let paid = canonical_authority(&CostAuthority {
            regions: vec![payer_region],
        })
        .expect("a canonical payer seal");
        let record = ListParWithRandom {
            pars: vec![int(1)],
            random_state: entropy,
            cost_authority: Some(stored.clone()),
            cost_stack: None,
        };
        let budget = RuntimeBudget::new(Cost::create(1_000, "record residue"));
        let introduction = Introduction::new(&budget, &region(9));
        budget
            .set_deploy_id_funded([7; 32], payer.clone())
            .expect("a funded deployment");
        assert_eq!(
            introduction
                .for_record(&record)
                .expect("the seal resolves")
                .authority(),
            &paid
        );
        for (deploy_id, other) in [([8; 32], payer), ([7; 32], Sig::Ground(vec![2; 33]))] {
            budget
                .set_deploy_id_funded(deploy_id, other)
                .expect("another funded deployment");
            assert_eq!(
                introduction
                    .for_record(&record)
                    .expect("the seal stays")
                    .authority(),
                &stored
            );
        }
        budget.set_system_payer();
        assert_eq!(
            introduction
                .for_record(&record)
                .expect("system work keeps the seal")
                .authority(),
            &stored
        );
        let bare = ListParWithRandom {
            cost_authority: None,
            ..record
        };
        assert_eq!(
            introduction
                .for_record(&bare)
                .expect("a record without a seal")
                .authority(),
            &CostAuthority::default()
        );
    }

    /// Added by decision 107 (DR-135 R4): a re-stored record resolves its residue seal to the
    /// region of the payer at the region price of the running native execution, as the host
    /// reducer's `produce_peeks` does. A system body of the deployment registered the produce
    /// with that priced region, so the re-store registers the same authority. The unpriced
    /// region of the earlier resolution conflicts with the first registration.
    #[test]
    fn space_record_restore_resolves_residue_at_the_region_price() {
        use crate::rust::interpreter::accounting::authority::{
            canonical_authority, priced_cost_region, sig_to_cost_signature, system_residue_region,
        };
        use crate::rust::interpreter::accounting::Sig;
        use crate::rust::interpreter::reified::system_processes::harness::priced_native_config;

        const PRICE: u64 = 7;
        let payer = Sig::Ground(vec![1; 33]);
        let entropy = b"rrs-residue-entropy".to_vec();
        let signature = sig_to_cost_signature(&payer).expect("a ground payer converts");
        let priced_region =
            priced_cost_region(&signature, &entropy, 0, PRICE).expect("the priced payer region");
        let unpriced_region =
            priced_cost_region(&signature, &entropy, 0, 0).expect("the unpriced payer region");
        let stored = canonical_authority(&CostAuthority {
            regions: vec![
                system_residue_region(&priced_region, &[7; 32]).expect("a residue region")
            ],
        })
        .expect("a canonical residue seal");
        let priced = canonical_authority(&CostAuthority {
            regions: vec![priced_region],
        })
        .expect("a canonical priced payer seal");
        let unpriced = canonical_authority(&CostAuthority {
            regions: vec![unpriced_region],
        })
        .expect("a canonical unpriced payer seal");
        let record = ListParWithRandom {
            pars: vec![int(1)],
            random_state: entropy,
            cost_authority: Some(stored),
            cost_stack: None,
        };
        let budget = RuntimeBudget::new(Cost::create(1_000, "record residue price"));
        budget
            .set_deploy_id_funded([7; 32], payer)
            .expect("a funded deployment");
        budget
            .reset_for_native_execution(priced_native_config(1_000_000, PRICE))
            .expect("the budget enters a priced native execution");
        assert_eq!(budget.region_price(), PRICE);
        let restored = Introduction::new(&budget, &region(9))
            .for_record(&record)
            .expect("the seal resolves");
        assert_eq!(restored.authority(), &priced);

        let _scope = budget.enter_comm_accounting_scope();
        let produce = Produce::create(&int(2), &record, false);
        let produced = callback(&budget, &priced);
        assert_eq!(produced(RSpaceOperationSource::Produce(&produce)), Ok(()));
        let restore = register_introduction(&restored);
        assert_eq!(restore(RSpaceOperationSource::Produce(&produce)), Ok(()));
        let unpriced_restore = callback(&budget, &unpriced);
        assert!(unpriced_restore(RSpaceOperationSource::Produce(&produce)).is_err());
    }

    /// Added by decision 124 (plan v2 C7.1, decision 99, rule CR-15): the seal of a machinery
    /// row is the system residue of the authority of its payer, with the deploy id of the
    /// budget. No authority gives no seal, and the payer introduction carries the empty
    /// authority.
    #[test]
    fn machinery_seal_is_the_residue_of_the_payer_authority() {
        let budget = RuntimeBudget::new(Cost::create(1_000, "record machinery seal"));
        let introduction = Introduction::new(&budget, &region(9));
        let residue = system_residue_authority_with_caller(&region(4), &budget.deploy_id(), None)
            .expect("the residue of a signed region");
        assert_eq!(introduction.seal_of(Some(&region(4))), Ok(Some(residue)));
        assert_eq!(introduction.seal_of(None), Ok(None));
        assert_eq!(introduction.payer().authority(), &CostAuthority::default());
    }

    // ── The slots of a priority queue (P5b, decision 38) ──────────────────────────────────

    /// The levels of the slot tests: the unprioritized level and some priority levels.
    const PRIORITIES: [Option<i64>; 4] = [None, Some(0), Some(3), Some(i64::MAX)];

    #[test]
    fn space_record_pq_datum_priority_slot_roundtrips() {
        for priority in PRIORITIES {
            for payload in [Vec::new(), vec![int(1)], vec![string("a"), int(2)]] {
                let original = datum(payload.clone());
                let record = wrap_pq_datum(7, priority, &original);
                assert_eq!(record.pars[0], pq_datum_envelope(7, priority));
                assert_eq!(&record.pars[1..], payload.as_slice());
                assert_eq!(pq_datum_slots(&record), Ok((7, priority)));
                assert_eq!(
                    unwrap_pq_datum(&record),
                    Ok(PqDatumRecord {
                        ord: 7,
                        priority,
                        datum: original.clone(),
                    })
                );
            }
        }
        let original = datum(vec![int(1)]);
        // The unprioritized record is the P4 record, so the strict decoders read it as well.
        assert_eq!(wrap_pq_datum(4, None, &original), wrap_datum(4, &original));
        assert_eq!(datum_ord(&wrap_pq_datum(4, None, &original)), Ok(4));
        // The strict P4 decoders reject a record with a level: they stay strict for every other
        // discipline.
        let leveled = wrap_pq_datum(4, Some(2), &original);
        let malformed = MalformedRecord(RecordKind::Datum);
        assert_eq!(datum_ord(&leveled), Err(malformed));
        assert_eq!(unwrap_datum(&leveled).err(), Some(malformed));
        // The level is part of the record, so records of two levels differ.
        assert_ne!(
            wrap_pq_datum(4, Some(1), &original),
            wrap_pq_datum(4, Some(2), &original)
        );
        assert_ne!(
            wrap_pq_datum(4, Some(0), &original),
            wrap_pq_datum(4, None, &original)
        );
    }

    #[test]
    fn space_record_pq_datum_decoder_rejects_malformed_slots() {
        let malformed = Some(MalformedRecord(RecordKind::Datum));
        let mut flagged_level = int(2);
        flagged_level.connective_used = true;
        for bad in [
            list_par(vec![
                RecordKind::Datum.tag(),
                int(1),
                int(-1),
                Par::default(),
            ]),
            list_par(vec![
                RecordKind::Datum.tag(),
                int(1),
                string("1"),
                Par::default(),
            ]),
            list_par(vec![
                RecordKind::Datum.tag(),
                int(1),
                flagged_level,
                Par::default(),
            ]),
            list_par(vec![
                RecordKind::Datum.tag(),
                int(1),
                list_par(vec![int(1)]),
                Par::default(),
            ]),
            list_par(vec![RecordKind::Datum.tag(), int(1), int(1), int(0)]),
            list_par(vec![
                RecordKind::Datum.tag(),
                int(-1),
                int(1),
                Par::default(),
            ]),
            list_par(vec![
                tag_with([REIFIED_TAG, RECORD_TAG_MARK, b'D', 2]),
                int(1),
                int(1),
                Par::default(),
            ]),
            list_par(vec![RecordKind::Datum.tag(), int(1), int(1)]),
            continuation_envelope(1, int(1)),
            int(1),
        ] {
            assert_eq!(
                pq_datum_slots(&malformed_datum(bad.clone())).err(),
                malformed,
                "{bad:?}"
            );
            assert_eq!(unwrap_pq_datum(&malformed_datum(bad)).err(), malformed);
        }
        assert_eq!(
            pq_datum_slots(&ListParWithRandom::default()).err(),
            malformed
        );
    }

    /// The canonical map `{"priority": level}` as the normalizer builds it.
    fn canonical_policy(level: i64) -> Par {
        use models::rust::par_map::ParMap;
        use models::rust::par_map_type_mapper::ParMapTypeMapper;
        let map = ParMapTypeMapper::par_map_to_emap(ParMap::create_from_vec(vec![(
            string(PRIORITY_KEY),
            int(level),
        )]));
        Par {
            locally_free: map.locally_free.clone(),
            connective_used: map.connective_used,
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EMapBody(map)),
            }],
            ..Par::default()
        }
    }

    #[test]
    fn space_record_pq_policy_slot_roundtrips() {
        assert_eq!(pq_policy(None), Par::default());
        for level in [0, 1, 9, i64::MAX] {
            assert_eq!(pq_policy(Some(level)), canonical_policy(level));
            assert_eq!(level_of_policy(&pq_policy(Some(level))), Some(Some(level)));
        }
        let original = parked(int(9));
        for priority in PRIORITIES {
            let wrapped =
                wrap_pq_continuation(5, priority, &original).expect("a body has a record");
            assert_eq!(pq_continuation_slots(&wrapped), Ok((5, priority)));
            assert_eq!(
                unwrap_pq_continuation(&wrapped),
                Ok(PqContinuationRecord {
                    ord: 5,
                    priority,
                    continuation: original.clone(),
                })
            );
            // The outcome decodes the fired waiter of every lane.
            assert_eq!(
                into_continuation(wrapped),
                Ok(ContinuationRecord {
                    ord: 5,
                    continuation: original.clone(),
                })
            );
        }
        // The unrestricted continuation is the P4 continuation.
        assert_eq!(
            wrap_pq_continuation(5, None, &original),
            wrap_continuation(5, &original)
        );
        // The strict P4 decoders reject a policy: they stay strict for every other discipline.
        let leveled = wrap_pq_continuation(5, Some(1), &original).expect("a body has a record");
        let malformed = MalformedRecord(RecordKind::Continuation);
        assert_eq!(continuation_ord(&leveled), Err(malformed));
        assert_eq!(unwrap_continuation(&leveled).err(), Some(malformed));
        // The waiters of two lanes differ.
        assert_ne!(
            wrap_pq_continuation(5, Some(1), &original),
            wrap_pq_continuation(5, Some(2), &original)
        );
        assert_ne!(
            wrap_pq_continuation(5, Some(0), &original),
            wrap_pq_continuation(5, None, &original)
        );
        // A ScalaBodyRef or a missing body has no record.
        for continuation in [
            TaggedContinuation {
                tagged_cont: Some(TaggedCont::ScalaBodyRef(201)),
                ..original.clone()
            },
            TaggedContinuation::default(),
        ] {
            assert_eq!(
                wrap_pq_continuation(0, Some(1), &continuation).err(),
                Some(malformed)
            );
        }
    }

    /// A map `Par` with the entries `kvs` and the flags `map_flag` and `par_flag`.
    fn policy_map(kvs: Vec<(Par, Par)>, map_flag: bool, par_flag: bool) -> Par {
        Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EMapBody(EMap {
                    kvs: kvs
                        .into_iter()
                        .map(|(key, value)| KeyValuePair {
                            key: Some(key),
                            value: Some(value),
                        })
                        .collect(),
                    locally_free: Vec::new(),
                    connective_used: map_flag,
                    remainder: None,
                })),
            }],
            connective_used: par_flag,
            ..Par::default()
        }
    }

    #[test]
    fn space_record_pq_policy_decoder_rejects_malformed_policies() {
        let key = || string(PRIORITY_KEY);
        let mut flagged_key = key();
        flagged_key.connective_used = true;
        let mut remainder = pq_policy(Some(1));
        if let Some(Expr {
            expr_instance: Some(ExprInstance::EMapBody(map)),
        }) = remainder.exprs.first_mut()
        {
            map.remainder = Some(Var {
                var_instance: Some(models::rhoapi::var::VarInstance::FreeVar(0)),
            });
        }
        let malformed = Err(MalformedRecord(RecordKind::Continuation));
        for bad in [
            policy_map(Vec::new(), false, false),
            policy_map(vec![(key(), int(1)), (string("w"), int(1))], false, false),
            policy_map(vec![(string("prio"), int(1))], false, false),
            policy_map(vec![(flagged_key, int(1))], false, false),
            policy_map(vec![(key(), int(-1))], false, false),
            policy_map(vec![(key(), string("1"))], false, false),
            policy_map(vec![(key(), int(1))], true, false),
            policy_map(vec![(key(), int(1))], false, true),
            remainder,
            list_par(vec![key(), int(1)]),
            int(1),
        ] {
            assert_eq!(level_of_policy(&bad), None, "{bad:?}");
            let continuation = parked(list_par(vec![
                RecordKind::Continuation.tag(),
                int(1),
                bad,
                int(9),
            ]));
            assert_eq!(pq_continuation_slots(&continuation), malformed);
            assert_eq!(
                into_continuation(continuation).err(),
                Some(MalformedRecord(RecordKind::Continuation))
            );
        }
    }

    // ── The continuation record of a join (P5c, decision 29) ──────────────────────────────

    fn slot(ord: i64, bind: usize, priority: Option<i64>) -> JoinSlot {
        JoinSlot {
            ord,
            bind,
            policy: WaiterPolicy::of_level(priority),
        }
    }

    /// Slot lists of two and three positions: plain orders, repeated orders (two positions on
    /// one collection), bind permutations and lane policies.
    fn join_slot_lists() -> Vec<Vec<JoinSlot>> {
        vec![
            vec![slot(0, 0, None), slot(0, 1, None)],
            vec![slot(4, 1, None), slot(9, 0, None)],
            vec![slot(3, 0, Some(2)), slot(3, 1, None)],
            vec![
                slot(7, 2, None),
                slot(1, 0, Some(0)),
                slot(i64::MAX, 1, Some(i64::MAX)),
            ],
        ]
    }

    #[test]
    fn space_record_join_envelope_roundtrips_the_slots_and_the_body() {
        let mut body = new_boundvar_par(0, Vec::new(), false);
        body.exprs.extend(new_freevar_par(1, Vec::new()).exprs);
        body.locally_free = vec![0b101];
        body.connective_used = true;
        let original = parked(body.clone());
        for slots in join_slot_lists() {
            let wrapped =
                wrap_join_continuation(&slots, &original).expect("a body has a join record");
            let envelope = match &wrapped.tagged_cont {
                Some(TaggedCont::ParBody(ParWithRandom {
                    body: Some(envelope),
                    random_state,
                })) => {
                    assert_eq!(random_state, &vec![5, 6]);
                    envelope.clone()
                }
                other => panic!("a wrapped join waiter keeps its ParBody: {other:?}"),
            };
            assert_eq!(
                Ok(envelope.clone()),
                join_continuation_envelope(&slots, body.clone())
            );
            // The bit sets of the envelope are those of the body: the slots are ground.
            assert!(envelope.connective_used);
            assert_eq!(envelope.locally_free, body.locally_free);
            assert_eq!(wrapped.guard, original.guard);
            assert_eq!(wrapped.cost_authority, original.cost_authority);
            assert_eq!(join_continuation_slots(&wrapped), Ok(slots.clone()));
            let decoded = JoinContinuationRecord {
                slots: slots.clone(),
                continuation: original.clone(),
            };
            assert_eq!(unwrap_join_continuation(&wrapped), Ok(decoded.clone()));
            assert_eq!(into_join_continuation(wrapped), Ok(decoded));
        }
        // The slots are part of the record: other orders, binds or lanes give other records.
        let base = vec![slot(1, 0, None), slot(2, 1, None)];
        for other in [
            vec![slot(1, 0, None), slot(3, 1, None)],
            vec![slot(1, 1, None), slot(2, 0, None)],
            vec![slot(1, 0, Some(0)), slot(2, 1, None)],
        ] {
            assert_ne!(
                wrap_join_continuation(&base, &original),
                wrap_join_continuation(&other, &original)
            );
        }
        // A ScalaBodyRef or a missing body has no record.
        let malformed = Some(MalformedRecord(RecordKind::Continuation));
        for continuation in [
            TaggedContinuation {
                tagged_cont: Some(TaggedCont::ScalaBodyRef(201)),
                ..original.clone()
            },
            TaggedContinuation {
                tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                    body: None,
                    random_state: Vec::new(),
                })),
                ..original.clone()
            },
            TaggedContinuation::default(),
        ] {
            assert_eq!(
                wrap_join_continuation(&base, &continuation).err(),
                malformed
            );
            assert_eq!(join_continuation_slots(&continuation).err(), malformed);
            assert_eq!(into_join_continuation(continuation).err(), malformed);
        }
    }

    #[test]
    fn space_record_join_and_single_bind_records_are_disjoint() {
        assert!(RecordKind::ALL
            .iter()
            .all(|kind| kind.tag_bytes() != JOIN_CONTINUATION_TAG));
        assert_eq!(JOIN_CONTINUATION_TAG, [
            REIFIED_TAG,
            RECORD_TAG_MARK,
            b'K',
            JOIN_CONTINUATION_VERSION
        ]);
        let original = parked(int(9));
        let malformed = MalformedRecord(RecordKind::Continuation);
        // The decoders of version 1 reject every join record.
        for slots in join_slot_lists() {
            let join = wrap_join_continuation(&slots, &original).expect("a join record");
            assert_eq!(continuation_ord(&join), Err(malformed));
            assert_eq!(unwrap_continuation(&join).err(), Some(malformed));
            assert_eq!(pq_continuation_slots(&join), Err(malformed));
            assert_eq!(unwrap_pq_continuation(&join).err(), Some(malformed));
            assert_eq!(into_continuation(join).err(), Some(malformed));
        }
        // The decoders of version 2 reject every record of version 1, with and without a lane.
        for priority in PRIORITIES {
            let single = wrap_pq_continuation(3, priority, &original).expect("a single record");
            assert_eq!(join_continuation_slots(&single), Err(malformed));
            assert_eq!(unwrap_join_continuation(&single).err(), Some(malformed));
            assert_eq!(into_join_continuation(single).err(), Some(malformed));
        }
    }

    /// A join envelope with the slot pars `slot_pars` around the body `9`, built without the
    /// checks of the encoder.
    fn raw_join(tag: Par, slot_pars: Vec<Par>) -> TaggedContinuation {
        parked(list_par(vec![tag, list_par(slot_pars), int(9)]))
    }

    fn raw_slot(ord: Par, bind: Par, policy: Par) -> Par { list_par(vec![ord, bind, policy]) }

    #[test]
    fn space_record_join_decoder_rejects_malformed_envelopes() {
        let malformed = MalformedRecord(RecordKind::Continuation);
        let good = || raw_slot(int(1), int(0), Par::default());
        let second = || raw_slot(int(1), int(1), Par::default());
        // The well-formed control decodes.
        assert_eq!(
            join_continuation_slots(&raw_join(join_tag(), vec![good(), second()])),
            Ok(vec![slot(1, 0, None), slot(1, 1, None)])
        );
        let mut flagged = int(1);
        flagged.connective_used = true;
        let mut remainder = list_par(vec![good(), second()]);
        if let Some(Expr {
            expr_instance: Some(ExprInstance::EListBody(list)),
        }) = remainder.exprs.first_mut()
        {
            list.remainder = Some(Var {
                var_instance: Some(models::rhoapi::var::VarInstance::FreeVar(0)),
            });
        }
        let cases: Vec<TaggedContinuation> = vec![
            // One slot: a waiter with one bind keeps the version 1 record.
            raw_join(join_tag(), vec![good()]),
            raw_join(join_tag(), Vec::new()),
            // The bind indices are not a permutation.
            raw_join(join_tag(), vec![good(), good()]),
            raw_join(join_tag(), vec![
                good(),
                raw_slot(int(1), int(2), Par::default()),
            ]),
            raw_join(join_tag(), vec![
                good(),
                raw_slot(int(1), int(-1), Par::default()),
            ]),
            // A negative, flagged or non-integer order.
            raw_join(join_tag(), vec![
                raw_slot(int(-1), int(0), Par::default()),
                second(),
            ]),
            raw_join(join_tag(), vec![
                raw_slot(flagged.clone(), int(0), Par::default()),
                second(),
            ]),
            raw_join(join_tag(), vec![
                raw_slot(string("1"), int(0), Par::default()),
                second(),
            ]),
            // A flagged bind index, a malformed lane policy, a negative level.
            raw_join(join_tag(), vec![
                raw_slot(int(1), flagged, Par::default()),
                second(),
            ]),
            raw_join(join_tag(), vec![raw_slot(int(1), int(0), int(2)), second()]),
            raw_join(join_tag(), vec![
                raw_slot(int(1), int(0), pq_policy(Some(1))),
                raw_slot(
                    int(1),
                    int(1),
                    policy_map(vec![(string(PRIORITY_KEY), int(-1))], false, false),
                ),
            ]),
            // A slot of two or four elements.
            raw_join(join_tag(), vec![list_par(vec![int(1), int(0)]), second()]),
            raw_join(join_tag(), vec![
                list_par(vec![int(1), int(0), Par::default(), Par::default()]),
                second(),
            ]),
            // A slot list with a remainder, or a slot list that is not a list.
            parked(list_par(vec![join_tag(), remainder, int(9)])),
            parked(list_par(vec![join_tag(), int(1), int(9)])),
            // The tags of version 1 and of version 3, and a short envelope.
            raw_join(RecordKind::Continuation.tag(), vec![good(), second()]),
            raw_join(tag_with([REIFIED_TAG, RECORD_TAG_MARK, b'K', 3]), vec![
                good(),
                second(),
            ]),
            parked(list_par(vec![join_tag(), list_par(vec![good(), second()])])),
        ];
        for bad in cases {
            assert_eq!(join_continuation_slots(&bad), Err(malformed), "{bad:?}");
            assert_eq!(unwrap_join_continuation(&bad).err(), Some(malformed));
            assert_eq!(into_join_continuation(bad).err(), Some(malformed));
        }
        // The encoder accepts exactly the slot lists that the decoder returns.
        let original = parked(int(9));
        for bad in [
            Vec::new(),
            vec![slot(0, 0, None)],
            vec![slot(0, 0, None), slot(0, 0, None)],
            vec![slot(0, 0, None), slot(0, 2, None)],
            vec![slot(-1, 0, None), slot(0, 1, None)],
            vec![slot(0, 0, Some(-1)), slot(0, 1, None)],
        ] {
            assert_eq!(join_continuation_envelope(&bad, int(9)), Err(malformed));
            assert_eq!(
                wrap_join_continuation(&bad, &original).err(),
                Some(malformed)
            );
            assert_eq!(
                wrap_join_continuation_from_parts(
                    &bad,
                    &ParWithRandom {
                        body: Some(int(9)),
                        random_state: vec![5, 6],
                    },
                    &None,
                    None,
                )
                .err(),
                Some(SpaceError::from(malformed))
            );
        }
    }

    #[test]
    fn space_record_join_results_return_to_bind_order() {
        // Every permutation of three binds puts result i at the place of its bind.
        let permutations: [[usize; 3]; 6] =
            [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [
                2, 1, 0,
            ]];
        for binds in permutations {
            let slots: Vec<JoinSlot> = binds.iter().map(|&bind| slot(0, bind, None)).collect();
            let results: Vec<usize> = binds.to_vec();
            assert_eq!(restore_bind_order(&slots, results), Ok(vec![0, 1, 2]));
        }
        let slots = vec![slot(0, 1, None), slot(0, 0, None)];
        assert_eq!(
            restore_bind_order(&slots, vec!["b", "a"]),
            Ok(vec!["a", "b"])
        );
        assert_eq!(
            restore_bind_order(&slots, vec!["only"]),
            Err(SpaceError::Inconsistent(
                "a join fire has not one result for each slot"
            ))
        );
        for bad in [vec![slot(0, 0, None), slot(0, 0, None)], vec![
            slot(0, 0, None),
            slot(0, 2, None),
        ]] {
            assert_eq!(
                restore_bind_order(&bad, vec![1, 2]),
                Err(SpaceError::Inconsistent(
                    "the slots of a join fire are not a permutation of its binds"
                ))
            );
        }
    }

    // ── Waiter policies and the records of a VectorDB space (P6, design v3 4.0) ───────────

    fn double(value: f64) -> Par { expr_par(ExprInstance::GDouble(value.to_bits())) }

    fn query(entries: Vec<(&str, Par)>) -> WaiterPolicy {
        WaiterPolicy::Query(Box::new(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        ))
    }

    // Changed by decision 128 (kind routing): the registry gives the policy keys. The text before:
    // /// Query policies with every key of `VDB_QUERY_KEYS` and with one key alone.
    /// Query policies with every policy key and with one key alone.
    fn query_policies() -> Vec<WaiterPolicy> {
        let id = expr_par(ExprInstance::GByteArray(vec![7; 32]));
        vec![
            query(vec![("q", list_par(vec![int(1), double(0.5)]))]),
            query(vec![("k", int(2))]),
            query(vec![
                ("ids", list_par(vec![id])),
                ("k", int(3)),
                ("metric", string("cosine")),
                ("q", list_par(vec![int(1), int(0)])),
                ("scores", expr_par(ExprInstance::GBool(true))),
                ("threshold", double(0.75)),
            ]),
            query(vec![
                ("metric", string("euclidean")),
                ("q", list_par(vec![double(0.25), double(-1.5)])),
                ("radius", double(0.3)),
                ("results", string("all")),
            ]),
        ]
    }

    #[test]
    fn space_record_waiter_policy_keeps_the_size_of_a_level() {
        // The query of P6 is boxed, so the policy keeps the size of the `Option<i64>` level of P5,
        // and a slot keeps its size.
        assert_eq!(size_of::<WaiterPolicy>(), size_of::<Option<i64>>());
        assert_eq!(
            size_of::<JoinSlot>(),
            size_of::<i64>() + size_of::<usize>() + size_of::<Option<i64>>()
        );
    }

    #[test]
    fn space_record_policy_slot_roundtrips() {
        assert_eq!(policy_par(&WaiterPolicy::None), Par::default());
        assert_eq!(policy_of(&Par::default()), Some(WaiterPolicy::None));
        for level in [0, 1, 9, i64::MAX] {
            let policy = WaiterPolicy::Level(level);
            // A level is the policy of P5b, byte for byte.
            assert_eq!(policy_par(&policy), pq_policy(Some(level)));
            assert_eq!(policy_of(&policy_par(&policy)), Some(policy.clone()));
            assert_eq!(policy.level(), Some(level));
            assert_eq!(WaiterPolicy::of_level(Some(level)), policy);
        }
        assert_eq!(WaiterPolicy::of_level(None), WaiterPolicy::None);
        for policy in query_policies() {
            let par = policy_par(&policy);
            assert_eq!(policy_of(&par), Some(policy.clone()));
            assert_eq!(policy.level(), None);
            assert!(policy.query().is_some());
        }
    }

    #[test]
    fn space_record_policy_decoder_rejects_malformed_policies() {
        use models::rust::par_map::ParMap;
        use models::rust::par_map_type_mapper::ParMapTypeMapper;
        // An empty map is not `Nil`, and a waiter without a query has the policy `Nil`.
        assert_eq!(policy_of(&map_par(Vec::new())), None);
        // Keys outside the query keys: the keys of tensor logic and `precision` left P6
        // (decisions 62 and 66), and a key of the priority queue does not mix with them.
        for key in ["score", "tensors", "precision", "embedding", "priority"] {
            let par = map_par(vec![
                (string(key), int(1)),
                (string("q"), list_par(vec![int(1)])),
            ]);
            assert_eq!(policy_of(&par), None, "the key {key} is not a query key");
        }
        // A map whose entries are not in the canonical order of `ParMap` is not the image of
        // the encoder.
        let canonical = policy_par(&query(vec![("k", int(2)), ("metric", string("dot"))]));
        let reversed = match sole_expr(&canonical) {
            Some(ExprInstance::EMapBody(EMap {
                kvs,
                locally_free,
                connective_used,
                remainder,
            })) => {
                let mut kvs = kvs.clone();
                kvs.reverse();
                expr_par(ExprInstance::EMapBody(EMap {
                    kvs,
                    locally_free: locally_free.clone(),
                    connective_used: *connective_used,
                    remainder: remainder.clone(),
                }))
            }
            _ => panic!("the policy of a query is a map"),
        };
        assert_ne!(reversed, canonical);
        assert_eq!(policy_of(&reversed), None);
        // A map with a remainder is not a policy.
        let with_remainder = {
            let mut map = ParMapTypeMapper::par_map_to_emap(ParMap::create_from_vec(vec![(
                string("k"),
                int(2),
            )]));
            map.remainder = Some(models::rhoapi::Var {
                var_instance: Some(models::rhoapi::var::VarInstance::FreeVar(0)),
            });
            expr_par(ExprInstance::EMapBody(map))
        };
        assert_eq!(policy_of(&with_remainder), None);
        // A level that is not natural is not a policy.
        assert_eq!(policy_of(&canonical_policy(-1)), None);
    }

    #[test]
    fn space_record_every_policy_waiter_decodes_after_its_fire() {
        let original = parked(int(9));
        let mut policies = vec![WaiterPolicy::None, WaiterPolicy::Level(4)];
        policies.extend(query_policies());
        for policy in policies {
            let wrapped = wrap_policy_continuation(5, &policy, &original).expect("a body");
            assert_eq!(continuation_policy_slots(&wrapped), Ok((5, policy.clone())));
            assert_eq!(
                unwrap_policy_continuation(&wrapped).map(|record| record.continuation),
                Ok(original.clone())
            );
            // The outcome of every family decodes a fired waiter with `into_continuation`.
            let decoded = into_continuation(wrapped.clone()).expect("a fired waiter decodes");
            assert_eq!(decoded.ord, 5);
            assert_eq!(decoded.continuation, original);
            // `Nil` and a level give the records of P4 and P5b, byte for byte.
            match &policy {
                WaiterPolicy::None => {
                    assert_eq!(wrapped, wrap_continuation(5, &original).expect("a body"))
                }
                WaiterPolicy::Level(level) => assert_eq!(
                    wrapped,
                    wrap_pq_continuation(5, Some(*level), &original).expect("a body")
                ),
                WaiterPolicy::Query(_) => {
                    // The strict decoders of P4 and P5b reject a query policy.
                    assert!(continuation_ord(&wrapped).is_err());
                    assert!(pq_continuation_slots(&wrapped).is_err());
                }
            }
        }
    }

    #[test]
    fn space_record_join_slot_with_a_query_roundtrips() {
        for policy in query_policies() {
            let slots = vec![
                JoinSlot {
                    ord: 0,
                    bind: 1,
                    policy: policy.clone(),
                },
                slot(3, 0, Some(2)),
                slot(0, 2, None),
            ];
            let wrapped = wrap_join_continuation(&slots, &parked(int(9))).expect("a join record");
            assert_eq!(join_continuation_slots(&wrapped), Ok(slots.clone()));
            let decoded = into_join_continuation(wrapped).expect("a join record decodes");
            assert_eq!(decoded.slots, slots);
        }
    }

    #[test]
    fn space_record_vdb_datum_roundtrips_and_rejects_malformed_slots() {
        let original = datum(vec![string("payload"), int(3)]);
        let vector = list_par(vec![int(1), double(-0.5), int(0)]);
        for embedding in [None, Some(&vector)] {
            let record = wrap_vdb_datum(0, embedding, &original).expect("a record");
            assert_eq!(vdb_datum_slots(&record), Ok((0, embedding)));
            let decoded = unwrap_vdb_datum(&record).expect("a record decodes");
            assert_eq!(decoded.embedding.as_ref(), embedding);
            assert_eq!(decoded.datum, original);
            match embedding {
                // Without an embedding the record is the record of P4.
                None => assert_eq!(record, wrap_datum(0, &original)),
                // The strict decoder of P4 rejects an embedding.
                Some(_) => assert!(datum_ord(&record).is_err()),
            }
        }
        // An embedding is a ground list.
        assert!(wrap_vdb_datum(0, Some(&int(1)), &original).is_err());
        assert!(wrap_vdb_datum(0, Some(&new_freevar_par(0, Vec::new())), &original).is_err());
        let malformed = |envelope: Par| {
            let mut record = wrap_datum(0, &original);
            record.pars[0] = envelope;
            vdb_datum_slots(&record).is_err()
        };
        let tag = RecordKind::Datum.tag();
        assert!(malformed(list_par(vec![
            tag.clone(),
            int(0),
            Par::default(),
            int(4)
        ])));
        assert!(malformed(list_par(vec![
            tag.clone(),
            int(0),
            int(1),
            vector.clone()
        ])));
        assert!(malformed(list_par(vec![
            tag.clone(),
            int(-1),
            Par::default(),
            vector.clone()
        ])));
        assert!(malformed(list_par(vec![
            RecordKind::Continuation.tag(),
            int(0),
            Par::default(),
            vector.clone()
        ])));
    }
}
