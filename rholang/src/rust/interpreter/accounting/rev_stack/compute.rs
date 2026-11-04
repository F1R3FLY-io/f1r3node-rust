use std::sync::Arc;

use crypto::rust::hash::blake2b256::Blake2b256;

use super::hooks::seal_authorizes;
use super::ops::{CellStore, RevStacks};
use super::owner::WalletOwner;
use super::{RevAmount, RevStackError};

pub const COMPUTE_WALLET_DOMAIN: &[u8] = b"f1r3fly/rev-stack/compute-wallet/v1";

pub fn compute_wallet_owner() -> WalletOwner {
    WalletOwner::Unforgeable(Blake2b256::hash(COMPUTE_WALLET_DOMAIN.to_vec()))
}

pub fn compute_balance(store: Arc<dyn CellStore>) -> Result<RevAmount, RevStackError> {
    RevStacks::new(store).balance(&compute_wallet_owner())
}

pub fn deposit_compute(store: Arc<dyn CellStore>, amount: RevAmount) -> Result<(), RevStackError> {
    RevStacks::new(store).deposit(&compute_wallet_owner(), amount)
}

pub fn spend_compute(
    store: Arc<dyn CellStore>,
    seal: &WalletOwner,
    to: &WalletOwner,
    amount: RevAmount,
) -> Result<RevAmount, RevStackError> {
    if !seal_authorizes(store.as_ref(), seal)? {
        return Err(RevStackError::InvalidOwner(
            "spending from the compute wallet needs a seal of the hook authority's quorum or of the installed hook"
                .into(),
        ));
    }
    if *to == compute_wallet_owner() {
        return Err(RevStackError::InvalidOwner(
            "the compute wallet cannot pay itself".into(),
        ));
    }
    RevStacks::new(store).transfer(&compute_wallet_owner(), to, amount)
}

pub fn charge_recorded_work(
    store: Arc<dyn CellStore>,
    work: &std::collections::BTreeMap<Option<crate::rust::interpreter::accounting::comm_ledger::Surface>, u64>,
) -> Result<RevAmount, RevStackError> {
    let stacks = RevStacks::new(store.clone());
    let mut charged: RevAmount = 0;
    for (surface, phlo) in work {
        let Some(surface) = surface else { continue };
        let Some(owner) = WalletOwner::from_canonical_bytes(&surface.owner) else {
            continue;
        };
        let owed = phlo
            .checked_mul(surface.phlo_price)
            .ok_or(RevStackError::InvalidAmount(RevAmount::MAX))?;
        for payer in super::seal::purse_of(&owner) {
            let drawn = owed;
            if drawn == 0 {
                continue;
            }
            stacks.spend(&payer, drawn)?;
            if payer.has_funders() {
                super::funders::draw(store.as_ref(), &payer, drawn)?;
            }
            stacks.deposit(&compute_wallet_owner(), drawn)?;
            charged = charged
                .checked_add(drawn)
                .ok_or(RevStackError::InvalidAmount(RevAmount::MAX))?;
        }
    }
    Ok(charged)
}
