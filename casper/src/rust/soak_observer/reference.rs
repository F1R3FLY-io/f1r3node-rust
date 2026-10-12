use std::collections::{BTreeMap, BTreeSet, VecDeque};

use block_storage::rust::dag::soak_snapshot::{BlockBody, DetachedDagSnapshot};
use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use models::rust::block_metadata::BlockMetadata;
use models::rust::validator::Validator;
use prost::bytes::Bytes;
use shared::rust::dag::observation_work::{CheckedWork, WorkKind, WorkMeter};

use super::evaluation::FloorValue;
use super::FloorOutcome;

#[derive(Debug)]
pub enum ReferenceError {
    Missing(BlockHash),
    Incompatible,
    Unavailable(&'static str),
    Work(String),
}

impl From<shared::rust::store::key_value_store::KvStoreError> for ReferenceError {
    fn from(error: shared::rust::store::key_value_store::KvStoreError) -> Self {
        Self::Work(error.to_string())
    }
}

impl std::fmt::Display for ReferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(hash) => write!(f, "missing_metadata:{}", hex::encode(hash)),
            Self::Incompatible => write!(f, "incompatible_finalized_fork"),
            Self::Unavailable(reason) => write!(f, "{reason}"),
            Self::Work(reason) => write!(f, "{reason}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ReferenceOracle {
    pub decision: bool,
    pub original_bits: u32,
    pub total_stake: Option<i64>,
    pub agreeing_stake: Option<i64>,
    pub clique_weight: Option<i64>,
}

pub struct Reference<'a> {
    snapshot: &'a DetachedDagSnapshot,
    meter: CheckedWork,
    threshold: i64,
    floors: BTreeMap<BlockHash, BlockHash>,
    signatures: BTreeMap<BlockHash, BTreeSet<Bytes>>,
}

impl<'a> Reference<'a> {
    pub fn new(snapshot: &'a DetachedDagSnapshot, meter: CheckedWork, threshold: i64) -> Self {
        Self {
            snapshot,
            meter,
            threshold,
            floors: BTreeMap::new(),
            signatures: BTreeMap::new(),
        }
    }

