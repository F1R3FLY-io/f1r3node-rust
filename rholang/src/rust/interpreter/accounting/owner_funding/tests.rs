//! Added by G3 (DR-135): the tests of the owner ledger primitives (plan S1).
//!
//! The purse locator of these tests is a plain function, as Casper's
//! `purse_of` is. A ground below `FORGEABLE` is an unforgeable purse with
//! custody `[byte; 32]`. A ground at or above `FORGEABLE` is forgeable. A
//! compound of unforgeable grounds has a vault whose custody is the digest of
//! the compound, and one atom purse for each element.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostAuthority, CostRegion, CostSignature, CostSignatureCompound};
use models::rust::rholang::sorter::cost_accounting_sorter::sort_signature;
use num_bigint::BigUint;
use proptest::prelude::*;
use prost::Message;

use super::*;
use crate::rust::interpreter::accounting::authority::canonical_authority;

/// The first ground byte of a forgeable signature.
const FORGEABLE: u8 = 0xF0;

/// The first ground byte of a client signature. Client bytes lie in
/// `CLIENT..FORGEABLE`.
const CLIENT: u8 = 0xC0;

fn ground(byte: u8) -> CostSignature {
    CostSignature {
        value: Some(Value::Ground(vec![byte; 4])),
    }
}

fn compound(bytes: &[u8]) -> CostSignature {
    sort_signature(&CostSignature {
        value: Some(Value::Compound(CostSignatureCompound {
            elements: bytes.iter().map(|byte| ground(*byte)).collect(),
        })),
    })
    .term
}

fn unit() -> CostSignature {
    CostSignature {
        value: Some(Value::Unit(true)),
    }
}

fn purse(byte: u8) -> OwnerPurse {
    OwnerPurse {
        custody: [byte; 32],
        signature: ground(byte),
    }
}

/// The vault of a compound: its custody is the digest of the compound, which
/// no atom custody `[byte; 32]` equals.
fn vault_of(signature: &CostSignature) -> OwnerPurse {
    OwnerPurse {
        custody: Blake2b256::hash(signature.encode_to_vec())
            .try_into()
            .expect("a 32-byte digest"),
        signature: signature.clone(),
    }
}

fn locate_atom(signature: &CostSignature) -> Option<OwnerPurse> {
    match signature.value.as_ref()? {
        Value::Ground(bytes) => match bytes.first() {
            Some(byte) if *byte < FORGEABLE => Some(purse(*byte)),
            _ => None,
        },
        _ => None,
    }
}

fn locate(signature: &CostSignature) -> Option<OwnerPurseSet> {
    match signature.value.as_ref()? {
        Value::Compound(compound) => {
            let mut atoms = Vec::with_capacity(compound.elements.len());
            for element in &compound.elements {
                atoms.push(locate_atom(element)?);
            }
            Some(OwnerPurseSet::Compound {
                vault: vault_of(signature),
                atoms: atoms.into(),
            })
        }
        _ => locate_atom(signature).map(OwnerPurseSet::Single),
    }
}

fn lane(signature: &CostSignature) -> [u8; 32] {
    cost_signature_to_sig(signature)
        .expect("a canonical signature")
        .lane_hash()
}

fn binding(client: &[CostSignature], table: OwnerTable) -> OwnerFundingBinding {
    OwnerFundingBinding::new(client.iter().map(lane).collect(), locate, table)
}

/// A view that lists `budgets` and records every purse that it answers.
fn recording_view<'a>(
    budgets: &'a BTreeMap<[u8; 32], u64>,
    consulted: &'a RefCell<BTreeSet<[u8; 32]>>,
) -> impl Fn(&OwnerPurse) -> Option<u64> + 'a {
    move |purse| {
        consulted.borrow_mut().insert(purse.custody);
        budgets.get(&purse.custody).copied()
    }
}

fn single_share(purse: OwnerPurse, amount: u128) -> OwnerRegionShare {
    OwnerRegionShare {
        purses: Some(OwnerPurseSet::Single(purse)),
        units: 1,
        usage: u64::try_from(amount).unwrap_or(u64::MAX),
        amount,
    }
}

fn compound_share(vault: OwnerPurse, atoms: Vec<OwnerPurse>, amount: u128) -> OwnerRegionShare {
    OwnerRegionShare {
        units: atoms.len() as u64,
        purses: Some(OwnerPurseSet::Compound {
            vault,
            atoms: atoms.into(),
        }),
        usage: u64::try_from(amount).unwrap_or(u64::MAX),
        amount,
    }
}

fn charge(shares: Vec<OwnerRegionShare>) -> OwnerCharge {
    OwnerCharge {
        shares,
        transferee: None,
        restorations: Vec::new(),
    }
}

fn multiset(entries: &[([u8; 32], u64)]) -> ResourceMultiset<[u8; 32]> {
    ResourceMultiset(
        entries
            .iter()
            .copied()
            .filter(|(_, amount)| *amount > 0)
            .collect(),
    )
}

