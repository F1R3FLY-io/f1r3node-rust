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

use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::utils::{new_etuple_par, new_gint_par};
use prost::Message;
use rholang::rust::interpreter::registry::registry::Registry;

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

/// Format a bundle as a Rholang map literal suitable for splicing
/// into the `Fs!?(0, 1, 2, <bundle>)` position of the composed
/// source.  Produces:
///
/// ```text
/// {"logical/name": ("/canon/path", "", "r", "file", "oracular"),
///  "cap":         ("/@bundle", "cap", "rw", "file", "consensus"), ...}
/// ```
///
/// The tuple shape matches Fs.rho's `bMap.get(n)` match pattern
/// `(canonRoot, rel, provisioned, kind, consensusMode)` (slice 26).
/// `consensusMode` is `"oracular"` or `"consensus"` per
/// `BundleConsensusMode::as_str`.
///
/// Root-slot semantics (Shape A, 2026-08-31 — see
/// `BUNDLE_ROOT_PREFIX` docs and auto-memory
/// `fileio_consensus_fs_shape_a.md`):
///   - **Oracular entries**: unchanged pre-Shape-A behavior — the
///     root slot is derived from the absolute `canon_path` (parent
///     directory for File entries, the path itself for Dir entries).
///     Operators sync Oracular-mode staging paths across validators,
///     so the composed source is byte-identical without further
///     transformation.
///   - **Consensus entries**: the root slot is `/@bundle/<X>` where
///     `X` derives from `logical_name` using the same File-vs-Dir
///     split as Oracular (parent-segments for File, whole
///     `logical_name` for Dir).  The Rholang side stays oblivious to
///     the on-disk absolute; each validator's registry resolves
///     `/@bundle/<X>` to `<validator_subdir>/<X>` at syscall time.
///
/// Deterministic output: entries sorted by logical name so the
/// composed source is byte-identical across runs (required for
/// genesis-block consensus).
pub fn format_bundle_for_rholang(bundle: &[BundleEntry]) -> String {
    if bundle.is_empty() {
        return "{}".to_string();
    }
    let mut sorted: Vec<&BundleEntry> = bundle.iter().collect();
    sorted.sort_by(|a, b| a.logical_name.cmp(&b.logical_name));

    // M-25-6 slice-25 review fix: assert no duplicate logical
    // names.  Slice 24's merge guarantees per-bucket uniqueness;
    // slice 23's cross-bucket check (M-25-7) covers the remaining
    // case.  A duplicate here would silently overwrite in the
    // Rholang map — panic before emitting.
    for window in sorted.windows(2) {
        assert!(
            window[0].logical_name != window[1].logical_name,
            "format_bundle_for_rholang: duplicate logical name `{}` — \
             slice-23 cross-bucket-name check should have caught this",
            window[0].logical_name
        );
    }

    let mut out = String::from("{");
    for (i, entry) in sorted.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        // H-25-4 slice-25 review fix: require UTF-8 paths.  Slice
        // 21 and slice 22 both enforce; a non-UTF-8 path here
        // indicates programmatic bypass.  Panic rather than emit
        // U+FFFD (which would silently open a different file at
        // runtime).
        let path_str = entry.canon_path.to_str().unwrap_or_else(|| {
            panic!(
                "format_bundle_for_rholang: non-UTF-8 canon_path {:?}; \
                 upstream validators should have rejected this input",
                entry.canon_path
            )
        });
        // Slice 30c H-P7-8 fix: split the (canonRoot, rel) tuple
        // differently by kind so `Fs.openFile`'s downstream
        // `safe_descend` has a leaf to walk.
        //
        // Pre-fix (all entries): emitted `(canon_path, "")`.  For
        // FILE entries, that made `openFileImplInner` call
        // `fs_stat(canon_path, "", ...)` → `safe_descend(root, "")`
        // → `QuarantineError::Empty` → "empty relative path".
        // Every consensus/oracle-static-file entry silently failed
        // to open in production.
        //
        // Post-fix:
        //   - FILE entries: emit `(parent_dir, filename)`.
        //     `fs_stat(parent, filename, ...)` gives safe_descend a
        //     real leaf; the syscall lands on the file.
        //   - DIR  entries: keep `(canon_path, "")` — Dir caps root
        //     ON the provisioned path, not inside it.  Nested
        //     `Dir.openFile("child")` uses `openFileImpl(canonRoot=dir,
        //     subPath="", rel="child", ...)` which is already correct.
        //
        // The operator-facing bundle shape is unchanged (still
        // `{"logical": (root, rel, mode, kind, cmode)}`); only the
        // interpretation of the (root, rel) split differs by kind.
        //
        // Shape A (2026-08-31): for Consensus-mode entries the root
        // slot derives from `logical_name` prefixed with
        // `BUNDLE_ROOT_PREFIX` instead of from the operator's
        // absolute `canon_path`.  This keeps the composed Rholang
        // source validator-independent (genesis-hash stable) while
        // each validator resolves `/@bundle/...` against its own
        // on-disk staging directory at syscall time.  Oracular
        // entries continue to emit the canon_path split — the plan
        // deliberately leaves non-Consensus paths untouched (see
        // auto-memory `fileio_consensus_fs_shape_a.md`).
        let (root_str, rel_str) = match (entry.kind, entry.consensus_mode) {
            (BundleEntryKind::File, BundleConsensusMode::Oracular) => {
                let parent = entry
                    .canon_path
                    .parent()
                    .and_then(|p| p.to_str())
                    .unwrap_or_else(|| {
                        panic!(
                            "format_bundle_for_rholang: file entry `{}` has no parent \
                         directory or non-UTF-8 parent; canon_path = {:?}.  \
                         Slice 25 requires absolute paths so this indicates \
                         upstream validator drift.",
                            entry.logical_name, entry.canon_path
                        )
                    });
                let filename = entry
                    .canon_path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or_else(|| {
                        panic!(
                            "format_bundle_for_rholang: file entry `{}` has no \
                             file_name component or non-UTF-8 name; canon_path = {:?}",
                            entry.logical_name, entry.canon_path
                        )
                    });
                (parent.to_string(), filename.to_string())
            }
            (BundleEntryKind::Dir, BundleConsensusMode::Oracular) => {
                (path_str.to_string(), String::new())
            }
            (BundleEntryKind::File, BundleConsensusMode::Consensus) => {
                // Shape A: split `logical_name` at its last `/` and
                // prepend `BUNDLE_ROOT_PREFIX` to the parent segment,
                // mirroring the Oracular File-split shape.  A bare
                // logical name (no `/`) yields `(BUNDLE_ROOT_PREFIX,
                // logical_name)` — the whole file lives directly under
                // the validator's bundle root.
                let logical_path = std::path::Path::new(&entry.logical_name);
                let filename = logical_path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or_else(|| {
                        panic!(
                            "format_bundle_for_rholang: Consensus file entry `{}` \
                             has no file_name component; logical_name must not end \
                             in `/` or be `.`/`..`.  Upstream validation should \
                             have rejected this.",
                            entry.logical_name
                        )
                    });
                let parent_rel = logical_path.parent().and_then(|p| p.to_str()).unwrap_or("");
                let root = if parent_rel.is_empty() {
                    BUNDLE_ROOT_PREFIX.to_string()
                } else {
                    format!("{BUNDLE_ROOT_PREFIX}/{parent_rel}")
                };
                (root, filename.to_string())
            }
            (BundleEntryKind::Dir, BundleConsensusMode::Consensus) => {
                // Shape A: Dir caps root ON the bundled directory
                // itself, so the whole `logical_name` sits under
                // `BUNDLE_ROOT_PREFIX`.  Nested `Dir.openFile("child")`
                // hands `("/@bundle/<logical_name>", "", "child")`
                // to the handler; the resolver joins the bundle root
                // to the validator's on-disk staging dir before
                // safe_descend.
                (
                    format!("{BUNDLE_ROOT_PREFIX}/{}", entry.logical_name),
                    String::new(),
                )
            }
        };
        let name = rholang_string_escape(&entry.logical_name);
        let root = rholang_string_escape(&root_str);
        let rel = rholang_string_escape(&rel_str);
        let mode = rholang_string_escape(&entry.mode);
        let kind = match entry.kind {
            BundleEntryKind::File => "file",
            BundleEntryKind::Dir => "dir",
        };
        let cmode = entry.consensus_mode.as_str();
        // ("canonRoot", "rel", "provisioned", "kind", "cmode") —
        // slice 30c H-P7-8: for File entries (parent, filename);
        // for Dir entries (canon_path, "").  `cmode` (slice 26) is
        // "oracular"/"consensus" — routed by Fs.rho into the
        // File/Dir constructor and back into native chown/stat/
        // entries dispatch.
        out.push_str(&format!(
            r#""{name}": ("{root}", "{rel}", "{mode}", "{kind}", "{cmode}")"#
        ));
    }
    out.push('}');
    out
}

