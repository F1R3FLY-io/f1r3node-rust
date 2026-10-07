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
//! The bit sets of an envelope are the union of the bit sets of its elements. They belong to no
//! identity. The decoders are strict: a record that is not the exact image of its encoder is a
//! [`MalformedRecord`], and the operation aborts with `Inconsistent`.
//!
//! The space agents call the metered forms (decision 42): each one reserves on the host-work
//! meter of the job the reads and the copies that its plain form makes, before the plain form
//! makes them (DR-88 of `feature/cost-accounted-rho`). Then it calls the plain form, so both
//! forms give the same record. Without a budget every reservation is free.

use std::mem::size_of;
use std::sync::{Arc, LazyLock};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    CostAuthority, EList, EMap, Expr, KeyValuePair, ListParWithRandom, Par, ParWithRandom,
    TaggedContinuation,
};
use models::rust::utils::union;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;
use rspace_plus_plus::rspace::space_store_handle::BeforeObserve;

use super::agent::{ConsumeOp, ProduceOp};
use super::errors::SpaceError;
use super::namespace::REIFIED_TAG;
use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use crate::rust::interpreter::accounting::byte_accounting::{
    consume_introduction_identity, produce_introduction_identity,
};
use crate::rust::interpreter::accounting::RuntimeBudget;
use crate::rust::interpreter::reified::costs::ScanMeter;

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

/// [`cost_authority`] with its copy reserved.
pub fn cost_authority_metered(
    authority: &CostAuthority,
    meter: &ScanMeter,
) -> Result<Option<CostAuthority>, RSpaceError> {
    stamped(authority)
        .map(|authority| meter.copy(authority))
        .transpose()
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

// ── Metered forms (decision 42) ───────────────────────────────────────────────────────────

/// The bytes of the spine of a continuation envelope: the `Par`, its `Expr`, the `EList` and the
/// four element `Par`s. A metered decode reserves the spine and reads the three small slots. It
/// does not read the body: the decoder reads only the `connective_used` flag of the body, which
/// the spine covers, and DR-88 charges no part that the decoder does not read.
const CONTINUATION_SPINE_BYTES: usize =
    size_of::<Par>() + size_of::<Expr>() + size_of::<EList>() + 4 * size_of::<Par>();

/// The slots that [`into_continuation`] moves out of a fired continuation: the body, the random
/// state, the guard and the cost authority. Each move is one operation that reads its slot
/// (DR-91).
const CONTINUATION_MOVED_SLOTS: [usize; 4] = [
    size_of::<Par>(),
    size_of::<Vec<u8>>(),
    size_of::<Option<Par>>(),
    size_of::<Option<CostAuthority>>(),
];

/// The machine words of a `RuntimeBudget` handle. Every field of the handle but one is a
/// shared pointer, so a copy of the handle copies at most this many shared pointers.
const RUNTIME_BUDGET_WORDS: usize = size_of::<RuntimeBudget>() / size_of::<usize>();

// The templates of the envelopes that an agent builds. Every envelope of a kind has the shape of
// its template: the order and the value are inline integers, and every bit set of a template
// element is empty. So a reserved copy of the template reserves the allocations of a new
// envelope, before the builder makes them.

/// The template of a datum envelope.
static DATUM_ENVELOPE: LazyLock<Par> = LazyLock::new(|| datum_envelope(0));

/// The template of a continuation envelope around an empty body: the spine, the tag, the order
/// and the policy slot.
static CONTINUATION_ENVELOPE: LazyLock<Par> =
    LazyLock::new(|| continuation_envelope(0, Par::default()));

/// The template of the pars of a counter record.
static COUNTER_PARS: LazyLock<Vec<Par>> = LazyLock::new(|| counter_pars(0));

/// The template of the envelope of a config record.
static CONFIG_ENVELOPE: LazyLock<Par> = LazyLock::new(config_envelope);

/// [`wrap_datum`] with its copies reserved: the envelope, the buffer of the envelope and the
/// payload, each payload par, the random state, the cost authority and the cost stack of
/// `datum`.
pub fn wrap_datum_metered(
    ord: i64,
    datum: &ListParWithRandom,
    meter: &ScanMeter,
) -> Result<ListParWithRandom, RSpaceError> {
    let length = datum
        .pars
        .len()
        .checked_add(1)
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve_copy(&*DATUM_ENVELOPE)?;
    meter.reserve_buffer::<Par>(length)?;
    for par in &datum.pars {
        meter.reserve_copy(par)?;
    }
    meter.reserve_copy(&datum.random_state)?;
    meter.reserve_copy(&datum.cost_authority)?;
    meter.reserve_copy(&datum.cost_stack)?;
    Ok(wrap_datum(ord, datum))
}

/// [`datum_ord`] with its read reserved: the envelope, which is small for every record that an
/// agent wrote (DR-76: inspect before an unmetered walk).
pub fn datum_ord_metered(record: &ListParWithRandom, meter: &ScanMeter) -> Result<i64, SpaceError> {
    if let Some(envelope) = record.pars.first() {
        meter.inspect(envelope)?;
    }
    Ok(datum_ord(record)?)
}

/// [`unwrap_datum`] with its reads and copies reserved: the envelope, the payload, the random
/// state, the cost authority and the cost stack.
pub fn unwrap_datum_metered(
    record: &ListParWithRandom,
    meter: &ScanMeter,
) -> Result<DatumRecord, SpaceError> {
    if let Some(envelope) = record.pars.first() {
        meter.inspect(envelope)?;
    }
    if let Some(payload) = record.pars.get(1..) {
        meter.reserve_copy_slice(payload)?;
    }
    meter.reserve_copy(&record.random_state)?;
    meter.reserve_copy(&record.cost_authority)?;
    meter.reserve_copy(&record.cost_stack)?;
    Ok(unwrap_datum(record)?)
}

/// The parked continuation of a receive with the order `ord`, built from its parts with one
/// copy of each part: the body inside the envelope, the random state, the guard and the cost
/// authority. It equals [`wrap_continuation`] of the continuation with these parts.
///
/// The reservations are the envelope, the body, the work of `list_par` on the bit set of the
/// body (its copy into the fold, the union and the copy into the list), the random state, the
/// guard and the cost authority.
pub fn wrap_continuation_metered(
    ord: i64,
    body: &ParWithRandom,
    guard: &Option<Par>,
    cost_authority: Option<&CostAuthority>,
    meter: &ScanMeter,
) -> Result<TaggedContinuation, SpaceError> {
    let Some(original) = &body.body else {
        return Err(MalformedRecord(RecordKind::Continuation).into());
    };
    meter.reserve_copy(&*CONTINUATION_ENVELOPE)?;
    meter.reserve_copy(original)?;
    meter.reserve_copy(&original.locally_free)?;
    meter.reserve_buffer::<u8>(original.locally_free.len())?;
    meter.reserve_copy(&original.locally_free)?;
    meter.reserve_copy(&body.random_state)?;
    meter.reserve_copy(guard)?;
    let cost_authority = cost_authority
        .map(|authority| meter.copy(authority))
        .transpose()?;
    Ok(TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(continuation_envelope(ord, original.clone())),
            random_state: body.random_state.clone(),
        })),
        guard: guard.clone(),
        cost_authority,
    })
}

