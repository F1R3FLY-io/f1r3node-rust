use std::collections::BTreeMap;
use std::num::NonZeroU64;
use std::sync::Arc;

use models::rust::block::state_hash::StateHash;
use prost::bytes::Bytes;
use rholang::rust::interpreter::accounting::rev_stack::compute::compute_wallet_owner;
use rholang::rust::interpreter::accounting::rev_stack::funders;
use rholang::rust::interpreter::accounting::rev_stack::genesis::{genesis_supply, legacy_bonds};
use rholang::rust::interpreter::accounting::rev_stack::journal::{JournalEntry, Source};
use rholang::rust::interpreter::accounting::rev_stack::{
    wallet_channel_id, CellStore, HotStoreCells, RevAmount, RevStacks, TokenKind, WalletOwner,
};
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use serde::{Deserialize, Serialize};

use super::gate::{GateEntry, GateInput, WalletId};
use super::hooks::{after_settlement, HookCall};
use super::run::EnvelopeRun;
use super::surface::{is_own, surface_purse, Surfaces};
use rholang::rust::interpreter::accounting::comm_ledger::Surface;
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
                if !phlo.is_multiple_of(size) {
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
pub struct NativeTransfer {
    pub envelope_sig: Bytes,
    pub kind: TokenKind,
    pub from: Source,
    pub to: WalletOwner,
    pub amount: RevAmount,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceCharge {
    pub envelope_sig: Bytes,
    pub purse: WalletOwner,
    pub phlo_price: u64,
    pub cells: u64,
    pub compute: u64,
    pub owed: RevAmount,
    pub drawn: RevAmount,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settlement {
    pub charges: Vec<EnvelopeCharge>,
    pub debits: BTreeMap<WalletId, RevAmount>,
    pub proposer: WalletId,
    pub proposer_credit: RevAmount,
    pub compute_credit: RevAmount,
    pub released: BTreeMap<WalletId, RevAmount>,
    pub transfers: Vec<NativeTransfer>,
    pub surface_charges: Vec<SurfaceCharge>,
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
    surfaces: &[Surfaces],
    work: &[Surfaces],
    proposer: &WalletOwner,
) -> Result<Settlement, CasperError> {
    if inputs.len() != entries.len()
        || inputs.len() != runs.len()
        || inputs.len() != surfaces.len()
        || inputs.len() != work.len()
    {
        return Err(CasperError::RuntimeError(
            "settlement needs one gate entry and one run per envelope".into(),
        ));
    }
    let mut charges = Vec::new();
    let mut debits: BTreeMap<WalletId, RevAmount> = BTreeMap::new();
    let mut credit: RevAmount = 0;
    let mut compute_credit: RevAmount = 0;
    for (((input, entry), surfaces), work) in inputs.iter().zip(entries).zip(surfaces).zip(work) {
        if !entry.admitted() {
            continue;
        }
        let price = u64::try_from(input.phlo_price).map_err(|_| overflow())?;
        let payers = input.paying_wallets();
        let mut budget = entry.delta;
        let mut owed: BTreeMap<Vec<u8>, (WalletOwner, u64, u64)> = BTreeMap::new();
        for (key, (cells, compute)) in own_meter(surfaces, work, &entry.envelope_sig) {
            let Some(own) = own_payers(&key, input, &payers) else {
                continue;
            };
            let cells = cells.min(budget);
            budget -= cells;
            let compute = compute.min(budget);
            budget -= compute;
            for payer in own {
                let o = owed
                    .entry(payer.canonical_bytes())
                    .or_insert((payer, 0, 0));
                o.1 = o.1.checked_add(cells).ok_or_else(overflow)?;
                o.2 = o.2.checked_add(compute).ok_or_else(overflow)?;
            }
        }
        for (owner, comm_phlo, compute_phlo) in owed.into_values() {
            let wallet = wallet_channel_id(&owner);
            let comm_rev = comm_phlo.checked_mul(price).ok_or_else(overflow)?;
            let compute_rev = compute_phlo.checked_mul(price).ok_or_else(overflow)?;
            let total_rev = comm_rev.checked_add(compute_rev).ok_or_else(overflow)?;
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
        transfers: Vec::new(),
        surface_charges: Vec::new(),
    })
}

type Meter = BTreeMap<Option<Surface>, (u64, u64)>;

fn meter<'a>(
    surfaces: &'a Surfaces,
    work: &'a Surfaces,
    keep: impl Fn(&Option<Surface>) -> bool,
) -> Meter {
    let mut m: Meter = BTreeMap::new();
    for (key, cells) in surfaces.iter().filter(|(k, _)| keep(k)) {
        m.entry(key.clone()).or_insert((0, 0)).0 += cells;
    }
    for (key, phlo) in work.iter().filter(|(k, _)| keep(k)) {
        m.entry(key.clone()).or_insert((0, 0)).1 += phlo;
    }
    m
}

pub fn own_meter(surfaces: &Surfaces, work: &Surfaces, envelope: &[u8]) -> Meter {
    meter(surfaces, work, |k| is_own(k, envelope))
}

pub fn own_phlo(surfaces: &Surfaces, work: &Surfaces, envelope: &[u8]) -> u64 {
    own_meter(surfaces, work, envelope)
        .values()
        .map(|(c, w)| c.saturating_add(*w))
        .fold(0u64, u64::saturating_add)
}

fn client(input: &GateInput, payers: &[WalletOwner]) -> Vec<WalletOwner> {
    if payers.contains(&input.deployer) {
        vec![input.deployer.clone()]
    } else {
        payers.to_vec()
    }
}

fn own_payers(key: &Option<Surface>, input: &GateInput, payers: &[WalletOwner]) -> Option<Vec<WalletOwner>> {
    let Some(owner) = key
        .as_ref()
        .and_then(|s| WalletOwner::from_canonical_bytes(&s.owner))
    else {
        return Some(client(input, payers));
    };
    if payers.contains(&owner) {
        return Some(vec![owner]);
    }
    match owner {
        WalletOwner::Compound(components) if components.iter().all(|c| payers.contains(c)) => {
            Some(components)
        }
        _ => None,
    }
}

pub fn gated_own(key: &Option<Surface>, input: &GateInput) -> bool {
    own_payers(key, input, &input.paying_wallets()).is_none()
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
    let stacks = RevStacks::new(store.clone());
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
    for sc in settlement.surface_charges.iter().filter(|sc| sc.drawn > 0) {
        stacks.spend(&sc.purse, sc.drawn).map_err(native)?;
    }
    for (wallet, owner) in owners {
        if settlement.debits.get(wallet).copied().unwrap_or(0) > 0 {
            stacks.combine(owner).map_err(native)?;
        }
    }
    for t in &settlement.transfers {
        if t.from == Source::Mint && t.kind.is_rev() {
            return Err(CasperError::RuntimeError(
                "a settlement cannot mint REV".into(),
            ));
        }
        stacks
            .of_kind(&t.kind)
            .deposit(&t.to, t.amount)
            .map_err(native)?;
    }
    for t in &settlement.transfers {
        if let Some(from) = t.from.payer() {
            stacks
                .of_kind(&t.kind)
                .spend(from, t.amount)
                .map_err(native)?;
        }
    }
    settle_funders(store.as_ref(), settlement, owners).map_err(native)?;
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
    surfaces: &[Surfaces],
    work: &[Surfaces],
    journals: &[Vec<JournalEntry>],
    kept: &[bool],
    proposer: &WalletOwner,
    boxcar: Boxcar,
) -> Result<(Settlement, StateHash), CasperError> {
    let mut settlement = charges(inputs, entries, runs, surfaces, work, proposer)?;
    settlement.transfers = kept_transfers(entries, journals, kept)?;
    settlement.released = released(entries, &settlement)?;
    let mut owners: BTreeMap<WalletId, WalletOwner> = inputs
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
    settlement.surface_charges = surface_charges(&RevStacks::new(cells.clone()), inputs, entries, surfaces, work, &settlement)?;
    for sc in &settlement.surface_charges {
        let wallet = wallet_channel_id(&sc.purse);
        if sc.drawn > 0 {
            let d = settlement.debits.entry(wallet.clone()).or_insert(0);
            *d = d.checked_add(sc.drawn).ok_or_else(overflow)?;
            settlement.proposer_credit =
                settlement.proposer_credit.checked_add(sc.drawn).ok_or_else(overflow)?;
        }
        owners.insert(wallet, sc.purse.clone());
    }
    let mut touched: BTreeMap<TokenKind, Vec<WalletOwner>> = BTreeMap::new();
    let rev = touched.entry(TokenKind::rev()).or_default();
    rev.extend(owners.values().cloned());
    rev.push(proposer.clone());
    rev.push(compute_wallet_owner());
    let mut minted: BTreeMap<TokenKind, u128> = BTreeMap::new();
    for t in &settlement.transfers {
        let wallets = touched.entry(t.kind.clone()).or_default();
        wallets.push(t.to.clone());
        match t.from.payer() {
            Some(from) => wallets.push(from.clone()),
            None => *minted.entry(t.kind.clone()).or_default() += t.amount as u128,
        }
    }
    if minted.contains_key(&TokenKind::rev()) {
        return Err(CasperError::RuntimeError(
            "a settlement cannot mint REV".into(),
        ));
    }
    let before = held_by_kind(cells.clone(), &touched)?;
    let writes = settlement.debited() > 0 || !settlement.transfers.is_empty();
    if writes {
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
    let after = held_by_kind(cells, &touched)?;
    for (kind, held) in &before {
        let expected = held + minted.get(kind).copied().unwrap_or(0);
        let now = after.get(kind).copied().unwrap_or(0);
        if now != expected {
            let what = if kind.is_rev() { "REV" } else { "a token kind" };
            return Err(CasperError::RuntimeError(format!(
                "settlement broke conservation of {what}: the settled wallets held {held}, minted {}, and now hold {now}",
                expected - held
            )));
        }
    }
    if !writes {
        return Ok((settlement, mid_root.clone()));
    }
    let settled_root = runtime.create_checkpoint().await.root.to_bytes_prost();
    Ok((settlement, settled_root))
}

fn surface_charges(
    stacks: &RevStacks,
    inputs: &[GateInput],
    entries: &[GateEntry],
    surfaces: &[Surfaces],
    work: &[Surfaces],
    settlement: &Settlement,
) -> Result<Vec<SurfaceCharge>, CasperError> {
    let mut planned: BTreeMap<WalletId, RevAmount> = settlement.debits.clone();
    for t in settlement.transfers.iter().filter(|t| t.kind.is_rev()) {
        if let Some(from) = t.from.payer() {
            let p = planned.entry(wallet_channel_id(from)).or_insert(0);
            *p = p.checked_add(t.amount).ok_or_else(overflow)?;
        }
    }
    let mut out = Vec::new();
    for (((input, entry), tally), done) in inputs.iter().zip(entries).zip(surfaces).zip(work) {
        if !entry.admitted() {
            continue;
        }
        let envelope = entry.envelope_sig.as_ref();
        for (key, (cells, compute)) in meter(tally, done, |k| !is_own(k, envelope) || gated_own(k, input)) {
            let Some(surface) = key else { continue };
            let surface = &surface;
            let phlo = cells.checked_add(compute).ok_or_else(overflow)?;
            let owed = phlo.checked_mul(surface.phlo_price).ok_or_else(overflow)?;
            let purses = surface_purse(surface).map_err(CasperError::RuntimeError)?;
            for purse in purses {
                let wallet = wallet_channel_id(&purse);
                let held = stacks
                    .balance(&purse)
                    .map_err(|e| CasperError::RuntimeError(format!("settlement: {e}")))?;
                let used = planned.entry(wallet).or_insert(0);
                if owed > held.saturating_sub(*used) {
                    return Err(CasperError::RuntimeError(
                        "settlement: a surface draw exceeds what the block ledger admitted".into(),
                    ));
                }
                let drawn = owed;
                *used = used.checked_add(drawn).ok_or_else(overflow)?;
                out.push(SurfaceCharge {
                    envelope_sig: entry.envelope_sig.clone(),
                    purse,
                    phlo_price: surface.phlo_price,
                    cells,
                    compute,
                    owed,
                    drawn,
                });
            }
        }
    }
    Ok(out)
}

fn kept_transfers(
    entries: &[GateEntry],
    journals: &[Vec<JournalEntry>],
    kept: &[bool],
) -> Result<Vec<NativeTransfer>, CasperError> {
    if journals.len() != entries.len() || kept.len() != entries.len() {
        return Err(CasperError::RuntimeError(
            "settlement needs one journal and one merge verdict per envelope".into(),
        ));
    }
    Ok(entries
        .iter()
        .zip(journals)
        .zip(kept)
        .filter(|((e, _), k)| e.admitted() && **k)
        .flat_map(|((e, journal), _)| {
            journal.iter().map(|j| NativeTransfer {
                envelope_sig: e.envelope_sig.clone(),
                kind: j.kind.clone(),
                from: j.from.clone(),
                to: j.to.clone(),
                amount: j.amount,
            })
        })
        .collect())
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

fn held_by_kind(
    store: Arc<dyn CellStore>,
    touched: &BTreeMap<TokenKind, Vec<WalletOwner>>,
) -> Result<BTreeMap<TokenKind, u128>, CasperError> {
    touched
        .iter()
        .map(|(kind, owners)| Ok((kind.clone(), held_of(store.clone(), kind, owners)?)))
        .collect()
}

fn held_by(store: Arc<dyn CellStore>, owners: &[WalletOwner]) -> Result<u128, CasperError> {
    held_of(store, &TokenKind::rev(), owners)
}

fn held_of(
    store: Arc<dyn CellStore>,
    kind: &TokenKind,
    owners: &[WalletOwner],
) -> Result<u128, CasperError> {
    let stacks = RevStacks::new(store).of_kind(kind);
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
    #[prost(message, repeated, tag = "6")]
    pub transfers: Vec<TransferProto>,
    #[prost(message, repeated, tag = "7")]
    pub surface_charges: Vec<SurfaceChargeProto>,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SurfaceChargeProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub envelope_id: Bytes,
    #[prost(bytes = "bytes", tag = "2")]
    pub purse: Bytes,
    #[prost(uint64, tag = "3")]
    pub phlo_price: u64,
    #[prost(uint64, tag = "4")]
    pub cells: u64,
    #[prost(uint64, tag = "5")]
    pub owed: u64,
    #[prost(uint64, tag = "6")]
    pub drawn: u64,
    #[prost(uint64, tag = "7")]
    pub compute: u64,
}

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct TransferProto {
    #[prost(bytes = "bytes", tag = "1")]
    pub envelope_id: Bytes,
    #[prost(bytes = "bytes", tag = "2")]
    pub from: Bytes,
    #[prost(bytes = "bytes", tag = "3")]
    pub to: Bytes,
    #[prost(uint64, tag = "4")]
    pub amount: u64,
    #[prost(bytes = "bytes", tag = "5")]
    pub kind: Bytes,
    #[prost(bool, tag = "6")]
    pub mint: bool,
    #[prost(bytes = "bytes", tag = "7")]
    pub share_funder: Bytes,
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
            transfers: self
                .transfers
                .iter()
                .map(|t| TransferProto {
                    envelope_id: t.envelope_sig.clone(),
                    from: t
                        .from
                        .payer()
                        .map_or_else(Bytes::new, |p| Bytes::from(p.canonical_bytes())),
                    to: Bytes::from(t.to.canonical_bytes()),
                    amount: t.amount,
                    kind: if t.kind.is_rev() {
                        Bytes::new()
                    } else {
                        Bytes::copy_from_slice(t.kind.name())
                    },
                    mint: t.from == Source::Mint,
                    share_funder: match &t.from {
                        Source::Share { funder, .. } => Bytes::from(funder.canonical_bytes()),
                        _ => Bytes::new(),
                    },
                })
                .collect(),
            surface_charges: self
                .surface_charges
                .iter()
                .map(|sc| SurfaceChargeProto {
                    envelope_id: sc.envelope_sig.clone(),
                    purse: Bytes::from(sc.purse.canonical_bytes()),
                    phlo_price: sc.phlo_price,
                    cells: sc.cells,
                    owed: sc.owed,
                    drawn: sc.drawn,
                    compute: sc.compute,
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
        let surface_charges = proto
            .surface_charges
            .iter()
            .map(|sc| {
                let purse = WalletOwner::from_canonical_bytes(&sc.purse)
                    .ok_or("a recorded surface charge names no purse")?;
                if sc.envelope_id.is_empty() || (sc.cells == 0 && sc.compute == 0) {
                    return Err("a recorded surface charge names no envelope and no phlo");
                }
                let phlo = sc.cells.checked_add(sc.compute);
                if phlo.and_then(|p| p.checked_mul(sc.phlo_price)) != Some(sc.owed) || sc.drawn > sc.owed {
                    return Err("a recorded surface charge does not add up");
                }
                Ok(SurfaceCharge {
                    envelope_sig: sc.envelope_id.clone(),
                    purse,
                    phlo_price: sc.phlo_price,
                    cells: sc.cells,
                    compute: sc.compute,
                    owed: sc.owed,
                    drawn: sc.drawn,
                })
            })
            .collect::<Result<Vec<_>, &str>>()?;
        for sc in surface_charges.iter().filter(|sc| sc.drawn > 0) {
            let d = debits.entry(wallet_channel_id(&sc.purse)).or_insert(0);
            *d = sum(*d, sc.drawn)?;
            comm = sum(comm, sc.drawn)?;
        }
        if comm != proto.proposer_credit || compute != proto.compute_credit {
            return Err("the recorded credits do not add up to the recorded charges".to_string());
        }
        let released = proto
            .released
            .into_iter()
            .map(|r| (r.wallet.to_vec(), r.amount))
            .collect();
        let transfers = proto
            .transfers
            .into_iter()
            .map(|t| {
                let owner = |b: &Bytes| {
                    WalletOwner::from_canonical_bytes(b)
                        .ok_or_else(|| "a recorded transfer names no wallet".to_string())
                };
                if t.amount == 0 {
                    return Err("a recorded transfer moves nothing".to_string());
                }
                let kind = if t.kind.is_empty() {
                    TokenKind::rev()
                } else {
                    TokenKind::from_name(t.kind.to_vec())
                        .ok_or_else(|| "a recorded transfer names no kind".to_string())?
                };
                let from = match (t.mint, kind.is_rev()) {
                    (true, true) => return Err("a recorded transfer mints REV".to_string()),
                    (true, false) if t.from.is_empty() => Source::Mint,
                    (true, false) => return Err("a recorded mint names a payer".to_string()),
                    (false, _) if t.share_funder.is_empty() => Source::Wallet(owner(&t.from)?),
                    (false, true) => Source::Share {
                        process: owner(&t.from)?,
                        funder: owner(&t.share_funder)?,
                    },
                    (false, false) => return Err("a recorded refusal is not REV".to_string()),
                };
                if t.mint && !t.share_funder.is_empty() {
                    return Err("a recorded mint names a share".to_string());
                }
                Ok(NativeTransfer {
                    envelope_sig: t.envelope_id.clone(),
                    kind,
                    from,
                    to: owner(&t.to)?,
                    amount: t.amount,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Settlement {
            charges,
            debits,
            proposer: proto.proposer.to_vec(),
            proposer_credit: proto.proposer_credit,
            compute_credit: proto.compute_credit,
            released,
            transfers,
            surface_charges,
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
            surface_charges: Vec::new(),
            transfers: vec![
                NativeTransfer {
                    envelope_sig: Bytes::from_static(b"a"),
                    kind: TokenKind::rev(),
                    from: Source::Wallet(WalletOwner::Unforgeable(vec![5; 32])),
                    to: WalletOwner::Unforgeable(vec![6; 32]),
                    amount: 7,
                },
                NativeTransfer {
                    envelope_sig: Bytes::from_static(b"b"),
                    kind: TokenKind::from_name(vec![4; 32]).unwrap(),
                    from: Source::Mint,
                    to: WalletOwner::Unforgeable(vec![6; 32]),
                    amount: 3,
                },
            ],
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

    #[test]
    fn a_recorded_settlement_cannot_mint_rev() {
        let mut bad = settlement().to_proto();
        bad.transfers[1].kind = Bytes::new();
        assert_eq!(
            Settlement::from_proto(bad),
            Err("a recorded transfer mints REV".to_string())
        );
        let mut bad = settlement().to_proto();
        bad.transfers[1].kind = Bytes::from(TokenKind::rev().name().to_vec());
        assert!(Settlement::from_proto(bad).is_err());
        let mut bad = settlement().to_proto();
        bad.transfers[1].from =
            Bytes::from(WalletOwner::Unforgeable(vec![5; 32]).canonical_bytes());
        assert!(Settlement::from_proto(bad).is_err());
    }
}

fn settle_funders(
    store: &dyn CellStore,
    settlement: &Settlement,
    owners: &BTreeMap<WalletId, WalletOwner>,
) -> Result<(), rholang::rust::interpreter::accounting::rev_stack::RevStackError> {
    for t in &settlement.transfers {
        if let Source::Share { process, funder } = &t.from {
            funders::refuse(store, process, funder, t.amount)?;
        }
    }
    for t in settlement.transfers.iter().filter(|t| t.kind.is_rev()) {
        if let Source::Wallet(funder) = &t.from {
            if t.to.has_funders() {
                funders::credit(store, &t.to, funder, t.amount)?;
            }
        }
    }
    for sc in settlement.surface_charges.iter().filter(|sc| sc.drawn > 0) {
        if sc.purse.has_funders() {
            funders::draw(store, &sc.purse, sc.drawn)?;
        }
    }
    for charge in &settlement.charges {
        if let Some(funded) = owners.get(&charge.wallet).filter(|o| o.has_funders()) {
            funders::draw(store, funded, charge.comm_rev + charge.compute_rev)?;
        }
    }
    for t in settlement.transfers.iter().filter(|t| t.kind.is_rev()) {
        if let Source::Wallet(from) = &t.from {
            if from.has_funders() {
                funders::draw(store, from, t.amount)?;
            }
        }
    }
    Ok(())
}
