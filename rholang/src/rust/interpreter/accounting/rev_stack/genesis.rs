use std::collections::BTreeMap;
use std::sync::Arc;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, ListParWithRandom, Par};
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::trace::event::Produce;

use super::hooks::{install_genesis_hooks, HookAuthority};
use super::native::RhoHotStore;
use super::ops::{CellStore, HotStoreCells, RevStacks};
use super::owner::WalletOwner;
use super::{RevAmount, RevStackError, MAX_AMOUNT};
use crate::rust::interpreter::rho_type::{RhoBoolean, RhoList, RhoNumber, RhoString, RhoTuple2};

pub const REV_GENESIS_URN: &str = "rho:rev:genesis";

pub const SUPPLY_CHANNEL_DOMAIN: &[u8] = b"f1r3fly/rev-stack/genesis-supply/v1";

pub const SUPPLY_TAG: &str = "supply";

pub fn is_genesis_only_urn(urn: &str) -> bool { urn == REV_GENESIS_URN }

pub fn supply_channel() -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
            id: Blake2b256::hash(SUPPLY_CHANNEL_DOMAIN.to_vec()),
        })),
    }])
}

pub fn genesis_supply(store: &dyn CellStore) -> Result<RevAmount, RevStackError> {
    let data = store.get_data(&supply_channel());
    match data.as_slice() {
        [] => Ok(0),
        [datum] => supply_amount(datum).ok_or_else(|| {
            RevStackError::CorruptWallet("the genesis supply datum is malformed".into())
        }),
        _ => Err(RevStackError::CorruptWallet(
            "more than one genesis supply datum".into(),
        )),
    }
}

pub fn mint(
    store: Arc<dyn CellStore>,
    lines: &[(WalletOwner, RevAmount)],
) -> Result<RevAmount, RevStackError> {
    let mut batch: RevAmount = 0;
    let mut per_wallet: BTreeMap<Vec<u8>, (WalletOwner, RevAmount)> = BTreeMap::new();
    for (owner, amount) in lines {
        if *amount == 0 || *amount > MAX_AMOUNT {
            return Err(RevStackError::InvalidAmount(*amount));
        }
        batch = add(batch, *amount)?;
        let entry = per_wallet
            .entry(owner.canonical_bytes())
            .or_insert_with(|| (owner.clone(), 0));
        entry.1 = add(entry.1, *amount)?;
    }
    let before = genesis_supply(store.as_ref())?;
    let after = add(before, batch)?;
    let stacks = RevStacks::new(store.clone());
    for (owner, amount) in per_wallet.values() {
        let held = stacks.balance(owner)?;
        add(held, *amount)?;
    }
    for (owner, amount) in per_wallet.values() {
        stacks.deposit(owner, *amount)?;
        stacks.combine(owner)?;
    }
    let channel = supply_channel();
    let existing = store.get_data(&channel).len();
    for index in (0..existing).rev() {
        store.remove_datum(&channel, index as i32)?;
    }
    store.put_datum(&channel, make_supply_datum(&channel, after));
    Ok(after)
}

pub fn handle(store: Option<RhoHotStore>, command: &str, arg: &Par) -> Option<Par> {
    let result = match command {
        "mint" => mint_args(arg).and_then(|lines| {
            let store = store.ok_or_else(|| "native REV layer unavailable".to_string())?;
            let cells: Arc<dyn CellStore> = Arc::new(HotStoreCells(store));
            mint(cells, &lines)
                .map(|supply| RhoNumber::create_par(supply as i64))
                .map_err(|e| e.to_string())
        }),
        "hooks" => hooks_args(arg).and_then(|authority| {
            let store = store.ok_or_else(|| "native REV layer unavailable".to_string())?;
            install_genesis_hooks(&HotStoreCells(store), &authority)
                .map(|_| Par::default())
                .map_err(|e| e.to_string())
        }),
        _ => return None,
    };
    Some(match result {
        Ok(value) => RhoTuple2::create_par((RhoBoolean::create_par(true), value)),
        Err(message) => RhoTuple2::create_par((
            RhoBoolean::create_par(false),
            RhoString::create_par(message),
        )),
    })
}

fn mint_args(arg: &Par) -> Result<Vec<(WalletOwner, RevAmount)>, String> {
    let usage = "mint expects a list of (revAddress, amount)";
    let items = RhoList::unapply(arg).ok_or_else(|| usage.to_string())?;
    items
        .iter()
        .map(|item| {
            let (address, amount) = RhoTuple2::unapply(item).ok_or_else(|| usage.to_string())?;
            let address = RhoString::unapply(&address).ok_or_else(|| usage.to_string())?;
            let owner = WalletOwner::from_address(&address).map_err(|e| e.to_string())?;
            let amount = RhoNumber::unapply(&amount)
                .filter(|a| *a > 0)
                .ok_or_else(|| "a genesis amount must be positive".to_string())?;
            Ok((owner, amount as RevAmount))
        })
        .collect()
}

fn hooks_args(arg: &Par) -> Result<HookAuthority, String> {
    let usage = "hooks expects (threshold, [revAddress, ...])";
    let (threshold, members) = RhoTuple2::unapply(arg).ok_or_else(|| usage.to_string())?;
    let threshold = RhoNumber::unapply(&threshold)
        .and_then(|t| u32::try_from(t).ok())
        .ok_or_else(|| usage.to_string())?;
    let members = RhoList::unapply(&members)
        .ok_or_else(|| usage.to_string())?
        .iter()
        .map(|m| {
            let address = RhoString::unapply(m).ok_or_else(|| usage.to_string())?;
            WalletOwner::from_address(&address).map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    HookAuthority::new(members, threshold).map_err(|e| e.to_string())
}

fn add(a: RevAmount, b: RevAmount) -> Result<RevAmount, RevStackError> {
    a.checked_add(b)
        .filter(|t| *t <= MAX_AMOUNT)
        .ok_or(RevStackError::Overflow)
}

fn make_supply_datum(channel: &Par, amount: RevAmount) -> Datum<ListParWithRandom> {
    let a = ListParWithRandom {
        pars: vec![RhoTuple2::create_par((
            RhoString::create_par(SUPPLY_TAG.to_string()),
            RhoNumber::create_par(amount as i64),
        ))],
        random_state: Vec::new(),
    };
    let source = Produce::create(channel, &a, false);
    Datum {
        a,
        persist: false,
        source,
    }
}

fn supply_amount(datum: &Datum<ListParWithRandom>) -> Option<RevAmount> {
    if datum.persist {
        return None;
    }
    let [par] = datum.a.pars.as_slice() else {
        return None;
    };
    let (tag, amount) = RhoTuple2::unapply(par)?;
    if RhoString::unapply(&tag)?.as_str() != SUPPLY_TAG {
        return None;
    }
    RhoNumber::unapply(&amount)
        .filter(|a| *a >= 0)
        .map(|a| a as RevAmount)
}
