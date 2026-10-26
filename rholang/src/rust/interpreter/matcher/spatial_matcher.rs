// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - trait SpatialMatcher

use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::rholang::implicits::{single_expr, vector_par};
use models::rust::utils::*;

use super::exports::*;
use super::fold_match::FoldMatch;
// Changed by D-D2 (D-M8, DR-104): the matcher tests its values by reference.
// use super::has_locally_free::HasLocallyFree;
use super::has_locally_free::HasLocallyFreeRef;
use super::list_match::{aggregate_updates, ListMatch, Pattern};
use super::match_pars::match_pars;
use super::par_count::ParCount;
use super::sub_pars::sub_pars;
use crate::list_match;

list_match!(
    Par,
    (Par, Par),
    Send,
    Receive,
    New,
    Expr,
    Match,
    Bundle,
    GUnforgeable,
    ReceiveBind,
    If,
    CostSignedTerm,
    CostStack
);

pub trait SpatialMatcher<T, P> {
    fn spatial_match(&mut self, target: T, pattern: P) -> Option<()>;
}

/// D-D3 (DR-105): the level of a pattern that is exactly one free variable. The
/// pattern uses connectives, its only expression is `EVar(FreeVar(level))`,
/// and its other lists, its connectives and its cost terms are empty. The
/// pattern's `locally_free` is not read.
pub(super) fn free_variable_level(pattern: &Par) -> Option<i32> {
    let Par {
        sends,
        receives,
        news,
        exprs,
        matches,
        unforgeables,
        bundles,
        connectives,
        conditionals,
        locally_free: _,
        connective_used,
        cost_signed_terms,
        cost_stacks,
    } = pattern;
    if !*connective_used
        || !sends.is_empty()
        || !receives.is_empty()
        || !news.is_empty()
        || !matches.is_empty()
        || !unforgeables.is_empty()
        || !bundles.is_empty()
        || !connectives.is_empty()
        || !conditionals.is_empty()
        || !cost_signed_terms.is_empty()
        || !cost_stacks.is_empty()
    {
        return None;
    }
    match exprs.as_slice() {
        [Expr {
            expr_instance:
                Some(EVarBody(EVar {
                    v:
                        Some(Var {
                            var_instance: Some(FreeVar(level)),
                        }),
                })),
        }] => Some(*level),
        _ => None,
    }
}

