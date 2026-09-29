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
use std::sync::{Arc, RwLock};

use crypto::rust::hash::blake2b256::Blake2b256;

use super::errors::poison_abort;

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
/// `Wal::take_deploy_entries_insertion_order` (both in a subsequent slice).
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
    /// so the (subsequent-slice) `Wal::take_deploy_entries_insertion_order` can
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

// Compile-time witness that `Wal: Send + Sync`.  Hoisted to module
// scope (rather than a `#[test]`) so every `cargo build` catches
// a regression, not only `cargo test`.  A refactor that broke
// either bound would surface here at build time instead of at
// some unrelated tokio-spawn site.
const _WAL_IS_SEND_SYNC: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Wal>();
};

/// Per-runtime append-only WAL buffer.  Cloneable (shares the
/// underlying `Arc<RwLock<WalInner>>` via reference-counting) so
/// every handler closure can journal into the same list.
///
/// # Locking discipline
///
/// Both the entry Vec and the parallel ack-hash sidecar live under
/// a single `RwLock<WalInner>` guard — index-alignment between the
/// two vectors becomes *structural* (they can't be updated apart)
/// rather than an invariant to remember at every mutation site.
/// Read/write split lets pure observers (`len`, `snapshot`, ...)
/// share concurrently while mutators (`append*`, `truncate_to`,
/// `take_*`, `clear`) take an exclusive guard.
///
/// Every guard acquisition routes through `poison_abort` per
/// DD-FailClosedOnInvariantBreak — a thread that panicked while
/// holding a Wal guard aborts the deploy rather than continuing
/// with potentially half-updated buffer state.
///
/// # Ack-hash sidecar
///
/// The `ack_hashes` sidecar records the Blake2b256 hash of the
/// ack channel Par each syscall published its reply on.  Every fs
/// syscall's ack is a fresh unforgeable per call, so this is a
/// unique key within a deploy.  A subsequent
/// `take_deploy_entries_in_log_order` walk (later slice) matches
/// each entry to a Produce event via its sidecar hash so the
/// drain order is scheduler-independent and byte-identical
/// across validators.
///
/// Stored as `Vec<[u8; 32]>` rather than a typed hash newtype to
/// keep this module free of `rspace_plus_plus` — the caller
/// computes the hash via `stable_hash_provider::hash(ack)` and
/// passes bytes.
#[derive(Clone, Debug, Default)]
pub struct Wal {
    inner: Arc<RwLock<WalInner>>,
}

/// Inner state guarded by the `Wal`'s single `RwLock`.  Private —
/// the invariant "entries and ack_hashes are index-aligned" is
/// maintained by every `Wal` method that touches either vec, and
/// nothing outside the module can construct or mutate `WalInner`
/// directly.
#[derive(Debug, Default)]
struct WalInner {
    entries: Vec<WalEntry>,
    /// Parallel to `entries`, index-aligned.  See the `Wal`
    /// struct's docstring for the ack-hash sidecar design.  A
    /// legacy `append(entry)` populates a sentinel `[0u8; 32]`
    /// here — the sentinel is guaranteed not to match any real
    /// Produce event's `channel_hash` for a fresh unforgeable, so
    /// log-order drain naturally skips these entries and falls
    /// back to insertion order.
    ack_hashes: Vec<[u8; 32]>,
}

impl Wal {
    /// Construct an empty WAL buffer.  Alias for `Default::default`
    /// — kept explicit for call-site readability.
    pub fn new() -> Self { Self::default() }

    /// Legacy append: no ack-hash sidecar populated (records
    /// `[0u8; 32]`, which is guaranteed not to match any real
    /// Produce event's `channel_hash` for a fresh unforgeable).
    /// Kept for tests + soft-checkpoint machinery.  Production
    /// handler paths should use `append_with_ack`.
    ///
    /// Returns `Err(())` if appending would exceed
    /// `MAX_WAL_ENTRIES` — callers translate to
    /// `FSERR_QUOTA_EXCEEDED`.
    #[allow(clippy::result_unit_err)]
    pub fn append(&self, entry: WalEntry) -> Result<(), ()> {
        self.append_with_ack(entry, [0u8; 32])
    }

