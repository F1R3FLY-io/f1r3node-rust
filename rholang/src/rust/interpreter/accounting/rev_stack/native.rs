use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, Bundle, ListParWithRandom, Par, TaggedContinuation};
use models::rust::rholang::implicits::single_expr;
use rspace_plus_plus::rspace::hot_store::HotStore;

use super::channel::wallet_channel;
use super::journal::{journal_open, journaled_transfer, visible_balance, JournalEntry};
use super::ops::RevStacks;
use super::owner::WalletOwner;
use super::reserve::reservation;
use super::{RevAmount, RevStackError};
use crate::rust::interpreter::rho_type::{
    RhoBoolean, RhoDeployerId, RhoName, RhoNumber, RhoString, RhoTuple2,
};

pub const REV_WALLET_URN: &str = "rho:rev:wallet";

pub type RhoHotStore =
    Arc<Box<dyn HotStore<Par, BindPattern, ListParWithRandom, TaggedContinuation>>>;

pub fn handle(store: Option<RhoHotStore>, command: &str, arg: &Par) -> Option<Par> {
    let result = match command {
        "find" => owner_of(arg).map(|owner| wallet_handle(&owner)),
        "balance" => with_stacks(store, |stacks| {
            let owner = owner_of(arg)?;
            let balance = stacks.balance(&owner).map_err(|e| e.to_string())?;
            let reserved = reservation(stacks.cells(), &owner).map_err(|e| e.to_string())?;
            let shown = visible_balance(stacks.cells(), &owner, balance, reserved.visible)
                .map_err(|e| e.to_string())?;
            Ok(RhoNumber::create_par(shown as i64))
        }),
        "transfer" => with_stacks(store, |stacks| {
            let (from, to, amount) = transfer_args(arg)?;
            let held = reservation(stacks.cells(), &from)
                .map_err(|e| e.to_string())?
                .held;
            if journal_open(stacks.cells()) {
                let balance = stacks.balance(&from).map_err(|e| e.to_string())?;
                return journaled_transfer(
                    stacks.cells(),
                    balance,
                    held,
                    JournalEntry { from, to, amount },
                )
                .map(|_| Par::default())
                .map_err(transfer_error);
            }
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
