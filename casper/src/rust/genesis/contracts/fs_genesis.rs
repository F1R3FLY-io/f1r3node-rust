//! File I/O FIP genesis composition (slice 19).
//!
//! Assembles the File / Dir / Stream / Buffer / Stdin / Stdout / Fs
//! library agents into a single Rholang deploy that (a) binds the
//! native primitive URNs into the shared new-scope, (b) mints one Fs
//! instance with default stdio fds and an empty static bundle, and
//! (c) publishes that Fs cap via the legacy
//! `rho:registry:insertSigned:secp256k1` mechanism (same pattern as
//! Stack.rho, ListOps.rho, etc.).
//!
//! # Native URN scheme
//!
//! `rho:io:fs:native:1.0.0/*` names are private implementation detail
//! shared between `rholang::interpreter::rho_runtime` (which registers
//! them) and this module (which binds them into the FsGenesis new-
//! scope).  The FIP itself does not fix these names; treat them as
//! internal until a §Native URNs spec section pins them.
//!
//! # Publication URI
//!
//! The Fs cap is published at `rho:id:<hash-of-FS_GENERATOR_PK>` via
//! `insertSigned`, following the legacy blessed-contract pattern.  The
//! FIP's spec examples (§826, §829, §874) obtain the Fs via
//! `getFS(`rho:io:fs:1.*`)` — a Versioned Registry lookup.  Slice 19
//! does NOT publish at the versioned URI; deploys must use the derived
//! `rho:id:...` returned by `fs_genesis_uri()`.  See MVP simplification
//! #4 below.
//!
//! # MVP simplifications (documented deferrals)
//!
//! 1. **Shared-Fs model.**  A single Fs instance is published at the
//!    registry URI derived from FS_GENERATOR_PK.  All deploys look up
//!    the same Fs handle and thus share the same static bundle and
//!    stdio fds; individual `openFile` / `openDir` calls fresh-mint
//!    File / Dir caps (slice 27 reverted the earlier memoization —
//!    FIP §Fresh-mint per open).  Spec §867 sketches per-principal
//!    Fs instances from the powerbox as the eventual production shape
//!    — IF a future powerbox slice lands that shape it would require
//!    runtime changes to the URN resolver (each grantee sees a
//!    distinct Fs cap).  Post-PB-M-1 narrowing, the per-principal
//!    delegation is a candidate design rather than a scheduled slice:
//!    shards may ship shared-Fs permanently or roll their own
//!    delegation mechanism.
//!
//! 2. **Empty static bundle.**  The published Fs has `bMap = {}`, so
//!    `openFile` / `openDir` return `FSERR_UNSUPPORTED` for every
//!    logical name.  Static provisioning (spec §846, config-driven
//!    bundle) is Phase 7.  Stdio methods (`stdin` / `stdout` /
//!    `stderr`) work.
//!
//! 3. **Shared stdio fds.**  `stdin` / `stdout` / `stderr` mint fresh
//!    Stdin / Stdout caps that wrap the host-shared fds (0 / 1 / 2).
//!    All deploys on a node see the same host stream.  Spec §Stdin
//!    sketches per-principal stdio delegation but the FIP leaves it
//!    out of scope; same posture as #1.
//!
//! 4. **`rho:id:...` publication (not `rho:io:fs:1.*`).**  The FIP's
//!    spec examples use `rho:io:fs:1.*` (a Versioned Registry URN
//!    shape) to look up the Fs cap.  Slice 19 uses the legacy
//!    `insertSigned` -> `rho:id:...` path because:
//!      - the Versioned Registry URN scheme is still being hashed
//!        out (`rho:lang:*`, `rho:ns:*` namespaces aren't yet pinned),
//!      - the Fs cap IS deploy-visible via the derived URI returned
//!        by `fs_genesis_uri()`, which deploys bind directly, and
//!      - `insertSigned` has a 7-year production track record in
//!        this codebase; the equivalent VRegistry-publication primitives
//!        aren't yet integrated into the compiler's genesis path.
//!    A follow-up slice could add a VR-publication wrapper deploy
//!    that `getRegistry("rho:io:fs:1.*")` resolves to this same cap;
//!    the pre-existing `insertSigned` publication stays as the
//!    source of truth.
//!
//! 5. **`consensus_static_files` validation.**  Slice 24's
//!    `merge_and_validate` guarantees the file-path uniqueness
//!    invariants that `BundleEntry::try_new` re-enforces; this
//!    module trusts those upstream invariants + adds defense-in-
//!    depth char-set + absolute-path + relative-shape (Consensus-
//!    only) checks.  See the inline try_new comment for the
//!    bundle checks.
//!
//! 6. **`Buffer` / `Allocator` published under the serve namespace
//!    (PB-B-5, 2026-09-02).**  The composed source splices in
//!    Buffer.rho's body AND mints one shared `Allocator` instance
//!    at genesis, publishing it via `insertVersion("serve", "buffer",
//!    "1.0.0", alloc, ...)` alongside the Fs cap's insertVersion.
//!    Callers resolve the Allocator via `lookupVersion` on
//!    `rho:serve:1.0.0:<FS_GENERATOR_PUB_KEY_HEX>:buffer:1.0.0` and
//!    invoke `allocBytes(n)` / `allocRows(m, innerN, unit)` to mint
//!    Buffer / Rows instances.  This unblocks the buffer-taking File
//!    methods (`readInto` / `writeFrom` / `readLineInto` /
//!    `readLinesInto` + their arity-N+1 variants) for user deploys.
//!    Per-principal Allocator delegation via a real Powerbox is a
//!    candidate follow-up (would mirror the PB-B-3 → PB-B-5 shape)
//!    but is not scheduled — post-PB-M-1 narrowing left it as an
//!    optional shard-authored capability rather than a core slice.
//!    Spec §Table 4's aspirational `rho:lang:buffer:1.0.0` shape
//!    awaits the same URN-parser extension slice that would add
//!    `rho:io:fs:1.0.0` aliasing.
//!
//! 7. **`ConsensusMode` per-cap plumbing (Phase 7 slice 26).**  Each
//!    bundle entry carries a `consensus_mode` (`Oracular` / `Consensus`)
//!    derived from its config bucket.  The mode is emitted as the 5th
//!    tuple element in the composed source, threaded through Fs.rho →
//!    File/Dir constructor → agent state cell → passed explicitly to
//!    the native `fs_chown` / `fs_stat` / `fs_entries` handlers on
//!    every dispatch.  The runtime-wide `ConsensusMode::default()`
//!    remains as a fallback for callers that omit the arg but is no
//!    longer relied on by the library agents.  `chown`
//!    short-circuits (returns `FSERR_UNSUPPORTED`) and `stat`/
//!    `entries` omit host-transient fields (`mtime`/`ctime`/`atime`/
//!    `owner`/`group`) when the cap's mode is `Consensus`.

