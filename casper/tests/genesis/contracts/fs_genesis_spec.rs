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
