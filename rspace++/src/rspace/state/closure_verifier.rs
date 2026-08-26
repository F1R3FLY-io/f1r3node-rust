use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

use shared::rust::store::key_value_store::KeyValueStore;

use super::import_session::StateImportError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::cold_store::{
    COLD_LEAF_ADDRESS_LEN, ColdLeafAddress, ColdLeafStoreError, PersistedDataKind, load_cold_leaf,
    validate_cold_leaf,
};
use crate::rspace::history::instances::radix_history::RadixHistory;
use crate::rspace::history::radix_tree::{Item, Node, decode, empty_node};

const RSPACE_HISTORY_KEY_LEN: usize = 33;
const DECODED_HISTORY_CACHE_MAX_ENTRIES: usize = 1_024;
const DECODED_HISTORY_CACHE_MAX_ENCODED_BYTES: usize = 8 * 1024 * 1024;
const VERIFIED_COLD_LEAF_CACHE_MAX_ENTRIES: usize = 4_096;
const VERIFIED_COLD_LEAF_CACHE_MAX_BYTES: usize =
    VERIFIED_COLD_LEAF_CACHE_MAX_ENTRIES * COLD_LEAF_ADDRESS_LEN;

pub(crate) type RecordValidator =
    Arc<dyn Fn(PersistedDataKind, &[u8]) -> Result<(), String> + Send + Sync>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StateClosureStats {
    pub roots: usize,
    pub history_objects_visited: usize,
    pub history_objects_read: usize,
    pub decoded_history_cache_hits: usize,
    pub cold_leaf_references: u64,
    pub cold_leaf_validations: u64,
    pub verified_cold_leaf_cache_hits: u64,
    pub records: u64,
    pub history_bytes_read: u64,
    pub cold_leaf_bytes_read: u64,
}

/// Proves that every target root reaches only present, content-bound radix
/// objects and typed, canonically framed cold leaves.
///
/// The traversal is an explicit worklist.  It has no call-stack dependence and
/// no traversal-depth cutoff.  The only path-length condition is the RSpace
/// history key schema itself: one collection-kind byte plus a 32-byte group
/// digest. Every child edge consumes at least one path byte, so malformed
/// cycles terminate at that schema boundary without an input-sized visited
/// set. Decoded objects use a fixed-size, fixed-byte-budget CLOCK cache: DAG
/// sharing can avoid repeated decoding, while adversarial input cannot turn
/// verification into a sloping full-state cache.
pub(crate) fn verify_state_closure(
    history_store: &dyn KeyValueStore,
    cold_store: &dyn KeyValueStore,
    roots: &[Blake2b256Hash],
    validate_record: &RecordValidator,
) -> Result<StateClosureStats, StateImportError> {
    verify_state_closure_with_getters(
        roots,
        validate_record,
        |node_hash| history_store.get_one(&node_hash.0).map_err(Into::into),
        |address| match load_cold_leaf(cold_store, address) {
            Ok(loaded) => Ok(Some(loaded.bytes().to_vec())),
            Err(ColdLeafStoreError::MissingObject(_)) => Ok(None),
            Err(error) => Err(error.into()),
        },
    )
}

