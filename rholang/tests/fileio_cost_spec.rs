//! Phase 9 slice 9a — golden-value regression pins for every
//! `rho:io:fs:native:*` handler weight.
//!
//! # Purpose
//!
//! Under D3 (cost-accounted-rho, landed 2026-08-21), every handler
//! weight is a **consensus parameter**.  A silent drift — even a
//! one-byte typo like `100 → 101` — would cause validators running
//! different builds to compute divergent `authority_cost_witness.realized`
//! values and reject each other's blocks with
//! `InvalidCostSettlement("replay authority trace differs from the
//! committed witness")`.
//!
//! This spec locks every weight defined in
//! `rholang/src/rust/interpreter/io/costs.rs` at its canonical value.
//! Any change to a weight MUST bump the corresponding golden pin as
//! a deliberate acknowledgment (mirroring the `compose_fs_genesis_source_
//! golden_hex` discipline).  Silent drift trips CI.
//!
//! # Interpretation of failures
//!
//! * **Constant-cost pin fails**: someone changed the numeric weight
//!   in `costs.rs`.  Either (a) intended change — bump the golden and
//!   coordinate with hard-fork activation planning, or (b) accidental
//!   change — revert.
//! * **Linear-cost pin fails at zero-argument variant**: the linear
//!   term's coefficient changed (e.g. `read` shifted from
//!   `1 * bytes` to `2 * bytes`).  Same triage as above.
//! * **Linear-cost pin fails at non-zero-argument variant only**: the
//!   linear-term coefficient differs but the base weight matches.
//!   Rare; investigate.
//!
//! # Reference
//!
//! `implementation-plan.md` §Phase 9 (line ~1213) — canonical
//! weight table.

use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::io::costs::{
    fs_chmod_cost, fs_chown_cost, fs_close_cost, fs_copy_file_cost, fs_entries_cost,
    fs_entries_stream_cost, fs_exists_cost, fs_flush_cost, fs_lock_range_cost,
    fs_lock_sequential_cost, fs_open_cost, fs_quarantine_cost, fs_read_at_cost, fs_read_cost,
    fs_release_all_for_holder_cost, fs_release_lock_cost, fs_remove_dir_cost, fs_remove_file_cost,
    fs_rename_cost, fs_seek_cost, fs_size_cost, fs_stat_cost, fs_tell_cost, fs_truncate_cost,
    fs_write_at_cost, fs_write_cost, FS_ENTRIES_PER_ENTRY, FS_ENTRIES_SETUP,
    FS_PATH_MUTATION_CONST, FS_SYSCALL_CONST,
};

// -------- Weight-class constants -----------------------------------

/// `FS_SYSCALL_CONST` is the base weight for the constant-work
/// syscalls (`open`, `close`, `stat`, ...).  Calibrated against
/// `equality_check_cost` per `implementation-plan.md:900` at 100
/// units — roughly the cost of comparing two 100-byte-encoded
/// terms.
#[test]
fn fs_syscall_const_is_pinned_at_100() {
    assert_eq!(
        FS_SYSCALL_CONST, 100,
        "consensus-critical weight drift: FS_SYSCALL_CONST is a hard-fork \
         parameter.  Every constant-work handler (open/close/stat/exists/...) \
         inherits this value; changing it drifts every one of those handlers \
         at once.  Requires coordinated hard-fork activation and matching \
         updates to the individual per-handler golden pins below."
    );
}

/// `FS_PATH_MUTATION_CONST` is the base weight for path-mutation
/// syscalls (`rename`, `copy_file`, `remove_file`).  Doubled vs.
/// `FS_SYSCALL_CONST` to reflect two-endpoint work (source unlink +
/// destination create, both subject to trusted-root policy check).
#[test]
fn fs_path_mutation_const_is_pinned_at_200() {
    assert_eq!(
        FS_PATH_MUTATION_CONST, 200,
        "consensus-critical weight drift: FS_PATH_MUTATION_CONST is a \
         hard-fork parameter.  Changes affect fs_rename / fs_copy_file / \
         fs_remove_file and the base of fs_remove_dir."
    );
}

