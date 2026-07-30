//! ★★★ **The build-side depth bounds — ONE definition, shared by the two test binaries
//! that have anything to say about them.**
//!
//! # Why this file exists (#157)
//!
//! `rholang/tests/deploy_depth_ceiling.rs` MEASURES these ceilings and deliberately
//! declines to assert either number:
//!
//! > *"Pinning either number would pin a constant that a legitimate improvement must then
//! > delete."*
//!
//! `rholang/tests/stack_depth_gate.rs` needs them anyway, to state the inventory's thesis —
//! *the wire is the binding constraint, and every build-side path clears it with headroom.*
//! It used to meet that need by TRANSCRIBING the measured values, which drifted exactly as
//! the source predicted: `env_get_deploy` was carried as 283 and the tree later measured 274.
//!
//! ⚠★★ **And the first repair was only half a repair.** Turning the rows into LOWER BOUNDS
//! removed the drifting value, but left nothing comparing a bound against a measurement — so
//! a REGRESSION that dropped the true ceiling below its bound would pass unnoticed. A bound
//! nothing checks is a comment.
//!
//! ## The shape, and why it is not a file channel
//!
//! The obvious design — the measuring test publishes its readings, the gate parses them — was
//! rejected. It needs a file under `target/`, it makes one test's verdict depend on whether
//! another test ran, and its failure mode when the file is absent is a SILENT SKIP: the gate
//! goes green having checked nothing, which is the class of defect this whole campaign exists
//! to close.
//!
//! ⇒ **Move the BOUND to the measurement, rather than the measurement to the bound.** The
//! bounds live here, once; `deploy_depth_ceiling.rs` includes this module and asserts each
//! measured ceiling clears its own bound at the point of measurement; `stack_depth_gate.rs`
//! includes the same module for the headroom thesis. One number, one place, checked where the
//! evidence is — and no order dependence between test binaries.
//!
//! ## The bounds are BOUNDS, and that is load-bearing
//!
//! A lower bound is monotone in the right direction: a legitimate improvement raises the true
//! ceiling and the bound stays true with no edit — which is what the measuring test asked for
//! when it refused to pin a value — while a regression that drops the true ceiling below the
//! bound is exactly what should fire.
//!
//! ⚠ Included via `#[path]` by both binaries, so some items are unused in each; that is what
//! `#![allow(dead_code)]` below is for, and it is scoped to this module alone.

#![allow(dead_code)]

/// ⚠★★ **`Bisected` IS DELETED (#157), and the reason is that this file used it to pin a
/// value its own source explicitly refuses to pin.**
///
/// `rholang/tests/deploy_depth_ceiling.rs` measures these ceilings and then declines to
/// assert either number, because *"pinning either number would pin a constant that a
/// legitimate improvement must then delete"* — it asserts the ORDERING instead. This
/// inventory transcribed `env_get_deploy` as a bisected **283** anyway, and the tree
/// subsequently measured **274**. The drift was the predicted consequence of the
/// transcription, not a surprise.
///
/// ★★★ **And re-transcribing 274 would have repaired nothing, which is the part worth
/// keeping.** The only consumer is
/// [`the_read_ceiling_binds_and_the_build_side_clears_it`], whose check is
/// `floor(depth / widest_read) >= ACKNOWLEDGED_HEADROOM` with `widest_read = 33` — so every
/// depth in `[264, 297)` produces the identical verdict. **The check's resolution is 33
/// levels against a drift of 9.** A number the gate cannot distinguish from its neighbours
/// is not being checked; it is being carried.
///
/// ⇒ The disposition is a **LOWER BOUND**, always, for every row. That is what the claim
/// actually needs (*"every build-side path clears the wire with headroom"*), it is what the
/// source is willing to stand behind, and it is monotone in the right direction: a
/// legitimate improvement only raises the true ceiling, so the bound stays true and needs no
/// edit, while a regression that drops the true ceiling below the bound is exactly what
/// should fire.
#[derive(Clone, Copy, Debug)]
pub enum BuildCeiling {
    /// Measured to reach at least this depth. ★ A **bound**, never an equality — see the
    /// type's own doc for why no row may claim a bisected value.
    AtLeast(usize),
}

