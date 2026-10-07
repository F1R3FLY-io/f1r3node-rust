use std::marker::{Send, Sync};

use models::rhoapi::expr::ExprInstance;
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{new_elist_expr, FreeMap};
use rho_pure_eval::Env as PureEnv;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;
use rspace_plus_plus::rspace::r#match::Match;
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
        spatial_matcher.reserve_clone(&data.random_state)?;
        spatial_matcher.reserve_clone(&data.cost_authority)?;
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

    use models::rhoapi::tagged_continuation::TaggedCont;
    use models::rhoapi::var::VarInstance::FreeVar;
    use models::rhoapi::ParWithRandom;

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
                Ok(Matcher.check_commit(&continuation, &owned)),
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
}
