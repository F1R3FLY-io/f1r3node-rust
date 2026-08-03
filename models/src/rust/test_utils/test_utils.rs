//! # Property-test generators for the `Par` family
//!
//! ## ⚠ The `0..1` vacuity defect (fixed 2026-07-26 — read before editing)
//!
//! Every collection size range in this file used to be written `0..1`.
//! `proptest`'s `SizeRange` is built from a `Range<usize>` with
//! `end_incl() == end - 1`, so `0..1` means **exactly zero elements, always**.
//! `generate_par(d)` therefore produced, for *every* `d`, one of only two
//! values — the empty `Par` and the empty `Par` with `connective_used = true`
//! — and `generate_send` / `generate_receive` / `generate_new` /
//! `generate_match` / `generate_bundle` / `generate_connective` were **never
//! invoked at all**, because the vectors that would have held their output
//! were always empty.
//!
//! Everything driven by these generators was consequently asserting a property
//! over a two-element set. That included `models/tests/scored_term_sort_test.rs`
//! — the sorter's *only* property tests — which is why the sorter could not
//! gate its own explicit-worklist conversion. See the audit's §8.4 limit #3:
//! `docs/design/audits/theta-depth-traversals-2026-07-26.md`.
//!
//! The ranges are now `0..=2` (inclusive), and [`assert_generator_not_vacuous`]
//! is available so that any new property test can assert its corpus actually
//! reaches the shape it claims to cover. **A harness that cannot fail loudly
//! will eventually report a comfortable number for the wrong reason.**
//!
//! ## What these generators are, and are not, fit for
//!
//! They produce *shallow, arbitrary* terms: good for value-level algebraic
//! properties. They are **not** fit for arm coverage (a `Union` of four
//! `ExprInstance` variants out of 36) and **not** fit for the depth regime the
//! Θ(depth) audit is about. Use a constructed corpus for arm coverage
//! (`rholang/tests/substitution_corpus.rs`,
//! `rholang/tests/by_reference_readers_equivalence.rs`) and
//! `rholang/tests/stack_depth_gate.rs` for depth.

use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;

use crate::rhoapi::{
    connective, expr, var, Bundle, Connective, ENot, Expr, Match, MatchCase, New, Par, Receive,
    ReceiveBind, Send, Var,
};
use crate::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use crate::rust::rholang::sorter::sortable::Sortable;

/// Draw `draws` values from `strategy` and fail if `predicate` never holds.
///
/// This is the anti-vacuity guard. It exists because the defect documented at
/// the top of this file was invisible: every test driven by the broken
/// generators *passed*, because a universally quantified property over an
/// empty (or two-element) set is trivially true.
///
/// Use it in any property test whose validity depends on the corpus reaching a
/// particular shape — and name the shape in `what`, so a future failure says
/// what stopped being generated.
pub fn assert_generator_not_vacuous<A: std::fmt::Debug>(
    what: &str,
    strategy: BoxedStrategy<A>,
    draws: usize,
    predicate: impl Fn(&A) -> bool,
) {
    let mut runner = TestRunner::deterministic();
    let mut hits = 0usize;
    for _ in 0..draws {
        let value = strategy
            .new_tree(&mut runner)
            .expect("assert_generator_not_vacuous: strategy failed to produce a value")
            .current();
        if predicate(&value) {
            hits += 1;
        }
    }
    assert!(
        hits > 0,
        "ANTI-VACUITY: `{}` was not produced in {} draws. Any property quantified over \
         this generator is currently vacuous — it passes without testing anything. See \
         the module documentation in models/src/rust/test_utils/test_utils.rs.",
        what,
        draws
    );
}

// models/src/test/scala/coop/rchain/models/testUtils/TestUtils.scala
pub fn sort(par: &Par) -> Par { ParSortMatcher::sort_match(par).term }

pub fn for_all_similar_a<A: Clone + std::fmt::Debug>(
    generator: BoxedStrategy<A>,
    block: impl Fn(&A, &A),
) {
    proptest!(|(values in prop::collection::vec(generator, 5))| {
        for x in &values {
            for y in &values {
                block(x, y);
            }
        }
    });
}

