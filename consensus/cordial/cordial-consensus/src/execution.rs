use cordial_miners_core::BlockIdentity;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionCheckpoint {
    pub chain: [u8; 32],
    pub next_index: u64,
    pub state: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommittedExecution {
    pub chain: [u8; 32],
    pub index: u64,
    pub object: BlockIdentity,
    pub pre_state: [u8; 32],
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExecutionReceipt {
    pub index: u64,
    pub object: BlockIdentity,
    pub pre_state: [u8; 32],
    pub post_state: [u8; 32],
    pub result: Vec<u8>,
    pub deploy_ids: Vec<[u8; 32]>,
}

pub const MAX_RECEIPT_BYTES: usize = 64 * 1024;
pub const MAX_RECEIPT_DEPLOYS: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Submission {
    pub id: [u8; 32],
    pub payload: Vec<u8>,
}

#[async_trait::async_trait]
pub trait CommittedExecutor: Send + Sync {
    async fn execute_next(&self, store: &mut crate::DurableBlocklace)
    -> Result<bool, crate::Error>;
}

pub trait Application: CommittedExecutor {
    fn validate_submission(&self, payload: &[u8]) -> Result<Submission, crate::Error>;
    fn proposal_payload(
        &self,
        submissions: &[Submission],
    ) -> Result<(Vec<u8>, usize), crate::Error>;
}
