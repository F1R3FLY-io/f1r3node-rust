use std::mem::size_of;
use std::sync::Arc;

use shared::rust::ByteVector;
use shared::rust::collection_backing::hash_backing;

use super::{DEF_SIZE, HEAD_SIZE, Item, NUM_ITEMS, Node, RadixTreeImpl, empty_node};
use crate::rspace::errors::{RSpaceError, RadixTreeError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::native_source::SourceMeter;
use crate::rspace::history::history_action::{DeleteAction, HistoryAction, InsertAction};

pub(crate) struct NativeRadixBuilder<'a> {
    tree: &'a RadixTreeImpl,
    meter: &'a dyn SourceMeter,
}

impl<'a> NativeRadixBuilder<'a> {
    pub(crate) fn new(tree: &'a RadixTreeImpl, meter: &'a dyn SourceMeter) -> Self {
        Self { tree, meter }
    }

    fn invalid() -> RSpaceError {
        RadixTreeError::SerializationError("malformed native radix node".to_owned()).into()
    }

    fn prefix_error() -> RSpaceError {
        RadixTreeError::PrefixError(
            "The length of all prefixes in the subtree must be the same.".to_owned(),
        )
        .into()
    }

    fn checked_sum(left: usize, right: usize) -> Result<usize, RSpaceError> {
        left.checked_add(right).ok_or(RSpaceError::HostWorkRejected)
    }

    fn node_bytes(node: &Node) -> Result<usize, RSpaceError> {
        node.len()
            .checked_mul(size_of::<Item>())
            .ok_or(RSpaceError::HostWorkRejected)
    }

    fn item_bytes(item: &Item) -> Result<usize, RSpaceError> {
        match item {
            Item::EmptyItem => Ok(0),
            Item::Leaf { prefix, value } => Self::checked_sum(prefix.len(), value.len()),
            Item::NodePtr { prefix, ptr } => Self::checked_sum(prefix.len(), ptr.len()),
        }
    }

    fn scan_node(&self, node: &Node) -> Result<(), RSpaceError> {
        self.meter.reserve(node.len(), Self::node_bytes(node)?, 0)?;
        for item in node {
            self.meter.reserve(1, Self::item_bytes(item)?, 0)?;
        }
        Ok(())
    }

    fn copy_bytes(&self, bytes: &[u8]) -> Result<ByteVector, RSpaceError> {
        self.meter.reserve(1, bytes.len(), bytes.len())?;
        let mut copied = Vec::new();
        copied
            .try_reserve_exact(bytes.len())
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        copied.extend_from_slice(bytes);
        Ok(copied)
    }

    fn copy_item(&self, item: &Item) -> Result<Item, RSpaceError> {
        self.meter.reserve(1, size_of::<Item>(), 0)?;
        match item {
            Item::EmptyItem => Ok(Item::EmptyItem),
            Item::Leaf { prefix, value } => Ok(Item::Leaf {
                prefix: self.copy_bytes(prefix)?,
                value: self.copy_bytes(value)?,
            }),
            Item::NodePtr { prefix, ptr } => Ok(Item::NodePtr {
                prefix: self.copy_bytes(prefix)?,
                ptr: self.copy_bytes(ptr)?,
            }),
        }
    }

    fn copy_node(&self, node: &Node) -> Result<Node, RSpaceError> {
        self.scan_node(node)?;
        self.meter.reserve(1, 0, Self::node_bytes(node)?)?;
        let mut copied = Vec::new();
        copied
            .try_reserve_exact(node.len())
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        for item in node {
            copied.push(self.copy_item(item)?);
        }
        Ok(copied)
    }

    pub(crate) fn clone_root(&self, node: &Node) -> Result<Node, RSpaceError> {
        self.copy_node(node)
    }

    fn empty_node(&self) -> Result<Node, RSpaceError> {
        let backing = NUM_ITEMS
            .checked_mul(size_of::<Item>())
            .ok_or(RSpaceError::HostWorkRejected)?;
        self.meter.reserve(NUM_ITEMS, 0, backing)?;
        Ok(empty_node())
    }

