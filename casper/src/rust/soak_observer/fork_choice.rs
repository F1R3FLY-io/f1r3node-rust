use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use block_storage::rust::dag::soak_snapshot::DetachedDagSnapshot;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::validator::Validator;
use serde::Serialize;
use shared::rust::dag::observation_work::{CheckedWork, WorkKind, WorkMeter};

use super::evaluation::{digest, Value};
use super::{AuthorityInputs, ForkChoiceInputs};

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

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReferenceControl {
    None,
    RankTipsByOwnScore,
    CreditAllParents,
    ReverseTieOrder,
    KeepInvalidMessages,
    KeepForeignMessages,
    BoundAt(BlockHash),
}

pub fn validate_result(result: &ForkChoiceResult) -> Result<(), String> {
    if result.tips.is_empty() || result.tips.len() > MAX_REPORTED_TIPS {
        return Err("schema:tip_count".to_string());
    }
    if result.tips[0] != result.head {
        return Err("schema:head_not_first_tip".to_string());
    }
    if result.tip_scores.len() != result.tips.len() {
        return Err("schema:score_count".to_string());
    }
    if result.head == result.lower_bound.hash && result.tips.len() != 1 {
        return Err("schema:head_is_lower_bound".to_string());
    }
    Ok(())
}

pub fn compare(
    bounded: &Value<ForkChoiceResult>,
    reference: &Value<ForkChoiceResult>,
) -> Value<ForkChoiceComparison> {
    match (bounded, reference) {
        (
            Value::Available {
                input_digest: left,
                value: bounded,
            },
            Value::Available {
                input_digest: right,
                value: reference,
            },
        ) => {
            if left != right {
                return Value::Unavailable {
                    input_digest: Some(left.clone()),
                    reason: "input_digest_mismatch".to_string(),
                };
            }
            Value::Available {
                input_digest: left.clone(),
                value: ForkChoiceComparison {
                    algorithm: COMPARISON_ALGORITHM,
                    head_matches: bounded.head == reference.head,
                    tips_match: bounded.tips == reference.tips,
                    bounds_differ: bounded.lower_bound.hash != reference.lower_bound.hash,
                },
            }
        }
        (Value::NotRequested, _) | (_, Value::NotRequested) => Value::NotRequested,
        (Value::Available { input_digest, .. }, _) => Value::Unavailable {
            input_digest: Some(input_digest.clone()),
            reason: "result_unavailable".to_string(),
        },
        (Value::Unavailable { input_digest, .. } | Value::Failed { input_digest, .. }, _) => {
            Value::Unavailable {
                input_digest: input_digest.clone(),
                reason: "result_unavailable".to_string(),
            }
        }
    }
}

pub struct ReferenceForkChoice<'a> {
    snapshot: &'a DetachedDagSnapshot,
    meter: &'a CheckedWork,
    inputs: &'a ForkChoiceInputs,
    authority: &'a AuthorityInputs,
    visited: HashSet<BlockHash>,
    examined_edges: u64,
    control: ReferenceControl,
}

struct Ranked {
    ancestor: BlockHash,
    tips: Vec<BlockHash>,
    scores: HashMap<BlockHash, i64>,
}

impl<'a> ReferenceForkChoice<'a> {
    pub fn new(
        snapshot: &'a DetachedDagSnapshot,
        meter: &'a CheckedWork,
        inputs: &'a ForkChoiceInputs,
        authority: &'a AuthorityInputs,
    ) -> Self {
        Self {
            snapshot,
            meter,
            inputs,
            authority,
            visited: HashSet::new(),
            examined_edges: 0,
            control: ReferenceControl::None,
        }
    }

    #[doc(hidden)]
    pub fn with_control(mut self, control: ReferenceControl) -> Self {
        self.control = control;
        self
    }

    fn order(
        &self,
        scores: &HashMap<BlockHash, i64>,
        a: &BlockHash,
        b: &BlockHash,
    ) -> std::cmp::Ordering {
        let score_a = scores.get(a).copied().unwrap_or(0);
        let score_b = scores.get(b).copied().unwrap_or(0);
        let tie = if self.control == ReferenceControl::ReverseTieOrder {
            b.cmp(a)
        } else {
            a.cmp(b)
        };
        score_b.cmp(&score_a).then(tie)
    }

