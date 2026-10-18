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
