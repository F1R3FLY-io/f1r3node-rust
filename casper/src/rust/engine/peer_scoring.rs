use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Mutex;

use comm::rust::peer_node::PeerNode;
use models::rust::block_hash::BlockHash;

/// Unsolicited blocks whose delivering peer is remembered until the block's
/// outcome is known. The oldest entry is forgotten first past this bound.
pub const MAX_TRACKED_SOURCES: usize = 16_384;

/// Peers with strikes in the current window, and peers being ignored. A new
/// peer past either bound is not tracked, as in the WAL payload blacklist.
pub const MAX_TRACKED_PEERS: usize = 1_024;

/// Strikes inside this window count towards the threshold.
pub const STRIKE_WINDOW_MS: u64 = 10 * 60 * 1000;

/// Strikes inside one window that make the node ignore a peer.
pub const STRIKE_THRESHOLD: usize = 16;

/// How long the node ignores block traffic from a peer past the threshold.
pub const IGNORE_TTL_MS: u64 = 60 * 60 * 1000;

/// An outcome that an honest peer never produces for a block it pushes
/// unsolicited: honest nodes announce and relay a block only after it
/// entered their DAG, so it passed every one of these checks there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strike {
    UnauthorizedSender,
    Malformed,
    InvalidSender,
    SenderLimitReached,
}

impl Strike {
    pub fn label(&self) -> &'static str {
        match self {
            Strike::UnauthorizedSender => "unauthorized-sender",
            Strike::Malformed => "malformed",
            Strike::InvalidSender => "invalid-sender",
            Strike::SenderLimitReached => "pending-sender-limit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrikeOutcome {
    /// The block was solicited, or its source is no longer tracked.
    NoSource,
    Counted(PeerNode),
    /// This strike crossed the threshold; the peer is now ignored.
    Ignored(PeerNode),
}

#[derive(Debug, Default)]
struct Inner {
    sources: HashMap<BlockHash, (PeerNode, u64)>,
    source_order: BTreeMap<u64, BlockHash>,
    next_seq: u64,
    strikes: HashMap<PeerNode, VecDeque<u64>>,
    ignored_until: HashMap<PeerNode, u64>,
}

impl Inner {
    fn take_source(&mut self, hash: &BlockHash) -> Option<PeerNode> {
        let (peer, seq) = self.sources.remove(hash)?;
        self.source_order.remove(&seq);
        Some(peer)
    }

    fn is_ignored(&self, peer: &PeerNode, now_ms: u64) -> bool {
        self.ignored_until
            .get(peer)
            .is_some_and(|until| now_ms < *until)
    }

    fn sweep(&mut self, now_ms: u64) {
        self.ignored_until.retain(|_, until| now_ms < *until);
        self.strikes.retain(|_, times| {
            while times
                .front()
                .is_some_and(|t| now_ms.saturating_sub(*t) >= STRIKE_WINDOW_MS)
            {
                times.pop_front();
            }
            !times.is_empty()
        });
    }
}

/// Node-local scoring of the peers that push blocks to this node. A peer
/// that pushes [`STRIKE_THRESHOLD`] blocks with a [`Strike`] outcome inside
/// [`STRIKE_WINDOW_MS`] has its block traffic ignored for [`IGNORE_TTL_MS`].
/// Blocks this node requested as a dependency are never attributed: an
/// honest peer can serve a forged block that this node asked for.
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

    /// Remember the first peer that pushed this block.
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

    /// Drop the source of a block whose outcome carries no strike.
    pub fn forget(&self, hash: &BlockHash) { self.lock().take_source(hash); }

    pub fn strike(&self, hash: &BlockHash, now_ms: u64) -> StrikeOutcome {
        let mut inner = self.lock();
        let Some(peer) = inner.take_source(hash) else {
            return StrikeOutcome::NoSource;
        };
        if inner.is_ignored(&peer, now_ms) {
            return StrikeOutcome::Counted(peer);
        }
        if !inner.strikes.contains_key(&peer) && inner.strikes.len() >= MAX_TRACKED_PEERS {
            inner.sweep(now_ms);
            if inner.strikes.len() >= MAX_TRACKED_PEERS {
                return StrikeOutcome::Counted(peer);
            }
        }
        let times = inner.strikes.entry(peer.clone()).or_default();
        while times
            .front()
            .is_some_and(|t| now_ms.saturating_sub(*t) >= STRIKE_WINDOW_MS)
        {
            times.pop_front();
        }
        times.push_back(now_ms);
        if times.len() < STRIKE_THRESHOLD {
            return StrikeOutcome::Counted(peer);
        }
        inner.strikes.remove(&peer);
        if inner.ignored_until.len() >= MAX_TRACKED_PEERS {
            inner.sweep(now_ms);
            if inner.ignored_until.len() >= MAX_TRACKED_PEERS {
                return StrikeOutcome::Counted(peer);
            }
        }
        inner
            .ignored_until
            .insert(peer.clone(), now_ms.saturating_add(IGNORE_TTL_MS));
        StrikeOutcome::Ignored(peer)
    }

