use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use block_storage::rust::key_value_block_store::KeyValueBlockStore;
use models::rust::block_hash::BlockHash;
use prost::bytes::Bytes;
use prost::Message;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError};

use super::body::{StateHash, V6Records};
use super::record::{ExecutionRecord, FixMarks, PriorEntry, PriorStatus, VoidReason};

pub const DEFAULT_RETAINED_RECORDS: usize = crate::rust::casper::V6_RETAINED_RECORDS_DEFAULT;

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

    fn fix(&mut self, target: BlockHash, citer: BlockHash, record: R) -> Result<(), KvStoreError>;

    fn fix_marked(
        &mut self,
        target: BlockHash,
        citer: BlockHash,
        record: R,
        _marks: &FixMarks,
    ) -> Result<(), KvStoreError> {
        self.fix(target, citer, record)
    }

    fn void_count(&self, _proposer: &Bytes, _reason: VoidReason) -> u64 { 0 }

    fn prior(&self, _envelope: &[u8]) -> Option<PriorEntry> { None }

    fn position_of(&self, block: &BlockHash) -> Option<usize>;

    fn reset(&mut self) -> Result<(), KvStoreError>;

    fn resident(&self) -> usize;
}

pub trait RecordSource<R: ExecutionRecord>: Send + Sync {
    fn record_in(&self, citer: &BlockHash, target: &BlockHash, root: &StateHash) -> Option<R>;
}

impl<R: ExecutionRecord> RecordSource<R> for KeyValueBlockStore {
    fn record_in(&self, citer: &BlockHash, target: &BlockHash, root: &StateHash) -> Option<R> {
        let block = self.get(citer).ok()??;
        block
            .body
            .v6
            .as_ref()?
            .decode_records::<R>()
            .ok()?
            .into_iter()
            .find(|record| record.target() == target && record.post_root() == root)
    }
}

struct Prefix {
    anchor: BlockHash,
    anchor_root: StateHash,
    positions: Vec<BlockHash>,
    roots: HashMap<BlockHash, StateHash>,
    index: HashMap<BlockHash, usize>,
    canonical_root: StateHash,
}

impl Prefix {
    fn new(anchor: BlockHash, anchor_root: StateHash) -> Self {
        Prefix {
            positions: vec![anchor.clone()],
            roots: HashMap::from([(anchor.clone(), anchor_root.clone())]),
            index: HashMap::from([(anchor.clone(), 0)]),
            canonical_root: anchor_root.clone(),
            anchor,
            anchor_root,
        }
    }

