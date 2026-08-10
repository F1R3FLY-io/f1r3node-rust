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
        merger: &dyn Fn(Par, Vec<T>) -> Par,
        remainder: Option<i32>,
        wildcard: bool,
    ) -> Option<()>;

    // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - listMatch
    fn list_match(
        &mut self,
        targets: Vec<T>,
        patterns: Vec<T>,
        merger: &dyn Fn(Par, Vec<T>) -> Par,
        remainder: Option<i32>,
        wildcard: bool,
    ) -> Option<()>;

    // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - handleRemainder
    fn handle_remainder(
        &mut self,
        remainder_targets: Vec<T>,
        level: i32,
        merger: &dyn Fn(Par, Vec<T>) -> Par,
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
                let _merger: &dyn Fn(Par, Vec<$type>) -> Par = &|p, _| p;

                self.list_match_single_(tlist, plist, _merger, None, false)
              }

              // See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - listMatchSingle_
              fn list_match_single_(
                  &mut self,
                  tlist: Vec<$type>,
                  plist: Vec<$type>,
                  merger: &dyn Fn(Par, Vec<$type>) -> Par,
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
                    if tlist
                        .iter()
                        .all(|t| self.locally_free(t.to_owned(), 0).is_empty())
                    {
                        self.handle_remainder(tlist, remainder.unwrap(), merger)
                    } else {
                        // println!("\nreturning None in list_match_single_");
                        None
                    }
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
                  merger: &dyn Fn(Par, Vec<$type>) -> Par,
                  remainder: Option<i32>,
                  wildcard: bool,
              ) -> Option<()> {
                // println!("\nHit list_match");
                // println!("\nlist_match targets: {:?}", targets);
                // println!("\nlist_match patterns: {:?}", patterns);

                let remainder_patterns: Vec<Pattern<$type>> = remainder
                    .map(|level| vec![Pattern::Remainder(level); targets.len() - patterns.len()])
                    .unwrap_or(Vec::new());
                let mut all_patterns: Vec<Pattern<$type>> = remainder_patterns;
                all_patterns.extend(patterns.clone().into_iter().map(Pattern::Term));

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
                let mut maximum_bipartite_match: MaximumBipartiteMatch<Pattern<$type>, $type, FreeMap> =
                    MaximumBipartiteMatch::new_with_cache_policy(
                        _match_function,
                        _cache_structural_term,
                    );

                // println!("\ncurrent free_map: {:?}", self.free_map);

                let matches = maximum_bipartite_match.find_matches(all_patterns, targets.clone())?;

                // println!("\nfree_map after MBM: {:?}", self.free_map);
                // println!("\nmatches: {:?}", matches);

                let free_maps: Vec<FreeMap> = matches
                    .iter()
                    .map(|(_, _, free_map)| free_map.clone())
                    .collect();

                let updated_free_map = aggregate_updates(self.free_map.clone(), free_maps)?;
                // println!("\nsetting free_map in list_match");
                self.free_map = updated_free_map;
                // println!("\nnew free_map: {:?}", self.free_map);

                let remainder_targets: Vec<$type> = matches
                    .iter()
                    .filter_map(|(target, pattern, _)| match pattern {
                        Pattern::Remainder(_) => Some(target.clone()),
                        _ => None,
                    })
                    .collect();

                let remainder_targets_sorted: Vec<$type> = targets
                    .iter()
                    .filter(|target| remainder_targets.contains(target))
                    .cloned()
                    .collect();

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
                  merger: &dyn Fn(Par, Vec<$type>) -> Par,
              ) -> Option<()> {
                // println!("\nhit handle_remainder");
                // println!("remainder_targets: {:#?}", remainder_targets);
                // println!("level: {:#?}", level);
                // let merger_type_name = std::any::type_name::<&dyn Fn(Par, Vec<$type>) -> Par>();
                // println!("merger: {:?}\n", merger_type_name);

                let remainder_par = self
                .free_map
                .clone()
                .get(&level)
                .cloned()
                .unwrap_or(vector_par(Vec::new(), false));

                // println!("\nremainder_par: {:#?}", remainder_par);

                let remainder_par_updated = merger(remainder_par, remainder_targets);

                // println!("\nremainder_par_updated: {:#?}", remainder_par_updated);

                // println!("\nmodifying free_map in handle_remainder");
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
                     if !self.connective_used(p.clone()) {
                         // Pure arm: no write to `free_map` is reachable from here, so
                         // the pre-attempt and post-attempt maps are the same map and
                         // no snapshot is needed.
                         guard(t == p).map(|_| self.free_map.clone())
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
                      guard(self.locally_free(t.clone(), 0).is_empty()).map(|_| self.free_map.clone()),
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

/// Fold the free maps of every match the bipartite matcher claimed into one.
///
/// The correctness of isolating the maximum-bipartite match from changing the
/// `FreeMap` relies on our ability to aggregate the variable assignments from
/// subsequent matches. That is only sound when the variables populated by the
/// matcher do not duplicate each other, so this is where that is checked —
/// see [`first_duplicate_added_var`] for what the check is and why the port
/// had lost it.
///
/// ★ **A duplicate is a DECIDABLE NEGATIVE: it refuses.** Scala raised
/// `BugFoundError` through an error channel this trait does not have —
/// `ListMatch`/`SpatialMatcher` return `Option`, and widening them to `Result`
/// would change every signature in the module. Refusing through the `Option`
/// the function already returns is a **deliberate divergence from upstream**,
/// and the caller's `?` reads it exactly as it should: *this list match does
/// not happen*.
///
/// **Why the previous `panic!` could not stay.** `Matcher::get` receives
/// `BindPattern`s deserialised from tuplespace history, including state served
/// by a peer during sync. Pattern free-variable linearity is enforced at
/// *normalization* (`FreeMap::merge` → "Free variable X is used twice as a
/// binder"), which a deserialised pattern never went through on this node.
/// The panic was unreachable only for as long as the guard was vacuous;
/// restoring the guard while keeping the panic would have converted a dead
/// check into a remotely-triggerable node crash
/// (`shared/tests/panic_expectation_gate.rs`).
///
/// **Why this is expected to be silent forever.** Under state isolation each
/// returned map is `current + δᵢ`; the added vars duplicate iff two claimed
/// matches bind the same level; each level occupies at most one pattern
/// position by linearity; and `Pattern::Remainder` binds nothing inside the
/// bipartite match — the remainder is bound afterwards, on `self`, in
/// `handle_remainder`. So this is a genuine defense-in-depth backstop, and it
/// is *implementable* precisely because isolation makes the disjointness hold.
/// The refusals counter reading non-zero means a non-linear `BindPattern`
/// reached the matcher: a normalizer defect, or hostile tuplespace state.
///
/// ⚠ **THE LINEARITY PREMISE IS NOT SELF-EVIDENT, AND IT DID NOT HOLD WHEN THAT
/// PARAGRAPH WAS WRITTEN.** "Each level occupies at most one pattern position"
/// is a property of **one free map**, and the matcher has *several* in flight:
/// `~P` and `P \/ Q` bodies are each normalized against a **fresh** `FreeMap`
/// which `combine_p_negation` then **discards**
/// (`normalizer/processes/p_negation_normalizer.rs:10-13, 65-89`), so a free
/// variable under `~` is `FreeVar(0)` in a numbering nobody kept — and level 0
/// is somebody *else's* level in the shared map. Two sibling sub-patterns each
/// containing a binding negation therefore both added level 0, and this
/// backstop fired:
///
/// ```text
///   match { @0!(7) | @1!(6) } { @0!(~{x /\ 8}) | @1!(~{y /\ 9}) => A  _ => B }
///
///   before:  δ₁ = {0 ↦ 7}, δ₂ = {0 ↦ 6}   ⇒ duplicate ⇒ REFUSE ⇒ B runs  ✗
///   after:   δ₁ = {},      δ₂ = {}        ⇒ disjoint  ⇒ commit ⇒ A runs  ✓
/// ```
///
/// The `δᵢ` above were *the negations' own* leaks, not other attempts' — which
/// is why `match_function`'s isolation, working exactly as specified, could not
/// see them. `b219e199` (task #148) isolated the four connective sites inside
/// `spatial_matcher.rs`, which is what makes a negation's `δ` empty and the
/// premise finally true. **Nothing about this function changed**; what changed
/// is that its precondition is now met. The mechanism — including that the
/// normalizer really does emit level 0 in *every* negation body — is pinned by
/// `rholang/tests/matcher_negation_isolation.rs`, and
/// `rholang/tests/matcher_negation_reduction_witness.rs` carries it out to the
/// reduction above.
pub fn aggregate_updates(current_free_map: FreeMap, free_maps: Vec<FreeMap>) -> Option<FreeMap> {
    if let Some(level) = first_duplicate_added_var(&current_free_map, &free_maps) {
        // Observability, not consensus — there is no cost accounting anywhere in
        // this module, so a counter here cannot move metering.
        metrics::counter!(
            crate::rust::interpreter::metrics_constants::RHOLANG_MATCHER_AGGREGATE_UPDATES_REFUSALS_METRIC,
            "source" => crate::rust::interpreter::metrics_constants::RHOLANG_METRICS_SOURCE
        )
        .increment(1);

        // Structured, for a node with a subscriber installed.
        tracing::error!(
            target: "f1r3fly.rholang.matcher",
            level,
            free_maps = ?free_maps,
            "aggregated updates conflicted with each other: two claimed matches added \
             the same free-variable level, which a linearly-normalised pattern cannot \
             do; declining to commit this list match"
        );

        // ★ And unstructured, for the audit that has no subscriber and no metrics
        // recorder: this backstop's whole claim is that it is SILENT, and that
        // claim has to be checkable by running the test suite and grepping. The
        // line is on a path that never executes, so it costs nothing.
        eprintln!(
            "RHOLANG-MATCHER-AGGREGATE-UPDATES-REFUSAL: level {} was added by two \
             separate matches; free_maps = {:?}",
            level, free_maps
        );

        return None;
    }

    // `updatedFreeMap = freeMaps.fold(currentFreeMap)(_ ++ _)`. The fold order is
    // the caller's `matches` BTreeMap order and is deliberately NOT changed.
    let updated_free_map = free_maps.into_iter().fold(current_free_map, |mut acc, fm| {
        acc.extend(fm);
        acc
    });

    Some(updated_free_map)
}
