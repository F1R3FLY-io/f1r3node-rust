use std::collections::HashSet;

use models::rhoapi::Par;
use models::rust::utils::FreeMap;
use shared::rust::clone_backing::{BackingError, CloneBacking, Walker};

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
                        self.reserve_blocks_copy_and_cleanup(target)?;
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

                self.reserve_blocks_copy_and_cleanup(&self.free_map)?;
                let mut cloned_self = self.clone();
                self.reserve(1, 0, std::mem::size_of::<SpatialMatcherContext<'_>>() + 2 * std::mem::size_of::<usize>())?;
                let _match_function: Box<
                    dyn for<'pattern, 'target> FnMut(
                        &'pattern Pattern<$type>,
                        &'target $type,
                    ) -> Option<FreeMap> + 'a,
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
                let mut maximum_bipartite_match: MaximumBipartiteMatch<'a, Pattern<$type>, $type, FreeMap> =
                    MaximumBipartiteMatch::new_with_cache_policy(
                        _match_function,
                        _cache_structural_term,
                    );

                // println!("\ncurrent free_map: {:?}", self.free_map);

                self.reserve_blocks_copy_and_cleanup(&targets)?;
                let work = self.work()?;
                let matches = maximum_bipartite_match
                    .find_indexed_matches_metered(all_patterns, targets.clone(), &work)?;

                // println!("\nfree_map after MBM: {:?}", self.free_map);
                // println!("\nmatches: {:?}", matches);

                let mut free_maps: Vec<FreeMap> = Vec::new();
                for (_, _, _, free_map) in &matches {
                    self.reserve_blocks_copy_and_cleanup(free_map)?;
                    self.reserve_vec(&mut free_maps, 1)?;
                    free_maps.push(free_map.clone());
                }

                self.reserve_blocks_copy_and_cleanup(&self.free_map)?;
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
                    self.inspect_blocks(target)?;
                    if remainder_target_indices[target_index] {
                        self.reserve_blocks_copy_and_cleanup(target)?;
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
                self.reserve_free_map_search(self.free_map.len())?;
                let remainder_par = match self.free_map.get(&level) {
                    Some(par) => {
                        self.reserve_blocks_copy_and_cleanup(par)?;
                        par.clone()
                    }
                    None => vector_par(Vec::new(), false),
                };

                // println!("\nremainder_par: {:#?}", remainder_par);

                let work = self.work()?;
                let remainder_par_updated = merger(remainder_par, remainder_targets, &work)?;

                self.reserve_free_map_insert(self.free_map.len())?;
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
                     self.reserve_blocks_copy_and_cleanup(p)?;
                     if !self.connective_used(p.clone()) {
                         // Pure arm: no write to `free_map` is reachable from here, so
                         // the pre-attempt and post-attempt maps are the same map and
                         // no snapshot is needed.
                         self.inspect_blocks(t)?;
                         self.inspect_blocks(p)?;
                         guard(t == p)?;
                         self.reserve_blocks_copy_and_cleanup(&self.free_map)?;
                         Some(self.free_map.clone())
                      } else {
                         // `initState <- get` — the ONLY new clone this fix introduces.
                         let __isolate_start = std::time::Instant::now();
                         self.reserve_blocks_copy_and_cleanup(&self.free_map)?;
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

                         self.reserve_blocks_copy_and_cleanup(t)?;
                         self.reserve_blocks_copy_and_cleanup(p)?;
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
                          self.reserve_blocks_copy_and_cleanup(t)?;
                          guard(self.locally_free(t.clone(), 0).is_empty())?;
                          self.reserve_blocks_copy_and_cleanup(&self.free_map)?;
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
                work.reserve_free_map_insert(current_free_map.len())?;
            }
            current_free_map.insert(level, value);
        }
    }
    Some(current_free_map)
}

#[cfg(test)]
mod metered_tests {
    use std::cell::Cell;
    use std::sync::Mutex;

    use rspace_plus_plus::rspace::errors::RSpaceError;

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

