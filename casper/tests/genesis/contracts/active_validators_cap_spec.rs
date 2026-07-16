// No upstream Scala analog: this closes a genesis-contract coverage gap. `PoS.pickActiveValidators`
// caps the active validator set to `numberOfActiveValidators` via `pks.take(N)` (PoS.rhox ~:770-778,
// applied at genesis PoS.rhox:168). With the default `number_of_active_validators = 100` and <= 4
// bonded validators, the `take(N)` truncation branch is never reached. Here we override the cap to 2
// against the default 4-validator genesis so truncation actually happens, then assert
// `getActiveValidators` returns a set of exactly `number_of_active_validators` (= 2).

use std::collections::HashMap;
use std::time::Duration;

use rholang::rust::build::compile_rholang_source::CompiledRholangSource;

use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::GenesisBuilder;

#[test]
fn active_validators_cap_spec() {
    // Mirror pos_spec.rs: a custom-parameter genesis is an unavoidable GENESIS_CACHE miss whose
    // build runs the genesis contracts (PoS.rhox, ...) through the interpreter, so we use a larger
    // (16MB) stack on a dedicated thread with its own Tokio runtime to avoid a stack overflow
    // during the genesis build.
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Runtime::new()
                .expect("Failed to create Tokio runtime")
                .block_on(async {
                    let test_object = crate::util::rholang::test_rho_loader::load_test_rho(
                        "ActiveValidatorsCapTest.rho",
                    )
                    .expect("Failed to load ActiveValidatorsCapTest.rho");

                    let compiled = CompiledRholangSource::new(
                        test_object,
                        HashMap::new(),
                        "ActiveValidatorsCapTest.rho".to_string(),
                    )
                    .expect("Failed to compile ActiveValidatorsCapTest.rho");

                    // The default genesis bonds 4 validators (DEFAULT_VALIDATOR_KEY_PAIRS). Override
                    // the active-validator cap to 2 (< 4) so `PoS.pickActiveValidators` truncates the
                    // active set at genesis. This distinct parameter combination is a fresh genesis
                    // build (GENESIS_CACHE is keyed on the full parameters).
                    let mut genesis_parameters =
                        GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
                    genesis_parameters.2.proof_of_stake.number_of_active_validators = 2;

                    let spec = RhoSpec::new_with_genesis_parameters(
                        compiled,
                        vec![],
                        // The genesis build is NOT bounded by this timeout (it runs before the
                        // timeout-wrapped test eval in RhoSpec::get_results); this only bounds the
                        // single lightweight test eval. A generous value guards a true hang while
                        // leaving ample headroom for scheduling latency under a full-workspace run.
                        Duration::from_secs(600),
                        genesis_parameters,
                    );

                    spec.run_tests()
                        .await
                        .expect("ActiveValidatorsCap tests failed");
                })
        })
        .expect("Failed to spawn ActiveValidatorsCap test thread")
        .join()
        .expect("ActiveValidatorsCap test thread panicked");
}
