use crypto::rust::public_key::PublicKey;
use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::cost_signature::Value;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{Connective, CostSignature, Expr, New, Par};
use prost::Message;

use super::kind::is_stack_body;
use super::{RevStackError, WalletOwner};

pub fn name_owner(name: &Par) -> Option<WalletOwner> {
    let solely_unforgeable = name.sends.is_empty()
        && name.receives.is_empty()
        && name.news.is_empty()
        && name.exprs.is_empty()
        && name.matches.is_empty()
        && name.bundles.is_empty()
        && name.connectives.is_empty()
        && name.conditionals.is_empty()
        && name.cost_signed_terms.is_empty()
        && name.cost_stacks.is_empty();
    match (solely_unforgeable, name.unforgeables.as_slice()) {
        (true, [unforgeable]) => match &unforgeable.unf_instance {
            Some(UnfInstance::GDeployerIdBody(id)) => {
                WalletOwner::from_public_key(&PublicKey::from_bytes(&id.public_key)).ok()
            }
            Some(UnfInstance::GPrivateBody(private)) => {
                Some(WalletOwner::from_unforgeable(private))
            }
            _ => None,
        },
        _ => None,
    }
}

pub fn signature_owner(signature: &CostSignature) -> Result<WalletOwner, RevStackError> {
    let unresolved = || {
        RevStackError::InvalidOwner(format!(
            "the signature names no wallet: {:?}",
            signature.value
        ))
    };
    match &signature.value {
        Some(Value::Name(name)) => name_owner(name).ok_or_else(unresolved),
        Some(Value::Compound(compound)) => WalletOwner::compound(
            compound
                .elements
                .iter()
                .map(signature_owner)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        _ => Err(unresolved()),
    }
}

pub fn admissible(owner: &WalletOwner, signers: &[WalletOwner]) -> bool {
    match owner {
        WalletOwner::Compound(components) => components.iter().all(|c| signers.contains(c)),
        single => signers.contains(single),
    }
}

pub fn paying_owners(
    deployer: &WalletOwner,
    named: &[WalletOwner],
    joint: &[WalletOwner],
) -> Vec<WalletOwner> {
    if named.is_empty() {
        return vec![deployer.clone()];
    }
    let mut owners: Vec<WalletOwner> = Vec::new();
    for owner in named {
        let components = match owner {
            compound if joint.contains(compound) => vec![compound.clone()],
            WalletOwner::Compound(components) => components.clone(),
            single => vec![single.clone()],
        };
        for component in components {
            if !owners.contains(&component) {
                owners.push(component);
            }
        }
    }
    owners.sort_by_cached_key(WalletOwner::canonical_bytes);
    owners
}

fn unresolved_owner(signature: &CostSignature) -> WalletOwner {
    WalletOwner::Unforgeable(signature.encode_to_vec())
}

pub fn named_owners(term: &Par) -> Vec<WalletOwner> {
    let mut named = Vec::new();
    let mut scope = Vec::new();
    visit_par(term, &mut scope, &mut named);
    named
}

type Scope = Vec<Option<WalletOwner>>;

fn static_owner(signature: &CostSignature, scope: &Scope) -> Option<WalletOwner> {
    match &signature.value {
        Some(Value::BoundLevel(index)) => usize::try_from(*index)
            .ok()
            .and_then(|index| scope.len().checked_sub(index + 1))
            .and_then(|position| scope[position].clone()),
        Some(Value::Name(name)) => name_owner(name),
        Some(Value::Compound(compound)) => compound
            .elements
            .iter()
            .map(|element| static_owner(element, scope))
            .collect::<Option<Vec<_>>>()
            .and_then(|owners| WalletOwner::compound(owners).ok()),
        _ => None,
    }
}

fn names_a_kind(signature: &CostSignature, scope: &Scope) -> bool {
    match &signature.value {
        Some(Value::BoundLevel(_)) => static_owner(signature, scope).is_none(),
        Some(Value::Name(_)) => super::kind::stack_kind(signature).is_some(),
        _ => false,
    }
}

fn name_signature(signature: &CostSignature, scope: &mut Scope, named: &mut Vec<WalletOwner>) {
    named.push(static_owner(signature, scope).unwrap_or_else(|| unresolved_owner(signature)));
    visit_signature_pars(signature, scope, named);
}

fn visit_signature_pars(
    signature: &CostSignature,
    scope: &mut Scope,
    named: &mut Vec<WalletOwner>,
) {
    match &signature.value {
        Some(Value::Quote(par)) | Some(Value::Name(par)) => visit_par(par, scope, named),
        Some(Value::Compound(compound)) => compound
            .elements
            .iter()
            .for_each(|element| visit_signature_pars(element, scope, named)),
        _ => {}
    }
}

fn with_binders(
    scope: &mut Scope,
    binders: Vec<Option<WalletOwner>>,
    body: Option<&Par>,
    named: &mut Vec<WalletOwner>,
) {
    let depth = scope.len();
    scope.extend(binders);
    if let Some(body) = body {
        visit_par(body, scope, named);
    }
    scope.truncate(depth);
}

fn new_binders(new: &New) -> Vec<Option<WalletOwner>> {
    let count = usize::try_from(new.bind_count).unwrap_or(0);
    let fresh = count.saturating_sub(new.uri.len());
    std::iter::repeat_n(None, fresh)
        .chain(
            new.uri
                .iter()
                .map(|uri| new.injections.get(uri).and_then(name_owner)),
        )
        .collect()
}

fn visit_par(par: &Par, scope: &mut Scope, named: &mut Vec<WalletOwner>) {
    for term in &par.cost_signed_terms {
        let kind_stack = term.body.as_ref().is_some_and(is_stack_body)
            && term
                .signature
                .as_ref()
                .is_some_and(|signature| names_a_kind(signature, scope));
        if let (Some(signature), false) = (&term.signature, kind_stack) {
            name_signature(signature, scope, named);
        }
        if let Some(body) = &term.body {
            visit_par(body, scope, named);
        }
    }
    for stack in &par.cost_stacks {
        for cell in &stack.cells {
            visit_signature_pars(cell, scope, named);
        }
    }
    for send in &par.sends {
        send.chan
            .iter()
            .chain(&send.data)
            .for_each(|p| visit_par(p, scope, named));
    }
    for receive in &par.receives {
        for bind in &receive.binds {
            if let Some(signature) = &bind.cost_signature {
                name_signature(signature, scope, named);
            }
            bind.source.iter().for_each(|p| visit_par(p, scope, named));
        }
        let bound = usize::try_from(receive.bind_count).unwrap_or(0);
        with_binders(scope, vec![None; bound], receive.body.as_ref(), named);
    }
    for new in &par.news {
        with_binders(scope, new_binders(new), new.p.as_ref(), named);
    }
    for matched in &par.matches {
        matched
            .target
            .iter()
            .for_each(|p| visit_par(p, scope, named));
        for case in &matched.cases {
            let bound = usize::try_from(case.free_count).unwrap_or(0);
            with_binders(scope, vec![None; bound], case.source.as_ref(), named);
        }
    }
    for bundle in &par.bundles {
        bundle.body.iter().for_each(|p| visit_par(p, scope, named));
    }
    for conditional in &par.conditionals {
        [
            &conditional.condition,
            &conditional.if_true,
            &conditional.if_false,
        ]
        .into_iter()
        .flatten()
        .for_each(|p| visit_par(p, scope, named));
    }
    for connective in &par.connectives {
        visit_connective(connective, scope, named);
    }
    for expr in &par.exprs {
        visit_expr(expr, scope, named);
    }
}

fn visit_connective(connective: &Connective, scope: &mut Scope, named: &mut Vec<WalletOwner>) {
    match &connective.connective_instance {
        Some(ConnectiveInstance::ConnAndBody(body))
        | Some(ConnectiveInstance::ConnOrBody(body)) => {
            body.ps.iter().for_each(|p| visit_par(p, scope, named))
        }
        Some(ConnectiveInstance::ConnNotBody(p)) => visit_par(p, scope, named),
        _ => {}
    }
}

fn visit_expr(expr: &Expr, scope: &mut Scope, named: &mut Vec<WalletOwner>) {
    let mut pars: Vec<&Par> = Vec::new();
    match &expr.expr_instance {
        Some(ExprInstance::ENotBody(e)) => pars.extend(&e.p),
        Some(ExprInstance::ENegBody(e)) => pars.extend(&e.p),
        Some(ExprInstance::EMultBody(e)) => pars.extend(e.p1.iter().chain(&e.p2)),
        Some(ExprInstance::EDivBody(e)) => pars.extend(e.p1.iter().chain(&e.p2)),
        Some(ExprInstance::EModBody(e)) => pars.extend(e.p1.iter().chain(&e.p2)),
        Some(ExprInstance::EPlusBody(e)) => pars.extend(e.p1.iter().chain(&e.p2)),
        Some(ExprInstance::EMinusBody(e)) => pars.extend(e.p1.iter().chain(&e.p2)),
        Some(ExprInstance::ELtBody(e)) => pars.extend(e.p1.iter().chain(&e.p2)),
        Some(ExprInstance::ELteBody(e)) => pars.extend(e.p1.iter().chain(&e.p2)),
        Some(ExprInstance::EGtBody(e)) => pars.extend(e.p1.iter().chain(&e.p2)),
        Some(ExprInstance::EGteBody(e)) => pars.extend(e.p1.iter().chain(&e.p2)),
        Some(ExprInstance::EEqBody(e)) => pars.extend(e.p1.iter().chain(&e.p2)),
        Some(ExprInstance::ENeqBody(e)) => pars.extend(e.p1.iter().chain(&e.p2)),
        Some(ExprInstance::EAndBody(e)) => pars.extend(e.p1.iter().chain(&e.p2)),
        Some(ExprInstance::EOrBody(e)) => pars.extend(e.p1.iter().chain(&e.p2)),
        Some(ExprInstance::EPercentPercentBody(e)) => pars.extend(e.p1.iter().chain(&e.p2)),
        Some(ExprInstance::EPlusPlusBody(e)) => pars.extend(e.p1.iter().chain(&e.p2)),
        Some(ExprInstance::EMinusMinusBody(e)) => pars.extend(e.p1.iter().chain(&e.p2)),
        Some(ExprInstance::EMatchesBody(e)) => pars.extend(e.target.iter().chain(&e.pattern)),
        Some(ExprInstance::EListBody(e)) => pars.extend(&e.ps),
        Some(ExprInstance::ETupleBody(e)) => pars.extend(&e.ps),
        Some(ExprInstance::ESetBody(e)) => pars.extend(&e.ps),
        Some(ExprInstance::EPathmapBody(e)) => pars.extend(&e.ps),
        Some(ExprInstance::EZipperBody(e)) => {
            pars.extend(e.pathmap.iter().flat_map(|pathmap| &pathmap.ps))
        }
        Some(ExprInstance::EMapBody(e)) => {
            pars.extend(e.kvs.iter().flat_map(|kv| kv.key.iter().chain(&kv.value)))
        }
        Some(ExprInstance::EMethodBody(e)) => pars.extend(e.target.iter().chain(&e.arguments)),
        _ => {}
    }
    pars.into_iter().for_each(|p| visit_par(p, scope, named));
}
