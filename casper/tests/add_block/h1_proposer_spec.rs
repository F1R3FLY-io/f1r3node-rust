use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use casper::rust::blocks::proposer::propose_result::{
    BlockCreatorResult, CheckProposeConstraintsResult,
};
use casper::rust::blocks::proposer::proposer::{
    block_has_user_deploys, BlockCreator, DeploySelection, HeightChecker, ProposeEffectHandler,
    ProposeRequest, Proposer,
};
use casper::rust::casper::{Casper, CasperSnapshot};
use casper::rust::errors::CasperError;
use casper::rust::validator_identity::ValidatorIdentity;
use crypto::rust::private_key::PrivateKey;
use models::rust::casper::protocol::casper_message::{BlockMessage, RejectedDeploy};

use super::proposer_spec::{
    AlwaysActiveChecker, OkProposeConstraintStakeChecker, TestBlockValidator,
    TestCasperSnapshotProvider,
};
use crate::helper::block_dag_storage_fixture::with_storage;
use crate::helper::no_ops_casper_effect::NoOpsCasperEffect;
use crate::util::genesis_builder::DEFAULT_VALIDATOR_SKS;
use crate::util::rholang::resources::mk_runtime_manager;

struct FixedCreator {
    block: BlockMessage,
    selection: Arc<Mutex<Option<DeploySelection>>>,
}

impl BlockCreator for FixedCreator {
    async fn create_block(
        &mut self,
        _: &CasperSnapshot,
        _: &ValidatorIdentity,
        _: Option<(PrivateKey, String)>,
        selection: DeploySelection,
    ) -> Result<BlockCreatorResult, CasperError> {
        *self.selection.lock().unwrap() = Some(selection);
        Ok(BlockCreatorResult::Created(
            self.block.clone(),
            Vec::new().into(),
            Vec::new().into(),
        ))
    }
}

struct CheckedHeight(bool);
impl HeightChecker for CheckedHeight {
    async fn check_finalized_height(
        &self,
        _: &CasperSnapshot,
    ) -> Result<CheckProposeConstraintsResult, CasperError> {
        Ok(if self.0 {
            CheckProposeConstraintsResult::too_far_ahead_of_last_finalized()
        } else {
            CheckProposeConstraintsResult::success()
        })
    }
}

struct CountEffects(Arc<AtomicUsize>);
impl ProposeEffectHandler for CountEffects {
    async fn handle_propose_effect(
        &mut self,
        _: Arc<dyn Casper + Send + Sync>,
        _: &BlockMessage,
    ) -> Result<(), CasperError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn publish_block_created(&self, _: &BlockMessage) -> Result<(), CasperError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

async fn verify_nonempty_request(kind: u8, too_far: bool, stale: bool) {
    with_storage(|block_store, block_dag_storage| async move {
        let runtime = mk_runtime_manager("h1-nonempty-request", None).await;
        let mut block = models::rust::block_implicits::get_random_block_default();
        block.body.deploys.clear();
        block.body.rejected_deploys.clear();
        if kind == 1 || kind == 2 {
            let mut deploy =
                casper::rust::util::construct_deploy::basic_processed_deploy(1, None).unwrap();
            if kind == 1 {
                let mut marker = vec![0; 33];
                marker[32] = casper::rust::system_deploy::HEARTBEAT_MARKER;
                deploy.deploy.sig = marker.into();
            } else {
                deploy.is_failed = true;
            }
            block.body.deploys.push(deploy);
        } else if kind == 3 {
            block.body.rejected_deploys.push(RejectedDeploy {
                sig: vec![1; 64].into(),
                duplicate: false,
                carrier: vec![2; 32].into(),
            });
        }
        let has_user = block_has_user_deploys(&block);
        assert_eq!(has_user, kind == 2);
        let selection = Arc::new(Mutex::new(None));
        let effects = Arc::new(AtomicUsize::new(0));
        let mut proposer = Proposer::new(
            Arc::new(ValidatorIdentity::new(&DEFAULT_VALIDATOR_SKS[0])),
            None,
            TestCasperSnapshotProvider,
            AlwaysActiveChecker,
            OkProposeConstraintStakeChecker,
            CheckedHeight(too_far),
            FixedCreator {
                block,
                selection: selection.clone(),
            },
            TestBlockValidator,
            CountEffects(effects.clone()),
            true,
        );
        let casper = Arc::new(NoOpsCasperEffect::new(
            Some(HashMap::new()),
            None,
            Arc::new(runtime),
            block_store,
            block_dag_storage.get_representation().unwrap(),
        ));
        let request = if stale {
            ProposeRequest::heartbeat_deploy().after_empty_self(vec![0x7a; 32].into())
        } else {
            ProposeRequest::heartbeat_deploy()
        };
        let result = proposer.propose(casper, request).await.unwrap();
        assert_eq!(
            result.block_message_opt.is_some(),
            has_user && !too_far && !stale
        );
        assert_eq!(
            effects.load(Ordering::SeqCst),
            if has_user && !too_far && !stale { 2 } else { 0 }
        );
        if too_far || stale {
            assert!(selection.lock().unwrap().is_none());
        } else {
            assert_eq!(*selection.lock().unwrap(), Some(DeploySelection::Standard));
            if !has_user {
                assert!(result.propose_result.is_no_new_deploys());
            }
        }
    })
    .await;
}

#[tokio::test]
async fn h1_rechecks_actual_deploys_before_publication() {
    for kind in 0..4 {
        verify_nonempty_request(kind, false, false).await;
    }
}

#[tokio::test]
async fn h1_does_not_gain_empty_recovery_authority_past_the_height_limit() {
    verify_nonempty_request(2, true, false).await;
}

#[tokio::test]
async fn h1_revalidates_the_previous_empty_block_after_queueing() {
    verify_nonempty_request(2, false, true).await;
}
