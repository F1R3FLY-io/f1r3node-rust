use casper::rust::casper::Casper;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, ClaimFamily, ClaimOutcome, ProcessedSystemDeploy, SystemDeployData,
};
use models::rust::validator::Validator;

use crate::economics::support::{admit_everywhere, identity, propose_by_own_view};
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

fn resolve_claims(block: &BlockMessage) -> Option<&SystemDeployData> {
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

/// A 4-validator shard in which one validator
/// never proposes. The others drive everything from their own views, as a
/// running proposer does: the idle validator drains, is offered, voted out,
/// and, never topping up, slashed when its grace ends.
///
/// An epoch of 20 makes the claim deadline 20 and grace 40, and the shipped
/// emission over it is a base reward far above what an honest proposer's
/// bucket drains between its blocks (the parameter note). The idle validator
/// is the lightest, so the rest hold the two thirds an ejection needs.
#[tokio::test]
async fn an_idle_validator_is_ejected_and_slashed_by_the_committee_alone() {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    parameters.2.proof_of_stake.epoch_length = 20;
    crate::economics::support::fund_validators(&mut parameters.2);
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
    let idle = nodes
        .iter()
        .position(|node| identity(node) == lightest)
        .expect("the lightest validator runs a node");
    let idle_key: Validator = identity(&nodes[idle]);
    let honest: Vec<usize> = (0..nodes.len()).filter(|i| *i != idle).collect();

    // One block per round: the idle bucket drains from full to the threshold,
    // stays below it for the observation window, then the claim runs to its
    // deadline and the grace to its end, with an epoch to spare.
    let params = *nodes[0].casper.bucket_view().params();
    let chain = nodes[0].casper.chain_parameters;
    let drain = (1_000_000 - params.theta_eject_ppm).div_ceil(params.lambda_ppm) as i64;
    let rounds = drain
        + params.eject_observation_blocks as i64
        + params.claim_deadline_blocks
        + chain.window_basis().ejection_grace_blocks()
        + chain.epoch_length as i64;

    let mut ejected_at = None;
    let mut expiring = None;
    for round in 0..rounds as usize {
        let proposer = honest[round % honest.len()];
        let block = propose_by_own_view(&mut nodes[proposer]).await;
        admit_everywhere(&mut nodes, &block).await;
        if let Some(SystemDeployData::ResolveClaims {
            resolutions,
            grace_expiries,
            ..
        }) = resolve_claims(&block)
        {
            if resolutions.iter().any(|r| {
                r.claim.family == ClaimFamily::Eject
                    && r.claim.target == idle_key
                    && r.outcome == ClaimOutcome::Pass
            }) {
                ejected_at = Some(block.body.state.block_number);
            }
            if grace_expiries.contains(&idle_key) {
                expiring = Some(block);
                break;
            }
        }
        for &other in &honest {
            assert!(
                !nodes[other]
                    .casper
                    .bucket_view()
                    .proposal(&identity(&nodes[other]))
                    .drained
                    .iter()
                    .any(|drained| *drained != idle_key),
                "an honest proposer drained in round {round}"
            );
        }
    }

    let ejected_at = ejected_at.expect("the idle validator is ejected");
    let expiring = expiring.expect("its grace ends");
    assert!(expiring.body.state.block_number > ejected_at);
    let state = nodes[0]
        .runtime_manager
        .floor_state(&expiring.body.state.post_state_hash)
        .await
        .expect("post-state read");
    assert!(
        state
            .bonds
            .iter()
            .any(|bond| bond.validator == idle_key && bond.stake == 0),
        "the idle validator is slashed"
    );
    assert!(!state.ejected.contains(&idle_key));
}
