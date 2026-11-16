use std::collections::BTreeMap;
use std::fmt::Debug;

use crate::rust::ByteBuffer;

/// A bounded page of key-value pairs returned by [`KeyValueStore::scan_page`].
///
/// `rows` are yielded in ascending lexicographic key order (over raw key
/// bytes, independent of any encoding the implementation uses internally).
/// `next` is `Some(cursor)` when a next page MAY contain more entries —
/// specifically, when the page was filled to the caller's `limit`. Pass the
/// cursor back as the `after` argument to continue. A `None` `next` is
/// definitive: the current page reached the end of the store.
///
/// The cursor is the raw key bytes of the page's last row. Store contents
/// may change between pages (concurrent writers permitted), and the scan is
/// NOT a consistent snapshot. Callers that need snapshot semantics open
/// their own LMDB read transaction and drive iteration themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorePage {
    pub rows: Vec<(ByteBuffer, ByteBuffer)>,
    pub next: Option<ByteBuffer>,
}

/// Maximum entries a single `scan_page` call may return. Matches the limit
/// `atomic_store::AtomicStoreGroup::scan` already enforces, and the
/// block-store GC's `GC_PAGE_SIZE` convention.
pub const SCAN_PAGE_LIMIT_MAX: usize = 1024;

/// Reject out-of-range `limit` arguments. `1..=1024` is the allowed window:
/// zero is a caller error (page with nothing in it); above 1024 risks long
/// write-blocking read transactions on large envs.
pub fn check_scan_limit(limit: usize) -> Result<(), KvStoreError> {
    if limit == 0 || limit > SCAN_PAGE_LIMIT_MAX {
        return Err(KvStoreError::InvalidArgument(format!(
            "scan_page limit must be in 1..={} (got {})",
            SCAN_PAGE_LIMIT_MAX, limit
        )));
    }
    Ok(())
}

/// Wrap a raw key in the bincode `Vec<u8>` form LMDB stores it in: an 8-byte
/// little-endian length prefix followed by the raw bytes. Allocates per call.
pub fn encode_bincode_key(key: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + key.len());
    out.extend_from_slice(&(key.len() as u64).to_le_bytes());
    out.extend_from_slice(key);
    out
}

