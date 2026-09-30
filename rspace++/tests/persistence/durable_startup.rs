use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use proptest::prelude::*;
use shared::rust::test_scratch;

use super::durable_recovery::{
    catalog_image, limits, malformed_checkpoint_image, valid_checkpoint_image, valid_two_pack_image,
};
use super::support::{TestSemanticProfile, trial_scratch};
use crate::rspace::persistence::durable::{
    DurableAdmissionErrorV1, DurableAdmissionLimitsV1, DurableDirectoryV1, DurableIoError,
    DurableIoFailpointV1, DurableStartupErrorV1, DurableStartupHookPointV1, DurableWriterLeaseV1,
    IndexBindingDispositionV1, admit_physical_source, arm_durable_io_failpoint,
    arm_durable_startup_hook, committed_index_constructions, index_file_name, pack_file_name,
    recover_physical_from_lease_v1, recover_physical_with_lease_v1, recover_read_authority,
    reset_committed_index_constructions,
};
use crate::rspace::persistence::{
    CATALOG_IMAGE_LEN, CanonicalFormatError, CatalogAuthorityTokenV1, DurableLimitConfig,
    DurableLimitKind, DurableLimits, GenerationLeaseKeyV1, INDEX_HEADER_LEN_V1,
    INDEX_RECORD_LEN_V1, IndexGenerationV1, LogPackDecodeLimits, ObjectIndexHeaderV1, PackId,
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

fn install_valid_store(root: &Path) -> (u64, u64) {
    let image = valid_two_pack_image();
    write_owner_only(root.join("catalog.v1").as_path(), &catalog_image(image.catalog));
    write_owner_only(root.join(pack_file_name(PackId::GENESIS)).as_path(), &image.first);
    write_owner_only(
        root.join(pack_file_name(PackId::GENESIS.successor().unwrap()))
            .as_path(),
        &image.second,
    );
    (image.first.len() as u64 + image.second.len() as u64, image.transactions)
}

fn install_checkpoint_store(root: &Path) -> super::durable_recovery::CheckpointPackImage {
    let image = valid_checkpoint_image();
    write_owner_only(root.join("catalog.v1").as_path(), &catalog_image(image.catalog));
    write_owner_only(root.join(pack_file_name(PackId::GENESIS)).as_path(), &image.pack);
    image
}

fn recover(
    root: &Path,
    limits: DurableLimits,
) -> crate::rspace::persistence::durable::PhysicallyRecoveredDurableWriterV1 {
    DurableDirectoryV1::open(root)
        .unwrap()
        .recover_writer_v1(limits, LogPackDecodeLimits::UNBOUNDED)
        .unwrap()
}

fn recover_error(root: &Path, limits: DurableLimits) -> DurableStartupErrorV1 {
    match DurableDirectoryV1::open(root)
        .unwrap()
        .recover_writer_v1(limits, LogPackDecodeLimits::UNBOUNDED)
    {
        Err(error) => error,
        Ok(_) => panic!("durable startup unexpectedly succeeded"),
    }
}

#[test]
fn checkpoint_index_is_rebuilt_byte_exactly_and_verified_on_reopen() {
    let scratch = test_scratch::acquire("rspace-durable-startup-checkpoint-");
    let image = install_checkpoint_store(scratch.path());
    let index_path = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST));

    reset_committed_index_constructions();
    let rebuilt = recover(scratch.path(), limits());
    assert_eq!(committed_index_constructions(), 1);
    assert_eq!(rebuilt.recovered().disposition(), IndexBindingDispositionV1::RebuiltAndVerified,);
    assert_eq!(rebuilt.recovered().transactions(), 0);
    assert_eq!(rebuilt.recovered().checkpoints(), 1);
    assert_eq!(rebuilt.recovered().state(), image.catalog.state());
    assert_eq!(rebuilt.recovered().terminal_address(), image.commit_address);
    assert_eq!(fs::read(&index_path).unwrap(), image.canonical_index);
    let rebuild_work = rebuilt.recovered().startup_work();
    assert_eq!(rebuild_work.wal_scan_passes(), 2);
    assert_eq!(rebuild_work.wal_bytes_read(), 2 * image.pack.len() as u64);
    assert_eq!(rebuild_work.index_bytes_written(), image.canonical_index.len() as u64,);
    drop(rebuilt);

    let verified = recover(scratch.path(), limits());
    assert_eq!(verified.recovered().disposition(), IndexBindingDispositionV1::VerifiedExisting,);
    assert_eq!(verified.recovered().transactions(), 0);
    assert_eq!(verified.recovered().checkpoints(), 1);
    assert_eq!(fs::read(&index_path).unwrap(), image.canonical_index);
    let verify_work = verified.recovered().startup_work();
    assert_eq!(verify_work.wal_scan_passes(), 1);
    assert_eq!(verify_work.wal_bytes_read(), image.pack.len() as u64);
    assert_eq!(verify_work.index_bytes_written(), 0);
}