impl BuildCeiling {
    /// The lower bound, in nesting levels.
    pub fn depth(self) -> usize {
        match self {
            BuildCeiling::AtLeast(d) => d,
        }
    }
}

/// ★★ **The acknowledged inventory of build-side depth ceilings.**
///
/// **Why this is an inventory and not a threshold.** Every row already clears
/// the read ceiling — by 8.6× at the tightest and by four orders of magnitude
/// at the loosest. A test that asserted "no build path exceeds what the wire can
/// carry" would be RED on arrival for every row, and a gate that is red on
/// arrival gets muted. So the claim asserted is the one that is actually true
/// and actually worth defending:
///
/// > **The wire is the binding constraint, and every build-side path clears it
/// > with headroom.**
///
/// That claim fails in exactly the two ways that matter — a NEW build path
/// joins without being classified (caught by
/// [`the_build_side_inventory_is_complete`]), or an existing path's headroom
/// ratio degrades (caught by [`the_read_ceiling_binds_and_the_build_side_clears_it`]).
///
/// Rows are `(path, ceiling, where it was measured)`. None is re-measured here:
/// re-bisecting an end-to-end deploy ceiling is
/// `rholang/tests/deploy_depth_ceiling.rs`'s whole job, and a second copy of
/// that measurement would be a second number to drift.
pub const BUILD_DEPTH_INVENTORY: &[(&str, BuildCeiling, &str)] = &[
    // ⭑⭑ **IT WAS THE BINDING ONE, AND THE NEW CHECK'S FIRST RUN FOUND THAT IT NO LONGER
    // IS.** Measured through `deploy_depth_ceiling.rs` at the production 2 MiB worker:
    //
    //     deploy depth ceiling: plain_deploy 6831, env_get_deploy 6831
    //     plain_deploy   : depth 6831 runs, depth 6832 exits 134 (SIGABRT)
    //     env_get_deploy : depth 6831 runs, depth 6832 exits 134 (SIGABRT)
    //
    // **The two shapes are now EQUAL, against a recorded 283 (then 274) — a 24× rise.**
    // This row was the binding one precisely because `Env::get` returns its bound value
    // through a Θ(depth) `<Par as Clone>::clone` (`substitute_deep_binding_body`), described
    // there as un-removable because *the copy IS the meaning of substitution*. That remains
    // true of the SEMANTICS and is now false of the COST: Stage F-4 converted the `Clone`
    // impl, so the extra traversal this shape contributes over `plain_deploy` has gone to
    // zero and the ceiling is set by the shared build/release path instead.
    //
    // ⚠ The superseded framing is kept rather than overwritten: *"THE BINDING ONE"* was
    // accurate when written and is what a reader will find quoted elsewhere.
    //
    // ⚠★★ **This row is #157, and its bound is the BAND FLOOR rather than a reading.**
    // It carried `Bisected(283)`; the tree later measured 274. Both sit inside the only
    // band the consumer can resolve — `floor(d / 33) >= 8` admits every depth in
    // `[264, 297)` — so neither number was ever being checked, and re-transcribing 274
    // would have changed nothing except the date on the drift.
    //
    // 264 is `widest_read * ACKNOWLEDGED_HEADROOM`: the weakest claim that still carries
    // the inventory's thesis. It needs no edit when the true ceiling moves within the
    // band, which is what its own source asked for when it declined to pin a value —
    // *"pinning either number would pin a constant that a legitimate improvement must
    // then delete."*
    //
    // ★ It stays at 264 rather than being raised to the measured 6,831, and that is the
    // point of a bound rather than a reading: raising it to today's number would re-pin
    // exactly the kind of value #157 is about, and hand the next legitimate improvement an
    // edit to make. What tracks the RELATIONSHIP between the two shapes is
    // `deploy_depth_ceiling.rs`'s ordering assertion (`env_get <= plain`); what 264 buys is
    // a floor under the headroom thesis, and 6,831 clears it by 25.9×.
    (
        "env_get_deploy",
        BuildCeiling::AtLeast(264),
        "rholang/tests/deploy_depth_ceiling.rs (bound, NOT a transcribed reading — see #157)",
    ),
    (
        "plain_deploy",
        BuildCeiling::AtLeast(6_831),
        "rholang/tests/deploy_depth_ceiling.rs",
    ),
    // `inj_attempt`'s `set-initial-cost` phase, after `into_source_process`
    // removed the `<Par as Clone>` copy: 0 B/level, flat 32,768 B from depth 4
    // to 4,096.
    (
        "inj_attempt set-initial-cost",
        BuildCeiling::AtLeast(1_048_576),
        "this file, `inj_attempt_clone_body`",
    ),
    // Source-text ingress. `normalize` is CONVERTED (43,542 → 0 B/level debug,
    // 7,261 → 0 release), so it has no stack slope; the figure is the deepest
    // source a 2 MiB worker was observed to normalize, not a bisected wall.
    (
        "normalize (source-text ingress, pre-metering)",
        BuildCeiling::AtLeast(39_960),
        "rholang/tests/stack_depth_probe.rs; audit evidence row E63",
    ),
];

