//! The markers and the delivery vehicle of a PathMap space (P5e: plan "PathMap delivery",
//! decision 40 Q3, F4, P5 design v3 A.11).
//!
//! A datum `d` on the path `p` lives on the data channel `D(sid, path_key(p))`. Each prefix `q`
//! of `p`, from `ε` to `p` itself, holds one marker of `d` on the index channel
//! `I(sid, prefix_key(q))`. So a receive on the prefix `q` finds every datum under `q` from the
//! markers of one channel, and a COMM on `I(q)` stays single-channel (F4).
//!
//! | Record   | `pars`                                                     | Stored |
//! |----------|------------------------------------------------------------|--------|
//! | marker   | `[EList[tag_I, GByteArray(src(d))], EList(p∖q)]`             | yes, on `I(q)` |
//! | vehicle  | `[EList[tag_I, GByteArray(src(d))], EList(p∖q), payload(d)…]` | never  |
//!
//! * `tag_I = GByteArray([0xE0, 0x52, b'I', 1])`. It has the layout of the record tags of
//!   `record.rs`: the namespace tag, the record mark, the kind `I` of the path index, and the
//!   version 1. This module defines it, so `RecordKind` does not change (P5 design v3, A.5).
//! * `src(d)` is the hash of the source produce of `d` on `D(p)`: the identity of `d`. Two
//!   datums with equal payloads differ in their random states, so their markers differ too. A
//!   marker hash therefore never equals the hash of an older produce, which merge would file as
//!   "copied by peek" (plan, "Marker uniqueness").
//! * `p∖q` is the suffix of `p` after `q`. A receive on `q` re-roots the payload of `d` under it.
//! * The random state of the marker on the prefix of depth `k` is
//!   `PathMarker.derive(d.rs).split_short(k)`. So the markers of one datum have distinct random
//!   states, and a marker is a function of `d` and `k` alone.
//! * A marker is persistent exactly when `d` is persistent. It has no cost stack, and it carries
//!   the cost authority of the record of `d` on `D(p)` ([`marker_authority`]).
//!
//! The vehicle of `d` on `I(q)` is the incoming record of a produce-side delivery (decision 40,
//! Q3). It is the marker of `d` on `q` followed by the payload of `d`, with the random state, the
//! cost authority and the cost stack of `d`. The substrate never stores the incoming record of a
//! fire, so the vehicle is never stored. Its introduction covers the payload, as the
//! introduction of the equivalent host produce does (DR-47), and the stored markers stay short.
//!
//! The decoders are strict: a record that is not the exact image of its encoder is a
//! [`MalformedMarker`], and the operation aborts with `Inconsistent`. The formal model is
//! `PathMapPrefix.v` (`marker_record_roundtrip`, `vehicle_carries_payload`).

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{CostAuthority, EList, Expr, ListParWithRandom, Par};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use super::path_codec::plain_list;
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::namespace::{RandomPurpose, REIFIED_TAG};
use crate::rust::interpreter::spaces::record::{expr_par, sole_expr, RECORD_TAG_MARK};

/// The kind byte of a marker tag: the KIND `I` of the path index channels.
pub const MARKER_KIND: u8 = b'I';

/// The version of the marker envelope.
pub const MARKER_VERSION: u8 = 1;

/// The bytes of the marker tag: `[0xE0, 0x52, b'I', 1]`.
pub const MARKER_TAG: [u8; 4] = [REIFIED_TAG, RECORD_TAG_MARK, MARKER_KIND, MARKER_VERSION];

/// The length of the source hash of a datum in a marker envelope.
pub const SOURCE_LEN: usize = 32;

/// The pars of a marker that come before the payload of a vehicle: the envelope and the suffix.
pub const MARKER_PARS: usize = 2;

/// A record on an index channel that is not the exact image of the marker encoder, or a vehicle
/// that is not the exact image of the vehicle encoder. It aborts the operation with
/// `Inconsistent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("a malformed {0:?} record of a PathMap space")]
pub struct MalformedMarker(pub MarkerShape);

/// The shape that a strict decoder expected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MarkerShape {
    /// A stored marker: the envelope and the suffix.
    Marker,
    /// A delivery vehicle: the envelope, the suffix and the payload.
    Vehicle,
}

impl From<MalformedMarker> for SpaceError {
    fn from(error: MalformedMarker) -> Self {
        SpaceError::Inconsistent(match error.0 {
            MarkerShape::Marker => "a malformed marker record",
            MarkerShape::Vehicle => "a malformed delivery vehicle",
        })
    }
}

