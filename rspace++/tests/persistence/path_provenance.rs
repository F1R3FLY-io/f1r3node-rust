use std::collections::BTreeSet;
use std::sync::Arc;

use super::support::{TEST_SEMANTIC_PROFILE_ID, admit, encode, nonzero, synthetic_ref};
use crate::rspace::persistence::{
    CanonicalDagNode, CanonicalFormatError, CanonicalObjectStore, ClosureError, DagChild,
    EndpointValue, ExactPath, ExactPathArena, IncrementalClosureLimits, InheritedBoundaryKind,
    InheritedObjectRef, RewriteProvenance, SemanticNamespace, SemanticRoots,
    SemanticStateManifestV1, StagingObjectStore, verify_incremental_closure,
};

fn extend(arena: &mut ExactPathArena, path: &mut ExactPath, bytes: &[u8]) {
    arena
        .extend(path, bytes, u64::MAX)
        .expect("the exact test path extends");
}

fn stage<T: crate::rspace::persistence::CanonicalEncode>(
    store: &StagingObjectStore<'_, CanonicalObjectStore>,
    value: &T,
) -> crate::rspace::persistence::ObjectRefV1 {
    store
        .stage(Arc::<[u8]>::from(encode(value)))
        .expect("the canonical target object stages")
        .0
}

fn target_with_missing_child(
    child_edge: u8,
) -> (CanonicalObjectStore, crate::rspace::persistence::ObjectRefV1, CanonicalDagNode) {
    let base = CanonicalObjectStore::new();
    let inherited_leaf = admit(
        &base,
        &CanonicalDagNode::new(
            Box::<[u8]>::default(),
            EndpointValue::Multiplicity(nonzero(9)),
            Box::<[DagChild]>::default(),
        )
        .expect("the inherited leaf is canonical"),
    );
    let target = CanonicalDagNode::new(
        b"root".as_slice(),
        EndpointValue::Multiplicity(nonzero(1)),
        vec![DagChild {
            edge: child_edge,
            target: inherited_leaf,
        }]
        .into_boxed_slice(),
    )
    .expect("the target root is canonical");
    (base, inherited_leaf, target)
}

fn verify_target(
    base: &CanonicalObjectStore,
    data_root: &CanonicalDagNode,
    provenance: &mut RewriteProvenance,
) -> Result<crate::rspace::persistence::StructuralClosureReport, String> {
    let staging = StagingObjectStore::new(base);
    let data_root = stage(&staging, data_root);
    let empty = stage(&staging, &CanonicalDagNode::empty());
    let manifest = SemanticStateManifestV1::new(
        1,
        TEST_SEMANTIC_PROFILE_ID,
        SemanticRoots {
            data_bag: data_root,
            continuation_map: empty,
            join_bag: empty,
        },
        None,
    )
    .expect("the target manifest is canonical");
    let manifest = stage(&staging, &manifest);
    verify_incremental_closure(&staging, manifest, provenance, IncrementalClosureLimits::UNBOUNDED)
        .map_err(|error| error.to_string())
}

#[test]
fn inherited_reuse_requires_the_exact_absolute_path() {
    let (base, inherited_leaf, target_at_b) = target_with_missing_child(b'b');
    let mut provenance = RewriteProvenance::new(u64::MAX);
    let mut path_at_a = ExactPath::empty();
    extend(&mut provenance.paths, &mut path_at_a, b"root");
    extend(&mut provenance.paths, &mut path_at_a, b"a");
    provenance
        .admit_inherited(InheritedObjectRef {
            namespace: SemanticNamespace::DataBag,
            kind: InheritedBoundaryKind::NodeEntry,
            path: path_at_a,
            reference: inherited_leaf,
        })
        .expect("the exact base boundary is admitted");

    let error = verify_target(&base, &target_at_b, &mut provenance)
        .expect_err("the same object at a different path must not inherit authority");
    assert!(error.contains("missing canonical object"), "{error}");
}

#[test]
fn inherited_reuse_accepts_the_same_exact_path() {
    let (base, inherited_leaf, target_at_a) = target_with_missing_child(b'a');
    let mut provenance = RewriteProvenance::new(u64::MAX);
    let mut path_at_a = ExactPath::empty();
    extend(&mut provenance.paths, &mut path_at_a, b"ro");
    extend(&mut provenance.paths, &mut path_at_a, b"ot");
    extend(&mut provenance.paths, &mut path_at_a, b"a");
    provenance
        .admit_inherited(InheritedObjectRef {
            namespace: SemanticNamespace::DataBag,
            kind: InheritedBoundaryKind::NodeEntry,
            path: path_at_a,
            reference: inherited_leaf,
        })
        .expect("the exact base boundary is admitted");

    verify_target(&base, &target_at_a, &mut provenance)
        .expect("the same object at the exact same path inherits authority");
}