// See shared/src/main/scala/coop/rchain/store/KeyValueStore.scala
pub trait KeyValueStore: Send + Sync + 'static {
    /// Enables downcasting to a concrete store type (e.g. `LmdbKeyValueStore`)
    /// so callers holding a store behind `Arc<dyn KeyValueStore>` can detect
    /// and batch same-environment LMDB writes — see
    /// `lmdb_key_value_store::batched_put`. Has no default body (trait
    /// objects can't provide one); every implementor must define it as
    /// `fn as_any(&self) -> &dyn std::any::Any { self }`.
    fn as_any(&self) -> &dyn std::any::Any;

    fn get(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<Option<ByteBuffer>>, KvStoreError>;

    fn put(&self, kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError>;

    /// Atomically insert one key/value pair, returning false when the key already exists.
    fn put_one_if_absent(&self, key: ByteBuffer, value: ByteBuffer) -> Result<bool, KvStoreError>;

    fn delete(&self, keys: Vec<ByteBuffer>) -> Result<usize, KvStoreError>;

    fn delete_if_value(
        &self,
        key: &ByteBuffer,
        expected: &ByteBuffer,
    ) -> Result<bool, KvStoreError>;

    fn iterate(&self, f: fn(ByteBuffer, ByteBuffer)) -> Result<(), KvStoreError>;
    fn iterate_while(
        &self,
        f: &mut dyn FnMut(ByteBuffer, ByteBuffer) -> Result<bool, KvStoreError>,
    ) -> Result<(), KvStoreError>;

    /// Return a bounded page of key-value pairs in ascending lexicographic
    /// key order. `after` is the cursor returned by a previous page (exclusive
    /// start) or `None` to start at the first key. `limit` must be in `1..=1024`.
    ///
    /// Returns [`StorePage`]: `rows` has at most `limit` entries, and `next`
    /// is `Some(cursor)` exactly when the page was filled (there may be more).
    /// The scan is NOT a consistent snapshot across pages — concurrent writers
    /// may have landed between calls. Callers requiring snapshot semantics
    /// should open their own read transaction and iterate themselves.
    ///
    /// Introduced for rspace-history pruning (issue #923, Option D): the
    /// pruner walks every node hash in the `rspace/history` LMDB env in
    /// bounded pages, deleting those not reached by a prior mark walk.
    ///
    /// The default implementation walks `iterate_while` and is O(cursor
    /// position) per page. Production impls (`LmdbKeyValueStore`,
    /// `MemoryKeyValueStore`) override it with a true cursor seek — O(log n)
    /// to position, O(limit) to fill. Test doubles can use the default.
    fn scan_page(&self, after: Option<&[u8]>, limit: usize) -> Result<StorePage, KvStoreError> {
        check_scan_limit(limit)?;
        let mut rows: Vec<(ByteBuffer, ByteBuffer)> = Vec::with_capacity(limit);
        let cursor_owned = after.map(|c| c.to_vec());
        let mut past_cursor = cursor_owned.is_none();
        self.iterate_while(&mut |key, value| {
            if !past_cursor {
                if let Some(cursor) = cursor_owned.as_ref() {
                    if key.as_slice() > cursor.as_slice() {
                        past_cursor = true;
                    }
                }
            }
            if past_cursor && rows.len() < limit {
                rows.push((key, value));
            }
            Ok(rows.len() < limit)
        })?;
        let next =
            (rows.len() == limit).then(|| rows.last().expect("positive page limit").0.clone());
        Ok(StorePage { rows, next })
    }

    fn clone_box(&self) -> Box<dyn KeyValueStore>;

    fn to_map(&self) -> Result<BTreeMap<ByteBuffer, ByteBuffer>, KvStoreError>;

    fn print_store(&self) -> Result<(), KvStoreError>;

    /// Check if the store contains any entries. O(1) time and space.
    fn non_empty(&self) -> Result<bool, KvStoreError>;

    fn contains(&self, keys: &Vec<ByteBuffer>) -> Result<Vec<bool>, KvStoreError> {
        let results = self.get(keys)?;

        Ok(results.into_iter().map(|result| result.is_some()).collect())
    }

    // See shared/src/main/scala/coop/rchain/store/KeyValueStoreSyntax.scala

    fn get_one(&self, key: &ByteBuffer) -> Result<Option<ByteBuffer>, KvStoreError> {
        let values = self.get(&vec![key.to_vec()])?;

        match values.split_first() {
            Some((first_value, _)) => Ok(first_value.clone()),
            None => Ok(None),
        }
    }

    fn put_one(&self, key: ByteBuffer, value: ByteBuffer) -> Result<(), KvStoreError> {
        self.put(vec![(key, value)])
    }

    fn put_if_absent(&self, kv_pairs: Vec<(ByteBuffer, ByteBuffer)>) -> Result<(), KvStoreError> {
        let keys: Vec<ByteBuffer> = kv_pairs.iter().map(|(k, _)| k.clone()).collect();
        let if_absent = self.contains(&keys)?;
        let kv_if_absent: Vec<_> = kv_pairs.into_iter().zip(if_absent).collect();
        let kv_absent: Vec<_> = kv_if_absent
            .clone()
            .into_iter()
            .filter(|(_, is_present)| !is_present)
            .map(|(kv, _)| kv)
            .collect();

        self.put(kv_absent)
    }

    fn size_bytes(&self) -> usize;
}

impl Clone for Box<dyn KeyValueStore> {
    fn clone(&self) -> Box<dyn KeyValueStore> { self.clone_box() }
}

/// Which accessor asked for a block that is not held, plus — when captured at
/// the storage boundary — the caller backtrace. `Display` owns the separator,
/// so accessors are bare names. Equality compares the accessor only.
#[derive(Debug, Clone)]
pub struct MissingBlockContext {
    accessor: &'static str,
    backtrace: Option<std::sync::Arc<std::backtrace::Backtrace>>,
}

impl MissingBlockContext {
    pub fn new(accessor: &'static str) -> Self {
        Self {
            accessor,
            backtrace: None,
        }
    }

    /// `force_capture` so the trace does not depend on RUST_BACKTRACE being
    /// set in the shard's environment; error path only.
    pub fn with_backtrace(accessor: &'static str) -> Self {
        Self {
            accessor,
            backtrace: Some(std::sync::Arc::new(
                std::backtrace::Backtrace::force_capture(),
            )),
        }
    }

    pub fn accessor(&self) -> &'static str { self.accessor }
}

impl PartialEq for MissingBlockContext {
    fn eq(&self, other: &Self) -> bool { self.accessor == other.accessor }
}

impl std::fmt::Display for MissingBlockContext {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if !self.accessor.is_empty() {
            write!(f, " [{}]", self.accessor)?;
        }
        if let Some(backtrace) = &self.backtrace {
            write!(f, "\n  caller backtrace:\n{}", backtrace)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum KvStoreError {
    KeyNotFound(String),
    IoError(String),
    SerializationError(String),
    InvalidArgument(String),
    LockError(String),
    /// Returned when a DAG representation is requested before the
    /// approved-block / last-finalized-block bootstrap has completed.
    LastFinalizedBlockUninitialized,
    /// A block the DAG index does not hold, carried as bytes so the caller can
    /// request it. Distinct from [`KvStoreError::KeyNotFound`], which means a
    /// store lost a value its index still points at: this one means the index
    /// never had the block, the normal condition of a node restored from a sync
    /// anchor. Callers that judge blocks must be able to tell the two apart.
    MissingBlock {
        hash: prost::bytes::Bytes,
        context: MissingBlockContext,
    },
}

impl std::fmt::Display for KvStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            KvStoreError::KeyNotFound(key) => write!(f, "Key not found: {}", key),
            KvStoreError::IoError(e) => write!(f, "I/O error: {}", e),
            KvStoreError::SerializationError(e) => write!(f, "SerializationError error: {}", e),
            KvStoreError::InvalidArgument(e) => write!(f, "Invalid argument: {}", e),
            KvStoreError::LockError(e) => write!(f, "Lock error: {}", e),
            KvStoreError::LastFinalizedBlockUninitialized => write!(
                f,
                "DagState does not contain lastFinalizedBlock (bootstrap incomplete)"
            ),
            KvStoreError::MissingBlock { hash, context } => {
                write!(
                    f,
                    "DAG storage is missing hash {}{}",
                    hex::encode(hash),
                    context
                )
            }
        }
    }
}

