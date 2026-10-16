// See models/src/main/scala/coop/rchain/models/rholang/sorter/ScoreTree.scala

use std::cmp::Ordering;
use std::mem::size_of;

use shared::rust::ByteString;
use shared::rust::clone_backing::{BackingError, CloneBacking};

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

/// ★★★ **The tie-break key: the bytes this element contributes to the enclosing term.**
///
/// # Why the sibling order needs a second key at all
///
/// Siblings are ordered by score, and **the score is not injective on canonical terms**.
/// `combine_emap` chains only the *key's* score, so `{3 → 30}` and `{3 → 90}` are distinct
/// canonical terms with byte-identical score trees; `EZipper`'s cursor and `ReceiveBind`'s
/// `free_count` are two further lossy paths. Where two distinct terms tie, `sort_by`'s
/// **stability** hands the decision to whatever filled the input vector — which is `HashSet`
/// iteration order (seeded **per process**) at `SortedParHashSet::create_from_vec`, and the
/// message's own **field order** at `combine_par`.
///
/// ⇒ Measured: a set containing both maps split **20/20 across 40 processes**; and
/// `{3:30} | {3:90}` versus `{3:90} | {3:30}` — two spellings of one process, `|` being
/// commutative — reached **different canonical bytes deterministically**. See `SS-Y4` in the
/// stack-safety report.
///
/// # Why THIS key, and not another
///
/// The key is **derived, not chosen**. Consensus observes exactly one thing about a sibling:
/// the bytes it contributes to the enclosing encoded term. Ordering by those bytes is the
/// unique key for which *"swapping two siblings is invisible"* and *"the two siblings are equal
/// under the key"* are the **same statement**. That gives totality **without** requiring the
/// encoding to be injective: if two distinct terms encode identically, swapping them is
/// byte-invisible, so their residual order cannot be observed by anything.
///
/// ⚠ **This is NOT the proposal the consensus register rejected.** `consensus-change-register.md`
/// `:3016-3019` rejected sorting on `(score, sibling_index)` — correctly, because `sibling_index`
/// *is* the hash iteration index and is therefore itself nondeterministic. This key is a pure
/// function of the element's own value, computed after the element is fully sorted, with no
/// reference to position, container, seed or iteration order.
///
/// # Why a trait rather than an expression at each call site
///
/// `sort_vec` has eleven call sites. Adding a tie-break expression to each is a
/// *complete-the-list* repair, and the list would silently gain a twelfth. Instead the bound on
/// [`ScoredTerm::sort_vec`] is `T: EmittedBytes`, so **a sortable type that has not answered
/// this question does not compile**. There is no list to keep current.
pub trait EmittedBytes {
    /// The bytes the enclosing message will emit for this element.
    fn emitted_bytes(&self) -> Vec<u8>;

    /// The same bytes, with all walks and the output allocation reserved first.
    fn emitted_bytes_metered(&self, meter: &SorterMeter<'_>) -> Result<Vec<u8>, BackingError>;
}

fn prost_bytes_metered<T: prost::Message + CloneBacking>(
    value: &T,
    meter: &SorterMeter<'_>,
) -> Result<Vec<u8>, BackingError> {
    // Reserve the nested traversal before `encoded_len` builds its work table;
    // the output length is not known until that metered measurement finishes.
    meter.nested_encode(value, 0)?;
    let len = value.encoded_len();
    meter.reserve(1, len, len)?;
    Ok(value.encode_to_vec())
}

/// Every sortable node type is a prost message, and its emitted bytes are its own encoding.
macro_rules! emitted_bytes_via_prost {
    ($($t:ty),* $(,)?) => {
        $(
            impl EmittedBytes for $t {
                fn emitted_bytes(&self) -> Vec<u8> {
                    prost::Message::encode_to_vec(self)
                }


                fn emitted_bytes_metered(
                    &self,
                    meter: &SorterMeter<'_>,
                ) -> Result<Vec<u8>, BackingError> {
                    prost_bytes_metered(self, meter)
                }
            }
        )*
    };
}

emitted_bytes_via_prost!(
    crate::rhoapi::Par,
    crate::rhoapi::CostSignature,
    crate::rhoapi::CostSignedTerm,
    crate::rhoapi::CostStack,
    crate::rhoapi::Expr,
    crate::rhoapi::Send,
    crate::rhoapi::Receive,
    crate::rhoapi::ReceiveBind,
    crate::rhoapi::New,
    crate::rhoapi::Match,
    crate::rhoapi::MatchCase,
    crate::rhoapi::Bundle,
    crate::rhoapi::Connective,
    crate::rhoapi::GUnforgeable,
    crate::rhoapi::If,
    crate::rhoapi::Var,
);

/// An `EMap` entry. ⚠ The key must be **the pair's `KeyValuePair` encoding**, not the two `Par`s
/// concatenated, because that is what `par_map_to_emap` actually emits for this element.
impl EmittedBytes for (crate::rhoapi::Par, crate::rhoapi::Par) {
    fn emitted_bytes(&self) -> Vec<u8> {
        prost::Message::encode_to_vec(&crate::rhoapi::KeyValuePair {
            key: Some(self.0.clone()),
            value: Some(self.1.clone()),
        })
    }

