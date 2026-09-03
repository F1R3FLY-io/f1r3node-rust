// See casper/src/test/scala/coop/rchain/casper/genesis/contracts/PoSSpec.scala

use std::collections::HashMap;
use std::time::Duration;

use casper::rust::genesis::contracts::vault::Vault;
use crypto::rust::public_key::PublicKey;
use rholang::rust::build::compile_rholang_source::CompiledRholangTemplate;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::GenesisBuilder;

fn prepare_vault(vault_data: (&str, u64)) -> Vault {
    let (hex_string, balance) = vault_data;

    let pk_bytes = hex::decode(hex_string).expect("Failed to decode hex string");
    let pk = PublicKey::from_bytes(&pk_bytes);

    Vault {
        vault_address: VaultAddress::from_public_key(&pk)
            .expect("Failed to create VaultAddress from public key"),
        initial_balance: balance,
    }
}

fn test_vaults() -> Vec<Vault> {
    vec![
        ("0".repeat(130).as_str(), 10000),
        ("1".repeat(130).as_str(), 10000),
        ("2".repeat(130).as_str(), 10000),
        ("3".repeat(130).as_str(), 10000),
        ("4".repeat(130).as_str(), 10000),
        ("5".repeat(130).as_str(), 10000),
        ("6".repeat(130).as_str(), 10000),
        ("7".repeat(130).as_str(), 10000),
        ("8".repeat(130).as_str(), 10000),
        ("9".repeat(130).as_str(), 10000),
        ("a".repeat(130).as_str(), 10000),
        ("b".repeat(130).as_str(), 10000),
        ("c".repeat(130).as_str(), 10000),
        ("d".repeat(130).as_str(), 10000),
        ("e".repeat(130).as_str(), 10000),
    ]
    .into_iter()
    .map(prepare_vault)
    .collect()
}

#[test]
fn pos_spec() {
    // Note: it's not 1:1 port, we should use larger stack size (16MB) to prevent stack overflow
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let test_object =
                    crate::util::rholang::test_rho_loader::load_test_rho("PoSTest.rho")
                        .expect("Failed to load PoSTest.rho");

                // Build genesis parameters with additional test vaults
                let mut genesis_parameters =
                    GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
                genesis_parameters.2.vaults.extend(test_vaults());

                // Make the minimum-bond and maximum-bond validation gates reachable by integer
                // bond amounts so PoSTest.rho can exercise BOTH sides of each. The builder
                // defaults (minimum_bond=1, maximum_bond=i64::MAX) make "Bond is less than
                // minimum!" (needs 0<amount<1) and "Bond is greater than maximum!" (needs
                // amount>i64::MAX) unreachable for Rholang's i64 integers. maximum_bond stays
                // above the 20000 deposit-failure case so that gate ordering is preserved; all
                // test/genesis bonds are even, so the minimum_bond factor in the reward formula
                // (PoS.rhox) divides evenly and reward math is unchanged.
                genesis_parameters.2.proof_of_stake.minimum_bond = 2;
                genesis_parameters.2.proof_of_stake.maximum_bond = 100_000;
                let initial_phlogiston = genesis_parameters
                    .2
                    .proof_of_stake
                    .initial_phlogiston
                    .to_string();
                let compiled =
                    CompiledRholangTemplate::new("PoSTest.rho", &test_object, HashMap::new(), &[(
                        "initialPhlogiston",
                        &initial_phlogiston,
                    )]);

                let spec = RhoSpec::new_with_genesis_parameters(
                    compiled,
                    vec![],
                    // pos_spec runs the full 16-test PoSTest.rho through the interpreter over a
                    // custom-param genesis with test vaults (an unavoidable GENESIS_CACHE miss) —
                    // the heaviest genesis-contract spec, ~11-50s in isolation now that the harness
                    // enforces. `.config/nextest.toml` gives it `threads-required = "num-cpus"`,
                    // which isolates it within a single-crate run (`cargo nextest run -p casper`
                    // => ~11s). But under a full-workspace run (`--workspace`, ~2700 tests) other
                    // crates' interpreter/LMDB-heavy tests still contend at the OS level, starving
                    // it well past a short bound (observed ~400-900s). This timeout only exists to
                    // catch a genuine HANG (the work is deterministic and finite); a very generous
                    // value keeps scheduling latency from producing a false failure while still
                    // bounding a true hang. See .config/nextest.toml.
                    Duration::from_secs(1800),
                    genesis_parameters,
                );

                spec.run_tests().await.expect("PoSSpec tests failed");
            })
        })
        .unwrap()
        .join()
        .unwrap();
}
