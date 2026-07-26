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

pub mod errors;
pub mod handle_table;
pub mod handlers;
pub mod mode;
pub mod nss;
pub mod path;
pub mod response;
pub mod stat;

/// Consensus vs. oracular execution mode.
///
/// Threaded from `ProcessContext` into every path-taking handler.  Under
/// `Consensus`, host-transient fields (`mtime`, `ctime`, `atime`, `owner`,
/// `group`) are omitted from `stat`/`entries` records, and `chown` returns
/// `FSERR_UNSUPPORTED`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConsensusMode {
    #[default]
    Oracular,
    Consensus,
}

/// Per-call size caps — spec §Efficiency + §Cost accounting.
pub const MAX_READ_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_TRUNCATE_BYTES: u64 = 16 * 1024 * 1024 * 1024;

/// Per-runtime fd cap.  Placeholder value; final calibration in the Cost FIP.
pub const MAX_OPEN_FDS: usize = 1024;
