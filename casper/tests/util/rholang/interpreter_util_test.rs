// See casper/src/test/scala/coop/rchain/casper/util/rholang/InterpreterUtilTest.scala

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// Changed by DR-134 (DR-66 item 1): the offered block step inserts its block.
// use block_storage::rust::dag::block_dag_key_value_storage::KeyValueDagRepresentation;
use block_storage::rust::dag::block_dag_key_value_storage::{
    InsertMode, KeyValueDagRepresentation,
};
// Added by DR-134 (DR-66 item 1): offered checkpoints take pending candidates.
use block_storage::rust::deploy::key_value_deploy_storage::PendingDeployCandidate;
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
// Added by DR-134 (DR-66 item 1): the offered block step takes the indexed DAG.
use block_storage::rust::test::indexed_block_dag_storage::IndexedBlockDagStorage;
use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
use casper::rust::errors::CasperError;
use casper::rust::util::rholang::interpreter_util;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum;
use casper::rust::util::{construct_deploy, proto_util, rspace_util};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::signed::Signed;
use dashmap::DashSet;
use models::rhoapi::PCost;
use models::rust::block::state_hash::StateHash;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::{BlockMessage, DeployData, ProcessedDeploy};
use rholang::rust::interpreter::system_processes::BlockData;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::block_dag_storage_fixture::{with_genesis, with_storage};
use crate::helper::block_generator;
// Added by DR-134 (DR-66 item 1): offered deploys. The legacy builder stays
// for the tests that need gap G2, the legacy-limit tests and the synthetic
// fixtures.
use crate::helper::offered_deploy;
use crate::helper::offered_deploy::OfferedDeploy;
use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};
use crate::util::rholang::resources;

// Note: In Scala, genesisContext is defined at class level. In Rust, each test creates its own genesis context
struct TestContext {
    genesis_context: GenesisContext,
}

impl TestContext {
    async fn new() -> Self {
        let mut genesis_builder = GenesisBuilder::new();
        let genesis_parameters_tuple =
            GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
        let genesis_context = genesis_builder
            .build_genesis_with_parameters(Some(genesis_parameters_tuple))
            .await
            .expect("Failed to build genesis context");

        Self { genesis_context }
    }

    // Build a genesis context bonded to exactly `validators_num` validators.
    // Tests that assert an exact multi-parent set need a full network (every
    // bonded validator proposes), otherwise the merge block also carries the
    // genesis block as a parent for each bonded-but-non-proposing validator.
    async fn new_with_validators(validators_num: usize) -> Self {
        let mut genesis_builder = GenesisBuilder::new();
        let genesis_parameters_tuple =
            GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(validators_num));
        let genesis_context = genesis_builder
            .build_genesis_with_parameters(Some(genesis_parameters_tuple))
            .await
            .expect("Failed to build genesis context");

