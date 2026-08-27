//! Per-native cost weights for the `rho:io:fs:native:*` handlers.
//!
//! # Overview
//!
//! Every `rho:io:fs:native:*` handler emits a
//! `BillableTokenEvent::Primitive` at handler entry carrying a weight
//! derived from the helpers in this module.  The `Cost` returned here
//! is what `metering.reserve_primitive(...)` charges against the
//! deploy's `RuntimeBudget`.
//!
//! Weights are calibrated against `equality_check_cost` from
//! [`accounting::costs`] — a 100-unit weight is roughly the cost of
//! comparing two 100-byte-encoded terms.  Constant-work handlers
//! (`open`, `close`, `stat`, `exists`, `chmod`, `chown`, `seek`,
//! `tell`, `size`, `truncate`, `flush`, `quarantine`, `lock_range`,
//! `lock_sequential`, `release_lock`) all share [`FS_SYSCALL_CONST`].
//! Bytes-transferred and dir-entry handlers add a linear term.
//!
//! # Consensus discipline
//!
//! Under D3 (cost-accounted-rho), every weight in this file is a
//! **consensus parameter**.  Two validators running with different
//! weights would compute divergent `authority_cost_witness.realized`
//! values and reject each other's blocks.  Changes require a
//! coordinated hard-fork block-height activation across every
//! validator, coordinated per `cost-accounting-migration.md` §6.
//!
//! The golden-value regression pins in
//! `rholang/tests/fileio_cost_spec.rs` lock every weight defined
//! here.  A change to any weight MUST bump the corresponding golden
//! pin as an intentional acknowledgment; a silent drift trips CI.
//!
//! # Reference
//!
//! `implementation-plan.md` §Phase 9 (line ~1213) — canonical
//! weight table for the 22 native syscalls plus stream-lifetime
//! variants.
//!
//! # Constant vs. length-parameterized helpers
//!
//! Length-parameterized helpers (`fs_read_cost(bytes_read)`,
//! `fs_write_cost(bytes_written)`, `fs_entries_cost(n_entries)`,
//! `fs_remove_dir_cost(subtree_entry_count)`) MUST be charged via
//! `metering.reserve_incremental_primitive` when the length argument
//! can legitimately be zero (empty read return, zero-length write,
//! empty directory), matching the discipline established for
//! `concat_bytes_cost` in `reduce.rs`.  Constant-work helpers use
//! `metering.reserve_primitive` because their weight is always
//! positive by construction.

use crate::rust::interpreter::accounting::costs::Cost;

/// Base weight class for constant-work syscalls.  Calibrated against
/// `equality_check_cost` such that a 100-unit charge is roughly the
/// cost of comparing two 100-byte-encoded terms.  The plan doc
/// (§Phase 9, line ~1213) calls out "~100 in legacy
/// `equality_check_cost` units" — this is the D3 canonical value.
///
/// Consensus-critical.  Do NOT change without coordinated hard-fork
/// activation and a golden-pin acknowledgment in
/// `fileio_cost_spec.rs`.
pub const FS_SYSCALL_CONST: i64 = 100;

/// Base weight class for path-mutation syscalls (`rename`,
/// `copy_file`, `remove_file`).  Doubled vs. `FS_SYSCALL_CONST`
/// because path mutations touch two directory entries in the
/// worst case (source unlink + destination create) and validate
/// against the trusted-root policy on both endpoints.
pub const FS_PATH_MUTATION_CONST: i64 = 200;

/// Per-entry incremental weight for `fs_entries` (directory
/// enumeration) and `fs_remove_dir` (recursive removal).  A
/// dir-entry record is ~32 bytes of encoded material (name +
/// stat-like fields under Consensus mode), so charging 32 per
/// entry keeps `entries` in the same order-of-magnitude as an
/// equality check over the same encoded bytes.
pub const FS_ENTRIES_PER_ENTRY: i64 = 32;

/// Fixed setup weight for `fs_entries` — dispatch, path lookup,
/// handle bookkeeping.  Amortized across the per-entry cost.
pub const FS_ENTRIES_SETUP: i64 = 50;

// -------- Constant-work syscalls (all charge `FS_SYSCALL_CONST`) --------

pub fn fs_open_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_open") }

pub fn fs_close_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_close") }

