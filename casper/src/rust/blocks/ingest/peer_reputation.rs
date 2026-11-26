use std::collections::{HashMap, VecDeque};

use comm::rust::peer_node::PeerNode;

use super::ingest_view::IngestView;

pub const MAX_TRACKED_PEERS: usize = 1_024;

pub const STRIKE_WINDOW_MS: u64 = 10 * 60 * 1000;

pub const STRIKE_THRESHOLD: usize = 16;

pub const IGNORE_TTL_MS: u64 = 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strike {
    UnauthorizedSender,
    Malformed,
    HashMismatch,
    InvalidSender,
}

impl Strike {
    pub fn label(&self) -> &'static str {
        match self {
            Strike::UnauthorizedSender => "unauthorized-sender",
            Strike::Malformed => "malformed",
            Strike::HashMismatch => "hash-mismatch",
            Strike::InvalidSender => "invalid-sender",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrikeOutcome {
    Counted,
    Ignored { until_ms: u64 },
}

/// Strikes against peers that pushed unsolicited blocks with a striking
/// outcome, and the peers ignored for reaching the threshold inside the
/// window. Both tables stay within `MAX_TRACKED_PEERS`. Publishes the ignore
/// list to the view.
#[derive(Debug)]
pub struct PeerReputation {
    view: IngestView,
    strikes: HashMap<PeerNode, VecDeque<u64>>,
    ignored_until: HashMap<PeerNode, u64>,
}

impl PeerReputation {
    pub fn new(view: IngestView) -> Self {
        Self {
            view,
            strikes: HashMap::new(),
            ignored_until: HashMap::new(),
        }
    }

    pub fn strike(&mut self, peer: &PeerNode, now_ms: u64) -> StrikeOutcome {
        if self.is_ignored(peer, now_ms) {
            return StrikeOutcome::Counted;
        }
        if !self.strikes.contains_key(peer) && self.strikes.len() >= MAX_TRACKED_PEERS {
            self.sweep(now_ms);
            if self.strikes.len() >= MAX_TRACKED_PEERS {
                self.evict_weakest_striker();
            }
        }
        let times = self.strikes.entry(peer.clone()).or_default();
        drop_strikes_outside_window(times, now_ms);
        times.push_back(now_ms);
        if times.len() < STRIKE_THRESHOLD {
            return StrikeOutcome::Counted;
        }
        self.strikes.remove(peer);
        if self.ignored_until.len() >= MAX_TRACKED_PEERS {
            self.sweep(now_ms);
            if self.ignored_until.len() >= MAX_TRACKED_PEERS {
                self.evict_earliest_expiry();
            }
        }
        let until_ms = now_ms.saturating_add(IGNORE_TTL_MS);
        self.ignored_until.insert(peer.clone(), until_ms);
        self.view.set_ignored_until(peer, Some(until_ms));
        StrikeOutcome::Ignored { until_ms }
    }

    pub fn is_ignored(&self, peer: &PeerNode, now_ms: u64) -> bool {
        self.ignored_until
            .get(peer)
            .is_some_and(|until| now_ms < *until)
    }

    pub fn ignored_peers(&self, now_ms: u64) -> usize {
        self.ignored_until
            .values()
            .filter(|until| now_ms < **until)
            .count()
    }

    #[cfg(test)]
    pub fn tracked_strikers(&self) -> usize { self.strikes.len() }

    fn sweep(&mut self, now_ms: u64) {
        let view = &self.view;
        self.ignored_until.retain(|peer, until| {
            let kept = now_ms < *until;
            if !kept {
                view.set_ignored_until(peer, None);
            }
            kept
        });
        self.strikes.retain(|_, times| {
            drop_strikes_outside_window(times, now_ms);
            !times.is_empty()
        });
    }

    fn evict_weakest_striker(&mut self) {
        let weakest = self
            .strikes
            .iter()
            .min_by_key(|(_, times)| (times.len(), times.back().copied()))
            .map(|(peer, _)| peer.clone());
        if let Some(peer) = weakest {
            self.strikes.remove(&peer);
        }
    }

    fn evict_earliest_expiry(&mut self) {
        let earliest = self
            .ignored_until
            .iter()
            .min_by_key(|(_, until)| **until)
            .map(|(peer, _)| peer.clone());
        if let Some(peer) = earliest {
            self.ignored_until.remove(&peer);
            self.view.set_ignored_until(&peer, None);
        }
    }
}

fn drop_strikes_outside_window(times: &mut VecDeque<u64>, now_ms: u64) {
    while times
        .front()
        .is_some_and(|t| now_ms.saturating_sub(*t) >= STRIKE_WINDOW_MS)
    {
        times.pop_front();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use comm::rust::peer_node::{Endpoint, NodeIdentifier};
    use prost::bytes::Bytes;

    use super::*;

    pub(crate) fn peer(name: &str) -> PeerNode {
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

    fn strike_n(
        reputation: &mut PeerReputation,
        p: &PeerNode,
        n: usize,
        now_ms: u64,
    ) -> StrikeOutcome {
        let mut last = StrikeOutcome::Counted;
        for _ in 0..n {
            last = reputation.strike(p, now_ms);
        }
        last
    }

    #[test]
    fn the_threshold_strike_ignores_the_peer() {
        let mut reputation = PeerReputation::new(IngestView::new());
        let bad = peer("bad");
        let below = strike_n(&mut reputation, &bad, STRIKE_THRESHOLD - 1, 1_000);
        assert_eq!(below, StrikeOutcome::Counted);
        assert!(!reputation.is_ignored(&bad, 1_000));
        assert_eq!(reputation.strike(&bad, 1_000), StrikeOutcome::Ignored {
            until_ms: 1_000 + IGNORE_TTL_MS
        });
        assert!(reputation.is_ignored(&bad, 1_000));
        assert!(!reputation.is_ignored(&peer("good"), 1_000));
    }

    #[test]
    fn a_strike_while_ignored_does_not_extend_the_ignore() {
        let mut reputation = PeerReputation::new(IngestView::new());
        let bad = peer("bad");
        strike_n(&mut reputation, &bad, STRIKE_THRESHOLD, 0);
        assert_eq!(reputation.strike(&bad, 1_000), StrikeOutcome::Counted);
        assert!(!reputation.is_ignored(&bad, IGNORE_TTL_MS));
    }

    #[test]
    fn the_ignore_expires_after_its_ttl() {
        let mut reputation = PeerReputation::new(IngestView::new());
        let bad = peer("bad");
        strike_n(&mut reputation, &bad, STRIKE_THRESHOLD, 0);
        assert!(reputation.is_ignored(&bad, IGNORE_TTL_MS - 1));
        assert!(!reputation.is_ignored(&bad, IGNORE_TTL_MS));
        assert_eq!(reputation.ignored_peers(IGNORE_TTL_MS), 0);
    }

    #[test]
    fn strikes_older_than_the_window_do_not_count() {
        let mut reputation = PeerReputation::new(IngestView::new());
        let p = peer("slow");
        strike_n(&mut reputation, &p, STRIKE_THRESHOLD - 1, 0);
        assert_eq!(
            reputation.strike(&p, STRIKE_WINDOW_MS),
            StrikeOutcome::Counted
        );
        assert!(!reputation.is_ignored(&p, STRIKE_WINDOW_MS));
    }

    #[test]
    fn many_identities_stay_inside_the_peer_bound() {
        let mut reputation = PeerReputation::new(IngestView::new());
        for i in 0..MAX_TRACKED_PEERS + 10 {
            reputation.strike(&peer(&format!("id{i}")), 0);
        }
        assert!(reputation.tracked_strikers() <= MAX_TRACKED_PEERS);
    }

    #[test]
    fn a_flooder_is_ignored_when_throwaway_identities_fill_the_strike_table() {
        let mut reputation = PeerReputation::new(IngestView::new());
        for i in 0..MAX_TRACKED_PEERS {
            reputation.strike(&peer(&format!("id{i}")), 0);
        }
        let bad = peer("bad");
        let last = strike_n(&mut reputation, &bad, STRIKE_THRESHOLD, 1);
        assert!(matches!(last, StrikeOutcome::Ignored { .. }));
        assert!(reputation.is_ignored(&bad, 1));
        assert!(reputation.tracked_strikers() <= MAX_TRACKED_PEERS);
    }

    #[test]
    fn a_new_flooder_is_ignored_when_the_ignore_table_is_full() {
        let mut reputation = PeerReputation::new(IngestView::new());
        for i in 0..MAX_TRACKED_PEERS {
            strike_n(
                &mut reputation,
                &peer(&format!("id{i}")),
                STRIKE_THRESHOLD,
                i as u64,
            );
        }
        assert_eq!(
            reputation.ignored_peers(MAX_TRACKED_PEERS as u64),
            MAX_TRACKED_PEERS
        );
        let bad = peer("bad");
        let last = strike_n(
            &mut reputation,
            &bad,
            STRIKE_THRESHOLD,
            MAX_TRACKED_PEERS as u64,
        );
        assert!(matches!(last, StrikeOutcome::Ignored { .. }));
        assert!(reputation.is_ignored(&bad, MAX_TRACKED_PEERS as u64));
        assert!(!reputation.is_ignored(&peer("id0"), MAX_TRACKED_PEERS as u64));
    }

    #[test]
    fn the_view_mirrors_the_ignore_list() {
        let view = IngestView::new();
        let mut reputation = PeerReputation::new(view.clone());
        for i in 0..MAX_TRACKED_PEERS {
            strike_n(
                &mut reputation,
                &peer(&format!("id{i}")),
                STRIKE_THRESHOLD,
                i as u64,
            );
        }
        assert!(view.is_ignored_at(&peer("id0"), MAX_TRACKED_PEERS as u64));
        strike_n(
            &mut reputation,
            &peer("bad"),
            STRIKE_THRESHOLD,
            MAX_TRACKED_PEERS as u64,
        );
        assert!(view.is_ignored_at(&peer("bad"), MAX_TRACKED_PEERS as u64));
        assert!(!view.is_ignored_at(&peer("id0"), MAX_TRACKED_PEERS as u64));
    }
}
