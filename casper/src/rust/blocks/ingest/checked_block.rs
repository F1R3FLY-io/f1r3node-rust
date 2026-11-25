use models::rust::casper::protocol::casper_message::BlockMessage;

use crate::rust::block_status::InvalidBlock;
use crate::rust::metrics_constants::BLOCK_INTEGRITY_REJECTED_METRIC;
use crate::rust::validate::Validate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrityFailure {
    Malformed,
    HashMismatch,
}

impl IntegrityFailure {
    fn reason(self) -> &'static str {
        match self {
            IntegrityFailure::Malformed => "malformed",
            IntegrityFailure::HashMismatch => "hash-mismatch",
        }
    }
}

/// A block whose fields, signature and hash have been checked against its
/// contents. `check` is the only way to get one, so no step keyed by a block's
/// hash acts on a hash that a message only claims.
#[derive(Debug, Clone, PartialEq)]
pub struct CheckedBlock(BlockMessage);

impl CheckedBlock {
    pub fn check(
        block: BlockMessage,
        source: &'static str,
    ) -> Result<Self, (Box<BlockMessage>, IntegrityFailure)> {
        let failure = if !Validate::format_of_fields(&block) {
            IntegrityFailure::Malformed
        } else {
            match Validate::block_hash_and_signature(&block) {
                Ok(()) => return Ok(Self(block)),
                Err(InvalidBlock::InvalidBlockHash) => IntegrityFailure::HashMismatch,
                Err(_) => IntegrityFailure::Malformed,
            }
        };
        metrics::counter!(BLOCK_INTEGRITY_REJECTED_METRIC, "source" => source, "reason" => failure.reason())
            .increment(1);
        Err((Box::new(block), failure))
    }

    pub fn block(&self) -> &BlockMessage { &self.0 }

    pub fn into_block(self) -> BlockMessage { self.0 }
}
