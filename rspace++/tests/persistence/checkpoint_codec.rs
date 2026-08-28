use crate::rspace::persistence::checkpoint::{
    CheckpointObjectInventoryV1, GenerationCheckpointDescriptorV1,
    GenerationCheckpointDescriptorViewV1, GenerationCheckpointErrorV1,
    OrderedObjectInventoryBuilderV1,
};
use crate::rspace::persistence::closure::RetainedUnionClosureReportV1;
use crate::rspace::persistence::{
    CanonicalEncode, ObjectDigest, ObjectRefV1, SemanticProfileId, StateVersionV1,
};

fn reference(byte: u8, encoded_len: u64) -> ObjectRefV1 {
    ObjectRefV1::new(ObjectDigest::from_array([byte; 32]), encoded_len).unwrap()
}

fn inventory(
    first: ObjectRefV1,
    retention: ObjectRefV1,
    last: ObjectRefV1,
) -> CheckpointObjectInventoryV1 {
    let expected_bytes = first.encoded_len() + retention.encoded_len() + last.encoded_len();
    let mut ordered = OrderedObjectInventoryBuilderV1::new(3, expected_bytes);
    ordered.push(first).unwrap();
    ordered.push(retention).unwrap();
    ordered.push(last).unwrap();
    CheckpointObjectInventoryV1::from_test_ordered(
        RetainedUnionClosureReportV1 {
            distinct_objects: 2,
            canonical_bytes: first.encoded_len() + last.encoded_len(),
        },
        retention,
        ordered.finish().unwrap(),
    )
    .unwrap()
}

#[test]
fn checkpoint_descriptor_round_trips_exact_inventory_authority() {
    let first = reference(1, 41);
    let retention = reference(2, 42);
    let last = reference(3, 43);
    let descriptor = GenerationCheckpointDescriptorV1::from_test_inventory(
        SemanticProfileId::from_array(*b"RSpace.Test.V1\0\0"),
        StateVersionV1::new(17, first),
        retention,
        1,
        first.encoded_len(),
        inventory(first, retention, last),
    )
    .unwrap();

    let bytes = descriptor.try_canonical_bytes().unwrap();
    assert_eq!(bytes.len(), 216);
    let decoded = GenerationCheckpointDescriptorViewV1::parse(&bytes).unwrap();
    assert_eq!(decoded.profile_id(), descriptor.profile_id());
    assert_eq!(decoded.state(), descriptor.state());
    assert_eq!(decoded.retention_manifest(), descriptor.retention_manifest());
    assert_eq!(decoded.current_state_objects(), descriptor.current_state_objects());
    assert_eq!(decoded.current_state_canonical_bytes(), descriptor.current_state_canonical_bytes());
    assert_eq!(
        decoded.retained_union_canonical_bytes(),
        descriptor.inventory().retained_union_canonical_bytes()
    );
    assert_eq!(decoded.retained_union_objects(), descriptor.inventory().retained_union_objects());
    assert_eq!(decoded.data_objects(), descriptor.inventory().data_objects());
    assert_eq!(decoded.data_canonical_bytes(), descriptor.inventory().data_canonical_bytes());
    assert_eq!(decoded.ordered_digest(), descriptor.inventory().ordered_digest());
}

#[test]
fn inventory_builder_rejects_duplicates_and_out_of_order_references() {
    let first = reference(1, 41);
    let second = reference(2, 42);
    let mut duplicate = OrderedObjectInventoryBuilderV1::new(2, 140);
    duplicate.push(first).unwrap();
    assert!(matches!(
        duplicate.push(reference(1, 99)),
        Err(GenerationCheckpointErrorV1::DuplicateInventoryDigest { .. })
    ));

    let mut descending = OrderedObjectInventoryBuilderV1::new(2, 83);
    descending.push(second).unwrap();
    assert!(matches!(
        descending.push(first),
        Err(GenerationCheckpointErrorV1::InventoryNotStrict { .. })
    ));

    let mut incomplete = OrderedObjectInventoryBuilderV1::new(2, 83);
    incomplete.push(first).unwrap();
    assert!(matches!(
        incomplete.finish(),
        Err(GenerationCheckpointErrorV1::InventoryIncoherent { .. })
    ));
}

#[test]
fn trusted_descriptor_construction_enforces_manifest_and_nonempty_closure_floors() {
    let state = reference(1, 41);
    let retention = reference(2, 42);
    let union = reference(3, 43);
    let valid_inventory = inventory(state, retention, union);
    assert!(matches!(
        GenerationCheckpointDescriptorV1::from_test_inventory(
            SemanticProfileId::from_array(*b"RSpace.Test.V1\0\0"),
            StateVersionV1::new(17, state),
            retention,
            1,
            state.encoded_len() - 1,
            valid_inventory,
        ),
        Err(GenerationCheckpointErrorV1::CurrentBelowManifest { .. })
    ));

    let mut retention_only = OrderedObjectInventoryBuilderV1::new(1, retention.encoded_len());
    retention_only.push(retention).unwrap();
    assert!(matches!(
        CheckpointObjectInventoryV1::from_test_ordered(
            RetainedUnionClosureReportV1 {
                distinct_objects: 0,
                canonical_bytes: 0,
            },
            retention,
            retention_only.finish().unwrap(),
        ),
        Err(GenerationCheckpointErrorV1::EmptyRetainedUnion)
    ));
}

