// See casper/src/test/scala/coop/rchain/casper/batch1/MultiParentCasperRholangSpec.scala

use casper::rust::util::rholang::tools::Tools;
use casper::rust::util::{construct_deploy, proto_util, rspace_util};
use crypto::rust::signatures::signed::Cosigned;
use models::rust::casper::protocol::casper_message::DeployData;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

//put a new casper instance at the start of each
//test since we cannot reset it
#[tokio::test]
async fn multi_parent_casper_should_create_blocks_based_on_deploys() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let mut standalone_node = TestNode::standalone(genesis).await.unwrap();

    let deploy = construct_deploy::basic_deploy_data(
        0,
        None,
        Some(standalone_node.genesis.shard_id.clone()),
    )
    .unwrap();
    let block = standalone_node
        .create_block_unsafe(std::slice::from_ref(&deploy))
        .await
        .unwrap();
    assert_eq!(block.body.deploys.len(), 1);
    let processed = block.body.deploys[0]
        .to_cosigned()
        .expect("protocol-v6 deploy envelope");
    let parents = proto_util::parent_hashes(&block);

    assert_eq!(parents.len(), 1);
    assert_eq!(parents[0], standalone_node.genesis.block_hash);
    assert_eq!(processed.data(), &deploy.data);
    assert!(processed.is_envelope_bound());

    let data =
        rspace_util::get_data_at_public_channel_block(&block, 0, &standalone_node.runtime_manager)
            .await;
    assert_eq!(data, vec!["0"]);
}

#[tokio::test]
async fn multi_parent_casper_should_be_able_to_use_the_registry() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("Failed to build genesis");

    let mut standalone_node = TestNode::standalone(genesis.clone()).await.unwrap();

    let register_source = r#"
new uriCh, rr(`rho:registry:insertArbitrary`), hello in {
  contract hello(@name, return) = {
    return!("Hello, ${name}!" %% {"name" : name})
  } |
  rr!(bundle+{*hello}, *uriCh)
}
"#;

    fn call_source(registry_id: &str) -> String {
        format!(
            r#"
new out, rl(`rho:registry:lookup`), helloCh in {{
  rl!({}, *helloCh) |
  for(hello <- helloCh){{
    hello!("World", *out)
  }}
}}
"#,
            registry_id
        )
    }

    fn calculate_unforgeable_name(deploy: &Cosigned<DeployData>) -> String {
        let unforgeable_id = Tools::user_deploy_rng(deploy).next();
        let unforgeable_id_u8: Vec<u8> = unforgeable_id.iter().map(|&b| b as u8).collect();
        hex::encode(unforgeable_id_u8)
    }

    // The legacy limit remains a helper input; protocol 4 proves and settles
    // the registry operation's complete debit against authenticated custody.
    let register_deploy = construct_deploy::source_deploy_now_full(
        register_source.to_string(),
        Some(900_000),
        None,
        None,
        None,
        Some(genesis.genesis_block.shard_id.clone()),
    )
    .unwrap();

    let block0 = standalone_node
        .add_block_from_deploys(std::slice::from_ref(&register_deploy))
        .await
        .unwrap();
    let register_envelope = block0.body.deploys[0]
        .to_cosigned()
        .expect("register deploy envelope");

    let registry_id = rspace_util::get_data_at_private_channel(
        &block0,
        &calculate_unforgeable_name(&register_envelope),
        &standalone_node.runtime_manager,
    )
    .await;

    let call_deploy = construct_deploy::source_deploy_now_full(
        call_source(&registry_id[0]),
        Some(900_000),
        None,
        None,
        None,
        Some(genesis.genesis_block.shard_id.clone()),
    )
    .unwrap();

    let block1 = standalone_node
        .add_block_from_deploys(std::slice::from_ref(&call_deploy))
        .await
        .unwrap();
    let call_envelope = block1.body.deploys[0]
        .to_cosigned()
        .expect("call deploy envelope");

    let data = rspace_util::get_data_at_private_channel(
        &block1,
        &calculate_unforgeable_name(&call_envelope),
        &standalone_node.runtime_manager,
    )
    .await;

    assert_eq!(data, vec!["\"Hello, World!\""]);
}
