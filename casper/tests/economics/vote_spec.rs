use casper::rust::block_status::{BlockError, InvalidBlock, ValidBlock};
use casper::rust::casper::Casper;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, ClaimFamily, ClaimId, LeakyBucketVote,
};
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::history::Either;

use super::support::{admit_everywhere, identity, judge, propose};
use crate::helper::block_util::resign_block;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

async fn network(vote_list_bound: Option<i64>) -> Vec<TestNode> {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    super::support::fund_validators(&mut parameters.2);
    if let Some(bound) = vote_list_bound {
        parameters.2.proof_of_stake.vote_list_bound = bound;
    }
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    TestNode::create_network(genesis, 3, Some(0.0), None, None)
        .await
        .expect("network")
}

fn claim_opened_by(block: &BlockMessage, target: &Validator) -> ClaimId {
    ClaimId {
        family: ClaimFamily::Eject,
        target: target.clone(),
        opened_at: block.body.state.block_number,
        proposer: block.sender.clone(),
    }
}

fn resigned(node: &TestNode, block: &BlockMessage) -> BlockMessage {
    let key = &node
        .validator_id_opt
        .as_ref()
        .expect("validator")
        .private_key;
    resign_block(block, key)
}

/// Every member of a claim's electorate owes one vote on it, the target and
/// the opener included, and owes it once: its next block carries it and no
/// later block repeats it. The vote is the proposer's own view of the target.
#[tokio::test]
async fn each_member_votes_once_and_votes_its_view() {
    let mut nodes = network(None).await;
    let target = identity(&nodes[2]);

    let opening = propose(&mut nodes[0], std::slice::from_ref(&target), &[]).await;
    admit_everywhere(&mut nodes, &opening).await;
    let claim = claim_opened_by(&opening, &target);

    let yea = propose(&mut nodes[1], &[], std::slice::from_ref(&target)).await;
    assert_eq!(yea.body.leaky_bucket_votes, vec![LeakyBucketVote {
        claim: claim.clone(),
        yea: true,
        testimony: None,
    }]);
    admit_everywhere(&mut nodes, &yea).await;

    let later = propose(&mut nodes[1], &[], &[]).await;
    assert!(
        later.body.leaky_bucket_votes.is_empty(),
        "a vote already cast is not owed again"
    );
    admit_everywhere(&mut nodes, &later).await;

    for index in [0, 2] {
        let block = propose(&mut nodes[index], &[], &[]).await;
        assert_eq!(
            block.body.leaky_bucket_votes,
            vec![LeakyBucketVote {
                claim: claim.clone(),
                yea: false,
                testimony: None,
            }],
            "member {index} votes its own view"
        );
        admit_everywhere(&mut nodes, &block).await;
    }
}

/// Withholding an owed vote invalidates the block.
#[tokio::test]
async fn a_block_withholding_an_owed_vote_is_invalid() {
    let mut nodes = network(None).await;
    let target = identity(&nodes[2]);
    let opening = propose(&mut nodes[0], std::slice::from_ref(&target), &[]).await;
    admit_everywhere(&mut nodes, &opening).await;

    let voting = propose(&mut nodes[1], &[], &[]).await;
    assert_eq!(voting.body.leaky_bucket_votes.len(), 1);
    let mut withheld = voting.clone();
    withheld.body.leaky_bucket_votes.clear();
    let withheld = resigned(&nodes[1], &withheld);

    let verdict = nodes[2].add_block(withheld).await.expect("process");
    assert!(
        matches!(
            verdict,
            Either::Left(BlockError::Invalid(InvalidBlock::MissingLeakyBucketVotes))
        ),
        "unexpected verdict: {verdict:?}"
    );
}

