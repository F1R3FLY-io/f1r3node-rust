use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::par_bag::Multiplicity;
use num_bigint::BigUint;
use num_traits::ToPrimitive;
use proptest::prelude::*;
use rholang::rust::interpreter::matcher::par_count::{CountBound, ParCount};
use rholang::rust::interpreter::matcher::sub_pars::sub_pars;

#[path = "support/counting_alloc.rs"]
mod counting_alloc;

#[path = "support/par_count_resources.rs"]
mod par_count_resources;

#[global_allocator]
static ALLOCATOR: counting_alloc::Counting = counting_alloc::Counting;

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

fn interval_contains(value: u128, lower: Option<u128>, upper: Option<u128>) -> bool {
    lower.is_some_and(|lower| lower <= value) && upper.is_none_or(|upper| value <= upper)
}

fn positional_reference(counts: &[u64], bounds: [Option<u128>; 4]) -> Vec<Vec<u64>> {
    let total: u64 = counts.iter().sum();
    assert!(total <= 9);
    let mut result = Vec::new();
    for mask in 0..(1u64 << total) {
        let mut offset = 0;
        let selected: Vec<_> = counts
            .iter()
            .map(|count| {
                let bits = (mask >> offset) & ((1u64 << count) - 1);
                offset += count;
                u64::from(bits.count_ones())
            })
            .collect();
        let selected_total = u128::from(selected.iter().sum::<u64>());
        if interval_contains(selected_total, bounds[0], bounds[1])
            && interval_contains(u128::from(total) - selected_total, bounds[2], bounds[3])
            && !result.contains(&selected)
        {
            result.push(selected);
        }
    }
    result
}

fn assert_partition_sequence(
    target: &Par,
    endpoints: &[Expr],
    counts: &[u64],
    bounds: [Option<u128>; 4],
) -> usize {
    let expected = positional_reference(counts, bounds);
    let converted = bounds.map(|bound| {
        expression_bound(match bound {
            Some(value) => CountBound::Finite(value),
            None => CountBound::Unbounded,
        })
    });
    let mut actual = Vec::new();
    let maximum_candidates: u64 = counts.iter().map(|count| count + 1).product();
    for (index, (selected, remainder)) in sub_pars(
        target,
        &converted[0],
        &converted[1],
        &converted[2],
        &converted[3],
    )
    .enumerate()
    {
        assert!(
            (index as u64) < maximum_candidates,
            "candidate iterator repeated beyond its finite domain"
        );
        let selected_counts: Vec<_> = endpoints
            .iter()
            .zip(counts)
            .map(|(endpoint, available)| {
                let selected = selected.count(endpoint);
                let remainder = remainder.count(endpoint);
                assert_eq!(
                    u128::from(selected) + u128::from(remainder),
                    u128::from(*available)
                );
                selected
            })
            .collect();
        assert_eq!(
            selected.family_cardinality_exact::<Expr>(),
            u128::from(selected_counts.iter().sum::<u64>())
        );
        assert!(interval_contains(
            selected.family_cardinality_exact::<Expr>(),
            bounds[0],
            bounds[1]
        ));
        assert!(interval_contains(
            remainder.family_cardinality_exact::<Expr>(),
            bounds[2],
            bounds[3]
        ));
        actual.push(selected_counts);
    }
    assert_eq!(actual, expected, "counts={counts:?}, bounds={bounds:?}");
    for (endpoint, count) in endpoints.iter().zip(counts) {
        assert_eq!(target.count(endpoint), *count);
    }
    actual.len()
}

fn assert_fixture_trie_order(target: &Par, endpoints: &[Expr], counts: &[u64]) {
    let expected: Vec<_> = endpoints
        .iter()
        .zip(counts)
        .filter(|(_, count)| **count != 0)
        .map(|(endpoint, _)| endpoint.clone())
        .collect();
    let mut observed = Vec::new();
    target
        .try_visit_components::<Expr, std::convert::Infallible>(|endpoint, _| {
            observed.push(endpoint.as_ref().clone());
            Ok(())
        })
        .unwrap();
    assert_eq!(observed, expected);
}

