//! Totality, canonicality, and restartability gates for the borrowed protobuf
//! cursor used by byte-native process analyses.

use models::rust::rholang::protobuf_cursor::{
    CanonicalDelimitedRun, CanonicalFieldCursor, CanonicalFieldValue, CanonicalParProcessIndex,
    CanonicalProtobufError, CanonicalWireType,
};
use proptest::prelude::*;

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
}