    fn push(&mut self, target: BlockHash, root: StateHash) {
        self.index.insert(target.clone(), self.positions.len());
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
    voids: HashMap<(Bytes, VoidReason), u64>,
    priors: HashMap<Bytes, PriorEntry>,
}

impl<R: ExecutionRecord> MemoryRecordStore<R> {
    pub fn new(genesis: BlockHash, genesis_root: StateHash, retention: Retention) -> Self {
        MemoryRecordStore {
            retention,
            prefix: Prefix::new(genesis, genesis_root),
            retained: HashMap::new(),
            retained_order: VecDeque::new(),
            voids: HashMap::new(),
            priors: HashMap::new(),
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

    fn fix(&mut self, target: BlockHash, _citer: BlockHash, record: R) -> Result<(), KvStoreError> {
        self.prefix.push(target.clone(), record.post_root().clone());
        self.retain(target, record);
        Ok(())
    }

    fn fix_marked(
        &mut self,
        target: BlockHash,
        citer: BlockHash,
        record: R,
        marks: &FixMarks,
    ) -> Result<(), KvStoreError> {
        self.fix(target, citer, record)?;
        if let Some((proposer, reason)) = &marks.void {
            *self.voids.entry((proposer.clone(), *reason)).or_insert(0) += 1;
        }
        for (envelope, entry) in &marks.statuses {
            self.priors.insert(envelope.clone(), *entry);
        }
        Ok(())
    }

    fn void_count(&self, proposer: &Bytes, reason: VoidReason) -> u64 {
        self.voids.get(&(proposer.clone(), reason)).copied().unwrap_or(0)
    }

    fn prior(&self, envelope: &[u8]) -> Option<PriorEntry> { self.priors.get(envelope).copied() }

    fn position_of(&self, block: &BlockHash) -> Option<usize> { self.prefix.index.get(block).copied() }

    fn reset(&mut self) -> Result<(), KvStoreError> {
        self.prefix = Prefix::new(self.prefix.anchor.clone(), self.prefix.anchor_root.clone());
        self.retained.clear();
        self.retained_order.clear();
        self.voids.clear();
        self.priors.clear();
        Ok(())
    }

    fn resident(&self) -> usize {
        self.prefix.positions.len() + self.prefix.roots.len() + self.retained.len()
    }
}

pub const EXECUTOR_STATE_VERSION: u32 = 2;

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
    #[prost(uint32, tag = "5")]
    version: u32,
    #[prost(bytes = "bytes", tag = "6")]
    last: Bytes,
}

const META_KEY: &[u8] = b"meta";
const POSITION: u8 = b'p';
const ROOT: u8 = b'r';
const CITER: u8 = b'k';
const VOIDS: u8 = b'v';
const PRIOR: u8 = b'e';
const INDEX: u8 = b'i';

fn encode_prior(entry: &PriorEntry) -> Vec<u8> {
    let mut bytes = entry.block_number.to_be_bytes().to_vec();
    bytes.push(match entry.status {
        PriorStatus::Included => 1,
        PriorStatus::Discarded => 2,
    });
    bytes
}

fn decode_prior(bytes: &[u8]) -> Option<PriorEntry> {
    let (number, status) = bytes.split_at_checked(8)?;
    let status = match status {
        [1] => PriorStatus::Included,
        [2] => PriorStatus::Discarded,
        _ => return None,
    };
    Some(PriorEntry {
        block_number: i64::from_be_bytes(number.try_into().ok()?),
        status,
    })
}

fn void_key(proposer: &[u8], reason: VoidReason) -> Vec<u8> {
    let mut body = Vec::with_capacity(1 + proposer.len());
    body.push(reason.tag() as u8);
    body.extend_from_slice(proposer);
    keyed(VOIDS, &body)
}

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

struct Head {
    anchor: BlockHash,
    anchor_root: StateHash,
    len: usize,
    last: BlockHash,
    canonical_root: StateHash,
}

impl Head {
    fn fresh(anchor: BlockHash, anchor_root: StateHash) -> Self {
        Head {
            len: 1,
            last: anchor.clone(),
            canonical_root: anchor_root.clone(),
            anchor,
            anchor_root,
        }
    }

    fn meta(&self) -> Vec<u8> {
        ExecutorMetaProto {
            anchor: self.anchor.clone(),
            anchor_root: self.anchor_root.clone(),
            canonical_len: self.len as u64,
            canonical_root: self.canonical_root.clone(),
            version: EXECUTOR_STATE_VERSION,
            last: self.last.clone(),
        }
        .encode_to_vec()
    }
}

pub struct LmdbRecordStore<R: ExecutionRecord> {
    db: Arc<dyn KeyValueStore>,
    source: Arc<dyn RecordSource<R>>,
    head: Head,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opened {
    Resumed,
    Fresh,
    ReplacedOtherAnchor,
    ReplacedOlderFormat,
}

impl<R: ExecutionRecord> LmdbRecordStore<R> {
    pub fn open(
        db: Arc<dyn KeyValueStore>,
        source: Arc<dyn RecordSource<R>>,
        anchor: BlockHash,
        anchor_root: StateHash,
    ) -> Result<(Self, Opened), KvStoreError> {
        let mut store = LmdbRecordStore {
            db,
            source,
            head: Head::fresh(anchor.clone(), anchor_root.clone()),
        };
        let Some(bytes) = store.db.get_one(&META_KEY.to_vec())? else {
            store.write_fresh()?;
            return Ok((store, Opened::Fresh));
        };
        let meta = ExecutorMetaProto::decode(bytes.as_slice())
            .map_err(|error| corrupt(&error.to_string()))?;
        if meta.version != EXECUTOR_STATE_VERSION {
            store.reset()?;
            return Ok((store, Opened::ReplacedOlderFormat));
        }
        if meta.anchor != anchor || meta.anchor_root != anchor_root {
            store.reset()?;
            return Ok((store, Opened::ReplacedOtherAnchor));
        }
        let len = usize::try_from(meta.canonical_len).map_err(|_| corrupt("length"))?;
        if len == 0 {
            return Err(corrupt("an empty prefix"));
        }
        store.head = Head {
            anchor,
            anchor_root,
            len,
            last: meta.last,
            canonical_root: meta.canonical_root,
        };
        if store.canonical_at(len - 1).as_ref() != Some(&store.head.last) {
            return Err(corrupt("the last position disagrees with the metadata"));
        }
        if RecordStore::<R>::executed(&store, &store.head.last).as_ref()
            != Some(&store.head.canonical_root)
        {
            return Err(corrupt("the canonical root disagrees with the last block"));
        }
        Ok((store, Opened::Resumed))
    }

