use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use proptest::prelude::*;
use proptest::test_runner::TestRunner;
use shared::rust::test_scratch;

use super::checkpoint_wal::{
    SyntheticCheckpointSource, fixture as checkpoint_source_fixture,
    fixture_with_retained_union_cardinality, fixture_with_root,
    historically_larger_semantic_fixture as historically_larger_checkpoint_source_fixture,
    semantic_fixture as semantic_checkpoint_source_fixture,
};
use super::durable_recovery::{
    CheckpointPackImage, catalog_image, checkpoint_image_from_source, limits,
    valid_checkpoint_image,
};
use super::durable_rspace_authority::assert_v2_owner_checkpoint_refuses_before_durable_mutation;
use super::property_support::independent_inventory_digest;
use super::retention_v2_format_properties::{
    History, RadixDefect, build_radix, encoded, metadata_closure, migrate_checkpoint_source_to_v2,
    model_anchor_bytes, model_decode_key, model_key, radix_defect_sources, v1_manifest_entries,
    v1_source_with_manifest, v2_source_from_objects, v2_source_with_history,
    v2_source_with_radix_child,
};
use super::support::{PersistenceTrialDir, TestSemanticProfile, trial_scratch};
use super::transaction_sealing::data_batch;
use crate::rspace::persistence::checkpoint::{
    CheckpointObjectInventoryV1, CheckpointReferenceVisitErrorV1, GENERATION_CHECKPOINT_DOMAIN,
    GenerationCheckpointDescriptorV1, GenerationCheckpointErrorV1, OrderedObjectInventoryBuilderV1,
};
use crate::rspace::persistence::closure::RetainedUnionClosureReportV1;
use crate::rspace::persistence::durable::{
    AdmittedCheckpointSourceV1, DurableAdmissionErrorV1, DurableAdmissionLimitKindV1,
    DurableAdmissionLimitsV1, DurableAdmissionReportV1, DurableDirectoryV1, DurableIoError,
    DurableIoFailpointV1, DurableStartupErrorV1, DurableTransactionPreparationError,
    DurableWriterLeaseV1, GenerationCheckpointPublicationErrorV1,
    GenerationCheckpointPublicationHookPointV1, GenerationCheckpointPublicationOutcomeV1,
    IndexBindingDispositionV1, InventoryMismatchKindV1, PreparedGenerationCheckpointIndexV1,
    RECLAMATION_RECORD_LEN_V1, ReclamationCandidateV1, RecoveryError, SealedDurableTransactionV1,
    SemanticallyAdmittedDurableWriterV1, TrustedSuccessorPolicyV1, arm_catalog_slot_write_prefix,
    arm_durable_io_failpoint, arm_generation_checkpoint_publication_failpoint,
    arm_generation_checkpoint_publication_hook, arm_reclamation_intent_write_prefix,
    index_file_name, pack_file_name, start_generation_checkpoint_publication_trace,
    take_generation_checkpoint_publication_trace,
};
use crate::rspace::persistence::retention::v2_format::{
    EMPTY_RETENTION_RADIX_NODE, RETENTION_ANCHOR_CANONICAL_LEN, RETENTION_ANCHOR_DOMAIN,
    RETENTION_RADIX_DOMAIN, RetentionAnchorV2, RetentionFormat, RetentionReader,
    empty_retention_radix_ref,
};
use crate::rspace::persistence::retention::v2_reader::{
    RetentionRadixErrorV2, RetentionRadixRefusalV2,
};
use crate::rspace::persistence::wal::{
    GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1, GenerationCheckpointCommitRecordV1,
    PreparedGenerationCheckpointV1,
};
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, CATALOG_SLOT_LEN, CanonicalBlob, CanonicalBlobKind, CanonicalDagNode,
    CanonicalEncode, CanonicalFormatError, CanonicalObjectView, CatalogAuthorityTokenV1,
    CatalogPackStateV1, CatalogSlotV1, ClosureError, DagChild, DagTransactionLimits,
    DurableLimitConfig, DurableLimitError, DurableLimitKind, DurableLimits,
    DurableObjectReadErrorV1, EndpointValue, IndexGenerationV1, LogPackAddress,
    LogPackDecodeLimits, LogPackFormatError, MutationBatchViewV1, ObjectDigest,
    ObjectIndexHeaderV1, ObjectRefV1, OpenDagTransaction, PACK_HEADER_LEN_V1, PackHeaderV1, PackId,
    PackTerminalIdentityV1, RetainedStateV1, RetentionManifestV1, SemanticValidationLimits,
    StateVersionV1, StructuralValidationLimits,
};

pub(super) const FORMAL_PUBLICATION_PHASE_ORDER: [GenerationCheckpointPublicationHookPointV1; 18] = [
    GenerationCheckpointPublicationHookPointV1::AfterPreflightBeforeCleanup,
    GenerationCheckpointPublicationHookPointV1::AfterCleanup,
    GenerationCheckpointPublicationHookPointV1::AfterPrepareWrite,
    GenerationCheckpointPublicationHookPointV1::AfterPrepareSync,
    GenerationCheckpointPublicationHookPointV1::AfterIndexWrite,
    GenerationCheckpointPublicationHookPointV1::AfterIndexSync,
    GenerationCheckpointPublicationHookPointV1::AfterCommitWrite,
    GenerationCheckpointPublicationHookPointV1::AfterCommitSync,
    GenerationCheckpointPublicationHookPointV1::AfterArtifactDirectorySync,
    GenerationCheckpointPublicationHookPointV1::BeforeCatalogCompareAndSwap,
    GenerationCheckpointPublicationHookPointV1::AfterCatalogCasVerifiedBeforeSlotWrite,
    GenerationCheckpointPublicationHookPointV1::AfterCatalogSlotWrite,
    GenerationCheckpointPublicationHookPointV1::AfterCatalogFileSyncBeforeDirectorySync,
    GenerationCheckpointPublicationHookPointV1::AfterCatalogDirectorySync,
    GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery,
    GenerationCheckpointPublicationHookPointV1::AfterPhysicalRecovery,
    GenerationCheckpointPublicationHookPointV1::BeforeSemanticAdmission,
    GenerationCheckpointPublicationHookPointV1::AfterSemanticAdmission,
];

fn write_owner_only(path: &Path, bytes: &[u8]) {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path).unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn install_checkpoint_store(root: &Path) {
    install_checkpoint_image(root, &valid_checkpoint_image());
}

pub(super) fn install_checkpoint_image(root: &Path, image: &CheckpointPackImage) {
    write_owner_only(root.join("catalog.v1").as_path(), &catalog_image(image.catalog));
    write_owner_only(root.join(pack_file_name(PackId::GENESIS)).as_path(), &image.pack);
    write_owner_only(
        root.join(index_file_name(IndexGenerationV1::FIRST))
            .as_path(),
        &image.canonical_index,
    );
}

fn encoded_blob(payload: &[u8]) -> (ObjectRefV1, Vec<u8>) {
    let bytes = CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, payload)
        .unwrap()
        .try_canonical_bytes()
        .unwrap();
    let reference =
        ObjectRefV1::new(ObjectDigest::of_canonical_bytes(&bytes), bytes.len() as u64).unwrap();
    (reference, bytes)
}

fn source_with_declared_current_objects(declared: u64) -> SyntheticCheckpointSource {
    let mut source = checkpoint_source_fixture();
    source.descriptor = GenerationCheckpointDescriptorV1::from_test_inventory(
        source.descriptor.profile_id(),
        source.descriptor.state(),
        source.descriptor.retention_manifest(),
        declared,
        source.descriptor.current_state_canonical_bytes().get(),
        source.descriptor.inventory(),
    )
    .unwrap();
    source
}

fn source_with_unreachable_retained_object() -> SyntheticCheckpointSource {
    source_with_unreachable_retained(encoded_blob(
        b"authenticated but unreachable checkpoint object",
    ))
}

/// A V1 checkpoint whose declared retained union counts `extra`, which no
/// retained state reaches.
fn source_with_unreachable_retained(extra: (ObjectRefV1, Vec<u8>)) -> SyntheticCheckpointSource {
    let mut source = checkpoint_source_fixture();
    let extra_len = extra.0.encoded_len();
    source.push_object(extra);
    source.sort_objects_by_digest();

    let data_bytes = source
        .objects
        .iter()
        .map(|(reference, _)| reference.encoded_len())
        .sum::<u64>();
    let mut ordered = OrderedObjectInventoryBuilderV1::new(source.objects.len() as u64, data_bytes);
    for (reference, _) in &source.objects {
        ordered.push(*reference).unwrap();
    }
    let old_inventory = source.descriptor.inventory();
    let inventory = CheckpointObjectInventoryV1::from_test_ordered(
        RetainedUnionClosureReportV1 {
            distinct_objects: old_inventory.retained_union_objects() + 1,
            canonical_bytes: old_inventory.retained_union_canonical_bytes().get() + extra_len,
        },
        source.descriptor.retention_manifest(),
        ordered.finish().unwrap(),
    )
    .unwrap();
    source.descriptor = GenerationCheckpointDescriptorV1::from_test_inventory(
        source.descriptor.profile_id(),
        source.descriptor.state(),
        source.descriptor.retention_manifest(),
        source.descriptor.current_state_objects(),
        source.descriptor.current_state_canonical_bytes().get(),
        inventory,
    )
    .unwrap();
    source
}

#[track_caller]
fn admit(
    root: &Path,
) -> crate::rspace::persistence::durable::SemanticallyAdmittedDurableWriterV1<TestSemanticProfile> {
    DurableDirectoryV1::open(root)
        .unwrap()
        .recover_and_admit_v1::<TestSemanticProfile>(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap()
}

#[test]
fn restart_admission_recomputes_both_checkpoint_object_counts() {
    let cases = [
        (
            source_with_declared_current_objects(1),
            InventoryMismatchKindV1::CheckpointCurrentStateObjects,
            1,
            2,
        ),
        (
            source_with_unreachable_retained_object(),
            InventoryMismatchKindV1::CheckpointRetainedUnionObjects,
            3,
            2,
        ),
    ];

    for (source, expected_kind, expected_declared, expected_computed) in cases {
        let scratch = test_scratch::acquire("rspace-checkpoint-object-count-readmission-");
        let image = checkpoint_image_from_source(source);
        install_checkpoint_image(scratch.path(), &image);
        let result = DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .recover_and_admit_v1::<TestSemanticProfile>(
                limits(),
                LogPackDecodeLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            );
        match result {
            Err(DurableAdmissionErrorV1::InventoryMismatch {
                kind,
                declared,
                computed,
            }) => {
                assert_eq!(kind, expected_kind);
                assert_eq!(declared, expected_declared);
                assert_eq!(computed, expected_computed);
            }
            Err(error) => panic!("unexpected admission error: {error}"),
            Ok(_) => panic!("incoherent checkpoint count was admitted"),
        }
    }
}

struct ExpectedSuccessor {
    pack: Vec<u8>,
    index: Vec<u8>,
    catalog: [u8; CATALOG_IMAGE_LEN],
    slot: CatalogSlotV1,
}

struct VerifyDuringUnwind<F: FnOnce()> {
    check: Option<F>,
}

impl<F: FnOnce()> VerifyDuringUnwind<F> {
    fn new(check: F) -> Self { Self { check: Some(check) } }
}

impl<F: FnOnce()> Drop for VerifyDuringUnwind<F> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.check.take().unwrap()();
        }
    }
}

fn assert_incremental_unwind_child(case: &str, expected_diagnostic: &str) {
    let scratch = test_scratch::acquire("rspace-incremental-unwind-parent-");
    let marker = scratch.path().join("verified");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "rspace::persistence::tests::durable_publication::incremental_unwind_child",
            "--nocapture",
        ])
        .env("RSPACE_INCREMENTAL_UNWIND_CASE", case)
        .env("RSPACE_INCREMENTAL_UNWIND_MARKER", &marker)
        .env_remove(shared::rust::test_process::ALLOW_EXPECTED_PROBE_CORE);
    let outcome = shared::rust::test_process::run_child_probe(
        &mut command,
        std::time::Duration::from_secs(30),
    )
    .expect("run the incremental publication unwind child");
    match outcome {
        shared::rust::test_process::ProbeOutcome::Failed(failure) => {
            assert_eq!(failure.kind, shared::rust::test_process::ProbeFailureKind::Panicked);
            assert!(failure.stderr.contains(expected_diagnostic), "{}", failure.stderr);
        }
        shared::rust::test_process::ProbeOutcome::Passed => {
            panic!("the incremental publication unwind child did not fail: {case}")
        }
    }
    assert_eq!(fs::read(marker).unwrap(), case.as_bytes());
}

fn incremental_unwind_marker() -> std::path::PathBuf {
    std::env::var_os("RSPACE_INCREMENTAL_UNWIND_MARKER")
        .expect("incremental publication unwind marker")
        .into()
}

#[test]
#[ignore = "isolated incremental publication unwind child"]
fn incremental_unwind_child() {
    let Some(case) = std::env::var_os("RSPACE_INCREMENTAL_UNWIND_CASE") else {
        return;
    };
    let case = case
        .to_str()
        .expect("incremental publication unwind case is UTF-8");
    match case {
        "terminal-capture" => run_terminal_capture_unwind_child(case),
        _ if case.starts_with("phase:") => {
            let index = case["phase:".len()..].parse::<usize>().unwrap();
            let point = [
                GenerationCheckpointPublicationHookPointV1::AfterPrepareWrite,
                GenerationCheckpointPublicationHookPointV1::AfterCommitSync,
                GenerationCheckpointPublicationHookPointV1::AfterCatalogSlotWrite,
                GenerationCheckpointPublicationHookPointV1::BeforeSemanticAdmission,
            ][index];
            let expected = expected_incremental_successor();
            check_borrowed_incremental_crash(
                IncrementalMode::ColdChecked,
                IncrementalCrash::Unwind(point),
                &expected,
            );
        }
        _ => panic!("unknown incremental publication unwind case: {case}"),
    }
}

fn expected_successor(authority: &CatalogAuthorityTokenV1) -> ExpectedSuccessor {
    let source = checkpoint_source_fixture();
    expected_successor_from_source(authority, &source)
}

