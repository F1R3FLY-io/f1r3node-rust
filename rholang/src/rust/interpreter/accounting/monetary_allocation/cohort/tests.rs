use models::rhoapi::cost_signature::Value;
use proptest::prelude::*;

use super::*;
use crate::rust::interpreter::accounting::authority::physicalize_balance_debit;

mod contribution;

fn fixture(
    custodies: &[u8],
    amounts: &[u64],
) -> (
    BTreeMap<[u8; 32], CostSignature>,
    AuthorityPhysicalInventory,
) {
    let mut eligible = BTreeMap::new();
    let mut inventory = AuthorityPhysicalInventory::default();
    for (index, custody) in custodies.iter().enumerate() {
        let signature = CostSignature {
            value: Some(Value::Ground((index as u64).to_le_bytes().to_vec())),
        };
        let lane = cost_signature_to_sig(&signature).unwrap().lane_hash();
        eligible.insert(lane, signature);
        inventory
            .insert_balance_lane(lane, [*custody; 32], amounts[usize::from(*custody)])
            .unwrap();
    }
    (eligible, inventory)
}

fn cohort(
    eligible: &BTreeMap<[u8; 32], CostSignature>,
    inventory: &AuthorityPhysicalInventory,
    limit: usize,
) -> MonetaryCohort {
    MonetaryCohort::from_inventory(eligible, inventory, NonZeroUsize::new(limit).unwrap()).unwrap()
}

#[test]
fn monetary_cohort_keeps_zero_balances_and_collapses_alias_positions() {
    let (eligible, inventory) = fixture(&[2, 0, 1, 0], &[5, 0, 5]);
    let payers = cohort(&eligible, &inventory, 3);
    assert_eq!(payers.payers().len(), 3);
    assert_eq!(payers.payers()[0].logical_lanes().len(), 2);
    assert_eq!(payers.payers()[1].custody(), &[1; 32]);
    let plan = payers.allocate(&inventory.balances, 2, 0).unwrap();
    assert_eq!(plan.settlement.custody_debit.get(&[0; 32]), 1);
    assert_eq!(plan.settlement.custody_debit.get(&[1; 32]), 0);
    assert_eq!(plan.settlement.custody_debit.get(&[2; 32]), 1);
    assert_eq!(
        physicalize_balance_debit(&plan.settlement.logical_debit, &inventory.balance_custody)
            .unwrap(),
        plan.settlement.custody_debit
    );
}

#[test]
fn monetary_cohort_scope_does_not_depend_on_balance_or_alias_count() {
    let (eligible, inventory) = fixture(&[0, 1], &[0, 5]);
    let (aliases, topped_up) = fixture(&[0, 1, 0, 1], &[8, 0]);
    let first = cohort(&eligible, &inventory, 2);
    let second = cohort(&aliases, &topped_up, 2);
    assert_eq!(first.scope_id(&[1; 32]), second.scope_id(&[1; 32]));
    assert_ne!(first.scope_id(&[1; 32]), first.scope_id(&[2; 32]));
}

#[test]
fn monetary_cohort_plan_binds_custody_scope_and_checked_revision() {
    let (eligible, inventory) = fixture(&[0, 1, 2], &[7, 7, 7]);
    let payers = cohort(&eligible, &inventory, 3);
    let context = [7; 32];
    let cursor = MonetaryCursor::new(11, 2, NonZeroUsize::new(3).unwrap()).unwrap();
    let plan = payers
        .plan(&inventory.balances, 1, &context, cursor)
        .unwrap();
    assert_eq!(plan.settlement.custody_debit.get(&[2; 32]), 1);
    assert_eq!(plan.settlement.custody_debit.0.len(), 1);
    assert_eq!(plan.cursor_transition.scope(), &payers.scope_id(&context));
    assert_eq!(plan.cursor_transition.expected(), cursor);
    assert_eq!(plan.cursor_transition.next().revision(), 12);
    assert_eq!(plan.cursor_transition.next().position(), 0);
    assert_eq!(
        plan.cursor_transition.checked_successor(
            &payers.scope_id(&[8; 32]),
            cursor,
            NonZeroUsize::new(3).unwrap(),
        ),
        Err(MonetaryCursorError::ScopeMismatch),
    );
}

