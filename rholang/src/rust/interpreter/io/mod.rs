// Every `unsafe` block in the io/ tree MUST carry a `// SAFETY: ...`
// comment documenting the caller's precondition and the FFI's
// postcondition.  The `#![warn]` attaches to this module and
// propagates to all descendant modules (`pub mod` declarations
// below).  Scoped to io/ rather than the whole rholang crate —
// legacy sites outside io/ are not part of this discipline.
#![warn(clippy::undocumented_unsafe_blocks)]

pub mod consensus_constants;
pub mod consensus_fingerprint;
pub mod costs;
pub mod dir_handle_table;
pub mod errors;
pub mod handle_table;
pub mod handler_trait;
pub mod handlers;
pub mod lock;
pub mod mode;
pub mod nss;
pub mod path;
pub mod response;
pub mod snapshot;
pub mod snapshot_chunk;
pub mod stat;
pub mod verify;
pub mod wal;
pub mod wal_applier;

/// Consensus vs. oracular execution mode.
///
/// Threaded from `ProcessContext` into every path-taking handler.  Under
/// `Consensus`, host-transient fields (`mtime`, `ctime`, `atime`, `owner`,
/// `group`) are omitted from `stat` / `entries` records, and `chown`
/// returns `FSERR_UNSUPPORTED`.
///
/// `Default` returns `Consensus` — the more restrictive mode — so any
/// construction site that omits the mode fails closed rather than
/// silently allowing chown and leaking host metadata.  All shipping
/// call sites should be explicit; this default only matters for future
/// refactors / test scaffolds that use `..Default::default()`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConsensusMode {
    Oracular,
    #[default]
    Consensus,
}

/// Rholang-boundary string encoding of `ConsensusMode::Oracular`.
/// Kept alongside the enum so handlers in this crate and the
/// composer in `casper` both source-of-truth from one location.
/// `casper::genesis::contracts::fs_genesis::BundleConsensusMode`
/// re-exports this constant and asserts (via drift test in that
/// crate) that its Rholang-side embedding still matches.
///
/// # CONSENSUS-OBSERVABLE
///
/// Embedded in every Consensus bundle entry of the composed
/// fs_genesis source, so a rename is caught by the composed-source
/// golden hex.  Folded into the runtime fingerprint (order 8) as
/// defense-in-depth — a validator whose CMODE constants drift
/// fails the peering-handshake `network_id` check at boot, before
/// it can produce any divergent state.
pub const CMODE_ORACULAR_STR: &str = "oracular";

crate::register_consensus_constant!(order = 8, name = CMODE_ORACULAR_STR, str_bytes);

/// Rholang-boundary string encoding of `ConsensusMode::Consensus`.
/// See `CMODE_ORACULAR_STR` for provenance + consensus-surface
/// discussion.  Folded into the runtime fingerprint at order 9.
pub const CMODE_CONSENSUS_STR: &str = "consensus";

crate::register_consensus_constant!(order = 9, name = CMODE_CONSENSUS_STR, str_bytes);

/// Parse a caller-supplied `Par` into a [`ConsensusMode`].  Returns
/// `None` for malformed input (non-String Par, unknown string).
/// The dispatcher's step-4 is_replay short-circuit treats `None`
/// as "fall through to Oracular echo" — matches pre-trait handler
/// behavior where an unresolved cmode on replay tautologically
/// echoed `previous`.
///
/// Accepted values (byte-for-byte against the registered
/// consensus constants above): `"oracular"` → [`ConsensusMode::Oracular`];
/// `"consensus"` → [`ConsensusMode::Consensus`].  Any other string
/// (including capitalization variants) returns `None`.
pub fn resolve_cmode(cmode_par: &models::rhoapi::Par) -> Option<ConsensusMode> {
    let s = crate::rust::interpreter::rho_type::RhoString::unapply(cmode_par)?;
    match s.as_str() {
        CMODE_ORACULAR_STR => Some(ConsensusMode::Oracular),
        CMODE_CONSENSUS_STR => Some(ConsensusMode::Consensus),
        _ => None,
    }
}

/// URI prefix of every `rho:io:fs:native:*` URN.  Kept alongside
/// the handler definitions so the reducer's phase-scoped URN
/// filter and `casper::genesis::contracts::fs_genesis::
/// FS_NATIVE_URN_PREFIX` both source-of-truth from one location.
///
/// The reducer will refuse to resolve URNs starting with this
/// prefix during state-execution deploys
/// (`play_deploys_for_state`); genesis deploys
/// (`play_deploys_for_genesis`) get an exemption so the composed
/// FsGenesis source can bind them.  Not registered in
/// `CONSENSUS_FOLD` — the URN string is host-transient (any change
/// to the prefix would also flip the composed-fs-genesis source
/// golden hex, which is the canonical peer-parity guard for
/// genesis-embedded strings).
pub const FS_NATIVE_URN_PREFIX: &str = "rho:io:fs:native:";