fn expected_successor_from_source(
    authority: &CatalogAuthorityTokenV1,
    source: &SyntheticCheckpointSource,
) -> ExpectedSuccessor {
    let current = authority.slot();
    let current_pack = PackId::new(current.pack().active_pack()).unwrap();
    let previous = PackTerminalIdentityV1::new(
        current_pack,
        current.commit_sequence(),
        current.commit_digest(),
    );
    let header = PackHeaderV1::successor(current.store_id(), previous).unwrap();
    let prepared = PreparedGenerationCheckpointV1::<TestSemanticProfile, _>::new(
        source,
        current.store_id(),
        header.first_sequence(),
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let mut pack = header.encode().to_vec();
    let prepare_identity = prepared.write_to(&mut pack).unwrap();
    let prepare_address = LogPackAddress::from_pack(
        header.pack_id(),
        PACK_HEADER_LEN_V1,
        prepare_identity.record_len(),
    )
    .unwrap();
    let index_generation = IndexGenerationV1::new(current.pack().index_generation())
        .unwrap()
        .successor()
        .unwrap();
    let index_plan = PreparedGenerationCheckpointIndexV1::new(
        ObjectIndexHeaderV1::new(current.store_id(), index_generation),
        prepare_address,
        &prepared,
    )
    .unwrap();
    let mut index = index_plan.header().encode().to_vec();
    index_plan
        .try_visit_records(|record| {
            index.extend_from_slice(&record.encode());
            Ok::<(), std::convert::Infallible>(())
        })
        .unwrap();
    let commit = GenerationCheckpointCommitRecordV1::new(
        current.store_id(),
        current.commit_digest(),
        prepare_address,
        prepare_identity,
        index_plan.publication(),
    )
    .unwrap();
    let commit_address = LogPackAddress::from_pack(
        header.pack_id(),
        prepare_address.end(),
        GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1,
    )
    .unwrap();
    pack.extend_from_slice(commit.canonical_bytes());
    let descriptor = source.descriptor;
    let pack_state = CatalogPackStateV1::new(
        header.pack_id().get(),
        header.pack_id().get(),
        commit_address.end(),
        index_generation.get(),
        index_plan.exact_tail(),
        descriptor.retention_manifest(),
        descriptor.current_state_canonical_bytes().get(),
        descriptor
            .inventory()
            .retained_union_canonical_bytes()
            .get(),
    )
    .unwrap();
    let slot = current
        .successor(
            current.state(),
            descriptor.state(),
            commit.digest(),
            prepared.sequence(),
            commit_address,
            pack_state,
        )
        .unwrap();
    let catalog = authority.successor_image(slot).unwrap();
    ExpectedSuccessor {
        pack,
        index,
        catalog,
        slot,
    }
}

fn publish(
    writer: crate::rspace::persistence::durable::SemanticallyAdmittedDurableWriterV1<
        TestSemanticProfile,
    >,
) -> Result<
    crate::rspace::persistence::durable::SemanticallyAdmittedDurableWriterV1<TestSemanticProfile>,
    GenerationCheckpointPublicationErrorV1,
> {
    writer.publish_generation_checkpoint_v1(
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
}

fn assert_admitted_objects(
    writer: &SemanticallyAdmittedDurableWriterV1<TestSemanticProfile>,
    objects: &[(ObjectRefV1, Vec<u8>)],
) {
    for (reference, bytes) in objects {
        let actual = writer
            .get_object_bounded(&reference.digest(), reference.encoded_len())
            .unwrap();
        assert_eq!(actual.as_deref(), Some(bytes.as_slice()));
    }
}

#[test]
fn incremental_publication_round_trips_with_target_retention_manifest() {
    let source = checkpoint_source_fixture();
    let scratch = test_scratch::acquire("rspace-incremental-publication-");
    install_checkpoint_image(scratch.path(), &checkpoint_image_from_source(source.clone()));
    let writer = admit(scratch.path());
    let base = writer.authority().slot().state();
    let mutation = data_batch(base, "incremental-publication");
    let view = crate::rspace::persistence::MutationBatchViewV1::parse(&mutation).unwrap();
    let sealed = writer
        .seal_transaction(
            view,
            crate::rspace::persistence::DagTransactionLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
    let sequence = writer
        .authority()
        .slot()
        .commit_sequence()
        .successor()
        .unwrap();
    let prepared = writer
        .prepare_transaction(&sealed, sequence, LogPackDecodeLimits::UNBOUNDED)
        .unwrap();
    let recovered = writer
        .publish_incremental_transaction_v1(
            &prepared,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
    assert_eq!(recovered.authority().slot().state(), sealed.receipt().target());
}

#[derive(Clone, Copy, Debug)]
enum IncrementalCrash {
    Phase(GenerationCheckpointPublicationHookPointV1),
    Torn(usize),
    Unwind(GenerationCheckpointPublicationHookPointV1),
}

#[derive(Clone, Copy, Debug)]
enum IncrementalMode {
    ColdChecked,
    Trusted,
}

fn bind_in_mode<'mutation>(
    mode: IncrementalMode,
    lease: &DurableWriterLeaseV1,
    source: &AdmittedCheckpointSourceV1<TestSemanticProfile>,
    view: MutationBatchViewV1<'mutation>,
) -> SealedDurableTransactionV1<'mutation, TestSemanticProfile> {
    match mode {
        IncrementalMode::ColdChecked => source.seal_transaction(
            lease,
            view,
            DagTransactionLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        ),
        IncrementalMode::Trusted => source.core().bind_trusted_transaction(
            lease,
            OpenDagTransaction::new(
                source.core().verified_state(),
                view,
                DagTransactionLimits::UNBOUNDED,
            )
            .and_then(OpenDagTransaction::seal)
            .expect("seal the borrowed incremental mutation on the authority core"),
            TrustedSuccessorPolicyV1::UNBOUNDED,
        ),
    }
    .expect("bind the borrowed incremental successor")
}

#[test]
fn borrowed_source_preparation_keeps_exact_authority_through_refusal_and_publication() {
    use crate::rspace::persistence::durable::{
        DurableTransactionPreparationError, publish_incremental_from_lease_v1,
    };
    use crate::rspace::persistence::{DagTransactionLimits, MutationBatchViewV1};
    let image = checkpoint_image_from_source(checkpoint_source_fixture());
    let first = trial_scratch("borrowed-preparation-first-");
    let second = trial_scratch("borrowed-preparation-foreign-");
    install_checkpoint_image(first.path(), &image);
    install_checkpoint_image(second.path(), &image);
    let writer = admit(first.path());
    let registry = writer.generation_leases();
    let pin = registry
        .acquire(crate::rspace::persistence::GenerationLeaseKeyV1::from_catalog(writer.catalog()))
        .unwrap();
    let (mut lease, source) = writer.into_parts();
    let (foreign_lease, foreign_source) = admit(second.path()).into_parts();
    assert_eq!(source.authority(), foreign_source.authority());
    let before = incremental_store_bytes(first.path());
    let foreign_before = incremental_store_bytes(second.path());
    let mutation = data_batch(source.authority().slot().state(), "borrowed-incremental");
    let seal = || {
        source
            .open_transaction(
                MutationBatchViewV1::parse(&mutation).unwrap(),
                DagTransactionLimits::UNBOUNDED,
            )
            .unwrap()
            .seal()
            .unwrap()
    };
    assert!(matches!(
        source.bind_durable_transaction(
            &foreign_lease,
            seal(),
            DurableAdmissionLimitsV1::UNBOUNDED
        ),
        Err(DurableTransactionPreparationError::AuthorityMismatch)
    ));
    let sealed = source
        .bind_durable_transaction(&lease, seal(), DurableAdmissionLimitsV1::UNBOUNDED)
        .unwrap();
    let sequence = source
        .authority()
        .slot()
        .commit_sequence()
        .successor()
        .unwrap();
    assert!(matches!(
        source.prepare_transaction(
            &foreign_lease,
            &sealed,
            sequence,
            LogPackDecodeLimits::UNBOUNDED
        ),
        Err(DurableTransactionPreparationError::AuthorityMismatch)
    ));
    assert!(matches!(
        foreign_source.prepare_transaction(
            &lease,
            &sealed,
            sequence,
            LogPackDecodeLimits::UNBOUNDED
        ),
        Err(DurableTransactionPreparationError::AuthorityMismatch)
    ));
    assert!(matches!(
        source.prepare_transaction(
            &lease,
            &sealed,
            sequence.successor().unwrap(),
            LogPackDecodeLimits::UNBOUNDED
        ),
        Err(DurableTransactionPreparationError::AuthorityMismatch)
    ));
    let mut short = LogPackDecodeLimits::UNBOUNDED;
    short.max_record_bytes = 0;
    assert!(matches!(
        source.prepare_transaction(&lease, &sealed, sequence, short),
        Err(DurableTransactionPreparationError::Format(_))
    ));
    assert_eq!(incremental_store_bytes(first.path()), before);
    assert_eq!(incremental_store_bytes(second.path()), foreign_before);
    assert!(registry.shares_registry(&lease.generation_leases()));
    assert!(
        DurableDirectoryV1::open(first.path())
            .unwrap()
            .try_acquire_writer()
            .is_err()
    );
    let prepared = source
        .prepare_transaction(&lease, &sealed, sequence, LogPackDecodeLimits::UNBOUNDED)
        .unwrap();
    assert!(matches!(
        publish_incremental_from_lease_v1(
            &mut lease,
            &foreign_source,
            &prepared,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        ),
        Err(crate::rspace::persistence::durable::IncrementalPublicationErrorV1::Preparation(
            DurableTransactionPreparationError::AuthorityMismatch
        ))
    ));
    assert_eq!(incremental_store_bytes(first.path()), before);
    assert_eq!(incremental_store_bytes(second.path()), foreign_before);
    let successor = publish_incremental_from_lease_v1(
        &mut lease,
        &source,
        &prepared,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .unwrap();
    let expected = expected_incremental_successor();
    assert_eq!(successor.authority().slot(), expected.slot);
    let [catalog, pack, index] = incremental_store_bytes(first.path());
    assert_eq!(catalog, expected.catalog);
    assert_eq!(pack, expected.pack);
    assert_eq!(index, expected.index);
    assert!(matches!(
        successor.prepare_transaction(&lease, &sealed, sequence, LogPackDecodeLimits::UNBOUNDED),
        Err(DurableTransactionPreparationError::AuthorityMismatch)
    ));
    assert!(registry.shares_registry(&lease.generation_leases()));
    assert!(
        DurableDirectoryV1::open(first.path())
            .unwrap()
            .try_acquire_writer()
            .is_err()
    );
    assert_eq!(incremental_store_bytes(second.path()), foreign_before);
    drop(pin);
    drop(lease);
    assert!(
        DurableDirectoryV1::open(first.path())
            .unwrap()
            .try_acquire_writer()
            .is_ok()
    );
    assert_eq!(admit(first.path()).catalog(), expected.slot);
}

fn expected_incremental_successor() -> ExpectedSuccessor {
    let scratch = trial_scratch("borrowed-incremental-control-");
    install_checkpoint_image(
        scratch.path(),
        &checkpoint_image_from_source(checkpoint_source_fixture()),
    );
    let writer = admit(scratch.path());
    let mutation = data_batch(writer.catalog().state(), "borrowed-incremental");
    let sealed = writer
        .seal_transaction(
            crate::rspace::persistence::MutationBatchViewV1::parse(&mutation).unwrap(),
            crate::rspace::persistence::DagTransactionLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
    let prepared = writer
        .prepare_transaction(
            &sealed,
            writer.catalog().commit_sequence().successor().unwrap(),
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap();
    let successor = writer
        .publish_incremental_transaction_v1(
            &prepared,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
    let [catalog, pack, index] = incremental_store_bytes(scratch.path());
    ExpectedSuccessor {
        pack,
        index,
        catalog: catalog.try_into().unwrap(),
        slot: successor.catalog(),
    }
}

fn check_borrowed_incremental_crash(
    mode: IncrementalMode,
    crash: IncrementalCrash,
    expected: &ExpectedSuccessor,
) {
    use GenerationCheckpointPublicationHookPointV1 as Point;

    use crate::rspace::persistence::GenerationLeaseKeyV1;
    use crate::rspace::persistence::durable::{
        DurableTransactionPreparationError, IncrementalPublicationErrorV1, admit_physical_source,
        admit_physical_writer, publish_incremental_from_lease_v1,
        publish_incremental_with_terminal_v1, publish_trusted_incremental_v1,
        recover_physical_from_lease_v1, recover_physical_with_lease_v1,
    };

    let scratch = trial_scratch("borrowed-incremental-");
    install_checkpoint_image(
        scratch.path(),
        &checkpoint_image_from_source(checkpoint_source_fixture()),
    );
    let writer = admit(scratch.path());
    let old = writer.catalog();
    let old_bytes = incremental_store_bytes(scratch.path());
    let mutation = data_batch(old.state(), "borrowed-incremental");
    let view = crate::rspace::persistence::MutationBatchViewV1::parse(&mutation).unwrap();
    let (lease, source) = writer.into_parts();
    let sealed = bind_in_mode(mode, &lease, &source, view);
    let target = sealed.receipt().target();
    let prepared = source
        .prepare_transaction(
            &lease,
            &sealed,
            old.commit_sequence()
                .successor()
                .expect("the admitted commit sequence has a successor"),
            LogPackDecodeLimits::UNBOUNDED,
        )
        .expect("prepare the borrowed incremental successor");
    let registry = lease.generation_leases();
    let generation = GenerationLeaseKeyV1::from_catalog(old);
    let pin = registry.acquire(generation).unwrap();
    let lease_cell = std::cell::RefCell::new(Some(lease));
    let source_cell = std::cell::RefCell::new(Some(source));
    let terminal = std::cell::Cell::new(None);
    let verify_post = || {
        let before_terminal = matches!(
            crash,
            IncrementalCrash::Phase(
                Point::AfterPrepareWrite |
                    Point::AfterPrepareSync |
                    Point::AfterIndexWrite |
                    Point::AfterIndexSync
            ) | IncrementalCrash::Unwind(
                Point::AfterPrepareWrite |
                    Point::AfterPrepareSync |
                    Point::AfterIndexWrite |
                    Point::AfterIndexSync
            )
        );
        assert_eq!(terminal.get(), (!before_terminal).then_some(expected.slot), "{crash:?}");
        let mut lease = lease_cell.borrow_mut().take().unwrap();
        let source_ref = source_cell.borrow();
        let source = source_ref.as_ref().unwrap();
        assert!(registry.shares_registry(&lease.generation_leases()));
        assert!(registry.has_active(generation).unwrap());
        let observer = lease.directory().try_clone_reader().unwrap();
        assert!(matches!(
            observer.try_clone_reader().unwrap().try_acquire_writer(),
            Err(DurableIoError::WriterLockContended { .. })
        ));
        let physical =
            recover_physical_from_lease_v1(&mut lease, limits(), LogPackDecodeLimits::UNBOUNDED)
                .unwrap();
        let recovered = admit_physical_source::<TestSemanticProfile>(
            &lease,
            physical,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
        let observed = recovered.authority().slot();
        assert_eq!(target, expected.slot.state());
        match crash {
            IncrementalCrash::Torn(_) => {
                assert!(observed == old || observed == expected.slot, "{crash:?}");
            }
            IncrementalCrash::Phase(point) | IncrementalCrash::Unwind(point) => {
                let expected_catalog = match point {
                    Point::AfterCatalogSlotWrite |
                    Point::AfterCatalogFileSyncBeforeDirectorySync |
                    Point::AfterCatalogDirectorySync |
                    Point::AfterCatalogSyncBeforeRecovery |
                    Point::AfterPhysicalRecovery |
                    Point::BeforeSemanticAdmission |
                    Point::AfterSemanticAdmission => expected.slot,
                    _ => old,
                };
                assert_eq!(observed, expected_catalog, "{crash:?}");
            }
        }
        let recovered_catalog = *recovered.authority().image();
        let cold_physical =
            recover_physical_from_lease_v1(&mut lease, limits(), LogPackDecodeLimits::UNBOUNDED)
                .unwrap();
        let cold = admit_physical_source::<TestSemanticProfile>(
            &lease,
            cold_physical,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
        assert_eq!(cold.authority().slot(), observed, "{crash:?}");
        assert_eq!(cold.authority().image(), &recovered_catalog, "{crash:?}");
        drop(cold);
        let pack_path = scratch.path().join(pack_file_name(PackId::GENESIS));
        let pack_after = fs::read(&pack_path).unwrap();
        assert_eq!(&pack_after[..old_bytes[1].len()], &old_bytes[1]);

        if observed == old && source.authority() != recovered.authority() {
            let before_retry = incremental_store_bytes(scratch.path());
            let refused = match mode {
                IncrementalMode::ColdChecked => publish_incremental_from_lease_v1(
                    &mut lease,
                    &recovered,
                    &prepared,
                    limits(),
                    LogPackDecodeLimits::UNBOUNDED,
                    DurableAdmissionLimitsV1::UNBOUNDED,
                )
                .map(|successor| successor.authority().slot()),
                IncrementalMode::Trusted => publish_trusted_incremental_v1(
                    &mut lease,
                    recovered.core(),
                    &prepared,
                    TrustedSuccessorPolicyV1::UNBOUNDED,
                    DurableLimits::UNBOUNDED,
                    |_| {},
                )
                .map(|successor| successor.authority().slot()),
            };
            assert!(
                matches!(
                    refused,
                    Err(IncrementalPublicationErrorV1::Preparation(
                        DurableTransactionPreparationError::AuthorityMismatch
                    ))
                ),
                "{crash:?}"
            );
            assert_eq!(incremental_store_bytes(scratch.path()), before_retry, "{crash:?}");
            let physical =
                recover_physical_with_lease_v1(lease, limits(), LogPackDecodeLimits::UNBOUNDED)
                    .unwrap();
            let retry_writer = admit_physical_writer::<TestSemanticProfile>(
                physical,
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
            .unwrap();
            assert!(registry.shares_registry(&retry_writer.generation_leases()));
            assert_eq!(retry_writer.authority().image(), &recovered_catalog);
            let (retry_lease, retry_source) = retry_writer.into_parts();
            let rebound = bind_in_mode(mode, &retry_lease, &retry_source, view);
            assert_eq!(rebound.receipt(), sealed.receipt());
            let retry_prepared = retry_source
                .prepare_transaction(
                    &retry_lease,
                    &rebound,
                    old.commit_sequence()
                        .successor()
                        .expect("the admitted commit sequence has a successor"),
                    LogPackDecodeLimits::UNBOUNDED,
                )
                .expect("prepare the rebound incremental successor");
            lease = retry_lease;
            let retried = match mode {
                IncrementalMode::ColdChecked => publish_incremental_from_lease_v1(
                    &mut lease,
                    &retry_source,
                    &retry_prepared,
                    limits(),
                    LogPackDecodeLimits::UNBOUNDED,
                    DurableAdmissionLimitsV1::UNBOUNDED,
                )
                .map(|successor| successor.authority().slot()),
                IncrementalMode::Trusted => publish_trusted_incremental_v1(
                    &mut lease,
                    retry_source.core(),
                    &retry_prepared,
                    TrustedSuccessorPolicyV1::UNBOUNDED,
                    DurableLimits::UNBOUNDED,
                    |_| {},
                )
                .map(|successor| successor.authority().slot()),
            }
            .expect("retry the rebound incremental publication");
            assert_eq!(retried, expected.slot, "{crash:?}");
        } else if observed == old {
            let retried = match mode {
                IncrementalMode::ColdChecked => publish_incremental_from_lease_v1(
                    &mut lease,
                    &recovered,
                    &prepared,
                    limits(),
                    LogPackDecodeLimits::UNBOUNDED,
                    DurableAdmissionLimitsV1::UNBOUNDED,
                )
                .map(|successor| successor.authority().slot()),
                IncrementalMode::Trusted => publish_trusted_incremental_v1(
                    &mut lease,
                    recovered.core(),
                    &prepared,
                    TrustedSuccessorPolicyV1::UNBOUNDED,
                    DurableLimits::UNBOUNDED,
                    |_| {},
                )
                .map(|successor| successor.authority().slot()),
            }
            .expect("retry the recovered incremental publication");
            assert_eq!(retried, expected.slot, "{crash:?}");
        }

        assert_eq!(fs::read(&pack_path).unwrap(), expected.pack, "{crash:?}");
        assert_eq!(
            fs::read(
                scratch
                    .path()
                    .join(index_file_name(IndexGenerationV1::FIRST))
            )
            .unwrap(),
            expected.index,
            "{crash:?}"
        );

        assert!(registry.has_active(generation).unwrap());
        drop(recovered);
        drop(source_ref);
        drop(source_cell.borrow_mut().take());
        drop(pin);
        assert!(!registry.has_active(generation).unwrap());
        drop(lease);
        drop(
            observer
                .try_acquire_writer()
                .expect("explicit owner release must be observable"),
        );
        let reopened = admit(scratch.path());
        assert_eq!(reopened.catalog(), expected.slot, "{crash:?}");
        assert_eq!(reopened.authority().image(), &expected.catalog, "{crash:?}");
    };

    match crash {
        IncrementalCrash::Phase(point) => arm_generation_checkpoint_publication_failpoint(point),
        IncrementalCrash::Torn(prefix) => arm_catalog_slot_write_prefix(prefix),
        IncrementalCrash::Unwind(point) => {
            arm_generation_checkpoint_publication_hook(point, || {
                panic!("injected incremental publication unwind");
            })
        }
    }
    let run_publication = || {
        let mut lease_ref = lease_cell.borrow_mut();
        let source_ref = source_cell.borrow();
        let lease = lease_ref
            .as_mut()
            .expect("the publication owns the borrowed lease");
        let source = source_ref
            .as_ref()
            .expect("the publication borrows the admitted source");
        let capture = |value: CatalogSlotV1| {
            assert!(terminal.replace(Some(value)).is_none());
        };
        match mode {
            IncrementalMode::ColdChecked => publish_incremental_with_terminal_v1(
                lease,
                source,
                &prepared,
                limits(),
                LogPackDecodeLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
                capture,
            )
            .map(|successor| successor.authority().slot()),
            IncrementalMode::Trusted => publish_trusted_incremental_v1(
                lease,
                source.core(),
                &prepared,
                TrustedSuccessorPolicyV1::UNBOUNDED,
                DurableLimits::UNBOUNDED,
                capture,
            )
            .map(|successor| successor.authority().slot()),
        }
    };
    if matches!(crash, IncrementalCrash::Unwind(_)) {
        let marker = incremental_unwind_marker();
        let case = std::env::var("RSPACE_INCREMENTAL_UNWIND_CASE").unwrap();
        let _verify = VerifyDuringUnwind::new(|| {
            verify_post();
            fs::write(marker, case.as_bytes()).unwrap();
        });
        let _ = run_publication();
    } else {
        let error = match run_publication() {
            Err(error) => error,
            Ok(_) => panic!("incremental fault did not fire: {crash:?}"),
        };
        use GenerationCheckpointPublicationOutcomeV1 as Outcome;
        let expected_outcome = match crash {
            IncrementalCrash::Torn(_) | IncrementalCrash::Phase(Point::AfterCatalogSlotWrite) => {
                Outcome::Indeterminate
            }
            IncrementalCrash::Phase(
                Point::AfterCatalogFileSyncBeforeDirectorySync |
                Point::AfterCatalogDirectorySync |
                Point::AfterCatalogSyncBeforeRecovery |
                Point::AfterPhysicalRecovery |
                Point::BeforeSemanticAdmission |
                Point::AfterSemanticAdmission,
            ) => Outcome::Published,
            _ => Outcome::DefinitelyNotPublished,
        };
        assert_eq!(error.publication_outcome(), expected_outcome, "{crash:?}");
        verify_post();
    }
}

#[test]
fn borrowed_incremental_publication_recovers_and_retries_every_phase() {
    let expected = expected_incremental_successor();
    for point in FORMAL_PUBLICATION_PHASE_ORDER.into_iter().skip(2) {
        check_borrowed_incremental_crash(
            IncrementalMode::ColdChecked,
            IncrementalCrash::Phase(point),
            &expected,
        );
    }
}

pub(super) fn production_trusted_crash_recovers_and_retries_every_phase() {
    let expected = expected_incremental_successor();
    for point in FORMAL_PUBLICATION_PHASE_ORDER[2..=14].iter().copied() {
        check_borrowed_incremental_crash(
            IncrementalMode::Trusted,
            IncrementalCrash::Phase(point),
            &expected,
        );
    }
    for prefix in 0..CATALOG_SLOT_LEN {
        check_borrowed_incremental_crash(
            IncrementalMode::Trusted,
            IncrementalCrash::Torn(prefix),
            &expected,
        );
    }
}

#[test]
fn incremental_terminal_capture_precedes_commit_and_survives_callback_unwind() {
    assert_incremental_unwind_child(
        "terminal-capture",
        "injected unwind after exact terminal capture",
    );
}

fn run_terminal_capture_unwind_child(case: &str) {
    use crate::rspace::persistence::durable::{
        admit_physical_source, publish_incremental_with_terminal_v1, recover_physical_from_lease_v1,
    };
    use crate::rspace::persistence::{
        DagTransactionLimits, GenerationLeaseKeyV1, MutationBatchViewV1,
    };
    let expected = expected_incremental_successor();
    let scratch = trial_scratch("incremental-terminal-capture-");
    install_checkpoint_image(
        scratch.path(),
        &checkpoint_image_from_source(checkpoint_source_fixture()),
    );
    let (lease, source) = admit(scratch.path()).into_parts();
    let old = source.authority().slot();
    let registry = lease.generation_leases();
    let generation = GenerationLeaseKeyV1::from_catalog(old);
    let pin = registry.acquire(generation).unwrap();
    let mutation = data_batch(old.state(), "borrowed-incremental");
    let sealed = source
        .seal_transaction(
            &lease,
            MutationBatchViewV1::parse(&mutation).unwrap(),
            DagTransactionLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
    let prepared = source
        .prepare_transaction(
            &lease,
            &sealed,
            old.commit_sequence().successor().unwrap(),
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap();
    let old_catalog = fs::read(scratch.path().join("catalog.v1")).unwrap();
    let captured = std::cell::Cell::new(None);
    let lease = std::cell::RefCell::new(lease);
    let marker = incremental_unwind_marker();
    let _verify = VerifyDuringUnwind::new(|| {
        assert_eq!(captured.get(), Some(expected.slot));
        assert!(registry.has_active(generation).unwrap());
        let mut lease = lease.borrow_mut();
        assert!(
            lease
                .directory()
                .try_clone_reader()
                .unwrap()
                .try_acquire_writer()
                .is_err()
        );
        let physical =
            recover_physical_from_lease_v1(&mut lease, limits(), LogPackDecodeLimits::UNBOUNDED)
                .unwrap();
        let recovered = admit_physical_source::<TestSemanticProfile>(
            &lease,
            physical,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
        assert_eq!(recovered.authority().slot(), old);
        let mut calls = 0;
        let successor = publish_incremental_with_terminal_v1(
            &mut lease,
            &recovered,
            &prepared,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
            |terminal| {
                calls += 1;
                assert_eq!(Some(terminal), captured.get());
            },
        )
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(successor.authority().slot(), expected.slot);
        let [catalog, pack, index] = incremental_store_bytes(scratch.path());
        assert_eq!(catalog, expected.catalog);
        assert_eq!(pack, expected.pack);
        assert_eq!(index, expected.index);
        assert!(registry.has_active(generation).unwrap());
        drop(pin);
        fs::write(&marker, case.as_bytes()).unwrap();
    });
    let mut lease_ref = lease.borrow_mut();
    let _ = publish_incremental_with_terminal_v1(
        &mut lease_ref,
        &source,
        &prepared,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
        |terminal| {
            captured.set(Some(terminal));
            let [catalog, pack, index] = incremental_store_bytes(scratch.path());
            assert_eq!(catalog, old_catalog);
            assert_eq!(pack.len() as u64, terminal.commit_address().offset());
            assert_eq!(pack, expected.pack[..pack.len()]);
            assert_eq!(index, expected.index);
            panic!("injected unwind after exact terminal capture");
        },
    );
}

#[test]
fn borrowed_incremental_publication_retains_authority_for_every_torn_slot_prefix() {
    let expected = expected_incremental_successor();
    for prefix in 0..crate::rspace::persistence::CATALOG_SLOT_LEN {
        check_borrowed_incremental_crash(
            IncrementalMode::ColdChecked,
            IncrementalCrash::Torn(prefix),
            &expected,
        );
    }
}

#[test]
fn borrowed_incremental_publication_unwind_keeps_the_outer_owners_capabilities() {
    for index in 0..4 {
        assert_incremental_unwind_child(
            &format!("phase:{index}"),
            "injected incremental publication unwind",
        );
    }
}

#[test]
fn borrowed_incremental_publication_never_extends_a_short_appointed_pack() {
    use crate::rspace::persistence::durable::{
        FileLengthExpectation, IncrementalPublicationErrorV1, publish_incremental_from_lease_v1,
    };

    let image = checkpoint_image_from_source(checkpoint_source_fixture());
    let appointed = image.catalog.pack().durable_pack_tail();
    for short in [0, PACK_HEADER_LEN_V1 - 1, PACK_HEADER_LEN_V1, appointed - 1] {
        let scratch = trial_scratch("borrowed-incremental-short-");
        install_checkpoint_image(scratch.path(), &image);
        let writer = admit(scratch.path());
        let mutation = data_batch(writer.catalog().state(), "borrowed-incremental");
        let sealed = writer
            .seal_transaction(
                crate::rspace::persistence::MutationBatchViewV1::parse(&mutation).unwrap(),
                crate::rspace::persistence::DagTransactionLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
            .unwrap();
        let prepared = writer
            .prepare_transaction(
                &sealed,
                writer.catalog().commit_sequence().successor().unwrap(),
                LogPackDecodeLimits::UNBOUNDED,
            )
            .unwrap();
        let (mut lease, source) = writer.into_parts();
        OpenOptions::new()
            .write(true)
            .open(scratch.path().join(pack_file_name(PackId::GENESIS)))
            .unwrap()
            .set_len(short)
            .unwrap();
        let before = incremental_store_bytes(scratch.path());
        let result = publish_incremental_from_lease_v1(
            &mut lease,
            &source,
            &prepared,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        );
        assert!(
            matches!(
                result,
                Err(IncrementalPublicationErrorV1::Io(DurableIoError::Length {
                    expected: FileLengthExpectation::AtLeast(minimum), actual, ..
                })) if actual == short && minimum > short
            ),
            "short pack length {short}"
        );
        assert_eq!(incremental_store_bytes(scratch.path()), before);
    }
}

#[test]
fn borrowed_incremental_publication_trims_only_under_exact_store_authority() {
    use crate::rspace::persistence::durable::{
        DurableTransactionPreparationError, IncrementalPublicationErrorV1,
        publish_incremental_from_lease_v1,
    };

    #[derive(Clone, Copy, Debug)]
    enum Authority {
        Exact,
        StaleCatalog,
        ForeignRegistry,
    }

    let expected = expected_incremental_successor();
    let image = checkpoint_image_from_source(checkpoint_source_fixture());
    for authority in [Authority::Exact, Authority::StaleCatalog, Authority::ForeignRegistry] {
        let scratch = trial_scratch("borrowed-incremental-suffix-");
        let foreign = trial_scratch("borrowed-incremental-foreign-");
        install_checkpoint_image(scratch.path(), &image);
        install_checkpoint_image(foreign.path(), &image);
        let writer = admit(scratch.path());
        let mutation = data_batch(writer.catalog().state(), "borrowed-incremental");
        let sealed = writer
            .seal_transaction(
                crate::rspace::persistence::MutationBatchViewV1::parse(&mutation).unwrap(),
                crate::rspace::persistence::DagTransactionLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
            .unwrap();
        let prepared = writer
            .prepare_transaction(
                &sealed,
                writer.catalog().commit_sequence().successor().unwrap(),
                LogPackDecodeLimits::UNBOUNDED,
            )
            .unwrap();
        let (mut lease, source) = writer.into_parts();
        let (mut foreign_lease, foreign_source) = admit(foreign.path()).into_parts();
        assert_eq!(source.authority(), foreign_source.authority());
        let (root, lease) = match authority {
            Authority::ForeignRegistry => (foreign.path(), &mut foreign_lease),
            _ => (scratch.path(), &mut lease),
        };
        let pack_path = root.join(pack_file_name(PackId::GENESIS));
        OpenOptions::new()
            .append(true)
            .open(&pack_path)
            .unwrap()
            .write_all(&vec![0x93; expected.pack.len() + 1])
            .unwrap();
        if matches!(authority, Authority::StaleCatalog) {
            let mut changed = *source.authority().image();
            let offset = source.authority().selection().inactive_slot_index() *
                crate::rspace::persistence::CATALOG_SLOT_LEN;
            changed[offset] ^= 1;
            fs::write(root.join("catalog.v1"), changed).unwrap();
        }
        let before = incremental_store_bytes(root);
        let result = publish_incremental_from_lease_v1(
            lease,
            &source,
            &prepared,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        );
        match authority {
            Authority::Exact => {
                let successor = result.unwrap();
                assert_eq!(successor.authority().slot(), expected.slot);
                assert_eq!(incremental_store_bytes(root), [
                    expected.catalog.to_vec(),
                    expected.pack.clone(),
                    expected.index.clone(),
                ]);
                assert_eq!(&expected.pack[..image.pack.len()], image.pack.as_ref());
            }
            Authority::StaleCatalog => {
                assert!(matches!(
                    result,
                    Err(IncrementalPublicationErrorV1::CatalogCompareAndSwap)
                ));
                assert_eq!(incremental_store_bytes(root), before);
            }
            Authority::ForeignRegistry => {
                assert!(matches!(
                    result,
                    Err(IncrementalPublicationErrorV1::Preparation(
                        DurableTransactionPreparationError::AuthorityMismatch
                    ))
                ));
                assert_eq!(incremental_store_bytes(root), before);
            }
        }
    }
}

/// SI-01 to SI-03 under each retention writer, from a V1 and a semantic
/// source.
fn check_incremental_inventory_history(operations: &[(u8, u8)]) {
    for writer_format in [RetentionFormat::V1, RetentionFormat::V2] {
        for source in [checkpoint_source_fixture(), semantic_checkpoint_source_fixture()] {
            check_incremental_inventory_history_from(source, writer_format, operations);
        }
    }
}

/// Publish `operations` as successors of `source` under `writer_format`. At
/// every step the sealed target, the catalog's incremental inventory and cold
/// admission after a restart must equal what an oracle store of the semantic
/// objects computes. The V1 writer's retained states are those its sealed
/// manifest lists. The V2 writer's are `RetentionOracle`'s: the sealed anchor
/// must be the oracle's, cold admission must appoint the oracle's metadata
/// closure, and no metadata object enters the semantic oracle store.
fn check_incremental_inventory_history_from(
    source: SyntheticCheckpointSource,
    writer_format: RetentionFormat,
    operations: &[(u8, u8)],
) {
    use crate::rspace::data_index::RSpaceDataIndex;
    use crate::rspace::internal::Datum;
    use crate::rspace::persistence::{
        CanonicalObjectStore, DagTransactionLimits, MutableObjectStore, MutationBatchViewV1,
        RSpaceMutationBatchV1, RetainedUnionClosureLimitsV1, RetentionManifestView,
        StructuralClosureLedgerV1, verify_structural_closure,
        verify_structural_closure_in_ledger_with_limits,
    };

    let oracle = CanonicalObjectStore::default();
    for (reference, bytes) in &source.objects {
        oracle
            .put_if_absent_or_equal(*reference, bytes.clone().into())
            .unwrap();
    }
    let mut retention_oracle = RetentionOracle::of(&source);
    let scratch = trial_scratch("incremental-inventory-");
    install_checkpoint_image(scratch.path(), &checkpoint_image_from_source(source));
    let mut writer = admitted_writing(scratch.path(), writer_format);
    let mut counts = [0u64; 4];
    for (step, &(identity, after)) in operations.iter().enumerate() {
        let index = usize::from(identity);
        let before = counts[index];
        let after = u64::from(after);
        if before == after {
            continue;
        }
        let channel = "inventory-channel".to_owned();
        let datum = Datum::create(&channel, format!("datum-{identity}"), false);
        let path = RSpaceDataIndex::datum_path(&channel, &datum);
        let mut mutation = RSpaceMutationBatchV1::new(writer.catalog().state()).unwrap();
        mutation.record_data(&path, before, after).unwrap();
        let bytes = mutation.plan().unwrap().try_canonical_bytes().unwrap();
        let sealed = writer
            .seal_transaction(
                MutationBatchViewV1::parse(&bytes).unwrap(),
                DagTransactionLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
            .unwrap();
        let mut metadata_objects = 0_usize;
        for object in sealed.objects() {
            let bytes = object.canonical_bytes();
            match bytes.starts_with(RETENTION_ANCHOR_DOMAIN) ||
                bytes.starts_with(RETENTION_RADIX_DOMAIN)
            {
                true => metadata_objects += 1,
                false => {
                    oracle
                        .put_if_absent_or_equal(object.reference(), object.canonical_bytes_arc())
                        .expect("the oracle store admits every semantic sealed object");
                }
            }
        }
        let target = sealed.receipt().target();
        let current = verify_structural_closure(&oracle, target.manifest()).unwrap();
        retention_oracle.commit(
            RetainedStateV1::new(target, current.canonical_bytes)
                .expect("the target's closure covers its manifest"),
        );
        let retention_object = sealed
            .objects()
            .iter()
            .find(|object| Some(object.reference()) == sealed.retention_manifest())
            .unwrap();
        let mut retained_manifests = Vec::with_capacity(retention_oracle.history.len() + 1);
        match writer_format {
            RetentionFormat::V1 => {
                assert_eq!(metadata_objects, 0, "step {step}: the V1 writer seals no V2 metadata");
                let retention =
                    RetentionManifestView::parse(retention_object.canonical_bytes()).unwrap();
                assert_eq!(retention.current().state(), target);
                assert_eq!(
                    retention.current().canonical_bytes(),
                    current.canonical_bytes,
                    "step {step}: identity {identity}, count {before} -> {after}"
                );
                for state in std::iter::once(Ok(retention.current())).chain(retention.historical())
                {
                    retained_manifests.push(state.unwrap().state().manifest());
                }
            }
            RetentionFormat::V2 => {
                let (anchor, _) = retention_oracle.metadata();
                assert_eq!(
                    RetentionAnchorV2::parse(retention_object.canonical_bytes()),
                    Ok(anchor),
                    "step {step}: identity {identity}, count {before} -> {after}"
                );
                retained_manifests.push(target.manifest());
                for key in retention_oracle.history.keys() {
                    let state = model_decode_key(key).expect("an oracle key decodes");
                    retained_manifests.push(state.manifest());
                }
            }
        }
        let mut retained = StructuralClosureLedgerV1::new();
        for manifest in retained_manifests {
            verify_structural_closure_in_ledger_with_limits(
                &oracle,
                manifest,
                StructuralValidationLimits::unbounded(),
                &mut retained,
                RetainedUnionClosureLimitsV1::UNBOUNDED,
            )
            .unwrap();
        }
        let sequence = writer.catalog().commit_sequence().successor().unwrap();
        let prepared = writer
            .prepare_transaction(&sealed, sequence, LogPackDecodeLimits::UNBOUNDED)
            .unwrap();
        writer = writer
            .publish_incremental_transaction_v1(
                &prepared,
                limits(),
                LogPackDecodeLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
            .unwrap();
        assert_eq!(
            writer.catalog().pack().current_state_canonical_bytes(),
            current.canonical_bytes
        );
        assert_eq!(
            writer.catalog().pack().retained_canonical_bytes(),
            retained.retained_union_report().canonical_bytes
        );
        drop(writer);
        writer = admitted_writing(scratch.path(), writer_format);
        assert_eq!(writer.catalog().state(), target);
        assert_eq!(writer.report().current_structural, current);
        assert_eq!(writer.report().retained_union, retained.retained_union_report());
        if let RetentionFormat::V2 = writer_format {
            assert_appoints(&writer, &retention_oracle);
        }
        counts[index] = after;
    }
}

#[test]
fn incremental_inventory_tracks_replacement_removal_sharing_and_reuse() {
    check_incremental_inventory_history(&[
        (0, 1),
        (1, 1),
        (0, 3),
        (1, 0),
        (0, 0),
        (0, 1),
        (2, 4),
        (0, 0),
        (2, 0),
    ]);
}

proptest! {
    #[test]
    fn incremental_inventory_matches_cold_admission_at_every_history_step(
        operations in prop::collection::vec((0u8..4, 0u8..5), 1..12),
    ) {
        check_incremental_inventory_history(&operations);
    }
}

fn incremental_store_bytes(root: &Path) -> [Vec<u8>; 3] {
    [
        fs::read(root.join("catalog.v1")).unwrap(),
        fs::read(root.join(pack_file_name(PackId::GENESIS))).unwrap(),
        fs::read(root.join(index_file_name(IndexGenerationV1::FIRST))).unwrap(),
    ]
}

#[test]
fn incremental_inventory_limits_refuse_before_any_durable_mutation() {
    use crate::rspace::persistence::{DagTransactionLimits, MutationBatchViewV1};
    let scratch = trial_scratch("inventory-limits-");
    install_checkpoint_image(
        scratch.path(),
        &checkpoint_image_from_source(checkpoint_source_fixture()),
    );
    let writer = admit(scratch.path());
    let before = incremental_store_bytes(scratch.path());
    let mutation = data_batch(writer.catalog().state(), "inventory-limits");
    for axis in 0..9 {
        let mut limits = DurableAdmissionLimitsV1::UNBOUNDED;
        match axis {
            0 => limits.max_retained_states = 0,
            1 => limits.max_union_objects = 0,
            2 => limits.max_union_canonical_bytes = 0,
            3 => limits.max_object_reads = 0,
            4 => limits.max_object_read_bytes = 0,
            5 => limits.per_state_structural.max_distinct_objects = 0,
            6 => limits.per_state_structural.max_canonical_bytes = 0,
            7 => limits.per_state_structural.max_references = 0,
            8 => limits.per_state_structural.max_worklist = 0,
            _ => unreachable!(),
        }
        let error = match writer.seal_transaction(
            MutationBatchViewV1::parse(&mutation).unwrap(),
            DagTransactionLimits::UNBOUNDED,
            limits,
        ) {
            Err(error) => error,
            Ok(_) => panic!("inventory resource axis {axis} was not enforced"),
        };
        let detail = format!("{error:?}");
        let expected = [
            "RetainedStates",
            "retained union objects",
            "retained union canonical bytes",
            "object reads",
            "object-read bytes",
            "distinct objects",
            "canonical bytes",
            "object references",
            "worklist",
        ][axis];
        assert!(detail.contains(expected), "axis {axis}: {detail}");
        assert_eq!(incremental_store_bytes(scratch.path()), before);
    }
    writer
        .seal_transaction(
            MutationBatchViewV1::parse(&mutation).unwrap(),
            DagTransactionLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
    assert_eq!(incremental_store_bytes(scratch.path()), before);
}

#[test]
fn incremental_inventory_is_bound_to_its_opened_authority_and_exact_sequence() {
    use crate::rspace::persistence::durable::DurableTransactionPreparationError;
    use crate::rspace::persistence::{DagTransactionLimits, MutationBatchViewV1};
    let image = checkpoint_image_from_source(checkpoint_source_fixture());
    let first = trial_scratch("inventory-authority-first-");
    let second = trial_scratch("inventory-authority-second-");
    install_checkpoint_image(first.path(), &image);
    install_checkpoint_image(second.path(), &image);
    let writer = admit(first.path());
    let foreign = admit(second.path());
    assert_eq!(writer.authority(), foreign.authority());
    let before = incremental_store_bytes(second.path());
    let mutation = data_batch(writer.catalog().state(), "inventory-authority");
    let sealed = writer
        .seal_transaction(
            MutationBatchViewV1::parse(&mutation).unwrap(),
            DagTransactionLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
    let sequence = writer.catalog().commit_sequence().successor().unwrap();
    assert!(matches!(
        foreign.prepare_transaction(&sealed, sequence, LogPackDecodeLimits::UNBOUNDED),
        Err(DurableTransactionPreparationError::AuthorityMismatch)
    ));
    assert!(matches!(
        writer.prepare_transaction(
            &sealed,
            sequence.successor().unwrap(),
            LogPackDecodeLimits::UNBOUNDED
        ),
        Err(DurableTransactionPreparationError::AuthorityMismatch)
    ));
    let prepared = writer
        .prepare_transaction(&sealed, sequence, LogPackDecodeLimits::UNBOUNDED)
        .unwrap();
    let outcome = foreign.publish_incremental_transaction_v1(
        &prepared,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    );
    assert!(matches!(
        outcome,
        Err(crate::rspace::persistence::durable::IncrementalPublicationErrorV1::Preparation(
            DurableTransactionPreparationError::AuthorityMismatch
        ))
    ));
    assert_eq!(incremental_store_bytes(second.path()), before);
    let successor = writer
        .publish_incremental_transaction_v1(
            &prepared,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
    assert!(matches!(
        successor.prepare_transaction(&sealed, sequence, LogPackDecodeLimits::UNBOUNDED),
        Err(DurableTransactionPreparationError::AuthorityMismatch)
    ));
}

#[test]
fn incremental_inventory_policy_cannot_change_after_preparation() {
    use crate::rspace::persistence::{DagTransactionLimits, MutationBatchViewV1};
    let scratch = trial_scratch("inventory-policy-");
    install_checkpoint_image(
        scratch.path(),
        &checkpoint_image_from_source(checkpoint_source_fixture()),
    );
    let writer = admit(scratch.path());
    let before = incremental_store_bytes(scratch.path());
    let mutation = data_batch(writer.catalog().state(), "inventory-policy");
    let sealed = writer
        .seal_transaction(
            MutationBatchViewV1::parse(&mutation).unwrap(),
            DagTransactionLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
    let prepared = writer
        .prepare_transaction(
            &sealed,
            writer.catalog().commit_sequence().successor().unwrap(),
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap();
    let changed = DurableAdmissionLimitsV1 {
        max_object_reads: 0,
        ..DurableAdmissionLimitsV1::UNBOUNDED
    };
    assert!(matches!(writer.publish_incremental_transaction_v1(
        &prepared, limits(), LogPackDecodeLimits::UNBOUNDED, changed,
    ), Err(crate::rspace::persistence::durable::IncrementalPublicationErrorV1::Preparation(
        crate::rspace::persistence::durable::DurableTransactionPreparationError::AdmissionPolicyMismatch))));
    assert_eq!(incremental_store_bytes(scratch.path()), before);
}

#[test]
fn incremental_publication_preflight_rejects_missing_target_retention_manifest() {
    let source = checkpoint_source_fixture();
    let scratch = test_scratch::acquire("rspace-incremental-publication-missing-manifest-");
    install_checkpoint_image(scratch.path(), &checkpoint_image_from_source(source));
    let writer = admit(scratch.path());
    let base = writer.authority().slot().state();
    let mutation = data_batch(base, "incremental-publication-missing-manifest");
    let view = crate::rspace::persistence::MutationBatchViewV1::parse(&mutation).unwrap();
    let sealed = writer
        .open_transaction(view, crate::rspace::persistence::DagTransactionLimits::UNBOUNDED)
        .unwrap()
        .seal()
        .unwrap();
    let sequence = writer
        .authority()
        .slot()
        .commit_sequence()
        .successor()
        .unwrap();
    let prepared = crate::rspace::persistence::PreparedLogPackTransactionV1::new(
        &sealed,
        writer.classify_transaction_objects(&sealed).unwrap(),
        writer.catalog().store_id(),
        sequence,
        LogPackDecodeLimits::UNBOUNDED,
    )
    .unwrap();
    let error = match crate::rspace::persistence::durable::require_incremental_retention_manifest(
        &prepared,
    ) {
        Err(error) => error,
        Ok(_) => panic!("missing retention manifest passed publication preflight"),
    };
    assert!(matches!(
        error,
        crate::rspace::persistence::durable::IncrementalPublicationErrorV1::MissingRetentionManifest
    ));
}

fn publication_error(
    result: Result<
        crate::rspace::persistence::durable::SemanticallyAdmittedDurableWriterV1<
            TestSemanticProfile,
        >,
        GenerationCheckpointPublicationErrorV1,
    >,
) -> GenerationCheckpointPublicationErrorV1 {
    match result {
        Err(error) => error,
        Ok(_) => panic!("generation-checkpoint publication unexpectedly succeeded"),
    }
}

#[derive(Clone, Copy, Debug)]
enum RepairableSuccessorIndexDamage {
    Missing,
    Truncated,
    CorruptRecord,
    UnexpectedSuffix,
}

fn damage_repairable_successor_index(
    path: &Path,
    canonical: &[u8],
    damage: RepairableSuccessorIndexDamage,
) {
    match damage {
        RepairableSuccessorIndexDamage::Missing => fs::remove_file(path).unwrap(),
        RepairableSuccessorIndexDamage::Truncated => {
            fs::write(path, &canonical[..canonical.len() - 1]).unwrap();
        }
        RepairableSuccessorIndexDamage::CorruptRecord => {
            let mut damaged = canonical.to_vec();
            let byte = damaged
                .len()
                .checked_sub(1)
                .expect("a canonical index is nonempty");
            damaged[byte] ^= 0x80;
            fs::write(path, damaged).unwrap();
        }
        RepairableSuccessorIndexDamage::UnexpectedSuffix => {
            let mut damaged = canonical.to_vec();
            damaged.extend_from_slice(b"unexpected-index-suffix");
            fs::write(path, damaged).unwrap();
        }
    }
}

fn rewrite_and_sync(path: &Path, bytes: &[u8]) {
    let mut options = OpenOptions::new();
    options.write(true).truncate(true);
    let mut file = options.open(path).unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

/// Executable refinement of CAT-03, NV-13, and NV-14.
///
/// The selected successor pack is authority and therefore fails closed when
/// damaged. Its canonical index is only a derived accelerator: every safely
/// replaceable damage shape is reconstructed from the selected pack without
/// falling back to the previous catalog slot.
#[test]
fn selected_successor_damage_never_falls_back_and_repairs_safe_index() {
    for damage in [
        RepairableSuccessorIndexDamage::Missing,
        RepairableSuccessorIndexDamage::Truncated,
        RepairableSuccessorIndexDamage::CorruptRecord,
        RepairableSuccessorIndexDamage::UnexpectedSuffix,
    ] {
        let scratch = test_scratch::acquire("rspace-selected-successor-index-repair-");
        install_checkpoint_store(scratch.path());
        let writer = admit(scratch.path());
        let expected = expected_successor(writer.authority());
        drop(publish(writer).unwrap());

        let index_path = scratch.path().join(index_file_name(
            IndexGenerationV1::new(expected.slot.pack().index_generation()).unwrap(),
        ));
        damage_repairable_successor_index(&index_path, &expected.index, damage);

        let repaired = admit(scratch.path());
        assert_eq!(
            repaired.report().startup_disposition,
            IndexBindingDispositionV1::RebuiltAndVerified,
            "repairable successor index shape {damage:?}",
        );
        assert_eq!(repaired.authority().image(), &expected.catalog);
        assert_eq!(repaired.authority().slot(), expected.slot);
        assert_eq!(fs::read(&index_path).unwrap(), expected.index);
        drop(repaired);

        let verified = admit(scratch.path());
        assert_eq!(
            verified.report().startup_disposition,
            IndexBindingDispositionV1::VerifiedExisting,
        );
        assert_eq!(verified.authority().image(), &expected.catalog);
    }

    for corrupt in [false, true] {
        let scratch = test_scratch::acquire("rspace-selected-successor-pack-reject-");
        install_checkpoint_store(scratch.path());
        let writer = admit(scratch.path());
        let expected = expected_successor(writer.authority());
        drop(publish(writer).unwrap());

        let pack_path = scratch
            .path()
            .join(pack_file_name(PackId::new(expected.slot.pack().active_pack()).unwrap()));
        let damaged = if corrupt {
            let mut bytes = expected.pack.clone();
            let byte = PACK_HEADER_LEN_V1 as usize + 8;
            bytes[byte] ^= 0x80;
            bytes
        } else {
            expected.pack[..expected.pack.len() - 1].to_vec()
        };
        rewrite_and_sync(&pack_path, &damaged);

        let error = match DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .recover_writer_v1(limits(), LogPackDecodeLimits::UNBOUNDED)
        {
            Err(error) => error,
            Ok(_) => panic!("damaged selected successor pack was accepted"),
        };
        assert!(
            matches!(error, DurableStartupErrorV1::Recovery(_)),
            "authoritative successor damage must be a typed recovery failure: {error}",
        );
        assert_eq!(fs::read(scratch.path().join("catalog.v1")).unwrap(), expected.catalog);

        rewrite_and_sync(&pack_path, &expected.pack);
        let reopened = admit(scratch.path());
        assert_eq!(reopened.authority().image(), &expected.catalog);
        assert_eq!(reopened.authority().slot(), expected.slot);
    }
}

#[test]
fn checkpoint_publication_is_byte_exact_restart_equivalent_and_keeps_one_lease() {
    let scratch = test_scratch::acquire("rspace-durable-publication-golden-");
    install_checkpoint_store(scratch.path());
    let old_pack_path = scratch.path().join(pack_file_name(PackId::GENESIS));
    let old_index_path = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST));
    let writer = admit(scratch.path());
    let old_generation =
        crate::rspace::persistence::GenerationLeaseKeyV1::from_catalog(writer.catalog());
    let registry = writer.generation_leases();
    #[cfg(unix)]
    let independently_opened_registry = DurableDirectoryV1::open(scratch.path())
        .unwrap()
        .generation_leases();
    #[cfg(unix)]
    assert!(registry.shares_registry(&independently_opened_registry));
    let initial_contention = DurableDirectoryV1::open(scratch.path())
        .unwrap()
        .recover_and_admit_v1::<TestSemanticProfile>(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        );
    assert!(matches!(
        initial_contention,
        Err(DurableAdmissionErrorV1::Startup(DurableStartupErrorV1::Io(
            DurableIoError::WriterLockContended { .. }
        )))
    ));
    let old_authority = writer.authority().clone();
    let old_state = writer.verified_state().version();
    let old_retention = writer.retention_manifest_bytes().to_vec();
    let old_report = writer.report();
    let expected = expected_successor(&old_authority);

    let published = publish(writer).unwrap();
    let new_authority = published.authority().clone();
    assert_eq!(new_authority.image(), &expected.catalog);
    assert_eq!(new_authority.slot(), expected.slot);
    assert_eq!(published.verified_state().version(), old_state);
    assert_eq!(published.retention_manifest_bytes(), old_retention);
    let new_report = published.report();
    assert_eq!(new_report.current_semantics, old_report.current_semantics);
    assert_eq!(new_report.retained_states, old_report.retained_states);
    assert_eq!(new_report.retained_union, old_report.retained_union);

    let successor_pack_path = scratch
        .path()
        .join(pack_file_name(PackId::new(expected.slot.pack().active_pack()).unwrap()));
    let successor_index_path = scratch.path().join(index_file_name(
        IndexGenerationV1::new(expected.slot.pack().index_generation()).unwrap(),
    ));
    assert_eq!(fs::read(&successor_pack_path).unwrap(), expected.pack);
    assert_eq!(fs::read(&successor_index_path).unwrap(), expected.index);
    assert_eq!(fs::read(scratch.path().join("catalog.v1")).unwrap(), expected.catalog);
    // Publication has released the predecessor generation, so reclamation
    // removes its physical pack and index.
    assert!(!old_pack_path.exists());
    assert!(!old_index_path.exists());
    assert!(!scratch.path().join("reclaim.v1").exists());
    assert!(matches!(
        registry.acquire(old_generation),
        Err(crate::rspace::persistence::GenerationLeaseError::Retired(key))
            if key == old_generation
    ));
    #[cfg(unix)]
    assert!(matches!(
        independently_opened_registry.acquire(old_generation),
        Err(crate::rspace::persistence::GenerationLeaseError::Retired(key))
            if key == old_generation
    ));
    assert!(
        registry
            .acquire(crate::rspace::persistence::GenerationLeaseKeyV1::from_catalog(
                published.catalog()
            ))
            .is_ok()
    );

    let contention = DurableDirectoryV1::open(scratch.path())
        .unwrap()
        .recover_and_admit_v1::<TestSemanticProfile>(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        );
    assert!(matches!(
        contention,
        Err(DurableAdmissionErrorV1::Startup(DurableStartupErrorV1::Io(
            DurableIoError::WriterLockContended { .. }
        )))
    ));
    drop(published);

    let physical = DurableDirectoryV1::open(scratch.path())
        .unwrap()
        .recover_writer_v1(limits(), LogPackDecodeLimits::UNBOUNDED)
        .unwrap();
    assert_eq!(physical.recovered().state(), old_state);
    assert_eq!(physical.recovered().transactions(), 0);
    assert_eq!(physical.recovered().checkpoints(), 1);
    drop(physical);

    let reopened = admit(scratch.path());
    assert_eq!(reopened.authority(), &new_authority);
    assert_eq!(reopened.verified_state().version(), old_state);
    assert_eq!(reopened.retention_manifest_bytes(), old_retention);
}

#[cfg(unix)]
#[test]
fn checkpoint_reclamation_waits_for_older_epoch_pin_from_independent_open() {
    use crate::rspace::persistence::GenerationLeaseKeyV1;

    let scratch = test_scratch::acquire("rspace-gc-pack-lease-checkpoint-");
    install_checkpoint_store(scratch.path());
    let writer = admit(scratch.path());
    let old = writer.catalog();
    let old_key = GenerationLeaseKeyV1::from_catalog(old);
    let old_pack_path = scratch.path().join(pack_file_name(PackId::GENESIS));
    let old_index_path = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST));
    let registry = DurableDirectoryV1::open(scratch.path())
        .unwrap()
        .generation_leases();
    assert!(registry.shares_registry(&writer.generation_leases()));
    let older_key = GenerationLeaseKeyV1::new(
        old.epoch().saturating_sub(1),
        old_key.first_live_pack(),
        old_key.active_pack(),
        old_key.index_generation(),
    );
    assert_ne!(older_key, old_key);
    let pin = registry.acquire(older_key).unwrap();

    let published = publish(writer).unwrap();
    let successor = published.catalog();
    assert!(old_pack_path.exists());
    assert!(old_index_path.exists());
    assert!(scratch.path().join("reclaim.v1").exists());
    drop(published);

    let reopened = admit(scratch.path());
    assert_eq!(reopened.catalog(), successor);
    assert!(old_pack_path.exists());
    assert!(scratch.path().join("reclaim.v1").exists());
    let (mut lease, _) = reopened.into_parts();
    let candidate = lease.read_reclamation_candidate().unwrap().unwrap();
    assert_eq!(candidate.old(), old_key);
    assert_eq!(candidate.successor(), GenerationLeaseKeyV1::from_catalog(successor));
    assert!(
        lease
            .reclaim_reclamation_candidate(candidate, successor)
            .unwrap()
            .deferred_for_reader
    );
    drop(pin);
    let outcome = lease
        .reclaim_reclamation_candidate(candidate, successor)
        .unwrap();
    assert_eq!(outcome.deleted_packs, 1);
    assert!(outcome.deleted_index);
    assert!(!outcome.deferred_for_reader);
    assert!(!old_pack_path.exists());
    assert!(!old_index_path.exists());
    assert!(!scratch.path().join("reclaim.v1").exists());
    assert!(matches!(
        registry.acquire(older_key),
        Err(crate::rspace::persistence::GenerationLeaseError::Retired(key)) if key == older_key
    ));
    assert!(
        registry
            .acquire(GenerationLeaseKeyV1::from_catalog(successor))
            .is_ok()
    );
}

#[cfg(unix)]
#[test]
fn compaction_keeps_transitive_retained_closure_through_old_pack_lease() {
    use crate::rspace::persistence::GenerationLeaseKeyV1;

    let (source, historical_report) = historically_larger_checkpoint_source_fixture();
    assert_eq!(historical_report.node_objects, 10);
    let objects = source.objects.clone();
    let scratch = test_scratch::acquire("rspace-gc-transitive-lease-");
    install_checkpoint_image(scratch.path(), &checkpoint_image_from_source(source));
    let old_pack_path = scratch.path().join(pack_file_name(PackId::GENESIS));
    let old_index_path = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST));
    let old_pack_bytes = fs::read(&old_pack_path).unwrap();
    let old_index_bytes = fs::read(&old_index_path).unwrap();

    let writer = admit(scratch.path());
    assert_admitted_objects(&writer, &objects);
    let old_key = GenerationLeaseKeyV1::from_catalog(writer.catalog());
    let registry = writer.generation_leases();
    let pin = registry.acquire(old_key).unwrap();

    let published = writer
        .compact_generation_v1(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
    let successor = published.catalog();
    assert_eq!(fs::read(&old_pack_path).unwrap(), old_pack_bytes);
    assert_eq!(fs::read(&old_index_path).unwrap(), old_index_bytes);
    assert!(scratch.path().join("reclaim.v1").exists());
    assert_admitted_objects(&published, &objects);
    drop(published);

    let reopened = admit(scratch.path());
    assert_eq!(reopened.catalog(), successor);
    assert_eq!(fs::read(&old_pack_path).unwrap(), old_pack_bytes);
    assert_eq!(fs::read(&old_index_path).unwrap(), old_index_bytes);
    assert_admitted_objects(&reopened, &objects);
    let (mut lease, admitted_source) = reopened.into_parts();
    let candidate = lease.read_reclamation_candidate().unwrap().unwrap();
    assert_eq!(candidate.old(), old_key);
    assert!(
        lease
            .reclaim_reclamation_candidate(candidate, successor)
            .unwrap()
            .deferred_for_reader
    );
    assert_eq!(fs::read(&old_pack_path).unwrap(), old_pack_bytes);
    assert_eq!(fs::read(&old_index_path).unwrap(), old_index_bytes);

    drop(pin);
    let outcome = lease
        .reclaim_reclamation_candidate(candidate, successor)
        .unwrap();
    assert_eq!(outcome.deleted_packs, 1);
    assert!(outcome.deleted_index);
    assert!(!outcome.deferred_for_reader);
    assert!(!old_pack_path.exists());
    assert!(!old_index_path.exists());
    assert!(!scratch.path().join("reclaim.v1").exists());
    drop(admitted_source);
    drop(lease);

    let reopened = admit(scratch.path());
    assert_eq!(reopened.catalog(), successor);
    assert_admitted_objects(&reopened, &objects);
}

fn generation_file_names(generations: &[u64], reclamation_record: bool) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    names.insert("catalog.v1".to_owned());
    if reclamation_record {
        names.insert("reclaim.v1".to_owned());
    }
    for &generation in generations {
        names.insert(pack_file_name(
            PackId::new(generation).expect("a generation names a nonzero pack"),
        ));
        names.insert(index_file_name(
            IndexGenerationV1::new(generation)
                .expect("a generation names a nonzero index generation"),
        ));
    }
    names
}

fn store_names(root: &Path) -> BTreeSet<String> { directory_snapshot(root).into_keys().collect() }

fn checkpoint_generation(catalog: CatalogSlotV1) -> u64 {
    let pack = catalog.pack();
    assert_eq!(
        pack.first_live_pack(),
        pack.active_pack(),
        "a checkpoint generation holds one live pack"
    );
    assert_eq!(
        pack.index_generation(),
        pack.active_pack(),
        "packs and index generations advance in lockstep"
    );
    pack.active_pack()
}

#[cfg(unix)]
#[test]
fn an_object_only_pin_blocks_no_later_generation_checkpoint() {
    use crate::rspace::persistence::{GenerationLeaseError, GenerationLeaseKeyV1};

    let scratch = trial_scratch("pending-reclaim-advance-");
    let root = scratch.path();
    install_checkpoint_image(root, &checkpoint_image_from_source(checkpoint_source_fixture()));
    let writer = admit(root);
    let old = writer.catalog();
    let old_key = GenerationLeaseKeyV1::from_catalog(old);
    let old_pack = root.join(pack_file_name(PackId::GENESIS));
    let old_index = root.join(index_file_name(IndexGenerationV1::FIRST));
    let old_pack_bytes = fs::read(&old_pack).expect("read the old pack");
    let old_index_bytes = fs::read(&old_index).expect("read the old index");
    let registry = DurableDirectoryV1::open(root)
        .expect("open the durable directory independently")
        .generation_leases();
    assert!(registry.shares_registry(&writer.generation_leases()));
    let older_key = GenerationLeaseKeyV1::new(
        old.epoch().saturating_sub(1),
        old_key.first_live_pack(),
        old_key.active_pack(),
        old_key.index_generation(),
    );
    assert_ne!(older_key, old_key);
    let pin = registry
        .acquire(older_key)
        .expect("pin the old generation under an older epoch");

    let selected = publish(writer).expect("the first checkpoint publishes under the pin");
    let selected_key = GenerationLeaseKeyV1::from_catalog(selected.catalog());
    let sidecar = root.join("reclaim.v1");
    let saved_intent = fs::read(&sidecar).expect("the first checkpoint leaves its record");
    assert_eq!(
        saved_intent,
        ReclamationCandidateV1::from_catalogs(old, selected.catalog())
            .expect("the first checkpoint supersedes the old generation")
            .encode()
    );
    assert_eq!(store_names(root), generation_file_names(&[1, 2], true));

    let advanced = publish_data_successor(selected, "reclaim-epoch-advance");
    let advanced_catalog = advanced.catalog();
    let advanced_key = GenerationLeaseKeyV1::from_catalog(advanced_catalog);
    assert!(advanced_key.epoch() > selected_key.epoch());
    assert_eq!(advanced_key.first_live_pack(), selected_key.first_live_pack());
    assert_eq!(advanced_key.active_pack(), selected_key.active_pack());
    assert_eq!(advanced_key.index_generation(), selected_key.index_generation());
    assert_eq!(fs::read(&sidecar).expect("the record survives the epoch"), saved_intent);
    drop(advanced);

    let reopened = admit(root);
    assert_eq!(reopened.catalog(), advanced_catalog);
    assert_eq!(fs::read(&sidecar).expect("the record survives the re-admission"), saved_intent);
    assert_eq!(fs::read(&old_pack).expect("the pinned pack stays"), old_pack_bytes);
    assert_eq!(fs::read(&old_index).expect("the pinned index stays"), old_index_bytes);
    assert_eq!(store_names(root), generation_file_names(&[1, 2], true));

    let second = publish(reopened).expect("the second checkpoint publishes under the pin");
    assert_eq!(checkpoint_generation(second.catalog()), 3);
    assert!(second.catalog().epoch() > advanced_catalog.epoch());
    assert_eq!(
        fs::read(&sidecar).expect("the record survives the second checkpoint"),
        saved_intent
    );
    assert_eq!(fs::read(&old_pack).expect("the pinned pack stays"), old_pack_bytes);
    assert_eq!(fs::read(&old_index).expect("the pinned index stays"), old_index_bytes);
    assert_eq!(store_names(root), generation_file_names(&[1, 3], true));

    let third = publish(second).expect("the third checkpoint publishes under the pin");
    let third_catalog = third.catalog();
    assert_eq!(checkpoint_generation(third_catalog), 4);
    assert_eq!(fs::read(&sidecar).expect("the record survives the third checkpoint"), saved_intent);
    assert_eq!(fs::read(&old_pack).expect("the pinned pack stays"), old_pack_bytes);
    assert_eq!(fs::read(&old_index).expect("the pinned index stays"), old_index_bytes);
    assert_eq!(store_names(root), generation_file_names(&[1, 4], true));
    assert!(matches!(
        registry.acquire(older_key),
        Err(GenerationLeaseError::Retired(key)) if key == older_key
    ));
    assert!(
        registry
            .acquire(GenerationLeaseKeyV1::from_catalog(third_catalog))
            .is_ok()
    );
    drop(third);

    drop(pin);
    let settled = admit(root);
    assert_eq!(settled.catalog(), third_catalog);
    assert!(!old_pack.exists());
    assert!(!old_index.exists());
    assert!(!sidecar.exists());
    assert_eq!(store_names(root), generation_file_names(&[4], false));
    let next = publish(settled).expect("a further checkpoint publishes");
    assert!(next.catalog().pack().active_pack() > third_catalog.pack().active_pack());
    assert_eq!(store_names(root), generation_file_names(&[5], false));
}

#[cfg(unix)]
#[test]
fn a_crash_at_every_checkpoint_phase_under_a_deferred_reclamation_completes_it() {
    use GenerationCheckpointPublicationOutcomeV1::{
        DefinitelyNotPublished, Indeterminate, Published,
    };

    use crate::rspace::persistence::GenerationLeaseKeyV1;

    let swap = FORMAL_PUBLICATION_PHASE_ORDER
        .iter()
        .position(|point| {
            *point == GenerationCheckpointPublicationHookPointV1::AfterCatalogSlotWrite
        })
        .expect("the formal phase order writes the catalog slot");
    let pinned_pack_name = pack_file_name(PackId::GENESIS);
    let pinned_index_name = index_file_name(IndexGenerationV1::FIRST);
    let successor_names = [
        pack_file_name(PackId::new(3).expect("the third pack id is nonzero")),
        index_file_name(IndexGenerationV1::new(3).expect("the third index generation is nonzero")),
    ];
    for (phase, point) in FORMAL_PUBLICATION_PHASE_ORDER.iter().copied().enumerate() {
        let (outcome, recovered_generation) = match phase.cmp(&swap) {
            std::cmp::Ordering::Less => (DefinitelyNotPublished, 2),
            std::cmp::Ordering::Equal => (Indeterminate, 3),
            std::cmp::Ordering::Greater => (Published, 3),
        };
        for release_before_restart in [false, true] {
            let scratch = trial_scratch("deferred-reclamation-crash-");
            let root = scratch.path();
            install_checkpoint_store(root);
            let writer = admit(root);
            let pin = writer
                .generation_leases()
                .acquire(GenerationLeaseKeyV1::from_catalog(writer.catalog()))
                .expect("pin the first generation");
            let first = publish(writer).expect("the first checkpoint publishes under the pin");
            let selected = first.catalog();
            let record =
                fs::read(root.join("reclaim.v1")).expect("the first checkpoint leaves its record");
            let pinned_pack = fs::read(root.join(&pinned_pack_name)).expect("read the pinned pack");
            let pinned_index =
                fs::read(root.join(&pinned_index_name)).expect("read the pinned index");
            assert_eq!(store_names(root), generation_file_names(&[1, 2], true), "{point:?}");

            start_generation_checkpoint_publication_trace();
            arm_generation_checkpoint_publication_failpoint(point);
            let error = publication_error(publish(first));
            assert_eq!(
                take_generation_checkpoint_publication_trace(),
                FORMAL_PUBLICATION_PHASE_ORDER[..=phase],
                "{point:?}"
            );
            assert_eq!(error.publication_outcome(), outcome, "{point:?}");
            let crashed = directory_snapshot(root);
            let orphans: Vec<String> = successor_names
                .iter()
                .filter(|name| recovered_generation == 2 && crashed.contains_key(*name))
                .cloned()
                .collect();
            let mut expected = match (release_before_restart, recovered_generation) {
                (false, generation) => generation_file_names(&[1, generation], true),
                (true, generation) => generation_file_names(&[generation], false),
            };
            expected.extend(orphans.iter().cloned());

            let pin = if release_before_restart {
                drop(pin);
                None
            } else {
                Some(pin)
            };
            let reopened = admit(root);
            assert_eq!(
                checkpoint_generation(reopened.catalog()),
                recovered_generation,
                "{point:?} release {release_before_restart}"
            );
            if recovered_generation == 2 {
                assert_eq!(reopened.catalog(), selected, "{point:?}");
            }
            let recovered = directory_snapshot(root);
            assert_eq!(
                recovered.keys().cloned().collect::<BTreeSet<_>>(),
                expected,
                "{point:?} release {release_before_restart}"
            );
            for name in &orphans {
                assert_eq!(recovered.get(name), crashed.get(name), "{point:?} orphan {name}");
            }
            let generation = recovered_generation + 1;
            match pin {
                Some(pin) => {
                    assert_eq!(recovered.get("reclaim.v1"), Some(&record), "{point:?}");
                    assert_eq!(recovered.get(&pinned_pack_name), Some(&pinned_pack), "{point:?}");
                    assert_eq!(recovered.get(&pinned_index_name), Some(&pinned_index), "{point:?}");
                    let retried = publish(reopened)
                        .expect("a checkpoint publishes after the crash while the pin is held");
                    assert_eq!(checkpoint_generation(retried.catalog()), generation, "{point:?}");
                    assert_eq!(
                        store_names(root),
                        generation_file_names(&[1, generation], true),
                        "{point:?}"
                    );
                    assert_eq!(
                        fs::read(root.join("reclaim.v1")).expect("the record survives the retry"),
                        record,
                        "{point:?}"
                    );
                    drop(retried);
                    drop(pin);
                    let settled = admit(root);
                    assert_eq!(checkpoint_generation(settled.catalog()), generation, "{point:?}");
                    assert_eq!(
                        store_names(root),
                        generation_file_names(&[generation], false),
                        "{point:?}"
                    );
                }
                None => {
                    let next = publish(reopened)
                        .expect("the next checkpoint publishes after the completed reclamation");
                    assert_eq!(checkpoint_generation(next.catalog()), generation, "{point:?}");
                    assert_eq!(
                        store_names(root),
                        generation_file_names(&[generation], false),
                        "{point:?}"
                    );
                }
            }
        }
    }

    let scratch = trial_scratch("deferred-reclamation-partial-");
    let root = scratch.path();
    install_checkpoint_store(root);
    let writer = admit(root);
    let registry = writer.generation_leases();
    let first_pin = registry
        .acquire(GenerationLeaseKeyV1::from_catalog(writer.catalog()))
        .expect("pin the first generation");
    let first = publish(writer).expect("the first checkpoint publishes under the pin");
    let record = fs::read(root.join("reclaim.v1")).expect("the first checkpoint leaves its record");
    let second_pin = registry
        .acquire(GenerationLeaseKeyV1::from_catalog(first.catalog()))
        .expect("pin the second generation");
    let second = publish(first).expect("the second checkpoint publishes under both pins");
    assert_eq!(checkpoint_generation(second.catalog()), 3);
    assert_eq!(store_names(root), generation_file_names(&[1, 2, 3], true));
    drop(second);
    fs::remove_file(
        root.join(pack_file_name(PackId::new(2).expect("the second pack id is nonzero"))),
    )
    .expect("remove the pack of the second generation and keep its index");
    drop(second_pin);
    let reopened = admit(root);
    assert_eq!(checkpoint_generation(reopened.catalog()), 3);
    assert_eq!(store_names(root), generation_file_names(&[1, 3], true));
    assert_eq!(
        fs::read(root.join("reclaim.v1")).expect("the record survives the partial state"),
        record
    );
    drop(reopened);
    drop(first_pin);
    let settled = admit(root);
    assert_eq!(checkpoint_generation(settled.catalog()), 3);
    assert_eq!(store_names(root), generation_file_names(&[3], false));
}

#[test]
fn each_superseded_generation_is_reclaimed_when_its_own_pins_release() {
    use crate::rspace::persistence::GenerationLeaseKeyV1;

    let scratch = trial_scratch("several-pins-");
    let root = scratch.path();
    install_checkpoint_store(root);
    let writer = admit(root);
    let registry = writer.generation_leases();
    let pin = |catalog: CatalogSlotV1| {
        registry
            .acquire(GenerationLeaseKeyV1::from_catalog(catalog))
            .expect("pin the current generation")
    };
    let settled = |catalog: CatalogSlotV1, superseded: &[u64], live_pins: usize| {
        let selected = checkpoint_generation(catalog);
        let mut generations = Vec::with_capacity(superseded.len() + 1);
        generations.extend_from_slice(superseded);
        generations.push(selected);
        let names = store_names(root);
        assert_eq!(
            names,
            generation_file_names(&generations, !superseded.is_empty()),
            "generation {selected} is selected"
        );
        let packs = names
            .iter()
            .filter(|name| name.starts_with("pack-"))
            .count();
        let indexes = names
            .iter()
            .filter(|name| name.starts_with("index-"))
            .count();
        assert_eq!(packs, indexes, "generation {selected} is selected");
        assert!(
            packs <= live_pins + 1,
            "{packs} generations are present under {live_pins} live pins"
        );
    };
    let record = || fs::read(root.join("reclaim.v1")).expect("a reclamation record is pending");

    let first_pin = pin(writer.catalog());
    let writer = publish(writer).expect("the checkpoint to the second generation publishes");
    settled(writer.catalog(), &[1], 1);
    let first_record = record();
    let second_pin = pin(writer.catalog());
    let writer = publish(writer).expect("the checkpoint to the third generation publishes");
    settled(writer.catalog(), &[1, 2], 2);
    let writer = publish(writer).expect("the checkpoint to the fourth generation publishes");
    settled(writer.catalog(), &[1, 2], 2);
    let writer = publish(writer).expect("the checkpoint to the fifth generation publishes");
    settled(writer.catalog(), &[1, 2], 2);
    assert_eq!(record(), first_record);

    drop(second_pin);
    let writer = publish_data_successor(writer, "several-pins-second-release");
    settled(writer.catalog(), &[1], 1);
    assert_eq!(record(), first_record);
    drop(first_pin);
    let writer = publish_data_successor(writer, "several-pins-first-release");
    settled(writer.catalog(), &[], 0);

    let fifth_pin = pin(writer.catalog());
    let writer = publish(writer).expect("the checkpoint to the sixth generation publishes");
    settled(writer.catalog(), &[5], 1);
    let second_record = record();
    assert_ne!(second_record, first_record);
    let writer = publish(writer).expect("the checkpoint to the seventh generation publishes");
    settled(writer.catalog(), &[5], 1);
    assert_eq!(record(), second_record);
    drop(fifth_pin);
    let writer = publish_data_successor(writer, "several-pins-fifth-release");
    settled(writer.catalog(), &[], 0);

    let writer = publish(writer).expect("the checkpoint to the eighth generation publishes");
    settled(writer.catalog(), &[], 0);
    let writer = publish(writer).expect("the checkpoint to the ninth generation publishes");
    settled(writer.catalog(), &[], 0);
    assert_eq!(checkpoint_generation(writer.catalog()), 9);
}

#[cfg(unix)]
#[test]
fn a_deferred_reclamation_never_deletes_the_selected_generation_or_an_unnamed_file() {
    use crate::rspace::persistence::GenerationLeaseKeyV1;

    let scratch = trial_scratch("deferred-reclamation-exact-");
    let root = scratch.path();
    install_checkpoint_store(root);
    let writer = admit(root);
    let pin = writer
        .generation_leases()
        .acquire(GenerationLeaseKeyV1::from_catalog(writer.catalog()))
        .expect("pin the first generation");
    let first = publish(writer).expect("the first checkpoint publishes under the pin");
    let second = publish(first).expect("the second checkpoint publishes under the pin");
    let selected = second.catalog();
    assert_eq!(checkpoint_generation(selected), 3);
    assert_eq!(store_names(root), generation_file_names(&[1, 3], true));
    drop(second);

    let orphan_pack = pack_file_name(PackId::new(4).expect("the fourth pack id is nonzero"));
    let orphan_index =
        index_file_name(IndexGenerationV1::new(4).expect("the fourth index generation is nonzero"));
    let rebuild_temporary = format!(
        "{}.rebuild",
        index_file_name(IndexGenerationV1::new(2).expect("the second index generation is nonzero"))
    );
    let planted: [(&str, &[u8]); 5] = [
        (orphan_pack.as_str(), b"an unpublished successor pack"),
        (orphan_index.as_str(), b"an unpublished successor index"),
        (rebuild_temporary.as_str(), b"an index rebuild temporary"),
        ("pack-2.rsp", b"a pack name that is not canonical"),
        ("operator-notes.txt", b"not a store file"),
    ];
    for (name, bytes) in planted {
        write_owner_only(&root.join(name), bytes);
    }
    let before = directory_snapshot(root);
    let mut expected_names = generation_file_names(&[1, 3, 4], true);
    expected_names.extend(
        [rebuild_temporary.as_str(), "pack-2.rsp", "operator-notes.txt"]
            .into_iter()
            .map(str::to_owned),
    );
    assert_eq!(before.keys().cloned().collect::<BTreeSet<_>>(), expected_names);

    let mut lease = DurableDirectoryV1::open(root)
        .and_then(DurableDirectoryV1::try_acquire_writer)
        .expect("take the writer lease");
    let candidate = lease
        .read_reclamation_candidate()
        .expect("read the reclamation record")
        .expect("the reclamation record is pending");
    let held = lease
        .reclaim_reclamation_candidate(candidate, selected)
        .expect("the attempt under the pin succeeds");
    assert!(held.deferred_for_reader);
    assert_eq!(held.deleted_packs, 0);
    assert!(!held.deleted_index);
    assert_eq!(directory_snapshot(root), before);

    drop(pin);
    let released = lease
        .reclaim_reclamation_candidate(candidate, selected)
        .expect("the attempt after the release succeeds");
    assert!(!released.deferred_for_reader);
    assert_eq!(released.deleted_packs, 1);
    assert!(released.deleted_index);
    let mut expected = before;
    for name in [
        "reclaim.v1".to_owned(),
        pack_file_name(PackId::GENESIS),
        index_file_name(IndexGenerationV1::FIRST),
    ] {
        assert!(expected.remove(&name).is_some(), "{name} was present before the release");
    }
    assert_eq!(directory_snapshot(root), expected);
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
enum PinHistoryStep {
    PinCurrent,
    ReleaseOne,
    Incremental,
    Checkpoint,
    ReAdmit,
    DropAllPins,
}

#[cfg(unix)]
const PIN_HISTORY_PREFIX: [PinHistoryStep; 2] =
    [PinHistoryStep::PinCurrent, PinHistoryStep::Checkpoint];

#[cfg(unix)]
const PIN_HISTORY_STEPS_WITHOUT_PINS: [PinHistoryStep; 8] = [
    PinHistoryStep::PinCurrent,
    PinHistoryStep::PinCurrent,
    PinHistoryStep::PinCurrent,
    PinHistoryStep::PinCurrent,
    PinHistoryStep::Checkpoint,
    PinHistoryStep::Checkpoint,
    PinHistoryStep::Incremental,
    PinHistoryStep::ReAdmit,
];

#[cfg(unix)]
const PIN_HISTORY_STEPS_WITH_PINS: [PinHistoryStep; 16] = [
    PinHistoryStep::PinCurrent,
    PinHistoryStep::PinCurrent,
    PinHistoryStep::ReleaseOne,
    PinHistoryStep::ReleaseOne,
    PinHistoryStep::Incremental,
    PinHistoryStep::Incremental,
    PinHistoryStep::Checkpoint,
    PinHistoryStep::Checkpoint,
    PinHistoryStep::Checkpoint,
    PinHistoryStep::Checkpoint,
    PinHistoryStep::Checkpoint,
    PinHistoryStep::Checkpoint,
    PinHistoryStep::ReAdmit,
    PinHistoryStep::ReAdmit,
    PinHistoryStep::ReAdmit,
    PinHistoryStep::DropAllPins,
];

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Default)]
struct PinHistoryCoverage {
    checkpoints_under_a_deferred_reclamation: usize,
    generations_reclaimed_after_their_release: usize,
    readmissions_under_a_deferred_reclamation: usize,
    readmissions_under_a_descendant_record: usize,
}

#[cfg(unix)]
fn reclaim_unpinned_generations<Lease>(
    superseded: &mut BTreeSet<u64>,
    pins: &[(u64, Lease)],
) -> usize {
    let before = superseded.len();
    superseded.retain(|generation| pins.iter().any(|(pinned, _)| pinned == generation));
    before - superseded.len()
}

#[cfg(unix)]
fn check_pin_history(selectors: &[(u8, u8)]) -> Result<PinHistoryCoverage, TestCaseError> {
    use crate::rspace::persistence::GenerationLeaseKeyV1;

    let scratch = trial_scratch("pin-history-");
    let root = scratch.path();
    install_checkpoint_store(root);
    let mut writer = admit(root);
    let registry = writer.generation_leases();
    let steps = PIN_HISTORY_PREFIX.len() + selectors.len();
    let mut coverage = PinHistoryCoverage::default();
    let mut pins = Vec::with_capacity(steps);
    let mut superseded = BTreeSet::new();
    let mut record_from: Option<u64> = None;
    let mut expected = generation_file_names(&[1], false);
    for position in 0..steps {
        let (step, argument) = match position.checked_sub(PIN_HISTORY_PREFIX.len()) {
            None => (PIN_HISTORY_PREFIX[position], 0),
            Some(generated) => {
                let (choice, argument) = selectors[generated];
                let table: &[PinHistoryStep] = if pins.is_empty() {
                    &PIN_HISTORY_STEPS_WITHOUT_PINS
                } else {
                    &PIN_HISTORY_STEPS_WITH_PINS
                };
                (table[usize::from(choice) % table.len()], usize::from(argument))
            }
        };
        let selected = checkpoint_generation(writer.catalog());
        let attempted = match step {
            PinHistoryStep::PinCurrent => {
                let pin = registry
                    .acquire(GenerationLeaseKeyV1::from_catalog(writer.catalog()))
                    .map_err(|error| {
                        TestCaseError::fail(format!(
                            "step {position} could not pin the current generation: {error:?}"
                        ))
                    })?;
                pins.push((selected, pin));
                false
            }
            PinHistoryStep::ReleaseOne => {
                if let Some(released) = argument.checked_rem(pins.len()) {
                    drop(pins.remove(released));
                }
                false
            }
            PinHistoryStep::DropAllPins => {
                pins.clear();
                false
            }
            PinHistoryStep::Incremental => {
                writer = publish_data_successor(writer, &format!("pin-history-{position}"));
                coverage.generations_reclaimed_after_their_release +=
                    reclaim_unpinned_generations(&mut superseded, &pins);
                true
            }
            PinHistoryStep::ReAdmit => {
                let pending = !superseded.is_empty();
                let descendant = record_from.is_some_and(|from| selected > from + 1);
                drop(writer);
                writer = admit(root);
                coverage.generations_reclaimed_after_their_release +=
                    reclaim_unpinned_generations(&mut superseded, &pins);
                coverage.readmissions_under_a_deferred_reclamation +=
                    usize::from(pending && !superseded.is_empty());
                coverage.readmissions_under_a_descendant_record +=
                    usize::from(pending && descendant);
                true
            }
            PinHistoryStep::Checkpoint => {
                coverage.generations_reclaimed_after_their_release +=
                    reclaim_unpinned_generations(&mut superseded, &pins);
                if superseded.is_empty() {
                    record_from = Some(selected);
                } else {
                    coverage.checkpoints_under_a_deferred_reclamation += 1;
                }
                writer = publish(writer).map_err(|error| {
                    TestCaseError::fail(format!("step {position} refused a checkpoint: {error:?}"))
                })?;
                prop_assert_eq!(checkpoint_generation(writer.catalog()), selected + 1);
                superseded.insert(selected);
                reclaim_unpinned_generations(&mut superseded, &pins);
                true
            }
        };
        if attempted {
            if superseded.is_empty() {
                record_from = None;
            }
            let mut generations = Vec::with_capacity(superseded.len() + 1);
            generations.extend(superseded.iter().copied());
            generations.push(checkpoint_generation(writer.catalog()));
            expected = generation_file_names(&generations, !superseded.is_empty());
        }
        prop_assert_eq!(
            &store_names(root),
            &expected,
            "after step {} ({:?}) with pins on {:?}",
            position,
            step,
            pins.iter().map(|(pinned, _)| *pinned).collect::<Vec<_>>()
        );
    }
    Ok(coverage)
}

#[cfg(unix)]
#[test]
fn generated_pin_histories_keep_exactly_the_pinned_generations_and_never_refuse_a_checkpoint() {
    let coverage = std::cell::RefCell::new(PinHistoryCoverage::default());
    let mut runner = TestRunner::new_with_rng(
        ProptestConfig {
            cases: 16,
            failure_persistence: None,
            max_shrink_iters: 32,
            ..ProptestConfig::default()
        },
        proptest::test_runner::TestRng::deterministic_rng(
            proptest::test_runner::RngAlgorithm::ChaCha,
        ),
    );
    runner
        .run(&prop::collection::vec((any::<u8>(), any::<u8>()), 6), |selectors| {
            let observed =
                super::trusted_history_production::isolated(move || check_pin_history(&selectors))?;
            let mut total = coverage.borrow_mut();
            total.checkpoints_under_a_deferred_reclamation +=
                observed.checkpoints_under_a_deferred_reclamation;
            total.generations_reclaimed_after_their_release +=
                observed.generations_reclaimed_after_their_release;
            total.readmissions_under_a_deferred_reclamation +=
                observed.readmissions_under_a_deferred_reclamation;
            total.readmissions_under_a_descendant_record +=
                observed.readmissions_under_a_descendant_record;
            Ok(())
        })
        .expect("every generated pin history keeps exactly the pinned generations");
    let coverage = coverage.into_inner();
    assert!(
        coverage.checkpoints_under_a_deferred_reclamation > 0 &&
            coverage.generations_reclaimed_after_their_release > 0 &&
            coverage.readmissions_under_a_deferred_reclamation > 0 &&
            coverage.readmissions_under_a_descendant_record > 0,
        "the generated pin histories missed a path: {coverage:?}"
    );
}

#[test]
fn pending_reclamation_retries_partial_deletion_after_restart() {
    use crate::rspace::persistence::GenerationLeaseKeyV1;

    let scratch = test_scratch::acquire("rspace-gc-pending-reclaim-partial-");
    install_checkpoint_store(scratch.path());
    let writer = admit(scratch.path());
    let registry = writer.generation_leases();
    let pin = registry
        .acquire(GenerationLeaseKeyV1::from_catalog(writer.catalog()))
        .unwrap();
    drop(publish(writer).unwrap());
    let old_pack = scratch.path().join(pack_file_name(PackId::GENESIS));
    let old_index = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST));
    let sidecar = scratch.path().join("reclaim.v1");
    assert!(sidecar.exists());
    drop(pin);
    fs::remove_file(&old_pack).unwrap();

    admit(scratch.path());
    assert!(!old_pack.exists());
    assert!(!old_index.exists());
    assert!(!sidecar.exists());
}

#[test]
fn pending_reclamation_waits_for_selected_successor_verification() {
    use crate::rspace::persistence::GenerationLeaseKeyV1;

    let scratch = test_scratch::acquire("rspace-gc-pending-reclaim-verify-");
    install_checkpoint_store(scratch.path());
    let writer = admit(scratch.path());
    let registry = writer.generation_leases();
    let pin = registry
        .acquire(GenerationLeaseKeyV1::from_catalog(writer.catalog()))
        .unwrap();
    let selected = publish(writer).unwrap();
    let selected_pack = scratch
        .path()
        .join(pack_file_name(PackId::new(selected.catalog().pack().active_pack()).unwrap()));
    let valid_bytes = fs::read(&selected_pack).unwrap();
    let mut damaged = valid_bytes.clone();
    damaged[PACK_HEADER_LEN_V1 as usize + 8] ^= 0x80;
    drop(selected);
    drop(pin);
    rewrite_and_sync(&selected_pack, &damaged);

    let old_pack = scratch.path().join(pack_file_name(PackId::GENESIS));
    let old_index = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST));
    let sidecar = scratch.path().join("reclaim.v1");
    let error = match DurableDirectoryV1::open(scratch.path())
        .unwrap()
        .recover_writer_v1(limits(), LogPackDecodeLimits::UNBOUNDED)
    {
        Err(error) => error,
        Ok(_) => panic!("damaged selected successor was recovered"),
    };
    assert!(matches!(error, DurableStartupErrorV1::Recovery(_)));
    assert!(old_pack.exists());
    assert!(old_index.exists());
    assert!(sidecar.exists());

    rewrite_and_sync(&selected_pack, &valid_bytes);
    admit(scratch.path());
    assert!(!old_pack.exists());
    assert!(!old_index.exists());
    assert!(!sidecar.exists());
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IntentTear {
    Create,
    SetLength,
    Prefix(usize),
    Sync,
    BeforeRename,
    DirectorySync { lost: bool },
}

#[cfg(unix)]
impl IntentTear {
    fn arm(self) {
        match self {
            Self::Create => arm_durable_io_failpoint(DurableIoFailpointV1::ReclamationIntentCreate),
            Self::SetLength => {
                arm_durable_io_failpoint(DurableIoFailpointV1::ReclamationIntentSetLength)
            }
            Self::Prefix(prefix_len) => arm_reclamation_intent_write_prefix(prefix_len),
            Self::Sync => arm_durable_io_failpoint(DurableIoFailpointV1::ReclamationIntentSync),
            Self::BeforeRename => {
                arm_durable_io_failpoint(DurableIoFailpointV1::ReclamationIntentBeforeRename)
            }
            Self::DirectorySync { .. } => {
                arm_durable_io_failpoint(DurableIoFailpointV1::ReclamationIntentDirectorySync)
            }
        }
    }

    fn expected_names(
        self,
        record: &[u8; RECLAMATION_RECORD_LEN_V1],
    ) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        match self {
            Self::Create => (None, None),
            Self::SetLength => (None, Some(Vec::new())),
            Self::Prefix(prefix_len) => {
                let mut torn = vec![0; RECLAMATION_RECORD_LEN_V1];
                torn[..prefix_len].copy_from_slice(&record[..prefix_len]);
                (None, Some(torn))
            }
            Self::Sync | Self::BeforeRename | Self::DirectorySync { lost: true } => {
                (None, Some(record.to_vec()))
            }
            Self::DirectorySync { lost: false } => (Some(record.to_vec()), None),
        }
    }
}

#[cfg(unix)]
fn every_intent_tear() -> Vec<IntentTear> {
    let mut tears = Vec::with_capacity(RECLAMATION_RECORD_LEN_V1 + 6);
    tears.extend([IntentTear::Create, IntentTear::SetLength]);
    tears.extend((0..RECLAMATION_RECORD_LEN_V1).map(IntentTear::Prefix));
    tears.extend([
        IntentTear::Sync,
        IntentTear::BeforeRename,
        IntentTear::DirectorySync { lost: false },
        IntentTear::DirectorySync { lost: true },
    ]);
    tears
}

#[cfg(unix)]
fn check_reclamation_intent_tears(
    root: &Path,
    tears: &[IntentTear],
    pinned: bool,
) -> Result<(), TestCaseError> {
    use crate::rspace::persistence::{GenerationLeaseError, GenerationLeaseKeyV1};

    install_checkpoint_store(root);
    let mut writer = admit(root);
    let old_catalog = *writer.authority().image();
    let old_slot = writer.authority().slot();
    let old_key = GenerationLeaseKeyV1::from_catalog(old_slot);
    let expected = expected_successor(writer.authority());
    let record = ReclamationCandidateV1::from_catalogs(old_slot, expected.slot)
        .expect("the successor supersedes its predecessor")
        .encode();
    let registry = writer.generation_leases();
    let pin = pinned.then(|| {
        registry
            .acquire(old_key)
            .expect("pin the predecessor generation")
    });
    let final_name = root.join("reclaim.v1");
    let temporary_name = root.join("reclaim.v1.tmp");
    let old_pack = root.join(pack_file_name(PackId::GENESIS));
    let old_index = root.join(index_file_name(IndexGenerationV1::FIRST));

    for &tear in tears {
        tear.arm();
        let error = match publish(writer) {
            Err(error) => error,
            Ok(_) => return Err(TestCaseError::fail(format!("tear {tear:?} published"))),
        };
        prop_assert_eq!(
            error.publication_outcome(),
            GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished,
            "tear {:?}",
            tear
        );
        prop_assert!(
            matches!(error, GenerationCheckpointPublicationErrorV1::Io(DurableIoError::Io { .. })),
            "tear {:?} failed otherwise: {:?}",
            tear,
            error
        );
        prop_assert_eq!(
            fs::read(root.join("catalog.v1")).expect("read the catalog"),
            old_catalog,
            "tear {:?}",
            tear
        );
        if tear == (IntentTear::DirectorySync { lost: true }) {
            fs::rename(&final_name, &temporary_name).expect("undo the unsynchronized rename");
        }
        let snapshot = directory_snapshot(root);
        writer = DurableDirectoryV1::open(root)
            .expect("open the durable directory")
            .recover_and_admit_v1::<TestSemanticProfile>(
                limits(),
                LogPackDecodeLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
            .map_err(|error| {
                TestCaseError::fail(format!("tear {tear:?} blocked cold admission: {error:?}"))
            })?;
        prop_assert_eq!(writer.authority().image(), &old_catalog, "tear {:?}", tear);
        prop_assert!(
            !final_name.exists() && !temporary_name.exists(),
            "tear {:?} left a reclamation name after recovery",
            tear
        );
        drop(registry.acquire(old_key).map_err(|error| {
            TestCaseError::fail(format!("tear {tear:?} retired the predecessor: {error:?}"))
        })?);
        let (expected_final, expected_temporary) = tear.expected_names(&record);
        prop_assert_eq!(snapshot.get("reclaim.v1"), expected_final.as_ref(), "tear {:?}", tear);
        prop_assert_eq!(
            snapshot.get("reclaim.v1.tmp"),
            expected_temporary.as_ref(),
            "tear {:?}",
            tear
        );
    }

    let retried = publish(writer)
        .map_err(|error| TestCaseError::fail(format!("the retry failed: {error:?}")))?;
    prop_assert_eq!(retried.authority().image(), &expected.catalog);
    let successor_pack = root.join(pack_file_name(
        PackId::new(expected.slot.pack().active_pack()).expect("the successor pack is nonzero"),
    ));
    let successor_index = root.join(index_file_name(
        IndexGenerationV1::new(expected.slot.pack().index_generation())
            .expect("the successor index generation is nonzero"),
    ));
    prop_assert_eq!(fs::read(successor_pack).expect("read the successor pack"), expected.pack);
    prop_assert_eq!(fs::read(successor_index).expect("read the successor index"), expected.index);
    drop(retried);
    if let Some(pin) = pin {
        prop_assert_eq!(
            fs::read(&final_name).expect("read the deferred reclamation intent"),
            record.to_vec()
        );
        prop_assert!(old_pack.exists() && old_index.exists());
        drop(pin);
        drop(admit(root));
    }
    prop_assert!(!old_pack.exists() && !old_index.exists());
    prop_assert!(!final_name.exists() && !temporary_name.exists());
    prop_assert!(matches!(
        registry.acquire(old_key),
        Err(GenerationLeaseError::Retired(key)) if key == old_key
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn every_reclamation_intent_tear_recovers_pre_cas_authority_and_retries() {
    let tears = every_intent_tear();
    let mut failures = Vec::with_capacity(2 * tears.len());
    let mut cases = 0_usize;
    for pinned in [false, true] {
        for &tear in &tears {
            cases += 1;
            let outcome = super::trusted_history_production::isolated(move || {
                let trial = trial_scratch("reclaim-intent-tear-");
                check_reclamation_intent_tears(trial.path(), &[tear], pinned)
            });
            if let Err(error) = outcome {
                failures.push(format!("{tear:?} pinned={pinned}: {error}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {cases} reclamation-intent tears failed; the first three: {:#?}",
        failures.len(),
        &failures[..failures.len().min(3)]
    );
}

#[cfg(unix)]
#[test]
fn generated_reclamation_intent_tears_never_publish_a_torn_record() {
    let strategy =
        (prop::collection::vec(prop::sample::select(every_intent_tear()), 1..=3), any::<bool>());
    runner(32)
        .run(&strategy, |(tears, pinned)| {
            super::trusted_history_production::isolated(move || {
                let trial = trial_scratch("reclaim-intent-tears-");
                check_reclamation_intent_tears(trial.path(), &tears, pinned)
            })
        })
        .expect("every generated sequence of interrupted intents recovers and retries");
}

#[cfg(unix)]
#[test]
fn interrupted_reclamation_intent_write_recovers_pre_cas_authority() {
    use std::os::unix::fs::MetadataExt;

    use crate::rspace::persistence::durable::recover_physical_from_lease_v1;

    let scratch = test_scratch::acquire("rspace-reclaim-intent-interrupted-");
    install_checkpoint_store(scratch.path());
    let writer = admit(scratch.path());
    let old_catalog = *writer.authority().image();
    let expected = expected_successor(writer.authority());
    let successor_pack = pack_file_name(
        PackId::GENESIS
            .successor()
            .expect("the genesis pack has a successor"),
    );
    let successor_index = index_file_name(
        IndexGenerationV1::FIRST
            .successor()
            .expect("the first index generation has a successor"),
    );

    arm_generation_checkpoint_publication_hook(
        GenerationCheckpointPublicationHookPointV1::AfterCommitSync,
        || arm_durable_io_failpoint(DurableIoFailpointV1::PhysicalWrite),
    );
    let error = publication_error(publish(writer));
    assert_eq!(
        error.publication_outcome(),
        GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished
    );
    assert!(
        matches!(error, GenerationCheckpointPublicationErrorV1::Io(DurableIoError::Io { .. })),
        "the interrupted intent write failed otherwise: {error:?}"
    );
    let interrupted = directory_snapshot(scratch.path());
    let held_successor = fs::File::open(scratch.path().join(&successor_pack))
        .expect("hold the unpublished successor pack");

    let mut lease = DurableDirectoryV1::open(scratch.path())
        .expect("open the durable directory")
        .try_acquire_writer()
        .expect("acquire the writer lease");
    let recovered = match recover_physical_from_lease_v1(
        &mut lease,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
    ) {
        Ok(recovered) => recovered,
        Err(error) => {
            panic!("an interrupted reclamation-intent write blocked held-lease recovery: {error:?}")
        }
    };
    assert_eq!(recovered.catalog_authority().image(), &old_catalog);
    assert_eq!(
        interrupted.get("reclaim.v1.tmp"),
        Some(&vec![0; RECLAMATION_RECORD_LEN_V1]),
        "the interrupted write left its zero-filled length under the temporary name only"
    );
    assert!(!interrupted.contains_key("reclaim.v1"));
    assert_eq!(interrupted.get("catalog.v1"), Some(&old_catalog.to_vec()));
    assert_eq!(interrupted.get(successor_pack.as_str()), Some(&expected.pack));
    assert_eq!(interrupted.get(successor_index.as_str()), Some(&expected.index));
    assert!(!scratch.path().join("reclaim.v1").exists());
    assert!(!scratch.path().join("reclaim.v1.tmp").exists());
    drop(recovered);
    drop(lease);

    let retried = publish(admit(scratch.path())).expect("the retry publishes the successor");
    assert_eq!(retried.authority().image(), &expected.catalog);
    assert_eq!(
        fs::read(scratch.path().join(&successor_pack)).expect("read the successor pack"),
        expected.pack
    );
    assert_eq!(
        fs::read(scratch.path().join(&successor_index)).expect("read the successor index"),
        expected.index
    );
    assert_eq!(
        held_successor
            .metadata()
            .expect("read the held successor pack metadata")
            .nlink(),
        0,
        "the retry removed the unpublished successor pack before writing its own"
    );
}

#[cfg(unix)]
#[test]
fn reclamation_intent_publication_never_replaces_an_existing_record() {
    let scratch = test_scratch::acquire("rspace-reclaim-intent-no-replace-");
    install_checkpoint_store(scratch.path());
    let writer = admit(scratch.path());
    let old_catalog = *writer.authority().image();
    let expected = expected_successor(writer.authority());
    let record = scratch.path().join("reclaim.v1");
    let planted = [0x5a; RECLAMATION_RECORD_LEN_V1];
    let hook_record = record.clone();

    arm_generation_checkpoint_publication_hook(
        GenerationCheckpointPublicationHookPointV1::AfterCommitSync,
        move || write_owner_only(&hook_record, &planted),
    );
    let error = publication_error(publish(writer));
    assert_eq!(
        error.publication_outcome(),
        GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished
    );
    match &error {
        GenerationCheckpointPublicationErrorV1::Io(DurableIoError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::AlreadyExists => {}
        error => panic!("an existing reclamation record was not refused as present: {error:?}"),
    }
    assert_eq!(fs::read(&record).expect("read the planted record"), planted);
    assert_eq!(fs::read(scratch.path().join("catalog.v1")).expect("read the catalog"), old_catalog);

    let refusal = match DurableDirectoryV1::open(scratch.path())
        .expect("open the durable directory")
        .recover_and_admit_v1::<TestSemanticProfile>(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        ) {
        Err(refusal) => refusal,
        Ok(_) => panic!("cold admission resolved an undecodable reclamation record"),
    };
    match refusal {
        DurableAdmissionErrorV1::Startup(DurableStartupErrorV1::Io(DurableIoError::Io {
            source,
            ..
        })) if source.kind() == std::io::ErrorKind::InvalidData => {}
        refusal => panic!("the undecodable reclamation record was refused otherwise: {refusal:?}"),
    }
    assert_eq!(fs::read(&record).expect("read the planted record"), planted);

    fs::remove_file(&record).expect("remove the planted record");
    let writer = admit(scratch.path());
    assert!(!scratch.path().join("reclaim.v1.tmp").exists());
    let retried = publish(writer).expect("the retry publishes the successor");
    assert_eq!(retried.authority().image(), &expected.catalog);
}

#[cfg(unix)]
#[test]
fn retired_pack_frontier_survives_all_handle_drop_until_process_exit() {
    use crate::rspace::persistence::GenerationLeaseKeyV1;

    let scratch = test_scratch::acquire("rspace-gc-pack-lease-reopen-");
    install_checkpoint_store(scratch.path());
    let (old_key, successor_key) = {
        let writer = admit(scratch.path());
        let old_key = GenerationLeaseKeyV1::from_catalog(writer.catalog());
        let published = publish(writer).unwrap();
        let successor_key = GenerationLeaseKeyV1::from_catalog(published.catalog());
        (old_key, successor_key)
    };
    let reopened_registry = DurableDirectoryV1::open(scratch.path())
        .unwrap()
        .generation_leases();
    assert!(matches!(
        reopened_registry.acquire(old_key),
        Err(crate::rspace::persistence::GenerationLeaseError::Retired(key)) if key == old_key
    ));
    assert!(reopened_registry.acquire(successor_key).is_ok());
}

#[test]
fn successful_publication_executes_the_exact_formal_phase_order() {
    let scratch = test_scratch::acquire("rspace-durable-publication-phase-trace-");
    install_checkpoint_store(scratch.path());
    let writer = admit(scratch.path());

    start_generation_checkpoint_publication_trace();
    let published = publish(writer).unwrap();
    let trace = take_generation_checkpoint_publication_trace();

    assert_eq!(trace, FORMAL_PUBLICATION_PHASE_ORDER);
    assert_eq!(published.authority().slot().pack().active_pack(), 2);
}

#[test]
fn authorized_partial_successors_are_replaced_by_the_exact_generation_on_retry() {
    let scratch = test_scratch::acquire("rspace-durable-publication-retry-");
    install_checkpoint_store(scratch.path());
    let writer = admit(scratch.path());
    let expected = expected_successor(writer.authority());
    let pack_path = scratch
        .path()
        .join(pack_file_name(PackId::GENESIS.successor().unwrap()));
    let index_path = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST.successor().unwrap()));
    write_owner_only(&pack_path, &[0x31; 17]);
    write_owner_only(&index_path, &[0x42; 19]);

    let published = publish(writer).unwrap();
    assert_eq!(fs::read(pack_path).unwrap(), expected.pack);
    assert_eq!(fs::read(index_path).unwrap(), expected.index);
    assert_eq!(published.authority().image(), &expected.catalog);
}

#[test]
fn every_pre_catalog_crash_shape_recovers_old_authority_and_retries_exactly() {
    let phases = [
        GenerationCheckpointPublicationHookPointV1::AfterPrepareSync,
        GenerationCheckpointPublicationHookPointV1::AfterIndexSync,
        GenerationCheckpointPublicationHookPointV1::AfterCommitSync,
        GenerationCheckpointPublicationHookPointV1::AfterArtifactDirectorySync,
    ];
    for phase in phases {
        let scratch = test_scratch::acquire("rspace-durable-publication-pre-catalog-");
        install_checkpoint_store(scratch.path());
        let old_pack_path = scratch.path().join(pack_file_name(PackId::GENESIS));
        let old_index_path = scratch
            .path()
            .join(index_file_name(IndexGenerationV1::FIRST));
        let old_pack = fs::read(&old_pack_path).unwrap();
        let old_index = fs::read(&old_index_path).unwrap();
        let writer = admit(scratch.path());
        let old_catalog = *writer.authority().image();
        let expected = expected_successor(writer.authority());

        arm_generation_checkpoint_publication_failpoint(phase);
        let error = publication_error(publish(writer));
        assert_eq!(
            error.publication_outcome(),
            GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished,
            "phase {phase:?}"
        );
        assert!(matches!(error, GenerationCheckpointPublicationErrorV1::Io(_)));
        assert_eq!(fs::read(scratch.path().join("catalog.v1")).unwrap(), old_catalog);
        assert_eq!(fs::read(&old_pack_path).unwrap(), old_pack);
        assert_eq!(fs::read(&old_index_path).unwrap(), old_index);

        let successor_pack_path = scratch
            .path()
            .join(pack_file_name(PackId::GENESIS.successor().unwrap()));
        let successor_index_path = scratch
            .path()
            .join(index_file_name(IndexGenerationV1::FIRST.successor().unwrap()));
        let expected_prepare_tail =
            expected.pack.len() as u64 - GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1;
        match phase {
            GenerationCheckpointPublicationHookPointV1::AfterPrepareSync => {
                assert_eq!(
                    fs::metadata(&successor_pack_path).unwrap().len(),
                    expected_prepare_tail
                );
                assert!(!successor_index_path.exists());
            }
            GenerationCheckpointPublicationHookPointV1::AfterIndexSync => {
                assert_eq!(
                    fs::metadata(&successor_pack_path).unwrap().len(),
                    expected_prepare_tail
                );
                assert_eq!(fs::read(&successor_index_path).unwrap(), expected.index);
            }
            GenerationCheckpointPublicationHookPointV1::AfterCommitSync |
            GenerationCheckpointPublicationHookPointV1::AfterArtifactDirectorySync => {
                assert_eq!(fs::read(&successor_pack_path).unwrap(), expected.pack);
                assert_eq!(fs::read(&successor_index_path).unwrap(), expected.index);
            }
            _ => unreachable!("the table contains only pre-catalog phases"),
        }

        let recovered_old = admit(scratch.path());
        assert_eq!(recovered_old.authority().image(), &old_catalog);
        let retried = publish(recovered_old).unwrap();
        assert_eq!(retried.authority().image(), &expected.catalog);
        assert_eq!(fs::read(successor_pack_path).unwrap(), expected.pack);
        assert_eq!(fs::read(successor_index_path).unwrap(), expected.index);
    }
}

#[cfg(unix)]
#[test]
fn unsafe_successor_collision_shapes_fail_closed_without_catalog_mutation() {
    use std::ffi::CString;
    use std::os::unix::fs::symlink;

    let symlink_case = test_scratch::acquire("rspace-durable-publication-symlink-");
    install_checkpoint_store(symlink_case.path());
    let symlink_writer = admit(symlink_case.path());
    let symlink_catalog = *symlink_writer.authority().image();
    let outside = symlink_case.path().join("outside");
    write_owner_only(&outside, b"must remain untouched");
    let successor_pack = symlink_case
        .path()
        .join(pack_file_name(PackId::GENESIS.successor().unwrap()));
    symlink(&outside, &successor_pack).unwrap();
    let error = publication_error(publish(symlink_writer));
    assert!(matches!(error, GenerationCheckpointPublicationErrorV1::Io(_)));
    assert_eq!(fs::read(&outside).unwrap(), b"must remain untouched");
    assert!(
        fs::symlink_metadata(&successor_pack)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(symlink_case.path().join("catalog.v1")).unwrap(), symlink_catalog);

    let fifo_case = test_scratch::acquire("rspace-durable-publication-fifo-");
    install_checkpoint_store(fifo_case.path());
    let fifo_writer = admit(fifo_case.path());
    let fifo_catalog = *fifo_writer.authority().image();
    let successor_index = fifo_case
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST.successor().unwrap()));
    let fifo_c = CString::new(successor_index.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);
    let error = publication_error(publish(fifo_writer));
    assert!(matches!(error, GenerationCheckpointPublicationErrorV1::Io(_)));
    assert_eq!(fs::read(fifo_case.path().join("catalog.v1")).unwrap(), fifo_catalog);

    let hardlink_case = test_scratch::acquire("rspace-durable-publication-hardlink-");
    install_checkpoint_store(hardlink_case.path());
    let hardlink_writer = admit(hardlink_case.path());
    let hardlink_catalog = *hardlink_writer.authority().image();
    let hardlink_source = hardlink_case.path().join("hardlink-source");
    write_owner_only(&hardlink_source, b"linked");
    let successor_pack = hardlink_case
        .path()
        .join(pack_file_name(PackId::GENESIS.successor().unwrap()));
    fs::hard_link(&hardlink_source, &successor_pack).unwrap();
    let error = publication_error(publish(hardlink_writer));
    assert!(matches!(error, GenerationCheckpointPublicationErrorV1::Io(_)));
    assert_eq!(fs::read(&hardlink_source).unwrap(), b"linked");
    assert_eq!(fs::read(hardlink_case.path().join("catalog.v1")).unwrap(), hardlink_catalog);

    let mode_case = test_scratch::acquire("rspace-durable-publication-mode-");
    install_checkpoint_store(mode_case.path());
    let mode_writer = admit(mode_case.path());
    let mode_catalog = *mode_writer.authority().image();
    let successor_index = mode_case
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST.successor().unwrap()));
    write_owner_only(&successor_index, b"unsafe mode");
    fs::set_permissions(&successor_index, fs::Permissions::from_mode(0o640)).unwrap();
    let error = publication_error(publish(mode_writer));
    assert!(matches!(error, GenerationCheckpointPublicationErrorV1::Io(_)));
    assert_eq!(fs::read(mode_case.path().join("catalog.v1")).unwrap(), mode_catalog);
}

#[derive(Clone, Copy, Debug)]
enum CheckpointWalBoundary {
    RecordBytes,
    MutationBytes,
    Objects,
    ObjectBytes,
    SingleObjectBytes,
    MutationEntries,
    MutationKeyBytes,
}

impl CheckpointWalBoundary {
    const fn resource_name(self) -> &'static str {
        match self {
            Self::RecordBytes => "record bytes",
            Self::MutationBytes => "mutation bytes",
            Self::Objects => "objects",
            Self::ObjectBytes => "canonical object bytes",
            Self::SingleObjectBytes => "single canonical object bytes",
            Self::MutationEntries => "mutation entries",
            Self::MutationKeyBytes => "mutation key bytes",
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum AdmissionBoundary {
    PerStateDistinctObjects,
    PerStateCanonicalBytes,
    PerStateReferences,
    PerStateWorklist,
    RetainedStates,
    UnionObjects,
    UnionCanonicalBytes,
    AggregateRootVisits,
    AggregateReferences,
    ObjectReads,
    ObjectReadBytes,
    LogicalNodes,
    Endpoints,
    LogicalPathBytes,
    PayloadBytes,
    JoinLookups,
    JoinLookupSteps,
    SemanticWorklist,
}

impl AdmissionBoundary {
    const fn kind(self) -> DurableAdmissionLimitKindV1 {
        match self {
            Self::PerStateDistinctObjects => DurableAdmissionLimitKindV1::PerStateDistinctObjects,
            Self::PerStateCanonicalBytes => DurableAdmissionLimitKindV1::PerStateCanonicalBytes,
            Self::PerStateReferences => DurableAdmissionLimitKindV1::PerStateReferences,
            Self::PerStateWorklist => DurableAdmissionLimitKindV1::PerStateWorklist,
            Self::RetainedStates => DurableAdmissionLimitKindV1::RetainedStates,
            Self::UnionObjects => DurableAdmissionLimitKindV1::UnionObjects,
            Self::UnionCanonicalBytes => DurableAdmissionLimitKindV1::UnionCanonicalBytes,
            Self::AggregateRootVisits => DurableAdmissionLimitKindV1::AggregateRootVisits,
            Self::AggregateReferences => DurableAdmissionLimitKindV1::AggregateReferences,
            Self::ObjectReads => DurableAdmissionLimitKindV1::ObjectReads,
            Self::ObjectReadBytes => DurableAdmissionLimitKindV1::ObjectReadBytes,
            Self::LogicalNodes => DurableAdmissionLimitKindV1::LogicalNodes,
            Self::Endpoints => DurableAdmissionLimitKindV1::Endpoints,
            Self::LogicalPathBytes => DurableAdmissionLimitKindV1::LogicalPathBytes,
            Self::PayloadBytes => DurableAdmissionLimitKindV1::PayloadBytes,
            Self::JoinLookups => DurableAdmissionLimitKindV1::JoinLookups,
            Self::JoinLookupSteps => DurableAdmissionLimitKindV1::JoinLookupSteps,
            Self::SemanticWorklist => DurableAdmissionLimitKindV1::SemanticWorklist,
        }
    }

    /// The resource name cold admission reports when this axis refuses.
    const fn runtime_name(self) -> &'static str {
        match self {
            Self::PerStateDistinctObjects => "distinct objects",
            Self::PerStateCanonicalBytes => "canonical bytes",
            Self::PerStateReferences => "object references",
            Self::PerStateWorklist => "closure worklist",
            Self::RetainedStates => "retained states",
            Self::UnionObjects => "retained union objects",
            Self::UnionCanonicalBytes => "retained union canonical bytes",
            Self::AggregateRootVisits => "aggregate retained-root visits",
            Self::AggregateReferences => "aggregate retained references",
            Self::ObjectReads => "object reads",
            Self::ObjectReadBytes => "object-read bytes",
            Self::LogicalNodes => "logical nodes",
            Self::Endpoints => "semantic endpoints",
            Self::LogicalPathBytes => "logical path bytes",
            Self::PayloadBytes => "continuation source payload bytes",
            Self::JoinLookups => "join lookups",
            Self::JoinLookupSteps => "join lookup steps",
            Self::SemanticWorklist => "semantic worklist",
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum CheckpointResourceBoundary {
    Durable(DurableLimitKind),
    Wal(CheckpointWalBoundary),
    Admission(AdmissionBoundary),
}

const CHECKPOINT_RESOURCE_BOUNDARIES: [CheckpointResourceBoundary; 35] = [
    CheckpointResourceBoundary::Durable(DurableLimitKind::LivePacks),
    CheckpointResourceBoundary::Durable(DurableLimitKind::PackBytes),
    CheckpointResourceBoundary::Durable(DurableLimitKind::Transactions),
    CheckpointResourceBoundary::Durable(DurableLimitKind::ObjectRecords),
    CheckpointResourceBoundary::Durable(DurableLimitKind::TotalScanBytes),
    CheckpointResourceBoundary::Durable(DurableLimitKind::TotalPhysicalReadBytes),
    CheckpointResourceBoundary::Durable(DurableLimitKind::IndexBytes),
    CheckpointResourceBoundary::Durable(DurableLimitKind::IndexRecords),
    CheckpointResourceBoundary::Durable(DurableLimitKind::FullByteComparisonBytes),
    CheckpointResourceBoundary::Durable(DurableLimitKind::AggregateFullByteComparisonBytes),
    CheckpointResourceBoundary::Wal(CheckpointWalBoundary::RecordBytes),
    CheckpointResourceBoundary::Wal(CheckpointWalBoundary::MutationBytes),
    CheckpointResourceBoundary::Wal(CheckpointWalBoundary::Objects),
    CheckpointResourceBoundary::Wal(CheckpointWalBoundary::ObjectBytes),
    CheckpointResourceBoundary::Wal(CheckpointWalBoundary::SingleObjectBytes),
    CheckpointResourceBoundary::Wal(CheckpointWalBoundary::MutationEntries),
    CheckpointResourceBoundary::Wal(CheckpointWalBoundary::MutationKeyBytes),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::PerStateDistinctObjects),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::PerStateCanonicalBytes),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::PerStateReferences),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::PerStateWorklist),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::RetainedStates),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::UnionObjects),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::UnionCanonicalBytes),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::AggregateRootVisits),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::AggregateReferences),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::ObjectReads),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::ObjectReadBytes),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::LogicalNodes),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::Endpoints),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::LogicalPathBytes),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::PayloadBytes),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::JoinLookups),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::JoinLookupSteps),
    CheckpointResourceBoundary::Admission(AdmissionBoundary::SemanticWorklist),
];

#[derive(Clone, Copy)]
struct ExactCheckpointPublicationLimits {
    durable: DurableLimitConfig,
    wal: LogPackDecodeLimits,
    admission: DurableAdmissionLimitsV1,
}

impl ExactCheckpointPublicationLimits {
    fn derive(
        source: &SyntheticCheckpointSource,
        expected: &ExpectedSuccessor,
        report: DurableAdmissionReportV1,
    ) -> Self {
        Self::exact(
            source,
            expected.pack.len() as u64,
            expected.index.len() as u64,
            expected.pack.len() as u64,
            report,
        )
    }

    /// The exact limits for scanning one checkpoint pack of `pack_bytes`
    /// whose index is `index_bytes` long, reading `physical_read_bytes`, and
    /// admitting `source` with the demand recorded in `report`.
    fn exact(
        source: &SyntheticCheckpointSource,
        pack_bytes: u64,
        index_bytes: u64,
        physical_read_bytes: u64,
        report: DurableAdmissionReportV1,
    ) -> Self {
        let inventory = source.descriptor.inventory();
        let objects = inventory.data_objects();
        let object_bytes = inventory.data_canonical_bytes().get();
        let single_object_bytes = source
            .objects
            .iter()
            .map(|(reference, _)| reference.encoded_len())
            .max()
            .expect("a generation checkpoint contains its retention manifest");
        let record_bytes = pack_bytes
            .checked_sub(PACK_HEADER_LEN_V1)
            .and_then(|bytes| bytes.checked_sub(GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1))
            .expect("the exact pack contains one header, PREPARE, and COMMIT");
        Self {
            durable: DurableLimitConfig {
                max_live_packs: 1,
                max_pack_bytes: pack_bytes,
                max_transactions: 1,
                max_object_records: objects,
                max_total_scan_bytes: pack_bytes,
                max_total_physical_read_bytes: physical_read_bytes,
                max_index_bytes: index_bytes,
                max_index_records: objects,
                max_full_byte_comparison_bytes: 1,
                max_aggregate_full_byte_comparison_bytes: 1,
            },
            wal: LogPackDecodeLimits {
                max_record_bytes: record_bytes,
                max_mutation_bytes: 0,
                max_objects: objects,
                max_object_bytes: object_bytes,
                max_single_object_bytes: single_object_bytes,
                max_mutation_entries: 0,
                max_mutation_key_bytes: 0,
            },
            admission: DurableAdmissionLimitsV1 {
                // Structural admission has already produced the non-forgeable
                // token consumed by semantic completion; these nested fields
                // are intentionally inapplicable to the post-structural pass.
                current_semantics: SemanticValidationLimits {
                    structural: StructuralValidationLimits {
                        max_distinct_objects: 0,
                        max_canonical_bytes: 0,
                        max_references: 0,
                        max_worklist: 0,
                    },
                    max_logical_nodes: report.current_semantics.logical_nodes,
                    max_endpoints: report.current_semantics.endpoints,
                    max_logical_path_bytes: report.current_semantics.maximum_logical_path_bytes,
                    max_payload_bytes: report.current_semantics.source_payload_bytes,
                    max_join_lookups: report.current_semantics.join_lookups,
                    max_join_lookup_steps: report.current_semantics.join_lookup_steps,
                    max_worklist: report.current_semantics.maximum_worklist,
                },
                per_state_structural: StructuralValidationLimits {
                    max_distinct_objects: report.current_structural.distinct_objects,
                    max_canonical_bytes: report.current_structural.canonical_bytes,
                    max_references: report.current_structural.references,
                    max_worklist: report.current_structural.maximum_worklist,
                },
                max_retained_states: report.retained_states,
                max_union_objects: report.retained_union.distinct_objects,
                max_union_canonical_bytes: report.retained_union.canonical_bytes,
                max_aggregate_root_visits: report.aggregate_root_visits,
                max_aggregate_references: report.aggregate_references,
                max_object_reads: report.object_reads,
                max_object_read_bytes: report.object_read_bytes,
            },
        }
    }

    fn build(self) -> (DurableLimits, LogPackDecodeLimits, DurableAdmissionLimitsV1) {
        (DurableLimits::new(self.durable).unwrap(), self.wal, self.admission)
    }

    const fn requirement(self, boundary: CheckpointResourceBoundary) -> u64 {
        match boundary {
            CheckpointResourceBoundary::Durable(resource) => match resource {
                DurableLimitKind::LivePacks => 1,
                DurableLimitKind::PackBytes => self.durable.max_pack_bytes,
                DurableLimitKind::Transactions => 0,
                DurableLimitKind::ObjectRecords => self.durable.max_object_records,
                DurableLimitKind::TotalScanBytes => self.durable.max_total_scan_bytes,
                DurableLimitKind::TotalPhysicalReadBytes => {
                    self.durable.max_total_physical_read_bytes
                }
                DurableLimitKind::IndexBytes => self.durable.max_index_bytes,
                DurableLimitKind::IndexRecords => self.durable.max_index_records,
                DurableLimitKind::FullByteComparisonBytes |
                DurableLimitKind::AggregateFullByteComparisonBytes => 0,
            },
            CheckpointResourceBoundary::Wal(resource) => match resource {
                CheckpointWalBoundary::RecordBytes => self.wal.max_record_bytes,
                CheckpointWalBoundary::MutationBytes => self.wal.max_mutation_bytes,
                CheckpointWalBoundary::Objects => self.wal.max_objects,
                CheckpointWalBoundary::ObjectBytes => self.wal.max_object_bytes,
                CheckpointWalBoundary::SingleObjectBytes => self.wal.max_single_object_bytes,
                CheckpointWalBoundary::MutationEntries => self.wal.max_mutation_entries,
                CheckpointWalBoundary::MutationKeyBytes => self.wal.max_mutation_key_bytes,
            },
            CheckpointResourceBoundary::Admission(resource) => match resource {
                AdmissionBoundary::PerStateDistinctObjects => {
                    self.admission.per_state_structural.max_distinct_objects
                }
                AdmissionBoundary::PerStateCanonicalBytes => {
                    self.admission.per_state_structural.max_canonical_bytes
                }
                AdmissionBoundary::PerStateReferences => {
                    self.admission.per_state_structural.max_references
                }
                AdmissionBoundary::PerStateWorklist => {
                    self.admission.per_state_structural.max_worklist
                }
                AdmissionBoundary::RetainedStates => self.admission.max_retained_states,
                AdmissionBoundary::UnionObjects => self.admission.max_union_objects,
                AdmissionBoundary::UnionCanonicalBytes => self.admission.max_union_canonical_bytes,
                AdmissionBoundary::AggregateRootVisits => self.admission.max_aggregate_root_visits,
                AdmissionBoundary::AggregateReferences => self.admission.max_aggregate_references,
                AdmissionBoundary::ObjectReads => self.admission.max_object_reads,
                AdmissionBoundary::ObjectReadBytes => self.admission.max_object_read_bytes,
                AdmissionBoundary::LogicalNodes => {
                    self.admission.current_semantics.max_logical_nodes
                }
                AdmissionBoundary::Endpoints => self.admission.current_semantics.max_endpoints,
                AdmissionBoundary::LogicalPathBytes => {
                    self.admission.current_semantics.max_logical_path_bytes
                }
                AdmissionBoundary::PayloadBytes => {
                    self.admission.current_semantics.max_payload_bytes
                }
                AdmissionBoundary::JoinLookups => self.admission.current_semantics.max_join_lookups,
                AdmissionBoundary::JoinLookupSteps => {
                    self.admission.current_semantics.max_join_lookup_steps
                }
                AdmissionBoundary::SemanticWorklist => {
                    self.admission.current_semantics.max_worklist
                }
            },
        }
    }

    const fn minimum(boundary: CheckpointResourceBoundary) -> u64 {
        match boundary {
            CheckpointResourceBoundary::Durable(_) => 1,
            CheckpointResourceBoundary::Wal(_) | CheckpointResourceBoundary::Admission(_) => 0,
        }
    }

    fn set(&mut self, boundary: CheckpointResourceBoundary, value: u64) {
        match boundary {
            CheckpointResourceBoundary::Durable(resource) => {
                set_durable_limit(&mut self.durable, resource, value);
            }
            CheckpointResourceBoundary::Wal(resource) => match resource {
                CheckpointWalBoundary::RecordBytes => self.wal.max_record_bytes = value,
                CheckpointWalBoundary::MutationBytes => self.wal.max_mutation_bytes = value,
                CheckpointWalBoundary::Objects => self.wal.max_objects = value,
                CheckpointWalBoundary::ObjectBytes => self.wal.max_object_bytes = value,
                CheckpointWalBoundary::SingleObjectBytes => {
                    self.wal.max_single_object_bytes = value;
                }
                CheckpointWalBoundary::MutationEntries => self.wal.max_mutation_entries = value,
                CheckpointWalBoundary::MutationKeyBytes => self.wal.max_mutation_key_bytes = value,
            },
            CheckpointResourceBoundary::Admission(resource) => match resource {
                AdmissionBoundary::PerStateDistinctObjects => {
                    self.admission.per_state_structural.max_distinct_objects = value;
                }
                AdmissionBoundary::PerStateCanonicalBytes => {
                    self.admission.per_state_structural.max_canonical_bytes = value;
                }
                AdmissionBoundary::PerStateReferences => {
                    self.admission.per_state_structural.max_references = value;
                }
                AdmissionBoundary::PerStateWorklist => {
                    self.admission.per_state_structural.max_worklist = value;
                }
                AdmissionBoundary::RetainedStates => self.admission.max_retained_states = value,
                AdmissionBoundary::UnionObjects => self.admission.max_union_objects = value,
                AdmissionBoundary::UnionCanonicalBytes => {
                    self.admission.max_union_canonical_bytes = value;
                }
                AdmissionBoundary::AggregateRootVisits => {
                    self.admission.max_aggregate_root_visits = value;
                }
                AdmissionBoundary::AggregateReferences => {
                    self.admission.max_aggregate_references = value;
                }
                AdmissionBoundary::ObjectReads => self.admission.max_object_reads = value,
                AdmissionBoundary::ObjectReadBytes => self.admission.max_object_read_bytes = value,
                AdmissionBoundary::LogicalNodes => {
                    self.admission.current_semantics.max_logical_nodes = value;
                }
                AdmissionBoundary::Endpoints => {
                    self.admission.current_semantics.max_endpoints = value;
                }
                AdmissionBoundary::LogicalPathBytes => {
                    self.admission.current_semantics.max_logical_path_bytes = value;
                }
                AdmissionBoundary::PayloadBytes => {
                    self.admission.current_semantics.max_payload_bytes = value;
                }
                AdmissionBoundary::JoinLookups => {
                    self.admission.current_semantics.max_join_lookups = value;
                }
                AdmissionBoundary::JoinLookupSteps => {
                    self.admission.current_semantics.max_join_lookup_steps = value;
                }
                AdmissionBoundary::SemanticWorklist => {
                    self.admission.current_semantics.max_worklist = value;
                }
            },
        }
    }
}

fn set_durable_limit(config: &mut DurableLimitConfig, resource: DurableLimitKind, value: u64) {
    match resource {
        DurableLimitKind::LivePacks => config.max_live_packs = value,
        DurableLimitKind::PackBytes => config.max_pack_bytes = value,
        DurableLimitKind::Transactions => config.max_transactions = value,
        DurableLimitKind::ObjectRecords => config.max_object_records = value,
        DurableLimitKind::TotalScanBytes => config.max_total_scan_bytes = value,
        DurableLimitKind::TotalPhysicalReadBytes => {
            config.max_total_physical_read_bytes = value;
        }
        DurableLimitKind::IndexBytes => config.max_index_bytes = value,
        DurableLimitKind::IndexRecords => config.max_index_records = value,
        DurableLimitKind::FullByteComparisonBytes => {
            config.max_full_byte_comparison_bytes = value;
        }
        DurableLimitKind::AggregateFullByteComparisonBytes => {
            config.max_aggregate_full_byte_comparison_bytes = value;
        }
    }
}

fn assert_exact_boundary_error(
    boundary: CheckpointResourceBoundary,
    limit: u64,
    required: u64,
    error: &GenerationCheckpointPublicationErrorV1,
) {
    match boundary {
        CheckpointResourceBoundary::Durable(resource) => assert!(matches!(
            error,
            GenerationCheckpointPublicationErrorV1::Limit(DurableLimitError::Exceeded {
                resource: actual_resource,
                limit: actual_limit,
                attempted,
            }) if *actual_resource == resource && *actual_limit == limit && *attempted == required
        )),
        CheckpointResourceBoundary::Wal(resource) => assert!(matches!(
            error,
            GenerationCheckpointPublicationErrorV1::LogPack(error)
                if matches!(error.as_ref(), LogPackFormatError::ResourceLimit {
                    resource: actual_resource,
                    limit: actual_limit,
                    attempted,
                } if *actual_resource == resource.resource_name() &&
                    *actual_limit == limit && *attempted == required)
        )),
        CheckpointResourceBoundary::Admission(resource) => assert!(matches!(
            error,
            GenerationCheckpointPublicationErrorV1::AdmissionPreflight(actual)
                if actual.resource == resource.kind() &&
                    actual.limit == limit && actual.attempted == required
        )),
    }
}

/// Exhaustive executable refinement of PUB-04, RES-02, and NV-06..NV-12.
#[test]
fn every_checkpoint_resource_boundary_refuses_one_below_and_accepts_exactly() {
    let source = semantic_checkpoint_source_fixture();
    let image = checkpoint_image_from_source(source.clone());
    let exact_case = test_scratch::acquire("rspace-checkpoint-resource-exact-");
    install_checkpoint_image(exact_case.path(), &image);
    let writer = admit(exact_case.path());
    let old_report = writer.report();
    let expected = expected_successor_from_source(writer.authority(), &source);
    let exact = ExactCheckpointPublicationLimits::derive(&source, &expected, old_report);

    start_generation_checkpoint_publication_trace();
    let (durable, wal, admission) = exact.build();
    let published = writer
        .publish_generation_checkpoint_v1(durable, wal, admission)
        .unwrap();
    assert_eq!(take_generation_checkpoint_publication_trace(), FORMAL_PUBLICATION_PHASE_ORDER);
    assert_eq!(published.authority().image(), &expected.catalog);
    assert_eq!(published.report().current_semantics, old_report.current_semantics);
    assert_eq!(published.report().retained_union, old_report.retained_union);
    drop(published);

    let (durable, wal, admission) = exact.build();
    let restarted = DurableDirectoryV1::open(exact_case.path())
        .unwrap()
        .recover_and_admit_v1::<TestSemanticProfile>(durable, wal, admission)
        .unwrap();
    assert_eq!(restarted.authority().image(), &expected.catalog);
    drop(restarted);

    let mut durable_below = 0;
    let mut wal_below = 0;
    let mut admission_below = 0;
    let mut wal_zero = 0;
    for boundary in CHECKPOINT_RESOURCE_BOUNDARIES {
        let required = exact.requirement(boundary);
        let minimum = ExactCheckpointPublicationLimits::minimum(boundary);
        if required <= minimum {
            if matches!(boundary, CheckpointResourceBoundary::Wal(_)) && required == 0 {
                wal_zero += 1;
            }
            continue;
        }
        match boundary {
            CheckpointResourceBoundary::Durable(_) => durable_below += 1,
            CheckpointResourceBoundary::Wal(_) => wal_below += 1,
            CheckpointResourceBoundary::Admission(_) => admission_below += 1,
        }

        let scratch = test_scratch::acquire("rspace-checkpoint-resource-below-");
        install_checkpoint_image(scratch.path(), &image);
        let writer = admit(scratch.path());
        let old_authority = writer.authority().clone();
        let catalog_before = fs::read(scratch.path().join("catalog.v1")).unwrap();
        let old_pack_path = scratch.path().join(pack_file_name(PackId::GENESIS));
        let old_index_path = scratch
            .path()
            .join(index_file_name(IndexGenerationV1::FIRST));
        let old_pack = fs::read(&old_pack_path).unwrap();
        let old_index = fs::read(&old_index_path).unwrap();
        let successor_pack = scratch
            .path()
            .join(pack_file_name(PackId::GENESIS.successor().unwrap()));
        let successor_index = scratch
            .path()
            .join(index_file_name(IndexGenerationV1::FIRST.successor().unwrap()));
        let pack_orphan = vec![0x53; 23];
        let index_orphan = vec![0x64; 29];
        write_owner_only(&successor_pack, &pack_orphan);
        write_owner_only(&successor_index, &index_orphan);

        let lower = required - 1;
        let mut insufficient = exact;
        insufficient.set(boundary, lower);
        let (durable, wal, admission) = insufficient.build();
        start_generation_checkpoint_publication_trace();
        let error =
            publication_error(writer.publish_generation_checkpoint_v1(durable, wal, admission));
        let trace = take_generation_checkpoint_publication_trace();
        assert_exact_boundary_error(boundary, lower, required, &error);
        assert_eq!(
            error.publication_outcome(),
            GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished,
            "required-minus-one boundary {boundary:?}",
        );
        assert!(trace.is_empty(), "resource refusal occurred after phase mutation: {boundary:?}");
        assert_eq!(fs::read(scratch.path().join("catalog.v1")).unwrap(), catalog_before);
        assert_eq!(fs::read(&old_pack_path).unwrap(), old_pack);
        assert_eq!(fs::read(&old_index_path).unwrap(), old_index);
        assert_eq!(fs::read(&successor_pack).unwrap(), pack_orphan);
        assert_eq!(fs::read(&successor_index).unwrap(), index_orphan);

        let recovered_old = admit(scratch.path());
        assert_eq!(recovered_old.authority(), &old_authority);
    }

    for resource in DurableLimitKind::ALL {
        let mut invalid = exact.durable;
        set_durable_limit(&mut invalid, resource, 0);
        assert_eq!(DurableLimits::new(invalid), Err(DurableLimitError::Zero { resource }),);
    }
    assert_eq!(durable_below, 6, "all six constructible durable lower boundaries ran");
    assert_eq!(wal_below, 4, "all four positive checkpoint-WAL lower boundaries ran");
    assert_eq!(admission_below, 18, "all eighteen admission lower boundaries ran");
    assert_eq!(wal_zero, 3, "all three zero-demand checkpoint-WAL axes stayed at zero");

    let (historical_source, historical_requirement) =
        historically_larger_checkpoint_source_fixture();
    let historical_image = checkpoint_image_from_source(historical_source);
    for boundary in [
        AdmissionBoundary::PerStateDistinctObjects,
        AdmissionBoundary::PerStateCanonicalBytes,
        AdmissionBoundary::PerStateReferences,
        AdmissionBoundary::PerStateWorklist,
    ] {
        let scratch = test_scratch::acquire("rspace-checkpoint-resource-historical-max-");
        install_checkpoint_image(scratch.path(), &historical_image);
        let writer = admit(scratch.path());
        let current = writer.report().current_structural;
        let required = match boundary {
            AdmissionBoundary::PerStateDistinctObjects => historical_requirement.distinct_objects,
            AdmissionBoundary::PerStateCanonicalBytes => historical_requirement.canonical_bytes,
            AdmissionBoundary::PerStateReferences => historical_requirement.references,
            AdmissionBoundary::PerStateWorklist => historical_requirement.maximum_worklist,
            _ => unreachable!("the table contains only per-state structural fields"),
        };
        let current_required = match boundary {
            AdmissionBoundary::PerStateDistinctObjects => current.distinct_objects,
            AdmissionBoundary::PerStateCanonicalBytes => current.canonical_bytes,
            AdmissionBoundary::PerStateReferences => current.references,
            AdmissionBoundary::PerStateWorklist => current.maximum_worklist,
            _ => unreachable!("the table contains only per-state structural fields"),
        };
        assert!(
            required > current_required,
            "historical fixture must distinguish {boundary:?} from current-only demand",
        );
        let mut admission = DurableAdmissionLimitsV1::UNBOUNDED;
        match boundary {
            AdmissionBoundary::PerStateDistinctObjects => {
                admission.per_state_structural.max_distinct_objects = required - 1;
            }
            AdmissionBoundary::PerStateCanonicalBytes => {
                admission.per_state_structural.max_canonical_bytes = required - 1;
            }
            AdmissionBoundary::PerStateReferences => {
                admission.per_state_structural.max_references = required - 1;
            }
            AdmissionBoundary::PerStateWorklist => {
                admission.per_state_structural.max_worklist = required - 1;
            }
            _ => unreachable!("the table contains only per-state structural fields"),
        }
        let catalog_before = fs::read(scratch.path().join("catalog.v1")).unwrap();
        start_generation_checkpoint_publication_trace();
        let error = publication_error(writer.publish_generation_checkpoint_v1(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            admission,
        ));
        assert!(
            matches!(
                error,
                GenerationCheckpointPublicationErrorV1::AdmissionPreflight(actual)
                    if actual.resource == boundary.kind() &&
                        actual.limit == required - 1 && actual.attempted == required
            ),
            "historical maximum {boundary:?} produced {error:?}; expected limit {} and demand \
             {required}",
            required - 1,
        );
        assert_eq!(
            error.publication_outcome(),
            GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished,
        );
        assert!(take_generation_checkpoint_publication_trace().is_empty());
        assert_eq!(fs::read(scratch.path().join("catalog.v1")).unwrap(), catalog_before,);
        assert!(
            !scratch
                .path()
                .join(pack_file_name(PackId::GENESIS.successor().unwrap()))
                .exists()
        );
        assert!(
            !scratch
                .path()
                .join(index_file_name(IndexGenerationV1::FIRST.successor().unwrap()))
                .exists()
        );
    }
}

#[test]
fn exact_catalog_change_before_cas_causes_no_publisher_slot_write() {
    let scratch = test_scratch::acquire("rspace-durable-publication-cas-");
    install_checkpoint_store(scratch.path());
    let writer = admit(scratch.path());
    let old_image = *writer.authority().image();
    let catalog_path = scratch.path().join("catalog.v1");
    let mut changed = old_image;
    changed[CATALOG_IMAGE_LEN - 1] ^= 0x80;
    let hook_path = catalog_path.clone();
    arm_generation_checkpoint_publication_hook(
        GenerationCheckpointPublicationHookPointV1::BeforeCatalogCompareAndSwap,
        move || {
            fs::write(&hook_path, changed).unwrap();
            #[cfg(unix)]
            fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o600)).unwrap();
        },
    );
    let error = publication_error(publish(writer));
    assert!(matches!(error, GenerationCheckpointPublicationErrorV1::CatalogCompareAndSwap));
    assert_eq!(
        error.publication_outcome(),
        GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished
    );
    assert_eq!(fs::read(&catalog_path).unwrap(), changed);

    fs::write(&catalog_path, old_image).unwrap();
    #[cfg(unix)]
    fs::set_permissions(&catalog_path, fs::Permissions::from_mode(0o600)).unwrap();
    let retried = publish(admit(scratch.path())).unwrap();
    assert_eq!(retried.authority().slot().pack().active_pack(), 2);
}

