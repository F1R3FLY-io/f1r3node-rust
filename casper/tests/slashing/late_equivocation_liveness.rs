// An honest validator that learns of an equivocation after the chain has built
// on the equivocator's block must still be able to propose valid blocks: with
// the slash while the evidence's epoch is current, without it once the epoch
// has passed.

use casper::rust::block_status::ValidBlock;
use casper::rust::util::{construct_deploy, proto_util};
use crypto::rust::signatures::signed::Signed;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, DeployData, ProcessedSystemDeploy, SystemDeployData,
};
use rspace_plus_plus::rspace::history::Either;

use super::integration_helpers::equivocate_block;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

struct Proposal {
    block: BlockMessage,
    equivocator: Vec<u8>,
    sibling_hash: Vec<u8>,
}

impl Proposal {
    fn slashes_equivocation(&self) -> bool {
        self.block.body.system_deploys.iter().any(|deploy| {
            matches!(
                deploy,
                ProcessedSystemDeploy::Succeeded {
                    system_deploy: SystemDeployData::Slash { evidence, .. },
                    ..
                } if [&evidence.first, &evidence.second].into_iter().any(|commitment| {
                    proto_util::commitment_hash(commitment).as_ref() == self.sibling_hash.as_slice()
                })
            )
        })
    }

    fn evidence(&self) -> String {
        let cited = self
            .block
            .justifications
            .iter()
            .find(|justification| justification.validator.as_ref() == self.equivocator.as_slice())
            .map(|justification| {
                justification.latest_block_hash.as_ref() == self.sibling_hash.as_slice()
            });
        let stake = self
            .block
            .body
            .state
            .bonds
            .iter()
            .find(|bond| bond.validator.as_ref() == self.equivocator.as_slice())
            .map(|bond| bond.stake.clone());
        format!(
            "block #{}: cites the sibling as the equivocator's justification: {cited:?}; \
             equivocator's stake in the bonds field: {stake:?}; carries the slash: {}",
            self.block.body.state.block_number,
            self.slashes_equivocation()
        )
    }
}

fn deploy(nonce: i32, shard_id: &str) -> Signed<DeployData> {
    construct_deploy::basic_deploy_data(nonce, None, Some(shard_id.to_string())).expect("deploy")
}

/// v0 signs b1 and an equivocating sibling. v1 accepts b1, builds
/// `blocks_before_release` blocks on it, then receives the sibling.
struct LateEquivocation {
    nodes: Vec<TestNode>,
    shard_id: String,
    equivocator: Vec<u8>,
    sibling_hash: Vec<u8>,
    /// Every block v1 has received or produced, in the order it saw them.
    seen_by_v1: Vec<BlockMessage>,
}

impl LateEquivocation {
    async fn new(epoch_length: i32, blocks_before_release: i32) -> Self {
        let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(3));
        parameters.2.proof_of_stake.epoch_length = epoch_length;
        let genesis = GenesisBuilder::new()
            .build_genesis_with_parameters(Some(parameters))
            .await
            .expect("genesis");
        let shard_id = genesis.genesis_block.shard_id.clone();
        let mut nodes = TestNode::create_network(genesis, 3, None, None, None, None)
            .await
            .expect("network");

        let b1 = nodes[0]
            .create_block_unsafe(&[deploy(0, &shard_id)])
            .await
            .expect("b1");
        let sibling = equivocate_block(&mut nodes[0], &b1, vec![deploy(1, &shard_id)])
            .await
            .expect("sibling");

        let accepted = nodes[1]
            .process_block(b1.clone())
            .await
            .expect("process b1");
        assert!(matches!(accepted, Either::Right(_)), "b1: {accepted:?}");
        let mut seen_by_v1 = vec![b1.clone()];
        for nonce in 0..blocks_before_release {
            seen_by_v1.push(
                nodes[1]
                    .add_block_from_deploys(&[deploy(10 + nonce, &shard_id)])
                    .await
                    .expect("v1 builds on b1"),
            );
        }
        let recorded = nodes[1]
            .process_block(sibling.clone())
            .await
            .expect("process the sibling");
        assert!(
            matches!(recorded, Either::Left(_)),
            "the sibling is an equivocation: {recorded:?}"
        );
        seen_by_v1.push(sibling.clone());