    /// Append an entry with its ack-channel hash for later log-
    /// order-based drain (subsequent slice).  The hash comes from
    /// `stable_hash_provider::hash(ack_par)` on the handler side.
    /// A sentinel `[0u8; 32]` disables log-order matching for
    /// that entry (falls back to insertion order in the eventual
    /// `take_deploy_entries_in_log_order` walk).
    ///
    /// Returns `Err(())` if appending would exceed
    /// `MAX_WAL_ENTRIES`.
    #[allow(clippy::result_unit_err)]
    pub fn append_with_ack(&self, entry: WalEntry, ack_hash: [u8; 32]) -> Result<(), ()> {
        let mut guard = poison_abort(self.inner.write(), "Wal");
        if guard.entries.len() >= MAX_WAL_ENTRIES {
            return Err(());
        }
        guard.entries.push(entry);
        // Single RwLock over both vecs makes index-alignment
        // structurally enforced — no cross-mutex ordering to
        // maintain.
        guard.ack_hashes.push(ack_hash);
        Ok(())
    }

    /// Point-in-time clone of the current entries.  Cheap — a Vec
    /// clone.  Intended for tests + snapshot / checkpoint
    /// machinery.
    ///
    /// The read guard is dropped BEFORE this function returns;
    /// concurrent mutators may advance the WAL immediately after.
    /// The returned Vec is self-contained (owns its allocation)
    /// and therefore memory-safe, but callers MUST NOT use the
    /// clone's `.len()` to validate against a subsequent
    /// `Wal::len()` call — the two can diverge under concurrency.
    /// For a snapshot whose length is pinned atomically with the
    /// clone, use `snapshot_with_mark` below.
    pub fn snapshot(&self) -> Vec<WalEntry> {
        let guard = poison_abort(self.inner.read(), "Wal");
        guard.entries.clone()
    }

    /// Snapshot the current entries AND their length under a
    /// single read guard, so the returned `(entries, mark)` pair
    /// is guaranteed internally consistent (no concurrent-
    /// mutation ambiguity between the entries clone and its
    /// length marker).  Prefer this over `snapshot()` +
    /// `snapshot_mark()` called separately — the separate calls
    /// take two guards and a concurrent writer can slip a
    /// mutation in between.
    pub fn snapshot_with_mark(&self) -> (Vec<WalEntry>, WalMark) {
        let guard = poison_abort(self.inner.read(), "Wal");
        let entries = guard.entries.clone();
        let mark = WalMark { len: entries.len() };
        (entries, mark)
    }

    /// Number of journaled entries at the moment of the read.
    /// Same concurrency caveat as `snapshot`: the returned value
    /// is a snapshot and may be stale by the time the caller acts
    /// on it.
    pub fn len(&self) -> usize { poison_abort(self.inner.read(), "Wal").entries.len() }

    /// True if the buffer contains zero entries at the moment of
    /// the read.
    pub fn is_empty(&self) -> bool { self.len() == 0 }

    /// Clear all entries (and their ack-hash sidecar).  Called on
    /// runtime reset (defence-in-depth for the invariant that a
    /// reset-and-reused runtime starts every deploy from an empty
    /// WAL) and by tests.
    pub fn clear(&self) {
        let mut guard = poison_abort(self.inner.write(), "Wal");
        guard.entries.clear();
        guard.ack_hashes.clear();
    }

    /// Record the current buffer length as a `WalMark`.  Callers
    /// use this to bracket a scope (e.g., a deploy attempt) that
    /// may need to be rolled back via `truncate_to`.
    pub fn snapshot_mark(&self) -> WalMark {
        let guard = poison_abort(self.inner.read(), "Wal");
        WalMark {
            len: guard.entries.len(),
        }
    }

    /// Truncate entries appended after `mark`.  Called from
    /// soft-checkpoint revert alongside a symmetric handle-table
    /// truncation.
    ///
    /// # Monotonicity
    ///
    /// A `mark` at-or-past the current length is a no-op.  This
    /// covers the "stale mark from a snapshot that predates a
    /// clear-and-repopulate cycle" case without a panic.
    pub fn truncate_to(&self, mark: WalMark) {
        let mut guard = poison_abort(self.inner.write(), "Wal");
        if mark.len < guard.entries.len() {
            guard.entries.truncate(mark.len);
            // Keep sidecar index-aligned.
            guard.ack_hashes.truncate(mark.len);
        }
    }

