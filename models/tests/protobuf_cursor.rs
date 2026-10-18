//! Totality, canonicality, and restartability gates for the borrowed and owned
//! protobuf cursors used by byte-native process analyses.

use models::rust::rholang::protobuf_cursor::{
    CanonicalByteRange, CanonicalDelimitedRun, CanonicalField, CanonicalFieldCursor,
    CanonicalFieldValue, CanonicalParProcessIndex, CanonicalParRegionIndex, CanonicalProtobufError,
    CanonicalRegionDelimitedRun, CanonicalRegionField, CanonicalRegionFieldCursor,
    CanonicalRegionFieldValue, CanonicalWireType, ProtobufRegion,
};
use proptest::prelude::*;
use prost::bytes::Bytes;

fn check_field_partition(bytes: &[u8], numeric_order: bool) -> bool {
    let mut borrowed = if numeric_order {
        CanonicalFieldCursor::new(bytes)
    } else {
        CanonicalFieldCursor::in_prost_slot_order(bytes)
    };
    let region = ProtobufRegion::from(bytes.to_vec());
    let mut owned = if numeric_order {
        CanonicalRegionFieldCursor::new(region)
    } else {
        CanonicalRegionFieldCursor::in_prost_slot_order(region)
    };
    let mut end = 0;
    for _ in 0..=bytes.len() {
        match (borrowed.next_field(), owned.next_field()) {
            (Ok(Some(field)), Ok(Some(retained))) => {
                assert_eq!(field.encoded_range.start, end);
                assert!(field.encoded_range.end > end);
                assert!(field.encoded_range.end <= bytes.len());
                assert!(field.payload_range.start >= field.encoded_range.start);
                assert_eq!(field.payload_range.end, field.encoded_range.end);
                let mut rebuilt = Vec::new();
                let wire = match field.wire_type {
                    CanonicalWireType::Varint => 0,
                    CanonicalWireType::Fixed64 => 1,
                    CanonicalWireType::LengthDelimited => 2,
                    CanonicalWireType::Fixed32 => 5,
                };
                prost::encoding::encode_varint((u64::from(field.number) << 3) | wire, &mut rebuilt);
                match &field.value {
                    CanonicalFieldValue::Varint(value) => {
                        prost::encoding::encode_varint(*value, &mut rebuilt)
                    }
                    CanonicalFieldValue::Fixed64(body) | CanonicalFieldValue::Fixed32(body) => {
                        rebuilt.extend_from_slice(body)
                    }
                    CanonicalFieldValue::LengthDelimited(body) => {
                        prost::encoding::encode_varint(body.len() as u64, &mut rebuilt);
                        rebuilt.extend_from_slice(body);
                    }
                }
                assert_eq!(rebuilt, &bytes[end..field.encoded_range.end]);
                end = field.encoded_range.end;
                assert_field_equivalent(field, retained);
            }
            (Ok(None), Ok(None)) => {
                assert_eq!(end, bytes.len());
                assert!(owned.next_field().unwrap().is_none());
                return true;
            }
            (Err(left), Err(right)) => {
                assert_eq!(left, right);
                assert_eq!(owned.next_field().unwrap_err(), right);
                assert_eq!(owned.next_field().unwrap_err(), right);
                return false;
            }
            (left, right) => panic!("partition cursor disagreement: {left:?} versus {right:?}"),
        }
    }
    panic!("successful fields exceeded the source-byte bound");
}

#[test]
fn canonical_varint_boundaries_reconstruct_exact_field_bytes() {
    let mut values = vec![0, 1, u64::MAX];
    for shift in 1..64 {
        let power = 1u64 << shift;
        values.extend([power - 1, power, power + 1]);
    }
    for value in values {
        for tag in [1u32, 15, 16, 2047, 2048, 0x1fff_ffff] {
            let mut bytes = Vec::new();
            prost::encoding::encode_varint(u64::from(tag) << 3, &mut bytes);
            prost::encoding::encode_varint(value, &mut bytes);
            assert!(check_field_partition(&bytes, true));
            assert!(check_field_partition(&bytes, false));
        }
    }
}