/// Getter-based closure verifier for callers that must bind every read to one
/// storage snapshot or transaction.
///
/// The getters deliberately borrow their environment and need not be
/// `Send`, `Sync`, or `'static`.  An LMDB authority can therefore open one
/// write transaction, supply closures that read history and cold databases
/// through that transaction, and publish the verified catalog before the same
/// transaction commits.
pub(crate) fn verify_state_closure_with_getters<H, C>(
    roots: &[Blake2b256Hash],
    validate_record: &RecordValidator,
    mut get_history: H,
    mut get_cold: C,
) -> Result<StateClosureStats, StateImportError>
where
    H: FnMut(&Blake2b256Hash) -> Result<Option<Vec<u8>>, StateImportError>,
    C: FnMut(&ColdLeafAddress) -> Result<Option<Vec<u8>>, StateImportError>,
{
    let mut stats = StateClosureStats {
        roots: roots.len(),
        ..StateClosureStats::default()
    };
    let empty_root = RadixHistory::empty_root_node_hash();
    let mut work = roots
        .iter()
        .cloned()
        .map(|root| (root, Vec::new()))
        .collect::<Vec<_>>();
    let mut decoded_objects = BoundedClockCache::<Blake2b256Hash, Arc<Node>>::new(
        DECODED_HISTORY_CACHE_MAX_ENTRIES,
        DECODED_HISTORY_CACHE_MAX_ENCODED_BYTES,
    );
    let mut verified_leaves = BoundedClockCache::<ColdLeafAddress, ()>::new(
        VERIFIED_COLD_LEAF_CACHE_MAX_ENTRIES,
        VERIFIED_COLD_LEAF_CACHE_MAX_BYTES,
    );

    while let Some((node_hash, logical_prefix)) = work.pop() {
        validate_digest(&node_hash, "history object")?;
        stats.history_objects_visited = stats
            .history_objects_visited
            .checked_add(1)
            .ok_or(StateImportError::AccountingOverflow("history-object visit"))?;

        let node = if let Some(node) = decoded_objects.get(&node_hash) {
            stats.decoded_history_cache_hits = stats
                .decoded_history_cache_hits
                .checked_add(1)
                .ok_or(StateImportError::AccountingOverflow("decoded-cache hit"))?;
            node
        } else {
            let stored = get_history(&node_hash)?;
            let (node, encoded_weight) = match stored {
                Some(bytes) => {
                    let actual = Blake2b256Hash::new(&bytes);
                    if actual != node_hash {
                        return Err(StateImportError::HistoryDigestMismatch {
                            expected: node_hash,
                            actual,
                        });
                    }
                    stats.history_bytes_read = stats
                        .history_bytes_read
                        .checked_add(bytes.len() as u64)
                        .ok_or(StateImportError::AccountingOverflow("history-byte"))?;
                    let encoded_weight = bytes.len();
                    (Arc::new(decode(bytes)?), encoded_weight)
                }
                None if node_hash == empty_root => (Arc::new(empty_node()), 0),
                None => return Err(StateImportError::MissingHistoryObject(node_hash)),
            };
            stats.history_objects_read = stats
                .history_objects_read
                .checked_add(1)
                .ok_or(StateImportError::AccountingOverflow("history-object read"))?;
            decoded_objects.insert(node_hash, Arc::clone(&node), encoded_weight);
            node
        };

        for (index, item) in node.iter().enumerate() {
            let (suffix, target, is_leaf) = match item {
                Item::EmptyItem => continue,
                Item::Leaf { prefix, value } => (prefix, value, true),
                Item::NodePtr { prefix, ptr } => (prefix, ptr, false),
            };
            let mut child_prefix = Vec::with_capacity(
                logical_prefix
                    .len()
                    .saturating_add(1)
                    .saturating_add(suffix.len()),
            );
            child_prefix.extend_from_slice(&logical_prefix);
            child_prefix.push(index as u8);
            child_prefix.extend_from_slice(suffix);
            if child_prefix.len() > RSPACE_HISTORY_KEY_LEN {
                return Err(StateImportError::InvalidHistoryKeyPath {
                    available: child_prefix.len(),
                });
            }
            let target_hash = Blake2b256Hash::from_bytes(target.clone());
            validate_digest(
                &target_hash,
                if is_leaf {
                    "cold-leaf digest"
                } else {
                    "history child digest"
                },
            )?;

            if !is_leaf {
                work.push((target_hash, child_prefix));
                continue;
            }
            if child_prefix.len() != RSPACE_HISTORY_KEY_LEN {
                return Err(StateImportError::InvalidHistoryKeyPath {
                    available: child_prefix.len(),
                });
            }
            let address = ColdLeafAddress::from_history_prefix(&child_prefix, target_hash)?;
            stats.cold_leaf_references = stats
                .cold_leaf_references
                .checked_add(1)
                .ok_or(StateImportError::AccountingOverflow("cold-leaf reference"))?;
            if verified_leaves.get(&address).is_some() {
                stats.verified_cold_leaf_cache_hits = stats
                    .verified_cold_leaf_cache_hits
                    .checked_add(1)
                    .ok_or(StateImportError::AccountingOverflow("verified-leaf cache hit"))?;
                continue;
            }
            let cold_bytes = match get_cold(&address)? {
                Some(bytes) => bytes,
                None => return Err(StateImportError::MissingColdLeaf(address)),
            };
            stats.cold_leaf_bytes_read = stats
                .cold_leaf_bytes_read
                .checked_add(cold_bytes.len() as u64)
                .ok_or(StateImportError::AccountingOverflow("cold-leaf byte"))?;
            stats.cold_leaf_validations = stats
                .cold_leaf_validations
                .checked_add(1)
                .ok_or(StateImportError::AccountingOverflow("cold-leaf validation"))?;
            let records = validate_cold_leaf(&address, &cold_bytes)?;
            stats.records = stats
                .records
                .checked_add(records.record_count())
                .ok_or(StateImportError::AccountingOverflow("cold-leaf record"))?;
            for (record_index, record) in records.records().enumerate() {
                validate_record(address.kind(), record).map_err(|error| {
                    StateImportError::MalformedRecord {
                        address: address.clone(),
                        index: record_index,
                        error,
                    }
                })?;
            }
            verified_leaves.insert(address, (), COLD_LEAF_ADDRESS_LEN);
        }
    }
    Ok(stats)
}

