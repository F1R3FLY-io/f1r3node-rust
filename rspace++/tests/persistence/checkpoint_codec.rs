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
    let mut ordered = OrderedObjectInventoryBuilderV1::new();
    ordered.push(first).unwrap();
    ordered.push(retention).unwrap();
    ordered.push(last).unwrap();
    CheckpointObjectInventoryV1::from_ordered(
        RetainedUnionClosureReportV1 {
            distinct_objects: 2,
            canonical_bytes: first.encoded_len() + last.encoded_len(),
        },
        retention,
        ordered.finish(),
    )
    .unwrap()
}

#[test]
fn checkpoint_descriptor_round_trips_exact_inventory_authority() {
    let first = reference(1, 41);
    let retention = reference(2, 42);
    let last = reference(3, 43);
    let descriptor = GenerationCheckpointDescriptorV1::new(
        SemanticProfileId::from_array(*b"RSpace.Test.V1\0\0"),
        StateVersionV1::new(17, first),
        retention,
        first.encoded_len(),
        inventory(first, retention, last),
    )
    .unwrap();

    let bytes = descriptor.try_canonical_bytes().unwrap();
    assert_eq!(bytes.len(), 208);
    let decoded = GenerationCheckpointDescriptorViewV1::parse(&bytes)
        .unwrap()
        .descriptor();
    assert_eq!(decoded, descriptor);
}

#[test]
fn inventory_builder_rejects_duplicates_and_out_of_order_references() {
    let first = reference(1, 41);
    let second = reference(2, 42);
    let mut duplicate = OrderedObjectInventoryBuilderV1::new();
    duplicate.push(first).unwrap();
    assert!(matches!(
        duplicate.push(reference(1, 99)),
        Err(GenerationCheckpointErrorV1::DuplicateInventoryDigest { .. })
    ));

    let mut descending = OrderedObjectInventoryBuilderV1::new();
    descending.push(second).unwrap();
    assert!(matches!(
        descending.push(first),
        Err(GenerationCheckpointErrorV1::InventoryNotStrict { .. })
    ));
}

#[test]
fn parser_rejects_incoherent_data_inventory_before_publication() {
    let state = reference(1, 41);
    let retention = reference(2, 42);
    let union = reference(3, 43);
    let descriptor = GenerationCheckpointDescriptorV1::new(
        SemanticProfileId::from_array(*b"RSpace.Test.V1\0\0"),
        StateVersionV1::new(17, state),
        retention,
        state.encoded_len(),
        inventory(state, retention, union),
    )
    .unwrap();
    let mut bytes = descriptor.try_canonical_bytes().unwrap();
    let data_count_offset = 16 + 8 + 8 + 16 + 8 + 40 + 40 + 8 + 8 + 8;
    bytes[data_count_offset..data_count_offset + 8].copy_from_slice(&99_u64.to_le_bytes());
    assert!(matches!(
        GenerationCheckpointDescriptorViewV1::parse(&bytes),
        Err(GenerationCheckpointErrorV1::InventoryIncoherent { .. })
    ));
}
