use std::collections::HashMap;
use std::time::Duration;

use casper::rust::genesis::contracts::vault::Vault;
use crypto::rust::public_key::PublicKey;
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::GenesisBuilder;

fn prepare_vault(hex_string: &str, balance: u64) -> Vault {
    let pk_bytes = hex::decode(hex_string).expect("failed to decode hex public key");
    let pk = PublicKey::from_bytes(&pk_bytes);

    Vault {
        vault_address: VaultAddress::from_public_key(&pk)
            .expect("failed to create vault address from public key"),
        initial_balance: balance,
    }
}

fn delegator_vaults() -> Vec<Vault> {
    (0x10u8..=0x20u8)
        .map(|byte| prepare_vault(&hex::encode(vec![byte; 65]), 10_000))
        .collect()
}

#[test]
fn pos_delegation_scalability_spec() {
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Runtime::new()
                .expect("failed to create Tokio runtime")
                .block_on(async {
                    let test_object = crate::util::rholang::test_rho_loader::load_test_rho(
                        "PoSDelegationScalabilityTest.rho",
                    )
                    .expect("failed to load PoSDelegationScalabilityTest.rho");

                    let compiled = CompiledRholangSource::new(
                        test_object,
                        HashMap::new(),
                        "PoSDelegationScalabilityTest.rho".to_string(),
                    )
                    .expect("failed to compile PoSDelegationScalabilityTest.rho");

                    let mut genesis_parameters =
                        GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
                    genesis_parameters.2.vaults.extend(delegator_vaults());
                    genesis_parameters
                        .2
                        .proof_of_stake
                        .number_of_active_validators = 1;
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
                        .expect("PoS delegation scalability tests failed");
                })
        })
        .expect("failed to spawn PoS delegation scalability test thread")
        .join()
        .expect("PoS delegation scalability test thread panicked");
}
