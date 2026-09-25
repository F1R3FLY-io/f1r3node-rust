use proptest::prelude::*;

use super::*;
use crate::rust::epathmap_trie_codec::payload_mapping_tests::capture_allocations;

fn literal(arena: &mut ParConstruction, score: &[u8], body: &[u8]) -> PlannedComponent {
    let score = arena.score_from_owned(score.to_vec()).unwrap();
    let body = arena
        .push_proto_node(ProtoProgram::One(ProtoPiece::Bytes(body.into())))
        .unwrap();
    arena.component_from_proto(ParComponentTag::Expr, score, body, false, None)
}

fn old_key(arena: &ParConstruction, component: PlannedComponent) -> Vec<u8> {
    let component = &arena.components[component.0];
    let mut key = PAR_BAG_KEY_DOMAIN.to_vec();
    key.push(component.tag.byte());
    key.extend(ScoreByteCursor::new(arena, component.score));
    key.extend(WireByteCursor::new(
        arena,
        WireRoot::Proto(component.protobuf),
    ));
    key.extend_from_slice(&(arena.scores[component.score.0].encoded_len as u64).to_be_bytes());
    key
}

fn old_finish(arena: &ParConstruction, root: PlannedPar) -> Par {
    let planned = &arena.pars[root.0];
    let mut bag = ParBag::new();
    for occurrence in OccurrenceIter::new(&arena.occurrence_nodes, planned.occurrence_root) {
        let node = &arena.components[occurrence.component.0];
        bag.insert_planned_key(
            node.tag,
            &old_key(arena, occurrence.component),
            occurrence.multiplicity,
            node.canonical_path_stable,
        )
        .unwrap();
    }
    Par::from_parts(bag, planned.locally_free.clone(), planned.connective_used)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]
    #[test]
    fn compact_programs_and_reused_keys_preserve_literal_and_composite_bytes(
        bodies in proptest::collection::vec(proptest::collection::vec(any::<u8>(), 0..40), 0..40),
        score in proptest::collection::vec(any::<u8>(), 0..40),
    ) {
        let mut arena = ParConstruction::new();
        let mut program = ProtoProgram::default();
        let mut expected = Vec::new();
        for (index, body) in bodies.iter().enumerate() {
            program.push(if index % 2 == 0 { ProtoPiece::Bytes(body.clone().into()) }
                else { ProtoPiece::SharedBytes(Box::new(prost::bytes::Bytes::copy_from_slice(body))) });
            expected.extend_from_slice(body);
        }
        let moved = std::mem::take(&mut program).into_vec();
        prop_assert!(program.as_slice().is_empty());
        let proto = arena.push_proto_node(moved).unwrap();
        let mut out = vec![33];
        arena.emit_proto(proto, &mut out);
        prop_assert_eq!(&out[1..], expected.as_slice());
        prop_assert_eq!(arena.protobuf_nodes[proto].encoded_len, expected.len());
        prop_assert_eq!(arena.protobuf_nodes[proto].zero_count, expected.iter().filter(|&&b| b == 0).count());
        let encoded = arena.score_from_owned(score.clone()).unwrap();
        let component = arena.component_from_proto(ParComponentTag::Expr, encoded, proto, false, None);
        arena.component_key_into(component, &mut out).unwrap();
        prop_assert_eq!(&out, &old_key(&arena, component));
        let other = literal(&mut arena, &[], &[]);
        arena.component_key_into(other, &mut out).unwrap();
        prop_assert_eq!(out, old_key(&arena, other));
    }

    #[test]
    fn one_cursor_publication_matches_prior_metrics_metadata_and_bytes(
        values in proptest::collection::vec((any::<i64>(), 1u64..16), 0..100),
        locally_free in proptest::collection::vec(any::<u8>(), 0..12), flag in any::<bool>(),
    ) {
        let mut arena = ParConstruction::new();
        let mut items = Vec::new();
        for (value, count) in values {
            let component = arena.expr_component(&Expr { expr_instance: Some(ExprInstance::GInt(value)) }, &[], false).unwrap();
            items.push((component, Multiplicity::new(count).unwrap()));
        }
        let root = arena.par(items, locally_free, flag).unwrap();
        let expected = old_finish(&arena, root);
        let actual = arena.finish(root).unwrap();
        prop_assert_eq!(actual.compact_snapshot(), expected.compact_snapshot());
        prop_assert_eq!(actual.protobuf_bytes().unwrap(), expected.protobuf_bytes().unwrap());
        prop_assert_eq!(actual.components().cardinality(), expected.components().cardinality());
        prop_assert_eq!(actual.components().distinct_len(), expected.components().distinct_len());
    }
}

