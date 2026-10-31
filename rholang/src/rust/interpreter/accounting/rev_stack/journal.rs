use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, ListParWithRandom, Par};
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::trace::event::Produce;

use super::ops::CellStore;
use super::owner::WalletOwner;
use super::{RevAmount, RevStackError};
use crate::rust::interpreter::rho_type::{RhoByteArray, RhoList, RhoNumber, RhoString};

pub const JOURNAL_DOMAIN: &[u8] = b"f1r3fly/rev-stack/journal/v1";
pub const JOURNAL_MARKER_DOMAIN: &[u8] = b"f1r3fly/rev-stack/journal-marker/v1";

const ENTRY_TAG: &str = "transfer";
const MARKER_TAG: &str = "journal";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalEntry {
    pub from: WalletOwner,
    pub to: WalletOwner,
    pub amount: RevAmount,
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
    let ch = journal_channel();
    let seq = store.get_data(&ch).len() as i64;
    let pars = vec![RhoList::create_par(vec![
        RhoString::create_par(ENTRY_TAG.into()),
        RhoNumber::create_par(seq),
        RhoByteArray::create_par(entry.from.canonical_bytes()),
        RhoByteArray::create_par(entry.to.canonical_bytes()),
        RhoNumber::create_par(entry.amount as i64),
    ])];
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
        let [tag, seq, from, to, amount] = items.as_slice() else {
            return Err(malformed());
        };
        if RhoString::unapply(tag).as_deref() != Some(ENTRY_TAG) {
            return Err(malformed());
        }
        let seq = RhoNumber::unapply(seq).ok_or_else(malformed)?;
        let owner = |p: &Par| {
            RhoByteArray::unapply(p)
                .and_then(|b| WalletOwner::from_canonical_bytes(&b))
                .ok_or_else(malformed)
        };
        let amount = RhoNumber::unapply(amount)
            .filter(|a| *a > 0)
            .ok_or_else(malformed)?;
        out.push((
            seq,
            JournalEntry {
                from: owner(from)?,
                to: owner(to)?,
                amount: amount as RevAmount,
            },
        ));
    }
    out.sort_by_key(|(seq, _)| *seq);
    Ok(out.into_iter().map(|(_, e)| e).collect())
}

pub fn net(
    store: &dyn CellStore,
    owner: &WalletOwner,
) -> Result<(RevAmount, RevAmount), RevStackError> {
    let mut debits: RevAmount = 0;
    let mut credits: RevAmount = 0;
    for e in entries(store)? {
        if &e.from == owner {
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

static JOURNAL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn journaled_transfer(
    store: &dyn CellStore,
    balance: RevAmount,
    held: RevAmount,
    entry: JournalEntry,
) -> Result<(), RevStackError> {
    if entry.amount == 0 || entry.amount > super::MAX_AMOUNT {
        return Err(RevStackError::InvalidAmount(entry.amount));
    }
    let _guard = JOURNAL_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (debits, credits) = net(store, &entry.from)?;
    let available = balance
        .saturating_add(credits)
        .saturating_sub(debits)
        .saturating_sub(held);
    if entry.amount > available {
        return Err(RevStackError::InsufficientFunds {
            balance: available,
            requested: entry.amount,
        });
    }
    record(store, &entry)
}

pub fn journaled_transfers(
    store: &dyn CellStore,
    balance: RevAmount,
    held: RevAmount,
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
    let _guard = JOURNAL_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (debits, credits) = net(store, from)?;
    let available = balance
        .saturating_add(credits)
        .saturating_sub(debits)
        .saturating_sub(held);
    if total > available {
        return Err(RevStackError::InsufficientFunds {
            balance: available,
            requested: total,
        });
    }
    for (to, amount) in payees.iter().filter(|(to, _)| to != from) {
        record(
            store,
            &JournalEntry {
                from: from.clone(),
                to: to.clone(),
                amount: *amount,
            },
        )?;
    }
    Ok(())
}

pub fn visible_balance(
    store: &dyn CellStore,
    owner: &WalletOwner,
    balance: RevAmount,
    visible: RevAmount,
) -> Result<RevAmount, RevStackError> {
    let (debits, credits) = net(store, owner)?;
    Ok(balance
        .saturating_add(credits)
        .saturating_sub(debits)
        .saturating_sub(visible))
}
