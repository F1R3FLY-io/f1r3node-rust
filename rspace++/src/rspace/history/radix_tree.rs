// See rspace/src/main/scala/coop/rchain/rspace/history/RadixTree.scala

use std::collections::BTreeMap;
use std::sync::Arc;

use dashmap::DashMap;
use itertools::Itertools;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use shared::rust::store::key_value_store::KeyValueStore;
use shared::rust::{Byte, ByteVector};

use super::history_action::{DeleteAction, HistoryAction, InsertAction};
use crate::rspace::errors::RadixTreeError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::history_action::HistoryActionTrait;

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub enum Item {
    EmptyItem,

    Leaf {
        prefix: ByteVector,
        value: ByteVector,
    },

    NodePtr {
        prefix: ByteVector,
        ptr: ByteVector,
    },
}

pub type Node = Vec<Item>;

const NUM_ITEMS: usize = 256;

pub fn empty_node() -> Node { vec![Item::EmptyItem; NUM_ITEMS] }

/**
 * Binary codecs for serializing/deserializing Node in Radix tree
 *
 * ```text
 * Coding structure for items:
 *   EmptyItem                   - Empty (not encode)
 *
 *   Leaf(prefix,value)    -> [item index] [second byte] [prefix0]..[prefixM] [value0]..[value31]
 *                               where is: [second byte] -> bit7 = 0 (Leaf identifier)
 *                                                          bit6..bit0 - prefix length = M (from 0 to 127)
 *
 *   NodePtr(prefix,ptr)   -> [item index] [second byte] [prefix0]..[prefixM] [ptr0]..[ptr31]
 *                               where is: [second byte] -> bit7 = 1 (NodePtr identifier)
 *                                                          bit6..bit0 - prefix length = M (from 0 to 127)
 *
 * For example encode this Node which contains 2 non-empty items (index 1 and index 2):
 * (0)[Empty] (1)[Leaf(prefix:0xFFFF,value:0x00..0001)] (2)[NodePtr(prefix:empty,value:0xFF..FFFF)] (3)...(255)[Empty].
 * Encoded data = 0x0102FFFF0000000000000000000000000000000000000000000000000000000000000001
 *                  0280FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF
 * where: item 1 (index_secondByte_prefix_value) = 01_02_FFFF_00..0001
 *        item 2 (index_secondByte_prefix_value) = 02_80_empty_FF..FFFF
 * ```
 */

// Default size for non-empty item data
const DEF_SIZE: usize = 32;
// 2 bytes: first - item index, second - second byte
const HEAD_SIZE: usize = 2;
// Read cache bound to prevent unbounded memory growth during long reads.
const READ_CACHE_MAX_ITEMS: usize = 4_096;
const READ_CACHE_TRIM_TARGET: usize = 2_048;

/** Serialization [[Node]] to [[ByteVector]]
 */
pub fn encode(node: &Node) -> ByteVector {
    // Calculate the size of the serialized data
    let calc_size = node.iter().fold(0, |size, item| match item {
        Item::EmptyItem => size,

        Item::Leaf {
            prefix: leaf_prefix,
            value,
        } => {
            let size_prefix = leaf_prefix.len();
            assert!(
                size_prefix <= 127,
                "Error during serialization: size of prefix more than 127."
            );
            assert!(
                value.len() == DEF_SIZE,
                "Error during serialization: size of leafValue not equal 32."
            );
            size + HEAD_SIZE + size_prefix + DEF_SIZE
        }

        Item::NodePtr {
            prefix: ptr_prefix,
            ptr,
        } => {
            let size_prefix = ptr_prefix.len();
            assert!(
                size_prefix <= 127,
                "Error during serialization: size of prefix more than 127."
            );
            assert!(
                ptr.len() == DEF_SIZE,
                "Error during serialization: size of ptrPrefix not equal 32."
            );
            size + HEAD_SIZE + size_prefix + DEF_SIZE
        }
    });

    let buf = vec![0u8; calc_size]; //Allocation memory

    // Leaf second byte: Leaf identifier (most significant bit = 0) and prefixSize
    // (lower 7 bits = size)
    fn encode_leaf_second_byte(prefix_size: usize) -> u8 {
        (prefix_size & 0x7F) as u8 // (size & 01111111b)
    }

    // NodePtr second byte: NodePtr identifier (most significant bit = 1) and
    // prefixSize (lower 7 bits = size)
    fn encode_node_ptr_second_byte(prefix_size: usize) -> u8 {
        (0x80 | (prefix_size & 0x7F)) as u8 // 10000000b | (size & 01111111b)
    }

    fn put_item_into_array(
        idx_item: usize,
        pos0: usize,
        mut arr: Vec<u8>,
        node: &Node,
        calc_size: usize,
    ) -> Vec<u8> {
        if pos0 == calc_size {
            arr // Happy end (return serializing data).
        } else {
            match &node[idx_item] {
                // If current item is empty - just skip serialization of this item
                Item::EmptyItem => put_item_into_array(idx_item + 1, pos0, arr, node, calc_size), /* Loop to the next item. */

                Item::Leaf { prefix, value } => {
                    // Fill first byte - item index
                    arr[pos0] = idx_item as u8;
                    // Fill second byte - Leaf identifier
                    let pos_second_byte = pos0 + 1;
                    let prefix_size = prefix.len();
                    arr[pos_second_byte] = encode_leaf_second_byte(prefix_size);
                    // Fill prefix
                    let pos_prefix_start = pos_second_byte + 1;
                    for i in 0..prefix_size {
                        arr[pos_prefix_start + i] = prefix[i];
                    }
                    // Fill leafValue
                    let pos_value_start = pos_prefix_start + prefix_size;
                    for i in 0..DEF_SIZE {
                        arr[pos_value_start + i] = value[i];
                    }

                    // Loop to the next item.
                    put_item_into_array(
                        idx_item + 1,
                        pos_value_start + DEF_SIZE,
                        arr,
                        node,
                        calc_size,
                    )
                }

                Item::NodePtr { prefix, ptr } => {
                    // Fill first byte - item index
                    arr[pos0] = idx_item as u8;
                    // Fill second byte - NodePtr identifier (most significant bit = 1) and
                    // prefixSize (lower 7 bits = size)
                    let pos_second_byte = pos0 + 1;
                    let prefix_size = prefix.len();
                    arr[pos_second_byte] = encode_node_ptr_second_byte(prefix_size);
                    // Fill prefix
                    let pos_prefix_start = pos_second_byte + 1;
                    for i in 0..prefix_size {
                        arr[pos_prefix_start + i] = prefix[i];
                    }
                    // Fill ptr
                    let pos_ptr_start = pos_prefix_start + prefix_size;
                    for i in 0..DEF_SIZE {
                        arr[pos_ptr_start + i] = ptr[i];
                    }

                    // Loop to the next item.
                    put_item_into_array(
                        idx_item + 1,
                        pos_ptr_start + DEF_SIZE,
                        arr,
                        node,
                        calc_size,
                    )
                }
            }
        }
    }

    put_item_into_array(0, 0, buf, node, calc_size)
}

