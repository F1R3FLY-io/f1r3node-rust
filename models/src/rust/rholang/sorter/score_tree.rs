// See models/src/main/scala/coop/rchain/models/rholang/sorter/ScoreTree.scala

use std::cmp::Ordering;
use std::mem::size_of;

use shared::rust::clone_backing::BackingError;
use shared::rust::ByteString;

use super::metered::SorterMeter;

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

#[derive(Clone, Debug, PartialEq)]
pub enum Tree<T> {
    Leaf(T),
    Node(Vec<Tree<T>>),
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
    pub fn create_node_from_i64s_metered(
        children: &[i64],
        meter: &SorterMeter<'_>,
    ) -> Result<Tree<ScoreAtom>, BackingError> {
        let mut result = meter.vec(children.len())?;
        for value in children {
            result.push(Tree::<ScoreAtom>::create_leaf_from_i64(*value));
        }
        Ok(Tree::Node(result))
    }

    pub fn create_node_from_i32_metered(
        left: i32,
        right: Vec<Tree<ScoreAtom>>,
        meter: &SorterMeter<'_>,
    ) -> Result<Tree<ScoreAtom>, BackingError> {
        let mut result = meter.vec(right.len().checked_add(1).ok_or(BackingError::Overflow)?)?;
        result.push(Tree::<ScoreAtom>::create_leaf_from_i64(i64::from(left)));
        result.extend(right);
        Ok(Tree::Node(result))
    }
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

impl<T: Clone> ScoredTerm<T> {
    pub fn sort_vec(scored_terms: &mut Vec<ScoredTerm<T>>) {
        fn compare_score_nodes(
            left: &[Tree<ScoreAtom>],
            right: &[Tree<ScoreAtom>],
        ) -> std::cmp::Ordering {
            match (left.first(), right.first()) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (Some(_), None) => std::cmp::Ordering::Greater,
                (Some(left_head), Some(right_head)) => {
                    let result = compare_score(left_head, right_head);
                    if result == std::cmp::Ordering::Equal {
                        compare_score_nodes(&left[1..], &right[1..])
                    } else {
                        result
                    }
                }
            }
        }

        fn compare_score(s1: &Tree<ScoreAtom>, s2: &Tree<ScoreAtom>) -> std::cmp::Ordering {
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

        scored_terms.sort_by(|s1, s2| compare_score(&s1.score, &s2.score));
    }
}

/**
 * Total order of all terms
 *
 * The general order is ground, vars, arithmetic, comparisons, logical, and then others
 */
impl<T> ScoredTerm<T> {
    pub fn sort_vec_metered(
        scored_terms: &mut Vec<ScoredTerm<T>>,
        meter: &SorterMeter<'_>,
    ) -> Result<(), BackingError> {
        let len = scored_terms.len();
        let mut indices = meter.vec(len)?;
        indices.extend(0..len);
        for start in (0..len / 2).rev() {
            sift_down(&mut indices, start, len, scored_terms, meter)?;
        }
        for end in (1..len).rev() {
            meter.reserve(1, size_of::<usize>() * 2, 0)?;
            indices.swap(0, end);
            sift_down(&mut indices, 0, end, scored_terms, meter)?;
        }
        let mut slots = meter.vec(len)?;
        let scanned = len
            .checked_mul(size_of::<ScoredTerm<T>>())
            .ok_or(BackingError::Overflow)?;
        meter.reserve(len, scanned, 0)?;
        slots.extend(scored_terms.drain(..).map(Some));
        for index in indices {
            scored_terms.push(slots[index].take().ok_or(BackingError::Rejected)?);
        }
        Ok(())
    }
}

fn sift_down<T>(
    indices: &mut [usize],
    mut root: usize,
    end: usize,
    scored_terms: &[ScoredTerm<T>],
    meter: &SorterMeter<'_>,
) -> Result<(), BackingError> {
    while let Some(left) = root.checked_mul(2).and_then(|value| value.checked_add(1)) {
        meter.reserve(1, size_of::<usize>() * 3, 0)?;
        if left >= end {
            break;
        }
        let mut child = left;
        if left + 1 < end
            && compare_index(scored_terms, indices[left], indices[left + 1], meter)?
                == Ordering::Less
        {
            child = left + 1;
        }
        if compare_index(scored_terms, indices[root], indices[child], meter)? != Ordering::Less {
            break;
        }
        indices.swap(root, child);
        root = child;
    }
    Ok(())
}

fn compare_index<T>(
    scored_terms: &[ScoredTerm<T>],
    left: usize,
    right: usize,
    meter: &SorterMeter<'_>,
) -> Result<Ordering, BackingError> {
    let order =
        compare_score_metered(&scored_terms[left].score, &scored_terms[right].score, meter)?;
    Ok(if order == Ordering::Equal {
        left.cmp(&right)
    } else {
        order
    })
}

fn compare_score_metered(
    left: &Tree<ScoreAtom>,
    right: &Tree<ScoreAtom>,
    meter: &SorterMeter<'_>,
) -> Result<Ordering, BackingError> {
    struct Frame<'a> {
        left: &'a [Tree<ScoreAtom>],
        right: &'a [Tree<ScoreAtom>],
        index: usize,
    }

