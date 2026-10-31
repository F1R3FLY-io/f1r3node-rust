use async_trait::async_trait;
use crypto::rust::public_key::PublicKey;
use models::rust::casper::protocol::casper_message::BlockMessage;
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

pub(super) fn close_block(block: &BlockMessage) -> SystemDeployEnum {
    SystemDeployEnum::Close(CloseBlockDeploy {
        initial_rand: system_deploy_util::generate_close_deploy_random_seed_from_pk(
            PublicKey::from_bytes(&block.sender),
            block.seq_num,
        ),
    })
}

#[cfg(test)]
mod tests {
    use models::rust::casper::protocol::casper_message::{Body, F1r3flyState, Header};
    use prost::bytes::Bytes;

    use super::*;

    fn empty_block(timestamp: i64, block_number: i64, seq_num: i32) -> BlockMessage {
        BlockMessage {
            block_hash: Bytes::new(),
            header: Header {
                parents_hash_list: vec![],
                timestamp,
                version: 1,
                extra_bytes: Bytes::new(),
            },
            body: Body {
                state: F1r3flyState {
                    pre_state_hash: Bytes::new(),
                    post_state_hash: Bytes::new(),
                    bonds: vec![],
                    block_number,
                },
                deploys: vec![],
                rejected_deploys: vec![],
                system_deploys: vec![],
                extra_bytes: Bytes::new(),
                applied_from_scope: vec![],
                merge_base: Bytes::new(),
            },
            justifications: vec![],
            sender: vec![0xAAu8; 33].into(),
            seq_num,
            sig: Bytes::new(),
            sig_algorithm: String::new(),
            shard_id: String::new(),
            extra_bytes: Bytes::new(),
        }
    }

    #[test]
    fn block_data_mirrors_the_block_header_and_sender() {
        let block = empty_block(1_700_000_000, 42, 7);
        let data = block_data(&block);
        assert_eq!(data.time_stamp, 1_700_000_000);
        assert_eq!(data.block_number, 42);
        assert_eq!(data.seq_num, 7);
        assert_eq!(data.sender.bytes.as_ref(), block.sender.as_ref());
    }

    #[test]
    fn the_close_block_seed_follows_the_sender_and_sequence_number() {
        let seed = |block: &BlockMessage| match close_block(block) {
            SystemDeployEnum::Close(close) => close.initial_rand,
            _ => panic!("expected the close-block system deploy"),
        };
        let block = empty_block(1, 1, 7);
        assert_eq!(
            seed(&block),
            system_deploy_util::generate_close_deploy_random_seed_from_pk(
                PublicKey::from_bytes(&block.sender),
                7,
            )
        );
        assert_eq!(seed(&block), seed(&empty_block(2, 9, 7)));
        assert_ne!(seed(&block), seed(&empty_block(1, 1, 8)));
    }
}
