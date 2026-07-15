// P1 proptests — `bonds_cache_from_floor` committee PLAY ≡ REPLAY.
//
// Models the committee derivation shared by the two independent copies:
//   - PROPOSER  blocks/proposer/block_creator.rs:1081-1084
//         let committee: Vec<Bond> = floor_bonds
//             .into_iter().filter(|bond| active.contains(&bond.validator)).collect();
//   - VALIDATOR validate.rs::bonds_cache_from_floor, :1471-1477
//         let computed_bonds_set: HashSet<_> = computed_bonds.iter()
//             .filter(|bond| active.contains(&bond.validator))
//             .map(|bond| (bond.validator.clone(), bond.stake)).collect();
//         if bonds_set == computed_bonds_set { Valid } else { InvalidBondsCache }
//
// Both sites: committee = compute_bonds(floor_state) filtered to the active validator
// set, via the BYTE-IDENTICAL predicate `active.contains(&bond.validator)`. Given the
// same finalized floor, both call the deterministic `compute_bonds(floor_state)` and
// `get_active_validators(floor_state)`, so they see the same `(floor_bonds, active)`.
// The proposer emits an ORDERED `Vec<Bond>` (→ block.bonds); the validator compares as
// a SET of (validator, stake). This test models that shared derivation and the
// validator's set-equality accept rule, and checks PLAY ≡ REPLAY plus rejection of
// every committee-CHANGING perturbation (order/duplicate perturbations are set-
// preserving and are correctly TOLERATED — see the two `..._is_tolerated` tests).
//
// Discharged math (no new Rocq): Selection.committee_is_floor_bonds +
// committee_deterministic + Floor.frontier_cache_transparent.

use proptest::prelude::*;
use std::collections::{BTreeSet, HashSet};

type Validator = Vec<u8>;
/// Faithful to `models::rust::casper::protocol::casper_message::Bond`
/// (`{ validator: ByteString, stake: i64 }`).
type Bond = (Validator, i64);

/// PROPOSER-side derivation (block_creator.rs:1081-1084): floor bonds filtered to
/// active, emitted as an ORDERED `Vec<Bond>` that becomes `block.bonds`.
fn proposer_committee(floor_bonds: &[Bond], active: &HashSet<Validator>) -> Vec<Bond> {
    floor_bonds
        .iter()
        .filter(|(v, _)| active.contains(v))
        .cloned()
        .collect()
}

/// VALIDATOR-side derivation (validate.rs:1471-1475): the SAME floor bonds filtered
/// to active, collected as a SET of (validator, stake) for the equality check.
fn validator_committee(floor_bonds: &[Bond], active: &HashSet<Validator>) -> BTreeSet<Bond> {
    floor_bonds
        .iter()
        .filter(|(v, _)| active.contains(v))
        .cloned()
        .collect()
}

/// VALIDATOR accept rule (validate.rs:1477): `bonds_set == computed_bonds_set`, where
/// `bonds_set` is the block's declared bonds projected to a SET of (validator, stake).
fn accepts(block_bonds: &[Bond], committee: &BTreeSet<Bond>) -> bool {
    let block_set: BTreeSet<Bond> = block_bonds.iter().cloned().collect();
    &block_set == committee
}

// A scenario is a set of floor bonds with UNIQUE validators (compute_bonds returns a
// per-validator map) plus a per-validator "active" flag. Encoded as entries indexed by
// position i ⇒ validator = [i], stake = entries[i].0, active = entries[i].1.
prop_compose! {
    fn scenario()(entries in prop::collection::vec((1i64..=1_000_000i64, any::<bool>()), 1..=8))
        -> (Vec<Bond>, HashSet<Validator>) {
        let floor_bonds: Vec<Bond> =
            entries.iter().enumerate().map(|(i, (s, _))| (vec![i as u8], *s)).collect();
        let active: HashSet<Validator> = entries
            .iter()
            .enumerate()
            .filter_map(|(i, (_, a))| if *a { Some(vec![i as u8]) } else { None })
            .collect();
        (floor_bonds, active)
    }
}

