use authority::{AuthorityStackBirth, ResourceMultiset};
use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostAuthority, CostSignature};
use proptest::prelude::*;

use super::*;

fn signature(purse: u8) -> CostSignature {
    CostSignature {
        value: Some(Value::Ground(vec![purse])),
    }
}

fn demand(purses: &[u8], cells: usize) -> (CostAuthority, ResourceMultiset<[u8; 32]>) {
    let mut regions = Vec::new();
    let mut amounts = BTreeMap::new();
    for (region, purse) in purses.iter().enumerate() {
        regions.push(
            authority::cost_region(&signature(*purse), &(region as u64).to_le_bytes(), 0).unwrap(),
        );
        *amounts
            .entry(Sig::Ground(vec![*purse]).lane_hash())
            .or_default() += cells as u64;
    }
    (CostAuthority { regions }, ResourceMultiset(amounts))
}

fn sum<'a>(
    entries: impl Iterator<Item = &'a ResourceMultiset<[u8; 32]>>,
) -> ResourceMultiset<[u8; 32]> {
    let mut total = BTreeMap::new();
    for entry in entries {
        for (purse, amount) in &entry.0 {
            *total.entry(*purse).or_default() += amount;
        }
    }
    ResourceMultiset(total)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn mixed_comm_and_transfer_histories_preserve_each_purse(
        capacities in prop::collection::vec(0_u64..33, 1..33),
        commands in prop::collection::vec(
            (0_u8..5, 1_u8..17, 1_usize..5, prop::collection::vec(any::<u8>(), 1..9)),
            1..100,
        ),
    ) {
        let allocation = ResourceMultiset(capacities.iter().enumerate().map(|(purse, amount)| {
            (Sig::Ground(vec![purse as u8]).lane_hash(), *amount)
        }).collect());
        let budget = RuntimeBudget::new(Cost::create(1_000_000, "mixed authority history"));
        let _scope = budget.enter_comm_accounting_scope();
        budget.install_authority_allocation(allocation.clone());
        let mut comms = BTreeMap::<u8, (CostAuthority, ResourceMultiset<[u8; 32]>)>::new();
        let mut pending = BTreeMap::<u8, (AuthorityStackTransferReservation, ResourceMultiset<[u8; 32]>, Vec<CostSignature>)>::new();
        let mut committed = BTreeMap::<u8, (ResourceMultiset<[u8; 32]>, Vec<CostSignature>)>::new();
        for (action, id, count, owners) in commands {
            let owners: Vec<_> = owners.into_iter().map(|purse| purse % capacities.len() as u8).collect();
            let total = sum(comms.values().map(|(_, debit)| debit)
                .chain(pending.values().map(|(_, debit, _)| debit))
                .chain(committed.values().map(|(debit, _)| debit)));
            if action == 0 {
                let (authority, debit) = demand(&owners, 1);
                let accepted = match comms.get(&id) {
                    Some((original, _)) => authority::canonical_authority(original).unwrap()
                        == authority::canonical_authority(&authority).unwrap(),
                    None => allocation.dominates(&sum([&total, &debit].into_iter())),
                };
                let actual = budget.reserve_comm_authority_identity([id; 32], &authority);
                prop_assert_eq!(actual.is_ok(), accepted);
                if accepted {
                    comms.entry(id).or_insert((authority, debit));
                }
            } else if action == 1 {
                let (authority, debit) = demand(&owners, count);
                let cells = vec![signature(id); count];
                let accepted = !pending.contains_key(&id) && !committed.contains_key(&id)
                    && allocation.dominates(&sum([&total, &debit].into_iter()));
                let actual = budget.prepare_authority_stack_transfer([id; 32], cells.clone(), &authority);
                prop_assert_eq!(actual.is_ok(), accepted);
                if let Ok(reservation) = actual {
                    pending.insert(id, (reservation, debit, cells));
                }
            } else if action == 4 {
                pending.clear();
                budget.rollback_authority_stack_transfers().unwrap();
                committed.clear();
            } else if let Some((reservation, debit, cells)) = pending.remove(&id) {
                if action == 2 {
                    reservation.commit();
                    committed.insert(id, (debit, cells));
                } else {
                    drop(reservation);
                }
            }
            let realized = sum(comms.values().map(|(_, debit)| debit)
                .chain(committed.values().map(|(debit, _)| debit)));
            let reserved = sum([&realized].into_iter().chain(pending.values().map(|(_, debit, _)| debit)));
            prop_assert_eq!(budget.authority_realized(), realized.clone());
            prop_assert!(allocation.dominates(&reserved));
            prop_assert_eq!(budget.total_cost().value, comms.len() as i64);
            prop_assert_eq!(budget.authority_stack_births(), committed.iter().map(|(id, (_, cells))| {
                AuthorityStackBirth { produce_hash: [*id; 32], cells: cells.clone() }
            }).collect::<Vec<_>>());
            let state = budget.authority_state.lock().unwrap();
            prop_assert_eq!(&state.reserved, &reserved);
            prop_assert_eq!(&state.realized, &realized);
        }
    }
}

#[test]
fn concurrent_comm_and_transfer_reservations_share_one_capacity_bound() {
    for capacity in 0..=3 {
        for cancel in [false, true] {
            let budget = RuntimeBudget::new(Cost::create(10, "concurrent authority"));
            let _scope = budget.enter_comm_accounting_scope();
            let (authority, single) = demand(&[1], 1);
            let lane = Sig::Ground(vec![1]).lane_hash();
            budget.install_authority_allocation(ResourceMultiset::singleton(lane, capacity));
            let barrier = Arc::new(std::sync::Barrier::new(3));
            let comm = {
                let budget = budget.clone();
                let authority = authority.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let accepted = budget
                        .reserve_comm_authority_identity([1; 32], &authority)
                        .is_ok();
                    barrier.wait();
                    accepted
                })
            };
            let transfer = {
                let budget = budget.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let reservation = budget.prepare_authority_stack_transfer(
                        [2; 32],
                        vec![signature(2); 2],
                        &authority,
                    );
                    let accepted = reservation.is_ok();
                    barrier.wait();
                    if !cancel {
                        if let Ok(reservation) = reservation {
                            reservation.commit();
                        }
                    }
                    accepted
                })
            };
            barrier.wait();
            barrier.wait();
            let comm = comm.join().unwrap();
            let transfer = transfer.join().unwrap();
            assert!(u64::from(comm) + 2 * u64::from(transfer) <= capacity);
            if capacity == 3 {
                assert!(comm && transfer);
            }
            let expected = u64::from(comm) + 2 * u64::from(transfer && !cancel);
            assert_eq!(budget.authority_realized().get(&lane), expected);
            assert_eq!(
                budget.authority_state.lock().unwrap().reserved.get(&lane),
                expected
            );
            assert_eq!(single.get(&lane), 1);
        }
    }
}