        Self { genesis_context }
    }

    // Disabled by DR-134 (DR-65, gap G2): the tests of several deploys in one
    // block now build offers with `create_offered_deploys`,
    // `create_offered_deploys_now` and `create_offered_deploys_now_with_shard`
    // below. Protocol 6 refuses the body-only deploys that these three legacy
    // builders sign, and no other test calls them.
    // // Helper function to create deploys from Rholang source code with timestamp and shard_id
    // // Note: Used in tests that need to create blocks with specific timestamps (e.g., Test #1)
    // // This wraps ConstructDeploy.sourceDeploy(..., timestamp, ..., shardId)
    // fn create_deploys(
    //     sources: Vec<&str>,
    //     timestamp: i64,
    //     shard_id: String,
    // ) -> Vec<Signed<DeployData>> {
    //     sources
    //         .into_iter()
    //         .map(|source| {
    //             construct_deploy::source_deploy(
    //                 source.to_string(),
    //                 timestamp,
    //                 None,
    //                 None,
    //                 Some(construct_deploy::DEFAULT_SEC.clone()),
    //                 None,
    //                 Some(shard_id.clone()),
    //             )
    //             .unwrap()
    //         })
    //         .collect()
    // }
    //
    // // Helper function to create deploys from Rholang source code without timestamp
    // // Note: Wraps ConstructDeploy.sourceDeployNow(source, sec) where sec defaults to DEFAULT_SEC if None
    // // Scala: def sourceDeployNow(source: String, sec: PrivateKey = defaultSec, ...)
    // fn create_deploys_now(sources: Vec<&str>, sec: Option<PrivateKey>) -> Vec<Signed<DeployData>> {
    //     sources
    //         .into_iter()
    //         .map(|source| {
    //             construct_deploy::source_deploy_now(
    //                 source.to_string(),
    //                 Some(
    //                     sec.clone()
    //                         .unwrap_or_else(|| construct_deploy::DEFAULT_SEC.clone()),
    //                 ),
    //                 None,
    //                 None,
    //             )
    //             .unwrap()
    //         })
    //         .collect()
    // }
    //
    // // Like `create_deploys_now`, but threads the genesis shard identifier
    // // through to each deploy. Required by tests that submit the deploys to
    // // the full block-production/validation path (`add_block_from_deploys`),
    // // which rejects blocks whose deploys do not carry the shard's root
    // // identifier (`Validate::shard_identifier`, InvalidShardId). The
    // // shard-less `create_deploys_now` is fine only for tests that feed the
    // // interpreter checkpoint directly (which does not run shard validation).
    // fn create_deploys_now_with_shard(
    //     sources: Vec<&str>,
    //     sec: Option<PrivateKey>,
    //     shard_id: String,
    // ) -> Vec<Signed<DeployData>> {
    //     sources
    //         .into_iter()
    //         .map(|source| {
    //             construct_deploy::source_deploy_now(
    //                 source.to_string(),
    //                 Some(
    //                     sec.clone()
    //                         .unwrap_or_else(|| construct_deploy::DEFAULT_SEC.clone()),
    //                 ),
    //                 None,
    //                 Some(shard_id.clone()),
    //             )
    //             .expect("failed to create deploy with shard")
    //         })
    //         .collect()
    // }

    // Added by DR-134 (DR-65, gap G2): `create_deploys` with offers. Every
    // offer of the list takes the given timestamp, as in `create_deploys`, so
    // two offers of one list must differ in their terms: an offer with the
    // signer, the term and the timestamp of another one is the same envelope.
    fn create_offered_deploys(
        sources: Vec<&str>,
        timestamp: i64,
        shard_id: String,
    ) -> Vec<OfferedDeploy> {
        sources
            .into_iter()
            .map(|source| {
                offered_deploy::source_deploy(
                    source.to_string(),
                    timestamp,
                    None,
                    None,
                    Some(offered_deploy::DEFAULT_SEC.clone()),
                    None,
                    Some(shard_id.clone()),
                )
                .expect("failed to create an offered deploy")
            })
            .collect()
    }

    // Added by DR-134 (DR-65, gap G2): `create_deploys_now_with_shard` with
    // offers.
    fn create_offered_deploys_now_with_shard(
        sources: Vec<&str>,
        sec: Option<PrivateKey>,
        shard_id: String,
    ) -> Vec<OfferedDeploy> {
        sources
            .into_iter()
            .map(|source| {
                offered_deploy::source_deploy_now(
                    source.to_string(),
                    Some(
                        sec.clone()
                            .unwrap_or_else(|| offered_deploy::DEFAULT_SEC.clone()),
                    ),
                    None,
                    Some(shard_id.clone()),
                )
                .expect("failed to create an offered deploy with shard")
            })
            .collect()
    }

    fn prepare_deploys(sources: Vec<&str>, cost: PCost) -> Vec<ProcessedDeploy> {
        let genesis_deploys: Vec<Signed<DeployData>> = sources
            .into_iter()
            .map(|source| {
                construct_deploy::source_deploy_now(source.to_string(), None, None, None).unwrap()
            })
            .collect();

        genesis_deploys
            .into_iter()
            .map(|d| ProcessedDeploy {
                deploy: d,
                cost,
                deploy_log: Vec::new(),
                is_failed: false,
                system_deploy_error: None,
            })
            .collect()
    }

    fn mk_casper_snapshot(dag: KeyValueDagRepresentation) -> CasperSnapshot {
        CasperSnapshot {
            dag,
            last_finalized_block: BlockHash::default(),
            parents: Vec::new(),
            justifications: HashSet::new(),
            invalid_blocks: HashMap::new(),
            deploys_in_scope: Arc::new(DashSet::new()),
            rejected_in_scope: Arc::new(DashSet::new()),
            max_block_num: 0,
            max_seq_nums: HashMap::new(),
            on_chain_state: OnChainCasperState {
                shard_conf: CasperShardConf::new(),
                bonds_map: HashMap::new(),
                active_validators: Vec::new(),
            },
        }
    }

    // Added by DR-134 (DR-66 item 1): `create_deploys_now` with offered deploys.
    fn create_offered_deploys_now(
        sources: Vec<&str>,
        sec: Option<PrivateKey>,
    ) -> Vec<OfferedDeploy> {
        sources
            .into_iter()
            .map(|source| {
                offered_deploy::source_deploy_now(
                    source.to_string(),
                    Some(
                        sec.clone()
                            .unwrap_or_else(|| offered_deploy::DEFAULT_SEC.clone()),
                    ),
                    None,
                    None,
                )
                .expect("failed to create an offered deploy")
            })
            .collect()
    }

    // Added by DR-134 (DR-66 item 1): `mk_casper_snapshot` with the shard
    // configuration under which an offered checkpoint adopts the genesis
    // policy again.
    fn mk_offered_casper_snapshot(&self, dag: KeyValueDagRepresentation) -> CasperSnapshot {
        let mut snapshot = Self::mk_casper_snapshot(dag);
        snapshot.on_chain_state.shard_conf =
            resources::offered_shard_conf(&self.genesis_context.genesis_block);
        snapshot
    }

    // Added by DR-134 (DR-66 item 1): `block_generator::step` for a block with
    // one offered deploy. It computes the offered checkpoint of `block` on its
    // parents and stores `block` with the post-state and the processed deploy,
    // as `block_generator::step` does for a legacy block.
    async fn step_offered(
        &self,
        block_dag_storage: &mut IndexedBlockDagStorage,
        block_store: &mut KeyValueBlockStore,
        runtime_manager: &RuntimeManager,
        block: &BlockMessage,
        deploy: OfferedDeploy,
    ) -> Result<(), CasperError> {
        let adopted =
            resources::adopted_policy(runtime_manager, &self.genesis_context.genesis_block).await;
        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let casper_snapshot = self.mk_offered_casper_snapshot(dag);
        let parents = proto_util::get_parents(block_store, block);
        let checkpoint = interpreter_util::compute_deploys_checkpoint_envelopes(
            block_store,
            parents,
            vec![PendingDeployCandidate::Envelope(deploy.envelope)],
            Vec::<SystemDeployEnum>::new(),
            &casper_snapshot,
            runtime_manager,
            Some(&adopted),
            BlockData::from_block(block),
            HashMap::new(),
            None,
            None,
            None,
        )
        .await?;

        let mut updated_block = block.clone();
        updated_block.body.state.post_state_hash = checkpoint.post_state_hash;
        updated_block.body.deploys = checkpoint.deploys;
        block_store.put(block.block_hash.clone(), &updated_block)?;
        block_dag_storage.insert(&updated_block, InsertMode::Normal)?;
        Ok(())
    }

    // Scala: def computeDeployCosts(...): Task[Seq[PCost]] =
    //   for {
    //     computeResult <- computeDeploysCheckpoint[Task](Seq(genesis), deploy, dag, runtimeManager)
    //     Right((_, _, processedDeploys, _, _)) = computeResult
    //   } yield processedDeploys.map(_.cost)
    // Changed by DR-134 (DR-65, gap G2): the deploys are offers, which one
    // offered checkpoint runs as a batch. The cost of an offer is its native
    // cost (its COMMs and its stored bytes).
    // async fn compute_deploy_costs(
    //     &self,
    //     runtime_manager: &mut RuntimeManager,
    //     dag: KeyValueDagRepresentation,
    //     block_store: &mut KeyValueBlockStore,
    //     deploys: Vec<Signed<DeployData>>,
    // ) -> Result<Vec<PCost>, CasperError> {
    async fn compute_deploy_costs(
        &self,
        runtime_manager: &mut RuntimeManager,
        dag: KeyValueDagRepresentation,
        block_store: &mut KeyValueBlockStore,
        deploys: Vec<OfferedDeploy>,
    ) -> Result<Vec<PCost>, CasperError> {
        let genesis = self.genesis_context.genesis_block.clone();
        let result = self
            .compute_deploys_checkpoint(
                block_store,
                vec![genesis],
                deploys,
                dag,
                runtime_manager,
                0,
                0,
            )
            .await?;

        // Scala: yield processedDeploys.map(_.cost)
        // Changed by DR-134 (DR-65, gap G2): the cost of a processed offer.
        // let costs = result.deploys.iter().map(|pd| pd.cost).collect();
        let costs = result.deploys.iter().map(|pd| *pd.cost()).collect();

        Ok(costs)
    }

    // Changed by DR-134 (DR-65, gap G2): the offered checkpoint of a batch of
    // offers, under the adopted policy of the test genesis and the shard
    // configuration under which the checkpoint adopts that policy again.
    // #[allow(clippy::too_many_arguments)]
    // async fn compute_deploys_checkpoint(
    //     &self,
    //     block_store: &mut KeyValueBlockStore,
    //     parents: Vec<BlockMessage>,
    //     deploys: Vec<Signed<DeployData>>,
    //     dag: KeyValueDagRepresentation,
    //     runtime_manager: &mut RuntimeManager,
    //     block_number: i64,
    //     seq_num: i32,
    // ) -> Result<interpreter_util::DeploysCheckpoint, CasperError> {
    #[allow(clippy::too_many_arguments)]
    async fn compute_deploys_checkpoint(
        &self,
        block_store: &mut KeyValueBlockStore,
        parents: Vec<BlockMessage>,
        deploys: Vec<OfferedDeploy>,
        dag: KeyValueDagRepresentation,
        runtime_manager: &mut RuntimeManager,
        block_number: i64,
        seq_num: i32,
    ) -> Result<interpreter_util::DeploysEnvelopeCheckpoint, CasperError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        // Changed by DR-134 (DR-65, gap G2).
        // let casper_snapshot = Self::mk_casper_snapshot(dag);
        let casper_snapshot = self.mk_offered_casper_snapshot(dag);
        // Added by DR-134 (DR-65, gap G2).
        let adopted =
            resources::adopted_policy(runtime_manager, &self.genesis_context.genesis_block).await;

        let block_data = BlockData {
            time_stamp: now,
            block_number,
            sender: self.genesis_context.validator_pks()[0].clone(),
            seq_num,
        };

        // Note: In Scala .attempt wraps result in Either[Throwable, T]
        // In Rust, we return Result which is equivalent
        // Changed by DR-134 (DR-65, gap G2): the offered checkpoint.
        // interpreter_util::compute_deploys_checkpoint(
        //     block_store,
        //     parents,
        //     deploys,
        //     Vec::<SystemDeployEnum>::new(),
        //     &casper_snapshot,
        //     runtime_manager,
        //     block_data,
        //     HashMap::new(),
        //     None,
        //     None,
        //     None,
        // )
        // .await
        interpreter_util::compute_deploys_checkpoint_envelopes(
            block_store,
            parents,
            deploys
                .into_iter()
                .map(|deploy| PendingDeployCandidate::Envelope(deploy.envelope))
                .collect(),
            Vec::<SystemDeployEnum>::new(),
            &casper_snapshot,
            runtime_manager,
            Some(&adopted),
            block_data,
            HashMap::new(),
            None,
            None,
            None,
        )
        .await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_block_checkpoint_should_compute_the_final_post_state_of_a_chain_properly() {
    let time = 0i64;

    let ctx = TestContext::new().await;
    let shard_id = ctx.genesis_context.genesis_block.shard_id.clone();

    let mut node = TestNode::standalone(ctx.genesis_context)
        .await
        .expect("Failed to create standalone node");

    // Changed by DR-134 (DR-65, gap G2): each block carries a batch of offers
    // of one signer (DEFAULT_SEC), as dev's blocks carry deploys of that
    // signer. Every COMM of the chain is between deploys of that signer. Each
    // block keeps dev's timestamp, and the terms of one block differ, so no
    // two offers are the same envelope.
    // let b0_deploys = TestContext::create_deploys(
    //     vec!["@1!(1)", "@2!(2)", "for(@a <- @1){ @123!(5 * a) }"],
    //     time + 1,
    //     shard_id.clone(),
    // );
    //
    // let b1_deploys = TestContext::create_deploys(
    //     vec!["@1!(1)", "for(@a <- @2){ @456!(5 * a) }"],
    //     time + 2,
    //     shard_id.clone(),
    // );
    //
    // let b2_deploys = TestContext::create_deploys(
    //     vec!["for(@a <- @123 & @b <- @456){ @1!(a + b) }"],
    //     time + 3,
    //     shard_id.clone(),
    // );
    //
    // let b3_deploys = TestContext::create_deploys(vec!["@7!(7)"], time + 4, shard_id.clone());
    let b0_deploys = TestContext::create_offered_deploys(
        vec!["@1!(1)", "@2!(2)", "for(@a <- @1){ @123!(5 * a) }"],
        time + 1,
        shard_id.clone(),
    );

    let b1_deploys = TestContext::create_offered_deploys(
        vec!["@1!(1)", "for(@a <- @2){ @456!(5 * a) }"],
        time + 2,
        shard_id.clone(),
    );

    let b2_deploys = TestContext::create_offered_deploys(
        vec!["for(@a <- @123 & @b <- @456){ @1!(a + b) }"],
        time + 3,
        shard_id.clone(),
    );

    let b3_deploys =
        TestContext::create_offered_deploys(vec!["@7!(7)"], time + 4, shard_id.clone());

    /*
     * DAG Looks like this:
     *
     *          b3
     *           |
     *          b2
     *           |
     *          b1
     *           |
     *          b0
     *           |
     *          genesis
     */

    let b0 = node
        .add_block_from_deploys(&b0_deploys)
        .await
        .expect("Failed to add b0");
    let b0_hash = proto_util::post_state_hash(&b0);

    let b1 = node
        .add_block_from_deploys(&b1_deploys)
        .await
        .expect("Failed to add b1");
    let b1_hash = proto_util::post_state_hash(&b1);

    let _b2 = node
        .add_block_from_deploys(&b2_deploys)
        .await
        .expect("Failed to add b2");

    let b3 = node
        .add_block_from_deploys(&b3_deploys)
        .await
        .expect("Failed to add b3");
    let b3_hash = proto_util::post_state_hash(&b3);

    // Verify data at public channels
    let b0_ch2 = rspace_util::get_data_at_public_channel(&b0_hash, 2, &node.runtime_manager).await;
    assert_eq!(b0_ch2, vec!["2"]);

    let b0_ch123 =
        rspace_util::get_data_at_public_channel(&b0_hash, 123, &node.runtime_manager).await;
    assert_eq!(b0_ch123, vec!["5"]);

    let b1_ch1 = rspace_util::get_data_at_public_channel(&b1_hash, 1, &node.runtime_manager).await;
    assert_eq!(b1_ch1, vec!["1"]);

    let b1_ch123 =
        rspace_util::get_data_at_public_channel(&b1_hash, 123, &node.runtime_manager).await;
    assert_eq!(b1_ch123, vec!["5"]);

    let b1_ch456 =
        rspace_util::get_data_at_public_channel(&b1_hash, 456, &node.runtime_manager).await;
    assert_eq!(b1_ch456, vec!["10"]);

    let b3_ch1 = rspace_util::get_data_at_public_channel(&b3_hash, 1, &node.runtime_manager).await;

    assert_eq!(b3_ch1.len(), 2);
    assert!(b3_ch1.contains(&"1".to_string()));
    assert!(b3_ch1.contains(&"15".to_string()));

    let b3_ch7 = rspace_util::get_data_at_public_channel(&b3_hash, 7, &node.runtime_manager).await;
    assert_eq!(b3_ch7, vec!["7"]);
}

// Ported from the Scala `InterpreterUtilTest`, where it was `ignore`d pending
// token-balance merging. It now runs against the Rust multi-parent-merging
// design: b3 merges the histories of its two parents b1 and b2, and the join
// `for(@a <- @123 & @b <- @456)` fires across the merged state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_block_checkpoint_should_merge_histories_in_case_of_multiple_parents() {
    // A full 2-validator network: both validators propose (b1, b2) before b3,
    // so under multi-parent merging b3's parents are exactly {b1, b2}. With the
    // default (4-validator) genesis, the two bonded-but-absent validators would
    // each contribute the genesis block, giving b3 a third parent.
    let ctx = TestContext::new_with_validators(2).await;
    let shard_id = ctx.genesis_context.genesis_block.shard_id.clone();

    // Changed by DR-134 (DR-65, gap G2): each block carries a batch of offers.
    // The signers stay dev's signers: b1 and b2 are siblings, and two sibling
    // offers of one signer share that signer's cost cursors (DR-119), so the
    // merge would keep only one of them. So the join of b3 (DEFAULT_SEC)
    // consumes the datum on @456 that a deploy of DEFAULT_SEC2 produced in b1.
    // That COMM needs the funding of the other signer, which is gap G3 (DR-68).
    // Until G3, the proposer rejects the offer of b3 ("measured funding case
    // has no feasible signed assignment", `family_selection.rs`), and the
    // creation of b3 returns `NoNewDeploys`.
    // Changed by G3 (DR-135): the owner purse of DEFAULT_SEC2 now funds the
    // share of its datum in that COMM, so b3 is created and merges both
    // histories, as on dev.
    // let b1_deploys = TestContext::create_deploys_now_with_shard(
    //     vec!["@5!(5)", "@2!(2)", "for(@a <- @2){ @456!(5 * a) }"],
    //     Some(construct_deploy::DEFAULT_SEC2.clone()),
    //     shard_id.clone(),
    // );
    //
    // let b2_deploys = TestContext::create_deploys_now_with_shard(
    //     vec!["@1!(1)", "for(@a <- @1){ @123!(5 * a) }"],
    //     None, // uses default key
    //     shard_id.clone(),
    // );
    //
    // let b3_deploys = TestContext::create_deploys_now_with_shard(
    //     vec!["for(@a <- @123 & @b <- @456){ @1!(a + b) }"],
    //     None, // uses default key
    //     shard_id.clone(),
    // );
    let b1_deploys = TestContext::create_offered_deploys_now_with_shard(
        vec!["@5!(5)", "@2!(2)", "for(@a <- @2){ @456!(5 * a) }"],
        Some(offered_deploy::DEFAULT_SEC2.clone()),
        shard_id.clone(),
    );

    let b2_deploys = TestContext::create_offered_deploys_now_with_shard(
        vec!["@1!(1)", "for(@a <- @1){ @123!(5 * a) }"],
        None, // uses default key
        shard_id.clone(),
    );

    let b3_deploys = TestContext::create_offered_deploys_now_with_shard(
        vec!["for(@a <- @123 & @b <- @456){ @1!(a + b) }"],
        None, // uses default key
        shard_id.clone(),
    );

    /*
     * DAG Looks like this:
     *
     *        b3
     *        |   \
     *        b1    b2
     *         \    /
     *         genesis
     */

    let mut nodes = TestNode::create_network(ctx.genesis_context, 2, None, None, None, None)
        .await
        .unwrap();

    let b1 = nodes[0].add_block_from_deploys(&b1_deploys).await.unwrap();

    let b2 = TestNode::propagate_block_to_one(&mut nodes, 1, 0, &b2_deploys)
        .await
        .unwrap();

    let b3 = nodes[0].add_block_from_deploys(&b3_deploys).await.unwrap();

    let b3_parents: HashSet<_> = b3.header.parents_hash_list.iter().cloned().collect();
    let expected_parents: HashSet<_> = vec![b1.block_hash.clone(), b2.block_hash.clone()]
        .into_iter()
        .collect();
    assert_eq!(b3_parents, expected_parents);

    let b3_hash = proto_util::post_state_hash(&b3);
    let b3_ch5 =
        rspace_util::get_data_at_public_channel(&b3_hash, 5, &nodes[0].runtime_manager).await;
    assert_eq!(b3_ch5, vec!["5"]);

    let b3_ch1 =
        rspace_util::get_data_at_public_channel(&b3_hash, 1, &nodes[0].runtime_manager).await;
    assert_eq!(b3_ch1, vec!["15"]);
}

