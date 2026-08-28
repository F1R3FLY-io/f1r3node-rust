use proptest::prelude::*;

use crate::rspace::rho_path::{
    RhoPathError, RhoPathNamespaceV1, RhoPathStreamValidatorV1, RhoPathV1,
};

fn oracle(namespace: RhoPathNamespaceV1, bytes: &[u8]) -> Result<(), RhoPathError> {
    match namespace {
        RhoPathNamespaceV1::Datum => RhoPathV1::parse_datum(bytes).map(|_| ()),
        RhoPathNamespaceV1::Continuation => RhoPathV1::parse_continuation(bytes).map(|_| ()),
        RhoPathNamespaceV1::Join => RhoPathV1::parse_join(bytes).map(|_| ()),
    }
}

fn streamed(
    namespace: RhoPathNamespaceV1,
    bytes: &[u8],
    widths: impl IntoIterator<Item = usize>,
) -> Result<(), RhoPathError> {
    let mut validator = RhoPathStreamValidatorV1::new(namespace);
    let mut offset = 0usize;
    for width in widths {
        let end = offset.saturating_add(width).min(bytes.len());
        validator.push(&bytes[offset..end])?;
        offset = end;
    }
    if offset < bytes.len() {
        validator.push(&bytes[offset..])?;
    }
    validator.finish()
}

fn assert_partition_refinement(namespace: RhoPathNamespaceV1, bytes: &[u8]) {
    let expected = oracle(namespace, bytes);
    assert_eq!(streamed(namespace, bytes, [bytes.len()]), expected);
    assert_eq!(streamed(namespace, bytes, std::iter::repeat_n(1, bytes.len())), expected,);
    assert_eq!(streamed(namespace, bytes, [0, bytes.len(), 0]), expected);
    for first in 0..=bytes.len() {
        assert_eq!(streamed(namespace, bytes, [first, bytes.len() - first]), expected,);
    }
}

fn namespaces() -> [RhoPathNamespaceV1; 3] {
    [RhoPathNamespaceV1::Datum, RhoPathNamespaceV1::Continuation, RhoPathNamespaceV1::Join]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn arbitrary_rho_path_bytes_match_the_contiguous_oracle_under_chunk_schedules(
        bytes in proptest::collection::vec(any::<u8>(), 0..256),
        widths in proptest::collection::vec(0usize..96, 0..96),
    ) {
        for namespace in namespaces() {
            prop_assert_eq!(streamed(namespace, &bytes, widths.iter().copied()), oracle(namespace, &bytes));
            prop_assert_eq!(streamed(namespace, &bytes, std::iter::repeat_n(1, bytes.len())), oracle(namespace, &bytes));
        }
    }

    #[test]
    fn rho_path_streaming_matches_contiguous_oracle_for_all_namespaces_partitions_and_prefixes(
        channel in proptest::collection::vec(any::<u8>(), 0..16),
        structure in proptest::collection::vec(any::<u8>(), 0..16),
        identity in proptest::collection::vec(any::<u8>(), 0..16),
        other_channel in proptest::collection::vec(any::<u8>(), 0..16),
    ) {
        let datum = RhoPathV1::datum_key(&channel, &structure, &identity);
        let continuation = RhoPathV1::collection_record_path(
            &RhoPathV1::continuation_prefix(&[channel.clone(), other_channel]),
            &identity,
        );
        let mut join = RhoPathV1::join_prefix(&channel);
        join.extend_from_slice(&identity);
        if identity.is_empty() {
            join.push(0);
        }
        for (namespace, bytes) in [
            (RhoPathNamespaceV1::Datum, datum),
            (RhoPathNamespaceV1::Continuation, continuation),
            (RhoPathNamespaceV1::Join, join),
        ] {
            prop_assert_eq!(oracle(namespace, &bytes), Ok(()));
            assert_partition_refinement(namespace, &bytes);
            for prefix_len in 0..bytes.len() {
                let prefix = &bytes[..prefix_len];
                let expected = oracle(namespace, prefix);
                prop_assert_eq!(
                    streamed(namespace, prefix, [0, prefix_len, 0]),
                    expected.clone(),
                );
                prop_assert_eq!(
                    streamed(namespace, prefix, std::iter::repeat_n(1, prefix_len)),
                    expected,
                );
            }
        }
    }
}

