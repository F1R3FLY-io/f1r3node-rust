use std::collections::BTreeSet;
use std::convert::Infallible;

use super::support::{encode, node, nonzero, synthetic_ref};
use crate::rspace::persistence::object::{reset_validating_node_parses, validating_node_parses};
use crate::rspace::persistence::{
    BranchKind, CanonicalBlob, CanonicalBlobKind, CanonicalBlobView, CanonicalDagNode,
    CanonicalDagNodeView, CanonicalEncode, CanonicalFormatError, CanonicalObjectView,
    CanonicalSink, DagChild, EndpointValue,
};

const MAXIMUM_CHILDREN: usize = 256;
const BRANCH_BOUNDARIES: [usize; 9] = [0, 1, 4, 5, 16, 17, 48, 49, 256];
const GENERATED_PREFIX_LENGTHS: [usize; 4] = [0, 1, 40, 300];
const SCATTER_SEED: u64 = 0x9e37_79b9_7f4a_7c15;
const SCATTER_MULTIPLIER: u64 = 6_364_136_223_846_793_005;
const SCATTER_INCREMENT: u64 = 1_442_695_040_888_963_407;

fn scattered_edges(count: usize) -> Vec<u8> {
    let mut edges: Vec<u8> = (0..=u8::MAX).collect();
    let mut state = SCATTER_SEED ^ u64::try_from(count).expect("a child count fits u64");
    for last in (1..edges.len()).rev() {
        state = state
            .wrapping_mul(SCATTER_MULTIPLIER)
            .wrapping_add(SCATTER_INCREMENT);
        let draw = usize::try_from(state >> 33).expect("a 31-bit draw fits usize");
        edges.swap(last, draw % (last + 1));
    }
    edges.truncate(count);
    edges.sort_unstable();
    edges
}

fn edge_selections(count: usize) -> [Vec<u8>; 3] {
    let edge = |value: usize| u8::try_from(value).expect("an edge fits one byte");
    [
        (0..count).map(edge).collect(),
        (MAXIMUM_CHILDREN - count..MAXIMUM_CHILDREN)
            .map(edge)
            .collect(),
        scattered_edges(count),
    ]
}

fn generated_node(variant: usize, edges: &[u8]) -> CanonicalDagNode {
    let endpoints = [
        EndpointValue::Absent,
        EndpointValue::ValueFree,
        EndpointValue::Unit,
        EndpointValue::Multiplicity(nonzero(17)),
        EndpointValue::Content(synthetic_ref(91)),
    ];
    let endpoint = match (edges.len(), endpoints[variant % endpoints.len()]) {
        (1, EndpointValue::Absent) => EndpointValue::Unit,
        (_, endpoint) => endpoint,
    };
    let prefix_len = match (edges.len(), endpoint) {
        (0, EndpointValue::Absent) => 0,
        _ => GENERATED_PREFIX_LENGTHS[variant % GENERATED_PREFIX_LENGTHS.len()],
    };
    let prefix: Vec<u8> = (0..prefix_len)
        .map(|index| u8::try_from(index % 251).expect("a prefix byte fits one byte"))
        .collect();
    let children: Vec<DagChild> = edges
        .iter()
        .map(|&edge| DagChild {
            edge,
            target: synthetic_ref(edge),
        })
        .collect();
    CanonicalDagNode::new(prefix, endpoint, children).expect("a generated node is canonical")
}

#[test]
fn empty_node_has_fixed_little_endian_bytes() {
    let bytes = encode(&CanonicalDagNode::empty());
    let expected = hex::decode(concat!(
        "5253706163654441472e4e6f64650000",
        "0100000000000000",
        "1200000000000000",
        "0000000000000000",
        "00",
        "00",
        "0000000000000000",
    ))
    .expect("golden hex is valid");
    assert_eq!(bytes, expected);
    let view = CanonicalDagNodeView::parse(&bytes).expect("golden node parses");
    assert!(view.prefix().is_empty());
    assert_eq!(view.endpoint(), EndpointValue::Absent);
    assert_eq!(view.children().kind(), BranchKind::Leaf);
    assert!(view.children().is_empty());
    assert_eq!(view.object_ref(), CanonicalDagNode::empty().object_ref());
}

