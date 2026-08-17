use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{ETuple, Expr, Par as GeneratedPar};
use models::rust::par::Par;
use models::rust::par_ext::ParExt;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use prost::bytes::Buf;
use prost::Message;

mod par_corpus;

#[test]
fn pathmap_par_streams_the_exact_canonical_protobuf_surface() {
    let source = models::par_from_default! {
        exprs: vec![
            Expr {
                expr_instance: Some(ExprInstance::GInt(9)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GInt(3)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GInt(9)),
            },
        ],
        locally_free: vec![0x05],
        connective_used: true,
        ..GeneratedPar::default()
    };
    let expected = models::rust::rholang::protobuf_encoder::encode_to_vec(
        &ParSortMatcher::sort_match(&source).term,
    );
    let actual = Par::import_generated(&source).expect("import generated Par");

    let mut streamed = Vec::new();
    actual
        .encode_protobuf(&mut streamed)
        .expect("stream PathMap-backed Par");
    assert_eq!(streamed, expected);
    assert_eq!(
        actual.protobuf_encoded_len().expect("length"),
        expected.len()
    );
    assert_eq!(actual.protobuf_bytes().expect("collected bytes"), expected);
    let decoded = Par::decode(expected.as_slice()).expect("decode into PathMap-backed Par");
    assert_eq!(decoded, actual);
    assert_eq!(decoded.locally_free(), actual.locally_free());
    assert_eq!(actual.components().distinct_len(), 2);
    assert_eq!(actual.components().cardinality(), 3);
    assert_eq!(actual.locally_free(), [0x05]);
    assert!(actual.connective_used());
}

#[test]
fn protobuf_merge_accumulates_multiplicity_without_a_repeated_field_collection() {
    let occurrence = models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(11)),
        }],
        ..GeneratedPar::default()
    };
    let mut bytes = models::rust::rholang::protobuf_encoder::encode_to_vec(&occurrence);
    bytes.extend_from_within(..);

    let decoded = Par::decode(bytes.as_slice()).expect("decode repeated occurrence");
    assert_eq!(decoded.components().distinct_len(), 1);
    assert_eq!(decoded.components().cardinality(), 2);
}

#[test]
fn first_component_uses_canonical_family_order_and_preserves_multiplicity() {
    let source = models::par_from_default! {
        exprs: vec![
            Expr {
                expr_instance: Some(ExprInstance::GInt(9)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GInt(3)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GInt(3)),
            },
        ],
        ..GeneratedPar::default()
    };
    let value = Par::import_generated(&source).expect("import");

    let (first, multiplicity) = value
        .first::<Expr>()
        .expect("decode first expression")
        .expect("expression family is nonempty");
    assert_eq!(first.expr_instance, Some(ExprInstance::GInt(3)));
    assert_eq!(multiplicity.get(), 2);
    assert!(Par::default()
        .first::<Expr>()
        .expect("empty lookup")
        .is_none());

    let mut occurrences = Vec::new();
    value
        .try_visit_occurrences::<Expr, std::convert::Infallible>(|expr| {
            occurrences.push(expr.clone());
            Ok(())
        })
        .expect("visit occurrences");
    assert_eq!(occurrences.len(), 3);
    assert_eq!(
        occurrences
            .iter()
            .filter(|expr| expr.expr_instance == Some(ExprInstance::GInt(3)))
            .count(),
        2
    );
}

#[test]
fn scalar_and_tuple_extractors_do_not_depend_on_repeated_field_borrows() {
    let cases = [
        ExprInstance::GString("value".to_owned()),
        ExprInstance::GInt(41),
        ExprInstance::GBool(true),
        ExprInstance::ETupleBody(ETuple {
            ps: vec![GeneratedPar::default()],
            ..ETuple::default()
        }),
    ];

    for expr_instance in cases {
        let generated = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(expr_instance),
            }],
            ..GeneratedPar::default()
        };
        let pathmap = Par::import_generated(&generated).expect("import scalar Par");

        assert_eq!(pathmap.get_g_string(), generated.get_g_string());
        assert_eq!(pathmap.get_g_int(), generated.get_g_int());
        assert_eq!(pathmap.get_g_bool(), generated.get_g_bool());
        assert_eq!(pathmap.get_e_tuple_body(), generated.get_e_tuple_body());
    }
}

