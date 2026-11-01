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
    WalletOwner::Process(Blake2b256::hash(input))
}

pub const RUNNER_CONTEXT_URNS: [&str; 5] = [
    "rho:system:deployerId",
    "rho:rchain:deployerId",
    "rho:system:deployId",
    "rho:system:cosignerId:",
    "rho:deploy:data",
];

pub fn seals_itself(quoted: &Par, body: &Par) -> bool { canonical(quoted) == canonical(body) }

pub fn is_closed(process: &Par) -> bool { process.locally_free.iter().all(|b| *b == 0) }

pub fn reads_runner_context(process: &Par) -> bool {
    let bytes = process.encode_to_vec();
    RUNNER_CONTEXT_URNS
        .iter()
        .any(|urn| bytes.windows(urn.len()).any(|w| w == urn.as_bytes()))
}

pub fn hash_seal_admissible(quoted: &Par, body: &Par) -> Result<(), String> {
    if !is_closed(quoted) || !is_closed(body) {
        return Err("a `# P` seal must name a closed process".to_string());
    }
    if !seals_itself(quoted, body) {
        return Err("a signed term may name `# P` only when its body is P".to_string());
    }
    if reads_runner_context(body) {
        return Err(
            "a `# P` body may not bind a name that resolves from the running deployment"
                .to_string(),
        );
    }
    Ok(())
}

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

pub fn self_sealed_envelope(term: &Par) -> Option<WalletOwner> {
    let [signed] = term.cost_signed_terms.as_slice() else {
        return None;
    };
    let mut rest = term.clone();
    rest.cost_signed_terms.clear();
    rest.locally_free.clear();
    rest.connective_used = false;
    if rest != Par::default() {
        return None;
    }
    let body = signed.body.as_ref()?;
    let Some(Value::Quote(quoted)) = signed.signature.as_ref().and_then(|s| s.value.as_ref())
    else {
        return None;
    };
    hash_seal_admissible(quoted, body)
        .ok()
        .map(|_| process_owner(body))
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
    fn only_a_lone_closed_self_sealed_term_is_a_self_sealed_envelope() {
        use crate::rust::interpreter::compiler::compiler::Compiler;
        use super::self_sealed_envelope;
        let term = |s: &str| Compiler::source_to_adt(s).unwrap();
        let own = term(r#"{% @"x"!(1) %}[# { @"x"!(1) }]"#);
        assert_eq!(
            self_sealed_envelope(&own),
            Some(process_owner(&term(r#"@"x"!(1)"#)))
        );
        for other in [
            r#"{% @"x"!(1) %}[# { @"x"!(1) }] | @"y"!(2)"#,
            r#"{% @"x"!(2) %}[# { @"x"!(1) }]"#,
            r#"for (z <- @"c") { {% z!(1) %}[# { z!(1) }] }"#,
            r#"@"x"!(1)"#,
        ] {
            assert_eq!(self_sealed_envelope(&term(other)), None, "{other}");
        }
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
