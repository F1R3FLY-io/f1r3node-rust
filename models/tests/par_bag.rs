use std::collections::BTreeMap;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Bundle, Connective, Expr, GUnforgeable, If, Match, New, Par, Receive, Send};
use models::rust::par_bag::{
    with_component_key, ParBag, ParBagCodecError, ParBagProtobufError, ParComponentTag,
    PAR_BAG_KEY_DOMAIN,
};
use prost::Message as _;
use rspace_plus_plus::rspace::bag::{Bag, Multiplicity};

mod par_corpus;
use par_corpus as corpus;

fn occurrence_count(par: &Par) -> usize {
    par.sends.len()
        + par.receives.len()
        + par.news.len()
        + par.exprs.len()
        + par.matches.len()
        + par.unforgeables.len()
        + par.connectives.len()
        + par.bundles.len()
        + par.conditionals.len()
}

#[test]
fn empty_par_is_one_unambiguous_empty_trie() {
    let imported = ParBag::import_legacy(&Par::default()).expect("empty import");
    assert!(imported.is_empty());
    assert_eq!(imported.distinct_len(), 0);
    assert_eq!(imported.cardinality(), 0);
    assert_eq!(
        ParBag::from_compact_bytes(&imported.compact_bytes()).expect("empty image"),
        imported
    );
}

#[test]
fn legacy_import_preserves_every_typed_occurrence() {
    let par = corpus::all_par_fields();
    let bag = ParBag::import_legacy(&par).expect("import");
    assert_eq!(bag.cardinality(), occurrence_count(&par) as u128);

    for value in &par.sends {
        assert!(bag.contains(value));
    }
    for value in &par.receives {
        assert!(bag.contains(value));
    }
    for value in &par.news {
        assert!(bag.contains(value));
    }
    for value in &par.exprs {
        assert!(bag.contains(value));
    }
    for value in &par.matches {
        assert!(bag.contains(value));
    }
    for value in &par.unforgeables {
        assert!(bag.contains(value));
    }
    for value in &par.connectives {
        assert!(bag.contains(value));
    }
    for value in &par.bundles {
        assert!(bag.contains(value));
    }
    for value in &par.conditionals {
        assert!(bag.contains(value));
    }
}

#[test]
fn equal_occurrences_collapse_to_one_path_with_checked_multiplicity() {
    let value = Expr {
        expr_instance: Some(ExprInstance::GInt(7)),
    };
    let mut par = Par::default();
    par.exprs = vec![value.clone(), value.clone(), value.clone()];
    let bag = ParBag::import_legacy(&par).expect("import");
    assert_eq!(bag.distinct_len(), 1);
    assert_eq!(bag.cardinality(), 3);
    assert_eq!(bag.count(&value), 3);
    assert_eq!(bag.family_distinct_len::<Expr>(), 1);
    assert_eq!(bag.family_cardinality::<Expr>(), 3);
}

#[test]
fn constructor_tags_prevent_cross_type_key_collisions() {
    let bundle = Bundle::default();
    let send = Send::default();
    let bundle_key = with_component_key(&bundle, <[u8]>::to_vec);
    let send_key = with_component_key(&send, <[u8]>::to_vec);

    assert_eq!(&bundle_key[..PAR_BAG_KEY_DOMAIN.len()], &PAR_BAG_KEY_DOMAIN);
    assert_eq!(&send_key[..PAR_BAG_KEY_DOMAIN.len()], &PAR_BAG_KEY_DOMAIN);
    assert_eq!(
        bundle_key[PAR_BAG_KEY_DOMAIN.len()],
        ParComponentTag::Bundle.byte()
    );
    assert_eq!(
        send_key[PAR_BAG_KEY_DOMAIN.len()],
        ParComponentTag::Send.byte()
    );
    assert_ne!(bundle_key, send_key);
}

