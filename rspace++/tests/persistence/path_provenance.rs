use std::collections::BTreeSet;
use std::sync::Arc;

use super::support::{TEST_SEMANTIC_PROFILE_ID, admit, encode, nonzero, synthetic_ref};
use crate::rspace::persistence::{
    CanonicalBlob, CanonicalBlobKind, CanonicalDagNode, CanonicalFormatError, CanonicalObjectStore,
    ClosureError, DagChild, EndpointValue, ExactPath, ExactPathArena, IncrementalClosureLimits,
    InheritedBoundaryKind, InheritedObjectRef, RewriteProvenance, SemanticNamespace, SemanticRoots,
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

fn continuation_source(payload: &[u8]) -> CanonicalBlob {
    CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, payload)
        .expect("the continuation source is canonical")
}

fn continuation_leaf(source: crate::rspace::persistence::ObjectRefV1) -> CanonicalDagNode {
    CanonicalDagNode::new(
        Box::<[u8]>::default(),
        EndpointValue::Content(source),
        Box::<[DagChild]>::default(),
    )
    .expect("the continuation leaf is canonical")
}

fn branch(children: Vec<DagChild>) -> CanonicalDagNode {
    CanonicalDagNode::new(Box::<[u8]>::default(), EndpointValue::Absent, children)
        .expect("the branch is canonical")
}

fn stage_manifest(
    staging: &StagingObjectStore<'_, CanonicalObjectStore>,
    roots: impl FnOnce(crate::rspace::persistence::ObjectRefV1) -> SemanticRoots,
) -> crate::rspace::persistence::ObjectRefV1 {
    let empty = stage(staging, &CanonicalDagNode::empty());
    stage(
        staging,
        &SemanticStateManifestV1::new(1, TEST_SEMANTIC_PROFILE_ID, roots(empty), None)
            .expect("the target manifest is canonical"),
    )
}

fn certify(
    provenance: &mut RewriteProvenance,
    namespace: SemanticNamespace,
    kind: InheritedBoundaryKind,
    path_bytes: &[u8],
    reference: crate::rspace::persistence::ObjectRefV1,
) {
    let mut path = ExactPath::empty();
    extend(&mut provenance.paths, &mut path, path_bytes);
    provenance
        .admit_inherited(InheritedObjectRef {
            namespace,
            kind,
            path,
            reference,
        })
        .expect("the exact base boundary is admitted");
}

struct CoincidenceFixture<'base> {
    staging: StagingObjectStore<'base, CanonicalObjectStore>,
    manifest: crate::rspace::persistence::ObjectRefV1,
    inherited_source: crate::rspace::persistence::ObjectRefV1,
    inherited_leaf: crate::rspace::persistence::ObjectRefV1,
    sibling_leaf: crate::rspace::persistence::ObjectRefV1,
}

fn coincidence_fixture(base: &CanonicalObjectStore) -> CoincidenceFixture<'_> {
    let inherited_source = admit(base, &continuation_source(b"inherited source"));
    let shared_leaf = continuation_leaf(inherited_source);
    let inherited_leaf = admit(base, &shared_leaf);

    let staging = StagingObjectStore::new(base);
    let emitted_leaf = stage(&staging, &shared_leaf);
    assert_eq!(emitted_leaf, inherited_leaf);
    let emitted_source = stage(&staging, &continuation_source(b"emitted source"));
    let sibling_leaf = stage(&staging, &continuation_leaf(emitted_source));
    let emitted_parent = stage(
        &staging,
        &branch(vec![
            DagChild {
                edge: 3,
                target: emitted_leaf,
            },
            DagChild {
                edge: 4,
                target: sibling_leaf,
            },
        ]),
    );
    let root = stage(
        &staging,
        &branch(vec![
            DagChild {
                edge: 1,
                target: inherited_leaf,
            },
            DagChild {
                edge: 2,
                target: emitted_parent,
            },
        ]),
    );
    let manifest = stage_manifest(&staging, |empty| SemanticRoots {
        data_bag: empty,
        continuation_map: root,
        join_bag: empty,
    });
    assert!(
        staging
            .get_staged_bounded(&inherited_leaf.digest(), inherited_leaf.encoded_len())
            .expect("the staging overlay is readable")
            .is_some()
    );
    assert!(
        staging
            .get_staged_bounded(&inherited_source.digest(), inherited_source.encoded_len())
            .expect("the staging overlay is readable")
            .is_none()
    );
    CoincidenceFixture {
        staging,
        manifest,
        inherited_source,
        inherited_leaf,
        sibling_leaf,
    }
}

fn coincidence_provenance(
    fixture: &CoincidenceFixture<'_>,
    leaf_boundary: Option<(
        SemanticNamespace,
        InheritedBoundaryKind,
        &[u8],
        crate::rspace::persistence::ObjectRefV1,
    )>,
) -> RewriteProvenance {
    let mut provenance = RewriteProvenance::new(u64::MAX);
    if let Some((namespace, kind, path_bytes, reference)) = leaf_boundary {
        certify(&mut provenance, namespace, kind, path_bytes, reference);
    }
    certify(
        &mut provenance,
        SemanticNamespace::ContinuationMap,
        InheritedBoundaryKind::ContentAtEndpoint,
        &[2, 3],
        fixture.inherited_source,
    );
    provenance
}

