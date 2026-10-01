use std::num::NonZeroUsize;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
}

fn limits() -> FundingSearchLimits {
    FundingSearchLimits {
        source_cap: NonZeroUsize::new(256).unwrap(),
        obligation_cap: NonZeroUsize::new(256).unwrap(),
    }
}

fn check(
    capacity: &[u64],
    demand: &[u64],
    edges: &[Vec<bool>],
    contribution: &[u64],
    selected: &[bool],
) -> Result<bool, FundingSearchError> {
    check_funding_domain_counterexample(
        capacity,
        demand,
        edges,
        FundingDomainCounterexampleView {
            contributions: contribution,
            selected_obligations: selected,
        },
        limits(),
        &budget(),
    )
}

#[test]
fn combined_restriction_returns_the_formal_capped_counterexample() {
    let edges = [vec![false, false, true], vec![true, true, true]];
    let witness = build_funding_domain_counterexample(
        &[3, 6],
        &[2, 2, 2],
        &edges,
        &[true, false],
        limits(),
        &budget(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(witness.view().contributions, &[3, 3]);
    assert_eq!(witness.view().selected_obligations, &[true, true, false]);
    assert_eq!(
        check(&[3, 6], &[2, 2, 2], &edges, &[3, 3], &[true, true, false]),
        Ok(true)
    );
    assert_eq!(
        check(&[3, 6], &[2, 2, 2], &edges, &[4, 2], &[true, true, false]),
        Ok(false)
    );
    assert_eq!(
        check(&[3, 6], &[2, 2, 2], &edges, &[3, 2], &[true, true, false]),
        Ok(false)
    );
    assert_eq!(
        check(&[3, 6], &[2, 2, 2], &edges, &[2, 4], &[true, true, false]),
        Ok(false)
    );
    assert_eq!(
        check(&[3, 6], &[2, 2, 2], &edges, &[3, 3], &[false, false, false]),
        Ok(false)
    );
    assert_eq!(
        check(
            &[3, 6],
            &[2, 2, 2],
            &[vec![true, true, true], vec![true, true, true]],
            &[3, 3],
            &[true, true, false]
        ),
        Ok(false)
    );
}

#[test]
fn witnesses_preserve_wide_capacity_arithmetic_and_zero_semantics() {
    let edges = [vec![false, true], vec![true, true]];
    let witness = build_funding_domain_counterexample(
        &[u64::MAX, u64::MAX],
        &[1, u64::MAX - 1],
        &edges,
        &[true, false],
        limits(),
        &budget(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(witness.view().contributions, &[u64::MAX, 0]);
    assert_eq!(
        check(
            &[u64::MAX, u64::MAX],
            &[1, u64::MAX - 1],
            &edges,
            &[u64::MAX, u64::MAX],
            &[true, false]
        ),
        Ok(false)
    );
    assert_eq!(
        build_funding_domain_counterexample(
            &[u64::MAX, u64::MAX],
            &[],
            &[vec![], vec![]],
            &[true, false],
            limits(),
            &budget()
        ),
        Ok(None)
    );
    assert_eq!(
        build_funding_domain_counterexample(
            &[1, 1],
            &[1, 1],
            &[vec![true, false], vec![true, true]],
            &[true, false],
            limits(),
            &budget()
        ),
        Ok(None)
    );
}

#[test]
fn malformed_witnesses_and_insufficient_aggregate_capacity_are_explicit_errors() {
    let edges = [vec![false], vec![true]];
    assert_eq!(
        check(&[1, 1], &[1], &edges, &[1], &[true]),
        Err(FundingAssignmentError::InvalidDimensions.into())
    );
    assert_eq!(
        check(&[1, 1], &[1], &edges, &[1, 0], &[]),
        Err(FundingAssignmentError::InvalidDimensions.into())
    );
    assert_eq!(
        check(&[1, 1], &[1], &[vec![], vec![true]], &[1, 0], &[true]),
        Err(FundingAssignmentError::InvalidDimensions.into())
    );
    assert_eq!(
        check(&[], &[], &[], &[], &[]),
        Err(FundingAssignmentError::EmptySources.into())
    );
    assert_eq!(
        build_funding_domain_counterexample(
            &[1, 1],
            &[3],
            &edges,
            &[true, false],
            limits(),
            &budget()
        ),
        Err(FundingAssignmentError::InsufficientSourceCapacity.into())
    );
    assert_eq!(
        build_funding_domain_counterexample(&[1, 1], &[1], &edges, &[true], limits(), &budget()),
        Err(FundingAssignmentError::InvalidDimensions.into())
    );
    assert_eq!(
        build_funding_domain_counterexample(
            &[1],
            &[u64::MAX, 1],
            &[vec![true, true]],
            &[true],
            limits(),
            &budget()
        ),
        Err(FundingAssignmentError::Overflow.into())
    );
    let tiny = FundingSearchLimits {
        source_cap: NonZeroUsize::new(1).unwrap(),
        obligation_cap: NonZeroUsize::new(1).unwrap(),
    };
    assert_eq!(
        build_funding_domain_counterexample(&[1, 1], &[1], &edges, &[true, false], tiny, &budget()),
        Err(FundingAssignmentError::TooManySources.into())
    );
    assert_eq!(
        build_funding_domain_counterexample(
            &[1],
            &[1, 1],
            &[vec![true, true]],
            &[true],
            tiny,
            &budget()
        ),
        Err(FundingAssignmentError::TooManyObligations.into())
    );
}

#[test]
fn every_budget_prefix_returns_no_partial_witness() {
    let capacity = [3, 6];
    let demand = [2, 2, 2];
    let edges = [vec![false, false, true], vec![true, true, true]];
    let measured = budget();
    assert!(build_funding_domain_counterexample(
        &capacity,
        &demand,
        &edges,
        &[true, false],
        limits(),
        &measured
    )
    .unwrap()
    .is_some());
    for dimension in [
        HostWorkDimension::SearchCandidates,
        HostWorkDimension::SearchStateBytes,
        HostWorkDimension::VerificationOperations,
    ] {
        for amount in 0..measured.usage(dimension).get() {
            let mut bounds = HostWorkLimits::uniform(HostWorkLimit::new(100_000_000));
            bounds.set(dimension, HostWorkLimit::new(amount));
            let work = HostWorkBudget::new(bounds);
            assert!(matches!(
                build_funding_domain_counterexample(
                    &capacity,
                    &demand,
                    &edges,
                    &[true, false],
                    limits(),
                    &work
                ),
                Err(FundingSearchError::HostWork(_))
            ));
            assert!(work.is_rejected());
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_cut_witnesses_match_wide_integer_bounds(
        capacity in prop::collection::vec(any::<u64>(), 1..130),
        demand in prop::collection::vec(0_u64..1_000_000, 0..9),
        selectors in prop::collection::vec(any::<bool>(), 129),
        bits in prop::collection::vec(any::<bool>(), 1032),
    ) {
        let edges: Vec<Vec<bool>> = (0..capacity.len()).map(|i|
            (0..demand.len()).map(|j| bits[i * 8 + j]).collect()).collect();
        let selected = &selectors[..capacity.len()];
        let total: u128 = demand.iter().copied().map(u128::from).sum();
        let available: u128 = capacity.iter().copied().map(u128::from).sum();
        let outcome = build_funding_domain_counterexample(&capacity, &demand, &edges, selected, limits(), &budget());
        if available < total {
            prop_assert_eq!(outcome, Err(FundingAssignmentError::InsufficientSourceCapacity.into()));
        } else {
            let selected_capacity: u128 = capacity.iter().zip(selected).filter(|(_, selected)| **selected)
                .map(|(amount, _)| u128::from(*amount)).sum();
            let neighbor_demand: u128 = demand.iter().enumerate().filter(|(j, _)|
                selected.iter().zip(&edges).any(|(selected, row)| *selected && row[*j]))
                .map(|(_, amount)| u128::from(*amount)).sum();
            let witness = outcome.unwrap();
            if total.min(selected_capacity) > neighbor_demand {
                prop_assert!(witness.is_some());
            }
            if let Some(witness) = witness {
                let view = witness.view();
                prop_assert_eq!(view.contributions.iter().copied().map(u128::from).sum::<u128>(), total);
                prop_assert!(view.contributions.iter().zip(&capacity).all(|(draw, cap)| draw <= cap));
                let selected_draw: u128 = view.contributions.iter().zip(selected).filter(|(_, selected)| **selected)
                    .map(|(draw, _)| u128::from(*draw)).sum();
                prop_assert_eq!(selected_draw, total.min(selected_capacity));
                let unfunded_demand: u128 = demand.iter().zip(view.selected_obligations).filter(|(_, chosen)| **chosen)
                    .map(|(amount, _)| u128::from(*amount)).sum();
                let neighboring_supply: u128 = view.contributions.iter().zip(&edges).filter(|(_, row)|
                    row.iter().zip(view.selected_obligations).any(|(allowed, chosen)| *allowed && *chosen))
                    .map(|(amount, _)| u128::from(*amount)).sum();
                prop_assert!(neighboring_supply < unfunded_demand);
            }
        }
    }
}
