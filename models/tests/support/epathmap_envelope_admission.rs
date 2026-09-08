use prost::Message;

use super::{
    canonical_epathmap_parts, canonical_epathmap_root_view, canonical_single_par_component_parts,
    canonical_singleton_epathmap_parts, canonical_singleton_gint_par,
};
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::var::{VarInstance, WildcardMsg};
use crate::rhoapi::{Expr, Par, Var};
use crate::rust::rholang::protobuf_encoder;

type Observation<'a> = (&'a [u8], bool, bool, Option<&'a [u8]>);
type CarrierObservation<'a> = (&'a [u8], &'a [u8], bool);

fn encode_carrier(expr: &[u8], lf: &[u8], connective: bool) -> Vec<u8> {
    let mut bytes = field(42, expr);
    if !lf.is_empty() {
        bytes.extend(field(74, lf));
    }
    if connective {
        bytes.extend([80, 1]);
    }
    bytes
}

fn observe_carrier(input: &[u8]) -> Option<CarrierObservation<'_>> {
    canonical_single_par_component_parts(input, 5)
        .map(|parts| (parts.body, parts.locally_free, parts.connective_used))
}

pub(super) fn reference_carrier(mut input: &[u8]) -> Option<CarrierObservation<'_>> {
    let expr = frame(&mut input, 42)?;
    let lf = if input.first() == Some(&74) {
        let lf = frame(&mut input, 74)?;
        if lf.is_empty() {
            return None;
        }
        lf
    } else {
        &[]
    };
    let connective = match input {
        [] => false,
        [80, 1] => true,
        _ => return None,
    };
    Some((expr, lf, connective))
}

fn integer_image(value: i64) -> Vec<u8> {
    let mut expr = vec![16];
    prost::encoding::encode_varint(zigzag_word(value), &mut expr);
    field(42, &expr)
}

fn zigzag_word(value: i64) -> u64 { ((value as u64) << 1) ^ ((value >> 63) as u64) }

fn generated_integer(value: i64) -> Vec<u8> {
    protobuf_encoder::encode_to_vec(&Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }]))
}

pub(super) fn field(key: u64, body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    prost::encoding::encode_varint(key, &mut bytes);
    prost::encoding::encode_varint(body.len() as u64, &mut bytes);
    bytes.extend_from_slice(body);
    bytes
}

pub(super) fn encode(
    lf: Option<&[u8]>,
    connective: bool,
    remainder: Option<&[u8]>,
    snapshot: &[u8],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    if let Some(lf) = lf {
        bytes.extend(field(26, lf));
    }
    if connective {
        bytes.extend([32, 1]);
    }
    if let Some(remainder) = remainder {
        bytes.extend(field(42, remainder));
    }
    bytes.extend(field(74, snapshot));
    bytes
}

fn observe(bytes: &[u8]) -> Option<Observation<'_>> {
    canonical_epathmap_parts(bytes).map(|parts| {
        (
            parts.snapshot,
            parts.connective_used,
            parts.locally_free_is_empty,
            parts.remainder,
        )
    })
}

pub(super) fn frame<'a>(input: &mut &'a [u8], key: u8) -> Option<&'a [u8]> {
    let (&head, rest) = input.split_first()?;
    if head != key {
        return None;
    }
    let mut payload = rest;
    let len = prost::encoding::decode_varint(&mut payload).ok()?;
    let mut header = Vec::new();
    prost::encoding::encode_varint(len, &mut header);
    if rest.get(..rest.len() - payload.len())? != header {
        return None;
    }
    let len = usize::try_from(len).ok()?;
    let body = payload.get(..len)?;
    *input = &payload[len..];
    Some(body)
}

pub(super) fn reference(mut input: &[u8]) -> Option<Observation<'_>> {
    let locally_free_is_empty = if input.first() == Some(&26) {
        if frame(&mut input, 26)?.is_empty() {
            return None;
        }
        false
    } else {
        true
    };
    let connective = if input.first() == Some(&32) {
        input = input.strip_prefix(&[32, 1])?;
        true
    } else {
        false
    };
    let remainder = if input.first() == Some(&42) {
        let body = frame(&mut input, 42)?;
        let decoded = Var::decode(body).ok()?;
        if protobuf_encoder::encode_to_vec(&decoded) != body {
            return None;
        }
        Some(body)
    } else {
        None
    };
    let snapshot = frame(&mut input, 74)?;
    input
        .is_empty()
        .then_some((snapshot, connective, locally_free_is_empty, remainder))
}

