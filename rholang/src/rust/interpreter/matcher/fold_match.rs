use models::rhoapi::var::VarInstance::{FreeVar, Wildcard};
use models::rhoapi::{MatchCase, Par, Var};

use super::has_locally_free::HasLocallyFree;
use super::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
use crate::rust::interpreter::metrics_constants::{
    RHOLANG_MATCHER_FOLD_MATCH_CALLS_METRIC,
    RHOLANG_MATCHER_FOLD_MATCH_RECURSION_DEPTH_TOTAL_METRIC,
    RHOLANG_MATCHER_FOLD_MATCH_TAIL_CLONE_NS_METRIC, RHOLANG_METRICS_SOURCE,
};

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - foldMatch
//
// EPathMap fix P4.2 (amendment PM-3): `fold_match` takes BORROWED slices —
// the `Match::get` entry no longer owns its inputs, so the pair walk borrows
// the target/pattern lists and clones only what actually enters the owned
// spatial lattice (a binding/connective pair) or the free_map (a bound
// subterm). The non-binding pair comparison (`match_pars`) runs entirely by
// reference — a failing candidate copies nothing.
pub trait FoldMatch<T, P> {
    fn fold_match(
        &mut self,
        tlist: &[T],
        plist: &[P],
        remainder: Option<Var>,
    ) -> Option<Vec<T>>;

    fn free_check(&self, trem: &[T], level: i32, acc: Vec<T>) -> Option<Vec<T>>;
}

impl FoldMatch<Par, Par> for SpatialMatcherContext {
    fn fold_match(
        &mut self,
        tlist: &[Par],
        plist: &[Par],
        remainder: Option<Var>,
    ) -> Option<Vec<Par>> {
        // Iterative pair-walk over (tlist, plist) — index into the originals
        // and hand each head pair to the REFERENCE entry: the non-binding
        // case compares by reference (zero clones), the binding/connective
        // case clones the pair once into the owned lattice (P4.2; that clone
        // is the bound-candidate copy).
        metrics::counter!(RHOLANG_MATCHER_FOLD_MATCH_CALLS_METRIC, "source" => RHOLANG_METRICS_SOURCE)
            .increment(1);
        metrics::counter!(RHOLANG_MATCHER_FOLD_MATCH_RECURSION_DEPTH_TOTAL_METRIC, "source" => RHOLANG_METRICS_SOURCE)
            .increment(tlist.len().max(plist.len()) as u64);

        let n = tlist.len().min(plist.len());
        for i in 0..n {
            self.spatial_match_par_ref(&tlist[i], &plist[i])?;
        }

        if tlist.len() == plist.len() {
            // Exact-length walk consumed both lists.
            Some(Vec::new())
        } else if plist.len() < tlist.len() {
            // Surplus targets — must be absorbed by the remainder var, if any.
            let trem = &tlist[n..];
            match remainder {
                None => None,
                Some(Var {
                    var_instance: Some(FreeVar(level)),
                }) => self.free_check(trem, level, Vec::new()),
                Some(Var {
                    var_instance: Some(Wildcard(_)),
                }) => Some(Vec::new()),
                _ => None,
            }
        } else {
            // Surplus patterns with no targets — no match possible.
            None
        }
    }

    fn free_check(&self, trem: &[Par], level: i32, mut acc: Vec<Par>) -> Option<Vec<Par>> {
        match trem {
            &[] => Some(acc),

            [item, rem @ ..] => {
                // P4.2: `HasLocallyFree<Par> for SpatialMatcherContext` is
                // literally `p.locally_free` (has_locally_free.rs:49-53) —
                // read the precomputed field instead of cloning the whole Par
                // to feed the consuming signature. Byte-identical semantics.
                if item.locally_free.is_empty() {
                    acc.push(item.clone());
                    self.free_check(rem, level, acc)
                } else {
                    None
                }
            }
        }
    }
}

impl FoldMatch<MatchCase, MatchCase> for SpatialMatcherContext {
    fn fold_match(
        &mut self,
        tlist: &[MatchCase],
        plist: &[MatchCase],
        remainder: Option<Var>,
    ) -> Option<Vec<MatchCase>> {
        // Iterative pair-walk; head-pair clone per iteration (MatchCase has
        // no reference fast path — the pre-P4.2 behavior, cost-identical).
        let n = tlist.len().min(plist.len());
        for i in 0..n {
            let __clone_start = std::time::Instant::now();
            let t_owned = tlist[i].clone();
            let p_owned = plist[i].clone();
            metrics::counter!(RHOLANG_MATCHER_FOLD_MATCH_TAIL_CLONE_NS_METRIC, "source" => RHOLANG_METRICS_SOURCE)
                .increment(__clone_start.elapsed().as_nanos() as u64);
            self.spatial_match(t_owned, p_owned)?;
        }

        if tlist.len() == plist.len() {
            Some(Vec::new())
        } else if plist.len() < tlist.len() {
            let trem = &tlist[n..];
            match remainder {
                None => None,
                Some(Var {
                    var_instance: Some(FreeVar(level)),
                }) => self.free_check(trem, level, Vec::new()),
                Some(Var {
                    var_instance: Some(Wildcard(_)),
                }) => Some(Vec::new()),
                _ => None,
            }
        } else {
            None
        }
    }

    fn free_check(
        &self,
        trem: &[MatchCase],
        level: i32,
        mut acc: Vec<MatchCase>,
    ) -> Option<Vec<MatchCase>> {
        match trem {
            &[] => Some(acc),

            [item, rem @ ..] => {
                if self.locally_free(item.to_owned(), 0).is_empty() {
                    acc.push(item.clone());
                    self.free_check(rem, level, acc)
                } else {
                    None
                }
            }
        }
    }
}
