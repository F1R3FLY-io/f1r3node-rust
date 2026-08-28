// See casper/src/test/scala/coop/rchain/casper/genesis/contracts/PoSSpec.scala

use std::collections::HashMap;
use std::time::Duration;

use casper::rust::genesis::contracts::vault::Vault;
use crypto::rust::public_key::PublicKey;
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::util::vault_address::VaultAddress;

use crate::helper::rho_spec::{timeout_phase, RhoSpec, EVAL_TEST_SOURCE_PHASE};
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
    [
        "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e",
    ]
    .into_iter()
    .map(|token| token.repeat(130))
    .chain(
        [
            "6a", "7b", "8c", "9d", "ae", "bc", "bf", "c1", "c2", "cd", "d1", "d2", "d3", "d4",
            "de", "e1", "e2", "e3", "e4", "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8",
        ]
        .into_iter()
        .map(|token| token.repeat(65)),
    )
    .map(|pk| prepare_vault((&pk, 10000)))
    .collect()
}

fn run_pos_spec_once() -> Result<(), InterpreterError> {
    // Note: it's not 1:1 port, we should use larger stack size (16MB) to prevent stack overflow
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Runtime::new().unwrap().block_on(async {
                let test_object =
                    crate::util::rholang::test_rho_loader::load_test_rho("PoSTest.rho")
                        .expect("Failed to load PoSTest.rho");

                let compiled = CompiledRholangSource::new(
                    test_object,
                    HashMap::new(),
                    "PoSTest.rho".to_string(),
                )
                .expect("Failed to compile PoSTest.rho");

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

                let spec = RhoSpec::new_with_genesis_parameters(
                    compiled,
                    vec![],
                    // pos_spec runs the full 16-test PoSTest.rho through the interpreter over a
                    // custom-param genesis with test vaults (an unavoidable GENESIS_CACHE miss) —
                    // the heaviest genesis-contract spec, ~10-50s in isolation. The bound is a
                    // WEDGE-CATCHER: pos_spec intermittently wedges under parallel suite
                    // execution (all tokio workers parked, zero runnable tasks; the timed run
                    // localized it to the 'eval-test-source' phase — the interpreter evaluating
                    // PoSTest.rho), and the previous 1800s value burned half an hour per
                    // occurrence. The RhoSpec harness times the WHOLE pipeline and names the
                    // wedged phase in the failure message, so an expiry here is diagnostic
                    // signal, not lost work; a healthy run that trips it under load should be
                    // retried, not accommodated with a wider bound.
                    Duration::from_secs(60),
                    genesis_parameters,
                );

                spec.run_tests().await.map(|_| ())
            })
        })
        .unwrap()
        .join()
        .unwrap()
}

/// The single retry absorbs a wedge under parallel suite load. It is not a
/// performance allowance: set `RHO_SPEC_NO_RETRY=1` to surface the first
/// timeout as the failure, so a run that must measure interpreter speed does
/// not hide a regression behind the retry.
#[test]
fn pos_spec() {
    match run_pos_spec_once() {
        Err(err)
            if timeout_phase(&err) == Some(EVAL_TEST_SOURCE_PHASE)
                && std::env::var_os("RHO_SPEC_NO_RETRY").is_none() =>
        {
            eprintln!(
                "PoSSpec timed out in phase '{EVAL_TEST_SOURCE_PHASE}' under parallel load. \
                 The test will retry once (set RHO_SPEC_NO_RETRY=1 to disable): {err:?}"
            );
            run_pos_spec_once().expect("PoSSpec tests failed after timeout retry");
        }
        result => result.expect("PoSSpec tests failed"),
    }
}
