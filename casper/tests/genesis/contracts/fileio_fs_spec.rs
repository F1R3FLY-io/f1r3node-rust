//! Phase 10 Fs surface coverage.
//!
//! Complements the canonical-example spec (`fileio_examples_spec.rs`)
//! and the per-error-code matrix (`fileio_error_matrix_spec.rs`) with
//! systematic coverage of the Fs / Stdin / Stdout agents' surface
//! that would otherwise slip past both:
//!
//!   - `Fs.openFile` mode-default inference on `{}` and on non-empty
//!     Maps without a `"mode"` key.
//!   - `Fs.openFile` accepting every non-exclusive-create mode string
//!     against a `"rw"`-provisioned File bundle.  (Exclusive-create
//!     modes `"wx"` / `"w+x"` deliberately excluded — they require the
//!     target NOT to exist, and bundle validation always seeds an
//!     on-disk file.  Their success path is unreachable from a
//!     provisioned File bundle; the exclusive-create *failure* path
//!     is pinned in `fileio_error_matrix_spec::openfile_exclusive_on_existing_returns_already_exists`.)
//!   - `Stdin.default(...)` and `Stdout.default(...)` fall-through
//!     for unknown methods.  The Fs default arm is already pinned in
//!     `fileio_error_matrix_spec::fs_default_arm_returns_unsupported`.
//!
//! ## Related tests (do not duplicate here)
//!
//! - `fileio_error_matrix_spec::openfile_non_map_options_returns_bad_arg`
//! - `fileio_error_matrix_spec::openfile_unknown_bundle_name_returns_unsupported`
//! - `fileio_error_matrix_spec::openfile_on_dir_kind_returns_bad_arg`
//! - `fileio_examples_spec::fileio_stdio_caps_are_resolvable`
//! - `fileio_dir_spec::dir_openfile_mode_attenuation` — the Dir-level
//!   mode-attenuation gate (r-Dir refuses rw-File open); Fs-level
//!   attenuation is the same logic and covered transitively.

use std::collections::HashMap;

use casper::rust::genesis::contracts::fs_genesis::{
    BundleConsensusMode, BundleEntry, BundleEntryKind,
};
use casper::rust::genesis::contracts::{fs_genesis, standard_deploys};
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::GenesisBuilder;

/// Bundle a rw File target with the supplied content.  Local to this
/// file so scaffolding can evolve independently per spec.
fn bundle_file(
    content: &[u8],
    mode: &str,
) -> (
    tempfile::TempDir,
    crate::util::genesis_builder::GenesisParameters,
    String,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let file_path = dir.path().join("target.bin");
    std::fs::write(&file_path, content).expect("seed file");
    let canon = std::fs::canonicalize(&file_path).expect("canonicalize");

    let entry = BundleEntry::try_new(
        "target".to_string(),
        canon,
        BundleEntryKind::File,
        mode.to_string(),
        BundleConsensusMode::Oracular,
    )
    .expect("bundle entry construction");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.fs_bundle = vec![entry];

    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);
    (dir, params, fs_uri)
}

