use std::collections::{HashMap, VecDeque};
use std::marker::PhantomData;
use std::sync::Arc;

use models::rust::block_hash::BlockHash;
use prost::bytes::Bytes;
use prost::Message;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};

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

    fn canonical_root(&self) -> StateHash;

    fn canonical_at(&self, index: usize) -> Option<BlockHash>;

    fn executed(&self, block: &BlockHash) -> Option<StateHash>;

    fn record(&self, block: &BlockHash) -> Option<R>;

    fn fix(&mut self, target: BlockHash, record: R) -> Result<(), KvStoreError>;

    fn reset(&mut self) -> Result<(), KvStoreError>;
}

struct Prefix {
    anchor: BlockHash,
    anchor_root: StateHash,
    positions: Vec<BlockHash>,
    roots: HashMap<BlockHash, StateHash>,
    canonical_root: StateHash,
}

impl Prefix {
    fn new(anchor: BlockHash, anchor_root: StateHash) -> Self {
        Prefix {
            positions: vec![anchor.clone()],
            roots: HashMap::from([(anchor.clone(), anchor_root.clone())]),
            canonical_root: anchor_root.clone(),
            anchor,
            anchor_root,
        }
    }

    fn push(&mut self, target: BlockHash, root: StateHash) {
        self.positions.push(target.clone());
        self.roots.insert(target, root.clone());
        self.canonical_root = root;
    }
}

pub struct MemoryRecordStore<R: ExecutionRecord> {
    retention: Retention,
    prefix: Prefix,
    retained: HashMap<BlockHash, R>,
    retained_order: VecDeque<BlockHash>,
}

