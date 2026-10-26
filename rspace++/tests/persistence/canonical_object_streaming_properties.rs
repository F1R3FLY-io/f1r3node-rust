use proptest::prelude::*;

use super::super::object::CanonicalObjectStreamValidatorV1;
use super::support::{TEST_SEMANTIC_PROFILE_ID, empty_roots, encode, node, nonzero, synthetic_ref};
use crate::rspace::persistence::{
    AcceleratorArtifact, AcceleratorArtifactKind, AcceleratorManifestV1, CanonicalBlob,
    CanonicalBlobKind, CanonicalFormatError, CanonicalObjectKind, CanonicalObjectView,
    EndpointValue, LegacyHistoryRoot, ObjectRefV1, RetainedStateV1, RetentionManifestV1,
    SemanticStateManifestV1, StateVersionV1,
};

fn oracle(bytes: &[u8]) -> Result<(CanonicalObjectKind, ObjectRefV1), CanonicalFormatError> {
    let object = CanonicalObjectView::parse(bytes)?;
    Ok(match object {
        CanonicalObjectView::DagNode(node) => (CanonicalObjectKind::DagNode, node.object_ref()),
        CanonicalObjectView::Blob(blob) => (CanonicalObjectKind::Blob, blob.object_ref()),
        CanonicalObjectView::SemanticManifest(manifest) => {
            (CanonicalObjectKind::SemanticManifest, manifest.object_ref())
        }
        CanonicalObjectView::AcceleratorManifest(manifest) => {
            (CanonicalObjectKind::AcceleratorManifest, manifest.object_ref())
        }
        CanonicalObjectView::RetentionManifest(manifest) => {
            (CanonicalObjectKind::RetentionManifest, manifest.object_ref())
        }
    })
}

fn validator_identity(
    bytes: &[u8],
    widths: impl IntoIterator<Item = usize>,
) -> Result<(CanonicalObjectKind, ObjectRefV1), CanonicalFormatError> {
    let mut validator = CanonicalObjectStreamValidatorV1::new(bytes.len());
    let mut offset = 0usize;
    for width in widths {
        let end = offset.saturating_add(width).min(bytes.len());
        validator.feed(&bytes[offset..end])?;
        offset = end;
    }
    if offset < bytes.len() {
        validator.feed(&bytes[offset..])?;
    }
    let identity = validator.finish()?;
    Ok((identity.kind(), identity.reference()))
}

