use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;
use prost::Message;

/// Bounds of the in-memory area that holds blocks whose sender authorization
/// is not known yet. The area is node-local and never part of consensus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingBlockLimits {
    pub max_blocks: usize,
    pub max_bytes: usize,
    pub max_per_sender: usize,
}

impl Default for PendingBlockLimits {
    fn default() -> Self {
        Self {
            max_blocks: 8_192,
            max_bytes: 256 * 1024 * 1024,
            max_per_sender: 2_048,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingInsert {
    Accepted,
    SenderLimitReached,
    AreaFull,
    TooLarge,
}

struct Entry {
    block: BlockMessage,
    bytes: usize,
    solicited: bool,
    seq: u64,
}

struct Inner {
    limits: PendingBlockLimits,
    entries: HashMap<BlockHash, Entry>,
    unsolicited: BTreeMap<u64, BlockHash>,
    solicited: BTreeMap<u64, BlockHash>,
    per_sender: HashMap<Bytes, usize>,
    bytes: usize,
    next_seq: u64,
}

impl Inner {
    fn remove(&mut self, hash: &BlockHash) -> Option<Entry> {
        let entry = self.entries.remove(hash)?;
        if entry.solicited {
            self.solicited.remove(&entry.seq);
        } else {
            self.unsolicited.remove(&entry.seq);
        }
        self.bytes -= entry.bytes;
        if let Some(count) = self.per_sender.get_mut(&entry.block.sender) {
            *count -= 1;
            if *count == 0 {
                self.per_sender.remove(&entry.block.sender);
            }
        }
        Some(entry)
    }

    fn oldest(&self, include_solicited: bool) -> Option<BlockHash> {
        self.unsolicited
            .values()
            .next()
            .or_else(|| {
                include_solicited
                    .then(|| self.solicited.values().next())
                    .flatten()
            })
            .cloned()
    }
}

/// Blocks that passed format and signature checks but whose main parent is
/// not validated yet, so the sender's bond cannot be judged. They stay here,
/// off disk, until the authorization is known. When the area is full,
/// unsolicited blocks are evicted before blocks this node asked for as a
/// dependency, oldest first: a catch-up is mostly solicited blocks, and a
/// flood starts from an unsolicited one.
pub struct PendingBlocks {
    inner: Mutex<Inner>,
}

impl PendingBlocks {
    pub fn new(limits: PendingBlockLimits) -> Self {
        Self {
            inner: Mutex::new(Inner {
                limits,
                entries: HashMap::new(),
                unsolicited: BTreeMap::new(),
                solicited: BTreeMap::new(),
                per_sender: HashMap::new(),
                bytes: 0,
                next_seq: 0,
            }),
        }
    }

    pub fn limits(&self) -> PendingBlockLimits {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).limits
    }

    /// Replace the bounds and evict, unsolicited first, until the area fits.
    pub fn set_limits(&self, limits: PendingBlockLimits) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.limits = limits;
        while inner.entries.len() > limits.max_blocks || inner.bytes > limits.max_bytes {
            match inner.oldest(true) {
                Some(victim) => {
                    inner.remove(&victim);
                }
                None => break,
            }
        }
    }

    pub fn insert(&self, block: BlockMessage, solicited: bool) -> PendingInsert {
        let bytes = block.to_proto().encoded_len();
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let limits = inner.limits;

        if let Some(existing) = inner.entries.get(&block.block_hash) {
            if solicited && !existing.solicited {
                let seq = existing.seq;
                let hash = block.block_hash.clone();
                inner.unsolicited.remove(&seq);
                inner.solicited.insert(seq, hash.clone());
                if let Some(entry) = inner.entries.get_mut(&hash) {
                    entry.solicited = true;
                }
            }
            return PendingInsert::Accepted;
        }
        if bytes > limits.max_bytes {
            return PendingInsert::TooLarge;
        }
        if inner.per_sender.get(&block.sender).copied().unwrap_or(0) >= limits.max_per_sender {
            return PendingInsert::SenderLimitReached;
        }
        while inner.entries.len() + 1 > limits.max_blocks || inner.bytes + bytes > limits.max_bytes
        {
            match inner.oldest(solicited) {
                Some(victim) => {
                    inner.remove(&victim);
                }
                None => return PendingInsert::AreaFull,
            }
        }

        let seq = inner.next_seq;
        inner.next_seq += 1;
        let hash = block.block_hash.clone();
        *inner.per_sender.entry(block.sender.clone()).or_insert(0) += 1;
        inner.bytes += bytes;
        if solicited {
            inner.solicited.insert(seq, hash.clone());
        } else {
            inner.unsolicited.insert(seq, hash.clone());
        }
        inner.entries.insert(hash, Entry {
            block,
            bytes,
            solicited,
            seq,
        });
        PendingInsert::Accepted
    }

    pub fn get(&self, hash: &BlockHash) -> Option<BlockMessage> {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.entries.get(hash).map(|entry| entry.block.clone())
    }

    pub fn contains(&self, hash: &BlockHash) -> bool {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.entries.contains_key(hash)
    }

    pub fn remove(&self, hash: &BlockHash) -> Option<BlockMessage> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.remove(hash).map(|entry| entry.block)
    }