#[test]
fn rho_path_frame_group_and_shape_truth_tables_match_the_contiguous_parsers() {
    let datum_empty_suffix = RhoPathV1::datum_key(&vec![0_u8], &[0_u8], &[]);
    let continuation_empty_identity =
        RhoPathV1::collection_record_path(&RhoPathV1::continuation_prefix(&[vec![0_u8]]), &[]);
    let join_empty_suffix = RhoPathV1::join_prefix(&vec![0_u8]);
    let continuation_trailing = {
        let mut bytes = continuation_empty_identity.clone();
        bytes.push(7);
        bytes
    };
    let cases = vec![
        (RhoPathNamespaceV1::Datum, datum_empty_suffix, Ok(())),
        (RhoPathNamespaceV1::Continuation, continuation_empty_identity, Ok(())),
        (RhoPathNamespaceV1::Datum, b"RHP\x01\x01\0".to_vec(), Err(RhoPathError::TruncatedFrame)),
        (
            RhoPathNamespaceV1::Datum,
            b"RHP\x01\x01\0\x01".to_vec(),
            Err(RhoPathError::InvalidEscape(1)),
        ),
        (
            RhoPathNamespaceV1::Continuation,
            b"RHP\x01\x02\x01\0\x01\0\0".to_vec(),
            Err(RhoPathError::EmptyChannelGroup),
        ),
        (
            RhoPathNamespaceV1::Continuation,
            b"RHP\x01\x02\x01a\0\x01\0\0".to_vec(),
            Err(RhoPathError::InvalidEscape(1)),
        ),
        (RhoPathNamespaceV1::Join, join_empty_suffix, Err(RhoPathError::TruncatedFrame)),
        (RhoPathNamespaceV1::Continuation, continuation_trailing, Err(RhoPathError::TrailingBytes)),
    ];
    for (namespace, bytes, expected) in cases {
        assert_eq!(oracle(namespace, &bytes), expected);
        for width in 1..=bytes.len().max(1) {
            assert_eq!(
                streamed(namespace, &bytes, std::iter::repeat_n(width, bytes.len() + 1)),
                expected,
            );
        }
    }
}

#[test]
fn rho_path_namespace_kind_matrix_and_sticky_first_error_are_exact() {
    let valid = [
        (RhoPathNamespaceV1::Datum, RhoPathV1::datum_key(&7_u64, b"shape", b"identity")),
        (
            RhoPathNamespaceV1::Continuation,
            RhoPathV1::collection_record_path(
                &RhoPathV1::continuation_prefix(&[7_u64, 8]),
                b"identity",
            ),
        ),
        (RhoPathNamespaceV1::Join, {
            let mut bytes = RhoPathV1::join_prefix(&7_u64);
            bytes.extend_from_slice(b"identity");
            bytes
        }),
    ];
    for (actual_namespace, bytes) in &valid {
        for expected_namespace in namespaces() {
            let expected = oracle(expected_namespace, bytes);
            assert_eq!(streamed(expected_namespace, bytes, [0, 4, 1, 0, usize::MAX]), expected,);
            assert_eq!(expected.is_ok(), expected_namespace == *actual_namespace);
        }
    }

    for namespace in namespaces() {
        let mut validator = RhoPathStreamValidatorV1::new(namespace);
        let first = validator.push(b"X").unwrap_err();
        assert_eq!(first, RhoPathError::InvalidMagic);
        assert_eq!(validator.push(b"RHP\x01\x01"), Err(first));
        assert_eq!(validator.push(&[]), Err(first));
        assert_eq!(validator.finish(), Err(first));
    }
}

#[test]
fn rho_path_stream_validator_has_constant_retained_state() {
    assert!(std::mem::size_of::<RhoPathStreamValidatorV1>() <= 32);
    let bytes = RhoPathV1::datum_key(&7_u64, &[0; 65_537], &[1; 65_537]);
    assert_eq!(
        streamed(RhoPathNamespaceV1::Datum, &bytes, std::iter::repeat_n(1, bytes.len()),),
        Ok(()),
    );
}