/// Per-call byte cap on `fs_read` / `fs_read_at` — spec §Efficiency
/// + §Cost accounting.  A read request larger than this surfaces
/// `FSERR_QUOTA_EXCEEDED`.
///
/// # CONSENSUS-OBSERVABLE
///
/// A divergent cap produces different `FSERR_QUOTA_EXCEEDED`
/// distributions on identical inputs and forks the tuplespace.
/// Folded into the runtime fingerprint at order 4.
pub const MAX_READ_BYTES: u64 = 64 * 1024 * 1024;

// Compile-time floor: reads below 1 MiB would surface
// FSERR_QUOTA_EXCEEDED on any legitimate large read.  Tripwire for
// a Cost FIP miscalibration by a factor of 64+.
const _: () = assert!(
    MAX_READ_BYTES >= 1024 * 1024,
    "MAX_READ_BYTES below 1 MiB — a divergent lower cap forks \
     consensus at every legitimate large-read workload"
);

crate::register_consensus_constant!(order = 4, name = MAX_READ_BYTES, u64_be);

/// Per-call byte cap on `fs_truncate` — spec §Efficiency +
/// §Cost accounting.  A truncate request that would move the file
/// end past this offset surfaces `FSERR_QUOTA_EXCEEDED`.
/// Consensus-observable; folded at order 5.
pub const MAX_TRUNCATE_BYTES: u64 = 16 * 1024 * 1024 * 1024;

// Compile-time floor: truncations below 4 GiB would fork consensus
// on any legitimate large-file workload.
const _: () = assert!(
    MAX_TRUNCATE_BYTES >= 4 * 1024 * 1024 * 1024,
    "MAX_TRUNCATE_BYTES below 4 GiB — a divergent lower cap forks \
     consensus on legitimate large-file truncate workloads"
);

crate::register_consensus_constant!(order = 5, name = MAX_TRUNCATE_BYTES, u64_be);

/// Per-runtime cap on concurrently open file descriptors.  Placeholder
/// value; final calibration in the Cost FIP.  A `fs_open` request
/// beyond this cap surfaces `FSERR_QUOTA_EXCEEDED`.
///
/// # CONSENSUS-OBSERVABLE
///
/// Divergent caps fork consensus (open request N + 1 succeeds on
/// one peer, fails on another).  Folded at order 6.
pub const MAX_OPEN_FDS: usize = 1024;

// Compile-time floor: fd caps below 256 (matching typical Unix
// `ulimit -n` defaults) would fork consensus at trivially small
// open counts.
const _: () = assert!(
    MAX_OPEN_FDS >= 256,
    "MAX_OPEN_FDS below 256 — a divergent lower cap forks consensus \
     at trivially small concurrent-open counts"
);

crate::register_consensus_constant!(order = 6, name = MAX_OPEN_FDS, u64_be);

/// Per-call cap on `Stream.chunk(n)` — the Rholang spec's `chunk`
/// method guarantees a minimum of 1024; we set 65_536 above the
/// floor.  Cross-language drift between this constant and the
/// Rholang literal `65536` is (will be) caught by a source-scan
/// pin in the eventual `fileio_cost_spec.rs`.  Consensus-observable;
/// folded at order 7.
pub const MAX_CHUNK_ITEMS: u64 = 65_536;

// Compile-time floor: chunk cap below the Rholang spec's 1024
// minimum would surface FSERR_QUOTA_EXCEEDED on every well-formed
// chunk request and fork consensus at every legitimate workload.
const _: () = assert!(
    MAX_CHUNK_ITEMS >= 1024,
    "MAX_CHUNK_ITEMS below the Rholang spec floor of 1024 — a lower \
     cap surfaces FSERR_QUOTA_EXCEEDED on legitimate chunk requests \
     and forks consensus at every well-formed workload"
);

crate::register_consensus_constant!(order = 7, name = MAX_CHUNK_ITEMS, u64_be);

/// Per-call cap on `fs_entries` output size — prevents a malicious
/// caller pointing the native at a million-entry directory and
/// OOMing the node.  Rholang-side alternative for large directories
/// is `entriesStreamOpen` / `_Next` / `_Close`.  Consensus-
/// observable (divergent caps fork at the `FSERR_QUOTA_EXCEEDED`
/// boundary); folded at order 16.
pub const MAX_ENTRIES: usize = 65_536;