struct ClockEntry<K, V> {
    key: K,
    value: V,
    encoded_weight: usize,
    referenced: bool,
}

/// Allocation-bounded O(1)-amortized cache for immutable content-addressed
/// objects. CLOCK avoids the list-node allocation and pointer chasing of an
/// LRU while retaining frequently reused DAG nodes.
struct BoundedClockCache<K, V> {
    slots: Vec<Option<ClockEntry<K, V>>>,
    by_key: HashMap<K, usize>,
    free: Vec<usize>,
    hand: usize,
    encoded_weight: usize,
    max_entries: usize,
    max_encoded_weight: usize,
}

impl<K, V> BoundedClockCache<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    fn new(max_entries: usize, max_encoded_weight: usize) -> Self {
        Self {
            slots: Vec::new(),
            by_key: HashMap::with_capacity(max_entries),
            free: Vec::new(),
            hand: 0,
            encoded_weight: 0,
            max_entries,
            max_encoded_weight,
        }
    }

    fn get(&mut self, key: &K) -> Option<V> {
        let slot = *self.by_key.get(key)?;
        let entry = self.slots.get_mut(slot)?.as_mut()?;
        entry.referenced = true;
        Some(entry.value.clone())
    }

    fn insert(&mut self, key: K, value: V, encoded_weight: usize) {
        if self.max_entries == 0 || encoded_weight > self.max_encoded_weight {
            return;
        }
        if let Some(slot) = self.by_key.get(&key).copied() {
            if let Some(entry) = self.slots[slot].as_mut() {
                entry.referenced = true;
            }
            return;
        }
        while self.by_key.len() >= self.max_entries ||
            self.encoded_weight
                .checked_add(encoded_weight)
                .is_none_or(|weight| weight > self.max_encoded_weight)
        {
            if !self.evict_one() {
                return;
            }
        }
        let slot = self.free.pop().unwrap_or_else(|| {
            self.slots.push(None);
            self.slots.len() - 1
        });
        self.encoded_weight += encoded_weight;
        self.slots[slot] = Some(ClockEntry {
            key: key.clone(),
            value,
            encoded_weight,
            referenced: true,
        });
        self.by_key.insert(key, slot);
    }

    fn evict_one(&mut self) -> bool {
        if self.by_key.is_empty() {
            return false;
        }
        loop {
            if self.hand >= self.slots.len() {
                self.hand = 0;
            }
            let slot = self.hand;
            self.hand += 1;
            let Some(entry) = self.slots[slot].as_mut() else {
                continue;
            };
            if entry.referenced {
                entry.referenced = false;
                continue;
            }
            let Some(entry) = self.slots[slot].take() else {
                continue;
            };
            self.by_key.remove(&entry.key);
            self.encoded_weight -= entry.encoded_weight;
            self.free.push(slot);
            return true;
        }
    }
}