#[test]
fn restart_grade_recovery_consumes_an_existing_writer_lease_without_reacquiring_it() {
    let scratch = test_scratch::acquire("rspace-durable-startup-held-lease-");
    let image = install_checkpoint_store(scratch.path());
    let lease = DurableDirectoryV1::open(scratch.path())
        .unwrap()
        .try_acquire_writer()
        .unwrap();

    let recovered =
        recover_physical_with_lease_v1(lease, limits(), LogPackDecodeLimits::UNBOUNDED).unwrap();
    assert_eq!(recovered.recovered().state(), image.catalog.state());
    assert_eq!(recovered.recovered().checkpoints(), 1);

    let contention = match DurableDirectoryV1::open(scratch.path())
        .unwrap()
        .recover_writer_v1(limits(), LogPackDecodeLimits::UNBOUNDED)
    {
        Ok(_) => panic!("held-lease recovery released the exclusive writer lock"),
        Err(error) => error,
    };
    assert!(matches!(
        contention,
        DurableStartupErrorV1::Io(DurableIoError::WriterLockContended { .. })
    ));
    drop(recovered);

    let reopened = recover(scratch.path(), limits());
    assert_eq!(reopened.recovered().state(), image.catalog.state());
}

#[track_caller]
fn assert_writer_and_generation_are_held(
    lease: &DurableWriterLeaseV1,
    generation: GenerationLeaseKeyV1,
) {
    let reader = lease.directory().try_clone_reader().unwrap();
    assert!(reader.generation_leases().has_active(generation).unwrap());
    assert!(matches!(reader.try_acquire_writer(), Err(DurableIoError::WriterLockContended { .. })));
}

#[test]
fn borrowed_recovery_retains_writer_and_pin_across_every_rebuild_failure() {
    for failpoint in [
        DurableIoFailpointV1::CandidateWrite,
        DurableIoFailpointV1::CandidateSetLength,
        DurableIoFailpointV1::CandidateSync,
        DurableIoFailpointV1::BeforeReplace,
        DurableIoFailpointV1::AfterReplaceBeforeDirectorySync,
        DurableIoFailpointV1::DirectorySync,
    ] {
        let scratch = test_scratch::acquire("rspace-borrowed-recovery-failure-");
        let image = install_checkpoint_store(scratch.path());
        let mut lease = DurableDirectoryV1::open(scratch.path())
            .unwrap()
            .try_acquire_writer()
            .unwrap();
        let generation = GenerationLeaseKeyV1::from_catalog(image.catalog);
        let registry = lease.generation_leases();
        let pin = registry.acquire(generation).unwrap();

        arm_durable_io_failpoint(failpoint);
        let result =
            recover_physical_from_lease_v1(&mut lease, limits(), LogPackDecodeLimits::UNBOUNDED);
        assert!(matches!(result, Err(DurableStartupErrorV1::Io(DurableIoError::Io { .. }))));
        assert_writer_and_generation_are_held(&lease, generation);

        let recovered =
            recover_physical_from_lease_v1(&mut lease, limits(), LogPackDecodeLimits::UNBOUNDED)
                .unwrap();
        assert_eq!(recovered.state(), image.catalog.state());
        assert_eq!(recovered.terminal_address(), image.commit_address);
        assert_writer_and_generation_are_held(&lease, generation);
        let reader = lease.directory().try_clone_reader().unwrap();
        drop(pin);
        assert!(!reader.generation_leases().has_active(generation).unwrap());
        drop(recovered);
        drop(lease);
        drop(
            reader
                .try_acquire_writer()
                .expect("the observer must detect owner release"),
        );
    }
}

