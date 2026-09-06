use std::collections::BTreeMap;
use std::num::NonZeroU64;
use std::sync::{Arc, Mutex};

use crate::rspace::persistence::{
    CanonicalDagNode, CanonicalEncode, CanonicalFormatError, CanonicalObjectStore, DagChild,
    EndpointValue, ImmutableObjectStore, MutableObjectStore, ObjectAdmission, ObjectDigest,
    ObjectRefV1, SemanticProfile, SemanticProfileId, SemanticRoots, SemanticStateManifestV1,
    semantic_profile_seal,
};

pub(super) const TEST_SEMANTIC_PROFILE_ID: SemanticProfileId =
    SemanticProfileId::from_array(*b"RSpace.Test.V1\0\0");

pub(super) struct TestSemanticProfile;

pub(super) struct PersistenceTrialDir(std::path::PathBuf);

impl PersistenceTrialDir {
    pub(super) fn path(&self) -> &std::path::Path { &self.0 }
}

impl Drop for PersistenceTrialDir {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if !std::thread::panicking() {
                panic!("cannot remove persistence trial {}: {error}", self.0.display());
            }
        }
    }
}

pub(super) fn trial_scratch(prefix: &str) -> PersistenceTrialDir {
    static ROOT: std::sync::OnceLock<shared::rust::test_scratch::ProcessScratchDir> =
        std::sync::OnceLock::new();
    let root =
        ROOT.get_or_init(|| shared::rust::test_scratch::acquire("rspace-persistence-trials-"));
    let path = root
        .path()
        .join(format!("{prefix}{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&path).expect("create isolated persistence trial");
    PersistenceTrialDir(path)
}

#[test]
fn trial_scratch_does_not_accumulate_case_directories_or_process_locks() {
    for _ in 0..1100 {
        let trial = trial_scratch("lifecycle-");
        let path = trial.path().to_path_buf();
        std::fs::write(path.join("case"), b"trial-local").unwrap();
        drop(trial);
        assert!(!path.exists());
    }
}

impl semantic_profile_seal::Sealed for TestSemanticProfile {}

impl SemanticProfile for TestSemanticProfile {
    type Channel = String;
    type Pattern = String;
    type Datum = String;
    type Continuation = String;

    const ID: SemanticProfileId = TEST_SEMANTIC_PROFILE_ID;
}

#[derive(Default)]
pub(super) struct TracingStore {
    pub(super) inner: CanonicalObjectStore,
    reads: Mutex<BTreeMap<ObjectDigest, u64>>,
}

impl TracingStore {
    pub(super) fn clear_reads(&self) { self.reads.lock().unwrap().clear(); }

    pub(super) fn read_count(&self, digest: ObjectDigest) -> u64 {
        self.reads
            .lock()
            .unwrap()
            .get(&digest)
            .copied()
            .unwrap_or(0)
    }

    pub(super) fn total_reads(&self) -> u64 { self.reads.lock().unwrap().values().copied().sum() }
}

impl ImmutableObjectStore for TracingStore {
    type Error = CanonicalFormatError;
    type Object = Arc<[u8]>;

    fn get_bounded(
        &self,
        digest: &ObjectDigest,
        max_encoded_len: u64,
    ) -> Result<Option<Self::Object>, Self::Error> {
        let mut reads = self.reads.lock().unwrap();
        *reads.entry(*digest).or_default() += 1;
        drop(reads);
        self.inner.get_bounded(digest, max_encoded_len)
    }
}

impl MutableObjectStore for TracingStore {
    fn put_if_absent_or_equal(
        &self,
        reference: ObjectRefV1,
        canonical_bytes: Arc<[u8]>,
    ) -> Result<ObjectAdmission, Self::Error> {
        self.inner
            .put_if_absent_or_equal(reference, canonical_bytes)
    }
}

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
    SemanticStateManifestV1::new(1, TEST_SEMANTIC_PROFILE_ID, roots, None)
        .expect("schema-one manifest is valid")
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
