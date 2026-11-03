use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, ListParWithRandom, Par};
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::trace::event::Produce;

use std::collections::BTreeMap;
use std::sync::Arc;

use super::kind::TokenKind;
use super::ops::{CellStore, RevStacks};
use super::reserve::reservation;
use super::owner::WalletOwner;
use super::{RevAmount, RevStackError};
use crate::rust::interpreter::rho_type::{RhoByteArray, RhoList, RhoNumber, RhoString};

pub const JOURNAL_DOMAIN: &[u8] = b"f1r3fly/rev-stack/journal/v1";
pub const JOURNAL_MARKER_DOMAIN: &[u8] = b"f1r3fly/rev-stack/journal-marker/v1";

const TRANSFER_TAG: &str = "transfer";
const MINT_TAG: &str = "mint";
const REFUSE_TAG: &str = "refuse";
const MARKER_TAG: &str = "journal";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Mint,
    Wallet(WalletOwner),
    Share {
        process: WalletOwner,
        funder: WalletOwner,
    },
}

impl Source {
    pub fn payer(&self) -> Option<&WalletOwner> {
        match self {
            Source::Mint => None,
            Source::Wallet(from) => Some(from),
            Source::Share { process, .. } => Some(process),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalEntry {
    pub kind: TokenKind,
    pub from: Source,
    pub to: WalletOwner,
    pub amount: RevAmount,
}

impl JournalEntry {
    pub fn canonical_key(&self) -> (Vec<u8>, Vec<u8>, Vec<u8>, RevAmount) {
        let source = match &self.from {
            Source::Mint => vec![0],
            Source::Wallet(from) => [vec![1], from.canonical_bytes()].concat(),
            Source::Share { process, funder } => {
                [vec![2], process.canonical_bytes(), funder.canonical_bytes()].concat()
            }
        };
        (
            self.kind.name().to_vec(),
            source,
            self.to.canonical_bytes(),
            self.amount,
        )
    }

    pub fn transfer(from: WalletOwner, to: WalletOwner, amount: RevAmount) -> Self {
        Self::transfer_of(TokenKind::rev(), from, to, amount)
    }

    pub fn transfer_of(
        kind: TokenKind,
        from: WalletOwner,
        to: WalletOwner,
        amount: RevAmount,
    ) -> Self {
        JournalEntry {
            kind,
            from: Source::Wallet(from),
            to,
            amount,
        }
    }

    pub fn mint(
        kind: TokenKind,
        to: WalletOwner,
        amount: RevAmount,
    ) -> Result<Self, RevStackError> {
        let entry = JournalEntry {
            kind,
            from: Source::Mint,
            to,
            amount,
        };
        entry.check()?;
        Ok(entry)
    }

    pub fn refusal(process: WalletOwner, funder: WalletOwner, amount: RevAmount) -> Self {
        JournalEntry {
            kind: TokenKind::rev(),
            from: Source::Share {
                process,
                funder: funder.clone(),
            },
            to: funder,
            amount,
        }
    }

    pub fn is_mint(&self) -> bool { self.from == Source::Mint }

    pub fn check(&self) -> Result<(), RevStackError> {
        if self.amount == 0 || self.amount > super::MAX_AMOUNT {
            return Err(RevStackError::InvalidAmount(self.amount));
        }
        if self.is_mint() && self.kind.is_rev() {
            return Err(RevStackError::RevMint);
        }
        if let Source::Share { funder, .. } = &self.from {
            if !self.kind.is_rev() || *funder != self.to {
                return Err(RevStackError::InvalidOwner(
                    "a refused share returns REV to its funder".into(),
                ));
            }
        }
        Ok(())
    }
}

fn channel(domain: &[u8]) -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
            id: Blake2b256::hash(domain.to_vec()),
        })),
    }])
}

pub fn journal_channel() -> Par { channel(JOURNAL_DOMAIN) }

pub fn marker_channel() -> Par { channel(JOURNAL_MARKER_DOMAIN) }

