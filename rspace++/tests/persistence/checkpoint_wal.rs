use std::collections::{BTreeSet, HashMap};
use std::convert::Infallible;

use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};

use super::property_support::independent_inventory_digest;
use super::retention_v2_format_properties::{
    migrate_checkpoint_source_to_v2, v2_source_from_parts,
};
use super::support::{TestSemanticProfile, nonzero, synthetic_ref};
use crate::rspace::data_index::RSpaceDataIndex;
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::checkpoint::{
    CheckpointObjectInventoryV1, CheckpointReferenceVisitErrorV1, GENERATION_CHECKPOINT_DOMAIN,
    GenerationCheckpointDescriptorV1, GenerationCheckpointDescriptorViewV1,
    GenerationCheckpointErrorV1, OrderedObjectInventoryBuilderV1,
};
use crate::rspace::persistence::closure::{RetainedUnionClosureReportV1, StructuralClosureReport};
use crate::rspace::persistence::durable::{
    INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, IndexGenerationV1, ObjectIndexHeaderV1,
    PACK_HEADER_LEN_V1, PreparedGenerationCheckpointIndexV1,
    PreparedGenerationCheckpointIndexVisitErrorV1,
};
use crate::rspace::persistence::retention::v2_format::{
    EMPTY_RETENTION_RADIX_NODE, GENERATION_CHECKPOINT_V2_DOMAIN, RETENTION_ANCHOR_CANONICAL_LEN,
    RETENTION_METADATA_MIN_CANONICAL_BYTES, RETENTION_METADATA_MIN_OBJECTS, RetentionAnchorV2,
    RetentionFormat, RetentionReader, empty_retention_radix_ref,
};
use crate::rspace::persistence::wal::{
    GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1, GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1,
    GenerationCheckpointCommitRecordV1, GenerationCheckpointCommitViewV1,
    GenerationCheckpointPrepareIdentityV1, GenerationCheckpointPrepareRecordViewV1,
    GenerationCheckpointSourceCopyErrorV1, GenerationCheckpointSourceTestSealV1,
    GenerationCheckpointSourceV1, PreparedGenerationCheckpointV1,
    preflight_generation_checkpoint_prepare_v1,
};
use crate::rspace::persistence::{
    COMMIT_DISCRIMINATOR_LEN_V1, CanonicalBlob, CanonicalBlobKind, CanonicalDagNode,
    CanonicalEncode, CanonicalFormatError, CanonicalObjectKind, CanonicalSink, CommitPublicationV1,
    CommitRecordKindV1, DagChild, EndpointValue, LegacyHistoryRoot, LogPackAddress,
    LogPackDecodeLimits, LogPackFormatError, LogPackSequence, ObjectDigest, ObjectRefV1,
    PREPARE_DISCRIMINATOR_LEN_V1, PhysicalDigest, PrepareRecordKindV1, RetainedStateV1,
    RetentionManifestV1, SemanticProfileId, SemanticRoots, SemanticStateManifestV1, StateVersionV1,
    StoreId, decode_commit_kind_v1, decode_prepare_kind_v1,
};
use crate::rspace::rho_path::RhoPathV1;
use crate::rspace::serializers::serializers::{
    encode_continuation_identity, encode_continuation_source_into, encode_join_record,
};

const PROFILE: SemanticProfileId = SemanticProfileId::from_array(*b"RSpace.Test.V1\0\0");

#[derive(Clone)]
pub(super) struct SyntheticCheckpointSource {
    pub(super) descriptor: GenerationCheckpointDescriptorV1,
    pub(super) objects: Vec<(ObjectRefV1, Vec<u8>)>,
    copy_lookup: HashMap<ObjectRefV1, usize>,
}

impl SyntheticCheckpointSource {
    fn new(
        descriptor: GenerationCheckpointDescriptorV1,
        objects: Vec<(ObjectRefV1, Vec<u8>)>,
    ) -> Self {
        let mut copy_lookup = HashMap::with_capacity(objects.len());
        for (index, (reference, _)) in objects.iter().enumerate() {
            copy_lookup.entry(*reference).or_insert(index);
        }
        Self {
            descriptor,
            objects,
            copy_lookup,
        }
    }

    pub(super) fn push_object(&mut self, object: (ObjectRefV1, Vec<u8>)) {
        self.copy_lookup
            .entry(object.0)
            .or_insert(self.objects.len());
        self.objects.push(object);
    }

    pub(super) fn sort_objects_by_digest(&mut self) {
        self.objects
            .sort_unstable_by_key(|(reference, _)| reference.digest());
        self.copy_lookup.clear();
        for (index, (reference, _)) in self.objects.iter().enumerate() {
            self.copy_lookup.entry(*reference).or_insert(index);
        }
    }
}

impl GenerationCheckpointSourceTestSealV1 for SyntheticCheckpointSource {}

impl GenerationCheckpointSourceV1 for SyntheticCheckpointSource {
    type ReadError = Infallible;

    fn checkpoint_descriptor(
        &self,
    ) -> Result<GenerationCheckpointDescriptorV1, GenerationCheckpointErrorV1> {
        Ok(self.descriptor)
    }

    fn try_visit_checkpoint_references<E>(
        &self,
        mut visitor: impl FnMut(ObjectRefV1) -> Result<(), E>,
    ) -> Result<(), CheckpointReferenceVisitErrorV1<E>> {
        for (reference, _) in &self.objects {
            visitor(*reference).map_err(CheckpointReferenceVisitErrorV1::Visitor)?;
        }
        Ok(())
    }

    fn copy_checkpoint_object<S: CanonicalSink>(
        &self,
        reference: ObjectRefV1,
        sink: &mut S,
        _scratch: &mut Vec<u8>,
    ) -> Result<(), GenerationCheckpointSourceCopyErrorV1<Self::ReadError, S::Error>> {
        let index = self
            .copy_lookup
            .get(&reference)
            .expect("the static test inventory contains every visited object");
        let bytes = &self.objects[*index].1;
        sink.write_all(bytes)
            .map_err(GenerationCheckpointSourceCopyErrorV1::Sink)
    }
}

fn encoded<T: CanonicalEncode>(value: &T) -> (ObjectRefV1, Vec<u8>) {
    let bytes = value.try_canonical_bytes().unwrap();
    let reference =
        ObjectRefV1::new(ObjectDigest::of_canonical_bytes(&bytes), bytes.len() as u64).unwrap();
    (reference, bytes)
}

pub(super) fn fixture() -> SyntheticCheckpointSource {
    fixture_with_root(encoded(&CanonicalDagNode::empty()))
}

pub(super) fn fixture_for_profile(profile: SemanticProfileId) -> SyntheticCheckpointSource {
    fixture_with_root_and_profile(encoded(&CanonicalDagNode::empty()), profile)
}

/// A checkpoint whose current state exercises every positive semantic-
/// admission resource axis used by generation-publication preflight.
pub(super) fn semantic_fixture() -> SyntheticCheckpointSource {
    let channel = "checkpoint-resource-channel".to_owned();
    let datum = Datum::create(&channel, "checkpoint-resource-datum".to_owned(), true);
    let data = encoded(
        &CanonicalDagNode::new(
            RSpaceDataIndex::datum_path(&channel, &datum),
            EndpointValue::Multiplicity(nonzero(3)),
            Box::default(),
        )
        .unwrap(),
    );

    let channels = vec![channel.clone()];
    let patterns = vec!["checkpoint-resource-pattern".to_owned()];
    let waiting = WaitingContinuation::create(
        &channels,
        &patterns,
        &"checkpoint-resource-continuation".to_owned(),
        false,
        BTreeSet::from([0]),
    );
    let mut source_payload = Vec::new();
    encode_continuation_source_into(&waiting, &mut source_payload);
    let source = encoded(
        &CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, source_payload).unwrap(),
    );
    let continuation_prefix = RhoPathV1::continuation_prefix(&channels);
    let continuation = encoded(
        &CanonicalDagNode::new(
            RhoPathV1::collection_record_path(
                &continuation_prefix,
                &encode_continuation_identity(&waiting),
            ),
            EndpointValue::Content(source.0),
            Box::default(),
        )
        .unwrap(),
    );
    let mut join_path = RhoPathV1::join_prefix(&channel);
    join_path.extend_from_slice(&encode_join_record(&channels));
    let join = encoded(
        &CanonicalDagNode::new(join_path, EndpointValue::Multiplicity(nonzero(2)), Box::default())
            .unwrap(),
    );
    let roots = SemanticRoots {
        data_bag: data.0,
        continuation_map: continuation.0,
        join_bag: join.0,
    };
    let state_manifest = encoded(&SemanticStateManifestV1::new(1, PROFILE, roots, None).unwrap());
    let state = StateVersionV1::new(7, state_manifest.0);
    let current = vec![data, source, continuation, join, state_manifest];
    let current_bytes = current
        .iter()
        .map(|(reference, _)| reference.encoded_len())
        .sum::<u64>();
    let retention_manifest = encoded(
        &RetentionManifestV1::new(RetainedStateV1::new(state, current_bytes).unwrap(), Vec::new())
            .unwrap(),
    );

    let mut objects = current;
    objects.push(retention_manifest);
    objects.sort_unstable_by_key(|(reference, _)| reference.digest());
    let data_bytes = objects
        .iter()
        .map(|(reference, _)| reference.encoded_len())
        .sum();
    let mut ordered = OrderedObjectInventoryBuilderV1::new(objects.len() as u64, data_bytes);
    for (reference, _) in &objects {
        ordered.push(*reference).unwrap();
    }
    let retention_reference = objects
        .iter()
        .find_map(|(reference, bytes)| {
            bytes
                .starts_with(b"RSpaceDAG.Retn\0\0")
                .then_some(*reference)
        })
        .unwrap();
    let inventory = CheckpointObjectInventoryV1::from_test_ordered(
        RetainedUnionClosureReportV1 {
            distinct_objects: 5,
            canonical_bytes: current_bytes,
        },
        retention_reference,
        ordered.finish().unwrap(),
    )
    .unwrap();
    let descriptor = GenerationCheckpointDescriptorV1::from_test_inventory(
        PROFILE,
        state,
        retention_reference,
        5,
        current_bytes,
        inventory,
    )
    .unwrap();
    SyntheticCheckpointSource::new(descriptor, objects)
}

