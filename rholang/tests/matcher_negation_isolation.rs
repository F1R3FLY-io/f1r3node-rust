//! A negation that **succeeds** must bind nothing — and two sibling
//! sub-patterns that each contain one must not collide in `aggregate_updates`.
//!
//! # The chain, measured at HEAD (`eaa905fe`)
//!
//! ```text
//!   normalizer          ~P and P \/ Q bodies are normalized against a FRESH
//!   (p_negation_        FreeMap — "a negation's bindings do not escape it" —
//!    normalizer.rs:     so a free variable under ~ is FreeVar(0) in a map that
//!    10-13)             combine_p_negation then DISCARDS (:65-89).
//!                                     │
//!                                     ▼
//!   matcher             ConnAndBody binds level 0, then refuses.
//!   (spatial_           ConnNotBody INVERTS the refusal into a success —
//!    matcher.rs:        and there is no restore, so the negation hands its
//!    112-171)           caller a binding it had no right to make.
//!                                     │
//!                                     ▼
//!   list_match          match_function returns that δ = {0 ↦ …} as the
//!   (list_match.rs:     attempt's value (task #144's isolation working
//!    260-299)           exactly as designed — the δ is clean of OTHER
//!                       attempts, but not of its own negation's leak).
//!                                     │
//!                                     ▼
//!   the backstop        Two siblings ⇒ two δ that BOTH carry level 0.
//!   (list_match.rs:     first_duplicate_added_var returns Some(0),
//!    334-343, 381-410)  aggregate_updates returns None, and the whole
//!                       list_match REFUSES.
//! ```
//!
//! ★ **This is why the backstop's "expected to be silent forever" argument does
//! not yet hold.** That argument is *"each level occupies at most one pattern
//! position by linearity"* — and linearity holds only **within one free map**.
//! Fresh-numbering under `~` and `\/` manufactures a second, independent level
//! 0 per sibling, which is exactly the premise the argument needs and does not
//! have. The claim becomes true once the negation restores.
//!
//! # Reachability from ordinary Rholang
//!
//! `fail_on_invalid_connective` (`normalizer/processes/utils.rs`) bans `~` and
//! `\/` in `for`/`contract` patterns, but **only at `bound_map_chain.depth() ==
//! 0`**, and it is never called for `match` case patterns. So:
//!
//! | construct | binding-bodied `~` / `\/` | reaches the matcher? |
//! |---|---|---|
//! | `match` case pattern | accepted, unchecked | **yes** |
//! | `for` / `contract` pattern, depth 0 | `PatternReceiveError` | no |
//! | nested `for` pattern inside a pattern (depth ≥ 1) | accepted | no — `SpatialMatcher<ReceiveBind, ReceiveBind>` compares `patterns` by **equality** |
//!
//! The normalizer neither rejects nor warns on a binding body: the shape is
//! **legal, silently useless, and — until this fix — corrupting**.
//!
//! ★ No test here expects a panic; every assertion is on a returned value or on
//! the observable `free_map`, pinned to a specific level
//! (`shared/tests/panic_expectation_gate.rs`).

use std::collections::HashMap;

use models::rhoapi::connective::ConnectiveInstance::*;
use models::rhoapi::{Connective, ConnectiveBody, Par, Send};
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{new_free_map, new_freevar_par, new_gint_par, new_send};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::matcher::list_match::ListMatch;
use rholang::rust::interpreter::matcher::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
use rholang::rust::interpreter::util::prepend_connective;

// ─────────────────────────────────────────────────────────────────────────
// Fixtures
// ─────────────────────────────────────────────────────────────────────────

fn gint(n: i64) -> Par { new_gint_par(n, Vec::new(), false) }

/// `free_var(0) /\ n`.
///
/// Against a target `t != n` the conjunction binds level 0 to `t` and *then*
/// demands `t == n`, so it writes before it refuses. That ordering is what
/// makes every test below non-vacuous: a conjunction that refused before
/// writing would leave nothing to leak.
///
/// Level 0 in **both** siblings is not a contrivance — it is what the
/// normalizer emits, because each `~` body is numbered from 0 in its own fresh
/// `FreeMap`. `normalizer_emits_level_zero_in_every_negation_body` pins that.
fn binds_then_refuses(n: i64) -> Par {
    prepend_connective(
        vector_par(Vec::new(), true),
        Connective {
            connective_instance: Some(ConnAndBody(ConnectiveBody {
                ps: vec![new_freevar_par(0, Vec::new()), gint(n)],
            })),
        },
        0,
    )
}

