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

    /// The reference entry to the
    /// Par-pair matcher — the exact branch structure of
    /// `SpatialMatcher<Par, Par>::spatial_match` with the ownership decision
    /// hoisted to the branch that needs it:
    ///
    /// * `!pattern.connective_used` — the non-binding comparison. The owned
    ///   impl already ran `guard(match_pars(&target, &pattern))` BY
    ///   REFERENCE; entering it through borrows makes the whole path (the
    ///   discard-heavy failure case of multi-datum channels) copy-free.
    /// * `connective_used` — a binding/connective pattern. The pair is
    ///   cloned ONCE into the owned lattice (sub_pars/list_match construct
    ///   owned intermediates and move bound subterms into the free_map —
    ///   that machinery is deliberately byte-untouched). For the common
    ///   free-var bind the clone IS the bound-candidate copy the
    ///   continuation env receives; it is discarded only when a connective
    ///   pattern fails midway (the earlier value-shaped behavior, minus the two
    ///   unconditional entry copies).
    pub fn spatial_match_par_ref(&mut self, target: &Par, pattern: &Par) -> Option<()> {
        if !pattern.connective_used {
            // Same predicate as the owned impl's first arm — already
            // reference-based there.
            guard(match_pars(target, pattern))
        } else {
            let __clone_start = std::time::Instant::now();
            let target_owned = target.clone();
            let pattern_owned = pattern.clone();
            metrics::counter!(
                crate::rust::interpreter::metrics_constants::RHOLANG_MATCHER_FOLD_MATCH_TAIL_CLONE_NS_METRIC,
                "source" => crate::rust::interpreter::metrics_constants::RHOLANG_METRICS_SOURCE
            )
            .increment(__clone_start.elapsed().as_nanos() as u64);
            self.spatial_match(target_owned, pattern_owned)
        }
    }
}

/// The matcher's search state is exactly one free map, so the isolation law
/// (`models::rust::utils::isolate_free_map`) applies to it whole.
///
/// One line, on purpose: the law itself lives at ONE address, in `models`,
/// beside the `FreeMap` it is about. A second copy of the snapshot/restore here
/// would be the same defect `a1feb437` cost — a law with copies is a law that
/// drifts.
impl IsolatableState for SpatialMatcherContext {
    fn free_map_mut(&mut self) -> &mut FreeMap { &mut self.free_map }
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
        // println!("\nHit Par, Connective");
        // println!("\ntarget in Par, Connective: {:?}\n", target);
        // println!("\npattern in Par, Connective: {:?}\n", pattern);