#[test]
fn owned_cursor_refuses_and_retries_malformed_fields_without_consuming_them() {
    let mut tenth_overflow = vec![8];
    tenth_overflow.extend([0x80; 9]);
    tenth_overflow.push(2);
    let mut eleventh_group = vec![8];
    eleventh_group.extend([0x80; 10]);
    eleventh_group.push(1);
    for bad in [
        vec![8, 0x80, 0],
        vec![8, 0x80],
        vec![0],
        vec![0x0b],
        vec![0x0a, 2, 42],
        vec![9, 1, 2],
        vec![0x0d, 1, 2],
        tenth_overflow,
        eleventh_group,
    ] {
        for prefix in [vec![], vec![8, 42]] {
            let bytes: Vec<_> = prefix.into_iter().chain(bad.iter().copied()).collect();
            assert!(!check_field_partition(&bytes, true));
            assert!(!check_field_partition(&bytes, false));
        }
    }
    assert!(!check_field_partition(&[16, 1, 8, 1], true));
    assert!(check_field_partition(&[16, 1, 8, 1], false));
}

proptest! {
    #[test]
    fn arbitrary_successful_fields_partition_and_reencode_source(
        bytes in prop::collection::vec(any::<u8>(), 0..4096)
    ) {
        check_field_partition(&bytes, true);
        check_field_partition(&bytes, false);
    }

    #[test]
    fn generated_fields_partition_source_in_numeric_and_producer_order(
        fields in prop::collection::vec(
            (0u8..4, any::<u64>(), prop::collection::vec(any::<u8>(), 0..128)), 0..32)
    ) {
        for numeric in [true, false] {
            let mut bytes = Vec::new();
            for (index, (kind, value, body)) in fields.iter().enumerate() {
                let tag = if numeric { index as u64 + 1 } else { 32 - index as u64 };
                let wire = [0u64, 1, 2, 5][usize::from(*kind)];
                prost::encoding::encode_varint(tag << 3 | wire, &mut bytes);
                match kind {
                    0 => prost::encoding::encode_varint(*value, &mut bytes),
                    1 => bytes.extend_from_slice(&value.to_le_bytes()),
                    2 => {
                        prost::encoding::encode_varint(body.len() as u64, &mut bytes);
                        bytes.extend_from_slice(body);
                    }
                    3 => bytes.extend_from_slice(&(*value as u32).to_le_bytes()),
                    _ => unreachable!(),
                }
            }
            prop_assert!(check_field_partition(&bytes, numeric));
        }
    }
}

fn assert_field_equivalent(borrowed: CanonicalField<'_>, owned: CanonicalRegionField) {
    assert_eq!(borrowed.number, owned.number);
    assert_eq!(borrowed.wire_type, owned.wire_type);
    assert_eq!(borrowed.encoded_range, owned.encoded_range);
    assert_eq!(borrowed.payload_range, owned.payload_range);
    match (borrowed.value, owned.value) {
        (CanonicalFieldValue::Varint(left), CanonicalRegionFieldValue::Varint(right)) => {
            assert_eq!(left, right);
        }
        (CanonicalFieldValue::Fixed64(left), CanonicalRegionFieldValue::Fixed64(right))
        | (
            CanonicalFieldValue::LengthDelimited(left),
            CanonicalRegionFieldValue::LengthDelimited(right),
        )
        | (CanonicalFieldValue::Fixed32(left), CanonicalRegionFieldValue::Fixed32(right)) => {
            assert_eq!(left, right.as_bytes());
        }
        (left, right) => panic!("borrowed/owned field kind mismatch: {left:?} versus {right:?}"),
    }
}

