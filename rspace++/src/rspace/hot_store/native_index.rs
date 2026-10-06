//! D-S1 (D-C2b, DR-96): the digest-keyed ordered index of the native session
//! store.
//!
//! The index has 256 shards. Each shard is an imbl 7.0.2 `OrdMap` from a
//! [`StoreKey`] to a shared entry, and the shard of a key is its first byte.
//! An `OrdMap` is a B+tree with at most 16 entries per leaf and 16 keys per
//! branch (imbl `config.rs`, `ORD_CHUNK_SIZE`). The store never removes a key,
//! so every node is built by inserts: a split leaves at least 8 entries or 8
//! keys in each half, and a root branch has at least 2 children. A tree with
//! at most `n` entries therefore has at most [`ord_levels_bound`]`(n)` levels,
//! and a search makes at most 5 comparisons per level
//! (`NativeDigestIndex.v`).
//!
//! The charges of a search, an insert and a replace depend only on the level
//! bound of the store's key limit. They never read the population of a shard,
//! another key or the sharing state, so the charges of the operations on one
//! key do not depend on how the operations on other keys interleave
//! (`NativeDigestIndex.per_key_schedule_total_invariant`,
//! `NativeDigestIndexCharge.tla`).

use std::mem::size_of;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use shared::rust::clone_backing::BLOCK_SHARED_HEADER_SCANNED;

use super::NUM_SHARDS;
use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::native_source::SourceMeter;
pub use crate::rspace::hashing::native_source::StoreKey;
use crate::rspace::native_backing::arc_allocation_bytes;

/// The most distinct keys that one native session stores. The charges below
/// assume this bound; D-C2e enforces it.
pub const NATIVE_STORE_KEY_BOUND: usize = 1 << 20;

/// The imbl `OrdMap` chunk size: at most 16 entries per leaf and 16 keys per
/// branch.
pub const ORD_CHUNK: usize = 16;

/// The comparisons of one binary search over at most [`ORD_CHUNK`] elements
/// (`NativeDigestIndex.bsearch_le_5`).
pub const ORD_NODE_COMPARISONS: usize = 5;

/// The levels of an ordered map with at most `entries` entries: one, plus
/// one for every `h >= 0` with `10 * 8^h <= entries`
/// (`NativeDigestIndex.levels_bound`).
pub const fn ord_levels_bound(entries: usize) -> usize {
    let mut levels = 1;
    let mut minimum: usize = 10;
    while minimum <= entries {
        levels += 1;
        minimum = match minimum.checked_mul(8) {
            Some(next) => next,
            None => return levels,
        };
    }
    levels
}

/// The level bound of the store: 7 levels for 2^20 keys.
pub const ORD_LEVELS: usize = ord_levels_bound(NATIVE_STORE_KEY_BOUND);

const _: () = assert!(ORD_LEVELS == 7);
const _: () = assert!(NUM_SHARDS == 256);

/// The layout of an imbl leaf of `(StoreKey, Arc<_>)` pairs: a chunk of 16
/// pairs and its two indices.
type OrdLeafLayout = ([usize; 2], [(StoreKey, usize); ORD_CHUNK]);

/// The layout of an imbl branch: a chunk of 16 keys, a chunk of 17 child
/// pointers, and the level of the children.
type OrdBranchLayout =
    ([usize; 2], [StoreKey; ORD_CHUNK], [usize; 2], [usize; ORD_CHUNK + 1], usize);

/// The bytes of one shared imbl node allocation: the larger of a leaf and a
/// branch.
pub fn ord_node_bytes() -> Option<usize> {
    Some(arc_allocation_bytes::<OrdLeafLayout>()?.max(arc_allocation_bytes::<OrdBranchLayout>()?))
}

/// A host-work charge: operations, scanned bytes and backing bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IndexCharge {
    pub operations: usize,
    pub scanned: usize,
    pub backing: usize,
}

impl IndexCharge {
    pub fn plus(self, other: Self) -> Option<Self> {
        Some(Self {
            operations: self.operations.checked_add(other.operations)?,
            scanned: self.scanned.checked_add(other.scanned)?,
            backing: self.backing.checked_add(other.backing)?,
        })
    }

    pub fn reserve(self, meter: &dyn SourceMeter) -> Result<(), RSpaceError> {
        meter.reserve(self.operations, self.scanned, self.backing)
    }
}

/// A search: at most 5 comparisons per level, each reading the sought and
/// the stored digest, and per level the chunk bounds and one child pointer.
pub const fn search_charge() -> IndexCharge {
    let per_level = ORD_NODE_COMPARISONS * 2 * size_of::<StoreKey>() + 3 * size_of::<usize>();
    IndexCharge {
        operations: ORD_NODE_COMPARISONS * ORD_LEVELS,
        scanned: per_level * ORD_LEVELS,
        backing: 0,
    }
}