/// A vote must name a claim open at the block's pre-state that the proposer
/// has not voted on yet, and a block names each claim once.
#[tokio::test]
async fn a_vote_the_proposer_does_not_owe_is_invalid() {
    let mut nodes = network(None).await;
    let target = identity(&nodes[2]);
    let opening = propose(&mut nodes[0], std::slice::from_ref(&target), &[]).await;
    admit_everywhere(&mut nodes, &opening).await;
    let claim = claim_opened_by(&opening, &target);

    let voting = propose(&mut nodes[1], &[], &[]).await;

    let mut doubled = voting.clone();
    doubled.body.leaky_bucket_votes.push(LeakyBucketVote {
        claim: claim.clone(),
        yea: true,
        testimony: None,
    });
    let mut unknown = voting.clone();
    unknown.body.leaky_bucket_votes = vec![LeakyBucketVote {
        claim: ClaimId {
            opened_at: claim.opened_at + 1,
            ..claim.clone()
        },
        yea: false,
        testimony: None,
    }];
    for forged in [doubled, unknown] {
        let forged = resigned(&nodes[1], &forged);
        let verdict = nodes[2].add_block(forged).await.expect("process");
        assert!(
            matches!(
                verdict,
                Either::Left(BlockError::Invalid(InvalidBlock::InvalidLeakyBucketVote))
            ),
            "unexpected verdict: {verdict:?}"
        );
    }

    admit_everywhere(&mut nodes, &voting).await;
    let mut repeated = propose(&mut nodes[1], &[], &[]).await;
    assert!(repeated.body.leaky_bucket_votes.is_empty());
    repeated.body.leaky_bucket_votes = vec![LeakyBucketVote {
        claim,
        yea: false,
        testimony: None,
    }];
    let repeated = resigned(&nodes[1], &repeated);
    let verdict = nodes[2].add_block(repeated).await.expect("process");
    assert!(
        matches!(
            verdict,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidLeakyBucketVote))
        ),
        "a second vote on the same claim: {verdict:?}"
    );
}

/// A block that reaches a node late is judged as it would have been on time:
/// the node pruning votes on claims its floor has passed does not change
/// what that block owed.
#[tokio::test]
async fn a_late_block_is_judged_the_same_after_its_claims_votes_are_pruned() {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    super::support::fund_validators(&mut parameters.2);
    // The claim deadline is an epoch, so a short one lets the floor pass it.
    parameters.2.proof_of_stake.epoch_length = 20;
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
    let late_index = nodes
        .iter()
        .position(|node| identity(node) == lightest)
        .expect("the lightest validator runs a node");
    let mut late_node = nodes.remove(late_index);
    let late = identity(&late_node);
    let target = identity(&nodes[1]);

    let opening = propose(&mut nodes[0], std::slice::from_ref(&target), &[]).await;
    admit_everywhere(&mut nodes, &opening).await;
    admit_everywhere(std::slice::from_mut(&mut late_node), &opening).await;
    let claim = claim_opened_by(&opening, &target);

    let voted = propose(&mut late_node, &[], &[]).await;
    assert_eq!(voted.body.leaky_bucket_votes.len(), 1);
    admit_everywhere(&mut nodes, &voted).await;
    admit_everywhere(std::slice::from_mut(&mut late_node), &voted).await;

    let late_block = propose(&mut late_node, &[], &[]).await;
    assert!(late_block.body.leaky_bucket_votes.is_empty());
    let deadline = nodes[0].casper.bucket_view().params().claim_deadline_blocks;
    assert!(claim.opened_at + deadline > late_block.body.state.block_number);
    assert!(matches!(
        judge(&nodes[0], &late_block).await,
        Either::Right(ValidBlock::Valid)
    ));

    let mut pruned = false;
    for round in 0..400 {
        let block = propose(&mut nodes[round % 3], &[], &[]).await;
        admit_everywhere(&mut nodes, &block).await;
        if nodes[0]
            .block_dag_storage
            .leaky_bucket_votes(&claim, &late)
            .expect("vote index")
            .is_none()
        {
            pruned = true;
            break;
        }
    }
    assert!(
        pruned,
        "the floor passes the claim and its votes are pruned"
    );

    let judged = judge(&nodes[0], &late_block).await;
    let admitted = nodes[0].add_block(late_block).await.expect("process");
    assert!(
        matches!(judged, Either::Right(ValidBlock::Valid))
            && matches!(admitted, Either::Right(ValidBlock::Valid)),
        "pruning changed the verdict: judged {judged:?}, admitted {admitted:?}"
    );
}

/// When more votes are owed than a block may carry, the oldest claims are
/// voted on first and the rest fall to later blocks.
#[tokio::test]
async fn the_vote_bound_takes_the_oldest_claims_first() {
    let mut nodes = network(Some(1)).await;
    let first_target = identity(&nodes[1]);
    let second_target = identity(&nodes[0]);

    let first = propose(&mut nodes[0], std::slice::from_ref(&first_target), &[]).await;
    admit_everywhere(&mut nodes, &first).await;
    let second = propose(&mut nodes[1], std::slice::from_ref(&second_target), &[]).await;
    admit_everywhere(&mut nodes, &second).await;
    assert_eq!(
        second.body.leaky_bucket_votes.len(),
        1,
        "the opener of the second claim owed the first"
    );

    let older = propose(&mut nodes[2], &[], &[]).await;
    assert_eq!(
        older
            .body
            .leaky_bucket_votes
            .iter()
            .map(|vote| vote.claim.clone())
            .collect::<Vec<_>>(),
        vec![claim_opened_by(&first, &first_target)]
    );
    admit_everywhere(&mut nodes, &older).await;

    let newer = propose(&mut nodes[2], &[], &[]).await;
    assert_eq!(
        newer
            .body
            .leaky_bucket_votes
            .iter()
            .map(|vote| vote.claim.clone())
            .collect::<Vec<_>>(),
        vec![claim_opened_by(&second, &second_target)]
    );
    admit_everywhere(&mut nodes, &newer).await;
}

