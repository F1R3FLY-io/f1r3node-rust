use casper::rust::genesis::genesis::Genesis;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::v6::genesis_policy::read_genesis_policy;
use casper::rust::v6::resource_policy::{ShardResourcePolicy, PROTOCOL_V6};

use crate::util::genesis_builder::GenesisBuilder;
use crate::util::rholang::resources::mk_runtime_manager;

fn parameters(version: i64) -> Genesis {
    let (_, _, mut genesis) = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(1));
    genesis.version = version;
    genesis
}

async fn runtime_manager() -> RuntimeManager {
    mk_runtime_manager("v6-genesis", Some(Genesis::default_mergeable_tags_arc())).await
}

fn policy() -> ShardResourcePolicy {
    ShardResourcePolicy {
        block_records: 16,
        block_record_bytes: 1 << 20,
        max_envelopes: 32,
        demand_cap: 1_000_000,
    }
}

#[tokio::test]
async fn a_protocol_6_genesis_holds_its_resource_policy() {
    let rm = runtime_manager().await;
    let genesis = parameters(PROTOCOL_V6);
    let block = Genesis::create_genesis_block_with_policy(&rm, &genesis, Some(&policy()))
        .await
        .expect("a protocol 6 genesis builds");
    assert_eq!(block.header.version, PROTOCOL_V6);
    assert!(block.body.deploys.iter().all(|d| !d.is_failed));
    let read = read_genesis_policy(&rm, &block.body.state.post_state_hash)
        .await
        .expect("the policy reads back");
    assert_eq!(read, Some(policy()));
}

#[tokio::test]
async fn a_legacy_genesis_holds_no_resource_policy() {
    let rm = runtime_manager().await;
    let genesis = parameters(1);
    let block = Genesis::create_genesis_block(&rm, &genesis)
        .await
        .expect("a legacy genesis builds");
    let read = read_genesis_policy(&rm, &block.body.state.post_state_hash)
        .await
        .expect("the query runs");
    assert_eq!(read, None);
}

#[tokio::test]
async fn the_policy_and_the_protocol_version_must_agree() {
    let rm = runtime_manager().await;
    assert!(
        Genesis::create_genesis_block_with_policy(&rm, &parameters(1), Some(&policy()))
            .await
            .is_err()
    );
    assert!(
        Genesis::create_genesis_block_with_policy(&rm, &parameters(PROTOCOL_V6), None)
            .await
            .is_err()
    );
    let zero = ShardResourcePolicy {
        max_envelopes: 0,
        ..policy()
    };
    assert!(
        Genesis::create_genesis_block_with_policy(&rm, &parameters(PROTOCOL_V6), Some(&zero))
            .await
            .is_err()
    );
}
