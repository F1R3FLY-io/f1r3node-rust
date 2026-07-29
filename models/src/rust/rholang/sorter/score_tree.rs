// See models/src/main/scala/coop/rchain/models/rholang/sorter/ScoreTree.scala

use shared::rust::ByteString;

/**
 * Sorts the insides of the Par and ESet/EMap of the rholangADT
 *
 * A score tree is recursively built for each term and is used to sort the insides of Par/ESet/EMap.
 * For most terms, the current term type's absolute value based on the Score object is added as a Leaf
 * to the left most branch and the score tree built for the inside terms are added to the right.
 * The Score object is a container of constants that arbitrarily assigns absolute values to term types.
 * The sort order is total as every term type is assigned an unique value in the Score object.
 * For ground types, the appropriate integer representation is used as the base score tree.
 * For var types, the Debruijn level from the normalization is used.
 *
 * In order to sort an term, call [Type]SortMatcher.sortMatch(term)
 * and extract the .term  of the returned ScoredTerm.
 *
 * NOTE: PartialEq is needed for testing purposes
 */
pub struct ScoreTree;

/// A score tree.
///
/// ## ⚠ Why `Clone`, `PartialEq` and `Drop` are hand-written here
///
/// `Tree<T>` is a **recursive Rust type that is not a proto message**. The
/// Θ(depth) audit's enumeration was a Tarjan SCC over `RhoTypes.proto`
/// (`docs/design/audits/theta-depth-traversals-2026-07-26.md` §3), and that
/// method structurally cannot see this type — yet its depth is proportional to
/// term depth and it carried four Θ traversals: the two comparators below,
/// derived `Clone`, and the compiler-synthesised `drop_in_place`.
///
/// Measured on 2026-07-26, bytes of native stack per nesting level (debug /
/// release): `compare_score` 1,329 / 128, derived `Clone` 1,578 / 485,
/// `drop_in_place` 370 / 204, derived `PartialEq` 719 / —. Small constants, but
/// a non-zero slope is a non-zero slope: they are program-controlled, and after
/// the substitution SCC and the sorter are converted they are what is left.
///
/// `Clone`, `PartialEq` and `Drop` are therefore explicit worklists. Each is
/// **structurally identical** to the derive it replaces — same shape, same
/// per-leaf `T` operation, same short-circuit — and
/// `derived_and_iterative_agree` is the differential.
///
/// `Debug` is deliberately still derived and still Θ(depth) (row 6 of the
/// audit's table; disposition §7.2 "derived — Leg-1 only"). It is reachable
/// only from assertion messages, never from the reduce path and never from
/// untrusted input, and reproducing the derive's exact output format by hand
/// would trade a real risk of format drift for no liveness gain. It is named in
/// `stack_depth_gate.rs`'s tripwire rather than left unmentioned.
#[derive(Debug)]
pub enum Tree<T> {
    Leaf(T),
    Node(Vec<Tree<T>>),
}

impl<T: Clone> Clone for Tree<T> {
    /// Post-order rebuild on an explicit stack — structurally identical to the
    /// derived `Clone` it replaces, and `O(1)` in native stack.
    fn clone(&self) -> Self {
        enum Step<'a, T> {
            Visit(&'a Tree<T>),
            /// Pop `n` finished children and wrap them in a `Node`.
            Build(usize),
        }

        let mut work: Vec<Step<'_, T>> = vec![Step::Visit(self)];
        let mut done: Vec<Tree<T>> = Vec::new();

        while let Some(step) = work.pop() {
            match step {
                Step::Visit(Tree::Leaf(value)) => done.push(Tree::Leaf(value.clone())),
                Step::Visit(Tree::Node(children)) => {
                    work.push(Step::Build(children.len()));
                    // Reversed, so children are cloned left-to-right.
                    for child in children.iter().rev() {
                        work.push(Step::Visit(child));
                    }
                }
                Step::Build(n) => {
                    let at = done.len() - n;
                    let children = done.split_off(at);
                    done.push(Tree::Node(children));
                }
            }
        }

        done.pop()
            .expect("Tree::clone: exactly one value must remain")
    }
}

