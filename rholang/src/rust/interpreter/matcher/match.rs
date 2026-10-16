use std::marker::{Send, Sync};

use models::rhoapi::expr::ExprInstance;
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{new_elist_expr, FreeMap};
use rho_pure_eval::{Env as PureEnv, EvalError, SpatialMatch};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;
use rspace_plus_plus::rspace::r#match::{Match, PreparedCommit};
use shared::rust::clone_backing::{self, BackingError, BackingMeter};

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
        spatial_matcher: &mut SpatialMatcherContext<'_>,
    ) -> Option<ListParWithRandom> {
        if data.cost_stack.is_some() {
            return None;
        }
        spatial_matcher.reserve_blocks_copy_and_cleanup(&pattern.remainder)?;
        let caught_rem =
            spatial_matcher.fold_match(&data.pars, &pattern.patterns, pattern.remainder.clone())?;
        let mut free_map = std::mem::take(&mut spatial_matcher.free_map);
        if let Some(Var {
            var_instance: Some(FreeVar(level)),
        }) = &pattern.remainder
        {
            let mut remainder = vector_par(Vec::new(), false);
            spatial_matcher.reserve_vec(&mut remainder.exprs, 1)?;
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
            spatial_matcher.reserve_free_map_insert(free_map.len())?;
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
        let bound_pars = Self::extract_bound_pars(free_map, pattern.free_count, spatial_matcher)?;
        spatial_matcher.reserve_blocks_copy_and_cleanup(&data.random_state)?;
        spatial_matcher.reserve_blocks_copy_and_cleanup(&data.cost_authority)?;
        Some(ListParWithRandom {
            pars: bound_pars,
            random_state: data.random_state.clone(),
            cost_authority: data.cost_authority.clone(),
            cost_stack: None,
        })
    }

    /// D-M4 (DR-91): the bound values of levels `0..free_count`, moved out of
    /// the owned free map in key order. Each level skips the keys below it
    /// (one operation each) and takes the key equal to it, if any (one
    /// operation and the bytes of the moved slot); a missing level binds the
    /// default `Par`, as the lookup did. The moved bindings are not copied, and
    /// the bindings that are skipped or left over are dropped with the map, as
    /// before (`FreeMapExtraction.extraction_by_move_equals_lookup`).
    fn extract_bound_pars(
        free_map: FreeMap,
        free_count: i32,
        spatial_matcher: &mut SpatialMatcherContext<'_>,
    ) -> Option<Vec<Par>> {
        let mut bound_pars = Vec::new();
        spatial_matcher.reserve_vec(&mut bound_pars, free_count.max(0) as usize)?;
        let slot = std::mem::size_of::<(i32, Par)>()
            .checked_mul(2)
            .expect("slot bytes fit in usize");
        let mut bindings = free_map.into_iter().peekable();
        for level in 0..free_count {
            while bindings.peek().is_some_and(|(key, _)| *key < level) {
                spatial_matcher.reserve(1, 0, 0)?;
                bindings.next();
            }
            spatial_matcher.reserve(1, slot, 0)?;
            match bindings.next_if(|(key, _)| *key == level) {
                Some((_, par)) => bound_pars.push(par),
                None => bound_pars.push(Par::default()),
            }
        }
        Some(bound_pars)
    }

    pub fn get_metered(
        &self,
        pattern: &BindPattern,
        data: &ListParWithRandom,
        meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<Option<ListParWithRandom>, RSpaceError> {
        let mut context = SpatialMatcherContext::with_meter(meter)?;
        let matched = Self::get_with_context(pattern, data, &mut context);
        match context.take_error() {
            Some(error) => Err(error),
            None => Ok(matched),
        }
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

    fn get_metered(
        &self,
        pattern: &BindPattern,
        data: &ListParWithRandom,
        meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<Option<ListParWithRandom>, RSpaceError> {
        Matcher::get_metered(self, pattern, data, meter)
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

    // Changed by D-M6 (DR-88): the matched data are borrowed.
    // matched: &[ListParWithRandom],
    fn check_commit_metered(
        &self,
        k: &TaggedContinuation,
        matched: &[&ListParWithRandom],
        meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<bool, RSpaceError> {
        meter.reserve(1, 0, 0)?;
        let Some(guard) = k.guard.as_ref() else {
            return Ok(true);
        };
        let failure = std::cell::RefCell::new(None);
        let backing = |operations, scanned, bytes| match meter.reserve(operations, scanned, bytes) {
            Ok(()) => Ok(()),
            Err(error) => {
                *failure.borrow_mut() = Some(error);
                Err(BackingError::Rejected)
            }
        };
        let result = (|| {
            clone_backing::inspect_blocks(guard, &backing)?;
            if is_empty_par(guard) {
                Ok(true)
            } else {
                guard_passes_metered(guard, matched, &backing)
            }
        })();
        if let Some(error) = failure.into_inner() {
            return Err(error);
        }
        result.map_err(|_| RSpaceError::HostWorkRejected)
    }

    fn prepare_commit_metered(
        &self,
        k: &TaggedContinuation,
        matched: &[&ListParWithRandom],
        meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<Option<PreparedCommit>, RSpaceError> {
        self.check_commit_metered(k, matched, meter)
            .map(|accepted| accepted.then(PreparedCommit::default))
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

    fn matches_metered(
        &self,
        target: &Par,
        pattern: &Par,
        meter: &dyn BackingMeter,
    ) -> Result<bool, BackingError> {
        let bridge = |operations, scanned, backing| {
            meter
                .reserve(operations, scanned, backing)
                .map_err(|_| RSpaceError::HostWorkRejected)
        };
        let mut context =
            SpatialMatcherContext::with_meter(&bridge).map_err(|_| BackingError::Rejected)?;
        clone_backing::reserve_blocks_copy_and_cleanup(target, meter)?;
        clone_backing::reserve_blocks_copy_and_cleanup(pattern, meter)?;
        let matched = context
            .spatial_match_result(target.clone(), pattern.clone())
            .is_some();
        match context.take_error() {
            Some(_) => Err(BackingError::Rejected),
            None => Ok(matched),
        }
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
        if let Err(error) = env.push(p.clone()) {
            return GuardDisposition::Failed(EvalError::HostWork(error));
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

// Changed by D-M6 (DR-88): the matched data are borrowed.
// matched: &[ListParWithRandom],
fn guard_passes_metered(
    condition: &Par,
    matched: &[&ListParWithRandom],
    meter: &dyn BackingMeter,
) -> Result<bool, BackingError> {
    let mut env: PureEnv<Par> = PureEnv::new();
    for binding in matched {
        meter.reserve(1, 0, 0)?;
        for par in &binding.pars {
            clone_backing::reserve_blocks_copy_and_cleanup(par, meter)?;
            env.push_metered(par.clone(), meter)?;
        }
    }
    match rho_pure_eval::eval_metered(condition, &env, meter) {
        Ok(result) => {
            // One walk extracts the verdict; a second pays the owned result's
            // release after this arm.
            clone_backing::inspect_blocks(&result, meter)?;
            clone_backing::inspect_blocks(&result, meter)?;
            Ok(extract_bool(&result) == Some(true))
        }
        Err(rho_pure_eval::EvalError::HostWork(error)) => Err(error),
        Err(_) => Ok(false),
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
mod metered_tests {
    use std::cell::{Cell, RefCell};
    use std::sync::Mutex;

    use models::rhoapi::tagged_continuation::TaggedCont;
    use models::rhoapi::var::VarInstance::FreeVar;
    use models::rhoapi::ParWithRandom;

    use super::*;

    #[test]
    fn metered_commit_guard_preserves_boolean_result_and_rejects_short_credit() {
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
        let used = Mutex::new([0usize; 3]);
        let full = |operations: usize, scanned: usize, backing: usize| {
            let mut totals = used.lock().unwrap();
            for (total, amount) in totals.iter_mut().zip([operations, scanned, backing]) {
                *total += amount;
            }
            Ok(())
        };
        assert_eq!(
            matcher.check_commit_metered(&continuation, &[], &full),
            Ok(true)
        );
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
            assert_eq!(
                matcher.check_commit_metered(&continuation, &[], &meter),
                Err(RSpaceError::HostWorkRejected)
            );
        }
        let false_guard = TaggedContinuation {
            guard: Some(models::par_from_default! {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::GBool(false)),
                }],
                ..Par::default()
            }),
            ..TaggedContinuation::default()
        };
        assert_eq!(
            matcher.check_commit_metered(&false_guard, &[], &full),
            Ok(false)
        );
        assert!(matcher
            .prepare_commit_metered(&continuation, &[], &full)
            .unwrap()
            .is_some());
        assert!(matcher
            .prepare_commit_metered(&false_guard, &[], &full)
            .unwrap()
            .is_none());
        let refused = |_: usize, _: usize, _: usize| -> Result<(), RSpaceError> {
            Err(RSpaceError::HostWorkRejected)
        };
        assert!(matches!(
            matcher.prepare_commit_metered(&continuation, &[], &refused),
            Err(RSpaceError::HostWorkRejected)
        ));
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

    fn nested_send(depth: usize, leaf: i64) -> Par {
        let mut par = models::par_from_default! {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GInt(leaf)),
            }],
            ..Par::default()
        };
        for _ in 0..depth {
            par = models::par_from_default! {
                sends: vec![models::rhoapi::Send {
                    chan: Some(Par::default()),
                    data: vec![par],
                    ..Default::default()
                }],
                ..Par::default()
            };
        }
        par
    }

    type Charge = (usize, usize, usize);

    fn oracle_trace(target: &Par, pattern: &Par) -> (Result<bool, BackingError>, Vec<Charge>) {
        let trace = RefCell::new(Vec::new());
        let meter = |operations, scanned, backing| {
            trace.borrow_mut().push((operations, scanned, backing));
            Ok(())
        };
        let result = SpatialMatcherOracle.matches_metered(target, pattern, &meter);
        (result, trace.into_inner())
    }

    fn oracle_copy_prefix(target: &Par, pattern: &Par) -> Vec<Charge> {
        let trace = RefCell::new(vec![(
            1,
            0,
            clone_backing::arc_allocation_bytes::<Mutex<Option<RSpaceError>>>()
                .expect("matcher error slot size"),
        )]);
        let meter = |operations, scanned, backing| {
            trace.borrow_mut().push((operations, scanned, backing));
            Ok(())
        };
        clone_backing::reserve_blocks_copy_and_cleanup(target, &meter).expect("target copy charge");
        clone_backing::reserve_blocks_copy_and_cleanup(pattern, &meter)
            .expect("pattern copy charge");
        trace.into_inner()
    }

    /// MeteredEMatchesOracle's reservation prefix is context, target copy,
    /// pattern copy, then the existing PDA. The generated Par Clone is an
    /// explicit-worklist traversal, so the deep path needs no recursive clone
    /// fallback or invented depth rejection. Binder-depth+1 is unchanged:
    /// this oracle receives the already-scoped target and pattern by borrow.
    #[test]
    fn metered_ematches_oracle_is_deep_stack_safe_and_charge_ordered() {
        let oracle = SpatialMatcherOracle;
        for depth in [0, 1, 4, 12] {
            let target = nested_send(depth, 7);
            for leaf in [7, 8] {
                let pattern = nested_send(depth, leaf);
                let (metered, trace) = oracle_trace(&target, &pattern);
                assert_eq!(metered, Ok(oracle.matches(&target, &pattern)));
                assert!(trace.starts_with(&oracle_copy_prefix(&target, &pattern)));
            }
        }

        let target = nested_send(128, 7);
        let pattern = nested_send(128, 7);
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || {
                let (result, trace) = oracle_trace(&target, &pattern);
                assert_eq!(result, Ok(true));
                let copy_prefix = oracle_copy_prefix(&target, &pattern);
                assert!(trace.starts_with(&copy_prefix));
                assert!(trace.len() > copy_prefix.len());

                let cutoff = copy_prefix.len() / 2;
                let accepted = RefCell::new(Vec::new());
                let calls = Cell::new(0_usize);
                let bounded = |operations, scanned, backing| {
                    if calls.get() == cutoff {
                        return Err(BackingError::Rejected);
                    }
                    calls.set(calls.get() + 1);
                    accepted.borrow_mut().push((operations, scanned, backing));
                    Ok(())
                };
                assert_eq!(
                    SpatialMatcherOracle.matches_metered(&target, &pattern, &bounded),
                    Err(BackingError::Rejected)
                );
                assert_eq!(accepted.borrow().as_slice(), &trace[..cutoff]);
            })
            .expect("spawn 256 KiB oracle thread")
            .join()
            .expect("metered EMatches oracle exhausted the 256 KiB stack");
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

    fn commit_charge(
        continuation: &TaggedContinuation,
        matched: &[&ListParWithRandom],
    ) -> (Result<bool, RSpaceError>, [usize; 3]) {
        let used = Mutex::new([0usize; 3]);
        let meter = |operations: usize, scanned: usize, backing: usize| {
            let mut totals = used.lock().expect("usage lock");
            for (total, amount) in totals.iter_mut().zip([operations, scanned, backing]) {
                *total += amount;
            }
            Ok(())
        };
        let result = Matcher.check_commit_metered(continuation, matched, &meter);
        let totals = *used.lock().expect("usage lock");
        (result, totals)
    }

    /// D-M6 (DR-88): the metered commit check over borrowed matched data
    /// decides exactly like the unmetered check over owned data.
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
            let (metered, _) = commit_charge(&continuation, &borrowed);
            assert_eq!(
                metered,
                Ok(Matcher.check_commit(&continuation, &borrowed)),
                "guard {guard:?}, matched {owned:?}"
            );
        }
    }

    /// D-M1 (DR-88): the metered commit check reads only the guard, so its
    /// charge does not depend on the continuation body; without a guard it
    /// reads none of the matched data either.
    #[test]
    fn commit_check_charge_is_independent_of_continuation_body() {
        let small = binding(vec![bool_par(true)]);
        let large = binding((0..512).map(|_| body_par(16)).collect());
        for guard in [None, Some(bool_par(true)), Some(bound_var_par(0))] {
            let (short_result, short) =
                commit_charge(&continuation_with(guard.clone(), 1), &[&small]);
            let (long_result, long) =
                commit_charge(&continuation_with(guard.clone(), 4_096), &[&small]);
            assert_eq!(short_result, long_result, "guard {guard:?}");
            assert_eq!(short, long, "guard {guard:?}");
        }
        assert_eq!(
            commit_charge(&continuation_with(None, 4_096), &[&large]),
            (Ok(true), [1, 0, 0])
        );
        assert_eq!(
            commit_charge(&continuation_with(None, 1), &[&small]),
            (Ok(true), [1, 0, 0])
        );
    }

    /// Negative control for D-M1 (DR-88): the legacy caller walked the whole
    /// continuation before the commit check, so its charge grew with the
    /// unread body although the check read only the guard.
    #[test]
    fn legacy_commit_charge_grew_with_unread_body() {
        let legacy = |continuation: &TaggedContinuation| {
            let used = Mutex::new([0usize; 3]);
            let meter = |operations: usize, scanned: usize, backing: usize| {
                let mut totals = used.lock().expect("usage lock");
                for (total, amount) in totals.iter_mut().zip([operations, scanned, backing]) {
                    *total += amount;
                }
                Ok(())
            };
            clone_backing::inspect(continuation, &meter).expect("legacy inspection");
            let (_, check) = commit_charge(continuation, &[]);
            let walk = *used.lock().expect("usage lock");
            [walk[0] + check[0], walk[1] + check[1], walk[2] + check[2]]
        };
        let guard = Some(bool_par(true));
        let (_, short_check) = commit_charge(&continuation_with(guard.clone(), 1), &[]);
        let (_, long_check) = commit_charge(&continuation_with(guard.clone(), 4_096), &[]);
        assert_eq!(short_check, long_check);
        let short = legacy(&continuation_with(guard.clone(), 1));
        let long = legacy(&continuation_with(guard, 4_096));
        assert!(
            long[0] > short[0] && long[1] > short[1] * 100,
            "{short:?} vs {long:?}"
        );
    }

    fn legacy_extraction(free_map: &FreeMap, free_count: i32) -> Vec<Par> {
        (0..free_count)
            .map(|level| free_map.get(&level).cloned().unwrap_or_default())
            .collect()
    }

    fn metered_extraction(free_map: FreeMap, free_count: i32) -> (Option<Vec<Par>>, [usize; 3]) {
        let used = Mutex::new([0usize; 3]);
        let meter = |operations: usize, scanned: usize, backing: usize| {
            let mut totals = used.lock().expect("usage lock");
            for (total, amount) in totals.iter_mut().zip([operations, scanned, backing]) {
                *total += amount;
            }
            Ok(())
        };
        let mut context = SpatialMatcherContext::with_meter(&meter).expect("matcher context");
        let before = *used.lock().expect("usage lock");
        let extracted = Matcher::extract_bound_pars(free_map, free_count, &mut context);
        let after = *used.lock().expect("usage lock");
        (extracted, [
            after[0] - before[0],
            after[1] - before[1],
            after[2] - before[2],
        ])
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
            let (moved, _) = metered_extraction(free_map, free_count);
            proptest::prop_assert_eq!(moved, Some(expected));
        }
    }

    /// D-M4 (DR-91): the extraction charge does not depend on the sizes of
    /// the bound values.
    #[test]
    fn extraction_charge_independent_of_binding_sizes() {
        let map = |size: usize| -> FreeMap {
            [-2, 0, 1, 3, 9]
                .into_iter()
                .map(|key| (key, body_par(size)))
                .collect()
        };
        let (small, small_charge) = metered_extraction(map(1), 5);
        let (large, large_charge) = metered_extraction(map(512), 5);
        assert_eq!(small.map(|pars| pars.len()), Some(5));
        assert_eq!(large.map(|pars| pars.len()), Some(5));
        assert_eq!(small_charge, large_charge);
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
        let meter = |_: usize, _: usize, _: usize| Ok(());
        let mut context = SpatialMatcherContext::with_meter(&meter).expect("matcher context");
        let (pars, allocated) = measured(|| Matcher::extract_bound_pars(free_map, 3, &mut context));
        let pars = pars.expect("extracted bindings");
        assert_eq!(pars.len(), 3);
        let vector = pars.capacity() * std::mem::size_of::<Par>();
        assert!(
            allocated <= vector,
            "allocated {allocated}, result vector {vector}"
        );
        // A copy of a single binding would allocate its 256 expressions.
        assert!(vector < 256 * std::mem::size_of::<Expr>());
    }

    #[test]
    fn metered_match_preserves_remainder_binding_and_rejects_short_credit() {
        let pattern = BindPattern {
            patterns: Vec::new(),
            remainder: Some(Var {
                var_instance: Some(FreeVar(0)),
            }),
            free_count: 1,
        };
        let data = ListParWithRandom {
            pars: vec![Par::default(); 2],
            ..ListParWithRandom::default()
        };
        let matcher = Matcher;
        let expected = matcher.get(&pattern, &data);
        assert!(expected.is_some());
        let used = Mutex::new([0usize; 3]);
        let unlimited = |operations: usize, scanned: usize, backing: usize| {
            let mut totals = used.lock().unwrap();
            for (total, amount) in totals.iter_mut().zip([operations, scanned, backing]) {
                *total += amount;
            }
            Ok(())
        };
        assert_eq!(
            matcher.get_metered(&pattern, &data, &unlimited).unwrap(),
            expected
        );
        let required = *used.lock().unwrap();
        let native: &dyn Match<BindPattern, ListParWithRandom, TaggedContinuation> = &matcher;
        assert_eq!(
            native.get_metered(&pattern, &data, &unlimited).unwrap(),
            expected
        );
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
            assert!(matches!(
                matcher.get_metered(&pattern, &data, &meter),
                Err(RSpaceError::HostWorkRejected)
            ));
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]

        #[test]
        fn oracle_never_applies_a_match_without_reserved_credit(
            size in 0_usize..7,
            equal in proptest::bool::ANY,
            budget_choice in 0_usize..64,
        ) {
            let target = body_par(size);
            let pattern = body_par(size + usize::from(!equal));
            let oracle = SpatialMatcherOracle;
            let expected = oracle.matches(&target, &pattern);
            let measured = Mutex::new(0usize);
            let unlimited = |operations, _scanned, _backing| {
                *measured.lock().unwrap() += operations;
                Ok(())
            };
            proptest::prop_assert_eq!(
                oracle.matches_metered(&target, &pattern, &unlimited),
                Ok(expected)
            );
            let required = *measured.lock().unwrap();
            proptest::prop_assert!(required > 0);
            let limit = budget_choice % (required + 2);
            let spent = Mutex::new(0usize);
            let bounded = |operations, _scanned, _backing| {
                let mut current = spent.lock().unwrap();
                if *current + operations > limit {
                    return Err(BackingError::Rejected);
                }
                *current += operations;
                Ok(())
            };
            let result = oracle.matches_metered(&target, &pattern, &bounded);
            if limit < required {
                proptest::prop_assert_eq!(result, Err(BackingError::Rejected));
            } else {
                proptest::prop_assert_eq!(result, Ok(expected));
            }
            proptest::prop_assert!(*spent.lock().unwrap() <= limit);
        }
    }
}
