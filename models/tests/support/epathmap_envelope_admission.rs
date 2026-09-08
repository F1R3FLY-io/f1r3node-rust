use prost::Message;

use super::{canonical_epathmap_parts, canonical_epathmap_root_view};
use crate::rhoapi::var::{VarInstance, WildcardMsg};
use crate::rhoapi::Var;
use crate::rust::rholang::protobuf_encoder;

type Observation<'a> = (&'a [u8], bool, bool, Option<&'a [u8]>);

fn field(key: u64, body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    prost::encoding::encode_varint(key, &mut bytes);
    prost::encoding::encode_varint(body.len() as u64, &mut bytes);
    bytes.extend_from_slice(body);
    bytes
}

fn encode(
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

fn frame<'a>(input: &mut &'a [u8], key: u8) -> Option<&'a [u8]> {
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

fn reference(mut input: &[u8]) -> Option<Observation<'_>> {
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
