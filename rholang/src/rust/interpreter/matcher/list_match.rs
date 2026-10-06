use std::collections::HashSet;

use models::rhoapi::Par;
use models::rust::utils::FreeMap;
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

pub trait ListMatch<T: Clone> {
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

    /// Borrowed hot path used by the relational matcher. The owned method is
    /// retained as the compatibility surface for direct callers.
    fn match_function_ref(&mut self, pattern: &Pattern<T>, t: &T) -> Option<FreeMap>;
}

// Rust extension doesn't auto-format custom macros.
#[macro_export]
macro_rules! list_match {
  ($($type:ty),*) => {
      $(
          impl ListMatch<$type> for SpatialMatcherContext {
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

                // println!("\nHit list_match_single_");
                // println!("\ntlist: {:#?}", tlist);
                // println!("\nplist: {:#?}", plist);
                // let merger_type_name = std::any::type_name::<&dyn Fn(Par, Vec<$type>) -> Par>();
                // println!("merger: {:?}", merger_type_name);
                // println!("remainder: {:#?}", remainder);
                // println!("wildcard: {:#?}", wildcard);

                let exact_match = !wildcard && remainder.is_none();
                let plen = plist.len();
                let tlen = tlist.len();

                let result: Option<()> = if exact_match && plen != tlen {
                    // println!("\nreturning None in list_match_single_");
                    None
                } else if plen > tlen {
                    // println!("\nreturning None in list_match_single_");
                    None
                } else if plen == 0 && tlen == 0 && remainder.is_none() {
                    // println!("\ncurrent free_map: {:?}", self.free_map);
                    // println!("\nreturning Some in list_match_single_");
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
                    // println!("calling list_match");
                    self.list_match(tlist, plist, merger, remainder, wildcard)
                };
                // println!("\nend of list_match_single_");
                result
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
                // println!("\nHit list_match");
                // println!("\nlist_match targets: {:?}", targets);
                // println!("\nlist_match patterns: {:?}", patterns);

                let remainder_count = targets.len().checked_sub(patterns.len())?;
                let mut all_patterns: Vec<Pattern<$type>> = Vec::new();
                self.reserve_vec(&mut all_patterns, targets.len())?;
                if let Some(level) = remainder {
                    for _ in 0..remainder_count {
                        all_patterns.push(Pattern::Remainder(level));
                    }
                }
                all_patterns.extend(patterns.into_iter().map(Pattern::Term));

                // println!("\nlist_match all_patterns: {:?}", all_patterns);

                self.reserve_clone(&self.free_map)?;
                let mut cloned_self = self.clone();
                self.reserve(1, 0, std::mem::size_of::<SpatialMatcherContext<'_>>() + 2 * std::mem::size_of::<usize>())?;
                let _match_function: Box<
                    dyn for<'pattern, 'target> FnMut(
                        &'pattern Pattern<$type>,
                        &'target $type,
                    ) -> Option<FreeMap>,
                > = Box::new(
                    move |pattern: &Pattern<$type>, t: &$type| {
                        cloned_self.match_function_ref(pattern, t)
                    },
                );
                // D-E4: retain the now-pure structural edge relation across augmenting
                // paths. The matcher stores only successful edges plus one scanned-prefix
                // frontier per row, so failed pairs never form a dense |P|×|T| matrix.
                // Remainder fillers are deliberately non-cacheable: they are cheap,
                // intentionally dense, bind nothing inside the bipartite match, and would
                // otherwise retain Θ((|T|-|fixed|)·|T|) identical FreeMaps. Term rows —
                // the actual structural AC obligations — receive exact lazy reuse.
                let _cache_structural_term = Box::new(|pattern: &Pattern<$type>| {
                    matches!(pattern, Pattern::Term(_))
                });
                let mut maximum_bipartite_match: MaximumBipartiteMatch<Pattern<$type>, $type, FreeMap> =
                    MaximumBipartiteMatch::new_with_cache_policy(
                        _match_function,
                        _cache_structural_term,
                    );

                // println!("\ncurrent free_map: {:?}", self.free_map);

                self.reserve_clone(&targets)?;
                let work = self.work()?;
                let matches = maximum_bipartite_match
                    .find_indexed_matches_metered(all_patterns, targets.clone(), &work)?;

                // println!("\nfree_map after MBM: {:?}", self.free_map);
                // println!("\nmatches: {:?}", matches);

                let mut free_maps: Vec<FreeMap> = Vec::new();
                for (_, _, _, free_map) in &matches {
                    self.reserve_clone(free_map)?;
                    self.reserve_vec(&mut free_maps, 1)?;
                    free_maps.push(free_map.clone());
                }

                self.reserve_clone(&self.free_map)?;
                let updated_free_map = aggregate_updates_metered(self.free_map.clone(), free_maps, &work)?;
                // println!("\nsetting free_map in list_match");
                self.free_map = updated_free_map;
                // println!("\nnew free_map: {:?}", self.free_map);

                self.reserve(1, 0, targets.len())?;
                let mut remainder_target_indices = vec![false; targets.len()];
                for (target_index, _, pattern, _) in &matches {
                    if matches!(pattern, Pattern::Remainder(_)) {
                        remainder_target_indices[*target_index] = true;
                    }
                }

                let mut remainder_targets_sorted: Vec<$type> = Vec::new();
                for (target_index, target) in targets.iter().enumerate() {
                    self.reserve_inspect(target)?;
                    if remainder_target_indices[target_index] {
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
                // println!("\nhit handle_remainder");
                // println!("remainder_targets: {:#?}", remainder_targets);
                // println!("level: {:#?}", level);
                // let merger_type_name = std::any::type_name::<&dyn Fn(Par, Vec<$type>) -> Par>();
                // println!("merger: {:?}\n", merger_type_name);

                self.reserve_inspect(&self.free_map)?;
                let remainder_par = match self.free_map.get(&level) {
                    Some(par) => {
                        self.reserve_clone(par)?;
                        par.clone()
                    }
                    None => vector_par(Vec::new(), false),
                };

                // println!("\nremainder_par: {:#?}", remainder_par);

                let work = self.work()?;
                let remainder_par_updated = merger(remainder_par, remainder_targets, &work)?;

                // println!("\nremainder_par_updated: {:#?}", remainder_par_updated);

                // println!("\nmodifying free_map in handle_remainder");
                let entries = self.free_map.len().checked_add(1)?;
                let (operations, bytes) = tree_backing::<i32, Par>(entries)?;
                self.reserve(operations, bytes, bytes)?;
                self.free_map.insert(level, remainder_par_updated);
                // println!("\ncurrent free_map: {:#?}", self.free_map);

                Some(())
              }

              // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - matchFunction
              //
              // ★★ STATE ISOLATION — task #144. Scala wraps every attempt in
              // `isolateState` (`SpatialMatcher.scala:241`, `:279-287`):
              //
              //     initState   <- get            // snapshot
              //     _           <- f              // run the attempt
              //     resultState <- get            // the post-attempt map IS the value
              //     _           <- set(initState) // the caller's map is put back
              //     yield resultState
              //
              // In `StateT`/`StreamT` a failed attempt is an EMPTY STREAM, so its
              // state never reaches the caller at all and the explicit
              // `set(initState)` is only needed on the success path. Rust mutates a
              // plain field, so failure is DESTRUCTIVE and both paths must restore.
              //
              // Without the restore, `list_match` builds `cloned_self` once, moves it
              // into the closure, and every attempt in the whole bipartite search
              // mutates that one map. Each claimed match's snapshot is then CUMULATIVE
              // — it carries the bindings written by every FAILED attempt that ran
              // before it — and `aggregate_updates` folds those snapshots in `matches`
              // BTreeMap order, which is structural `Par` order, i.e. prost's
              // field-declaration derive. Which of two conflicting values survived was
              // decided by a `.proto` field number rather than by the semantics.
              //
              // ★ THE SNAPSHOT IS TAKEN ON EXACTLY ONE ARM, and that is a proof
              // rather than a heuristic. `guard(t == p)` is a pure comparison and
              // `guard(self.locally_free(t, 0).is_empty())` is a pure query — neither
              // can reach `self.free_map` — and `connective_used(p)` is the predicate
              // this function ALREADY branches on. The failure-heavy non-connective
              // path pays nothing.
              //
              // 'The Box is used here to store the function on the heap rather than the stack. This is because the size of the function is not known at
              // compile time (it's a closure that captures its environment), so it cannot be stored directly on the stack. The Box provides a fixed-size
              // pointer to the function on the heap, which can be stored on the stack.' - GPT-4
              fn match_function(&mut self, pattern: Pattern<$type>, t: $type) -> Option<FreeMap> {
                self.match_function_ref(&pattern, &t)
              }

              fn match_function_ref(&mut self, pattern: &Pattern<$type>, t: &$type) -> Option<FreeMap> {
                match pattern {
                  Pattern::Term(p) => {
                     self.reserve_clone(p)?;
                     if !self.connective_used(p.clone()) {
                         // Pure arm: no write to `free_map` is reachable from here, so
                         // the pre-attempt and post-attempt maps are the same map and
                         // no snapshot is needed.
                         self.reserve_inspect(t)?;
                         self.reserve_inspect(p)?;
                         guard(t == p)?;
                         self.reserve_clone(&self.free_map)?;
                         Some(self.free_map.clone())
                      } else {
                         // `initState <- get` — the ONLY new clone this fix introduces.
                         let __isolate_start = std::time::Instant::now();
                         self.reserve_clone(&self.free_map)?;
                         let init_state = self.free_map.clone();
                         ::metrics::counter!(
                             $crate::rust::interpreter::metrics_constants::RHOLANG_MATCHER_ISOLATE_STATE_CLONE_ENTRIES_METRIC,
                             "source" => $crate::rust::interpreter::metrics_constants::RHOLANG_METRICS_SOURCE
                         )
                         .increment(init_state.len() as u64);
                         ::metrics::counter!(
                             $crate::rust::interpreter::metrics_constants::RHOLANG_MATCHER_ISOLATE_STATE_CLONE_NS_METRIC,
                             "source" => $crate::rust::interpreter::metrics_constants::RHOLANG_METRICS_SOURCE
                         )
                         .increment(__isolate_start.elapsed().as_nanos() as u64);

                         self.reserve_clone(t)?;
                         self.reserve_clone(p)?;
                         let effect = self.spatial_match(t.clone(), p.clone());

                         // `resultState <- get; set(initState); yield resultState` as
                         // ONE move. `std::mem::replace` — not clone-then-assign: the
                         // restore itself is free, only the entry snapshot costs.
                         // Unconditional, so the failure path restores too.
                         let result_state = std::mem::replace(&mut self.free_map, init_state);
                         effect.map(|_| result_state)
                      }
                  }
                  // Remainders can't match non-concrete terms, because they can't be
                  // captured. They match everything that's concrete. Pure arm — the
                  // remainder's own binding happens AFTER the bipartite match, on
                  // `self`, in `handle_remainder`.
                  Pattern::Remainder(_) =>
                      {
                          self.reserve_clone(t)?;
                          guard(self.locally_free(t.clone(), 0).is_empty())?;
                          self.reserve_clone(&self.free_map)?;
                          Some(self.free_map.clone())
                      },
                }
              }
          }
      )*
  };
}

/// The first free-variable level that two of `free_maps` **both** added, if any.
///
/// This is the predicate of Scala's `aggregateUpdates`
/// (`SpatialMatcher.scala:294-311`), extracted so that it can be tested on its
/// own and so that a refusal can *name* the offending level instead of
/// answering with a bare boolean:
///
/// ```text
///   addedVars = freeMaps.flatMap(_.keys.filterNot(currentVars.contains))
///   ensure(addedVars.size == addedVars.distinct.size)
/// ```
///
/// ★ **`addedVars` is a `Seq`, so multiplicity is kept**, and the size-vs-distinct
/// comparison is a real test. The Rust port collected into a `HashSet` first and
/// then compared that set's length against
/// `set.iter().collect::<HashSet<_>>().len()` — identical *by construction*, so
/// the guard was a tautology that could never fire (task #144). Iterating the
/// keys directly, in order, and refusing on the first repeat restores the check.
///
/// ★ **`filterNot(currentVars.contains)` is load-bearing and is kept exactly.**
/// Every map returned by a state-isolated `match_function` is
/// `current_free_map + delta`, so the levels already present in
/// `current_free_map` appear in *every* aggregated map. Without the filter the
/// shared prefix would read as a duplicate and every ordinary aggregation would
/// be refused.
///
/// Iteration is over `BTreeMap::keys`, which is ascending, and over `free_maps`
/// in the order the caller supplied — so the level reported is deterministic.
pub fn first_duplicate_added_var(current_free_map: &FreeMap, free_maps: &[FreeMap]) -> Option<i32> {
    let mut seen: HashSet<i32> = HashSet::with_capacity(free_maps.iter().map(|m| m.len()).sum());

    free_maps
        .iter()
        .flat_map(|free_map| free_map.keys())
        .filter(|level| !current_free_map.contains_key(*level))
        .find(|level| !seen.insert(**level))
        .copied()
}

/// Merge claimed maps in deterministic source order. Repeated levels are last-wins,
/// including malformed/foreign patterns: duplicate rejection would change consensus.
pub fn aggregate_updates(current_free_map: FreeMap, free_maps: Vec<FreeMap>) -> Option<FreeMap> {
    aggregate_updates_inner(current_free_map, free_maps, None)
}

pub(super) fn aggregate_updates_metered(
    current_free_map: FreeMap,
    free_maps: Vec<FreeMap>,
    work: &MatcherWork<'_>,
) -> Option<FreeMap> {
    aggregate_updates_inner(current_free_map, free_maps, Some(work))
}

fn aggregate_updates_inner(
    mut current_free_map: FreeMap,
    free_maps: Vec<FreeMap>,
    work: Option<&MatcherWork<'_>>,
) -> Option<FreeMap> {
    for free_map in free_maps {
        for (level, value) in free_map {
            if let Some(work) = work {
                work.reserve_inspect(&current_free_map)?;
                let entries = current_free_map.len().checked_add(1)?;
                let (operations, bytes) = tree_backing::<i32, Par>(entries)?;
                work.reserve(operations, bytes, bytes)?;
            }
            current_free_map.insert(level, value);
        }
    }
    Some(current_free_map)
}
