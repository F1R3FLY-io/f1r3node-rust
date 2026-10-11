// Per-native cost weights for the `rho:io:fs:native:*` handlers.
//
// # Overview
//
// Every `rho:io:fs:native:*` handler emits a cost reservation at
// handler entry carrying a weight derived from the helpers in this
// module.  The `Cost` returned here is what
// `metering.reserve_primitive(...)` charges against the deploy's
// budget under Wave 6; under Wave 4 + 5 the stub
// [`crate::rust::interpreter::accounting::noop::NoopMetering`]
// accepts every weight as a no-op (zero-cost dispatch).
//
// Weights are calibrated against `equality_check_cost` from
// [`accounting::costs`] — a 100-unit weight is roughly the cost of
// comparing two 100-byte-encoded terms.  Constant-work handlers
// (`open`, `close`, `stat`, `exists`, `chmod`, `chown`, `seek`,
// `tell`, `size`, `truncate`, `flush`, `quarantine`, `lock_range`,
// `lock_sequential`, `release_lock`, `release_all_for_holder`) all
// share [`FS_SYSCALL_CONST`].  Path-mutation handlers (`rename`,
// `copy_file`, `remove_file`) share [`FS_PATH_MUTATION_CONST`].
// Bytes-transferred and dir-entry handlers add a linear term (yet
// to land in a subsequent slice).
//
// # Consensus discipline
//
// Under Wave 6 (cost-accounted-rho), every weight in this file is a
// **consensus parameter**.  Two validators running with different
// weights would compute divergent cost witnesses and reject each
// other's blocks.  Changes require a coordinated hard-fork activation
// across every validator.
//
// Under Wave 4 + 5, weights are called but NoopMetering discards
// them — divergent weights across validators are byte-equivalent at
// consensus (both read zero cost).  Wave 6 lights up the enforcement.
//
// The golden-value regression tests in this module lock every weight
// defined here.  A change to any weight MUST bump the corresponding
// golden pin as an intentional acknowledgment; a silent drift trips
// CI.
//
// # Constant vs. length-parameterized helpers
//
// Constant-work helpers (fs_open, fs_close, fs_stat, ...) use
// `metering.reserve_primitive` because their weight is always
// positive by construction.  Length-parameterized helpers
// (`fs_read_cost(bytes_read)`, `fs_write_cost(bytes_written)`,
// `fs_entries_cost(n_entries)`, `fs_remove_dir_cost(subtree_entry_count)`)
// MUST be charged via `metering.reserve_incremental_primitive` when
// the length argument can legitimately be zero (empty-read return,
// zero-length write, empty directory, empty subtree), matching the
// discipline established for the trait's incremental method
// (see `accounting/noop.rs`).  All length-parameterized helpers
// delegate to `saturate_linear` for overflow-safe arithmetic on
// adversarial inputs — see that function's docstring for the
// defense-in-depth rationale.

use crate::rust::interpreter::accounting::costs::Cost;

/// Base weight class for constant-work syscalls.  Calibrated against
/// `equality_check_cost` such that a 100-unit charge is roughly the
/// cost of comparing two 100-byte-encoded terms.
///
/// Consensus-critical under Wave 6.  Do NOT change without
/// coordinated hard-fork activation and a golden-pin acknowledgment
/// in this module's test suite.
pub const FS_SYSCALL_CONST: i64 = 100;

/// Base weight class for path-mutation syscalls (`rename`,
/// `copy_file`, `remove_file`).  Doubled vs. `FS_SYSCALL_CONST`
/// because path mutations touch two directory entries in the worst
/// case (source unlink + destination create) and validate against
/// the trusted-root policy on both endpoints.
pub const FS_PATH_MUTATION_CONST: i64 = 200;

/// Per-entry incremental weight for `fs_entries` (directory
/// enumeration) and `fs_remove_dir` (recursive removal).  A
/// dir-entry record is ~32 bytes of encoded material (name + stat-
/// like fields under Consensus mode), so charging 32 per entry
/// keeps `entries` in the same order-of-magnitude as an equality
/// check over the same encoded bytes.
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
/// primitive event at this weight.  NOT scaled by interval-tree
/// lookup cost (would leak an internal data structure choice into
/// consensus).  Under `wait:true`, this weight fires once per resume
/// (successful acquire, cancellation, or timeout), not per idle-tick.
pub fn fs_lock_range_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_lock_range") }

