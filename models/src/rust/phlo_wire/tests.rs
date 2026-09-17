use proptest::prelude::*;

use super::*;

fn limits() -> PhloWireLimits {
    PhloWireLimits {
        total_bytes: 65_536,
        field_bytes: 4096,
    }
}

#[test]
fn wire_golden_vector_fixes_width_order_and_byte_length_prefix() {
    let mut writer = PhloWireEncoder::new(limits());
    writer.u16(0x0102).unwrap();
    writer.u32(0x03040506).unwrap();
    writer.u64(0x0708090a0b0c0d0e).unwrap();
    writer.u128(0x0f101112131415161718191a1b1c1d1e).unwrap();
    writer.bytes(&[0, 255]).unwrap();
    let mut expected: Vec<_> = (1..=30).collect();
    expected.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 2, 0, 255]);
    assert_eq!(writer.as_bytes(), &expected);
    let mut reader = PhloWireDecoder::new(&expected, limits()).unwrap();
    assert_eq!(reader.u16(), Ok(0x0102));
    assert_eq!(reader.u32(), Ok(0x03040506));
    assert_eq!(reader.u64(), Ok(0x0708090a0b0c0d0e));
    assert_eq!(reader.u128(), Ok(0x0f101112131415161718191a1b1c1d1e));
    assert_eq!(reader.bytes(), Ok([0, 255].as_slice()));
    assert_eq!(reader.finish(), Ok(()));
}

#[test]
fn every_integer_width_preserves_zero_and_maximum_values() {
    let mut writer = PhloWireEncoder::new(limits());
    for value in [0, u8::MAX] {
        writer.u8(value).unwrap();
    }
    for value in [0, u16::MAX] {
        writer.u16(value).unwrap();
    }
    for value in [0, u32::MAX] {
        writer.u32(value).unwrap();
    }
    for value in [0, u64::MAX] {
        writer.u64(value).unwrap();
    }
    for value in [0, u128::MAX] {
        writer.u128(value).unwrap();
    }
    let bytes = writer.into_bytes();
    let mut reader = PhloWireDecoder::new(&bytes, limits()).unwrap();
    for value in [0, u8::MAX] {
        assert_eq!(reader.u8(), Ok(value));
    }
    for value in [0, u16::MAX] {
        assert_eq!(reader.u16(), Ok(value));
    }
    for value in [0, u32::MAX] {
        assert_eq!(reader.u32(), Ok(value));
    }
    for value in [0, u64::MAX] {
        assert_eq!(reader.u64(), Ok(value));
    }
    for value in [0, u128::MAX] {
        assert_eq!(reader.u128(), Ok(value));
    }
    assert_eq!(reader.finish(), Ok(()));
}

#[test]
fn failed_appends_preserve_the_complete_existing_buffer() {
    let bound = PhloWireLimits {
        total_bytes: 11,
        field_bytes: 2,
    };
    let mut writer = PhloWireEncoder::new(bound);
    writer.u8(7).unwrap();
    let before = writer.clone();
    assert_eq!(writer.bytes(&[1, 2, 3]), Err(PhloWireError::LimitExceeded));
    assert_eq!(writer, before);
    assert_eq!(writer.u128(0), Err(PhloWireError::LimitExceeded));
    assert_eq!(writer, before);
    writer.bytes(&[1, 2]).unwrap();
    let full = writer.clone();
    assert_eq!(writer.u8(0), Err(PhloWireError::LimitExceeded));
    assert_eq!(writer.bytes(&[]), Err(PhloWireError::LimitExceeded));
    assert_eq!(writer, full);
    assert_eq!(writer.as_bytes().len(), bound.total_bytes);
}

#[test]
fn empty_fields_have_headers_and_trailing_bytes_are_not_ignored() {
    let bound = PhloWireLimits {
        total_bytes: 8,
        field_bytes: 0,
    };
    let mut writer = PhloWireEncoder::new(bound);
    writer.bytes(&[]).unwrap();
    assert_eq!(writer.as_bytes(), &[0; 8]);
    let mut reader = PhloWireDecoder::new(writer.as_bytes(), bound).unwrap();
    assert_eq!(reader.clone().finish(), Err(PhloWireError::TrailingBytes));
    assert_eq!(reader.bytes(), Ok([].as_slice()));
    assert_eq!(reader.finish(), Ok(()));
    assert_eq!(
        PhloWireDecoder::new(&[0; 9], bound),
        Err(PhloWireError::LimitExceeded)
    );
}

