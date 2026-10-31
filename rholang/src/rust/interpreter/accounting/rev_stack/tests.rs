use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rhoapi::{BindPattern, GPrivate, ListParWithRandom, Par, TaggedContinuation};
use proptest::prelude::*;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use super::cell::{cell_amount, make_cell};
use super::*;
use crate::rust::interpreter::matcher::r#match::Matcher;
use crate::rust::interpreter::util::vault_address::VaultAddress;

#[derive(Default)]
struct MemCells(Mutex<HashMap<Par, Vec<Datum<ListParWithRandom>>>>);

impl CellStore for MemCells {
    fn get_data(&self, channel: &Par) -> Vec<Datum<ListParWithRandom>> {
        self.0
            .lock()
            .unwrap()
            .get(channel)
            .cloned()
            .unwrap_or_default()
    }

    fn put_datum(&self, channel: &Par, datum: Datum<ListParWithRandom>) {
        self.0
            .lock()
            .unwrap()
            .entry(channel.clone())
            .or_default()
            .insert(0, datum);
    }

    fn remove_datum(&self, channel: &Par, index: i32) -> Result<(), RevStackError> {
        let mut map = self.0.lock().unwrap();
        let data = map.entry(channel.clone()).or_default();
        if index < 0 || index as usize >= data.len() {
            return Err(RevStackError::Store(format!("index {index} out of bounds")));
        }
        data.remove(index as usize);
        Ok(())
    }
}

fn mem_stacks() -> (Arc<MemCells>, RevStacks) {
    let store = Arc::new(MemCells::default());
    let stacks = RevStacks::new(store.clone());
    (store, stacks)
}

type RhoSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

async fn rspace() -> RhoSpace {
    let mut kvm = InMemoryStoreManager::new();
    let store = kvm.r_space_stores().await.unwrap();
    RSpace::create(store, Arc::new(Box::new(Matcher))).unwrap()
}

fn rspace_stacks(space: &RhoSpace) -> RevStacks { RevStacks::over_hot_store(space.get_store()) }

async fn root(space: &RhoSpace) -> Blake2b256Hash { space.create_checkpoint().await.unwrap().root }

fn key_owner() -> (Vec<u8>, WalletOwner) {
    let (_sk, pk) = Secp256k1.new_key_pair();
    let bytes = pk.bytes.to_vec();
    (
        bytes.clone(),
        WalletOwner::from_deployer_id(&bytes).unwrap(),
    )
}

fn unforgeable(id: &[u8]) -> WalletOwner {
    WalletOwner::from_unforgeable(&GPrivate { id: id.to_vec() })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn deposits_commute(amounts in prop::collection::vec(1u64..1_000_000, 1..24), seed in any::<u64>()) {
        let owner = unforgeable(b"wallet");
        let (store_a, a) = mem_stacks();
        let (store_b, b) = mem_stacks();

        for x in &amounts {
            a.deposit(&owner, *x).unwrap();
        }
        let mut shuffled = amounts.clone();
        let mut s = seed;
        for i in (1..shuffled.len()).rev() {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            shuffled.swap(i, (s >> 33) as usize % (i + 1));
        }
        for x in &shuffled {
            b.deposit(&owner, *x).unwrap();
        }

        let total: u64 = amounts.iter().sum();
        prop_assert_eq!(a.balance(&owner).unwrap(), total);
        prop_assert_eq!(b.balance(&owner).unwrap(), total);
        prop_assert_eq!(store_a.get_data(&wallet_channel(&owner)).len(), amounts.len());

        prop_assert_eq!(a.combine(&owner).unwrap(), total);
        prop_assert_eq!(b.combine(&owner).unwrap(), total);
        let cells_a = store_a.get_data(&wallet_channel(&owner));
        let cells_b = store_b.get_data(&wallet_channel(&owner));
        prop_assert_eq!(cells_a.len(), 1);
        prop_assert_eq!(&cells_a, &cells_b);
    }
}

#[tokio::test]
async fn deposits_commute_to_same_state_root() {
    let alice = unforgeable(b"alice");
    let bob = unforgeable(b"bob");
    let deposits = [
        (&alice, 5u64),
        (&bob, 7),
        (&alice, 11),
        (&bob, 13),
        (&alice, 17),
    ];

    let s1 = rspace().await;
    let r1 = rspace_stacks(&s1);
    for (w, x) in deposits.iter() {
        r1.deposit(w, *x).unwrap();
    }
    let s2 = rspace().await;
    let r2 = rspace_stacks(&s2);
    for (w, x) in deposits.iter().rev() {
        r2.deposit(w, *x).unwrap();
    }
    for r in [&r1, &r2] {
        assert_eq!(r.combine(&alice).unwrap(), 33);
        assert_eq!(r.combine(&bob).unwrap(), 20);
    }
    assert_eq!(root(&s1).await, root(&s2).await);

    let s3 = rspace().await;
    let r3 = rspace_stacks(&s3);
    r3.deposit(&alice, 33).unwrap();
    r3.deposit(&bob, 20).unwrap();
    assert_eq!(root(&s1).await, root(&s3).await);
}

