use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::channel_image::{
    ChannelIdentityV2, ChannelImageError, ChannelImageV2, ValidatedChannelImage,
    CHANNEL_IMAGE_V2_DOMAIN,
};
use models::rust::par_bag::PAR_BAG_KEY_DOMAIN;
use models::rust::rspace_path::{
    rho_path_v2_continuation_key, rho_path_v2_datum_key, rho_path_v2_join_key,
};
use proptest::prelude::*;
use rspace_plus_plus::rspace::bag::Multiplicity;

mod par_corpus;

fn gint(value: i64) -> Expr {
    Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }
}

fn image(par: &Par) -> ChannelImageV2 {
    ChannelImageV2::from_par(par).expect("a resident Par has a canonical RCI2 image")
}

#[test]
fn rspace_v2_adapters_use_rci2_bytes_without_projection() {
    let channel = Par::default();
    let datum = rho_path_v2_datum_key(&channel, b"rmp2", b"datum").unwrap();
    let datum_view = rspace_plus_plus::rspace::rho_path::RhoPathV2::parse_datum(&datum).unwrap();
    assert_eq!(datum_view.channel_image(), image(&channel).as_bytes());
    let continuation = rho_path_v2_continuation_key(&[channel.clone()], b"continuation").unwrap();
    let continuation_view =
        rspace_plus_plus::rspace::rho_path::RhoPathV2::parse_continuation(&continuation).unwrap();
    assert_eq!(continuation_view.channel_frames.len(), 1);
    let join = rho_path_v2_join_key(&channel, b"join").unwrap();
    assert!(rspace_plus_plus::rspace::rho_path::RhoPathV2::parse_join(&join).is_ok());
}

fn hash<T: Hash>(value: &T) -> u64 {
    let mut state = DefaultHasher::new();
    value.hash(&mut state);
    state.finish()
}

/// Locate the first endpoint terminator. Escaped zero is `00 FF`; `00 00`
/// ends the key. The returned index points at the first terminator byte.
fn first_frame_end(bytes: &[u8]) -> usize {
    let mut cursor = CHANNEL_IMAGE_V2_DOMAIN.len();
    loop {
        match (bytes[cursor], bytes.get(cursor + 1).copied()) {
            (0, Some(0xFF)) => cursor += 2,
            (0, Some(0)) => return cursor,
            (0, other) => panic!("invalid fixture escape at {cursor}: {other:?}"),
            _ => cursor += 1,
        }
    }
}

fn one_endpoint_layout(bytes: &[u8]) -> (usize, usize, usize) {
    let frame_end = first_frame_end(bytes);
    let endpoint_end = frame_end + 2 + 8;
    let count_offset = endpoint_end + 2;
    assert_eq!(&bytes[endpoint_end..endpoint_end + 2], &[0, 0]);
    assert_eq!(count_offset + 8 + 1, bytes.len());
    (frame_end, endpoint_end, count_offset)
}

fn single_endpoint_key(bytes: &[u8]) -> Vec<u8> {
    let frame_end = first_frame_end(bytes);
    let mut key = PAR_BAG_KEY_DOMAIN.to_vec();
    let mut cursor = CHANNEL_IMAGE_V2_DOMAIN.len();
    while cursor < frame_end {
        if bytes[cursor] == 0 {
            assert_eq!(bytes[cursor + 1], 0xFF);
            key.push(0);
            cursor += 2;
        } else {
            key.push(bytes[cursor]);
            cursor += 1;
        }
    }
    key
}

fn image_with_single_endpoint_key(original: &[u8], key: &[u8]) -> Vec<u8> {
    assert!(key.starts_with(&PAR_BAG_KEY_DOMAIN));
    let frame_end = first_frame_end(original);
    let mut image = CHANNEL_IMAGE_V2_DOMAIN.to_vec();
    for &byte in &key[PAR_BAG_KEY_DOMAIN.len()..] {
        image.push(byte);
        if byte == 0 {
            image.push(0xFF);
        }
    }
    image.extend_from_slice(&[0, 0]);
    image.extend_from_slice(&original[frame_end + 2..]);
    image
}

