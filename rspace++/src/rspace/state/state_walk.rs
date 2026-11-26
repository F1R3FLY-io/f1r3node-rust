use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::rspace::errors::RadixTreeError;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::radix_tree::{Item, Node, try_decode};
use crate::rspace::shared::trie_exporter::NodePath;
use crate::rspace::state::rspace_exporter::node_cursor;
use crate::rspace::state::rspace_importer::RSpaceImporter;

pub const WALK_PAGE_SIZE: i32 = 2;

const DATA_LOOKUP_BATCH: usize = 1024;

struct LeafRef {
    node: Blake2b256Hash,
    slot: usize,
    node_kind: Option<u8>,
    data: Blake2b256Hash,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MissingData {
    pub path: NodePath,
    pub kind: Option<u8>,
    pub data: HashSet<Blake2b256Hash>,
}

pub struct StateWalk {
    importer: Arc<dyn RSpaceImporter>,
    visited: HashSet<(Blake2b256Hash, Option<u8>)>,
    waiting: HashMap<Blake2b256Hash, HashSet<Option<u8>>>,
    leaves_seen: HashSet<(Blake2b256Hash, u8)>,
    unchecked: Vec<LeafRef>,
}

impl StateWalk {
    pub fn new(importer: Arc<dyn RSpaceImporter>) -> Self {
        Self {
            importer,
            visited: HashSet::new(),
            waiting: HashMap::new(),
            leaves_seen: HashSet::new(),
            unchecked: Vec::new(),
        }
    }

    pub fn add_root(
        &mut self,
        root: &Blake2b256Hash,
    ) -> Result<Vec<Blake2b256Hash>, RadixTreeError> {
        self.walk(vec![(root.clone(), None)])
    }

    pub fn leaf_kind(&self, node: &Blake2b256Hash) -> Option<u8> {
        self.waiting
            .get(node)
            .and_then(|kinds| kinds.iter().flatten().next().copied())
    }

    pub fn node_imported(
        &mut self,
        node: &Blake2b256Hash,
    ) -> Result<Vec<Blake2b256Hash>, RadixTreeError> {
        let Some(kinds) = self.waiting.remove(node) else {
            return Ok(Vec::new());
        };
        let entries = kinds
            .into_iter()
            .map(|kind| {
                self.visited.remove(&(node.clone(), kind));
                (node.clone(), kind)
            })
            .collect();
        let missing = self.walk(entries)?;
        if missing.contains(node) {
            return Err(node_not_found(node));
        }
        Ok(missing)
    }

    pub fn take_missing_data(&mut self) -> Result<Vec<MissingData>, RadixTreeError> {
        let unchecked = std::mem::take(&mut self.unchecked);
        let mut missing: HashMap<NodePath, MissingData> = HashMap::new();
        for batch in unchecked.chunks(DATA_LOOKUP_BATCH) {
            let hashes: Vec<Blake2b256Hash> = batch.iter().map(|leaf| leaf.data.clone()).collect();
            let held = self.importer.has_data_items(&hashes);
            for (leaf, _) in batch.iter().zip(held).filter(|(_, held)| !held) {
                let path = self.page_with_leaf(&leaf.node, leaf.slot)?;
                missing
                    .entry(path.clone())
                    .or_insert_with(|| MissingData {
                        path,
                        kind: leaf.node_kind,
                        data: HashSet::new(),
                    })
                    .data
                    .insert(leaf.data.clone());
            }
        }
        Ok(missing.into_values().collect())
    }

    fn walk(
        &mut self,
        mut stack: Vec<(Blake2b256Hash, Option<u8>)>,
    ) -> Result<Vec<Blake2b256Hash>, RadixTreeError> {
        let mut missing = Vec::new();
        while let Some((hash, kind)) = stack.pop() {
            if !self.visited.insert((hash.clone(), kind)) {
                continue;
            }
            let Some(bytes) = self.importer.get_history_item(hash.clone()) else {
                let kinds = self.waiting.entry(hash.clone()).or_default();
                if kinds.is_empty() {
                    missing.push(hash);
                }
                kinds.insert(kind);
                continue;
            };
            for (slot, item) in try_decode(&bytes)?.into_iter().enumerate() {
                let item_kind = kind.unwrap_or(slot as u8);
                match item {
                    Item::EmptyItem => {}
                    Item::NodePtr { ptr, .. } => {
                        stack.push((Blake2b256Hash::from_bytes(ptr), Some(item_kind)));
                    }
                    Item::Leaf { value, .. } => {
                        let data = Blake2b256Hash::from_bytes(value);
                        if self.leaves_seen.insert((data.clone(), item_kind)) {
                            self.unchecked.push(LeafRef {
                                node: hash.clone(),
                                slot,
                                node_kind: kind,
                                data,
                            });
                        }
                    }
                }
            }
        }
        Ok(missing)
    }

    fn page_with_leaf(
        &self,
        node: &Blake2b256Hash,
        slot: usize,
    ) -> Result<NodePath, RadixTreeError> {
        let decoded = self.load(node)?;
        let Some(child) = last_child_before(&decoded, slot) else {
            return Ok(vec![(node.clone(), None)]);
        };

        let mut parent = (Vec::new(), decoded);
        let mut child_slot = child;
        let last_node = loop {
            let (prefix, decoded) = self.descend(&parent.0, &parent.1, child_slot)?;
            match last_child_before(&decoded, decoded.len()) {
                Some(next) => {
                    parent = (prefix, decoded);
                    child_slot = next;
                }
                None => break (parent, child_slot),
            }
        };

        let ((mut prefix, mut decoded), last_slot) = last_node;
        let mut before = last_slot;
        while let Some(previous) = last_child_before(&decoded, before) {
            (prefix, decoded) = self.descend(&prefix, &decoded, previous)?;
            before = decoded.len();
        }
        Ok(node_cursor(node.clone(), &prefix))
    }

    fn descend(
        &self,
        prefix: &[u8],
        node: &Node,
        slot: usize,
    ) -> Result<(Vec<u8>, Node), RadixTreeError> {
        let Item::NodePtr {
            prefix: item_prefix,
            ptr,
        } = &node[slot]
        else {
            return Err(RadixTreeError::PrefixError(format!(
                "State walk: no child node in slot {slot}"
            )));
        };
        let mut child_prefix = prefix.to_vec();
        child_prefix.push(slot as u8);
        child_prefix.extend_from_slice(item_prefix);
        Ok((child_prefix, self.load(&Blake2b256Hash::from_bytes(ptr.clone()))?))
    }

    fn load(&self, node: &Blake2b256Hash) -> Result<Node, RadixTreeError> {
        let bytes = self
            .importer
            .get_history_item(node.clone())
            .ok_or_else(|| node_not_found(node))?;
        try_decode(&bytes)
    }
}

fn node_not_found(node: &Blake2b256Hash) -> RadixTreeError {
    RadixTreeError::KeyNotFound(format!("State walk: node {node} not found"))
}

fn last_child_before(node: &Node, slot: usize) -> Option<usize> {
    node[..slot]
        .iter()
        .rposition(|item| matches!(item, Item::NodePtr { .. }))
}
