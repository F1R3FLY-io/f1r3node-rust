use serde::Serialize;

use super::evaluation::Value;
use super::ForkChoiceInputs;

pub const INPUT_DIGEST_DOMAIN: &str = "batch-d-fork-choice-v1";
pub const LATEST_MESSAGE_SCOPE: &str = "captured_latest_messages";
pub const COMPARISON_ALGORITHM: &str = "immutable-ghost-reference-v1";
pub const MAX_REPORTED_TIPS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationMode {
    Bounded,
    Reference,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LowerBoundRule {
    FinalizedFloor,
    ApprovedBlock,
}

impl LowerBoundRule {
    pub const fn name(self) -> &'static str {
        match self {
            Self::FinalizedFloor => "finalized_floor",
            Self::ApprovedBlock => "approved_block",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LowerBound {
    pub hash: String,
    pub block_number: i64,
    pub rule: LowerBoundRule,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct LatestMessageCounts {
    pub captured: usize,
    pub invalid: usize,
    pub not_held: usize,
    pub not_own_testimony: usize,
    pub used: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ForkChoiceResult {
    pub mode: EvaluationMode,
    pub lower_bound: LowerBound,
    pub common_ancestor: String,
    pub head: String,
    pub tips: Vec<String>,
    pub tip_scores: Vec<i64>,
    pub score_count: usize,
    pub score_digest: String,
    pub visited_blocks: u64,
    pub examined_edges: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ForkChoiceComparison {
    pub algorithm: &'static str,
    pub head_matches: bool,
    pub tips_match: bool,
    pub bounds_differ: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ForkChoiceObservation {
    pub input_digest: String,
    pub inputs: ForkChoiceInputs,
    pub latest_messages: LatestMessageCounts,
    pub bounded: Value<ForkChoiceResult>,
    pub reference: Value<ForkChoiceResult>,
    pub comparison: Value<ForkChoiceComparison>,
}
