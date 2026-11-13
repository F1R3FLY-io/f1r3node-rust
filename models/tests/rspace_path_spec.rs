use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, ListParWithRandom, Par};
use models::rust::canonical_path::decode_trie_path;
use models::rust::channel_image::ChannelImageV2;
use models::rust::rspace_path::{
    rho_path_v2_continuation_key, rho_path_v2_continuation_prefix, rho_path_v2_datum_key,
    rho_path_v2_join_key, rho_path_v2_join_prefix,
};
use rspace_plus_plus::rspace::data_index::RSpaceDataIndex;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::rho_path::{RhoPathEncode, RhoPathV1};

mod par_corpus;

fn integer(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

#[test]
fn par_channel_structure_is_the_canonical_reversible_trie_path() {
    let channel = integer(42);
    let path = RhoPathV1::datum_key(&channel, b"datum", b"identity");
    let view = RhoPathV1::parse_datum(&path).unwrap();
    assert_eq!(
        decode_trie_path(&view.channel_structure()).unwrap(),
        channel
    );
}

#[test]
fn v2_channel_field_is_the_authoritative_rci2_image() {
    let channel = integer(42);
    let path = rho_path_v2_datum_key(&channel, b"rmp2", b"identity").unwrap();
    let view = rspace_plus_plus::rspace::rho_path::RhoPathV2::parse_datum(&path).unwrap();
    let image = ChannelImageV2::from_par(&channel).unwrap();
    assert_eq!(view.channel_image(), image.as_bytes());
    let join = rho_path_v2_join_key(&channel, b"join").unwrap();
    assert!(rspace_plus_plus::rspace::rho_path::RhoPathV2::parse_join(&join).is_ok());
    let continuation_prefix =
        rho_path_v2_continuation_prefix(std::slice::from_ref(&channel)).unwrap();
    let continuation =
        rho_path_v2_continuation_key(std::slice::from_ref(&channel), b"continuation").unwrap();
    assert!(continuation.starts_with(&continuation_prefix));
    assert!(join.starts_with(&rho_path_v2_join_prefix(&channel).unwrap()));
}

#[test]
fn random_state_changes_identity_without_destroying_structural_sharing() {
    let first = ListParWithRandom {
        pars: vec![integer(1), integer(2)],
        random_state: vec![1, 2, 3],
        cost_region: None,
    };
    let second = ListParWithRandom {
        random_state: vec![4, 5, 6],
        ..first.clone()
    };
    assert_eq!(first.rho_path_structure(), second.rho_path_structure());
    assert_ne!(first.rho_path_identity(), second.rho_path_identity());
}

#[test]
fn par_sequence_framing_preserves_arity_and_element_boundaries() {
    let one = ListParWithRandom {
        pars: vec![integer(1), integer(2)],
        random_state: Vec::new(),
        cost_region: None,
    };
    let other = ListParWithRandom {
        pars: vec![integer(12)],
        random_state: Vec::new(),
        cost_region: None,
    };
    let longer = ListParWithRandom {
        pars: vec![integer(1), integer(2), integer(3)],
        random_state: Vec::new(),
        cost_region: None,
    };
    assert_ne!(one.rho_path_structure(), other.rho_path_structure());
    assert_ne!(one.rho_path_structure(), longer.rho_path_structure());
}

#[test]
fn data_index_collapses_exact_duplicates_but_not_random_state() {
    let channel = integer(99);
    let first = ListParWithRandom {
        pars: vec![integer(1), integer(2)],
        random_state: vec![1, 2, 3],
        cost_region: None,
    };
    let second = ListParWithRandom {
        random_state: vec![4, 5, 6],
        ..first.clone()
    };
    let first_datum = Datum::create(&channel, first.clone(), false);
    let second_datum = Datum::create(&channel, second.clone(), false);
    let mut index = RSpaceDataIndex::new();
    index.insert(&channel, &first_datum).unwrap();
    index.insert(&channel, &first_datum).unwrap();
    index.insert(&channel, &second_datum).unwrap();
    let entries = index
        .structure_prefix_entries::<_, ListParWithRandom>(&channel, &first.rho_path_structure())
        .unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.multiplicity.get())
            .sum::<u64>(),
        3
    );
    assert!(entries.iter().any(|entry| entry.datum.a.as_ref() == &first));
    assert!(entries
        .iter()
        .any(|entry| entry.datum.a.as_ref() == &second));
}