fn endpoint_records(bytes: &[u8]) -> Vec<std::ops::Range<usize>> {
    let mut records = Vec::new();
    let mut cursor = CHANNEL_IMAGE_V2_DOMAIN.len();
    while bytes.get(cursor..cursor + 2) != Some(&[0, 0]) {
        let start = cursor;
        loop {
            match (bytes[cursor], bytes.get(cursor + 1).copied()) {
                (0, Some(0xFF)) => cursor += 2,
                (0, Some(0)) => {
                    cursor += 2 + 8;
                    break;
                }
                (0, other) => panic!("invalid fixture escape at {cursor}: {other:?}"),
                _ => cursor += 1,
            }
        }
        records.push(start..cursor);
    }
    records
}

#[test]
fn empty_images_pin_the_domain_count_width_and_equality_metadata() {
    let empty_false = image(&Par::default());
    assert_eq!(empty_false.as_bytes(), &[
        b'R', b'C', b'I', 2, // domain and version
        0, 0, // endpoint-sequence terminator
        0, 0, 0, 0, 0, 0, 0, 0, // distinct endpoint count
        0, // connective_used
    ]);

    let empty_true = image(&Par::default().with_connective_used(true));
    assert_eq!(empty_true.as_bytes(), &[
        b'R', b'C', b'I', 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
    ]);
    assert!(empty_false < empty_true);
}

#[test]
fn image_is_exactly_a_function_of_par_equality() {
    for (label, par) in par_corpus::par_corpus() {
        let different_summary = par.with_locally_free(vec![0x00, 0xAA, 0xFF]);
        assert_eq!(
            par, different_summary,
            "root locally_free quotient: {label}"
        );

        let left = image(&par);
        let right = image(&different_summary);
        assert_eq!(left, right, "image locally_free quotient: {label}");
        assert_eq!(hash(&left), hash(&right), "image hash quotient: {label}");

        let different_connective = par.with_connective_used(!par.connective_used());
        assert_ne!(par, different_connective, "connective equality: {label}");
        assert_ne!(
            left,
            image(&different_connective),
            "connective image: {label}"
        );
    }
}

#[test]
fn paired_identity_keeps_structural_image_separate_from_legacy_digest() {
    let par = par_corpus::gint(7);
    let paired = ChannelIdentityV2::from_par(&par).expect("resident Par has both identities");
    let same_semantics = par.with_locally_free(vec![0xDE, 0xAD]);
    let paired_same = ChannelIdentityV2::from_par(&same_semantics)
        .expect("semantic equal Par remains admissible");

    assert_eq!(paired.image(), paired_same.image());
    assert_eq!(paired.legacy_digest(), paired_same.legacy_digest());
    assert!(paired.legacy_digest_matches(paired_same.legacy_digest()));

    let different_connective = par.with_connective_used(!par.connective_used());
    let paired_different = ChannelIdentityV2::from_par(&different_connective)
        .expect("connective-separated Par has both identities");
    assert_ne!(paired.image(), paired_different.image());
    // The legacy digest remains an independent compatibility value; callers
    // must not infer structural equality from a digest-routing match.
    assert_eq!(
        paired.legacy_digest(),
        &rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(&par)
    );
}

#[test]
fn raw_image_order_matches_par_order_over_the_complete_schema_corpus() {
    let mut corpus = par_corpus::par_corpus();

    let expr = gint(7);
    for count in [1, 2, 16, u32::MAX as u64, u64::MAX] {
        let mut par = Par::default();
        par.insert_component_many(
            &expr,
            Multiplicity::new(count).expect("nonzero fixture multiplicity"),
            Vec::new(),
            false,
        )
        .expect("one endpoint admits every fixture multiplicity");
        corpus.push((format!("multiplicity::{count}"), par));
    }

    for (left_label, left) in &corpus {
        for (right_label, right) in &corpus {
            assert_eq!(
                image(left).cmp(&image(right)),
                left.cmp(right),
                "raw image order diverged for {left_label} versus {right_label}"
            );
        }
    }
}

#[test]
fn decode_reconstructs_the_identity_normalized_par_without_entry_projection() {
    for (label, par) in par_corpus::par_corpus() {
        let encoded = image(&par);
        let (admitted, decoded) =
            ChannelImageV2::decode_bytes(encoded.as_bytes()).unwrap_or_else(|error| {
                panic!("failed to decode {label}: {error}");
            });
        assert_eq!(admitted, encoded, "admitted byte identity: {label}");
        assert_eq!(decoded, par, "semantic round trip: {label}");
        assert!(
            decoded.locally_free().is_empty(),
            "RCI2 intentionally normalizes root locally_free: {label}"
        );
        assert_eq!(image(&decoded), encoded, "byte-stable round trip: {label}");
    }
}

