// Range-lock module — data types (slice 1, prior PR) + state
// containers + registry skeleton (slice 2, this PR of the `lock`
// submodule tree).
//
// Prior slice (earlier Wave 2) provided the pure-data foundations:
//   - `DevInode` / `DeployScope` — identity + ownership tags.
//   - `LockId` / `HolderId` — newtype identifiers, with
//     `HolderId::ct_eq` for the constant-time release-path check.
//   - `LockMode` — read/write.
//   - `MAX_RANGES_PER_FILE` / `MAX_WAITERS_PER_FILE` /
//     `LOCK_ID_CEILING` — consensus-observable caps.
//   - `LockError` — the `FSERR_*` error taxonomy.
//   - `RangeEntry` — one granted range-lock record.
//
// This slice adds the state-container surface the acquire/release
// methods (yet-to-land) manipulate:
//   - `FileLockState { ranges, sequential_holder, waiters }` —
//     per-`(dev, inode)` state.
//   - `SequentialEntry` — a whole-file sequential lock record.
//   - `WaitPolicy` — Fail (MVP) vs. Wait (slice 8b opt-in).
//   - `AcquireOutcome { Immediate, Parked { lock_id, admit } }` —
//     the shape of the acquire-side return.
//   - `Waiter` (private) + `WaitKind` (private) — one parked
//     waiter entry.
//   - `LockRegistry { inner, next_lock_id }` + `new()` +
//     `mint_next_lock_id` — the empty skeleton.
//
// Acquire / release / wake / deadlock-detection logic (`range_
// conflicts` / `wake_waiters` / `would_close_cycle` / etc.) land
// in subsequent slices.  The data surface here lets a future
// slice land the acquire/release methods without re-touching the
// enum shapes or the struct fields.
//
// # Consensus surface
//
// Three constants register into `CONSENSUS_FOLD`:
//   - `MAX_RANGES_PER_FILE` (order 11) — per-`(dev, inode)` cap on
//     concurrent range locks; a divergent value fires
//     `FSERR_QUOTA_EXCEEDED` on different inputs across peers and
//     forks the tuplespace.
//   - `MAX_WAITERS_PER_FILE` (order 12) — per-`(dev, inode)` cap
//     on parked `wait: true` acquirers.  Same failure-mode
//     rationale.
//   - `LOCK_ID_CEILING` (order 13) — guard threshold on the LockId
//     monotone counter (wraps would collide new LockIds with
//     stale ones still in RSpace).

/// Filesystem identity — `(st_dev, st_ino)` from `fstat(2)`.
/// Keying on this collapses hard-linked aliases, bind-mount
/// duplicates, and symlink chains to a single lock entry.
///
/// # Follow-up: newtype
///
/// Currently a type alias — passing a random `(u64, u64)` where a
/// `DevInode` is expected compiles without complaint.  A later
/// slice that centralises `fstat` interpretation should promote
/// this to a proper `pub struct DevInode { dev: u64, inode: u64 }`
/// newtype (matching the `Fd` / `LockId` discipline).  Left as an
/// alias here so this slice remains a pure data-types PR without
/// churning downstream call-site ergonomics.
pub type DevInode = (u64, u64);

/// Opaque tag identifying which deploy owns this lock, for
/// `release_all_for_deploy` auto-release at deploy-end.
/// Concretely, whatever key the (not-yet-landed) `WalDeployScope`
/// already uses — typically the deploy hash.
///
/// # Follow-up: newtype
///
/// Also an alias for the same "keep this slice pure data-types"
/// reason as `DevInode`.  A future slice that adds semantics
/// (deploy-scope hashing / display) should promote to a newtype.
pub type DeployScope = [u8; 32];

/// Opaque per-runtime handle returned by `try_acquire` and passed
/// back to `release`.  Also carried inside the Rholang-side
/// `LockToken` agent's `stateP`.
///
/// # Consensus surface
///
/// Individual `LockId` *values* are NOT compared across peers —
/// the rig-protocol layer above (a subsequent slice) ensures
/// deterministic acquire/release *outcomes* from a byte-identical
/// `LockRegistry`, but the numeric id a given acquire returns is
/// per-runtime.
///
/// However, a `LockId` DOES travel through Rholang state (embedded
/// in a `LockToken` agent's `stateP` for later `release(@lockId)`
/// calls), which means it round-trips through the tuplespace as a
/// Rholang integer — currently `i64` at the interpreter layer.  A
/// raw `u64` value above `i64::MAX` would truncate on emission
/// and mangle on extraction, corrupting the release-path lookup
/// for a single validator (not a consensus fork, but a functional
/// bug).
///
/// # Newtype discipline (mirrors `Fd`)
///
/// The field is private (`u64` wrapped in `#[repr(transparent)]`)
/// to match `Fd`'s discipline.  Construction is **fallible** via
/// `TryFrom<u64>` — raw values above `i64::MAX` are rejected with
/// `LockIdOutOfRange` so the wire-safety invariant is compiler-
/// checked at every call site.  The `LOCK_ID_CEILING` consensus
/// constant is set to `i64::MAX - 2^16` so `LockRegistry`'s
/// (subsequent-slice) allocator never mints an out-of-range id in
/// the first place; the fallible constructor is the belt for the
/// suspenders.  Unwrap via `.as_u64()` at the reply-emission
/// boundary — the value is guaranteed to fit in `i64` by
/// construction, so downstream `as i64` casts are lossless.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LockId(u64);

/// Returned by `LockId::try_from(u64)` when the raw value is
/// outside the wire-safe range `[0, i64::MAX]`.  Analogous to
/// `response::FdOutOfRange` on `Fd`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockIdOutOfRange {
    pub attempted: u64,
}

impl LockId {
    /// Unwrap into the raw `u64` for handoff to the Rholang-side
    /// reply / release path.  Guaranteed by construction to fit
    /// in `i64` (see the struct-level docstring), so a downstream
    /// `as i64` cast is lossless.
    #[inline]
    pub fn as_u64(self) -> u64 { self.0 }
}

impl TryFrom<u64> for LockId {
    type Error = LockIdOutOfRange;

    /// Lift a raw `u64` (typically extracted from a Rholang
    /// `release(@lockId)` call) into a `LockId`.  Rejects values
    /// above `i64::MAX` — those cannot round-trip through Rholang's
    /// `i64` integer type without mangling the release-path
    /// lookup.  `LockRegistry`'s (subsequent-slice) allocator
    /// stays below `LOCK_ID_CEILING` (< `i64::MAX`), so a well-
    /// behaved runtime never mints values that would fail this
    /// check; the reject arm exists to defend against a Rholang-
    /// side caller passing a hand-crafted out-of-range value.
    #[inline]
    fn try_from(raw: u64) -> Result<Self, Self::Error> {
        if raw > i64::MAX as u64 {
            Err(LockIdOutOfRange { attempted: raw })
        } else {
            Ok(LockId(raw))
        }
    }
}