pub fn fs_stat_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_stat") }

pub fn fs_exists_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_exists") }

pub fn fs_chmod_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_chmod") }

pub fn fs_chown_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_chown") }

pub fn fs_seek_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_seek") }

pub fn fs_tell_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_tell") }

pub fn fs_size_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_size") }

pub fn fs_truncate_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_truncate") }

pub fn fs_flush_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_flush") }

pub fn fs_quarantine_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_quarantine") }

/// `fs_lock_range` — both immediate (`wait:false`) and each
/// `wait:true` acquisition attempt that resolves emit a single
/// primitive event at this weight.  See
/// `implementation-plan.md:900` for the rationale on why this is
/// NOT scaled by interval-tree lookup cost (would leak an internal
/// data structure choice into consensus).  Under `wait:true`, this
/// weight fires once per resume (successful acquire, cancellation,
/// or timeout), not per idle-tick.
pub fn fs_lock_range_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_lock_range") }

pub fn fs_lock_sequential_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_lock_sequential") }

pub fn fs_release_lock_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_release_lock") }

// -------- Path-mutation syscalls (all charge `FS_PATH_MUTATION_CONST`) --------

pub fn fs_rename_cost() -> Cost { Cost::create(FS_PATH_MUTATION_CONST, "fs_rename") }

pub fn fs_copy_file_cost() -> Cost { Cost::create(FS_PATH_MUTATION_CONST, "fs_copy_file") }

pub fn fs_remove_file_cost() -> Cost { Cost::create(FS_PATH_MUTATION_CONST, "fs_remove_file") }

// -------- Length-parameterized syscalls --------

/// Compute `base + coefficient * argument` with saturating
/// arithmetic and clamp to `i64::MAX`.  Every length-parameterized
/// cost helper delegates here so the overflow-safety property is
/// enforced in one place.
///
/// # Why saturating
///
/// `argument` is derived from user-controlled Rholang input (a byte
/// count or entry count in a syscall request).  A naive `base +
/// coefficient * argument as i64` wraps to a negative value at
/// `argument ≈ 2^63 / coefficient` — which then trips
/// `reserve_primitive`'s `amount.value <= 0` guard and crashes the
/// deploy with `BugFoundError("Billable metering cost must be
/// positive")`.  That is a *soft* DoS (controlled crash, no state
/// corruption) but still an unnecessary exposure; the MVP defense
/// (per-call size caps in `mod.rs::MAX_READ_BYTES` etc.) requires
/// caller discipline that is easy to miss during slice 9b handler
/// wiring.
///
/// By saturating at `i64::MAX`, an adversarial length simply
/// produces the maximum billable cost — which any finite budget
/// rejects — without going through the crash path.  Callers may
/// (and should) still enforce per-call byte caps upstream for
/// spec-conformance reasons, but the cost helper is defense-in-
/// depth: it stays valid under any `u64` input.
///
/// # Consensus discipline
///
/// The saturation ceiling (`i64::MAX`) is a consensus parameter.
/// Two validators MUST agree on it byte-for-byte; using the Rust
/// stdlib constant makes this trivially portable.  The `debug_assert`
/// on `base >= 0` is a construction-time invariant on the compile-
/// time constants declared above, not on runtime input, so it
/// cannot cause validator divergence.
#[inline]
fn saturate_linear(base: i64, coefficient: u64, argument: u64) -> i64 {
    debug_assert!(base >= 0, "base weight must be non-negative");
    let scaled = coefficient.saturating_mul(argument);
    let sum = (base as u64).saturating_add(scaled);
    sum.min(i64::MAX as u64) as i64
}

/// `fs_read(len)` and `fs_read_at(offset, len)` — dispatch cost
/// plus one unit per byte read.  Byte-return values can legitimately
/// be zero (end-of-file), so callers MUST charge via
/// `reserve_incremental_primitive` on the pre-computed byte count
/// available before the syscall (`min(requested, remaining)`).
///
/// Note the pre-charge boundary: the byte count charged is the
/// requested count, not the actually-returned count.  This is
/// deliberate — an EOF-truncated read still burns the requested
/// bytes at handler entry to avoid a mispredicted-read amplification
/// vector where a caller requests megabytes at zero cost by
/// pre-seeking past EOF.
pub fn fs_read_cost(bytes_read: u64) -> Cost {
    Cost::create(saturate_linear(FS_SYSCALL_CONST, 1, bytes_read), "fs_read")
}

