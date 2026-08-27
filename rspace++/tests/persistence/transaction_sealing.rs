use std::collections::BTreeSet;
use std::sync::Arc;

use super::canonical_rebuild_oracle::{Entries, rebuild};
use super::support::{
    TEST_SEMANTIC_PROFILE_ID, TestSemanticProfile, TracingStore, admit, empty_roots, encode,
};
use crate::rspace::data_index::RSpaceDataIndex;
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::{
    CanonicalBlob, CanonicalBlobKind, CanonicalDagNode, CanonicalFormatError, CanonicalObjectStore,
    CanonicalObjectView, ClosureError, DagTransactionError, DagTransactionLimits, EndpointValue,
    ImmutableObjectStore, IncrementalClosureLimits, IncrementalSemanticLimits, LegacyHistoryRoot,
    MutationBatchViewV1, ObjectAdmission, ObjectDigest, ObjectRefV1, OpenDagTransaction,
    RSpaceMutationBatchV1, RewriteError, SemanticRoots, SemanticStateManifestV1,
    SemanticValidationLimits, StagingStoreError, StagingStoreLimits, StateVersionV1,
    StructuralValidationLimits, VerifiedDagState,
};
use crate::rspace::rho_path::RhoPathV1;
use crate::rspace::serializers::serializers::{
    encode_continuation_identity, encode_continuation_source_into, encode_join_record,
};

fn base_state(
    store: &CanonicalObjectStore,
    generation: u64,
    legacy: Option<LegacyHistoryRoot>,
) -> StateVersionV1 {
    let empty = admit(store, &CanonicalDagNode::empty());
    let manifest =
        SemanticStateManifestV1::new(1, TEST_SEMANTIC_PROFILE_ID, empty_roots(empty), legacy)
            .unwrap();
    StateVersionV1::new(generation, admit(store, &manifest))
}

fn data_batch(base: StateVersionV1, identity: &str) -> Vec<u8> {
    let channel = "channel".to_owned();
    let datum = Datum::create(&channel, identity.to_owned(), false);
    let path = RSpaceDataIndex::datum_path(&channel, &datum);
    let mut batch = RSpaceMutationBatchV1::new(base).unwrap();
    batch.record_data(&path, 0, 1).unwrap();
    encode(&batch.plan().unwrap())
}

fn traced_data_state(cardinality: usize) -> (TracingStore, StateVersionV1, Vec<u8>) {
    let store = TracingStore::default();
    let channel = "frontier-channel".to_owned();
    let mut entries = Entries::new();
    let mut target_path = None;
    for index in 0..cardinality {
        let identity = format!("datum-{index:04}");
        let datum = Datum::create(&channel, identity, false);
        let path = RSpaceDataIndex::datum_path(&channel, &datum);
        if index == 0 {
            target_path = Some(path.clone());
        }
        entries.insert(path, EndpointValue::Multiplicity(std::num::NonZeroU64::new(1).unwrap()));
    }
    let empty = rebuild(&store, &Entries::new());
    let roots = SemanticRoots {
        data_bag: rebuild(&store, &entries),
        ..empty_roots(empty)
    };
    let manifest = SemanticStateManifestV1::new(1, TEST_SEMANTIC_PROFILE_ID, roots, None).unwrap();
    let manifest_bytes: Arc<[u8]> = encode(&manifest).into();
    assert_eq!(
        store
            .put_if_absent_or_equal(manifest.object_ref(), manifest_bytes)
            .unwrap(),
        ObjectAdmission::Inserted
    );
    (
        store,
        StateVersionV1::new(31, manifest.object_ref()),
        target_path.expect("the trace fixture has a target"),
    )
}

