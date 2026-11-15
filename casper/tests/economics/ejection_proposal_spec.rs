use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::blocks::proposer::block_creator;
use casper::rust::blocks::proposer::propose_result::BlockCreatorResult;
use casper::rust::blocks::proposer::proposer::DeploySelection;
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::economics::bucket_view::LeakyBucketProposal;
use models::rust::casper::protocol::casper_message::{BlockMessage, ClaimFamily};
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::block_util::resign_block;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// A block from `node` carrying its requests to eject `targets`.
async fn create_offering(node: &mut TestNode, targets: &[Validator]) -> BlockMessage {
    super::support::request_ejections(node, targets).await;
    let snapshot = node.casper.get_snapshot().await.expect("snapshot");
    let validator = node.casper.get_validator().expect("validator");
    match block_creator::create(
        &snapshot,
        &validator,
        None,
        node.deploy_storage.clone(),
        node.rejected_deploy_buffer.clone(),
        &node.runtime_manager,
        &mut node.block_store,
        &node.block_dag_storage,
        DeploySelection::StandardAllowEmpty,
        LeakyBucketProposal::abstaining(*node.casper.bucket_view().params()).fixed(),
    )
    .await
    .expect("block creation")
    {
        BlockCreatorResult::Created(block, ..) => block,
        other => panic!("expected a created block, got {other:?}"),
    }
}

/// The targets of the ejection claims `block` opened, in identity order.
async fn ejections(node: &TestNode, block: &BlockMessage) -> Vec<Validator> {
    let mut opened: Vec<Validator> = node
        .runtime_manager
        .floor_state(&block.body.state.post_state_hash)
        .await
        .expect("post-state read")
        .claims
        .iter()
        .filter(|record| {
            record.claim.family == ClaimFamily::Eject
                && record.claim.opened_at == block.body.state.block_number
        })
        .map(|record| record.claim.target.clone())
        .collect();
    opened.sort();
    opened
}

async fn admits(node: &mut TestNode, block: &BlockMessage) -> bool {
    let verdict = node.add_block(block.clone()).await.expect("process");
    matches!(verdict, Either::Right(_))
}

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

/// The ejection requests waiting in `node`'s pool, by target.
fn pooled_requests(node: &TestNode) -> Vec<String> {
    let mut terms: Vec<String> = node
        .deploy_storage
        .read_all()
        .expect("pool")
        .into_iter()
        .map(|deploy| deploy.data.term)
        .filter(|term| term.contains("\"ejectValidator\""))
        .collect();
    terms.sort();
    terms
}

/// A node asks once for each target while its request waits, and withdraws
/// the request once another validator's claim against the target is open.
#[tokio::test]
async fn a_waiting_request_is_not_repeated_and_is_withdrawn_once_a_claim_opens() {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    super::support::fund_validators(&mut parameters.2);
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let mut nodes = TestNode::create_network(genesis, 3, Some(0.0), None, None)
        .await
        .expect("network");
    let target = identity(&nodes[2]);

    super::support::request_ejections(&nodes[0], std::slice::from_ref(&target)).await;
    super::support::request_ejections(&nodes[0], std::slice::from_ref(&target)).await;
    assert_eq!(pooled_requests(&nodes[0]).len(), 1, "asked once");

    let opening = create_offering(&mut nodes[1], std::slice::from_ref(&target)).await;
    assert_eq!(ejections(&nodes[1], &opening).await, vec![target.clone()]);
    for node in nodes.iter_mut() {
        assert!(admits(node, &opening).await);
    }
    let voting = create_offering(&mut nodes[2], &[]).await;
    for node in nodes.iter_mut() {
        assert!(admits(node, &voting).await);
    }
    assert!(nodes[0].casper.bucket_view().ejection_claim_open(&target));

    super::support::request_ejections(&nodes[0], std::slice::from_ref(&target)).await;
    assert!(
        pooled_requests(&nodes[0]).is_empty(),
        "the waiting request is withdrawn"
    );
}

/// A request waiting in the pool survives a restart, and the restarted node
/// does not ask for the same ejection again.
#[tokio::test]
async fn a_restarted_node_does_not_repeat_a_waiting_request() {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    super::support::fund_validators(&mut parameters.2);
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let nodes = TestNode::create_network(genesis, 3, Some(0.0), None, None)
        .await
        .expect("network");
    let target = identity(&nodes[2]);
    super::support::request_ejections(&nodes[0], std::slice::from_ref(&target)).await;
    assert_eq!(pooled_requests(&nodes[0]).len(), 1);

    let restarted = super::support::restart(&nodes[0]).await;
    let snapshot = restarted.get_snapshot().await.expect("snapshot");
    restarted
        .request_ejections_of(&snapshot, std::slice::from_ref(&target))
        .await
        .expect("requests");
    let pooled = restarted
        .deploy_storage
        .read_all()
        .expect("pool")
        .into_iter()
        .filter(|deploy| deploy.data.term.contains("\"ejectValidator\""))
        .count();
    assert_eq!(pooled, 1, "asked once across the restart");
}