#[test]
fn borrowed_recovery_unwind_keeps_the_owners_writer_and_generation_pin() {
    let scratch = test_scratch::acquire("rspace-borrowed-recovery-unwind-parent-");
    let marker = scratch.path().join("authority-preserved");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "rspace::persistence::tests::durable_startup::borrowed_recovery_unwind_child",
            "--nocapture",
        ])
        .env("RSPACE_BORROWED_RECOVERY_UNWIND_MARKER", &marker)
        .env_remove(shared::rust::test_process::ALLOW_EXPECTED_PROBE_CORE);
    let outcome = shared::rust::test_process::run_child_probe(
        &mut command,
        std::time::Duration::from_secs(30),
    )
    .expect("run the borrowed recovery unwind child");
    match outcome {
        shared::rust::test_process::ProbeOutcome::Failed(failure) => {
            assert_eq!(failure.kind, shared::rust::test_process::ProbeFailureKind::Panicked);
            assert!(failure.stderr.contains("injected borrowed recovery unwind"));
        }
        shared::rust::test_process::ProbeOutcome::Passed => {
            panic!("the borrowed recovery unwind child did not fail")
        }
    }
    assert_eq!(fs::read(marker).unwrap(), b"authority-preserved");
}

#[test]
#[ignore = "isolated borrowed recovery unwind child"]
fn borrowed_recovery_unwind_child() {
    let Some(marker) = std::env::var_os("RSPACE_BORROWED_RECOVERY_UNWIND_MARKER") else {
        return;
    };
    struct VerifyOnDrop {
        lease: DurableWriterLeaseV1,
        generation: GenerationLeaseKeyV1,
        image: super::durable_recovery::CheckpointPackImage,
        marker: std::path::PathBuf,
    }
    impl Drop for VerifyOnDrop {
        fn drop(&mut self) {
            assert_writer_and_generation_are_held(&self.lease, self.generation);
            let recovered = recover_physical_from_lease_v1(
                &mut self.lease,
                limits(),
                LogPackDecodeLimits::UNBOUNDED,
            )
            .unwrap();
            assert_eq!(recovered.state(), self.image.catalog.state());
            assert_eq!(recovered.terminal_address(), self.image.commit_address);
            assert_writer_and_generation_are_held(&self.lease, self.generation);
            fs::write(&self.marker, b"authority-preserved").unwrap();
        }
    }
    let scratch = test_scratch::acquire("rspace-borrowed-recovery-unwind-");
    let image = install_checkpoint_store(scratch.path());
    let lease = DurableDirectoryV1::open(scratch.path())
        .unwrap()
        .try_acquire_writer()
        .unwrap();
    let generation = GenerationLeaseKeyV1::from_catalog(image.catalog);
    let _pin = lease.generation_leases().acquire(generation).unwrap();
    let mut verify = VerifyOnDrop {
        lease,
        generation,
        image,
        marker: marker.into(),
    };
    arm_durable_startup_hook(DurableStartupHookPointV1::BeforeFirstCatalogVerification, || {
        panic!("injected borrowed recovery unwind")
    });
    recover_physical_from_lease_v1(&mut verify.lease, limits(), LogPackDecodeLimits::UNBOUNDED)
        .unwrap();
}

#[test]
fn borrowed_admission_does_not_release_authority_on_a_semantic_error() {
    let scratch = test_scratch::acquire("rspace-borrowed-admission-invalid-");
    let image = malformed_checkpoint_image();
    write_owner_only(&scratch.path().join("catalog.v1"), &catalog_image(image.catalog));
    write_owner_only(&scratch.path().join(pack_file_name(PackId::GENESIS)), &image.pack);
    let mut lease = DurableDirectoryV1::open(scratch.path())
        .unwrap()
        .try_acquire_writer()
        .unwrap();
    let generation = GenerationLeaseKeyV1::from_catalog(image.catalog);
    let _pin = lease.generation_leases().acquire(generation).unwrap();

    for _ in 0..2 {
        let recovered =
            recover_physical_from_lease_v1(&mut lease, limits(), LogPackDecodeLimits::UNBOUNDED)
                .unwrap();
        let result = admit_physical_source::<TestSemanticProfile>(
            &lease,
            recovered,
            DurableAdmissionLimitsV1::UNBOUNDED,
        );
        assert!(matches!(result, Err(DurableAdmissionErrorV1::Closure(_))));
        assert_writer_and_generation_are_held(&lease, generation);
    }
}

