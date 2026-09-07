use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::par_bag::Multiplicity;
use num_bigint::BigUint;
use num_traits::ToPrimitive;
use proptest::prelude::*;
use rholang::rust::interpreter::matcher::par_count::{CountBound, ParCount};
use rholang::rust::interpreter::matcher::sub_pars::sub_pars;

fn integer(value: i64) -> Expr {
    Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }
}

fn counted_target(counts: &[u64]) -> (Par, Vec<Expr>) {
    let mut target = Par::default();
    let endpoints: Vec<_> = (0..counts.len())
        .map(|index| integer(index as i64))
        .collect();
    for (endpoint, count) in endpoints.iter().zip(counts) {
        if *count != 0 {
            target
                .insert_component_many(
                    endpoint,
                    Multiplicity::new(*count).unwrap(),
                    Vec::new(),
                    false,
                )
                .unwrap();
        }
    }
    (target, endpoints)
}

fn expression_bound(value: CountBound) -> ParCount {
    let mut bound = ParCount::zero();
    bound.exprs = value;
    bound
}

#[test]
fn finite_maximum_is_distinct_from_top_and_zero_annihilates_top() {
    let finite = CountBound::Finite(u128::MAX);
    let top = CountBound::Unbounded;
    assert!(finite < top);
    assert_eq!(finite.add(CountBound::Finite(0)), finite);
    assert_eq!(finite.add(CountBound::Finite(1)), top);
    assert_eq!(finite.scale(1), finite);
    assert_eq!(finite.scale(2), top);
    assert_eq!(top.scale(0), CountBound::Finite(0));
    assert_eq!(top.scale(1), top);
    assert_eq!(
        expression_bound(finite).min(&expression_bound(top)).exprs,
        finite
    );
    assert_eq!(
        expression_bound(finite).max(&expression_bound(top)).exprs,
        top
    );
    assert_eq!(
        expression_bound(finite)
            .add(&expression_bound(CountBound::Finite(1)))
            .exprs,
        top
    );
}

#[test]
fn overflowing_or_impossible_lower_bounds_never_invent_a_partition() {
    let (target, _) = counted_target(&[1]);
    let zero = ParCount::zero();
    let upper = ParCount::unbounded();
    let overflow = CountBound::Finite(u128::MAX).add(CountBound::Finite(1));
    for lower in [
        CountBound::Unbounded,
        overflow,
        CountBound::Finite(2),
        CountBound::Finite(u128::MAX),
    ] {
        let lower = expression_bound(lower);
        assert!(sub_pars(&target, &lower, &upper, &zero, &upper)
            .next()
            .is_none());
        assert!(sub_pars(&target, &zero, &upper, &lower, &upper)
            .next()
            .is_none());
    }
    let overflowing_upper = expression_bound(overflow);
    let mut candidates = sub_pars(&target, &zero, &overflowing_upper, &zero, &zero);
    let (selected, remainder) = candidates.next().unwrap();
    assert_eq!(selected.family_cardinality_exact::<Expr>(), 1);
    assert!(remainder.is_empty());
    assert!(candidates.next().is_none());
    let one = expression_bound(CountBound::Finite(1));
    assert!(sub_pars(&target, &one, &zero, &zero, &upper)
        .next()
        .is_none());
    assert!(sub_pars(&target, &zero, &upper, &one, &zero)
        .next()
        .is_none());
}

#[test]
fn finite_u64_maximum_partition_api_does_not_mean_unbounded() {
    let (target, endpoints) = counted_target(&[u64::MAX, 1]);
    let retained = target.clone();
    let exact_total = u128::from(u64::MAX) + 1;
    assert_eq!(target.family_cardinality_exact::<Expr>(), exact_total);
    let mut finite = target
        .components()
        .family_partitions::<Expr>(u64::MAX, u64::MAX);
    for _ in 0..2 {
        let (selected, remainder) = finite.next().unwrap();
        assert_eq!(
            selected.family_cardinality_exact::<Expr>(),
            u128::from(u64::MAX)
        );
        assert_eq!(remainder.family_cardinality_exact::<Expr>(), 1);
        for endpoint in &endpoints {
            assert_eq!(
                u128::from(selected.count(endpoint)) + u128::from(remainder.count(endpoint)),
                u128::from(target.count(endpoint))
            );
        }
    }
    assert!(finite.next().is_none());
    let mut exact = target
        .components()
        .family_partitions_exact::<Expr>(exact_total, exact_total);
    let (selected, remainder) = exact.next().unwrap();
    assert_eq!(selected.family_cardinality_exact::<Expr>(), exact_total);
    assert_eq!(remainder.family_cardinality_exact::<Expr>(), 0);
    assert!(exact.next().is_none());
    for endpoint in &endpoints {
        assert_eq!(target.count(endpoint), retained.count(endpoint));
    }
}

fn quotient(value: BigUint) -> CountBound {
    match value.to_u128() {
        Some(value) => CountBound::Finite(value),
        None => CountBound::Unbounded,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn finite_count_arithmetic_matches_unbounded_integer_reference(
        left in any::<u128>(), right in any::<u128>(), factor in any::<u64>()
    ) {
        let l = expression_bound(CountBound::Finite(left));
        let r = expression_bound(CountBound::Finite(right));
        prop_assert_eq!(l.add(&r).exprs, quotient(BigUint::from(left) + BigUint::from(right)));
        prop_assert_eq!(l.min(&r).exprs, CountBound::Finite(if left < right { left } else { right }));
        prop_assert_eq!(l.max(&r).exprs, CountBound::Finite(if left > right { left } else { right }));
        prop_assert_eq!(l.exprs.scale(factor), quotient(BigUint::from(left) * BigUint::from(factor)));
        prop_assert_eq!(l.add(&ParCount::unbounded()), ParCount::unbounded());
        prop_assert_eq!(l.min(&ParCount::unbounded()), l.clone());
        prop_assert_eq!(l.max(&ParCount::unbounded()), ParCount::unbounded());
    }
}
