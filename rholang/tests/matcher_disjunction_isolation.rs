//! A **disjunction branch that fails** must not leave its bindings behind.
//!
//! # The defect
//!
//! `SpatialMatcher<Par, Connective>`'s `ConnOrBody` arm walks the branches with
//! `find_map` and, on each branch, snapshots `free_map`, runs the branch, and
//! restores the snapshot:
//!
//! ```text
//!   let matches = self.free_map.clone();
//!   self.spatial_match(target.clone(), p)?;   // ← ★ returns from the CLOSURE
//!   self.free_map = matches;                  //   … skipping this line
//!   Some(())
//! ```
//!
//! The restore is faithful on **success** — a disjunction cannot bind, because
//! its branches disagree about which variables they bind, so upstream returns
//! the pre-branch state (`SpatialMatcher.scala:377-386`). But the `?` returns
//! `None` *from the closure*, so on **failure** the restore never runs. The
//! next branch is then attempted against a `free_map` that already carries the
//! failed branch's bindings, and if every branch fails those bindings escape to
//! the caller.
//!
//! Upstream cannot express this. Scala's branches are united with
//! `Alternative_[F].unite`, which hands **every branch the same input state**;
//! a failed branch is an empty stream whose state never propagates at all. The
//! explicit `freeMap.set(matches)` is only needed for the success path — which
//! is exactly the half the Rust port kept.
//!
//! ```text
//!    ps = [ B₁ , B₂ ]                      HEAD                    fixed
//!    ────────────────────────────────────────────────────────────────────────
//!    snapshot S = free_map                 S = {}                  S = {}
//!    B₁ binds x, then refuses              free_map = {x↦v}        free_map = {x↦v}
//!    restore                               ✗ SKIPPED by `?`        ✓ free_map = S
//!    snapshot for B₂                       S′ = {x↦v}  ← polluted  S′ = {}
//!    B₂ succeeds
//!    restore                               free_map = {x↦v}        free_map = {}
//! ```
//!
//! # Relationship to task #144
//!
//! Same invariant — an attempt owns its own state — and the same repair shape,
//! but a **distinct defect** at a distinct site, so it ships as its own commit
//! with its own RED. #144 restores per-attempt isolation in
//! `list_match::match_function`; this restores per-branch isolation in the
//! disjunction matcher.
//!
//! ★ No test here expects a panic; every assertion is on a returned value or on
//! the observable `free_map` (`shared/tests/panic_expectation_gate.rs`).

use models::rhoapi::connective::ConnectiveInstance::*;
use models::rhoapi::{Connective, ConnectiveBody, Par};
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{new_free_map, new_freevar_par, new_gint_par};
use rholang::rust::interpreter::matcher::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
use rholang::rust::interpreter::util::prepend_connective;

// ─────────────────────────────────────────────────────────────────────────
// Fixtures
// ─────────────────────────────────────────────────────────────────────────

fn seven() -> Par { new_gint_par(7, Vec::new(), false) }

fn eight() -> Par { new_gint_par(8, Vec::new(), false) }

/// The branch `free_var(0) /\ 8`.
///
/// Against the target `7` the conjunction binds level 0 from the target and
/// *then* demands the target be `8`, so it writes before it refuses. That
/// ordering is what makes it a witness: a branch that refused before writing
/// anything would leave nothing to leak and the test would be vacuous.
fn binds_then_refuses() -> Par {
    prepend_connective(
        vector_par(Vec::new(), true),
        Connective {
            connective_instance: Some(ConnAndBody(ConnectiveBody {
                ps: vec![new_freevar_par(0, Vec::new()), eight()],
            })),
        },
        0,
    )
}

/// `branch₁ \/ branch₂` as a bare `Connective`, so the disjunction arm is the
/// unit under test with no `Par`/`Par` bookkeeping interposed.
fn disjunction(branches: Vec<Par>) -> Connective {
    Connective {
        connective_instance: Some(ConnOrBody(ConnectiveBody { ps: branches })),
    }
}

// ─────────────────────────────────────────────────────────────────────────
// The RED
// ─────────────────────────────────────────────────────────────────────────

/// ★ THE RED. A disjunction that succeeds through its *second* branch must not
/// carry the bindings its *first* branch wrote before refusing.
///
/// The first assertion is the mutation-applied evidence: branch 1 really is
/// attempted and really does refuse, which is why branch 2 has to be reached at
/// all. The second is the property.
#[test]
fn a_failed_disjunction_branch_does_not_leak_its_bindings() {
    // ★ MUTATION APPLIED: branch 1, on its own, refuses.
    let mut probe = SpatialMatcherContext::new();
    assert_eq!(
        probe.spatial_match(seven(), binds_then_refuses()),
        None,
        "`free_var(0) /\\ 8` cannot match the target 7"
    );

    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(
        ctx.spatial_match(seven(), disjunction(vec![binds_then_refuses(), seven()])),
        Some(()),
        "the second branch matches, so the disjunction matches"
    );
    assert_eq!(
        ctx.free_map,
        new_free_map(),
        "★ the refused first branch must not have left its binding behind"
    );
}

/// The same leak observed at the end of the walk: when **every** branch
/// refuses, the disjunction refuses — and it must hand the caller back the
/// `free_map` the caller gave it.
#[test]
fn a_disjunction_that_matches_nothing_leaves_the_caller_s_map_alone() {
    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(
        ctx.spatial_match(
            seven(),
            disjunction(vec![binds_then_refuses(), binds_then_refuses()])
        ),
        None,
        "no branch matches, so the disjunction does not match"
    );
    assert_eq!(
        ctx.free_map,
        new_free_map(),
        "★ a disjunction that matched nothing bound nothing"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Controls — must NOT discriminate
// ─────────────────────────────────────────────────────────────────────────

/// Control: the *verdict* of a disjunction is unchanged by the restore.
///
/// Restoring `free_map` on the failure path cannot change which branch is
/// selected, because no branch's decision reads `free_map` — the arm's answer
/// is a pure function of `(target, branches)`. Green before and after.
#[test]
fn control_the_disjunction_verdict_is_unchanged() {
    let mut ctx = SpatialMatcherContext::new();

    // First branch matches outright.
    assert_eq!(
        ctx.spatial_match(seven(), disjunction(vec![seven(), eight()])),
        Some(()),
        "the first branch matches"
    );

    // Only the second branch matches.
    assert_eq!(
        ctx.spatial_match(seven(), disjunction(vec![eight(), seven()])),
        Some(()),
        "the second branch matches"
    );

    // Neither matches.
    assert_eq!(
        ctx.spatial_match(seven(), disjunction(vec![eight(), eight()])),
        None,
        "no branch matches"
    );

    // The empty disjunction has nothing to match.
    assert_eq!(
        ctx.spatial_match(seven(), disjunction(Vec::new())),
        None,
        "an empty disjunction matches nothing"
    );
}

/// Control: a disjunction that succeeds on a **non-binding** branch binds
/// nothing, at HEAD and after.
///
/// This is the case the existing restore-on-success already covered, and it is
/// here so that the RED above cannot be explained by "the arm was rewritten".
#[test]
fn control_a_successful_non_binding_branch_binds_nothing() {
    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(
        ctx.spatial_match(seven(), disjunction(vec![seven()])),
        Some(()),
        "a concrete branch equal to the target matches"
    );
    assert_eq!(
        ctx.free_map,
        new_free_map(),
        "a disjunction binds nothing, because its branches disagree about what they bind"
    );
}
