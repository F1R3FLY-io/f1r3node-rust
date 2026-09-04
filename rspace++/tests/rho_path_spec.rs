use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::rho_path::{
    RHO_MATCH_FIELD_END, RHO_MATCH_FIELD_START, RHO_MATCH_I64, RHO_MATCH_NODE_END,
    RHO_MATCH_NODE_START, RHO_MATCH_PATH_VERSION, RhoMatchQuery, RhoMatchQueryPart,
    RhoPathCaptureId, RhoPathError, RhoPathQuery, RhoPathQueryAtom, RhoPathV1, RhoPathV2,
    encode_structural_sequence,
};

#[test]
fn datum_paths_round_trip_all_byte_values() {
    let channel = (0u8..=u8::MAX).collect::<Vec<_>>();
    let structure = (0u8..=u8::MAX).rev().collect::<Vec<_>>();
    let identity = vec![0, 1, 0, 255, 0];
    let path = RhoPathV1::datum_key(&channel, &structure, &identity);
    let view = RhoPathV1::parse_datum(&path).unwrap();
    assert_eq!(view.channel_structure(), channel);
    assert_eq!(view.channel_identity(), channel.stable_hash_bytes());
    assert_eq!(view.datum_structure(), structure);
    assert_eq!(view.datum_identity(), identity);
}

#[test]
fn v2_datum_uses_three_framed_canonical_fields() {
    let path = RhoPathV2::datum_key(b"channel\0image", b"shape", b"identity\0tail");
    let view = RhoPathV2::parse_datum(&path).unwrap();
    assert_eq!(view.channel_image(), b"channel\0image");
    assert_eq!(view.datum_structure(), b"shape");
    assert_eq!(view.datum_identity(), b"identity\0tail");
    assert!(view.encoded_channel_image().ends_with(&[0, 0]));
    assert!(view.encoded_datum_structure().ends_with(&[0, 0]));
    assert!(view.encoded_datum_identity().ends_with(&[0, 0]));
    let mut trailing = path.clone();
    trailing.push(9);
    assert_eq!(RhoPathV2::parse_datum(&trailing), Err(RhoPathError::TrailingBytes));
}

#[test]
fn v2_datum_round_trips_all_byte_values_in_each_field() {
    let fields = (0u8..=u8::MAX).collect::<Vec<_>>();
    let path = RhoPathV2::datum_key(&fields, &fields, &fields);
    let view = RhoPathV2::parse_datum(&path).unwrap();
    assert_eq!(view.channel_image(), fields);
    assert_eq!(view.datum_structure(), fields);
    assert_eq!(view.datum_identity(), fields);
}

#[test]
fn v1_and_v2_datum_namespaces_are_disjoint() {
    let channel = 7_u64;
    let v1 = RhoPathV1::datum_key(&channel, b"shape", b"identity");
    let v2 = RhoPathV2::datum_key(b"channel-image", b"shape", b"identity");
    assert!(RhoPathV1::parse_datum(&v1).is_ok());
    assert!(RhoPathV2::parse_datum(&v2).is_ok());
    assert_eq!(RhoPathV1::parse_datum(&v2), Err(RhoPathError::InvalidMagic));
    assert_eq!(RhoPathV2::parse_datum(&v1), Err(RhoPathError::InvalidMagic));
}

#[test]
fn rho_path_errors_are_not_mislabeled_by_wire_version() {
    assert_eq!(RhoPathError::InvalidMagic.to_string(), "invalid RhoPath magic");
    assert_eq!(RhoPathError::TrailingBytes.to_string(), "unexpected bytes after RhoPath prefix");
}

#[test]
fn v2_continuation_has_u64_count_and_rejects_empty_groups() {
    assert_eq!(RhoPathV2::continuation_key(&[], b"id"), Err(RhoPathError::EmptyChannelGroup));
    let path = RhoPathV2::continuation_key(&[b"a", b"b"], b"id").unwrap();
    assert_eq!(&path[6..14], &2_u64.to_be_bytes());
    let view = RhoPathV2::parse_continuation(&path).unwrap();
    assert_eq!(view.channel_count(), 2);
    assert!(view.encoded_identity_frame().ends_with(&[0, 0]));
    assert_eq!(view.channel_frames().count(), 2);
}

