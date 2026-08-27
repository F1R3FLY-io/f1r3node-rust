use std::sync::Arc;

use crate::rspace::persistence::checkpoint::CheckpointObjectInventoryV1;
use crate::rspace::persistence::{
    CanonicalDagNode, CanonicalEncode, CanonicalFormatError, CanonicalObjectStore, ClosureError,
    LegacyHistoryRoot, ObjectDigest, ObjectRefV1, RetainedUnionClosureLimitsV1, SemanticProfileId,
    SemanticRoots, SemanticStateManifestV1, StructuralClosureLedgerV1, StructuralValidationLimits,
    verify_structural_closure_in_ledger_with_limits,
};

const PROFILE: SemanticProfileId = SemanticProfileId::from_array(*b"RSpace.Test.V1\0\0");

fn admit<T: CanonicalEncode>(store: &CanonicalObjectStore, object: &T) -> ObjectRefV1 {
    let bytes: Arc<[u8]> = object
        .try_canonical_bytes()
        .expect("test object encodes")
        .into();
    store.admit(bytes).expect("test object is admitted").0
}

fn shared_roots(root: ObjectRefV1) -> SemanticRoots {
    SemanticRoots {
        data_bag: root,
        continuation_map: root,
        join_bag: root,
    }
}

#[test]
fn retained_union_deduplicates_shared_objects_without_losing_per_state_inventories() {
    let store = CanonicalObjectStore::new();
    let empty = admit(&store, &CanonicalDagNode::empty());
    let roots = shared_roots(empty);
    let current = SemanticStateManifestV1::new(1, PROFILE, roots, None).unwrap();
    let historical = SemanticStateManifestV1::new(
        1,
        PROFILE,
        roots,
        Some(LegacyHistoryRoot::from_array([0xA5; 32])),
    )
    .unwrap();
    let current_ref = admit(&store, &current);
    let historical_ref = admit(&store, &historical);

    let mut ledger = StructuralClosureLedgerV1::new();
    let current_report = verify_structural_closure_in_ledger_with_limits(
        &store,
        current_ref,
        StructuralValidationLimits::unbounded(),
        &mut ledger,
        RetainedUnionClosureLimitsV1::UNBOUNDED,
    )
    .expect("current state is structurally admitted")
    .report();
    let historical_report = verify_structural_closure_in_ledger_with_limits(
        &store,
        historical_ref,
        StructuralValidationLimits::unbounded(),
        &mut ledger,
        RetainedUnionClosureLimitsV1::UNBOUNDED,
    )
    .expect("historical state is structurally admitted")
    .report();

    assert_eq!(current_report.distinct_objects, 2);
    assert_eq!(current_report.canonical_bytes, current_ref.encoded_len() + empty.encoded_len());
    assert_eq!(historical_report.distinct_objects, 2);
    assert_eq!(
        historical_report.canonical_bytes,
        historical_ref.encoded_len() + empty.encoded_len()
    );

    let union = ledger.retained_union_report();
    assert_eq!(union.distinct_objects, 3);
    assert_eq!(
        union.canonical_bytes,
        current_ref.encoded_len() + historical_ref.encoded_len() + empty.encoded_len()
    );

    let mut references = Vec::new();
    ledger
        .try_visit_retained_union(|reference| {
            references.push(reference);
            Ok::<(), std::convert::Infallible>(())
        })
        .unwrap();
    assert_eq!(references.len() as u64, union.distinct_objects);
    assert_eq!(
        references
            .iter()
            .map(|reference| reference.encoded_len())
            .sum::<u64>(),
        union.canonical_bytes
    );
    assert!(
        references
            .windows(2)
            .all(|pair| pair[0].digest() < pair[1].digest())
    );

    let retention = ObjectRefV1::new(ObjectDigest::from_array([0xFF; 32]), 64).unwrap();
    let checkpoint = CheckpointObjectInventoryV1::from_retained_union(&ledger, retention).unwrap();
    assert_eq!(checkpoint.retained_union_objects(), union.distinct_objects);
    assert_eq!(checkpoint.retained_union_canonical_bytes().get(), union.canonical_bytes);
    assert_eq!(checkpoint.data_objects(), union.distinct_objects + 1);
    assert_eq!(
        checkpoint.data_canonical_bytes().get(),
        union.canonical_bytes + retention.encoded_len()
    );
}

#[test]
fn shared_ledger_rejects_a_conflicting_declared_length_before_memoized_reuse() {
    let store = CanonicalObjectStore::new();
    let empty = admit(&store, &CanonicalDagNode::empty());
    let manifest = SemanticStateManifestV1::new(1, PROFILE, shared_roots(empty), None).unwrap();
    let manifest_ref = admit(&store, &manifest);
    let mut ledger = StructuralClosureLedgerV1::new();

    verify_structural_closure_in_ledger_with_limits(
        &store,
        manifest_ref,
        StructuralValidationLimits::unbounded(),
        &mut ledger,
        RetainedUnionClosureLimitsV1::UNBOUNDED,
    )
    .expect("first declaration is admitted");

    let conflicting =
        ObjectRefV1::new(manifest_ref.digest(), manifest_ref.encoded_len().checked_add(1).unwrap())
            .unwrap();
    assert!(matches!(
        verify_structural_closure_in_ledger_with_limits(
            &store,
            conflicting,
            StructuralValidationLimits::unbounded(),
            &mut ledger,
            RetainedUnionClosureLimitsV1::UNBOUNDED,
        ),
        Err(ClosureError::Format(CanonicalFormatError::ObjectLengthMismatch { .. }))
    ));
}