fn traced_single_update_reads(cardinality: usize) -> u64 {
    let (store, base, target_path) = traced_data_state(cardinality);
    let mut batch = RSpaceMutationBatchV1::new(base).unwrap();
    batch.record_data(&target_path, 1, 2).unwrap();
    let mutation = encode(&batch.plan().unwrap());
    let view = MutationBatchViewV1::parse(&mutation).unwrap();
    let verified = VerifiedDagState::<_, TestSemanticProfile>::admit(
        &store,
        base,
        SemanticValidationLimits::unbounded(),
    )
    .unwrap();
    store.clear_reads();
    let sealed = OpenDagTransaction::new(verified, view, DagTransactionLimits::UNBOUNDED)
        .unwrap()
        .seal()
        .unwrap();
    assert_eq!(sealed.diagnostics().semantic.changed_data, 1);
    store.total_reads()
}

fn seal_data<'a>(
    store: &CanonicalObjectStore,
    bytes: &'a [u8],
) -> crate::rspace::persistence::SealedDagTransaction<'a, TestSemanticProfile> {
    let view = MutationBatchViewV1::parse(bytes).unwrap();
    let verified = VerifiedDagState::<_, TestSemanticProfile>::admit(
        store,
        view.base(),
        SemanticValidationLimits::unbounded(),
    )
    .expect("the test base is fully admitted");
    OpenDagTransaction::new(verified, view, DagTransactionLimits::UNBOUNDED)
        .unwrap()
        .seal()
        .expect("valid data transition seals")
}

#[test]
fn sealing_preserves_authority_and_consumes_the_base_borrow() {
    let store = CanonicalObjectStore::new();
    let legacy = LegacyHistoryRoot::from_array([0x5a; 32]);
    let base = base_state(&store, 41, Some(legacy));
    let bytes = data_batch(base, "datum");
    let payload_digest = ObjectDigest::of_canonical_bytes(&bytes);
    let sealed = seal_data(&store, &bytes);
    drop(store);

    let receipt = sealed.receipt();
    assert_eq!(receipt.base(), base);
    assert_eq!(receipt.target().generation(), 42);
    assert_eq!(receipt.mutation_digest(), payload_digest);
    assert_eq!(receipt.profile_id(), TEST_SEMANTIC_PROFILE_ID);
    assert_eq!(sealed.mutation_bytes(), bytes);
    assert_eq!(sealed.diagnostics().semantic.changed_data, 1);
    assert_eq!(sealed.diagnostics().semantic.changed_continuations, 0);
    assert_eq!(sealed.diagnostics().semantic.changed_joins, 0);

    let target = sealed
        .objects()
        .iter()
        .find(|object| object.reference() == receipt.target().manifest())
        .expect("the target manifest is owned by the sealed batch");
    let CanonicalObjectView::SemanticManifest(manifest) =
        CanonicalObjectView::parse(target.canonical_bytes()).unwrap()
    else {
        unreachable!()
    };
    assert_eq!(manifest.schema_epoch(), 1);
    assert_eq!(manifest.profile_id(), TEST_SEMANTIC_PROFILE_ID);
    assert_eq!(manifest.legacy_history_root(), Some(legacy));

    for pair in sealed.objects().windows(2) {
        assert!(pair[0].reference().digest() < pair[1].reference().digest());
    }
    for object in sealed.objects() {
        assert_eq!(
            object.reference().digest(),
            ObjectDigest::of_canonical_bytes(object.canonical_bytes())
        );
        assert_eq!(
            object.reference().encoded_len(),
            u64::try_from(object.canonical_bytes().len()).unwrap()
        );
    }
}

#[test]
fn same_base_and_payload_seal_to_identical_objects() {
    let store = CanonicalObjectStore::new();
    let base = base_state(&store, 3, None);
    let bytes = data_batch(base, "deterministic");
    let first = seal_data(&store, &bytes);
    let second = seal_data(&store, &bytes);
    assert_eq!(first.receipt(), second.receipt());
    let first = first
        .objects()
        .iter()
        .map(|object| (object.reference(), object.canonical_bytes()))
        .collect::<Vec<_>>();
    let second = second
        .objects()
        .iter()
        .map(|object| (object.reference(), object.canonical_bytes()))
        .collect::<Vec<_>>();
    assert_eq!(first, second);
}

