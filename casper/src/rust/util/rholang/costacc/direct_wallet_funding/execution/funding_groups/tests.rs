// Added by DR-134 (DR-65 items 1 to 5, O-8, O-9, O-10): the example and
// property tests of the funding groups of an offered batch.

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;

use super::*;

fn purse(byte: u8) -> [u8; 32] { [byte; 32] }

fn identity(position: usize) -> [u8; 32] {
    let mut identity = [0xA0; 32];
    identity[31] = u8::try_from(position).expect("a test batch has fewer than 256 members");
    identity
}

/// A candidate that holds `holds` on its purses, with the supply of each purse
/// from `supply`.
fn candidate(
    position: usize,
    holds: &[(u8, u128)],
    supply: &BTreeMap<[u8; 32], u128>,
) -> BatchCandidate {
    let holds: Vec<([u8; 32], u128)> = holds
        .iter()
        .map(|(byte, hold)| (purse(*byte), *hold))
        .collect();
    BatchCandidate {
        identity: identity(position),
        custodies: holds.iter().map(|(custody, _)| *custody).collect(),
        supply: holds
            .iter()
            .map(|(custody, _)| (*custody, supply.get(custody).copied().unwrap_or(0)))
            .collect(),
        holds,
        grants: Vec::new(),
    }
}

/// A random batch: up to six candidates over four purses, each holding one or
/// two distinct purses, with one supply for each purse.
fn batch() -> impl Strategy<Value = Vec<BatchCandidate>> {
    (
        proptest::collection::vec(0u128..12, 4),
        proptest::collection::vec(
            proptest::collection::btree_map(0u8..4, 0u128..8, 1..3),
            0..6,
        ),
    )
        .prop_map(|(supplies, rows)| {
            let supply: BTreeMap<[u8; 32], u128> = supplies
                .into_iter()
                .enumerate()
                .map(|(byte, amount)| (purse(u8::try_from(byte).expect("four purses")), amount))
                .collect();
            rows.into_iter()
                .enumerate()
                .map(|(position, row)| {
                    let holds: Vec<(u8, u128)> = row.into_iter().collect();
                    candidate(position, &holds, &supply)
                })
                .collect()
        })
}

/// Added by G3 (DR-135, S8): a random batch for truncation condition (c): two
/// to six candidates over six purses, each holding one or two distinct
/// purses. The supplies are large, so most groups fit, and a dropped member
/// often holds a purse that no shipped member holds.
fn owner_batch() -> impl Strategy<Value = Vec<BatchCandidate>> {
    (
        proptest::collection::vec(8u128..24, 6),
        proptest::collection::vec(
            proptest::collection::btree_map(0u8..6, 0u128..8, 1..3),
            2..7,
        ),
    )
        .prop_map(|(supplies, rows)| {
            let supply: BTreeMap<[u8; 32], u128> = supplies
                .into_iter()
                .enumerate()
                .map(|(byte, amount)| (purse(u8::try_from(byte).expect("six purses")), amount))
                .collect();
            rows.into_iter()
                .enumerate()
                .map(|(position, row)| {
                    let holds: Vec<(u8, u128)> = row.into_iter().collect();
                    candidate(position, &holds, &supply)
                })
                .collect()
        })
}