const REGISTRY: &str = r#"
new ri(`rho:registry:insertArbitrary`) in {
  new X, Y in {
    ri!(*X, *Y)
  }
}
"#;

// Ported from the Scala `InterpreterUtilTest`, where it was `ignore`d pending
// token-balance merging. It now runs: validating b3, which merges the complex
// registry-insert histories of b1 and b2, yields Right(None) (no post-state
// mismatch).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_block_checkpoint_should_merge_histories_in_case_of_multiple_parents_with_complex_contract(
) {
    let contract = REGISTRY;

    // Changed by DR-134 (DR-66 item 1): b1 and b2 run offered deploys, which the
    // offered step computes. The sibling blocks b1 and b2 use different keys,
    // because two offers of one signer share a cost cursor. The deploys of b3
    // stay a synthetic fixture: the pre-state check returns before b3 runs.
    // let b1_deploys_with_cost = TestContext::prepare_deploys(vec![contract], PCost { cost: 2 });
    // let b2_deploys_with_cost = TestContext::prepare_deploys(vec![contract], PCost { cost: 1 });
    let b1_deploy =
        offered_deploy::source_deploy_now(contract.to_string(), None, None, None).unwrap();
    let b2_deploy = offered_deploy::source_deploy_now(
        contract.to_string(),
        Some(offered_deploy::DEFAULT_SEC2.clone()),
        None,
        None,
    )
    .unwrap();
    let b3_deploys_with_cost = TestContext::prepare_deploys(vec![], PCost { cost: 5 });

    /*
     * DAG Looks like this:
     *
     *           b3
     *          /  \
     *        b1    b2
     *         \    /
     *         genesis
     */

    let ctx = TestContext::new().await;

    with_genesis(
        ctx.genesis_context.clone(),
        // Changed by DR-134 (DR-66 item 1): the offered step borrows the
        // runtime manager.
        // |mut block_store, mut block_dag_storage, mut runtime_manager| async move {
        |mut block_store, mut block_dag_storage, runtime_manager| async move {
            let genesis = ctx.genesis_context.genesis_block.clone();
            let creator = ctx.genesis_context.validator_pks()[0].bytes.clone();

            let b1 = block_generator::build_block(
                vec![genesis.block_hash.clone()],
                Some(creator.clone()),
                100,
                None,
                None,
                // Changed by DR-134 (DR-66 item 1): the offered step adds the
                // processed deploy.
                // Some(b1_deploys_with_cost),
                None,
                None,
                None,
                None,
                None,
            );

            let b2 = block_generator::build_block(
                vec![genesis.block_hash.clone()],
                Some(creator.clone()),
                200,
                None,
                None,
                // Changed by DR-134 (DR-66 item 1): the offered step adds the
                // processed deploy.
                // Some(b2_deploys_with_cost),
                None,
                None,
                None,
                None,
                None,
            );

            let b3 = block_generator::build_block(
                vec![b1.block_hash.clone(), b2.block_hash.clone()],
                Some(creator),
                300,
                None,
                None,
                Some(b3_deploys_with_cost),
                None,
                None,
                None,
                None,
            );

            // Changed by DR-134 (DR-66 item 1): the offered step.
            // block_generator::step(
            //     &mut block_dag_storage,
            //     &mut block_store,
            //     &mut runtime_manager,
            //     &b1,
            // )
            ctx.step_offered(
                &mut block_dag_storage,
                &mut block_store,
                &runtime_manager,
                &b1,
                b1_deploy,
            )
            .await
            .expect("Failed to step b1");

            // Changed by DR-134 (DR-66 item 1): the offered step.
            // block_generator::step(
            //     &mut block_dag_storage,
            //     &mut block_store,
            //     &mut runtime_manager,
            //     &b2,
            // )
            ctx.step_offered(
                &mut block_dag_storage,
                &mut block_store,
                &runtime_manager,
                &b2,
                b2_deploy,
            )
            .await
            .expect("Failed to step b2");

            let dag = block_dag_storage
                .get_representation()
                .expect("dag representation");
            let mut casper_snapshot = TestContext::mk_casper_snapshot(dag);

            let post_state = interpreter_util::validate_block_checkpoint(
                &b3,
                &block_store,
                &mut casper_snapshot,
                &runtime_manager,
                None,
                None,
                None,
            )
            .await
            .expect("Failed to validate block checkpoint");

            assert_eq!(
                post_state,
                rspace_plus_plus::rspace::history::Either::Right(None),
                "Block validation should return Right(None) for blocks with complex contract merges"
            );
        },
    )
    .await;
}