proptest! {
    #[test]
    fn borrowed_recovery_and_admission_retry_histories_keep_exact_authority(
        physical_failures in proptest::collection::vec(any::<bool>(), 1..9),
    ) {
        let scratch = trial_scratch("borrowed-admission-history-");
        let image = install_checkpoint_store(scratch.path());
        write_owner_only(
            &scratch.path().join(index_file_name(IndexGenerationV1::FIRST)),
            &image.canonical_index,
        );
        let mut lease = DurableDirectoryV1::open(scratch.path())
            .unwrap().try_acquire_writer().unwrap();
        let generation = GenerationLeaseKeyV1::from_catalog(image.catalog);
        let _pin = lease.generation_leases().acquire(generation).unwrap();
        for physical_failure in physical_failures {
            if physical_failure {
                let result = recover_physical_from_lease_v1(
                    &mut lease, limits(), LogPackDecodeLimits {
                        max_record_bytes: 0,
                        ..LogPackDecodeLimits::UNBOUNDED
                    },
                );
                prop_assert!(result.is_err());
            } else {
                let recovered = recover_physical_from_lease_v1(
                    &mut lease, limits(), LogPackDecodeLimits::UNBOUNDED,
                ).unwrap();
                let result = admit_physical_source::<TestSemanticProfile>(
                    &lease, recovered, DurableAdmissionLimitsV1 {
                        max_retained_states: 0,
                        ..DurableAdmissionLimitsV1::UNBOUNDED
                    },
                );
                assert!(matches!(result, Err(DurableAdmissionErrorV1::Canonical(
                    CanonicalFormatError::ResourceLimit { resource: "retained states", .. }
                ))));
            }
            assert_writer_and_generation_are_held(&lease, generation);
        }
        let recovered = recover_physical_from_lease_v1(
            &mut lease, limits(), LogPackDecodeLimits::UNBOUNDED,
        ).unwrap();
        let source = admit_physical_source::<TestSemanticProfile>(
            &lease, recovered, DurableAdmissionLimitsV1::UNBOUNDED,
        ).unwrap();
        prop_assert_eq!(source.authority().slot(), image.catalog);
        prop_assert_eq!(source.report().retained_states, 1);
        assert_writer_and_generation_are_held(&lease, generation);
        prop_assert_eq!(fs::read(scratch.path().join("catalog.v1")).unwrap(), catalog_image(image.catalog));
        prop_assert_eq!(fs::read(scratch.path().join(pack_file_name(PackId::GENESIS))).unwrap(), image.pack.as_ref());
    }
}

#[test]
fn exact_catalog_authority_guards_successor_orphan_cleanup() {
    let scratch = test_scratch::acquire("rspace-durable-startup-orphan-cleanup-");
    let image = install_checkpoint_store(scratch.path());
    let catalog_bytes = catalog_image(image.catalog);
    let authority = CatalogAuthorityTokenV1::select(catalog_bytes)
        .unwrap()
        .unwrap();
    let successor_pack = PackId::GENESIS.successor().unwrap();
    let successor_index = IndexGenerationV1::FIRST.successor().unwrap();
    let successor_pack_path = scratch.path().join(pack_file_name(successor_pack));
    let successor_index_path = scratch.path().join(index_file_name(successor_index));
    write_owner_only(&successor_pack_path, &[0x11; 7]);
    write_owner_only(&successor_index_path, &[0x22; 9]);
    let current_pack_path = scratch.path().join(pack_file_name(PackId::GENESIS));
    let current_pack = fs::read(&current_pack_path).unwrap();

    let mut lease = DurableDirectoryV1::open(scratch.path())
        .unwrap()
        .try_acquire_writer()
        .unwrap();
    let prepared = lease
        .prepare_unpublished_successor_artifacts(&authority)
        .unwrap();
    assert_eq!(prepared.pack(), successor_pack);
    assert_eq!(prepared.index(), successor_index);
    assert!(prepared.removed_pack());
    assert!(prepared.removed_index());
    assert!(!successor_pack_path.exists());
    assert!(!successor_index_path.exists());
    assert_eq!(fs::read(current_pack_path).unwrap(), current_pack);

    let clean = lease
        .prepare_unpublished_successor_artifacts(&authority)
        .unwrap();
    assert!(!clean.removed_pack());
    assert!(!clean.removed_index());
}

#[test]
fn changed_catalog_prevents_any_successor_orphan_cleanup() {
    let scratch = test_scratch::acquire("rspace-durable-startup-orphan-cas-");
    let image = install_checkpoint_store(scratch.path());
    let authority = CatalogAuthorityTokenV1::select(catalog_image(image.catalog))
        .unwrap()
        .unwrap();
    let successor_pack_path = scratch
        .path()
        .join(pack_file_name(PackId::GENESIS.successor().unwrap()));
    let successor_index_path = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST.successor().unwrap()));
    write_owner_only(&successor_pack_path, &[0x33; 5]);
    write_owner_only(&successor_index_path, &[0x44; 6]);

    let catalog_path = scratch.path().join("catalog.v1");
    let mut changed = fs::read(&catalog_path).unwrap();
    changed[CATALOG_IMAGE_LEN - 1] ^= 1;
    fs::write(&catalog_path, changed).unwrap();
    #[cfg(unix)]
    fs::set_permissions(&catalog_path, fs::Permissions::from_mode(0o600)).unwrap();

    let mut lease = DurableDirectoryV1::open(scratch.path())
        .unwrap()
        .try_acquire_writer()
        .unwrap();
    let error = lease
        .prepare_unpublished_successor_artifacts(&authority)
        .unwrap_err();
    assert!(matches!(error, DurableIoError::CatalogAuthorityChanged { .. }));
    assert_eq!(fs::read(successor_pack_path).unwrap(), &[0x33; 5]);
    assert_eq!(fs::read(successor_index_path).unwrap(), &[0x44; 6]);
}

