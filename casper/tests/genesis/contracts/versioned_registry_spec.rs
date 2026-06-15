// Spec for the Versioned Registry FIP rollout — Step 2 placeholder.
// Mirrors registry_spec.rs. The test deploys VersionedRegistryTest.rho
// against a runtime whose genesis sequence includes VersionedRegistry.rho;
// a clean run proves both the embedding and the genesis deploy work.
// Real behavioral assertions land at Step 3.

use std::collections::HashMap;

use rholang::rust::build::compile_rholang_source::CompiledRholangSource;

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::RhoSpec;

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

    let spec = RhoSpec::new(compiled, vec![], GENESIS_TEST_TIMEOUT);

    spec.run_tests()
        .await
        .expect("VersionedRegistrySpec tests failed");
}