#[test]
fn multiplicity_is_one_fixed_width_counter_not_repeated_endpoints() {
    let expr = gint(99);
    let mut one = Par::default();
    one.insert_component_many(&expr, Multiplicity::ONE, Vec::new(), false)
        .unwrap();
    let mut sixteen = Par::default();
    sixteen
        .insert_component_many(&expr, Multiplicity::new(16).unwrap(), Vec::new(), false)
        .unwrap();

    let one_image = image(&one);
    let sixteen_image = image(&sixteen);
    assert_eq!(one_image.as_bytes().len(), sixteen_image.as_bytes().len());
    let (one_frame_end, _, _) = one_endpoint_layout(one_image.as_bytes());
    let (sixteen_frame_end, _, _) = one_endpoint_layout(sixteen_image.as_bytes());
    assert_eq!(one_frame_end, sixteen_frame_end);
    assert_eq!(
        &one_image.as_bytes()[one_frame_end + 2..one_frame_end + 10],
        &1u64.to_be_bytes()
    );
    assert_eq!(
        &sixteen_image.as_bytes()[sixteen_frame_end + 2..sixteen_frame_end + 10],
        &16u64.to_be_bytes()
    );
}

#[test]
fn construction_history_cow_and_compact_round_trip_do_not_change_the_image() {
    let values = [gint(-5), gint(0), gint(7), gint(i64::MAX)];
    let mut forward = Par::default();
    for value in &values {
        forward
            .insert_component(value, Vec::new(), false)
            .expect("forward insertion");
    }

    let mut reverse = Par::default();
    for value in values.iter().rev() {
        reverse
            .insert_component(value, Vec::new(), false)
            .expect("reverse insertion");
    }
    assert_eq!(forward, reverse);
    assert_eq!(image(&forward), image(&reverse));

    let shared = forward.clone();
    assert!(forward
        .components()
        .shares_storage_with(shared.components()));
    assert_eq!(image(&forward), image(&shared));

    let compact = forward.compact_snapshot().to_vec();
    let restored = models::rust::par_bag::ParBag::from_compact_bytes(&compact)
        .expect("canonical ACT snapshot reconstructs the same logical Bag");
    let restored = Par::from_parts(restored, vec![0xCC], forward.connective_used());
    assert_eq!(forward, restored);
    assert_eq!(image(&forward), image(&restored));
}

#[test]
fn borrowed_cursor_takes_exactly_one_image_from_a_stream() {
    let first = image(&par_corpus::all_par_fields());
    let second = image(&par_corpus::gint(-17).with_connective_used(true));
    let mut stream = Vec::new();
    stream.extend_from_slice(first.as_bytes());
    stream.extend_from_slice(second.as_bytes());

    let (first_view, consumed) = ValidatedChannelImage::take_prefix(&stream).unwrap();
    assert_eq!(first_view.as_bytes(), first.as_bytes());
    let (second_view, second_consumed) =
        ValidatedChannelImage::take_prefix(&stream[consumed..]).unwrap();
    assert_eq!(second_view.as_bytes(), second.as_bytes());
    assert_eq!(consumed + second_consumed, stream.len());
    assert!(matches!(
        ValidatedChannelImage::validate_exact(&stream),
        Err(ChannelImageError::TrailingBytes { .. })
    ));
}

