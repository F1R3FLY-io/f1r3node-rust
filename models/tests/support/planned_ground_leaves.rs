use proptest::prelude::*;

use super::*;
use crate::rhoapi::{ENot, EVar, GBigRational, GFixedPoint, Var};
use crate::rust::epathmap_trie_codec::payload_mapping_tests::capture_allocations;

fn expression(instance: ExprInstance) -> Expr {
    Expr {
        expr_instance: Some(instance),
    }
}

fn generic(arena: &mut ParConstruction, shell: &Expr) -> PlannedComponent {
    let score = ExprSortMatcher::sort_match(shell).score;
    let score = arena.score_tree(&score).unwrap();
    arena.schema_expr_component(shell, &[], score).unwrap()
}

fn check(shell: &Expr) {
    let mut arena = ParConstruction::new();
    let component = arena.expr_component(shell, &[], false).unwrap();
    let node = &arena.components[component.0];
    let mut score = Vec::new();
    encode_order_key(&ExprSortMatcher::sort_match(shell).score, &mut score);
    let protobuf = protobuf_encoder::encode_to_vec(shell);
    assert_eq!(
        ScoreByteCursor::new(&arena, node.score).collect::<Vec<_>>(),
        score
    );
    assert_eq!(
        WireByteCursor::new(&arena, WireRoot::Proto(node.protobuf)).collect::<Vec<_>>(),
        protobuf
    );
    assert_eq!(arena.scores[node.score.0].encoded_len, score.len());
    assert_eq!(
        arena.protobuf_nodes[node.protobuf].encoded_len,
        protobuf.len()
    );
    assert_eq!(
        arena.protobuf_nodes[node.protobuf].zero_count,
        protobuf.iter().filter(|&&b| b == 0).count()
    );
    assert!(matches!(
        node.semantic_shape,
        ComponentSemanticShape::Expr(PlannedExprSemantic::Inert)
    ));
    let key = arena.component_key(component).unwrap();
    let (segment, stable) = arena.expr_canonical_segment(shell, &[]).unwrap();
    assert_eq!(node.canonical_path_stable, stable);
    match (&node.canonical_segment, segment) {
        (None, None) => {}
        (Some(CanonicalSegment::Bytes(actual)), Some(CanonicalSegment::Bytes(expected))) => {
            assert_eq!(actual, &expected)
        }
        (Some(CanonicalSegment::PooledBytes(actual)), Some(CanonicalSegment::Bytes(expected))) => {
            assert_eq!(actual.view(&arena.byte_pool), expected.as_ref())
        }
        _ => panic!("ground segment representation differs"),
    }
    let mut reference = ParConstruction::new();
    let expected = generic(&mut reference, shell);
    assert_eq!(key, reference.component_key(expected).unwrap());
    let root = arena
        .par([(component, Multiplicity::ONE)], vec![], false)
        .unwrap();
    let expected_root = reference
        .par([(expected, Multiplicity::ONE)], vec![], false)
        .unwrap();
    assert_eq!(
        arena.protobuf_bytes(root),
        reference.protobuf_bytes(expected_root)
    );
    let actual = arena.finish(root).unwrap();
    let expected = reference.finish(expected_root).unwrap();
    assert_eq!(actual.compact_snapshot(), expected.compact_snapshot());
    if stable {
        let mut encoded = Vec::new();
        arena
            .emit_canonical_segment_into(root, &mut encoded)
            .unwrap();
        let resident = Par::default().with_exprs(vec![shell.clone()]);
        assert_eq!(
            encoded,
            crate::rust::canonical_path::encode_trie_segment(&resident)
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]
    #[test]
    fn ground_fragments_refine_independent_sorter_and_full_encoder(
        integer in any::<i64>(), bits in any::<u64>(), scale in any::<u32>(),
        bytes in proptest::collection::vec(any::<u8>(), 0..96),
        denominator in proptest::collection::vec(any::<u8>(), 0..96),
        chars in proptest::collection::vec(any::<char>(), 0..32),
    ) {
        let text: String = chars.into_iter().collect();
        for value in [
            Expr::default(), expression(ExprInstance::GBool(false)), expression(ExprInstance::GBool(true)),
            expression(ExprInstance::GInt(integer)), expression(ExprInstance::GDouble(bits)),
            expression(ExprInstance::GString(text.clone())), expression(ExprInstance::GUri(text)),
            expression(ExprInstance::GByteArray(bytes.clone())), expression(ExprInstance::GBigInt(bytes.clone())),
            expression(ExprInstance::GBigRat(GBigRational { numerator: bytes.clone(), denominator })),
            expression(ExprInstance::GFixedPoint(GFixedPoint { unscaled: bytes, scale })),
        ] { check(&value); }
    }
}

#[test]
fn ground_boundary_values_and_raw_bits_remain_exact() {
    check(&Expr::default());
    for value in [i64::MIN, -1, 0, 1, i64::MAX] {
        check(&expression(ExprInstance::GInt(value)));
    }
    for bits in [
        0,
        1u64 << 63,
        f64::INFINITY.to_bits(),
        f64::NEG_INFINITY.to_bits(),
        f64::NAN.to_bits(),
        u64::MAX,
    ] {
        check(&expression(ExprInstance::GDouble(bits)));
    }
    for text in ["", "\0", "a\0λ\0"] {
        check(&expression(ExprInstance::GString(text.into())));
        check(&expression(ExprInstance::GUri(text.into())));
    }
    for bytes in [vec![], vec![0], vec![0, 0, 255, 0]] {
        check(&expression(ExprInstance::GByteArray(bytes.clone())));
        check(&expression(ExprInstance::GBigInt(bytes.clone())));
        check(&expression(ExprInstance::GBigRat(GBigRational {
            numerator: bytes.clone(),
            denominator: bytes.clone(),
        })));
        for scale in [0, 1, u32::MAX] {
            check(&expression(ExprInstance::GFixedPoint(GFixedPoint {
                unscaled: bytes.clone(),
                scale,
            })));
        }
    }
    let mut arena = ParConstruction::new();
    let t = arena
        .expr_component(&expression(ExprInstance::GBool(true)), &[], false)
        .unwrap();
    let f = arena
        .expr_component(&expression(ExprInstance::GBool(false)), &[], false)
        .unwrap();
    assert_eq!(arena.compare_components(t, f), Ordering::Less);
}

#[test]
fn ground_admission_preserves_arity_errors_and_recursive_fallback() {
    let mut arena = ParConstruction::new();
    let child = arena.empty_par().unwrap();
    for (shell, context) in [
        (Expr::default(), "absent Expr"),
        (expression(ExprInstance::GInt(0)), "leaf Expr"),
    ] {
        let before = arena.construction_checkpoint();
        assert!(matches!(arena.expr_component(&shell, &[child], false),
            Err(ParConstructionError::ChildArity { context: actual, expected: 0, supplied: 1 }) if actual == context));
        assert_eq!(arena.construction_checkpoint(), before);
    }
    let shell = expression(ExprInstance::ENotBody(ENot {
        p: Some(Par::default()),
    }));
    let mut out = vec![17];
    assert!(!crate::rust::par_bag::append_leaf_expr_score(
        &shell, &mut out
    ));
    assert_eq!(out, [17]);
    let before = arena.construction_checkpoint();
    assert!(matches!(
        arena.expr_component(&shell, &[], false),
        Err(ParConstructionError::ChildArity {
            context: "unary Expr",
            expected: 1,
            supplied: 0
        })
    ));
    assert_eq!(arena.construction_checkpoint(), before);
    let var = expression(ExprInstance::EVarBody(EVar {
        v: Some(Var {
            var_instance: Some(crate::rhoapi::var::VarInstance::BoundVar(7)),
        }),
    }));
    assert!(!crate::rust::par_bag::append_leaf_expr_score(
        &var, &mut out
    ));
    assert_eq!(out, [17]);
    check(&var);
}

#[test]
fn ground_planning_avoids_general_sorter_allocation_volume() {
    let shell = expression(ExprInstance::GInt(7));
    let mut optimized = ParConstruction::new();
    let (_, optimized_counts) = capture_allocations(|| {
        for _ in 0..64 {
            optimized.expr_component(&shell, &[], false).unwrap();
        }
    });
    let mut reference = ParConstruction::new();
    let (_, reference_counts) = capture_allocations(|| {
        for _ in 0..64 {
            generic(&mut reference, &shell);
        }
    });
    assert!(optimized_counts.allocations < reference_counts.allocations);
    assert!(optimized_counts.allocated * 4 < reference_counts.allocated);
}