#[test]
fn every_endpoint_round_trips_without_projecting_entries() {
    let endpoints = [
        EndpointValue::Absent,
        EndpointValue::ValueFree,
        EndpointValue::Unit,
        EndpointValue::Multiplicity(nonzero(17)),
        EndpointValue::Content(synthetic_ref(91)),
    ];
    for endpoint in endpoints {
        let prefix: &[u8] = if endpoint == EndpointValue::Absent {
            &[]
        } else {
            &[1, 2, 3]
        };
        let node = node(prefix, endpoint, 0).expect("endpoint node is canonical");
        let bytes = encode(&node);
        let parsed = CanonicalDagNodeView::parse(&bytes).expect("endpoint node parses");
        assert_eq!(parsed.prefix(), prefix);
        assert_eq!(parsed.endpoint(), endpoint);
        assert!(parsed.children().is_empty());
    }
}

#[test]
fn adaptive_branch_boundaries_are_fixed_and_ordered() {
    let cases = [
        (0, BranchKind::Leaf),
        (1, BranchKind::Node4),
        (4, BranchKind::Node4),
        (5, BranchKind::Node16),
        (16, BranchKind::Node16),
        (17, BranchKind::Node48),
        (48, BranchKind::Node48),
        (49, BranchKind::Node256),
        (256, BranchKind::Node256),
    ];
    for (fanout, expected_kind) in cases {
        let endpoint = if fanout == 1 {
            EndpointValue::Unit
        } else {
            EndpointValue::Absent
        };
        let value = node(&[], endpoint, fanout).expect("boundary node is canonical");
        assert_eq!(value.branch_kind(), expected_kind);
        let bytes = encode(&value);
        let parsed = CanonicalDagNodeView::parse(&bytes).expect("boundary node parses");
        assert_eq!(parsed.children().kind(), expected_kind);
        assert_eq!(parsed.children().len(), fanout);
        let edges: Vec<_> = parsed.children().iter().map(|child| child.edge).collect();
        assert_eq!(edges, (0..fanout).map(|edge| edge as u8).collect::<Vec<_>>());
        for edge in 0..fanout {
            assert_eq!(parsed.children().get(edge as u8), Some(synthetic_ref(edge as u8)));
        }
        if fanout < 256 {
            assert_eq!(parsed.children().get(fanout as u8), None);
        }
    }
}

#[test]
fn prefix_and_blob_payload_views_borrow_the_original_bytes() {
    let node_bytes =
        encode(&node(&[7, 8, 9, 10], EndpointValue::Unit, 0).expect("leaf node is canonical"));
    let node = CanonicalDagNodeView::parse(&node_bytes).expect("node parses");
    assert_eq!(node.prefix(), &[7, 8, 9, 10]);
    assert!(std::ptr::eq(node.prefix().as_ptr(), node_bytes[40..].as_ptr()));

    let blob = CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, &b"payload"[..])
        .expect("blob is valid");
    let blob_bytes = encode(&blob);
    let parsed = CanonicalBlobView::parse(&blob_bytes).expect("blob parses");
    assert_eq!(parsed.payload(), b"payload");
    assert!(std::ptr::eq(parsed.payload().as_ptr(), blob_bytes[64..].as_ptr()));
}