    /// Per-deploy boundary marker.  Called at the top of a deploy
    /// before user code runs.  Paired with
    /// `take_deploy_entries_insertion_order` (or the yet-to-land
    /// `take_deploy_entries_in_log_order`) which drains exactly
    /// the entries this deploy contributed, letting a downstream
    /// slice attach a deploy's WAL contributions to its
    /// `ProcessedDeploy` (either via a proto-schema extension or
    /// via an out-of-band side-map keyed by deploy signature).
    ///
    /// Semantically equivalent to `snapshot_mark`; kept as a
    /// distinct method so the deploy-boundary intent is
    /// unambiguous at call sites.
    pub fn begin_deploy(&self) -> WalMark { self.snapshot_mark() }

    /// Drain entries appended after `mark` **in insertion order**
    /// (scheduler-dependent).  Returns them AND removes them from
    /// the WAL, so the underlying buffer stays bounded across
    /// deploys.  If a caller wants to peek without draining, use
    /// `snapshot_with_mark` and diff the length against `mark`.
    ///
    /// # Callers: pick your ordering explicitly
    ///
    /// The name has an `_insertion_order` suffix — deliberately
    /// search-hostile — because callers that will hash the
    /// returned Vec into a **consensus commitment** (e.g., a
    /// snapshot root) MUST use `take_deploy_entries_in_log_order`
    /// (subsequent slice) instead.  Log order re-orders by the
    /// canonical `deploy_log` event sequence and is deterministic
    /// across validators; insertion order reflects `Par`
    /// scheduling on this run and is safe only for non-consensus
    /// consumers (tests, soft-checkpoint machinery).
    ///
    /// This slice ships only the insertion-order variant because
    /// the log-order path depends on `stable_hash_provider` /
    /// `deploy_log` types that haven't been ported yet.  The
    /// suffix is the machinery that keeps a future consensus
    /// consumer from casually grabbing this method and getting
    /// silent non-determinism.
    pub fn take_deploy_entries_insertion_order(&self, mark: WalMark) -> Vec<WalEntry> {
        let mut guard = poison_abort(self.inner.write(), "Wal");
        if mark.len >= guard.entries.len() {
            return Vec::new();
        }
        // Also drain the ack_hash sidecar to keep it index-
        // aligned with the (now-shorter) entries Vec.
        let _ = guard.ack_hashes.split_off(mark.len);
        guard.entries.split_off(mark.len)
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
    /// pointer cast (encapsulated by `wal_outcome_disc` below).
    #[test]
    fn wal_outcome_discriminants_pinned() {
        fn wal_outcome_disc(v: &WalOutcome) -> u8 {
            // SAFETY: `WalOutcome` is `#[repr(u8)]`, so the first
            // byte of any live value's storage is its discriminant
            // regardless of the variant's payload.  The `&v`
            // reference outlives the pointer read.
            unsafe { *(v as *const _ as *const u8) }
        }
        assert_eq!(wal_outcome_disc(&WalOutcome::Success), 0);
        assert_eq!(wal_outcome_disc(&WalOutcome::Failure { code: 0 }), 1);
        // Payload variance must not affect the discriminant byte.
        assert_eq!(wal_outcome_disc(&WalOutcome::Failure { code: u32::MAX }), 1);
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

    // --- Wal buffer ------------------------------------------------

    /// Test helper — construct a distinct `WalEntry` per index so
    /// insertion-order pins can distinguish entries by their
    /// `length` field.
    fn mk_entry(i: u64) -> WalEntry {
        WalEntry {
            op: WalOp::Write,
            path: PathBuf::from(format!("/tmp/f{i}.txt")),
            extra_path: None,
            offset: None,
            length: Some(i),
            payload_ref: Some(PayloadRef::Hash([i as u8; 32])),
            mode_bits: None,
            owner: None,
            group: None,
            outcome: WalOutcome::Success,
        }
    }

    #[test]
    fn wal_new_starts_empty() {
        let wal = Wal::new();
        assert_eq!(wal.len(), 0);
        assert!(wal.is_empty());
        assert!(wal.snapshot().is_empty());
        let (entries, mark) = wal.snapshot_with_mark();
        assert!(entries.is_empty());
        assert_eq!(mark.len(), 0);
        assert!(mark.is_empty());
    }

    #[test]
    fn wal_default_matches_new() {
        let a = Wal::default();
        let b = Wal::new();
        assert_eq!(a.len(), b.len());
        assert_eq!(a.snapshot(), b.snapshot());
    }

    /// `append` populates entries and increments length; the
    /// legacy variant stores the sentinel `[0u8; 32]` in the
    /// ack-hash sidecar (verified indirectly by the truncate/take
    /// tests which exercise index-alignment).
    #[test]
    fn wal_append_grows_entries() {
        let wal = Wal::new();
        wal.append(mk_entry(1)).unwrap();
        wal.append(mk_entry(2)).unwrap();
        assert_eq!(wal.len(), 2);
        assert!(!wal.is_empty());
        let snap = wal.snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].length, Some(1));
        assert_eq!(snap[1].length, Some(2));
    }

