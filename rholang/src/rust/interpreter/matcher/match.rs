use std::marker::{Send, Sync};

use models::rhoapi::expr::ExprInstance;
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{new_elist_expr, to_vec};
use rho_pure_eval::{Env as PureEnv, SpatialMatch};
use rspace_plus_plus::rspace::r#match::Match;

use super::exports::*;
use super::fold_match::FoldMatch;
use super::spatial_matcher::SpatialMatcherContext;

#[derive(Clone, Default)]
pub struct Matcher;

// Matcher must implement Send + Sync to satisfy Match trait bounds
unsafe impl Send for Matcher {}
unsafe impl Sync for Matcher {}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/storage/package.scala - matchListPar
//
// EPathMap fix P4.2 (amendment PM-3): `get` BORROWS the pattern and the
// candidate datum — the space no longer clones either per match attempt.
// The interior pair walk (`fold_match` over borrowed slices) compares
// non-binding pairs by reference and clones only binding/connective pairs
// into the owned spatial lattice; the returned `ListParWithRandom` owns
// exactly the BOUND subterms (free_map values) plus a copy of the datum's
// `random_state`.
impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for Matcher {
    fn get(&self, pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom> {
        let mut spatial_matcher = SpatialMatcherContext::new();

        let fold_match_result =
            spatial_matcher.fold_match(&data.pars, &pattern.patterns, pattern.remainder.clone());
        let match_result = match fold_match_result {
            Some(pars) => Some((spatial_matcher.free_map, pars)),
            None => None,
        };

        match match_result {
            Some((mut free_map, caught_rem)) => {
                let remainder_map = match pattern.remainder {
                    Some(Var {
                        var_instance: Some(FreeVar(level)),
                    }) => {
                        free_map.insert(
                            level,
                            vector_par(Vec::new(), false).with_exprs(vec![new_elist_expr(
                                caught_rem,
                                Vec::new(),
                                false,
                                None,
                            )]),
                        );
                        free_map
                    }
                    _ => free_map,
                };

                let bound_pars = to_vec(remainder_map, pattern.free_count);

                Some(ListParWithRandom {
                    pars: bound_pars,
                    random_state: data.random_state.clone(),
                })
            }
            None => None,
        }
    }

    /// Cross-channel `where`-clause guard. Called by the matcher
    /// coordinator after every spatial bind has produced a
    /// `ListParWithRandom`. The bound variables of every bind are
    /// concatenated in receive-bind order (matching the De Bruijn
    /// indices the parser assigned), then the guard expression is
    /// evaluated via rho-pure-eval. Returns true iff it reduces to
    /// `GBool(true)`. Anything else (false, non-bool, error) means
    /// guard-fail and the consume stays uncommitted. See plan §7.12.
    fn check_commit(&self, k: &TaggedContinuation, matched: &[&ListParWithRandom]) -> bool {
        let Some(guard) = k.guard.as_ref() else {
            return true;
        };
        if is_empty_par(guard) {
            return true;
        }
        let mut combined: Vec<Par> = Vec::new();
        for m in matched {
            combined.extend_from_slice(&m.pars);
        }
        guard_passes(guard, &combined)
    }
}

fn is_empty_par(par: &Par) -> bool { par == &Par::default() }

/// The production spatial-match oracle for guard evaluation (M-1a).
///
/// `rho-pure-eval` needs an answer to "does this target match this
/// pattern?" for `EMatchesBody`, but it cannot ask the matcher directly:
/// `rholang` depends on `rho-pure-eval`, not the other way round. So the
/// matcher is injected here, at the two places a guard is evaluated —
/// [`guard_passes`] below (the `for … where` cross-bind guard) and
/// `Reduce::eval_match` (the `match … where` case guard). Both use THIS
/// type, so the two guard languages decide spatial tests identically.
///
/// ★ Determinism. `SpatialMatcherContext` accumulates bindings in its
/// own `free_map` as it matches, so a shared context would make the
/// verdict depend on call history. A FRESH context is therefore built
/// per question and dropped before returning: no state survives a call,
/// the answer is a pure function of `(target, pattern)`, and the
/// determinism guarantee `rho-pure-eval` publishes ("same input Par +
/// same Env produce the same output Par byte-for-byte on every node")
/// extends unbroken across the seam. This is the same
/// construct-use-discard discipline `Matcher::get` and
/// `Reduce::combine_matches` already follow.
///
/// The bindings the match produces are deliberately dropped. A guard
/// runs after every bind is already resolved and may only answer
/// commit / do-not-commit, so `is_some()` is the whole answer.
pub struct SpatialMatcherOracle;

impl SpatialMatch for SpatialMatcherOracle {
    fn matches(&self, target: &Par, pattern: &Par) -> bool {
        let mut spatial_matcher = SpatialMatcherContext::new();
        spatial_matcher
            .spatial_match_result(target.clone(), pattern.clone())
            .is_some()
    }
}

/// Evaluates a guard against the combined cross-bind variables.
/// Returns true iff the guard reduces to GBool(true). Anything else
/// (false, non-bool, or eval-error) is treated as guard-fail.
///
/// ★ The collapse of "false", "not a boolean" and "evaluation failed"
/// into one guard-fail verdict is deliberate and consensus-visible:
/// changing it would change which COMMs commit. It is preserved
/// verbatim. Diagnosability is paid instead at a non-consensus layer —
/// the DEBUG events below name which of the two silent cases occurred
/// without touching the returned verdict.
fn guard_passes(condition: &Par, bound_pars: &[Par]) -> bool {
    let mut env: PureEnv<Par> = PureEnv::new();
    for p in bound_pars.iter() {
        env = env.put(p.clone());
    }
    match rho_pure_eval::eval_with(condition, &env, &SpatialMatcherOracle) {
        Ok(result) => match extract_bool(&result) {
            Some(verdict) => verdict,
            None => {
                tracing::debug!(
                    target: "f1r3fly.rholang.guard",
                    result = ?result,
                    "guard did not reduce to a boolean; treating as guard-fail"
                );
                false
            }
        },
        Err(error) => {
            tracing::debug!(
                target: "f1r3fly.rholang.guard",
                error = %error,
                "guard evaluation failed; treating as guard-fail"
            );
            false
        }
    }
}

fn extract_bool(par: &Par) -> Option<bool> {
    if !par.sends.is_empty()
        || !par.receives.is_empty()
        || !par.news.is_empty()
        || !par.matches.is_empty()
        || !par.bundles.is_empty()
        || !par.unforgeables.is_empty()
        || !par.connectives.is_empty()
        || !par.conditionals.is_empty()
        || par.exprs.len() != 1
    {
        return None;
    }
    match par.exprs[0].expr_instance.as_ref()? {
        ExprInstance::GBool(b) => Some(*b),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    //! M-1a: the guard-side of the `SpatialMatch` seam.
    //!
    //! Before the seam, `rho_pure_eval::eval` refused `EMatchesBody`
    //! outright, so EVERY spatial guard failed shut regardless of whether
    //! it was true — the `Err(_) => false` collapse in [`guard_passes`]
    //! swallowed the `UnsupportedExpression`. These tests pin the flip
    //! (a true spatial guard now passes) and pin the two ways a guard
    //! must still fail shut (a genuine mismatch, and an eval error).

    use models::rust::utils::{new_gint_par, new_gstring_par, new_wildcard_par};

    use super::*;

    /// `BoundVar(0) matches <pattern>` — the shape a `for (x <- c) where
    /// (x matches P)` guard lowers to. `BoundVar(0)` is the first bound
    /// variable, which `guard_passes` puts at de Bruijn index 0.
    fn guard_first_bound_matches(pattern: Par) -> Par {
        ematches_guard(
            vector_par(models::create_bit_vector(&[0]), false).with_exprs(vec![Expr {
                expr_instance: Some(EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(BoundVar(0)),
                    }),
                })),
            }]),
            pattern,
        )
    }

    fn ematches_guard(target: Par, pattern: Par) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(EMatchesBody(EMatches {
                target: Some(target),
                pattern: Some(pattern),
            })),
        }])
    }

    /// The `Int` type pattern: a connective, so the matcher takes its
    /// connective path rather than structural equality.
    fn int_pattern() -> Par {
        vector_par(Vec::new(), true).with_connectives(vec![Connective {
            connective_instance: Some(ConnInt(true)),
        }])
    }

    #[test]
    fn spatial_guard_passes_when_the_bound_value_matches_a_wildcard() {
        let guard = guard_first_bound_matches(new_wildcard_par(Vec::new(), true));
        assert!(guard_passes(&guard, &[new_gint_par(5, Vec::new(), false)]));
    }

    #[test]
    fn spatial_guard_passes_when_the_bound_value_matches_a_type_pattern() {
        let guard = guard_first_bound_matches(int_pattern());
        assert!(guard_passes(&guard, &[new_gint_par(5, Vec::new(), false)]));
    }

    #[test]
    fn spatial_guard_fails_shut_on_a_genuine_mismatch() {
        // A String bound where the pattern demands an Int: the matcher
        // says no, so the guard says no.
        let guard = guard_first_bound_matches(int_pattern());
        assert!(!guard_passes(&guard, &[new_gstring_par(
            "five".to_string(),
            Vec::new(),
            false
        )]));
    }

    #[test]
    fn spatial_guard_fails_shut_on_a_ground_mismatch() {
        let guard = guard_first_bound_matches(new_gint_par(7, Vec::new(), false));
        assert!(!guard_passes(&guard, &[new_gint_par(5, Vec::new(), false)]));
        assert!(guard_passes(&guard, &[new_gint_par(7, Vec::new(), false)]));
    }

    #[test]
    fn spatial_guard_fails_shut_on_an_eval_error() {
        // BoundVar(3) with a single bound par is unbound → EvalError →
        // guard-fail. The DEBUG event names it; the verdict is unchanged.
        let unbound_target =
            vector_par(models::create_bit_vector(&[3]), false).with_exprs(vec![Expr {
                expr_instance: Some(EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(BoundVar(3)),
                    }),
                })),
            }]);
        let guard = ematches_guard(unbound_target, new_wildcard_par(Vec::new(), true));
        assert!(!guard_passes(&guard, &[new_gint_par(5, Vec::new(), false)]));
    }

    #[test]
    fn spatial_guard_fails_shut_when_it_does_not_reduce_to_a_boolean() {
        // A non-boolean guard is guard-fail, not an error and not a pass.
        // This is the branch the new DEBUG event covers.
        assert!(!guard_passes(&new_gint_par(1, Vec::new(), false), &[
            new_gint_par(5, Vec::new(), false)
        ]));
    }

    #[test]
    fn the_seam_is_what_flipped_the_spatial_guard() {
        // Control: the same guard under the default `NoSpatialMatch`
        // oracle still yields `UnsupportedExpression`, which the
        // `Err(_) => false` collapse would turn into guard-fail. So the
        // pass above is attributable to the injected oracle and nothing
        // else — and the `eval` entry point is unchanged for every other
        // caller.
        let guard = guard_first_bound_matches(new_wildcard_par(Vec::new(), true));
        let mut env: PureEnv<Par> = PureEnv::new();
        let env = env.put(new_gint_par(5, Vec::new(), false));
        assert_eq!(
            rho_pure_eval::eval(&guard, &env),
            Err(rho_pure_eval::EvalError::UnsupportedExpression {
                kind: "EMatchesBody"
            })
        );
    }

    #[test]
    fn the_oracle_is_deterministic_across_calls() {
        // A fresh SpatialMatcherContext per question ⇒ no state carries
        // over. Ask a binding-heavy question (a free variable in the
        // pattern, which the matcher records in its free_map) repeatedly
        // and interleave a mismatch, then re-ask: every answer must be
        // the one the pair alone determines.
        let oracle = SpatialMatcherOracle;
        let five = new_gint_par(5, Vec::new(), false);
        let text = new_gstring_par("five".to_string(), Vec::new(), false);
        let binder = models::rust::utils::new_freevar_par(0, Vec::new());

        for _ in 0..8 {
            assert!(oracle.matches(&five, &binder));
            assert!(oracle.matches(&text, &binder));
            assert!(oracle.matches(&five, &int_pattern()));
            assert!(!oracle.matches(&text, &int_pattern()));
        }
    }

    #[test]
    fn cross_bind_spatial_guard_sees_every_bound_variable() {
        // Two binds. `guard_passes` pushes the bound pars onto the env in
        // receive-bind order, and de Bruijn indices count back from the
        // most recent push — so for `for (@x <- a; @y <- b)` it is
        // BoundVar(1) that reads `x` (bind 0) and BoundVar(0) that reads
        // `y` (bind 1). This test therefore pins the index assignment as
        // well as the spatial verdict: a guard on BoundVar(1) must track
        // the FIRST bound par as the pair is swapped.
        let first_bound =
            vector_par(models::create_bit_vector(&[1]), false).with_exprs(vec![Expr {
                expr_instance: Some(EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(BoundVar(1)),
                    }),
                })),
            }]);
        let guard = ematches_guard(first_bound, int_pattern());
        assert!(guard_passes(&guard, &[
            new_gint_par(9, Vec::new(), false),
            new_gstring_par("a".to_string(), Vec::new(), false),
        ]));
        assert!(!guard_passes(&guard, &[
            new_gstring_par("a".to_string(), Vec::new(), false),
            new_gint_par(9, Vec::new(), false),
        ]));
    }
}