    fn metadata(&mut self, hash: &BlockHash) -> Result<&'a BlockMetadata, String> {
        self.meter
            .step(WorkKind::Metadata)
            .map_err(|e| e.to_string())?;
        if self.visited.insert(hash.clone()) {
            self.meter
                .allocate(1, std::mem::size_of::<BlockHash>())
                .map_err(|e| e.to_string())?;
        }
        self.snapshot
            .blocks
            .get(hash)
            .map(|block| &block.metadata)
            .ok_or_else(|| "history_incomplete".to_string())
    }

    fn edge(&mut self) -> Result<(), String> {
        self.meter
            .step(WorkKind::Traversal)
            .map_err(|e| e.to_string())?;
        self.examined_edges += 1;
        Ok(())
    }

    fn used_messages(
        &mut self,
    ) -> Result<(LatestMessageCounts, BTreeMap<Validator, BlockHash>), String> {
        let mut counts = LatestMessageCounts {
            captured: self.snapshot.latest_messages.len(),
            ..LatestMessageCounts::default()
        };
        let mut used = BTreeMap::new();
        for (validator, hash) in &self.snapshot.latest_messages {
            self.meter
                .step(WorkKind::Metadata)
                .map_err(|e| e.to_string())?;
            let Some(block) = self.snapshot.blocks.get(hash) else {
                counts.not_held += 1;
                continue;
            };
            self.visited.insert(hash.clone());
            if block.metadata.invalid || self.snapshot.invalid_blocks.contains_key(hash) {
                counts.invalid += 1;
                if self.control != ReferenceControl::KeepInvalidMessages {
                    continue;
                }
            }
            if block.metadata.sender != *validator {
                counts.not_own_testimony += 1;
                if self.control != ReferenceControl::KeepForeignMessages {
                    continue;
                }
            }
            used.insert(validator.clone(), hash.clone());
        }
        counts.used = used.len();
        Ok((counts, used))
    }

    fn depth_filtered(
        &mut self,
        used: &BTreeMap<Validator, BlockHash>,
    ) -> Result<BTreeMap<Validator, BlockHash>, String> {
        let top = self
            .snapshot
            .blocks
            .values()
            .map(|block| block.metadata.block_number)
            .max()
            .unwrap_or(0);
        let mut kept = BTreeMap::new();
        for (validator, hash) in used {
            let number = self.metadata(hash)?.block_number;
            if number > top - self.inputs.latest_message_depth {
                kept.insert(validator.clone(), hash.clone());
            }
        }
        Ok(kept)
    }

    fn common_ancestor(
        &mut self,
        messages: &BTreeMap<Validator, BlockHash>,
        bound: &BlockMetadata,
    ) -> Result<BlockHash, String> {
        let mut frontier: BTreeSet<(i64, BlockHash)> = BTreeSet::new();
        for hash in messages.values() {
            let number = self.metadata(hash)?.block_number;
            frontier.insert((number, hash.clone()));
        }
        if frontier.is_empty() {
            return Ok(bound.block_hash.clone());
        }
        self.meter
            .allocate(frontier.len(), std::mem::size_of::<(i64, BlockHash)>())
            .map_err(|e| e.to_string())?;
        loop {
            if frontier.len() == 1 {
                let (_, hash) = frontier.into_iter().next().ok_or("history_incomplete")?;
                return Ok(hash);
            }
            let highest = frontier
                .iter()
                .next_back()
                .cloned()
                .ok_or("history_incomplete")?;
            if highest.0 <= bound.block_number {
                return Ok(bound.block_hash.clone());
            }
            frontier.remove(&highest);
            let parents = self.metadata(&highest.1)?.parents.clone();
            for parent in parents {
                self.edge()?;
                let number = self.metadata(&parent)?.block_number;
                frontier.insert((number, parent));
            }
        }
    }

    fn weight_at(&mut self, block: &BlockMetadata, validator: &Validator) -> Result<i64, String> {
        let source = match block.parents.first() {
            Some(main_parent) => {
                self.edge()?;
                self.metadata(main_parent)?
            }
            None => block,
        };
        Ok(source.weight_map.get(validator).copied().unwrap_or(0))
    }

    fn scores(
        &mut self,
        messages: &BTreeMap<Validator, BlockHash>,
        ancestor_number: i64,
    ) -> Result<HashMap<BlockHash, i64>, String> {
        let mut scores: HashMap<BlockHash, i64> = HashMap::new();
        for (validator, hash) in messages {
            let mut current = hash.clone();
            loop {
                let block = self.metadata(&current)?;
                if block.block_number < ancestor_number {
                    break;
                }
                let weight = self.weight_at(block, validator)?;
                if !scores.contains_key(&current) {
                    self.meter
                        .allocate(1, std::mem::size_of::<(BlockHash, i64)>())
                        .map_err(|e| e.to_string())?;
                }
                let entry = scores.entry(current.clone()).or_insert(0);
                *entry = entry.checked_add(weight).ok_or("score_overflow")?;
                if self.control == ReferenceControl::CreditAllParents {
                    for secondary in block.parents.iter().skip(1) {
                        self.edge()?;
                        let entry = scores.entry(secondary.clone()).or_insert(0);
                        *entry = entry.checked_add(weight).ok_or("score_overflow")?;
                    }
                }
                match block.parents.first() {
                    Some(main_parent) => {
                        self.edge()?;
                        current = main_parent.clone();
                    }
                    None => break,
                }
            }
        }
        Ok(scores)
    }

    fn scored_main_children(
        &mut self,
        block: &BlockHash,
        scores: &HashMap<BlockHash, i64>,
    ) -> Result<Vec<BlockHash>, String> {
        let mut children = Vec::new();
        if let Some(candidates) = self.snapshot.child_map.get(block) {
            for child in candidates {
                self.edge()?;
                if scores.contains_key(child)
                    && self.metadata(child)?.parents.first() == Some(block)
                {
                    children.push(child.clone());
                }
            }
        }
        Ok(children)
    }

    fn rank(
        &mut self,
        messages: &BTreeMap<Validator, BlockHash>,
        bound: &BlockMetadata,
    ) -> Result<Ranked, String> {
        let ancestor = self.common_ancestor(messages, bound)?;
        let ancestor_number = self.metadata(&ancestor)?.block_number;
        let scores = self.scores(messages, ancestor_number)?;
        let mut head = ancestor.clone();
        if self.control == ReferenceControl::RankTipsByOwnScore {
            let mut candidates: Vec<BlockHash> = messages.values().cloned().collect();
            candidates.sort_by(|a, b| self.order(&scores, a, b));
            if let Some(first) = candidates.first() {
                head = first.clone();
            }
        } else {
            loop {
                let mut children = self.scored_main_children(&head, &scores)?;
                if children.is_empty() {
                    break;
                }
                children.sort_by(|a, b| self.order(&scores, a, b));
                head = children.swap_remove(0);
            }
        }
        let mut frontier: BTreeSet<BlockHash> = BTreeSet::new();
        for hash in messages.values() {
            if *hash != head && self.scored_main_children(hash, &scores)?.is_empty() {
                frontier.insert(hash.clone());
            }
        }
        let mut ranked: Vec<BlockHash> = frontier.into_iter().collect();
        self.meter
            .charge(WorkKind::Traversal, ranked.len() as u64, 0)
            .map_err(|e| e.to_string())?;
        ranked.sort_by(|a, b| self.order(&scores, a, b));
        let head_number = self.metadata(&head)?.block_number;
        let depth = i64::from(self.authority.max_parent_depth);
        let mut tips = vec![head.clone()];
        for hash in ranked {
            let number = self.metadata(&hash)?.block_number;
            if head_number - number <= depth {
                tips.push(hash);
            }
        }
        let limit = self.inputs.max_number_of_parents;
        if limit >= 0 && limit != i32::MAX {
            tips.truncate(limit as usize);
        }
        Ok(Ranked {
            ancestor,
            tips,
            scores,
        })
    }

    pub fn evaluate(mut self) -> (LatestMessageCounts, Result<ForkChoiceResult, String>) {
        let mut counts = LatestMessageCounts::default();
        let result = (|| {
            let (used_counts, used) = self.used_messages()?;
            counts = used_counts;
            let approved_hash: BlockHash = hex::decode(&self.authority.approved_block_hash)
                .map_err(|_| "approved_block_not_captured".to_string())?
                .into();
            let approved = self
                .snapshot
                .blocks
                .get(&approved_hash)
                .map(|block| block.metadata.clone())
                .ok_or("approved_block_not_captured")?;
            if approved.block_number != self.inputs.approved_block_number {
                return Err("approved_block_mismatch".to_string());
            }
            let messages = self.depth_filtered(&used)?;
            let bound = match &self.control {
                ReferenceControl::BoundAt(hash) => self
                    .snapshot
                    .blocks
                    .get(hash)
                    .map(|block| block.metadata.clone())
                    .ok_or("history_incomplete")?,
                _ => approved.clone(),
            };
            let ranked = self.rank(&messages, &bound)?;
            let head = ranked.tips.first().ok_or("no_head_selected")?.clone();
            let mut sorted_scores: BTreeMap<String, i64> = BTreeMap::new();
            for (hash, score) in &ranked.scores {
                self.meter
                    .step(WorkKind::Traversal)
                    .map_err(|e| e.to_string())?;
                sorted_scores.insert(hex::encode(hash), *score);
            }
            let score_digest = digest(&sorted_scores, self.meter)?;
            Ok(ForkChoiceResult {
                mode: EvaluationMode::Reference,
                lower_bound: LowerBound {
                    hash: hex::encode(&bound.block_hash),
                    block_number: bound.block_number,
                    rule: LowerBoundRule::ApprovedBlock,
                },
                common_ancestor: hex::encode(&ranked.ancestor),
                head: hex::encode(&head),
                tips: ranked
                    .tips
                    .iter()
                    .take(MAX_REPORTED_TIPS)
                    .map(hex::encode)
                    .collect(),
                tip_scores: ranked
                    .tips
                    .iter()
                    .take(MAX_REPORTED_TIPS)
                    .map(|tip| ranked.scores.get(tip).copied().unwrap_or(0))
                    .collect(),
                score_count: ranked.scores.len(),
                score_digest,
                visited_blocks: self.visited.len() as u64,
                examined_edges: self.examined_edges,
            })
        })();
        (counts, result)
    }
}