fn datum(channel: &Par, pars: Vec<Par>) -> Datum<ListParWithRandom> {
    let a = ListParWithRandom {
        pars,
        random_state: Vec::new(),
        seal: None,
    };
    let source = Produce::create(channel, &a, false);
    Datum {
        a,
        persist: false,
        source,
    }
}

fn clear(store: &dyn CellStore, channel: &Par) -> Result<(), RevStackError> {
    let n = store.get_data(channel).len();
    for index in (0..n).rev() {
        store.remove_datum(channel, index as i32)?;
    }
    Ok(())
}

pub fn open_journal(store: &dyn CellStore) -> Result<(), RevStackError> {
    clear(store, &journal_channel())?;
    clear(store, &marker_channel())?;
    let marker = marker_channel();
    store.put_datum(
        &marker,
        datum(&marker, vec![RhoString::create_par(MARKER_TAG.into())]),
    );
    Ok(())
}

pub fn journal_open(store: &dyn CellStore) -> bool { !store.get_data(&marker_channel()).is_empty() }

pub fn record(store: &dyn CellStore, entry: &JournalEntry) -> Result<(), RevStackError> {
    entry.check()?;
    let ch = journal_channel();
    let mut items = vec![
        RhoString::create_par(TRANSFER_TAG.into()),
        RhoByteArray::create_par(entry.kind.name().to_vec()),
    ];
    match &entry.from {
        Source::Mint => items[0] = RhoString::create_par(MINT_TAG.into()),
        Source::Wallet(from) => items.push(RhoByteArray::create_par(from.canonical_bytes())),
        Source::Share { process, funder } => {
            items[0] = RhoString::create_par(REFUSE_TAG.into());
            items.push(RhoByteArray::create_par(process.canonical_bytes()));
            items.push(RhoByteArray::create_par(funder.canonical_bytes()));
        }
    }
    items.push(RhoByteArray::create_par(entry.to.canonical_bytes()));
    items.push(RhoNumber::create_par(entry.amount as i64));
    let pars = vec![RhoList::create_par(items)];
    store.put_datum(&ch, datum(&ch, pars));
    Ok(())
}

pub fn entries(store: &dyn CellStore) -> Result<Vec<JournalEntry>, RevStackError> {
    let malformed = || RevStackError::CorruptWallet("a journal entry is malformed".into());
    let mut out = Vec::new();
    for d in store.get_data(&journal_channel()) {
        let [par] = d.a.pars.as_slice() else {
            return Err(malformed());
        };
        let items = RhoList::unapply(par).ok_or_else(malformed)?;
        let owner = |p: &Par| {
            RhoByteArray::unapply(p)
                .and_then(|b| WalletOwner::from_canonical_bytes(&b))
                .ok_or_else(malformed)
        };
        let (kind, from, to, amount) = match items.as_slice() {
            [tag, kind, from, to, amount]
                if RhoString::unapply(tag).as_deref() == Some(TRANSFER_TAG) =>
            {
                (kind, Source::Wallet(owner(from)?), to, amount)
            }
            [tag, kind, to, amount]
                if RhoString::unapply(tag).as_deref() == Some(MINT_TAG) =>
            {
                (kind, Source::Mint, to, amount)
            }
            [tag, kind, process, funder, to, amount]
                if RhoString::unapply(tag).as_deref() == Some(REFUSE_TAG) =>
            {
                (
                    kind,
                    Source::Share {
                        process: owner(process)?,
                        funder: owner(funder)?,
                    },
                    to,
                    amount,
                )
            }
            _ => return Err(malformed()),
        };
        let kind = RhoByteArray::unapply(kind)
            .and_then(TokenKind::from_name)
            .ok_or_else(malformed)?;
        let amount = RhoNumber::unapply(amount)
            .filter(|a| *a > 0)
            .ok_or_else(malformed)?;
        let entry = JournalEntry {
            kind,
            from,
            to: owner(to)?,
            amount: amount as RevAmount,
        };
        entry.check().map_err(|_| malformed())?;
        out.push(entry);
    }
    out.sort_by_cached_key(JournalEntry::canonical_key);
    Ok(out)
}

