use std::cmp::Ordering;
use std::mem::size_of;
use std::sync::Arc;

use models::rust::host_work::HostWorkDimension;

use super::recording::work;
use super::{HostWorkBudget, InterpreterError};
use crate::rust::interpreter::accounting::native_phlo_rules::{
    NativeAttemptStage, NativeBudgetOccurrence,
};

pub(super) trait IndexKey: Ord {
    fn comparison_work(&self) -> (usize, usize);
}

impl IndexKey for Arc<[u8]> {
    fn comparison_work(&self) -> (usize, usize) { (1, self.len()) }
}

impl IndexKey for NativeBudgetOccurrence {
    fn comparison_work(&self) -> (usize, usize) {
        (
            self.path.len().saturating_mul(2).saturating_add(4),
            self.path.len().saturating_mul(16).saturating_add(33),
        )
    }
}

impl IndexKey for (NativeAttemptStage, [u8; 32]) {
    fn comparison_work(&self) -> (usize, usize) { (2, 33) }
}

pub(super) fn height_bound(nodes: usize) -> usize {
    let bits = usize::BITS as usize;
    2 * nodes
        .checked_add(1)
        .map_or(bits + 1, |n| bits - n.leading_zeros() as usize)
}

pub(super) fn reserve_lookup(
    bound: (usize, usize),
    nodes: usize,
    budget: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    let comparisons = height_bound(nodes);
    work(
        budget,
        HostWorkDimension::VerificationOperations,
        comparisons
            .checked_mul(
                bound
                    .0
                    .checked_add(2)
                    .ok_or(InterpreterError::HostWorkRejected)?,
            )
            .ok_or(InterpreterError::HostWorkRejected)?,
    )?;
    work(
        budget,
        HostWorkDimension::VerificationBytes,
        comparisons
            .checked_mul(bound.1)
            .ok_or(InterpreterError::HostWorkRejected)?,
    )
}

pub(in crate::rust::interpreter::accounting) fn reserve_vector<T>(
    values: &mut Vec<T>,
    logical_capacity: &mut usize,
    additional: usize,
    budget: &HostWorkBudget,
) -> Result<(), InterpreterError> {
    let required = values
        .len()
        .checked_add(additional)
        .ok_or(InterpreterError::HostWorkRejected)?;
    if required <= *logical_capacity {
        return Ok(());
    }
    let capacity = logical_capacity
        .checked_mul(2)
        .ok_or(InterpreterError::HostWorkRejected)?
        .max(required)
        .max(4);
    let bytes = capacity
        .checked_mul(size_of::<T>())
        .ok_or(InterpreterError::HostWorkRejected)?;
    let moved = values
        .len()
        .checked_mul(size_of::<T>())
        .ok_or(InterpreterError::HostWorkRejected)?;
    work(budget, HostWorkDimension::SearchStateBytes, bytes)?;
    work(budget, HostWorkDimension::VerificationBytes, moved)?;
    let mut replacement = Vec::new();
    replacement
        .try_reserve_exact(capacity)
        .map_err(|_| InterpreterError::HostWorkRejected)?;
    replacement.append(values);
    *values = replacement;
    *logical_capacity = capacity;
    Ok(())
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
    logical_capacity: usize,
    root: Option<usize>,
    revision: usize,
}

impl<K, V> Default for NativeIndex<K, V> {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            logical_capacity: 0,
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

impl<K: IndexKey, V> NativeIndex<K, V> {
    pub(super) fn len(&self) -> usize { self.nodes.len() }

    #[cfg(test)]
    pub(super) fn capacity(&self) -> usize { self.logical_capacity }

    pub(super) fn keys(&self) -> impl Iterator<Item = &K> {
        self.nodes.iter().map(|node| &node.key)
    }

    fn locate(
        &self,
        key: &K,
        mut compare: impl FnMut(&K, &K) -> Result<Ordering, InterpreterError>,
    ) -> Result<Location, InterpreterError> {
        let mut current = self.root;
        let mut parent = None;
        let mut left = false;
        while let Some(index) = current {
            let node = &self.nodes[index];
            match compare(key, &node.key)? {
                Ordering::Equal => return Ok(Location::Existing(index)),
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
        Ok(Location::Vacant { parent, left })
    }

    fn locate_metered(
        &self,
        key: &K,
        budget: &HostWorkBudget,
    ) -> Result<Location, InterpreterError> {
        let (operations, bytes) = key.comparison_work();
        self.locate(key, |a, b| {
            work(
                budget,
                HostWorkDimension::VerificationOperations,
                operations.saturating_add(2),
            )?;
            work(budget, HostWorkDimension::VerificationBytes, bytes)?;
            Ok(a.cmp(b))
        })
    }

    pub(super) fn get(
        &self,
        key: &K,
        budget: &HostWorkBudget,
    ) -> Result<Option<&V>, InterpreterError> {
        Ok(match self.locate_metered(key, budget)? {
            Location::Existing(index) => Some(&self.nodes[index].value),
            Location::Vacant { .. } => None,
        })
    }

    pub(super) fn get_prepaid(&self, key: &K) -> Option<&V> {
        match self
            .locate(key, |a, b| Ok(a.cmp(b)))
            .expect("prepaid comparison is infallible")
        {
            Location::Existing(index) => Some(&self.nodes[index].value),
            Location::Vacant { .. } => None,
        }
    }

    fn reserve_repair(
        nodes: usize,
        count: usize,
        budget: &HostWorkBudget,
    ) -> Result<(), InterpreterError> {
        let work_bound = height_bound(nodes)
            .checked_add(1)
            .and_then(|height| height.checked_mul(256))
            .and_then(|single| single.checked_mul(count))
            .ok_or(InterpreterError::HostWorkRejected)?;
        work(
            budget,
            HostWorkDimension::VerificationOperations,
            work_bound,
        )
    }

    pub(super) fn prepare_insert(
        &mut self,
        key: K,
        value: V,
        budget: &HostWorkBudget,
    ) -> Result<PreparedInsert<K, V>, InterpreterError> {
        self.revision
            .checked_add(1)
            .ok_or(InterpreterError::HostWorkRejected)?;
        let location = self.locate_metered(&key, budget)?;
        if matches!(location, Location::Vacant { .. }) {
            Self::reserve_repair(self.len(), 1, budget)?;
            reserve_vector(&mut self.nodes, &mut self.logical_capacity, 1, budget)?;
        }
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
        budget: &HostWorkBudget,
    ) -> Result<PreparedBatch<K, V>, InterpreterError> {
        self.revision
            .checked_add(updates.len())
            .ok_or(InterpreterError::HostWorkRejected)?;
        let final_size = self
            .len()
            .checked_add(updates.len())
            .ok_or(InterpreterError::HostWorkRejected)?;
        for (key, _) in &updates {
            reserve_lookup(key.comparison_work(), final_size, budget)?;
        }
        Self::reserve_repair(final_size, updates.len(), budget)?;
        reserve_vector(
            &mut self.nodes,
            &mut self.logical_capacity,
            updates.len(),
            budget,
        )?;
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
        for (key, value) in prepared.updates {
            let location = self
                .locate(&key, |a, b| Ok(a.cmp(b)))
                .expect("prepaid comparison is infallible");
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
                assert!(
                    self.nodes.len() < self.logical_capacity,
                    "native index insertion lacks reserved storage"
                );
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