fn assert_all_partitions(bytes: &[u8]) {
    let expected = oracle(bytes);
    assert_eq!(validator_identity(bytes, [usize::MAX]), expected);
    assert_eq!(validator_identity(bytes, std::iter::repeat_n(1, bytes.len())), expected);
    for split in 0..=bytes.len() {
        assert_eq!(validator_identity(bytes, [split, bytes.len() - split]), expected);
    }
    for width in [2, 7, 15, 16, 31, 32, 39, 40, 41, 255, 256, 257, 4096, 65_536] {
        assert_eq!(
            validator_identity(bytes, std::iter::repeat_n(width, bytes.len() / width + 2)),
            expected
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn arbitrary_canonical_object_bytes_and_chunk_schedules_refine_the_contiguous_oracle(
        bytes in proptest::collection::vec(any::<u8>(), 0..2048),
        widths in proptest::collection::vec(0usize..128, 0..128),
    ) {
        prop_assert_eq!(validator_identity(&bytes, widths), oracle(&bytes));
        prop_assert_eq!(validator_identity(&bytes, std::iter::repeat_n(1, bytes.len())), oracle(&bytes));
    }

    #[test]
    fn one_byte_substitution_in_each_object_kind_preserves_oracle_equivalence(
        kind_index in 0usize..5,
        replacement in any::<u8>(),
        selected in any::<usize>(),
        width in 1usize..512,
    ) {
        let mut bytes = valid_objects().swap_remove(kind_index);
        let index = selected % bytes.len();
        bytes[index] ^= replacement | 1;
        prop_assert_eq!(validator_identity(&bytes, std::iter::repeat_n(width, bytes.len() / width + 2)), oracle(&bytes));
    }

    #[test]
    fn generated_valid_instances_of_every_object_kind_match_under_arbitrary_chunk_schedules(
        prefix in proptest::collection::vec(any::<u8>(), 0..32),
        payload in proptest::collection::vec(any::<u8>(), 0..512),
        fanout in prop_oneof![
            Just(0usize), Just(2), Just(4), Just(5), Just(16),
            Just(17), Just(48), Just(49), Just(256),
        ],
        include_legacy in any::<bool>(),
        artifact_count in 0usize..=6,
        historical_count in 0usize..8,
        widths in proptest::collection::vec(0usize..160, 0..96),
    ) {
        let dag = encode(&node(&prefix, EndpointValue::Unit, fanout).unwrap());
        let blob = encode(
            &CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, payload).unwrap(),
        );
        let semantic = SemanticStateManifestV1::new(
            1,
            TEST_SEMANTIC_PROFILE_ID,
            empty_roots(synthetic_ref(0xc1)),
            include_legacy.then(|| LegacyHistoryRoot::from_array([0xc2; 32])),
        ).unwrap();
        let semantic_bytes = encode(&semantic);
        let artifact_kinds = [
            AcceleratorArtifactKind::ChannelReceives,
            AcceleratorArtifactKind::ContinuationIdentity,
            AcceleratorArtifactKind::CandidateCatalog,
            AcceleratorArtifactKind::SetAutomata,
            AcceleratorArtifactKind::ActImage,
            AcceleratorArtifactKind::CachedSummaries,
        ];
        let artifacts = artifact_kinds[..artifact_count]
            .iter()
            .enumerate()
            .map(|(index, kind)| AcceleratorArtifact {
                kind: *kind,
                object: synthetic_ref(0xd0 + index as u8),
            })
            .collect::<Vec<_>>();
        let accelerator = encode(
            &AcceleratorManifestV1::new(semantic.object_ref(), 17, artifacts).unwrap(),
        );
        let history = (0..historical_count)
            .map(|index| retained(index as u64, 0xe0 + index as u8, 4096))
            .collect::<Vec<_>>();
        let retention = encode(
            &RetentionManifestV1::new(retained(100, 0xef, 8192), history).unwrap(),
        );
        for bytes in [dag, blob, semantic_bytes, accelerator, retention] {
            let expected = oracle(&bytes);
            prop_assert!(expected.is_ok());
            prop_assert_eq!(
                validator_identity(&bytes, widths.iter().copied()),
                expected.clone(),
            );
            prop_assert_eq!(
                validator_identity(&bytes, [0, 1, 15, 16, 31, 32, bytes.len()]),
                expected,
            );
        }
    }
}

fn retained(generation: u64, seed: u8, canonical_bytes: u64) -> RetainedStateV1 {
    RetainedStateV1::new(StateVersionV1::new(generation, synthetic_ref(seed)), canonical_bytes)
        .unwrap()
}

fn valid_objects() -> Vec<Vec<u8>> {
    let dag = node(&[1, 2, 3], EndpointValue::Unit, 17).unwrap();
    let blob =
        CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, vec![0x5a; 257]).unwrap();
    let semantic = SemanticStateManifestV1::new(
        1,
        TEST_SEMANTIC_PROFILE_ID,
        empty_roots(synthetic_ref(0x31)),
        Some(LegacyHistoryRoot::from_array([0x44; 32])),
    )
    .unwrap();
    let accelerator = AcceleratorManifestV1::new(semantic.object_ref(), 7, [
        AcceleratorArtifact {
            kind: AcceleratorArtifactKind::ChannelReceives,
            object: synthetic_ref(0x51),
        },
        AcceleratorArtifact {
            kind: AcceleratorArtifactKind::ActImage,
            object: synthetic_ref(0x52),
        },
    ])
    .unwrap();
    let retention = RetentionManifestV1::new(retained(12, 0x61, 8192), [
        retained(3, 0x62, 4096),
        retained(8, 0x63, 6144),
    ])
    .unwrap();
    vec![encode(&dag), encode(&blob), encode(&semantic), encode(&accelerator), encode(&retention)]
}