/// The marker tag `GByteArray(MARKER_TAG)`.
pub fn marker_tag() -> Par { expr_par(ExprInstance::GByteArray(MARKER_TAG.to_vec())) }

/// The envelope of the markers of the datum whose source hash is `source`:
/// `EList[tag_I, GByteArray(source)]`. Both elements are ground, so the list has no bit set.
pub fn marker_envelope(source: &Blake2b256Hash) -> Par { envelope_of(source.0.clone()) }

/// The envelope with the source bytes `source`.
fn envelope_of(source: Vec<u8>) -> Par {
    let elements = vec![marker_tag(), expr_par(ExprInstance::GByteArray(source))];
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: elements,
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }],
        ..Par::default()
    }
}

/// The cost authority of every marker of the datum whose record on `D(p)` is `datum_record`: the
/// cost authority of that record (open point 6 of the seams arbitration). The store of the
/// markers and the restore of a marker that a peek removed take the authority from this one
/// function. So a restored marker has the bytes and the produce identity of the removed marker,
/// also when the record carries no authority or an empty one.
pub fn marker_authority(datum_record: &ListParWithRandom) -> Option<&CostAuthority> {
    datum_record.cost_authority.as_ref()
}

/// The random state of the marker of a datum on the prefix of depth `depth`:
/// `PathMarker.derive(datum_random).split_short(depth)`. A prefix is at most
/// `PATH_DEPTH_CAP` deep, so a depth that does not fit an `i16` is `Inconsistent`.
pub fn marker_random(
    datum_random: &Blake2b512Random,
    depth: usize,
) -> Result<Blake2b512Random, SpaceError> {
    let index = i16::try_from(depth).map_err(|_| {
        SpaceError::Inconsistent("a marker depth exceeds the range of a random split")
    })?;
    Ok(RandomPurpose::PathMarker
        .derive(datum_random)
        .split_short(index))
}

/// The marker of the datum whose source hash is `source`, on the prefix whose suffix is
/// `suffix`: the pars `[envelope, EList(suffix)]`, the random state `random`, the cost
/// authority `cost_authority` and no cost stack. The suffix moves into the record.
pub fn marker_record(
    source: &Blake2b256Hash,
    suffix: Vec<Par>,
    random: &Blake2b512Random,
    cost_authority: Option<CostAuthority>,
) -> ListParWithRandom {
    ListParWithRandom {
        pars: vec![marker_envelope(source), plain_list(suffix)],
        random_state: random.to_bytes(),
        cost_authority,
        cost_stack: None,
    }
}

/// The vehicle of the datum `datum` on the prefix whose suffix is `suffix`: the marker pars
/// `[envelope, EList(suffix)]`, then the payload of `datum`, with the random state, the cost
/// authority and the cost stack of `datum` (decision 40, Q3). The suffix moves into the record.
pub fn vehicle(
    source: &Blake2b256Hash,
    suffix: Vec<Par>,
    datum: &ListParWithRandom,
) -> ListParWithRandom {
    let mut pars = Vec::with_capacity(MARKER_PARS + datum.pars.len());
    pars.push(marker_envelope(source));
    pars.push(plain_list(suffix));
    pars.extend(datum.pars.iter().cloned());
    ListParWithRandom {
        pars,
        random_state: datum.random_state.clone(),
        cost_authority: datum.cost_authority.clone(),
        cost_stack: datum.cost_stack.clone(),
    }
}

/// A decoded marker, borrowed from its record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkerView<'r> {
    /// The source hash of the datum, [`SOURCE_LEN`] bytes.
    pub source: &'r [u8],
    /// The suffix `p∖q` of the path of the datum after the prefix of the marker.
    pub suffix: &'r [Par],
}

impl MarkerView<'_> {
    /// The source hash of the datum as a hash value.
    pub fn source_hash(&self) -> Blake2b256Hash { Blake2b256Hash::from_bytes(self.source.to_vec()) }

    /// True when the marker belongs to the datum whose source hash is `source`.
    pub fn is_of(&self, source: &Blake2b256Hash) -> bool { self.source == source.0.as_slice() }
}

/// A decoded vehicle, borrowed from its record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VehicleView<'r> {
    /// The marker part of the vehicle.
    pub marker: MarkerView<'r>,
    /// The payload of the datum.
    pub payload: &'r [Par],
}

