// See casper/src/test/scala/coop/rchain/casper/addblock/ProposerSpec.scala

use std::sync::Arc;

use casper::rust::blocks::proposer::propose_result::{
    BlockCreatorResult, CheckProposeConstraintsResult,
};
use casper::rust::blocks::proposer::proposer::{
    ActiveValidatorChecker, BlockCreator, BlockValidator, CasperSnapshotProvider, HeightChecker,
    ProposeEffectHandler, ProposeReturnType, Proposer, StakeChecker,
};
use casper::rust::casper::{Casper, CasperSnapshot};
use casper::rust::errors::CasperError;
use casper::rust::validator_identity::ValidatorIdentity;
use casper::rust::ValidBlockProcessing;
use crypto::rust::private_key::PrivateKey;
use models::rust::casper::protocol::casper_message::BlockMessage;

use crate::helper::block_dag_storage_fixture::with_storage;
use crate::util::genesis_builder::DEFAULT_VALIDATOR_SKS;
use crate::util::rholang::resources::{mk_dummy_casper_snapshot, mk_runtime_manager};

// Test implementations for different scenarios
pub struct TestCasperSnapshotProvider;
impl CasperSnapshotProvider for TestCasperSnapshotProvider {
    async fn get_casper_snapshot(
        &self,
        _: Arc<dyn Casper + Send + Sync + 'static>,
    ) -> Result<CasperSnapshot, CasperError> {
        Ok(mk_dummy_casper_snapshot())
    }
}

/// A node whose history does not reach far enough to build a snapshot at all.
pub struct HistoryIncompleteSnapshotProvider;
impl CasperSnapshotProvider for HistoryIncompleteSnapshotProvider {
    async fn get_casper_snapshot(
        &self,
        _: Arc<dyn Casper + Send + Sync + 'static>,
    ) -> Result<CasperSnapshot, CasperError> {
        Err(CasperError::BlockNotHeld(
            models::rust::block_hash::BlockHash::from(b"below-my-anchor".to_vec()),
            String::new(),
        ))
    }
}

pub struct AlwaysNotActiveChecker;
impl ActiveValidatorChecker for AlwaysNotActiveChecker {
    fn check_active_validator(
        &self,
        _: &CasperSnapshot,
        _: &ValidatorIdentity,
    ) -> CheckProposeConstraintsResult {
        CheckProposeConstraintsResult::not_bonded()
    }
}

pub struct AlwaysActiveChecker;
impl ActiveValidatorChecker for AlwaysActiveChecker {
    fn check_active_validator(
        &self,
        _: &CasperSnapshot,
        _: &ValidatorIdentity,
    ) -> CheckProposeConstraintsResult {
        CheckProposeConstraintsResult::success()
    }
}

pub struct AlwaysNotEnoughBlocksStakeChecker;
impl StakeChecker for AlwaysNotEnoughBlocksStakeChecker {
    async fn check_enough_base_stake(
        &self,
        _: &CasperSnapshot,
    ) -> Result<CheckProposeConstraintsResult, CasperError> {
        Ok(CheckProposeConstraintsResult::not_enough_new_block())
    }
}

pub struct AlwaysTooFarAheadChecker;
impl HeightChecker for AlwaysTooFarAheadChecker {
    async fn check_finalized_height(
        &self,
        _: &CasperSnapshot,
    ) -> Result<CheckProposeConstraintsResult, CasperError> {
        Ok(CheckProposeConstraintsResult::too_far_ahead_of_last_finalized())
    }
}

pub struct OkProposeConstraintStakeChecker;
impl StakeChecker for OkProposeConstraintStakeChecker {
    async fn check_enough_base_stake(
        &self,
        _: &CasperSnapshot,
    ) -> Result<CheckProposeConstraintsResult, CasperError> {
        Ok(CheckProposeConstraintsResult::success())
    }
}

pub struct OkHeightChecker;
impl HeightChecker for OkHeightChecker {
    async fn check_finalized_height(
        &self,
        _: &CasperSnapshot,
    ) -> Result<CheckProposeConstraintsResult, CasperError> {
        Ok(CheckProposeConstraintsResult::success())
    }
}

