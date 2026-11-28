use std::sync::Arc;

use casper::rust::api::block_api::BlockAPI;
use casper::rust::api::deploy_finalization_status::DeployFinalizationState;
use casper::rust::casper::MultiParentCasper;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::engine::engine_with_casper::EngineWithCasper;
use casper::rust::util::construct_deploy;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::signed::Signed;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GDeployId, GUnforgeable, Par};
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::utils::new_gint_par;

use crate::helper::test_node::{ShardOverrides, TestNode};
use crate::util::genesis_builder::GenesisBuilder;

const INSTALL: &str = r#"
new insertArbitrary(`rho:registry:insertArbitrary`), deployId(`rho:system:deployId`),
    counter, uriCh in {
  contract counter(@n, ret) = { ret!(n + 1) } |
  insertArbitrary!(bundle+{*counter}, *uriCh) |
  for (@uri <- uriCh) { deployId!(uri) }
}
"#;

fn signed_deploy(
    term: String,
    timestamp: i64,
    key: &PrivateKey,
    shard: &str,
) -> Signed<DeployData> {
    construct_deploy::source_deploy(
        term,
        timestamp,
        Some(1_000_000),
        None,
        Some(key.clone()),
        Some(0),
        Some(shard.to_owned()),
    )
    .expect("sign deploy")
}

fn call_term(uri: &str, input: i64) -> String {
    format!(
        r#"new lookup(`rho:registry:lookup`), deployId(`rho:system:deployId`), ch, ret in {{
  lookup!(`{uri}`, *ch) |
  for (c <- ch) {{ c!({input}, *ret) | for (@v <- ret) {{ deployId!(v) }} }}
}}"#
    )
}

