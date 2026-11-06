use casper::rust::block_status::{BlockError, InvalidBlock};
use casper::rust::casper::{Casper, MultiParentCasper};
use casper::rust::util::construct_deploy;
use casper::rust::validator_identity::ValidatorIdentity;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, ClaimFamily, ClaimOutcome, ProcessedSystemDeploy, SystemDeployData,
};
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::history::Either;

use super::support::{admit_everywhere, identity, judge, propose};
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

/// A four-validator network whose validators can pay for deploys and topups,
/// and the index of its lightest validator.
async fn funded_network(epoch_length: Option<i32>) -> (Vec<TestNode>, usize, String) {
    funded_network_with(|proof_of_stake| {
        if let Some(epoch_length) = epoch_length {
            proof_of_stake.epoch_length = epoch_length;
        }
    })
    .await
}

/// A reward rate of 2.67 a block per member per bond unit. The lightest
/// validator, bonded at 1, acknowledges the other two producers' new work,
/// half the committee's stake (0.75), at a recency between 1/2 and 1: it
/// earns 4 to 8 a block in a committee of four. Eight of its blocks prove
/// more overflow than a claim of 10; no single block does.
fn proving_overflow(
    proof_of_stake: &mut casper::rust::genesis::contracts::proof_of_stake::ProofOfStake,
) {
    proof_of_stake.reward_rate_ppm = 2_666_667;
}

/// The same network, with `adjust` applied to its proof-of-stake parameters.
async fn funded_network_with(
    adjust: impl FnOnce(&mut casper::rust::genesis::contracts::proof_of_stake::ProofOfStake),
) -> (Vec<TestNode>, usize, String) {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    adjust(&mut parameters.2.proof_of_stake);
    super::support::fund_validators(&mut parameters.2);
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let shard_id = genesis.genesis_block.shard_id.clone();
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
    let nodes = TestNode::create_network(genesis, 4, Some(0.0), None, None)
        .await
        .expect("network");
    let target = nodes
        .iter()
        .position(|node| identity(node) == lightest)
        .expect("the lightest validator runs a node");
    (nodes, target, shard_id)
}

fn resolutions_of(block: &BlockMessage) -> Option<&SystemDeployData> {
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

/// Proposes round-robin from every node but `skip`, admitting each block
/// everywhere, until a block carries a claim resolution `wanted` accepts.
async fn propose_until(
    nodes: &mut [TestNode],
    skip: Option<usize>,
    wanted: impl Fn(&SystemDeployData) -> bool,
) -> BlockMessage {
    let block = propose_until_unadmitted(nodes, skip, wanted).await;
    admit_everywhere(nodes, &block).await;
    block
}

/// The same, leaving the block that carries the resolution unadmitted.
async fn propose_until_unadmitted(
    nodes: &mut [TestNode],
    skip: Option<usize>,
    wanted: impl Fn(&SystemDeployData) -> bool,
) -> BlockMessage {
    for round in 0..200 {
        let index = round % nodes.len();
        if Some(index) == skip {
            continue;
        }
        let block = propose(&mut nodes[index], &[], &[]).await;
        if resolutions_of(&block).is_some_and(&wanted) {
            return block;
        }
        admit_everywhere(nodes, &block).await;
    }
    panic!("no block carried the resolution");
}

/// The rest of the committee finds `target` drained and ejects it.
async fn eject(nodes: &mut [TestNode], target: usize) {
    let target_key = identity(&nodes[target]);
    let accusers: Vec<usize> = (0..nodes.len()).filter(|i| *i != target).collect();
    let opening = propose(
        &mut nodes[accusers[0]],
        std::slice::from_ref(&target_key),
        &[],
    )
    .await;
    admit_everywhere(nodes, &opening).await;
    for &index in &accusers {
        let voting = propose(&mut nodes[index], &[], std::slice::from_ref(&target_key)).await;
        admit_everywhere(nodes, &voting).await;
    }
    let resolving = propose_until(nodes, None, |data| {
        matches!(data, SystemDeployData::ResolveClaims { resolutions, .. }
            if resolutions.iter().any(|r| r.claim.family == ClaimFamily::Eject
                && r.claim.target == target_key && r.outcome == ClaimOutcome::Pass))
    })
    .await;
    let Some(SystemDeployData::ResolveClaims { floor_number, .. }) = resolutions_of(&resolving)
    else {
        unreachable!()
    };
    assert!(
        *floor_number < resolving.body.state.block_number,
        "the ejection floor F ({floor_number}) lies below the block that ejects ({})",
        resolving.body.state.block_number
    );
}

async fn state_of(
    node: &TestNode,
    block: &BlockMessage,
) -> std::sync::Arc<casper::rust::rholang::runtime::FloorState> {
    node.runtime_manager
        .floor_state(&block.body.state.post_state_hash)
        .await
        .expect("post-state read")
}

fn key_of(node: &TestNode) -> &ValidatorIdentity {
    node.validator_id_opt.as_ref().expect("validator")
}

/// `term` signed by `key` now, with a phlo limit a funded validator's vault
/// covers and a claim's opening fits in.
fn signed_now(
    term: String,
    key: &crypto::rust::private_key::PrivateKey,
    shard_id: &str,
) -> crypto::rust::signatures::signed::Signed<
    models::rust::casper::protocol::casper_message::DeployData,
> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis() as i64;
    construct_deploy::source_deploy(
        term,
        now,
        Some(500_000),
        None,
        Some(key.clone()),
        None,
        Some(shard_id.to_string()),
    )
    .expect("deploy")
}

/// A block from `node` carrying one deploy, signed by `node`, that calls the
/// PoS method `call` with the deployer's identity first.
async fn call_pos(node: &mut TestNode, call: &str, shard_id: &str) -> BlockMessage {
    let deploy = signed_now(
        format!(
            r#"
              new rl(`rho:registry:lookup`), poSCh, deployerId(`rho:system:deployerId`), ret in {{
                rl!(`rho:system:pos`, *poSCh) |
                for (@(_, PoS) <- poSCh) {{
                  @PoS!({call}, *ret)
                }}
              }}
            "#
        ),
        &key_of(node).private_key,
        shard_id,
    );
    match node.create_block(&[deploy]).await.expect("block creation") {
        casper::rust::blocks::proposer::propose_result::BlockCreatorResult::Created(block, ..) => {
            block
        }
        other => panic!("expected a created block, got {other:?}"),
    }
}