/// The acknowledged headroom floor: the tightest build-side ceiling divided by
/// the loosest read ceiling, rounded down. Recorded at
/// $`283 / 33 = 8.57`$, so the floor is **8**.
///
/// ⚠ This is a **tripwire, not a pass** — the same standing this file gives
/// [`assert_slope_below`]. It certifies only that the relationship has not got
/// *worse*; it does not certify that 283 is a comfortable number.
///
/// ⚠★ **The 283 in that derivation is the stale value #157 is about** — the tree has since
/// measured **274** — and the constant is nonetheless correct and stays: $`274 / 33 = 8.3`$,
/// which still floors to **8**. That is not luck. It is the same resolution argument that
/// makes the transcription pointless in the first place: the check cannot distinguish any
/// depth in $`[264, 297)`$, and both values sit inside it. The derivation is left reading 283
/// rather than silently updated to 274, because the number it was taken from is part of the
/// record and **annotating beats overwriting**.
///
/// ★ The ratchet's DIRECTION is what must be preserved: it may only ever go **UP**, as
/// `UNMEASURED_TRAVERSALS` may only go down. Raising it claims the relationship improved and
/// must arrive with the measurement that shows it; lowering it is accepting a regression, and
/// there is no form of words that makes that a repair.
pub const ACKNOWLEDGED_HEADROOM: usize = 8;

/// The bound recorded for `subject`, if this inventory carries one.
///
/// ★ Used by `deploy_depth_ceiling.rs` to check its own measurement against the bound the
/// gate relies on — the check that closes #157's remaining half.
pub fn bound_for(subject: &str) -> Option<usize> {
    BUILD_DEPTH_INVENTORY
        .iter()
        .find(|(path, _, _)| *path == subject)
        .map(|(_, ceiling, _)| ceiling.depth())
}

/// Assert that a MEASURED ceiling clears the bound this inventory publishes for it.
///
/// ⚠ A subject with no bound is not silently accepted: it is named, because an unbounded
/// build path is precisely what `the_build_side_inventory_is_complete` exists to catch and a
/// second silent skip here would defeat it.
pub fn assert_measured_clears_bound(subject: &str, measured: usize) {
    let Some(bound) = bound_for(subject) else {
        panic!(
            "`{subject}` was measured to depth {measured} but BUILD_DEPTH_INVENTORY carries no \
             bound for it. Add a row — an unbounded build path is what the inventory exists to \
             make impossible."
        );
    };
    assert!(
        measured >= bound,
        "BUILD-SIDE REGRESSION: `{subject}` now reaches only depth {measured}, below the \
         {bound} this inventory publishes as its lower bound.\n\
         \n\
         The bound is what `stack_depth_gate.rs`'s headroom thesis rests on — that the wire is \
         the binding constraint and every build-side path clears it. If the true ceiling has \
         fallen, that thesis is no longer supported by this path.\n\
         \n\
         ⚠ Lowering the bound is ACCEPTING the regression. It is not a repair, and the bound \
         may only ever move UP."
    );
}