    /// The three totals that a metered closure receives during `action`.
    fn charge_of(action: impl FnOnce(&mut SpatialMatcherContext<'_>)) -> [usize; 3] {
        let totals = Mutex::new([0usize; 3]);
        let meter = |operations: usize, scanned: usize, backing: usize| {
            let mut sum = totals.lock().expect("totals lock");
            for (total, amount) in sum.iter_mut().zip([operations, scanned, backing]) {
                *total += amount;
            }
            Ok(())
        };
        let mut context = SpatialMatcherContext::with_meter(&meter).expect("metered context");
        let before = *totals.lock().expect("totals lock");
        action(&mut context);
        assert!(context.take_error().is_none());
        let after = *totals.lock().expect("totals lock");
        [
            after[0] - before[0],
            after[1] - before[1],
            after[2] - before[2],
        ]
    }

    fn payload(size: usize) -> Par {
        let mut par = Par::default();
        par.exprs = vec![models::rust::utils::new_gint_expr(7); size];
        par
    }

    fn bindings(levels: &[i32], size: usize) -> FreeMap {
        levels.iter().map(|level| (*level, payload(size))).collect()
    }

    fn remainder_charge(free_map: FreeMap) -> [usize; 3] {
        charge_of(|context| {
            context.free_map = free_map;
            context
                .handle_remainder(vec![Par::default()], 0, &|par, _, _| Some(par))
                .expect("the remainder binds");
        })
    }

    /// D-D1a (DR-103): a remainder binding charges one search and one insert,
    /// whatever the size of the other bindings in the free map.
    #[test]
    fn remainder_charge_is_independent_of_other_bindings() {
        for target_present in [false, true] {
            let charges = [1usize, 16].map(|scale| {
                let mut free_map = bindings(&[1, 2, 3, 4], 8 * scale);
                if target_present {
                    free_map.insert(0, payload(3));
                }
                remainder_charge(free_map)
            });
            assert_eq!(charges[0], charges[1], "target present: {target_present}");
            assert!(charges[0][0] > 0 && charges[0][1] > 0);
        }
    }

    /// The charge before D-D1a: a walk of the whole map, then the backing of
    /// the whole tree as scanned and as backing bytes.
    fn legacy_remainder_charge(free_map: &FreeMap) -> [usize; 3] {
        let totals = Mutex::new([0usize; 3]);
        let meter = |operations: usize, scanned: usize, backing: usize| {
            let mut sum = totals.lock().expect("totals lock");
            for (total, amount) in sum.iter_mut().zip([operations, scanned, backing]) {
                *total += amount;
            }
            Ok::<(), BackingError>(())
        };
        shared::rust::clone_backing::inspect(free_map, &meter).expect("legacy inspection");
        let (operations, bytes) =
            shared::rust::collection_backing::tree_backing::<i32, Par>(free_map.len() + 1)
                .expect("legacy tree backing");
        let mut sum = totals.into_inner().expect("totals lock");
        sum[0] += operations;
        sum[1] += bytes;
        sum[2] += bytes;
        sum
    }

    /// Negative control: the legacy charge grew with the other bindings.
    #[test]
    fn legacy_remainder_charge_grew_with_other_bindings() {
        let small = legacy_remainder_charge(&bindings(&[1, 2, 3, 4], 8));
        let large = legacy_remainder_charge(&bindings(&[1, 2, 3, 4], 128));
        assert!(large[0] > small[0]);
        assert!(large[1] > small[1]);
    }

    /// D-D1a (DR-103): the exact search and insert charges at the sizes where
    /// the height bound changes.
    #[test]
    fn free_map_charges_at_boundary_sizes() {
        use shared::rust::collection_backing::{
            tree_insert_moves, tree_insert_transient_backing, tree_search_bound,
        };

        for entries in [0usize, 1, 5, 9, 10, 11, 70, 71, 430, 431] {
            let search = tree_search_bound(entries);
            let (growth_operations, growth_bytes) =
                tree_insert_transient_backing::<i32, Par>(entries)
                    .expect("transient insert backing");
            let moves = tree_insert_moves::<i32, Par>(entries + 1).expect("insert moves");
            assert_eq!(
                charge_of(|context| context
                    .reserve_free_map_search(entries)
                    .expect("search charge")),
                [search, 8 * search, 0],
                "search at {entries}"
            );
            assert_eq!(
                charge_of(|context| context
                    .reserve_free_map_insert(entries)
                    .expect("insert charge")),
                [
                    search + 1 + growth_operations,
                    8 * search + moves,
                    growth_bytes
                ],
                "insert at {entries}"
            );
        }
        assert_eq!(
            charge_of(|context| context.reserve_free_map_insert(0).expect("insert charge")),
            [5, 3_832, 3_472]
        );
        assert_eq!(
            charge_of(|context| context.reserve_free_map_insert(10).expect("insert charge")),
            [18, 7_752, 6_944]
        );
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]

        /// D-D1a (DR-103): from an empty map or a charged clone, the insert
        /// charges cover every node that the std `BTreeMap` allocates.
        #[test]
        fn free_map_insert_reservation_covers_std_btree_allocations(
            keys in proptest::collection::vec(proptest::prelude::any::<i32>(), 1..600),
            base in 0usize..40,
        ) {
            let original: FreeMap = (0..base as i32).map(|key| (key, Par::default())).collect();
            let totals = Mutex::new([0usize; 3]);
            let meter = |operations: usize, scanned: usize, backing: usize| {
                let mut sum = totals.lock().expect("totals lock");
                for (total, amount) in sum.iter_mut().zip([operations, scanned, backing]) {
                    *total += amount;
                }
                Ok(())
            };
            let context = SpatialMatcherContext::with_meter(&meter).expect("metered context");
            let start = totals.lock().expect("totals lock")[2];
            context.reserve_blocks_copy_and_cleanup(&original).expect("clone charge");
            let (mut map, mut allocated) =
                crate::rust::interpreter::accounting::measured_allocations(|| original.clone());
            for key in keys {
                context.reserve_free_map_insert(map.len()).expect("insert charge");
                let (_, used) = crate::rust::interpreter::accounting::measured_allocations(|| {
                    map.insert(key, Par::default());
                });
                allocated += used;
                let reserved = totals.lock().expect("totals lock")[2] - start;
                proptest::prop_assert!(
                    allocated <= reserved,
                    "allocated {} > reserved {} at {} entries",
                    allocated,
                    reserved,
                    map.len()
                );
            }
            proptest::prop_assert!(allocated > 0, "the counting allocator saw the nodes");
        }
    }
}
