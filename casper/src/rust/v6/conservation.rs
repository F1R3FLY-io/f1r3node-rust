use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, Par};
use models::rust::block::state_hash::StateHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use rholang::rust::interpreter::accounting::rev_stack::bond::bond_owner;
use rholang::rust::interpreter::accounting::rev_stack::cell::cell_amount;
use rholang::rust::interpreter::accounting::rev_stack::compute::compute_wallet_owner;
use rholang::rust::interpreter::accounting::rev_stack::genesis::{genesis_supply, legacy_bonds};
use rholang::rust::interpreter::accounting::rev_stack::journal::Source;
use rholang::rust::interpreter::accounting::rev_stack::{
    wallet_channel_id_of, CellStore, RevAmount, TokenKind, WalletOwner,
};

use super::gate::WalletId;
use super::genesis_wallets::cells_at;
use super::settle::Settlement;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

pub const CONSERVATION_AUDIT_INTERVAL_DEFAULT: u64 = 64;

const AUDIT_FAILURES_METRIC: &str = "v6.conservation.audit.failures";
const AUDIT_RUNS_METRIC: &str = "v6.conservation.audit.runs";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindAudit {
    pub kind: TokenKind,
    pub wallets: usize,
    pub held: u128,
    pub minted: u128,
}

impl KindAudit {
    pub fn conserved(&self) -> bool { self.held == self.minted }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WalletIndex {
    kinds: BTreeMap<TokenKind, BTreeSet<WalletId>>,
    mints: BTreeMap<TokenKind, u128>,
}

impl WalletIndex {
    pub fn insert(&mut self, kind: TokenKind, wallet: WalletId) {
        self.kinds.entry(kind).or_default().insert(wallet);
    }

    pub fn insert_owner(&mut self, kind: TokenKind, owner: &WalletOwner) {
        self.insert(kind.clone(), wallet_channel_id_of(owner, &kind));
    }

    pub fn wallets(&self, kind: &TokenKind) -> impl Iterator<Item = &WalletId> {
        self.kinds.get(kind).into_iter().flatten()
    }

    pub fn kinds(&self) -> impl Iterator<Item = &TokenKind> { self.kinds.keys() }

    pub fn len(&self, kind: &TokenKind) -> usize { self.kinds.get(kind).map_or(0, BTreeSet::len) }

    pub fn record_mint(&mut self, kind: TokenKind, amount: RevAmount) {
        let total = self.mints.entry(kind).or_default();
        *total = total.saturating_add(u128::from(amount));
    }

    pub fn mints(&self, kind: &TokenKind) -> u128 { self.mints.get(kind).copied().unwrap_or(0) }

    pub fn from_genesis(genesis: &BlockMessage) -> Self {
        let mut index = WalletIndex::default();
        index.insert_owner(TokenKind::rev(), &compute_wallet_owner());
        for deploy in &genesis.body.deploys {
            let term = &deploy.deploy.data.term;
            if term.contains("`rho:rev:genesis`") && term.contains("\"mint\"") {
                for address in quoted_addresses(term) {
                    if let Ok(owner) = WalletOwner::from_address(&address) {
                        index.insert_owner(TokenKind::rev(), &owner);
                    }
                }
            }
        }
        index.observe_bonds(genesis);
        index
    }

    pub fn observe_bonds(&mut self, block: &BlockMessage) {
        for bond in &block.body.state.bonds {
            self.insert_owner(TokenKind::rev(), &bond_owner(&bond.validator));
            if let Ok(owner) = WalletOwner::from_deployer_id(&bond.validator) {
                self.insert_owner(TokenKind::rev(), &owner);
            }
        }
    }

    pub fn observe_settlement(&mut self, settlement: &Settlement) {
        for charge in &settlement.charges {
            self.insert(TokenKind::rev(), charge.wallet.clone());
        }
        self.insert(TokenKind::rev(), settlement.proposer.clone());
        for transfer in &settlement.transfers {
            match &transfer.from {
                Source::Wallet(from) => self.insert_owner(transfer.kind.clone(), from),
                Source::Mint => self.record_mint(transfer.kind.clone(), transfer.amount),
            }
            self.insert_owner(transfer.kind.clone(), &transfer.to);
        }
    }
}

fn quoted_addresses(term: &str) -> Vec<String> {
    let list = term
        .split_once("\"mint\", [")
        .map_or("", |(_, rest)| rest.split(']').next().unwrap_or(""));
    list.split("(\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .map(str::to_string)
        .collect()
}

fn wallet_channel_of(id: &WalletId) -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: id.clone() })),
    }])
}

fn balance_of(store: &dyn CellStore, wallet: &WalletId) -> Result<RevAmount, CasperError> {
    store
        .get_data(&wallet_channel_of(wallet))
        .iter()
        .try_fold(0 as RevAmount, |total, datum| {
            cell_amount(datum)
                .and_then(|amount| total.checked_add(amount))
                .ok_or_else(|| {
                    CasperError::RuntimeError(
                        "conservation: a wallet holds a datum that is not an amount cell".into(),
                    )
                })
        })
}

fn native(e: rholang::rust::interpreter::accounting::rev_stack::RevStackError) -> CasperError {
    CasperError::RuntimeError(format!("conservation: {e}"))
}

fn minted(
    store: &dyn CellStore,
    index: &WalletIndex,
    kind: &TokenKind,
) -> Result<u128, CasperError> {
    if kind.is_rev() {
        return genesis_supply(store).map(u128::from).map_err(native);
    }
    Ok(index.mints(kind))
}