#[test]
fn parser_rejects_incoherent_data_inventory_before_publication() {
    let state = reference(1, 41);
    let retention = reference(2, 42);
    let union = reference(3, 43);
    let descriptor = GenerationCheckpointDescriptorV1::from_test_inventory(
        SemanticProfileId::from_array(*b"RSpace.Test.V1\0\0"),
        StateVersionV1::new(17, state),
        retention,
        1,
        state.encoded_len(),
        inventory(state, retention, union),
    )
    .unwrap();
    let mut bytes = descriptor.try_canonical_bytes().unwrap();
    let data_count_offset = 16 + 8 + 8 + 16 + 8 + 40 + 40 + 8 + 8 + 8 + 8;
    bytes[data_count_offset..data_count_offset + 8].copy_from_slice(&99_u64.to_le_bytes());
    assert!(matches!(
        GenerationCheckpointDescriptorViewV1::parse(&bytes),
        Err(GenerationCheckpointErrorV1::InventoryIncoherent { .. })
    ));
}

#[test]
fn parser_rejects_current_inventory_outside_the_retained_union() {
    let state = reference(1, 41);
    let retention = reference(2, 42);
    let union = reference(3, 43);
    let descriptor = GenerationCheckpointDescriptorV1::from_test_inventory(
        SemanticProfileId::from_array(*b"RSpace.Test.V1\0\0"),
        StateVersionV1::new(17, state),
        retention,
        1,
        state.encoded_len(),
        inventory(state, retention, union),
    )
    .unwrap();
    let bytes = descriptor.try_canonical_bytes().unwrap();
    let current_objects_offset = 16 + 8 + 8 + 16 + 8 + 40 + 40;

    let mut zero = bytes.clone();
    zero[current_objects_offset..current_objects_offset + 8].copy_from_slice(&0_u64.to_le_bytes());
    assert!(matches!(
        GenerationCheckpointDescriptorViewV1::parse(&zero),
        Err(GenerationCheckpointErrorV1::CurrentObjectsBelowManifest { actual: 0 })
    ));

    let mut exceeds_retained = bytes;
    exceeds_retained[current_objects_offset..current_objects_offset + 8]
        .copy_from_slice(&3_u64.to_le_bytes());
    assert!(matches!(
        GenerationCheckpointDescriptorViewV1::parse(&exceeds_retained),
        Err(GenerationCheckpointErrorV1::CurrentObjectsExceedRetained {
            current: 3,
            retained: 2,
        })
    ));

    let current_bytes_offset = current_objects_offset + 8;
    let mut below_manifest = descriptor.try_canonical_bytes().unwrap();
    below_manifest[current_bytes_offset..current_bytes_offset + 8]
        .copy_from_slice(&(state.encoded_len() - 1).to_le_bytes());
    assert!(matches!(
        GenerationCheckpointDescriptorViewV1::parse(&below_manifest),
        Err(GenerationCheckpointErrorV1::CurrentBelowManifest { .. })
    ));

    let mut exceeds_retained_bytes = descriptor.try_canonical_bytes().unwrap();
    exceeds_retained_bytes[current_bytes_offset..current_bytes_offset + 8]
        .copy_from_slice(&(state.encoded_len() + union.encoded_len() + 1).to_le_bytes());
    assert!(matches!(
        GenerationCheckpointDescriptorViewV1::parse(&exceeds_retained_bytes),
        Err(GenerationCheckpointErrorV1::CurrentExceedsRetained { .. })
    ));
}

#[test]
fn descriptor_arithmetic_overflow_fails_closed() {
    let state = reference(1, 41);
    let retention = reference(2, 42);
    let union = reference(3, 43);
    let descriptor = GenerationCheckpointDescriptorV1::from_test_inventory(
        SemanticProfileId::from_array(*b"RSpace.Test.V1\0\0"),
        StateVersionV1::new(17, state),
        retention,
        1,
        state.encoded_len(),
        inventory(state, retention, union),
    )
    .unwrap();
    let bytes = descriptor.try_canonical_bytes().unwrap();
    let retained_bytes_offset = 16 + 8 + 8 + 16 + 8 + 40 + 40 + 8 + 8;
    let retained_objects_offset = retained_bytes_offset + 8;

    let mut object_overflow = bytes.clone();
    object_overflow[retained_objects_offset..retained_objects_offset + 8]
        .copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(matches!(
        GenerationCheckpointDescriptorViewV1::parse(&object_overflow),
        Err(GenerationCheckpointErrorV1::CounterOverflow("checkpoint DATA objects"))
    ));

    let mut byte_overflow = bytes;
    byte_overflow[retained_bytes_offset..retained_bytes_offset + 8]
        .copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(matches!(
        GenerationCheckpointDescriptorViewV1::parse(&byte_overflow),
        Err(GenerationCheckpointErrorV1::Canonical(_))
    ));
}
