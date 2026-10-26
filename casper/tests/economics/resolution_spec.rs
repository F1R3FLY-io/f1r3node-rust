use std::collections::HashSet;

use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use casper::rust::casper::Casper;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, ClaimOutcome, ProcessedSystemDeploy, SystemDeployData,
};
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::history::Either;

use super::support::{admit_everywhere, floor_view, identity, judge, propose};
use crate::helper::block_util::resign_block;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn resolution_of(block: &BlockMessage) -> Option<&SystemDeployData> {
    block
        .body
        .system_deploys
        .iter()
        .find_map(|deploy| match deploy {
            ProcessedSystemDeploy::Succeeded {
                system_deploy: data @ SystemDeployData::ResolveClaims { .. },
                ..
            } => Some(data),
            _ => None,
        })
}

fn resigned(nodes: &[TestNode], block: &BlockMessage) -> BlockMessage {
    let sender = nodes
        .iter()
        .find(|node| identity(node) == block.sender)
        .expect("the sender runs a node");
    let key = &sender
        .validator_id_opt
        .as_ref()
        .expect("validator")
        .private_key;
    resign_block(block, key)
}

struct Decided {
    nodes: Vec<TestNode>,
    target: Validator,
    /// The first block that resolves the claim, not yet admitted anywhere.
    resolving: BlockMessage,
    /// The admitted block before it.
    unresolved: BlockMessage,
}

/// A network in which a claim against the lightest validator has just
/// become decidable: every member voted on it, yea from the rest of the
/// committee when `backed`, nay otherwise, and the target nay.
async fn decide_a_claim(backed: bool) -> Decided { decide_a_claim_at_depth(backed, None).await }

async fn decide_a_claim_at_depth(backed: bool, max_parent_depth: Option<i32>) -> Decided {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    super::support::fund_validators(&mut parameters.2);
    if let Some(depth) = max_parent_depth {
        parameters.2.proof_of_stake.max_parent_depth = depth;
    }
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let lightest = genesis
        .genesis_block
        .body
        .state
        .bonds
        .iter()
        .min_by_key(|bond| bond.stake)
        .expect("bonds")
        .validator
        .clone();
    let mut nodes = TestNode::create_network(genesis, 4, Some(0.0), None, None)
        .await
        .expect("network");
    let target_index = nodes
        .iter()
        .position(|node| identity(node) == lightest)
        .expect("the lightest validator runs a node");
    let target = identity(&nodes[target_index]);
    let accusers: Vec<usize> = (0..nodes.len()).filter(|i| *i != target_index).collect();
    let drained = if backed {
        vec![target.clone()]
    } else {
        Vec::new()
    };

    let opening = propose(&mut nodes[accusers[0]], std::slice::from_ref(&target), &[]).await;
    admit_everywhere(&mut nodes, &opening).await;
    for &index in &accusers {
        let voting = propose(&mut nodes[index], &[], &drained).await;
        admit_everywhere(&mut nodes, &voting).await;
    }
    let against = propose(&mut nodes[target_index], &[], &[]).await;
    admit_everywhere(&mut nodes, &against).await;

    let mut unresolved = against;
    for round in 0..40 {
        let index = round % nodes.len();
        let block = propose(&mut nodes[index], &[], &[]).await;
        if resolution_of(&block).is_some() {
            return Decided {
                nodes,
                target,
                resolving: block,
                unresolved,
            };
        }
        admit_everywhere(&mut nodes, &block).await;
        unresolved = block;
    }
    panic!("the claim resolves once the floor shows the votes");
}

/// Every member votes in its next block, so a claim the whole
/// committee proposes on in turn must be decidable whatever the parent depth:
/// the depth bounds how far apart a block's parents may sit, not how long a
/// member may go between blocks.
#[tokio::test]
async fn a_claim_voted_on_in_turn_is_decided_at_the_smallest_depth() {
    let decided = decide_a_claim_at_depth(true, Some(1)).await;
    let Some(SystemDeployData::ResolveClaims { resolutions, .. }) =
        resolution_of(&decided.resolving)
    else {
        unreachable!()
    };
    assert!(resolutions
        .iter()
        .any(|r| r.claim.target == decided.target && r.outcome == ClaimOutcome::Pass));
}