/// Escape a string for safe embedding in a Rholang `"..."` literal.
///
/// Rholang string grammar (`rholang_mercury.cf`):
///   StringLiteral ::= '"' ((char - ["\"\\"]) | ('\\' ["\"\\nt"]))* '"'
///
/// Only `\"`, `\\`, `\n`, `\t` are valid escapes; `\r` and other
/// escape sequences would produce a lexer error.  Slice 21's HOCON
/// deserializer + slice 22's CLI parser + slice 23's boot
/// validation all call `reject_forbidden_chars`, which rejects NUL
/// / C0 controls / DEL / C1 controls / BOM / RTL overrides / line
/// separators before this function is ever reached.  Any control
/// char that does reach this function indicates a programmatic-
/// construction bypass — we panic rather than emit a Rholang
/// source that would fail at deploy time (C-25-2 slice-25 review
/// fix: previously we emitted `\r` which the Rholang lexer rejects,
/// causing genesis-time panic on legitimate Windows-CRLF input).
fn rholang_string_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str(r"\\"),
            '"' => out.push_str(r#"\""#),
            '\n' => out.push_str(r"\n"),
            '\t' => out.push_str(r"\t"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7F => {
                panic!(
                    "rholang_string_escape: control char U+{:04X} unexpectedly \
                     reached the composer; upstream validators should have \
                     rejected this input.  If this fires in production, some \
                     caller bypassed reject_forbidden_chars.",
                    c as u32
                );
            }
            _ => out.push(c),
        }
    }
    out
}