/// The partition of a decision as sets of identity sets, and its rejected
/// identities, which a permutation of the batch must not change.
fn decision_shape(
    candidates: &[BatchCandidate],
    decision: &GroupDecision,
) -> (BTreeSet<BTreeSet<[u8; 32]>>, BTreeSet<[u8; 32]>) {
    let mut groups: BTreeMap<usize, BTreeSet<[u8; 32]>> = BTreeMap::new();
    for (position, group) in decision.group_of.iter().enumerate() {
        groups
            .entry(*group)
            .or_default()
            .insert(candidates[position].identity);
    }
    let rejected = decision
        .rejected
        .iter()
        .flat_map(|group| group.members.iter())
        .map(|member| candidates[*member].identity)
        .collect();
    (groups.into_values().collect(), rejected)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    // GroupAllOrNone (DR-65 item 4): the members of one group are all accepted
    // or all rejected.
    #[test]
    fn group_all_or_none(candidates in batch()) {
        let decision = decide_groups(&candidates).expect("a consistent batch decides");
        let accepted: BTreeSet<usize> = decision.accepted.iter().copied().collect();
        let mut verdict: BTreeMap<usize, bool> = BTreeMap::new();
        for (position, group) in decision.group_of.iter().enumerate() {
            let is_accepted = accepted.contains(&position);
            if let Some(previous) = verdict.insert(*group, is_accepted) {
                prop_assert_eq!(previous, is_accepted);
            }
        }
    }

    // NoOverdraft at R0: the accepted members of each group fit the supply of
    // every purse that they hold.
    #[test]
    fn accepted_groups_fit_their_supply(candidates in batch()) {
        let decision = decide_groups(&candidates).expect("a consistent batch decides");
        let mut totals: BTreeMap<[u8; 32], u128> = BTreeMap::new();
        let mut supply: BTreeMap<[u8; 32], u128> = BTreeMap::new();
        for member in &decision.accepted {
            for (custody, hold) in &candidates[*member].holds {
                *totals.entry(*custody).or_insert(0) += *hold;
            }
            supply.extend(candidates[*member].supply.iter().copied());
        }
        for (custody, total) in totals {
            prop_assert!(total <= supply[&custody]);
        }
    }

    // DecisionPermutationInvariant: the partition and the rejected members do
    // not depend on the order of the batch.
    #[test]
    fn decision_is_invariant_under_permutation(
        candidates in batch(),
        seed in any::<u64>(),
    ) {
        let decision = decide_groups(&candidates).expect("a consistent batch decides");
        let mut permuted = candidates.clone();
        let mut state = seed;
        for index in (1..permuted.len()).rev() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let other = usize::try_from(state >> 33).expect("a shifted u64 fits") % (index + 1);
            permuted.swap(index, other);
        }
        let permuted_decision = decide_groups(&permuted).expect("a consistent batch decides");
        prop_assert_eq!(
            decision_shape(&candidates, &decision),
            decision_shape(&permuted, &permuted_decision)
        );
    }

    // The later holds of member i are the suffix sums of the holds of the
    // members after it, with zero sums left out.
    #[test]
    fn later_holds_are_suffix_sums(candidates in batch()) {
        let members: Vec<usize> = (0..candidates.len()).collect();
        let later = later_holds(&candidates, &members).expect("small holds never overflow");
        prop_assert_eq!(later.len(), members.len());
        for (index, holds) in later.iter().enumerate() {
            let mut expected: BTreeMap<[u8; 32], u128> = BTreeMap::new();
            for member in &members[index + 1..] {
                for (custody, hold) in &candidates[*member].holds {
                    if *hold > 0 {
                        *expected.entry(*custody).or_insert(0) += *hold;
                    }
                }
            }
            prop_assert_eq!(holds, &expected);
        }
    }

    // TruncationConsistency (O-10): a shipped prefix needs no re-run exactly
    // when no group straddles the cut and no shipped member rolled back.
    #[test]
    fn truncation_reruns_exactly_on_a_straddle_or_a_guard_rollback(
        candidates in batch(),
        cut in 0usize..7,
        rollbacks in proptest::collection::vec(any::<bool>(), 6),
    ) {
        let decision = decide_groups(&candidates).expect("a consistent batch decides");
        let shipped = cut.min(decision.accepted.len());
        // Changed by G3 (DR-135, S8): the rule takes the candidates and the
        // owner purses of the shipped members, here none.
        // let rerun = rerun_after_truncation(&decision, shipped, &rollbacks)
        let rerun = rerun_after_truncation(&decision, shipped, &rollbacks, &candidates, &[])
            .expect("the cut is inside the accepted members");
        let (kept, dropped) = decision.accepted.split_at(shipped);
        let straddles = kept.iter().any(|kept_member| {
            dropped
                .iter()
                .any(|dropped_member| decision.group_of[*kept_member] == decision.group_of[*dropped_member])
        });
        let rolled_back = rollbacks[..shipped].iter().any(|failed| *failed);
        prop_assert_eq!(rerun, straddles || rolled_back);
    }

    // Added by G3 (DR-135, S8): TruncationConsistency with owner purses. A
    // shipped prefix also runs again exactly when a dropped member holds a
    // positive hold on a purse that the owner table of a shipped member lists
    // (condition (c)). The batch of `owner_batch` keeps most members, the cut
    // ships at least one member, and a rollback is rare, so condition (c)
    // often decides alone. The owner purses range over the six purses of the
    // batch and two that no candidate holds.
    #[test]
    fn truncation_reruns_when_a_dropped_member_holds_a_shipped_owner_purse(
        candidates in owner_batch(),
        cut in 1usize..7,
        rollbacks in proptest::collection::vec(proptest::bool::weighted(0.2), 6),
        owners in proptest::collection::vec(proptest::collection::vec(0u8..8, 0..4), 6),
    ) {
        let decision = decide_groups(&candidates).expect("a consistent batch decides");
        let shipped = cut.min(decision.accepted.len());
        let owner_purses: Vec<Vec<[u8; 32]>> = owners[..shipped]
            .iter()
            .map(|row| row.iter().map(|byte| purse(*byte)).collect())
            .collect();
        let rerun =
            rerun_after_truncation(&decision, shipped, &rollbacks, &candidates, &owner_purses)
                .expect("the cut is inside the accepted members");
        let (kept, dropped) = decision.accepted.split_at(shipped);
        let straddles = kept.iter().any(|kept_member| {
            dropped
                .iter()
                .any(|dropped_member| decision.group_of[*kept_member] == decision.group_of[*dropped_member])
        });
        let rolled_back = rollbacks[..shipped].iter().any(|failed| *failed);
        let holds_owner_purse = dropped.iter().any(|member| {
            candidates[*member].holds.iter().any(|(held, hold)| {
                *hold > 0 && owner_purses.iter().flatten().any(|owner| owner == held)
            })
        });
        prop_assert_eq!(rerun, straddles || rolled_back || holds_owner_purse);
    }
}