/// Reserves the decode of the envelope of `continuation`: its spine and its tag, order and
/// policy slots, never its body.
fn reserve_continuation_envelope(
    continuation: &TaggedContinuation,
    meter: &ScanMeter,
) -> Result<(), SpaceError> {
    meter.charge(4, CONTINUATION_SPINE_BYTES, 0)?;
    let slots = match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(body), ..
        })) => match sole_expr(body) {
            Some(ExprInstance::EListBody(EList { ps, .. })) => ps.get(..3),
            _ => None,
        },
        Some(TaggedCont::ParBody(ParWithRandom { body: None, .. }))
        | Some(TaggedCont::ScalaBodyRef(_))
        | None => None,
    };
    for slot in slots.unwrap_or_default() {
        meter.inspect(slot)?;
    }
    Ok(())
}

/// [`continuation_ord`] with its read reserved: the spine and the small slots of the envelope.
pub fn continuation_ord_metered(
    continuation: &TaggedContinuation,
    meter: &ScanMeter,
) -> Result<i64, SpaceError> {
    reserve_continuation_envelope(continuation, meter)?;
    Ok(continuation_ord(continuation)?)
}

/// Reserves the later [`into_continuation`] of a fired continuation at the time its fire is
/// staged: the decode of its envelope and the moves of its four slots. The outcome of the
/// operation decodes the receipt after the commit, so the agent pays for that work before the
/// commit, and a host-work rejection never follows a commit.
pub fn prepay_continuation_unwrap(
    continuation: &TaggedContinuation,
    meter: &ScanMeter,
) -> Result<(), SpaceError> {
    reserve_continuation_envelope(continuation, meter)?;
    for slot in CONTINUATION_MOVED_SLOTS {
        meter.moved(slot)?;
    }
    Ok(())
}

