// Consensus-mode Write-Ahead Log — data types (slice 1 of the
// `wal` submodule tree).
//
// This slice provides the pure-data foundations:
//   - `MAX_WAL_ENTRIES` — per-runtime cap (consensus-observable).
//   - `WalEntry` / `WalOutcome` / `WalOp` / `PayloadRef` — the
//     entry shape and its variant enumerations.
//   - `WalMark` — opaque length marker for deploy-scope drains.
//
// The `Wal` buffer struct + `PayloadPersistence` /
// `PayloadSourceRecorder` traits + append/drain/mark logic land in
// subsequent Wave 2 slices.
//
// # Consensus surface
//
// Three constants register into `CONSENSUS_FOLD`:
//   1. `MAX_WAL_ENTRIES` — the per-runtime cap; a divergent value
//      surfaces `FSERR_QUOTA_EXCEEDED` on different inputs across
//      peers and forks the tuplespace.
//   2. `WAL_OUTCOME_VARIANTS` — a `#[repr(u8)]` add/remove flips
//      the WAL entry wire encoding.
//   3. `WAL_OP_VARIANTS` — same rationale as `WAL_OUTCOME_VARIANTS`.
//
// The `#[repr(u8)]` + explicit `= N` discriminants on `WalOp` and
// `WalOutcome` pin the wire encoding at the source level.  Pre-pin,
// Rust's default enum layout could theoretically shift under
// compiler / edition changes → silent WAL wire divergence across a
// validator binary rebuild.  The runtime discriminant-pin tests
// (`wal_op_discriminants_pinned` / `wal_outcome_discriminants_pinned`)
// guard the exact `as u8` values; `WAL_*_VARIANTS` in the
// fingerprint catches add/remove.  Together: reorder, add, and
// remove are all covered.
//
// # Payload references
//
// Per the design plan: WAL rows carry a cryptographic hash of the
// bytes, not the bytes themselves.  This MVP uses
// `PayloadRef::Hash([u8; 32])` (Blake2b256) for every write; the
// `DeployRef` optimization (block-hash + deploy-index + arg-index)
// is a forward-compatible variant kept in the enum so consumers
// can pattern-match today and gain the optimization later without
// a wire-format change.

use std::path::PathBuf;

use crypto::rust::hash::blake2b256::Blake2b256;

/// Per-runtime cap on WAL entries.  Prevents an adversarial deploy
/// from growing the WAL without bound.  Enforced by the (yet-to-
/// land) `Wal::append` method, which returns `Err(())` on overflow
/// and translates to `FSERR_QUOTA_EXCEEDED` at the handler layer.
///
/// Set to 65_536 as a rough analog to `MAX_OPEN_FDS = 1024` scaled
/// up for the higher-throughput write-op vs. long-lived-handle
/// distinction; final calibration is a Cost FIP concern.
///
/// # CONSENSUS-OBSERVABLE
///
/// Every validator on the network MUST agree on this value — a
/// divergent cap would produce different `FSERR_QUOTA_EXCEEDED`
/// reply distributions on identical inputs and fork consensus at
/// the tuplespace level.  Any change here is a coordinated network
/// hard fork.  Pinned by `max_wal_entries_pinned_at_65536` and
/// folded into the runtime fingerprint (order 1).
pub const MAX_WAL_ENTRIES: usize = 65_536;

// Compile-time floor check.  Ensures the cap is above any
// realistic single-deploy legitimate use so overflow triggers
// only on adversarial workloads (per-deploy WAL entries are
// bounded by contract cost, which caps around 10k in practice).
// If a future cost calibration lowers this, the const-assert
// forces the maintainer to think about consensus impact.
const _: () = assert!(
    MAX_WAL_ENTRIES >= 1024,
    "MAX_WAL_ENTRIES below 1024 would surface FSERR_QUOTA_EXCEEDED \
     on legitimate workloads — a divergent lower cap would fork consensus"
);

crate::register_consensus_constant!(order = 1, name = MAX_WAL_ENTRIES, u64_be);

