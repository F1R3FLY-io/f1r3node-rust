// See models/src/main/scala/coop/rchain/models/ParSetTypeMapper.scala

use super::par_set::ParSet;
use super::rholang::sorter::ordering::Ordering;
use super::rholang::sorter::par_sort_matcher::ParSortMatcher;
use super::rholang::sorter::score_tree::{compare_score_trees, ScoreAtom, Tree};
use super::rholang::sorter::sortable::Sortable;
use super::utils::union;
use crate::rhoapi::{ESet, Par};

pub struct ParSetTypeMapper;

impl ParSetTypeMapper {
    /// Whether `eset` has an element with the same sorted form as `elem`.
    /// Every set is built canonical, so its elements are already sorted.
    pub fn contains(eset: &ESet, elem: &Par) -> bool {
        eset.ps.contains(&ParSortMatcher::sort_match(elem).term)
    }

    /// Adds the sorted form of `elem` to the canonical elements `ps`.
    pub fn insert(ps: &mut Vec<Par>, elem: &Par) {
        let sorted = ParSortMatcher::sort_match(elem);
        if let Err(position) = Self::search(ps, &sorted.score) {
            ps.insert(position, sorted.term);
        }
    }

    /// Removes the element with the same sorted form as `elem` from the
    /// canonical elements `ps`.
    pub fn remove(ps: &mut Vec<Par>, elem: &Par) {
        if let Ok(position) = Self::search(ps, &ParSortMatcher::sort_match(elem).score) {
            ps.remove(position);
        }
    }

    /// The canonical elements of both canonical sequences.
    pub fn union(ps: Vec<Par>, other: Vec<Par>) -> Vec<Par> {
        Ordering::union_sorted(ps, other, Self::score)
    }

    /// The canonical elements of `ps` that `other` does not hold.
    pub fn diff(ps: Vec<Par>, other: &[Par]) -> Vec<Par> {
        Ordering::diff_sorted(ps, other, Self::score, Self::score)
    }

    /// A set of the canonical elements `ps`, with the flags they imply.
    pub fn canonical_eset(ps: Vec<Par>) -> ESet {
        let locally_free = ps
            .iter()
            .fold(Vec::new(), |acc, p| union(acc, p.locally_free.clone()));
        let connective_used = ps.iter().any(|p| p.connective_used);
        ESet {
            ps,
            locally_free,
            connective_used,
            remainder: None,
        }
    }

    pub fn eset_to_par_set(eset: ESet) -> ParSet {
        ParSet::new(
            eset.ps,
            eset.connective_used,
            eset.locally_free,
            eset.remainder,
        )
    }

    pub fn par_set_to_eset(par_set: ParSet) -> ESet {
        ESet {
            ps: par_set.ps.sorted_pars,
            locally_free: par_set.locally_free,
            connective_used: par_set.connective_used,
            remainder: par_set.remainder,
        }
    }

    fn score(p: &Par) -> Tree<ScoreAtom> { ParSortMatcher::sort_match(p).score }

    fn search(ps: &[Par], score: &Tree<ScoreAtom>) -> Result<usize, usize> {
        ps.binary_search_by(|p| compare_score_trees(&Self::score(p), score))
    }
}
