// See models/src/main/scala/coop/rchain/models/ParSetTypeMapper.scala

use super::par_set::ParSet;
use super::rholang::sorter::par_sort_matcher::ParSortMatcher;
use super::rholang::sorter::sortable::Sortable;
use crate::rhoapi::{ESet, Par};

pub struct ParSetTypeMapper;

impl ParSetTypeMapper {
    /// Whether `eset` has an element with the same sorted form as `elem`.
    /// Every set is built canonical, so its elements are already sorted.
    pub fn contains(eset: &ESet, elem: &Par) -> bool {
        eset.ps.contains(&ParSortMatcher::sort_match(elem).term)
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
}
