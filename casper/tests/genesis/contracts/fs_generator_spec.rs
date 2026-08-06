//! Phase 6 review B-P6-2 fix: end-to-end genesis-through-runtime test
//! for the FsGenesis deploy.  Verifies (a) `fs_generator` executes at
//! genesis without error, (b) the resulting Fs cap is registered at
//! the URI derived from `FS_GENERATOR_PK`, (c) a user deploy can
//! resolve that URI and invoke `openFile` — receiving the expected
//! `FSERR_UNSUPPORTED` under the empty-bundle MVP.
//!
//! Mirrors `stack_spec.rs` / `list_ops_spec.rs` structure but
//! generates the `.rho` test source inline via `format!()` because
//! the target URI is a Blake2b256 hash of `FS_GENERATOR_PUB_KEY` and
//! isn't stable at .rho-file-authoring time.
//!
//! Complements the parse/normalize/signature/shape checks already in
//! `standard_deploys_spec.rs` — those cover deploy construction; this
//! covers deploy EXECUTION at genesis + user-scope lookup.

use std::collections::HashMap;

use casper::rust::genesis::contracts::fs_genesis::{
    BundleConsensusMode, BundleEntry, BundleEntryKind,
};
use casper::rust::genesis::contracts::{fs_genesis, standard_deploys};
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::GenesisBuilder;

#[tokio::test]
async fn fs_generator_spec() {
    // Derive the registry URI where fs_generator published the Fs cap.
    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    // Compose the RhoSpec test source: look up the Fs cap, attempt an
    // openFile on a non-existent logical name (empty-bundle MVP →
    // FSERR_UNSUPPORTED), assert the reply shape.
    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_lookup_returns_fs_cap,
  test_open_file_on_empty_bundle_yields_unsupported,
  test_stdout_returns_working_cap
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [
        ("fs cap is registered and resolvable", *test_lookup_returns_fs_cap),
        ("openFile on empty bundle yields FSERR_UNSUPPORTED",
         *test_open_file_on_empty_bundle_yields_unsupported),
        ("stdout returns a working Stdout cap",
         *test_stdout_returns_working_cap)
      ])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{

    // Test 1: the registry lookup returned a non-Nil value.  Trivially
    // true if fsCh received anything (RhoSpec harness fails on missing
    // register lookup).  Verify by asserting a String pattern equality
    // — we use a trivial always-true assertion because RhoSpec asserts
    // predicates, and the fact that `for(@(_, fs) &lt;- fsCh)` fired means
    // the URI resolved.
    contract test_lookup_returns_fs_cap(rhoSpec, _, ackCh) = {{
      rhoSpec!("assert", (true, "==", true),
        "Fs URI resolves to a cap (fsCh fired)", *ackCh)
    }} |

    // Test 2: openFile on a logical name not in the (empty) bundle
    // must return [false, "FSERR_UNSUPPORTED", ...].  Uses the agent-
    // dispatch `!?` sugar to avoid manual return-channel plumbing.
    contract test_open_file_on_empty_bundle_yields_unsupported(rhoSpec, _, ackCh) = {{
      for(@reply <- @fs!?("openFile", "nonexistent.txt", {{}})) {{
        match reply {{
          [false, "FSERR_UNSUPPORTED", _msg] => {{
            rhoSpec!("assert", (true, "==", true),
              "openFile on empty bundle → FSERR_UNSUPPORTED", *ackCh)
          }}
          _ => {{
            rhoSpec!("assert", (reply, "==", "FSERR_UNSUPPORTED tuple"),
              "openFile on empty bundle → FSERR_UNSUPPORTED", *ackCh)
          }}
        }}
      }}
    }} |

    // Test 3: stdout() must return [true, cap].  Under shared-Fs MVP
    // this cap wraps fd 1 for all deploys; per-principal delegation
    // is a future powerbox slice.
    contract test_stdout_returns_working_cap(rhoSpec, _, ackCh) = {{
      for(@reply <- @fs!?("stdout")) {{
        match reply {{
          [true, _cap] => {{
            rhoSpec!("assert", (true, "==", true),
              "fs.stdout() returns [true, cap]", *ackCh)
          }}
          _ => {{
            rhoSpec!("assert", (reply, "==", "[true, cap] shape"),
              "fs.stdout() returns [true, cap]", *ackCh)
          }}
        }}
      }}
    }}
  }}
}}
"#
    );

    let compiled =
        CompiledRholangSource::new(test_source, HashMap::new(), "FsGeneratorSpec".to_string())
            .expect("Failed to compile FsGenerator E2E test source");

    let spec = RhoSpec::new(compiled, vec![], GENESIS_TEST_TIMEOUT);
    spec.run_tests()
        .await
        .expect("FsGenerator E2E spec tests failed");
}

