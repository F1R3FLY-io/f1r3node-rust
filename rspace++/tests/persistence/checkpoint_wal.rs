use std::convert::Infallible;

use super::support::TestSemanticProfile;
use crate::rspace::persistence::checkpoint::{
    CheckpointObjectInventoryV1, CheckpointReferenceVisitErrorV1, GenerationCheckpointDescriptorV1,
    GenerationCheckpointErrorV1, OrderedObjectInventoryBuilderV1,
};
use crate::rspace::persistence::closure::RetainedUnionClosureReportV1;
use crate::rspace::persistence::durable::{
    INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1, IndexGenerationV1, ObjectIndexHeaderV1,
    PACK_HEADER_LEN_V1, PreparedGenerationCheckpointIndexV1,
    PreparedGenerationCheckpointIndexVisitErrorV1,
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
    COMMIT_DISCRIMINATOR_LEN_V1, CanonicalDagNode, CanonicalEncode, CanonicalSink,
    CommitPublicationV1, CommitRecordKindV1, LogPackAddress, LogPackDecodeLimits,
    LogPackFormatError, LogPackSequence, ObjectDigest, ObjectRefV1, PREPARE_DISCRIMINATOR_LEN_V1,
    PhysicalDigest, PrepareRecordKindV1, RetainedStateV1, RetentionManifestV1, SemanticProfileId,
    SemanticRoots, SemanticStateManifestV1, StateVersionV1, StoreId, decode_commit_kind_v1,
    decode_prepare_kind_v1,
};

const PROFILE: SemanticProfileId = SemanticProfileId::from_array(*b"RSpace.Test.V1\0\0");

#[derive(Clone)]
pub(super) struct SyntheticCheckpointSource {
    pub(super) descriptor: GenerationCheckpointDescriptorV1,
    pub(super) objects: Vec<(ObjectRefV1, Vec<u8>)>,
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
        let bytes = self
            .objects
            .iter()
            .find_map(|(candidate, bytes)| (*candidate == reference).then_some(bytes))
            .expect("the static test inventory contains every visited object");
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

pub(super) fn malformed_interior_fixture() -> SyntheticCheckpointSource {
    let mut bytes = b"RSpaceDAG.Node\0\0".to_vec();
    bytes.resize(32, 0);
    let reference =
        ObjectRefV1::new(ObjectDigest::of_canonical_bytes(&bytes), bytes.len() as u64).unwrap();
    fixture_with_root((reference, bytes))
}

fn fixture_with_root(root: (ObjectRefV1, Vec<u8>)) -> SyntheticCheckpointSource {
    let roots = SemanticRoots {
        data_bag: root.0,
        continuation_map: root.0,
        join_bag: root.0,
    };
    let state_manifest = encoded(&SemanticStateManifestV1::new(1, PROFILE, roots, None).unwrap());
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
        PROFILE,
        state,
        retention_reference,
        current_bytes,
        inventory,
    )
    .unwrap();
    SyntheticCheckpointSource {
        descriptor,
        objects,
    }
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
        "len=763;header=52537061636557414c2e50726570000001000000000000000300000000000000fb020000000000001300000000000000919191919191919191919191919191915253706163654441472e43686b7074000100000000000000b0000000000000005253706163652e546573742e56310000070000000000000008a2bad5d01cd13ce6c39000aa50c93509172aebb94fed4f47858b8414404bcfd10000000000000039f726adc51675b622380f86cd0388fe825d30d66d5eaaa2f2ba1da32e404916600000000000000003010000000000000301000000000000020000000000000003000000000000006301000000000000bbb31f85f5194118002f2c656ad3790578f13c11efa309593ee28e4da308651bc0f3675600000000;digest=9b646d05f070db6922cf0dcee759a28c1c3c205629ad48a8a7d6b17fd61bff34;crc=7708862c"
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
        "len=392;bytes=52537061636557414c2e436f6d6d00000100000000000000040000000000000088010000000000001300000000000000919191919191919191919191919191917a6782fd8b272fc8a88eb7f987fa2aac6676db0a8c71f7e85ecc7bf45a488b5f1d000000000000008000000000000000fb020000000000009b646d05f070db6922cf0dcee759a28c1c3c205629ad48a8a7d6b17fd61bff345253706163654441472e43686b7074000100000000000000b0000000000000005253706163652e546573742e56310000070000000000000008a2bad5d01cd13ce6c39000aa50c93509172aebb94fed4f47858b8414404bcfd10000000000000039f726adc51675b622380f86cd0388fe825d30d66d5eaaa2f2ba1da32e404916600000000000000003010000000000000301000000000000020000000000000003000000000000006301000000000000bbb31f85f5194118002f2c656ad3790578f13c11efa309593ee28e4da308651b18010000000000001d000000000000000b00000000000000320cdf5300000000;digest=6594d48d9387f05e22699da3787bdb56725599eb8550bb3c4cc4d4db4f9ece9b"
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

    let mut corrupt = bytes.clone();
    corrupt[120] ^= 1;
    assert!(matches!(
        GenerationCheckpointCommitViewV1::parse(&corrupt),
        Err(LogPackFormatError::RecordChecksum { .. })
    ));

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