/// The current state is the two-object empty state while the retained history
/// contains the five-object semantic fixture. This catches replay certificates
/// that accidentally record only current-state structural demand.
pub(super) fn historically_larger_semantic_fixture()
-> (SyntheticCheckpointSource, StructuralClosureReport) {
    let mut historical_closure = Vec::new();
    let mut children = Vec::new();
    for edge in 0_u8..8 {
        let leaf = encoded(
            &CanonicalDagNode::new(
                vec![edge],
                EndpointValue::Multiplicity(nonzero(1)),
                Box::<[DagChild]>::default(),
            )
            .unwrap(),
        );
        children.push(DagChild {
            edge,
            target: leaf.0,
        });
        historical_closure.push(leaf);
    }
    let wide_root = encoded(
        &CanonicalDagNode::new(Vec::new(), EndpointValue::Absent, children)
            .expect("the historical structural root has eight ordered branches"),
    );
    historical_closure.push(wide_root.clone());
    let historical_empty = encoded(&CanonicalDagNode::empty());
    historical_closure.push(historical_empty.clone());
    let historical_manifest = encoded(
        &SemanticStateManifestV1::new(
            1,
            PROFILE,
            SemanticRoots {
                data_bag: wide_root.0,
                continuation_map: historical_empty.0,
                join_bag: historical_empty.0,
            },
            None,
        )
        .unwrap(),
    );
    let historical_state = StateVersionV1::new(7, historical_manifest.0);
    historical_closure.push(historical_manifest);
    let historical_bytes = historical_closure
        .iter()
        .map(|(reference, _)| reference.encoded_len())
        .sum::<u64>();
    let historical_report = StructuralClosureReport {
        distinct_objects: 11,
        node_objects: 10,
        payload_objects: 0,
        canonical_bytes: historical_bytes,
        references: 11,
        // The three manifest-root expectations are still live when the
        // eight-way data branch is expanded, and the empty shared root adds
        // one more pending expectation.
        maximum_worklist: 12,
    };
    let mut retained_union = historical_closure;

    let empty = encoded(&CanonicalDagNode::empty());
    let current_manifest = encoded(
        &SemanticStateManifestV1::new(
            1,
            PROFILE,
            SemanticRoots {
                data_bag: empty.0,
                continuation_map: empty.0,
                join_bag: empty.0,
            },
            None,
        )
        .unwrap(),
    );
    let current_state = StateVersionV1::new(historical_state.generation() + 1, current_manifest.0);
    let current_bytes = empty.0.encoded_len() + current_manifest.0.encoded_len();
    retained_union.push(empty);
    retained_union.push(current_manifest);
    retained_union.sort_unstable_by_key(|(reference, _)| reference.digest());
    retained_union.dedup_by(|left, right| {
        if left.0.digest() != right.0.digest() {
            return false;
        }
        assert_eq!(left, right, "equal canonical digests have byte-identical objects");
        true
    });

    let retention_manifest = encoded(
        &RetentionManifestV1::new(
            RetainedStateV1::new(current_state, current_bytes).unwrap(),
            vec![RetainedStateV1::new(historical_state, historical_bytes).unwrap()],
        )
        .unwrap(),
    );
    let retained_bytes = retained_union
        .iter()
        .map(|(reference, _)| reference.encoded_len())
        .sum::<u64>();
    let retained_objects = retained_union.len() as u64;
    let mut objects = retained_union;
    objects.push(retention_manifest);
    objects.sort_unstable_by_key(|(reference, _)| reference.digest());
    let data_bytes = objects
        .iter()
        .map(|(reference, _)| reference.encoded_len())
        .sum();
    let mut ordered = OrderedObjectInventoryBuilderV1::new(objects.len() as u64, data_bytes);
    for (reference, _) in &objects {
        ordered.push(*reference).unwrap();
    }
    let retention_reference = objects
        .iter()
        .find_map(|(reference, bytes)| {
            bytes
                .starts_with(b"RSpaceDAG.Retn\0\0")
                .then_some(*reference)
        })
        .unwrap();
    let inventory = CheckpointObjectInventoryV1::from_test_ordered(
        RetainedUnionClosureReportV1 {
            distinct_objects: retained_objects,
            canonical_bytes: retained_bytes,
        },
        retention_reference,
        ordered.finish().unwrap(),
    )
    .unwrap();
    let descriptor = GenerationCheckpointDescriptorV1::from_test_inventory(
        PROFILE,
        current_state,
        retention_reference,
        2,
        current_bytes,
        inventory,
    )
    .unwrap();
    (SyntheticCheckpointSource::new(descriptor, objects), historical_report)
}

pub(super) fn malformed_interior_fixture() -> SyntheticCheckpointSource {
    let mut bytes = b"RSpaceDAG.Node\0\0".to_vec();
    bytes.resize(32, 0);
    let reference =
        ObjectRefV1::new(ObjectDigest::of_canonical_bytes(&bytes), bytes.len() as u64).unwrap();
    fixture_with_root((reference, bytes))
}

pub(super) fn fixture_with_retained_union_cardinality(
    retained_union_objects: usize,
) -> SyntheticCheckpointSource {
    assert!(retained_union_objects >= 2, "a valid state closure needs its manifest plus a root",);
    let root = encoded(&CanonicalDagNode::empty());
    let roots = SemanticRoots {
        data_bag: root.0,
        continuation_map: root.0,
        join_bag: root.0,
    };
    let current_manifest = encoded(&SemanticStateManifestV1::new(1, PROFILE, roots, None).unwrap());
    let current_generation = u64::try_from(retained_union_objects)
        .unwrap()
        .checked_add(10_000)
        .unwrap();
    let current_state = StateVersionV1::new(current_generation, current_manifest.0);
    let current_bytes = root.0.encoded_len() + current_manifest.0.encoded_len();

    let historical_count = retained_union_objects - 2;
    let mut historical = Vec::with_capacity(historical_count);
    let mut manifest_objects = Vec::with_capacity(historical_count + 1);
    manifest_objects.push(current_manifest);
    for index in 0..historical_count {
        let generation = u64::try_from(index).unwrap().checked_add(1).unwrap();
        let mut legacy_root = [0_u8; 32];
        legacy_root[..8].copy_from_slice(&generation.to_be_bytes());
        let manifest = encoded(
            &SemanticStateManifestV1::new(
                1,
                PROFILE,
                roots,
                Some(LegacyHistoryRoot::from_array(legacy_root)),
            )
            .unwrap(),
        );
        historical.push(
            RetainedStateV1::new(
                StateVersionV1::new(generation, manifest.0),
                root.0.encoded_len() + manifest.0.encoded_len(),
            )
            .unwrap(),
        );
        manifest_objects.push(manifest);
    }
    let retention_manifest = encoded(
        &RetentionManifestV1::new(
            RetainedStateV1::new(current_state, current_bytes).unwrap(),
            historical,
        )
        .unwrap(),
    );

    let retained_bytes = root.0.encoded_len() +
        manifest_objects
            .iter()
            .map(|(reference, _)| reference.encoded_len())
            .sum::<u64>();
    let mut objects = Vec::with_capacity(retained_union_objects + 1);
    objects.push(root);
    objects.extend(manifest_objects);
    objects.push(retention_manifest);
    objects.sort_unstable_by_key(|(reference, _)| reference.digest());
    let data_bytes = objects
        .iter()
        .map(|(reference, _)| reference.encoded_len())
        .sum();
    let mut ordered = OrderedObjectInventoryBuilderV1::new(objects.len() as u64, data_bytes);
    for (reference, _) in &objects {
        ordered.push(*reference).unwrap();
    }
    let retention_reference = objects
        .iter()
        .find_map(|(reference, bytes)| {
            bytes
                .starts_with(b"RSpaceDAG.Retn\0\0")
                .then_some(*reference)
        })
        .unwrap();
    let inventory = CheckpointObjectInventoryV1::from_test_ordered(
        RetainedUnionClosureReportV1 {
            distinct_objects: retained_union_objects as u64,
            canonical_bytes: retained_bytes,
        },
        retention_reference,
        ordered.finish().unwrap(),
    )
    .unwrap();
    let descriptor = GenerationCheckpointDescriptorV1::from_test_inventory(
        PROFILE,
        current_state,
        retention_reference,
        2,
        current_bytes,
        inventory,
    )
    .unwrap();
    SyntheticCheckpointSource::new(descriptor, objects)
}

pub(super) fn fixture_with_root(root: (ObjectRefV1, Vec<u8>)) -> SyntheticCheckpointSource {
    fixture_with_root_and_profile(root, PROFILE)
}

fn fixture_with_root_and_profile(
    root: (ObjectRefV1, Vec<u8>),
    profile: SemanticProfileId,
) -> SyntheticCheckpointSource {
    let roots = SemanticRoots {
        data_bag: root.0,
        continuation_map: root.0,
        join_bag: root.0,
    };
    let state_manifest = encoded(&SemanticStateManifestV1::new(1, profile, roots, None).unwrap());
    let state = StateVersionV1::new(7, state_manifest.0);
    let current_bytes = root.0.encoded_len() + state_manifest.0.encoded_len();
    let retention_manifest = encoded(
        &RetentionManifestV1::new(RetainedStateV1::new(state, current_bytes).unwrap(), Vec::new())
            .unwrap(),
    );
    let mut objects = vec![root, state_manifest, retention_manifest];
    objects.sort_unstable_by_key(|(reference, _)| reference.digest());
    let data_bytes = objects
        .iter()
        .map(|(reference, _)| reference.encoded_len())
        .sum();
    let mut ordered = OrderedObjectInventoryBuilderV1::new(objects.len() as u64, data_bytes);
    for (reference, _) in &objects {
        ordered.push(*reference).unwrap();
    }
    let retention_reference = objects
        .iter()
        .find_map(|(reference, bytes)| {
            bytes
                .starts_with(b"RSpaceDAG.Retn\0\0")
                .then_some(*reference)
        })
        .unwrap();
    let inventory = CheckpointObjectInventoryV1::from_test_ordered(
        RetainedUnionClosureReportV1 {
            distinct_objects: 2,
            canonical_bytes: current_bytes,
        },
        retention_reference,
        ordered.finish().unwrap(),
    )
    .unwrap();
    let descriptor = GenerationCheckpointDescriptorV1::from_test_inventory(
        profile,
        state,
        retention_reference,
        2,
        current_bytes,
        inventory,
    )
    .unwrap();
    SyntheticCheckpointSource::new(descriptor, objects)
}

fn encode_checkpoint(source: &SyntheticCheckpointSource) -> Vec<u8> {
    encode_checkpoint_with_identity(source).0
}