/// Records the selection the proposer asked for, then behaves like
/// `TestBlockCreator`.
pub struct SelectionRecordingBlockCreator(
    pub Arc<std::sync::Mutex<Option<casper::rust::blocks::proposer::proposer::DeploySelection>>>,
);
impl BlockCreator for SelectionRecordingBlockCreator {
    async fn create_block(
        &mut self,
        _: &CasperSnapshot,
        _: &ValidatorIdentity,
        _: Option<(PrivateKey, String)>,
        selection: casper::rust::blocks::proposer::proposer::DeploySelection,
    ) -> Result<BlockCreatorResult, CasperError> {
        *self.0.lock().unwrap() = Some(selection);
        use models::rust::block_implicits::get_random_block_default;
        Ok(BlockCreatorResult::Created(
            get_random_block_default(),
            prost::bytes::Bytes::new(),
            prost::bytes::Bytes::new(),
        ))
    }
}

pub struct TestBlockCreator;
impl BlockCreator for TestBlockCreator {
    async fn create_block(
        &mut self,
        _: &CasperSnapshot,
        _: &ValidatorIdentity,
        _: Option<(PrivateKey, String)>,
        _: casper::rust::blocks::proposer::proposer::DeploySelection,
    ) -> Result<BlockCreatorResult, CasperError> {
        use models::rust::block_implicits::get_random_block_default;
        Ok(BlockCreatorResult::Created(
            get_random_block_default(),
            prost::bytes::Bytes::new(),
            prost::bytes::Bytes::new(),
        ))
    }
}

pub struct TestBlockValidator;
impl BlockValidator for TestBlockValidator {
    async fn validate_block(
        &self,
        _: Arc<dyn Casper + Send + Sync + 'static>,
        _: &mut CasperSnapshot,
        _: &BlockMessage,
    ) -> Result<ValidBlockProcessing, CasperError> {
        use casper::rust::block_status::ValidBlock;
        Ok(ValidBlockProcessing::Right(ValidBlock::Valid))
    }
}

pub struct AlwaysUnsuccessfulValidator;
impl BlockValidator for AlwaysUnsuccessfulValidator {
    async fn validate_block(
        &self,
        _: Arc<dyn Casper + Send + Sync + 'static>,
        _: &mut CasperSnapshot,
        _: &BlockMessage,
    ) -> Result<ValidBlockProcessing, CasperError> {
        use casper::rust::block_status::{BlockError, InvalidBlock};
        Ok(ValidBlockProcessing::Left(BlockError::Invalid(
            InvalidBlock::InvalidFormat,
        )))
    }
}

pub struct TestProposeEffectHandler;
impl ProposeEffectHandler for TestProposeEffectHandler {
    async fn handle_propose_effect(
        &mut self,
        _: Arc<dyn Casper + Send + Sync + 'static>,
        _: &BlockMessage,
    ) -> Result<(), CasperError> {
        Ok(())
    }

    fn publish_block_created(&self, _: &BlockMessage) -> Result<(), CasperError> { Ok(()) }
}

use std::sync::atomic::{AtomicI32, Ordering};

// Global variable to track propose effects (similar to proposeEffectVar in Scala)
static PROPOSE_EFFECT_VAR: AtomicI32 = AtomicI32::new(0);

pub struct TrackingProposeEffectHandler {
    value: i32,
}

impl TrackingProposeEffectHandler {
    pub fn new(value: i32) -> Self { Self { value } }
}

impl ProposeEffectHandler for TrackingProposeEffectHandler {
    async fn handle_propose_effect(
        &mut self,
        _: Arc<dyn Casper + Send + Sync + 'static>,
        _: &BlockMessage,
    ) -> Result<(), CasperError> {
        PROPOSE_EFFECT_VAR.store(self.value, Ordering::SeqCst);
        Ok(())
    }

    fn publish_block_created(&self, _: &BlockMessage) -> Result<(), CasperError> { Ok(()) }
}

fn get_propose_effect_var() -> i32 { PROPOSE_EFFECT_VAR.load(Ordering::SeqCst) }

fn reset_propose_effect_var() { PROPOSE_EFFECT_VAR.store(0, Ordering::SeqCst); }

fn dummy_validator_identity() -> ValidatorIdentity {
    ValidatorIdentity::new(&DEFAULT_VALIDATOR_SKS[0])
}