pub fn fs_read_at_cost(bytes_read: u64) -> Cost {
    Cost::create(
        saturate_linear(FS_SYSCALL_CONST, 1, bytes_read),
        "fs_read_at",
    )
}

/// `fs_write(bytes)` and `fs_write_at(offset, bytes)` — dispatch
/// cost plus two units per byte written.  The 2× multiplier vs.
/// read reflects the WAL-append cost on consensus caps (the write
/// hits both the underlying handler AND the per-runtime WAL);
/// non-consensus (oracular) caps still charge the same weight to
/// keep consensus and oracular deploys byte-for-byte comparable.
pub fn fs_write_cost(bytes_written: u64) -> Cost {
    Cost::create(
        saturate_linear(FS_SYSCALL_CONST, 2, bytes_written),
        "fs_write",
    )
}

pub fn fs_write_at_cost(bytes_written: u64) -> Cost {
    Cost::create(
        saturate_linear(FS_SYSCALL_CONST, 2, bytes_written),
        "fs_write_at",
    )
}

/// `fs_entries(dir)` — setup cost plus per-entry cost.  Charge via
/// `reserve_incremental_primitive` because an empty directory
/// legitimately produces zero-entry output (`50 + 0*32 = 50` still
/// positive so this is defensive; but future changes to
/// `FS_ENTRIES_SETUP` could hit zero).
pub fn fs_entries_cost(n_entries: u64) -> Cost {
    Cost::create(
        saturate_linear(FS_ENTRIES_SETUP, FS_ENTRIES_PER_ENTRY as u64, n_entries),
        "fs_entries",
    )
}

/// `fs_entries_stream(dir)` — same shape as `fs_entries_cost`.
/// The streaming variant amortizes the setup+per-entry cost across
/// resume events rather than charging up-front, but the total-over-
/// the-stream weight matches `fs_entries_cost(total_delivered)`.
/// Phase 9 slice 9c will refine this into per-resume increments
/// when the stream-methods layer is wired.
pub fn fs_entries_stream_cost(n_entries: u64) -> Cost {
    Cost::create(
        saturate_linear(FS_ENTRIES_SETUP, FS_ENTRIES_PER_ENTRY as u64, n_entries),
        "fs_entries_stream",
    )
}

/// Streaming-backing slice (2026-08-25): per-handler cost aliases for
/// the three natives that back the per-fd streaming primitive.  Each
/// alias exists to satisfy the naming pin in
/// `fileio_cost_spec::every_fs_handler_charges_its_cost_helper` —
/// a load-bearing static check that each `fs_X` handler references
/// `costs::fs_X_cost(...)` in its body, so a future refactor that
/// deletes the charge site is caught at test-time.  Under D3 a
/// missing handler charge is a leader/replay consensus divergence.
///
/// Semantically these delegate to the pre-existing shapes:
/// - `fs_entries_stream_open_cost` = `fs_entries_stream_cost(0)`
///   (setup only; per-entry supplement is fs_entries_stream_next).
/// - `fs_entries_stream_next_cost` = `fs_entries_stream_cost(0)`
///   (per-call setup; per-entry supplement charged separately via
///   `fs_entries_stream_per_entry_supplement_cost`).
/// - `fs_entries_stream_close_cost` = `fs_close_cost()` (fd release
///   is a hashmap remove + closedir, same class as fs_close).
pub fn fs_entries_stream_open_cost() -> Cost { fs_entries_stream_cost(0) }
pub fn fs_entries_stream_next_cost() -> Cost { fs_entries_stream_cost(0) }
pub fn fs_entries_stream_close_cost() -> Cost { fs_close_cost() }

/// `fs_remove_dir` recursive — path-mutation base plus per-entry
/// cost across the subtree.  Under D3 canonical accounting, the
/// per-entry count is measured (not estimated) — the handler
/// enumerates the subtree once and passes the count into this
/// helper before performing the recursive removal.  This makes
/// the charge deterministic and byte-identical across validators.
pub fn fs_remove_dir_cost(subtree_entry_count: u64) -> Cost {
    Cost::create(
        saturate_linear(
            FS_PATH_MUTATION_CONST,
            FS_ENTRIES_PER_ENTRY as u64,
            subtree_entry_count,
        ),
        "fs_remove_dir",
    )
}

