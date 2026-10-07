use casper::rust::genesis::contracts::fs_genesis;
use rholang::rust::interpreter::io;

/// Cross-crate drift pin for the versioned FS native URN prefix.
///
/// Two source-of-truth constants carry `"rho:io:fs:native:1.0.0/"`:
///
///   * `rholang::rust::interpreter::io::FS_NATIVE_URN_PREFIX_VERSIONED`
///     — used by `rho_runtime::fs_handlers_to_definitions` when it
///     registers each `FS_HANDLERS` entry's URN into the dispatch
///     table.
///   * `casper::rust::genesis::contracts::fs_genesis::
///     FS_NATIVE_URN_PREFIX` — used by `compose_fs_genesis_source`
///     when it composes the FsGenesis Rholang source's top-level
///     `new`-clause bindings (`fs<Xyz>(\`rho:io:fs:native:1.0.0/
///     <suffix>\`)`).
///
/// A future Phase 1 hotfix bumping to `1.0.1` must edit BOTH
/// constants in lockstep.  A drift between them silently breaks
/// FsGenesis dispatch: the composed Rholang source would bind URNs
/// at one version while the dispatch table registers them at
/// another, so every FsGenesis `fs<Xyz>!(...)` call would hit an
/// unregistered URN and stall at genesis composition.
#[test]
fn fs_native_urn_versioned_prefix_matches_rholang() {
    assert_eq!(
        fs_genesis::FS_NATIVE_URN_PREFIX,
        io::FS_NATIVE_URN_PREFIX_VERSIONED,
        "versioned FS native URN prefix drift between casper \
         (fs_genesis::FS_NATIVE_URN_PREFIX = {:?}) and rholang \
         (io::FS_NATIVE_URN_PREFIX_VERSIONED = {:?}).  A hotfix \
         bumping the version must edit both constants.",
        fs_genesis::FS_NATIVE_URN_PREFIX,
        io::FS_NATIVE_URN_PREFIX_VERSIONED,
    );
}

/// The versioned prefix must be a strict superset of the shorter
/// filter prefix the reducer checks in `eval_new`.  The invariant
/// is also enforced at compile time inside rholang (const-assertion
/// alongside `FS_NATIVE_URN_PREFIX_VERSIONED`); this test pins it
/// from the casper side as defense-in-depth across the two-crate
/// boundary.
#[test]
fn fs_native_versioned_prefix_starts_with_filter_prefix() {
    assert!(
        io::FS_NATIVE_URN_PREFIX_VERSIONED.starts_with(io::FS_NATIVE_URN_PREFIX),
        "FS_NATIVE_URN_PREFIX_VERSIONED ({:?}) must start with \
         FS_NATIVE_URN_PREFIX ({:?}); otherwise URNs registered \
         under the versioned prefix would bypass the reducer's \
         filter_fs_native_urns gate.",
        io::FS_NATIVE_URN_PREFIX_VERSIONED,
        io::FS_NATIVE_URN_PREFIX,
    );
    assert!(
        fs_genesis::FS_NATIVE_URN_PREFIX.starts_with(io::FS_NATIVE_URN_PREFIX),
        "casper's fs_genesis::FS_NATIVE_URN_PREFIX ({:?}) must \
         start with rholang's io::FS_NATIVE_URN_PREFIX ({:?}); \
         otherwise the FsGenesis-bound URNs would not match the \
         reducer's filter gate.",
        fs_genesis::FS_NATIVE_URN_PREFIX,
        io::FS_NATIVE_URN_PREFIX,
    );
}

