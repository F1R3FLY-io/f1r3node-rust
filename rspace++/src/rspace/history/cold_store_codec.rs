use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Instant;

use bincode::Options;
use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use serde::{Deserialize, Serialize};
use shared::rust::ByteBuffer;
use shared::rust::store::key_value_store::{KeyValueStore, KvStoreError, StorePage};
use shared::rust::store::lmdb_key_value_store::LmdbKeyValueStore;

const MAGIC: [u8; 8] = *b"F1CLZ4\0\0";
const HEADER_LEN: usize = 60;
pub const MAX_COMPRESSED_RECORD_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_CANONICAL_RECORD_BYTES: usize = 16 * 1024 * 1024;
const MIN_COMPRESS_BYTES: usize = 1024;
const MIN_SAVING_BYTES: usize = 64;
const MAX_CODEC_OPERATIONS: usize = 2;
const MAX_SCRATCH_BYTES: usize = 64 * 1024 * 1024;
const ENCODER_WORKSPACE_BYTES: usize = 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Header {
    magic: [u8; 8],
    version: u8,
    codec: u8,
    flags: u16,
    canonical_len: u64,
    payload_len: u64,
    digest: [u8; 32],
}

#[derive(Deserialize)]
struct LeafBytes<'a> {
    #[serde(borrow)]
    bytes: &'a [u8],
}

#[derive(Deserialize)]
enum CanonicalLeaf<'a> {
    Joins(#[serde(borrow)] LeafBytes<'a>),
    Data(#[serde(borrow)] LeafBytes<'a>),
    Continuations(#[serde(borrow)] LeafBytes<'a>),
}

fn invalid(message: &'static str) -> KvStoreError {
    KvStoreError::SerializationError(format!("Cold-store codec: {message}"))
}

fn limit_exceeded(message: &'static str) -> KvStoreError {
    metrics::counter!("rspace.cold-codec.limit-failures").increment(1);
    invalid(message)
}

fn validate_canonical(key: &[u8], value: &[u8]) -> Result<bool, KvStoreError> {
    let leaf: CanonicalLeaf<'_> = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .reject_trailing_bytes()
        .deserialize(value)
        .map_err(|_| invalid("invalid canonical leaf"))?;
    let continuation = matches!(leaf, CanonicalLeaf::Continuations(_));
    let bytes = match leaf {
        CanonicalLeaf::Joins(leaf) |
        CanonicalLeaf::Data(leaf) |
        CanonicalLeaf::Continuations(leaf) => leaf.bytes,
    };
    let hash: &[u8] = if key.len() == 32 {
        key
    } else {
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .reject_trailing_bytes()
            .deserialize(key)
            .map_err(|_| invalid("invalid leaf key"))?
    };
    let mut hasher = Blake2b::<U32>::new();
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
    if hash != &hasher.finalize()[..] {
        return Err(invalid("leaf hash mismatch"));
    }
    Ok(continuation)
}

#[derive(Default)]
struct BudgetState {
    active: usize,
    bytes: usize,
}

struct CodecBudget {
    state: Mutex<BudgetState>,
    available: Condvar,
    max_active: usize,
    max_bytes: usize,
}

impl CodecBudget {
    fn new(max_active: usize, max_bytes: usize) -> Self {
        Self {
            state: Mutex::new(BudgetState::default()),
            available: Condvar::new(),
            max_active,
            max_bytes,
        }
    }

    fn acquire(&self, bytes: usize, wait: bool) -> Result<Option<CodecPermit<'_>>, KvStoreError> {
        if bytes > self.max_bytes {
            return Err(limit_exceeded("scratch budget exceeded"));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| KvStoreError::LockError("Cold-store codec budget poisoned".into()))?;
        while state.active >= self.max_active || bytes > self.max_bytes - state.bytes {
            if !wait {
                return Ok(None);
            }
            state = self
                .available
                .wait(state)
                .map_err(|_| KvStoreError::LockError("Cold-store codec budget poisoned".into()))?;
        }
        state.active += 1;
        state.bytes += bytes;
        metrics::gauge!("rspace.cold-codec.scratch.bytes").set(state.bytes as f64);
        metrics::gauge!("rspace.cold-codec.active").set(state.active as f64);
        Ok(Some(CodecPermit {
            budget: self,
            bytes,
        }))
    }
}

struct CodecPermit<'a> {
    budget: &'a CodecBudget,
    bytes: usize,
}