/// Nonce used in the FsGenesis signed-registry insertion.  MAX_LONG
/// so nobody can overwrite the entry once published.
///
/// X-1 / CONS-4 (2026-09-12): source-of-truth moved to
/// `rholang::rust::interpreter::io::FS_NONCE` so the fingerprint-
/// fold registration lives alongside the other consensus constants
/// (all fingerprint entries must live in a single crate for the
/// `linkme::distributed_slice` collection to be complete in every
/// linkage context, including rholang-lib-only tests).  Re-exported
/// here for backwards compatibility with existing casper call
/// sites; the value is guaranteed identical by the const-eq
/// assertion below.
pub const FS_NONCE: i64 = rholang::rust::interpreter::io::FS_NONCE;
const _: () = assert!(
    FS_NONCE == i64::MAX,
    "CONS-4: FS_NONCE canonical value is i64::MAX; a drift here is \
     a hard-fork event.  See rholang::rust::interpreter::io::FS_NONCE."
);

/// URN prefix shared between the runtime's `fs_native_def` registrations
/// and this module's composed FsGenesis source.  A future Phase 1 hotfix
/// bumping to `1.0.1` must edit HERE only, and both the runtime
/// (`rho_runtime.rs`) and the composed source rebuild from this constant.
pub const FS_NATIVE_URN_PREFIX: &str = "rho:io:fs:native:1.0.0/";