/*
In this code, we chose a manual approach to generate recursive structures for testing instead of relying on the automatic Arbitrary trait provided by proptest. Here's why:

1. Recursive Nature of Structures:
   Many Rholang data structures (e.g., `Par`, `Bundle`, `Match`, etc.) have recursive fields, allowing them to contain other instances of similar structures. This makes automatic generation challenging and prone to issues like infinite recursion or stack overflow.

2. Control Over Depth:
   By using a manual generator with a `depth` parameter, we can explicitly limit the depth of recursion during generation. This ensures that:
   - Tests remain performant.
   - We avoid excessively nested structures that may cause panics or unmanageable complexity.

3. Handling Optional Fields:
   Some fields, such as `Option<Par>` in `Bundle`, require non-`None` values for meaningful tests. The automatic approach doesn't guarantee that these fields will be populated correctly, whereas manual generation allows us to enforce such constraints.

4. Custom Combinations:
   Recursive structures often involve multiple variants and edge cases. Manual generators let us explicitly define how these combinations are generated, providing better test coverage and control compared to the default Arbitrary implementation.

5. Avoiding Arbitrary's Limitations:
   While proptest's Arbitrary trait simplifies test data generation for flat or non-recursive types, it struggles with deeply recursive or highly interdependent types. Manual generation avoids the inherent limitations of the trait and ensures reliable and predictable behavior.
*/

pub fn generate_par(depth: usize) -> BoxedStrategy<Par> {
    if depth == 0 {
        return Just(Par {
            sends: vec![],
            receives: vec![],
            news: vec![],
            exprs: vec![],
            matches: vec![],
            bundles: vec![],
            connectives: vec![],
            conditionals: vec![],
            locally_free: vec![],
            connective_used: false,
            unforgeables: vec![],
        })
        .boxed();
    }

    (
        proptest::collection::vec(generate_send(depth - 1), 0..=2),
        proptest::collection::vec(generate_receive(depth - 1), 0..=2),
        proptest::collection::vec(generate_new(depth - 1), 0..=2),
        proptest::collection::vec(generate_expr(depth - 1), 0..=2),
        proptest::collection::vec(generate_match(depth - 1), 0..=2),
        proptest::collection::vec(generate_bundle(depth - 1), 0..=2),
        proptest::collection::vec(generate_connective(depth - 1), 0..=2),
        proptest::collection::vec(any::<u8>(), 0..=2),
        any::<bool>(),
    )
        .prop_map(
            |(
                sends,
                receives,
                news,
                exprs,
                matches,
                bundles,
                connectives,
                locally_free,
                connective_used,
            )| Par {
                sends,
                receives,
                news,
                exprs,
                matches,
                bundles,
                connectives,
                conditionals: vec![],
                locally_free,
                connective_used,
                unforgeables: vec![],
            },
        )
        .boxed()
}

pub fn generate_send(depth: usize) -> BoxedStrategy<Send> {
    if depth == 0 {
        return Just(Send {
            chan: Some(
                generate_par(0)
                    .boxed()
                    .new_tree(&mut Default::default())
                    .unwrap()
                    .current(),
            ),
            data: vec![],
            persistent: false,
            locally_free: vec![],
            connective_used: false,
        })
        .boxed();
    }

    (
        generate_par(depth - 1),
        proptest::collection::vec(generate_par(depth - 1), 0..=2),
        any::<bool>(),
        proptest::collection::vec(any::<u8>(), 0..=2),
        any::<bool>(),
    )
        .prop_map(
            |(chan, data, persistent, locally_free, connective_used)| Send {
                chan: Some(chan),
                data,
                persistent,
                locally_free,
                connective_used,
            },
        )
        .boxed()
}

pub fn generate_receive(depth: usize) -> BoxedStrategy<Receive> {
    if depth == 0 {
        return Just(Receive {
            binds: vec![],
            body: Some(
                generate_par(0)
                    .boxed()
                    .new_tree(&mut Default::default())
                    .unwrap()
                    .current(),
            ),
            persistent: false,
            peek: false,
            bind_count: 0,
            locally_free: vec![],
            connective_used: false,
            condition: None,
        })
        .boxed();
    }

    (
        proptest::collection::vec(
            (
                proptest::collection::vec(generate_par(depth - 1), 0..=2),
                generate_par(depth - 1),
                generate_option_var(depth),
                any::<i32>(),
            )
                .prop_map(|(patterns, source, remainder, free_count)| ReceiveBind {
                    patterns,
                    source: Some(source),
                    remainder,
                    free_count,
                }),
            0..=2,
        ),
        generate_par(depth - 1),
        any::<bool>(),
        any::<bool>(),
        any::<i32>(),
        proptest::collection::vec(any::<u8>(), 0..=2),
        any::<bool>(),
    )
        .prop_map(
            |(binds, body, persistent, peek, bind_count, locally_free, connective_used)| Receive {
                binds,
                body: Some(body),
                persistent,
                peek,
                bind_count,
                locally_free,
                connective_used,
                condition: None,
            },
        )
        .boxed()
}