fn validate_digest(digest: &Blake2b256Hash, field: &'static str) -> Result<(), StateImportError> {
    if digest.0.len() == 32 {
        Ok(())
    } else {
        Err(StateImportError::InvalidDigestWidth {
            field,
            available: digest.0.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use super::*;
    use crate::rspace::history::cold_store::{
        DataLeaf, PersistedData, PersistedDataKind, put_cold_leaves_if_absent_verified,
    };
    use crate::rspace::history::radix_tree::{Item, empty_node, hash_node};
    use crate::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

    fn validator() -> RecordValidator { Arc::new(|_, _| Ok(())) }

    #[test]
    fn empty_root_is_a_complete_closure_without_a_materialized_node() {
        let history = InMemoryKeyValueStore::new();
        let cold = InMemoryKeyValueStore::new();
        let stats = verify_state_closure(
            &history,
            &cold,
            &[RadixHistory::empty_root_node_hash()],
            &validator(),
        )
        .expect("canonical empty root closure");
        assert_eq!(stats.roots, 1);
        assert_eq!(stats.history_objects_read, 1);
        assert_eq!(stats.cold_leaf_references, 0);
        assert_eq!(stats.cold_leaf_validations, 0);
    }

    #[test]
    fn closure_verifies_the_authenticated_kind_and_record_sequence() {
        let history = InMemoryKeyValueStore::new();
        let cold = InMemoryKeyValueStore::new();
        let record = bincode::serialize(&"datum").expect("record");
        let payload = bincode::serialize(&vec![record.clone()]).expect("record sequence");
        let bytes = PersistedData::Data(DataLeaf { bytes: payload }).into_legacy_bincode();
        let view = crate::rspace::history::cold_store::PersistedDataView::parse_canonical(&bytes)
            .expect("canonical data envelope")
            .validate_records(PersistedDataKind::Data, Some(1))
            .expect("one record");
        let digest = Blake2b256Hash::new(view.leaf_encoding());
        let address =
            ColdLeafAddress::new(PersistedDataKind::Data, digest.clone()).expect("typed address");
        put_cold_leaves_if_absent_verified(&cold, vec![(address, bytes)])
            .expect("store typed leaf");

        let mut node = empty_node();
        node[0] = Item::Leaf {
            prefix: vec![7; 32],
            value: digest.0,
        };
        let (root_bytes, encoded) = hash_node(&node);
        history
            .put_one(root_bytes.clone(), encoded)
            .expect("store root node");
        let root = Blake2b256Hash::from_bytes(root_bytes);
        let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed_for_validator = Arc::clone(&observed);
        let validate: RecordValidator = Arc::new(move |kind, bytes| {
            observed_for_validator
                .lock()
                .expect("observed lock")
                .push((kind, bytes.to_vec()));
            Ok(())
        });
        let stats = verify_state_closure(&history, &cold, &[root], &validate)
            .expect("complete typed closure");
        assert_eq!(stats.cold_leaf_references, 1);
        assert_eq!(stats.cold_leaf_validations, 1);
        assert_eq!(stats.records, 1);
        assert_eq!(*observed.lock().expect("observed lock"), vec![(
            PersistedDataKind::Data,
            record
        )]);
    }

    #[test]
    fn getter_core_reads_child_nodes_and_cold_leaf_from_one_borrowed_snapshot() {
        let record = bincode::serialize(&"child datum").expect("record");
        let payload = bincode::serialize(&vec![record]).expect("record sequence");
        let cold_bytes = PersistedData::Data(DataLeaf { bytes: payload }).into_legacy_bincode();
        let view =
            crate::rspace::history::cold_store::PersistedDataView::parse_canonical(&cold_bytes)
                .expect("canonical data envelope")
                .validate_records(PersistedDataKind::Data, Some(1))
                .expect("one record");
        let digest = Blake2b256Hash::new(view.leaf_encoding());
        let address =
            ColdLeafAddress::new(PersistedDataKind::Data, digest.clone()).expect("typed address");

        let mut child = empty_node();
        child[1] = Item::Leaf {
            prefix: vec![8; 16],
            value: digest.0,
        };
        let (child_hash, child_bytes) = hash_node(&child);

        let mut root_node = empty_node();
        root_node[0] = Item::NodePtr {
            prefix: vec![7; 15],
            ptr: child_hash.clone(),
        };
        let (root_hash, root_bytes) = hash_node(&root_node);
        let root = Blake2b256Hash::from_bytes(root_hash.clone());

        let history_snapshot = BTreeMap::from([(root_hash, root_bytes), (child_hash, child_bytes)]);
        let cold_snapshot = BTreeMap::from([(address.encode(), cold_bytes)]);
        let mut history_reads = 0_u8;
        let mut cold_reads = 0_u8;

        let stats = verify_state_closure_with_getters(
            std::slice::from_ref(&root),
            &validator(),
            |hash| {
                history_reads += 1;
                Ok(history_snapshot.get(&hash.0).cloned())
            },
            |address| {
                cold_reads += 1;
                Ok(cold_snapshot.get(&address.encode()).cloned())
            },
        )
        .expect("transaction-bound getter closure");

        assert_eq!(history_reads, 2);
        assert_eq!(cold_reads, 1);
        assert_eq!(stats.history_objects_visited, 2);
        assert_eq!(stats.history_objects_read, 2);
        assert_eq!(stats.cold_leaf_references, 1);
        assert_eq!(stats.cold_leaf_validations, 1);
        assert_eq!(stats.records, 1);
    }

    #[test]
    fn shared_cold_leaf_is_reused_only_through_the_bounded_cache() {
        let history = InMemoryKeyValueStore::new();
        let cold = InMemoryKeyValueStore::new();
        let record = bincode::serialize(&"shared").expect("record");
        let payload = bincode::serialize(&vec![record]).expect("record sequence");
        let bytes = PersistedData::Data(DataLeaf { bytes: payload }).into_legacy_bincode();
        let view = crate::rspace::history::cold_store::PersistedDataView::parse_canonical(&bytes)
            .expect("canonical data envelope")
            .validate_records(PersistedDataKind::Data, Some(1))
            .expect("one record");
        let digest = Blake2b256Hash::new(view.leaf_encoding());
        let address =
            ColdLeafAddress::new(PersistedDataKind::Data, digest.clone()).expect("typed address");
        put_cold_leaves_if_absent_verified(&cold, vec![(address, bytes)])
            .expect("store shared leaf");

        let roots = [3_u8, 9_u8].map(|path_byte| {
            let mut node = empty_node();
            node[0] = Item::Leaf {
                prefix: vec![path_byte; 32],
                value: digest.0.clone(),
            };
            let (root_bytes, encoded) = hash_node(&node);
            history
                .put_one(root_bytes.clone(), encoded)
                .expect("store root node");
            Blake2b256Hash::from_bytes(root_bytes)
        });

        let stats = verify_state_closure(&history, &cold, &roots, &validator())
            .expect("both roots share a valid typed leaf");
        assert_eq!(stats.cold_leaf_references, 2);
        assert_eq!(stats.cold_leaf_validations, 1);
        assert_eq!(stats.verified_cold_leaf_cache_hits, 1);
    }

    #[test]
    fn missing_nonempty_root_is_rejected() {
        let history = InMemoryKeyValueStore::new();
        let cold = InMemoryKeyValueStore::new();
        let missing = Blake2b256Hash::new(b"missing");
        assert_eq!(
            verify_state_closure(&history, &cold, std::slice::from_ref(&missing), &validator()),
            Err(StateImportError::MissingHistoryObject(missing))
        );
    }

    #[test]
    fn decoded_object_clock_cache_is_bounded_by_entries_and_encoded_bytes() {
        let mut cache = BoundedClockCache::new(2, 5);
        cache.insert(1_u8, "one", 2);
        cache.insert(2_u8, "two", 2);
        assert_eq!(cache.get(&1), Some("one"));
        cache.insert(3_u8, "three", 2);
        assert!(cache.by_key.len() <= 2);
        assert!(cache.encoded_weight <= 5);
        assert_eq!(cache.get(&3), Some("three"));
        assert_ne!(cache.get(&1).is_some(), cache.get(&2).is_some());

        cache.insert(4_u8, "too-heavy", 6);
        assert_eq!(cache.get(&4), None);
        assert!(cache.by_key.len() <= 2);
        assert!(cache.encoded_weight <= 5);
    }
}
