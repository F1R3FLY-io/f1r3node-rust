use proptest::prelude::*;

use super::support::{TEST_SEMANTIC_PROFILE_ID, empty_roots, encode, synthetic_ref};
use crate::rspace::persistence::{
    AcceleratorArtifact, AcceleratorArtifactKind, AcceleratorManifestV1, AcceleratorManifestView,
    CanonicalEncode, CanonicalFormatError, LegacyHistoryRoot, SemanticProfileId, SemanticRoots,
    SemanticStateManifestV1, SemanticStateManifestView, StateVersionV1,
};

#[test]
fn semantic_manifest_round_trips_three_roots_and_legacy_provenance() {
    let roots = empty_roots(synthetic_ref(1));
    let legacy = LegacyHistoryRoot::from_array([9; 32]);
    let manifest = SemanticStateManifestV1::new(1, TEST_SEMANTIC_PROFILE_ID, roots, Some(legacy))
        .expect("schema-one manifest is valid");
    let bytes = encode(&manifest);
    let parsed = SemanticStateManifestView::parse(&bytes).expect("manifest parses");
    assert_eq!(parsed.schema_epoch(), 1);
    assert_eq!(parsed.profile_id(), TEST_SEMANTIC_PROFILE_ID);
    assert_eq!(parsed.roots(), roots);
    assert_eq!(parsed.legacy_history_root(), Some(legacy));
    assert_eq!(parsed.object_ref(), manifest.object_ref());
}

#[test]
fn unknown_semantic_schema_epoch_fails_closed() {
    assert!(matches!(
        SemanticStateManifestV1::new(
            2,
            TEST_SEMANTIC_PROFILE_ID,
            empty_roots(synthetic_ref(1)),
            None,
        ),
        Err(CanonicalFormatError::UnsupportedVersion { version: 2, .. })
    ));

    let mut bytes = encode(
        &SemanticStateManifestV1::new(
            1,
            TEST_SEMANTIC_PROFILE_ID,
            empty_roots(synthetic_ref(2)),
            None,
        )
        .expect("schema-one manifest is valid"),
    );
    bytes[32] = 7;
    assert!(matches!(
        SemanticStateManifestView::parse(&bytes),
        Err(CanonicalFormatError::UnsupportedVersion { version: 7, .. })
    ));
}

#[test]
fn semantic_root_count_and_order_are_canonical() {
    let manifest = SemanticStateManifestV1::new(
        1,
        TEST_SEMANTIC_PROFILE_ID,
        empty_roots(synthetic_ref(3)),
        None,
    )
    .expect("manifest is valid");
    let mut wrong_count = encode(&manifest);
    wrong_count[56] = 4;
    assert_eq!(
        SemanticStateManifestView::parse(&wrong_count).expect_err("wrong root count is rejected"),
        CanonicalFormatError::SemanticRootCount(4)
    );

    let mut wrong_order = encode(&manifest);
    wrong_order[64] = 2;
    assert_eq!(
        SemanticStateManifestView::parse(&wrong_order).expect_err("wrong root order is rejected"),
        CanonicalFormatError::SemanticRootOrder {
            expected: 1,
            actual: 2,
        }
    );
}

#[test]
fn accelerator_replacement_cannot_change_semantic_identity() {
    let semantic = SemanticStateManifestV1::new(
        1,
        TEST_SEMANTIC_PROFILE_ID,
        empty_roots(synthetic_ref(3)),
        None,
    )
    .expect("semantic manifest is valid");
    let semantic_ref = semantic.object_ref();
    let before = semantic
        .try_canonical_bytes()
        .expect("semantic bytes encode");

    let first = AcceleratorManifestV1::new(semantic_ref, 1, [AcceleratorArtifact {
        kind: AcceleratorArtifactKind::ActImage,
        object: synthetic_ref(4),
    }])
    .expect("first accelerator manifest is valid");
    let second = AcceleratorManifestV1::new(semantic_ref, 2, [AcceleratorArtifact {
        kind: AcceleratorArtifactKind::CachedSummaries,
        object: synthetic_ref(5),
    }])
    .expect("replacement accelerator manifest is valid");
    assert_ne!(first.object_ref(), second.object_ref());
    assert_eq!(semantic.object_ref(), semantic_ref);
    assert_eq!(
        semantic
            .try_canonical_bytes()
            .expect("semantic bytes encode"),
        before
    );

    for accelerator in [&first, &second] {
        let bytes = encode(accelerator);
        let parsed = AcceleratorManifestView::parse(&bytes).expect("accelerator parses");
        assert_eq!(parsed.semantic_manifest(), semantic_ref);
        assert_eq!(parsed.artifact_count(), 1);
        assert_eq!(parsed.object_ref(), accelerator.object_ref());
    }
}