#[test]
fn checkpoint_physical_authenticity_never_bypasses_semantic_admission() {
    let scratch = test_scratch::acquire("rspace-durable-checkpoint-admission-");
    let image = malformed_checkpoint_image();
    write_owner_only(scratch.path().join("catalog.v1").as_path(), &catalog_image(image.catalog));
    write_owner_only(
        scratch
            .path()
            .join(pack_file_name(PackId::GENESIS))
            .as_path(),
        &image.pack,
    );

    let physical = recover(scratch.path(), limits());
    assert_eq!(physical.recovered().checkpoints(), 1);
    assert_eq!(physical.recovered().object_records(), image.records.len() as u64);
    drop(physical);

    let result = DurableDirectoryV1::open(scratch.path())
        .unwrap()
        .recover_and_admit_v1::<TestSemanticProfile>(
            limits(),
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        );
    assert!(matches!(result, Err(DurableAdmissionErrorV1::Closure(_))));
}

#[test]
fn read_authority_view_reopens_the_authenticated_catalog_state() {
    let scratch = test_scratch::acquire("rspace-durable-read-authority-");
    let image = install_checkpoint_store(scratch.path());

    let view = recover_read_authority::<TestSemanticProfile>(
        scratch.path(),
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("the valid checkpoint must pass restart-grade admission");

    assert_eq!(view.state(), image.catalog.state());
    assert_eq!(view.manifest(), image.catalog.state().manifest());
    assert!(view.report().current_structural.distinct_objects > 0);
    let manifest = view
        .get_object_bounded(&view.manifest().digest(), view.manifest().encoded_len())
        .expect("manifest lookup must be admitted")
        .expect("catalog manifest must be indexed");
    assert_eq!(manifest.len() as u64, view.manifest().encoded_len());
    let too_small = view.manifest().encoded_len() - 1;
    assert!(matches!(
        view.get_object_bounded(&view.manifest().digest(), too_small),
        Err(crate::rspace::persistence::durable::DurableObjectReadErrorV1::MaterializationLimit {
            limit,
            ..
        }) if limit == too_small
    ));
    let missing = crate::rspace::persistence::ObjectDigest::from_array([0xA5; 32]);
    assert!(
        view.get_object_bounded(&missing, 1024)
            .expect("an absent digest is a valid lookup result")
            .is_none()
    );

    let roots = view
        .semantic_roots()
        .expect("semantic roots must come from the admitted manifest");
    let projected = view
        .project_prefix(roots.data_bag, &[], u64::MAX, u64::MAX)
        .expect("authenticated empty-prefix projection must succeed");
    assert!(projected.is_empty(), "fixture data root is intentionally empty");

    let data = view
        .data_for_channel(&"missing-channel".to_owned(), u64::MAX, u64::MAX, u64::MAX)
        .expect("typed data projection must preserve an empty channel");
    assert!(data.is_empty());
    let continuations = view
        .continuations_for_channels(&["missing-channel".to_owned()], u64::MAX, u64::MAX, u64::MAX)
        .expect("typed continuation projection must preserve an empty channel group");
    assert!(continuations.is_empty());
    let joins = view
        .joins_for_channel(&"missing-channel".to_owned(), u64::MAX, u64::MAX, u64::MAX)
        .expect("typed join projection must preserve an empty channel");
    assert!(joins.is_empty());
}

#[test]
fn missing_index_is_rebuilt_exactly_then_mandatorily_verified_on_reopen() {
    let scratch = test_scratch::acquire("rspace-durable-startup-missing-");
    let (_, transactions) = install_valid_store(scratch.path());
    let index_path = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST));

    reset_committed_index_constructions();
    let rebuilt = recover(scratch.path(), limits());
    assert_eq!(committed_index_constructions(), 1);
    assert_eq!(rebuilt.recovered().disposition(), IndexBindingDispositionV1::RebuiltAndVerified);
    assert_eq!(rebuilt.recovered().transactions(), transactions);
    assert_eq!(
        rebuilt.recovered().object_index().record_count(),
        rebuilt.recovered().object_records()
    );
    assert_eq!(fs::metadata(&index_path).unwrap().len(), rebuilt.recovered().expected_index_tail());
    let rebuild_work = rebuilt.recovered().startup_work();
    assert_eq!(rebuild_work.wal_scan_passes(), 2);
    assert!(rebuild_work.wal_bytes_read() >= 2 * rebuilt.recovered().scanned_bytes());
    assert_eq!(rebuild_work.index_bytes_read(), rebuilt.recovered().expected_index_tail());
    assert_eq!(rebuild_work.index_bytes_written(), rebuilt.recovered().expected_index_tail());
    assert_eq!(rebuild_work.file_syncs(), 1);
    assert_eq!(rebuild_work.directory_syncs(), 1);
    let canonical = fs::read(&index_path).unwrap();

    let contention = match DurableDirectoryV1::open(scratch.path())
        .unwrap()
        .recover_writer_v1(limits(), LogPackDecodeLimits::UNBOUNDED)
    {
        Err(error) => error,
        Ok(_) => panic!("a second recovered writer acquired the live writer lease"),
    };
    assert!(matches!(
        contention,
        DurableStartupErrorV1::Io(DurableIoError::WriterLockContended { .. })
    ));
    drop(rebuilt);

    let verified = recover(scratch.path(), limits());
    assert_eq!(verified.recovered().disposition(), IndexBindingDispositionV1::VerifiedExisting);
    assert_eq!(fs::read(&index_path).unwrap(), canonical);
    let verify_work = verified.recovered().startup_work();
    assert_eq!(verify_work.wal_scan_passes(), 1);
    assert_eq!(
        rebuild_work.wal_bytes_read(),
        verify_work.wal_bytes_read().checked_mul(2).unwrap(),
        "repair performs two identical physical recovery scans, including authenticated rereads",
    );
    assert_eq!(verify_work.index_bytes_read(), verified.recovered().expected_index_tail());
    assert_eq!(verify_work.index_bytes_written(), 0);
    assert_eq!(verify_work.file_syncs(), 0);
    assert_eq!(verify_work.directory_syncs(), 0);
}

