use models::rust::block_hash::{BlockHash, BlockHashSerde};
use serde::{Deserialize, Serialize};

pub const LFB_POINTER_KEY: &str = "lfb";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FinalizationCommitMarker {
    pub directly: BlockHashSerde,
    pub height: i64,
    pub indirect_sorted: Vec<BlockHashSerde>,
    pub ft_value: f32,
    pub commit_epoch: u64,
}

impl FinalizationCommitMarker {
    pub fn finalized_in_order(&self) -> Vec<BlockHash> {
        let mut blocks: Vec<BlockHash> = self
            .indirect_sorted
            .iter()
            .map(|hash| hash.0.clone())
            .collect();
        blocks.push(self.directly.0.clone());
        blocks
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LfbPointer {
    pub hash: BlockHashSerde,
    pub height: i64,
    pub commit_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommitRefusal {
    NotInDag,
    NotHigher { lfb_height: i64 },
    StateChanged(String),
    ALinViolation(String),
}

impl CommitRefusal {
    pub fn kind(&self) -> &'static str {
        match self {
            CommitRefusal::NotInDag => "not_in_dag",
            CommitRefusal::NotHigher { .. } => "not_higher",
            CommitRefusal::StateChanged(_) => "state_changed",
            CommitRefusal::ALinViolation(_) => "alin_violation",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CommitOutcome {
    Committed(FinalizationCommitMarker),
    Refused(CommitRefusal),
}

pub fn commit_epoch_key(commit_epoch: u64) -> Vec<u8> { commit_epoch.to_be_bytes().to_vec() }