fn encode_checkpoint_with_identity(
    source: &SyntheticCheckpointSource,
) -> (Vec<u8>, GenerationCheckpointPrepareIdentityV1) {
    let prepared = PreparedGenerationCheckpointV1::<TestSemanticProfile, _>::new(
        source,
        StoreId::from_array([0x91; 16]),
        LogPackSequence::new(19).unwrap(),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let mut bytes = Vec::new();
    let identity = prepared.write_to(&mut bytes).unwrap();
    assert_eq!(identity.record_len(), bytes.len() as u64);
    assert_eq!(identity.sequence(), LogPackSequence::new(19).unwrap());
    assert_eq!(identity.store_id(), StoreId::from_array([0x91; 16]));
    assert_eq!(identity.descriptor(), source.descriptor);
    (bytes, identity)
}

#[test]
fn target_checkpoint_wire_extents_follow_the_formal_length_equations() {
    let source = fixture();
    let prepared = PreparedGenerationCheckpointV1::<TestSemanticProfile, _>::new(
        &source,
        StoreId::from_array([0x91; 16]),
        LogPackSequence::new(19).unwrap(),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let inventory = source.descriptor.inventory();
    let data_objects = inventory.data_objects();
    let data_bytes = inventory.data_canonical_bytes().get();

    assert_eq!(GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1, 288);
    assert_eq!(prepared.record_len(), 288 + 40 * data_objects + data_bytes + 8,);

    let prepare_address =
        LogPackAddress::new(29, PACK_HEADER_LEN_V1, prepared.record_len()).unwrap();
    let index = PreparedGenerationCheckpointIndexV1::new(
        ObjectIndexHeaderV1::new(StoreId::from_array([0x91; 16]), IndexGenerationV1::FIRST),
        prepare_address,
        &prepared,
    )
    .unwrap();
    assert_eq!(index.exact_tail(), 64 + 72 * data_objects);

    let commit_address =
        LogPackAddress::new(29, prepare_address.end(), GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1)
            .unwrap();
    assert_eq!(prepare_address.offset(), 128);
    assert_eq!(commit_address.offset(), 128 + prepared.record_len());
    assert_eq!(GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1, 400);
    assert_eq!(commit_address.end(), 128 + prepared.record_len() + 400,);
}

fn checkpoint_commit_fixture(
    source: &SyntheticCheckpointSource,
) -> (Vec<u8>, GenerationCheckpointPrepareIdentityV1, LogPackAddress, CommitPublicationV1) {
    let (prepare_bytes, identity) = encode_checkpoint_with_identity(source);
    let address = LogPackAddress::new(29, PACK_HEADER_LEN_V1, prepare_bytes.len() as u64).unwrap();
    let prepared = PreparedGenerationCheckpointV1::<TestSemanticProfile, _>::new(
        source,
        StoreId::from_array([0x91; 16]),
        LogPackSequence::new(19).unwrap(),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let index = PreparedGenerationCheckpointIndexV1::new(
        ObjectIndexHeaderV1::new(
            StoreId::from_array([0x91; 16]),
            IndexGenerationV1::new(11).unwrap(),
        ),
        address,
        &prepared,
    )
    .unwrap();
    let commit = GenerationCheckpointCommitRecordV1::new(
        StoreId::from_array([0x91; 16]),
        PhysicalDigest::of_bytes(b"previous checkpoint terminal"),
        address,
        identity,
        index.publication(),
    )
    .unwrap();
    (commit.canonical_bytes().to_vec(), identity, address, index.publication())
}

fn rewrite_commit_crc(bytes: &mut [u8]) {
    let auth_len = bytes.len() - 8;
    let checksum = crate::rspace::persistence::checksum::crc32c(&bytes[..auth_len]);
    bytes[auth_len..]
        .copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(checksum));
}

fn rewrite_prepare_header_crc(bytes: &mut [u8]) {
    let authenticated_header_len = GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1 as usize - 8;
    let checksum = crate::rspace::persistence::checksum::crc32c(&bytes[..authenticated_header_len]);
    bytes[authenticated_header_len..GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1 as usize]
        .copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(checksum));
}

fn rewrite_prepare_record_crc(bytes: &mut [u8]) {
    let authenticated_record_len = bytes.len() - 8;
    let checksum = crate::rspace::persistence::checksum::crc32c(&bytes[..authenticated_record_len]);
    bytes[authenticated_record_len..]
        .copy_from_slice(&crate::rspace::persistence::checksum::encode_crc32c(checksum));
}

fn reseal_prepare(bytes: &mut [u8]) {
    rewrite_prepare_header_crc(bytes);
    rewrite_prepare_record_crc(bytes);
}

fn replace_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn assert_length_overflow(error: LogPackFormatError, expected_field: &'static str) {
    assert!(
        matches!(
            error,
            LogPackFormatError::Canonical(CanonicalFormatError::LengthOverflow { field })
                if field == expected_field
        ),
        "expected {expected_field:?} overflow, got {error:?}",
    );
}

#[test]
fn checkpoint_prepare_stream_round_trips_exact_inventory_and_offsets() {
    let source = fixture();
    let prepared = PreparedGenerationCheckpointV1::<TestSemanticProfile, _>::new(
        &source,
        StoreId::from_array([0x91; 16]),
        LogPackSequence::new(19).unwrap(),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let mut expected_layouts = Vec::new();
    prepared
        .try_visit_object_layouts(|layout| {
            expected_layouts.push(layout);
            Ok::<(), Infallible>(())
        })
        .unwrap();
    let bytes = encode_checkpoint(&source);
    assert_eq!(
        decode_prepare_kind_v1(&bytes[..PREPARE_DISCRIMINATOR_LEN_V1 as usize]).unwrap(),
        PrepareRecordKindV1::GenerationCheckpoint,
    );
    let preflight = preflight_generation_checkpoint_prepare_v1(
        &bytes[..GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1 as usize],
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    assert_eq!(preflight.record_len(), bytes.len() as u64);
    assert_eq!(preflight.sequence(), LogPackSequence::new(19).unwrap());
    assert_eq!(preflight.store_id(), StoreId::from_array([0x91; 16]));
    let view =
        GenerationCheckpointPrepareRecordViewV1::parse(&bytes, LogPackDecodeLimits::UNBOUNDED)
            .unwrap();
    assert_eq!(view.preflight(), preflight);
    assert_ne!(view.digest().as_bytes(), &[0; 32]);
    assert_ne!(view.crc32c(), 0);
    let observed = view.objects().collect::<Vec<_>>();
    assert_eq!(observed.len(), source.objects.len());
    for ((expected_reference, expected_bytes), (layout, object)) in source
        .objects
        .iter()
        .zip(expected_layouts.iter().zip(observed))
    {
        assert_eq!(object.reference(), *expected_reference);
        assert_eq!(object.canonical_bytes(), expected_bytes);
        assert_eq!(object.record_offset(), layout.record_offset());
        assert_eq!(object.canonical_offset(), layout.canonical_offset());
        assert_eq!(layout.reference(), *expected_reference);
    }
}

#[test]
fn canonical_v1_checkpoint_prepare_image_is_frozen() {
    let source = fixture();
    let bytes = encode_checkpoint(&source);
    let view =
        GenerationCheckpointPrepareRecordViewV1::parse(&bytes, LogPackDecodeLimits::UNBOUNDED)
            .unwrap();
    let frozen = format!(
        "len={};header={};digest={};crc={:08x}",
        bytes.len(),
        hex::encode(&bytes[..GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1 as usize]),
        view.digest(),
        view.crc32c(),
    );
    assert_eq!(
        frozen,
        "len=771;header=52537061636557414c2e5072657000000100000000000000030000000000000003030000000000001300000000000000919191919191919191919191919191915253706163654441472e43686b7074000100000000000000b8000000000000005253706163652e546573742e56310000070000000000000008a2bad5d01cd13ce6c39000aa50c93509172aebb94fed4f47858b8414404bcfd10000000000000039f726adc51675b622380f86cd0388fe825d30d66d5eaaa2f2ba1da32e4049166000000000000000020000000000000003010000000000000301000000000000020000000000000003000000000000006301000000000000bbb31f85f5194118002f2c656ad3790578f13c11efa309593ee28e4da308651b083ae2c600000000;digest=67f6edcb75a1c61a9946c219eeaae0295c14c205949accf5aa2c5c7db23c1c7f;crc=7708862c"
    );
}

#[test]
fn checkpoint_prepare_rejects_truncation_corruption_and_resource_excess() {
    let source = fixture();
    let bytes = encode_checkpoint(&source);
    for length in 0..bytes.len() {
        assert!(
            GenerationCheckpointPrepareRecordViewV1::parse(
                &bytes[..length],
                LogPackDecodeLimits::UNBOUNDED,
            )
            .is_err(),
            "checkpoint truncation at {length} must fail",
        );
    }
    let mut extended = bytes.clone();
    extended.push(0);
    assert!(matches!(
        GenerationCheckpointPrepareRecordViewV1::parse(
            &extended,
            LogPackDecodeLimits::UNBOUNDED,
        ),
        Err(LogPackFormatError::LengthMismatch { declared, actual })
            if declared == bytes.len() as u64 && actual == extended.len() as u64
    ));

    let mut wrong_kind = bytes.clone();
    wrong_kind[24..32].copy_from_slice(&2_u64.to_le_bytes());
    reseal_prepare(&mut wrong_kind);
    assert!(matches!(
        GenerationCheckpointPrepareRecordViewV1::parse(&wrong_kind, LogPackDecodeLimits::UNBOUNDED,),
        Err(LogPackFormatError::UnexpectedRecordKind {
            expected: 3,
            actual: 2
        })
    ));

    for declared in [bytes.len() as u64 - 1, bytes.len() as u64 + 1] {
        let mut wrong_length = bytes.clone();
        wrong_length[32..40].copy_from_slice(&declared.to_le_bytes());
        reseal_prepare(&mut wrong_length);
        assert!(matches!(
            GenerationCheckpointPrepareRecordViewV1::parse(
                &wrong_length,
                LogPackDecodeLimits::UNBOUNDED,
            ),
            Err(LogPackFormatError::LengthMismatch { declared: observed, actual })
                if observed == declared && actual == bytes.len() as u64
        ));
    }
    let mut header = bytes.clone();
    header[64] ^= 1;
    assert!(matches!(
        GenerationCheckpointPrepareRecordViewV1::parse(&header, LogPackDecodeLimits::UNBOUNDED,),
        Err(LogPackFormatError::HeaderChecksum { .. })
    ));
    let mut payload = bytes.clone();
    payload[GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1 as usize + 40] ^= 1;
    assert!(matches!(
        GenerationCheckpointPrepareRecordViewV1::parse(&payload, LogPackDecodeLimits::UNBOUNDED,),
        Err(LogPackFormatError::RecordChecksum { .. })
    ));
    let largest = source
        .objects
        .iter()
        .map(|(reference, _)| reference.encoded_len())
        .max()
        .unwrap();
    let limits = LogPackDecodeLimits {
        max_single_object_bytes: largest - 1,
        ..LogPackDecodeLimits::UNBOUNDED
    };
    assert!(matches!(
        PreparedGenerationCheckpointV1::<TestSemanticProfile, _>::new(
            &source,
            StoreId::from_array([0x91; 16]),
            LogPackSequence::new(19).unwrap(),
            limits,
        ),
        Err(LogPackFormatError::ResourceLimit { .. })
    ));
    assert!(matches!(
        GenerationCheckpointPrepareRecordViewV1::parse(&bytes, limits),
        Err(LogPackFormatError::ResourceLimit { .. })
    ));
}

#[test]
fn checkpoint_discriminators_checksums_and_authenticated_lengths_fail_closed() {
    fn changed_u64(prefix: &[u8], offset: usize, value: u64) -> Vec<u8> {
        let mut changed = prefix.to_vec();
        replace_u64(&mut changed, offset, value);
        changed
    }

    let source = fixture();
    let prepare = encode_checkpoint(&source);
    let prepare_discriminator = &prepare[..PREPARE_DISCRIMINATOR_LEN_V1 as usize];
    for length in [31_usize, 33] {
        let mut candidate = prepare_discriminator.to_vec();
        candidate.resize(length, 0);
        assert!(matches!(
            decode_prepare_kind_v1(&candidate),
            Err(LogPackFormatError::LengthMismatch { declared: 32, actual })
                if actual == length as u64
        ));
    }
    let mut invalid_prepare_domain = prepare_discriminator.to_vec();
    invalid_prepare_domain[0] ^= 1;
    assert!(matches!(
        decode_prepare_kind_v1(&invalid_prepare_domain),
        Err(LogPackFormatError::InvalidDomain)
    ));
    assert!(matches!(
        decode_prepare_kind_v1(&changed_u64(prepare_discriminator, 16, 2)),
        Err(LogPackFormatError::UnsupportedVersion(2))
    ));
    assert_eq!(
        decode_prepare_kind_v1(&changed_u64(prepare_discriminator, 24, 1)).unwrap(),
        PrepareRecordKindV1::Incremental,
    );
    assert_eq!(
        decode_prepare_kind_v1(&changed_u64(prepare_discriminator, 24, 3)).unwrap(),
        PrepareRecordKindV1::GenerationCheckpoint,
    );
    assert!(matches!(
        decode_prepare_kind_v1(&changed_u64(prepare_discriminator, 24, 99)),
        Err(LogPackFormatError::UnsupportedPrepareKind(99))
    ));

    let (commit, _, _, _) = checkpoint_commit_fixture(&source);
    let commit_discriminator = &commit[..COMMIT_DISCRIMINATOR_LEN_V1 as usize];
    for length in [31_usize, 33] {
        let mut candidate = commit_discriminator.to_vec();
        candidate.resize(length, 0);
        assert!(matches!(
            decode_commit_kind_v1(&candidate),
            Err(LogPackFormatError::LengthMismatch { declared: 32, actual })
                if actual == length as u64
        ));
    }
    let mut invalid_commit_domain = commit_discriminator.to_vec();
    invalid_commit_domain[0] ^= 1;
    assert!(matches!(
        decode_commit_kind_v1(&invalid_commit_domain),
        Err(LogPackFormatError::InvalidDomain)
    ));
    assert!(matches!(
        decode_commit_kind_v1(&changed_u64(commit_discriminator, 16, 2)),
        Err(LogPackFormatError::UnsupportedVersion(2))
    ));
    assert_eq!(
        decode_commit_kind_v1(&changed_u64(commit_discriminator, 24, 2)).unwrap(),
        CommitRecordKindV1::Incremental,
    );
    assert_eq!(
        decode_commit_kind_v1(&changed_u64(commit_discriminator, 24, 4)).unwrap(),
        CommitRecordKindV1::GenerationCheckpoint,
    );
    assert!(matches!(
        decode_commit_kind_v1(&changed_u64(commit_discriminator, 24, 99)),
        Err(LogPackFormatError::UnsupportedCommitKind(99))
    ));

    for (offset, expected) in [(0_usize, "domain"), (16, "version"), (24, "kind")] {
        let mut changed = prepare.clone();
        changed[offset] ^= 1;
        reseal_prepare(&mut changed);
        let error = GenerationCheckpointPrepareRecordViewV1::parse(
            &changed,
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap_err();
        match expected {
            "domain" => assert!(matches!(error, LogPackFormatError::InvalidDomain)),
            "version" => assert!(matches!(error, LogPackFormatError::UnsupportedVersion(_))),
            "kind" => assert!(matches!(error, LogPackFormatError::UnexpectedRecordKind { .. })),
            _ => unreachable!(),
        }
    }

    let mut header_checksum = prepare.clone();
    header_checksum[280] ^= 1;
    assert!(matches!(
        GenerationCheckpointPrepareRecordViewV1::parse(
            &header_checksum,
            LogPackDecodeLimits::UNBOUNDED,
        ),
        Err(LogPackFormatError::HeaderChecksum { .. })
    ));
    let mut wide_header_checksum = prepare.clone();
    wide_header_checksum[284] ^= 1;
    assert!(matches!(
        GenerationCheckpointPrepareRecordViewV1::parse(
            &wide_header_checksum,
            LogPackDecodeLimits::UNBOUNDED,
        ),
        Err(LogPackFormatError::ChecksumOutOfRange {
            field: "checkpoint prepare header",
            ..
        })
    ));
    let prepare_trailer = prepare.len() - 8;
    let mut record_checksum = prepare.clone();
    record_checksum[prepare_trailer] ^= 1;
    assert!(matches!(
        GenerationCheckpointPrepareRecordViewV1::parse(
            &record_checksum,
            LogPackDecodeLimits::UNBOUNDED,
        ),
        Err(LogPackFormatError::RecordChecksum { .. })
    ));
    let mut wide_record_checksum = prepare.clone();
    wide_record_checksum[prepare_trailer + 4] ^= 1;
    assert!(matches!(
        GenerationCheckpointPrepareRecordViewV1::parse(
            &wide_record_checksum,
            LogPackDecodeLimits::UNBOUNDED,
        ),
        Err(LogPackFormatError::ChecksumOutOfRange {
            field: "checkpoint prepare record",
            ..
        })
    ));
    let mut commit_checksum = commit.clone();
    commit_checksum[392] ^= 1;
    assert!(matches!(
        GenerationCheckpointCommitViewV1::parse(&commit_checksum),
        Err(LogPackFormatError::RecordChecksum { .. })
    ));
    let mut wide_commit_checksum = commit.clone();
    wide_commit_checksum[396] ^= 1;
    assert!(matches!(
        GenerationCheckpointCommitViewV1::parse(&wide_commit_checksum),
        Err(LogPackFormatError::ChecksumOutOfRange {
            field: "checkpoint COMMIT record",
            ..
        })
    ));

    for delta in [-1_i64, 1] {
        let mut changed = prepare.clone();
        let body_len = 184_u64.checked_add_signed(delta).unwrap();
        replace_u64(&mut changed, 88, body_len);
        reseal_prepare(&mut changed);
        let error = GenerationCheckpointPrepareRecordViewV1::parse(
            &changed,
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap_err();
        assert!(matches!(
            (delta, error),
            (
                -1,
                LogPackFormatError::Checkpoint(GenerationCheckpointErrorV1::Canonical(
                    CanonicalFormatError::TrailingBytes { .. }
                ))
            ) | (
                1,
                LogPackFormatError::Checkpoint(GenerationCheckpointErrorV1::Canonical(
                    CanonicalFormatError::Truncated { .. }
                ))
            )
        ));

        let mut changed = commit.clone();
        replace_u64(&mut changed, 176, body_len);
        rewrite_commit_crc(&mut changed);
        let error = GenerationCheckpointCommitViewV1::parse(&changed).unwrap_err();
        assert!(matches!(
            (delta, error),
            (
                -1,
                LogPackFormatError::Checkpoint(GenerationCheckpointErrorV1::Canonical(
                    CanonicalFormatError::TrailingBytes { .. }
                ))
            ) | (
                1,
                LogPackFormatError::Checkpoint(GenerationCheckpointErrorV1::Canonical(
                    CanonicalFormatError::Truncated { .. }
                ))
            )
        ));

        let mut changed = prepare.clone();
        let first_object_len = u64::from_le_bytes(changed[320..328].try_into().unwrap());
        replace_u64(&mut changed, 320, first_object_len.checked_add_signed(delta).unwrap());
        rewrite_prepare_record_crc(&mut changed);
        let error = GenerationCheckpointPrepareRecordViewV1::parse(
            &changed,
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap_err();
        assert!(matches!(
            (delta, error),
            (-1, LogPackFormatError::Canonical(CanonicalFormatError::Truncated { .. })) |
                (1, LogPackFormatError::Canonical(CanonicalFormatError::TrailingBytes { .. }))
        ));

        let mut changed = prepare.clone();
        let first_body_len = u64::from_le_bytes(changed[352..360].try_into().unwrap());
        replace_u64(&mut changed, 352, first_body_len.checked_add_signed(delta).unwrap());
        rewrite_prepare_record_crc(&mut changed);
        let error = GenerationCheckpointPrepareRecordViewV1::parse(
            &changed,
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap_err();
        assert!(matches!(
            (delta, error),
            (-1, LogPackFormatError::Canonical(CanonicalFormatError::TrailingBytes { .. })) |
                (1, LogPackFormatError::Canonical(CanonicalFormatError::Truncated { .. }))
        ));
    }
}

#[test]
fn checkpoint_wire_arithmetic_fails_closed_at_every_prepare_and_index_boundary() {
    const PREPARE_DESCRIPTOR_OFFSET: usize = 64;
    const COMMIT_DESCRIPTOR_OFFSET: usize = 152;
    const RETAINED_BYTES_OFFSET: usize = 152;
    const RETAINED_OBJECTS_OFFSET: usize = 160;
    const DATA_OBJECTS_OFFSET: usize = 168;
    const DATA_BYTES_OFFSET: usize = 176;
    let source = fixture();
    let prepare = encode_checkpoint(&source);
    let retention_manifest_len = source.descriptor.retention_manifest().encoded_len();
    let descriptor = &prepare[PREPARE_DESCRIPTOR_OFFSET..PREPARE_DESCRIPTOR_OFFSET + 216];

    let mut retained_count_overflow = descriptor.to_vec();
    replace_u64(&mut retained_count_overflow, RETAINED_OBJECTS_OFFSET, u64::MAX);
    assert!(matches!(
        GenerationCheckpointDescriptorViewV1::parse(&retained_count_overflow),
        Err(GenerationCheckpointErrorV1::CounterOverflow("checkpoint DATA objects"))
    ));

    let mut retained_bytes_overflow = descriptor.to_vec();
    replace_u64(&mut retained_bytes_overflow, RETAINED_BYTES_OFFSET, u64::MAX);
    assert!(matches!(
        GenerationCheckpointDescriptorViewV1::parse(&retained_bytes_overflow),
        Err(GenerationCheckpointErrorV1::Canonical(CanonicalFormatError::LengthOverflow {
            field: "checkpoint DATA canonical bytes"
        }))
    ));

    let mut object_header_overflow = prepare.clone();
    let overflowing_objects = u64::MAX / 40 + 1;
    replace_u64(
        &mut object_header_overflow,
        PREPARE_DESCRIPTOR_OFFSET + RETAINED_OBJECTS_OFFSET,
        overflowing_objects - 1,
    );
    replace_u64(
        &mut object_header_overflow,
        PREPARE_DESCRIPTOR_OFFSET + DATA_OBJECTS_OFFSET,
        overflowing_objects,
    );
    rewrite_prepare_header_crc(&mut object_header_overflow);
    assert_length_overflow(
        preflight_generation_checkpoint_prepare_v1(
            &object_header_overflow[..GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1 as usize],
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap_err(),
        "checkpoint object headers",
    );

    for (data_bytes, expected_field) in [
        (u64::MAX, "checkpoint object records"),
        (u64::MAX - 120, "checkpoint prepare record"),
        (u64::MAX - 128, "checkpoint prepare record"),
    ] {
        let mut extent_overflow = prepare.clone();
        replace_u64(
            &mut extent_overflow,
            PREPARE_DESCRIPTOR_OFFSET + RETAINED_BYTES_OFFSET,
            data_bytes - retention_manifest_len,
        );
        replace_u64(
            &mut extent_overflow,
            PREPARE_DESCRIPTOR_OFFSET + DATA_BYTES_OFFSET,
            data_bytes,
        );
        rewrite_prepare_header_crc(&mut extent_overflow);
        assert_length_overflow(
            preflight_generation_checkpoint_prepare_v1(
                &extent_overflow[..GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1 as usize],
                LogPackDecodeLimits::UNBOUNDED,
            )
            .unwrap_err(),
            expected_field,
        );
    }

    let (commit, _, _, _) = checkpoint_commit_fixture(&source);
    for (objects, expected_field) in [
        (u64::MAX / INDEX_RECORD_LEN_V1 + 1, "checkpoint index records"),
        (u64::MAX / INDEX_RECORD_LEN_V1, "checkpoint exact index tail"),
    ] {
        let mut index_overflow = commit.clone();
        replace_u64(
            &mut index_overflow,
            COMMIT_DESCRIPTOR_OFFSET + RETAINED_OBJECTS_OFFSET,
            objects - 1,
        );
        replace_u64(&mut index_overflow, COMMIT_DESCRIPTOR_OFFSET + DATA_OBJECTS_OFFSET, objects);
        rewrite_commit_crc(&mut index_overflow);
        assert_length_overflow(
            GenerationCheckpointCommitViewV1::parse(&index_overflow).unwrap_err(),
            expected_field,
        );
    }

    let maximal_prepare =
        LogPackAddress::new(29, PACK_HEADER_LEN_V1, u64::MAX - PACK_HEADER_LEN_V1).unwrap();
    assert_eq!(maximal_prepare.end(), u64::MAX);
    assert!(matches!(
        LogPackAddress::new(29, PACK_HEADER_LEN_V1, u64::MAX - PACK_HEADER_LEN_V1 + 1),
        Err(LogPackFormatError::AddressOverflow { offset, length })
            if offset == PACK_HEADER_LEN_V1 && length == u64::MAX - PACK_HEADER_LEN_V1 + 1
    ));
    let maximal_commit = LogPackAddress::new(
        29,
        u64::MAX - GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1,
        GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1,
    )
    .unwrap();
    assert_eq!(maximal_commit.end(), u64::MAX);
    assert!(matches!(
        LogPackAddress::new(
            29,
            u64::MAX - GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1 + 1,
            GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1,
        ),
        Err(LogPackFormatError::AddressOverflow { offset, length })
            if offset == u64::MAX - GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1 + 1
                && length == GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1
    ));
}

#[test]
fn checkpoint_parser_requires_the_designated_manifest_objects_and_kinds() {
    let valid = fixture();
    let current_bytes = valid.descriptor.current_state_canonical_bytes().get();
    let inventory = valid.descriptor.inventory();

    let missing_reference = ObjectRefV1::new(
        ObjectDigest::from_array([0xF7; 32]),
        valid.descriptor.state().manifest().encoded_len(),
    )
    .unwrap();
    let mut missing = valid.clone();
    missing.descriptor = GenerationCheckpointDescriptorV1::from_test_inventory(
        PROFILE,
        StateVersionV1::new(7, missing_reference),
        valid.descriptor.retention_manifest(),
        valid.descriptor.current_state_objects(),
        current_bytes,
        inventory,
    )
    .unwrap();
    assert!(matches!(
        GenerationCheckpointPrepareRecordViewV1::parse(
            &encode_checkpoint(&missing),
            LogPackDecodeLimits::UNBOUNDED,
        ),
        Err(LogPackFormatError::CheckpointDesignatedObjectMissing {
            role: "state manifest",
            reference,
        }) if reference == missing_reference
    ));

    let wrong_kind = valid
        .objects
        .iter()
        .find_map(|(reference, bytes)| bytes.starts_with(b"RSpaceDAG.Node\0").then_some(*reference))
        .unwrap();
    let mut wrong = valid.clone();
    wrong.descriptor = GenerationCheckpointDescriptorV1::from_test_inventory(
        PROFILE,
        StateVersionV1::new(7, wrong_kind),
        valid.descriptor.retention_manifest(),
        valid.descriptor.current_state_objects(),
        current_bytes,
        inventory,
    )
    .unwrap();
    assert!(matches!(
        GenerationCheckpointPrepareRecordViewV1::parse(
            &encode_checkpoint(&wrong),
            LogPackDecodeLimits::UNBOUNDED,
        ),
        Err(LogPackFormatError::CheckpointDesignatedObjectKind {
            role: "state manifest",
            reference,
            ..
        }) if reference == wrong_kind
    ));
}

#[test]
fn checkpoint_commit_and_fresh_index_share_one_preflighted_layout() {
    let source = fixture();
    let prepared = PreparedGenerationCheckpointV1::<TestSemanticProfile, _>::new(
        &source,
        StoreId::from_array([0x91; 16]),
        LogPackSequence::new(19).unwrap(),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let (prepare_bytes, prepare_identity) = encode_checkpoint_with_identity(&source);
    let prepare_address =
        LogPackAddress::new(29, PACK_HEADER_LEN_V1, prepare_bytes.len() as u64).unwrap();
    let header = ObjectIndexHeaderV1::new(
        StoreId::from_array([0x91; 16]),
        IndexGenerationV1::new(11).unwrap(),
    );
    let index =
        PreparedGenerationCheckpointIndexV1::new(header, prepare_address, &prepared).unwrap();
    assert_eq!(index.header(), header);
    assert_eq!(index.prepare_address(), prepare_address);
    assert_eq!(index.record_count(), source.objects.len() as u64);
    assert_eq!(
        index.exact_tail(),
        INDEX_HEADER_LEN_V1 + source.objects.len() as u64 * INDEX_RECORD_LEN_V1,
    );

    let mut expected_layouts = Vec::new();
    prepared
        .try_visit_object_layouts(|layout| {
            expected_layouts.push(layout);
            Ok::<(), Infallible>(())
        })
        .unwrap();
    let mut records = Vec::new();
    index
        .try_visit_records(|record| {
            records.push(record);
            Ok::<(), Infallible>(())
        })
        .unwrap();
    let parsed_prepare = GenerationCheckpointPrepareRecordViewV1::parse(
        &prepare_bytes,
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let prepare_references = parsed_prepare
        .objects()
        .map(|object| object.reference())
        .collect::<Vec<_>>();
    let index_references = records
        .iter()
        .map(|record| record.reference())
        .collect::<Vec<_>>();
    assert_eq!(prepare_references, index_references);
    assert_eq!(records.len(), expected_layouts.len());
    for (record, layout) in records.iter().zip(expected_layouts) {
        assert_eq!(record.reference(), layout.reference());
        assert_eq!(record.location().pack_id().get(), 29);
        assert_eq!(record.location().sequence(), LogPackSequence::new(19).unwrap());
        assert_eq!(
            record.location().canonical_offset(),
            PACK_HEADER_LEN_V1 + layout.canonical_offset(),
        );
    }

    let previous_commit = PhysicalDigest::of_bytes(b"previous checkpoint terminal");
    let commit = GenerationCheckpointCommitRecordV1::new(
        StoreId::from_array([0x91; 16]),
        previous_commit,
        prepare_address,
        prepare_identity,
        index.publication(),
    )
    .unwrap();
    assert_eq!(commit.canonical_bytes().len() as u64, GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1);
    assert_eq!(
        decode_commit_kind_v1(&commit.canonical_bytes()[..COMMIT_DISCRIMINATOR_LEN_V1 as usize],)
            .unwrap(),
        CommitRecordKindV1::GenerationCheckpoint,
    );
    let view = GenerationCheckpointCommitViewV1::parse(commit.canonical_bytes()).unwrap();
    assert_eq!(view.sequence(), LogPackSequence::new(19).unwrap());
    assert_eq!(view.store_id(), StoreId::from_array([0x91; 16]));
    assert_eq!(view.previous_commit(), previous_commit);
    assert_eq!(view.prepare_address(), prepare_address);
    assert_eq!(view.prepare_digest(), prepare_identity.digest());
    assert_eq!(view.descriptor(), source.descriptor.as_view());
    assert_eq!(view.publication(), index.publication());
    assert_eq!(view.digest(), commit.digest());
    view.verify_prepare(prepare_identity).unwrap();
    view.verify_record_view(
        GenerationCheckpointPrepareRecordViewV1::parse(
            &prepare_bytes,
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap(),
    )
    .unwrap();

    let failure = index.try_visit_records(|_| Err::<(), _>("stop"));
    assert!(matches!(failure, Err(PreparedGenerationCheckpointIndexVisitErrorV1::Visitor("stop"))));
}

#[test]
fn canonical_v1_checkpoint_commit_image_is_frozen() {
    let source = fixture();
    let (bytes, _, _, _) = checkpoint_commit_fixture(&source);
    let view = GenerationCheckpointCommitViewV1::parse(&bytes).unwrap();
    let frozen =
        format!("len={};bytes={};digest={}", bytes.len(), hex::encode(&bytes), view.digest(),);
    assert_eq!(
        frozen,
        "len=400;bytes=52537061636557414c2e436f6d6d00000100000000000000040000000000000090010000000000001300000000000000919191919191919191919191919191917a6782fd8b272fc8a88eb7f987fa2aac6676db0a8c71f7e85ecc7bf45a488b5f1d000000000000008000000000000000030300000000000067f6edcb75a1c61a9946c219eeaae0295c14c205949accf5aa2c5c7db23c1c7f5253706163654441472e43686b7074000100000000000000b8000000000000005253706163652e546573742e56310000070000000000000008a2bad5d01cd13ce6c39000aa50c93509172aebb94fed4f47858b8414404bcfd10000000000000039f726adc51675b622380f86cd0388fe825d30d66d5eaaa2f2ba1da32e4049166000000000000000020000000000000003010000000000000301000000000000020000000000000003000000000000006301000000000000bbb31f85f5194118002f2c656ad3790578f13c11efa309593ee28e4da308651b18010000000000001d000000000000000b000000000000003be4e21100000000;digest=89afde7663a6fdad7ce6eba27e8b5ea77cae896f3a9d22fc462a96acc0d8266a"
    );
}

#[test]
fn checkpoint_commit_rejects_truncation_corruption_and_rebinding() {
    let source = fixture();
    let (bytes, prepare, address, publication) = checkpoint_commit_fixture(&source);
    for length in 0..bytes.len() {
        assert!(
            GenerationCheckpointCommitViewV1::parse(&bytes[..length]).is_err(),
            "checkpoint COMMIT truncation at {length} must fail",
        );
    }

    let mut extended = bytes.clone();
    extended.push(0);
    assert!(matches!(
        GenerationCheckpointCommitViewV1::parse(&extended),
        Err(LogPackFormatError::LengthMismatch { declared, actual })
            if declared == GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1
                && actual == extended.len() as u64
    ));

    let mut corrupt = bytes.clone();
    corrupt[120] ^= 1;
    assert!(matches!(
        GenerationCheckpointCommitViewV1::parse(&corrupt),
        Err(LogPackFormatError::RecordChecksum { .. })
    ));

    let descriptor_offset = 16 + 4 * 8 + 16 + 32 + 3 * 8 + 32;
    let inventory_scalar_offsets = [136, 144, 152, 160, 168, 176];
    for scalar_offset in inventory_scalar_offsets {
        let mut changed = bytes.clone();
        changed[descriptor_offset + scalar_offset] ^= 1;
        assert!(matches!(
            GenerationCheckpointCommitViewV1::parse(&changed),
            Err(LogPackFormatError::RecordChecksum { .. })
        ));
    }

    for (offset, expected) in [(0, "domain"), (16, "version"), (24, "kind"), (32, "length")] {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        rewrite_commit_crc(&mut changed);
        assert!(
            GenerationCheckpointCommitViewV1::parse(&changed).is_err(),
            "changed {expected} must fail",
        );
    }

    let mut high_crc = bytes.clone();
    let last = high_crc.len() - 1;
    high_crc[last] = 1;
    assert!(matches!(
        GenerationCheckpointCommitViewV1::parse(&high_crc),
        Err(LogPackFormatError::ChecksumOutOfRange { .. })
    ));

    let discriminator = &bytes[..COMMIT_DISCRIMINATOR_LEN_V1 as usize];
    let mut incremental = discriminator.to_vec();
    incremental[24..32].copy_from_slice(&2_u64.to_le_bytes());
    let incremental = decode_commit_kind_v1(&incremental).unwrap();
    assert_eq!(incremental, CommitRecordKindV1::Incremental);
    assert_eq!(
        incremental.fixed_record_len(),
        crate::rspace::persistence::TRANSACTION_COMMIT_RECORD_LEN_V1,
    );
    let mut unsupported = discriminator.to_vec();
    unsupported[24..32].copy_from_slice(&99_u64.to_le_bytes());
    assert!(matches!(
        decode_commit_kind_v1(&unsupported),
        Err(LogPackFormatError::UnsupportedCommitKind(99))
    ));
    assert!(decode_commit_kind_v1(&discriminator[..discriminator.len() - 1]).is_err());

    for field_offset in [40_usize, 48, 120, 200] {
        let mut rebound = bytes.clone();
        rebound[field_offset] ^= 1;
        rewrite_commit_crc(&mut rebound);
        let view = GenerationCheckpointCommitViewV1::parse(&rebound).unwrap();
        assert!(
            view.verify_prepare(prepare).is_err(),
            "rebound field at {field_offset} must not verify against the original PREPARE",
        );
    }

    let wrong_offset =
        LogPackAddress::new(address.segment(), PACK_HEADER_LEN_V1 + 1, address.length()).unwrap();
    assert!(matches!(
        GenerationCheckpointCommitRecordV1::new(
            StoreId::from_array([0x91; 16]),
            PhysicalDigest::of_bytes(b"previous checkpoint terminal"),
            wrong_offset,
            prepare,
            publication,
        ),
        Err(LogPackFormatError::CheckpointPrepareOffset { .. })
    ));

    let wrong_first_live = CommitPublicationV1::new(
        address.segment() + 1,
        publication.index_generation(),
        publication.durable_index_tail(),
        publication.retention_manifest(),
        publication.current_state_canonical_bytes(),
        publication.retained_canonical_bytes(),
    )
    .unwrap();
    assert!(matches!(
        GenerationCheckpointCommitRecordV1::new(
            StoreId::from_array([0x91; 16]),
            PhysicalDigest::of_bytes(b"previous checkpoint terminal"),
            address,
            prepare,
            wrong_first_live,
        ),
        Err(LogPackFormatError::CheckpointFirstLivePack { .. })
    ));

    for tail in [
        publication.durable_index_tail() - INDEX_RECORD_LEN_V1,
        publication.durable_index_tail() + INDEX_RECORD_LEN_V1,
    ] {
        let wrong_tail = CommitPublicationV1::new(
            publication.first_live_pack(),
            publication.index_generation(),
            tail,
            publication.retention_manifest(),
            publication.current_state_canonical_bytes(),
            publication.retained_canonical_bytes(),
        )
        .unwrap();
        assert!(matches!(
            GenerationCheckpointCommitRecordV1::new(
                StoreId::from_array([0x91; 16]),
                PhysicalDigest::of_bytes(b"previous checkpoint terminal"),
                address,
                prepare,
                wrong_tail,
            ),
            Err(LogPackFormatError::CheckpointIndexTail { .. })
        ));
    }

    let wrong_retention = CommitPublicationV1::new(
        publication.first_live_pack(),
        publication.index_generation(),
        publication.durable_index_tail(),
        source.descriptor.state().manifest(),
        publication.current_state_canonical_bytes(),
        publication.retained_canonical_bytes(),
    )
    .unwrap();
    assert!(matches!(
        GenerationCheckpointCommitRecordV1::new(
            StoreId::from_array([0x91; 16]),
            PhysicalDigest::of_bytes(b"previous checkpoint terminal"),
            address,
            prepare,
            wrong_retention,
        ),
        Err(LogPackFormatError::CheckpointPublicationRetention { .. })
    ));

    let wrong_current = CommitPublicationV1::new(
        publication.first_live_pack(),
        publication.index_generation(),
        publication.durable_index_tail(),
        publication.retention_manifest(),
        publication.current_state_canonical_bytes() - 1,
        publication.retained_canonical_bytes(),
    )
    .unwrap();
    assert!(matches!(
        GenerationCheckpointCommitRecordV1::new(
            StoreId::from_array([0x91; 16]),
            PhysicalDigest::of_bytes(b"previous checkpoint terminal"),
            address,
            prepare,
            wrong_current,
        ),
        Err(LogPackFormatError::CheckpointPublicationCurrentBytes { .. })
    ));

    let wrong_retained = CommitPublicationV1::new(
        publication.first_live_pack(),
        publication.index_generation(),
        publication.durable_index_tail(),
        publication.retention_manifest(),
        publication.current_state_canonical_bytes(),
        publication.retained_canonical_bytes() + 1,
    )
    .unwrap();
    assert!(matches!(
        GenerationCheckpointCommitRecordV1::new(
            StoreId::from_array([0x91; 16]),
            PhysicalDigest::of_bytes(b"previous checkpoint terminal"),
            address,
            prepare,
            wrong_retained,
        ),
        Err(LogPackFormatError::CheckpointPublicationRetainedBytes { .. })
    ));
}

#[test]
fn fresh_index_rejects_store_address_and_offset_drift_before_visiting() {
    let source = fixture();
    let prepared = PreparedGenerationCheckpointV1::<TestSemanticProfile, _>::new(
        &source,
        StoreId::from_array([0x91; 16]),
        LogPackSequence::new(19).unwrap(),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let header = ObjectIndexHeaderV1::new(
        StoreId::from_array([0x91; 16]),
        IndexGenerationV1::new(11).unwrap(),
    );
    let address = LogPackAddress::new(29, PACK_HEADER_LEN_V1, prepared.record_len()).unwrap();

    let wrong_store = ObjectIndexHeaderV1::new(
        StoreId::from_array([0x92; 16]),
        IndexGenerationV1::new(11).unwrap(),
    );
    assert!(matches!(
        PreparedGenerationCheckpointIndexV1::new(wrong_store, address, &prepared),
        Err(crate::rspace::persistence::IndexFormatError::PrepareStore { .. })
    ));

    let wrong_length =
        LogPackAddress::new(29, PACK_HEADER_LEN_V1, prepared.record_len() + 1).unwrap();
    assert!(matches!(
        PreparedGenerationCheckpointIndexV1::new(header, wrong_length, &prepared),
        Err(crate::rspace::persistence::IndexFormatError::Length {
            field: "checkpoint PREPARE address",
            ..
        })
    ));

    for offset in [PACK_HEADER_LEN_V1 - 1, PACK_HEADER_LEN_V1 + 1] {
        let wrong_offset = LogPackAddress::new(29, offset, prepared.record_len()).unwrap();
        assert!(matches!(
            PreparedGenerationCheckpointIndexV1::new(header, wrong_offset, &prepared),
            Err(crate::rspace::persistence::IndexFormatError::CheckpointPrepareOffset {
                expected: PACK_HEADER_LEN_V1,
                actual,
            }) if actual == offset
        ));
    }
}

#[test]
fn v1_checkpoint_records_refuse_the_frozen_v2_descriptor() {
    const PREPARE_DESCRIPTOR_OFFSET: usize = 64;
    const COMMIT_DESCRIPTOR_OFFSET: usize = 152;
    const DESCRIPTOR_LEN: usize = 216;
    const RETENTION_REFERENCE_OFFSET: usize = 96;
    const DATA_OBJECTS_OFFSET: usize = 168;
    const DATA_BYTES_OFFSET: usize = 176;
    let source = fixture();
    let prepare = encode_checkpoint(&source);
    let (commit, _, _, _) = checkpoint_commit_fixture(&source);
    let prepare_descriptor = PREPARE_DESCRIPTOR_OFFSET..PREPARE_DESCRIPTOR_OFFSET + DESCRIPTOR_LEN;
    let commit_descriptor = COMMIT_DESCRIPTOR_OFFSET..COMMIT_DESCRIPTOR_OFFSET + DESCRIPTOR_LEN;
    let descriptor = prepare[prepare_descriptor.clone()].to_vec();
    assert_eq!(descriptor, commit[commit_descriptor.clone()]);
    assert!(GenerationCheckpointDescriptorViewV1::parse(&descriptor).is_ok());
    assert!(
        GenerationCheckpointPrepareRecordViewV1::parse(&prepare, LogPackDecodeLimits::UNBOUNDED)
            .is_ok()
    );
    assert!(GenerationCheckpointCommitViewV1::parse(&commit).is_ok());

    let current = RetainedStateV1::new(
        source.descriptor.state(),
        source.descriptor.current_state_canonical_bytes().get(),
    )
    .expect("the fixture's current closure covers its manifest");
    let anchor = RetentionAnchorV2::new(current, 0, empty_retention_radix_ref())
        .expect("an empty history appoints the empty radix node")
        .object_ref();
    let metadata_bytes = anchor.encoded_len() + empty_retention_radix_ref().encoded_len();
    assert_eq!(metadata_bytes, 186);
    let inventory = source.descriptor.inventory();
    let retained_objects = inventory.retained_union_objects();
    let retained_bytes = inventory.retained_union_canonical_bytes().get();

    let retag = |descriptor: &mut [u8]| {
        descriptor[..16].copy_from_slice(GENERATION_CHECKPOINT_V2_DOMAIN);
    };
    let appoint_v2_inventory = |descriptor: &mut [u8]| {
        descriptor[RETENTION_REFERENCE_OFFSET..RETENTION_REFERENCE_OFFSET + 32]
            .copy_from_slice(anchor.digest().as_bytes());
        replace_u64(descriptor, RETENTION_REFERENCE_OFFSET + 32, anchor.encoded_len());
        replace_u64(descriptor, DATA_OBJECTS_OFFSET, retained_objects + 2);
        replace_u64(descriptor, DATA_BYTES_OFFSET, retained_bytes + metadata_bytes);
    };
    let invalid_domain =
        GenerationCheckpointErrorV1::Canonical(CanonicalFormatError::InvalidDomain {
            expected: GENERATION_CHECKPOINT_DOMAIN,
        });
    let incoherent = GenerationCheckpointErrorV1::InventoryIncoherent {
        expected_objects: retained_objects + 1,
        actual_objects: retained_objects + 2,
        expected_bytes: retained_bytes + anchor.encoded_len(),
        actual_bytes: retained_bytes + metadata_bytes,
    };
    let mutations: [(&dyn Fn(&mut [u8]), GenerationCheckpointErrorV1); 3] = [
        (&retag, invalid_domain.clone()),
        (&appoint_v2_inventory, incoherent),
        (
            &|descriptor: &mut [u8]| {
                appoint_v2_inventory(descriptor);
                retag(descriptor);
            },
            invalid_domain,
        ),
    ];
    for (mutate, expected) in mutations {
        let mut changed = descriptor.clone();
        mutate(&mut changed);
        assert_eq!(
            GenerationCheckpointDescriptorViewV1::parse(&changed).err(),
            Some(expected.clone())
        );

        let mut changed = prepare.clone();
        mutate(&mut changed[prepare_descriptor.clone()]);
        reseal_prepare(&mut changed);
        assert_eq!(
            GenerationCheckpointPrepareRecordViewV1::parse(
                &changed,
                LogPackDecodeLimits::UNBOUNDED
            )
            .err(),
            Some(LogPackFormatError::Checkpoint(expected.clone()))
        );

        let mut changed = commit.clone();
        mutate(&mut changed[commit_descriptor.clone()]);
        rewrite_commit_crc(&mut changed);
        assert_eq!(
            GenerationCheckpointCommitViewV1::parse(&changed).err(),
            Some(LogPackFormatError::Checkpoint(expected))
        );
    }
}

const PREPARE_DESCRIPTOR_EXTENT: std::ops::Range<usize> = 64..280;
const COMMIT_DESCRIPTOR_EXTENT: std::ops::Range<usize> = 152..368;
const COMMIT_INDEX_TAIL_OFFSET: usize = 368;
const DESCRIPTOR_VERSION_OFFSET: usize = 16;
const DESCRIPTOR_RETENTION_OFFSET: usize = 96;
const DESCRIPTOR_DATA_OBJECTS_OFFSET: usize = 168;
const DESCRIPTOR_DATA_BYTES_OFFSET: usize = 176;

/// Every accessor of one checkpoint COMMIT view, in record order.
type CommitViewFields = (
    LogPackSequence,
    StoreId,
    PhysicalDigest,
    LogPackAddress,
    PhysicalDigest,
    GenerationCheckpointDescriptorViewV1,
    CommitPublicationV1,
    PhysicalDigest,
);

type ReaderOutcome = (
    Result<GenerationCheckpointDescriptorViewV1, GenerationCheckpointErrorV1>,
    Result<GenerationCheckpointPrepareIdentityV1, LogPackFormatError>,
    Result<CommitViewFields, LogPackFormatError>,
);

/// The unchanged strict entry points, and the two retention readers.
#[derive(Clone, Copy, Debug)]
enum RecordReader {
    Strict,
    V1Only,
    Dual,
}

impl RecordReader {
    const ALL: [Self; 3] = [Self::Strict, Self::V1Only, Self::Dual];

    fn descriptor(
        self,
        bytes: &[u8],
    ) -> Result<GenerationCheckpointDescriptorViewV1, GenerationCheckpointErrorV1> {
        match self {
            Self::Strict => GenerationCheckpointDescriptorViewV1::parse(bytes),
            Self::V1Only => {
                GenerationCheckpointDescriptorViewV1::parse_for(bytes, RetentionReader::V1Only)
            }
            Self::Dual => {
                GenerationCheckpointDescriptorViewV1::parse_for(bytes, RetentionReader::Dual)
            }
        }
    }

    fn prepare(
        self,
        bytes: &[u8],
    ) -> Result<GenerationCheckpointPrepareIdentityV1, LogPackFormatError> {
        let limits = LogPackDecodeLimits::UNBOUNDED;
        match self {
            Self::Strict => GenerationCheckpointPrepareRecordViewV1::parse(bytes, limits),
            Self::V1Only => GenerationCheckpointPrepareRecordViewV1::parse_for(
                bytes,
                limits,
                RetentionReader::V1Only,
            ),
            Self::Dual => GenerationCheckpointPrepareRecordViewV1::parse_for(
                bytes,
                limits,
                RetentionReader::Dual,
            ),
        }
        .map(GenerationCheckpointPrepareIdentityV1::from_record_view)
    }

    fn commit(self, bytes: &[u8]) -> Result<CommitViewFields, LogPackFormatError> {
        match self {
            Self::Strict => GenerationCheckpointCommitViewV1::parse(bytes),
            Self::V1Only => {
                GenerationCheckpointCommitViewV1::parse_for(bytes, RetentionReader::V1Only)
            }
            Self::Dual => GenerationCheckpointCommitViewV1::parse_for(bytes, RetentionReader::Dual),
        }
        .map(|view| {
            (
                view.sequence(),
                view.store_id(),
                view.previous_commit(),
                view.prepare_address(),
                view.prepare_digest(),
                view.descriptor(),
                view.publication(),
                view.digest(),
            )
        })
    }

    fn read(self, images: &CheckpointRecordImages) -> ReaderOutcome {
        (
            self.descriptor(&images.descriptor),
            self.prepare(&images.prepare),
            self.commit(&images.commit),
        )
    }
}

/// A checkpoint's descriptor and the PREPARE and COMMIT records embedding it.
#[derive(Clone)]
struct CheckpointRecordImages {
    descriptor: Vec<u8>,
    prepare: Vec<u8>,
    commit: Vec<u8>,
    writer_identity: GenerationCheckpointPrepareIdentityV1,
}

impl CheckpointRecordImages {
    fn of(source: &SyntheticCheckpointSource) -> Self {
        let (prepare, writer_identity) = encode_checkpoint_with_identity(source);
        let (commit, ..) = checkpoint_commit_fixture(source);
        Self {
            descriptor: prepare[PREPARE_DESCRIPTOR_EXTENT].to_vec(),
            prepare,
            commit,
            writer_identity,
        }
    }

    /// Apply one descriptor edit to the descriptor and to both records, then
    /// reseal the record checksums so the edit reaches the descriptor parser.
    fn edited(&self, edit: impl Fn(&mut [u8])) -> Self {
        let mut images = self.clone();
        edit(&mut images.descriptor);
        edit(&mut images.prepare[PREPARE_DESCRIPTOR_EXTENT]);
        reseal_prepare(&mut images.prepare);
        edit(&mut images.commit[COMMIT_DESCRIPTOR_EXTENT]);
        rewrite_commit_crc(&mut images.commit);
        images
    }
}

fn prepare_record_len(descriptor: GenerationCheckpointDescriptorViewV1) -> u64 {
    GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1 +
        40 * descriptor.data_objects() +
        descriptor.data_canonical_bytes().get() +
        8
}

fn index_tail(data_objects: u64) -> u64 { INDEX_HEADER_LEN_V1 + INDEX_RECORD_LEN_V1 * data_objects }

fn reencoded(descriptor: GenerationCheckpointDescriptorViewV1) -> Vec<u8> {
    GenerationCheckpointDescriptorV1::from_view(descriptor)
        .try_canonical_bytes()
        .expect("a parsed descriptor re-encodes")
}

fn invalid_v1_domain() -> GenerationCheckpointErrorV1 {
    GenerationCheckpointErrorV1::Canonical(CanonicalFormatError::InvalidDomain {
        expected: GENERATION_CHECKPOINT_DOMAIN,
    })
}

fn check_v1_reader_parity(images: &CheckpointRecordImages) -> Result<(), TestCaseError> {
    let strict = RecordReader::Strict.read(images);
    let (descriptor, prepare, commit) = &strict;
    prop_assert_eq!(descriptor.as_ref().map(|view| view.format()), Ok(RetentionFormat::V1));
    prop_assert_eq!(prepare, &Ok(images.writer_identity));
    prop_assert!(commit.is_ok(), "strict COMMIT refusal: {:?}", commit);
    for reader in [RecordReader::V1Only, RecordReader::Dual] {
        prop_assert_eq!(&reader.read(images), &strict, "{:?}", reader);
    }
    Ok(())
}

fn check_shared_refusal(
    images: &CheckpointRecordImages,
    reader: RecordReader,
    expected: &GenerationCheckpointErrorV1,
) -> Result<(), TestCaseError> {
    let (descriptor, prepare, commit) = reader.read(images);
    prop_assert_eq!(descriptor.err(), Some(expected.clone()), "{:?} descriptor", reader);
    prop_assert_eq!(
        prepare.err(),
        Some(LogPackFormatError::Checkpoint(expected.clone())),
        "{:?} PREPARE",
        reader
    );
    prop_assert_eq!(
        commit.err(),
        Some(LogPackFormatError::Checkpoint(expected.clone())),
        "{:?} COMMIT",
        reader
    );
    Ok(())
}

/// V1Only is the strict reader; Dual is the strict reader outside the V2
/// domain; and under every reader a record refuses exactly with its
/// descriptor's refusal, or carries the descriptor the reader parsed.
fn check_reader_composition(images: &CheckpointRecordImages) -> Result<(), TestCaseError> {
    let strict = RecordReader::Strict.read(images);
    prop_assert_eq!(&RecordReader::V1Only.read(images), &strict);
    let dual = RecordReader::Dual.read(images);
    if !images
        .descriptor
        .starts_with(GENERATION_CHECKPOINT_V2_DOMAIN)
    {
        prop_assert_eq!(&dual, &strict);
    }
    for (descriptor, prepare, commit) in [strict, dual] {
        match descriptor {
            Err(error) => {
                prop_assert_eq!(prepare.err(), Some(LogPackFormatError::Checkpoint(error.clone())));
                prop_assert_eq!(commit.err(), Some(LogPackFormatError::Checkpoint(error)));
            }
            Ok(view) => {
                if let Ok(identity) = prepare {
                    prop_assert_eq!(identity.descriptor().as_view(), view);
                }
                if let Ok(fields) = commit {
                    prop_assert_eq!(fields.5, view);
                }
            }
        }
    }
    Ok(())
}

fn check_versioned_checkpoint_codec(
    retained_union_objects: usize,
    corrupted_offset: usize,
    corruption: u8,
) -> Result<(), TestCaseError> {
    let v1_source = fixture_with_retained_union_cardinality(retained_union_objects);
    let v2_source = migrate_checkpoint_source_to_v2(&v1_source);
    let v1 = CheckpointRecordImages::of(&v1_source);
    let v2 = CheckpointRecordImages::of(&v2_source);
    check_v1_reader_parity(&v1)?;

    let manifest = v1_source.descriptor.retention_manifest();
    let retained = v1_source
        .objects
        .iter()
        .map(|(reference, _)| *reference)
        .filter(|reference| *reference != manifest)
        .collect::<Vec<_>>();
    let retained_digests = retained
        .iter()
        .map(|reference| reference.digest())
        .collect::<BTreeSet<_>>();
    let metadata = v2_source
        .objects
        .iter()
        .map(|(reference, _)| *reference)
        .filter(|reference| !retained_digests.contains(&reference.digest()))
        .collect::<Vec<_>>();
    let canonical_bytes = |references: &[ObjectRefV1]| {
        references
            .iter()
            .map(|reference| reference.encoded_len())
            .sum::<u64>()
    };
    let mut merged = Vec::with_capacity(retained.len() + metadata.len());
    merged.extend_from_slice(&retained);
    merged.extend_from_slice(&metadata);
    merged.sort_unstable_by_key(|reference| reference.digest());
    prop_assert!(metadata.len() as u64 >= RETENTION_METADATA_MIN_OBJECTS);
    prop_assert!(canonical_bytes(&metadata) >= RETENTION_METADATA_MIN_CANONICAL_BYTES);
    prop_assert!(
        merged
            .windows(2)
            .all(|pair| pair[0].digest() < pair[1].digest())
    );

    let prepare = match GenerationCheckpointPrepareRecordViewV1::parse_for(
        &v2.prepare,
        LogPackDecodeLimits::UNBOUNDED,
        RetentionReader::Dual,
    ) {
        Ok(prepare) => prepare,
        Err(error) => {
            return Err(TestCaseError::fail(format!("dual PREPARE refusal: {error:?}")));
        }
    };
    let listed = prepare
        .objects()
        .map(|object| object.reference())
        .collect::<Vec<_>>();
    prop_assert_eq!(&listed, &merged);
    let descriptor = prepare.preflight().descriptor();
    prop_assert_eq!(descriptor.format(), RetentionFormat::V2);
    prop_assert_eq!(descriptor.retention_manifest().encoded_len(), RETENTION_ANCHOR_CANONICAL_LEN);
    prop_assert_eq!(descriptor.retained_union_objects(), retained.len() as u64);
    prop_assert_eq!(descriptor.retained_union_canonical_bytes().get(), canonical_bytes(&retained));
    prop_assert_eq!(descriptor.data_objects(), merged.len() as u64);
    prop_assert_eq!(
        descriptor.data_canonical_bytes().get(),
        canonical_bytes(&retained) + canonical_bytes(&metadata)
    );
    prop_assert_eq!(descriptor.ordered_digest(), independent_inventory_digest(&merged));
    prop_assert_eq!(reencoded(descriptor), v2.descriptor.clone());
    prop_assert_eq!(
        GenerationCheckpointPrepareIdentityV1::from_record_view(prepare),
        v2.writer_identity
    );
    let commit =
        match GenerationCheckpointCommitViewV1::parse_for(&v2.commit, RetentionReader::Dual) {
            Ok(commit) => commit,
            Err(error) => {
                return Err(TestCaseError::fail(format!("dual COMMIT refusal: {error:?}")));
            }
        };
    prop_assert_eq!(commit.descriptor(), descriptor);
    prop_assert_eq!(
        commit.publication().durable_index_tail(),
        index_tail(descriptor.data_objects())
    );
    prop_assert_eq!(commit.verify_prepare(v2.writer_identity), Ok(()));
    for reader in [RecordReader::Strict, RecordReader::V1Only] {
        check_shared_refusal(&v2, reader, &invalid_v1_domain())?;
    }

    let v2_under_v1_domain =
        v2.edited(|bytes| bytes[..16].copy_from_slice(GENERATION_CHECKPOINT_DOMAIN));
    let plus_one_refusal = GenerationCheckpointErrorV1::InventoryIncoherent {
        expected_objects: retained.len() as u64 + 1,
        actual_objects: descriptor.data_objects(),
        expected_bytes: canonical_bytes(&retained) + RETENTION_ANCHOR_CANONICAL_LEN,
        actual_bytes: descriptor.data_canonical_bytes().get(),
    };
    for reader in RecordReader::ALL {
        check_shared_refusal(&v2_under_v1_domain, reader, &plus_one_refusal)?;
    }

    let historical_states = (retained_union_objects - 2) as u64;
    prop_assert_eq!(manifest.encoded_len(), 96 + 56 * historical_states);
    let v1_under_v2_domain =
        v1.edited(|bytes| bytes[..16].copy_from_slice(GENERATION_CHECKPOINT_V2_DOMAIN));
    check_shared_refusal(
        &v1_under_v2_domain,
        RecordReader::Dual,
        &GenerationCheckpointErrorV1::RetentionAnchorLength {
            actual: manifest.encoded_len(),
        },
    )?;
    for reader in [RecordReader::Strict, RecordReader::V1Only] {
        check_shared_refusal(&v1_under_v2_domain, reader, &invalid_v1_domain())?;
    }

    let mut v1_sized_tail = v2.commit.clone();
    replace_u64(
        &mut v1_sized_tail,
        COMMIT_INDEX_TAIL_OFFSET,
        index_tail(retained.len() as u64 + 1),
    );
    rewrite_commit_crc(&mut v1_sized_tail);
    prop_assert_eq!(
        RecordReader::Dual.commit(&v1_sized_tail).err(),
        Some(LogPackFormatError::CheckpointIndexTail {
            expected: index_tail(descriptor.data_objects()),
            actual: index_tail(retained.len() as u64 + 1),
        })
    );

    for images in [&v1, &v2] {
        check_reader_composition(&images.edited(|bytes| bytes[corrupted_offset] ^= corruption))?;
    }
    Ok(())
}

#[test]
fn versioned_checkpoint_codec_accepts_v2_inventory_only_under_the_dual_reader() {
    for source in [fixture(), semantic_fixture(), historically_larger_semantic_fixture().0] {
        check_v1_reader_parity(&CheckpointRecordImages::of(&source))
            .expect("every reader parses a V1 checkpoint identically");
    }

    let source = fixture();
    let v1 = CheckpointRecordImages::of(&source);
    let inventory = source.descriptor.inventory();
    let retained_objects = inventory.retained_union_objects();
    let retained_bytes = inventory.retained_union_canonical_bytes().get();
    let manifest_len = source.descriptor.retention_manifest().encoded_len();
    assert_eq!(manifest_len, 96);
    let current = RetainedStateV1::new(
        source.descriptor.state(),
        source.descriptor.current_state_canonical_bytes().get(),
    )
    .expect("the fixture's current closure covers its manifest");
    let anchor = RetentionAnchorV2::new(current, 0, empty_retention_radix_ref())
        .expect("an empty history appoints the empty radix node")
        .object_ref();

    let retagged = v1.edited(|bytes| bytes[..16].copy_from_slice(GENERATION_CHECKPOINT_V2_DOMAIN));
    let appointed = v1.edited(|bytes| {
        bytes[DESCRIPTOR_RETENTION_OFFSET..DESCRIPTOR_RETENTION_OFFSET + 32]
            .copy_from_slice(anchor.digest().as_bytes());
        replace_u64(bytes, DESCRIPTOR_RETENTION_OFFSET + 32, anchor.encoded_len());
        replace_u64(
            bytes,
            DESCRIPTOR_DATA_OBJECTS_OFFSET,
            retained_objects + RETENTION_METADATA_MIN_OBJECTS,
        );
        replace_u64(
            bytes,
            DESCRIPTOR_DATA_BYTES_OFFSET,
            retained_bytes + RETENTION_METADATA_MIN_CANONICAL_BYTES,
        );
    });
    let both =
        appointed.edited(|bytes| bytes[..16].copy_from_slice(GENERATION_CHECKPOINT_V2_DOMAIN));
    let incoherent = GenerationCheckpointErrorV1::InventoryIncoherent {
        expected_objects: retained_objects + 1,
        actual_objects: retained_objects + RETENTION_METADATA_MIN_OBJECTS,
        expected_bytes: retained_bytes + anchor.encoded_len(),
        actual_bytes: retained_bytes + RETENTION_METADATA_MIN_CANONICAL_BYTES,
    };
    for reader in [RecordReader::Strict, RecordReader::V1Only] {
        check_shared_refusal(&retagged, reader, &invalid_v1_domain())
            .expect("strict readers refuse the V2 domain");
        check_shared_refusal(&both, reader, &invalid_v1_domain())
            .expect("strict readers refuse the V2 domain");
    }
    for reader in RecordReader::ALL {
        check_shared_refusal(&appointed, reader, &incoherent)
            .expect("the V1 domain keeps the V1 inventory rule under every reader");
    }
    check_shared_refusal(
        &retagged,
        RecordReader::Dual,
        &GenerationCheckpointErrorV1::RetentionAnchorLength {
            actual: manifest_len,
        },
    )
    .expect("a retagged V1 descriptor appoints a manifest, never an anchor");
    let (descriptor, prepare, commit) = RecordReader::Dual.read(&both);
    let view = descriptor.expect("the dual reader accepts the appointed V2 descriptor");
    assert_eq!(view.format(), RetentionFormat::V2);
    assert_eq!(reencoded(view), both.descriptor);
    assert_eq!(prepare_record_len(view), both.prepare.len() as u64 + 40 + 186 - manifest_len);
    assert_eq!(
        prepare.err(),
        Some(LogPackFormatError::LengthMismatch {
            declared: both.prepare.len() as u64,
            actual: prepare_record_len(view),
        })
    );
    assert_eq!(
        commit.err(),
        Some(LogPackFormatError::CheckpointIndexTail {
            expected: index_tail(view.data_objects()),
            actual: index_tail(retained_objects + 1),
        })
    );

    let migrated = CheckpointRecordImages::of(&migrate_checkpoint_source_to_v2(&source));
    let minimum = RecordReader::Dual
        .descriptor(&migrated.descriptor)
        .expect("the migrated minimum parses under the dual reader");
    assert_eq!(minimum.format(), RetentionFormat::V2);
    assert_eq!(minimum.retention_manifest(), anchor);
    assert_eq!(minimum.retained_union_objects(), retained_objects);
    assert_eq!(minimum.retained_union_canonical_bytes().get(), retained_bytes);
    let data_objects = minimum.data_objects();
    let data_bytes = minimum.data_canonical_bytes().get();
    assert_eq!(data_objects, retained_objects + RETENTION_METADATA_MIN_OBJECTS);
    assert_eq!(data_bytes, retained_bytes + RETENTION_METADATA_MIN_CANONICAL_BYTES);
    assert_eq!(reencoded(minimum), migrated.descriptor);
    let below =
        |data_objects, data_bytes| GenerationCheckpointErrorV1::RetentionMetadataBelowMinimum {
            data_objects,
            retained_objects,
            data_bytes,
            retained_bytes,
        };
    let edits = [
        (DESCRIPTOR_DATA_OBJECTS_OFFSET, data_objects + 1, Ok(())),
        (DESCRIPTOR_DATA_BYTES_OFFSET, data_bytes + 1, Ok(())),
        (
            DESCRIPTOR_DATA_OBJECTS_OFFSET,
            data_objects - 1,
            Err(below(data_objects - 1, data_bytes)),
        ),
        (DESCRIPTOR_DATA_BYTES_OFFSET, data_bytes - 1, Err(below(data_objects, data_bytes - 1))),
        (
            DESCRIPTOR_DATA_OBJECTS_OFFSET,
            retained_objects - 1,
            Err(below(retained_objects - 1, data_bytes)),
        ),
        (
            DESCRIPTOR_DATA_BYTES_OFFSET,
            retained_bytes - 1,
            Err(below(data_objects, retained_bytes - 1)),
        ),
        (
            DESCRIPTOR_RETENTION_OFFSET + 32,
            RETENTION_ANCHOR_CANONICAL_LEN - 1,
            Err(GenerationCheckpointErrorV1::RetentionAnchorLength {
                actual: RETENTION_ANCHOR_CANONICAL_LEN - 1,
            }),
        ),
        (
            DESCRIPTOR_RETENTION_OFFSET + 32,
            RETENTION_ANCHOR_CANONICAL_LEN + 1,
            Err(GenerationCheckpointErrorV1::RetentionAnchorLength {
                actual: RETENTION_ANCHOR_CANONICAL_LEN + 1,
            }),
        ),
        (
            DESCRIPTOR_VERSION_OFFSET,
            2,
            Err(GenerationCheckpointErrorV1::Canonical(CanonicalFormatError::UnsupportedVersion {
                domain: "RSpace generation checkpoint V2",
                version: 2,
            })),
        ),
    ];
    for (offset, value, expected) in edits {
        let edited = migrated.edited(|bytes| replace_u64(bytes, offset, value));
        for reader in [RecordReader::Strict, RecordReader::V1Only] {
            check_shared_refusal(&edited, reader, &invalid_v1_domain())
                .expect("strict readers refuse every V2 descriptor edit at the domain");
        }
        check_reader_composition(&edited).expect("records refuse with their descriptor");
        match (RecordReader::Dual.read(&edited), expected) {
            ((Err(error), _, _), Err(expected)) => assert_eq!(error, expected),
            ((Ok(view), prepare, commit), Ok(())) => {
                assert_eq!(reencoded(view), edited.descriptor);
                assert_eq!(
                    prepare.err(),
                    Some(LogPackFormatError::LengthMismatch {
                        declared: edited.prepare.len() as u64,
                        actual: prepare_record_len(view),
                    })
                );
                if view.data_objects() == data_objects {
                    let commit = GenerationCheckpointCommitViewV1::parse_for(
                        &edited.commit,
                        RetentionReader::Dual,
                    )
                    .expect("a COMMIT lower-bounds the metadata it does not carry");
                    assert_eq!(
                        commit.verify_prepare(migrated.writer_identity),
                        Err(LogPackFormatError::CheckpointDescriptorMismatch)
                    );
                } else {
                    assert_eq!(
                        commit.err(),
                        Some(LogPackFormatError::CheckpointIndexTail {
                            expected: index_tail(view.data_objects()),
                            actual: index_tail(data_objects),
                        })
                    );
                }
            }
            (outcome, expected) => {
                panic!(
                    "descriptor offset {offset} set to {value}: {outcome:?} against {expected:?}"
                )
            }
        }
    }

    let impostor = CanonicalDagNode::new(vec![0xA5_u8; 4], EndpointValue::Absent, vec![
        DagChild {
            edge: 0x10,
            target: synthetic_ref(0x10),
        },
        DagChild {
            edge: 0x20,
            target: synthetic_ref(0x20),
        },
    ])
    .expect("a two-child node with an absent endpoint is canonical");
    let (impostor_reference, impostor_bytes) = encoded(&impostor);
    assert_eq!(impostor_reference.encoded_len(), RETENTION_ANCHOR_CANONICAL_LEN);
    let impostor_images =
        CheckpointRecordImages::of(&v2_source_from_parts(&source, impostor_bytes, vec![
            EMPTY_RETENTION_RADIX_NODE.to_vec(),
        ]));
    assert!(
        RecordReader::Dual
            .descriptor(&impostor_images.descriptor)
            .is_ok()
    );
    assert_eq!(
        RecordReader::Dual.prepare(&impostor_images.prepare).err(),
        Some(LogPackFormatError::CheckpointDesignatedObjectKind {
            role: "retention anchor",
            reference: impostor_reference,
            expected: CanonicalObjectKind::RetentionAnchor,
            actual: CanonicalObjectKind::DagNode,
        })
    );

    TestRunner::new(ProptestConfig {
        cases: 64,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
    .run(
        &(2_usize..=10, 0..PREPARE_DESCRIPTOR_EXTENT.len(), 1_u8..=u8::MAX),
        |(retained_union_objects, corrupted_offset, corruption)| {
            check_versioned_checkpoint_codec(retained_union_objects, corrupted_offset, corruption)
        },
    )
    .expect("the versioned checkpoint codec refines the V2 inventory rule");
}
