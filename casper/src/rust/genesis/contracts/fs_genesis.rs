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
//!    the same handle and thus share the cache and stdio caps.  Spec
//!    §867 wants per-principal Fs instances from the powerbox — that
//!    requires runtime changes to the URN resolver (each grantee sees
//!    a distinct cap) and is deferred to a future powerbox slice.
//!
//! 2. **Empty static bundle.**  The published Fs has `bMap = {}`, so
//!    `openFile` / `openDir` return `FSERR_UNSUPPORTED` for every
//!    logical name.  Static provisioning (spec §846, config-driven
//!    bundle) is Phase 7.  Stdio methods (`stdin` / `stdout` /
//!    `stderr`) work.
//!
//! 3. **Hardwired stdio fds.**  The instance is minted with fds
//!    (0, 1, 2).  A future powerbox slice will let each principal
//!    override these (e.g. `/dev/null`-equivalent for cases 4-6 per
//!    spec §Storage).
//!
//! 4. **Versioned Registry URI not published.**  The Fs cap lives at
//!    `rho:id:<hash>` (see `fs_genesis_uri()`), not the spec's
//!    `rho:io:fs:1.0.0`.  A future slice must add an `insertVersion`
//!    call (Versioned Registry) mapping `rho:io:fs:1.0.0` to the same
//!    handle so spec-canonical `getFS(`rho:io:fs:1.*`)` lookups
//!    resolve.  Interim: deploys should substitute the `rho:id:...`
//!    form returned by `fs_genesis_uri(&FS_GENERATOR_PUB_KEY)`.
//!
//! 5. **`rho:io:fs:native:*` URN filter (Phase 7 slice 31 — RESOLVED
//!    2026-08-04).**  Every `rho:io:fs:native:*` URN is registered in
//!    the runtime's `urn_map` for fixed-channel dispatch, but the
//!    reducer's `filter_fs_native_urns` flag (set to `true` at
//!    runtime construction) causes `eval_new` to reject any user
//!    deploy that binds one — the caller sees a `ReduceError`
//!    referencing the rejected URN, not a raw fd.  The flag is
//!    toggled off around `play_deploys_for_genesis` (casper's
//!    `runtime.rs::play_deploys_for_genesis`) so the composed
//!    FsGenesis deploy can bind `fsRead` / `fsWrite` / etc. via
//!    `new fsRead(`rho:io:fs:native:1.0.0/read`) in { ... }`.  The
//!    prefix-based check (`FS_NATIVE_URN_PREFIX` in
//!    `rholang/interpreter/io/mod.rs`) catches every current and
//!    future suffix without per-URN maintenance.  User deploys can
//!    only reach the filesystem through the Fs cap published at
//!    genesis via `insertSigned`, which enforces sandbox / mode-cap /
//!    bundle checks.
//!
//! 6. **`Buffer` / `Allocator` compiled but unreachable.**  The composed
//!    source splices in Buffer.rho's body (agent definitions live at
//!    module scope alongside Fs, File, Dir, etc.) but does NOT publish
//!    the `Allocator` cap — only Fs is exported via `rs!(...)`.  Storage
//!    cost is incurred (contracts persist in the tuplespace) with zero
//!    user reach.  Consequence: File.rho's Buffer-taking methods
//!    (`readInto` / `writeFrom` / `readLineInto` / `readLinesInto`) are
//!    functionally dead until PB-B-5 lands the per-principal Allocator
//!    delegation at `rho:lang:buffer:1.0.0`.  Deploys cannot obtain a
//!    Buffer without that publication.
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
use rholang::rust::interpreter::rho_source::lib_body;

use super::embedded_rho;

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