// Compile-time layout witness — mirrors `Fd`'s
// `_FD_LAYOUT_MATCHES_U64`.  A drop of `#[repr(transparent)]` or
// an added field would fail this static check at build time
// instead of runtime.
#[allow(dead_code)]
const _LOCKID_LAYOUT_MATCHES_U64: fn() = || {
    // SAFETY: `LockId` is `#[repr(transparent)]` around `u64`, which
    // guarantees identical size and alignment.  The closure body
    // is type-checked (never executed), which forces
    // `std::mem::transmute`'s compile-time size-equality
    // constraint to fire — a layout drift on `LockId` would fail
    // this static check.
    let _: u64 = unsafe { std::mem::transmute::<LockId, u64>(LockId(0)) };
    // SAFETY: same as above, other direction; the resulting
    // `LockId` value is not observed at runtime (the closure never
    // runs).
    let _: LockId = unsafe { std::mem::transmute::<u64, LockId>(0) };
};

/// Cap-scoped identity for `release_all_for_holder` cleanup on
/// `File.close`.  Derived from the File agent's per-instance
/// `this` GPrivate name at cap-mint time — unique per fresh-mint
/// open.
///
/// # Threat model — HolderId unforgeability
///
/// `HolderId = Blake2b256(*this bytes)`.  The release-time
/// identity check (in `LockRegistry::release`, subsequent slice)
/// uses this hash to enforce cross-cap release refusal.  Its
/// security rests on TWO separate assumptions:
///
/// 1. **GPrivate opacity** — `*this` bytes are not directly
///    observable to a Rholang deploy that does NOT hold the cap.
///    Rholang's GPrivate names are minted with random 32-byte
///    payloads via `Blake2b512Random::next_bytes` inside the `new`
///    scope's desugaring; a deploy without the bundled dispatch
///    channel cannot enumerate or forge them.  This is EMPIRICAL,
///    not formally proven — if a future Rholang feature exposed
///    reflection over GPrivate names (e.g., a `serialize_name`
///    primitive, or a debug-mode escape hatch), the release-time
///    identity check would collapse to zero-security.  Any such
///    feature MUST audit this pathway before landing.
///
/// 2. **Blake2b256 collision resistance** — two distinct `*this`
///    inputs must produce distinct `HolderId` outputs.  Blake2b256's
///    collision resistance is 2^-256 per pair, so a random
///    collision has astronomically low probability.  BUT: HolderId
///    uniqueness is a *cryptographic* assumption, not a
///    *structural* guarantee.  Callers that rely on HolderId as a
///    unique cap identifier (e.g., a hypothetical future "holder-
///    scoped audit log") should read this note before depending on
///    100%-unique-per-cap semantics.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HolderId {
    /// Private to match `LockId` / `Fd` newtype discipline —
    /// callers read via `bytes()`, construct via `from_bytes()`.
    /// Prevents a caller from silently swapping the derivation
    /// (e.g., using raw `*this` bytes instead of
    /// `Blake2b256(*this)`) without touching the constructor
    /// call site.
    bytes: [u8; 32],
}

impl HolderId {
    /// Construct from raw bytes.  The bytes should be
    /// `Blake2b256(*this)` per the threat model above; this
    /// constructor does not enforce the derivation (callers are
    /// responsible for the hash step).
    pub fn from_bytes(bytes: [u8; 32]) -> Self { Self { bytes } }

    /// Read-only accessor for the 32-byte identity.  Callers that
    /// need to serialise or hash the id go through this; anyone
    /// comparing two `HolderId`s for authentication purposes MUST
    /// use `ct_eq` instead of `==` on the returned slice (short-
    /// circuit compare is the exact side-channel `ct_eq` defeats).
    #[inline]
    pub fn bytes(&self) -> &[u8; 32] { &self.bytes }

    /// Constant-time byte comparison for holder verification in
    /// the release path.  Defense-in-depth against a timing side-
    /// channel where an attacker observing response latency on
    /// failed `release()` calls could probabilistically narrow
    /// down a holder byte-by-byte if the underlying compare short-
    /// circuited on first mismatch.
    ///
    /// The threat is low in practice (sub-microsecond timing delta
    /// per byte, dwarfed by network jitter; consensus-managed FS
    /// assumes trusted operator network) but the constant-time
    /// compare costs nothing at 32 bytes and eliminates the side-
    /// channel entirely.  Reads all 32 bytes regardless of
    /// mismatch position via bit-XOR accumulation.
    ///
    /// Prefer this over derived `PartialEq` for any
    /// authentication-adjacent comparison.  A grep pin in the
    /// (subsequent-slice) release path will enforce the
    /// discipline; a companion test-file pin lives here below.
    ///
    /// # Future hardening
    ///
    /// If future compiler versions vectorize the loop with SIMD
    /// or reorder loads in ways that erode the constant-time
    /// guarantee, promote to `subtle::ConstantTimeEq` — the
    /// industry-standard hardening (uses `#[inline(never)]` +
    /// `black_box` tricks to defeat optimizer regressions).  Not
    /// adopted today because (a) the manual `while`-loop shape is
    /// source-level auditable, (b) `subtle` adds a workspace dep
    /// for a threat the docstring already frames as low-in-
    /// practice, (c) if we adopt it, the source-scan grep pin (on
    /// the release path, subsequent slice) should also flag
    /// direct `==` on `HolderId` to keep the migration honest.
    #[inline]
    pub fn ct_eq(&self, other: &HolderId) -> bool {
        let mut acc: u8 = 0;
        // Manual `while` loop rather than `.zip().fold()` to make
        // the constant-time property source-level obvious (no
        // early exit, no iterator adapter that a future compiler
        // pass might inline into a short-circuit).
        let mut i = 0;
        while i < 32 {
            acc |= self.bytes[i] ^ other.bytes[i];
            i += 1;
        }
        acc == 0
    }
}

/// Requested access mode.  Multiple readers of overlapping ranges
/// coexist; a writer conflicts with any overlapping reader OR
/// writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
    Read,
    Write,
}

/// Per-`(dev, inode)` cap on concurrent range locks.  Bounds the
/// per-file lock-table growth against pathological workloads (many
/// disjoint tiny locks on one file).  Matches `MAX_OPEN_FDS`'s
/// scale so a runtime's aggregate lock count is bounded by
/// `MAX_OPEN_FDS * MAX_RANGES_PER_FILE`.  A hostile deploy hitting
/// this cap gets `FSERR_QUOTA_EXCEEDED` at the native boundary and
/// cannot amplify further.
///
/// # CONSENSUS-OBSERVABLE
///
/// A divergent cap produces different `FSERR_QUOTA_EXCEEDED`
/// distributions on identical inputs and forks the tuplespace.
/// Folded into the runtime fingerprint at order 11.
pub const MAX_RANGES_PER_FILE: usize = 1024;

// Compile-time floor: below 256 the cap would fire on legitimate
// concurrent-range workloads (small file-server patterns
// commonly hold ~100 range locks).
const _: () = assert!(
    MAX_RANGES_PER_FILE >= 256,
    "MAX_RANGES_PER_FILE below 256 would fire FSERR_QUOTA_EXCEEDED \
     on legitimate concurrent-range workloads and fork consensus"
);

crate::register_consensus_constant!(order = 11, name = MAX_RANGES_PER_FILE, u64_be);

