use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;

fn budget(limit: u64) -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(limit)))
}

#[test]
fn native_family_preflight_bounds_each_hold_without_bounding_the_sum() {
    let maximum = i64::MAX as u64;
    for holds in [vec![0], vec![maximum], vec![maximum; 129]] {
        let work = budget(holds.len() as u64);
        check_native_holds(&holds, &work).unwrap();
        assert_eq!(
            work.usage(HostWorkDimension::VerificationOperations).get(),
            holds.len() as u64
        );
    }
    for too_large in [maximum + 1, u64::MAX] {
        for position in 0..129 {
            let mut holds = vec![0; 129];
            holds[position] = too_large;
            assert!(matches!(
                check_native_holds(&holds, &budget(129)),
                Err(NativePhloPolicyError::Amount(
                    NativePhloAmountError::OutOfRange
                ))
            ));
        }
    }
}

#[test]
fn native_family_preflight_reserves_the_complete_scan_before_inspection() {
    for holds in [[0, 0], [u64::MAX, 0], [0, u64::MAX]] {
        let work = budget(1);
        assert!(matches!(
            check_native_holds(&holds, &work),
            Err(NativePhloPolicyError::Search(FundingSearchError::HostWork(
                _
            )))
        ));
        assert!(work.is_rejected());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn native_family_preflight_accepts_exactly_representable_holds(
        holds in prop::collection::vec(any::<u64>(), 1..130),
    ) {
        let original = holds.clone();
        let result = check_native_holds(&holds, &budget(holds.len() as u64));
        prop_assert_eq!(result.is_ok(), holds.iter().all(|hold| *hold <= i64::MAX as u64));
        prop_assert_eq!(holds, original);
    }

    #[test]
    fn native_family_preflight_implies_all_bounded_branch_partitions_fit(
        holds in prop::collection::vec(0_u64..=i64::MAX as u64, 1..130),
        branches in prop::collection::vec((any::<u64>(), any::<u64>()), 1..17),
    ) {
        check_native_holds(&holds, &budget(holds.len() as u64)).unwrap();
        for (index, hold) in holds.iter().copied().enumerate() {
            let custody = index.to_be_bytes();
            for (debit_seed, fee_seed) in &branches {
                let debit = debit_seed % (hold + 1);
                let fee = fee_seed % (debit + 1);
                let amounts = NativePhloSourceAmounts::checked(&custody, hold, debit, fee).unwrap();
                prop_assert_eq!(amounts.custody(), custody.as_slice());
                prop_assert_eq!(amounts.acquisition() as u64, debit - fee);
                prop_assert_eq!(amounts.refund() as u64, hold - debit);
                prop_assert_eq!(amounts.fee() as u64, fee);
                prop_assert_eq!(
                    amounts.acquisition() as u128 + amounts.fee() as u128 + amounts.refund() as u128,
                    u128::from(hold)
                );
            }
        }
    }
}