/** Deserialization [[ByteVector]] to [[Node]]
 */
pub fn decode(bv: ByteVector) -> Node {
    try_decode(&bv).unwrap_or_else(|e| panic!("Error during deserialization: {}", e))
}

pub fn try_decode(bv: &[u8]) -> Result<Node, RadixTreeError> {
    let max_size = bv.len();
    let malformed = |reason: &str| {
        RadixTreeError::SerializationError(format!(
            "Radix Tree - malformed node of {} bytes: {}",
            max_size, reason
        ))
    };

    // If first bit 0 - return true, otherwise false.
    fn is_leaf(second_byte: u8) -> bool { (second_byte & 0x80) == 0x00 }
    // Decode each non-empty item in-place to avoid cloning the whole node per step.
    let mut node = empty_node();
    let mut pos0 = 0;
    while pos0 < max_size {
        let idx_item = byte_to_int(bv[pos0]); // Take first byte - it's item's index
        if node[idx_item] != Item::EmptyItem {
            return Err(malformed("wrong index of item"));
        }
        let pos1 = pos0 + 1;
        let second_byte = *bv
            .get(pos1)
            .ok_or_else(|| malformed("truncated item header"))?; // Take second byte

        // Decoding prefix
        let prefix_size = (second_byte & 0x7F) as usize; // Lower 7 bits - it's size of prefix (0..127).
        let pos_prefix_start = pos1 + 1;
        let pos_val_or_ptr_start = pos_prefix_start + prefix_size;
        let prefix = bv
            .get(pos_prefix_start..pos_val_or_ptr_start)
            .ok_or_else(|| malformed("truncated item prefix"))?
            .to_vec();

        // Decoding leaf or nodePtr data
        let val_or_ptr = bv
            .get(pos_val_or_ptr_start..pos_val_or_ptr_start + DEF_SIZE)
            .ok_or_else(|| malformed("truncated item value"))?
            .to_vec();

        // Decoding type of non-empty item
        node[idx_item] = if is_leaf(second_byte) {
            Item::Leaf {
                prefix,
                value: val_or_ptr,
            }
        } else {
            Item::NodePtr {
                prefix,
                ptr: val_or_ptr,
            }
        };

        // Move to next encoded item.
        pos0 = pos_val_or_ptr_start + DEF_SIZE;
    }

    Ok(node)
}

fn common_prefix(b1: ByteVector, b2: ByteVector) -> (ByteVector, ByteVector, ByteVector) {
    fn go(
        common: ByteVector,
        l: ByteVector,
        r: ByteVector,
    ) -> (ByteVector, ByteVector, ByteVector) {
        if r.is_empty() || l.is_empty() {
            (common, l, r)
        } else {
            let (l_head, l_tail) = l.split_first().unwrap();
            let (r_head, r_tail) = r.split_first().unwrap();
            if l_head == r_head {
                let mut new_common = common.clone();
                new_common.push(*l_head);
                go(new_common, l_tail.to_vec(), r_tail.to_vec())
            } else {
                (common, l, r)
            }
        }
    }
    go(Vec::new(), b1, b2)
}

pub fn hash_node(node: &Node) -> (ByteVector, ByteVector) {
    let node_bytes = encode(node);
    let hash = Blake2b256Hash::new(&node_bytes);
    (hash.bytes(), node_bytes)
}

fn byte_to_int(b: u8) -> usize { b as usize & 0xff }

/**
 * Data returned after export
 *
 * @param nodePrefixes Node prefixes
 * @param nodeKeys Node KVDB keys
 * @param nodeValues Node KVDB values
 * @param leafPrefixes Leaf prefixes
 * @param leafValues Leaf values (it's pointer for data in datastore)
 */
#[derive(Clone, Debug)]
pub struct ExportData {
    pub node_prefixes: Vec<ByteVector>,
    pub node_keys: Vec<ByteVector>,
    pub node_values: Vec<ByteVector>,
    pub leaf_prefixes: Vec<ByteVector>,
    pub leaf_values: Vec<ByteVector>,
}

impl ExportData {
    fn new(
        node_prefixes: Vec<ByteVector>,
        node_keys: Vec<ByteVector>,
        node_values: Vec<ByteVector>,
        leaf_prefixes: Vec<ByteVector>,
        leaf_values: Vec<ByteVector>,
    ) -> Self {
        ExportData {
            node_prefixes,
            node_keys,
            node_values,
            leaf_prefixes,
            leaf_values,
        }
    }
}

/**
 * Settings for [[ExportData]]
 *
 * If false - data will not be exported.
 */
#[derive(Clone)]
pub struct ExportDataSettings {
    pub flag_node_prefixes: bool,
    pub flag_node_keys: bool,
    pub flag_node_values: bool,
    pub flag_leaf_prefixes: bool,
    pub flag_leaf_values: bool,
}

