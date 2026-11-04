use std::sync::Arc;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;
use models::rust::rholang::implicits::single_expr;

use super::compute::compute_wallet_owner;
use super::journal::{journal_open, journaled_transfer, JournalEntry};
use super::native::RhoHotStore;
use super::ops::{CellStore, HotStoreCells, RevStacks};
use super::owner::WalletOwner;
use super::reserve::reservation;
use super::region::admissible;
use super::{RevAmount, RevStackError};
use crate::rust::interpreter::rho_type::{
    RhoBoolean, RhoByteArray, RhoDeployerId, RhoName, RhoNumber, RhoString, RhoTuple2,
};
use models::rhoapi::GPrivate;

pub const REV_BOND_URN: &str = "rho:rev:bond";
pub const REV_BOND_ADMIN_URN: &str = "rho:rev:bondAdmin";

pub const BOND_DOMAIN: &[u8] = b"f1r3fly/rev-stack/bond/v1";

pub fn bond_owner(validator_pk: &[u8]) -> WalletOwner {
    let mut input = BOND_DOMAIN.to_vec();
    input.extend_from_slice(validator_pk);
    WalletOwner::Unforgeable(Blake2b256::hash(input))
}

pub fn bond(
    store: Arc<dyn CellStore>,
    deployer_pk: &[u8],
    amount: RevAmount,
) -> Result<RevAmount, RevStackError> {
    let from = WalletOwner::from_deployer_id(deployer_pk)?;
    RevStacks::new(store).transfer(&from, &bond_owner(deployer_pk), amount)
}

pub fn release(
    store: Arc<dyn CellStore>,
    validator_pk: &[u8],
    amount: RevAmount,
) -> Result<RevAmount, RevStackError> {
    let to = WalletOwner::from_deployer_id(validator_pk)?;
    RevStacks::new(store).transfer(&bond_owner(validator_pk), &to, amount)
}

pub fn slash(store: Arc<dyn CellStore>, validator_pk: &[u8]) -> Result<RevAmount, RevStackError> {
    let stacks = RevStacks::new(store);
    let owner = bond_owner(validator_pk);
    let held = stacks.balance(&owner)?;
    if held > 0 {
        stacks.transfer(&owner, &compute_wallet_owner(), held)?;
    }
    Ok(held)
}

pub fn bond_balance(
    store: Arc<dyn CellStore>,
    validator_pk: &[u8],
) -> Result<RevAmount, RevStackError> {
    RevStacks::new(store).balance(&bond_owner(validator_pk))
}

const RECEIPT_DOMAIN: &[u8] = b"f1r3fly/rev-stack/bond-receipt/v1";

pub fn bond_receipt(validator_pk: &[u8], random_state: &[u8]) -> Par {
    let mut id = RECEIPT_DOMAIN.to_vec();
    id.extend_from_slice(validator_pk);
    id.extend_from_slice(&Blake2b256::hash(random_state.to_vec()));
    RhoName::create_par(GPrivate { id })
}

pub fn receipt_key(receipt: &Par) -> Result<Vec<u8>, String> {
    let refused = || "expected a bond receipt from rho:rev:bond".to_string();
    let id = RhoName::unapply(receipt).ok_or_else(refused)?.id;
    let rest = id.strip_prefix(RECEIPT_DOMAIN).ok_or_else(refused)?;
    if rest.len() <= 32 {
        return Err(refused());
    }
    Ok(rest[..rest.len() - 32].to_vec())
}

const HANDLE_DOMAIN: &[u8] = b"f1r3fly/rev-stack/validator-handle/v1";

pub fn validator_handle(validator_pk: &[u8]) -> Par {
    let mut id = HANDLE_DOMAIN.to_vec();
    id.extend_from_slice(validator_pk);
    RhoName::create_par(GPrivate { id })
}

pub fn handle_key(handle: &Par) -> Result<Vec<u8>, String> {
    let refused = || "Unknown validator handle".to_string();
    let id = RhoName::unapply(handle).ok_or_else(refused)?.id;
    let pk = id.strip_prefix(HANDLE_DOMAIN).ok_or_else(refused)?;
    if pk.is_empty() {
        return Err(refused());
    }
    Ok(pk.to_vec())
}