    fn emitted_bytes_metered(&self, meter: &SorterMeter<'_>) -> Result<Vec<u8>, BackingError> {
        let pair = crate::rhoapi::KeyValuePair {
            key: Some(meter.clone_blocks(&self.0)?),
            value: Some(meter.clone_blocks(&self.1)?),
        };
        prost_bytes_metered(&pair, meter)
    }
}

/// ★ The twelfth `sort_vec` site — `rholang`'s `pre_sort_binds`, which sorts
/// `(ReceiveBind, FreeMap<T>)`. **The compiler found it**, not a search: that file failed to
/// build until this question was answered, which is the whole point of the bound.
///
/// ⚠ It lives here rather than in `rholang` because the orphan rule forbids implementing a
/// foreign trait for a tuple ("tuples are always foreign"). A *local* trait may be implemented
/// for any type, so `models` is the only crate that can answer for this shape.
///
/// ⚠ **The second element is deliberately ignored, and is generic to say so.** Consensus
/// observes the `ReceiveBind` that gets emitted; the `FreeMap` is normalizer bookkeeping that
/// reaches no byte. Ordering siblings on something a validator cannot see is precisely the
/// defect class `SS-Y4` repairs, so it must not enter the key.
///
/// ⇒ **Stated residual:** two entries with byte-identical `ReceiveBind`s stay tied and keep
/// source order. That is sound by the same argument that makes the key total without requiring
/// injectivity — if the emitted bytes are identical, swapping them is invisible.
impl<X> EmittedBytes for (crate::rhoapi::ReceiveBind, X) {
    fn emitted_bytes(&self) -> Vec<u8> { prost::Message::encode_to_vec(&self.0) }

    fn emitted_bytes_metered(&self, meter: &SorterMeter<'_>) -> Result<Vec<u8>, BackingError> {
        prost_bytes_metered(&self.0, meter)
    }
}

/// ⚠ Used only by `ScoredTerm<String>` in the sorter's own tests. Real terms never reach it.
impl EmittedBytes for String {
    fn emitted_bytes(&self) -> Vec<u8> { self.as_bytes().to_vec() }

    fn emitted_bytes_metered(&self, meter: &SorterMeter<'_>) -> Result<Vec<u8>, BackingError> {
        meter.reserve(1, self.len(), self.len())?;
        Ok(self.as_bytes().to_vec())
    }
}

/// ⚠ Test-only, and its presence is deliberate rather than incidental.
///
/// `score_tree.rs`'s permutation oracle sorts `ScoredTerm<usize>`, using the `usize` as a
/// position marker. ★ It is impl'd here rather than in the test module because that oracle's
/// whole purpose is to check the PERMUTATION `sort_vec` produces — so it must go through the
/// same code path, including the tie-break, or it would be checking a function that no longer
/// exists. Encoding as big-endian bytes keeps the byte order agreeing with the numeric order,
/// so a tie between two markers breaks the same way a reader would expect.
impl EmittedBytes for usize {
    fn emitted_bytes(&self) -> Vec<u8> { self.to_be_bytes().to_vec() }

