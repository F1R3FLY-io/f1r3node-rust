// See models/src/main/scala/coop/rchain/models/rholang/sorter/ordering.scala

use std::collections::HashMap;

use prost::Message;

use super::par_sort_matcher::ParSortMatcher;
use super::score_tree::{compare_score_trees, ScoredTerm};
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
}
