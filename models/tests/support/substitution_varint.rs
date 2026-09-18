use proptest::prelude::*;

use super::read_protobuf_varint;
use crate::rust::rholang::protobuf_cursor::varint_test_access::scan as scan_protobuf;
use crate::rust::rholang::protobuf_cursor::CanonicalProtobufError;

fn reference(bytes: &[u8], start: usize) -> Option<(u64, usize)> {
    let available = bytes.get(start..)?;
    let last = available.iter().take(10).position(|byte| *byte < 128)?;
    let body = &available[..=last];
    let wide = body
        .iter()
        .rev()
        .fold(0u128, |value, byte| value * 128 + u128::from(byte % 128));
    let value = u64::try_from(wide).ok()?;
    let mut canonical = Vec::new();
    prost::encoding::encode_varint(value, &mut canonical);
    (canonical == body).then_some((value, start + body.len()))
}

fn compare_scanners(bytes: &[u8], start: usize) {
    let expected = reference(bytes, start);
    let mut protobuf_end = start;
    let mut epm_end = start;
    let protobuf = scan_protobuf(bytes, &mut protobuf_end);
    let epm = read_protobuf_varint(bytes, &mut epm_end, "value");
    assert_eq!(protobuf_end, epm_end);
    assert!(protobuf_end >= start && protobuf_end <= bytes.len());
    match (expected, protobuf, epm) {
        (Some((value, end)), Ok(left), Ok(right)) => {
            assert_eq!(left, value);
            assert_eq!(right, value);
            assert_eq!(protobuf_end, end);
            assert_eq!(prost::encoding::encoded_len_varint(value), end - start);
        }
        (None, Err(_), Err(_)) => {}
        (expected, protobuf, epm) => panic!(
            "scanner disagreement at {start} in {bytes:?}: {expected:?}, {protobuf:?}, {epm:?}"
        ),
    }
}

#[test]
fn both_varint_scanners_cover_every_terminal_byte_and_group_position() {
    for prefix in [vec![], vec![99, 0, 255]] {
        for lower in [128, 255] {
            for groups in 0..10 {
                for terminal in 0..=255u8 {
                    let mut bytes = prefix.clone();
                    bytes.extend(std::iter::repeat_n(lower, groups));
                    bytes.push(terminal);
                    bytes.extend_from_slice(&[0, 255, 42]);
                    compare_scanners(&bytes, prefix.len());
                }
            }
        }
    }
}

#[test]
fn both_varint_scanners_cover_u64_boundaries_without_consuming_suffix() {
    let mut values = vec![0, 1, u64::MAX];
    for shift in 1..64 {
        let power = 1u64 << shift;
        values.extend([power - 1, power, power + 1]);
    }
    for value in values {
        for prefix in [vec![], vec![255, 17, 0, 88]] {
            let mut bytes = prefix.clone();
            prost::encoding::encode_varint(value, &mut bytes);
            let end = bytes.len();
            bytes.extend_from_slice(&[255, 255, 0]);
            compare_scanners(&bytes, prefix.len());
            assert_eq!(reference(&bytes, prefix.len()), Some((value, end)));
        }
    }
}

#[test]
fn malformed_varints_preserve_each_scanners_error_class_and_position() {
    for (body, expected_kind, expected_offset) in [
        (vec![], "truncated", 0),
        (vec![128], "truncated", 1),
        (vec![128, 0], "non-canonical", 2),
        ([vec![255; 9], vec![2]].concat(), "overflowing", 10),
        ([vec![128; 10], vec![1]].concat(), "overflowing", 10),
    ] {
        let prefix = [42, 255, 0];
        let bytes = [prefix.as_slice(), body.as_slice()].concat();
        let mut protobuf_end = prefix.len();
        let mut epm_end = prefix.len();
        let protobuf = scan_protobuf(&bytes, &mut protobuf_end).unwrap_err();
        let epm = read_protobuf_varint(&bytes, &mut epm_end, "value").unwrap_err();
        match expected_kind {
            "truncated" => assert!(matches!(
                protobuf,
                CanonicalProtobufError::TruncatedVarint { offset: 3 }
            )),
            "non-canonical" => assert!(matches!(
                protobuf,
                CanonicalProtobufError::NonMinimalVarint { offset: 3 }
            )),
            "overflowing" => assert!(matches!(protobuf, CanonicalProtobufError::VarintOverflow {
                offset: 3
            })),
            _ => unreachable!(),
        }
        assert!(epm.to_string().contains(expected_kind));
        assert_eq!(protobuf_end, prefix.len() + expected_offset);
        assert_eq!(epm_end, protobuf_end);
    }
}

proptest! {
    #[test]
    fn both_varint_scanners_match_widened_reference_on_arbitrary_bytes(
        prefix in prop::collection::vec(any::<u8>(), 0..24),
        body in prop::collection::vec(any::<u8>(), 0..32),
    ) {
        let start = prefix.len();
        let bytes = [prefix, body].concat();
        compare_scanners(&bytes, start);
    }

    #[test]
    fn both_varint_scanners_preserve_generated_values_and_offsets(
        prefix in prop::collection::vec(any::<u8>(), 0..24),
        value in any::<u64>(),
        suffix in prop::collection::vec(any::<u8>(), 0..24),
    ) {
        let start = prefix.len();
        let mut bytes = prefix;
        prost::encoding::encode_varint(value, &mut bytes);
        let end = bytes.len();
        bytes.extend_from_slice(&suffix);
        compare_scanners(&bytes, start);
        prop_assert_eq!(reference(&bytes, start), Some((value, end)));
    }
}