impl Drop for CodecPermit<'_> {
    fn drop(&mut self) {
        let mut state = self.budget.state.lock().unwrap_or_else(|e| e.into_inner());
        state.active -= 1;
        state.bytes -= self.bytes;
        metrics::gauge!("rspace.cold-codec.scratch.bytes").set(state.bytes as f64);
        metrics::gauge!("rspace.cold-codec.active").set(state.active as f64);
        self.budget.available.notify_all();
    }
}

#[derive(Clone)]
pub struct ColdStoreCodec {
    inner: Arc<dyn KeyValueStore>,
    compress_writes: bool,
    budget: Arc<CodecBudget>,
}

impl ColdStoreCodec {
    pub fn new(inner: Arc<dyn KeyValueStore>, compress_writes: bool) -> Self {
        static BUDGET: OnceLock<Arc<CodecBudget>> = OnceLock::new();
        if let Some(codec) = inner.as_any().downcast_ref::<Self>() {
            return Self {
                inner: codec.inner.clone(),
                compress_writes,
                budget: codec.budget.clone(),
            };
        }
        Self {
            inner,
            compress_writes,
            budget: BUDGET
                .get_or_init(|| Arc::new(CodecBudget::new(MAX_CODEC_OPERATIONS, MAX_SCRATCH_BYTES)))
                .clone(),
        }
    }

    pub fn ensure(store: Arc<dyn KeyValueStore>) -> Arc<dyn KeyValueStore> {
        if store.as_any().is::<Self>() {
            store
        } else {
            Arc::new(Self::new(store, false))
        }
    }

    pub fn try_size_bytes(&self) -> Result<usize, KvStoreError> {
        if let Some(store) = self.inner.as_any().downcast_ref::<LmdbKeyValueStore>() {
            let transaction = store.env.read_txn()?;
            let stats = store.db.stat(&transaction)?;
            stats
                .branch_pages
                .checked_add(stats.leaf_pages)
                .and_then(|pages| pages.checked_add(stats.overflow_pages))
                .and_then(|pages| pages.checked_mul(stats.page_size as usize))
                .ok_or_else(|| KvStoreError::IoError("Cold-store page size overflow".into()))
        } else {
            Ok(self.inner.size_bytes())
        }
    }

    fn raw_fallback(value: ByteBuffer, reason: &'static str) -> ByteBuffer {
        metrics::counter!("rspace.cold-codec.raw", "reason" => reason).increment(1);
        value
    }

    fn encode(&self, key: &[u8], value: ByteBuffer) -> Result<ByteBuffer, KvStoreError> {
        let start = Instant::now();
        let result = self.encode_inner(key, value);
        metrics::histogram!("rspace.cold-codec.encode.seconds")
            .record(start.elapsed().as_secs_f64());
        if result.is_err() {
            metrics::counter!("rspace.cold-codec.failures", "operation" => "encode").increment(1);
        }
        result
    }