#[test]
fn every_recoverable_index_shape_is_replaced_by_the_same_canonical_bytes() {
    let scratch = test_scratch::acquire("rspace-durable-startup-shapes-");
    install_valid_store(scratch.path());
    let index_path = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST));
    let first = recover(scratch.path(), limits());
    let store = first.recovered().store_id();
    drop(first);
    let canonical = fs::read(&index_path).unwrap();
    assert!(canonical.len() >= (INDEX_HEADER_LEN_V1 + 2 * INDEX_RECORD_LEN_V1) as usize);

    let mut shapes = Vec::new();
    shapes.push(canonical[..INDEX_HEADER_LEN_V1 as usize - 1].to_vec());

    let mut suffix = canonical.clone();
    suffix.extend_from_slice(&[0xa5; 31]);
    shapes.push(suffix);

    let mut corrupt = canonical.clone();
    corrupt[INDEX_HEADER_LEN_V1 as usize] ^= 1;
    shapes.push(corrupt);

    let first_record = INDEX_HEADER_LEN_V1 as usize;
    let second_record = first_record + INDEX_RECORD_LEN_V1 as usize;
    let mut reordered = canonical.clone();
    reordered[first_record..second_record + INDEX_RECORD_LEN_V1 as usize]
        .rotate_left(INDEX_RECORD_LEN_V1 as usize);
    shapes.push(reordered);

    let mut wrong_generation = canonical.clone();
    wrong_generation[..INDEX_HEADER_LEN_V1 as usize].copy_from_slice(
        &ObjectIndexHeaderV1::new(store, IndexGenerationV1::FIRST.successor().unwrap()).encode(),
    );
    shapes.push(wrong_generation);

    for shape in shapes {
        fs::write(&index_path, shape).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&index_path, fs::Permissions::from_mode(0o600)).unwrap();
        let repaired = recover(scratch.path(), limits());
        assert_eq!(
            repaired.recovered().disposition(),
            IndexBindingDispositionV1::RebuiltAndVerified
        );
        assert_eq!(fs::read(&index_path).unwrap(), canonical);
        drop(repaired);
    }
}