#[test]
fn v2_continuation_preserves_order_and_zero_escaping() {
    let channels = [vec![0, 1, 0, 255], vec![255, 0], vec![]];
    let refs = channels.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let path = RhoPathV2::continuation_key(&refs, &[0, 255, 0]).unwrap();
    let view = RhoPathV2::parse_continuation(&path).unwrap();
    assert_eq!(view.channel_count(), channels.len());
    for (frame, expected) in view.channel_frames().zip(channels) {
        assert!(frame.ends_with(&[0, 0]));
        assert!(frame.len() >= expected.len() + 2);
    }
    assert!(view.encoded_identity_frame().windows(2).any(|pair| pair == [0, 255]));
}

#[test]
fn v2_continuation_rejects_count_mismatch_without_fallback() {
    let valid = RhoPathV2::continuation_key(&[b"a", b"b"], b"id").unwrap();
    let mut overdeclared = valid.clone();
    overdeclared[13] = 3;
    assert!(RhoPathV2::parse_continuation(&overdeclared).is_err());
    let mut underdeclared = valid;
    underdeclared[13] = 1;
    assert!(RhoPathV2::parse_continuation(&underdeclared).is_err());
}

#[test]
fn v2_headers_and_escapes_fail_closed() {
    let valid = RhoPathV2::continuation_key(&[b"channel"], b"id").unwrap();
    let mut wrong_version = valid.clone();
    wrong_version[5] = 9;
    assert_eq!(
        RhoPathV2::parse_continuation(&wrong_version),
        Err(RhoPathError::InvalidCollectionVersion(9))
    );
    let mut wrong_kind = valid.clone();
    wrong_kind[4] = RhoPathV2::JOIN_KIND;
    assert_eq!(
        RhoPathV2::parse_continuation(&wrong_kind),
        Err(RhoPathError::InvalidKind(RhoPathV2::JOIN_KIND))
    );
    let mut malformed = valid;
    malformed[14] = 0;
    malformed[15] = 2;
    assert_eq!(RhoPathV2::parse_continuation(&malformed), Err(RhoPathError::InvalidEscape(2)));
}

#[test]
fn v2_megabyte_fields_parse_on_a_tiny_native_stack() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let channel = vec![0; 1_000_000];
            let structure = vec![0xA5; 1_000_000];
            let identity = vec![0x5A; 1_000_000];
            let path = RhoPathV2::datum_key(&channel, &structure, &identity);
            let view = RhoPathV2::parse_datum(&path).unwrap();
            assert_eq!(view.channel_image(), channel);
            assert_eq!(view.datum_structure(), structure);
            assert_eq!(view.datum_identity(), identity);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn v2_join_frames_identity() {
    let path = RhoPathV2::join_key(b"channel", b"join\0identity");
    assert!(path.windows(2).any(|pair| pair == [0, 1]));
    assert!(path.windows(2).any(|pair| pair == [0, 255]));
    assert!(path.ends_with(&[0, 0]));
    let (channel, identity) = RhoPathV2::parse_join(&path).unwrap();
    assert!(channel.ends_with(&[0, 0]));
    assert!(identity.ends_with(&[0, 0]));
}

#[test]
fn channel_and_structural_prefixes_are_real_trie_prefixes() {
    let channel = b"channel\0with-zero".to_vec();
    let first = RhoPathV1::datum_key(&channel, b"shared\0left", b"first");
    let second = RhoPathV1::datum_key(&channel, b"shared\0right", b"second");
    let channel_prefix = RhoPathV1::datum_prefix(&channel);
    let structure_prefix = RhoPathV1::datum_structure_prefix(&channel, b"shared\0");
    let relative_structure_prefix = RhoPathV1::relative_datum_structure_prefix(b"shared\0");
    assert!(first.starts_with(&channel_prefix));
    assert!(second.starts_with(&channel_prefix));
    assert!(first.starts_with(&structure_prefix));
    assert!(second.starts_with(&structure_prefix));
    assert_eq!(&structure_prefix[channel_prefix.len()..], relative_structure_prefix.as_slice());
}

