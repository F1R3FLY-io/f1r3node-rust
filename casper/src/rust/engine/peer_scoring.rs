use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use comm::rust::peer_node::PeerNode;
use models::rust::block_hash::BlockHash;

use crate::rust::blocks::ingest::peer_reputation::{self, PeerReputation};
pub use crate::rust::blocks::ingest::peer_reputation::{Strike, IGNORE_TTL_MS, STRIKE_THRESHOLD};

pub const MAX_TRACKED_SOURCES: usize = 16_384;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrikeOutcome {
    NoSource,
    Counted(PeerNode),
    Ignored(PeerNode),
}

#[derive(Debug, Default)]
struct Inner {
    sources: HashMap<BlockHash, (PeerNode, u64)>,
    source_order: BTreeMap<u64, BlockHash>,
    next_seq: u64,
    reputation: PeerReputation,
}

impl Inner {
    fn take_source(&mut self, hash: &BlockHash) -> Option<PeerNode> {
        let (peer, seq) = self.sources.remove(hash)?;
        self.source_order.remove(&seq);
        Some(peer)
    }
}

#[derive(Debug)]
pub struct PeerScores {
    inner: Mutex<Inner>,
}

impl Default for PeerScores {
    fn default() -> Self { Self::new() }
}

impl PeerScores {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn note_unsolicited(&self, hash: &BlockHash, peer: &PeerNode) {
        let mut inner = self.lock();
        if inner.sources.contains_key(hash) {
            return;
        }
        while inner.sources.len() >= MAX_TRACKED_SOURCES {
            let Some((_, oldest)) = inner.source_order.pop_first() else {
                break;
            };
            inner.sources.remove(&oldest);
        }
        let seq = inner.next_seq;
        inner.next_seq += 1;
        inner.sources.insert(hash.clone(), (peer.clone(), seq));
        inner.source_order.insert(seq, hash.clone());
    }

    pub fn forget(&self, hash: &BlockHash) { self.lock().take_source(hash); }

    pub fn strike(&self, hash: &BlockHash, now_ms: u64) -> StrikeOutcome {
        let mut inner = self.lock();
        let Some(peer) = inner.take_source(hash) else {
            return StrikeOutcome::NoSource;
        };
        match inner.reputation.strike(&peer, now_ms) {
            peer_reputation::StrikeOutcome::Counted => StrikeOutcome::Counted(peer),
            peer_reputation::StrikeOutcome::Ignored { .. } => StrikeOutcome::Ignored(peer),
        }
    }

    pub fn is_ignored(&self, peer: &PeerNode, now_ms: u64) -> bool {
        self.lock().reputation.is_ignored(peer, now_ms)
    }

    pub fn tracked_sources(&self) -> usize { self.lock().sources.len() }

    pub fn ignored_peers(&self, now_ms: u64) -> usize {
        self.lock().reputation.ignored_peers(now_ms)
    }
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;
    use crate::rust::blocks::ingest::peer_reputation::tests::peer;

    fn hash(i: u32) -> BlockHash { Bytes::copy_from_slice(&i.to_be_bytes()) }

    #[test]
    fn a_strike_without_a_tracked_source_counts_nothing() {
        let scores = PeerScores::new();
        assert_eq!(scores.strike(&hash(1), 0), StrikeOutcome::NoSource);
    }

    #[test]
    fn a_forgotten_block_carries_no_strike() {
        let scores = PeerScores::new();
        let p = peer("honest");
        scores.note_unsolicited(&hash(1), &p);
        scores.forget(&hash(1));
        assert_eq!(scores.strike(&hash(1), 0), StrikeOutcome::NoSource);
        assert_eq!(scores.tracked_sources(), 0);
    }

    #[test]
    fn the_first_pushing_peer_is_the_source() {
        let scores = PeerScores::new();
        scores.note_unsolicited(&hash(1), &peer("first"));
        scores.note_unsolicited(&hash(1), &peer("second"));
        assert_eq!(
            scores.strike(&hash(1), 0),
            StrikeOutcome::Counted(peer("first"))
        );
        assert_eq!(scores.strike(&hash(1), 0), StrikeOutcome::NoSource);
    }

    #[test]
    fn the_source_map_forgets_the_oldest_entry_past_its_bound() {
        let scores = PeerScores::new();
        let p = peer("flood");
        for i in 0..=MAX_TRACKED_SOURCES as u32 {
            scores.note_unsolicited(&hash(i), &p);
        }
        assert_eq!(scores.tracked_sources(), MAX_TRACKED_SOURCES);
        assert_eq!(scores.strike(&hash(0), 0), StrikeOutcome::NoSource);
        assert_eq!(
            scores.strike(&hash(MAX_TRACKED_SOURCES as u32), 0),
            StrikeOutcome::Counted(p)
        );
    }

    #[test]
    fn a_struck_source_reaches_the_ignore_threshold() {
        let scores = PeerScores::new();
        let bad = peer("bad");
        let mut last = StrikeOutcome::NoSource;
        for i in 0..peer_reputation::STRIKE_THRESHOLD as u32 {
            scores.note_unsolicited(&hash(i), &bad);
            last = scores.strike(&hash(i), 0);
        }
        assert_eq!(last, StrikeOutcome::Ignored(bad.clone()));
        assert!(scores.is_ignored(&bad, 0));
    }
}