/// Opaque marker returned by `Wal::begin_deploy` and consumed by
/// `Wal::take_deploy_entries` (both in a subsequent slice).
/// Records the WAL length at the deploy boundary so post-deploy
/// drain covers exactly the entries this deploy contributed.  Also
/// usable by soft-checkpoint machinery as a snapshot point to
/// truncate back to on revert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalMark {
    pub(crate) len: usize,
}

impl WalMark {
    /// Length recorded at the moment this mark was minted.  Public
    /// so the (subsequent-slice) `Wal::take_deploy_entries` can
    /// slice the buffer from `mark.len()` to the current tail.
    pub fn len(&self) -> usize { self.len }

    /// Convenience predicate — a zero-length mark is the initial
    /// state before any deploy has appended.
    pub fn is_empty(&self) -> bool { self.len == 0 }
}

/// One journaled mutation or observation.  Ordered by insertion
/// into the WAL — the replay protocol applies (or verifies) entries
/// in insertion order.
///
/// # In-memory form vs. on-wire form
///
/// The struct as declared here is the **in-memory** representation
/// used at the handler layer, where `path` / `extra_path` are
/// resolved host paths (canonicalized-lexical against the
/// `Root::path()` from `identity::RootIdentityRegistry`).  Host
/// paths differ per validator — every operator's on-disk provisioned
/// root lives at a different filesystem location — so the WAL's
/// **on-wire** form MUST NOT carry them directly.  When the
/// serialization slice lands (later Wave 2), each `path` /
/// `extra_path` will be encoded as `(logical_root_id, rel)` and
/// resolved back to a host path via `RootIdentityRegistry::get` on
/// load.  A future contributor: do NOT `Serialize` this struct as
/// declared — round-trip through the logical-path encoder.
///
/// # Field-population contract
///
/// Fields are `Option<T>` because different `WalOp` variants
/// populate different subsets — the docstring on each field
/// enumerates which ops fill it.  Nothing at the type level
/// enforces the per-op contract today; the coming typed builders
/// (`Wal::append_write(fd, bytes)`, `Wal::append_chmod(root, rel,
/// mode)`, etc., landing with the `Wal` buffer slice) will make
/// the contract structural — raw `WalEntry { .. }` construction
/// becomes internal, only the typed builders leak to callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalEntry {
    pub op: WalOp,
    /// In-memory: canonical host path of the target file.  On-wire:
    /// serialized as `(logical_root_id, rel)` via the (yet-to-land)
    /// serialization slice — see the struct-level docstring's
    /// "In-memory form vs. on-wire form" section.
    pub path: PathBuf,
    /// Additional target path — only populated for `Rename` and
    /// `CopyFile` (the destination).  Same in-memory-vs-on-wire
    /// discipline as `path`.
    pub extra_path: Option<PathBuf>,
    /// Offset for `WriteAt` and `Truncate`; `None` otherwise.
    pub offset: Option<u64>,
    /// Byte length for `Write` / `WriteAt` (total bytes written to
    /// the target) or observation ops that carry a side-band count;
    /// `None` for ops that derive count from the hashed reply.
    pub length: Option<u64>,
    /// Reference to the write payload.  `None` for non-write ops
    /// (chmod / chown / rename / remove / copy carry their args
    /// inline via other fields; the payload_ref slot is
    /// specifically for byte content of a write or the hashed
    /// reply of an observation op).
    pub payload_ref: Option<PayloadRef>,
    /// Mode bits for `Chmod`; `None` otherwise.
    pub mode_bits: Option<u32>,
    /// Owner + group strings for `Chown`; empty otherwise.  Kept as
    /// String not uid/gid because chown at the syscall boundary
    /// accepts names (resolved via NSS on the host).
    ///
    /// These fields only appear on entries with `op = WalOp::Chown`,
    /// which the Rholang handler layer produces **only under
    /// `ConsensusMode::Oracular`** — `Consensus` rejects `chown` at
    /// `FSERR_UNSUPPORTED` before journaling reaches this op (see
    /// `io::ConsensusMode` in `mod.rs`).  So the "replay is operator-
    /// responsible; they need matching NSS on every replaying node"
    /// caveat is scoped away for consensus: no consensus-replayed
    /// WAL contains `Chown` entries, so NSS drift can never
    /// influence tuplespace state.  Under Oracular the entry exists
    /// only for host-local audit / observability, not consensus.
    pub owner: Option<String>,
    pub group: Option<String>,
    /// Whether the underlying syscall SUCCEEDED or FAILED on the
    /// leader.  Reserve-pattern callers append with `Success`
    /// optimistically before the syscall runs; a post-syscall
    /// finalize updates the entry to `Failure { code }` when the
    /// reply carries an error.  Replayers reading a `Failure`
    /// entry MUST NOT apply it to reconstructed state — the leader
    /// never wrote anything to disk.  Without this field the WAL
    /// commits "requested payload was written" for syscalls that
    /// actually returned EIO/ENOSPC/EROFS, forcing followers to
    /// diverge from the leader's on-disk state.
    pub outcome: WalOutcome,
}