#[cfg(unix)]
#[test]
fn unsafe_index_identity_is_fatal_and_never_reclassified_as_repairable_damage() {
    let scratch = test_scratch::acquire("rspace-durable-startup-unsafe-");
    install_valid_store(scratch.path());
    let index_path = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST));
    drop(recover(scratch.path(), limits()));
    let canonical = fs::read(&index_path).unwrap();
    let alias = scratch.path().join("index-hard-link");
    fs::hard_link(&index_path, &alias).unwrap();

    let error = match DurableDirectoryV1::open(scratch.path())
        .unwrap()
        .recover_writer_v1(limits(), LogPackDecodeLimits::UNBOUNDED)
    {
        Err(error) => error,
        Ok(_) => panic!("a hard-linked index was accepted as durable authority"),
    };
    assert!(matches!(
        error,
        DurableStartupErrorV1::Io(DurableIoError::HardLinkedFile { links: 2, .. })
    ));
    assert_eq!(fs::read(&index_path).unwrap(), canonical);
    fs::remove_file(alias).unwrap();
}

#[test]
fn two_pass_scan_budget_is_admitted_before_a_rebuild_file_is_created() {
    let scratch = test_scratch::acquire("rspace-durable-startup-budget-");
    let (one_scan, _) = install_valid_store(scratch.path());
    let index_name = index_file_name(IndexGenerationV1::FIRST);
    let index_path = scratch.path().join(&index_name);
    let rebuild_path = scratch.path().join(format!("{index_name}.rebuild"));
    let insufficient = DurableLimits::new(DurableLimitConfig {
        max_live_packs: 8,
        max_pack_bytes: 16 * 1024 * 1024,
        max_transactions: 1024,
        max_object_records: 65_536,
        max_total_scan_bytes: one_scan.checked_mul(2).unwrap() - 1,
        max_total_physical_read_bytes: one_scan.checked_mul(6).unwrap(),
        max_index_bytes: 16 * 1024 * 1024,
        max_index_records: 65_536,
        max_full_byte_comparison_bytes: 16 * 1024 * 1024,
        max_aggregate_full_byte_comparison_bytes: 64 * 1024 * 1024,
    })
    .unwrap();

    let error = match DurableDirectoryV1::open(scratch.path())
        .unwrap()
        .recover_writer_v1(insufficient, LogPackDecodeLimits::UNBOUNDED)
    {
        Err(error) => error,
        Ok(_) => panic!("repair exceeded its admitted aggregate scan budget"),
    };
    assert!(matches!(
        error,
        DurableStartupErrorV1::Limit(crate::rspace::persistence::DurableLimitError::Exceeded {
            resource: DurableLimitKind::TotalScanBytes,
            ..
        })
    ));
    assert!(!index_path.exists());
    assert!(!rebuild_path.exists());

    let exact = DurableLimits::new(DurableLimitConfig {
        max_live_packs: 8,
        max_pack_bytes: 16 * 1024 * 1024,
        max_transactions: 1024,
        max_object_records: 65_536,
        max_total_scan_bytes: one_scan.checked_mul(2).unwrap(),
        max_total_physical_read_bytes: one_scan.checked_mul(6).unwrap(),
        max_index_bytes: 16 * 1024 * 1024,
        max_index_records: 65_536,
        max_full_byte_comparison_bytes: 16 * 1024 * 1024,
        max_aggregate_full_byte_comparison_bytes: 64 * 1024 * 1024,
    })
    .unwrap();
    let rebuilt = recover(scratch.path(), exact);
    assert_eq!(rebuilt.recovered().disposition(), IndexBindingDispositionV1::RebuiltAndVerified);
}

#[test]
fn prepublication_io_failures_leave_the_official_index_byte_identical() {
    let scratch = test_scratch::acquire("rspace-durable-startup-prepublish-");
    install_valid_store(scratch.path());
    let index_path = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST));
    drop(recover(scratch.path(), limits()));
    let canonical = fs::read(&index_path).unwrap();
    let mut damaged = canonical.clone();
    damaged.extend_from_slice(&[0x7b; 17]);

    for failpoint in [
        DurableIoFailpointV1::CandidateWrite,
        DurableIoFailpointV1::CandidateSetLength,
        DurableIoFailpointV1::CandidateSync,
        DurableIoFailpointV1::BeforeReplace,
    ] {
        fs::write(&index_path, &damaged).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&index_path, fs::Permissions::from_mode(0o600)).unwrap();
        arm_durable_io_failpoint(failpoint);
        assert!(matches!(
            recover_error(scratch.path(), limits()),
            DurableStartupErrorV1::Io(DurableIoError::Io { .. }) |
                DurableStartupErrorV1::Recovery(
                    crate::rspace::persistence::durable::RecoveryError::Read(
                        DurableIoError::Io { .. }
                    )
                )
        ));
        assert_eq!(fs::read(&index_path).unwrap(), damaged);
    }
}