    fn cache_node(&self, key: &ByteVector, node: Node) -> Result<Arc<Node>, RSpaceError> {
        self.meter.reserve(1, key.len(), 0)?;
        let index = self.tree.cache_r.determine_map(key);
        let count = self.tree.cache_r.shards()[index].read().len();
        let next = count.checked_add(1).ok_or(RSpaceError::HostWorkRejected)?;
        let (old_ops, old_backing) =
            hash_backing::<ByteVector, Arc<Node>>(count).ok_or(RSpaceError::HostWorkRejected)?;
        let (new_ops, new_backing) =
            hash_backing::<ByteVector, Arc<Node>>(next).ok_or(RSpaceError::HostWorkRejected)?;
        let backing = new_backing
            .checked_sub(old_backing)
            .and_then(|bytes| bytes.checked_add(key.len()))
            .and_then(|bytes| bytes.checked_add(size_of::<Node>() + 2 * size_of::<usize>()))
            .ok_or(RSpaceError::HostWorkRejected)?;
        self.meter.reserve(
            new_ops
                .checked_sub(old_ops)
                .and_then(|ops| ops.checked_add(2))
                .ok_or(RSpaceError::HostWorkRejected)?,
            key.len(),
            backing,
        )?;
        let cached = Arc::new(node);
        self.tree
            .cache_r
            .insert(self.copy_bytes(key)?, cached.clone());
        Ok(cached)
    }

    fn decode_node(&self, key: &[u8], bytes: &[u8]) -> Result<Node, RSpaceError> {
        self.meter.reserve(1, bytes.len(), DEF_SIZE)?;
        if Blake2b256Hash::new(bytes).0 != key {
            return Err(Self::invalid());
        }
        let mut node = self.empty_node()?;
        let mut cursor = 0usize;
        while cursor < bytes.len() {
            let header_end = Self::checked_sum(cursor, HEAD_SIZE)?;
            if header_end > bytes.len() {
                return Err(Self::invalid());
            }
            let index = bytes[cursor] as usize;
            let header = bytes[cursor + 1];
            let prefix_len = (header & 0x7f) as usize;
            let prefix_end = Self::checked_sum(header_end, prefix_len)?;
            let payload_end = Self::checked_sum(prefix_end, DEF_SIZE)?;
            if payload_end > bytes.len() || node[index] != Item::EmptyItem {
                return Err(Self::invalid());
            }
            self.meter.reserve(1, payload_end - cursor, 0)?;
            let prefix = self.copy_bytes(&bytes[header_end..prefix_end])?;
            let payload = self.copy_bytes(&bytes[prefix_end..payload_end])?;
            node[index] = if header & 0x80 == 0 {
                Item::Leaf {
                    prefix,
                    value: payload,
                }
            } else {
                Item::NodePtr {
                    prefix,
                    ptr: payload,
                }
            };
            cursor = payload_end;
        }
        Ok(node)
    }

    fn load_node(&self, key: &ByteVector) -> Result<Arc<Node>, RSpaceError> {
        if key.len() != DEF_SIZE {
            return Err(Self::invalid());
        }
        self.meter.reserve(1, key.len(), 0)?;
        if let Some(cached) = self.tree.cache_r.get(key) {
            return Ok(Arc::clone(cached.value()));
        }
        if let Some(encoded) = self.tree.cache_w.get(key) {
            let node = self.decode_node(key, encoded.value())?;
            drop(encoded);
            return self.cache_node(key, node);
        }
        self.meter.reserve(1, key.len(), 0)?;
        let mut outcome = None;
        self.tree.store.with_value(key, &mut |bytes| {
            outcome = Some(match bytes {
                Some(bytes) => self.decode_node(key, bytes),
                None => {
                    let diagnostic = key
                        .len()
                        .checked_mul(2)
                        .ok_or(RSpaceError::HostWorkRejected);
                    diagnostic
                        .and_then(|diagnostic| self.meter.reserve(1, key.len(), diagnostic))
                        .and_then(|()| Err(RadixTreeError::KeyNotFound(hex::encode(key)).into()))
                }
            });
            Ok(())
        })?;
        self.cache_node(key, outcome.ok_or_else(Self::invalid)??)
    }