#[test]
fn cursor_frames_every_non_group_wire_type_without_copying_payloads() {
    let message = [
        0x08, 0x96, 0x01, // field 1, varint 150
        0x11, 1, 2, 3, 4, 5, 6, 7, 8, // field 2, fixed64
        0x1a, 0x02, 9, 10, // field 3, length-delimited
        0x25, 11, 12, 13, 14, // field 4, fixed32
    ];
    let mut cursor = CanonicalFieldCursor::new(&message);

    let varint = cursor.next_field().unwrap().unwrap();
    assert_eq!(varint.number, 1);
    assert_eq!(varint.wire_type, CanonicalWireType::Varint);
    assert_eq!(varint.value, CanonicalFieldValue::Varint(150));

    let fixed64 = cursor.next_field().unwrap().unwrap();
    assert_eq!(fixed64.number, 2);
    assert_eq!(fixed64.value, CanonicalFieldValue::Fixed64(&message[4..12]));

    let delimited = cursor.next_field().unwrap().unwrap();
    assert_eq!(delimited.number, 3);
    assert_eq!(delimited.expect_length_delimited().unwrap(), &[9, 10]);
    assert_eq!(delimited.payload_range.slice(&message).unwrap(), &[9, 10]);

    let fixed32 = cursor.next_field().unwrap().unwrap();
    assert_eq!(fixed32.number, 4);
    assert_eq!(
        fixed32.value,
        CanonicalFieldValue::Fixed32(&[11, 12, 13, 14])
    );
    assert!(cursor.next_field().unwrap().is_none());
    assert!(cursor.is_empty());
}

#[test]
fn owned_regions_slice_one_backing_allocation_and_match_borrowed_fields() {
    let message = vec![
        0x08, 0x96, 0x01, 0x11, 1, 2, 3, 4, 5, 6, 7, 8, 0x1a, 0x02, 9, 10, 0x25, 11, 12, 13, 14,
    ];
    let root = ProtobufRegion::from(message.clone());
    let child = root
        .subregion(CanonicalByteRange { start: 14, end: 16 })
        .unwrap();
    assert_eq!(child.as_bytes(), &[9, 10]);
    assert_eq!(child.absolute_range(), 14..16);
    assert_eq!(child.as_bytes().as_ptr(), root.as_bytes()[14..].as_ptr());
    let grandchild = child
        .subregion(CanonicalByteRange { start: 1, end: 2 })
        .unwrap();
    assert_eq!(grandchild.as_bytes(), &[10]);
    assert_eq!(grandchild.absolute_range(), 15..16);
    assert_eq!(
        grandchild.as_bytes().as_ptr(),
        root.as_bytes()[15..].as_ptr()
    );

    let mut borrowed = CanonicalFieldCursor::new(&message);
    let mut owned = CanonicalRegionFieldCursor::new(root);
    loop {
        match (borrowed.next_field().unwrap(), owned.next_field().unwrap()) {
            (Some(left), Some(right)) => assert_field_equivalent(left, right),
            (None, None) => break,
            (left, right) => panic!("borrowed/owned cursor length mismatch: {left:?} {right:?}"),
        }
    }
}

#[test]
fn owned_slot_order_cursor_matches_borrowed_slot_order_without_weakening_numeric_mode() {
    // A generated oneof occupies the slot of its minimum arm tag, but an
    // active arm may emit tag 3 before a later ordinary slot emits tag 1.
    let message = vec![0x1a, 0x00, 0x0a, 0x00];

    let mut borrowed = CanonicalFieldCursor::in_prost_slot_order(&message);
    let mut owned =
        CanonicalRegionFieldCursor::in_prost_slot_order(ProtobufRegion::from(message.clone()));
    loop {
        match (borrowed.next_field().unwrap(), owned.next_field().unwrap()) {
            (Some(left), Some(right)) => assert_field_equivalent(left, right),
            (None, None) => break,
            (left, right) => panic!("slot-order cursor length mismatch: {left:?} {right:?}"),
        }
    }

    let mut numeric = CanonicalRegionFieldCursor::new(ProtobufRegion::from(message));
    assert_eq!(numeric.next_field().unwrap().unwrap().number, 3);
    assert!(matches!(
        numeric.next_field(),
        Err(CanonicalProtobufError::DecreasingFieldOrder {
            previous: 3,
            current: 1,
            ..
        })
    ));
}

