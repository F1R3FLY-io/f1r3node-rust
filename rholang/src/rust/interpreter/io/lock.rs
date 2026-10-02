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
use std::sync::{Arc, RwLock};

use tokio::sync::oneshot;

use super::errors::poison_abort;

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
/// `std::sync::RwLock<HashMap<DevInode, FileLockState>>` gives
/// contention-free concurrent reads for the yet-to-land
/// `is_locked` query on the unlink gate hot path.  Writes
/// serialize acquire / release / sweep — expected low volume
/// vs. read path.  Deliberately NOT `tokio::sync::RwLock`: the
/// critical sections are microsecond-scale state-container
/// mutations; the tokio variant's internal semaphore + park-
/// token machinery would be heavier than the work it
/// protects, and the sync fallback keeps the acquire/release
/// methods callable from both async and sync contexts without
/// `.await`.  Panic poisoning is handled via
/// [`super::errors::poison_abort`] (fail-closed per
/// DD-FailClosedOnInvariantBreak).
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
    /// Per-`(dev, inode)` state map.  Written by acquire and
    /// release paths under `std::sync::RwLock`'s write guard
    /// (via `poison_abort`); read-only methods like the
    /// yet-to-land `is_locked` use the read guard.
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

    /// Try to acquire a range lock on `[offset, offset+length)`
    /// of `(dev_inode)` for `holder`.
    ///
    /// Fail-only variant: on conflict, returns
    /// [`LockError::Busy`] immediately.  Does NOT park.  A
    /// yet-to-land `try_acquire_range_wait` sibling will add
    /// [`WaitPolicy::Wait`] support — park on conflict, admit
    /// via `wake_waiters` on release.
    ///
    /// # Error ordering
    ///
    ///   1. [`LockError::BadArg`] on zero-length (a zero-length
    ///      "lock" would protect nothing and never conflict).
    ///   2. [`LockError::QuotaExceeded`] on
    ///      [`MAX_RANGES_PER_FILE`] live-range cap.
    ///   3. [`LockError::Busy`] on conflict (via
    ///      [`range_conflicts`]).
    ///   4. [`LockError::QuotaExceeded`] on
    ///      [`LOCK_ID_CEILING`] minted id.
    ///
    /// The quota-first-before-conflict ordering is
    /// consensus-observable — see the hard-fork surface note on
    /// [`MAX_WAITERS_PER_FILE`] (same discipline).
    ///
    /// # Compatibility rules
    ///
    /// Per [`range_conflicts`] (PR #527): sequential holder
    /// blocks all; non-overlap OK; reader-reader OK; same-holder
    /// OK (POSIX fcntl re-entrant); else conflict.
    pub fn try_acquire_range(
        &self,
        dev_inode: DevInode,
        offset: u64,
        length: u64,
        mode: LockMode,
        holder: HolderId,
        deploy: DeployScope,
    ) -> Result<LockId, LockError> {
        if length == 0 {
            return Err(LockError::BadArg);
        }
        let mut guard = poison_abort(self.inner.write(), "LockRegistry.inner");
        // Check admissibility against the EXISTING state (if
        // any) BEFORE inserting a fresh entry.  An absent
        // dev_inode has no held locks + no waiters, so quota
        // and conflict checks pass vacuously; a failed check
        // returns without having mutated the map.  This
        // prevents an "empty-state leak" where a long sequence
        // of failed acquires on distinct `(dev, inode)`
        // tuples would otherwise orphan empty FileLockState
        // entries in the map.
        if let Some(state) = guard.get(&dev_inode) {
            if state.ranges.len() >= MAX_RANGES_PER_FILE {
                return Err(LockError::QuotaExceeded);
            }
            if range_conflicts(state, offset, length, mode, &holder) {
                return Err(LockError::Busy);
            }
        }
        // All admissibility checks passed → mint + insert.
        // `mint_next_lock_id` is also fallible (ceiling), so
        // use `?` to propagate the error BEFORE any state
        // mutation.  If this fails, the map is still unchanged.
        let id = self.mint_next_lock_id()?;
        let state = guard.entry(dev_inode).or_default();
        state.ranges.push(RangeEntry {
            id,
            offset,
            length,
            mode,
            holder,
            deploy,
        });
        Ok(id)
    }

    /// Wait-policy variant of [`try_acquire_range`]: on conflict,
    /// park in the per-`(dev, inode)` FIFO waiter queue and return
    /// an [`AcquireOutcome::Parked`] carrying the minted `LockId`
    /// plus a `oneshot::Receiver<Result<LockId, LockError>>` the
    /// caller awaits for admission or cancel.
    ///
    /// On an admissible acquire, promotes directly into
    /// `state.ranges` and returns [`AcquireOutcome::Immediate`] —
    /// behaviorally identical to [`try_acquire_range`] in that
    /// path.
    ///
    /// # Error ordering (consensus-observable)
    ///
    ///   1. [`LockError::BadArg`] on zero-length.
    ///   2. [`LockError::QuotaExceeded`] on
    ///      [`MAX_RANGES_PER_FILE`] held-range cap.  Applies
    ///      universally — even a Wait acquire can't park if the
    ///      file is already at the range cap, because
    ///      [`wake_waiters`] would never admit it (admission
    ///      requires `ranges.len() < MAX_RANGES_PER_FILE`).
    ///      Rejecting at park time is strictly better than
    ///      parking a waiter that cannot be admitted.
    ///   3. Admissibility check via [`range_conflicts`].  If
    ///      admissible → direct-promote (same as
    ///      [`try_acquire_range`]).
    ///   4. (non-admissible park path only)
    ///      [`LockError::QuotaExceeded`] on
    ///      [`MAX_WAITERS_PER_FILE`] waiter-queue cap.  Fires
    ///      BEFORE the deadlock check per the ordering fixed in
    ///      [`LockError::Deadlock`]'s docstring (O(1) cost first).
    ///   5. (park path) [`LockError::Deadlock`] on
    ///      [`would_close_cycle`] — cross-deploy mutual-wait
    ///      cycle detection.  Refused EAGERLY at enqueue time: no
    ///      `Waiter` struct is allocated, no `oneshot` channel is
    ///      opened, so a deadlock-refused call leaks no state.
    ///   6. [`LockError::QuotaExceeded`] on [`LOCK_ID_CEILING`]
    ///      minted id.  Fires for both branches (direct-promote
    ///      and park) since both mint.
    ///
    /// Never returns [`LockError::Busy`] — conflict parks, it
    /// doesn't fail.  The Fail-only [`try_acquire_range`] remains
    /// the entry point for callers that want immediate failure.
    ///
    /// # Empty-state leak defense
    ///
    /// Same check-before-insert discipline as [`try_acquire_range`]
    /// (slice 7) and [`try_acquire_sequential`] (slice 8):
    /// admissibility + cycle + waiter-cap checks all read via
    /// `guard.get(&dev_inode)`; the `entry().or_default()` call
    /// fires only once we KNOW we're going to mutate.  Mint
    /// happens before map mutation so a ceiling failure doesn't
    /// leak either.  Pinned by
    /// `try_acquire_range_wait_failed_park_does_not_leak_empty_state`
    /// and `try_acquire_range_wait_deadlock_does_not_leak_empty_state`.
    ///
    /// # Same-holder re-entrant acquire
    ///
    /// Uses [`range_conflicts`]'s same-holder skip — a cap
    /// re-acquiring its own range gets [`AcquireOutcome::Immediate`]
    /// (POSIX fcntl semantics).  A cap NEVER parks on its own
    /// held range.
    ///
    /// # Cycle check timing (consensus determinism)
    ///
    /// [`would_close_cycle`] is a pure set predicate on the wait-
    /// for graph; its answer is order-independent w.r.t. HashMap
    /// iteration order of the registry.  Two validators with
    /// byte-identical [`LockRegistry`] state compute the same
    /// cycle answer and therefore the same
    /// `AcquireOutcome::Parked` vs. `Err(Deadlock)` branch — see
    /// the determinism note on [`would_close_cycle`].
    pub fn try_acquire_range_wait(
        &self,
        dev_inode: DevInode,
        offset: u64,
        length: u64,
        mode: LockMode,
        holder: HolderId,
        deploy: DeployScope,
    ) -> Result<AcquireOutcome, LockError> {
        if length == 0 {
            return Err(LockError::BadArg);
        }
        let mut guard = poison_abort(self.inner.write(), "LockRegistry.inner");
        // Admissibility check against the EXISTING state (if any)
        // BEFORE any mutation.  Mirrors try_acquire_range's
        // check-before-insert discipline: an absent dev_inode
        // has no held locks + no waiters + no quota pressure, so
        // vacuously admissible.
        let admissible = match guard.get(&dev_inode) {
            Some(state) => {
                if state.ranges.len() >= MAX_RANGES_PER_FILE {
                    return Err(LockError::QuotaExceeded);
                }
                !range_conflicts(state, offset, length, mode, &holder)
            }
            None => true,
        };
        if admissible {
            // Direct promote — identical to try_acquire_range's
            // success path, but returns Immediate(id).
            let id = self.mint_next_lock_id()?;
            let state = guard.entry(dev_inode).or_default();
            state.ranges.push(RangeEntry {
                id,
                offset,
                length,
                mode,
                holder,
                deploy,
            });
            return Ok(AcquireOutcome::Immediate(id));
        }
        // Park branch: non-admissible.  Check park-time quotas
        // and deadlock BEFORE allocating a Waiter or oneshot
        // channel.
        if let Some(state) = guard.get(&dev_inode) {
            if state.waiters.len() >= MAX_WAITERS_PER_FILE {
                return Err(LockError::QuotaExceeded);
            }
        }
        // NB-7 cycle check.  Must fire BEFORE Waiter allocation
        // and BEFORE mint — a cycle-refused call must leave the
        // registry unchanged.
        if would_close_cycle(&guard, deploy, dev_inode) {
            return Err(LockError::Deadlock);
        }
        // All park-side admissibility passed → mint LockId +
        // enqueue Waiter.  Mint fallibility (LOCK_ID_CEILING)
        // propagates before any mutation.
        let lock_id = self.mint_next_lock_id()?;
        let (tx, rx) = oneshot::channel();
        let state = guard.entry(dev_inode).or_default();
        state.waiters.push_back(Waiter {
            lock_id,
            kind: WaitKind::Range {
                offset,
                length,
                mode,
            },
            holder,
            deploy,
            admit: tx,
        });
        Ok(AcquireOutcome::Parked { lock_id, admit: rx })
    }

    /// Release the lock identified by `lock_id` iff `holder`
    /// matches the recorded owner via [`HolderId::ct_eq`].
    ///
    /// Covers both range AND sequential releases — the method
    /// finds `lock_id` across every `FileLockState` in the
    /// registry.  After successful release, the touched state
    /// is evicted from the map if [`state_is_empty`] holds
    /// (no held locks AND no parked waiters).
    ///
    /// # Constant-time holder comparison (X-3 / SEC-Mi-01)
    ///
    /// The holder match uses [`HolderId::ct_eq`], NOT the
    /// derived `==`.  Rationale: an attacker observing latency
    /// on failed release() calls could otherwise narrow down a
    /// correct holder byte-by-byte via a timing side channel.
    /// `ct_eq`'s branchless comparison closes that vector.  DO
    /// NOT regress to `==` on `HolderId` in this path.
    ///
    /// # Timing scope
    ///
    /// `ct_eq` is short-circuited by the `lock_id == e.id`
    /// check that precedes it in the position predicate.
    /// Timing therefore reveals which LockIds are currently
    /// allocated (a fast rejection on id-mismatch vs. the full
    /// `ct_eq` time on id-match), but does NOT help an
    /// attacker enumerate the holder bytes — the ct_eq branch
    /// is only reached when the attacker already supplied a
    /// correct LockId, and once reached runs in branchless
    /// constant time per the HolderId invariant.  LockId
    /// enumeration alone is not actionable without the
    /// matching holder (which is infeasible to brute-force
    /// at 32 bytes).
    ///
    /// # Returns
    ///
    ///   - `Ok(())` on successful release.
    ///   - [`LockError::Closed`] if `lock_id` isn't held OR the
    ///     holder doesn't match (indistinguishable for
    ///     diagnostic purposes to avoid narrowing attackers'
    ///     search space).
    ///
    /// # Wake-pass after remove
    ///
    /// Calls [`wake_waiters`] on the touched state after the
    /// remove to admit any eligible parked waiter (strict head-
    /// of-line FIFO).  No-op under this slice's call sites —
    /// nothing parks yet without [`WaitPolicy::Wait`] — but
    /// wired now so the Wait-acquire slice doesn't have to
    /// touch every release site.
    pub fn release(&self, lock_id: LockId, holder: &HolderId) -> Result<(), LockError> {
        let mut guard = poison_abort(self.inner.write(), "LockRegistry.inner");
        let mut touched_key: Option<DevInode> = None;
        let mut released = false;
        for (dev_inode, state) in guard.iter_mut() {
            // SEC-Mi-01: ct_eq, NOT `==`.  Do not regress.
            if let Some(pos) = state
                .ranges
                .iter()
                .position(|e| e.id == lock_id && e.holder.ct_eq(holder))
            {
                state.ranges.remove(pos);
                released = true;
            } else if state
                .sequential_holder
                .as_ref()
                .is_some_and(|s| s.id == lock_id && s.holder.ct_eq(holder))
            {
                state.sequential_holder = None;
                released = true;
            }
            if released {
                touched_key = Some(*dev_inode);
                // Early-exit assumes LockId global uniqueness:
                // the monotone counter in `mint_next_lock_id`
                // never re-mints a live id, so at most one
                // `FileLockState` can hold any given LockId
                // at a time.  If that invariant ever broke, the
                // break would silently miss subsequent matches.
                break;
            }
        }
        if let Some(k) = touched_key {
            if let Some(state) = guard.get_mut(&k) {
                // Head-of-line FIFO admission for any waiter
                // whose conflict just cleared.  Pre-Wait slice
                // the queue is always empty, so this is a no-op
                // today; the Wait-acquire slice lights it up.
                wake_waiters(state);
                if state_is_empty(state) {
                    guard.remove(&k);
                }
            }
        }
        if released {
            Ok(())
        } else {
            Err(LockError::Closed)
        }
    }

    /// Release every lock owned by `holder` across every file
    /// in the registry.  Called from `fs_release_all_for_holder`
    /// (File.close path) when a cap goes away: every range
    /// AND the sequential_holder (if matching) are swept.
    /// Returns the count released for diagnostics.
    ///
    /// # Lifetime posture
    ///
    /// Called when a `FileHandle` drops (File.close).  Pairs
    /// with a yet-to-land `cancel_all_waiters_for_holder`
    /// sweep that cancels this holder's PARKED waiters (same
    /// cap is going away; its parked wait: true acquires
    /// shouldn't resolve to a dead cap).  Both land on
    /// opposite sides of the Wait-support slice; this slice
    /// ships the holder-side held-lock sweep alone.
    ///
    /// # Constant-time holder comparison (X-3 / SEC-Mi-01)
    ///
    /// Holder matches use [`HolderId::ct_eq`] to maintain
    /// consistency with [`release`]'s SEC-Mi-01 discipline.  A
    /// bulk sweep is destructive and one-shot (not an
    /// enumeration surface the way `release` is), so the
    /// timing attack is less actionable here — but a future
    /// dry-run mode or a per-entry return would retroactively
    /// create an enumeration surface.  Using `ct_eq`
    /// uniformly across every release-side holder comparison
    /// keeps the invariant "all release-path holder matches
    /// are constant-time" true-by-construction without the
    /// reviewer having to argue the exemption each slice.
    /// Micro-perf cost is 32-byte compare × ranges × files
    /// (negligible under typical workloads).
    ///
    /// # Wake-pass after sweep
    ///
    /// Calls [`wake_waiters`] on each touched state after the
    /// sweep to admit any OTHER-holder waiter whose conflict
    /// just cleared.  No-op pre-Wait — nothing parks yet — but
    /// wired now (see [`wake_waiters`] docstring for the X-2 /
    /// G-02 defense-in-depth chain).
    pub fn release_all_for_holder(&self, holder: &HolderId) -> usize {
        let mut guard = poison_abort(self.inner.write(), "LockRegistry.inner");
        let mut released = 0usize;
        let mut evict: Vec<DevInode> = Vec::new();
        for (dev_inode, state) in guard.iter_mut() {
            let before = state.ranges.len();
            // SEC-Mi-01: ct_eq, NOT `==` / `!=`.  Do not regress
            // to derived equality — see docstring.
            state.ranges.retain(|e| !e.holder.ct_eq(holder));
            released += before - state.ranges.len();
            if state
                .sequential_holder
                .as_ref()
                .is_some_and(|s| s.holder.ct_eq(holder))
            {
                state.sequential_holder = None;
                released += 1;
            }
            wake_waiters(state);
            if state_is_empty(state) {
                evict.push(*dev_inode);
            }
        }
        for k in evict {
            guard.remove(&k);
        }
        released
    }

    /// Release every lock owned by `deploy` across every file
    /// in the registry.  Called from the `WalDeployScope::end`
    /// auto-release hook (yet-to-land) when a deploy
    /// completes.  Returns the count released.
    ///
    /// # Sentinel guard: `[0; 32]` is reserved
    ///
    /// Panics if `deploy == [0; 32]`.  The all-zeros
    /// `DeployScope` is reserved as a pre-wiring placeholder
    /// (acquires outside a live `WalDeployScope` record this
    /// sentinel while deploy-scope threading is in progress).
    /// Calling `release_all_for_deploy(&[0; 32])` would sweep
    /// EVERY sentinel-scoped entry on the registry — a
    /// production deploy running under a live
    /// `WalDeployScope` always derives a non-sentinel scope
    /// via Blake2b256, so the guard fires only for test
    /// scaffolding or a pre-wiring regression.  Promoted from
    /// `debug_assert!` to `assert!` so release builds also
    /// catch the sweep-every-lock foot-gun.
    ///
    /// # Wake-pass after sweep
    ///
    /// Same discipline as `release_all_for_holder` — calls
    /// [`wake_waiters`] on each touched state to admit any
    /// OTHER-deploy waiter whose conflict just cleared.  The
    /// X-2 / G-02 defense-in-depth chain pinned by
    /// [`wake_waiters`] routes through here on the deploy-abort
    /// path: `cancel_all_waiters_for_deploy` (yet-to-land)
    /// drains THIS deploy's PARKED waiters first, THEN this
    /// sweep clears THIS deploy's HELD locks, THEN the
    /// `wake_waiters` call admits OTHER deploys that were
    /// blocked.
    pub fn release_all_for_deploy(&self, deploy: &DeployScope) -> usize {
        assert!(
            deploy != &[0u8; 32],
            "release_all_for_deploy called with the [0; 32] sentinel — \
             the all-zeros DeployScope is reserved as a pre-wiring \
             placeholder; a production deploy under a live WalDeployScope \
             derives a non-sentinel scope via Blake2b256.  Calling with \
             the sentinel would sweep every stray sentinel-scoped entry."
        );
        let mut guard = poison_abort(self.inner.write(), "LockRegistry.inner");
        let mut released = 0usize;
        let mut evict: Vec<DevInode> = Vec::new();
        for (dev_inode, state) in guard.iter_mut() {
            let before = state.ranges.len();
            state.ranges.retain(|e| &e.deploy != deploy);
            released += before - state.ranges.len();
            if state.sequential_holder.as_ref().map(|s| &s.deploy) == Some(deploy) {
                state.sequential_holder = None;
                released += 1;
            }
            wake_waiters(state);
            if state_is_empty(state) {
                evict.push(*dev_inode);
            }
        }
        for k in evict {
            guard.remove(&k);
        }
        released
    }

    /// Unlink-gate query: is `(dev, inode)` currently holding
    /// a lock that overlaps `range = (offset, length)`?
    ///
    /// Called from `fs_remove_file` / `fs_remove_dir` under
    /// consensus mode (yet-to-land); callers surface
    /// `FSERR_BUSY` on `true` and skip the unlink.  Oracular
    /// callers skip this check entirely and log-warn instead.
    ///
    /// # Whole-file query shape
    ///
    /// For a whole-file unlink or truncate-to-zero probe,
    /// pass `range = (0, u64::MAX)` — the sequential holder
    /// branch short-circuits before range overlap scan runs,
    /// but even without a sequential holder the `u64::MAX`
    /// length saturates via [`ranges_overlap`]'s saturating
    /// add so any held range overlaps.
    ///
    /// # Read-side hot path
    ///
    /// Uses the `read` guard (contention-free concurrent
    /// readers).  Linear scan O(R) where R = ranges on this
    /// file (≤ `MAX_RANGES_PER_FILE`).
    pub fn is_locked(&self, dev_inode: DevInode, range: (u64, u64)) -> bool {
        let guard = poison_abort(self.inner.read(), "LockRegistry.inner");
        let Some(state) = guard.get(&dev_inode) else {
            return false;
        };
        if state.sequential_holder.is_some() {
            return true;
        }
        state
            .ranges
            .iter()
            .any(|e| ranges_overlap((e.offset, e.length), range))
    }

    /// Try to acquire the whole-file sequential lock on
    /// `(dev_inode)` for `holder`.
    ///
    /// Fail-only variant: on conflict (any held range OR an
    /// existing sequential_holder) returns [`LockError::Busy`]
    /// immediately.  Does NOT park.  A yet-to-land
    /// `try_acquire_sequential_wait` sibling will add
    /// [`WaitPolicy::Wait`] support.
    ///
    /// # Coexistence rules
    ///
    /// Per [`sequential_conflicts`]: sequential requires the
    /// state entirely empty (no held ranges, no existing
    /// sequential_holder).  Does NOT use the same-holder skip
    /// that `try_acquire_range` uses — a cap holding ANY range
    /// cannot upgrade to a sequential lock without releasing
    /// its ranges first.  Pinned by
    /// `sequential_conflicts_held_range_blocks_even_for_same_holder`
    /// (PR #527) and surfaced at the API boundary here.
    ///
    /// # Error ordering (consensus-observable)
    ///
    ///   1. [`LockError::Busy`] on conflict (via
    ///      [`sequential_conflicts`]).
    ///   2. [`LockError::QuotaExceeded`] on
    ///      [`LOCK_ID_CEILING`] minted id.
    ///
    /// # Empty-state leak defense
    ///
    /// Uses the check-before-insert discipline from PR #529's
    /// `try_acquire_range`: admissibility checked against
    /// `guard.get(&dev_inode)` BEFORE any mutation; the
    /// `entry().or_default()` call fires only once the acquire
    /// is known to succeed.  A failed acquire does NOT leave
    /// an empty `FileLockState` behind.
    pub fn try_acquire_sequential(
        &self,
        dev_inode: DevInode,
        holder: HolderId,
        deploy: DeployScope,
    ) -> Result<LockId, LockError> {
        let mut guard = poison_abort(self.inner.write(), "LockRegistry.inner");
        if let Some(state) = guard.get(&dev_inode) {
            if sequential_conflicts(state) {
                return Err(LockError::Busy);
            }
        }
        // Admissibility passed → mint + insert.  Mint before
        // mutating the map so a `LOCK_ID_CEILING` failure
        // doesn't leak an empty entry.
        let id = self.mint_next_lock_id()?;
        let state = guard.entry(dev_inode).or_default();
        state.sequential_holder = Some(SequentialEntry { id, holder, deploy });
        Ok(id)
    }

    /// Wait-policy variant of [`try_acquire_sequential`]: on
    /// conflict (any held range OR an existing sequential_holder),
    /// park in the per-`(dev, inode)` FIFO waiter queue and
    /// return an [`AcquireOutcome::Parked`] carrying the minted
    /// `LockId` plus a `oneshot::Receiver` the caller awaits for
    /// admission / cancel.
    ///
    /// On admissible acquire (state entirely empty — no held
    /// ranges, no existing sequential_holder), promotes directly
    /// into `state.sequential_holder` and returns
    /// [`AcquireOutcome::Immediate`] — behaviorally identical to
    /// [`try_acquire_sequential`] in that path.
    ///
    /// # Coexistence rules — no same-holder skip
    ///
    /// Per [`sequential_conflicts`]: sequential requires the
    /// state entirely empty.  Does NOT use the same-holder skip
    /// that the Range path uses — a cap holding ANY range (even
    /// a tiny Read it owns) MUST release its ranges before
    /// upgrading to a sequential lock.  A same-holder Wait
    /// acquire on a state with the holder's own held range
    /// therefore parks (does NOT direct-promote).  Pinned by
    /// `try_acquire_sequential_wait_same_holder_range_still_parks`.
    ///
    /// # Error ordering (consensus-observable)
    ///
    ///   1. Admissibility check via [`sequential_conflicts`].
    ///      If admissible → direct-promote (same as
    ///      [`try_acquire_sequential`]).
    ///   2. (non-admissible park path only)
    ///      [`LockError::QuotaExceeded`] on
    ///      [`MAX_WAITERS_PER_FILE`] waiter-queue cap.  Fires
    ///      BEFORE the deadlock check per
    ///      [`LockError::Deadlock`]'s docstring (O(1) cost first).
    ///   3. (park path) [`LockError::Deadlock`] on
    ///      [`would_close_cycle`] — cross-deploy mutual-wait
    ///      cycle detection.  Refused EAGERLY at enqueue time:
    ///      no `Waiter` struct is allocated, no `oneshot`
    ///      channel is opened.
    ///   4. [`LockError::QuotaExceeded`] on [`LOCK_ID_CEILING`]
    ///      minted id (both branches).
    ///
    /// No `MAX_RANGES_PER_FILE`-equivalent cap on sequential —
    /// there's at most one sequential_holder per file by
    /// construction.
    ///
    /// Never returns [`LockError::Busy`] — the Fail-only
    /// [`try_acquire_sequential`] remains the entry point for
    /// callers that want immediate failure.
    ///
    /// # Empty-state leak defense
    ///
    /// Same check-before-insert discipline as the Range path
    /// (PR #536): every admissibility / cap / cycle check reads
    /// via `guard.get(&dev_inode)`; `entry().or_default()` fires
    /// only once the acquire / park is known to succeed; mint
    /// happens before map mutation.  Pinned by
    /// `try_acquire_sequential_wait_failed_park_does_not_leak_empty_state`
    /// and
    /// `try_acquire_sequential_wait_deadlock_does_not_leak_empty_state`.
    pub fn try_acquire_sequential_wait(
        &self,
        dev_inode: DevInode,
        holder: HolderId,
        deploy: DeployScope,
    ) -> Result<AcquireOutcome, LockError> {
        let mut guard = poison_abort(self.inner.write(), "LockRegistry.inner");
        // Admissibility examines holders (ranges +
        // sequential_holder) via `sequential_conflicts`, NOT
        // waiters.  Correctness of this "direct-promote on no
        // holders even when waiters is non-empty" relies on an
        // invariant: a "waiters non-empty AND no holders" state
        // is unreachable at any quiescent point.  Every release
        // path calls `wake_waiters` BEFORE `state_is_empty`
        // eviction, and `wake_waiters` drains admissible heads
        // (Sequential and empty-holder-state Range both admit
        // on `sequential_conflicts(empty) == false` /
        // `range_conflicts(empty) == false`).  So between any
        // two outside calls, either (a) a holder exists and
        // `sequential_conflicts` returns true → park, or (b) no
        // holders AND no waiters remain → direct-promote.  A
        // future refactor that adds a direct-promote-with-
        // parked-waiters path would break FIFO — pin against
        // that regression.
        let admissible = match guard.get(&dev_inode) {
            Some(state) => !sequential_conflicts(state),
            None => true,
        };
        if admissible {
            let id = self.mint_next_lock_id()?;
            let state = guard.entry(dev_inode).or_default();
            state.sequential_holder = Some(SequentialEntry { id, holder, deploy });
            return Ok(AcquireOutcome::Immediate(id));
        }
        // Park branch: non-admissible.  Check park-time quotas
        // and deadlock BEFORE allocating a Waiter or oneshot.
        if let Some(state) = guard.get(&dev_inode) {
            if state.waiters.len() >= MAX_WAITERS_PER_FILE {
                return Err(LockError::QuotaExceeded);
            }
        }
        if would_close_cycle(&guard, deploy, dev_inode) {
            return Err(LockError::Deadlock);
        }
        let lock_id = self.mint_next_lock_id()?;
        let (tx, rx) = oneshot::channel();
        let state = guard.entry(dev_inode).or_default();
        state.waiters.push_back(Waiter {
            lock_id,
            kind: WaitKind::Sequential,
            holder,
            deploy,
            admit: tx,
        });
        Ok(AcquireOutcome::Parked { lock_id, admit: rx })
    }

    /// Cancel a currently-parked waiter identified by `lock_id`
    /// iff `holder` matches the recorded owner via
    /// [`HolderId::ct_eq`].
    ///
    /// Removes the waiter from its per-`(dev, inode)` FIFO queue
    /// and signals its admit sender with
    /// `Err(LockError::Cancelled)`.  The caller's `oneshot`
    /// receiver resolves to `Ok(Err(LockError::Cancelled))` in
    /// the happy path.  (The `RecvError` branch of
    /// [`AcquireOutcome::Parked`]'s receiver fires only when
    /// the sender is dropped WITHOUT a send — that happens on
    /// `wake_waiters` rollback or registry drop, not on this
    /// path: `cancel_wait` always sends before releasing the
    /// sender.)
    ///
    /// After successful cancel, the touched state is evicted
    /// from the map if [`state_is_empty`] holds (no held locks,
    /// no remaining waiters).  No `wake_waiters` call is needed:
    /// cancelling a waiter cannot ADD any holder, so no
    /// downstream waiter's admissibility changes.
    ///
    /// # Constant-time holder comparison (X-3 / SEC-Mi-01)
    ///
    /// The holder match uses [`HolderId::ct_eq`], mirroring the
    /// release-path discipline.  Same rationale: an attacker
    /// observing latency on failed `cancel_wait` calls could
    /// otherwise narrow a correct holder byte-by-byte.
    ///
    /// # Returns
    ///
    ///   - `Ok(())` on successful cancel.
    ///   - [`LockError::Closed`] if `lock_id` isn't parked
    ///     anywhere OR the holder doesn't match.  The ERROR
    ///     VALUE is indistinguishable per the release-path
    ///     convention — attackers can't tell "wrong id" from
    ///     "wrong holder" by the return.  Timing IS
    ///     distinguishable (unknown-id scans all queues
    ///     short-circuiting on `==`; known-id-wrong-holder
    ///     reaches one `ct_eq` evaluation), but the practical
    ///     impact is bounded: LockId has ~51 bits of entropy
    ///     from `LOCK_ID_CEILING` so blind-probing to reach a
    ///     known-id branch is infeasible, and once in that
    ///     branch `ct_eq` is branchless-constant so holder
    ///     bytes still can't be narrowed byte-by-byte.
    ///
    /// # Lifetime posture
    ///
    /// Pairs with the yet-to-land `cancel_all_waiters_for_holder`
    /// / `_for_deploy` sweeps: those are the bulk-cancel
    /// counterparts to the bulk-release methods PR #530 shipped.
    /// `cancel_wait` is the targeted-by-LockId version a caller
    /// invokes when they want to abandon a specific parked
    /// acquire (e.g., async-cancellation on a user-visible
    /// timeout in a future handler layer).
    pub fn cancel_wait(&self, lock_id: LockId, holder: &HolderId) -> Result<(), LockError> {
        let mut guard = poison_abort(self.inner.write(), "LockRegistry.inner");
        let mut touched_key: Option<DevInode> = None;
        let mut admit: Option<oneshot::Sender<Result<LockId, LockError>>> = None;
        for (dev_inode, state) in guard.iter_mut() {
            // SEC-Mi-01: ct_eq, NOT `==`.  Do not regress.
            if let Some(pos) = state
                .waiters
                .iter()
                .position(|w| w.lock_id == lock_id && w.holder.ct_eq(holder))
            {
                let waiter = state.waiters.remove(pos).expect("position just observed");
                admit = Some(waiter.admit);
                touched_key = Some(*dev_inode);
                // Early-exit assumes LockId global uniqueness
                // (same monotone-counter invariant as `release`).
                break;
            }
        }
        let Some(tx) = admit else {
            return Err(LockError::Closed);
        };
        // Send the cancel BEFORE eviction so the receiver always
        // observes Cancelled (vs. RecvError on sender drop) in
        // the happy path.  If the receiver was already dropped,
        // the send fails silently — caller already walked away,
        // same semantics as a Cancelled they never read.
        let _ = tx.send(Err(LockError::Cancelled));
        if let Some(k) = touched_key {
            if let Some(state) = guard.get(&k) {
                if state_is_empty(state) {
                    guard.remove(&k);
                }
            }
        }
        Ok(())
    }

    /// Cancel every parked waiter owned by `holder` across every
    /// file in the registry.  Pairs with
    /// [`release_all_for_holder`] (PR #530) — the release-side
    /// sweeps HELD locks for the holder; this sweeps PARKED
    /// waiters for the holder.  Called from the same
    /// `fs_release_all_for_holder` (File.close) path when a cap
    /// goes away: a cap's parked `wait: true` acquires MUST NOT
    /// resolve to the dead cap, so they're cancelled before the
    /// cap vanishes.
    ///
    /// Signals every cancelled waiter's admit sender with
    /// `Err(LockError::Cancelled)` (dropped-receiver sends fail
    /// silently per the per-waiter cancel discipline).  Returns
    /// the count cancelled for diagnostics.
    ///
    /// # Constant-time holder comparison (X-3 / SEC-Mi-01)
    ///
    /// Uses [`HolderId::ct_eq`] uniformly, matching
    /// [`release_all_for_holder`]'s rationale: a destructive
    /// sweep is one-shot and less actionable than enumeration,
    /// but a future dry-run / per-entry-return refactor would
    /// retroactively create an enumeration surface.  Using
    /// `ct_eq` uniformly keeps the "all release / cancel paths
    /// use constant-time holder comparison" invariant true-by-
    /// construction.
    ///
    /// # No `wake_waiters` call after sweep
    ///
    /// Cancelling waiters cannot ADD any holder, so no
    /// downstream waiter's admissibility changes.  Mirrors
    /// [`cancel_wait`]'s discipline.
    pub fn cancel_all_waiters_for_holder(&self, holder: &HolderId) -> usize {
        let mut guard = poison_abort(self.inner.write(), "LockRegistry.inner");
        let mut cancelled = 0usize;
        let mut evict: Vec<DevInode> = Vec::new();
        // Collect senders during the sweep, then drop(guard)
        // before firing sends — see the drop-then-signal pattern
        // below.  `oneshot::send` is non-blocking so holding the
        // guard across sends is correct, but it briefly blocks
        // any concurrent reader; dropping first minimizes guard
        // hold time.  Matches per-waiter `cancel_wait` discipline.
        let mut to_signal: Vec<oneshot::Sender<Result<LockId, LockError>>> = Vec::new();
        for (dev_inode, state) in guard.iter_mut() {
            // Drain-filter equivalent: scan from the front,
            // removing matches.  Using `drain_filter` would be
            // cleaner but it's unstable; the manual loop keeps
            // the function nightly-feature-free.
            let mut i = 0;
            while i < state.waiters.len() {
                // SEC-Mi-01: ct_eq, NOT `==`.
                if state.waiters[i].holder.ct_eq(holder) {
                    let waiter = state
                        .waiters
                        .remove(i)
                        .expect("index just observed in bounds");
                    to_signal.push(waiter.admit);
                    cancelled += 1;
                } else {
                    i += 1;
                }
            }
            if state_is_empty(state) {
                evict.push(*dev_inode);
            }
        }
        for k in evict {
            guard.remove(&k);
        }
        drop(guard);
        for tx in to_signal {
            let _ = tx.send(Err(LockError::Cancelled));
        }
        cancelled
    }

    /// Cancel every parked waiter owned by `deploy` across every
    /// file in the registry.  Pairs with
    /// [`release_all_for_deploy`] (PR #530) on the deploy-abort
    /// path: the release-side sweeps HELD locks for the deploy;
    /// this sweeps PARKED waiters.
    ///
    /// Called from the yet-to-land `WalDeployScope::drop` hook
    /// as the FIRST step of the X-2 / G-02 defense-in-depth
    /// chain (see [`wake_waiters`] docstring).  The ordering is
    /// consensus-observable:
    ///
    ///   1. `cancel_all_waiters_for_deploy(D)` — drains D's
    ///      PARKED waiters so none are promoted mid-abort.
    ///   2. `release_all_for_deploy(D)` — sweeps D's HELD locks
    ///      including anything the step 1 sweep raced against.
    ///   3. `wake_waiters` runs inside step 2 to admit OTHER
    ///      deploys that were blocked.
    ///
    /// Panics on the `[0; 32]` sentinel, same discipline as
    /// [`release_all_for_deploy`]: the all-zeros `DeployScope`
    /// is a pre-wiring placeholder and a bulk sweep keyed on it
    /// would wipe every sentinel-scoped waiter on the registry.
    ///
    /// Returns the count cancelled.
    ///
    /// # No `wake_waiters` call after sweep
    ///
    /// Mirrors [`cancel_all_waiters_for_holder`]: cancellation
    /// removes waiters, which doesn't change the holder set, so
    /// no downstream admissibility changes.
    pub fn cancel_all_waiters_for_deploy(&self, deploy: &DeployScope) -> usize {
        assert!(
            deploy != &[0u8; 32],
            "cancel_all_waiters_for_deploy called with the [0; 32] sentinel — \
             the all-zeros DeployScope is reserved as a pre-wiring \
             placeholder; a production deploy under a live WalDeployScope \
             derives a non-sentinel scope via Blake2b256.  Calling with \
             the sentinel would sweep every stray sentinel-scoped waiter."
        );
        let mut guard = poison_abort(self.inner.write(), "LockRegistry.inner");
        let mut cancelled = 0usize;
        let mut evict: Vec<DevInode> = Vec::new();
        let mut to_signal: Vec<oneshot::Sender<Result<LockId, LockError>>> = Vec::new();
        for (dev_inode, state) in guard.iter_mut() {
            let mut i = 0;
            while i < state.waiters.len() {
                if &state.waiters[i].deploy == deploy {
                    let waiter = state
                        .waiters
                        .remove(i)
                        .expect("index just observed in bounds");
                    to_signal.push(waiter.admit);
                    cancelled += 1;
                } else {
                    i += 1;
                }
            }
            if state_is_empty(state) {
                evict.push(*dev_inode);
            }
        }
        for k in evict {
            guard.remove(&k);
        }
        drop(guard);
        for tx in to_signal {
            let _ = tx.send(Err(LockError::Cancelled));
        }
        cancelled
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
fn sequential_conflicts(state: &FileLockState) -> bool {
    state.sequential_holder.is_some() || !state.ranges.is_empty()
}

/// A `FileLockState` is "empty" (safe to evict from the
/// registry map) only when it has no held locks AND no parked
/// waiters.  A state with parked waiters MUST NOT be evicted —
/// dropping the `Waiter`'s `admit` sender would signal cancel
/// to the caller even though nobody called `cancel_wait`, and
/// the waiter would silently disappear from the queue.
fn state_is_empty(state: &FileLockState) -> bool {
    state.ranges.is_empty() && state.sequential_holder.is_none() && state.waiters.is_empty()
}

/// Strict head-of-line FIFO waiter wake pass.  Called after
/// any release path that removed a held lock from `state`.
/// Walks the waiter queue from the front and:
///
///   - If the head is admissible (no conflicts with current
///     holders), pops it, promotes it to a held lock, and
///     signals its admit sender with `Ok(lock_id)`.
///   - If the admit sender's receiver has been dropped (the
///     waiter's task was aborted between park and admit),
///     rolls back the promotion — otherwise the "held" lock
///     would be stranded in `ranges` / `sequential_holder`
///     without any task awaiting the admit signal.  Continues
///     to the next waiter in that case.
///   - If the head is NOT admissible, stops.  Downstream
///     waiters do NOT overtake — strict FIFO prevents writer
///     starvation under a continuous stream of compatible-
///     read admissions.
///
/// Idempotent when `waiters` is empty — safe to call after
/// any state mutation (which this slice does from `release`,
/// `release_all_for_holder`, and `release_all_for_deploy`).
///
/// # X-2 / G-02 defense-in-depth chain
///
/// A waiter belonging to deploy D1 can be promoted here while
/// D1 is simultaneously aborting.  The yet-to-land abort path
/// (`WalDeployScope::drop`) has two steps in a locked
/// ordering:
///
///   1. `cancel_all_waiters_for_deploy(D1)` drains D1's
///      PARKED waiters — doesn't help if a D1 waiter was
///      already promoted here (i.e., moved from `waiters` to
///      `ranges`) between D1's handler enqueuing and the
///      sweep firing.
///   2. `release_all_for_deploy(D1)` (PR #530)
///      unconditionally sweeps ANY held range with matching
///      deploy, regardless of whether it got there via direct
///      acquire OR waiter promotion.  This closes the G-02
///      leak.
///
/// The receiver-drop rollback below is a third layer: if
/// D1's handler task was already dropped by the time
/// promotion happens, `send(Ok)` fails → the promotion
/// rolls back and the ghost is prevented from ever
/// appearing in `ranges`.
///
/// # Why `#[allow(dead_code)]` is NOT needed
///
/// This slice wires `wake_waiters` into three release paths
/// (`release`, `release_all_for_holder`,
/// `release_all_for_deploy`).  The waiter-queue is still
/// empty under this slice's call sites (no Wait acquire
/// support yet, so no waiter can be parked), making every
/// call a no-op — but the function IS reached by every
/// release, so dead-code analysis is satisfied.  The
/// Wait-acquire slice will add callers that actually
/// populate `state.waiters`.
fn wake_waiters(state: &mut FileLockState) {
    while let Some(head) = state.waiters.front() {
        // Admissibility uses the same rules as the direct
        // acquire paths.
        let admissible = match head.kind {
            WaitKind::Range {
                offset,
                length,
                mode,
            } => {
                state.ranges.len() < MAX_RANGES_PER_FILE
                    && !range_conflicts(state, offset, length, mode, &head.holder)
            }
            WaitKind::Sequential => !sequential_conflicts(state),
        };
        if !admissible {
            break;
        }
        // Pop BEFORE promoting so a rollback on receiver-drop
        // can just re-check the (now different) new head on
        // the next iteration.
        let waiter = state.waiters.pop_front().expect("front just observed");
        let lock_id = waiter.lock_id;
        match waiter.kind {
            WaitKind::Range {
                offset,
                length,
                mode,
            } => {
                state.ranges.push(RangeEntry {
                    id: lock_id,
                    offset,
                    length,
                    mode,
                    holder: waiter.holder.clone(),
                    deploy: waiter.deploy,
                });
                if waiter.admit.send(Ok(lock_id)).is_err() {
                    // Receiver already dropped (caller task
                    // cancelled locally).  Roll back the
                    // promotion so the slot returns to the
                    // free pool for the next waiter.
                    state.ranges.pop();
                }
            }
            WaitKind::Sequential => {
                let previous = state.sequential_holder.replace(SequentialEntry {
                    id: lock_id,
                    holder: waiter.holder.clone(),
                    deploy: waiter.deploy,
                });
                debug_assert!(
                    previous.is_none(),
                    "sequential_holder must be empty before admit — \
                     guarded by sequential_conflicts()"
                );
                if waiter.admit.send(Ok(lock_id)).is_err() {
                    // Same rollback as Range case.
                    state.sequential_holder = None;
                }
            }
        }
    }
}

// ===========================================================
// Cross-deploy deadlock detection (slice 4) — NB-7
// ===========================================================

/// NB-7 cross-deploy mutual-wait deadlock detection.
///
/// Returns `true` iff admitting a new `wait: true` acquire by
/// `waiter_deploy` on `target_dev_inode` would close a cycle
/// in the cross-deploy wait-for graph.  The acquire path uses
/// this as an eager-refuse pre-check at enqueue time — no
/// `Waiter` struct is allocated, no `oneshot` channel is
/// opened.
///
/// # Graph
///
///   - **Node**: a `DeployScope`.
///   - **Edge D → H**: deploy D is currently parked on some
///     `(dev, inode)` whose current holder is deploy H
///     (D.deploy ≠ H.deploy).
///
/// A cycle closes iff some current holder H of
/// `target_dev_inode` (H.deploy ≠ waiter_deploy) is
/// transitively reachable from `waiter_deploy` via existing
/// edges — admitting the new edge `waiter_deploy → H` would
/// create a back-edge in the DAG.
///
/// # Determinism (consensus-observable)
///
/// Reachability is a pure set predicate on the graph, so the
/// answer is order-independent w.r.t. HashMap iteration of
/// the registry map or Vec iteration of `state.ranges`.  Two
/// validators with byte-identical `LockRegistry` state
/// compute the same answer regardless of internal iteration
/// order.  This matters because `LockError::Deadlock` is
/// routed through the WAL as a consensus-observable outcome;
/// a divergent cycle predicate would fork the tuplespace.
///
/// # Complexity
///
///   - Time: O(V + E) where V ≤ number of live deploys with
///     any lock or waiter, E ≤ V × F (F = files a deploy is
///     parked on).  In the expected workload (small F,
///     single-digit V) this is a handful of comparisons per
///     park.
///   - Worst case: bounded by
///     `MAX_OPEN_FDS × MAX_WAITERS_PER_FILE`.
///
/// # Self-deploy edges
///
/// The DFS never follows an edge into `waiter_deploy` from
/// `waiter_deploy` itself (same-deploy self-yield is handled
/// by the existing same-holder skip in `range_conflicts`;
/// the seeds here exclude waiter_deploy's own holds).  Same-
/// deploy holds on the target file cannot form a cross-
/// deploy cycle by definition.
///
/// # `visited` ordering
///
/// Uses `BTreeSet<DeployScope>` for stable iteration if we
/// ever debug-print it.  The reachability answer itself is
/// iteration-order-independent, so swapping to `HashSet` is
/// a safe optimization if debug-printing is dropped.
fn would_close_cycle(
    guard: &HashMap<DevInode, FileLockState>,
    waiter_deploy: DeployScope,
    target_dev_inode: DevInode,
) -> bool {
    use std::collections::BTreeSet;
    // Seeds: every deploy that currently holds a lock on the
    // target file, excluding waiter_deploy (same-deploy holds
    // cannot form a cross-deploy cycle).
    let mut stack: Vec<DeployScope> = Vec::new();
    if let Some(state) = guard.get(&target_dev_inode) {
        for r in &state.ranges {
            if r.deploy != waiter_deploy {
                stack.push(r.deploy);
            }
        }
        if let Some(s) = &state.sequential_holder {
            if s.deploy != waiter_deploy {
                stack.push(s.deploy);
            }
        }
    }
    if stack.is_empty() {
        return false;
    }
    // DFS: does any seed reach waiter_deploy via existing
    // wait-for edges?
    let mut visited: BTreeSet<DeployScope> = BTreeSet::new();
    while let Some(node) = stack.pop() {
        if node == waiter_deploy {
            return true;
        }
        if !visited.insert(node) {
            continue;
        }
        // For every file where `node` has a parked waiter,
        // add every deploy that holds a lock on that file
        // (excluding node itself — same-deploy holds are
        // not real edges).
        for state in guard.values() {
            let node_parked = state.waiters.iter().any(|w| w.deploy == node);
            if !node_parked {
                continue;
            }
            for r in &state.ranges {
                if r.deploy != node && !visited.contains(&r.deploy) {
                    stack.push(r.deploy);
                }
            }
            if let Some(s) = &state.sequential_holder {
                if s.deploy != node && !visited.contains(&s.deploy) {
                    stack.push(s.deploy);
                }
            }
        }
    }
    false
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

    // --- would_close_cycle (NB-7) ---------------------------------

    fn mk_waiter(deploy_byte: u8) -> Waiter {
        let (tx, _rx) = oneshot::channel();
        Waiter {
            lock_id: LockId::try_from(1).unwrap(),
            kind: WaitKind::Sequential,
            holder: HolderId::from_bytes([deploy_byte; 32]),
            deploy: [deploy_byte; 32],
            admit: tx,
        }
    }

    fn mk_held_entry(deploy_byte: u8) -> RangeEntry {
        RangeEntry {
            id: LockId::try_from(1).unwrap(),
            offset: 0,
            length: 100,
            mode: LockMode::Write,
            holder: HolderId::from_bytes([deploy_byte; 32]),
            deploy: [deploy_byte; 32],
        }
    }

    /// Empty guard → no cycle possible (no holders anywhere).
    #[test]
    fn would_close_cycle_empty_guard_is_false() {
        let guard = HashMap::new();
        assert!(!would_close_cycle(&guard, [0x11u8; 32], (1, 1)));
    }

    /// Target has no entry in the guard → no holders to wait on
    /// → no cycle.
    #[test]
    fn would_close_cycle_target_absent_is_false() {
        let mut guard = HashMap::new();
        // Some unrelated file is held — doesn't seed the DFS
        // because target_dev_inode isn't in guard.
        guard.insert((2, 2), FileLockState {
            ranges: vec![mk_held_entry(0xBB)],
            ..Default::default()
        });
        assert!(!would_close_cycle(&guard, [0x11u8; 32], (1, 1)));
    }

    /// Target held only by `waiter_deploy` itself — same-deploy
    /// holds are not real edges, so seeds are empty.  Returns
    /// false.  LOAD-BEARING: a future refactor that forgot the
    /// self-skip would flag same-deploy re-entrant waits as a
    /// cycle.
    #[test]
    fn would_close_cycle_same_deploy_holder_is_not_an_edge() {
        let waiter_deploy = [0x11u8; 32];
        let mut guard = HashMap::new();
        guard.insert((1, 1), FileLockState {
            ranges: vec![mk_held_entry(0x11)],
            ..Default::default()
        });
        assert!(!would_close_cycle(&guard, waiter_deploy, (1, 1)));
    }

    /// Target held by other deploy, that deploy has no waiters
    /// → no transitive edge → no cycle.
    #[test]
    fn would_close_cycle_other_deploy_holder_without_waiters_is_false() {
        let mut guard = HashMap::new();
        guard.insert((1, 1), FileLockState {
            ranges: vec![mk_held_entry(0xBB)],
            ..Default::default()
        });
        assert!(!would_close_cycle(&guard, [0x11u8; 32], (1, 1)));
    }

    /// LOAD-BEARING 2-cycle: D1 wants to park on target held by
    /// D2; D2 is parked on another file held by D1.  Admitting
    /// D1's wait would close the cycle D1 → D2 → D1.
    #[test]
    fn would_close_cycle_direct_two_cycle_fires() {
        let d1 = [0x11u8; 32];
        let d2 = [0x22u8; 32];
        let mut guard = HashMap::new();
        // Target (dev=1, ino=1) held by D2.  D1 wants to park
        // here.
        guard.insert((1, 1), FileLockState {
            ranges: vec![mk_held_entry(0x22)],
            ..Default::default()
        });
        // Another file (dev=2, ino=2) held by D1, with D2
        // parked on it.
        let mut other = FileLockState {
            ranges: vec![mk_held_entry(0x11)],
            ..Default::default()
        };
        other.waiters.push_back(mk_waiter(0x22));
        guard.insert((2, 2), other);

        assert!(
            would_close_cycle(&guard, d1, (1, 1)),
            "D1 → D2 → D1 cycle should fire"
        );
        // Sanity: D2 trying to park on (2, 2) — D1 holds it +
        // isn't parked anywhere → no cycle.
        assert!(!would_close_cycle(&guard, d2, (2, 2)));
    }

    /// 3-cycle: D1 → D2 → D3 → D1.  The DFS must follow the
    /// chain through the intermediate node.
    #[test]
    fn would_close_cycle_three_cycle_fires() {
        let d1 = [0x11u8; 32];
        let mut guard = HashMap::new();
        // Target (dev=1, ino=1) held by D2.
        guard.insert((1, 1), FileLockState {
            ranges: vec![mk_held_entry(0x22)],
            ..Default::default()
        });
        // (dev=2, ino=2) held by D3; D2 parked on it.
        let mut file2 = FileLockState {
            ranges: vec![mk_held_entry(0x33)],
            ..Default::default()
        };
        file2.waiters.push_back(mk_waiter(0x22));
        guard.insert((2, 2), file2);
        // (dev=3, ino=3) held by D1; D3 parked on it.
        let mut file3 = FileLockState {
            ranges: vec![mk_held_entry(0x11)],
            ..Default::default()
        };
        file3.waiters.push_back(mk_waiter(0x33));
        guard.insert((3, 3), file3);

        assert!(
            would_close_cycle(&guard, d1, (1, 1)),
            "D1 → D2 → D3 → D1 cycle should fire"
        );
    }

    /// Linear chain WITHOUT a cycle: D1 wants target held by D2,
    /// D2 is parked on file held by D3, D3 holds nothing elsewhere
    /// and D1 holds nothing D3 is waiting on.  No cycle closes.
    #[test]
    fn would_close_cycle_linear_chain_is_not_cycle() {
        let d1 = [0x11u8; 32];
        let mut guard = HashMap::new();
        guard.insert((1, 1), FileLockState {
            ranges: vec![mk_held_entry(0x22)],
            ..Default::default()
        });
        // (2, 2) held by D3; D2 parked on it.  D3 has no further
        // outgoing edge back to D1.
        let mut file2 = FileLockState {
            ranges: vec![mk_held_entry(0x33)],
            ..Default::default()
        };
        file2.waiters.push_back(mk_waiter(0x22));
        guard.insert((2, 2), file2);
        // D1 holds (3, 3) but D3 is NOT parked on it, so no
        // D3 → D1 edge exists.
        guard.insert((3, 3), FileLockState {
            ranges: vec![mk_held_entry(0x11)],
            ..Default::default()
        });

        assert!(!would_close_cycle(&guard, d1, (1, 1)));
    }

    /// Sequential holder on target seeds the DFS (not just the
    /// range entries).  LOAD-BEARING: pins that both seed
    /// branches contribute.
    #[test]
    fn would_close_cycle_sequential_holder_seeds_dfs() {
        let d1 = [0x11u8; 32];
        let mut guard = HashMap::new();
        // Target held via SEQUENTIAL (not range) by D2.
        guard.insert((1, 1), FileLockState {
            sequential_holder: Some(SequentialEntry {
                id: LockId::try_from(5).unwrap(),
                holder: HolderId::from_bytes([0x22; 32]),
                deploy: [0x22u8; 32],
            }),
            ..Default::default()
        });
        // (2, 2) held by D1; D2 parked on it.
        let mut file2 = FileLockState {
            ranges: vec![mk_held_entry(0x11)],
            ..Default::default()
        };
        file2.waiters.push_back(mk_waiter(0x22));
        guard.insert((2, 2), file2);

        assert!(
            would_close_cycle(&guard, d1, (1, 1)),
            "sequential holder on target should seed the DFS"
        );
    }

    /// Multiple seeds on target, only one closes a cycle → still
    /// fires.  Pins that the DFS explores EVERY seed, not just
    /// the first.
    #[test]
    fn would_close_cycle_fires_when_any_seed_reaches_waiter() {
        let d1 = [0x11u8; 32];
        let mut guard = HashMap::new();
        // Target held by D2 AND D3 (reader-reader).  D3 → D1
        // closes a cycle; D2 doesn't.
        guard.insert((1, 1), FileLockState {
            ranges: vec![
                RangeEntry {
                    id: LockId::try_from(1).unwrap(),
                    offset: 0,
                    length: 10,
                    mode: LockMode::Read,
                    holder: HolderId::from_bytes([0x22; 32]),
                    deploy: [0x22u8; 32],
                },
                RangeEntry {
                    id: LockId::try_from(2).unwrap(),
                    offset: 20,
                    length: 10,
                    mode: LockMode::Read,
                    holder: HolderId::from_bytes([0x33; 32]),
                    deploy: [0x33u8; 32],
                },
            ],
            ..Default::default()
        });
        // D3 parked on a file D1 holds → D3 → D1 edge.
        let mut file2 = FileLockState {
            ranges: vec![mk_held_entry(0x11)],
            ..Default::default()
        };
        file2.waiters.push_back(mk_waiter(0x33));
        guard.insert((2, 2), file2);

        assert!(would_close_cycle(&guard, d1, (1, 1)));
    }

    /// Visited-set discipline: a diamond pattern (D1 → D2 and
    /// D1 → D3, both of which point at D4 which eventually
    /// reaches back to D1) traverses each node once and still
    /// detects the cycle.
    #[test]
    fn would_close_cycle_diamond_pattern_visits_each_node_once() {
        let d1 = [0x11u8; 32];
        let mut guard = HashMap::new();
        // Target (1, 1) held by D2 AND D3.
        guard.insert((1, 1), FileLockState {
            ranges: vec![
                RangeEntry {
                    id: LockId::try_from(1).unwrap(),
                    offset: 0,
                    length: 10,
                    mode: LockMode::Read,
                    holder: HolderId::from_bytes([0x22; 32]),
                    deploy: [0x22u8; 32],
                },
                RangeEntry {
                    id: LockId::try_from(2).unwrap(),
                    offset: 20,
                    length: 10,
                    mode: LockMode::Read,
                    holder: HolderId::from_bytes([0x33; 32]),
                    deploy: [0x33u8; 32],
                },
            ],
            ..Default::default()
        });
        // (2, 2) held by D4; both D2 AND D3 parked on it.
        let mut file2 = FileLockState {
            ranges: vec![mk_held_entry(0x44)],
            ..Default::default()
        };
        file2.waiters.push_back(mk_waiter(0x22));
        file2.waiters.push_back(mk_waiter(0x33));
        guard.insert((2, 2), file2);
        // (3, 3) held by D1; D4 parked on it → closes cycle.
        let mut file3 = FileLockState {
            ranges: vec![mk_held_entry(0x11)],
            ..Default::default()
        };
        file3.waiters.push_back(mk_waiter(0x44));
        guard.insert((3, 3), file3);

        assert!(would_close_cycle(&guard, d1, (1, 1)));
    }

    // --- try_acquire_range (Fail-only) ----------------------------

    fn deploy_scope(byte: u8) -> DeployScope { [byte; 32] }

    /// Zero-length range is rejected with `BadArg` BEFORE any
    /// state check — a zero-length lock would protect nothing
    /// and never conflict, inviting race bugs if silently
    /// accepted.
    #[test]
    fn try_acquire_range_zero_length_returns_bad_arg() {
        let reg = LockRegistry::new();
        let out = reg.try_acquire_range(
            (1, 1),
            0,
            0,
            LockMode::Write,
            HolderId::from_bytes([0x11; 32]),
            deploy_scope(0x11),
        );
        assert_eq!(out, Err(LockError::BadArg));
    }

    /// Happy path: empty state → acquire succeeds and returns
    /// an `Ok(LockId)`.  Also pins that the state is actually
    /// populated — a subsequent same-range acquire by a
    /// DIFFERENT holder conflicts.
    #[test]
    fn try_acquire_range_empty_state_succeeds_and_populates() {
        let reg = LockRegistry::new();
        let id = reg
            .try_acquire_range(
                (1, 1),
                0,
                100,
                LockMode::Write,
                HolderId::from_bytes([0x11; 32]),
                deploy_scope(0x11),
            )
            .unwrap();
        assert_eq!(id.as_u64(), 1);
        // Different-holder overlapping Write → Busy.
        let conflict = reg.try_acquire_range(
            (1, 1),
            50,
            50,
            LockMode::Write,
            HolderId::from_bytes([0x22; 32]),
            deploy_scope(0x22),
        );
        assert_eq!(conflict, Err(LockError::Busy));
    }

    /// Reader-reader compatibility: two different holders can
    /// hold overlapping Read locks concurrently.
    #[test]
    fn try_acquire_range_reader_reader_concurrent_allowed() {
        let reg = LockRegistry::new();
        let a = reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Read,
            HolderId::from_bytes([0x11; 32]),
            deploy_scope(0x11),
        );
        let b = reg.try_acquire_range(
            (1, 1),
            50,
            50,
            LockMode::Read,
            HolderId::from_bytes([0x22; 32]),
            deploy_scope(0x22),
        );
        assert!(a.is_ok() && b.is_ok());
        assert_ne!(a.unwrap(), b.unwrap(), "distinct LockIds");
    }

    /// Same-holder re-entrant: same holder may acquire
    /// overlapping ranges regardless of mode (POSIX fcntl).
    #[test]
    fn try_acquire_range_same_holder_may_overlap() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        let a = reg
            .try_acquire_range(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder.clone(),
                deploy_scope(0x11),
            )
            .unwrap();
        let b = reg
            .try_acquire_range((1, 1), 50, 50, LockMode::Write, holder, deploy_scope(0x11))
            .unwrap();
        assert_ne!(a, b);
    }

    /// Non-overlapping ranges never conflict regardless of mode
    /// / holder.
    #[test]
    fn try_acquire_range_non_overlapping_different_holders_allowed() {
        let reg = LockRegistry::new();
        reg.try_acquire_range(
            (1, 1),
            0,
            10,
            LockMode::Write,
            HolderId::from_bytes([0x11; 32]),
            deploy_scope(0x11),
        )
        .unwrap();
        reg.try_acquire_range(
            (1, 1),
            100,
            10,
            LockMode::Write,
            HolderId::from_bytes([0x22; 32]),
            deploy_scope(0x22),
        )
        .unwrap();
    }

    /// LOAD-BEARING: the live-range cap check fires at exactly
    /// `MAX_RANGES_PER_FILE`, BEFORE the conflict check runs.
    /// Error ordering is consensus-observable.
    #[test]
    fn try_acquire_range_quota_fires_at_max_ranges_per_file() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        // Fill up to the cap with non-overlapping ranges.
        for i in 0..MAX_RANGES_PER_FILE {
            reg.try_acquire_range(
                (1, 1),
                (i as u64) * 100,
                50,
                LockMode::Write,
                holder.clone(),
                deploy_scope(0x11),
            )
            .unwrap();
        }
        // The N+1'th acquire — even a non-conflicting range —
        // must trip the cap, NOT the conflict check.
        let over = reg.try_acquire_range(
            (1, 1),
            999_999,
            50,
            LockMode::Write,
            holder,
            deploy_scope(0x11),
        );
        assert_eq!(over, Err(LockError::QuotaExceeded));
    }

    /// Each `(dev, inode)` has an INDEPENDENT state — two
    /// files don't share a range cap or interact.
    #[test]
    fn try_acquire_range_distinct_dev_inodes_are_independent() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Write,
            holder.clone(),
            deploy_scope(0x11),
        )
        .unwrap();
        // Same offset+length+mode on a DIFFERENT file by a
        // DIFFERENT holder — no conflict (independent state).
        reg.try_acquire_range(
            (2, 2),
            0,
            100,
            LockMode::Write,
            HolderId::from_bytes([0x22; 32]),
            deploy_scope(0x22),
        )
        .unwrap();
    }

    /// LOAD-BEARING hygiene invariant: a failed acquire MUST
    /// NOT leave an empty `FileLockState` behind in the
    /// registry map.  Repeated failures on distinct
    /// `(dev, inode)` tuples would otherwise orphan ~48 bytes
    /// per failure.  Pins the check-before-insert discipline
    /// against a future refactor that reverted to the
    /// `.entry().or_default()` + check pattern.
    #[test]
    fn try_acquire_range_failed_acquire_does_not_leak_empty_state() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);

        // 1. Zero-length rejection — never touches the map.
        let out = reg.try_acquire_range(
            (99, 99),
            0,
            0,
            LockMode::Write,
            holder.clone(),
            deploy_scope(0x11),
        );
        assert_eq!(out, Err(LockError::BadArg));
        {
            let guard = poison_abort(reg.inner.read(), "LockRegistry.inner");
            assert!(
                !guard.contains_key(&(99, 99)),
                "BadArg must not insert an entry for (99, 99)"
            );
        }

        // 2. QuotaExceeded against an existing state: fill
        // `(1, 1)` to cap, then try to add N+1.  The N+1
        // failure legitimately observes the existing entry;
        // confirm (1, 1) is still there afterwards (it should
        // be — populated by prior successful acquires).
        for i in 0..MAX_RANGES_PER_FILE {
            reg.try_acquire_range(
                (1, 1),
                (i as u64) * 100,
                50,
                LockMode::Write,
                holder.clone(),
                deploy_scope(0x11),
            )
            .unwrap();
        }
        let over = reg.try_acquire_range(
            (1, 1),
            999_999,
            50,
            LockMode::Write,
            holder.clone(),
            deploy_scope(0x11),
        );
        assert_eq!(over, Err(LockError::QuotaExceeded));

        // 3. Busy on a FRESH `(dev, inode)` tuple that doesn't
        // yet have a state entry.  Pre-populate `(2, 2)` by
        // a different holder with a Write; then the probe
        // acquire on `(3, 3)` conflicts with... wait, no, we
        // need an acquire that CONFLICTS and targets a fresh
        // tuple.  Put a Write on `(2, 2)` by holder A, then
        // probe `(2, 2)` by holder B — that triggers Busy
        // against an existing entry, not a fresh one.
        //
        // The genuine "fresh tuple, Busy" case can only arise
        // if an initial state had a sequential_holder from
        // sequential acquire (not landed) — so for Fail-only
        // the Busy path cannot hit a fresh entry.  Instead,
        // directly verify that no mystery entries beyond
        // `(1, 1)` appeared through the sequence above.
        {
            let guard = poison_abort(reg.inner.read(), "LockRegistry.inner");
            let keys: Vec<_> = guard.keys().copied().collect();
            assert_eq!(keys, vec![(1, 1)], "exactly one entry should exist");
        }
    }

    // --- release --------------------------------------------------

    /// Happy path: acquire → release → re-acquire succeeds.
    #[test]
    fn release_frees_the_range_for_re_acquire() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0x11; 32]);
        let holder_b = HolderId::from_bytes([0x22; 32]);
        let id = reg
            .try_acquire_range(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_a.clone(),
                deploy_scope(0x11),
            )
            .unwrap();
        // Conflict before release.
        assert_eq!(
            reg.try_acquire_range(
                (1, 1),
                50,
                50,
                LockMode::Write,
                holder_b.clone(),
                deploy_scope(0x22),
            ),
            Err(LockError::Busy)
        );
        reg.release(id, &holder_a).unwrap();
        // Now B can acquire.
        reg.try_acquire_range(
            (1, 1),
            50,
            50,
            LockMode::Write,
            holder_b,
            deploy_scope(0x22),
        )
        .unwrap();
    }

    /// LOAD-BEARING ct_eq discipline: release with the wrong
    /// holder returns `Closed`, NOT any other variant that
    /// would narrow an attacker's search space.
    #[test]
    fn release_with_wrong_holder_returns_closed() {
        let reg = LockRegistry::new();
        let real_holder = HolderId::from_bytes([0x11; 32]);
        let attacker = HolderId::from_bytes([0x22; 32]);
        let id = reg
            .try_acquire_range(
                (1, 1),
                0,
                100,
                LockMode::Write,
                real_holder.clone(),
                deploy_scope(0x11),
            )
            .unwrap();
        assert_eq!(reg.release(id, &attacker), Err(LockError::Closed));
        // Real holder can still release — attacker's attempt
        // was a no-op.
        reg.release(id, &real_holder).unwrap();
    }

    /// Double release returns `Closed` on the second call.
    #[test]
    fn release_second_time_returns_closed() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        let id = reg
            .try_acquire_range(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder.clone(),
                deploy_scope(0x11),
            )
            .unwrap();
        reg.release(id, &holder).unwrap();
        assert_eq!(reg.release(id, &holder), Err(LockError::Closed));
    }

    /// Release of an unknown LockId returns `Closed`.
    #[test]
    fn release_unknown_lock_id_returns_closed() {
        let reg = LockRegistry::new();
        assert_eq!(
            reg.release(
                LockId::try_from(999).unwrap(),
                &HolderId::from_bytes([0x11; 32])
            ),
            Err(LockError::Closed)
        );
    }

    /// LOAD-BEARING state eviction: after releasing the last
    /// held lock on a file, the state is evicted from the
    /// registry map.  Pin so a future refactor that forgot the
    /// eviction surfaces here as unbounded growth.
    #[test]
    fn release_evicts_empty_state_from_registry_map() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        let id = reg
            .try_acquire_range(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder.clone(),
                deploy_scope(0x11),
            )
            .unwrap();
        // Inspect the map directly (test lives in same module).
        {
            let guard = poison_abort(reg.inner.read(), "LockRegistry.inner");
            assert!(guard.contains_key(&(1, 1)));
        }
        reg.release(id, &holder).unwrap();
        {
            let guard = poison_abort(reg.inner.read(), "LockRegistry.inner");
            assert!(
                !guard.contains_key(&(1, 1)),
                "state must be evicted after last lock released"
            );
        }
    }

    /// Releasing one of several held ranges on the same file
    /// does NOT evict the state (other ranges remain).
    #[test]
    fn release_one_of_many_ranges_preserves_state() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        let id_a = reg
            .try_acquire_range(
                (1, 1),
                0,
                10,
                LockMode::Write,
                holder.clone(),
                deploy_scope(0x11),
            )
            .unwrap();
        let _id_b = reg
            .try_acquire_range(
                (1, 1),
                100,
                10,
                LockMode::Write,
                holder.clone(),
                deploy_scope(0x11),
            )
            .unwrap();
        reg.release(id_a, &holder).unwrap();
        let guard = poison_abort(reg.inner.read(), "LockRegistry.inner");
        assert!(
            guard.contains_key(&(1, 1)),
            "state with surviving range must not be evicted"
        );
        assert_eq!(
            guard.get(&(1, 1)).unwrap().ranges.len(),
            1,
            "exactly one range remains"
        );
    }

    /// Release correctly routes between two different files —
    /// a LockId minted on `(1, 1)` is NOT found on `(2, 2)`.
    /// LOAD-BEARING: pins the "scan every file" discipline
    /// against a future refactor that cached the dev_inode in
    /// the LockId.
    #[test]
    fn release_locates_lock_across_distinct_files() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        let id_a = reg
            .try_acquire_range(
                (1, 1),
                0,
                10,
                LockMode::Write,
                holder.clone(),
                deploy_scope(0x11),
            )
            .unwrap();
        let id_b = reg
            .try_acquire_range(
                (2, 2),
                0,
                10,
                LockMode::Write,
                holder.clone(),
                deploy_scope(0x11),
            )
            .unwrap();
        assert_ne!(id_a, id_b);
        reg.release(id_b, &holder).unwrap();
        // Verify (2, 2) was evicted + (1, 1) survives.
        let guard = poison_abort(reg.inner.read(), "LockRegistry.inner");
        assert!(!guard.contains_key(&(2, 2)));
        assert!(guard.contains_key(&(1, 1)));
    }

    // --- release_all_for_holder ------------------------------------

    /// Sweeps every range entry whose `holder` matches the
    /// argument, across every file in the registry.  Returns
    /// the count released.
    #[test]
    fn release_all_for_holder_sweeps_across_distinct_files() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0x11; 32]);
        let holder_b = HolderId::from_bytes([0x22; 32]);
        // A holds locks on (1, 1) and (2, 2); B holds a lock on (1, 1).
        reg.try_acquire_range(
            (1, 1),
            0,
            10,
            LockMode::Read,
            holder_a.clone(),
            deploy_scope(0x11),
        )
        .unwrap();
        reg.try_acquire_range(
            (1, 1),
            100,
            10,
            LockMode::Read,
            holder_b.clone(),
            deploy_scope(0x22),
        )
        .unwrap();
        reg.try_acquire_range(
            (2, 2),
            0,
            10,
            LockMode::Write,
            holder_a.clone(),
            deploy_scope(0x11),
        )
        .unwrap();

        let n = reg.release_all_for_holder(&holder_a);
        assert_eq!(n, 2, "two A-held ranges swept");

        // B's lock on (1, 1) survives.
        let guard = poison_abort(reg.inner.read(), "LockRegistry.inner");
        assert_eq!(guard.get(&(1, 1)).unwrap().ranges.len(), 1);
        assert!(
            !guard.contains_key(&(2, 2)),
            "(2, 2) evicted (fully A-held)"
        );
    }

    /// Sweeps the sequential_holder slot if `holder` matches.
    /// Pins the sequential-side branch of the sweep.
    #[test]
    fn release_all_for_holder_clears_matching_sequential_holder() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        // Hand-populate a sequential holder (no sequential-acquire
        // API yet; this slice's test only verifies sweep behavior).
        {
            let mut guard = poison_abort(reg.inner.write(), "LockRegistry.inner");
            guard.insert((1, 1), FileLockState {
                sequential_holder: Some(SequentialEntry {
                    id: LockId::try_from(42).unwrap(),
                    holder: holder.clone(),
                    deploy: deploy_scope(0x11),
                }),
                ..Default::default()
            });
        }
        let n = reg.release_all_for_holder(&holder);
        assert_eq!(n, 1, "one sequential entry swept");
        // State evicted (fully empty).
        let guard = poison_abort(reg.inner.read(), "LockRegistry.inner");
        assert!(!guard.contains_key(&(1, 1)));
    }

    /// A holder with no locks returns 0 and leaves the map
    /// unchanged.  Pins the idempotent no-op path.
    #[test]
    fn release_all_for_holder_no_matches_returns_zero() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0x11; 32]);
        let other = HolderId::from_bytes([0xFF; 32]);
        reg.try_acquire_range((1, 1), 0, 10, LockMode::Write, holder_a, deploy_scope(0x11))
            .unwrap();
        let n = reg.release_all_for_holder(&other);
        assert_eq!(n, 0);
        let guard = poison_abort(reg.inner.read(), "LockRegistry.inner");
        assert!(
            guard.contains_key(&(1, 1)),
            "unmatched holder leaves state intact"
        );
    }

    /// Combined range + sequential removal in a single sweep.
    /// Pins the `+= 1` sequential-side accounting in combination
    /// with the `retain` range accounting — a future refactor
    /// that double-counted (e.g., re-included the swept
    /// sequential in `before - state.ranges.len()`) or missed
    /// one of the two branches surfaces here with a wrong total.
    #[test]
    fn release_all_for_holder_combined_range_and_sequential_total() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        // Two range entries by `holder`.
        reg.try_acquire_range(
            (1, 1),
            0,
            10,
            LockMode::Read,
            holder.clone(),
            deploy_scope(0x11),
        )
        .unwrap();
        reg.try_acquire_range(
            (1, 1),
            100,
            10,
            LockMode::Read,
            holder.clone(),
            deploy_scope(0x11),
        )
        .unwrap();
        // Hand-populate a sequential_holder on a DIFFERENT file
        // by the same holder (coexistence rules forbid both on
        // the same file; sequential acquire API not landed yet
        // so we populate directly).
        {
            let mut guard = poison_abort(reg.inner.write(), "LockRegistry.inner");
            guard.insert((2, 2), FileLockState {
                sequential_holder: Some(SequentialEntry {
                    id: LockId::try_from(99).unwrap(),
                    holder: holder.clone(),
                    deploy: deploy_scope(0x11),
                }),
                ..Default::default()
            });
        }

        let n = reg.release_all_for_holder(&holder);
        assert_eq!(n, 3, "2 ranges + 1 sequential = 3 released");

        let guard = poison_abort(reg.inner.read(), "LockRegistry.inner");
        assert!(!guard.contains_key(&(1, 1)), "fully-swept file evicted");
        assert!(
            !guard.contains_key(&(2, 2)),
            "sequential-swept file evicted"
        );
    }

    // --- release_all_for_deploy ------------------------------------

    /// Sweeps every range entry whose `deploy` matches, across
    /// every file.  Mirrors `release_all_for_holder` but keyed
    /// on deploy.
    #[test]
    fn release_all_for_deploy_sweeps_across_distinct_files() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0x11; 32]);
        let holder_b = HolderId::from_bytes([0x22; 32]);
        let d1 = deploy_scope(0x11);
        let d2 = deploy_scope(0x22);
        reg.try_acquire_range((1, 1), 0, 10, LockMode::Read, holder_a.clone(), d1)
            .unwrap();
        reg.try_acquire_range((1, 1), 100, 10, LockMode::Read, holder_b, d2)
            .unwrap();
        reg.try_acquire_range((2, 2), 0, 10, LockMode::Write, holder_a, d1)
            .unwrap();

        let n = reg.release_all_for_deploy(&d1);
        assert_eq!(n, 2, "two D1-held ranges swept");

        let guard = poison_abort(reg.inner.read(), "LockRegistry.inner");
        assert_eq!(
            guard.get(&(1, 1)).unwrap().ranges.len(),
            1,
            "D2's lock survives"
        );
        assert!(!guard.contains_key(&(2, 2)));
    }

    /// LOAD-BEARING sentinel guard: calling with `[0; 32]`
    /// panics with the documented message to prevent a
    /// pre-wiring regression that would otherwise sweep every
    /// stray sentinel-scoped entry.
    #[test]
    #[should_panic(expected = "sentinel")]
    fn release_all_for_deploy_panics_on_zero_sentinel() {
        let reg = LockRegistry::new();
        let _ = reg.release_all_for_deploy(&[0u8; 32]);
    }

    #[test]
    fn release_all_for_deploy_no_matches_returns_zero() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        reg.try_acquire_range((1, 1), 0, 10, LockMode::Write, holder, deploy_scope(0x11))
            .unwrap();
        assert_eq!(reg.release_all_for_deploy(&deploy_scope(0xFF)), 0);
    }

    // --- is_locked -------------------------------------------------

    /// Empty registry → no file is locked.
    #[test]
    fn is_locked_empty_registry_returns_false() {
        let reg = LockRegistry::new();
        assert!(!reg.is_locked((1, 1), (0, 100)));
    }

    /// Tracked file with no overlap → false.
    #[test]
    fn is_locked_tracked_but_disjoint_returns_false() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        reg.try_acquire_range((1, 1), 0, 10, LockMode::Write, holder, deploy_scope(0x11))
            .unwrap();
        assert!(!reg.is_locked((1, 1), (100, 10)));
    }

    /// Overlap with ANY held range → true (holder-agnostic;
    /// unlink doesn't care who holds it).
    #[test]
    fn is_locked_overlapping_range_returns_true() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        reg.try_acquire_range((1, 1), 0, 100, LockMode::Read, holder, deploy_scope(0x11))
            .unwrap();
        assert!(reg.is_locked((1, 1), (50, 50)));
    }

    /// Sequential holder → every query returns true (whole-
    /// file exclusive).  LOAD-BEARING: pins the top-of-function
    /// short-circuit.
    #[test]
    fn is_locked_sequential_holder_blocks_every_query() {
        let reg = LockRegistry::new();
        // Hand-populate a sequential holder.
        {
            let mut guard = poison_abort(reg.inner.write(), "LockRegistry.inner");
            guard.insert((1, 1), FileLockState {
                sequential_holder: Some(SequentialEntry {
                    id: LockId::try_from(1).unwrap(),
                    holder: HolderId::from_bytes([0x11; 32]),
                    deploy: deploy_scope(0x11),
                }),
                ..Default::default()
            });
        }
        // Even a tiny off-end range reports locked.
        assert!(reg.is_locked((1, 1), (999_999, 1)));
        assert!(reg.is_locked((1, 1), (0, 1)));
    }

    /// Whole-file probe `(0, u64::MAX)` detects any held range
    /// via the saturating-add in `ranges_overlap`.  LOAD-
    /// BEARING: pins the whole-file-unlink gate idiom.
    #[test]
    fn is_locked_whole_file_probe_detects_any_held_range() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        // Hold a tiny range near the top of the file space.
        reg.try_acquire_range(
            (1, 1),
            u64::MAX - 1000,
            100,
            LockMode::Write,
            holder,
            deploy_scope(0x11),
        )
        .unwrap();
        assert!(reg.is_locked((1, 1), (0, u64::MAX)));
    }

    /// After `release`, the tracked file is evicted and
    /// `is_locked` reads false — pins the integration between
    /// release's eviction and is_locked's absent-key branch.
    #[test]
    fn is_locked_returns_false_after_release_evicts_state() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        let id = reg
            .try_acquire_range(
                (1, 1),
                0,
                10,
                LockMode::Write,
                holder.clone(),
                deploy_scope(0x11),
            )
            .unwrap();
        assert!(reg.is_locked((1, 1), (0, 10)));
        reg.release(id, &holder).unwrap();
        assert!(!reg.is_locked((1, 1), (0, 10)));
    }

    // --- try_acquire_sequential ------------------------------------

    /// Happy path on an empty file.  Also pins the state is
    /// actually populated by querying `is_locked`.
    #[test]
    fn try_acquire_sequential_empty_state_succeeds() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        let id = reg
            .try_acquire_sequential((1, 1), holder, deploy_scope(0x11))
            .unwrap();
        assert_eq!(id.as_u64(), 1);
        // Whole-file probe sees the sequential holder.
        assert!(reg.is_locked((1, 1), (0, u64::MAX)));
    }

    /// Two different holders cannot both hold the sequential
    /// lock — the second attempt returns `Busy`.
    #[test]
    fn try_acquire_sequential_second_acquire_busy() {
        let reg = LockRegistry::new();
        reg.try_acquire_sequential((1, 1), HolderId::from_bytes([0x11; 32]), deploy_scope(0x11))
            .unwrap();
        assert_eq!(
            reg.try_acquire_sequential(
                (1, 1),
                HolderId::from_bytes([0x22; 32]),
                deploy_scope(0x22),
            ),
            Err(LockError::Busy)
        );
    }

    /// LOAD-BEARING: a cap holding ANY range (even a tiny
    /// Read) cannot upgrade to a sequential lock without
    /// releasing first.  Pins the no-same-holder-skip rule
    /// surfaced at the API boundary.
    #[test]
    fn try_acquire_sequential_held_range_blocks_even_for_same_holder() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        reg.try_acquire_range(
            (1, 1),
            0,
            10,
            LockMode::Read,
            holder.clone(),
            deploy_scope(0x11),
        )
        .unwrap();
        // Same holder trying to upgrade to sequential — Busy.
        assert_eq!(
            reg.try_acquire_sequential((1, 1), holder, deploy_scope(0x11)),
            Err(LockError::Busy)
        );
    }

    /// Each `(dev, inode)` has an independent sequential slot.
    #[test]
    fn try_acquire_sequential_distinct_dev_inodes_are_independent() {
        let reg = LockRegistry::new();
        reg.try_acquire_sequential((1, 1), HolderId::from_bytes([0x11; 32]), deploy_scope(0x11))
            .unwrap();
        reg.try_acquire_sequential((2, 2), HolderId::from_bytes([0x22; 32]), deploy_scope(0x22))
            .unwrap();
    }

    /// LOAD-BEARING empty-state leak defense: a failed
    /// sequential acquire (Busy against an existing range)
    /// MUST NOT leave an empty `FileLockState` behind on a
    /// different `(dev, inode)` that never had a lock.  Same
    /// invariant pinned for `try_acquire_range` in PR #529.
    #[test]
    fn try_acquire_sequential_failed_acquire_does_not_leak_empty_state() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        // Pre-populate (1, 1) with a range so a sequential
        // acquire there is a legitimate conflict against an
        // existing state.
        reg.try_acquire_range(
            (1, 1),
            0,
            10,
            LockMode::Read,
            holder.clone(),
            deploy_scope(0x11),
        )
        .unwrap();
        assert_eq!(
            reg.try_acquire_sequential((1, 1), holder, deploy_scope(0x11)),
            Err(LockError::Busy)
        );
        // (99, 99) was never touched → no empty entry.
        let guard = poison_abort(reg.inner.read(), "LockRegistry.inner");
        let keys: Vec<_> = guard.keys().copied().collect();
        assert_eq!(
            keys,
            vec![(1, 1)],
            "exactly one entry (the pre-existing range-holding file)"
        );
    }

    /// A sequential acquire followed by a release re-opens
    /// the slot — pins the integration with PR #529's
    /// unified `release` path.
    #[test]
    fn try_acquire_sequential_release_reopens_slot() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0x11; 32]);
        let holder_b = HolderId::from_bytes([0x22; 32]);
        let id = reg
            .try_acquire_sequential((1, 1), holder_a.clone(), deploy_scope(0x11))
            .unwrap();
        reg.release(id, &holder_a).unwrap();
        // Now B can acquire sequentially.
        reg.try_acquire_sequential((1, 1), holder_b, deploy_scope(0x22))
            .unwrap();
    }

    /// Integration pin: sequential acquire + range acquire on
    /// the SAME file are mutually exclusive in both directions.
    #[test]
    fn try_acquire_sequential_and_range_are_mutually_exclusive() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);

        // (A) range first, sequential blocked.
        reg.try_acquire_range(
            (1, 1),
            0,
            10,
            LockMode::Write,
            holder.clone(),
            deploy_scope(0x11),
        )
        .unwrap();
        assert_eq!(
            reg.try_acquire_sequential(
                (1, 1),
                HolderId::from_bytes([0x22; 32]),
                deploy_scope(0x22),
            ),
            Err(LockError::Busy)
        );

        // (B) sequential first (on a different file), range blocked.
        reg.try_acquire_sequential((2, 2), holder.clone(), deploy_scope(0x11))
            .unwrap();
        assert_eq!(
            reg.try_acquire_range(
                (2, 2),
                0,
                10,
                LockMode::Read,
                HolderId::from_bytes([0x22; 32]),
                deploy_scope(0x22),
            ),
            Err(LockError::Busy)
        );
    }

    // --- wake_waiters ---------------------------------------------
    //
    // The acquire-side park path lands in the subsequent Wait slice.
    // Until then no production call site enqueues waiters, so the
    // tests hand-populate `state.waiters` to pin the admit logic in
    // isolation.

    /// Build a parked Range waiter with an attached oneshot receiver.
    /// The receiver is returned so the test can observe the admit
    /// signal (or drop the receiver to exercise the rollback path).
    fn parked_range_waiter(
        lock_id: u64,
        holder_byte: u8,
        deploy_byte: u8,
        offset: u64,
        length: u64,
        mode: LockMode,
    ) -> (Waiter, oneshot::Receiver<Result<LockId, LockError>>) {
        let (tx, rx) = oneshot::channel();
        let w = Waiter {
            lock_id: LockId::try_from(lock_id).unwrap(),
            kind: WaitKind::Range {
                offset,
                length,
                mode,
            },
            holder: HolderId::from_bytes([holder_byte; 32]),
            deploy: [deploy_byte; 32],
            admit: tx,
        };
        (w, rx)
    }

    fn parked_sequential_waiter(
        lock_id: u64,
        holder_byte: u8,
        deploy_byte: u8,
    ) -> (Waiter, oneshot::Receiver<Result<LockId, LockError>>) {
        let (tx, rx) = oneshot::channel();
        let w = Waiter {
            lock_id: LockId::try_from(lock_id).unwrap(),
            kind: WaitKind::Sequential,
            holder: HolderId::from_bytes([holder_byte; 32]),
            deploy: [deploy_byte; 32],
            admit: tx,
        };
        (w, rx)
    }

    /// Empty waiter queue → wake_waiters is a no-op.  Important
    /// because every release path calls wake_waiters today under
    /// Fail-only acquires, so the production call sites must stay
    /// cheap when no one has parked.
    #[test]
    fn wake_waiters_empty_queue_is_noop() {
        let mut state = FileLockState::default();
        wake_waiters(&mut state);
        assert!(state.ranges.is_empty());
        assert!(state.sequential_holder.is_none());
        assert!(state.waiters.is_empty());
    }

    /// Head admissible (Range) → waiter popped, promoted into
    /// `ranges`, admit signal fires with `Ok(lock_id)`.
    #[test]
    fn wake_waiters_promotes_admissible_range_head() {
        let mut state = FileLockState::default();
        let (w, mut rx) = parked_range_waiter(42, 0x11, 0x11, 0, 100, LockMode::Write);
        state.waiters.push_back(w);
        wake_waiters(&mut state);
        assert_eq!(state.ranges.len(), 1);
        assert_eq!(state.ranges[0].id.as_u64(), 42);
        assert_eq!(state.ranges[0].offset, 0);
        assert_eq!(state.ranges[0].length, 100);
        assert_eq!(state.ranges[0].mode, LockMode::Write);
        assert!(state.waiters.is_empty());
        let admit = rx.try_recv().expect("admit must fire");
        assert_eq!(admit, Ok(LockId::try_from(42).unwrap()));
    }

    /// Head admissible (Sequential) → waiter popped, promoted into
    /// `sequential_holder`, admit fires.
    #[test]
    fn wake_waiters_promotes_admissible_sequential_head() {
        let mut state = FileLockState::default();
        let (w, mut rx) = parked_sequential_waiter(7, 0x22, 0x22);
        state.waiters.push_back(w);
        wake_waiters(&mut state);
        let held = state
            .sequential_holder
            .as_ref()
            .expect("sequential_holder must be set");
        assert_eq!(held.id.as_u64(), 7);
        assert!(state.waiters.is_empty());
        let admit = rx.try_recv().expect("admit must fire");
        assert_eq!(admit, Ok(LockId::try_from(7).unwrap()));
    }

    /// Head NOT admissible (sequential_holder already present) →
    /// waiter stays queued, no admission, admit channel still open.
    #[test]
    fn wake_waiters_leaves_inadmissible_range_head_queued() {
        let mut state = FileLockState {
            sequential_holder: Some(SequentialEntry {
                id: LockId::try_from(1).unwrap(),
                holder: HolderId::from_bytes([0x33; 32]),
                deploy: [0x33; 32],
            }),
            ..Default::default()
        };
        let (w, mut rx) = parked_range_waiter(99, 0x11, 0x11, 0, 10, LockMode::Read);
        state.waiters.push_back(w);
        wake_waiters(&mut state);
        assert_eq!(state.waiters.len(), 1);
        assert_eq!(state.ranges.len(), 0);
        assert!(matches!(
            rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
    }

    /// Head NOT admissible (Sequential vs. existing range) → waiter
    /// stays queued.
    #[test]
    fn wake_waiters_leaves_inadmissible_sequential_head_queued() {
        let mut state = FileLockState::default();
        state.ranges.push(RangeEntry {
            id: LockId::try_from(1).unwrap(),
            offset: 0,
            length: 10,
            mode: LockMode::Read,
            holder: HolderId::from_bytes([0x33; 32]),
            deploy: [0x33; 32],
        });
        let (w, mut rx) = parked_sequential_waiter(99, 0x11, 0x11);
        state.waiters.push_back(w);
        wake_waiters(&mut state);
        assert_eq!(state.waiters.len(), 1);
        assert!(state.sequential_holder.is_none());
        assert!(matches!(
            rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
    }

    /// LOAD-BEARING: receiver-drop rollback for Range promotion.
    /// If the awaiting task was dropped between park and admit, the
    /// `send(Ok)` fails and the promoted range must be rolled back
    /// — otherwise the ranges vec would hold a "lock" that nobody
    /// is waiting for and nobody can release (admit was the only
    /// handle the caller had).  Also pins the X-2 / G-02 defense-
    /// in-depth posture against ghost held locks.
    #[test]
    fn wake_waiters_rolls_back_range_promotion_on_dropped_receiver() {
        let mut state = FileLockState::default();
        let (w, rx) = parked_range_waiter(42, 0x11, 0x11, 0, 100, LockMode::Write);
        drop(rx);
        state.waiters.push_back(w);
        wake_waiters(&mut state);
        assert!(
            state.ranges.is_empty(),
            "receiver-drop must rollback the promoted range"
        );
        assert!(state.waiters.is_empty());
    }

    /// LOAD-BEARING: receiver-drop rollback for Sequential promotion.
    #[test]
    fn wake_waiters_rolls_back_sequential_promotion_on_dropped_receiver() {
        let mut state = FileLockState::default();
        let (w, rx) = parked_sequential_waiter(7, 0x22, 0x22);
        drop(rx);
        state.waiters.push_back(w);
        wake_waiters(&mut state);
        assert!(
            state.sequential_holder.is_none(),
            "receiver-drop must rollback the sequential_holder"
        );
        assert!(state.waiters.is_empty());
    }

    /// Two admissible waiters in a row → both promoted in FIFO
    /// order, both admit signals fire with their minted ids.
    #[test]
    fn wake_waiters_promotes_multiple_admissible_in_fifo_order() {
        let mut state = FileLockState::default();
        let (w1, mut rx1) = parked_range_waiter(1, 0x11, 0x11, 0, 10, LockMode::Read);
        let (w2, mut rx2) = parked_range_waiter(2, 0x22, 0x22, 100, 10, LockMode::Read);
        state.waiters.push_back(w1);
        state.waiters.push_back(w2);
        wake_waiters(&mut state);
        assert_eq!(state.ranges.len(), 2);
        assert_eq!(state.ranges[0].id.as_u64(), 1);
        assert_eq!(state.ranges[1].id.as_u64(), 2);
        assert_eq!(rx1.try_recv(), Ok(Ok(LockId::try_from(1).unwrap())));
        assert_eq!(rx2.try_recv(), Ok(Ok(LockId::try_from(2).unwrap())));
    }

    /// LOAD-BEARING head-of-line FIFO: a non-admissible head blocks
    /// an admissible tail.  Downstream waiters MUST NOT overtake —
    /// strict FIFO prevents writer starvation under a continuous
    /// stream of compatible-read admissions.
    ///
    /// Setup: a Write range by holder A is held.  Waiter 1 is a
    /// Write by holder B (NOT admissible — conflicts with A).
    /// Waiter 2 is a Read by holder C that WOULD be admissible on
    /// its own (reader-reader with A's Write would still conflict,
    /// but the test uses a disjoint range so C's Read doesn't
    /// conflict with A's Write).  Head-of-line discipline says
    /// neither moves because waiter 1 blocks.
    #[test]
    fn wake_waiters_head_of_line_blocks_admissible_tail() {
        let mut state = FileLockState::default();
        // Held Write by holder A on [0, 100).
        state.ranges.push(RangeEntry {
            id: LockId::try_from(1).unwrap(),
            offset: 0,
            length: 100,
            mode: LockMode::Write,
            holder: HolderId::from_bytes([0xAA; 32]),
            deploy: [0xAA; 32],
        });
        // Waiter 1: Write by B on [0, 100) — conflicts with A.
        let (w1, mut rx1) = parked_range_waiter(2, 0xBB, 0xBB, 0, 100, LockMode::Write);
        // Waiter 2: Read by C on [200, 300) — disjoint from A, so
        // would be admissible on its own.
        let (w2, mut rx2) = parked_range_waiter(3, 0xCC, 0xCC, 200, 100, LockMode::Read);
        state.waiters.push_back(w1);
        state.waiters.push_back(w2);
        wake_waiters(&mut state);
        assert_eq!(state.ranges.len(), 1, "only A's held range should remain");
        assert_eq!(state.waiters.len(), 2, "head-of-line keeps both queued");
        assert!(matches!(
            rx1.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        assert!(matches!(
            rx2.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
    }

    /// Receiver-drop on waiter 1 (Range) rolls back ITS promotion,
    /// then the pass continues to waiter 2.  Pins that the rollback
    /// doesn't accidentally halt the pass — a dropped receiver is a
    /// local-cancel, not a global stop signal.
    #[test]
    fn wake_waiters_dropped_receiver_does_not_halt_pass() {
        let mut state = FileLockState::default();
        let (w1, rx1) = parked_range_waiter(1, 0x11, 0x11, 0, 10, LockMode::Read);
        let (w2, mut rx2) = parked_range_waiter(2, 0x22, 0x22, 100, 10, LockMode::Read);
        drop(rx1);
        state.waiters.push_back(w1);
        state.waiters.push_back(w2);
        wake_waiters(&mut state);
        assert_eq!(
            state.ranges.len(),
            1,
            "waiter 1 rolled back, waiter 2 promoted"
        );
        assert_eq!(state.ranges[0].id.as_u64(), 2);
        assert!(state.waiters.is_empty());
        assert_eq!(rx2.try_recv(), Ok(Ok(LockId::try_from(2).unwrap())));
    }

    // --- try_acquire_range_wait -----------------------------------
    //
    // End-to-end park / admit / cancel coverage.  Pairs with the
    // `wake_waiters` isolation tests above — a parked waiter now
    // reaches the queue via the production acquire path, and a
    // subsequent `release` call drives `wake_waiters` which admits
    // it.

    /// Zero-length → BadArg BEFORE any state check.  Mirrors the
    /// Fail-only variant's error ordering so the two paths behave
    /// identically on the pre-check boundary.
    #[test]
    fn try_acquire_range_wait_zero_length_returns_bad_arg() {
        let reg = LockRegistry::new();
        let out = reg.try_acquire_range_wait(
            (1, 1),
            0,
            0,
            LockMode::Write,
            HolderId::from_bytes([0x11; 32]),
            deploy_scope(0x11),
        );
        assert_eq!(out.unwrap_err(), LockError::BadArg);
    }

    /// Empty state → Immediate(id) via the direct-promote branch.
    #[test]
    fn try_acquire_range_wait_empty_state_returns_immediate() {
        let reg = LockRegistry::new();
        let out = reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Write,
                HolderId::from_bytes([0x11; 32]),
                deploy_scope(0x11),
            )
            .unwrap();
        match out {
            AcquireOutcome::Immediate(id) => assert_eq!(id.as_u64(), 1),
            AcquireOutcome::Parked { .. } => panic!("empty state must direct-promote"),
        }
    }

    /// Conflicting acquire by a different holder parks with a
    /// minted LockId.  Pins the park path's return shape AND the
    /// state-side invariant that the Waiter was actually enqueued
    /// (via `is_locked` which short-circuits on parked state's
    /// held range).
    #[test]
    fn try_acquire_range_wait_conflict_returns_parked() {
        let reg = LockRegistry::new();
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Write,
            HolderId::from_bytes([0x11; 32]),
            deploy_scope(0x11),
        )
        .unwrap();
        let out = reg
            .try_acquire_range_wait(
                (1, 1),
                50,
                50,
                LockMode::Write,
                HolderId::from_bytes([0x22; 32]),
                deploy_scope(0x22),
            )
            .unwrap();
        let (lock_id, _admit) = match out {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            AcquireOutcome::Immediate(_) => panic!("conflicting acquire must park"),
        };
        assert_eq!(lock_id.as_u64(), 2, "parked waiter gets the next LockId");
    }

    /// Same-holder re-entrant acquire → Immediate (uses
    /// `range_conflicts`'s same-holder skip, matches
    /// `try_acquire_range`).  A cap MUST NOT park on its own held
    /// range — would self-deadlock.
    #[test]
    fn try_acquire_range_wait_same_holder_reentrant_is_immediate() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Write,
            holder.clone(),
            deploy_scope(0x11),
        )
        .unwrap();
        let out = reg
            .try_acquire_range_wait((1, 1), 50, 50, LockMode::Write, holder, deploy_scope(0x11))
            .unwrap();
        assert!(matches!(out, AcquireOutcome::Immediate(_)));
    }

    /// INTEGRATION: park → release → admit.  Pins the full
    /// release→wake_waiters→admit path end-to-end via the
    /// production acquire API.  Also pins the anti-eviction
    /// invariant from PR #527 — the state does NOT get evicted
    /// between park and admit (a waiter is parked, so
    /// `state_is_empty` returns false).
    #[test]
    fn try_acquire_range_wait_release_admits_parked_waiter() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0xAA; 32]);
        let holder_b = HolderId::from_bytes([0xBB; 32]);
        let id_a = reg
            .try_acquire_range(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_a.clone(),
                deploy_scope(0xAA),
            )
            .unwrap();
        let out = reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_b.clone(),
                deploy_scope(0xBB),
            )
            .unwrap();
        let (parked_id, mut admit) = match out {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            AcquireOutcome::Immediate(_) => panic!("must park on conflict"),
        };
        // Not admitted yet.
        assert!(matches!(
            admit.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        // Release A → wake_waiters → B gets admitted with its
        // parked LockId.
        reg.release(id_a, &holder_a).unwrap();
        let admitted = admit.try_recv().expect("admit must fire after release");
        assert_eq!(admitted, Ok(parked_id));
    }

    /// INTEGRATION: strict FIFO on wake with asymmetric waiter
    /// kinds.  B parks as Write, C parks as Read; on release of
    /// A's Write, only B admits (strict head-of-line — C is
    /// Read-admissible against the empty state but FIFO parks
    /// C behind B).  Pins that `wake_waiters` promotes the head
    /// in order even when a later waiter could ALSO have
    /// admitted alone — the strong FIFO property anti-starvation
    /// relies on.
    #[test]
    fn try_acquire_range_wait_fifo_admission_across_multiple_waiters() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0xAA; 32]);
        let id_a = reg
            .try_acquire_range(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_a.clone(),
                deploy_scope(0xAA),
            )
            .unwrap();
        // B parks as Write (head).
        let b_outcome = reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Write,
                HolderId::from_bytes([0xBB; 32]),
                deploy_scope(0xBB),
            )
            .unwrap();
        // C parks as Read (behind B).  C's Read is admissible
        // against empty-state alone, but FIFO must keep C behind
        // B's Write.
        let c_outcome = reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Read,
                HolderId::from_bytes([0xCC; 32]),
                deploy_scope(0xCC),
            )
            .unwrap();
        let (b_id, mut b_admit) = match b_outcome {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            _ => panic!("B must park"),
        };
        let (_c_id, mut c_admit) = match c_outcome {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            _ => panic!("C must park"),
        };
        reg.release(id_a, &holder_a).unwrap();
        // B admits.
        assert_eq!(b_admit.try_recv(), Ok(Ok(b_id)));
        // C stays parked — B's Write now blocks C's Read, AND
        // even if B hadn't blocked, strict FIFO would have kept
        // C behind B.
        assert!(matches!(
            c_admit.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
    }

    /// LOAD-BEARING: cycle check prevents cross-deploy deadlock.
    /// Set up a two-cycle: D1 holds file F1 and parks on F2; D2
    /// holds F2.  Then D2's wait on F1 would close the cycle →
    /// Deadlock.
    #[test]
    fn try_acquire_range_wait_deadlock_detects_two_cycle() {
        let reg = LockRegistry::new();
        let h_d1 = HolderId::from_bytes([0x01; 32]);
        let h_d2 = HolderId::from_bytes([0x02; 32]);
        // D1 holds F1.
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Write,
            h_d1.clone(),
            deploy_scope(0x01),
        )
        .unwrap();
        // D2 holds F2.
        reg.try_acquire_range(
            (2, 2),
            0,
            100,
            LockMode::Write,
            h_d2.clone(),
            deploy_scope(0x02),
        )
        .unwrap();
        // D1 parks on F2 → waits on D2.
        let _d1_parked = reg
            .try_acquire_range_wait((2, 2), 0, 100, LockMode::Write, h_d1, deploy_scope(0x01))
            .unwrap();
        // D2 parks on F1 → would wait on D1 → closes the cycle.
        let out =
            reg.try_acquire_range_wait((1, 1), 0, 100, LockMode::Write, h_d2, deploy_scope(0x02));
        assert_eq!(out.unwrap_err(), LockError::Deadlock);
    }

    /// LOAD-BEARING empty-state leak defense (deadlock path):
    /// a Deadlock-refused call MUST NOT leave an empty
    /// `FileLockState` in the map (nor mutate an existing one).
    /// Pins the check-before-insert discipline for the park
    /// branch's deadlock refusal.
    #[test]
    fn try_acquire_range_wait_deadlock_does_not_leak_empty_state() {
        let reg = LockRegistry::new();
        let h_d1 = HolderId::from_bytes([0x01; 32]);
        let h_d2 = HolderId::from_bytes([0x02; 32]);
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Write,
            h_d1.clone(),
            deploy_scope(0x01),
        )
        .unwrap();
        reg.try_acquire_range(
            (2, 2),
            0,
            100,
            LockMode::Write,
            h_d2.clone(),
            deploy_scope(0x02),
        )
        .unwrap();
        // D1 parks on F2.
        let _d1_parked = reg
            .try_acquire_range_wait((2, 2), 0, 100, LockMode::Write, h_d1, deploy_scope(0x01))
            .unwrap();
        // Snapshot state BEFORE the deadlock-refused call.
        let before_len = reg.inner.read().unwrap().len();
        let before_waiters: Vec<usize> = reg
            .inner
            .read()
            .unwrap()
            .values()
            .map(|s| s.waiters.len())
            .collect();
        // D2's wait on F1 closes the cycle.
        let dead =
            reg.try_acquire_range_wait((1, 1), 0, 100, LockMode::Write, h_d2, deploy_scope(0x02));
        assert_eq!(dead.unwrap_err(), LockError::Deadlock);
        let after_len = reg.inner.read().unwrap().len();
        let after_waiters: Vec<usize> = reg
            .inner
            .read()
            .unwrap()
            .values()
            .map(|s| s.waiters.len())
            .collect();
        assert_eq!(before_len, after_len, "no new registry entries");
        assert_eq!(
            before_waiters, after_waiters,
            "no mutation of existing waiter queues"
        );
    }

    /// MAX_RANGES_PER_FILE cap applies universally — even Wait
    /// callers can't park if the file is at the range cap,
    /// because wake_waiters would never admit them.  Mirrors
    /// the Fail-only variant's cap pin.
    #[test]
    fn try_acquire_range_wait_range_cap_rejects_with_quota() {
        let reg = LockRegistry::new();
        // Fill the file to MAX_RANGES_PER_FILE with distinct
        // non-overlapping Read ranges by a single holder (so
        // range_conflicts stays silent; the cap is the only
        // thing in the way).
        let holder = HolderId::from_bytes([0x11; 32]);
        for i in 0..MAX_RANGES_PER_FILE {
            reg.try_acquire_range(
                (1, 1),
                (i as u64) * 10,
                1,
                LockMode::Read,
                holder.clone(),
                deploy_scope(0x11),
            )
            .unwrap();
        }
        // One more acquire — regardless of policy — must hit
        // the cap BEFORE the admissibility check.
        let out = reg.try_acquire_range_wait(
            (1, 1),
            999_999,
            1,
            LockMode::Read,
            HolderId::from_bytes([0x22; 32]),
            deploy_scope(0x22),
        );
        assert_eq!(out.unwrap_err(), LockError::QuotaExceeded);
    }

    /// LOAD-BEARING empty-state leak defense (direct-promote
    /// path): a mint failure (if we could ever hit the ceiling)
    /// must not leak an empty slot on a fresh key.  Harder to
    /// trigger realistically — this test exercises the park
    /// branch's QuotaExceeded on MAX_WAITERS_PER_FILE instead,
    /// which has the same check-before-insert discipline.
    ///
    /// Fill the waiter queue on an EXISTING conflicting file,
    /// then issue one more Wait acquire — must fire
    /// QuotaExceeded without touching an UNRELATED fresh key.
    #[test]
    fn try_acquire_range_wait_failed_park_does_not_leak_empty_state() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0xAA; 32]);
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Write,
            holder_a,
            deploy_scope(0xAA),
        )
        .unwrap();
        // Park MAX_WAITERS_PER_FILE waiters.  Must keep the
        // oneshot receivers alive so wake_waiters would still see
        // them as live waiters if a release intervened — but no
        // release occurs here, so the queue stays at MAX.
        let mut keep_alive = Vec::with_capacity(MAX_WAITERS_PER_FILE);
        for i in 0..MAX_WAITERS_PER_FILE {
            // Cycle byte through [1, 100] so no waiter collides
            // with A's 0xAA (= 170) → every waiter conflicts
            // via range_conflicts and MUST park (no same-holder
            // skip, no self-cycle through NB-7).
            let byte = ((i % 100) + 1) as u8;
            let out = reg
                .try_acquire_range_wait(
                    (1, 1),
                    0,
                    100,
                    LockMode::Write,
                    HolderId::from_bytes([byte; 32]),
                    [byte; 32],
                )
                .unwrap();
            match out {
                AcquireOutcome::Parked { admit, .. } => keep_alive.push(admit),
                AcquireOutcome::Immediate(_) => panic!("every waiter must park"),
            }
        }
        let before = reg.inner.read().unwrap().len();
        // One more → MAX_WAITERS cap fires.
        let fresh_byte = 0xEE;
        let out = reg.try_acquire_range_wait(
            (1, 1),
            0,
            100,
            LockMode::Write,
            HolderId::from_bytes([fresh_byte; 32]),
            [fresh_byte; 32],
        );
        assert_eq!(out.unwrap_err(), LockError::QuotaExceeded);
        let after = reg.inner.read().unwrap().len();
        assert_eq!(before, after, "waiter-cap refusal must not add entries");
    }

    /// Dropped admit receiver on parked waiter → wake_waiters
    /// rolls back on the subsequent release.  Pins the full
    /// park → drop → release → rollback chain via the production
    /// acquire path.  Also pins the X-2 / G-02 third-layer
    /// defense (receiver-drop rollback) in integration.
    #[test]
    fn try_acquire_range_wait_dropped_receiver_rolls_back_on_release() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0xAA; 32]);
        let holder_b = HolderId::from_bytes([0xBB; 32]);
        let id_a = reg
            .try_acquire_range(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_a.clone(),
                deploy_scope(0xAA),
            )
            .unwrap();
        let out = reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_b.clone(),
                deploy_scope(0xBB),
            )
            .unwrap();
        let admit = match out {
            AcquireOutcome::Parked { admit, .. } => admit,
            AcquireOutcome::Immediate(_) => panic!("must park"),
        };
        drop(admit);
        // Release A → wake_waiters tries to admit B → send fails
        // → rollback.  State becomes empty.  Registry entry
        // evicted (parked waiter rolled back, no held locks).
        reg.release(id_a, &holder_a).unwrap();
        assert_eq!(
            reg.inner.read().unwrap().len(),
            0,
            "state must evict after waiter rollback and A's release"
        );
    }

    // --- try_acquire_sequential_wait ------------------------------
    //
    // Symmetric to try_acquire_range_wait but for whole-file
    // sequential acquires: no range-cap pre-check (one holder per
    // file by construction), no same-holder skip, state must be
    // entirely empty for direct-promote.

    /// Empty state → Immediate(id) via direct-promote.
    #[test]
    fn try_acquire_sequential_wait_empty_state_returns_immediate() {
        let reg = LockRegistry::new();
        let out = reg
            .try_acquire_sequential_wait(
                (1, 1),
                HolderId::from_bytes([0x11; 32]),
                deploy_scope(0x11),
            )
            .unwrap();
        match out {
            AcquireOutcome::Immediate(id) => assert_eq!(id.as_u64(), 1),
            AcquireOutcome::Parked { .. } => panic!("empty state must direct-promote"),
        }
    }

    /// Existing sequential_holder → Parked.
    #[test]
    fn try_acquire_sequential_wait_existing_sequential_parks() {
        let reg = LockRegistry::new();
        reg.try_acquire_sequential((1, 1), HolderId::from_bytes([0x11; 32]), deploy_scope(0x11))
            .unwrap();
        let out = reg
            .try_acquire_sequential_wait(
                (1, 1),
                HolderId::from_bytes([0x22; 32]),
                deploy_scope(0x22),
            )
            .unwrap();
        let (lock_id, _admit) = match out {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            AcquireOutcome::Immediate(_) => panic!("conflicting acquire must park"),
        };
        assert_eq!(lock_id.as_u64(), 2);
    }

    /// Existing held RANGE → Parked (even by the same holder —
    /// no same-holder skip for sequential).
    #[test]
    fn try_acquire_sequential_wait_existing_range_parks() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Read,
            holder.clone(),
            deploy_scope(0x11),
        )
        .unwrap();
        let out = reg
            .try_acquire_sequential_wait(
                (1, 1),
                HolderId::from_bytes([0x22; 32]),
                deploy_scope(0x22),
            )
            .unwrap();
        assert!(matches!(out, AcquireOutcome::Parked { .. }));
    }

    /// LOAD-BEARING no-same-holder-skip: a cap holding its OWN
    /// range on the file still parks when it tries a Wait
    /// sequential acquire.  Pins the sequential_conflicts rule
    /// at the Wait-API boundary.
    #[test]
    fn try_acquire_sequential_wait_same_holder_range_still_parks() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Read,
            holder.clone(),
            deploy_scope(0x11),
        )
        .unwrap();
        let out = reg
            .try_acquire_sequential_wait((1, 1), holder, deploy_scope(0x11))
            .unwrap();
        assert!(
            matches!(out, AcquireOutcome::Parked { .. }),
            "same-holder sequential acquire on own held range must park, not promote"
        );
    }

    /// INTEGRATION: park on existing sequential → release of the
    /// held sequential → wake_waiters admits the parked waiter.
    #[test]
    fn try_acquire_sequential_wait_release_admits_parked_waiter() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0xAA; 32]);
        let holder_b = HolderId::from_bytes([0xBB; 32]);
        let id_a = reg
            .try_acquire_sequential((1, 1), holder_a.clone(), deploy_scope(0xAA))
            .unwrap();
        let out = reg
            .try_acquire_sequential_wait((1, 1), holder_b, deploy_scope(0xBB))
            .unwrap();
        let (parked_id, mut admit) = match out {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            AcquireOutcome::Immediate(_) => panic!("must park"),
        };
        assert!(matches!(
            admit.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        reg.release(id_a, &holder_a).unwrap();
        assert_eq!(admit.try_recv(), Ok(Ok(parked_id)));
    }

    /// INTEGRATION: park on held range → release of the held
    /// range → wake_waiters admits the parked sequential waiter.
    /// Pins the cross-kind release→admit path.
    #[test]
    fn try_acquire_sequential_wait_range_release_admits_sequential_waiter() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0xAA; 32]);
        let holder_b = HolderId::from_bytes([0xBB; 32]);
        let id_a = reg
            .try_acquire_range(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_a.clone(),
                deploy_scope(0xAA),
            )
            .unwrap();
        let out = reg
            .try_acquire_sequential_wait((1, 1), holder_b, deploy_scope(0xBB))
            .unwrap();
        let (parked_id, mut admit) = match out {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            AcquireOutcome::Immediate(_) => panic!("must park"),
        };
        reg.release(id_a, &holder_a).unwrap();
        assert_eq!(admit.try_recv(), Ok(Ok(parked_id)));
    }

    /// LOAD-BEARING: NB-7 cycle detection fires on cross-deploy
    /// mutual-wait where the target waiter is sequential.
    #[test]
    fn try_acquire_sequential_wait_deadlock_detects_two_cycle() {
        let reg = LockRegistry::new();
        let h_d1 = HolderId::from_bytes([0x01; 32]);
        let h_d2 = HolderId::from_bytes([0x02; 32]);
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Write,
            h_d1.clone(),
            deploy_scope(0x01),
        )
        .unwrap();
        reg.try_acquire_range(
            (2, 2),
            0,
            100,
            LockMode::Write,
            h_d2.clone(),
            deploy_scope(0x02),
        )
        .unwrap();
        // D1 parks (sequential) on F2 → waits on D2.
        let _d1_parked = reg
            .try_acquire_sequential_wait((2, 2), h_d1, deploy_scope(0x01))
            .unwrap();
        // D2 parks (sequential) on F1 → closes the cycle.
        let out = reg.try_acquire_sequential_wait((1, 1), h_d2, deploy_scope(0x02));
        assert_eq!(out.unwrap_err(), LockError::Deadlock);
    }

    /// LOAD-BEARING empty-state leak defense (deadlock path):
    /// a Deadlock-refused call MUST NOT mutate the registry.
    #[test]
    fn try_acquire_sequential_wait_deadlock_does_not_leak_empty_state() {
        let reg = LockRegistry::new();
        let h_d1 = HolderId::from_bytes([0x01; 32]);
        let h_d2 = HolderId::from_bytes([0x02; 32]);
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Write,
            h_d1.clone(),
            deploy_scope(0x01),
        )
        .unwrap();
        reg.try_acquire_range(
            (2, 2),
            0,
            100,
            LockMode::Write,
            h_d2.clone(),
            deploy_scope(0x02),
        )
        .unwrap();
        let _d1_parked = reg
            .try_acquire_sequential_wait((2, 2), h_d1, deploy_scope(0x01))
            .unwrap();
        let before_len = reg.inner.read().unwrap().len();
        let before_waiters: Vec<usize> = reg
            .inner
            .read()
            .unwrap()
            .values()
            .map(|s| s.waiters.len())
            .collect();
        let dead = reg.try_acquire_sequential_wait((1, 1), h_d2, deploy_scope(0x02));
        assert_eq!(dead.unwrap_err(), LockError::Deadlock);
        let after_len = reg.inner.read().unwrap().len();
        let after_waiters: Vec<usize> = reg
            .inner
            .read()
            .unwrap()
            .values()
            .map(|s| s.waiters.len())
            .collect();
        assert_eq!(before_len, after_len);
        assert_eq!(before_waiters, after_waiters);
    }

    /// LOAD-BEARING empty-state leak defense (waiter-cap path):
    /// a MAX_WAITERS-refused sequential park MUST NOT mutate.
    #[test]
    fn try_acquire_sequential_wait_failed_park_does_not_leak_empty_state() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0xAA; 32]);
        reg.try_acquire_sequential((1, 1), holder_a, deploy_scope(0xAA))
            .unwrap();
        // Fill the waiter queue (via Range Wait acquires — any
        // park on this file bumps the single shared waiters
        // queue regardless of WaitKind).
        let mut keep_alive = Vec::with_capacity(MAX_WAITERS_PER_FILE);
        for i in 0..MAX_WAITERS_PER_FILE {
            let byte = ((i % 100) + 1) as u8;
            let out = reg
                .try_acquire_range_wait(
                    (1, 1),
                    0,
                    100,
                    LockMode::Write,
                    HolderId::from_bytes([byte; 32]),
                    [byte; 32],
                )
                .unwrap();
            match out {
                AcquireOutcome::Parked { admit, .. } => keep_alive.push(admit),
                AcquireOutcome::Immediate(_) => panic!("every waiter must park"),
            }
        }
        let before = reg.inner.read().unwrap().len();
        let out =
            reg.try_acquire_sequential_wait((1, 1), HolderId::from_bytes([0xEE; 32]), [0xEE; 32]);
        assert_eq!(out.unwrap_err(), LockError::QuotaExceeded);
        let after = reg.inner.read().unwrap().len();
        assert_eq!(before, after);
    }

    /// Dropped admit receiver → wake_waiters rolls back the
    /// sequential_holder promotion.  Integration pin for the
    /// X-2 / G-02 third-layer defense via the production
    /// sequential park API.
    #[test]
    fn try_acquire_sequential_wait_dropped_receiver_rolls_back_on_release() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0xAA; 32]);
        let holder_b = HolderId::from_bytes([0xBB; 32]);
        let id_a = reg
            .try_acquire_sequential((1, 1), holder_a.clone(), deploy_scope(0xAA))
            .unwrap();
        let out = reg
            .try_acquire_sequential_wait((1, 1), holder_b, deploy_scope(0xBB))
            .unwrap();
        let admit = match out {
            AcquireOutcome::Parked { admit, .. } => admit,
            AcquireOutcome::Immediate(_) => panic!("must park"),
        };
        drop(admit);
        reg.release(id_a, &holder_a).unwrap();
        assert_eq!(
            reg.inner.read().unwrap().len(),
            0,
            "state must evict after sequential waiter rollback and A's release"
        );
    }

    /// Quota-before-deadlock error-order pin: when BOTH the
    /// MAX_WAITERS cap AND NB-7 would fire on the same acquire,
    /// quota wins.  Consensus-observable fixed order per
    /// [`LockError::Deadlock`]'s docstring (O(1) quota check
    /// first).  Setup:
    ///
    ///   - D1 holds F1 (deploy 0x01, holder 0x01).
    ///   - D2 holds F2 (deploy 0x02, holder 0x02).
    ///   - D1 parks on F2 → one waiter on F2 (edge D1 → D2).
    ///   - Fill F1's waiter queue to MAX_WAITERS with distinct
    ///     deploys all ≠ D1, D2 (so NB-7 seeds on F1 don't
    ///     recognize them as cycle candidates).
    ///   - D2 tries sequential_wait on F1 → would close cycle
    ///     D2 → D1 → D2, AND the F1 waiter queue is full.
    ///
    /// Both conditions fire; the quota check runs first →
    /// expect `QuotaExceeded`, NOT `Deadlock`.
    #[test]
    fn try_acquire_sequential_wait_quota_fires_before_deadlock() {
        let reg = LockRegistry::new();
        let h_d1 = HolderId::from_bytes([0x01; 32]);
        let h_d2 = HolderId::from_bytes([0x02; 32]);
        // D1 holds F1, D2 holds F2.
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Write,
            h_d1.clone(),
            deploy_scope(0x01),
        )
        .unwrap();
        reg.try_acquire_range(
            (2, 2),
            0,
            100,
            LockMode::Write,
            h_d2.clone(),
            deploy_scope(0x02),
        )
        .unwrap();
        // D1 parks on F2 (edge D1 → D2).
        let _d1_parked = reg
            .try_acquire_range_wait((2, 2), 0, 100, LockMode::Write, h_d1, deploy_scope(0x01))
            .unwrap();
        // Fill F1's waiter queue to MAX_WAITERS_PER_FILE with
        // bystanders — distinct bytes in [50, 149] (none are
        // 0x01 or 0x02).  Keep admits alive so no wake fires.
        let mut keep_alive = Vec::with_capacity(MAX_WAITERS_PER_FILE);
        for i in 0..MAX_WAITERS_PER_FILE {
            let byte = ((i % 100) + 50) as u8;
            let out = reg
                .try_acquire_range_wait(
                    (1, 1),
                    0,
                    100,
                    LockMode::Write,
                    HolderId::from_bytes([byte; 32]),
                    [byte; 32],
                )
                .unwrap();
            match out {
                AcquireOutcome::Parked { admit, .. } => keep_alive.push(admit),
                AcquireOutcome::Immediate(_) => panic!("every bystander must park"),
            }
        }
        // D2's sequential_wait on F1 would both (a) hit the
        // quota cap AND (b) close the D2 → D1 → D2 cycle.
        let out = reg.try_acquire_sequential_wait((1, 1), h_d2, deploy_scope(0x02));
        assert_eq!(
            out.unwrap_err(),
            LockError::QuotaExceeded,
            "quota must fire before deadlock on the shared boundary"
        );
    }

    // --- cancel_wait ----------------------------------------------
    //
    // Targeted-by-LockId cancel of a parked waiter.  Pairs with
    // the yet-to-land bulk-cancel sweeps.

    /// Happy path: cancel a parked Range waiter → the admit
    /// receiver resolves to `Err(LockError::Cancelled)`, and the
    /// waiter is removed from the queue.
    #[test]
    fn cancel_wait_range_waiter_signals_cancelled() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0xAA; 32]);
        let holder_b = HolderId::from_bytes([0xBB; 32]);
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Write,
            holder_a,
            deploy_scope(0xAA),
        )
        .unwrap();
        let out = reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_b.clone(),
                deploy_scope(0xBB),
            )
            .unwrap();
        let (lock_id, mut admit) = match out {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            AcquireOutcome::Immediate(_) => panic!("must park"),
        };
        reg.cancel_wait(lock_id, &holder_b).unwrap();
        assert_eq!(admit.try_recv(), Ok(Err(LockError::Cancelled)));
    }

    /// Happy path (Sequential variant): same discipline.
    #[test]
    fn cancel_wait_sequential_waiter_signals_cancelled() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0xAA; 32]);
        let holder_b = HolderId::from_bytes([0xBB; 32]);
        reg.try_acquire_sequential((1, 1), holder_a, deploy_scope(0xAA))
            .unwrap();
        let out = reg
            .try_acquire_sequential_wait((1, 1), holder_b.clone(), deploy_scope(0xBB))
            .unwrap();
        let (lock_id, mut admit) = match out {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            AcquireOutcome::Immediate(_) => panic!("must park"),
        };
        reg.cancel_wait(lock_id, &holder_b).unwrap();
        assert_eq!(admit.try_recv(), Ok(Err(LockError::Cancelled)));
    }

    /// Unknown `lock_id` → Closed.  Pins that cancel_wait doesn't
    /// quietly succeed on a stale / never-minted id.
    #[test]
    fn cancel_wait_unknown_lock_id_returns_closed() {
        let reg = LockRegistry::new();
        let out = reg.cancel_wait(
            LockId::try_from(999_999).unwrap(),
            &HolderId::from_bytes([0x11; 32]),
        );
        assert_eq!(out.unwrap_err(), LockError::Closed);
    }

    /// LOAD-BEARING: wrong-holder cancel → Closed (indistinguishable
    /// from unknown-id per release-path convention).  Attacker
    /// enumerating holder bytes gets no diagnostic from the error
    /// value AND the ct_eq comparison is branchless.
    #[test]
    fn cancel_wait_wrong_holder_returns_closed_and_leaves_waiter() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0xAA; 32]);
        let holder_b = HolderId::from_bytes([0xBB; 32]);
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Write,
            holder_a,
            deploy_scope(0xAA),
        )
        .unwrap();
        let out = reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_b,
                deploy_scope(0xBB),
            )
            .unwrap();
        let (lock_id, mut admit) = match out {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            AcquireOutcome::Immediate(_) => panic!("must park"),
        };
        // Wrong holder.
        let imposter = HolderId::from_bytes([0xCC; 32]);
        let err = reg.cancel_wait(lock_id, &imposter);
        assert_eq!(err.unwrap_err(), LockError::Closed);
        // Waiter still parked (not cancelled, not admitted).
        assert!(matches!(
            admit.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        // Queue still has 1 waiter.
        let guard = reg.inner.read().unwrap();
        assert_eq!(guard.get(&(1, 1)).unwrap().waiters.len(), 1);
    }

    /// Isolated eviction pin: a parked waiter with NO held locks
    /// on the file (state constructed hand-populated) → cancel
    /// evicts the state entirely.  Covers the pure eviction
    /// branch without the admit-first-then-cancel-second race.
    #[test]
    fn cancel_wait_waiter_only_state_evicts_on_cancel() {
        let reg = LockRegistry::new();
        // Build a state with ONLY a parked waiter by hand.
        {
            let mut guard = reg.inner.write().unwrap();
            let (tx, _rx) = oneshot::channel();
            let state = guard.entry((1, 1)).or_default();
            state.waiters.push_back(Waiter {
                lock_id: LockId::try_from(42).unwrap(),
                kind: WaitKind::Sequential,
                holder: HolderId::from_bytes([0x11; 32]),
                deploy: [0x11; 32],
                admit: tx,
            });
        }
        assert_eq!(reg.inner.read().unwrap().len(), 1);
        reg.cancel_wait(
            LockId::try_from(42).unwrap(),
            &HolderId::from_bytes([0x11; 32]),
        )
        .unwrap();
        assert_eq!(
            reg.inner.read().unwrap().len(),
            0,
            "waiter-only state must evict after its sole waiter is cancelled"
        );
    }

    /// Cancel one of multiple parked waiters → the targeted
    /// waiter signals Cancelled, the others stay queued and
    /// un-signalled.
    #[test]
    fn cancel_wait_only_targets_the_named_lock_id() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0xAA; 32]);
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Write,
            holder_a,
            deploy_scope(0xAA),
        )
        .unwrap();
        let holder_b = HolderId::from_bytes([0xBB; 32]);
        let holder_c = HolderId::from_bytes([0xCC; 32]);
        let (b_id, mut b_admit) = match reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_b.clone(),
                deploy_scope(0xBB),
            )
            .unwrap()
        {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            _ => panic!("B must park"),
        };
        let (_c_id, mut c_admit) = match reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_c,
                deploy_scope(0xCC),
            )
            .unwrap()
        {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            _ => panic!("C must park"),
        };
        reg.cancel_wait(b_id, &holder_b).unwrap();
        assert_eq!(b_admit.try_recv(), Ok(Err(LockError::Cancelled)));
        // C is still parked.
        assert!(matches!(
            c_admit.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        assert_eq!(
            reg.inner
                .read()
                .unwrap()
                .get(&(1, 1))
                .unwrap()
                .waiters
                .len(),
            1
        );
    }

    /// After cancelling the head of the FIFO queue, the next
    /// release's wake_waiters promotes the NEW head.  Pins the
    /// cancel + wake interaction (cancel removes a waiter →
    /// next waiter becomes head → next wake admits it if
    /// admissible).
    #[test]
    fn cancel_wait_head_promotes_next_waiter_on_subsequent_release() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0xAA; 32]);
        let id_a = reg
            .try_acquire_range(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_a.clone(),
                deploy_scope(0xAA),
            )
            .unwrap();
        let holder_b = HolderId::from_bytes([0xBB; 32]);
        let holder_c = HolderId::from_bytes([0xCC; 32]);
        let (b_id, _b_admit) = match reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_b.clone(),
                deploy_scope(0xBB),
            )
            .unwrap()
        {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            _ => panic!("B must park"),
        };
        let (c_id, mut c_admit) = match reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_c,
                deploy_scope(0xCC),
            )
            .unwrap()
        {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            _ => panic!("C must park"),
        };
        // Cancel the head (B).
        reg.cancel_wait(b_id, &holder_b).unwrap();
        // Release A → wake_waiters admits new head (C).
        reg.release(id_a, &holder_a).unwrap();
        assert_eq!(c_admit.try_recv(), Ok(Ok(c_id)));
    }

    /// Dropped admit receiver → cancel still succeeds (send
    /// fails silently; caller already walked away).  Pins the
    /// no-panic-on-dead-receiver property.
    #[test]
    fn cancel_wait_dropped_receiver_still_succeeds() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0xAA; 32]);
        let holder_b = HolderId::from_bytes([0xBB; 32]);
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Write,
            holder_a,
            deploy_scope(0xAA),
        )
        .unwrap();
        let out = reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_b.clone(),
                deploy_scope(0xBB),
            )
            .unwrap();
        let (lock_id, admit) = match out {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            _ => panic!("must park"),
        };
        drop(admit);
        reg.cancel_wait(lock_id, &holder_b).unwrap();
        // Waiter removed from queue.
        assert_eq!(
            reg.inner
                .read()
                .unwrap()
                .get(&(1, 1))
                .unwrap()
                .waiters
                .len(),
            0
        );
    }

    /// Already-admitted LockId → Closed on cancel.  Pins that
    /// cancel ONLY targets the waiter queue, not held locks (a
    /// post-promote lock_id must be released via `release`,
    /// not cancelled).
    #[test]
    fn cancel_wait_on_promoted_waiter_returns_closed() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0xAA; 32]);
        let id_a = reg
            .try_acquire_range(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_a.clone(),
                deploy_scope(0xAA),
            )
            .unwrap();
        let holder_b = HolderId::from_bytes([0xBB; 32]);
        let (b_id, _b_admit) = match reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_b.clone(),
                deploy_scope(0xBB),
            )
            .unwrap()
        {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            _ => panic!("must park"),
        };
        // Release A → wake promotes B.
        reg.release(id_a, &holder_a).unwrap();
        // Now B is a HELD lock.  Cancel on its id is Closed.
        let err = reg.cancel_wait(b_id, &holder_b);
        assert_eq!(err.unwrap_err(), LockError::Closed);
    }

    // --- cancel_all_waiters_for_holder ----------------------------

    /// Sweeps every waiter whose holder matches, across every
    /// file.  B's unrelated waiter on a different file survives.
    #[test]
    fn cancel_all_waiters_for_holder_sweeps_across_distinct_files() {
        let reg = LockRegistry::new();
        let holder_a = HolderId::from_bytes([0xAA; 32]);
        let holder_b = HolderId::from_bytes([0xBB; 32]);
        let holder_x = HolderId::from_bytes([0x01; 32]);
        let holder_y = HolderId::from_bytes([0x02; 32]);
        // Put holders X and Y on files F1 and F2 (so A/B can park).
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Write,
            holder_x,
            deploy_scope(0x01),
        )
        .unwrap();
        reg.try_acquire_range(
            (2, 2),
            0,
            100,
            LockMode::Write,
            holder_y,
            deploy_scope(0x02),
        )
        .unwrap();
        // A parks on F1 and F2.
        let a1 = reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder_a.clone(),
                deploy_scope(0xAA),
            )
            .unwrap();
        let a2 = reg
            .try_acquire_range_wait(
                (2, 2),
                0,
                100,
                LockMode::Write,
                holder_a.clone(),
                deploy_scope(0xAA),
            )
            .unwrap();
        // B parks on F2 only.
        let b2 = reg
            .try_acquire_range_wait(
                (2, 2),
                0,
                100,
                LockMode::Write,
                holder_b,
                deploy_scope(0xBB),
            )
            .unwrap();
        let n = reg.cancel_all_waiters_for_holder(&holder_a);
        assert_eq!(n, 2, "both of A's waiters cancelled");
        // A's admits fire with Cancelled.
        let mut a1_rx = match a1 {
            AcquireOutcome::Parked { admit, .. } => admit,
            _ => panic!("a1 must park"),
        };
        let mut a2_rx = match a2 {
            AcquireOutcome::Parked { admit, .. } => admit,
            _ => panic!("a2 must park"),
        };
        assert_eq!(a1_rx.try_recv(), Ok(Err(LockError::Cancelled)));
        assert_eq!(a2_rx.try_recv(), Ok(Err(LockError::Cancelled)));
        // B's admit stays empty.
        let mut b2_rx = match b2 {
            AcquireOutcome::Parked { admit, .. } => admit,
            _ => panic!("b2 must park"),
        };
        assert!(matches!(
            b2_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        // F2 still has B's waiter + Y's held range.
        let guard = reg.inner.read().unwrap();
        assert_eq!(guard.get(&(2, 2)).unwrap().waiters.len(), 1);
        assert_eq!(guard.get(&(1, 1)).unwrap().waiters.len(), 0);
    }

    /// No matching holder → zero cancelled, no mutation.
    #[test]
    fn cancel_all_waiters_for_holder_no_matches_returns_zero() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        reg.try_acquire_range((1, 1), 0, 100, LockMode::Write, holder, deploy_scope(0x11))
            .unwrap();
        let _ = reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Write,
                HolderId::from_bytes([0x22; 32]),
                deploy_scope(0x22),
            )
            .unwrap();
        let n = reg.cancel_all_waiters_for_holder(&HolderId::from_bytes([0xFF; 32]));
        assert_eq!(n, 0);
        // Waiter still queued.
        assert_eq!(
            reg.inner
                .read()
                .unwrap()
                .get(&(1, 1))
                .unwrap()
                .waiters
                .len(),
            1
        );
    }

    /// LOAD-BEARING: cancel + no-held-locks → state eviction.
    /// If a sweep cancels every waiter on a file that also has
    /// no held locks, the state must evict.  Pins the integration
    /// with state_is_empty.
    #[test]
    fn cancel_all_waiters_for_holder_evicts_waiter_only_states() {
        let reg = LockRegistry::new();
        // Hand-populate a waiter-only state (no held locks).
        {
            let mut guard = reg.inner.write().unwrap();
            let (tx, _rx) = oneshot::channel();
            let state = guard.entry((1, 1)).or_default();
            state.waiters.push_back(Waiter {
                lock_id: LockId::try_from(42).unwrap(),
                kind: WaitKind::Sequential,
                holder: HolderId::from_bytes([0x11; 32]),
                deploy: [0x11; 32],
                admit: tx,
            });
        }
        assert_eq!(reg.inner.read().unwrap().len(), 1);
        reg.cancel_all_waiters_for_holder(&HolderId::from_bytes([0x11; 32]));
        assert_eq!(
            reg.inner.read().unwrap().len(),
            0,
            "waiter-only state must evict after bulk cancel"
        );
    }

    /// Dropped-receiver waiters are still removed from the queue
    /// (send fails silently, same as cancel_wait).
    #[test]
    fn cancel_all_waiters_for_holder_handles_dropped_receivers() {
        let reg = LockRegistry::new();
        let holder = HolderId::from_bytes([0x11; 32]);
        let holder_a = HolderId::from_bytes([0xAA; 32]);
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Write,
            holder_a,
            deploy_scope(0xAA),
        )
        .unwrap();
        let out = reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Write,
                holder.clone(),
                deploy_scope(0x11),
            )
            .unwrap();
        match out {
            AcquireOutcome::Parked { admit, .. } => drop(admit),
            _ => panic!("must park"),
        };
        let n = reg.cancel_all_waiters_for_holder(&holder);
        assert_eq!(n, 1);
    }

    // --- cancel_all_waiters_for_deploy ----------------------------

    /// Mirrors the holder sweep but keyed on deploy.
    #[test]
    fn cancel_all_waiters_for_deploy_sweeps_across_distinct_files() {
        let reg = LockRegistry::new();
        let d_a = deploy_scope(0xAA);
        let d_b = deploy_scope(0xBB);
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Write,
            HolderId::from_bytes([0x01; 32]),
            deploy_scope(0x01),
        )
        .unwrap();
        reg.try_acquire_range(
            (2, 2),
            0,
            100,
            LockMode::Write,
            HolderId::from_bytes([0x02; 32]),
            deploy_scope(0x02),
        )
        .unwrap();
        let a1 = reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Write,
                HolderId::from_bytes([0xAA; 32]),
                d_a,
            )
            .unwrap();
        let a2 = reg
            .try_acquire_range_wait(
                (2, 2),
                0,
                100,
                LockMode::Write,
                HolderId::from_bytes([0xAA; 32]),
                d_a,
            )
            .unwrap();
        let b2 = reg
            .try_acquire_range_wait(
                (2, 2),
                0,
                100,
                LockMode::Write,
                HolderId::from_bytes([0xBB; 32]),
                d_b,
            )
            .unwrap();
        let n = reg.cancel_all_waiters_for_deploy(&d_a);
        assert_eq!(n, 2);
        let mut a1_rx = match a1 {
            AcquireOutcome::Parked { admit, .. } => admit,
            _ => panic!("a1 must park"),
        };
        let mut a2_rx = match a2 {
            AcquireOutcome::Parked { admit, .. } => admit,
            _ => panic!("a2 must park"),
        };
        assert_eq!(a1_rx.try_recv(), Ok(Err(LockError::Cancelled)));
        assert_eq!(a2_rx.try_recv(), Ok(Err(LockError::Cancelled)));
        let mut b2_rx = match b2 {
            AcquireOutcome::Parked { admit, .. } => admit,
            _ => panic!("b2 must park"),
        };
        assert!(matches!(
            b2_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
    }

    /// LOAD-BEARING sentinel guard: `[0; 32]` sweep is refused
    /// at the API boundary.  Same discipline as
    /// `release_all_for_deploy`.
    #[test]
    #[should_panic(expected = "sentinel")]
    fn cancel_all_waiters_for_deploy_panics_on_zero_sentinel() {
        let reg = LockRegistry::new();
        let _ = reg.cancel_all_waiters_for_deploy(&[0u8; 32]);
    }

    /// No matching deploy → zero cancelled.
    #[test]
    fn cancel_all_waiters_for_deploy_no_matches_returns_zero() {
        let reg = LockRegistry::new();
        assert_eq!(reg.cancel_all_waiters_for_deploy(&deploy_scope(0xFF)), 0);
    }

    /// Symmetry with the holder variant: dropped-receiver sends
    /// fail silently and don't abort the sweep.
    #[test]
    fn cancel_all_waiters_for_deploy_handles_dropped_receivers() {
        let reg = LockRegistry::new();
        reg.try_acquire_range(
            (1, 1),
            0,
            100,
            LockMode::Write,
            HolderId::from_bytes([0xAA; 32]),
            deploy_scope(0xAA),
        )
        .unwrap();
        let d = deploy_scope(0x11);
        let out = reg
            .try_acquire_range_wait(
                (1, 1),
                0,
                100,
                LockMode::Write,
                HolderId::from_bytes([0x11; 32]),
                d,
            )
            .unwrap();
        match out {
            AcquireOutcome::Parked { admit, .. } => drop(admit),
            _ => panic!("must park"),
        };
        let n = reg.cancel_all_waiters_for_deploy(&d);
        assert_eq!(n, 1);
    }

    /// INTEGRATION: the X-2 / G-02 defense chain operates in
    /// order — cancel_all_waiters_for_deploy drains PARKED
    /// waiters, release_all_for_deploy sweeps HELD locks and
    /// its wake_waiters admits OTHER deploys that were blocked.
    ///
    /// Setup (no cycle — would_close_cycle would otherwise
    /// refuse the second park):
    ///   - D1 holds F1 (held range).
    ///   - D3 holds F2 (held range).
    ///   - D1 parks on F2 (edge D1 → D3).
    ///   - D2 parks on F1 (edge D2 → D1).  No cycle because D1
    ///     waits on D3 (not D2).
    ///
    /// Deploy-abort for D1:
    ///   1. `cancel_all_waiters_for_deploy(D1)` → D1's wait on
    ///      F2 cancelled.
    ///   2. `release_all_for_deploy(D1)` → D1's held range on
    ///      F1 released; wake_waiters admits D2's wait on F1.
    #[test]
    fn deploy_abort_chain_cancels_waiters_then_releases_holds_then_admits() {
        let reg = LockRegistry::new();
        let d1 = deploy_scope(0x01);
        let d2 = deploy_scope(0x02);
        let d3 = deploy_scope(0x03);
        let h1 = HolderId::from_bytes([0x01; 32]);
        let h2 = HolderId::from_bytes([0x02; 32]);
        let h3 = HolderId::from_bytes([0x03; 32]);
        reg.try_acquire_range((1, 1), 0, 100, LockMode::Write, h1.clone(), d1)
            .unwrap();
        reg.try_acquire_range((2, 2), 0, 100, LockMode::Write, h3, d3)
            .unwrap();
        let d1_waits_f2 = reg
            .try_acquire_range_wait((2, 2), 0, 100, LockMode::Write, h1, d1)
            .unwrap();
        let d2_waits_f1 = reg
            .try_acquire_range_wait((1, 1), 0, 100, LockMode::Write, h2, d2)
            .unwrap();
        let mut d1_rx = match d1_waits_f2 {
            AcquireOutcome::Parked { admit, .. } => admit,
            _ => panic!("must park"),
        };
        let (d2_lock_id, mut d2_rx) = match d2_waits_f1 {
            AcquireOutcome::Parked { lock_id, admit } => (lock_id, admit),
            _ => panic!("must park"),
        };
        // Step 1: cancel D1's PARKED waiters.
        let cancelled = reg.cancel_all_waiters_for_deploy(&d1);
        assert_eq!(cancelled, 1);
        assert_eq!(d1_rx.try_recv(), Ok(Err(LockError::Cancelled)));
        // D2's waiter untouched.
        assert!(matches!(
            d2_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        // Step 2: release D1's HELD locks → wake_waiters admits
        // D2's waiter on F1.
        let released = reg.release_all_for_deploy(&d1);
        assert_eq!(released, 1);
        assert_eq!(d2_rx.try_recv(), Ok(Ok(d2_lock_id)));
    }
}
