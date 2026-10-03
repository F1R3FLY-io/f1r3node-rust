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
// This slice (Wave 4 PR 4.2) ports constant-weight helpers only.
// Length-parameterized helpers (`fs_read_cost(bytes_read)`,
// `fs_write_cost(bytes_written)`, `fs_entries_cost(n_entries)`,
// `fs_remove_dir_cost(subtree_entry_count)`) are deferred to the
// next slice (Wave 4 PR 4.3) — they share the saturating-arithmetic
// helper `saturate_linear` which is best reviewed as its own unit.
// Constant-work helpers use `metering.reserve_primitive` because
// their weight is always positive by construction.

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

pub fn fs_copy_file_cost() -> Cost { Cost::create(FS_PATH_MUTATION_CONST, "fs_copy_file") }

pub fn fs_remove_file_cost() -> Cost { Cost::create(FS_PATH_MUTATION_CONST, "fs_remove_file") }

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
        ];
        let ops: Vec<&str> = costs.iter().map(|c| c.operation.as_ref()).collect();

        let mut sorted = ops.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ops.len(), "duplicate operation string");
    }
}