#[test]
fn postrename_sync_failures_leave_only_a_complete_recoverable_index() {
    let scratch = test_scratch::acquire("rspace-durable-startup-postrename-");
    install_valid_store(scratch.path());
    let index_path = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST));
    drop(recover(scratch.path(), limits()));
    let canonical = fs::read(&index_path).unwrap();
    let mut damaged = canonical.clone();
    damaged.extend_from_slice(&[0x8c; 23]);

    for failpoint in
        [DurableIoFailpointV1::AfterReplaceBeforeDirectorySync, DurableIoFailpointV1::DirectorySync]
    {
        fs::write(&index_path, &damaged).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&index_path, fs::Permissions::from_mode(0o600)).unwrap();
        arm_durable_io_failpoint(failpoint);
        assert!(matches!(
            recover_error(scratch.path(), limits()),
            DurableStartupErrorV1::Io(DurableIoError::Io { .. })
        ));
        assert_eq!(fs::read(&index_path).unwrap(), canonical);
        let verified = recover(scratch.path(), limits());
        assert_eq!(verified.recovered().disposition(), IndexBindingDispositionV1::VerifiedExisting);
        drop(verified);
    }
}

#[test]
fn catalog_interleavings_and_postinstall_damage_never_expose_physical_authority() {
    let scratch = test_scratch::acquire("rspace-durable-startup-interleavings-");
    let image = valid_two_pack_image();
    write_owner_only(scratch.path().join("catalog.v1").as_path(), &catalog_image(image.catalog));
    write_owner_only(
        scratch
            .path()
            .join(pack_file_name(PackId::GENESIS))
            .as_path(),
        &image.first,
    );
    write_owner_only(
        scratch
            .path()
            .join(pack_file_name(PackId::GENESIS.successor().unwrap()))
            .as_path(),
        &image.second,
    );
    let catalog_path = scratch.path().join("catalog.v1");
    let valid_catalog = catalog_image(image.catalog);
    let index_path = scratch
        .path()
        .join(index_file_name(IndexGenerationV1::FIRST));

    let mutate_catalog = |path: std::path::PathBuf, mut bytes: [u8; 672]| {
        move || {
            bytes[671] ^= 1;
            fs::write(&path, bytes).unwrap();
            #[cfg(unix)]
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
    };

    arm_durable_startup_hook(
        DurableStartupHookPointV1::BeforeFirstCatalogVerification,
        mutate_catalog(catalog_path.clone(), valid_catalog),
    );
    assert!(matches!(
        recover_error(scratch.path(), limits()),
        DurableStartupErrorV1::CatalogVerification(_)
    ));
    assert!(!index_path.exists());

    fs::write(&catalog_path, valid_catalog).unwrap();
    #[cfg(unix)]
    fs::set_permissions(&catalog_path, fs::Permissions::from_mode(0o600)).unwrap();
    arm_durable_startup_hook(
        DurableStartupHookPointV1::BeforeSecondCatalogVerification,
        mutate_catalog(catalog_path.clone(), valid_catalog),
    );
    assert!(matches!(
        recover_error(scratch.path(), limits()),
        DurableStartupErrorV1::CatalogVerification(_)
    ));
    assert!(!index_path.exists());

    fs::write(&catalog_path, valid_catalog).unwrap();
    #[cfg(unix)]
    fs::set_permissions(&catalog_path, fs::Permissions::from_mode(0o600)).unwrap();
    let corrupt_path = index_path.clone();
    arm_durable_startup_hook(
        DurableStartupHookPointV1::BeforePostInstallIndexVerification,
        move || {
            let mut bytes = fs::read(&corrupt_path).unwrap();
            bytes[INDEX_HEADER_LEN_V1 as usize] ^= 1;
            fs::write(&corrupt_path, bytes).unwrap();
            #[cfg(unix)]
            fs::set_permissions(&corrupt_path, fs::Permissions::from_mode(0o600)).unwrap();
        },
    );
    assert!(matches!(
        recover_error(scratch.path(), limits()),
        DurableStartupErrorV1::PostInstallIndexMismatch
    ));

    fs::write(&catalog_path, valid_catalog).unwrap();
    #[cfg(unix)]
    fs::set_permissions(&catalog_path, fs::Permissions::from_mode(0o600)).unwrap();
    arm_durable_startup_hook(
        DurableStartupHookPointV1::BeforeFinalCatalogVerification,
        mutate_catalog(catalog_path.clone(), valid_catalog),
    );
    assert!(matches!(
        recover_error(scratch.path(), limits()),
        DurableStartupErrorV1::CatalogVerification(_)
    ));
}