pub fn generate_new(depth: usize) -> BoxedStrategy<New> {
    if depth == 0 {
        return Just(New {
            bind_count: 0,
            p: Some(
                generate_par(0)
                    .boxed()
                    .new_tree(&mut Default::default())
                    .unwrap()
                    .current(),
            ),
            uri: vec![],
            injections: Default::default(),
            locally_free: vec![],
        })
        .boxed();
    }

    (
        any::<i32>(),
        generate_par(depth - 1),
        proptest::collection::vec(any::<String>(), 0..=2),
        proptest::collection::btree_map(any::<String>(), generate_par(depth - 1), 0..=2),
        proptest::collection::vec(any::<u8>(), 0..=2),
    )
        .prop_map(|(bind_count, p, uri, injections, locally_free)| New {
            bind_count,
            p: Some(p),
            uri,
            injections,
            locally_free,
        })
        .boxed()
}

pub fn generate_expr(depth: usize) -> BoxedStrategy<Expr> {
    if depth == 0 {
        return Just(Expr {
            expr_instance: None,
        })
        .boxed();
    }

    proptest::strategy::Union::new(vec![
        any::<bool>()
            .prop_map(|v| Expr {
                expr_instance: Some(expr::ExprInstance::GBool(v)),
            })
            .boxed(),
        any::<i64>()
            .prop_map(|v| Expr {
                expr_instance: Some(expr::ExprInstance::GInt(v)),
            })
            .boxed(),
        any::<String>()
            .prop_map(|v| Expr {
                expr_instance: Some(expr::ExprInstance::GString(v)),
            })
            .boxed(),
        generate_par(depth - 1)
            .prop_map(|p| Expr {
                expr_instance: Some(expr::ExprInstance::ENotBody(ENot { p: Some(p) })),
            })
            .boxed(),
    ])
    .boxed()
}

pub fn generate_match(depth: usize) -> BoxedStrategy<Match> {
    if depth == 0 {
        return Just(Match {
            target: Some(
                generate_par(0)
                    .boxed()
                    .new_tree(&mut Default::default())
                    .unwrap()
                    .current(),
            ),
            cases: vec![],
            locally_free: vec![],
            connective_used: false,
        })
        .boxed();
    }

    (
        generate_par(depth - 1),
        proptest::collection::vec(
            (
                generate_par(depth - 1),
                generate_par(depth - 1),
                any::<i32>(),
            )
                .prop_map(|(pattern, source, free_count)| MatchCase {
                    pattern: Some(pattern),
                    source: Some(source),
                    free_count,
                    guard: None,
                }),
            0..=2,
        ),
        proptest::collection::vec(any::<u8>(), 0..=2),
        any::<bool>(),
    )
        .prop_map(|(target, cases, locally_free, connective_used)| Match {
            target: Some(target),
            cases,
            locally_free,
            connective_used,
        })
        .boxed()
}

pub fn generate_bundle(depth: usize) -> BoxedStrategy<Bundle> {
    if depth == 0 {
        return Just(Bundle {
            body: Some(
                generate_par(0)
                    .boxed()
                    .new_tree(&mut Default::default())
                    .unwrap()
                    .current(),
            ),
            write_flag: false,
            read_flag: false,
        })
        .boxed();
    }

    (generate_par(depth - 1), any::<bool>(), any::<bool>())
        .prop_map(|(body, write_flag, read_flag)| Bundle {
            body: Some(body),
            write_flag,
            read_flag,
        })
        .boxed()
}

