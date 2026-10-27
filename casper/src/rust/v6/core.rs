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

pub struct EmittedBlock<R> {
    pub hash: BlockHash,
    pub sender: Bytes,
    pub envelope_ids: Vec<Bytes>,
    pub records: Vec<R>,
}

pub struct ExecutorCore<R: ExecutionRecord> {
    params: CoreParams,
    order: Vec<BlockHash>,
    index: HashMap<BlockHash, usize>,
    senders: HashMap<BlockHash, Bytes>,
    envelopes: HashMap<BlockHash, Vec<Bytes>>,
    store: Box<dyn RecordStore<R>>,
    spec: VecDeque<R>,
    citations: HashMap<BlockHash, Vec<Candidate<R>>>,
    dismissed: HashSet<(BlockHash, BlockHash)>,
    cited: HashSet<(BlockHash, StateHash)>,
    pending_attestations: HashMap<BlockHash, Vec<(BlockHash, Bytes, StateHash)>>,
    evidence: Vec<Evidence>,
    diverged: bool,
    store_error: Option<String>,
}

impl<R: ExecutionRecord> ExecutorCore<R> {
    pub fn new(genesis: BlockHash, genesis_root: StateHash, params: CoreParams) -> Self {
        let store = MemoryRecordStore::new(genesis.clone(), genesis_root, Retention::default());
        Self::with_store(genesis, params, Box::new(store))
    }

    pub fn with_store(
        genesis: BlockHash,
        params: CoreParams,
        store: Box<dyn RecordStore<R>>,
    ) -> Self {
        ExecutorCore {
            params,
            order: vec![genesis.clone()],
            index: HashMap::from([(genesis, 0)]),
            senders: HashMap::new(),
            envelopes: HashMap::new(),
            store,
            spec: VecDeque::new(),
            citations: HashMap::new(),
            dismissed: HashSet::new(),
            cited: HashSet::new(),
            pending_attestations: HashMap::new(),
            evidence: Vec::new(),
            diverged: false,
            store_error: None,
        }
    }

    pub fn reset(&mut self) -> Result<(), String> {
        self.store.reset().map_err(|error| error.to_string())?;
        let anchor = self.order[0].clone();
        self.order = vec![anchor.clone()];
        self.index = HashMap::from([(anchor, 0)]);
        self.senders.clear();
        self.envelopes.clear();
        self.spec.clear();
        self.citations.clear();
        self.dismissed.clear();
        self.cited.clear();
        self.pending_attestations.clear();
        self.diverged = false;
        self.store_error = None;
        Ok(())
    }

    pub fn diverged(&self) -> bool { self.diverged }

    pub fn take_store_error(&mut self) -> Option<String> { self.store_error.take() }

    fn sender_of(&self, block: &BlockHash) -> Bytes {
        self.senders.get(block).cloned().unwrap_or_default()
    }

    fn canon_pos(&self) -> usize { self.store.canonical_len() }

    fn canon_root(&self) -> StateHash { self.store.canonical_root() }

