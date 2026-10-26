use std::collections::HashMap;
use std::time::Duration;

use casper::rust::genesis::contracts::vault::Vault;
use crypto::rust::public_key::PublicKey;
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::GenesisBuilder;

#[test]
fn pos_lazy_rewards_spec() {
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let mut source =
                    crate::util::rholang::test_rho_loader::load_test_rho("PoSLazyRewardsTest.rho")
                        .unwrap();
                let mut parameters =
                    GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
                for (name, byte) in [("firstPk", 0x71), ("secondPk", 0x72), ("payerPk", 0x73)] {
                    let pk = PublicKey::from_bytes(&[byte; 65]);
                    source = source.replace(&format!("$${name}$$"), &hex::encode(&pk.bytes));
                    parameters.2.vaults.push(Vault {
                        vault_address: VaultAddress::from_public_key(&pk).unwrap(),
                        initial_balance: 10_000,
                    });
                }
                parameters.2.proof_of_stake.validators.truncate(2);
                for validator in &mut parameters.2.proof_of_stake.validators {
                    validator.stake = 100.into();
                }
                parameters.2.proof_of_stake.minimum_bond = 2.into();
                parameters.2.proof_of_stake.maximum_bond = 100_000.into();
                parameters.2.proof_of_stake.epoch_length = 10;
                parameters.2.proof_of_stake.quarantine_length = 20;
                parameters.2.proof_of_stake.number_of_active_validators = 1;
                for name in [
                    "lazy rewards follow stake changes and slash liabilities",
                    "lazy rewards retain fractions across claims",
                    "withdrawal settles rewards and preserves quarantine",
                    "rounding reserves remain funded until slashing",
                    "claims conserve rewards across slashed and live validators",
                ] {
                    let mut case_parameters = parameters.clone();
                    if name == "claims conserve rewards across slashed and live validators" {
                        case_parameters.2.proof_of_stake.number_of_active_validators = 2;
                    } else {
                        case_parameters.2.proof_of_stake.validators.truncate(1);
                    }
                    let compiled = CompiledRholangSource::new(
                        source.clone(),
                        HashMap::new(),
                        "PoSLazyRewardsTest.rho".to_string(),
                    )
                    .unwrap();
                    let spec = RhoSpec::new_with_genesis_parameters_and_enabled_tests(
                        compiled,
                        vec![],
                        Duration::from_secs(120),
                        case_parameters,
                        vec![name.to_string()],
                    );
                    let results = spec
                        .run_tests()
                        .await
                        .unwrap_or_else(|error| panic!("{name}: {error:?}"));
                    assert_eq!(results.assertions.len(), 1, "{name}");
                }
            })
        })
        .unwrap()
        .join()
        .unwrap();
}