use std::path::PathBuf;

/// Static-provisioning bundle-entry kind.  Matches slice 23's
/// `EntryKind` shape but redefined here to avoid a `node → casper`
/// dependency direction issue (casper is a lower-level crate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BundleEntryKind {
    File,
    Dir,
}

/// Per-cap consensus mode (plan §369, spec §Storage cases 1-6).
/// Determines whether host-transient fields (`mtime`, `ctime`,
/// `atime`, `owner`, `group`) appear in `stat`/`entries` records
/// and whether `chown` succeeds.  Slice 26 threads this from the
/// bundle tuple through Fs.rho → File/Dir agent state → native
/// dispatch, replacing the runtime-wide `ConsensusMode::default()`
/// fallback that Phase 1 stubbed in.
///
/// String encoding at the Rholang boundary: `"oracular"` /
/// `"consensus"` — chosen for readability in composed sources and
/// symmetry with the operator's config-bucket prefixes
/// (`oracle-static-*` / `consensus-static-*`).  `as_str()` and
/// `parse_str()` bracket the serde boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BundleConsensusMode {
    Oracular,
    Consensus,
}

impl BundleConsensusMode {
    // M-26-3 review fix: single-source the cmode strings from
    // `rholang/interpreter/io/mod.rs` — the composer emits them into
    // the tuplespace, the native `resolve_cmode` matches them, and
    // both must agree byte-for-byte or the composed source becomes
    // unroutable at the syscall boundary.  A drift-assertion test
    // below pins the pair.
    pub const ORACULAR_STR: &'static str = rholang::rust::interpreter::io::CMODE_ORACULAR_STR;
    pub const CONSENSUS_STR: &'static str = rholang::rust::interpreter::io::CMODE_CONSENSUS_STR;