pub fn fs_lock_sequential_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_lock_sequential") }

pub fn fs_release_lock_cost() -> Cost { Cost::create(FS_SYSCALL_CONST, "fs_release_lock") }

/// `fs_release_all_for_holder` — administrative primitive that
/// sweeps every lock held by a given holder identifier.  Charged at
/// the same constant class as `fs_release_lock` since the per-entry
/// work is amortized across the holder's typical lock count (small
/// integer) and pricing sub-linear here would leak interval-tree
/// internals into consensus.
pub fn fs_release_all_for_holder_cost() -> Cost {
    Cost::create(FS_SYSCALL_CONST, "fs_release_all_for_holder")
}

// -------- Path-mutation syscalls (all charge `FS_PATH_MUTATION_CONST`) --------

pub fn fs_rename_cost() -> Cost { Cost::create(FS_PATH_MUTATION_CONST, "fs_rename") }

pub fn fs_bulk_apply_cost() -> Cost { Cost::create(FS_PATH_MUTATION_CONST, "fs_bulk_apply") }

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
/// count or entry count in a syscall request).  A naive
/// `base + coefficient * argument as i64` wraps to a negative value
/// at `argument ≈ 2^63 / coefficient` — which would then trip
/// `reserve_primitive`'s eventual `amount.value <= 0` guard
/// (Wave 6) and crash the deploy with `BugFoundError`.  That is a
/// *soft* DoS (controlled crash, no state corruption) but still an
/// unnecessary exposure.
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
/// stdlib constant makes this trivially portable.  The
/// `debug_assert` on `base >= 0` is a construction-time invariant
/// on the compile-time constants declared above, not on runtime
/// input, so it cannot cause validator divergence.
#[inline]
fn saturate_linear(base: i64, coefficient: u64, argument: u64) -> i64 {
    debug_assert!(base >= 0, "base weight must be non-negative");
    let scaled = coefficient.saturating_mul(argument);
    let sum = (base as u64).saturating_add(scaled);
    sum.min(i64::MAX as u64) as i64
}

/// `fs_read(len)` — dispatch cost plus one unit per byte read.
/// Byte-return values can legitimately be zero (end-of-file), so
/// callers MUST charge via `reserve_incremental_primitive` on the
/// pre-computed byte count available before the syscall
/// (`min(requested, remaining)`).
///
/// Note the pre-charge boundary: the byte count charged is the
/// *requested* count, not the actually-returned count.  Deliberate:
/// an EOF-truncated read still burns the requested bytes at handler
/// entry to avoid a mispredicted-read amplification vector where a
/// caller requests megabytes at zero cost by pre-seeking past EOF.
pub fn fs_read_cost(bytes_read: u64) -> Cost {
    Cost::create(saturate_linear(FS_SYSCALL_CONST, 1, bytes_read), "fs_read")
}

pub fn fs_read_at_cost(bytes_read: u64) -> Cost {
    Cost::create(
        saturate_linear(FS_SYSCALL_CONST, 1, bytes_read),
        "fs_read_at",
    )
}

/// `fs_write(bytes)` — dispatch cost plus two units per byte
/// written.  The 2× multiplier vs. read reflects the WAL-append
/// cost on consensus caps (the write hits both the underlying
/// handler AND the per-runtime WAL); non-consensus (oracular) caps
/// still charge the same weight to keep consensus and oracular
/// deploys byte-for-byte comparable.
///
/// Pre-charge boundary matches [`fs_read_cost`]: `bytes_written`
/// is the caller's *requested* write size, not the actually-
/// performed count.  Charging on the request-time count prevents
/// a short-write amplification vector symmetric to the
/// mispredicted-read vector `fs_read_cost` closes.
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
/// legitimately produces zero-entry output (50 + 0*32 = 50 is still
/// positive here, so this is defensive; a future tightening of
/// `FS_ENTRIES_SETUP` toward zero would hit a legitimate zero).
pub fn fs_entries_cost(n_entries: u64) -> Cost {
    Cost::create(
        saturate_linear(FS_ENTRIES_SETUP, FS_ENTRIES_PER_ENTRY as u64, n_entries),
        "fs_entries",
    )
}