    fn encode_inner(&self, key: &[u8], value: ByteBuffer) -> Result<ByteBuffer, KvStoreError> {
        let continuation = validate_canonical(key, &value)?;
        if !self.compress_writes {
            return Ok(Self::raw_fallback(value, "disabled"));
        }
        if !continuation {
            return Ok(Self::raw_fallback(value, "leaf-kind"));
        }
        if value.len() < MIN_COMPRESS_BYTES {
            return Ok(Self::raw_fallback(value, "small"));
        }
        if value.len() > MAX_CANONICAL_RECORD_BYTES {
            return Ok(Self::raw_fallback(value, "large"));
        }
        let max_output = lz4_flex::block::get_maximum_output_size(value.len());
        let scratch = value.len() + max_output + HEADER_LEN + ENCODER_WORKSPACE_BYTES;
        let Some(_permit) = self.budget.acquire(scratch, false)? else {
            return Ok(Self::raw_fallback(value, "busy"));
        };
        let mut record = Vec::new();
        if record.try_reserve_exact(HEADER_LEN + max_output).is_err() {
            return Ok(Self::raw_fallback(value, "allocation"));
        }
        record.resize(HEADER_LEN + max_output, 0);
        let payload_len = lz4_flex::block::compress_into(&value, &mut record[HEADER_LEN..])
            .map_err(|_| invalid("compression failed"))?;
        let stored_len = HEADER_LEN + payload_len;
        if stored_len > MAX_COMPRESSED_RECORD_BYTES || stored_len + MIN_SAVING_BYTES > value.len() {
            return Ok(Self::raw_fallback(value, "saving"));
        }
        let header = Header {
            magic: MAGIC,
            version: 1,
            codec: 1,
            flags: 0,
            canonical_len: value.len() as u64,
            payload_len: payload_len as u64,
            digest: Blake2b::<U32>::digest(&value).into(),
        };
        bincode::serialize_into(&mut record[..HEADER_LEN], &header)?;
        record.truncate(stored_len);
        metrics::counter!("rspace.cold-codec.encoded.canonical-bytes")
            .increment(value.len() as u64);
        metrics::counter!("rspace.cold-codec.encoded.physical-bytes").increment(stored_len as u64);
        metrics::counter!("rspace.cold-codec.encoded.saved-bytes")
            .increment((value.len() - stored_len) as u64);
        drop(value);
        Ok(record.into_boxed_slice().into_vec())
    }

    fn decode(&self, key: &[u8], value: ByteBuffer) -> Result<ByteBuffer, KvStoreError> {
        let start = Instant::now();
        let result = self.decode_inner(key, value);
        metrics::histogram!("rspace.cold-codec.decode.seconds")
            .record(start.elapsed().as_secs_f64());
        if result.is_err() {
            metrics::counter!("rspace.cold-codec.failures", "operation" => "decode").increment(1);
        }
        result
    }

    fn decode_inner(&self, key: &[u8], value: ByteBuffer) -> Result<ByteBuffer, KvStoreError> {
        if !value.starts_with(&MAGIC) {
            validate_canonical(key, &value)?;
            return Ok(value);
        }
        self.expand(key, &value)
    }

    fn expand(&self, key: &[u8], value: &[u8]) -> Result<ByteBuffer, KvStoreError> {
        if value.len() > MAX_COMPRESSED_RECORD_BYTES {
            return Err(limit_exceeded("physical record length out of bounds"));
        }
        if value.len() < HEADER_LEN {
            return Err(invalid("truncated record header"));
        }
        let header: Header = bincode::deserialize(&value[..HEADER_LEN])
            .map_err(|_| invalid("invalid record header"))?;
        if header.version != 1 || header.codec != 1 || header.flags != 0 {
            return Err(invalid("unsupported format, codec, or flags"));
        }
        if header.canonical_len > MAX_CANONICAL_RECORD_BYTES as u64 {
            return Err(limit_exceeded("canonical length out of bounds"));
        }
        if header.payload_len != (value.len() - HEADER_LEN) as u64 || header.canonical_len < 12 {
            return Err(invalid("invalid payload or canonical length"));
        }
        let length = header.canonical_len as usize;
        let _permit = self.budget.acquire(length + value.len(), true)?;
        let mut canonical = Vec::new();
        canonical
            .try_reserve_exact(length)
            .map_err(|_| invalid("decode allocation failed"))?;
        canonical.resize(length, 0);
        let written = lz4_flex::block::decompress_into(&value[HEADER_LEN..], &mut canonical)
            .map_err(|_| invalid("invalid compressed payload"))?;
        if written != length {
            return Err(invalid("decoded length mismatch"));
        }
        if Blake2b::<U32>::digest(&canonical)[..] != header.digest {
            return Err(invalid("canonical digest mismatch"));
        }
        validate_canonical(key, &canonical)?;
        metrics::counter!("rspace.cold-codec.decoded.bytes").increment(length as u64);
        Ok(canonical)
    }
}