#[cfg(test)]
thread_local! {
    /// D-D3 (DR-105): while the flag is set, matches on this thread take the
    /// general path for a free-variable pattern, for the equality tests.
    pub(super) static LEGACY_FREE_VARIABLE_PATH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn free_variable_fast_path() -> bool {
    #[cfg(test)]
    {
        !LEGACY_FREE_VARIABLE_PATH.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        true
    }
}

/// D-D1b (DR-103): merges the remainder of a set pattern into its binding in
/// place.
pub(super) fn merge_set_remainder(p: &mut Par, r: Vec<Par>) {
    let mut unique = Vec::with_capacity(r.len());
    for element in r {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // work.reserve_inspect(&element)?;
        // work.reserve_inspect(&unique)?;
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
}

/// D-D1b (DR-103): merges the remainder of a map pattern into its binding in
/// place.
pub(super) fn merge_map_remainder(p: &mut Par, r: Vec<(Par, Par)>) {
    let mut unique = Vec::with_capacity(r.len());
    for (key, value) in r {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // work.reserve_inspect(&key)?;
        // work.reserve_inspect(&unique)?;
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
}

#[derive(Clone)]
pub struct SpatialMatcherContext {
    pub free_map: FreeMap,
}

impl SpatialMatcherContext {
    pub fn new() -> Self {
        Self {
            free_map: new_free_map(),
        }
    }

    /// D-D2 (DR-104): compares a target with a ground pattern by reference.
    pub(super) fn match_ground_par(&self, target: &Par, pattern: &Par) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(pattern)?;
        // self.reserve_inspect(pattern)?;
        guard(match_pars(target, pattern))
    }

    pub(super) fn free_variable_fast_path(&self) -> bool { free_variable_fast_path() }

    /// D-D3 (DR-105): binds an owned target to a free-variable pattern. It
    /// makes the ten `list_match_single_` calls of the Par/Par matcher, in the
    /// same order and with the same empty pattern lists, so the result and the
    /// free map equal those of the general path, also on failure
    /// (`FreeVariableFastPath.fast_path_equals_general`).
    pub(super) fn bind_free_variable(&mut self, target: Par, level: i32) -> Option<()> {
        let remainder = Some(level);
        self.list_match_single_(
            target.sends,
            Vec::new(),
            &|p, s| p.sends = s,
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.receives,
            Vec::new(),
            &|p, s| p.receives = s,
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.news,
            Vec::new(),
            &|p, s| p.news = s,
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.exprs,
            Vec::new(),
            &|p, s| p.exprs = s,
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.matches,
            Vec::new(),
            &|p, s| p.matches = s,
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.bundles,
            Vec::new(),
            &|p, s| p.bundles = s,
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.unforgeables,
            Vec::new(),
            &|p, s| p.unforgeables = s,
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.conditionals,
            Vec::new(),
            &|p, values| p.conditionals = values,
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.cost_signed_terms,
            Vec::new(),
            &|p, values| p.cost_signed_terms = values,
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.cost_stacks,
            Vec::new(),
            &|p, values| p.cost_stacks = values,
            remainder,
            false,
        )
    }

    /// D-D3 (DR-105): binds a borrowed target to a free-variable pattern. Each
    /// field is checked as `list_match_single_` checks it, then copied and
    /// merged by `handle_remainder`, in the order of the Par/Par matcher.
    pub(super) fn bind_free_variable_by_reference(
        &mut self,
        target: &Par,
        level: i32,
    ) -> Option<()> {
        self.bind_field_by_reference(&target.sends, level, &|p, s| p.sends = s)?;
        self.bind_field_by_reference(&target.receives, level, &|p, s| p.receives = s)?;
        self.bind_field_by_reference(&target.news, level, &|p, s| p.news = s)?;
        self.bind_field_by_reference(&target.exprs, level, &|p, s| p.exprs = s)?;
        self.bind_field_by_reference(&target.matches, level, &|p, s| p.matches = s)?;
        self.bind_field_by_reference(&target.bundles, level, &|p, s| p.bundles = s)?;
        self.bind_field_by_reference(&target.unforgeables, level, &|p, s| p.unforgeables = s)?;
        self.bind_field_by_reference(&target.conditionals, level, &|p, values| {
            p.conditionals = values
        })?;
        self.bind_field_by_reference(&target.cost_signed_terms, level, &|p, values| {
            p.cost_signed_terms = values
        })?;
        self.bind_field_by_reference(&target.cost_stacks, level, &|p, values| {
            p.cost_stacks = values
        })
    }

    fn bind_field_by_reference<T: Clone>(
        &mut self,
        field: &[T],
        level: i32,
        merger: &dyn Fn(&mut Par, Vec<T>),
    ) -> Option<()>
    where
        Self: ListMatch<T> + HasLocallyFreeRef<T>,
    {
        for element in field {
            // Changed by D-O1 (DR-109): block accounting charges inline bytes
            // once per enclosing block.
            // self.reserve_inspect(element)?;
            if !self.locally_free_is_empty(element, 0) {
                return None;
            }
        }
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_slice(field)?;
        let copied = field.to_vec();
        self.handle_remainder(copied, level, merger)
    }

    pub fn spatial_match_result(&mut self, target: Par, pattern: Par) -> Option<&FreeMap> {
        let do_match = self.spatial_match(target, pattern);

        match do_match {
            Some(_) => Some(&self.free_map),
            None => None,
        }
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - forTuple
impl SpatialMatcher<(Par, Par), (Par, Par)> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: (Par, Par), pattern: (Par, Par)) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        self.spatial_match(target.0, pattern.0)
            .and_then(|_| self.spatial_match(target.1, pattern.1))
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - connectiveMatcher
impl SpatialMatcher<Par, Connective> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Par, pattern: Connective) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        match pattern.connective_instance {
            Some(ConnAndBody(connective_body)) => {
                connective_body.ps.into_iter().try_fold((), |_, p| {
                    // Changed by D-O1 (DR-109): block accounting charges inline bytes
                    // once per enclosing block.
                    // self.reserve_clone(&target)?;
                    let match_result = self.spatial_match(target.clone(), p);
                    match_result.map(|_| ())
                })
            }

            Some(ConnOrBody(connective_body)) => connective_body.ps.into_iter().find_map(|p| {
                // Changed by D-O1 (DR-109): block accounting charges inline bytes
                // once per enclosing block.
                // self.reserve_clone(&self.free_map)?;
                let matches = self.free_map.clone();
                // Changed by D-O1 (DR-109): block accounting charges inline bytes
                // once per enclosing block.
                // self.reserve_clone(&target)?;
                self.spatial_match(target.clone(), p)?;
                self.free_map = matches;
                Some(())
            }),

            Some(ConnNotBody(p)) => {
                // Check if there is a ConnOrBody inside the ConnNotBody
                let has_or_body = match &p {
                    Par { connectives, .. } => connectives
                        .iter()
                        .any(|c| matches!(c.connective_instance, Some(ConnOrBody(_)))),
                };

                if has_or_body {
                    // If there is a ConnOrBody inside, we need to handle it specially
                    let match_option = self.spatial_match(target, p);
                    match match_option {
                        Some(_) => None,  // If inner pattern matches, the negation fails
                        None => Some(()), // If inner pattern doesn't match, the negation succeeds
                    }
                } else {
                    // Regular negation handling
                    let match_option = self.spatial_match(target, p);
                    match match_option {
                        Some(_) => None,
                        None => Some(()),
                    }
                }
            }

            Some(VarRefBody(_)) => None,

            Some(ConnBool(_)) => match single_expr(&target) {
                Some(Expr {
                    expr_instance: Some(GBool(_)),
                }) => Some(()),
                _ => None,
            },

            Some(ConnInt(_)) => match single_expr(&target) {
                Some(Expr {
                    expr_instance: Some(GInt(_)),
                }) => Some(()),
                _ => None,
            },

            Some(ConnString(_)) => match single_expr(&target) {
                Some(Expr {
                    expr_instance: Some(GString(_)),
                }) => Some(()),
                _ => None,
            },

            Some(ConnUri(_)) => match single_expr(&target) {
                Some(Expr {
                    expr_instance: Some(GUri(_)),
                }) => Some(()),
                _ => None,
            },

            Some(ConnByteArray(_)) => match single_expr(&target) {
                Some(Expr {
                    expr_instance: Some(GByteArray(_)),
                }) => Some(()),
                _ => None,
            },

            None => None,
        }
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - parSpatialMatcher
impl SpatialMatcher<Par, Par> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Par, pattern: Par) -> Option<()> {
        // Changed by D-D2 (D-M8, DR-104): a ground pattern charges the lockstep
        // bound of match_ground_par, not an inspection of the target. A pattern
        // with connectives still inspects both values.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        // if !pattern.connective_used {
        //     // guard(pattern == target)
        //     guard(match_pars(&target, &pattern))
        // } else {
        if !pattern.connective_used {
            self.match_ground_par(&target, &pattern)
        } else {
            // Changed by D-D3 (D-M9, DR-105): the inspection of the pattern comes
            // first and prepays the free-variable test, so a free-variable pattern
            // binds the target's fields without an inspection of the target.
            // self.reserve_inspect(&target)?;
            // self.reserve_inspect(&pattern)?;
            // Changed by D-O1 (DR-109): block accounting charges inline bytes
            // once per enclosing block.
            // self.reserve_inspect(&pattern)?;
            if self.free_variable_fast_path() {
                if let Some(level) = free_variable_level(&pattern) {
                    return self.bind_free_variable(target, level);
                }
            }
            // Changed by D-O1 (DR-109): block accounting charges inline bytes
            // once per enclosing block.
            // self.reserve_inspect(&target)?;
            let var_level: Option<i32> = pattern.exprs.iter().find_map(|expr| match expr {
                Expr {
                    expr_instance:
                        Some(EVarBody(EVar {
                            v:
                                Some(Var {
                                    var_instance: Some(FreeVar(level)),
                                }),
                        })),
                } => Some(*level),
                _ => None,
            });

            let wildcard: bool = pattern
                .exprs
                .iter()
                .find_map(|expr| match expr {
                    Expr {
                        expr_instance:
                            Some(EVarBody(EVar {
                                v:
                                    Some(Var {
                                        var_instance: Some(Wildcard(_)),
                                    }),
                            })),
                    } => Some(()),
                    _ => None,
                })
                .is_some();

            let pc = ParCount::without_frees(&pattern);
            let min_rem = pc.clone();
            let max_rem = if wildcard || !var_level.is_none() {
                pc._max()
            } else {
                pc.clone()
            };

            let mut individual_bounds = Vec::with_capacity(pattern.connectives.len());
            for con in &pattern.connectives {
                // Changed by D-O1 (DR-109): block accounting charges inline bytes
                // once per enclosing block.
                // self.reserve_inspect(con)?;
                individual_bounds.push(pc.min_max_con(con));
            }

            let mut remainder_bounds = Vec::with_capacity(individual_bounds.len() + 1);
            remainder_bounds.push((min_rem, max_rem));
            for bounds in individual_bounds.iter().rev() {
                let last = remainder_bounds.last().unwrap();
                let next = (bounds.0.add(&last.0), bounds.1.add(&last.1));
                remainder_bounds.push(next);
            }
            remainder_bounds.pop();
            remainder_bounds.reverse();

            fn match_connective_with_bounds(
                s: &mut SpatialMatcherContext,
                target: Par,
                labeled_connective: (&Connective, &(ParCount, ParCount), &(ParCount, ParCount)),
            ) -> Option<Par> {
                let (con, bounds, remainders) = labeled_connective;

                let subsets =
                    sub_pars(&target, &bounds.0, &bounds.1, &remainders.0, &remainders.1)?;
                for sp in subsets {
                    // Changed by D-O1 (DR-109): block accounting charges inline bytes
                    // once per enclosing block.
                    // s.reserve_clone(con)?;
                    if s.spatial_match(sp.0, con.clone()).is_some() {
                        return Some(sp.1);
                    }
                }
                None
            }

            let remainder = pattern
                .connectives
                .iter()
                .zip(individual_bounds.iter())
                .zip(remainder_bounds.iter())
                .try_fold(target, |acc, ((connective, bounds), remainders)| {
                    match_connective_with_bounds(self, acc, (connective, bounds, remainders))
                })?;

            self.list_match_single_(
                remainder.sends,
                pattern.sends,
                &|p, s| p.sends = s,
                var_level,
                wildcard,
            )
            .and_then(|_| {
                self.list_match_single_(
                    remainder.receives,
                    pattern.receives,
                    &|p, s| p.receives = s,
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.news,
                    pattern.news,
                    &|p, s| p.news = s,
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                // Changed by D-D2 (D-M8, DR-104): the owned pattern expressions are
                // filtered in place, without a copy of the slice.
                // self.reserve_slice(&pattern.exprs)?;
                let mut exprs = pattern.exprs;
                retain_no_frees(&mut exprs);
                self.list_match_single_(
                    remainder.exprs,
                    // no_frees_exprs(&pattern.exprs),
                    exprs,
                    &|p, s| p.exprs = s,
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.matches,
                    pattern.matches,
                    &|p, s| p.matches = s,
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.bundles,
                    pattern.bundles,
                    &|p, s| p.bundles = s,
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.unforgeables,
                    pattern.unforgeables,
                    &|p, s| p.unforgeables = s,
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.conditionals,
                    pattern.conditionals,
                    &|p, values| p.conditionals = values,
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.cost_signed_terms,
                    pattern.cost_signed_terms,
                    &|p, values| p.cost_signed_terms = values,
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.cost_stacks,
                    pattern.cost_stacks,
                    &|p, values| p.cost_stacks = values,
                    var_level,
                    wildcard,
                )
            })
        }
    }
}

impl SpatialMatcher<If, If> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: If, pattern: If) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        guard(target == pattern)
    }
}

