// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - trait SpatialMatcher

use std::sync::{Arc, Mutex};

use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::rholang::implicits::{single_expr, vector_par};
use models::rust::utils::*;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;
use shared::rust::clone_backing::{self, arc_allocation_bytes, BackingError, CloneBacking};

use super::exports::*;
use super::fold_match::FoldMatch;
use super::has_locally_free::HasLocallyFree;
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

#[derive(Clone)]
pub(super) struct MatcherWork<'a> {
    meter: Option<&'a (dyn SourceMeter + std::marker::Send + std::marker::Sync)>,
    error: Option<Arc<Mutex<Option<RSpaceError>>>>,
}

impl<'a> MatcherWork<'a> {
    fn new() -> Self {
        Self {
            meter: None,
            error: None,
        }
    }

    fn with_meter(
        meter: &'a (dyn SourceMeter + std::marker::Send + std::marker::Sync),
    ) -> Result<Self, RSpaceError> {
        let bytes = arc_allocation_bytes::<Mutex<Option<RSpaceError>>>()
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(1, 0, bytes)?;
        Ok(Self {
            meter: Some(meter),
            error: Some(Arc::new(Mutex::new(None))),
        })
    }

    pub fn reserve(&self, operations: usize, scanned: usize, backing: usize) -> Option<()> {
        if let Some(error) = &self.error {
            if error.lock().expect("matcher meter lock").is_some() {
                return None;
            }
        }
        if let Some(meter) = self.meter {
            if let Err(error) = meter.reserve(operations, scanned, backing) {
                *self
                    .error
                    .as_ref()
                    .expect("meter error slot")
                    .lock()
                    .expect("matcher meter lock") = Some(error);
                return None;
            }
        }
        Some(())
    }

    pub fn take_error(&self) -> Option<RSpaceError> {
        self.error
            .as_ref()?
            .lock()
            .expect("matcher meter lock")
            .take()
    }

    pub fn reject<T>(&self, error: RSpaceError) -> Option<T> {
        if let Some(slot) = &self.error {
            let mut failed = slot.lock().expect("matcher meter lock");
            if failed.is_none() {
                *failed = Some(error);
            }
        }
        None
    }

    pub fn reserve_vec<T>(&self, values: &mut Vec<T>, additional: usize) -> Option<()> {
        let needed = match values.len().checked_add(additional) {
            Some(needed) => needed,
            None => return self.reject(RSpaceError::HostWorkRejected),
        };
        if needed <= values.capacity() {
            return self.reserve(1, 0, 0);
        }
        let Some(next_capacity) = values
            .capacity()
            .checked_mul(2)
            .map(|doubled| needed.max(doubled).max(4))
        else {
            return self.reject(RSpaceError::HostWorkRejected);
        };
        let Some(bytes) = next_capacity.checked_mul(std::mem::size_of::<T>()) else {
            return self.reject(RSpaceError::HostWorkRejected);
        };
        let Some(scanned) = values.len().checked_mul(std::mem::size_of::<T>()) else {
            return self.reject(RSpaceError::HostWorkRejected);
        };
        self.reserve(1, scanned, bytes)?;
        if values
            .try_reserve_exact(next_capacity - values.len())
            .is_err()
        {
            return self.reject(RSpaceError::HostWorkRejected);
        }
        Some(())
    }

    fn reserve_backing<T: CloneBacking>(&self, value: &T, inspect: bool) -> Option<()> {
        if self.meter.is_none() {
            return self.reserve(0, 0, 0);
        }
        let meter = |operations, scanned, backing| {
            self.reserve(operations, scanned, backing)
                .ok_or(BackingError::Rejected)
        };
        let result = if inspect {
            clone_backing::inspect(value, &meter)
        } else {
            clone_backing::reserve(value, &meter)
        };
        if result.is_err() {
            return self.reject(RSpaceError::HostWorkRejected);
        }
        Some(())
    }