#[tokio::test]
async fn concurrent_deposits_all_land() {
    let space = rspace().await;
    let stacks = rspace_stacks(&space);
    let payee = unforgeable(b"payee");
    std::thread::scope(|scope| {
        for t in 0..8u64 {
            let (stacks, payee) = (&stacks, &payee);
            scope.spawn(move || {
                for i in 1..=50u64 {
                    stacks.deposit(payee, t * 100 + i).unwrap();
                }
            });
        }
    });
    let expected: u64 = (0..8u64)
        .map(|t| (1..=50u64).map(|i| t * 100 + i).sum::<u64>())
        .sum();
    assert_eq!(stacks.balance(&payee).unwrap(), expected);
    assert_eq!(
        space.get_store().get_data(&wallet_channel(&payee)).len(),
        400
    );
}

#[test]
fn spend_combines_and_refuses_overdraft() {
    let (store, stacks) = mem_stacks();
    let w = unforgeable(b"w");
    for x in [10, 20, 30] {
        stacks.deposit(&w, x).unwrap();
    }
    assert_eq!(stacks.spend(&w, 25).unwrap(), 35);
    assert_eq!(store.get_data(&wallet_channel(&w)).len(), 1);

    assert_eq!(
        stacks.spend(&w, 40),
        Err(RevStackError::InsufficientFunds {
            balance: 35,
            requested: 40
        })
    );
    assert_eq!(stacks.balance(&w).unwrap(), 35);

    assert_eq!(stacks.spend(&w, 35).unwrap(), 0);
    assert!(store.get_data(&wallet_channel(&w)).is_empty());
    assert_eq!(stacks.balance(&w).unwrap(), 0);
}

#[tokio::test]
async fn spends_from_one_wallet_serialize() {
    let space = rspace().await;
    let stacks = rspace_stacks(&space);
    let w = unforgeable(b"hot-wallet");
    let other = unforgeable(b"other-wallet");
    stacks.deposit(&w, 1_000).unwrap();
    stacks.deposit(&other, 1_000).unwrap();

    let spent = Mutex::new(0u64);
    let refused = Mutex::new(0u64);
    std::thread::scope(|scope| {
        for t in 0..8u64 {
            let (stacks, w, other, spent, refused) = (&stacks, &w, &other, &spent, &refused);
            scope.spawn(move || {
                for i in 0..40u64 {
                    let amount = 1 + (t * 7 + i * 3) % 13;
                    match stacks.spend(w, amount) {
                        Ok(_) => *spent.lock().unwrap() += amount,
                        Err(RevStackError::InsufficientFunds { .. }) => {
                            *refused.lock().unwrap() += 1
                        }
                        Err(e) => panic!("unexpected error: {e}"),
                    }
                    stacks.spend(other, 1).ok();
                }
            });
        }
        let (stacks, w) = (&stacks, &w);
        scope.spawn(move || {
            for _ in 0..100 {
                stacks.deposit(w, 2).unwrap();
            }
        });
    });

    let spent = *spent.lock().unwrap();
    assert!(
        *refused.lock().unwrap() > 0,
        "the test should drive the wallet dry"
    );
    assert_eq!(stacks.balance(&w).unwrap(), 1_000 + 200 - spent);
    assert_eq!(stacks.balance(&other).unwrap(), 1_000 - 320);
}

#[test]
fn key_and_wallets_txt_address_find_same_wallet() {
    let (pk_bytes, from_key) = key_owner();

    let address = VaultAddress::from_deployer_id(pk_bytes.clone())
        .unwrap()
        .to_base58();
    let wallets_txt = format!("{address},1000000\n");
    let line = wallets_txt.lines().next().unwrap();
    let (addr_field, balance_field) = line.split_once(',').unwrap();
    let from_file = WalletOwner::from_address(addr_field).unwrap();

    assert_eq!(from_key, from_file);
    assert_eq!(wallet_channel(&from_key), wallet_channel(&from_file));

    let (_, stacks) = mem_stacks();
    stacks
        .deposit(&from_file, balance_field.parse().unwrap())
        .unwrap();
    assert_eq!(stacks.balance(&from_key).unwrap(), 1_000_000);
}

#[test]
fn distinct_owners_have_distinct_wallets() {
    let (_, k1) = key_owner();
    let (_, k2) = key_owner();
    let u1 = unforgeable(b"u1");
    let u2 = unforgeable(b"u2");
    let owners = [
        k1.clone(),
        k2.clone(),
        u1.clone(),
        u2.clone(),
        WalletOwner::compound(vec![k1, u1]).unwrap(),
    ];
    for (i, a) in owners.iter().enumerate() {
        for b in owners.iter().skip(i + 1) {
            assert_ne!(wallet_channel(a), wallet_channel(b), "{a:?} vs {b:?}");
        }
    }
}