impl SpatialMatcher<CostSignedTerm, CostSignedTerm> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: CostSignedTerm, pattern: CostSignedTerm) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        guard(target == pattern)
    }
}

impl SpatialMatcher<CostStack, CostStack> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: CostStack, pattern: CostStack) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        guard(target == pattern)
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - bundleSpatialMatcherInstance
// Apparently this code is never reached according to Scala code comment
impl SpatialMatcher<Bundle, Bundle> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Bundle, pattern: Bundle) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        guard(pattern == target)
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - sendSpatialMatcherInstance
impl SpatialMatcher<Send, Send> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Send, pattern: Send) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        let result = guard(target.persistent == pattern.persistent)
            .and_then(|_| self.spatial_match(target.chan.unwrap(), pattern.chan.unwrap()))
            .and_then(|_| self.fold_match(&target.data, &pattern.data, None));

        result.map(|_| ())
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - receiveSpatialMatcherInstance
impl SpatialMatcher<Receive, Receive> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Receive, pattern: Receive) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        guard(target.persistent == pattern.persistent)
            .and_then(|_| self.list_match_single(target.binds, pattern.binds))
            .and_then(|_| self.spatial_match(target.body.unwrap(), pattern.body.unwrap()))
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - newSpatialMatcherInstance
impl SpatialMatcher<New, New> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: New, pattern: New) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        guard(target.bind_count == pattern.bind_count)
            .and_then(|_| self.spatial_match(target.p.unwrap(), pattern.p.unwrap()))
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - exprSpatialMatcherInstance
impl SpatialMatcher<Expr, Expr> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Expr, pattern: Expr) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        match (target.expr_instance, pattern.expr_instance) {
            (
                Some(EListBody(EList {
                    ps: tlist,
                    locally_free: _,
                    connective_used: _,
                    remainder: _,
                })),
                Some(EListBody(EList {
                    ps: plist,
                    locally_free: _,
                    connective_used: _,
                    remainder: rem,
                })),
            ) => {
                let free_level = match &rem {
                    Some(Var {
                        var_instance: Some(FreeVar(level)),
                    }) => Some(*level),
                    _ => None,
                };
                let matched_rem = self.fold_match(&tlist, &plist, rem)?;

                match free_level {
                    Some(level) => {
                        // Changed by D-D1a (D-M2, DR-103): whole-tree backing on every
                        // insert, the pattern DR-77 replaced; the insert now charges its
                        // search, its moves and the growth of the tree.
                        // let Some(entries) = self.free_map.len().checked_add(1) else {
                        //     return self.reject(RSpaceError::HostWorkRejected);
                        // };
                        // let Some((operations, bytes)) =
                        //     shared::rust::collection_backing::tree_backing::<i32, Par>(entries)
                        // else {
                        //     return self.reject(RSpaceError::HostWorkRejected);
                        // };
                        // let Some(backing) = bytes.checked_add(std::mem::size_of::<Expr>()) else {
                        //     return self.reject(RSpaceError::HostWorkRejected);
                        // };
                        // let Some(operations) = operations.checked_add(2) else {
                        //     return self.reject(RSpaceError::HostWorkRejected);
                        // };
                        // self.reserve(operations, bytes, backing)?;
                        self.free_map.insert(
                            level,
                            new_elist_par(matched_rem, Vec::new(), false, None, Vec::new(), false),
                        );
                        Some(())
                    }

                    _ => Some(()),
                }
            }

