// See models/src/main/scala/coop/rchain/models/rholang/sorter/ordering.scala

use std::collections::HashMap;

use prost::Message;

use super::par_sort_matcher::ParSortMatcher;
use super::score_tree::{compare_score_trees, ScoreAtom, ScoredTerm, Tree};
use super::sortable::Sortable;
use crate::rhoapi::Par;

pub struct Ordering;

impl Ordering {
    pub fn sort_pars<'a>(ps: impl IntoIterator<Item = &'a Par>) -> Vec<Par> {
        let mut ps_sorted: Vec<ScoredTerm<Par>> =
            ps.into_iter().map(ParSortMatcher::sort_match).collect();
        ScoredTerm::sort_vec(&mut ps_sorted);
        ps_sorted.into_iter().map(|st| st.term).collect()
    }

    pub fn sort_key_value_pair(key: &Par, value: &Par) -> ScoredTerm<(Par, Par)> {
        let sorted_key = ParSortMatcher::sort_match(key);
        let sorted_value = ParSortMatcher::sort_match(value);

        ScoredTerm {
            term: (sorted_key.term, sorted_value.term),
            score: sorted_key.score,
        }
    }

    // When two raw keys canonicalize to the same form their scores tie, and a
    // stable sort would preserve the (nondeterministic) HashMap iteration order.
    // Break ties by the protobuf-encoded raw-key bytes so the output is a pure
    // function of the input. See the SortedParMap determinism test.
    pub fn sort_map(ps: &HashMap<Par, Par>) -> Vec<(Par, Par)> {
        let mut rows: Vec<(&Par, ScoredTerm<(Par, Par)>)> = ps
            .iter()
            .map(|(k, v)| (k, Ordering::sort_key_value_pair(k, v)))
            .collect();

        rows.sort_by(|(a_raw, a), (b_raw, b)| {
            compare_score_trees(&a.score, &b.score)
                .then_with(|| a_raw.encode_to_vec().cmp(&b_raw.encode_to_vec()))
        });

        rows.into_iter().map(|(_, st)| st.term).collect()
    }

    /// Merges two sequences sorted by `score` into one. Where both hold an
    /// item of the same score, the item from `right` is kept.
    pub fn union_sorted<T>(
        left: Vec<T>,
        right: Vec<T>,
        score: impl Fn(&T) -> Tree<ScoreAtom>,
    ) -> Vec<T> {
        let mut merged = Vec::with_capacity(left.len() + right.len());
        let mut left = left.into_iter().map(|t| (score(&t), t)).peekable();
        let mut right = right.into_iter().map(|t| (score(&t), t)).peekable();
        loop {
            let order = match (left.peek(), right.peek()) {
                (Some((l, _)), Some((r, _))) => compare_score_trees(l, r),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => return merged,
            };
            let (_, item) = match order {
                std::cmp::Ordering::Less => left.next(),
                std::cmp::Ordering::Greater => right.next(),
                std::cmp::Ordering::Equal => {
                    left.next();
                    right.next()
                }
            }
            .expect("the peeked side has an item");
            merged.push(item);
        }
    }

    /// The items of `left` whose score no item of `right` has. Both are
    /// sorted by score.
    pub fn diff_sorted<T, U>(
        left: Vec<T>,
        right: &[U],
        score_left: impl Fn(&T) -> Tree<ScoreAtom>,
        score_right: impl Fn(&U) -> Tree<ScoreAtom>,
    ) -> Vec<T> {
        let mut right = right.iter().map(score_right).peekable();
        left.into_iter()
            .filter(|item| {
                let score = score_left(item);
                while right
                    .next_if(|r| compare_score_trees(r, &score) == std::cmp::Ordering::Less)
                    .is_some()
                {}
                right.peek().is_none_or(|r| compare_score_trees(r, &score) != std::cmp::Ordering::Equal)
            })
            .collect()
    }
}