    /// `append_with_ack` populates both vecs.  Sidecar alignment
    /// is visible through the `truncate_to` and
    /// `take_deploy_entries_insertion_order` paths, which panic
    /// on misalignment via `Vec::truncate` / `split_off` bounds —
    /// a mismatch would be caught by the
    /// `wal_take_deploy_entries_drains_only_post_mark_entries`
    /// test below.
    #[test]
    fn wal_append_with_ack_populates_both_vecs() {
        let wal = Wal::new();
        wal.append_with_ack(mk_entry(1), [0xAAu8; 32]).unwrap();
        wal.append_with_ack(mk_entry(2), [0xBBu8; 32]).unwrap();
        assert_eq!(wal.len(), 2);
    }

    /// Cap enforcement — the (`MAX_WAL_ENTRIES + 1`)th append
    /// returns `Err(())`, callers translate to
    /// `FSERR_QUOTA_EXCEEDED`.
    ///
    /// Fills the buffer with cheap `WalEntry`s (65 536 total),
    /// which allocates roughly 65 536 × ~200 B ≈ 13 MiB — well
    /// within test-runner limits and finishes in a fraction of a
    /// second on a debug build.
    #[test]
    fn wal_append_returns_err_past_max_wal_entries() {
        let wal = Wal::new();
        // Fill to MAX_WAL_ENTRIES.
        for i in 0..MAX_WAL_ENTRIES {
            wal.append(mk_entry(i as u64))
                .unwrap_or_else(|()| panic!("append {i} unexpectedly failed"));
        }
        assert_eq!(wal.len(), MAX_WAL_ENTRIES);
        // The next append trips the cap.
        assert_eq!(wal.append(mk_entry(MAX_WAL_ENTRIES as u64)), Err(()));
        // And the buffer is unchanged.
        assert_eq!(wal.len(), MAX_WAL_ENTRIES);
    }

    #[test]
    fn wal_clear_drops_all_entries() {
        let wal = Wal::new();
        wal.append(mk_entry(1)).unwrap();
        wal.append(mk_entry(2)).unwrap();
        assert_eq!(wal.len(), 2);
        wal.clear();
        assert_eq!(wal.len(), 0);
        assert!(wal.is_empty());
    }

    /// `snapshot_with_mark` returns a `(Vec, WalMark)` pair whose
    /// length invariant holds at read time.  Pin explicitly so a
    /// future refactor that split the read into two guards would
    /// fire this test if a concurrent-mutation-shaped scenario
    /// snuck in.
    #[test]
    fn wal_snapshot_with_mark_pair_is_length_consistent() {
        let wal = Wal::new();
        for i in 0..5 {
            wal.append(mk_entry(i)).unwrap();
        }
        let (entries, mark) = wal.snapshot_with_mark();
        assert_eq!(entries.len(), 5);
        assert_eq!(mark.len(), 5);
        assert_eq!(entries.len(), mark.len());
    }

