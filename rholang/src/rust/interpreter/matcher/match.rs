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
impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for Matcher {
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

                Some(ListParWithRandom {
                    pars: bound_pars,
                    random_state: data.random_state,
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
}

fn is_empty_par(par: &Par) -> bool { par == &Par::default() }

/// Evaluates a guard against the combined cross-bind variables.
/// Returns true iff the guard reduces to GBool(true). Anything else
/// (false, non-bool, or eval-error) is treated as guard-fail.
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
