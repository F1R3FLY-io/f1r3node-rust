use std::marker::{Send, Sync};

use models::rhoapi::expr::ExprInstance;
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{new_elist_expr, FreeMap};
use rho_pure_eval::Env as PureEnv;
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

    /// Added by DR-129 (E3-a): `check_commit` vetoes only through a guard
    /// that is present and not empty, so exactly those continuations may
    /// veto.
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
}

fn is_empty_par(par: &Par) -> bool { par == &Par::default() }

/// Evaluates a guard against the combined cross-bind variables.
/// Returns true iff the guard reduces to GBool(true). Anything else
/// (false, non-bool, or eval-error) is treated as guard-fail.
fn guard_passes(condition: &Par, bound_pars: &[Par]) -> bool {
    let mut env: PureEnv<Par> = PureEnv::new();
    for p in bound_pars.iter() {
        if env.push(p.clone()).is_none() {
            return false;
        }
    }
    match rho_pure_eval::eval(condition, &env) {
        Ok(result) => extract_bool(&result) == Some(true),
        Err(_) => false,
    }
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
    match rho_pure_eval::eval(condition, &env) {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // clone_backing::inspect(&result, meter)?;
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
            guard: Some(Par {
                exprs: vec![Expr {
                    expr_instance: Some(ExprInstance::GBool(true)),
                }],
                ..Par::default()
            }),
            ..TaggedContinuation::default()
        };
        assert!(matcher.check_commit_by_reference(&continuation, &[]));
        let false_guard = TaggedContinuation {
            guard: Some(Par {
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
        Par {
            exprs: vec![Expr {
                expr_instance: Some(ExprInstance::GBool(value)),
            }],
            ..Par::default()
        }
    }

    fn bound_var_par(index: i32) -> Par {
        Par {
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
        Par {
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
                Matcher.check_commit(&continuation, &owned),
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
    /// an empty guard never vetoes. Every other guard may, so the Stored check
    /// of replay skips it, even a guard that is the literal `true`.
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
                let committed = matcher.check_commit(&continuation, matched);
                if !committed {
                    vetoed += 1;
                    assert!(may_veto, "{guard:?} vetoed {matched:?}");
                }
            }
        }
        assert!(vetoed >= 2, "the fixture must exercise a veto");
    }
}
