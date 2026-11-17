use std::collections::HashMap;

use rholang::rust::build::compile_rholang_source::CompiledRholangSource;

use crate::genesis::contracts::GENESIS_TEST_TIMEOUT;
use crate::helper::rho_spec::RhoSpec;
use crate::util::genesis_builder::GenesisBuilder;

const ADMIN_PUBLIC_KEYS: [&str; 3] = [
    "04ffc016579a68050d655d55df4e09f04605164543e257c8e6df10361e6068a5336588e9b355ea859c5ab4285a5ef0efdf62bc28b80320ce99e26bb1607b3ad93d",
    "04fa70d7be5eb750e0915c0f6d19e7085d18bb1c22d030feb2a877ca2cd226d04438aa819359c56c720142fbc66e9da03a5ab960a3d8b75363a226b7c800f60420",
    "04837a4cff833e3157e3135d7b40b8e1f33c6e6b5a4342b9fc784230ca4c4f9d356f258debef56ad4984726d6ab3e7709e1632ef079b4bcd653db00b68b2df065f",
];
const ADMIN_QUORUM: u32 = 2;

#[tokio::test]
async fn identity_registry_spec() {
    let test_object =
        crate::util::rholang::test_rho_loader::load_test_rho("IdentityRegistryTest.rho")
            .expect("Failed to load IdentityRegistryTest.rho");

    let compiled = CompiledRholangSource::new(
        test_object,
        HashMap::new(),
        "IdentityRegistryTest.rho".to_string(),
    )
    .expect("Failed to compile IdentityRegistryTest.rho");

    let mut params = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    params.2.identity_registry_admin_public_keys = ADMIN_PUBLIC_KEYS.map(String::from).to_vec();
    params.2.identity_registry_admin_quorum = ADMIN_QUORUM;

    let spec = RhoSpec::new_with_genesis_parameters(compiled, vec![], GENESIS_TEST_TIMEOUT, params);

    spec.run_tests()
        .await
        .expect("IdentityRegistrySpec tests failed");
}