#[test]
fn cursor_rejects_every_representable_framing_and_order_failure_class() {
    let cases: &[(&[u8], fn(&CanonicalProtobufError) -> bool)] = &[
        (&[0x08, 0x80], |error| {
            matches!(error, CanonicalProtobufError::TruncatedVarint { .. })
        }),
        (&[0x08, 0x80, 0x00], |error| {
            matches!(error, CanonicalProtobufError::NonMinimalVarint { .. })
        }),
        (
            &[
                0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02,
            ],
            |error| matches!(error, CanonicalProtobufError::VarintOverflow { .. }),
        ),
        (&[0x00, 0x00], |error| {
            matches!(error, CanonicalProtobufError::InvalidFieldNumber { .. })
        }),
        (&[0x0b], |error| {
            matches!(error, CanonicalProtobufError::UnsupportedWireType {
                wire: 3,
                ..
            })
        }),
        (&[0x10, 0x01, 0x08, 0x01], |error| {
            matches!(error, CanonicalProtobufError::DecreasingFieldOrder { .. })
        }),
        (&[0x0a, 0x02, 0x01], |error| {
            matches!(error, CanonicalProtobufError::TruncatedField { .. })
        }),
        (&[0x09, 1, 2, 3, 4, 5, 6, 7], |error| {
            matches!(error, CanonicalProtobufError::TruncatedField { .. })
        }),
        (&[0x0d, 1, 2, 3], |error| {
            matches!(error, CanonicalProtobufError::TruncatedField { .. })
        }),
    ];

    for (message, predicate) in cases {
        let mut cursor = CanonicalFieldCursor::new(message);
        let error = loop {
            match cursor.next_field() {
                Ok(Some(_)) => continue,
                Ok(None) => panic!("malformed fixture was accepted: {message:?}"),
                Err(error) => break error,
            }
        };
        assert!(predicate(&error), "wrong error for {message:?}: {error:?}");
    }
}

#[cfg(target_pointer_width = "32")]
#[test]
fn cursor_rejects_lengths_that_do_not_fit_usize() {
    let mut cursor = CanonicalFieldCursor::new(&[0x0a, 0x80, 0x80, 0x80, 0x80, 0x10]);
    assert!(matches!(
        cursor.next_field(),
        Err(CanonicalProtobufError::LengthOverflow { .. })
    ));
}

#[test]
fn delimited_run_rejects_wrong_tags_and_wire_types_but_accepts_present_empty() {
    let mut wrong_tag = CanonicalDelimitedRun::new(&[0x12, 0x00], 1).cursor();
    assert!(matches!(
        wrong_tag.next_payload(),
        Err(CanonicalProtobufError::UnexpectedField { field: 2, .. })
    ));

    let mut wrong_wire = CanonicalDelimitedRun::new(&[0x08, 0x00], 1).cursor();
    assert!(matches!(
        wrong_wire.next_payload(),
        Err(CanonicalProtobufError::UnexpectedWireType {
            expected: CanonicalWireType::LengthDelimited,
            actual: CanonicalWireType::Varint,
            ..
        })
    ));

    let mut present_empty = CanonicalDelimitedRun::new(&[0x0a, 0x00], 1).cursor();
    assert_eq!(present_empty.next_payload().unwrap(), Some(&[][..]));
    assert!(present_empty.next_payload().unwrap().is_none());
}