/**
 * Sequential export algorithm
 *
 * @param rootHash Root node hash, starting point
 * @param lastPrefix Describes the path of root to last processed element
 * (if None - start from root) @param skipSize Describes how many elements
 * to skip @param takeSize Describes how many elements to take
 * @param getNodeDataFromStore Function to get data from storage
 * @param settings [[ExportDataSettings]]
 *
 * @return
 * Return the data and prefix of the last processed item.
 * If all bonds in the tree are processed, returns None as prefix.
 * {{{
 * prefix - Prefix that describes the path of root to node
 * decoded - Deserialized data (from parsing)
 * lastItemIndex - Last processed item index
 * }}}
 */
pub fn sequential_export(
    root_hash: ByteVector,
    last_prefix: Option<ByteVector>,
    skip_size: i32,
    take_size: i32,
    get_node_data_from_store: Arc<dyn Fn(&ByteVector) -> Option<ByteVector>>,
    settings: ExportDataSettings,
) -> Result<(ExportData, Option<ByteVector>), RadixTreeError> {
    struct NodeData {
        prefix: ByteVector,
        decoded: Node,
        next_item_index: usize,
    }

    if (skip_size, take_size) == (0, 0) {
        panic!("Export error: invalid initial conditions (skipSize, takeSize)==(0,0).")
    }

    let mut exp_data = ExportData::new(Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());

    let Some(root_node_ser) = get_node_data_from_store(&root_hash) else {
        return Ok((exp_data, None));
    };

    let mut path: Vec<NodeData> = Vec::new();
    let mut node_prefix: ByteVector = Vec::new();
    let mut rest_prefix = last_prefix.clone().unwrap_or_default();
    let mut decoded = try_decode(&root_node_ser)?;
    while let Some((&item_index, rest_tail)) = rest_prefix.split_first() {
        let prefix_not_found = || {
            RadixTreeError::PrefixError(format!(
                "Radix Tree - Export error: node with prefix {} not found.",
                hex::encode([node_prefix.as_slice(), rest_prefix.as_slice()].concat())
            ))
        };
        let next_item_index = byte_to_int(item_index);
        let Some(Item::NodePtr { prefix, ptr }) = decoded.get(next_item_index) else {
            return Err(prefix_not_found());
        };
        let Some(next_rest_prefix) = rest_tail.strip_prefix(prefix.as_slice()) else {
            return Err(prefix_not_found());
        };
        let Some(child_ser) = get_node_data_from_store(ptr) else {
            return Err(RadixTreeError::KeyNotFound(format!(
                "Radix Tree - Export error: node with key {} not found.",
                hex::encode(ptr)
            )));
        };
        let child_decoded = try_decode(&child_ser)?;
        let mut child_prefix = node_prefix.clone();
        child_prefix.push(item_index);
        child_prefix.extend_from_slice(prefix);
        let next_rest_prefix = next_rest_prefix.to_vec();
        path.push(NodeData {
            prefix: std::mem::replace(&mut node_prefix, child_prefix),
            decoded: std::mem::replace(&mut decoded, child_decoded),
            next_item_index: next_item_index + 1,
        });
        rest_prefix = next_rest_prefix;
    }
    path.push(NodeData {
        prefix: node_prefix,
        decoded,
        next_item_index: 0,
    });

    let mut skip = skip_size;
    let mut take = take_size;
    if last_prefix.is_none() {
        if skip > 0 {
            skip -= 1;
        } else {
            if settings.flag_node_prefixes {
                exp_data.node_prefixes.push(Vec::new());
            }
            if settings.flag_node_keys {
                exp_data.node_keys.push(root_hash);
            }
            if settings.flag_node_values {
                exp_data.node_values.push(root_node_ser);
            }
            take -= 1;
        }
    }

    while let Some(current) = path.last_mut() {
        if (skip, take) == (0, 0) {
            return Ok((exp_data, Some(current.prefix.clone())));
        }

        let first = current.next_item_index;
        let found = current.decoded[first..]
            .iter()
            .position(|item| match item {
                Item::EmptyItem => false,
                Item::Leaf { .. } => settings.flag_leaf_prefixes || settings.flag_leaf_values,
                Item::NodePtr { .. } => true,
            })
            .map(|offset| first + offset);
        let Some(index) = found else {
            path.pop();
            continue;
        };
        current.next_item_index = index + 1;

        let item_prefix = |prefix: &ByteVector| {
            let mut full = Vec::with_capacity(current.prefix.len() + 1 + prefix.len());
            full.extend_from_slice(&current.prefix);
            full.push(index as u8);
            full.extend_from_slice(prefix);
            full
        };
        match &current.decoded[index] {
            Item::EmptyItem => {}
            Item::Leaf { prefix, value } => {
                if skip <= 0 {
                    if settings.flag_leaf_prefixes {
                        exp_data.leaf_prefixes.push(item_prefix(prefix));
                    }
                    if settings.flag_leaf_values {
                        exp_data.leaf_values.push(value.clone());
                    }
                }
            }
            Item::NodePtr { prefix, ptr } => {
                let Some(child_ser) = get_node_data_from_store(ptr) else {
                    return Err(RadixTreeError::KeyNotFound(format!(
                        "Radix Tree - Export error: node with key {} not found. here",
                        hex::encode(ptr)
                    )));
                };
                let child_decoded = try_decode(&child_ser)?;
                let child_prefix = item_prefix(prefix);
                let child_key = ptr.clone();
                if skip > 0 {
                    skip -= 1;
                } else {
                    if settings.flag_node_prefixes {
                        exp_data.node_prefixes.push(child_prefix.clone());
                    }
                    if settings.flag_node_keys {
                        exp_data.node_keys.push(child_key);
                    }
                    if settings.flag_node_values {
                        exp_data.node_values.push(child_ser);
                    }
                    take -= 1;
                }
                path.push(NodeData {
                    prefix: child_prefix,
                    decoded: child_decoded,
                    next_item_index: 0,
                });
            }
        }
    }

    Ok((exp_data, None))
}

/**
 * Radix Tree implementation
 */