/// Decodes a stored marker (the inverse of [`marker_record`]). The record holds exactly the
/// envelope and the suffix list, and no cost stack. The random state and the cost authority are
/// not part of the decode.
pub fn decode_marker(record: &ListParWithRandom) -> Result<MarkerView<'_>, MalformedMarker> {
    let malformed = MalformedMarker(MarkerShape::Marker);
    match (record.pars.as_slice(), &record.cost_stack) {
        ([envelope, suffix], None) => marker_parts(envelope, suffix).ok_or(malformed),
        _ => Err(malformed),
    }
}

/// Decodes a delivery vehicle (the inverse of [`vehicle`]): the marker part and the payload.
pub fn decode_vehicle(record: &ListParWithRandom) -> Result<VehicleView<'_>, MalformedMarker> {
    let malformed = MalformedMarker(MarkerShape::Vehicle);
    match record.pars.as_slice() {
        [envelope, suffix, payload @ ..] => match marker_parts(envelope, suffix) {
            Some(marker) => Ok(VehicleView { marker, payload }),
            None => Err(malformed),
        },
        _ => Err(malformed),
    }
}

/// The marker part of a record: an envelope with the marker tag and a source of
/// [`SOURCE_LEN`] bytes, and a strict suffix list.
fn marker_parts<'r>(envelope: &'r Par, suffix: &'r Par) -> Option<MarkerView<'r>> {
    let source = match strict_list(envelope)? {
        [tag, source] if is_marker_tag(tag) => ground_bytes(source)?,
        _ => return None,
    };
    match source.len() == SOURCE_LEN {
        true => Some(MarkerView {
            source,
            suffix: strict_list(suffix)?,
        }),
        false => None,
    }
}

/// True when `par` is exactly the marker tag, with no connective flag.
fn is_marker_tag(par: &Par) -> bool { ground_bytes(par) == Some(MARKER_TAG.as_slice()) }

/// The bytes of a `Par` that is exactly one `GByteArray` with no connective flag.
fn ground_bytes(par: &Par) -> Option<&[u8]> {
    match (sole_expr(par), par.connective_used) {
        (Some(ExprInstance::GByteArray(bytes)), false) => Some(bytes.as_slice()),
        _ => None,
    }
}

