// File I/O native primitives — Phase 1 of the File I/O FIP.
//
// This module exposes the 22 syscall bridges backing the Rholang-side
// `Fs`/`File`/`Dir` agent library (Phases 5-6, which are blocked on the
// prerequisite Agents/Private-Methods/Try-Catch/Versioned-Registry FIPs).
//
// URNs live under `rho:io:fs:native:1.0.0/*` and are registered in
// `std_system_processes()` but filtered out of the user-reachable
// `urn_map` — they are only reachable through the genesis-installed
// `Fs` agent.

pub mod consensus_fingerprint;
pub mod costs;
pub mod errors;
pub mod handle_table;
pub mod handlers;
pub mod lock;
pub mod mode;
pub mod nss;
pub mod path;
pub mod response;
pub mod snapshot;
pub mod stat;
pub mod wal;

/// Consensus vs. oracular execution mode.
///
/// Threaded from `ProcessContext` into every path-taking handler.  Under
/// `Consensus`, host-transient fields (`mtime`, `ctime`, `atime`, `owner`,
/// `group`) are omitted from `stat`/`entries` records, and `chown` returns
/// `FSERR_UNSUPPORTED`.
///
/// H-26-F3 review fix: `Default` returns `Consensus` — the more restrictive
/// mode — so any construction site that omits the mode fails closed rather
/// than silently allowing chown and leaking host metadata.  All slice-26
/// call sites are explicit; this default only matters for future refactors
/// / test-scaffolds that use `..Default::default()`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConsensusMode {
    Oracular,
    #[default]
    Consensus,
}

/// Rholang-boundary string encoding of `ConsensusMode` (slice 26).  Kept
/// here alongside the enum so handlers in this crate and the composer in
/// casper both source-of-truth from one location.  `BundleConsensusMode`
/// in `casper/src/rust/genesis/contracts/fs_genesis.rs` re-exports these
/// constants and asserts (via drift test) that they still match.
pub const CMODE_ORACULAR_STR: &str = "oracular";
pub const CMODE_CONSENSUS_STR: &str = "consensus";

/// Slice 31: URI prefix of every rho:io:fs:native:* URN.  Kept here
/// alongside the handler definitions so the reducer's phase-scoped
/// URN filter and `casper::genesis::contracts::fs_genesis::
/// FS_NATIVE_URN_PREFIX` both source-of-truth from one location.
/// The reducer refuses to resolve URNs starting with this prefix
/// during state-execution deploys (`play_deploys_for_state`); genesis
/// deploys (`play_deploys_for_genesis`) get an exemption so the
/// composed FsGenesis source can bind them.
pub const FS_NATIVE_URN_PREFIX: &str = "rho:io:fs:native:";

/// Per-call size caps — spec §Efficiency + §Cost accounting.
pub const MAX_READ_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_TRUNCATE_BYTES: u64 = 16 * 1024 * 1024 * 1024;

/// Per-runtime fd cap.  Placeholder value; final calibration in the Cost FIP.
pub const MAX_OPEN_FDS: usize = 1024;