#[derive(Clone)]
pub struct RadixTreeImpl {
    pub store: Arc<dyn KeyValueStore>,
    /**
     * Cache for storing read and decoded nodes.
     *
     * Cache stores kv-pairs (hash, node).
     * Where hash - Blake2b256Hash of serializing nodes data,
     *       node - deserialized data of this node.
     */
    pub cache_r: DashMap<ByteVector, Arc<Node>>,
    /**
     * Cache for storing serializing nodes. For subsequent unloading in KVDB
     *
     * Cache stores kv-pairs (hash, bytes).
     * Where hash -  Blake2b256Hash of bytes,
     *       bytes - serializing data of nodes.
     */
    pub cache_w: DashMap<ByteVector, ByteVector>,
}

impl RadixTreeImpl {
    pub fn new(store: Arc<dyn KeyValueStore>) -> Self {
        RadixTreeImpl {
            store,
            cache_r: DashMap::new(),
            cache_w: DashMap::new(),
        }
    }

    /**
     * Load and decode serializing data from KVDB.
     */
    fn load_node_from_store(&self, node_ptr: &ByteVector) -> Result<Option<Node>, RadixTreeError> {
        let get_result = self.store.get_one(node_ptr)?;

        match get_result {
            Some(bytes) => {
                let deserialized_node = decode(bytes);
                Ok(Some(deserialized_node))
            }
            None => Ok(None),
        }
    }

    /**
     * Load one node from [[cacheR]].
     *
     * If there is no such record in cache - load and decode from KVDB, then
     * save to cacheR. If there is no such record in KVDB - execute
     * assert (if set noAssert flag - return emptyNode).
     */
    pub fn load_node(
        &self,
        node_ptr: ByteVector,
        no_assert: Option<bool>,
    ) -> Result<Node, RadixTreeError> {
        self.load_node_arc(node_ptr, no_assert)
            .map(|node| node.as_ref().clone())
    }

    fn enforce_read_cache_bounds(&self) {
        if self.cache_r.len() <= READ_CACHE_MAX_ITEMS {
            return;
        }

        let number_to_remove = self.cache_r.len().saturating_sub(READ_CACHE_TRIM_TARGET);
        let keys = self
            .cache_r
            .iter()
            .take(number_to_remove)
            .map(|entry| entry.key().clone())
            .collect::<Vec<_>>();
        for key in keys {
            self.cache_r.remove(&key);
        }
    }

    pub fn load_node_arc(
        &self,
        node_ptr: ByteVector,
        no_assert: Option<bool>,
    ) -> Result<Arc<Node>, RadixTreeError> {
        // `no_assert = Some(true)` preserves the legacy "tolerate missing
        // root" semantics used by `RadixHistory::create` / `::reset`: an
        // absent root is reported as an empty tree, so a newly-created
        // history can bootstrap against a not-yet-imported root. Any OTHER
        // call site now gets a typed `MissingNode` error instead of the
        // panic it used to get. See issue #923 Option D: the pre-#1045
        // assert crashed the node on a sweep-vs-write race; the typed error
        // lets replay / merge / block-report / exploratory-deploy surface
        // a precise "pruned subtree" answer.
        let tolerate_missing = no_assert.unwrap_or(false);

        if let Some(cached) = self.cache_r.get(&node_ptr) {
            return Ok(Arc::clone(cached.value()));
        }

        let store_node_opt = self.load_node_from_store(&node_ptr)?;
        match store_node_opt {
            Some(node) => {
                let node_arc = Arc::new(node);
                self.cache_r.insert(node_ptr.clone(), Arc::clone(&node_arc));
                self.enforce_read_cache_bounds();
                Ok(node_arc)
            }
            None if tolerate_missing => Ok(Arc::new(empty_node())),
            None => Err(RadixTreeError::MissingNode {
                ptr: Blake2b256Hash::from_bytes(node_ptr),
            }),
        }
    }

    /**
     * Clear [[cacheR]] (cache for storing read nodes).
     */
    pub fn clear_read_cache(&self) {
        self.cache_r.clear();
        self.cache_r.shrink_to_fit();
    }

    /**
     * Serializing and hashing one [[Node]].
     *
     * Serializing data load in [[cacheW]].
     * If detected collision with older cache data - executing assert
     */
    pub fn save_node(&self, node: Node) -> ByteVector {
        let (node_bytes_hash, node_bytes) = hash_node(&node);
        let check_collision = |v: &Node| {
            assert!(
                *v == node,
                "Radix Tree - Collision in cache: record with key = ${} has already existed.",
                hex::encode(node_bytes_hash.clone())
            )
        };

        match self.cache_r.get(&node_bytes_hash) {
            Some(node) => check_collision(node.value().as_ref()),
            None => {
                self.cache_r
                    .insert(node_bytes_hash.clone(), Arc::new(node.clone()));
                self.enforce_read_cache_bounds();
            }
        };

        self.cache_w.insert(node_bytes_hash.clone(), node_bytes);
        node_bytes_hash
    }

    /**
     * Save all [[cacheW]] to [[store]]
     *
     * If detected collision with older KVDB data - execute Exception
     */
    pub fn commit(&self) -> Result<(), RadixTreeError> {
        let pending = self.pending_writes()?;
        self.store.put(pending)?;
        Ok(())
    }

