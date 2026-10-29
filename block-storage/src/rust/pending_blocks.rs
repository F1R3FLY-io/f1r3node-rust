use std::collections::HashMap;
use std::sync::Mutex;

use models::casper::BlockMessageProto;
use models::rust::block_hash::BlockHash;
use models::rust::casper::protocol::casper_message::BlockMessage;
use prost::bytes::Bytes;
use prost::Message;

/// An unsolicited block is admitted only below this share of each limit; the
/// rest of the area is reserved for blocks the node asked for.
pub const LOW_WATER_PERCENT: usize = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingBlockLimits {
    pub max_blocks: usize,
    pub max_bytes: usize,
    pub max_per_sender: usize,
}

impl PendingBlockLimits {
    pub fn low_water(limit: usize) -> usize { limit * LOW_WATER_PERCENT / 100 }
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

/// What the area holds: blocks, encoded bytes, and blocks per sender (only
/// senders with a block in the area).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PendingUsage {
    pub blocks: usize,
    pub bytes: usize,
    pub by_sender: HashMap<Bytes, usize>,
}

struct Entry {
    encoded: Bytes,
    sender: Bytes,
}

fn decode(encoded: &Bytes) -> Option<BlockMessage> {
    BlockMessageProto::decode(encoded.as_ref())
        .ok()
        .and_then(|proto| BlockMessage::from_proto(proto).ok())
}

struct Inner {
    limits: PendingBlockLimits,
    entries: HashMap<BlockHash, Entry>,
    per_sender: HashMap<Bytes, usize>,
    bytes: usize,
}

impl Inner {
    fn remove(&mut self, hash: &BlockHash) -> Option<Entry> {
        let entry = self.entries.remove(hash)?;
        self.bytes -= entry.encoded.len();
        let count = self
            .per_sender
            .get_mut(&entry.sender)
            .expect("every entry is counted under its sender");
        *count -= 1;
        if *count == 0 {
            self.per_sender.remove(&entry.sender);
        }
        Some(entry)
    }
}

/// Blocks held in memory until their sender's authorization is known: bounded
/// storage with one admission class. A block the node asked for is refused
/// only at the hard limits; any other block is refused once the area, or its
/// sender's share, is above the low-water mark. The area never evicts: block
/// ingest decides what leaves.
pub struct PendingBlocks {
    inner: Mutex<Inner>,
}

