//! **An attempt that writes `free_map` and then refuses must revert it** — at
//! the three connective sites and the one retry loop that are interior to a
//! single `spatial_match(Par, Par)` call.
//!
//! # The defect
//!
//! `SpatialMatcherContext::free_map` is a plain `&mut` field, so a refusal is
//! *destructive*: whatever the refused attempt wrote is simply still there. The
//! matcher is a **search**, and a search that cannot undo a step is a search
//! that reports the wrong answer.
//!
//! Two earlier commits fixed this at two sites — `list_match::match_function`
//! (the bipartite-matcher boundary) and the `ConnOrBody` arm. Neither can see
//! the sites below, because all four are interior to one `spatial_match(Par,
//! Par)` call:
//!
//! ```text
//!   spatial_match(Par, Par)                        ← an entry snapshot here
//!     │                                              would HIDE the leak,
//!     │                                              not revert it
//!     ├─ match_connective_with_bounds               ← ④ the RETRY LOOP:
//!     │    for sp in sub_pars(target, …)                 one attempt per
//!     │      spatial_match(sp.0, connective) ─┐          candidate split
//!     │                                       │
//!     └──────────────────────────────────────▼
//!        SpatialMatcher<Par, Connective>
//!          ├─ ConnAndBody   try_fold over conjuncts   ← ① writes, then refuses
//!          ├─ ConnOrBody    find_map over branches    ← ⑤ fixed by `eaa905fe`
//!          ├─ ConnNotBody   INVERTS the verdict       ← ②③ the worst one
//!          └─ Conn{Bool,Int,String,Uri,ByteArray},
//!             VarRefBody, None                        ← pure: cannot write
//! ```
//!
//! ★ **Site ③ is the one that escapes on SUCCESS.** A negation whose body binds
//! and *then* refuses reports `Some(())` — correctly, because the inner pattern
//! did not match — and hands its caller a binding it had no right to make. The
//! caller cannot tell: with a bare `Option`, "the inner attempt refused" and
//! "the negation succeeded" are the same `Some(())`. That conflation is what
//! hid the leak, and `Attempt<T>` (`models::rust::utils`) is what separates
//! them.
//!
//! # Why a negation's bindings are worth nothing, and cost something
//!
//! `~P` and `P \/ Q` bodies are normalized against a **fresh** `FreeMap` that
//! is then discarded (`p_negation_normalizer.rs:10-13, 65-89`), so a free
//! variable under `~` is `FreeVar(0)` in a numbering nobody kept — while the
//! enclosing pattern's level 0 is a *different* variable in the *same shared*
//! `free_map`. The leak therefore lands on top of a legitimate binding rather
//! than trailing harmlessly behind it. `matcher_negation_isolation.rs` carries
//! that consequence out to a changed **verdict**; this file pins the four
//! sites' state behaviour.
//!
//! # What each test is for
//!
//! | test | site | escapes on |
//! |---|---|---|
//! | `a_refused_conjunction_reverts_the_conjuncts_that_already_bound` | ① `ConnAndBody` | refusal |
//! | `a_refused_conjunction_reverts_at_the_par_level_too` | ① `ConnAndBody` | refusal |
//! | `a_refused_negation_reverts_the_inner_matchs_bindings` | ② `ConnNotBody` | refusal |
//! | `a_rejected_retry_candidate_does_not_ride_into_the_accepted_one` | ②③ + ④ | **success** |
//! | `a_retry_loop_keeps_the_legitimate_binding_and_drops_the_leak` | ①②③ + ④ | **success** |
//! | `a_negation_binds_nothing_whatever_its_body_is_made_of` | ②③ vs ⑤ | both |
//!
//! Site ③'s own minimal witness lives in `matcher_negation_isolation.rs`
//! (`a_negation_that_succeeds_binds_nothing`), together with the verdict-level
//! consequence; it is not duplicated here.
//!
//! ⚠ **Honest caveat on the last two.** They exercise the retry loop *through*
//! the connective arms, and are therefore not independently discriminating for
//! site ④ from the outside: with the arms isolated, every arm already returns a
//! clean map on refusal, so site ④'s own isolation is a **local guarantee**
//! (the loop does not have to trust a whole-program property of every present
//! and future arm) rather than a second independent repair. The measured
//! diagnostic matrix in this change's commit message reports exactly that.
//!
//! ★ No test here expects a panic; every assertion is on a returned value or on
//! the observable `free_map`, pinned to a specific level
//! (`shared/tests/panic_expectation_gate.rs`).