    pub fn pending_writes(&self) -> Result<Vec<(ByteVector, ByteVector)>, RadixTreeError> {
        fn collision_panic(collisions: Vec<(ByteVector, ByteVector)>) -> RadixTreeError {
            RadixTreeError::CollisionError(format!(
                "{} collisions in KVDB (first collision with key = {}.",
                collisions.len(),
                hex::encode(collisions.first().unwrap().0.clone()),
            ))
        }

        let kv_pairs: Vec<(ByteVector, ByteVector)> = self
            .cache_w
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect();

        let if_absent: Vec<bool> = self
            .store
            .contains(&kv_pairs.clone().into_iter().map(|(k, _)| k).collect_vec())?;
        let kv_if_absent: Vec<((ByteVector, ByteVector), bool)> =
            kv_pairs.into_iter().zip(if_absent.into_iter()).collect();

        let kv_exist: Vec<(ByteVector, ByteVector)> = kv_if_absent
            .iter()
            .filter(|&(_, absent)| *absent)
            .map(|(kv, _)| kv.clone())
            .collect();

        let value_exist_in_store: Vec<Option<ByteVector>> = self
            .store
            .get(&kv_exist.clone().into_iter().map(|(k, _)| k).collect_vec())?;

        let kvv_exist: Vec<((ByteVector, ByteVector), ByteVector)> = kv_exist
            .into_iter()
            .zip(
                value_exist_in_store
                    .into_iter()
                    .map(|v| v.unwrap_or_else(|| Vec::new())),
            )
            .collect();

        let kv_collision: Vec<(ByteVector, ByteVector)> = kvv_exist
            .into_iter()
            .filter(|kvv| !(kvv.0.1 == kvv.1))
            .map(|(kv, _)| kv)
            .collect();

        if !kv_collision.is_empty() {
            return Err(collision_panic(kv_collision));
        }

        let kv_absent: Vec<(ByteVector, ByteVector)> = kv_if_absent
            .into_iter()
            .filter(|&(_, absent)| !absent)
            .map(|(kv, _)| kv)
            .collect();

        Ok(kv_absent)
    }

    /**
     * Clear [[cacheW]] (cache for storing data to write in KVDB).
     */
    pub fn clear_write_cache(&self) {
        self.cache_w.clear();
        self.cache_w.shrink_to_fit();
    }

    /**
     * Read leaf data with prefix. If data not found, returned [[None]]
     */
    fn read_at(
        &self,
        cur_node: &Node,
        prefix: &[u8],
    ) -> Result<Option<ByteVector>, RadixTreeError> {
        self.read_at_cached(cur_node, prefix)
    }

    fn read_at_cached(
        &self,
        cur_node: &Node,
        prefix: &[u8],
    ) -> Result<Option<ByteVector>, RadixTreeError> {
        if prefix.is_empty() {
            return Ok(None);
        }

        match &cur_node[byte_to_int(prefix[0])] {
            Item::EmptyItem => Ok(None),
            Item::Leaf {
                prefix: leaf_prefix,
                value,
            } => {
                let prefix_tail = &prefix[1..];
                if leaf_prefix.as_slice() == prefix_tail {
                    Ok(Some(value.clone()))
                } else {
                    Ok(None)
                }
            }
            Item::NodePtr {
                prefix: ptr_prefix,
                ptr,
            } => {
                let prefix_tail = &prefix[1..];
                if !prefix_tail.starts_with(ptr_prefix.as_slice()) {
                    return Ok(None);
                }

                let prefix_rest = &prefix_tail[ptr_prefix.len()..];
                self.read_at_from_ptr(ptr, prefix_rest)
            }
        }
    }

    fn read_at_from_ptr(
        &self,
        node_ptr: &ByteVector,
        prefix: &[u8],
    ) -> Result<Option<ByteVector>, RadixTreeError> {
        if prefix.is_empty() {
            return Ok(None);
        }

        let node = self.load_node_arc(node_ptr.clone(), None)?;
        self.read_at_cached(node.as_ref(), prefix)
    }

    pub fn read(
        &self,
        start_node: &Node,
        start_prefix: &[u8],
    ) -> Result<Option<ByteVector>, RadixTreeError> {
        self.read_at(start_node, start_prefix)
    }

    fn create_node_from_item(&self, item: Item) -> Node {
        match item {
            Item::EmptyItem => empty_node(),
            Item::Leaf {
                prefix: leaf_prefix,
                value: leaf_value,
            } => {
                assert!(
                    !leaf_prefix.is_empty(),
                    "Impossible to create a node. LeafPrefix should be non empty."
                );

                let index = byte_to_int(*leaf_prefix.first().unwrap());
                let (_, leaf_prefix_tail) = leaf_prefix.split_first().unwrap();
                let mut empty_node = empty_node();
                empty_node[index] = Item::Leaf {
                    prefix: leaf_prefix_tail.to_vec(),
                    value: leaf_value,
                };

                empty_node
            }
            Item::NodePtr {
                prefix: node_ptr_prefix,
                ptr,
            } => {
                assert!(
                    !node_ptr_prefix.is_empty(),
                    "Impossible to create a node. NodePtrPrefix should be non empty."
                );

                let index = byte_to_int(*node_ptr_prefix.first().unwrap());
                let (_, node_ptr_prefix_tail) = node_ptr_prefix.split_first().unwrap();
                let mut empty_node = empty_node();
                empty_node[index] = Item::NodePtr {
                    prefix: node_ptr_prefix_tail.to_vec(),
                    ptr,
                };

                empty_node
            }
        }
    }

    /**
     * Create node from [[Item]].
     *
     * If item is NodePtr and prefix is empty - load child node
     */
    fn construct_node_from_item(&self, item: &Item) -> Result<Arc<Node>, RadixTreeError> {
        match item {
            Item::NodePtr { prefix, ptr } if prefix.is_empty() => {
                self.load_node_arc(ptr.clone(), None)
            }
            _ => Ok(Arc::new(self.create_node_from_item(item.clone()))),
        }
    }