/// Per-`(dev, inode)` cap on parked `wait: true` acquirers.
/// Symmetric with `MAX_RANGES_PER_FILE` — bounds the waiter deque
/// against a hostile deploy that spams `try_acquire_range_wait(...,
/// WaitPolicy::Wait)` on a locked file.  Each waiter allocates
/// ~150 bytes, so at saturation with `MAX_OPEN_FDS *
/// MAX_WAITERS_PER_FILE` the runtime-wide waiter memory tops out
/// at ~150 MiB.
///
/// A hostile deploy hitting this cap gets `FSERR_QUOTA_EXCEEDED`
/// at the native boundary — same code as the live-range cap so
/// callers do not need to differentiate.  Defense-in-depth
/// against future cost-tuning changes that might inadvertently
/// lower per-call cost enough to allow massive waiter allocations.
///
/// # CONSENSUS-OBSERVABLE
///
/// Same rationale as `MAX_RANGES_PER_FILE`.  Folded at order 12.
pub const MAX_WAITERS_PER_FILE: usize = 1024;

// Compile-time floor: same 256 rationale as MAX_RANGES_PER_FILE.
const _: () = assert!(
    MAX_WAITERS_PER_FILE >= 256,
    "MAX_WAITERS_PER_FILE below 256 would fire FSERR_QUOTA_EXCEEDED \
     on legitimate waiter-queue workloads and fork consensus"
);

crate::register_consensus_constant!(order = 12, name = MAX_WAITERS_PER_FILE, u64_be);

/// Guard threshold on the `LockId` monotone counter.  With `i64`
/// headroom the wrap is still astronomical (~292 years at 10^9
/// acquires/sec), but a wrap would collide new `LockId`s with
/// stale `LockToken`s still in RSpace — allowing a spurious
/// release-after-release.  Refusing acquisitions past
/// `LOCK_ID_CEILING` closes that vector at negligible cost.
///
/// Set to `i64::MAX - 2^16` (not `u64::MAX - 2^16`) so the
/// allocator produces values that always satisfy `LockId`'s
/// `TryFrom<u64>` wire-safety check.  See the struct-level
/// `LockId` docstring for the Rholang `i64` round-trip rationale.
/// The `- 2^16` margin gives ~65k acquires of hard-failure warning
/// before the ceiling is hit in practice.
///
/// # CONSENSUS-OBSERVABLE
///
/// A divergent ceiling would surface `FSERR_QUOTA_EXCEEDED` at
/// different counter positions across peers.  Folded at order 13.
pub const LOCK_ID_CEILING: u64 = (i64::MAX as u64) - (1 << 16);

// Compile-time invariant: the ceiling MUST fit in i64 so
// `LockRegistry`'s allocator (subsequent slice) never mints a
// `LockId` that would fail `TryFrom<u64>`'s wire-safety check.
const _: () = assert!(
    LOCK_ID_CEILING <= i64::MAX as u64,
    "LOCK_ID_CEILING above i64::MAX would let the allocator mint \
     LockIds that fail TryFrom<u64>'s wire-safety check"
);

// Compile-time floor: below 2^60 the ceiling would fire on
// realistic (though astronomical) acquire rates.
const _: () = assert!(
    LOCK_ID_CEILING >= 1u64 << 60,
    "LOCK_ID_CEILING below 2^60 leaves too little headroom on the \
     monotone counter — a lower ceiling risks premature \
     FSERR_QUOTA_EXCEEDED at production acquire rates"
);

crate::register_consensus_constant!(order = 13, name = LOCK_ID_CEILING, u64_be);

/// Errors surfaced through the native handlers as `FSERR_*` codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockError {
    /// Requested range conflicts with an existing holder.  Maps to
    /// `FSERR_BUSY` at the native boundary.
    Busy,
    /// `release` called with a `LockId` that isn't held (double
    /// release, release after `File.close` swept it, wait:true
    /// cancellation resolved first, etc.).  Maps to `FSERR_CLOSED`.
    Closed,
    /// Zero-length range or other malformed input.  Maps to
    /// `FSERR_BAD_ARG` at the native boundary.  A zero-length
    /// "lock" would protect nothing and never conflict with
    /// anything, so silently accepting it invites subtle race
    /// bugs — reject at the boundary instead.
    BadArg,
    /// Per-`(dev, inode)` range cap reached
    /// (`MAX_RANGES_PER_FILE`), or waiter cap reached
    /// (`MAX_WAITERS_PER_FILE`), or `LockId` counter approaching
    /// `LOCK_ID_CEILING`.  Maps to `FSERR_QUOTA_EXCEEDED` at the
    /// native boundary.
    QuotaExceeded,
    /// A `wait: true` acquire was cancelled while parked — either
    /// via explicit `cancel_wait`, via the deploy-end sweep, or
    /// because the `LockRegistry` was dropped with waiters still
    /// parked.  Maps to `FSERR_CANCELLED`.
    Cancelled,
    /// Cross-deploy mutual-wait deadlock detection: the requested
    /// `wait: true` acquire would close a cycle in the cross-
    /// deploy wait-for graph — some current holder H of the target
    /// `(dev, inode)` is transitively parked waiting for a lock
    /// held by the requesting deploy.  Refused eagerly at enqueue
    /// time; no `Waiter` struct is allocated.  Maps to
    /// `FSERR_DEADLOCK`.
    ///
    /// # Consensus-observable
    ///
    /// Every validator computes the same wait-for graph from a
    /// byte-identical `LockRegistry`, so the cycle predicate is
    /// byte-identical.  Firing order relative to `QuotaExceeded`
    /// is fixed: quota is checked first (matches the existing
    /// idiom + O(1) cost).
    Deadlock,
}

/// One granted range lock.
///
/// # PartialEq / Eq
///
/// Derived — `[u8; 32]` and every other field type is already
/// `PartialEq`.  Note that derived equality uses **short-
/// circuiting** compares on the `holder: HolderId` field, which
/// is *NOT* constant-time.  This is fine for test-side auditing
/// (the release-path lookup uses `HolderId::ct_eq` explicitly,
/// not `==` on the containing `RangeEntry`), but a future author
/// who reaches for `range_entry_a == range_entry_b` in an
/// authentication-adjacent path SHOULD re-derive equality
/// explicitly using `ct_eq` on the `holder` component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeEntry {
    pub id: LockId,
    pub offset: u64,
    pub length: u64,
    pub mode: LockMode,
    pub holder: HolderId,
    pub deploy: DeployScope,
}

// ===========================================================
// State containers + registry skeleton (slice 2)
// ===========================================================

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::{oneshot, RwLock};

/// Whole-file sequential lock record — one per `(dev, inode)`.
/// Mutually exclusive with any range lock on the same file.
#[derive(Debug, Clone)]
pub struct SequentialEntry {
    pub id: LockId,
    pub holder: HolderId,
    pub deploy: DeployScope,
}