fn remainders(value: i32) -> [Option<Vec<u8>>; 5] {
    [
        None,
        Some(Vec::new()),
        Some(protobuf_encoder::encode_to_vec(&Var {
            var_instance: Some(VarInstance::BoundVar(value)),
        })),
        Some(protobuf_encoder::encode_to_vec(&Var {
            var_instance: Some(VarInstance::FreeVar(value)),
        })),
        Some(protobuf_encoder::encode_to_vec(&Var {
            var_instance: Some(VarInstance::Wildcard(WildcardMsg {})),
        })),
    ]
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config {
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("regressions"),
        )),
        ..proptest::test_runner::Config::with_cases(1_024)
    })]

    #[test]
    fn canonical_epathmap_envelope_proof_images_and_presence(
        value in proptest::prelude::any::<i32>(),
        lf in proptest::collection::vec(proptest::prelude::any::<u8>(), 1..33),
        snapshot in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..257),
    ) {
        for lf in [None, Some(lf.as_slice())] {
            for connective in [false, true] {
                for remainder in remainders(value) {
                    let bytes = encode(lf, connective, remainder.as_deref(), &snapshot);
                    let expected = Some((snapshot.as_slice(), connective, lf.is_none(), remainder.as_deref()));
                    proptest::prop_assert_eq!(observe(&bytes), expected);
                    proptest::prop_assert_eq!(reference(&bytes), expected);
                }
            }
        }
    }

    #[test]
    fn canonical_epathmap_envelope_proof_ordered_cursor_correspondence(
        value in proptest::prelude::any::<i32>(),
        arbitrary in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..65),
        snapshot in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..257),
    ) {
        let remainder = remainders(value)[2].clone().unwrap();
        let fields = [field(26, &[0]), vec![32, 1], field(42, &remainder), field(74, &snapshot)];
        let canonical = fields.concat();
        let mut candidates = vec![arbitrary, canonical.clone()];
        for at in 0..fields.len() {
            let mut duplicate = fields[..at].concat();
            duplicate.extend_from_slice(&fields[at]);
            duplicate.extend_from_slice(&fields[at..].concat());
            candidates.push(duplicate);
            let mut padded = canonical.clone();
            let start: usize = fields[..at].iter().map(Vec::len).sum();
            padded[start] |= 0x80;
            padded.insert(start + 1, 0);
            candidates.push(padded);
            if at + 1 < fields.len() {
                let mut reordered = fields.clone();
                reordered.swap(at, at + 1);
                candidates.push(reordered.concat());
            }
        }
        candidates.push(canonical[..canonical.len() - 1].to_vec());
        for (index, bytes) in candidates.iter().enumerate() {
            let actual = observe(bytes);
            proptest::prop_assert_eq!(actual, reference(bytes), "candidate {}: {:02x?}", index, bytes);
            if index == 1 {
                proptest::prop_assert!(actual.is_some(), "guaranteed positive control");
            } else if index > 1 {
                proptest::prop_assert!(actual.is_none(), "mutation {} was admitted", index);
            }
        }
    }

    #[test]
    fn canonical_epathmap_envelope_proof_late_remainder_refusal(
        lf in proptest::collection::vec(proptest::prelude::any::<u8>(), 1..33),
        suffix in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..65),
    ) {
        for lf in [None, Some(lf.as_slice())] {
            for connective in [false, true] {
                for malformed in [vec![8], vec![32, 0], vec![8, 128, 0], vec![26, 1, 0]] {
                    let mut bytes = encode(lf, connective, Some(&malformed), &[]);
                    proptest::prop_assert!(observe(&bytes).is_none());
                    proptest::prop_assert!(reference(&bytes).is_none());
                    bytes.extend_from_slice(&suffix);
                    proptest::prop_assert!(observe(&bytes).is_none());
                    proptest::prop_assert!(reference(&bytes).is_none());
                }
            }
        }
    }

    #[test]
    fn canonical_epathmap_envelope_proof_opaque_snapshot_and_borrowing(
        snapshot in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..513),
        value in proptest::prelude::any::<i32>(),
    ) {
        let remainder = remainders(value)[3].clone().unwrap();
        let bytes = encode(None, false, Some(&remainder), &snapshot);
        let observed = canonical_epathmap_parts(&bytes).unwrap();
        let snapshot_start = bytes.len() - snapshot.len();
        proptest::prop_assert_eq!(observed.snapshot, snapshot.as_slice());
        proptest::prop_assert_eq!(observed.snapshot.as_ptr(), bytes[snapshot_start..].as_ptr());
        proptest::prop_assert_eq!(observed.remainder.unwrap().as_ptr(), bytes[2..].as_ptr());
        proptest::prop_assert_eq!(observed.remainder, Some(remainder.as_slice()));
    }

    #[test]
    fn canonical_epathmap_carrier_proof_ordered_images(
        expr in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..129),
        lf in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..33),
        connective in proptest::prelude::any::<bool>(),
        arbitrary in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..65),
    ) {
        let bytes = encode_carrier(&expr, &lf, connective);
        let expected = Some((expr.as_slice(), lf.as_slice(), connective));
        proptest::prop_assert_eq!(observe_carrier(&bytes), expected);
        proptest::prop_assert_eq!(reference_carrier(&bytes), expected);
        proptest::prop_assert_eq!(observe_carrier(&arbitrary), reference_carrier(&arbitrary));
        let mut duplicate = bytes.clone();
        duplicate.extend(field(42, &expr));
        let mut reordered = field(74, &[7]);
        reordered.extend_from_slice(&bytes);
        let mut padded_key = bytes.clone();
        padded_key[0] |= 0x80;
        padded_key.insert(1, 0);
        let mut wrong_family = bytes.clone();
        wrong_family[0] = 34;
        for malformed in [duplicate, reordered, padded_key, wrong_family] {
            proptest::prop_assert_eq!(observe_carrier(&malformed), None);
            proptest::prop_assert_eq!(reference_carrier(&malformed), None);
        }
    }

    #[test]
    fn canonical_epathmap_carrier_proof_nested_metadata_images(
        snapshot in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..129),
        root_lf in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..17),
        map_lf in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..17),
        value in proptest::prelude::any::<i32>(),
    ) {
        let lf = (!map_lf.is_empty()).then_some(map_lf.as_slice());
        for root_connective in [false, true] {
            for map_connective in [false, true] {
                for remainder in remainders(value) {
                    let body = encode(lf, map_connective, remainder.as_deref(), &snapshot);
                    let expr = field(258, &body);
                    let bytes = encode_carrier(&expr, &root_lf, root_connective);
                    let actual = canonical_singleton_epathmap_parts(&bytes).unwrap();
                    let (reference_expr, reference_lf, reference_connective) =
                        reference_carrier(&bytes).unwrap();
                    let mut after_first_key_byte = reference_expr.strip_prefix(&[130]).unwrap();
                    let reference_body = frame(&mut after_first_key_byte, 2).unwrap();
                    proptest::prop_assert!(after_first_key_byte.is_empty());
                    let reference_map = reference(reference_body).unwrap();
                    proptest::prop_assert_eq!(actual.expr_body, reference_expr);
                    proptest::prop_assert_eq!(actual.snapshot, reference_map.0);
                    proptest::prop_assert_eq!(actual.epathmap_connective_used, reference_map.1);
                    proptest::prop_assert_eq!(actual.epathmap_locally_free_is_empty, reference_map.2);
                    proptest::prop_assert_eq!(actual.epathmap_has_remainder, reference_map.3.is_some());
                    proptest::prop_assert_eq!(actual.locally_free, reference_lf);
                    proptest::prop_assert_eq!(actual.connective_used, reference_connective);
                    proptest::prop_assert_eq!(actual.expr_body, expr.as_slice());
                    proptest::prop_assert_eq!(actual.locally_free, root_lf.as_slice());
                    proptest::prop_assert_eq!(reference_map.3, remainder.as_deref());
                    proptest::prop_assert!(!canonical_singleton_gint_par(&bytes));
                }
            }
        }
    }

    #[test]
    fn canonical_epathmap_carrier_proof_exact_int64_terminal(
        value in proptest::prelude::any::<i64>(),
    ) {
        let bytes = integer_image(value);
        proptest::prop_assert_eq!(&bytes, &generated_integer(value));
        proptest::prop_assert!(canonical_singleton_gint_par(&bytes));
        let mut expr = vec![16];
        prost::encoding::encode_varint(zigzag_word(value), &mut expr);
        let mut padded = expr.clone();
        *padded.last_mut().unwrap() |= 128;
        padded.push(0);
        let mut trailing = bytes.clone();
        trailing.extend([80, 1]);
        let mut duplicate = bytes.clone();
        duplicate.extend_from_slice(&bytes);
        for malformed in [field(42, &padded), trailing, duplicate,
            field(42, &expr[..expr.len() - 1])] {
            proptest::prop_assert!(!canonical_singleton_gint_par(&malformed));
        }
    }

    #[test]
    fn canonical_epathmap_carrier_proof_map_shape_excludes_integer(
        body in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..257),
        lf in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..33),
        connective in proptest::prelude::any::<bool>(),
    ) {
        for body in [body, vec![8, 0], vec![42, 1, 8, 74, 0], vec![74, 0]] {
            let expr = field(258, &body);
            let bytes = encode_carrier(&expr, &lf, connective);
            proptest::prop_assert!(observe_carrier(&bytes).is_some());
            proptest::prop_assert!(!canonical_singleton_gint_par(&bytes));
        }
    }
}

