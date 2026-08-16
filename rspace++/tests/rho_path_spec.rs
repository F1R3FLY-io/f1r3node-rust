use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::rho_path::{
    RhoPathCaptureId, RhoPathError, RhoPathQuery, RhoPathQueryAtom, RhoPathV1,
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