    fn common_length(&self, left: &[u8], right: &[u8]) -> Result<usize, RSpaceError> {
        self.meter.reserve(1, left.len().min(right.len()), 0)?;
        Ok(left.iter().zip(right).take_while(|(a, b)| a == b).count())
    }

    fn joined_prefix(
        &self,
        parent: &[u8],
        index: u8,
        child: &[u8],
    ) -> Result<ByteVector, RSpaceError> {
        let length = Self::checked_sum(Self::checked_sum(parent.len(), 1)?, child.len())?;
        self.meter.reserve(1, parent.len() + child.len(), length)?;
        let mut joined = Vec::new();
        joined
            .try_reserve_exact(length)
            .map_err(|_| RSpaceError::HostWorkRejected)?;
        joined.extend_from_slice(parent);
        joined.push(index);
        joined.extend_from_slice(child);
        Ok(joined)
    }

    fn save_node(&self, node: Node) -> Result<ByteVector, RSpaceError> {
        let key = self.tree.save_native_root(&node, self.meter)?;
        self.meter.reserve(1, key.len(), 0)?;
        if !self.tree.cache_r.contains_key(&key) {
            self.cache_node(&key, node)?;
        }
        Ok(key)
    }

    fn save_item(
        &self,
        node: Node,
        prefix: ByteVector,
        compact: bool,
    ) -> Result<Item, RSpaceError> {
        if compact {
            self.scan_node(&node)?;
            let mut nonempty = node
                .iter()
                .enumerate()
                .filter(|(_, item)| **item != Item::EmptyItem);
            match (nonempty.next(), nonempty.next()) {
                (None, _) => return Ok(Item::EmptyItem),
                (
                    Some((
                        index,
                        Item::Leaf {
                            prefix: child,
                            value,
                        },
                    )),
                    None,
                ) => {
                    return Ok(Item::Leaf {
                        prefix: self.joined_prefix(&prefix, index as u8, child)?,
                        value: self.copy_bytes(value)?,
                    });
                }
                (Some((index, Item::NodePtr { prefix: child, ptr })), None) => {
                    return Ok(Item::NodePtr {
                        prefix: self.joined_prefix(&prefix, index as u8, child)?,
                        ptr: self.copy_bytes(ptr)?,
                    });
                }
                (Some(_), Some(_)) => {}
                (Some((_, Item::EmptyItem)), None) => unreachable!(),
            }
        }
        Ok(Item::NodePtr {
            prefix,
            ptr: self.save_node(node)?,
        })
    }

    fn construct_node(&self, item: &Item) -> Result<Arc<Node>, RSpaceError> {
        if let Item::NodePtr { prefix, ptr } = item {
            if prefix.is_empty() {
                return self.load_node(ptr);
            }
        }
        let mut node = self.empty_node()?;
        match item {
            Item::EmptyItem => {}
            Item::Leaf { prefix, value } => {
                let (first, tail) = prefix.split_first().ok_or_else(Self::prefix_error)?;
                node[*first as usize] = Item::Leaf {
                    prefix: self.copy_bytes(tail)?,
                    value: self.copy_bytes(value)?,
                };
            }
            Item::NodePtr { prefix, ptr } => {
                let (first, tail) = prefix.split_first().ok_or_else(Self::prefix_error)?;
                node[*first as usize] = Item::NodePtr {
                    prefix: self.copy_bytes(tail)?,
                    ptr: self.copy_bytes(ptr)?,
                };
            }
        }
        self.meter
            .reserve(1, 0, size_of::<Node>() + 2 * size_of::<usize>())?;
        Ok(Arc::new(node))
    }

