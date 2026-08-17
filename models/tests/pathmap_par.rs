use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par as GeneratedPar};
use models::rust::par::Par;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;

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
    assert_eq!(actual.components().distinct_len(), 2);
    assert_eq!(actual.components().cardinality(), 3);
    assert_eq!(actual.locally_free(), [0x05]);
    assert!(actual.connective_used());
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