/// Per-`(dev, inode)` lock state.  Combines:
///
///   - `ranges` — currently-granted range locks (one or more
///     non-overlapping ranges for Read, else at most one Write).
///   - `sequential_holder` — the whole-file sequential lock
///     holder, if any.  Mutually exclusive with `ranges` being
///     non-empty.
///   - `waiters` — FIFO queue of `wait: true` acquires that hit
///     a conflict.  Head-of-line serialization prevents writer
///     starvation under read-heavy admission.
///
/// # Representation choice
///
/// `Vec<RangeEntry>` scanned linearly on every acquire/release.
/// Correct against the four operations at any N; appropriate for
/// the small-N contention profile expected in MVP workloads.
/// Candidate future optimization: `BTreeMap<offset, ...>` or a
/// segment tree once real workloads expose N large enough for
/// the scan cost to matter.  The API surface hides the
/// representation, so a swap is behind an implementation
/// boundary.
///
/// # Waiter-queue lifecycle
///
/// A state with parked waiters is NOT evicted from the registry
/// map even if `ranges` and `sequential_holder` are empty — the
/// waiters need somewhere to live until admit or cancel.  The
/// yet-to-land release path checks `state_is_empty` AFTER
/// cancelling / waking before evicting.
#[derive(Debug, Default)]
pub struct FileLockState {
    pub ranges: Vec<RangeEntry>,
    pub sequential_holder: Option<SequentialEntry>,
    /// FIFO queue of `wait: true` acquires parked on a conflict.
    /// Private — the acquire path mints a `LockId` for the
    /// waiter and surfaces it via `AcquireOutcome::Parked`;
    /// callers manipulate the queue only indirectly via
    /// `cancel_wait` / release-side admission (both land in
    /// subsequent slices).  `allow(dead_code)` until those
    /// slices land; the field is pre-declared so the acquire
    /// slice doesn't need a schema change.
    #[allow(dead_code)]
    waiters: VecDeque<Waiter>,
}

/// Policy for a conflicting acquire: fail fast, or park and
/// await admission.
///
/// Slice 8a MVP (unmerged on this triage branch) always uses
/// `Fail`.  Slice 8b's `lockRange(..., {"wait": true})` opts
/// into `Wait`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitPolicy {
    /// Return `Err(LockError::Busy)` on conflict (MVP).
    Fail,
    /// On conflict, mint a `LockId`, enqueue a `Waiter` in the
    /// per-`(dev, inode)` FIFO queue, and return
    /// `AcquireOutcome::Parked { lock_id, admit }`.  The caller
    /// awaits `admit` for the eventual grant / cancel.
    Wait,
}

/// Outcome of a `wait: true`-capable acquire.  Wraps the
/// immediate-success path and the parked path uniformly.
///
/// Under `WaitPolicy::Fail`, only `Immediate` is ever returned
/// — a conflict short-circuits to `Err(LockError::Busy)` before
/// the dispatcher ever constructs this enum.
///
/// # Not `Clone`
///
/// Deliberately NOT `Clone` because the `Parked` variant
/// carries `oneshot::Receiver`, which has move-only semantics
/// by design (a one-shot channel's receiver cannot be
/// duplicated — the admission signal fires exactly once).
/// Callers that need to pass the outcome by reference should
/// `match`-destructure once + hold the pieces individually.
#[derive(Debug)]
pub enum AcquireOutcome {
    /// Acquired immediately.  Behaves exactly like a pre-wait-
    /// policy `Ok(LockId)` return.
    Immediate(LockId),
    /// Parked in the waiter queue.  `lock_id` is the id that
    /// WILL be granted on admission (also the handle for
    /// `cancel_wait`).  `admit` resolves to:
    ///
    ///   - `Ok(Ok(lock_id))` — a release path admitted this
    ///     waiter.
    ///   - `Ok(Err(LockError::Cancelled))` — the deploy-end
    ///     sweep or an explicit `cancel_wait` fired.
    ///   - `Err(_)` — `oneshot::RecvError`, surfaced when the
    ///     `LockRegistry` is dropped without signalling.  The
    ///     caller should treat this as `Cancelled` too.
    Parked {
        lock_id: LockId,
        admit: oneshot::Receiver<Result<LockId, LockError>>,
    },
}

/// One parked wait entry.  Private — the waiter's identity is
/// only visible externally as its `LockId` (used by
/// `cancel_wait`, a subsequent slice).
///
/// Fields are pre-declared for the yet-to-land acquire/release
/// slices; `#[allow(dead_code)]` suppresses the dead-field
/// warning until those slices wire them up.  Preserved via a
/// module-scope attribute (not per-field) because the acquire
/// slice will light up every field at once.
#[allow(dead_code)]
#[derive(Debug)]
struct Waiter {
    lock_id: LockId,
    kind: WaitKind,
    holder: HolderId,
    deploy: DeployScope,
    /// Signalled with `Ok(lock_id)` on admission or
    /// `Err(LockError::Cancelled)` on cancel.  Dropping the
    /// sender (registry drop / waiter removal without signal)
    /// surfaces to the receiver as `Err(RecvError)` which the
    /// caller treats as Cancelled.
    admit: oneshot::Sender<Result<LockId, LockError>>,
}

/// What kind of lock a parked waiter is trying to take.
/// Pre-declared for the yet-to-land acquire slices (same
/// `allow(dead_code)` rationale as `Waiter`).
#[allow(dead_code)]
#[derive(Debug)]
enum WaitKind {
    Range {
        offset: u64,
        length: u64,
        mode: LockMode,
    },
    Sequential,
}

/// Range-lock registry — shared across every runtime spawned
/// from a single `RuntimeManager` via `share_lock_registry`
/// (yet-to-land, mirrors the `RootIdentityRegistry` broadcast
/// pattern).
///
/// # Lock topology
///
/// `RwLock<HashMap<DevInode, FileLockState>>` gives contention-
/// free concurrent reads for the yet-to-land `is_locked` query
/// on the unlink gate hot path.  Writes serialize acquire /
/// release / sweep — expected low volume vs. read path.
///
/// `next_lock_id` is a monotone `AtomicU64` — LockIds are
/// ephemeral per-runtime handles, NOT consensus-observable.
/// Two validators replaying the same WAL may mint different
/// LockId sequences; the only consensus-observable surface is
/// the WAL (which carries the holder's semantic action, not
/// the opaque LockId).
///
/// # LockId minting discipline
///
/// `mint_next_lock_id` returns `Err(LockError::QuotaExceeded)`
/// past [`LOCK_ID_CEILING`] to prevent 2⁶⁴-wrap collisions with
/// stale `LockToken`s still in RSpace.  Skips zero so a
/// sentinel-friendly `0u64` can distinguish "no lock" from "lock
/// with id 0" if callers ever need the discipline.
#[derive(Debug, Clone, Default)]
pub struct LockRegistry {
    /// Per-`(dev, inode)` state map.  Private — exposed via
    /// yet-to-land `try_acquire_range` / `release_range` /
    /// `is_locked` methods.  `allow(dead_code)` until those
    /// methods wire up the field.
    #[allow(dead_code)]
    inner: Arc<RwLock<HashMap<DevInode, FileLockState>>>,
    next_lock_id: Arc<AtomicU64>,
}

impl LockRegistry {
    /// Fresh empty registry.  LockIds start at 1 (zero is
    /// skipped per the type-level comment on
    /// `mint_next_lock_id`).
    pub fn new() -> Self { Self::default() }

