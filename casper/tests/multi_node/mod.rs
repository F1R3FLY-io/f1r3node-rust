// Multi-node tests reproducing integration-suite failure modes at unit level.
//
// These tests use `TestNode::create_network` to simulate the multi-validator
// topology of the integration test suite. They run deterministically and
// without docker rebuild cycles, so we can exercise concurrency-sensitive
// merge behavior at the unit-test layer.

mod bridge_contract_concurrent_merge;
mod integer_channel_resend_merge;
mod registry_key_merge;
mod foreign_genesis_rejection;
mod store_items_flood_resilience;
mod v6_order_then_execute;
mod v6_engine_state;
mod v6_inclusion_checks;
mod v6_lag_gate;
