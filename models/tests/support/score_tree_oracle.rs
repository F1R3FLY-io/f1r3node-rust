//! # Recursive score-tree oracle and differential
//!
//! Kept physically under `models/tests/support` and compiled only through the
//! `#[cfg(test)]` module declaration in the production comparator module.

use super::*;

// ===========================================================================
// The recursive oracle twin, and the differential
// ===========================================================================

/// The pre-conversion recursive comparator, retained verbatim as the oracle.
///
/// It is `#[cfg(test)]` and Θ(depth)/Θ(width) on purpose: it is the reference
/// the explicit-worklist form is compared against, exactly as `reduce.rs` keeps
/// `eval_expr_recursive` beside its trampoline (commit `a929a2d6`).
#[cfg(test)]
pub(crate) fn compare_score_recursive(
    s1: &Tree<ScoreAtom>,
    s2: &Tree<ScoreAtom>,
) -> std::cmp::Ordering {
    fn compare_score_nodes(
        left: &[Tree<ScoreAtom>],
        right: &[Tree<ScoreAtom>],
    ) -> std::cmp::Ordering {
        match (left.first(), right.first()) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (Some(left_head), Some(right_head)) => {
                let result = compare_score_recursive(left_head, right_head);
                if result == std::cmp::Ordering::Equal {
                    compare_score_nodes(&left[1..], &right[1..])
                } else {
                    result
                }
            }
        }
    }

    match (s1, s2) {
        (Tree::Leaf(a), Tree::Leaf(b)) => a.compare(b).cmp(&0),
        (Tree::Leaf(_), Tree::Node(_)) => std::cmp::Ordering::Less,
        (Tree::Node(_), Tree::Leaf(_)) => std::cmp::Ordering::Greater,
        (Tree::Node(a), Tree::Node(b)) => match (a.is_empty(), b.is_empty()) {
            (true, true) => std::cmp::Ordering::Equal,
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => compare_score_nodes(a.as_slice(), b.as_slice()),
        },
    }
}

#[cfg(test)]
mod differential_score_comparator {
    //! ## The obligation, and why it is stricter here than elsewhere
    //!
    //! `ParSortMatcher` produces the CANONICAL FORM, and
    //! `cost_accounting/sig.rs` signs `sort_match(&par).term.encode_to_vec()`.
    //! A reordering of one element is a fork. "The sorter takes no metering
    //! handle, so nothing can charge" makes the *cost* obligation vacuous; it
    //! makes the *result* obligation the whole obligation.
    //!
    //! So this module asserts, over a corpus that reaches both the depth and
    //! the width axis:
    //!
    //! 1. **Agreement** with the recursive oracle, for every ordered pair.
    //! 2. **Reflexivity** — `cmp(x, x) == Equal`.
    //! 3. **Antisymmetry** — `cmp(x, y) == cmp(y, x).reverse()`.
    //! 4. **Transitivity** — `x ≤ y ∧ y ≤ z ⟹ x ≤ z`, over every ordered
    //!    triple.
    //! 5. **Result equality on multi-sibling terms** — `sort_vec` produces the
    //!    same permutation as an oracle-driven stable sort, which is the
    //!    property `sig.rs` actually depends on.

    use super::*;

    fn leaf(n: i64) -> Tree<ScoreAtom> { Tree::Leaf(ScoreAtom::create_from_i64(n)) }
    fn sleaf(s: &str) -> Tree<ScoreAtom> {
        Tree::Leaf(ScoreAtom::create_from_string(s.to_string()))
    }
    fn bleaf(b: &[u8]) -> Tree<ScoreAtom> { Tree::Leaf(ScoreAtom::create_from_bytes(b.to_vec())) }

    /// A corpus that reaches every arm of the comparator: leaves of all three
    /// tagged-atom kinds, empty and non-empty nodes, leaf-vs-node mismatches,
    /// differing widths, and chains deep enough to matter.
    fn corpus() -> Vec<Tree<ScoreAtom>> {
        let chain = |depth: usize, leaf_value: i64| {
            let mut t = leaf(leaf_value);
            for _ in 0..depth {
                t = Tree::Node(vec![leaf(0), t]);
            }
            t
        };
        let wide = |width: usize, last: i64| {
            let mut children: Vec<Tree<ScoreAtom>> = (0..width as i64).map(leaf).collect();
            if let Some(slot) = children.last_mut() {
                *slot = leaf(last);
            }
            Tree::Node(children)
        };

        vec![
            leaf(i64::MIN),
            leaf(-1),
            leaf(0),
            leaf(1),
            leaf(i64::MAX),
            sleaf(""),
            sleaf("a"),
            sleaf("b"),
            bleaf(&[]),
            bleaf(&[0x80]),
            bleaf(&[0xd9]),
            bleaf(&[0x80, 0x00]),
            Tree::Node(vec![]),
            Tree::Node(vec![leaf(1)]),
            Tree::Node(vec![leaf(1), leaf(2)]),
            Tree::Node(vec![leaf(1), leaf(2), leaf(3)]),
            Tree::Node(vec![leaf(2), leaf(1)]),
            Tree::Node(vec![Tree::Node(vec![]), leaf(1)]),
            Tree::Node(vec![leaf(1), Tree::Node(vec![leaf(1)])]),
            Tree::Node(vec![sleaf("a"), leaf(1)]),
            chain(1, 7),
            chain(2, 7),
            chain(8, 7),
            chain(8, 8),
            chain(9, 7),
            wide(4, 0),
            wide(4, 99),
            wide(16, 15),
            wide(16, 99),
        ]
    }