impl From<heed::Error> for KvStoreError {
    fn from(error: heed::Error) -> Self { KvStoreError::IoError(error.to_string()) }
}

impl From<Box<bincode::ErrorKind>> for KvStoreError {
    fn from(error: Box<bincode::ErrorKind>) -> Self {
        KvStoreError::SerializationError(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_formats_each_variant() {
        assert_eq!(
            KvStoreError::KeyNotFound("k1".to_string()).to_string(),
            "Key not found: k1"
        );
        assert_eq!(
            KvStoreError::IoError("disk gone".to_string()).to_string(),
            "I/O error: disk gone"
        );
        assert_eq!(
            KvStoreError::SerializationError("bad bytes".to_string()).to_string(),
            "SerializationError error: bad bytes"
        );
        assert_eq!(
            KvStoreError::InvalidArgument("nope".to_string()).to_string(),
            "Invalid argument: nope"
        );
        assert_eq!(
            KvStoreError::LockError("poisoned".to_string()).to_string(),
            "Lock error: poisoned"
        );
        assert_eq!(
            KvStoreError::LastFinalizedBlockUninitialized.to_string(),
            "DagState does not contain lastFinalizedBlock (bootstrap incomplete)"
        );
        assert_eq!(
            KvStoreError::MissingBlock {
                hash: prost::bytes::Bytes::from_static(&[0xab, 0xcd]),
                context: MissingBlockContext::new("while merging"),
            }
            .to_string(),
            "DAG storage is missing hash abcd [while merging]"
        );
    }

    #[test]
    fn bincode_errors_convert_to_serialization_errors() {
        let bincode_err = bincode::deserialize::<String>(&[0xff]).unwrap_err();
        let converted: KvStoreError = bincode_err.into();
        assert!(matches!(converted, KvStoreError::SerializationError(_)));
    }

    #[test]
    fn heed_errors_convert_to_io_errors() {
        let heed_err = heed::Error::Io(std::io::Error::other("mmap failure"));
        let converted: KvStoreError = heed_err.into();
        assert_eq!(converted, KvStoreError::IoError("mmap failure".to_string()));
    }
}