    pub fn as_str(self) -> &'static str {
        match self {
            BundleConsensusMode::Oracular => Self::ORACULAR_STR,
            BundleConsensusMode::Consensus => Self::CONSENSUS_STR,
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            Self::ORACULAR_STR => Some(BundleConsensusMode::Oracular),
            Self::CONSENSUS_STR => Some(BundleConsensusMode::Consensus),
            _ => None,
        }
    }
}

/// One entry in the static-provisioning bundle handed to
/// `fs_generator` (Phase 7 slice 25).  Projected from the merged
/// `FileIoProvisioning` produced by slice 24's `merge_and_validate`.
///
/// `consensus_mode` (slice 26): derived by `project_bundle` from
/// the bucket a config entry came from — `oracle-static-*` →
/// `Oracular`, `consensus-static-*` → `Consensus` — and emitted as
/// the 5th tuple element in `format_bundle_for_rholang`.  Fs.rho
/// threads it through openFileImpl/openDirImpl into the File/Dir
/// constructor's state cell, and File/Dir's chown/stat/entries
/// methods pass it back to the native handler as an explicit arg.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BundleEntry {
    pub logical_name: String,
    pub canon_path: PathBuf,
    pub kind: BundleEntryKind,
    pub mode: String,
    pub consensus_mode: BundleConsensusMode,
}