/// A view can be stale — restarted, or unaware a merge kept another claim —
/// so a request the contract refuses opens nothing, and the requester takes
/// its targets in turn, so a refused one does not hold back an acceptable one.
#[tokio::test]
async fn refused_ejection_requests_open_nothing() {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    super::support::fund_validators(&mut parameters.2);
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let mut nodes = TestNode::create_network(genesis, 3, Some(0.0), None, None)
        .await
        .expect("network");

    let stranger: Validator = prost::bytes::Bytes::from(vec![9u8; 65]);
    let first = create_offering(&mut nodes[0], std::slice::from_ref(&stranger)).await;
    assert!(
        ejections(&nodes[0], &first).await.is_empty(),
        "an unseated target is refused"
    );
    for node in nodes.iter_mut() {
        assert!(admits(node, &first).await);
    }

    let target = identity(&nodes[2]);
    let opening = create_offering(&mut nodes[0], std::slice::from_ref(&target)).await;
    assert_eq!(ejections(&nodes[0], &opening).await, vec![target.clone()]);
    for node in nodes.iter_mut() {
        assert!(admits(node, &opening).await);
    }

    // An unseated target, one already under proceedings, an acceptable one
    // and the requester itself: one request a block, in turn, and only the
    // acceptable one opens.
    let other = identity(&nodes[0]);
    let own = identity(&nodes[1]);
    let offered = [stranger, target.clone(), other.clone(), own];
    let mut opened = Vec::new();
    for _ in 0..3 {
        let block = create_offering(&mut nodes[1], &offered).await;
        opened.extend(ejections(&nodes[1], &block).await);
        for node in nodes.iter_mut() {
            assert!(admits(node, &block).await, "replay agrees with creation");
        }
    }
    assert_eq!(opened, vec![other]);
}

/// Sibling blocks that each open proceedings both write the PoS state, so a
/// block merging them keeps one claim while both blocks stay accepted. The
/// view corrects itself from the floor's state; see `bucket_view`'s
/// reconciliation tests.
#[tokio::test]
async fn merging_sibling_ejections_keeps_one_claim() {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    super::support::fund_validators(&mut parameters.2);
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let validators: Vec<Validator> = genesis
        .validator_pks()
        .iter()
        .map(|pk| prost::bytes::Bytes::copy_from_slice(&pk.bytes))
        .collect();
    let mut nodes = TestNode::create_network(genesis, 3, Some(0.0), None, None)
        .await
        .expect("network");

    let proposers = [identity(&nodes[0]), identity(&nodes[1])];
    let targets: Vec<Validator> = validators
        .iter()
        .filter(|v| !proposers.contains(v))
        .cloned()
        .collect();
    assert!(targets.len() >= 2, "two seated non-proposers to target");

    let left = create_offering(&mut nodes[0], &targets[..1]).await;
    let right = create_offering(&mut nodes[1], &targets[1..2]).await;
    assert_eq!(ejections(&nodes[0], &left).await, targets[..1].to_vec());
    assert_eq!(ejections(&nodes[1], &right).await, targets[1..2].to_vec());

    for block in [&left, &right] {
        assert!(admits(&mut nodes[2], block).await);
    }
    let merge = create_offering(&mut nodes[2], &[]).await;
    assert_eq!(
        merge.header.parents_hash_list.len(),
        2,
        "the merge cites both siblings"
    );
    assert!(admits(&mut nodes[2], &merge).await);

    let state = &merge.body.state.post_state_hash;
    let runtime_manager = &nodes[2].runtime_manager;
    let floor = runtime_manager
        .floor_state(state)
        .await
        .expect("floor state");
    assert_eq!(
        floor.claims.len(),
        1,
        "one of the two claims survives the merge"
    );
    let claim = &floor.claims[0].claim;
    assert!(targets[..2].contains(&claim.target));
    assert_eq!(claim.opened_at, left.body.state.block_number);

    // The combined read answers what the separate reads answer.
    assert_eq!(
        floor.bonds,
        runtime_manager.compute_bonds(state).await.expect("bonds")
    );
    assert_eq!(
        floor.active_validators,
        runtime_manager
            .get_active_validators(state)
            .await
            .expect("active")
    );
    assert!(floor.withdrawers.is_empty(), "nobody is unbonding");
}