#[test]
fn monetary_cohort_exhausted_revision_rejects_before_allocation() {
    let (eligible, inventory) = fixture(&[0], &[0]);
    let payers = cohort(&eligible, &inventory, 1);
    let cursor = MonetaryCursor::new(i64::MAX, 0, NonZeroUsize::new(1).unwrap()).unwrap();
    assert_eq!(
        payers.plan(&inventory.balances, 1, &[9; 32], cursor),
        Err(MonetaryCohortError::Cursor(
            MonetaryCursorError::RevisionExhausted
        )),
    );
}

#[test]
fn monetary_cohort_excludes_unrelated_inventory_even_when_funded() {
    let (eligible, mut inventory) = fixture(&[0], &[0]);
    inventory.balances.0.insert([99; 32], u64::MAX);
    let payers = cohort(&eligible, &inventory, 1);
    assert_eq!(payers.payers().len(), 1);
    assert_eq!(
        payers.allocate(&inventory.balances, 1, 0),
        Err(MonetaryCohortError::Allocation(
            MonetaryAllocationError::InsufficientCapacity
        ))
    );
}

#[test]
fn monetary_cohort_rejects_empty_and_unit_authority() {
    let inventory = AuthorityPhysicalInventory::default();
    let limit = NonZeroUsize::new(1).unwrap();
    assert_eq!(
        MonetaryCohort::from_inventory(&BTreeMap::new(), &inventory, limit),
        Err(MonetaryCohortError::Allocation(
            MonetaryAllocationError::EmptyPayers
        ))
    );
    let unit = CostSignature {
        value: Some(Value::Unit(true)),
    };
    let lane = cost_signature_to_sig(&unit).unwrap().lane_hash();
    let eligible = BTreeMap::from([(lane, unit)]);
    assert_eq!(
        MonetaryCohort::from_inventory(&eligible, &inventory, limit),
        Err(MonetaryCohortError::Authority(
            AuthorityError::EventSignatureConflict
        ))
    );
}

#[test]
fn monetary_cohort_new_zero_balance_custody_changes_the_scope() {
    let (eligible, inventory) = fixture(&[0], &[5]);
    let (expanded, expanded_inventory) = fixture(&[0, 1], &[5, 0]);
    let initial = cohort(&eligible, &inventory, 2);
    let next = cohort(&expanded, &expanded_inventory, 2);
    assert_ne!(initial.scope_id(&[1; 32]), next.scope_id(&[1; 32]));
}

#[test]
fn monetary_cohort_rejects_unbound_or_mismatched_lanes() {
    let (mut eligible, mut inventory) = fixture(&[0], &[5]);
    let lane = *eligible.keys().next().unwrap();
    inventory.balance_custody.clear();
    assert_eq!(
        MonetaryCohort::from_inventory(&eligible, &inventory, NonZeroUsize::new(1).unwrap()),
        Err(MonetaryCohortError::Authority(
            AuthorityError::UnknownPhysicalCustody
        ))
    );
    eligible.insert(lane, CostSignature {
        value: Some(Value::Ground(vec![99])),
    });
    assert_eq!(
        MonetaryCohort::from_inventory(&eligible, &inventory, NonZeroUsize::new(1).unwrap()),
        Err(MonetaryCohortError::Authority(
            AuthorityError::EventSignatureConflict
        ))
    );
}

#[test]
fn monetary_cohort_cap_counts_distinct_purses_not_signer_aliases() {
    let (eligible, inventory) = fixture(&[0, 0, 1], &[3, 3]);
    assert_eq!(cohort(&eligible, &inventory, 2).payers().len(), 2);
    assert_eq!(
        MonetaryCohort::from_inventory(&eligible, &inventory, NonZeroUsize::new(1).unwrap()),
        Err(MonetaryCohortError::Allocation(
            MonetaryAllocationError::TooManyPayers
        ))
    );
    let (eligible, inventory) = fixture(&(0..65).collect::<Vec<_>>(), &[1; 65]);
    let payers = cohort(&eligible, &inventory, 65);
    let plan = payers.allocate(&inventory.balances, 65, 0).unwrap();
    assert_eq!(plan.settlement.custody_debit.0.len(), 65);
}

