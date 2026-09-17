use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
use proptest::prelude::*;

use super::*;
use crate::rust::util::rholang::costacc::vault_cost_deploy::VaultRole;

fn budget() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(100_000_000)))
}

fn payer(index: u32) -> VaultPayer {
    let signature = CostSignature {
        value: Some(Value::Ground(index.to_be_bytes().to_vec())),
    };
    vault_payer(&signature).unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]
    #[test]
    fn projection_preserves_arbitrary_cohorts_and_each_native_amount(
        seeds in prop::collection::vec((any::<u64>(), any::<u64>(), any::<u64>()), 1..97),
        reverse in any::<bool>(),
    ) {
        let mut entries: Vec<_> = (0..seeds.len()).map(|i| {
            let payer = payer(i as u32);
            (payer.custody_key, payer)
        }).collect();
        if reverse { entries.reverse(); }
        let payers: BTreeMap<_, _> = entries.into_iter().collect();
        let canonical: Vec<_> = payers.keys().map(|key| key.as_slice()).collect();
        let rows: Vec<_> = canonical.iter().zip(&seeds).map(|(custody, &(a,b,c))| {
            let hold = a % (i64::MAX as u64 + 1);
            let debit = b % (hold + 1);
            let fee = c % (debit + 1);
            SettlementRow { custody, hold: hold as i64, acquisition: (debit - fee) as i64,
                fee: fee as i64, refund: (hold - debit) as i64 }
        }).collect();
        let projected = project_amounts(&payers, &canonical, rows.iter().copied(), &budget()).unwrap();
        prop_assert_eq!(projected.payer_count.get(), seeds.len());
        prop_assert_eq!(projected.has_resource, rows.iter().any(|row| row.acquisition > 0));
        prop_assert_eq!(projected.has_fee, rows.iter().any(|row| row.fee > 0));
        prop_assert_eq!(projected.allocations.len(), rows.iter().filter(|row| row.hold > 0).count());
        let mut index = 0;
        for ((_, payer), row) in payers.iter().zip(&rows) {
            if row.hold == 0 { prop_assert_eq!((row.acquisition, row.fee, row.refund), (0,0,0)); continue; }
            let allocation = &projected.allocations[index];
            let settlement = &projected.settlements[index];
            prop_assert_eq!(&allocation.address, &payer.address.to_base58());
            prop_assert_eq!(&settlement.address, &allocation.address);
            prop_assert_eq!((allocation.role, settlement.role), (VaultRole::General, VaultRole::General));
            prop_assert_eq!((allocation.amount, settlement.burn, settlement.fee), (row.hold, row.acquisition, row.fee));
            prop_assert_eq!(allocation.amount - settlement.burn - settlement.fee, row.refund);
            index += 1;
        }
        if rows.len() > 1 {
            let mut substituted = rows.clone();
            substituted[0].custody = rows[1].custody;
            prop_assert!(matches!(project_amounts(&payers, &canonical, substituted.into_iter(), &budget()), Err(DirectWalletSettlementError::CustodyMismatch)));
        }
    }

    #[test]
    fn transitions_use_observed_scope_revision_position_and_complete_cohort(
        n in 1usize..1025, revision in 0i64..i64::MAX, position in any::<u16>(), next in any::<u16>(),
        positive in any::<bool>(), present in any::<bool>(),
    ) {
        let count = NonZeroUsize::new(n).unwrap();
        let observed = MonetaryCursor::new(revision, i64::from(position) % n as i64, count).unwrap();
        let transition = MonetaryCursorTransition::new([1;32], observed, i64::from(next) % n as i64, count).unwrap();
        prop_assert_eq!(check_transition(present.then_some(&transition), &[1;32], Some(observed), positive, count).is_ok(), positive == present);
        prop_assert!(check_transition(Some(&transition), &[2;32], Some(observed), true, count).is_err());
        prop_assert!(check_transition(Some(&transition), &[1;32], Some(transition.next()), true, count).is_err());
        prop_assert_eq!(check_transition(Some(&transition), &[1;32], None, true, count).is_ok(), observed == MonetaryCursor::INITIAL);
        if n > 1 {
            let other_position = MonetaryCursor::new(revision, (observed.position()+1) % n as i64, count).unwrap();
            prop_assert!(check_transition(Some(&transition), &[1;32], Some(other_position), true, count).is_err());
        }
    }
}