/// `Fs.openFile("target", {})` on a "r"-provisioned bundle succeeds
/// with `[true, cap]` — the empty options Map defaults `mode` to
/// `"r"` per Fs.rho:247.  Pins the empty-Map arm; a regression that
/// rejects `{}` as malformed would break the "no options, just give
/// me a reader" idiom that many callers use.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn openfile_empty_options_defaults_mode_to_r() {
    let (_dir, params, fs_uri) = bundle_file(b"content", "r");
    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_default_mode
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("openFile(target, {{}}) defaults mode to r", *test_default_mode)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_default_mode(rhoSpec, _, ackCh) = {{
      for(@r <- @fs!?("openFile", "target", {{}})) {{
        match r {{
          [true, _cap] => {{
            rhoSpec!("assert", (true, "==", true),
              "openFile with empty options succeeds (mode defaults to r)",
              *ackCh)
          }}
          _ => {{
            rhoSpec!("assert", (r, "==", "[true, _cap]"),
              "openFile with empty options must return [true, cap]",
              *ackCh)
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
        "FsOpenFileEmptyOptionsSpec".to_string(),
    )
    .expect("compile openfile empty options spec");
    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("openfile empty options spec failed");
}

/// `Fs.openFile("target", {"other": "value"})` — a non-empty Map
/// without a `"mode"` key — defaults `mode` to `"r"` per
/// Fs.rho:249's `{_ : _ ..._} => modeCh!("r")` arm.  Distinct from
/// `openfile_non_map_options_returns_bad_arg` (which tests a
/// non-Map options value) and `openfile_empty_options_defaults_mode_to_r`
/// (which tests the empty-Map arm).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn openfile_non_mode_options_defaults_mode_to_r() {
    let (_dir, params, fs_uri) = bundle_file(b"content", "r");
    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_no_mode_key
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("openFile with Map lacking mode key defaults to r",
        *test_no_mode_key)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_no_mode_key(rhoSpec, _, ackCh) = {{
      for(@r <- @fs!?("openFile", "target", {{"other": "value"}})) {{
        match r {{
          [true, _cap] => {{
            rhoSpec!("assert", (true, "==", true),
              "openFile with non-mode options defaults to r",
              *ackCh)
          }}
          _ => {{
            rhoSpec!("assert", (r, "==", "[true, _cap]"),
              "openFile with non-mode options must return [true, cap]",
              *ackCh)
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
        "FsOpenFileNoModeOptionsSpec".to_string(),
    )
    .expect("compile openfile no-mode-key spec");
    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("openfile no-mode-key spec failed");
}

/// `Fs.openFile("target", {"mode": M})` succeeds for every mode M
/// in the native-parser-accepted, non-exclusive-create set on a
/// `"rw"`-provisioned bundle.  Sweeps `{"r", "rw", "w", "w+", "a",
/// "a+"}` in a single test so genesis-build cost amortizes.  A
/// regression that drops any mode from Fs.rho:275's disjunction OR
/// from `mode.rs::parse_open_mode`'s match would fail here.
///
/// Exclusive-create modes `"wx"` / `"w+x"` are deliberately excluded:
/// bundle validation always seeds an on-disk file, so O_CREAT|O_EXCL
/// against a bundled target always collides — the collision failure
/// is pinned as `openfile_exclusive_on_existing_returns_already_exists`.
///
/// `"r+"` IS included on triage: the Fs.rho ↔ `parse_open_mode`
/// drift that fileio-canonical called out (Fs.rho accepting `"r+"`
/// but the native parser not matching it) is already resolved on
/// triage — `mode.rs::parse_open_mode` handles `"r+" => (ReadWrite,
/// Require)` (line 62) so POSIX `r+` semantics work via both
/// `"r+"` and the legacy `"rw"` alias.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn openfile_all_non_exclusive_modes_succeed_on_rw_bundle() {
    let (_dir, params, fs_uri) = bundle_file(b"seed", "rw");
    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_all_modes
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("openFile succeeds for every non-exclusive mode on rw bundle",
        *test_all_modes)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_all_modes(rhoSpec, _, ackCh) = {{
      for(@rR   <- @fs!?("openFile", "target", {{"mode": "r"}});
          @rRW  <- @fs!?("openFile", "target", {{"mode": "r+"}});
          @rW   <- @fs!?("openFile", "target", {{"mode": "w"}});
          @rWP  <- @fs!?("openFile", "target", {{"mode": "w+"}});
          @rA   <- @fs!?("openFile", "target", {{"mode": "a"}});
          @rAP  <- @fs!?("openFile", "target", {{"mode": "a+"}})) {{
        match [rR, rRW, rW, rWP, rA, rAP] {{
          [[true, _], [true, _], [true, _], [true, _], [true, _], [true, _]] => {{
            rhoSpec!("assert", (true, "==", true),
              "every non-exclusive mode on rw bundle returns [true, cap]",
              *ackCh)
          }}
          _ => {{
            rhoSpec!("assert",
              ([rR, rRW, rW, rWP, rA, rAP], "==",
               "[[true,_], [true,_], [true,_], [true,_], [true,_], [true,_]]"),
              "every non-exclusive mode must succeed on rw bundle",
              *ackCh)
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
        "FsOpenFileAllModesSpec".to_string(),
    )
    .expect("compile openfile all-modes spec");
    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("openfile all-modes spec failed");
}

// NOTE: fileio-canonical includes a sibling test
// `openfile_r_plus_mode_surfaces_native_parser_gap` here that pinned
// `[false, "FSERR_BAD_ARG", "unknown fopen mode \"r+\""]` as a
// spec-vs-impl drift (Fs.rho accepts "r+" but parse_open_mode on
// fileio-canonical rejected it).  Per that test's own instruction
// ("A fix that lands `\"r+\"` in `parse_open_mode` ... MUST also
// delete this pin and add `\"r+\"` to the success sweep above"), the
// pin is dropped on triage because `parse_open_mode` here already
// handles `"r+" => (AccessMode::ReadWrite, ExistPolicy::Require)`
// (mode.rs:62).  The success sweep
// (`openfile_all_non_exclusive_modes_succeed_on_rw_bundle` above)
// includes `"r+"` and covers the resolved behavior.

/// `Stdin.<unknownMethod>(...)` routes through the default arm and
/// returns exactly `[false, "FSERR_UNSUPPORTED", "unknown method or
/// not supported on Stdin"]`.  Pins Stdin.rho:791-796.  Complements
/// `fs_default_arm_returns_unsupported` (Fs default) with the same
/// invariant on Stdin — a caller doing feature detection via
/// FSERR_UNSUPPORTED must see a stable code+message.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdin_default_arm_returns_unsupported() {
    let params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);
    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_stdin_default
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Stdin.default returns FSERR_UNSUPPORTED",
        *test_stdin_default)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_stdin_default(rhoSpec, _, ackCh) = {{
      for(@[true, stdin] <- @fs!?("stdin")) {{
        for(@r <- @stdin!?("noSuchMethod", 1)) {{
          rhoSpec!("assert",
            (r, "==",
             [false, "FSERR_UNSUPPORTED",
              "unknown method or not supported on Stdin"]),
            "Stdin unknown-method returns pinned FSERR_UNSUPPORTED tuple",
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
        "StdinDefaultArmSpec".to_string(),
    )
    .expect("compile stdin default spec");
    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("stdin default spec failed");
}

/// `Stdout.<unknownMethod>(...)` → `[false, "FSERR_UNSUPPORTED",
/// "unknown method or not supported on Stdout"]`.  Pins
/// Stdout.rho:325-329.  Symmetric to `stdin_default_arm_returns_unsupported`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stdout_default_arm_returns_unsupported() {
    let params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let fs_uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);
    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  RhoSpecCh,
  fsCh,
  test_stdout_default
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("Stdout.default returns FSERR_UNSUPPORTED",
        *test_stdout_default)])
  }} |

  rl!(`{fs_uri}`, *fsCh) |
  for(@(_, fs) <- fsCh) {{
    contract test_stdout_default(rhoSpec, _, ackCh) = {{
      for(@[true, stdout] <- @fs!?("stdout")) {{
        for(@r <- @stdout!?("noSuchMethod", 1)) {{
          rhoSpec!("assert",
            (r, "==",
             [false, "FSERR_UNSUPPORTED",
              "unknown method or not supported on Stdout"]),
            "Stdout unknown-method returns pinned FSERR_UNSUPPORTED tuple",
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
        "StdoutDefaultArmSpec".to_string(),
    )
    .expect("compile stdout default spec");
    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests().await.expect("stdout default spec failed");
}

/// PB-B-3 (2026-08-24): the Fs cap is published under TWO URIs
/// (both to the same underlying `bundle+{*this}` handle):
///
/// - `rho:id:<hash>` via `insertSigned` — legacy blessed-contract
///   pattern; resolution covered by
///   `fileio_examples_spec::fileio_stdio_caps_are_resolvable`.
/// - `rho:serve:1.0.0:<FS_GENERATOR_PUB_KEY_HEX>:fs:1.0.0` via
///   `insertVersion` — Versioned Registry with semver + notify
///   support; resolution covered by this test.
///
/// End-to-end: `lookupVersion` on the versioned URN via
/// `rho:registry:v1:internal` returns the fs bundle+ Par, and
/// `stdin()` on the retrieved cap returns `[true, cap]` — proving
/// it's a live Fs, not a stripped or Nil'd value.
///
/// ## Regression envelope
///
/// A regression that drops the `insertVersion` call from the
/// composed source, or drops the `for(@_ <- insertVerRet)` await
/// (letting fs_generator terminate while the store update is
/// still in-flight), breaks this test at the lookup step: the
/// nested `for(@fs <- fsCh)` never fires, RhoSpec times out with
/// `has_finished=false`.  The companion source-scan pin
/// `compose_fs_genesis_source_calls_insert_version_for_fs` in
/// `fs_genesis.rs::tests` catches the call-site drop even before
/// this test runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fs_cap_is_resolvable_via_versioned_registry_uri() {
    let params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let pk_hex = hex::encode(standard_deploys::FS_GENERATOR_PUB_KEY.bytes.clone());

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  v1Api(`rho:registry:v1:internal`),
  RhoSpecCh,
  fsCh, fsStrCh, fsBundleAltCh,
  test_versioned_uri
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("PB-B-3 diagnostic 4-way", *test_versioned_uri)])
  }} |

  v1Api!("lookupVersion",
    "rho:serve:1.0.0:{pk_hex}:fs:1.0.0", Nil, *fsCh) |
  for(@fs <- fsCh) {{
    contract test_versioned_uri(rhoSpec, _, ackCh) = {{
      // fs is the bundle+ around the Fs unforgeable name.  Send
      // "stdin" to it and verify the reply is [true, cap].
      for (@r <- @fs!?("stdin")) {{
        match r {{
          [true, _cap] => {{
            rhoSpec!("assert", (true, "==", true),
              "versioned URN resolves to a functional Fs cap",
              *ackCh)
          }}
          _ => {{
            rhoSpec!("assert", (r, "==", "[true, _]"),
              "Fs.stdin via versioned URN must return [true, cap]",
              *ackCh)
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
        "FsVersionedRegistryLookupSpec".to_string(),
    )
    .expect("compile fs versioned lookup spec");
    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("fs versioned lookup spec failed");
}

/// PB-B-5 E2E (2026-09-02): `lookupVersion` on the buffer versioned
/// URN via `rho:registry:v1:internal` returns the Allocator cap, and
/// `allocBytes(128)` on the retrieved cap returns `[true, buf]` —
/// proving the Allocator is minted at genesis, published under the
/// serve namespace, and functionally reachable from a user deploy.
///
/// Mirrors `fs_cap_is_resolvable_via_versioned_registry_uri`
/// (PB-B-3): same lookup mechanism, same [true, cap] shape assertion.
///
/// ## Regression envelope
///
/// A regression that drops the `insertVersion` call for `alloc`
/// from the composed source (or drops the `for(@_ <-
/// allocInsertVerRet)` await, letting fs_generator terminate before
/// the store commit) breaks this test at the lookup step: the
/// nested `for(@alloc <- allocCh)` never fires, RhoSpec times out.
/// Source-scan pin `compose_fs_genesis_source_calls_insert_version_
/// for_buffer` in `fs_genesis.rs::tests` catches the call-site drop
/// even before this test runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn buffer_cap_is_resolvable_via_versioned_registry_uri() {
    let params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let pk_hex = hex::encode(standard_deploys::FS_GENERATOR_PUB_KEY.bytes.clone());

    let test_source = format!(
        r#"
new
  rl(`rho:registry:lookup`),
  v1Api(`rho:registry:v1:internal`),
  RhoSpecCh,
  allocCh,
  test_versioned_uri
in {{
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {{
    @RhoSpec!("testSuite",
      [("PB-B-5 allocator lookup", *test_versioned_uri)])
  }} |

  v1Api!("lookupVersion",
    "rho:serve:1.0.0:{pk_hex}:buffer:1.0.0", Nil, *allocCh) |
  for(@alloc <- allocCh) {{
    contract test_versioned_uri(rhoSpec, _, ackCh) = {{
      // alloc is the Allocator cap.  Call allocBytes(128) and
      // verify [true, buf] is returned — proves the cap is
      // functional (not Nil / stripped / wrongly-typed).
      for (@r <- @alloc!?("allocBytes", 128)) {{
        match r {{
          [true, _buf] => {{
            rhoSpec!("assert", (true, "==", true),
              "buffer versioned URN resolves to a functional Allocator cap",
              *ackCh)
          }}
          _ => {{
            rhoSpec!("assert", (r, "==", "[true, _]"),
              "Allocator.allocBytes via versioned URN must return [true, buf]",
              *ackCh)
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
        "BufferVersionedRegistryLookupSpec".to_string(),
    )
    .expect("compile buffer versioned lookup spec");
    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);
    spec.run_tests()
        .await
        .expect("buffer versioned lookup spec failed");
}