    /// Mint the next monotone `LockId`.  Skips zero (sentinel-
    /// friendly) and refuses past [`LOCK_ID_CEILING`] with
    /// `LockError::QuotaExceeded`.
    ///
    /// Returns `Ok(LockId)` with a value in `[1, LOCK_ID_CEILING]`
    /// on success.
    ///
    /// # Why BOTH checks (ceiling + TryFrom)
    ///
    /// On this triage branch [`LOCK_ID_CEILING`] is defined as
    /// `(i64::MAX as u64) - (1 << 16)`, so it strictly precedes
    /// the `TryFrom<u64> for LockId` reject boundary at
    /// `i64::MAX`.  The `lock_consensus_constants_pinned` test
    /// pins this relationship with
    /// `assert!(LOCK_ID_CEILING <= i64::MAX as u64)` and a
    /// companion compile-time assertion lives on
    /// `LOCK_ID_CEILING`'s definition.  So when control reaches
    /// `LockId::try_from(raw)` below, `raw` is already in the
    /// `TryFrom`-safe range — the `.map_err` arm appears dead
    /// today.
    ///
    /// Kept intentionally as a belt-and-suspenders defense: a
    /// future refactor that loosened `LOCK_ID_CEILING` toward
    /// `u64::MAX - 2^16` (matching fileio's upstream shape)
    /// without also loosening `TryFrom` would silently truncate
    /// values on the Rholang `i64` round-trip.  The `?` through
    /// the fallible path makes the invariant explicit at every
    /// mint site.
    pub fn mint_next_lock_id(&self) -> Result<LockId, LockError> {
        let raw = self.next_lock_id.fetch_add(1, Ordering::Relaxed);
        // Skip 0: the counter starts at 0 so the first `fetch_add`
        // returns 0 — bump through to 1.  This skip-zero logic
        // assumes `next_lock_id` is never explicitly reset to 0
        // after construction (there is no public reset method
        // today; a future contributor adding one must either
        // teach this loop to retry past the reset or document
        // that mint-after-reset can race a concurrent mint and
        // return 0).
        let raw = if raw == 0 {
            self.next_lock_id.fetch_add(1, Ordering::Relaxed)
        } else {
            raw
        };
        if raw > LOCK_ID_CEILING {
            return Err(LockError::QuotaExceeded);
        }
        LockId::try_from(raw).map_err(|_| LockError::QuotaExceeded)
    }
}

// Compile-time witness that `LockRegistry: Send + Sync` —
// required for the yet-to-land handler-dispatch spawn_blocking
// path and for sharing across runtime clones via
// `share_lock_registry`.  Hoisted to module scope so every
// `cargo build` catches a regression.
const _LOCK_REGISTRY_IS_SEND_SYNC: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<LockRegistry>();
};

// ===========================================================
// Conflict-detection predicates (slice 3)
// ===========================================================
//
// Pure functions over `FileLockState` and the incoming acquire
// parameters.  Called from the yet-to-land `try_acquire_range` /
// `try_acquire_sequential` / `wake_waiters` methods.
// `#[allow(dead_code)]` until those methods land; tests in this
// file DO exercise each predicate, so test builds don't need
// the allow (cfg-test visibility suffices).

/// Two half-open intervals `[o1, o1+l1)` and `[o2, o2+l2)`
/// overlap iff `o1 < o2 + l2` AND `o2 < o1 + l1`.
///
/// Zero-length ranges do NOT overlap anything (defensive — the
/// natives should reject zero-length before calling here, but
/// the invariant is cheap and prevents a zero-length "lock"
/// from protecting nothing while also never conflicting).
///
/// Uses `saturating_add` to avoid overflow on `u64::MAX`
/// end-of-file sentinel ranges that the sequential-flag
/// whole-file query uses.
#[allow(dead_code)]
fn ranges_overlap(a: (u64, u64), b: (u64, u64)) -> bool {
    if a.1 == 0 || b.1 == 0 {
        return false;
    }
    let a_end = a.0.saturating_add(a.1);
    let b_end = b.0.saturating_add(b.1);
    a.0 < b_end && b.0 < a_end
}

/// Predicate: does an incoming range acquire conflict with any
/// currently-held lock in `state`?
///
/// Returns `true` on conflict (acquire must fail or park); false
/// on free (acquire can proceed).  Rules, in order:
///
///   1. If a sequential holder exists, every range acquire
///      conflicts (sequential is a whole-file exclusive lock).
///   2. For each existing range entry:
///      - Non-overlapping → no conflict (continue).
///      - Both this acquire AND the entry are `Read` →
///        reader-reader compatibility, no conflict (continue).
///      - Same `holder` → re-entrant acquire by the same cap,
///        no conflict (continue).  Matches POSIX fcntl(2)
///        semantics where a process may upgrade/downgrade/
///        shadow its own locks.
///      - Otherwise → conflict.
///
/// The holder check is intentionally structural `==` on
/// `HolderId` (not `ct_eq`): this is a scan-time admissibility
/// check, not an authentication step.  The release path uses
/// `ct_eq` explicitly.
#[allow(dead_code)]
fn range_conflicts(
    state: &FileLockState,
    offset: u64,
    length: u64,
    mode: LockMode,
    holder: &HolderId,
) -> bool {
    if state.sequential_holder.is_some() {
        return true;
    }
    for entry in &state.ranges {
        if !ranges_overlap((entry.offset, entry.length), (offset, length)) {
            continue;
        }
        if mode == LockMode::Read && entry.mode == LockMode::Read {
            continue;
        }
        if &entry.holder == holder {
            continue;
        }
        return true;
    }
    false
}

/// Predicate: does an incoming sequential acquire conflict with
/// any currently-held lock in `state`?
///
/// Sequential requires the state entirely empty (no ranges, no
/// sequential_holder) per the FIP coexistence rule.  Does NOT
/// use the same-holder skip — a cap that already holds a range
/// cannot upgrade to sequential without releasing first.
#[allow(dead_code)]
fn sequential_conflicts(state: &FileLockState) -> bool {
    state.sequential_holder.is_some() || !state.ranges.is_empty()
}