#[test]
fn all_five_canonical_object_stream_pdas_match_contiguous_oracles_for_generated_valid_and_malformed_images()
 {
    for bytes in valid_objects() {
        assert_all_partitions(&bytes);
    }
    for bytes in typed_malformed_objects() {
        assert!(oracle(&bytes).is_err());
        assert_representative_partitions(&bytes);
    }
}

/// A declared extent past the address space is out of range in the
/// contiguous cursor before it is truncated; one below that boundary it is
/// truncated. Every chunked stream must report the same, for the envelope
/// body and for each length-prefixed body region.
#[test]
fn overflowing_declared_extents_refine_the_contiguous_oracle() {
    let objects = valid_objects();
    // (object, length-field offset, region start in its cursor, unit width)
    let fields = [(0, 24, 32, 1), (0, 32, 8, 1), (1, 56, 32, 1), (3, 80, 56, 48), (4, 88, 64, 56)];
    for (object, at, start, width) in fields {
        let overflowing = (usize::MAX - start) / width + 1;
        for (declared, overflows) in [(overflowing, true), (overflowing - 1, false)] {
            let mut bytes = objects[object].clone();
            bytes[at..at + 8].copy_from_slice(
                &u64::try_from(declared)
                    .expect("usize fits u64")
                    .to_le_bytes(),
            );
            match (oracle(&bytes), overflows) {
                (
                    Err(CanonicalFormatError::LengthOutOfRange {
                        field: "cursor extent",
                        value: u64::MAX,
                    }),
                    true,
                ) |
                (Err(CanonicalFormatError::Truncated { .. }), false) => {}
                (other, _) => panic!("declared {declared} at byte {at}: {other:?}"),
            }
            assert_all_partitions(&bytes);
        }
    }
}

#[test]
fn every_valid_object_prefix_has_the_exact_contiguous_error() {
    for bytes in valid_objects() {
        for length in 0..bytes.len() {
            let prefix = &bytes[..length];
            assert_eq!(
                validator_identity(prefix, std::iter::repeat_n(17, length / 17 + 1)),
                oracle(prefix)
            );
        }
    }
}

fn replace_body_len(bytes: &mut [u8], body_len: u64) {
    bytes[24..32].copy_from_slice(&body_len.to_le_bytes());
}

fn assert_representative_partitions(bytes: &[u8]) {
    let expected = oracle(bytes);
    assert_eq!(validator_identity(bytes, [bytes.len()]), expected);
    assert_eq!(validator_identity(bytes, [0, bytes.len(), 0]), expected);
    assert_eq!(validator_identity(bytes, std::iter::repeat_n(1, bytes.len())), expected,);
    for width in [2, 7, 15, 16, 31, 32, 39, 40, 41, 255, 256, 257] {
        assert_eq!(
            validator_identity(bytes, std::iter::repeat_n(width, bytes.len() / width + 2),),
            expected,
        );
    }
}

