//! Phase 5 Dir-agent end-to-end regressions.
//!
//! Complements the mock-syscall Dir coverage in `file_dir_check.rs`
//! by exercising the real openat-descent + fs_stat / fs_exists /
//! openFileImpl chain through the genesis+RhoSpec harness.
//!
//! Scope (Phase-9-independent):
//!   - `Dir.exists(rel)` reports present/absent children correctly.
//!   - `Dir.openFile(rel, "rw")` on a "r"-mode Dir returns
//!     FSERR_UNSUPPORTED (monotonic mode attenuation).
//!   - `Dir.openFile(rel, "r")` on a "r"-mode Dir returns a File cap
//!     whose readN drains the seeded content.
//!
//! Phase 10 Dir mutations coverage (2026-08-23): the four mutation
//! methods (`removeFile` / `removeDir(recursive)` / `rename` /
//! `copyFile`) each get an rw-mode round-trip with on-disk `std::fs`
//! verification, mirroring `file_truncate_write_mode_roundtrip` in
//! `fileio_file_spec.rs`.  Each test seeds the tempdir with
//! predictable content, exercises the mutation through the Dir
//! agent's API, asserts the Rholang reply shape, then re-reads the
//! filesystem to confirm the mutation actually landed on disk (not
//! just that the reply arrived).

use std::collections::HashMap;

use casper::rust::genesis::contracts::fs_genesis::{
    BundleConsensusMode, BundleEntry, BundleEntryKind,
};
use casper::rust::genesis::contracts::{fs_genesis, standard_deploys};
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::GenesisBuilder;

/// Bundle a Dir at a tempdir with `child.txt` inside.  Returns
/// (tempdir, params, fs_uri) — tempdir must be kept alive.
fn bundle_dir_with_child(
    dir_mode: &str,
    child_bytes: &[u8],
) -> (
    tempfile::TempDir,
    crate::util::genesis_builder::GenesisParameters,
    String,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(dir.path()).expect("canonicalize");
    std::fs::write(root.join("child.txt"), child_bytes).expect("seed child");

    let entry = BundleEntry::try_new(
        "shareddir".to_string(),
        root,
        BundleEntryKind::Dir,
        dir_mode.to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("bundle entry construction");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);
    (dir, params, fs_uri)
}

