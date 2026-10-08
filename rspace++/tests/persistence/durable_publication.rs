use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use proptest::prelude::*;
use shared::rust::test_scratch;

use super::checkpoint_wal::{
    SyntheticCheckpointSource, fixture as checkpoint_source_fixture,
    historically_larger_semantic_fixture as historically_larger_checkpoint_source_fixture,
    semantic_fixture as semantic_checkpoint_source_fixture,
};
use super::durable_recovery::{
    CheckpointPackImage, catalog_image, checkpoint_image_from_source, limits,
    valid_checkpoint_image,
};
use super::support::{TestSemanticProfile, trial_scratch};
use super::transaction_sealing::data_batch;
use crate::rspace::persistence::checkpoint::{
    CheckpointObjectInventoryV1, GenerationCheckpointDescriptorV1, OrderedObjectInventoryBuilderV1,
};
use crate::rspace::persistence::closure::RetainedUnionClosureReportV1;
use crate::rspace::persistence::durable::{
    DurableAdmissionErrorV1, DurableAdmissionLimitKindV1, DurableAdmissionLimitsV1,
    DurableAdmissionReportV1, DurableDirectoryV1, DurableIoError, DurableIoFailpointV1,
    DurableStartupErrorV1, GenerationCheckpointPublicationErrorV1,
    GenerationCheckpointPublicationHookPointV1, GenerationCheckpointPublicationOutcomeV1,
    IndexBindingDispositionV1, InventoryMismatchKindV1, PreparedGenerationCheckpointIndexV1,
    arm_catalog_slot_write_prefix, arm_durable_io_failpoint,
    arm_generation_checkpoint_publication_failpoint, arm_generation_checkpoint_publication_hook,
    index_file_name, pack_file_name, start_generation_checkpoint_publication_trace,
    take_generation_checkpoint_publication_trace,
};
use crate::rspace::persistence::wal::{
    GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1, GenerationCheckpointCommitRecordV1,
    PreparedGenerationCheckpointV1,
};
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, CanonicalBlob, CanonicalBlobKind, CanonicalEncode, CatalogAuthorityTokenV1,
    CatalogPackStateV1, CatalogSlotV1, DurableLimitConfig, DurableLimitError, DurableLimitKind,
    DurableLimits, IndexGenerationV1, LogPackAddress, LogPackDecodeLimits, LogPackFormatError,
    ObjectDigest, ObjectIndexHeaderV1, ObjectRefV1, PACK_HEADER_LEN_V1, PackHeaderV1, PackId,
    PackTerminalIdentityV1, SemanticValidationLimits, StructuralValidationLimits,
};

