use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::normalizer_env::normalizer_env_from_deploy;
use rholang::rust::interpreter::compiler::compiler::Compiler;

use crate::error::IntakeError;

/// The checks that need no chain state: the term must compile, as it must
/// for the node to admit it. Run off the async runtime; compiling is CPU
/// work proportional to the term.
pub fn check_term(deploy: &Signed<DeployData>) -> Result<(), IntakeError> {
    let normalizer_env = normalizer_env_from_deploy(deploy)
        .map_err(|e| IntakeError::IllegalArgument(format!("Invalid deploy parameters: {}", e)))?;
    Compiler::source_to_adt_with_normalizer_env(&deploy.data.term, normalizer_env)
        .map(|_| ())
        .map_err(|e| IntakeError::RholangBadTerm(format!("Error in parsing term: \n{}", e)))
}

#[cfg(test)]
mod tests {
    use crypto::rust::private_key::PrivateKey;
    use crypto::rust::signatures::secp256k1::Secp256k1;

    use super::*;

    fn deploy(term: &str) -> Signed<DeployData> {
        Signed::create(
            DeployData {
                term: term.to_string(),
                time_stamp: 1,
                phlo_price: 1,
                phlo_limit: 1000,
                valid_after_block_number: 0,
                shard_id: "root".to_string(),
                expiration_timestamp: None,
                parameters: Vec::new(),
            },
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[3; 32]),
        )
        .unwrap()
    }

    #[test]
    fn a_term_that_compiles_passes_and_one_that_does_not_is_refused() {
        assert_eq!(check_term(&deploy("new x in { x!(1) }")), Ok(()));
        assert!(matches!(
            check_term(&deploy("new x in { x!(")),
            Err(IntakeError::RholangBadTerm(_))
        ));
    }
}