impl KeyValueStore for ColdStoreCodec {
    fn as_any(&self) -> &dyn std::any::Any { self }

    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError> {
        self.inner
            .get(keys)?
            .into_iter()
            .zip(keys)
            .map(|(value, key)| value.map(|value| self.decode(key, value)).transpose())
            .collect()
    }

    fn get_one(&self, key: &ByteBuffer) -> Result<Option<ByteBuffer>, KvStoreError> {
        self.get(&vec![key.clone()])
            .map(|values| values.into_iter().next().flatten())
    }

    fn put(&self, pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        let encoded = pairs
            .into_iter()
            .map(|(key, value)| self.encode(&key, value).map(|value| (key, value)))
            .collect::<Result<Vec<_>, _>>()?;
        self.inner.put(encoded)
    }

    fn put_if_absent(&self, pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        for (key, value) in &pairs {
            validate_canonical(key, value)?;
        }
        let keys = pairs.iter().map(|(key, _)| key.clone()).collect();
        let present = self.contains(&keys)?;
        self.put(
            pairs
                .into_iter()
                .zip(present)
                .filter_map(|(pair, present)| (!present).then_some(pair))
                .collect(),
        )
    }

    fn put_one_if_absent(&self, key: ByteBuffer, value: ByteBuffer) -> Result<bool, KvStoreError> {
        let encoded = self.encode(&key, value)?;
        self.inner.put_one_if_absent(key, encoded)
    }

    fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError> {
        self.inner.delete(keys)
    }

    fn delete_if_value(
        &self,
        key: &ByteBuffer,
        expected: &ByteBuffer,
    ) -> Result<bool, KvStoreError> {
        for _ in 0..8 {
            let Some(physical) = self.inner.get_one(key)? else {
                return Ok(false);
            };
            let matches = if physical.starts_with(&MAGIC) {
                &self.expand(key, &physical)? == expected
            } else {
                validate_canonical(key, &physical)?;
                &physical == expected
            };
            if !matches {
                return Ok(false);
            }
            if self.inner.delete_if_value(key, &physical)? {
                return Ok(true);
            }
        }
        Err(KvStoreError::LockError("Cold-store conditional deletion remained contended".into()))
    }

    fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError> {
        self.iterate_while(&mut |key, value| {
            f(key, value);
            Ok(true)
        })
    }

    fn iterate_while(
        &self,
        f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError> {
        self.inner.iterate_while(&mut |key, value| {
            let canonical = self.decode(&key, value)?;
            f(key, canonical)
        })
    }

    fn scan_page(&self, after: Option<&[u8]>, limit: usize) -> Result<StorePage, KvStoreError> {
        let page = self.inner.scan_page(after, limit)?;
        let rows = page
            .rows
            .into_iter()
            .map(|(key, value)| self.decode(&key, value).map(|value| (key, value)))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(StorePage {
            rows,
            next: page.next,
        })
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore> { Box::new(self.clone()) }

    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError> {
        let mut map = BTreeMap::new();
        self.iterate_while(&mut |key, value| {
            map.insert(key, value);
            Ok(true)
        })?;
        Ok(map)
    }

    fn print_store(&self) -> Result<(), KvStoreError> {
        let mut rows = 0u64;
        let mut bytes = 0u64;
        self.iterate_while(&mut |_, value| {
            rows = rows.saturating_add(1);
            bytes = bytes.saturating_add(value.len() as u64);
            Ok(true)
        })?;
        println!(
            "Cold store: rows={rows}, canonical_bytes={bytes}, physical_bytes={}",
            self.try_size_bytes()?
        );
        Ok(())
    }

    fn non_empty(&self) -> Result<bool, KvStoreError> { self.inner.non_empty() }

    fn size_bytes(&self) -> usize {
        self.try_size_bytes().unwrap_or_else(|_| {
            metrics::counter!("rspace.cold-codec.failures", "operation" => "size").increment(1);
            tracing::warn!("Cold-store physical size is unavailable");
            0
        })
    }
}

#[cfg(test)]
mod tests;