/// `~body` as a Par-level pattern. Its `sub_pars` remainder bound is `(0, 0)`
/// (`par_count.rs::min_max_con` gives `ConnNotBody` the widest individual
/// bounds and this pattern has no other components), so the negation is offered
/// the whole target and the enclosing `Par`/`Par` match has nothing left over.
fn neg_par(body: Par) -> Par {
    prepend_connective(
        vector_par(Vec::new(), true),
        Connective {
            connective_instance: Some(ConnNotBody(body)),
        },
        0,
    )
}

fn send(chan: Par, data: Vec<Par>, connective_used: bool) -> Send {
    new_send(chan, data, false, Vec::new(), connective_used)
}

fn par_of_sends(sends: Vec<Send>, connective_used: bool) -> Par {
    vector_par(Vec::new(), connective_used).with_sends(sends)
}

// ─────────────────────────────────────────────────────────────────────────
// §1 — the value leak: a negation that SUCCEEDS must bind nothing
// ─────────────────────────────────────────────────────────────────────────

/// ★ RED. `~(x /\ 8)` matches the target `7` — the inner conjunction demands
/// `8`, refuses, and the negation therefore succeeds. It must carry **no**
/// binding out.
///
/// At HEAD the verdict is already correct and only the `free_map` is wrong:
/// `Some(())` **with** `{0 ↦ 7}` — a completed match holding a binding the
/// pattern never legitimately made.
#[test]
fn a_negation_that_succeeds_binds_nothing() {
    // ★ MUTATION APPLIED: the inner conjunction is really attempted and really
    // refuses, which is the only reason the negation succeeds at all.
    let mut probe = SpatialMatcherContext::new();
    assert_eq!(
        probe.spatial_match(gint(7), binds_then_refuses(8)),
        None,
        "`free_var(0) /\\ 8` cannot match the target 7"
    );

    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(
        ctx.spatial_match(gint(7), neg_par(binds_then_refuses(8))),
        Some(()),
        "the inner conjunction refused, so the negation succeeds"
    );
    assert_eq!(
        ctx.free_map.get(&0),
        None,
        "★ a negation binds nothing — its body's bindings live in a free map \
         the normalizer already discarded"
    );
    assert_eq!(ctx.free_map, new_free_map(), "and nothing else either");
}

/// The same property one layer up, where the leak actually escapes into a
/// caller: `list_match` with a **single** sibling. The verdict is right on both
/// sides; the `free_map` is `{0 ↦ 7}` at HEAD.
#[test]
fn a_single_sibling_negation_leaves_the_callers_map_clean() {
    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(
        ctx.list_match_single(vec![gint(7)], vec![neg_par(binds_then_refuses(8))]),
        Some(()),
        "one sibling: a single delta cannot duplicate itself, so the \
         aggregation cannot refuse — on either side of this fix"
    );
    assert_eq!(ctx.free_map.get(&0), None, "★ the negation bound nothing");
    assert_eq!(ctx.free_map, new_free_map(), "and nothing else either");
}

// ─────────────────────────────────────────────────────────────────────────
// §2 — ★ THE VERDICT: two siblings, and the whole list match is refused
// ─────────────────────────────────────────────────────────────────────────

