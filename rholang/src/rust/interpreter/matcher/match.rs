use std::marker::{Send, Sync};

use models::rhoapi::expr::ExprInstance;
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::new_elist_expr;
use rho_pure_eval::Env as PureEnv;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;
use rspace_plus_plus::rspace::r#match::Match;
use shared::rust::clone_backing::{self, BackingError, BackingMeter};
use shared::rust::collection_backing::tree_backing;

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
        spatial_matcher.reserve_clone(&pattern.remainder)?;
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
            spatial_matcher.reserve_inspect(&free_map)?;
            let Some(entries) = free_map.len().checked_add(1) else {
                return spatial_matcher.reject(RSpaceError::HostWorkRejected);
            };
            let Some((operations, bytes)) = tree_backing::<i32, Par>(entries) else {
                return spatial_matcher.reject(RSpaceError::HostWorkRejected);
            };
            spatial_matcher.reserve(operations, bytes, bytes)?;
            free_map.insert(*level, remainder);
        }
        let mut bound_pars = Vec::new();
        spatial_matcher.reserve_vec(&mut bound_pars, pattern.free_count.max(0) as usize)?;
        for level in 0..pattern.free_count {
            spatial_matcher.reserve_inspect(&free_map)?;
            if let Some(par) = free_map.get(&level) {
                spatial_matcher.reserve_clone(par)?;
                bound_pars.push(par.clone());
            } else {
                bound_pars.push(Par::default());
            }
        }
        spatial_matcher.reserve_clone(&data.random_state)?;
        spatial_matcher.reserve_clone(&data.cost_authority)?;
        Some(ListParWithRandom {
            pars: bound_pars,
            random_state: data.random_state.clone(),
            cost_authority: data.cost_authority.clone(),
            cost_stack: None,
        })
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
    fn check_commit(&self, k: &TaggedContinuation, matched: &[ListParWithRandom]) -> bool {
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

    fn check_commit_metered(
        &self,
        k: &TaggedContinuation,
        matched: &[ListParWithRandom],
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
            clone_backing::inspect(guard, &backing)?;
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
}

fn is_empty_par(par: &Par) -> bool { par == &Par::default() }

/// Evaluates a guard against the combined cross-bind variables.
/// Returns true iff the guard reduces to GBool(true). Anything else
/// (false, non-bool, or eval-error) is treated as guard-fail.
fn guard_passes(condition: &Par, bound_pars: &[Par]) -> bool {
    let mut env: PureEnv<Par> = PureEnv::new();
    for p in bound_pars.iter() {
        if env.push(p.clone()).is_err() {
            return false;
        }
    }
    match rho_pure_eval::eval(condition, &env) {
        Ok(result) => extract_bool(&result) == Some(true),
        Err(_) => false,
    }
}

fn guard_passes_metered(
    condition: &Par,
    matched: &[ListParWithRandom],
    meter: &dyn BackingMeter,
) -> Result<bool, BackingError> {
    let mut env: PureEnv<Par> = PureEnv::new();
    for binding in matched {
        meter.reserve(1, 0, 0)?;
        for par in &binding.pars {
            clone_backing::reserve_copy_and_cleanup(par, meter)?;
            env.push_metered(par.clone(), meter)?;
        }
    }
    match rho_pure_eval::eval_metered(condition, &env, meter) {
        Ok(result) => {
            clone_backing::inspect(&result, meter)?;
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
mod metered_tests {
    use std::sync::Mutex;

    use models::rhoapi::var::VarInstance::FreeVar;

    use super::*;

    #[test]
    fn metered_commit_guard_preserves_boolean_result_and_rejects_short_credit() {
        let matcher = Matcher;
        let continuation = TaggedContinuation {
            guard: Some(Par {
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
            guard: Some(Par {
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
}