/// Format a bundle as a Rholang map literal suitable for splicing
/// into the `Fs!?(0, 1, 2, <bundle>)` position of the composed
/// source.  Produces:
///
/// ```text
/// {"logical/name": ("/canon/path", "", "r", "file", "oracular"), ...}
/// ```
///
/// The tuple shape matches Fs.rho's `bMap.get(n)` match pattern
/// `(canonRoot, rel, provisioned, kind, consensusMode)` (slice 26).
/// For projection we use `rel = ""` (the whole provisioned path IS
/// the canonical root of that entry's cap).  `consensusMode` is
/// `"oracular"` or `"consensus"` per `BundleConsensusMode::as_str`.
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
        let (root_str, rel_str) = match entry.kind {
            BundleEntryKind::File => {
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
            BundleEntryKind::Dir => (path_str.to_string(), String::new()),
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
pub const FS_NONCE: i64 = i64::MAX;

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
    // M-3 fix (2026-08-06): entriesStream + quarantine had
    // `fs_native_def` registrations in rho_runtime.rs but were
    // NOT bound in the composed new-clause below.  A future
    // slice referencing `fsEntriesStream!(...)` would silently
    // bind to a fresh unforgeable and never fire.  Both are now
    // in the suffix list; the bidirectional drift check
    // `fs_native_urn_suffixes_matches_composed_source_bidirectionally`
    // pins the correspondence in both directions.
    "entriesStream",
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
];

/// Compose the full FsGenesis Rholang source.
///
/// Wraps every library body inside a shared `new` scope that also
/// binds the native URNs to the names the library bodies capture
/// lexically (`fsRead`, `fsWrite`, etc.) and the registry-insertion
/// URN needed for publication.
///
/// `pk_hex` and `sig_hex` MUST be lowercase ASCII hex strings; the
/// debug-asserts below prevent a future refactor from passing
/// untrusted bytes through the `format!` boundary.
pub fn compose_fs_genesis_source(
    pk_hex: &str,
    sig_hex: &str,
    bundle: &[BundleEntry],
    consensus_fs_snapshot_cadence: Option<u64>,
) -> String {
    // H-25-2 slice-25 review fix: promoted from debug_assert! to
    // assert! so release builds also reject non-hex input.  Genesis
    // runs once at boot; the constant-time hex check is negligible.
    assert!(
        pk_hex.chars().all(|c| c.is_ascii_hexdigit()),
        "pk_hex must be ASCII hex"
    );
    assert!(
        sig_hex.chars().all(|c| c.is_ascii_hexdigit()),
        "sig_hex must be ASCII hex"
    );
    let bundle_rho = format_bundle_for_rholang(bundle);
    // CRIT-2 fix (2026-08-06): embed cadence as a Rholang literal
    // in the FsGenesis deploy term so it is consensus-observable via
    // the deploy hash.  Pre-fix, `Genesis.consensus_fs_snapshot_cadence`
    // was hashed into the Genesis struct but the value never flowed
    // into an on-wire artifact — `BlockApproverProtocol::validate_
    // candidate` does a byte-for-byte deploy-term comparison, and since
    // cadence didn't affect any deploy term, a leader with cadence=100
    // and a validator with cadence=50 both passed validation while
    // silently writing snapshots at different block heights (the
    // FIPS review CRIT-2 finding).  Post-fix, cadence is a literal
    // in the composed source; deploy-term diff fires on mismatch.
    //
    // `None` → literal `Nil` (no cadence).  `Some(n)` → literal
    // integer.  Bound to a private name and immediately consumed
    // so the commitment leaves no live message on any user-reachable
    // channel — the sole purpose is to make cadence appear in the
    // deploy term's serialized bytes.
    let cadence_literal = match consensus_fs_snapshot_cadence {
        None => "Nil".to_string(),
        Some(n) => n.to_string(),
    };

    let file_body = lib_body(embedded_rho::FILE);
    let dir_body = lib_body(embedded_rho::DIR);
    let stream_body = lib_body(embedded_rho::STREAM);
    let buffer_body = lib_body(embedded_rho::BUFFER);
    let stdin_body = lib_body(embedded_rho::STDIN);
    let stdout_body = lib_body(embedded_rho::STDOUT);
    let fs_body = lib_body(embedded_rho::FS);

    let nonce = FS_NONCE;

    format!(
        r#"
new
  File, fdP, stateP, cmodeP, Dir, rootP,
  Stream, paramsP, gatherN, foldLoop, forEachLoop, foldChunksLoop,
  Buffer, Allocator, Rows, metaP, chunkP, innerP, rowsMetaP,
  gatherChunks, drainChunks, allocInnersLoop, parkInnersLoop,
  clearInnersLoop, closeInnersLoop,
  Stdin, stdinFdP, stdinStateP,
  Stdout, stdoutFdP, stdoutStateP,
  Fs, fsBundleP,
  fsStdinFdP, fsStdoutFdP, fsStderrFdP,
  openFileImpl, openFileImplInner, openDirImpl, openDirImplInner, joinRel,
  parseRwxToBits, parseRwxLoop,
  writeBytesLoop, writeBytesAtLoop, writeCharsLoop, writeLinesLoop,
  readLinesIntoLoop, drainToNextLF,
  codepointLen, concatStringsLoop, scanLineForLF,
  // Phase 8 slice 8a — LockToken agent + per-instance state key.
  // See File.rho's module-level `new` docstring for the design
  // rationale; must be bound at THIS outer scope because File.rho
  // gets its own top-level `new` stripped by `lib_body` at
  // composition time.
  LockToken, lockStateP,
  fsOpen(`rho:io:fs:native:1.0.0/open`),
  fsClose(`rho:io:fs:native:1.0.0/close`),
  fsRead(`rho:io:fs:native:1.0.0/read`),
  fsReadAt(`rho:io:fs:native:1.0.0/readAt`),
  fsWrite(`rho:io:fs:native:1.0.0/write`),
  fsWriteAt(`rho:io:fs:native:1.0.0/writeAt`),
  fsSeek(`rho:io:fs:native:1.0.0/seek`),
  fsTell(`rho:io:fs:native:1.0.0/tell`),
  fsSize(`rho:io:fs:native:1.0.0/size`),
  fsFlush(`rho:io:fs:native:1.0.0/flush`),
  fsStat(`rho:io:fs:native:1.0.0/stat`),
  fsExists(`rho:io:fs:native:1.0.0/exists`),
  fsTruncate(`rho:io:fs:native:1.0.0/truncate`),
  fsChmod(`rho:io:fs:native:1.0.0/chmod`),
  fsChown(`rho:io:fs:native:1.0.0/chown`),
  fsRemoveFile(`rho:io:fs:native:1.0.0/removeFile`),
  fsRemoveDir(`rho:io:fs:native:1.0.0/removeDir`),
  fsRename(`rho:io:fs:native:1.0.0/rename`),
  fsCopyFile(`rho:io:fs:native:1.0.0/copyFile`),
  fsEntries(`rho:io:fs:native:1.0.0/entries`),
  fsEntriesStream(`rho:io:fs:native:1.0.0/entriesStream`),
  fsQuarantine(`rho:io:fs:native:1.0.0/quarantine`),
  fsLockRange(`rho:io:fs:native:1.0.0/lockRange`),
  fsLockSequential(`rho:io:fs:native:1.0.0/lockSequential`),
  fsReleaseLock(`rho:io:fs:native:1.0.0/releaseLock`),
  fsReleaseAllForHolder(`rho:io:fs:native:1.0.0/releaseAllForHolder`),
  rs(`rho:registry:insertSigned:secp256k1`),
  uriOut
in {{
  {file_body}
  |
  {dir_body}
  |
  {stream_body}
  |
  {buffer_body}
  |
  {stdin_body}
  |
  {stdout_body}
  |
  {fs_body}
  |
  // CRIT-2 fix (2026-08-06): snapshot-cadence commitment.  Binds
  // cadence to a fresh unforgeable name and immediately consumes
  // it (peek + drop) so the term serializes deterministically as
  // a function of cadence but leaves no user-reachable state.  The
  // sole purpose is byte-diff detection at
  // `BlockApproverProtocol::validate_candidate` — a leader with
  // cadence=100 and a validator with cadence=50 now produce
  // different fs_generator deploy terms, so validation fails loudly
  // instead of silently proceeding with divergent snapshot behavior.
  new snapshotCadenceCommitmentP in {{
    snapshotCadenceCommitmentP!({cadence_literal}) |
    for (_ <- snapshotCadenceCommitmentP) {{ Nil }}
  }} |
  // Slice 25: mint one shared Fs instance (stdio fds 0/1/2, static
  // bundle populated from operator config+CLI merge) and publish
  // it at the registry URI derived from FS_GENERATOR_PK.  Per-
  // principal delegation via powerbox is a future slice (see
  // fs_genesis.rs docstring).
  for (@fs <- Fs!?(0, 1, 2, {bundle_rho})) {{
    rs!(
      "{pk_hex}".hexToBytes(),
      ({nonce}, fs),
      "{sig_hex}".hexToBytes(),
      *uriOut
    )
  }}
}}
"#
    )
}

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

/// The registry URI at which the FsGenesis deploy publishes the
/// shared Fs cap.  Deterministic function of FS_GENERATOR_PK.
pub fn fs_genesis_uri(pk: &PublicKey) -> String {
    let key_hash = Blake2b256::hash(pk.bytes.to_vec());
    Registry::build_uri(&key_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    // `lib_body_*` unit tests moved to
    // `rholang::interpreter::rho_source::tests` (M-14 resolution, 2026-08-11).

    #[test]
    fn fs_native_urn_suffixes_covers_composed_source() {
        // Every URN listed in FS_NATIVE_URN_SUFFIXES must be bound in
        // the composed source.  This is a drift assertion: if someone
        // adds a suffix to the constant without updating the format!
        // template, this test fails.
        let src = compose_fs_genesis_source("00", "00", &[], None);
        for suffix in FS_NATIVE_URN_SUFFIXES {
            let expected = format!("`{FS_NATIVE_URN_PREFIX}{suffix}`");
            assert!(
                src.contains(&expected),
                "composed source missing URN binding: {expected}"
            );
        }
    }

    /// M-2 fix (2026-08-06): pin fs_native_def arities against
    /// a golden URN → arity map.  A slice that bumps a handler's
    /// arity but forgets to update the corresponding `fs_native_def`
    /// call (or vice versa) creates a silent-hang class where the
    /// dispatch pattern doesn't match and the reply channel is
    /// never fired — H-P7-8-E2E was exactly this class of bug.
    /// This test locks the 22 arities against a hardcoded table
    /// derived from Slice 26's arity documentation in handlers.rs.
    ///
    /// A regression bumping (say) `fs_stat` from arity 4 to 5
    /// without updating this table fails HERE — surfacing at
    /// build time rather than as a mysterious deploy hang.
    #[test]
    fn fs_native_def_arities_match_golden_table() {
        // Golden table: URN suffix → (arity, rationale).
        // Rationale is documented in rho_runtime.rs at the
        // fs_native_def call sites; keep this table in sync.
        // Every arity change here IS a cross-source change and
        // usually a hard fork of caller code.
        let golden: &[(&str, usize)] = &[
            ("open", 5),          // (root, rel, mode, cmode, ack)
            ("close", 2),         // (fd, ack)
            ("read", 3),          // (fd, n, ack)
            ("readAt", 4),        // (fd, off, n, ack)
            ("write", 3),         // (fd, bytes, ack)
            ("writeAt", 4),       // (fd, off, bytes, ack)
            ("seek", 4),          // (fd, whence, off, ack)
            ("tell", 2),          // (fd, ack)
            ("size", 2),          // (fd, ack)
            ("flush", 2),         // (fd, ack)
            ("stat", 4),          // (root, rel, cmode, ack) — Slice 26
            ("exists", 3),        // (root, rel, ack)
            ("truncate", 3),      // (fd, n, ack)
            ("chmod", 5),         // (root, rel, mode, cmode, ack) — Slice 26
            ("chown", 6),         // (root, rel, owner, group, cmode, ack) — Slice 26
            ("removeFile", 4),    // (root, rel, cmode, ack) — Slice 26
            ("removeDir", 5),     // (root, rel, recursive, cmode, ack) — Slice 26
            ("rename", 6),        // (from_root, from_rel, to_root, to_rel, cmode, ack) — Slice 26
            ("copyFile", 6),      // (from_root, from_rel, to_root, to_rel, cmode, ack) — Slice 26
            ("entries", 4),       // (root, rel, cmode, ack) — Slice 26
            ("entriesStream", 3), // (root, rel, ack)
            ("quarantine", 3),    // (root, rel, ack)
            // Phase 8 slice 8a — range-lock natives (fd-based after
            // review-2 fix, 2026-08-12: keys correctly under oracular
            // file swap; path-based keying was semantically wrong).
            ("lockRange", 7),      // (fd, offset, length, mode, holder, cmode, ack)
            ("lockSequential", 4), // (fd, holder, cmode, ack)
            ("releaseLock", 2),    // (lockId, ack)
            // Phase 8 slice 8a step-4 — File.close sweep native (X-1 §901).
            ("releaseAllForHolder", 2), // (holder, ack)
        ];

        // Read rho_runtime.rs from the sibling rholang crate.
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../rholang/src/rust/interpreter/rho_runtime.rs"
        ))
        .expect("read rho_runtime.rs to extract fs_native_def arities");

        // Simple scanner: find each `fs_native_def(` opening,
        // then within the parentheses find:
        //   - the URN string literal after the prefix
        //   - the first numeric literal following the URN — that's the arity
        for (suffix, expected_arity) in golden {
            let urn = format!("\"{FS_NATIVE_URN_PREFIX}{suffix}\"");
            let anchor = src.find(&urn).unwrap_or_else(|| {
                panic!(
                    "M-2: rho_runtime.rs is missing fs_native_def registration \
                     for `{FS_NATIVE_URN_PREFIX}{suffix}`; the M-3 bidirectional \
                     drift check should have flagged this too"
                )
            });
            // Scan forward line-by-line past the URN, skipping
            // comments and the channel line, until a line whose
            // trimmed body starts with an integer literal — that
            // is the arity argument.
            let window = &src[anchor..anchor + 800];
            let mut actual: Option<usize> = None;
            for line in window.lines() {
                let trimmed = line.trim();
                // Skip blanks, //-comments, block-comment lines,
                // and the URN string itself.
                if trimmed.is_empty()
                    || trimmed.starts_with("//")
                    || trimmed.starts_with("/*")
                    || trimmed.starts_with('*')
                    || trimmed.contains(&urn)
                    || trimmed.starts_with("FixedChannels::")
                {
                    continue;
                }
                // First numeric-leading line after we've passed
                // the channel is the arity.  Strip trailing `,`
                // + comments.
                let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
                if !digits.is_empty() {
                    actual =
                        Some(digits.parse().unwrap_or_else(|e| {
                            panic!("M-2: arity for {suffix} did not parse: {e}")
                        }));
                    break;
                }
            }
            let actual = actual.unwrap_or_else(|| {
                panic!(
                    "M-2: could not find an integer-literal arity line \
                     within 800 bytes after the URN for {suffix}; the \
                     `fs_native_def(URN, channel, arity, body_ref, closure)` \
                     layout may have changed"
                )
            });
            assert_eq!(
                actual, *expected_arity,
                "M-2: arity drift for `{FS_NATIVE_URN_PREFIX}{suffix}` — \
                 handler destructures {expected_arity} args (per handlers.rs) \
                 but fs_native_def registers arity {actual}.  If intentional, \
                 update this golden table AND the destructure in handlers.rs \
                 AND every caller.  A silent mismatch produces the H-P7-8-E2E \
                 hang class."
            );
        }
    }

    /// M-12 fix (2026-08-06): golden-hex byte-anchor for
    /// `compose_fs_genesis_source`.  The determinism tests
    /// elsewhere in this module confirm the same-input →
    /// same-output invariant within a single process; this
    /// pin adds a cross-build anchor.
    ///
    /// A subtle lib_body edit that changes the composed source
    /// (say, reorders `new` bindings or drops a fs-native
    /// registration) would fork validators at block 0 because
    /// the genesis deploy source hash would differ.  Pre-M-12,
    /// no test caught this at CI time — only a divergent
    /// genesis on live nodes would surface it.
    ///
    /// If this hash changes, either:
    ///  (a) You deliberately hard-forked the fs_generator source
    ///      — regenerate via
    ///      `cargo test --package casper --test mod
    ///      compose_fs_genesis_source_golden_hex -- --nocapture`
    ///      and update the constant, treating the change as a
    ///      Genesis hard fork; or
    ///  (b) An unintended edit slipped through — fix it.
    #[test]
    fn compose_fs_genesis_source_golden_hex() {
        use crypto::rust::hash::blake2b256::Blake2b256;
        // Pin against a specific input: empty bundle, empty pk/sig,
        // no cadence override.  These inputs don't depend on
        // operator config or bundle contents, so the resulting
        // hash reflects the composed source structure alone.
        let src = compose_fs_genesis_source("00", "00", &[], None);
        let h = Blake2b256::hash(src.into_bytes());
        let hex: String = h.iter().fold(String::with_capacity(64), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{b:02x}");
            acc
        });
        // Recompute with `--nocapture` after any intentional edit.
        // Pre-M-3 baseline (before entriesStream+quarantine bindings)
        // is intentionally NOT preserved — the fix landed with the
        // test; this hash is the post-fix anchor.
        //
        // If this test fails with a diff you didn't intend, `git
        // diff casper/src/rust/genesis/contracts/fs_genesis.rs`
        // should surface the offending edit.
        // Golden value pinned 2026-08-06.  Prior anchor line-up:
        //   - Post-M-3 (entriesStream + quarantine bindings):
        //     742b4ef484620d08fe02e601d13a807a9ae3b02eea26ab442ceabd884adb3000
        //   - Post-M-6 (Fs.rho File-cap logic + docstring update
        //     — Fs.rho is embedded into the composed source, so
        //     any Fs.rho edit that isn't a comment-only tweak
        //     flips this hash).  Current post-M-6 anchor below.
        // Regenerate deliberately via
        // `cargo test -p casper --lib -- --nocapture \
        //   compose_fs_genesis_source_golden_hex`.
        // Anchor rolled forward for Phase 8 slice 8a: added three
        // range-lock URN bindings (`fsLockRange`, `fsLockSequential`,
        // `fsReleaseLock`) to the composed source's top-level `new`
        // clause.  IS a Genesis-composed-source hard fork: any
        // validator running pre-slice-8a code composes without the
        // new URNs, hashes to the previous anchor, and diverges on
        // the genesis block.  Not a live-network concern for this
        // branch (unreleased), but any future backport must land
        // atomically across all validators at a coordinated block
        // height.  The bindings themselves are unused until File.rho
        // invokes them (slice 8a step 5), but their PRESENCE in the
        // composed source is enough to shift the block hash.
        //
        // Anchor rolled forward again for Phase 8 slice 8a step-4
        // prep (2026-08-12): added `fsReleaseAllForHolder` URN
        // binding for the File.close sweep native (X-1 §901).  Same
        // hard-fork discipline as the initial slice-8a roll.
        //
        // Anchor rolled forward again for Phase 8 slice 8a step 4c-2
        // (2026-08-12): File.rho now contains the `agent LockToken`
        // block and the `method lockRange` implementation, and the
        // outer new-clause binds `LockToken, lockStateP`.  File.rho is
        // embedded in the composed source, so any File.rho edit that
        // isn't a comment-only tweak flips this hash.  Same hard-fork
        // discipline as previous rolls.
        //
        // Anchor rolled forward again for Phase 8 slice 8a step 4d-1
        // (2026-08-12): four positional methods (readInto, readAtInto,
        // writeFrom, writeFromAt) now wrap their native call with
        // fsLockRange acquire + fsReleaseLock release.  Cursor-based
        // variants (readInto, writeFrom) additionally do an fsTell
        // before the lock acquire to know the range extent.  Same
        // hard-fork discipline.
        //
        // Anchor rolled forward again for Phase 8 slice 8a step 4d-2a
        // (2026-08-12): writeBytesAt now wraps its writeBytesAtLoop
        // dispatch with a range lock over the DECLARED maxLength
        // (spec §959: caller commits to the max write extent so the
        // lock is known at construction).  Same hard-fork discipline.
        //
        // Anchor rolled forward again for Phase 8 slice 8a step 4d-2b
        // (2026-08-12): bytesAt now acquires a stream-lifetime range
        // lock — extent = length (Int) OR 2^62 sentinel (Nil, to-EOF).
        // Release via a separate lockCell that guards against amplifying
        // release syscalls on repeated post-EOS polls.  Caller-driven
        // early abandonment relies on File.close sweep (step 4f) /
        // deploy-end auto-release (step 5) per spec §Explicit locks
        // MUST safety net.  Same hard-fork discipline.
        //
        // Anchor rolled forward again for slice-8a step-4 review cleanup
        // (2026-08-12): added §Release-reply discards anchor docstring
        // to LockToken block; added trailing "// reply discarded — see
        // ..." comments at the 8 auto-acquire wrap sites; added a
        // clarifying comment at LockToken.release noting its reply is
        // FORWARDED (not discarded, unlike the wraps).  IMPORTANT:
        // comments inside File.rho DO flip this hash — the composed
        // source is hashed as a raw string.  Earlier docstring
        // language claiming "any edit that isn't a comment-only tweak
        // flips this hash" was inaccurate; comment-only edits also
        // roll the anchor, and any future comment cleanup must be
        // coordinated the same way as functional changes.
        //
        // Anchor rolled forward again for Phase 8 slice 8a step 4e-1
        // (2026-08-12): four sequential write methods (writeByteArray,
        // writeBytes, writeChars, writeLine) now acquire a whole-file
        // sequential lock via fsLockSequential around their native/loop
        // dispatch.  Sequential (not range) matches spec §1143 "one
        // active sequential stream per File" — same-holder sequential
        // stays STRICT even under Prep A's rule (sequential-vs-anything
        // exclusion applies to the same cap).  writeString and writeLines
        // deliberately DO NOT acquire their own locks; they delegate to
        // writeByteArray / writeLine respectively and inherit per-inner-
        // call atomicity from those wraps.  writeLines' atomicity is
        // therefore per-line, not per-writeLines — documented in the
        // method's block comment for callers who need cross-cap atomic
        // multi-line writes (they must hold an outer lockRange).
        //
        // Anchor rolled forward again for Phase 8 slice 8a step 4e-2
        // (2026-08-12): bytes() and chars() read-stream constructors now
        // acquire a stream-lifetime sequential lock, released via the
        // separate lockCell guard pattern (same as bytesAt step 4d-2b)
        // at every termination path.  bytes() has 2 termination paths
        // (EOF, fsRead error); chars() has 5 (2 EOS variants + 3 error
        // variants for UTF-8 boundary handling).
        //
        // Anchor rolled forward again for Phase 8 slice 8a step 4e-3
        // (2026-08-12): readLine() acquires a stream-lifetime sequential
        // lock with 9 termination-path guards (2 repeat-EOS variants +
        // 1 LF-consumption EOS with 2 seek-back sub-cases + 5 error
        // variants).  LF-consumption is the primary termination for
        // well-behaved consumers; other paths cover EOF-before-LF and
        // UTF-8 boundary failures.
        //
        // Anchor rolled forward again for Phase 8 slice 8a step 4e-4
        // (2026-08-12): lines() (nested LineStream-of-CharStreams)
        // acquires a stream-lifetime sequential lock at outer mint.
        // Inner CharStreams share the OUTER lock — no per-inner acquire.
        // The single-active-inner rule (spec §349) guarantees at most
        // one inner is live at a time, and outer.next() revokes the
        // current inner before minting the next or terminating, so no
        // inner is left doing I/O after outer's lock releases.  Release
        // fires at the 3 outer termination paths (EOF via empty source,
        // EOF via zero-read, fsRead error).  Step 4e is now COMPLETE:
        // all 10 sequential-stream methods (4 read constructors + 4
        // write methods) are auto-locked; writeString and writeLines
        // deliberately delegate to writeByteArray / writeLine.
        //
        // Anchor rolled forward again for Phase 8 slice 8a step 4f
        // (2026-08-12): File.close now invokes fsReleaseAllForHolder!
        // (*stateP, ...) BEFORE fsClose!, sweeping every lock held via
        // this cap.  Spec §File > close: "Implicitly releases every
        // range lock held via this File cap; any subsequent
        // lockToken!release() on those tokens returns [false,
        // FSERR_CLOSED, ...]."  Sweep is scoped by HolderId so cross-
        // cap locks on the same (dev, inode) survive.
        //
        // Anchor rolled forward again for slice-8a step-4e-1 review
        // follow-up (2026-08-12): added §Atomicity scope block to
        // writeLines docstring, spelling out the per-line-not-per-
        // writeLines semantic + cross-cap-lockRange workaround +
        // rejected-alternatives (hoisted lock deferred as YAGNI).
        // Docs-only; no behavior change.  Rolled because File.rho is
        // hashed as raw source including comments.
        //
        // Anchor rolled forward again for slice-8a step-4g holder-
        // identity fix (2026-08-12): File.rho's 16 lock-native call
        // sites now pass `*this` (per-instance dispatch channel,
        // unique per fresh-mint cap) as the `holder` argument,
        // replacing the earlier `*stateP` (module-level, shared
        // across ALL File caps).  Previous holder derivation
        // collapsed every cap to a single HolderId — cross-cap
        // coordination silently degraded to a same-holder no-op,
        // breaking spec §Range locks' different-holder rule.  Bug
        // surfaced via step 4g's two-cap integration test.  All
        // inline "holder MUST be *stateP" invariant comments +
        // Rust-side docstrings in lock.rs (`HolderId`) and
        // handlers.rs (`fs_lock_range` / `fs_release_all_for_holder`
        // / `holder_id_of`) updated with the same *stateP → *this
        // correction + rationale.  Same hard-fork discipline.
        //
        // Anchor rolled forward again for Phase 8 slice 8b sub-4
        // (2026-08-12): added arity-4 `method lockRange(@offset,
        // @length, @mode, @options)` alongside the existing arity-3
        // method.  The new method extracts `wait: Bool` from the
        // options map per spec §1181 and passes it through to
        // fsLockRange (arity-8, wait: Bool at slot 7).  The arity-3
        // method is unchanged and continues to invoke fsLockRange
        // arity-7 (native's legacy shim defaults wait:false); both
        // paths coexist for backward compat.  Same hard-fork
        // discipline as previous rolls.
        //
        // Anchor rolled forward again for Phase 8 slice 8b sub-6
        // review-fix (2026-08-12): the arity-4 lockRange method now
        // releases stateP EARLY (immediately after the state check)
        // rather than late (after the native reply arrives).  Under
        // wait:true, the native's admit await can block indefinitely
        // — holding stateP through that wait would block every other
        // method on the same cap, including close().  Early release
        // is the fix per the review; body-comments in the method
        // document the concurrent-close scenario.  Same hard-fork
        // discipline as previous rolls.
        const EXPECTED: &str = "c5d5be358612c8cd6a8cdd7ac005010e1868fbaebaecc93d874f4698903e00e4";
        assert_eq!(
            hex, EXPECTED,
            "M-12: compose_fs_genesis_source() hash changed.  If intentional \
             (a Genesis hard fork), rerun with --nocapture and update EXPECTED; \
             else find and revert the source edit."
        );
        // Print the hash so `--nocapture` runs surface it for
        // easy regeneration.
        println!("compose_fs_genesis_source hash = {hex}");
    }

    /// M-4 fix (2026-08-06): cross-language drift pin for
    /// consensus-mode string literals.  The Rust constants
    /// `CMODE_ORACULAR_STR` / `CMODE_CONSENSUS_STR` in
    /// `rholang/src/rust/interpreter/io/mod.rs` are pinned
    /// Rust-to-Rust (via `resolve_cmode` unit tests), but the
    /// 16 `.rho` sites in File.rho / Dir.rho / Fs.rho that
    /// embed `"oracular"` / `"consensus"` were never checked
    /// against them.  Renaming the Rust constant (say to
    /// `"oracle"`) leaves the .rho matching the old string, and
    /// every dispatch call silently falls into the REVOKED
    /// (default-arm) branch.
    ///
    /// This test grep-counts the literals in the three shipped
    /// .rho library files and asserts they exist.  A future
    /// slice that renames either constant to something the .rho
    /// files don't reference fails HERE.
    #[test]
    fn cmode_string_literals_pinned_across_rholang_and_rust() {
        // Reach the Rust constants via the rholang crate.  Kept
        // here (in casper) as a cross-crate anchor — a rename in
        // rholang without matching .rho updates flips this.
        let oracular: &str = rholang::rust::interpreter::io::CMODE_ORACULAR_STR;
        let consensus: &str = rholang::rust::interpreter::io::CMODE_CONSENSUS_STR;
        assert_eq!(
            oracular, "oracular",
            "CMODE_ORACULAR_STR is pinned at \"oracular\" — see below"
        );
        assert_eq!(
            consensus, "consensus",
            "CMODE_CONSENSUS_STR is pinned at \"consensus\" — see below"
        );

        // Now cross-check against every shipped .rho library.
        let rho_files: &[&str] = &[
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/main/resources/Fs.rho"),
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/main/resources/File.rho"),
            concat!(env!("CARGO_MANIFEST_DIR"), "/src/main/resources/Dir.rho"),
        ];
        let quoted_oracular = format!("\"{oracular}\"");
        let quoted_consensus = format!("\"{consensus}\"");
        let mut total_oracular = 0usize;
        let mut total_consensus = 0usize;
        for path in rho_files {
            let src =
                std::fs::read_to_string(path).unwrap_or_else(|e| panic!("M-4: read {path}: {e}"));
            total_oracular += src.matches(&quoted_oracular).count();
            total_consensus += src.matches(&quoted_consensus).count();
        }
        // Both literals appear somewhere in the shipped .rho
        // libraries — a rename of either Rust constant leaves
        // the .rho matching the OLD string and this fires.
        assert!(
            total_oracular > 0,
            "M-4: `{quoted_oracular}` not found in shipped .rho library files.  \
             Either the Rust CMODE_ORACULAR_STR was renamed without updating \
             File.rho / Dir.rho / Fs.rho, or the .rho files removed all cmode \
             match arms (unlikely).  Every affected dispatch silently falls \
             into the REVOKED default arm."
        );
        assert!(
            total_consensus > 0,
            "M-4: `{quoted_consensus}` not found in shipped .rho library files.  \
             Same drift class as the oracular pin above."
        );
    }

    /// M-3 fix (2026-08-06): bidirectional drift check.  The
    /// existing `..._covers_composed_source` test caught missing
    /// bindings in one direction (constant → source).  The
    /// opposite direction was uncovered: a `rho:io:fs:native:1.0.0/*`
    /// URN bound in the composed source but NOT listed in
    /// `FS_NATIVE_URN_SUFFIXES` would go undetected.  Pre-M-3,
    /// `entriesStream` + `quarantine` had `fs_native_def`
    /// registrations in rho_runtime.rs but no composed-source
    /// binding — a `fsEntriesStream!(...)` reference from
    /// (e.g.) `Dir.entries` would silently bind to a fresh
    /// unforgeable and never fire.
    ///
    /// Post-M-3, both directions are pinned.  A future URN
    /// added to either side without the other trips this test.
    #[test]
    fn composed_source_urns_covered_by_fs_native_urn_suffixes() {
        let src = compose_fs_genesis_source("00", "00", &[], None);
        // Find every `rho:io:fs:native:1.0.0/<suffix>` URN in
        // the composed source and confirm the suffix is in the
        // constant.  Regex-lite scan: split on the prefix and
        // pull the suffix up to the next backtick.
        let mut found: Vec<String> = Vec::new();
        for chunk in src.split(FS_NATIVE_URN_PREFIX).skip(1) {
            if let Some(end) = chunk.find('`') {
                found.push(chunk[..end].to_string());
            }
        }
        assert!(
            !found.is_empty(),
            "sanity: composed source should contain at least one fs-native URN"
        );
        for suffix in &found {
            assert!(
                FS_NATIVE_URN_SUFFIXES.contains(&suffix.as_str()),
                "M-3: composed source binds `{FS_NATIVE_URN_PREFIX}{suffix}` but that \
                 suffix is missing from FS_NATIVE_URN_SUFFIXES.  A caller using this \
                 URN would silently bind to a fresh unforgeable and never fire.  Add \
                 it to the constant (see fs_genesis.rs:FS_NATIVE_URN_SUFFIXES)."
            );
        }
    }

    // -------------------- Slice 25: bundle formatting --------------------

    #[test]
    fn format_bundle_empty_yields_empty_map() {
        assert_eq!(format_bundle_for_rholang(&[]), "{}");
    }

    #[test]
    fn format_bundle_single_file_entry() {
        let b = [BundleEntry {
            logical_name: "cfg".into(),
            canon_path: PathBuf::from("/etc/cfg"),
            kind: BundleEntryKind::File,
            mode: "r".into(),
            consensus_mode: BundleConsensusMode::Oracular,
        }];
        // Slice 30c H-P7-8: file entries now split canon_path into
        // (parent_dir, filename).  So `/etc/cfg` becomes
        // `("/etc", "cfg", ...)` instead of `("/etc/cfg", "", ...)`.
        // This gives Fs.openFile's downstream safe_descend a real
        // leaf to walk.
        assert_eq!(
            format_bundle_for_rholang(&b),
            r#"{"cfg": ("/etc", "cfg", "r", "file", "oracular")}"#
        );
    }

    #[test]
    fn format_bundle_single_dir_entry() {
        let b = [BundleEntry {
            logical_name: "logs/".into(),
            canon_path: PathBuf::from("/var/log/rnode"),
            kind: BundleEntryKind::Dir,
            mode: "rw".into(),
            consensus_mode: BundleConsensusMode::Oracular,
        }];
        assert_eq!(
            format_bundle_for_rholang(&b),
            r#"{"logs/": ("/var/log/rnode", "", "rw", "dir", "oracular")}"#
        );
    }

    #[test]
    fn format_bundle_sorts_by_logical_name() {
        // Deterministic composition — required for consensus.
        let b = [
            BundleEntry {
                logical_name: "b".into(),
                canon_path: PathBuf::from("/2"),
                kind: BundleEntryKind::File,
                mode: "r".into(),
                consensus_mode: BundleConsensusMode::Oracular,
            },
            BundleEntry {
                logical_name: "a".into(),
                canon_path: PathBuf::from("/1"),
                kind: BundleEntryKind::File,
                mode: "r".into(),
                consensus_mode: BundleConsensusMode::Oracular,
            },
            BundleEntry {
                logical_name: "c".into(),
                canon_path: PathBuf::from("/3"),
                kind: BundleEntryKind::File,
                mode: "r".into(),
                consensus_mode: BundleConsensusMode::Oracular,
            },
        ];
        let formatted = format_bundle_for_rholang(&b);
        let a_pos = formatted.find("\"a\":").unwrap();
        let b_pos = formatted.find("\"b\":").unwrap();
        let c_pos = formatted.find("\"c\":").unwrap();
        assert!(a_pos < b_pos && b_pos < c_pos, "not sorted: {formatted}");
    }

    #[test]
    fn format_bundle_escapes_quote_and_backslash() {
        // Slice 30c H-P7-8: use a Dir entry here so the canon_path
        // (with its escapable chars) stays intact through the
        // formatter — File entries now split into parent+filename
        // which would put the escapable chars in whichever half
        // contains them (parent or filename), obscuring the escape
        // test's intent.  Dir keeps canon_path in the root slot.
        let b = [BundleEntry {
            logical_name: r#"has"quote"#.into(),
            canon_path: PathBuf::from(r"/back\slash"),
            kind: BundleEntryKind::Dir,
            mode: "r".into(),
            consensus_mode: BundleConsensusMode::Oracular,
        }];
        let out = format_bundle_for_rholang(&b);
        // Escaped `"` becomes `\"`; escaped `\` becomes `\\`.
        assert!(out.contains(r#""has\"quote""#), "quote not escaped: {out}");
        assert!(
            out.contains(r"/back\\slash"),
            "backslash not escaped: {out}"
        );
    }

    #[test]
    fn compose_fs_genesis_source_injects_bundle_map() {
        let b = [BundleEntry {
            logical_name: "cfg/theme.json".into(),
            canon_path: PathBuf::from("/etc/myapp/theme.json"),
            kind: BundleEntryKind::File,
            mode: "r".into(),
            consensus_mode: BundleConsensusMode::Oracular,
        }];
        let src = compose_fs_genesis_source("00", "00", &b, None);
        // Slice 30c H-P7-8: File entries split into
        // (parent_dir, filename), so `/etc/myapp/theme.json`
        // becomes `("/etc/myapp", "theme.json", ...)`.
        assert!(
            src.contains(
                r#"Fs!?(0, 1, 2, {"cfg/theme.json": ("/etc/myapp", "theme.json", "r", "file", "oracular")})"#
            ),
            "bundle not injected into Fs!? call:\n{src}"
        );
    }

    #[test]
    fn compose_fs_genesis_source_empty_bundle_still_uses_empty_map() {
        let src = compose_fs_genesis_source("00", "00", &[], None);
        assert!(src.contains("Fs!?(0, 1, 2, {})"));
    }

    // -------------------- Slice 25 review-fix regression tests --------------------

    // M-25-6: `format_bundle_for_rholang` must panic on duplicate
    // logical names — the Rholang map would silently overwrite one
    // entry.  Slice 23 boot validation catches this cross-bucket; this
    // asserts defense-in-depth if a caller constructs the bundle
    // programmatically without going through validation.
    #[test]
    #[should_panic(expected = "duplicate logical name")]
    fn format_bundle_panics_on_duplicate_logical_name() {
        let b = vec![
            BundleEntry {
                logical_name: "dup".into(),
                canon_path: PathBuf::from("/a"),
                kind: BundleEntryKind::File,
                mode: "r".into(),
                consensus_mode: BundleConsensusMode::Oracular,
            },
            BundleEntry {
                logical_name: "dup".into(),
                canon_path: PathBuf::from("/b"),
                kind: BundleEntryKind::File,
                mode: "r".into(),
                consensus_mode: BundleConsensusMode::Oracular,
            },
        ];
        let _ = format_bundle_for_rholang(&b);
    }

    // C-25-2: `rholang_string_escape` must panic on any C0 control
    // character.  Previously it emitted `\r` which the Rholang lexer
    // rejects — a Windows-CRLF operator input would crash genesis.
    // Now upstream must reject before we get here; if it slips
    // through, we panic locally rather than emitting invalid source.
    #[test]
    #[should_panic(expected = "control char U+000D")]
    fn rholang_string_escape_panics_on_carriage_return() {
        let _ = rholang_string_escape("line\r\n");
    }

    #[test]
    #[should_panic(expected = "control char U+0000")]
    fn rholang_string_escape_panics_on_nul() { let _ = rholang_string_escape("a\0b"); }

    #[test]
    #[should_panic(expected = "control char U+0007")]
    fn rholang_string_escape_panics_on_bell() { let _ = rholang_string_escape("a\x07b"); }

    // Newline and tab are the two valid escapes; they must NOT panic.
    #[test]
    fn rholang_string_escape_allows_newline_and_tab() {
        assert_eq!(rholang_string_escape("a\nb"), r"a\nb");
        assert_eq!(rholang_string_escape("a\tb"), r"a\tb");
    }

    // H-25-2: promoted debug_assert! → assert! for hex validation on
    // pk_hex / sig_hex.  Release builds must reject non-hex.
    #[test]
    #[should_panic(expected = "pk_hex must be ASCII hex")]
    fn compose_fs_genesis_source_panics_on_non_hex_pk() {
        let _ = compose_fs_genesis_source("zz", "00", &[], None);
    }

    #[test]
    #[should_panic(expected = "sig_hex must be ASCII hex")]
    fn compose_fs_genesis_source_panics_on_non_hex_sig() {
        let _ = compose_fs_genesis_source("00", "zz", &[], None);
    }

    // H-25-3: `BundleEntry::try_new` is the defense-in-depth
    // constructor for programmatic callers.  Verify each rejection.
    #[test]
    fn try_new_rejects_empty_logical_name() {
        let r = BundleEntry::try_new(
            "".into(),
            PathBuf::from("/etc/x"),
            BundleEntryKind::File,
            "r".into(),
            BundleConsensusMode::Oracular,
        );
        assert!(r.is_err());
    }

    #[test]
    fn try_new_rejects_empty_mode() {
        let r = BundleEntry::try_new(
            "n".into(),
            PathBuf::from("/etc/x"),
            BundleEntryKind::File,
            "".into(),
            BundleConsensusMode::Oracular,
        );
        assert!(r.is_err());
    }

    #[test]
    fn try_new_rejects_relative_path() {
        let r = BundleEntry::try_new(
            "n".into(),
            PathBuf::from("etc/x"),
            BundleEntryKind::File,
            "r".into(),
            BundleConsensusMode::Oracular,
        );
        assert!(r.is_err(), "relative canon_path must be rejected");
    }

    #[test]
    fn try_new_rejects_nul_in_logical_name() {
        let r = BundleEntry::try_new(
            "a\0b".into(),
            PathBuf::from("/etc/x"),
            BundleEntryKind::File,
            "r".into(),
            BundleConsensusMode::Oracular,
        );
        assert!(r.is_err());
    }

    #[test]
    fn try_new_rejects_newline_in_logical_name() {
        let r = BundleEntry::try_new(
            "a\nb".into(),
            PathBuf::from("/etc/x"),
            BundleEntryKind::File,
            "r".into(),
            BundleConsensusMode::Oracular,
        );
        assert!(r.is_err());
    }

    #[test]
    fn try_new_rejects_cr_in_logical_name() {
        let r = BundleEntry::try_new(
            "a\rb".into(),
            PathBuf::from("/etc/x"),
            BundleEntryKind::File,
            "r".into(),
            BundleConsensusMode::Oracular,
        );
        assert!(r.is_err());
    }

    #[test]
    fn try_new_rejects_bom_in_logical_name() {
        let r = BundleEntry::try_new(
            "\u{FEFF}n".into(),
            PathBuf::from("/etc/x"),
            BundleEntryKind::File,
            "r".into(),
            BundleConsensusMode::Oracular,
        );
        assert!(r.is_err());
    }

    #[test]
    fn try_new_rejects_rtl_override_in_logical_name() {
        let r = BundleEntry::try_new(
            "a\u{202E}b".into(),
            PathBuf::from("/etc/x"),
            BundleEntryKind::File,
            "r".into(),
            BundleConsensusMode::Oracular,
        );
        assert!(r.is_err());
    }

    #[test]
    fn try_new_rejects_line_separator_in_logical_name() {
        let r = BundleEntry::try_new(
            "a\u{2028}b".into(),
            PathBuf::from("/etc/x"),
            BundleEntryKind::File,
            "r".into(),
            BundleConsensusMode::Oracular,
        );
        assert!(r.is_err());
    }

    #[test]
    fn try_new_accepts_valid_input() {
        let r = BundleEntry::try_new(
            "cfg/theme.json".into(),
            PathBuf::from("/etc/myapp/theme.json"),
            BundleEntryKind::File,
            "r".into(),
            BundleConsensusMode::Oracular,
        );
        assert!(r.is_ok(), "valid input rejected: {r:?}");
    }

    // MT-25-1: a non-empty-bundle composed source must actually parse
    // through the Rholang normalizer, not just format as a string.
    // Compiles the composed source via CompiledRholangSource with a
    // fake but hex-valid pk/sig to catch template drift that only
    // shows up on the parse path (e.g. missing punctuation, mismatched
    // braces).
    #[test]
    fn compose_fs_genesis_source_with_bundle_compiles() {
        use std::collections::HashMap;

        use rholang::rust::build::compile_rholang_source::CompiledRholangSource;
        let b = [BundleEntry {
            logical_name: "cfg/theme.json".into(),
            canon_path: PathBuf::from("/etc/myapp/theme.json"),
            kind: BundleEntryKind::File,
            mode: "r".into(),
            consensus_mode: BundleConsensusMode::Oracular,
        }];
        // Use realistic hex — non-hex would panic at compose time
        // (H-25-2), and a short empty-hex would fail signature
        // validation only at runtime, but that's not what we test
        // here — we just want the parser to accept the source.
        let pk_hex = "00".repeat(33);
        let sig_hex = "00".repeat(64);
        let src = compose_fs_genesis_source(&pk_hex, &sig_hex, &b, None);
        let r = CompiledRholangSource::new(src, HashMap::new(), "FsGenesisBundle".into());
        assert!(
            r.is_ok(),
            "composed source failed to compile: {}",
            r.err().map(|e| format!("{e:?}")).unwrap_or_default()
        );
    }

    // MT-25-2: escape-heavy bundle values (quote, backslash, newline,
    // tab) all round-trip through the composer + parser.
    #[test]
    fn compose_fs_genesis_source_with_escape_heavy_bundle_compiles() {
        use std::collections::HashMap;

        use rholang::rust::build::compile_rholang_source::CompiledRholangSource;
        let b = [BundleEntry {
            logical_name: "has\"quote\tand\\backslash".into(),
            canon_path: PathBuf::from("/etc/\"awkward\\path"),
            kind: BundleEntryKind::File,
            mode: "r".into(),
            consensus_mode: BundleConsensusMode::Oracular,
        }];
        let pk_hex = "00".repeat(33);
        let sig_hex = "00".repeat(64);
        let src = compose_fs_genesis_source(&pk_hex, &sig_hex, &b, None);
        let r = CompiledRholangSource::new(src, HashMap::new(), "FsGenesisEscape".into());
        assert!(
            r.is_ok(),
            "escape-heavy composed source failed to compile: {}",
            r.err().map(|e| format!("{e:?}")).unwrap_or_default()
        );
    }

    // -------------------- Slice 26: consensus-mode plumbing --------------------

    #[test]
    fn consensus_mode_as_str_round_trips() {
        assert_eq!(BundleConsensusMode::Oracular.as_str(), "oracular");
        assert_eq!(BundleConsensusMode::Consensus.as_str(), "consensus");
        assert_eq!(
            BundleConsensusMode::parse_str("oracular"),
            Some(BundleConsensusMode::Oracular)
        );
        assert_eq!(
            BundleConsensusMode::parse_str("consensus"),
            Some(BundleConsensusMode::Consensus)
        );
        assert_eq!(BundleConsensusMode::parse_str("bogus"), None);
    }

    // The Rholang boundary is a string.  A stray tuple-position drift
    // in the composer would land the cmode string in the wrong tuple
    // slot; assert the 5th element is exactly the cmode string, in
    // the exact position Fs.rho's `bMap.get(n)` match expects.
    #[test]
    fn format_bundle_emits_consensus_mode_as_5th_tuple_element() {
        let b = [
            BundleEntry {
                logical_name: "orc".into(),
                canon_path: PathBuf::from("/o"),
                kind: BundleEntryKind::File,
                mode: "r".into(),
                consensus_mode: BundleConsensusMode::Oracular,
            },
            BundleEntry {
                logical_name: "con".into(),
                canon_path: PathBuf::from("/c"),
                kind: BundleEntryKind::Dir,
                mode: "rw".into(),
                consensus_mode: BundleConsensusMode::Consensus,
            },
        ];
        let out = format_bundle_for_rholang(&b);
        // Slice 30c H-P7-8: Dir stays (canon_path, ""); File splits.
        // Here `/o` has parent `/` and filename `o`.
        assert!(
            out.contains(r#""con": ("/c", "", "rw", "dir", "consensus")"#),
            "consensus tuple missing: {out}"
        );
        assert!(
            out.contains(r#""orc": ("/", "o", "r", "file", "oracular")"#),
            "oracular tuple missing: {out}"
        );
    }

    // Composed-source parse test with mixed consensus modes: catches
    // any composed-template drift that only fires when both modes
    // appear (e.g. one arm hard-codes "oracular").
    #[test]
    fn compose_fs_genesis_source_with_mixed_consensus_modes_compiles() {
        use std::collections::HashMap;

        use rholang::rust::build::compile_rholang_source::CompiledRholangSource;
        let b = [
            BundleEntry {
                logical_name: "orc-file".into(),
                canon_path: PathBuf::from("/etc/orc"),
                kind: BundleEntryKind::File,
                mode: "rw".into(),
                consensus_mode: BundleConsensusMode::Oracular,
            },
            BundleEntry {
                logical_name: "con-dir".into(),
                canon_path: PathBuf::from("/var/con"),
                kind: BundleEntryKind::Dir,
                mode: "rw".into(),
                consensus_mode: BundleConsensusMode::Consensus,
            },
        ];
        let pk_hex = "00".repeat(33);
        let sig_hex = "00".repeat(64);
        let src = compose_fs_genesis_source(&pk_hex, &sig_hex, &b, None);
        // MT-26-18 review fix: assert both cmode strings actually
        // appear in the composed source, in the correct 5th-position
        // syntactic slot.  A template regression that hard-coded
        // `"oracular"` would still compile — this test catches that.
        // Slice 30c H-P7-8: File entries split into (parent, filename).
        // Dirs stay (canon_path, "").
        assert!(
            src.contains(r#"("/etc", "orc", "rw", "file", "oracular")"#),
            "oracular 5-tuple missing from composed source:\n{src}"
        );
        assert!(
            src.contains(r#"("/var/con", "", "rw", "dir", "consensus")"#),
            "consensus 5-tuple missing from composed source:\n{src}"
        );
        let r = CompiledRholangSource::new(src, HashMap::new(), "FsGenesisMixed".into());
        assert!(
            r.is_ok(),
            "mixed-mode composed source failed to compile: {}",
            r.err().map(|e| format!("{e:?}")).unwrap_or_default()
        );
    }

    // ST-26-21 review fix: determinism under mixed cmodes.  Running
    // compose_fs_genesis_source multiple times over the same input
    // must produce byte-identical output (composed source goes into
    // genesis-block computation; validator/proposer drift = network
    // fork).  format_bundle_for_rholang sorts by logical_name; this
    // pins the invariant so a future refactor that dropped either
    // sort would trip a test rather than fork the network.
    // CRIT-2 fix regression pin (2026-08-06): differing cadence
    // values must produce differing composed source, so the
    // deploy-term diff at `BlockApproverProtocol::validate_candidate`
    // catches leader/validator disagreement.  Pre-fix, cadence was
    // hashed into the `Genesis` STRUCT but not embedded in any
    // deploy term, so two validators with the same Genesis hash
    // (both cadence=None because bootstrap never plumbed) could
    // still write snapshots at different block heights (both using
    // their own local HOCON).  Post-fix, cadence is a Rholang
    // literal in the composed source.
    #[test]
    fn compose_fs_genesis_source_differs_when_cadence_differs() {
        let pk_hex = "00".repeat(33);
        let sig_hex = "00".repeat(64);
        let none_src = compose_fs_genesis_source(&pk_hex, &sig_hex, &[], None);
        let some1_src = compose_fs_genesis_source(&pk_hex, &sig_hex, &[], Some(100));
        let some2_src = compose_fs_genesis_source(&pk_hex, &sig_hex, &[], Some(200));
        assert_ne!(
            none_src, some1_src,
            "None-vs-Some cadence must produce differing composed source; \
             otherwise CRIT-2 divergence is undetectable at deploy-diff time"
        );
        assert_ne!(
            some1_src, some2_src,
            "differing Some cadence values must produce differing composed source"
        );
        // Positive: sanity-check that the literal actually appears
        // in the composed source.  If a future refactor drops the
        // commitment binding, the assertion above would still pass
        // (since composition uses format! and other trivia differ),
        // but this substring check would trip.
        assert!(
            some1_src.contains("snapshotCadenceCommitmentP!(100)"),
            "expected `snapshotCadenceCommitmentP!(100)` in composed source; not found"
        );
        assert!(
            some2_src.contains("snapshotCadenceCommitmentP!(200)"),
            "expected `snapshotCadenceCommitmentP!(200)` in composed source; not found"
        );
        assert!(
            none_src.contains("snapshotCadenceCommitmentP!(Nil)"),
            "expected `snapshotCadenceCommitmentP!(Nil)` in composed source; not found"
        );
    }

    #[test]
    fn compose_fs_genesis_source_is_deterministic_across_runs() {
        let b = [
            BundleEntry {
                logical_name: "z".into(),
                canon_path: PathBuf::from("/z"),
                kind: BundleEntryKind::File,
                mode: "r".into(),
                consensus_mode: BundleConsensusMode::Consensus,
            },
            BundleEntry {
                logical_name: "a".into(),
                canon_path: PathBuf::from("/a"),
                kind: BundleEntryKind::File,
                mode: "r".into(),
                consensus_mode: BundleConsensusMode::Oracular,
            },
        ];
        let pk_hex = "00".repeat(33);
        let sig_hex = "00".repeat(64);
        let baseline = compose_fs_genesis_source(&pk_hex, &sig_hex, &b, None);
        for _ in 0..20 {
            assert_eq!(
                compose_fs_genesis_source(&pk_hex, &sig_hex, &b, None),
                baseline,
                "compose_fs_genesis_source must be deterministic"
            );
        }
    }

    // A composed source with only consensus caps must still compile
    // (defense against a template arm that assumes at least one
    // oracular cap).
    #[test]
    fn compose_fs_genesis_source_with_only_consensus_compiles() {
        use std::collections::HashMap;

        use rholang::rust::build::compile_rholang_source::CompiledRholangSource;
        let b = [BundleEntry {
            logical_name: "con-only".into(),
            canon_path: PathBuf::from("/var/con"),
            kind: BundleEntryKind::File,
            mode: "rw".into(),
            consensus_mode: BundleConsensusMode::Consensus,
        }];
        let pk_hex = "00".repeat(33);
        let sig_hex = "00".repeat(64);
        let src = compose_fs_genesis_source(&pk_hex, &sig_hex, &b, None);
        let r = CompiledRholangSource::new(src, HashMap::new(), "FsGenesisConsensusOnly".into());
        assert!(
            r.is_ok(),
            "consensus-only composed source failed to compile: {}",
            r.err().map(|e| format!("{e:?}")).unwrap_or_default()
        );
    }
}