/// Rule 3 and Rule 2 on a compound of two atoms: the vault pays the whole
/// share when it can, else each atom pays half when both can, else nothing.
/// A repeated atom pays once for each occurrence. The charge of a compound
/// region carries its leaf units and its atom purses.
#[test]
fn compound_owner_pays_by_vault_or_equal_shares() {
    let signature = compound(&[1, 2]);
    let vault = vault_of(&signature);
    let (a, b) = (purse(1), purse(2));
    let share = charge(vec![compound_share(
        vault.clone(),
        vec![a.clone(), b.clone()],
        10,
    )]);
    let run = |budgets: &[([u8; 32], u64)]| {
        let budgets: BTreeMap<_, _> = budgets.iter().copied().collect();
        allocate(
            &share,
            &|purse: &OwnerPurse| budgets.get(&purse.custody).copied(),
            UnlistedRule::Strict,
            false,
            None,
        )
    };
    assert_eq!(
        run(&[(vault.custody, 10), (a.custody, 5), (b.custody, 5)]),
        Ok(multiset(&[(vault.custody, 10)])),
        "Rule 3: a funded vault pays the whole share"
    );
    assert_eq!(
        run(&[(vault.custody, 9), (a.custody, 5), (b.custody, 5)]),
        Ok(multiset(&[(a.custody, 5), (b.custody, 5)])),
        "Rule 2: each atom pays an equal part"
    );
    assert_eq!(
        run(&[(vault.custody, 9), (a.custody, 5), (b.custody, 4)]),
        Err(OwnerShortfall),
        "an atom short of its part refuses the whole share"
    );
    assert_eq!(
        run(&[(a.custody, 5), (b.custody, 5)]),
        Ok(multiset(&[(a.custody, 5), (b.custody, 5)])),
        "an unlisted vault has budget 0 under the strict rule"
    );

    let doubled = charge(vec![compound_share(
        vault.clone(),
        vec![a.clone(), a.clone()],
        10,
    )]);
    let run_doubled = |budget: u64| {
        allocate(
            &doubled,
            &|purse: &OwnerPurse| (purse.custody == a.custody).then_some(budget),
            UnlistedRule::Strict,
            false,
            None,
        )
    };
    assert_eq!(run_doubled(10), Ok(multiset(&[(a.custody, 10)])));
    assert_eq!(run_doubled(9), Err(OwnerShortfall));

    // A later share on an atom sees the atom's Rule 2 part.
    let then_single = charge(vec![
        compound_share(vault.clone(), vec![a.clone(), b.clone()], 10),
        single_share(a.clone(), 3),
    ]);
    let run_then_single = |budget_of_a: u64| {
        let budgets: BTreeMap<_, _> =
            [(vault.custody, 9), (a.custody, budget_of_a), (b.custody, 5)].into();
        allocate(
            &then_single,
            &|purse: &OwnerPurse| budgets.get(&purse.custody).copied(),
            UnlistedRule::Strict,
            false,
            None,
        )
    };
    assert_eq!(
        run_then_single(8),
        Ok(multiset(&[(a.custody, 8), (b.custody, 5)]))
    );
    assert_eq!(run_then_single(7), Err(OwnerShortfall));

    // The charge of a region signed by the compound, at price 3 with usage 4.
    let binding = binding(&[], OwnerTable::empty(8));
    let region = priced_cost_region(&signature, b"compound owner", 0, 3).expect("a region");
    let authority = canonical_authority(&CostAuthority {
        regions: vec![region],
    })
    .expect("a canonical authority");
    let owned = binding
        .owner_charge(&authority, &[4])
        .expect("an owner charge");
    assert_eq!(owned.shares.len(), 1);
    assert_eq!(owned.shares[0].units, 2);
    assert_eq!(owned.shares[0].usage, 4);
    assert_eq!(owned.shares[0].amount, 12);
    assert_eq!(owned.shares[0].purses, locate(&signature));
    let budgets: BTreeMap<_, _> = [(vault.custody, 11), (a.custody, 6), (b.custody, 6)].into();
    assert_eq!(
        allocate(
            &owned,
            &|purse: &OwnerPurse| budgets.get(&purse.custody).copied(),
            UnlistedRule::Strict,
            false,
            None,
        ),
        Ok(multiset(&[(a.custody, 6), (b.custody, 6)]))
    );
}

/// The owner table keeps strictly ascending custodies within its room, finds
/// an entry by binary search, and `take_notes` returns each unlisted noted
/// custody once, in custody order, with the smallest signature.
#[test]
fn the_owner_table_and_its_notes_keep_their_order() {
    let entry = |budget: u64| OwnerTableEntry {
        signature: ground(1),
        budget,
    };
    assert_eq!(
        OwnerTable::new(vec![([2; 32], entry(1)), ([1; 32], entry(1))], 4),
        Err(OwnerTableError::NotAscending)
    );
    assert_eq!(
        OwnerTable::new(vec![([1; 32], entry(1)), ([1; 32], entry(1))], 4),
        Err(OwnerTableError::NotAscending)
    );
    assert_eq!(
        OwnerTable::new(vec![([1; 32], entry(1)), ([2; 32], entry(1))], 1),
        Err(OwnerTableError::OverRoom)
    );
    let table = OwnerTable::new(vec![([1; 32], entry(5)), ([3; 32], entry(7))], 3)
        .expect("an ascending table within its room");
    assert_eq!(table.budget(&[1; 32]), Some(5));
    assert_eq!(table.budget(&[3; 32]), Some(7));
    assert_eq!(table.budget(&[2; 32]), None);
    assert!(table.has_room());
    assert!(!OwnerTable::empty(0).has_room());

    let binding = binding(&[], table);
    assert!(!binding.notes().any());
    for noted in [purse(4), purse(1), purse(2), purse(4)] {
        binding.notes().note(noted);
    }
    binding.notes().note(OwnerPurse {
        custody: [2; 32],
        signature: ground(0),
    });
    assert!(binding.notes().any());
    assert_eq!(binding.take_notes(), vec![
        OwnerPurse {
            custody: [2; 32],
            signature: ground(0),
        },
        purse(4),
    ]);
    assert!(!binding.notes().any());
    assert!(binding.take_notes().is_empty());
}

/// A region signature of the property tests: Unit, a client ground, an
/// owner ground, a forgeable ground, or a compound of owner grounds.
fn arb_signature() -> impl Strategy<Value = CostSignature> {
    prop_oneof![
        Just(unit()),
        (CLIENT..FORGEABLE).prop_map(ground),
        (0..CLIENT).prop_map(ground),
        (FORGEABLE..=u8::MAX).prop_map(ground),
        prop::collection::vec(0..CLIENT, 2..5).prop_map(|bytes| compound(&bytes)),
        prop::collection::vec(prop_oneof![0..CLIENT, FORGEABLE..=u8::MAX], 2..4)
            .prop_map(|bytes| compound(&bytes)),
    ]
}

fn arb_price() -> impl Strategy<Value = u64> {
    prop_oneof![Just(0_u64), Just(u64::MAX), 1_u64..1000, any::<u64>()]
}