use models::rhoapi::connective::ConnectiveInstance::*;
use models::rhoapi::{
    Bundle, Connective, ConnectiveBody, Expr, GUnforgeable, If, Match, New, Par, Receive, Send,
};
use models::rust::par_bag::Multiplicity;
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{
    new_free_map, new_freevar_par, new_gint_par, new_gstring_par, new_send, new_wildcard_par,
};
use rholang::rust::interpreter::matcher::par_count::{CountBound, ParCount};
use rholang::rust::interpreter::matcher::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
use rholang::rust::interpreter::matcher::sub_pars::sub_pars;
use rholang::rust::interpreter::util::prepend_connective;

// ─────────────────────────────────────────────────────────────────────────
// Fixtures
// ─────────────────────────────────────────────────────────────────────────

fn seven() -> Par { new_gint_par(7, Vec::new(), false) }

fn eight() -> Par { new_gint_par(8, Vec::new(), false) }

fn nil() -> Par { Par::default() }

fn free_var(level: i32) -> Par { new_freevar_par(level, Vec::new()) }

/// `p₁ /\ p₂ /\ …` as a bare `Connective`, so the conjunction arm is the unit
/// under test with no `Par`/`Par` bookkeeping interposed.
fn conjunction(ps: Vec<Par>) -> Connective {
    Connective {
        connective_instance: Some(ConnAndBody(ConnectiveBody { ps })),
    }
}

/// `p₁ \/ p₂ \/ …`, likewise bare.
fn disjunction(ps: Vec<Par>) -> Connective {
    Connective {
        connective_instance: Some(ConnOrBody(ConnectiveBody { ps })),
    }
}

/// `~p`, likewise bare.
fn negation(p: Par) -> Connective {
    Connective {
        connective_instance: Some(ConnNotBody(p)),
    }
}

/// A connective lifted to a `Par` of its own, which is how a pattern actually
/// carries one.
fn as_par(con: Connective) -> Par { prepend_connective(vector_par(Vec::new(), true), con, 0) }

/// `con | _` — the trailing wildcard is load-bearing.
///
/// Without it the pattern's remainder bound is `(0, 0)`
/// (`spatial_matcher.rs`'s `min_rem`/`max_rem`), so `sub_pars` offers the
/// connective exactly ONE candidate — the whole target — and the retry loop
/// never retries. The wildcard widens `max_rem` to `ParCount::unbounded()`, which is
/// what makes every subset of the target a candidate and the loop a real
/// search.
fn as_par_with_wildcard(con: Connective) -> Par {
    prepend_connective(new_wildcard_par(Vec::new(), true), con, 0)
}

/// `free_var(0) /\ 8`.
///
/// Against a target `t != 8` this binds level 0 to `t` and *then* demands
/// `t == 8`, so it **writes before it refuses**. That ordering is what makes
/// every test below non-vacuous: a conjunction that refused before writing
/// would leave nothing to leak.
///
/// The conjuncts share one free map on purpose
/// (`p_conjunction_normalizer.rs:12-14`) — unlike `~` and `\/`, whose bodies
/// get a fresh one — so this really is how a conjunction binds.
fn binds_then_refuses() -> Par { as_par(conjunction(vec![free_var(0), eight()])) }

/// `free_var(level) /\ Nil` — binds, and **matches `Nil`**.
///
/// Matching `Nil` is what forces the retry loop to work: `sub_pars` offers the
/// empty split first, and a body that matches it would let a negation refuse
/// (or accept) on the very first candidate with nothing else ever attempted.
/// This body makes the negation *refuse* the empty candidate, so a later
/// candidate must be tried — which is the only way a rejected candidate can
/// leave anything behind for the accepted one.
fn binds_and_matches_nil(level: i32) -> Par { as_par(conjunction(vec![free_var(level), nil()])) }

fn send(chan: Par, data: Vec<Par>, connective_used: bool) -> Send {
    new_send(chan, data, false, Vec::new(), connective_used)
}

fn par_of_sends(sends: Vec<Send>) -> Par { vector_par(Vec::new(), false).with_sends(sends) }

