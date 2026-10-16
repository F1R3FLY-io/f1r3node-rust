use models::rhoapi::var::VarInstance::{FreeVar, Wildcard};
use models::rhoapi::{MatchCase, Par, Var};

// Changed by D-D2 (D-M8, DR-104): the matcher tests its values by reference.
// use super::has_locally_free::HasLocallyFree;
use super::has_locally_free::HasLocallyFreeRef;
use super::spatial_matcher::{free_variable_level, SpatialMatcher, SpatialMatcherContext};
use crate::rust::interpreter::metrics_constants::{
    RHOLANG_MATCHER_FOLD_MATCH_CALLS_METRIC,
    RHOLANG_MATCHER_FOLD_MATCH_RECURSION_DEPTH_TOTAL_METRIC,
    RHOLANG_MATCHER_FOLD_MATCH_TAIL_CLONE_NS_METRIC, RHOLANG_METRICS_SOURCE,
};

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - foldMatch
pub trait FoldMatch<T, P> {
    fn fold_match(&mut self, tlist: &[T], plist: &[P], remainder: Option<Var>) -> Option<Vec<T>>;

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
        // and clone only the per-iteration head pair that `spatial_match`
        // consumes. Total Par clones: O(min(tlist.len(), plist.len())).
        metrics::counter!(RHOLANG_MATCHER_FOLD_MATCH_CALLS_METRIC, "source" => RHOLANG_METRICS_SOURCE)
            .increment(1);
        metrics::counter!(RHOLANG_MATCHER_FOLD_MATCH_RECURSION_DEPTH_TOTAL_METRIC, "source" => RHOLANG_METRICS_SOURCE)
            .increment(tlist.len().max(plist.len()) as u64);

        let n = tlist.len().min(plist.len());
        for i in 0..n {
            // Changed by D-D2 (D-M8, DR-104): a ground pattern is compared by
            // reference, without copies of the pair.
            if !plist[i].connective_used {
                self.match_ground_par(&tlist[i], &plist[i])?;
                continue;
            }
            // D-D3 (D-M9, DR-105): a pattern that is one free variable binds the
            // target's fields directly, without copies of the pair.
            if self.free_variable_fast_path() {
                if let Some(level) = free_variable_level(&plist[i]) {
                    self.bind_free_variable_by_reference(&tlist[i], level)?;
                    continue;
                }
            }
            // Changed by D-O1 (DR-109): block accounting charges inline bytes
            // once per enclosing block.
            // self.reserve_clone(&tlist[i])?;
            // self.reserve_clone(&plist[i])?;
            let __clone_start = std::time::Instant::now();
            let t_owned = tlist[i].clone();
            let p_owned = plist[i].clone();
            metrics::counter!(RHOLANG_MATCHER_FOLD_MATCH_TAIL_CLONE_NS_METRIC, "source" => RHOLANG_METRICS_SOURCE)
                .increment(__clone_start.elapsed().as_nanos() as u64);
            self.spatial_match(t_owned, p_owned)?;
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

    fn free_check(&self, trem: &[Par], _level: i32, mut acc: Vec<Par>) -> Option<Vec<Par>> {
        for item in trem {
            // Changed by D-D2 (D-M8, DR-104): the predicate reads the item by
            // reference, so an inspection replaces the copy.
            // self.reserve_clone(item)?;
            // if !self.locally_free(item.to_owned(), 0).is_empty() {
            // Changed by D-O1 (DR-109): block accounting charges inline bytes
            // once per enclosing block.
            // self.reserve_inspect(item)?;
            if !self.locally_free_is_empty(item, 0) {
                return None;
            }
            // Changed by D-O1 (DR-109): block accounting charges inline bytes
            // once per enclosing block.
            // self.reserve_clone(item)?;
            acc.push(item.clone());
        }
        Some(acc)
    }
}

impl FoldMatch<MatchCase, MatchCase> for SpatialMatcherContext {
    fn fold_match(
        &mut self,
        tlist: &[MatchCase],
        plist: &[MatchCase],
        remainder: Option<Var>,
    ) -> Option<Vec<MatchCase>> {
        // Iterative pair-walk; head-pair clone per iteration, no tail-vec clone.
        let n = tlist.len().min(plist.len());
        for i in 0..n {
            // Changed by D-O1 (DR-109): block accounting charges inline bytes
            // once per enclosing block.
            // self.reserve_clone(&tlist[i])?;
            // self.reserve_clone(&plist[i])?;
            self.spatial_match(tlist[i].clone(), plist[i].clone())?;
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
        _level: i32,
        mut acc: Vec<MatchCase>,
    ) -> Option<Vec<MatchCase>> {
        for item in trem {
            // Changed by D-D2 (D-M8, DR-104): the predicate reads the item by
            // reference, so an inspection replaces the copy.
            // self.reserve_clone(item)?;
            // if !self.locally_free(item.to_owned(), 0).is_empty() {
            // Changed by D-O1 (DR-109): block accounting charges inline bytes
            // once per enclosing block.
            // self.reserve_inspect(item)?;
            if !self.locally_free_is_empty(item, 0) {
                return None;
            }
            // Changed by D-O1 (DR-109): block accounting charges inline bytes
            // once per enclosing block.
            // self.reserve_clone(item)?;
            acc.push(item.clone());
        }
        Some(acc)
    }
}