#[test]
fn relative_datum_keys_share_the_complete_key_framing_contract() {
    let channel = b"channel\0with-zero".to_vec();
    let structure = b"structure\0with-zero";
    let identity = b"identity\0suffix";
    let complete = RhoPathV1::datum_key(&channel, structure, identity);
    let channel_prefix = RhoPathV1::datum_prefix(&channel);
    let relative = RhoPathV1::relative_datum_key(structure, identity);
    assert_eq!(&complete[channel_prefix.len()..], relative.as_slice());
    assert!(relative.starts_with(&RhoPathV1::relative_datum_structure_prefix(structure)));
}

#[test]
fn structural_sequences_are_injective() {
    let one = encode_structural_sequence([b"a".as_slice(), b"bc".as_slice()]);
    let other = encode_structural_sequence([b"ab".as_slice(), b"c".as_slice()]);
    let empty = encode_structural_sequence(std::iter::empty());
    assert_ne!(one, other);
    assert_ne!(one, empty);
}

#[test]
fn structural_queries_match_exact_wildcard_coreference_and_remainder_cases() {
    let capture = RhoPathCaptureId::new(7);
    let coreferential = RhoPathQuery::structural_sequence(
        [
            RhoPathQueryAtom::Capture(capture),
            RhoPathQueryAtom::Exact(vec![0, 1, 255]),
            RhoPathQueryAtom::Capture(capture),
        ],
        false,
    );
    let matching =
        encode_structural_sequence([[0, 9].as_slice(), [0, 1, 255].as_slice(), [0, 9].as_slice()]);
    let unequal_capture =
        encode_structural_sequence([[0, 9].as_slice(), [0, 1, 255].as_slice(), [0, 8].as_slice()]);
    let wrong_exact =
        encode_structural_sequence([[0, 9].as_slice(), [0, 1, 254].as_slice(), [0, 9].as_slice()]);
    assert!(coreferential.matches_structure(&matching));
    assert!(!coreferential.matches_structure(&unequal_capture));
    assert!(!coreferential.matches_structure(&wrong_exact));

    let remainder = RhoPathQuery::structural_sequence(
        [RhoPathQueryAtom::Wildcard, RhoPathQueryAtom::Exact(vec![2])],
        true,
    );
    assert!(remainder.matches_structure(&encode_structural_sequence([
        [1].as_slice(),
        [2].as_slice(),
        [3].as_slice(),
        [4].as_slice(),
    ])));
    assert!(!remainder.matches_structure(&encode_structural_sequence([
        [1].as_slice(),
        [3].as_slice(),
        [2].as_slice(),
    ])));
}

#[test]
fn structural_queries_reject_malformed_framing() {
    let query = RhoPathQuery::structural_sequence([RhoPathQueryAtom::Wildcard], true);
    assert!(!query.matches_structure(&[]));
    assert!(!query.matches_structure(&[1, 0]));
    assert!(!query.matches_structure(&[1, 0, 2]));
}

