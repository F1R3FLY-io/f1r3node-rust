//! Phase 5 Dir-agent end-to-end regressions.
//!
//! Complements the mock-syscall Dir coverage in `file_dir_check.rs`
//! by exercising the real openat-descent + fs_stat / fs_exists /
//! openFileImpl chain through the genesis+RhoSpec harness.
//!
//! Scope (Phase-9-independent):
//!   - `Dir.exists(rel)` reports present/absent children correctly.
//!   - `Dir.openFile(rel, "r+")` on a "r"-mode Dir returns
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
/// a "r+"-mode File child.  Pins Dir.rho line 220's
/// `"requested mode exceeds Dir attenuation"` gate.  Regression
/// would let a subordinate escalate mode by requesting "r+" on a
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
        for(@r <- @dir!?("openFile", "child.txt", {{"mode": "r+"}})) {{
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
        for(@[true, file] <- @dir!?("openFile", "child.txt", {{"mode": "r"}})) {{
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
    let (dir, params, fs_uri) = bundle_dir_with_child("r+", b"unlink me");
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
      for(@[true, d] <- @fs!?("openDir", "shareddir", {{"mode": "r+"}})) {{
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

/// `Dir.removeDir("subdir", true)` on an rw-mode Dir returns
/// `[true, nDeleted]` per DD-RemoveDirReplyShape (2026-09-03).
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
        "r+".to_string(),
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
      for(@[true, d] <- @fs!?("openDir", "shareddir", {{"mode": "r+"}})) {{
        for(@r <- @d!?("removeDir", "subdir", {{"recursive": true}})) {{
          // DD-RemoveDirReplyShape: subdir contains 2 files + subdir itself
          // = 3 filesystem entries removed.
          rhoSpec!("assert", (r, "==", [true, 3]),
            "Dir.removeDir('subdir', true) → [true, nDeleted=3]", *ackCh)
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

/// H6 (coverage-review 2026-09-03): Consensus recursive removeDir
/// returns a truncated `[true, nDeleted]` at the Rholang boundary
/// (Dir.rho strips the manifest that lives at position 2 of the
/// native reply).  Callers must see the same 2-element shape they
/// see under Oracular recursive — Consensus vs Oracular is not
/// observable from the reply Par.  Same 3-entry subtree as the
/// Oracular version above; only `BundleConsensusMode` differs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dir_remove_dir_recursive_consensus_truncates_manifest_upward() {
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
        "r+".to_string(),
        BundleConsensusMode::Consensus,
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
  test_remove_dir_consensus_truncated
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Consensus recursive removeDir reply is truncated to [true, n]",
        *test_remove_dir_consensus_truncated)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_remove_dir_consensus_truncated(rhoSpec, _, ackCh) = {{
      for(@[true, d] <- @fs!?("openDir", "shareddir", {{"mode": "r+"}})) {{
        for(@r <- @d!?("removeDir", "subdir", {{"recursive": true}})) {{
          // Native returns [true, 3, [[path, kind], ...]] for
          // Consensus recursive; Dir.rho unwraps to [true, 3].
          // Callers observe the same 2-element shape as Oracular
          // recursive — cmode is not distinguishable from the reply.
          rhoSpec!("assert", (r, "==", [true, 3]),
            "Consensus recursive removeDir reply truncated to [true, 3]",
            *ackCh)
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
        "DirRemoveDirConsensusTruncatedSpec".to_string(),
    )
    .expect("compile dir_remove_dir_consensus_truncated spec");
    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("dir_remove_dir_consensus_truncated spec failed");

    // NOTE: under Shape A, Consensus caps operate against a per-
    // validator on-disk subdir (via the RootIdentityRegistry
    // resolver), not the raw tempdir root.  So `sub.exists()` on
    // the tempdir stays true — the removed subdir lives at
    // FS_ROOT/<validator_key>/shareddir/subdir.  The rhoSpec
    // `[true, 3]` assertion is the load-bearing check that proves
    // shape truncation + correct count.
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
    let (dir, params, fs_uri) = bundle_dir_with_child("r+", b"rename me");
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
      for(@[true, d] <- @fs!?("openDir", "shareddir", {{"mode": "r+"}})) {{
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
    let (dir, params, fs_uri) = bundle_dir_with_child("r+", b"copy me");
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
      for(@[true, d] <- @fs!?("openDir", "shareddir", {{"mode": "r+"}})) {{
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

/// Non-cascading-revoke end-to-end: `Fs.revoke()` is the ambient-
/// authority off switch, but it MUST NOT cascade to previously-
/// minted File / Dir caps.  The Fs.rho docstring (lines 204-212)
/// states:
///
///   > Previously-minted File/Dir caps are unaffected — they are
///   > independent agents.  See spec §Revocation for the normative
///   > semantics (revoke does NOT cascade to previously-minted
///   > caps).
///
/// This test pins that invariant end-to-end at the Rholang API
/// surface by exercising **every Dir method and every File method**
/// that can run against an "r+"-mode cap, after revoke, through the
/// pre-held dirCap minted pre-revoke.  Any method that routes
/// through an `fsRevokedP` peek (regression) would fail at its
/// chained step with a specific-step error.
///
/// Chain (sequential; `_dir` must stay alive throughout):
///
/// A. Setup verification
///   1. Pre-revoke `Fs.openDir("shareddir", {"mode": "r+"})` → `[true, dirCap]`.
///   2. `Fs.revoke()` → `[true]`.
///   3. Fresh `Fs.openDir(...)` post-revoke → `[false, "FSERR_REVOKED", _]`.
///
/// B. Dir query methods on the pre-held cap
///   4. `dirCap.exists("child.txt")` → `[true, true]`.
///   5. `dirCap.exists("missing.txt")` → `[true, false]`.
///   6. `dirCap.stat("child.txt")` → `[true, _record]`.
///
/// C. File read methods on an "r"-mode fileCap minted from the pre-held dirCap
///   7. `dirCap.openFile("child.txt", {"mode": "r"})` → `[true, roFile]`.
///   8. `roFile.size()` → `[true, _n]`.
///   9. `roFile.tell()` → `[true, _pos]`.
///  10. `roFile.seek(5, "set")` → `[true, ..._]`.
///  11. `roFile.readN(64)` → `[true, _bytes]`.
///  12. `roFile.flush()` → `[true, ..._]`.
///  13. `roFile.close()` → `[true, ..._]`.
///
/// D. File write methods on an "r+"-mode fileCap
///  14. `dirCap.openFile("child.txt", {"mode": "r+"})` → `[true, rwFile]`.
///      (POSIX r+: read + write, require existing file. Dir.openFile
///      forwards the mode string straight to `fs_open` which routes
///      through `parse_open_mode`; "r+" is NOT a valid native-side
///      mode — only "r", "r+", "w", "w+", "wx", "w+x", "a", "a+".)
///  15. `rwFile.writeString("rewritten after revoke")` → `[true, ..._]`.
///  16. `rwFile.truncate(10)` → `[true, ..._]`.
///  17. `rwFile.chmod(420)` → `[true, ..._]` (0o644).
///  18. `rwFile.close()` → `[true, ..._]`.
///
/// E. Dir mutation methods on the pre-held cap
///  19. `dirCap.removeFile("to_remove.txt")` → `[true, ..._]`.
///  20. `dirCap.removeDir("empty_sub", {})` → `[true, ..._]`.
///  21. `dirCap.rename("rename_src.txt", "renamed.txt")` → `[true, ..._]`.
///  22. `dirCap.copyFile("copy_src.txt", "copy_dst.txt")` → `[true, ..._]`.
///
/// Final rhoSpec assertion fires on step 22.  Any earlier step that
/// fails emits a specific "step N: ..." message naming the failing
/// method and reply shape.
///
/// # Methods not exercised (pre-existing drifts, unrelated to revoke)
///
/// - `dirCap.entries()`: root-bundle dirCap carries `subPath=""`;
///   `fsEntriesStreamOpen` rejects with "empty relative path".
/// - `dirCap.openDir("subdir", {})`: Dir.rho checks
///   `rec.get("kind") == "dir"` but `io/stat.rs` encodes
///   `Kind::Directory` as `"directory"` — spec/impl drift.
/// - `fileCap.lockRange(...)`: lock-registry setup orthogonal to
///   this test's scope.
/// - `fileCap.chown(...)`: NSS lookup side-effects orthogonal to
///   this test's scope.
/// - Buffer-backed `readInto` / `writeFrom`: require a Buffer cap
///   plumbed into the deploy, orthogonal to this test's scope.
///
/// Regression value: a future refactor that routes any File / Dir
/// method dispatch through an `fsRevokedP` peek (e.g., someone adds
/// a "revoke everything" switch at the agent layer) would silently
/// break the non-cascade invariant.  The failing method's step
/// marker in the error message pinpoints the broken dispatch.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_revoke_leaves_pre_held_dir_cap_live() {
    // Inline setup: tempdir seeded with multiple files and subdirs,
    // so the test can exercise mutations (rename/copy/removeFile/
    // removeDir) alongside reads without later steps conflicting
    // with earlier ones.  `bundle_dir_with_child` only seeds a
    // single child, so we build the fixture directly here.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(dir.path()).expect("canonicalize");

    std::fs::write(root.join("child.txt"), b"non-cascade payload").expect("seed child.txt"); // 19 bytes
    std::fs::create_dir(root.join("subdir")).expect("seed subdir");
    std::fs::write(root.join("subdir").join("nested.txt"), b"nested").expect("seed nested.txt");
    std::fs::write(root.join("to_remove.txt"), b"gone soon").expect("seed to_remove.txt");
    std::fs::create_dir(root.join("empty_sub")).expect("seed empty_sub");
    std::fs::write(root.join("rename_src.txt"), b"rename me").expect("seed rename_src.txt");
    std::fs::write(root.join("copy_src.txt"), b"copy me").expect("seed copy_src.txt");

    let entry = BundleEntry::try_new(
        "shareddir".to_string(),
        root,
        BundleEntryKind::Dir,
        "r+".to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("bundle entry construction");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    // The chain is flattened via one Rholang `contract stepN(...)`
    // per step.  Each step's success arm sends on the next step's
    // channel; the failure arm fires a step-labeled rhoSpec
    // assertion that pinpoints the method that didn't survive
    // revoke.  Avoids the deep-nesting pyramid that an inline
    // `for(@r <- @cap!?(...)) { match r { ... } }` chain produces,
    // and keeps each method's test contained to its own contract
    // body so inserting / removing a method is a one-contract edit.
    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  step1, step2, step3, step4, step5, step6, step7, step8, step9,
  step10, step11, step12, step13, step14, step15, step16, step17, step18,
  step19, step20, step21, step22
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Fs.revoke() leaves pre-held Dir cap live for every method (non-cascade)",
        *step1)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{

    // ===== Section A — Setup verification =====

    // Step 1: pre-revoke `Fs.openDir` mints an rw Dir cap.  On
    // success, forwards (rhoSpec, dirCap, ackCh) to step2 so the
    // minted-pre-revoke cap threads through every later step.
    contract step1(rhoSpec, _, ackCh) = {{
      for(@r1 <- @fs!?("openDir", "shareddir", {{"mode": "r+"}})) {{
        match r1 {{
          [true, dirCap] => step2!(*rhoSpec, dirCap, *ackCh)
          _ => rhoSpec!("assert", (r1, "==", "[true, dirCap]"),
            "step 1: pre-revoke openDir mints Dir cap", *ackCh)
        }}
      }}
    }} |

    // Step 2: `Fs.revoke()` must return `[true]`.
    contract step2(rhoSpec, @dirCap, ackCh) = {{
      for(@r2 <- @fs!?("revoke")) {{
        match r2 {{
          [true] => step3!(*rhoSpec, dirCap, *ackCh)
          _ => rhoSpec!("assert", (r2, "==", "[true]"),
            "step 2: Fs.revoke() returns [true]", *ackCh)
        }}
      }}
    }} |

    // Step 3: fresh `Fs.openDir` post-revoke must be inert.
    contract step3(rhoSpec, @dirCap, ackCh) = {{
      for(@r3 <- @fs!?("openDir", "shareddir", {{"mode": "r+"}})) {{
        match r3 {{
          [false, "FSERR_REVOKED", _msg] => step4!(*rhoSpec, dirCap, *ackCh)
          _ => rhoSpec!("assert", (r3, "==", "[false, FSERR_REVOKED, _]"),
            "step 3: ambient Fs.openDir inert post-revoke", *ackCh)
        }}
      }}
    }} |

    // ===== Section B — Dir query methods on the pre-held cap =====

    // Step 4: dirCap.exists(present) → [true, true].
    contract step4(rhoSpec, @dirCap, ackCh) = {{
      for(@r4 <- @dirCap!?("exists", "child.txt")) {{
        match r4 {{
          [true, true] => step5!(*rhoSpec, dirCap, *ackCh)
          _ => rhoSpec!("assert", (r4, "==", "[true, true]"),
            "step 4: exists(present)", *ackCh)
        }}
      }}
    }} |

    // Step 5: dirCap.exists(absent) → [true, false].
    contract step5(rhoSpec, @dirCap, ackCh) = {{
      for(@r5 <- @dirCap!?("exists", "no-such-file.txt")) {{
        match r5 {{
          [true, false] => step6!(*rhoSpec, dirCap, *ackCh)
          _ => rhoSpec!("assert", (r5, "==", "[true, false]"),
            "step 5: exists(absent)", *ackCh)
        }}
      }}
    }} |

    // Step 6: dirCap.stat → [true, _record].
    contract step6(rhoSpec, @dirCap, ackCh) = {{
      for(@r6 <- @dirCap!?("stat", "child.txt")) {{
        match r6 {{
          [true, _rec] => step7!(*rhoSpec, dirCap, *ackCh)
          _ => rhoSpec!("assert", (r6, "==", "[true, _rec]"),
            "step 6: stat", *ackCh)
        }}
      }}
    }} |

    // ===== Section C — File read methods on an r-mode fileCap =====

    // Step 7: dirCap.openFile(r) → mints a File cap.  Forwards the
    // roFile handle alongside dirCap so steps 8-13 can use both.
    contract step7(rhoSpec, @dirCap, ackCh) = {{
      for(@r7 <- @dirCap!?("openFile", "child.txt", {{"mode": "r"}})) {{
        match r7 {{
          [true, roFile] => step8!(*rhoSpec, dirCap, roFile, *ackCh)
          _ => rhoSpec!("assert", (r7, "==", "[true, roFile]"),
            "step 7: openFile(r)", *ackCh)
        }}
      }}
    }} |

    // Step 8: roFile.size() → [true, _n].
    contract step8(rhoSpec, @dirCap, @roFile, ackCh) = {{
      for(@r8 <- @roFile!?("size")) {{
        match r8 {{
          [true, _sz] => step9!(*rhoSpec, dirCap, roFile, *ackCh)
          _ => rhoSpec!("assert", (r8, "==", "[true, _sz]"),
            "step 8: size", *ackCh)
        }}
      }}
    }} |

    // Step 9: roFile.tell() → [true, _pos].
    contract step9(rhoSpec, @dirCap, @roFile, ackCh) = {{
      for(@r9 <- @roFile!?("tell")) {{
        match r9 {{
          [true, _pos] => step10!(*rhoSpec, dirCap, roFile, *ackCh)
          _ => rhoSpec!("assert", (r9, "==", "[true, _pos]"),
            "step 9: tell", *ackCh)
        }}
      }}
    }} |

    // Step 10: roFile.seek(5, "set") → [true, ..._].
    contract step10(rhoSpec, @dirCap, @roFile, ackCh) = {{
      for(@r10 <- @roFile!?("seek", 5, "set")) {{
        match r10 {{
          [true, ..._] => step11!(*rhoSpec, dirCap, roFile, *ackCh)
          _ => rhoSpec!("assert", (r10, "==", "[true, ..._]"),
            "step 10: seek", *ackCh)
        }}
      }}
    }} |

    // Step 11: roFile.readN(64) → [true, _bytes].
    contract step11(rhoSpec, @dirCap, @roFile, ackCh) = {{
      for(@r11 <- @roFile!?("readN", 64)) {{
        match r11 {{
          [true, _bs] => step12!(*rhoSpec, dirCap, roFile, *ackCh)
          _ => rhoSpec!("assert", (r11, "==", "[true, _bs]"),
            "step 11: readN", *ackCh)
        }}
      }}
    }} |

    // Step 12: roFile.flush() → [true, ..._].
    contract step12(rhoSpec, @dirCap, @roFile, ackCh) = {{
      for(@r12 <- @roFile!?("flush")) {{
        match r12 {{
          [true, ..._] => step13!(*rhoSpec, dirCap, roFile, *ackCh)
          _ => rhoSpec!("assert", (r12, "==", "[true, ..._]"),
            "step 12: flush", *ackCh)
        }}
      }}
    }} |

    // Step 13: roFile.close() → [true, ..._].  roFile is consumed
    // here; step 14 opens a fresh rwFile for the write-mode section.
    contract step13(rhoSpec, @dirCap, @roFile, ackCh) = {{
      for(@r13 <- @roFile!?("close")) {{
        match r13 {{
          [true, ..._] => step14!(*rhoSpec, dirCap, *ackCh)
          _ => rhoSpec!("assert", (r13, "==", "[true, ..._]"),
            "step 13: roFile.close", *ackCh)
        }}
      }}
    }} |

    // ===== Section D — File write methods on an r+-mode fileCap =====
    //
    // POSIX "r+" = read + write, require-exist.  Dir.openFile
    // forwards the mode straight to `fs_open` which routes through
    // `parse_open_mode`; the accepted file-mode set is
    // r / r+ / w / w+ / wx / w+x / a / a+.

    // Step 14: dirCap.openFile(r+) → mints a writable File cap.
    contract step14(rhoSpec, @dirCap, ackCh) = {{
      for(@r14 <- @dirCap!?("openFile", "child.txt", {{"mode": "r+"}})) {{
        match r14 {{
          [true, rwFile] => step15!(*rhoSpec, dirCap, rwFile, *ackCh)
          _ => rhoSpec!("assert", (r14, "==", "[true, rwFile]"),
            "step 14: openFile(r+)", *ackCh)
        }}
      }}
    }} |

    // Step 15: rwFile.writeString(...) → [true, ..._].
    contract step15(rhoSpec, @dirCap, @rwFile, ackCh) = {{
      for(@r15 <- @rwFile!?("writeString", "rewritten after revoke")) {{
        match r15 {{
          [true, ..._] => step16!(*rhoSpec, dirCap, rwFile, *ackCh)
          _ => rhoSpec!("assert", (r15, "==", "[true, ..._]"),
            "step 15: writeString", *ackCh)
        }}
      }}
    }} |

    // Step 16: rwFile.truncate(10) → [true, ..._].
    contract step16(rhoSpec, @dirCap, @rwFile, ackCh) = {{
      for(@r16 <- @rwFile!?("truncate", 10)) {{
        match r16 {{
          [true, ..._] => step17!(*rhoSpec, dirCap, rwFile, *ackCh)
          _ => rhoSpec!("assert", (r16, "==", "[true, ..._]"),
            "step 16: truncate", *ackCh)
        }}
      }}
    }} |

    // Step 17: rwFile.chmod(0o644 = 420) → [true, ..._].
    contract step17(rhoSpec, @dirCap, @rwFile, ackCh) = {{
      for(@r17 <- @rwFile!?("chmod", 420)) {{
        match r17 {{
          [true, ..._] => step18!(*rhoSpec, dirCap, rwFile, *ackCh)
          _ => rhoSpec!("assert", (r17, "==", "[true, ..._]"),
            "step 17: chmod", *ackCh)
        }}
      }}
    }} |

    // Step 18: rwFile.close() → [true, ..._].  rwFile consumed here;
    // Section E runs mutations against dirCap directly.
    contract step18(rhoSpec, @dirCap, @rwFile, ackCh) = {{
      for(@r18 <- @rwFile!?("close")) {{
        match r18 {{
          [true, ..._] => step19!(*rhoSpec, dirCap, *ackCh)
          _ => rhoSpec!("assert", (r18, "==", "[true, ..._]"),
            "step 18: rwFile.close", *ackCh)
        }}
      }}
    }} |

    // ===== Section E — Dir mutation methods on the pre-held cap =====

    // Step 19: dirCap.removeFile("to_remove.txt") → [true, ..._].
    contract step19(rhoSpec, @dirCap, ackCh) = {{
      for(@r19 <- @dirCap!?("removeFile", "to_remove.txt")) {{
        match r19 {{
          [true, ..._] => step20!(*rhoSpec, dirCap, *ackCh)
          _ => rhoSpec!("assert", (r19, "==", "[true, ..._]"),
            "step 19: removeFile", *ackCh)
        }}
      }}
    }} |

    // Step 20: dirCap.removeDir("empty_sub", {{}}) → [true, ..._].
    contract step20(rhoSpec, @dirCap, ackCh) = {{
      for(@r20 <- @dirCap!?("removeDir", "empty_sub", {{}})) {{
        match r20 {{
          [true, ..._] => step21!(*rhoSpec, dirCap, *ackCh)
          _ => rhoSpec!("assert", (r20, "==", "[true, ..._]"),
            "step 20: removeDir", *ackCh)
        }}
      }}
    }} |

    // Step 21: dirCap.rename("rename_src.txt", "renamed.txt") → [true, ..._].
    contract step21(rhoSpec, @dirCap, ackCh) = {{
      for(@r21 <- @dirCap!?("rename", "rename_src.txt", "renamed.txt")) {{
        match r21 {{
          [true, ..._] => step22!(*rhoSpec, dirCap, *ackCh)
          _ => rhoSpec!("assert", (r21, "==", "[true, ..._]"),
            "step 21: rename", *ackCh)
        }}
      }}
    }} |

    // Step 22: dirCap.copyFile("copy_src.txt", "copy_dst.txt") → [true, ..._].
    // Final success assertion fires here when every prior step
    // forwarded cleanly.
    contract step22(rhoSpec, @dirCap, ackCh) = {{
      for(@r22 <- @dirCap!?("copyFile", "copy_src.txt", "copy_dst.txt")) {{
        match r22 {{
          [true, ..._] => rhoSpec!("assert", (true, "==", true),
            "all 22 Dir/File methods survived Fs.revoke on the pre-held cap",
            *ackCh)
          _ => rhoSpec!("assert", (r22, "==", "[true, ..._]"),
            "step 22: copyFile", *ackCh)
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
        "FsRevokeNonCascadeSpec".to_string(),
    )
    .expect("compile fs_revoke non-cascade spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("fs_revoke non-cascade spec failed");
}

/// Companion to `fs_revoke_leaves_pre_held_dir_cap_live`: the
/// non-cascade invariant is paired with a *no-escape* invariant.
/// Once `Fs.revoke()` has been called, re-looking up the registered
/// `fs_uri` via the registry MUST return the same (now-inert) Fs
/// cap — not a fresh, un-revoked instance.  Otherwise a deploy
/// could circumvent revocation by doing `rl!(fs_uri, *ch)` again
/// after revoke and regain ambient access to the filesystem.
///
/// The guarantee lives at `Fs.rho:197-212`:
///
///   > Module-level cell `fsRevokedP` holds the one-way revoke
///   > flag, shared across every Fs instance minted from this
///   > composed source. … Post-revoke, any Fs instance (including
///   > instances minted AFTER revoke) has all methods returning
///   > FSERR_REVOKED.
///
/// Combined with the registry's identity semantics — a URI lookup
/// returns a reference to the same registered cap on every call,
/// not a fresh mint — the only path to the filesystem post-revoke
/// is through File / Dir caps that were acquired *before* revoke.
/// This test pins that end-to-end at the Rholang API surface.
///
/// Chain:
///   1. `rl!(fs_uri, *fsCh1)` + revoke → `[true]`.
///   2. **Second** `rl!(fs_uri, *fsCh2)` — fresh registry lookup
///      against the same URI.  Forward the resolved `fs2` to each
///      subsequent step.
///   3-7. On the re-looked-up `fs2`, every ambient method
///      (`openFile`, `openDir`, `stdin`, `stdout`, `stderr`) must
///      return `[false, "FSERR_REVOKED", _]`.  The revoke gate at
///      the top of each Fs method body peeks `fsRevokedP` before
///      the bundle / fd lookup, so any args work; the test uses
///      syntactically valid shapes to avoid a different error-arm
///      masking the revoke gate.
///
/// Regression value: a refactor that mints a fresh Fs on each
/// registry lookup (e.g., by replacing `insertSigned` with a
/// factory registration), or wires `fsRevokedP` per-instance
/// rather than module-level, would silently restore ambient access
/// post-revoke and this test would catch it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_revoke_blocks_registry_re_lookup() {
    // Setup: a minimal populated bundle so Fs mint succeeds at
    // genesis; the test doesn't exercise bundle contents because
    // the revoke gate fires before any name-in-bundle check.
    let (_dir, params, fs_uri) = bundle_dir_with_child("r", b"irrelevant");

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh1,
  step1, step2, step3, step4, step5, step6
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Fs.revoke() blocks subsequent registry re-lookups (no ambient-access escape)",
        *step1)])
  }} |

  rl!(`{fs_uri}`, *fsCh1) |
  for(@(_, fs1) <- fsCh1) {{

    // Step 1: fs1.revoke() → [true].  Then do a FRESH
    // registry lookup for the same URI and forward the resolved
    // `fs2` into step2.  `fs2` is a Rholang-level distinct binding
    // but the registry returns the SAME registered cap, so fs2 is
    // bound to the same agent and reads the same (now-flipped)
    // fsRevokedP cell.
    contract step1(rhoSpec, _, ackCh) = {{
      for(@r1 <- @fs1!?("revoke")) {{
        match r1 {{
          [true] => {{
            new fsCh2 in {{
              rl!(`{fs_uri}`, *fsCh2) |
              for(@(_, fs2) <- fsCh2) {{
                step2!(*rhoSpec, fs2, *ackCh)
              }}
            }}
          }}
          _ => rhoSpec!("assert", (r1, "==", "[true]"),
            "step 1: Fs.revoke() returns [true]", *ackCh)
        }}
      }}
    }} |

    // Step 2: re-looked-up fs2.openFile post-revoke must be inert.
    contract step2(rhoSpec, @fs2, ackCh) = {{
      for(@r2 <- @fs2!?("openFile", "shareddir", {{}})) {{
        match r2 {{
          [false, "FSERR_REVOKED", _msg] => step3!(*rhoSpec, fs2, *ackCh)
          _ => rhoSpec!("assert", (r2, "==", "[false, FSERR_REVOKED, _]"),
            "step 2: re-looked-up fs.openFile inert", *ackCh)
        }}
      }}
    }} |

    // Step 3: re-looked-up fs2.openDir post-revoke must be inert.
    contract step3(rhoSpec, @fs2, ackCh) = {{
      for(@r3 <- @fs2!?("openDir", "shareddir", {{}})) {{
        match r3 {{
          [false, "FSERR_REVOKED", _msg] => step4!(*rhoSpec, fs2, *ackCh)
          _ => rhoSpec!("assert", (r3, "==", "[false, FSERR_REVOKED, _]"),
            "step 3: re-looked-up fs.openDir inert", *ackCh)
        }}
      }}
    }} |

    // Step 4: re-looked-up fs2.stdin post-revoke must be inert.
    contract step4(rhoSpec, @fs2, ackCh) = {{
      for(@r4 <- @fs2!?("stdin")) {{
        match r4 {{
          [false, "FSERR_REVOKED", _msg] => step5!(*rhoSpec, fs2, *ackCh)
          _ => rhoSpec!("assert", (r4, "==", "[false, FSERR_REVOKED, _]"),
            "step 4: re-looked-up fs.stdin inert", *ackCh)
        }}
      }}
    }} |

    // Step 5: re-looked-up fs2.stdout post-revoke must be inert.
    contract step5(rhoSpec, @fs2, ackCh) = {{
      for(@r5 <- @fs2!?("stdout")) {{
        match r5 {{
          [false, "FSERR_REVOKED", _msg] => step6!(*rhoSpec, fs2, *ackCh)
          _ => rhoSpec!("assert", (r5, "==", "[false, FSERR_REVOKED, _]"),
            "step 5: re-looked-up fs.stdout inert", *ackCh)
        }}
      }}
    }} |

    // Step 6: re-looked-up fs2.stderr post-revoke must be inert.
    // Final success assertion fires here.
    contract step6(rhoSpec, @fs2, ackCh) = {{
      for(@r6 <- @fs2!?("stderr")) {{
        match r6 {{
          [false, "FSERR_REVOKED", _msg] =>
            rhoSpec!("assert", (true, "==", true),
              "every Fs method on a re-looked-up fs post-revoke returns FSERR_REVOKED — no ambient-access escape via registry re-import",
              *ackCh)
          _ => rhoSpec!("assert", (r6, "==", "[false, FSERR_REVOKED, _]"),
            "step 6: re-looked-up fs.stderr inert", *ackCh)
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
        "FsRevokeNoReLookupEscapeSpec".to_string(),
    )
    .expect("compile fs_revoke re-lookup spec");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("fs_revoke re-lookup spec failed");
}
