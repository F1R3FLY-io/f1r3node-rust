use std::collections::HashMap;

use casper::rust::util::construct_deploy;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;
use models::rust::par_map_type_mapper::ParMapTypeMapper;

use super::support::admit_everywhere;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// The validator-to-stake map a committee reply carries.
fn stakes_of(reply: &Par) -> HashMap<Vec<u8>, i64> {
    let [models::rhoapi::Expr {
        expr_instance: Some(ExprInstance::EMapBody(map)),
    }] = reply.exprs.as_slice()
    else {
        panic!("the committee is a map: {reply:?}");
    };
    ParMapTypeMapper::emap_to_par_map(map.clone())
        .ps
        .map_iter(
            |(key, value)| match (key.exprs.as_slice(), value.exprs.as_slice()) {
                (
                    [models::rhoapi::Expr {
                        expr_instance: Some(ExprInstance::GByteArray(validator)),
                    }],
                    [models::rhoapi::Expr {
                        expr_instance: Some(ExprInstance::GInt(stake)),
                    }],
                ) => (validator.clone(), *stake),
                other => panic!("a committee entry is a validator and its stake: {other:?}"),
            },
        )
        .collect()
}

/// A deploy reads its block's committee, and another node replaying the
/// block reads the same one.
#[tokio::test]
async fn a_deploy_reads_its_blocks_committee_at_play_and_replay() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();
    let deployer = genesis.genesis_vaults[0].0.clone();
    let mut nodes = TestNode::create_network(genesis, 2, Some(0.0), None, None)
        .await
        .expect("network");

    let probe = construct_deploy::source_deploy_now(
        r#"
          new committee(`rho:block:committee`), ch in {
            committee!(*ch) |
            for (@seen <- ch) { @"block-committee-probe"!(seen) }
          }
        "#
        .to_string(),
        Some(deployer),
        None,
        Some(shard_id),
    )
    .expect("deploy");
    let block = match nodes[0]
        .create_block(&[probe])
        .await
        .expect("block creation")
    {
        casper::rust::blocks::proposer::propose_result::BlockCreatorResult::Created(block, ..) => {
            block
        }
        other => panic!("expected a created block, got {other:?}"),
    };
    admit_everywhere(&mut nodes, &block).await;

    let seen = nodes[1]
        .runtime_manager
        .get_data(
            block.body.state.post_state_hash.clone(),
            &models::rust::utils::new_gstring_par(
                "block-committee-probe".to_string(),
                Vec::new(),
                false,
            ),
        )
        .await
        .expect("read");
    let [seen] = seen.as_slice() else {
        panic!("the probe recorded one committee: {seen:?}");
    };
    let expected: HashMap<Vec<u8>, i64> = block
        .body
        .state
        .bonds
        .iter()
        .filter(|bond| bond.stake > 0)
        .map(|bond| (bond.validator.to_vec(), bond.stake))
        .collect();
    assert!(!expected.is_empty());
    assert_eq!(stakes_of(seen), expected, "the block's own bonds");
}