    fn write_fresh(&self) -> Result<(), KvStoreError> {
        self.db.put(vec![
            (position_key(0), self.head.anchor.to_vec()),
            (
                keyed(ROOT, &self.head.anchor),
                self.head.anchor_root.to_vec(),
            ),
            (META_KEY.to_vec(), self.head.meta()),
        ])
    }
}

impl<R: ExecutionRecord> RecordStore<R> for LmdbRecordStore<R> {
    fn canonical_len(&self) -> usize { self.head.len }

    fn canonical_root(&self) -> StateHash { self.head.canonical_root.clone() }

    fn canonical_at(&self, index: usize) -> Option<BlockHash> {
        if index >= self.head.len {
            return None;
        }
        if index + 1 == self.head.len {
            return Some(self.head.last.clone());
        }
        self.db.get_one(&position_key(index)).ok()?.map(Bytes::from)
    }

    fn executed(&self, block: &BlockHash) -> Option<StateHash> {
        if *block == self.head.last {
            return Some(self.head.canonical_root.clone());
        }
        self.db.get_one(&keyed(ROOT, block)).ok()?.map(Bytes::from)
    }

    fn record(&self, block: &BlockHash) -> Option<R> {
        let root = self.executed(block)?;
        let citer = Bytes::from(self.db.get_one(&keyed(CITER, block)).ok()??);
        self.source.record_in(&citer, block, &root)
    }

    fn fix(&mut self, target: BlockHash, citer: BlockHash, record: R) -> Result<(), KvStoreError> {
        self.fix_rows(target, citer, record, Vec::new())
    }

    fn fix_marked(
        &mut self,
        target: BlockHash,
        citer: BlockHash,
        record: R,
        marks: &FixMarks,
    ) -> Result<(), KvStoreError> {
        let mut rows = Vec::new();
        if let Some((proposer, reason)) = &marks.void {
            let count = RecordStore::<R>::void_count(self, proposer, *reason) + 1;
            rows.push((void_key(proposer, *reason), count.to_be_bytes().to_vec()));
        }
        for (envelope, entry) in &marks.statuses {
            rows.push((keyed(PRIOR, envelope), encode_prior(entry)));
        }
        self.fix_rows(target, citer, record, rows)
    }

    fn position_of(&self, block: &BlockHash) -> Option<usize> {
        if *block == self.head.anchor {
            return Some(0);
        }
        if *block == self.head.last {
            return Some(self.head.len - 1);
        }
        self.db
            .get_one(&keyed(INDEX, block))
            .ok()
            .flatten()
            .and_then(|bytes| <[u8; 8]>::try_from(bytes.as_slice()).ok())
            .map(|bytes| u64::from_be_bytes(bytes) as usize)
    }

    fn prior(&self, envelope: &[u8]) -> Option<PriorEntry> {
        self.db
            .get_one(&keyed(PRIOR, envelope))
            .ok()
            .flatten()
            .and_then(|bytes| decode_prior(&bytes))
    }

    fn void_count(&self, proposer: &Bytes, reason: VoidReason) -> u64 {
        self.db
            .get_one(&void_key(proposer, reason))
            .ok()
            .flatten()
            .and_then(|bytes| <[u8; 8]>::try_from(bytes.as_slice()).ok())
            .map_or(0, u64::from_be_bytes)
    }

    fn reset(&mut self) -> Result<(), KvStoreError> {
        let keys: Vec<Vec<u8>> = self.db.to_map()?.into_keys().collect();
        if !keys.is_empty() {
            self.db.delete(keys)?;
        }
        self.head = Head::fresh(self.head.anchor.clone(), self.head.anchor_root.clone());
        self.write_fresh()
    }

