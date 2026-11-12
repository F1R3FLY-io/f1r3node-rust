use std::sync::Arc;

use super::support::encode;
use crate::rspace::persistence::{
    CanonicalDagNode, CanonicalFormatError, CanonicalObjectStore, MutableObjectStore,
    ObjectAdmission, ObjectDigest, ObjectRefV1,
};

#[test]
fn equal_canonical_bytes_are_idempotent() {
    let store = CanonicalObjectStore::new();
    let bytes: Arc<[u8]> = encode(&CanonicalDagNode::empty()).into();
    let (reference, first) = store
        .admit(Arc::clone(&bytes))
        .expect("first admission succeeds");
    assert_eq!(first, ObjectAdmission::Inserted);
    assert_eq!(
        store
            .put_if_absent_or_equal(reference, bytes)
            .expect("equal repeat succeeds"),
        ObjectAdmission::AlreadyPresent
    );
    assert_eq!(store.len().expect("store lock is healthy"), 1);
}

#[test]
fn a_forced_same_digest_with_different_bytes_is_fatal() {
    let store = CanonicalObjectStore::new();
    let digest = ObjectDigest::from_array([11; 32]);
    assert_eq!(
        store
            .put_prehashed_for_test(digest, Arc::from(&b"first"[..]))
            .expect("first forced object is inserted"),
        ObjectAdmission::Inserted
    );
    assert_eq!(
        store
            .put_prehashed_for_test(digest, Arc::from(&b"first"[..]))
            .expect("equal forced object is idempotent"),
        ObjectAdmission::AlreadyPresent
    );
    assert_eq!(
        store
            .put_prehashed_for_test(digest, Arc::from(&b"second"[..]))
            .expect_err("different bytes at one digest are rejected"),
        CanonicalFormatError::ContentCollision(digest)
    );
}

#[test]
fn declared_length_and_digest_are_verified_before_admission() {
    let store = CanonicalObjectStore::new();
    let bytes: Arc<[u8]> = encode(&CanonicalDagNode::empty()).into();
    let digest = ObjectDigest::of_canonical_bytes(&bytes);
    let wrong_len = ObjectRefV1::new(digest, bytes.len() as u64 + 1)
        .expect("wrong length still includes an envelope");
    assert!(matches!(
        store.put_if_absent_or_equal(wrong_len, Arc::clone(&bytes)),
        Err(CanonicalFormatError::ObjectLengthMismatch { .. })
    ));

    let wrong_digest = ObjectRefV1::new(ObjectDigest::from_array([12; 32]), bytes.len() as u64)
        .expect("reference is structurally valid");
    assert!(matches!(
        store.put_if_absent_or_equal(wrong_digest, bytes),
        Err(CanonicalFormatError::DigestMismatch { .. })
    ));
    assert!(store.is_empty().expect("store lock is healthy"));
}