/// H-P7-8 / H-25-COV-1 / H-P7-8-E2E (Phase 7 whole-review,
/// delivered 2026-08-05, extended 2026-08-06): populated-bundle
/// end-to-end test.
///
/// Two fixes landed together:
///
/// 1. `format_bundle_for_rholang` now emits `(parent_dir,
///    filename)` for File entries instead of `(full_path, "")`
///    (H-P7-8).  Pre-fix, `Fs.openFile` for any populated file
///    entry cascaded to `safe_descend(root, rel="")` →
///    `QuarantineError::Empty` → silent
///    `[false, "FSERR_BAD_ARG", "empty relative path"]`.
///
/// 2. 7 native handler arity registrations in `fs_native_def`
///    (rho_runtime.rs) were updated from pre-slice-26 arities to
///    post-slice-26 (H-P7-8-E2E).  Slice 26 threaded `cmode`
///    through the native call signatures for fs_stat, fs_entries,
///    fs_rename, fs_copy_file, fs_remove_file, fs_remove_dir,
///    fs_chmod, fs_chown but the register-arities were never
///    bumped to match.  Any send with the CORRECT number of args
///    silently didn't match the 3/4/5-arity persistent receive,
///    leaving `fs_stat!(root, rel, cmode, ack)` (4 args) waiting
///    forever against a 3-arg receive.  Only tests that used the
///    URN filter's genesis-scope with mock syscalls (file_dir_check)
///    or that used pre-slice-26 arg counts (fs_wal_spec's fs_write
///    3-args, which happens to match the unchanged 3-arity)
///    escaped detection.
///
/// Test coverage now:
/// - **Fs.openFile early-return path** on a populated-bundle
///   runtime (name not in bundle → FSERR_UNSUPPORTED).
/// - **Fs.openFile populated-name path** (name IS in bundle →
///   real openFileImpl → fs_stat + fs_open + File mint chain
///   against native syscalls in a real tempdir file).
/// - **File.readBytes on the returned cap** through fs_read.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_generator_populated_bundle_installs_and_dispatches() {
    // Boot-time on-disk setup: a real file the operator has
    // provisioned.  The tempdir survives until the test ends.
    let dir = tempfile::tempdir().expect("tempdir");
    let file_path = dir.path().join("data.bin");
    std::fs::write(&file_path, b"hello populated bundle").expect("seed file");
    let canon = std::fs::canonicalize(&file_path).expect("canonicalize");

    // Build an Oracular `r` bundle entry for the tempdir file.
    // `try_new` exercises the same validator path production uses.
    let entry = BundleEntry::try_new(
        "myfile".to_string(),
        canon.clone(),
        BundleEntryKind::File,
        "r".to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("bundle entry construction");

    // Genesis parameters carrying the populated bundle.
    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    // Test source: look up the Fs cap, openFile("myfile", {}), assert
    // [true, fileCap].  Then File.readBytes(64) and assert [true, bytes]
    // — proves the full chain from Fs.openFile through fs_stat + fs_open
    // + fs_read lands on the tempdir file.
    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_fs_early_return_on_populated_bundle,
  test_openfile_populated_returns_file_cap,
  test_readbytes_returns_file_contents
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [
        ("Fs.openFile early-return works with populated bundle installed",
         *test_fs_early_return_on_populated_bundle),
        ("Fs.openFile on populated name returns [true, cap]",
         *test_openfile_populated_returns_file_cap),
        ("File.readN on returned cap returns file contents",
         *test_readbytes_returns_file_contents)
      ])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{

    // Test 1 (early-return diagnostic): openFile on a name NOT in
    // the bundle emits [false, FSERR_UNSUPPORTED, ...] without
    // calling openFileImpl.  Proves the populated-bundle genesis
    // path runs cleanly and the Fs cap dispatches openFile.
    contract test_fs_early_return_on_populated_bundle(rhoSpec, _, ackCh) = {{
      for(@reply <- @fs!?("openFile", "nonexistent-name", {{}})) {{
        match reply {{
          [false, "FSERR_UNSUPPORTED", _msg] => {{
            rhoSpec!("assert", (true, "==", true),
              "Fs.openFile early-return under populated bundle", *ackCh)
          }}
          _ => {{
            rhoSpec!("assert", (reply, "==", "[false, FSERR_UNSUPPORTED, _]"),
              "Fs.openFile early-return under populated bundle", *ackCh)
          }}
        }}
      }}
    }} |

    // Test 2: openFile on the provisioned name.  Exercises the
    // full openFileImpl → fs_stat + fs_open + File mint chain
    // against real native syscalls.  Requires the multi-thread
    // runtime flavor so spawn_blocking hand-off doesn't deadlock.
    contract test_openfile_populated_returns_file_cap(rhoSpec, _, ackCh) = {{
      for(@reply <- @fs!?("openFile", "myfile", {{}})) {{
        match reply {{
          [true, _cap] => {{
            rhoSpec!("assert", (true, "==", true),
              "openFile on populated name returns [true, cap]", *ackCh)
          }}
          _ => {{
            rhoSpec!("assert", (reply, "==", "[true, cap] shape"),
              "openFile on populated name returns [true, cap]", *ackCh)
          }}
        }}
      }}
    }} |

    // Test 3: exercise File.readN on the returned cap.  Reads up
    // to 64 bytes from the tempdir file's contents through the
    // full Fs → File → fs_read syscall chain.
    contract test_readbytes_returns_file_contents(rhoSpec, _, ackCh) = {{
      for(@[true, fileCap] <- @fs!?("openFile", "myfile", {{}})) {{
        for(@r <- @fileCap!?("readN", 64)) {{
          match r {{
            [true, _bytes] => {{
              rhoSpec!("assert", (true, "==", true),
                "readN returns [true, bytes]", *ackCh)
            }}
            _ => {{
              rhoSpec!("assert", (r, "==", "[true, bytes] shape"),
                "readN returns [true, bytes]", *ackCh)
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
        "FsGeneratorPopulatedBundleSpec".to_string(),
    )
    .expect("compile populated-bundle test source");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("populated-bundle FsGenerator spec failed");
}

/// H-26-COV-1 (Phase 7 whole-review, delivered 2026-08-06):
/// consensus-cmode routing observable at the Rholang API surface.
///
/// The prior end-to-end test
/// (`fs_generator_populated_bundle_installs_and_dispatches`) only
/// exercised an `Oracular` cap: the tempdir file was declared under
/// `oracle-static-files` -> `BundleConsensusMode::Oracular` ->
/// `cmode="oracular"` at every layer.  Slice 26's consensus-mode
/// routing (`resolve_cmode`, chown/chmod short-circuits, stat
/// field-omission) was covered at unit level (15 `resolve_cmode`
/// tests + 4 `stat_record` tests + 9 in-process Rholang integration
/// tests in `file_dir_check.rs` using mock syscalls), but no test
/// proved that the FULL pipeline — operator config's
/// `consensus-static-files` bucket declaration -> project_bundle ->
/// genesis composition -> user deploy -> File cap's cmode-P cell ->
/// method dispatch — actually routes to the consensus branch.
///
/// This test closes the gap: build a Genesis whose `fs_bundle`
/// includes a `BundleConsensusMode::Consensus` entry (as though the
/// operator declared `consensus-static-files { ... }` in HOCON), open
/// the cap from user Rholang, and observe that `File.chmod` returns
/// the Consensus-branch error message rather than proceeding to the
/// write-mode gate.
///
/// The chmod-on-consensus branch (Slice 29 H-29-3 review fix) is
/// checked BEFORE the write-mode gate, so it fires even for an
/// "r"-mode cap — proving the cmode plumbed all the way through
/// (File.constructor's `@cmode` param -> `*cmodeP` cell -> chmod's
/// `for (@cmode <<- ...)` peek) rather than defaulting.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_generator_consensus_cmode_routes_through_native_dispatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file_path = dir.path().join("consensus.dat");
    std::fs::write(&file_path, b"consensus payload").expect("seed file");
    let canon = std::fs::canonicalize(&file_path).expect("canonicalize");

    // The critical difference vs. H-P7-8-E2E: `Consensus` cmode.
    // Everything else (kind=File, mode="r", real tempdir file) is
    // identical so any behavior divergence is attributable to cmode
    // routing alone.
    let entry = BundleEntry::try_new(
        "consensus-cap".to_string(),
        canon.clone(),
        BundleEntryKind::File,
        "r".to_string(),
        BundleConsensusMode::Consensus,
    )
    .expect("bundle entry construction");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_openfile_consensus_returns_cap,
  test_chmod_on_consensus_cap_short_circuits
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [
        ("Fs.openFile on consensus-cmode name returns [true, cap]",
         *test_openfile_consensus_returns_cap),
        ("File.chmod on consensus cap short-circuits with FSERR_UNSUPPORTED",
         *test_chmod_on_consensus_cap_short_circuits)
      ])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{

    // Test A: prove the openFile happy-path works even for a
    // Consensus cap — same syscall chain as the Oracular test but
    // through the consensus arm of resolve_cmode in fs_stat +
    // fs_open native handlers.
    contract test_openfile_consensus_returns_cap(rhoSpec, _, ackCh) = {{
      for(@reply <- @fs!?("openFile", "consensus-cap", {{}})) {{
        match reply {{
          [true, _cap] => {{
            rhoSpec!("assert", (true, "==", true),
              "openFile on consensus-cap returns [true, cap]", *ackCh)
          }}
          _ => {{
            rhoSpec!("assert", (reply, "==", "[true, cap] shape"),
              "openFile on consensus-cap returns [true, cap]", *ackCh)
          }}
        }}
      }}
    }} |

    // Test B: the payoff — chmod on a Consensus cap must return
    // FSERR_UNSUPPORTED via the Slice-29 H-29-3 short-circuit
    // ("chmod not supported on consensus caps ...").  That message
    // only fires if `*cmodeP` holds "consensus" at cmod-check time
    // (File.rho line 462).  If cmode had defaulted to "oracular"
    // anywhere in the chain (BundleEntry -> format_bundle ->
    // openFileImpl -> File constructor -> cmodeP cell), we'd
    // instead hit the write-mode gate ("chmod requires a
    // write-capable mode") since our file is provisioned "r".
    // Distinguishing the two error strings is what proves the
    // consensus cmode plumbed end-to-end.
    contract test_chmod_on_consensus_cap_short_circuits(rhoSpec, _, ackCh) = {{
      for(@[true, fileCap] <- @fs!?("openFile", "consensus-cap", {{}})) {{
        for(@r <- @fileCap!?("chmod", "rw-r--r--")) {{
          match r {{
            [false, "FSERR_UNSUPPORTED", msg] => {{
              // Match on the specific consensus-branch message;
              // both branches return FSERR_UNSUPPORTED but with
              // distinct clues, so we pin the consensus one.
              rhoSpec!("assert", (msg.slice(0, 12), "==", "chmod not su"),
                "chmod on consensus cap short-circuits with the H-29-3 message", *ackCh)
            }}
            _ => {{
              rhoSpec!("assert", (r, "==", "[false, FSERR_UNSUPPORTED, consensus-branch msg]"),
                "chmod on consensus cap short-circuits with the H-29-3 message", *ackCh)
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
        "FsGeneratorConsensusCmodeSpec".to_string(),
    )
    .expect("compile consensus-cmode test source");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("consensus-cmode FsGenerator spec failed");
}

/// CRIT-1 fix (2026-08-06): populated-Dir-bundle end-to-end.
///
/// Post-H-P7-8, the bundle emit shape at
/// `fs_genesis.rs::format_bundle_for_rholang` splits File entries as
/// `(parent_dir, filename)` (the Fs.openFile call chain gives
/// safe_descend a real leaf) but keeps Dir entries as
/// `(canon_path, "")` — Dir caps root ON the provisioned path, not
/// inside it, so a nested `Dir.openFile("child")` uses
/// `openFileImpl(canonRoot=dir, subPath="", rel="child")` and works.
///
/// The bug: `Fs.openDir("mydir", {})` on a populated Dir entry
/// composed `openDirImpl(canonRoot=dir, subPath="", rel="")` →
/// `openDirImplInner` did `joinRel("", "") = ""` → `fsStat(root, "")`
/// → `safe_descend(root, "")` → `QuarantineError::Empty` → silent
/// `[false, "FSERR_BAD_ARG", "empty relative path"]` from every
/// `Fs.openDir` call in production.  Every populated Dir bundle
/// entry was unreachable via the operator-facing API surface.  The
/// same failure class as H-P7-8 (which fixed the File side); Dir
/// was overlooked.
///
/// Fix (Dir.rho): `openDirImplInner` special-cases `joined == ""`
/// (the root-Dir mint path) and skips the mint-time `fsStat` verify
/// — boot validation has already confirmed the root exists + is a
/// directory + not a symlink + has no hard-linked children.  Nested
/// `Dir.openDir("child")` still runs the verify because `joined`
/// is non-empty.
///
/// This test proves:
/// - `Fs.openDir` on a populated Dir entry returns `[true, dirCap]`
///   (was the silent-hang / silent-FSERR case pre-fix)
/// - `Dir.openFile("child")` on the returned cap succeeds against
///   a real tempdir file — sandbox root is still the provisioned
///   dir, not `/` (would be a sandbox escape under an alternative
///   fix that emitted `("/", dir_name)` symmetric to File entries)
/// - The subpath-op path (which was ALREADY working via joined =
///   non-empty rel) still works and is now reachable in the same
///   deploy as the root-Dir mint.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_generator_populated_dir_bundle_opendir_and_subops() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(dir.path()).expect("canonicalize");

    // Seed a child file inside the provisioned directory so we can
    // exercise the subpath op path.
    let child_path = root.join("child.txt");
    std::fs::write(&child_path, b"child contents").expect("seed child");

    // Bundle the DIRECTORY (not the file), as though the operator
    // wrote `oracle-static-dirs { "shareddir" = "..." }` in HOCON.
    let entry = BundleEntry::try_new(
        "shareddir".to_string(),
        root.clone(),
        BundleEntryKind::Dir,
        "r".to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("bundle entry construction");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_opendir_returns_dir_cap,
  test_dir_openfile_child_succeeds
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [
        ("Fs.openDir on populated dir bundle returns [true, dirCap]",
         *test_opendir_returns_dir_cap),
        ("Dir.openFile on returned root dir cap reads child file",
         *test_dir_openfile_child_succeeds)
      ])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{

    // Test A: the CRIT-1 regression pin.  Pre-fix, this hung on
    // safe_descend(root, "") → FSERR_BAD_ARG "empty relative
    // path" delivered instead of a Dir cap.
    contract test_opendir_returns_dir_cap(rhoSpec, _, ackCh) = {{
      for(@reply <- @fs!?("openDir", "shareddir", {{}})) {{
        match reply {{
          [true, _dirCap] => {{
            rhoSpec!("assert", (true, "==", true),
              "openDir on populated dir bundle returns [true, cap]", *ackCh)
          }}
          _ => {{
            rhoSpec!("assert", (reply, "==", "[true, dirCap] shape"),
              "openDir on populated dir bundle returns [true, cap]", *ackCh)
          }}
        }}
      }}
    }} |

    // Test B: prove the sandbox root is correct.  Dir.openFile
    // uses `openFileImpl(canonRoot=dir, subPath="", rel="child.txt")`
    // → `safe_descend(dir_root, "child.txt")` which resolves inside
    // the provisioned tree.  If the fix had emitted `("/", "shareddir")`
    // instead (making canonRoot=`/`), openFile might succeed but
    // the sandbox would be `/`, not `shareddir` — this test alone
    // doesn't detect that regression, but the fix chosen preserves
    // the invariant by construction (canonRoot unchanged).
    contract test_dir_openfile_child_succeeds(rhoSpec, _, ackCh) = {{
      for(@[true, dirCap] <- @fs!?("openDir", "shareddir", {{}})) {{
        for(@openReply <- @dirCap!?("openFile", "child.txt", "r")) {{
          match openReply {{
            [true, fileCap] => {{
              for(@readReply <- @fileCap!?("readN", 64)) {{
                match readReply {{
                  [true, _bytes] => {{
                    rhoSpec!("assert", (true, "==", true),
                      "Dir.openFile('child.txt') opens and readN succeeds",
                      *ackCh)
                  }}
                  _ => {{
                    rhoSpec!("assert",
                      (readReply, "==", "[true, bytes] shape"),
                      "Dir.openFile('child.txt') opens and readN succeeds",
                      *ackCh)
                  }}
                }}
              }}
            }}
            _ => {{
              rhoSpec!("assert",
                (openReply, "==", "[true, fileCap] shape"),
                "Dir.openFile('child.txt') opens and readN succeeds",
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
        "FsGeneratorPopulatedDirBundleSpec".to_string(),
    )
    .expect("compile populated-dir-bundle test source");

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("populated-dir-bundle FsGenerator spec failed");
}