proptest! {
    // (i) PLAY ≡ REPLAY: proposer and validator derive the SAME committee (set) from
    // the same floor — the filter predicate is identical on both sides.
    #[test]
    fn proposer_derive_eq_validator_derive((floor_bonds, active) in scenario()) {
        let proposer: BTreeSet<Bond> = proposer_committee(&floor_bonds, &active).into_iter().collect();
        let validator = validator_committee(&floor_bonds, &active);
        prop_assert_eq!(proposer, validator);
    }

    // (ii) accept rule IS set-equality, and the honest proposer block is accepted.
    #[test]
    fn accept_rule_is_set_equality((floor_bonds, active) in scenario()) {
        let committee = validator_committee(&floor_bonds, &active);
        let honest = proposer_committee(&floor_bonds, &active);
        // honest block (the proposer's committee) is accepted on replay.
        prop_assert!(accepts(&honest, &committee));
        // accepts(b) ⟺ set(b) == committee, definitionally.
        let honest_set: BTreeSet<Bond> = honest.iter().cloned().collect();
        prop_assert_eq!(accepts(&honest, &committee), honest_set == committee);
    }

    // (ii-drop) drop-one perturbation ⇒ REJECT (committee non-empty).
    #[test]
    fn reject_drop_one((floor_bonds, active) in scenario(), idx in any::<prop::sample::Index>()) {
        let committee = validator_committee(&floor_bonds, &active);
        let honest = proposer_committee(&floor_bonds, &active);
        prop_assume!(!honest.is_empty());
        let j = idx.index(honest.len());
        let mut perturbed = honest.clone();
        perturbed.remove(j);
        prop_assert!(!accepts(&perturbed, &committee), "drop-one must be rejected");
    }

    // (ii-stake) stake-delta perturbation ⇒ REJECT (committee non-empty).
    #[test]
    fn reject_stake_delta((floor_bonds, active) in scenario(), idx in any::<prop::sample::Index>()) {
        let committee = validator_committee(&floor_bonds, &active);
        let honest = proposer_committee(&floor_bonds, &active);
        prop_assume!(!honest.is_empty());
        let j = idx.index(honest.len());
        let mut perturbed = honest.clone();
        perturbed[j].1 += 1; // stakes ∈ [1, 1e6]; +1 cannot overflow i64.
        prop_assert!(!accepts(&perturbed, &committee), "stake-delta must be rejected");
    }

    // (ii-spurious) spurious-validator perturbation ⇒ REJECT. The 2-byte validator
    // cannot collide with any single-byte floor validator, so it is never in committee.
    #[test]
    fn reject_spurious_validator((floor_bonds, active) in scenario(), stake in any::<i64>()) {
        let committee = validator_committee(&floor_bonds, &active);
        let mut perturbed = proposer_committee(&floor_bonds, &active);
        perturbed.push((vec![0xFF, 0xFF], stake));
        prop_assert!(!accepts(&perturbed, &committee), "spurious validator must be rejected");
    }

    // (ii-reorder) reorder perturbation ⇒ TOLERATED (accepted). The validator compares
    // SETS (validate.rs:1477), so bond ORDER in the block is consensus-irrelevant to
    // this check — which is exactly why PLAY ≡ REPLAY is robust to bond ordering.
    #[test]
    fn reorder_is_tolerated((floor_bonds, active) in scenario()) {
        let committee = validator_committee(&floor_bonds, &active);
        let mut perturbed = proposer_committee(&floor_bonds, &active);
        perturbed.reverse();
        prop_assert!(accepts(&perturbed, &committee), "reorder is set-preserving ⇒ accepted");
    }

    // (ii-dup) duplicate perturbation ⇒ TOLERATED (accepted). Duplicating an existing
    // (validator, stake) is set-preserving under the HashSet comparison.
    #[test]
    fn duplicate_is_tolerated((floor_bonds, active) in scenario()) {
        let committee = validator_committee(&floor_bonds, &active);
        let mut perturbed = proposer_committee(&floor_bonds, &active);
        prop_assume!(!perturbed.is_empty());
        let first = perturbed[0].clone();
        perturbed.push(first);
        prop_assert!(accepts(&perturbed, &committee), "duplicate is set-preserving ⇒ accepted");
    }
}
