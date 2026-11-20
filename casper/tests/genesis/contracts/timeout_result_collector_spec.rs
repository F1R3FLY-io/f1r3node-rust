// See casper/src/test/scala/coop/rchain/casper/genesis/contracts/TimeoutResultCollectorSpec.scala

use std::collections::HashMap;
use std::sync::Arc;

use casper::rust::helper::test_result_collector::TestResultCollector;
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::get_results;
use crate::util::genesis_builder::GenesisBuilder;

#[tokio::test]
async fn test_finished_should_be_false_if_suite_never_reports_completion() {
    let test_object =
        crate::util::rholang::test_rho_loader::load_test_rho("TimeoutResultCollectorTest.rho")
            .expect("Failed to load TimeoutResultCollectorTest.rho");

    let compiled = CompiledRholangSource::new(
        test_object,
        HashMap::new(),
        "TimeoutResultCollectorTest.rho".to_string(),
    )
    .expect("Failed to compile TimeoutResultCollectorTest.rho");

    let test_result_collector = Arc::new(TestResultCollector::new());
    let genesis_parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);

    // GENESIS_TEST_TIMEOUT (60s) covers the WHOLE pipeline —
    // genesis-build + store-open + runtime-create + genesis-reset
    // + rhospec-install + eval-test-source — not just the Rholang
    // test eval.  The prior 10s budget was tuned when genesis-build
    // was fast; slice 5.36 added fs_generator to blessed terms (an
    // ~800-line FsGenesis Rholang source), pushing CI genesis-build
    // past 10s before test eval starts.  Semantics unchanged: this
    // spec loads a `Nil` Rholang program and checks that
    // `has_finished == false` because no `testCompleted!(true)`
    // signal is sent — the timeout window just needs to cover
    // pipeline setup so the signal-not-sent condition is observable.
    let result = get_results(
        &compiled,
        &[],
        GENESIS_TEST_TIMEOUT,
        genesis_parameters,
        test_result_collector,
    )
    .await
    .expect("Failed to get results");

    assert!(
        !result.has_finished,
        "testFinished should be false if suite completion was not reported"
    );
}