/// `FS_ENTRIES_PER_ENTRY` — per-directory-entry incremental weight
/// used by `fs_entries` and `fs_remove_dir`.  32 units reflects the
/// encoded size of a single entry record under Consensus mode.
#[test]
fn fs_entries_per_entry_is_pinned_at_32() {
    assert_eq!(
        FS_ENTRIES_PER_ENTRY, 32,
        "consensus-critical weight drift: FS_ENTRIES_PER_ENTRY governs the \
         linear cost of fs_entries / fs_entries_stream / fs_remove_dir."
    );
}

/// `FS_ENTRIES_SETUP` — fixed setup cost for directory enumeration
/// (dispatch, path lookup, handle bookkeeping).  50 units amortizes
/// across the per-entry cost so short listings aren't dominated by
/// per-entry overhead.
#[test]
fn fs_entries_setup_is_pinned_at_50() {
    assert_eq!(
        FS_ENTRIES_SETUP, 50,
        "consensus-critical weight drift: FS_ENTRIES_SETUP is the fixed \
         base for fs_entries / fs_entries_stream."
    );
}

/// **Design-intent pin.**  `FS_PATH_MUTATION_CONST` is *intended* to
/// be exactly 2× `FS_SYSCALL_CONST` — path mutations touch two
/// endpoints so they cost twice a single-endpoint syscall.  Pinning
/// each side independently would let a future change to
/// `FS_SYSCALL_CONST` silently break the ratio (e.g. lowering
/// `FS_SYSCALL_CONST` to 50 while leaving `FS_PATH_MUTATION_CONST`
/// at 200 would make the mutation class 4× the syscall class,
/// contradicting the design rationale).  This pin makes the design
/// intent load-bearing.
#[test]
fn fs_path_mutation_is_2x_syscall_const() {
    assert_eq!(
        FS_PATH_MUTATION_CONST,
        2 * FS_SYSCALL_CONST,
        "design-intent regression: FS_PATH_MUTATION_CONST must be exactly \
         2x FS_SYSCALL_CONST because path mutations touch two endpoints \
         (source unlink + destination create).  If you intended to change \
         the ratio, update this pin AND the docstring on \
         FS_PATH_MUTATION_CONST in costs.rs."
    );
}

// -------- Constant-work syscalls -----------------------------------

#[test]
fn fs_open_weight_is_pinned() {
    assert_eq!(fs_open_cost(), Cost::create(100, "fs_open"));
}

#[test]
fn fs_close_weight_is_pinned() {
    assert_eq!(fs_close_cost(), Cost::create(100, "fs_close"));
}

#[test]
fn fs_stat_weight_is_pinned() {
    assert_eq!(fs_stat_cost(), Cost::create(100, "fs_stat"));
}

#[test]
fn fs_exists_weight_is_pinned() {
    assert_eq!(fs_exists_cost(), Cost::create(100, "fs_exists"));
}

#[test]
fn fs_chmod_weight_is_pinned() {
    assert_eq!(fs_chmod_cost(), Cost::create(100, "fs_chmod"));
}

#[test]
fn fs_chown_weight_is_pinned() {
    assert_eq!(fs_chown_cost(), Cost::create(100, "fs_chown"));
}

#[test]
fn fs_seek_weight_is_pinned() {
    assert_eq!(fs_seek_cost(), Cost::create(100, "fs_seek"));
}

#[test]
fn fs_tell_weight_is_pinned() {
    assert_eq!(fs_tell_cost(), Cost::create(100, "fs_tell"));
}

#[test]
fn fs_size_weight_is_pinned() {
    assert_eq!(fs_size_cost(), Cost::create(100, "fs_size"));
}

#[test]
fn fs_truncate_weight_is_pinned() {
    assert_eq!(fs_truncate_cost(), Cost::create(100, "fs_truncate"));
}

#[test]
fn fs_flush_weight_is_pinned() {
    assert_eq!(fs_flush_cost(), Cost::create(100, "fs_flush"));
}

#[test]
fn fs_quarantine_weight_is_pinned() {
    assert_eq!(fs_quarantine_cost(), Cost::create(100, "fs_quarantine"));
}

#[test]
fn fs_lock_range_weight_is_pinned() {
    assert_eq!(fs_lock_range_cost(), Cost::create(100, "fs_lock_range"));
}

