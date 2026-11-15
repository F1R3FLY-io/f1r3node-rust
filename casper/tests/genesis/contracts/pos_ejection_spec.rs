// Kept out of PoSTest.rho: that suite's 60 s bound catches wedges rather than
// budgeting work, and these tests would push it over.

use std::collections::HashMap;
use std::time::Duration;

use casper::rust::genesis::contracts::vault::Vault;
use crypto::rust::public_key::PublicKey;
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::GenesisBuilder;

#[test]
fn pos_ejection_spec() { run_spec("PoSEjectionTest.rho"); }

#[test]
fn pos_grace_spec() { run_spec("PoSGraceTest.rho"); }

#[test]
fn pos_overflow_spec() { run_spec("PoSOverflowTest.rho"); }

#[test]
fn pos_slash_spec() { run_spec("PoSSlashTest.rho"); }

fn run_spec(file: &'static str) {
    // Mirror pos_spec.rs: a custom-parameter genesis is an unavoidable
    // GENESIS_CACHE miss whose build runs the genesis contracts through the
    // interpreter, so use a larger stack on a dedicated thread.
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            tokio::runtime::Runtime::new()
                .expect("Failed to create Tokio runtime")
                .block_on(async {
                    let test_object = crate::util::rholang::test_rho_loader::load_test_rho(file)
                        .unwrap_or_else(|error| panic!("Failed to load {file}: {error:?}"));

                    let compiled =
                        CompiledRholangSource::new(test_object, HashMap::new(), file.to_string())
                            .unwrap_or_else(|error| panic!("Failed to compile {file}: {error:?}"));

                    let mut genesis_parameters =
                        GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
                    // Half the bond, so the fee shows on bonds of a few units.
                    genesis_parameters.2.proof_of_stake.failed_claim_fee_ppm = 500_000;
                    // A funded user whose charged deploy leaves a reward
                    // surplus in the PoS vault to distribute.
                    let payer = PublicKey::from_bytes(
                        &hex::decode("e".repeat(130)).expect("hex public key"),
                    );
                    genesis_parameters.2.vaults.push(Vault {
                        vault_address: VaultAddress::from_public_key(&payer)
                            .expect("vault address"),
                        initial_balance: 10_000.into(),
                    });
                    // Validators that can pay a rejoin's topup.
                    let validator_vaults: Vec<VaultAddress> = genesis_parameters
                        .2
                        .proof_of_stake
                        .validators
                        .iter()
                        .map(|validator| {
                            VaultAddress::from_public_key(&validator.pk).expect("vault address")
                        })
                        .collect();
                    for vault in genesis_parameters.2.vaults.iter_mut() {
                        if validator_vaults.contains(&vault.vault_address) {
                            vault.initial_balance = 1_000.into();
                        }
                    }

                    let spec = RhoSpec::new_with_genesis_parameters(
                        compiled,
                        vec![],
                        // The genesis build is not bounded by this; it bounds
                        // the test eval only, so a generous value guards a
                        // true hang without accommodating scheduling latency
                        // under a full-workspace run.
                        Duration::from_secs(600),
                        genesis_parameters,
                    );

                    spec.run_tests()
                        .await
                        .unwrap_or_else(|error| panic!("{file} failed: {error:?}"));
                })
        })
        .expect("Failed to spawn the contract test thread")
        .join()
        .expect("The contract test thread panicked");
}
