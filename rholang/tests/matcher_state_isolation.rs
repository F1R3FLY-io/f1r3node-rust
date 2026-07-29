//! Every attempt the maximum-bipartite matcher makes must return **its own
//! delta**, and the backstop that says so must be able to fire.
//!
//! # The two defects this file pins, and why they are ONE change
//!
//! ## Defect A — `match_function` never restores state (`isolateState` missing)
//!
//! Scala's `matchFunction` wraps every attempt in `isolateState`
//! (`SpatialMatcher.scala:279-287`):
//!
//! ```text
//!   initState   <- get          // snapshot
//!   _           <- f            // run the attempt
//!   resultState <- get          // the post-attempt map …
//!   _           <- set(initState)   // … is the VALUE; the caller's map is put back
//!   yield resultState
//! ```
//!
//! In `StateT`/`StreamT` a failed attempt is an *empty stream*, so its state
//! never reaches the caller at all; the explicit `set(initState)` is only
//! needed on the success path.
//!
//! The Rust port mutates a plain `free_map` field, so failure is
//! **destructive**. `list_match` builds `cloned_self` once, moves it into the
//! `match_function` closure, and every attempt in the whole bipartite search
//! mutates that one map. Each claimed match's snapshot is therefore
//! **cumulative** — it carries the bindings written by every *failed* attempt
//! that ran before it.
//!
//! ## Defect B — the multiplicity guard was a tautology (#144)
//!
//! `aggregate_updates` is the fold that merges those snapshots. Scala guards it:
//!
//! ```text
//!   addedVars = freeMaps.flatMap(_.keys.filterNot(currentVars.contains))  // a Seq
//!   ensure(addedVars.size == addedVars.distinct.size)
//! ```
//!
//! `addedVars` is a `Seq`, so **multiplicity is kept** and the check is real.
//! The Rust port collected into a `HashSet` first and then compared that set's
//! length against `set.iter().collect::<HashSet<_>>().len()` — identical *by
//! construction*. The guard could not fire.
//!
//! ## ★ Why they are one change
//!
//! The vacuous guard is **load-bearing**. It is the only reason the `?` on
//! `aggregate_updates` in `list_match` never short-circuits, and therefore the
//! only reason the cumulative snapshots never changed a match *verdict*. Repair
//! the guard alone and the duplicates it can now see are the *artifact of
//! defect A* — eight of the forty-nine tests in `tests/matcher/match_test.rs`
//! turn red. Restore isolation alone and the guard stays blind. Only the
//! composite is correct.
//!
//! ```text
//!            ┌─────────────── ONE MBM SEARCH, ONE `cloned_self` ───────────────┐
//!            │                                                                 │
//!   attempt 1│  P_L vs c1   →  free_map {L↦v1}         ── claimed, snapshot S₁ │
//!   attempt 2│  P_L vs c2   →  free_map {L↦v2}   FAILS ── nothing restores it   │
//!   attempt 3│  P_M vs c3   →  free_map {L↦v2, M↦w}    ── claimed, snapshot S₂ │
//!            └─────────────────────────────────────────────────────────────────┘
//!                                        │
//!                 aggregate_updates folds S₁ then S₂ in `matches` BTreeMap
//!                 order — structural `Par` order, i.e. prost's field-declaration
//!                 derive. Whether v1 or v2 survives is decided by a `.proto`
//!                 field number.
//! ```
//!
//! # What is asserted here
//!
//! | test | pins | HEAD | composite |
//! |---|---|---|---|
//! | `a_second_attempt_returns_its_own_delta_not_a_cumulative_snapshot` | defect A | red | green |
//! | `control_a_non_binding_attempt_is_unchanged_by_the_fix` | the arms that provably do NOT write | green | green |
//! | `aggregate_updates_names_the_level_two_attempts_both_claimed` | defect B | red | green |
//! | `aggregate_updates_refuses_a_repeat_even_when_the_two_values_agree` | defect B is about KEYS, not disagreeing values | red | green |
//! | `control_a_level_already_in_the_current_map_is_not_an_added_var` | `filterNot(currentVars.contains)` is load-bearing | green | green |
//!
//! The two controls are what make the two REDs evidence rather than noise: they
//! are green on **both** sides, so a RED flipping green cannot be explained by
//! "`match_function` was rewritten" or "`aggregate_updates` was rewritten".
//!
//! ★ **No test here expects a panic.** `aggregate_updates` used to answer a
//! duplicate with `panic!`; it now refuses through the `Option` channel it
//! already had, and every assertion below is on a returned value
//! (`shared/tests/panic_expectation_gate.rs`).