impl<T: PartialEq> PartialEq for Tree<T> {
    /// Structural equality on an explicit stack. Short-circuits on the first
    /// mismatch in the same left-to-right, depth-first order the derived
    /// implementation used.
    ///
    /// ⚠ **No catch-all arm, deliberately.** This match used to end in
    /// `_ => return false`, which made it exhaustive to the compiler and so
    /// disabled the only check that a third `Tree` variant would need. Such a
    /// variant would have compiled, fallen into the catch-all, and compared
    /// **unequal to itself** — and `Tree` is the sorter's score carrier, so a
    /// broken reflexivity here reaches canonical ordering. `Tree` has two
    /// variants, so the four cross-pairs are written out in full: a third
    /// variant leaves pairs uncovered and the build stops with E0004. See the
    /// banner in `models/src/lib.rs` for the five sibling instances of this
    /// shape and `models/tests/variant_exhaustiveness_gate.rs` for the gate.
    fn eq(&self, other: &Self) -> bool {
        let mut work: Vec<(&Tree<T>, &Tree<T>)> = vec![(self, other)];
        while let Some((a, b)) = work.pop() {
            match (a, b) {
                (Tree::Leaf(x), Tree::Leaf(y)) => {
                    if x != y {
                        return false;
                    }
                }
                (Tree::Node(xs), Tree::Node(ys)) => {
                    if xs.len() != ys.len() {
                        return false;
                    }
                    // Reversed, so pairs are compared left-to-right.
                    for pair in xs.iter().zip(ys.iter()).rev() {
                        work.push(pair);
                    }
                }
                (Tree::Leaf(_), Tree::Node(_)) | (Tree::Node(_), Tree::Leaf(_)) => return false,
            }
        }
        true
    }
}

