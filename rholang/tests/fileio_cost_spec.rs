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
    fs_entries_per_entry_supplement_cost, fs_entries_stream_close_cost, fs_entries_stream_cost,
    fs_entries_stream_next_cost, fs_entries_stream_open_cost,
    fs_entries_stream_per_entry_supplement_cost, fs_exists_cost, fs_flush_cost, fs_lock_range_cost,
    fs_lock_sequential_cost, fs_open_cost, fs_quarantine_cost, fs_read_at_cost, fs_read_cost,
    fs_release_all_for_holder_cost, fs_release_lock_cost, fs_remove_dir_cost,
    fs_remove_dir_per_entry_supplement_cost, fs_remove_file_cost, fs_rename_cost, fs_seek_cost,
    fs_size_cost, fs_stat_cost, fs_tell_cost, fs_truncate_cost, fs_write_at_cost, fs_write_cost,
    FS_ENTRIES_PER_ENTRY, FS_ENTRIES_SETUP, FS_PATH_MUTATION_CONST, FS_SYSCALL_CONST,
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

// -------- Streaming-backing slice per-handler aliases --------------
//
// The three natives that back the per-fd directory-streaming primitive
// (`entriesStreamOpen`/`Next`/`Close`) each ship a per-handler cost
// alias so the every-handler-charges-its-cost pin
// (`every_fs_handler_charges_its_cost_helper`) passes.  Semantically
// they delegate to the pre-existing shapes:
//   - open + next  → fs_entries_stream_cost(0) = FS_ENTRIES_SETUP = 50
//     (the per-entry supplement is a separate two-branch charge via
//      `fs_entries_stream_per_entry_supplement_cost`).
//   - close        → fs_close_cost() = FS_SYSCALL_CONST = 100.
// Golden values pinned here so a future consensus-observable retune of
// the underlying shapes flips the corresponding pin.

#[test]
fn fs_entries_stream_open_weight_is_pinned() {
    assert_eq!(
        fs_entries_stream_open_cost(),
        Cost::create(FS_ENTRIES_SETUP, "fs_entries_stream")
    );
}

#[test]
fn fs_entries_stream_next_weight_is_pinned() {
    assert_eq!(
        fs_entries_stream_next_cost(),
        Cost::create(FS_ENTRIES_SETUP, "fs_entries_stream")
    );
}