/// ★★ THE RED THAT MOVES A VERDICT. Two sibling sub-patterns, each a negation
/// whose body binds at level 0, in one `list_match`.
///
/// Both attempts are claimed, both deltas carry level 0,
/// `first_duplicate_added_var` returns `Some(0)`, `aggregate_updates` returns
/// `None`, and the list match refuses. HEAD prints
/// `RHOLANG-MATCHER-AGGREGATE-UPDATES-REFUSAL: level 0 …` and answers `None`.
///
/// `ListMatch<Par>` is the production `ESetBody` list
/// (`spatial_matcher.rs:578-586`), i.e. a Rholang `Set` pattern.
#[test]
fn two_sibling_binding_negations_do_not_refuse_the_list_match() {
    // ★ MUTATION APPLIED (1): each sibling matches its target on its own, so
    // the refusal below cannot be explained by "the pattern does not match".
    for (target, n) in [(7i64, 8i64), (6, 9)] {
        let mut one = SpatialMatcherContext::new();
        assert_eq!(
            one.list_match_single(vec![gint(target)], vec![neg_par(binds_then_refuses(n))]),
            Some(()),
            "sibling ~({} /\\ {}) matches {} on its own",
            "x",
            n,
            target
        );
    }

    // ★ THE PROPERTY.
    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(
        ctx.list_match_single(vec![gint(7), gint(6)], vec![
            neg_par(binds_then_refuses(8)),
            neg_par(binds_then_refuses(9)),
        ]),
        Some(()),
        "★ two negations bind nothing, so their deltas are empty and disjoint; \
         the aggregation has nothing to refuse"
    );
    assert_eq!(
        ctx.free_map.get(&0),
        None,
        "★ neither negation bound level 0"
    );
    assert_eq!(ctx.free_map, new_free_map(), "and nothing else either");
}

/// The same verdict through a full `spatial_match(Par, Par)` — two sibling
/// **sends**, which is `ListMatch<Send>` (`spatial_matcher.rs:353`), the list a
/// `match` case pattern of the shape `@0!(~{x /\ 8}) | @1!(~{y /\ 9})`
/// actually drives.
#[test]
fn two_sibling_sends_with_binding_negations_still_match() {
    let pattern = par_of_sends(
        vec![
            send(gint(0), vec![neg_par(binds_then_refuses(8))], true),
            send(gint(1), vec![neg_par(binds_then_refuses(9))], true),
        ],
        true,
    );
    let target = par_of_sends(
        vec![
            send(gint(0), vec![gint(7)], false),
            send(gint(1), vec![gint(6)], false),
        ],
        false,
    );

    // ★ MUTATION APPLIED: one send alone matches.
    let mut one = SpatialMatcherContext::new();
    assert_eq!(
        one.spatial_match(
            par_of_sends(vec![send(gint(0), vec![gint(7)], false)], false),
            par_of_sends(
                vec![send(gint(0), vec![neg_par(binds_then_refuses(8))], true)],
                true
            ),
        ),
        Some(()),
        "a single send whose datum is a succeeding negation matches"
    );

    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(
        ctx.spatial_match(target, pattern),
        Some(()),
        "★ two sibling sends, each carrying a succeeding negation, match"
    );
    assert_eq!(ctx.free_map.get(&0), None, "★ the negations bound nothing");
    assert_eq!(ctx.free_map, new_free_map(), "and nothing else either");
}

// ─────────────────────────────────────────────────────────────────────────
// §3 — the normalizer's disposition, and the same verdict on a pattern the
//      normalizer produced rather than one this file hand-built
// ─────────────────────────────────────────────────────────────────────────

const RED_5_SOURCE: &str = "match Nil { @0!(~{x /\\ 8}) | @1!(~{y /\\ 9}) => Nil }";

fn red_5_pattern() -> Par {
    let par = Compiler::source_to_adt_with_normalizer_env(RED_5_SOURCE, HashMap::new())
        .expect("a match case pattern may contain a binding-bodied negation");
    par.matches
        .first()
        .and_then(|m| m.cases.first())
        .and_then(|c| c.pattern.clone())
        .expect("the single match case has a pattern")
}

