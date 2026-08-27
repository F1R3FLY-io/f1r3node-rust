use std::collections::BTreeSet;

use super::support::{
    TEST_SEMANTIC_PROFILE_ID, TestSemanticProfile, admit, empty_roots, manifest, nonzero,
};
use crate::rspace::data_index::RSpaceDataIndex;
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::{
    CanonicalBlob, CanonicalBlobKind, CanonicalDagNode, CanonicalFormatError, CanonicalObjectStore,
    ClosureError, DagChild, EndpointValue, SemanticProfileId, SemanticRoots,
    SemanticStateManifestV1, SemanticValidationLimits, verify_rspace_semantics,
};
use crate::rspace::rho_path::RhoPathV1;
use crate::rspace::serializers::serializers::{
    encode_continuation_identity, encode_continuation_source_into, encode_datum, encode_join_record,
};

struct StateImage {
    store: CanonicalObjectStore,
    manifest: crate::rspace::persistence::ObjectRefV1,
}

fn valid_image() -> StateImage { image_with_options(false, false, false, false) }

fn image_with_options(
    omit_join: bool,
    mismatched_arity: bool,
    malformed_source: bool,
    independent_source: bool,
) -> StateImage {
    let store = CanonicalObjectStore::new();
    let empty = admit(&store, &CanonicalDagNode::empty());
    let channel = "channel".to_owned();
    let datum = Datum::create(&channel, "datum".to_owned(), true);
    let data_path = RSpaceDataIndex::datum_path(&channel, &datum);
    let data = admit(
        &store,
        &CanonicalDagNode::new(
            data_path,
            EndpointValue::Multiplicity(nonzero(3)),
            Box::<[DagChild]>::default(),
        )
        .expect("data leaf is canonical"),
    );

    let channels = vec![channel.clone()];
    let patterns = if mismatched_arity {
        Vec::new()
    } else {
        vec!["pattern".to_owned()]
    };
    let mut waiting = WaitingContinuation::create(
        &channels,
        &patterns,
        &"continuation".to_owned(),
        false,
        BTreeSet::from([0]),
    );
    if independent_source {
        waiting.source = WaitingContinuation::create(
            &vec!["source-channel".to_owned()],
            &vec!["source-pattern".to_owned()],
            &"source-continuation".to_owned(),
            true,
            BTreeSet::new(),
        )
        .source;
    }
    let source_payload = if malformed_source {
        vec![0xff; 3]
    } else {
        let mut payload = Vec::new();
        encode_continuation_source_into(&waiting, &mut payload);
        payload
    };
    let source = admit(
        &store,
        &CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, source_payload)
            .expect("source blob is canonical"),
    );
    let continuation_prefix = RhoPathV1::continuation_prefix(&channels);
    let continuation_path = RhoPathV1::collection_record_path(
        &continuation_prefix,
        &encode_continuation_identity(&waiting),
    );
    let continuation = admit(
        &store,
        &CanonicalDagNode::new(
            continuation_path,
            EndpointValue::Content(source),
            Box::<[DagChild]>::default(),
        )
        .expect("continuation leaf is canonical"),
    );

    let join = if omit_join {
        empty
    } else {
        let mut join_path = RhoPathV1::join_prefix(&channel);
        join_path.extend_from_slice(&encode_join_record(&channels));
        admit(
            &store,
            &CanonicalDagNode::new(
                join_path,
                EndpointValue::Multiplicity(nonzero(2)),
                Box::<[DagChild]>::default(),
            )
            .expect("join leaf is canonical"),
        )
    };
    let manifest = admit(
        &store,
        &manifest(SemanticRoots {
            data_bag: data,
            continuation_map: continuation,
            join_bag: join,
        }),
    );
    StateImage { store, manifest }
}

fn verify(
    image: &StateImage,
) -> Result<crate::rspace::persistence::SemanticValidationReport, ClosureError<CanonicalFormatError>>
{
    verify_rspace_semantics::<TestSemanticProfile, _>(
        &image.store,
        image.manifest,
        SemanticValidationLimits::unbounded(),
    )
}