#[test]
fn inherited_boundary_budget_counts_eligible_and_inherited_as_one_union() {
    let mut provenance = RewriteProvenance::new(1);
    provenance
        .admit_inherited(InheritedObjectRef {
            namespace: SemanticNamespace::DataBag,
            kind: InheritedBoundaryKind::NodeEntry,
            path: ExactPath::empty(),
            reference: synthetic_ref(0x41),
        })
        .expect("the first boundary consumes the complete budget");

    assert!(matches!(
        provenance.admit_inherited(InheritedObjectRef {
            namespace: SemanticNamespace::JoinBag,
            kind: InheritedBoundaryKind::NodeEntry,
            path: ExactPath::empty(),
            reference: synthetic_ref(0x42),
        }),
        Err(CanonicalFormatError::ResourceLimit {
            resource: "inherited boundaries",
            limit: 1,
            attempted: 2,
        })
    ));
}

#[test]
fn fixed_chunk_identity_is_independent_of_prefix_segmentation() {
    let mut arena = ExactPathArena::new();
    let bytes = (0_u8..=255).cycle().take(4_097).collect::<Vec<_>>();
    let mut single = ExactPath::empty();
    extend(&mut arena, &mut single, &bytes);

    let mut segmented = ExactPath::empty();
    for segment in bytes.chunks(37) {
        extend(&mut arena, &mut segmented, segment);
    }
    assert_eq!(single, segmented);
}

#[test]
fn a_one_byte_difference_after_a_deep_shared_prefix_remains_distinct() {
    let mut arena = ExactPathArena::new();
    let prefix = vec![0x5a; 100_000];
    let mut left = ExactPath::empty();
    extend(&mut arena, &mut left, &prefix);
    let mut right = left;
    extend(&mut arena, &mut left, &[0]);
    extend(&mut arena, &mut right, &[1]);
    assert_ne!(left, right);
}

#[test]
fn shared_prefix_chunks_are_not_duplicated_per_boundary() {
    let mut arena = ExactPathArena::new();
    let prefix = vec![0x33; 64 * 100];
    let mut shared = ExactPath::empty();
    extend(&mut arena, &mut shared, &prefix);
    assert_eq!(arena.chunk_count(), 100);

    let mut siblings = BTreeSet::new();
    for edge in 0_u8..=255 {
        let mut sibling = shared;
        extend(&mut arena, &mut sibling, &[edge]);
        siblings.insert(sibling);
    }
    assert_eq!(siblings.len(), 256);
    assert_eq!(arena.chunk_count(), 100);
}

#[test]
fn exact_path_chunk_cap_failure_is_atomic_and_retryable() {
    let mut arena = ExactPathArena::new();
    let mut path = ExactPath::empty();
    let original = path;
    let error = arena
        .extend(&mut path, &[0xaa; 128], 1)
        .expect_err("the second distinct chunk exceeds the cap");
    assert!(matches!(error, CanonicalFormatError::ResourceLimit {
        resource: "exact path chunks",
        limit: 1,
        attempted: 2,
    }));
    assert_eq!(arena.chunk_count(), 0);
    assert_eq!(path, original);

    arena
        .extend(&mut path, &[0xaa; 64], 1)
        .expect("the exact same arena remains usable after refusal");
    assert_eq!(arena.chunk_count(), 1);
}

