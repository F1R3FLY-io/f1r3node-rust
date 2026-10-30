use std::collections::BTreeMap;
use std::num::NonZeroU64;
use std::sync::Arc;

use models::rust::block::state_hash::StateHash;
use prost::bytes::Bytes;
use rholang::rust::interpreter::accounting::rev_stack::compute::compute_wallet_owner;
use rholang::rust::interpreter::accounting::rev_stack::genesis::{genesis_supply, legacy_bonds};
use rholang::rust::interpreter::accounting::rev_stack::{
    wallet_channel_id, CellStore, HotStoreCells, RevAmount, RevStacks, WalletOwner,
};
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use serde::{Deserialize, Serialize};

use super::gate::{GateEntry, GateInput, WalletId};
use super::hooks::{after_settlement, HookCall};
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
    pub compute_phlo: u64,
    pub compute_rev: RevAmount,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settlement {
    pub charges: Vec<EnvelopeCharge>,
    pub debits: BTreeMap<WalletId, RevAmount>,
    pub proposer: WalletId,
    pub proposer_credit: RevAmount,
    pub compute_credit: RevAmount,
    pub released: BTreeMap<WalletId, RevAmount>,
}

impl Settlement {
    pub fn debited(&self) -> RevAmount { self.debits.values().sum() }

