use std::sync::Arc;

use super::support::{admit, empty_roots, manifest, nonzero, synthetic_ref};
use crate::rspace::persistence::{
    CanonicalBlob, CanonicalBlobKind, CanonicalDagNode, CanonicalFormatError, CanonicalObjectStore,
    ClosureError, DagChild, EndpointValue, ObjectRefV1, SemanticRoots, StructuralValidationLimits,
    verify_structural_closure, verify_structural_closure_with_limits,
};

#[test]
fn complete_empty_state_closure_is_verified_once_per_distinct_object() {
    let store = CanonicalObjectStore::new();
    let empty = admit(&store, &CanonicalDagNode::empty());
    let manifest_ref = admit(&store, &manifest(empty_roots(empty)));
    let report =
        verify_structural_closure(&store, manifest_ref).expect("complete empty closure is valid");
    assert_eq!(report.distinct_objects, 2);
    assert_eq!(report.node_objects, 1);
    assert_eq!(report.payload_objects, 0);
    assert_eq!(report.references, 3);
    assert_eq!(report.canonical_bytes, manifest_ref.encoded_len() + empty.encoded_len());
}

#[test]
fn structural_resource_budgets_bound_total_work_without_a_depth_cap() {
    let store = CanonicalObjectStore::new();
    let empty = admit(&store, &CanonicalDagNode::empty());
    let manifest_ref = admit(&store, &manifest(empty_roots(empty)));
    let limits = StructuralValidationLimits {
        max_distinct_objects: 1,
        ..StructuralValidationLimits::unbounded()
    };
    assert!(matches!(
        verify_structural_closure_with_limits(&store, manifest_ref, limits),
        Err(ClosureError::Format(CanonicalFormatError::ResourceLimit {
            resource: "distinct objects",
            limit: 1,
            attempted: 2,
        }))
    ));
}

#[test]
fn namespace_endpoint_policy_wrong_kind_and_missing_objects_fail_closed() {
    let store = CanonicalObjectStore::new();
    let empty = admit(&store, &CanonicalDagNode::empty());
    let unit = admit(
        &store,
        &CanonicalDagNode::new(&b"datum"[..], EndpointValue::Unit, Box::<[DagChild]>::default())
            .expect("unit leaf is canonical"),
    );
    let roots = SemanticRoots {
        data_bag: unit,
        ..empty_roots(empty)
    };
    let manifest_ref = admit(&store, &manifest(roots));
    assert!(matches!(
        verify_structural_closure(&store, manifest_ref),
        Err(ClosureError::Format(CanonicalFormatError::EndpointPolicy { .. }))
    ));

    let blob = admit(
        &store,
        &CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, &b"bytes"[..])
            .expect("blob is canonical"),
    );
    let wrong_kind = admit(
        &store,
        &manifest(SemanticRoots {
            data_bag: blob,
            ..empty_roots(empty)
        }),
    );
    assert!(matches!(
        verify_structural_closure(&store, wrong_kind),
        Err(ClosureError::Format(CanonicalFormatError::UnexpectedObjectKind { .. }))
    ));

    let missing = admit(
        &store,
        &manifest(SemanticRoots {
            data_bag: synthetic_ref(77),
            ..empty_roots(empty)
        }),
    );
    assert!(matches!(
        verify_structural_closure(&store, missing),
        Err(ClosureError::Format(CanonicalFormatError::MissingObject(_)))
    ));
}

#[test]
fn conflicting_declared_lengths_for_one_digest_never_hit_the_verified_fast_path() {
    let store = CanonicalObjectStore::new();
    let empty = admit(&store, &CanonicalDagNode::empty());
    let conflicting = ObjectRefV1::new(empty.digest(), empty.encoded_len() + 1)
        .expect("conflicting reference is structurally valid");
    let manifest_ref = admit(
        &store,
        &manifest(SemanticRoots {
            data_bag: empty,
            continuation_map: conflicting,
            join_bag: empty,
        }),
    );
    assert!(matches!(
        verify_structural_closure(&store, manifest_ref),
        Err(ClosureError::Format(CanonicalFormatError::ObjectLengthMismatch { .. }))
    ));
}

#[test]
fn continuation_content_resolves_to_the_only_semantic_blob_kind() {
    let store = CanonicalObjectStore::new();
    let empty = admit(&store, &CanonicalDagNode::empty());
    let source = admit(
        &store,
        &CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, &b"source"[..])
            .expect("source blob is canonical"),
    );
    let continuation_root = admit(
        &store,
        &CanonicalDagNode::new(
            &b"continuation-identity"[..],
            EndpointValue::Content(source),
            Box::<[DagChild]>::default(),
        )
        .expect("content leaf is canonical"),
    );
    let manifest_ref = admit(
        &store,
        &manifest(SemanticRoots {
            continuation_map: continuation_root,
            ..empty_roots(empty)
        }),
    );
    let report = verify_structural_closure(&store, manifest_ref)
        .expect("typed content closure is structurally valid");
    assert_eq!(report.payload_objects, 1);
}

#[test]
fn twenty_thousand_levels_validate_on_a_small_native_stack() {
    const DEPTH: usize = 20_000;
    let store = Arc::new(CanonicalObjectStore::new());
    let empty = admit(&store, &CanonicalDagNode::empty());
    let mut child = admit(
        &store,
        &CanonicalDagNode::new(
            &b"leaf"[..],
            EndpointValue::Multiplicity(nonzero(1)),
            Box::<[DagChild]>::default(),
        )
        .expect("leaf is canonical"),
    );
    for depth in 1..DEPTH {
        let edge = (depth % 251) as u8;
        child = admit(
            &store,
            &CanonicalDagNode::new(
                Box::<[u8]>::default(),
                EndpointValue::Multiplicity(nonzero(1)),
                vec![DagChild {
                    edge,
                    target: child,
                }]
                .into_boxed_slice(),
            )
            .expect("endpoint-bearing unary node is canonical"),
        );
    }
    let manifest_ref = admit(
        &store,
        &manifest(SemanticRoots {
            data_bag: child,
            ..empty_roots(empty)
        }),
    );

    let verifier_store = Arc::clone(&store);
    let report = std::thread::Builder::new()
        .name("persistent-dag-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(move || verify_structural_closure(verifier_store.as_ref(), manifest_ref))
        .expect("small-stack verifier thread starts")
        .join()
        .expect("small-stack verifier does not panic")
        .expect("deep closure is valid");
    assert_eq!(report.node_objects, DEPTH as u64 + 1);
    assert_eq!(report.distinct_objects, DEPTH as u64 + 2);
    assert!(report.maximum_worklist >= DEPTH as u64);
}

#[test]
fn multiplicity_roots_accept_nonzero_values_without_flattening_bags() {
    let store = CanonicalObjectStore::new();
    let empty = admit(&store, &CanonicalDagNode::empty());
    let data = admit(
        &store,
        &CanonicalDagNode::new(
            &b"datum"[..],
            EndpointValue::Multiplicity(nonzero(u64::MAX)),
            Box::<[DagChild]>::default(),
        )
        .expect("maximum multiplicity is canonical"),
    );
    let manifest_ref = admit(
        &store,
        &manifest(SemanticRoots {
            data_bag: data,
            ..empty_roots(empty)
        }),
    );
    verify_structural_closure(&store, manifest_ref).expect("nonzero multiplicity closure is valid");
}
