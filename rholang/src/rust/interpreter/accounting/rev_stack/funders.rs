use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, ListParWithRandom, Par};
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::trace::event::Produce;

use super::ops::CellStore;
use super::owner::WalletOwner;
use super::{RevAmount, RevStackError};
use crate::rust::interpreter::rho_type::{RhoByteArray, RhoNumber, RhoTuple2};

pub const FUNDERS_DOMAIN: &[u8] = b"f1r3fly/rev-stack/funders/v1";

pub type Shares = Vec<(WalletOwner, RevAmount)>;

fn ledger_channel(process: &WalletOwner) -> Par {
    let mut input = FUNDERS_DOMAIN.to_vec();
    input.extend(process.canonical_bytes());
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
            id: Blake2b256::hash(input),
        })),
    }])
}

fn malformed() -> RevStackError {
    RevStackError::CorruptWallet("a funder share is malformed".into())
}

pub fn shares(store: &dyn CellStore, process: &WalletOwner) -> Result<Shares, RevStackError> {
    let mut out: Shares = Vec::new();
    for d in store.get_data(&ledger_channel(process)) {
        let [par] = d.a.pars.as_slice() else {
            return Err(malformed());
        };
        let (funder, amount) = RhoTuple2::unapply(par).ok_or_else(malformed)?;
        let funder = RhoByteArray::unapply(&funder)
            .and_then(|b| WalletOwner::from_canonical_bytes(&b))
            .ok_or_else(malformed)?;
        let amount = RhoNumber::unapply(&amount)
            .filter(|a| *a > 0)
            .ok_or_else(malformed)? as RevAmount;
        match out.iter_mut().find(|(f, _)| *f == funder) {
            Some((_, total)) => *total = total.checked_add(amount).ok_or_else(malformed)?,
            None => out.push((funder, amount)),
        }
    }
    out.sort_by_cached_key(|(f, _)| f.canonical_bytes());
    Ok(out)
}

fn write(
    store: &dyn CellStore,
    process: &WalletOwner,
    shares: &Shares,
) -> Result<(), RevStackError> {
    let channel = ledger_channel(process);
    let n = store.get_data(&channel).len();
    for index in (0..n).rev() {
        store.remove_datum(&channel, index as i32)?;
    }
    let mut sorted: Shares = shares.iter().filter(|(_, a)| *a > 0).cloned().collect();
    sorted.sort_by_cached_key(|(f, _)| f.canonical_bytes());
    for (funder, amount) in sorted {
        let a = ListParWithRandom {
            pars: vec![RhoTuple2::create_par((
                RhoByteArray::create_par(funder.canonical_bytes()),
                RhoNumber::create_par(amount as i64),
            ))],
            random_state: Vec::new(),
            seal: None,
        };
        let source = Produce::create(&channel, &a, false);
        store.put_datum(&channel, Datum {
            a,
            persist: false,
            source,
        });
    }
    Ok(())
}

pub fn share(
    store: &dyn CellStore,
    process: &WalletOwner,
    funder: &WalletOwner,
) -> Result<RevAmount, RevStackError> {
    Ok(shares(store, process)?
        .into_iter()
        .find(|(f, _)| f == funder)
        .map_or(0, |(_, a)| a))
}

pub fn credit(
    store: &dyn CellStore,
    process: &WalletOwner,
    funder: &WalletOwner,
    amount: RevAmount,
) -> Result<(), RevStackError> {
    let mut all = shares(store, process)?;
    match all.iter_mut().find(|(f, _)| f == funder) {
        Some((_, total)) => {
            *total = total
                .checked_add(amount)
                .ok_or(RevStackError::InvalidAmount(amount))?
        }
        None => all.push((funder.clone(), amount)),
    }
    write(store, process, &all)
}

pub fn pro_rata(shares: &Shares, amount: RevAmount) -> Vec<RevAmount> {
    let total: u128 = shares.iter().map(|(_, a)| *a as u128).sum();
    if total == 0 {
        return vec![0; shares.len()];
    }
    let amount = (amount as u128).min(total);
    let mut parts: Vec<u128> = Vec::with_capacity(shares.len());
    let mut remainders: Vec<(u128, usize)> = Vec::with_capacity(shares.len());
    for (i, (_, s)) in shares.iter().enumerate() {
        let exact = amount * *s as u128;
        parts.push(exact / total);
        remainders.push((exact % total, i));
    }
    let mut left = amount - parts.iter().sum::<u128>();
    remainders.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for (_, i) in remainders {
        if left == 0 {
            break;
        }
        parts[i] += 1;
        left -= 1;
    }
    parts.into_iter().map(|p| p as RevAmount).collect()
}

pub fn draw(
    store: &dyn CellStore,
    process: &WalletOwner,
    amount: RevAmount,
) -> Result<Shares, RevStackError> {
    let all = shares(store, process)?;
    let parts = pro_rata(&all, amount);
    let drawn: Shares = all
        .iter()
        .zip(&parts)
        .map(|((f, _), p)| (f.clone(), *p))
        .filter(|(_, p)| *p > 0)
        .collect();
    let left: Shares = all
        .into_iter()
        .zip(parts)
        .map(|((f, s), p)| (f, s - p))
        .collect();
    write(store, process, &left)?;
    Ok(drawn)
}

pub fn refuse(
    store: &dyn CellStore,
    process: &WalletOwner,
    funder: &WalletOwner,
    amount: RevAmount,
) -> Result<(), RevStackError> {
    let mut all = shares(store, process)?;
    let Some((_, held)) = all.iter_mut().find(|(f, _)| f == funder) else {
        return Err(RevStackError::InvalidAmount(amount));
    };
    *held = held
        .checked_sub(amount)
        .ok_or(RevStackError::InsufficientFunds {
            balance: *held,
            requested: amount,
        })?;
    write(store, process, &all)
}

#[cfg(test)]
mod tests {
    use super::{pro_rata, Shares};
    use crate::rust::interpreter::accounting::rev_stack::WalletOwner;

    fn shares(amounts: &[u64]) -> Shares {
        amounts
            .iter()
            .enumerate()
            .map(|(i, a)| (WalletOwner::Unforgeable(vec![i as u8; 32]), *a))
            .collect()
    }

    #[test]
    fn a_draw_is_split_by_largest_remainder() {
        assert_eq!(pro_rata(&shares(&[1, 1, 1]), 2), vec![1, 1, 0]);
        assert_eq!(pro_rata(&shares(&[50, 30, 20]), 10), vec![5, 3, 2]);
        assert_eq!(pro_rata(&shares(&[2, 1]), 2), vec![1, 1]);
        assert_eq!(pro_rata(&shares(&[7, 0, 3]), 5), vec![4, 0, 1]);
        assert_eq!(pro_rata(&shares(&[5, 5]), 20), vec![5, 5]);
        assert_eq!(pro_rata(&shares(&[]), 3), Vec::<u64>::new());
    }

    #[test]
    fn a_draw_never_takes_more_than_it_asks_or_a_share_holds() {
        let s = shares(&[13, 29, 1, 57]);
        for amount in 0..=100 {
            let parts = pro_rata(&s, amount);
            assert_eq!(parts.iter().sum::<u64>(), amount);
            assert!(parts.iter().zip(&s).all(|(p, (_, h))| p <= h));
        }
    }
}
