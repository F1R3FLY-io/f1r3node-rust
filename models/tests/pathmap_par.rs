use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par as GeneratedPar};
use models::rust::par::Par;
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
