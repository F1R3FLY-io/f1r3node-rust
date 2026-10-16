use models::rhoapi::Par;
use models::rust::utils::FreeMap;
// Changed by D-D1a (D-M2, DR-103): free-map inserts charge through MatcherWork;
// only the tests still name RSpaceError.
// use rspace_plus_plus::rspace::errors::RSpaceError;

// Changed by D-D1a (D-M2, DR-103): whole-tree backing is no longer charged here.
// use shared::rust::collection_backing::tree_backing;

#[derive(Clone, Debug)]
pub enum Pattern<T: Clone> {
    Term(T),
    Remainder(i32),
}

pub(super) trait ListMatch<T: Clone> {
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
                        // Changed by D-D2 (D-M8, DR-104): the predicate reads the target
                        // by reference, so an inspection replaces the copy.
                        // self.reserve_clone(target)?;
                        // if !self.locally_free(target.to_owned(), 0).is_empty() {
                        // Changed by D-O1 (DR-109): block accounting charges inline bytes
                        // once per enclosing block.
                        // self.reserve_inspect(target)?;
                        if !self.locally_free_is_empty(target, 0) {
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
                  merger: &dyn Fn(&mut Par, Vec<$type>),
                  remainder: Option<i32>,
                  wildcard: bool,
              ) -> Option<()> {
                let remainder_count = targets.len().checked_sub(patterns.len())?;
                let mut all_patterns: Vec<Pattern<$type>> = Vec::with_capacity(targets.len());
                if let Some(level) = remainder {
                    for _ in 0..remainder_count {
                        all_patterns.push(Pattern::Remainder(level));
                    }
                }
                all_patterns.extend(patterns.into_iter().map(Pattern::Term));

                // Changed by D-O1 (DR-109): block accounting charges inline bytes
                // once per enclosing block.
                // self.reserve_clone(&self.free_map)?;
                let mut cloned_self = self.clone();
                let _match_function = Box::new(move |pattern, t| cloned_self.match_function(pattern, t));
                // NOTE: Bypassing 'memoizeInHashMap' here
                let mut maximum_bipartite_match: MaximumBipartiteMatch<Pattern<$type>, $type, FreeMap> = MaximumBipartiteMatch::new(_match_function);

                // Changed by D-O1 (DR-109): block accounting charges inline bytes
                // once per enclosing block.
                // self.reserve_clone(&targets)?;
                let matches = maximum_bipartite_match.find_matches(all_patterns, targets.clone())?;

                let mut free_maps = Vec::new();
                for (_, _, free_map) in &matches {
                    // Changed by D-O1 (DR-109): block accounting charges inline bytes
                    // once per enclosing block.
                    // self.reserve_clone(free_map)?;
                    free_maps.push(free_map.clone());
                }

                // Changed by D-O1 (DR-109): block accounting charges inline bytes
                // once per enclosing block.
                // self.reserve_clone(&self.free_map)?;
                let updated_free_map = aggregate_updates(self.free_map.clone(), free_maps);
                self.free_map = updated_free_map;

                let mut remainder_targets = Vec::new();
                for (target, pattern, _) in &matches {
                    if matches!(pattern, Pattern::Remainder(_)) {
                        // Changed by D-O1 (DR-109): block accounting charges inline bytes
                        // once per enclosing block.
                        // self.reserve_clone(target)?;
                        remainder_targets.push(target.clone());
                    }
                }

                let mut remainder_targets_sorted = Vec::new();
                for target in &targets {
                    // Changed by D-O1 (DR-109): block accounting charges inline bytes
                    // once per enclosing block.
                    // self.reserve_inspect(target)?;
                    // self.reserve_inspect(&remainder_targets)?;
                    if remainder_targets.contains(target) {
                        // Changed by D-O1 (DR-109): block accounting charges inline bytes
                        // once per enclosing block.
                        // self.reserve_clone(target)?;
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
                // Changed by D-D1a (D-M2, DR-103): one B-tree search reads one
                // root-to-leaf path of i32 keys, not the whole map.
                // self.reserve_inspect(&self.free_map)?;
                // Changed by D-D1b (D-M2, DR-103): an existing binding is merged in
                // place, without a copy, and a new binding is inserted only after its
                // merge.
                // let remainder_par = match self.free_map.get(&level) {
                //     Some(par) => {
                //         self.reserve_clone(par)?;
                //         par.clone()
                //     }
                //     None => vector_par(Vec::new(), false),
                // };
                //
                // let work = self.work()?;
                // let remainder_par_updated = merger(remainder_par, remainder_targets, &work)?;

                // Changed by D-D1a (D-M2, DR-103): whole-tree backing on every insert,
                // the pattern DR-77 replaced; the insert now charges its search, its
                // moves and the growth of the tree.
                // let Some(entries) = self.free_map.len().checked_add(1) else {
                //     return self.reject(rspace_plus_plus::rspace::errors::RSpaceError::HostWorkRejected);
                // };
                // let Some((operations, bytes)) = shared::rust::collection_backing::tree_backing::<i32, Par>(entries) else {
                //     return self.reject(rspace_plus_plus::rspace::errors::RSpaceError::HostWorkRejected);
                // };
                // self.reserve(operations, bytes, bytes)?;
                // Changed by D-D1b (D-M2, DR-103): only a new binding is inserted.
                // self.reserve_free_map_insert(self.free_map.len())?;
                // self.free_map.insert(level, remainder_par_updated);
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

              /*
                  'The Box is used here to store the function on the heap rather than the stack. This is because the size of the function is not known at
                compile time (it's a closure that captures its environment), so it cannot be stored directly on the stack. The Box provides a fixed-size
                pointer to the function on the heap, which can be stored on the stack.' - GPT-4
              */
              fn match_function(&mut self, pattern: Pattern<$type>, t: $type) -> Option<FreeMap> {
                let match_effect: Option<()> = match pattern {
                  Pattern::Term(p) => {
                     // Changed by D-D2 (D-M8, DR-104): the predicate reads the pattern
                     // by reference, so an inspection replaces the copy.
                     // self.reserve_clone(&p)?;
                     // if !self.connective_used(p.clone()) {
                     // Changed by D-O1 (DR-109): block accounting charges inline bytes
                     // once per enclosing block.
                     // self.reserve_inspect(&p)?;
                     if !self.connective_used_ref(&p) {
                         // Changed by D-O1 (DR-109): block accounting charges inline bytes
                         // once per enclosing block.
                         // self.reserve_inspect(&t)?;
                         // self.reserve_inspect(&p)?;
                         guard(t == p)
                      } else {
                         self.spatial_match(t, p)
                      }
                  }
                  Pattern::Remainder(_) => {
                      // Changed by D-O1 (DR-109): block accounting charges inline bytes
                      // once per enclosing block.
                      // self.reserve_inspect(&t)?;
                      // Changed by D-D2 (D-M8, DR-104): the predicate reads the target
                      // by reference and builds no union bitset.
                      // guard(self.locally_free(t, 0).is_empty())
                      guard(self.locally_free_is_empty(&t, 0))
                  }
                };

                match match_effect {
                  Some(_) => {
                    let free_map = &self.free_map;
                    // Changed by D-O1 (DR-109): block accounting charges inline bytes
                    // once per enclosing block.
                    // self.reserve_clone(free_map)?;
                    Some(free_map.clone())},
                  None => None,
                }
              }
          }
      )*
  };
}

pub(super) fn aggregate_updates(mut current_free_map: FreeMap, free_maps: Vec<FreeMap>) -> FreeMap {
    for free_map in free_maps {
        for (level, value) in free_map {
            // Changed by D-D1a (D-M2, DR-103): one insert charges its search, its
            // moves and the growth of the tree, not a whole-map walk and the
            // whole-tree backing.
            // work.reserve_inspect(&current_free_map)?;
            // let Some(entries) = current_free_map.len().checked_add(1) else {
            //     return work.reject(RSpaceError::HostWorkRejected);
            // };
            // let Some((operations, bytes)) = tree_backing::<i32, Par>(entries) else {
            //     return work.reject(RSpaceError::HostWorkRejected);
            // };
            // work.reserve(operations, bytes, bytes)?;
            current_free_map.insert(level, value);
        }
    }
    current_free_map
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