#[test]
fn unforgeable_wallet_is_not_the_name_itself() {
    let name = GPrivate {
        id: b"some-name".to_vec(),
    };
    let owner = WalletOwner::from_unforgeable(&name);
    assert_ne!(wallet_channel_id(&owner), name.id);
}

#[test]
fn invalid_owners_are_refused() {
    assert!(WalletOwner::from_address("not-an-address").is_err());
    assert!(WalletOwner::from_deployer_id(&[1, 2, 3]).is_err());
    assert!(WalletOwner::compound(vec![]).is_err());
}

#[test]
fn compound_is_canonical() {
    let (_, k) = key_owner();
    let a = unforgeable(b"a");
    let b = unforgeable(b"b");

    let ab = WalletOwner::compound(vec![a.clone(), b.clone()]).unwrap();
    let ba = WalletOwner::compound(vec![b.clone(), a.clone()]).unwrap();
    assert_eq!(wallet_channel(&ab), wallet_channel(&ba));

    let left = WalletOwner::compound(vec![ab.clone(), k.clone()]).unwrap();
    let right = WalletOwner::compound(vec![
        a.clone(),
        WalletOwner::compound(vec![b.clone(), k.clone()]).unwrap(),
    ])
    .unwrap();
    let flat = WalletOwner::compound(vec![k.clone(), b.clone(), a.clone()]).unwrap();
    assert_eq!(left, right);
    assert_eq!(left, flat);

    assert_eq!(WalletOwner::compound(vec![a.clone()]).unwrap(), a);
    let aa = WalletOwner::compound(vec![a.clone(), a.clone()]).unwrap();
    assert_ne!(wallet_channel(&aa), wallet_channel(&a));
}

#[test]
fn cells_round_trip_and_reject_bad_amounts() {
    let channel = wallet_channel(&unforgeable(b"c"));
    let cell = make_cell(&channel, 42).unwrap();
    assert_eq!(cell_amount(&cell), Some(42));
    assert!(!cell.persist);
    assert_eq!(make_cell(&channel, 0), Err(RevStackError::InvalidAmount(0)));
    assert_eq!(
        make_cell(&channel, MAX_AMOUNT + 1),
        Err(RevStackError::InvalidAmount(MAX_AMOUNT + 1))
    );
    assert!(make_cell(&channel, MAX_AMOUNT).is_ok());
}

#[test]
fn zero_deposit_writes_nothing() {
    let (store, stacks) = mem_stacks();
    let w = unforgeable(b"z");
    stacks.deposit(&w, 0).unwrap();
    assert!(store.get_data(&wallet_channel(&w)).is_empty());
}

#[test]
fn overflowing_wallet_is_refused() {
    let (_, stacks) = mem_stacks();
    let w = unforgeable(b"big");
    stacks.deposit(&w, MAX_AMOUNT).unwrap();
    stacks.deposit(&w, 1).unwrap();
    assert_eq!(stacks.balance(&w), Err(RevStackError::Overflow));
    assert_eq!(stacks.spend(&w, 1), Err(RevStackError::Overflow));
}

#[test]
fn foreign_datum_marks_wallet_corrupt() {
    let (store, stacks) = mem_stacks();
    let w = unforgeable(b"corrupt");
    let channel = wallet_channel(&w);
    let mut bad = make_cell(&channel, 5).unwrap();
    bad.persist = true;
    store.put_datum(&channel, bad);
    assert!(matches!(
        stacks.balance(&w),
        Err(RevStackError::CorruptWallet(_))
    ));
}

#[test]
fn term_builders_accept_only_valid_addresses_and_amounts() {
    let (pk, _) = key_owner();
    let address = VaultAddress::from_deployer_id(pk).unwrap().to_base58();

    let transfer = super::native::transfer_term(&address, 5).unwrap();
    assert!(transfer.contains(&format!("\"{address}\", 5")));
    assert!(super::native::balance_term(&address)
        .unwrap()
        .contains(&address));

    let injected = format!("{address}\"), *deployId) | @\"x\"!(\"y");
    for bad in ["", "lala", injected.as_str()] {
        assert!(super::native::transfer_term(bad, 5).is_err(), "{bad}");
        assert!(super::native::balance_term(bad).is_err(), "{bad}");
    }
    assert!(super::native::transfer_term(&address, 0).is_err());
    assert!(super::native::transfer_term(&address, -1).is_err());
}

#[test]
fn balance_results_parse() {
    use crate::rust::interpreter::rho_type::{RhoBoolean, RhoNumber, RhoString, RhoTuple2};
    let ok = RhoTuple2::create_par((RhoBoolean::create_par(true), RhoNumber::create_par(42)));
    assert_eq!(super::native::parse_balance(&ok), Ok(42));
    let failed = RhoTuple2::create_par((
        RhoBoolean::create_par(false),
        RhoString::create_par("invalid".to_string()),
    ));
    assert_eq!(
        super::native::parse_balance(&failed),
        Err("invalid".to_string())
    );
    assert!(super::native::parse_balance(&RhoNumber::create_par(1)).is_err());
}
