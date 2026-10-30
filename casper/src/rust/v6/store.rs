use std::collections::{HashMap, VecDeque};

use models::rust::block_hash::BlockHash;

use super::body::StateHash;
use super::record::ExecutionRecord;

pub const DEFAULT_RETAINED_RECORDS: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retention {
    LastBlocks(usize),
}

impl Default for Retention {
    fn default() -> Self { Retention::LastBlocks(DEFAULT_RETAINED_RECORDS) }
}

pub trait RecordStore<R: ExecutionRecord>: Send {
    fn canonical_len(&self) -> usize;

    fn canonical_root(&self) -> &StateHash;

    fn executed(&self, block: &BlockHash) -> Option<&StateHash>;

    fn record(&self, block: &BlockHash) -> Option<&R>;

    fn fix(&mut self, target: BlockHash, record: R);
}

pub struct MemoryRecordStore<R: ExecutionRecord> {
    retention: Retention,
    canonical_len: usize,
    canonical_root: StateHash,
    executed: HashMap<BlockHash, StateHash>,
    retained: HashMap<BlockHash, R>,
    retained_order: VecDeque<BlockHash>,
}

impl<R: ExecutionRecord> MemoryRecordStore<R> {
    pub fn new(genesis: BlockHash, genesis_root: StateHash, retention: Retention) -> Self {
        MemoryRecordStore {
            retention,
            canonical_len: 1,
            canonical_root: genesis_root.clone(),
            executed: HashMap::from([(genesis, genesis_root)]),
            retained: HashMap::new(),
            retained_order: VecDeque::new(),
        }
    }

    fn retain(&mut self, target: BlockHash, record: R) {
        let Retention::LastBlocks(bound) = self.retention;
        if bound == 0 {
            return;
        }
        if self.retained.insert(target.clone(), record).is_none() {
            self.retained_order.push_back(target);
        }
        while self.retained_order.len() > bound {
            if let Some(oldest) = self.retained_order.pop_front() {
                self.retained.remove(&oldest);
            }
        }
    }
}

impl<R: ExecutionRecord> RecordStore<R> for MemoryRecordStore<R> {
    fn canonical_len(&self) -> usize { self.canonical_len }

    fn canonical_root(&self) -> &StateHash { &self.canonical_root }

    fn executed(&self, block: &BlockHash) -> Option<&StateHash> { self.executed.get(block) }

    fn record(&self, block: &BlockHash) -> Option<&R> { self.retained.get(block) }

    fn fix(&mut self, target: BlockHash, record: R) {
        let root = record.post_root().clone();
        self.executed.insert(target.clone(), root.clone());
        self.canonical_root = root;
        self.canonical_len += 1;
        self.retain(target, record);
    }
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;
    use crate::rust::v6::block_record::BlockRecord;

    fn h(s: &str) -> Bytes { Bytes::copy_from_slice(s.as_bytes()) }

    #[test]
    fn the_store_keeps_every_root_and_the_last_records_within_the_bound() {
        let mut store: MemoryRecordStore<BlockRecord> =
            MemoryRecordStore::new(h("g"), h("r0"), Retention::LastBlocks(2));
        assert_eq!(store.canonical_len(), 1);
        assert_eq!(store.executed(&h("g")), Some(&h("r0")));
        for (target, pre, post) in [("b1", "r0", "r1"), ("b2", "r1", "r2"), ("b3", "r2", "r3")] {
            store.fix(h(target), BlockRecord::empty(h(target), h(pre), h(post)));
        }
        assert_eq!(store.canonical_len(), 4);
        assert_eq!(store.canonical_root(), &h("r3"));
        assert_eq!(store.executed(&h("b1")), Some(&h("r1")));
        assert_eq!(store.record(&h("b1")), None);
        assert_eq!(
            store.record(&h("b2")),
            Some(&BlockRecord::empty(h("b2"), h("r1"), h("r2")))
        );
        assert!(store.record(&h("b3")).is_some());
    }

    #[test]
    fn a_zero_bound_keeps_roots_but_no_records() {
        let mut store: MemoryRecordStore<BlockRecord> =
            MemoryRecordStore::new(h("g"), h("r0"), Retention::LastBlocks(0));
        store.fix(h("b1"), BlockRecord::empty(h("b1"), h("r0"), h("r1")));
        assert_eq!(store.executed(&h("b1")), Some(&h("r1")));
        assert_eq!(store.record(&h("b1")), None);
        assert_eq!(Retention::default(), Retention::LastBlocks(1024));
    }
}
