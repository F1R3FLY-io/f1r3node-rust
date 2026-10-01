use std::fs::File;
use std::os::windows::fs::{symlink_dir, symlink_file};
use std::process::Command;

use shared::rust::test_scratch;

use crate::rspace::persistence::PackId;
use crate::rspace::persistence::durable::{
    CATALOG_FILE_NAME_V1, DurableDirectoryV1, DurableIoError, DurableIoFailpointV1,
    DurableIoOperation, INDEX_HEADER_LEN_V1, IndexGenerationV1, PACK_HEADER_LEN_V1,
    WRITER_LOCK_FILE_NAME_V1, arm_durable_io_failpoint, index_file_name, pack_file_name,
};

#[test]
fn windows_writer_lease_excludes_second_writer_and_releases_on_drop() {
    let scratch = test_scratch::acquire("rspace-windows-authority-lock-");
    let first = DurableDirectoryV1::open(scratch.path())
        .expect("qualified private directory opens")
        .try_acquire_writer()
        .expect("first writer takes the lock");
    let error = DurableDirectoryV1::open(scratch.path())
        .expect("same directory opens for second writer")
        .try_acquire_writer()
        .expect_err("second writer cannot take the lock");
    assert!(matches!(error, DurableIoError::WriterLockContended {
        operation: DurableIoOperation::AcquireWriterLock,
        ..
    }));
    drop(first);
    DurableDirectoryV1::open(scratch.path())
        .expect("directory reopens after lease drop")
        .try_acquire_writer()
        .expect("dropping the lease releases its lock");
}

#[test]
fn windows_reparse_directory_cannot_become_authority() {
    let scratch = test_scratch::acquire("rspace-windows-authority-directory-reparse-");
    let target = scratch.path().join("store");
    let alias = scratch.path().join("alias");
    std::fs::create_dir(&target).expect("create ordinary target directory");
    DurableDirectoryV1::open(&target).expect("ordinary directory must be accepted first");
    symlink_dir(&target, &alias)
        .expect("create directory symlink; Windows CI needs Developer Mode or symlink privilege");
    assert!(DurableDirectoryV1::open(&alias).is_err());
}

#[test]
fn windows_reparse_and_hard_link_authorities_are_refused() {
    let scratch = test_scratch::acquire("rspace-windows-authority-identity-");
    let outside = scratch.path().join("outside");
    File::create(&outside)
        .expect("create reparse target")
        .set_len(crate::rspace::persistence::durable::CATALOG_IMAGE_LEN as u64)
        .expect("size reparse target to a valid catalog length");
    symlink_file(&outside, scratch.path().join(CATALOG_FILE_NAME_V1))
        .expect("create file symlink; Windows CI needs Developer Mode or symlink privilege");
    let directory =
        DurableDirectoryV1::open(scratch.path()).expect("qualified private directory opens");
    let error = directory
        .open_catalog()
        .expect_err("catalog reparse entry must not resolve to target bytes");
    assert!(matches!(error, DurableIoError::Io { .. } | DurableIoError::UnsafeFileType { .. }));

    let pack_path = scratch.path().join(pack_file_name(PackId::GENESIS));
    let pack = File::create(&pack_path).expect("create pack target");
    pack.set_len(PACK_HEADER_LEN_V1).expect("size pack target");
    std::fs::hard_link(&pack_path, scratch.path().join("pack-alias"))
        .expect("create same-volume hard link");
    assert!(matches!(
        directory.open_pack(PackId::GENESIS),
        Err(DurableIoError::HardLinkedFile { .. })
    ));
}

#[test]
fn windows_world_writable_directory_and_world_readable_child_are_refused() {
    let scratch = test_scratch::acquire("rspace-windows-authority-acl-");
    let opened = DurableDirectoryV1::open(scratch.path())
        .expect("qualified private directory opens before ACL mutation");
    let pack_path = scratch.path().join(pack_file_name(PackId::GENESIS));
    let pack = File::create(&pack_path).expect("create pack for ACL rejection");
    pack.set_len(PACK_HEADER_LEN_V1)
        .expect("size pack for ACL rejection");
    let output = Command::new("icacls")
        .arg(&pack_path)
        .args(["/grant", "*S-1-1-0:R"])
        .output()
        .expect("icacls is required for the Windows ACL property fixture");
    assert!(output.status.success(), "icacls child grant failed: {output:?}");
    assert!(opened.open_pack(PackId::GENESIS).is_err());

    drop(opened);
    let output = Command::new("icacls")
        .arg(scratch.path())
        .args(["/grant", "*S-1-1-0:(OI)(CI)M"])
        .output()
        .expect("icacls is required for the Windows ACL property fixture");
    assert!(output.status.success(), "icacls directory grant failed: {output:?}");
    assert!(DurableDirectoryV1::open(scratch.path()).is_err());
}

#[test]
fn windows_pinned_directory_does_not_reparse_its_old_path() {
    let scratch = test_scratch::acquire("rspace-windows-authority-pinned-");
    let original = scratch.path().join("store");
    let moved = scratch.path().join("moved");
    std::fs::create_dir(&original).expect("create first store directory");
    let directory =
        DurableDirectoryV1::open(&original).expect("qualified private first directory opens");

    std::fs::rename(&original, &moved)
        .expect("rename of pinned directory must be supported on qualified filesystem");
    std::fs::create_dir(&original).expect("create replacement at old path");
    let _lease = directory
        .try_acquire_writer()
        .expect("pinned writer lease opens");
    assert!(moved.join(WRITER_LOCK_FILE_NAME_V1).is_file());
    assert!(!original.join(WRITER_LOCK_FILE_NAME_V1).exists());
}

#[test]
fn windows_replace_failure_preserves_old_name_and_sync_failure_never_acks() {
    let scratch = test_scratch::acquire("rspace-windows-authority-replace-");
    let mut lease = DurableDirectoryV1::open(scratch.path())
        .expect("qualified private directory opens")
        .try_acquire_writer()
        .expect("writer lease opens");
    let generation = IndexGenerationV1::FIRST;
    let final_path = scratch.path().join(index_file_name(generation));
    let original = vec![0x41; INDEX_HEADER_LEN_V1 as usize];
    {
        let mut file = lease
            .create_index(generation)
            .expect("create official index");
        file.write_all_at(0, &original)
            .expect("write official index");
        file.sync_all().expect("sync official index");
    }
    lease.sync_directory().expect("sync official name");

    let replacement = vec![0x42; INDEX_HEADER_LEN_V1 as usize];
    let mut candidate = lease
        .create_rebuilt_index(generation)
        .expect("create replacement candidate");
    candidate
        .write_all_at(0, &replacement)
        .expect("write replacement");
    let sealed = candidate
        .sync_and_seal(replacement.len() as u64)
        .expect("seal replacement");
    arm_durable_io_failpoint(DurableIoFailpointV1::BeforeReplace);
    assert!(sealed.publish_replace().is_err());
    assert_eq!(std::fs::read(&final_path).expect("read old official bytes"), original);

    let mut candidate = lease
        .create_rebuilt_index(generation)
        .expect("create second replacement candidate");
    candidate
        .write_all_at(0, &replacement)
        .expect("write second replacement");
    let sealed = candidate
        .sync_and_seal(replacement.len() as u64)
        .expect("seal second replacement");
    arm_durable_io_failpoint(DurableIoFailpointV1::DirectorySync);
    assert!(sealed.publish_replace().is_err());
    assert_eq!(std::fs::read(&final_path).expect("read installed complete bytes"), replacement);
}