        Self {
            nodes,
            shard_id,
            equivocator: b1.sender.to_vec(),
            sibling_hash: sibling.block_hash.to_vec(),
            seen_by_v1,
        }
    }

    /// v1 proposes; returns the block and the verdict its own pipeline gives it.
    async fn v1_proposes(&mut self, nonce: i32) -> (Proposal, casper::rust::ValidBlockProcessing) {
        let block = self.nodes[1]
            .create_block_unsafe(&[deploy(nonce, &self.shard_id)])
            .await
            .expect("v1 proposes");
        let verdict = self.nodes[1]
            .process_block(block.clone())
            .await
            .expect("v1 validates its proposal");
        self.seen_by_v1.push(block.clone());
        (self.proposal(block), verdict)
    }

    /// v2 receives what v1 saw, in v1's order; returns v2's verdict on the last.
    async fn v2_receives_what_v1_saw(&mut self) -> casper::rust::ValidBlockProcessing {
        let (last, earlier) = self.seen_by_v1.split_last().expect("v1 saw blocks");
        for block in earlier {
            self.nodes[2]
                .process_block(block.clone())
                .await
                .expect("v2 processes a block v1 saw");
        }
        self.nodes[2]
            .process_block(last.clone())
            .await
            .expect("v2 processes v1's latest block")
    }

    fn proposal(&self, block: BlockMessage) -> Proposal {
        Proposal {
            block,
            equivocator: self.equivocator.clone(),
            sibling_hash: self.sibling_hash.clone(),
        }
    }
}

fn assert_valid(verdict: &casper::rust::ValidBlockProcessing, proposal: &Proposal, whose: &str) {
    assert!(
        matches!(verdict, Either::Right(ValidBlock::Valid)),
        "{whose} rejects v1's block: {verdict:?}; {}",
        proposal.evidence()
    );
}

#[serial_test::serial]
#[tokio::test]
async fn honest_validator_slashes_an_equivocation_learned_late_in_its_epoch() {
    let mut scenario = LateEquivocation::new(1000, 3).await;
    let (proposal, verdict) = scenario.v1_proposes(100).await;
    assert_valid(&verdict, &proposal, "v1");
    assert!(proposal.slashes_equivocation(), "{}", proposal.evidence());
}

#[serial_test::serial]
#[tokio::test]
async fn honest_validator_slashes_an_equivocation_from_a_past_epoch() {
    let mut scenario = LateEquivocation::new(4, 6).await;
    let (proposal, verdict) = scenario.v1_proposes(100).await;
    assert_valid(&verdict, &proposal, "v1");
    assert!(proposal.slashes_equivocation(), "{}", proposal.evidence());
    let verdict = scenario.v2_receives_what_v1_saw().await;
    assert_valid(&verdict, &proposal, "v2");
}

#[serial_test::serial]
#[tokio::test]
async fn another_honest_validator_accepts_the_late_slash() {
    let mut scenario = LateEquivocation::new(1000, 3).await;
    let (proposal, _) = scenario.v1_proposes(100).await;
    assert!(proposal.slashes_equivocation(), "{}", proposal.evidence());
    let verdict = scenario.v2_receives_what_v1_saw().await;
    assert_valid(&verdict, &proposal, "v2");
}

#[serial_test::serial]
#[tokio::test]
async fn the_block_after_the_slash_is_valid_everywhere() {
    let mut scenario = LateEquivocation::new(1000, 3).await;
    let (slashing, verdict) = scenario.v1_proposes(100).await;
    assert_valid(&verdict, &slashing, "v1");
    let (next, verdict) = scenario.v1_proposes(101).await;
    assert_valid(&verdict, &next, "v1");
    let verdict = scenario.v2_receives_what_v1_saw().await;
    assert_valid(&verdict, &next, "v2");
    assert!(!next.slashes_equivocation(), "{}", next.evidence());
}