    /**
     * Optimize and save Node, create item from this Node
     */
    fn save_node_and_create_item(&self, node: Node, prefix: ByteVector, compaction: bool) -> Item {
        if compaction {
            let non_empty_items: Vec<Item> = node
                .clone()
                .into_iter()
                .filter(|item| *item != Item::EmptyItem)
                .take(2)
                .collect();

            match non_empty_items.len() {
                0 => Item::EmptyItem, // All items are empty.
                1 => {
                    // Only one item is not empty - merge child and parent nodes.
                    let idx_item = node
                        .iter()
                        .position(|item| *item == non_empty_items[0])
                        .unwrap();

                    match &non_empty_items[0] {
                        Item::EmptyItem => Item::EmptyItem,
                        Item::Leaf {
                            prefix: leaf_prefix,
                            value,
                        } => {
                            let mut new_prefix = prefix.clone();
                            new_prefix.push(idx_item as u8);
                            new_prefix.append(&mut leaf_prefix.clone());
                            Item::Leaf {
                                prefix: new_prefix,
                                value: value.clone(),
                            }
                        }
                        Item::NodePtr {
                            prefix: node_ptr_prefix,
                            ptr,
                        } => {
                            let mut new_prefix = prefix.clone();
                            new_prefix.push(idx_item as u8);
                            new_prefix.append(&mut node_ptr_prefix.clone());
                            Item::NodePtr {
                                prefix: new_prefix,
                                ptr: ptr.to_vec(),
                            }
                        }
                    }
                }
                2 => {
                    Item::NodePtr {
                        // 2 or more items are not empty.
                        prefix,
                        ptr: self.save_node(node),
                    }
                }
                _ => unreachable!(),
            }
        } else {
            Item::NodePtr {
                prefix,
                ptr: self.save_node(node),
            }
        }
    }

    /**
     * Save new leaf value to this part of tree (start from curItems).
     * Rehash and save all depend node to [[cacheW]].
     *
     * If exist leaf with same prefix but different value - update leaf
     * value. If exist leaf with same prefix and same value - return
     * [[None]]. @return Updated current item.
     */
    fn update(
        &self,
        curr_item: Item,
        ins_prefix: ByteVector,
        ins_value: ByteVector,
    ) -> Result<Option<Item>, RadixTreeError> {
        let insert_new_node_to_child: Box<
            dyn Fn(ByteVector, ByteVector, ByteVector) -> Result<Option<Item>, RadixTreeError>,
        > = Box::new(|child_ptr: ByteVector, child_prefix: ByteVector, ins_prefix: ByteVector| {
            let child_node = self.load_node_arc(child_ptr, None)?;
            let (ins_prefix_head, ins_prefix_tail) = ins_prefix.split_first().unwrap();
            let (child_item_idx, child_ins_prefix) =
                (byte_to_int(*ins_prefix_head), ins_prefix_tail);

            let child_item_opt = self.update(
                child_node.as_ref().get(child_item_idx).unwrap().clone(),
                child_ins_prefix.to_vec(),
                ins_value.clone(),
            )?; // Deeper

            match child_item_opt {
                Some(child_item) => {
                    let mut updated_child_node = child_node.as_ref().clone();
                    updated_child_node[child_item_idx] = child_item;
                    let returned_item =
                        self.save_node_and_create_item(updated_child_node, child_prefix, false);
                    Ok(Some(returned_item))
                }
                None => Ok(None),
            }
        });

        match curr_item {
            Item::EmptyItem => Ok(Some(Item::Leaf {
                prefix: ins_prefix,
                value: ins_value.clone(),
            })),
            Item::Leaf {
                prefix: leaf_prefix,
                value: leaf_value,
            } => {
                assert!(
                    leaf_prefix.len() == ins_prefix.len(),
                    "Radix Tree: All Radix keys should be same length."
                );

                if leaf_prefix == ins_prefix {
                    if ins_value == leaf_value {
                        Ok(None)
                    } else {
                        Ok(Some(Item::Leaf {
                            prefix: ins_prefix,
                            value: ins_value.clone(),
                        })) // Update Leaf.
                    }
                } else {
                    // Create child node, insert existing and new leaf in this node.
                    // Intentionally not recursive for speed up.
                    let (comm_prefix, ins_prefix_rest, leaf_prefix_rest) =
                        common_prefix(ins_prefix, leaf_prefix);

                    let mut new_node = empty_node();
                    let (leaf_prefix_rest_head, leaf_prefix_rest_tail) =
                        leaf_prefix_rest.split_first().unwrap();

                    new_node[byte_to_int(*leaf_prefix_rest_head)] = Item::Leaf {
                        prefix: leaf_prefix_rest_tail.to_vec(),
                        value: leaf_value,
                    };

                    let (ins_prefix_rest_head, ins_prefix_rest_tail) =
                        ins_prefix_rest.split_first().unwrap();

                    new_node[byte_to_int(*ins_prefix_rest_head)] = Item::Leaf {
                        prefix: ins_prefix_rest_tail.to_vec(),
                        value: ins_value.clone(),
                    };

                    Ok(Some(self.save_node_and_create_item(new_node, comm_prefix, false)))
                }
            }
            Item::NodePtr {
                prefix: ptr_prefix,
                ptr,
            } => {
                assert!(
                    ptr_prefix.len() < ins_prefix.len(),
                    "Radix Tree: Radix key should be longer than NodePtr key."
                );

                let (comm_prefix, ins_prefix_rest, ptr_prefix_rest) =
                    common_prefix(ins_prefix, ptr_prefix);

                if ptr_prefix_rest.is_empty() {
                    insert_new_node_to_child(ptr, comm_prefix, ins_prefix_rest) // Add new node to existing child node.
                } else {
                    // Create child node, insert existing Ptr and new leaf in this node.
                    let mut new_node = empty_node();
                    let (ptr_prefix_rest_head, ptr_prefix_rest_tail) =
                        ptr_prefix_rest.split_first().unwrap();

                    new_node[byte_to_int(*ptr_prefix_rest_head)] = Item::NodePtr {
                        prefix: ptr_prefix_rest_tail.to_vec(),
                        ptr: ptr.clone(),
                    };

                    let (ins_prefix_rest_head, ins_prefix_rest_tail) =
                        ins_prefix_rest.split_first().unwrap();

                    new_node[byte_to_int(*ins_prefix_rest_head)] = Item::Leaf {
                        prefix: ins_prefix_rest_tail.to_vec(),
                        value: ins_value.clone(),
                    };

                    Ok(Some(self.save_node_and_create_item(new_node, comm_prefix, false)))
                }
            }
        }
    }