/// Over supply: two members hold 6 each on a purse with supply 10. Each fits
/// alone, and the group does not, so both are rejected.
#[test]
fn an_over_supply_group_is_rejected_entirely() {
    let supply = BTreeMap::from([(purse(1), 10)]);
    let candidates = vec![
        candidate(0, &[(1, 6)], &supply),
        candidate(1, &[(1, 6)], &supply),
    ];
    let decision = decide_groups(&candidates).expect("a consistent batch decides");
    assert!(decision.accepted.is_empty());
    assert_eq!(decision.rejected.len(), 1);
    assert_eq!(decision.rejected[0].members, vec![0, 1]);
    assert_eq!(decision.rejected[0].cause, GroupRejection::Purse {
        purse: purse(1),
        holds: 12,
        supply: 10,
    });
    assert!(decision.rejected[0]
        .cause
        .to_string()
        .starts_with("insufficient phlogiston"));
}

/// Negative control (prefix admission): admitting members in order while they
/// fit would admit the first member of the over-supply group and split it.
/// The decision rejects the group as a whole.
#[test]
fn prefix_admission_would_split_the_group_that_the_decision_keeps_whole() {
    let supply = BTreeMap::from([(purse(1), 10)]);
    let candidates = vec![
        candidate(0, &[(1, 6)], &supply),
        candidate(1, &[(1, 6)], &supply),
    ];
    let mut remaining = 10u128;
    let prefix: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter_map(|(position, candidate)| {
            let hold = candidate.holds[0].1;
            match hold <= remaining {
                true => {
                    remaining -= hold;
                    Some(position)
                }
                false => None,
            }
        })
        .collect();
    assert_eq!(prefix, vec![0], "prefix admission splits the group");
    let decision = decide_groups(&candidates).expect("a consistent batch decides");
    assert!(decision.accepted.is_empty(), "the group is all or none");
}

/// Members that share no purse form separate groups, and the over-supply of
/// one group leaves the other accepted.
#[test]
fn an_independent_group_stays_accepted() {
    let supply = BTreeMap::from([(purse(1), 10), (purse(2), 10)]);
    let candidates = vec![
        candidate(0, &[(1, 6)], &supply),
        candidate(1, &[(2, 4)], &supply),
        candidate(2, &[(1, 6)], &supply),
    ];
    let decision = decide_groups(&candidates).expect("a consistent batch decides");
    assert_eq!(decision.group_of, vec![0, 1, 0]);
    assert_eq!(decision.accepted, vec![1]);
    assert_eq!(decision.rejected[0].members, vec![0, 2]);
}

/// O-8: two members draw one grant. Their holds must fit its remaining
/// ceiling at R0, and both are rejected when they do not.
#[test]
fn two_draws_on_one_grant_must_fit_its_remaining_ceiling() {
    let supply = BTreeMap::from([(purse(1), 100), (purse(2), 100)]);
    let mut first = candidate(0, &[(1, 5)], &supply);
    let mut second = candidate(1, &[(2, 5)], &supply);
    first.grants = vec![GrantDraw {
        grant: b"grant".to_vec(),
        hold: 5,
        remaining: 9,
    }];
    second.grants = vec![GrantDraw {
        grant: b"grant".to_vec(),
        hold: 5,
        remaining: 9,
    }];
    let decision =
        decide_groups(&[first.clone(), second.clone()]).expect("a consistent batch decides");
    assert!(decision.accepted.is_empty());
    assert_eq!(decision.rejected[0].cause, GroupRejection::Grant {
        grant: b"grant".to_vec(),
        holds: 10,
        remaining: 9,
    });
    first.grants[0].remaining = 10;
    second.grants[0].remaining = 10;
    let decision = decide_groups(&[first, second]).expect("a consistent batch decides");
    assert_eq!(decision.accepted, vec![0, 1]);
}

