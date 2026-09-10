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
    fs_entries_per_entry_supplement_cost, fs_entries_stream_close_cost,
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
         linear cost of fs_entries / fs_entries_stream_next / fs_remove_dir."
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
         base for fs_entries / fs_entries_stream_open + _next."
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
// they deliver setup weights:
//   - open + next  → FS_ENTRIES_SETUP = 50 (per-entry supplement for
//     `_next` is a separate two-branch charge via
//     `fs_entries_stream_per_entry_supplement_cost`).
//   - close        → fs_close_cost() = FS_SYSCALL_CONST = 100.
// A8-M-1 (2026-09-03): the bulk `fs_entries_stream_cost(n)` helper
// was retired; the per-fd variants below now carry their own cost
// class strings (`fs_entries_stream_open` / `_next`) so the every-
// handler-charges-its-cost pin still names each handler distinctly.
// Golden values pinned here so a future consensus-observable retune
// of the underlying shapes flips the corresponding pin.

#[test]
fn fs_entries_stream_open_weight_is_pinned() {
    assert_eq!(
        fs_entries_stream_open_cost(),
        Cost::create(FS_ENTRIES_SETUP, "fs_entries_stream_open")
    );
}

#[test]
fn fs_entries_stream_next_weight_is_pinned() {
    assert_eq!(
        fs_entries_stream_next_cost(),
        Cost::create(FS_ENTRIES_SETUP, "fs_entries_stream_next")
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

// A8-M-1 (2026-09-03): fs_entries_stream_weight_at_zero_entries_is_pinned
// and fs_entries_stream_weight_at_10_entries_is_pinned retired
// alongside the bulk `fs_entries_stream_cost(n)` helper.  The per-fd
// variants have their own `_weight_is_pinned` tests above; per-entry
// scaling for `_next` is covered by
// `fs_entries_stream_per_entry_supplement_at_10_is_pinned` below.

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
        // A8-M-1 (2026-09-03): fs_entries_stream_cost(n) helper
        // retired.  The per-fd `_next` handler pairs
        // `fs_entries_stream_next_cost()` (setup) with
        // `fs_entries_stream_per_entry_supplement_cost(n)`; the
        // combined weight must equal `FS_ENTRIES_SETUP + n *
        // FS_ENTRIES_PER_ENTRY` under saturation (matches the retired
        // bulk helper's shape).
        let expected_next = FS_ENTRIES_SETUP
            .saturating_add(FS_ENTRIES_PER_ENTRY.saturating_mul(n.min(i64::MAX as u64) as i64));
        assert_eq!(
            fs_entries_stream_next_cost()
                .value
                .saturating_add(fs_entries_stream_per_entry_supplement_cost(n).value),
            expected_next,
            "slice 9b-iv total-weight drift for _next at n={n}",
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

// A8-M-1 (2026-09-03): fs_entries_stream_cost_saturates_at_u64_max
// retired alongside the bulk helper.  Per-fd variants use
// FS_ENTRIES_SETUP directly (constant) + per-entry supplement (which
// has its own saturation pin).

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

// A8-M-1 (2026-09-03): fs_entries_stream_cost_is_strictly_monotone_
// and_linear retired.  The per-entry supplement helper covers the
// linearity concern for the live `_next` charge path (see the
// supplement pins above).

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

/// Wave-3 helper: inverse of `to_camel_case_handler` — convert
/// `FsFlushHandler` → `fs_flush`.  Used by
/// `handlers_top_comment_phase5_verifying_count_matches_actual`
/// to key the verifying-handler set by snake_case handler name.
fn camel_handler_to_snake(camel: &str) -> String {
    // Strip the `Handler` suffix if present.
    let core = camel.strip_suffix("Handler").unwrap_or(camel);
    let mut out = String::with_capacity(core.len() + 4);
    for (i, c) in core.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// Wave-3 helper: convert `fs_flush` → `FsFlushHandler`, etc.
/// Used by `every_fs_handler_charges_its_cost_helper` to locate
/// the trait-impl block for migrated handlers.
fn to_camel_case_handler(snake_case: &str) -> String {
    let mut out = String::with_capacity(snake_case.len() + 8);
    let mut capitalize_next = true;
    for c in snake_case.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            out.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            out.push(c);
        }
    }
    out.push_str("Handler");
    out
}

/// Wave-3 helper: locate a top-level `impl FsHandler for FsXHandler
/// { ... }` block in `handlers.rs` and return its body slice.
/// Returns None if the anchor is missing (i.e., the handler isn't
/// migrated yet).
///
/// Scans for `\n<anchor> {` — the leading newline + trailing ` {`
/// discriminate the actual impl block from prose-comment
/// references like `// lives at `impl FsHandler for FsXHandler``
/// higher up in the file.
fn trait_impl_block<'a>(src: &'a str, anchor: &str) -> Option<&'a str> {
    let needle = format!("\n{anchor} {{");
    let start = src.find(&needle)? + 1; // skip the leading '\n'
    let after = &src[start..];
    // Trait-impl blocks are top-level; the closing `}` is at column
    // 0, matching `^}\n` in this file's formatting.
    let end = after.find("\n}\n").map(|e| e + 3).unwrap_or(after.len());
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
    use rholang::rust::interpreter::io::handler_trait::FS_HANDLERS;
    // Wave-3 S3.12b (2026-09-09) rewrite: source of truth for the
    // 27 migrated handlers is now FS_HANDLERS.
    // Wave-3 S3.13b (2026-09-10) extension: family-split modules —
    // Stream trait impls live in `handlers_stream.rs`, not
    // `handlers.rs`.  Aggregate content from every candidate file so
    // the trait-impl scan covers all migrated handlers regardless of
    // which family file they live in.
    let handlers_src = include_str!("../src/rust/interpreter/io/handlers.rs");
    let handlers_stream_src = include_str!("../src/rust/interpreter/io/handlers_stream.rs");
    // Future family files (S3.13b continuation): concat with `\n---\n`
    // separator between; trait_impl_block anchor-search still works
    // per-file because each block is bounded by its own `}\n}\n`.
    let all_src: String = format!("{handlers_src}\n// ---\n{handlers_stream_src}");

    let mut missing = Vec::new();
    let mut handlers_to_check: Vec<String> =
        FS_HANDLERS.iter().map(|e| e.name.to_string()).collect();
    handlers_to_check.push("fs_remove_dir".to_string());

    for handler_name in &handlers_to_check {
        let expected_call = format!("costs::{handler_name}_cost(");

        // 1. Migrated handlers: check the trait impl block (may live
        //    in handlers.rs or any handlers_{family}.rs).
        let handler_struct = to_camel_case_handler(handler_name);
        let trait_anchor = format!("impl FsHandler for {handler_struct}");
        if let Some(trait_block) = trait_impl_block(&all_src, &trait_anchor) {
            if trait_block.contains(&expected_call) {
                continue;
            }
        }
        // 2. Trait-exempt fs_remove_dir: still lives inside
        //    `impl FsProcesses` as a `pub async fn fs_remove_dir`
        //    wrapper in handlers.rs.
        let signature_prefix = format!("    pub async fn {handler_name}(");
        if let Some(body) = method_body(handlers_src, &signature_prefix) {
            if body.contains(&expected_call) {
                continue;
            }
        }
        missing.push(handler_name.clone());
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

/// Docstring-count enforcement pin (2026-09-03 review follow-up).
/// The top-of-file comment in `handlers.rs` claims "28 native
/// filesystem handlers"; this pin catches drift when a future handler
/// is added or removed without a corresponding docstring bump.
///
/// A handler count check is the cheapest guarantee that the
/// informational categorization stays accurate.  The
/// per-category breakdown (`20 fs syscalls + 4 lock natives + 3
/// per-fd stream natives + 1 quarantine helper`) is not enforced
/// separately — a new lock native or fs syscall trips this pin,
/// forcing the author to update both the count AND the category
/// prose.
///
/// Why exact-count rather than lower-bound: an unchanged docstring
/// after adding a handler is a silent lie about the shipped
/// surface, and the doc-writer is the person best positioned to
/// re-check their own categorization.  Exact-count enforcement
/// gives them the signal.
#[test]
fn handlers_top_comment_count_matches_actual_handlers() {
    use rholang::rust::interpreter::io::handler_trait::FS_HANDLERS;
    let src = include_str!("../src/rust/interpreter/io/handlers.rs");
    // Wave-3 S3.12b (2026-09-09) rewrite: post-wrapper retirement,
    // the source of truth for the migrated count is the FS_HANDLERS
    // distributed slice.  fs_remove_dir stays trait-exempt with a
    // `pub async fn fs_remove_dir` wrapper on `impl FsProcesses`.
    let migrated = FS_HANDLERS.len();
    let exempt = src
        .lines()
        .filter(|line| line.starts_with("    pub async fn fs_"))
        .count();
    // fs_remove_dir is the only remaining wrapper.
    assert_eq!(
        exempt, 1,
        "S3.12b: expected exactly 1 remaining `pub async fn fs_*` wrapper \
         on `impl FsProcesses` (fs_remove_dir, trait-exempt).  Found {exempt}. \
         If a new trait-exempt handler was added, update this pin."
    );
    let actual = migrated + exempt;
    // Extract the count claimed in the top comment (first
    // "The N native filesystem handlers" line).
    let claimed = src
        .lines()
        .find_map(|line| {
            let trimmed = line.trim_start_matches("// ").trim_start_matches("//");
            let after_the = trimmed.strip_prefix("The ")?;
            let (num_str, _rest) = after_the.split_once(' ')?;
            num_str.parse::<usize>().ok()
        })
        .expect(
            "handlers.rs top comment must open with `// The N native filesystem handlers.` — \
             docstring shape changed",
        );
    assert_eq!(
        claimed, actual,
        "handlers.rs top-comment count drift: comment claims `{claimed}` handlers, actual \
         `pub async fn fs_` count is `{actual}`.  Update the comment's count AND re-verify \
         the category breakdown (`20 fs syscalls + 4 lock natives + 3 per-fd stream natives \
         + 1 quarantine helper`) to match the new total.",
    );
}

/// M-51 fix (2026-09-08, A7-F19) — companion to the handler count
/// pin above.  The top-of-file comment ALSO claims a specific number
/// of "Phase-5-verifying handlers" (currently 15).  Pre-fix, only
/// the "28 total" claim was enforced; the "14" (later "15") verify
/// count drifted silently when fs_exists's Consensus ban was lifted.
///
/// Count = unique handler fns whose body contains
/// `match verify_reply_hash_matches_cached`, plus any migrated
/// handler (`impl FsHandler for FsXHandler`) that sets
/// `const VERIFYING: bool = true`.  Wave-3 S3.5 (2026-09-09):
/// migrated verifying handlers move the verify_reply_hash_matches_
/// cached call from their inline body into the framework's
/// `dispatch_via_trait`, so the pin now scans both surfaces.
///
/// `fs_remove_dir` has TWO verify sites (recursive + non-recursive
/// branches) but counts once because it's one handler.
#[test]
fn handlers_top_comment_phase5_verifying_count_matches_actual() {
    let src = include_str!("../src/rust/interpreter/io/handlers.rs");

    // Pass 1: pre-wave-3 style — scan `pub async fn fs_X` bodies
    // for the inline `match verify_reply_hash_matches_cached`.
    let mut current_fn: Option<&str> = None;
    let mut verifying: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for line in src.lines() {
        if let Some(rest) = line.strip_prefix("    pub async fn ") {
            if let Some((name, _)) = rest.split_once('(') {
                current_fn = Some(name);
            }
        }
        if line.contains("match verify_reply_hash_matches_cached") {
            if let Some(name) = current_fn {
                verifying.insert(name.to_string());
            }
        }
    }

    // Pass 2: wave-3 style — scan `impl FsHandler for FsXHandler`
    // blocks with `const VERIFYING: bool = true`.  Handler struct
    // `FsXHandler` maps back to `fs_x` (snake_case).
    let mut cursor = 0usize;
    while let Some(rel) = src[cursor..].find("impl FsHandler for Fs") {
        let abs = cursor + rel;
        let after = &src[abs..];
        // Handler name: `impl FsHandler for FsXYHandler {` → "fs_x_y".
        // Extract token between "for " and " {".
        let struct_tok = after
            .split_once("for ")
            .and_then(|(_, rest)| rest.split_once(' '))
            .map(|(s, _)| s)
            .unwrap_or("");
        cursor = abs + "impl FsHandler for ".len();
        let name_snake = camel_handler_to_snake(struct_tok);
        // Scan the trait-impl block for `const VERIFYING: bool = true`.
        let end = after.find("\n}\n").map(|e| e + 3).unwrap_or(after.len());
        let block = &after[..end];
        if block.contains("const VERIFYING: bool = true") {
            verifying.insert(name_snake);
        }
    }

    let actual = verifying.len();

    // Extract the count claimed in the top comment's "N Phase-5-
    // verifying handlers" phrase.
    let claimed = src
        .lines()
        .find_map(|line| {
            let trimmed = line.trim_start_matches("// ").trim_start_matches("//");
            // Find e.g. "for the 15 Phase-5-verifying handlers"
            let after_for_the = trimmed.split("for the ").nth(1)?;
            let (num_str, rest) = after_for_the.split_once(' ')?;
            if !rest.starts_with("Phase-5-verifying") {
                return None;
            }
            num_str.parse::<usize>().ok()
        })
        .expect(
            "handlers.rs top comment must contain `for the N Phase-5-verifying handlers` — \
             docstring shape changed",
        );

    assert_eq!(
        claimed, actual,
        "M-51: handlers.rs top-comment Phase-5-verifying count drift: comment claims \
         `{claimed}` verifying handlers, actual count of unique handler fns with \
         `match verify_reply_hash_matches_cached` is `{actual}` ({:?}).  Update the \
         top comment's claim (both the leading count AND the trailing `N/28 verify \
         matrix` phrase) to match.",
        verifying
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
    // Wave-3 S3.12b (2026-09-09) rewrite: post-wrapper retirement,
    // fs_entries lives in `impl FsHandler for FsEntriesHandler`.
    // The setup charge is returned by `pre_charge_cost()`; the
    // per-entry supplement is returned by `post_reply_supplement()`.
    // The framework handles the both-branches invariant: it applies
    // `post_reply_supplement` on the is_replay tautological path AND
    // on the leader / verify paths (see handler_trait.rs).  So a
    // single `post_reply_supplement` override on the trait impl is
    // sufficient — the framework fires it on both branches.
    let body = trait_impl_block(src, "impl FsHandler for FsEntriesHandler")
        .expect("FsEntriesHandler trait impl must exist in handlers.rs");

    assert!(
        body.contains("costs::fs_entries_cost(0)"),
        "slice 9b-iv regression: fs_entries must retain its setup-only \
         `costs::fs_entries_cost(0)` charge at handler entry \
         (`pre_charge_cost` return) — the per-entry supplement is \
         layered on top, not a replacement."
    );

    let supplement_call = "costs::fs_entries_per_entry_supplement_cost(";
    let n_supplement = body.matches(supplement_call).count();
    assert!(
        n_supplement >= 1,
        "slice 9b-iv regression: FsEntriesHandler must charge \
         `{supplement_call}` via `post_reply_supplement` (currently \
         {n_supplement} call site(s) in the trait impl).  The framework \
         applies post_reply_supplement on BOTH the replay tautological \
         path AND the leader / verify paths — see dispatch_via_trait in \
         handler_trait.rs.  A missing supplement means both leader and \
         follower undercharge every non-empty directory listing, causing \
         the authority_cost_witness cost to drift from realized cost."
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

// A8-M-1 (2026-09-03): arity3_entries_stream_stub_still_returns_
// fserr_unsupported retired.  The bulk `fs_entries_stream` handler
// stub, its URN binding at `fs_genesis.rs`, its dispatch registration
// at `rho_runtime.rs`, and its FixedChannels byte-54 slot were all
// removed — there's no more stub to guard.  Byte 54 is documented as
// reserved (do NOT reassign) in `system_processes.rs`; the source-
// scan pins below enforce the reservation.

/// A8-M-1 review-follow-up pin (2026-09-03).  `system_processes.rs`
/// reserved `byte_name(54)` for the retired bulk `fs_entries_stream`
/// FixedChannel.  A future PR that added a new FixedChannels function
/// returning `byte_name(54)` alongside the reservation comment would
/// compile cleanly AND pass every downstream test (there's no channel-
/// identity collision detector at the byte level).  The comment
/// enforces intent; this pin enforces it mechanically — source-scan
/// for any `byte_name(54)` occurrence in `system_processes.rs`.
///
/// Why not just leave the retired `fs_entries_stream() -> byte_name(54)`
/// function as an inert placeholder: it would reintroduce a function
/// that has zero callers and no semantic meaning, making the surface
/// deceptively larger.  This pin is the smallest defense-in-depth
/// that still catches the reservation being violated.
#[test]
fn fixed_channels_byte_54_stays_reserved_after_a8_m1_retirement() {
    let src = include_str!("../src/rust/interpreter/system_processes.rs");
    // `byte_name(54)` at any use site inside the FixedChannels
    // module trips this pin.  A comment mentioning "byte 54" is
    // ignored by this pattern (we scan for the exact call form).
    assert!(
        !src.contains("byte_name(54)"),
        "A8-M-1 reserved slot violated: `byte_name(54)` reappeared in \
         system_processes.rs.  Byte 54 held the retired bulk \
         `fs_entries_stream` FixedChannel and MUST NOT be reassigned — \
         any historical channel-derived identity referencing byte 54 \
         would silently alias the new native.  Pick an unused byte \
         (as of A8-M-1, the next free slot is 69) and add it there \
         instead.  If reviving `fs_entries_stream` is genuinely the \
         intent, restore the full retirement (handler + URN binding + \
         dispatch registration + cost helper) and retire this pin."
    );
}

/// M-39 fix (2026-09-08, A8-F6): FixedChannels assigned-set pin.
///
/// Pre-fix, only byte 54 had an explicit reservation pin.  Bytes 9,
/// 38, and 39 in FixedChannels are ALSO skipped (never assigned to
/// a native), but no test enforced those gaps.  A future author
/// could assign one of them to a new native without noticing —
/// aliasing any historical channel-derived identity that was
/// pre-computed against a byte-N `PrivateName` in that slot (e.g.,
/// from an offline analysis, a debug tool that snapshots the
/// tuplespace, or a legacy sidecar that persisted channel bytes).
///
/// This pin source-scans `system_processes.rs` for every
/// `byte_name(N)` call inside `impl FixedChannels`, extracts the
/// full assigned-N set, and asserts:
///   1. No byte in `RESERVED_GAPS` (9, 38, 39, 54) is assigned.
///   2. The assigned set exactly matches the expected set (catches
///      silent removal too — same class as M-38's mapping pin).
///
/// Adding a new FixedChannels native is a two-line change:
///   - Add `pub fn foo() -> Par { byte_name(N) }` in
///     `system_processes.rs` (with N = next unused byte).
///   - Add `N` to `EXPECTED_ASSIGNED` below.
///
/// If you genuinely need to open a currently-reserved gap, do the
/// same three retirements the original assignment did (add native
/// handler + URN binding + dispatch registration + cost helper),
/// then remove the byte from `RESERVED_GAPS`.  The pin fires until
/// both sides agree.
/// Scanner used by `fixed_channels_assigned_set_pinned_and_gaps_reserved`
/// (and its sanity-check companions).  Extracted so the scan
/// itself is testable against synthetic sources.
///
/// M-39 review-fix (2026-09-08): filters `//` line comments FIRST,
/// then strips `/* ... */` block comments.  Order matters: a `/*`
/// sequence inside a `//` line comment (e.g., the URN glob
/// `rho:io:fs:native:1.0.0/*` on line 284 of the real source)
/// would otherwise fool the block-comment stripper into eating
/// everything up to the next actual `*/`, which sits well
/// downstream and truncates the scan.  The impl-block boundary is
/// anchored on `impl BodyRefs {` if present, then `pub struct
/// BodyRefs`, then any next impl / pub struct.
fn extract_fixed_channels_assigned_bytes(src: &str) -> Vec<u32> {
    let impl_start = match src.find("impl FixedChannels {") {
        Some(i) => i,
        None => return Vec::new(),
    };
    let after = &src[impl_start..];
    let end_rel = after[1..]
        .find("\nimpl BodyRefs")
        .or_else(|| after[1..].find("\npub struct BodyRefs"))
        .or_else(|| after[1..].find("\npub struct "))
        .or_else(|| after[1..].find("\nimpl "))
        .map(|i| i + 1)
        .unwrap_or(after.len());
    let block = &after[..end_rel];

    // Filter line comments FIRST, then strip block comments.
    // A `/*` inside a `//` line comment is not a real block
    // comment; if we ran block-comment stripping first, the URN
    // glob `rho:io:fs:native:1.0.0/*` (inside a `//` line comment
    // in the real source) would trigger the stripper to eat
    // everything up to the next `*/` and truncate the scan.
    let non_line_comments: String = block
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let clean = strip_block_comments(&non_line_comments);

    let mut assigned: Vec<u32> = Vec::new();
    for line in clean.lines() {
        let mut rest = line;
        while let Some(idx) = rest.find("byte_name(") {
            let tail = &rest[idx + "byte_name(".len()..];
            let close = tail.find(')').unwrap_or(0);
            let n_str = &tail[..close];
            if let Ok(n) = n_str.trim().parse::<u32>() {
                assigned.push(n);
            }
            rest = &tail[close..];
        }
    }
    assigned.sort_unstable();
    assigned
}

/// Strip `/* ... */` block comments from `src`.  Non-nesting;
/// works over a single pass.  Used by the M-39 scanner so a
/// `/* byte_name(9) */` inside `impl FixedChannels` doesn't
/// silently register byte 9 as assigned.
fn strip_block_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // Find the matching */.
            let start = i + 2;
            let mut j = start;
            while j + 1 < bytes.len() && !(bytes[j] == b'*' && bytes[j + 1] == b'/') {
                j += 1;
            }
            i = if j + 1 < bytes.len() {
                j + 2
            } else {
                bytes.len()
            };
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// M-39 review-fix (2026-09-08, S2): scanner block-comment sanity
/// pin.  Proves that a `/* byte_name(9) */` inside a synthetic
/// `impl FixedChannels` is NOT registered as an assignment.
/// Pre-fix, the scanner filtered only `//` line comments; a
/// block-commented byte_name call would have silently registered.
#[test]
fn extract_fixed_channels_scanner_strips_block_comments() {
    let synthetic = "\
impl FixedChannels {
    /* byte_name(9) */
    pub fn foo() -> Par { byte_name(42) }
}
impl BodyRefs {
    pub const FOO: i64 = 0;
}
";
    let bytes = extract_fixed_channels_assigned_bytes(synthetic);
    assert_eq!(
        bytes,
        vec![42],
        "block-commented byte_name(9) must not register; scanner \
         returned {bytes:?}"
    );
}

/// M-39 review-fix (2026-09-08, C4): scanner boundary sanity pin.
/// Proves that a `byte_name(N)` inside `impl BodyRefs` (a
/// downstream sibling block) is NOT picked up by the
/// FixedChannels scanner.  Pre-fix, the boundary was
/// `\npub struct ` OR `\nimpl `; if BodyRefs were declared with
/// leading whitespace (say inside a mod block), the scanner
/// would stray.  Post-fix, the boundary explicitly prefers
/// `\nimpl BodyRefs`.
#[test]
fn extract_fixed_channels_scanner_stops_at_body_refs_impl() {
    let synthetic = "\
impl FixedChannels {
    pub fn foo() -> Par { byte_name(42) }
}
impl BodyRefs {
    pub const BAR: i64 = 100;
    // A hypothetical `byte_name(99)` inside BodyRefs — the scanner
    // must NOT pick this up.
    pub fn should_not_leak() -> Par { byte_name(99) }
}
";
    let bytes = extract_fixed_channels_assigned_bytes(synthetic);
    assert_eq!(
        bytes,
        vec![42],
        "byte_name(99) inside impl BodyRefs must NOT leak into the \
         FixedChannels assigned set; scanner returned {bytes:?}"
    );
}

/// M-39 review-fix (2026-09-08, S2 regression pin): a `/*` glob
/// character inside a `//` line comment must NOT trigger the
/// block-comment stripper to eat downstream lines.  This
/// pattern actually occurs in the real source at line 284
/// (`rho:io:fs:native:1.0.0/*`), and an earlier revision of the
/// scanner truncated bytes 40-68 because of this exact
/// confusion.
#[test]
fn extract_fixed_channels_scanner_ignores_glob_in_line_comment() {
    let synthetic = "\
impl FixedChannels {
    pub fn foo() -> Par { byte_name(1) }
    // URN prefix: rho:io:fs:native:1.0.0/*   <-- glob, not a real /* */ block
    pub fn bar() -> Par { byte_name(40) }
    pub fn baz() -> Par { byte_name(41) }
}
impl BodyRefs {
    pub const X: i64 = 0;
}
";
    let bytes = extract_fixed_channels_assigned_bytes(synthetic);
    assert_eq!(
        bytes,
        vec![1, 40, 41],
        "regression: /* inside a // comment must not truncate the scan; \
         got {bytes:?}"
    );
}

/// M-39 review-fix (2026-09-08, C4): scanner picks up multiple
/// byte_name calls per line.  Real source has one per line today,
/// but the scanner's inner `while` loop must handle multi-hit.
#[test]
fn extract_fixed_channels_scanner_handles_multiple_calls_per_line() {
    let synthetic = "\
impl FixedChannels {
    pub fn foo() -> Par { byte_name(1) }
    pub fn bar() -> Par { byte_name(2) }
}
";
    let bytes = extract_fixed_channels_assigned_bytes(synthetic);
    assert_eq!(bytes, vec![1, 2]);
}

#[test]
fn fixed_channels_assigned_set_pinned_and_gaps_reserved() {
    let src = include_str!("../src/rust/interpreter/system_processes.rs");
    let assigned = extract_fixed_channels_assigned_bytes(src);

    // Expected assigned set (M-39 canonical, 2026-09-08).  Adding
    // a new native means adding its byte here AND at its
    // declaration site in `system_processes.rs`.
    let expected_assigned: &[u32] = &[
        0, 1, 2, 3, 4, 5, 6, 7, 8, // io + crypto
        10, 11, 12, 13, 14, 15, 16, 17, 18, 19, // block/reg/vault
        20, 21, 22, 23, 24, 25, 26, 27, 28, 29, // gpt/ollama/etc
        30, 31, 32, 33, 34, 35, 36, 37, // chroma + registry_lookup
        40, 41, 42, 43, 44, 45, 46, 47, 48, 49, // fs_open..fs_truncate
        50, 51, 52, 53, // fs_flush..fs_entries
        55, 56, 57, 58, 59, 60, 61, // fs_rename..fs_quarantine
        62, 63, 64, 65, // fs_lock_*
        66, 67, 68, // fs_entries_stream_*
    ];

    // Reserved gaps: bytes intentionally NOT assigned.  Byte 54 is
    // the retirement gap (A8-M-1); 9, 38, 39 are historical unused
    // slots pinned here so they stay unused.
    let reserved_gaps: &[u32] = &[9, 38, 39, 54];

    // 1. No reserved gap has been assigned.
    for gap in reserved_gaps {
        assert!(
            !assigned.contains(gap),
            "M-39: FixedChannels byte {gap} is a reserved gap but got assigned. \
             Any pre-computed channel-derived identity against this byte would \
             silently alias the new native.  Pick the next unused byte (as of \
             M-39, the next free is 69) or explicitly retire this reservation."
        );
    }

    // 2. Assigned set matches expected.  Catches silent removal
    // (missing byte) AND silent addition (extra byte) — either
    // direction is a hard-fork surface change.
    assert_eq!(
        assigned,
        expected_assigned.to_vec(),
        "M-39: FixedChannels assigned-set drifted.  Either a native \
         was added without updating `expected_assigned`, or one was \
         silently removed.  Both directions are hard-fork surface \
         changes affecting channel-derived identity."
    );
}

/// A8-M-1 review-follow-up pin (2026-09-03).  Companion to the byte
/// 54 pin above: `BodyRefs::FS_ENTRIES_STREAM = 54` was also retired
/// and the const slot marked reserved.  Source-scan enforces no new
/// `pub const * = 54` inside `BodyRefs`.
#[test]
fn body_refs_54_stays_reserved_after_a8_m1_retirement() {
    let src = include_str!("../src/rust/interpreter/system_processes.rs");
    let violation = find_body_refs_54_violation(src);
    assert!(
        violation.is_none(),
        "A8-M-1 reserved slot violated: a BodyRefs const was assigned = 54 \
         (line: `{}`).  BodyRef 54 held the retired `FS_ENTRIES_STREAM` and \
         MUST NOT be reassigned.  Pick an unused body-ref id (as of A8-M-1, \
         next free is 69).",
        violation.unwrap_or_default()
    );
}

/// Scanner for `body_refs_54_stays_reserved_after_a8_m1_retirement`.
/// Extracted so the helper's logic is unit-testable against synthetic
/// source strings (see below) — proving the scanner fires on a real
/// violation is otherwise sandbox-blocked because writing a
/// `= 54;` line would itself violate the reservation.
fn find_body_refs_54_violation(src: &str) -> Option<String> {
    let body_refs_section = src
        .split("mod BodyRefs")
        .nth(1)
        .or_else(|| src.split("impl BodyRefs").nth(1))
        .or_else(|| src.split("pub struct BodyRefs").nth(1))
        .unwrap_or(src);
    for line in body_refs_section.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        if trimmed.starts_with("pub const ") && trimmed.contains(": i64 = 54;") {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Scanner sanity pin (2026-09-03): the `find_body_refs_54_violation`
/// helper MUST return `Some(...)` for a synthetic source containing a
/// `pub const X: i64 = 54;` line inside a `mod BodyRefs` block, and
/// `None` for a source without that pattern.  Without this, the
/// scanner could silently regress to "always returns None" (e.g. if
/// the split-on-marker logic misfires) and the `body_refs_54_stays_
/// reserved` pin would false-green.
#[test]
fn find_body_refs_54_violation_catches_synthetic_violation() {
    let violating = "\
mod BodyRefs {
    pub const FIRST: i64 = 1;
    pub const REVIVED_ENTRIES_STREAM: i64 = 54;
    pub const OTHER: i64 = 100;
}
";
    let got = find_body_refs_54_violation(violating);
    assert!(
        got.is_some() && got.as_deref().unwrap().contains("REVIVED_ENTRIES_STREAM"),
        "scanner must catch the `= 54;` line; got {got:?}"
    );

    let clean = "\
mod BodyRefs {
    pub const FIRST: i64 = 1;
    pub const OTHER: i64 = 100;
    // Byte 54 is reserved — a comment mentioning 54 must NOT trip the pin.
    pub const NEXT_FREE: i64 = 69;
}
";
    assert_eq!(
        find_body_refs_54_violation(clean),
        None,
        "scanner must not false-fire on a clean source (comment mentions of 54 don't count)"
    );
}

/// RH-2 review-follow-up pin (2026-09-04).  Every stream producer's
/// release ceremony MUST invoke the `releaseSeqLockOnce` helper
/// (introduced by RH-2) instead of hand-inlining `for (@lockState
/// <- lockCell) { match ... }` — otherwise the load-bearing hazard
/// the review flagged (release-once bookkeeping divergence between
/// stream methods) returns.  Source-scan asserts File.rho contains
/// EXACTLY one `for (@lockState <- lockCell)` call site (the
/// helper's own implementation at `contract releaseSeqLockOnce`).
///
/// Why exact-count-1: the helper contract itself uses the pattern
/// at its definition site.  Any other site is a regression — a
/// stream method that skipped the helper and re-inlined the
/// ceremony.  Comment mentions of the pattern (docstring examples
/// referencing the old shape) are ignored by the scanner because
/// they're inside `//` comments.
///
/// A source-scan pin here is the right defense: adding a new
/// stream method (chars/bytes/lines/etc.) that inlines the
/// release would compile cleanly, pass all existing E2E tests
/// (the new method's own tests would validate its individual
/// behavior), and silently reopen the divergence hazard.  This
/// pin fires at test time with an actionable message pointing at
/// the helper.
#[test]
fn file_rho_stream_release_ceremony_delegates_to_release_seq_lock_once() {
    let src = include_str!("../../casper/src/main/resources/File.rho");
    // Naive `contains` would catch comments too; strip `//` lines
    // before scanning to isolate genuine call sites.
    let non_comment_source: String = src
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let hits: Vec<usize> = non_comment_source
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            if line.contains("for (@lockState <- lockCell)") {
                Some(i + 1)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "RH-2 delegation regression: File.rho has {} non-comment site(s) matching \
         `for (@lockState <- lockCell)`, expected exactly 1 (the helper's own \
         implementation inside `contract releaseSeqLockOnce`).  Extra sites at \
         lines {hits:?} suggest a new stream method (or a regression to an old \
         one) re-inlined the release ceremony instead of delegating to \
         `releaseSeqLockOnce!(*lockCell, *doneCh)`.  See RH-2 commit \
         `3c74be56f` for the pattern.  If a genuinely new use case requires \
         the raw pattern (e.g., a non-stream method with different tail \
         sequencing), extend the helper OR add a sister helper AND update \
         this pin.",
        hits.len()
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
    // Wave-3 S3.12b (2026-09-09) rewrite: post-wrapper retirement,
    // fs_lock_range / fs_lock_sequential live in
    // `impl FsHandler for FsLockRangeHandler` / `FsLockSequentialHandler`.
    // Anchor on trait impls.

    // fs_lock_range must NOT contain a match arm of the legacy
    // arity-7 shape.  Post-tightening the only arm is the 8-arg
    // pattern; a resurrected shim would add `[fd, off, len, mode,
    // holder, cmode, ack]` (7 identifiers) as a second arm.
    let range_body = trait_impl_block(src, "impl FsHandler for FsLockRangeHandler")
        .expect("FsLockRangeHandler trait impl must exist");
    assert!(
        !range_body.contains("[fd, off, len, mode, holder, cmode, ack]"),
        "arity-tightening regression: fs_lock_range trait impl contains \
         the legacy arity-7 match arm `[fd, off, len, mode, holder, \
         cmode, ack]`.  The shim was retired in commit 5e8f3e2a0; \
         all File.rho callers now pass arity 8 with explicit wait: \
         Bool.  If the shim was intentionally resurrected, ALSO bump \
         `fs_native_def(\"lockRange\", 8)` back to 7 in \
         rho_runtime.rs — otherwise the dispatch will still enforce \
         arity 8 at the channel binding and the shim is dead code."
    );

    let seq_body = trait_impl_block(src, "impl FsHandler for FsLockSequentialHandler")
        .expect("FsLockSequentialHandler trait impl must exist");
    assert!(
        !seq_body.contains("[fd, holder, cmode, ack]"),
        "arity-tightening regression: fs_lock_sequential trait impl \
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

    // M-19 review follow-up (2026-09-04, Gap 2): the Rholang literal
    // `65536` MUST match the Rust `MAX_CHUNK_ITEMS` constant folded
    // into `consensus_runtime_fingerprint`.  A per-validator patch of
    // the Rholang literal without also updating the Rust constant
    // would peer silently (fingerprint unchanged, but the runtime
    // caps diverge).  Reading the Rust constant here proves the
    // pair-alignment at test time.
    let rust_cap: u64 = rholang::rust::interpreter::io::MAX_CHUNK_ITEMS;
    assert_eq!(
        rust_cap, 65536,
        "MAX_CHUNK_ITEMS Rust constant must be 65536 (the value the \
         Rholang literal in Stream.rho encodes)"
    );
    assert!(
        body.contains(&rust_cap.to_string()),
        "slice 9c-i cap regression: Stream.rho::chunk(@n) must enforce \
         MAX_CHUNK_ITEMS={rust_cap}.  The literal `{rust_cap}` was not \
         found in the method body; a silent removal of the cap opens an \
         unbounded reply-payload allocation vector.  If MAX_CHUNK_ITEMS \
         was intentionally changed on the Rust side, update Stream.rho \
         to match AND regenerate the consensus fingerprint golden hex \
         in `consensus_fingerprint.rs`."
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

/// M-29 (2026-09-05, RH-A5-10) — pin `Dir.rho`'s header method
/// summary to the current arity-2 signatures for `openFile` and
/// `openDir`.  Pre-fix (B4, 2026-09-03) both methods took bare
/// positional `@mode`; post-B4 both take `(@rel, @options)`.  A
/// stale docstring at the top of Dir.rho listed the arity-1 form,
/// which contradicted the actual signatures.  The comment is now
/// updated; this pin catches a regression that reverts the header
/// to the arity-1 form.
#[test]
fn dir_rho_header_lists_options_signatures_for_open_methods() {
    let src = include_str!("../../casper/src/main/resources/Dir.rho");
    let header_end = src.find("new Dir,").expect("Dir.rho outer new");
    let header = &src[..header_end];
    for name in ["openFile", "openDir"] {
        let sig_options = format!("`{name}(rel, options)`");
        let sig_mode = format!("`{name}(rel, mode)`");
        assert!(
            header.contains(&sig_options),
            "RH-A5-10 regression: Dir.rho's header method summary \
             must list `{name}(rel, options)` (post-B4 arity-2 form)."
        );
        assert!(
            !header.contains(&sig_mode),
            "RH-A5-10 regression: Dir.rho's header method summary \
             still lists the retired arity-1 form `{name}(rel, mode)`. \
             The `B4` slice (2026-09-03) migrated these methods to \
             `(@rel, @options)`; the header docstring must not \
             advertise the old shape."
        );
    }
}

/// M-30 (2026-09-05, RH-A5-11) — pin the `Fs.rho` module-level
/// `@[*fsRevokedP]!(false)` init at module scope.
///
/// The reviewer flagged this as non-atomic in principle: if any
/// Fs method could race with genesis composition, its `<<-` peek
/// at `fsRevokedP` would read `Nil`.  In practice module init runs
/// before any user code — the composed FsGenesis pipeline evaluates
/// the module body to quiescence before publishing the
/// `bundle+{*fs}` at the registry URI, so no method can race with
/// the init.  We accept the current shape as-is (no defensive
/// `new snapCh in { ... }` wrapper) and pin it here so a future
/// slice that changes the composition semantics has to update this
/// pin and re-verify the invariant.
///
/// The pin also asserts the init is at MODULE scope (outside the
/// `agent Fs {` block) — moving it into a per-method arm would
/// break the "false parked before any method activates" invariant.
#[test]
fn fs_rho_module_level_fs_revoked_init_is_at_outer_scope() {
    let src = include_str!("../../casper/src/main/resources/Fs.rho");
    let init_idx = src
        .find("@[*fsRevokedP]!(false)")
        .expect("RH-A5-11 regression: Fs.rho must initialize fsRevokedP to false at module scope");
    let agent_start = src.find("agent Fs {").expect("Fs.rho agent Fs block");
    assert!(
        init_idx < agent_start,
        "RH-A5-11 regression: `@[*fsRevokedP]!(false)` must appear \
         BEFORE the `agent Fs {{` block (module scope).  Found at \
         byte {init_idx}, agent starts at {agent_start}.  Moving it \
         inside a method body would break the \"false parked before \
         any method activates\" invariant."
    );
}

/// M-32 (2026-09-05, RH-A5-13) — pin `Buffer.beginFill`'s return
/// shape to the bare dereferenced `*fillToken` (not a `bundle+` wrap).
///
/// The reviewer flagged this as a design subtlety: `endFill`'s
/// equality check `lease == presented` needs the same Par shape on
/// both sides, so `beginFill` returns `*fillToken` directly (the
/// unforgeable Name's dereferenced form) rather than
/// `bundle+{*fillToken}`.  The token is still unforgeable — a
/// caller cannot construct it — and can only be used by presenting
/// it back to endFill.
///
/// This pin catches a regression that adds a `bundle+` wrap on the
/// beginFill return — a change that would look "more secure" at
/// first glance but would silently break endFill's equality check.
#[test]
fn buffer_begin_fill_returns_bare_dereferenced_fill_token() {
    let src = include_str!("../../casper/src/main/resources/Buffer.rho");
    let start = src
        .find("method beginFill() {")
        .expect("Buffer.rho::method beginFill");
    let after = &src[start..];
    let end = after[1..]
        .find("method ")
        .map(|i| i + 1)
        .unwrap_or(after.len());
    let body = &after[..end];
    assert!(
        body.contains("return!([true, *fillToken])"),
        "RH-A5-13 regression: Buffer.beginFill must return \
         `[true, *fillToken]` (bare dereferenced Name).  A bundle+ \
         wrap would break endFill's `lease == presented` equality \
         check (Par shape mismatch)."
    );
    assert!(
        !body.contains("bundle+{*fillToken}") && !body.contains("bundle+{ *fillToken}"),
        "RH-A5-13 regression: Buffer.beginFill introduced a `bundle+` \
         wrap around fillToken — this looks more secure but breaks \
         endFill's equality check.  If bundle wrapping is desired, \
         endFill must also unwrap before comparing."
    );
}

/// M-28 (2026-09-04, RH-A5-9) — pin `Stream.rho`'s inner-new
/// clause names so cross-lib references would surface as drift.
///
/// `Stream.rho` has TWO stacked `new` clauses at line 88 and 89:
///   `new Stream, paramsP, stateP, gatherN, foldLoop, ... in {`
///   `new foldConcurrentDispatch, foldConcurrentWorker,
///        mapReduceDispatch, mapReduceWorker, collectDone,
///        collectPartials, foldPartials in {`
/// `lib_body` (rho_source.rs::extract_lib_body) strips only the
/// OUTERMOST `new ... in {`, so the inner `new` is preserved inline
/// in the composed FsGenesis body.  This works today because no
/// other lib references the 7 inner-scope names, but the RH-1 outer-
/// new drift test (`extract_outer_new_names`) reads only the FIRST
/// `new` clause per file and cannot detect a cross-lib reference
/// against an inner-scope name.
///
/// This source-scan pin enumerates the inner-new names and asserts:
///
/// 1. They appear inside Stream.rho's SECOND `new` clause (proving
///    the two-stacked-clause structure hasn't been rearranged).
/// 2. They do NOT appear in any other library's outer-new clause
///    (proving no cross-lib reference exists).
///
/// A regression that extracts one of the 7 inner-scope names to a
/// cross-lib reference would fail assertion (2) — surfacing the
/// drift the RH-1 test cannot see.
#[test]
fn stream_rho_inner_new_names_are_not_cross_lib_referenced() {
    const INNER_NEW_NAMES: &[&str] = &[
        "foldConcurrentDispatch",
        "foldConcurrentWorker",
        "mapReduceDispatch",
        "mapReduceWorker",
        "collectDone",
        "collectPartials",
        "foldPartials",
    ];
    let stream_src = include_str!("../../casper/src/main/resources/Stream.rho");

    // Assertion (1): every inner-new name appears in Stream.rho's
    // SECOND top-level new clause.  We locate the first `new ... in
    // {` end and search forward for the next `new ... in {`; the
    // inner-new names must all appear in the substring between the
    // second `new` and its `in {`.
    let first_in_idx = stream_src
        .find(" in {")
        .expect("Stream.rho must have an outer `new ... in {`");
    let after_first = &stream_src[first_in_idx + 5..];
    let second_new_idx = after_first
        .find("new ")
        .expect("Stream.rho must have a second nested `new` clause");
    let second_new_start = &after_first[second_new_idx..];
    let second_in_idx = second_new_start
        .find(" in {")
        .expect("Second `new` clause must have `in {`");
    let second_clause = &second_new_start[..second_in_idx];
    for name in INNER_NEW_NAMES {
        assert!(
            second_clause.contains(name),
            "RH-A5-9 regression: Stream.rho's inner-new clause is \
             expected to bind `{name}` but the name was not found in \
             the clause between the second `new` keyword and its \
             `in {{` — either the clause structure changed or a \
             variable was renamed.  Second clause was: {second_clause:?}"
        );
    }

    // Assertion (2): none of the inner-new names appears in any
    // other library's outer-new clause.  Check File.rho, Dir.rho,
    // Fs.rho, Buffer.rho, Stdin.rho, Stdout.rho — the six sibling
    // libs whose outer news share the composer's scope.
    let sibling_libs: &[(&str, &str)] = &[
        (
            "File.rho",
            include_str!("../../casper/src/main/resources/File.rho"),
        ),
        (
            "Dir.rho",
            include_str!("../../casper/src/main/resources/Dir.rho"),
        ),
        (
            "Fs.rho",
            include_str!("../../casper/src/main/resources/Fs.rho"),
        ),
        (
            "Buffer.rho",
            include_str!("../../casper/src/main/resources/Buffer.rho"),
        ),
        (
            "Stdin.rho",
            include_str!("../../casper/src/main/resources/Stdin.rho"),
        ),
        (
            "Stdout.rho",
            include_str!("../../casper/src/main/resources/Stdout.rho"),
        ),
    ];
    for (lib_name, lib_src) in sibling_libs {
        // Only inspect the sibling lib's own outer-new clause; a
        // stray occurrence inside a method body (say a String literal
        // that happens to contain "foldPartials") is fine.
        let lib_in_idx = lib_src
            .find(" in {")
            .unwrap_or_else(|| panic!("{lib_name} must have an outer `new ... in {{`"));
        let lib_new_clause = &lib_src[..lib_in_idx];
        for name in INNER_NEW_NAMES {
            // Word-boundary match: look for the name preceded and
            // followed by non-identifier chars.
            let has = lib_new_clause
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|tok| tok == *name);
            assert!(
                !has,
                "RH-A5-9 regression: Stream.rho's inner-new name \
                 `{name}` appears in `{lib_name}`'s outer-new clause \
                 — a cross-lib reference has been introduced against \
                 an inner-scope name, and `lib_body` composition will \
                 bind it to a fresh unforgeable (silent hang on first \
                 `<<-` peek).  Either promote `{name}` to Stream.rho's \
                 OUTER new clause (so it's shared at the composer's \
                 outer scope) or remove the cross-lib reference from \
                 `{lib_name}`."
            );
        }
    }
}

/// M-26 (2026-09-04, RH-A5-7) — pin arity-1 stream methods to
/// `wait:false` in their `fsLockSequential!` invocations.
///
/// Arity-1 methods (`bytes`, `chars`, `readLine`, `lines`,
/// `writeChars`, `writeLine`) execute `for (@state <- stateP) {
/// ... fsLockSequential(...); for (@lockReply <- lockCh) { ... }}` —
/// stateP is held during the lock-acquire wait.  Today safe because
/// every arity-1 method passes `wait:false` so the native returns
/// quickly.  A future maintainer who accepts `wait:true` at arity-1
/// (mimicking arity-2) would deadlock — a concurrent `close()`
/// waiting for stateP couldn't proceed while the current call is
/// parked in the lock waiter queue.
///
/// This test source-scans `File.rho` for every `fsLockSequential!`
/// invocation and asserts the wait arg is the literal `false`.  Any
/// call with `wait:true` fires this pin and forces the author to
/// document the deadlock analysis (via arity-2 early-release
/// migration) before the change lands.
#[test]
fn arity_one_stream_methods_pin_wait_false_on_fs_lock_sequential() {
    let src = include_str!("../../casper/src/main/resources/File.rho");
    // The arity-5 native call shape is
    // `fsLockSequential!(fd, *this, cmode, <wait>, *lockCh)` — locate
    // every such invocation, extract the 4th argument, and assert it
    // is `false`.  Callers accepting wait:true (the arity-2 options-
    // map methods) invoke `fsLockSequential!` INDIRECTLY via the
    // `withSequentialLock` / `acquireSequentialForStream` helpers,
    // which is precisely why those helpers exist — that pattern is
    // exempt from this pin.
    let calls: Vec<&str> = src
        .match_indices("fsLockSequential!(")
        .map(|(idx, _)| {
            let after = &src[idx..];
            let end = after
                .find(')')
                .expect("fsLockSequential! call must have closing paren");
            &after[..=end]
        })
        .collect();
    assert!(
        !calls.is_empty(),
        "File.rho must have at least one direct fsLockSequential! call"
    );
    for call in &calls {
        // The 4th positional arg is the wait flag.  Split on comma,
        // trim, and assert the token is `false`.
        let args_start = call.find('(').expect("call must have `(`") + 1;
        let args_end = call.rfind(')').expect("call must have `)`");
        let args_str = &call[args_start..args_end];
        let tokens: Vec<&str> = args_str.split(',').map(str::trim).collect();
        assert!(
            tokens.len() >= 5,
            "fsLockSequential! call must have >=5 positional args \
             (fd, holder, cmode, wait, ack); got {tokens:?}"
        );
        let wait = tokens[3];
        // Skip variable-passthrough patterns like the helper
        // contracts (`withSequentialLock`, `acquireSequentialForStream`)
        // which take `wait` as a parameter and forward it — those are
        // called by both arity-1 (with the literal false) and arity-2
        // (with the caller's requested wait) so the discipline lives
        // at the CALLER of the helper, not inside the helper itself.
        // Literal `true` is always a violation; literal `false` is
        // always fine; identifiers (variables) get skipped.
        if wait == "true" {
            panic!(
                "RH-A5-7 regression: fsLockSequential! call with \
                 wait:true literal.  arity-1 stream methods must use \
                 wait:false to avoid the stateP-held-during-wait \
                 deadlock hazard; a wait:true invocation must migrate \
                 to the arity-2 early-release + withSequentialLock \
                 helper pattern.  Offending call: {call}"
            );
        } else if wait != "false" && !wait.chars().all(|c| c.is_ascii_alphabetic() || c == '_') {
            panic!(
                "RH-A5-7 regression: fsLockSequential! 4th arg is \
                 neither `false` (arity-1 direct call convention) nor \
                 an identifier passthrough (helper contract): {wait:?}.  \
                 Offending call: {call}"
            );
        }
    }
}

/// M-24 (2026-09-04, RH-A5-5) — pin `Stream.rho::method chunk(@n)`
/// to `<<-` peek on the state cell, and pin the header
/// §Concurrency docstring to admit chunk is the exception.
///
/// The blanket claim at Stream.rho:82-86 (`every named method
/// acquires the state token via linear-receive`) is contradicted by
/// `chunk`'s use of `<<-` (peek) at the entry check.  Peek is
/// correct — `chunk` is a peek-then-delegate to `next()`, which
/// linearly consumes stateP internally — but the docstring drifted.
/// Fix per review recommendation was to convert docstring OR add a
/// test; this pin does the latter, ensuring:
///   1. `chunk`'s body uses `<<-` on stateP (proving the exception
///      is real).
///   2. The header concurrency docstring mentions `chunk` as the
///      peek-delegate exception (proving the exception is
///      documented near where a maintainer would look).
///
/// A regression that converts `chunk` to `<-` (linear consume)
/// silently strengthens serialization but breaks the docstring
/// promise about parallel chunk callers.  A regression that
/// removes the chunk-exception note fires the second assertion.
#[test]
fn stream_chunk_uses_peek_on_state_p() {
    let src = include_str!("../../casper/src/main/resources/Stream.rho");
    let start = src
        .find("method chunk(@n) {")
        .expect("Stream.rho must define method chunk(@n)");
    let after = &src[start..];
    let end = after[1..]
        .find("method ")
        .map(|i| i + 1)
        .unwrap_or(after.len());
    let body = &after[..end];
    assert!(
        body.contains("<<- @[*private, *stateP]"),
        "RH-A5-5 regression: Stream.rho::chunk(@n) must peek stateP \
         via `<<-`, not linearly consume it.  A silent conversion to \
         `<-` (linear consume) would break the concurrency contract \
         allowing parallel chunk callers."
    );
    // Header docstring must warn maintainers about the peek exception.
    let header_end = src.find("agent Stream {").expect("agent block start");
    let header = &src[..header_end];
    assert!(
        header.contains("chunk") && header.contains("peek"),
        "RH-A5-5 regression: Stream.rho's header must mention that \
         `chunk` is the peek-delegate exception to the linear-receive \
         concurrency discipline.  A silent removal of the exception \
         note leaves the header docstring inconsistent with the code."
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
