//! Rholang agent-class sources for the File I/O FIP.
//!
//! Each constant is the raw text of an `agent` block (per rholang-rs
//! PR #94's sugar) that expects its enclosing scope to bind the
//! native-URN names it references. That factoring lets one string
//! serve two purposes:
//!
//! 1. **Integration tests** wrap the block in a `new`-scope that
//!    binds the native URNs and directly exercises the agent's
//!    methods, isolating the agent-layer semantics from the rest
//!    of the FS-agent stack.
//! 2. **The `Fs` agent's genesis deploy** (Phase 2) embeds the
//!    same block inside its own `new`-scope, which binds the
//!    native URNs via `NormalizerEnv` injection and never exposes
//!    them outward. User code reaches these agents only through
//!    the `Fs` methods that construct them.
//!
//! Native URNs each `.rho` file expects:
//!
//! - `file.rho` — `nRead`, `nWrite`, `nSeek`, `nTell`, `nSize`,
//!   `nTruncate`, `nFlush`, `nClose`, `nReadLine`, `nReadAllLines`,
//!   `nAppendLines`.
//! - `dir.rho`  — `nQuarantine`, `nEntries`, `nStat`, `nExists`,
//!   `nRemoveFile`, `nRemoveDir`, `nRename`, `nCopyFile`, `nOpen`,
//!   plus the `File` constructor channel (`openFile` composes
//!   with the File agent).

/// Source of the `File` agent block. Expects `File`, `nRead`,
/// `nWrite`, `nSeek`, `nTell`, `nSize`, `nTruncate`, `nFlush`,
/// `nClose`, `nReadLine`, `nReadAllLines`, `nAppendLines` to be
/// bound in the enclosing scope.
pub const FILE_AGENT_SRC: &str = include_str!("agents/file.rho");

/// Source of the `Dir` agent block. Expects `Dir`, `File`,
/// `nQuarantine`, `nEntries`, `nStat`, `nExists`, `nRemoveFile`,
/// `nRemoveDir`, `nRename`, `nCopyFile`, `nOpen` to be bound in
/// the enclosing scope.
///
/// The `File` binding is required because `Dir.openFile`
/// constructs a `File` instance around the fd returned by
/// `nativeOpen`. In practice this means the genesis-installed
/// `Fs` agent's outer `new`-scope must bind BOTH agent
/// classes and their combined native URN set.
pub const DIR_AGENT_SRC: &str = include_str!("agents/dir.rho");