/// Decodes an owned parked continuation (the inverse of [`wrap_continuation`] and of
/// [`wrap_pq_continuation`]). It moves the original body, the random state, the guard and the
/// cost authority out of the record, so it copies nothing. [`prepay_continuation_unwrap`]
/// reserves its work. The outcome of every family decodes its fired continuations here, and
/// the fired waiter of a priority queue carries its policy (P5b), so this decoder accepts the
/// policy slot `Nil` and the policy of a priority queue.
pub fn into_continuation(
    continuation: TaggedContinuation,
) -> Result<ContinuationRecord, MalformedRecord> {
    let malformed = MalformedRecord(RecordKind::Continuation);
    // Replaced by P5b (decision 38): the strict decoder rejects the policy of a waiter of a
    // priority queue, so a fire of such a waiter would fail after its commit. The decoder of the
    // priority queue accepts `Nil` and that policy, and it is the strict decoder on `Nil`.
    // let ord = continuation_ord(&continuation)?;
    let (ord, _) = pq_continuation_slots(&continuation)?;
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

/// [`counter_record`] with its copies reserved: the pars and the bytes of the random state. The
/// caller reserves the copy of `cost_authority`.
pub fn counter_record_metered(
    value: i64,
    random: &Blake2b512Random,
    cost_authority: Option<CostAuthority>,
    meter: &ScanMeter,
) -> Result<ListParWithRandom, RSpaceError> {
    meter.reserve_copy(&*COUNTER_PARS)?;
    meter.random_bytes(random)?;
    Ok(counter_record(value, random, cost_authority))
}

/// [`counter_value`] with its read reserved: the envelope and the value.
pub fn counter_value_metered(
    record: &ListParWithRandom,
    meter: &ScanMeter,
) -> Result<i64, SpaceError> {
    meter.inspect(&record.pars)?;
    Ok(counter_value(record)?)
}

/// [`config_record`] with its copies reserved: the envelope, the buffer of the two pars and the
/// bytes of the random state. The config map moves into the record.
pub fn config_record_metered(
    config: Par,
    random: &Blake2b512Random,
    meter: &ScanMeter,
) -> Result<ListParWithRandom, RSpaceError> {
    meter.reserve_copy(&*CONFIG_ENVELOPE)?;
    meter.reserve_buffer::<Par>(2)?;
    meter.random_bytes(random)?;
    Ok(config_record(config, random))
}

/// [`config_map`] with its read reserved: the envelope. The caller reserves its reads of the
/// map.
pub fn config_map_metered<'a>(
    record: &'a ListParWithRandom,
    meter: &ScanMeter,
) -> Result<&'a Par, SpaceError> {
    if let Some(envelope) = record.pars.first() {
        meter.inspect(envelope)?;
    }
    Ok(config_map(record)?)
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

/// The template of a datum envelope of a priority queue with a level: the order and the level
/// are inline integers, so a reserved copy of it reserves the allocations of such an envelope.
static PQ_DATUM_ENVELOPE: LazyLock<Par> = LazyLock::new(|| pq_datum_envelope(0, Some(0)));

/// The template of the policy of a waiter of a level: one entry with an inline integer.
static PQ_POLICY: LazyLock<Par> = LazyLock::new(|| pq_policy(Some(0)));

/// [`wrap_pq_datum`] with its copies reserved: the envelope (the template of its shape), the
/// buffer of the envelope and the payload, each payload par, the random state, the cost
/// authority and the cost stack of `datum`.
pub fn wrap_pq_datum_metered(
    ord: i64,
    priority: Option<i64>,
    datum: &ListParWithRandom,
    meter: &ScanMeter,
) -> Result<ListParWithRandom, RSpaceError> {
    let length = datum
        .pars
        .len()
        .checked_add(1)
        .ok_or(RSpaceError::HostWorkRejected)?;
    let template = match priority {
        Some(_) => &*PQ_DATUM_ENVELOPE,
        None => &*DATUM_ENVELOPE,
    };
    meter.reserve_copy(template)?;
    meter.reserve_buffer::<Par>(length)?;
    for par in &datum.pars {
        meter.reserve_copy(par)?;
    }
    meter.reserve_copy(&datum.random_state)?;
    meter.reserve_copy(&datum.cost_authority)?;
    meter.reserve_copy(&datum.cost_stack)?;
    Ok(wrap_pq_datum(ord, priority, datum))
}