impl BundleEntry {
    /// Fallible constructor (H-25-3 slice-25 review fix) that
    /// re-runs the invariants slice 21/22/23 enforce upstream, as
    /// defense in depth against programmatic-construction bypasses.
    ///
    /// Checks:
    /// - `logical_name`, `mode`, and `canon_path.to_str()` contain
    ///   no NUL, control chars, DEL, C1 controls, BOM, RTL
    ///   overrides, or line separators (would break the Rholang
    ///   lexer or produce log-injection / visual-confusable
    ///   hazards).
    /// - `canon_path` is UTF-8 (Fs.rho's `bMap` keys and tuple
    ///   values are Rholang strings).
    /// - `canon_path` is absolute (Fs.rho expects `canonRoot` to
    ///   be an absolute host path).
    ///
    /// Slice 24's `project_bundle` uses this constructor so all
    /// projection-path entries are validated.  Direct field
    /// initialization is still permitted (`pub` fields) for tests
    /// and future callers who have their own validation pipeline;
    /// prefer `try_new` when constructing from operator-derived
    /// data.
    pub fn try_new(
        logical_name: String,
        canon_path: PathBuf,
        kind: BundleEntryKind,
        mode: String,
        consensus_mode: BundleConsensusMode,
    ) -> Result<Self, String> {
        // Reject empties (should be caught upstream).
        if logical_name.is_empty() {
            return Err("logical_name is empty".into());
        }
        if mode.is_empty() {
            return Err("mode is empty".into());
        }
        if canon_path.as_os_str().is_empty() {
            return Err("canon_path is empty".into());
        }
        // UTF-8.
        let path_str = canon_path
            .to_str()
            .ok_or_else(|| format!("canon_path {canon_path:?} is not valid UTF-8"))?;
        // Absolute.
        if !canon_path.is_absolute() {
            return Err(format!("canon_path {canon_path:?} is not absolute"));
        }
        // Forbidden chars (NUL, C0/C1, DEL, BOM, RTL overrides,
        // line separators).  Same set as slice 21 / 22 enforce.
        reject_char_set(&logical_name).map_err(|d| format!("logical_name: {d}"))?;
        reject_char_set(path_str).map_err(|d| format!("canon_path: {d}"))?;
        reject_char_set(&mode).map_err(|d| format!("mode: {d}"))?;
        // Consensus `logical_name` MUST be a well-formed relative
        // path — no absolute root, no Windows prefix, no ParentDir
        // segments (2026-09-02, defense-in-depth per the
        // consensus-reexecute plan's Deferred item).  Rationale:
        // `project_bundle_per_validator` (in the test harness AND
        // in the production RootIdentityRegistry provisioning
        // flow) computes `on_disk_target = <subdir>.join(&logical_
        // name)`.  `PathBuf::join` REPLACES self when the argument
        // is absolute, so a malicious `logical_name = "/etc/
        // passwd"` in an operator's bundle config would land the
        // projection at `/etc/passwd` instead of under the
        // validator subdir — a straight arbitrary-file overwrite
        // via operator config compromise.  `..` segments similarly
        // escape the subdir via lexical traversal.
        //
        // Oracular entries don't go through per-validator subdir
        // projection (they stay at their operator-supplied
        // canon_path), so this check is Consensus-only.  Operator
        // config trust is assumed today (per f1r3node's threat
        // model where the node is the only software on the
        // machine), but this rejection surfaces a config bug or
        // supply-chain attack as a genesis-build failure rather
        // than a silent projection escape.
        if consensus_mode == BundleConsensusMode::Consensus {
            for component in std::path::Path::new(&logical_name).components() {
                match component {
                    std::path::Component::Normal(_) | std::path::Component::CurDir => {}
                    std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_) => {
                        return Err(format!(
                            "logical_name {logical_name:?} contains illegal component \
                             {component:?} for Consensus entry (must be a well-formed \
                             relative path with no `..`, absolute root, or Windows \
                             prefix segments — projection at `subdir.join(&logical_name)` \
                             would escape the validator subdir)"
                        ));
                    }
                }
            }
        }
        Ok(BundleEntry {
            logical_name,
            canon_path,
            kind,
            mode,
            consensus_mode,
        })
    }
}

/// Mirror of `node::configuration::file_io_provisioning::reject_forbidden_chars`.
/// Duplicated here (rather than re-imported) to keep `casper`'s
/// dependency footprint independent of `node` — casper is the lower
/// crate.  The set MUST stay in sync with `reject_forbidden_chars`
/// in node.
fn reject_char_set(value: &str) -> Result<(), String> {
    if let Some((i, c)) = value.char_indices().find(|(_, c)| {
        let cp = *c as u32;
        cp < 0x20
            || cp == 0x7F
            || (0x80..=0x9F).contains(&cp)
            || matches!(
                cp,
                0x200E | 0x200F
                | 0x202A..=0x202E
                | 0x2028 | 0x2029
                | 0xFEFF
                | 0x2066..=0x2069
            )
    }) {
        return Err(format!(
            "forbidden control character U+{:04X} at byte {i}",
            c as u32
        ));
    }
    Ok(())
}