    /**
     * Delete leaf value from this part of tree (start from curItem).
     * Rehash and save all depend node to cacheW.
     *
     * If not found leaf with  delPrefix - return [[None]].
     * @return Updated current item.
     */
    fn delete(
        &self,
        curr_item: Item,
        del_prefix: ByteVector,
    ) -> Result<Option<Item>, RadixTreeError> {
        let delete_from_child_node: Box<
            dyn Fn(ByteVector, ByteVector, ByteVector) -> Result<Option<Item>, RadixTreeError>,
        > = Box::new(|child_ptr: ByteVector, child_prefix: ByteVector, del_prefix: ByteVector| {
            let child_node = self.load_node_arc(child_ptr, None)?;
            let (del_prefix_head, del_prefix_tail) = del_prefix.split_first().unwrap();
            let (del_item_idx, del_item_prefix) = (byte_to_int(*del_prefix_head), del_prefix_tail);
            let child_item_opt = self.delete(
                child_node.as_ref().get(del_item_idx).unwrap().clone(),
                del_item_prefix.to_vec(),
            )?;

            match child_item_opt {
                Some(child_item) => {
                    let mut child_node_updated = child_node.as_ref().clone();
                    // child_node_updated.insert(del_item_idx, child_item);
                    child_node_updated[del_item_idx] = child_item;
                    Ok(Some(self.save_node_and_create_item(child_node_updated, child_prefix, true)))
                }
                None => Ok(None),
            }
        });

        match curr_item {
            Item::EmptyItem => Ok(None), // Not found
            Item::Leaf {
                prefix: leaf_prefix,
                value: _,
            } => {
                if leaf_prefix == del_prefix {
                    Ok(Some(Item::EmptyItem)) // Happy end
                } else {
                    Ok(None) // Not found
                }
            }
            Item::NodePtr {
                prefix: ptr_prefix,
                ptr,
            } => {
                let (comm_prefix, del_prefix_rest, ptr_prefix_rest) =
                    common_prefix(del_prefix, ptr_prefix);

                if !ptr_prefix_rest.is_empty() || del_prefix_rest.is_empty() {
                    Ok(None) // Not found
                } else {
                    delete_from_child_node(ptr, comm_prefix, del_prefix_rest) // Deeper
                }
            }
        }
    }

    /**
     * Parallel processing of [[HistoryAction]]s in this part of tree (start
     * from curNode).
     *
     * New data load to [[cacheW]].
     * @return Updated curNode. if no action was taken - return [[None]].
     */
    pub fn make_actions(
        &self,
        curr_node: &Node,
        actions: Vec<HistoryAction>,
    ) -> Result<Option<Node>, RadixTreeError> {
        // If we have 1 action in group.
        // We can't parallel next and we should use sequential traversing with help
        // update() or delete().
        let process_one_action: Box<
            dyn Fn(HistoryAction, Item, i32) -> Result<(i32, Option<Item>), RadixTreeError>
                + Send
                + Sync,
        > = Box::new(|action: HistoryAction, item: Item, item_idx: i32| {
            let new_item = match action {
                HistoryAction::Insert(InsertAction { key, hash }) => {
                    let (_, key_tail) = key.split_first().unwrap();
                    self.update(item, key_tail.to_vec(), hash.bytes())
                }
                HistoryAction::Delete(DeleteAction { key }) => {
                    let (_, key_tail) = key.split_first().unwrap();
                    self.delete(item, key_tail.to_vec())
                }
            }?;

            Ok((item_idx, new_item))
        });

        fn clearing_delete_actions(actions: Vec<HistoryAction>, item: Item) -> Vec<HistoryAction> {
            let not_exist_insert_action = actions
                .iter()
                .find_map(|action| {
                    if let HistoryAction::Insert(_) = action {
                        Some(true)
                    } else {
                        None
                    }
                })
                .is_none();

            if item == Item::EmptyItem && not_exist_insert_action {
                Vec::new()
            } else {
                actions
            }
        }

        fn trim_keys(actions: Vec<HistoryAction>) -> Vec<HistoryAction> {
            actions
                .iter()
                .map(|action| match action {
                    HistoryAction::Insert(InsertAction { key, hash }) => {
                        let (_, key_tail) = key.split_first().unwrap();
                        HistoryAction::Insert(InsertAction {
                            key: key_tail.to_vec(),
                            hash: hash.clone(),
                        })
                    }
                    HistoryAction::Delete(DeleteAction { key }) => {
                        let (_, key_tail) = key.split_first().unwrap();
                        HistoryAction::Delete(DeleteAction {
                            key: key_tail.to_vec(),
                        })
                    }
                })
                .collect()
        }

        let process_non_empty_actions: Box<
            dyn Fn(Vec<HistoryAction>, i32) -> Result<(i32, Option<Item>), RadixTreeError>
                + Send
                + Sync,
        > = Box::new(|actions: Vec<HistoryAction>, item_idx: i32| {
            let created_node =
                self.construct_node_from_item(curr_node.get(item_idx as usize).unwrap())?;

            let new_actions = trim_keys(actions);
            let new_node_opt = self.make_actions(created_node.as_ref(), new_actions)?;

            let new_item = new_node_opt
                .map(|new_node| self.save_node_and_create_item(new_node, Vec::new(), true));

            Ok((item_idx, new_item))
        });

        // If we have more than 1 action. We can create more parallel processes.
        let process_several_actions: Box<
            dyn Fn(Vec<HistoryAction>, Item, i32) -> Result<(i32, Option<Item>), RadixTreeError>
                + Send
                + Sync,
        > = Box::new(|actions: Vec<HistoryAction>, item: Item, item_idx: i32| {
            let cleared_actions = clearing_delete_actions(actions, item);
            if cleared_actions.is_empty() {
                Ok((item_idx, None))
            } else {
                process_non_empty_actions(cleared_actions, item_idx)
            }
        });

        // Process actions within each group.
        let process_grouped_actions: Box<
            dyn Fn(
                Vec<(Byte, Vec<HistoryAction>)>,
                &Node,
            ) -> Vec<Result<(i32, Option<Item>), RadixTreeError>>,
        > = Box::new(|grouped_actions: Vec<(Byte, Vec<HistoryAction>)>, curr_node: &Node| {
            grouped_actions
                .par_iter()
                .map(|grouped_action| match grouped_action {
                    (group_idx, actions_in_group) => {
                        let item_idx = byte_to_int(*group_idx);
                        let item = curr_node[item_idx].clone();
                        if actions_in_group.len() == 1 {
                            process_one_action(
                                actions_in_group.first().unwrap().clone(),
                                item,
                                item_idx as i32,
                            )
                        } else {
                            process_several_actions(
                                actions_in_group.to_vec(),
                                item,
                                item_idx as i32,
                            )
                        }
                    }
                })
                .collect()
        });

        // Group the actions by the first byte of the prefix.
        fn grouping(
            actions: Vec<HistoryAction>,
        ) -> Result<Vec<(Byte, Vec<HistoryAction>)>, RadixTreeError> {
            let mut groups: BTreeMap<Byte, Vec<HistoryAction>> = BTreeMap::new();
            for action in actions {
                let first_byte = match action.key().first() {
                    Some(b) => *b,
                    None => {
                        return Err(RadixTreeError::PrefixError(
                            "The length of all prefixes in the subtree must be the same."
                                .to_string(),
                        ));
                    }
                };
                groups.entry(first_byte).or_default().push(action);
            }
            Ok(groups.into_iter().collect())
        }

        // Group the actions by the first byte of the prefix.
        let grouped_actions = grouping(actions)?;

        // Process actions within each group.
        // TODO: Update to handle parallel execution. See Scala side
        let new_group_items_results = process_grouped_actions(grouped_actions, curr_node);

        let mut new_group_items = Vec::with_capacity(new_group_items_results.len());
        for result in new_group_items_results {
            let value = result?;
            new_group_items.push(value);
        }

        // Update all changed items in current node.
        let mut new_cur_node = curr_node.clone();
        for (index, new_item_opt) in new_group_items {
            new_cur_node = match new_item_opt {
                Some(new_item) => {
                    new_cur_node[index as usize] = new_item;
                    new_cur_node
                }
                None => new_cur_node,
            };
        }
        // If current node changing return new node, otherwise return none.
        if new_cur_node != *curr_node {
            Ok(Some(new_cur_node))
        } else {
            Ok(None)
        }
    }

