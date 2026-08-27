use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use shared::rust::test_scratch;

use super::durable_recovery::{catalog_image, limits, valid_two_pack_image};
use crate::rspace::persistence::durable::{
    DurableDirectoryV1, DurableIoError, DurableIoFailpointV1, DurableStartupErrorV1,
    DurableStartupHookPointV1, IndexBindingDispositionV1, arm_durable_io_failpoint,
    arm_durable_startup_hook, committed_index_constructions, index_file_name, pack_file_name,
    reset_committed_index_constructions,
};
use crate::rspace::persistence::{
    DurableLimitConfig, DurableLimitKind, DurableLimits, INDEX_HEADER_LEN_V1, INDEX_RECORD_LEN_V1,
    IndexGenerationV1, LogPackDecodeLimits, ObjectIndexHeaderV1, PackId,
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
    assert_eq!(rebuild_work.wal_scan_passes(), 3);
    assert_eq!(rebuild_work.wal_bytes_read(), 3 * rebuilt.recovered().scanned_bytes());
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
    assert_eq!(verify_work.wal_bytes_read(), verified.recovered().scanned_bytes());
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
fn three_pass_scan_budget_is_admitted_before_a_rebuild_file_is_created() {
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
        max_total_scan_bytes: one_scan.checked_mul(3).unwrap() - 1,
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
        max_total_scan_bytes: one_scan.checked_mul(3).unwrap(),
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