fn typed_malformed_objects() -> Vec<Vec<u8>> {
    let mut faults = Vec::new();

    for mut bytes in valid_objects() {
        bytes[0] ^= 0x80;
        faults.push(bytes);
    }
    for mut bytes in valid_objects() {
        bytes[16..24].copy_from_slice(&2_u64.to_le_bytes());
        faults.push(bytes);
    }
    for mut bytes in valid_objects() {
        let body_len = u64::from_le_bytes(bytes[24..32].try_into().unwrap());
        bytes[24..32].copy_from_slice(&(body_len + 1).to_le_bytes());
        faults.push(bytes);
    }
    for mut bytes in valid_objects() {
        let body_len = u64::from_le_bytes(bytes[24..32].try_into().unwrap());
        bytes[24..32].copy_from_slice(&(body_len - 1).to_le_bytes());
        faults.push(bytes);
    }

    let mut invalid_endpoint = encode(&node(&[], EndpointValue::Unit, 0).unwrap());
    invalid_endpoint[40] = 0xff;
    faults.push(invalid_endpoint);

    let mut zero_multiplicity =
        encode(&node(&[], EndpointValue::Multiplicity(nonzero(1)), 0).unwrap());
    zero_multiplicity[41..49].copy_from_slice(&0_u64.to_le_bytes());
    faults.push(zero_multiplicity);

    let mut invalid_branch = encode(&node(&[], EndpointValue::Unit, 0).unwrap());
    invalid_branch[41] = 0xff;
    faults.push(invalid_branch);

    let mut wrong_branch_class = encode(&node(&[], EndpointValue::Unit, 0).unwrap());
    wrong_branch_class[41] = 1;
    faults.push(wrong_branch_class);

    let mut unordered_edges = encode(&node(&[], EndpointValue::Unit, 2).unwrap());
    unordered_edges[91] = unordered_edges[50];
    faults.push(unordered_edges);

    let mut malformed_node48 = encode(&node(&[], EndpointValue::Unit, 17).unwrap());
    malformed_node48[50] = 2;
    faults.push(malformed_node48);

    let mut malformed_node256 = encode(&node(&[], EndpointValue::Unit, 49).unwrap());
    malformed_node256[50 + 100 / 8] |= 1 << (100 % 8);
    faults.push(malformed_node256);

    let mut empty_shape = encode(&node(&[7], EndpointValue::Unit, 0).unwrap());
    empty_shape[41] = 0;
    faults.push(empty_shape);

    let mut unary_shape = encode(&node(&[], EndpointValue::Unit, 1).unwrap());
    unary_shape[40] = 0;
    faults.push(unary_shape);

    let blob = encode(
        &CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, vec![1, 2, 3]).unwrap(),
    );
    let mut invalid_blob_domain = blob.clone();
    invalid_blob_domain[32] ^= 0x80;
    faults.push(invalid_blob_domain);
    let mut invalid_blob_version = blob.clone();
    invalid_blob_version[48..56].copy_from_slice(&2_u64.to_le_bytes());
    faults.push(invalid_blob_version);
    let mut short_blob_payload = blob.clone();
    short_blob_payload[56..64].copy_from_slice(&4_u64.to_le_bytes());
    faults.push(short_blob_payload);
    let mut trailing_blob_payload = blob;
    trailing_blob_payload[56..64].copy_from_slice(&2_u64.to_le_bytes());
    faults.push(trailing_blob_payload);

    let semantic = encode(
        &SemanticStateManifestV1::new(
            1,
            TEST_SEMANTIC_PROFILE_ID,
            empty_roots(synthetic_ref(0x91)),
            Some(LegacyHistoryRoot::from_array([0x92; 32])),
        )
        .unwrap(),
    );
    let mut invalid_semantic_epoch = semantic.clone();
    invalid_semantic_epoch[32..40].copy_from_slice(&2_u64.to_le_bytes());
    faults.push(invalid_semantic_epoch);
    let mut invalid_root_count = semantic.clone();
    invalid_root_count[56..64].copy_from_slice(&2_u64.to_le_bytes());
    faults.push(invalid_root_count);
    for (offset, wrong_tag) in [(64, 2_u64), (112, 1), (160, 2)] {
        let mut wrong_root_order = semantic.clone();
        wrong_root_order[offset..offset + 8].copy_from_slice(&wrong_tag.to_le_bytes());
        faults.push(wrong_root_order);
    }
    let mut invalid_root_presence = semantic.clone();
    invalid_root_presence[208] = 2;
    faults.push(invalid_root_presence);
    let mut truncated_legacy_root = semantic;
    truncated_legacy_root.pop();
    let body_len = u64::from_le_bytes(truncated_legacy_root[24..32].try_into().unwrap());
    truncated_legacy_root[24..32].copy_from_slice(&(body_len - 1).to_le_bytes());
    faults.push(truncated_legacy_root);

    let semantic = SemanticStateManifestV1::new(
        1,
        TEST_SEMANTIC_PROFILE_ID,
        empty_roots(synthetic_ref(0xa1)),
        None,
    )
    .unwrap();
    let accelerator = encode(
        &AcceleratorManifestV1::new(semantic.object_ref(), 1, [
            AcceleratorArtifact {
                kind: AcceleratorArtifactKind::ChannelReceives,
                object: synthetic_ref(0xa2),
            },
            AcceleratorArtifact {
                kind: AcceleratorArtifactKind::ActImage,
                object: synthetic_ref(0xa3),
            },
        ])
        .unwrap(),
    );
    let mut invalid_accelerator_count = accelerator.clone();
    invalid_accelerator_count[80..88].copy_from_slice(&3_u64.to_le_bytes());
    faults.push(invalid_accelerator_count);
    let mut invalid_artifact_kind = accelerator.clone();
    invalid_artifact_kind[88..96].copy_from_slice(&0_u64.to_le_bytes());
    faults.push(invalid_artifact_kind);
    let mut duplicate_artifact_kind = accelerator.clone();
    duplicate_artifact_kind[136..144].copy_from_slice(&1_u64.to_le_bytes());
    faults.push(duplicate_artifact_kind);
    let mut reversed_artifact_kinds = accelerator;
    reversed_artifact_kinds[88..96].copy_from_slice(&6_u64.to_le_bytes());
    faults.push(reversed_artifact_kinds);

    let retention = encode(
        &RetentionManifestV1::new(retained(12, 0xb1, 8192), [
            retained(3, 0xb2, 4096),
            retained(8, 0xb3, 6144),
        ])
        .unwrap(),
    );
    let mut insufficient_retained_bytes = retention.clone();
    insufficient_retained_bytes[80..88].copy_from_slice(&49_u64.to_le_bytes());
    faults.push(insufficient_retained_bytes);
    let mut invalid_history_count = retention.clone();
    invalid_history_count[88..96].copy_from_slice(&3_u64.to_le_bytes());
    faults.push(invalid_history_count);
    let mut contains_current = retention.clone();
    let current = contains_current[32..88].to_vec();
    contains_current[96..152].copy_from_slice(&current);
    faults.push(contains_current);
    let mut unordered_history = retention;
    let first = unordered_history[96..152].to_vec();
    unordered_history[152..208].copy_from_slice(&first);
    faults.push(unordered_history);

    faults
}