fn judge_index(nodes: &[TestNode], block: &BlockMessage) -> usize {
    nodes
        .iter()
        .position(|node| identity(node) != block.sender)
        .expect("a node other than the proposer")
}

/// An ejected validator is not expected to propose: its silence no
/// longer holds back the synchrony constraint.
#[tokio::test]
async fn an_ejected_validator_is_not_waited_for() {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    super::support::fund_validators(&mut parameters.2);
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let mut nodes = TestNode::create_network(genesis, 4, Some(0.0), None, None)
        .await
        .expect("network");
    for index in 0..4 {
        let block = propose(&mut nodes[index], &[], &[]).await;
        admit_everywhere(&mut nodes, &block).await;
    }
    for index in 0..3 {
        let block = propose(&mut nodes[index], &[], &[]).await;
        admit_everywhere(&mut nodes, &block).await;
    }
    let silent = identity(&nodes[3]);

    let mut snapshot = nodes[0].casper.get_snapshot().await.expect("snapshot");
    snapshot
        .on_chain_state
        .shard_conf
        .synchrony_constraint_threshold = 0.99;
    let validator = nodes[0].validator_id_opt.clone().expect("validator");
    let check = |ejected: HashSet<Validator>| {
        let snapshot = snapshot.clone();
        let validator = validator.clone();
        async move {
            casper::rust::synchrony_constraint_checker::check(&snapshot, &validator, &ejected)
                .await
                .expect("check")
        }
    };
    assert!(
        !matches!(
            check(HashSet::new()).await,
            casper::rust::blocks::proposer::propose_result::CheckProposeConstraintsResult::Success
        ),
        "a silent seated validator holds the constraint back"
    );
    let with_ejected = check(HashSet::from([silent])).await;
    assert!(
        matches!(
            with_ejected,
            casper::rust::blocks::proposer::propose_result::CheckProposeConstraintsResult::Success
        ),
        "{with_ejected:?}"
    );
}

/// A validator the rest of the committee finds drained is ejected once
/// their votes are visible at a floor: some block resolves the claim as
/// passed, and its target lands in `ejectedValidators`. That block carries
/// exactly the resolution its floor requires, and no other does.
#[tokio::test]
async fn a_claim_the_committee_backs_ejects_its_target() {
    let Decided {
        mut nodes,
        target,
        resolving,
        unresolved,
    } = decide_a_claim(true).await;

    let Some(SystemDeployData::ResolveClaims { resolutions, .. }) = resolution_of(&resolving)
    else {
        unreachable!()
    };
    assert_eq!(resolutions.len(), 1);
    assert_eq!(resolutions[0].claim.target, target);
    assert_eq!(resolutions[0].outcome, ClaimOutcome::Pass);

    let judge_node = judge_index(&nodes, &resolving);
    assert!(matches!(
        judge(&nodes[judge_node], &resolving).await,
        Either::Right(ValidBlock::Valid)
    ));
    let mut withheld = resolving.clone();
    withheld.body.system_deploys.retain(|deploy| {
        !matches!(deploy, ProcessedSystemDeploy::Succeeded {
            system_deploy: SystemDeployData::ResolveClaims { .. },
            ..
        })
    });
    assert!(
        matches!(
            judge(&nodes[judge_node], &withheld).await,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidClaimResolution))
        ),
        "a block withholding a required resolution is invalid"
    );

    // The resolution names the premature block's own floor, so only the
    // tally at that floor can refuse it.
    let premature_floor = floor_view(&nodes[judge_node], &unresolved).await;
    let mut premature = unresolved.clone();
    premature
        .body
        .system_deploys
        .insert(0, ProcessedSystemDeploy::Succeeded {
            event_list: Vec::new(),
            system_deploy: SystemDeployData::ResolveClaims {
                resolutions: resolutions.clone(),
                grace_expiries: Vec::new(),
                expiries: Vec::new(),
                floor_hash: premature_floor.hash.clone(),
                floor_number: premature_floor.number,
            },
        });
    assert!(
        matches!(
            judge(&nodes[judge_node], &premature).await,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidClaimResolution))
        ),
        "a block resolving a claim its floor does not decide is invalid"
    );

    admit_everywhere(&mut nodes, &resolving).await;
    let state = nodes[0]
        .runtime_manager
        .floor_state(&resolving.body.state.post_state_hash)
        .await
        .expect("post-state read");
    assert!(state.ejected.contains(&target), "the target is ejected");
    assert!(
        !state
            .claims
            .iter()
            .any(|record| record.claim.target == target),
        "the passed claim's slot is cleared"
    );
}