#[test]
fn fs_lock_sequential_weight_is_pinned() {
    assert_eq!(
        fs_lock_sequential_cost(),
        Cost::create(100, "fs_lock_sequential")
    );
}

#[test]
fn fs_release_lock_weight_is_pinned() {
    assert_eq!(fs_release_lock_cost(), Cost::create(100, "fs_release_lock"));
}

#[test]
fn fs_release_all_for_holder_weight_is_pinned() {
    assert_eq!(
        fs_release_all_for_holder_cost(),
        Cost::create(100, "fs_release_all_for_holder")
    );
}

// -------- Path-mutation syscalls -----------------------------------

#[test]
fn fs_rename_weight_is_pinned() {
    assert_eq!(fs_rename_cost(), Cost::create(200, "fs_rename"));
}

#[test]
fn fs_copy_file_weight_is_pinned() {
    assert_eq!(fs_copy_file_cost(), Cost::create(200, "fs_copy_file"));
}

#[test]
fn fs_remove_file_weight_is_pinned() {
    assert_eq!(fs_remove_file_cost(), Cost::create(200, "fs_remove_file"));
}

// -------- Length-parameterized syscalls ----------------------------
//
// Pin the base weight (zero-argument variant) AND the linear-term
// coefficient (via a small non-zero sample).  A regression that
// shifts the coefficient — e.g. `read` from `1 * bytes` to
// `2 * bytes` — trips the sample even if the base still matches.
// A regression that shifts the base trips both.

#[test]
fn fs_read_weight_at_zero_bytes_is_pinned() {
    assert_eq!(fs_read_cost(0), Cost::create(100, "fs_read"));
}

#[test]
fn fs_read_weight_at_1024_bytes_is_pinned() {
    assert_eq!(fs_read_cost(1024), Cost::create(100 + 1024, "fs_read"));
}

#[test]
fn fs_read_at_weight_at_zero_bytes_is_pinned() {
    assert_eq!(fs_read_at_cost(0), Cost::create(100, "fs_read_at"));
}

#[test]
fn fs_read_at_weight_at_1024_bytes_is_pinned() {
    assert_eq!(
        fs_read_at_cost(1024),
        Cost::create(100 + 1024, "fs_read_at")
    );
}

#[test]
fn fs_write_weight_at_zero_bytes_is_pinned() {
    assert_eq!(fs_write_cost(0), Cost::create(100, "fs_write"));
}

#[test]
fn fs_write_weight_at_1024_bytes_is_pinned() {
    assert_eq!(
        fs_write_cost(1024),
        Cost::create(100 + 2 * 1024, "fs_write"),
        "fs_write linear coefficient must be 2x (WAL-append cost); a shift \
         to 1x drifts the write-heavy workload cost class and is a hard-fork"
    );
}

#[test]
fn fs_write_at_weight_at_zero_bytes_is_pinned() {
    assert_eq!(fs_write_at_cost(0), Cost::create(100, "fs_write_at"));
}

#[test]
fn fs_write_at_weight_at_1024_bytes_is_pinned() {
    assert_eq!(
        fs_write_at_cost(1024),
        Cost::create(100 + 2 * 1024, "fs_write_at")
    );
}

#[test]
fn fs_entries_weight_at_zero_entries_is_pinned() {
    // Empty directory — setup cost only.
    assert_eq!(fs_entries_cost(0), Cost::create(50, "fs_entries"));
}

#[test]
fn fs_entries_weight_at_10_entries_is_pinned() {
    assert_eq!(
        fs_entries_cost(10),
        Cost::create(50 + 32 * 10, "fs_entries")
    );
}

#[test]
fn fs_entries_stream_weight_at_zero_entries_is_pinned() {
    assert_eq!(
        fs_entries_stream_cost(0),
        Cost::create(50, "fs_entries_stream")
    );
}

#[test]
fn fs_entries_stream_weight_at_10_entries_is_pinned() {
    assert_eq!(
        fs_entries_stream_cost(10),
        Cost::create(50 + 32 * 10, "fs_entries_stream")
    );
}

#[test]
fn fs_remove_dir_weight_at_zero_entries_is_pinned() {
    // Empty directory removal — path-mutation base only.
    assert_eq!(fs_remove_dir_cost(0), Cost::create(200, "fs_remove_dir"));
}