/// The bytes that copying or releasing one node touches: the node and one
/// shared-pointer header for each of its at most 17 entries or children.
fn node_touch_bytes(node: usize) -> Option<usize> {
    node.checked_add((ORD_CHUNK + 1).checked_mul(BLOCK_SHARED_HEADER_SCANNED)?)
}

/// The path copy of a write: at most one shared node per level, and at most
/// 33 element clones per node.
fn copy_charge(node: usize) -> Option<IndexCharge> {
    Some(IndexCharge {
        operations: (2 * ORD_CHUNK + 1).checked_mul(ORD_LEVELS)?,
        scanned: node_touch_bytes(node)?.checked_mul(ORD_LEVELS)?,
        backing: 0,
    })
}

/// `count` node allocations, each with its release prepaid: a node created
/// by a write is released at most once, by a later write, a restore or the
/// end of the session.
fn nodes_charge(node: usize, count: usize) -> Option<IndexCharge> {
    Some(IndexCharge {
        operations: count.checked_mul(2 * ORD_CHUNK + 2)?,
        scanned: count.checked_mul(node.checked_add(node_touch_bytes(node)?)?)?,
        backing: count.checked_mul(node)?,
    })
}

/// The allocation and the release of one entry `Arc`.
fn entry_charge<T>() -> Option<IndexCharge> {
    let bytes = arc_allocation_bytes::<T>()?;
    Some(IndexCharge {
        operations: 2,
        scanned: bytes,
        backing: bytes,
    })
}

/// A replace of a present key: the search, the path copy, at most one new
/// node per level, and the new entry.
pub fn replace_charge<K, V>() -> Option<IndexCharge> {
    let node = ord_node_bytes()?;
    search_charge()
        .plus(copy_charge(node)?)?
        .plus(nodes_charge(node, ORD_LEVELS)?)?
        .plus(entry_charge::<NativeEntry<K, V>>()?)
}

/// An insert of a new key: the search, the path copy, at most `2L + 2` new
/// nodes (a path copy and a split sibling per level, and the transient
/// default leaf and the new root of a root split), and the new entry.
pub fn insert_charge<K, V>() -> Option<IndexCharge> {
    let node = ord_node_bytes()?;
    search_charge()
        .plus(copy_charge(node)?)?
        .plus(nodes_charge(node, 2 * ORD_LEVELS + 2)?)?
        .plus(entry_charge::<NativeEntry<K, V>>()?)
}

/// A shared view of an entry: the pointer copy and the header touches of the
/// clone and of its release.
pub const fn view_charge() -> IndexCharge {
    IndexCharge {
        operations: 2,
        scanned: size_of::<usize>() + 2 * BLOCK_SHARED_HEADER_SCANNED,
        backing: 0,
    }
}

/// One stored entry: the key value, the value, and whether a publishing
/// write changed the entry since the session began. The key value serves the
/// collision check of D-C2e and the export; D-C3 reads the dirty flag.
#[derive(Debug)]
pub struct NativeEntry<K, V> {
    pub key: Arc<K>,
    pub value: V,
    pub dirty: bool,
}

pub type DigestShard<K, V> = imbl::OrdMap<StoreKey, Arc<NativeEntry<K, V>>>;

/// The shards and the key count of an index at a checkpoint.
pub struct DigestSnapshot<K, V> {
    shards: Box<[DigestShard<K, V>; NUM_SHARDS]>,
    keys: usize,
}

pub struct DigestShards<K, V> {
    shards: Box<[RwLock<DigestShard<K, V>>; NUM_SHARDS]>,
    keys: AtomicUsize,
}

impl<K, V> Default for DigestShards<K, V> {
    fn default() -> Self { Self::new() }
}

impl<K, V> DigestShards<K, V> {
    pub fn new() -> Self {
        Self {
            shards: Box::new(std::array::from_fn(|_| RwLock::new(imbl::OrdMap::new()))),
            keys: AtomicUsize::new(0),
        }
    }

    /// The operations and bytes of [`DigestShards::new`]: one boxed array of
    /// empty shards.
    pub fn constructor_layout() -> (usize, usize) {
        (NUM_SHARDS + 1, size_of::<[RwLock<DigestShard<K, V>>; NUM_SHARDS]>())
    }

    /// The shard count and the bytes of one snapshot.
    pub fn snapshot_layout() -> (usize, usize) {
        (NUM_SHARDS, size_of::<[DigestShard<K, V>; NUM_SHARDS]>())
    }

