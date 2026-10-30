use std::collections::{HashMap, HashSet, VecDeque};

use models::rust::block_hash::BlockHash;
use prost::bytes::Bytes;

use super::body::{Attestation, StateHash};
use super::evidence::{Evidence, InvalidRecordReason, Offence};
use super::record::ExecutionRecord;
use super::store::{MemoryRecordStore, RecordStore, Retention};

#[derive(Clone, Debug)]
pub struct CoreParams {
    pub max_speculation: usize,
    pub fallback_distance: usize,
}

impl Default for CoreParams {
    fn default() -> Self {
        CoreParams {
            max_speculation: 32,
            fallback_distance: 8,
        }
    }
}

#[derive(Clone, Debug)]
struct Candidate<R> {
    citer: BlockHash,
    citer_index: usize,
    record: R,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action<R> {
    Idle,
    Replay {
        target: BlockHash,
        citer: BlockHash,
        record: R,
    },
    Execute {
        target: BlockHash,
        pre_root: StateHash,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InclusionCheck {
    pub block: BlockHash,
    pub sender: Bytes,
    pub attested: BlockHash,
    pub root: StateHash,
}

#[derive(Clone, Debug)]
struct PendingAttestation {
    citer: BlockHash,
    sender: Bytes,
    root: StateHash,
    carries_envelopes: bool,
}

pub struct EmittedBlock<R> {
    pub hash: BlockHash,
    pub sender: Bytes,
    pub envelope_ids: Vec<Bytes>,
    pub records: Vec<R>,
}

pub struct ExecutorCore<R: ExecutionRecord> {
    params: CoreParams,
    base: usize,
    window: VecDeque<BlockHash>,
    index: HashMap<BlockHash, usize>,
    senders: HashMap<BlockHash, Bytes>,
    envelopes: HashMap<BlockHash, Vec<Bytes>>,
    store: Box<dyn RecordStore<R>>,
    spec: VecDeque<R>,
    citations: HashMap<BlockHash, Vec<Candidate<R>>>,
    dismissed: HashMap<BlockHash, HashSet<BlockHash>>,
    cited: HashMap<BlockHash, HashSet<StateHash>>,
    pending_attestations: HashMap<BlockHash, Vec<PendingAttestation>>,
    pending_by_citer: HashMap<BlockHash, BlockHash>,
    inclusion_due: VecDeque<InclusionCheck>,
    inclusion_seen: HashSet<BlockHash>,
    evidence: Vec<Evidence>,
    store_error: Option<String>,
}

impl<R: ExecutionRecord> ExecutorCore<R> {
    pub fn new(genesis: BlockHash, genesis_root: StateHash, params: CoreParams) -> Self {
        let store = MemoryRecordStore::new(genesis, genesis_root, Retention::default());
        Self::with_store(params, Box::new(store))
    }

    pub fn with_store(params: CoreParams, store: Box<dyn RecordStore<R>>) -> Self {
        ExecutorCore {
            params,
            base: store.canonical_len(),
            window: VecDeque::new(),
            index: HashMap::new(),
            senders: HashMap::new(),
            envelopes: HashMap::new(),
            store,
            spec: VecDeque::new(),
            citations: HashMap::new(),
            dismissed: HashMap::new(),
            cited: HashMap::new(),
            pending_attestations: HashMap::new(),
            pending_by_citer: HashMap::new(),
            inclusion_due: VecDeque::new(),
            inclusion_seen: HashSet::new(),
            evidence: Vec::new(),
            store_error: None,
        }
    }

    pub fn reset(&mut self) -> Result<(), String> {
        self.store.reset().map_err(|error| error.to_string())?;
        self.base = self.store.canonical_len();
        self.window.clear();
        self.index.clear();
        self.senders.clear();
        self.envelopes.clear();
        self.spec.clear();
        self.citations.clear();
        self.dismissed.clear();
        self.cited.clear();
        self.pending_attestations.clear();
        self.pending_by_citer.clear();
        self.inclusion_due.clear();
        self.inclusion_seen.clear();
        self.store_error = None;
        Ok(())
    }

    fn at(&self, position: usize) -> Option<&BlockHash> {
        position
            .checked_sub(self.base)
            .and_then(|offset| self.window.get(offset))
    }

    fn order_len(&self) -> usize { self.base + self.window.len() }

    pub fn is_emitted(&self, block: &BlockHash) -> bool {
        self.index.contains_key(block) || self.store.executed(block).is_some()
    }

    pub fn resident(&self) -> usize {
        self.window.len()
            + self.index.len()
            + self.senders.len()
            + self.envelopes.len()
            + self.spec.len()
            + self.citations.len()
            + self.dismissed.len()
            + self.cited.len()
            + self.pending_attestations.len()
            + self.pending_by_citer.len()
            + self.inclusion_due.len()
            + self.inclusion_seen.len()
            + self.store.resident()
    }

    fn prune(&mut self) {
        while self.base < self.canon_pos() {
            let Some(block) = self.window.pop_front() else {
                self.base = self.canon_pos();
                break;
            };
            self.base += 1;
            self.index.remove(&block);
            self.senders.remove(&block);
            self.envelopes.remove(&block);
            self.citations.remove(&block);
            self.dismissed.remove(&block);
            self.cited.remove(&block);
            self.inclusion_seen.remove(&block);
            if let Some(attested) = self.pending_by_citer.remove(&block) {
                if !self.is_emitted(&attested) {
                    if let Some(pending) = self.pending_attestations.get_mut(&attested) {
                        pending.retain(|p| p.citer != block);
                        if pending.is_empty() {
                            self.pending_attestations.remove(&attested);
                        }
                    }
                    tracing::warn!(
                        target: "f1r3fly.v6",
                        block = %hex::encode(&block),
                        attested = %hex::encode(&attested),
                        "a canonical block attests a block that was never emitted; its attestation is dropped"
                    );
                }
            }
        }
    }

    pub fn take_store_error(&mut self) -> Option<String> { self.store_error.take() }

    fn sender_of(&self, block: &BlockHash) -> Bytes {
        self.senders.get(block).cloned().unwrap_or_default()
    }

    fn canon_pos(&self) -> usize { self.store.canonical_len() }

    fn canon_root(&self) -> StateHash { self.store.canonical_root() }

    pub fn emit(&mut self, block: EmittedBlock<R>) {
        if self.is_emitted(&block.hash) {
            return;
        }
        let citer_index = self.order_len();
        self.index.insert(block.hash.clone(), citer_index);
        self.window.push_back(block.hash.clone());
        self.senders.insert(block.hash.clone(), block.sender);
        self.envelopes
            .insert(block.hash.clone(), block.envelope_ids);
        for record in block.records {
            if !self.index.contains_key(record.target()) {
                continue;
            }
            self.citations
                .entry(record.target().clone())
                .or_default()
                .push(Candidate {
                    citer: block.hash.clone(),
                    citer_index,
                    record,
                });
        }
    }

    pub fn attestation(&self) -> Attestation {
        Attestation {
            block_hash: self
                .store
                .canonical_at(self.canon_pos() - 1)
                .expect("the store holds every canonical position"),
            root: self.canon_root(),
        }
    }

    pub fn executed(&self, block: &BlockHash) -> Option<StateHash> { self.store.executed(block) }

    pub fn record(&self, block: &BlockHash) -> Option<R> { self.store.record(block) }

    pub fn canonical_len(&self) -> usize { self.canon_pos() }

    pub fn emitted_len(&self) -> usize { self.order_len() }

    pub fn speculation_len(&self) -> usize { self.spec.len() }

    pub fn evidence(&self) -> &[Evidence] { &self.evidence }

    pub fn take_evidence(&mut self) -> Vec<Evidence> { std::mem::take(&mut self.evidence) }

    pub fn next_inclusion_check(&mut self) -> Option<InclusionCheck> {
        self.inclusion_due.pop_front()
    }

    fn inclusion_due_for(&mut self, block: &BlockHash, sender: &Bytes, attestation: &Attestation) {
        if self.inclusion_seen.insert(block.clone()) {
            self.inclusion_due.push_back(InclusionCheck {
                block: block.clone(),
                sender: sender.clone(),
                attested: attestation.block_hash.clone(),
                root: attestation.root.clone(),
            });
        }
    }

    pub fn next_action(&mut self) -> Action<R> {
        loop {
            if self.store_error.is_some() {
                return Action::Idle;
            }
            let Some(target) = self.at(self.canon_pos()).cloned() else {
                break;
            };
            let target_index = self.canon_pos();
            let mut candidates: Vec<Candidate<R>> = self
                .citations
                .get(&target)
                .map(|c| {
                    c.iter()
                        .filter(|c| c.citer_index > target_index)
                        .filter(|c| {
                            !self
                                .dismissed
                                .get(&target)
                                .is_some_and(|citers| citers.contains(&c.citer))
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            candidates.sort_by_key(|c| c.citer_index);
            let Some(candidate) = candidates.into_iter().next() else {
                break;
            };
            if *candidate.record.pre_root() != self.canon_root() {
                self.dismiss(candidate.citer, target);
                continue;
            }
            let expected = self.envelopes.get(&target).cloned().unwrap_or_default();
            if candidate.record.envelope_ids() != expected.as_slice() {
                self.evidence.push(Evidence {
                    offender: self.sender_of(&candidate.citer),
                    citer: candidate.citer.clone(),
                    target: target.clone(),
                    offence: Offence::InvalidRecord {
                        reason: InvalidRecordReason::Envelopes,
                        record: candidate.record.encode(),
                    },
                });
                self.dismiss(candidate.citer, target);
                continue;
            }
            let matches_own = self
                .spec
                .front()
                .map(|own| own.target() == &target && own.digest() == candidate.record.digest())
                .unwrap_or(false);
            if matches_own {
                self.fix(target, candidate.citer, candidate.record);
                continue;
            }
            return Action::Replay {
                target,
                citer: candidate.citer,
                record: candidate.record,
            };
        }
        let next = self.canon_pos() + self.spec.len();
        if self.spec.len() < self.params.max_speculation {
            if let Some(target) = self.at(next).cloned() {
                let pre_root = self
                    .spec
                    .back()
                    .map(|r| r.post_root().clone())
                    .unwrap_or_else(|| self.canon_root());
                return Action::Execute { target, pre_root };
            }
        }
        Action::Idle
    }

    fn dismiss(&mut self, citer: BlockHash, target: BlockHash) {
        self.dismissed.entry(target).or_default().insert(citer);
    }

    pub fn apply_replay(
        &mut self,
        target: &BlockHash,
        citer: &BlockHash,
        record: &R,
        result: Result<StateHash, String>,
    ) {
        if self.at(self.canon_pos()) != Some(target) || *record.pre_root() != self.canon_root() {
            return;
        }
        match result {
            Ok(root) if root == *record.post_root() => {
                self.fix(target.clone(), citer.clone(), record.clone())
            }
            _ => {
                self.evidence.push(Evidence {
                    offender: self.sender_of(citer),
                    citer: citer.clone(),
                    target: target.clone(),
                    offence: Offence::InvalidRecord {
                        reason: InvalidRecordReason::Replay,
                        record: record.encode(),
                    },
                });
                self.dismiss(citer.clone(), target.clone());
            }
        }
    }

    pub fn apply_execute(&mut self, record: R) {
        let next = self.canon_pos() + self.spec.len();
        let expected_pre = self
            .spec
            .back()
            .map(|r| r.post_root().clone())
            .unwrap_or_else(|| self.canon_root());
        if self.at(next) == Some(record.target()) && *record.pre_root() == expected_pre {
            self.spec.push_back(record);
        }
    }

    pub fn records_to_cite(&self, self_key: &Bytes, max: usize) -> Vec<R> {
        let newest = self.order_len().saturating_sub(1);
        self.spec
            .iter()
            .filter(|r| {
                !self
                    .cited
                    .get(r.target())
                    .is_some_and(|roots| roots.contains(r.pre_root()))
            })
            .filter(|r| {
                let own = self.senders.get(r.target()) == Some(self_key);
                let overdue = self
                    .index
                    .get(r.target())
                    .map(|i| newest.saturating_sub(*i) >= self.params.fallback_distance)
                    .unwrap_or(false);
                own || overdue
            })
            .take(max)
            .cloned()
            .collect()
    }

    pub fn mark_cited(&mut self, records: &[R]) {
        for r in records {
            self.cited
                .entry(r.target().clone())
                .or_default()
                .insert(r.pre_root().clone());
        }
    }

    pub fn observe_attestation(
        &mut self,
        citer: &BlockHash,
        sender: &Bytes,
        attestation: &Attestation,
        carries_envelopes: bool,
    ) {
        match self.store.executed(&attestation.block_hash) {
            Some(executed) => {
                if executed == attestation.root {
                    if carries_envelopes {
                        self.inclusion_due_for(citer, sender, attestation);
                    }
                } else {
                    let canonical_record = self
                        .store
                        .record(&attestation.block_hash)
                        .map(|record| record.encode());
                    self.evidence.push(Evidence {
                        offender: sender.clone(),
                        citer: citer.clone(),
                        target: attestation.block_hash.clone(),
                        offence: Offence::Misattestation {
                            attested: attestation.root.clone(),
                            executed,
                            canonical_record,
                        },
                    });
                }
            }
            None => {
                self.pending_by_citer
                    .insert(citer.clone(), attestation.block_hash.clone());
                self.pending_attestations
                    .entry(attestation.block_hash.clone())
                    .or_default()
                    .push(PendingAttestation {
                        citer: citer.clone(),
                        sender: sender.clone(),
                        root: attestation.root.clone(),
                        carries_envelopes,
                    });
            }
        }
    }

    fn fix(&mut self, target: BlockHash, citer: BlockHash, record: R) {
        let root = record.post_root().clone();
        let digest = record.digest();
        let misattested = self
            .pending_attestations
            .get(&target)
            .is_some_and(|pending| pending.iter().any(|p| p.root != root));
        let encoded = misattested.then(|| record.encode());
        if let Err(error) = self.store.fix(target.clone(), citer, record) {
            self.store_error = Some(error.to_string());
            return;
        }
        match self.spec.front() {
            Some(own)
                if own.target() == &target
                    && own.post_root() == &root
                    && own.digest() == digest =>
            {
                self.spec.pop_front();
            }
            _ => self.spec.clear(),
        }
        self.citations.remove(&target);
        for pending in self
            .pending_attestations
            .remove(&target)
            .unwrap_or_default()
        {
            self.pending_by_citer.remove(&pending.citer);
            if pending.root == root {
                if pending.carries_envelopes {
                    let attestation = Attestation {
                        block_hash: target.clone(),
                        root: root.clone(),
                    };
                    self.inclusion_due_for(&pending.citer, &pending.sender, &attestation);
                }
                continue;
            }
            self.evidence.push(Evidence {
                offender: pending.sender,
                citer: pending.citer,
                target: target.clone(),
                offence: Offence::Misattestation {
                    attested: pending.root,
                    executed: root.clone(),
                    canonical_record: encoded.clone(),
                },
            });
        }
        self.prune();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::v6::block_record::BlockRecord;

    fn h(s: &str) -> Bytes { Bytes::copy_from_slice(s.as_bytes()) }

    fn rec(target: &str, pre: &str, post: &str) -> BlockRecord {
        BlockRecord::empty(h(target), h(pre), h(post))
    }

    fn block(name: &str, sender: &str, records: Vec<BlockRecord>) -> EmittedBlock<BlockRecord> {
        EmittedBlock {
            hash: h(name),
            sender: h(sender),
            envelope_ids: vec![],
            records,
        }
    }

    fn core() -> ExecutorCore<BlockRecord> {
        ExecutorCore::with_store(
            CoreParams {
                max_speculation: 4,
                fallback_distance: 2,
            },
            Box::new(MemoryRecordStore::new(
                h("g"),
                h("r0"),
                Retention::LastBlocks(2),
            )),
        )
    }

    #[test]
    fn fixed_records_are_retained_up_to_the_bound() {
        let mut c = core();
        c.emit(block("b1", "v1", vec![]));
        c.emit(block("b2", "v2", vec![rec("b1", "r0", "r1")]));
        c.emit(block("b3", "v3", vec![rec("b2", "r1", "r2")]));
        c.emit(block("b4", "v4", vec![rec("b3", "r2", "r3")]));
        for _ in 0..3 {
            let Action::Replay {
                target,
                citer,
                record,
            } = c.next_action()
            else {
                panic!("expected a replay")
            };
            let root = record.post_root().clone();
            c.apply_replay(&target, &citer, &record, Ok(root));
        }
        assert_eq!(c.record(&h("b1")), None);
        assert_eq!(c.record(&h("b2")), Some(rec("b2", "r1", "r2")));
        assert_eq!(c.record(&h("b3")), Some(rec("b3", "r2", "r3")));
    }

    fn store_with_b1_fixed() -> Box<dyn RecordStore<BlockRecord>> {
        let mut store = MemoryRecordStore::new(h("g"), h("r0"), Retention::default());
        store.fix(h("b1"), h("c1"), rec("b1", "r0", "r1")).unwrap();
        Box::new(store)
    }

    struct FailingStore(MemoryRecordStore<BlockRecord>);

    impl RecordStore<BlockRecord> for FailingStore {
        fn canonical_len(&self) -> usize { self.0.canonical_len() }

        fn canonical_root(&self) -> StateHash { self.0.canonical_root() }

        fn canonical_at(&self, index: usize) -> Option<BlockHash> { self.0.canonical_at(index) }

        fn executed(&self, block: &BlockHash) -> Option<StateHash> { self.0.executed(block) }

        fn record(&self, block: &BlockHash) -> Option<BlockRecord> { self.0.record(block) }

        fn fix(
            &mut self,
            _target: BlockHash,
            _citer: BlockHash,
            _record: BlockRecord,
        ) -> Result<(), shared::rust::store::key_value_store::KvStoreError> {
            Err(shared::rust::store::key_value_store::KvStoreError::IoError(
                "disk full".to_string(),
            ))
        }

        fn reset(&mut self) -> Result<(), shared::rust::store::key_value_store::KvStoreError> {
            self.0.reset()
        }

        fn resident(&self) -> usize { self.0.resident() }
    }

    #[test]
    fn a_resumed_prefix_is_not_replayed() {
        let mut c = ExecutorCore::with_store(CoreParams::default(), store_with_b1_fixed());
        c.emit(block("b1", "v1", vec![]));
        c.emit(block("b2", "v2", vec![rec("b1", "r0", "r1")]));
        assert_eq!(c.canonical_len(), 2);
        assert_eq!(c.attestation(), Attestation {
            block_hash: h("b1"),
            root: h("r1")
        });
        assert_eq!(c.next_action(), Action::Execute {
            target: h("b2"),
            pre_root: h("r1")
        });
    }

    #[test]
    fn a_block_in_the_stored_prefix_is_not_emitted_again_and_a_reset_starts_afresh() {
        let mut c = ExecutorCore::with_store(CoreParams::default(), store_with_b1_fixed());
        assert_eq!(c.emitted_len(), 2);
        assert!(c.is_emitted(&h("b1")));
        c.emit(block("b1", "v1", vec![]));
        assert_eq!(c.emitted_len(), 2);
        c.emit(block("x1", "v1", vec![]));
        assert_eq!(c.emitted_len(), 3);
        assert_eq!(c.next_action(), Action::Execute {
            target: h("x1"),
            pre_root: h("r1")
        });
        c.reset().unwrap();
        assert_eq!(c.canonical_len(), 1);
        assert_eq!(c.emitted_len(), 1);
        assert_eq!(c.executed(&h("b1")), None);
        c.emit(block("x1", "v1", vec![]));
        assert_eq!(c.next_action(), Action::Execute {
            target: h("x1"),
            pre_root: h("r0")
        });
    }

    fn name(prefix: &str, i: usize) -> String { format!("{prefix}{i}") }

    #[test]
    fn the_window_stays_bounded_while_the_prefix_grows() {
        let mut c = ExecutorCore::with_store(
            CoreParams::default(),
            Box::new(MemoryRecordStore::new(
                h("g"),
                h("r0"),
                Retention::LastBlocks(0),
            )),
        );
        let mut largest = 0;
        for i in 1..=300 {
            let records = if i > 1 {
                vec![rec(&name("b", i - 1), &name("r", i - 2), &name("r", i - 1))]
            } else {
                vec![]
            };
            c.emit(block(&name("b", i), &name("v", i % 3), records));
            c.observe_attestation(
                &h(&name("b", i)),
                &h(&name("v", i % 3)),
                &Attestation {
                    block_hash: h(&name("b", i.saturating_sub(2).max(1))),
                    root: h(&name("r", i.saturating_sub(2).max(1))),
                },
                true,
            );
            loop {
                match c.next_action() {
                    Action::Replay {
                        target,
                        citer,
                        record,
                    } => {
                        let root = record.post_root().clone();
                        c.apply_replay(&target, &citer, &record, Ok(root));
                    }
                    Action::Execute { target, pre_root } => {
                        let post = h(&format!("own-{}", String::from_utf8_lossy(&target)));
                        c.apply_execute(BlockRecord::empty(target, pre_root, post));
                    }
                    Action::Idle => break,
                }
            }
            while c.next_inclusion_check().is_some() {}
            largest = largest.max(c.resident() - c.store.resident());
        }
        assert_eq!(c.canonical_len(), 300);
        assert!(largest <= 64, "the window grew to {largest}");
        assert!(c.evidence().is_empty(), "{:?}", c.evidence());
    }

    #[test]
    fn an_attestation_of_a_block_never_emitted_is_dropped_when_its_attester_leaves_the_window() {
        let mut c = core();
        c.observe_attestation(
            &h("y"),
            &h("vy"),
            &Attestation {
                block_hash: h("ghost"),
                root: h("rg"),
            },
            true,
        );
        assert_eq!(c.pending_attestations.len(), 1);
        c.emit(block("y", "vy", vec![]));
        c.emit(block("z", "vz", vec![rec("y", "r0", "r1")]));
        let Action::Replay {
            target,
            citer,
            record,
        } = c.next_action()
        else {
            panic!("expected a replay")
        };
        c.apply_replay(&target, &citer, &record, Ok(h("r1")));
        assert_eq!(c.canonical_len(), 2);
        assert!(c.pending_attestations.is_empty());
        assert!(c.pending_by_citer.is_empty());
        assert!(!c.index.contains_key(&h("y")));
        assert_eq!(c.emitted_len(), 3);
    }

    #[test]
    fn a_failed_write_stops_the_core_and_is_reported_once() {
        let store = FailingStore(MemoryRecordStore::new(
            h("g"),
            h("r0"),
            Retention::default(),
        ));
        let mut c = ExecutorCore::with_store(CoreParams::default(), Box::new(store));
        c.emit(block("b1", "v1", vec![]));
        c.emit(block("b2", "v2", vec![rec("b1", "r0", "r1")]));
        let Action::Replay {
            target,
            citer,
            record,
        } = c.next_action()
        else {
            panic!("expected a replay")
        };
        c.apply_replay(&target, &citer, &record, Ok(h("r1")));
        assert_eq!(c.canonical_len(), 1);
        assert_eq!(c.next_action(), Action::Idle);
        assert!(c.take_store_error().is_some());
        assert!(c.take_store_error().is_none());
    }

    #[test]
    fn executes_speculatively_in_canonical_order() {
        let mut c = core();
        c.emit(block("b1", "v1", vec![]));
        c.emit(block("b2", "v2", vec![]));
        assert_eq!(c.next_action(), Action::Execute {
            target: h("b1"),
            pre_root: h("r0")
        });
        c.apply_execute(rec("b1", "r0", "r1"));
        assert_eq!(c.next_action(), Action::Execute {
            target: h("b2"),
            pre_root: h("r1")
        });
    }

    #[test]
    fn a_citation_matching_the_own_run_is_adopted_without_replay() {
        let mut c = core();
        c.emit(block("b1", "v1", vec![]));
        let action = c.next_action();
        assert!(matches!(action, Action::Execute { .. }), "{:?}", action);
        c.apply_execute(rec("b1", "r0", "r1"));
        c.emit(block("b2", "v2", vec![rec("b1", "r0", "r1")]));
        let _ = c.next_action();
        assert_eq!(c.executed(&h("b1")), Some(h("r1")));
        assert_eq!(c.canonical_len(), 2);
    }

    #[test]
    fn a_different_canonical_record_is_replayed_and_discards_speculation() {
        let mut c = core();
        c.emit(block("b1", "v1", vec![]));
        c.apply_execute(rec("b1", "r0", "mine"));
        c.emit(block("b2", "v2", vec![rec("b1", "r0", "theirs")]));
        let action = c.next_action();
        let Action::Replay {
            target,
            citer,
            record,
        } = action
        else {
            panic!("{:?}", action)
        };
        c.apply_replay(&target, &citer, &record, Ok(h("theirs")));
        assert_eq!(c.executed(&h("b1")), Some(h("theirs")));
        assert_eq!(c.speculation_len(), 0);
        assert_eq!(c.next_action(), Action::Execute {
            target: h("b2"),
            pre_root: h("theirs")
        });
    }

    #[test]
    fn an_invalid_record_yields_evidence_and_the_next_citation_wins() {
        let mut c = core();
        c.emit(block("b1", "v1", vec![]));
        c.emit(block("b2", "v2", vec![rec("b1", "r0", "bad")]));
        c.emit(block("b3", "v3", vec![rec("b1", "r0", "good")]));
        let Action::Replay {
            target,
            citer,
            record,
        } = c.next_action()
        else {
            panic!()
        };
        c.apply_replay(&target, &citer, &record, Ok(h("other")));
        assert_eq!(c.evidence()[0].offender, h("v2"));
        assert!(matches!(
            &c.evidence()[0].offence,
            Offence::InvalidRecord { reason: InvalidRecordReason::Replay, record }
                if *record == rec("b1", "r0", "bad").encode()
        ));
        assert_eq!(c.take_evidence().len(), 1);
        assert!(c.evidence().is_empty());
        let Action::Replay {
            target,
            citer,
            record,
        } = c.next_action()
        else {
            panic!()
        };
        assert_eq!(citer, h("b3"));
        c.apply_replay(&target, &citer, &record, Ok(h("good")));
        assert_eq!(c.executed(&h("b1")), Some(h("good")));
    }

    #[test]
    fn stale_records_are_skipped_without_evidence() {
        let mut c = core();
        c.emit(block("b1", "v1", vec![]));
        c.emit(block("b2", "v2", vec![rec("b1", "elsewhere", "x")]));
        assert!(matches!(c.next_action(), Action::Execute { .. }));
        assert!(c.evidence().is_empty());
    }

    #[test]
    fn a_citation_must_come_after_its_target() {
        let mut c = core();
        c.emit(block("b1", "v1", vec![rec("b2", "r0", "x")]));
        c.emit(block("b2", "v2", vec![]));
        assert!(matches!(c.next_action(), Action::Execute { .. }));
    }

    #[test]
    fn mismatched_envelopes_yield_evidence() {
        let mut c = core();
        c.emit(EmittedBlock {
            hash: h("b1"),
            sender: h("v1"),
            envelope_ids: vec![h("sig")],
            records: vec![],
        });
        c.emit(block("b2", "v2", vec![rec("b1", "r0", "r1")]));
        let _ = c.next_action();
        assert!(matches!(c.evidence()[0].offence, Offence::InvalidRecord {
            reason: InvalidRecordReason::Envelopes,
            ..
        }));
        assert_eq!(c.evidence()[0].offender, h("v2"));
    }

    #[test]
    fn nodes_cite_their_own_blocks_and_overdue_blocks() {
        let mut c = core();
        c.emit(block("b1", "me", vec![]));
        c.emit(block("b2", "other", vec![]));
        c.apply_execute(rec("b1", "r0", "r1"));
        c.apply_execute(rec("b2", "r1", "r2"));
        let cited: Vec<_> = c
            .records_to_cite(&h("me"), 10)
            .into_iter()
            .map(|r| r.target().clone())
            .collect();
        assert_eq!(cited, vec![h("b1")]);
        c.emit(block("b3", "other", vec![]));
        c.emit(block("b4", "other", vec![]));
        let cited: Vec<_> = c
            .records_to_cite(&h("me"), 10)
            .into_iter()
            .map(|r| r.target().clone())
            .collect();
        assert_eq!(cited, vec![h("b1"), h("b2")]);
        let to_cite = c.records_to_cite(&h("me"), 10);
        c.mark_cited(&to_cite);
        assert!(c.records_to_cite(&h("me"), 10).is_empty());
    }

    #[test]
    fn attestations_are_checked_when_the_root_is_fixed() {
        let mut c = core();
        c.emit(block("b1", "v1", vec![]));
        c.observe_attestation(
            &h("x"),
            &h("vx"),
            &Attestation {
                block_hash: h("b1"),
                root: h("wrong"),
            },
            true,
        );
        c.observe_attestation(
            &h("y"),
            &h("vy"),
            &Attestation {
                block_hash: h("b1"),
                root: h("r1"),
            },
            true,
        );
        assert_eq!(c.next_inclusion_check(), None);
        c.emit(block("b2", "v2", vec![rec("b1", "r0", "r1")]));
        let Action::Replay {
            target,
            citer,
            record,
        } = c.next_action()
        else {
            panic!()
        };
        c.apply_replay(&target, &citer, &record, Ok(h("r1")));
        assert_eq!(c.evidence().len(), 1);
        assert_eq!(c.evidence()[0].citer, h("x"));
        assert_eq!(c.evidence()[0].offender, h("vx"));
        assert_eq!(c.evidence()[0].offence, Offence::Misattestation {
            attested: h("wrong"),
            executed: h("r1"),
            canonical_record: Some(rec("b1", "r0", "r1").encode()),
        });
        assert_eq!(
            c.next_inclusion_check(),
            Some(InclusionCheck {
                block: h("y"),
                sender: h("vy"),
                attested: h("b1"),
                root: h("r1"),
            })
        );
        assert_eq!(c.next_inclusion_check(), None);
    }

    fn fixed_b1() -> ExecutorCore<BlockRecord> {
        let mut c = ExecutorCore::with_store(CoreParams::default(), store_with_b1_fixed());
        c.emit(block("b1", "v1", vec![]));
        c
    }

    fn attests_b1(root: &str) -> Attestation {
        Attestation {
            block_hash: h("b1"),
            root: h(root),
        }
    }

    #[test]
    fn a_block_attesting_a_fixed_root_is_due_for_its_inclusion_check_at_once() {
        let mut c = fixed_b1();
        c.observe_attestation(&h("y"), &h("vy"), &attests_b1("r1"), true);
        assert_eq!(
            c.next_inclusion_check(),
            Some(InclusionCheck {
                block: h("y"),
                sender: h("vy"),
                attested: h("b1"),
                root: h("r1"),
            })
        );
        assert!(c.evidence().is_empty());
    }

    #[test]
    fn a_block_without_envelopes_or_with_a_wrong_root_gets_no_inclusion_check() {
        let mut c = fixed_b1();
        c.observe_attestation(&h("y"), &h("vy"), &attests_b1("r1"), false);
        c.observe_attestation(&h("z"), &h("vz"), &attests_b1("wrong"), true);
        assert_eq!(c.next_inclusion_check(), None);
        assert_eq!(c.evidence().len(), 1);
        assert!(matches!(
            c.evidence()[0].offence,
            Offence::Misattestation { .. }
        ));
    }

    #[test]
    fn each_block_is_checked_once_and_a_reset_forgets_the_checks() {
        let mut c = fixed_b1();
        c.observe_attestation(&h("y"), &h("vy"), &attests_b1("r1"), true);
        c.observe_attestation(&h("y"), &h("vy"), &attests_b1("r1"), true);
        assert!(c.next_inclusion_check().is_some());
        assert_eq!(c.next_inclusion_check(), None);
        c.observe_attestation(&h("w"), &h("vw"), &attests_b1("r1"), true);
        c.reset().unwrap();
        assert_eq!(c.next_inclusion_check(), None);
    }

    #[test]
    fn two_runners_with_different_races_converge_on_one_root() {
        let mut a = core();
        let mut b = core();
        for c in [&mut a, &mut b] {
            c.emit(block("b1", "va", vec![]));
        }
        a.apply_execute(rec("b1", "r0", "ra"));
        b.apply_execute(rec("b1", "r0", "rb"));
        let citing = || block("c1", "va", vec![rec("b1", "r0", "ra")]);
        a.emit(citing());
        b.emit(citing());
        let _ = a.next_action();
        let Action::Replay {
            target,
            citer,
            record,
        } = b.next_action()
        else {
            panic!()
        };
        b.apply_replay(&target, &citer, &record, Ok(h("ra")));
        assert_eq!(a.executed(&h("b1")), b.executed(&h("b1")));
    }

    #[test]
    fn an_own_run_with_the_same_root_but_another_record_is_not_adopted() {
        let mut c = core();
        c.emit(block("b1", "v1", vec![]));
        let own = BlockRecord::with_mid(h("b1"), h("r0"), h("own"), h("r1"));
        c.apply_execute(own.clone());
        let cited = BlockRecord::with_mid(h("b1"), h("r0"), h("cited"), h("r1"));
        c.emit(block("b2", "v2", vec![cited.clone()]));
        let Action::Replay {
            target,
            citer,
            record,
        } = c.next_action()
        else {
            panic!("a record that differs from the own run must be replayed")
        };
        assert_eq!(record, cited);
        c.apply_replay(&target, &citer, &record, Ok(h("r1")));
        assert_eq!(c.executed(&h("b1")), Some(h("r1")));
        assert_eq!(c.speculation_len(), 0);
        assert!(c.records_to_cite(&h("v1"), 10).is_empty());
    }
}
