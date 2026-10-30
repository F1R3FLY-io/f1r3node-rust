use models::rhoapi::Par;
use rholang::rust::interpreter::accounting::rev_stack::hooks::{
    hook_authority, rev_hook, HookAuthority, HookPoint, RevHook,
};
use rholang::rust::interpreter::accounting::rev_stack::{CellStore, WalletOwner};

use crate::rust::errors::CasperError;
use crate::rust::genesis::contracts::proof_of_stake::ProofOfStake;
use crate::rust::util::rholang::runtime_manager::RuntimeManager;

#[derive(Clone, Debug, PartialEq)]
pub enum HookCall {
    Noop,
    Contract(Par),
}

pub fn genesis_authority(pos: &ProofOfStake) -> Result<HookAuthority, CasperError> {
    let members = pos
        .pos_multi_sig_public_keys
        .iter()
        .map(|hex_key| {
            let bytes = hex::decode(hex_key).map_err(|e| {
                CasperError::RuntimeError(format!("hook authority key {hex_key}: {e}"))
            })?;
            WalletOwner::from_deployer_id(&bytes)
                .map_err(|e| CasperError::RuntimeError(e.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    HookAuthority::new(members, pos.pos_multi_sig_quorum)
        .map_err(|e| CasperError::RuntimeError(e.to_string()))
}

pub fn authority_addresses(authority: &HookAuthority) -> Vec<String> {
    authority
        .members()
        .iter()
        .filter_map(|m| match m {
            WalletOwner::Address(a) => Some(a.to_base58()),
            _ => None,
        })
        .collect()
}

pub fn rev_hook_call(store: &dyn CellStore, point: HookPoint) -> Result<HookCall, CasperError> {
    let hook = rev_hook(store)
        .map_err(|e| CasperError::RuntimeError(e.to_string()))?
        .unwrap_or(RevHook::Noop);
    Ok(match (point, hook) {
        (HookPoint::AfterSettlement, RevHook::Contract(p)) => HookCall::Contract(p),
        _ => HookCall::Noop,
    })
}

pub fn after_settlement(store: &dyn CellStore) -> Result<HookCall, CasperError> {
    rev_hook_call(store, HookPoint::AfterSettlement)
}

pub async fn read_hook_authority(
    runtime_manager: &RuntimeManager,
    state_hash: &models::rust::block::state_hash::StateHash,
) -> Result<Option<HookAuthority>, CasperError> {
    let cells = super::genesis_wallets::cells_at(runtime_manager, state_hash).await?;
    hook_authority(&cells).map_err(|e| CasperError::RuntimeError(e.to_string()))
}