/// Per-fd streaming open — setup-only weight (per-entry cost is
/// charged separately via `fs_entries_stream_per_entry_supplement_cost`
/// once the row count is known).
pub fn fs_entries_stream_open_cost() -> Cost {
    Cost::create(FS_ENTRIES_SETUP, "fs_entries_stream_open")
}

/// Per-fd streaming next — setup-only weight; per-row cost comes
/// through the two-branch supplement after the row count is yielded.
pub fn fs_entries_stream_next_cost() -> Cost {
    Cost::create(FS_ENTRIES_SETUP, "fs_entries_stream_next")
}

/// Per-fd streaming close — alias for `fs_close_cost()`.  Close is
/// a hashmap-remove + `closedir`, same class as `fs_close`.
pub fn fs_entries_stream_close_cost() -> Cost { fs_close_cost() }

/// `fs_remove_dir` recursive — path-mutation base plus per-entry
/// cost across the subtree.  Under canonical accounting, the
/// per-entry count is *measured* (not estimated) — the handler
/// enumerates the subtree once and passes the count into this
/// helper before performing the recursive removal.  This makes the
/// charge deterministic and byte-identical across validators.
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

// -------- Two-branch per-entry supplements --------
//
// The entries-family handlers charge in two `reserve_*_primitive`
// calls: the first at handler entry (setup only via
// `fs_<name>_cost(0)`), the second after the reply is known
// (per-entry supplement scaled by the entry count).  Both branches
// (leader from syscall result, replay from previous event log) emit
// the same two events with the same weights in the same order,
// keeping the Wave 6 event log byte-identical.  Sum of the two
// charges MUST equal `fs_<name>_cost(n_entries)` — verified by the
// `entries_family_supplement_matches_combined_cost` test below.

/// Per-entry supplement for the `fs_entries` two-branch charge.
/// Sits alongside `fs_entries_cost(0)` (the setup component at
/// handler entry) as a second `reserve_incremental_primitive` call
/// executed after the entry count is knowable.  Sum matches
/// `fs_entries_cost(n)`.
pub fn fs_entries_per_entry_supplement_cost(n_entries: u64) -> Cost {
    Cost::create(
        saturate_linear(0, FS_ENTRIES_PER_ENTRY as u64, n_entries),
        "fs_entries_per_entry",
    )
}

/// Per-entry supplement for the per-fd streaming
/// (`fs_entries_stream_next`) two-branch charge — same shape as
/// `fs_entries_per_entry_supplement_cost`.  Fires once per `_next`
/// yield; sum across all yields of a stream matches the equivalent
/// `fs_entries_cost(total_rows)`.
pub fn fs_entries_stream_per_entry_supplement_cost(n_entries: u64) -> Cost {
    Cost::create(
        saturate_linear(0, FS_ENTRIES_PER_ENTRY as u64, n_entries),
        "fs_entries_stream_per_entry",
    )
}