#[test]
fn fs_entries_stream_close_weight_is_pinned() {
    assert_eq!(
        fs_entries_stream_close_cost(),
        Cost::create(FS_SYSCALL_CONST, "fs_close")
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

// -------- Slice 9b-iv follow-up: per-entry supplement pins ---------
//
// The supplement helpers are the second `reserve_primitive` call in
// the two-branch entries-family pattern.  Sum with `_cost(0)` must
// equal `_cost(n)` for every input — `entries_family_supplements_match_combined_costs`
// below sweeps that invariant.  These pins additionally lock the
// supplement shape (0 at n=0, coefficient at 1, saturation at
// u64::MAX) so a regression in the split can't silently be "fixed"
// by tweaking the setup component to compensate.

#[test]
fn fs_entries_per_entry_supplement_at_zero_is_zero() {
    assert_eq!(
        fs_entries_per_entry_supplement_cost(0),
        Cost::create(0, "fs_entries_per_entry"),
    );
}

#[test]
fn fs_entries_per_entry_supplement_at_one_pins_coefficient() {
    assert_eq!(
        fs_entries_per_entry_supplement_cost(1),
        Cost::create(FS_ENTRIES_PER_ENTRY, "fs_entries_per_entry"),
    );
}

#[test]
fn fs_entries_per_entry_supplement_at_10_is_pinned() {
    assert_eq!(
        fs_entries_per_entry_supplement_cost(10),
        Cost::create(32 * 10, "fs_entries_per_entry"),
    );
}

#[test]
fn fs_entries_per_entry_supplement_saturates_at_u64_max() {
    assert_eq!(
        fs_entries_per_entry_supplement_cost(u64::MAX),
        Cost::create(i64::MAX, "fs_entries_per_entry"),
    );
}

#[test]
fn fs_entries_stream_per_entry_supplement_at_zero_is_zero() {
    assert_eq!(
        fs_entries_stream_per_entry_supplement_cost(0),
        Cost::create(0, "fs_entries_stream_per_entry"),
    );
}

#[test]
fn fs_entries_stream_per_entry_supplement_at_10_is_pinned() {
    assert_eq!(
        fs_entries_stream_per_entry_supplement_cost(10),
        Cost::create(32 * 10, "fs_entries_stream_per_entry"),
    );
}

#[test]
fn fs_remove_dir_per_entry_supplement_at_zero_is_zero() {
    assert_eq!(
        fs_remove_dir_per_entry_supplement_cost(0),
        Cost::create(0, "fs_remove_dir_per_entry"),
    );
}

#[test]
fn fs_remove_dir_per_entry_supplement_at_10_is_pinned() {
    assert_eq!(
        fs_remove_dir_per_entry_supplement_cost(10),
        Cost::create(32 * 10, "fs_remove_dir_per_entry"),
    );
}

/// Invariant that ties the split back to the whole: for every
/// entries-family helper, the SATURATING SUM of
/// `_cost(0)` plus `_per_entry_supplement_cost(n)` MUST equal
/// `_cost(n)` at every input.  This guards against a refactor that
/// alters the split (e.g. moves part of the setup coefficient into
/// the supplement) without adjusting the total.  Total-weight drift
/// is the load-bearing consensus risk; this pin catches it directly.
///
/// Sweeps small integers (0, 1, 10, 1000, 100_000) plus u64::MAX to
/// hit both normal paths and the saturation boundary.  Uses
/// `saturating_add` on the two-charge sum because the two individual
/// `reserve_primitive` calls at runtime each carry an
/// already-saturated Cost — an adversarial u64::MAX supplement would
/// reserve i64::MAX in the second charge and fail the budget check
/// on its own, but the compile-time golden pin must not overflow
/// while verifying the equality.
#[test]
fn entries_family_supplements_match_combined_costs() {
    let sample_ns: &[u64] = &[0, 1, 10, 1000, 100_000, u64::MAX];
    for &n in sample_ns {
        // fs_entries
        assert_eq!(
            fs_entries_cost(0)
                .value
                .saturating_add(fs_entries_per_entry_supplement_cost(n).value),
            fs_entries_cost(n).value,
            "slice 9b-iv total-weight drift: \
             fs_entries_cost(0) + fs_entries_per_entry_supplement_cost({n}) \
             must equal fs_entries_cost({n}).  A mismatch means the two-branch \
             split has drifted from the single-charge helper — every \
             non-empty entries call would over- or under-charge.",
        );
        // fs_entries_stream
        assert_eq!(
            fs_entries_stream_cost(0)
                .value
                .saturating_add(fs_entries_stream_per_entry_supplement_cost(n).value),
            fs_entries_stream_cost(n).value,
            "slice 9b-iv total-weight drift (fs_entries_stream at n={n})",
        );
        // fs_remove_dir
        assert_eq!(
            fs_remove_dir_cost(0)
                .value
                .saturating_add(fs_remove_dir_per_entry_supplement_cost(n).value),
            fs_remove_dir_cost(n).value,
            "slice 9b-iv total-weight drift (fs_remove_dir at n={n})",
        );
    }
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

// -------- Slice 9b regression pins ---------------------------------

/// Extract the function-body substring for a top-level
/// `impl`-method declaration in a Rust source string.  Used by the
/// slice 9b pins to bound their per-handler scans.  Returns the body
/// bounded by the next `pub async fn ` OR `pub fn ` on the same
/// four-space-indent level (i.e. the next method in the same
/// `impl` block), or the end of source if no next method exists.
fn method_body<'a>(src: &'a str, method_signature_prefix: &str) -> Option<&'a str> {
    let start = src.find(method_signature_prefix)?;
    let body_start = start + method_signature_prefix.len();
    let after = &src[body_start..];
    // Look for the next top-level `pub async fn ` or `pub fn ` at
    // the same indent level.  `impl`-block methods in this codebase
    // start with `    pub async fn ` (four-space indent).
    let end_a = after.find("\n    pub async fn ").unwrap_or(after.len());
    let end_b = after.find("\n    pub fn ").unwrap_or(after.len());
    let end = end_a.min(end_b);
    Some(&after[..end])
}

/// **Slice 9b regression pin — handler charge presence.**
///
/// For every `pub async fn fs_<name>(` in `handlers.rs`, this test
/// requires the body to contain a corresponding `costs::fs_<name>_cost(`
/// call site.  Catches the class of regression where a future PR
/// silently reverts (or forgets to add) a handler's cost charge —
/// which would slip past existing runtime tests (`fs_wal_spec` doesn't
/// observe cost accounting) and past compile-time checks (the
/// helper stays exported).
///
/// Under D3, a missing handler charge is a load-bearing consensus
/// bug: leader realizes cost C, validator re-executes and misses
/// the charge → validator's realized cost is C - w → certificate
/// mismatch → block rejected.  The failure mode is silent at code-
/// review time and expensive at runtime, so a static pin is the
/// right defense.
///
/// Placement flexibility: the test only requires the substring
/// `costs::fs_<name>_cost(` inside the handler body.  Whether the
/// charge is via `reserve_primitive` or `reserve_incremental_primitive`
/// is irrelevant to this pin (both are consensus-observable).
#[test]
fn every_fs_handler_charges_its_cost_helper() {
    let src = include_str!("../src/rust/interpreter/io/handlers.rs");
    let mut missing = Vec::new();

    // Iterate every `pub async fn fs_<name>(` declaration.
    let mut cursor = 0usize;
    while let Some(rel) = src[cursor..].find("pub async fn fs_") {
        let abs = cursor + rel;
        let after = &src[abs + "pub async fn ".len()..];
        let name_end = after
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(after.len());
        let handler_name = &after[..name_end];
        // Advance the cursor past this occurrence so the loop
        // progresses even if the body scan fails.
        cursor = abs + "pub async fn ".len() + name_end;

        // Skip the mod-test names — they mention handler names in
        // pinning strings, not in real dispatch declarations.
        // Real handlers are in `impl FsProcesses { ... }` — indent
        // matches "    pub async fn fs_<name>(".
        let signature_prefix = format!("    pub async fn {handler_name}(");
        let Some(body) = method_body(src, &signature_prefix) else {
            continue;
        };
        let expected_call = format!("costs::{handler_name}_cost(");
        if !body.contains(&expected_call) {
            missing.push(handler_name.to_string());
        }
    }

    assert!(
        missing.is_empty(),
        "slice 9b charge-presence regression: the following fs native handlers \
         exist in handlers.rs but do NOT reference their `costs::<name>_cost(...)` \
         helper in the function body: {missing:?}.  Under D3, a missing handler \
         charge is a leader/replay consensus divergence: the validator will \
         compute a realized cost different from the leader's committed witness \
         and reject the block.  Add `self.metering.reserve_primitive(\
         costs::<name>_cost(...))?;` at handler entry (or reserve_incremental_primitive \
         for length-parameterized helpers that might legitimately produce zero \
         cost).  See slice 9b-ii commit and the fs_open docstring in handlers.rs \
         for placement rationale."
    );
}

/// **Slice 9b regression pin — shared MeteredMachine.**
///
/// Verify `setup_reducer` in `rho_runtime.rs` creates ONE
/// `MeteredMachine`, threads it into `dispatch_table_creator` (which
/// gives the fs handlers a clone via `ProcessContext::create ->
/// SystemProcesses::create -> FsProcesses::new`), AND passes clones
/// to the reducer and its substitute.  A future refactor that
/// accidentally created a SEPARATE `MeteredMachine::new(...)` for
/// the handlers would silently break budget-consumption accounting
/// (handler charges would decrement a different budget than the
/// reducer's), which is a leader/replay divergence trap invisible
/// until the first cost-heavy deploy.
#[test]
fn setup_reducer_shares_one_metered_machine() {
    let src = include_str!("../src/rust/interpreter/rho_runtime.rs");
    let setup_start = src
        .find("async fn setup_reducer(")
        .expect("setup_reducer must exist in rho_runtime.rs");
    // Bound the scan at the next top-level `fn ` after setup_reducer.
    let after = &src[setup_start..];
    let end = after[1..]
        .find("\nfn ")
        .or_else(|| after[1..].find("\nasync fn "))
        .unwrap_or(after.len());
    let body = &after[..end + 1];

    // Invariant 1: exactly one `MeteredMachine::new(` call site.
    // A second call site would indicate a diverged budget.
    let n_new = body.matches("MeteredMachine::new(").count();
    assert_eq!(
        n_new, 1,
        "slice 9b-i plumbing regression: setup_reducer must construct EXACTLY \
         ONE MeteredMachine (currently {n_new}).  Multiple MeteredMachine::new(...) \
         call sites mean handler-side charges decrement a different budget than \
         the reducer's — a silent leader/replay divergence trap under D3.  \
         Clone the single machine into every consumer (dispatch_table_creator, \
         DebruijnInterpreter's metering field, DebruijnInterpreter's Substitute)."
    );

    // Invariant 2: the `metering` binding must be passed to
    // `dispatch_table_creator(...)`.  Enforce by requiring the
    // `metering.clone()` argument appears inside the dispatch_table
    // construction site.
    let dtc_start = body
        .find("dispatch_table_creator(")
        .expect("setup_reducer must call dispatch_table_creator");
    let dtc_body = &body[dtc_start..];
    let dtc_close = dtc_body
        .find(");")
        .expect("dispatch_table_creator call must close");
    let dtc_args = &dtc_body[..dtc_close];
    assert!(
        dtc_args.contains("metering.clone()") || dtc_args.contains("metering,"),
        "slice 9b-i plumbing regression: dispatch_table_creator(...) call in \
         setup_reducer must receive the SHARED `metering` binding (either as \
         `metering.clone()` or as a final move of `metering`).  Without this \
         thread, fs handlers get no MeteredMachine and cost accounting is a \
         no-op for every fs syscall."
    );

    // Invariant 3: DebruijnInterpreter must be constructed with the
    // same `metering` binding (as `metering: metering.clone()`) and
    // its Substitute must consume the remaining `metering` (as
    // `Substitute { metering }`).
    let di_start = body
        .find("DebruijnInterpreter {")
        .expect("setup_reducer must construct DebruijnInterpreter");
    let di_body = &body[di_start..];
    let di_close = di_body.find("});").expect("DebruijnInterpreter must close");
    let di_construct = &di_body[..di_close];
    assert!(
        di_construct.contains("metering: metering"),
        "slice 9b-i plumbing regression: DebruijnInterpreter construction in \
         setup_reducer must set `metering: metering.clone()` (or `metering: metering,\
         `) using the SAME `metering` binding threaded into dispatch_table_creator."
    );
    assert!(
        di_construct.contains("Substitute { metering }"),
        "slice 9b-i plumbing regression: DebruijnInterpreter's Substitute must \
         consume the same `metering` binding via `Substitute {{ metering }}`.  \
         A refactor that constructs Substitute with a fresh MeteredMachine \
         would leak a divergent budget into substitution accounting."
    );
}

/// **Slice 9b-iv follow-up pin — fs_entries has full two-branch charge.**
///
/// After landing per-entry two-branch charges on `fs_entries`, this pin
/// requires the handler body to contain BOTH the setup call
/// (`costs::fs_entries_cost(0)`) AND the per-entry supplement call
/// (`costs::fs_entries_per_entry_supplement_cost(`).  The supplement
/// MUST be present on both branches — leader (post-`spawn_blocking`,
/// extracting `n` from the fresh reply) and replay (extracting `n`
/// from `previous`) — to keep the D3 canonical event log
/// byte-identical across leader and follower.
///
/// A single-branch charge (leader-only or replay-only) is a leader/
/// replay consensus divergence trap.  A missing supplement means we're
/// undercharging every non-empty directory listing.
///
/// Pin shape: substring-match the supplement helper name and require
/// at least two occurrences within the handler body (one per branch).
#[test]
fn fs_entries_charges_supplement_on_both_branches() {
    let src = include_str!("../src/rust/interpreter/io/handlers.rs");
    let signature_prefix = "    pub async fn fs_entries(";
    let body =
        method_body(src, signature_prefix).expect("fs_entries handler must exist in handlers.rs");

    assert!(
        body.contains("costs::fs_entries_cost(0)"),
        "slice 9b-iv regression: fs_entries must retain its setup-only \
         `costs::fs_entries_cost(0)` charge at handler entry — the per-entry \
         supplement is layered on top, not a replacement."
    );

    let supplement_call = "costs::fs_entries_per_entry_supplement_cost(";
    let n_supplement = body.matches(supplement_call).count();
    assert!(
        n_supplement >= 2,
        "slice 9b-iv regression: fs_entries must charge \
         `{supplement_call}` on BOTH the replay and leader branches \
         (currently {n_supplement} call site(s)).  A single-branch \
         charge is a leader/replay consensus divergence: the two \
         validators compute different `authority_cost_witness.realized` \
         values and reject each other's blocks.  Preserve two \
         `reserve_primitive(costs::fs_entries_per_entry_supplement_cost(n))?;` \
         call sites — one after `if is_replay {{` extracts n from \
         `previous`, one after the leader's `spawn_blocking` completes \
         extracting n from the fresh reply."
    );
}

/// **fs_remove_dir has full two-branch charge post
/// DD-RemoveDirReplyShape (2026-09-03).**
///
/// Superseded the pre-DD-RemoveDirReplyShape pin
/// `remove_dir_charges_setup_only_pending_reply_shape_change`
/// (which held the deferral while the reply shape lacked `nDeleted`).
/// Post-shape-change, every removeDir code path — non-recursive
/// (any cmode), recursive Oracular, recursive Consensus — returns
/// `nDeleted` at position 1 of the success reply / position 3 of the
/// failure reply.  Cost helpers (`fs_remove_dir_supplement_count` +
/// `fs_remove_dir_supplement_count_from_previous`) read the count
/// directly from the reply via `extract_removedir_n_deleted`; both
/// leader (from fresh reply) and follower (from `previous`) derive
/// the same value.  Oracular recursive now bills per-entry
/// symmetrically with Consensus recursive, closing the DoS opening.
///
/// Pin shape: handler body must contain BOTH the setup call
/// (`costs::fs_remove_dir_cost(0)`) AND the per-entry supplement call
/// (`costs::fs_remove_dir_per_entry_supplement_cost(`) on both
/// branches.  A single-branch charge (leader-only or replay-only) is
/// a leader/follower consensus divergence trap.
#[test]
fn fs_remove_dir_charges_supplement_on_both_branches() {
    let src = include_str!("../src/rust/interpreter/io/handlers.rs");
    let signature_prefix = "    pub async fn fs_remove_dir(";
    let body = method_body(src, signature_prefix).expect("fs_remove_dir handler must exist");

    assert!(
        body.contains("costs::fs_remove_dir_cost(0)"),
        "DD-RemoveDirReplyShape regression: fs_remove_dir must retain its \
         setup-only `costs::fs_remove_dir_cost(0)` charge at handler entry \
         — the per-entry supplement is layered on top, not a replacement."
    );

    let supplement_call = "costs::fs_remove_dir_per_entry_supplement_cost(";
    let n_supplement = body.matches(supplement_call).count();
    assert!(
        n_supplement >= 2,
        "DD-RemoveDirReplyShape regression: fs_remove_dir must charge \
         `{supplement_call}` on BOTH the replay and leader branches \
         (currently {n_supplement} call site(s)).  A single-branch \
         charge is a leader/follower consensus divergence: the two \
         validators compute different `authority_cost_witness.realized` \
         values and reject each other's blocks.  Preserve two \
         `reserve_incremental_primitive(costs::fs_remove_dir_per_entry_supplement_cost(n))?;` \
         call sites — one after `if is_replay {{` extracts n from \
         `previous`, one after the leader's `spawn_blocking` completes \
         extracting n from the fresh reply."
    );
}

/// **Cost-helper audit pin (2026-08-26).**  Length-parameterized
/// helpers (`fs_read_cost`, `fs_read_at_cost`, `fs_write_cost`,
/// `fs_write_at_cost`, `fs_entries_per_entry_supplement_cost`) MUST
/// be charged via `reserve_incremental_primitive` — per the
/// discipline docstring at costs.rs:43-49 and the fix that landed
/// after the empty-directory `BugFoundError` regression documented
/// in the Deferred items catalog.
///
/// Rationale: each of these helpers can legitimately compute zero
/// weight (EOF-truncated read → 0 bytes; empty write → 0 bytes;
/// empty directory → 0 entries).  `reserve_primitive` returns
/// `BugFoundError` on ≤ 0 cost (metering.rs:137), silently
/// populating `EvaluateResult.errors` and skipping any post-charge
/// side-effect (WAL journal, etc.).  `reserve_incremental_primitive`
/// early-returns Ok on zero and only reserves for positive cost;
/// both branches emit the same `BillableTokenEvent::Primitive` when
/// cost is non-zero, so the switch is consensus-safe.
///
/// A regression that switches any of these back to `reserve_primitive`
/// would trip this pin.  For any NEW length-parameterized helper
/// added later, extend `INCREMENTAL_REQUIRED` below.
#[test]
fn length_parameterized_cost_helpers_use_reserve_incremental_primitive() {
    let src = include_str!("../src/rust/interpreter/io/handlers.rs");
    // Every length-parameterized helper whose length argument can
    // legitimately be zero at any call site.  Constant-work helpers
    // (fs_open_cost, fs_close_cost, ...) stay on `reserve_primitive`
    // because their weight is always positive by construction.
    const INCREMENTAL_REQUIRED: &[&str] = &[
        "fs_read_cost",
        "fs_read_at_cost",
        "fs_write_cost",
        "fs_write_at_cost",
        "fs_entries_per_entry_supplement_cost",
        "fs_entries_stream_per_entry_supplement_cost",
    ];
    let mut violations = Vec::new();
    for helper in INCREMENTAL_REQUIRED {
        // Match `reserve_primitive(costs::<helper>` — the exact
        // (bad) shape we want to prohibit.  A false positive on
        // this substring would require the helper's name to appear
        // inside a `reserve_primitive(...)` call, which is exactly
        // what we're policing.
        let bad = format!("reserve_primitive(costs::{helper}");
        if src.contains(&bad) {
            violations.push(*helper);
        }
    }
    assert!(
        violations.is_empty(),
        "cost-helper audit regression: the following length-parameterized \
         helpers MUST be charged via `reserve_incremental_primitive` (per \
         the discipline docstring at costs.rs:43-49), but a call site in \
         handlers.rs still uses `reserve_primitive`: {violations:?}.  \
         Regression risk: an input that produces zero cost (EOF read, \
         empty write, empty directory) will trip `BugFoundError` inside \
         `reserve_primitive` (metering.rs:137), silently poisoning \
         `EvaluateResult.errors` and skipping any post-charge side-effect \
         (WAL journal, etc.).  Fix: switch the offending call to \
         `reserve_incremental_primitive`."
    );
}

/// **Streaming-slice Step 8 review-fixup pin (2026-08-26).**  The
/// arity-3 `fs_entries_stream` handler at handlers.rs:2450 is a
/// deprecated stub — replaced by the three arity-2/4 streaming
/// natives (`fs_entries_stream_open` / `_next` / `_close`) landed in
/// Steps 2-3.  It stays in the composed source for URN-backward-
/// compatibility (`rho:io:fs:native:1.0.0/entriesStream` remains
/// bound at `fs_genesis.rs:682`) but is unreachable from any
/// production caller after Step 5 swapped Dir.rho to the streaming
/// primitives.
///
/// The prior deferred-charge pin (dropped in Step 8) implicitly
/// guarded against a silent "upgrade" of this stub to a real
/// implementation without wiring the paired supplement charge.
/// This pin re-instates that guard by requiring the stub to
/// explicitly return `FSERR_UNSUPPORTED`: a PR that flips the
/// return to `[true, ...]` without ALSO wiring the two-branch
/// supplement pattern would trip this pin — forcing the author to
/// justify why the arity-3 shape is being resurrected when arity-2
/// streaming already covers the use case.
#[test]
fn arity3_entries_stream_stub_still_returns_fserr_unsupported() {
    let src = include_str!("../src/rust/interpreter/io/handlers.rs");
    // Anchor at the arity-3 handler — signature line grep is
    // sufficient to disambiguate from arity-2 / arity-4 variants.
    let signature_prefix = "    pub async fn fs_entries_stream(";
    let body = method_body(src, signature_prefix)
        .expect("fs_entries_stream handler (arity-3 stub) must exist");
    assert!(
        body.contains("FSERR_UNSUPPORTED"),
        "arity-3 fs_entries_stream stub must return FSERR_UNSUPPORTED — \
         it is deprecated post-Step-5, replaced by the arity-2/4 streaming \
         primitives.  Resurrecting the arity-3 shape requires wiring the \
         two-branch supplement charge on BOTH branches AND replacing this \
         pin with a `_charges_supplement_on_both_branches` shape.  \
         Confirm with a review pass that the URN backward-compatibility \
         binding at fs_genesis.rs is still the right disposition."
    );
}

/// **Phase 8 arity-tightening retirement pin (2026-08-26).**  The
/// `fs_lock_range` and `fs_lock_sequential` handlers dropped their
/// legacy arity-7 / arity-4 shim branches in commit `5e8f3e2a0`;
/// every File.rho caller now threads an explicit `wait: Bool`.
/// This pin prohibits re-adding a compat shim that silently defaults
/// `wait=false` for callers that omit it — a regression that would
/// let malformed callers slip past without a loud "arity mismatch"
/// signal.
///
/// Why not a runtime test: Rholang arity mismatch is enforced at
/// channel-binding level (the `arity: 8` on `fs_native_def`);
/// wrong-arity sends sit on the channel with no matching receiver
/// and simply hang.  The handler's `_ => illegal_argument_error`
/// arm is only reachable if a Rust caller invokes the handler
/// directly with wrong args — not from Rholang.  A source-scan pin
/// on the handler body catches the intended regression:
/// resurrecting the shim requires either (a) adding an arity-7
/// match arm (caught by this pin's substring check) OR (b) bumping
/// the `fs_native_def` arity back to 7 (caught by
/// `fs_native_def_arities_match_golden_table` in fs_genesis.rs).
#[test]
fn lock_range_and_sequential_handlers_reject_arity_shim() {
    let src = include_str!("../src/rust/interpreter/io/handlers.rs");

    // fs_lock_range must NOT contain a match arm of the legacy
    // arity-7 shape.  Post-tightening the only arm is the 8-arg
    // pattern; a resurrected shim would add `[fd, off, len, mode,
    // holder, cmode, ack]` (7 identifiers) as a second arm.
    let range_body = method_body(src, "    pub async fn fs_lock_range(")
        .expect("fs_lock_range handler must exist");
    assert!(
        !range_body.contains("[fd, off, len, mode, holder, cmode, ack]"),
        "arity-tightening regression: fs_lock_range handler contains \
         the legacy arity-7 match arm `[fd, off, len, mode, holder, \
         cmode, ack]`.  The shim was retired in commit 5e8f3e2a0; \
         all File.rho callers now pass arity 8 with explicit wait: \
         Bool.  If the shim was intentionally resurrected, ALSO bump \
         `fs_native_def(\"lockRange\", 8)` back to 7 in \
         rho_runtime.rs — otherwise the dispatch will still enforce \
         arity 8 at the channel binding and the shim is dead code."
    );

    let seq_body = method_body(src, "    pub async fn fs_lock_sequential(")
        .expect("fs_lock_sequential handler must exist");
    assert!(
        !seq_body.contains("[fd, holder, cmode, ack]"),
        "arity-tightening regression: fs_lock_sequential handler \
         contains the legacy arity-4 match arm `[fd, holder, cmode, \
         ack]`.  Same rationale as fs_lock_range above.  Companion \
         golden-table pin: fs_native_def_arities_match_golden_table."
    );
}

// -------- Slice 9c-i regression pin --------------------------------

/// **Slice 9c-i regression pin — Stream.rho chunk(n) payload cap.**
///
/// Enforces the `MAX_CHUNK_ITEMS=65536` cap on `Stream.rho::chunk(@n)`:
/// n above the cap must return `FSERR_QUOTA_EXCEEDED` before any
/// gathering starts.  Defense-in-depth against a caller requesting
/// a billion-item chunk that would allocate an unbounded reply list
/// AND against the per-item runtime cost cascading through
/// `gatherN` past the caller's intended budget.
///
/// Under D3, silently removing the cap would let a deploy consume
/// unbounded reply-payload allocation for a small charged cost
/// (`chunk(n)` charges through the underlying stream `next()`
/// dispatches, one per item — but the per-item consumption still
/// runs unless capped).  Not a leader/replay-divergence risk (the
/// same code runs on both), but a real DoS/fairness concern.
///
/// String-scan pin because Stream.rho is a `.rho` resource, not
/// Rust code; the cap is embedded as an integer literal (Rholang
/// has no shared-constant mechanism cross-file).  A silent drift
/// would ALSO trip `compose_fs_genesis_source_golden_hex` in
/// `casper::genesis::contracts::fs_genesis`, so this pin is
/// defense-in-depth on top of the golden-hash discipline.
#[test]
fn stream_chunk_enforces_max_chunk_items_cap() {
    let src = include_str!("../../casper/src/main/resources/Stream.rho");
    // The cap MUST live inside the `method chunk(@n)` block.
    let start = src
        .find("method chunk(@n) {")
        .expect("Stream.rho must define method chunk(@n)");
    let after = &src[start..];
    // Bound the scan at the next `method ` declaration in Stream.rho.
    let end = after[1..]
        .find("method ")
        .map(|i| i + 1)
        .unwrap_or(after.len());
    let body = &after[..end];

    assert!(
        body.contains("65536"),
        "slice 9c-i cap regression: Stream.rho::chunk(@n) must enforce \
         MAX_CHUNK_ITEMS=65536.  The literal `65536` was not found in the \
         method body; a silent removal of the cap opens an unbounded \
         reply-payload allocation vector."
    );
    assert!(
        body.contains("FSERR_QUOTA_EXCEEDED"),
        "slice 9c-i cap regression: Stream.rho::chunk(@n) must return \
         `FSERR_QUOTA_EXCEEDED` when n exceeds the cap.  Return code was \
         not found in the method body; a change to a different error code \
         (e.g. FSERR_BAD_ARG) would break caller error-taxonomy discipline."
    );
    assert!(
        body.contains("MAX_CHUNK_ITEMS"),
        "slice 9c-i cap regression: Stream.rho::chunk(@n) must reference \
         `MAX_CHUNK_ITEMS` in either its cap comparison or its error \
         message.  Removing the identifier while keeping the literal 65536 \
         hides the design intent from readers."
    );
}

/// **Slice 9c-ii landed-pin — Buffer.toByteArray(@cap) materialization cap.**
///
/// Replaces the prior deferral pin (`buffer_to_byte_array_deferral_still_holds`).
/// The materialization cap docstringed at spec §446 (`FSERR_QUOTA_EXCEEDED`)
/// is now wired on `Buffer.rho::method toByteArray(@cap)`:
///
///   * `cap` is a required positional argument of type Int.
///   * Non-Int cap → `[false, "BUFERR_INVALID_ARGUMENT", ...]`.
///   * cap < 0 → `[false, "BUFERR_INVALID_CAPACITY", ...]`.
///   * `ell > cap` → `[false, "FSERR_QUOTA_EXCEEDED", ...]`.
///
/// The 4 `File.rho` callers (writeFrom / writeFromAt, arity-1 and
/// arity-2 wait:true variants) pass `67108864` = `MAX_WRITE_BYTES`
/// (64 MiB) so an over-cap buffer fails at Buffer materialization
/// rather than downstream `fs_write` dispatch.
///
/// This pin holds all three sides:
///   1. Buffer.rho::toByteArray REQUIRES the `@cap` argument
///      (`method toByteArray(@cap)` present; `method toByteArray()`
///      no longer present).
///   2. Buffer.rho contains the `FSERR_QUOTA_EXCEEDED` reply arm
///      for `ell > cap`.
///   3. Every `File.rho` `toByteArray` call site passes a
///      non-empty second argument (grep-based check for arity-1
///      calls; no `!?("toByteArray")` bare pattern in File.rho).
#[test]
fn buffer_to_byte_array_has_cap_arg_and_quota_check() {
    let buffer_src = include_str!("../../casper/src/main/resources/Buffer.rho");
    let file_src = include_str!("../../casper/src/main/resources/File.rho");

    assert!(
        buffer_src.contains("method toByteArray(@cap)"),
        "slice 9c-ii regression: Buffer.rho::toByteArray must accept `@cap` \
         as its explicit argument.  A regression removing the `@cap` \
         parameter would silently defeat the FSERR_QUOTA_EXCEEDED gate \
         and let arbitrary-size buffer materialization proceed."
    );
    assert!(
        !buffer_src.contains("method toByteArray()"),
        "slice 9c-ii regression: Buffer.rho must NOT define arity-0 \
         `method toByteArray()` alongside the arity-1 variant.  A dual \
         signature would let legacy callers bypass the cap; the slice \
         explicitly transitions to the arity-1-only shape."
    );
    assert!(
        buffer_src.contains("FSERR_QUOTA_EXCEEDED"),
        "slice 9c-ii regression: Buffer.rho::toByteArray must return \
         `FSERR_QUOTA_EXCEEDED` on `ell > cap`.  Missing this string \
         means the cap arg landed without wiring the quota check — \
         DoS defense is a no-op."
    );

    // Every File.rho toByteArray call site must pass a cap; no bare
    // `!?("toByteArray")` (no arg) may remain.  The pattern
    // `!?("toByteArray",` (with comma) proves the caller passes at
    // least one explicit argument beyond the method name.
    assert!(
        !file_src.contains("!?(\"toByteArray\")"),
        "slice 9c-ii regression: File.rho contains at least one arity-0 \
         `!?(\"toByteArray\")` call site.  Every caller must pass a `cap` \
         argument (typically `67108864` = MAX_WRITE_BYTES) so a bloated \
         buffer fails at materialization rather than downstream fs_write."
    );
    // At least one arity-1 caller must exist (grepping for the specific
    // MAX_WRITE_BYTES cap value confirms the caller pattern lands intact).
    assert!(
        file_src.contains("!?(\"toByteArray\", 67108864)"),
        "slice 9c-ii regression: File.rho must contain the arity-1 \
         `!?(\"toByteArray\", 67108864)` call site pattern.  If the cap \
         value changed intentionally, update this pin to match the new \
         value AND document why (typical rationale: matching the \
         downstream fs_write MAX_WRITE_BYTES cap)."
    );
}

/// **Stdio is intrinsically oracular — no cmode arg on Stdin/Stdout.**
///
/// Reclassifies slice 10c (stdio replay wiring) from "deferred pending
/// harness" to "not needed by design": nondeterministic data sources
/// (stdin, stdout side effects, and any future non-reproducible
/// primitive) cannot be consensus-mode.  Their byte streams are
/// intrinsically per-node — followers were not there when the leader's
/// stdin arrived, and re-issuing `libc::read(0, ...)` produces
/// different bytes (or nothing at all).  Consensus mode requires
/// deterministic per-node reproducibility, which stdio cannot provide.
///
/// Enforcement is at the Stdin.rho / Stdout.rho constructor signatures:
/// both take a bare `(@fd)` with NO `cmode` argument, mirroring
/// File.rho / Dir.rho which DO take `(fd, canonRoot, rel, mode, cmode)`.
/// A missing `cmode` field means Stdin / Stdout instances literally
/// cannot be minted with a consensus mode — the semantic contradiction
/// is closed at the type-of-signature level rather than at runtime.
///
/// This pin holds the invariant by refusing:
///   * `constructor(@fd, @cmode)` (arity-2 with cmode)
///   * `constructor(@fd, @canonRoot, @rel, @mode, @cmode)` (File-style)
///   * any signature containing `@cmode` in Stdin.rho or Stdout.rho
///
/// A regression that adds cmode plumbing to Stdin/Stdout would trip
/// this pin, forcing the author to either (a) revert (preferred) or
/// (b) deliberately redesign — with the plan-doc reclassification
/// of stdio-replay to "needed" and matching harness work.
///
/// Sanity check: the invariant relies on the pin catching a
/// substring match against `@cmode` in the constructor signature.
/// A refactor that renames `cmode` to something else (e.g. `mode`)
/// while keeping the same semantic would evade this pin — but
/// `mode` collides with File.rho's actual open-mode arg, so the
/// naming convention itself acts as a secondary trip-wire.
#[test]
fn stdio_agents_have_no_cmode_arg_and_stay_oracular() {
    let stdin_src = include_str!("../../casper/src/main/resources/Stdin.rho");
    let stdout_src = include_str!("../../casper/src/main/resources/Stdout.rho");

    // Stdin / Stdout constructor signatures must be exactly `(@fd)`.
    // The plain string check catches the canonical shape; the
    // negative check on `@cmode` catches most plausible refactors.
    for (name, src) in [("Stdin.rho", stdin_src), ("Stdout.rho", stdout_src)] {
        assert!(
            src.contains("constructor(@fd) {"),
            "stdio-oracular regression: {name} must retain the arity-1 \
             `constructor(@fd) {{` signature.  A regression here likely \
             means someone widened the constructor to accept cmode or \
             other state — which would falsely imply stdio has a \
             consensus-mode variant.  Stdio is intrinsically oracular; \
             see the constructor docstring for the full rationale."
        );
        assert!(
            !src.contains("@cmode"),
            "stdio-oracular regression: {name} contains a `@cmode` \
             argument somewhere.  Stdio has no consensus-mode variant \
             (bytes are non-reproducible across nodes by nature).  \
             Either revert the addition OR — if genuinely intended — \
             redesign slice 10c with a capture/replay harness AND \
             reclassify this pin as obsolete."
        );
    }
}
