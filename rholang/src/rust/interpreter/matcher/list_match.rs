use std::collections::HashSet;

use models::rhoapi::Par;
use models::rust::utils::FreeMap;

#[derive(Clone, Debug)]
pub enum Pattern<T: Clone> {
    Term(T),
    Remainder(i32),
}

pub trait ListMatch<T: Clone> {
    // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - listMatchSingle
    fn list_match_single(&mut self, tlist: Vec<T>, plist: Vec<T>) -> Option<()>;

    // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - listMatchSingle_
    fn list_match_single_(
        &mut self,
        tlist: Vec<T>,
        plist: Vec<T>,
        merger: &dyn Fn(&mut Par, Vec<T>),
        remainder: Option<i32>,
        wildcard: bool,
    ) -> Option<()>;

    // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - listMatch
    fn list_match(
        &mut self,
        targets: Vec<T>,
        patterns: Vec<T>,
        merger: &dyn Fn(&mut Par, Vec<T>),
        remainder: Option<i32>,
        wildcard: bool,
    ) -> Option<()>;

    // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - handleRemainder
    fn handle_remainder(
        &mut self,
        remainder_targets: Vec<T>,
        level: i32,
        merger: &dyn Fn(&mut Par, Vec<T>),
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
                let _merger: &dyn Fn(&mut Par, Vec<$type>) = &|_, _| ();

                self.list_match_single_(tlist, plist, _merger, None, false)
              }

              // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - listMatchSingle_
              fn list_match_single_(
                  &mut self,
                  tlist: Vec<$type>,
                  plist: Vec<$type>,
                  merger: &dyn Fn(&mut Par, Vec<$type>),
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
                  merger: &dyn Fn(&mut Par, Vec<$type>),
                  remainder: Option<i32>,
                  wildcard: bool,
              ) -> Option<()> {
                // println!("\nHit list_match");
                // println!("\nlist_match targets: {:?}", targets);
                // println!("\nlist_match patterns: {:?}", patterns);

                let remainder_count = targets.len().checked_sub(patterns.len())?;
                let mut all_patterns: Vec<Pattern<$type>> = Vec::with_capacity(targets.len());
                if let Some(level) = remainder {
                    for _ in 0..remainder_count {
                        all_patterns.push(Pattern::Remainder(level));
                    }
                }
                all_patterns.extend(patterns.into_iter().map(Pattern::Term));

                // println!("\nlist_match all_patterns: {:?}", all_patterns);

                let mut cloned_self = self.clone();
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
                let mut maximum_bipartite_match: MaximumBipartiteMatch<'_, Pattern<$type>, $type, FreeMap> =
                    MaximumBipartiteMatch::new_with_cache_policy(
                        _match_function,
                        _cache_structural_term,
                    );

                // println!("\ncurrent free_map: {:?}", self.free_map);

                let matches = maximum_bipartite_match
                    .find_indexed_matches(all_patterns, targets.clone())?;

                // println!("\nfree_map after MBM: {:?}", self.free_map);
                // println!("\nmatches: {:?}", matches);

                let mut free_maps: Vec<FreeMap> = Vec::new();
                for (_, _, _, free_map) in &matches {
                    free_maps.push(free_map.clone());
                }

                let updated_free_map = aggregate_updates(self.free_map.clone(), free_maps)?;
                // println!("\nsetting free_map in list_match");
                self.free_map = updated_free_map;
                // println!("\nnew free_map: {:?}", self.free_map);

                let mut remainder_target_indices = vec![false; targets.len()];
                for (target_index, _, pattern, _) in &matches {
                    if matches!(pattern, Pattern::Remainder(_)) {
                        remainder_target_indices[*target_index] = true;
                    }
                }

                let mut remainder_targets_sorted: Vec<$type> = Vec::new();
                for (target_index, target) in targets.iter().enumerate() {
                    if remainder_target_indices[target_index] {
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
                  merger: &dyn Fn(&mut Par, Vec<$type>),
              ) -> Option<()> {
                match self.free_map.get_mut(&level) {
                    Some(par) => merger(par, remainder_targets),
                    None => {
                        let mut par = vector_par(Vec::new(), false);
                        merger(&mut par, remainder_targets);
                        self.free_map.insert(level, par);
                    }
                }
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
                     if !self.connective_used(p.clone()) {
                         // Pure arm: no write to `free_map` is reachable from here, so
                         // the pre-attempt and post-attempt maps are the same map and
                         // no snapshot is needed.
                         guard(t == p)?;
                         Some(self.free_map.clone())
                      } else {
                         // `initState <- get` — the ONLY new clone this fix introduces.
                         let __isolate_start = std::time::Instant::now();
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
                          guard(self.locally_free(t.clone(), 0).is_empty())?;
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
pub fn aggregate_updates(
    mut current_free_map: FreeMap,
    free_maps: Vec<FreeMap>,
) -> Option<FreeMap> {
    for free_map in free_maps {
        for (level, value) in free_map {
            current_free_map.insert(level, value);
        }
    }
    Some(current_free_map)
}

#[cfg(test)]
mod tests {
    use models::rhoapi::expr::ExprInstance::{EMapBody, ESetBody};
    use models::rhoapi::{
        Bundle, CostSignedTerm, CostStack, EMap, ESet, Expr, GUnforgeable, If, KeyValuePair, Match,
        Receive,
    };

    use super::super::spatial_matcher::{
        merge_map_remainder, merge_set_remainder, SpatialMatcherContext,
    };
    use super::*;
    use crate::rust::interpreter::accounting::random_par_term as term;

    /// The remainder merge before D-D1b, frozen as the oracle: copy the binding
    /// at the level, or start a new one, merge the copy, then insert it.
    fn legacy_copy_merge<T>(
        free_map: &FreeMap,
        targets: Vec<T>,
        level: i32,
        merger: &dyn Fn(Par, Vec<T>) -> Par,
    ) -> FreeMap {
        let start = match free_map.get(&level) {
            Some(par) => par.clone(),
            None => models::rust::rholang::implicits::vector_par(Vec::new(), false),
        };
        let merged = merger(start, targets);
        let mut result = free_map.clone();
        result.insert(level, merged);
        result
    }

    /// The set merger before D-D1b, frozen as the oracle.
    fn legacy_merge_set_remainder(mut p: Par, r: Vec<Par>) -> Par {
        let mut unique = Vec::with_capacity(r.len());
        for element in r {
            if !unique.contains(&element) {
                unique.push(element);
            }
        }
        p.exprs = vec![Expr {
            expr_instance: Some(ESetBody(ESet {
                ps: unique,
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }];
        p
    }

    /// The map merger before D-D1b, frozen as the oracle.
    fn legacy_merge_map_remainder(mut p: Par, r: Vec<(Par, Par)>) -> Par {
        let mut unique = Vec::with_capacity(r.len());
        for (key, value) in r {
            if let Some(index) = unique
                .iter()
                .position(|(existing, _): &(Par, Par)| existing == &key)
            {
                unique[index].1 = value;
            } else {
                unique.push((key, value));
            }
        }
        let mut kvs = Vec::with_capacity(unique.len());
        for (key, value) in unique {
            kvs.push(KeyValuePair {
                key: Some(key),
                value: Some(value),
            });
        }
        p.exprs = vec![Expr {
            expr_instance: Some(EMapBody(EMap {
                kvs,
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }];
        p
    }

    fn merged_in_place<T: Clone>(
        free_map: &FreeMap,
        targets: Vec<T>,
        merger: &dyn Fn(&mut Par, Vec<T>),
    ) -> FreeMap
    where
        SpatialMatcherContext: ListMatch<T>,
    {
        let mut context = SpatialMatcherContext::new();
        context.free_map = free_map.clone();
        context
            .handle_remainder(targets, 0, merger)
            .expect("the remainder binds");
        context.free_map
    }

    fn field_merge_matches_copy<T: Clone>(
        free_map: &FreeMap,
        targets: Vec<T>,
        assign: fn(&mut Par, Vec<T>),
    ) -> Result<(), proptest::test_runner::TestCaseError>
    where
        SpatialMatcherContext: ListMatch<T>,
    {
        let in_place = merged_in_place(free_map, targets.clone(), &|par, values| {
            assign(par, values)
        });
        let copy = legacy_copy_merge(free_map, targets, 0, &|mut par, values| {
            assign(&mut par, values);
            par
        });
        proptest::prop_assert_eq!(in_place, copy);
        Ok(())
    }

    fn set_targets(elements: &[Par]) -> Vec<Par> {
        let mut targets = elements.to_vec();
        targets.extend(elements.first().cloned());
        targets
    }

    fn map_targets(elements: &[Par]) -> Vec<(Par, Par)> {
        let mut targets: Vec<(Par, Par)> = elements
            .iter()
            .cloned()
            .zip(elements.iter().rev().cloned())
            .collect();
        targets.extend(elements.first().cloned().map(|key| (key, Par::default())));
        targets
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(128))]

        /// D-D1b (DR-103): for each of the ten field mergers of a Par remainder,
        /// the set merger and the map merger, the in-place merge gives the free
        /// map of the copy merge, with and without an existing binding.
        #[test]
        fn remainder_merge_matches_legacy_copy_merge(
            existing in proptest::option::of(term()),
            others in proptest::collection::vec((1i32..6, term()), 0..3),
            source in term(),
            elements in proptest::collection::vec(term(), 0..4),
            count in 0usize..3,
        ) {
            let mut free_map: FreeMap = others.into_iter().collect();
            if let Some(existing) = existing {
                free_map.insert(0, existing);
            }
            field_merge_matches_copy(&free_map, source.sends.clone(), |p, values| p.sends = values)?;
            field_merge_matches_copy(&free_map, vec![Receive::default(); count], |p, values| p.receives = values)?;
            field_merge_matches_copy(&free_map, source.news.clone(), |p, values| p.news = values)?;
            field_merge_matches_copy(&free_map, source.exprs.clone(), |p, values| p.exprs = values)?;
            field_merge_matches_copy(&free_map, vec![Match::default(); count], |p, values| p.matches = values)?;
            field_merge_matches_copy(&free_map, vec![Bundle::default(); count], |p, values| p.bundles = values)?;
            field_merge_matches_copy(&free_map, vec![GUnforgeable::default(); count], |p, values| p.unforgeables = values)?;
            field_merge_matches_copy(&free_map, vec![If::default(); count], |p, values| p.conditionals = values)?;
            field_merge_matches_copy(&free_map, vec![CostSignedTerm::default(); count], |p, values| p.cost_signed_terms = values)?;
            field_merge_matches_copy(&free_map, vec![CostStack::default(); count], |p, values| p.cost_stacks = values)?;

            let in_place = merged_in_place(&free_map, set_targets(&elements), &merge_set_remainder);
            let copy = legacy_copy_merge(&free_map, set_targets(&elements), 0, &legacy_merge_set_remainder);
            proptest::prop_assert_eq!(in_place, copy);
            let in_place = merged_in_place(&free_map, map_targets(&elements), &merge_map_remainder);
            let copy = legacy_copy_merge(&free_map, map_targets(&elements), 0, &legacy_merge_map_remainder);
            proptest::prop_assert_eq!(in_place, copy);
        }
    }
}