fn held_outside_stacks(store: &dyn CellStore, kind: &TokenKind) -> Result<u128, CasperError> {
    if kind.is_rev() {
        return legacy_bonds(store).map(u128::from).map_err(native);
    }
    Ok(0)
}

pub fn audit_kind(
    store: &dyn CellStore,
    index: &WalletIndex,
    kind: &TokenKind,
) -> Result<KindAudit, CasperError> {
    let mut held = held_outside_stacks(store, kind)?;
    for wallet in index.wallets(kind) {
        held += u128::from(balance_of(store, wallet)?);
    }
    Ok(KindAudit {
        kind: kind.clone(),
        wallets: index.len(kind),
        held,
        minted: minted(store, index, kind)?,
    })
}

pub fn audit_all(
    store: &dyn CellStore,
    index: &WalletIndex,
) -> Result<Vec<KindAudit>, CasperError> {
    let mut kinds: BTreeSet<TokenKind> = index.kinds().cloned().collect();
    kinds.insert(TokenKind::rev());
    kinds
        .iter()
        .map(|kind| audit_kind(store, index, kind))
        .collect()
}

pub async fn check_at(
    runtime_manager: &RuntimeManager,
    root: &StateHash,
    index: &WalletIndex,
) -> Result<Vec<KindAudit>, CasperError> {
    let cells = cells_at(runtime_manager, root).await?;
    audit_all(&cells, index)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditSchedule {
    pub interval: u64,
}

impl AuditSchedule {
    pub fn due(&self, block_number: i64) -> bool {
        self.interval > 0 && block_number > 0 && (block_number as u64).is_multiple_of(self.interval)
    }
}

pub struct Conservation {
    schedule: AuditSchedule,
    index: parking_lot::Mutex<WalletIndex>,
    last: parking_lot::Mutex<Option<(i64, Vec<KindAudit>)>>,
    failures: AtomicU64,
}

impl Conservation {
    pub fn new(schedule: AuditSchedule, index: WalletIndex) -> Arc<Self> {
        Arc::new(Conservation {
            schedule,
            index: parking_lot::Mutex::new(index),
            last: parking_lot::Mutex::new(None),
            failures: AtomicU64::new(0),
        })
    }

    pub fn index(&self) -> WalletIndex { self.index.lock().clone() }

    pub fn last(&self) -> Option<(i64, Vec<KindAudit>)> { self.last.lock().clone() }

    pub fn failures(&self) -> u64 { self.failures.load(Ordering::Relaxed) }

    pub fn observe(&self, block: &BlockMessage, settlement: &Settlement) {
        let mut index = self.index.lock();
        index.observe_bonds(block);
        index.observe_settlement(settlement);
    }

    pub async fn after_block(
        &self,
        runtime_manager: &RuntimeManager,
        block: &BlockMessage,
        settlement: &Settlement,
        post_root: &StateHash,
    ) -> Result<(), CasperError> {
        self.observe(block, settlement);
        let block_number = block.body.state.block_number;
        if !self.schedule.due(block_number) {
            return Ok(());
        }
        let index = self.index();
        let audits = check_at(runtime_manager, post_root, &index).await?;
        metrics::counter!(AUDIT_RUNS_METRIC).increment(1);
        for audit in audits.iter().filter(|a| !a.conserved()) {
            self.failures.fetch_add(1, Ordering::Relaxed);
            metrics::counter!(AUDIT_FAILURES_METRIC).increment(1);
            tracing::error!(
                target: "f1r3fly.v6",
                block = %hex::encode(&block.block_hash),
                block_number,
                kind = ?audit.kind,
                held = audit.held,
                minted = audit.minted,
                wallets = audit.wallets,
                "v6 conservation audit failed: the kind's holdings differ from its recorded mints"
            );
        }
        *self.last.lock() = Some((block_number, audits));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schedule_runs_every_interval_and_zero_turns_it_off() {
        let every4 = AuditSchedule { interval: 4 };
        assert!(!every4.due(0));
        assert!(!every4.due(3));
        assert!(every4.due(4));
        assert!(every4.due(8));
        assert!(!AuditSchedule { interval: 0 }.due(4));
    }

    #[test]
    fn genesis_mint_terms_give_their_addresses() {
        let term = r#"new revGenesis(`rho:rev:genesis`), ret in { revGenesis!("mint", [("1111a", 5), ("1111b", 7)], *ret) }"#;
        assert_eq!(
            quoted_addresses(term),
            vec!["1111a".to_string(), "1111b".to_string()]
        );
    }

    #[test]
    fn a_kind_is_judged_against_its_observed_mints() {
        let kind = TokenKind::from_name(vec![1; 32]).unwrap();
        let mut index = WalletIndex::default();
        index.record_mint(kind.clone(), 5);
        index.record_mint(kind.clone(), 4);
        assert_eq!(index.mints(&kind), 9);
        assert_eq!(index.mints(&TokenKind::rev()), 0);
        let audit = KindAudit {
            kind,
            wallets: 1,
            held: 9,
            minted: index.mints(&TokenKind::from_name(vec![1; 32]).unwrap()),
        };
        assert!(audit.conserved());
        assert!(!KindAudit { held: 8, ..audit }.conserved());
    }

    #[test]
    fn the_index_keeps_kinds_apart_and_ignores_repeats() {
        let mut index = WalletIndex::default();
        index.insert(TokenKind::rev(), vec![1]);
        index.insert(TokenKind::rev(), vec![1]);
        index.insert(TokenKind::from_name(vec![9]).unwrap(), vec![1]);
        assert_eq!(index.len(&TokenKind::rev()), 1);
        assert_eq!(index.len(&TokenKind::from_name(vec![9]).unwrap()), 1);
        assert_eq!(index.kinds().count(), 2);
    }
}