#[test]
fn owned_and_borrowed_delimited_runs_are_restartable_and_differential() {
    let encoded = [0x0a, 0x02, 0xa1, 0xa2, 0x0a, 0x00, 0x0a, 0x01, 0xb1];
    let borrowed = CanonicalDelimitedRun::new(&encoded, 1);
    let owned = CanonicalRegionDelimitedRun::new(ProtobufRegion::copy_from_slice(&encoded), 1);

    let collect_borrowed = || {
        let mut cursor = borrowed.cursor();
        let mut values = Vec::new();
        while let Some(payload) = cursor.next_payload().unwrap() {
            values.push(payload.to_vec());
        }
        values
    };
    let collect_owned = || {
        let mut cursor = owned.cursor();
        let mut values = Vec::new();
        while let Some(payload) = cursor.next_region().unwrap() {
            values.push(payload.as_bytes().to_vec());
        }
        values
    };
    assert_eq!(collect_borrowed(), collect_owned());
    assert_eq!(collect_owned(), vec![vec![0xa1, 0xa2], vec![], vec![0xb1]]);
}

#[test]
fn par_index_reorders_runs_semantically_and_delimited_cursors_restart() {
    // Protobuf order: Send(1), Receive(2), New(4), Expr(5), Match(6),
    // metadata(9,10), Bundle(11), If(12). Payload bytes are arbitrary here;
    // the index owns framing, while the typed accounting consumer owns each
    // component schema.
    let message = [
        0x0a, 0x01, 0xa1, // Send #1
        0x0a, 0x01, 0xa2, // Send #2
        0x12, 0x01, 0xb1, // Receive
        0x22, 0x01, 0xc1, // New
        0x2a, 0x01, 0xd1, // Expr
        0x32, 0x01, 0xe1, // Match
        0x4a, 0x01, 0x01, // locallyFree
        0x50, 0x01, // connective_used
        0x5a, 0x01, 0xf1, // Bundle
        0x62, 0x01, 0xf2, // If
        0x6a, 0x01, 0xf3,
    ];
    let index = CanonicalParProcessIndex::parse(&message).unwrap();

    let send_run = index.sends.unwrap();
    let collect = || {
        let mut cursor = send_run.cursor();
        let mut values = Vec::new();
        while let Some(payload) = cursor.next_payload().unwrap() {
            values.push(payload[0]);
        }
        values
    };
    assert_eq!(collect(), vec![0xa1, 0xa2]);
    assert_eq!(collect(), vec![0xa1, 0xa2], "a run must be restartable");

    let singleton = |run: Option<CanonicalDelimitedRun<'_>>| {
        let mut cursor = run.unwrap().cursor();
        let payload = cursor.next_payload().unwrap().unwrap()[0];
        assert!(cursor.next_payload().unwrap().is_none());
        payload
    };
    assert_eq!(singleton(index.receives), 0xb1);
    assert_eq!(singleton(index.news), 0xc1);
    assert_eq!(singleton(index.matches), 0xe1);
    assert_eq!(singleton(index.conditionals), 0xf2);
    assert_eq!(singleton(index.bundles), 0xf1);
    assert_eq!(singleton(index.exprs), 0xd1);
    assert_eq!(singleton(index.cost_token_stacks), 0xf3);
}