pub fn generate_connective(depth: usize) -> BoxedStrategy<Connective> {
    if depth == 0 {
        return Just(Connective {
            connective_instance: None,
        })
        .boxed();
    }

    proptest::strategy::Union::new(vec![
        generate_par(depth - 1)
            .prop_map(|p| Connective {
                connective_instance: Some(connective::ConnectiveInstance::ConnNotBody(p)),
            })
            .boxed(),
        any::<bool>()
            .prop_map(|v| Connective {
                connective_instance: Some(connective::ConnectiveInstance::ConnBool(v)),
            })
            .boxed(),
    ])
    .boxed()
}

pub fn generate_var(depth: usize) -> BoxedStrategy<Var> {
    if depth == 0 {
        return Just(Var { var_instance: None }).boxed();
    }

    proptest::strategy::Union::new(vec![
        any::<i32>()
            .prop_map(|v| Var {
                var_instance: Some(var::VarInstance::BoundVar(v)),
            })
            .boxed(),
        any::<i32>()
            .prop_map(|v| Var {
                var_instance: Some(var::VarInstance::FreeVar(v)),
            })
            .boxed(),
        Just(Var {
            var_instance: Some(var::VarInstance::Wildcard(var::WildcardMsg {})),
        })
        .boxed(),
    ])
    .boxed()
}

pub fn generate_option_var(depth: usize) -> BoxedStrategy<Option<Var>> {
    if depth == 0 {
        return Just(None).boxed();
    }

    proptest::option::of(generate_var(depth)).boxed()
}

#[cfg(test)]
mod anti_vacuity {
    //! Guards for the defect documented at the top of this file. Each test
    //! names one shape the generators are supposed to reach; if a size range
    //! regresses to an exclusive `0..1`, the corresponding guard fails
    //! immediately rather than silently emptying every downstream property.

    use super::*;

    const DRAWS: usize = 256;

    #[test]
    fn generate_par_reaches_a_non_empty_expr_slot() {
        assert_generator_not_vacuous(
            "a Par with at least one Expr",
            generate_par(3),
            DRAWS,
            |p| !p.exprs.is_empty(),
        );
    }

    #[test]
    fn generate_par_reaches_every_par_bearing_slot() {
        let slots: [(&str, fn(&Par) -> bool); 7] = [
            ("a Par with a Send", |p| !p.sends.is_empty()),
            ("a Par with a Receive", |p| !p.receives.is_empty()),
            ("a Par with a New", |p| !p.news.is_empty()),
            ("a Par with an Expr", |p| !p.exprs.is_empty()),
            ("a Par with a Match", |p| !p.matches.is_empty()),
            ("a Par with a Bundle", |p| !p.bundles.is_empty()),
            ("a Par with a Connective", |p| !p.connectives.is_empty()),
        ];
        for (what, predicate) in slots {
            assert_generator_not_vacuous(what, generate_par(3), DRAWS, predicate);
        }
    }

    #[test]
    fn generate_par_reaches_a_nested_par() {
        // Depth is what makes the family interesting; a generator that only
        // ever yields leaves cannot exercise a recursive traversal at all.
        assert_generator_not_vacuous(
            "a Par nested at least two levels deep",
            generate_par(3),
            DRAWS,
            |p| {
                use crate::rust::rholang::par_children::par_child_pars;
                let mut level1: Vec<&Par> = Vec::new();
                par_child_pars(p, &mut level1);
                level1.iter().any(|c| {
                    let mut level2: Vec<&Par> = Vec::new();
                    par_child_pars(c, &mut level2);
                    !level2.is_empty()
                })
            },
        );
    }

    #[test]
    fn generate_send_and_receive_carry_payloads() {
        assert_generator_not_vacuous("a Send with data", generate_send(3).boxed(), DRAWS, |s| {
            !s.data.is_empty()
        });
        assert_generator_not_vacuous(
            "a Receive with binds",
            generate_receive(3).boxed(),
            DRAWS,
            |r| !r.binds.is_empty(),
        );
    }

    #[test]
    fn generate_new_carries_injections() {
        assert_generator_not_vacuous(
            "a New with injections",
            generate_new(3).boxed(),
            DRAWS,
            |n| !n.injections.is_empty(),
        );
    }

    #[test]
    fn generate_match_carries_cases() {
        assert_generator_not_vacuous(
            "a Match with cases",
            generate_match(3).boxed(),
            DRAWS,
            |m| !m.cases.is_empty(),
        );
    }
}
