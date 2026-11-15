// No ejection commits without a 2/3-stake witness.
// The tally decides every claim family alike, so the property is stated on it.

use std::collections::HashSet;

use casper::rust::economics::claims::tally;
use models::rust::casper::protocol::casper_message::{Bond, ClaimOutcome};
use models::rust::validator::Validator;
use proptest::prelude::*;

fn validator(index: usize) -> Validator { Validator::from(vec![index as u8; 33]) }

#[derive(Clone, Copy, Debug)]
struct Member {
    stake: u64,
    vote: Option<bool>,
    slashed: bool,
    seated: bool,
}

/// An electorate whose members each hold a vote or none, may be slashed, and
/// may have left the committee. Small stakes make an exact two-thirds split,
/// the boundary, common.
fn electorate() -> impl Strategy<Value = Vec<Member>> {
    prop::collection::vec(
        (
            prop_oneof![1u64..4, 1u64..1_000_000],
            prop::option::of(any::<bool>()),
            any::<bool>(),
            any::<bool>(),
        )
            .prop_map(|(stake, vote, slashed, seated)| Member {
                stake,
                vote,
                slashed,
                seated,
            }),
        1..16,
    )
}

fn decide(members: &[Member]) -> Option<ClaimOutcome> {
    let bonds: Vec<Bond> = members
        .iter()
        .enumerate()
        .map(|(index, member)| Bond {
            validator: validator(index),
            stake: member.stake.into(),
        })
        .collect();
    let with = |flag: fn(&Member) -> bool| -> HashSet<Validator> {
        members
            .iter()
            .enumerate()
            .filter(|(_, member)| flag(member))
            .map(|(index, _)| validator(index))
            .collect()
    };
    tally(&bonds, &with(|m| m.slashed), &with(|m| m.seated), |voter| {
        members[voter[0] as usize].vote
    })
}

/// The stake of the members that count, unslashed and either voting or
/// seated, and of those among them voting yea and nay.
fn stakes(members: &[Member]) -> (i128, i128, i128) {
    let counted = members
        .iter()
        .filter(|m| !m.slashed && (m.vote.is_some() || m.seated));
    let total = counted.clone().map(|m| m.stake as i128).sum();
    let with = |vote: bool| {
        counted
            .clone()
            .filter(|m| m.vote == Some(vote))
            .map(|m| m.stake as i128)
            .sum()
    };
    (total, with(true), with(false))
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1024,
        .. ProptestConfig::default()
    })]

    /// A pass has more than two thirds of the counted stake behind it.
    #[test]
    fn no_claim_passes_without_a_two_thirds_witness(members in electorate()) {
        let (total, yea, _) = stakes(&members);
        if decide(&members) == Some(ClaimOutcome::Pass) {
            prop_assert!(3 * yea > 2 * total);
        }
    }

    /// An honest third of the counted stake that withholds its yea blocks the
    /// claim, whatever the rest does.
    #[test]
    fn a_third_withholding_its_yea_blocks_a_pass(members in electorate()) {
        let (total, yea, _) = stakes(&members);
        if 3 * (total - yea) >= total {
            prop_assert_ne!(decide(&members), Some(ClaimOutcome::Pass));
        }
    }

    /// A slashed member's stake and vote count for nothing.
    #[test]
    fn a_slashed_member_counts_for_nothing(members in electorate()) {
        let unslashed: Vec<Member> = members.iter().filter(|m| !m.slashed).copied().collect();
        prop_assume!(!unslashed.is_empty());
        prop_assert_eq!(decide(&members), decide(&unslashed));
    }

    /// A member that left the committee without voting counts for nothing.
    #[test]
    fn a_member_that_left_without_voting_counts_for_nothing(members in electorate()) {
        let remaining: Vec<Member> = members
            .iter()
            .filter(|m| m.seated || m.vote.is_some())
            .copied()
            .collect();
        prop_assume!(!remaining.is_empty());
        prop_assert_eq!(decide(&members), decide(&remaining));
    }

    /// A member that voted before leaving the committee counts as if seated.
    #[test]
    fn a_member_that_voted_before_leaving_still_counts(members in electorate()) {
        let voters_seated: Vec<Member> = members
            .iter()
            .map(|m| Member { seated: m.seated || m.vote.is_some(), ..*m })
            .collect();
        prop_assert_eq!(decide(&members), decide(&voters_seated));
    }
}