    pub fn is_ignored(&self, peer: &PeerNode, now_ms: u64) -> bool {
        self.lock().is_ignored(peer, now_ms)
    }

    pub fn tracked_sources(&self) -> usize { self.lock().sources.len() }

    pub fn ignored_peers(&self, now_ms: u64) -> usize {
        let inner = self.lock();
        inner
            .ignored_until
            .values()
            .filter(|until| now_ms < **until)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use comm::rust::peer_node::{Endpoint, NodeIdentifier};
    use prost::bytes::Bytes;

    use super::*;

    fn peer(name: &str) -> PeerNode {
        PeerNode {
            id: NodeIdentifier {
                key: Bytes::copy_from_slice(name.as_bytes()),
            },
            endpoint: Endpoint {
                host: format!("{name}.local"),
                tcp_port: 40400,
                udp_port: 40404,
            },
        }
    }

    fn hash(i: u32) -> BlockHash { Bytes::copy_from_slice(&i.to_be_bytes()) }

    fn strike_n(
        scores: &PeerScores,
        p: &PeerNode,
        from: u32,
        n: u32,
        now_ms: u64,
    ) -> StrikeOutcome {
        let mut last = StrikeOutcome::NoSource;
        for i in from..from + n {
            scores.note_unsolicited(&hash(i), p);
            last = scores.strike(&hash(i), now_ms);
        }
        last
    }

    #[test]
    fn a_strike_without_a_tracked_source_counts_nothing() {
        let scores = PeerScores::new();
        assert_eq!(scores.strike(&hash(1), 0), StrikeOutcome::NoSource);
    }

    #[test]
    fn the_threshold_strike_ignores_the_peer() {
        let scores = PeerScores::new();
        let bad = peer("bad");
        let below = strike_n(&scores, &bad, 0, STRIKE_THRESHOLD as u32 - 1, 1_000);
        assert_eq!(below, StrikeOutcome::Counted(bad.clone()));
        assert!(!scores.is_ignored(&bad, 1_000));
        let last = strike_n(&scores, &bad, 100, 1, 1_000);
        assert_eq!(last, StrikeOutcome::Ignored(bad.clone()));
        assert!(scores.is_ignored(&bad, 1_000));
        assert!(!scores.is_ignored(&peer("good"), 1_000));
    }

    #[test]
    fn the_ignore_expires_after_its_ttl() {
        let scores = PeerScores::new();
        let bad = peer("bad");
        strike_n(&scores, &bad, 0, STRIKE_THRESHOLD as u32, 0);
        assert!(scores.is_ignored(&bad, IGNORE_TTL_MS - 1));
        assert!(!scores.is_ignored(&bad, IGNORE_TTL_MS));
        assert_eq!(scores.ignored_peers(IGNORE_TTL_MS), 0);
    }

    #[test]
    fn strikes_older_than_the_window_do_not_count() {
        let scores = PeerScores::new();
        let p = peer("slow");
        strike_n(&scores, &p, 0, STRIKE_THRESHOLD as u32 - 1, 0);
        let last = strike_n(&scores, &p, 100, 1, STRIKE_WINDOW_MS);
        assert_eq!(last, StrikeOutcome::Counted(p.clone()));
        assert!(!scores.is_ignored(&p, STRIKE_WINDOW_MS));
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
    fn many_identities_stay_inside_the_peer_bound() {
        let scores = PeerScores::new();
        for i in 0..(MAX_TRACKED_PEERS as u32 + 10) {
            let p = peer(&format!("id{i}"));
            scores.note_unsolicited(&hash(i), &p);
            scores.strike(&hash(i), 0);
        }
        assert!(scores.lock().strikes.len() <= MAX_TRACKED_PEERS);
    }
}
