// See casper/src/test/scala/coop/rchain/casper/genesis/contracts/ListOpsSpec.
// scala

use std::collections::HashMap;

use rholang::rust::build::compile_rholang_source::CompiledRholangSource;

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::RhoSpec;

#[tokio::test]
async fn list_ops_spec() {
    let test_object = CompiledRholangSource::load_source("ListOpsTest.rho")
        .expect("Failed to load ListOpsTest.rho");

    let compiled =
        CompiledRholangSource::new(test_object, HashMap::new(), "ListOpsTest.rho".to_string())
            .expect("Failed to compile ListOpsTest.rho");

    let spec = RhoSpec::new(compiled, vec![], GENESIS_TEST_TIMEOUT);

    spec.run_tests().await.expect("ListOpsSpec tests failed");
}