#[test]
fn envelope_version_and_extent_errors_precede_body_field_errors_exactly() {
    let mut invalid_version = encode(&node(&[], EndpointValue::Unit, 0).unwrap());
    invalid_version[16..24].copy_from_slice(&2_u64.to_le_bytes());
    invalid_version[40] = 0xff;
    assert_eq!(validator_identity(&invalid_version, [3, 5, 7, 11, 13]), oracle(&invalid_version));
    assert!(matches!(
        oracle(&invalid_version),
        Err(CanonicalFormatError::UnsupportedVersion { .. })
    ));

    let mut short_extent = encode(&node(&[], EndpointValue::Unit, 0).unwrap());
    short_extent[40] = 0xff;
    let declared = u64::from_le_bytes(short_extent[24..32].try_into().unwrap());
    replace_body_len(&mut short_extent, declared + 1);
    assert_eq!(validator_identity(&short_extent, [1; 96]), oracle(&short_extent));
    assert!(matches!(
        oracle(&short_extent),
        Err(CanonicalFormatError::Truncated { offset: 32, .. })
    ));

    let mut trailing_extent = encode(&node(&[], EndpointValue::Unit, 0).unwrap());
    trailing_extent[40] = 0xff;
    let declared = u64::from_le_bytes(trailing_extent[24..32].try_into().unwrap());
    replace_body_len(&mut trailing_extent, declared - 1);
    assert_eq!(validator_identity(&trailing_extent, [31, 1, 8, 64]), oracle(&trailing_extent));
    assert!(matches!(oracle(&trailing_extent), Err(CanonicalFormatError::TrailingBytes { .. })));
}