/// A claim the committee votes down fails once the nays are visible at a
/// floor: its slot is cleared and its target stays.
#[tokio::test]
async fn a_claim_the_committee_rejects_clears_its_slot() {
    let Decided {
        mut nodes,
        target,
        resolving,
        ..
    } = decide_a_claim(false).await;

    let Some(SystemDeployData::ResolveClaims { resolutions, .. }) = resolution_of(&resolving)
    else {
        unreachable!()
    };
    assert_eq!(resolutions.len(), 1);
    assert_eq!(resolutions[0].claim.target, target);
    assert_eq!(resolutions[0].outcome, ClaimOutcome::Fail);

    admit_everywhere(&mut nodes, &resolving).await;
    let state = nodes[0]
        .runtime_manager
        .floor_state(&resolving.body.state.post_state_hash)
        .await
        .expect("post-state read");
    assert!(
        !state.ejected.contains(&target),
        "the target is not ejected"
    );
    assert!(
        !state
            .claims
            .iter()
            .any(|record| record.claim.target == target),
        "the failed claim's slot is cleared"
    );
}

/// A block whose floor requires no resolution carries no claim-resolution
/// deploy at all: not an empty one, and not a failed one.
#[tokio::test]
async fn a_block_requiring_no_resolution_carries_none() {
    let Decided {
        mut nodes,
        resolving,
        ..
    } = decide_a_claim(true).await;
    admit_everywhere(&mut nodes, &resolving).await;
    let quiet = propose(&mut nodes[0], &[], &[]).await;
    assert!(resolution_of(&quiet).is_none());
    let judge_node = judge_index(&nodes, &quiet);
    assert!(matches!(
        judge(&nodes[judge_node], &quiet).await,
        Either::Right(ValidBlock::Valid)
    ));

    let floor = floor_view(&nodes[judge_node], &quiet).await;
    let mut empty = quiet.clone();
    empty
        .body
        .system_deploys
        .insert(0, ProcessedSystemDeploy::Succeeded {
            event_list: Vec::new(),
            system_deploy: SystemDeployData::ResolveClaims {
                resolutions: Vec::new(),
                grace_expiries: Vec::new(),
                expiries: Vec::new(),
                floor_hash: floor.hash.clone(),
                floor_number: floor.number,
            },
        });
    assert!(
        matches!(
            judge(&nodes[judge_node], &empty).await,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidClaimResolution))
        ),
        "an empty resolution is a resolution nothing required"
    );

    // A failed record names no deploy kind, so the claims rule cannot see it;
    // replay refuses it before that rule runs.
    let mut failed = quiet.clone();
    failed
        .body
        .system_deploys
        .insert(0, ProcessedSystemDeploy::Failed {
            event_list: Vec::new(),
            error_msg: "A resolution names no claim open at this block.".to_string(),
        });
    let failed = resigned(&nodes, &failed);
    let verdict = nodes[judge_node].add_block(failed).await;
    assert!(
        !matches!(verdict, Ok(Either::Right(ValidBlock::Valid))),
        "a failed system-deploy record does not pass: {verdict:?}"
    );
}