#[test]
fn projection_keeps_full_refunds_and_zero_hold_owners_in_cohort() {
    let payers: BTreeMap<_, _> = (0..5)
        .map(|i| {
            let payer = payer(i);
            (payer.custody_key, payer)
        })
        .collect();
    let keys: Vec<_> = payers.keys().map(|key| key.as_slice()).collect();
    let zero: Vec<_> = keys
        .iter()
        .map(|custody| SettlementRow {
            custody,
            hold: 0,
            acquisition: 0,
            fee: 0,
            refund: 0,
        })
        .collect();
    let all_zero = project_amounts(&payers, &keys, zero.iter().copied(), &budget()).unwrap();
    assert!(all_zero.allocations.is_empty());
    assert!(all_zero.settlements.is_empty());
    assert!(!all_zero.has_resource && !all_zero.has_fee);
    assert_eq!(all_zero.payer_count.get(), 5);
    let mut full_refund = zero.clone();
    full_refund[2].hold = i64::MAX;
    full_refund[2].refund = i64::MAX;
    let projected = project_amounts(&payers, &keys, full_refund.into_iter(), &budget()).unwrap();
    assert_eq!(projected.allocations.len(), 1);
    assert_eq!(projected.allocations[0].amount, i64::MAX);
    assert_eq!(
        (projected.settlements[0].burn, projected.settlements[0].fee),
        (0, 0)
    );
    assert_eq!(projected.payer_count.get(), 5);
    let mut substituted_zero = zero.clone();
    substituted_zero[0].custody = &[255; 32];
    assert!(matches!(
        project_amounts(&payers, &keys, substituted_zero.into_iter(), &budget()),
        Err(DirectWalletSettlementError::CustodyMismatch)
    ));
    let tiny = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    assert!(matches!(
        project_amounts(&payers, &keys, zero.into_iter(), &tiny),
        Err(DirectWalletSettlementError::HostWork(_))
    ));
}

#[test]
fn projection_rejects_missing_rows_overflow_and_inconsistent_amounts() {
    let payer = payer(7);
    let key = payer.custody_key;
    let payers = BTreeMap::from([(key, payer)]);
    let keys = [key.as_slice()];
    let row = SettlementRow {
        custody: &key,
        hold: 5,
        acquisition: 2,
        fee: 1,
        refund: 2,
    };
    assert!(project_amounts(&payers, &keys, std::iter::empty(), &budget()).is_err());
    assert!(project_amounts(&payers, &[], [row].into_iter(), &budget()).is_err());
    assert!(project_amounts(&BTreeMap::new(), &[], std::iter::empty(), &budget()).is_err());
    for invalid in [
        SettlementRow { hold: -1, ..row },
        SettlementRow {
            acquisition: -1,
            ..row
        },
        SettlementRow { fee: -1, ..row },
        SettlementRow { refund: -1, ..row },
        SettlementRow { hold: 0, ..row },
        SettlementRow { refund: 3, ..row },
        SettlementRow {
            hold: i64::MAX,
            acquisition: i64::MAX,
            fee: 1,
            refund: 0,
            ..row
        },
    ] {
        assert!(matches!(
            project_amounts(&payers, &keys, [invalid].into_iter(), &budget()),
            Err(DirectWalletSettlementError::InconsistentAmounts)
        ));
    }
    let mut wrong_payers = payers.clone();
    wrong_payers.get_mut(&key).unwrap().custody_key = [0; 32];
    assert!(matches!(
        project_amounts(&wrong_payers, &keys, [row].into_iter(), &budget()),
        Err(DirectWalletSettlementError::CustodyMismatch)
    ));
}
