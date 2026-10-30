use std::collections::BTreeMap;
use std::num::NonZeroU64;
use std::sync::Arc;

use models::rust::block::state_hash::StateHash;
use prost::bytes::Bytes;
use rholang::rust::interpreter::accounting::rev_stack::{
    wallet_channel_id, CellStore, HotStoreCells, RevAmount, RevStacks, WalletOwner,
};
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use serde::{Deserialize, Serialize};

use super::gate::{GateEntry, GateInput, WalletId};
use super::run::EnvelopeRun;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Boxcar(pub Option<NonZeroU64>);

impl Boxcar {
    pub fn unbounded() -> Self { Boxcar(None) }

    pub fn of(phlo: u64) -> Self { Boxcar(NonZeroU64::new(phlo)) }

    #[cfg(test)]
    fn chunks(&self, phlo: u64) -> Vec<u64> {
        match self.0 {
            None => vec![phlo],
            Some(size) => {
                let size = size.get();
                let mut out = vec![size; (phlo / size) as usize];
                if phlo % size != 0 {
                    out.push(phlo % size);
                }
                out
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvelopeCharge {
    pub envelope_sig: Bytes,
    pub wallet: WalletId,
    pub phlo_price: u64,
    pub comm_phlo: u64,
    pub comm_rev: RevAmount,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settlement {
    pub charges: Vec<EnvelopeCharge>,
    pub debits: BTreeMap<WalletId, RevAmount>,
    pub proposer: WalletId,
    pub proposer_credit: RevAmount,
}

impl Settlement {
    pub fn debited(&self) -> RevAmount { self.debits.values().sum() }

    pub fn credited(&self) -> RevAmount { self.proposer_credit }

    pub fn is_balanced(&self) -> bool { self.debited() == self.credited() }
}

fn overflow() -> CasperError { CasperError::RuntimeError("a settlement amount overflows".into()) }

pub fn charges(
    inputs: &[GateInput],
    entries: &[GateEntry],
    runs: &[EnvelopeRun],
    proposer: &WalletOwner,
) -> Result<Settlement, CasperError> {
    if inputs.len() != entries.len() || inputs.len() != runs.len() {
        return Err(CasperError::RuntimeError(
            "settlement needs one gate entry and one run per envelope".into(),
        ));
    }
    let mut charges = Vec::new();
    let mut debits: BTreeMap<WalletId, RevAmount> = BTreeMap::new();
    let mut credit: RevAmount = 0;
    for ((input, entry), run) in inputs.iter().zip(entries).zip(runs) {
        if !entry.admitted() {
            continue;
        }
        let price = u64::try_from(input.phlo_price).map_err(|_| overflow())?;
        let comm_phlo = run.comm_phlo.min(entry.delta);
        let comm_rev = comm_phlo.checked_mul(price).ok_or_else(overflow)?;
        for owner in input.paying_wallets() {
            let wallet = wallet_channel_id(&owner);
            let committed: RevAmount = entry
                .commitments
                .iter()
                .filter(|c| c.wallet == wallet)
                .map(|c| c.amount)
                .sum();
            if comm_rev > committed {
                return Err(CasperError::RuntimeError(
                    "a charge exceeds the envelope's commitment".into(),
                ));
            }
            let debit = debits.entry(wallet.clone()).or_insert(0);
            *debit = debit.checked_add(comm_rev).ok_or_else(overflow)?;
            credit = credit.checked_add(comm_rev).ok_or_else(overflow)?;
            charges.push(EnvelopeCharge {
                envelope_sig: entry.envelope_sig.clone(),
                wallet,
                phlo_price: price,
                comm_phlo,
                comm_rev,
            });
        }
    }
    Ok(Settlement {
        charges,
        debits,
        proposer: wallet_channel_id(proposer),
        proposer_credit: credit,
    })
}

pub fn apply(
    store: Arc<dyn CellStore>,
    settlement: &Settlement,
    owners: &BTreeMap<WalletId, WalletOwner>,
    proposer: &WalletOwner,
    boxcar: Boxcar,
) -> Result<(), CasperError> {
    if !settlement.is_balanced() {
        return Err(CasperError::RuntimeError(
            "a settlement must move as much REV as it takes".into(),
        ));
    }
    let stacks = RevStacks::new(store);
    let native = |e: rholang::rust::interpreter::accounting::rev_stack::RevStackError| {
        CasperError::RuntimeError(format!("settlement: {e}"))
    };
    for charge in &settlement.charges {
        let owner = owners.get(&charge.wallet).ok_or_else(|| {
            CasperError::RuntimeError("settlement names a wallet with no owner".into())
        })?;
        if charge.phlo_price == 0 {
            continue;
        }
        let step = boxcar.0.map_or(charge.comm_phlo, NonZeroU64::get);
        let mut left = charge.comm_phlo;
        while left > 0 {
            let phlo = step.min(left);
            stacks
                .spend(owner, phlo * charge.phlo_price)
                .map_err(native)?;
            left -= phlo;
        }
    }
    for (wallet, owner) in owners {
        if settlement.debits.get(wallet).copied().unwrap_or(0) > 0 {
            stacks.combine(owner).map_err(native)?;
        }
    }
    stacks
        .deposit(proposer, settlement.proposer_credit)
        .map_err(native)
}

pub async fn settle_block(
    runtime_manager: &RuntimeManager,
    mid_root: &StateHash,
    inputs: &[GateInput],
    entries: &[GateEntry],
    runs: &[EnvelopeRun],
    proposer: &WalletOwner,
    boxcar: Boxcar,
) -> Result<(Settlement, StateHash), CasperError> {
    let settlement = charges(inputs, entries, runs, proposer)?;
    if settlement.debited() == 0 {
        return Ok((settlement, mid_root.clone()));
    }
    let owners: BTreeMap<WalletId, WalletOwner> = inputs
        .iter()
        .flat_map(GateInput::paying_wallets)
        .map(|o| (wallet_channel_id(&o), o))
        .collect();
    let mut runtime = runtime_manager.spawn_runtime().await?;
    runtime
        .reset(&Blake2b256Hash::from_bytes_prost(mid_root))
        .await
        .map_err(CasperError::InterpreterError)?;
    let store = runtime.reducer.space.hot_store().ok_or_else(|| {
        CasperError::RuntimeError("the runtime's space has no native REV layer".into())
    })?;
    apply(
        Arc::new(HotStoreCells(store)),
        &settlement,
        &owners,
        proposer,
        boxcar,
    )?;
    let settled_root = runtime.create_checkpoint().await.root.to_bytes_prost();
    Ok((settlement, settled_root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxcar_chunks_cover_the_phlo_exactly() {
        assert_eq!(Boxcar::unbounded().chunks(10), vec![10]);
        assert_eq!(Boxcar::of(3).chunks(10), vec![3, 3, 3, 1]);
        assert_eq!(Boxcar::of(5).chunks(10), vec![5, 5]);
        assert_eq!(Boxcar::of(0), Boxcar::unbounded());
        assert_eq!(Boxcar::of(4).chunks(0), Vec::<u64>::new());
    }
}
