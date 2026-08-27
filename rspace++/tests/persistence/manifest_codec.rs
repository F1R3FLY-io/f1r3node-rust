use super::support::{TEST_SEMANTIC_PROFILE_ID, empty_roots, encode, synthetic_ref};
use crate::rspace::persistence::{
    AcceleratorArtifact, AcceleratorArtifactKind, AcceleratorManifestV1, AcceleratorManifestView,
    CanonicalEncode, CanonicalFormatError, LegacyHistoryRoot, SemanticStateManifestV1,
    SemanticStateManifestView,
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