#[test]
fn dag_aggregate_and_shape_precedence_matches_the_contiguous_oracle() {
    let mut aggregate = encode(&node(&[], EndpointValue::Absent, 2).unwrap());
    aggregate.pop();
    let body_len = aggregate.len() as u64 - 32;
    replace_body_len(&mut aggregate, body_len);
    assert_eq!(validator_identity(&aggregate, [40, 1, 41, 3]), oracle(&aggregate));
    assert!(matches!(oracle(&aggregate), Err(CanonicalFormatError::Truncated { .. })));

    let mut empty_shape = encode(&node(&[7], EndpointValue::Unit, 0).unwrap());
    empty_shape[41] = 0;
    assert_eq!(validator_identity(&empty_shape, [1; 128]), oracle(&empty_shape));
    assert_eq!(oracle(&empty_shape), Err(CanonicalFormatError::EmptyNode));

    let mut unary_shape = encode(&node(&[], EndpointValue::Unit, 1).unwrap());
    unary_shape[40] = 0;
    assert_eq!(validator_identity(&unary_shape, [32, 8, 1, 1, 8, 41]), oracle(&unary_shape));
    assert_eq!(oracle(&unary_shape), Err(CanonicalFormatError::CompressibleUnaryNode));

    let mut node48 = encode(&node(&[], EndpointValue::Absent, 17).unwrap());
    node48[51] = 2;
    assert_eq!(validator_identity(&node48, [49, 2, 255, 4096]), oracle(&node48));
    assert!(matches!(oracle(&node48), Err(CanonicalFormatError::ChildBitmapCount { .. })));
}

#[test]
fn accelerator_and_retention_aggregate_extents_precede_entry_fields() {
    let semantic = SemanticStateManifestV1::new(
        1,
        TEST_SEMANTIC_PROFILE_ID,
        empty_roots(synthetic_ref(0x71)),
        None,
    )
    .unwrap();
    let mut accelerator = encode(
        &AcceleratorManifestV1::new(semantic.object_ref(), 1, [AcceleratorArtifact {
            kind: AcceleratorArtifactKind::ActImage,
            object: synthetic_ref(0x72),
        }])
        .unwrap(),
    );
    accelerator[80..88].copy_from_slice(&2_u64.to_le_bytes());
    accelerator[88..96].copy_from_slice(&99_u64.to_le_bytes());
    assert_eq!(validator_identity(&accelerator, [7, 40, 41, 48]), oracle(&accelerator));
    assert!(matches!(oracle(&accelerator), Err(CanonicalFormatError::Truncated { .. })));

    let mut retention = encode(
        &RetentionManifestV1::new(retained(12, 0x81, 8192), [retained(3, 0x82, 4096)]).unwrap(),
    );
    retention[88..96].copy_from_slice(&2_u64.to_le_bytes());
    retention[144..152].copy_from_slice(&0_u64.to_le_bytes());
    assert_eq!(validator_identity(&retention, [32, 56, 8, 56]), oracle(&retention));
    assert!(matches!(oracle(&retention), Err(CanonicalFormatError::Truncated { .. })));
}

#[test]
fn canonical_object_stream_retains_constant_state_for_large_payloads() {
    let validator_size = std::mem::size_of::<CanonicalObjectStreamValidatorV1>();
    assert!(
        validator_size <= 512,
        "canonical object stream validator retained {validator_size} bytes"
    );
    let bytes = encode(
        &CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, vec![0x5a; 1_000_000])
            .unwrap(),
    );
    assert_eq!(validator_identity(&bytes, std::iter::repeat_n(257, 4_000)), oracle(&bytes));
}
