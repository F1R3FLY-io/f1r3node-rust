// Spec for the Versioned Registry FIP rollout (Step 3).
//
// Does NOT use RhoSpec — see ../../regver-known-issues.md item #2. The
// RhoSpec harness silently passes across the existing suite, so this
// spec calls `get_results` directly and walks the recorded
// `TestResultCollector` with an explicit non-emptiness guard so a
// vacuous pass would surface as a failure.

use std::collections::HashMap;
use std::sync::Arc;

use casper::rust::genesis::contracts::standard_deploys;
use casper::rust::helper::test_result_collector::TestResultCollector;
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;
use rholang::rust::interpreter::pretty_printer::PrettyPrinter;

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::get_results;
use crate::util::genesis_builder::GenesisBuilder;

/// Test names the probe in `VersionedRegistryTest.rho` is required to
/// register. If any is missing from `result.assertions` the test fails
/// loudly rather than passing vacuously.
const EXPECTED_TEST_NAMES: &[&str] = &[
    "insertVersion_lib_happy_path",
    "insertVersion_serve_happy_path",
    "insertVersion_duplicate_rejected",
    "insertVersion_bad_namespace_rejected",
    "deprecateVersion_sets_flag",
    "deprecateVersion_unknown_rejected",
    "approveVersion_clears_flag",
];

#[tokio::test]
async fn versioned_registry_spec() {
    let test_object =
        crate::util::rholang::test_rho_loader::load_test_rho("VersionedRegistryTest.rho")
            .expect("Failed to load VersionedRegistryTest.rho");

    let compiled = CompiledRholangSource::new(
        test_object,
        HashMap::new(),
        "VersionedRegistryTest.rho".to_string(),
    )
    .expect("Failed to compile VersionedRegistryTest.rho");

    // VersionedRegistry.rho has to be re-deployed in the test runtime
    // because the test harness creates a fresh scope_id rather than
    // inheriting the genesis scope (see regver-known-issues.md #2).
    let extra_libs = vec![standard_deploys::versioned_registry("root-shard")];

    let test_result_collector = Arc::new(TestResultCollector::new());

    let result = get_results(
        &compiled,
        &extra_libs,
        GENESIS_TEST_TIMEOUT,
        GenesisBuilder::build_genesis_parameters_with_defaults(None, None),
        test_result_collector,
    )
    .await
    .expect("Failed to run VersionedRegistry probe");

    // Vacuous-pass guard: every named test must have at least one
    // recorded assertion.
    for &name in EXPECTED_TEST_NAMES {
        assert!(
            result.assertions.contains_key(name),
            "Test '{}' recorded no assertions. The probe never reached \
             the assert call — likely a hang on a `for` upstream of the \
             assert.",
            name
        );
    }

    // Surface every recorded assertion. Mirrors `RhoSpec::mk_test` but
    // without the bug of returning Ok when assertions is empty.
    let mut printer = PrettyPrinter::new();
    for (test_name, attempts) in &result.assertions {
        for (attempt, assertions) in attempts {
            for assertion in assertions {
                use casper::rust::helper::test_result_collector::RhoTestAssertion::*;
                match assertion {
                    RhoAssertEquals {
                        expected,
                        actual,
                        clue,
                        ..
                    } => {
                        assert_eq!(
                            printer.build_string_from_message(expected),
                            printer.build_string_from_message(actual),
                            "{} (test: {}, attempt: {})",
                            clue,
                            test_name,
                            attempt
                        );
                    }
                    RhoAssertNotEquals {
                        unexpected,
                        actual,
                        clue,
                        ..
                    } => {
                        assert_ne!(
                            printer.build_string_from_message(unexpected),
                            printer.build_string_from_message(actual),
                            "{} (test: {}, attempt: {})",
                            clue,
                            test_name,
                            attempt
                        );
                    }
                    RhoAssertTrue {
                        is_success, clue, ..
                    } => {
                        assert!(
                            *is_success,
                            "{} (test: {}, attempt: {})",
                            clue, test_name, attempt
                        );
                    }
                }
            }
        }
    }
}