/// Bundle-relative root prefix for Consensus-mode entries under
/// Shape A (2026-08-31).  See auto-memory
/// `fileio_consensus_fs_shape_a.md`.
///
/// Consensus-mode bundle entries emit `/@bundle/<logical_name>` in
/// the `canonRoot` slot of the Rholang tuple instead of the
/// operator-configured absolute `canon_path`.  Rationale:
///   1. **Genesis-hash stability**: the composed Rholang source bytes
///      are validator-independent, so every validator computes the
///      same genesis-block hash even when their on-disk staging
///      directories differ.
///   2. **Per-validator resolution at syscall time**: each validator's
///      `RootIdentityRegistry` holds a `register_with_remap(logical
///      = "/@bundle/...", on_disk = "<validator_subdir>/...", id)`
///      entry, and every fs handler's `resolve_or_identity(canonRoot)`
///      call maps the bundle-relative form to the validator's
///      own on-disk absolute before `safe_descend_verified`.
///   3. **Safe fall-through**: `/@bundle` is absolute-shaped, so a
///      handler that forgets to route through the resolver hands the
///      unresolved logical path to `safe_descend_verified`, which
///      fails with ENOENT — a clean detection signal, not a
///      cwd-relative surprise like a bare `target/data.bin` form
///      would produce.
///
/// Oracular-mode bundle entries are unchanged and continue to emit
/// the absolute `canon_path` (per the Shape A decision "non-Consensus
/// paths untouched" — see auto-memory line 37-38).
pub const BUNDLE_ROOT_PREFIX: &str = "/@bundle";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_entry_try_new_rejects_relative_canon_path() {
        let err = BundleEntry::try_new(
            "name".into(),
            PathBuf::from("relative/path"),
            BundleEntryKind::File,
            "mode".into(),
            BundleConsensusMode::Oracular,
        )
        .unwrap_err();
        assert!(err.contains("not absolute"), "unexpected error: {err}");
    }

    #[test]
    fn bundle_entry_try_new_rejects_control_chars_in_mode() {
        let err = BundleEntry::try_new(
            "name".into(),
            PathBuf::from("/abs/path"),
            BundleEntryKind::File,
            "mode\0with-nul".into(),
            BundleConsensusMode::Oracular,
        )
        .unwrap_err();
        assert!(err.contains("mode: forbidden"), "unexpected error: {err}");
    }

    #[test]
    fn bundle_entry_try_new_accepts_oracular_absolute_logical_name() {
        // Oracular bundle entries don't go through per-validator
        // projection; absolute logical_name is permitted.
        let entry = BundleEntry::try_new(
            "/host/absolute".into(),
            PathBuf::from("/abs/path"),
            BundleEntryKind::File,
            "mode".into(),
            BundleConsensusMode::Oracular,
        )
        .expect("oracular absolute logical_name accepted");
        assert_eq!(entry.logical_name, "/host/absolute");
    }

    #[test]
    fn bundle_entry_try_new_rejects_consensus_absolute_logical_name() {
        // Consensus logical_name MUST be a well-formed relative
        // path — subdir.join(absolute) would escape the per-
        // validator projection subdir.
        let err = BundleEntry::try_new(
            "/etc/passwd".into(),
            PathBuf::from("/abs/path"),
            BundleEntryKind::File,
            "mode".into(),
            BundleConsensusMode::Consensus,
        )
        .unwrap_err();
        assert!(err.contains("illegal component"), "unexpected error: {err}");
    }

    #[test]
    fn bundle_entry_try_new_rejects_consensus_parent_dir_segment() {
        let err = BundleEntry::try_new(
            "foo/../bar".into(),
            PathBuf::from("/abs/path"),
            BundleEntryKind::File,
            "mode".into(),
            BundleConsensusMode::Consensus,
        )
        .unwrap_err();
        assert!(err.contains("illegal component"), "unexpected error: {err}");
    }

    #[test]
    fn consensus_mode_strings_match_rholang_side() {
        // Single-source pin: fs_genesis emits these strings into
        // the composed source; rholang::io::resolve_cmode matches
        // them.  Both source from the same constants — drift
        // breaks every bundle entry's cmode slot at the syscall
        // boundary.
        assert_eq!(BundleConsensusMode::ORACULAR_STR, "oracular");
        assert_eq!(BundleConsensusMode::CONSENSUS_STR, "consensus");
        assert_eq!(
            BundleConsensusMode::parse_str("oracular"),
            Some(BundleConsensusMode::Oracular)
        );
        assert_eq!(
            BundleConsensusMode::parse_str("consensus"),
            Some(BundleConsensusMode::Consensus)
        );
        assert_eq!(BundleConsensusMode::parse_str("CONSENSUS"), None);
    }

    #[test]
    fn bundle_root_prefix_pinned() {
        assert_eq!(BUNDLE_ROOT_PREFIX, "/@bundle");
    }
}
