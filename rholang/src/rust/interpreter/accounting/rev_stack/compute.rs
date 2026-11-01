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