const FORMAL_PUBLICATION_PHASE_ORDER: [GenerationCheckpointPublicationHookPointV1; 18] = [
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
    let mut source = checkpoint_source_fixture();
    let extra = encoded_blob(b"authenticated but unreachable checkpoint object");
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
            check_borrowed_incremental_crash(IncrementalCrash::Unwind(point), &expected);
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

fn check_borrowed_incremental_crash(crash: IncrementalCrash, expected: &ExpectedSuccessor) {
    use GenerationCheckpointPublicationHookPointV1 as Point;

    use crate::rspace::persistence::GenerationLeaseKeyV1;
    use crate::rspace::persistence::durable::{
        DurableTransactionPreparationError, IncrementalPublicationErrorV1, admit_physical_source,
        admit_physical_writer, publish_incremental_from_lease_v1,
        publish_incremental_with_terminal_v1, recover_physical_from_lease_v1,
        recover_physical_with_lease_v1,
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
    let sealed = writer
        .seal_transaction(
            view,
            crate::rspace::persistence::DagTransactionLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
    let target = sealed.receipt().target();
    let prepared = writer
        .prepare_transaction(
            &sealed,
            old.commit_sequence().successor().unwrap(),
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap();
    let (lease, source) = writer.into_parts();
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
            assert!(
                matches!(
                    publish_incremental_from_lease_v1(
                        &mut lease,
                        &recovered,
                        &prepared,
                        limits(),
                        LogPackDecodeLimits::UNBOUNDED,
                        DurableAdmissionLimitsV1::UNBOUNDED,
                    ),
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
            let rebound = retry_writer
                .seal_transaction(
                    view,
                    crate::rspace::persistence::DagTransactionLimits::UNBOUNDED,
                    DurableAdmissionLimitsV1::UNBOUNDED,
                )
                .unwrap();
            assert_eq!(rebound.receipt(), sealed.receipt());
            let retry_prepared = retry_writer
                .prepare_transaction(
                    &rebound,
                    old.commit_sequence().successor().unwrap(),
                    LogPackDecodeLimits::UNBOUNDED,
                )
                .unwrap();
            let (retry_lease, retry_source) = retry_writer.into_parts();
            lease = retry_lease;
            let retried = publish_incremental_from_lease_v1(
                &mut lease,
                &retry_source,
                &retry_prepared,
                limits(),
                LogPackDecodeLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
            .unwrap();
            assert_eq!(retried.authority().slot(), expected.slot, "{crash:?}");
        } else if observed == old {
            let retried = publish_incremental_from_lease_v1(
                &mut lease,
                &recovered,
                &prepared,
                limits(),
                LogPackDecodeLimits::UNBOUNDED,
                DurableAdmissionLimitsV1::UNBOUNDED,
            )
            .unwrap();
            assert_eq!(retried.authority().slot(), expected.slot, "{crash:?}");
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
        publish_incremental_with_terminal_v1(
            lease_ref.as_mut().unwrap(),
            source_ref.as_ref().unwrap(),
            &prepared,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
            |value| {
                assert!(terminal.replace(Some(value)).is_none());
            },
        )
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
        check_borrowed_incremental_crash(IncrementalCrash::Phase(point), &expected);
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
        check_borrowed_incremental_crash(IncrementalCrash::Torn(prefix), &expected);
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

fn check_incremental_inventory_history(operations: &[(u8, u8)]) {
    for source in [checkpoint_source_fixture(), semantic_checkpoint_source_fixture()] {
        check_incremental_inventory_history_from(source, operations);
    }
}

fn check_incremental_inventory_history_from(
    source: SyntheticCheckpointSource,
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
    let scratch = trial_scratch("incremental-inventory-");
    install_checkpoint_image(scratch.path(), &checkpoint_image_from_source(source));
    let mut writer = admit(scratch.path());
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
        for object in sealed.objects() {
            oracle
                .put_if_absent_or_equal(object.reference(), object.canonical_bytes_arc())
                .unwrap();
        }
        let target = sealed.receipt().target();
        let current = verify_structural_closure(&oracle, target.manifest()).unwrap();
        let retention_object = sealed
            .objects()
            .iter()
            .find(|object| Some(object.reference()) == sealed.retention_manifest())
            .unwrap();
        let retention = RetentionManifestView::parse(retention_object.canonical_bytes()).unwrap();
        assert_eq!(retention.current().state(), target);
        assert_eq!(
            retention.current().canonical_bytes(),
            current.canonical_bytes,
            "step {step}: identity {identity}, count {before} -> {after}"
        );
        let mut retained = StructuralClosureLedgerV1::new();
        for state in std::iter::once(Ok(retention.current())).chain(retention.historical()) {
            verify_structural_closure_in_ledger_with_limits(
                &oracle,
                state.unwrap().state().manifest(),
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
        writer = admit(scratch.path());
        assert_eq!(writer.catalog().state(), target);
        assert_eq!(writer.report().current_structural, current);
        assert_eq!(writer.report().retained_union, retained.retained_union_report());
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

#[test]
fn pending_reclamation_survives_incremental_epoch_and_blocks_second_checkpoint() {
    use crate::rspace::persistence::{
        DagTransactionLimits, GenerationLeaseKeyV1, MutationBatchViewV1,
    };

    let scratch = test_scratch::acquire("rspace-gc-pending-reclaim-advance-");
    install_checkpoint_image(
        scratch.path(),
        &checkpoint_image_from_source(checkpoint_source_fixture()),
    );
    let writer = admit(scratch.path());
    let old_key = GenerationLeaseKeyV1::from_catalog(writer.catalog());
    let old_pack = scratch.path().join(pack_file_name(PackId::GENESIS));
    let old_index = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST));
    let registry = writer.generation_leases();
    let pin = registry.acquire(old_key).unwrap();
    let selected = publish(writer).unwrap();
    let selected_key = GenerationLeaseKeyV1::from_catalog(selected.catalog());
    let sidecar = scratch.path().join("reclaim.v1");
    let saved_intent = fs::read(&sidecar).unwrap();

    let mutation = data_batch(selected.catalog().state(), "reclaim-epoch-advance");
    let sealed = selected
        .seal_transaction(
            MutationBatchViewV1::parse(&mutation).unwrap(),
            DagTransactionLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
    let prepared = selected
        .prepare_transaction(
            &sealed,
            selected.catalog().commit_sequence().successor().unwrap(),
            LogPackDecodeLimits::UNBOUNDED,
        )
        .unwrap();
    let advanced = selected
        .publish_incremental_transaction_v1(
            &prepared,
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .unwrap();
    let advanced_catalog = advanced.catalog();
    let advanced_key = GenerationLeaseKeyV1::from_catalog(advanced_catalog);
    assert!(advanced_key.epoch() > selected_key.epoch());
    assert_eq!(advanced_key.first_live_pack(), selected_key.first_live_pack());
    assert_eq!(advanced_key.active_pack(), selected_key.active_pack());
    assert_eq!(advanced_key.index_generation(), selected_key.index_generation());
    assert_eq!(fs::read(&sidecar).unwrap(), saved_intent);
    drop(advanced);

    let reopened = admit(scratch.path());
    assert_eq!(reopened.catalog(), advanced_catalog);
    assert_eq!(fs::read(&sidecar).unwrap(), saved_intent);
    assert!(old_pack.exists());
    assert!(old_index.exists());
    let before_catalog = fs::read(scratch.path().join("catalog.v1")).unwrap();
    let second = publication_error(publish(reopened));
    assert!(matches!(
        second,
        GenerationCheckpointPublicationErrorV1::Io(DurableIoError::ReclamationPending { .. })
    ));
    assert_eq!(fs::read(scratch.path().join("catalog.v1")).unwrap(), before_catalog);
    assert_eq!(fs::read(&sidecar).unwrap(), saved_intent);
    assert!(old_pack.exists());
    assert!(old_index.exists());

    drop(pin);
    let retry = admit(scratch.path());
    assert_eq!(retry.catalog(), advanced_catalog);
    assert!(!old_pack.exists());
    assert!(!old_index.exists());
    assert!(!sidecar.exists());
    let next = publish(retry).unwrap();
    assert!(next.catalog().pack().active_pack() > advanced_catalog.pack().active_pack());
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
        let inventory = source.descriptor.inventory();
        let pack_bytes = expected.pack.len() as u64;
        let index_bytes = expected.index.len() as u64;
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
                max_total_physical_read_bytes: pack_bytes,
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