#[test]
fn transaction_open_requires_the_exact_verified_base_version() {
    let store = CanonicalObjectStore::new();
    let verified_base = base_state(&store, 3, None);
    let mutation_base = StateVersionV1::new(4, verified_base.manifest());
    let bytes = data_batch(mutation_base, "wrong-base");
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let verified = VerifiedDagState::<_, TestSemanticProfile>::admit(
        &store,
        verified_base,
        SemanticValidationLimits::unbounded(),
    )
    .unwrap();
    assert!(matches!(
        OpenDagTransaction::new(verified, view, DagTransactionLimits::UNBOUNDED),
        Err(DagTransactionError::BaseVersion {
            verified,
            mutation,
        }) if verified == verified_base && mutation == mutation_base
    ));
}

fn continuation_fixture(
    base: StateVersionV1,
    include_join: bool,
) -> (Vec<u8>, CanonicalBlob, ObjectRefV1) {
    let channels = vec!["channel".to_owned()];
    let patterns = vec!["pattern".to_owned()];
    let waiting = WaitingContinuation::create(
        &channels,
        &patterns,
        &"continuation".to_owned(),
        false,
        BTreeSet::from([0]),
    );
    let mut source_payload = Vec::new();
    encode_continuation_source_into(&waiting, &mut source_payload);
    let source =
        CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, source_payload).unwrap();
    let source_ref = source.object_ref();
    let prefix = RhoPathV1::continuation_prefix(&channels);
    let continuation_path =
        RhoPathV1::collection_record_path(&prefix, &encode_continuation_identity(&waiting));
    let mut join_path = RhoPathV1::join_prefix(&channels[0]);
    join_path.extend_from_slice(&encode_join_record(&channels));
    let mut batch = RSpaceMutationBatchV1::new(base).unwrap();
    batch
        .record_continuation(&continuation_path, None, Some(source_ref))
        .unwrap();
    if include_join {
        batch.record_join(&join_path, 0, 1).unwrap();
    }
    (encode(&batch.plan().unwrap()), source, source_ref)
}

#[test]
fn referenced_continuation_source_is_sealed_and_semantically_verified() {
    let store = CanonicalObjectStore::new();
    let base = base_state(&store, 9, None);
    let (bytes, source, expected_source) = continuation_fixture(base, true);
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let verified = VerifiedDagState::<_, TestSemanticProfile>::admit(
        &store,
        base,
        SemanticValidationLimits::unbounded(),
    )
    .unwrap();
    let mut open =
        OpenDagTransaction::new(verified, view, DagTransactionLimits::UNBOUNDED).unwrap();
    let (source_ref, admission) = open
        .stage_continuation_source(encode(&source).into())
        .unwrap();
    assert_eq!(source_ref, expected_source);
    assert_eq!(admission, crate::rspace::persistence::ObjectAdmission::Inserted);
    let sealed = open
        .seal()
        .expect("the source, continuation, and join form a coherent target");
    assert!(
        sealed
            .objects()
            .iter()
            .any(|object| object.reference() == source_ref)
    );
    let report = sealed.diagnostics().semantic;
    assert_eq!(report.changed_continuations, 1);
    assert_eq!(report.changed_joins, 1);
    assert_eq!(report.join_lookups, 1);
}