/// The leaf count of a test signature.
fn leaves(signature: &CostSignature) -> u64 {
    match signature.value.as_ref() {
        Some(Value::Compound(compound)) => compound.elements.len() as u64,
        Some(Value::Unit(_)) => 0,
        _ => 1,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// The shares of a charge are the owner regions with usage, in region
    /// order, and they sum to the owner usage times the region price. Unit
    /// and client regions have no share. A forgeable owner has a share with
    /// no purse.
    #[test]
    fn owner_shares_sum_to_owner_usage_times_price(
        regions in prop::collection::vec((arb_signature(), arb_price()), 1..12),
        usages in prop::collection::vec(prop_oneof![Just(0_u64), Just(u64::MAX), any::<u64>()], 12),
        client in prop::collection::vec(CLIENT..FORGEABLE, 0..4),
    ) {
        let client: Vec<_> = client.into_iter().map(ground).collect();
        let binding = binding(&client, OwnerTable::empty(8));
        let authority = canonical_authority(&CostAuthority {
            regions: regions
                .iter()
                .enumerate()
                .map(|(index, (signature, price))| {
                    priced_cost_region(signature, &index.to_le_bytes(), 0, *price)
                        .expect("a canonical region")
                })
                .collect(),
        })
        .expect("distinct region identities");
        let usages = &usages[..authority.regions.len()];
        let owned = binding.owner_charge(&authority, usages).expect("an owner charge");

        let mut expected_total = BigUint::from(0_u8);
        let mut expected_shares = Vec::new();
        for (region, usage) in authority.regions.iter().zip(usages) {
            let signature = region.signature.as_ref().expect("a signature");
            let is_client = client.iter().any(|client| lane(client) == lane(signature));
            if *usage == 0 || is_unit_cost_signature(signature) || is_client {
                prop_assert_eq!(
                    binding.is_owner_region(region).expect("a canonical region"),
                    !is_unit_cost_signature(signature) && !is_client
                );
                continue;
            }
            prop_assert!(binding.is_owner_region(region).expect("a canonical region"));
            let amount = u128::from(*usage) * u128::from(region.price);
            expected_total += BigUint::from(amount);
            expected_shares.push(OwnerRegionShare {
                purses: locate(signature),
                units: leaves(signature),
                usage: *usage,
                amount,
            });
        }
        prop_assert_eq!(owned.shares.iter().map(|share| BigUint::from(share.amount)).sum::<BigUint>(), expected_total);
        prop_assert_eq!(&owned.shares, &expected_shares);
        prop_assert_eq!(owned.transferee, None);
        prop_assert!(owned.restorations.is_empty());
        prop_assert_eq!(
            binding.owner_charge(&authority, &usages[1..]),
            Err(AuthorityError::MissingAuthority),
            "usages of another authority are rejected"
        );
    }

    /// A compound share is paid exactly: by the vault when its budget covers
    /// the share (Rule 3), else by every atom with one equal part for each
    /// occurrence (Rule 2), else not at all. Rule 2 never draws the vault, and
    /// a refused share draws nothing.
    #[test]
    fn compound_split_is_exact_and_all_or_nothing(
        atom_bytes in prop::collection::vec(1_u8..5, 2..6),
        part in prop_oneof![Just(0_u64), 1_u64..1000, Just(u64::MAX / 8)],
        vault_slack in 0_u64..3,
        vault_budget in prop_oneof![Just(0_u64), 0_u64..4000, Just(u64::MAX)],
        atom_budgets in prop::collection::vec(prop_oneof![Just(0_u64), 0_u64..6000, Just(u64::MAX)], 4),
    ) {
        let signature = compound(&atom_bytes);
        let Some(OwnerPurseSet::Compound { vault, atoms }) = locate(&signature) else {
            panic!("a compound of owner grounds has compound purses");
        };
        let units = atoms.len() as u128;
        let amount = u128::from(part) * units;
        let mut budgets: BTreeMap<[u8; 32], u64> = BTreeMap::new();
        budgets.insert(vault.custody, if vault_slack == 0 {
            u64::try_from(amount).unwrap_or(u64::MAX)
        } else {
            vault_budget
        });
        for (index, byte) in (1_u8..5).enumerate() {
            budgets.insert([byte; 32], atom_budgets[index]);
        }
        let owned = charge(vec![compound_share(vault.clone(), atoms.to_vec(), amount)]);
        let result = allocate(
            &owned,
            &|purse: &OwnerPurse| budgets.get(&purse.custody).copied(),
            UnlistedRule::Strict,
            false,
            None,
        );

        let mut multiplicity: BTreeMap<[u8; 32], u128> = BTreeMap::new();
        for atom in atoms.iter() {
            *multiplicity.entry(atom.custody).or_insert(0) += 1;
        }
        let vault_pays = u128::from(budgets[&vault.custody]) >= amount;
        let atoms_pay = multiplicity.iter().all(|(custody, count)| {
            u128::from(budgets[custody]) >= u128::from(part) * count
        });
        let expected = if amount == 0 {
            Ok(ResourceMultiset::default())
        } else if vault_pays {
            Ok(multiset(&[(vault.custody, u64::try_from(amount).expect("covered by a u64 budget"))]))
        } else if atoms_pay {
            Ok(ResourceMultiset(
                multiplicity
                    .iter()
                    .map(|(custody, count)| {
                        (*custody, u64::try_from(u128::from(part) * count).expect("covered by a u64 budget"))
                    })
                    .filter(|(_, amount)| *amount > 0)
                    .collect(),
            ))
        } else {
            Err(OwnerShortfall)
        };
        prop_assert_eq!(&result, &expected);
        if let Ok(draws) = result {
            prop_assert_eq!(draws.0.values().map(|amount| u128::from(*amount)).sum::<u128>(), amount);
            if !vault_pays {
                prop_assert_eq!(draws.get(&vault.custody), 0);
            }
        }
    }
}

/// A random owner charge over the purses `[byte; 32]` for bytes `1..8`, with
/// single and compound shares, a transferee and one level of restorations.
fn arb_charge() -> impl Strategy<Value = OwnerCharge> {
    let purse_set = prop_oneof![
        Just(None),
        (1_u8..8).prop_map(|byte| Some(OwnerPurseSet::Single(purse(byte)))),
        (1_u8..8, prop::collection::vec(1_u8..8, 2..4)).prop_map(|(vault, atoms)| {
            Some(OwnerPurseSet::Compound {
                vault: purse(vault),
                atoms: atoms.into_iter().map(purse).collect::<Vec<_>>().into(),
            })
        }),
    ];
    let share = (purse_set.clone(), prop_oneof![
        Just(0_u64),
        1_u64..200,
        Just(u64::MAX / 4),
        Just(u64::MAX)
    ])
        .prop_map(|(purses, part)| {
            let units = match purses.as_ref() {
                Some(OwnerPurseSet::Compound { atoms, .. }) => atoms.len() as u64,
                _ => 1,
            };
            OwnerRegionShare {
                purses,
                units,
                usage: part,
                amount: u128::from(part) * u128::from(units),
            }
        });
    let shares = prop::collection::vec(share, 0..5);
    (
        shares.clone(),
        prop_oneof![Just(None), purse_set.prop_map(Some)],
        prop::collection::vec(shares, 0..3),
    )
        .prop_map(|(shares, transferee, restorations)| OwnerCharge {
            shares,
            transferee,
            restorations: restorations
                .into_iter()
                .enumerate()
                .map(|(index, shares)| ([index as u8; 32], charge(shares)))
                .collect(),
        })
}

fn arb_rule() -> impl Strategy<Value = UnlistedRule> {
    prop_oneof![
        Just(UnlistedRule::Play),
        Just(UnlistedRule::Strict),
        Just(UnlistedRule::Lenient),
    ]
}

/// The budgets of a listed subset of the purses `1..8`.
fn arb_listed() -> impl Strategy<Value = BTreeMap<[u8; 32], u64>> {
    prop::collection::btree_map(
        (1_u8..8).prop_map(|byte| [byte; 32]),
        prop_oneof![Just(0_u64), 0_u64..1000, Just(u64::MAX)],
        0..8,
    )
}

/// Every share of a charge and of its restorations, in allocation order.
fn every_share(charge: &OwnerCharge) -> Vec<&OwnerRegionShare> {
    let mut shares: Vec<&OwnerRegionShare> = charge.shares.iter().collect();
    for (_, restoration) in &charge.restorations {
        shares.extend(restoration.shares.iter());
    }
    shares
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Whatever the view, the rule and the room, a granted allocation draws
    /// each purse within its budget, pays every share exactly, and leaves
    /// each transferee a positive budget (A5). An unlisted purse pays only
    /// under the lenient rule with a free room.
    #[test]
    fn allocate_never_overdraws_any_view(
        owned in arb_charge(),
        listed in arb_listed(),
        rule in arb_rule(),
        room_free in any::<bool>(),
    ) {
        let notes = OwnerNotes::default();
        let result = allocate(
            &owned,
            &|purse: &OwnerPurse| listed.get(&purse.custody).copied(),
            rule,
            room_free,
            Some(&notes),
        );
        let budget = |custody: &[u8; 32]| -> u128 {
            match (listed.get(custody), rule, room_free) {
                (Some(budget), _, _) => u128::from(*budget),
                (None, UnlistedRule::Lenient, true) => u128::MAX,
                (None, _, _) => 0,
            }
        };
        if let Ok(draws) = result {
            for (custody, amount) in &draws.0 {
                prop_assert!(u128::from(*amount) <= budget(custody), "an overdrawn purse");
            }
            let paid: u128 = draws.0.values().map(|amount| u128::from(*amount)).sum();
            let owed: u128 = every_share(&owned).iter().map(|share| share.amount).sum();
            prop_assert_eq!(paid, owed, "every share is paid exactly");
            prop_assert!(
                every_share(&owned).iter().all(|share| share.amount <= u128::from(u64::MAX)),
                "no share above u64::MAX is granted"
            );
            let remaining =
                |purse: &OwnerPurse| budget(&purse.custody) - u128::from(draws.get(&purse.custody));
            if let Some(transferee) = owned.transferee.as_ref() {
                match transferee {
                    None => prop_assert!(false, "a forgeable transferee never passes"),
                    Some(OwnerPurseSet::Single(purse)) => prop_assert!(remaining(purse) > 0),
                    Some(OwnerPurseSet::Compound { vault, atoms }) => prop_assert!(
                        remaining(vault) > 0 || atoms.iter().all(|atom| remaining(atom) > 0)
                    ),
                }
            }
        }
    }

    /// An unlisted purse has budget 0 in play: play decides as the strict
    /// rule does, and as the strict rule with the purse listed at 0. Play
    /// notes exactly the unlisted purses that the allocation consulted, each
    /// once, when the table has room, and none when the table is full.
    #[test]
    fn unknown_purse_is_zero_and_noted_in_play(
        owned in arb_charge(),
        listed in arb_listed(),
        room_free in any::<bool>(),
    ) {
        let consulted = RefCell::new(BTreeSet::new());
        let strict = allocate(
            &owned,
            &recording_view(&listed, &consulted),
            UnlistedRule::Strict,
            room_free,
            None,
        );
        let zeroed: BTreeMap<[u8; 32], u64> = (1_u8..8)
            .map(|byte| ([byte; 32], listed.get(&[byte; 32]).copied().unwrap_or(0)))
            .collect();
        let listed_at_zero = allocate(
            &owned,
            &|purse: &OwnerPurse| zeroed.get(&purse.custody).copied(),
            UnlistedRule::Strict,
            room_free,
            None,
        );
        let notes = OwnerNotes::default();
        let play = allocate(
            &owned,
            &|purse: &OwnerPurse| listed.get(&purse.custody).copied(),
            UnlistedRule::Play,
            room_free,
            Some(&notes),
        );
        prop_assert_eq!(&play, &strict);
        prop_assert_eq!(&play, &listed_at_zero);

        let noted = notes.drain();
        let noted_set: BTreeSet<[u8; 32]> = noted.iter().map(|purse| purse.custody).collect();
        prop_assert_eq!(noted.len(), noted_set.len(), "each purse is noted once");
        let expected: BTreeSet<[u8; 32]> = if room_free {
            consulted
                .borrow()
                .iter()
                .filter(|custody| !listed.contains_key(*custody))
                .copied()
                .collect()
        } else {
            BTreeSet::new()
        };
        prop_assert_eq!(noted_set, expected);
        prop_assert!(!notes.any());
    }

    /// Added by G3 (DR-135, G-4): `allocate_parts` decides as `allocate`
    /// does and draws the same total. The main part pays exactly the shares
    /// of the main charge, each restoration part pays exactly the shares of
    /// its restoration, and the parts sum to the total.
    #[test]
    fn allocate_parts_splits_the_draws_by_part(
        owned in arb_charge(),
        listed in arb_listed(),
        rule in arb_rule(),
        room_free in any::<bool>(),
    ) {
        let view = |purse: &OwnerPurse| listed.get(&purse.custody).copied();
        let whole = allocate(&owned, &view, rule, room_free, None);
        let parts = allocate_parts(&owned, &view, rule, room_free, None);
        prop_assert_eq!(whole.as_ref().ok(), parts.as_ref().ok().map(|draws| &draws.total));
        prop_assert_eq!(whole.is_ok(), parts.is_ok());
        if let Ok(parts) = parts {
            let paid = |draws: &ResourceMultiset<[u8; 32]>| -> u128 {
                draws.0.values().map(|amount| u128::from(*amount)).sum()
            };
            let owed = |charge: &OwnerCharge| -> u128 {
                charge.shares.iter().map(|share| share.amount).sum()
            };
            prop_assert_eq!(parts.restorations.len(), owned.restorations.len());
            prop_assert_eq!(paid(&parts.main), owed(&owned));
            let mut sum = parts.main.clone();
            for (part, (_, restoration)) in parts.restorations.iter().zip(&owned.restorations) {
                prop_assert_eq!(paid(part), owed(restoration));
                sum = sum.checked_add(part).expect("the parts of a granted allocation fit");
            }
            prop_assert_eq!(sum, parts.total);
        }
    }
}

/// The binding tells a client region from an owner region by the lane of
/// its signature. A Unit region is neither, and a region without a signature
/// is an error. The binding locates purses with its locator and keeps its
/// table.
#[test]
fn the_binding_tells_client_and_owner_regions_apart() {
    let client = ground(CLIENT);
    let owner = ground(1);
    let binding = binding(std::slice::from_ref(&client), OwnerTable::empty(2));
    let region = |signature: &CostSignature| {
        priced_cost_region(signature, b"binding", 0, 5).expect("a region")
    };
    assert!(binding
        .is_client_signature(&client)
        .expect("a canonical signature"));
    assert!(!binding
        .is_client_signature(&owner)
        .expect("a canonical signature"));
    assert!(!binding.is_owner_region(&region(&client)).expect("a region"));
    assert!(binding.is_owner_region(&region(&owner)).expect("a region"));
    assert!(!binding.is_owner_region(&region(&unit())).expect("a region"));
    assert_eq!(
        binding.is_owner_region(&CostRegion::default()),
        Err(AuthorityError::MissingSignature)
    );
    assert_eq!(
        binding.locate(&owner),
        Some(OwnerPurseSet::Single(purse(1)))
    );
    assert_eq!(binding.locate(&ground(FORGEABLE)), None);
    assert_eq!(binding.table(), &OwnerTable::empty(2));
}

// Added by G3 (DR-135, S3): the context of a fired body (R1, A1, R11).

/// A region of `signature` at `price`, with an identity from `entropy`.
fn priced(signature: &CostSignature, entropy: &[u8], price: u64) -> CostRegion {
    priced_cost_region(signature, entropy, 0, price).expect("a region")
}

/// A Unit residue region with identity `id` that records `caller`.
fn residue(id: &[u8], caller: Option<&RegionOwner>) -> CostRegion {
    CostRegion {
        instance_id: Blake2b256::hash(id.to_vec()),
        signature: Some(unit()),
        price: 0,
        caller: caller.map(|owner| CostCaller {
            signature: Some(owner.signature.clone()),
            price: owner.price,
        }),
    }
}

/// A seal of `regions` in the given order. A context reads regions in seal
/// order, so the tests keep the order explicit.
fn sealed(regions: Vec<CostRegion>) -> CostAuthority { CostAuthority { regions } }

/// A datum with seal `seal`.
fn datum(seal: CostAuthority) -> ListParWithRandom {
    ListParWithRandom {
        pars: Vec::new(),
        random_state: Vec::new(),
        cost_authority: Some(seal),
        cost_stack: None,
    }
}

fn region_owner(signature: CostSignature, price: u64) -> RegionOwner {
    RegionOwner { signature, price }
}

fn owner_context(signature: CostSignature, price: u64) -> BodyContext {
    BodyContext {
        system: false,
        owner: Some(region_owner(signature, price)),
        caller: None,
        // Added by G3 (DR-135, S3b): the data of these tests bind no value.
        held: HeldMap::default(),
    }
}

fn system_context(caller: Option<RegionOwner>) -> BodyContext {
    BodyContext {
        system: true,
        owner: None,
        caller,
        // Added by G3 (DR-135, S3b): the data of these tests bind no value.
        held: HeldMap::default(),
    }
}

/// Added by G3 (DR-135, S3b): a process, the shape of a value that carries
/// code.
fn code() -> Par {
    Par {
        sends: vec![models::rhoapi::Send::default()],
        ..Default::default()
    }
}

/// Added by G3 (DR-135, S3b): a datum with seal `seal` that binds `pars`.
fn binding_datum(seal: CostAuthority, pars: Vec<Par>) -> ListParWithRandom {
    ListParWithRandom {
        pars,
        ..datum(seal)
    }
}

/// A compound whose elements are out of canonical order.
fn unsorted_compound() -> CostSignature {
    let signature = CostSignature {
        value: Some(Value::Compound(CostSignatureCompound {
            elements: vec![ground(2), ground(1)],
        })),
    };
    assert_eq!(
        canonical_cost_signature(&signature),
        Err(AuthorityError::NonCanonicalSignature),
        "the fixture is not canonical"
    );
    signature
}

/// Without a binding, a fired body runs in the DR-101 context of dev (R11):
/// the system flag alone. The seals are not read, so even a malformed seal
/// gives that context.
#[test]
fn without_a_binding_the_body_context_is_the_dev_context() {
    let owner = sealed(vec![priced(&ground(1), b"owner", 7)]);
    let malformed = sealed(vec![CostRegion::default()]);
    let data = [datum(owner.clone()), datum(malformed.clone())];
    for system in [false, true] {
        for seal in [None, Some(&owner), Some(&malformed)] {
            assert_eq!(
                body_context(None, seal, &data, system).expect("no binding reads no seal"),
                BodyContext {
                    system,
                    owner: None,
                    caller: None,
                    // Added by G3 (DR-135, S3b): no held values (H7).
                    held: HeldMap::default(),
                }
            );
        }
    }
}

/// A fired owner body runs for the distinct canonical non-Unit signatures of
/// its seal (R1). One signature stays as it is. Several form their canonical
/// compound, whose flatten keeps the multiplicity of each atom. The price is
/// the minimum price of their regions. An owner on a client lane is no
/// owner, because the running deployment pays for its own lanes.
#[test]
fn an_owner_body_runs_for_the_owner_of_its_seal() {
    let binding = binding(&[ground(CLIENT)], OwnerTable::empty(2));
    let context = |regions: Vec<CostRegion>| {
        body_context(Some(&binding), Some(&sealed(regions)), &[], false)
            .expect("a well-formed seal")
    };
    assert_eq!(
        context(vec![priced(&ground(1), b"a", 7)]),
        owner_context(ground(1), 7)
    );
    assert_eq!(
        context(vec![
            priced(&ground(1), b"a", 9),
            priced(&ground(1), b"b", 7)
        ]),
        owner_context(ground(1), 7),
        "one signature in two regions is one owner at the lower price"
    );
    assert_eq!(
        context(vec![
            priced(&ground(2), b"a", 5),
            priced(&unit(), b"u", 1),
            priced(&ground(1), b"b", 7),
        ]),
        owner_context(compound(&[1, 2]), 5),
        "two signatures are their compound, and a Unit region names no owner"
    );
    assert_eq!(
        context(vec![
            priced(&compound(&[1, 2]), b"a", 3),
            priced(&ground(1), b"b", 4)
        ]),
        owner_context(compound(&[1, 1, 2]), 3),
        "the flatten of a compound and its atom keeps the repeated atom"
    );
    assert_eq!(
        context(vec![priced(&ground(CLIENT), b"a", 7)]),
        BodyContext::default(),
        "an owner on a client lane is no owner"
    );
    assert_eq!(
        context(vec![
            priced(&ground(CLIENT), b"a", 7),
            priced(&ground(1), b"b", 7)
        ]),
        owner_context(compound(&[1, CLIENT]), 7),
        "a compound with a client atom is not on the client lane"
    );
    assert_eq!(
        context(vec![priced(&unit(), b"u", 0)]),
        BodyContext::default()
    );
    assert_eq!(
        body_context(Some(&binding), None, &[], false).expect("no seal"),
        BodyContext::default()
    );
}

/// A fired system body runs for its caller (A1): the first caller that a
/// region of its seal records, else the first caller that a datum records,
/// in data order, else the single owner of the data seals at the minimum
/// price of its regions. A Unit caller names no owner, so the search goes
/// on. A caller on a client lane is no caller.
#[test]
fn a_system_body_runs_for_its_recorded_caller() {
    let binding = binding(&[ground(CLIENT)], OwnerTable::empty(2));
    let context = |seal: CostAuthority, data: &[ListParWithRandom]| {
        body_context(Some(&binding), Some(&seal), data, true).expect("well-formed seals")
    };
    let first = region_owner(ground(3), 11);
    let second = region_owner(ground(4), 5);
    let third = region_owner(ground(5), 6);
    let unit_caller = region_owner(unit(), 2);
    let plain = || sealed(vec![residue(b"plain", None)]);

    assert_eq!(
        context(
            sealed(vec![residue(b"r1", None), residue(b"r2", Some(&first))]),
            &[datum(sealed(vec![residue(b"d", Some(&second))]))],
        ),
        system_context(Some(first.clone())),
        "a caller of the seal comes before a caller of the data"
    );
    assert_eq!(
        context(plain(), &[
            datum(plain()),
            datum(sealed(vec![residue(b"d1", Some(&second))])),
            datum(sealed(vec![residue(b"d2", Some(&third))])),
        ]),
        // Changed by G3 (DR-135, S3b H6): a datum that binds no value carries
        // no code, so it adds no member to K.
        // system_context(Some(second.clone())),
        // "the first caller of the data in data order"
        system_context(None),
        "data that bind no value name no caller"
    );
    let owned_data = [
        datum(sealed(vec![
            residue(b"d", Some(&unit_caller)),
            priced(&ground(6), b"a", 8),
            priced(&ground(CLIENT), b"c", 1),
            priced(&unit(), b"u", 1),
        ])),
        datum(sealed(vec![priced(&ground(6), b"b", 6)])),
    ];
    assert_eq!(
        context(sealed(vec![residue(b"r", Some(&unit_caller))]), &owned_data),
        // Changed by G3 (DR-135, S3b H6): the data-owner fallback of S3 is
        // gone, and data that bind no value carry no code.
        // system_context(Some(region_owner(ground(6), 6))),
        // "without a recorded caller, the single owner of the data seals"
        system_context(None),
        "without a code-carrying datum, no caller"
    );
    assert_eq!(
        context(plain(), &[
            datum(sealed(vec![priced(&ground(6), b"a", 8)])),
            datum(sealed(vec![priced(&ground(7), b"b", 6)])),
        ]),
        system_context(None),
        "two owners of the data seals name no caller"
    );
    assert_eq!(
        context(
            sealed(vec![residue(b"r", Some(&region_owner(ground(CLIENT), 4)))]),
            &[datum(sealed(vec![priced(&ground(6), b"a", 8)]))],
        ),
        // Changed by G3 (DR-135, S3b H6): K keeps a caller on a client lane.
        // A held process of the body then runs for the running deployment.
        // system_context(None),
        // "a recorded caller on a client lane is no caller"
        system_context(Some(region_owner(ground(CLIENT), 4))),
        "a recorded caller on a client lane is the caller"
    );
    assert_eq!(context(plain(), &[]), system_context(None));
    assert_eq!(
        body_context(Some(&binding), None, &[], true).expect("no seal"),
        system_context(None)
    );
}

/// Whether `result` is the run error of a malformed stored seal.
fn is_run_error(result: Result<BodyContext, InterpreterError>) -> bool {
    match result {
        Err(InterpreterError::ReduceError(message)) => message.contains("malformed"),
        _ => false,
    }
}

/// A stored seal comes from the reducer, so a malformed region or recorded
/// caller is a run error, never a missing owner. A Unit caller is well
/// formed and names no owner.
#[test]
fn a_malformed_stored_seal_is_a_run_error() {
    let binding = binding(&[ground(CLIENT)], OwnerTable::empty(2));
    for region in [CostRegion::default(), CostRegion {
        signature: Some(unsorted_compound()),
        ..priced(&ground(1), b"a", 1)
    }] {
        let seal = sealed(vec![region.clone()]);
        assert!(is_run_error(body_context(
            Some(&binding),
            Some(&seal),
            &[],
            false
        )));
        // Changed by G3 (DR-135, S3b H6): Rule H reads the seal of a datum
        // that binds a value, so the malformed datum binds code.
        // assert!(is_run_error(body_context(
        //     Some(&binding),
        //     Some(&sealed(vec![residue(b"r", None)])),
        //     &[datum(seal)],
        //     true
        // )));
        assert!(is_run_error(body_context(
            Some(&binding),
            Some(&sealed(vec![residue(b"r", None)])),
            &[binding_datum(seal, vec![code()])],
            true
        )));
    }
    for caller in [
        CostCaller {
            signature: None,
            price: 1,
        },
        CostCaller {
            signature: Some(unsorted_compound()),
            price: 1,
        },
    ] {
        let region = CostRegion {
            caller: Some(caller),
            ..residue(b"r", None)
        };
        assert!(is_run_error(body_context(
            Some(&binding),
            Some(&sealed(vec![region.clone()])),
            &[],
            true
        )));
        // Changed by G3 (DR-135, S3b H6): the malformed datum binds code, so
        // Rule H reads its seal.
        // assert!(is_run_error(body_context(
        //     Some(&binding),
        //     Some(&sealed(vec![residue(b"s", None)])),
        //     &[datum(sealed(vec![region]))],
        //     true
        // )));
        assert!(is_run_error(body_context(
            Some(&binding),
            Some(&sealed(vec![residue(b"s", None)])),
            &[binding_datum(sealed(vec![region]), vec![code()])],
            true
        )));
    }
    assert_eq!(
        body_context(
            Some(&binding),
            Some(&sealed(vec![residue(b"r", Some(&region_owner(unit(), 1)))])),
            &[],
            true
        )
        .expect("a Unit caller is well formed"),
        system_context(None)
    );
}

/// Added by G3 (DR-135, S5): a continuation whose body is `body`, sealed by
/// `seal`.
fn continuation_of(body: Par, seal: Option<CostAuthority>) -> TaggedContinuation {
    TaggedContinuation {
        tagged_cont: Some(TaggedCont::ParBody(models::rhoapi::ParWithRandom {
            body: Some(body),
            random_state: Vec::new(),
        })),
        guard: None,
        cost_authority: seal,
    }
}

/// Added by G3 (DR-135, S5): a body that is one signed term, a held term when
/// `held_price` is set.
fn signed_body(signature: CostSignature, held_price: Option<u64>) -> Par {
    Par {
        cost_signed_terms: vec![CostSignedTerm {
            body: Some(Par::default()),
            signature: Some(signature),
            held_price,
        }],
        ..Par::default()
    }
}

/// Added by G3 (DR-135, S5, A5 and Rule H §13.5): the transferee of a COMM is
/// the signer of a continuation body that is one signed term. A held term
/// keeps its stored signature, also as the body of a system continuation. A
/// signer on a client lane is no transferee, and neither is a body that holds
/// more than one term. A forgeable signer has no purses. A surface signed
/// term that names a bound value takes the value from the data of the COMM:
/// the result changes when the bound name is on a client lane.
#[test]
fn the_transferee_of_a_comm_is_the_signer_of_its_body() {
    let budget =
        RuntimeBudget::new(crate::rust::interpreter::accounting::costs::Cost::unsafe_max());
    let client_name =
        models::rust::utils::new_gstring_par("client-name".to_string(), Vec::new(), false);
    let client_name_signature = CostSignature {
        value: Some(Value::Name(client_name.clone())),
    };
    let binding = binding(
        &[ground(CLIENT), client_name_signature],
        OwnerTable::empty(4),
    );
    let system_seal = CostAuthority {
        regions: vec![
            crate::rust::interpreter::accounting::authority::cost_region(&unit(), b"system", 0)
                .expect("a Unit region"),
        ],
    };
    let transferee = |body: Par, seal: Option<CostAuthority>, data: &[&ListParWithRandom]| {
        comm_transferee(&binding, &budget, &continuation_of(body, seal), data)
            .expect("a well-formed body")
    };
    assert_eq!(
        transferee(signed_body(ground(1), Some(3)), None, &[]),
        Some(locate(&ground(1)))
    );
    assert_eq!(
        transferee(signed_body(ground(1), Some(3)), Some(system_seal), &[]),
        Some(locate(&ground(1)))
    );
    assert_eq!(
        transferee(signed_body(ground(2), None), None, &[]),
        Some(locate(&ground(2)))
    );
    assert_eq!(
        transferee(signed_body(ground(CLIENT), None), None, &[]),
        None
    );
    assert_eq!(
        transferee(signed_body(ground(FORGEABLE), None), None, &[]),
        Some(None)
    );
    let mut two_terms = signed_body(ground(1), None);
    two_terms
        .cost_signed_terms
        .push(two_terms.cost_signed_terms[0].clone());
    assert_eq!(transferee(two_terms, None, &[]), None);
    let unsigned = Par::default();
    assert_eq!(transferee(unsigned, None, &[]), None);

    let bound = CostSignature {
        value: Some(Value::BoundLevel(0)),
    };
    let bound_to = |name: Par| ListParWithRandom {
        pars: vec![name],
        ..Default::default()
    };
    let other_name =
        models::rust::utils::new_gstring_par("other-name".to_string(), Vec::new(), false);
    assert_eq!(
        transferee(signed_body(bound.clone(), None), None, &[&bound_to(
            client_name
        )]),
        None,
        "the bound name is on a client lane"
    );
    assert_eq!(
        transferee(signed_body(bound, None), None, &[&bound_to(other_name)]),
        Some(None),
        "the bound name locates no purse of the test locator"
    );
}

/// Added by G3 (DR-135, S6): a random owner charge like `arb_charge`, whose
/// amounts stay far below `u64::MAX`. So no purse ever draws `u64::MAX`, and
/// a budget of `u64::MAX` stands for an unbounded budget in the oracle of
/// `the_unlisted_rule_matches_a_brute_force_oracle`.
fn arb_small_charge() -> impl Strategy<Value = OwnerCharge> {
    let purse_set = prop_oneof![
        Just(None),
        (1_u8..8).prop_map(|byte| Some(OwnerPurseSet::Single(purse(byte)))),
        (1_u8..8, prop::collection::vec(1_u8..8, 2..4)).prop_map(|(vault, atoms)| {
            Some(OwnerPurseSet::Compound {
                vault: purse(vault),
                atoms: atoms.into_iter().map(purse).collect::<Vec<_>>().into(),
            })
        }),
    ];
    let share = (purse_set.clone(), 0_u64..200).prop_map(|(purses, part)| {
        let units = match purses.as_ref() {
            Some(OwnerPurseSet::Compound { atoms, .. }) => atoms.len() as u64,
            _ => 1,
        };
        OwnerRegionShare {
            purses,
            units,
            usage: part,
            amount: u128::from(part) * u128::from(units),
        }
    });
    let shares = prop::collection::vec(share, 0..5);
    (
        shares.clone(),
        prop_oneof![Just(None), purse_set.prop_map(Some)],
        prop::collection::vec(shares, 0..3),
    )
        .prop_map(|(shares, transferee, restorations)| OwnerCharge {
            shares,
            transferee,
            restorations: restorations
                .into_iter()
                .enumerate()
                .map(|(index, shares)| ([index as u8; 32], charge(shares)))
                .collect(),
        })
}

/// Added by G3 (DR-135, S6): the custody of every purse that `owned` names in
/// its shares, its restorations and its transferee.
fn named_purses(owned: &OwnerCharge) -> BTreeSet<[u8; 32]> {
    let mut purses = BTreeSet::new();
    let mut sets: Vec<&OwnerPurseSet> = every_share(owned)
        .into_iter()
        .filter_map(|share| share.purses.as_ref())
        .collect();
    if let Some(Some(transferee)) = owned.transferee.as_ref() {
        sets.push(transferee);
    }
    for set in sets {
        match set {
            OwnerPurseSet::Single(purse) => {
                purses.insert(purse.custody);
            }
            OwnerPurseSet::Compound { vault, atoms } => {
                purses.insert(vault.custody);
                purses.extend(atoms.iter().map(|atom| atom.custody));
            }
        }
    }
    purses
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Added by G3 (DR-135, S6, SA8): each replay decision follows its
    /// unlisted rule, by a brute-force oracle. The oracle gives each unlisted
    /// purse of the charge every budget of {0, 1, u64::MAX} in turn and
    /// allocates with every purse listed. With room in the table, the item
    /// test of the Stored rule passes when some assignment pays, and a
    /// refusal shows a shortfall when no assignment pays. The item test of a
    /// refused tuple passes when the assignment of 0 pays. A full table gives
    /// every unlisted purse 0.
    #[test]
    fn the_unlisted_rule_matches_a_brute_force_oracle(
        owned in arb_small_charge(),
        listed in arb_listed(),
        room_free in any::<bool>(),
    ) {
        const BUDGETS: [u64; 3] = [0, 1, u64::MAX];
        let unlisted: Vec<[u8; 32]> = named_purses(&owned)
            .into_iter()
            .filter(|custody| !listed.contains_key(custody))
            .collect();
        let pays = |assignment: &[u64]| {
            let view = |purse: &OwnerPurse| {
                listed.get(&purse.custody).copied().or_else(|| {
                    unlisted
                        .iter()
                        .position(|custody| *custody == purse.custody)
                        .map(|slot| assignment[slot])
                })
            };
            allocate(&owned, &view, UnlistedRule::Strict, false, None).is_ok()
        };
        let mut assignment = vec![0_u64; unlisted.len()];
        let zero_pays = pays(&assignment);
        let mut some_pays = zero_pays;
        let assignments = BUDGETS.len().pow(unlisted.len() as u32);
        for index in 1..assignments {
            if some_pays {
                break;
            }
            let mut rest = index;
            for slot in assignment.iter_mut() {
                *slot = BUDGETS[rest % BUDGETS.len()];
                rest /= BUDGETS.len();
            }
            some_pays = pays(&assignment);
        }
        let lenient_pays = match room_free {
            true => some_pays,
            false => zero_pays,
        };
        let view = |purse: &OwnerPurse| listed.get(&purse.custody).copied();
        prop_assert_eq!(stored_item_passes(&owned, &view, room_free), lenient_pays);
        prop_assert_eq!(refused_item_passes(&owned, &view, room_free), zero_pays);
        prop_assert_eq!(refusal_shortfall(&owned, &view, room_free), !lenient_pays);
    }
}