    pub fn print_tree(&self, node: &Node, prefix: Vec<u8>) -> Result<String, RadixTreeError> {
        let mut output = String::new();
        for item in node.iter() {
            match item {
                Item::EmptyItem => {
                    // Skip empty items
                }
                Item::Leaf {
                    prefix: leaf_prefix,
                    value,
                } => {
                    let full_prefix = [prefix.clone(), leaf_prefix.clone()].concat();
                    let prefix_str = format!("{:02X?}", full_prefix);
                    let value_str = format!("{:02X?}", value);
                    output.push_str(&format!("Leaf at prefix:{}: data: {}", prefix_str, value_str));
                }
                Item::NodePtr {
                    prefix: node_prefix,
                    ptr,
                } => {
                    let full_prefix = [prefix.clone(), node_prefix.clone()].concat();
                    let prefix_str = format!("{:02X?}", full_prefix);
                    output.push_str(&format!("NodePtr at {}: ", prefix_str));

                    // Retrieve the serialized child node using the store's get method
                    let serialized_child_nodes = self.store.get(&vec![ptr.clone()])?;
                    if let Some(serialized_child_node) = serialized_child_nodes.first() {
                        if let Some(child_node_bytes) = serialized_child_node {
                            let child_node = bincode::deserialize::<Node>(
                                child_node_bytes.as_ref(),
                            )
                            .map_err(|_| {
                                RadixTreeError::SerializationError(
                                    "Radix Tree: Failed to deserialize".to_string(),
                                )
                            })?;
                            output.push_str(&self.print_tree(&child_node, full_prefix)?);
                        }
                    }
                }
            }
        }
        Ok(output)
    }
}

#[cfg(test)]
mod load_node_arc_tests {
    //! The missing-node contract. Before issue #923 Option D the assert in
    //! `load_node_arc` crashed the process when a sweep-vs-write race left
    //! a descendant pointer dangling. The typed `MissingNode` makes that
    //! path recoverable, so block-report replay, exploratory-deploy,
    //! get-data-at-par and merge can surface a precise "pruned subtree"
    //! answer to their own callers rather than disappear the node.

    use std::sync::Arc;

    use shared::rust::store::key_value_store::KeyValueStore;

    use super::*;
    use crate::rspace::errors::RadixTreeError;
    use crate::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

    fn empty_store() -> Arc<dyn KeyValueStore> { Arc::new(InMemoryKeyValueStore::new()) }

    #[test]
    fn missing_pointer_without_no_assert_returns_typed_error() {
        let imple = RadixTreeImpl::new(empty_store());
        let bogus = vec![0x42u8; 32];
        let err = imple
            .load_node_arc(bogus.clone(), None)
            .expect_err("a missing pointer must not resolve to an empty node when no_assert=false");
        match err {
            RadixTreeError::MissingNode { ptr } => {
                assert_eq!(
                    ptr.bytes(),
                    bogus,
                    "the error names the pointer that could not be resolved"
                );
            }
            other => panic!("expected MissingNode, got {:?}", other),
        }
    }

    #[test]
    fn missing_pointer_with_no_assert_returns_empty_node() {
        // The legacy escape hatch used by RadixHistory::create / ::reset:
        // a not-yet-imported root bootstraps to an empty tree instead of
        // raising.
        let imple = RadixTreeImpl::new(empty_store());
        let bogus = vec![0xABu8; 32];
        let node = imple
            .load_node_arc(bogus, Some(true))
            .expect("no_assert=true must tolerate a missing pointer");
        assert_eq!(node.as_ref(), &empty_node(), "no_assert=true must synthesize an empty node");
    }
}