#[test]
fn catalog_uncertainty_classifies_both_old_and_new_observable_outcomes() {
    let old_case = test_scratch::acquire("rspace-durable-publication-catalog-old-");
    install_checkpoint_store(old_case.path());
    let old_writer = admit(old_case.path());
    let old_catalog = *old_writer.authority().image();
    let old_expected = expected_successor(old_writer.authority());
    arm_durable_io_failpoint(DurableIoFailpointV1::CatalogSlotWrite);
    let old_error = publication_error(publish(old_writer));
    assert_eq!(
        old_error.publication_outcome(),
        GenerationCheckpointPublicationOutcomeV1::Indeterminate
    );
    assert!(matches!(
        &old_error,
        GenerationCheckpointPublicationErrorV1::CatalogCommitIndeterminate(_)
    ));
    assert_eq!(fs::read(old_case.path().join("catalog.v1")).unwrap(), old_catalog);
    assert_eq!(
        fs::read(
            old_case
                .path()
                .join(pack_file_name(PackId::GENESIS.successor().unwrap()))
        )
        .unwrap(),
        old_expected.pack,
    );
    assert_eq!(
        fs::read(
            old_case
                .path()
                .join(index_file_name(IndexGenerationV1::FIRST.successor().unwrap()))
        )
        .unwrap(),
        old_expected.index,
    );
    let old_recovered = admit(old_case.path());
    assert_eq!(old_recovered.authority().image(), &old_catalog);
    let old_retry = publish(old_recovered).unwrap();
    assert_eq!(old_retry.authority().slot().pack().active_pack(), 2);
    assert_eq!(
        old_error.publication_outcome(),
        GenerationCheckpointPublicationOutcomeV1::Indeterminate,
    );

    let new_case = test_scratch::acquire("rspace-durable-publication-catalog-new-");
    install_checkpoint_store(new_case.path());
    let new_writer = admit(new_case.path());
    let expected = expected_successor(new_writer.authority());
    arm_durable_io_failpoint(DurableIoFailpointV1::CatalogSync);
    let new_error = publication_error(publish(new_writer));
    assert_eq!(
        new_error.publication_outcome(),
        GenerationCheckpointPublicationOutcomeV1::Indeterminate
    );
    assert!(matches!(
        &new_error,
        GenerationCheckpointPublicationErrorV1::CatalogCommitIndeterminate(_)
    ));
    assert_eq!(fs::read(new_case.path().join("catalog.v1")).unwrap(), expected.catalog);
    assert_eq!(
        fs::read(
            new_case
                .path()
                .join(pack_file_name(PackId::GENESIS.successor().unwrap()))
        )
        .unwrap(),
        expected.pack,
    );
    assert_eq!(
        fs::read(
            new_case
                .path()
                .join(index_file_name(IndexGenerationV1::FIRST.successor().unwrap()))
        )
        .unwrap(),
        expected.index,
    );
    let new_recovered = admit(new_case.path());
    assert_eq!(new_recovered.authority().image(), &expected.catalog);
    assert_eq!(
        new_error.publication_outcome(),
        GenerationCheckpointPublicationOutcomeV1::Indeterminate,
    );
}