#[test]
fn ambient_equal_source_bytes_remain_in_the_portable_sealed_delta() {
    let store = CanonicalObjectStore::new();
    let base = base_state(&store, 10, None);
    let (bytes, source, source_ref) = continuation_fixture(base, true);
    assert_eq!(admit(&store, &source), source_ref);
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let verified = VerifiedDagState::<_, TestSemanticProfile>::admit(
        &store,
        base,
        SemanticValidationLimits::unbounded(),
    )
    .unwrap();
    let mut open =
        OpenDagTransaction::new(verified, view, DagTransactionLimits::UNBOUNDED).unwrap();
    let (_, admission) = open
        .stage_continuation_source(encode(&source).into())
        .unwrap();
    assert_eq!(admission, crate::rspace::persistence::ObjectAdmission::Inserted);
    let sealed = open.seal().expect("the portable delta seals");
    assert!(
        sealed
            .objects()
            .iter()
            .any(|object| object.reference() == source_ref),
        "ambient physical presence must not omit a logically required WAL object"
    );
}

#[test]
fn missing_or_unreachable_sources_fail_without_touching_the_base() {
    let store = CanonicalObjectStore::new();
    let base = base_state(&store, 11, None);
    let base_len = store.len().unwrap();
    let (bytes, source, _) = continuation_fixture(base, true);
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let verified = VerifiedDagState::<_, TestSemanticProfile>::admit(
        &store,
        base,
        SemanticValidationLimits::unbounded(),
    )
    .unwrap();
    let missing = OpenDagTransaction::new(verified, view, DagTransactionLimits::UNBOUNDED)
        .unwrap()
        .seal()
        .expect_err("a referenced source must exist in the layered closure");
    assert!(matches!(
        missing,
        DagTransactionError::Validation(error)
            if matches!(
                error.as_ref(),
                ClosureError::Format(CanonicalFormatError::SemanticCoherence { .. })
            )
    ));
    assert_eq!(store.len().unwrap(), base_len);

    let data = data_batch(base, "unreachable-source");
    let view = MutationBatchViewV1::parse(&data).unwrap();
    let verified = VerifiedDagState::<_, TestSemanticProfile>::admit(
        &store,
        base,
        SemanticValidationLimits::unbounded(),
    )
    .unwrap();
    let mut open =
        OpenDagTransaction::new(verified, view, DagTransactionLimits::UNBOUNDED).unwrap();
    let (_source_ref, _) = open
        .stage_continuation_source(encode(&source).into())
        .unwrap();
    let unreachable = open
        .seal()
        .expect_err("unreferenced staged content must not enter a sealed batch");
    assert!(matches!(
        unreachable,
        DagTransactionError::Validation(error)
            if matches!(
                error.as_ref(),
                ClosureError::Format(CanonicalFormatError::SemanticCoherence { .. })
            )
    ));
    assert_eq!(store.len().unwrap(), base_len);
}

#[test]
fn semantically_incoherent_target_discards_its_complete_private_overlay() {
    let store = CanonicalObjectStore::new();
    let base = base_state(&store, 12, None);
    let base_len = store.len().unwrap();
    let (bytes, source, _) = continuation_fixture(base, false);
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let verified = VerifiedDagState::<_, TestSemanticProfile>::admit(
        &store,
        base,
        SemanticValidationLimits::unbounded(),
    )
    .unwrap();
    let mut open =
        OpenDagTransaction::new(verified, view, DagTransactionLimits::UNBOUNDED).unwrap();
    open.stage_continuation_source(encode(&source).into())
        .unwrap();
    let error = open
        .seal()
        .expect_err("a continuation without its JoinBag membership cannot seal");
    assert!(matches!(
        error,
        DagTransactionError::Validation(error)
            if matches!(
                error.as_ref(),
                ClosureError::Format(CanonicalFormatError::SemanticCoherence { .. })
            )
    ));
    assert_eq!(store.len().unwrap(), base_len);
}

