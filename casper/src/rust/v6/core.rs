use std::collections::{HashMap, HashSet, VecDeque};

use models::rust::block_hash::BlockHash;
use prost::bytes::Bytes;

use super::body::{Attestation, StateHash};
use super::record::ExecutionRecord;

#[derive(Clone, Debug)]
pub struct CoreParams {
    pub max_speculation: usize,
    pub fallback_distance: usize,
    pub retained_records: usize,
}

impl Default for CoreParams {
    fn default() -> Self {
        CoreParams {
            max_speculation: 32,
            fallback_distance: 8,
            retained_records: 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvidenceKind {
    InvalidRecord,
    MismatchedEnvelopes,
    Misattestation {
        attested: StateHash,
        executed: StateHash,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evidence {
    pub against: BlockHash,
    pub target: BlockHash,
    pub kind: EvidenceKind,
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
    canon_pos: usize,
    canon_root: StateHash,
    executed: HashMap<BlockHash, StateHash>,
    spec: VecDeque<R>,
    citations: HashMap<BlockHash, Vec<Candidate<R>>>,
    dismissed: HashSet<(BlockHash, BlockHash)>,
    cited: HashSet<(BlockHash, StateHash)>,
    pending_attestations: HashMap<BlockHash, Vec<(BlockHash, StateHash)>>,
    evidence: Vec<Evidence>,
    retained: HashMap<BlockHash, R>,
    retained_order: VecDeque<BlockHash>,
}

impl<R: ExecutionRecord> ExecutorCore<R> {
    pub fn new(genesis: BlockHash, genesis_root: StateHash, params: CoreParams) -> Self {
        ExecutorCore {
            params,
            order: vec![genesis.clone()],
            index: HashMap::from([(genesis.clone(), 0)]),
            senders: HashMap::new(),
            envelopes: HashMap::new(),
            canon_pos: 1,
            canon_root: genesis_root.clone(),
            executed: HashMap::from([(genesis, genesis_root)]),
            spec: VecDeque::new(),
            citations: HashMap::new(),
            dismissed: HashSet::new(),
            cited: HashSet::new(),
            pending_attestations: HashMap::new(),
            evidence: Vec::new(),
            retained: HashMap::new(),
            retained_order: VecDeque::new(),
        }
    }

    pub fn emit(&mut self, block: EmittedBlock<R>) {
        if self.index.contains_key(&block.hash) {
            return;
        }
        let citer_index = self.order.len();
        self.index.insert(block.hash.clone(), citer_index);
        self.order.push(block.hash.clone());
        self.senders.insert(block.hash.clone(), block.sender);
        self.envelopes
            .insert(block.hash.clone(), block.envelope_ids);
        for record in block.records {
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
            block_hash: self.order[self.canon_pos - 1].clone(),
            root: self.canon_root.clone(),
        }
    }

    pub fn executed(&self, block: &BlockHash) -> Option<&StateHash> { self.executed.get(block) }

    pub fn record(&self, block: &BlockHash) -> Option<&R> { self.retained.get(block) }

    pub fn canonical_len(&self) -> usize { self.canon_pos }

    pub fn emitted_len(&self) -> usize { self.order.len() }

    pub fn speculation_len(&self) -> usize { self.spec.len() }

    pub fn evidence(&self) -> &[Evidence] { &self.evidence }

    pub fn next_action(&mut self) -> Action<R> {
        loop {
            if self.canon_pos >= self.order.len() {
                break;
            }
            let target = self.order[self.canon_pos].clone();
            let target_index = self.canon_pos;
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
            if *candidate.record.pre_root() != self.canon_root {
                self.dismissed.insert((candidate.citer, target));
                continue;
            }
            let expected = self.envelopes.get(&target).cloned().unwrap_or_default();
            if candidate.record.envelope_ids() != expected.as_slice() {
                self.evidence.push(Evidence {
                    against: candidate.citer.clone(),
                    target: target.clone(),
                    kind: EvidenceKind::MismatchedEnvelopes,
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
        let next = self.canon_pos + self.spec.len();
        if self.spec.len() < self.params.max_speculation && next < self.order.len() {
            let pre_root = self
                .spec
                .back()
                .map(|r| r.post_root().clone())
                .unwrap_or_else(|| self.canon_root.clone());
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
        if self.order.get(self.canon_pos) != Some(target) || *record.pre_root() != self.canon_root {
            return;
        }
        match result {
            Ok(root) if root == *record.post_root() => self.fix(target.clone(), record.clone()),
            _ => {
                self.evidence.push(Evidence {
                    against: citer.clone(),
                    target: target.clone(),
                    kind: EvidenceKind::InvalidRecord,
                });
                self.dismissed.insert((citer.clone(), target.clone()));
            }
        }
    }

    pub fn apply_execute(&mut self, record: R) {
        let next = self.canon_pos + self.spec.len();
        let expected_pre = self
            .spec
            .back()
            .map(|r| r.post_root().clone())
            .unwrap_or_else(|| self.canon_root.clone());
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

    pub fn observe_attestation(&mut self, citer: &BlockHash, attestation: &Attestation) {
        match self.executed.get(&attestation.block_hash) {
            Some(executed) => {
                if *executed != attestation.root {
                    self.evidence.push(Evidence {
                        against: citer.clone(),
                        target: attestation.block_hash.clone(),
                        kind: EvidenceKind::Misattestation {
                            attested: attestation.root.clone(),
                            executed: executed.clone(),
                        },
                    });
                }
            }
            None => self
                .pending_attestations
                .entry(attestation.block_hash.clone())
                .or_default()
                .push((citer.clone(), attestation.root.clone())),
        }
    }

    fn fix(&mut self, target: BlockHash, record: R) {
        let root = record.post_root().clone();
        let digest = record.digest();
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
        if self.params.retained_records > 0 {
            if self.retained.insert(target.clone(), record).is_none() {
                self.retained_order.push_back(target.clone());
            }
            while self.retained_order.len() > self.params.retained_records {
                if let Some(oldest) = self.retained_order.pop_front() {
                    self.retained.remove(&oldest);
                }
            }
        }
        self.executed.insert(target.clone(), root.clone());
        self.canon_root = root.clone();
        self.canon_pos += 1;
        self.citations.remove(&target);
        if let Some(pending) = self.pending_attestations.remove(&target) {
            for (citer, attested) in pending {
                if attested != root {
                    self.evidence.push(Evidence {
                        against: citer,
                        target: target.clone(),
                        kind: EvidenceKind::Misattestation {
                            attested,
                            executed: root.clone(),
                        },
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::v6::concurrent_record::ConcurrentRecord;

    fn h(s: &str) -> Bytes { Bytes::copy_from_slice(s.as_bytes()) }

    fn rec(target: &str, pre: &str, post: &str) -> ConcurrentRecord {
        ConcurrentRecord::empty(h(target), h(pre), h(post))
    }

    fn block(
        name: &str,
        sender: &str,
        records: Vec<ConcurrentRecord>,
    ) -> EmittedBlock<ConcurrentRecord> {
        EmittedBlock {
            hash: h(name),
            sender: h(sender),
            envelope_ids: vec![],
            records,
        }
    }

    fn core() -> ExecutorCore<ConcurrentRecord> {
        ExecutorCore::new(h("g"), h("r0"), CoreParams {
            max_speculation: 4,
            fallback_distance: 2,
            retained_records: 2,
        })
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
        assert_eq!(c.record(&h("b2")), Some(&rec("b2", "r1", "r2")));
        assert_eq!(c.record(&h("b3")), Some(&rec("b3", "r2", "r3")));
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
        assert_eq!(c.executed(&h("b1")), Some(&h("r1")));
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
        assert_eq!(c.executed(&h("b1")), Some(&h("theirs")));
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
        assert_eq!(c.evidence()[0].kind, EvidenceKind::InvalidRecord);
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
        assert_eq!(c.executed(&h("b1")), Some(&h("good")));
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
        assert_eq!(c.evidence()[0].kind, EvidenceKind::MismatchedEnvelopes);
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
            .map(|r| r.target)
            .collect();
        assert_eq!(cited, vec![h("b1")]);
        c.emit(block("b3", "other", vec![]));
        c.emit(block("b4", "other", vec![]));
        let cited: Vec<_> = c
            .records_to_cite(&h("me"), 10)
            .into_iter()
            .map(|r| r.target)
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
        c.observe_attestation(&h("x"), &Attestation {
            block_hash: h("b1"),
            root: h("wrong"),
        });
        c.observe_attestation(&h("y"), &Attestation {
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
        assert_eq!(c.evidence()[0].against, h("x"));
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
    fn an_own_run_with_the_same_root_but_other_verdicts_is_not_adopted() {
        let mut c = core();
        c.emit(block("b1", "v1", vec![]));
        let own = ConcurrentRecord {
            failed: vec![true],
            ..rec("b1", "r0", "r1")
        };
        c.apply_execute(own.clone());
        let cited = ConcurrentRecord {
            failed: vec![false],
            ..rec("b1", "r0", "r1")
        };
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
        assert_eq!(c.executed(&h("b1")), Some(&h("r1")));
        assert_eq!(c.speculation_len(), 0);
        assert!(c.records_to_cite(&h("v1"), 10).is_empty());
    }
}