#[test]
fn typed_prefix_cursor_decodes_only_the_requested_family() {
    let par = corpus::all_par_fields();
    let bag = ParBag::import_legacy(&par).expect("import");
    let mut decoded = Vec::new();
    bag.try_visit_decoded::<Expr, ()>(|value, multiplicity| {
        decoded.push((value, multiplicity.get()));
        Ok(())
    })
    .expect("typed traversal");

    assert_eq!(decoded.len(), bag.family_distinct_len::<Expr>());
    assert_eq!(
        decoded
            .iter()
            .map(|(_, count)| u128::from(*count))
            .sum::<u128>(),
        par.exprs.len() as u128
    );
}

#[test]
fn encoded_cursor_exposes_the_canonical_payload_without_decoding() {
    let value = Expr {
        expr_instance: Some(ExprInstance::GInt(99)),
    };
    let mut bag = ParBag::new();
    bag.insert_many(&value, Multiplicity::new(7).expect("positive"))
        .expect("insert");

    let expected = models::rust::rholang::bincode_encoder::encode(&value);
    let mut visits = 0usize;
    bag.visit_encoded::<Expr>(|entry| {
        visits += 1;
        assert_eq!(entry.payload(), expected);
        assert_eq!(entry.multiplicity().get(), 7);
    });
    assert_eq!(visits, 1);
}

#[test]
fn clone_is_copy_on_write_and_family_removal_is_prefix_native() {
    let par = corpus::all_par_fields();
    let original = ParBag::import_legacy(&par).expect("import");
    let mut changed = original.clone();
    let expected_distinct = changed.family_distinct_len::<Expr>();
    let expected_cardinality = changed.family_cardinality::<Expr>();
    let removed = changed.remove_family::<Expr>();

    assert_eq!(removed, (expected_distinct, expected_cardinality));
    assert_eq!(changed.family_cardinality::<Expr>(), 0);
    assert_eq!(
        original.family_cardinality::<Expr>(),
        par.exprs.len() as u128
    );
}

#[test]
fn exact_bag_algebra_preserves_multiplicity_laws() {
    let value = Expr {
        expr_instance: Some(ExprInstance::GInt(42)),
    };
    let mut one = ParBag::new();
    one.insert(&value).expect("one");
    let mut three = ParBag::new();
    three
        .insert_many(&value, Multiplicity::new(3).expect("positive"))
        .expect("three");

    assert_eq!(one.sum(&three).expect("sum").count(&value), 4);
    assert_eq!(one.join(&three).count(&value), 3);
    assert_eq!(one.meet(&three).count(&value), 1);
    assert_eq!(three.subtract(&one).count(&value), 2);
    assert!(one.is_subbag_of(&three));
}

#[test]
fn direct_act_image_round_trips_without_an_entry_projection() {
    let bag = ParBag::import_legacy(&corpus::all_par_fields()).expect("import");
    let bytes = bag.compact_bytes();
    assert_eq!(ParBag::from_compact_bytes(&bytes).expect("decode"), bag);

    let bincode_bytes = bincode::serialize(&bag).expect("serialize");
    let decoded: ParBag = bincode::deserialize(&bincode_bytes).expect("deserialize");
    assert_eq!(decoded, bag);
}

fn canonical_component_par(bag: &ParBag) -> Par {
    let mut par = Par::default();
    macro_rules! expand {
        ($ty:ty, $field:ident) => {
            bag.try_visit_decoded::<$ty, ()>(|value, multiplicity| {
                for _ in 0..multiplicity.get() {
                    par.$field.push(value.clone());
                }
                Ok(())
            })
            .expect("valid ParBag")
        };
    }
    expand!(Send, sends);
    expand!(Receive, receives);
    expand!(New, news);
    expand!(Expr, exprs);
    expand!(Match, matches);
    expand!(GUnforgeable, unforgeables);
    expand!(Connective, connectives);
    expand!(Bundle, bundles);
    expand!(If, conditionals);
    par
}