    pub fn reserve_clone<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.reserve_backing(value, false)
    }

    pub fn reserve_inspect<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.reserve_backing(value, true)
    }

    pub fn reserve_slice<T: CloneBacking>(&self, values: &[T]) -> Option<()> {
        if self.meter.is_none() {
            return self.reserve(0, 0, 0);
        }
        let meter = |operations, scanned, backing| {
            self.reserve(operations, scanned, backing)
                .ok_or(BackingError::Rejected)
        };
        if clone_backing::reserve_slice(values, &meter).is_err() {
            return self.reject(RSpaceError::HostWorkRejected);
        }
        Some(())
    }
}

#[derive(Clone)]
pub struct SpatialMatcherContext<'a> {
    pub free_map: FreeMap,
    work: MatcherWork<'a>,
}

impl<'a> SpatialMatcherContext<'a> {
    pub fn new() -> Self {
        Self {
            free_map: new_free_map(),
            work: MatcherWork::new(),
        }
    }

    pub fn with_meter(
        meter: &'a (dyn SourceMeter + std::marker::Send + std::marker::Sync),
    ) -> Result<Self, RSpaceError> {
        Ok(Self {
            free_map: new_free_map(),
            work: MatcherWork::with_meter(meter)?,
        })
    }

    pub fn reserve(&self, operations: usize, scanned: usize, backing: usize) -> Option<()> {
        self.work.reserve(operations, scanned, backing)
    }

    pub fn take_error(&self) -> Option<RSpaceError> { self.work.take_error() }

    pub fn reject<T>(&self, error: RSpaceError) -> Option<T> { self.work.reject(error) }

    pub fn reserve_vec<T>(&self, values: &mut Vec<T>, additional: usize) -> Option<()> {
        self.work.reserve_vec(values, additional)
    }

