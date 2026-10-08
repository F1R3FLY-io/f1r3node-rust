use models::rhoapi::Par;
use models::rust::utils::FreeMap;
// Changed by D-D1a (D-M2, DR-103): free-map inserts charge through MatcherWork;
// only the tests still name RSpaceError.
// use rspace_plus_plus::rspace::errors::RSpaceError;
use shared::rust::clone_backing::{BackingError, CloneBacking, Walker};

// Changed by D-D1a (D-M2, DR-103): whole-tree backing is no longer charged here.
// use shared::rust::collection_backing::tree_backing;
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
        merger: &dyn Fn(&mut Par, Vec<T>, &MatcherWork<'_>) -> Option<()>,
        remainder: Option<i32>,
        wildcard: bool,
    ) -> Option<()>;

    // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - listMatch
    fn list_match(
        &mut self,
        targets: Vec<T>,
        patterns: Vec<T>,
        merger: &dyn Fn(&mut Par, Vec<T>, &MatcherWork<'_>) -> Option<()>,
        remainder: Option<i32>,
        wildcard: bool,
    ) -> Option<()>;

    // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - handleRemainder
    fn handle_remainder(
        &mut self,
        remainder_targets: Vec<T>,
        level: i32,
        merger: &dyn Fn(&mut Par, Vec<T>, &MatcherWork<'_>) -> Option<()>,
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
                let _merger: &dyn Fn(&mut Par, Vec<$type>, &MatcherWork<'_>) -> Option<()> = &|_, _, _| Some(());

                self.list_match_single_(tlist, plist, _merger, None, false)
              }

              // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - listMatchSingle_
              fn list_match_single_(
                  &mut self,
                  tlist: Vec<$type>,
                  plist: Vec<$type>,
                  merger: &dyn Fn(&mut Par, Vec<$type>, &MatcherWork<'_>) -> Option<()>,
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
                        self.inspect_blocks(target)?;
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
                  merger: &dyn Fn(&mut Par, Vec<$type>, &MatcherWork<'_>) -> Option<()>,
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

                // Changed by D-O1 (DR-109): block accounting charges inline bytes
                // once per enclosing block.
                // self.reserve_clone(&self.free_map)?;
                self.reserve_blocks_copy_and_cleanup(&self.free_map)?;
                let mut cloned_self = self.clone();
                self.reserve(1, 0, std::mem::size_of::<SpatialMatcherContext<'a>>() + std::mem::size_of::<usize>() * 2)?;
                let _match_function = Box::new(move |pattern, t| cloned_self.match_function(pattern, t));
                // NOTE: Bypassing 'memoizeInHashMap' here
                let mut maximum_bipartite_match: MaximumBipartiteMatch<'_, Pattern<$type>, $type, FreeMap> = MaximumBipartiteMatch::new(_match_function, self.work()?);

                // Changed by D-O1 (DR-109): block accounting charges inline bytes
                // once per enclosing block.
                // self.reserve_clone(&targets)?;
                self.reserve_blocks_copy_and_cleanup(&targets)?;
                let matches = maximum_bipartite_match.find_matches(all_patterns, targets.clone())?;

                let mut free_maps = Vec::new();
                for (_, _, free_map) in &matches {
                    // Changed by D-O1 (DR-109): block accounting charges inline bytes
                    // once per enclosing block.
                    // self.reserve_clone(free_map)?;
                    self.reserve_blocks_copy_and_cleanup(free_map)?;
                    self.reserve_vec(&mut free_maps, 1)?;
                    free_maps.push(free_map.clone());
                }

                // Changed by D-O1 (DR-109): block accounting charges inline bytes
                // once per enclosing block.
                // self.reserve_clone(&self.free_map)?;
                self.reserve_blocks_copy_and_cleanup(&self.free_map)?;
                let updated_free_map = aggregate_updates(self.free_map.clone(), free_maps, &self.work()?)?;
                self.free_map = updated_free_map;

                let mut remainder_targets = Vec::new();
                for (target, pattern, _) in &matches {
                    if matches!(pattern, Pattern::Remainder(_)) {
                        // Changed by D-O1 (DR-109): block accounting charges inline bytes
                        // once per enclosing block.
                        // self.reserve_clone(target)?;
                        self.reserve_blocks_copy_and_cleanup(target)?;
                        self.reserve_vec(&mut remainder_targets, 1)?;
                        remainder_targets.push(target.clone());
                    }
                }

                let mut remainder_targets_sorted = Vec::new();
                for target in &targets {
                    // Changed by D-O1 (DR-109): block accounting charges inline bytes
                    // once per enclosing block.
                    // self.reserve_inspect(target)?;
                    // self.reserve_inspect(&remainder_targets)?;
                    self.inspect_blocks(target)?;
                    self.inspect_blocks(&remainder_targets)?;
                    // Added by D-E2 (DR-109): `contains` reads the target once
                    // for each remainder target, in lockstep, so it reads no
                    // more of the target than of the remainder targets. A
                    // second inspection of them pays that side
                    // (`MatcherReadsByReference.two_container_inspections_cover_membership_scan`).
                    self.inspect_blocks(&remainder_targets)?;
                    if remainder_targets.contains(target) {
                        // Changed by D-O1 (DR-109): block accounting charges inline bytes
                        // once per enclosing block.
                        // self.reserve_clone(target)?;
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
                  merger: &dyn Fn(&mut Par, Vec<$type>, &MatcherWork<'_>) -> Option<()>,
              ) -> Option<()> {
                // Changed by D-D1a (D-M2, DR-103): one B-tree search reads one
                // root-to-leaf path of i32 keys, not the whole map.
                // self.reserve_inspect(&self.free_map)?;
                self.reserve_free_map_search(self.free_map.len())?;
                // Changed by D-D1b (D-M2, DR-103): an existing binding is merged in
                // place, without a copy, and a new binding is inserted only after its
                // merge. Each merger assigns after all its reservations, so a rejected
                // merge leaves the free map unchanged.
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
                let work = self.work()?;
                match self.free_map.get_mut(&level) {
                    Some(par) => merger(par, remainder_targets, &work)?,
                    None => {
                        let mut par = vector_par(Vec::new(), false);
                        merger(&mut par, remainder_targets, &work)?;
                        self.reserve_free_map_insert(self.free_map.len())?;
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
                     self.inspect_blocks(&p)?;
                     if !self.connective_used_ref(&p) {
                         // Changed by D-O1 (DR-109): block accounting charges inline bytes
                         // once per enclosing block.
                         // self.reserve_inspect(&t)?;
                         // self.reserve_inspect(&p)?;
                         self.inspect_blocks(&t)?;
                         self.inspect_blocks(&p)?;
                         guard(t == p)
                      } else {
                         self.spatial_match(t, p)
                      }
                  }
                  Pattern::Remainder(_) => {
                      // Changed by D-O1 (DR-109): block accounting charges inline bytes
                      // once per enclosing block.
                      // self.reserve_inspect(&t)?;
                      self.inspect_blocks(&t)?;
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
                    self.reserve_blocks_copy_and_cleanup(free_map)?;
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
            work.reserve_free_map_insert(current_free_map.len())?;
            current_free_map.insert(level, value);
        }
    }
    Some(current_free_map)
}

#[cfg(test)]
mod metered_tests {
    use std::cell::Cell;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use models::rhoapi::expr::ExprInstance::{EMapBody, ESetBody};
    use models::rhoapi::{
        Bundle, CostSignedTerm, CostStack, EMap, ESet, Expr, GUnforgeable, If, KeyValuePair, Match,
        Receive,
    };
    use rspace_plus_plus::rspace::errors::RSpaceError;

    use super::super::spatial_matcher::{
        merge_map_remainder, merge_set_remainder, SpatialMatcherContext,
    };
    use super::*;
    use crate::rust::interpreter::accounting::random_par_term as term;

    fn run(context: &mut SpatialMatcherContext<'_>) -> Option<()> {
        context.list_match_single_(
            vec![Par::default(); 2],
            vec![Par::default()],
            &|_, _, _| Some(()),
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
        let merger = |_: &mut Par, _: Vec<Par>, work: &MatcherWork<'_>| {
            work.reserve(1, 0, 4096)?;
            materialized.set(true);
            Some(())
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
        Par {
            exprs: vec![models::rust::utils::new_gint_expr(7); size],
            ..Default::default()
        }
    }

    fn bindings(levels: &[i32], size: usize) -> FreeMap {
        levels.iter().map(|level| (*level, payload(size))).collect()
    }

    fn remainder_charge(free_map: FreeMap) -> [usize; 3] {
        charge_of(|context| {
            context.free_map = free_map;
            context
                .handle_remainder(vec![Par::default()], 0, &|_, _, _| Some(()))
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
        use shared::rust::collection_backing::{tree_growth, tree_insert_moves, tree_search_bound};

        for entries in [0usize, 1, 5, 9, 10, 11, 70, 71, 430, 431] {
            let search = tree_search_bound(entries);
            let (growth_operations, growth_bytes) =
                tree_growth::<i32, Par>(entries, 1).expect("tree growth");
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
            [16, 7_752, 3_472]
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
            // Changed by D-O1 (DR-109): the matcher charges a free-map clone
            // with a block copy, so the test does too.
            // context.reserve_clone(&original).expect("clone charge");
            context
                .reserve_blocks_copy_and_cleanup(&original)
                .expect("clone charge");
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

    /// D-D1b (DR-103): merging into an existing binding charges no copy of
    /// that binding.
    #[test]
    fn in_place_merge_charge_is_independent_of_the_existing_binding() {
        let charges = [8usize, 512].map(|size| {
            let mut free_map = bindings(&[1, 2], 8);
            free_map.insert(0, payload(size));
            remainder_charge(free_map)
        });
        assert_eq!(charges[0], charges[1]);
    }

    /// Negative control: the copy merge before D-D1b charged a copy and a
    /// cleanup of the existing binding, so its charge grew with the binding.
    #[test]
    fn legacy_copy_merge_charge_grew_with_the_existing_binding() {
        let copy_charge = |par: &Par| {
            let totals = Mutex::new([0usize; 3]);
            let meter = |operations: usize, scanned: usize, backing: usize| {
                let mut sum = totals.lock().expect("totals lock");
                for (total, amount) in sum.iter_mut().zip([operations, scanned, backing]) {
                    *total += amount;
                }
                Ok::<(), BackingError>(())
            };
            shared::rust::clone_backing::reserve_copy_and_cleanup(par, &meter)
                .expect("legacy copy charge");
            totals.into_inner().expect("totals lock")
        };
        let small = copy_charge(&payload(8));
        let large = copy_charge(&payload(512));
        assert!(large[1] > small[1]);
        assert!(large[2] > small[2]);
    }

    /// Negative control (`FreeMapBindings.assign_first_merge_changes_map_on_rejection`):
    /// a merger that assigns before a rejected reservation changes the binding
    /// in place, so the in-place merge needs every merger to assign last.
    #[test]
    fn assign_first_merger_changes_the_binding_on_rejection() {
        let meter = |_: usize, _: usize, backing: usize| {
            if backing >= 4096 {
                Err(RSpaceError::HostWorkRejected)
            } else {
                Ok(())
            }
        };
        let mut context = SpatialMatcherContext::with_meter(&meter).expect("metered context");
        let existing = payload(2);
        context.free_map.insert(0, existing.clone());
        let assign_first = |par: &mut Par, _: Vec<Par>, work: &MatcherWork<'_>| {
            par.exprs = Vec::new();
            work.reserve(1, 0, 4096)?;
            Some(())
        };
        assert!(context
            .handle_remainder(vec![Par::default()], 0, &assign_first)
            .is_none());
        assert_ne!(context.free_map.get(&0), Some(&existing));
        let assign_last = |par: &mut Par, _: Vec<Par>, work: &MatcherWork<'_>| {
            work.reserve(1, 0, 4096)?;
            par.exprs = Vec::new();
            Some(())
        };
        let mut context = SpatialMatcherContext::with_meter(&meter).expect("metered context");
        context.free_map.insert(0, existing.clone());
        assert!(context
            .handle_remainder(vec![Par::default()], 0, &assign_last)
            .is_none());
        assert_eq!(context.free_map.get(&0), Some(&existing));
    }

    /// The remainder merge before D-D1b, frozen as the oracle: copy the binding
    /// at the level, or start a new one, merge the copy, then insert it.
    fn legacy_copy_merge<T>(
        free_map: &FreeMap,
        targets: Vec<T>,
        level: i32,
        merger: &dyn Fn(Par, Vec<T>) -> Option<Par>,
    ) -> Option<FreeMap> {
        let start = match free_map.get(&level) {
            Some(par) => par.clone(),
            None => models::rust::rholang::implicits::vector_par(Vec::new(), false),
        };
        let merged = merger(start, targets)?;
        let mut result = free_map.clone();
        result.insert(level, merged);
        Some(result)
    }

    /// The set merger before D-D1b, frozen as the oracle.
    fn legacy_merge_set_remainder(mut p: Par, r: Vec<Par>, work: &MatcherWork<'_>) -> Option<Par> {
        let mut unique = Vec::new();
        for element in r {
            work.reserve_inspect(&element)?;
            work.reserve_inspect(&unique)?;
            if !unique.contains(&element) {
                work.reserve_vec(&mut unique, 1)?;
                unique.push(element);
            }
        }
        let mut exprs = Vec::new();
        work.reserve_vec(&mut exprs, 1)?;
        exprs.push(Expr {
            expr_instance: Some(ESetBody(ESet {
                ps: unique,
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        });
        p.exprs = exprs;
        Some(p)
    }

    /// The map merger before D-D1b, frozen as the oracle.
    fn legacy_merge_map_remainder(
        mut p: Par,
        r: Vec<(Par, Par)>,
        work: &MatcherWork<'_>,
    ) -> Option<Par> {
        let mut unique = Vec::new();
        for (key, value) in r {
            work.reserve_inspect(&key)?;
            work.reserve_inspect(&unique)?;
            if let Some(index) = unique
                .iter()
                .position(|(existing, _): &(Par, Par)| existing == &key)
            {
                unique[index].1 = value;
            } else {
                work.reserve_vec(&mut unique, 1)?;
                unique.push((key, value));
            }
        }
        let mut kvs = Vec::new();
        work.reserve_vec(&mut kvs, unique.len())?;
        for (key, value) in unique {
            kvs.push(KeyValuePair {
                key: Some(key),
                value: Some(value),
            });
        }
        let mut exprs = Vec::new();
        work.reserve_vec(&mut exprs, 1)?;
        exprs.push(Expr {
            expr_instance: Some(EMapBody(EMap {
                kvs,
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        });
        p.exprs = exprs;
        Some(p)
    }

    fn merged_in_place<T: Clone>(
        free_map: &FreeMap,
        targets: Vec<T>,
        merger: &dyn Fn(&mut Par, Vec<T>, &MatcherWork<'_>) -> Option<()>,
    ) -> FreeMap
    where
        for<'a> SpatialMatcherContext<'a>: ListMatch<T>,
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
        for<'a> SpatialMatcherContext<'a>: ListMatch<T>,
    {
        let in_place = merged_in_place(free_map, targets.clone(), &|par, values, _| {
            assign(par, values);
            Some(())
        });
        let copy = legacy_copy_merge(free_map, targets, 0, &|mut par, values| {
            assign(&mut par, values);
            Some(par)
        })
        .expect("the copy merge binds");
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

            let unmetered = SpatialMatcherContext::new();
            let work = unmetered.work().expect("unmetered work");
            let in_place = merged_in_place(&free_map, set_targets(&elements), &merge_set_remainder);
            let copy = legacy_copy_merge(&free_map, set_targets(&elements), 0, &|par, values| {
                legacy_merge_set_remainder(par, values, &work)
            })
            .expect("the copy merge binds");
            proptest::prop_assert_eq!(in_place, copy);
            let in_place = merged_in_place(&free_map, map_targets(&elements), &merge_map_remainder);
            let copy = legacy_copy_merge(&free_map, map_targets(&elements), 0, &|par, values| {
                legacy_merge_map_remainder(par, values, &work)
            })
            .expect("the copy merge binds");
            proptest::prop_assert_eq!(in_place, copy);
        }

        /// D-D1b (DR-103): a set or map merge that the meter rejects at a
        /// reservation leaves the free map unchanged, and an accepted one gives
        /// the copy merge (`FreeMapBindings.rejected_merge_leaves_map`). Each
        /// case counts the reservations of the whole merge, rejects at the
        /// first 16, the last 16 and one random reservation, and then accepts
        /// with the whole budget, so both outcomes occur in every case.
        #[test]
        fn rejected_remainder_merge_leaves_the_free_map(
            existing in proptest::option::of(term()),
            elements in proptest::collection::vec(term(), 0..4),
            map_merge in proptest::prelude::any::<bool>(),
            probe in proptest::prelude::any::<usize>(),
        ) {
            let mut free_map = FreeMap::new();
            if let Some(existing) = existing {
                free_map.insert(0, existing);
            }
            let unmetered = SpatialMatcherContext::new();
            let work = unmetered.work().expect("unmetered work");
            let copy = if map_merge {
                legacy_copy_merge(&free_map, map_targets(&elements), 0, &|par, values| {
                    legacy_merge_map_remainder(par, values, &work)
                })
            } else {
                legacy_copy_merge(&free_map, set_targets(&elements), 0, &|par, values| {
                    legacy_merge_set_remainder(par, values, &work)
                })
            }
            .expect("the copy merge binds");
            let run = |allowed: usize| {
                let calls = AtomicUsize::new(0);
                let meter = |_: usize, _: usize, _: usize| {
                    if calls.fetch_add(1, Ordering::Relaxed) >= allowed {
                        Err(RSpaceError::HostWorkRejected)
                    } else {
                        Ok(())
                    }
                };
                let outcome = SpatialMatcherContext::with_meter(&meter).ok().map(|mut context| {
                    context.free_map = free_map.clone();
                    let merged = if map_merge {
                        context.handle_remainder(map_targets(&elements), 0, &merge_map_remainder)
                    } else {
                        context.handle_remainder(set_targets(&elements), 0, &merge_set_remainder)
                    };
                    let error = context.take_error();
                    (merged, context.free_map, error)
                });
                (outcome, calls.load(Ordering::Relaxed))
            };
            let (whole, needed) = run(usize::MAX);
            let (merged, merged_map, _) = whole.expect("an unlimited meter admits the context");
            proptest::prop_assert!(merged.is_some());
            proptest::prop_assert_eq!(&merged_map, &copy);
            let mut points: Vec<usize> = (0..needed.min(16)).collect();
            points.extend(needed.saturating_sub(16)..needed);
            points.push(probe % needed.max(1));
            points.sort_unstable();
            points.dedup();
            let mut rejections = 0usize;
            for allowed in points {
                let (outcome, _) = run(allowed);
                if let Some((merged, rejected_map, error)) = outcome {
                    proptest::prop_assert!(merged.is_none(), "allowed {} of {}", allowed, needed);
                    proptest::prop_assert_eq!(&rejected_map, &free_map);
                    proptest::prop_assert!(matches!(error, Some(RSpaceError::HostWorkRejected)));
                    rejections += 1;
                }
            }
            proptest::prop_assert!(rejections > 0, "a short budget rejects the merge");
        }
    }

    /// The reservations of one shared block walk, in call order.
    fn walk_calls(
        walk: impl FnOnce(
            &dyn shared::rust::clone_backing::BackingMeter,
        ) -> Result<(), shared::rust::clone_backing::BackingError>,
    ) -> Vec<[usize; 3]> {
        let log = std::cell::RefCell::new(Vec::new());
        let meter = |operations: usize, scanned: usize, backing: usize| {
            log.borrow_mut().push([operations, scanned, backing]);
            Ok::<(), shared::rust::clone_backing::BackingError>(())
        };
        walk(&meter).expect("an unlimited meter");
        log.into_inner()
    }

    fn string_par(length: usize) -> Par {
        Par {
            exprs: vec![Expr {
                expr_instance: Some(models::rhoapi::expr::ExprInstance::GString(
                    "t".repeat(length),
                )),
            }],
            ..Par::default()
        }
    }

    /// D-E2 (DR-109): before each `contains` scan of the remainder targets,
    /// `list_match` inspects the target once and the remainder targets twice
    /// (`MatcherReadsByReference.two_container_inspections_cover_membership_scan`).
    /// The reservation log holds that run for each target, in order.
    #[test]
    fn list_match_remainder_sort_charges_two_container_traversals() {
        let target = string_par(128);
        let targets = vec![target.clone(); 3];
        let log = Mutex::new(Vec::with_capacity(4_096));
        let meter = |operations: usize, scanned: usize, backing: usize| {
            log.lock()
                .expect("log lock")
                .push([operations, scanned, backing]);
            Ok(())
        };
        let mut context = SpatialMatcherContext::with_meter(&meter).expect("metered context");
        let merger = |_: &mut Par, _: Vec<Par>, _: &MatcherWork<'_>| Some(());
        assert!(context
            .list_match(targets.clone(), Vec::new(), &merger, Some(0), false)
            .is_some());
        assert!(context.take_error().is_none());
        let calls = log.lock().expect("log lock").clone();
        let remainder = vec![target.clone(); 3];
        let inspect_remainder =
            walk_calls(|meter| shared::rust::clone_backing::inspect_blocks(&remainder, meter));
        let run = [
            walk_calls(|meter| shared::rust::clone_backing::inspect_blocks(&target, meter)),
            inspect_remainder.clone(),
            inspect_remainder,
        ]
        .concat();
        let mut from = 0;
        for scan in 0..targets.len() {
            let found = (from..=calls.len().saturating_sub(run.len()))
                .find(|start| calls[*start..*start + run.len()] == run[..])
                .unwrap_or_else(|| panic!("no two-traversal run for scan {scan}"));
            from = found + run.len();
        }
    }

    /// D-E2 (DR-109): the copies that `list_match` makes fit the reserved
    /// backing when the targets hold 4 KiB payloads, which the walks' own
    /// worklist backing would not cover.
    #[test]
    fn list_match_copies_fit_reserved_backing() {
        let targets = vec![string_par(4_096), string_par(4_096), string_par(8)];
        let totals = Mutex::new([0usize; 3]);
        let meter = |operations: usize, scanned: usize, backing: usize| {
            let mut sum = totals.lock().expect("totals lock");
            for (total, amount) in sum.iter_mut().zip([operations, scanned, backing]) {
                *total += amount;
            }
            Ok(())
        };
        let mut context = SpatialMatcherContext::with_meter(&meter).expect("metered context");
        let merger = |_: &mut Par, _: Vec<Par>, _: &MatcherWork<'_>| Some(());
        let start = totals.lock().expect("totals lock")[2];
        let (arguments, patterns) = (targets.clone(), vec![string_par(8)]);
        let (result, allocated) =
            crate::rust::interpreter::accounting::measured_allocations(|| {
                context.list_match(arguments, patterns, &merger, Some(0), false)
            });
        assert!(result.is_some());
        assert!(context.take_error().is_none());
        let reserved = totals.lock().expect("totals lock")[2] - start;
        assert!(
            allocated <= reserved,
            "allocated {allocated}, reserved {reserved}"
        );
    }
}