    fn update(
        &self,
        item: &Item,
        prefix: &[u8],
        value: &[u8],
    ) -> Result<Option<Item>, RSpaceError> {
        match item {
            Item::EmptyItem => Ok(Some(Item::Leaf {
                prefix: self.copy_bytes(prefix)?,
                value: self.copy_bytes(value)?,
            })),
            Item::Leaf {
                prefix: old,
                value: previous,
            } => {
                if old.len() != prefix.len() {
                    return Err(Self::prefix_error());
                }
                self.meter.reserve(1, old.len(), 0)?;
                if old == prefix {
                    self.meter.reserve(1, value.len(), 0)?;
                    if previous == value {
                        return Ok(None);
                    }
                    return Ok(Some(Item::Leaf {
                        prefix: self.copy_bytes(prefix)?,
                        value: self.copy_bytes(value)?,
                    }));
                }
                let common = self.common_length(old, prefix)?;
                let mut split = self.empty_node()?;
                let (old_index, old_tail) =
                    old[common..].split_first().ok_or_else(Self::prefix_error)?;
                let (new_index, new_tail) = prefix[common..]
                    .split_first()
                    .ok_or_else(Self::prefix_error)?;
                split[*old_index as usize] = Item::Leaf {
                    prefix: self.copy_bytes(old_tail)?,
                    value: self.copy_bytes(previous)?,
                };
                split[*new_index as usize] = Item::Leaf {
                    prefix: self.copy_bytes(new_tail)?,
                    value: self.copy_bytes(value)?,
                };
                Ok(Some(self.save_item(split, self.copy_bytes(&prefix[..common])?, false)?))
            }
            Item::NodePtr { prefix: old, ptr } => {
                if old.len() >= prefix.len() {
                    return Err(Self::prefix_error());
                }
                let common = self.common_length(old, prefix)?;
                if common == old.len() {
                    let (index, tail) = prefix[common..]
                        .split_first()
                        .ok_or_else(Self::prefix_error)?;
                    let child = self.load_node(ptr)?;
                    let Some(updated) = self.update(&child[*index as usize], tail, value)? else {
                        return Ok(None);
                    };
                    let mut copy = self.copy_node(&child)?;
                    copy[*index as usize] = updated;
                    Ok(Some(self.save_item(copy, self.copy_bytes(&old[..common])?, false)?))
                } else {
                    let mut split = self.empty_node()?;
                    let (old_index, old_tail) =
                        old[common..].split_first().ok_or_else(Self::prefix_error)?;
                    let (new_index, new_tail) = prefix[common..]
                        .split_first()
                        .ok_or_else(Self::prefix_error)?;
                    split[*old_index as usize] = Item::NodePtr {
                        prefix: self.copy_bytes(old_tail)?,
                        ptr: self.copy_bytes(ptr)?,
                    };
                    split[*new_index as usize] = Item::Leaf {
                        prefix: self.copy_bytes(new_tail)?,
                        value: self.copy_bytes(value)?,
                    };
                    Ok(Some(self.save_item(split, self.copy_bytes(&prefix[..common])?, false)?))
                }
            }
        }
    }

    fn delete(&self, item: &Item, prefix: &[u8]) -> Result<Option<Item>, RSpaceError> {
        match item {
            Item::EmptyItem => Ok(None),
            Item::Leaf { prefix: old, .. } => {
                self.meter.reserve(1, old.len().min(prefix.len()), 0)?;
                Ok((old == prefix).then_some(Item::EmptyItem))
            }
            Item::NodePtr { prefix: old, ptr } => {
                let common = self.common_length(old, prefix)?;
                if common != old.len() || common == prefix.len() {
                    return Ok(None);
                }
                let (index, tail) = prefix[common..]
                    .split_first()
                    .ok_or_else(Self::prefix_error)?;
                let child = self.load_node(ptr)?;
                let Some(updated) = self.delete(&child[*index as usize], tail)? else {
                    return Ok(None);
                };
                let mut copy = self.copy_node(&child)?;
                copy[*index as usize] = updated;
                Ok(Some(self.save_item(copy, self.copy_bytes(&old[..common])?, true)?))
            }
        }
    }