/// Per-entry supplement for `fs_remove_dir`.  Fires on both leader
/// (from the walk's actual deletion count) and follower (from the
/// replay-visible manifest / count) so the Wave 6 event log stays
/// byte-identical.  Sum with `fs_remove_dir_cost(0)` matches
/// `fs_remove_dir_cost(n)`.
pub fn fs_remove_dir_per_entry_supplement_cost(subtree_entry_count: u64) -> Cost {
    Cost::create(
        saturate_linear(0, FS_ENTRIES_PER_ENTRY as u64, subtree_entry_count),
        "fs_remove_dir_per_entry",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------- Constant pins (consensus-observable) --------
    //
    // These pins lock the base weight classes.  A change to either
    // constant MUST be reflected in a Wave-6 hard-fork activation;
    // under Wave 4 + 5 NoopMetering discards weights, so a silent
    // drift here is a byte-equivalent change at consensus but still
    // a programming error waiting to light up at Wave 6.

    #[test]
    fn fs_syscall_const_is_100() {
        assert_eq!(FS_SYSCALL_CONST, 100);
    }

    #[test]
    fn fs_path_mutation_const_is_200() {
        assert_eq!(FS_PATH_MUTATION_CONST, 200);
    }

    #[test]
    fn fs_path_mutation_is_double_fs_syscall() {
        assert_eq!(FS_PATH_MUTATION_CONST, FS_SYSCALL_CONST * 2);
    }

    // -------- Constant-work syscall golden pins --------
    //
    // Each pin asserts BOTH the value AND the operation string.  The
    // operation string is NOT a consensus parameter but IS a
    // debugging surface — a rename there would silently break log /
    // trace / cost-event reconciliation across the handler + weight
    // layers.

    fn assert_cost(c: Cost, value: i64, op: &str) {
        assert_eq!(c.value, value, "value for {}", op);
        assert_eq!(c.operation, op, "operation string");
    }

    #[test]
    fn fs_open_cost_pin() { assert_cost(fs_open_cost(), 100, "fs_open"); }

    #[test]
    fn fs_close_cost_pin() { assert_cost(fs_close_cost(), 100, "fs_close"); }

    #[test]
    fn fs_stat_cost_pin() { assert_cost(fs_stat_cost(), 100, "fs_stat"); }

    #[test]
    fn fs_exists_cost_pin() { assert_cost(fs_exists_cost(), 100, "fs_exists"); }

    #[test]
    fn fs_chmod_cost_pin() { assert_cost(fs_chmod_cost(), 100, "fs_chmod"); }

    #[test]
    fn fs_chown_cost_pin() { assert_cost(fs_chown_cost(), 100, "fs_chown"); }

    #[test]
    fn fs_seek_cost_pin() { assert_cost(fs_seek_cost(), 100, "fs_seek"); }

    #[test]
    fn fs_tell_cost_pin() { assert_cost(fs_tell_cost(), 100, "fs_tell"); }

    #[test]
    fn fs_size_cost_pin() { assert_cost(fs_size_cost(), 100, "fs_size"); }

    #[test]
    fn fs_truncate_cost_pin() { assert_cost(fs_truncate_cost(), 100, "fs_truncate"); }

    #[test]
    fn fs_flush_cost_pin() { assert_cost(fs_flush_cost(), 100, "fs_flush"); }

    #[test]
    fn fs_quarantine_cost_pin() { assert_cost(fs_quarantine_cost(), 100, "fs_quarantine"); }

    #[test]
    fn fs_lock_range_cost_pin() { assert_cost(fs_lock_range_cost(), 100, "fs_lock_range"); }

    #[test]
    fn fs_lock_sequential_cost_pin() {
        assert_cost(fs_lock_sequential_cost(), 100, "fs_lock_sequential");
    }

    #[test]
    fn fs_release_lock_cost_pin() { assert_cost(fs_release_lock_cost(), 100, "fs_release_lock"); }

    #[test]
    fn fs_release_all_for_holder_cost_pin() {
        assert_cost(
            fs_release_all_for_holder_cost(),
            100,
            "fs_release_all_for_holder",
        );
    }

    // -------- Path-mutation syscall golden pins --------

    #[test]
    fn fs_rename_cost_pin() { assert_cost(fs_rename_cost(), 200, "fs_rename"); }

    #[test]
    fn fs_copy_file_cost_pin() { assert_cost(fs_copy_file_cost(), 200, "fs_copy_file"); }

    #[test]
    fn fs_remove_file_cost_pin() { assert_cost(fs_remove_file_cost(), 200, "fs_remove_file"); }

    // -------- Operation-string uniqueness pin --------
    //
    // Each helper's operation string MUST be unique.  A duplicate
    // would collapse two different handlers' cost events into one
    // debugging surface — a leader/replay divergence detector that
    // depends on the operation string (e.g. log-based reconciliation)
    // would see spurious matches.

    #[test]
    fn all_operation_strings_are_unique() {
        let costs: Vec<Cost> = vec![
            fs_open_cost(),
            fs_close_cost(),
            fs_stat_cost(),
            fs_exists_cost(),
            fs_chmod_cost(),
            fs_chown_cost(),
            fs_seek_cost(),
            fs_tell_cost(),
            fs_size_cost(),
            fs_truncate_cost(),
            fs_flush_cost(),
            fs_quarantine_cost(),
            fs_lock_range_cost(),
            fs_lock_sequential_cost(),
            fs_release_lock_cost(),
            fs_release_all_for_holder_cost(),
            fs_rename_cost(),
            fs_copy_file_cost(),
            fs_remove_file_cost(),
            // Length-parameterized helpers — sample each at a
            // deterministic input so the operation string surfaces.
            fs_read_cost(0),
            fs_read_at_cost(0),
            fs_write_cost(0),
            fs_write_at_cost(0),
            fs_entries_cost(0),
            fs_entries_stream_open_cost(),
            fs_entries_stream_next_cost(),
            // fs_entries_stream_close_cost intentionally omitted —
            // it is a documented alias for fs_close_cost (sharing
            // the "fs_close" operation string is the invariant,
            // see `fs_entries_stream_close_is_fs_close` below).
            fs_remove_dir_cost(0),
            fs_entries_per_entry_supplement_cost(0),
            fs_entries_stream_per_entry_supplement_cost(0),
            fs_remove_dir_per_entry_supplement_cost(0),
        ];
        let ops: Vec<&str> = costs.iter().map(|c| c.operation.as_ref()).collect();

        let mut sorted = ops.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ops.len(), "duplicate operation string");
    }

    // -------- Additional-constant pins --------

    #[test]
    fn fs_entries_per_entry_is_32() {
        assert_eq!(FS_ENTRIES_PER_ENTRY, 32);
    }

    #[test]
    fn fs_entries_setup_is_50() {
        assert_eq!(FS_ENTRIES_SETUP, 50);
    }

    // -------- saturate_linear --------

    #[test]
    fn saturate_linear_basic_sum() {
        // 100 + 1 * 42 = 142
        assert_eq!(saturate_linear(100, 1, 42), 142);
        // 50 + 32 * 10 = 370
        assert_eq!(saturate_linear(50, 32, 10), 370);
        // 200 + 32 * 1 = 232
        assert_eq!(saturate_linear(200, 32, 1), 232);
    }

    #[test]
    fn saturate_linear_zero_argument_yields_base() {
        assert_eq!(saturate_linear(100, 1, 0), 100);
        assert_eq!(saturate_linear(50, 32, 0), 50);
        assert_eq!(saturate_linear(0, 32, 0), 0);
    }

    #[test]
    fn saturate_linear_zero_coefficient_yields_base() {
        assert_eq!(saturate_linear(100, 0, u64::MAX), 100);
    }

    /// LOAD-BEARING: adversarial inputs saturate at `i64::MAX`
    /// rather than wrap to a negative value or crash.  A regression
    /// that reintroduced naive arithmetic would turn a large-length
    /// request into a `BugFoundError` under Wave 6 — soft DoS.
    #[test]
    fn saturate_linear_adversarial_inputs_saturate_at_i64_max() {
        assert_eq!(saturate_linear(100, 1, u64::MAX), i64::MAX);
        assert_eq!(saturate_linear(100, 2, u64::MAX), i64::MAX);
        assert_eq!(saturate_linear(0, 32, u64::MAX), i64::MAX);
        // Coefficient * argument alone overflows u64 — must still saturate.
        assert_eq!(saturate_linear(0, u64::MAX, u64::MAX), i64::MAX);
    }

    #[test]
    fn saturate_linear_base_near_i64_max_saturates() {
        // Base alone at i64::MAX, any positive scaled term saturates.
        assert_eq!(saturate_linear(i64::MAX, 1, 1), i64::MAX);
        assert_eq!(saturate_linear(i64::MAX - 10, 1, 100), i64::MAX);
    }

    // -------- Length-parameterized golden pins --------
    //
    // Each helper is pinned at (0, 1, 1000) plus u64::MAX to anchor
    // the coefficient AND verify saturation at the adversarial end.

    #[test]
    fn fs_read_cost_pins() {
        assert_cost(fs_read_cost(0), 100, "fs_read");
        assert_cost(fs_read_cost(1), 101, "fs_read");
        assert_cost(fs_read_cost(1000), 1100, "fs_read");
        assert_eq!(fs_read_cost(u64::MAX).value, i64::MAX);
    }

    #[test]
    fn fs_read_at_cost_pins() {
        assert_cost(fs_read_at_cost(0), 100, "fs_read_at");
        assert_cost(fs_read_at_cost(1), 101, "fs_read_at");
        assert_cost(fs_read_at_cost(1000), 1100, "fs_read_at");
        assert_eq!(fs_read_at_cost(u64::MAX).value, i64::MAX);
    }

    #[test]
    fn fs_write_cost_pins() {
        // 2× coefficient vs. read.
        assert_cost(fs_write_cost(0), 100, "fs_write");
        assert_cost(fs_write_cost(1), 102, "fs_write");
        assert_cost(fs_write_cost(1000), 2100, "fs_write");
        assert_eq!(fs_write_cost(u64::MAX).value, i64::MAX);
    }

    #[test]
    fn fs_write_at_cost_pins() {
        assert_cost(fs_write_at_cost(0), 100, "fs_write_at");
        assert_cost(fs_write_at_cost(1), 102, "fs_write_at");
        assert_cost(fs_write_at_cost(1000), 2100, "fs_write_at");
        assert_eq!(fs_write_at_cost(u64::MAX).value, i64::MAX);
    }

    #[test]
    fn fs_entries_cost_pins() {
        // 50 setup + 32 * n_entries.
        assert_cost(fs_entries_cost(0), 50, "fs_entries");
        assert_cost(fs_entries_cost(1), 82, "fs_entries");
        assert_cost(fs_entries_cost(100), 3250, "fs_entries");
        assert_eq!(fs_entries_cost(u64::MAX).value, i64::MAX);
    }

    #[test]
    fn fs_entries_stream_open_cost_pin() {
        assert_cost(fs_entries_stream_open_cost(), 50, "fs_entries_stream_open");
    }

    #[test]
    fn fs_entries_stream_next_cost_pin() {
        assert_cost(fs_entries_stream_next_cost(), 50, "fs_entries_stream_next");
    }

    /// `fs_entries_stream_close_cost` is a documented alias for
    /// `fs_close_cost`.  Pinned by behavior AND operation string:
    /// a future refactor that silently drifts (e.g., gives
    /// close-stream its own operation name) would break log-based
    /// cost reconciliation.
    #[test]
    fn fs_entries_stream_close_is_fs_close() {
        let a = fs_entries_stream_close_cost();
        let b = fs_close_cost();
        assert_eq!(a.value, b.value);
        assert_eq!(a.operation, b.operation);
    }

    #[test]
    fn fs_remove_dir_cost_pins() {
        // 200 path-mutation base + 32 * entries.
        assert_cost(fs_remove_dir_cost(0), 200, "fs_remove_dir");
        assert_cost(fs_remove_dir_cost(1), 232, "fs_remove_dir");
        assert_cost(fs_remove_dir_cost(100), 3400, "fs_remove_dir");
        assert_eq!(fs_remove_dir_cost(u64::MAX).value, i64::MAX);
    }

    // -------- Two-branch supplement golden pins --------

    #[test]
    fn fs_entries_per_entry_supplement_cost_pins() {
        // Zero base; 32 * n_entries.
        assert_cost(
            fs_entries_per_entry_supplement_cost(0),
            0,
            "fs_entries_per_entry",
        );
        assert_cost(
            fs_entries_per_entry_supplement_cost(1),
            32,
            "fs_entries_per_entry",
        );
        assert_cost(
            fs_entries_per_entry_supplement_cost(100),
            3200,
            "fs_entries_per_entry",
        );
        assert_eq!(
            fs_entries_per_entry_supplement_cost(u64::MAX).value,
            i64::MAX
        );
    }

    #[test]
    fn fs_entries_stream_per_entry_supplement_cost_pins() {
        assert_cost(
            fs_entries_stream_per_entry_supplement_cost(0),
            0,
            "fs_entries_stream_per_entry",
        );
        assert_cost(
            fs_entries_stream_per_entry_supplement_cost(1),
            32,
            "fs_entries_stream_per_entry",
        );
        assert_cost(
            fs_entries_stream_per_entry_supplement_cost(100),
            3200,
            "fs_entries_stream_per_entry",
        );
    }

    #[test]
    fn fs_remove_dir_per_entry_supplement_cost_pins() {
        assert_cost(
            fs_remove_dir_per_entry_supplement_cost(0),
            0,
            "fs_remove_dir_per_entry",
        );
        assert_cost(
            fs_remove_dir_per_entry_supplement_cost(1),
            32,
            "fs_remove_dir_per_entry",
        );
        assert_cost(
            fs_remove_dir_per_entry_supplement_cost(100),
            3200,
            "fs_remove_dir_per_entry",
        );
    }

    // -------- Two-branch sum invariants (LOAD-BEARING) --------
    //
    // The two-branch pattern requires that the sum of the
    // handler-entry charge (`fs_<name>_cost(0)`) + the per-entry
    // supplement (`fs_<name>_per_entry_supplement_cost(n)`) equal
    // the combined-charge value (`fs_<name>_cost(n)`).  Under Wave 6
    // a divergence here would produce a different cost witness on
    // the one-branch handler path vs. the two-branch handler path,
    // crashing byte-equivalence between leader + replay.

    fn sample_entry_counts() -> Vec<u64> { vec![0, 1, 10, 100, 10_000] }

    #[test]
    fn fs_entries_supplement_matches_combined_cost() {
        for n in sample_entry_counts() {
            let setup = fs_entries_cost(0).value;
            let supplement = fs_entries_per_entry_supplement_cost(n).value;
            let combined = fs_entries_cost(n).value;
            assert_eq!(
                setup + supplement,
                combined,
                "entries two-branch sum invariant at n={}",
                n
            );
        }
    }

    #[test]
    fn fs_entries_stream_next_supplement_matches_entries_combined() {
        // fs_entries_stream_next_cost() == FS_ENTRIES_SETUP ==
        // fs_entries_cost(0).  Streaming yields a single row's
        // worth of supplement per `_next` call; summed across all
        // _next yields on a stream of n rows, the total equals
        // fs_entries_cost(n) (ignoring the open+close framing).
        for n in sample_entry_counts() {
            let setup = fs_entries_stream_next_cost().value;
            let supplement = fs_entries_stream_per_entry_supplement_cost(n).value;
            let combined = fs_entries_cost(n).value;
            assert_eq!(
                setup + supplement,
                combined,
                "stream-next two-branch sum invariant at n={}",
                n
            );
        }
    }

    #[test]
    fn fs_remove_dir_supplement_matches_combined_cost() {
        for n in sample_entry_counts() {
            let setup = fs_remove_dir_cost(0).value;
            let supplement = fs_remove_dir_per_entry_supplement_cost(n).value;
            let combined = fs_remove_dir_cost(n).value;
            assert_eq!(
                setup + supplement,
                combined,
                "remove_dir two-branch sum invariant at n={}",
                n
            );
        }
    }

    /// LOAD-BEARING: write charges 2× per byte vs. read.  A silent
    /// drift to equal coefficients would make writes under-charge
    /// relative to the WAL-append reality — Wave 6 budget leakage.
    #[test]
    fn fs_write_charges_double_fs_read_at_same_byte_count() {
        for n in [1u64, 10, 1000, 10_000] {
            let read = fs_read_cost(n).value;
            let write = fs_write_cost(n).value;
            // base common; delta is coefficient * n.
            assert_eq!(write - FS_SYSCALL_CONST, 2 * (read - FS_SYSCALL_CONST));
        }
    }
}