#[test]
fn trie_native_sub_pars_preserves_candidate_order_and_compresses_duplicate_positions() {
    let zero = ParCount::new(&Par::default());
    let mut exactly_one = zero.clone();
    exactly_one.sends = CountBound::Finite(1);
    let unconstrained_prune = ParCount::unbounded();

    let distinct = par_of_sends(vec![
        send(eight(), Vec::new(), false),
        send(new_gint_par(9, Vec::new(), false), Vec::new(), false),
        send(seven(), Vec::new(), false),
    ]);
    let selected_channels = sub_pars(
        &distinct,
        &exactly_one,
        &exactly_one,
        &zero,
        &unconstrained_prune,
    )
    .map(|(selected, complement)| {
        assert_eq!(selected.family_cardinality::<Send>(), 1);
        assert_eq!(complement.family_cardinality::<Send>(), 2);
        selected
            .first::<Send>()
            .expect("a trie-native matcher partition is canonical")
            .expect("an exactly-one partition carries one Send")
            .0
            .chan
            .expect("the fixture Send has a channel")
    })
    .collect::<Vec<_>>();
    assert_eq!(selected_channels, vec![
        seven(),
        eight(),
        new_gint_par(9, Vec::new(), false)
    ]);

    let duplicate = send(seven(), Vec::new(), false);
    let duplicate_target = par_of_sends(vec![
        duplicate.clone(),
        duplicate.clone(),
        duplicate.clone(),
    ]);
    let duplicate_candidates = sub_pars(
        &duplicate_target,
        &exactly_one,
        &exactly_one,
        &zero,
        &unconstrained_prune,
    )
    .collect::<Vec<_>>();
    assert_eq!(duplicate_candidates.len(), 1);
    assert!(duplicate_candidates.iter().all(|(selected, complement)| {
        selected.family_cardinality::<Send>() == 1 && complement.family_cardinality::<Send>() == 2
    }));

    let mut above_legacy_ceiling = Par::default();
    above_legacy_ceiling
        .insert_component_many(
            &duplicate,
            Multiplicity::new(1_001).expect("positive multiplicity"),
            Vec::new(),
            false,
        )
        .expect("one compressed Send endpoint stores 1,001 occurrences");
    let mut exactly_all = zero.clone();
    exactly_all.sends = CountBound::Finite(1_001);
    let above_ceiling_candidates = sub_pars(
        &above_legacy_ceiling,
        &exactly_all,
        &exactly_all,
        &zero,
        &ParCount::unbounded(),
    )
    .collect::<Vec<_>>();
    assert_eq!(above_ceiling_candidates.len(), 1);
    assert_eq!(
        above_ceiling_candidates[0].0.family_cardinality::<Send>(),
        1_001
    );
    assert_eq!(
        above_ceiling_candidates[0].1.family_cardinality::<Send>(),
        0
    );
}

