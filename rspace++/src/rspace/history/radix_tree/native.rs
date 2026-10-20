use std::sync::Arc;

use shared::rust::ByteVector;

use super::{DEF_SIZE, HEAD_SIZE, Item, NUM_ITEMS, Node, RadixTreeImpl, empty_node};
use crate::rspace::errors::{RSpaceError, RadixTreeError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::history_action::{DeleteAction, HistoryAction, InsertAction};

pub(crate) struct NativeRadixBuilder<'a> {
    tree: &'a RadixTreeImpl,
}

impl<'a> NativeRadixBuilder<'a> {
    pub(crate) fn new(tree: &'a RadixTreeImpl) -> Self { Self { tree } }

    fn invalid() -> RSpaceError {
        RadixTreeError::SerializationError("malformed native radix node".to_owned()).into()
    }

    fn prefix_error() -> RSpaceError {
        RadixTreeError::PrefixError(
            "The length of all prefixes in the subtree must be the same.".to_owned(),
        )
        .into()
    }

    /// Changed by DR-131 S6: an offset of a stored node that does not fit in
    /// `usize` is a malformed node.
    fn checked_sum(left: usize, right: usize) -> Result<usize, RSpaceError> {
        left.checked_add(right).ok_or_else(Self::invalid)
    }

    fn cache_node(&self, key: &ByteVector, node: Node) -> Arc<Node> {
        // Changed by D-C5a (DR-100): the growth was charged at the population of
        // the key's shard. The DashMap assigns a key to a shard with a
        // per-instance RandomState and has a CPU-dependent shard count, so
        // validators charged one checkpoint differently. Every insert is now
        // charged the first-table growth hash_backing(1), which bounds the
        // growth of every shard (hash_backing(k) <= k * hash_backing(1)) and
        // depends on no shard.
        // let index = self.tree.cache_r.determine_map(key);
        // let count = self.tree.cache_r.shards()[index].read().len();
        // let next = count.checked_add(1).ok_or(RSpaceError::HostWorkRejected)?;
        // let (old_ops, old_backing) =
        //     hash_backing::<ByteVector,
        // Arc<Node>>(count).ok_or(RSpaceError::HostWorkRejected)?;
        // let (new_ops, new_backing) =
        //     hash_backing::<ByteVector,
        // Arc<Node>>(next).ok_or(RSpaceError::HostWorkRejected)?;
        let cached = Arc::new(node);
        self.tree.cache_r.insert(key.clone(), cached.clone());
        cached
    }

    fn decode_node(&self, key: &[u8], bytes: &[u8]) -> Result<Node, RSpaceError> {
        if Blake2b256Hash::new(bytes).0 != key {
            return Err(Self::invalid());
        }
        let mut node = empty_node();
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
            let prefix = bytes[header_end..prefix_end].to_vec();
            let payload = bytes[prefix_end..payload_end].to_vec();
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
        if let Some(cached) = self.tree.cache_r.get(key) {
            return Ok(Arc::clone(cached.value()));
        }
        if let Some(encoded) = self.tree.cache_w.get(key) {
            let node = self.decode_node(key, encoded.value())?;
            drop(encoded);
            return Ok(self.cache_node(key, node));
        }
        let mut outcome = None;
        self.tree.store.with_value(key, &mut |bytes| {
            outcome = Some(match bytes {
                Some(bytes) => self.decode_node(key, bytes),
                None => Err(RadixTreeError::KeyNotFound(hex::encode(key)).into()),
            });
            Ok(())
        })?;
        Ok(self.cache_node(key, outcome.ok_or_else(Self::invalid)??))
    }

    fn common_length(left: &[u8], right: &[u8]) -> usize {
        left.iter().zip(right).take_while(|(a, b)| a == b).count()
    }

    fn joined_prefix(parent: &[u8], index: u8, child: &[u8]) -> ByteVector {
        let mut joined = Vec::with_capacity(parent.len() + 1 + child.len());
        joined.extend_from_slice(parent);
        joined.push(index);
        joined.extend_from_slice(child);
        joined
    }

    fn save_node(&self, node: Node) -> Result<ByteVector, RSpaceError> {
        let key = self.tree.save_native_root(&node)?;
        if !self.tree.cache_r.contains_key(&key) {
            self.cache_node(&key, node);
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
                        prefix: Self::joined_prefix(&prefix, index as u8, child),
                        value: value.clone(),
                    });
                }
                (Some((index, Item::NodePtr { prefix: child, ptr })), None) => {
                    return Ok(Item::NodePtr {
                        prefix: Self::joined_prefix(&prefix, index as u8, child),
                        ptr: ptr.clone(),
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
        let mut node = empty_node();
        match item {
            Item::EmptyItem => {}
            Item::Leaf { prefix, value } => {
                let (first, tail) = prefix.split_first().ok_or_else(Self::prefix_error)?;
                node[*first as usize] = Item::Leaf {
                    prefix: tail.to_vec(),
                    value: value.clone(),
                };
            }
            Item::NodePtr { prefix, ptr } => {
                let (first, tail) = prefix.split_first().ok_or_else(Self::prefix_error)?;
                node[*first as usize] = Item::NodePtr {
                    prefix: tail.to_vec(),
                    ptr: ptr.clone(),
                };
            }
        }
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
                prefix: prefix.to_vec(),
                value: value.to_vec(),
            })),
            Item::Leaf {
                prefix: old,
                value: previous,
            } => {
                if old.len() != prefix.len() {
                    return Err(Self::prefix_error());
                }
                if old == prefix {
                    if previous == value {
                        return Ok(None);
                    }
                    return Ok(Some(Item::Leaf {
                        prefix: prefix.to_vec(),
                        value: value.to_vec(),
                    }));
                }
                let common = Self::common_length(old, prefix);
                let mut split = empty_node();
                let (old_index, old_tail) =
                    old[common..].split_first().ok_or_else(Self::prefix_error)?;
                let (new_index, new_tail) = prefix[common..]
                    .split_first()
                    .ok_or_else(Self::prefix_error)?;
                split[*old_index as usize] = Item::Leaf {
                    prefix: old_tail.to_vec(),
                    value: previous.clone(),
                };
                split[*new_index as usize] = Item::Leaf {
                    prefix: new_tail.to_vec(),
                    value: value.to_vec(),
                };
                Ok(Some(self.save_item(split, prefix[..common].to_vec(), false)?))
            }
            Item::NodePtr { prefix: old, ptr } => {
                if old.len() >= prefix.len() {
                    return Err(Self::prefix_error());
                }
                let common = Self::common_length(old, prefix);
                if common == old.len() {
                    let (index, tail) = prefix[common..]
                        .split_first()
                        .ok_or_else(Self::prefix_error)?;
                    let child = self.load_node(ptr)?;
                    let Some(updated) = self.update(&child[*index as usize], tail, value)? else {
                        return Ok(None);
                    };
                    let mut copy = child.as_ref().clone();
                    copy[*index as usize] = updated;
                    Ok(Some(self.save_item(copy, old[..common].to_vec(), false)?))
                } else {
                    let mut split = empty_node();
                    let (old_index, old_tail) =
                        old[common..].split_first().ok_or_else(Self::prefix_error)?;
                    let (new_index, new_tail) = prefix[common..]
                        .split_first()
                        .ok_or_else(Self::prefix_error)?;
                    split[*old_index as usize] = Item::NodePtr {
                        prefix: old_tail.to_vec(),
                        ptr: ptr.clone(),
                    };
                    split[*new_index as usize] = Item::Leaf {
                        prefix: new_tail.to_vec(),
                        value: value.to_vec(),
                    };
                    Ok(Some(self.save_item(split, prefix[..common].to_vec(), false)?))
                }
            }
        }
    }

    fn delete(&self, item: &Item, prefix: &[u8]) -> Result<Option<Item>, RSpaceError> {
        match item {
            Item::EmptyItem => Ok(None),
            Item::Leaf { prefix: old, .. } => Ok((old == prefix).then_some(Item::EmptyItem)),
            Item::NodePtr { prefix: old, ptr } => {
                let common = Self::common_length(old, prefix);
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
                let mut copy = child.as_ref().clone();
                copy[*index as usize] = updated;
                Ok(Some(self.save_item(copy, old[..common].to_vec(), true)?))
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
        for action in &actions {
            let key = match action {
                HistoryAction::Insert(insert) => &insert.key,
                HistoryAction::Delete(delete) => &delete.key,
            };
            let index = *key.first().ok_or_else(Self::prefix_error)? as usize;
            counts[index] += 1;
        }
        let mut groups: [Vec<HistoryAction>; NUM_ITEMS] =
            std::array::from_fn(|index| Vec::with_capacity(counts[index]));
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
                    key.remove(0);
                }
                self.make_actions(&child, group)?
                    .map(|next| self.save_item(next, Vec::new(), true))
                    .transpose()?
            };
            if let Some(updated) = updated {
                if updated != *item {
                    if changed.is_none() {
                        changed = Some(root.clone());
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
    use std::sync::Arc;

    use shared::rust::store::key_value_store::KeyValueStore;

    use super::*;
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
            let builder = NativeRadixBuilder::new(&native);
            if let Some(next) = builder
                .make_actions(&native_root, actions)
                .expect("native radix actions")
            {
                native.save_native_root(&next).expect("native root save");
                native_store
                    .put(
                        native
                            .prepare_native_commit()
                            .expect("native commit preparation"),
                    )
                    .expect("native store write");
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
        let builder = NativeRadixBuilder::new(&tree);
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

    /// Changed by DR-131 S6: the native decoder no longer prepays the digest
    /// comparison. The test keeps its check that a short record whose digest
    /// does not match its key is a malformed node.
    #[test]
    fn native_decoder_rejects_a_short_record() {
        let store = Arc::new(InMemoryKeyValueStore::new());
        let tree = RadixTreeImpl::new(store);
        let bytes = [3, 1, 9];
        let key = [0; DEF_SIZE];
        assert!(matches!(
            NativeRadixBuilder::new(&tree).decode_node(&key, &bytes),
            Err(RSpaceError::RadixTreeError(RadixTreeError::SerializationError(_)))
        ));
    }
}
