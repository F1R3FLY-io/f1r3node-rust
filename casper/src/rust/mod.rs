pub mod api;
pub mod block_status;
pub mod blocks;
pub mod casper;
pub mod causal_equivocation;
pub mod casper_conf;
pub mod engine;
pub mod epoch;
pub mod equivocation_detector;
pub mod errors;
pub mod estimator;
pub mod finality;
pub mod genesis;
pub mod heartbeat_signal;
pub mod helper;
pub mod last_finalized_height_constraint_checker;
pub mod merging;
pub mod metrics_constants;
pub mod protocol;
pub mod report_store;
pub mod reporting_casper;
pub mod reporting_proto_transformer;
pub mod rholang;
pub mod safety;
pub mod safety_oracle;
pub mod slashing_authorization;
pub mod state;
pub mod storage;
pub mod synchrony_constraint_checker;
pub mod system_deploy;
#[cfg(test)]
pub(crate) mod test_metadata;
pub mod util;
pub mod validate;
pub mod validator_identity;

// Test utilities module - only available when "test-utils" feature is enabled
#[cfg(feature = "test-utils")]
pub mod test_utils;

// See casper/src/main/scala/coop/rchain/casper/package.scala

use std::future::Future;
use std::pin::Pin;

use models::rust::block_hash::BlockHash;
use models::rust::validator::Validator;
use rspace_plus_plus::rspace::history::Either;

use crate::rust::block_status::{BlockError, ValidBlock};
use crate::rust::blocks::proposer::proposer::ProposerResult;
use crate::rust::errors::CasperError;

pub type TopoSort = Vec<Vec<BlockHash>>;

pub type BlockProcessing<A> = Either<BlockError, A>;

pub type ValidBlockProcessing = BlockProcessing<ValidBlock>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalityRecoveryPermit {
    pub lfb_hash: BlockHash,
    pub lfb_height: i64,
    pub recovery_round: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProposeRequestKind {
    Manual,
    PendingDeploy,
    FinalityRecovery(FinalityRecoveryPermit),
}

pub fn finality_recovery_leader(
    mut validators: Vec<Validator>,
    lfb_height: i64,
    recovery_round: u64,
) -> Option<Validator> {
    if validators.is_empty() {
        return None;
    }

    validators.sort_unstable();
    validators.dedup();

    let finalized_height = u128::try_from(lfb_height).ok()?;
    let validator_count = u128::try_from(validators.len()).ok()?;
    let leader_index =
        usize::try_from((finalized_height + u128::from(recovery_round)) % validator_count).ok()?;
    validators.get(leader_index).cloned()
}

pub type ProposeFunction = dyn Fn(
        ProposeRequestKind,
    ) -> Pin<Box<dyn Future<Output = Result<ProposerResult, CasperError>> + Send>>
    + Send
    + Sync;