    fn emitted_bytes_metered(&self, meter: &SorterMeter<'_>) -> Result<Vec<u8>, BackingError> {
        meter.reserve(1, size_of::<usize>(), size_of::<usize>())?;
        Ok(self.to_be_bytes().to_vec())
    }
}

impl<T: EmittedBytes> ScoredTerm<T> {
    /// Sort siblings into canonical order — a **total** order.
    ///
    /// ★ The tie-break **refines and never reorders**: it is consulted only where
    /// `compare_score` returns `Equal`. Every pair the score already separates keeps its order
    /// exactly, so byte-neutrality on any tie-free input is true **by construction** rather than
    /// by measurement. That is why `sorter_canonical_golden.rs` — whose collections carry
    /// pairwise-distinct scores by construction (`:88-101`) — must be byte-identical across this
    /// change, and why a move there would be a bug in this function rather than a legitimate
    /// canonical-form change.
    ///
    /// ⚠ `sort_by` is kept rather than `sort_unstable_by`, but the reason has CHANGED and the
    /// old one no longer applies. It used to be load-bearing: the comparator returned `Equal`
    /// for distinct terms, so an unstable sort could reorder them and fork the canonical form.
    /// With a total order there are no ties left for stability to have an opinion about, so the
    /// choice is now immaterial to correctness. It is kept because the consensus register cites
    /// it, and changing it would be an unmeasured second axis in a commit that already moves
    /// bytes.
    pub fn sort_vec(scored_terms: &mut Vec<ScoredTerm<T>>) {
        scored_terms.sort_by(|s1, s2| {
            compare_score(&s1.score, &s2.score)
                .then_with(|| s1.term.emitted_bytes().cmp(&s2.term.emitted_bytes()))
        });
    }
}

/**
 * Total order of all terms
 *
 * The general order is ground, vars, arithmetic, comparisons, logical, and then others
 */
impl<T: EmittedBytes> ScoredTerm<T> {
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

fn sift_down<T: EmittedBytes>(
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

fn compare_index<T: EmittedBytes>(
    scored_terms: &[ScoredTerm<T>],
    left: usize,
    right: usize,
    meter: &SorterMeter<'_>,
) -> Result<Ordering, BackingError> {
    let order =
        compare_score_metered(&scored_terms[left].score, &scored_terms[right].score, meter)?;
    if order != Ordering::Equal {
        return Ok(order);
    }
    // Match `sort_vec`'s emitted-byte tie-break. The index is only the stable
    // fallback for byte-identical terms; `scored_terms` is untouched until all
    // comparisons and the final output reservation succeed.
    let left_bytes = scored_terms[left].term.emitted_bytes_metered(meter)?;
    let right_bytes = scored_terms[right].term.emitted_bytes_metered(meter)?;
    let emitted_order = compare_bytes_metered(&left_bytes, &right_bytes, meter)?;
    Ok(emitted_order.then_with(|| left.cmp(&right)))
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
    pub const AUTHORITY_ID: i32 = 18;
    pub const PRINCIPAL_ID: i32 = 20;
    pub const EPATHMAP: i32 = 13;
    pub const DOUBLE: i32 = 14;
    pub const BIG_INT: i32 = 15;
    pub const BIG_RAT: i32 = 16;
    pub const FIXED_POINT: i32 = 17;
    pub const UINT64: i32 = 24;
    pub const INT32: i32 = 25;
    pub const UINT32: i32 = 26;
    pub const UINT16: i32 = 27;
    pub const UINT8: i32 = 28;
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
    pub const COST_STACK_PARALLEL: i32 = 318;

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
// Test-only recursive oracle and differential
// ===========================================================================

#[cfg(test)]
mod score_tag_tests {
    use std::collections::HashMap;

    use super::Score;

    #[test]
    fn all_score_tags_are_unique_and_declared() {
        let tags = [
            ("ABSENT", Score::ABSENT),
            ("BOOL", Score::BOOL),
            ("INT", Score::INT),
            ("STRING", Score::STRING),
            ("URI", Score::URI),
            ("PRIVATE", Score::PRIVATE),
            ("ELIST", Score::ELIST),
            ("ETUPLE", Score::ETUPLE),
            ("ESET", Score::ESET),
            ("EMAP", Score::EMAP),
            ("DEPLOYER_AUTH", Score::DEPLOYER_AUTH),
            ("DEPLOY_ID", Score::DEPLOY_ID),
            ("SYS_AUTH_TOKEN", Score::SYS_AUTH_TOKEN),
            ("AUTHORITY_ID", Score::AUTHORITY_ID),
            ("PRINCIPAL_ID", Score::PRINCIPAL_ID),
            ("EPATHMAP", Score::EPATHMAP),
            ("DOUBLE", Score::DOUBLE),
            ("BIG_INT", Score::BIG_INT),
            ("BIG_RAT", Score::BIG_RAT),
            ("FIXED_POINT", Score::FIXED_POINT),
            ("UINT64", Score::UINT64),
            ("INT32", Score::INT32),
            ("UINT32", Score::UINT32),
            ("UINT16", Score::UINT16),
            ("UINT8", Score::UINT8),
            ("FLOAT32", Score::FLOAT32),
            ("BOUND_VAR", Score::BOUND_VAR),
            ("FREE_VAR", Score::FREE_VAR),
            ("WILDCARD", Score::WILDCARD),
            ("REMAINDER", Score::REMAINDER),
            ("EVAR", Score::EVAR),
            ("ENEG", Score::ENEG),
            ("EMULT", Score::EMULT),
            ("EDIV", Score::EDIV),
            ("EPLUS", Score::EPLUS),
            ("EMINUS", Score::EMINUS),
            ("ELT", Score::ELT),
            ("ELTE", Score::ELTE),
            ("EGT", Score::EGT),
            ("EGTE", Score::EGTE),
            ("EEQ", Score::EEQ),
            ("ENEQ", Score::ENEQ),
            ("ENOT", Score::ENOT),
            ("EAND", Score::EAND),
            ("EOR", Score::EOR),
            ("EMETHOD", Score::EMETHOD),
            ("EBYTEARR", Score::EBYTEARR),
            ("EEVAL", Score::EEVAL),
            ("EMATCHES", Score::EMATCHES),
            ("EPERCENT", Score::EPERCENT),
            ("EPLUSPLUS", Score::EPLUSPLUS),
            ("EMINUSMINUS", Score::EMINUSMINUS),
            ("EMOD", Score::EMOD),
            ("QUOTE", Score::QUOTE),
            ("CHAN_VAR", Score::CHAN_VAR),
            ("SEND", Score::SEND),
            ("RECEIVE", Score::RECEIVE),
            ("NEW", Score::NEW),
            ("MATCH", Score::MATCH),
            ("BUNDLE_EQUIV", Score::BUNDLE_EQUIV),
            ("BUNDLE_READ", Score::BUNDLE_READ),
            ("BUNDLE_WRITE", Score::BUNDLE_WRITE),
            ("BUNDLE_READ_WRITE", Score::BUNDLE_READ_WRITE),
            ("IF", Score::IF),
            ("COST_SIG_GROUND", Score::COST_SIG_GROUND),
            ("COST_SIG_BOUND", Score::COST_SIG_BOUND),
            ("COST_SIG_QUOTE", Score::COST_SIG_QUOTE),
            ("COST_SIG_COMPOUND", Score::COST_SIG_COMPOUND),
            ("COST_SIGNED_TERM", Score::COST_SIGNED_TERM),
            ("COST_STACK", Score::COST_STACK),
            ("COST_SIG_NAME", Score::COST_SIG_NAME),
            ("COST_SIG_UNIT", Score::COST_SIG_UNIT),
            ("COST_STACK_PARALLEL", Score::COST_STACK_PARALLEL),
            ("CONNECTIVE_NOT", Score::CONNECTIVE_NOT),
            ("CONNECTIVE_AND", Score::CONNECTIVE_AND),
            ("CONNECTIVE_OR", Score::CONNECTIVE_OR),
            ("CONNECTIVE_VARREF", Score::CONNECTIVE_VARREF),
            ("CONNECTIVE_BOOL", Score::CONNECTIVE_BOOL),
            ("CONNECTIVE_INT", Score::CONNECTIVE_INT),
            ("CONNECTIVE_STRING", Score::CONNECTIVE_STRING),
            ("CONNECTIVE_URI", Score::CONNECTIVE_URI),
            ("CONNECTIVE_BYTEARRAY", Score::CONNECTIVE_BYTEARRAY),
            ("PAR", Score::PAR),
        ];
        let declarations = include_str!("score_tree.rs")
            .split_once("impl Score {")
            .expect("Score implementation")
            .1
            .split_once("\n}\n")
            .expect("Score implementation end")
            .0
            .lines()
            .filter_map(|line| {
                line.trim_start()
                    .strip_prefix("pub const ")
                    .and_then(|rest| rest.split_once(": i32"))
                    .map(|(name, _)| name)
            })
            .collect::<Vec<_>>();
        assert_eq!(
            declarations,
            tags.iter().map(|(name, _)| *name).collect::<Vec<_>>()
        );

        let mut seen = HashMap::new();
        for (name, value) in tags {
            assert!(
                seen.insert(value, name).is_none(),
                "duplicate score tag: {name}"
            );
        }
        assert_eq!(Score::EPATHMAP, 13);
        assert_eq!(Score::DOUBLE, 14);
        assert_eq!(Score::AUTHORITY_ID, 18);
        assert_eq!(Score::FLOAT32, 19);
        assert_eq!(Score::PRINCIPAL_ID, 20);
        assert_eq!(Score::UINT64, 24);
        assert_eq!(Score::INT32, 25);
        assert_eq!(Score::UINT32, 26);
        assert_eq!(Score::UINT16, 27);
        assert_eq!(Score::UINT8, 28);
    }
}

#[cfg(test)]
#[path = "../../../../tests/support/score_tree_oracle.rs"]
mod score_tree_oracle;
#[cfg(test)]
mod metered_tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn metered_sort_preserves_score_order_and_equal_score_emitted_bytes_order() {
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
            [1, 2, 3, 4]
        );
    }

    #[derive(Clone, Debug, PartialEq)]
    struct MarkedBytes {
        emitted: Vec<u8>,
        input_index: usize,
    }

    impl EmittedBytes for MarkedBytes {
        fn emitted_bytes(&self) -> Vec<u8> { self.emitted.clone() }

        fn emitted_bytes_metered(&self, meter: &SorterMeter<'_>) -> Result<Vec<u8>, BackingError> {
            meter.reserve(1, self.emitted.len(), self.emitted.len())?;
            Ok(self.emitted.clone())
        }
    }

    proptest! {
        #[test]
        fn metered_tie_order_agrees_with_ordinary_and_stabilizes_identical_bytes(
            items in proptest::collection::vec((-2_i64..=2, proptest::collection::vec(any::<u8>(), 0..5)), 0..24)
        ) {
            let mut ordinary = items.iter().enumerate().map(|(input_index, (score, emitted))| {
                ScoredTerm {
                    term: MarkedBytes { emitted: emitted.clone(), input_index },
                    score: Tree::<ScoreAtom>::create_leaf_from_i64(*score),
                }
            }).collect::<Vec<_>>();
            let mut metered = ordinary.clone();
            ScoredTerm::sort_vec(&mut ordinary);
            let backing = |_: usize, _: usize, _: usize| Ok(());
            ScoredTerm::sort_vec_metered(&mut metered, &SorterMeter::new(&backing)).unwrap();
            prop_assert_eq!(&metered, &ordinary);
            for pair in metered.windows(2) {
                if compare_score(&pair[0].score, &pair[1].score) == Ordering::Equal
                    && pair[0].term.emitted == pair[1].term.emitted
                {
                    prop_assert!(pair[0].term.input_index < pair[1].term.input_index);
                }
            }
        }
    }

    #[test]
    fn metered_tie_refusal_leaves_terms_unchanged() {
        let mut terms = vec![
            ScoredTerm {
                term: MarkedBytes {
                    emitted: vec![2],
                    input_index: 0,
                },
                score: Tree::<ScoreAtom>::create_leaf_from_i64(1),
            },
            ScoredTerm {
                term: MarkedBytes {
                    emitted: vec![1],
                    input_index: 1,
                },
                score: Tree::<ScoreAtom>::create_leaf_from_i64(1),
            },
        ];
        let before = terms.clone();
        let rejected_tie_bytes = std::cell::Cell::new(false);
        let backing = |_: usize, scanned: usize, backing: usize| {
            if scanned == 1 && backing == 1 {
                rejected_tie_bytes.set(true);
                Err(BackingError::Rejected)
            } else {
                Ok(())
            }
        };
        assert_eq!(
            ScoredTerm::sort_vec_metered(&mut terms, &SorterMeter::new(&backing)),
            Err(BackingError::Rejected)
        );
        assert!(rejected_tie_bytes.get());
        assert_eq!(terms, before);
    }
}