#[tokio::test]
async fn proposer_should_reject_to_propose_if_proposer_is_not_active_validator() {
    with_storage(|block_store, block_dag_storage| async move {
        let runtime_manager = mk_runtime_manager("block-query-response-api-test", None).await;
        let validator_identity = Arc::new(dummy_validator_identity());

        let mut proposer = Proposer::new(
            validator_identity,
            None,
            TestCasperSnapshotProvider,
            AlwaysNotActiveChecker,
            OkProposeConstraintStakeChecker,
            OkHeightChecker,
            TestBlockCreator,
            TestBlockValidator,
            TestProposeEffectHandler,
            false, // allow_empty_blocks
        );

        use std::collections::HashMap;

        use crate::helper::no_ops_casper_effect::NoOpsCasperEffect;

        let dag_representation = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let casper = Arc::new(NoOpsCasperEffect::new(
            Some(HashMap::new()),
            None,
            Arc::new(runtime_manager),
            block_store,
            dag_representation,
        ));

        let result = proposer.propose(casper, false).await;

        match result {
            Ok(ProposeReturnType {
                propose_result,
                propose_result_to_send: _,
                block_message_opt,
            }) => {
                use casper::rust::blocks::proposer::propose_result::{
                    CheckProposeConstraintsFailure, ProposeFailure, ProposeStatus,
                };

                assert!(matches!(
                    propose_result.propose_status,
                    ProposeStatus::Failure(ProposeFailure::CheckConstraintsFailure(
                        CheckProposeConstraintsFailure::NotBonded
                    ))
                ));
                assert!(block_message_opt.is_none());
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    })
    .await
}

/// Building the snapshot walks the same history the floor does, so a node whose
/// history is cut short fails there before any propose constraint is consulted —
/// the constraint that would have stopped it lives *inside* the snapshot it
/// cannot build. Left as an error, every propose attempt re-runs the whole
/// failing walk: one wedged joiner did this 4,563 times in an hour, pinning a
/// core. Not being able to build a snapshot is a reason not to propose, so it
/// must read as one.
#[tokio::test]
async fn proposer_should_reject_to_propose_if_its_history_is_incomplete() {
    with_storage(|block_store, block_dag_storage| async move {
        let runtime_manager = mk_runtime_manager("block-query-response-api-test", None).await;
        let validator_identity = Arc::new(dummy_validator_identity());

        let mut proposer = Proposer::new(
            validator_identity,
            None,
            HistoryIncompleteSnapshotProvider,
            AlwaysActiveChecker,
            OkProposeConstraintStakeChecker,
            OkHeightChecker,
            TestBlockCreator,
            TestBlockValidator,
            TestProposeEffectHandler,
            false,
        );

        use std::collections::HashMap;

        use crate::helper::no_ops_casper_effect::NoOpsCasperEffect;

        let dag_representation = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let casper = Arc::new(NoOpsCasperEffect::new(
            Some(HashMap::new()),
            None,
            Arc::new(runtime_manager),
            block_store,
            dag_representation,
        ));

        match proposer.propose(casper, false).await {
            Ok(ProposeReturnType {
                propose_result,
                propose_result_to_send: _,
                block_message_opt,
            }) => {
                use casper::rust::blocks::proposer::propose_result::{
                    CheckProposeConstraintsFailure, ProposeFailure, ProposeStatus,
                };

                assert!(
                    matches!(
                        propose_result.propose_status,
                        ProposeStatus::Failure(ProposeFailure::CheckConstraintsFailure(
                            CheckProposeConstraintsFailure::HistoryIncomplete
                        ))
                    ),
                    "a node that cannot build a snapshot must decline to propose, not error; \
                     got {:?}",
                    propose_result.propose_status
                );
                assert!(block_message_opt.is_none());
            }
            Err(e) => panic!(
                "incomplete history must not surface as a propose error: {:?}",
                e
            ),
        }
    })
    .await
}

#[tokio::test]
async fn proposer_should_reject_to_propose_if_synchrony_constraint_not_met() {
    with_storage(|block_store, block_dag_storage| async move {
        let runtime_manager = mk_runtime_manager("block-query-response-api-test", None).await;
        let validator_identity = Arc::new(dummy_validator_identity());

        let mut proposer = Proposer::new(
            validator_identity,
            None,
            TestCasperSnapshotProvider,
            AlwaysActiveChecker,               // permissive - validator is active
            AlwaysNotEnoughBlocksStakeChecker, // synchrony constraint is not met
            OkHeightChecker,                   // permissive
            TestBlockCreator,
            TestBlockValidator,
            TestProposeEffectHandler,
            false, // allow_empty_blocks
        );

        use std::collections::HashMap;

        use crate::helper::no_ops_casper_effect::NoOpsCasperEffect;

        let dag_representation = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let casper = Arc::new(NoOpsCasperEffect::new(
            Some(HashMap::new()),
            None,
            Arc::new(runtime_manager),
            block_store,
            dag_representation,
        ));

        let result = proposer.propose(casper, false).await;

        match result {
            Ok(ProposeReturnType {
                propose_result,
                propose_result_to_send: _,
                block_message_opt,
            }) => {
                use casper::rust::blocks::proposer::propose_result::{
                    CheckProposeConstraintsFailure, ProposeFailure, ProposeStatus,
                };

                assert!(matches!(
                    propose_result.propose_status,
                    ProposeStatus::Failure(ProposeFailure::CheckConstraintsFailure(
                        CheckProposeConstraintsFailure::NotEnoughNewBlocks
                    ))
                ));
                assert!(block_message_opt.is_none());
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    })
    .await
}

/// Finality frozen with this validator's latest message 1001 blocks above
/// it and an on-chain threshold of 1000: the real height checker refuses,
/// and if the recovery lane is gated by that refusal no proposal can ever
/// land — the deadlock is only exitable by a chain reset. The recovery lane
/// must mint an empty block through the gate.
#[tokio::test]
async fn lag_past_the_height_threshold_still_mints_recovery() {
    use casper::rust::casper::test_helpers::TestCasperWithSnapshot;
    use casper::rust::last_finalized_height_constraint_checker;
    use models::rust::block_metadata::BlockMetadata;

    fn build_lagged_snapshot(validator_id: &prost::bytes::Bytes) -> CasperSnapshot {
        let mut snapshot = TestCasperWithSnapshot::create_empty_snapshot();
        TestCasperWithSnapshot::bond_validator_in_snapshot(&mut snapshot, validator_id.clone());

        let put_meta = |snapshot: &mut CasperSnapshot,
                        hash: prost::bytes::Bytes,
                        number: i64,
                        finalized: bool| {
            let meta = BlockMetadata {
                block_hash: hash.clone(),
                parents: Vec::new(),
                sender: validator_id.clone(),
                justifications: Vec::new(),
                weight_map: std::collections::BTreeMap::new(),
                block_number: number,
                sequence_number: number as i32,
                invalid: false,
                directly_finalized: finalized,
                finalized,
                fault_tolerance_value: 1.0,
                merge_base: prost::bytes::Bytes::new(),
            };
            snapshot.dag.dag_set.insert(hash.clone());
            snapshot.dag.block_number_map.insert(hash.clone(), number);
            snapshot
                .dag
                .block_metadata_index
                .write()
                .add(meta)
                .expect("add metadata");
            hash
        };

        let lfb = put_meta(
            &mut snapshot,
            prost::bytes::Bytes::from(vec![0x0Fu8; 32]),
            117_766,
            true,
        );
        snapshot.dag.finalized_blocks_set.insert(lfb.clone());
        snapshot.dag.last_finalized_block_hash = lfb.clone();
        snapshot.last_finalized_block = lfb;

        let own_latest = put_meta(
            &mut snapshot,
            prost::bytes::Bytes::from(vec![0x7Au8; 32]),
            118_767,
            false,
        );
        snapshot
            .dag
            .latest_messages_map
            .insert(validator_id.clone(), own_latest);

        snapshot.max_block_num = 118_767;
        snapshot
            .on_chain_state
            .shard_conf
            .height_constraint_threshold = 1000;
        snapshot
    }

    let validator_identity = Arc::new(dummy_validator_identity());
    let validator_id = validator_identity.public_key.bytes.clone();

    // The gate itself: diff 1001 > threshold 1000.
    let refused = last_finalized_height_constraint_checker::check(
        &build_lagged_snapshot(&validator_id),
        &validator_identity,
    )
    .expect("checker");
    assert!(
        matches!(
            refused,
            CheckProposeConstraintsResult::Failure(
                casper::rust::blocks::proposer::propose_result::CheckProposeConstraintsFailure::TooFarAheadOfLastFinalized
            )
        ),
        "1001 over a 1000 threshold must refuse, got {refused:?}"
    );

    // The recovery lane through the real checker mints the empty recovery block.
    struct LaggedSnapshotProvider(prost::bytes::Bytes);
    impl CasperSnapshotProvider for LaggedSnapshotProvider {
        async fn get_casper_snapshot(
            &self,
            _: Arc<dyn Casper + Send + Sync + 'static>,
        ) -> Result<CasperSnapshot, CasperError> {
            Ok(build_lagged_snapshot(&self.0))
        }
    }

    with_storage(|block_store, block_dag_storage| async move {
        let runtime_manager = mk_runtime_manager("block-query-response-api-test", None).await;
        let recorded = Arc::new(std::sync::Mutex::new(None));

        let mut proposer = Proposer::new(
            validator_identity,
            None,
            LaggedSnapshotProvider(validator_id),
            AlwaysActiveChecker,
            OkProposeConstraintStakeChecker,
            casper::rust::blocks::proposer::proposer::ProductionHeightChecker::new(Arc::new(
                dummy_validator_identity(),
            )),
            SelectionRecordingBlockCreator(recorded.clone()),
            TestBlockValidator,
            TestProposeEffectHandler,
            true,
        );

        use std::collections::HashMap;

        use crate::helper::no_ops_casper_effect::NoOpsCasperEffect;

        let dag_representation = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let casper = Arc::new(NoOpsCasperEffect::new(
            Some(HashMap::new()),
            None,
            Arc::new(runtime_manager),
            block_store,
            dag_representation,
        ));

        let result = proposer.propose(casper, true).await.expect("propose");
        assert!(
            result.block_message_opt.is_some(),
            "the recovery lane must mint through the real height gate, got {:?}",
            result.propose_result.propose_status
        );
        assert_eq!(
            *recorded.lock().unwrap(),
            Some(casper::rust::blocks::proposer::proposer::DeploySelection::RecoveryEmpty),
        );
    })
    .await
}

/// The height constraint must not gate the recovery lane it exists to be
/// rescued by: on the heartbeat lane the gate degrades to an empty recovery
/// mint instead of a failure.
#[tokio::test]
async fn height_constraint_degrades_to_an_empty_recovery_mint_on_the_recovery_lane() {
    with_storage(|block_store, block_dag_storage| async move {
        let runtime_manager = mk_runtime_manager("block-query-response-api-test", None).await;
        let validator_identity = Arc::new(dummy_validator_identity());
        let recorded = Arc::new(std::sync::Mutex::new(None));

        let mut proposer = Proposer::new(
            validator_identity,
            None,
            TestCasperSnapshotProvider,
            AlwaysActiveChecker,
            OkProposeConstraintStakeChecker,
            AlwaysTooFarAheadChecker,
            SelectionRecordingBlockCreator(recorded.clone()),
            TestBlockValidator,
            TestProposeEffectHandler,
            true, // allow_empty_blocks: the heartbeat/recovery lane
        );

        use std::collections::HashMap;

        use crate::helper::no_ops_casper_effect::NoOpsCasperEffect;

        let dag_representation = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let casper = Arc::new(NoOpsCasperEffect::new(
            Some(HashMap::new()),
            None,
            Arc::new(runtime_manager),
            block_store,
            dag_representation,
        ));

        let result = proposer.propose(casper, true).await.expect("propose");
        assert!(
            result.block_message_opt.is_some(),
            "the recovery lane must mint a block past the height gate, got {:?}",
            result.propose_result.propose_status
        );
        assert_eq!(
            *recorded.lock().unwrap(),
            Some(casper::rust::blocks::proposer::proposer::DeploySelection::RecoveryEmpty),
            "the minted block must carry no user deploys"
        );
    })
    .await
}

#[tokio::test]
async fn proposer_should_reject_to_propose_if_last_finalized_height_constraint_not_met() {
    with_storage(|block_store, block_dag_storage| async move {
        let runtime_manager = mk_runtime_manager("block-query-response-api-test", None).await;
        let validator_identity = Arc::new(dummy_validator_identity());

        let mut proposer = Proposer::new(
            validator_identity,
            None,
            TestCasperSnapshotProvider,
            AlwaysActiveChecker,             // permissive - validator is active
            OkProposeConstraintStakeChecker, // permissive
            AlwaysTooFarAheadChecker,        // height constraint is not met
            TestBlockCreator,
            TestBlockValidator,
            TestProposeEffectHandler,
            false, // allow_empty_blocks
        );

        use std::collections::HashMap;

        use crate::helper::no_ops_casper_effect::NoOpsCasperEffect;

        let dag_representation = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let casper = Arc::new(NoOpsCasperEffect::new(
            Some(HashMap::new()),
            None,
            Arc::new(runtime_manager),
            block_store,
            dag_representation,
        ));

        let result = proposer.propose(casper, false).await;

        match result {
            Ok(ProposeReturnType {
                propose_result,
                propose_result_to_send: _,
                block_message_opt,
            }) => {
                use casper::rust::blocks::proposer::propose_result::{
                    CheckProposeConstraintsFailure, ProposeFailure, ProposeStatus,
                };

                assert!(matches!(
                    propose_result.propose_status,
                    ProposeStatus::Failure(ProposeFailure::CheckConstraintsFailure(
                        CheckProposeConstraintsFailure::TooFarAheadOfLastFinalized
                    ))
                ));
                assert!(block_message_opt.is_none());
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    })
    .await
}

#[tokio::test]
async fn proposer_should_shut_down_the_node_if_block_created_is_not_successfully_replayed() {
    with_storage(|block_store, block_dag_storage| async move {
        let runtime_manager = mk_runtime_manager("block-query-response-api-test", None).await;
        let validator_identity = Arc::new(dummy_validator_identity());

        let mut proposer = Proposer::new(
            validator_identity,
            None,
            TestCasperSnapshotProvider,
            AlwaysActiveChecker,             // permissive - validator is active
            OkProposeConstraintStakeChecker, // permissive
            OkHeightChecker,                 // permissive
            TestBlockCreator,                // creates a block
            AlwaysUnsuccessfulValidator,     // validation fails
            TestProposeEffectHandler,        // handles effects
            false,                           // allow_empty_blocks
        );

        use std::collections::HashMap;

        use crate::helper::no_ops_casper_effect::NoOpsCasperEffect;

        let dag_representation = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let casper = Arc::new(NoOpsCasperEffect::new_with_self_created_validation_failure(
            Some(HashMap::new()),
            None,
            Arc::new(runtime_manager),
            block_store,
            dag_representation,
        ));

        let result = proposer.propose(casper, false).await;

        // Should return an error when block validation fails
        match result {
            Ok(_) => panic!("Expected error when block validation fails"),
            Err(e) => {
                let error_msg = format!("{:?}", e);
                assert!(error_msg.contains("Validation of self created block failed"));
            }
        }
    })
    .await
}

#[tokio::test]
async fn proposer_should_execute_propose_effects_if_block_created_successfully_replayed() {
    with_storage(|block_store, block_dag_storage| async move {
        // Reset the effect variable before test
        reset_propose_effect_var();

        let runtime_manager = mk_runtime_manager("block-query-response-api-test", None).await;
        let validator_identity = Arc::new(dummy_validator_identity());

        let mut proposer = Proposer::new(
            validator_identity,
            None,
            TestCasperSnapshotProvider,
            AlwaysActiveChecker,             // permissive - validator is active
            OkProposeConstraintStakeChecker, // permissive
            OkHeightChecker,                 // permissive
            TestBlockCreator,                // creates a block
            TestBlockValidator,              // validates successfully
            TrackingProposeEffectHandler::new(10), // tracks effects with value 10
            false,                           // allow_empty_blocks
        );

        use std::collections::HashMap;

        use crate::helper::no_ops_casper_effect::NoOpsCasperEffect;

        let dag_representation = block_dag_storage
            .get_representation()
            .expect("dag representation");
        let casper = Arc::new(NoOpsCasperEffect::new(
            Some(HashMap::new()),
            None,
            Arc::new(runtime_manager),
            block_store,
            dag_representation,
        ));

        let result = proposer.propose(casper, false).await;

        match result {
            Ok(ProposeReturnType {
                propose_result,
                propose_result_to_send: _,
                block_message_opt,
            }) => {
                use casper::rust::block_status::ValidBlock;
                use casper::rust::blocks::proposer::propose_result::{
                    ProposeStatus, ProposeSuccess,
                };

                assert!(matches!(
                    propose_result.propose_status,
                    ProposeStatus::Success(ProposeSuccess {
                        result: ValidBlock::Valid
                    })
                ));
                assert!(block_message_opt.is_some());
                // Verify that the propose effect was executed
                assert_eq!(get_propose_effect_var(), 10);
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    })
    .await
}
