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
//! The bit sets of an envelope are the union of the bit sets of its elements. They belong to no
//! identity. The decoders are strict: a record that is not the exact image of its encoder is a
//! [`MalformedRecord`], and the operation aborts with `Inconsistent`.

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    CostAuthority, EList, Expr, ListParWithRandom, Par, ParWithRandom, TaggedContinuation,
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

/// The counter record with the value `value`, the random state `random` and the cost authority
/// `cost_authority`.
pub fn counter_record(
    value: i64,
    random: &Blake2b512Random,
    cost_authority: Option<CostAuthority>,
) -> ListParWithRandom {
    ListParWithRandom {
        pars: vec![list_par(vec![RecordKind::Counter.tag()]), int_par(value)],
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

/// The config record of the config map `config` (I-17) with the random state `random`. It has
/// no cost authority (P4 design B.7).
pub fn config_record(config: Par, random: &Blake2b512Random) -> ListParWithRandom {
    ListParWithRandom {
        pars: vec![list_par(vec![RecordKind::Config.tag()]), config],
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

/// The cost authority that a record carries for `authority`: the authority when it has a
/// region, as `eval_send` stamps a datum and `consume` stamps a continuation.
pub fn cost_authority(authority: &CostAuthority) -> Option<CostAuthority> {
    match authority.regions.is_empty() {
        true => None,
        false => Some(authority.clone()),
    }
}

/// The authorities of the records that one collection operation stages (P4 design B.7).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OperationAuthority {
    /// The cost authority of a counter continuation: the cost authority of the datum or of the
    /// continuation of the operation.
    pub continuation: Option<CostAuthority>,
    /// The introduction authority of the operation. Every staged operation registers it, and a
    /// new counter datum carries it.
    pub introduction: CostAuthority,
}

impl OperationAuthority {
    /// The authorities of a produce: the cost authority of its datum and its introduction
    /// authority.
    pub fn of_produce(op: &ProduceOp) -> Self {
        Self {
            continuation: op.data.cost_authority.clone(),
            introduction: op.authority.clone(),
        }
    }

    /// The authorities of a consume: its receive authority and its introduction authority.
    pub fn of_consume(op: &ConsumeOp) -> Self {
        Self {
            continuation: cost_authority(&op.authority),
            introduction: op.introduction_authority.clone(),
        }
    }
}

/// The `before_observe` callback of a staged operation. At commit, right before the observer
/// sees the operation, it registers `authority` as the introduction authority of the operation
/// identity, where the reducer's `produce` and `consume` register it (P4 design B.7). A repeated
/// identity, a re-offer or a re-fire, registers its first authority again. Another authority
/// fails the commit at that operation.
pub fn register_introduction(budget: &RuntimeBudget, authority: &CostAuthority) -> BeforeObserve {
    let budget = budget.clone();
    let authority = authority.clone();
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
        budget
            .register_introduction_authority(identity, kind, &authority)
            .map_err(RSpaceError::from)
    })
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn space_record_operation_authorities_follow_b7() {
        assert_eq!(cost_authority(&CostAuthority::default()), None);
        assert_eq!(cost_authority(&region(2)), Some(region(2)));
        let produce = ProduceOp {
            space: space(),
            key: CollectionKey::from_bytes(vec![1]).expect("a key"),
            channel: int(1),
            data: datum(vec![int(1)]),
            persistent: false,
            authority: region(5),
        };
        assert_eq!(
            OperationAuthority::of_produce(&produce),
            OperationAuthority {
                continuation: Some(region(4)),
                introduction: region(5),
            }
        );
        let consume = ConsumeOp {
            space: space(),
            key: CollectionKey::from_bytes(vec![1]).expect("a key"),
            channel: int(1),
            pattern: BindPattern::default(),
            body: ParWithRandom::default(),
            persistent: false,
            peek: false,
            guard: None,
            authority: region(6),
            introduction_authority: region(8),
        };
        assert_eq!(
            OperationAuthority::of_consume(&consume),
            OperationAuthority {
                continuation: Some(region(6)),
                introduction: region(8),
            }
        );
        let unsigned = ConsumeOp {
            authority: CostAuthority::default(),
            ..consume
        };
        assert_eq!(OperationAuthority::of_consume(&unsigned).continuation, None);
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
        let unscoped = register_introduction(&budget, &region(1));
        assert_eq!(unscoped(RSpaceOperationSource::Produce(&produce)), Ok(()));

        let _scope = budget.enter_comm_accounting_scope();
        let first = register_introduction(&budget, &region(1));
        assert_eq!(first(RSpaceOperationSource::Produce(&produce)), Ok(()));
        let produce_identity = produce_introduction_identity(&produce);
        assert_eq!(
            budget.introduction_authority(
                produce_identity,
                AuthorityByteEventKind::ProduceIntroduction
            ),
            Ok(region(1))
        );
        let repeated = register_introduction(&budget, &region(1));
        assert_eq!(repeated(RSpaceOperationSource::Produce(&produce)), Ok(()));
        let conflicting = register_introduction(&budget, &region(2));
        assert!(conflicting(RSpaceOperationSource::Produce(&produce)).is_err());
        assert_eq!(
            budget.introduction_authority(
                produce_identity,
                AuthorityByteEventKind::ProduceIntroduction
            ),
            Ok(region(1))
        );

        let consumer = register_introduction(&budget, &region(9));
        assert_eq!(consumer(RSpaceOperationSource::Consume(&consume)), Ok(()));
        assert_eq!(
            budget.introduction_authority(
                consume_introduction_identity(&consume),
                AuthorityByteEventKind::ConsumeIntroduction
            ),
            Ok(region(9))
        );
    }
}