async fn deploy_reply(engine: &EngineCell, sig: &[u8], hash: &BlockHash) -> Vec<Par> {
    let name = Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GDeployIdBody(GDeployId { sig: sig.to_vec() })),
    }]);
    let block_hash = hex::encode(hash);
    let (reply, block) = BlockAPI::get_data_at_par(engine, &name, block_hash.clone(), false)
        .await
        .unwrap_or_else(|error| {
            panic!(
                "read of deploy {} at block {block_hash} failed: {error:#}",
                hex::encode(sig)
            )
        });
    assert_eq!(
        block.block_hash, block_hash,
        "reply must come from the requested block"
    );
    reply
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn finalized_calls_keep_their_deploy_id_replies_after_parallel_registry_installs() {
    let params = GenesisBuilder::build_genesis_parameters_with_defaults(
        Some(|keys| {
            keys.into_iter()
                .take(3)
                .map(|key| (key, 10u64.into()))
                .collect()
        }),
        Some(4),
    );
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(params))
        .await
        .expect("build genesis");
    let shard = genesis.genesis_block.shard_id.clone();
    let [(left_key, _), (right_key, _), ..] = genesis.genesis_vaults.as_slice() else {
        panic!("the fixture needs two funded deployers");
    };
    let (left_key, right_key) = (left_key.clone(), right_key.clone());
    let mut nodes = TestNode::create_network_with_overrides(genesis, 3, ShardOverrides {
        finalization_rate: Some(0),
        with_read_only_size: Some(1),
        ..Default::default()
    })
    .await
    .expect("start three validators and a read-only node");
    let reader = &nodes[3];
    assert!(reader.casper.get_validator().is_none());
    let engine = EngineCell::init();
    engine
        .set(Arc::new(EngineWithCasper::new(reader.casper.clone())))
        .await;

    let install = signed_deploy(INSTALL.to_owned(), 1, &left_key, &shard);
    let setup = TestNode::propagate_block_at_index(&mut nodes, 2, std::slice::from_ref(&install))
        .await
        .expect("install and propagate the contract");
    let install_reply = deploy_reply(&engine, &install.sig, &setup.block_hash).await;
    let [uri] = install_reply.as_slice() else {
        panic!("the setup install must return one registry URI: {install_reply:?}");
    };
    let Some(ExprInstance::GUri(uri)) = uri
        .exprs
        .first()
        .and_then(|expr| expr.expr_instance.as_ref())
    else {
        panic!("the setup install must return a registry URI: {uri:?}");
    };
    assert!(uri.starts_with("rho:id:"));

    let mut calls = Vec::new();
    let fork_deploys = [(&left_key, [10, 11]), (&right_key, [20, 21])].map(|(key, inputs)| {
        let mut deploys = vec![signed_deploy(INSTALL.to_owned(), inputs[0], key, &shard)];
        for input in inputs {
            let call = signed_deploy(call_term(uri, input), input + 100, key, &shard);
            calls.push((call.sig.clone(), input + 1));
            deploys.push(call);
        }
        deploys
    });
    let [left, right] = nodes.get_disjoint_mut([0, 1]).expect("distinct validators");
    let (left_block, right_block) = tokio::join!(
        left.add_block_from_deploys(&fork_deploys[0]),
        right.add_block_from_deploys(&fork_deploys[1]),
    );
    let forks = [
        left_block.expect("first parallel block"),
        right_block.expect("second parallel block"),
    ];
    assert_ne!(forks[0].sender, forks[1].sender);
    for (index, fork) in forks.iter().enumerate() {
        assert_eq!(
            fork.header.parents_hash_list,
            std::slice::from_ref(&setup.block_hash)
        );
        for deploy in &fork_deploys[index] {
            assert!(
                fork.body
                    .deploys
                    .iter()
                    .any(|entry| entry.deploy.sig == deploy.sig && !entry.is_failed),
                "parallel block must execute deploy {}",
                hex::encode(&deploy.sig)
            );
        }
        for node in nodes
            .iter_mut()
            .filter(|node| !node.contains(&fork.block_hash))
        {
            node.process_block(fork.clone())
                .await
                .expect("replay parallel block");
        }
    }

    nodes[2].allow_empty_blocks = true;
    let merge = TestNode::propagate_block_at_index(&mut nodes, 2, &[])
        .await
        .expect("merge the parallel blocks");
    for fork in &forks {
        assert!(
            merge.header.parents_hash_list.contains(&fork.block_hash),
            "both parallel blocks must be parents of the merge"
        );
    }

    for node in &mut nodes[..3] {
        node.allow_empty_blocks = true;
    }
    for round in 0..12 {
        TestNode::propagate_block_at_index(&mut nodes, round % 3, &[])
            .await
            .expect("advance past the merge");
    }
    let reader = &nodes[3];
    for _ in 0..12 {
        let finalized = reader
            .casper
            .finalize()
            .await
            .expect("reader finalization cycle");
        let dag = reader.casper.block_dag().await.expect("reader DAG");
        if finalized.block_hash == merge.block_hash
            || dag
                .is_dag_ancestor(&merge.block_hash, &finalized.block_hash)
                .expect("merge ancestry")
        {
            break;
        }
    }
    let dag = reader.casper.block_dag().await.expect("reader DAG");
    let finalized = dag.last_finalized_block();
    assert!(
        finalized == merge.block_hash
            || dag
                .is_dag_ancestor(&merge.block_hash, &finalized)
                .expect("finalized ancestry"),
        "the read-only node must finalize the merge"
    );

    for (sig, expected) in calls {
        let status = BlockAPI::deploy_finalization_status(&engine, &sig)
            .await
            .expect("call status");
        assert_eq!(
            status.state,
            DeployFinalizationState::Finalized,
            "call {} must finalize: {status:?}",
            hex::encode(&sig)
        );
        let canonical = status
            .latest_block_hash
            .expect("finalized call must have a canonical block");
        let expected = vec![new_gint_par(expected, vec![], false)];
        assert_eq!(
            deploy_reply(&engine, &sig, &canonical).await,
            expected,
            "call {} reply at canonical block {}",
            hex::encode(&sig),
            hex::encode(canonical)
        );
        assert_eq!(
            deploy_reply(&engine, &sig, &merge.block_hash).await,
            expected,
            "call {} reply at merge block {}",
            hex::encode(&sig),
            hex::encode(&merge.block_hash)
        );
    }
}