#[test]
fn protobuf_decode_dispatches_every_component_family_from_a_chunked_buffer() {
    let mut source = par_corpus::all_par_fields();
    source.sends.retain(|send| send.chan.is_some());
    for receive in &mut source.receives {
        receive.binds.retain(|bind| bind.source.is_some());
    }
    source.news.retain(|new_node| new_node.p.is_some());
    source.exprs.truncate(1);
    for match_node in &mut source.matches {
        match_node
            .cases
            .retain(|case| case.pattern.is_some() && case.source.is_some());
    }
    source.bundles.retain(|bundle| bundle.body.is_some());
    for conditional in &mut source.conditionals {
        conditional
            .if_false
            .get_or_insert_with(GeneratedPar::default);
    }

    let expected = models::rust::rholang::protobuf_encoder::encode_to_vec(
        &ParSortMatcher::sort_match(&source).term,
    );
    let split = expected.len() / 2;
    let chunks = (&expected[..split]).chain(&expected[split..]);
    let decoded = Par::decode(chunks).expect("decode every component family across chunks");
    let imported = Par::import_generated(&source).expect("import every component family");

    assert_eq!(decoded, imported);
    assert_eq!(decoded.locally_free(), imported.locally_free());
    assert_eq!(decoded.protobuf_bytes().expect("re-encode"), expected);
}

#[test]
fn protobuf_decode_rejects_truncated_component_bodies() {
    let malformed = [0x0a, 0x05, 0x00];
    assert!(Par::decode(malformed.as_slice()).is_err());
}

#[test]
fn bincode_is_one_memoized_prefix_compressed_snapshot_plus_root_metadata() {
    let source = models::par_from_default! {
        exprs: vec![
            Expr {
                expr_instance: Some(ExprInstance::GInt(9)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GInt(3)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GInt(9)),
            },
        ],
        locally_free: vec![0x05],
        connective_used: true,
        ..GeneratedPar::default()
    };
    let value = Par::import_generated(&source).expect("import");
    let snapshot = value.compact_snapshot();
    let bincode = bincode::serialize(&value).expect("serialize direct Par image");
    let driven = models::rust::rholang::bincode_encoder::encode(&value);

    assert_eq!(bincode, driven);
    assert_eq!(
        u64::from_le_bytes(bincode[..8].try_into().expect("snapshot length")),
        snapshot.len() as u64
    );
    assert_eq!(&bincode[8..8 + snapshot.len()], snapshot);
    assert_eq!(&bincode[8 + snapshot.len()..16 + snapshot.len()], &[0; 8]);
    assert_eq!(bincode[16 + snapshot.len()], 1);

    let decoded: Par = bincode::deserialize(&bincode).expect("decode direct Par image");
    assert_eq!(decoded, value);
    assert!(decoded.locally_free().is_empty());
    assert!(decoded.connective_used());

    let mut shared = value.clone();
    assert_eq!(
        shared.compact_snapshot().as_ptr(),
        value.compact_snapshot().as_ptr(),
        "CoW clones must share the immutable compact image"
    );
    let added = Expr {
        expr_instance: Some(ExprInstance::GInt(41)),
    };
    shared
        .insert_component(&added, Vec::new(), false)
        .expect("mutate CoW clone");
    assert_ne!(shared.compact_snapshot(), value.compact_snapshot());
    assert!(!value.contains(&added));
    assert!(shared.contains(&added));
}

#[test]
fn semantic_debug_matches_the_canonical_generated_surface_without_a_vec_shadow() {
    let source = models::par_from_default! {
        exprs: vec![
            Expr {
                expr_instance: Some(ExprInstance::GInt(9)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GInt(3)),
            },
            Expr {
                expr_instance: Some(ExprInstance::GInt(9)),
            },
        ],
        locally_free: vec![0x05],
        connective_used: true,
        ..GeneratedPar::default()
    };
    let canonical = ParSortMatcher::sort_match(&source).term;
    let value = Par::import_generated(&source).expect("import");

    assert_eq!(format!("{value:?}"), format!("{canonical:?}"));
}

#[test]
fn equality_hashing_and_ordering_ignore_only_transient_root_locally_free() {
    let base = models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(7)),
        }],
        ..GeneratedPar::default()
    };
    let plain = Par::import_generated(&base).expect("plain");
    let mut tagged_source = base.clone();
    tagged_source.locally_free = vec![0xff];
    let tagged = Par::import_generated(&tagged_source).expect("tagged");

    assert_eq!(plain, tagged);
    assert_eq!(plain.cmp(&tagged), std::cmp::Ordering::Equal);

    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let digest = |value: &Par| {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    };
    assert_eq!(digest(&plain), digest(&tagged));
}

#[test]
fn parallel_composition_uses_bag_sum_and_metadata_join() {
    let left_source = models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(1)),
        }],
        locally_free: vec![0b0000_0001],
        ..GeneratedPar::default()
    };
    let right_source = models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(1)),
        }],
        locally_free: vec![0b0000_0010],
        connective_used: true,
        ..GeneratedPar::default()
    };

    let combined = Par::import_generated(&left_source)
        .expect("left")
        .parallel(Par::import_generated(&right_source).expect("right"))
        .expect("checked bag sum");
    assert_eq!(combined.components().distinct_len(), 1);
    assert_eq!(combined.components().cardinality(), 2);
    assert_eq!(combined.locally_free(), [0b0000_0011]);
    assert!(combined.connective_used());
}