#[test]
fn a_staged_node_equal_to_an_inherited_subtree_stops_at_its_exact_certificate() {
    let base = CanonicalObjectStore::new();
    let fixture = coincidence_fixture(&base);
    let exact = || {
        coincidence_provenance(
            &fixture,
            Some((
                SemanticNamespace::ContinuationMap,
                InheritedBoundaryKind::NodeEntry,
                &[1],
                fixture.inherited_leaf,
            )),
        )
    };

    let report = verify_incremental_closure(
        &fixture.staging,
        fixture.manifest,
        &mut exact(),
        IncrementalClosureLimits::UNBOUNDED,
    )
    .expect("the inherited occurrence stops at its certificate and the emitted one is verified");
    assert_eq!(report.distinct_objects, 7);
    assert_eq!(report.node_objects, 5);
    assert_eq!(report.payload_objects, 1);
    assert_eq!(report.references, 9);

    verify_incremental_closure(
        &fixture.staging,
        fixture.manifest,
        &mut exact(),
        IncrementalClosureLimits {
            max_logical_visits: 10,
            ..IncrementalClosureLimits::UNBOUNDED
        },
    )
    .expect("the model's eight visits and the two empty roots fit ten logical visits");
    let error = verify_incremental_closure(
        &fixture.staging,
        fixture.manifest,
        &mut exact(),
        IncrementalClosureLimits {
            max_logical_visits: 9,
            ..IncrementalClosureLimits::UNBOUNDED
        },
    )
    .expect_err("the tenth logical visit exceeds a nine-visit budget");
    assert!(matches!(
        error,
        ClosureError::Format(CanonicalFormatError::ResourceLimit {
            resource: "incremental logical visits",
            limit: 9,
            attempted: 10,
        })
    ));
}

#[test]
fn an_inexact_boundary_does_not_stop_a_coincident_staged_node() {
    let base = CanonicalObjectStore::new();
    let fixture = coincidence_fixture(&base);
    let rows: [(
        &str,
        Option<(
            SemanticNamespace,
            InheritedBoundaryKind,
            &[u8],
            crate::rspace::persistence::ObjectRefV1,
        )>,
    ); 5] = [
        ("absent", None),
        (
            "path",
            Some((
                SemanticNamespace::ContinuationMap,
                InheritedBoundaryKind::NodeEntry,
                &[1, 0],
                fixture.inherited_leaf,
            )),
        ),
        (
            "namespace",
            Some((
                SemanticNamespace::DataBag,
                InheritedBoundaryKind::NodeEntry,
                &[1],
                fixture.inherited_leaf,
            )),
        ),
        (
            "kind",
            Some((
                SemanticNamespace::ContinuationMap,
                InheritedBoundaryKind::ContentAtEndpoint,
                &[1],
                fixture.inherited_leaf,
            )),
        ),
        (
            "reference",
            Some((
                SemanticNamespace::ContinuationMap,
                InheritedBoundaryKind::NodeEntry,
                &[1],
                fixture.sibling_leaf,
            )),
        ),
    ];
    for (row, leaf_boundary) in rows {
        let error = verify_incremental_closure(
            &fixture.staging,
            fixture.manifest,
            &mut coincidence_provenance(&fixture, leaf_boundary),
            IncrementalClosureLimits::UNBOUNDED,
        )
        .expect_err("only the exact boundary stops the walk at the coincident node");
        assert!(
            matches!(
                &error,
                ClosureError::Format(CanonicalFormatError::MissingObject(digest))
                    if *digest == fixture.inherited_source.digest()
            ),
            "{row}: {error}"
        );
    }
}

#[test]
fn an_object_staged_only_where_it_is_inherited_is_outside_the_delta() {
    let base = CanonicalObjectStore::new();
    let leaf = CanonicalDagNode::new(
        Box::<[u8]>::default(),
        EndpointValue::Multiplicity(nonzero(9)),
        Box::<[DagChild]>::default(),
    )
    .expect("the inherited leaf is canonical");
    let inherited_leaf = admit(&base, &leaf);

    let staging = StagingObjectStore::new(&base);
    assert_eq!(stage(&staging, &leaf), inherited_leaf);
    let root = stage(
        &staging,
        &CanonicalDagNode::new(b"root".as_slice(), EndpointValue::Multiplicity(nonzero(1)), vec![
            DagChild {
                edge: b'a',
                target: inherited_leaf,
            },
        ])
        .expect("the target root is canonical"),
    );
    let manifest = stage_manifest(&staging, |empty| SemanticRoots {
        data_bag: root,
        continuation_map: empty,
        join_bag: empty,
    });

    let mut provenance = RewriteProvenance::new(u64::MAX);
    certify(
        &mut provenance,
        SemanticNamespace::DataBag,
        InheritedBoundaryKind::NodeEntry,
        b"roota",
        inherited_leaf,
    );

    let error = verify_incremental_closure(
        &staging,
        manifest,
        &mut provenance,
        IncrementalClosureLimits::UNBOUNDED,
    )
    .expect_err("an object reached only through its certificate is not part of the delta");
    assert!(matches!(
        error,
        ClosureError::Format(CanonicalFormatError::SemanticCoherence { ref detail })
            if detail == "transaction staged an object outside the target delta closure"
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
