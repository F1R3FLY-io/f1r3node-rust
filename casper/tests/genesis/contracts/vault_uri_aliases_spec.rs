use casper::rust::util::construct_deploy::DEFAULT_PUB;
use models::rust::normalizer_env::with_deployer_id;
use rholang::rust::build::compile_rholang_source::CompiledRholangSource;

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::RhoSpec;

#[tokio::test]
async fn vault_uri_aliases_spec() {
    let test_object =
        crate::util::rholang::test_rho_loader::load_test_rho("VaultUriAliasesTest.rho")
            .expect("Failed to load VaultUriAliasesTest.rho");

    let compiled = CompiledRholangSource::new(
        test_object,
        with_deployer_id(&DEFAULT_PUB),
        "VaultUriAliasesTest.rho".to_string(),
    )
    .expect("Failed to compile VaultUriAliasesTest.rho");

    RhoSpec::new(compiled, vec![], GENESIS_TEST_TIMEOUT)
        .run_tests()
        .await
        .expect("VaultUriAliasesSpec tests failed");
}