    /// The number of distinct keys stored.
    pub fn key_count(&self) -> usize { self.keys.load(Ordering::Acquire) }

    pub fn read(&self, key: &StoreKey) -> RwLockReadGuard<'_, DigestShard<K, V>> {
        self.shards[key.shard()]
            .read()
            .expect("digest shard read lock")
    }

    pub fn write(&self, key: &StoreKey) -> RwLockWriteGuard<'_, DigestShard<K, V>> {
        self.shards[key.shard()]
            .write()
            .expect("digest shard write lock")
    }

    pub fn write_shard(&self, shard: usize) -> RwLockWriteGuard<'_, DigestShard<K, V>> {
        self.shards[shard].write().expect("digest shard write lock")
    }

    /// D-C2c (DR-96): one shard, for the export's iteration in digest order.
    pub fn read_shard(&self, shard: usize) -> RwLockReadGuard<'_, DigestShard<K, V>> {
        self.shards[shard].read().expect("digest shard read lock")
    }

    /// The entry of `key`, after the search charge.
    pub fn get<'s>(
        shard: &'s DigestShard<K, V>,
        key: &StoreKey,
        meter: &dyn SourceMeter,
    ) -> Result<Option<&'s Arc<NativeEntry<K, V>>>, RSpaceError> {
        search_charge().reserve(meter)?;
        Ok(shard.get(key))
    }

    /// Inserts a key that the shard does not hold, after the search charge
    /// and the insert charge. A present key is rejected, as in the legacy
    /// store. Returns the stored entry; the insert also charges this clone of
    /// the stored pointer ([`view_charge`]).
    pub fn insert_new(
        &self,
        shard: &mut DigestShard<K, V>,
        key: StoreKey,
        value_key: Arc<K>,
        value: V,
        dirty: bool,
        meter: &dyn SourceMeter,
    ) -> Result<Arc<NativeEntry<K, V>>, RSpaceError> {
        search_charge().reserve(meter)?;
        if shard.contains_key(&key) {
            return Err(RSpaceError::HostWorkRejected);
        }
        // Changed by D-C2c (D-S1, DR-96): the returned entry is a clone of the
        // stored pointer, so the insert charges its view too.
        // insert_charge::<K, V>()
        //     .ok_or(RSpaceError::HostWorkRejected)?
        //     .reserve(meter)?;
        insert_charge::<K, V>()
            .and_then(|charge| charge.plus(view_charge()))
            .ok_or(RSpaceError::HostWorkRejected)?
            .reserve(meter)?;
        let entry = Arc::new(NativeEntry {
            key: value_key,
            value,
            dirty,
        });
        shard.insert(key, Arc::clone(&entry));
        self.keys.fetch_add(1, Ordering::AcqRel);
        Ok(entry)
    }

    /// The charge of a replace, reserved before the caller mutates anything.
    pub fn reserve_replace(meter: &dyn SourceMeter) -> Result<(), RSpaceError> {
        replace_charge::<K, V>()
            .ok_or(RSpaceError::HostWorkRejected)?
            .reserve(meter)
    }

    /// Replaces the value of a present key: imbl copies the path to the
    /// entry, and a new entry takes the old entry's key value. The caller
    /// found the key under the same write guard and reserved
    /// [`DigestShards::reserve_replace`].
    pub fn replace(shard: &mut DigestShard<K, V>, key: &StoreKey, value: V) {
        let slot = shard
            .get_mut(key)
            .expect("a replaced key was found under the same write guard");
        let key_value = Arc::clone(&slot.key);
        *slot = Arc::new(NativeEntry {
            key: key_value,
            value,
            dirty: true,
        });
    }

    /// The shards at this point: 256 constant-time clones. The caller holds
    /// the session's exclusive gate.
    pub fn snapshot(&self) -> DigestSnapshot<K, V> {
        DigestSnapshot {
            shards: Box::new(std::array::from_fn(|index| {
                self.shards[index]
                    .read()
                    .expect("digest shard read lock")
                    .clone()
            })),
            keys: self.keys.load(Ordering::Acquire),
        }
    }

    /// Returns the shards to a snapshot. The release of every node that the
    /// restore drops was prepaid when the node was allocated.
    pub fn restore(&self, snapshot: DigestSnapshot<K, V>) {
        for (lock, shard) in self.shards.iter().zip(*snapshot.shards) {
            *lock.write().expect("digest shard write lock") = shard;
        }
        self.keys.store(snapshot.keys, Ordering::Release);
    }
}

#[cfg(test)]
mod tests;
