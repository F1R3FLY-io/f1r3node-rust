use std::convert::Infallible;

use super::support::TestSemanticProfile;
use crate::rspace::persistence::checkpoint::{
    CheckpointObjectInventoryV1, CheckpointReferenceVisitErrorV1, GenerationCheckpointDescriptorV1,
    GenerationCheckpointErrorV1, OrderedObjectInventoryBuilderV1,
};
use crate::rspace::persistence::closure::RetainedUnionClosureReportV1;
use crate::rspace::persistence::wal::{
    GENERATION_CHECKPOINT_PREPARE_HEADER_LEN_V1, GenerationCheckpointPrepareRecordViewV1,
    GenerationCheckpointSourceCopyErrorV1, GenerationCheckpointSourceTestSealV1,
    GenerationCheckpointSourceV1, PreparedGenerationCheckpointV1,
    preflight_generation_checkpoint_prepare_v1,
};
use crate::rspace::persistence::{
    CanonicalDagNode, CanonicalEncode, CanonicalSink, LogPackDecodeLimits, LogPackFormatError,
    LogPackSequence, ObjectDigest, ObjectRefV1, PREPARE_DISCRIMINATOR_LEN_V1, PrepareRecordKindV1,
    RetainedStateV1, RetentionManifestV1, SemanticProfileId, SemanticRoots,
    SemanticStateManifestV1, StateVersionV1, StoreId, decode_prepare_kind_v1,
};

const PROFILE: SemanticProfileId = SemanticProfileId::from_array(*b"RSpace.Test.V1\0\0");

#[derive(Clone)]
struct SyntheticCheckpointSource {
    descriptor: GenerationCheckpointDescriptorV1,
    objects: Vec<(ObjectRefV1, Vec<u8>)>,
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

fn fixture() -> SyntheticCheckpointSource {
    let empty = encoded(&CanonicalDagNode::empty());
    let roots = SemanticRoots {
        data_bag: empty.0,
        continuation_map: empty.0,
        join_bag: empty.0,
    };
    let state_manifest = encoded(&SemanticStateManifestV1::new(1, PROFILE, roots, None).unwrap());
    let state = StateVersionV1::new(7, state_manifest.0);
    let current_bytes = empty.0.encoded_len() + state_manifest.0.encoded_len();
    let retention_manifest = encoded(
        &RetentionManifestV1::new(RetainedStateV1::new(state, current_bytes).unwrap(), Vec::new())
            .unwrap(),
    );
    let mut objects = vec![empty, state_manifest, retention_manifest];
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
    bytes
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