/// Regression gate for the fresh-reducer construction / disable-guard
/// discipline.  Addresses slice-5.43 reviewer observation (3): the
/// filter toggle lives in three replay-adjacent paths (play, state-
/// replay, reporting) and the construction defaults the flag to TRUE
/// in two sites.  A future refactor that adds a new fresh-reducer
/// construction site (e.g., a per-RPC `report_deploy_cost` or a
/// future diagnostic replay path) MUST also wrap the enclosing
/// replay/genesis loop with `FsNativeFilterGuard::disable` so the
/// FsGenesis-blessed URNs resolve the same way they do at the main
/// play / state-replay / reporting paths.
///
/// This test pins the current counts: 2 construction sites, 3 guard
/// invocations.  If either count changes, the author of the change
/// is forced to decide: add the guard at the new site (and bump the
/// count), OR document why the new site is state-execution-only
/// (filter stays TRUE at default — no guard needed) and bump the
/// construction count only.
///
/// Enumeration is grep-based over the shipped sources via
/// `std::fs::read_to_string` + `CARGO_MANIFEST_DIR` (matches the
/// cross-file read pattern used by `compose_fs_genesis_source`
/// drift tests).
#[test]
fn fs_native_filter_guard_discipline_pinned_across_crates() {
    const CONSTRUCTION_NEEDLE: &str =
        "filter_fs_native_urns: Arc::new(std::sync::atomic::AtomicBool::new(true))";
    const GUARD_NEEDLE: &str = "FsNativeFilterGuard::disable";

    // Rholang-side construction sites.  Current (slice 5.44):
    //   * `rholang/src/rust/interpreter/reduce.rs::DebruijnInterpreter::new`
    //     — test fixtures + ReportingRuntime path
    //   * `rholang/src/rust/interpreter/rho_runtime.rs::setup_reducer`
    //     — main RhoRuntimeImpl path (RuntimeManager, create_rho_runtime)
    const EXPECTED_CONSTRUCTION_COUNT: usize = 2;
    let rholang_files: &[&str] = &[
        "/../rholang/src/rust/interpreter/reduce.rs",
        "/../rholang/src/rust/interpreter/rho_runtime.rs",
    ];
    let mut construction_count = 0usize;
    for rel in rholang_files {
        let path = format!("{}{}", env!("CARGO_MANIFEST_DIR"), rel);
        let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
        construction_count += src.matches(CONSTRUCTION_NEEDLE).count();
    }
    assert_eq!(
        construction_count, EXPECTED_CONSTRUCTION_COUNT,
        "Fresh-reducer construction site count drift: found {} \
         occurrences of `{}` across rholang sources but expected \
         {}.  A new site was added or removed.  Policy: every \
         fresh-reducer construction MUST default filter_fs_native_urns \
         to TRUE (reject fs URNs — state-execution posture) AND the \
         enclosing replay/genesis loop MUST wrap the deploys with \
         `FsNativeFilterGuard::disable` if it is a genesis / replay / \
         reporting path.  If the new site is state-execution-only, \
         bump EXPECTED_CONSTRUCTION_COUNT in this test.  If it is a \
         replay-adjacent path, also wire the guard and bump \
         EXPECTED_GUARD_COUNT below.",
        construction_count, CONSTRUCTION_NEEDLE, EXPECTED_CONSTRUCTION_COUNT,
    );

    // Casper-side guard-invocation sites.  Current (slice 5.44):
    //   * `casper/src/rust/rholang/runtime.rs::play_deploys_for_genesis`
    //     (slice 5.33) — play path.
    //   * `casper/src/rust/rholang/replay_runtime.rs::replay_compute_state`
    //     (slice 5.35, is_genesis=true) — state-replay path.
    //   * `casper/src/rust/reporting_casper.rs::replay_deploys` (slice
    //     5.43, !with_cost_accounting) — reporting-replay path.
    const EXPECTED_GUARD_COUNT: usize = 3;
    let casper_files: &[&str] = &[
        "/src/rust/rholang/runtime.rs",
        "/src/rust/rholang/replay_runtime.rs",
        "/src/rust/reporting_casper.rs",
    ];
    let mut guard_count = 0usize;
    for rel in casper_files {
        let path = format!("{}{}", env!("CARGO_MANIFEST_DIR"), rel);
        let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
        guard_count += src.matches(GUARD_NEEDLE).count();
    }
    assert_eq!(
        guard_count, EXPECTED_GUARD_COUNT,
        "FsNativeFilterGuard::disable invocation count drift: found \
         {} occurrences across casper sources but expected {}.  A \
         replay-adjacent path lost or gained a filter toggle.  \
         Policy: every path that constructs a fresh reducer AND \
         then replays genesis-blessed deploys MUST toggle the filter \
         off (otherwise FsGenesis-bound URNs trip \
         `filter_fs_native_urns` and the replay fails).  If a new \
         replay-adjacent path was intentionally added, bump \
         EXPECTED_GUARD_COUNT in this test.  If the delta is \
         accidental, re-add the guard before merging.",
        guard_count, EXPECTED_GUARD_COUNT,
    );
}