// -------- u64::MAX saturation pins ---------------------------------
//
// Pins the overflow-safety property of `saturate_linear` in
// costs.rs.  Under D3 a validator MUST NOT crash on adversarial
// syscall arguments; every length-parameterized helper must clamp
// to `i64::MAX` under `u64::MAX` input.  A regression that reverts
// the helper to naive `base + argument as i64` would produce a
// negative value at `argument ≈ 2^63` and crash the deploy with
// `BugFoundError("Billable metering cost must be positive")`.
// These pins catch the revert locally.

#[test]
fn fs_read_cost_saturates_at_u64_max() {
    assert_eq!(fs_read_cost(u64::MAX), Cost::create(i64::MAX, "fs_read"));
}

#[test]
fn fs_read_at_cost_saturates_at_u64_max() {
    assert_eq!(
        fs_read_at_cost(u64::MAX),
        Cost::create(i64::MAX, "fs_read_at")
    );
}

#[test]
fn fs_write_cost_saturates_at_u64_max() {
    assert_eq!(fs_write_cost(u64::MAX), Cost::create(i64::MAX, "fs_write"));
}

#[test]
fn fs_write_at_cost_saturates_at_u64_max() {
    assert_eq!(
        fs_write_at_cost(u64::MAX),
        Cost::create(i64::MAX, "fs_write_at")
    );
}

#[test]
fn fs_entries_cost_saturates_at_u64_max() {
    assert_eq!(
        fs_entries_cost(u64::MAX),
        Cost::create(i64::MAX, "fs_entries")
    );
}

#[test]
fn fs_entries_stream_cost_saturates_at_u64_max() {
    assert_eq!(
        fs_entries_stream_cost(u64::MAX),
        Cost::create(i64::MAX, "fs_entries_stream")
    );
}

#[test]
fn fs_remove_dir_cost_saturates_at_u64_max() {
    assert_eq!(
        fs_remove_dir_cost(u64::MAX),
        Cost::create(i64::MAX, "fs_remove_dir")
    );
}

// -------- Monotonicity / linearity pins -----------------------------
//
// Hard-lock the linear-shape assumption of every length-
// parameterized helper.  A regression that introduces non-linearity
// (e.g. Θ(n²) accidental substitution) or non-monotonicity would
// slip past the two sample-point pins above (0 and 1024) if the
// substituted shape happens to agree at those two points.
// Sweeping the small-integer range (0..64) catches such regressions
// cheaply.

fn assert_strictly_monotone(cost_fn: impl Fn(u64) -> Cost, name: &str) {
    let mut prev = cost_fn(0).value;
    for k in 1..64u64 {
        let curr = cost_fn(k).value;
        assert!(
            curr > prev,
            "monotonicity regression on {name}: cost({k}) = {curr} must be \
             strictly greater than cost({}) = {prev}.  A non-monotone helper \
             lets a caller pay less for MORE work, which is a soft-DoS + \
             pricing-fairness bug.",
            k - 1
        );
        prev = curr;
    }
}

fn assert_linear_shape(cost_fn: impl Fn(u64) -> Cost, coefficient: i64, name: &str) {
    let base = cost_fn(0).value;
    for k in 1..64u64 {
        let expected = base + coefficient * k as i64;
        let actual = cost_fn(k).value;
        assert_eq!(
            actual, expected,
            "linearity regression on {name}: cost({k}) = {actual} but a \
             linear extrapolation from cost(0) = {base} with coefficient \
             {coefficient} predicts {expected}.  A helper that deviates \
             from linearity within [0, 64) has introduced quadratic or \
             higher-order growth (e.g. accidental n*n substitution) or \
             per-argument branching that is not consensus-portable."
        );
    }
}

#[test]
fn fs_read_cost_is_strictly_monotone_and_linear() {
    assert_strictly_monotone(fs_read_cost, "fs_read_cost");
    assert_linear_shape(fs_read_cost, 1, "fs_read_cost");
}

#[test]
fn fs_read_at_cost_is_strictly_monotone_and_linear() {
    assert_strictly_monotone(fs_read_at_cost, "fs_read_at_cost");
    assert_linear_shape(fs_read_at_cost, 1, "fs_read_at_cost");
}