/// Outcome of the syscall the WAL entry represents.
///
/// `#[repr(u8)]` + explicit discriminants pin the wire encoding at
/// the source level.  A silent add/reorder would produce a
/// different WAL byte encoding across a rebuilt validator binary
/// even when the source-level enum shape looks identical.  The
/// `wal_outcome_discriminants_pinned` test guards each variant's
/// exact `as u8`; `WAL_OUTCOME_VARIANTS` in the fingerprint
/// catches add/remove.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WalOutcome {
    /// The syscall completed successfully.  Reserve-pattern
    /// placeholders default to this — the leader finalizes to
    /// `Failure` if the syscall reply carries an error.
    Success = 0,
    /// The syscall failed with the given upstream FSERR_* code.
    /// Followers MUST NOT apply the entry's mutation to
    /// reconstructed state.
    Failure { code: u32 } = 1,
}

/// Total variant count of `WalOutcome`.  Registered in
/// `CONSENSUS_FOLD` so any add/remove is a fingerprint-hex roll
/// (visible in code review + peering-handshake mismatch).  Bump
/// this constant AND the golden hex if a variant is added.
pub const WAL_OUTCOME_VARIANTS: usize = 2;

crate::register_consensus_constant!(order = 2, name = WAL_OUTCOME_VARIANTS, u64_be);

/// Enumeration of consensus-observable filesystem operations
/// captured in the WAL.  Covers both mutations AND observation-
/// preserving reads whose results feed the tuplespace (read-hash
/// verification pattern).
///
/// `#[repr(u8)]` + explicit `= N` discriminants pin the wire
/// encoding.  Adding a variant → bump `WAL_OP_VARIANTS`, append at
/// the tail with the next discriminant, roll the fingerprint
/// golden hex.  NEVER reorder existing variants (each variant's
/// discriminant is a hard-fork surface embedded in every WAL
/// entry's leading byte).  The `wal_op_discriminants_pinned` test
/// pins the exact assignments; the fingerprint fold pins the
/// count across validators.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WalOp {
    /// `fs_write(fd, bytes)` — sequential write; offset = None,
    /// length = bytes.len(), payload_ref = Hash(blake2b(bytes)).
    Write = 0,
    /// `fs_write_at(fd, off, bytes)` — positional write;
    /// offset = off, length = bytes.len(), payload_ref = Hash(...).
    WriteAt = 1,
    /// `fs_truncate(fd, n)` — file length becomes n; offset = n,
    /// length = None, payload_ref = None.
    Truncate = 2,
    /// `fs_chmod(root, rel, mode)` — mode_bits = Some(mode).
    Chmod = 3,
    /// `fs_chown(root, rel, owner, group)` — owner / group
    /// populated.  Produced **only under `ConsensusMode::Oracular`**;
    /// `Consensus` rejects `chown` at `FSERR_UNSUPPORTED` upstream,
    /// so this variant never appears in a consensus-replayed WAL.
    /// See the field docs on `WalEntry.owner` / `WalEntry.group`
    /// for the NSS drift discussion.
    Chown = 4,
    /// `fs_remove_file(root, rel)`.
    RemoveFile = 5,
    /// `fs_remove_dir(root, rel)` — non-recursive AND recursive
    /// share this variant; the recursive-manifest side of the
    /// remove lands separately.
    RemoveDir = 6,
    /// `fs_rename(root, rel, dest_rel)` — extra_path = Some(dest).
    Rename = 7,
    /// `fs_copy_file(root, src_rel, dest_rel)` —
    /// extra_path = Some(dest); payload_ref hashes the copied
    /// content.
    CopyFile = 8,
    /// `fs_read(fd, n) -> bytes` — sequential read.  Records
    /// `Hash(returned_bytes)` so a joining validator can verify a
    /// byte-identical read against reconstructed state produces
    /// the same hash.  offset = None; length = returned_bytes.len();
    /// payload_ref = Hash(blake2b(returned_bytes)).
    Read = 9,
    /// `fs_read_at(fd, off, n) -> bytes` — positional read.
    /// offset = off; length = returned_bytes.len();
    /// payload_ref = Hash(...).
    ReadAt = 10,
    /// `fs_stat(root, rel, cmode) -> record` — journaled on
    /// Consensus caps only.  offset/length None;
    /// payload_ref = Hash(stable_hash(reply_par)).  Outcome
    /// distinguishes Success replies from Failure(code).
    Stat = 11,
    /// `fs_entries(root, rel, cmode) -> [record, ...]` — journaled
    /// on Consensus caps.  The entry count is derivable from the
    /// hashed reply, not surfaced as a side-band length.
    Entries = 12,
    /// `fs_size(fd) -> u64` — journaled when the fd's
    /// `cmode == Consensus`.  The u64 size is inside the hashed
    /// reply.
    Size = 13,
    /// `entriesStreamNext(streamFd)` — one Next call on a
    /// Consensus-cap dir stream.  Journaled per call, symmetric on
    /// both leader (fresh readdir reply) and follower (cached
    /// previous reply).  length = `Some(1)` for
    /// `[true, entryRecord]`, `Some(0)` for `[false, "EOS"]` or an
    /// error tuple.
    EntriesStreamNext = 14,
    /// `fs_exists(root, rel, cmode) -> [true, Bool]` — journaled
    /// on Consensus caps only.  The Bool value is inside the
    /// hashed reply.
    Exists = 15,
}

