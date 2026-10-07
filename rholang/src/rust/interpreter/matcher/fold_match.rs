use models::rhoapi::var::VarInstance::{FreeVar, Wildcard};
use models::rhoapi::{MatchCase, Par, Var};

// Changed by D-D2 (D-M8, DR-104): the matcher tests its values by reference.
// use super::has_locally_free::HasLocallyFree;
use super::has_locally_free::HasLocallyFreeRef;
use super::spatial_matcher::{SpatialMatcher, SpatialMatcherContext};
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

impl<'a> FoldMatch<Par, Par> for SpatialMatcherContext<'a> {
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
            self.reserve_flag_read()?;
            if !plist[i].connective_used {
                self.match_ground_par(&tlist[i], &plist[i])?;
                continue;
            }
            self.reserve_clone(&tlist[i])?;
            self.reserve_clone(&plist[i])?;
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
            self.reserve_inspect(item)?;
            if !self.locally_free_is_empty(item, 0) {
                return None;
            }
            self.reserve_clone(item)?;
            self.reserve_vec(&mut acc, 1)?;
            acc.push(item.clone());
        }
        Some(acc)
    }
}

impl<'a> FoldMatch<MatchCase, MatchCase> for SpatialMatcherContext<'a> {
    fn fold_match(
        &mut self,
        tlist: &[MatchCase],
        plist: &[MatchCase],
        remainder: Option<Var>,
    ) -> Option<Vec<MatchCase>> {
        // Iterative pair-walk; head-pair clone per iteration, no tail-vec clone.
        let n = tlist.len().min(plist.len());
        for i in 0..n {
            self.reserve_clone(&tlist[i])?;
            self.reserve_clone(&plist[i])?;
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
            self.reserve_inspect(item)?;
            if !self.locally_free_is_empty(item, 0) {
                return None;
            }
            self.reserve_clone(item)?;
            self.reserve_vec(&mut acc, 1)?;
            acc.push(item.clone());
        }
        Some(acc)
    }
}

#[cfg(test)]
mod metered_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use models::rhoapi::var::VarInstance::FreeVar;
    use rspace_plus_plus::rspace::errors::RSpaceError;

    use super::*;

    #[test]
    fn remainder_matching_preserves_results_and_rejects_each_missing_operation() {
        let targets = vec![Par::default(); 4];
        let patterns = vec![Par::default()];
        let remainder = Some(Var {
            var_instance: Some(FreeVar(0)),
        });
        let expected = SpatialMatcherContext::new()
            .fold_match(&targets, &patterns, remainder.clone())
            .unwrap();
        let used = AtomicUsize::new(0);
        let unlimited = |operations: usize, _: usize, _: usize| {
            used.fetch_add(operations, Ordering::Relaxed);
            Ok(())
        };
        let mut context = SpatialMatcherContext::with_meter(&unlimited).unwrap();
        assert_eq!(
            context.fold_match(&targets, &patterns, remainder.clone()),
            Some(expected.clone())
        );
        assert!(context.take_error().is_none());
        let required = used.load(Ordering::Relaxed);
        assert!(required > 1);
        for limit in 0..required {
            let spent = AtomicUsize::new(0);
            let meter = |operations: usize, _: usize, _: usize| {
                let next = spent.load(Ordering::Relaxed) + operations;
                if next > limit {
                    return Err(RSpaceError::HostWorkRejected);
                }
                spent.store(next, Ordering::Relaxed);
                Ok(())
            };
            match SpatialMatcherContext::with_meter(&meter) {
                Ok(mut context) => {
                    assert!(context
                        .fold_match(&targets, &patterns, remainder.clone())
                        .is_none());
                    assert!(matches!(
                        context.take_error(),
                        Some(RSpaceError::HostWorkRejected)
                    ));
                }
                Err(RSpaceError::HostWorkRejected) => {}
                Err(error) => panic!("unexpected matcher error: {error}"),
            }
        }
    }
}