#[test]
fn canonical_epathmap_carrier_proof_boundary_controls() {
    assert_eq!(integer_image(-1), vec![42, 2, 16, 1]);
    assert_eq!(integer_image(0), vec![42, 2, 16, 0]);
    assert_eq!(integer_image(1), vec![42, 2, 16, 2]);
    assert_eq!(zigzag_word(i64::MIN), u64::MAX);
    assert_eq!(zigzag_word(i64::MAX), u64::MAX - 1);
    assert_ne!(zigzag_word(-1), (-1i64) as u64);
    assert_eq!(
        observe_carrier(&[42, 0, 74, 1, 7, 80, 1]),
        Some((&[][..], &[7][..], true))
    );
    for malformed in [
        vec![42, 0, 74, 0],
        vec![42, 0, 80, 0],
        vec![74, 1, 7, 42, 0],
        vec![42, 0, 42, 0],
        vec![42, 0, 80, 2],
        vec![42, 0, 80, 129, 0],
        vec![42, 0, 74, 1, 7, 74, 1, 7],
    ] {
        assert_eq!(observe_carrier(&malformed), None);
        assert_eq!(reference_carrier(&malformed), None);
    }
    for value in [i64::MIN, -1, 0, 1, i64::MAX] {
        let bytes = integer_image(value);
        assert_eq!(bytes, generated_integer(value));
        assert!(canonical_singleton_gint_par(&bytes));
    }
    for malformed in [
        vec![42, 3, 16, 128, 0],
        vec![42, 11, 16, 128, 128, 128, 128, 128, 128, 128, 128, 128, 2],
        vec![42, 2, 16, 0, 80, 1],
    ] {
        assert!(!canonical_singleton_gint_par(&malformed));
    }
}