    pub fn reserve_clone<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.work.reserve_clone(value)
    }

    pub fn reserve_inspect<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.work.reserve_inspect(value)
    }

    pub fn reserve_slice<T: CloneBacking>(&self, values: &[T]) -> Option<()> {
        self.work.reserve_slice(values)
    }

    pub(super) fn work(&self) -> Option<MatcherWork<'a>> {
        self.reserve(1, 0, 0)?;
        Some(self.work.clone())
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
impl<'a> SpatialMatcher<(Par, Par), (Par, Par)> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: (Par, Par), pattern: (Par, Par)) -> Option<()> {
        self.reserve_inspect(&target)?;
        self.reserve_inspect(&pattern)?;
        self.spatial_match(target.0, pattern.0)
            .and_then(|_| self.spatial_match(target.1, pattern.1))
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - connectiveMatcher
impl<'a> SpatialMatcher<Par, Connective> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: Par, pattern: Connective) -> Option<()> {
        self.reserve_inspect(&target)?;
        self.reserve_inspect(&pattern)?;
        match pattern.connective_instance {
            Some(ConnAndBody(connective_body)) => {
                connective_body.ps.into_iter().try_fold((), |_, p| {
                    self.reserve_clone(&target)?;
                    let match_result = self.spatial_match(target.clone(), p);
                    match_result.map(|_| ())
                })
            }

            Some(ConnOrBody(connective_body)) => connective_body.ps.into_iter().find_map(|p| {
                self.reserve_clone(&self.free_map)?;
                let matches = self.free_map.clone();
                self.reserve_clone(&target)?;
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
impl<'a> SpatialMatcher<Par, Par> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: Par, pattern: Par) -> Option<()> {
        self.reserve_inspect(&target)?;
        self.reserve_inspect(&pattern)?;
        if !pattern.connective_used {
            // guard(pattern == target)
            guard(match_pars(&target, &pattern))
        } else {
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

            let mut individual_bounds = Vec::new();
            for con in &pattern.connectives {
                self.reserve_inspect(con)?;
                self.reserve_vec(&mut individual_bounds, 1)?;
                individual_bounds.push(pc.min_max_con(con));
            }

            let mut remainder_bounds = Vec::new();
            self.reserve_vec(&mut remainder_bounds, 1)?;
            remainder_bounds.push((min_rem, max_rem));
            for bounds in individual_bounds.iter().rev() {
                let last = remainder_bounds.last().unwrap();
                let next = (bounds.0.add(&last.0), bounds.1.add(&last.1));
                self.reserve_vec(&mut remainder_bounds, 1)?;
                remainder_bounds.push(next);
            }
            remainder_bounds.pop();
            remainder_bounds.reverse();

            fn match_connective_with_bounds(
                s: &mut SpatialMatcherContext<'_>,
                target: Par,
                labeled_connective: (&Connective, &(ParCount, ParCount), &(ParCount, ParCount)),
            ) -> Option<Par> {
                let (con, bounds, remainders) = labeled_connective;

                let subsets = sub_pars(
                    &target,
                    &bounds.0,
                    &bounds.1,
                    &remainders.0,
                    &remainders.1,
                    s.work()?,
                )?;
                for sp in subsets {
                    let sp = sp?;
                    s.reserve_clone(con)?;
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
                &|mut p, s, _| {
                    p.sends = s;
                    Some(p)
                },
                var_level,
                wildcard,
            )
            .and_then(|_| {
                self.list_match_single_(
                    remainder.receives,
                    pattern.receives,
                    &|mut p, s, _| {
                        p.receives = s;
                        Some(p)
                    },
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.news,
                    pattern.news,
                    &|mut p, s, _| {
                        p.news = s;
                        Some(p)
                    },
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.reserve_slice(&pattern.exprs)?;
                self.list_match_single_(
                    remainder.exprs,
                    no_frees_exprs(&pattern.exprs),
                    &|mut p, s, _| {
                        p.exprs = s;
                        Some(p)
                    },
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.matches,
                    pattern.matches,
                    &|mut p, s, _| {
                        p.matches = s;
                        Some(p)
                    },
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.bundles,
                    pattern.bundles,
                    &|mut p, s, _| {
                        p.bundles = s;
                        Some(p)
                    },
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.unforgeables,
                    pattern.unforgeables,
                    &|mut p, s, _| {
                        p.unforgeables = s;
                        Some(p)
                    },
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.conditionals,
                    pattern.conditionals,
                    &|mut p, values, _| {
                        p.conditionals = values;
                        Some(p)
                    },
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.cost_signed_terms,
                    pattern.cost_signed_terms,
                    &|mut p, values, _| {
                        p.cost_signed_terms = values;
                        Some(p)
                    },
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.cost_stacks,
                    pattern.cost_stacks,
                    &|mut p, values, _| {
                        p.cost_stacks = values;
                        Some(p)
                    },
                    var_level,
                    wildcard,
                )
            })
        }
    }
}

impl<'a> SpatialMatcher<If, If> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: If, pattern: If) -> Option<()> {
        self.reserve_inspect(&target)?;
        self.reserve_inspect(&pattern)?;
        guard(target == pattern)
    }
}

impl<'a> SpatialMatcher<CostSignedTerm, CostSignedTerm> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: CostSignedTerm, pattern: CostSignedTerm) -> Option<()> {
        self.reserve_inspect(&target)?;
        self.reserve_inspect(&pattern)?;
        guard(target == pattern)
    }
}