#[test]
fn prefix_body_trailer_inversion_does_not_require_sorted_publication() {
    let mut arena = ParConstruction::new();
    let shorter = literal(&mut arena, &[1], &[]);
    let longer = literal(&mut arena, &[1], &[0]);
    assert_eq!(arena.compare_components(shorter, longer), Ordering::Less);
    assert!(old_key(&arena, shorter) > old_key(&arena, longer));
    let root = arena
        .par(
            [(shorter, Multiplicity::ONE), (longer, Multiplicity::ONE)],
            vec![],
            false,
        )
        .unwrap();
    let actual = arena.finish(root).unwrap();
    let expected = old_finish(&arena, root);
    assert_eq!(
        actual.components().compact_bytes(),
        expected.components().compact_bytes()
    );
}

#[test]
fn warmed_literal_key_buffer_emits_without_allocating() {
    let mut arena = ParConstruction::new();
    let item = literal(&mut arena, &[2, 1, 0], &[0, 255, 0]);
    let mut key = old_key(&arena, item);
    let (_, counts) = capture_allocations(|| {
        for _ in 0..256 {
            arena.component_key_into(item, &mut key).unwrap();
        }
    });
    assert_eq!(counts.allocations, 0);
    assert_eq!(counts.freed, 0);
    assert_eq!(key, old_key(&arena, item));
    assert!(std::mem::size_of::<ScoreNode>() <= 32);
    assert!(std::mem::size_of::<ProtoNode>() <= 48);
}

#[test]
fn publication_keeps_every_family_and_singleton_score_cache() {
    let mut arena = ParConstruction::new();
    let components = [
        arena.import_component(&Send::default()).unwrap(),
        arena.import_component(&Receive::default()).unwrap(),
        arena.import_component(&New::default()).unwrap(),
        arena
            .expr_component(
                &Expr {
                    expr_instance: Some(ExprInstance::GInt(9)),
                },
                &[],
                false,
            )
            .unwrap(),
        arena.import_component(&Match::default()).unwrap(),
        arena.import_component(&GUnforgeable::default()).unwrap(),
        arena.import_component(&Bundle::default()).unwrap(),
        arena.import_component(&Connective::default()).unwrap(),
        arena.import_component(&If::default()).unwrap(),
    ];
    for component in components {
        for count in [Multiplicity::ONE, Multiplicity::new(3).unwrap()] {
            let root = arena.par([(component, count)], vec![1, 2], true).unwrap();
            let actual = arena.finish(root).unwrap();
            let expected = old_finish(&arena, root);
            assert_eq!(actual.compact_snapshot(), expected.compact_snapshot());
            assert_eq!(
                actual.protobuf_bytes().unwrap(),
                expected.protobuf_bytes().unwrap()
            );
            let (mut actual_score, mut expected_score) = (Vec::new(), Vec::new());
            let (_, counts) = capture_allocations(|| {
                actual
                    .components()
                    .append_par_score_key(true, &mut actual_score)
            });
            let (_, old_counts) = capture_allocations(|| {
                expected
                    .components()
                    .append_par_score_key(true, &mut expected_score)
            });
            assert_eq!(actual_score, expected_score);
            assert_eq!(counts.allocations, old_counts.allocations);
        }
    }
    let root = arena
        .par(
            components
                .into_iter()
                .map(|component| (component, Multiplicity::new(3).unwrap())),
            vec![1, 2],
            true,
        )
        .unwrap();
    let actual = arena.finish(root).unwrap();
    let expected = old_finish(&arena, root);
    assert_eq!(actual.compact_snapshot(), expected.compact_snapshot());
    assert_eq!(
        actual.protobuf_bytes().unwrap(),
        expected.protobuf_bytes().unwrap()
    );
}
