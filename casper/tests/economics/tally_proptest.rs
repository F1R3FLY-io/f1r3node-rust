// No ejection commits without a 2/3-stake witness.
// The tally decides every claim family alike, so the property is stated on it.

use std::collections::HashSet;

use casper::rust::economics::claims::tally;
use models::rust::casper::protocol::casper_message::{Bond, ClaimOutcome};
use models::rust::validator::Validator;
use proptest::prelude::*;

fn validator(index: usize) -> Validator { Validator::from(vec![index as u8; 33]) }

/// An electorate with a vote, or none, and a slashed flag for each member.
/// Small stakes make an exact two-thirds split, the boundary, common.
fn electorate() -> impl Strategy<Value = Vec<(i64, Option<bool>, bool)>> {
    prop::collection::vec(
        (
            prop_oneof![1i64..4, 1i64..1_000_000],
            prop::option::of(any::<bool>()),
            any::<bool>(),
        ),
        1..16,
    )
}

fn decide(members: &[(i64, Option<bool>, bool)]) -> Option<ClaimOutcome> {
    let bonds: Vec<Bond> = members
        .iter()
        .enumerate()
        .map(|(index, (stake, _, _))| Bond {
            validator: validator(index),
            stake: *stake,
        })
        .collect();
    let slashed: HashSet<Validator> = members
        .iter()
        .enumerate()
        .filter(|(_, (_, _, slashed))| *slashed)
        .map(|(index, _)| validator(index))
        .collect();
    tally(&bonds, &slashed, |voter| members[voter[0] as usize].1)
}

/// The stake of the unslashed members, and of those among them voting yea and
/// nay.
fn stakes(members: &[(i64, Option<bool>, bool)]) -> (i128, i128, i128) {
    let counted = members.iter().filter(|(_, _, slashed)| !slashed);
    let total = counted.clone().map(|(stake, _, _)| *stake as i128).sum();
    let with = |vote: bool| {
        counted
            .clone()
            .filter(|(_, cast, _)| *cast == Some(vote))
            .map(|(stake, _, _)| *stake as i128)
            .sum()
    };
    (total, with(true), with(false))
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1024,
        .. ProptestConfig::default()
    })]

    /// A pass has more than two thirds of the unslashed stake behind it.
    #[test]
    fn no_claim_passes_without_a_two_thirds_witness(members in electorate()) {
        let (total, yea, _) = stakes(&members);
        if decide(&members) == Some(ClaimOutcome::Pass) {
            prop_assert!(3 * yea > 2 * total);
        }
    }

    /// An honest third that withholds its yea blocks the claim, whatever the
    /// rest does.
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
        let unslashed: Vec<(i64, Option<bool>, bool)> = members
            .iter()
            .filter(|(_, _, slashed)| !slashed)
            .copied()
            .collect();
        prop_assume!(!unslashed.is_empty());
        prop_assert_eq!(decide(&members), decide(&unslashed));
    }
}