/// `fs_release_all_for_holder` — administrative primitive that
/// sweeps every lock held by a given holder identifier.  Charged
/// at the same constant class as `fs_release_lock` since the
/// per-entry work is amortized across the holder's typical lock
/// count (small integer) and pricing sub-linear here would leak
/// interval-tree internals into consensus.
pub fn fs_release_all_for_holder_cost() -> Cost {
    Cost::create(FS_SYSCALL_CONST, "fs_release_all_for_holder")
}

// -------- Two-branch per-entry supplements (slice 9b-iv follow-up) --------
//
// The entries-family handlers charge in two `reserve_primitive` calls:
// the first at handler entry (setup only via `fs_<name>_cost(0)`),
// the second after the reply is known (per-entry supplement scaled by
// the entry count).  Both branches (leader from syscall result,
// replay from `previous`) emit the same two events with the same
// weights in the same order, keeping the D3 canonical event log
// byte-identical.  Sum of the two charges MUST equal
// `fs_<name>_cost(n_entries)` — verified in
// `fileio_cost_spec::entries_family_supplement_matches_combined_cost`.
//
// Wired: `fs_entries` charges its supplement post-reply.  `fs_remove_dir`
// wired 2026-08-27 after the H-29-3 lift added a manifest to the
// recursive Consensus reply — leader charges from the walk's actual
// deletion count; follower extracts the same count from `previous`'s
// manifest.  Oracular recursive skips the supplement (no wire-visible
// count — see `fs_remove_dir_per_entry_supplement_cost` docstring).
// Deferred: `fs_entries_stream` (still a stub returning
// FSERR_UNSUPPORTED — no entries to charge for).

/// Per-entry supplement for the `fs_entries` two-branch charge.
/// Sits alongside `fs_entries_cost(0)` (the setup component at
/// handler entry) as a second `reserve_primitive` call executed
/// after the entry count is knowable — leader from the fresh
/// reply; replay from `previous`.  Sum matches `fs_entries_cost(n)`.
pub fn fs_entries_per_entry_supplement_cost(n_entries: u64) -> Cost {
    Cost::create(
        saturate_linear(0, FS_ENTRIES_PER_ENTRY as u64, n_entries),
        "fs_entries_per_entry",
    )
}

/// Per-entry supplement for `fs_entries_stream` — same shape as
/// `fs_entries_per_entry_supplement_cost`.  Deferred wiring: the
/// current stub returns `FSERR_UNSUPPORTED` unconditionally, so
/// no supplement charge is needed (n = 0 always).  Ready-to-use
/// helper for when the streaming backing lands.
pub fn fs_entries_stream_per_entry_supplement_cost(n_entries: u64) -> Cost {
    Cost::create(
        saturate_linear(0, FS_ENTRIES_PER_ENTRY as u64, n_entries),
        "fs_entries_stream_per_entry",
    )
}

/// Per-entry supplement for `fs_remove_dir` — same shape.  Wired
/// post-H-29-3-lift (2026-08-27) via the manifest carried by the
/// recursive Consensus reply.  Per-branch charge derivation:
///
/// * Non-recursive (any cmode): 1 attempted entry — supplement =
///   `fs_remove_dir_per_entry_supplement_cost(1)` on both branches.
/// * Recursive Consensus: leader derives count from its walk;
///   follower derives the same count from `extract_removedir_manifest
///   (previous)`.  Both sides charge identically.
/// * Recursive Oracular: 0 (both sides).  The Oracular reply is
///   `[true]` and doesn't carry a count; rather than change the
///   Oracular reply shape or introduce cost-asymmetry across the
///   leader/follower split, we accept the under-charge as an
///   Oracular-scope pricing choice.  Oracular operations are per-
///   validator-local by design (plan §Storage cases: "operators do
///   what they want"), so exact cost fidelity here isn't a
///   consensus concern.
pub fn fs_remove_dir_per_entry_supplement_cost(subtree_entry_count: u64) -> Cost {
    Cost::create(
        saturate_linear(0, FS_ENTRIES_PER_ENTRY as u64, subtree_entry_count),
        "fs_remove_dir_per_entry",
    )
}