    fn metadata(&self, hash: &BlockHash) -> Result<&'a BlockMetadata, ReferenceError> {
        self.meter.step(WorkKind::Metadata)?;
        self.snapshot
            .blocks
            .get(hash)
            .map(|b| &b.metadata)
            .ok_or_else(|| ReferenceError::Missing(hash.clone()))
    }

    fn main_contains(
        &self,
        ancestor: &BlockHash,
        descendant: &BlockHash,
    ) -> Result<bool, ReferenceError> {
        if ancestor == descendant {
            return Ok(true);
        }
        let height = self.metadata(ancestor)?.block_number;
        let mut current = descendant.clone();
        loop {
            self.meter.step(WorkKind::Traversal)?;
            let metadata = self.metadata(&current)?;
            if metadata.block_number <= height {
                return Ok(current == *ancestor);
            }
            match metadata.parents.first() {
                Some(parent) => current = parent.clone(),
                None => return Ok(false),
            }
        }
    }

    fn self_previous(&self, hash: &BlockHash) -> Result<Option<BlockHash>, ReferenceError> {
        let metadata = self.metadata(hash)?;
        for justification in &metadata.justifications {
            self.meter.step(WorkKind::Traversal)?;
            if justification.validator == metadata.sender {
                return Ok(Some(justification.latest_block_hash.clone()));
            }
        }
        Ok(None)
    }

    fn direction(
        &self,
        latest: &BlockHash,
        seen: &BlockHash,
        target: &BlockHash,
    ) -> Result<bool, ReferenceError> {
        let stopper = self.self_previous(seen)?.unwrap_or_else(|| seen.clone());
        let mut cursor = self.self_previous(latest)?;
        let target_height = self.metadata(target)?.block_number;
        while let Some(hash) = cursor {
            self.meter.step(WorkKind::Traversal)?;
            if hash == stopper {
                break;
            }
            let compatible = if self.metadata(&hash)?.block_number < target_height {
                self.main_contains(&hash, target)?
            } else {
                self.main_contains(target, &hash)?
            };
            if !compatible {
                return Ok(false);
            }
            cursor = self.self_previous(&hash)?;
        }
        Ok(true)
    }

    pub fn oracle(
        &self,
        target: &BlockHash,
        latest: &BTreeMap<Validator, BlockHash>,
        strict: bool,
    ) -> Result<ReferenceOracle, ReferenceError> {
        self.meter.step(WorkKind::Oracle)?;
        let minimum = crate::rust::safety_oracle::MIN_FAULT_TOLERANCE.to_bits();
        if !self.snapshot.blocks.contains_key(target) {
            return Ok(ReferenceOracle {
                decision: false,
                original_bits: minimum,
                total_stake: None,
                agreeing_stake: None,
                clique_weight: None,
            });
        }
        let metadata = self.metadata(target)?;
        let committee = match metadata.parents.first() {
            Some(parent) => &self.metadata(parent)?.weight_map,
            None => &metadata.weight_map,
        };
        if committee.len() > 16 {
            return Err(ReferenceError::Unavailable("reference_committee_limit"));
        }
        let mut total = 0i64;
        let mut agreeing = 0i64;
        self.meter.allocate(committee.len(), 256)?;
        let mut validators = Vec::new();
        for (validator, weight) in committee {
            self.meter.step(WorkKind::Oracle)?;
            if *weight < 0 {
                return Err(ReferenceError::Unavailable("negative_stake"));
            }
            total = total
                .checked_add(*weight)
                .ok_or(ReferenceError::Unavailable("stake_overflow"))?;
            let agrees = match latest.get(validator) {
                None => false,
                Some(hash) => match self.main_contains(target, hash) {
                    Err(ReferenceError::Missing(_)) => false,
                    result => result?,
                },
            };
            if agrees {
                agreeing = agreeing
                    .checked_add(*weight)
                    .ok_or(ReferenceError::Unavailable("stake_overflow"))?;
                validators.push((validator, *weight));
            }
        }
        if total <= 0 {
            return Ok(ReferenceOracle {
                decision: false,
                original_bits: minimum,
                total_stake: Some(total),
                agreeing_stake: None,
                clique_weight: None,
            });
        }
        if i128::from(agreeing) * 2 <= i128::from(total) {
            return Ok(ReferenceOracle {
                decision: false,
                original_bits: minimum,
                total_stake: Some(total),
                agreeing_stake: Some(agreeing),
                clique_weight: None,
            });
        }
        let n = validators.len();
        self.meter.allocate(n * n, std::mem::size_of::<bool>())?;
        self.meter.allocate(n, std::mem::size_of::<Vec<bool>>())?;
        let mut edges = vec![vec![false; n]; n];
        for a in 0..n {
            for b in a + 1..n {
                self.meter.step(WorkKind::Oracle)?;
                let (va, _) = validators[a];
                let (vb, _) = validators[b];
                let (Some(la), Some(lb)) = (latest.get(va), latest.get(vb)) else {
                    continue;
                };
                let mut a_seen_b = None;
                for j in &self.metadata(la)?.justifications {
                    self.meter.step(WorkKind::Traversal)?;
                    if j.validator == *vb {
                        a_seen_b = Some(j.latest_block_hash.clone());
                    }
                }
                let mut b_seen_a = None;
                for j in &self.metadata(lb)?.justifications {
                    self.meter.step(WorkKind::Traversal)?;
                    if j.validator == *va {
                        b_seen_a = Some(j.latest_block_hash.clone());
                    }
                }
                if let (Some(ab), Some(ba)) = (a_seen_b, b_seen_a) {
                    let forward = self.direction(lb, &ab, target)?;
                    let backward = self.direction(la, &ba, target)?;
                    edges[a][b] = forward && backward;
                    edges[b][a] = forward && backward;
                }
            }
        }
        let mut best = 0i64;
        for subset in 1usize..(1usize << n) {
            self.meter.expand(0)?;
            let mut weight = 0i64;
            let mut clique = true;
            for a in 0..n {
                self.meter.step(WorkKind::Clique)?;
                if subset & (1 << a) == 0 {
                    continue;
                }
                weight = weight
                    .checked_add(validators[a].1)
                    .ok_or(ReferenceError::Unavailable("stake_overflow"))?;
                for (b, connected) in edges[a].iter().enumerate().skip(a + 1) {
                    self.meter.step(WorkKind::Clique)?;
                    if subset & (1 << b) != 0 && !connected {
                        clique = false;
                    }
                }
            }
            if clique {
                best = best.max(weight);
            }
        }
        let lhs = i128::from(best)
            .checked_mul(2)
            .and_then(|v| v.checked_mul(1_000_000))
            .ok_or(ReferenceError::Unavailable("threshold_overflow"))?;
        let rhs = i128::from(total)
            .checked_mul(1_000_000 + i128::from(self.threshold))
            .ok_or(ReferenceError::Unavailable("threshold_overflow"))?;
        let decision = if strict { lhs > rhs } else { lhs >= rhs };
        let original_bits = if agreeing as f32 <= total as f32 / 2.0 {
            minimum
        } else {
            ((best as f32 * 2.0 - total as f32) / total as f32).to_bits()
        };
        Ok(ReferenceOracle {
            decision,
            original_bits,
            total_stake: Some(total),
            agreeing_stake: Some(agreeing),
            clique_weight: Some(best),
        })
    }

    fn require_complete_history(&self) -> Result<(), ReferenceError> {
        for block in self.snapshot.blocks.values() {
            self.meter.step(WorkKind::Traversal)?;
            for parent in &block.metadata.parents {
                self.meter.step(WorkKind::Traversal)?;
                let Some(parent) = self.snapshot.blocks.get(parent) else {
                    return Err(ReferenceError::Unavailable(
                        "restore_seed_provenance_unavailable",
                    ));
                };
                if parent.metadata.block_number >= block.metadata.block_number {
                    return Err(ReferenceError::Unavailable("invalid_parent_height"));
                }
            }
        }
        Ok(())
    }

    fn state_parent(&self, hash: &BlockHash) -> Result<Option<BlockHash>, ReferenceError> {
        let metadata = self.metadata(hash)?;
        if !metadata.merge_base.is_empty() {
            return Ok(Some(metadata.merge_base.clone()));
        }
        match metadata.parents.as_slice() {
            [] => Ok(None),
            [parent] => Ok(Some(parent.clone())),
            _ => Err(ReferenceError::Unavailable("state_parent_unavailable")),
        }
    }

    fn meet(
        &self,
        left: &BlockHash,
        right: &BlockHash,
    ) -> Result<Option<BlockHash>, ReferenceError> {
        let (mut a, mut b) = (left.clone(), right.clone());
        loop {
            self.meter.step(WorkKind::Traversal)?;
            if a == b {
                return Ok(Some(a));
            }
            let ah = self.metadata(&a)?.block_number;
            let bh = self.metadata(&b)?.block_number;
            if ah >= bh {
                match self.state_parent(&a)? {
                    Some(parent) => a = parent,
                    None => return Ok(None),
                }
            }
            if bh >= ah {
                match self.state_parent(&b)? {
                    Some(parent) => b = parent,
                    None => return Ok(None),
                }
            }
        }
    }

    fn introduced(&mut self, hash: &BlockHash) -> Result<&BTreeSet<Bytes>, ReferenceError> {
        if !self.signatures.contains_key(hash) {
            let bytes = match self.snapshot.bodies.get(hash) {
                Some(BlockBody::Held(bytes)) => bytes,
                Some(BlockBody::NotHeld) => {
                    return Err(ReferenceError::Unavailable("body_not_held"))
                }
                None => return Err(ReferenceError::Unavailable("body_not_requested")),
            };
            self.meter
                .charge(WorkKind::Allocation, 1, self.meter.body_bytes())?;
            let block =
                KeyValueBlockStore::decode_block_bounded(bytes, &self.snapshot.limits.block_decode)
                    .map_err(|_| ReferenceError::Unavailable("body_decode_failed"))?;
            let mut signatures = BTreeSet::new();
            for deploy in &block.body.deploys {
                self.meter.step(WorkKind::Signature)?;
                if !deploy.is_failed {
                    self.meter.allocate(2, deploy.deploy.sig.len() + 128)?;
                    signatures.insert(deploy.deploy.sig.clone());
                }
            }
            for signature in &block.body.applied_from_scope {
                self.meter.step(WorkKind::Signature)?;
                self.meter.allocate(2, signature.len() + 128)?;
                signatures.insert(signature.clone());
            }
            self.meter.allocate(1, 128)?;
            self.signatures.insert(hash.clone(), signatures);
        }
        Ok(self
            .signatures
            .get(hash)
            .expect("The signature set was inserted"))
    }

    fn segment(
        &mut self,
        from: &BlockHash,
        meet: &BlockHash,
    ) -> Result<BTreeSet<Bytes>, ReferenceError> {
        let mut cursor = from.clone();
        let mut result = BTreeSet::new();
        let meter = self.meter.clone();
        while cursor != *meet {
            meter.step(WorkKind::Traversal)?;
            for signature in self.introduced(&cursor)? {
                meter.step(WorkKind::Signature)?;
                meter.allocate(2, signature.len() + 128)?;
                result.insert(signature.clone());
            }
            cursor = self
                .state_parent(&cursor)?
                .ok_or(ReferenceError::Unavailable("disconnected_state_lineage"))?;
        }
        Ok(result)
    }

    pub fn contains_state(
        &mut self,
        candidate: &BlockHash,
        settled: &BlockHash,
    ) -> Result<bool, ReferenceError> {
        if candidate == settled {
            return Ok(true);
        }
        let Some(meet) = self.meet(candidate, settled)? else {
            return Ok(false);
        };
        if meet == *settled {
            return Ok(true);
        }
        let old = self.segment(settled, &meet)?;
        if old.is_empty() {
            return Ok(true);
        }
        let new = self.segment(candidate, &meet)?;
        for signature in &old {
            self.meter.step(WorkKind::Signature)?;
            if !new.contains(signature) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn ancestor(
        &self,
        ancestor: &BlockHash,
        descendant: &BlockHash,
    ) -> Result<bool, ReferenceError> {
        if ancestor == descendant {
            return Ok(true);
        }
        let height = self.metadata(ancestor)?.block_number;
        let mut queue = VecDeque::new();
        let mut visited = BTreeSet::new();
        self.meter.allocate(2, 128)?;
        queue.push_back(descendant.clone());
        while let Some(hash) = queue.pop_front() {
            self.meter.step(WorkKind::Traversal)?;
            if hash == *ancestor {
                return Ok(true);
            }
            if !visited.insert(hash.clone()) {
                continue;
            }
            let metadata = self.metadata(&hash)?;
            if metadata.block_number <= height {
                continue;
            }
            for parent in &metadata.parents {
                self.meter.step(WorkKind::Traversal)?;
                self.meter.allocate(4, 128)?;
                queue.push_back(parent.clone());
            }
        }
        Ok(false)
    }

    fn frontier(
        &self,
        parent: &BlockHash,
        latest: &BTreeMap<Validator, BlockHash>,
    ) -> Result<BlockHash, ReferenceError> {
        let mut current = parent.clone();
        loop {
            self.meter.step(WorkKind::Traversal)?;
            if self.oracle(&current, latest, false)?.decision {
                return Ok(current);
            }
            match self.metadata(&current)?.parents.first() {
                Some(parent) => current = parent.clone(),
                None => return Ok(current),
            }
        }
    }

    fn derive(
        &mut self,
        parents: &[BlockHash],
        latest: &BTreeMap<Validator, BlockHash>,
    ) -> Result<BlockHash, ReferenceError> {
        if parents.is_empty() {
            return Err(ReferenceError::Unavailable("empty_parent_set"));
        }
        self.meter.allocate(parents.len(), 512)?;
        let mut inherited = Vec::new();
        let mut candidates = Vec::new();
        for parent in parents {
            self.meter.step(WorkKind::Traversal)?;
            let floor = self.block_floor_inner(parent)?;
            inherited.push(floor.clone());
            candidates.push(floor);
            candidates.push(self.frontier(parent, latest)?);
        }
        self.meter.allocate(candidates.len(), 128)?;
        let mut ordered = Vec::new();
        for hash in candidates {
            self.meter.step(WorkKind::Traversal)?;
            ordered.push((self.metadata(&hash)?.block_number, hash));
        }
        for position in 1..ordered.len() {
            let mut at = position;
            while at > 0 {
                self.meter.step(WorkKind::Traversal)?;
                if ordered[at - 1] >= ordered[at] {
                    break;
                }
                self.meter.step(WorkKind::Traversal)?;
                ordered.swap(at - 1, at);
                at -= 1;
            }
        }
        'candidate: for (_, candidate) in ordered {
            self.meter.step(WorkKind::Traversal)?;
            for other in &inherited {
                self.meter.step(WorkKind::Traversal)?;
                if self.contains_state(&candidate, other)? {
                    continue;
                }
                let mut sound = false;
                if !self.ancestor(other, &candidate)? {
                    for parent in parents {
                        self.meter.step(WorkKind::Traversal)?;
                        if self.ancestor(other, parent)? && self.ancestor(&candidate, parent)? {
                            if let Some(meet) = self.meet(&candidate, other)? {
                                sound = self.segment(&candidate, &meet)?.is_empty();
                            }
                            break;
                        }
                    }
                }
                if !sound {
                    continue 'candidate;
                }
            }
            return Ok(candidate);
        }
        Err(ReferenceError::Incompatible)
    }

    fn block_floor_inner(&mut self, hash: &BlockHash) -> Result<BlockHash, ReferenceError> {
        self.meter.allocate(1, 128)?;
        let mut stack = vec![hash.clone()];
        while let Some(current) = stack.last().cloned() {
            self.meter.step(WorkKind::Traversal)?;
            if self.floors.contains_key(&current) {
                stack.pop();
                continue;
            }
            let metadata = self.metadata(&current)?;
            if metadata.parents.is_empty() {
                self.meter.allocate(1, 256)?;
                self.floors.insert(current.clone(), current);
                stack.pop();
                continue;
            }
            let mut missing = false;
            for parent in &metadata.parents {
                self.meter.step(WorkKind::Traversal)?;
                if !self.floors.contains_key(parent) {
                    self.meter.allocate(2, 128)?;
                    stack.push(parent.clone());
                    missing = true;
                }
            }
            if missing {
                continue;
            }
            self.meter.allocate(metadata.justifications.len(), 256)?;
            let mut latest = BTreeMap::new();
            for j in &metadata.justifications {
                self.meter.step(WorkKind::Traversal)?;
                latest.insert(j.validator.clone(), j.latest_block_hash.clone());
            }
            let floor = self.derive(&metadata.parents, &latest)?;
            self.meter.allocate(1, 256)?;
            self.floors.insert(current, floor);
            stack.pop();
        }
        self.floors
            .get(hash)
            .cloned()
            .ok_or(ReferenceError::Unavailable("floor_unavailable"))
    }

    pub fn block_floor(&mut self, hash: &BlockHash) -> Result<FloorValue, ReferenceError> {
        self.require_complete_history()?;
        let hash = self.block_floor_inner(hash)?;
        Ok(FloorValue::with_hash(
            FloorOutcome::Advance,
            &hash,
            Some(self.metadata(&hash)?.block_number),
        ))
    }

    pub fn view_floor(
        &mut self,
        current: &BlockHash,
        height: i64,
    ) -> Result<FloorValue, ReferenceError> {
        self.require_complete_history()?;
        let mut tips = BTreeSet::new();
        let mut latest = BTreeMap::new();
        for (validator, hash) in &self.snapshot.latest_messages {
            self.meter.step(WorkKind::Traversal)?;
            let Some(block) = self.snapshot.blocks.get(hash) else {
                continue;
            };
            if block.metadata.sender != *validator {
                continue;
            }
            self.meter.allocate(2, 256)?;
            if block.metadata.block_number > height {
                tips.insert(hash.clone());
            }
            latest.insert(validator.clone(), hash.clone());
        }
        if tips.is_empty() {
            return Ok(FloorValue::empty(FloorOutcome::NoAdvance));
        }
        self.meter.allocate(tips.len(), 128)?;
        let tips: Vec<_> = tips.into_iter().collect();
        let derived = match self.derive(&tips, &latest) {
            Ok(hash) => hash,
            Err(ReferenceError::Incompatible) if self.threshold <= 0 => {
                return Ok(FloorValue::empty(FloorOutcome::IncompatibilityHold));
            }
            Err(ReferenceError::Missing(missing)) => {
                let mut decidable = Vec::new();
                for tip in &tips {
                    self.meter.step(WorkKind::Traversal)?;
                    let result = self
                        .block_floor_inner(tip)
                        .and_then(|_| self.frontier(tip, &latest));
                    match result {
                        Ok(_) => {
                            self.meter.allocate(2, 128)?;
                            decidable.push(tip.clone());
                        }
                        Err(ReferenceError::Missing(_)) => {}
                        Err(error) => return Err(error),
                    }
                }
                if decidable.is_empty() {
                    return Ok(FloorValue::empty(FloorOutcome::NoAdvance));
                }
                if decidable.len() == tips.len() {
                    return Ok(FloorValue::with_hash(
                        FloorOutcome::AbsenceHold,
                        &missing,
                        None,
                    ));
                }
                match self.derive(&decidable, &latest) {
                    Ok(hash) => hash,
                    Err(ReferenceError::Missing(hash)) => {
                        return Ok(FloorValue::with_hash(
                            FloorOutcome::AbsenceHold,
                            &hash,
                            None,
                        ))
                    }
                    Err(ReferenceError::Incompatible) if self.threshold <= 0 => {
                        return Ok(FloorValue::empty(FloorOutcome::IncompatibilityHold))
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        };
        let derived_height = self.metadata(&derived)?.block_number;
        if derived == *current || derived_height <= height {
            return Ok(FloorValue::empty(FloorOutcome::NoAdvance));
        }
        let outcome = match self.contains_state(&derived, current) {
            Ok(true) => FloorOutcome::Advance,
            Ok(false) => FloorOutcome::ContainmentHold,
            Err(ReferenceError::Missing(hash)) => {
                return Ok(FloorValue::with_hash(
                    FloorOutcome::AbsenceHold,
                    &hash,
                    None,
                ))
            }
            Err(error) => return Err(error),
        };
        Ok(FloorValue::with_hash(
            outcome,
            &derived,
            Some(derived_height),
        ))
    }
}