/// Total variant count of `WalOp`.  Registered in `CONSENSUS_FOLD`
/// so any add/remove is a fingerprint-hex roll.  Bump this + the
/// golden hex if a variant is added.
pub const WAL_OP_VARIANTS: usize = 16;

crate::register_consensus_constant!(order = 3, name = WAL_OP_VARIANTS, u64_be);

impl WalOp {
    /// True iff this op is an observation-only WAL entry —
    /// `Read` / `ReadAt` / `Stat` / `Entries` / `Size` /
    /// `EntriesStreamNext` / `Exists` — whose `payload_ref`
    /// records the hash of a Rholang reply Par for consensus
    /// verification by the follower (re-execute the syscall,
    /// rehash, compare).  These hashes are NEVER served by any
    /// peer's payload_store; the fetch protocol does not
    /// participate in observation-op verification.
    ///
    /// The complementary predicate — "op that MAY need sidecar
    /// bytes at boot" — is `!self.is_observation_only()`.
    pub const fn is_observation_only(self) -> bool {
        matches!(
            self,
            WalOp::Read
                | WalOp::ReadAt
                | WalOp::Stat
                | WalOp::Entries
                | WalOp::Size
                | WalOp::EntriesStreamNext
                | WalOp::Exists
        )
    }
}

/// Reference to write payload bytes.  The MVP uses `Hash` only;
/// the `DeployRef` optimization lands with the block-context
/// plumbing slice.  `DeployRef` is kept in the enum so consumers
/// can pattern-match forward-compatibly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadRef {
    /// Blake2b256 hash of the payload bytes.  Recipients of the
    /// WAL look up the actual bytes via the separate byte-payload
    /// distribution sub-protocol.
    Hash([u8; 32]),
    /// Reference into a deploy from a specific block position:
    /// `(block_hash, deploy_index, arg_index)`.  Followers can
    /// reconstruct the payload directly from the on-chain deploy
    /// data, avoiding the byte-payload sub-protocol.  Not emitted
    /// by the MVP.
    #[allow(dead_code)]
    DeployRef {
        block_hash: [u8; 32],
        deploy_index: u32,
        arg_index: u32,
    },
}