/// Native URN suffixes that this module binds into the FsGenesis
/// new-scope.  Combined with `FS_NATIVE_URN_PREFIX` to form the full
/// URN.  Kept as a constant so a slice-drift assertion in the test
/// suite can cross-check against the runtime's registered set
/// (`rho::interpreter::rho_runtime::fs_native_def` call sites).
///
/// Order matches the `new`-clause below (documentation aid only).
///
/// # Cross-file drift discipline
///
/// Any new fs-native URN MUST be added in FIVE places (only the first
/// three are drift-checked by existing tests; the last two require
/// manual attention):
///
/// 1. **This constant** (`FS_NATIVE_URN_SUFFIXES`) — checked by
///    `fs_native_urn_suffixes_covers_composed_source` +
///    `composed_source_urns_covered_by_fs_native_urn_suffixes`
///    against the composed source below.
/// 2. **The composed source's top-level `new` clause** below (the
///    `fs<Xyz>(...` bindings) — checked by the same two drift tests.
/// 3. **The arity golden table** in
///    `fs_native_def_arities_match_golden_table` — cross-checks the
///    `fs_native_def(...)` call sites in
///    `rholang::interpreter::rho_runtime::std_system_processes`.
/// 4. **The `all_fs_native_suffixes_are_rejected` iteration list**
///    in `rholang/tests/fs_native_urn_filter_spec.rs` — HARDCODED,
///    not auto-iterated over this constant.  A new suffix added
///    here without adding it there will pass the drift checks but
///    won't be verified for URN-filter rejection under state-execution.
///    (The `filter_catches_unknown_fs_native_urn_prefix` test provides
///    prefix-based defense-in-depth, so the suffix IS rejected in
///    practice — but not directly asserted.)
/// 5. **`rho_runtime::std_system_processes`'s `fs_native_def` call**
///    for that suffix, wiring URN → `BodyRefs::FS_<XYZ>` →
///    handler.  The arity drift-check in (3) catches missing
///    entries; but the handler itself must also be added to
///    `handlers.rs` and the FixedChannel to `system_processes.rs`.
///    Compilation catches missing pieces.
pub const FS_NATIVE_URN_SUFFIXES: &[&str] = &[
    "open",
    "close",
    "read",
    "readAt",
    "write",
    "writeAt",
    "seek",
    "tell",
    "size",
    "flush",
    "stat",
    "exists",
    "truncate",
    "chmod",
    "chown",
    "removeFile",
    "removeDir",
    "rename",
    "copyFile",
    "entries",
    // M-3 fix (2026-08-06): quarantine had `fs_native_def`
    // registration in rho_runtime.rs but was NOT bound in the
    // composed new-clause below.  The bidirectional drift check
    // `fs_native_urn_suffixes_matches_composed_source_bidirectionally`
    // pins the correspondence in both directions.
    //
    // A8-M-1 (2026-09-03): the bulk "entriesStream" URN was
    // retired alongside its handler stub — per-fd variants at
    // `entriesStreamOpen` / `_Next` / `_Close` are the live surface.
    "quarantine",
    // Phase 8 slice 8a — range-lock natives.  File.rho binds these
    // via lexical `new` capture the same way it binds fsRead/fsWrite/etc.
    "lockRange",
    "lockSequential",
    "releaseLock",
    // Phase 8 slice 8a step-4 — File.close sweep native (X-1 §901).
    // Invoked inside File.close before dispatching fs_close so a cap
    // that still holds locks at close time doesn't strand them until
    // deploy-end auto-release fires.  Scoped by HolderId — cross-cap
    // locks on the same (dev, inode) survive.
    "releaseAllForHolder",
    // Streaming-backing slice (2026-08-25) — per-fd directory-entries
    // streaming primitive.  Open allocates a stream fd; Next yields
    // one entry per call; Close releases the fd.  See implementation-
    // plan.md §"Streaming-backing slice" for the full design.  Replaces
    // the retired bulk `entriesStream` URN (A8-M-1, 2026-09-03).
    "entriesStreamOpen",
    "entriesStreamNext",
    "entriesStreamClose",
];
/// Deterministically derive the secp256k1 signature the composed
/// FsGenesis source needs for the `rs!(...)` call.  Matches
/// RegistrySigGen::derive_from's `to_sign` construction and hashing.
/// Signing is RFC 6979 (deterministic k) via the k256 crate — cross-
/// process consistency required for consensus.
pub fn fs_genesis_signature_hex(sk: &PrivateKey, timestamp: i64) -> String {
    let secp256k1 = Secp256k1;
    let pk: PublicKey = secp256k1.to_public(sk);
    let to_sign: Par = new_etuple_par(vec![
        new_gint_par(timestamp, Vec::new(), false),
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GByteArray(pk.bytes.to_vec())),
        }]),
        new_gint_par(FS_NONCE, Vec::new(), false),
    ]);
    let sign_bytes = Blake2b256::hash(to_sign.encode_to_vec());
    let sig = secp256k1.sign(&sign_bytes, &sk.bytes);
    hex::encode(sig)
}

