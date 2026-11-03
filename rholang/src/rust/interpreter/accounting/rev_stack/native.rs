use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    cost_signature, BindPattern, Bundle, CostSignature, ListParWithRandom, Par, TaggedContinuation,
};
use models::rust::rholang::implicits::single_expr;
use rspace_plus_plus::rspace::hot_store::HotStore;

use super::channel::wallet_channel;
use super::funders;
use super::compute::compute_wallet_owner;
use super::hooks::{replace_rev_hook, seal_authorizes, RevHook};
use super::seal;
use super::grant::Grant;
use super::hooks::hook_authority;
use super::process::process_owner;
use super::journal::{journal_open, journaled_transfer, visible_balance, JournalEntry};
use super::kind::{stack_kind, TokenKind};
use super::ops::RevStacks;
use super::owner::WalletOwner;
use super::region::admissible;
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
    block_number: i64,
) -> Option<Par> {
    let result = match command {
        "find" => owner_of(arg).map(|owner| wallet_handle(&owner)),
        "balance" => with_stacks(store, |stacks| {
            let owner = owner_of(arg)?;
            let balance = stacks.balance(&owner).map_err(|e| e.to_string())?;
            let reserved = reservation(stacks.cells(), &owner).map_err(|e| e.to_string())?;
            let shown = visible_balance(balance, reserved.visible);
            Ok(RhoNumber::create_par(shown as i64))
        }),
        "kindBalance" => with_stacks(store, |stacks| {
            let (kind, owner) = kind_args(arg)?;
            let wallets = stacks.of_kind(&kind);
            let balance = wallets.balance(&owner).map_err(|e| e.to_string())?;
            Ok(RhoNumber::create_par(balance as i64))
        }),
        "transfer" => with_stacks(store, |stacks| {
            let (from, presented, to, amount) = transfer_args(arg)?;
            if !presented && !admissible(&from, signers) {
                return Err("the payer must be a signer of this deployment".to_string());
            }
            move_rev(stacks, from, to, amount)
        }),
        "fundGrant" => with_stacks(store, |stacks| {
            let usage = "fundGrant expects (deployerId, (slot, opens, closes), amount)";
            let [payer, grant, amount] = tuple_of(arg, usage)?.try_into().map_err(|_| usage)?;
            let from = payer_of(&payer)?;
            if !admissible(&from, signers) {
                return Err("the payer must be a signer of this deployment".to_string());
            }
            move_rev(stacks, from, grant_of(&grant)?.owner(), amount_of(&amount)?)
        }),
        "drawGrant" => with_stacks(store, |stacks| {
            let usage = "drawGrant expects ((slot, opens, closes), payee, amount)";
            let [grant, payee, amount] = tuple_of(arg, usage)?.try_into().map_err(|_| usage)?;
            let grant = grant_of(&grant)?;
            if !grant.open_at(block_number) {
                return Err(format!(
                    "the grant is open from block {} to block {}, not at block {block_number}",
                    grant.opens, grant.closes
                ));
            }
            move_rev(stacks, grant.owner(), owner_of(&payee)?, amount_of(&amount)?)
        }),
        "returnGrant" => with_stacks(store, |stacks| {
            let usage = "returnGrant expects ((slot, opens, closes), funder)";
            let [grant, funder] = tuple_of(arg, usage)?.try_into().map_err(|_| usage)?;
            let grant = grant_of(&grant)?;
            if block_number <= grant.closes {
                return Err(format!(
                    "the grant closes at block {}, so its shares return only after it",
                    grant.closes
                ));
            }
            let (owner, funder) = (grant.owner(), owner_of(&funder)?);
            let share = funders::share(stacks.cells(), &owner, &funder).map_err(|e| e.to_string())?;
            if share == 0 {
                return Err("the funder holds no share of this grant".to_string());
            }
            if journal_open(stacks.cells()) {
                let balance = stacks.balance(&owner).map_err(|e| e.to_string())?;
                return journaled_transfer(
                    stacks.cells(),
                    balance,
                    0,
                    JournalEntry::refusal(owner, funder, share),
                )
                .map(|_| Par::default())
                .map_err(transfer_error);
            }
            stacks
                .transfer_keeping(&owner, &funder, share, 0)
                .map_err(transfer_error)?;
            funders::refuse(stacks.cells(), &owner, &funder, share).map_err(|e| e.to_string())?;
            Ok(Par::default())
        }),
        "spendCompute" => with_stacks(store, |stacks| {
            let usage = "spendCompute expects (payee, amount)";
            let [payee, amount] = tuple_of(arg, usage)?.try_into().map_err(|_| usage)?;
            let seal = seal::current_owner()
                .ok_or("spending from the compute wallet needs a seal")?;
            if !seal_authorizes(stacks.cells(), &seal).map_err(|e| e.to_string())? {
                return Err(
                    "spending from the compute wallet needs a seal of the hook authority's quorum or of the installed hook"
                        .to_string(),
                );
            }
            let (from, to) = (compute_wallet_owner(), owner_of(&payee)?);
            if to == from {
                return Err("the compute wallet cannot pay itself".to_string());
            }
            move_rev(stacks, from, to, amount_of(&amount)?)
        }),
        "replaceHook" => with_stacks(store, |stacks| {
            let seal = seal::current_owner().ok_or("replacing a hook needs a seal")?;
            if journal_open(stacks.cells()) {
                return Err("a hook cannot be replaced inside a gated run".to_string());
            }
            let hook = if *arg == Par::default() {
                RevHook::Noop
            } else {
                RevHook::Contract(arg.clone())
            };
            replace_rev_hook(stacks.cells(), &seal, hook)
                .map(|_| Par::default())
                .map_err(|e| e.to_string())
        }),
        "grantBalance" => with_stacks(store, |stacks| {
            let balance = stacks
                .balance(&grant_of(arg)?.owner())
                .map_err(|e| e.to_string())?;
            Ok(RhoNumber::create_par(balance as i64))
        }),
        "fund" => with_stacks(store, |stacks| {
            let (from, process, amount) = fund_args(arg)?;
            if !admissible(&from, signers) {
                return Err("the payer must be a signer of this deployment".to_string());
            }
            let held = reservation(stacks.cells(), &from)
                .map_err(|e| e.to_string())?
                .held;
            if journal_open(stacks.cells()) {
                let balance = stacks.balance(&from).map_err(|e| e.to_string())?;
                return journaled_transfer(
                    stacks.cells(),
                    balance,
                    held,
                    JournalEntry::transfer(from, process, amount),
                )
                .map(|_| Par::default())
                .map_err(transfer_error);
            }
            stacks
                .transfer_keeping(&from, &process, amount, held)
                .map_err(transfer_error)?;
            funders::credit(stacks.cells(), &process, &from, amount).map_err(|e| e.to_string())?;
            Ok(Par::default())
        }),
        "share" => with_stacks(store, |stacks| {
            let (process, funder) = share_args(arg)?;
            let share = funders::share(stacks.cells(), &process, &funder).map_err(|e| e.to_string())?;
            Ok(RhoNumber::create_par(share as i64))
        }),
        "refuse" => with_stacks(store, |stacks| {
            let (process, funder) = share_args(arg)?;
            let authority = hook_authority(stacks.cells())
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "this shard has no hook authority".to_string())?;
            if !authority.authorizes(signers) {
                return Err("only the hook authority may refuse a funder's share".to_string());
            }
            let share = funders::share(stacks.cells(), &process, &funder).map_err(|e| e.to_string())?;
            if share == 0 {
                return Err("the funder holds no share of this process".to_string());
            }
            let held = reservation(stacks.cells(), &process)
                .map_err(|e| e.to_string())?
                .held;
            if journal_open(stacks.cells()) {
                let balance = stacks.balance(&process).map_err(|e| e.to_string())?;
                return journaled_transfer(
                    stacks.cells(),
                    balance,
                    held,
                    JournalEntry::refusal(process, funder, share),
                )
                .map(|_| Par::default())
                .map_err(transfer_error);
            }
            stacks
                .transfer_keeping(&process, &funder, share, held)
                .map_err(transfer_error)?;
            funders::refuse(stacks.cells(), &process, &funder, share).map_err(|e| e.to_string())?;
            Ok(Par::default())
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
    if let Some(ExprInstance::ETupleBody(tuple)) = single_expr(par).and_then(|e| e.expr_instance) {
        let components = tuple
            .ps
            .iter()
            .map(owner_of)
            .collect::<Result<Vec<_>, _>>()?;
        return WalletOwner::compound(components).map_err(|e| e.to_string());
    }
    Err("expected a REV address, a deployer id, an unforgeable name or a tuple of them".to_string())
}

fn transfer_args(arg: &Par) -> Result<(WalletOwner, bool, WalletOwner, RevAmount), String> {
    let usage = "transfer expects (deployerId or slot, toAddress, amount)";
    let Some(ExprInstance::ETupleBody(tuple)) = single_expr(arg).and_then(|e| e.expr_instance)
    else {
        return Err(usage.to_string());
    };
    let [deployer_id, to, amount] = tuple.ps.as_slice() else {
        return Err(usage.to_string());
    };
    let (from, presented) = match RhoName::unapply(deployer_id) {
        Some(slot) => (WalletOwner::from_unforgeable(&slot), true),
        None => (payer_of(deployer_id)?, false),
    };
    Ok((from, presented, owner_of(to)?, amount_of(amount)?))
}

fn kind_args(arg: &Par) -> Result<(TokenKind, WalletOwner), String> {
    let usage = "kindBalance expects (kind, owner)";
    let Some(ExprInstance::ETupleBody(tuple)) = single_expr(arg).and_then(|e| e.expr_instance)
    else {
        return Err(usage.to_string());
    };
    let [kind, owner] = tuple.ps.as_slice() else {
        return Err(usage.to_string());
    };
    let signature = CostSignature {
        value: Some(cost_signature::Value::Name(kind.clone())),
    };
    let kind = stack_kind(&signature)
        .ok_or_else(|| "a kind is an unforgeable name or its bundle0".to_string())?
        .kind;
    Ok((kind, owner_of(owner)?))
}

fn payer_of(par: &Par) -> Result<WalletOwner, String> {
    if let Some(deployer_id) = RhoDeployerId::unapply(par) {
        return WalletOwner::from_deployer_id(&deployer_id).map_err(|e| e.to_string());
    }
    if let Some(ExprInstance::ETupleBody(tuple)) = single_expr(par).and_then(|e| e.expr_instance) {
        let components = tuple
            .ps
            .iter()
            .map(payer_of)
            .collect::<Result<Vec<_>, _>>()?;
        return WalletOwner::compound(components).map_err(|e| e.to_string());
    }
    Err("the payer must be a signer's deployer id, or a tuple of them".to_string())
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

pub fn bond_term(amount: i64) -> Result<String, String> {
    if amount <= 0 {
        return Err("Amount must be positive value".to_string());
    }
    Ok(format!(
        "new rb(`rho:rev:bond`), deployerId(`rho:system:deployerId`), \
         deployId(`rho:system:deployId`), rl(`rho:registry:lookup`), posCh, bonded in {{\n  \
         rb!(\"bond\", (*deployerId, {amount}), *bonded) |\n  \
         for (@(true, receipt) <- bonded) {{\n    \
         rl!(`rho:system:pos`, *posCh) |\n    \
         for (@(_, PoS) <- posCh) {{ @PoS!(\"bond\", receipt, {amount}, *deployId, *deployId) }}\n  \
         }} |\n  \
         for (@(false, message) <- bonded) {{ deployId!((false, message)) }}\n}}\n"
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

fn tuple_of(arg: &Par, usage: &str) -> Result<Vec<Par>, String> {
    match single_expr(arg).and_then(|e| e.expr_instance) {
        Some(ExprInstance::ETupleBody(tuple)) => Ok(tuple.ps),
        _ => Err(usage.to_string()),
    }
}

fn fund_args(arg: &Par) -> Result<(WalletOwner, WalletOwner, RevAmount), String> {
    let usage = "fund expects (deployerId, process, amount)";
    let [deployer_id, process, amount] = tuple_of(arg, usage)?.try_into().map_err(|_| usage)?;
    let amount = RhoNumber::unapply(&amount)
        .filter(|a| *a > 0)
        .ok_or_else(|| "Amount must be positive value".to_string())?;
    Ok((payer_of(&deployer_id)?, process_owner(&process), amount as RevAmount))
}

fn share_args(arg: &Par) -> Result<(WalletOwner, WalletOwner), String> {
    let usage = "expects (process, funder)";
    let [process, funder] = tuple_of(arg, usage)?.try_into().map_err(|_| usage)?;
    Ok((process_owner(&process), owner_of(&funder)?))
}

fn amount_of(par: &Par) -> Result<RevAmount, String> {
    RhoNumber::unapply(par)
        .filter(|a| *a > 0)
        .map(|a| a as RevAmount)
        .ok_or_else(|| "Amount must be positive value".to_string())
}

fn grant_of(par: &Par) -> Result<Grant, String> {
    let usage = "a grant is (slot, opens, closes)";
    let [slot, opens, closes] = tuple_of(par, usage)?.try_into().map_err(|_| usage)?;
    let slot = RhoName::unapply(&slot).ok_or("a grant's slot must be an unforgeable name")?;
    let opens = RhoNumber::unapply(&opens).ok_or(usage)?;
    let closes = RhoNumber::unapply(&closes).ok_or(usage)?;
    Grant::new(slot.id, opens, closes)
}

fn move_rev(
    stacks: &RevStacks,
    from: WalletOwner,
    to: WalletOwner,
    amount: RevAmount,
) -> Result<Par, String> {
    let held = reservation(stacks.cells(), &from)
        .map_err(|e| e.to_string())?
        .held;
    if journal_open(stacks.cells()) {
        let balance = stacks.balance(&from).map_err(|e| e.to_string())?;
        return journaled_transfer(
            stacks.cells(),
            balance,
            held,
            JournalEntry::transfer(from, to, amount),
        )
        .map(|_| Par::default())
        .map_err(transfer_error);
    }
    stacks
        .transfer_keeping(&from, &to, amount, held)
        .map_err(transfer_error)?;
    if to.has_funders() {
        funders::credit(stacks.cells(), &to, &from, amount).map_err(|e| e.to_string())?;
    }
    if from.has_funders() {
        funders::draw(stacks.cells(), &from, amount).map_err(|e| e.to_string())?;
    }
    Ok(Par::default())
}