#[test]
fn owned_par_index_covers_every_family_and_root_metadata_without_decoding() {
    let message = Bytes::from_static(&[
        0x0a, 0x01, 0xa1, // Send
        0x12, 0x01, 0xb1, // Receive
        0x22, 0x01, 0xc1, // New
        0x2a, 0x01, 0xd1, // Expr
        0x32, 0x01, 0xe1, // Match
        0x3a, 0x01, 0xe2, // Unforgeable
        0x42, 0x01, 0xe3, // Connective
        0x4a, 0x01, 0x01, // locallyFree
        0x50, 0x01, // connective_used
        0x5a, 0x01, 0xf1, // Bundle
        0x62, 0x01, 0xf2, // If
        0x6a, 0x01, 0xf3,
    ]);
    let index = CanonicalParRegionIndex::parse(ProtobufRegion::from(message)).unwrap();

    let singleton = |run: &models::rust::rholang::protobuf_cursor::CanonicalRegionDelimitedRun| {
        let mut cursor = run.cursor();
        let payload = cursor.next_region().unwrap().unwrap().as_bytes()[0];
        assert!(cursor.next_region().unwrap().is_none());
        payload
    };
    assert_eq!(singleton(index.sends.as_ref().unwrap()), 0xa1);
    assert_eq!(singleton(index.receives.as_ref().unwrap()), 0xb1);
    assert_eq!(singleton(index.news.as_ref().unwrap()), 0xc1);
    assert_eq!(singleton(index.exprs.as_ref().unwrap()), 0xd1);
    assert_eq!(singleton(index.matches.as_ref().unwrap()), 0xe1);
    assert_eq!(singleton(index.unforgeables.as_ref().unwrap()), 0xe2);
    assert_eq!(singleton(index.connectives.as_ref().unwrap()), 0xe3);
    assert_eq!(singleton(index.bundles.as_ref().unwrap()), 0xf1);
    assert_eq!(singleton(index.conditionals.as_ref().unwrap()), 0xf2);
    assert_eq!(singleton(index.cost_token_stacks.as_ref().unwrap()), 0xf3);
    assert_eq!(index.locally_free.unwrap().as_bytes(), &[1]);
    assert!(index.connective_used);
}

#[test]
fn owned_par_index_distinguishes_absent_from_present_empty_message_fields() {
    let absent = CanonicalParRegionIndex::parse(ProtobufRegion::from(Vec::new())).unwrap();
    assert!(absent.sends.is_none());

    let present = CanonicalParRegionIndex::parse(ProtobufRegion::from(vec![0x0a, 0x00])).unwrap();
    let mut sends = present.sends.unwrap().cursor();
    assert!(sends.next_region().unwrap().unwrap().is_empty());
    assert!(sends.next_region().unwrap().is_none());
}

#[test]
fn par_index_rejects_noncanonical_metadata_defaults_and_duplicates() {
    for message in [
        &[0x4a, 0x00][..],             // explicitly empty locallyFree
        &[0x50, 0x00][..],             // explicitly false connective_used
        &[0x50, 0x01, 0x50, 0x01][..], // duplicate connective_used
    ] {
        assert!(CanonicalParProcessIndex::parse(message).is_err());
    }
}

proptest! {
    #[test]
    fn arbitrary_bytes_are_total_and_make_bounded_progress(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        let mut cursor = CanonicalFieldCursor::new(&bytes);
        for _ in 0..=bytes.len() {
            match cursor.next_field() {
                Ok(Some(_)) => continue,
                Ok(None) | Err(_) => return Ok(()),
            }
        }
        prop_assert!(false, "cursor produced more fields than input bytes");
    }

    #[test]
    fn par_index_is_total_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        let _ = CanonicalParProcessIndex::parse(&bytes);
    }

    #[test]
    fn owned_and_borrowed_field_cursors_are_exactly_differential(
        bytes in prop::collection::vec(any::<u8>(), 0..4096)
    ) {
        let mut borrowed = CanonicalFieldCursor::new(&bytes);
        let mut owned = CanonicalRegionFieldCursor::new(ProtobufRegion::from(bytes.clone()));
        for _ in 0..=bytes.len() {
            match (borrowed.next_field(), owned.next_field()) {
                (Ok(Some(left)), Ok(Some(right))) => assert_field_equivalent(left, right),
                (Ok(None), Ok(None)) => return Ok(()),
                (Err(left), Err(right)) => {
                    prop_assert_eq!(left, right);
                    return Ok(());
                }
                (left, right) => {
                    prop_assert!(false, "borrowed/owned cursor mismatch: {left:?} versus {right:?}");
                }
            }
        }
        prop_assert!(false, "owned/borrowed cursors exceeded the input-byte progress bound");
    }

    #[test]
    fn owned_par_index_is_total_on_arbitrary_bytes(
        bytes in prop::collection::vec(any::<u8>(), 0..4096)
    ) {
        let _ = CanonicalParRegionIndex::parse(ProtobufRegion::from(bytes));
    }
}
