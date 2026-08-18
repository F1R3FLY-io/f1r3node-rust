use std::time::Duration;

pub mod rho_spec_probe;
pub mod test_util;

pub mod auth_key_spec;
pub mod blessed_contract_source_pins;
pub mod block_data_contract_spec;
pub mod capabilities_registry_spec;
pub mod deep_recursion_spec;
pub mod either_spec;
pub mod exchange_spec;
pub mod failing_result_collector_spec;
pub mod genesis_contract_failure_mechanisms;
pub mod genesis_overflow_guard_shape;
pub mod list_ops_spec;
pub mod make_mint_dispatch_is_unambiguous;
pub mod make_mint_spec;
pub mod multi_sig_system_vault_spec;
pub mod non_negative_number_spec;
pub mod pos_spec;
pub mod registry_ops_spec;
pub mod registry_spec;
pub mod rho_spec_contract_spec;
pub mod rho_spec_floor_spec;
pub mod stack_spec;
pub mod standard_deploys_spec;
pub mod system_vault_spec;
pub mod timeout_result_collector_spec;
pub mod token_metadata_spec;
pub mod tree_hash_map_delete_restores_never_set;
pub mod tree_hash_map_spec;
pub mod vault_address_spec;
pub mod vault_issuance_test;

// See casper/src/test/scala/coop/rchain/casper/genesis/contracts/package.scala
pub const GENESIS_TEST_TIMEOUT: Duration = Duration::from_secs(60);
