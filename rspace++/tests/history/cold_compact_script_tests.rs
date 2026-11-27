//! End-to-end test for `scripts/rspace-cold-compact.sh` (issue #1181,
//! Phase 5). Populates a real LMDB env with sparse data, runs the
//! operator script against it, and asserts the on-disk file shrank
//! without altering the data readable from the compacted copy.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use heed::EnvOpenOptions;
use heed::types::Bytes;

fn script_path() -> PathBuf {
    // CARGO_MANIFEST_DIR points at rspace++/; the script lives in
    // repo-root/scripts/.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .expect("CARGO_MANIFEST_DIR has a parent")
        .join("scripts")
        .join("rspace-cold-compact.sh")
}

fn file_len(path: &std::path::Path) -> u64 {
    fs::metadata(path).expect("data.mdb must exist").len()
}

/// Populate a fresh env with sparse data (many writes, delete half),
/// run the compact script, and assert `data.mdb` shrinks and keys
/// survive. If `mdb_copy` is not on PATH, the test skips.
#[test]
fn compact_script_shrinks_cold_env_and_preserves_data() {
    if Command::new("mdb_copy").arg("--help").output().is_err() {
        eprintln!("skip: mdb_copy not on PATH");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let cold_dir = tmp.path().join("rspace").join("cold");
    fs::create_dir_all(&cold_dir).unwrap();
    // Insert and delete in separate transactions so the delete's
    // page releases land on the free list rather than being
    // short-circuited into an in-txn in-place rewrite.
    {
        let env = unsafe {
            EnvOpenOptions::new()
                .map_size(64 * 1024 * 1024)
                .max_dbs(1)
                .open(&cold_dir)
                .unwrap()
        };
        let mut wtxn = env.write_txn().unwrap();
        let db = env
            .create_database::<Bytes, Bytes>(&mut wtxn, Some("rspace-cold"))
            .unwrap();
        for i in 0u32..4000 {
            db.put(&mut wtxn, &i.to_le_bytes()[..], &[0u8; 1024])
                .unwrap();
        }
        wtxn.commit().unwrap();
    }
    {
        let env = unsafe {
            EnvOpenOptions::new()
                .map_size(64 * 1024 * 1024)
                .max_dbs(1)
                .open(&cold_dir)
                .unwrap()
        };
        let mut wtxn = env.write_txn().unwrap();
        let db: heed::Database<Bytes, Bytes> = env
            .open_database(&wtxn, Some("rspace-cold"))
            .unwrap()
            .unwrap();
        for i in (0u32..4000).step_by(2) {
            db.delete(&mut wtxn, &i.to_le_bytes()[..]).unwrap();
        }
        wtxn.commit().unwrap();
    }

    let data_file = cold_dir.join("data.mdb");
    let before = file_len(&data_file);
    assert!(before > 0, "fixture env must have data");

    let out = Command::new(script_path())
        .arg("--data-dir")
        .arg(tmp.path())
        .output()
        .expect("compact script must spawn");
    assert!(
        out.status.success(),
        "compact script failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    let after = file_len(&data_file);
    assert!(
        after < before,
        "cold store did not shrink ({before} -> {after}). Script stdout:\n{}\nScript stderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(!cold_dir.with_file_name("cold.new").exists(), "compact script left cold.new residue");
    assert!(!cold_dir.with_file_name("cold.old").exists(), "compact script left cold.old residue");

    // Every odd key survives; even keys were deleted.
    let env = unsafe {
        EnvOpenOptions::new()
            .map_size(64 * 1024 * 1024)
            .max_dbs(1)
            .open(&cold_dir)
            .unwrap()
    };
    let rtxn = env.read_txn().unwrap();
    let db: heed::Database<Bytes, Bytes> = env
        .open_database(&rtxn, Some("rspace-cold"))
        .unwrap()
        .unwrap();
    let mut survived = 0usize;
    for item in db.iter(&rtxn).unwrap() {
        let (_k, _v) = item.unwrap();
        survived += 1;
    }
    assert_eq!(survived, 2000, "compacted env must still hold 2000 live keys");
}