pub fn net(
    store: &dyn CellStore,
    kind: &TokenKind,
    owner: &WalletOwner,
) -> Result<(RevAmount, RevAmount), RevStackError> {
    let mut debits: RevAmount = 0;
    let mut credits: RevAmount = 0;
    for e in entries(store)?.into_iter().filter(|e| &e.kind == kind) {
        if e.from.payer() == Some(owner) {
            debits = debits.saturating_add(e.amount);
        }
        if &e.to == owner {
            credits = credits.saturating_add(e.amount);
        }
    }
    Ok((debits, credits))
}

pub fn close_journal(store: &dyn CellStore) -> Result<Vec<JournalEntry>, RevStackError> {
    let taken = entries(store)?;
    clear(store, &journal_channel())?;
    clear(store, &marker_channel())?;
    Ok(taken)
}

fn spendable(balance: RevAmount, held: RevAmount, requested: RevAmount) -> Result<(), RevStackError> {
    let available = balance.saturating_sub(held);
    if requested > available {
        return Err(RevStackError::InsufficientFunds {
            balance: available,
            requested,
        });
    }
    Ok(())
}

pub fn journaled_transfer(
    store: &dyn CellStore,
    balance: RevAmount,
    held: RevAmount,
    entry: JournalEntry,
) -> Result<(), RevStackError> {
    entry.check()?;
    if entry.from.payer().is_some() {
        spendable(balance, held, entry.amount)?;
    }
    record(store, &entry)
}

pub fn journaled_transfers(
    store: &dyn CellStore,
    balance: RevAmount,
    held: RevAmount,
    kind: &TokenKind,
    from: &WalletOwner,
    payees: &[(WalletOwner, RevAmount)],
) -> Result<(), RevStackError> {
    let total = payees
        .iter()
        .try_fold(0 as RevAmount, |sum, (_, amount)| sum.checked_add(*amount))
        .filter(|total| *total <= super::MAX_AMOUNT)
        .ok_or(RevStackError::InvalidAmount(RevAmount::MAX))?;
    if payees.iter().any(|(_, amount)| *amount == 0) {
        return Err(RevStackError::InvalidAmount(0));
    }
    spendable(balance, held, total)?;
    for (to, amount) in payees.iter().filter(|(to, _)| to != from) {
        record(
            store,
            &JournalEntry::transfer_of(kind.clone(), from.clone(), to.clone(), *amount),
        )?;
    }
    Ok(())
}

pub fn visible_balance(balance: RevAmount, visible: RevAmount) -> RevAmount {
    balance.saturating_sub(visible)
}

type Totals = BTreeMap<(Vec<u8>, Vec<u8>), (TokenKind, WalletOwner, u128, u128)>;

fn totals_of<'a>(
    totals: &'a mut Totals,
    kind: &TokenKind,
    owner: &WalletOwner,
) -> &'a mut (TokenKind, WalletOwner, u128, u128) {
    totals
        .entry((kind.name().to_vec(), owner.canonical_bytes()))
        .or_insert_with(|| (kind.clone(), owner.clone(), 0, 0))
}

pub fn journal_overdrawn(store: Arc<dyn CellStore>) -> Result<bool, RevStackError> {
    let mut totals = Totals::new();
    for e in entries(store.as_ref())? {
        if let Some(from) = e.from.payer() {
            totals_of(&mut totals, &e.kind, from).2 += e.amount as u128;
        }
        totals_of(&mut totals, &e.kind, &e.to).3 += e.amount as u128;
    }
    let stacks = RevStacks::new(store.clone());
    for (kind, owner, debits, credits) in totals.into_values() {
        if debits == 0 {
            continue;
        }
        let balance = stacks.of_kind(&kind).balance(&owner)? as u128;
        let held = if kind.is_rev() {
            reservation(store.as_ref(), &owner)?.held as u128
        } else {
            0
        };
        if balance + credits < debits + held {
            return Ok(true);
        }
    }
    Ok(false)
}