#[test]
fn protobuf_streams_canonical_trie_order_without_reconstructing_production_vecs() {
    let mut source = corpus::all_par_fields();
    source.exprs.push(source.exprs[0].clone());
    let bag = ParBag::import_legacy(&source).expect("import");

    // Test-only oracle: reconstruct the canonical repeated fields and hand
    // them to the already-proven generated encoder. Production calls the
    // ParBag streamer directly and never builds this projection.
    let oracle = canonical_component_par(&bag);
    let expected = models::rust::rholang::protobuf_encoder::encode_to_vec(&oracle);
    let actual = bag.protobuf_components_to_vec().expect("stream");
    assert_eq!(actual, expected);
    assert_eq!(
        bag.protobuf_components_encoded_len().expect("length"),
        actual.len()
    );
}

#[test]
fn deep_component_import_and_act_validation_are_stack_safe() {
    let par = corpus::deep_par(4_096);
    let bag = ParBag::import_legacy(&par).expect("deep import");
    assert_eq!(bag.cardinality(), 1);
    let bytes = bag.compact_bytes();
    assert_eq!(
        ParBag::from_compact_bytes(&bytes).expect("deep decode"),
        bag
    );
    let protobuf = bag.protobuf_components_to_vec().expect("deep protobuf");
    let decoded = Par::decode(protobuf.as_slice()).expect("stack-safe Par protobuf decoder");
    assert_eq!(decoded.exprs.len(), 1);
}

#[test]
fn protobuf_expansion_rejects_impossible_output_length_before_allocating() {
    let value = Expr {
        expr_instance: Some(ExprInstance::GInt(1)),
    };
    let mut bag = ParBag::new();
    bag.insert_many(
        &value,
        Multiplicity::new(u64::MAX).expect("positive multiplicity"),
    )
    .expect("logical multiplicity fits Bag");
    assert!(matches!(
        bag.protobuf_components_encoded_len(),
        Err(ParBagProtobufError::EncodedLengthOverflow {
            tag: ParComponentTag::Expr,
            multiplicity: u64::MAX,
            ..
        })
    ));
}

#[test]
fn act_import_rejects_every_key_outside_the_typed_language() {
    let invalid_paths = [
        vec![],
        b"BAD\x01\x05".to_vec(),
        b"RPB\x01\xff".to_vec(),
        b"RPB\x01\x05".to_vec(),
    ];

    let mut observed = BTreeMap::new();
    for path in invalid_paths {
        let mut raw = Bag::new();
        raw.insert(&path)
            .expect("raw Bag accepts arbitrary byte paths");
        let error = ParBag::from_compact_bytes(&raw.compact_bytes()).expect_err("invalid key");
        observed.insert(path, error);
    }

    assert!(matches!(
        observed.get(&[][..]),
        Some(ParBagCodecError::KeyTooShort { .. })
    ));
    assert!(matches!(
        observed.get(b"BAD\x01\x05".as_slice()),
        Some(ParBagCodecError::InvalidDomain { .. })
    ));
    assert!(matches!(
        observed.get(b"RPB\x01\xff".as_slice()),
        Some(ParBagCodecError::InvalidTag(0xff))
    ));
    assert!(matches!(
        observed.get(b"RPB\x01\x05".as_slice()),
        Some(ParBagCodecError::ComponentDecode { .. })
    ));
}

#[test]
fn exact_key_validation_rejects_a_valid_payload_with_trailing_bytes() {
    let value = GUnforgeable::default();
    let mut key = with_component_key(&value, <[u8]>::to_vec);
    key.push(0xaa);
    let mut raw = Bag::new();
    raw.insert(&key).expect("raw Bag");

    assert!(matches!(
        ParBag::from_compact_bytes(&raw.compact_bytes()),
        Err(ParBagCodecError::TrailingComponentBytes {
            tag: ParComponentTag::GUnforgeable,
            ..
        })
    ));
}