/// Dir.exists reports present/absent children correctly.
/// Both calls run under the same Dir cap so the tempdir + genesis
/// setup amortize.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_exists_reports_present_and_absent() {
    let (_dir, params, fs_uri) = bundle_dir_with_child("r", b"child content");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_exists_reports_present_and_absent
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Dir.exists reports present + absent children",
        *test_exists_reports_present_and_absent)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_exists_reports_present_and_absent(rhoSpec, _, ackCh) = {{
      for(@[true, dir] <- @fs!?("openDir", "shareddir", {{}})) {{
        for(@rPresent <- @dir!?("exists", "child.txt")) {{
          for(@rAbsent <- @dir!?("exists", "no-such-file.txt")) {{
            match [rPresent, rAbsent] {{
              [[true, true], [true, false]] => {{
                rhoSpec!("assert", (true, "==", true),
                  "exists reports present + absent correctly", *ackCh)
              }}
              _ => {{
                rhoSpec!("assert", ([rPresent, rAbsent], "==",
                  "[[true, true], [true, false]]"),
                  "exists reports present + absent correctly", *ackCh)
              }}
            }}
          }}
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled =
        CompiledRholangSource::new(test_source, HashMap::new(), "DirExistsSpec".to_string())
            .expect("compile dir_exists spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("dir_exists spec failed");
}

/// Dir.openFile mode attenuation: an "r"-mode Dir refuses to yield
/// a "rw"-mode File child.  Pins Dir.rho line 220's
/// `"requested mode exceeds Dir attenuation"` gate.  Regression
/// would let a subordinate escalate mode by requesting "rw" on a
/// read-only Dir cap.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_openfile_mode_attenuation() {
    let (_dir, params, fs_uri) = bundle_dir_with_child("r", b"child content");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_openfile_mode_attenuation
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Dir.openFile('child', 'rw') on r-Dir returns FSERR_UNSUPPORTED",
        *test_openfile_mode_attenuation)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_openfile_mode_attenuation(rhoSpec, _, ackCh) = {{
      for(@[true, dir] <- @fs!?("openDir", "shareddir", {{}})) {{
        for(@r <- @dir!?("openFile", "child.txt", "rw")) {{
          match r {{
            [false, "FSERR_UNSUPPORTED", _] => {{
              rhoSpec!("assert", (true, "==", true),
                "Dir.openFile('child', 'rw') on r-Dir gates with FSERR_UNSUPPORTED",
                *ackCh)
            }}
            _ => {{
              rhoSpec!("assert", (r, "==", "[false, FSERR_UNSUPPORTED, _]"),
                "Dir.openFile('child', 'rw') on r-Dir gates with FSERR_UNSUPPORTED",
                *ackCh)
            }}
          }}
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled = CompiledRholangSource::new(
        test_source,
        HashMap::new(),
        "DirOpenFileAttenuationSpec".to_string(),
    )
    .expect("compile dir_openfile_attenuation spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("dir_openfile_attenuation spec failed");
}

/// Dir.openFile(rel, "r") + File.readN roundtrip on the returned
/// child cap.  Exercises the openFileImpl(canonRoot=dir, subPath="",
/// rel="child.txt") chain from Dir plus the File.readN native
/// dispatch, and verifies the read bytes match the seeded content
/// exactly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_openfile_child_readn_roundtrip() {
    let child_bytes = b"hello dir spec"; // 14 bytes
    let (_dir, params, fs_uri) = bundle_dir_with_child("r", child_bytes);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_dir_openfile_readn_roundtrip
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Dir.openFile('child.txt') + readN yields seeded content",
        *test_dir_openfile_readn_roundtrip)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_dir_openfile_readn_roundtrip(rhoSpec, _, ackCh) = {{
      for(@[true, dir] <- @fs!?("openDir", "shareddir", {{}})) {{
        for(@[true, file] <- @dir!?("openFile", "child.txt", "r")) {{
          for(@[true, bytes] <- @file!?("readN", 64)) {{
            // "hello dir spec".toUtf8Bytes() as hex.
            rhoSpec!("assert",
              (bytes, "==", "68656c6c6f2064697220737065".hexToBytes() ++ "63".hexToBytes()),
              "readN on Dir-opened child returns seeded content", *ackCh)
          }}
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled = CompiledRholangSource::new(
        test_source,
        HashMap::new(),
        "DirOpenFileReadNSpec".to_string(),
    )
    .expect("compile dir_openfile_readn spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("dir_openfile_readn spec failed");
}

// ----- Phase 10 mutations coverage (2026-08-23) ---------------------

/// `Dir.removeFile("child.txt")` on an rw-mode Dir returns `[true]`
/// AND the child file is actually gone from disk after the deploy
/// completes.  Post-run `std::fs::exists` check catches a regression
/// where the reply is synthesized without invoking the underlying
/// `unlinkat(2)`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_remove_file_actually_unlinks() {
    let (dir, params, fs_uri) = bundle_dir_with_child("rw", b"unlink me");
    let child_path = std::fs::canonicalize(dir.path())
        .expect("canonicalize root")
        .join("child.txt");
    assert!(child_path.exists(), "seed child.txt must exist pre-run");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_remove_file
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Dir.removeFile actually unlinks the child file",
        *test_remove_file)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_remove_file(rhoSpec, _, ackCh) = {{
      for(@[true, d] <- @fs!?("openDir", "shareddir", {{"mode": "rw"}})) {{
        for(@r <- @d!?("removeFile", "child.txt")) {{
          rhoSpec!("assert", (r, "==", [true]),
            "Dir.removeFile('child.txt') → [true]", *ackCh)
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled =
        CompiledRholangSource::new(test_source, HashMap::new(), "DirRemoveFileSpec".to_string())
            .expect("compile dir_remove_file spec");
    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("dir_remove_file spec failed");

    assert!(
        !child_path.exists(),
        "post-run child.txt must be gone from disk (Dir.removeFile is expected to \
         invoke unlinkat, not just synthesize a [true] reply)"
    );
}

/// `Dir.removeDir("subdir", true)` on an rw-mode Dir returns `[true]`
/// AND the entire subdirectory subtree is gone from disk.  Seeds a
/// two-file subdirectory so a non-recursive `unlinkat(AT_REMOVEDIR)`
/// would fail with ENOTEMPTY — verifies the recursive walker is
/// actually invoked.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_remove_dir_recursive_wipes_subtree() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(dir.path()).expect("canonicalize");
    let sub = root.join("subdir");
    std::fs::create_dir(&sub).expect("mkdir subdir");
    std::fs::write(sub.join("a.txt"), b"a").expect("seed a.txt");
    std::fs::write(sub.join("b.txt"), b"b").expect("seed b.txt");
    assert!(sub.exists(), "subdir must exist pre-run");

    let entry = BundleEntry::try_new(
        "shareddir".to_string(),
        root,
        BundleEntryKind::Dir,
        "rw".to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("bundle entry");
    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];
    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_remove_dir_recursive
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Dir.removeDir(recursive) wipes the subtree",
        *test_remove_dir_recursive)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_remove_dir_recursive(rhoSpec, _, ackCh) = {{
      for(@[true, d] <- @fs!?("openDir", "shareddir", {{"mode": "rw"}})) {{
        for(@r <- @d!?("removeDir", "subdir", true)) {{
          rhoSpec!("assert", (r, "==", [true]),
            "Dir.removeDir('subdir', true) → [true]", *ackCh)
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled = CompiledRholangSource::new(
        test_source,
        HashMap::new(),
        "DirRemoveDirRecursiveSpec".to_string(),
    )
    .expect("compile dir_remove_dir_recursive spec");
    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("dir_remove_dir_recursive spec failed");

    assert!(
        !sub.exists(),
        "post-run subdir/ must be gone from disk (recursive removeDir must \
         walk children and unlink them)"
    );
    // Keep tempdir alive through the assertions.
    drop(dir);
}

/// `Dir.rename("old.txt", "new.txt")` returns `[true]` AND the old
/// filename is gone from disk while the new filename holds the same
/// content.  Verifies the underlying `renameat` actually swapped the
/// directory entry rather than copying or synthesizing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_rename_moves_child_on_disk() {
    // bundle_dir_with_child seeds `child.txt`; rename it to
    // `renamed.txt`.
    let (dir, params, fs_uri) = bundle_dir_with_child("rw", b"rename me");
    let root = std::fs::canonicalize(dir.path()).expect("canonicalize");
    let old_path = root.join("child.txt");
    let new_path = root.join("renamed.txt");
    assert!(old_path.exists(), "seed child.txt must exist");
    assert!(!new_path.exists(), "renamed.txt must not exist yet");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_rename
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Dir.rename moves child.txt → renamed.txt on disk",
        *test_rename)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_rename(rhoSpec, _, ackCh) = {{
      for(@[true, d] <- @fs!?("openDir", "shareddir", {{"mode": "rw"}})) {{
        for(@r <- @d!?("rename", "child.txt", "renamed.txt")) {{
          rhoSpec!("assert", (r, "==", [true]),
            "Dir.rename → [true]", *ackCh)
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled =
        CompiledRholangSource::new(test_source, HashMap::new(), "DirRenameSpec".to_string())
            .expect("compile dir_rename spec");
    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("dir_rename spec failed");

    assert!(
        !old_path.exists(),
        "post-run child.txt (source) must be gone from disk"
    );
    assert!(
        new_path.exists(),
        "post-run renamed.txt (destination) must exist on disk"
    );
    let content = std::fs::read(&new_path).expect("read renamed.txt");
    assert_eq!(
        content, b"rename me",
        "rename must preserve the file content byte-for-byte"
    );
}

/// `Dir.copyFile("src.txt", "dst.txt")` returns `[true, n]` where n
/// is the byte count copied AND both source and destination exist on
/// disk after the deploy, with identical content.  Verifies the
/// native copy actually reads/writes rather than link-substituting
/// (a hard-link would also make both paths resolve to the same
/// content, but changing dst.txt would then also mutate src.txt —
/// this test bounds the shape but not that specific correctness
/// aspect; file_dir_check covers that in the mock-syscall layer).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_copy_file_duplicates_content_on_disk() {
    let (dir, params, fs_uri) = bundle_dir_with_child("rw", b"copy me");
    let root = std::fs::canonicalize(dir.path()).expect("canonicalize");
    let src_path = root.join("child.txt");
    let dst_path = root.join("copy.txt");
    assert!(src_path.exists(), "seed child.txt must exist");
    assert!(!dst_path.exists(), "copy.txt must not exist yet");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_copy
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Dir.copyFile duplicates child.txt → copy.txt on disk",
        *test_copy)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_copy(rhoSpec, _, ackCh) = {{
      for(@[true, d] <- @fs!?("openDir", "shareddir", {{"mode": "rw"}})) {{
        for(@r <- @d!?("copyFile", "child.txt", "copy.txt")) {{
          // Native fs_copyFile returns [true, nBytes]; content is
          // 7 bytes ("copy me" without a trailing newline).
          rhoSpec!("assert", (r, "==", [true, 7]),
            "Dir.copyFile → [true, 7]", *ackCh)
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled =
        CompiledRholangSource::new(test_source, HashMap::new(), "DirCopyFileSpec".to_string())
            .expect("compile dir_copy_file spec");
    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("dir_copy_file spec failed");

    assert!(src_path.exists(), "source child.txt must still exist");
    assert!(dst_path.exists(), "destination copy.txt must exist");
    let src_bytes = std::fs::read(&src_path).expect("read child.txt");
    let dst_bytes = std::fs::read(&dst_path).expect("read copy.txt");
    assert_eq!(
        src_bytes, dst_bytes,
        "source and destination must have identical content post-copy"
    );
    assert_eq!(src_bytes, b"copy me", "source content preserved");
}