impl<R: ExecutionRecord> MemoryRecordStore<R> {
    pub fn new(genesis: BlockHash, genesis_root: StateHash, retention: Retention) -> Self {
        MemoryRecordStore {
            retention,
            prefix: Prefix::new(genesis, genesis_root),
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
    fn canonical_len(&self) -> usize { self.prefix.positions.len() }

    fn canonical_root(&self) -> StateHash { self.prefix.canonical_root.clone() }

    fn canonical_at(&self, index: usize) -> Option<BlockHash> {
        self.prefix.positions.get(index).cloned()
    }

    fn executed(&self, block: &BlockHash) -> Option<StateHash> {
        self.prefix.roots.get(block).cloned()
    }

    fn record(&self, block: &BlockHash) -> Option<R> { self.retained.get(block).cloned() }

    fn fix(&mut self, target: BlockHash, record: R) -> Result<(), KvStoreError> {
        self.prefix.push(target.clone(), record.post_root().clone());
        self.retain(target, record);
        Ok(())
    }

    fn reset(&mut self) -> Result<(), KvStoreError> {
        self.prefix = Prefix::new(self.prefix.anchor.clone(), self.prefix.anchor_root.clone());
        self.retained.clear();
        self.retained_order.clear();
        Ok(())
    }
}

#[derive(Clone, PartialEq, ::prost::Message)]
struct ExecutorMetaProto {
    #[prost(bytes = "bytes", tag = "1")]
    anchor: Bytes,
    #[prost(bytes = "bytes", tag = "2")]
    anchor_root: Bytes,
    #[prost(uint64, tag = "3")]
    canonical_len: u64,
    #[prost(bytes = "bytes", tag = "4")]
    canonical_root: Bytes,
}

const META_KEY: &[u8] = b"meta";
const POSITION: u8 = b'p';
const ROOT: u8 = b'r';
const RECORD: u8 = b'c';

fn keyed(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(1 + body.len());
    key.push(kind);
    key.extend_from_slice(body);
    key
}

fn position_key(index: usize) -> Vec<u8> { keyed(POSITION, &(index as u64).to_be_bytes()) }

fn corrupt(message: &str) -> KvStoreError {
    KvStoreError::InvalidArgument(format!("v6 executor state: {message}"))
}

pub struct LmdbRecordStore<R: ExecutionRecord> {
    db: Arc<dyn KeyValueStore>,
    prefix: Prefix,
    record_type: PhantomData<fn() -> R>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opened {
    Resumed,
    Fresh,
    ReplacedOtherAnchor,
}

impl<R: ExecutionRecord> LmdbRecordStore<R> {
    pub fn open(
        db: Arc<dyn KeyValueStore>,
        anchor: BlockHash,
        anchor_root: StateHash,
    ) -> Result<(Self, Opened), KvStoreError> {
        let mut store = LmdbRecordStore {
            db,
            prefix: Prefix::new(anchor.clone(), anchor_root.clone()),
            record_type: PhantomData,
        };
        let Some(bytes) = store.db.get_one(&META_KEY.to_vec())? else {
            store.write_fresh()?;
            return Ok((store, Opened::Fresh));
        };
        let meta = ExecutorMetaProto::decode(bytes.as_slice())
            .map_err(|error| corrupt(&error.to_string()))?;
        if meta.anchor != anchor || meta.anchor_root != anchor_root {
            store.reset()?;
            return Ok((store, Opened::ReplacedOtherAnchor));
        }
        let len = usize::try_from(meta.canonical_len).map_err(|_| corrupt("length"))?;
        let position_keys: Vec<Vec<u8>> = (1..len).map(position_key).collect();
        let positions = store.db.get(&position_keys)?;
        let mut hashes = Vec::with_capacity(positions.len());
        for position in positions {
            hashes.push(Bytes::from(
                position.ok_or_else(|| corrupt("a missing position"))?,
            ));
        }
        let root_keys: Vec<Vec<u8>> = hashes.iter().map(|hash| keyed(ROOT, hash)).collect();
        let roots = store.db.get(&root_keys)?;
        for (hash, root) in hashes.into_iter().zip(roots) {
            let root = Bytes::from(root.ok_or_else(|| corrupt("a missing root"))?);
            store.prefix.push(hash, root);
        }
        if store.prefix.canonical_root != meta.canonical_root {
            return Err(corrupt("the canonical root disagrees with the positions"));
        }
        Ok((store, Opened::Resumed))
    }

    fn meta(&self) -> Vec<u8> {
        ExecutorMetaProto {
            anchor: self.prefix.anchor.clone(),
            anchor_root: self.prefix.anchor_root.clone(),
            canonical_len: self.prefix.positions.len() as u64,
            canonical_root: self.prefix.canonical_root.clone(),
        }
        .encode_to_vec()
    }

    fn write_fresh(&self) -> Result<(), KvStoreError> {
        self.db.put(vec![
            (
                keyed(ROOT, &self.prefix.anchor),
                self.prefix.anchor_root.to_vec(),
            ),
            (META_KEY.to_vec(), self.meta()),
        ])
    }
}

impl<R: ExecutionRecord> RecordStore<R> for LmdbRecordStore<R> {
    fn canonical_len(&self) -> usize { self.prefix.positions.len() }

    fn canonical_root(&self) -> StateHash { self.prefix.canonical_root.clone() }

    fn canonical_at(&self, index: usize) -> Option<BlockHash> {
        self.prefix.positions.get(index).cloned()
    }

    fn executed(&self, block: &BlockHash) -> Option<StateHash> {
        self.prefix.roots.get(block).cloned()
    }

    fn record(&self, block: &BlockHash) -> Option<R> {
        let bytes = self.db.get_one(&keyed(RECORD, block)).ok()??;
        R::decode(&bytes).ok()
    }

    fn fix(&mut self, target: BlockHash, record: R) -> Result<(), KvStoreError> {
        let index = self.prefix.positions.len();
        let root = record.post_root().clone();
        self.prefix.push(target.clone(), root.clone());
        let written = self.db.put(vec![
            (position_key(index), target.to_vec()),
            (keyed(ROOT, &target), root.to_vec()),
            (keyed(RECORD, &target), record.encode().to_vec()),
            (META_KEY.to_vec(), self.meta()),
        ]);
        if written.is_err() {
            self.prefix.positions.pop();
            self.prefix.roots.remove(&target);
            self.prefix.canonical_root = self
                .prefix
                .positions
                .last()
                .and_then(|last| self.prefix.roots.get(last).cloned())
                .unwrap_or_else(|| self.prefix.anchor_root.clone());
        }
        written
    }

    fn reset(&mut self) -> Result<(), KvStoreError> {
        let keys: Vec<Vec<u8>> = self.db.to_map()?.into_keys().collect();
        if !keys.is_empty() {
            self.db.delete(keys)?;
        }
        self.prefix = Prefix::new(self.prefix.anchor.clone(), self.prefix.anchor_root.clone());
        self.write_fresh()
    }
}

#[cfg(test)]
mod tests {
    use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

    use super::*;
    use crate::rust::v6::block_record::BlockRecord;

    fn h(s: &str) -> Bytes { Bytes::copy_from_slice(s.as_bytes()) }

    fn rec(target: &str, pre: &str, post: &str) -> BlockRecord {
        BlockRecord::empty(h(target), h(pre), h(post))
    }

    fn fix_three(store: &mut dyn RecordStore<BlockRecord>) {
        for (target, pre, post) in [("b1", "r0", "r1"), ("b2", "r1", "r2"), ("b3", "r2", "r3")] {
            store.fix(h(target), rec(target, pre, post)).unwrap();
        }
    }

    #[test]
    fn the_memory_store_keeps_every_root_and_the_last_records_within_the_bound() {
        let mut store: MemoryRecordStore<BlockRecord> =
            MemoryRecordStore::new(h("g"), h("r0"), Retention::LastBlocks(2));
        assert_eq!(store.canonical_len(), 1);
        assert_eq!(store.executed(&h("g")), Some(h("r0")));
        fix_three(&mut store);
        assert_eq!(store.canonical_len(), 4);
        assert_eq!(store.canonical_root(), h("r3"));
        assert_eq!(store.canonical_at(2), Some(h("b2")));
        assert_eq!(store.executed(&h("b1")), Some(h("r1")));
        assert_eq!(store.record(&h("b1")), None);
        assert_eq!(store.record(&h("b2")), Some(rec("b2", "r1", "r2")));
        store.reset().unwrap();
        assert_eq!(store.canonical_len(), 1);
        assert_eq!(store.executed(&h("b1")), None);
        assert_eq!(store.record(&h("b3")), None);
    }

    #[test]
    fn a_zero_bound_keeps_roots_but_no_records() {
        let mut store: MemoryRecordStore<BlockRecord> =
            MemoryRecordStore::new(h("g"), h("r0"), Retention::LastBlocks(0));
        store.fix(h("b1"), rec("b1", "r0", "r1")).unwrap();
        assert_eq!(store.executed(&h("b1")), Some(h("r1")));
        assert_eq!(store.record(&h("b1")), None);
        assert_eq!(Retention::default(), Retention::LastBlocks(1024));
    }

    #[test]
    fn the_database_store_resumes_its_prefix_after_reopening() {
        let db: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let (mut store, opened) =
            LmdbRecordStore::<BlockRecord>::open(Arc::clone(&db), h("g"), h("r0")).unwrap();
        assert!(matches!(opened, Opened::Fresh));
        fix_three(&mut store);
        drop(store);

        let (store, opened) =
            LmdbRecordStore::<BlockRecord>::open(Arc::clone(&db), h("g"), h("r0")).unwrap();
        assert!(matches!(opened, Opened::Resumed));
        assert_eq!(store.canonical_len(), 4);
        assert_eq!(store.canonical_root(), h("r3"));
        assert_eq!(
            (0..4)
                .map(|i| store.canonical_at(i).unwrap())
                .collect::<Vec<_>>(),
            vec![h("g"), h("b1"), h("b2"), h("b3")]
        );
        assert_eq!(store.executed(&h("b2")), Some(h("r2")));
        assert_eq!(store.record(&h("b1")), Some(rec("b1", "r0", "r1")));
        assert_eq!(store.record(&h("b3")), Some(rec("b3", "r2", "r3")));
    }

    #[test]
    fn another_anchor_or_a_reset_starts_the_database_store_afresh() {
        let db: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let (mut store, _) =
            LmdbRecordStore::<BlockRecord>::open(Arc::clone(&db), h("g"), h("r0")).unwrap();
        fix_three(&mut store);
        drop(store);

        let (store, opened) =
            LmdbRecordStore::<BlockRecord>::open(Arc::clone(&db), h("anchor"), h("ra")).unwrap();
        assert!(matches!(opened, Opened::ReplacedOtherAnchor));
        assert_eq!(store.canonical_len(), 1);
        assert_eq!(store.canonical_root(), h("ra"));
        assert_eq!(store.record(&h("b1")), None);
        drop(store);

        let (mut store, opened) =
            LmdbRecordStore::<BlockRecord>::open(Arc::clone(&db), h("anchor"), h("ra")).unwrap();
        assert!(matches!(opened, Opened::Resumed));
        store.fix(h("b9"), rec("b9", "ra", "r9")).unwrap();
        store.reset().unwrap();
        assert_eq!(store.canonical_len(), 1);
        assert_eq!(db.to_map().unwrap().len(), 2);
    }
}