impl PendingBlocks {
    pub fn new(limits: PendingBlockLimits) -> Self {
        Self {
            inner: Mutex::new(Inner {
                limits,
                entries: HashMap::new(),
                per_sender: HashMap::new(),
                bytes: 0,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn limits(&self) -> PendingBlockLimits { self.lock().limits }

    /// New limits apply to later inserts; blocks already held stay.
    pub fn set_limits(&self, limits: PendingBlockLimits) { self.lock().limits = limits; }

    pub fn insert(&self, block: &BlockMessage, solicited: bool) -> PendingInsert {
        let encoded = Bytes::from(block.to_proto().encode_to_vec());
        let bytes = encoded.len();
        let mut inner = self.lock();
        let limits = inner.limits;

        if inner.entries.contains_key(&block.block_hash) {
            return PendingInsert::Accepted;
        }
        if bytes > limits.max_bytes {
            return PendingInsert::TooLarge;
        }
        let (max_blocks, max_bytes, max_per_sender) = match solicited {
            true => (limits.max_blocks, limits.max_bytes, limits.max_per_sender),
            false => (
                PendingBlockLimits::low_water(limits.max_blocks),
                PendingBlockLimits::low_water(limits.max_bytes),
                PendingBlockLimits::low_water(limits.max_per_sender),
            ),
        };
        // A sender absent from the count holds no block in the area.
        let sender_blocks = inner.per_sender.get(&block.sender).copied().unwrap_or(0);
        if sender_blocks + 1 > max_per_sender {
            return PendingInsert::SenderLimitReached;
        }
        if inner.entries.len() + 1 > max_blocks || inner.bytes + bytes > max_bytes {
            return PendingInsert::AreaFull;
        }

        *inner.per_sender.entry(block.sender.clone()).or_insert(0) += 1;
        inner.bytes += bytes;
        inner.entries.insert(block.block_hash.clone(), Entry {
            encoded,
            sender: block.sender.clone(),
        });
        PendingInsert::Accepted
    }

    pub fn get(&self, hash: &BlockHash) -> Option<BlockMessage> {
        let encoded = self.lock().entries.get(hash)?.encoded.clone();
        decode(&encoded)
    }

    pub fn contains(&self, hash: &BlockHash) -> bool { self.lock().entries.contains_key(hash) }

    pub fn remove(&self, hash: &BlockHash) -> bool { self.lock().remove(hash).is_some() }

    pub fn len(&self) -> usize { self.lock().entries.len() }

    pub fn is_empty(&self) -> bool { self.len() == 0 }

    pub fn bytes(&self) -> usize { self.lock().bytes }

    pub fn usage(&self) -> PendingUsage {
        let inner = self.lock();
        PendingUsage {
            blocks: inner.entries.len(),
            bytes: inner.bytes,
            by_sender: inner.per_sender.clone(),
        }
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

    fn size() -> usize { block(1, 1).to_proto().encoded_len() }

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
        assert_eq!(area.insert(&b, false), PendingInsert::Accepted);
        assert_eq!(area.get(&b.block_hash), Some(b.clone()));
        assert_eq!(area.len(), 1);
        assert!(area.bytes() > 0);
        assert!(area.remove(&b.block_hash));
        assert!(area.is_empty());
        assert_eq!(area.bytes(), 0);
        assert_eq!(area.usage(), PendingUsage::default());
    }

    #[test]
    fn unsolicited_blocks_are_refused_above_the_low_water_mark_and_requested_ones_are_not() {
        let area = PendingBlocks::new(limits(10, usize::MAX, 100));
        for i in 0..9 {
            assert_eq!(area.insert(&block(i, i), false), PendingInsert::Accepted);
        }
        assert_eq!(area.insert(&block(9, 9), false), PendingInsert::AreaFull);
        assert_eq!(area.insert(&block(9, 9), true), PendingInsert::Accepted);
        assert_eq!(area.insert(&block(10, 10), true), PendingInsert::AreaFull);
        assert_eq!(area.len(), 10, "the area never evicts");
    }

    #[test]
    fn the_byte_low_water_mark_reserves_room_for_requested_blocks() {
        let area = PendingBlocks::new(limits(100, size() * 10, 100));
        for i in 0..9 {
            assert_eq!(area.insert(&block(i, i), false), PendingInsert::Accepted);
        }
        assert_eq!(area.insert(&block(9, 9), false), PendingInsert::AreaFull);
        assert_eq!(area.insert(&block(9, 9), true), PendingInsert::Accepted);
        assert!(area.bytes() <= size() * 10);
    }

    #[test]
    fn a_sender_above_its_low_water_mark_is_refused_unsolicited_blocks_only() {
        let area = PendingBlocks::new(limits(100, usize::MAX, 10));
        for i in 0..9 {
            assert_eq!(area.insert(&block(i, 7), false), PendingInsert::Accepted);
        }
        assert_eq!(
            area.insert(&block(9, 7), false),
            PendingInsert::SenderLimitReached
        );
        assert_eq!(area.insert(&block(9, 8), false), PendingInsert::Accepted);
        assert_eq!(area.insert(&block(10, 7), true), PendingInsert::Accepted);
        assert_eq!(
            area.insert(&block(11, 7), true),
            PendingInsert::SenderLimitReached
        );
        assert_eq!(area.usage().by_sender.get(&block(0, 7).sender), Some(&10));
    }

    #[test]
    fn a_block_larger_than_the_area_is_too_large() {
        assert_eq!(
            PendingBlocks::new(limits(100, size() - 1, 100)).insert(&block(1, 1), true),
            PendingInsert::TooLarge
        );
    }

    #[test]
    fn many_identities_stay_below_the_low_water_mark() {
        let area = PendingBlocks::new(limits(20, usize::MAX, 10));
        for i in 0..=255u8 {
            area.insert(&block(i, i), false);
        }
        assert_eq!(area.len(), PendingBlockLimits::low_water(20));
    }

    #[test]
    fn shrinking_the_limits_keeps_held_blocks_and_refuses_new_ones() {
        let area = PendingBlocks::new(limits(10, usize::MAX, 10));
        area.insert(&block(1, 1), true);
        area.insert(&block(2, 2), false);
        area.set_limits(limits(1, usize::MAX, 10));
        assert_eq!(area.len(), 2);
        assert_eq!(area.insert(&block(3, 3), true), PendingInsert::AreaFull);
        assert_eq!(area.limits().max_blocks, 1);
    }

    #[test]
    fn a_duplicate_insert_is_accepted_without_a_second_entry() {
        let area = PendingBlocks::new(limits(10, usize::MAX, 10));
        area.insert(&block(1, 1), false);
        assert_eq!(area.insert(&block(1, 1), true), PendingInsert::Accepted);
        assert_eq!(area.len(), 1);
    }

    #[test]
    fn the_byte_count_is_the_encoded_size_that_the_area_holds() {
        use models::rust::casper::protocol::casper_message::Justification;

        let mut big = block(1, 1);
        big.justifications = (0..10_000u32)
            .map(|i| Justification {
                validator: Bytes::from(vec![1u8; 65]),
                latest_block_hash: Bytes::copy_from_slice(&[i.to_be_bytes(); 8].concat()),
            })
            .collect();
        let encoded = big.to_proto().encoded_len();
        let area = PendingBlocks::new(PendingBlockLimits::default());

        area.insert(&big, false);

        assert_eq!(area.bytes(), encoded);
        assert_eq!(area.usage().bytes, encoded);
        assert_eq!(area.get(&big.block_hash), Some(big));
    }
}
