use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, Bundle, ListParWithRandom, Par, TaggedContinuation};
use models::rust::rholang::implicits::single_expr;
use rspace_plus_plus::rspace::hot_store::HotStore;

use super::channel::wallet_channel;
use super::ops::RevStacks;
use super::owner::WalletOwner;
use super::reserve::reservation;
use super::{RevAmount, RevStackError};
use crate::rust::interpreter::rho_type::{
    RhoBoolean, RhoDeployerId, RhoName, RhoNumber, RhoString, RhoTuple2,
};
use crate::rust::interpreter::util::vault_address::VaultAddress;

pub const REV_WALLET_URN: &str = "rho:rev:wallet";

pub type RhoHotStore =
    Arc<Box<dyn HotStore<Par, BindPattern, ListParWithRandom, TaggedContinuation>>>;

pub fn handle(
    store: Option<RhoHotStore>,
    command: &str,
    arg: &Par,
    signers: &[WalletOwner],
) -> Option<Par> {
    let result = match command {
        "find" => owner_of(arg).map(|owner| wallet_handle(&owner)),
        "balance" => with_stacks(store, |stacks| {
            let owner = owner_of(arg)?;
            let balance = stacks.balance(&owner).map_err(|e| e.to_string())?;
            let reserved = reservation(stacks.cells(), &owner).map_err(|e| e.to_string())?;
            Ok(RhoNumber::create_par(balance.saturating_sub(reserved.visible) as i64))
        }),
        "transfer" => with_stacks(store, |stacks| {
            let (from, to, amount) = transfer_args(arg)?;
            if !signers.contains(&from) {
                return Err("the payer must be a signer of this deployment".to_string());
            }
            let held = reservation(stacks.cells(), &from)
                .map_err(|e| e.to_string())?
                .held;
            stacks
                .transfer_keeping(&from, &to, amount, held)
                .map(|_| Par::default())
                .map_err(transfer_error)
        }),
        _ => return None,
    };
    Some(match result {
        Ok(value) => RhoTuple2::create_par((RhoBoolean::create_par(true), value)),
        Err(message) => RhoTuple2::create_par((
            RhoBoolean::create_par(false),
            RhoString::create_par(message),
        )),
    })
}

pub fn wallet_handle(owner: &WalletOwner) -> Par {
    Par::default().with_bundles(vec![Bundle {
        body: Some(wallet_channel(owner)),
        write_flag: false,
        read_flag: false,
    }])
}

fn with_stacks(
    store: Option<RhoHotStore>,
    f: impl FnOnce(&RevStacks) -> Result<Par, String>,
) -> Result<Par, String> {
    let store = store.ok_or_else(|| "native REV layer unavailable on this space".to_string())?;
    f(&RevStacks::over_hot_store(store))
}

fn owner_of(par: &Par) -> Result<WalletOwner, String> {
    if let Some(address) = RhoString::unapply(par) {
        return WalletOwner::from_address(&address).map_err(|e| e.to_string());
    }
    if let Some(deployer_id) = RhoDeployerId::unapply(par) {
        return WalletOwner::from_deployer_id(&deployer_id).map_err(|e| e.to_string());
    }
    if let Some(name) = RhoName::unapply(par) {
        return Ok(WalletOwner::from_unforgeable(&name));
    }
    Err("expected a REV address, a deployer id or an unforgeable name".to_string())
}

fn transfer_args(arg: &Par) -> Result<(WalletOwner, WalletOwner, RevAmount), String> {
    let usage = "transfer expects (deployerId, toAddress, amount)";
    let Some(ExprInstance::ETupleBody(tuple)) = single_expr(arg).and_then(|e| e.expr_instance)
    else {
        return Err(usage.to_string());
    };
    let [deployer_id, to, amount] = tuple.ps.as_slice() else {
        return Err(usage.to_string());
    };
    let deployer_id = RhoDeployerId::unapply(deployer_id)
        .ok_or_else(|| "the payer must be the deployer's own deployer id".to_string())?;
    let from = WalletOwner::from_deployer_id(&deployer_id).map_err(|e| e.to_string())?;
    let to = owner_of(to)?;
    let amount = RhoNumber::unapply(amount)
        .filter(|a| *a > 0)
        .ok_or_else(|| "Amount must be positive value".to_string())?;
    Ok((from, to, amount as RevAmount))
}

fn transfer_error(e: RevStackError) -> String {
    match e {
        RevStackError::InsufficientFunds { .. } => "Insufficient funds".to_string(),
        other => other.to_string(),
    }
}

pub fn transfer_term(to: &str, amount: i64) -> Result<String, String> {
    checked_address(to)?;
    if amount <= 0 {
        return Err("Amount must be positive value".to_string());
    }
    Ok(format!(
        "new w(`{REV_WALLET_URN}`), deployerId(`rho:system:deployerId`), \
         deployId(`rho:system:deployId`) in {{\n  \
         w!(\"transfer\", (*deployerId, \"{to}\", {amount}), *deployId)\n}}\n"
    ))
}

pub fn balance_term(address: &str) -> Result<String, String> {
    checked_address(address)?;
    Ok(format!(
        "new return, w(`{REV_WALLET_URN}`) in {{\n  w!(\"balance\", \"{address}\", *return)\n}}\n"
    ))
}

pub fn parse_balance(par: &Par) -> Result<RevAmount, String> {
    let (flag, value) =
        RhoTuple2::unapply(par).ok_or_else(|| "expected an (ok, value) pair".to_string())?;
    match RhoBoolean::unapply(&flag) {
        Some(true) => RhoNumber::unapply(&value)
            .filter(|n| *n >= 0)
            .map(|n| n as RevAmount)
            .ok_or_else(|| "expected a non-negative balance".to_string()),
        Some(false) => {
            Err(RhoString::unapply(&value).unwrap_or_else(|| "lookup failed".to_string()))
        }
        None => Err("expected an (ok, value) pair".to_string()),
    }
}

fn checked_address(address: &str) -> Result<(), String> {
    VaultAddress::parse(address)
        .map(|_| ())
        .map_err(|e| format!("invalid REV address: {e}"))
}
