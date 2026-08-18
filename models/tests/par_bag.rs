use std::collections::BTreeMap;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{
    Bundle, Connective, Expr, GSysAuthToken, GUnforgeable, If, Match, New, Par, Receive, Send,
};
use models::rust::par_bag::{
    with_component_key, CanonicalParBagKey, ParBag, ParBagCodecError, ParBagProtobufError,
    ParComponentTag, PAR_BAG_KEY_DOMAIN,
};
use prost::Message as _;
use rspace_plus_plus::rspace::bag::{Bag, BagError, Multiplicity};

mod par_corpus;
use par_corpus as corpus;

/// The shared codec corpus intentionally contains missing required children so
/// protobuf decoders exercise absent-field behavior. Canonical sorting has
/// always rejected those shapes; ParBag's domain is the sortable runtime AST.
fn canonical_par_corpus() -> Par {
    let mut par = Par::default();
    par.replace_sends(vec![Send {
        chan: Some(Par::default()),
        ..Send::default()
    }]);
    par.replace_receives(vec![Receive {
        body: Some(Par::default()),
        ..Receive::default()
    }]);
    par.replace_news(vec![New {
        p: Some(Par::default()),
        ..New::default()
    }]);
    par.replace_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(1)),
    }]);
    par.replace_matches(vec![Match {
        target: Some(Par::default()),
        ..Match::default()
    }]);
    par.replace_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GSysAuthTokenBody(GSysAuthToken {})),
    }]);
    par.replace_connectives(vec![Connective {
        connective_instance: Some(ConnectiveInstance::ConnBool(true)),
    }]);
    par.replace_bundles(vec![Bundle {
        body: Some(Par::default()),
        ..Bundle::default()
    }]);
    par.replace_conditionals(vec![If {
        condition: Some(Par::default()),
        if_true: Some(Par::default()),
        if_false: Some(Par::default()),
        ..If::default()
    }]);
    par
}

fn occurrence_count(par: &Par) -> usize {
    usize::try_from(par.cardinality()).expect("fixture cardinality fits usize")
}

#[test]
fn empty_par_is_one_unambiguous_empty_trie() {
    let imported = Par::default().into_components();
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
    let par = canonical_par_corpus();
    let bag = par.clone().into_components();
    assert_eq!(
        bag.cardinality(),
        u64::try_from(occurrence_count(&par)).expect("fixture cardinality fits u64")
    );

    for value in par.decode_occurrences::<Send>() {
        assert!(bag.contains(&value));
    }
    for value in par.decode_occurrences::<Receive>() {
        assert!(bag.contains(&value));
    }
    for value in par.decode_occurrences::<New>() {
        assert!(bag.contains(&value));
    }
    for value in par.decode_occurrences::<Expr>() {
        assert!(bag.contains(&value));
    }
    for value in par.decode_occurrences::<Match>() {
        assert!(bag.contains(&value));
    }
    for value in par.decode_occurrences::<GUnforgeable>() {
        assert!(bag.contains(&value));
    }
    for value in par.decode_occurrences::<Connective>() {
        assert!(bag.contains(&value));
    }
    for value in par.decode_occurrences::<Bundle>() {
        assert!(bag.contains(&value));
    }
    for value in par.decode_occurrences::<If>() {
        assert!(bag.contains(&value));
    }
}

#[test]
fn equal_occurrences_collapse_to_one_path_with_checked_multiplicity() {
    let value = Expr {
        expr_instance: Some(ExprInstance::GInt(7)),
    };
    let par = Par::default().with_exprs(vec![value.clone(), value.clone(), value.clone()]);
    let bag = par.into_components();
    assert_eq!(bag.distinct_len(), 1);
    assert_eq!(bag.cardinality(), 3);
    assert_eq!(bag.count(&value), 3);
    assert_eq!(bag.family_distinct_len::<Expr>(), 1);
    assert_eq!(bag.family_cardinality::<Expr>(), 3);
}