fn channel_image(channel: &Par) -> Vec<u8> {
    rspace_plus_plus::rspace::rho_path::RhoPathChannelImage::rho_path_channel_image(channel)
}

fn channel_from_image(image: &[u8]) -> Result<Par, String> {
    rspace_plus_plus::rspace::rho_path::RhoPathChannelImage::from_rho_path_channel_image(image)
}

fn decoded_channel(label: &str, image: &[u8]) -> Par {
    match channel_from_image(image) {
        Ok(decoded) => decoded,
        Err(error) => panic!("the channel image of {label} is refused: {error}"),
    }
}

#[test]
fn par_channel_image_round_trips_up_to_par_equality() {
    for (label, channel) in par_corpus::par_corpus() {
        let image = channel_image(&channel);
        let rci2_image =
            ChannelImageV2::from_par(&channel).expect("a resident Par has a canonical RCI2 image");
        assert_eq!(
            image,
            rci2_image.as_bytes(),
            "the channel image is the RCI2 image: {label}"
        );
        let decoded = decoded_channel(&label, &image);
        assert_eq!(
            decoded, channel,
            "the decoded channel equals the original: {label}"
        );
        assert_eq!(
            channel_image(&decoded),
            image,
            "the image of the decoded channel is the same bytes: {label}"
        );
    }

    let valid = channel_image(&integer(42));
    let mut other_version = valid.clone();
    other_version[3] = 0x01;
    let mut trailing_byte = Vec::with_capacity(valid.len() + 1);
    trailing_byte.extend_from_slice(&valid);
    trailing_byte.push(0);
    let mut unknown_flag = valid.clone();
    *unknown_flag
        .last_mut()
        .expect("an RCI2 image ends with its flags byte") |= 0x02;
    let malformed_images: [&[u8]; 6] = [
        &[],
        b"RC",
        &other_version,
        &valid[..valid.len() - 1],
        &trailing_byte,
        &unknown_flag,
    ];
    for image in malformed_images {
        let refusal = ChannelImageV2::try_from_bytes(image)
            .expect_err("the fixture is not a canonical RCI2 image")
            .to_string();
        assert_eq!(
            channel_from_image(image),
            Err(refusal),
            "a malformed image is refused with the ChannelImageError text: {image:?}"
        );
    }
}

#[test]
fn par_channel_image_ignores_root_locally_free_and_keeps_connective_used() {
    for (label, channel) in par_corpus::par_corpus() {
        let image = channel_image(&channel);
        let other_locally_free = channel.with_locally_free(vec![0x00, 0xAA, 0xFF]);
        assert_ne!(
            other_locally_free.locally_free(),
            channel.locally_free(),
            "the variant has another root locally_free: {label}"
        );
        assert_eq!(
            channel_image(&other_locally_free),
            image,
            "the image does not read root locally_free: {label}"
        );
        let other_connective_used = channel.with_connective_used(!channel.connective_used());
        let other_image = channel_image(&other_connective_used);
        assert_ne!(
            other_image, image,
            "the image reads root connective_used: {label}"
        );
        for (variant, variant_image) in [
            (&other_locally_free, &image),
            (&other_connective_used, &other_image),
        ] {
            let decoded = decoded_channel(&label, variant_image);
            assert!(
                decoded.locally_free().is_empty(),
                "the decoded channel has an empty root locally_free: {label}"
            );
            assert_eq!(
                decoded.connective_used(),
                variant.connective_used(),
                "the decoded channel keeps root connective_used: {label}"
            );
        }
    }
}

#[test]
fn decoded_image_keeps_the_legacy_identity_of_normalized_channels() {
    let corpus = par_corpus::par_corpus();
    assert!(
        corpus
            .iter()
            .any(|(_, channel)| !channel.locally_free().is_empty()),
        "the Par corpus holds a channel with a non-empty root locally_free"
    );
    let mut changed_identities = Vec::with_capacity(corpus.len());
    for (label, channel) in &corpus {
        let decoded = decoded_channel(label, &channel_image(channel));
        if decoded.rho_path_identity() != channel.rho_path_identity() {
            changed_identities.push(label.as_str());
        }
    }
    assert!(
        changed_identities.is_empty(),
        "the legacy identity changed for {} of {} corpus channels: {changed_identities:?}",
        changed_identities.len(),
        corpus.len()
    );
}