#[test]
fn malformed_images_fail_closed_at_every_scalar_and_trie_boundary() {
    let valid = image(&par_corpus::gint(42));
    let bytes = valid.as_bytes();
    let (frame_end, endpoint_end, count_offset) = one_endpoint_layout(bytes);

    for cut in 0..bytes.len() {
        assert!(
            ValidatedChannelImage::validate_exact(&bytes[..cut]).is_err(),
            "truncation at {cut} unexpectedly validated"
        );
    }

    let mut wrong_domain = bytes.to_vec();
    wrong_domain[0] ^= 1;
    assert!(matches!(
        ChannelImageV2::try_from_bytes(&wrong_domain),
        Err(ChannelImageError::InvalidDomain { .. })
    ));

    let mut wrong_version = bytes.to_vec();
    wrong_version[3] = 3;
    assert!(matches!(
        ChannelImageV2::try_from_bytes(&wrong_version),
        Err(ChannelImageError::InvalidDomain { .. })
    ));

    let mut bad_family = bytes.to_vec();
    bad_family[CHANNEL_IMAGE_V2_DOMAIN.len()] = 0xFE;
    assert!(matches!(
        ChannelImageV2::try_from_bytes(&bad_family),
        Err(ChannelImageError::Component(_))
    ));

    let mut invalid_escape = bytes.to_vec();
    invalid_escape[frame_end + 1] = 0x7E;
    assert!(matches!(
        ChannelImageV2::try_from_bytes(&invalid_escape),
        Err(ChannelImageError::InvalidEscape(0x7E))
    ));

    let mut zero_multiplicity = bytes.to_vec();
    zero_multiplicity[frame_end + 2..frame_end + 10].fill(0);
    assert!(matches!(
        ChannelImageV2::try_from_bytes(&zero_multiplicity),
        Err(ChannelImageError::ZeroMultiplicity)
    ));

    let endpoint = &bytes[CHANNEL_IMAGE_V2_DOMAIN.len()..endpoint_end];
    let mut duplicate = Vec::new();
    duplicate.extend_from_slice(&bytes[..endpoint_end]);
    duplicate.extend_from_slice(endpoint);
    duplicate.extend_from_slice(&bytes[endpoint_end..]);
    let duplicate_count = count_offset + endpoint.len();
    duplicate[duplicate_count..duplicate_count + 8].copy_from_slice(&2u64.to_be_bytes());
    assert!(matches!(
        ChannelImageV2::try_from_bytes(&duplicate),
        Err(ChannelImageError::NonAscendingEndpoint)
    ));

    let two = image(&Par::default().with_exprs(vec![gint(-1), gint(1)]));
    let records = endpoint_records(two.as_bytes());
    assert_eq!(records.len(), 2);
    let mut reversed = Vec::new();
    reversed.extend_from_slice(&two.as_bytes()[..CHANNEL_IMAGE_V2_DOMAIN.len()]);
    reversed.extend_from_slice(&two.as_bytes()[records[1].clone()]);
    reversed.extend_from_slice(&two.as_bytes()[records[0].clone()]);
    reversed.extend_from_slice(&two.as_bytes()[records[1].end..]);
    assert!(matches!(
        ChannelImageV2::try_from_bytes(&reversed),
        Err(ChannelImageError::NonAscendingEndpoint)
    ));

    let mut count_mismatch = bytes.to_vec();
    count_mismatch[count_offset..count_offset + 8].copy_from_slice(&2u64.to_be_bytes());
    assert!(matches!(
        ChannelImageV2::try_from_bytes(&count_mismatch),
        Err(ChannelImageError::DistinctCountMismatch {
            declared: 2,
            observed: 1
        })
    ));

    let mut reserved_flag = bytes.to_vec();
    *reserved_flag.last_mut().unwrap() = 0x80;
    assert!(matches!(
        ChannelImageV2::try_from_bytes(&reserved_flag),
        Err(ChannelImageError::InvalidFlags(0x80))
    ));

    let mut trailing = bytes.to_vec();
    trailing.push(0);
    assert!(matches!(
        ChannelImageV2::try_from_bytes(&trailing),
        Err(ChannelImageError::TrailingBytes { .. })
    ));

    let key = single_endpoint_key(bytes);
    let trailer = key.len() - std::mem::size_of::<u64>();
    let score_len =
        usize::try_from(u64::from_be_bytes(key[trailer..].try_into().unwrap())).unwrap();
    let payload_start = PAR_BAG_KEY_DOMAIN.len() + 1 + score_len;
    assert!(payload_start < trailer);

    let mut invalid_score = key.clone();
    invalid_score[PAR_BAG_KEY_DOMAIN.len() + 1] = 0xFE;
    assert!(matches!(
        ChannelImageV2::decode_bytes(image_with_single_endpoint_key(bytes, &invalid_score)),
        Err(ChannelImageError::Component(_))
    ));

    let mut invalid_score_length = key.clone();
    invalid_score_length[trailer..].copy_from_slice(&u64::MAX.to_be_bytes());
    assert!(matches!(
        ChannelImageV2::decode_bytes(image_with_single_endpoint_key(bytes, &invalid_score_length)),
        Err(ChannelImageError::Component(_))
    ));

    let payload = &key[payload_start..trailer];
    assert_eq!(payload, &[0x10, 84], "zig-zag encoded GInt fixture body");

    let mut mismatched_score = key.clone();
    mismatched_score[payload_start + 1] = 86;
    assert!(matches!(
        ChannelImageV2::decode_bytes(image_with_single_endpoint_key(bytes, &mismatched_score)),
        Err(ChannelImageError::Component(_))
    ));

    let mut nonminimal_varint = key.clone();
    nonminimal_varint.splice(payload_start + 1..payload_start + 2, [0xD4, 0x00]);
    assert!(matches!(
        ChannelImageV2::decode_bytes(image_with_single_endpoint_key(bytes, &nonminimal_varint)),
        Err(ChannelImageError::Component(_))
    ));

    let mut unknown_field = key.clone();
    unknown_field.splice(trailer..trailer, [0x98, 0x06, 0x01]);
    assert!(matches!(
        ChannelImageV2::decode_bytes(image_with_single_endpoint_key(bytes, &unknown_field)),
        Err(ChannelImageError::Component(_))
    ));

    let mut duplicate_oneof = key.clone();
    duplicate_oneof.splice(trailer..trailer, payload.iter().copied());
    assert!(matches!(
        ChannelImageV2::decode_bytes(image_with_single_endpoint_key(bytes, &duplicate_oneof)),
        Err(ChannelImageError::Component(_))
    ));
}