            (
                Some(ETupleBody(ETuple {
                    ps: tlist,
                    locally_free: _,
                    connective_used: _,
                })),
                Some(ETupleBody(ETuple {
                    ps: plist,
                    locally_free: _,
                    connective_used: _,
                })),
            ) => self.fold_match(&tlist, &plist, None).map(|_| ()),

            (Some(ESetBody(t_set)), Some(ESetBody(p_set))) => {
                let is_wildcard = match p_set.remainder.as_ref() {
                    Some(Var {
                        var_instance: Some(Wildcard(_)),
                    }) => true,
                    _ => false,
                };

                let remainder_var_opt = match p_set.remainder.as_ref() {
                    Some(Var {
                        var_instance: Some(FreeVar(level)),
                    }) => Some(*level),
                    _ => None,
                };
                let tlist = ParSetTypeMapper::eset_to_par_set(t_set).ps;
                let plist = ParSetTypeMapper::eset_to_par_set(p_set).ps;

                // Changed by D-D1b (D-M2, DR-103): the merger takes the binding by
                // reference and merges it in place (merge_set_remainder).
                // let merger = |mut p: Par, r: Vec<Par>, work: &MatcherWork<'_>| {
                //     let mut unique = Vec::new();
                //     for element in r {
                //         work.reserve_inspect(&element)?;
                //         work.reserve_inspect(&unique)?;
                //         if !unique.contains(&element) {
                //             work.reserve_vec(&mut unique, 1)?;
                //             unique.push(element);
                //         }
                //     }
                //     let mut exprs = Vec::new();
                //     work.reserve_vec(&mut exprs, 1)?;
                //     exprs.push(Expr {
                //         expr_instance: Some(ESetBody(ESet {
                //             ps: unique,
                //             locally_free: Vec::new(),
                //             connective_used: false,
                //             remainder: None,
                //         })),
                //     });
                //     p.exprs = exprs;
                //     Some(p)
                // };

                self.list_match_single_(
                    tlist.sorted_pars,
                    plist.sorted_pars,
                    &merge_set_remainder,
                    remainder_var_opt,
                    is_wildcard,
                )
            }

            (Some(EMapBody(t_emap)), Some(EMapBody(p_emap))) => {
                let is_wildcard = match p_emap.remainder.as_ref() {
                    Some(Var {
                        var_instance: Some(Wildcard(_)),
                    }) => true,
                    _ => false,
                };

                let remainder_var_opt = match p_emap.remainder.as_ref() {
                    Some(Var {
                        var_instance: Some(FreeVar(level)),
                    }) => Some(*level),
                    _ => None,
                };
                let tlist = ParMapTypeMapper::emap_to_par_map(t_emap).ps;
                let plist = ParMapTypeMapper::emap_to_par_map(p_emap).ps;

                // Changed by D-D1b (D-M2, DR-103): the merger takes the binding by
                // reference and merges it in place (merge_map_remainder).
                // let merger = |mut p: Par, r: Vec<(Par, Par)>, work: &MatcherWork<'_>| {
                //     let mut unique = Vec::new();
                //     for (key, value) in r {
                //         work.reserve_inspect(&key)?;
                //         work.reserve_inspect(&unique)?;
                //         if let Some(index) = unique
                //             .iter()
                //             .position(|(existing, _): &(Par, Par)| existing == &key)
                //         {
                //             unique[index].1 = value;
                //         } else {
                //             work.reserve_vec(&mut unique, 1)?;
                //             unique.push((key, value));
                //         }
                //     }
                //     let mut kvs = Vec::new();
                //     work.reserve_vec(&mut kvs, unique.len())?;
                //     for (key, value) in unique {
                //         kvs.push(KeyValuePair {
                //             key: Some(key),
                //             value: Some(value),
                //         });
                //     }
                //     let mut exprs = Vec::new();
                //     work.reserve_vec(&mut exprs, 1)?;
                //     exprs.push(Expr {
                //         expr_instance: Some(EMapBody(EMap {
                //             kvs,
                //             locally_free: Vec::new(),
                //             connective_used: false,
                //             remainder: None,
                //         })),
                //     });
                //     p.exprs = exprs;
                //     Some(p)
                // };

                self.list_match_single_(
                    tlist.sorted_list,
                    plist.sorted_list,
                    &merge_map_remainder,
                    remainder_var_opt,
                    is_wildcard,
                )
            }

            (Some(EVarBody(EVar { v: vp })), Some(EVarBody(EVar { v: vt }))) => guard(vp == vt),

            (Some(ENotBody(ENot { p: t })), Some(ENotBody(ENot { p }))) => {
                self.spatial_match(t.unwrap(), p.unwrap())
            }

            (Some(ENegBody(ENeg { p: t })), Some(ENegBody(ENeg { p }))) => {
                self.spatial_match(t.unwrap(), p.unwrap())
            }

            (Some(EMultBody(EMult { p1: t1, p2: t2 })), Some(EMultBody(EMult { p1, p2 }))) => self
                .spatial_match(t1.unwrap(), p1.unwrap())
                .and_then(|_| self.spatial_match(t2.unwrap(), p2.unwrap())),

