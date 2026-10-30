use std::sync::Arc;

use crypto::rust::hash::blake2b256::Blake2b256;

use super::hooks::hook_authority;
use super::ops::{CellStore, RevStacks};
use super::owner::WalletOwner;
use super::{RevAmount, RevStackError};

pub const COMPUTE_WALLET_DOMAIN: &[u8] = b"f1r3fly/rev-stack/compute-wallet/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComputeSpender {
    Authority(Vec<WalletOwner>),
    InstalledHook,
}

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
    spender: &ComputeSpender,
    to: &WalletOwner,
    amount: RevAmount,
) -> Result<RevAmount, RevStackError> {
    if let ComputeSpender::Authority(signers) = spender {
        let authority = hook_authority(store.as_ref())?
            .ok_or_else(|| RevStackError::CorruptWallet("no hook authority at genesis".into()))?;
        if !authority.authorizes(signers) {
            return Err(RevStackError::InvalidOwner(format!(
                "spending from the compute wallet needs at least {} of the {} genesis hook addresses",
                authority.threshold(),
                authority.members().len()
            )));
        }
    }
    if *to == compute_wallet_owner() {
        return Err(RevStackError::InvalidOwner(
            "the compute wallet cannot pay itself".into(),
        ));
    }
    RevStacks::new(store).transfer(&compute_wallet_owner(), to, amount)
}
