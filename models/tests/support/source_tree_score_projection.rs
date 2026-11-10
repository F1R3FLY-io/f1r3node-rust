use std::collections::HashSet;

use super::*;
use crate::rust::rholang::canonical_bytes_arena::{CanonicalBytesArena, CanonicalLocatedByte};
use crate::rust::rholang::canonical_score_bytes::CanonicalScoreArena;
use crate::rust::rholang::indexed_protobuf::IndexedByteRegion;

fn projected_remaining(
    projection: &SourceDagProjection,
    cursor: &SourceTreeByteCursor<'_>,
) -> Vec<u8> {
    let mut bytes = match cursor.0.active.as_ref() {
        Some(ActiveLeaf::Borrowed { bytes, at, .. }) => bytes[*at..].to_vec(),
        Some(ActiveLeaf::Frozen(cursor)) => cursor.clone().collect(),
        None => Vec::new(),
    };
    for window in projection.pending.iter().rev() {
        bytes.extend(projection.window_bytes(*window));
    }
    bytes
}

#[test]
fn projected_postorder_dag_matches_live_composed_score_cursor() {
    let borrowed_first = [0, 1, 2, 0, 3];
    let borrowed_last = [4, 5, 0, 6];
    let frozen_bytes = (0..193)
        .map(|at| if at % 17 == 0 { 0 } else { at as u8 })
        .collect::<Vec<_>>();
    let mut byte_arena = CanonicalBytesArena::new();
    let mut handle = byte_arena.empty();
    for (at, byte) in frozen_bytes.iter().copied().enumerate() {
        handle = byte_arena
            .append(
                handle,
                CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(37, 1000 + at)),
            )
            .unwrap();
    }
    let frozen = byte_arena.freeze(handle);
    assert!(frozen.stats().content_leaves > 1);

    let first = SourceTreeWindow::from_borrowed(&borrowed_first, 11, 101)
        .unwrap()
        .root
        .unwrap();
    let middle = SourceTreeWindow::from_frozen(frozen).root.unwrap();
    let last = SourceTreeWindow::from_borrowed(&borrowed_last, 13, 303)
        .unwrap()
        .root
        .unwrap();
    let branch2 = node2(Arc::clone(&first), Arc::clone(&middle)).unwrap();
    let branch3 = node3(Arc::clone(&middle), Arc::clone(&last), Arc::clone(&first)).unwrap();
    let root_node = node3(Arc::clone(&branch2), branch3, Arc::clone(&branch2)).unwrap();
    let root = SourceTreeWindow::from_root(Some(root_node));
    let view = root.subview(3..root.len() - 2).unwrap();
    let mut cursor = view.bytes();
    let projection = SourceDagProjection::capture(&view, &cursor.0.pending);
    assert_eq!(projection.nodes.len(), 6);
    assert_eq!(projection.pending, vec![projection.root]);
    let identities = projection
        .nodes
        .iter()
        .map(|node| node.identity)
        .collect::<HashSet<_>>();
    assert_eq!(identities.len(), projection.nodes.len());
    for (id, node) in projection.nodes.iter().enumerate() {
        match &node.payload {
            ProjectedSourcePayload::Borrowed(bytes) | ProjectedSourcePayload::Frozen(bytes) => {
                assert_eq!(bytes.len(), node.len)
            }
            ProjectedSourcePayload::Branch2(children) => {
                assert!(children.iter().all(|child| *child < id));
                assert_eq!(
                    node.len,
                    children
                        .iter()
                        .map(|child| projection.nodes[*child].len)
                        .sum::<usize>()
                );
            }
            ProjectedSourcePayload::Branch3(children) => {
                assert!(children.iter().all(|child| *child < id));
                assert_eq!(
                    node.len,
                    children
                        .iter()
                        .map(|child| projection.nodes[*child].len)
                        .sum::<usize>()
                );
            }
        }
    }
    let root_id = projection.root.root.unwrap();
    let ProjectedSourcePayload::Branch3(children) = &projection.nodes[root_id].payload else {
        panic!("expected ternary root");
    };
    assert_eq!(children[0], children[2]);
    assert!(projection
        .nodes
        .iter()
        .any(|node| { matches!(&node.payload, ProjectedSourcePayload::Branch2(_)) }));
    assert!(projection.nodes.iter().any(|node| {
        matches!(&node.payload, ProjectedSourcePayload::Frozen(bytes) if bytes == &frozen_bytes)
    }));

    let expected = projection.window_bytes(projection.root);
    let mut score_arena = CanonicalScoreArena::new();
    let indexed =
        IndexedByteRegion::from_source(view.clone(), CanonicalByteProvenance::new(13, 307));
    let score = score_arena.indexed_bytes(indexed).unwrap().into_score();
    let mut expected_score = vec![1, 3];
    for byte in &expected {
        expected_score.push(*byte);
        if *byte == 0 {
            expected_score.push(0xff);
        }
    }
    expected_score.extend([0, 0]);
    assert_eq!(score_arena.encoded_len(score), expected_score.len());
    assert_eq!(
        score_arena.cursor(score).collect::<Vec<_>>(),
        expected_score
    );

    for (consumed, byte) in expected.iter().copied().enumerate() {
        let snapshot = SourceDagProjection::capture(&view, &cursor.0.pending);
        assert_eq!(snapshot.root, projection.root);
        assert_eq!(cursor.0.remaining, expected.len() - consumed);
        assert_eq!(
            projected_remaining(&snapshot, &cursor),
            expected[consumed..]
        );
        assert_eq!(cursor.next(), Some(byte));
    }
    assert_eq!(cursor.next(), None);
    let final_snapshot = SourceDagProjection::capture(&view, &cursor.0.pending);
    assert_eq!(
        projected_remaining(&final_snapshot, &cursor),
        Vec::<u8>::new()
    );
}