    pub fn credited(&self) -> RevAmount { self.proposer_credit.saturating_add(self.compute_credit) }

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
    let mut compute_credit: RevAmount = 0;
    for ((input, entry), run) in inputs.iter().zip(entries).zip(runs) {
        if !entry.admitted() {
            continue;
        }
        let price = u64::try_from(input.phlo_price).map_err(|_| overflow())?;
        let comm_phlo = run.comm_phlo.min(entry.delta);
        let compute_phlo = run.compute_phlo.min(entry.delta - comm_phlo);
        let comm_rev = comm_phlo.checked_mul(price).ok_or_else(overflow)?;
        let compute_rev = compute_phlo.checked_mul(price).ok_or_else(overflow)?;
        let total_rev = comm_rev.checked_add(compute_rev).ok_or_else(overflow)?;
        for owner in input.paying_wallets() {
            let wallet = wallet_channel_id(&owner);
            let committed: RevAmount = entry
                .commitments
                .iter()
                .filter(|c| c.wallet == wallet)
                .map(|c| c.amount)
                .sum();
            if total_rev > committed {
                return Err(CasperError::RuntimeError(
                    "a charge exceeds the envelope's commitment".into(),
                ));
            }
            let debit = debits.entry(wallet.clone()).or_insert(0);
            *debit = debit.checked_add(total_rev).ok_or_else(overflow)?;
            credit = credit.checked_add(comm_rev).ok_or_else(overflow)?;
            compute_credit = compute_credit
                .checked_add(compute_rev)
                .ok_or_else(overflow)?;
            charges.push(EnvelopeCharge {
                envelope_sig: entry.envelope_sig.clone(),
                wallet,
                phlo_price: price,
                comm_phlo,
                comm_rev,
                compute_phlo,
                compute_rev,
            });
        }
    }
    Ok(Settlement {
        charges,
        debits,
        proposer: wallet_channel_id(proposer),
        proposer_credit: credit,
        compute_credit,
        released: BTreeMap::new(),
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
        let phlo_total = charge.comm_phlo + charge.compute_phlo;
        let step = boxcar.0.map_or(phlo_total, NonZeroU64::get);
        let mut left = phlo_total;
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
        .map_err(native)?;
    stacks
        .deposit(&compute_wallet_owner(), settlement.compute_credit)
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
    let mut settlement = charges(inputs, entries, runs, proposer)?;
    settlement.released = released(entries, &settlement)?;
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
    let cells: Arc<dyn CellStore> = Arc::new(HotStoreCells(store));
    let mut touched: Vec<WalletOwner> = owners.values().cloned().collect();
    touched.push(proposer.clone());
    touched.push(compute_wallet_owner());
    let before = held_by(cells.clone(), &touched)?;
    if settlement.debited() > 0 {
        apply(cells.clone(), &settlement, &owners, proposer, boxcar)?;
    }
    match after_settlement(cells.as_ref())? {
        HookCall::Noop => {}
        HookCall::Contract(_) => {
            return Err(CasperError::RuntimeError(
                "a contract REV hook cannot run yet: only the no-op hook is supported".into(),
            ))
        }
    }
    let after = held_by(cells, &touched)?;
    if before != after {
        return Err(CasperError::RuntimeError(format!(
            "settlement broke conservation: the settled wallets held {before} and now hold {after}"
        )));
    }
    if settlement.debited() == 0 {
        return Ok((settlement, mid_root.clone()));
    }
    let settled_root = runtime.create_checkpoint().await.root.to_bytes_prost();
    Ok((settlement, settled_root))
}

fn released(
    entries: &[GateEntry],
    settlement: &Settlement,
) -> Result<BTreeMap<WalletId, RevAmount>, CasperError> {
    let mut committed: BTreeMap<WalletId, RevAmount> = BTreeMap::new();
    for entry in entries.iter().filter(|e| e.admitted()) {
        for c in &entry.commitments {
            let total = committed.entry(c.wallet.clone()).or_insert(0);
            *total = total.checked_add(c.amount).ok_or_else(overflow)?;
        }
    }
    committed
        .into_iter()
        .map(|(wallet, amount)| {
            let spent = settlement.debits.get(&wallet).copied().unwrap_or(0);
            amount
                .checked_sub(spent)
                .map(|left| (wallet, left))
                .ok_or_else(|| {
                    CasperError::RuntimeError("a wallet paid more than it committed".into())
                })
        })
        .collect()
}

fn held_by(store: Arc<dyn CellStore>, owners: &[WalletOwner]) -> Result<u128, CasperError> {
    let stacks = RevStacks::new(store);
    let mut seen = std::collections::BTreeSet::new();
    let mut total: u128 = 0;
    for owner in owners {
        if seen.insert(owner.canonical_bytes()) {
            let b = stacks
                .balance(owner)
                .map_err(|e| CasperError::RuntimeError(format!("settlement: {e}")))?;
            total += b as u128;
        }
    }
    Ok(total)
}

pub fn audit(store: Arc<dyn CellStore>, wallets: &[WalletOwner]) -> Result<Audit, CasperError> {
    let mut all = wallets.to_vec();
    all.push(compute_wallet_owner());
    let held = held_by(store.clone(), &all)?;
    let native = |e: rholang::rust::interpreter::accounting::rev_stack::RevStackError| {
        CasperError::RuntimeError(format!("audit: {e}"))
    };
    Ok(Audit {
        held,
        legacy_bonds: legacy_bonds(store.as_ref()).map_err(native)?,
        supply: genesis_supply(store.as_ref()).map_err(native)?,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Audit {
    pub held: u128,
    pub legacy_bonds: RevAmount,
    pub supply: RevAmount,
}

impl Audit {
    pub fn conserved(&self) -> bool { self.held + self.legacy_bonds as u128 == self.supply as u128 }
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

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ChargeProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub envelope_id: Bytes,
    #[prost(bytes = "bytes", tag = "2")]
    pub wallet: Bytes,
    #[prost(uint64, tag = "3")]
    pub phlo_price: u64,
    #[prost(uint64, tag = "4")]
    pub comm_phlo: u64,
    #[prost(uint64, tag = "5")]
    pub comm_rev: u64,
    #[prost(uint64, tag = "6")]
    pub compute_phlo: u64,
    #[prost(uint64, tag = "7")]
    pub compute_rev: u64,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct WalletAmountProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub wallet: Bytes,
    #[prost(uint64, tag = "2")]
    pub amount: u64,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SettlementProto {
    #[prost(message, repeated, tag = "1")]
    pub charges: Vec<ChargeProto>,
    #[prost(bytes = "bytes", tag = "2")]
    pub proposer: Bytes,
    #[prost(uint64, tag = "3")]
    pub proposer_credit: u64,
    #[prost(uint64, tag = "4")]
    pub compute_credit: u64,
    #[prost(message, repeated, tag = "5")]
    pub released: Vec<WalletAmountProto>,
}

impl Settlement {
    pub fn to_proto(&self) -> SettlementProto {
        SettlementProto {
            charges: self
                .charges
                .iter()
                .map(|c| ChargeProto {
                    envelope_id: c.envelope_sig.clone(),
                    wallet: Bytes::copy_from_slice(&c.wallet),
                    phlo_price: c.phlo_price,
                    comm_phlo: c.comm_phlo,
                    comm_rev: c.comm_rev,
                    compute_phlo: c.compute_phlo,
                    compute_rev: c.compute_rev,
                })
                .collect(),
            proposer: Bytes::copy_from_slice(&self.proposer),
            proposer_credit: self.proposer_credit,
            compute_credit: self.compute_credit,
            released: self
                .released
                .iter()
                .map(|(w, a)| WalletAmountProto {
                    wallet: Bytes::copy_from_slice(w),
                    amount: *a,
                })
                .collect(),
        }
    }

    pub fn from_proto(proto: SettlementProto) -> Result<Self, String> {
        let charges: Vec<EnvelopeCharge> = proto
            .charges
            .into_iter()
            .map(|c| {
                let comm = c.comm_phlo.checked_mul(c.phlo_price);
                let compute = c.compute_phlo.checked_mul(c.phlo_price);
                if comm != Some(c.comm_rev) || compute != Some(c.compute_rev) {
                    return Err("a recorded charge is not its phlo times its price".to_string());
                }
                Ok(EnvelopeCharge {
                    envelope_sig: c.envelope_id,
                    wallet: c.wallet.to_vec(),
                    phlo_price: c.phlo_price,
                    comm_phlo: c.comm_phlo,
                    comm_rev: c.comm_rev,
                    compute_phlo: c.compute_phlo,
                    compute_rev: c.compute_rev,
                })
            })
            .collect::<Result<_, _>>()?;
        let mut debits: BTreeMap<WalletId, RevAmount> = BTreeMap::new();
        let (mut comm, mut compute) = (0u64, 0u64);
        let sum = |a: u64, b: u64| a.checked_add(b).ok_or("a recorded amount overflows");
        for c in &charges {
            let d = debits.entry(c.wallet.clone()).or_insert(0);
            *d = sum(*d, sum(c.comm_rev, c.compute_rev)?)?;
            comm = sum(comm, c.comm_rev)?;
            compute = sum(compute, c.compute_rev)?;
        }
        if comm != proto.proposer_credit || compute != proto.compute_credit {
            return Err("the recorded credits do not add up to the recorded charges".to_string());
        }
        let released = proto
            .released
            .into_iter()
            .map(|r| (r.wallet.to_vec(), r.amount))
            .collect();
        Ok(Settlement {
            charges,
            debits,
            proposer: proto.proposer.to_vec(),
            proposer_credit: proto.proposer_credit,
            compute_credit: proto.compute_credit,
            released,
        })
    }

    pub fn charged_phlo(&self, envelope: &[u8]) -> Option<u64> {
        self.charges
            .iter()
            .find(|c| c.envelope_sig.as_ref() == envelope)
            .map(|c| c.comm_phlo + c.compute_phlo)
    }
}

#[cfg(test)]
mod proto_tests {
    use super::*;

    fn settlement() -> Settlement {
        let charge = |id: &'static [u8], wallet: u8, comm: u64, compute: u64| EnvelopeCharge {
            envelope_sig: Bytes::from_static(id),
            wallet: vec![wallet; 32],
            phlo_price: 3,
            comm_phlo: comm,
            comm_rev: comm * 3,
            compute_phlo: compute,
            compute_rev: compute * 3,
        };
        let charges = vec![
            charge(b"a", 1, 2, 10),
            charge(b"b", 1, 1, 0),
            charge(b"c", 2, 0, 4),
        ];
        Settlement {
            debits: BTreeMap::from([(vec![1; 32], 39), (vec![2; 32], 12)]),
            charges,
            proposer: vec![9; 32],
            proposer_credit: 9,
            compute_credit: 42,
            released: BTreeMap::from([(vec![1; 32], 61)]),
        }
    }

    #[test]
    fn a_settlement_round_trips_and_its_arithmetic_is_checked() {
        let s = settlement();
        assert_eq!(Settlement::from_proto(s.to_proto()), Ok(s.clone()));
        assert_eq!(s.charged_phlo(b"a"), Some(12));
        let mut bad = s.to_proto();
        bad.charges[0].comm_rev += 1;
        assert!(Settlement::from_proto(bad).is_err());
        let mut bad = s.to_proto();
        bad.compute_credit += 1;
        assert!(Settlement::from_proto(bad).is_err());
    }
}
