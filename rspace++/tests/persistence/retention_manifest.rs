use super::support::{encode, synthetic_ref};
use crate::rspace::persistence::{
    CanonicalFormatError, CanonicalObjectKind, CanonicalObjectView, RetainedStateV1,
    RetentionManifestV1, RetentionManifestView, StateVersionV1,
};

fn retained(generation: u64, seed: u8, canonical_bytes: u64) -> RetainedStateV1 {
    RetainedStateV1::new(StateVersionV1::new(generation, synthetic_ref(seed)), canonical_bytes)
        .unwrap()
}

#[test]
fn retention_manifest_round_trips_as_a_zero_copy_canonical_object() {
    let current = retained(12, 0x44, 8192);
    let historical = [retained(3, 0x11, 4096), retained(8, 0x22, 6144), retained(8, 0x33, 7168)];
    let manifest = RetentionManifestV1::new(current, historical).unwrap();
    let bytes = encode(&manifest);
    let view = RetentionManifestView::parse(&bytes).unwrap();
    assert_eq!(view.current(), current);
    assert_eq!(view.historical().collect::<Result<Vec<_>, _>>().unwrap(), historical);
    assert_eq!(view.canonical_bytes().as_ptr(), bytes.as_ptr());
    assert_eq!(view.object_ref(), manifest.object_ref());
    assert_eq!(
        CanonicalObjectView::parse(&bytes).unwrap().kind(),
        CanonicalObjectKind::RetentionManifest
    );
}

#[test]
fn retention_history_rejects_current_duplicates_and_noncanonical_order() {
    let current = retained(12, 0x44, 8192);
    assert!(matches!(
        RetentionManifestV1::new(current, [current]),
        Err(CanonicalFormatError::RetentionContainsCurrent)
    ));
    assert!(matches!(
        RetentionManifestV1::new(current, [retained(8, 0x33, 7168), retained(3, 0x11, 4096)]),
        Err(CanonicalFormatError::RetainedStatesNotStrict)
    ));
    assert!(matches!(
        RetainedStateV1::new(StateVersionV1::new(1, synthetic_ref(1)), 49),
        Err(CanonicalFormatError::RetentionStateBytes { .. })
    ));
}

#[test]
fn every_truncated_retention_manifest_is_rejected() {
    let manifest = RetentionManifestV1::new(retained(12, 0x44, 8192), [
        retained(3, 0x11, 4096),
        retained(8, 0x22, 6144),
    ])
    .unwrap();
    let bytes = encode(&manifest);
    for length in 0..bytes.len() {
        assert!(
            RetentionManifestView::parse(&bytes[..length]).is_err(),
            "truncation at {length} bytes was accepted"
        );
    }
}

#[test]
fn zero_history_encoding_is_frozen() {
    let manifest = RetentionManifestV1::new(retained(1, 0xaa, 50), []).unwrap();
    assert_eq!(
        hex::encode(encode(&manifest)),
        concat!(
            "5253706163654441472e5265746e0000",
            "0100000000000000",
            "4000000000000000",
            "0100000000000000",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "3200000000000000",
            "3200000000000000",
            "0000000000000000"
        )
    );
}