#[test]
fn source_gate_keeps_channel_images_on_the_borrowed_trie_stream() {
    let source = include_str!("../src/rust/channel_image.rs");
    assert!(source.contains("try_visit_canonical_key_parts"));
    assert!(source.contains("Bag::try_from_sorted_unique"));
    assert!(source.contains("CanonicalParBagAccumulator"));
    for forbidden in [
        "visit_decoded",
        "decode_occurrences",
        ".sort(",
        ".sort_by",
        ".sort_by_key",
        ".sort_by_cached_key",
        ".sort_unstable",
        ".sort_unstable_by",
        ".sort_unstable_by_key",
        "compact_snapshot(",
        "trie_snapshot(",
        ".iter()",
        "CanonicalParBagBuilder",
        "push_prevalidated",
    ] {
        assert!(
            !source.contains(forbidden),
            "ChannelImageV2 reintroduced forbidden hot-path operation `{forbidden}`"
        );
    }
}

#[test]
fn deep_and_wide_images_use_the_same_iterative_cursor_on_a_small_stack() {
    let deep = Arc::new(par_corpus::deep_mixed_par(192));
    let mut wide = Par::default();
    for value in 0..1_024 {
        wide.insert_component(&gint(value), Vec::new(), false)
            .expect("unique wide endpoint");
    }
    let wide = Arc::new(wide);

    std::thread::Builder::new()
        .name("rci2-small-stack".to_string())
        .stack_size(256 * 1024)
        .spawn(move || {
            for par in [deep, wide] {
                let encoded = image(&par);
                let decoded = encoded.decode_par().expect("iterative RCI2 decode");
                assert_eq!(decoded, *par);
                assert_eq!(image(&decoded), encoded);
            }
        })
        .expect("spawn RCI2 small-stack worker")
        .join()
        .expect("RCI2 small-stack worker completed without overflow");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_bytes_never_escape_the_checked_cursor(bytes in prop::collection::vec(any::<u8>(), 0..16_384)) {
        let _ = ValidatedChannelImage::validate_exact(&bytes);
        let _ = ValidatedChannelImage::take_prefix(&bytes);
    }

    #[test]
    fn multiplicity_order_is_unsigned_big_endian(counts in prop::collection::vec(1u64..=u64::MAX, 1..32)) {
        let expr = gint(5);
        let mut pars = counts
            .into_iter()
            .map(|count| {
                let mut par = Par::default();
                par.insert_component_many(
                    &expr,
                    Multiplicity::new(count).unwrap(),
                    Vec::new(),
                    false,
                ).unwrap();
                par
            })
            .collect::<Vec<_>>();
        let mut images = pars.iter().map(image).collect::<Vec<_>>();
        pars.sort();
        images.sort();
        prop_assert_eq!(
            images.into_iter().map(|value| value.decode_par().unwrap()).collect::<Vec<_>>(),
            pars
        );
    }
}
