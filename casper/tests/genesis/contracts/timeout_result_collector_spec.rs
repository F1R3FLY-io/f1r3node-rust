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
