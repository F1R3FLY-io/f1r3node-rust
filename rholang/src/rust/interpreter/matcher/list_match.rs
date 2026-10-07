use models::rhoapi::Par;
use models::rust::utils::FreeMap;
use rspace_plus_plus::rspace::errors::RSpaceError;
use shared::rust::clone_backing::{BackingError, CloneBacking, Walker};
use shared::rust::collection_backing::tree_backing;

use super::spatial_matcher::MatcherWork;

#[derive(Clone, Debug)]
pub enum Pattern<T: Clone> {
    Term(T),
    Remainder(i32),
}

impl<T: Clone + CloneBacking> CloneBacking for Pattern<T> {
    fn children<'a>(&'a self, walker: &mut Walker<'a>) -> Result<(), BackingError> {
        match self {
            Self::Term(value) => walker.push(value),
            Self::Remainder(level) => walker.push(level),
        }
    }
}

pub(super) trait ListMatch<T: Clone> {
    // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - listMatchSingle
    fn list_match_single(&mut self, tlist: Vec<T>, plist: Vec<T>) -> Option<()>;

    // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - listMatchSingle_
    fn list_match_single_(
        &mut self,
        tlist: Vec<T>,
        plist: Vec<T>,
        merger: &dyn Fn(Par, Vec<T>, &MatcherWork<'_>) -> Option<Par>,
        remainder: Option<i32>,
        wildcard: bool,
    ) -> Option<()>;

    // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - listMatch
    fn list_match(
        &mut self,
        targets: Vec<T>,
        patterns: Vec<T>,
        merger: &dyn Fn(Par, Vec<T>, &MatcherWork<'_>) -> Option<Par>,
        remainder: Option<i32>,
        wildcard: bool,
    ) -> Option<()>;

    // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - handleRemainder
    fn handle_remainder(
        &mut self,
        remainder_targets: Vec<T>,
        level: i32,
        merger: &dyn Fn(Par, Vec<T>, &MatcherWork<'_>) -> Option<Par>,
    ) -> Option<()>;

    fn match_function(&mut self, pattern: Pattern<T>, t: T) -> Option<FreeMap>;
}

// Rust extension doesn't auto-format custom macros.
#[macro_export]
macro_rules! list_match {
  ($($type:ty),*) => {
      $(
          impl<'a> ListMatch<$type> for SpatialMatcherContext<'a> {
              // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - listMatchSingle
              fn list_match_single(&mut self, tlist: Vec<$type>, plist: Vec<$type>) -> Option<()> {
                let _merger: &dyn Fn(Par, Vec<$type>, &MatcherWork<'_>) -> Option<Par> = &|p, _, _| Some(p);

                self.list_match_single_(tlist, plist, _merger, None, false)
              }

              // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - listMatchSingle_
              fn list_match_single_(
                  &mut self,
                  tlist: Vec<$type>,
                  plist: Vec<$type>,
                  merger: &dyn Fn(Par, Vec<$type>, &MatcherWork<'_>) -> Option<Par>,
                  remainder: Option<i32>,
                  wildcard: bool,
              ) -> Option<()> {
                let exact_match = !wildcard && remainder.is_none();
                let plen = plist.len();
                let tlen = tlist.len();

                if exact_match && plen != tlen {
                    None
                } else if plen > tlen {
                    None
                } else if plen == 0 && tlen == 0 && remainder.is_none() {
                    Some(())
                } else if plen == 0 && remainder.is_some() {
                    for target in &tlist {
                        self.reserve_clone(target)?;
                        if !self.locally_free(target.to_owned(), 0).is_empty() {
                            return None;
                        }
                    }
                    self.handle_remainder(tlist, remainder.unwrap(), merger)
                } else {
                    self.list_match(tlist, plist, merger, remainder, wildcard)
                }
              }

              // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - listMatch
              fn list_match(
                  &mut self,
                  targets: Vec<$type>,
                  patterns: Vec<$type>,
                  merger: &dyn Fn(Par, Vec<$type>, &MatcherWork<'_>) -> Option<Par>,
                  remainder: Option<i32>,
                  wildcard: bool,
              ) -> Option<()> {
                let remainder_count = targets.len().checked_sub(patterns.len())?;
                let mut all_patterns: Vec<Pattern<$type>> = Vec::new();
                self.reserve_vec(&mut all_patterns, targets.len())?;
                if let Some(level) = remainder {
                    for _ in 0..remainder_count {
                        all_patterns.push(Pattern::Remainder(level));
                    }
                }
                all_patterns.extend(patterns.into_iter().map(Pattern::Term));

                self.reserve_clone(&self.free_map)?;
                let mut cloned_self = self.clone();
                self.reserve(1, 0, std::mem::size_of::<SpatialMatcherContext<'a>>() + std::mem::size_of::<usize>() * 2)?;
                let _match_function = Box::new(move |pattern, t| cloned_self.match_function(pattern, t));
                // NOTE: Bypassing 'memoizeInHashMap' here
                let mut maximum_bipartite_match: MaximumBipartiteMatch<'_, Pattern<$type>, $type, FreeMap> = MaximumBipartiteMatch::new(_match_function, self.work()?);

                self.reserve_clone(&targets)?;
                let matches = maximum_bipartite_match.find_matches(all_patterns, targets.clone())?;

                let mut free_maps = Vec::new();
                for (_, _, free_map) in &matches {
                    self.reserve_clone(free_map)?;
                    self.reserve_vec(&mut free_maps, 1)?;
                    free_maps.push(free_map.clone());
                }

                self.reserve_clone(&self.free_map)?;
                let updated_free_map = aggregate_updates(self.free_map.clone(), free_maps, &self.work()?)?;
                self.free_map = updated_free_map;

                let mut remainder_targets = Vec::new();
                for (target, pattern, _) in &matches {
                    if matches!(pattern, Pattern::Remainder(_)) {
                        self.reserve_clone(target)?;
                        self.reserve_vec(&mut remainder_targets, 1)?;
                        remainder_targets.push(target.clone());
                    }
                }

                let mut remainder_targets_sorted = Vec::new();
                for target in &targets {
                    self.reserve_inspect(target)?;
                    self.reserve_inspect(&remainder_targets)?;
                    if remainder_targets.contains(target) {
                        self.reserve_clone(target)?;
                        self.reserve_vec(&mut remainder_targets_sorted, 1)?;
                        remainder_targets_sorted.push(target.clone());
                    }
                }

                match remainder {
                    None => {
                        if wildcard || remainder_targets_sorted.is_empty() {
                            Some(())
                        } else {
                            None
                        }
                    }
                    Some(level) => self.handle_remainder(remainder_targets_sorted, level, merger),
                }
              }

              // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - handleRemainder
              fn handle_remainder(
                  &mut self,
                  remainder_targets: Vec<$type>,
                  level: i32,
                  merger: &dyn Fn(Par, Vec<$type>, &MatcherWork<'_>) -> Option<Par>,
              ) -> Option<()> {
                self.reserve_inspect(&self.free_map)?;
                let remainder_par = match self.free_map.get(&level) {
                    Some(par) => {
                        self.reserve_clone(par)?;
                        par.clone()
                    }
                    None => vector_par(Vec::new(), false),
                };

                let work = self.work()?;
                let remainder_par_updated = merger(remainder_par, remainder_targets, &work)?;

                let Some(entries) = self.free_map.len().checked_add(1) else {
                    return self.reject(rspace_plus_plus::rspace::errors::RSpaceError::HostWorkRejected);
                };
                let Some((operations, bytes)) = shared::rust::collection_backing::tree_backing::<i32, Par>(entries) else {
                    return self.reject(rspace_plus_plus::rspace::errors::RSpaceError::HostWorkRejected);
                };
                self.reserve(operations, bytes, bytes)?;
                self.free_map.insert(level, remainder_par_updated);

                Some(())
              }

              /*
                  'The Box is used here to store the function on the heap rather than the stack. This is because the size of the function is not known at
                compile time (it's a closure that captures its environment), so it cannot be stored directly on the stack. The Box provides a fixed-size
                pointer to the function on the heap, which can be stored on the stack.' - GPT-4
              */
              fn match_function(&mut self, pattern: Pattern<$type>, t: $type) -> Option<FreeMap> {
                let match_effect: Option<()> = match pattern {
                  Pattern::Term(p) => {
                     self.reserve_clone(&p)?;
                     if !self.connective_used(p.clone()) {
                         self.reserve_inspect(&t)?;
                         self.reserve_inspect(&p)?;
                         guard(t == p)
                      } else {
                         self.spatial_match(t, p)
                      }
                  }
                  Pattern::Remainder(_) => {
                      self.reserve_inspect(&t)?;
                      guard(self.locally_free(t, 0).is_empty())
                  }
                };

                match match_effect {
                  Some(_) => {
                    let free_map = &self.free_map;
                    self.reserve_clone(free_map)?;
                    Some(free_map.clone())},
                  None => None,
                }
              }
          }
      )*
  };
}

pub(super) fn aggregate_updates(
    mut current_free_map: FreeMap,
    free_maps: Vec<FreeMap>,
    work: &MatcherWork<'_>,
) -> Option<FreeMap> {
    for free_map in free_maps {
        for (level, value) in free_map {
            work.reserve_inspect(&current_free_map)?;
            let Some(entries) = current_free_map.len().checked_add(1) else {
                return work.reject(RSpaceError::HostWorkRejected);
            };
            let Some((operations, bytes)) = tree_backing::<i32, Par>(entries) else {
                return work.reject(RSpaceError::HostWorkRejected);
            };
            work.reserve(operations, bytes, bytes)?;
            current_free_map.insert(level, value);
        }
    }
    Some(current_free_map)
}

#[cfg(test)]
mod metered_tests {
    use std::cell::Cell;
    use std::sync::Mutex;

    use super::super::spatial_matcher::SpatialMatcherContext;
    use super::*;

    fn run(context: &mut SpatialMatcherContext<'_>) -> Option<()> {
        context.list_match_single_(
            vec![Par::default(); 2],
            vec![Par::default()],
            &|par, _, _| Some(par),
            Some(0),
            false,
        )
    }

    #[test]
    fn remainder_assignment_preserves_free_map_and_rejects_short_credit() {
        let mut ordinary = SpatialMatcherContext::new();
        assert!(run(&mut ordinary).is_some());
        let expected = ordinary.free_map;
        let used = Mutex::new([0usize; 3]);
        let unlimited = |operations: usize, scanned: usize, backing: usize| {
            let mut totals = used.lock().unwrap();
            for (total, amount) in totals.iter_mut().zip([operations, scanned, backing]) {
                *total += amount;
            }
            Ok(())
        };
        let mut context = SpatialMatcherContext::with_meter(&unlimited).unwrap();
        assert!(run(&mut context).is_some());
        assert_eq!(context.free_map, expected);
        assert!(context.take_error().is_none());
        let required = *used.lock().unwrap();
        assert!(required.iter().all(|value| *value > 0));
        for dimension in 0..3 {
            let mut limit = required;
            limit[dimension] -= 1;
            let spent = Mutex::new([0usize; 3]);
            let meter = |operations: usize, scanned: usize, backing: usize| {
                let mut next = spent.lock().unwrap();
                let amounts = [operations, scanned, backing];
                if next
                    .iter()
                    .zip(amounts)
                    .zip(limit)
                    .any(|((used, add), max)| *used + add > max)
                {
                    return Err(RSpaceError::HostWorkRejected);
                }
                for (used, add) in next.iter_mut().zip(amounts) {
                    *used += add;
                }
                Ok(())
            };
            match SpatialMatcherContext::with_meter(&meter) {
                Ok(mut context) => {
                    assert!(run(&mut context).is_none());
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

    #[test]
    fn remainder_merger_rejects_before_payload_materialization_or_publication() {
        let meter = |_: usize, _: usize, backing: usize| {
            if backing >= 4096 {
                Err(RSpaceError::HostWorkRejected)
            } else {
                Ok(())
            }
        };
        let mut context = SpatialMatcherContext::with_meter(&meter).unwrap();
        let materialized = Cell::new(false);
        let merger = |par: Par, _: Vec<Par>, work: &MatcherWork<'_>| {
            work.reserve(1, 0, 4096)?;
            materialized.set(true);
            Some(par)
        };
        assert!(context
            .handle_remainder(vec![Par::default()], 0, &merger)
            .is_none());
        assert!(!materialized.get());
        assert!(context.free_map.is_empty());
        assert!(matches!(
            context.take_error(),
            Some(RSpaceError::HostWorkRejected)
        ));
    }
}