    pub fn emit(&mut self, block: EmittedBlock<R>) {
        if self.index.contains_key(&block.hash) {
            return;
        }
        let citer_index = self.order.len();
        let fixed = citer_index < self.canon_pos();
        if fixed && self.store.canonical_at(citer_index).as_ref() != Some(&block.hash) {
            self.diverged = true;
        }
        self.index.insert(block.hash.clone(), citer_index);
        self.order.push(block.hash.clone());
        self.senders.insert(block.hash.clone(), block.sender);
        self.envelopes
            .insert(block.hash.clone(), block.envelope_ids);
        for record in block.records {
            if self.store.executed(record.target()).is_some() {
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

    pub fn emitted_len(&self) -> usize { self.order.len() }

    pub fn speculation_len(&self) -> usize { self.spec.len() }

    pub fn evidence(&self) -> &[Evidence] { &self.evidence }

    pub fn take_evidence(&mut self) -> Vec<Evidence> { std::mem::take(&mut self.evidence) }

    pub fn next_action(&mut self) -> Action<R> {
        loop {
            if self.store_error.is_some() || self.diverged {
                return Action::Idle;
            }
            if self.canon_pos() >= self.order.len() {
                break;
            }
            let target = self.order[self.canon_pos()].clone();
            let target_index = self.canon_pos();
            let mut candidates: Vec<Candidate<R>> = self
                .citations
                .get(&target)
                .map(|c| {
                    c.iter()
                        .filter(|c| c.citer_index > target_index)
                        .filter(|c| !self.dismissed.contains(&(c.citer.clone(), target.clone())))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            candidates.sort_by_key(|c| c.citer_index);
            let Some(candidate) = candidates.into_iter().next() else {
                break;
            };
            if *candidate.record.pre_root() != self.canon_root() {
                self.dismissed.insert((candidate.citer, target));
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
                self.dismissed.insert((candidate.citer, target));
                continue;
            }
            let matches_own = self
                .spec
                .front()
                .map(|own| own.target() == &target && own.digest() == candidate.record.digest())
                .unwrap_or(false);
            if matches_own {
                self.fix(target, candidate.record);
                continue;
            }
            return Action::Replay {
                target,
                citer: candidate.citer,
                record: candidate.record,
            };
        }
        let next = self.canon_pos() + self.spec.len();
        if self.spec.len() < self.params.max_speculation && next < self.order.len() {
            let pre_root = self
                .spec
                .back()
                .map(|r| r.post_root().clone())
                .unwrap_or_else(|| self.canon_root());
            return Action::Execute {
                target: self.order[next].clone(),
                pre_root,
            };
        }
        Action::Idle
    }

    pub fn apply_replay(
        &mut self,
        target: &BlockHash,
        citer: &BlockHash,
        record: &R,
        result: Result<StateHash, String>,
    ) {
        if self.order.get(self.canon_pos()) != Some(target)
            || *record.pre_root() != self.canon_root()
        {
            return;
        }
        match result {
            Ok(root) if root == *record.post_root() => self.fix(target.clone(), record.clone()),
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
                self.dismissed.insert((citer.clone(), target.clone()));
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
        if self.order.get(next) == Some(record.target()) && *record.pre_root() == expected_pre {
            self.spec.push_back(record);
        }
    }

    pub fn records_to_cite(&self, self_key: &Bytes, max: usize) -> Vec<R> {
        let newest = self.order.len().saturating_sub(1);
        self.spec
            .iter()
            .filter(|r| {
                !self
                    .cited
                    .contains(&(r.target().clone(), r.pre_root().clone()))
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
                .insert((r.target().clone(), r.pre_root().clone()));
        }
    }

    pub fn observe_attestation(
        &mut self,
        citer: &BlockHash,
        sender: &Bytes,
        attestation: &Attestation,
    ) {
        match self.store.executed(&attestation.block_hash) {
            Some(executed) => {
                if executed != attestation.root {
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
            None => self
                .pending_attestations
                .entry(attestation.block_hash.clone())
                .or_default()
                .push((citer.clone(), sender.clone(), attestation.root.clone())),
        }
    }

    fn fix(&mut self, target: BlockHash, record: R) {
        let root = record.post_root().clone();
        let digest = record.digest();
        let misattested = self
            .pending_attestations
            .get(&target)
            .is_some_and(|pending| pending.iter().any(|(_, _, attested)| *attested != root));
        let encoded = misattested.then(|| record.encode());
        if let Err(error) = self.store.fix(target.clone(), record) {
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
        for (citer, sender, attested) in self
            .pending_attestations
            .remove(&target)
            .unwrap_or_default()
        {
            if attested == root {
                continue;
            }
            self.evidence.push(Evidence {
                offender: sender,
                citer,
                target: target.clone(),
                offence: Offence::Misattestation {
                    attested,
                    executed: root.clone(),
                    canonical_record: encoded.clone(),
                },
            });
        }
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
            h("g"),
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
        store.fix(h("b1"), rec("b1", "r0", "r1")).unwrap();
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
            _record: BlockRecord,
        ) -> Result<(), shared::rust::store::key_value_store::KvStoreError> {
            Err(shared::rust::store::key_value_store::KvStoreError::IoError(
                "disk full".to_string(),
            ))
        }

        fn reset(&mut self) -> Result<(), shared::rust::store::key_value_store::KvStoreError> {
            self.0.reset()
        }
    }

    #[test]
    fn a_resumed_prefix_is_not_replayed() {
        let mut c = ExecutorCore::with_store(h("g"), CoreParams::default(), store_with_b1_fixed());
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
        assert!(!c.diverged());
    }

    #[test]
    fn a_different_order_below_the_stored_prefix_is_divergence() {
        let mut c = ExecutorCore::with_store(h("g"), CoreParams::default(), store_with_b1_fixed());
        c.emit(block("x1", "v1", vec![]));
        assert!(c.diverged());
        assert_eq!(c.next_action(), Action::Idle);
        c.reset().unwrap();
        assert!(!c.diverged());
        assert_eq!(c.canonical_len(), 1);
        assert_eq!(c.executed(&h("b1")), None);
        c.emit(block("x1", "v1", vec![]));
        assert_eq!(c.next_action(), Action::Execute {
            target: h("x1"),
            pre_root: h("r0")
        });
    }

    #[test]
    fn a_failed_write_stops_the_core_and_is_reported_once() {
        let store = FailingStore(MemoryRecordStore::new(
            h("g"),
            h("r0"),
            Retention::default(),
        ));
        let mut c = ExecutorCore::with_store(h("g"), CoreParams::default(), Box::new(store));
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
        c.observe_attestation(&h("x"), &h("vx"), &Attestation {
            block_hash: h("b1"),
            root: h("wrong"),
        });
        c.observe_attestation(&h("y"), &h("vy"), &Attestation {
            block_hash: h("b1"),
            root: h("r1"),
        });
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