    let mut frames = Vec::new();
    meter.push(&mut frames, Frame {
        left: std::slice::from_ref(left),
        right: std::slice::from_ref(right),
        index: 0,
    })?;
    while let Some(frame) = frames.last_mut() {
        meter.reserve(1, size_of::<Tree<ScoreAtom>>() * 2, 0)?;
        let left = frame.left.get(frame.index);
        let right = frame.right.get(frame.index);
        match (left, right) {
            (None, None) => {
                frames.pop();
            }
            (None, Some(_)) => return Ok(Ordering::Less),
            (Some(_), None) => return Ok(Ordering::Greater),
            (Some(left), Some(right)) => {
                frame.index += 1;
                match (left, right) {
                    (Tree::Leaf(left), Tree::Leaf(right)) => {
                        let order = compare_atom_metered(left, right, meter)?;
                        if order != Ordering::Equal {
                            return Ok(order);
                        }
                    }
                    (Tree::Leaf(_), Tree::Node(_)) => return Ok(Ordering::Less),
                    (Tree::Node(_), Tree::Leaf(_)) => return Ok(Ordering::Greater),
                    (Tree::Node(left), Tree::Node(right)) => {
                        meter.push(&mut frames, Frame {
                            left,
                            right,
                            index: 0,
                        })?;
                    }
                }
            }
        }
    }
    Ok(Ordering::Equal)
}

fn compare_atom_metered(
    left: &ScoreAtom,
    right: &ScoreAtom,
    meter: &SorterMeter<'_>,
) -> Result<Ordering, BackingError> {
    match (&left.value, &right.value) {
        (TaggedAtom::IntAtom(left), TaggedAtom::IntAtom(right)) => {
            meter.reserve(1, size_of::<i64>() * 2, 0)?;
            Ok(left.cmp(right))
        }
        (TaggedAtom::IntAtom(_), _) => Ok(Ordering::Less),
        (_, TaggedAtom::IntAtom(_)) => Ok(Ordering::Greater),
        (TaggedAtom::StringAtom(left), TaggedAtom::StringAtom(right)) => {
            compare_bytes_metered(left.as_bytes(), right.as_bytes(), meter)
        }
        (TaggedAtom::StringAtom(_), _) => Ok(Ordering::Less),
        (_, TaggedAtom::StringAtom(_)) => Ok(Ordering::Greater),
        (TaggedAtom::BytesAtom(left), TaggedAtom::BytesAtom(right)) => {
            compare_bytes_metered(left, right, meter)
        }
    }
}

fn compare_bytes_metered(
    left: &[u8],
    right: &[u8],
    meter: &SorterMeter<'_>,
) -> Result<Ordering, BackingError> {
    for (left, right) in left.iter().zip(right) {
        meter.reserve(1, 2, 0)?;
        let order = left.cmp(right);
        if order != Ordering::Equal {
            return Ok(order);
        }
    }
    meter.reserve(1, size_of::<usize>() * 2, 0)?;
    Ok(left.len().cmp(&right.len()))
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
mod metered_tests {
    use super::*;

    #[test]
    fn metered_sort_preserves_score_order_and_equal_score_input_order() {
        let mut ordinary = vec![
            ScoredTerm {
                term: 3,
                score: Tree::<ScoreAtom>::create_leaf_from_i64(2),
            },
            ScoredTerm {
                term: 2,
                score: Tree::<ScoreAtom>::create_leaf_from_i64(1),
            },
            ScoredTerm {
                term: 1,
                score: Tree::<ScoreAtom>::create_leaf_from_i64(1),
            },
            ScoredTerm {
                term: 4,
                score: Tree::<ScoreAtom>::create_leaf_from_i64(3),
            },
        ];
        let mut metered = ordinary.clone();
        ScoredTerm::sort_vec(&mut ordinary);
        let backing = |_: usize, _: usize, _: usize| Ok(());
        ScoredTerm::sort_vec_metered(&mut metered, &SorterMeter::new(&backing)).unwrap();
        assert_eq!(metered, ordinary);
        assert_eq!(
            metered
                .into_iter()
                .map(|item| item.term)
                .collect::<Vec<_>>(),
            [2, 1, 3, 4]
        );
    }
}
