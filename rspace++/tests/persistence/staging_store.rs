use std::sync::Arc;

use super::support::{admit, encode};
use crate::rspace::persistence::{
    CanonicalDagNode, CanonicalFormatError, CanonicalObjectStore, ImmutableObjectStore,
    MutableObjectStore, ObjectAdmission, ObjectDigest, ObjectRefV1, StagingObjectStore,
    StagingStoreError,
};

#[test]
fn staged_objects_are_visible_through_the_overlay_but_not_the_base() {
    let base = CanonicalObjectStore::new();
    let overlay = StagingObjectStore::new(&base);
    let bytes: Arc<[u8]> = encode(&CanonicalDagNode::empty()).into();
    let (reference, admission) = overlay.stage(Arc::clone(&bytes)).expect("object stages");
    assert_eq!(admission, ObjectAdmission::Inserted);
    assert_eq!(overlay.get(&reference.digest()).unwrap().unwrap().as_ref(), bytes.as_ref());
    assert!(base.get(&reference.digest()).unwrap().is_none());
    assert_eq!(overlay.len().unwrap(), 1);
}

#[test]
fn equal_base_content_is_owned_by_the_private_staging_snapshot() {
    let base = CanonicalObjectStore::new();
    let reference = admit(&base, &CanonicalDagNode::empty());
    let bytes = base.get(&reference.digest()).unwrap().unwrap();
    let overlay = StagingObjectStore::new(&base);
    assert_eq!(overlay.stage(bytes).unwrap(), (reference, ObjectAdmission::Inserted));
    assert_eq!(overlay.len().unwrap(), 1);
    let snapshot = overlay.staged_snapshot().unwrap();
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].0, reference);
}

#[test]
fn staging_checks_declared_length_digest_and_full_byte_collisions() {
    let base = CanonicalObjectStore::new();
    let overlay = StagingObjectStore::new(&base);
    let bytes: Arc<[u8]> = encode(&CanonicalDagNode::empty()).into();
    let digest = ObjectDigest::of_canonical_bytes(&bytes);
    let wrong_length = ObjectRefV1::new(digest, bytes.len() as u64 + 1).unwrap();
    assert!(matches!(
        overlay.put_if_absent_or_equal(wrong_length, Arc::clone(&bytes)),
        Err(StagingStoreError::Format(CanonicalFormatError::ObjectLengthMismatch { .. }))
    ));

    let wrong_digest = ObjectRefV1::new(ObjectDigest::from_array([0x55; 32]), bytes.len() as u64)
        .expect("synthetic reference length is valid");
    assert!(matches!(
        overlay.put_if_absent_or_equal(wrong_digest, bytes),
        Err(StagingStoreError::Format(CanonicalFormatError::DigestMismatch { .. }))
    ));
}

#[test]
fn owned_staged_snapshot_is_digest_ordered() {
    let base = CanonicalObjectStore::new();
    let overlay = StagingObjectStore::new(&base);
    let first = CanonicalDagNode::new(
        &b"first"[..],
        crate::rspace::persistence::EndpointValue::Unit,
        Box::default(),
    )
    .unwrap();
    let second = CanonicalDagNode::new(
        &b"second"[..],
        crate::rspace::persistence::EndpointValue::Unit,
        Box::default(),
    )
    .unwrap();
    let _ = overlay.stage(encode(&second).into()).unwrap();
    let _ = overlay.stage(encode(&first).into()).unwrap();
    let digests = overlay
        .staged_snapshot()
        .unwrap()
        .iter()
        .map(|(reference, _)| reference.digest())
        .collect::<Vec<_>>();
    assert!(digests.windows(2).all(|pair| pair[0] < pair[1]));
}