#[test]
fn fs_write_cost_is_strictly_monotone_and_linear() {
    assert_strictly_monotone(fs_write_cost, "fs_write_cost");
    assert_linear_shape(fs_write_cost, 2, "fs_write_cost");
}

#[test]
fn fs_write_at_cost_is_strictly_monotone_and_linear() {
    assert_strictly_monotone(fs_write_at_cost, "fs_write_at_cost");
    assert_linear_shape(fs_write_at_cost, 2, "fs_write_at_cost");
}

#[test]
fn fs_entries_cost_is_strictly_monotone_and_linear() {
    assert_strictly_monotone(fs_entries_cost, "fs_entries_cost");
    assert_linear_shape(fs_entries_cost, 32, "fs_entries_cost");
}

#[test]
fn fs_entries_stream_cost_is_strictly_monotone_and_linear() {
    assert_strictly_monotone(fs_entries_stream_cost, "fs_entries_stream_cost");
    assert_linear_shape(fs_entries_stream_cost, 32, "fs_entries_stream_cost");
}

#[test]
fn fs_remove_dir_cost_is_strictly_monotone_and_linear() {
    assert_strictly_monotone(fs_remove_dir_cost, "fs_remove_dir_cost");
    assert_linear_shape(fs_remove_dir_cost, 32, "fs_remove_dir_cost");
}

#[test]
fn fs_remove_dir_weight_at_10_entries_is_pinned() {
    assert_eq!(
        fs_remove_dir_cost(10),
        Cost::create(200 + 32 * 10, "fs_remove_dir")
    );
}

// -------- Coverage pin ---------------------------------------------

/// Meta-pin: assert we have a golden test for every helper exported
/// by `costs.rs`.  A future PR that adds a new native handler must
/// (a) add a `<name>_cost()` helper AND (b) add a golden-value test
/// for it here — otherwise this coverage pin fails.
///
/// Implementation: string-scan the cost helper source for every
/// `pub fn fs_*_cost(...)` and verify each name appears in this
/// test file **as a call site** (`<name>(`), not merely as a
/// `use`-import identifier.  Requiring the call-site pattern
/// closes the hole where a helper is imported but the golden
/// `#[test] fn <name>_weight_is_pinned()` is subsequently deleted:
/// the identifier still appears in the `use` block but no
/// `<name>(` call site remains, so the meta-pin fails.
#[test]
fn every_cost_helper_has_a_golden_pin() {
    let costs_src = include_str!("../src/rust/interpreter/io/costs.rs");
    let spec_src = include_str!("fileio_cost_spec.rs");

    let mut missing = Vec::new();
    for line in costs_src.lines() {
        // Match `pub fn fs_<name>_cost(` — the naming convention
        // for per-handler cost helpers.  Skip lines inside doc
        // comments (`///` prefix) so the helper's docstring
        // examples don't count as declarations.
        let trimmed = line.trim_start();
        if trimmed.starts_with("///") || trimmed.starts_with("//") {
            continue;
        }
        if let Some(idx) = trimmed.find("pub fn fs_") {
            let after = &trimmed[idx + "pub fn ".len()..];
            let end = after
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .unwrap_or(after.len());
            let name = &after[..end];
            if !name.ends_with("_cost") {
                continue;
            }
            // Require the call-site pattern `<name>(` — the trailing
            // `(` excludes bare identifier presence in the `use`
            // import block.  A deleted golden test that leaves the
            // stale import behind would fail this check because the
            // `<name>(` invocation site would be gone.
            let call_pattern = format!("{name}(");
            if !spec_src.contains(&call_pattern) {
                missing.push(name.to_string());
            }
        }
    }

    assert!(
        missing.is_empty(),
        "coverage regression: the following per-handler cost helpers exist in \
         io/costs.rs but have no golden-value call site (`<name>(...)`) in this \
         spec: {missing:?}.  Every new native handler MUST ship with a golden \
         `#[test] fn <name>_weight_is_pinned()` that invokes the helper by \
         name; a bare `use`-import does NOT count.  Add the test and update \
         the import block at the top of this file."
    );
}