#[test]
fn every_partitioned_par_family_admits_compressed_multiplicity_above_the_legacy_ceiling() {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::g_unforgeable::UnfInstance;
    use models::rhoapi::{GPrivate, MatchCase, ReceiveBind};

    let zero = ParCount::zero();

    macro_rules! assert_family {
        ($label:literal, $make:expr, $family:ty, $field:ident) => {{
            let make = $make;
            let component: $family = make(7i64);
            let other: $family = make(8i64);
            assert_ne!(component, other);
            for (cardinality, other_count) in [(1_001, 0), (u64::MAX, 0), (u64::MAX, 1)] {
                let multiplicity = Multiplicity::new(cardinality).unwrap();
                let mut target = Par::default();
                target
                    .insert_component_many(&component, multiplicity, Vec::new(), false)
                    .unwrap_or_else(|error| {
                        panic!("cannot build compressed {} target: {error}", $label)
                    });
                if other_count != 0 {
                    target
                        .insert_component_many(&other, Multiplicity::ONE, Vec::new(), false)
                        .unwrap();
                }
                let retained = target.clone();
                let expected_total = u128::from(cardinality) + u128::from(other_count);
                let mut endpoint_total = 0u128;
                target
                    .try_visit_components::<$family, std::convert::Infallible>(|_, count| {
                        endpoint_total += u128::from(count.get());
                        Ok(())
                    })
                    .unwrap();
                assert_eq!(endpoint_total, expected_total);
                assert_eq!(target.family_cardinality_exact::<$family>(), endpoint_total);
                assert_eq!(
                    target.family_distinct_len::<$family>(),
                    1 + usize::from(other_count != 0)
                );
                assert_eq!(
                    target.family_cardinality_is_saturated::<$family>(),
                    other_count != 0
                );
                assert_eq!(
                    ParCount::new(&target).$field,
                    CountBound::Finite(endpoint_total)
                );
                let mut candidates = sub_pars(&target, &zero, &ParCount::unbounded(), &zero, &zero);
                let (selected, complement) = candidates.next().unwrap_or_else(|| {
                    panic!(
                        "logical top omitted the complete {} target of {cardinality}",
                        $label
                    )
                });
                assert!(candidates.next().is_none());
                assert_eq!(
                    selected.family_cardinality_exact::<$family>(),
                    endpoint_total,
                    "selected {} multiplicity was truncated",
                    $label
                );
                assert_eq!(
                    complement.family_cardinality_exact::<$family>(),
                    0,
                    "exact-all {} partition left a complement",
                    $label
                );
                assert!(complement.is_empty());
                assert!(target
                    .components()
                    .shares_storage_with(retained.components()));
                for (value, expected_count) in [(&component, cardinality), (&other, other_count)] {
                    assert_eq!(target.count(value), expected_count);
                    assert_eq!(retained.count(value), expected_count);
                    assert_eq!(selected.count(value), expected_count);
                    assert_eq!(complement.count(value), 0);
                    assert_eq!(
                        u128::from(selected.count(value)) + u128::from(complement.count(value)),
                        u128::from(target.count(value))
                    );
                }
            }
        }};
    }

    assert_family!(
        "Send",
        |value| send(new_gint_par(value, Vec::new(), false), Vec::new(), false),
        Send,
        sends
    );
    assert_family!(
        "Receive",
        |value| Receive {
            binds: vec![ReceiveBind {
                patterns: vec![nil()],
                source: Some(seven()),
                ..Default::default()
            }],
            body: Some(new_gint_par(value, Vec::new(), false)),
            ..Default::default()
        },
        Receive,
        receives
    );
    assert_family!(
        "New",
        |value| New {
            bind_count: 1,
            p: Some(new_gint_par(value, Vec::new(), false)),
            ..Default::default()
        },
        New,
        news
    );
    assert_family!(
        "Expr",
        |value| Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        },
        Expr,
        exprs
    );
    assert_family!(
        "Match",
        |value| Match {
            target: Some(new_gint_par(value, Vec::new(), false)),
            cases: vec![MatchCase {
                pattern: Some(new_gint_par(value, Vec::new(), false)),
                source: Some(nil()),
                ..Default::default()
            }],
            ..Default::default()
        },
        Match,
        matches
    );
    assert_family!(
        "GUnforgeable",
        |value: i64| GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                id: value.to_be_bytes().to_vec(),
            })),
        },
        GUnforgeable,
        unforgeables
    );
    assert_family!(
        "Bundle",
        |value| Bundle {
            body: Some(new_gint_par(value, Vec::new(), false)),
            read_flag: true,
            write_flag: true,
        },
        Bundle,
        bundles
    );
    assert_family!(
        "If",
        |value| If {
            condition: Some(models::rust::utils::new_gbool_par(true, Vec::new(), false)),
            if_true: Some(new_gint_par(value, Vec::new(), false)),
            if_false: Some(nil()),
            ..Default::default()
        },
        If,
        conditionals
    );
}

fn maximum_plus_one_expression_target() -> (Par, Expr, Expr) {
    let first = seven().take_occurrences::<Expr>().pop().unwrap();
    let second = eight().take_occurrences::<Expr>().pop().unwrap();
    let mut target = Par::default();
    target
        .insert_component_many(
            &first,
            Multiplicity::new(u64::MAX).unwrap(),
            Vec::new(),
            false,
        )
        .unwrap();
    target
        .insert_component_many(&second, Multiplicity::ONE, Vec::new(), false)
        .unwrap();
    assert_eq!(target.family_distinct_len::<Expr>(), 2);
    assert!(target.family_cardinality_is_saturated::<Expr>());
    (target, first, second)
}

#[test]
fn parcount_saturated_target_respects_zero_complement() {
    let (target, first, second) = maximum_plus_one_expression_target();
    let retained = target.clone();
    let zero = ParCount::zero();
    let mut candidates = sub_pars(&target, &zero, &ParCount::unbounded(), &zero, &zero);
    let (selected, complement) = candidates.next().expect("the complete target is feasible");
    assert!(
        complement.is_empty(),
        "an exact-zero complement must be empty"
    );
    assert_eq!(selected.count(&first), u64::MAX);
    assert_eq!(selected.count(&second), 1);
    assert!(candidates.next().is_none());
    assert_eq!(target.count(&first), retained.count(&first));
    assert_eq!(target.count(&second), retained.count(&second));
}