impl<T> Drop for Tree<T> {
    /// Iterative teardown.
    ///
    /// The compiler-synthesised `drop_in_place` recurses with the tree, so a
    /// deep score tree could abort the process while being released — after the
    /// traversal that built it had already succeeded. Detaching each node's
    /// children before its shell goes out of scope keeps native stack `O(1)`:
    /// every shell this loop drops is already child-free, so its own `drop`
    /// appends nothing and returns immediately.
    fn drop(&mut self) {
        let mut work: Vec<Tree<T>> = Vec::new();
        if let Tree::Node(children) = self {
            work.append(children);
        }
        while let Some(mut node) = work.pop() {
            if let Tree::Node(children) = &mut node {
                work.append(children);
            }
            // `node` is now child-free: dropping it here is O(1).
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TaggedAtom {
    IntAtom(i64),
    StringAtom(String),
    BytesAtom(ByteString),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScoreAtom {
    value: TaggedAtom,
}

impl ScoreAtom {
    fn bs_compare(&self, b1: &ByteString, b2: &ByteString) -> i32 {
        let mut it1 = b1.iter();
        let mut it2 = b2.iter();

        loop {
            match (it1.next(), it2.next()) {
                (Some(&byte1), Some(&byte2)) => {
                    let comp = byte1.cmp(&byte2);

                    if comp != std::cmp::Ordering::Equal {
                        return comp as i32;
                    }
                }

                (Some(_), None) => return 1,
                (None, Some(_)) => return -1,
                (None, None) => return 0,
            }
        }
    }

    pub fn compare(&self, that: &ScoreAtom) -> i32 {
        match (&self.value, &that.value) {
            (TaggedAtom::IntAtom(i1), TaggedAtom::IntAtom(i2)) => i1.cmp(i2) as i32,

            (TaggedAtom::IntAtom(_), _) => -1,

            (_, TaggedAtom::IntAtom(_)) => 1,

            (TaggedAtom::StringAtom(s1), TaggedAtom::StringAtom(s2)) => s1.cmp(s2) as i32,

            (TaggedAtom::StringAtom(_), _) => -1,

            (_, TaggedAtom::StringAtom(_)) => 1,

            (TaggedAtom::BytesAtom(b1), TaggedAtom::BytesAtom(b2)) => self.bs_compare(b1, b2),
        }
    }

    pub fn create_from_i64(value: i64) -> ScoreAtom {
        ScoreAtom {
            value: TaggedAtom::IntAtom(value),
        }
    }

    pub fn create_from_string(value: String) -> ScoreAtom {
        ScoreAtom {
            value: TaggedAtom::StringAtom(value),
        }
    }

    pub fn create_from_bytes(value: ByteString) -> ScoreAtom {
        ScoreAtom {
            value: TaggedAtom::BytesAtom(value),
        }
    }
}

impl<T> Tree<T> {
    pub fn create_leaf_from_i64(item: i64) -> Tree<ScoreAtom> {
        Tree::Leaf(ScoreAtom::create_from_i64(item))
    }

    pub fn create_leaf_from_string(item: String) -> Tree<ScoreAtom> {
        Tree::Leaf(ScoreAtom::create_from_string(item))
    }

    pub fn create_leaf_from_bytes(item: ByteString) -> Tree<ScoreAtom> {
        Tree::Leaf(ScoreAtom::create_from_bytes(item))
    }

    pub fn create_node_from_i64s(children: Vec<i64>) -> Tree<ScoreAtom> {
        Tree::Node(
            children
                .iter()
                .map(|item: &i64| Tree::<ScoreAtom>::create_leaf_from_i64(*item))
                .collect(),
        )
    }

    pub fn create_node_from_i32(left: i32, right: Vec<Tree<ScoreAtom>>) -> Tree<ScoreAtom> {
        let mut new_tree = vec![Tree::<ScoreAtom>::create_leaf_from_i64(left as i64)];
        new_tree.extend(right);
        Tree::Node(new_tree)
    }

    pub fn create_node_from_string(left: String, right: Vec<Tree<ScoreAtom>>) -> Tree<ScoreAtom> {
        let mut new_tree = vec![Tree::<ScoreAtom>::create_leaf_from_string(left)];
        new_tree.extend(right);
        Tree::Node(new_tree)
    }
}

// Effectively a tuple that groups the term to its score tree.
#[derive(Clone, Debug, PartialEq)]
pub struct ScoredTerm<T> {
    pub term: T,
    pub score: Tree<ScoreAtom>,
}

/// One pending comparison. `Pair` is `compare_score`'s subject; `Nodes` is
/// `compare_score_nodes`'s — two sibling SLICES walked head-first.
enum ScoreCmp<'a> {
    Pair(&'a Tree<ScoreAtom>, &'a Tree<ScoreAtom>),
    Nodes(&'a [Tree<ScoreAtom>], &'a [Tree<ScoreAtom>]),
}

/// The score-tree comparator, as an explicit worklist.
///
/// ## ⚠ This decides the CANONICAL FORM, and the canonical form is signed
///
/// `cost_accounting/sig.rs` computes `sort_match(&par).term.encode_to_vec()` —
/// the bytes that get signed. A one-element reordering here is a consensus
/// fork, not a performance regression. The obligation on this conversion is
/// therefore *stricter* than "no panic": it is a total-order differential
/// (reflexivity, antisymmetry, transitivity, agreement with the recursive
/// oracle) **plus** result equality on multi-sibling terms. See
/// `differential_score_comparator` at the bottom of this file.
///
/// ## Two axes were fixed here, not one
///
/// * **Depth** — `compare_score` recursed into `compare_score_nodes` and back;
///   measured 1,329 B/level (debug).
/// * **Width** — `compare_score_nodes` recursed on the list TAIL
///   (`&left[1..]`), so its native stack grew with SIBLING COUNT: 201 B per
///   sibling (debug). That axis does not exist in the proto-message SCC the
///   audit enumerated, and no gate looked for it.
///
///   ⚠ At `-O2` LLVM turns that tail call into a loop and the measured width
///   slope is 0. That is a codegen accident, not a property — nothing obliges
///   any backend to keep doing it, and it does not hold at `-O0`. An explicit
///   loop makes the property hold *by construction* in both profiles.
///
/// ## The visitation order is the recursion's, exactly
///
/// The recursive form returns the FIRST non-`Equal` result of a left-to-right,
/// depth-first walk of the pair tree, and `Equal` if every comparison is
/// `Equal`. This loop pushes the TAIL pair before the HEAD pair, so LIFO pops
/// the head first and any sub-work the head generates sits above the tail —
/// which is precisely `if head == Equal { recurse on the tail }`.
pub fn compare_score(s1: &Tree<ScoreAtom>, s2: &Tree<ScoreAtom>) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let mut work: Vec<ScoreCmp<'_>> = Vec::with_capacity(32);
    work.push(ScoreCmp::Pair(s1, s2));

    while let Some(cmp) = work.pop() {
        match cmp {
            ScoreCmp::Pair(a, b) => match (a, b) {
                (Tree::Leaf(x), Tree::Leaf(y)) => {
                    let result = x.compare(y).cmp(&0);
                    if result != Ordering::Equal {
                        return result;
                    }
                }
                (Tree::Leaf(_), Tree::Node(_)) => return Ordering::Less,
                (Tree::Node(_), Tree::Leaf(_)) => return Ordering::Greater,
                (Tree::Node(x), Tree::Node(y)) => match (x.is_empty(), y.is_empty()) {
                    (true, true) => {}
                    (true, false) => return Ordering::Less,
                    (false, true) => return Ordering::Greater,
                    (false, false) => work.push(ScoreCmp::Nodes(x.as_slice(), y.as_slice())),
                },
            },
            ScoreCmp::Nodes(left, right) => match (left.first(), right.first()) {
                (None, None) => {}
                (None, Some(_)) => return Ordering::Less,
                (Some(_), None) => return Ordering::Greater,
                (Some(left_head), Some(right_head)) => {
                    work.push(ScoreCmp::Nodes(&left[1..], &right[1..]));
                    work.push(ScoreCmp::Pair(left_head, right_head));
                }
            },
        }
    }

    Ordering::Equal
}

impl<T: Clone> ScoredTerm<T> {
    /// Sort siblings into canonical order.
    ///
    /// ⚠ `sort_by`, never `sort_unstable_by`. This sort is **stable**, and the
    /// comparator returns `Equal` for distinct terms with equal scores (the
    /// sorter is a normalizer, so it is not injective — directly measured:
    /// `Par{exprs:[GInt 1, GInt 2]}` and `Par{exprs:[GInt 2, GInt 1]}` are
    /// unequal terms with equal scores). An unstable sort would be free to
    /// reorder those, which would fork the canonical form and therefore the
    /// signed bytes.
    pub fn sort_vec(scored_terms: &mut Vec<ScoredTerm<T>>) {
        scored_terms.sort_by(|s1, s2| compare_score(&s1.score, &s2.score));
    }
}

/**
 * Total order of all terms
 *
 * The general order is ground, vars, arithmetic, comparisons, logical, and then others
 */
pub struct Score;

impl Score {
    // For things that are truly optional
    pub const ABSENT: i32 = 0;

    // Ground types
    pub const BOOL: i32 = 1;
    pub const INT: i32 = 2;
    pub const STRING: i32 = 3;
    pub const URI: i32 = 4;
    pub const PRIVATE: i32 = 5;
    pub const ELIST: i32 = 6;
    pub const ETUPLE: i32 = 7;
    pub const ESET: i32 = 8;
    pub const EMAP: i32 = 9;
    pub const DEPLOYER_AUTH: i32 = 10;
    pub const DEPLOY_ID: i32 = 11;
    pub const SYS_AUTH_TOKEN: i32 = 12;
    pub const EPATHMAP: i32 = 13;
    pub const DOUBLE: i32 = 14;
    pub const BIG_INT: i32 = 15;
    pub const BIG_RAT: i32 = 16;
    pub const FIXED_POINT: i32 = 17;

    // Vars
    pub const BOUND_VAR: i32 = 50;
    pub const FREE_VAR: i32 = 51;
    pub const WILDCARD: i32 = 52;
    pub const REMAINDER: i32 = 53;

    // Expr
    pub const EVAR: i32 = 100;
    pub const ENEG: i32 = 101;
    pub const EMULT: i32 = 102;
    pub const EDIV: i32 = 103;
    pub const EPLUS: i32 = 104;
    pub const EMINUS: i32 = 105;
    pub const ELT: i32 = 106;
    pub const ELTE: i32 = 107;
    pub const EGT: i32 = 108;
    pub const EGTE: i32 = 109;
    pub const EEQ: i32 = 110;
    pub const ENEQ: i32 = 111;
    pub const ENOT: i32 = 112;
    pub const EAND: i32 = 113;
    pub const EOR: i32 = 114;
    pub const EMETHOD: i32 = 115;
    pub const EBYTEARR: i32 = 116;
    pub const EEVAL: i32 = 117;
    pub const EMATCHES: i32 = 118;
    pub const EPERCENT: i32 = 119;
    pub const EPLUSPLUS: i32 = 120;
    pub const EMINUSMINUS: i32 = 121;
    pub const EMOD: i32 = 122;

    // Other
    pub const QUOTE: i32 = 203;
    pub const CHAN_VAR: i32 = 204;

    pub const SEND: i32 = 300;
    pub const RECEIVE: i32 = 301;
    pub const NEW: i32 = 303;
    pub const MATCH: i32 = 304;
    pub const BUNDLE_EQUIV: i32 = 305;
    pub const BUNDLE_READ: i32 = 306;
    pub const BUNDLE_WRITE: i32 = 307;
    pub const BUNDLE_READ_WRITE: i32 = 308;
    pub const IF: i32 = 309;

    pub const CONNECTIVE_NOT: i32 = 400;
    pub const CONNECTIVE_AND: i32 = 401;
    pub const CONNECTIVE_OR: i32 = 402;
    pub const CONNECTIVE_VARREF: i32 = 403;
    pub const CONNECTIVE_BOOL: i32 = 404;
    pub const CONNECTIVE_INT: i32 = 405;
    pub const CONNECTIVE_STRING: i32 = 406;
    pub const CONNECTIVE_URI: i32 = 407;
    pub const CONNECTIVE_BYTEARRAY: i32 = 408;

    pub const PAR: i32 = 999;
}

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

    fn leaf(n: i64) -> Tree<ScoreAtom> {
        Tree::Leaf(ScoreAtom::create_from_i64(n))
    }
    fn sleaf(s: &str) -> Tree<ScoreAtom> {
        Tree::Leaf(ScoreAtom::create_from_string(s.to_string()))
    }
    fn bleaf(b: &[u8]) -> Tree<ScoreAtom> {
        Tree::Leaf(ScoreAtom::create_from_bytes(b.to_vec()))
    }

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
            assert_eq!(compare_score(x, x), Ordering::Equal, "not reflexive: {:?}", x);
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
