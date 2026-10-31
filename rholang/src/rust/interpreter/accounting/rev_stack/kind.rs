use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::cost_signature::Value;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{CostSignature, Par};

use super::WalletOwner;

pub const TOKEN_KIND_DOMAIN: &[u8] = b"f1r3fly/token-kind/wallet/v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KindStack {
    pub kind: Vec<u8>,
    pub issuer: bool,
}

pub fn kind_owner(kind: &[u8], owner: &WalletOwner) -> WalletOwner {
    let mut input = Vec::with_capacity(TOKEN_KIND_DOMAIN.len() + 4 + kind.len() + 64);
    input.extend_from_slice(TOKEN_KIND_DOMAIN);
    input.extend_from_slice(&(kind.len() as u32).to_be_bytes());
    input.extend_from_slice(kind);
    input.extend_from_slice(&owner.canonical_bytes());
    WalletOwner::Unforgeable(Blake2b256::hash(input))
}

pub fn is_stack_body(body: &Par) -> bool {
    !body.cost_stacks.is_empty()
        && body.sends.is_empty()
        && body.receives.is_empty()
        && body.news.is_empty()
        && body.exprs.is_empty()
        && body.matches.is_empty()
        && body.unforgeables.is_empty()
        && body.bundles.is_empty()
        && body.connectives.is_empty()
        && body.conditionals.is_empty()
        && body.cost_signed_terms.is_empty()
}

fn only_unforgeable(par: &Par) -> Option<Vec<u8>> {
    let alone = par.sends.is_empty()
        && par.receives.is_empty()
        && par.news.is_empty()
        && par.exprs.is_empty()
        && par.matches.is_empty()
        && par.bundles.is_empty()
        && par.connectives.is_empty()
        && par.conditionals.is_empty()
        && par.cost_signed_terms.is_empty()
        && par.cost_stacks.is_empty();
    match (alone, par.unforgeables.as_slice()) {
        (true, [unforgeable]) => match &unforgeable.unf_instance {
            Some(UnfInstance::GPrivateBody(private)) => Some(private.id.clone()),
            _ => None,
        },
        _ => None,
    }
}

pub fn stack_kind(signature: &CostSignature) -> Option<KindStack> {
    let Some(Value::Name(name)) = &signature.value else {
        return None;
    };
    if let Some(kind) = only_unforgeable(name) {
        return Some(KindStack { kind, issuer: true });
    }
    let alone = name.sends.is_empty()
        && name.receives.is_empty()
        && name.news.is_empty()
        && name.exprs.is_empty()
        && name.matches.is_empty()
        && name.unforgeables.is_empty()
        && name.connectives.is_empty()
        && name.conditionals.is_empty()
        && name.cost_signed_terms.is_empty()
        && name.cost_stacks.is_empty();
    match (alone, name.bundles.as_slice()) {
        (true, [bundle]) if !bundle.read_flag && !bundle.write_flag => bundle
            .body
            .as_ref()
            .and_then(only_unforgeable)
            .map(|kind| KindStack {
                kind,
                issuer: false,
            }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::{Bundle, GPrivate, GUnforgeable};

    use super::*;

    fn private(id: u8) -> Par {
        Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![id; 32] })),
        }])
    }

    fn name(par: Par) -> CostSignature {
        CostSignature {
            value: Some(Value::Name(par)),
        }
    }

    #[test]
    fn the_kind_name_mints_and_its_public_bundle_transfers() {
        assert_eq!(
            stack_kind(&name(private(1))),
            Some(KindStack {
                kind: vec![1; 32],
                issuer: true
            })
        );
        let public = Par::default().with_bundles(vec![Bundle {
            body: Some(private(1)),
            write_flag: false,
            read_flag: false,
        }]);
        assert_eq!(
            stack_kind(&name(public)),
            Some(KindStack {
                kind: vec![1; 32],
                issuer: false
            })
        );
        let writable = Par::default().with_bundles(vec![Bundle {
            body: Some(private(1)),
            write_flag: true,
            read_flag: false,
        }]);
        assert_eq!(stack_kind(&name(writable)), None);
    }

    #[test]
    fn kind_wallets_are_apart_from_rev_wallets_and_from_each_other() {
        let owner = WalletOwner::Unforgeable(vec![7; 32]);
        assert_ne!(kind_owner(&[1; 32], &owner), owner);
        assert_ne!(kind_owner(&[1; 32], &owner), kind_owner(&[2; 32], &owner));
    }
}
