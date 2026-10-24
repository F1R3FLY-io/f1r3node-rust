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

#[derive(Debug)]
pub enum Tree<T> {
    Leaf(T),
    Node(Vec<Tree<T>>),
}

impl<T: Clone> Clone for Tree<T> {
    fn clone(&self) -> Self {
        enum Step<'a, T> {
            Visit(&'a Tree<T>),
            Build(usize),
        }

        let mut work: Vec<Step<'_, T>> = vec![Step::Visit(self)];
        let mut done: Vec<Tree<T>> = Vec::new();

        while let Some(step) = work.pop() {
            match step {
                Step::Visit(Tree::Leaf(value)) => done.push(Tree::Leaf(value.clone())),
                Step::Visit(Tree::Node(children)) => {
                    work.push(Step::Build(children.len()));
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
                    for pair in xs.iter().zip(ys.iter()).rev() {
                        work.push(pair);
                    }
                }
                _ => return false,
            }
        }
        true
    }
}

impl<T> Drop for Tree<T> {
    fn drop(&mut self) {
        let mut work: Vec<Tree<T>> = Vec::new();
        if let Tree::Node(children) = self {
            work.append(children);
        }
        while let Some(mut node) = work.pop() {
            if let Tree::Node(children) = &mut node {
                work.append(children);
            }
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

#[derive(Clone, Debug, PartialEq)]
pub struct ScoredTerm<T> {
    pub term: T,
    pub score: Tree<ScoreAtom>,
}

enum ScoreCmp<'a> {
    Pair(&'a Tree<ScoreAtom>, &'a Tree<ScoreAtom>),
    Nodes(&'a [Tree<ScoreAtom>], &'a [Tree<ScoreAtom>]),
}

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
    pub fn sort_vec(scored_terms: &mut Vec<ScoredTerm<T>>) {
        scored_terms.sort_by(|s1, s2| compare_score(&s1.score, &s2.score));
    }
}

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
    pub const AUTHORITY_ID: i32 = 13;
    pub const PRINCIPAL_ID: i32 = 14;
    pub const EPATHMAP: i32 = 13;
    pub const DOUBLE: i32 = 14;
    pub const BIG_INT: i32 = 15;
    pub const BIG_RAT: i32 = 16;
    pub const FIXED_POINT: i32 = 17;
    pub const UINT64: i32 = 18;
    pub const INT32: i32 = 20;
    pub const UINT32: i32 = 21;
    pub const UINT16: i32 = 22;
    pub const UINT8: i32 = 23;
    pub const FLOAT32: i32 = 19;

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
    pub const COST_SIG_GROUND: i32 = 310;
    pub const COST_SIG_BOUND: i32 = 311;
    pub const COST_SIG_QUOTE: i32 = 312;
    pub const COST_SIG_COMPOUND: i32 = 313;
    pub const COST_SIGNED_TERM: i32 = 314;
    pub const COST_STACK: i32 = 315;
    pub const COST_SIG_NAME: i32 = 316;
    pub const COST_SIG_UNIT: i32 = 317;

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

    use super::*;

    fn leaf(n: i64) -> Tree<ScoreAtom> { Tree::Leaf(ScoreAtom::create_from_i64(n)) }
    fn sleaf(s: &str) -> Tree<ScoreAtom> {
        Tree::Leaf(ScoreAtom::create_from_string(s.to_string()))
    }
    fn bleaf(b: &[u8]) -> Tree<ScoreAtom> { Tree::Leaf(ScoreAtom::create_from_bytes(b.to_vec())) }

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

    #[test]
    fn sort_vec_produces_the_oracle_permutation() {
        let corpus = corpus();
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

    #[test]
    fn clone_eq_and_drop_are_depth_independent_in_practice() {
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