#[test]
fn parcount_saturated_target_respects_exact_one_complement() {
    let (target, first, second) = maximum_plus_one_expression_target();
    let zero = ParCount::zero();
    let mut one = zero.clone();
    one.exprs = CountBound::Finite(1);
    let mut candidates = sub_pars(&target, &zero, &ParCount::unbounded(), &one, &one);
    let (selected, complement) = candidates
        .next()
        .expect("an exact-one complement is feasible");
    assert_eq!(complement.family_cardinality::<Expr>(), 1);
    assert_eq!(
        u128::from(selected.count(&first)) + u128::from(selected.count(&second)),
        u128::from(u64::MAX)
    );
    for expr in [&first, &second] {
        assert_eq!(
            u128::from(selected.count(expr)) + u128::from(complement.count(expr)),
            u128::from(target.count(expr))
        );
    }
}

#[test]
fn parcount_saturated_free_mass_preserves_concrete_expression_minimum() {
    let concrete = seven().take_occurrences::<Expr>().pop().unwrap();
    for mut free in [free_var(0), new_wildcard_par(Vec::new(), true)] {
        let free = free.take_occurrences::<Expr>().pop().unwrap();
        let mut pattern = Par::default();
        pattern
            .insert_component_many(
                &free,
                Multiplicity::new(u64::MAX).unwrap(),
                Vec::new(),
                true,
            )
            .unwrap();
        pattern
            .insert_component_many(&concrete, Multiplicity::ONE, Vec::new(), false)
            .unwrap();
        let (minimum, maximum) = ParCount::zero().min_max_par_ref(&pattern);
        assert_eq!(minimum.exprs, CountBound::Finite(1));
        assert_eq!(maximum.exprs, CountBound::Unbounded);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// The REDs
// ─────────────────────────────────────────────────────────────────────────

/// ★ RED — site ①. A conjunction whose second conjunct refuses must un-bind the
/// first one.
///
/// The isolation goes around the **whole** fold, not around each conjunct: the
/// conjuncts stand or fall together, so a refusal must revert *all* of them,
/// not merely the one that refused.
///
/// At HEAD: `None`, with `free_map = {0 ↦ GInt(7)}`.
#[test]
fn a_refused_conjunction_reverts_the_conjuncts_that_already_bound() {
    // ★ MUTATION APPLIED: the first conjunct really is attempted and really
    // does bind, which is the only reason there is anything to revert.
    let mut probe = SpatialMatcherContext::new();
    assert_eq!(
        probe.spatial_match(seven(), conjunction(vec![free_var(0)])),
        Some(()),
        "a one-conjunct conjunction of a free variable matches and binds"
    );
    assert_eq!(
        probe.free_map.get(&0),
        Some(&seven()),
        "…level 0 to the target"
    );

    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(
        ctx.spatial_match(seven(), conjunction(vec![free_var(0), eight()])),
        None,
        "the second conjunct demands 8 and the target is 7, so the conjunction refuses"
    );
    assert_eq!(
        ctx.free_map.get(&0),
        None,
        "★ a refused conjunction un-binds the conjunct that had already bound"
    );
    assert_eq!(ctx.free_map, new_free_map(), "and nothing else either");
}

/// The same property one layer out, where a pattern actually carries the
/// connective: `{x /\ 8}` as a `Par`.
#[test]
fn a_refused_conjunction_reverts_at_the_par_level_too() {
    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(ctx.spatial_match(seven(), binds_then_refuses()), None);
    assert_eq!(ctx.free_map.get(&0), None, "★ nothing bound");
    assert_eq!(ctx.free_map, new_free_map());
}

/// ★ RED — site ②. A negation that **refuses** must un-bind whatever its inner
/// match bound before succeeding.
///
/// `~x` refuses against any target, because a free variable matches everything
/// — and binds it on the way. The negation's verdict is right at HEAD; the
/// binding it leaves behind is not.
///
/// At HEAD: `None`, with `free_map = {0 ↦ GInt(7)}`.
#[test]
fn a_refused_negation_reverts_the_inner_matchs_bindings() {
    // ★ MUTATION APPLIED: the inner match really does succeed, and really does
    // bind, which is precisely why the negation refuses.
    let mut probe = SpatialMatcherContext::new();
    assert_eq!(
        probe.spatial_match(seven(), free_var(0)),
        Some(()),
        "a free variable matches the target"
    );
    assert_eq!(probe.free_map.get(&0), Some(&seven()), "…and binds it");

    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(
        ctx.spatial_match(seven(), negation(free_var(0))),
        None,
        "the inner pattern matched, so the negation refuses"
    );
    assert_eq!(
        ctx.free_map.get(&0),
        None,
        "★ a negation binds nothing — not even when it refuses"
    );
    assert_eq!(ctx.free_map, new_free_map(), "and nothing else either");
}

/// ★ RED — site ④ (through ②③). A candidate split the retry loop **rejected**
/// must not leave its bindings behind for the one it **accepts**.
///
/// `~{x /\ Nil} | _` against `@1!(9)`:
///
/// ```text
///   candidate {}          inner {x /\ Nil} MATCHES Nil, binding level 0
///                         ⇒ the negation REFUSES ⇒ next candidate
///   candidate {@1!(9)}    inner {x /\ Nil} refuses (Nil ≠ @1!(9))
///                         ⇒ the negation SUCCEEDS ⇒ accepted, remainder = ∅
/// ```
///
/// The accepted candidate bound nothing. At HEAD the answer nevertheless
/// carries `{0 ↦ Nil}` — the rejected candidate's binding, riding out on the
/// accepted one's verdict.
#[test]
fn a_rejected_retry_candidate_does_not_ride_into_the_accepted_one() {
    // ★ MUTATION APPLIED: the first candidate really is rejected — the body
    // matches `Nil`, so the negation refuses it and the loop must try another.
    let mut probe = SpatialMatcherContext::new();
    assert_eq!(
        probe.spatial_match(nil(), as_par(negation(binds_and_matches_nil(0)))),
        None,
        "against the empty target the body matches, so the negation refuses"
    );

    for target in [
        par_of_sends(vec![send(
            new_gint_par(1, Vec::new(), false),
            vec![nil()],
            false,
        )]),
        par_of_sends(vec![
            send(new_gint_par(2, Vec::new(), false), vec![nil()], false),
            send(new_gint_par(1, Vec::new(), false), vec![nil()], false),
        ]),
    ] {
        let mut ctx = SpatialMatcherContext::new();
        assert_eq!(
            ctx.spatial_match(
                target.clone(),
                as_par_with_wildcard(negation(binds_and_matches_nil(0)))
            ),
            Some(()),
            "a later candidate makes the negation succeed, so the pattern matches"
        );
        assert_eq!(
            ctx.free_map.get(&0),
            None,
            "★ the REJECTED candidate's binding did not ride out on the accepted one"
        );
        assert_eq!(ctx.free_map, new_free_map(), "and nothing else either");
    }
}

/// ★ RED — the two dispositions in one pattern, which is the sharpest form of
/// the property: `{x /\ Nil} | ~{y /\ Nil} | _`.
///
/// The conjunction's binding of `x` is the pattern's **real answer** and must
/// survive. The negation's binding of `y` is a write into a numbering the
/// normalizer discarded and must not. At HEAD *both* survive; a fix that
/// reverted too much would drop *both*, and that is why this test asserts each
/// level separately instead of asserting the map is empty.
#[test]
fn a_retry_loop_keeps_the_legitimate_binding_and_drops_the_leak() {
    let target = par_of_sends(vec![
        send(new_gint_par(2, Vec::new(), false), vec![nil()], false),
        send(new_gint_par(1, Vec::new(), false), vec![nil()], false),
    ]);

    // `{x /\ Nil} | ~{y /\ Nil} | _`, built outwards from the wildcard.
    let pattern = prepend_connective(
        prepend_connective(
            new_wildcard_par(Vec::new(), true),
            negation(binds_and_matches_nil(1)),
            0,
        ),
        conjunction(vec![free_var(0), nil()]),
        0,
    );

    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(
        ctx.spatial_match(target, pattern),
        Some(()),
        "the conjunction takes the empty split and the negation a non-empty one"
    );
    assert_eq!(
        ctx.free_map.get(&0),
        Some(&nil()),
        "★ ANTI-OVER-RESTORE: the conjunction's binding is the pattern's answer \
         and must survive"
    );
    assert_eq!(
        ctx.free_map.get(&1),
        None,
        "★ the negation's binding is a write into a discarded numbering and must not"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Controls — must NOT discriminate (green at HEAD and after)
// ─────────────────────────────────────────────────────────────────────────

/// C1. The **verdict** of every connective is a pure function of `(target,
/// pattern)` and is unchanged by the restore.
///
/// No arm's decision reads `free_map`, so reverting one cannot move an answer.
/// This is the table that says so, over all three binding connectives and the
/// degenerate shapes at their edges.
#[test]
fn control_the_connective_verdict_is_unchanged() {
    let mut ctx = SpatialMatcherContext::new();
    let cases: Vec<(&str, Connective, Option<()>)> = vec![
        ("all conjuncts match", conjunction(vec![seven()]), Some(())),
        (
            "the first conjunct fails",
            conjunction(vec![eight(), seven()]),
            None,
        ),
        (
            "the second conjunct fails",
            conjunction(vec![seven(), eight()]),
            None,
        ),
        (
            "an empty conjunction demands nothing",
            conjunction(Vec::new()),
            Some(()),
        ),
        ("a negation of a match", negation(seven()), None),
        ("a negation of a non-match", negation(eight()), Some(())),
        (
            "a nested negation",
            negation(as_par(negation(seven()))),
            Some(()),
        ),
        (
            "the first branch matches",
            disjunction(vec![seven()]),
            Some(()),
        ),
        (
            "only the second branch matches",
            disjunction(vec![eight(), seven()]),
            Some(()),
        ),
        (
            "no branch matches",
            disjunction(vec![eight(), eight()]),
            None,
        ),
        (
            "an empty disjunction matches nothing",
            disjunction(Vec::new()),
            None,
        ),
    ];

    for (what, con, expected) in cases {
        assert_eq!(ctx.spatial_match(seven(), con), expected, "{}", what);
    }
}

/// C2 ★ THE ANTI-OVER-RESTORE CONTROL. A **successful** conjunction binds the
/// union of its conjuncts, and every one of those bindings survives.
///
/// If the conjunction's restore were written *unconditional* — copying the
/// disjunction's shape, which is correct for a disjunction because its branches
/// disagree about what they bind — this flips green → red. That would be a
/// **defect in the fix**, not an expectation to adjust.
#[test]
fn control_a_successful_conjunction_still_binds_the_union_of_its_conjuncts() {
    // One conjunct binds.
    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(
        ctx.spatial_match(seven(), conjunction(vec![free_var(0), seven()])),
        Some(()),
        "the free variable matches and the literal agrees, so the conjunction matches"
    );
    assert_eq!(
        ctx.free_map.get(&0),
        Some(&seven()),
        "★ and it really did bind"
    );

    // Two conjuncts bind, at distinct levels — the UNION, not the last one.
    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(
        ctx.spatial_match(seven(), conjunction(vec![free_var(0), free_var(1)])),
        Some(()),
        "two free variables both match the target"
    );
    assert_eq!(ctx.free_map.get(&0), Some(&seven()), "★ level 0 survived");
    assert_eq!(ctx.free_map.get(&1), Some(&seven()), "★ and so did level 1");
}

/// C3. A **non-binding** negation is unchanged, in verdict and in state.
///
/// This is the shape the 49-test acceptance oracle uses, so it pins that the
/// arm rewrite did not move the oracle's ground.
#[test]
fn control_a_non_binding_negation_is_unchanged() {
    let mut ctx = SpatialMatcherContext::new();

    assert_eq!(
        ctx.spatial_match(seven(), negation(nil())),
        Some(()),
        "`~Nil` succeeds against a non-empty target"
    );
    assert_eq!(ctx.free_map, new_free_map(), "and binds nothing");

    assert_eq!(
        ctx.spatial_match(nil(), negation(nil())),
        None,
        "`~Nil` refuses against `Nil`"
    );
    assert_eq!(ctx.free_map, new_free_map(), "and still binds nothing");
}

/// C4. The disjunction arm is behaviourally untouched by being re-expressed
/// through the combinator.
///
/// It was already correct (`eaa905fe`), and its hand-written clone/run/restore
/// was byte-for-byte what `attempt_opt` does — so it is the **control for the
/// combinator itself**: if `attempt_opt` did anything other than that, this
/// moves. `rholang/tests/matcher_disjunction_isolation.rs` stays byte-untouched
/// and asserts the same two properties independently.
#[test]
fn control_the_disjunction_arm_is_behaviourally_untouched() {
    // A refused branch leaves nothing behind for the branch that succeeds.
    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(
        ctx.spatial_match(seven(), disjunction(vec![binds_then_refuses(), seven()])),
        Some(()),
        "the second branch matches, so the disjunction matches"
    );
    assert_eq!(ctx.free_map, new_free_map(), "and the first left nothing");

    // When every branch refuses, the caller's map comes back as it went in.
    let mut ctx = SpatialMatcherContext::new();
    assert_eq!(
        ctx.spatial_match(
            seven(),
            disjunction(vec![binds_then_refuses(), binds_then_refuses()])
        ),
        None,
        "no branch matches, so the disjunction does not match"
    );
    assert_eq!(ctx.free_map, new_free_map(), "and bound nothing");
}

/// The three (`~body-with-a-disjunction`, `~equivalent-body-without-one`)
/// pairs shared by C5 and the RED below it.
///
/// The third pair is the interesting one: both bodies bind and then refuse, so
/// at HEAD they *differ* in state — the disjunction restores per branch
/// (`eaa905fe`) while the bare conjunction does not. Their **verdicts** agree on
/// both sides of this change, which is the only thing the deleted `has_or_body`
/// branch could ever have decided.
fn negation_body_pairs() -> Vec<(&'static str, Par, Par)> {
    let with_or = |ps: Vec<Par>| as_par(disjunction(ps));

    vec![
        (
            "a body that matches",
            with_or(vec![seven(), eight()]),
            seven(),
        ),
        (
            "a body that does not match",
            with_or(vec![
                eight(),
                new_gstring_par("no".to_string(), Vec::new(), false),
            ]),
            eight(),
        ),
        (
            "a body whose branches all bind and all refuse",
            with_or(vec![binds_then_refuses(), binds_then_refuses()]),
            binds_then_refuses(),
        ),
    ]
}

/// C5. A negation whose body contains a disjunction is **not** a special case.
///
/// The arm used to compute `has_or_body` and then branch on it into two
/// byte-identical blocks: the flag was computed, matched on, and then both arms
/// did exactly the same thing. Deleting the dead branch is behaviour-free — this
/// is what says so rather than leaving it trusted.
///
/// ★ It asserts on the **verdict** and only the verdict, because the verdict is
/// all a branch taken on `has_or_body` could ever have moved. Green at HEAD and
/// after; the *state* half of the same comparison is a RED and is asserted
/// separately below, where its classification is honest.
#[test]
fn control_a_negation_over_a_disjunction_is_not_a_special_case() {
    for (what, or_body, plain_body) in negation_body_pairs() {
        let mut with = SpatialMatcherContext::new();
        let with_verdict = with.spatial_match(seven(), negation(or_body));

        let mut without = SpatialMatcherContext::new();
        let without_verdict = without.spatial_match(seven(), negation(plain_body));

        assert_eq!(
            with_verdict, without_verdict,
            "★ the deleted `has_or_body` branch decided nothing: {}",
            what
        );
    }
}

/// ★ RED — and the reason C5 above stops at the verdict.
///
/// A negation binds nothing, whatever its body is made of. At HEAD the two
/// bodies of the third pair **disagree**: `~{B \/ B}` answers with an empty map
/// because the disjunction arm restores per branch (`eaa905fe`), while
/// `~{x /\ 8}` answers with `{0 ↦ 7}` because the conjunction and the negation
/// do not. Asserting that inside a *control* would have been asserting that the
/// leak is absent, in a test whose job is to show that nothing moved.
#[test]
fn a_negation_binds_nothing_whatever_its_body_is_made_of() {
    for (what, or_body, plain_body) in negation_body_pairs() {
        let mut with = SpatialMatcherContext::new();
        with.spatial_match(seven(), negation(or_body));

        let mut without = SpatialMatcherContext::new();
        without.spatial_match(seven(), negation(plain_body));

        assert_eq!(
            with.free_map, without.free_map,
            "★ the two bodies agree on state as well as verdict: {}",
            what
        );
        assert_eq!(
            with.free_map,
            new_free_map(),
            "★ and what they agree on is that a negation binds nothing: {}",
            what
        );
    }
}