#[test]
fn open_phase_refuses_non_blob_objects_and_preflights_staging_limits() {
    let store = CanonicalObjectStore::new();
    let base = base_state(&store, 13, None);
    let bytes = data_batch(base, "limits");
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let verified = VerifiedDagState::<_, TestSemanticProfile>::admit(
        &store,
        base,
        SemanticValidationLimits::unbounded(),
    )
    .unwrap();
    let mut open =
        OpenDagTransaction::new(verified, view, DagTransactionLimits::UNBOUNDED).unwrap();
    let node: Arc<[u8]> = encode(&CanonicalDagNode::empty()).into();
    assert!(matches!(
        open.stage_continuation_source(node),
        Err(DagTransactionError::Format(error))
            if matches!(error.as_ref(), CanonicalFormatError::UnexpectedObjectKind { .. })
    ));

    let source =
        CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, vec![1, 2, 3]).unwrap();
    let source_bytes: Arc<[u8]> = encode(&source).into();
    let limits = DagTransactionLimits {
        staging: StagingStoreLimits {
            max_objects: 0,
            max_canonical_bytes: u64::MAX,
        },
        ..DagTransactionLimits::UNBOUNDED
    };
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let verified = VerifiedDagState::<_, TestSemanticProfile>::admit(
        &store,
        base,
        SemanticValidationLimits::unbounded(),
    )
    .unwrap();
    let mut limited = OpenDagTransaction::new(verified, view, limits).unwrap();
    assert!(matches!(
        limited.stage_continuation_source(Arc::clone(&source_bytes)),
        Err(DagTransactionError::Stage(error))
            if matches!(
                error.as_ref(),
                StagingStoreError::Format(CanonicalFormatError::ResourceLimit {
                    resource: "staged objects",
                    limit: 0,
                    attempted: 1,
                })
            )
    ));

    let limits = DagTransactionLimits {
        staging: StagingStoreLimits {
            max_objects: u64::MAX,
            max_canonical_bytes: u64::try_from(source_bytes.len()).unwrap() - 1,
        },
        ..DagTransactionLimits::UNBOUNDED
    };
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let verified = VerifiedDagState::<_, TestSemanticProfile>::admit(
        &store,
        base,
        SemanticValidationLimits::unbounded(),
    )
    .unwrap();
    let mut limited = OpenDagTransaction::new(verified, view, limits).unwrap();
    assert!(matches!(
        limited.stage_continuation_source(source_bytes),
        Err(DagTransactionError::Stage(error))
            if matches!(
                error.as_ref(),
                StagingStoreError::Format(CanonicalFormatError::ResourceLimit {
                    resource: "staged canonical bytes",
                    limit,
                    attempted,
                }) if *attempted == *limit + 1
            )
    ));
}

#[test]
fn semantic_budget_failure_precedes_rewrite_and_leaves_the_base_unchanged() {
    let store = CanonicalObjectStore::new();
    let base = base_state(&store, 14, None);
    let base_len = store.len().unwrap();
    let bytes = data_batch(base, "semantic-budget");
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let verified = VerifiedDagState::<_, TestSemanticProfile>::admit(
        &store,
        base,
        SemanticValidationLimits::unbounded(),
    )
    .unwrap();
    let limits = DagTransactionLimits {
        semantic: IncrementalSemanticLimits {
            max_changed_endpoints: 0,
            ..IncrementalSemanticLimits::UNBOUNDED
        },
        ..DagTransactionLimits::UNBOUNDED
    };
    let error = OpenDagTransaction::new(verified, view, limits)
        .unwrap()
        .seal()
        .expect_err("the semantic delta budget is enforced before rewrite");
    assert!(matches!(
        error,
        DagTransactionError::Validation(error)
            if matches!(
                error.as_ref(),
                ClosureError::Format(CanonicalFormatError::ResourceLimit {
                    resource: "changed semantic endpoints",
                    limit: 0,
                    attempted: 1,
                })
            )
    ));
    assert_eq!(store.len().unwrap(), base_len);
}