/// A `FileLockState` is "empty" (safe to evict from the
/// registry map) only when it has no held locks AND no parked
/// waiters.  A state with parked waiters MUST NOT be evicted —
/// dropping the `Waiter`'s `admit` sender would signal cancel
/// to the caller even though nobody called `cancel_wait`, and
/// the waiter would silently disappear from the queue.
#[allow(dead_code)]
fn state_is_empty(state: &FileLockState) -> bool {
    state.ranges.is_empty() && state.sequential_holder.is_none() && state.waiters.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- LockId newtype discipline --------------------------------

    /// Every value in `[0, i64::MAX]` round-trips through
    /// `TryFrom<u64>` + `as_u64`.  Covers the boundary points
    /// (0, 1, `LOCK_ID_CEILING`, `i64::MAX`) plus a middle value.
    #[test]
    fn lockid_try_from_accepts_wire_safe_values() {
        for raw in [0u64, 1, 42, LOCK_ID_CEILING, i64::MAX as u64] {
            let id = LockId::try_from(raw).expect("wire-safe value must accept");
            assert_eq!(id.as_u64(), raw);
        }
    }

    /// `TryFrom<u64>` rejects values above `i64::MAX` — those
    /// cannot round-trip through Rholang's `i64` integer type
    /// without truncation.
    #[test]
    fn lockid_try_from_rejects_values_above_i64_max() {
        for raw in [
            (i64::MAX as u64) + 1,
            u64::MAX / 2 + (1u64 << 62),
            u64::MAX - 1,
            u64::MAX,
        ] {
            match LockId::try_from(raw) {
                Err(LockIdOutOfRange { attempted }) => assert_eq!(attempted, raw),
                Ok(_) => panic!("value {raw} above i64::MAX must reject"),
            }
        }
    }

    /// `LockId` must be `#[repr(transparent)]` so it can be
    /// transmuted / cast interchangeably with a raw `u64` at any
    /// FFI boundary (or the fd-table layer's serialization path)
    /// without a size / alignment mismatch.  The
    /// `_LOCKID_LAYOUT_MATCHES_U64` compile-time witness (module
    /// scope) catches drift at build time; this runtime pin is a
    /// belt for the suspenders.
    #[test]
    fn lockid_layout_matches_u64() {
        use std::mem::{align_of, size_of};
        assert_eq!(size_of::<LockId>(), size_of::<u64>());
        assert_eq!(align_of::<LockId>(), align_of::<u64>());
    }

    // --- HolderId ct_eq security -----------------------------------

    #[test]
    fn holderid_from_bytes_roundtrip() {
        let bytes = [0xABu8; 32];
        let h = HolderId::from_bytes(bytes);
        assert_eq!(h.bytes(), &bytes);
    }

    /// `ct_eq` matches derived `PartialEq` for identical inputs.
    #[test]
    fn holderid_ct_eq_matches_partial_eq_on_equal_inputs() {
        let a = HolderId::from_bytes([0x11u8; 32]);
        let b = HolderId::from_bytes([0x11u8; 32]);
        assert!(a.ct_eq(&b));
        assert_eq!(a, b);
    }

    /// `ct_eq` matches derived `PartialEq` for divergent inputs.
    #[test]
    fn holderid_ct_eq_matches_partial_eq_on_unequal_inputs() {
        let a = HolderId::from_bytes([0x11u8; 32]);
        let b = HolderId::from_bytes([0x22u8; 32]);
        assert!(!a.ct_eq(&b));
        assert_ne!(a, b);
    }

    /// A byte-difference at position 0 and one at position 31 both
    /// surface as `false` — a naive short-circuiting `==` would
    /// return at byte 0 for the former and only at byte 31 for the
    /// latter (structurally observable timing side channel).  This
    /// test doesn't measure timing (unreliable in CI) but pins the
    /// *behavioral* property that both positions correctly reject.
    /// A regression that broke `ct_eq` into an early-exit compare
    /// would still pass this test — pair with a source-level grep
    /// pin on the release path (subsequent slice) that fires if
    /// derived `==` sneaks in.
    #[test]
    fn holderid_ct_eq_rejects_regardless_of_difference_position() {
        let base = HolderId::from_bytes([0x00u8; 32]);

        let mut first = [0x00u8; 32];
        first[0] = 0xFF;
        assert!(!base.ct_eq(&HolderId::from_bytes(first)));

        let mut last = [0x00u8; 32];
        last[31] = 0xFF;
        assert!(!base.ct_eq(&HolderId::from_bytes(last)));
    }

    // --- LockMode --------------------------------------------------

    #[test]
    fn lockmode_variants_smoke() {
        let r = LockMode::Read;
        let w = LockMode::Write;
        assert_ne!(r, w);
        // Trigger Debug + Clone + Copy.
        let _ = format!("{r:?}{w:?}");
        let _ = r;
        let _ = w;
    }

    // --- Consensus constants ---------------------------------------

    /// Hard-fork surface pins.  Divergent caps fork consensus.
    #[test]
    fn lock_consensus_constants_pinned() {
        assert_eq!(MAX_RANGES_PER_FILE, 1024);
        assert_eq!(MAX_WAITERS_PER_FILE, 1024);
        assert_eq!(LOCK_ID_CEILING, (i64::MAX as u64) - (1 << 16));
        // Companion check: the ceiling MUST fit in `i64` so
        // `LockId::try_from(LOCK_ID_CEILING as u64)` succeeds.
        // Guards against a future edit that raises the ceiling
        // past `i64::MAX` without also loosening the `TryFrom`
        // wire-safety check.
        assert!(LOCK_ID_CEILING <= i64::MAX as u64);
        assert!(LockId::try_from(LOCK_ID_CEILING).is_ok());
    }

    // --- LockError -------------------------------------------------

    #[test]
    fn lockerror_variants_smoke() {
        for variant in [
            LockError::Busy,
            LockError::Closed,
            LockError::BadArg,
            LockError::QuotaExceeded,
            LockError::Cancelled,
            LockError::Deadlock,
        ] {
            // Trigger Debug + Copy + PartialEq.
            let _ = format!("{variant:?}");
            assert_eq!(variant, variant);
        }
    }

    // --- RangeEntry ------------------------------------------------

    #[test]
    fn range_entry_construction_smoke() {
        let entry = RangeEntry {
            id: LockId::try_from(7).unwrap(),
            offset: 100,
            length: 200,
            mode: LockMode::Write,
            holder: HolderId::from_bytes([0xAAu8; 32]),
            deploy: [0xBBu8; 32],
        };
        let _ = format!("{entry:?}");
        assert_eq!(entry, entry.clone());
    }

    // --- State containers (slice 2) -------------------------------

    #[test]
    fn sequential_entry_construction_smoke() {
        let e = SequentialEntry {
            id: LockId::try_from(5).unwrap(),
            holder: HolderId::from_bytes([0x11u8; 32]),
            deploy: [0x22u8; 32],
        };
        let _ = format!("{e:?}");
        let cloned = e.clone();
        assert_eq!(cloned.id.as_u64(), 5);
    }

    #[test]
    fn file_lock_state_default_is_empty() {
        let s = FileLockState::default();
        assert!(s.ranges.is_empty());
        assert!(s.sequential_holder.is_none());
        assert!(s.waiters.is_empty());
    }

    #[test]
    fn wait_policy_variants_smoke() {
        for p in [WaitPolicy::Fail, WaitPolicy::Wait] {
            // Trigger Debug + Copy + PartialEq.
            let _ = format!("{p:?}");
            assert_eq!(p, p);
        }
        assert_ne!(WaitPolicy::Fail, WaitPolicy::Wait);
    }

    #[test]
    fn acquire_outcome_immediate_carries_lock_id() {
        let outcome = AcquireOutcome::Immediate(LockId::try_from(42).unwrap());
        match outcome {
            AcquireOutcome::Immediate(id) => assert_eq!(id.as_u64(), 42),
            other => panic!("expected Immediate, got {other:?}"),
        }
    }

    #[test]
    fn acquire_outcome_parked_carries_lock_id_and_receiver() {
        let (tx, rx) = oneshot::channel();
        let outcome = AcquireOutcome::Parked {
            lock_id: LockId::try_from(99).unwrap(),
            admit: rx,
        };
        match outcome {
            AcquireOutcome::Parked { lock_id, admit } => {
                assert_eq!(lock_id.as_u64(), 99);
                // Signal through the sender → receiver sees it.
                let _ = tx.send(Ok(lock_id));
                let got = futures::executor::block_on(admit).unwrap().unwrap();
                assert_eq!(got.as_u64(), 99);
            }
            other => panic!("expected Parked, got {other:?}"),
        }
    }

    // --- LockRegistry skeleton (slice 2) --------------------------

    #[test]
    fn lock_registry_new_is_empty() {
        let reg = LockRegistry::new();
        // Via the public mint path we observe the counter starts
        // at 1 (zero skipped per the sentinel-friendly rule).
        assert_eq!(reg.mint_next_lock_id().unwrap().as_u64(), 1);
    }

    /// `mint_next_lock_id` returns monotonically increasing ids
    /// starting at 1 (zero skipped).  LOAD-BEARING: a future
    /// refactor that mints zero would break the sentinel-friendly
    /// discipline and silently create a lock the release path
    /// can't distinguish from "no lock."
    #[test]
    fn mint_next_lock_id_is_monotonic_and_skips_zero() {
        let reg = LockRegistry::new();
        let a = reg.mint_next_lock_id().unwrap().as_u64();
        let b = reg.mint_next_lock_id().unwrap().as_u64();
        let c = reg.mint_next_lock_id().unwrap().as_u64();
        assert_eq!(a, 1, "first mint skips zero → 1");
        assert_eq!(b, 2);
        assert_eq!(c, 3);
    }

    /// `mint_next_lock_id` refuses past `LOCK_ID_CEILING` with
    /// `QuotaExceeded`.  Prevents 2⁶⁴-wrap collisions with stale
    /// LockTokens in RSpace.  LOAD-BEARING consensus invariant.
    #[test]
    fn mint_next_lock_id_refuses_past_ceiling() {
        let reg = LockRegistry::new();
        // Push the counter to just past the ceiling.  The counter
        // is behind an `Arc`, so we have to go through the public
        // mint path repeatedly — do it via a direct store on the
        // Atomic that `Arc`-shares into the struct.
        reg.next_lock_id
            .store(LOCK_ID_CEILING + 1, Ordering::Relaxed);
        match reg.mint_next_lock_id() {
            Err(LockError::QuotaExceeded) => (),
            other => panic!("expected QuotaExceeded past ceiling, got {other:?}"),
        }
    }

    /// Right at the ceiling (`raw == LOCK_ID_CEILING`), the mint
    /// still succeeds — the refuse-threshold is `> ceiling`, not
    /// `>= ceiling`.
    #[test]
    fn mint_next_lock_id_accepts_at_ceiling() {
        let reg = LockRegistry::new();
        reg.next_lock_id.store(LOCK_ID_CEILING, Ordering::Relaxed);
        assert_eq!(reg.mint_next_lock_id().unwrap().as_u64(), LOCK_ID_CEILING);
    }

    /// Clones share the counter — a `Clone` LockRegistry (used by
    /// `share_lock_registry` to broadcast one registry across
    /// runtimes) must observe each other's mints.
    #[test]
    fn lock_registry_clone_shares_next_lock_id_counter() {
        let a = LockRegistry::new();
        let b = a.clone();
        let a1 = a.mint_next_lock_id().unwrap().as_u64();
        let b1 = b.mint_next_lock_id().unwrap().as_u64();
        let a2 = a.mint_next_lock_id().unwrap().as_u64();
        assert_eq!(a1, 1);
        assert_eq!(b1, 2, "clone sees prior mint on the sibling");
        assert_eq!(a2, 3);
    }

    /// `LockRegistry: Send + Sync` — pinned by the module-scope
    /// `_LOCK_REGISTRY_IS_SEND_SYNC` compile-time witness; this
    /// runtime pin is a belt-and-suspenders catch if someone
    /// deletes the witness.
    #[test]
    fn lock_registry_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<LockRegistry>();
    }

    // --- ranges_overlap --------------------------------------------

    /// Classic non-overlap: [0, 10) and [100, 110) are disjoint.
    #[test]
    fn ranges_overlap_disjoint_returns_false() {
        assert!(!ranges_overlap((0, 10), (100, 10)));
        assert!(!ranges_overlap((100, 10), (0, 10)));
    }

    /// Full containment: [0, 100) contains [10, 20).  Overlap.
    #[test]
    fn ranges_overlap_contained_returns_true() {
        assert!(ranges_overlap((0, 100), (10, 20)));
        assert!(ranges_overlap((10, 20), (0, 100)));
    }

    /// Partial overlap at the boundary: [0, 50) and [40, 20)
    /// overlap on [40, 50).
    #[test]
    fn ranges_overlap_partial_returns_true() {
        assert!(ranges_overlap((0, 50), (40, 20)));
        assert!(ranges_overlap((40, 20), (0, 50)));
    }

    /// Touching but non-overlapping: [0, 10) and [10, 10) share
    /// a single endpoint (10) but the half-open semantics mean
    /// no byte is in both.  Must NOT report overlap.
    #[test]
    fn ranges_overlap_touching_endpoints_returns_false() {
        assert!(!ranges_overlap((0, 10), (10, 10)));
        assert!(!ranges_overlap((10, 10), (0, 10)));
    }

    /// Zero-length ranges never overlap anything — defense
    /// against a zero-length "lock" protecting nothing.
    #[test]
    fn ranges_overlap_zero_length_never_overlaps() {
        assert!(!ranges_overlap((0, 0), (0, 10)));
        assert!(!ranges_overlap((5, 0), (0, 10)));
        assert!(!ranges_overlap((0, 10), (5, 0)));
        assert!(!ranges_overlap((0, 0), (0, 0)));
    }

    /// Saturating-add protects against overflow on `u64::MAX`
    /// end-of-file sentinel ranges.  `[u64::MAX - 10, 100)`
    /// mathematically wraps past u64::MAX; the function must
    /// clamp the end at u64::MAX and still detect overlap with
    /// a range near the top.
    #[test]
    fn ranges_overlap_saturates_on_overflow() {
        // Range near end-of-u64 with length that would overflow.
        let near_top = (u64::MAX - 10, 100);
        // Point inside the saturated range.
        let probe = (u64::MAX - 5, 1);
        assert!(ranges_overlap(near_top, probe));
        assert!(ranges_overlap(probe, near_top));
    }

    // --- range_conflicts -------------------------------------------

    fn mk_entry(offset: u64, length: u64, mode: LockMode, holder: HolderId) -> RangeEntry {
        RangeEntry {
            id: LockId::try_from(42).unwrap(),
            offset,
            length,
            mode,
            holder,
            deploy: [0u8; 32],
        }
    }

    /// Sequential holder blocks every range acquire (whole-file
    /// exclusive).  LOAD-BEARING: pins the top-of-function
    /// short-circuit so a future refactor that interleaved the
    /// sequential check with the per-entry loop surfaces here.
    #[test]
    fn range_conflicts_blocked_by_sequential_holder() {
        let state = FileLockState {
            sequential_holder: Some(SequentialEntry {
                id: LockId::try_from(1).unwrap(),
                holder: HolderId::from_bytes([0xAAu8; 32]),
                deploy: [0u8; 32],
            }),
            ..Default::default()
        };
        let probe_holder = HolderId::from_bytes([0xBBu8; 32]);
        // Even an empty-ranges state blocks under sequential.
        assert!(range_conflicts(
            &state,
            0,
            100,
            LockMode::Read,
            &probe_holder
        ));
        assert!(range_conflicts(
            &state,
            500,
            1,
            LockMode::Write,
            &probe_holder
        ));
    }

    /// Non-overlapping ranges never conflict regardless of mode
    /// or holder.
    #[test]
    fn range_conflicts_non_overlapping_entries_do_not_conflict() {
        let holder_a = HolderId::from_bytes([0xAAu8; 32]);
        let holder_b = HolderId::from_bytes([0xBBu8; 32]);
        let state = FileLockState {
            ranges: vec![mk_entry(0, 10, LockMode::Write, holder_a)],
            ..Default::default()
        };
        // Disjoint range, different holder, write mode — must not conflict.
        assert!(!range_conflicts(
            &state,
            100,
            10,
            LockMode::Write,
            &holder_b
        ));
    }

    /// Reader-reader overlap is allowed (POSIX fcntl + fileio
    /// semantics).  LOAD-BEARING compatibility rule.
    #[test]
    fn range_conflicts_overlapping_read_locks_do_not_conflict() {
        let holder_a = HolderId::from_bytes([0xAAu8; 32]);
        let holder_b = HolderId::from_bytes([0xBBu8; 32]);
        let state = FileLockState {
            ranges: vec![mk_entry(0, 100, LockMode::Read, holder_a)],
            ..Default::default()
        };
        // Different holder, overlapping range, both Read → no conflict.
        assert!(!range_conflicts(&state, 50, 50, LockMode::Read, &holder_b));
    }

    /// Same-holder re-entrant acquires are allowed (POSIX fcntl
    /// semantics: a process may upgrade/downgrade/shadow its own
    /// locks).
    #[test]
    fn range_conflicts_same_holder_may_overlap_regardless_of_mode() {
        let holder = HolderId::from_bytes([0xAAu8; 32]);
        let state = FileLockState {
            ranges: vec![mk_entry(0, 100, LockMode::Write, holder.clone())],
            ..Default::default()
        };
        // Same holder overlapping Write-on-Write → no conflict.
        assert!(!range_conflicts(&state, 50, 50, LockMode::Write, &holder));
        // Same holder overlapping Read-on-Write → no conflict.
        assert!(!range_conflicts(&state, 50, 50, LockMode::Read, &holder));
    }

    /// Different-holder overlap with any Write involvement is a
    /// conflict.  LOAD-BEARING: three sub-cases
    /// (Write-on-Write, Write-on-Read, Read-on-Write).
    #[test]
    fn range_conflicts_different_holder_write_overlap_conflicts() {
        let holder_a = HolderId::from_bytes([0xAAu8; 32]);
        let holder_b = HolderId::from_bytes([0xBBu8; 32]);
        for (entry_mode, probe_mode) in [
            (LockMode::Write, LockMode::Write),
            (LockMode::Write, LockMode::Read),
            (LockMode::Read, LockMode::Write),
        ] {
            let state = FileLockState {
                ranges: vec![mk_entry(0, 100, entry_mode, holder_a.clone())],
                ..Default::default()
            };
            assert!(
                range_conflicts(&state, 50, 50, probe_mode, &holder_b),
                "{entry_mode:?} entry + {probe_mode:?} probe should conflict \
                 across holders"
            );
        }
    }

    /// Empty state (no ranges, no sequential_holder) → every
    /// range acquire succeeds.
    #[test]
    fn range_conflicts_empty_state_never_conflicts() {
        let state = FileLockState::default();
        let holder = HolderId::from_bytes([0xAAu8; 32]);
        assert!(!range_conflicts(&state, 0, 100, LockMode::Write, &holder));
        assert!(!range_conflicts(&state, 500, 1, LockMode::Read, &holder));
    }

    // --- sequential_conflicts --------------------------------------

    #[test]
    fn sequential_conflicts_empty_state_is_free() {
        assert!(!sequential_conflicts(&FileLockState::default()));
    }

    #[test]
    fn sequential_conflicts_held_sequential_blocks() {
        let state = FileLockState {
            sequential_holder: Some(SequentialEntry {
                id: LockId::try_from(1).unwrap(),
                holder: HolderId::from_bytes([0xAAu8; 32]),
                deploy: [0u8; 32],
            }),
            ..Default::default()
        };
        assert!(sequential_conflicts(&state));
    }

    /// Any held range (even a single read lock by the SAME holder)
    /// blocks a sequential acquire.  LOAD-BEARING: pins the
    /// no-same-holder-skip rule — a cap can't upgrade from
    /// range to sequential without releasing first.
    #[test]
    fn sequential_conflicts_held_range_blocks_even_for_same_holder() {
        let holder = HolderId::from_bytes([0xAAu8; 32]);
        let state = FileLockState {
            ranges: vec![mk_entry(0, 100, LockMode::Read, holder)],
            ..Default::default()
        };
        assert!(sequential_conflicts(&state));
    }

    // --- state_is_empty --------------------------------------------

    #[test]
    fn state_is_empty_default_state_is_empty() {
        assert!(state_is_empty(&FileLockState::default()));
    }

    #[test]
    fn state_is_empty_held_range_is_not_empty() {
        let state = FileLockState {
            ranges: vec![mk_entry(
                0,
                10,
                LockMode::Read,
                HolderId::from_bytes([0xAAu8; 32]),
            )],
            ..Default::default()
        };
        assert!(!state_is_empty(&state));
    }

    #[test]
    fn state_is_empty_held_sequential_is_not_empty() {
        let state = FileLockState {
            sequential_holder: Some(SequentialEntry {
                id: LockId::try_from(1).unwrap(),
                holder: HolderId::from_bytes([0xAAu8; 32]),
                deploy: [0u8; 32],
            }),
            ..Default::default()
        };
        assert!(!state_is_empty(&state));
    }

    /// LOAD-BEARING anti-eviction invariant: a state with ONLY
    /// parked waiters (empty ranges + no sequential holder)
    /// is still NOT empty.  Evicting it would drop the
    /// `Waiter`'s `admit` sender, signalling cancel to a caller
    /// that never asked for it.  Pin against a future refactor
    /// that tightened the `state_is_empty` definition to just
    /// `ranges.is_empty() && sequential_holder.is_none()`.
    #[test]
    fn state_is_empty_parked_waiters_prevent_eviction() {
        let (tx, _rx) = oneshot::channel();
        let waiter = Waiter {
            lock_id: LockId::try_from(99).unwrap(),
            kind: WaitKind::Sequential,
            holder: HolderId::from_bytes([0xAAu8; 32]),
            deploy: [0u8; 32],
            admit: tx,
        };
        let mut state = FileLockState::default();
        state.waiters.push_back(waiter);
        assert!(
            !state_is_empty(&state),
            "waiters alone must prevent eviction"
        );
    }
}