            (Some(EDivBody(EDiv { p1: t1, p2: t2 })), Some(EDivBody(EDiv { p1, p2 }))) => self
                .spatial_match(t1.unwrap(), p1.unwrap())
                .and_then(|_| self.spatial_match(t2.unwrap(), p2.unwrap())),

            (Some(EModBody(EMod { p1: t1, p2: t2 })), Some(EModBody(EMod { p1, p2 }))) => self
                .spatial_match(t1.unwrap(), p1.unwrap())
                .and_then(|_| self.spatial_match(t2.unwrap(), p2.unwrap())),

            (
                Some(EPercentPercentBody(EPercentPercent { p1: t1, p2: t2 })),
                Some(EPercentPercentBody(EPercentPercent { p1, p2 })),
            ) => self
                .spatial_match(t1.unwrap(), p1.unwrap())
                .and_then(|_| self.spatial_match(t2.unwrap(), p2.unwrap())),

            (Some(EPlusBody(EPlus { p1: t1, p2: t2 })), Some(EPlusBody(EPlus { p1, p2 }))) => self
                .spatial_match(t1.unwrap(), p1.unwrap())
                .and_then(|_| self.spatial_match(t2.unwrap(), p2.unwrap())),

            (
                Some(EPlusPlusBody(EPlusPlus { p1: t1, p2: t2 })),
                Some(EPlusPlusBody(EPlusPlus { p1, p2 })),
            ) => self
                .spatial_match(t1.unwrap(), p1.unwrap())
                .and_then(|_| self.spatial_match(t2.unwrap(), p2.unwrap())),

            (
                Some(EMinusMinusBody(EMinusMinus { p1: t1, p2: t2 })),
                Some(EMinusMinusBody(EMinusMinus { p1, p2 })),
            ) => self
                .spatial_match(t1.unwrap(), p1.unwrap())
                .and_then(|_| self.spatial_match(t2.unwrap(), p2.unwrap())),

            _ => None,
        }
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - matchSpatialMatcherInstance
impl SpatialMatcher<Match, Match> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Match, pattern: Match) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        let result = self
            .spatial_match(target.target.unwrap(), pattern.target.unwrap())
            .and_then(|_| self.fold_match(&target.cases, &pattern.cases, None));

        result.map(|_| ())
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - unfSpatialMatcherInstance
// Apparently this code is never reached according to Scala code comment
impl SpatialMatcher<GUnforgeable, GUnforgeable> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: GUnforgeable, pattern: GUnforgeable) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        match (target.unf_instance, pattern.unf_instance) {
            (Some(GPrivateBody(t)), Some(GPrivateBody(p))) => guard(t == p),
            (Some(GDeployerIdBody(t)), Some(GDeployerIdBody(p))) => guard(t == p),
            _ => None,
        }
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - receiveBindSpatialMatcherInstance
impl SpatialMatcher<ReceiveBind, ReceiveBind> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: ReceiveBind, pattern: ReceiveBind) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        guard(target.patterns == pattern.patterns)
            .and_then(|_| self.spatial_match(target.source.unwrap(), pattern.source.unwrap()))
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - matchCaseSpatialMatcherInstance
impl SpatialMatcher<MatchCase, MatchCase> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: MatchCase, pattern: MatchCase) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        guard(target.pattern == pattern.pattern)
            .and_then(|_| self.spatial_match(target.source.unwrap(), pattern.source.unwrap()))
    }
}