impl PayloadRef {
    /// Hash-only convenience constructor.  Panics if the crate's
    /// `Blake2b256::hash` doesn't return a 32-byte digest (a
    /// misconfiguration bug, not a runtime failure mode).
    ///
    /// TODO(follow-up, cross-crate): `crypto::rust::hash::blake2b256::
    /// Blake2b256::hash` takes `Vec<u8>` by value, forcing the
    /// `.to_vec()` copy of every input.  For a WAL that may hash up
    /// to `MAX_WAL_ENTRIES` (65_536) payloads on hot paths, the
    /// redundant alloc adds up.  Tracked with the same TODO on
    /// `snapshot_chunk::hash_leaf` — fix the crypto crate API to
    /// accept `&[u8]` and both callers drop the clone.
    pub fn hash(bytes: &[u8]) -> Self {
        let h = Blake2b256::hash(bytes.to_vec());
        assert_eq!(h.len(), 32, "Blake2b256 must produce 32-byte digest");
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&h);
        PayloadRef::Hash(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hard-fork surface: MAX_WAL_ENTRIES stays at 65_536.  A
    /// change is a coordinated peer-upgrade event — pin here so an
    /// accidental edit trips CI before it ships.
    #[test]
    fn max_wal_entries_pinned_at_65536() {
        assert_eq!(MAX_WAL_ENTRIES, 65_536);
    }

    /// Wire-encoding pin: every `WalOp` variant's `as u8` must
    /// stay at the assigned discriminant.  A reorder or new-in-
    /// middle insertion silently flips the leading byte of every
    /// WAL entry using the affected op → wire divergence across
    /// validator binary rebuilds.  Table below is the authoritative
    /// reference; treat any change as a hard fork.
    #[test]
    fn wal_op_discriminants_pinned() {
        for (variant, expected) in [
            (WalOp::Write, 0u8),
            (WalOp::WriteAt, 1),
            (WalOp::Truncate, 2),
            (WalOp::Chmod, 3),
            (WalOp::Chown, 4),
            (WalOp::RemoveFile, 5),
            (WalOp::RemoveDir, 6),
            (WalOp::Rename, 7),
            (WalOp::CopyFile, 8),
            (WalOp::Read, 9),
            (WalOp::ReadAt, 10),
            (WalOp::Stat, 11),
            (WalOp::Entries, 12),
            (WalOp::Size, 13),
            (WalOp::EntriesStreamNext, 14),
            (WalOp::Exists, 15),
        ] {
            assert_eq!(
                variant as u8, expected,
                "WalOp::{variant:?} discriminant drifted from {expected} — \
                 this is a WAL wire-format hard fork"
            );
        }
    }

    /// Wire-encoding pin: `WalOutcome` variants stay at
    /// Success = 0, Failure = 1.  A reorder swaps the leading
    /// outcome byte of every WAL entry → same wire-divergence
    /// hazard as `WalOp`.
    ///
    /// `WalOutcome::Failure { code: u32 }` carries a payload, so
    /// the direct `as u8` cast that works on field-less enums
    /// won't compile.  `#[repr(u8)]` guarantees the discriminant
    /// is the first byte of the value's storage — read it via a
    /// pointer cast.
    #[test]
    fn wal_outcome_discriminants_pinned() {
        // SAFETY for all three reads below: `WalOutcome` is
        // `#[repr(u8)]`, so the first byte of any live value is
        // its discriminant regardless of the variant's payload.
        // The reference outlives the pointer read.
        let success_disc = unsafe { *(&WalOutcome::Success as *const _ as *const u8) };
        let failure_zero = unsafe { *(&WalOutcome::Failure { code: 0 } as *const _ as *const u8) };
        let failure_max =
            unsafe { *(&WalOutcome::Failure { code: u32::MAX } as *const _ as *const u8) };
        assert_eq!(success_disc, 0);
        assert_eq!(failure_zero, 1);
        // Payload variance must not affect the discriminant byte.
        assert_eq!(failure_max, 1);
    }

    /// Count parity: `WAL_OP_VARIANTS` matches the actual variant
    /// count.  If a new variant is added without bumping the const,
    /// this pin fires — a reader that trusts `WAL_OP_VARIANTS` for
    /// wire framing would silently underread otherwise.
    #[test]
    fn wal_op_variants_matches_authoritative_count() {
        // Authoritative count comes from `wal_op_discriminants_pinned`'s
        // table (16 rows).  Duplicated deliberately: a new variant
        // requires updating both this count AND the discriminant table
        // — a single-side edit trips one of these two tests, not both.
        assert_eq!(WAL_OP_VARIANTS, 16);
    }

    #[test]
    fn wal_outcome_variants_matches_authoritative_count() {
        assert_eq!(WAL_OUTCOME_VARIANTS, 2);
    }

    /// Pins the mutation-vs-observation split.  Adding a new
    /// observation-only op MUST also add it to
    /// `is_observation_only`'s match arm — this table catches the
    /// omission.  A mis-classified mutation as observation-only
    /// would cause `wal_applier::apply_wal_to_fresh_tree` to skip a
    /// state-mutating replay step, silently forking follower
    /// state.  The reverse (obs as mutation) would enqueue reply
    /// hashes for peer fetch that no peer ever served — timeout at
    /// boot.
    #[test]
    fn wal_op_is_observation_only_split_pinned() {
        for (variant, expected) in [
            (WalOp::Write, false),
            (WalOp::WriteAt, false),
            (WalOp::Truncate, false),
            (WalOp::Chmod, false),
            (WalOp::Chown, false),
            (WalOp::RemoveFile, false),
            (WalOp::RemoveDir, false),
            (WalOp::Rename, false),
            (WalOp::CopyFile, false),
            (WalOp::Read, true),
            (WalOp::ReadAt, true),
            (WalOp::Stat, true),
            (WalOp::Entries, true),
            (WalOp::Size, true),
            (WalOp::EntriesStreamNext, true),
            (WalOp::Exists, true),
        ] {
            assert_eq!(
                variant.is_observation_only(),
                expected,
                "WalOp::{variant:?} misclassified — expected \
                 is_observation_only = {expected}"
            );
        }
    }

    /// `PayloadRef::hash` produces the same bytes as
    /// `Blake2b256::hash` directly.  Regression pin — a refactor
    /// swapping the underlying hash function would silently
    /// diverge every WAL payload reference on the network.
    #[test]
    fn payload_ref_hash_matches_blake2b256_directly() {
        let bytes = b"consensus-fixture-payload";
        let PayloadRef::Hash(computed) = PayloadRef::hash(bytes) else {
            panic!("PayloadRef::hash must yield the Hash variant");
        };
        let expected_vec = Blake2b256::hash(bytes.to_vec());
        assert_eq!(computed.as_slice(), expected_vec.as_slice());
    }

    /// Empty input produces a deterministic Blake2b256 digest —
    /// pin the constructor's edge-case behavior so a future change
    /// (special-casing empty bytes for perf, say) is a visible
    /// hard-fork event.
    #[test]
    fn payload_ref_hash_on_empty_input_pins_blake2b256_of_empty() {
        let PayloadRef::Hash(bytes) = PayloadRef::hash(&[]) else {
            unreachable!()
        };
        let expected = Blake2b256::hash(Vec::new());
        assert_eq!(bytes.as_slice(), expected.as_slice());
    }

    /// `WalMark::len` and `WalMark::is_empty` round-trip.  Small
    /// but load-bearing: subsequent-slice drain logic keys off
    /// these accessors.
    #[test]
    fn wal_mark_accessors_roundtrip() {
        let mark = WalMark { len: 0 };
        assert_eq!(mark.len(), 0);
        assert!(mark.is_empty());

        let mark = WalMark { len: 7 };
        assert_eq!(mark.len(), 7);
        assert!(!mark.is_empty());
    }

    /// Smoke test on `WalEntry` construction + trait derivations
    /// (Debug / Clone / PartialEq).  A refactor that dropped a
    /// derive would break downstream tests silently otherwise.
    #[test]
    fn wal_entry_construction_smoke() {
        let entry = WalEntry {
            op: WalOp::Write,
            path: PathBuf::from("/tmp/f.txt"),
            extra_path: None,
            offset: None,
            length: Some(5),
            payload_ref: Some(PayloadRef::hash(b"hello")),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        };
        // Trigger Debug.
        let _ = format!("{entry:?}");
        // Trigger Clone + PartialEq.
        assert_eq!(entry, entry.clone());
    }
}
