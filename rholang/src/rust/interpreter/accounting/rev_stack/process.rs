use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostSignature, Par};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use prost::Message;

use super::owner::WalletOwner;

pub const PROCESS_WALLET_DOMAIN: &[u8] = b"f1r3fly/rev-stack/process/v1";

fn canonical(process: &Par) -> Par { ParSortMatcher::sort_match(process).term }

pub fn process_owner(process: &Par) -> WalletOwner {
    let mut input = PROCESS_WALLET_DOMAIN.to_vec();
    input.extend(canonical(process).encode_to_vec());
    WalletOwner::Unforgeable(Blake2b256::hash(input))
}

pub fn seals_itself(quoted: &Par, body: &Par) -> bool { canonical(quoted) == canonical(body) }

pub fn split_hash_seals(signature: &CostSignature) -> (Vec<Par>, Vec<CostSignature>) {
    let mut hashes = Vec::new();
    let mut others = Vec::new();
    let mut pending = vec![signature.clone()];
    while let Some(next) = pending.pop() {
        match next.value {
            Some(Value::Quote(process)) => hashes.push(process),
            Some(Value::Compound(compound)) => pending.extend(compound.elements),
            _ => others.push(next),
        }
    }
    (hashes, others)
}

#[cfg(test)]
mod tests {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{Expr, Par};

    use super::{process_owner, seals_itself};

    fn number(n: i64) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(n)),
        }])
    }

    fn both(a: Par, b: Par) -> Par {
        let mut exprs = a.exprs;
        exprs.extend(b.exprs);
        Par::default().with_exprs(exprs)
    }

    #[test]
    fn a_process_owner_depends_only_on_the_canonical_process() {
        assert_eq!(
            process_owner(&both(number(1), number(2))),
            process_owner(&both(number(2), number(1)))
        );
        assert_ne!(process_owner(&number(1)), process_owner(&number(2)));
    }

    #[test]
    fn a_hash_seal_seals_only_its_own_process() {
        assert!(seals_itself(
            &both(number(1), number(2)),
            &both(number(2), number(1))
        ));
        assert!(!seals_itself(&number(1), &number(2)));
    }
}