// Compile-time floor: entry cap below 4096 would surface
// FSERR_QUOTA_EXCEEDED on legitimate medium directories and fork
// consensus at every well-formed enumerate workload.
const _: () = assert!(
    MAX_ENTRIES >= 4096,
    "MAX_ENTRIES below 4096 — a divergent lower cap forks consensus \
     at legitimate medium-directory enumerate workloads"
);

crate::register_consensus_constant!(order = 16, name = MAX_ENTRIES, u64_be);

/// Composition-time nonce embedded into the composed fs_genesis
/// source (`new_gint_par(FS_NONCE, ...)` in the signed-registry
/// insertion).  A drift is caught by the composed-source golden
/// hex; folding here at order 10 catches it via the peering-
/// handshake `network_id` mismatch instead — a validator whose
/// `FS_NONCE` differs from `i64::MAX` refuses to peer with the
/// canonical fleet at boot.
///
/// The nonce lives in `rholang` rather than `casper` so the
/// `linkme::distributed_slice` registration lives in the same
/// crate as the other fingerprint entries (rholang-lib-only tests
/// must see the same fold count as the full-node binary; a casper-
/// side registration would be invisible to `cargo test -p rholang
/// --lib`).  A future `casper::fs_genesis::FS_NONCE` re-exports
/// this value for backwards compatibility.
pub const FS_NONCE: i64 = i64::MAX;

crate::register_consensus_constant!(order = 10, name = FS_NONCE, i64_be);

#[cfg(test)]
mod tests {
    use super::*;

    /// Fail-closed default pin: any refactor that swaps the
    /// `#[default]` to `Oracular` is a security regression — a
    /// construction site that omits the mode would silently allow
    /// `chown` and leak host metadata.
    #[test]
    fn consensus_mode_default_is_consensus() {
        assert_eq!(ConsensusMode::default(), ConsensusMode::Consensus);
    }

    /// Hard-fork surface pin.  A cross-peer disagreement on any of
    /// these constants surfaces `FSERR_QUOTA_EXCEEDED` on different
    /// inputs and forks the tuplespace.
    #[test]
    fn byte_gate_constants_pinned() {
        assert_eq!(MAX_READ_BYTES, 64 * 1024 * 1024);
        assert_eq!(MAX_TRUNCATE_BYTES, 16 * 1024 * 1024 * 1024);
        assert_eq!(MAX_OPEN_FDS, 1024);
        assert_eq!(MAX_CHUNK_ITEMS, 65_536);
    }

    /// String pins on the CMODE Rholang-boundary tags.  A rename
    /// here without a matching update to `casper::genesis::contracts::
    /// fs_genesis::BundleConsensusMode` would break the composed-
    /// source golden hex; folding into the runtime fingerprint
    /// double-catches it via the peering handshake.
    #[test]
    fn cmode_string_constants_pinned() {
        assert_eq!(CMODE_ORACULAR_STR, "oracular");
        assert_eq!(CMODE_CONSENSUS_STR, "consensus");
    }

    /// The CMODE tags reach Rholang as lowercase-ASCII identifiers.
    /// Pin the lowercase invariant so a future refactor that
    /// introduces a mixed-case variant (`"Oracular"`, `"CONSENSUS"`)
    /// trips CI before Rholang-side comparison semantics silently
    /// break.  Non-ASCII characters would also trip this — the
    /// comparison covers both cases via `to_lowercase()` semantics.
    #[test]
    fn cmode_string_constants_are_lowercase_ascii() {
        assert_eq!(CMODE_ORACULAR_STR, CMODE_ORACULAR_STR.to_lowercase());
        assert_eq!(CMODE_CONSENSUS_STR, CMODE_CONSENSUS_STR.to_lowercase());
        assert!(CMODE_ORACULAR_STR.chars().all(|c| c.is_ascii_lowercase()));
        assert!(CMODE_CONSENSUS_STR.chars().all(|c| c.is_ascii_lowercase()));
    }

    /// FS_NONCE = `i64::MAX`.  Pinning the exact value rather than
    /// just the type — a future refactor that changes to (say)
    /// `i64::MAX - 1` would silently roll the composed-source
    /// golden hex on every rebuild.
    #[test]
    fn fs_nonce_pinned_at_i64_max() {
        assert_eq!(FS_NONCE, i64::MAX);
    }

    /// URN prefix is source-of-truth for the reducer's phase-scoped
    /// filter AND the composed fs_genesis binding.  A rename that
    /// touches only one site silently breaks the other (bindings
    /// would exist under the old prefix; the filter would let user
    /// deploys bypass it under the new prefix).  Not consensus-
    /// observable in the fingerprint sense (host-transient), but
    /// still a two-site invariant worth pinning.
    #[test]
    fn fs_native_urn_prefix_pinned() {
        assert_eq!(FS_NATIVE_URN_PREFIX, "rho:io:fs:native:");
    }
}