    #[test]
    fn the_iterative_comparator_agrees_with_the_recursive_oracle() {
        let corpus = corpus();
        for x in &corpus {
            for y in &corpus {
                assert_eq!(
                    compare_score(x, y),
                    compare_score_recursive(x, y),
                    "COMPARATOR DIVERGENCE — this decides the canonical form, and the \
                     canonical form is what gets signed.\n  left  = {:?}\n  right = {:?}",
                    x,
                    y
                );
            }
        }
    }

    #[test]
    fn the_comparator_is_a_total_order() {
        use std::cmp::Ordering;
        let corpus = corpus();

        for x in &corpus {
            assert_eq!(
                compare_score(x, x),
                Ordering::Equal,
                "not reflexive: {:?}",
                x
            );
        }

        for x in &corpus {
            for y in &corpus {
                assert_eq!(
                    compare_score(x, y),
                    compare_score(y, x).reverse(),
                    "not antisymmetric:\n  x = {:?}\n  y = {:?}",
                    x,
                    y
                );
            }
        }

        for x in &corpus {
            for y in &corpus {
                if compare_score(x, y) == Ordering::Greater {
                    continue;
                }
                for z in &corpus {
                    if compare_score(y, z) == Ordering::Greater {
                        continue;
                    }
                    assert_ne!(
                        compare_score(x, z),
                        Ordering::Greater,
                        "not transitive:\n  x = {:?}\n  y = {:?}\n  z = {:?}",
                        x,
                        y,
                        z
                    );
                }
            }
        }
    }

    /// The property `sig.rs` actually depends on: the PERMUTATION `sort_vec`
    /// produces must be the one an oracle-driven stable sort produces.
    #[test]
    fn sort_vec_produces_the_oracle_permutation() {
        let corpus = corpus();
        // Tag each element so the permutation itself is observable, not merely
        // the multiset of scores.
        let mut actual: Vec<ScoredTerm<usize>> = corpus
            .iter()
            .enumerate()
            .map(|(i, score)| ScoredTerm {
                term: i,
                score: score.clone(),
            })
            .collect();
        let mut expected = actual.clone();

        ScoredTerm::sort_vec(&mut actual);
        expected.sort_by(|a, b| compare_score_recursive(&a.score, &b.score));

        let actual_order: Vec<usize> = actual.iter().map(|s| s.term).collect();
        let expected_order: Vec<usize> = expected.iter().map(|s| s.term).collect();
        assert_eq!(
            actual_order, expected_order,
            "sort_vec produced a different PERMUTATION from the oracle. The canonical \
             form is signed; a one-element reordering is a consensus fork."
        );
    }

    /// `Clone`, `PartialEq` and `Drop` are hand-written. `Clone` and `PartialEq`
    /// must be structurally identical to the derives they replaced.
    #[test]
    fn derived_and_iterative_agree() {
        let corpus = corpus();
        for x in &corpus {
            let copy = x.clone();
            assert!(
                copy == *x,
                "the iterative Clone did not reproduce its input: {:?}",
                x
            );
            for y in &corpus {
                // Structural equality must agree with a straightforward
                // recursive definition on every pair, including mismatched
                // shapes and mismatched widths.
                fn eq_recursive(a: &Tree<ScoreAtom>, b: &Tree<ScoreAtom>) -> bool {
                    match (a, b) {
                        (Tree::Leaf(x), Tree::Leaf(y)) => x == y,
                        (Tree::Node(xs), Tree::Node(ys)) => {
                            xs.len() == ys.len()
                                && xs.iter().zip(ys.iter()).all(|(p, q)| eq_recursive(p, q))
                        }
                        _ => false,
                    }
                }
                assert_eq!(
                    x == y,
                    eq_recursive(x, y),
                    "the iterative PartialEq diverged:\n  x = {:?}\n  y = {:?}",
                    x,
                    y
                );
            }
        }
    }

    /// The three hand-written impls have to survive a tree far deeper than any
    /// recursive form would, on this test thread's ordinary stack.
    #[test]
    fn clone_eq_and_drop_are_depth_independent_in_practice() {
        // 20,000 levels: at the derived `Clone`'s measured 1,578 B/level that
        // would need ~30 MiB, and at `drop_in_place`'s 370 B/level ~7 MiB.
        let mut deep = leaf(1);
        for _ in 0..20_000 {
            deep = Tree::Node(vec![deep]);
        }
        let copy = deep.clone();
        assert!(copy == deep);
        drop(copy);
        drop(deep);
    }
}
