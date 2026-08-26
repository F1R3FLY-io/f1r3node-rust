// See casper/src/test/scala/coop/rchain/casper/genesis/contracts/StandardDeploysSpec.scala

use casper::rust::genesis::contracts::standard_deploys;

use crate::util::genesis_builder::GenesisBuilder;

#[test]
fn should_print_public_keys_used_for_signing_standard_blessed_contracts() {
    println!("Public keys used to sign standard (blessed) contracts");
    println!("=====================================================");

    for (idx, pub_key) in standard_deploys::system_public_keys().iter().enumerate() {
        println!("{}. {}", idx + 1, hex::encode(&pub_key.bytes));
    }
}

/// Fast parse/normalize check on the new VersionedRegistry.rho embedded
/// constant. Runs the same compile path the genesis loader uses so a typo
/// in the new resource fails here before the slower RhoSpec deploy test.
#[test]
fn versioned_registry_embedded_source_compiles() {
    // `standard_deploys::versioned_registry` internally calls
    // `embedded_source(..., embedded_rho::VERSIONED_REGISTRY)`, which
    // invokes `CompiledRholangSource::new` and panics on a parse/normalize
    // error. A clean return here is the check.
    let _ = standard_deploys::versioned_registry("root");
}

#[test]
fn system_vault_embedded_source_compiles() { let _ = standard_deploys::system_vault("root"); }

#[test]
fn proof_of_stake_embedded_template_compiles() {
    let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let _ = standard_deploys::pos_generator(&parameters.2.proof_of_stake, "root");
}