    pub fn len(&self) -> usize {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.entries.len()
    }

    pub fn is_empty(&self) -> bool { self.len() == 0 }

    pub fn bytes(&self) -> usize {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.bytes
    }
}

#[cfg(test)]
mod tests {
    use models::rust::block_implicits::get_random_block_default;

    use super::*;

    fn base() -> &'static BlockMessage {
        static BASE: std::sync::OnceLock<BlockMessage> = std::sync::OnceLock::new();
        BASE.get_or_init(get_random_block_default)
    }

    fn block(hash: u8, sender: u8) -> BlockMessage {
        let mut b = base().clone();
        b.block_hash = Bytes::from(vec![hash; 32]);
        b.sender = Bytes::from(vec![sender; 65]);
        b
    }

    fn limits(max_blocks: usize, max_bytes: usize, max_per_sender: usize) -> PendingBlockLimits {
        PendingBlockLimits {
            max_blocks,
            max_bytes,
            max_per_sender,
        }
    }

    #[test]
    fn inserted_block_is_readable_and_removable() {
        let area = PendingBlocks::new(PendingBlockLimits::default());
        let b = block(1, 1);
        assert_eq!(area.insert(b.clone(), false), PendingInsert::Accepted);
        assert_eq!(area.get(&b.block_hash), Some(b.clone()));
        assert_eq!(area.len(), 1);
        assert!(area.bytes() > 0);
        assert_eq!(area.remove(&b.block_hash), Some(b.clone()));
        assert!(area.is_empty());
        assert_eq!(area.bytes(), 0);
    }

    #[test]
    fn full_area_evicts_the_oldest_unsolicited_block_first() {
        let area = PendingBlocks::new(limits(3, usize::MAX, 10));
        area.insert(block(1, 1), true);
        area.insert(block(2, 2), false);
        area.insert(block(3, 3), false);
        assert_eq!(area.insert(block(4, 4), false), PendingInsert::Accepted);
        assert!(area.contains(&block(1, 1).block_hash));
        assert!(!area.contains(&block(2, 2).block_hash));
        assert!(area.contains(&block(3, 3).block_hash));
        assert!(area.contains(&block(4, 4).block_hash));
        assert_eq!(area.len(), 3);
    }

    #[test]
    fn unsolicited_block_never_evicts_a_solicited_one() {
        let area = PendingBlocks::new(limits(2, usize::MAX, 10));
        area.insert(block(1, 1), true);
        area.insert(block(2, 2), true);
        assert_eq!(area.insert(block(3, 3), false), PendingInsert::AreaFull);
        assert_eq!(area.insert(block(4, 4), true), PendingInsert::Accepted);
        assert!(!area.contains(&block(1, 1).block_hash));
        assert!(area.contains(&block(2, 2).block_hash));
        assert!(area.contains(&block(4, 4).block_hash));
    }

    #[test]
    fn byte_limit_bounds_the_area() {
        let size = block(1, 1).to_proto().encoded_len();
        let area = PendingBlocks::new(limits(100, size * 2, 100));
        for i in 0..10u8 {
            area.insert(block(i, i), false);
        }
        assert!(area.bytes() <= size * 2);
        assert!(area.len() <= 2);
        assert_eq!(
            PendingBlocks::new(limits(100, size - 1, 100)).insert(block(1, 1), false),
            PendingInsert::TooLarge
        );
    }

    #[test]
    fn sender_limit_rejects_new_blocks_from_that_sender_only() {
        let area = PendingBlocks::new(limits(100, usize::MAX, 2));
        area.insert(block(1, 7), false);
        area.insert(block(2, 7), false);
        assert_eq!(
            area.insert(block(3, 7), false),
            PendingInsert::SenderLimitReached
        );
        assert_eq!(area.insert(block(4, 8), false), PendingInsert::Accepted);
        area.remove(&block(1, 7).block_hash);
        assert_eq!(area.insert(block(3, 7), false), PendingInsert::Accepted);
    }

    #[test]
    fn many_identities_stay_inside_the_global_limit() {
        let area = PendingBlocks::new(limits(16, usize::MAX, 1));
        for i in 0..=255u8 {
            area.insert(block(i, i), false);
        }
        assert_eq!(area.len(), 16);
    }

    #[test]
    fn shrinking_the_limits_evicts_unsolicited_blocks_first() {
        let area = PendingBlocks::new(limits(10, usize::MAX, 10));
        area.insert(block(1, 1), true);
        area.insert(block(2, 2), false);
        area.insert(block(3, 3), false);
        area.set_limits(limits(2, usize::MAX, 10));
        assert_eq!(area.len(), 2);
        assert!(area.contains(&block(1, 1).block_hash));
        assert!(area.contains(&block(3, 3).block_hash));
        assert_eq!(area.limits().max_blocks, 2);
    }

    #[test]
    fn a_solicited_reinsert_protects_an_unsolicited_entry() {
        let area = PendingBlocks::new(limits(2, usize::MAX, 10));
        area.insert(block(1, 1), false);
        area.insert(block(2, 2), false);
        area.insert(block(1, 1), true);
        area.insert(block(3, 3), false);
        assert!(area.contains(&block(1, 1).block_hash));
        assert!(!area.contains(&block(2, 2).block_hash));
    }
}
