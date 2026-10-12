use std::collections::HashMap;
use std::time::Duration;

use casper::rust::genesis::contracts::vault::Vault;
use crypto::rust::public_key::PublicKey;
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::GenesisBuilder;

const VALIDATOR_PK: &str = "6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a6a";
const DELEGATOR_PK: &str = "7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b";

fn prepare_vault(hex_string: &str, balance: u64) -> Vault {
    let pk_bytes = hex::decode(hex_string).expect("Failed to decode hex string");
    let pk = PublicKey::from_bytes(&pk_bytes);

    Vault {
        vault_address: VaultAddress::from_public_key(&pk)
            .expect("Failed to create VaultAddress from public key"),
        initial_balance: balance,
    }
}

#[test]
fn pos_delegation_review_spec() {
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Runtime::new()
                .expect("Failed to create Tokio runtime")
                .block_on(async {
                    let test_object = crate::util::rholang::test_rho_loader::load_test_rho(
                        "PoSDelegationReviewTest.rho",
                    )
                    .expect("Failed to load PoSDelegationReviewTest.rho");

                    let compiled = CompiledRholangSource::new(
                        test_object,
                        HashMap::new(),
                        "PoSDelegationReviewTest.rho".to_string(),
                    )
                    .expect("Failed to compile PoSDelegationReviewTest.rho");

                    let mut genesis_parameters =
                        GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
                    genesis_parameters.2.vaults.extend([
                        prepare_vault(VALIDATOR_PK, 10_000),
                        prepare_vault(DELEGATOR_PK, 10_000),
                    ]);
                    genesis_parameters.2.proof_of_stake.minimum_bond = 2;
                    genesis_parameters.2.proof_of_stake.maximum_bond = 100_000;

                    let spec = RhoSpec::new_with_genesis_parameters(
                        compiled,
                        vec![],
                        Duration::from_secs(60),
                        genesis_parameters,
                    );

                    spec.run_tests()
                        .await
                        .expect("PoS delegation review tests failed");
                })
        })
        .expect("Failed to spawn PoS delegation review test thread")
        .join()
        .expect("PoS delegation review test thread panicked");
}