#[test]
fn fallible_chunk_sink_matches_contiguous_encoding_and_propagates_failure() {
    #[derive(Default)]
    struct Chunks {
        bytes: Vec<u8>,
        calls: usize,
    }
    impl CanonicalSink for Chunks {
        type Error = Infallible;

        fn write_all(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
            self.calls += 1;
            self.bytes.extend_from_slice(bytes);
            Ok(())
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct SinkStopped;
    struct FailingSink {
        remaining_calls: usize,
    }
    impl CanonicalSink for FailingSink {
        type Error = SinkStopped;

        fn write_all(&mut self, _bytes: &[u8]) -> Result<(), Self::Error> {
            if self.remaining_calls == 0 {
                return Err(SinkStopped);
            }
            self.remaining_calls -= 1;
            Ok(())
        }
    }

    let value = node(&[3, 1, 4], EndpointValue::Unit, 5).expect("node is canonical");
    let contiguous = encode(&value);
    let mut chunks = Chunks::default();
    value
        .write_canonical(&mut chunks)
        .expect("chunk sink succeeds");
    assert!(chunks.calls > 1);
    assert_eq!(chunks.bytes, contiguous);

    let error = value
        .write_canonical(&mut FailingSink { remaining_calls: 2 })
        .expect_err("sink failure propagates");
    assert_eq!(error, SinkStopped);
}

#[test]
fn malformed_envelopes_endpoints_and_branches_fail_closed() {
    let empty = encode(&CanonicalDagNode::empty());

    let mut wrong_domain = empty.clone();
    wrong_domain[0] ^= 1;
    assert!(matches!(
        CanonicalDagNodeView::parse(&wrong_domain),
        Err(CanonicalFormatError::InvalidDomain { .. })
    ));

    let mut wrong_version = empty.clone();
    wrong_version[16] = 2;
    assert!(matches!(
        CanonicalDagNodeView::parse(&wrong_version),
        Err(CanonicalFormatError::UnsupportedVersion { version: 2, .. })
    ));

    let mut trailing = empty.clone();
    trailing.push(0);
    assert!(matches!(
        CanonicalDagNodeView::parse(&trailing),
        Err(CanonicalFormatError::TrailingBytes { .. })
    ));
    assert!(matches!(
        CanonicalDagNodeView::parse(&empty[..empty.len() - 1]),
        Err(CanonicalFormatError::Truncated { .. })
    ));

    let mut wrong_class = empty.clone();
    wrong_class[41] = BranchKind::Node4 as u8;
    assert!(matches!(
        CanonicalDagNodeView::parse(&wrong_class),
        Err(CanonicalFormatError::NonCanonicalBranchClass { .. })
    ));

    let multiplicity = encode(
        &node(&[], EndpointValue::Multiplicity(nonzero(1)), 0)
            .expect("multiplicity node is canonical"),
    );
    let mut zero = multiplicity;
    zero[41..49].fill(0);
    assert_eq!(
        CanonicalDagNodeView::parse(&zero).expect_err("zero multiplicity is rejected"),
        CanonicalFormatError::ZeroMultiplicity
    );

    assert!(matches!(
        CanonicalObjectView::parse(b"too short"),
        Err(CanonicalFormatError::Truncated { .. })
    ));
}

#[test]
fn malformed_child_tables_and_noncompressed_nodes_fail_closed() {
    let mut pairs =
        encode(&node(&[], EndpointValue::Absent, 2).expect("two-child node is canonical"));
    pairs[91] = pairs[50];
    assert!(matches!(
        CanonicalDagNodeView::parse(&pairs),
        Err(CanonicalFormatError::ChildEdgesNotStrict { .. })
    ));

    let mut node48 = encode(&node(&[], EndpointValue::Absent, 17).expect("node48 is canonical"));
    node48[51] = 2;
    assert!(matches!(
        CanonicalDagNodeView::parse(&node48),
        Err(CanonicalFormatError::ChildBitmapCount { .. })
    ));

    let mut node256 = encode(&node(&[], EndpointValue::Absent, 49).expect("node256 is canonical"));
    node256[50] &= !1;
    assert!(matches!(
        CanonicalDagNodeView::parse(&node256),
        Err(CanonicalFormatError::ChildBitmapCount { .. })
    ));

    assert_eq!(
        node(&[1], EndpointValue::Absent, 0).expect_err("dead node is invalid"),
        CanonicalFormatError::EmptyNode
    );
    assert_eq!(
        node(&[], EndpointValue::Absent, 1).expect_err("uncompressed unary node is invalid"),
        CanonicalFormatError::CompressibleUnaryNode
    );
}

#[test]
fn a_rebuilt_view_equals_the_parsed_view_at_every_resume_position() {
    let mut kinds = BTreeSet::new();
    let mut resumes = 0usize;
    for count in 0..=MAXIMUM_CHILDREN {
        for (selection, edges) in edge_selections(count).into_iter().enumerate() {
            let value = generated_node(count * 3 + selection, &edges);
            let bytes = encode(&value);
            let parsed = CanonicalDagNodeView::parse(&bytes).expect("a generated node parses");
            let expected = value.children().to_vec();
            assert_eq!(parsed.children().iter().collect::<Vec<_>>(), expected);
            kinds.insert(parsed.children().kind() as u8);

            let layout = parsed.children_layout();
            let node = layout
                .node(&bytes)
                .expect("the node view is rebuilt from the same bytes");
            assert_eq!(
                (node.prefix(), node.endpoint(), node.object_ref()),
                (parsed.prefix(), parsed.endpoint(), parsed.object_ref()),
                "the rebuilt node view of {count} children has the parsed prefix and endpoint"
            );
            let children = layout
                .children(&bytes)
                .expect("the children view is rebuilt from the same bytes");
            for rebuilt in [node.children(), children] {
                assert_eq!(
                    (rebuilt.kind(), rebuilt.len(), rebuilt.is_empty()),
                    (parsed.children().kind(), count, count == 0)
                );
                assert_eq!(rebuilt.iter().collect::<Vec<_>>(), expected);
                for edge in 0..=u8::MAX {
                    assert_eq!(rebuilt.get(edge), parsed.children().get(edge));
                }
            }

            let mut position = parsed.children().highest();
            for child in expected.iter().rev() {
                let resumed = layout
                    .children(&bytes)
                    .expect("a resume rebuilds the children view");
                let current = position.expect("a position for every child not yet taken");
                assert_eq!(
                    (resumed.at(current), parsed.children().at(current)),
                    (Some(*child), Some(*child)),
                    "the child at a resume position of a node of {count} children"
                );
                let lower = resumed.below(current);
                assert_eq!(
                    lower.and_then(|next| resumed.at(next)),
                    parsed
                        .children()
                        .below(current)
                        .and_then(|next| parsed.children().at(next)),
                    "the next lower position of a node of {count} children"
                );
                position = lower;
                resumes += 1;
            }
            assert!(position.is_none(), "no position is left below the lowest child");
        }
    }
    assert_eq!(
        kinds.into_iter().collect::<Vec<_>>(),
        [
            BranchKind::Leaf as u8,
            BranchKind::Node4 as u8,
            BranchKind::Node16 as u8,
            BranchKind::Node48 as u8,
            BranchKind::Node256 as u8,
        ],
        "every branch encoding is generated"
    );
    assert_eq!(
        resumes,
        3 * (0..=MAXIMUM_CHILDREN).sum::<usize>(),
        "every position of every generated node is resumed"
    );
}

#[test]
fn a_children_layout_refuses_bytes_of_another_length() {
    let empty = encode(&CanonicalDagNode::empty());
    for count in BRANCH_BOUNDARIES {
        let bytes = encode(&generated_node(count, &scattered_edges(count)));
        let layout = CanonicalDagNodeView::parse(&bytes)
            .expect("a generated node parses")
            .children_layout();
        let mut longer = bytes.clone();
        longer.push(0);
        let shorter = &bytes[..bytes.len() - 1];
        assert!(matches!(
            layout.children(&longer),
            Err(CanonicalFormatError::TrailingBytes { .. })
        ));
        assert!(matches!(layout.node(&longer), Err(CanonicalFormatError::TrailingBytes { .. })));
        assert!(matches!(layout.children(shorter), Err(CanonicalFormatError::Truncated { .. })));
        assert!(matches!(layout.node(shorter), Err(CanonicalFormatError::Truncated { .. })));
        assert!(matches!(layout.children(&[]), Err(CanonicalFormatError::Truncated { .. })));
        if bytes.len() != empty.len() {
            assert!(layout.children(&empty).is_err());
            assert!(layout.node(&empty).is_err());
        }
    }
}

#[test]
fn only_a_validating_parse_is_counted_and_a_rebuild_is_not() {
    let bytes = encode(&generated_node(2, &scattered_edges(49)));
    reset_validating_node_parses();
    assert_eq!(validating_node_parses(), 0);
    let parsed = CanonicalDagNodeView::parse(&bytes).expect("a generated node parses");
    assert_eq!(validating_node_parses(), 1);
    CanonicalObjectView::parse(&bytes).expect("a generated node parses as an object");
    assert_eq!(validating_node_parses(), 2);
    assert!(CanonicalDagNodeView::parse(&bytes[..bytes.len() - 1]).is_err());
    assert_eq!(validating_node_parses(), 3, "a refused parse is counted too");

    let layout = parsed.children_layout();
    let node = layout.node(&bytes).expect("the node view is rebuilt");
    let children = layout
        .children(&bytes)
        .expect("the children view is rebuilt");
    let mut position = children.highest();
    let mut taken = 0usize;
    while let Some(current) = position {
        assert!(children.at(current).is_some());
        position = children.below(current);
        taken += 1;
    }
    assert_eq!((taken, node.children().len()), (49, 49));
    assert_eq!(validating_node_parses(), 3, "a rebuild and a resume validate nothing");
    reset_validating_node_parses();
    assert_eq!(validating_node_parses(), 0);
}