/// The legacy `rho:id:<hash>` URI at which the FsGenesis deploy
/// publishes the shared Fs cap via `insertSigned`.  Deterministic
/// function of FS_GENERATOR_PK.  Kept alongside `fs_versioned_uri`
/// (below) for callers that either prefer the terse `rho:id:` form
/// or are pinned to it by prior code.  Both URIs resolve to the
/// same `bundle+{*this}` handle produced by the single `Fs!?(0, 1, 2,
/// bundle)` mint inside the composed source.
pub fn fs_genesis_uri(pk: &PublicKey) -> String {
    let key_hash = Blake2b256::hash(pk.bytes.to_vec());
    Registry::build_uri(&key_hash)
}

/// PB-B-3 (2026-08-24): the Versioned Registry URN at which the
/// FsGenesis deploy also publishes the shared Fs cap via
/// `insertVersion` — resolvable through `rho:registry:1.0.0`'s
/// `lookupVersion` machinery with semver + notify support.
///
/// URN shape: `rho:serve:1.0.0:<FS_GENERATOR_PUB_KEY_HEX>:fs:1.0.0`.
///
/// The FIP spec §325 aspirationally uses the shorter
/// `rho:io:fs:1.0.0` form, but the current versioned URN parser
/// (`rholang/src/rust/interpreter/registry/versioned_urn.rs`) only
/// recognizes `rho:lib:*` / `rho:serve:*` / `rho:registry:*` shapes.
/// Extending the parser + registry store schema to add an `io`
/// namespace is a substantial cross-cutting change (URN parser,
/// store key shape, namespace-policy gate for who may register
/// there); deferred as a separate slice.  Callers use the serve
/// URN today.
///
/// The `serve` namespace's authenticated-caller discipline means
/// only holders of `FS_GENERATOR_PK` can register under
/// `<FS_GENERATOR_PUB_KEY_HEX>:fs:*` — genesis is the sole such
/// caller by construction.  Wildcard lookup
/// `rho:serve:1.0.0:<hex>:fs:1.*` works via the resolver's semver
/// matching.
///
/// **E2E resolution + method-dispatch verified** by
/// `fs_cap_is_resolvable_via_versioned_registry_uri` in
/// `casper/tests/genesis/contracts/fileio_fs_spec.rs` — looks up
/// via `lookupVersion`, invokes `stdin()`, asserts `[true, cap]`.
pub fn fs_versioned_uri(pk: &PublicKey) -> String {
    let pk_hex = hex::encode(pk.bytes.clone());
    format!("rho:serve:1.0.0:{pk_hex}:fs:1.0.0")
}