use models::rhoapi::Par;
use models::rust::utils::{new_free_map, new_freevar_par, new_gint_par, FreeMap};
use rholang::rust::interpreter::matcher::list_match::{
    aggregate_updates, first_duplicate_added_var, ListMatch, Pattern,
};
use rholang::rust::interpreter::matcher::spatial_matcher::SpatialMatcherContext;

// ─────────────────────────────────────────────────────────────────────────
// Fixtures
//
// Two distinct ground terms and two distinct free-variable levels. Distinctness
// on BOTH axes is essential: with one level reused, a cumulative map and an
// isolated one would have the same `len()` and the RED would not discriminate.
// ─────────────────────────────────────────────────────────────────────────

/// The ground target `7`. `connective_used == false`, `locally_free` empty.
fn seven() -> Par { new_gint_par(7, Vec::new(), false) }

/// The ground target `8`, structurally distinct from [`seven`].
fn eight() -> Par { new_gint_par(8, Vec::new(), false) }

/// The pattern `free_var(level)`. `connective_used == true`, so
/// `match_function` routes it to the connective arm — the arm that binds, and
/// the only arm that snapshots.
fn free_var(level: i32) -> Pattern<Par> { Pattern::Term(new_freevar_par(level, Vec::new())) }

// ─────────────────────────────────────────────────────────────────────────
// §1 — RED 1: `match_function` is state-isolated
// ─────────────────────────────────────────────────────────────────────────

/// A second attempt must return the bindings **it** made, not those plus every
/// binding left behind by the attempts before it.
///
/// This is `isolateState` stated as an observation: after the call, the map the
/// caller handed in is the map the caller still has, and the post-attempt map
/// came back as the *value*.
///
/// At HEAD this fails three ways over: `second` contains level 0 as well as
/// level 1, its length is 2 rather than 1, and `ctx.free_map` has been mutated
/// out from under the caller.
#[test]
fn a_second_attempt_returns_its_own_delta_not_a_cumulative_snapshot() {
    let mut ctx = SpatialMatcherContext::new();

    let first = ctx
        .match_function(free_var(0), seven())
        .expect("a free variable binds a ground target");

    // ★ MUTATION APPLIED. Without this pair the assertions below would be
    // vacuously true for an empty map — the arm under test really did bind.
    assert_eq!(
        first.get(&0),
        Some(&seven()),
        "attempt 1 must bind level 0 to its target"
    );
    assert_eq!(first.len(), 1, "attempt 1's delta is exactly one binding");

    // ★ THE MUTATION UNDER TEST: a second attempt, at a DIFFERENT level. The
    // bipartite matcher performs repeated attempts routinely — there is no
    // memoization between them (`list_match`'s "Bypassing memoizeInHashMap").
    let second = ctx
        .match_function(free_var(1), eight())
        .expect("a free variable binds a ground target");

    assert_eq!(
        second.get(&1),
        Some(&eight()),
        "attempt 2 must report its own binding"
    );
    assert_eq!(
        second.get(&0),
        None,
        "★ THE CUMULATIVE SIGNATURE: attempt 2 must not carry attempt 1's binding"
    );
    assert_eq!(second.len(), 1, "attempt 2's delta is exactly one binding");

    // ★ THE PROPERTY ITSELF (Scala `isolateState`): the delta is the RESULT,
    // never a side effect on the caller's state.
    assert_eq!(
        ctx.free_map,
        new_free_map(),
        "the caller's free_map must be exactly as it was handed in"
    );
}