impl<'a> SpatialMatcher<CostStack, CostStack> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: CostStack, pattern: CostStack) -> Option<()> {
        self.reserve_inspect(&target)?;
        self.reserve_inspect(&pattern)?;
        guard(target == pattern)
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - bundleSpatialMatcherInstance
// Apparently this code is never reached according to Scala code comment
impl<'a> SpatialMatcher<Bundle, Bundle> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: Bundle, pattern: Bundle) -> Option<()> {
        self.reserve_inspect(&target)?;
        self.reserve_inspect(&pattern)?;
        guard(pattern == target)
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - sendSpatialMatcherInstance
impl<'a> SpatialMatcher<Send, Send> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: Send, pattern: Send) -> Option<()> {
        self.reserve_inspect(&target)?;
        self.reserve_inspect(&pattern)?;
        let result = guard(target.persistent == pattern.persistent)
            .and_then(|_| self.spatial_match(target.chan.unwrap(), pattern.chan.unwrap()))
            .and_then(|_| self.fold_match(&target.data, &pattern.data, None));

        result.map(|_| ())
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - receiveSpatialMatcherInstance
impl<'a> SpatialMatcher<Receive, Receive> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: Receive, pattern: Receive) -> Option<()> {
        self.reserve_inspect(&target)?;
        self.reserve_inspect(&pattern)?;
        guard(target.persistent == pattern.persistent)
            .and_then(|_| self.list_match_single(target.binds, pattern.binds))
            .and_then(|_| self.spatial_match(target.body.unwrap(), pattern.body.unwrap()))
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - newSpatialMatcherInstance
impl<'a> SpatialMatcher<New, New> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: New, pattern: New) -> Option<()> {
        self.reserve_inspect(&target)?;
        self.reserve_inspect(&pattern)?;
        guard(target.bind_count == pattern.bind_count)
            .and_then(|_| self.spatial_match(target.p.unwrap(), pattern.p.unwrap()))
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - exprSpatialMatcherInstance
impl<'a> SpatialMatcher<Expr, Expr> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: Expr, pattern: Expr) -> Option<()> {
        self.reserve_inspect(&target)?;
        self.reserve_inspect(&pattern)?;
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
                        let Some(entries) = self.free_map.len().checked_add(1) else {
                            return self.reject(RSpaceError::HostWorkRejected);
                        };
                        let Some((operations, bytes)) =
                            shared::rust::collection_backing::tree_backing::<i32, Par>(entries)
                        else {
                            return self.reject(RSpaceError::HostWorkRejected);
                        };
                        let Some(backing) = bytes.checked_add(std::mem::size_of::<Expr>()) else {
                            return self.reject(RSpaceError::HostWorkRejected);
                        };
                        let Some(operations) = operations.checked_add(2) else {
                            return self.reject(RSpaceError::HostWorkRejected);
                        };
                        self.reserve(operations, bytes, backing)?;
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
                let work = self.work()?;
                let meter = |operations, scanned, backing| {
                    work.reserve(operations, scanned, backing)
                        .ok_or(BackingError::Rejected)
                };
                let tlist = match ParSetTypeMapper::eset_to_par_set_metered(t_set, &meter) {
                    Ok(set) => set.ps,
                    Err(_) => return self.reject(RSpaceError::HostWorkRejected),
                };
                let plist = match ParSetTypeMapper::eset_to_par_set_metered(p_set, &meter) {
                    Ok(set) => set.ps,
                    Err(_) => return self.reject(RSpaceError::HostWorkRejected),
                };

                let merger = |mut p: Par, r: Vec<Par>, work: &MatcherWork<'_>| {
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
                };

                self.list_match_single_(
                    tlist.sorted_pars,
                    plist.sorted_pars,
                    &merger,
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
                let work = self.work()?;
                let meter = |operations, scanned, backing| {
                    work.reserve(operations, scanned, backing)
                        .ok_or(BackingError::Rejected)
                };
                let tlist = match ParMapTypeMapper::emap_to_par_map_metered(t_emap, &meter) {
                    Ok(map) => map.ps,
                    Err(_) => return self.reject(RSpaceError::HostWorkRejected),
                };
                let plist = match ParMapTypeMapper::emap_to_par_map_metered(p_emap, &meter) {
                    Ok(map) => map.ps,
                    Err(_) => return self.reject(RSpaceError::HostWorkRejected),
                };

                let merger = |mut p: Par, r: Vec<(Par, Par)>, work: &MatcherWork<'_>| {
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
                };

                self.list_match_single_(
                    tlist.sorted_list,
                    plist.sorted_list,
                    &merger,
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
impl<'a> SpatialMatcher<Match, Match> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: Match, pattern: Match) -> Option<()> {
        self.reserve_inspect(&target)?;
        self.reserve_inspect(&pattern)?;
        let result = self
            .spatial_match(target.target.unwrap(), pattern.target.unwrap())
            .and_then(|_| self.fold_match(&target.cases, &pattern.cases, None));

        result.map(|_| ())
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - unfSpatialMatcherInstance
// Apparently this code is never reached according to Scala code comment
impl<'a> SpatialMatcher<GUnforgeable, GUnforgeable> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: GUnforgeable, pattern: GUnforgeable) -> Option<()> {
        self.reserve_inspect(&target)?;
        self.reserve_inspect(&pattern)?;
        match (target.unf_instance, pattern.unf_instance) {
            (Some(GPrivateBody(t)), Some(GPrivateBody(p))) => guard(t == p),
            (Some(GDeployerIdBody(t)), Some(GDeployerIdBody(p))) => guard(t == p),
            _ => None,
        }
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - receiveBindSpatialMatcherInstance
impl<'a> SpatialMatcher<ReceiveBind, ReceiveBind> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: ReceiveBind, pattern: ReceiveBind) -> Option<()> {
        self.reserve_inspect(&target)?;
        self.reserve_inspect(&pattern)?;
        guard(target.patterns == pattern.patterns)
            .and_then(|_| self.spatial_match(target.source.unwrap(), pattern.source.unwrap()))
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - matchCaseSpatialMatcherInstance
impl<'a> SpatialMatcher<MatchCase, MatchCase> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: MatchCase, pattern: MatchCase) -> Option<()> {
        self.reserve_inspect(&target)?;
        self.reserve_inspect(&pattern)?;
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
mod metered_tests {
    use super::*;

    #[test]
    fn list_remainder_rejects_before_free_map_publication() {
        let target = new_elist_expr(Vec::new(), Vec::new(), false, None);
        let pattern = new_elist_expr(Vec::new(), Vec::new(), false, Some(new_freevar_var(0)));
        let used = Mutex::new(0usize);
        let unlimited = |operations: usize, _: usize, _: usize| {
            *used.lock().unwrap() += operations;
            Ok(())
        };
        let mut context = SpatialMatcherContext::with_meter(&unlimited).unwrap();
        assert!(context
            .spatial_match(target.clone(), pattern.clone())
            .is_some());
        assert!(context.free_map.contains_key(&0));
        let limit = *used.lock().unwrap() - 1;
        let spent = Mutex::new(0usize);
        let meter = |operations: usize, _: usize, _: usize| {
            let mut current = spent.lock().unwrap();
            if *current + operations > limit {
                return Err(RSpaceError::HostWorkRejected);
            }
            *current += operations;
            Ok(())
        };
        let mut rejected = SpatialMatcherContext::with_meter(&meter).unwrap();
        assert!(rejected.spatial_match(target, pattern).is_none());
        assert!(rejected.free_map.is_empty());
        assert!(matches!(
            rejected.take_error(),
            Some(RSpaceError::HostWorkRejected)
        ));
    }

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
    fn malformed_map_pair_rejects_without_publication() {
        let target = Expr {
            expr_instance: Some(EMapBody(EMap {
                kvs: vec![KeyValuePair {
                    key: None,
                    value: Some(Par::default()),
                }],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let pattern = Expr {
            expr_instance: Some(EMapBody(EMap {
                kvs: Vec::new(),
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let meter = |_: usize, _: usize, _: usize| Ok(());
        let mut context = SpatialMatcherContext::with_meter(&meter).unwrap();
        assert!(context.spatial_match(target, pattern).is_none());
        assert!(context.free_map.is_empty());
        assert!(matches!(
            context.take_error(),
            Some(RSpaceError::HostWorkRejected)
        ));
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
            let mut ordinary = SpatialMatcherContext::new();
            assert!(ordinary
                .spatial_match(target.clone(), pattern.clone())
                .is_some());
            let meter = |_: usize, _: usize, _: usize| Ok(());
            let mut metered = SpatialMatcherContext::with_meter(&meter).unwrap();
            assert!(metered.spatial_match(target, pattern).is_some());
            assert_eq!(metered.free_map, ordinary.free_map);
            assert!(metered.take_error().is_none());
        }
    }

    #[test]
    fn set_and_map_conversion_reject_before_initial_allocation() {
        let element = new_gstring_par("x".repeat(4096), Vec::new(), false);
        let set = Expr {
            expr_instance: Some(ESetBody(ESet {
                ps: vec![element],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let set_meter = |_: usize, _: usize, backing: usize| {
            if backing >= 4096 {
                Err(RSpaceError::HostWorkRejected)
            } else {
                Ok(())
            }
        };
        let mut set_context = SpatialMatcherContext::with_meter(&set_meter).unwrap();
        assert!(set_context.spatial_match(set.clone(), set).is_none());
        assert!(matches!(
            set_context.take_error(),
            Some(RSpaceError::HostWorkRejected)
        ));

        let kvs: Vec<_> = (0..16)
            .map(|value| {
                new_key_value_pair(
                    new_gint_par(value, Vec::new(), false),
                    new_gint_par(value, Vec::new(), false),
                )
            })
            .collect();
        let threshold = kvs.len() * std::mem::size_of::<(Par, Par)>();
        let map = Expr {
            expr_instance: Some(EMapBody(EMap {
                kvs,
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let map_meter = |_: usize, _: usize, backing: usize| {
            if backing >= threshold {
                Err(RSpaceError::HostWorkRejected)
            } else {
                Ok(())
            }
        };
        let mut map_context = SpatialMatcherContext::with_meter(&map_meter).unwrap();
        assert!(map_context.spatial_match(map.clone(), map).is_none());
        assert!(matches!(
            map_context.take_error(),
            Some(RSpaceError::HostWorkRejected)
        ));
    }

    #[test]
    fn connective_clones_require_complete_host_credit() {
        let target_bytes = "target".len() * 4096;
        let binding_bytes = "binding".len() * 4096;
        let target = new_gstring_par("target".repeat(4096), Vec::new(), false);
        let binding = new_gstring_par("binding".repeat(4096), Vec::new(), false);
        for disjunction in [false, true] {
            let body = ConnectiveBody {
                ps: vec![Par::default()],
            };
            let pattern = Connective {
                connective_instance: Some(if disjunction {
                    ConnOrBody(body)
                } else {
                    ConnAndBody(body)
                }),
            };
            let used = Mutex::new([0usize; 3]);
            let unlimited = |operations: usize, scanned: usize, backing: usize| {
                let mut totals = used.lock().unwrap();
                for (total, amount) in totals.iter_mut().zip([operations, scanned, backing]) {
                    *total += amount;
                }
                Ok(())
            };
            let mut context = SpatialMatcherContext::with_meter(&unlimited).unwrap();
            context.free_map.insert(0, binding.clone());
            let actual = context.spatial_match(target.clone(), pattern.clone());
            assert!(context.take_error().is_none());
            let mut legacy = SpatialMatcherContext::new();
            legacy.free_map.insert(0, binding.clone());
            assert_eq!(
                actual,
                legacy.spatial_match(target.clone(), pattern.clone())
            );
            let required = *used.lock().unwrap();
            assert!(required[2] >= target_bytes);
            if disjunction {
                assert!(required[2] >= binding_bytes);
            }
            for dimension in 0..3 {
                let mut limit = required;
                limit[dimension] -= 1;
                let spent = Mutex::new([0usize; 3]);
                let meter = |operations: usize, scanned: usize, backing: usize| {
                    let mut totals = spent.lock().unwrap();
                    let amounts = [operations, scanned, backing];
                    if totals
                        .iter()
                        .zip(amounts)
                        .zip(limit)
                        .any(|((used, add), max)| *used + add > max)
                    {
                        return Err(RSpaceError::HostWorkRejected);
                    }
                    for (used, add) in totals.iter_mut().zip(amounts) {
                        *used += add;
                    }
                    Ok(())
                };
                let mut context = SpatialMatcherContext::with_meter(&meter).unwrap();
                context.free_map.insert(0, binding.clone());
                context.spatial_match(target.clone(), pattern.clone());
                assert!(matches!(
                    context.take_error(),
                    Some(RSpaceError::HostWorkRejected)
                ));
            }
        }
    }
}
