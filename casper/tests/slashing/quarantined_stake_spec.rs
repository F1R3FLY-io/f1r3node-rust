use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use casper::rust::casper::MultiParentCasper;
use casper::rust::util::construct_deploy;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, ProcessedSystemDeploy, SystemDeployData,
};
use models::rust::validator::Validator;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::block_util::resign_block;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn identity(node: &TestNode) -> Validator {
    prost::bytes::Bytes::copy_from_slice(
        &node
            .casper
            .get_validator()
            .expect("validator")
            .public_key
            .bytes,
    )
}

async fn admit_everywhere(nodes: &mut [TestNode], block: &BlockMessage) {
    for node in nodes.iter_mut() {
        let verdict = node.add_block(block.clone()).await.expect("process");
        assert!(
            matches!(verdict, Either::Right(ValidBlock::Valid)),
            "block {} refused: {verdict:?}",
            block.body.state.block_number
        );
    }
}

/// The stake `validator` holds in quarantine after `block`, if it is still
/// withdrawing.
async fn quarantined(node: &TestNode, block: &BlockMessage, validator: &Validator) -> Option<i64> {
    node.runtime_manager
        .quarantined_stakes(&block.body.state.post_state_hash)
        .await
        .expect("withdrawers")
        .into_iter()
        .find(|bond| bond.validator == *validator)
        .map(|bond| bond.stake)
}

/// A validator that equivocates and then withdraws is still slashed while its
/// stake sits in quarantine: the quarantine keeps that stake answerable until
/// payout. Every node accepts the slash, and the emptied withdrawal is closed
/// when its quarantine ends.
#[tokio::test]
async fn an_equivocator_is_slashed_while_its_withdrawal_is_in_quarantine() {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    parameters.2.proof_of_stake.epoch_length = 10;
    parameters.2.proof_of_stake.quarantine_length = 40;
    let validator_vaults: Vec<VaultAddress> = parameters
        .2
        .proof_of_stake
        .validators
        .iter()
        .map(|validator| VaultAddress::from_public_key(&validator.pk).expect("vault address"))
        .collect();
    for vault in parameters.2.vaults.iter_mut() {
        if validator_vaults.contains(&vault.vault_address) {
            vault.initial_balance = 1_000_000;
        }
    }
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();
    let mut nodes = TestNode::create_network(genesis, 4, Some(0.0), None, None, None)
        .await
        .expect("network");
    for node in nodes.iter_mut() {
        node.allow_empty_blocks = true;
    }

    let leaving = 0;
    let leaving_key = identity(&nodes[leaving]);
    let private_key = nodes[leaving]
        .validator_id_opt
        .as_ref()
        .expect("validator")
        .private_key
        .clone();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis() as i64;
    let withdraw = construct_deploy::source_deploy(
        r#"
          new rl(`rho:registry:lookup`), poSCh, deployerId(`rho:system:deployerId`), ret in {
            rl!(`rho:system:pos`, *poSCh) |
            for (@(_, PoS) <- poSCh) {
              @PoS!("withdraw", *deployerId, *ret)
            }
          }
        "#
        .to_string(),
        now,
        Some(500_000),
        None,
        Some(private_key.clone()),
        None,
        Some(shard_id),
    )
    .expect("deploy");
    let withdrawing = nodes[leaving]
        .create_block_unsafe(&[withdraw])
        .await
        .expect("block creation");
    let mut twin = withdrawing.clone();
    twin.header.timestamp += 1;
    let twin = resign_block(&twin, &private_key);
    assert_ne!(twin.block_hash, withdrawing.block_hash);
    admit_everywhere(&mut nodes, &withdrawing).await;

    let size = nodes.len();
    let mut last = withdrawing.clone();
    for round in 0..200 {
        if !last
            .body
            .state
            .bonds
            .iter()
            .any(|bond| bond.validator == leaving_key && bond.stake > 0)
        {
            break;
        }
        assert!(round < 199, "the withdrawal never took effect");
        last = nodes[1 + round % (size - 1)]
            .create_block_unsafe(&[])
            .await
            .expect("block creation");
        admit_everywhere(&mut nodes, &last).await;
    }

    let verdict = nodes[1].add_block(twin).await.expect("process");
    assert!(
        matches!(
            verdict,
            Either::Left(BlockError::Invalid(
                InvalidBlock::IgnorableEquivocation | InvalidBlock::AdmissibleEquivocation
            ))
        ),
        "the twin is an equivocation: {verdict:?}"
    );
    let next = nodes[1]
        .create_block_unsafe(&[])
        .await
        .expect("block creation");
    let slashed: Vec<Validator> = next
        .body
        .system_deploys
        .iter()
        .filter_map(|deploy| match deploy {
            ProcessedSystemDeploy::Succeeded {
                system_deploy: SystemDeployData::Slash { evidence, .. },
                ..
            } => Some(evidence.first.sender.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        slashed,
        vec![leaving_key.clone()],
        "the quarantined equivocator is slashed"
    );
    admit_everywhere(&mut nodes, &next).await;
    assert_eq!(
        quarantined(&nodes[1], &next, &leaving_key).await,
        Some(0),
        "the slash takes the quarantined stake"
    );

    let mut last = next;
    for round in 0..120 {
        if quarantined(&nodes[1], &last, &leaving_key).await.is_none() {
            return;
        }
        last = nodes[1 + round % (size - 1)]
            .create_block_unsafe(&[])
            .await
            .expect("block creation");
        admit_everywhere(&mut nodes, &last).await;
    }
    panic!("the emptied withdrawal is never closed");
}
