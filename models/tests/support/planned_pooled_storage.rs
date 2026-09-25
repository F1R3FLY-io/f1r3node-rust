use proptest::prelude::*;

use super::*;
use crate::rust::epathmap_trie_codec::payload_mapping_tests::capture_allocations;

fn append(arena: &mut ParConstruction, bytes: &[u8]) -> ByteSpan {
    let start = arena.byte_pool.len();
    arena.byte_pool.extend_from_slice(bytes);
    ByteSpan {
        start,
        end: arena.byte_pool.len(),
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]
    #[test]
    fn pooled_owned_shared_emission_survives_growth_and_nested_rewind(
        first in proptest::collection::vec(any::<u8>(), 0..100),
        second in proptest::collection::vec(any::<u8>(), 0..100),
        rejected in proptest::collection::vec(any::<u8>(), 0..4096),
    ) {
        let mut arena = ParConstruction::new();
        let first_span = append(&mut arena, &first);
        let pooled_score = arena.pooled_score(first_span);
        let owned_score = arena.score_from_owned(first.clone()).unwrap();
        let owner_ptr = match arena.scores[owned_score.0].pieces {
            ScoreProgram::Literal(i) => arena.byte_fragments[i].as_ptr(),
            _ => panic!("owned import was copied to a pool"),
        };
        let checkpoint = arena.construction_checkpoint();
        let second_span = append(&mut arena, &second);
        let proto = arena.push_proto_node(vec![
            ProtoPiece::PooledBytes(first_span), ProtoPiece::Bytes(second.clone().into()),
            ProtoPiece::SharedBytes(Box::new(prost::bytes::Bytes::copy_from_slice(&second))),
            ProtoPiece::PooledBytes(second_span),
        ]).unwrap();
        let nested = arena.construction_checkpoint();
        append(&mut arena, &rejected);
        arena.byte_pool.reserve(8192);
        arena.rewind_construction(nested);
        let mut actual = Vec::new();
        arena.emit_proto(proto, &mut actual);
        let expected = [first.as_slice(), second.as_slice(), second.as_slice(), second.as_slice()].concat();
        prop_assert_eq!(&actual, &expected);
        prop_assert_eq!(arena.protobuf_nodes[proto].encoded_len, expected.len());
        prop_assert_eq!(arena.protobuf_nodes[proto].zero_count, expected.iter().filter(|&&b| b == 0).count());
        arena.rewind_construction(checkpoint);
        append(&mut arena, &rejected);
        prop_assert_eq!(ScoreByteCursor::new(&arena, pooled_score).collect::<Vec<_>>(), first.clone());
        prop_assert_eq!(arena.score_fragment(pooled_score), arena.score_fragment(owned_score));
        prop_assert_eq!(arena.score_fragment(owned_score).unwrap().as_ptr(), owner_ptr);
    }

    #[test]
    fn canonical_leaf_append_is_prefix_independent(prefix in proptest::collection::vec(any::<u8>(), 0..100), value in any::<i64>()) {
        let shell = Expr { expr_instance: Some(ExprInstance::GInt(value)) };
        let mut out = prefix.clone();
        prop_assert!(ParConstruction::append_ground_segment(&shell, &mut out));
        let mut expected = prefix;
        let arena = ParConstruction::new();
        let (Some(CanonicalSegment::Bytes(bytes)), true) = arena.expr_canonical_segment(&shell, &[]).unwrap() else { panic!("ground segment missing") };
        expected.extend_from_slice(&bytes);
        prop_assert_eq!(&out, &expected);
        prop_assert!(!ParConstruction::append_ground_segment(&Expr::default(), &mut out));
        prop_assert_eq!(out, expected);
    }
}

#[test]
fn warmed_ground_candidates_allocate_no_fragment_buffers_and_restore_pool() {
    let mut arena = ParConstruction::new();
    let shell = Expr {
        expr_instance: Some(ExprInstance::GInt(71)),
    };
    let checkpoint = arena.construction_checkpoint();
    arena.expr_component(&shell, &[], false).unwrap();
    arena.rewind_construction(checkpoint);
    let (_, counts) = capture_allocations(|| {
        for _ in 0..256 {
            let checkpoint = arena.construction_checkpoint();
            arena.expr_component(&shell, &[], false).unwrap();
            arena.rewind_construction(checkpoint);
        }
    });
    assert_eq!(counts.allocations, 0);
    assert_eq!(counts.freed, 0);
    assert!(arena.byte_pool.is_empty());
    assert!(arena.byte_fragments.is_empty());
}

#[test]
fn compact_link_encoding_is_checked_and_spans_full_addressable_indices() {
    for value in [
        None,
        Some(0),
        Some(1),
        Some(usize::MAX / 2),
        Some(usize::MAX - 1),
    ] {
        let encoded = OccurrenceTreeNode::encode_link(value).unwrap();
        assert_eq!(encoded.map(|value| value.get() - 1), value);
    }
    assert!(OccurrenceTreeNode::encode_link(Some(usize::MAX)).is_err());
    assert_eq!(
        std::mem::size_of::<Option<NonZeroUsize>>(),
        std::mem::size_of::<usize>()
    );
    if usize::BITS == 64 {
        assert_eq!(std::mem::size_of::<ConstructionCheckpoint>(), 64);
        assert_eq!(std::mem::size_of::<(ConstructionCheckpoint, usize)>(), 72);
        assert_eq!(std::mem::size_of::<ScoreCursorOp<'_>>(), 32);
        assert!(std::mem::size_of::<WireCursorOp<'_>>() <= 32);
        assert!(std::mem::size_of::<ScorePiece>() <= 40);
        assert!(std::mem::size_of::<ProtoPiece>() <= 24);
        assert!(std::mem::size_of::<ScoreNode>() <= 32);
        assert!(std::mem::size_of::<ProtoNode>() <= 48);
        assert!(std::mem::size_of::<ComponentNode>() <= 72);
        assert!(std::mem::size_of::<OccurrenceTreeNode>() <= 80);
    }
}

#[test]
fn large_abandoned_payload_does_not_change_a_surviving_small_leaf() {
    let mut arena = ParConstruction::new();
    let small = Expr {
        expr_instance: Some(ExprInstance::GInt(17)),
    };
    let component = arena.expr_component(&small, &[], false).unwrap();
    let root = arena
        .par([(component, Multiplicity::ONE)], vec![], false)
        .unwrap();
    let expected = arena.component_key(component).unwrap();
    let checkpoint = arena.construction_checkpoint();
    let retained = checkpoint.pool;
    let large = Expr {
        expr_instance: Some(ExprInstance::GByteArray(vec![0; 65536])),
    };
    let rejected = arena.expr_component(&large, &[], false).unwrap();
    arena
        .par([(rejected, Multiplicity::ONE)], vec![], false)
        .unwrap();
    arena.rewind_construction(checkpoint);
    assert_eq!(arena.byte_pool.len(), retained);
    arena.expr_component(&small, &[], false).unwrap();
    assert_eq!(arena.component_key(component).unwrap(), expected);
    let mut encoded = Vec::new();
    arena
        .emit_canonical_segment_into(root, &mut encoded)
        .unwrap();
    assert_eq!(
        encoded,
        crate::rust::canonical_path::encode_trie_segment(&Par::default().with_exprs(vec![small]))
    );
}