// Ported from the Scala `InterpreterUtilTest`, where it was `ignore`d pending
// token-balance merging. It now runs against the Rust multi-parent-merging
// design: validating b5, which merges the uneven-length branches b2 and b4,
// yields Right(None). Each block carries a distinct deploy (see the per-block
// source suffixes below): the same deploy may not appear in both a block and
// its descendant, or re-executing it on the descendant's post-state trips the
// per-deploy phlogiston number-channel's single-value invariant.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_block_checkpoint_should_merge_histories_in_case_of_multiple_parents_uneven_histories(
) {
    let b1_src = format!("{}\n//b1", REGISTRY);
    let b2_src = format!("{}\n//b2", REGISTRY);
    let b3_src = format!("{}\n//b3", REGISTRY);
    let b4_src = format!("{}\n//b4", REGISTRY);
    let b5_src = format!("{}\n//b5", REGISTRY);

    // Changed by DR-134 (DR-66 item 1): b1 to b4 run offered deploys, which the
    // offered step computes. b2 is a sibling of the branch b3-b4, so b2 uses
    // another key, because two offers of one signer share a cost cursor. The
    // deploys of b5 stay a synthetic fixture: the pre-state check returns
    // before b5 runs.
    // let b1_deploys_with_cost =
    //     TestContext::prepare_deploys(vec![b1_src.as_str()], PCost { cost: 2 });
    // let b2_deploys_with_cost =
    //     TestContext::prepare_deploys(vec![b2_src.as_str()], PCost { cost: 1 });
    // let b3_deploys_with_cost =
    //     TestContext::prepare_deploys(vec![b3_src.as_str()], PCost { cost: 5 });
    // let b4_deploys_with_cost =
    //     TestContext::prepare_deploys(vec![b4_src.as_str()], PCost { cost: 5 });
    let b1_deploy = offered_deploy::source_deploy_now(b1_src, None, None, None).unwrap();
    let b2_deploy = offered_deploy::source_deploy_now(
        b2_src,
        Some(offered_deploy::DEFAULT_SEC2.clone()),
        None,
        None,
    )
    .unwrap();
    let b3_deploy = offered_deploy::source_deploy_now(b3_src, None, None, None).unwrap();
    let b4_deploy = offered_deploy::source_deploy_now(b4_src, None, None, None).unwrap();
    let b5_deploys_with_cost =
        TestContext::prepare_deploys(vec![b5_src.as_str()], PCost { cost: 5 });

    /*
     * DAG Looks like this:
     *
     *           b5
     *          /  \
     *         |    |
     *         |    b4
     *         |    |
     *        b2    b3
     *         \    /
     *          \  /
     *           |
     *           b1
     *           |
     *         genesis
     */

    let ctx = TestContext::new().await;

    with_genesis(
        ctx.genesis_context.clone(),
        // Changed by DR-134 (DR-66 item 1): the offered step borrows the
        // runtime manager.
        // |mut block_store, mut block_dag_storage, mut runtime_manager| async move {
        |mut block_store, mut block_dag_storage, runtime_manager| async move {
            let genesis = ctx.genesis_context.genesis_block.clone();
            let creator = ctx.genesis_context.validator_pks()[0].bytes.clone();

            let b1 = block_generator::build_block(
                vec![genesis.block_hash.clone()],
                Some(creator.clone()),
                100,
                None,
                None,
                // Changed by DR-134 (DR-66 item 1): the offered step adds the
                // processed deploy.
                // Some(b1_deploys_with_cost),
                None,
                None,
                None,
                None,
                None,
            );

            let b2 = block_generator::build_block(
                vec![b1.block_hash.clone()],
                Some(creator.clone()),
                200,
                None,
                None,
                // Changed by DR-134 (DR-66 item 1): the offered step adds the
                // processed deploy.
                // Some(b2_deploys_with_cost),
                None,
                None,
                None,
                None,
                None,
            );

            let b3 = block_generator::build_block(
                vec![b1.block_hash.clone()],
                Some(creator.clone()),
                200,
                None,
                None,
                // Changed by DR-134 (DR-66 item 1): the offered step adds the
                // processed deploy.
                // Some(b3_deploys_with_cost),
                None,
                None,
                None,
                None,
                None,
            );

            let b4 = block_generator::build_block(
                vec![b3.block_hash.clone()],
                Some(creator.clone()),
                300,
                None,
                None,
                // Changed by DR-134 (DR-66 item 1): the offered step adds the
                // processed deploy.
                // Some(b4_deploys_with_cost),
                None,
                None,
                None,
                None,
                None,
            );

            let b5 = block_generator::build_block(
                vec![b2.block_hash.clone(), b4.block_hash.clone()],
                Some(creator),
                500,
                None,
                None,
                Some(b5_deploys_with_cost),
                None,
                None,
                None,
                None,
            );

            // Changed by DR-134 (DR-66 item 1): the offered step.
            // block_generator::step(
            //     &mut block_dag_storage,
            //     &mut block_store,
            //     &mut runtime_manager,
            //     &b1,
            // )
            ctx.step_offered(
                &mut block_dag_storage,
                &mut block_store,
                &runtime_manager,
                &b1,
                b1_deploy,
            )
            .await
            .expect("Failed to step b1");

            // Changed by DR-134 (DR-66 item 1): the offered step.
            // block_generator::step(
            //     &mut block_dag_storage,
            //     &mut block_store,
            //     &mut runtime_manager,
            //     &b2,
            // )
            ctx.step_offered(
                &mut block_dag_storage,
                &mut block_store,
                &runtime_manager,
                &b2,
                b2_deploy,
            )
            .await
            .expect("Failed to step b2");

            // Changed by DR-134 (DR-66 item 1): the offered step.
            // block_generator::step(
            //     &mut block_dag_storage,
            //     &mut block_store,
            //     &mut runtime_manager,
            //     &b3,
            // )
            ctx.step_offered(
                &mut block_dag_storage,
                &mut block_store,
                &runtime_manager,
                &b3,
                b3_deploy,
            )
            .await
            .expect("Failed to step b3");

            // Changed by DR-134 (DR-66 item 1): the offered step.
            // block_generator::step(
            //     &mut block_dag_storage,
            //     &mut block_store,
            //     &mut runtime_manager,
            //     &b4,
            // )
            ctx.step_offered(
                &mut block_dag_storage,
                &mut block_store,
                &runtime_manager,
                &b4,
                b4_deploy,
            )
            .await
            .expect("Failed to step b4");

            let dag = block_dag_storage
                .get_representation()
                .expect("dag representation");
            let mut casper_snapshot = TestContext::mk_casper_snapshot(dag);

            let post_state = interpreter_util::validate_block_checkpoint(
                &b5,
                &block_store,
                &mut casper_snapshot,
                &runtime_manager,
                None,
                None,
                None,
            )
            .await
            .expect("Failed to validate block checkpoint");

            assert_eq!(
                post_state,
                rspace_plus_plus::rspace::history::Either::Right(None),
                "Block validation should return Right(None) for blocks with uneven history merges"
            );
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_deploys_checkpoint_should_aggregate_cost_of_deploying_rholang_programs_within_the_block(
) {
    //reference costs
    //deploy each Rholang program separately and record its cost

    // Changed by DR-134 (DR-65, gap G2): offers of one signer (DEFAULT_SEC).
    // The offered builders sign the offer limit and the schedule price, so the
    // legacy limit and price arguments have no effect. The cost of each offer
    // is its native cost, its COMMs and its stored bytes.
    // let deploy1 = construct_deploy::source_deploy_now_full(
    let deploy1 = offered_deploy::source_deploy_now_full(
        "@1!(Nil)".to_string(),
        Some(1000000),
        Some(1),
        None,
        None,
        None,
    )
    .expect("offer 1 builds");

    // Changed by DR-134 (DR-65, gap G2): an offer of the same signer.
    // let deploy2 = construct_deploy::source_deploy_now_full(
    let deploy2 = offered_deploy::source_deploy_now_full(
        "@3!([1,2,3,4])".to_string(),
        Some(1000000),
        Some(1),
        None,
        None,
        None,
    )
    .expect("offer 2 builds");

    // Changed by DR-134 (DR-65, gap G2): an offer of the same signer.
    // let deploy3 = construct_deploy::source_deploy_now_full(
    let deploy3 = offered_deploy::source_deploy_now_full(
        "for(@x <- @0) { @4!(x.toByteArray()) }".to_string(),
        Some(1000000),
        Some(1),
        None,
        None,
        None,
    )
    .expect("offer 3 builds");

    let ctx = TestContext::new().await;
    let mut node = TestNode::standalone(ctx.genesis_context.clone())
        .await
        .expect("Failed to create standalone node");

    let dag = node
        .block_dag_storage
        .get_representation()
        .expect("dag representation");

    let cost1 = ctx
        .compute_deploy_costs(
            &mut node.runtime_manager,
            dag.clone(),
            &mut node.block_store,
            vec![deploy1.clone()],
        )
        .await
        .unwrap();

    let cost2 = ctx
        .compute_deploy_costs(
            &mut node.runtime_manager,
            dag.clone(),
            &mut node.block_store,
            vec![deploy2.clone()],
        )
        .await
        .unwrap();

    let cost3 = ctx
        .compute_deploy_costs(
            &mut node.runtime_manager,
            dag.clone(),
            &mut node.block_store,
            vec![deploy3.clone()],
        )
        .await
        .unwrap();

    let mut acc_costs_sep = Vec::new();
    acc_costs_sep.extend(cost1);
    acc_costs_sep.extend(cost2);
    acc_costs_sep.extend(cost3);

    let acc_cost_batch = ctx
        .compute_deploy_costs(&mut node.runtime_manager, dag, &mut node.block_store, vec![
            deploy1, deploy2, deploy3,
        ])
        .await
        .unwrap();

    assert_eq!(
        acc_cost_batch.len(),
        acc_costs_sep.len(),
        "Batch and separate costs should have same length"
    );

    for cost in &acc_costs_sep {
        assert!(
            acc_cost_batch.contains(cost),
            "Batch cost should contain all separate costs"
        );
    }

    for cost in &acc_cost_batch {
        assert!(
            acc_costs_sep.contains(cost),
            "Separate costs should contain all batch costs"
        );
    }
}

// Ported from the Scala `InterpreterUtilTest`, where it was `pendingUntilFixed`
// because the reference set omitted the failing deploy's cost. In the Rust
// runtime a throwing deploy is still processed into a `ProcessedDeploy` with a
// recorded cost, so the batch yields one cost entry per deploy; the reference
// set includes the failing deploy's standalone cost and the two agree.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_deploys_checkpoint_should_return_cost_of_deploying_even_if_one_of_the_programs_within_the_deployment_throws_an_error(
) {
    // Changed by DR-134 (DR-65, gap G2): offers of one signer (DEFAULT_SEC).
    // let deploy1 =
    //     construct_deploy::source_deploy_now("@1!(Nil)".to_string(), None, None, None).unwrap();
    //
    // let deploy2 =
    //     construct_deploy::source_deploy_now("@2!([1,2,3,4])".to_string(), None, None, None)
    //         .unwrap();
    let deploy1 = offered_deploy::source_deploy_now("@1!(Nil)".to_string(), None, None, None)
        .expect("offer 1 builds");

    let deploy2 = offered_deploy::source_deploy_now("@2!([1,2,3,4])".to_string(), None, None, None)
        .expect("offer 2 builds");

    let ctx = TestContext::new().await;
    let mut node = TestNode::standalone(ctx.genesis_context.clone())
        .await
        .expect("Failed to create standalone node");

    let dag = node
        .block_dag_storage
        .get_representation()
        .expect("dag representation");

    let cost1 = ctx
        .compute_deploy_costs(
            &mut node.runtime_manager,
            dag.clone(),
            &mut node.block_store,
            vec![deploy1.clone()],
        )
        .await
        .unwrap();

    let cost2 = ctx
        .compute_deploy_costs(
            &mut node.runtime_manager,
            dag.clone(),
            &mut node.block_store,
            vec![deploy2.clone()],
        )
        .await
        .unwrap();

    // A deploy whose program throws is still processed: it yields a
    // `ProcessedDeploy` with `is_failed = true` and a recorded cost (the
    // phlogiston consumed up to the point of the error). Therefore a batch of
    // three deploys — even when one of them throws — produces three cost
    // entries, and the failing deploy's cost is the same whether it runs alone
    // or alongside the others (the deploys act on independent channels). Include
    // the failing deploy's standalone cost in the reference set so the
    // batch-vs-separate comparison covers all three deploys.
    // Changed by DR-134 (DR-65, gap G2): an offer of the same signer. A user
    // failure of an offer stays in its batch as a charged failure.
    // let deploy_err =
    //     construct_deploy::source_deploy_now(r#"@3!("a" + 3)"#.to_string(), None, None, None)
    //         .expect("failed to create erroring deploy");
    let deploy_err =
        offered_deploy::source_deploy_now(r#"@3!("a" + 3)"#.to_string(), None, None, None)
            .expect("failed to create erroring deploy");

    let cost_err = ctx
        .compute_deploy_costs(
            &mut node.runtime_manager,
            dag.clone(),
            &mut node.block_store,
            vec![deploy_err.clone()],
        )
        .await
        .expect("failed to compute standalone cost of erroring deploy");

    let mut acc_costs_sep = Vec::new();
    acc_costs_sep.extend(cost1);
    acc_costs_sep.extend(cost2);
    acc_costs_sep.extend(cost_err);

    let acc_cost_batch = ctx
        .compute_deploy_costs(&mut node.runtime_manager, dag, &mut node.block_store, vec![
            deploy1, deploy2, deploy_err,
        ])
        .await
        .expect("failed to compute batch cost");

    assert_eq!(
        acc_cost_batch.len(),
        acc_costs_sep.len(),
        "Batch and separate costs should have same length"
    );

    for cost in &acc_costs_sep {
        assert!(
            acc_cost_batch.contains(cost),
            "Batch cost should contain all separate costs"
        );
    }

    for cost in &acc_cost_batch {
        assert!(
            acc_costs_sep.contains(cost),
            "Separate costs should contain all batch costs"
        );
    }
}

//7
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validate_block_checkpoint_should_not_return_a_checkpoint_for_an_invalid_block() {
    with_storage(|mut block_store, mut block_dag_storage| async move {
        let processed_deploys = TestContext::prepare_deploys(vec!["@1!(1)"], PCost { cost: 1 });

        let invalid_hash = StateHash::default();

        // Scala: mkRuntimeManager[Task]("interpreter-util-test").use { runtimeManager =>
        let runtime_manager = resources::mk_runtime_manager("interpreter-util-test-", None).await;

        let block = block_generator::create_genesis_block(
            &mut block_store,
            &mut block_dag_storage,
            None,
            None,
            None,
            Some(processed_deploys),
            Some(invalid_hash),
            None,
            None,
            None,
        );

        let dag = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let mut casper_snapshot = TestContext::mk_casper_snapshot(dag);

        let validate_result = interpreter_util::validate_block_checkpoint(
            &block,
            &block_store,
            &mut casper_snapshot,
            &runtime_manager,
            None,
            None,
            None,
        )
        .await
        .expect("Failed to validate block checkpoint");

        if let Either::Right(state_hash) = validate_result {
            assert_eq!(
                state_hash, None,
                "State hash should be None for invalid block"
            );
        } else {
            panic!("Expected Right(None) but got Left");
        }
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validate_block_checkpoint_should_return_a_checkpoint_with_the_right_hash_for_a_valid_block(
) {
    let ctx = TestContext::new().await;

    with_genesis(
        ctx.genesis_context.clone(),
        |mut block_store, mut block_dag_storage, runtime_manager| async move {
            // Added by DR-134 (DR-65, gap G2): the offered checkpoint and the
            // offered replay take the adopted policy of the test genesis.
            let adopted =
                resources::adopted_policy(&runtime_manager, &ctx.genesis_context.genesis_block)
                    .await;
            // Changed by DR-134 (DR-65, gap G2): a batch of eight offers of one
            // signer (DEFAULT_SEC), so every COMM of the block is between
            // deploys of that signer.
            // let deploys = TestContext::create_deploys_now(
            let deploys = TestContext::create_offered_deploys_now(
                vec![
                    "@1!(1)",
                    "@2!(1)",
                    "@2!(2)",
                    "@2!(3)",
                    "@2!(4)",
                    "@2!(5)",
                    "for (@x <- @1) { @2!(x) }",
                    "for (@x <- @2) { @3!(x) }",
                ],
                None,
            );

            let dag1 = block_dag_storage
                .get_representation()
                .expect("dag representation");
            // Changed by DR-134 (DR-65, gap G2): an offered checkpoint adopts the
            // genesis policy again under the snapshot's shard configuration.
            // let casper_snapshot = TestContext::mk_casper_snapshot(dag1);
            let casper_snapshot = ctx.mk_offered_casper_snapshot(dag1);

            let genesis = ctx.genesis_context.genesis_block.clone();

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            let block_data = BlockData {
                time_stamp: now,
                block_number: 0,
                sender: ctx.genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            };

            // Changed by DR-134 (DR-65, gap G2): the offered checkpoint of the
            // batch.
            // let deploys_checkpoint = interpreter_util::compute_deploys_checkpoint(
            //     &mut block_store,
            //     vec![genesis.clone()],
            //     deploys,
            //     Vec::<SystemDeployEnum>::new(),
            //     &casper_snapshot,
            //     &runtime_manager,
            //     block_data,
            let deploys_checkpoint = interpreter_util::compute_deploys_checkpoint_envelopes(
                &mut block_store,
                vec![genesis.clone()],
                deploys
                    .into_iter()
                    .map(|deploy| PendingDeployCandidate::Envelope(deploy.envelope))
                    .collect(),
                Vec::<SystemDeployEnum>::new(),
                &casper_snapshot,
                &runtime_manager,
                Some(&adopted),
                block_data,
                HashMap::new(),
                None,
                None,
                None,
            )
            .await
            .expect("Failed to compute deploys checkpoint");

            // Changed by DR-134 (DR-65, gap G2).
            // let interpreter_util::DeploysCheckpoint {
            let interpreter_util::DeploysEnvelopeCheckpoint {
                pre_state_hash,
                post_state_hash: computed_ts_hash,
                deploys: processed_deploys,
                ..
            } = deploys_checkpoint;
            // Added by DR-134 (DR-65, gap G2): the block carries the whole batch.
            assert_eq!(
                processed_deploys.len(),
                8,
                "the checkpoint keeps all eight offers"
            );

            let creator = ctx.genesis_context.validator_pks()[0].bytes.clone();
            // Changed by DR-134 (DR-65, gap G2): the block takes the processed
            // offers after `create_block` builds it, and the block store keeps
            // that body.
            // let block = block_generator::create_block(
            let mut block = block_generator::create_block(
                &mut block_store,
                &mut block_dag_storage,
                vec![genesis.block_hash.clone()],
                &genesis,
                Some(creator),
                None,
                None,
                // Some(processed_deploys),
                None,
                Some(computed_ts_hash.clone()),
                None,
                Some(pre_state_hash),
                None,
                None,
                None,
            );
            // Added by DR-134 (DR-65, gap G2).
            block.body.deploys = processed_deploys;
            block_store
                .put(block.block_hash.clone(), &block)
                .expect("store the offered block");

            let dag2 = block_dag_storage
                .get_representation()
                .expect("dag representation");
            let mut casper_snapshot = TestContext::mk_casper_snapshot(dag2);

            // Changed by DR-134 (DR-65, gap G2): the offered replay takes the
            // adopted policy.
            // let validate_result = interpreter_util::validate_block_checkpoint(
            let validate_result = interpreter_util::validate_block_checkpoint_with_policy(
                &block,
                &block_store,
                &mut casper_snapshot,
                &runtime_manager,
                None,
                None,
                None,
                // Added by DR-134 (DR-65, gap G2).
                Some(&adopted),
            )
            .await
            .expect("Failed to validate block checkpoint");

            if let Either::Right(ts_hash) = validate_result {
                assert_eq!(
                    ts_hash,
                    Some(computed_ts_hash),
                    "State hash should match computed hash"
                );
            } else {
                panic!("Expected Right(Some(hash)) but got Left");
            }
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validate_block_checkpoint_should_pass_linked_list_test() {
    let ctx = TestContext::new().await;

    with_genesis(
        ctx.genesis_context.clone(),
        |mut block_store, mut block_dag_storage, runtime_manager| async move {
            // Added by DR-134 (DR-66 item 1): the offered checkpoint and the
            // offered replay take the adopted policy of the test genesis.
            let adopted =
                resources::adopted_policy(&runtime_manager, &ctx.genesis_context.genesis_block)
                    .await;
            // Changed by DR-134 (DR-66 item 1): an offered deploy.
            // let deploys = TestContext::create_deploys_now(
            let deploys = TestContext::create_offered_deploys_now(
                vec![
                    r#"
contract @"recursionTest"(@list) = {
  new loop in {
    contract loop(@rem, @acc) = {
      match rem {
        [head, ...tail] => {
          new newAccCh in {
            newAccCh!([head, acc]) |
            for(@newAcc <- newAccCh) {
              loop!(tail, newAcc)
            }
          }
        }
        _ => { Nil } // Normally we would print the "acc" ([2,[1,[]]]) out
      }
    } |
    new unusedCh in {
      loop!(list, [])
    }
  }
} |
@"recursionTest"!([1,2])
"#,
                ],
                None,
            );

            let dag1 = block_dag_storage
                .get_representation()
                .expect("dag representation");
            // Changed by DR-134 (DR-66 item 1): an offered checkpoint adopts the
            // genesis policy again under the snapshot's shard configuration.
            // let casper_snapshot = TestContext::mk_casper_snapshot(dag1);
            let casper_snapshot = ctx.mk_offered_casper_snapshot(dag1);

            let genesis = ctx.genesis_context.genesis_block.clone();

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            let block_data = BlockData {
                time_stamp: now,
                block_number: 0,
                sender: ctx.genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            };

            // Changed by DR-134 (DR-66 item 1): the offered checkpoint.
            // let deploys_checkpoint = interpreter_util::compute_deploys_checkpoint(
            //     &mut block_store,
            //     vec![genesis.clone()],
            //     deploys,
            //     Vec::<SystemDeployEnum>::new(),
            //     &casper_snapshot,
            //     &runtime_manager,
            //     block_data,
            let deploys_checkpoint = interpreter_util::compute_deploys_checkpoint_envelopes(
                &mut block_store,
                vec![genesis.clone()],
                deploys
                    .into_iter()
                    .map(|deploy| PendingDeployCandidate::Envelope(deploy.envelope))
                    .collect(),
                Vec::<SystemDeployEnum>::new(),
                &casper_snapshot,
                &runtime_manager,
                Some(&adopted),
                block_data,
                HashMap::new(),
                None,
                None,
                None,
            )
            .await
            .expect("Failed to compute deploys checkpoint");

            // Changed by DR-134 (DR-66 item 1).
            // let interpreter_util::DeploysCheckpoint {
            let interpreter_util::DeploysEnvelopeCheckpoint {
                pre_state_hash,
                post_state_hash: computed_ts_hash,
                deploys: processed_deploys,
                ..
            } = deploys_checkpoint;

            let creator = ctx.genesis_context.validator_pks()[0].bytes.clone();
            // Changed by DR-134 (DR-66 item 1): the block takes the processed
            // offered deploy after `create_block` builds it, and the block store
            // keeps that body.
            // let block = block_generator::create_block(
            let mut block = block_generator::create_block(
                &mut block_store,
                &mut block_dag_storage,
                vec![genesis.block_hash.clone()],
                &genesis,
                Some(creator),
                None,
                None,
                // Some(processed_deploys),
                None,
                Some(computed_ts_hash.clone()),
                None,
                Some(pre_state_hash),
                None,
                None,
                None,
            );
            // Added by DR-134 (DR-66 item 1).
            block.body.deploys = processed_deploys;
            block_store
                .put(block.block_hash.clone(), &block)
                .expect("store the offered block");

            let dag2 = block_dag_storage
                .get_representation()
                .expect("dag representation");
            let mut casper_snapshot = TestContext::mk_casper_snapshot(dag2);

            // Changed by DR-134 (DR-66 item 1): the offered replay takes the
            // adopted policy.
            // let validate_result = interpreter_util::validate_block_checkpoint(
            let validate_result = interpreter_util::validate_block_checkpoint_with_policy(
                &block,
                &block_store,
                &mut casper_snapshot,
                &runtime_manager,
                None,
                None,
                None,
                // Added by DR-134 (DR-66 item 1).
                Some(&adopted),
            )
            .await
            .expect("Failed to validate block checkpoint");

            if let Either::Right(ts_hash) = validate_result {
                assert_eq!(
                    ts_hash,
                    Some(computed_ts_hash),
                    "State hash should match computed hash"
                );
            } else {
                panic!("Expected Right(Some(hash)) but got Left");
            }
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validate_block_checkpoint_should_pass_persistent_produce_test_with_causality() {
    let ctx = TestContext::new().await;

    with_genesis(
        ctx.genesis_context.clone(),
        |mut block_store, mut block_dag_storage, runtime_manager| async move {
            // Added by DR-134 (DR-66 item 1): the offered checkpoint and the
            // offered replay take the adopted policy of the test genesis.
            let adopted =
                resources::adopted_policy(&runtime_manager, &ctx.genesis_context.genesis_block)
                    .await;
            // Changed by DR-134 (DR-66 item 1): an offered deploy.
            // let deploys = TestContext::create_deploys_now(
            let deploys = TestContext::create_offered_deploys_now(
                vec![
                    r#"new x, y, delay in {
              contract delay(@n) = {
                if (n < 100) {
                  delay!(n + 1)
                } else {
                  x!!(1)
                }
              } |
              delay!(0) |
              y!(0) |
              for (_ <- x & @0 <- y) { y!(1) } |
              for (_ <- x & @1 <- y) { y!(2) } |
              for (_ <- x & @2 <- y) { y!(3) } |
              for (_ <- x & @3 <- y) { y!(4) } |
              for (_ <- x & @4 <- y) { y!(5) } |
              for (_ <- x & @5 <- y) { y!(6) } |
              for (_ <- x & @6 <- y) { y!(7) } |
              for (_ <- x & @7 <- y) { y!(8) } |
              for (_ <- x & @8 <- y) { y!(9) } |
              for (_ <- x & @9 <- y) { y!(10) } |
              for (_ <- x & @10 <- y) { y!(11) } |
              for (_ <- x & @11 <- y) { y!(12) } |
              for (_ <- x & @12 <- y) { y!(13) } |
              for (_ <- x & @13 <- y) { y!(14) } |
              for (_ <- x & @14 <- y) { Nil }
             }
          "#,
                ],
                None, // uses default key
            );

            let dag1 = block_dag_storage
                .get_representation()
                .expect("dag representation");
            // Changed by DR-134 (DR-66 item 1): an offered checkpoint adopts the
            // genesis policy again under the snapshot's shard configuration.
            // let casper_snapshot = TestContext::mk_casper_snapshot(dag1);
            let casper_snapshot = ctx.mk_offered_casper_snapshot(dag1);

            let genesis = ctx.genesis_context.genesis_block.clone();

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            let block_data = BlockData {
                time_stamp: now,
                block_number: 0,
                sender: ctx.genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            };

            // Changed by DR-134 (DR-66 item 1): the offered checkpoint.
            // let deploys_checkpoint = interpreter_util::compute_deploys_checkpoint(
            //     &mut block_store,
            //     vec![genesis.clone()],
            //     deploys,
            //     Vec::<SystemDeployEnum>::new(),
            //     &casper_snapshot,
            //     &runtime_manager,
            //     block_data,
            let deploys_checkpoint = interpreter_util::compute_deploys_checkpoint_envelopes(
                &mut block_store,
                vec![genesis.clone()],
                deploys
                    .into_iter()
                    .map(|deploy| PendingDeployCandidate::Envelope(deploy.envelope))
                    .collect(),
                Vec::<SystemDeployEnum>::new(),
                &casper_snapshot,
                &runtime_manager,
                Some(&adopted),
                block_data,
                HashMap::new(),
                None,
                None,
                None,
            )
            .await
            .expect("Failed to compute deploys checkpoint");

            // Changed by DR-134 (DR-66 item 1).
            // let interpreter_util::DeploysCheckpoint {
            let interpreter_util::DeploysEnvelopeCheckpoint {
                pre_state_hash,
                post_state_hash: computed_ts_hash,
                deploys: processed_deploys,
                ..
            } = deploys_checkpoint;

            let creator = ctx.genesis_context.validator_pks()[0].bytes.clone();
            // Changed by DR-134 (DR-66 item 1): the block takes the processed
            // offered deploy after `create_block` builds it, and the block store
            // keeps that body.
            // let block = block_generator::create_block(
            let mut block = block_generator::create_block(
                &mut block_store,
                &mut block_dag_storage,
                vec![genesis.block_hash.clone()],
                &genesis,
                Some(creator),
                None,
                None,
                // Some(processed_deploys),
                None,
                Some(computed_ts_hash.clone()),
                None,
                Some(pre_state_hash),
                None,
                None,
                None,
            );
            // Added by DR-134 (DR-66 item 1).
            block.body.deploys = processed_deploys;
            block_store
                .put(block.block_hash.clone(), &block)
                .expect("store the offered block");

            let dag2 = block_dag_storage
                .get_representation()
                .expect("dag representation");
            let mut casper_snapshot = TestContext::mk_casper_snapshot(dag2);

            // Changed by DR-134 (DR-66 item 1): the offered replay takes the
            // adopted policy.
            // let validate_result = interpreter_util::validate_block_checkpoint(
            let validate_result = interpreter_util::validate_block_checkpoint_with_policy(
                &block,
                &block_store,
                &mut casper_snapshot,
                &runtime_manager,
                None,
                None,
                None,
                // Added by DR-134 (DR-66 item 1).
                Some(&adopted),
            )
            .await
            .expect("Failed to validate block checkpoint");

            if let Either::Right(ts_hash) = validate_result {
                assert_eq!(
                    ts_hash,
                    Some(computed_ts_hash),
                    "State hash should match computed hash"
                );
            } else {
                panic!("Expected Right(Some(hash)) but got Left");
            }
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validate_block_checkpoint_should_pass_tests_involving_primitives() {
    let ctx = TestContext::new().await;

    with_genesis(
        ctx.genesis_context.clone(),
        |mut block_store, mut block_dag_storage, runtime_manager| async move {
            // Added by DR-134 (DR-66 item 1): the offered checkpoint and the
            // offered replay take the adopted policy of the test genesis.
            let adopted =
                resources::adopted_policy(&runtime_manager, &ctx.genesis_context.genesis_block)
                    .await;
            // Changed by DR-134 (DR-66 item 1): an offered deploy.
            // let deploys = TestContext::create_deploys_now(
            let deploys = TestContext::create_offered_deploys_now(
                vec![
                    r#"
new loop, primeCheck, stdoutAck(`rho:io:stdoutAck`) in {
  contract loop(@x) = {
    match x {
      [] => Nil
      [head ...tail] => {
        new ret in {
          for (_ <- ret) {
            loop!(tail)
          } | primeCheck!(head, *ret)
        }
      }
    }
  } |
  contract primeCheck(@x, ret) = {
    match x {
      Nil => stdoutAck!("Nil", *ret)
      ~{~Nil | ~Nil} => stdoutAck!("Prime", *ret)
      _ => stdoutAck!("Composite", *ret)
    }
  } |
  loop!([Nil, 7, 7 | 8, 9 | Nil, 9 | 10, Nil, 9])
}"#,
                ],
                None,
            );

            let dag1 = block_dag_storage
                .get_representation()
                .expect("dag representation");
            // Changed by DR-134 (DR-66 item 1): an offered checkpoint adopts the
            // genesis policy again under the snapshot's shard configuration.
            // let casper_snapshot = TestContext::mk_casper_snapshot(dag1);
            let casper_snapshot = ctx.mk_offered_casper_snapshot(dag1);

            let genesis = ctx.genesis_context.genesis_block.clone();

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            let block_data = BlockData {
                time_stamp: now,
                block_number: 0,
                sender: ctx.genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            };

            // Changed by DR-134 (DR-66 item 1): the offered checkpoint.
            // let deploys_checkpoint = interpreter_util::compute_deploys_checkpoint(
            //     &mut block_store,
            //     vec![genesis.clone()],
            //     deploys,
            //     Vec::<SystemDeployEnum>::new(),
            //     &casper_snapshot,
            //     &runtime_manager,
            //     block_data,
            let deploys_checkpoint = interpreter_util::compute_deploys_checkpoint_envelopes(
                &mut block_store,
                vec![genesis.clone()],
                deploys
                    .into_iter()
                    .map(|deploy| PendingDeployCandidate::Envelope(deploy.envelope))
                    .collect(),
                Vec::<SystemDeployEnum>::new(),
                &casper_snapshot,
                &runtime_manager,
                Some(&adopted),
                block_data,
                HashMap::new(),
                None,
                None,
                None,
            )
            .await
            .expect("Failed to compute deploys checkpoint");

            // Changed by DR-134 (DR-66 item 1).
            // let interpreter_util::DeploysCheckpoint {
            let interpreter_util::DeploysEnvelopeCheckpoint {
                pre_state_hash,
                post_state_hash: computed_ts_hash,
                deploys: processed_deploys,
                ..
            } = deploys_checkpoint;

            let creator = ctx.genesis_context.validator_pks()[0].bytes.clone();
            // Changed by DR-134 (DR-66 item 1): the block takes the processed
            // offered deploy after `create_block` builds it, and the block store
            // keeps that body.
            // let block = block_generator::create_block(
            let mut block = block_generator::create_block(
                &mut block_store,
                &mut block_dag_storage,
                vec![genesis.block_hash.clone()],
                &genesis,
                Some(creator),
                None,
                None,
                // Some(processed_deploys),
                None,
                Some(computed_ts_hash.clone()),
                None,
                Some(pre_state_hash),
                None,
                None,
                None,
            );
            // Added by DR-134 (DR-66 item 1).
            block.body.deploys = processed_deploys;
            block_store
                .put(block.block_hash.clone(), &block)
                .expect("store the offered block");

            let dag2 = block_dag_storage
                .get_representation()
                .expect("dag representation");
            let mut casper_snapshot = TestContext::mk_casper_snapshot(dag2);

            // Changed by DR-134 (DR-66 item 1): the offered replay takes the
            // adopted policy.
            // let validate_result = interpreter_util::validate_block_checkpoint(
            let validate_result = interpreter_util::validate_block_checkpoint_with_policy(
                &block,
                &block_store,
                &mut casper_snapshot,
                &runtime_manager,
                None,
                None,
                None,
                // Added by DR-134 (DR-66 item 1).
                Some(&adopted),
            )
            .await
            .expect("Failed to validate block checkpoint");

            if let Either::Right(ts_hash) = validate_result {
                assert_eq!(
                    ts_hash,
                    Some(computed_ts_hash),
                    "State hash should match computed hash"
                );
            } else {
                panic!("Expected Right(Some(hash)) but got Left");
            }
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validate_block_checkpoint_should_pass_tests_involving_races() {
    let ctx = TestContext::new().await;

    with_genesis(
        ctx.genesis_context.clone(),
        |mut block_store, mut block_dag_storage, runtime_manager| async move {
            // Added by DR-134 (DR-66 item 1): the offered checkpoint and the
            // offered replay take the adopted policy of the test genesis.
            let adopted =
                resources::adopted_policy(&runtime_manager, &ctx.genesis_context.genesis_block)
                    .await;
            for i in 0..=10 {
                // Changed by DR-134 (DR-66 item 1): an offered deploy. Each
                // iteration validates its own child of genesis, and no block
                // merges two of them.
                // let deploys = TestContext::create_deploys_now(
                let deploys = TestContext::create_offered_deploys_now(
                    vec![
                        r#"
 contract @"loop"(@xs) = {
   match xs {
     [] => {
       for (@winner <- @"ch") {
         @"return"!(winner)
       }
     }
     [first, ...rest] => {
       @"ch"!(first) | @"loop"!(rest)
     }
   }
 } | @"loop"!(["a","b","c","d"])
"#,
                    ],
                    None,
                );

                let dag1 = block_dag_storage
                    .get_representation()
                    .expect("dag representation");
                // Changed by DR-134 (DR-66 item 1): an offered checkpoint adopts
                // the genesis policy again under the snapshot's shard
                // configuration.
                // let casper_snapshot = TestContext::mk_casper_snapshot(dag1);
                let casper_snapshot = ctx.mk_offered_casper_snapshot(dag1);

                let genesis = ctx.genesis_context.genesis_block.clone();

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64;

                let block_data = BlockData {
                    time_stamp: now,
                    block_number: (i + 1) as i64,
                    sender: ctx.genesis_context.validator_pks()[0].clone(),
                    seq_num: (i + 1),
                };

                // Changed by DR-134 (DR-66 item 1): the offered checkpoint.
                // let deploys_checkpoint = interpreter_util::compute_deploys_checkpoint(
                //     &mut block_store,
                //     vec![genesis.clone()],
                //     deploys,
                //     Vec::<SystemDeployEnum>::new(),
                //     &casper_snapshot,
                //     &runtime_manager,
                //     block_data,
                let deploys_checkpoint = interpreter_util::compute_deploys_checkpoint_envelopes(
                    &mut block_store,
                    vec![genesis.clone()],
                    deploys
                        .into_iter()
                        .map(|deploy| PendingDeployCandidate::Envelope(deploy.envelope))
                        .collect(),
                    Vec::<SystemDeployEnum>::new(),
                    &casper_snapshot,
                    &runtime_manager,
                    Some(&adopted),
                    block_data,
                    HashMap::new(),
                    None,
                    None,
                    None,
                )
                .await
                .expect("Failed to compute deploys checkpoint");

                // Changed by DR-134 (DR-66 item 1).
                // let interpreter_util::DeploysCheckpoint {
                let interpreter_util::DeploysEnvelopeCheckpoint {
                    pre_state_hash,
                    post_state_hash: computed_ts_hash,
                    deploys: processed_deploys,
                    ..
                } = deploys_checkpoint;

                let creator = ctx.genesis_context.validator_pks()[0].bytes.clone();
                // Changed by DR-134 (DR-66 item 1): the block takes the
                // processed offered deploy after `create_block` builds it, and
                // the block store keeps that body.
                // let block = block_generator::create_block(
                let mut block = block_generator::create_block(
                    &mut block_store,
                    &mut block_dag_storage,
                    vec![genesis.block_hash.clone()],
                    &genesis,
                    Some(creator),
                    None,
                    None,
                    // Some(processed_deploys),
                    None,
                    Some(computed_ts_hash.clone()),
                    None,
                    Some(pre_state_hash),
                    Some(i + 1),
                    None,
                    None,
                );
                // Added by DR-134 (DR-66 item 1).
                block.body.deploys = processed_deploys;
                block_store
                    .put(block.block_hash.clone(), &block)
                    .expect("store the offered block");

                let dag2 = block_dag_storage
                    .get_representation()
                    .expect("dag representation");
                let mut casper_snapshot = TestContext::mk_casper_snapshot(dag2);

                // Changed by DR-134 (DR-66 item 1): the offered replay takes the
                // adopted policy.
                // let validate_result = interpreter_util::validate_block_checkpoint(
                let validate_result = interpreter_util::validate_block_checkpoint_with_policy(
                    &block,
                    &block_store,
                    &mut casper_snapshot,
                    &runtime_manager,
                    None,
                    None,
                    None,
                    // Added by DR-134 (DR-66 item 1).
                    Some(&adopted),
                )
                .await
                .expect("Failed to validate block checkpoint");

                if let Either::Right(ts_hash) = validate_result {
                    assert_eq!(
                        ts_hash,
                        Some(computed_ts_hash),
                        "State hash should match computed hash for iteration {}",
                        i
                    );
                } else {
                    panic!(
                        "Expected Right(Some(hash)) but got Left for iteration {}",
                        i
                    );
                }
            }
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validate_block_checkpoint_should_return_none_for_logs_containing_extra_comm_events() {
    let ctx = TestContext::new().await;

    with_genesis(
        ctx.genesis_context.clone(),
        |mut block_store, mut block_dag_storage, runtime_manager| async move {
            // Added by DR-134 (DR-65, gap G2): the offered checkpoint and the
            // offered replay take the adopted policy of the test genesis.
            let adopted =
                resources::adopted_policy(&runtime_manager, &ctx.genesis_context.genesis_block)
                    .await;
            // Changed by DR-134 (DR-65, gap G2): two offers in place of one. Dev's
            // block carries one deploy twice, the second copy with the extra
            // events. Replay rejects a batch that repeats a member (O-9) before
            // it reads any deploy log, so that block would fail for the repeat
            // and not for the extra events. The two offers are distinct, and the
            // log of the first one carries the extra events of the second one,
            // as the comment below describes. Both offers have one signer
            // (DEFAULT_SEC).
            // let sources: Vec<String> = (0..1)
            //     .map(|i| format!("for(_ <- @{}){{{ } Nil }} | @{}!({})", i, "", i, i))
            //     .collect();
            // let deploys =
            //     TestContext::create_deploys_now(sources.iter().map(|s| s.as_str()).collect(), None);
            let sources: Vec<String> = (0..2)
                .map(|i| format!("for(_ <- @{}){{{ } Nil }} | @{}!({})", i, "", i, i))
                .collect();
            let deploys = TestContext::create_offered_deploys_now(
                sources.iter().map(|s| s.as_str()).collect(),
                None,
            );

            let dag1 = block_dag_storage
                .get_representation()
                .expect("dag representation");
            // Changed by DR-134 (DR-65, gap G2): an offered checkpoint adopts the
            // genesis policy again under the snapshot's shard configuration.
            // let casper_snapshot = TestContext::mk_casper_snapshot(dag1);
            let casper_snapshot = ctx.mk_offered_casper_snapshot(dag1);

            let genesis = ctx.genesis_context.genesis_block.clone();

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            let block_data = BlockData {
                time_stamp: now,
                block_number: 0,
                sender: ctx.genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            };

            // Changed by DR-134 (DR-65, gap G2): the offered checkpoint of the
            // batch.
            // let deploys_checkpoint = interpreter_util::compute_deploys_checkpoint(
            //     &mut block_store,
            //     vec![genesis.clone()],
            //     deploys,
            //     Vec::<SystemDeployEnum>::new(),
            //     &casper_snapshot,
            //     &runtime_manager,
            //     block_data,
            let deploys_checkpoint = interpreter_util::compute_deploys_checkpoint_envelopes(
                &mut block_store,
                vec![genesis.clone()],
                deploys
                    .into_iter()
                    .map(|deploy| PendingDeployCandidate::Envelope(deploy.envelope))
                    .collect(),
                Vec::<SystemDeployEnum>::new(),
                &casper_snapshot,
                &runtime_manager,
                Some(&adopted),
                block_data,
                HashMap::new(),
                None,
                None,
                None,
            )
            .await
            .expect("Failed to compute deploys checkpoint");

            // Changed by DR-134 (DR-65, gap G2).
            // let interpreter_util::DeploysCheckpoint {
            let interpreter_util::DeploysEnvelopeCheckpoint {
                pre_state_hash,
                post_state_hash: computed_ts_hash,
                deploys: processed_deploys,
                ..
            } = deploys_checkpoint;
            // Added by DR-134 (DR-65, gap G2).
            assert_eq!(
                processed_deploys.len(),
                2,
                "the checkpoint keeps both offers"
            );

            // create single deploy with log that includes excess comm events
            // Changed by DR-134 (DR-65, gap G2): a processed offer exposes its
            // log only through its proto form, so the extra events join the
            // proto log, and the extended form decodes again.
            // let mut bad_processed_deploy = processed_deploys[0].clone();
            // let extra_events = processed_deploys
            //     .last()
            //     .unwrap()
            //     .deploy_log
            //     .iter()
            //     .take(5)
            //     .cloned()
            //     .collect::<Vec<_>>();
            // bad_processed_deploy.deploy_log.extend(extra_events);
            let mut bad_processed_proto = processed_deploys[0].to_proto();
            let extra_events = processed_deploys
                .last()
                .expect("the checkpoint has a last offer")
                .deploy_log()
                .iter()
                .take(5)
                .map(|event| event.to_proto())
                .collect::<Vec<_>>();
            assert!(
                !extra_events.is_empty(),
                "the last offer has events to copy"
            );
            bad_processed_proto.deploy_log.extend(extra_events);
            let bad_processed_deploy =
                models::rust::casper::protocol::casper_message::ProcessedUserDeploy::from_proto(
                    bad_processed_proto,
                )
                .expect("the offer with the extra events decodes");

            let creator = ctx.genesis_context.validator_pks()[0].bytes.clone();
            let deploys_for_block = vec![
                bad_processed_deploy,
                processed_deploys
                    .last()
                    .expect("the checkpoint has a last offer")
                    .clone(),
            ];
            // Changed by DR-134 (DR-65, gap G2): the block takes the processed
            // offers after `create_block` builds it, and the block store keeps
            // that body.
            // let block = block_generator::create_block(
            let mut block = block_generator::create_block(
                &mut block_store,
                &mut block_dag_storage,
                vec![genesis.block_hash.clone()],
                &genesis,
                Some(creator),
                None,
                None,
                // Some(deploys_for_block),
                None,
                Some(computed_ts_hash.clone()),
                None,
                Some(pre_state_hash),
                None,
                None,
                None,
            );
            // Added by DR-134 (DR-65, gap G2).
            block.body.deploys = deploys_for_block;
            block_store
                .put(block.block_hash.clone(), &block)
                .expect("store the offered block");

            let dag2 = block_dag_storage
                .get_representation()
                .expect("dag representation");
            let mut casper_snapshot = TestContext::mk_casper_snapshot(dag2);

            // Changed by DR-134 (DR-65, gap G2): the offered replay takes the
            // adopted policy.
            // let validate_result = interpreter_util::validate_block_checkpoint(
            let validate_result = interpreter_util::validate_block_checkpoint_with_policy(
                &block,
                &block_store,
                &mut casper_snapshot,
                &runtime_manager,
                None,
                None,
                None,
                // Added by DR-134 (DR-65, gap G2).
                Some(&adopted),
            )
            .await
            .expect("Failed to validate block checkpoint");

            match validate_result {
                Either::Right(ts_hash) => {
                    assert_eq!(
                        ts_hash, None,
                        "State hash should be None for block with extra comm events"
                    );
                }
                Either::Left(status) => {
                    // In Rust implementation, invalid blocks may return Left with InvalidBlock status
                    // which is also acceptable for this test (block is invalid due to extra comm events)
                    println!("Block validation returned Left with status: {:?}", status);
                }
            }
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn validate_block_checkpoint_should_pass_map_update_test() {
    let ctx = TestContext::new().await;

    with_genesis(
        ctx.genesis_context.clone(),
        |mut block_store, mut block_dag_storage, runtime_manager| async move {
            let genesis = ctx.genesis_context.genesis_block.clone();
            // Added by DR-134 (DR-65, gap G2): the offered checkpoint and the
            // offered replay take the adopted policy of the test genesis.
            let adopted =
                resources::adopted_policy(&runtime_manager, &ctx.genesis_context.genesis_block)
                    .await;

            for i in 0..=10 {
                // Changed by DR-134 (DR-65, gap G2): a batch of three offers of
                // one signer (DEFAULT_SEC), so the store contract, its map cell
                // and the two store calls belong to that signer.
                // let deploys = TestContext::create_deploys_now(
                let deploys = TestContext::create_offered_deploys_now(
                    vec![
                        r#"
 @"mapStore"!({}) |
 contract @"store"(@value) = {
   new key in {
     for (@map <- @"mapStore") {
       @"mapStore"!(map.set(*key.toByteArray(), value))
     }
   }
 }
"#,
                        r#"
@"store"!("1")
"#,
                        r#"
@"store"!("2")
"#,
                    ],
                    None,
                );

                let dag1 = block_dag_storage
                    .get_representation()
                    .expect("dag representation");
                // Changed by DR-134 (DR-65, gap G2): an offered checkpoint adopts
                // the genesis policy again under the snapshot's shard
                // configuration.
                // let casper_snapshot = TestContext::mk_casper_snapshot(dag1);
                let casper_snapshot = ctx.mk_offered_casper_snapshot(dag1);

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64;

                let block_data = BlockData {
                    time_stamp: now,
                    block_number: (i + 1) as i64,
                    sender: ctx.genesis_context.validator_pks()[0].clone(),
                    seq_num: (i + 1),
                };

                // Changed by DR-134 (DR-65, gap G2): the offered checkpoint of
                // the batch.
                // let deploys_checkpoint = interpreter_util::compute_deploys_checkpoint(
                //     &mut block_store,
                //     vec![genesis.clone()],
                //     deploys,
                //     Vec::<SystemDeployEnum>::new(),
                //     &casper_snapshot,
                //     &runtime_manager,
                //     block_data,
                let deploys_checkpoint = interpreter_util::compute_deploys_checkpoint_envelopes(
                    &mut block_store,
                    vec![genesis.clone()],
                    deploys
                        .into_iter()
                        .map(|deploy| PendingDeployCandidate::Envelope(deploy.envelope))
                        .collect(),
                    Vec::<SystemDeployEnum>::new(),
                    &casper_snapshot,
                    &runtime_manager,
                    Some(&adopted),
                    block_data,
                    HashMap::new(),
                    None,
                    None,
                    None,
                )
                .await
                .expect("Failed to compute deploys checkpoint");

                // Changed by DR-134 (DR-65, gap G2).
                // let interpreter_util::DeploysCheckpoint {
                let interpreter_util::DeploysEnvelopeCheckpoint {
                    pre_state_hash,
                    post_state_hash: computed_ts_hash,
                    deploys: processed_deploys,
                    ..
                } = deploys_checkpoint;
                // Added by DR-134 (DR-65, gap G2): the block carries the whole
                // batch.
                assert_eq!(
                    processed_deploys.len(),
                    3,
                    "the checkpoint keeps all three offers for iteration {}",
                    i
                );

                let creator = ctx.genesis_context.validator_pks()[0].bytes.clone();
                // Changed by DR-134 (DR-65, gap G2): the block takes the
                // processed offers after `create_block` builds it, and the block
                // store keeps that body.
                // let block = block_generator::create_block(
                let mut block = block_generator::create_block(
                    &mut block_store,
                    &mut block_dag_storage,
                    vec![genesis.block_hash.clone()],
                    &genesis,
                    Some(creator),
                    None,
                    None,
                    // Some(processed_deploys),
                    None,
                    Some(computed_ts_hash.clone()),
                    None,
                    Some(pre_state_hash),
                    Some(i + 1),
                    None,
                    None,
                );
                // Added by DR-134 (DR-65, gap G2).
                block.body.deploys = processed_deploys;
                block_store
                    .put(block.block_hash.clone(), &block)
                    .expect("store the offered block");

                let dag2 = block_dag_storage
                    .get_representation()
                    .expect("dag representation");
                let mut casper_snapshot = TestContext::mk_casper_snapshot(dag2);

                // Changed by DR-134 (DR-65, gap G2): the offered replay takes the
                // adopted policy.
                // let validate_result = interpreter_util::validate_block_checkpoint(
                let validate_result = interpreter_util::validate_block_checkpoint_with_policy(
                    &block,
                    &block_store,
                    &mut casper_snapshot,
                    &runtime_manager,
                    None,
                    None,
                    None,
                    // Added by DR-134 (DR-65, gap G2).
                    Some(&adopted),
                )
                .await
                .expect("Failed to validate block checkpoint");

                if let Either::Right(ts_hash) = validate_result {
                    assert_eq!(
                        ts_hash,
                        Some(computed_ts_hash),
                        "State hash should match computed hash for iteration {}",
                        i
                    );
                } else {
                    panic!(
                        "Expected Right(Some(hash)) but got Left for iteration {}",
                        i
                    );
                }

                // Scala: _ <- timeEff.advance()
                // Note: timeEff.advance() in Scala is used to advance logical time
            }
        },
    )
    .await;
}

// Test for cost mismatch between play and replay in case of out of phlo error
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn used_deploy_with_insufficient_phlos_should_be_added_to_a_block_with_all_phlos_consumed() {
    let ctx = TestContext::new().await;

    let sample_term = r#"
  new
    rl(`rho:registry:lookup`), RevVaultCh, vaultCh, balanceCh, deployId(`rho:system:deployId`)
  in {
    rl!(`rho:vault:system`, *RevVaultCh) |
    for (@(_, RevVault) <- RevVaultCh) {
      match "1111MnCcfyG9sExhw1jQcW6hSb98c2XUtu3E4KGSxENo1nTn4e5cx" {
        revAddress => {
          @RevVault!("findOrCreate", revAddress, *vaultCh) |
          for (@(true, vault) <- vaultCh) {
            @vault!("balance", *balanceCh) |
            for (@balance <- balanceCh) {
              deployId!(balance)
            }
          }
        }
      }
    }
  }
"#;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    // Changed by DR-134 (DR-66 item 1): an offered deploy signs its own phlo
    // limit. Under the native meter one COMM with its two introductions costs
    // about 3,100 phlo, so the estimate of the visible sends and receives of
    // this term passes the dev limit of 3,000. The check refuses an offer
    // whose estimate passes its signed limit before execution, and no block
    // holds it (`decide` in
    // casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/gate.rs,
    // P1:2210-2212). The signed limit of 20,000, the limit of
    // `replay_should_match_in_case_of_out_of_phlo_error` below, holds the
    // estimate. The registry lookup and the vault calls of the term cost more,
    // so the run exhausts the signed limit, which is a charged user failure
    // that stays in the block (DR-113).
    // let deploy = construct_deploy::source_deploy(
    //     sample_term.to_string(),
    //     timestamp,
    //     Some(3000),
    //     None,
    //     None,
    //     None,
    //     Some(ctx.genesis_context.genesis_block.shard_id.clone()),
    // )
    // .expect("Failed to create deploy");
    let phlo_limit = 20_000;
    let deploy = OfferedDeploy::from_proto(offered_deploy::owner_offer_limited(
        &(
            construct_deploy::DEFAULT_SEC.clone(),
            construct_deploy::DEFAULT_PUB.clone(),
        ),
        timestamp,
        sample_term.to_string(),
        0,
        ctx.genesis_context.genesis_block.shard_id.clone(),
        phlo_limit,
    ));

    let mut node = TestNode::standalone(ctx.genesis_context)
        .await
        .expect("Failed to create standalone node");

    let b = node
        .add_block_from_deploys(&[deploy])
        .await
        .expect("Failed to add block");

    assert_eq!(
        b.body.deploys.len(),
        1,
        "Block should have exactly 1 deploy"
    );

    let deploy_cost = b.body.deploys[0].cost().cost;
    // Changed by DR-134 (DR-66 item 1): the native meter grants each charge
    // that fits the signed limit and denies the first charge that does not
    // (`reserve_native_usage` in
    // rholang/src/rust/interpreter/accounting/native_phlo_rules/execution.rs).
    // The exhausted deploy is published as failed and pays the work granted
    // up to the limit, as the receipt of
    // `signed_limit_exhaustion_is_published_as_a_charged_user_failure`
    // (casper/tests/api/offered_funded_api_test.rs) shows.
    // assert_eq!(deploy_cost, 3000, "Deploy should consume all phlos (3000)");
    assert!(
        b.body.deploys[0].is_failed(),
        "Deploy should exhaust its signed phlo limit"
    );
    assert!(
        deploy_cost > 0 && deploy_cost <= phlo_limit,
        "Deploy should consume its phlos up to the signed limit ({phlo_limit}), got {deploy_cost}"
    );
}

const MULTI_BRANCH_SAMPLE_TERM_WITH_ERROR: &str = r#"
  new rl(`rho:registry:lookup`), RevVaultCh, ackCh, out(`rho:io:stdout`)
  in {
    new signal in {
      signal!(0) | signal!(0) | signal!(0) | signal!(0) | signal!(0) | signal!(0) | signal!(1) |
      contract signal(@x) = {
        rl!(`rho:vault:system`, *RevVaultCh) | ackCh!(x) |
        if (x == 1) {}.xxx() // Simulates error in one branch
      }
    } |
    for (@(_, RevVault) <= RevVaultCh & @x<= ackCh) {
      @(*ackCh, "parallel universe")!("Rick and Morty")
    }
  }
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replay_should_match_in_case_of_out_of_phlo_error() {
    let ctx = TestContext::new().await;

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;

    // Changed by DR-134 (DR-66 item 1): an offered deploy that signs the dev
    // limit. The limit holds the estimate of the visible sends and receives of
    // the term. The seven calls of the contract start seven registry lookups,
    // which together cost more than the limit, so the run exhausts the signed
    // limit, a charged user failure that stays in the block (DR-113). The
    // `add_block_from_deploys` call validates the block, so its replay must
    // match play.
    // let deploy = construct_deploy::source_deploy(
    //     MULTI_BRANCH_SAMPLE_TERM_WITH_ERROR.to_string(),
    //     timestamp,
    //     Some(20000), // Not enough phlo
    //     None,
    //     None,
    //     None,
    //     Some(ctx.genesis_context.genesis_block.shard_id.clone()),
    // )
    // .expect("Failed to create deploy");
    let phlo_limit = 20_000; // Not enough phlo
    let deploy = OfferedDeploy::from_proto(offered_deploy::owner_offer_limited(
        &(
            construct_deploy::DEFAULT_SEC.clone(),
            construct_deploy::DEFAULT_PUB.clone(),
        ),
        timestamp,
        MULTI_BRANCH_SAMPLE_TERM_WITH_ERROR.to_string(),
        0,
        ctx.genesis_context.genesis_block.shard_id.clone(),
        phlo_limit,
    ));

    let mut node = TestNode::standalone(ctx.genesis_context)
        .await
        .expect("Failed to create standalone node");

    let b = node
        .add_block_from_deploys(&[deploy])
        .await
        .expect("Failed to add block");

    assert_eq!(
        b.body.deploys.len(),
        1,
        "Block should have exactly 1 deploy"
    );

    let deploy_cost = b.body.deploys[0].cost().cost;
    // Changed by DR-134 (DR-66 item 1): the native meter denies the first
    // charge that does not fit the signed limit, so the failed deploy pays
    // the work granted up to the limit, as in
    // `used_deploy_with_insufficient_phlos_should_be_added_to_a_block_with_all_phlos_consumed`
    // above.
    // assert_eq!(
    //     deploy_cost, 20000,
    //     "Deploy should consume all phlos (20000)"
    // );
    assert!(
        b.body.deploys[0].is_failed(),
        "Deploy should exhaust its signed phlo limit"
    );
    assert!(
        deploy_cost > 0 && deploy_cost <= phlo_limit,
        "Deploy should consume its phlos up to the signed limit ({phlo_limit}), got {deploy_cost}"
    );
}

// Disabled by I6 (epic 8946, D3): legacy accounting is forbidden under v6.
// This test asserted the legacy charge of all phlo for a user error in a
// legacy body-only deploy. The offered form is
// `a_user_error_in_one_branch_is_published_as_a_charged_user_failure` in
// `casper/tests/api/offered_funded_api_test.rs`. It checks the adopted policy
// (economic-failure-policy-decisions.md:84) and one equal receipt on every node.
// #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
// async fn replay_should_match_in_case_of_user_execution_error() {
//     let ctx = TestContext::new().await;
//
//     let timestamp = std::time::SystemTime::now()
//         .duration_since(std::time::UNIX_EPOCH)
//         .unwrap()
//         .as_millis() as i64;
//
//     let deploy = construct_deploy::source_deploy(
//         MULTI_BRANCH_SAMPLE_TERM_WITH_ERROR.to_string(),
//         timestamp,
//         Some(300000), //Enough phlo
//         None,
//         None,
//         None,
//         Some(ctx.genesis_context.genesis_block.shard_id.clone()),
//     )
//     .expect("Failed to create deploy");
//
//     let mut node = TestNode::standalone(ctx.genesis_context)
//         .await
//         .expect("Failed to create standalone node");
//
//     let b = node
//         .add_block_from_deploys(&[deploy])
//         .await
//         .expect("Failed to add block");
//
//     assert_eq!(
//         b.body.deploys.len(),
//         1,
//         "Block should have exactly 1 deploy"
//     );
//
//     let deploy_cost = b.body.deploys[0].cost().cost;
//     assert_eq!(
//         deploy_cost, 300000,
//         "Deploy should consume all phlos (300000)"
//     );
// }
