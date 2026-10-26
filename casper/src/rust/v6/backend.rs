use async_trait::async_trait;
use crypto::rust::public_key::PublicKey;
use models::rust::casper::protocol::casper_message::BlockMessage;
use rholang::rust::interpreter::errors::InterpreterError;
use rholang::rust::interpreter::system_processes::BlockData;

use super::body::StateHash;
use super::record::ExecutionRecord;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use crate::rust::util::rholang::system_deploy_enum::SystemDeployEnum;
use crate::rust::util::rholang::system_deploy_util;

#[async_trait]
pub trait ExecutionBackend: Send + Sync {
    type Record: ExecutionRecord;

    async fn execute(
        &self,
        pre_root: &StateHash,
        block: &BlockMessage,
    ) -> Result<Self::Record, CasperError>;
    async fn replay(
        &self,
        record: &Self::Record,
        block: &BlockMessage,
    ) -> Result<StateHash, CasperError>;
}

pub(super) fn block_data(block: &BlockMessage) -> BlockData {
    BlockData {
        time_stamp: block.header.timestamp,
        block_number: block.body.state.block_number,
        sender: PublicKey::from_bytes(&block.sender),
        seq_num: block.seq_num,
    }
}

pub(crate) fn exhausts(error: &InterpreterError) -> bool {
    match error {
        InterpreterError::OutOfPhlogistonsError => true,
        InterpreterError::AggregateError { interpreter_errors } => {
            interpreter_errors.iter().any(exhausts)
        }
        _ => false,
    }
}

pub(super) fn close_block(block: &BlockMessage) -> SystemDeployEnum {
    SystemDeployEnum::Close(CloseBlockDeploy {
        initial_rand: system_deploy_util::generate_close_deploy_random_seed_from_pk(
            PublicKey::from_bytes(&block.sender),
            block.seq_num,
        ),
    })
}