/// A **failed** attempt must leave nothing behind either — the destructive half
/// of the divergence, and the half `StreamT`'s empty stream hid in Scala.
///
/// The witness is a pair pattern, because `SpatialMatcher<(Par, Par), (Par, Par)>`
/// is exactly `spatial_match(t.0, p.0).and_then(|_| spatial_match(t.1, p.1))`:
/// the first component binds, and only then does the second component refuse.
/// `list_match!` is instantiated for `(Par, Par)`, so `match_function` is the
/// unit under test here just as it is in §1, and `connective_used((p0, p1))` is
/// `connective_used(p0) || connective_used(p1)` — the free variable in the
/// first component routes the pair to the arm that can write.
///
/// This is the shape that actually corrupts: `list_match` builds `cloned_self`
/// once and every attempt mutates it, so a half-finished failed attempt's
/// bindings are still there when the *next* attempt is claimed and snapshotted.
#[test]
fn a_failed_attempt_leaves_no_bindings_behind() {
    // ★ MUTATION APPLIED. The same pattern against a target it *does* match
    // proves the bind really happens — without this the refusal below could be
    // failing before it ever wrote anything, and the assertion would be vacuous.
    let mut proof = SpatialMatcherContext::new();
    assert_eq!(
        proof.match_function(
            Pattern::Term((new_freevar_par(3, Vec::new()), eight())),
            (seven(), eight())
        ),
        Some(FreeMap::from([(3, seven())])),
        "the first component binds level 3 when the second component agrees"
    );

    // Same pattern, second component now refuses — after the bind.
    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(
        ctx.match_function(
            Pattern::Term((new_freevar_par(3, Vec::new()), eight())),
            (seven(), seven())
        ),
        None,
        "the second component refuses, so the pair does not match"
    );
    assert_eq!(
        ctx.free_map,
        new_free_map(),
        "★ a FAILED attempt must not leave its partial bindings in the caller's map"
    );
}