#[test]
fn canonical_epathmap_envelope_proof_refusal_controls() {
    for bytes in [
        vec![32, 1, 26, 1, 7, 74, 0],
        vec![26, 1, 7, 26, 1, 8, 74, 0],
        vec![32, 1, 32, 1, 74, 0],
        vec![42, 0, 42, 0, 74, 0],
        vec![74, 0, 74, 0],
        vec![74, 0, 32, 1],
        vec![],
        vec![26, 0, 74, 0],
        vec![32, 0, 74, 0],
        vec![32, 129, 0, 74, 0],
        vec![24, 1, 74, 0],
        vec![8, 0, 74, 0],
        vec![202, 0, 0],
        vec![74, 128, 0],
        vec![74, 2, 0],
        vec![26, 1, 7, 32, 1, 42, 1, 8, 74, 0],
    ] {
        assert_eq!(observe(&bytes), None, "{bytes:02x?}");
        assert_eq!(reference(&bytes), None, "{bytes:02x?}");
    }
    assert_eq!(observe(&[74, 0]), Some((&[][..], false, true, None)));
    assert_eq!(
        observe(&[42, 0, 74, 0]),
        Some((&[][..], false, true, Some(&[][..])))
    );
    for snapshot in [vec![], vec![255]] {
        let bytes = encode(None, false, None, &snapshot);
        assert!(observe(&bytes).is_some());
        assert!(canonical_epathmap_root_view(&field(258, &bytes)).is_none());
    }
}
