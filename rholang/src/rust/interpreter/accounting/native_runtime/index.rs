use std::cmp::Ordering;

use rspace_plus_plus::rspace::operation_context::PathDigest;

use super::recording::recording_error;
use super::InterpreterError;
use crate::rust::interpreter::accounting::native_phlo_rules::NativeAttemptStage;

/// The key of the recording occurrence index (C7a, DR-85): the session, the
/// chained digest of the path, the path depth, the last segment and the
/// stage. Two occurrences have equal keys exactly when their paths are
/// equal, unless Blake2b-256 collides. A comparison reads a fixed number of
/// bytes, whatever the path depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct OccurrenceKey {
    pub(super) session: [u8; 32],
    pub(super) digest: PathDigest,
    pub(super) depth: usize,
    pub(super) last: Option<(u64, u64)>,
    pub(super) stage: NativeAttemptStage,
}

struct Node<K, V> {
    key: K,
    value: V,
    parent: Option<usize>,
    left: Option<usize>,
    right: Option<usize>,
    height: usize,
}

pub(super) struct NativeIndex<K, V> {
    nodes: Vec<Node<K, V>>,
    root: Option<usize>,
    revision: usize,
}

impl<K, V> Default for NativeIndex<K, V> {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            root: None,
            revision: 0,
        }
    }
}

#[derive(Clone, Copy)]
enum Location {
    Existing(usize),
    Vacant { parent: Option<usize>, left: bool },
}

pub(super) struct PreparedInsert<K, V> {
    revision: usize,
    location: Location,
    key: K,
    value: V,
}

pub(super) struct PreparedBatch<K, V> {
    revision: usize,
    updates: Vec<(K, V)>,
}

impl<K: Ord, V> NativeIndex<K, V> {
    pub(super) fn len(&self) -> usize { self.nodes.len() }

    // Since C7b (DR-86) only tests read the keys in insertion order.
    #[cfg(test)]
    pub(super) fn keys(&self) -> impl Iterator<Item = &K> {
        self.nodes.iter().map(|node| &node.key)
    }

    fn locate(&self, key: &K) -> Location {
        let mut current = self.root;
        let mut parent = None;
        let mut left = false;
        while let Some(index) = current {
            let node = &self.nodes[index];
            match key.cmp(&node.key) {
                Ordering::Equal => return Location::Existing(index),
                Ordering::Less => {
                    current = node.left;
                    left = true;
                }
                Ordering::Greater => {
                    current = node.right;
                    left = false;
                }
            }
            parent = Some(index);
        }
        Location::Vacant { parent, left }
    }

    pub(super) fn get(&self, key: &K) -> Option<&V> {
        match self.locate(key) {
            Location::Existing(index) => Some(&self.nodes[index].value),
            Location::Vacant { .. } => None,
        }
    }

    pub(super) fn prepare_insert(
        &mut self,
        key: K,
        value: V,
    ) -> Result<PreparedInsert<K, V>, InterpreterError> {
        self.revision
            .checked_add(1)
            .ok_or_else(|| recording_error("native index revision overflow"))?;
        let location = self.locate(&key);
        Ok(PreparedInsert {
            revision: self.revision,
            location,
            key,
            value,
        })
    }

    pub(super) fn commit(&mut self, prepared: PreparedInsert<K, V>) -> Option<V> {
        assert_eq!(
            prepared.revision, self.revision,
            "native index preparation is stale"
        );
        self.publish(prepared.location, prepared.key, prepared.value)
    }

    pub(super) fn prepare_batch(
        &mut self,
        updates: Vec<(K, V)>,
    ) -> Result<PreparedBatch<K, V>, InterpreterError> {
        self.revision
            .checked_add(updates.len())
            .ok_or_else(|| recording_error("native index revision overflow"))?;
        Ok(PreparedBatch {
            revision: self.revision,
            updates,
        })
    }

    pub(super) fn commit_batch(&mut self, prepared: PreparedBatch<K, V>) {
        assert_eq!(
            prepared.revision, self.revision,
            "native index preparation is stale"
        );
        self.nodes.reserve(prepared.updates.len());
        for (key, value) in prepared.updates {
            let location = self.locate(&key);
            self.publish(location, key, value);
        }
    }

    fn height(&self, index: Option<usize>) -> usize { index.map_or(0, |i| self.nodes[i].height) }

    fn refresh(&mut self, index: usize) {
        self.nodes[index].height = 1 + self
            .height(self.nodes[index].left)
            .max(self.height(self.nodes[index].right));
    }

    fn replace_root(&mut self, old: usize, new: usize) {
        let parent = self.nodes[old].parent;
        self.nodes[new].parent = parent;
        if let Some(parent) = parent {
            if self.nodes[parent].left == Some(old) {
                self.nodes[parent].left = Some(new);
            } else {
                self.nodes[parent].right = Some(new);
            }
        } else {
            self.root = Some(new);
        }
    }

    fn rotate_left(&mut self, root: usize) -> usize {
        let right = self.nodes[root]
            .right
            .expect("AVL left rotation requires right child");
        let middle = self.nodes[right].left;
        self.replace_root(root, right);
        self.nodes[right].left = Some(root);
        self.nodes[root].parent = Some(right);
        self.nodes[root].right = middle;
        if let Some(middle) = middle {
            self.nodes[middle].parent = Some(root);
        }
        self.refresh(root);
        self.refresh(right);
        right
    }

    fn rotate_right(&mut self, root: usize) -> usize {
        let left = self.nodes[root]
            .left
            .expect("AVL right rotation requires left child");
        let middle = self.nodes[left].right;
        self.replace_root(root, left);
        self.nodes[left].right = Some(root);
        self.nodes[root].parent = Some(left);
        self.nodes[root].left = middle;
        if let Some(middle) = middle {
            self.nodes[middle].parent = Some(root);
        }
        self.refresh(root);
        self.refresh(left);
        left
    }

    fn repair(&mut self, mut current: Option<usize>) {
        while let Some(index) = current {
            self.refresh(index);
            let left_height = self.height(self.nodes[index].left);
            let right_height = self.height(self.nodes[index].right);
            let root = if left_height > right_height + 1 {
                let left = self.nodes[index].left.unwrap();
                if self.height(self.nodes[left].right) > self.height(self.nodes[left].left) {
                    self.rotate_left(left);
                }
                self.rotate_right(index)
            } else if right_height > left_height + 1 {
                let right = self.nodes[index].right.unwrap();
                if self.height(self.nodes[right].left) > self.height(self.nodes[right].right) {
                    self.rotate_right(right);
                }
                self.rotate_left(index)
            } else {
                index
            };
            current = self.nodes[root].parent;
        }
    }

    fn publish(&mut self, location: Location, key: K, value: V) -> Option<V> {
        self.revision += 1;
        match location {
            Location::Existing(index) => {
                Some(std::mem::replace(&mut self.nodes[index].value, value))
            }
            Location::Vacant { parent, left } => {
                let index = self.nodes.len();
                self.nodes.push(Node {
                    key,
                    value,
                    parent,
                    left: None,
                    right: None,
                    height: 1,
                });
                if let Some(parent) = parent {
                    if left {
                        self.nodes[parent].left = Some(index);
                    } else {
                        self.nodes[parent].right = Some(index);
                    }
                } else {
                    self.root = Some(index);
                }
                self.repair(parent);
                None
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/index.rs"]
mod tests;