#[test]
fn nested_match_queries_skip_values_and_enforce_coreference_without_decoding() {
    let capture = RhoPathCaptureId::new(3);
    let mut prefix = RHO_MATCH_PATH_VERSION.to_vec();
    prefix.extend_from_slice(&[RHO_MATCH_NODE_START, 0, 1, RHO_MATCH_FIELD_START, 0, 0, 0, 1]);
    let between = vec![RHO_MATCH_FIELD_END, RHO_MATCH_FIELD_START, 0, 0, 0, 2];
    let suffix = vec![RHO_MATCH_FIELD_END, RHO_MATCH_NODE_END];
    let query = RhoMatchQuery::new([
        RhoMatchQueryPart::Exact(prefix.clone()),
        RhoMatchQueryPart::Capture(capture),
        RhoMatchQueryPart::Exact(between.clone()),
        RhoMatchQueryPart::Capture(capture),
        RhoMatchQueryPart::Exact(suffix.clone()),
    ]);
    let query = RhoPathQuery::structural_sequence([RhoPathQueryAtom::Match(query)], false);

    let scalar = |value: i64| {
        let mut bytes = vec![RHO_MATCH_I64];
        bytes.extend_from_slice(&value.to_be_bytes());
        bytes
    };
    let value = |left: i64, right: i64| {
        let mut bytes = prefix.clone();
        bytes.extend_from_slice(&scalar(left));
        bytes.extend_from_slice(&between);
        bytes.extend_from_slice(&scalar(right));
        bytes.extend_from_slice(&suffix);
        encode_structural_sequence([bytes.as_slice()])
    };
    assert!(query.matches_structure(&value(0, 0)));
    assert!(query.matches_structure(&value(17, 17)));
    assert!(!query.matches_structure(&value(17, 18)));
}

#[test]
fn nested_value_skip_is_stack_safe_and_rejects_every_small_truncation() {
    let query = RhoPathQuery::structural_sequence(
        [RhoPathQueryAtom::Match(RhoMatchQuery::new([
            RhoMatchQueryPart::Exact(RHO_MATCH_PATH_VERSION.to_vec()),
            RhoMatchQueryPart::Wildcard,
        ]))],
        false,
    );
    let shallow = {
        let mut value = RHO_MATCH_PATH_VERSION.to_vec();
        value.extend_from_slice(&[
            RHO_MATCH_NODE_START,
            0,
            1,
            RHO_MATCH_FIELD_START,
            0,
            0,
            0,
            1,
            RHO_MATCH_I64,
        ]);
        value.extend_from_slice(&7i64.to_be_bytes());
        value.extend_from_slice(&[RHO_MATCH_FIELD_END, RHO_MATCH_NODE_END]);
        value
    };
    for end in RHO_MATCH_PATH_VERSION.len()..shallow.len() {
        assert!(!query.matches_structure(&encode_structural_sequence([&shallow[..end]])));
    }

    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || {
            let mut value = RHO_MATCH_PATH_VERSION.to_vec();
            for _ in 0..20_000 {
                value.extend_from_slice(&[
                    RHO_MATCH_NODE_START,
                    0,
                    1,
                    RHO_MATCH_FIELD_START,
                    0,
                    0,
                    0,
                    1,
                ]);
            }
            value.push(RHO_MATCH_I64);
            value.extend_from_slice(&7i64.to_be_bytes());
            for _ in 0..20_000 {
                value.extend_from_slice(&[RHO_MATCH_FIELD_END, RHO_MATCH_NODE_END]);
            }
            assert!(query.matches_structure(&encode_structural_sequence([value.as_slice()])));
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn malformed_frames_are_rejected_without_partial_acceptance() {
    let channel = b"channel".to_vec();
    let path = RhoPathV1::datum_key(&channel, b"structure", b"identity");
    for cut in 0..RhoPathV1::datum_prefix(&channel).len() {
        assert!(RhoPathV1::parse_datum(&path[..cut]).is_err());
    }
    let mut malformed = path;
    let prefix_len = RhoPathV1::datum_prefix(&channel).len();
    malformed[prefix_len] = 0;
    malformed[prefix_len + 1] = 2;
    assert_eq!(RhoPathV1::parse_datum(&malformed), Err(RhoPathError::InvalidEscape(2)));
}

#[test]
fn million_byte_fields_parse_on_a_small_native_stack() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            let channel = vec![0; 1_000_000];
            let structure = vec![0xA5; 1_000_000];
            let identity = vec![0x5A; 1_000_000];
            let path = RhoPathV1::datum_key(&channel, &structure, &identity);
            let view = RhoPathV1::parse_datum(&path).unwrap();
            assert_eq!(view.channel_structure(), channel);
            assert_eq!(view.datum_structure(), structure);
            assert_eq!(view.datum_identity(), identity);
        })
        .unwrap()
        .join()
        .unwrap();
}