pub fn handle(
    store: Option<RhoHotStore>,
    command: &str,
    arg: &Par,
    authority: &[WalletOwner],
    random_state: &[u8],
) -> Option<Par> {
    let cells = || -> Result<Arc<dyn CellStore>, String> {
        let store = store
            .clone()
            .ok_or_else(|| "native REV layer unavailable".to_string())?;
        Ok(Arc::new(HotStoreCells(store)))
    };
    let result = match command {
        "bond" => tuple(arg, 2).and_then(|t| {
            let pk = RhoDeployerId::unapply(&t[0])
                .ok_or_else(|| "the bonder must be the deployer's own deployer id".to_string())?;
            let bonder = WalletOwner::from_deployer_id(&pk).map_err(|e| e.to_string())?;
            if !admissible(&bonder, authority) {
                return Err("the bonder must hold the enclosing seal".to_string());
            }
            let amount = positive(&t[1])?;
            let cells = cells()?;
            if journal_open(cells.as_ref()) {
                let from = WalletOwner::from_deployer_id(&pk).map_err(|e| e.to_string())?;
                let stacks = RevStacks::new(cells.clone());
                let balance = stacks.balance(&from).map_err(|e| e.to_string())?;
                let held = reservation(cells.as_ref(), &from)
                    .map_err(|e| e.to_string())?
                    .held;
                return journaled_transfer(
                    cells.as_ref(),
                    balance,
                    held,
                    JournalEntry::transfer(from, bond_owner(&pk), amount),
                )
                .map(|_| bond_receipt(&pk, random_state))
                .map_err(funds_error);
            }
            bond(cells, &pk, amount)
                .map(|_| bond_receipt(&pk, random_state))
                .map_err(funds_error)
        }),
        "handle" => {
            let pk = RhoDeployerId::unapply(arg)
                .ok_or_else(|| "expected the validator's deployer id".to_string());
            pk.and_then(|pk| {
                let validator = WalletOwner::from_deployer_id(&pk).map_err(|e| e.to_string())?;
                if !admissible(&validator, authority) {
                    return Err("the validator must hold the enclosing seal".to_string());
                }
                Ok(validator_handle(&pk))
            })
        }
        "balance" => bytes(arg).and_then(|pk| {
            bond_balance(cells()?, &pk)
                .map(|b| RhoNumber::create_par(b as i64))
                .map_err(|e| e.to_string())
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

fn tuple(arg: &Par, n: usize) -> Result<Vec<Par>, String> {
    match single_expr(arg).and_then(|e| e.expr_instance) {
        Some(ExprInstance::ETupleBody(t)) if t.ps.len() == n => Ok(t.ps),
        _ => Err(format!("expected a tuple of {n}")),
    }
}

fn bytes(par: &Par) -> Result<Vec<u8>, String> {
    RhoByteArray::unapply(par).ok_or_else(|| "expected a public key as bytes".to_string())
}

fn positive(par: &Par) -> Result<RevAmount, String> {
    RhoNumber::unapply(par)
        .filter(|a| *a > 0)
        .map(|a| a as RevAmount)
        .ok_or_else(|| "Amount must be positive value".to_string())
}

fn funds_error(e: RevStackError) -> String {
    match e {
        RevStackError::InsufficientFunds { .. } => "Insufficient funds".to_string(),
        other => other.to_string(),
    }
}

pub fn admin_handle(store: Option<RhoHotStore>, command: &str, arg: &Par) -> Option<Par> {
    let cells = || -> Result<Arc<dyn CellStore>, String> {
        let store = store
            .clone()
            .ok_or_else(|| "native REV layer unavailable".to_string())?;
        Ok(Arc::new(HotStoreCells(store)))
    };
    let result = match command {
        "release" => tuple(arg, 2).and_then(|t| {
            let pk = bytes(&t[0])?;
            let amount = positive(&t[1])?;
            release(cells()?, &pk, amount)
                .map(|_| Par::default())
                .map_err(funds_error)
        }),
        "receipt" => receipt_key(arg).map(|pk| RhoByteArray::create_par(pk)),
        "handleKey" => handle_key(arg).map(|pk| RhoByteArray::create_par(pk)),
        "handleOf" => bytes(arg).map(|pk| validator_handle(&pk)),
        "slash" => bytes(arg).and_then(|pk| {
            slash(cells()?, &pk)
                .map(|held| RhoNumber::create_par(held as i64))
                .map_err(|e| e.to_string())
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