/// Control for §1 — must **not** discriminate.
///
/// Both of these arms are provably incapable of touching `free_map`:
/// `guard(t == p)` is a pure comparison and `guard(locally_free(t, 0).is_empty())`
/// is a pure query. That is exactly why the entry snapshot is taken on the
/// connective arm only, and this control is the assertion that the other arms
/// were left alone.
///
/// Green at HEAD **and** after the fix.
#[test]
fn control_a_non_binding_attempt_is_unchanged_by_the_fix() {
    let mut ctx = SpatialMatcherContext::new();

    // Remainder arm: `guard(self.locally_free(t, 0).is_empty())`.
    assert_eq!(
        ctx.match_function(Pattern::<Par>::Remainder(0), seven()),
        Some(new_free_map()),
        "a concrete target satisfies a remainder and binds nothing here"
    );

    // Non-connective Term arm, matching: `guard(t == p)`.
    assert_eq!(
        ctx.match_function(Pattern::Term(seven()), seven()),
        Some(new_free_map()),
        "equal concrete terms match and bind nothing"
    );

    // Non-connective Term arm, refusing.
    assert_eq!(
        ctx.match_function(Pattern::Term(seven()), eight()),
        None,
        "unequal concrete terms do not match"
    );

    assert_eq!(
        ctx.free_map,
        new_free_map(),
        "none of the three touched the caller's free_map"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// §2 — RED 2: the multiplicity guard is live
// ─────────────────────────────────────────────────────────────────────────

/// Two aggregated maps that both claim the same level must be refused, and the
/// refusal must be able to **name** the level.
///
/// The fixture is the measured signature: an instrumented probe over the 49-test
/// matcher suite found aggregations whose `added_vars` sequence read `[0, 1, 0, 1]`
/// — two maps, each carrying levels 0 and 1, which is precisely what a pair of
/// cumulative snapshots looks like. Pinning it as a fixture makes the property
/// permanently checkable in CI without a diagnostic build.
///
/// At HEAD `aggregate_updates` returns `Some({0↦7, 1↦8})` and the test fails by
/// assertion — cleanly, with no panic.
#[test]
fn aggregate_updates_names_the_level_two_attempts_both_claimed() {
    let current = new_free_map();
    let a = FreeMap::from([(0, seven()), (1, eight())]);
    let b = FreeMap::from([(0, seven()), (1, eight())]);

    assert_eq!(
        first_duplicate_added_var(&current, &[a.clone(), b.clone()]),
        Some(0),
        "the refusal names a LEVEL, not merely a boolean"
    );
    assert_eq!(
        aggregate_updates(current, vec![a, b]),
        None,
        "the aggregation refuses through its own Option channel"
    );
}

/// ★ THE DISCRIMINATOR between a faithful port and a plausible-looking one.
///
/// Scala checks the multiplicity of **keys**, not disagreement of values. A
/// "fix" that compares the two bound `Par`s and only refuses when they differ
/// passes the test above — the values there are equal in one map and different
/// in the other, so it is not decisive on its own — and fails this one, where
/// the two maps are identical.
///
/// Refusing here is correct on the merits, not just for fidelity: two matches
/// that both claim level 5 mean the pattern used level 5 as a binder twice,
/// which is a linearity violation regardless of what the two matches bound.
#[test]
fn aggregate_updates_refuses_a_repeat_even_when_the_two_values_agree() {
    let current = new_free_map();
    let a = FreeMap::from([(5, seven())]);

    assert_eq!(
        first_duplicate_added_var(&current, &[a.clone(), a.clone()]),
        Some(5),
        "identical maps still duplicate the KEY"
    );
    assert_eq!(
        aggregate_updates(current, vec![a.clone(), a]),
        None,
        "agreement on the value does not make the repeat legal"
    );
}

/// Control for §2 — must **not** discriminate, and it pins
/// `filterNot(currentVars.contains)`.
///
/// This is the control that makes the fix *possible*. Under isolation every map
/// a `match_function` attempt returns is `current + delta`, so the levels
/// already present in `current` appear in **every** aggregated map. Drop the
/// `filterNot` filter and this test goes red immediately: level 5 would read as
/// a duplicate and every ordinary aggregation would be refused.
///
/// Green before and after.
#[test]
fn control_a_level_already_in_the_current_map_is_not_an_added_var() {
    let current = FreeMap::from([(5, seven())]);
    // The shape isolation produces: a shared prefix plus one disjoint delta each.
    let a = FreeMap::from([(5, seven()), (6, eight())]);
    let b = FreeMap::from([(5, seven()), (7, seven())]);

    assert_eq!(
        first_duplicate_added_var(&current, &[a.clone(), b.clone()]),
        None,
        "the shared prefix is not an added var"
    );

    let out = aggregate_updates(current, vec![a, b]).expect("disjoint deltas aggregate");
    assert_eq!(out.len(), 3, "prefix plus both deltas");
    assert_eq!(out.get(&5), Some(&seven()), "the prefix survives");
    assert_eq!(out.get(&6), Some(&eight()), "delta from the first map");
    assert_eq!(out.get(&7), Some(&seven()), "delta from the second map");
}

/// The empty aggregation, and the single-map aggregation, still aggregate.
///
/// A refusal that fired on these would be catastrophic — `list_match` calls
/// `aggregate_updates` on every list match, including the ones with a single
/// claimed match. Green before and after.
#[test]
fn control_the_degenerate_aggregations_are_not_refused() {
    assert_eq!(
        aggregate_updates(new_free_map(), Vec::new()),
        Some(new_free_map()),
        "aggregating nothing yields the current map"
    );

    let current = FreeMap::from([(2, eight())]);
    let only = FreeMap::from([(2, eight()), (3, seven())]);
    assert_eq!(
        aggregate_updates(current, vec![only.clone()]),
        Some(only),
        "a single map cannot duplicate itself"
    );
}