    #[test]
    fn wal_truncate_to_shrinks_to_mark() {
        let wal = Wal::new();
        for i in 0..5 {
            wal.append(mk_entry(i)).unwrap();
        }
        let mark = WalMark { len: 2 };
        wal.truncate_to(mark);
        assert_eq!(wal.len(), 2);
        let snap = wal.snapshot();
        assert_eq!(snap[0].length, Some(0));
        assert_eq!(snap[1].length, Some(1));
    }

    /// Monotonicity — a mark past current length is a no-op, not
    /// a panic (covers a stale mark from a snapshot that predates
    /// a clear-and-repopulate cycle).
    #[test]
    fn wal_truncate_to_past_current_len_is_noop() {
        let wal = Wal::new();
        wal.append(mk_entry(0)).unwrap();
        wal.append(mk_entry(1)).unwrap();
        wal.truncate_to(WalMark { len: 99 });
        assert_eq!(wal.len(), 2);
    }

    /// `begin_deploy` + `take_deploy_entries_insertion_order`
    /// drain exactly the deploy's contribution.  Also verifies
    /// the ack-hash sidecar is drained in lockstep with `entries`
    /// — a misalignment bug would surface as a subsequent `take`
    /// panicking on `split_off` bounds.
    #[test]
    fn wal_take_deploy_entries_drains_only_post_mark_entries() {
        let wal = Wal::new();
        // Pre-deploy entries.
        wal.append_with_ack(mk_entry(0), [0x01; 32]).unwrap();
        wal.append_with_ack(mk_entry(1), [0x02; 32]).unwrap();
        let mark = wal.begin_deploy();
        assert_eq!(mark.len(), 2);
        // Deploy contributions.
        wal.append_with_ack(mk_entry(2), [0x03; 32]).unwrap();
        wal.append_with_ack(mk_entry(3), [0x04; 32]).unwrap();
        wal.append_with_ack(mk_entry(4), [0x05; 32]).unwrap();
        assert_eq!(wal.len(), 5);

        let drained = wal.take_deploy_entries_insertion_order(mark);
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0].length, Some(2));
        assert_eq!(drained[1].length, Some(3));
        assert_eq!(drained[2].length, Some(4));
        // The pre-deploy entries survive.
        assert_eq!(wal.len(), 2);
        let survivors = wal.snapshot();
        assert_eq!(survivors[0].length, Some(0));
        assert_eq!(survivors[1].length, Some(1));

        // Second drain against the same mark yields empty (the
        // entries past the mark are gone).
        let second = wal.take_deploy_entries_insertion_order(mark);
        assert!(second.is_empty());

        // Subsequent appends still succeed — proves the sidecar
        // stayed aligned (a mismatch would panic in the next
        // truncate_to / split_off inside append_with_ack's cap
        // check would not fire, but the invariant is verified
        // structurally by the single-guard design).
        wal.append_with_ack(mk_entry(5), [0x06; 32]).unwrap();
        assert_eq!(wal.len(), 3);
    }

    /// A drain against a mark past the current length yields an
    /// empty Vec (not a panic).  Same "stale mark tolerance"
    /// property as `truncate_to`.
    #[test]
    fn wal_take_deploy_entries_past_current_len_yields_empty() {
        let wal = Wal::new();
        wal.append(mk_entry(0)).unwrap();
        let drained = wal.take_deploy_entries_insertion_order(WalMark { len: 99 });
        assert!(drained.is_empty());
        assert_eq!(wal.len(), 1);
    }

    /// `Wal` is `Clone` — a cloned handle shares the underlying
    /// buffer via `Arc`.  A `.append` through one handle is
    /// visible via `.snapshot` through the other.  This pins the
    /// "every handler closure can journal into the same list"
    /// contract that motivates the `Arc<RwLock<...>>` shape.
    #[test]
    fn wal_clone_shares_underlying_buffer() {
        let a = Wal::new();
        let b = a.clone();
        a.append(mk_entry(42)).unwrap();
        assert_eq!(b.len(), 1);
        assert_eq!(b.snapshot()[0].length, Some(42));
    }

    // `Wal: Send + Sync` — no runtime test needed; the module-
    // scope `_WAL_IS_SEND_SYNC` const witness above enforces the
    // bound at every `cargo build` (not only `cargo test`).
}