async fn rejoin(nodes: &mut [TestNode], target: usize, topup: i64, shard_id: &str) {
    let requesting = call_pos(
        &mut nodes[target],
        &format!(r#""rejoinFromEjection", *deployerId, {topup}"#),
        shard_id,
    )
    .await;
    admit_everywhere(nodes, &requesting).await;
}

/// An ejected validator that asks to rejoin with a topup covering its
/// shortfall is voted back by the electorate that ejected it, pays the topup
/// on success, and is reinstated.
#[tokio::test]
async fn a_rejoin_the_electorate_backs_reinstates_its_validator() {
    let (mut nodes, target, shard_id) = funded_network(None).await;
    eject(&mut nodes, target).await;
    let target_key: Validator = identity(&nodes[target]);

    let requesting = call_pos(
        &mut nodes[target],
        r#""rejoinFromEjection", *deployerId, 100"#,
        &shard_id,
    )
    .await;
    admit_everywhere(&mut nodes, &requesting).await;
    let opened = state_of(&nodes[0], &requesting).await;
    assert!(
        opened
            .claims
            .iter()
            .any(|record| record.claim.family == ClaimFamily::Rejoin
                && record.claim.target == target_key),
        "the request opens a rejoin claim"
    );

    let resolving = propose_until(&mut nodes, None, |data| {
        matches!(data, SystemDeployData::ResolveClaims { resolutions, .. }
            if resolutions.iter().any(|r| r.claim.family == ClaimFamily::Rejoin
                && r.claim.target == target_key))
    })
    .await;
    let Some(SystemDeployData::ResolveClaims { resolutions, .. }) = resolutions_of(&resolving)
    else {
        unreachable!()
    };
    assert!(resolutions
        .iter()
        .any(|r| r.claim.target == target_key && r.outcome == ClaimOutcome::Pass));

    let state = state_of(&nodes[0], &resolving).await;
    assert!(
        !state.ejected.contains(&target_key),
        "the validator is reinstated"
    );
    let paid_at = resolving.body.state.block_number;
    assert!(
        state
            .freezes
            .iter()
            .any(|(validator, intervals)| validator == &target_key
                && intervals.iter().any(|interval| interval.rejoin
                    == Some(models::rust::casper::protocol::casper_message::Rejoin {
                        at: paid_at,
                        topup: 100
                    }))),
        "the paid rejoin closes the freeze at its block"
    );
}

/// A validator that does not rejoin within its grace is
/// slashed by the first block past it, and a block that withholds the expiry
/// is invalid.
#[tokio::test]
async fn an_ejected_validator_is_slashed_when_its_grace_ends() {
    let (mut nodes, target, _) = funded_network(Some(10)).await;
    eject(&mut nodes, target).await;
    let target_key: Validator = identity(&nodes[target]);

    let expiring = propose_until_unadmitted(&mut nodes, Some(target), |data| {
        matches!(data, SystemDeployData::ResolveClaims { grace_expiries, .. }
            if grace_expiries.contains(&target_key))
    })
    .await;
    let judge_node = nodes
        .iter()
        .position(|node| identity(node) != expiring.sender && identity(node) != target_key)
        .expect("a judge");
    let mut withheld = expiring.clone();
    for deploy in withheld.body.system_deploys.iter_mut() {
        if let ProcessedSystemDeploy::Succeeded {
            system_deploy: SystemDeployData::ResolveClaims { grace_expiries, .. },
            ..
        } = deploy
        {
            grace_expiries.clear();
        }
    }
    assert!(
        matches!(
            judge(&nodes[judge_node], &withheld).await,
            Either::Left(BlockError::Invalid(InvalidBlock::InvalidClaimResolution))
        ),
        "a block withholding a due grace expiry is invalid"
    );

    admit_everywhere(&mut nodes, &expiring).await;
    let state = state_of(&nodes[0], &expiring).await;
    assert!(
        !state.ejected.contains(&target_key),
        "the ejection is closed"
    );
    assert!(
        state
            .bonds
            .iter()
            .any(|bond| bond.validator == target_key && bond.stake == 0),
        "the validator's bond is slashed"
    );
}

/// A nay on an overflow claim records what the voting block's floor shows of
/// the claimant, and a block whose nay records anything else, or nothing, is
/// invalid.
#[tokio::test]
async fn a_nay_on_an_overflow_claim_records_what_its_floor_shows() {
    let (mut nodes, claimant, shard_id) = funded_network(None).await;
    let claimant_key: Validator = identity(&nodes[claimant]);
    let requesting = call_pos(
        &mut nodes[claimant],
        r#""claimOverflow", *deployerId, 10"#,
        &shard_id,
    )
    .await;
    admit_everywhere(&mut nodes, &requesting).await;

    let voter = (claimant + 2) % nodes.len();
    let voting = propose(&mut nodes[voter], &[], &[]).await;
    let nay = voting
        .body
        .leaky_bucket_votes
        .iter()
        .find(|vote| {
            vote.claim.family == ClaimFamily::Overflow && vote.claim.target == claimant_key
        })
        .expect("the voter owes a vote on the open claim")
        .clone();
    assert!(!nay.yea, "nothing has been earned, so the claim is refused");
    let floor = super::support::floor_view(&nodes[voter], &voting).await;
    assert_eq!(
        nay.testimony,
        Some(
            models::rust::casper::protocol::casper_message::OverflowTestimony {
                amount: 10,
                floor_number: floor.number,
                claimed_total: 0,
                freezes: Vec::new(),
            }
        )
    );

    let judge_node = nodes
        .iter()
        .position(|node| identity(node) != voting.sender)
        .expect("a judge");
    let forge =
        |testimony: Option<models::rust::casper::protocol::casper_message::OverflowTestimony>| {
            let mut forged = voting.clone();
            for vote in forged.body.leaky_bucket_votes.iter_mut() {
                if vote.claim == nay.claim {
                    vote.testimony = testimony.clone();
                }
            }
            forged
        };
    for (forged, what) in [
        (
            forge(nay.testimony.clone().map(|testimony| {
                models::rust::casper::protocol::casper_message::OverflowTestimony {
                    claimed_total: testimony.claimed_total + 1,
                    ..testimony
                }
            })),
            "a total the floor does not show",
        ),
        (forge(None), "no testimony"),
    ] {
        assert!(
            matches!(
                judge(&nodes[judge_node], &forged).await,
                Either::Left(BlockError::Invalid(InvalidBlock::InvalidLeakyBucketVote))
            ),
            "a nay carrying {what} is invalid"
        );
    }
    admit_everywhere(&mut nodes, &voting).await;
}

/// A relay that changes a vote in a block it forwards produces a block every
/// other node refuses, because the proposer's signature no longer covers it.
#[tokio::test]
async fn a_vote_changed_in_transit_is_refused() {
    let (mut nodes, claimant, shard_id) = funded_network(None).await;
    let claimant_key: Validator = identity(&nodes[claimant]);
    let requesting = call_pos(
        &mut nodes[claimant],
        r#""claimOverflow", *deployerId, 10"#,
        &shard_id,
    )
    .await;
    admit_everywhere(&mut nodes, &requesting).await;

    let voter = (claimant + 2) % nodes.len();
    let voting = propose(&mut nodes[voter], &[], &[]).await;
    let mut relayed = voting.clone();
    let vote = relayed
        .body
        .leaky_bucket_votes
        .iter_mut()
        .find(|vote| {
            vote.claim.family == ClaimFamily::Overflow && vote.claim.target == claimant_key
        })
        .expect("the voter owes a vote on the open claim");
    assert!(
        !vote.yea,
        "nothing has been earned, so the claim is refused"
    );
    vote.yea = true;
    vote.testimony = None;
    assert_eq!(relayed.block_hash, voting.block_hash);

    for (index, node) in nodes.iter_mut().enumerate() {
        if index == voter {
            continue;
        }
        let verdict = node.add_block(relayed.clone()).await.expect("process");
        assert!(
            matches!(
                verdict,
                Either::Left(BlockError::Invalid(InvalidBlock::InvalidBlockHash))
            ),
            "node {index}: {verdict:?}"
        );
    }
}

/// `block` re-signed by `node` with a nay on `target`'s overflow claim,
/// carrying what the block's floor records. The proposer never casts a nay
/// its own ancestry contradicts, so a dishonest voter edits its own block.
async fn with_forged_nay(
    node: &TestNode,
    block: &BlockMessage,
    target: &Validator,
) -> BlockMessage {
    let book = node
        .runtime_manager
        .claim_book(&block.body.state.pre_state_hash)
        .await
        .expect("claims");
    let record = book
        .records
        .iter()
        .find(|record| {
            record.claim.family == ClaimFamily::Overflow && record.claim.target == *target
        })
        .expect("the claim is open")
        .clone();
    let floor = super::support::floor_view(node, block).await;
    let mut forged = block.clone();
    for vote in forged.body.leaky_bucket_votes.iter_mut() {
        if vote.claim == record.claim {
            vote.yea = false;
            vote.testimony = casper::rust::economics::claims::testimony(&record, false, &floor);
        }
    }
    crate::helper::block_util::resign_block(&forged, &key_of(node).private_key)
}

fn evidence_in(block: &BlockMessage) -> Vec<Validator> {
    block
        .body
        .system_deploys
        .iter()
        .filter_map(|deploy| match deploy {
            ProcessedSystemDeploy::Succeeded {
                system_deploy: SystemDeployData::DishonestVote { accused, .. },
                ..
            } => Some(accused.clone()),
            _ => None,
        })
        .collect()
}

/// A validator that votes nay on a claim its own ancestry proves the claimant
/// earned is slashed on evidence another node's own view offers, while a nay
/// its ancestry does not contradict is never offered; evidence that proves
/// nothing makes its block invalid.
#[tokio::test]
async fn a_nay_its_own_ancestry_contradicts_is_slashed_and_an_honest_one_is_not() {
    use std::sync::Arc;

    use casper::rust::economics::bucket_view::{LeakyBucketDecision, LeakyBucketProposal};

    let (mut nodes, claimant, shard_id) = funded_network_with(proving_overflow).await;
    let size = nodes.len();
    let (liar, accuser, idle) = (
        (claimant + 1) % size,
        (claimant + 2) % size,
        (claimant + 3) % size,
    );
    let claimant_key = identity(&nodes[claimant]);
    let liar_key = identity(&nodes[liar]);
    let idle_key = identity(&nodes[idle]);

    let mut last = None;
    for _ in 0..8 {
        for index in [claimant, liar, accuser] {
            let block = propose(&mut nodes[index], &[], &[]).await;
            admit_everywhere(&mut nodes, &block).await;
            last = Some(block);
        }
    }
    let last = last.expect("blocks were proposed");
    for node in &nodes {
        let floor = super::support::floor_view(node, &last).await;
        let own = identity(node);
        let held = node
            .casper
            .bucket_view()
            .proposal_at(&own, &floor.knowledge);
        let dag = node.block_dag_storage.get_representation().expect("dag");
        for validator in held.overflow.keys() {
            let testimony = models::rust::casper::protocol::casper_message::OverflowTestimony {
                amount: 0,
                floor_number: floor.number,
                claimed_total: floor.knowledge.claimed_total(validator),
                freezes: floor.knowledge.freezes_of(validator).to_vec(),
            };
            let provable = casper::rust::economics::dishonest_vote::provable_overflow(
                &dag,
                &last.block_hash,
                validator,
                &testimony,
                0,
                &node.casper.chain_parameters().leaky_bucket.reward,
            )
            .expect("provable overflow");
            assert!(
                provable <= held.overflow[validator],
                "the ancestry proves {provable} where an honest view holds {}",
                held.overflow[validator]
            );
        }
    }
    let requesting = call_pos(
        &mut nodes[claimant],
        r#""claimOverflow", *deployerId, 10"#,
        &shard_id,
    )
    .await;
    admit_everywhere(&mut nodes, &requesting).await;
    assert!(
        state_of(&nodes[0], &requesting)
            .await
            .claims
            .iter()
            .any(|record| record.claim.target == claimant_key),
        "the claimant's claim opens in the block that carries it"
    );
    // The idle validator's own request rides in another validator's block, so
    // it holds no block of its own and no view credits it anything.
    let idle_request = signed_now(
        r#"
          new rl(`rho:registry:lookup`), poSCh, deployerId(`rho:system:deployerId`), ret in {
            rl!(`rho:system:pos`, *poSCh) |
            for (@(_, PoS) <- poSCh) {
              @PoS!("claimOverflow", *deployerId, 10, *ret)
            }
          }
        "#
        .to_string(),
        &key_of(&nodes[idle]).private_key,
        &shard_id,
    );
    assert!(matches!(
        nodes[claimant]
            .casper
            .deploy(idle_request)
            .expect("admission"),
        Either::Right(_)
    ));
    let own_view = nodes[claimant]
        .casper
        .bucket_view()
        .decision(claimant_key.clone());
    let carrying = super::support::create(&mut nodes[claimant], own_view).await;
    admit_everywhere(&mut nodes, &carrying).await;
    assert_eq!(
        state_of(&nodes[0], &carrying).await.claims.len(),
        2,
        "both claims are open"
    );

    let view = nodes[liar].casper.bucket_view();
    let own = liar_key.clone();
    let decision: LeakyBucketDecision = Arc::new(move |knowledge| LeakyBucketProposal {
        overflow: Default::default(),
        ..view.proposal_at(&own, knowledge)
    });
    let created = super::support::create(&mut nodes[liar], decision).await;
    let lying = with_forged_nay(&nodes[liar], &created, &claimant_key).await;
    let nays: Vec<&Validator> = lying
        .body
        .leaky_bucket_votes
        .iter()
        .filter(|vote| vote.claim.family == ClaimFamily::Overflow && !vote.yea)
        .map(|vote| &vote.claim.target)
        .collect();
    assert_eq!(nays.len(), 2, "the liar refuses both claims");
    assert!(nays.contains(&&claimant_key) && nays.contains(&&idle_key));
    admit_everywhere(&mut nodes, &lying).await;

    let accusing = super::support::propose_by_own_view(&mut nodes[accuser]).await;
    assert_eq!(
        evidence_in(&accusing),
        vec![liar_key.clone()],
        "only the liar is accused, and only once"
    );
    let honest_nay = accusing
        .body
        .leaky_bucket_votes
        .iter()
        .find(|vote| vote.claim.target == idle_key)
        .expect("the accuser votes on the idle validator's claim");
    assert!(
        !honest_nay.yea,
        "one block has not earned the claimed overflow, so the accuser refuses it too"
    );

    let judge_node = nodes
        .iter()
        .position(|node| identity(node) != accusing.sender)
        .expect("a judge");
    let floor = super::support::floor_view(&nodes[judge_node], &accusing).await;
    let reach = nodes[judge_node]
        .casper
        .chain_parameters()
        .window_basis()
        .evidence_reach();
    let reward = nodes[judge_node]
        .casper
        .chain_parameters()
        .leaky_bucket
        .reward;
    let judged = |block: &BlockMessage| {
        let snapshot_dag = nodes[judge_node]
            .block_dag_storage
            .get_representation()
            .expect("dag");
        casper::rust::validate::Validate::dishonest_votes(
            block,
            &snapshot_dag,
            &nodes[judge_node].block_store,
            floor.number,
            reach,
            &reward,
        )
    };
    assert!(matches!(
        judged(&accusing),
        Either::Right(casper::rust::block_status::ValidBlock::Valid)
    ));
    // A judge that does not hold the vote block defers and fetches it, as a
    // judge holding it would otherwise rule differently.
    let mut without_vote = nodes[judge_node]
        .block_dag_storage
        .get_representation()
        .expect("dag");
    without_vote.dag_set.remove(&lying.block_hash);
    let deferred = casper::rust::validate::Validate::dishonest_votes(
        &accusing,
        &without_vote,
        &nodes[judge_node].block_store,
        floor.number,
        reach,
        &reward,
    );
    assert!(
        matches!(&deferred, Either::Left(BlockError::Undecidable(missing)) if *missing == lying.block_hash),
        "a judge missing the vote block decides {deferred:?}"
    );
    let reach_blocks = reach.expect("the test chain has a finite window");
    let mut without_parents = nodes[judge_node]
        .block_dag_storage
        .get_representation()
        .expect("dag");
    for parent in &lying.header.parents_hash_list {
        without_parents.dag_set.remove(parent);
    }
    let claimant_claim = lying
        .body
        .leaky_bucket_votes
        .iter()
        .find(|vote| vote.claim.target == claimant_key)
        .expect("the liar's nay on the claimant's claim")
        .claim
        .clone();
    assert!(!casper::rust::economics::dishonest_vote::proves_dishonesty(
        &without_parents,
        &nodes[judge_node].block_store,
        &casper::rust::economics::dishonest_vote::Accusation {
            accused: liar_key.clone(),
            vote_block: lying.block_hash.clone(),
            claim: claimant_claim.clone(),
        },
        lying.body.state.block_number + reach_blocks + 1,
        reach,
        &reward,
    )
    .expect("a vote below the reach is judged without reading its ancestry"));

    // A pending accusation judges as the evidence check does, keeping what it
    // read: a floor whose cutoff is at or above the one it read at is
    // answered without the ancestry, and one below it reads again.
    let full = nodes[judge_node]
        .block_dag_storage
        .get_representation()
        .expect("dag");
    let liar_accusation = casper::rust::economics::dishonest_vote::Accusation {
        accused: liar_key.clone(),
        vote_block: lying.block_hash.clone(),
        claim: claimant_claim,
    };
    let exact = |floor_number: i64| {
        casper::rust::economics::dishonest_vote::proves_dishonesty(
            &full,
            &nodes[judge_node].block_store,
            &liar_accusation,
            floor_number,
            reach,
            &reward,
        )
        .expect("evidence check")
    };
    let pending =
        casper::rust::economics::dishonest_vote::PendingAccusation::new(liar_accusation.clone());
    let judged_at =
        |dag: &block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation,
         floor_number: i64| {
            pending.proves(
                dag,
                &nodes[judge_node].block_store,
                floor_number,
                reach,
                &reward,
            )
        };
    assert_eq!(
        judged_at(&full, floor.number + 10).expect("read"),
        exact(floor.number + 10)
    );
    assert!(exact(floor.number + 10), "the liar's nay is proven");
    assert_eq!(
        judged_at(&without_parents, floor.number + 20)
            .expect("a higher cutoff is answered from what was read"),
        exact(floor.number + 20)
    );
    assert!(
        matches!(
            judged_at(&without_parents, floor.number),
            Err(casper::rust::errors::CasperError::BlockNotHeld(..))
        ),
        "a lower cutoff reads the ancestry again"
    );
    assert_eq!(
        judged_at(&full, floor.number).expect("read"),
        exact(floor.number)
    );
    let idle_claim = lying
        .body
        .leaky_bucket_votes
        .iter()
        .find(|vote| vote.claim.target == idle_key)
        .expect("the liar's nay on the idle claim")
        .claim
        .clone();
    let forge =
        |accused: &Validator,
         claim: Option<&models::rust::casper::protocol::casper_message::ClaimId>| {
            let mut forged = accusing.clone();
            for deploy in forged.body.system_deploys.iter_mut() {
                if let ProcessedSystemDeploy::Succeeded {
                    system_deploy:
                        SystemDeployData::DishonestVote {
                            accused: named,
                            claim: named_claim,
                            ..
                        },
                    ..
                } = deploy
                {
                    *named = accused.clone();
                    if let Some(claim) = claim {
                        *named_claim = claim.clone();
                    }
                }
            }
            forged
        };
    for (forged, what) in [
        (
            forge(&liar_key, Some(&idle_claim)),
            "a nay the ancestry does not contradict",
        ),
        (
            forge(&claimant_key, None),
            "a validator that did not cast the nay",
        ),
    ] {
        assert!(
            matches!(
                judged(&forged),
                Either::Left(BlockError::Invalid(InvalidBlock::InvalidDishonestVote))
            ),
            "evidence naming {what} is invalid"
        );
    }

    admit_everywhere(&mut nodes, &accusing).await;
    for node in &nodes {
        assert!(
            node.casper
                .bucket_view()
                .proposal(&identity(node))
                .accusations
                .iter()
                .any(|pending| pending.accusation.accused == liar_key),
            "the evidence's block may never become canonical, so the nay stays offered"
        );
    }
    let state = state_of(&nodes[0], &accusing).await;
    assert!(
        state
            .bonds
            .iter()
            .any(|bond| bond.validator == liar_key && bond.stake == 0),
        "the liar's bond is slashed"
    );
    assert!(
        !state.active_validators.contains(&liar_key),
        "and it leaves the active set"
    );
}

/// A voter whose view credits less than its own ancestry proves, as a view
/// that never observed some of the claimant's blocks does, backs the claim
/// rather than casting a nay another node can prove dishonest.
#[tokio::test]
async fn a_view_short_of_its_ancestry_backs_what_the_ancestry_proves() {
    use std::sync::Arc;

    use casper::rust::economics::bucket_view::{LeakyBucketDecision, LeakyBucketProposal};

    let (mut nodes, claimant, shard_id) = funded_network_with(proving_overflow).await;
    let size = nodes.len();
    let (voter, accuser) = ((claimant + 1) % size, (claimant + 2) % size);
    let claimant_key = identity(&nodes[claimant]);
    let voter_key = identity(&nodes[voter]);
    for _ in 0..8 {
        for index in [claimant, voter, accuser] {
            let block = propose(&mut nodes[index], &[], &[]).await;
            admit_everywhere(&mut nodes, &block).await;
        }
    }
    let requesting = call_pos(
        &mut nodes[claimant],
        r#""claimOverflow", *deployerId, 10"#,
        &shard_id,
    )
    .await;
    admit_everywhere(&mut nodes, &requesting).await;

    let view = nodes[voter].casper.bucket_view();
    let own = voter_key.clone();
    let short: LeakyBucketDecision = Arc::new(move |knowledge| LeakyBucketProposal {
        overflow: Default::default(),
        ..view.proposal_at(&own, knowledge)
    });
    let voting = super::support::create(&mut nodes[voter], short).await;
    let vote = voting
        .body
        .leaky_bucket_votes
        .iter()
        .find(|vote| vote.claim.target == claimant_key)
        .expect("the voter owes a vote on the claim");
    assert!(
        vote.yea,
        "the voter's own ancestry proves the claim, so it backs it"
    );
    admit_everywhere(&mut nodes, &voting).await;

    let accusing = super::support::propose_by_own_view(&mut nodes[accuser]).await;
    assert!(
        !evidence_in(&accusing).contains(&voter_key),
        "the voter cast no nay to prove"
    );
    admit_everywhere(&mut nodes, &accusing).await;
}

/// A proposer that would cast a nay but lacks a block its ancestry check
/// needs builds nothing and names that block, so it is fetched before the
/// proposer signs.
#[tokio::test]
async fn a_proposer_does_not_sign_a_nay_over_ancestry_it_does_not_hold() {
    use std::sync::Arc;

    use casper::rust::blocks::proposer::block_creator;
    use casper::rust::blocks::proposer::proposer::DeploySelection;
    use casper::rust::economics::bucket_view::{LeakyBucketDecision, LeakyBucketProposal};

    let (mut nodes, claimant, shard_id) = funded_network_with(proving_overflow).await;
    let size = nodes.len();
    let (voter, other) = ((claimant + 1) % size, (claimant + 2) % size);
    let voter_key = identity(&nodes[voter]);
    let mut claimant_blocks = Vec::new();
    for _ in 0..8 {
        for index in [claimant, voter, other] {
            let block = propose(&mut nodes[index], &[], &[]).await;
            if index == claimant {
                claimant_blocks.push(block.block_hash.clone());
            }
            admit_everywhere(&mut nodes, &block).await;
        }
    }
    let requesting = call_pos(
        &mut nodes[claimant],
        r#""claimOverflow", *deployerId, 10"#,
        &shard_id,
    )
    .await;
    admit_everywhere(&mut nodes, &requesting).await;

    let view = nodes[voter].casper.bucket_view();
    let own = voter_key.clone();
    let short: LeakyBucketDecision = Arc::new(move |knowledge| LeakyBucketProposal {
        overflow: Default::default(),
        ..view.proposal_at(&own, knowledge)
    });
    let mut snapshot = nodes[voter].casper.get_snapshot().await.expect("snapshot");
    snapshot.dag.dag_set.remove(&claimant_blocks[0]);
    let validator = nodes[voter].casper.get_validator().expect("validator");
    let node = &mut nodes[voter];
    let created = block_creator::create(
        &snapshot,
        &validator,
        None,
        node.deploy_storage.clone(),
        node.rejected_deploy_buffer.clone(),
        &node.runtime_manager,
        &mut node.block_store,
        &node.block_dag_storage,
        DeploySelection::StandardAllowEmpty,
        short,
    )
    .await;
    assert!(
        matches!(&created, Err(casper::rust::errors::CasperError::BlockNotHeld(missing, _))
            if *missing == claimant_blocks[0]),
        "block creation gave {created:?}"
    );
}

/// One block slashes two validators whose nays their own ancestry
/// contradicts, and a block merging it as a secondary parent keeps both
/// slashes.
#[tokio::test]
async fn evidence_against_two_voters_in_one_block_survives_a_merge() {
    use std::sync::Arc;

    use casper::rust::economics::bucket_view::{LeakyBucketDecision, LeakyBucketProposal};

    let (mut nodes, claimant, shard_id) = funded_network_with(proving_overflow).await;
    let stake_of = |node: &TestNode| {
        nodes[0]
            .casper
            .approved_block
            .body
            .state
            .bonds
            .iter()
            .find(|bond| bond.validator == identity(node))
            .expect("bonded")
            .stake
    };
    let mut others: Vec<usize> = (0..nodes.len()).filter(|i| *i != claimant).collect();
    others.sort_by_key(|i| stake_of(&nodes[*i]));
    let (accuser, liars) = (others[0], [others[1], others[2]]);
    let liar_keys: Vec<Validator> = liars.iter().map(|i| identity(&nodes[*i])).collect();

    for _ in 0..8 {
        for index in [claimant, liars[0], liars[1], accuser] {
            let block = propose(&mut nodes[index], &[], &[]).await;
            admit_everywhere(&mut nodes, &block).await;
        }
    }
    let requesting = call_pos(
        &mut nodes[claimant],
        r#""claimOverflow", *deployerId, 10"#,
        &shard_id,
    )
    .await;
    admit_everywhere(&mut nodes, &requesting).await;

    let without = |node: &TestNode, overflow: bool| -> LeakyBucketDecision {
        let view = node.casper.bucket_view();
        let own = identity(node);
        Arc::new(move |knowledge| {
            let decided = view.proposal_at(&own, knowledge);
            LeakyBucketProposal {
                overflow: if overflow {
                    decided.overflow.clone()
                } else {
                    Default::default()
                },
                accusations: Vec::new(),
                ..decided
            }
        })
    };
    for liar in liars {
        let decision = without(&nodes[liar], false);
        let created = super::support::create(&mut nodes[liar], decision).await;
        let lying = with_forged_nay(&nodes[liar], &created, &identity(&nodes[claimant])).await;
        assert!(
            lying
                .body
                .leaky_bucket_votes
                .iter()
                .any(|vote| vote.claim.family == ClaimFamily::Overflow && !vote.yea),
            "the liar refuses the claim"
        );
        admit_everywhere(&mut nodes, &lying).await;
    }

    let accusing = super::support::propose_by_own_view(&mut nodes[accuser]).await;
    let mut accused = evidence_in(&accusing);
    accused.sort();
    let mut expected = liar_keys.clone();
    expected.sort();
    assert_eq!(accused, expected, "one block carries evidence against both");
    let sibling_decision = without(&nodes[liars[1]], true);
    let sibling = super::support::create(&mut nodes[liars[1]], sibling_decision).await;
    assert!(evidence_in(&sibling).is_empty());
    admit_everywhere(&mut nodes, &accusing).await;
    admit_everywhere(&mut nodes, &sibling).await;

    let child_decision = without(&nodes[claimant], true);
    let child = super::support::create(&mut nodes[claimant], child_decision).await;
    assert_eq!(
        child.header.parents_hash_list.first(),
        Some(&sibling.block_hash),
        "the heavier sibling is the main parent"
    );
    assert!(
        child
            .header
            .parents_hash_list
            .contains(&accusing.block_hash),
        "the accusing block is merged in"
    );
    admit_everywhere(&mut nodes, &child).await;
    let state = state_of(&nodes[0], &child).await;
    for liar in &liar_keys {
        assert!(
            state
                .bonds
                .iter()
                .any(|bond| bond.validator == *liar && bond.stake == 0),
            "both slashes survive the merge: {:?}",
            state.bonds
        );
    }
}

/// A node restarted after a claim was paid from the overflow of blocks it
/// still holds takes that payment off again, and decides as it did before.
#[tokio::test]
async fn a_restarted_node_takes_off_a_payment_it_had_taken_off() {
    let (mut nodes, claimant, shard_id) = funded_network_with(proving_overflow).await;
    let claimant_key = identity(&nodes[claimant]);
    for _ in 0..8 {
        for index in 0..nodes.len() {
            let block = propose(&mut nodes[index], &[], &[]).await;
            admit_everywhere(&mut nodes, &block).await;
        }
    }
    let requesting = call_pos(
        &mut nodes[claimant],
        r#""claimOverflow", *deployerId, 10"#,
        &shard_id,
    )
    .await;
    admit_everywhere(&mut nodes, &requesting).await;
    propose_until(&mut nodes, None, |data| {
        matches!(data, SystemDeployData::ResolveClaims { resolutions, .. }
            if resolutions.iter().any(|r| r.claim.family == ClaimFamily::Overflow
                && r.claim.target == claimant_key && r.outcome == ClaimOutcome::Pass))
    })
    .await;

    let observer = (claimant + 1) % nodes.len();
    let mut shown = false;
    for round in 0..100 {
        let finalized = nodes[observer]
            .casper
            .last_finalized_block()
            .await
            .expect("last finalized block");
        if state_of(&nodes[observer], &finalized)
            .await
            .claimed
            .iter()
            .any(|claimed| claimed.validator == claimant_key && claimed.total == 10)
        {
            shown = true;
            break;
        }
        let index = round % nodes.len();
        let block = propose(&mut nodes[index], &[], &[]).await;
        admit_everywhere(&mut nodes, &block).await;
    }
    assert!(shown, "the finalized floor shows the claim paid");

    let own = identity(&nodes[observer]);
    let before = nodes[observer].casper.bucket_view().proposal(&own);
    assert!(
        before.overflow.contains_key(&claimant_key),
        "the view tracks the claimant"
    );
    let restarted = super::support::restart(&nodes[observer]).await;
    assert_eq!(
        restarted.bucket_view().proposal(&own),
        before,
        "the rebuilt view takes the payment off as the view it replaces did"
    );
}

/// The targets of the ejection claims `block` opened, in identity order.
async fn opened_in(node: &TestNode, block: &BlockMessage) -> Vec<Validator> {
    let mut opened: Vec<Validator> = state_of(node, block)
        .await
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

/// The ejection requests `block` carries.
fn ejection_deploys(
    block: &BlockMessage,
) -> Vec<&models::rust::casper::protocol::casper_message::ProcessedDeploy> {
    block
        .body
        .deploys
        .iter()
        .filter(|deploy| deploy.deploy.data.term.contains("\"ejectValidator\""))
        .collect()
}

/// A validator asks to eject two targets one request at a time, each an
/// ordinary deploy paid at the chain's minimum price; every node replays
/// them, and a block merging the second as a secondary parent keeps both
/// claims.
#[tokio::test]
async fn paid_ejections_replay_on_every_node_and_survive_a_merge() {
    let (mut nodes, lightest, _) =
        funded_network_with(|proof_of_stake| proof_of_stake.min_phlo_price = 1).await;
    let stake_of = |node: &TestNode| {
        nodes[0]
            .casper
            .approved_block
            .body
            .state
            .bonds
            .iter()
            .find(|bond| bond.validator == identity(node))
            .expect("bonded")
            .stake
    };
    let mut by_stake: Vec<usize> = (0..nodes.len()).collect();
    by_stake.sort_by_key(|i| stake_of(&nodes[*i]));
    let (accuser, heavy) = (lightest, by_stake[by_stake.len() - 1]);
    let targets: Vec<Validator> = (0..nodes.len())
        .filter(|i| *i != accuser && *i != heavy)
        .map(|i| identity(&nodes[i]))
        .collect();
    assert_eq!(targets.len(), 2);

    let mut expected = targets.clone();
    expected.sort();
    let first = propose(&mut nodes[accuser], &targets, &[]).await;
    admit_everywhere(&mut nodes, &first).await;
    let accusing = propose(&mut nodes[accuser], &targets, &[]).await;
    for (block, target) in [(&first, &expected[0]), (&accusing, &expected[1])] {
        assert_eq!(
            opened_in(&nodes[accuser], block).await,
            vec![target.clone()],
            "each block opens the next target"
        );
        let requests = ejection_deploys(block);
        assert_eq!(requests.len(), 1, "one request at a time");
        assert_eq!(requests[0].deploy.pk.bytes, identity(&nodes[accuser]));
        assert_eq!(
            requests[0].deploy.data.phlo_price, 1,
            "at the chain's minimum price"
        );
        assert!(
            !requests[0].is_failed && requests[0].cost.cost > 0,
            "the request runs and is paid for"
        );
    }
    let sibling = propose(&mut nodes[heavy], &[], &[]).await;
    admit_everywhere(&mut nodes, &accusing).await;
    admit_everywhere(&mut nodes, &sibling).await;

    let child_proposer = (0..nodes.len())
        .find(|i| *i != accuser && *i != heavy)
        .expect("a third node");
    let child = propose(&mut nodes[child_proposer], &[], &[]).await;
    assert_eq!(
        child.header.parents_hash_list.first(),
        Some(&sibling.block_hash),
        "the heavier sibling is the main parent"
    );
    assert!(child
        .header
        .parents_hash_list
        .contains(&accusing.block_hash));
    admit_everywhere(&mut nodes, &child).await;
    let state = state_of(&nodes[0], &child).await;
    for target in &targets {
        assert!(
            state
                .claims
                .iter()
                .any(|record| record.claim.family == ClaimFamily::Eject
                    && record.claim.target == *target),
            "both claims survive the merge"
        );
    }
}

/// A vault that covers one opening, asked to eject two validators, opens one
/// claim rather than splitting itself between two requests that both run
/// out.
#[tokio::test]
async fn a_vault_that_covers_one_opening_opens_a_claim() {
    let (mut nodes, accuser, _) =
        funded_network_with(|proof_of_stake| proof_of_stake.min_phlo_price = 1).await;
    let targets: Vec<Validator> = (0..nodes.len())
        .filter(|i| *i != accuser)
        .take(2)
        .map(|i| identity(&nodes[i]))
        .collect();
    let measuring = propose(&mut nodes[accuser], &targets[..1], &[]).await;
    let cost = ejection_deploys(&measuring)[0].cost.cost as u128;

    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    parameters.2.proof_of_stake.min_phlo_price = 1;
    super::support::fund_validators(&mut parameters.2);
    let lightest = parameters
        .2
        .proof_of_stake
        .validators
        .iter()
        .min_by_key(|validator| validator.stake)
        .expect("validators")
        .pk
        .clone();
    let vault =
        rholang::rust::interpreter::util::vault_address::VaultAddress::from_public_key(&lightest)
            .expect("vault");
    for funded in parameters.2.vaults.iter_mut() {
        if funded.vault_address == vault {
            funded.initial_balance = cost * 3 / 2;
        }
    }
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let mut nodes = TestNode::create_network(genesis, 4, Some(0.0), None, None)
        .await
        .expect("network");
    let accuser = nodes
        .iter()
        .position(|node| identity(node).as_ref() == lightest.bytes.as_ref())
        .expect("the lightest validator runs a node");
    let targets: Vec<Validator> = (0..nodes.len())
        .filter(|i| *i != accuser)
        .take(2)
        .map(|i| identity(&nodes[i]))
        .collect();
    let accusing = propose(&mut nodes[accuser], &targets, &[]).await;
    let requests = ejection_deploys(&accusing);
    assert_eq!(requests.len(), 1, "one request at a time");
    assert!(!requests[0].is_failed, "the request is funded");
    assert_eq!(opened_in(&nodes[accuser], &accusing).await.len(), 1);
}

/// A validator whose vault cannot pay asks for no ejection.
#[tokio::test]
async fn a_validator_that_cannot_pay_requests_no_ejection() {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    parameters.2.proof_of_stake.min_phlo_price = 1;
    let genesis = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .expect("genesis");
    let mut nodes = TestNode::create_network(genesis, 4, Some(0.0), None, None)
        .await
        .expect("network");
    let target = identity(&nodes[1]);
    let offering = propose(&mut nodes[0], std::slice::from_ref(&target), &[]).await;
    assert!(ejection_deploys(&offering).is_empty());
    assert!(
        opened_in(&nodes[0], &offering).await.is_empty(),
        "an unfunded validator opens nothing"
    );
    admit_everywhere(&mut nodes, &offering).await;
}

/// Holds `first` and its sibling `second` on `node` as an equivocation
/// verdict leaves them, so the next block `node` proposes slashes their
/// sender.
fn record_equivocation(node: &TestNode, first: &BlockMessage, second: &BlockMessage) {
    use block_storage::rust::dag::block_dag_key_value_storage::InsertMode;
    for block in [first, second] {
        node.block_store
            .put_block_message(block)
            .expect("store evidence block");
    }
    if !node.casper.dag_contains(&first.block_hash) {
        node.casper
            .block_dag_storage
            .insert(first, InsertMode::Normal)
            .expect("hold the first block");
    }
    node.casper
        .block_dag_storage
        .insert(second, InsertMode::Equivocation {
            sibling: first.block_hash.clone(),
        })
        .expect("hold the sibling as an equivocation");
}

/// A block that must slash an ejected validator in the block its grace expires
/// slashes it and expires nothing: the slash closes the ejection, and the
/// block can still be built and judged valid.
#[tokio::test]
async fn a_block_slashing_an_ejected_validator_as_its_grace_ends_is_built_and_valid() {
    let (mut nodes, target, _) = funded_network_with(|proof_of_stake| {
        proof_of_stake.epoch_length = 10;
        for validator in proof_of_stake.validators.iter_mut() {
            validator.stake = 100;
        }
    })
    .await;
    eject(&mut nodes, target).await;
    let target_key: Validator = identity(&nodes[target]);

    let due = propose_until_unadmitted(&mut nodes, Some(target), |data| {
        matches!(data, SystemDeployData::ResolveClaims { grace_expiries, .. }
            if grace_expiries.contains(&target_key))
    })
    .await;
    let proposer = nodes
        .iter()
        .position(|node| identity(node) == due.sender)
        .expect("the proposer runs a node");
    let first = propose(&mut nodes[target], &[], &[]).await;
    let mut sibling = first.clone();
    sibling.extra_bytes = prost::bytes::Bytes::from_static(b"sibling");
    let sibling = nodes[target]
        .validator_id_opt
        .as_ref()
        .expect("the target is a validator")
        .sign_block(&sibling);
    for node in &nodes {
        record_equivocation(node, &first, &sibling);
    }

    let slashing = propose(&mut nodes[proposer], &[], &[]).await;
    let offenders: Vec<&prost::bytes::Bytes> = slashing
        .body
        .system_deploys
        .iter()
        .filter_map(|deploy| match deploy {
            ProcessedSystemDeploy::Succeeded {
                system_deploy: SystemDeployData::Slash { evidence, .. },
                ..
            } => Some(&evidence.first.sender),
            _ => None,
        })
        .collect();
    assert_eq!(offenders, vec![&target_key], "the block slashes the target");
    assert!(
        !matches!(resolutions_of(&slashing), Some(SystemDeployData::ResolveClaims { grace_expiries, .. })
            if grace_expiries.contains(&target_key)),
        "the slash leaves no grace to expire"
    );
    admit_everywhere(&mut nodes, &slashing).await;
    let state = state_of(&nodes[0], &slashing).await;
    assert!(
        !state.ejected.contains(&target_key),
        "the ejection is closed"
    );
    assert!(
        state
            .bonds
            .iter()
            .any(|bond| bond.validator == target_key && bond.stake == 0),
        "the target is slashed"
    );
}

/// Yeas cast in a claim's last blocks become visible only at a floor past its
/// deadline. They still count, so the claim passes rather than expiring.
#[tokio::test]
async fn yeas_cast_late_in_a_claims_lifetime_still_count() {
    use std::sync::Arc;

    use casper::rust::economics::bucket_view::LeakyBucketDecision;

    let (mut nodes, claimant, shard_id) = funded_network(Some(10)).await;
    let claimant_key = identity(&nodes[claimant]);
    let requesting = call_pos(
        &mut nodes[claimant],
        r#""claimOverflow", *deployerId, 1"#,
        &shard_id,
    )
    .await;
    admit_everywhere(&mut nodes, &requesting).await;
    let deadline = requesting.body.state.block_number + 10;
    let yea = |node: &TestNode| -> LeakyBucketDecision {
        let view = node.casper.bucket_view();
        let own = identity(node);
        let key = claimant_key.clone();
        Arc::new(move |knowledge| {
            let mut decided = view.proposal_at(&own, knowledge);
            decided.overflow.insert(key.clone(), u128::MAX / 4);
            decided
        })
    };
    let stake_of = |node: &TestNode| {
        nodes[0]
            .casper
            .approved_block
            .body
            .state
            .bonds
            .iter()
            .find(|bond| bond.validator == identity(node))
            .expect("bonded")
            .stake
    };
    // The heaviest two of the rest cast the yeas that cross, in the
    // claim's last two blocks.
    let mut others: Vec<usize> = (0..nodes.len()).filter(|i| *i != claimant).collect();
    others.sort_by_key(|i| -stake_of(&nodes[*i]));
    let late = [others[0], others[1]];

    let mut order: Vec<usize> = Vec::new();
    let mut number = requesting.body.state.block_number;
    let mut decided = None;
    for round in 0..60 {
        let index = if number + 1 < deadline - 2 {
            claimant
        } else if number + 1 < deadline {
            late[(number + 1 - (deadline - 2)) as usize]
        } else {
            round % nodes.len()
        };
        let decision = yea(&nodes[index]);
        let block = super::support::create(&mut nodes[index], decision).await;
        number = block.body.state.block_number;
        order.push(index);
        if let Some(SystemDeployData::ResolveClaims {
            resolutions,
            expiries,
            ..
        }) = resolutions_of(&block)
        {
            let resolved = resolutions
                .iter()
                .find(|r| r.claim.target == claimant_key)
                .map(|r| r.outcome);
            let expired = expiries.iter().any(|claim| claim.target == claimant_key);
            if resolved.is_some() || expired {
                decided = Some((resolved, expired));
                admit_everywhere(&mut nodes, &block).await;
                break;
            }
        }
        admit_everywhere(&mut nodes, &block).await;
    }
    assert!(
        order.iter().filter(|index| late.contains(index)).count() >= 2,
        "the heaviest two proposed in the claim's last blocks"
    );
    assert_eq!(
        decided,
        Some((Some(ClaimOutcome::Pass), false)),
        "the late yeas carry the claim"
    );
}

fn overflow_at(node: &TestNode, validator: &Validator) -> u128 {
    node.casper
        .bucket_view()
        .proposal(&identity(node))
        .overflow
        .get(validator)
        .copied()
        .expect("every node tracks the validator")
}

/// Proposes round-robin from every node but `skip` until `done` holds.
async fn propose_while(
    nodes: &mut [TestNode],
    skip: Option<usize>,
    done: impl Fn(&[TestNode]) -> bool,
) {
    for round in 0..200 {
        if done(nodes) {
            return;
        }
        let index = round % nodes.len();
        if Some(index) == skip {
            continue;
        }
        let block = propose(&mut nodes[index], &[], &[]).await;
        admit_everywhere(nodes, &block).await;
    }
    panic!("the condition never held");
}

/// A topup past the shortfall is credited as overflow in every
/// view, and the validator claims it back. The committee backs the claim by
/// its views, the held topup pays it, and every view is debited by it once.
#[tokio::test]
async fn an_over_topped_rejoin_is_claimed_back() {
    use casper::rust::economics::reward::PPM;

    let (mut nodes, target, shard_id) = funded_network(None).await;
    eject(&mut nodes, target).await;
    let target_key: Validator = identity(&nodes[target]);
    rejoin(&mut nodes, target, 100, &shard_id).await;
    propose_until(&mut nodes, None, |data| {
        matches!(data, SystemDeployData::ResolveClaims { resolutions, .. }
            if resolutions.iter().any(|r| r.claim.family == ClaimFamily::Rejoin
                && r.claim.target == target_key && r.outcome == ClaimOutcome::Pass))
    })
    .await;
    let claim = 10u128 * PPM as u128;
    propose_while(&mut nodes, None, |nodes| {
        nodes
            .iter()
            .all(|node| overflow_at(node, &target_key) >= claim)
    })
    .await;

    let claiming = call_pos(
        &mut nodes[target],
        r#""claimOverflow", *deployerId, 10"#,
        &shard_id,
    )
    .await;
    admit_everywhere(&mut nodes, &claiming).await;
    let opened = state_of(&nodes[0], &claiming).await;
    let record = opened
        .claims
        .iter()
        .find(|record| {
            record.claim.family == ClaimFamily::Overflow && record.claim.target == target_key
        })
        .expect("the claim opens in the block that carries it");
    assert_eq!(record.claim.opened_at, claiming.body.state.block_number);
    let mut electorate = record.electorate.clone();
    electorate.sort_by(|a, b| a.validator.cmp(&b.validator));
    let mut committee: Vec<_> = claiming
        .body
        .state
        .bonds
        .iter()
        .filter(|bond| bond.stake > 0 && !opened.ejected.contains(&bond.validator))
        .cloned()
        .collect();
    committee.sort_by(|a, b| a.validator.cmp(&b.validator));
    assert_eq!(
        electorate, committee,
        "its electorate is that block's committee, less the ejected"
    );

    let resolving = propose_until(&mut nodes, Some(target), |data| {
        matches!(data, SystemDeployData::ResolveClaims { resolutions, .. }
            if resolutions.iter().any(|r| r.claim.family == ClaimFamily::Overflow
                && r.claim.target == target_key && r.outcome == ClaimOutcome::Pass))
    })
    .await;
    let state = state_of(&nodes[0], &resolving).await;
    assert!(
        state
            .claimed
            .iter()
            .any(|claimed| claimed.validator == target_key && claimed.total == 10),
        "the claim is paid"
    );

    // The claimant proposes no more, so nothing else moves its overflow.
    let before = overflow_at(&nodes[0], &target_key);
    propose_while(&mut nodes, Some(target), |nodes| {
        overflow_at(&nodes[0], &target_key) != before
    })
    .await;
    assert_eq!(
        overflow_at(&nodes[0], &target_key),
        before - claim,
        "the view is debited by the claim once its floor shows it paid"
    );
}

/// A withdrawing validator's floor can drop it from its own block's committee
/// while its main parent's committee still seats it, so it may still propose.
/// That block holds back its waiting ejection request, which the contract
/// would refuse there and charge for.
#[tokio::test]
async fn a_proposer_outside_its_own_committee_holds_back_its_ejection_request() {
    let (mut nodes, _, shard_id) = funded_network(Some(10)).await;
    let size = nodes.len();
    let (leaving, target) = (0usize, 1usize);
    let leaving_key = identity(&nodes[leaving]);
    let target_key = identity(&nodes[target]);
    let withdrawing = call_pos(&mut nodes[leaving], r#""withdraw", *deployerId"#, &shard_id).await;
    admit_everywhere(&mut nodes, &withdrawing).await;

    for round in 0..60 {
        let block = propose(&mut nodes[1 + round % (size - 1)], &[], &[]).await;
        admit_everywhere(&mut nodes, &block).await;

        super::support::request_ejections(&nodes[leaving], std::slice::from_ref(&target_key)).await;
        let snapshot = nodes[leaving]
            .casper
            .get_snapshot()
            .await
            .expect("snapshot");
        assert!(
            snapshot
                .on_chain_state
                .bonds_map
                .get(&leaving_key)
                .is_some_and(|stake| *stake > 0),
            "the main parent dropped the validator before its own committee did"
        );
        let decision = nodes[leaving]
            .casper
            .bucket_view()
            .decision(leaving_key.clone());
        let candidate = super::support::create(&mut nodes[leaving], decision).await;
        let seated = candidate
            .body
            .state
            .bonds
            .iter()
            .any(|bond| bond.validator == leaving_key && bond.stake > 0);
        if seated {
            continue;
        }

        assert!(
            ejection_deploys(&candidate).is_empty(),
            "the request is held back"
        );
        let pooled = nodes[leaving]
            .deploy_storage
            .read_all()
            .expect("pool")
            .into_iter()
            .filter(|deploy| deploy.data.term.contains("\"ejectValidator\""))
            .count();
        assert_eq!(pooled, 1, "and still waits in the pool");
        let verdict = nodes[2].add_block(candidate).await.expect("process");
        assert!(
            matches!(verdict, Either::Right(_)),
            "the block is valid: {verdict:?}"
        );
        return;
    }
    panic!("the validator's own committee never dropped it");
}