/// The elements of a `Par` that is exactly one list with the flags of the normalizer: no
/// remainder, and the `connective_used` flags of the list and of the `Par` are the disjunction
/// of the flags of the elements. It restates the private `list_elements` of `record.rs`, so a
/// marker list has the strictness of every other space record.
fn strict_list(par: &Par) -> Option<&[Par]> {
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

#[cfg(test)]
mod tests {
    use models::rhoapi::{CostRegion, CostSignature, CostStack};
    use proptest::prelude::*;

    use super::super::test_support::*;
    use super::*;
    use crate::rust::interpreter::spaces::record::{
        counter_record, wrap_datum, RecordKind, RECORD_VERSION,
    };

    fn source(byte: u8) -> Blake2b256Hash { Blake2b256Hash::from_bytes(vec![byte; SOURCE_LEN]) }

    fn region(byte: u8) -> CostAuthority {
        CostAuthority {
            regions: vec![CostRegion {
                instance_id: vec![byte; 32],
                signature: Some(CostSignature {
                    value: Some(models::rhoapi::cost_signature::Value::Ground(vec![byte])),
                }),
            }],
        }
    }

    fn random(seed: &[u8]) -> Blake2b512Random { Blake2b512Random::create_from_bytes(seed) }

    fn datum(pars: Vec<Par>) -> ListParWithRandom {
        ListParWithRandom {
            pars,
            random_state: random(b"datum").to_bytes(),
            cost_authority: Some(region(3)),
            cost_stack: None,
        }
    }

    fn marker(suffix: Vec<Par>) -> ListParWithRandom {
        marker_record(
            &source(7),
            suffix,
            &marker_random(&random(b"datum"), 1).expect("a depth of 1 fits"),
            Some(region(3)),
        )
    }

    #[test]
    fn the_marker_tag_is_a_record_tag_of_the_kind_i() {
        assert_eq!(MARKER_TAG, [0xE0, 0x52, b'I', 1]);
        assert_eq!(MARKER_TAG[0], REIFIED_TAG);
        assert_eq!(MARKER_TAG[1], RECORD_TAG_MARK);
        assert_eq!(MARKER_VERSION, RECORD_VERSION);
        for kind in RecordKind::ALL {
            assert_ne!(kind.tag_bytes(), MARKER_TAG, "{kind:?}");
        }
        assert_eq!(
            MARKER_KIND,
            crate::rust::interpreter::spaces::namespace::Kind::PathIndex as u8
        );
    }

    #[test]
    fn a_marker_decodes_to_its_source_and_suffix() {
        let suffix = vec![gint(2), gstring("hi")];
        let record = marker(suffix.clone());
        assert_eq!(record.pars.len(), MARKER_PARS);
        assert_eq!(record.cost_stack, None);
        assert_eq!(record.cost_authority, Some(region(3)));
        let view = decode_marker(&record).expect("a marker decodes");
        assert_eq!(view.source, source(7).0.as_slice());
        assert_eq!(view.suffix, suffix.as_slice());
        assert!(view.is_of(&source(7)));
        assert!(!view.is_of(&source(8)));
        assert_eq!(view.source_hash(), source(7));
        let exact = marker(Vec::new());
        assert_eq!(
            decode_marker(&exact)
                .expect("an exact-path marker decodes")
                .suffix,
            &[] as &[Par]
        );
    }

    #[test]
    fn the_marker_decoder_is_strict() {
        let good = marker(vec![gint(1)]);
        let malformed = Err(MalformedMarker(MarkerShape::Marker));
        let mut wrong_version = good.clone();
        wrong_version.pars[0] = envelope_of_tag([REIFIED_TAG, RECORD_TAG_MARK, MARKER_KIND, 2]);
        assert_eq!(decode_marker(&wrong_version), malformed);
        let mut datum_tag = good.clone();
        datum_tag.pars[0] = envelope_of_tag(RecordKind::Datum.tag_bytes());
        assert_eq!(decode_marker(&datum_tag), malformed);
        let mut short_source = good.clone();
        short_source.pars[0] = envelope_of(vec![7; SOURCE_LEN - 1]);
        assert_eq!(decode_marker(&short_source), malformed);
        let mut long_source = good.clone();
        long_source.pars[0] = envelope_of(vec![7; SOURCE_LEN + 1]);
        assert_eq!(decode_marker(&long_source), malformed);
        let mut stacked = good.clone();
        stacked.cost_stack = Some(CostStack { cells: Vec::new() });
        assert_eq!(decode_marker(&stacked), malformed);
        let mut extra = good.clone();
        extra.pars.push(gint(9));
        assert_eq!(decode_marker(&extra), malformed);
        let mut short = good.clone();
        short.pars.pop();
        assert_eq!(decode_marker(&short), malformed);
        let mut not_a_list = good.clone();
        not_a_list.pars[1] = gint(1);
        assert_eq!(decode_marker(&not_a_list), malformed);
        let mut flagged = good.clone();
        flagged.pars[1].connective_used = true;
        assert_eq!(decode_marker(&flagged), malformed);
        let mut flagged_envelope = good.clone();
        flagged_envelope.pars[0].connective_used = true;
        assert_eq!(decode_marker(&flagged_envelope), malformed);
        let datum_record = wrap_datum(0, &datum(vec![gint(1)]));
        assert_eq!(decode_marker(&datum_record), malformed);
        let counter = counter_record(3, &random(b"counter"), None);
        assert_eq!(decode_marker(&counter), malformed);
        assert_eq!(
            SpaceError::from(MalformedMarker(MarkerShape::Marker)),
            SpaceError::Inconsistent("a malformed marker record")
        );
    }

    fn envelope_of_tag(tag: [u8; 4]) -> Par {
        let mut envelope = envelope_of(vec![7; SOURCE_LEN]);
        if let Some(Expr {
            expr_instance: Some(ExprInstance::EListBody(EList { ps, .. })),
        }) = envelope.exprs.first_mut()
        {
            ps[0] = expr_par(ExprInstance::GByteArray(tag.to_vec()));
        }
        envelope
    }

    #[test]
    fn marker_random_states_differ_by_depth_and_follow_the_path_marker_purpose() {
        let base = random(b"datum");
        let depths: Vec<Vec<u8>> = (0..=4)
            .map(|depth| {
                marker_random(&base, depth)
                    .expect("a small depth fits")
                    .to_bytes()
            })
            .collect();
        for (index, left) in depths.iter().enumerate() {
            for right in &depths[index + 1..] {
                assert_ne!(left, right);
            }
        }
        assert_eq!(
            marker_random(&base, 3).expect("depth 3").to_bytes(),
            base.split_byte(REIFIED_TAG)
                .split_byte(RandomPurpose::PathMarker as u8)
                .split_short(3)
                .to_bytes()
        );
        assert!(matches!(
            marker_random(&base, usize::from(u16::MAX)),
            Err(SpaceError::Inconsistent(_))
        ));
    }

    #[test]
    fn a_vehicle_is_the_marker_followed_by_the_payload() {
        let payload = vec![gint(42), gstring("x")];
        let sent = datum(payload.clone());
        let carried = vehicle(&source(7), vec![gint(2)], &sent);
        assert_eq!(carried.pars.len(), MARKER_PARS + payload.len());
        assert_eq!(carried.random_state, sent.random_state);
        assert_eq!(carried.cost_authority, sent.cost_authority);
        assert_eq!(carried.cost_stack, sent.cost_stack);
        let view = decode_vehicle(&carried).expect("a vehicle decodes");
        assert_eq!(view.payload, payload.as_slice());
        assert_eq!(view.marker.suffix, &[gint(2)]);
        assert!(view.marker.is_of(&source(7)));
        let stored = marker(vec![gint(2)]);
        assert_eq!(&carried.pars[..MARKER_PARS], stored.pars.as_slice());
        assert_eq!(
            decode_marker(&carried),
            Err(MalformedMarker(MarkerShape::Marker))
        );
        assert_eq!(
            decode_vehicle(&ListParWithRandom {
                pars: vec![gint(1)],
                ..ListParWithRandom::default()
            }),
            Err(MalformedMarker(MarkerShape::Vehicle))
        );
    }

    // `marker_record_roundtrip` of `PathMapPrefix.v`: the decode inverts the encode, and the
    // encode is injective on the source and the suffix.
    #[test]
    fn prop_marker_record_roundtrip() {
        check(
            pure_config(),
            (
                any::<[u8; 32]>(),
                any::<[u8; 32]>(),
                path(),
                path(),
                0usize..=64,
            ),
            |(left_source, right_source, left_suffix, right_suffix, depth)| {
                let base = random(b"roundtrip");
                let at = marker_random(&base, depth)
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                let left_hash = Blake2b256Hash::from_bytes(left_source.to_vec());
                let right_hash = Blake2b256Hash::from_bytes(right_source.to_vec());
                let left = marker_record(&left_hash, left_suffix.clone(), &at, None);
                let right = marker_record(&right_hash, right_suffix.clone(), &at, None);
                let view =
                    decode_marker(&left).map_err(|error| TestCaseError::fail(error.to_string()))?;
                ensure(view.source == left_source.as_slice(), "the source survives")?;
                ensure(view.suffix == left_suffix.as_slice(), "the suffix survives")?;
                let same_parts = left_source == right_source && left_suffix == right_suffix;
                ensure(
                    (left.pars == right.pars) == same_parts,
                    "equal marker pars come from equal sources and suffixes",
                )
            },
        );
    }

    // `vehicle_carries_payload` of `PathMapPrefix.v`: the vehicle is the marker of the datum on
    // the prefix, followed by the whole payload, with the random state, the authority and the
    // cost stack of the datum.
    #[test]
    fn prop_vehicle_carries_payload() {
        check(
            pure_config(),
            (
                any::<[u8; 32]>(),
                path(),
                prop::collection::vec(element(), 0..4),
            ),
            |(bytes, suffix, payload)| {
                let hash = Blake2b256Hash::from_bytes(bytes.to_vec());
                let sent = datum(payload.clone());
                let carried = vehicle(&hash, suffix.clone(), &sent);
                let at = marker_random(&random(b"vehicle"), suffix.len())
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                let stored = marker_record(&hash, suffix.clone(), &at, None);
                ensure(
                    carried.pars[..MARKER_PARS] == stored.pars[..],
                    "the vehicle starts with the marker pars",
                )?;
                ensure(
                    carried.pars[MARKER_PARS..] == payload[..],
                    "the vehicle ends with the payload",
                )?;
                ensure(
                    carried.random_state == sent.random_state
                        && carried.cost_authority == sent.cost_authority
                        && carried.cost_stack == sent.cost_stack,
                    "the vehicle keeps the random state, the authority and the stack",
                )?;
                let view = decode_vehicle(&carried)
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                ensure(
                    view.payload == payload.as_slice() && view.marker.suffix == suffix.as_slice(),
                    "the vehicle decodes to its parts",
                )
            },
        );
    }
}