        match pattern.connective_instance {
            // ★ A REFUSED CONJUNCTION UN-BINDS **ALL** OF ITS CONJUNCTS.
            //
            // A conjunction is a sequential fold and the right conjunct sees the
            // left one's bindings on purpose (`p_conjunction_normalizer.rs:12-14`
            // — the conjuncts share one free map, unlike `~` and `\/`, whose
            // bodies get a fresh one). So `free_var(0) /\ 8` against the target
            // `7` binds level 0 from the target and only THEN demands `8`: it
            // writes before it refuses.
            //
            // The isolation therefore goes around the WHOLE fold, not around each
            // conjunct. A per-conjunct restore would revert only the conjunct that
            // refused and leave its predecessors' bindings behind — which is the
            // whole point of `try_fold`: the conjuncts stand or fall together.
            //
            // `_keeping_bindings`, because a conjunction that SUCCEEDS binds the
            // union of its conjuncts and that union is the pattern's real answer.
            // The disjunction below is the opposite disposition, at the same site,
            // for a stated reason — the two are not variations on a theme.
            Some(ConnAndBody(connective_body)) => {
                // println!("\nhit ConnAndBody");
                // println!("\ntarget in ConnAndBody: {:?}", target);
                // println!("\nps in ConnAndBody: {:?}", connective_body.ps);

                attempt_opt_keeping_bindings(self, |s| {
                    connective_body.ps.into_iter().try_fold((), |_, p| {
                        // println!("\ncalling spatial match in ConnAndBody");
                        let match_result = s.spatial_match(target.clone(), p);
                        if match_result.is_some() {
                            // println!("\nfinished calling spatialMatch in ConnAndBody");
                        }
                        match_result.map(|_| ())
                    })
                })
                .into_option()
            }

            // ★ A DISJUNCTION BRANCH OWNS ITS OWN STATE, ON BOTH PATHS.
            //
            // Scala unites the branches with `Alternative_[F].unite`
            // (`SpatialMatcher.scala:377-386`), which hands EVERY branch the
            // same input state; a branch that fails is an empty stream whose
            // bindings never propagate anywhere. The explicit
            // `freeMap.set(matches)` there is needed only for the SUCCESS path
            // — a disjunction binds nothing, because its branches disagree
            // about which variables they would bind.
            //
            // The port kept the success half and lost the failure half: `?` on
            // the branch returned from the CLOSURE, skipping the restore. A
            // refused branch's bindings were therefore still in `free_map` when
            // the next branch was attempted, and escaped to the caller when
            // every branch refused. Restoring unconditionally is the whole fix,
            // and it is the same invariant as task 144's `match_function`
            // isolation at a distinct site: an attempt owns its own state.
            //
            // Verdict-invariant by construction — no branch's decision reads
            // `free_map`, so which branch `find_map` selects cannot change.
            //
            // ★ RE-EXPRESSED THROUGH THE COMBINATOR, BEHAVIOUR UNCHANGED. The
            // hand-written clone/run/restore above was byte-for-byte what
            // `attempt_opt` does, so this branch is the CONTROL for the three
            // sites below it: if the combinator did anything other than what the
            // fix for `eaa905fe` did, this arm would move, and
            // `matcher_disjunction_isolation.rs` — untouched since that fix —
            // would say so.
            Some(ConnOrBody(connective_body)) => connective_body.ps.into_iter().find_map(|p| {
                attempt_opt(self, |s| s.spatial_match(target.clone(), p)).into_option()
            }),

            // ★ A NEGATION THAT SUCCEEDS BOUND NOTHING — AND NEITHER DID ONE THAT
            //   REFUSED.
            //
            // This arm INVERTS the disposition, and inverting it through a bare
            // `Option` is what hid the leak: "the inner attempt refused" and "the
            // negation succeeded" are both `Some(())`, "the inner attempt
            // succeeded" and "the negation refused" are both `None`. Two distinct
            // facts per cell, one of which is about state and was silently
            // dropped. `Attempt` separates them, so both arms below are written —
            // and both are reached with `free_map` already restored, because
            // `attempt_opt` restores unconditionally, inside.
            //
            // `attempt_opt` and not `_keeping_bindings`: a negation binds NOTHING,
            // ever. `~P`'s body is normalized against a fresh `FreeMap` which
            // `combine_p_negation` then discards
            // (`p_negation_normalizer.rs:10-13, 65-89`), so a free variable under
            // `~` is `FreeVar(0)` in a numbering nobody kept — and level 0 is very
            // probably somebody ELSE's level in the shared map it would leak into.
            // The enclosing `MatchCase.free_count` counts none of it.
            //
            // ⚠ The two branches this replaces were BYTE-IDENTICAL: `has_or_body`
            // was computed, matched on, and then both arms did exactly the same
            // thing. Deleting it is behaviour-free — `control_a_negation_over_a_
            // disjunction_is_not_a_special_case` pins that rather than trusting it
            // — and it is deleted rather than kept because two copies of a law
            // that has just become non-trivial is how the next drift starts.
            Some(ConnNotBody(p)) => match attempt_opt(self, |s| s.spatial_match(target, p)) {
                Attempt::Bound(_) => None, // the inner pattern matched → the negation refuses
                Attempt::Refused => Some(()), // the inner pattern refused → the negation succeeds
            },

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
                }) => {
                    // println!("ConnByteArray returning some");
                    Some(())
                }
                _ => {
                    // println!("ConnByteArray returning none");
                    None
                }
            },

            None => None,
        }
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - parSpatialMatcher
impl SpatialMatcher<Par, Par> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Par, mut pattern: Par) -> Option<()> {
        // println!("\nhit Par, Par");
        // println!("\ntarget in Par, Par: {:?}", target);
        // println!("\npattern in Par, Par: {:?}", pattern);