    pub(crate) fn make_actions(
        &self,
        root: &Node,
        actions: Vec<HistoryAction>,
    ) -> Result<Option<Node>, RSpaceError> {
        if actions.is_empty() {
            return Ok(None);
        }
        let mut counts = [0usize; NUM_ITEMS];
        self.meter.reserve(actions.len(), actions.len(), 0)?;
        for action in &actions {
            let key = match action {
                HistoryAction::Insert(insert) => &insert.key,
                HistoryAction::Delete(delete) => &delete.key,
            };
            let index = *key.first().ok_or_else(Self::prefix_error)? as usize;
            counts[index] = counts[index]
                .checked_add(1)
                .ok_or(RSpaceError::HostWorkRejected)?;
        }
        let mut groups: [Vec<HistoryAction>; NUM_ITEMS] = std::array::from_fn(|_| Vec::new());
        for (group, count) in groups.iter_mut().zip(counts) {
            if count != 0 {
                let backing = count
                    .checked_mul(size_of::<HistoryAction>())
                    .ok_or(RSpaceError::HostWorkRejected)?;
                self.meter.reserve(1, 0, backing)?;
                group
                    .try_reserve_exact(count)
                    .map_err(|_| RSpaceError::HostWorkRejected)?;
            }
        }
        for action in actions {
            let key = match &action {
                HistoryAction::Insert(insert) => &insert.key,
                HistoryAction::Delete(delete) => &delete.key,
            };
            groups[key[0] as usize].push(action);
        }

        let mut changed = None;
        for (index, mut group) in groups.into_iter().enumerate() {
            if group.is_empty() {
                continue;
            }
            let item = &root[index];
            let only_deletes = if *item == Item::EmptyItem && group.len() > 1 {
                self.meter.reserve(group.len(), group.len(), 0)?;
                group
                    .iter()
                    .all(|action| matches!(action, HistoryAction::Delete(_)))
            } else {
                false
            };
            let updated = if group.len() == 1 {
                match &group[0] {
                    HistoryAction::Insert(InsertAction { key, hash }) => {
                        self.update(item, &key[1..], &hash.0)?
                    }
                    HistoryAction::Delete(DeleteAction { key }) => self.delete(item, &key[1..])?,
                }
            } else if only_deletes {
                None
            } else {
                let child = self.construct_node(item)?;
                for action in &mut group {
                    let key = match action {
                        HistoryAction::Insert(insert) => &mut insert.key,
                        HistoryAction::Delete(delete) => &mut delete.key,
                    };
                    self.meter.reserve(1, key.len(), 0)?;
                    key.remove(0);
                }
                self.make_actions(&child, group)?
                    .map(|next| self.save_item(next, Vec::new(), true))
                    .transpose()?
            };
            if let Some(updated) = updated {
                self.meter.reserve(1, Self::item_bytes(item)?, 0)?;
                if updated != *item {
                    if changed.is_none() {
                        changed = Some(self.copy_node(root)?);
                    }
                    changed.as_mut().expect("node allocated above")[index] = updated;
                }
            }
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::Arc;

    use shared::rust::store::key_value_store::KeyValueStore;

    use super::*;
    use crate::rspace::history::native_reader::measure_allocations;
    use crate::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;

    fn action(index: usize, round: usize) -> HistoryAction {
        let key = vec![9, (index / 16) as u8, (index % 16) as u8, (index / 3) as u8, 7];
        if (index + round).is_multiple_of(4) {
            HistoryAction::Delete(DeleteAction { key })
        } else {
            HistoryAction::Insert(InsertAction {
                key,
                hash: Blake2b256Hash::new(&[index as u8, round as u8]),
            })
        }
    }

    #[test]
    fn native_radix_preparation_prepays_requested_allocation() {
        for count in [1, 2, 6, 7, 32] {
            let store = Arc::new(InMemoryKeyValueStore::new());
            let tree = RadixTreeImpl::new(store);
            let backing = Cell::new(0usize);
            let meter = |_, _, bytes| {
                backing.set(
                    backing
                        .get()
                        .checked_add(bytes)
                        .ok_or(RSpaceError::HostWorkRejected)?,
                );
                Ok(())
            };
            let builder = NativeRadixBuilder::new(&tree, &meter);
            let actions = if count == 7 {
                (0..count)
                    .map(|index| {
                        let mut key = vec![(index % 3) as u8];
                        key.extend(Blake2b256Hash::new(&[index as u8]).0);
                        if index > 3 {
                            HistoryAction::Delete(DeleteAction { key })
                        } else {
                            HistoryAction::Insert(InsertAction {
                                key,
                                hash: Blake2b256Hash::new(&[index as u8, 9]),
                            })
                        }
                    })
                    .collect()
            } else {
                (0..count).map(|index| action(index, 1)).collect()
            };
            let original = empty_node();
            let (next, built) = measure_allocations(|| builder.make_actions(&original, actions));
            let next = next.unwrap().unwrap();
            let built_paid = backing.get();
            let (root, saved) = measure_allocations(|| tree.save_native_root(&next, &meter));
            root.unwrap();
            let saved_paid = backing.get() - built_paid;
            let (writes, collected) = measure_allocations(|| tree.prepare_native_commit(&meter));
            writes.unwrap();
            let collected_paid = backing.get() - built_paid - saved_paid;
            assert!(
                built + saved + collected <= built_paid + saved_paid + collected_paid,
                "count={count}, build={built}/{built_paid}, save={saved}/{saved_paid}, \
                 collect={collected}/{collected_paid}"
            );
        }
    }

    #[test]
    fn native_and_legacy_radix_roots_match_across_splits_deletes_and_reloads() {
        let legacy_store = Arc::new(InMemoryKeyValueStore::new());
        let native_store = Arc::new(InMemoryKeyValueStore::new());
        let mut legacy_root = empty_node();
        let mut native_root = empty_node();
        for round in 0..12 {
            let actions = (0..32)
                .map(|offset| action((offset * 13 + round * 7) % 71, round))
                .collect::<Vec<_>>();
            let legacy = RadixTreeImpl::new(legacy_store.clone());
            if let Some(next) = legacy.make_actions(&legacy_root, actions.clone()).unwrap() {
                legacy.save_node(next.clone());
                legacy_store.put(legacy.prepare_commit().unwrap()).unwrap();
                legacy_root = next;
            }
            let native = RadixTreeImpl::new(native_store.clone());
            let builder = NativeRadixBuilder::new(&native, &|_, _, _| Ok(()));
            if let Some(next) = builder.make_actions(&native_root, actions).unwrap() {
                native.save_native_root(&next, &|_, _, _| Ok(())).unwrap();
                native_store
                    .put(native.prepare_native_commit(&|_, _, _| Ok(())).unwrap())
                    .unwrap();
                native_root = next;
            }
            assert_eq!(
                super::super::hash_node(&native_root).0,
                super::super::hash_node(&legacy_root).0,
                "round={round}"
            );
            assert_eq!(native_root, legacy_root, "round={round}");
        }
    }

    #[test]
    fn native_borrowed_loader_rejects_malformed_records() {
        let store = Arc::new(InMemoryKeyValueStore::new());
        let tree = RadixTreeImpl::new(store.clone());
        let builder = NativeRadixBuilder::new(&tree, &|_, _, _| Ok(()));
        let malformed = [vec![3, 1, 9], {
            let mut duplicate = vec![3, 0];
            duplicate.extend([1; DEF_SIZE]);
            duplicate.extend([3, 0]);
            duplicate.extend([2; DEF_SIZE]);
            duplicate
        }];
        for bytes in malformed {
            let key = Blake2b256Hash::new(&bytes).0;
            store.put_one(key.clone(), bytes).unwrap();
            assert!(matches!(
                builder.load_node(&key),
                Err(RSpaceError::RadixTreeError(RadixTreeError::SerializationError(_)))
            ));
        }
    }
}