#[test]
fn compiled_typed_keys_reuse_the_exact_authoritative_path() {
    let present = Expr {
        expr_instance: Some(ExprInstance::GString("rho:prepared".into())),
    };
    let absent = Expr {
        expr_instance: Some(ExprInstance::GString("rho:absent".into())),
    };
    let present_key = CanonicalParBagKey::compile(&present);
    let absent_key = CanonicalParBagKey::compile(&absent);
    assert_eq!(
        present_key.as_bytes(),
        with_component_key(&present, <[u8]>::to_vec)
    );

    let mut bag = ParBag::new();
    bag.insert_key_many(&present_key, Multiplicity::new(3).expect("positive"))
        .expect("insert compiled key");
    assert!(bag.contains_key(&present_key));
    assert!(!bag.contains_key(&absent_key));
    assert_eq!(bag.count_key(&present_key), 3);
    assert_eq!(
        bag.remove_key_many(&present_key, Multiplicity::new(2).expect("positive")),
        Some(Multiplicity::new(1).expect("positive"))
    );
    assert_eq!(bag.remove_key(&present_key), None);
}

#[test]
fn constructor_tags_prevent_cross_type_key_collisions() {
    let bundle = Bundle {
        body: Some(Par::default()),
        ..Default::default()
    };
    let send = Send {
        chan: Some(Par::default()),
        ..Default::default()
    };
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
    let par = canonical_par_corpus();
    let bag = par.clone().into_components();
    let mut decoded = Vec::new();
    bag.try_visit_decoded::<Expr, ()>(|value, multiplicity| {
        decoded.push((value, multiplicity.get()));
        Ok(())
    })
    .expect("typed traversal");

    assert_eq!(decoded.len(), bag.family_distinct_len::<Expr>());
    assert_eq!(
        decoded.iter().map(|(_, count)| *count).sum::<u64>(),
        par.family_cardinality::<Expr>()
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

    let expected = models::rust::rholang::protobuf_encoder::encode_to_vec(&value);
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
    let par = canonical_par_corpus();
    let original = par.clone().into_components();
    let mut changed = original.clone();
    let expected_distinct = changed.family_distinct_len::<Expr>();
    let expected_cardinality = changed.family_cardinality::<Expr>();
    let removed = changed.remove_family::<Expr>();

    assert_eq!(removed, (expected_distinct, expected_cardinality));
    assert_eq!(changed.family_cardinality::<Expr>(), 0);
    assert_eq!(
        original.family_cardinality::<Expr>(),
        par.family_cardinality::<Expr>()
    );
}

#[test]
fn family_metrics_track_every_mutation_and_compact_round_trip() {
    let first = Expr {
        expr_instance: Some(ExprInstance::GInt(1)),
    };
    let second = Expr {
        expr_instance: Some(ExprInstance::GInt(2)),
    };
    let send = Send {
        chan: Some(Par::default()),
        ..Send::default()
    };
    let mut bag = ParBag::new();
    bag.insert_many(&first, Multiplicity::new(3).expect("positive"))
        .expect("insert first expression");
    bag.insert_many(&second, Multiplicity::new(2).expect("positive"))
        .expect("insert second expression");
    bag.insert(&send).expect("insert send");
    assert_eq!(bag.family_cardinality::<Expr>(), 5);
    assert_eq!(bag.family_distinct_len::<Expr>(), 2);
    assert_eq!(bag.family_cardinality::<Send>(), 1);

    assert_eq!(
        bag.remove_many(&first, Multiplicity::new(2).expect("positive")),
        Some(Multiplicity::ONE)
    );
    assert_eq!(bag.family_cardinality::<Expr>(), 3);
    assert_eq!(bag.family_distinct_len::<Expr>(), 2);
    assert!(matches!(
        bag.try_remove_many(&first, Multiplicity::new(2).expect("positive")),
        Err(BagError::InsufficientMultiplicity {
            available: 1,
            requested: 2
        })
    ));
    assert_eq!(bag.family_cardinality::<Expr>(), 3);
    assert_eq!(bag.family_distinct_len::<Expr>(), 2);

    assert_eq!(
        bag.remove_many(&second, Multiplicity::new(9).expect("positive")),
        None
    );
    assert_eq!(bag.family_cardinality::<Expr>(), 1);
    assert_eq!(bag.family_distinct_len::<Expr>(), 1);
    assert_eq!(bag.remove_family::<Send>(), (1, 1));
    assert_eq!(bag.family_cardinality::<Send>(), 0);
    assert_eq!(bag.family_distinct_len::<Send>(), 0);

    let decoded = ParBag::from_compact_bytes(bag.compact_snapshot()).expect("compact round trip");
    assert_eq!(decoded.family_cardinality::<Expr>(), 1);
    assert_eq!(decoded.family_distinct_len::<Expr>(), 1);
    assert_eq!(decoded.family_cardinality::<Send>(), 0);
}

#[test]
fn endpoint_overflow_is_atomic_while_bag_lattice_remains_closed() {
    let first = Expr {
        expr_instance: Some(ExprInstance::GInt(1)),
    };
    let second = Expr {
        expr_instance: Some(ExprInstance::GInt(2)),
    };
    let maximum = Multiplicity::new(u64::MAX).expect("positive");

    let mut bag = ParBag::new();
    bag.insert_many(&first, maximum).expect("maximum family");
    let snapshot_ptr = bag.compact_snapshot().as_ptr();
    assert!(matches!(
        bag.insert(&first),
        Err(BagError::MultiplicityOverflow {
            current: u64::MAX,
            added: 1
        })
    ));
    assert_eq!(bag.family_cardinality::<Expr>(), u64::MAX);
    assert!(!bag.family_cardinality_is_saturated::<Expr>());
    assert_eq!(bag.family_distinct_len::<Expr>(), 1);
    assert_eq!(bag.compact_snapshot().as_ptr(), snapshot_ptr);

    bag.insert(&second)
        .expect("a distinct endpoint preserves the u64 value domain");
    assert_eq!(bag.family_cardinality::<Expr>(), u64::MAX);
    assert!(bag.family_cardinality_is_saturated::<Expr>());
    assert_eq!(bag.cardinality(), u64::MAX);
    assert!(bag.cardinality_is_saturated());
    assert_eq!(bag.family_distinct_len::<Expr>(), 2);

    let mut raw = Bag::new();
    with_component_key(&first, |key| {
        raw.insert_many(key, maximum).expect("raw maximum endpoint");
    });
    with_component_key(&second, |key| {
        raw.insert(key).expect("raw second endpoint");
    });
    let decoded = ParBag::from_compact_bytes(&raw.compact_bytes()).expect("valid ACT family");
    assert_eq!(decoded.family_cardinality::<Expr>(), u64::MAX);
    assert!(decoded.family_cardinality_is_saturated::<Expr>());
    assert!(decoded.cardinality_is_saturated());
    assert_eq!(decoded.family_distinct_len::<Expr>(), 2);
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

    let sum = one.sum(&three).expect("sum");
    let join = one.join(&three);
    let meet = one.meet(&three);
    let difference = three.subtract(&one);
    assert_eq!(sum.count(&value), 4);
    assert_eq!(sum.family_cardinality::<Expr>(), 4);
    assert_eq!(join.count(&value), 3);
    assert_eq!(join.family_cardinality::<Expr>(), 3);
    assert_eq!(meet.count(&value), 1);
    assert_eq!(meet.family_cardinality::<Expr>(), 1);
    assert_eq!(difference.count(&value), 2);
    assert_eq!(difference.family_cardinality::<Expr>(), 2);
    assert!(one.is_subbag_of(&three));
}

#[test]
fn scalar_multiplication_reuses_each_canonical_component_key() {
    let value = Expr {
        expr_instance: Some(ExprInstance::GInt(42)),
    };
    let mut source = ParBag::new();
    source.insert(&value).expect("source");
    let scaled = source.scale(17).expect("scaled");

    assert_eq!(source.count(&value), 1);
    assert_eq!(scaled.count(&value), 17);
    assert_eq!(scaled.family_distinct_len::<Expr>(), 1);
    assert_eq!(scaled.family_cardinality::<Expr>(), 17);
    assert!(source.scale(0).expect("zero").is_empty());
}

#[test]
fn direct_act_image_round_trips_without_an_entry_projection() {
    let bag = canonical_par_corpus().into_components();
    let bytes = bag.compact_bytes();
    assert_eq!(ParBag::from_compact_bytes(&bytes).expect("decode"), bag);

    let bincode_bytes = bincode::serialize(&bag).expect("serialize");
    let decoded: ParBag = bincode::deserialize(&bincode_bytes).expect("deserialize");
    assert_eq!(decoded, bag);
}

#[test]
fn protobuf_streams_legacy_canonical_order_without_reconstructing_production_vecs() {
    let mut source = canonical_par_corpus();
    let mut exprs = source.decode_occurrences::<Expr>();
    exprs.push(exprs[0].clone());
    exprs.reverse();
    source.replace_exprs(exprs);

    // The Par encoder and component stream traverse the same authoritative
    // trie; the latter omits only root metadata, which this fixture leaves at
    // its defaults.
    let expected = models::rust::rholang::protobuf_encoder::encode_to_vec(&source);
    let bag = source.into_components();
    let mut actual = Vec::new();
    bag.encode_protobuf_components(&mut actual)
        .expect("direct stream");
    assert_eq!(actual, expected);
    assert_eq!(
        bag.protobuf_components_bytes().expect("collected stream"),
        actual
    );
    assert_eq!(
        bag.protobuf_components_encoded_len().expect("length"),
        actual.len()
    );
}

#[test]
fn deep_component_import_and_act_validation_are_stack_safe() {
    let par = corpus::deep_par(4_096);
    let bag = par.into_components();
    assert_eq!(bag.cardinality(), 1);
    let bytes = bag.compact_bytes();
    assert_eq!(
        ParBag::from_compact_bytes(&bytes).expect("deep decode"),
        bag
    );
    let protobuf = bag.protobuf_components_bytes().expect("deep protobuf");
    let decoded = Par::decode(protobuf.as_slice()).expect("stack-safe Par protobuf decoder");
    assert_eq!(decoded.family_cardinality::<Expr>(), 1);
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
    let mut bad_domain = b"BAD\x03\x05".to_vec();
    bad_domain.extend_from_slice(&0u64.to_be_bytes());
    let mut bad_tag = b"RPB\x03\xff".to_vec();
    bad_tag.extend_from_slice(&0u64.to_be_bytes());
    let mut bad_score = b"RPB\x03\x05".to_vec();
    bad_score.push(0xff);
    bad_score.extend_from_slice(&1u64.to_be_bytes());
    let invalid_paths = [
        vec![],
        bad_domain.clone(),
        bad_tag.clone(),
        bad_score.clone(),
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
        observed.get(bad_domain.as_slice()),
        Some(ParBagCodecError::InvalidDomain { .. })
    ));
    assert!(matches!(
        observed.get(bad_tag.as_slice()),
        Some(ParBagCodecError::InvalidTag(0xff))
    ));
    assert!(matches!(
        observed.get(bad_score.as_slice()),
        Some(ParBagCodecError::InvalidOrderKey { .. })
    ));
}

