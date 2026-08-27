use std::num::NonZeroU64;
use std::sync::Arc;

use crate::rspace::persistence::{
    CanonicalDagNode, CanonicalEncode, CanonicalFormatError, CanonicalObjectStore, DagChild,
    EndpointValue, ObjectAdmission, ObjectDigest, ObjectRefV1, SemanticRoots,
    SemanticStateManifestV1,
};

pub(super) fn nonzero(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).expect("test multiplicities are nonzero")
}

pub(super) fn synthetic_ref(seed: u8) -> ObjectRefV1 {
    ObjectRefV1::new(ObjectDigest::from_array([seed; 32]), 50)
        .expect("the synthetic object length includes an envelope")
}

pub(super) fn encode<T: CanonicalEncode>(value: &T) -> Vec<u8> {
    value
        .try_canonical_bytes()
        .expect("test canonical value encodes")
}

pub(super) fn admit<T: CanonicalEncode>(store: &CanonicalObjectStore, value: &T) -> ObjectRefV1 {
    let bytes: Arc<[u8]> = encode(value).into();
    let (reference, admission) = store.admit(bytes).expect("test object is admitted");
    assert_eq!(admission, ObjectAdmission::Inserted);
    reference
}

pub(super) fn empty_roots(root: ObjectRefV1) -> SemanticRoots {
    SemanticRoots {
        data_bag: root,
        continuation_map: root,
        join_bag: root,
    }
}

pub(super) fn manifest(roots: SemanticRoots) -> SemanticStateManifestV1 {
    SemanticStateManifestV1::new(1, roots, None).expect("schema-one manifest is valid")
}

pub(super) fn children(count: usize) -> Box<[DagChild]> {
    (0..count)
        .map(|edge| DagChild {
            edge: u8::try_from(edge).expect("test fanout fits one byte"),
            target: synthetic_ref(edge as u8),
        })
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

pub(super) fn node(
    prefix: &[u8],
    endpoint: EndpointValue,
    fanout: usize,
) -> Result<CanonicalDagNode, CanonicalFormatError> {
    CanonicalDagNode::new(prefix, endpoint, children(fanout))
}
