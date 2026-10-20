use std::marker::{Send, Sync};

use models::rhoapi::expr::ExprInstance;
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{new_elist_expr, FreeMap};
use rho_pure_eval::{Env as PureEnv, EvalError, SpatialMatch};
// Changed by compile blocker C4 (RT-01) of the cost-accounted-rho merge:
// the `prepare_commit` override below is commented out, so these two names
// have no use.
// use rspace_plus_plus::rspace::errors::RSpaceError;
// use rspace_plus_plus::rspace::r#match::{Match, PreparedCommit};
use rspace_plus_plus::rspace::r#match::Match;

// Changed by D-D1a (D-M2, DR-103): whole-tree backing is no longer charged here.
// use shared::rust::collection_backing::tree_backing;
use super::exports::*;
use super::fold_match::FoldMatch;
use super::spatial_matcher::SpatialMatcherContext;

#[derive(Clone, Default)]
pub struct Matcher;

impl Matcher {
    fn get_with_context(
        pattern: &BindPattern,
        data: &ListParWithRandom,
        spatial_matcher: &mut SpatialMatcherContext,
    ) -> Option<ListParWithRandom> {
        if data.cost_stack.is_some() {
            return None;
        }
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // spatial_matcher.reserve_clone(&pattern.remainder)?;
        let caught_rem =
            spatial_matcher.fold_match(&data.pars, &pattern.patterns, pattern.remainder.clone())?;
        let mut free_map = std::mem::take(&mut spatial_matcher.free_map);
        if let Some(Var {
            var_instance: Some(FreeVar(level)),
        }) = &pattern.remainder
        {
            let mut remainder = vector_par(Vec::new(), false);
            remainder
                .exprs
                .push(new_elist_expr(caught_rem, Vec::new(), false, None));
            // Changed by D-D1a (D-M2, DR-103): one insert charges its search, its
            // moves and the growth of the tree, not a whole-map walk and the
            // whole-tree backing.
            // spatial_matcher.reserve_inspect(&free_map)?;
            // let Some(entries) = free_map.len().checked_add(1) else {
            //     return spatial_matcher.reject(RSpaceError::HostWorkRejected);
            // };
            // let Some((operations, bytes)) = tree_backing::<i32, Par>(entries) else {
            //     return spatial_matcher.reject(RSpaceError::HostWorkRejected);
            // };
            // spatial_matcher.reserve(operations, bytes, bytes)?;
            free_map.insert(*level, remainder);
        }
        // Changed by D-M4 (DR-91): the owned free map is consumed in key order;
        // each level skips the keys below it and moves its binding out.
        // let mut bound_pars = Vec::new();
        // spatial_matcher.reserve_vec(&mut bound_pars, pattern.free_count.max(0) as usize)?;
        // for level in 0..pattern.free_count {
        //     spatial_matcher.reserve_inspect(&free_map)?;
        //     if let Some(par) = free_map.get(&level) {
        //         spatial_matcher.reserve_clone(par)?;
        //         bound_pars.push(par.clone());
        //     } else {
        //         bound_pars.push(Par::default());
        //     }
        // }
        let bound_pars = Self::extract_bound_pars(free_map, pattern.free_count);
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // spatial_matcher.reserve_clone(&data.random_state)?;
        // spatial_matcher.reserve_clone(&data.cost_authority)?;
        Some(ListParWithRandom {
            pars: bound_pars,
            random_state: data.random_state.clone(),
            cost_authority: data.cost_authority.clone(),
            cost_stack: None,
        })
    }

    /// D-M4 (DR-91): the bound values of levels `0..free_count`, moved out of
    /// the owned free map in key order. Each level skips the keys below it and
    /// takes the key equal to it, if any. A missing level binds the default
    /// `Par`, as the lookup did. The moved bindings are not copied, and the
    /// bindings that are skipped or left over are dropped with the map, as
    /// before (`FreeMapExtraction.extraction_by_move_equals_lookup`).
    fn extract_bound_pars(free_map: FreeMap, free_count: i32) -> Vec<Par> {
        let mut bound_pars = Vec::with_capacity(free_count.max(0) as usize);
        let mut bindings = free_map.into_iter().peekable();
        for level in 0..free_count {
            while bindings.peek().is_some_and(|(key, _)| *key < level) {
                bindings.next();
            }
            match bindings.next_if(|(key, _)| *key == level) {
                Some((_, par)) => bound_pars.push(par),
                None => bound_pars.push(Par::default()),
            }
        }
        bound_pars
    }
}