#[test]
fn malformed_field_reads_never_advance_the_cursor() {
    let mut writer = PhloWireEncoder::new(limits());
    writer.u16(9).unwrap();
    writer.bytes(&[1, 2, 3]).unwrap();
    let complete = writer.into_bytes();
    for length in 2..complete.len() {
        let mut reader = PhloWireDecoder::new(&complete[..length], limits()).unwrap();
        assert_eq!(reader.u16(), Ok(9));
        let before = reader.clone();
        assert_eq!(reader.bytes(), Err(PhloWireError::Truncated));
        assert_eq!(reader, before);
    }
    let mut oversized = Vec::from([0, 9]);
    oversized.extend_from_slice(&u64::MAX.to_be_bytes());
    let mut reader = PhloWireDecoder::new(&oversized, limits()).unwrap();
    assert_eq!(reader.u16(), Ok(9));
    let before = reader.clone();
    assert_eq!(reader.bytes(), Err(PhloWireError::LimitExceeded));
    assert_eq!(reader, before);
}

#[test]
fn truncated_integer_reads_are_atomic_at_every_width() {
    for length in 0..16 {
        let input = vec![0; length];
        let mut reader = PhloWireDecoder::new(&input, limits()).unwrap();
        let before = reader.clone();
        assert_eq!(reader.u128(), Err(PhloWireError::Truncated));
        assert_eq!(reader, before);
        if length < 8 {
            assert_eq!(reader.u64(), Err(PhloWireError::Truncated));
        }
        if length < 4 {
            assert_eq!(reader.u32(), Err(PhloWireError::Truncated));
        }
        if length < 2 {
            assert_eq!(reader.u16(), Err(PhloWireError::Truncated));
        }
        if length == 0 {
            assert_eq!(reader.u8(), Err(PhloWireError::Truncated));
        }
        assert_eq!(reader, before);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn full_width_words_and_variable_fields_roundtrip(
        a in any::<u8>(), b in any::<u16>(), c in any::<u32>(), d in any::<u64>(), e in any::<u128>(),
        fields in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..128), 0..32),
    ) {
        let mut writer = PhloWireEncoder::new(limits());
        writer.u8(a).unwrap(); writer.u16(b).unwrap(); writer.u32(c).unwrap();
        writer.u64(d).unwrap(); writer.u128(e).unwrap();
        for field in &fields { writer.bytes(field).unwrap(); }
        let bytes = writer.into_bytes();
        let mut reader = PhloWireDecoder::new(&bytes, limits()).unwrap();
        prop_assert_eq!(reader.u8(), Ok(a)); prop_assert_eq!(reader.u16(), Ok(b));
        prop_assert_eq!(reader.u32(), Ok(c)); prop_assert_eq!(reader.u64(), Ok(d));
        prop_assert_eq!(reader.u128(), Ok(e));
        for field in &fields { prop_assert_eq!(reader.bytes(), Ok(field.as_slice())); }
        prop_assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn decoded_fields_reencode_the_exact_consumed_prefix(
        payload in prop::collection::vec(any::<u8>(), 0..128),
        suffix in prop::collection::vec(any::<u8>(), 0..128),
        count in 0u64..160,
    ) {
        let mut input = count.to_be_bytes().to_vec();
        input.extend_from_slice(&payload); input.extend_from_slice(&suffix);
        let mut reader = PhloWireDecoder::new(&input, limits()).unwrap();
        let before = reader.clone();
        match reader.bytes() {
            Ok(field) => {
                let mut writer = PhloWireEncoder::new(limits());
                writer.bytes(field).unwrap();
                prop_assert_eq!(writer.as_bytes(), &input[..reader.position()]);
                prop_assert_eq!(reader.position(), 8 + field.len());
                prop_assert_eq!(reader.remaining(), &input[reader.position()..]);
                prop_assert_eq!(field.len() as u64, count);
            }
            Err(error) => {
                prop_assert_eq!(error, PhloWireError::Truncated);
                prop_assert!(count as usize > payload.len() + suffix.len());
                prop_assert_eq!(reader, before);
            }
        }
    }

    #[test]
    fn bounded_append_matches_the_formal_acceptance_condition(
        prefix in prop::collection::vec(any::<u8>(), 0..32),
        payload in prop::collection::vec(any::<u8>(), 0..128),
        extra_capacity in 0usize..160, field_limit in 0usize..160,
    ) {
        let bounds = PhloWireLimits { total_bytes: prefix.len() + extra_capacity, field_bytes: field_limit };
        let mut writer = PhloWireEncoder::new(bounds);
        for byte in &prefix { writer.u8(*byte).unwrap(); }
        let before = writer.clone();
        let result = writer.bytes(&payload);
        let expected = payload.len() <= field_limit && 8 + payload.len() <= extra_capacity;
        prop_assert_eq!(result.is_ok(), expected);
        if expected {
            prop_assert_eq!(&writer.as_bytes()[..prefix.len()], prefix.as_slice());
            prop_assert_eq!(writer.as_bytes().len(), prefix.len() + 8 + payload.len());
            prop_assert!(writer.as_bytes().len() <= bounds.total_bytes);
        } else {
            prop_assert_eq!(result, Err(PhloWireError::LimitExceeded));
            prop_assert_eq!(writer, before);
        }
    }

    #[test]
    fn field_partition_changes_cannot_alias(
        shared in prop::collection::vec(any::<u8>(), 1..128),
        first_seed in any::<usize>(), second_seed in any::<usize>(),
    ) {
        let first = first_seed % (shared.len() + 1);
        let second = second_seed % (shared.len() + 1);
        let encode = |split| {
            let mut writer = PhloWireEncoder::new(limits());
            writer.bytes(&shared[..split]).unwrap(); writer.bytes(&shared[split..]).unwrap();
            writer.into_bytes()
        };
        prop_assert_eq!(encode(first) == encode(second), first == second);
    }

    #[test]
    fn mixed_append_sequences_preserve_the_reference_buffer(
        operations in prop::collection::vec((any::<bool>(), any::<u64>(), prop::collection::vec(any::<u8>(), 0..24)), 0..80),
        total in 0usize..512, field_limit in 0usize..32,
    ) {
        let bounds = PhloWireLimits { total_bytes: total, field_bytes: field_limit };
        let mut writer = PhloWireEncoder::new(bounds);
        let mut reference = Vec::new();
        for (field, number, payload) in operations {
            let value = if field { payload.len() as u64 } else { number };
            let mut encoded: Vec<_> = (0..8).rev().map(|index| (value >> (8 * index)) as u8).collect();
            if field { encoded.extend_from_slice(&payload); }
            let accepted = (!field || payload.len() <= field_limit) && reference.len() + encoded.len() <= total;
            let result = if field { writer.bytes(&payload) } else { writer.u64(number) };
            if accepted { reference.extend_from_slice(&encoded); }
            prop_assert_eq!(result.is_ok(), accepted);
            prop_assert_eq!(writer.as_bytes(), reference.as_slice());
        }
    }

    #[test]
    fn mixed_read_sequences_preserve_position_and_exact_remaining_bytes(
        input in prop::collection::vec(any::<u8>(), 0..256),
        operations in prop::collection::vec(0u8..6, 0..80),
        field_limit in 0usize..128,
    ) {
        let bounds = PhloWireLimits { total_bytes: 256, field_bytes: field_limit };
        let mut reader = PhloWireDecoder::new(&input, bounds).unwrap();
        let mut position = 0usize;
        for operation in operations {
            let suffix = &input[position..];
            if operation == 5 {
                let expected = if suffix.len() < 8 {
                    Err(PhloWireError::Truncated)
                } else {
                    let count = suffix[..8].iter().fold(0u128, |value, byte| value * 256 + u128::from(*byte));
                    if count > field_limit as u128 { Err(PhloWireError::LimitExceeded) }
                    else if 8 + count as usize > suffix.len() { Err(PhloWireError::Truncated) }
                    else { Ok(&suffix[8..8 + count as usize]) }
                };
                prop_assert_eq!(reader.bytes(), expected);
                if let Ok(payload) = expected { position += 8 + payload.len(); }
            } else {
                let width = 1usize << operation;
                let expected = if suffix.len() < width { Err(PhloWireError::Truncated) }
                    else { Ok(suffix[..width].iter().fold(0u128, |value, byte| value * 256 + u128::from(*byte))) };
                let actual = match operation {
                    0 => reader.u8().map(u128::from),
                    1 => reader.u16().map(u128::from),
                    2 => reader.u32().map(u128::from),
                    3 => reader.u64().map(u128::from),
                    _ => reader.u128(),
                };
                prop_assert_eq!(actual, expected);
                if expected.is_ok() { position += width; }
            }
            prop_assert_eq!(reader.position(), position);
            prop_assert_eq!(reader.remaining(), &input[position..]);
        }
        prop_assert_eq!(reader.finish().is_ok(), position == input.len());
    }
}
