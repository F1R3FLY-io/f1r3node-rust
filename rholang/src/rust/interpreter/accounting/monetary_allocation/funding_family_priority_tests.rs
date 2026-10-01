use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::monetary_allocation::FundingSearchLimits;

fn work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
}

fn limits() -> FundingFamilyLimits {
    FundingFamilyLimits {
        search: FundingSearchLimits {
            source_cap: NonZeroUsize::new(129).unwrap(),
            obligation_cap: NonZeroUsize::new(16).unwrap(),
        },
        case_cap: NonZeroUsize::new(16).unwrap(),
    }
}

fn key(rows: &[&[u64]], cursor: usize) -> FundingFamilyResourcePriority {
    FundingFamilyResourcePriority::from_canonical_resource_draws(rows, cursor, limits(), &work())
        .unwrap()
}

#[test]
fn family_priority_later_rank_wins_over_earlier_residual_tie() {
    let fair = key(&[&[0, 1], &[1, 1]], 0);
    let early_tie = key(&[&[1, 0], &[2, 0]], 0);
    assert_eq!(fair.compare(&early_tie, &work()).unwrap(), Ordering::Less);
    assert_eq!(
        early_tie.compare(&fair, &work()).unwrap(),
        Ordering::Greater
    );
}

#[test]
fn family_priority_selects_alpha_tie_after_all_fairness_ranks() {
    let alpha = key(&[&[0, 1, 1, 0, 0], &[0, 1, 1, 1, 0]], 0);
    let beta = key(&[&[0, 0, 1, 1, 0], &[1, 0, 1, 1, 0]], 0);
    assert_eq!(alpha.compare(&beta, &work()).unwrap(), Ordering::Less);
}

#[test]
fn family_priority_is_not_worst_case_liability_minimization() {
    let x = key(&[&[2, 0], &[2, 2]], 0);
    let y = key(&[&[1, 1], &[3, 1]], 0);
    assert_eq!(y.compare(&x, &work()).unwrap(), Ordering::Less);
}

#[test]
fn family_priority_rejects_incompatible_and_unbounded_inputs() {
    let base = key(&[&[1, 0]], 0);
    for other in [
        key(&[&[2, 0]], 0),
        key(&[&[1, 0]], 1),
        key(&[&[1, 0, 0]], 0),
        key(&[&[1, 0], &[0, 0]], 0),
    ] {
        assert_eq!(
            base.compare(&other, &work()),
            Err(FundingFamilyError::IncompatiblePriorityContext)
        );
    }
    assert!(
        FundingFamilyResourcePriority::from_canonical_resource_draws(
            &[&[u64::MAX, 1]],
            0,
            limits(),
            &work()
        )
        .is_err()
    );
    assert!(
        FundingFamilyResourcePriority::from_canonical_resource_draws(
            &[&[1, 0], &[1]],
            0,
            limits(),
            &work()
        )
        .is_err()
    );
    let empty = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    assert!(matches!(
        base.compare(&base, &empty),
        Err(FundingFamilyError::Search(FundingSearchError::HostWork(_)))
    ));
}

#[test]
fn family_priority_supports_large_source_counts_without_amount_iteration() {
    for n in [1, 3, 65, 129] {
        let mut amounts = vec![0; n];
        amounts[n - 1] = u64::MAX;
        for cursor in [0, n - 1] {
            let value = key(&[&amounts], cursor);
            assert_eq!(value.compare(&value, &work()).unwrap(), Ordering::Equal);
        }
    }
}

fn oracle(left: &[Vec<u64>], right: &[Vec<u64>], cursor: usize) -> Ordering {
    for (lhs, rhs) in left.iter().zip(right) {
        let mut histogram = BTreeMap::<u64, (usize, usize)>::new();
        for value in lhs {
            histogram.entry(*value).or_default().0 += 1;
        }
        for value in rhs {
            histogram.entry(*value).or_default().1 += 1;
        }
        for (l, r) in histogram.values().rev() {
            if l != r {
                return l.cmp(r);
            }
        }
    }
    for (lhs, rhs) in left.iter().zip(right) {
        for offset in 0..lhs.len() {
            let i = (cursor + offset) % lhs.len();
            if lhs[i] != rhs[i] {
                return rhs[i].cmp(&lhs[i]);
            }
        }
    }
    Ordering::Equal
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn family_priority_matches_histogram_oracle_and_order_laws(
        data in (1_usize..7, 1_usize..5).prop_flat_map(|(n, k)| (
            Just((n, k)), prop::collection::vec(0_u64..17, k),
            prop::collection::vec(0_usize..n, k*16*3), 0_usize..n,
        ))
    ) {
        let ((n, k), amounts, recipients, cursor) = data;
        let mut candidates = vec![vec![vec![0_u64; n]; k]; 3];
        for c in 0..3 {
            for o in 0..k {
                for unit in 0..amounts[o] as usize { candidates[c][o][recipients[(c*k+o)*16+unit]] += 1; }
            }
        }
        let keys: Vec<_> = candidates.iter().map(|rows| key(&rows.iter().map(Vec::as_slice).collect::<Vec<_>>(), cursor)).collect();
        for a in 0..3 {
            for b in 0..3 {
                let comparison = keys[a].compare(&keys[b], &work()).unwrap();
                prop_assert_eq!(comparison, oracle(&candidates[a], &candidates[b], cursor));
                prop_assert_eq!(comparison.reverse(), keys[b].compare(&keys[a], &work()).unwrap());
                if comparison == Ordering::Equal { prop_assert_eq!(&candidates[a], &candidates[b]); }
                for c in 0..3 {
                    if comparison != Ordering::Greater && keys[b].compare(&keys[c], &work()).unwrap() != Ordering::Greater {
                        prop_assert_ne!(keys[a].compare(&keys[c], &work()).unwrap(), Ordering::Greater);
                    }
                }
            }
        }
    }
}