/// PB-B-5 (2026-09-02): the Versioned Registry URN at which the
/// FsGenesis deploy publishes the shared `Allocator` cap via
/// `insertVersion`.  Mirrors `fs_versioned_uri` — same serve
/// namespace, same authenticated-caller discipline (only holders of
/// `FS_GENERATOR_PK` can register under this projection), same
/// wildcard-lookup support (`rho:serve:1.0.0:<hex>:buffer:1.*` via
/// the resolver's semver matching).
///
/// URN shape: `rho:serve:1.0.0:<FS_GENERATOR_PUB_KEY_HEX>:buffer:1.0.0`.
///
/// Callers resolve via `lookupVersion` on `rho:registry:1.0.0` and
/// obtain an Allocator cap, which they invoke with `allocBytes(n)` /
/// `allocRows(m, innerN, unit)` etc. to mint `Buffer` and `Rows`
/// instances.  This unblocks the buffer-taking File methods
/// (`readInto` / `writeFrom` / `readLineInto` / `readLinesInto` and
/// their arity-N+1 variants) for user deploys.
///
/// Unlike `fs_versioned_uri`, there is NO `insertSigned` counterpart
/// for the Allocator — the legacy `rho:id:<hash>` publication was a
/// compatibility affordance for the Fs cap.  Allocator ships fresh
/// under PB-B-5, so callers use the serve-URN form exclusively.
///
/// The FIP spec §Table 4 lists the shorter `rho:lang:buffer:1.0.0`
/// aspirational form.  Same URN-parser-extension constraint as
/// `rho:io:fs:1.0.0`: the current parser recognizes only `rho:lib:*`
/// / `rho:serve:*` / `rho:registry:*`.  Adding a `lang` namespace
/// alias is the same URN-parser extension slice that would add `io`
/// aliasing; both are deferred together.
pub fn buffer_versioned_uri(pk: &PublicKey) -> String {
    let pk_hex = hex::encode(pk.bytes.clone());
    format!("rho:serve:1.0.0:{pk_hex}:buffer:1.0.0")
}

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

    #[test]
    fn format_bundle_for_rholang_empty_bundle_returns_empty_map() {
        assert_eq!(format_bundle_for_rholang(&[]), "{}");
    }

    #[test]
    fn format_bundle_for_rholang_oracular_file_splits_parent_filename() {
        let entry = BundleEntry::try_new(
            "data".into(),
            PathBuf::from("/host/dir/data.bin"),
            BundleEntryKind::File,
            "r".into(),
            BundleConsensusMode::Oracular,
        )
        .expect("try_new");
        let s = format_bundle_for_rholang(&[entry]);
        // Oracular File: (parent, filename, mode, "file", cmode).
        assert_eq!(
            s,
            r#"{"data": ("/host/dir", "data.bin", "r", "file", "oracular")}"#
        );
    }

    #[test]
    fn format_bundle_for_rholang_oracular_dir_leaves_rel_empty() {
        let entry = BundleEntry::try_new(
            "root".into(),
            PathBuf::from("/host/dir"),
            BundleEntryKind::Dir,
            "r".into(),
            BundleConsensusMode::Oracular,
        )
        .expect("try_new");
        let s = format_bundle_for_rholang(&[entry]);
        // Oracular Dir: (canon_path, "", mode, "dir", cmode).
        assert_eq!(s, r#"{"root": ("/host/dir", "", "r", "dir", "oracular")}"#);
    }

    #[test]
    fn format_bundle_for_rholang_consensus_file_bundle_root_split() {
        let entry = BundleEntry::try_new(
            "nested/data.bin".into(),
            PathBuf::from("/host/dir/data.bin"),
            BundleEntryKind::File,
            "rw".into(),
            BundleConsensusMode::Consensus,
        )
        .expect("try_new");
        let s = format_bundle_for_rholang(&[entry]);
        // Consensus File: ("/@bundle/<parent>", filename, mode, "file", "consensus").
        assert_eq!(
            s,
            r#"{"nested/data.bin": ("/@bundle/nested", "data.bin", "rw", "file", "consensus")}"#
        );
    }

    #[test]
    fn format_bundle_for_rholang_consensus_file_bare_logical_name_uses_bare_root() {
        let entry = BundleEntry::try_new(
            "flat.bin".into(),
            PathBuf::from("/host/anywhere/flat.bin"),
            BundleEntryKind::File,
            "r".into(),
            BundleConsensusMode::Consensus,
        )
        .expect("try_new");
        let s = format_bundle_for_rholang(&[entry]);
        // No parent segment → ("/@bundle", "flat.bin", ...).
        assert_eq!(
            s,
            r#"{"flat.bin": ("/@bundle", "flat.bin", "r", "file", "consensus")}"#
        );
    }

    #[test]
    fn format_bundle_for_rholang_consensus_dir_whole_logical_under_bundle() {
        let entry = BundleEntry::try_new(
            "subdir".into(),
            PathBuf::from("/host/path"),
            BundleEntryKind::Dir,
            "r".into(),
            BundleConsensusMode::Consensus,
        )
        .expect("try_new");
        let s = format_bundle_for_rholang(&[entry]);
        // Consensus Dir: ("/@bundle/<logical_name>", "", ...).
        assert_eq!(
            s,
            r#"{"subdir": ("/@bundle/subdir", "", "r", "dir", "consensus")}"#
        );
    }

    #[test]
    fn format_bundle_for_rholang_sorts_entries_by_logical_name() {
        let a = BundleEntry::try_new(
            "zebra".into(),
            PathBuf::from("/host/z/file"),
            BundleEntryKind::File,
            "r".into(),
            BundleConsensusMode::Oracular,
        )
        .expect("try_new");
        let b = BundleEntry::try_new(
            "aardvark".into(),
            PathBuf::from("/host/a/file"),
            BundleEntryKind::File,
            "r".into(),
            BundleConsensusMode::Oracular,
        )
        .expect("try_new");
        let s = format_bundle_for_rholang(&[a, b]);
        // aardvark comes first — determinism for genesis-hash
        // stability.
        let a_pos = s.find("aardvark").expect("has aardvark");
        let z_pos = s.find("zebra").expect("has zebra");
        assert!(a_pos < z_pos, "entries must sort by logical name: {s}");
    }

    #[test]
    #[should_panic(expected = "duplicate logical name")]
    fn format_bundle_for_rholang_panics_on_duplicate_logical_name() {
        let a = BundleEntry::try_new(
            "dup".into(),
            PathBuf::from("/host/a/f"),
            BundleEntryKind::File,
            "r".into(),
            BundleConsensusMode::Oracular,
        )
        .unwrap();
        let b = BundleEntry::try_new(
            "dup".into(),
            PathBuf::from("/host/b/f"),
            BundleEntryKind::File,
            "r".into(),
            BundleConsensusMode::Oracular,
        )
        .unwrap();
        let _ = format_bundle_for_rholang(&[a, b]);
    }

    #[test]
    fn rholang_string_escape_backslash_and_quote() {
        assert_eq!(rholang_string_escape(r#"a\b"c"#), r#"a\\b\"c"#);
    }

    #[test]
    fn rholang_string_escape_newline_and_tab() {
        assert_eq!(
            rholang_string_escape("line1\nline2\ttab"),
            r"line1\nline2\ttab"
        );
    }

    #[test]
    #[should_panic(expected = "control char U+0000")]
    fn rholang_string_escape_panics_on_control_char() {
        // Upstream validators should reject before reaching this fn;
        // panic confirms the defense-in-depth guard.
        let _ = rholang_string_escape("\0");
    }

    /// Single-source pin: FS_NONCE re-exports from
    /// `rholang::rust::interpreter::io::FS_NONCE`.  The const-eq
    /// `const _: () = assert!(FS_NONCE == i64::MAX)` catches a
    /// rename or value drift at compile time; this runtime
    /// assertion is defense-in-depth for the pin.
    #[test]
    fn fs_nonce_matches_rholang_source_of_truth() {
        assert_eq!(FS_NONCE, i64::MAX);
        assert_eq!(FS_NONCE, rholang::rust::interpreter::io::FS_NONCE);
    }

    #[test]
    fn fs_native_urn_prefix_pinned() {
        assert_eq!(FS_NATIVE_URN_PREFIX, "rho:io:fs:native:1.0.0/");
    }

    /// LOAD-BEARING: FS_NATIVE_URN_SUFFIXES is the single source of
    /// truth for which native URNs the composed FsGenesis source
    /// binds into the top-level `new` scope.  A new native URN
    /// requires updating this list AND the composed source's `new`
    /// clause (yet to land, slice 5.14+); the drift tests will
    /// enforce correspondence once compose_fs_genesis_source
    /// lands.  This pin asserts the current content.
    #[test]
    fn fs_native_urn_suffixes_pinned() {
        // Current migration-complete set: 27 suffixes (fs_remove_dir
        // is trait-exempt but IS registered as a native URN).
        let expected: &[&str] = &[
            "open",
            "close",
            "read",
            "readAt",
            "write",
            "writeAt",
            "seek",
            "tell",
            "size",
            "flush",
            "stat",
            "exists",
            "truncate",
            "chmod",
            "chown",
            "removeFile",
            "removeDir",
            "rename",
            "copyFile",
            "entries",
            "quarantine",
            "lockRange",
            "lockSequential",
            "releaseLock",
            "releaseAllForHolder",
            "entriesStreamOpen",
            "entriesStreamNext",
            "entriesStreamClose",
        ];
        assert_eq!(
            FS_NATIVE_URN_SUFFIXES, expected,
            "FS_NATIVE_URN_SUFFIXES drifted — a native URN was added \
             or removed.  Update this pin AND the composed FsGenesis \
             `new` clause (yet to land) + the arity golden table + \
             the all_fs_native_suffixes_are_rejected list in \
             rholang/tests/fs_native_urn_filter_spec.rs."
        );
    }

    /// Suffixes must be unique.  A dup would silently double-bind in
    /// the composed source's `new` clause and either shadow or raise
    /// a lexical-redecl error at parse time.
    #[test]
    fn fs_native_urn_suffixes_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for s in FS_NATIVE_URN_SUFFIXES {
            assert!(
                seen.insert(*s),
                "FS_NATIVE_URN_SUFFIXES contains duplicate: `{s}`"
            );
        }
    }

    /// Signature derivation is deterministic (RFC 6979 k).  Same
    /// (sk, timestamp) must produce the same hex every call —
    /// otherwise consensus-time replay would see different genesis
    /// deploy signatures.
    #[test]
    fn fs_genesis_signature_hex_is_deterministic() {
        use crate::rust::genesis::contracts::standard_deploys::FS_GENERATOR_PK;
        let sk = PrivateKey::from_bytes(&hex::decode(FS_GENERATOR_PK).expect("hex decode"));
        let a = fs_genesis_signature_hex(&sk, 1785600000000);
        let b = fs_genesis_signature_hex(&sk, 1785600000000);
        assert_eq!(a, b);
        // DER-encoded secp256k1 signature is 70-72 bytes depending on
        // R/S byte lengths after leading-zero stripping; hex double
        // the byte count.  Pin the shape (even length, all hex) rather
        // than an exact length to tolerate benign DER-length drift.
        assert!(
            a.len() >= 140 && a.len() <= 144,
            "unexpected sig len: {}",
            a.len()
        );
        assert!(a.len().is_multiple_of(2));
        assert!(a
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    /// Different timestamps produce different signatures (the
    /// `to_sign` payload includes the timestamp).
    #[test]
    fn fs_genesis_signature_hex_timestamp_sensitive() {
        use crate::rust::genesis::contracts::standard_deploys::FS_GENERATOR_PK;
        let sk = PrivateKey::from_bytes(&hex::decode(FS_GENERATOR_PK).expect("hex decode"));
        let a = fs_genesis_signature_hex(&sk, 1785600000000);
        let b = fs_genesis_signature_hex(&sk, 1785600000001);
        assert_ne!(a, b);
    }

    #[test]
    fn fs_genesis_uri_is_rho_id_shape() {
        use crate::rust::genesis::contracts::standard_deploys::FS_GENERATOR_PUB_KEY;
        let uri = fs_genesis_uri(&FS_GENERATOR_PUB_KEY);
        assert!(
            uri.starts_with("rho:id:"),
            "fs_genesis_uri must be rho:id:-shaped; got {uri}"
        );
    }

    #[test]
    fn fs_versioned_uri_shape_pinned() {
        use crate::rust::genesis::contracts::standard_deploys::{
            FS_GENERATOR_PK, FS_GENERATOR_PUB_KEY,
        };
        let uri = fs_versioned_uri(&FS_GENERATOR_PUB_KEY);
        // Lower-case hex-encoded pub key between `rho:serve:1.0.0:`
        // and `:fs:1.0.0`.
        let sk = PrivateKey::from_bytes(&hex::decode(FS_GENERATOR_PK).expect("hex decode"));
        let pk_hex = hex::encode(Secp256k1.to_public(&sk).bytes);
        assert_eq!(uri, format!("rho:serve:1.0.0:{pk_hex}:fs:1.0.0"));
    }

    #[test]
    fn buffer_versioned_uri_shape_pinned() {
        use crate::rust::genesis::contracts::standard_deploys::{
            FS_GENERATOR_PK, FS_GENERATOR_PUB_KEY,
        };
        let uri = buffer_versioned_uri(&FS_GENERATOR_PUB_KEY);
        let sk = PrivateKey::from_bytes(&hex::decode(FS_GENERATOR_PK).expect("hex decode"));
        let pk_hex = hex::encode(Secp256k1.to_public(&sk).bytes);
        assert_eq!(uri, format!("rho:serve:1.0.0:{pk_hex}:buffer:1.0.0"));
    }
}