// This implementation for type 'KeyValuePair' is NOT on the Scala side
// Somewhere, somehow, on Scala side they are are just calling this logic
// Could be related to ParMap. See RhoTypes.proto and how they set custom types for fields
// impl SpatialMatcher<KeyValuePair, KeyValuePair> for SpatialMatcherContext {
//     fn spatial_match(&mut self, target: KeyValuePair, pattern: KeyValuePair) -> Option<()> {
//         self.spatial_match(target.key.unwrap(), pattern.key.unwrap())
//             .and_then(|_| self.spatial_match(target.value.unwrap(), pattern.value.unwrap()))
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_map_remainders_keep_canonical_payloads() {
        let element = new_gint_par(7, Vec::new(), false);
        let set_target = Expr {
            expr_instance: Some(ESetBody(ESet {
                ps: vec![element.clone(), element.clone()],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let set_pattern = Expr {
            expr_instance: Some(ESetBody(ESet {
                ps: Vec::new(),
                locally_free: Vec::new(),
                connective_used: false,
                remainder: Some(new_freevar_var(0)),
            })),
        };
        let mut set_context = SpatialMatcherContext::new();
        assert!(set_context.spatial_match(set_target, set_pattern).is_some());
        let expected_set = vector_par(Vec::new(), false).with_exprs(vec![new_eset_expr(
            vec![element],
            Vec::new(),
            false,
            None,
        )]);
        assert_eq!(set_context.free_map.get(&0), Some(&expected_set));

        let key = new_gint_par(1, Vec::new(), false);
        let old_value = new_gint_par(2, Vec::new(), false);
        let new_value = new_gint_par(3, Vec::new(), false);
        let map_target = Expr {
            expr_instance: Some(EMapBody(EMap {
                kvs: vec![
                    new_key_value_pair(key.clone(), old_value),
                    new_key_value_pair(key.clone(), new_value.clone()),
                ],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let map_pattern = Expr {
            expr_instance: Some(EMapBody(EMap {
                kvs: Vec::new(),
                locally_free: Vec::new(),
                connective_used: false,
                remainder: Some(new_freevar_var(0)),
            })),
        };
        let mut map_context = SpatialMatcherContext::new();
        assert!(map_context.spatial_match(map_target, map_pattern).is_some());
        let expected_map = vector_par(Vec::new(), false).with_exprs(vec![new_emap_expr(
            vec![new_key_value_pair(key, new_value)],
            Vec::new(),
            false,
            None,
        )]);
        assert_eq!(map_context.free_map.get(&0), Some(&expected_map));

        let first = Par {
            exprs: vec![new_gint_expr(1), new_gint_expr(2)],
            ..Par::default()
        };
        let second = Par {
            exprs: vec![new_gint_expr(2), new_gint_expr(1)],
            ..Par::default()
        };
        assert_ne!(first, second);
        let duplicate_set = ESet {
            ps: vec![first.clone(), second.clone()],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        };
        let canonical_set = ParSetTypeMapper::eset_to_par_set(duplicate_set.clone())
            .ps
            .sorted_pars;
        let mut set_context = SpatialMatcherContext::new();
        assert!(set_context
            .spatial_match(
                Expr {
                    expr_instance: Some(ESetBody(duplicate_set)),
                },
                Expr {
                    expr_instance: Some(ESetBody(ESet {
                        ps: Vec::new(),
                        locally_free: Vec::new(),
                        connective_used: false,
                        remainder: Some(new_freevar_var(0)),
                    })),
                },
            )
            .is_some());
        let expected_set = vector_par(Vec::new(), false).with_exprs(vec![new_eset_expr(
            canonical_set,
            Vec::new(),
            false,
            None,
        )]);
        assert_eq!(set_context.free_map.get(&0), Some(&expected_set));

        let duplicate_map = EMap {
            kvs: vec![
                new_key_value_pair(first, new_gint_par(4, Vec::new(), false)),
                new_key_value_pair(second, new_gint_par(4, Vec::new(), false)),
            ],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        };
        let canonical_map = ParMapTypeMapper::emap_to_par_map(duplicate_map.clone())
            .ps
            .sorted_list;
        let mut map_context = SpatialMatcherContext::new();
        assert!(map_context
            .spatial_match(
                Expr {
                    expr_instance: Some(EMapBody(duplicate_map)),
                },
                Expr {
                    expr_instance: Some(EMapBody(EMap {
                        kvs: Vec::new(),
                        locally_free: Vec::new(),
                        connective_used: false,
                        remainder: Some(new_freevar_var(0)),
                    })),
                },
            )
            .is_some());
        let expected_map = vector_par(Vec::new(), false).with_exprs(vec![new_emap_expr(
            canonical_map
                .into_iter()
                .map(|(key, value)| new_key_value_pair(key, value))
                .collect(),
            Vec::new(),
            false,
            None,
        )]);
        assert_eq!(map_context.free_map.get(&0), Some(&expected_map));
    }

    #[test]
    fn set_and_map_matching_preserve_unique_element_semantics() {
        let element = new_gstring_par("element".to_owned(), Vec::new(), false);
        let target_set = Expr {
            expr_instance: Some(ESetBody(ESet {
                ps: vec![element.clone(), element.clone()],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let pattern_set = Expr {
            expr_instance: Some(ESetBody(ESet {
                ps: vec![element],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let key = new_gint_par(1, Vec::new(), false);
        let old_value = new_gint_par(2, Vec::new(), false);
        let new_value = new_gint_par(3, Vec::new(), false);
        let target_map = Expr {
            expr_instance: Some(EMapBody(EMap {
                kvs: vec![
                    new_key_value_pair(key.clone(), old_value),
                    new_key_value_pair(key.clone(), new_value.clone()),
                ],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let pattern_map = Expr {
            expr_instance: Some(EMapBody(EMap {
                kvs: vec![new_key_value_pair(key, new_value)],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };

        for (target, pattern) in [(target_set, pattern_set), (target_map, pattern_map)] {
            let mut context = SpatialMatcherContext::new();
            assert!(context.spatial_match(target, pattern).is_some());
        }
    }

    fn free_or_ground_expr() -> impl proptest::strategy::Strategy<Value = Expr> {
        use proptest::strategy::Strategy;

        (0usize..6, 0i32..4).prop_map(|(variant, index)| {
            let v = match variant {
                0 => {
                    return Expr {
                        expr_instance: Some(GInt(index as i64)),
                    }
                }
                1 => None,
                2 => Some(Var { var_instance: None }),
                3 => Some(Var {
                    var_instance: Some(BoundVar(index)),
                }),
                4 => Some(Var {
                    var_instance: Some(FreeVar(index)),
                }),
                _ => Some(Var {
                    var_instance: Some(Wildcard(WildcardMsg {})),
                }),
            };
            Expr {
                expr_instance: Some(EVarBody(EVar { v })),
            }
        })
    }

    fn ground_par(size: usize) -> Par {
        Par {
            exprs: vec![new_gint_expr(7); size],
            ..Default::default()
        }
    }

    /// D-D2 (DR-104): the in-place filter allocates nothing.
    #[test]
    fn retain_no_frees_allocates_nothing() {
        let mut exprs: Vec<Expr> = (0..32)
            .map(|index| {
                if index % 3 == 0 {
                    new_gint_expr(index)
                } else {
                    Expr {
                        expr_instance: Some(EVarBody(EVar {
                            v: Some(Var {
                                var_instance: Some(FreeVar(index as i32)),
                            }),
                        })),
                    }
                }
            })
            .collect();
        let ((), allocated) = crate::rust::interpreter::accounting::measured_allocations(|| {
            retain_no_frees(&mut exprs)
        });
        assert_eq!(allocated, 0);
        assert_eq!(exprs.len(), 11);
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

        /// D-D2 (DR-104): the in-place filter keeps the expressions that
        /// `no_frees_exprs` keeps, in the same order.
        #[test]
        fn retained_no_frees_equals_cloned_filter(
            exprs in proptest::collection::vec(free_or_ground_expr(), 0..12),
        ) {
            let expected = no_frees_exprs(&exprs);
            let mut retained = exprs;
            retain_no_frees(&mut retained);
            proptest::prop_assert_eq!(retained, expected);
        }

        /// D-D2 (DR-104): a fold that compares ground pairs by reference gives
        /// the result and the free map of the walk that copies every pair into
        /// `spatial_match`.
        #[test]
        fn fold_match_by_reference_equals_cloned_pair_walk(
            targets in proptest::collection::vec((0i64..3, 1usize..3), 0..5),
            patterns in proptest::collection::vec((proptest::prelude::any::<bool>(), 0i64..3, 1usize..3), 0..5),
        ) {
            let targets: Vec<Par> = targets
                .into_iter()
                .map(|(value, size)| Par {
                    exprs: vec![new_gint_expr(value); size],
                    ..Default::default()
                })
                .collect();
            let patterns: Vec<Par> = patterns
                .into_iter()
                .enumerate()
                .map(|(level, (free, value, size))| {
                    if free {
                        let mut par = new_freevar_par(level as i32, Vec::new());
                        par.connective_used = true;
                        par
                    } else {
                        Par {
                            exprs: vec![new_gint_expr(value); size],
                            ..Default::default()
                        }
                    }
                })
                .collect();
            let mut by_reference = SpatialMatcherContext::new();
            let folded = by_reference.fold_match(&targets, &patterns, None);
            let mut copied = SpatialMatcherContext::new();
            let walked = (|| {
                for (target, pattern) in targets.iter().zip(&patterns) {
                    copied.spatial_match(target.clone(), pattern.clone())?;
                }
                (targets.len() == patterns.len()).then(Vec::new)
            })();
            proptest::prop_assert_eq!(folded, walked);
            proptest::prop_assert_eq!(by_reference.free_map, copied.free_map);
        }
    }

    fn with_legacy_free_variable_path<R>(legacy: bool, action: impl FnOnce() -> R) -> R {
        LEGACY_FREE_VARIABLE_PATH.with(|flag| flag.set(legacy));
        let result = action();
        LEGACY_FREE_VARIABLE_PATH.with(|flag| flag.set(false));
        result
    }

    fn free_variable(level: i32, locally_free: Vec<u8>) -> Par {
        let mut par = new_freevar_par(level, Vec::new());
        par.connective_used = true;
        par.locally_free = locally_free;
        par
    }

    fn locally(open: bool) -> Vec<u8> {
        if open {
            vec![1]
        } else {
            Vec::new()
        }
    }

    fn body(open: bool) -> Option<Par> {
        Some(Par {
            locally_free: locally(open),
            ..Default::default()
        })
    }

    fn open_or_closed_expr(open: bool) -> Expr {
        if open {
            new_boundvar_expr(0)
        } else {
            new_gint_expr(3)
        }
    }

    /// A target whose ten fields hold closed or open elements, with a random
    /// shell. An open element has a locally free variable.
    fn free_variable_target() -> impl proptest::strategy::Strategy<Value = Par> {
        use proptest::strategy::Strategy;

        let field = || proptest::collection::vec(proptest::prelude::any::<bool>(), 0..3);
        (
            (field(), field(), field(), field(), field()),
            (field(), field(), field(), field(), field()),
            proptest::prelude::any::<bool>(),
            proptest::collection::vec(0u8..2, 0..3),
        )
            .prop_map(
                |(
                    (sends, receives, news, exprs, matches),
                    (bundles, unforgeables, conditionals, signed, stacks),
                    connective_used,
                    locally_free,
                )| Par {
                    sends: sends
                        .into_iter()
                        .map(|open| Send {
                            locally_free: locally(open),
                            ..Default::default()
                        })
                        .collect(),
                    receives: receives
                        .into_iter()
                        .map(|open| Receive {
                            locally_free: locally(open),
                            ..Default::default()
                        })
                        .collect(),
                    news: news
                        .into_iter()
                        .map(|open| New {
                            locally_free: locally(open),
                            ..Default::default()
                        })
                        .collect(),
                    exprs: exprs.into_iter().map(open_or_closed_expr).collect(),
                    matches: matches
                        .into_iter()
                        .map(|open| Match {
                            locally_free: locally(open),
                            ..Default::default()
                        })
                        .collect(),
                    bundles: bundles
                        .into_iter()
                        .map(|open| Bundle {
                            body: body(open),
                            ..Default::default()
                        })
                        .collect(),
                    unforgeables: unforgeables
                        .into_iter()
                        .map(|_| GUnforgeable::default())
                        .collect(),
                    conditionals: conditionals
                        .into_iter()
                        .map(|open| If {
                            condition: body(open),
                            ..Default::default()
                        })
                        .collect(),
                    cost_signed_terms: signed
                        .into_iter()
                        .map(|open| CostSignedTerm {
                            body: body(open),
                            signature: None,
                        })
                        .collect(),
                    cost_stacks: stacks
                        .into_iter()
                        .map(|open| CostStack {
                            cells: if open {
                                vec![CostSignature {
                                    value: Some(models::rhoapi::cost_signature::Value::BoundLevel(
                                        0,
                                    )),
                                }]
                            } else {
                                Vec::new()
                            },
                        })
                        .collect(),
                    locally_free,
                    connective_used,
                    ..Default::default()
                },
            )
    }

    /// A stored binding with a shell that the merge never writes.
    fn shell_binding() -> Par {
        Par {
            exprs: vec![new_gint_expr(9)],
            locally_free: vec![1, 0],
            connective_used: true,
            connectives: vec![Connective::default()],
            ..Default::default()
        }
    }

    fn run_owned(
        target: &Par,
        pattern: &Par,
        free_map: &FreeMap,
        legacy: bool,
    ) -> (Option<()>, FreeMap) {
        with_legacy_free_variable_path(legacy, || {
            let mut context = SpatialMatcherContext::new();
            context.free_map = free_map.clone();
            let result = context.spatial_match(target.clone(), pattern.clone());
            (result, context.free_map)
        })
    }

    fn run_borrowed(
        target: &Par,
        pattern: &Par,
        free_map: &FreeMap,
        legacy: bool,
    ) -> (Option<Vec<Par>>, FreeMap) {
        with_legacy_free_variable_path(legacy, || {
            let mut context = SpatialMatcherContext::new();
            context.free_map = free_map.clone();
            let result = context.fold_match(
                std::slice::from_ref(target),
                std::slice::from_ref(pattern),
                None,
            );
            (result, context.free_map)
        })
    }

    /// D-D3 (DR-105): the predicate accepts exactly one free variable.
    #[test]
    fn free_variable_level_accepts_only_one_free_variable() {
        assert_eq!(free_variable_level(&free_variable(2, vec![1])), Some(2));
        let mut not_connective = free_variable(2, Vec::new());
        not_connective.connective_used = false;
        assert_eq!(free_variable_level(&not_connective), None);
        let mut wildcard = new_wildcard_par(Vec::new(), true);
        wildcard.connective_used = true;
        assert_eq!(free_variable_level(&wildcard), None);
        let mut two = free_variable(0, Vec::new());
        two.exprs.push(new_freevar_expr(1));
        assert_eq!(free_variable_level(&two), None);
        let mut with_wildcard = free_variable(0, Vec::new());
        with_wildcard.exprs.push(new_wildcard_expr());
        assert_eq!(free_variable_level(&with_wildcard), None);
        let mut bound = free_variable(0, Vec::new());
        bound.exprs = vec![new_boundvar_expr(0)];
        assert_eq!(free_variable_level(&bound), None);
        for extend in [
            |p: &mut Par| p.sends.push(Send::default()),
            |p: &mut Par| p.receives.push(Receive::default()),
            |p: &mut Par| p.news.push(New::default()),
            |p: &mut Par| p.matches.push(Match::default()),
            |p: &mut Par| p.unforgeables.push(GUnforgeable::default()),
            |p: &mut Par| p.bundles.push(Bundle::default()),
            |p: &mut Par| p.connectives.push(Connective::default()),
            |p: &mut Par| p.conditionals.push(If::default()),
            |p: &mut Par| p.cost_signed_terms.push(CostSignedTerm::default()),
            |p: &mut Par| p.cost_stacks.push(CostStack::default()),
        ] {
            let mut pattern = free_variable(0, Vec::new());
            extend(&mut pattern);
            assert_eq!(free_variable_level(&pattern), None);
        }
    }

    /// D-D3 (DR-105): a field with a locally free element stops the binding,
    /// and the fields before it stay merged, as on the general path. The
    /// Par/Par order puts bundles before unforgeables.
    #[test]
    fn free_variable_failure_leaves_the_general_partial_binding() {
        let level = 1;
        let pattern = free_variable(level, Vec::new());
        let target = Par {
            sends: vec![Send::default()],
            exprs: vec![new_gint_expr(4)],
            bundles: vec![Bundle {
                body: body(true),
                ..Default::default()
            }],
            unforgeables: vec![GUnforgeable::default()],
            ..Default::default()
        };
        let mut expected = vector_par(Vec::new(), false);
        expected.sends = target.sends.clone();
        expected.exprs = target.exprs.clone();
        let expected_map: FreeMap = [(level, expected)].into_iter().collect();
        for legacy in [true, false] {
            let (result, free_map) = run_owned(&target, &pattern, &FreeMap::new(), legacy);
            assert!(result.is_none());
            assert_eq!(free_map, expected_map, "owned, legacy {legacy}");
            let (result, free_map) = run_borrowed(&target, &pattern, &FreeMap::new(), legacy);
            assert!(result.is_none());
            assert_eq!(free_map, expected_map, "borrowed, legacy {legacy}");
        }
        let first_open = Par {
            sends: vec![Send {
                locally_free: vec![1],
                ..Default::default()
            }],
            exprs: vec![new_gint_expr(4)],
            ..Default::default()
        };
        let existing: FreeMap = [(level, shell_binding())].into_iter().collect();
        for legacy in [true, false] {
            assert_eq!(
                run_owned(&first_open, &pattern, &existing, legacy),
                (None, existing.clone())
            );
            assert_eq!(
                run_borrowed(&first_open, &pattern, &existing, legacy),
                (None, existing.clone())
            );
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

        /// D-D3 (DR-105): on the owned and the borrowed entry, the fast path for
        /// a free-variable pattern gives the result and the free map of the
        /// general path, also when a field holds a locally free element, with
        /// and without a stored binding at the level.
        #[test]
        fn free_variable_fast_path_equals_general_path(
            level in 0i32..4,
            target in free_variable_target(),
            pattern_locally_free in proptest::collection::vec(0u8..2, 0..3),
            existing in proptest::prelude::any::<bool>(),
            others in proptest::collection::vec(0i64..3, 0..3),
        ) {
            let pattern = free_variable(level, pattern_locally_free);
            let mut free_map = FreeMap::new();
            for (offset, value) in others.into_iter().enumerate() {
                free_map.insert(level + 1 + offset as i32, Par {
                    exprs: vec![new_gint_expr(value)],
                    ..Default::default()
                });
            }
            if existing {
                free_map.insert(level, shell_binding());
            }
            let general = run_owned(&target, &pattern, &free_map, true);
            proptest::prop_assert_eq!(run_owned(&target, &pattern, &free_map, false), general.clone());
            let general_fold = run_borrowed(&target, &pattern, &free_map, true);
            proptest::prop_assert_eq!(run_borrowed(&target, &pattern, &free_map, false), general_fold.clone());
            proptest::prop_assert_eq!(&general_fold.1, &general.1);
            proptest::prop_assert_eq!(general_fold.0.is_some(), general.0.is_some());
        }
    }

    fn large_string_par(length: usize) -> Par {
        Par {
            exprs: vec![new_gstring_expr("s".repeat(length))],
            ..Par::default()
        }
    }

    /// Changed by DR-131: a ground comparison no longer charges host work. The
    /// test keeps its check that a ground pattern matches only an equal target.
    #[test]
    fn ground_comparison_matches_only_an_equal_target() {
        let pattern = ground_par(3);
        let context = SpatialMatcherContext::new();
        for size in [1usize, 3, 4_096] {
            assert_eq!(
                context
                    .match_ground_par(&ground_par(size), &pattern)
                    .is_some(),
                size == 3,
                "{size}"
            );
        }
    }

    /// Changed by DR-131: a scalar connective no longer charges host work. The
    /// test keeps its checks of the results. A string connective matches a
    /// target whose only expression is a string. It does not match a target
    /// with two expressions. A variable-reference connective does not match.
    #[test]
    fn scalar_connective_tests_only_a_single_expression() {
        let target = large_string_par(4_096);
        let string = Connective {
            connective_instance: Some(ConnString(true)),
        };
        let mut context = SpatialMatcherContext::new();
        assert_eq!(
            context.spatial_match(target.clone(), string.clone()),
            Some(())
        );
        let two = Par {
            exprs: vec![new_gstring_expr("a".to_owned()); 2],
            ..Par::default()
        };
        let mut context = SpatialMatcherContext::new();
        assert_eq!(context.spatial_match(two, string), None);
        let reference = Connective {
            connective_instance: Some(VarRefBody(VarRef::default())),
        };
        let mut context = SpatialMatcherContext::new();
        assert_eq!(context.spatial_match(target, reference), None);
    }
}
