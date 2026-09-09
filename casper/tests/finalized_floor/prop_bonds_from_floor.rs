use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;

type Validator = Vec<u8>;
type Bond = (Validator, i64);
type BlockHash = Vec<u8>;

fn positive_bonds(bonds: &[Bond]) -> BTreeMap<Validator, i64> {
    bonds
        .iter()
        .filter(|(_, stake)| *stake > 0)
        .cloned()
        .collect()
}

fn cache_accepts(serialized: &[Bond], replayed_post_state: &[Bond]) -> bool {
    let serialized_set = serialized.iter().cloned().collect::<BTreeSet<_>>();
    let replayed_set = replayed_post_state.iter().cloned().collect::<BTreeSet<_>>();
    serialized_set.len() == serialized.len()
        && replayed_set.len() == replayed_post_state.len()
        && serialized_set == replayed_set
}

fn canonical_active(bonds: &[Bond], limit: usize) -> BTreeSet<Validator> {
    positive_bonds(bonds).into_keys().take(limit).collect()
}

fn active_bonds(bonds: &[Bond], active: &BTreeSet<Validator>) -> BTreeMap<Validator, i64> {
    positive_bonds(bonds)
        .into_iter()
        .filter(|(validator, _)| active.contains(validator))
        .collect()
}

fn authority_accepts(
    sender: &Validator,
    justifications: &[Validator],
    floor_bonds: &[Bond],
    floor_active: &BTreeSet<Validator>,
) -> bool {
    let committee = active_bonds(floor_bonds, floor_active);
    let justified = justifications.iter().cloned().collect::<BTreeSet<_>>();
    committee.contains_key(sender)
        && justified == committee.keys().cloned().collect::<BTreeSet<_>>()
}

fn transition_active(
    boundary: bool,
    previous_active: &BTreeSet<Validator>,
    post_state_bonds: &[Bond],
    limit: usize,
) -> BTreeSet<Validator> {
    if boundary {
        canonical_active(post_state_bonds, limit)
    } else {
        previous_active.clone()
    }
}

fn register_post_state_bonds(
    accepted: bool,
    registered: &BTreeSet<Validator>,
    post_state: &[Bond],
) -> BTreeSet<Validator> {
    let mut result = registered.clone();
    if accepted {
        result.extend(
            post_state
                .iter()
                .filter(|(_, stake)| *stake > 0)
                .map(|(validator, _)| validator.clone()),
        );
    }
    result
}

fn justification_has_provenance(
    validator: &Validator,
    latest_hash: &BlockHash,
    genesis_hash: &BlockHash,
    cited_sender: &Validator,
) -> bool {
    latest_hash == genesis_hash || validator == cited_sender
}

fn register_with_canonical_genesis(
    accepted: bool,
    registered: &BTreeMap<Validator, BlockHash>,
    post_state: &[Bond],
    genesis_hash: &BlockHash,
) -> BTreeMap<Validator, BlockHash> {
    let mut result = registered.clone();
    if accepted {
        for (validator, _stake) in post_state.iter().filter(|(_, stake)| *stake > 0) {
            result
                .entry(validator.clone())
                .or_insert_with(|| genesis_hash.clone());
        }
    }
    result
}

prop_compose! {
    fn committee()(entries in prop::collection::vec(1i64..=1_000_000i64, 1..=8)) -> Vec<Bond> {
        entries
            .into_iter()
            .enumerate()
            .map(|(index, stake)| (vec![index as u8], stake))
            .collect()
    }
}