#[test]
fn exact_three_root_image_validates_without_flattening_the_tries() {
    let image = valid_image();
    let report = verify(&image).expect("the complete semantic state is valid");
    assert_eq!(report.data_entries, 1);
    assert_eq!(report.continuation_entries, 1);
    assert_eq!(report.join_entries, 1);
    assert_eq!(report.endpoints, 3);
    assert_eq!(report.join_lookups, 1);
    assert_eq!(report.join_lookup_steps, 1);
    assert!(report.source_payload_bytes > 0);
}

#[test]
fn semantic_admission_is_bound_to_the_manifest_profile() {
    let store = CanonicalObjectStore::new();
    let empty = admit(&store, &CanonicalDagNode::empty());
    let wrong_profile = SemanticProfileId::from_array(*b"RSpace.Other.V1\0");
    assert_ne!(wrong_profile, TEST_SEMANTIC_PROFILE_ID);
    let manifest_ref = admit(
        &store,
        &SemanticStateManifestV1::new(1, wrong_profile, empty_roots(empty), None)
            .expect("the alternate profile identifier is structurally valid"),
    );

    assert!(matches!(
        verify_rspace_semantics::<TestSemanticProfile, _>(
            &store,
            manifest_ref,
            SemanticValidationLimits::unbounded(),
        ),
        Err(ClosureError::Format(CanonicalFormatError::SemanticProfileMismatch {
            expected,
            actual,
        })) if expected == *TEST_SEMANTIC_PROFILE_ID.as_bytes()
            && actual == *wrong_profile.as_bytes()
    ));
}

#[test]
fn malformed_keys_and_payloads_fail_closed_after_structural_validation() {
    let store = CanonicalObjectStore::new();
    let empty = admit(&store, &CanonicalDagNode::empty());
    let malformed = admit(
        &store,
        &CanonicalDagNode::new(
            &b"not-a-rhopath"[..],
            EndpointValue::Multiplicity(nonzero(1)),
            Box::<[DagChild]>::default(),
        )
        .expect("arbitrary bytes remain structurally canonical"),
    );
    let manifest_ref = admit(
        &store,
        &manifest(SemanticRoots {
            data_bag: malformed,
            ..empty_roots(empty)
        }),
    );
    let error = verify_rspace_semantics::<TestSemanticProfile, _>(
        &store,
        manifest_ref,
        SemanticValidationLimits::unbounded(),
    )
    .expect_err("structurally valid arbitrary paths are not semantic state");
    assert!(matches!(error, ClosureError::Format(CanonicalFormatError::SemanticKey { .. })));

    let image = image_with_options(false, false, true, false);
    assert!(matches!(
        verify(&image),
        Err(ClosureError::Format(CanonicalFormatError::SemanticPayload { .. }))
    ));
}

#[test]
fn continuation_arity_and_cross_root_join_membership_are_mandatory() {
    let mismatched = image_with_options(false, true, false, false);
    assert!(matches!(
        verify(&mismatched),
        Err(ClosureError::Format(CanonicalFormatError::SemanticKey { .. }))
    ));

    let missing_join = image_with_options(true, false, false, false);
    assert!(matches!(
        verify(&missing_join),
        Err(ClosureError::Format(CanonicalFormatError::SemanticCoherence { .. }))
    ));
}

#[test]
fn selected_first_wins_source_is_not_recomputed_from_identity_fields() {
    let image = image_with_options(false, false, false, true);
    verify(&image).expect("an independently selected source remains observable and valid");
}