#[test]
fn staged_shared_nodes_are_revalidated_at_each_logical_path() {
    let base = CanonicalObjectStore::new();
    let inherited_leaf = admit(
        &base,
        &CanonicalDagNode::new(
            Box::<[u8]>::default(),
            EndpointValue::Multiplicity(nonzero(7)),
            Box::<[DagChild]>::default(),
        )
        .unwrap(),
    );
    let staging = StagingObjectStore::new(&base);
    let shared = stage(
        &staging,
        &CanonicalDagNode::new(
            b"mid".as_slice(),
            EndpointValue::Multiplicity(nonzero(2)),
            vec![DagChild {
                edge: b'x',
                target: inherited_leaf,
            }]
            .into_boxed_slice(),
        )
        .unwrap(),
    );
    let root = stage(
        &staging,
        &CanonicalDagNode::new(
            b"root".as_slice(),
            EndpointValue::Multiplicity(nonzero(1)),
            vec![
                DagChild {
                    edge: b'a',
                    target: shared,
                },
                DagChild {
                    edge: b'b',
                    target: shared,
                },
            ]
            .into_boxed_slice(),
        )
        .unwrap(),
    );
    let empty = stage(&staging, &CanonicalDagNode::empty());
    let manifest = stage(
        &staging,
        &SemanticStateManifestV1::new(
            1,
            TEST_SEMANTIC_PROFILE_ID,
            SemanticRoots {
                data_bag: root,
                continuation_map: empty,
                join_bag: empty,
            },
            None,
        )
        .unwrap(),
    );

    let mut provenance = RewriteProvenance::new(u64::MAX);
    let mut only_a = ExactPath::empty();
    for segment in [b"root".as_slice(), b"a", b"mid", b"x"] {
        extend(&mut provenance.paths, &mut only_a, segment);
    }
    provenance
        .admit_inherited(InheritedObjectRef {
            namespace: SemanticNamespace::DataBag,
            kind: InheritedBoundaryKind::NodeEntry,
            path: only_a,
            reference: inherited_leaf,
        })
        .unwrap();

    let error = verify_incremental_closure(
        &staging,
        manifest,
        &mut provenance,
        IncrementalClosureLimits::UNBOUNDED,
    )
    .expect_err("the shared node's uncertified second logical path must be visited");
    assert!(matches!(
        error,
        ClosureError::Format(CanonicalFormatError::MissingObject(digest))
            if digest == inherited_leaf.digest()
    ));
}

#[test]
fn incremental_logical_visits_are_explicitly_bounded() {
    let (base, _inherited_leaf, target) = target_with_missing_child(b'a');
    let staging = StagingObjectStore::new(&base);
    let data_root = stage(&staging, &target);
    let empty = stage(&staging, &CanonicalDagNode::empty());
    let manifest = stage(
        &staging,
        &SemanticStateManifestV1::new(
            1,
            TEST_SEMANTIC_PROFILE_ID,
            SemanticRoots {
                data_bag: data_root,
                continuation_map: empty,
                join_bag: empty,
            },
            None,
        )
        .unwrap(),
    );
    let mut provenance = RewriteProvenance::new(u64::MAX);
    let error =
        verify_incremental_closure(&staging, manifest, &mut provenance, IncrementalClosureLimits {
            max_logical_visits: 1,
            ..IncrementalClosureLimits::UNBOUNDED
        })
        .expect_err("the second logical visit exceeds the explicit work budget");
    assert!(matches!(
        error,
        ClosureError::Format(CanonicalFormatError::ResourceLimit {
            resource: "incremental logical visits",
            limit: 1,
            attempted: 2,
        })
    ));
}

#[test]
fn incremental_exact_path_chunks_are_explicitly_bounded() {
    let base = CanonicalObjectStore::new();
    let staging = StagingObjectStore::new(&base);
    let data_root = stage(
        &staging,
        &CanonicalDagNode::new(
            vec![b'p'; 65],
            EndpointValue::Multiplicity(nonzero(1)),
            Box::<[DagChild]>::default(),
        )
        .unwrap(),
    );
    let empty = stage(&staging, &CanonicalDagNode::empty());
    let manifest = stage(
        &staging,
        &SemanticStateManifestV1::new(
            1,
            TEST_SEMANTIC_PROFILE_ID,
            SemanticRoots {
                data_bag: data_root,
                continuation_map: empty,
                join_bag: empty,
            },
            None,
        )
        .unwrap(),
    );
    let mut provenance = RewriteProvenance::new(u64::MAX);
    let error =
        verify_incremental_closure(&staging, manifest, &mut provenance, IncrementalClosureLimits {
            max_exact_path_chunks: 0,
            ..IncrementalClosureLimits::UNBOUNDED
        })
        .expect_err("the first distinct exact-path chunk exceeds the explicit retention budget");
    assert!(matches!(
        error,
        ClosureError::Format(CanonicalFormatError::ResourceLimit {
            resource: "exact path chunks",
            limit: 0,
            attempted: 1,
        })
    ));
}
