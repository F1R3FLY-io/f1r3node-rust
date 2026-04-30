use std::marker::{Send, Sync};

use models::rhoapi::expr::ExprInstance;
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{new_elist_expr, to_vec};
use rho_pure_eval::Env as PureEnv;
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
impl Match<BindPattern, ListParWithRandom> for Matcher {
    fn get(&self, pattern: BindPattern, data: ListParWithRandom) -> Option<ListParWithRandom> {
        let mut spatial_matcher = SpatialMatcherContext::new();

        let fold_match_result =
            spatial_matcher.fold_match(data.pars, pattern.patterns, pattern.remainder);
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

                // Optional `where`-clause guard. If present, evaluate
                // it via rho-pure-eval against this bind's bound vars.
                // Treat anything-not-GBool(true) as match-fail (plan
                // §3.5 / Option 3): guard-fail is indistinguishable
                // from spatial mismatch, so the messages stay in the
                // tuple space and the continuation stays installed.
                //
                // Limitation: cross-channel guards (in &-joined receives
                // where the guard mentions vars from another bind) are
                // evaluated per-channel here, so a missing var triggers
                // an UnboundVariable error which we treat as guard-fail.
                // Full multi-channel coordination is a follow-up.
                if let Some(condition) = pattern.condition.as_ref() {
                    if !is_empty_par(condition) && !guard_passes(condition, &bound_pars) {
                        return None;
                    }
                }

                Some(ListParWithRandom {
                    pars: bound_pars,
                    random_state: data.random_state,
                })
            }
            None => None,
        }
    }
}

fn is_empty_par(par: &Par) -> bool { par == &Par::default() }

/// Evaluates a guard against bind-bound variables. Returns true iff
/// the guard reduces to GBool(true). Anything else (false, non-bool,
/// eval-error including unbound-variable for cross-channel references)
/// is treated as guard-fail.
fn guard_passes(condition: &Par, bound_pars: &[Par]) -> bool {
    let mut env: PureEnv<Par> = PureEnv::new();
    for p in bound_pars.iter() {
        env = env.put(p.clone());
    }
    match rho_pure_eval::eval(condition, &env) {
        Ok(result) => extract_bool(&result) == Some(true),
        Err(_) => false,
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