/// The bonds field is a replay input, since an ejection's seat check and
/// electorate are the block's own committee, so it is judged before replay:
/// a forged committee is reported as such rather than as a replay mismatch.
#[tokio::test]
async fn a_forged_committee_is_rejected_before_replay() {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    super::support::fund_validators(&mut parameters.2);
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let mut nodes = TestNode::create_network(genesis, 3, Some(0.0), None, None)
        .await
        .expect("network");

    let target = identity(&nodes[2]);
    let opening = create_offering(&mut nodes[0], std::slice::from_ref(&target)).await;
    assert_eq!(ejections(&nodes[0], &opening).await, vec![target.clone()]);

    let mut forged = opening.clone();
    forged
        .body
        .state
        .bonds
        .retain(|bond| bond.validator != target);
    let key = &nodes[0]
        .validator_id_opt
        .as_ref()
        .expect("validator")
        .private_key;
    let forged = resign_block(&forged, key);

    let verdict = nodes[1].add_block(forged).await.expect("process");
    assert!(
        matches!(
            verdict,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidBondsCache))
        ),
        "unexpected verdict: {verdict:?}"
    );
}

/// A committee that lists a validator twice is not the floor's committee,
/// however its entries compare as a set: the reward counts members and
/// stake per entry.
#[tokio::test]
async fn a_committee_listing_a_validator_twice_is_rejected() {
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(None)
        .await
        .expect("genesis");
    let mut nodes = TestNode::create_network(genesis, 2, Some(0.0), None, None)
        .await
        .expect("network");
    let block = create_offering(&mut nodes[0], &[]).await;
    let mut forged = block.clone();
    let repeated = forged.body.state.bonds[0].clone();
    forged.body.state.bonds.push(repeated);
    let key = &nodes[0]
        .validator_id_opt
        .as_ref()
        .expect("validator")
        .private_key;
    let forged = resign_block(&forged, key);

    let verdict = nodes[1].add_block(forged).await.expect("process");
    assert!(
        matches!(
            verdict,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidBondsCache))
        ),
        "unexpected verdict: {verdict:?}"
    );
}

/// What a block's floor records, its pre-state still holds: merges reject
/// only chains above the floor, so a claim recorded at the floor survives
/// into every later block until something deliberately removes it.
#[tokio::test]
async fn every_claim_at_a_blocks_floor_is_in_its_pre_state() {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    super::support::fund_validators(&mut parameters.2);
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let validators: Vec<Validator> = genesis
        .validator_pks()
        .iter()
        .map(|pk| prost::bytes::Bytes::copy_from_slice(&pk.bytes))
        .collect();
    let mut nodes = TestNode::create_network(genesis, 3, Some(0.0), None, None)
        .await
        .expect("network");

    // Sibling blocks that each open a claim, so their PoS writes conflict.
    let mut blocks = Vec::new();
    for (index, node) in nodes.iter_mut().enumerate() {
        let own = identity(node);
        let target = validators
            .iter()
            .filter(|v| **v != own)
            .nth(index)
            .expect("a target")
            .clone();
        blocks.push(create_offering(node, &[target]).await);
    }
    for block in &blocks {
        for node in nodes.iter_mut() {
            node.add_block(block.clone()).await.expect("process");
        }
    }
    for round in 0..12 {
        let index = round % nodes.len();
        let block = create_offering(&mut nodes[index], &[]).await;
        for node in nodes.iter_mut() {
            assert!(admits(node, &block).await);
        }
        blocks.push(block);
    }

    let ftt = casper::rust::safety::clique_oracle::FtThreshold::from_ppm(
        nodes[0]
            .casper
            .get_snapshot()
            .await
            .expect("snapshot")
            .on_chain_state
            .chain
            .fault_tolerance_threshold_ppm,
    );
    let mut checked_nonempty = 0;
    for block in blocks
        .iter()
        .filter(|b| !b.header.parents_hash_list.is_empty())
    {
        let snapshot = nodes[0].casper.get_snapshot().await.expect("snapshot");
        let latest_messages = block
            .justifications
            .iter()
            .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
            .collect();
        let floor = casper::rust::finality::floor_context::FloorContext::derive(
            &snapshot.dag,
            &nodes[0].block_store,
            &block.header.parents_hash_list,
            &latest_messages,
            ftt,
        )
        .await
        .expect("floor");
        let runtime_manager = &nodes[0].runtime_manager;
        let at_floor = runtime_manager
            .floor_state(&floor.floor_state)
            .await
            .expect("floor read")
            .claims
            .clone();
        let at_pre_state = runtime_manager
            .floor_state(&block.body.state.pre_state_hash)
            .await
            .expect("pre-state read")
            .claims
            .clone();
        for record in &at_floor {
            assert!(
                at_pre_state.contains(record),
                "block {} lost {record:?} recorded at its floor {}",
                block.body.state.block_number,
                floor.floor.block_number
            );
        }
        if !at_floor.is_empty() {
            checked_nonempty += 1;
        }
    }
    assert!(
        checked_nonempty > 0,
        "no floor carried a claim, so the property was never exercised"
    );
}
