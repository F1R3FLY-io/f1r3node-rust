use proptest::prelude::*;

use super::*;
use crate::rust::epathmap_trie_codec::payload_mapping_tests::capture_allocations;

fn reference(arena: &ParConstruction, left: PlannedComponent, right: PlannedComponent) -> Ordering {
    let left = &arena.components[left.0];
    let right = &arena.components[right.0];
    left.tag
        .cmp(&right.tag)
        .then_with(|| {
            ScoreByteCursor::new(arena, left.score).cmp(ScoreByteCursor::new(arena, right.score))
        })
        .then_with(|| {
            WireByteCursor::new(arena, WireRoot::Proto(left.protobuf))
                .cmp(WireByteCursor::new(arena, WireRoot::Proto(right.protobuf)))
        })
}

fn subject(arena: &mut ParConstruction, score: &[u8], body: &[u8], shape: u8) -> PlannedComponent {
    let fragment = arena.bytes(score);
    let literal = arena
        .score_from_pieces(vec![ScorePiece::Bytes(fragment)])
        .unwrap();
    let score = match shape % 4 {
        0 | 1 => literal,
        2 => arena
            .score_from_pieces(vec![ScorePiece::Child(literal)])
            .unwrap(),
        _ => {
            let empty = arena.bytes(&[][..]);
            arena
                .score_from_pieces(vec![ScorePiece::Bytes(fragment), ScorePiece::Bytes(empty)])
                .unwrap()
        }
    };
    let pieces = match shape % 4 {
        0 => vec![ProtoPiece::Bytes(body.into())],
        1 => vec![ProtoPiece::SharedBytes(Box::new(
            prost::bytes::Bytes::copy_from_slice(body),
        ))],
        2 => vec![
            ProtoPiece::Bytes(body.into()),
            ProtoPiece::Bytes(Box::new([])),
        ],
        _ if body.is_empty() => vec![],
        _ => vec![ProtoPiece::Bytes(body.into())],
    };
    let protobuf = arena.push_proto_node(pieces).unwrap();
    arena.component_from_proto(ParComponentTag::Expr, score, protobuf, false, None)
}

fn assert_pair(arena: &ParConstruction, left: PlannedComponent, right: PlannedComponent) {
    let expected = reference(arena, left, right);
    assert_eq!(arena.compare_components(left, right), expected);
    assert_eq!(arena.compare_components(right, left), expected.reverse());
    assert_eq!(arena.compare_components(left, left), Ordering::Equal);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]
    #[test]
    fn fragment_dispatch_refines_cursor(
        left_score in proptest::collection::vec(any::<u8>(), 0..40),
        right_score in proptest::collection::vec(any::<u8>(), 0..40),
        left_body in proptest::collection::vec(any::<u8>(), 0..40),
        right_body in proptest::collection::vec(any::<u8>(), 0..40),
    ) {
        for left_shape in 0..4 {
            for right_shape in 0..4 {
                let mut arena = ParConstruction::new();
                let left = subject(&mut arena, &left_score, &left_body, left_shape);
                let right = subject(&mut arena, &right_score, &right_body, right_shape);
                let before = (arena.byte_fragments.len(), arena.scores.len(), arena.protobuf_nodes.len(), arena.occurrence_nodes.len());
                assert_pair(&arena, left, right);
                prop_assert_eq!(before, (arena.byte_fragments.len(), arena.scores.len(), arena.protobuf_nodes.len(), arena.occurrence_nodes.len()));
                let tie = subject(&mut arena, &left_score, &right_body, right_shape);
                assert_pair(&arena, left, tie);
            }
        }
    }
}

#[test]
fn literal_comparisons_allocate_nothing_and_preserve_arena() {
    let mut arena = ParConstruction::new();
    let mut components = Vec::new();
    for score in [&[][..], &[0], &[0, 0], &[0, 255], &[255]] {
        for body in [&[][..], &[0], &[0, 255], &[255]] {
            for shape in 0..2 {
                components.push(subject(&mut arena, score, body, shape));
            }
        }
    }
    let keys: Vec<_> = components
        .iter()
        .map(|&component| arena.component_key(component).unwrap())
        .collect();
    let expected: Vec<Vec<_>> = components
        .iter()
        .map(|&left| {
            components
                .iter()
                .map(|&right| reference(&arena, left, right))
                .collect()
        })
        .collect();
    let ((), counts) = capture_allocations(|| {
        for (i, &left) in components.iter().enumerate() {
            for (j, &right) in components.iter().enumerate() {
                assert_eq!(
                    std::hint::black_box(arena.compare_components(left, right)),
                    expected[i][j]
                );
            }
        }
    });
    assert_eq!(counts.allocations, 0);
    assert_eq!(counts.allocated, 0);
    assert_eq!(counts.freed, 0);
    for (component, key) in components.into_iter().zip(keys) {
        assert_eq!(arena.component_key(component).unwrap(), key);
    }
}

#[test]
fn tag_precedence_and_equal_score_body_tiebreak_are_retained() {
    let mut arena = ParConstruction::new();
    let left = subject(&mut arena, &[1], &[2], 0);
    let right = subject(&mut arena, &[1], &[3], 1);
    assert_eq!(arena.compare_components(left, right), Ordering::Less);
    arena.components[left.0].tag = ParComponentTag::Send;
    assert_eq!(
        arena.compare_components(left, right),
        ParComponentTag::Send.cmp(&ParComponentTag::Expr)
    );
    assert_pair(&arena, left, right);
}

#[test]
fn escaped_wire_and_occurrence_scores_use_exact_graph_fallback() {
    let mut arena = ParConstruction::new();
    let first = subject(&mut arena, &[1, 0], &[0, 255], 0);
    let second = subject(&mut arena, &[1, 0], &[0, 254], 1);
    let par = arena
        .par(
            [(first, Multiplicity::ONE), (second, Multiplicity::ONE)],
            vec![0],
            false,
        )
        .unwrap();
    let score = arena.par_score(par);
    let protobuf = arena.components[first.0].protobuf;
    let graph = arena.component_from_proto(ParComponentTag::Expr, score, protobuf, false, None);
    let bytes: Vec<_> = ScoreByteCursor::new(&arena, score).collect();
    let literal = subject(&mut arena, &bytes, &[0, 255], 0);
    assert_pair(&arena, graph, literal);
    assert_eq!(arena.compare_components(graph, literal), Ordering::Equal);
    let escaped = arena
        .score_from_pieces(vec![ScorePiece::EscapedWire {
            root: WireRoot::Proto(protobuf),
            encoded_len: 3,
        }])
        .unwrap();
    let graph = arena.component_from_proto(ParComponentTag::Expr, escaped, protobuf, false, None);
    let literal = subject(&mut arena, &[0, 255, 255], &[0, 255], 0);
    assert_pair(&arena, graph, literal);
    assert_eq!(arena.compare_components(graph, literal), Ordering::Equal);
}
