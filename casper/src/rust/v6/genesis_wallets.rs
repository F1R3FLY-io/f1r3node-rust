use models::rust::block::state_hash::StateHash;
use rholang::rust::interpreter::accounting::rev_stack::genesis::genesis_supply;
use rholang::rust::interpreter::accounting::rev_stack::{
    HotStoreCells, RevAmount, RevStacks, WalletOwner, MAX_AMOUNT,
};
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use crate::rust::errors::CasperError;
use crate::rust::genesis::contracts::vault::Vault;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

pub fn genesis_lines(vaults: &[Vault]) -> Result<Vec<(String, RevAmount)>, CasperError> {
    let mut total: RevAmount = 0;
    let mut lines = Vec::with_capacity(vaults.len());
    for vault in vaults {
        let address = vault.vault_address.to_base58();
        let amount = RevAmount::try_from(vault.initial_balance)
            .ok()
            .filter(|a| *a <= MAX_AMOUNT)
            .ok_or_else(|| {
                CasperError::RuntimeError(format!(
                    "wallets.txt balance {} for {address} exceeds the largest REV amount {MAX_AMOUNT}",
                    vault.initial_balance
                ))
            })?;
        total = total
            .checked_add(amount)
            .filter(|t| *t <= MAX_AMOUNT)
            .ok_or_else(|| {
                CasperError::RuntimeError(format!(
                    "the wallets.txt supply exceeds the largest REV amount {MAX_AMOUNT}"
                ))
            })?;
        if amount > 0 {
            lines.push((address, amount));
        }
    }
    Ok(lines)
}

pub fn wallets_supply(vaults: &[Vault]) -> Result<RevAmount, CasperError> {
    Ok(genesis_lines(vaults)?.iter().map(|(_, a)| *a).sum())
}

pub async fn read_genesis_supply(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
) -> Result<RevAmount, CasperError> {
    let cells = cells_at(runtime_manager, state_hash).await?;
    genesis_supply(&cells).map_err(|e| CasperError::RuntimeError(e.to_string()))
}

pub async fn read_wallet_balance(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
    owner: &WalletOwner,
) -> Result<RevAmount, CasperError> {
    let cells = cells_at(runtime_manager, state_hash).await?;
    RevStacks::new(std::sync::Arc::new(cells))
        .balance(owner)
        .map_err(|e| CasperError::RuntimeError(e.to_string()))
}

async fn cells_at(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
) -> Result<HotStoreCells, CasperError> {
    let mut runtime = runtime_manager.spawn_runtime().await?;
    runtime
        .reset(&Blake2b256Hash::from_bytes_prost(state_hash))
        .await?;
    let store = runtime.reducer.space.hot_store().ok_or_else(|| {
        CasperError::RuntimeError("the runtime's space has no native REV layer".to_string())
    })?;
    Ok(HotStoreCells(store))
}