#[test]
fn monetary_reservations_cannot_reuse_shared_physical_capacity() {
    let (eligible, inventory) = fixture(&[0, 0, 0], &[10]);
    let payers = cohort(&eligible, &inventory, 64);
    let first = payers.allocate(&inventory.balances, 6, 0).unwrap();
    let stale_second = payers.allocate(&inventory.balances, 6, 0).unwrap();
    let combined = first
        .settlement
        .custody_debit
        .checked_add(&stale_second.settlement.custody_debit)
        .unwrap();
    assert_eq!(combined.get(&[0; 32]), 12);
    assert!(!inventory.balances.dominates(&combined));
    assert_eq!(
        inventory.balances.checked_sub(&combined),
        Err(AuthorityError::InsufficientAuthority)
    );
    let remaining = inventory
        .balances
        .checked_sub(&first.settlement.custody_debit)
        .unwrap();
    assert_eq!(
        payers.allocate(&remaining, 6, 0),
        Err(MonetaryCohortError::Allocation(
            MonetaryAllocationError::InsufficientCapacity
        ))
    );
    assert_eq!(inventory.balances.get(&[0; 32]), 10);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn monetary_reservation_refunds_preserve_original_custody(
        bindings in prop::collection::vec(0_u8..8, 1..65),
        amounts in prop::array::uniform8(0_u64..1000),
        requested in 0_u64..8000, realized in 0_u64..8000,
        cursor_seed in any::<usize>(),
    ) {
        let (eligible, inventory) = fixture(&bindings, &amounts);
        let payers = cohort(&eligible, &inventory, 64);
        let total = payers.payers().iter().map(|payer| inventory.balances.get(payer.custody())).sum();
        let maximum = requested.min(total);
        let actual = realized.min(maximum);
        let cursor = cursor_seed % payers.payers().len();
        let reserved = payers.allocate(&inventory.balances, maximum, cursor).unwrap();
        let settled = payers.allocate(&reserved.settlement.custody_debit, actual, cursor).unwrap();
        let refund = reserved.settlement.custody_debit.checked_sub(&settled.settlement.custody_debit).unwrap();
        let remaining = inventory.balances.checked_sub(&reserved.settlement.custody_debit).unwrap();
        let after_refund = remaining.checked_add(&refund).unwrap();
        prop_assert_eq!(after_refund.checked_add(&settled.settlement.custody_debit).unwrap(), inventory.balances.clone());
        prop_assert_eq!(refund.0.values().sum::<u64>() + actual, maximum);
        prop_assert_eq!(
            physicalize_balance_debit(&settled.settlement.logical_debit, &inventory.balance_custody).unwrap(),
            settled.settlement.custody_debit
        );
    }

    #[test]
    fn monetary_cohort_generated_aliases_preserve_physical_conservation(
        bindings in prop::collection::vec(0_u8..8, 1..65),
        amounts in prop::array::uniform8(0_u64..1000),
        requested in 0_u64..8000, cursor_seed in any::<usize>(),
        revision in 0_i64..i64::MAX,
        context in prop::array::uniform32(any::<u8>()),
    ) {
        let (eligible, inventory) = fixture(&bindings, &amounts);
        let payers = cohort(&eligible, &inventory, 64);
        let distinct: BTreeSet<_> = bindings.iter().copied().collect();
        prop_assert_eq!(payers.payers().len(), distinct.len());
        let available: u64 = distinct.iter().map(|key| amounts[usize::from(*key)]).sum();
        let obligation = requested.min(available);
        let plan = payers.allocate(&inventory.balances, obligation, cursor_seed % distinct.len()).unwrap();
        let count = NonZeroUsize::new(distinct.len()).unwrap();
        let cursor = MonetaryCursor::new(revision, (cursor_seed % distinct.len()) as i64, count).unwrap();
        let scoped = payers.plan(&inventory.balances, obligation, &context, cursor).unwrap();
        prop_assert_eq!(scoped.settlement(), &plan.settlement);
        prop_assert_eq!(scoped.cursor_transition().scope(), &payers.scope_id(&context));
        prop_assert_eq!(scoped.cursor_transition().expected(), cursor);
        let next = scoped.cursor_transition().checked_successor(&payers.scope_id(&context), cursor, count).unwrap();
        prop_assert_eq!(next.revision(), revision + 1);
        prop_assert_eq!(next.position(), plan.next_cursor as i64);
        prop_assert_eq!(plan.settlement.logical_debit.0.values().sum::<u64>(), obligation);
        prop_assert_eq!(plan.settlement.custody_debit.0.values().sum::<u64>(), obligation);
        prop_assert_eq!(physicalize_balance_debit(&plan.settlement.logical_debit, &inventory.balance_custody).unwrap(), plan.settlement.custody_debit.clone());
        for (key, amount) in &plan.settlement.custody_debit.0 {
            prop_assert!(*amount <= inventory.balances.get(key));
        }
    }
}