// Matcher must implement Send + Sync to satisfy Match trait bounds
unsafe impl Send for Matcher {}
unsafe impl Sync for Matcher {}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/storage/package.scala - matchListPar
//
// `get` borrows the pattern and the
// candidate datum — the space no longer clones either per match attempt.
// The interior pair walk (`fold_match` over borrowed slices) compares
// non-binding pairs by reference and clones only binding/connective pairs
// into the owned spatial lattice; the returned `ListParWithRandom` owns
// exactly the BOUND subterms (free_map values) plus a copy of the datum's
// `random_state`.
impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for Matcher {
    fn get(&self, pattern: &BindPattern, data: &ListParWithRandom) -> Option<ListParWithRandom> {
        let mut spatial_matcher = SpatialMatcherContext::new();
        Self::get_with_context(pattern, data, &mut spatial_matcher)
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

    /// Added by DR-129 (E3-a): `check_commit` vetoes only through a guard
    /// that is present and not empty, so exactly those continuations may
    /// veto.
    /// User decision 6 (2026-10-16) removed the Stored-check skip that read
    /// this hook (complete guard search, defect D1), so no production code
    /// calls it. It stays exact, so that the trait matches upstream.
    fn commit_may_veto(&self, k: &TaggedContinuation) -> bool {
        k.guard.as_ref().is_some_and(|guard| !is_empty_par(guard))
    }

    // Changed by D-M6 (DR-88): the matched data are borrowed.
    // matched: &[ListParWithRandom],
    /// Changed by DR-131 S6: the commit check of the native replay over the
    /// matched data by reference (D-I). It decides as `check_commit` does,
    /// without a copy of the data.
    fn check_commit_by_reference(
        &self,
        k: &TaggedContinuation,
        matched: &[&ListParWithRandom],
    ) -> bool {
        let Some(guard) = k.guard.as_ref() else {
            return true;
        };
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // clone_backing::inspect(guard, &backing)?;
        if is_empty_par(guard) {
            true
        } else {
            guard_passes_by_reference(guard, matched)
        }
    }

    // Disabled by compile blocker C4 (RT-01) of the cost-accounted-rho merge:
    // the trait method returns `Option<PreparedCommit>`, so this override,
    // which returns a `Result`, does not compile. Its body equals the trait
    // default (`check_commit(..).then(PreparedCommit::default)`), so the
    // default decides the same way.
    // fn prepare_commit(
    //     &self,
    //     k: &TaggedContinuation,
    //     matched: &[&ListParWithRandom],
    // ) -> Result<Option<PreparedCommit>, RSpaceError> {
    //     Ok(self.check_commit(k, matched).then(PreparedCommit::default))
    // }
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

/// What actually happened when a guard was put to the decider.
///
/// ★ **Why this is not a `bool`.** A guard verdict used to be one, and a
/// single bit cannot hold the distinction the caller most needs: whether the
/// guard was *decided against* or *never decided at all*. Every failure mode
/// collapsed into `false`, so a guard containing a construct the decider has
/// no arm for reported exactly what a refuted guard reports — and the program
/// then compiled, ran, exited 0, admitted nothing, and said nothing.
///
/// The same conflation was diagnosed and removed on the host lane, where the
/// remedy was a three-valued `GuardDisposition::{Fires, Blocks, Declines}`.
/// This is that remedy on the machine lane, with the two *decided-but-failed*
/// cases separated as well, so each variant is one fact:
///
/// | variant | evaluated? | what it means |
/// | --- | --- | --- |
/// | [`Admits`](Self::Admits) | yes | the predicate holds — commit |
/// | [`Refutes`](Self::Refutes) | yes | the predicate does not hold — an ordinary, correct non-commit |
/// | [`NotABoolean`](Self::NotABoolean) | yes | the guard reduced to something that is not a verdict |
/// | [`Failed`](Self::Failed) | attempted | the guard is decidable but errored on *this* datum (type mismatch, `/0`, overflow, unbound index) |
/// | [`Undecidable`](Self::Undecidable) | **no** | ★ the decider has no arm for a node in this guard — it never ran |
///
/// [`Undecidable`](Self::Undecidable) is the one the gate in
/// [`crate::rust::interpreter::guard`] exists to make unreachable, and
/// separating it here is what lets the gate be *checked* rather than assumed.
#[derive(Clone, Debug, PartialEq)]
pub enum GuardDisposition {
    /// Evaluated to `true`.
    Admits,
    /// Evaluated to `false`.
    Refutes,
    /// Evaluated to a value that is not a single `GBool`.
    NotABoolean,
    /// Evaluation ran and failed on this datum. Carries the exact error so a
    /// diagnostic can name it.
    Failed(EvalError),
    /// ★ Evaluation never ran: the guard contains a node kind
    /// `rho-pure-eval` has no arm for. `kind` is the `ExprInstance` variant
    /// name, e.g. `"EMethodBody"`.
    Undecidable { kind: &'static str },
}

impl GuardDisposition {
    /// The commit verdict: `true` only for [`Admits`](Self::Admits).
    ///
    /// ⚠ **Fail-closed, and the asymmetry is deliberate.** Firing a COMM the
    /// reducer would not fire is unsoundness in the worst direction, so every
    /// non-`Admits` disposition declines. What has changed is that declining
    /// is no longer the *only* thing that happens: the caller has the
    /// disposition itself and can refuse, log, or raise on it.
    pub fn commits(&self) -> bool { matches!(self, GuardDisposition::Admits) }
}

/// Decides a guard against the combined cross-bind variables.
///
/// `bound_pars` is the concatenation, in receive-bind order, of every bind's
/// matched data — which *is* the de Bruijn numbering the normalizer assigned,
/// so `BoundVar(k)` reads `bound_pars[bound_pars.len() - 1 - k]`.
///
/// The environment is built by `put`ting each bound par in order, so this
/// function is a pure function of `(condition, bound_pars)`: no state carries
/// between calls, which is what the determinism contract in
/// [`rho_pure_eval`]'s module docs requires of anything the matcher calls.
pub fn guard_disposition(condition: &Par, bound_pars: &[Par]) -> GuardDisposition {
    let mut env: PureEnv<Par> = PureEnv::new();
    for p in bound_pars.iter() {
        if env.push(p.clone()).is_none() {
            return GuardDisposition::Failed(EvalError::ArithmeticOverflow {
                op: "guard environment",
            });
        }
    }
    guard_disposition_in_env(condition, &env)
}

/// [`guard_disposition`] against an environment the caller has already built.
///
/// `Reduce::eval_match` needs this: a case guard is evaluated in `case_env`,
/// which is the ENCLOSING environment extended with the case's bindings, not
/// the bindings alone. Building a fresh environment from the bindings would
/// silently drop every outer variable the guard mentions and turn it into an
/// `UnboundVariable` failure.
///
/// Both entry points funnel into the same classification, so `for … where`
/// and `match … where` cannot disagree about what a disposition means.
pub fn guard_disposition_in_env(condition: &Par, env: &PureEnv<Par>) -> GuardDisposition {
    match rho_pure_eval::eval_with(condition, env, &SpatialMatcherOracle) {
        Ok(result) => match extract_bool(&result) {
            Some(true) => GuardDisposition::Admits,
            Some(false) => GuardDisposition::Refutes,
            None => {
                tracing::error!(
                    target: "f1r3fly.rholang.guard",
                    result = ?result,
                    "guard did not reduce to a boolean; declining to commit"
                );
                GuardDisposition::NotABoolean
            }
        },
        // ★ THE ARM THE WHOLE FIX TURNS ON. `UnsupportedExpression` is not a
        // verdict about the datum — it is the decider reporting that it never
        // looked. It gets its own disposition and an ERROR-level event naming
        // the node kind, because reaching it means a guard slipped past both
        // layers of `interpreter::guard`'s gate and that is a defect in the
        // node, not in the program.
        Err(EvalError::UnsupportedExpression { kind }) => {
            tracing::error!(
                target: "f1r3fly.rholang.guard",
                kind,
                condition = ?condition,
                "UNDECIDABLE GUARD reached the matcher: `interpreter::guard` \
                 should have refused this at normalize time or in eval_receive. \
                 Declining to commit."
            );
            GuardDisposition::Undecidable { kind }
        }
        Err(error) => {
            tracing::error!(
                target: "f1r3fly.rholang.guard",
                error = %error,
                "guard evaluation failed on this datum; declining to commit"
            );
            GuardDisposition::Failed(error)
        }
    }
}

/// [`guard_disposition`], projected onto the boolean `check_commit` returns.
///
/// ★ **Why the projection stays here rather than becoming fallible.**
/// `Match::check_commit` returns `bool` (`rspace++/src/rspace/match.rs`), and
/// widening it to a `Result` would thread a new failure path through
/// `consume`/`produce` in **both** the play and the replay space — the one
/// place where the comm-event sequence must be reproduced exactly — and would
/// break every out-of-workspace implementor of the trait. The undecidable case
/// is removed at its source instead (see
/// [`crate::rust::interpreter::guard`]), which leaves nothing for a fallible
/// signature to report.
fn guard_passes(condition: &Par, bound_pars: &[Par]) -> bool {
    guard_disposition(condition, bound_pars).commits()
}

/// Changed by DR-131: the guard of `check_commit_by_reference`. It evaluates
/// the guard over the borrowed matched data, as `guard_passes` does over
/// owned data, and a failed push or evaluation fails the guard.
// Changed by D-M6 (DR-88): the matched data are borrowed.
// matched: &[ListParWithRandom],
fn guard_passes_by_reference(condition: &Par, matched: &[&ListParWithRandom]) -> bool {
    let mut env: PureEnv<Par> = PureEnv::new();
    for binding in matched {
        for par in &binding.pars {
            // Changed by D-O1 (DR-109): block accounting charges inline bytes
            // once per enclosing block.
            // clone_backing::reserve_copy_and_cleanup(par, meter)?;
            if env.push(par.clone()).is_none() {
                return false;
            }
        }
    }
    // Changed by the cost-accounted-rho merge: the guard decides through
    // `guard_disposition_in_env`, with the spatial-match oracle, as
    // `check_commit` does. So play and native replay decide each guard the
    // same way, also a guard that matches a pattern.
    // match rho_pure_eval::eval(condition, &env) {
    //     Ok(result) => extract_bool(&result) == Some(true),
    //     Err(_) => false,
    // }
    guard_disposition_in_env(condition, &env).commits()
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
        || !par.cost_signed_terms.is_empty()
        || !par.cost_stacks.is_empty()
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

#[cfg(test)]
mod commit_tests {
    use models::rhoapi::tagged_continuation::TaggedCont;
    use models::rhoapi::ParWithRandom;

    use super::*;

    /// Changed by DR-131: the commit check no longer charges host work. The
    /// test keeps its check that the commit check returns the value of the
    /// guard.
    #[test]
    fn commit_guard_preserves_boolean_result() {
        let matcher = Matcher;
        let continuation = TaggedContinuation {
            guard: Some(models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::GBool(true)),
                }],
                ..Par::default()
            }),
            ..TaggedContinuation::default()
        };
        assert!(matcher.check_commit_by_reference(&continuation, &[]));
        let false_guard = TaggedContinuation {
            guard: Some(models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::GBool(false)),
                }],
                ..Par::default()
            }),
            ..TaggedContinuation::default()
        };
        assert!(!matcher.check_commit_by_reference(&false_guard, &[]));
    }

    fn bool_par(value: bool) -> Par {
        models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GBool(value)),
            }],
            ..Par::default()
        }
    }

    fn bound_var_par(index: i32) -> Par {
        models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(BoundVar(index)),
                    }),
                })),
            }],
            ..Par::default()
        }
    }

    fn body_par(size: usize) -> Par {
        models::par_from_default! {
            exprs: (0..size)
                .map(|index| Expr {
                    expr_instance: Some(ExprInstance::GInt(
                        i64::try_from(index).expect("index fits in i64"),
                    )),
                })
                .collect(),
            ..Par::default()
        }
    }

    fn continuation_with(guard: Option<Par>, body_size: usize) -> TaggedContinuation {
        TaggedContinuation {
            guard,
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(body_par(body_size)),
                random_state: vec![7; 32],
            })),
            ..TaggedContinuation::default()
        }
    }

    fn binding(pars: Vec<Par>) -> ListParWithRandom {
        ListParWithRandom {
            pars,
            ..ListParWithRandom::default()
        }
    }

    /// D-M6 (DR-88): the commit check over borrowed matched data decides
    /// exactly like the check over owned data.
    #[test]
    fn commit_guard_by_reference_matches_owned_guard() {
        let cases = vec![
            (None, Vec::new()),
            (Some(Par::default()), vec![binding(vec![bool_par(false)])]),
            (Some(bool_par(true)), vec![binding(vec![bool_par(false)])]),
            (Some(bool_par(false)), vec![binding(vec![bool_par(true)])]),
            (Some(bound_var_par(0)), vec![binding(vec![bool_par(true)])]),
            (Some(bound_var_par(0)), vec![binding(vec![bool_par(false)])]),
            (Some(bound_var_par(0)), vec![
                binding(vec![bool_par(true)]),
                binding(vec![bool_par(false)]),
            ]),
            (Some(bound_var_par(1)), vec![
                binding(vec![bool_par(true)]),
                binding(vec![bool_par(false)]),
            ]),
            (Some(bound_var_par(1)), vec![binding(vec![
                bool_par(false),
                bool_par(true),
            ])]),
            (Some(bound_var_par(3)), vec![binding(vec![bool_par(true)])]),
            (Some(body_par(2)), vec![binding(vec![bool_par(true)])]),
        ];
        for (guard, owned) in cases {
            let continuation = continuation_with(guard.clone(), 3);
            let borrowed: Vec<&ListParWithRandom> = owned.iter().collect();
            assert_eq!(
                Matcher.check_commit_by_reference(&continuation, &borrowed),
                Matcher.check_commit(&continuation, &borrowed),
                "guard {guard:?}, matched {owned:?}"
            );
        }
    }

    fn legacy_extraction(free_map: &FreeMap, free_count: i32) -> Vec<Par> {
        (0..free_count)
            .map(|level| free_map.get(&level).cloned().unwrap_or_default())
            .collect()
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

        /// D-M4 (DR-91): moving the bindings out of the owned free map in
        /// key order gives exactly the legacy per-level lookups, for keys
        /// below the first level, between levels and above the last level.
        #[test]
        fn bound_pars_by_move_equal_lookup_extraction(
            entries in proptest::collection::btree_map(-3_i32..9, 0_usize..4, 0..8),
            free_count in 0_i32..7,
        ) {
            let free_map: FreeMap = entries
                .into_iter()
                .map(|(key, size)| (key, body_par(size + 1)))
                .collect();
            let expected = legacy_extraction(&free_map, free_count);
            let moved = Matcher::extract_bound_pars(free_map, free_count);
            proptest::prop_assert_eq!(moved, expected);
        }
    }

    /// D-M4 (DR-91): the extraction allocates only the result vector; the
    /// bound values are moved, not copied.
    #[test]
    fn extraction_allocates_no_binding_copy() {
        use crate::rust::interpreter::accounting::measured_allocations as measured;
        let free_map: FreeMap = [0, 1, 2]
            .into_iter()
            .map(|key| (key, body_par(256)))
            .collect();
        let (pars, allocated) = measured(|| Matcher::extract_bound_pars(free_map, 3));
        assert_eq!(pars.len(), 3);
        let vector = pars.capacity() * std::mem::size_of::<Par>();
        assert!(
            allocated <= vector,
            "allocated {allocated}, result vector {vector}"
        );
        // A copy of a single binding would allocate its 256 expressions.
        assert!(vector < 256 * std::mem::size_of::<Expr>());
    }

    /// Added by DR-129 (E3-a, risk R2): `commit_may_veto` is false only for a
    /// continuation whose commit cannot veto. A continuation with no guard or
    /// an empty guard never vetoes. Every other guard may, even a guard that
    /// is the literal `true`.
    /// Changed by user decision 6 (2026-10-16): the Stored check of replay no
    /// longer skips such a continuation (complete guard search, defect D1).
    /// The hook stays exact, so that the trait matches upstream.
    #[test]
    fn commit_may_veto_marks_every_continuation_whose_commit_can_veto() {
        let matcher = Matcher;
        let samples = [Vec::new(), vec![binding(vec![bool_par(true)])], vec![
            binding(vec![bool_par(false)]),
        ]];
        let cases = [
            (None, false),
            (Some(Par::default()), false),
            (Some(bool_par(true)), true),
            (Some(bool_par(false)), true),
            (Some(bound_var_par(0)), true),
        ];
        let mut vetoed = 0;
        for (guard, may_veto) in cases {
            let continuation = continuation_with(guard.clone(), 2);
            assert_eq!(
                matcher.commit_may_veto(&continuation),
                may_veto,
                "{guard:?}"
            );
            for matched in &samples {
                let committed =
                    matcher.check_commit(&continuation, &matched.iter().collect::<Vec<_>>());
                if !committed {
                    vetoed += 1;
                    assert!(may_veto, "{guard:?} vetoed {matched:?}");
                }
            }
        }
        assert!(vetoed >= 2, "the fixture must exercise a veto");
    }
}