/// [`pq_datum_slots`] with its read reserved: the envelope, which is small for every record that
/// an agent wrote.
pub fn pq_datum_slots_metered(
    record: &ListParWithRandom,
    meter: &ScanMeter,
) -> Result<(i64, Option<i64>), SpaceError> {
    if let Some(envelope) = record.pars.first() {
        meter.inspect(envelope)?;
    }
    Ok(pq_datum_slots(record)?)
}

/// [`unwrap_pq_datum`] with its reads and copies reserved: the envelope, the payload, the random
/// state, the cost authority and the cost stack.
pub fn unwrap_pq_datum_metered(
    record: &ListParWithRandom,
    meter: &ScanMeter,
) -> Result<PqDatumRecord, SpaceError> {
    if let Some(envelope) = record.pars.first() {
        meter.inspect(envelope)?;
    }
    if let Some(payload) = record.pars.get(1..) {
        meter.reserve_copy_slice(payload)?;
    }
    meter.reserve_copy(&record.random_state)?;
    meter.reserve_copy(&record.cost_authority)?;
    meter.reserve_copy(&record.cost_stack)?;
    Ok(unwrap_pq_datum(record)?)
}

/// The parked continuation of a receive of a priority queue with the order `ord` and the policy
/// of `priority`, built from its parts with one copy of each part. It equals
/// [`wrap_pq_continuation`] of the continuation with these parts.
///
/// The reservations are those of [`wrap_continuation_metered`], and for a waiter of a level the
/// policy (the template of its shape).
pub fn wrap_pq_continuation_metered(
    ord: i64,
    priority: Option<i64>,
    body: &ParWithRandom,
    guard: &Option<Par>,
    cost_authority: Option<&CostAuthority>,
    meter: &ScanMeter,
) -> Result<TaggedContinuation, SpaceError> {
    let Some(original) = &body.body else {
        return Err(MalformedRecord(RecordKind::Continuation).into());
    };
    meter.reserve_copy(&*CONTINUATION_ENVELOPE)?;
    if priority.is_some() {
        meter.reserve_copy(&*PQ_POLICY)?;
    }
    meter.reserve_copy(original)?;
    meter.reserve_copy(&original.locally_free)?;
    meter.reserve_buffer::<u8>(original.locally_free.len())?;
    meter.reserve_copy(&original.locally_free)?;
    meter.reserve_copy(&body.random_state)?;
    meter.reserve_copy(guard)?;
    let cost_authority = cost_authority
        .map(|authority| meter.copy(authority))
        .transpose()?;
    Ok(TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(pq_continuation_envelope(ord, priority, original.clone())),
            random_state: body.random_state.clone(),
        })),
        guard: guard.clone(),
        cost_authority,
    })
}

/// [`pq_continuation_slots`] with its read reserved: the spine and the small slots of the
/// envelope, the policy included, never the body.
pub fn pq_continuation_slots_metered(
    continuation: &TaggedContinuation,
    meter: &ScanMeter,
) -> Result<(i64, Option<i64>), SpaceError> {
    reserve_continuation_envelope(continuation, meter)?;
    Ok(pq_continuation_slots(continuation)?)
}

/// The introduction authority of one operation and the phlo budget of its deploy. The staged
/// operations of the operation share it, so each callback copies one shared pointer instead of
/// the budget and the authority.
pub struct Introduction {
    budget: RuntimeBudget,
    authority: CostAuthority,
}

impl Introduction {
    /// The introduction of `authority` on `budget`, with its copies reserved: the shared
    /// pointers of the budget handle, the authority and the shared allocation.
    pub fn new(
        budget: &RuntimeBudget,
        authority: &CostAuthority,
        meter: &ScanMeter,
    ) -> Result<Arc<Introduction>, RSpaceError> {
        meter.shared(RUNTIME_BUDGET_WORDS)?;
        meter.reserve_copy(authority)?;
        let allocation = shared::rust::clone_backing::arc_allocation_bytes::<Introduction>()
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.charge(1, 0, allocation)?;
        Ok(Arc::new(Introduction {
            budget: budget.clone(),
            authority: authority.clone(),
        }))
    }

    /// The introduction authority.
    pub fn authority(&self) -> &CostAuthority { &self.authority }