/// When more votes are owed than a block may carry, a block that votes on a
/// newer claim in place of the oldest is invalid.
#[tokio::test]
async fn a_block_skipping_the_oldest_owed_claim_is_invalid() {
    let mut nodes = network(Some(1)).await;
    let first_target = identity(&nodes[1]);
    let second_target = identity(&nodes[0]);

    let first = propose(&mut nodes[0], std::slice::from_ref(&first_target), &[]).await;
    admit_everywhere(&mut nodes, &first).await;
    let second = propose(&mut nodes[1], std::slice::from_ref(&second_target), &[]).await;
    admit_everywhere(&mut nodes, &second).await;

    let older = propose(&mut nodes[2], &[], &[]).await;
    assert_eq!(older.body.leaky_bucket_votes.len(), 1);
    assert_eq!(
        older.body.leaky_bucket_votes[0].claim,
        claim_opened_by(&first, &first_target)
    );
    let mut skipping = older.clone();
    skipping.body.leaky_bucket_votes = vec![LeakyBucketVote {
        claim: claim_opened_by(&second, &second_target),
        yea: false,
        testimony: None,
    }];
    let skipping = resigned(&nodes[2], &skipping);
    let verdict = nodes[0].add_block(skipping).await.expect("process");
    assert!(
        matches!(
            verdict,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidLeakyBucketVote))
        ),
        "unexpected verdict: {verdict:?}"
    );
}

/// An equivocator's vote counts on the branch that carries it whichever of
/// its twins a node received first, so honest nodes judge the next block on
/// that branch alike.
#[tokio::test]
async fn a_vote_counts_on_its_branch_whichever_twin_arrived_first() {
    let mut nodes = network(None).await;
    let target = identity(&nodes[2]);
    let opening = propose(&mut nodes[0], std::slice::from_ref(&target), &[]).await;
    admit_everywhere(&mut nodes, &opening).await;
    let claim = claim_opened_by(&opening, &target);

    let voting = propose(&mut nodes[1], &[], std::slice::from_ref(&target)).await;
    let twin = propose(&mut nodes[1], &[], &[]).await;
    assert_eq!(voting.seq_num, twin.seq_num);
    assert_eq!(voting.body.leaky_bucket_votes, vec![LeakyBucketVote {
        claim: claim.clone(),
        yea: true,
        testimony: None,
    }]);
    assert_eq!(twin.body.leaky_bucket_votes, vec![LeakyBucketVote {
        claim,
        yea: false,
        testimony: None,
    }]);
    admit_everywhere(std::slice::from_mut(&mut nodes[1]), &voting).await;
    let above_voting = propose(&mut nodes[1], &[], &[]).await;
    assert!(above_voting.body.leaky_bucket_votes.is_empty());

    let mut verdicts = Vec::new();
    for (judge_index, first, second) in [(0, &voting, &twin), (2, &twin, &voting)] {
        let judge = &mut nodes[judge_index];
        let verdict = judge.add_block(first.clone()).await.expect("process");
        assert!(matches!(verdict, Either::Right(ValidBlock::Valid)));
        let verdict = judge.add_block(second.clone()).await.expect("process");
        assert!(
            matches!(
                verdict,
                Either::Left(BlockError::Invalid(
                    InvalidBlock::IgnorableEquivocation | InvalidBlock::AdmissibleEquivocation
                ))
            ),
            "the second twin is an equivocation: {verdict:?}"
        );
        verdicts.push(
            judge
                .add_block(above_voting.clone())
                .await
                .expect("process"),
        );
    }
    assert!(
        verdicts
            .iter()
            .all(|verdict| matches!(verdict, Either::Right(ValidBlock::Valid))),
        "verdicts by arrival order (voting first, twin first): {verdicts:?}"
    );
}