    fn resident(&self) -> usize { 1 }
}

impl<R: ExecutionRecord> LmdbRecordStore<R> {
    fn fix_rows(
        &mut self,
        target: BlockHash,
        citer: BlockHash,
        record: R,
        extra: Vec<(Vec<u8>, Vec<u8>)>,
    ) -> Result<(), KvStoreError> {
        let root = record.post_root().clone();
        let next = Head {
            anchor: self.head.anchor.clone(),
            anchor_root: self.head.anchor_root.clone(),
            len: self.head.len + 1,
            last: target.clone(),
            canonical_root: root.clone(),
        };
        let mut rows = vec![
            (position_key(self.head.len), target.to_vec()),
            (keyed(ROOT, &target), root.to_vec()),
            (keyed(CITER, &target), citer.to_vec()),
            (keyed(INDEX, &target), (self.head.len as u64).to_be_bytes().to_vec()),
            (META_KEY.to_vec(), next.meta()),
        ];
        rows.extend(extra);
        self.db.put(rows)?;
        self.head = next;
        Ok(())
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

    #[derive(Default)]
    struct Citers(parking_lot::Mutex<HashMap<BlockHash, Vec<BlockRecord>>>);

    impl Citers {
        fn cite(&self, citer: &str, record: BlockRecord) {
            self.0.lock().entry(h(citer)).or_default().push(record);
        }
    }

    impl RecordSource<BlockRecord> for Citers {
        fn record_in(
            &self,
            citer: &BlockHash,
            target: &BlockHash,
            root: &StateHash,
        ) -> Option<BlockRecord> {
            self.0
                .lock()
                .get(citer)?
                .iter()
                .find(|r| r.target() == target && r.post_root() == root)
                .cloned()
        }
    }

    const THREE: [(&str, &str, &str, &str); 3] = [
        ("b1", "c1", "r0", "r1"),
        ("b2", "c2", "r1", "r2"),
        ("b3", "c3", "r2", "r3"),
    ];

    fn fix_three(store: &mut dyn RecordStore<BlockRecord>, citers: &Citers) {
        for (target, citer, pre, post) in THREE {
            citers.cite(citer, rec(target, pre, post));
            store
                .fix(h(target), h(citer), rec(target, pre, post))
                .unwrap();
        }
    }

    fn open(
        db: &Arc<dyn KeyValueStore>,
        citers: &Arc<Citers>,
        anchor: &str,
        root: &str,
    ) -> (LmdbRecordStore<BlockRecord>, Opened) {
        let source: Arc<dyn RecordSource<BlockRecord>> = citers.clone();
        LmdbRecordStore::open(Arc::clone(db), source, h(anchor), h(root)).unwrap()
    }

    #[test]
    fn the_memory_store_keeps_every_root_and_the_last_records_within_the_bound() {
        let mut store: MemoryRecordStore<BlockRecord> =
            MemoryRecordStore::new(h("g"), h("r0"), Retention::LastBlocks(2));
        assert_eq!(store.canonical_len(), 1);
        assert_eq!(store.executed(&h("g")), Some(h("r0")));
        fix_three(&mut store, &Citers::default());
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
        store.fix(h("b1"), h("c1"), rec("b1", "r0", "r1")).unwrap();
        assert_eq!(store.executed(&h("b1")), Some(h("r1")));
        assert_eq!(store.record(&h("b1")), None);
        assert_eq!(Retention::default(), Retention::LastBlocks(1024));
    }

    #[test]
    fn the_database_store_resumes_its_prefix_after_reopening() {
        let db: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let citers = Arc::new(Citers::default());
        let (mut store, opened) = open(&db, &citers, "g", "r0");
        assert_eq!(opened, Opened::Fresh);
        fix_three(&mut store, &citers);
        drop(store);

        let (store, opened) = open(&db, &citers, "g", "r0");
        assert_eq!(opened, Opened::Resumed);
        assert_eq!(store.canonical_len(), 4);
        assert_eq!(store.canonical_root(), h("r3"));
        assert_eq!(
            (0..4)
                .map(|i| store.canonical_at(i).unwrap())
                .collect::<Vec<_>>(),
            vec![h("g"), h("b1"), h("b2"), h("b3")]
        );
        assert_eq!(store.canonical_at(4), None);
        assert_eq!(store.executed(&h("g")), Some(h("r0")));
        assert_eq!(store.executed(&h("b2")), Some(h("r2")));
        assert_eq!(store.executed(&h("x")), None);
    }

    #[test]
    fn records_are_read_back_through_their_citing_block() {
        let db: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let citers = Arc::new(Citers::default());
        let (mut store, _) = open(&db, &citers, "g", "r0");
        fix_three(&mut store, &citers);
        citers.cite("c1", rec("b1", "r0", "other"));
        assert_eq!(store.record(&h("b1")), Some(rec("b1", "r0", "r1")));
        assert_eq!(store.record(&h("b3")), Some(rec("b3", "r2", "r3")));
        assert_eq!(store.record(&h("g")), None);
        citers.0.lock().remove(&h("c2"));
        assert_eq!(store.record(&h("b2")), None);
        let stored: Vec<Vec<u8>> = db.to_map().unwrap().into_values().collect();
        assert!(!stored
            .iter()
            .any(|value| *value == rec("b1", "r0", "r1").encode().to_vec()));
    }

    #[test]
    fn the_database_store_keeps_nothing_in_memory_that_grows_with_the_prefix() {
        let db: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let citers = Arc::new(Citers::default());
        let (mut store, _) = open(&db, &citers, "g", "r0");
        let before = RecordStore::<BlockRecord>::resident(&store);
        let mut pre = h("r0");
        for i in 0..500 {
            let post = h(&format!("r{}", i + 1));
            let record = BlockRecord::empty(h(&format!("b{i}")), pre.clone(), post.clone());
            store
                .fix(h(&format!("b{i}")), h(&format!("c{i}")), record)
                .unwrap();
            pre = post;
        }
        assert_eq!(store.canonical_len(), 501);
        assert_eq!(RecordStore::<BlockRecord>::resident(&store), before);
        assert_eq!(store.canonical_at(250), Some(h("b249")));
    }

    #[test]
    fn another_anchor_an_older_format_or_a_reset_starts_the_database_store_afresh() {
        let db: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let citers = Arc::new(Citers::default());
        let (mut store, _) = open(&db, &citers, "g", "r0");
        fix_three(&mut store, &citers);
        drop(store);

        let (store, opened) = open(&db, &citers, "anchor", "ra");
        assert_eq!(opened, Opened::ReplacedOtherAnchor);
        assert_eq!(store.canonical_len(), 1);
        assert_eq!(store.canonical_root(), h("ra"));
        assert_eq!(store.executed(&h("b1")), None);
        drop(store);

        let (mut store, opened) = open(&db, &citers, "anchor", "ra");
        assert_eq!(opened, Opened::Resumed);
        store.fix(h("b9"), h("c9"), rec("b9", "ra", "r9")).unwrap();
        store.reset().unwrap();
        assert_eq!(store.canonical_len(), 1);
        assert_eq!(db.to_map().unwrap().len(), 3);
        drop(store);

        let older = ExecutorMetaProto {
            anchor: h("anchor"),
            anchor_root: h("ra"),
            canonical_len: 1,
            canonical_root: h("ra"),
            version: 0,
            last: Bytes::new(),
        };
        db.put(vec![(META_KEY.to_vec(), older.encode_to_vec())])
            .unwrap();
        let (store, opened) = open(&db, &citers, "anchor", "ra");
        assert_eq!(opened, Opened::ReplacedOlderFormat);
        assert_eq!(store.canonical_len(), 1);
    }

    #[test]
    fn void_counts_are_written_with_the_record_and_survive_a_restart() {
        let db: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
        let citers = Arc::new(Citers::default());
        let (mut store, _) = open(&db, &citers, "g", "r0");
        let void = |target: &str| BlockRecord::voided(h(target), h("r0"), &[], VoidReason::CloseUncovered);
        for target in ["b1", "b2"] {
            citers.cite("c", void(target));
            let marks = FixMarks {
                void: Some((h("V1"), VoidReason::CloseUncovered)),
                statuses: vec![(h(&format!("d-{target}")), PriorEntry {
                    block_number: 7,
                    status: PriorStatus::Discarded,
                })],
            };
            store.fix_marked(h(target), h("c"), void(target), &marks).unwrap();
        }
        let (reopened, opened) = open(&db, &citers, "g", "r0");
        assert_eq!(opened, Opened::Resumed);
        assert_eq!(RecordStore::<BlockRecord>::canonical_len(&reopened), 3);
        assert_eq!(RecordStore::<BlockRecord>::void_count(&reopened, &h("V1"), VoidReason::CloseUncovered), 2);
        assert_eq!(RecordStore::<BlockRecord>::void_count(&reopened, &h("V1"), VoidReason::CloseFailed), 0);
        assert_eq!(RecordStore::<BlockRecord>::void_count(&reopened, &h("V2"), VoidReason::CloseUncovered), 0);
        assert_eq!(
            RecordStore::<BlockRecord>::prior(&reopened, b"d-b2"),
            Some(PriorEntry {
                block_number: 7,
                status: PriorStatus::Discarded
            })
        );
        assert_eq!(RecordStore::<BlockRecord>::prior(&reopened, b"d-b3"), None);
    }
}