#[test]
fn complete_small_partition_sequences_match_independent_positional_reference() {
    let mut cases = 0;
    let mut admitted = 0;
    for first in 0..=2 {
        for second in 0..=2 {
            let counts = [first, second];
            let (target, endpoints) = counted_target(&counts);
            let retained = target.clone();
            assert_fixture_trie_order(&target, &endpoints, &counts);
            let mut bounds: Vec<_> = (0..=u128::from(first + second) + 1).map(Some).collect();
            bounds.push(None);
            for selection_min in &bounds {
                for selection_max in &bounds {
                    for remainder_min in &bounds {
                        for remainder_max in &bounds {
                            admitted += assert_partition_sequence(&target, &endpoints, &counts, [
                                *selection_min,
                                *selection_max,
                                *remainder_min,
                                *remainder_max,
                            ]);
                            cases += 1;
                        }
                    }
                }
            }
            for endpoint in &endpoints {
                assert_eq!(target.count(endpoint), retained.count(endpoint));
            }
        }
    }
    assert_eq!(cases, 7_461);
    assert_eq!(admitted, 3_532);
    eprintln!(
        "exact counted partition oracle: {cases} interval cases, {admitted} ordered candidates"
    );
}

#[test]
fn fixed_endpoint_partition_scratch_does_not_grow_with_multiplicity() {
    let run = |multiplicity| {
        let (target, _) = counted_target(&[multiplicity, 1]);
        let zero = ParCount::zero();
        let top = ParCount::unbounded();
        let consume = || {
            let mut candidates = sub_pars(&target, &zero, &top, &zero, &zero);
            let result = candidates.next().unwrap();
            assert!(candidates.next().is_none());
            result
        };
        drop(consume());
        let ((selected, remainder), allocations, bytes) = counting_alloc::measure(consume);
        assert_eq!(
            selected.family_cardinality_exact::<Expr>(),
            u128::from(multiplicity) + 1
        );
        assert!(remainder.is_empty());
        (allocations, bytes)
    };
    let small = run(1);
    for multiplicity in [1_001, u64::MAX] {
        let large = run(multiplicity);
        assert_eq!(
            large, small,
            "fixed two-endpoint allocation changed at {multiplicity}"
        );
        eprintln!("exact counted partition scratch: multiplicity {multiplicity}, {} allocations, {} cumulative allocated bytes", large.0, large.1);
    }
}

#[test]
fn partition_allocations_scale_with_distinct_endpoints() {
    let measure = |width| {
        let (target, _) = counted_target(&vec![2; width]);
        let zero = ParCount::zero();
        let top = ParCount::unbounded();
        let ((selected, remainder), allocations, bytes) = counting_alloc::measure(|| {
            let mut candidates = sub_pars(&target, &zero, &top, &zero, &zero);
            let result = candidates.next().unwrap();
            assert!(candidates.next().is_none());
            result
        });
        assert_eq!(selected.family_distinct_len::<Expr>(), width);
        assert_eq!(
            selected.family_cardinality_exact::<Expr>(),
            2 * width as u128
        );
        assert!(remainder.is_empty());
        (allocations, bytes)
    };
    let low = measure(32);
    let high = measure(128);
    counting_alloc::assert_subquadratic("partition allocation calls", 32, low.0, 128, high.0)
        .unwrap();
    counting_alloc::assert_subquadratic("partition allocated bytes", 32, low.1, 128, high.1)
        .unwrap();
    eprintln!("exact counted partition width: 32 endpoints = {low:?}; 128 endpoints = {high:?} (allocation calls, cumulative allocated bytes)");
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct(
            concat!(env!("CARGO_MANIFEST_DIR"), "/proptest-regressions/matcher_count_intervals.txt")
        ))),
        ..ProptestConfig::default()
    })]

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

    #[test]
    fn generated_partition_sequences_match_independent_positional_reference(
        counts in prop::collection::vec(0u64..=3, 0..=3),
        bounds in prop::array::uniform4(prop::option::of(0u128..=12))
    ) {
        let (target, endpoints) = counted_target(&counts);
        assert_fixture_trie_order(&target, &endpoints, &counts);
        assert_partition_sequence(&target, &endpoints, &counts, bounds);
    }
}