#[test]
fn exact_key_validation_rejects_noncanonical_unknown_protobuf_fields() {
    let value = GUnforgeable::default();
    let mut key = with_component_key(&value, <[u8]>::to_vec);
    // Unknown varint field 100. The protobuf machine accepts and ignores it,
    // but regenerating the canonical RPB3 key removes it.
    let trailer = key.split_off(key.len() - std::mem::size_of::<u64>());
    key.extend_from_slice(&[0xa0, 0x06, 0x01]);
    key.extend_from_slice(&trailer);
    let mut raw = Bag::new();
    raw.insert(&key).expect("raw Bag");

    assert!(matches!(
        ParBag::from_compact_bytes(&raw.compact_bytes()),
        Err(ParBagCodecError::NonCanonicalComponent {
            tag: ParComponentTag::GUnforgeable
        })
    ));
}

#[test]
fn protobuf_component_bodies_preserve_nested_locally_free_metadata() {
    let mut value = Send::default();
    let mut channel = Par::default();
    channel.replace_locally_free(vec![0x80, 0x01]);
    value.chan = Some(channel);
    let mut bag = ParBag::new();
    bag.insert(&value).expect("insert");

    bag.try_visit_decoded::<Send, ()>(|decoded, multiplicity| {
        assert_eq!(multiplicity.get(), 1);
        assert_eq!(decoded.chan.expect("channel").locally_free(), [0x80, 0x01]);
        Ok(())
    })
    .expect("decode stored protobuf body");
}