        if !pattern.connective_used {
            // guard(pattern == target)
            // println!("\nHit guard in Par, Par");
            guard(match_pars(&target, &pattern))
        } else {
            // println!("\npassed guard in Par, Par");

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

            // println!("var_level: {:?}", var_level);

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

            // println!("wildcard: {:?}", wildcard);

            let filtered_pattern = no_frees(&pattern);
            // println!("filtered_pattern: {:?}", filtered_pattern);
            let pc = ParCount::new(&filtered_pattern);
            // println!("pc: {:?}", pc);
            let min_rem = pc.clone();
            let max_rem = if wildcard || !var_level.is_none() {
                pc._max()
            } else {
                pc.clone()
            };
            // println!("\nmin_rem: {:?}", min_rem);
            // println!("\nmax_rem: {:?}", max_rem);

            let individual_bounds: Vec<(ParCount, ParCount)> = filtered_pattern
                .connectives
                .iter()
                .map(|con| pc.min_max_con(con.clone()))
                .collect();
            // println!("\nindividual_bounds: {:?}", individual_bounds);

            let mut remainder_bounds: Vec<(ParCount, ParCount)> = vec![(min_rem, max_rem)];
            for bounds in individual_bounds.iter().rev() {
                let last = remainder_bounds
                    .last()
                    .expect("remainder_bounds is seeded with one element before this loop");
                remainder_bounds.push((bounds.0.add(&last.0), bounds.1.add(&last.1)));
            }
            remainder_bounds.pop();
            remainder_bounds.reverse();

            // println!("\nremainder_bounds: {:?}", remainder_bounds);

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

            // println!("\nconnectives_with_bounds length: {:?}", connectives_with_bounds.len());

            /// ★ THE RETRY LOOP — NOT AN ARM, AND THAT IS WHY IT NEEDS ITS OWN
            ///   APPLICATION OF THE LAW.
            ///
            /// This is the loop that *drives* the connective arms: it offers the
            /// connective one split of the target after another and keeps the
            /// first split the connective accepts. Every rejected split is an
            /// attempt, and a rejected attempt must not ride into the accepted
            /// one. No snapshot taken *inside* an arm can see this — the loop is
            /// above all of them — and no snapshot at the enclosing
            /// `spatial_match(Par, Par)` boundary can either, because the whole
            /// walk is interior to one such call.
            ///
            /// `_keeping_bindings`, because the accepted split's bindings are the
            /// answer: they are what `return Some(sp.1)` is carrying home.
            ///
            /// ⚠ The fan-out here is the widest in the matcher — `ConnNotBody`'s
            /// `min_max_con` is `(_new(), _max())` (`par_count.rs`), i.e. the full
            /// subset lattice of the target — so this is the one site where the
            /// entry clone can be paid many times per match. Two things bound it:
            /// the elision below, and the fact that TODAY (before this fix)
            /// `free_map` grows monotonically across candidates, so under the
            /// restore each clone is `O(entry)` rather than `O(entry + everything
            /// every rejected candidate leaked)`.
            fn match_connective_with_bounds(
                s: &mut SpatialMatcherContext,
                target: Par,
                labeled_connective: (Connective, (ParCount, ParCount), (ParCount, ParCount)),
            ) -> Option<Par> {
                // println!("\nhit match_connective_with_bounds");
                let (con, bounds, remainders) = labeled_connective;

                // ★ A PROVED ELISION, NOT A HEURISTIC — the same move task #144
                // made at `match_function`'s non-connective arm.
                //
                // The whitelist is of the arms that PROVABLY cannot reach
                // `free_map`: `ConnBool`/`ConnInt`/`ConnString`/`ConnUri`/
                // `ConnByteArray` are each a pure `match single_expr(&target)`,
                // `VarRefBody` and `None` are each a bare `None`. Read the
                // `SpatialMatcher<Par, Connective>` impl above: those seven arms
                // contain no call at all, so there is nothing to isolate and the
                // clone would be pure cost.
                //
                // It is stated as "elide on exactly these" rather than "isolate on
                // exactly those" so that a NEW connective variant lands in the
                // isolated branch by default. A safe default is worth the double
                // negative.
                let free_map_is_unreachable = matches!(
                    con.connective_instance,
                    Some(ConnBool(_))
                        | Some(ConnInt(_))
                        | Some(ConnString(_))
                        | Some(ConnUri(_))
                        | Some(ConnByteArray(_))
                        | Some(VarRefBody(_))
                        | None
                );

                for sp in sub_pars(&target, &bounds.0, &bounds.1, &remainders.0, &remainders.1) {
                    // println!("\ntarget in match_connective_with_bounds: {:?}", target);
                    // println!("\nsp_0 in match_connective_with_bounds: {:?}", sp.clone().0);
                    // println!("\nsp_1 in match_connective_with_bounds: {:?}", sp.clone().1);
                    // println!("\ncalling spatialMatch in match_connective_with_bounds");

                    let (candidate, remainder) = sp;
                    let accepted = if free_map_is_unreachable {
                        s.spatial_match(candidate, con.clone()).is_some()
                    } else {
                        matches!(
                            attempt_opt_keeping_bindings(s, |s| s
                                .spatial_match(candidate, con.clone())),
                            Attempt::Bound(_)
                        )
                    };

                    if accepted {
                        // println!("\nfinished calling spatialMatch in match_connective_with_bounds");
                        // println!("\nreturning sp.1: {:?}", sp.1);
                        return Some(remainder);
                    }
                }
                None
            }

            let mut remainder = connectives_with_bounds.iter().try_fold(
                target,
                |acc, &(connective, bounds1, bounds2)| {
                    match_connective_with_bounds(
                        self,
                        acc,
                        (connective.clone(), bounds1.clone(), bounds2.clone()),
                    )
                },
            )?;
            // println!("\nRemainder: {:?}", remainder);

            self.list_match_single_(
                std::mem::take(&mut remainder.sends),
                std::mem::take(&mut pattern.sends),
                &|p, s| p.with_sends(s),
                var_level,
                wildcard,
            )
            .and_then(|_| {
                self.list_match_single_(
                    std::mem::take(&mut remainder.receives),
                    std::mem::take(&mut pattern.receives),
                    &|p, s| p.with_receives(s),
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    std::mem::take(&mut remainder.news),
                    std::mem::take(&mut pattern.news),
                    &|p, s| p.with_news(s),
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    std::mem::take(&mut remainder.exprs),
                    no_frees_exprs(&pattern.exprs),
                    &|p, s| p.with_exprs(s),
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    std::mem::take(&mut remainder.matches),
                    std::mem::take(&mut pattern.matches),
                    &|p, s| p.with_matches(s),
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    std::mem::take(&mut remainder.bundles),
                    std::mem::take(&mut pattern.bundles),
                    &|p, s| p.with_bundles(s),
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    std::mem::take(&mut remainder.unforgeables),
                    std::mem::take(&mut pattern.unforgeables),
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
        // println!("\nHit Bundle, Bundle");
        // println!("Target: {:?}\n", target);
        // println!("Pattern: {:?}", pattern);
        guard(pattern == target)
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - sendSpatialMatcherInstance
impl SpatialMatcher<Send, Send> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Send, pattern: Send) -> Option<()> {
        // println!("\nHit Send, Send");
        // println!("\ntarget in send, send: {:?}", target);
        // println!("\npattern in send, send: {:?}", pattern);

        let result = guard(target.persistent == pattern.persistent)
            .and_then(|_| {
                // println!("\ncalling spatial_match in Send, Send");
                self.spatial_match(
                    target.chan.expect("Send.chan (target)"),
                    pattern.chan.expect("Send.chan (pattern)"),
                )
            })
            .and_then(|_| {
                // println!("\npassed calling spatial_match in Send, Send");
                self.fold_match(&target.data, &pattern.data, None)
            });

        result.map(|_| ())
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - receiveSpatialMatcherInstance
impl SpatialMatcher<Receive, Receive> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Receive, pattern: Receive) -> Option<()> {
        // println!("\nHit Receive, Receive");
        guard(target.persistent == pattern.persistent)
            .and_then(|_| self.list_match_single(target.binds, pattern.binds))
            .and_then(|_| {
                self.spatial_match(
                    target.body.expect("Receive.body (target)"),
                    pattern.body.expect("Receive.body (pattern)"),
                )
            })
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - newSpatialMatcherInstance
impl SpatialMatcher<New, New> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: New, pattern: New) -> Option<()> {
        // println!("\nHit New, New");
        guard(target.bind_count == pattern.bind_count).and_then(|_| {
            self.spatial_match(
                target.p.expect("New.p (target)"),
                pattern.p.expect("New.p (pattern)"),
            )
        })
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - exprSpatialMatcherInstance
impl SpatialMatcher<Expr, Expr> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Expr, pattern: Expr) -> Option<()> {
        // println!("\nHit Expr, Expr");
        // println!("\nExpr, Expr target: {:?}", target);
        // println!("\nExpr, Expr pattern: {:?}", pattern);

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
                // println!("\n calling fold_match in ElistBody");
                let matched_rem = self.fold_match(&tlist, &plist, rem.clone())?;
                // println!("\nmatched_rem: {:?}", matched_rem);

                // println!("\ncurrent free_map: {:#?}", self.free_map);

                match &rem {
                    Some(Var {
                        var_instance: Some(FreeVar(level)),
                    }) => {
                        // println!("\nmodifying free_map in EListBody");s
                        self.free_map.insert(
                            *level,
                            new_elist_par(matched_rem, Vec::new(), false, None, Vec::new(), false),
                        );
                        // println!("\nfree_map after insert: {:#?}", self.free_map);
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

                // println!("\ncalling list_match_single_ in ESetBody");
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

                // println!("\ncalling list_match_single_ in EMapBody");
                self.list_match_single_(
                    tlist.sorted_list,
                    plist.sorted_list,
                    &merger,
                    remainder_var_opt.copied(),
                    is_wildcard,
                )
            }

            (Some(EPathmapBody(target)), Some(EPathmapBody(pattern))) => {
                self.spatial_match_epathmap(target, pattern)
            }

            (Some(EVarBody(EVar { v: vp })), Some(EVarBody(EVar { v: vt }))) => guard(vp == vt),

            (Some(ENotBody(ENot { p: t })), Some(ENotBody(ENot { p }))) => {
                self.spatial_match(t.expect("ENot.p (target)"), p.expect("ENot.p (pattern)"))
            }

            (Some(ENegBody(ENeg { p: t })), Some(ENegBody(ENeg { p }))) => {
                self.spatial_match(t.expect("ENeg.p (target)"), p.expect("ENeg.p (pattern)"))
            }

            (Some(EMultBody(EMult { p1: t1, p2: t2 })), Some(EMultBody(EMult { p1, p2 }))) => self
                .spatial_match(
                    t1.expect("EMult.p1 (target)"),
                    p1.expect("EMult.p1 (pattern)"),
                )
                .and_then(|_| {
                    self.spatial_match(
                        t2.expect("EMult.p2 (target)"),
                        p2.expect("EMult.p2 (pattern)"),
                    )
                }),

            (Some(EDivBody(EDiv { p1: t1, p2: t2 })), Some(EDivBody(EDiv { p1, p2 }))) => self
                .spatial_match(
                    t1.expect("EDiv.p1 (target)"),
                    p1.expect("EDiv.p1 (pattern)"),
                )
                .and_then(|_| {
                    self.spatial_match(
                        t2.expect("EDiv.p2 (target)"),
                        p2.expect("EDiv.p2 (pattern)"),
                    )
                }),

            (Some(EModBody(EMod { p1: t1, p2: t2 })), Some(EModBody(EMod { p1, p2 }))) => self
                .spatial_match(
                    t1.expect("EMod.p1 (target)"),
                    p1.expect("EMod.p1 (pattern)"),
                )
                .and_then(|_| {
                    self.spatial_match(
                        t2.expect("EMod.p2 (target)"),
                        p2.expect("EMod.p2 (pattern)"),
                    )
                }),

            (
                Some(EPercentPercentBody(EPercentPercent { p1: t1, p2: t2 })),
                Some(EPercentPercentBody(EPercentPercent { p1, p2 })),
            ) => self
                .spatial_match(
                    t1.expect("EPercentPercent.p1 (target)"),
                    p1.expect("EPercentPercent.p1 (pattern)"),
                )
                .and_then(|_| {
                    self.spatial_match(
                        t2.expect("EPercentPercent.p2 (target)"),
                        p2.expect("EPercentPercent.p2 (pattern)"),
                    )
                }),

            (Some(EPlusBody(EPlus { p1: t1, p2: t2 })), Some(EPlusBody(EPlus { p1, p2 }))) => self
                .spatial_match(
                    t1.expect("EPlus.p1 (target)"),
                    p1.expect("EPlus.p1 (pattern)"),
                )
                .and_then(|_| {
                    self.spatial_match(
                        t2.expect("EPlus.p2 (target)"),
                        p2.expect("EPlus.p2 (pattern)"),
                    )
                }),

            (
                Some(EPlusPlusBody(EPlusPlus { p1: t1, p2: t2 })),
                Some(EPlusPlusBody(EPlusPlus { p1, p2 })),
            ) => self
                .spatial_match(
                    t1.expect("EPlusPlus.p1 (target)"),
                    p1.expect("EPlusPlus.p1 (pattern)"),
                )
                .and_then(|_| {
                    self.spatial_match(
                        t2.expect("EPlusPlus.p2 (target)"),
                        p2.expect("EPlusPlus.p2 (pattern)"),
                    )
                }),

            (
                Some(EMinusMinusBody(EMinusMinus { p1: t1, p2: t2 })),
                Some(EMinusMinusBody(EMinusMinus { p1, p2 })),
            ) => self
                .spatial_match(
                    t1.expect("EMinusMinus.p1 (target)"),
                    p1.expect("EMinusMinus.p1 (pattern)"),
                )
                .and_then(|_| {
                    self.spatial_match(
                        t2.expect("EMinusMinus.p2 (target)"),
                        p2.expect("EMinusMinus.p2 (pattern)"),
                    )
                }),

            // ---- the arms below were absent, and their absence was silent ----
            //
            // Every one of these variants carries child `Par`s, so a pattern
            // built from one can carry a free variable — `connective_used` is
            // `true` and `list_match::match_function` routes the pair HERE
            // rather than to its `guard(t == p)` fast path. With no arm, the
            // pair fell to `_ => None`: the pattern matched nothing, the COMM
            // never fired, the receive rested forever, and nothing was
            // reported. See `models::rust::rholang::par_children::
            // spatial_match_descends_into` for the per-variant disposition and
            // `rholang/tests/spatial_matcher_disposition.rs` for the gate that
            // makes a future omission fail the suite instead of the network.
            //
            // ⚠ Adding them WIDENS the match relation: a program that rests
            // today will fire. That is a protocol change and needs a
            // coordinated version bump, which is F1r3node's decision to take —
            // `Validate::version` is exact equality with no activation-height
            // machinery.

            // Subtraction. Its six arithmetic siblings (`*`, `/`, `%`, `+`,
            // `++`, `--`) and `%%` were all present with identical bodies;
            // `-` alone was not. No design produces that asymmetry.
            (Some(EMinusBody(EMinus { p1: t1, p2: t2 })), Some(EMinusBody(EMinus { p1, p2 }))) => {
                self.spatial_match(
                    t1.expect("EMinus.p1 (target)"),
                    p1.expect("EMinus.p1 (pattern)"),
                )
                .and_then(|_| {
                    self.spatial_match(
                        t2.expect("EMinus.p2 (target)"),
                        p2.expect("EMinus.p2 (pattern)"),
                    )
                })
            }

            // The six comparisons. Same `p1`/`p2` shape, same role in the AST,
            // same treatment.
            (Some(ELtBody(ELt { p1: t1, p2: t2 })), Some(ELtBody(ELt { p1, p2 }))) => self
                .spatial_match(t1.expect("ELt.p1 (target)"), p1.expect("ELt.p1 (pattern)"))
                .and_then(|_| {
                    self.spatial_match(t2.expect("ELt.p2 (target)"), p2.expect("ELt.p2 (pattern)"))
                }),

            (Some(ELteBody(ELte { p1: t1, p2: t2 })), Some(ELteBody(ELte { p1, p2 }))) => self
                .spatial_match(
                    t1.expect("ELte.p1 (target)"),
                    p1.expect("ELte.p1 (pattern)"),
                )
                .and_then(|_| {
                    self.spatial_match(
                        t2.expect("ELte.p2 (target)"),
                        p2.expect("ELte.p2 (pattern)"),
                    )
                }),

            (Some(EGtBody(EGt { p1: t1, p2: t2 })), Some(EGtBody(EGt { p1, p2 }))) => self
                .spatial_match(t1.expect("EGt.p1 (target)"), p1.expect("EGt.p1 (pattern)"))
                .and_then(|_| {
                    self.spatial_match(t2.expect("EGt.p2 (target)"), p2.expect("EGt.p2 (pattern)"))
                }),

            (Some(EGteBody(EGte { p1: t1, p2: t2 })), Some(EGteBody(EGte { p1, p2 }))) => self
                .spatial_match(
                    t1.expect("EGte.p1 (target)"),
                    p1.expect("EGte.p1 (pattern)"),
                )
                .and_then(|_| {
                    self.spatial_match(
                        t2.expect("EGte.p2 (target)"),
                        p2.expect("EGte.p2 (pattern)"),
                    )
                }),

            (Some(EEqBody(EEq { p1: t1, p2: t2 })), Some(EEqBody(EEq { p1, p2 }))) => self
                .spatial_match(t1.expect("EEq.p1 (target)"), p1.expect("EEq.p1 (pattern)"))
                .and_then(|_| {
                    self.spatial_match(t2.expect("EEq.p2 (target)"), p2.expect("EEq.p2 (pattern)"))
                }),

            (Some(ENeqBody(ENeq { p1: t1, p2: t2 })), Some(ENeqBody(ENeq { p1, p2 }))) => self
                .spatial_match(
                    t1.expect("ENeq.p1 (target)"),
                    p1.expect("ENeq.p1 (pattern)"),
                )
                .and_then(|_| {
                    self.spatial_match(
                        t2.expect("ENeq.p2 (target)"),
                        p2.expect("ENeq.p2 (pattern)"),
                    )
                }),

            // The two boolean connectives of the EXPRESSION language. Note that
            // these are `and`/`or` over evaluated processes — `ConnAndBody` /
            // `ConnOrBody`, the pattern connectives `/\` and `\/`, are a
            // different node handled by `SpatialMatcher<Par, Connective>`.
            (Some(EAndBody(EAnd { p1: t1, p2: t2 })), Some(EAndBody(EAnd { p1, p2 }))) => self
                .spatial_match(
                    t1.expect("EAnd.p1 (target)"),
                    p1.expect("EAnd.p1 (pattern)"),
                )
                .and_then(|_| {
                    self.spatial_match(
                        t2.expect("EAnd.p2 (target)"),
                        p2.expect("EAnd.p2 (pattern)"),
                    )
                }),

            (Some(EOrBody(EOr { p1: t1, p2: t2 })), Some(EOrBody(EOr { p1, p2 }))) => self
                .spatial_match(t1.expect("EOr.p1 (target)"), p1.expect("EOr.p1 (pattern)"))
                .and_then(|_| {
                    self.spatial_match(t2.expect("EOr.p2 (target)"), p2.expect("EOr.p2 (pattern)"))
                }),

            // A method call. The NAME is compared by equality — `x.nth(0)` is
            // not a candidate match for `x.length()` no matter what binds — and
            // the ARGUMENTS go through `fold_match` with no remainder, which is
            // positional and exact-arity: argument lists are ordered and a
            // method of a different arity is a different call.
            (
                Some(EMethodBody(EMethod {
                    method_name: t_name,
                    target: t_target,
                    arguments: t_arguments,
                    locally_free: _,
                    connective_used: _,
                })),
                Some(EMethodBody(EMethod {
                    method_name: p_name,
                    target: p_target,
                    arguments: p_arguments,
                    locally_free: _,
                    connective_used: _,
                })),
            ) => guard(t_name == p_name)
                .and_then(|_| {
                    self.spatial_match(
                        t_target.expect("EMethod.target (target)"),
                        p_target.expect("EMethod.target (pattern)"),
                    )
                })
                .and_then(|_| {
                    self.fold_match(&t_arguments, &p_arguments, None)
                        .map(|_| ())
                }),

            // `p matches q`. The `target` slot is descended into; the `pattern`
            // slot is compared by EQUALITY.
            //
            // That asymmetry is not a shortcut, it is the binding structure.
            // `EMatches`'s `pattern` is a nested pattern living at depth + 1, so
            // its connectives are not "used" in the enclosing scope —
            // `has_locally_free` computes this node's `connective_used` from the
            // TARGET alone, precisely so that a `matches` right-hand side never
            // introduces a binder here. Descending into it would also be
            // actively wrong: for `@{x matches Int}` against `@{5 matches Int}`
            // the right-hand sides are `ConnInt` CONNECTIVE `Par`s, and
            // `SpatialMatcher<Par, Connective>`'s `ConnInt` arm demands a
            // `GInt` EXPRESSION on the target side, so the obvious case would
            // stop matching. Equality is the same treatment
            // `SpatialMatcher<ReceiveBind, ReceiveBind>` gives `patterns` and
            // `SpatialMatcher<MatchCase, MatchCase>` gives `pattern`, a few
            // impls below.
            (
                Some(EMatchesBody(EMatches {
                    target: t_target,
                    pattern: t_pattern,
                })),
                Some(EMatchesBody(EMatches {
                    target: p_target,
                    pattern: p_pattern,
                })),
            ) => guard(t_pattern == p_pattern).and_then(|_| {
                self.spatial_match(
                    t_target.expect("EMatches.target (target)"),
                    p_target.expect("EMatches.target (pattern)"),
                )
            }),

            // `EZipperBody` reaches this arm deliberately. It is runtime cursor
            // state, not surface pattern syntax; equality handles the
            // non-connective case before this matcher is entered.
            _ => None,
        }
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - matchSpatialMatcherInstance
impl SpatialMatcher<Match, Match> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Match, pattern: Match) -> Option<()> {
        // println!("\nHit Match, Match");

        let result = self
            .spatial_match(
                target.target.expect("Match.target (target)"),
                pattern.target.expect("Match.target (pattern)"),
            )
            .and_then(|_| self.fold_match(&target.cases, &pattern.cases, None));

        result.map(|_| ())
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - unfSpatialMatcherInstance
//
// ## "Never reached" — now a checked claim rather than a comment
//
// `list_match::match_function` consults `spatial_match` only when the pattern
// reports `connective_used`, and `HasLocallyFree<GUnforgeable> for
// SpatialMatcherContext` is a constant `false` (`has_locally_free.rs`). Every
// unforgeable pattern therefore takes the `guard(t == p)` fast path and this
// impl is unreachable from the `Par` walk. `no_unforgeable_pattern_reports_
// connective_used` in `rholang/tests/spatial_matcher_disposition.rs` pins that
// premise, so if `connective_used` ever stops being constant, the suite says so
// rather than this comment quietly becoming false.
//
// ## Why the body no longer enumerates pairs
//
// It used to list two of the four `UnfInstance` arms — `GPrivateBody` and
// `GDeployerIdBody` — and drop `GDeployIdBody` and `GSysAuthTokenBody` into a
// `_ => None`, so a deploy id would not have matched ITSELF. That was undeclared
// (`matcher/exports.rs` re-exported the same two, which is why a sweep over the
// module found nothing missing). An unforgeable name is an opaque byte string:
// no sub-`Par`, no binder, nothing to descend into, so the verdict for every
// variant is the same — EQUALITY. The match below states that once per variant
// and has **no `_` arm**, so a fifth `UnfInstance` added to `RhoTypes.proto`
// fails to compile here instead of being silently assigned "matches nothing".
impl SpatialMatcher<GUnforgeable, GUnforgeable> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: GUnforgeable, pattern: GUnforgeable) -> Option<()> {
        // println!("\nHit GUnforgeable, GUnforgeable");

        match &pattern.unf_instance {
            Some(GPrivateBody(_))
            | Some(GDeployIdBody(_))
            | Some(GDeployerIdBody(_))
            | Some(GSysAuthTokenBody(_)) => guard(target == pattern),

            // `unf_instance` is a required oneof, so an absent payload is a
            // malformed term rather than a wildcard, and it matches nothing —
            // including another absent payload. That is what the old catch-all
            // did for this case, preserved deliberately.
            None => None,
        }
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - receiveBindSpatialMatcherInstance
impl SpatialMatcher<ReceiveBind, ReceiveBind> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: ReceiveBind, pattern: ReceiveBind) -> Option<()> {
        // println!("\nHit ReceiveBind, ReceiveBind");
        guard(target.patterns == pattern.patterns).and_then(|_| {
            self.spatial_match(
                target.source.expect("ReceiveBind.source (target)"),
                pattern.source.expect("ReceiveBind.source (pattern)"),
            )
        })
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - matchCaseSpatialMatcherInstance
impl SpatialMatcher<MatchCase, MatchCase> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: MatchCase, pattern: MatchCase) -> Option<()> {
        // println!("\nHit MatchCase, MatchCase");
        guard(target.pattern == pattern.pattern).and_then(|_| {
            self.spatial_match(
                target.source.expect("MatchCase.source (target)"),
                pattern.source.expect("MatchCase.source (pattern)"),
            )
        })
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
