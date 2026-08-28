use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use shared::rust::test_scratch;

use super::checkpoint_wal::fixture as checkpoint_source_fixture;
use super::durable_recovery::{catalog_image, limits, valid_checkpoint_image};
use super::support::TestSemanticProfile;
use crate::rspace::persistence::durable::{
    DurableAdmissionErrorV1, DurableAdmissionLimitsV1, DurableDirectoryV1, DurableIoError,
    DurableIoFailpointV1, DurableStartupErrorV1, GenerationCheckpointPublicationErrorV1,
    GenerationCheckpointPublicationHookPointV1, GenerationCheckpointPublicationOutcomeV1,
    PreparedGenerationCheckpointIndexV1, arm_durable_io_failpoint,
    arm_generation_checkpoint_publication_failpoint, arm_generation_checkpoint_publication_hook,
    index_file_name, pack_file_name,
};
use crate::rspace::persistence::wal::{
    GENERATION_CHECKPOINT_COMMIT_RECORD_LEN_V1, GenerationCheckpointCommitRecordV1,
    PreparedGenerationCheckpointV1,
};
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, CatalogAuthorityTokenV1, CatalogPackStateV1, CatalogSlotV1,
    DurableLimitConfig, DurableLimits, IndexGenerationV1, LogPackAddress, LogPackDecodeLimits,
    ObjectIndexHeaderV1, PACK_HEADER_LEN_V1, PackHeaderV1, PackId, PackTerminalIdentityV1,
};

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
    let image = valid_checkpoint_image();
    write_owner_only(root.join("catalog.v1").as_path(), &catalog_image(image.catalog));
    write_owner_only(root.join(pack_file_name(PackId::GENESIS)).as_path(), &image.pack);
    write_owner_only(
        root.join(index_file_name(IndexGenerationV1::FIRST))
            .as_path(),
        &image.canonical_index,
    );
}

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

struct ExpectedSuccessor {
    pack: Vec<u8>,
    index: Vec<u8>,
    catalog: [u8; CATALOG_IMAGE_LEN],
    slot: CatalogSlotV1,
}

fn expected_successor(authority: &CatalogAuthorityTokenV1) -> ExpectedSuccessor {
    let source = checkpoint_source_fixture();
    let current = authority.slot();
    let current_pack = PackId::new(current.pack().active_pack()).unwrap();
    let previous = PackTerminalIdentityV1::new(
        current_pack,
        current.commit_sequence(),
        current.commit_digest(),
    );
    let header = PackHeaderV1::successor(current.store_id(), previous).unwrap();
    let prepared = PreparedGenerationCheckpointV1::<TestSemanticProfile, _>::new(
        &source,
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

#[test]
fn checkpoint_publication_is_byte_exact_restart_equivalent_and_keeps_one_lease() {
    let scratch = test_scratch::acquire("rspace-durable-publication-golden-");
    install_checkpoint_store(scratch.path());
    let old_pack_path = scratch.path().join(pack_file_name(PackId::GENESIS));
    let old_index_path = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST));
    let old_pack = fs::read(&old_pack_path).unwrap();
    let old_index = fs::read(&old_index_path).unwrap();
    let writer = admit(scratch.path());
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
    assert_eq!(fs::read(old_pack_path).unwrap(), old_pack);
    assert_eq!(fs::read(old_index_path).unwrap(), old_index);

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

fn rejecting_pack_limit() -> DurableLimits {
    DurableLimits::new(DurableLimitConfig {
        max_live_packs: u64::MAX,
        max_pack_bytes: 1,
        max_transactions: u64::MAX,
        max_object_records: u64::MAX,
        max_total_scan_bytes: u64::MAX,
        max_index_bytes: u64::MAX,
        max_index_records: u64::MAX,
        max_full_byte_comparison_bytes: u64::MAX,
        max_aggregate_full_byte_comparison_bytes: u64::MAX,
    })
    .unwrap()
}

#[test]
fn resource_refusal_precedes_cleanup_and_preserves_all_old_evidence() {
    let scratch = test_scratch::acquire("rspace-durable-publication-limit-");
    install_checkpoint_store(scratch.path());
    let writer = admit(scratch.path());
    let catalog_before = fs::read(scratch.path().join("catalog.v1")).unwrap();
    let pack_path = scratch
        .path()
        .join(pack_file_name(PackId::GENESIS.successor().unwrap()));
    let index_path = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST.successor().unwrap()));
    let pack_orphan = vec![0x53; 23];
    let index_orphan = vec![0x64; 29];
    write_owner_only(&pack_path, &pack_orphan);
    write_owner_only(&index_path, &index_orphan);

    let error = publication_error(writer.publish_generation_checkpoint_v1(
        rejecting_pack_limit(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    ));
    assert_eq!(
        error.publication_outcome(),
        GenerationCheckpointPublicationOutcomeV1::DefinitelyNotPublished
    );
    assert!(matches!(error, GenerationCheckpointPublicationErrorV1::Limit(_)));
    assert_eq!(fs::read(scratch.path().join("catalog.v1")).unwrap(), catalog_before);
    assert_eq!(fs::read(pack_path).unwrap(), pack_orphan);
    assert_eq!(fs::read(index_path).unwrap(), index_orphan);
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
    arm_durable_io_failpoint(DurableIoFailpointV1::CatalogSlotWrite);
    let old_error = publication_error(publish(old_writer));
    assert_eq!(
        old_error.publication_outcome(),
        GenerationCheckpointPublicationOutcomeV1::Indeterminate
    );
    assert!(matches!(
        old_error,
        GenerationCheckpointPublicationErrorV1::CatalogCommitIndeterminate(_)
    ));
    assert_eq!(fs::read(old_case.path().join("catalog.v1")).unwrap(), old_catalog);
    let old_recovered = admit(old_case.path());
    assert_eq!(old_recovered.authority().image(), &old_catalog);
    let old_retry = publish(old_recovered).unwrap();
    assert_eq!(old_retry.authority().slot().pack().active_pack(), 2);

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
        new_error,
        GenerationCheckpointPublicationErrorV1::CatalogCommitIndeterminate(_)
    ));
    assert_eq!(fs::read(new_case.path().join("catalog.v1")).unwrap(), expected.catalog);
    let new_recovered = admit(new_case.path());
    assert_eq!(new_recovered.authority().image(), &expected.catalog);
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