#[test]
fn decoded_structure_and_indexed_join_membership_are_authenticated() {
    let store = CanonicalObjectStore::new();
    let empty = admit(&store, &CanonicalDagNode::empty());
    let channel = "channel".to_owned();
    let datum = Datum::create(&channel, "datum".to_owned(), false);
    let wrong_structure_path =
        RhoPathV1::datum_key(&channel, b"wrong-structure", &encode_datum(&datum));
    let data = admit(
        &store,
        &CanonicalDagNode::new(
            wrong_structure_path,
            EndpointValue::Multiplicity(nonzero(1)),
            Box::<[DagChild]>::default(),
        )
        .expect("the incorrect attribution is still a canonical radix leaf"),
    );
    let manifest_ref = admit(
        &store,
        &manifest(SemanticRoots {
            data_bag: data,
            ..empty_roots(empty)
        }),
    );
    assert!(matches!(
        verify_rspace_semantics::<TestSemanticProfile, _>(
            &store,
            manifest_ref,
            SemanticValidationLimits::unbounded(),
        ),
        Err(ClosureError::Format(CanonicalFormatError::SemanticKey { .. }))
    ));

    let store = CanonicalObjectStore::new();
    let empty = admit(&store, &CanonicalDagNode::empty());
    let indexed = "indexed".to_owned();
    let members = vec!["different".to_owned()];
    let mut join_path = RhoPathV1::join_prefix(&indexed);
    join_path.extend_from_slice(&encode_join_record(&members));
    let join = admit(
        &store,
        &CanonicalDagNode::new(
            join_path,
            EndpointValue::Multiplicity(nonzero(1)),
            Box::<[DagChild]>::default(),
        )
        .expect("the incorrect index is still a canonical radix leaf"),
    );
    let manifest_ref = admit(
        &store,
        &manifest(SemanticRoots {
            join_bag: join,
            ..empty_roots(empty)
        }),
    );
    assert!(matches!(
        verify_rspace_semantics::<TestSemanticProfile, _>(
            &store,
            manifest_ref,
            SemanticValidationLimits::unbounded(),
        ),
        Err(ClosureError::Format(CanonicalFormatError::SemanticKey { .. }))
    ));
}

#[test]
fn semantic_resource_budgets_are_not_hidden_depth_limits() {
    let image = valid_image();
    let limits = SemanticValidationLimits {
        max_endpoints: 2,
        ..SemanticValidationLimits::unbounded()
    };
    let error =
        verify_rspace_semantics::<TestSemanticProfile, _>(&image.store, image.manifest, limits)
            .expect_err("the explicit endpoint budget is enforced");
    assert!(matches!(
        error,
        ClosureError::Format(CanonicalFormatError::ResourceLimit {
            resource: "semantic endpoints",
            limit: 2,
            attempted: 3,
        })
    ));
}

#[test]
fn duplicate_channels_require_only_one_join_membership() {
    let store = CanonicalObjectStore::new();
    let empty = admit(&store, &CanonicalDagNode::empty());
    let channel = "same-channel".to_owned();
    let channels = vec![channel.clone(), channel.clone()];
    let waiting = WaitingContinuation::create(
        &channels,
        &vec!["left".to_owned(), "right".to_owned()],
        &"continuation".to_owned(),
        true,
        BTreeSet::new(),
    );
    let mut source_payload = Vec::new();
    encode_continuation_source_into(&waiting, &mut source_payload);
    let source = admit(
        &store,
        &CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, source_payload)
            .expect("source blob is canonical"),
    );
    let continuation = admit(
        &store,
        &CanonicalDagNode::new(
            RhoPathV1::collection_record_path(
                &RhoPathV1::continuation_prefix(&channels),
                &encode_continuation_identity(&waiting),
            ),
            EndpointValue::Content(source),
            Box::<[DagChild]>::default(),
        )
        .expect("continuation leaf is canonical"),
    );
    let mut join_path = RhoPathV1::join_prefix(&channel);
    join_path.extend_from_slice(&encode_join_record(&channels));
    let join = admit(
        &store,
        &CanonicalDagNode::new(
            join_path,
            EndpointValue::Multiplicity(nonzero(1)),
            Box::<[DagChild]>::default(),
        )
        .expect("join leaf is canonical"),
    );
    let manifest_ref = admit(
        &store,
        &manifest(SemanticRoots {
            data_bag: empty,
            continuation_map: continuation,
            join_bag: join,
        }),
    );
    let report = verify_rspace_semantics::<TestSemanticProfile, _>(
        &store,
        manifest_ref,
        SemanticValidationLimits::unbounded(),
    )
    .expect("one membership per distinct channel is sufficient");
    assert_eq!(report.join_lookups, 1);
}