    /// The introduction of a re-stored record (decision 45, Rule E): the budget of this
    /// introduction and the record's own cost authority, or the empty authority when the record
    /// has none. The host reducer's `produce_peeks` registers the same authority when it sends a
    /// peeked datum again. The copies are reserved as in [`Introduction::new`].
    pub fn for_record(
        &self,
        record: &ListParWithRandom,
        meter: &ScanMeter,
    ) -> Result<Arc<Introduction>, RSpaceError> {
        match &record.cost_authority {
            Some(authority) => Introduction::new(&self.budget, authority, meter),
            None => Introduction::new(&self.budget, &CostAuthority::default(), meter),
        }
    }
}

/// The `before_observe` callback of a staged operation of the operation of `introduction`.
/// At commit, right before the observer sees the staged operation, it registers the
/// introduction authority under the identity of the staged operation, where the reducer's
/// `produce` and `consume` register it (P4 design B.7). A repeated identity, a re-offer or a
/// re-fire, registers its first authority again. Another authority fails the commit at that
/// operation. The callback copies one shared pointer, which the meter reserves.
pub fn register_introduction(
    introduction: &Arc<Introduction>,
    meter: &ScanMeter,
) -> Result<BeforeObserve, RSpaceError> {
    meter.shared(1)?;
    let introduction = Arc::clone(introduction);
    Ok(Box::new(move |source: RSpaceOperationSource<'_>| {
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
    }))
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

    /// The introduction of `authority` on a fresh budget, without a meter.
    fn introduction_of(authority: &CostAuthority) -> Arc<Introduction> {
        let budget = RuntimeBudget::new(Cost::create(1_000, "record test"));
        Introduction::new(&budget, authority, &ScanMeter::new(None))
            .expect("an unmetered introduction")
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
        let meter = ScanMeter::new(None);
        let introduction =
            Introduction::new(budget, authority, &meter).expect("an unmetered introduction");
        register_introduction(&introduction, &meter).expect("an unmetered callback")
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

    #[test]
    fn space_record_pq_metered_forms_equal_the_plain_forms_and_reserve_first() {
        use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};

        use crate::rust::interpreter::host_work::HostWorkBudget;

        let unbounded =
            || HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)));
        let operations = HostWorkDimension::VerificationOperations;
        let original = datum(vec![string("payload"), int(3)]);
        let body = ParWithRandom {
            body: Some(int(9)),
            random_state: vec![5, 6],
        };
        for priority in PRIORITIES {
            let budget = unbounded();
            let meter = ScanMeter::new(Some(budget.clone()));
            let record = wrap_pq_datum_metered(2, priority, &original, &meter).expect("a record");
            assert_eq!(record, wrap_pq_datum(2, priority, &original));
            assert_eq!(pq_datum_slots_metered(&record, &meter), Ok((2, priority)));
            assert_eq!(
                unwrap_pq_datum_metered(&record, &meter).map(|decoded| decoded.datum),
                Ok(original.clone())
            );
            let continuation = wrap_pq_continuation_metered(
                3,
                priority,
                &body,
                &Some(int(1)),
                Some(&region(7)),
                &meter,
            )
            .expect("a continuation");
            assert_eq!(
                continuation,
                wrap_pq_continuation(3, priority, &parked(int(9))).expect("a body")
            );
            assert_eq!(
                pq_continuation_slots_metered(&continuation, &meter),
                Ok((3, priority))
            );
            let usage = budget.usage(operations).get();
            assert!(usage > 0, "the forms reserve their work");
            // A budget one unit short rejects the forms before they return a value.
            let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
            limits.set(operations, HostWorkLimit::new(usage - 1));
            let cut = ScanMeter::new(Some(HostWorkBudget::new(limits)));
            let outcome = wrap_pq_datum_metered(2, priority, &original, &cut)
                .map_err(SpaceError::from)
                .and_then(|record| {
                    pq_datum_slots_metered(&record, &cut)?;
                    unwrap_pq_datum_metered(&record, &cut)?;
                    let continuation = wrap_pq_continuation_metered(
                        3,
                        priority,
                        &body,
                        &Some(int(1)),
                        Some(&region(7)),
                        &cut,
                    )?;
                    pq_continuation_slots_metered(&continuation, &cut)
                });
            assert_eq!(
                outcome,
                Err(SpaceError::Host(RSpaceError::HostWorkRejected))
            );
        }
        // A waiter of a level reserves its policy, so it reserves more than an unrestricted one.
        let usage_of = |priority: Option<i64>| {
            let budget = unbounded();
            let meter = ScanMeter::new(Some(budget.clone()));
            wrap_pq_continuation_metered(3, priority, &body, &None, None, &meter)
                .expect("a continuation");
            budget.usage(operations).get()
        };
        assert!(usage_of(Some(1)) > usage_of(None));
    }
}
