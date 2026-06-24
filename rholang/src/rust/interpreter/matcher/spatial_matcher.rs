// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - trait SpatialMatcher

use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::rholang::implicits::{single_expr, vector_par};
use models::rust::utils::*;

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
    ReceiveBind
);

pub trait SpatialMatcher<T, P> {
    fn spatial_match(&mut self, target: T, pattern: P) -> Option<()>;
}

#[derive(Clone)]
pub struct SpatialMatcherContext {
    pub free_map: FreeMap,
}

impl SpatialMatcherContext {
    pub fn new() -> Self {
        SpatialMatcherContext {
            free_map: new_free_map(),
        }
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
        self.spatial_match(target.0, pattern.0)
            .and_then(|_| self.spatial_match(target.1, pattern.1))
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - connectiveMatcher
impl SpatialMatcher<Par, Connective> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Par, pattern: Connective) -> Option<()> {
        match pattern.connective_instance {
            Some(ConnAndBody(connective_body)) => {
                connective_body.ps.into_iter().try_fold((), |_, p| {
                    let match_result = self.spatial_match(target.clone(), p);
                    match_result.map(|_| ())
                })
            }

            Some(ConnOrBody(connective_body)) => connective_body.ps.into_iter().find_map(|p| {
                let matches = self.free_map.clone();
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

            let filtered_pattern = no_frees(&pattern);
            let pc = ParCount::new(&filtered_pattern);
            let min_rem = pc.clone();
            let max_rem = if wildcard || !var_level.is_none() {
                pc._max()
            } else {
                pc.clone()
            };

            let individual_bounds: Vec<(ParCount, ParCount)> = filtered_pattern
                .connectives
                .iter()
                .map(|con| pc.min_max_con(con.clone()))
                .collect();

            let mut remainder_bounds: Vec<(ParCount, ParCount)> = vec![(min_rem, max_rem)];
            for bounds in individual_bounds.iter().rev() {
                let last = remainder_bounds.last().unwrap();
                remainder_bounds.push((bounds.0.add(&last.0), bounds.1.add(&last.1)));
            }
            remainder_bounds.pop();
            remainder_bounds.reverse();

            let connectives_with_bounds: Vec<(
                &Connective,
                &(ParCount, ParCount),
                &(ParCount, ParCount),
            )> = filtered_pattern
                .connectives
                .iter()
                .zip(individual_bounds.iter())
                .zip(remainder_bounds.iter())
                .map(|((connective, individual_bound), remainder_bound)| {
                    (connective, individual_bound, remainder_bound)
                })
                .collect();

            fn match_connective_with_bounds(
                s: &mut SpatialMatcherContext,
                target: Par,
                labeled_connective: (Connective, (ParCount, ParCount), (ParCount, ParCount)),
            ) -> Option<Par> {
                let (con, bounds, remainders) = labeled_connective;

                for sp in sub_pars(&target, &bounds.0, &bounds.1, &remainders.0, &remainders.1) {
                    if s.spatial_match(sp.0, con.clone()).is_some() {
                        return Some(sp.1);
                    }
                }
                None
            }

            let remainder = connectives_with_bounds.iter().try_fold(
                target,
                |acc, &(connective, bounds1, bounds2)| {
                    match_connective_with_bounds(
                        self,
                        acc,
                        (connective.clone(), bounds1.clone(), bounds2.clone()),
                    )
                },
            )?;

            self.list_match_single_(
                remainder.sends,
                pattern.sends,
                &|p, s| p.with_sends(s),
                var_level,
                wildcard,
            )
            .and_then(|_| {
                self.list_match_single_(
                    remainder.receives,
                    pattern.receives,
                    &|p, s| p.with_receives(s),
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.news,
                    pattern.news,
                    &|p, s| p.with_news(s),
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.exprs,
                    no_frees_exprs(&pattern.exprs),
                    &|p, s| p.with_exprs(s),
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.matches,
                    pattern.matches,
                    &|p, s| p.with_matches(s),
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.bundles,
                    pattern.bundles,
                    &|p, s| p.with_bundles(s),
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.unforgeables,
                    pattern.unforgeables,
                    &|p, s| p.with_unforgeables(s),
                    var_level,
                    wildcard,
                )
            })
        }
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - bundleSpatialMatcherInstance
// Apparently this code is never reached according to Scala code comment
impl SpatialMatcher<Bundle, Bundle> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Bundle, pattern: Bundle) -> Option<()> {
        guard(pattern == target)
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - sendSpatialMatcherInstance
impl SpatialMatcher<Send, Send> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Send, pattern: Send) -> Option<()> {
        let result = guard(target.persistent == pattern.persistent)
            .and_then(|_| self.spatial_match(target.chan.unwrap(), pattern.chan.unwrap()))
            .and_then(|_| self.fold_match(target.data, pattern.data, None));

        result.map(|_| ())
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - receiveSpatialMatcherInstance
impl SpatialMatcher<Receive, Receive> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Receive, pattern: Receive) -> Option<()> {
        guard(target.persistent == pattern.persistent)
            .and_then(|_| self.list_match_single(target.binds, pattern.binds))
            .and_then(|_| self.spatial_match(target.body.unwrap(), pattern.body.unwrap()))
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - newSpatialMatcherInstance
impl SpatialMatcher<New, New> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: New, pattern: New) -> Option<()> {
        guard(target.bind_count == pattern.bind_count)
            .and_then(|_| self.spatial_match(target.p.unwrap(), pattern.p.unwrap()))
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - exprSpatialMatcherInstance
impl SpatialMatcher<Expr, Expr> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Expr, pattern: Expr) -> Option<()> {
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
                let matched_rem = self.fold_match(tlist, plist, rem.clone())?;

                match &rem {
                    Some(Var {
                        var_instance: Some(FreeVar(level)),
                    }) => {
                        self.free_map.insert(
                            *level,
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
            ) => self.fold_match(tlist, plist, None).map(|_| ()),

            (
                Some(ESetBody(
                    t_set @ ESet {
                        ps: _,
                        locally_free: _,
                        connective_used: _,
                        remainder: _,
                    },
                )),
                Some(ESetBody(
                    ref p_set @ ESet {
                        ps: _,
                        locally_free: _,
                        connective_used: _,
                        remainder: ref rem,
                    },
                )),
            ) => {
                let tlist = ParSetTypeMapper::eset_to_par_set(t_set.clone()).ps;
                let plist = ParSetTypeMapper::eset_to_par_set(p_set.clone()).ps;

                let is_wildcard = match rem {
                    Some(Var {
                        var_instance: Some(Wildcard(_)),
                    }) => true,
                    _ => false,
                };

                let remainder_var_opt = match rem {
                    Some(Var {
                        var_instance: Some(FreeVar(level)),
                    }) => Some(level),
                    _ => None,
                };

                let merger = |p: Par, r: Vec<Par>| {
                    p.with_exprs(vec![new_eset_expr(r, Vec::new(), false, None)])
                };

                self.list_match_single_(
                    tlist.sorted_pars,
                    plist.sorted_pars,
                    &merger,
                    remainder_var_opt.copied(),
                    is_wildcard,
                )
            }

            (
                Some(EMapBody(
                    t_emap @ EMap {
                        kvs: _,
                        locally_free: _,
                        connective_used: _,
                        remainder: _,
                    },
                )),
                Some(EMapBody(
                    ref p_emap @ EMap {
                        kvs: _,
                        locally_free: _,
                        connective_used: _,
                        remainder: ref rem,
                    },
                )),
            ) => {
                let tlist = ParMapTypeMapper::emap_to_par_map(t_emap.clone()).ps;
                let plist = ParMapTypeMapper::emap_to_par_map(p_emap.clone()).ps;

                let is_wildcard = match rem {
                    Some(Var {
                        var_instance: Some(Wildcard(_)),
                    }) => true,
                    _ => false,
                };

                let remainder_var_opt = match rem {
                    Some(Var {
                        var_instance: Some(FreeVar(level)),
                    }) => Some(level),
                    _ => None,
                };

                let merger = |p: Par, r: Vec<(Par, Par)>| {
                    p.with_exprs(vec![new_emap_expr(
                        r.into_iter()
                            .map(|(k, v)| KeyValuePair {
                                key: Some(k),
                                value: Some(v),
                            })
                            .collect(),
                        Vec::new(),
                        false,
                        None,
                    )])
                };

                self.list_match_single_(
                    tlist.sorted_list,
                    plist.sorted_list,
                    &merger,
                    remainder_var_opt.copied(),
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
        let result = self
            .spatial_match(target.target.unwrap(), pattern.target.unwrap())
            .and_then(|_| self.fold_match(target.cases, pattern.cases, None));

        result.map(|_| ())
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - unfSpatialMatcherInstance
// Apparently this code is never reached according to Scala code comment
impl SpatialMatcher<GUnforgeable, GUnforgeable> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: GUnforgeable, pattern: GUnforgeable) -> Option<()> {
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
        guard(target.patterns == pattern.patterns)
            .and_then(|_| self.spatial_match(target.source.unwrap(), pattern.source.unwrap()))
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - matchCaseSpatialMatcherInstance
impl SpatialMatcher<MatchCase, MatchCase> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: MatchCase, pattern: MatchCase) -> Option<()> {
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