#[test]
fn exact_path_budget_is_aggregate_across_rewrite_and_closure() {
    let store = CanonicalObjectStore::new();
    let base = base_state(&store, 16, None);
    let bytes = data_batch(base, &"x".repeat(128));
    let view = MutationBatchViewV1::parse(&bytes).unwrap();
    let verified = VerifiedDagState::<_, TestSemanticProfile>::admit(
        &store,
        base,
        SemanticValidationLimits::unbounded(),
    )
    .unwrap();
    let limits = DagTransactionLimits {
        closure: IncrementalClosureLimits {
            max_exact_path_chunks: 0,
            ..IncrementalClosureLimits::UNBOUNDED
        },
        ..DagTransactionLimits::UNBOUNDED
    };
    let error = OpenDagTransaction::new(verified, view, limits)
        .unwrap()
        .seal()
        .expect_err("a closure-side zero cap governs the shared rewrite path arena");
    assert!(matches!(
        error,
        DagTransactionError::Rewrite(error)
            if matches!(
                error.as_ref(),
                RewriteError::Format(CanonicalFormatError::ResourceLimit {
                    resource: "exact path chunks",
                    limit: 0,
                    attempted,
                }) if *attempted > 0
            )
    ));
}

#[test]
fn transaction_reads_scale_with_the_touched_frontier_not_state_cardinality() {
    let one_entry_reads = traced_single_update_reads(1);
    let two_hundred_fifty_six_entry_reads = traced_single_update_reads(256);
    let four_thousand_ninety_six_entry_reads = traced_single_update_reads(4_096);
    eprintln!(
        "transaction base-object reads: cardinality 1 = {one_entry_reads}, cardinality 256 = \
         {two_hundred_fifty_six_entry_reads}, cardinality 4096 = \
         {four_thousand_ninety_six_entry_reads}"
    );
    assert_eq!(one_entry_reads, 2);
    assert_eq!(two_hundred_fifty_six_entry_reads, 5);
    assert!(
        two_hundred_fifty_six_entry_reads <= one_entry_reads + 4,
        "one update read {one_entry_reads} objects at cardinality 1 but \
         {two_hundred_fifty_six_entry_reads} objects at cardinality 256"
    );
    assert!(
        four_thousand_ninety_six_entry_reads <= two_hundred_fifty_six_entry_reads + 2 &&
            four_thousand_ninety_six_entry_reads <= 8,
        "one update read {two_hundred_fifty_six_entry_reads} objects at cardinality 256 but \
         {four_thousand_ninety_six_entry_reads} objects at cardinality 4096"
    );
}

#[test]
fn zero_distinct_object_budget_prevents_the_structural_object_read() {
    let (store, base, _) = traced_data_state(1);
    store.clear_reads();
    let limits = SemanticValidationLimits {
        structural: StructuralValidationLimits {
            max_distinct_objects: 0,
            ..StructuralValidationLimits::unbounded()
        },
        ..SemanticValidationLimits::unbounded()
    };
    let error = match VerifiedDagState::<_, TestSemanticProfile>::admit(&store, base, limits) {
        Err(error) => error,
        Ok(_) => panic!("the structural distinct-object ceiling must reject the manifest"),
    };
    assert!(matches!(
        error,
        ClosureError::Format(CanonicalFormatError::ResourceLimit {
            resource: "distinct objects",
            limit: 0,
            attempted: 1,
        })
    ));
    assert_eq!(
        store.total_reads(),
        1,
        "only the semantic-profile authority read may precede structural preflight"
    );
    assert_eq!(store.read_count(base.manifest().digest()), 1);
}

#[test]
fn hundred_thousand_byte_transition_seals_on_a_small_native_stack() {
    std::thread::Builder::new()
        .name("persistent-transaction-seal-stack-gate".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let store = CanonicalObjectStore::new();
            let base = base_state(&store, 17, None);
            let identity = "x".repeat(100_000);
            let bytes = data_batch(base, &identity);
            let sealed = seal_data(&store, &bytes);
            assert_eq!(sealed.diagnostics().semantic.changed_data, 1);
            assert!(sealed.diagnostics().rewrite.retained_prefix_bytes >= 100_000);
        })
        .unwrap()
        .join()
        .unwrap();
}