#[derive(Clone, Copy, Debug)]
enum PublicationOutcomeCase {
    Success,
    Hook(GenerationCheckpointPublicationHookPointV1),
    PhysicalWrite,
    PhysicalSync,
    CatalogSlotWrite,
    CatalogFileSync,
    CatalogDirectorySync,
    ReclamationIntent(DurableIoFailpointV1),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestartAuthority {
    Old,
    New,
}

#[test]
fn publication_outcomes_are_total_and_mutually_exclusive() {
    use GenerationCheckpointPublicationOutcomeV1::{
        DefinitelyNotPublished, Indeterminate, Published,
    };

    let mut cases = vec![(PublicationOutcomeCase::Success, None, RestartAuthority::New, 18)];
    for (phase_index, point) in FORMAL_PUBLICATION_PHASE_ORDER.iter().copied().enumerate() {
        let outcome = match point {
            GenerationCheckpointPublicationHookPointV1::AfterPreflightBeforeCleanup |
            GenerationCheckpointPublicationHookPointV1::AfterCleanup |
            GenerationCheckpointPublicationHookPointV1::AfterPrepareWrite |
            GenerationCheckpointPublicationHookPointV1::AfterPrepareSync |
            GenerationCheckpointPublicationHookPointV1::AfterIndexWrite |
            GenerationCheckpointPublicationHookPointV1::AfterIndexSync |
            GenerationCheckpointPublicationHookPointV1::AfterCommitWrite |
            GenerationCheckpointPublicationHookPointV1::AfterCommitSync |
            GenerationCheckpointPublicationHookPointV1::AfterArtifactDirectorySync |
            GenerationCheckpointPublicationHookPointV1::BeforeCatalogCompareAndSwap |
            GenerationCheckpointPublicationHookPointV1::AfterCatalogCasVerifiedBeforeSlotWrite => {
                DefinitelyNotPublished
            }
            GenerationCheckpointPublicationHookPointV1::AfterCatalogSlotWrite => Indeterminate,
            GenerationCheckpointPublicationHookPointV1::AfterCatalogFileSyncBeforeDirectorySync |
            GenerationCheckpointPublicationHookPointV1::AfterCatalogDirectorySync |
            GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery |
            GenerationCheckpointPublicationHookPointV1::AfterPhysicalRecovery |
            GenerationCheckpointPublicationHookPointV1::BeforeSemanticAdmission |
            GenerationCheckpointPublicationHookPointV1::AfterSemanticAdmission => Published,
        };
        let restart = if outcome == DefinitelyNotPublished {
            RestartAuthority::Old
        } else {
            RestartAuthority::New
        };
        cases.push((PublicationOutcomeCase::Hook(point), Some(outcome), restart, phase_index + 1));
    }
    cases.extend([
        (
            PublicationOutcomeCase::PhysicalWrite,
            Some(DefinitelyNotPublished),
            RestartAuthority::Old,
            2,
        ),
        (
            PublicationOutcomeCase::PhysicalSync,
            Some(DefinitelyNotPublished),
            RestartAuthority::Old,
            3,
        ),
        (PublicationOutcomeCase::CatalogSlotWrite, Some(Indeterminate), RestartAuthority::Old, 11),
        (PublicationOutcomeCase::CatalogFileSync, Some(Indeterminate), RestartAuthority::New, 12),
        (PublicationOutcomeCase::CatalogDirectorySync, Some(Published), RestartAuthority::New, 13),
    ]);
    cases.extend(
        [
            DurableIoFailpointV1::ReclamationIntentCreate,
            DurableIoFailpointV1::ReclamationIntentSetLength,
            DurableIoFailpointV1::ReclamationIntentSync,
            DurableIoFailpointV1::ReclamationIntentBeforeRename,
            DurableIoFailpointV1::ReclamationIntentDirectorySync,
        ]
        .map(|failpoint| {
            (
                PublicationOutcomeCase::ReclamationIntent(failpoint),
                Some(DefinitelyNotPublished),
                RestartAuthority::Old,
                8,
            )
        }),
    );

    for (case, expected, restart, trace_len) in cases {
        let scratch = test_scratch::acquire("rspace-durable-publication-outcome-matrix-");
        install_checkpoint_store(scratch.path());
        let writer = admit(scratch.path());
        let old_authority = writer.authority().slot();
        let successor = expected_successor(writer.authority());
        start_generation_checkpoint_publication_trace();
        match case {
            PublicationOutcomeCase::Success => {}
            PublicationOutcomeCase::Hook(point) => {
                let root = scratch.path().to_path_buf();
                arm_generation_checkpoint_publication_hook(point, move || {
                    let contender = DurableDirectoryV1::open(&root)
                        .unwrap()
                        .recover_and_admit_v1::<TestSemanticProfile>(
                            limits(),
                            LogPackDecodeLimits::UNBOUNDED,
                            DurableAdmissionLimitsV1::UNBOUNDED,
                        );
                    assert!(matches!(
                        contender,
                        Err(DurableAdmissionErrorV1::Startup(DurableStartupErrorV1::Io(
                            DurableIoError::WriterLockContended { .. }
                        )))
                    ));
                });
                arm_generation_checkpoint_publication_failpoint(point);
            }
            PublicationOutcomeCase::PhysicalWrite => {
                arm_durable_io_failpoint(DurableIoFailpointV1::PhysicalWrite);
            }
            PublicationOutcomeCase::PhysicalSync => {
                arm_durable_io_failpoint(DurableIoFailpointV1::PhysicalSync);
            }
            PublicationOutcomeCase::CatalogSlotWrite => {
                arm_durable_io_failpoint(DurableIoFailpointV1::CatalogSlotWrite);
            }
            PublicationOutcomeCase::CatalogFileSync => {
                arm_durable_io_failpoint(DurableIoFailpointV1::CatalogSync);
            }
            PublicationOutcomeCase::CatalogDirectorySync => {
                arm_durable_io_failpoint(DurableIoFailpointV1::CatalogDirectorySync);
            }
            PublicationOutcomeCase::ReclamationIntent(failpoint) => {
                arm_durable_io_failpoint(failpoint);
            }
        }

        let result = publish(writer);
        let trace = take_generation_checkpoint_publication_trace();
        assert_eq!(trace, FORMAL_PUBLICATION_PHASE_ORDER[..trace_len], "trace case {case:?}");
        let observed = result
            .as_ref()
            .err()
            .map(|error| error.publication_outcome());
        assert_eq!(observed, expected, "outcome case {case:?}");
        if expected.is_none() {
            match result {
                Ok(writer) => drop(writer),
                Err(error) => panic!("success case failed: {error}"),
            }
            continue;
        }

        let error = publication_error(result);
        let original_outcome = error.publication_outcome();
        let reopened = admit(scratch.path());
        match restart {
            RestartAuthority::Old => assert_eq!(reopened.authority().slot(), old_authority),
            RestartAuthority::New => assert_eq!(reopened.authority().slot(), successor.slot),
        }
        assert_eq!(error.publication_outcome(), original_outcome);

        if restart == RestartAuthority::Old {
            let retried = publish(reopened).unwrap();
            assert_eq!(retried.authority().slot(), successor.slot);
        } else {
            let successor_pack = scratch
                .path()
                .join(pack_file_name(PackId::new(successor.slot.pack().active_pack()).unwrap()));
            let successor_index = scratch.path().join(index_file_name(
                IndexGenerationV1::new(successor.slot.pack().index_generation()).unwrap(),
            ));
            assert_eq!(fs::read(successor_pack).unwrap(), successor.pack);
            assert_eq!(fs::read(successor_index).unwrap(), successor.index);
        }
    }
}

#[test]
fn every_injected_torn_catalog_prefix_recovers_only_old_or_exact_new_authority() {
    for prefix_len in 0..crate::rspace::persistence::CATALOG_SLOT_LEN {
        let scratch = trial_scratch("catalog-prefix-");
        install_checkpoint_store(scratch.path());
        let writer = admit(scratch.path());
        let old_authority = writer.authority().slot();
        let expected = expected_successor(writer.authority());

        arm_catalog_slot_write_prefix(prefix_len);
        let error = publication_error(publish(writer));
        assert_eq!(
            error.publication_outcome(),
            GenerationCheckpointPublicationOutcomeV1::Indeterminate,
        );
        assert!(matches!(
            error,
            GenerationCheckpointPublicationErrorV1::CatalogCommitIndeterminate(_)
        ));

        let reopened = admit(scratch.path());
        let observed = reopened.authority().slot();
        assert!(
            observed == old_authority || observed == expected.slot,
            "prefix {prefix_len} selected an authority other than exact old or exact new"
        );
        if observed == old_authority {
            let retried = publish(reopened).unwrap();
            assert_eq!(retried.authority().slot(), expected.slot);
            assert_eq!(fs::read(scratch.path().join("catalog.v1")).unwrap(), expected.catalog);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn physical_layout_variation_preserves_semantics_and_exact_successor(
        pack_orphan_len in 1_usize..4096,
        index_orphan_len in 1_usize..4096,
    ) {
        let left = trial_scratch("physical-left-");
        let right = trial_scratch("physical-right-");
        install_checkpoint_store(left.path());
        install_checkpoint_store(right.path());
        let left_writer = admit(left.path());
        let right_writer = admit(right.path());
        let left_before = left_writer.report();
        let right_before = right_writer.report();
        prop_assert_eq!(left_before, right_before);
        let left_expected = expected_successor(left_writer.authority());
        let right_expected = expected_successor(right_writer.authority());
        prop_assert_eq!(&left_expected.pack, &right_expected.pack);
        prop_assert_eq!(&left_expected.index, &right_expected.index);
        prop_assert_eq!(&left_expected.catalog, &right_expected.catalog);

        let left_pack_orphan = left
            .path()
            .join(pack_file_name(PackId::GENESIS.successor().unwrap()));
        let right_pack_orphan = right
            .path()
            .join(pack_file_name(PackId::GENESIS.successor().unwrap()));
        let left_index_orphan = left.path().join(index_file_name(
            IndexGenerationV1::FIRST.successor().unwrap(),
        ));
        let right_index_orphan = right.path().join(index_file_name(
            IndexGenerationV1::FIRST.successor().unwrap(),
        ));
        write_owner_only(
            &left_pack_orphan,
            &vec![0x53; pack_orphan_len],
        );
        write_owner_only(
            &right_pack_orphan,
            &vec![0x53; pack_orphan_len + 4096],
        );
        write_owner_only(
            &left_index_orphan,
            &vec![0x64; index_orphan_len],
        );
        write_owner_only(
            &right_index_orphan,
            &vec![0x64; index_orphan_len + 8192],
        );
        prop_assert_ne!(
            fs::metadata(&left_pack_orphan).unwrap().len(),
            fs::metadata(&right_pack_orphan).unwrap().len(),
        );
        prop_assert_ne!(
            fs::metadata(&left_index_orphan).unwrap().len(),
            fs::metadata(&right_index_orphan).unwrap().len(),
        );

        let left_published = publish(left_writer).unwrap();
        let right_published = publish(right_writer).unwrap();
        prop_assert_eq!(left_published.report(), right_published.report());
        prop_assert_eq!(
            left_published.verified_state().version(),
            right_published.verified_state().version(),
        );
        prop_assert_eq!(
            left_published.retention_manifest_bytes(),
            right_published.retention_manifest_bytes(),
        );
        prop_assert_eq!(left_published.authority(), right_published.authority());
        prop_assert_eq!(
            fs::read(left.path().join("catalog.v1")).unwrap(),
            fs::read(right.path().join("catalog.v1")).unwrap(),
        );
        prop_assert_eq!(fs::read(left_pack_orphan).unwrap(), left_expected.pack);
        prop_assert_eq!(fs::read(right_pack_orphan).unwrap(), right_expected.pack);
        prop_assert_eq!(fs::read(left_index_orphan).unwrap(), left_expected.index);
        prop_assert_eq!(fs::read(right_index_orphan).unwrap(), right_expected.index);
    }
}

#[test]
fn post_catalog_directory_failure_is_published_and_restart_selects_new() {
    let scratch = test_scratch::acquire("rspace-durable-publication-catalog-directory-");
    install_checkpoint_store(scratch.path());
    let writer = admit(scratch.path());
    let expected = expected_successor(writer.authority());

    arm_durable_io_failpoint(DurableIoFailpointV1::CatalogDirectorySync);
    let error = publication_error(publish(writer));
    assert_eq!(error.publication_outcome(), GenerationCheckpointPublicationOutcomeV1::Published,);
    assert!(matches!(error, GenerationCheckpointPublicationErrorV1::PublishedCatalogDirectory(_)));
    assert_eq!(fs::read(scratch.path().join("catalog.v1")).unwrap(), expected.catalog);

    let reopened = admit(scratch.path());
    assert_eq!(reopened.authority().image(), &expected.catalog);
}

#[test]
fn catalog_sync_to_recovery_has_no_writer_lock_gap() {
    let scratch = test_scratch::acquire("rspace-durable-publication-lock-gap-");
    install_checkpoint_store(scratch.path());
    let writer = admit(scratch.path());
    let root = scratch.path().to_path_buf();
    arm_generation_checkpoint_publication_hook(
        GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery,
        move || {
            let contender = DurableDirectoryV1::open(&root)
                .unwrap()
                .recover_and_admit_v1::<TestSemanticProfile>(
                    limits(),
                    LogPackDecodeLimits::UNBOUNDED,
                    DurableAdmissionLimitsV1::UNBOUNDED,
                );
            assert!(matches!(
                contender,
                Err(DurableAdmissionErrorV1::Startup(DurableStartupErrorV1::Io(
                    DurableIoError::WriterLockContended { .. }
                )))
            ));
        },
    );
    let published = publish(writer).unwrap();
    assert_eq!(published.authority().slot().pack().active_pack(), 2);
}

#[test]
fn physical_write_and_sync_failures_leave_the_old_authority_selected() {
    for failpoint in [DurableIoFailpointV1::PhysicalWrite, DurableIoFailpointV1::PhysicalSync] {
        let scratch = test_scratch::acquire("rspace-durable-publication-physical-io-");
        install_checkpoint_store(scratch.path());
        let writer = admit(scratch.path());
        let old_authority = writer.authority().slot();
        arm_durable_io_failpoint(failpoint);

        let error = publication_error(publish(writer));
        assert_eq!(
            error.publication_outcome(),
            GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished,
            "physical failpoint {failpoint:?} must fail before catalog publication",
        );

        let reopened = admit(scratch.path());
        assert_eq!(reopened.authority().slot(), old_authority);
    }
}

#[cfg(unix)]
#[test]
fn published_but_recovery_failure_is_typed_and_restartable() {
    let scratch = test_scratch::acquire("rspace-durable-publication-readmit-");
    install_checkpoint_store(scratch.path());
    let writer = admit(scratch.path());
    let expected = expected_successor(writer.authority());
    let successor_pack = scratch
        .path()
        .join(pack_file_name(PackId::GENESIS.successor().unwrap()));
    let hook_path = successor_pack.clone();
    arm_generation_checkpoint_publication_hook(
        GenerationCheckpointPublicationHookPointV1::AfterCatalogSyncBeforeRecovery,
        move || fs::set_permissions(hook_path, fs::Permissions::from_mode(0o000)).unwrap(),
    );
    let error = publication_error(publish(writer));
    assert_eq!(error.publication_outcome(), GenerationCheckpointPublicationOutcomeV1::Published);
    assert!(matches!(error, GenerationCheckpointPublicationErrorV1::PublishedRecovery(_)));
    assert_eq!(fs::read(scratch.path().join("catalog.v1")).unwrap(), expected.catalog);

    fs::set_permissions(&successor_pack, fs::Permissions::from_mode(0o600)).unwrap();
    let reopened = admit(scratch.path());
    assert_eq!(reopened.authority().image(), &expected.catalog);
}

#[test]
fn published_but_semantic_admission_failure_is_typed_and_restartable() {
    let scratch = test_scratch::acquire("rspace-durable-publication-admission-");
    install_checkpoint_store(scratch.path());
    let writer = admit(scratch.path());
    let expected = expected_successor(writer.authority());

    arm_generation_checkpoint_publication_failpoint(
        GenerationCheckpointPublicationHookPointV1::BeforeSemanticAdmission,
    );
    let error = publication_error(publish(writer));
    assert_eq!(error.publication_outcome(), GenerationCheckpointPublicationOutcomeV1::Published,);
    assert!(matches!(error, GenerationCheckpointPublicationErrorV1::PublishedAdmission(_)));
    assert_eq!(fs::read(scratch.path().join("catalog.v1")).unwrap(), expected.catalog);

    let reopened = admit(scratch.path());
    assert_eq!(reopened.authority().image(), &expected.catalog);
}

/// Cold admission's refusal, classified by the layer that refused it.
#[derive(Debug, PartialEq, Eq)]
enum ColdRefusal {
    DurableLimit(DurableLimitError),
    Wal(LogPackFormatError),
    Canonical(CanonicalFormatError),
    ClosureFormat(CanonicalFormatError),
    MetadataReadLimit {
        resource: &'static str,
        limit: u64,
        attempted: u64,
    },
    ClosureReadLimit {
        resource: &'static str,
        limit: u64,
        attempted: u64,
    },
    Materialization {
        digest: ObjectDigest,
        limit: u64,
        attempted: u64,
    },
    Radix(RetentionRadixRefusalV2),
    CurrentMismatch {
        catalog: StateVersionV1,
        manifest: StateVersionV1,
    },
    NonHistorical {
        current_generation: u64,
        retained_generation: u64,
    },
    Inventory {
        kind: InventoryMismatchKindV1,
        declared: u64,
        computed: u64,
    },
    WriterUnreadable {
        writer: RetentionFormat,
        reader: RetentionReader,
    },
    Unclassified(String),
}

fn cold_refusal(error: DurableAdmissionErrorV1) -> ColdRefusal {
    match error {
        DurableAdmissionErrorV1::Startup(
            DurableStartupErrorV1::Recovery(RecoveryError::Limit(error)) |
            DurableStartupErrorV1::Limit(error),
        ) => ColdRefusal::DurableLimit(error),
        DurableAdmissionErrorV1::Startup(DurableStartupErrorV1::Recovery(RecoveryError::Wal(
            error,
        ))) => ColdRefusal::Wal(error),
        DurableAdmissionErrorV1::Canonical(error) => ColdRefusal::Canonical(error),
        DurableAdmissionErrorV1::Closure(ClosureError::Format(error)) => {
            ColdRefusal::ClosureFormat(error)
        }
        DurableAdmissionErrorV1::Closure(ClosureError::Store(
            DurableObjectReadErrorV1::AdmissionReadLimit {
                resource,
                limit,
                attempted,
            },
        )) => ColdRefusal::ClosureReadLimit {
            resource,
            limit,
            attempted,
        },
        DurableAdmissionErrorV1::ObjectStore(DurableObjectReadErrorV1::AdmissionReadLimit {
            resource,
            limit,
            attempted,
        }) => ColdRefusal::MetadataReadLimit {
            resource,
            limit,
            attempted,
        },
        DurableAdmissionErrorV1::ObjectStore(DurableObjectReadErrorV1::MaterializationLimit {
            digest,
            limit,
            attempted,
        }) => ColdRefusal::Materialization {
            digest,
            limit,
            attempted,
        },
        DurableAdmissionErrorV1::RetentionRadix(refusal) => ColdRefusal::Radix(refusal),
        DurableAdmissionErrorV1::RetentionCurrentMismatch { catalog, manifest } => {
            ColdRefusal::CurrentMismatch { catalog, manifest }
        }
        DurableAdmissionErrorV1::NonHistoricalState {
            current_generation,
            retained_generation,
        } => ColdRefusal::NonHistorical {
            current_generation,
            retained_generation,
        },
        DurableAdmissionErrorV1::InventoryMismatch {
            kind,
            declared,
            computed,
        } => ColdRefusal::Inventory {
            kind,
            declared,
            computed,
        },
        DurableAdmissionErrorV1::RetentionWriterUnreadable { writer, reader } => {
            ColdRefusal::WriterUnreadable { writer, reader }
        }
        error => ColdRefusal::Unclassified(format!("{error:?}")),
    }
}

type AdmittedWriter = SemanticallyAdmittedDurableWriterV1<TestSemanticProfile>;

fn object(bytes: Vec<u8>) -> (ObjectRefV1, Vec<u8>) {
    let reference = ObjectRefV1::new(ObjectDigest::of_canonical_bytes(&bytes), bytes.len() as u64)
        .expect("a test object carries its envelope");
    (reference, bytes)
}

fn installed(
    prefix: &str,
    source: &SyntheticCheckpointSource,
) -> (PersistenceTrialDir, CheckpointPackImage) {
    let scratch = trial_scratch(prefix);
    let image = checkpoint_image_from_source(source.clone());
    install_checkpoint_image(scratch.path(), &image);
    (scratch, image)
}

/// Cold admission under `reader`. The V1-only reader stands in for a node
/// built before the V2 retention reader, which writes V1 as well; the dual
/// reader keeps the opened directory's writer.
fn admit_with(
    root: &Path,
    reader: RetentionReader,
    recovery: DurableLimits,
    wal: LogPackDecodeLimits,
    admission: DurableAdmissionLimitsV1,
) -> Result<AdmittedWriter, ColdRefusal> {
    let directory = DurableDirectoryV1::open(root)
        .expect("open the durable trial directory")
        .with_retention_reader_for_test(reader);
    let directory = match reader {
        RetentionReader::V1Only => directory.with_retention_writer_for_test(RetentionFormat::V1),
        RetentionReader::Dual => directory,
    };
    directory
        .recover_and_admit_v1::<TestSemanticProfile>(recovery, wal, admission)
        .map_err(cold_refusal)
}

fn admit_unbounded(root: &Path, reader: RetentionReader) -> Result<AdmittedWriter, ColdRefusal> {
    admit_with(
        root,
        reader,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
}

#[track_caller]
fn admitted(root: &Path, reader: RetentionReader) -> AdmittedWriter {
    match admit_unbounded(root, reader) {
        Ok(writer) => writer,
        Err(refusal) => {
            panic!("the {reader:?} reader refused an admissible checkpoint: {refusal:?}")
        }
    }
}

#[track_caller]
fn refused(root: &Path, reader: RetentionReader) -> ColdRefusal {
    match admit_unbounded(root, reader) {
        Ok(_) => panic!("the {reader:?} reader adopted an inadmissible checkpoint"),
        Err(refusal) => refusal,
    }
}

/// Dual-reader admission with every limit unbounded but the configured
/// admission axes.
fn admit_bounded(
    root: &Path,
    configure: impl FnOnce(&mut DurableAdmissionLimitsV1),
) -> Result<AdmittedWriter, ColdRefusal> {
    let mut admission = DurableAdmissionLimitsV1::UNBOUNDED;
    configure(&mut admission);
    admit_with(root, RetentionReader::Dual, limits(), LogPackDecodeLimits::UNBOUNDED, admission)
}

#[track_caller]
fn refused_bounded(
    root: &Path,
    configure: impl FnOnce(&mut DurableAdmissionLimitsV1),
) -> ColdRefusal {
    match admit_bounded(root, configure) {
        Ok(_) => panic!("bounded admission adopted a checkpoint beyond its limits"),
        Err(refusal) => refusal,
    }
}

/// Every durable file but the advisory writer lock that admission takes.
pub(super) fn directory_snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(root)
        .expect("list the durable trial directory")
        .map(|entry| entry.expect("read a durable directory entry"))
        .filter_map(|entry| {
            let name = entry
                .file_name()
                .into_string()
                .expect("durable file names are UTF-8");
            match name.as_str() {
                "writer.lock.v1" => None,
                _ => Some((name, fs::read(entry.path()).expect("read a durable file"))),
            }
        })
        .collect()
}

/// A strict V1 reader refuses a V2 checkpoint at its descriptor domain,
/// before any object is read.
fn v1_reader_refusal() -> ColdRefusal {
    ColdRefusal::Wal(LogPackFormatError::Checkpoint(GenerationCheckpointErrorV1::Canonical(
        CanonicalFormatError::InvalidDomain {
            expected: GENERATION_CHECKPOINT_DOMAIN,
        },
    )))
}

/// The retained union of a V1 source: its DATA without the manifest.
fn retained_references(v1: &SyntheticCheckpointSource) -> Vec<ObjectRefV1> {
    let manifest = v1.descriptor.retention_manifest();
    v1.objects
        .iter()
        .map(|(reference, _)| *reference)
        .filter(|reference| *reference != manifest)
        .collect()
}

fn total_canonical_bytes(references: &[ObjectRefV1]) -> u64 {
    references
        .iter()
        .map(|reference| reference.encoded_len())
        .sum()
}

/// The report, appointed anchor, and authenticated metadata closure (digest
/// order, canonical bytes) of an admitted V2 checkpoint.
fn admitted_metadata(
    writer: AdmittedWriter,
) -> (DurableAdmissionReportV1, RetentionAnchorV2, Vec<ObjectRefV1>, u64) {
    let report = writer.report();
    let (_lease, source) = writer.into_parts();
    let admitted = source
        .retention_metadata_v2()
        .expect("a V2 checkpoint admits its metadata closure");
    (
        report,
        admitted.anchor(),
        admitted.metadata().objects().to_vec(),
        admitted.metadata().canonical_bytes(),
    )
}

/// The anchor and radix objects of `v2`: its DATA outside the retained union
/// of the V1 source it was migrated from.
fn metadata_objects(
    v1: &SyntheticCheckpointSource,
    v2: &SyntheticCheckpointSource,
) -> ((ObjectRefV1, Vec<u8>), Vec<(ObjectRefV1, Vec<u8>)>) {
    let retained = retained_references(v1).into_iter().collect::<BTreeSet<_>>();
    let anchor_reference = v2.descriptor.retention_manifest();
    let mut anchor = None;
    let mut radix = Vec::with_capacity(v2.objects.len() - retained.len());
    for (reference, bytes) in &v2.objects {
        match (*reference == anchor_reference, retained.contains(reference)) {
            (true, _) => anchor = Some((*reference, bytes.clone())),
            (false, false) => radix.push((*reference, bytes.clone())),
            (false, true) => {}
        }
    }
    (anchor.expect("a V2 source carries its anchor"), radix)
}

/// The refusal cold admission reports when `boundary` is one below its
/// demand.
fn cold_boundary_refusal(
    boundary: CheckpointResourceBoundary,
    limit: u64,
    attempted: u64,
) -> ColdRefusal {
    match boundary {
        CheckpointResourceBoundary::Durable(resource) => {
            ColdRefusal::DurableLimit(DurableLimitError::Exceeded {
                resource,
                limit,
                attempted,
            })
        }
        CheckpointResourceBoundary::Wal(resource) => {
            ColdRefusal::Wal(LogPackFormatError::ResourceLimit {
                resource: resource.resource_name(),
                limit,
                attempted,
            })
        }
        CheckpointResourceBoundary::Admission(resource) => {
            let format = CanonicalFormatError::ResourceLimit {
                resource: resource.runtime_name(),
                limit,
                attempted,
            };
            match resource {
                AdmissionBoundary::RetainedStates |
                AdmissionBoundary::AggregateRootVisits |
                AdmissionBoundary::AggregateReferences => ColdRefusal::Canonical(format),
                AdmissionBoundary::ObjectReads | AdmissionBoundary::ObjectReadBytes => {
                    ColdRefusal::ClosureReadLimit {
                        resource: resource.runtime_name(),
                        limit,
                        attempted,
                    }
                }
                AdmissionBoundary::PerStateDistinctObjects |
                AdmissionBoundary::PerStateCanonicalBytes |
                AdmissionBoundary::PerStateReferences |
                AdmissionBoundary::PerStateWorklist |
                AdmissionBoundary::UnionObjects |
                AdmissionBoundary::UnionCanonicalBytes |
                AdmissionBoundary::LogicalNodes |
                AdmissionBoundary::Endpoints |
                AdmissionBoundary::LogicalPathBytes |
                AdmissionBoundary::PayloadBytes |
                AdmissionBoundary::JoinLookups |
                AdmissionBoundary::JoinLookupSteps |
                AdmissionBoundary::SemanticWorklist => ColdRefusal::ClosureFormat(format),
            }
        }
    }
}

fn runner(cases: u32) -> TestRunner {
    TestRunner::new(ProptestConfig {
        cases,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
}

/// The verdicts of the C20/C21 inventory model on objects given as a size
/// and a membership mask over the retained union R (bit 0), the metadata
/// closure M (bit 1), and DATA (bit 2).
struct UnionVerdict {
    accepted: bool,
    mutant_accepted: bool,
    disjoint: bool,
    exact_union: bool,
    unauthenticated: bool,
}

fn union_verdict(objects: &[(u64, u8)]) -> UnionVerdict {
    let members = |bit: u8| {
        objects
            .iter()
            .enumerate()
            .filter(|(_, (_, mask))| mask & bit != 0)
            .map(|(index, (size, _))| (index, *size))
            .collect::<Vec<_>>()
    };
    let (retained, metadata, data) = (members(1), members(2), members(4));
    let bytes = |set: &[(usize, u64)]| set.iter().map(|(_, size)| size).sum::<u64>();
    let in_data = |set: &[(usize, u64)]| set.iter().all(|(index, _)| objects[*index].1 & 4 != 0);
    let disjoint = objects.iter().all(|(_, mask)| mask & 3 != 3);
    let counted = data.len() == retained.len() + metadata.len() &&
        bytes(&data) == bytes(&retained) + bytes(&metadata);
    let mutant_accepted = in_data(&retained) && in_data(&metadata) && counted;
    let mut union = retained
        .iter()
        .chain(&metadata)
        .map(|(index, _)| *index)
        .collect::<Vec<_>>();
    union.sort_unstable();
    let data_indexes = data.iter().map(|(index, _)| *index).collect::<Vec<_>>();
    UnionVerdict {
        accepted: disjoint && mutant_accepted,
        mutant_accepted,
        disjoint,
        exact_union: union == data_indexes,
        unauthenticated: objects.iter().any(|(_, mask)| *mask == 4),
    }
}

/// Executable refinement of RV-24 and of the RetentionV2Format capstones
/// `count_and_inclusion_authenticate_the_disjoint_union` (C20) and
/// `overlapping_count_mutant_accepts_an_unauthenticated_object` (C21).
#[test]
fn v2_admission_authenticates_metadata_separately_from_the_retained_union() {
    for historical in 0..=8_usize {
        let v1 = fixture_with_retained_union_cardinality(historical + 2);
        let v2 = migrate_checkpoint_source_to_v2(&v1);
        let (scratch, _) = installed("v2-metadata-union-", &v2);
        let (report, anchor, metadata, metadata_bytes) =
            admitted_metadata(admitted(scratch.path(), RetentionReader::Dual));
        let retained = retained_references(&v1);
        let retained_set = retained.iter().copied().collect::<BTreeSet<_>>();
        let data = v2
            .objects
            .iter()
            .map(|(reference, _)| *reference)
            .collect::<Vec<_>>();
        let outside_retained = data
            .iter()
            .copied()
            .filter(|reference| !retained_set.contains(reference))
            .collect::<Vec<_>>();
        assert_eq!(metadata, outside_retained, "the metadata closure is DATA outside R");
        let mut merged = Vec::with_capacity(retained.len() + metadata.len());
        merged.extend_from_slice(&retained);
        merged.extend_from_slice(&metadata);
        merged.sort_unstable();
        assert_eq!(merged, data, "DATA is the disjoint union of R and the metadata closure");
        let inventory = v2.descriptor.inventory();
        assert_eq!(independent_inventory_digest(&merged), inventory.ordered_digest());
        assert_eq!(inventory.retained_union_objects(), retained.len() as u64);
        assert_eq!(
            inventory.retained_union_canonical_bytes().get(),
            total_canonical_bytes(&retained)
        );
        assert_eq!(
            inventory.data_objects(),
            inventory.retained_union_objects() + metadata.len() as u64
        );
        assert_eq!(
            inventory.data_canonical_bytes().get(),
            inventory.retained_union_canonical_bytes().get() + metadata_bytes
        );
        assert_eq!(metadata_bytes, total_canonical_bytes(&metadata));
        assert_eq!(report.retention_metadata_objects, metadata.len() as u64);
        assert_eq!(report.retention_metadata_canonical_bytes, metadata_bytes);
        assert_eq!(report.retention_manifest_canonical_bytes, RETENTION_ANCHOR_CANONICAL_LEN);
        assert_eq!(report.retained_union, RetainedUnionClosureReportV1 {
            distinct_objects: retained.len() as u64,
            canonical_bytes: total_canonical_bytes(&retained),
        });
        assert_eq!(report.retained_states, historical as u64 + 1);
        assert_eq!(anchor.historical_count(), historical as u64);
        assert_eq!(anchor.object_ref(), v2.descriptor.retention_manifest());
        for (reference, bytes) in &v2.objects {
            match retained_set.contains(reference) {
                true => assert!(
                    CanonicalObjectView::parse(bytes).is_ok(),
                    "R holds only semantic objects"
                ),
                false => {
                    assert!(
                        bytes.starts_with(RETENTION_ANCHOR_DOMAIN) ||
                            bytes.starts_with(RETENTION_RADIX_DOMAIN)
                    );
                    assert_eq!(
                        CanonicalObjectView::parse(bytes).err(),
                        Some(CanonicalFormatError::UnknownObjectDomain),
                        "semantic readers refuse every metadata object"
                    );
                }
            }
        }
    }

    let mut accepted = 0_u32;
    let mut overlapping = 0_u32;
    for code in 0..1_u32 << 12 {
        let objects: [(u64, u8); 3] = std::array::from_fn(|slot| {
            let digit = (code >> (4 * slot)) & 0xf;
            (u64::from(digit >> 3) + 1, u8::try_from(digit & 7).expect("a three-bit mask"))
        });
        let verdict = union_verdict(&objects);
        assert_eq!(verdict.accepted, verdict.exact_union, "C20 on {objects:?}");
        assert!(
            !verdict.mutant_accepted || verdict.disjoint || verdict.unauthenticated,
            "C21 on {objects:?}"
        );
        accepted += u32::from(verdict.accepted && objects.iter().any(|(_, mask)| mask & 4 != 0));
        overlapping += u32::from(verdict.mutant_accepted && !verdict.disjoint);
    }
    assert!(accepted > 0, "the exhaustive universe accepts a nonempty DATA");
    assert!(overlapping > 0, "the exhaustive universe witnesses C21");
    let witness = union_verdict(&[(50, 0b101), (50, 0b111), (136, 0b110), (50, 0b100)]);
    assert!(witness.mutant_accepted && !witness.disjoint && witness.unauthenticated);
    assert!(!witness.accepted, "disjointness refuses the C21 witness");
    runner(512)
        .run(&prop::collection::vec((1_u64..=4, 0_u8..8), 0..=12), |objects| {
            let verdict = union_verdict(&objects);
            prop_assert_eq!(verdict.accepted, verdict.exact_union);
            prop_assert!(!verdict.mutant_accepted || verdict.disjoint || verdict.unauthenticated);
            Ok(())
        })
        .expect("the disjoint-union rule is exact and the overlap mutant is unsound");

    let v1 = checkpoint_source_fixture();
    let v2 = migrate_checkpoint_source_to_v2(&v1);
    let (scratch, _) = installed("v2-metadata-union-twin-", &v2);
    let (_, anchor, metadata, metadata_bytes) =
        admitted_metadata(admitted(scratch.path(), RetentionReader::Dual));
    let mut minimum = vec![anchor.object_ref(), empty_retention_radix_ref()];
    minimum.sort_unstable();
    assert_eq!(metadata, minimum);
    assert_eq!(
        metadata_bytes,
        RETENTION_ANCHOR_CANONICAL_LEN + EMPTY_RETENTION_RADIX_NODE.len() as u64
    );
    let retained = retained_references(&v1);
    let semantic_empty = object(
        CanonicalDagNode::empty()
            .try_canonical_bytes()
            .expect("the empty DAG node encodes"),
    )
    .0;
    assert!(retained.contains(&semantic_empty));
    let mut impostor = vec![anchor.object_ref(), semantic_empty];
    impostor.sort_unstable();
    assert_eq!(impostor.len(), metadata.len(), "the overlapping impostor matches the count");
    assert_eq!(total_canonical_bytes(&impostor), metadata_bytes, "and the canonical bytes");
    let data = v2
        .objects
        .iter()
        .map(|(reference, _)| *reference)
        .collect::<Vec<_>>();
    assert!(impostor.iter().all(|reference| data.contains(reference)));
    let unauthenticated = data
        .iter()
        .copied()
        .filter(|reference| !retained.contains(reference) && !impostor.contains(reference))
        .collect::<Vec<_>>();
    assert_eq!(unauthenticated, [empty_retention_radix_ref()]);
    assert!(
        metadata
            .iter()
            .all(|reference| !retained.contains(reference))
    );

    let v1 = fixture_with_retained_union_cardinality(4);
    let v2 = migrate_checkpoint_source_to_v2(&v1);
    let (anchor_object, mut radix_objects) = metadata_objects(&v1, &v2);
    let metadata_count = 1 + radix_objects.len() as u64;
    radix_objects.push((empty_retention_radix_ref(), EMPTY_RETENTION_RADIX_NODE.to_vec()));
    let padded = v2_source_from_objects(&v1, anchor_object, radix_objects);
    let (scratch, _) = installed("v2-metadata-padding-", &padded);
    assert_eq!(refused(scratch.path(), RetentionReader::Dual), ColdRefusal::Inventory {
        kind: InventoryMismatchKindV1::CheckpointRetentionMetadataObjects,
        declared: metadata_count + 1,
        computed: metadata_count,
    });

    let (current, historical) = v1_manifest_entries(&v1);
    let (foreign_child, _) =
        v2_source_with_radix_child(&v1, current, &historical, &[1], |_| semantic_empty);
    let (scratch, _) = installed("v2-radix-semantic-child-", &foreign_child);
    assert_eq!(
        refused(scratch.path(), RetentionReader::Dual),
        ColdRefusal::Radix(RetentionRadixRefusalV2::NotRadixDomain {
            object: semantic_empty.digest(),
        })
    );
    for longer in [false, true] {
        let (impostor, original) =
            v2_source_with_radix_child(&v1, current, &historical, &[1], |child| {
                let length = match longer {
                    true => child.encoded_len() + 1,
                    false => child.encoded_len() - 1,
                };
                ObjectRefV1::new(child.digest(), length)
                    .expect("a shifted radix length still covers its envelope")
            });
        let (scratch, _) = installed("v2-radix-length-impostor-", &impostor);
        let expected = match longer {
            true => ColdRefusal::Canonical(CanonicalFormatError::ObjectLengthMismatch {
                expected: original.encoded_len() + 1,
                actual: original.encoded_len(),
            }),
            false => ColdRefusal::Materialization {
                digest: original.digest(),
                limit: original.encoded_len() - 1,
                attempted: original.encoded_len(),
            },
        };
        assert_eq!(refused(scratch.path(), RetentionReader::Dual), expected);
    }

    let v1 = fixture_with_retained_union_cardinality(3);
    let (current, historical) = v1_manifest_entries(&v1);
    let [entry] = historical[..] else {
        panic!("a three-object retained union retains one historical state")
    };
    let alias = RetainedStateV1::new(
        StateVersionV1::new(entry.state().generation() + 1, entry.state().manifest()),
        entry.canonical_bytes(),
    )
    .expect("an alias keeps its claim");
    let (shared, leaf) = v2_source_with_radix_child(&v1, current, &[entry, alias], &[1], |leaf| {
        ObjectRefV1::new(leaf.digest(), leaf.encoded_len() + 1)
            .expect("a longer radix length covers its envelope")
    });
    let (scratch, _) = installed("v2-radix-cached-length-", &shared);
    let mismatch = ColdRefusal::Canonical(CanonicalFormatError::ObjectLengthMismatch {
        expected: leaf.encoded_len() + 1,
        actual: leaf.encoded_len(),
    });
    assert_eq!(refused(scratch.path(), RetentionReader::Dual), mismatch);
    assert_eq!(refused_bounded(scratch.path(), |limits| limits.max_object_reads = 3), mismatch);
    assert_eq!(
        refused_bounded(scratch.path(), |limits| limits.max_object_reads = 2),
        ColdRefusal::MetadataReadLimit {
            resource: "object reads",
            limit: 2,
            attempted: 3,
        }
    );

    let radix = empty_retention_radix_ref();
    let crossing = CanonicalDagNode::new(Vec::<u8>::new(), EndpointValue::Absent, vec![
        DagChild {
            edge: 0,
            target: radix,
        },
        DagChild {
            edge: 1,
            target: radix,
        },
    ])
    .expect("two ordered children form a canonical branch");
    let crossing = migrate_checkpoint_source_to_v2(&fixture_with_root(object(
        crossing
            .try_canonical_bytes()
            .expect("the crossing branch encodes"),
    )));
    let (scratch, _) = installed("v2-semantic-radix-child-", &crossing);
    assert_eq!(
        refused(scratch.path(), RetentionReader::Dual),
        ColdRefusal::ClosureFormat(CanonicalFormatError::UnknownObjectDomain)
    );

    let v1 = checkpoint_source_fixture();
    let current = RetainedStateV1::new(
        v1.descriptor.state(),
        v1.descriptor.current_state_canonical_bytes().get(),
    )
    .expect("the current state covers its manifest");
    let stray = RetentionAnchorV2::new(current, 0, empty_retention_radix_ref())
        .expect("an empty history appoints the empty radix node");
    let stray = source_with_unreachable_retained(object(
        stray
            .try_canonical_bytes()
            .expect("the stray anchor encodes"),
    ));
    let (scratch, _) = installed("v1-stray-anchor-", &stray);
    assert_eq!(refused(scratch.path(), RetentionReader::Dual), ColdRefusal::Inventory {
        kind: InventoryMismatchKindV1::CheckpointRetainedUnionObjects,
        declared: 3,
        computed: 2,
    });
    assert_eq!(
        refused(scratch.path(), RetentionReader::V1Only),
        ColdRefusal::Wal(LogPackFormatError::Canonical(CanonicalFormatError::UnknownObjectDomain))
    );
}

/// Executable refinement of RV-25: a V2 checkpoint admits exactly the state,
/// semantics, and retained union of its V1 twin, charges exactly its metadata
/// reads, and refuses each shared history defect for the same reason.
#[test]
fn v2_cold_admission_matches_v1_admission_and_charges_metadata_reads_exactly() {
    let mut sources = Vec::with_capacity(10);
    sources.push(checkpoint_source_fixture());
    sources.push(semantic_checkpoint_source_fixture());
    sources.push(historically_larger_checkpoint_source_fixture().0);
    sources.extend((2..=8).map(fixture_with_retained_union_cardinality));
    for v1 in &sources {
        let manifest_len = v1.descriptor.retention_manifest().encoded_len();
        let (v1_scratch, _) = installed("v2-twin-v1-", v1);
        let twin = admitted(v1_scratch.path(), RetentionReader::Dual).report();
        assert_eq!(
            admitted(v1_scratch.path(), RetentionReader::V1Only).report(),
            twin,
            "both readers admit a V1 checkpoint identically"
        );
        let v2 = migrate_checkpoint_source_to_v2(v1);
        let (v2_scratch, _) = installed("v2-twin-v2-", &v2);
        let (report, _, metadata, metadata_bytes) =
            admitted_metadata(admitted(v2_scratch.path(), RetentionReader::Dual));
        assert_eq!(report.startup_disposition, twin.startup_disposition);
        assert_eq!(report.current_structural, twin.current_structural);
        assert_eq!(report.current_semantics, twin.current_semantics);
        assert_eq!(report.retained_states, twin.retained_states);
        assert_eq!(report.aggregate_root_visits, twin.aggregate_root_visits);
        assert_eq!(report.aggregate_references, twin.aggregate_references);
        assert_eq!(report.retained_union, twin.retained_union);
        assert_eq!(
            (
                twin.retention_metadata_objects,
                twin.retention_metadata_canonical_bytes,
                twin.retention_manifest_canonical_bytes,
            ),
            (1, manifest_len, manifest_len)
        );
        assert_eq!(
            (
                report.retention_metadata_objects,
                report.retention_metadata_canonical_bytes,
                report.retention_manifest_canonical_bytes,
            ),
            (metadata.len() as u64, metadata_bytes, RETENTION_ANCHOR_CANONICAL_LEN)
        );
        assert_eq!(report.object_reads, twin.object_reads - 1 + metadata.len() as u64);
        assert_eq!(
            report.object_read_bytes,
            twin.object_read_bytes - manifest_len + metadata_bytes
        );
    }

    let v2 = migrate_checkpoint_source_to_v2(&fixture_with_retained_union_cardinality(5));
    let (scratch, _) = installed("v2-metadata-reads-", &v2);
    let snapshot = directory_snapshot(scratch.path());
    let (report, _, metadata, metadata_bytes) =
        admitted_metadata(admitted(scratch.path(), RetentionReader::Dual));
    let reads = metadata.len() as u64;
    let historical = report.retained_states - 1;
    assert_eq!(historical, 3);
    let first_closure_read = v2.descriptor.state().manifest().encoded_len();
    assert_eq!(
        refused_bounded(scratch.path(), |limits| limits.max_object_reads = reads - 1),
        ColdRefusal::MetadataReadLimit {
            resource: "object reads",
            limit: reads - 1,
            attempted: reads,
        }
    );
    assert_eq!(
        refused_bounded(scratch.path(), |limits| limits.max_object_reads = reads),
        ColdRefusal::ClosureReadLimit {
            resource: "object reads",
            limit: reads,
            attempted: reads + 1,
        }
    );
    assert_eq!(
        refused_bounded(scratch.path(), |limits| limits.max_object_read_bytes = metadata_bytes - 1),
        ColdRefusal::MetadataReadLimit {
            resource: "object-read bytes",
            limit: metadata_bytes - 1,
            attempted: metadata_bytes,
        }
    );
    assert_eq!(
        refused_bounded(scratch.path(), |limits| limits.max_object_read_bytes = metadata_bytes),
        ColdRefusal::ClosureReadLimit {
            resource: "object-read bytes",
            limit: metadata_bytes,
            attempted: metadata_bytes + first_closure_read,
        }
    );
    match admit_bounded(scratch.path(), |limits| limits.max_retained_states = historical + 1) {
        Ok(writer) => assert_eq!(writer.report(), report),
        Err(refusal) => panic!("the exact retained-state limit refused: {refusal:?}"),
    }
    assert_eq!(
        refused_bounded(scratch.path(), |limits| {
            limits.max_retained_states = historical;
            limits.max_object_reads = 1;
        }),
        ColdRefusal::Canonical(CanonicalFormatError::ResourceLimit {
            resource: "retained states",
            limit: historical,
            attempted: historical + 1,
        }),
        "the declared count refuses before any radix node is read"
    );
    assert_eq!(directory_snapshot(scratch.path()), snapshot);

    let v1 = fixture_with_retained_union_cardinality(4);
    let (current, historical) = v1_manifest_entries(&v1);
    let [first, second] = historical[..] else {
        panic!("a four-object retained union retains two historical states")
    };
    let generation = current.state().generation();
    let later = RetainedStateV1::new(
        StateVersionV1::new(generation + 1, first.state().manifest()),
        first.canonical_bytes(),
    )
    .expect("a later entry keeps its claim");
    let current_later = RetainedStateV1::new(
        StateVersionV1::new(generation + 1, current.state().manifest()),
        current.canonical_bytes(),
    )
    .expect("a later current state keeps its claim");
    let manifest = first.state().manifest();
    let longer_alias = RetainedStateV1::new(
        StateVersionV1::new(
            first.state().generation(),
            ObjectRefV1::new(manifest.digest(), manifest.encoded_len() + 1)
                .expect("a longer manifest length covers its envelope"),
        ),
        first.canonical_bytes() + 1,
    )
    .expect("a longer alias claims its longer manifest");
    let twins = [
        (current, vec![first, second, later], ColdRefusal::NonHistorical {
            current_generation: generation,
            retained_generation: generation + 1,
        }),
        (current_later, vec![first, second], ColdRefusal::CurrentMismatch {
            catalog: v1.descriptor.state(),
            manifest: current_later.state(),
        }),
        (
            current,
            vec![first, longer_alias],
            ColdRefusal::ClosureFormat(CanonicalFormatError::ObjectLengthMismatch {
                expected: manifest.encoded_len(),
                actual: manifest.encoded_len() + 1,
            }),
        ),
    ];
    for (twin_current, twin_history, expected) in twins {
        let manifest_bytes = RetentionManifestV1::new(twin_current, twin_history.clone())
            .expect("the V1 twin manifest is well formed")
            .try_canonical_bytes()
            .expect("the V1 twin manifest encodes");
        let formats = [
            ("V1", v1_source_with_manifest(&v1, manifest_bytes)),
            ("V2", v2_source_with_history(&v1, twin_current, &twin_history)),
        ];
        for (format, source) in formats {
            let (scratch, _) = installed("v2-parity-twin-", &source);
            assert_eq!(refused(scratch.path(), RetentionReader::Dual), expected, "{format} twin");
        }
    }

    // A V1 manifest that lists its current state cannot be encoded, so its
    // twin patches entry 0 with the current entry. Physical recovery
    // classifies only domain prefixes, so both formats reach admission, which
    // parses the appointed manifest or decodes the radix history and applies
    // the same rule.
    let mut manifest_bytes = RetentionManifestV1::new(current, vec![first])
        .expect("the V1 base manifest is well formed")
        .try_canonical_bytes()
        .expect("the V1 base manifest encodes");
    manifest_bytes.copy_within(32..88, 96);
    let (scratch, _) =
        installed("v2-parity-current-v1-", &v1_source_with_manifest(&v1, manifest_bytes));
    assert_eq!(
        refused(scratch.path(), RetentionReader::Dual),
        ColdRefusal::Canonical(CanonicalFormatError::RetentionContainsCurrent)
    );
    let (scratch, _) =
        installed("v2-parity-current-v2-", &v2_source_with_history(&v1, current, &[current]));
    assert_eq!(
        refused(scratch.path(), RetentionReader::Dual),
        ColdRefusal::Canonical(CanonicalFormatError::RetentionContainsCurrent)
    );
}

/// Executable refinement of RV-26: every cold-admission resource boundary of
/// a V2 checkpoint refuses one below its exact demand without durable
/// mutation, and the exact limits admit the same report.
#[test]
fn every_v2_cold_admission_resource_boundary_refuses_one_below_and_accepts_exactly() {
    let v2 = migrate_checkpoint_source_to_v2(&semantic_checkpoint_source_fixture());
    let (scratch, image) = installed("v2-cold-resource-", &v2);
    let report = admitted(scratch.path(), RetentionReader::Dual).report();
    let snapshot = directory_snapshot(scratch.path());
    let exact = ExactCheckpointPublicationLimits::exact(
        &v2,
        image.pack.len() as u64,
        image.canonical_index.len() as u64,
        report.startup_work.wal_bytes_read(),
        report,
    );
    let (durable, wal, admission) = exact.build();
    match admit_with(scratch.path(), RetentionReader::Dual, durable, wal, admission) {
        Ok(writer) => assert_eq!(writer.report(), report),
        Err(refusal) => panic!("the exact cold limits refused: {refusal:?}"),
    }

    let mut durable_below = 0;
    let mut wal_below = 0;
    let mut admission_below = 0;
    let mut wal_zero = 0;
    for boundary in CHECKPOINT_RESOURCE_BOUNDARIES {
        let required = exact.requirement(boundary);
        let minimum = ExactCheckpointPublicationLimits::minimum(boundary);
        if required <= minimum {
            if matches!(boundary, CheckpointResourceBoundary::Wal(_)) && required == 0 {
                wal_zero += 1;
            }
            continue;
        }
        match boundary {
            CheckpointResourceBoundary::Durable(_) => durable_below += 1,
            CheckpointResourceBoundary::Wal(_) => wal_below += 1,
            CheckpointResourceBoundary::Admission(_) => admission_below += 1,
        }
        let mut insufficient = exact;
        insufficient.set(boundary, required - 1);
        let (durable, wal, admission) = insufficient.build();
        let refusal =
            match admit_with(scratch.path(), RetentionReader::Dual, durable, wal, admission) {
                Ok(_) => panic!("{boundary:?} admitted one below its demand {required}"),
                Err(refusal) => refusal,
            };
        assert_eq!(
            refusal,
            cold_boundary_refusal(boundary, required - 1, required),
            "{boundary:?}"
        );
        assert_eq!(directory_snapshot(scratch.path()), snapshot, "{boundary:?}");
    }
    assert_eq!(durable_below, 6, "all six constructible durable lower boundaries ran");
    assert_eq!(wal_below, 4, "all four positive checkpoint-WAL lower boundaries ran");
    assert_eq!(admission_below, 18, "all eighteen admission lower boundaries ran");
    assert_eq!(wal_zero, 3, "all three zero-demand checkpoint-WAL axes stayed at zero");

    let (historical_source, requirement) = historically_larger_checkpoint_source_fixture();
    let historical_v2 = migrate_checkpoint_source_to_v2(&historical_source);
    let (scratch, _) = installed("v2-cold-resource-historical-", &historical_v2);
    let current = admitted(scratch.path(), RetentionReader::Dual)
        .report()
        .current_structural;
    let snapshot = directory_snapshot(scratch.path());
    for boundary in [
        AdmissionBoundary::PerStateDistinctObjects,
        AdmissionBoundary::PerStateCanonicalBytes,
        AdmissionBoundary::PerStateReferences,
        AdmissionBoundary::PerStateWorklist,
    ] {
        let (required, current_required) = match boundary {
            AdmissionBoundary::PerStateDistinctObjects => {
                (requirement.distinct_objects, current.distinct_objects)
            }
            AdmissionBoundary::PerStateCanonicalBytes => {
                (requirement.canonical_bytes, current.canonical_bytes)
            }
            AdmissionBoundary::PerStateReferences => (requirement.references, current.references),
            AdmissionBoundary::PerStateWorklist => {
                (requirement.maximum_worklist, current.maximum_worklist)
            }
            _ => unreachable!("the table contains only per-state structural fields"),
        };
        assert!(
            required > current_required,
            "the historical fixture must distinguish {boundary:?} from current-only demand",
        );
        let refusal = refused_bounded(scratch.path(), |limits| match boundary {
            AdmissionBoundary::PerStateDistinctObjects => {
                limits.per_state_structural.max_distinct_objects = required - 1;
            }
            AdmissionBoundary::PerStateCanonicalBytes => {
                limits.per_state_structural.max_canonical_bytes = required - 1;
            }
            AdmissionBoundary::PerStateReferences => {
                limits.per_state_structural.max_references = required - 1;
            }
            AdmissionBoundary::PerStateWorklist => {
                limits.per_state_structural.max_worklist = required - 1;
            }
            _ => unreachable!("the table contains only per-state structural fields"),
        });
        assert_eq!(
            refusal,
            cold_boundary_refusal(
                CheckpointResourceBoundary::Admission(boundary),
                required - 1,
                required
            ),
            "historical maximum {boundary:?}"
        );
        assert_eq!(directory_snapshot(scratch.path()), snapshot);
    }
}

/// A V1 genesis store with the V2 checkpoint successor of `v2` written beside
/// it but not yet appointed: the old catalog image and slot, and the
/// expected successor.
fn staged_v2_successor(
    prefix: &str,
    v2: &SyntheticCheckpointSource,
) -> (PersistenceTrialDir, [u8; CATALOG_IMAGE_LEN], CatalogSlotV1, ExpectedSuccessor) {
    let (scratch, _) = installed(prefix, &checkpoint_source_fixture());
    let writer = admitted(scratch.path(), RetentionReader::Dual);
    let old_image = *writer.authority().image();
    let old_slot = writer.authority().slot();
    let expected = expected_successor_from_source(writer.authority(), v2);
    drop(writer);
    write_owner_only(
        &scratch.path().join(pack_file_name(
            PackId::GENESIS
                .successor()
                .expect("the genesis pack has a successor"),
        )),
        &expected.pack,
    );
    write_owner_only(
        &scratch.path().join(index_file_name(
            IndexGenerationV1::FIRST
                .successor()
                .expect("the first index generation has a successor"),
        )),
        &expected.index,
    );
    (scratch, old_image, old_slot, expected)
}

/// Executable refinement of RV-27: strict V1 readers refuse every V2 restart
/// fixture before adoption and without durable mutation, while dual readers
/// adopt the valid ones and refuse each corruption for its reason.
#[test]
fn v2_restart_fixtures_are_refused_by_old_readers_and_adopted_by_dual_readers() {
    for (cardinality, retained_states) in [(2, 1), (3, 2), (8, 7)] {
        let v1 = fixture_with_retained_union_cardinality(cardinality);
        let v2 = migrate_checkpoint_source_to_v2(&v1);
        let (scratch, _) = installed("v2-restart-migrated-", &v2);
        let snapshot = directory_snapshot(scratch.path());
        assert_eq!(refused(scratch.path(), RetentionReader::V1Only), v1_reader_refusal());
        assert_eq!(directory_snapshot(scratch.path()), snapshot);
        let (report, _, metadata, metadata_bytes) =
            admitted_metadata(admitted(scratch.path(), RetentionReader::Dual));
        assert_eq!(report.retained_states, retained_states);
        let (_, radix) = metadata_objects(&v1, &v2);
        assert_eq!(metadata.len(), radix.len() + 1);
        if retained_states == 1 {
            assert_eq!(
                (report.retention_metadata_objects, metadata_bytes),
                (2, RETENTION_ANCHOR_CANONICAL_LEN + EMPTY_RETENTION_RADIX_NODE.len() as u64)
            );
        }
    }

    let v1 = fixture_with_retained_union_cardinality(5);
    let (current, historical) = v1_manifest_entries(&v1);
    let mut product = Vec::with_capacity(3 * historical.len());
    for generation in 1..=3 {
        for entry in &historical {
            product.push(
                RetainedStateV1::new(
                    StateVersionV1::new(generation, entry.state().manifest()),
                    entry.canonical_bytes(),
                )
                .expect("a product entry keeps its claim"),
            );
        }
    }
    product.sort_unstable_by_key(|entry| (entry.state().generation(), entry.state().manifest()));
    let manifest_bytes = RetentionManifestV1::new(current, product.clone())
        .expect("the product history is strictly ordered")
        .try_canonical_bytes()
        .expect("the product manifest encodes");
    let (v1_scratch, _) =
        installed("v2-restart-product-v1-", &v1_source_with_manifest(&v1, manifest_bytes));
    let twin = admitted(v1_scratch.path(), RetentionReader::V1Only).report();
    assert_eq!(admitted(v1_scratch.path(), RetentionReader::Dual).report(), twin);
    let (scratch, _) =
        installed("v2-restart-product-", &v2_source_with_history(&v1, current, &product));
    assert_eq!(refused(scratch.path(), RetentionReader::V1Only), v1_reader_refusal());
    let (report, _, metadata, metadata_bytes) =
        admitted_metadata(admitted(scratch.path(), RetentionReader::Dual));
    assert_eq!(report.retained_states, 10);
    assert_eq!(report.retained_union, twin.retained_union);
    assert_eq!(report.aggregate_root_visits, twin.aggregate_root_visits);
    assert_eq!(report.aggregate_references, twin.aggregate_references);
    assert_eq!(
        report.object_reads,
        twin.object_reads - 1 + metadata.len() as u64,
        "shared radix nodes are read once"
    );
    assert_eq!(
        report.object_read_bytes,
        twin.object_read_bytes - twin.retention_manifest_canonical_bytes + metadata_bytes
    );

    let v2 = migrate_checkpoint_source_to_v2(&checkpoint_source_fixture());
    let (scratch, old_image, old_slot, expected) = staged_v2_successor("v2-restart-torn-", &v2);
    let differing = (0..2_usize)
        .filter(|slot| {
            let extent = slot * CATALOG_SLOT_LEN..(slot + 1) * CATALOG_SLOT_LEN;
            old_image[extent.clone()] != expected.catalog[extent]
        })
        .collect::<Vec<_>>();
    let [inactive] = differing[..] else {
        panic!("a successor rewrites exactly the inactive slot: {differing:?}")
    };
    let start = inactive * CATALOG_SLOT_LEN;
    let mut torn = old_image;
    torn[start..start + CATALOG_SLOT_LEN / 2]
        .copy_from_slice(&expected.catalog[start..start + CATALOG_SLOT_LEN / 2]);
    assert_ne!(torn, old_image);
    rewrite_and_sync(&scratch.path().join("catalog.v1"), &torn);
    for reader in [RetentionReader::V1Only, RetentionReader::Dual] {
        assert_eq!(admitted(scratch.path(), reader).authority().slot(), old_slot, "{reader:?}");
    }

    let (scratch, _, _, expected) = staged_v2_successor("v2-restart-successor-", &v2);
    rewrite_and_sync(&scratch.path().join("catalog.v1"), &expected.catalog);
    let snapshot = directory_snapshot(scratch.path());
    assert_eq!(refused(scratch.path(), RetentionReader::V1Only), v1_reader_refusal());
    assert_eq!(directory_snapshot(scratch.path()), snapshot);
    assert_eq!(
        admitted(scratch.path(), RetentionReader::Dual)
            .authority()
            .slot(),
        expected.slot
    );

    let v1 = checkpoint_source_fixture();
    let ((anchor_reference, anchor_bytes), radix) = metadata_objects(&v1, &v2);
    let mut versioned = anchor_bytes.clone();
    versioned[16..24].copy_from_slice(&2_u64.to_le_bytes());
    let mut unknown = Vec::with_capacity(40);
    unknown.extend_from_slice(b"RSpaceDAG.RetX\0\0");
    unknown.extend_from_slice(&1_u64.to_le_bytes());
    unknown.extend_from_slice(&8_u64.to_le_bytes());
    unknown.extend_from_slice(&[0x5a; 8]);
    let mut with_unknown = radix.clone();
    with_unknown.push(object(unknown));
    // Physical recovery classifies the versioned anchor by its domain prefix
    // alone, so admission refuses its version when it parses the appointed
    // anchor; an unknown domain is refused physically.
    let corruptions = [
        (
            v2_source_from_objects(&v1, object(versioned), radix),
            ColdRefusal::Canonical(CanonicalFormatError::UnsupportedVersion {
                domain: "RSpace retention anchor",
                version: 2,
            }),
        ),
        (
            v2_source_from_objects(&v1, (anchor_reference, anchor_bytes), with_unknown),
            ColdRefusal::Wal(LogPackFormatError::Canonical(
                CanonicalFormatError::UnknownObjectDomain,
            )),
        ),
    ];
    for (source, expected) in corruptions {
        let (scratch, _) = installed("v2-restart-corrupt-", &source);
        let snapshot = directory_snapshot(scratch.path());
        assert_eq!(refused(scratch.path(), RetentionReader::Dual), expected);
        assert_eq!(refused(scratch.path(), RetentionReader::V1Only), v1_reader_refusal());
        assert_eq!(directory_snapshot(scratch.path()), snapshot);
    }

    // Physical recovery authenticates every stored object's digest and
    // classifies its domain prefix; complete parsing is admission's. So only
    // a defect that stores forged bytes under the original digest is refused
    // physically; every other defect reaches the radix reader, except a moved
    // current state, which the catalog comparison refuses first.
    let mut physical = BTreeSet::new();
    for (defect, source, unit) in radix_defect_sources(&fixture_with_retained_union_cardinality(6))
    {
        let forged = source.objects.iter().find_map(|(reference, bytes)| {
            let actual =
                ObjectRefV1::new(ObjectDigest::of_canonical_bytes(bytes), reference.encoded_len())
                    .expect("a stored object keeps its envelope");
            (actual != *reference).then_some(LogPackFormatError::ObjectReference {
                declared: *reference,
                actual,
            })
        });
        let expected = match (forged, defect) {
            (Some(error), _) => {
                physical.insert(defect);
                ColdRefusal::Wal(error)
            }
            (None, RadixDefect::CurrentInHistory) => {
                let anchor = source.descriptor.retention_manifest();
                let (_, bytes) = source
                    .objects
                    .iter()
                    .find(|(reference, _)| *reference == anchor)
                    .expect("a V2 source carries its anchor");
                ColdRefusal::CurrentMismatch {
                    catalog: source.descriptor.state(),
                    manifest: RetentionAnchorV2::parse(bytes)
                        .expect("the moved anchor parses")
                        .current()
                        .state(),
                }
            }
            (None, _) => match unit {
                RetentionRadixErrorV2::Refusal(refusal) => ColdRefusal::Radix(refusal),
                RetentionRadixErrorV2::Canonical(error) => ColdRefusal::Canonical(error),
                RetentionRadixErrorV2::Store(error) => {
                    panic!("the unit reader's store refused {defect:?}: {error:?}")
                }
            },
        };
        if let RadixDefect::EnvelopeVersion2 = defect {
            assert_eq!(
                expected,
                ColdRefusal::Radix(RetentionRadixRefusalV2::Body(
                    CanonicalFormatError::UnsupportedVersion {
                        domain: "RSpace retention radix node",
                        version: 2,
                    }
                ))
            );
        }
        let (scratch, _) = installed("v2-restart-radix-defect-", &source);
        let snapshot = directory_snapshot(scratch.path());
        assert_eq!(refused(scratch.path(), RetentionReader::Dual), expected, "{defect:?}");
        assert_eq!(refused(scratch.path(), RetentionReader::V1Only), v1_reader_refusal());
        assert_eq!(directory_snapshot(scratch.path()), snapshot, "{defect:?}");
    }
    assert_eq!(
        physical,
        BTreeSet::from([
            RadixDefect::LengthShift { longer: false },
            RadixDefect::LengthShift { longer: true },
            RadixDefect::ForgeDigest,
        ])
    );

    let (scratch, image) = installed("v2-restart-", &v2);
    let snapshot = directory_snapshot(scratch.path());
    let first = admitted(scratch.path(), RetentionReader::Dual);
    let (report, authority) = (first.report(), first.authority().clone());
    drop(first);
    let again = admitted(scratch.path(), RetentionReader::Dual);
    assert_eq!((again.report(), again.authority()), (report, &authority));
    drop(again);
    assert_eq!(refused(scratch.path(), RetentionReader::V1Only), v1_reader_refusal());
    assert_eq!(directory_snapshot(scratch.path()), snapshot);
    let index_path = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST));
    fs::remove_file(&index_path).expect("remove the derived index");
    let without_index = directory_snapshot(scratch.path());
    assert_eq!(refused(scratch.path(), RetentionReader::V1Only), v1_reader_refusal());
    assert_eq!(
        directory_snapshot(scratch.path()),
        without_index,
        "a refused rebuild writes nothing"
    );
    assert_eq!(
        admitted(scratch.path(), RetentionReader::Dual)
            .report()
            .startup_disposition,
        IndexBindingDispositionV1::RebuiltAndVerified
    );
    assert_eq!(fs::read(&index_path).expect("read the rebuilt index"), image.canonical_index);
    assert_eq!(
        admitted(scratch.path(), RetentionReader::Dual)
            .report()
            .startup_disposition,
        IndexBindingDispositionV1::VerifiedExisting
    );
}

type CheckpointPublisher =
    fn(AdmittedWriter) -> Result<AdmittedWriter, GenerationCheckpointPublicationErrorV1>;

/// Executable refinement of RV-28: under the V1 writer, every successor path
/// of an admitted V2 source refuses before durable mutation.
#[test]
fn v2_admitted_sources_refuse_every_successor_before_durable_mutation() {
    let v2 = migrate_checkpoint_source_to_v2(&checkpoint_source_fixture());
    let (scratch, _) = installed("v2-successor-refusal-", &v2);
    write_owner_only(
        &scratch.path().join(pack_file_name(
            PackId::GENESIS
                .successor()
                .expect("the genesis pack has a successor"),
        )),
        &[0x53; 23],
    );
    write_owner_only(
        &scratch.path().join(index_file_name(
            IndexGenerationV1::FIRST
                .successor()
                .expect("the first index generation has a successor"),
        )),
        &[0x64; 29],
    );
    let snapshot = directory_snapshot(scratch.path());
    let baseline = admitted_writing(scratch.path(), RetentionFormat::V1);
    let (report, authority) = (baseline.report(), baseline.authority().clone());
    drop(baseline);

    let not_writable =
        GenerationCheckpointErrorV1::SourceRetentionFormatNotWritable(RetentionFormat::V2);
    let publishers: [(&str, CheckpointPublisher); 3] = [
        ("writer publication", |writer| {
            writer.publish_generation_checkpoint_v1(
                limits(),
                LogPackDecodeLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
        }),
        ("writer compaction", |writer| {
            writer.compact_generation_v1(
                limits(),
                LogPackDecodeLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
        }),
        ("publication authority", |writer| {
            writer
                .into_checkpoint_publication()
                .publish_generation_checkpoint_v1(
                    limits(),
                    LogPackDecodeLimits::UNBOUNDED,
                    DurableAdmissionLimitsV1::UNBOUNDED,
                )
        }),
    ];
    for (name, publisher) in publishers {
        let writer = admitted_writing(scratch.path(), RetentionFormat::V1);
        start_generation_checkpoint_publication_trace();
        let error = publication_error(publisher(writer));
        let trace = take_generation_checkpoint_publication_trace();
        assert!(
            matches!(
                &error,
                GenerationCheckpointPublicationErrorV1::LogPack(format)
                    if **format == LogPackFormatError::Checkpoint(not_writable.clone())
            ),
            "{name}: {error:?}"
        );
        assert_eq!(
            error.publication_outcome(),
            GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished,
            "{name}"
        );
        assert!(trace.is_empty(), "{name} reached a publication phase: {trace:?}");
        assert_eq!(directory_snapshot(scratch.path()), snapshot, "{name}");
    }

    let writer = admitted_writing(scratch.path(), RetentionFormat::V1);
    let mutation = data_batch(writer.authority().slot().state(), "v2-successor");
    assert!(matches!(
        writer.seal_transaction(
            MutationBatchViewV1::parse(&mutation).expect("the successor mutation parses"),
            DagTransactionLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        ),
        Err(DurableTransactionPreparationError::RetentionFormatNotWritable(RetentionFormat::V2))
    ));
    let (lease, source) = writer.into_parts();
    let sealed = source
        .open_transaction(
            MutationBatchViewV1::parse(&mutation).expect("the successor mutation parses"),
            DagTransactionLimits::UNBOUNDED,
        )
        .and_then(OpenDagTransaction::seal)
        .expect("a V2 source seals an in-memory transaction");
    assert!(matches!(
        source.bind_durable_transaction(&lease, sealed, DurableAdmissionLimitsV1::UNBOUNDED),
        Err(DurableTransactionPreparationError::RetentionFormatNotWritable(RetentionFormat::V2))
    ));
    assert_eq!(source.checkpoint_descriptor().err(), Some(not_writable.clone()));
    let mut visits = 0_u64;
    let visited = source.try_visit_checkpoint_references(|_| {
        visits += 1;
        Ok::<(), Infallible>(())
    });
    assert!(matches!(
        visited,
        Err(CheckpointReferenceVisitErrorV1::Checkpoint(ref error)) if *error == not_writable
    ));
    assert_eq!(visits, 0, "a V2 source visits no checkpoint reference");
    drop((lease, source));
    assert_eq!(directory_snapshot(scratch.path()), snapshot);

    assert_v2_owner_checkpoint_refuses_before_durable_mutation();

    let readmitted = admitted_writing(scratch.path(), RetentionFormat::V1);
    assert_eq!((readmitted.report(), readmitted.authority()), (report, &authority));
    drop(readmitted);
    assert_eq!(directory_snapshot(scratch.path()), snapshot);
}

/// Cold admission by a directory whose writer seals retention in `writer`.
fn admit_writing(
    root: &Path,
    writer: RetentionFormat,
    reader: RetentionReader,
) -> Result<AdmittedWriter, ColdRefusal> {
    DurableDirectoryV1::open(root)
        .expect("open the durable trial directory")
        .with_retention_reader_for_test(reader)
        .with_retention_writer_for_test(writer)
        .recover_and_admit_v1::<TestSemanticProfile>(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .map_err(cold_refusal)
}

#[track_caller]
fn admitted_writing(root: &Path, writer: RetentionFormat) -> AdmittedWriter {
    match admit_writing(root, writer, RetentionReader::Dual) {
        Ok(admitted) => admitted,
        Err(refusal) => {
            panic!("the {writer:?} writer refused an admissible checkpoint: {refusal:?}")
        }
    }
}

#[track_caller]
fn admitted_v2_writer(root: &Path) -> AdmittedWriter { admitted_writing(root, RetentionFormat::V2) }

/// The retention entries a directory must appoint, kept without the editor:
/// the current state and the key-ordered history.
struct RetentionOracle {
    current: RetainedStateV1,
    history: History,
}

impl RetentionOracle {
    fn of(v1: &SyntheticCheckpointSource) -> Self {
        let (current, historical) = v1_manifest_entries(v1);
        let history = historical
            .iter()
            .map(|entry| (model_key(entry.state()), entry.canonical_bytes()))
            .collect::<History>();
        assert_eq!(history.len(), historical.len(), "a V1 history has distinct keys");
        Self { current, history }
    }

    /// A successor moves the current state into the history.
    fn commit(&mut self, target: RetainedStateV1) {
        let predecessor = std::mem::replace(&mut self.current, target);
        assert_eq!(
            self.history
                .insert(model_key(predecessor.state()), predecessor.canonical_bytes()),
            None,
            "a successor's predecessor is not yet historical"
        );
    }

    /// The appointed anchor and every object of its metadata closure.
    fn metadata(&self) -> (RetentionAnchorV2, Vec<(ObjectRefV1, Vec<u8>)>) {
        let image = build_radix(&self.history);
        let anchor = RetentionAnchorV2::new(self.current, self.history.len() as u64, image.root)
            .expect("the oracle anchor appoints its radix root");
        let objects = metadata_closure(anchor, &image)
            .expect("the oracle radix image is canonical")
            .into_values()
            .map(|reference| match image.objects.get(&reference.digest()) {
                Some(bytes) => (reference, bytes.clone()),
                None => {
                    assert_eq!(
                        reference,
                        anchor.object_ref(),
                        "the metadata closure is the anchor and its radix image"
                    );
                    (reference, encoded(&anchor))
                }
            })
            .collect();
        (anchor, objects)
    }
}

/// `writer` appoints the oracle's anchor, admitted exactly the oracle's
/// metadata closure, and stores every object of it.
#[track_caller]
fn assert_appoints(writer: &AdmittedWriter, oracle: &RetentionOracle) {
    let (anchor, objects) = oracle.metadata();
    let count = oracle.history.len() as u64;
    assert_eq!(writer.catalog().pack().retention_manifest(), anchor.object_ref());
    assert_eq!(
        writer.retention_manifest_bytes(),
        model_anchor_bytes(oracle.current, count, anchor.historical_root())
    );
    let report = writer.report();
    assert_eq!(report.retained_states, count + 1);
    assert_eq!(report.retention_manifest_canonical_bytes, RETENTION_ANCHOR_CANONICAL_LEN);
    assert_eq!(
        (report.retention_metadata_objects, report.retention_metadata_canonical_bytes),
        (
            objects.len() as u64,
            objects
                .iter()
                .map(|(reference, _)| reference.encoded_len())
                .sum::<u64>()
        )
    );
    assert_admitted_objects(writer, &objects);
}

/// Seal, prepare and publish one data successor. The returned writer is the
/// published successor as cold admission adopted it.
pub(super) fn publish_data_successor(writer: AdmittedWriter, identity: &str) -> AdmittedWriter {
    let mutation = data_batch(writer.catalog().state(), identity);
    let sealed = writer
        .seal_transaction(
            MutationBatchViewV1::parse(&mutation).expect("the successor mutation parses"),
            DagTransactionLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("the writer seals a data successor");
    let sequence = writer
        .catalog()
        .commit_sequence()
        .successor()
        .expect("the commit sequence has a successor");
    let prepared = writer
        .prepare_transaction(&sealed, sequence, LogPackDecodeLimits::UNBOUNDED)
        .expect("the sealed successor prepares");
    let published = writer
        .publish_incremental_transaction_v1(
            &prepared,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("the prepared successor publishes");
    assert_eq!(published.catalog().state(), sealed.receipt().target());
    published
}

/// Publish a data successor and advance the oracle by the retention entry
/// step; the target claims the closure bytes that cold admission computed.
fn publish_and_track(
    writer: AdmittedWriter,
    oracle: &mut RetentionOracle,
    identity: &str,
) -> AdmittedWriter {
    let published = publish_data_successor(writer, identity);
    oracle.commit(
        RetainedStateV1::new(
            published.catalog().state(),
            published.report().current_structural.canonical_bytes,
        )
        .expect("the current closure covers its manifest"),
    );
    assert_appoints(&published, oracle);
    published
}

/// Drop `writer` and restart its directory. The V1-only reader refuses with
/// `old_reader` under either writer and writes nothing; the V2 writer under
/// the dual reader then adopts exactly what `writer` admitted, apart from how
/// recovery bound the index.
#[track_caller]
fn restart_v2_writer(
    root: &Path,
    writer: AdmittedWriter,
    old_reader: ColdRefusal,
) -> AdmittedWriter {
    let (report, authority) = (writer.report(), writer.authority().clone());
    let retention = writer.retention_manifest_bytes().to_vec();
    drop(writer);
    let snapshot = directory_snapshot(root);
    assert_eq!(refused(root, RetentionReader::V1Only), old_reader);
    assert_eq!(
        admit_writing(root, RetentionFormat::V2, RetentionReader::V1Only).err(),
        Some(old_reader)
    );
    assert_eq!(directory_snapshot(root), snapshot, "a refused restart writes nothing");
    let restarted = admitted_v2_writer(root);
    assert_eq!(restarted.authority(), &authority);
    assert_eq!(restarted.retention_manifest_bytes(), retention);
    assert_eq!(
        DurableAdmissionReportV1 {
            startup_disposition: report.startup_disposition,
            startup_work: report.startup_work,
            ..restarted.report()
        },
        report
    );
    restarted
}

/// Executable refinement of RV-33 and RV-34 at the durable directory: each
/// successor the V2 writer publishes appoints the anchor of the retention
/// entry step, and cold admission adopts exactly the oracle's metadata
/// closure. The writer survives publication and restart, and the V1-only
/// reader refuses the directory at its V2 descriptor without changing it.
#[test]
fn v2_writer_successors_appoint_the_retention_entry_step_across_restarts() {
    let v1 = checkpoint_source_fixture();
    let (scratch, _) = installed("v2-writer-successors-", &migrate_checkpoint_source_to_v2(&v1));
    let mut oracle = RetentionOracle::of(&v1);
    let writer = admitted_v2_writer(scratch.path());
    assert_appoints(&writer, &oracle);
    let writer = publish_and_track(writer, &mut oracle, "v2-writer-first");
    let writer = publish_and_track(writer, &mut oracle, "v2-writer-second");
    let writer = restart_v2_writer(scratch.path(), writer, v1_reader_refusal());
    assert_appoints(&writer, &oracle);
    let writer = publish_and_track(writer, &mut oracle, "v2-writer-third");
    assert_eq!(oracle.history.len(), v1_manifest_entries(&v1).1.len() + 3);
    drop(restart_v2_writer(scratch.path(), writer, v1_reader_refusal()));
}

/// Executable refinement of RV-37 at the durable directory: the first
/// successor the V2 writer publishes over a V1 source migrates its history
/// together with its current state, and the next one edits the migrated
/// radix. The V1-only reader refuses the migration transaction in recovery;
/// the V1 writer adopts the migrated directory as the V2 writer does but
/// refuses to extend it.
#[test]
fn v2_writer_migrates_a_v1_source_at_its_first_successor() {
    let v1 = checkpoint_source_fixture();
    let (scratch, _) = installed("v2-writer-migration-", &v1);
    let mut oracle = RetentionOracle::of(&v1);
    let writer = admitted_v2_writer(scratch.path());
    assert_eq!(
        writer.catalog().pack().retention_manifest(),
        v1.descriptor.retention_manifest(),
        "the V2 writer adopts a V1 source unchanged"
    );
    let writer = publish_and_track(writer, &mut oracle, "v2-writer-migration");
    let writer = publish_and_track(writer, &mut oracle, "v2-writer-after-migration");
    let writer = restart_v2_writer(
        scratch.path(),
        writer,
        ColdRefusal::Wal(LogPackFormatError::Canonical(CanonicalFormatError::UnknownObjectDomain)),
    );
    let (report, authority) = (writer.report(), writer.authority().clone());
    drop(writer);

    let snapshot = directory_snapshot(scratch.path());
    let v1_writer = admitted_writing(scratch.path(), RetentionFormat::V1);
    assert_eq!((v1_writer.report(), v1_writer.authority()), (report, &authority));
    let mutation = data_batch(authority.slot().state(), "v1-writer-after-migration");
    assert!(matches!(
        v1_writer.seal_transaction(
            MutationBatchViewV1::parse(&mutation).expect("the successor mutation parses"),
            DagTransactionLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        ),
        Err(DurableTransactionPreparationError::RetentionFormatNotWritable(RetentionFormat::V2))
    ));
    drop(v1_writer);
    assert_eq!(directory_snapshot(scratch.path()), snapshot);
}

/// Executable refinement of RV-08 and RV-24 for the production checkpoint
/// builder: under the V2 writer, the writer's publication, the writer's
/// compaction and the publication authority each publish a generation
/// checkpoint of a V2 source whose DATA is the retained union plus the
/// metadata closure. With the pack that held the edited radix nodes
/// reclaimed, restart still adopts the oracle's anchor and closure. A V1
/// source keeps its V1 checkpoint under the V2 writer until its first
/// successor.
#[test]
fn v2_writer_checkpoints_carry_the_retained_union_and_the_metadata_closure() {
    let v1 = checkpoint_source_fixture();
    let (scratch, _) = installed("v2-writer-checkpoint-", &migrate_checkpoint_source_to_v2(&v1));
    let mut oracle = RetentionOracle::of(&v1);
    let mut writer = publish_and_track(
        admitted_v2_writer(scratch.path()),
        &mut oracle,
        "v2-writer-before-checkpoint",
    );
    let edited_pack = scratch.path().join(pack_file_name(
        PackId::new(writer.catalog().pack().active_pack()).expect("the active pack is live"),
    ));
    assert!(edited_pack.exists());
    let publishers: [(&str, CheckpointPublisher); 3] = [
        ("writer publication", |writer| {
            writer.publish_generation_checkpoint_v1(
                limits(),
                LogPackDecodeLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
        }),
        ("writer compaction", |writer| {
            writer.compact_generation_v1(
                limits(),
                LogPackDecodeLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
        }),
        ("publication authority", |writer| {
            writer
                .into_checkpoint_publication()
                .publish_generation_checkpoint_v1(
                    limits(),
                    LogPackDecodeLimits::UNBOUNDED,
                    DurableAdmissionLimitsV1::UNBOUNDED,
                )
        }),
    ];
    for (name, publisher) in publishers {
        let (state, retained_union) = (writer.catalog().state(), writer.report().retained_union);
        let published = match publisher(writer) {
            Ok(published) => published,
            Err(error) => panic!("{name} refused under the V2 writer: {error:?}"),
        };
        assert_eq!(published.catalog().state(), state, "{name}");
        assert_eq!(published.report().retained_union, retained_union, "{name}");
        assert_appoints(&published, &oracle);
        assert!(!edited_pack.exists(), "{name} kept the pack of the edited radix nodes");
        writer = restart_v2_writer(scratch.path(), published, v1_reader_refusal());
    }
    drop(writer);

    let (scratch, _) = installed("v2-writer-v1-checkpoint-", &v1);
    let writer = admitted_v2_writer(scratch.path());
    let (state, retention) = (writer.catalog().state(), writer.retention_manifest_bytes().to_vec());
    let published = publish(writer).expect("the V2 writer checkpoints a V1 source");
    assert_eq!(published.catalog().state(), state);
    assert_eq!(published.retention_manifest_bytes(), retention);
    drop(published);
    let v1_reader = admitted(scratch.path(), RetentionReader::V1Only);
    assert_eq!(v1_reader.retention_manifest_bytes(), retention);
}

/// The V2 writer must read back what it writes, so it requires the dual
/// reader: with the V1-only reader, a V1 directory is refused after physical
/// recovery and before the store is pinned, and a V2 directory is refused in
/// recovery as before; neither refusal changes the directory. The V1 writer
/// works under both readers, and opened directories pair the V2 writer with
/// the dual reader.
#[test]
fn v2_writer_requires_the_dual_reader_and_is_the_default() {
    let v1 = checkpoint_source_fixture();
    let (scratch, _) = installed("v2-writer-v1-only-", &v1);
    let opened =
        DurableDirectoryV1::open(scratch.path()).expect("open the durable trial directory");
    assert_eq!(
        (opened.retention_writer(), opened.retention_reader()),
        (RetentionFormat::V2, RetentionReader::Dual)
    );
    drop(opened);
    let snapshot = directory_snapshot(scratch.path());
    assert_eq!(
        admit_writing(scratch.path(), RetentionFormat::V2, RetentionReader::V1Only).err(),
        Some(ColdRefusal::WriterUnreadable {
            writer: RetentionFormat::V2,
            reader: RetentionReader::V1Only,
        })
    );
    assert_eq!(directory_snapshot(scratch.path()), snapshot);
    for reader in [RetentionReader::V1Only, RetentionReader::Dual] {
        assert_eq!(admit_writing(scratch.path(), RetentionFormat::V1, reader).err(), None);
    }
    assert_eq!(directory_snapshot(scratch.path()), snapshot);

    let (scratch, _) = installed("v2-writer-v2-v1-only-", &migrate_checkpoint_source_to_v2(&v1));
    let snapshot = directory_snapshot(scratch.path());
    assert_eq!(
        admit_writing(scratch.path(), RetentionFormat::V2, RetentionReader::V1Only).err(),
        Some(v1_reader_refusal())
    );
    assert_eq!(directory_snapshot(scratch.path()), snapshot);
}
