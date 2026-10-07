// See models/src/main/scala/coop/rchain/models/rholang/sorter/BundleSortMatcher.scala

use shared::rust::clone_backing::BackingError;

use super::metered::SorterMeter;
use super::par_sort_matcher::ParSortMatcher;
use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::sortable::Sortable;
use crate::rhoapi::Bundle;

pub struct BundleSortMatcher;

impl BundleSortMatcher {
    pub fn sort_match_metered(
        bundle: &Bundle,
        meter: &SorterMeter<'_>,
    ) -> Result<ScoredTerm<Bundle>, BackingError> {
        let score = match (bundle.write_flag, bundle.read_flag) {
            (true, true) => Score::BUNDLE_READ_WRITE,
            (true, false) => Score::BUNDLE_WRITE,
            (false, true) => Score::BUNDLE_READ,
            (false, false) => Score::BUNDLE_EQUIV,
        };
        let sorted_body = ParSortMatcher::sort_match_metered(
            bundle.body.as_ref().ok_or(BackingError::Rejected)?,
            meter,
        )?;
        let mut children = meter.vec(1)?;
        children.push(sorted_body.score);
        Ok(ScoredTerm {
            term: Bundle {
                body: Some(sorted_body.term),
                write_flag: bundle.write_flag,
                read_flag: bundle.read_flag,
            },
            score: Tree::<ScoreAtom>::create_node_from_i32_metered(score, children, meter)?,
        })
    }
}

impl Sortable<Bundle> for BundleSortMatcher {
    fn sort_match(b: &Bundle) -> ScoredTerm<Bundle> {
        let score = if b.write_flag && b.read_flag {
            Score::BUNDLE_READ_WRITE
        } else if b.write_flag && !b.read_flag {
            Score::BUNDLE_WRITE
        } else if !b.write_flag && b.read_flag {
            Score::BUNDLE_READ
        } else {
            Score::BUNDLE_EQUIV
        };

        let sorted_par = ParSortMatcher::sort_match(
            b.body.as_ref().expect("body was None, should be Some(Par)"),
        );

        ScoredTerm {
            term: {
                let mut b_cloned = b.clone();
                b_cloned.body = Some(sorted_par.term);
                b_cloned
            },
            score: Tree::<ScoreAtom>::create_node_from_i32(score, vec![sorted_par.score]),
        }
    }
}