/// ★ The premise, pinned: **every** `~` body is numbered from 0, because each
/// is normalized against its own fresh `FreeMap`. Two siblings therefore both
/// claim level 0 — which is precisely the linearity premise the
/// `aggregate_updates` backstop assumes and does not have.
///
/// This also answers "is a binding-bodied `~` legal?": it normalizes, the
/// enclosing `free_count` stays 0, and nothing warns.
#[test]
fn normalizer_emits_level_zero_in_every_negation_body() {
    let par = Compiler::source_to_adt_with_normalizer_env(RED_5_SOURCE, HashMap::new())
        .expect("★ a binding-bodied negation in a match case is ACCEPTED");

    let case = par
        .matches
        .first()
        .and_then(|m| m.cases.first())
        .expect("one match case");

    assert_eq!(
        case.free_count, 0,
        "★ the negations' bindings do not escape: the case binds nothing, so \
         the two `x`/`y` writes are silently useless"
    );

    let levels: Vec<i32> = case
        .pattern
        .as_ref()
        .expect("case pattern")
        .sends
        .iter()
        .map(|s| {
            let datum = s.data.first().expect("one datum per send");
            let body = match &datum
                .connectives
                .first()
                .expect("a connective")
                .connective_instance
            {
                Some(ConnNotBody(b)) => b.clone(),
                other => panic!("expected ConnNotBody, got {:?}", other),
            };
            let conjuncts = match &body
                .connectives
                .first()
                .expect("a connective")
                .connective_instance
            {
                Some(ConnAndBody(cb)) => cb.ps.clone(),
                other => panic!("expected ConnAndBody, got {:?}", other),
            };
            match &conjuncts[0].exprs[0].expr_instance {
                Some(models::rhoapi::expr::ExprInstance::EVarBody(models::rhoapi::EVar {
                    v:
                        Some(models::rhoapi::Var {
                            var_instance: Some(models::rhoapi::var::VarInstance::FreeVar(level)),
                        }),
                })) => *level,
                other => panic!("expected a FreeVar, got {:?}", other),
            }
        })
        .collect();

    assert_eq!(
        levels,
        vec![0, 0],
        "★ BOTH negation bodies are numbered from 0 — fresh-`FreeMap` \
         numbering, not a hand-built collision"
    );
}

/// The verdict again, on the normalizer's own output.
#[test]
fn a_normalized_two_sibling_negation_pattern_still_matches() {
    let target = par_of_sends(
        vec![
            send(gint(0), vec![gint(7)], false),
            send(gint(1), vec![gint(6)], false),
        ],
        false,
    );

    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(
        ctx.spatial_match(target, red_5_pattern()),
        Some(()),
        "★ `@0!(~{{x /\\ 8}}) | @1!(~{{y /\\ 9}})` matches `@0!(7) | @1!(6)`"
    );
    assert_eq!(ctx.free_map.get(&0), None, "★ the negations bound nothing");
    assert_eq!(ctx.free_map, new_free_map(), "and nothing else either");
}

// ─────────────────────────────────────────────────────────────────────────
// Controls — must NOT discriminate (green at HEAD and after)
// ─────────────────────────────────────────────────────────────────────────

/// Control: two siblings whose negations have **non-binding** bodies. No leak,
/// so no duplicated level, so no refusal — on either side. This is what makes
/// the RED above evidence rather than "the negation arm was rewritten".
#[test]
fn control_two_sibling_non_binding_negations_match() {
    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(
        ctx.list_match_single(vec![gint(7), gint(6)], vec![
            neg_par(gint(8)),
            neg_par(gint(9))
        ]),
        Some(()),
        "`~8` and `~9` both succeed against 7 and 6"
    );
    assert_eq!(ctx.free_map, new_free_map(), "a negation binds nothing");
}

/// Control: the negation **verdict** is a pure function of the inner verdict
/// and is unchanged by the restore.
#[test]
fn control_the_negation_verdict_is_unchanged() {
    let mut ctx = SpatialMatcherContext::new();

    assert_eq!(
        ctx.spatial_match(gint(7), neg_par(gint(8))),
        Some(()),
        "~8 succeeds against 7"
    );
    assert_eq!(
        ctx.spatial_match(gint(7), neg_par(gint(7))),
        None,
        "~7 refuses against 7"
    );
    assert_eq!(
        ctx.spatial_match(gint(7), neg_par(neg_par(gint(7)))),
        Some(()),
        "~~7 succeeds against 7"
    );
    assert_eq!(
        ctx.spatial_match(gint(7), neg_par(new_freevar_par(0, Vec::new()))),
        None,
        "a free variable matches everything, so ~x refuses"
    );
}

/// Control: `for` and `contract` patterns still reject a binding-bodied
/// negation at depth 0. The fix must not widen what the normalizer admits.
#[test]
fn control_for_patterns_still_reject_a_negation_at_depth_zero() {
    for src in [
        "for (@{~{x /\\ 8}} <- @0) { Nil }",
        "for (@{ @0!(~{x /\\ 8}) | @1!(~{y /\\ 9}) } <- @2) { Nil }",
        "contract @0(@{~{x /\\ 8}}) = { Nil }",
    ] {
        assert!(
            Compiler::source_to_adt_with_normalizer_env(src, HashMap::new()).is_err(),
            "a depth-0 receive pattern must still reject `~`: {}",
            src
        );
    }
}