/// O-8: a single draw on a grant is not checked here. The settlement of the
/// draw checks its own remaining ceiling, as before G2.
#[test]
fn a_single_grant_draw_is_not_checked_by_the_group() {
    let supply = BTreeMap::from([(purse(1), 100)]);
    let mut only = candidate(0, &[(1, 5)], &supply);
    only.grants = vec![GrantDraw {
        grant: b"grant".to_vec(),
        hold: 50,
        remaining: 9,
    }];
    let decision = decide_groups(&[only]).expect("a consistent batch decides");
    assert_eq!(decision.accepted, vec![0]);
}

/// Two candidates that report different supplies for one purse are a defect
/// of the caller, because both read the same balance at R0.
#[test]
fn disagreeing_supplies_are_a_caller_defect() {
    let low = BTreeMap::from([(purse(1), 10)]);
    let high = BTreeMap::from([(purse(1), 11)]);
    let candidates = vec![
        candidate(0, &[(1, 1)], &low),
        candidate(1, &[(1, 1)], &high),
    ];
    assert_eq!(
        decide_groups(&candidates),
        Err(FundingGroupError::SupplyDisagrees { purse: purse(1) })
    );
}

/// O-9: the canonical order is strictly increasing in (valid-after block,
/// timestamp, identity). A repeated or swapped member fails at its position.
#[test]
fn the_canonical_order_is_strictly_increasing() {
    let key = |valid_after, time_stamp, byte| CanonicalKey {
        valid_after_block_number: valid_after,
        time_stamp,
        identity: [byte; 32],
    };
    assert_eq!(
        check_canonical_order(&[key(0, 1, 1), key(0, 1, 2), key(0, 2, 0), key(1, 0, 0)]),
        Ok(())
    );
    assert_eq!(
        check_canonical_order(&[key(0, 1, 1), key(0, 1, 1)]),
        Err(FundingGroupError::NonCanonicalOrder { position: 1 })
    );
    assert_eq!(
        check_canonical_order(&[key(0, 1, 1), key(0, 2, 0), key(0, 1, 9)]),
        Err(FundingGroupError::NonCanonicalOrder { position: 2 })
    );
}

/// O-10: the shipped prefix runs again when a group straddles the cut or a
/// shipped member rolled back its guard, and not otherwise.
#[test]
fn truncation_examples() {
    let supply = BTreeMap::from([(purse(1), 100), (purse(2), 100)]);
    let candidates = vec![
        candidate(0, &[(1, 5)], &supply),
        candidate(1, &[(2, 5)], &supply),
        candidate(2, &[(1, 5)], &supply),
    ];
    let decision = decide_groups(&candidates).expect("a consistent batch decides");
    assert_eq!(decision.accepted, vec![0, 1, 2]);
    // Changed by G3 (DR-135, S8): each call also passes the candidates and no
    // owner purses.
    // Members 0 and 2 share purse 1: a cut after member 1 splits their group.
    // assert_eq!(
    //     rerun_after_truncation(&decision, 2, &[false, false, false]),
    //     Ok(true)
    // );
    assert_eq!(
        rerun_after_truncation(&decision, 2, &[false, false, false], &candidates, &[]),
        Ok(true)
    );
    // A cut after member 0 also splits it.
    // assert_eq!(
    //     rerun_after_truncation(&decision, 1, &[false, false, false]),
    //     Ok(true)
    // );
    assert_eq!(
        rerun_after_truncation(&decision, 1, &[false, false, false], &candidates, &[]),
        Ok(true)
    );
    // A batch without a straddle: members 0 and 1 share nothing with 2.
    let independent = vec![
        candidate(0, &[(1, 5)], &supply),
        candidate(1, &[(1, 5)], &supply),
        candidate(2, &[(2, 5)], &supply),
    ];
    let decision = decide_groups(&independent).expect("a consistent batch decides");
    // Changed by G3 (DR-135, S8): each call also passes the candidates and no
    // owner purses.
    // assert_eq!(
    //     rerun_after_truncation(&decision, 2, &[false, false, false]),
    //     Ok(false)
    // );
    assert_eq!(
        rerun_after_truncation(&decision, 2, &[false, false, false], &independent, &[]),
        Ok(false)
    );
    // The same cut runs again when a shipped member rolled back its guard.
    // assert_eq!(
    //     rerun_after_truncation(&decision, 2, &[false, true, false]),
    //     Ok(true)
    // );
    assert_eq!(
        rerun_after_truncation(&decision, 2, &[false, true, false], &independent, &[]),
        Ok(true)
    );
    // A cut past the accepted members is a caller defect.
    // assert_eq!(
    //     rerun_after_truncation(&decision, 4, &[false; 4]),
    //     Err(FundingGroupError::MemberOutOfRange)
    // );
    assert_eq!(
        rerun_after_truncation(&decision, 4, &[false; 4], &independent, &[]),
        Err(FundingGroupError::MemberOutOfRange)
    );
}