proptest! {
    #[test]
    fn serialized_bonds_are_exactly_the_replayed_post_state(
        post_state in committee(),
        floor in committee(),
    ) {
        prop_assert!(cache_accepts(&post_state, &post_state));
        if post_state.iter().cloned().collect::<BTreeSet<_>>()
            != floor.iter().cloned().collect::<BTreeSet<_>>()
        {
            prop_assert!(!cache_accepts(&floor, &post_state));
        }
    }

    #[test]
    fn post_state_transition_cannot_authorize_its_own_block(
        floor in committee(),
        new_validator_byte in 128u8..=254,
        stake in 1i64..=1_000_000,
    ) {
        let new_validator = vec![new_validator_byte];
        let floor_active = canonical_active(&floor, floor.len());
        prop_assume!(!floor_active.contains(&new_validator));
        let mut post_state = floor.clone();
        post_state.push((new_validator.clone(), stake));
        let justifications = floor_active.iter().cloned().collect::<Vec<_>>();

        prop_assert!(cache_accepts(&post_state, &post_state));
        prop_assert!(!authority_accepts(
            &new_validator,
            &justifications,
            &floor,
            &floor_active,
        ));
    }

    #[test]
    fn certified_activation_boundary_authorizes_selected_validator_on_later_block(
        floor in committee(),
        new_validator_byte in 128u8..=254,
        stake in 1i64..=1_000_000,
    ) {
        let new_validator = vec![new_validator_byte];
        prop_assume!(!positive_bonds(&floor).contains_key(&new_validator));
        let mut promoted_floor = floor;
        promoted_floor.push((new_validator.clone(), stake));
        let promoted_active = canonical_active(&promoted_floor, promoted_floor.len());
        let justifications = promoted_active.iter().cloned().collect::<Vec<_>>();

        prop_assert!(authority_accepts(
            &new_validator,
            &justifications,
            &promoted_floor,
            &promoted_active,
        ));
    }

    #[test]
    fn off_boundary_bond_is_visible_without_granting_authority(
        floor in committee(),
        new_validator_byte in 128u8..=254,
        stake in 1i64..=1_000_000,
    ) {
        let new_validator = vec![new_validator_byte];
        let floor_active = canonical_active(&floor, floor.len());
        prop_assume!(!floor_active.contains(&new_validator));
        let mut post_state = floor.clone();
        post_state.push((new_validator.clone(), stake));
        let post_active = transition_active(false, &floor_active, &post_state, post_state.len());
        let justifications = post_active.iter().cloned().collect::<Vec<_>>();

        prop_assert!(positive_bonds(&post_state).contains_key(&new_validator));
        prop_assert!(!post_active.contains(&new_validator));
        prop_assert!(!authority_accepts(
            &new_validator,
            &justifications,
            &post_state,
            &post_active,
        ));
    }

    #[test]
    fn boundary_selection_is_order_independent_and_capped(
        bonds in committee(),
        limit in 0usize..=8,
    ) {
        let mut reverse = bonds.clone();
        reverse.reverse();
        let selected = canonical_active(&bonds, limit);
        let reverse_selected = canonical_active(&reverse, limit);
        let selected_are_positive = selected
            .iter()
            .all(|validator| positive_bonds(&bonds).contains_key(validator));

        prop_assert_eq!(&selected, &reverse_selected);
        prop_assert!(selected.len() <= limit);
        prop_assert!(selected_are_positive);
    }

    #[test]
    fn inactive_positive_bonds_do_not_enter_authority(
        bonds in committee(),
        limit in 0usize..=7,
    ) {
        prop_assume!(limit < bonds.len());
        let active = canonical_active(&bonds, limit);
        let inactive = positive_bonds(&bonds)
            .into_keys()
            .find(|validator| !active.contains(validator))
            .expect("the cap leaves one validator inactive");
        let justifications = active.iter().cloned().collect::<Vec<_>>();

        prop_assert!(!authority_accepts(
            &inactive,
            &justifications,
            &bonds,
            &active,
        ));
    }

    #[test]
    fn only_accepted_post_state_bonds_register_new_validator_slots(
        floor in committee(),
        new_validator_byte in 128u8..=254,
        stake in 1i64..=1_000_000,
    ) {
        let registered = positive_bonds(&floor).keys().cloned().collect::<BTreeSet<_>>();
        let new_validator = vec![new_validator_byte];
        prop_assume!(!registered.contains(&new_validator));
        let mut post_state = floor;
        post_state.push((new_validator.clone(), stake));

        prop_assert!(register_post_state_bonds(true, &registered, &post_state)
            .contains(&new_validator));
        prop_assert!(!register_post_state_bonds(false, &registered, &post_state)
            .contains(&new_validator));
    }

    #[test]
    fn nonpositive_post_state_bonds_never_register_new_slots(
        floor in committee(),
        new_validator_byte in 128u8..=254,
        stake in i64::MIN..=0i64,
    ) {
        let registered = positive_bonds(&floor).keys().cloned().collect::<BTreeSet<_>>();
        let new_validator = vec![new_validator_byte];
        prop_assume!(!registered.contains(&new_validator));
        let post_state = vec![(new_validator.clone(), stake)];

        prop_assert!(!register_post_state_bonds(true, &registered, &post_state)
            .contains(&new_validator));
    }

    #[test]
    fn registration_placeholder_is_independent_of_local_junk(
        floor in committee(),
        new_validator_byte in 128u8..=254,
        genesis_hash in prop::collection::vec(any::<u8>(), 32),
        junk_hash in prop::collection::vec(any::<u8>(), 32),
    ) {
        let registered = positive_bonds(&floor)
            .keys()
            .cloned()
            .map(|validator| (validator, genesis_hash.clone()))
            .collect::<BTreeMap<_, _>>();
        let new_validator = vec![new_validator_byte];
        prop_assume!(!registered.contains_key(&new_validator));
        prop_assume!(junk_hash != genesis_hash);
        let post_state = vec![(new_validator.clone(), 1)];

        let without_junk = register_with_canonical_genesis(
            true,
            &registered,
            &post_state,
            &genesis_hash,
        );
        let mut with_junk = registered.clone();
        with_junk.insert(vec![0xff, 0xff], junk_hash);
        let with_junk = register_with_canonical_genesis(
            true,
            &with_junk,
            &post_state,
            &genesis_hash,
        );

        prop_assert_eq!(without_junk.get(&new_validator), Some(&genesis_hash));
        prop_assert_eq!(with_junk.get(&new_validator), Some(&genesis_hash));
    }

    #[test]
    fn justification_hashes_cannot_be_relabelled_between_validators(
        validator in prop::collection::vec(any::<u8>(), 1..=65),
        cited_sender in prop::collection::vec(any::<u8>(), 1..=65),
        latest_hash in prop::collection::vec(any::<u8>(), 32),
        genesis_hash in prop::collection::vec(any::<u8>(), 32),
    ) {
        prop_assume!(validator != cited_sender);
        prop_assume!(latest_hash != genesis_hash);

        prop_assert!(!justification_has_provenance(
            &validator,
            &latest_hash,
            &genesis_hash,
            &cited_sender,
        ));
        prop_assert!(justification_has_provenance(
            &validator,
            &genesis_hash,
            &genesis_hash,
            &cited_sender,
        ));
    }

    #[test]
    fn authority_is_independent_of_head_and_post_state_bonds(
        floor in committee(),
        head in committee(),
        post_state in committee(),
    ) {
        let floor_active = canonical_active(&floor, floor.len());
        let committee = active_bonds(&floor, &floor_active);
        let sender = committee.keys().next().cloned().expect("committee is non-empty");
        let justifications = committee.keys().cloned().collect::<Vec<_>>();
        let expected = authority_accepts(&sender, &justifications, &floor, &floor_active);

        let divergent_head = head
            .into_iter()
            .map(|(mut validator, stake)| {
                validator.insert(0, 0xfe);
                (validator, stake)
            })
            .collect::<Vec<_>>();
        let divergent_post_state = post_state
            .into_iter()
            .map(|(mut validator, stake)| {
                validator.insert(0, 0xfd);
                (validator, stake)
            })
            .collect::<Vec<_>>();

        prop_assert!(expected);
        prop_assert!(!authority_accepts(
            &sender,
            &justifications,
            &divergent_head,
            &floor_active,
        ));
        prop_assert!(!authority_accepts(
            &sender,
            &justifications,
            &divergent_post_state,
            &floor_active,
        ));
        prop_assert_eq!(
            authority_accepts(&sender, &justifications, &floor, &floor_active),
            expected,
        );
    }

    #[test]
    fn post_state_cache_rejects_drop_stake_and_spurious_mutations(
        post_state in committee(),
        index in any::<prop::sample::Index>(),
    ) {
        let selected = index.index(post_state.len());

        let mut dropped = post_state.clone();
        dropped.remove(selected);
        prop_assert!(!cache_accepts(&dropped, &post_state));

        let mut changed_stake = post_state.clone();
        changed_stake[selected].1 += 1;
        prop_assert!(!cache_accepts(&changed_stake, &post_state));

        let mut spurious = post_state.clone();
        spurious.push((vec![0xff, 0xff], 1));
        prop_assert!(!cache_accepts(&spurious, &post_state));

        let mut duplicate = post_state.clone();
        duplicate.push(post_state[selected].clone());
        prop_assert!(!cache_accepts(&duplicate, &post_state));
    }

    #[test]
    fn authority_requires_exact_floor_justifications(floor in committee()) {
        let floor_active = canonical_active(&floor, floor.len());
        let committee = active_bonds(&floor, &floor_active);
        let sender = committee.keys().next().cloned().expect("committee is non-empty");
        let mut justifications = committee.keys().cloned().collect::<Vec<_>>();
        prop_assert!(authority_accepts(
            &sender,
            &justifications,
            &floor,
            &floor_active,
        ));

        justifications.pop();
        prop_assert!(!authority_accepts(
            &sender,
            &justifications,
            &floor,
            &floor_active,
        ));

        let mut extra = committee.keys().cloned().collect::<Vec<_>>();
        extra.push(vec![0xff, 0xff]);
        prop_assert!(!authority_accepts(
            &sender,
            &extra,
            &floor,
            &floor_active,
        ));
    }
}