#[test]
fn accelerator_artifacts_require_strict_canonical_kind_order() {
    let artifacts = [
        AcceleratorArtifact {
            kind: AcceleratorArtifactKind::ActImage,
            object: synthetic_ref(1),
        },
        AcceleratorArtifact {
            kind: AcceleratorArtifactKind::SetAutomata,
            object: synthetic_ref(2),
        },
    ];
    assert!(matches!(
        AcceleratorManifestV1::new(synthetic_ref(8), 1, artifacts),
        Err(CanonicalFormatError::AcceleratorKindsNotStrict { .. })
    ));
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        rng_seed: proptest::test_runner::RngSeed::Fixed(41317941),
        ..ProptestConfig::default()
    })]

    #[test]
    fn semantic_identity_binds_exact_namespaces_profile_and_provenance(
        seed in any::<u8>(),
        provenance in any::<[u8; 32]>(),
        generation in any::<u64>(),
    ) {
        let roots = SemanticRoots {
            data_bag: synthetic_ref(seed),
            continuation_map: synthetic_ref(seed.wrapping_add(1)),
            join_bag: synthetic_ref(seed.wrapping_add(2)),
        };
        let legacy = LegacyHistoryRoot::from_array(provenance);
        let manifest = SemanticStateManifestV1::new(
            1, TEST_SEMANTIC_PROFILE_ID, roots, Some(legacy),
        ).unwrap();
        let bytes = encode(&manifest);
        let parsed = SemanticStateManifestView::parse(&bytes).unwrap();
        prop_assert_eq!(parsed.roots(), roots);
        prop_assert_eq!(parsed.profile_id(), TEST_SEMANTIC_PROFILE_ID);
        prop_assert_eq!(parsed.legacy_history_root(), Some(legacy));
        prop_assert_eq!(parsed.object_ref(), manifest.object_ref());

        let mut other_profile = *TEST_SEMANTIC_PROFILE_ID.as_bytes();
        other_profile[0] ^= 1;
        let mut other_provenance = provenance;
        other_provenance[0] ^= 1;
        let variants = [
            SemanticStateManifestV1::new(1, TEST_SEMANTIC_PROFILE_ID,
                SemanticRoots { data_bag: roots.continuation_map,
                    continuation_map: roots.data_bag, ..roots }, Some(legacy)).unwrap(),
            SemanticStateManifestV1::new(1, TEST_SEMANTIC_PROFILE_ID,
                SemanticRoots { join_bag: roots.continuation_map,
                    continuation_map: roots.join_bag, ..roots }, Some(legacy)).unwrap(),
            SemanticStateManifestV1::new(1, SemanticProfileId::from_array(other_profile),
                roots, Some(legacy)).unwrap(),
            SemanticStateManifestV1::new(1, TEST_SEMANTIC_PROFILE_ID, roots, None).unwrap(),
            SemanticStateManifestV1::new(1, TEST_SEMANTIC_PROFILE_ID, roots,
                Some(LegacyHistoryRoot::from_array(other_provenance))).unwrap(),
        ];
        for variant in variants {
            prop_assert_ne!(encode(&variant), bytes.clone());
            prop_assert_ne!(variant.object_ref(), manifest.object_ref());
        }
        let without_provenance = SemanticStateManifestV1::new(
            1, TEST_SEMANTIC_PROFILE_ID, roots, None,
        ).unwrap();
        prop_assert_eq!(without_provenance.roots(), manifest.roots());
        prop_assert_ne!(without_provenance.object_ref(), manifest.object_ref());
        prop_assert_eq!(StateVersionV1::new(generation, manifest.object_ref()).manifest(),
            StateVersionV1::new(0, manifest.object_ref()).manifest());
    }

    #[test]
    fn unsupported_schema_is_refused_by_both_constructor_and_decoder(epoch in any::<u64>()) {
        prop_assume!(epoch != 1);
        let roots = empty_roots(synthetic_ref(1));
        prop_assert!(matches!(
            SemanticStateManifestV1::new(epoch, TEST_SEMANTIC_PROFILE_ID, roots, None),
            Err(CanonicalFormatError::UnsupportedVersion { version, .. }) if version == epoch
        ), "constructor must reject the exact unsupported schema epoch");
        let mut bytes = encode(&SemanticStateManifestV1::new(
            1, TEST_SEMANTIC_PROFILE_ID, roots, None,
        ).unwrap());
        bytes[32..40].copy_from_slice(&epoch.to_le_bytes());
        prop_assert!(matches!(
            SemanticStateManifestView::parse(&bytes),
            Err(CanonicalFormatError::UnsupportedVersion { version, .. }) if version == epoch
        ), "decoder must reject the exact unsupported schema epoch");
    }
}
