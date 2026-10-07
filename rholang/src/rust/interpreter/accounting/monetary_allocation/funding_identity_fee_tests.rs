use std::num::NonZeroUsize;

use models::rust::host_work::{HostWorkLimit, HostWorkLimits};

use super::*;

fn work() -> HostWorkBudget {
    HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)))
}

#[test]
fn named_fee_permissions_and_both_cursors_survive_source_permutations() {
    let limits = FundingSearchLimits {
        source_cap: NonZeroUsize::new(3).unwrap(),
        obligation_cap: NonZeroUsize::new(2).unwrap(),
    };
    let keys: [&[u8]; 3] = [b"A", b"B", b"C"];
    let mut reference = None;
    for order in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [
        2, 1, 0,
    ]] {
        for columns in [[0, 1], [1, 0]] {
            let source_keys: Vec<_> = order.iter().map(|i| keys[*i]).collect();
            let obligation_keys: Vec<&[u8]> = columns
                .iter()
                .map(|i| if *i == 0 { &b"x"[..] } else { &b"y"[..] })
                .collect();
            let edges = vec![vec![true; 2]; 3];
            let fee: Vec<_> = order.iter().map(|i| *i == 0).collect();
            let canonical = canonicalize_funding_problem(
                FundingMinimaxProblem {
                    capacities: &[1, 1, 1],
                    obligations: &[1, 1],
                    eligible: &edges,
                },
                &source_keys,
                &obligation_keys,
                limits,
                &work(),
            )
            .unwrap();
            let selected = canonical
                .select_with_unit_fee(&fee, 0, 2, limits, &work())
                .unwrap()
                .unwrap();
            if let Some(prior) = &reference {
                assert_eq!(&selected, prior);
            } else {
                reference = Some(selected.clone());
            }
            let assignment = canonical
                .restore_assignment(selected.resource_assignment(), &work())
                .unwrap();
            let mut fee_debits = vec![0; 3];
            for (i, original) in canonical.original_source_positions().iter().enumerate() {
                fee_debits[*original] = selected.fee().debits[i];
            }
            let proposal = FundingFeeProposal {
                resource_assignment: &assignment,
                fee_debits: &fee_debits,
                resource_next_cursor: selected.resource_next_cursor(),
                fee_next_cursor: selected.fee().next_cursor,
            };
            assert!(canonical
                .verify_with_unit_fee(&fee, 0, 2, proposal, limits, &work())
                .unwrap());
            assert!(!canonical
                .verify_with_unit_fee(
                    &fee,
                    0,
                    2,
                    FundingFeeProposal {
                        fee_next_cursor: (selected.fee().next_cursor + 1) % 3,
                        ..proposal
                    },
                    limits,
                    &work()
                )
                .unwrap());
            assert!(!canonical
                .verify_with_unit_fee(
                    &fee,
                    0,
                    2,
                    FundingFeeProposal {
                        resource_next_cursor: None,
                        ..proposal
                    },
                    limits,
                    &work()
                )
                .unwrap());
            for source in 0..3 {
                let mut changed_fee = fee_debits.clone();
                changed_fee[source] += 1;
                assert!(!canonical
                    .verify_with_unit_fee(
                        &fee,
                        0,
                        2,
                        FundingFeeProposal {
                            fee_debits: &changed_fee,
                            ..proposal
                        },
                        limits,
                        &work()
                    )
                    .unwrap());
                for target in 0..2 {
                    let mut changed = assignment.clone();
                    changed[source][target] += 1;
                    assert!(!canonical
                        .verify_with_unit_fee(
                            &fee,
                            0,
                            2,
                            FundingFeeProposal {
                                resource_assignment: &changed,
                                ..proposal
                            },
                            limits,
                            &work()
                        )
                        .unwrap());
                }
            }
            assert!(!canonical
                .verify_with_unit_fee(&[false; 3], 0, 2, proposal, limits, &work())
                .unwrap());
        }
    }
}
