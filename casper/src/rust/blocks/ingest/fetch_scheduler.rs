use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, VecDeque};

use comm::rust::peer_node::PeerNode;
use models::rust::block_hash::BlockHash;
use models::rust::casper::pretty_printer::PrettyPrinter;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use super::ingest_view::IngestView;
use super::LOG_TARGET;
// The retriever's source label is part of the exported names that the
// monitoring rules and dashboards read.
use crate::rust::metrics_constants::{
    BLOCK_INGEST_METRICS_SOURCE, BLOCK_INGEST_NEEDS_DROPPED_METRIC, BLOCK_REQUESTS_RETRIES_METRIC,
    BLOCK_REQUESTS_TOTAL_METRIC, BLOCK_RETRIEVER_METRICS_SOURCE as REQUEST_METRICS_SOURCE,
};

/// First re-request interval. A view frozen on one missing block must recover
/// well inside the citability window (max-parent-depth heights of cadence) or
/// a lost delivery becomes a finality stall; startup validation asserts that
/// relation through `total_unresolved_rerequest_span_ms`.
pub const UNRESOLVED_REREQUEST_ANCHOR_MS: u64 = 500;

/// Attempts after which the re-request interval stops growing.
pub const LADDER_STEPS: u32 = 32;

pub const MAX_PEERS_PER_NEED: usize = 64;

pub const MAX_OUTSTANDING_FETCHES: usize = 256;

pub const MAX_UNCITED_NEEDS: usize = 4_096;

/// Connected peers asked first for a need no peer has announced.
pub const SEED_PEERS: usize = 4;

/// The interval before re-request `attempts + 1`: the base for the first
/// attempts, then growing until `LADDER_STEPS`, where it holds.
pub fn unresolved_rerequest_interval_ms(attempts: u32, base_interval_ms: u64) -> u64 {
    if attempts <= 4 {
        return base_interval_ms;
    }
    let multiplier = 1 + std::cmp::min(((attempts - 4) / 4) as u64, 7);
    base_interval_ms.saturating_mul(multiplier)
}

/// Wall-clock time until re-requests reach their slowest interval. A
/// citability window shorter than this cannot recover a lost delivery at the
/// fast intervals before its blocks fall below the parent-depth horizon.
pub fn total_unresolved_rerequest_span_ms() -> u64 {
    (0..LADDER_STEPS)
        .map(|attempts| unresolved_rerequest_interval_ms(attempts, UNRESOLVED_REREQUEST_ANCHOR_MS))
        .sum()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchAction {
    LookupLocal(BlockHash),
    RequestBlock { hash: BlockHash, peer: PeerNode },
    BroadcastHasBlockRequest(BlockHash),
    FetchStateRoot(Blake2b256Hash),
}

/// What the node knows about a needed block from the held blocks that cite it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NeedFacts {
    pub height_hint: Option<i64>,
    pub trusted: bool,
    /// A block by a validator bonded at the anchor cites it, or this node's
    /// proposer asked for it: the node's own chain names it, so at or below the
    /// anchor it is settled history.
    pub vouched: bool,
}

impl NeedFacts {
    fn merge(&mut self, other: &NeedFacts) {
        self.height_hint = match (self.height_hint, other.height_hint) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        self.trusted |= other.trusted;
        self.vouched |= other.vouched;
    }
}

/// Why the node wants a block it does not hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeedOrigin {
    /// A held block lists it as a missing dependency.
    Cited,
    /// The proposer or heartbeat asked for it.
    Requested,
    /// Only a peer's announcement.
    Announced,
}

/// Fetch order: trusted first, then lowest height, then oldest.
type Priority = (Reverse<bool>, i64, u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    LocalLookup { issued: bool },
    Network,
}

#[derive(Debug)]
struct BlockNeed {
    facts: NeedFacts,
    origin: NeedOrigin,
    stage: Stage,
    candidates: VecDeque<PeerNode>,
    asked: Vec<PeerNode>,
    announced: bool,
    requery_cursor: usize,
    attempts: u32,
    next_due_ms: u64,
    first_requested_ms: Option<u64>,
    last_requested_ms: Option<u64>,
    pushed_by: Option<PeerNode>,
    seq: u64,
}

impl BlockNeed {
    fn add_candidate(&mut self, peer: PeerNode) {
        if self.candidates.contains(&peer) {
            return;
        }
        if self.candidates.len() >= MAX_PEERS_PER_NEED {
            self.candidates.pop_back();
        }
        self.candidates.push_front(peer);
    }

    fn priority(&self) -> Priority {
        (
            Reverse(self.facts.trusted),
            self.facts.height_hint.unwrap_or(i64::MAX),
            self.seq,
        )
    }

    fn is_outstanding(&self, now_ms: u64) -> bool {
        self.stage == Stage::Network && self.attempts > 0 && now_ms < self.next_due_ms
    }

    /// Ask again as soon as one base interval has passed since the last
    /// request, if that is sooner than the pacing allows.
    fn hasten(&mut self, now_ms: u64) {
        let earliest = self.last_requested_ms.map_or(now_ms, |last| {
            now_ms.max(last + UNRESOLVED_REREQUEST_ANCHOR_MS)
        });
        self.next_due_ms = self.next_due_ms.min(earliest);
    }

    fn next_attempt(&mut self, hash: &BlockHash, connected: &[PeerNode]) -> Vec<FetchAction> {
        if self.attempts == 0 && self.candidates.is_empty() && self.asked.is_empty() {
            for peer in connected.iter().take(SEED_PEERS) {
                self.candidates.push_back(peer.clone());
            }
        }
        if let Some(peer) = self.candidates.pop_front() {
            if !self.asked.contains(&peer) {
                self.asked.push(peer.clone());
            }
            let mut actions = vec![FetchAction::RequestBlock {
                hash: hash.clone(),
                peer,
            }];
            if self.candidates.is_empty() {
                actions.push(FetchAction::BroadcastHasBlockRequest(hash.clone()));
            }
            return actions;
        }
        if !self.asked.is_empty() && self.attempts % 2 == 1 {
            let peer = self.asked[self.requery_cursor % self.asked.len()].clone();
            self.requery_cursor = self.requery_cursor.wrapping_add(1);
            return vec![FetchAction::RequestBlock {
                hash: hash.clone(),
                peer,
            }];
        }
        vec![FetchAction::BroadcastHasBlockRequest(hash.clone())]
    }
}

#[derive(Debug)]
struct RootNeed {
    attempts: u32,
    next_due_ms: u64,
    in_flight: bool,
}

/// What a removed block need carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedNeed {
    pub facts: NeedFacts,
    pub origin: NeedOrigin,
    pub pushed_by: Option<PeerNode>,
    /// When the network was first asked for the block.
    pub first_requested_ms: Option<u64>,
}

/// Blocks and state roots the node needs and does not hold, the peers known
/// to have each block, and when each is next asked for. Re-requests are paced
/// by `unresolved_rerequest_interval_ms`; pacing never ends a need, which
/// leaves only through `remove_block` / `remove_root` or, for a need nothing
/// cites, the `MAX_UNCITED_NEEDS` bound. Publishes which blocks are needed,
/// and which of them are solicited, to the view.
#[derive(Debug)]
pub struct FetchScheduler {
    view: IngestView,
    blocks: HashMap<BlockHash, BlockNeed>,
    uncited: BTreeMap<u64, BlockHash>,
    roots: HashMap<Blake2b256Hash, RootNeed>,
    next_seq: u64,
}

impl FetchScheduler {
    pub fn new(view: IngestView) -> Self {
        Self {
            view,
            blocks: HashMap::new(),
            uncited: BTreeMap::new(),
            roots: HashMap::new(),
            next_seq: 0,
        }
    }

    /// Need `hash` for `origin`. An existing need keeps its peers and pacing
    /// and is promoted from `Announced`.
    pub fn need_block(
        &mut self,
        hash: &BlockHash,
        origin: NeedOrigin,
        facts: NeedFacts,
        now_ms: u64,
    ) {
        if let Some(need) = self.blocks.get_mut(hash) {
            need.facts.merge(&facts);
            if need.origin != NeedOrigin::Cited && origin != NeedOrigin::Announced {
                let was_uncited = need.origin;
                need.origin = origin;
                if origin == NeedOrigin::Cited {
                    self.uncited.remove(&need.seq);
                }
                if was_uncited == NeedOrigin::Announced && origin == NeedOrigin::Requested {
                    need.next_due_ms = need.next_due_ms.min(now_ms);
                }
                self.view.set_needed(hash, true, true);
            }
            return;
        }
        tracing::debug!(
            target: LOG_TARGET,
            block = %PrettyPrinter::build_string_bytes(hash),
            origin = ?origin,
            height_hint = facts.height_hint,
            "Block needed"
        );
        let seq = self.next_seq;
        self.next_seq += 1;
        self.blocks.insert(hash.clone(), BlockNeed {
            facts,
            origin,
            stage: Stage::LocalLookup { issued: false },
            candidates: VecDeque::new(),
            asked: Vec::new(),
            announced: false,
            requery_cursor: 0,
            attempts: 0,
            next_due_ms: now_ms,
            first_requested_ms: None,
            last_requested_ms: None,
            pushed_by: None,
            seq,
        });
        self.view
            .set_needed(hash, true, origin != NeedOrigin::Announced);
        if origin != NeedOrigin::Cited {
            self.uncited.insert(seq, hash.clone());
            self.enforce_uncited_bound();
        }
    }

    /// `peer` announced it holds `hash`: it is asked next, even if it was
    /// asked before, but no sooner than one base interval after the last
    /// request, however many peers announce.
    pub fn announce(&mut self, hash: &BlockHash, peer: &PeerNode, now_ms: u64) {
        self.need_block(hash, NeedOrigin::Announced, NeedFacts::default(), now_ms);
        if let Some(need) = self.blocks.get_mut(hash) {
            need.announced = true;
            need.add_candidate(peer.clone());
            if need.stage == Stage::Network {
                need.hasten(now_ms);
            }
        }
    }

    /// Ask for `hash` again as soon as one base interval has passed since the
    /// last request.
    pub fn hasten(&mut self, hash: &BlockHash, now_ms: u64) {
        if let Some(need) = self.blocks.get_mut(hash) {
            if need.stage == Stage::Network {
                need.hasten(now_ms);
            }
        }
    }

    /// The local lookup for `hash` could not be issued; issue it again.
    pub fn defer_local_lookup(&mut self, hash: &BlockHash) {
        if let Some(need) = self.blocks.get_mut(hash) {
            if need.stage == (Stage::LocalLookup { issued: true }) {
                need.stage = Stage::LocalLookup { issued: false };
            }
        }
    }

    /// The local block store does not hold `hash`; ask the network.
    pub fn local_miss(&mut self, hash: &BlockHash, now_ms: u64) {
        if let Some(need) = self.blocks.get_mut(hash) {
            need.stage = Stage::Network;
            need.next_due_ms = now_ms;
        }
    }

    /// No copy of `hash` can be held before `until_ms`: ask the network then.
    pub fn defer_until(&mut self, hash: &BlockHash, until_ms: u64) {
        if let Some(need) = self.blocks.get_mut(hash) {
            need.stage = Stage::Network;
            need.next_due_ms = need.next_due_ms.max(until_ms);
        }
    }

    /// `peer` delivered something for `hash` that cannot satisfy the need;
    /// move on to the next peer now.
    pub fn peer_failed(&mut self, hash: &BlockHash, peer: &PeerNode, now_ms: u64) {
        if let Some(need) = self.blocks.get_mut(hash) {
            need.candidates.retain(|p| p != peer);
            need.asked.retain(|p| p != peer);
            if need.stage == Stage::Network {
                need.next_due_ms = now_ms;
            }
        }
    }

    pub fn carry_pusher(&mut self, hash: &BlockHash, peer: PeerNode) {
        if let Some(need) = self.blocks.get_mut(hash) {
            need.pushed_by.get_or_insert(peer);
        }
    }

    pub fn remove_block(&mut self, hash: &BlockHash) -> Option<RemovedNeed> {
        let need = self.blocks.remove(hash)?;
        self.uncited.remove(&need.seq);
        self.view.set_needed(hash, false, false);
        Some(RemovedNeed {
            facts: need.facts,
            origin: need.origin,
            pushed_by: need.pushed_by,
            first_requested_ms: need.first_requested_ms,
        })
    }

    /// A cited need whose last citer left. It stays, as an uncited need, only
    /// if a peer announced it.
    pub fn uncite(&mut self, hash: &BlockHash) {
        let Some(need) = self.blocks.get_mut(hash) else {
            return;
        };
        if need.origin != NeedOrigin::Cited {
            return;
        }
        if !need.announced {
            tracing::debug!(
                target: LOG_TARGET,
                block = %PrettyPrinter::build_string_bytes(hash),
                "Need ended: no held block waits on it and no peer announced it"
            );
            metrics::counter!(
                BLOCK_INGEST_NEEDS_DROPPED_METRIC,
                "source" => BLOCK_INGEST_METRICS_SOURCE,
                "reason" => "uncited"
            )
            .increment(1);
            self.remove_block(hash);
            return;
        }
        need.origin = NeedOrigin::Announced;
        let seq = need.seq;
        self.uncited.insert(seq, hash.clone());
        self.view.set_needed(hash, true, false);
        self.enforce_uncited_bound();
    }

    pub fn need_root(&mut self, root: &Blake2b256Hash, now_ms: u64) {
        self.roots.entry(root.clone()).or_insert(RootNeed {
            attempts: 0,
            next_due_ms: now_ms,
            in_flight: false,
        });
    }

    /// The requester gave up on this attempt; the next follows the pacing.
    pub fn root_dropped(&mut self, root: &Blake2b256Hash) {
        if let Some(need) = self.roots.get_mut(root) {
            need.in_flight = false;
        }
    }

    pub fn remove_root(&mut self, root: &Blake2b256Hash) { self.roots.remove(root); }

    pub fn is_needed(&self, hash: &BlockHash) -> bool { self.blocks.contains_key(hash) }

    #[cfg(test)]
    pub fn origin(&self, hash: &BlockHash) -> Option<NeedOrigin> {
        self.blocks.get(hash).map(|need| need.origin)
    }

    pub fn block_needs(&self) -> usize { self.blocks.len() }

    pub fn uncited_needs(&self) -> usize { self.uncited.len() }

    pub fn root_needs(&self) -> usize { self.roots.len() }

    pub fn outstanding(&self, now_ms: u64) -> usize {
        self.blocks
            .values()
            .filter(|need| need.is_outstanding(now_ms))
            .count()
    }

    /// The fetches due at `now_ms`: every local lookup not yet issued, up to
    /// `local_budget`; state roots whose pacing allows; and network requests,
    /// trusted needs first and lowest height first, while fewer than
    /// `MAX_OUTSTANDING_FETCHES` are outstanding.
    pub fn due(
        &mut self,
        now_ms: u64,
        connected: &[PeerNode],
        local_budget: usize,
    ) -> Vec<FetchAction> {
        let mut actions = Vec::new();

        let mut local: Vec<(u64, BlockHash)> = self
            .blocks
            .iter()
            .filter(|(_, need)| need.stage == Stage::LocalLookup { issued: false })
            .map(|(hash, need)| (need.seq, hash.clone()))
            .collect();
        local.sort_unstable();
        for (_, hash) in local.into_iter().take(local_budget) {
            let need = self
                .blocks
                .get_mut(&hash)
                .expect("the hash was collected from the needs map above");
            need.stage = Stage::LocalLookup { issued: true };
            actions.push(FetchAction::LookupLocal(hash));
        }

        for (root, need) in self.roots.iter_mut() {
            if !need.in_flight && now_ms >= need.next_due_ms {
                need.in_flight = true;
                need.next_due_ms = now_ms
                    + unresolved_rerequest_interval_ms(
                        need.attempts,
                        UNRESOLVED_REREQUEST_ANCHOR_MS,
                    );
                need.attempts = need.attempts.saturating_add(1);
                actions.push(FetchAction::FetchStateRoot(root.clone()));
            }
        }

        let mut window = MAX_OUTSTANDING_FETCHES.saturating_sub(self.outstanding(now_ms));
        if window == 0 {
            return actions;
        }
        let mut due: Vec<(Priority, BlockHash)> = self
            .blocks
            .iter()
            .filter(|(_, need)| need.stage == Stage::Network && now_ms >= need.next_due_ms)
            .map(|(hash, need)| (need.priority(), hash.clone()))
            .collect();
        due.sort_unstable();
        for (_, hash) in due {
            if window == 0 {
                break;
            }
            let need = self
                .blocks
                .get_mut(&hash)
                .expect("the hash was collected from the needs map above");
            let metric = match need.attempts {
                0 => BLOCK_REQUESTS_TOTAL_METRIC,
                _ => BLOCK_REQUESTS_RETRIES_METRIC,
            };
            metrics::counter!(metric, "source" => REQUEST_METRICS_SOURCE).increment(1);
            let first_requested_ms = *need.first_requested_ms.get_or_insert(now_ms);
            need.last_requested_ms = Some(now_ms);
            let attempt = need.next_attempt(&hash, connected);
            for action in &attempt {
                match action {
                    FetchAction::RequestBlock { peer, .. } => tracing::debug!(
                        target: LOG_TARGET,
                        block = %PrettyPrinter::build_string_bytes(&hash),
                        %peer,
                        attempt = need.attempts + 1,
                        "Requesting block"
                    ),
                    FetchAction::BroadcastHasBlockRequest(_) => tracing::debug!(
                        target: LOG_TARGET,
                        block = %PrettyPrinter::build_string_bytes(&hash),
                        attempt = need.attempts + 1,
                        "Asking every peer whether it holds the block"
                    ),
                    FetchAction::LookupLocal(_) | FetchAction::FetchStateRoot(_) => {}
                }
            }
            actions.extend(attempt);
            let interval_ms =
                unresolved_rerequest_interval_ms(need.attempts, UNRESOLVED_REREQUEST_ANCHOR_MS);
            need.next_due_ms = now_ms + interval_ms;
            need.attempts = need.attempts.saturating_add(1);
            if need.attempts == LADDER_STEPS {
                // Only a block this node needs is worth an info line: peers can
                // announce any number of hashes that do not exist.
                macro_rules! still_missing {
                    ($level:ident) => {
                        tracing::$level!(
                            target: LOG_TARGET,
                            block = %PrettyPrinter::build_string_bytes(&hash),
                            origin = ?need.origin,
                            height_hint = need.facts.height_hint,
                            requests = LADDER_STEPS,
                            missing_for_ms = now_ms.saturating_sub(first_requested_ms),
                            interval_ms,
                            "Block still missing; it is asked for at the slowest interval from now on"
                        )
                    };
                }
                match need.origin {
                    NeedOrigin::Cited | NeedOrigin::Requested => still_missing!(info),
                    NeedOrigin::Announced => still_missing!(debug),
                }
            }
            window -= 1;
        }
        actions
    }

    fn enforce_uncited_bound(&mut self) {
        while self.uncited.len() > MAX_UNCITED_NEEDS {
            let announced = self.uncited.iter().find_map(|(seq, hash)| {
                (self.blocks.get(hash).map(|need| need.origin) == Some(NeedOrigin::Announced))
                    .then_some(*seq)
            });
            let seq = match announced {
                Some(seq) => seq,
                None => *self
                    .uncited
                    .keys()
                    .next()
                    .expect("the loop runs only while uncited needs exceed the bound"),
            };
            let hash = self
                .uncited
                .get(&seq)
                .cloned()
                .expect("the sequence number was read from the map");
            // Debug, not info: peers announce freely, so this rate is theirs to set.
            tracing::debug!(
                target: LOG_TARGET,
                block = %PrettyPrinter::build_string_bytes(&hash),
                bound = MAX_UNCITED_NEEDS,
                "Need dropped: more needs than the bound that no held block waits on"
            );
            metrics::counter!(
                BLOCK_INGEST_NEEDS_DROPPED_METRIC,
                "source" => BLOCK_INGEST_METRICS_SOURCE,
                "reason" => "uncited-bound"
            )
            .increment(1);
            self.remove_block(&hash);
        }
    }
}

#[cfg(test)]
mod tests {
    use prost::bytes::Bytes;

    use super::*;
    use crate::rust::blocks::ingest::peer_reputation::tests::peer;

    fn hash(i: u32) -> BlockHash { Bytes::copy_from_slice(&i.to_be_bytes()) }

    fn peers(n: usize) -> Vec<PeerNode> { (0..n).map(|i| peer(&format!("p{i}"))).collect() }

    fn scheduler() -> FetchScheduler { FetchScheduler::new(IngestView::new()) }

    fn cited(height: i64, trusted: bool) -> NeedFacts {
        NeedFacts {
            height_hint: Some(height),
            trusted,
            vouched: false,
        }
    }

    fn to_network(s: &mut FetchScheduler, h: &BlockHash, now_ms: u64) {
        assert_eq!(s.due(now_ms, &[], usize::MAX), vec![
            FetchAction::LookupLocal(h.clone())
        ]);
        s.local_miss(h, now_ms);
    }

    #[test]
    fn the_span_until_the_slowest_interval_is_fifty_eight_seconds() {
        assert_eq!(total_unresolved_rerequest_span_ms(), 58_000);
        assert_eq!(
            unresolved_rerequest_interval_ms(LADDER_STEPS, UNRESOLVED_REREQUEST_ANCHOR_MS),
            unresolved_rerequest_interval_ms(10 * LADDER_STEPS, UNRESOLVED_REREQUEST_ANCHOR_MS),
        );
    }

    #[test]
    fn a_need_is_looked_up_locally_before_the_network() {
        let mut s = scheduler();
        let h = hash(1);
        s.need_block(&h, NeedOrigin::Cited, cited(5, true), 0);
        assert_eq!(s.due(0, &peers(4), usize::MAX), vec![
            FetchAction::LookupLocal(h.clone())
        ]);
        assert!(s.due(0, &peers(4), usize::MAX).is_empty());
        s.local_miss(&h, 0);
        assert!(matches!(
            s.due(0, &peers(4), usize::MAX)[0],
            FetchAction::RequestBlock { .. }
        ));
    }

    #[test]
    fn seed_peers_are_asked_in_turn_then_the_need_is_broadcast() {
        let mut s = scheduler();
        let h = hash(1);
        let connected = peers(6);
        s.need_block(&h, NeedOrigin::Cited, cited(5, true), 0);
        to_network(&mut s, &h, 0);
        let mut now = 0;
        let mut asked = Vec::new();
        let mut broadcast_at = None;
        for attempt in 0..SEED_PEERS {
            for action in s.due(now, &connected, usize::MAX) {
                match action {
                    FetchAction::RequestBlock { peer, .. } => asked.push(peer),
                    FetchAction::BroadcastHasBlockRequest(_) => broadcast_at = Some(attempt),
                    other => panic!("unexpected {other:?}"),
                }
            }
            now += 10_000;
        }
        assert_eq!(asked, connected[..SEED_PEERS].to_vec());
        assert_eq!(broadcast_at, Some(SEED_PEERS - 1));
    }

    #[test]
    fn retries_slow_to_the_slowest_interval_and_never_end() {
        let mut s = scheduler();
        let h = hash(1);
        s.need_block(&h, NeedOrigin::Cited, cited(5, true), 0);
        to_network(&mut s, &h, 0);
        let slowest =
            unresolved_rerequest_interval_ms(LADDER_STEPS, UNRESOLVED_REREQUEST_ANCHOR_MS);
        let mut now = 0;
        for _ in 0..(10 * LADDER_STEPS) {
            assert!(!s.due(now, &peers(4), usize::MAX).is_empty());
            now += slowest;
        }
        assert!(s.is_needed(&h));
    }

    #[test]
    fn a_need_is_not_asked_again_before_its_interval() {
        let mut s = scheduler();
        let h = hash(1);
        s.need_block(&h, NeedOrigin::Cited, cited(5, true), 0);
        to_network(&mut s, &h, 0);
        assert!(!s.due(0, &peers(4), usize::MAX).is_empty());
        assert!(s
            .due(UNRESOLVED_REREQUEST_ANCHOR_MS - 1, &peers(4), usize::MAX)
            .is_empty());
        assert!(!s
            .due(UNRESOLVED_REREQUEST_ANCHOR_MS, &peers(4), usize::MAX)
            .is_empty());
    }

    #[test]
    fn a_peer_asked_before_that_announces_the_block_is_asked_at_once() {
        let mut s = scheduler();
        let h = hash(1);
        let connected = peers(1);
        s.need_block(&h, NeedOrigin::Cited, cited(5, true), 0);
        to_network(&mut s, &h, 0);
        let mut now = 0;
        for _ in 0..(2 * LADDER_STEPS) {
            s.due(now, &connected, usize::MAX);
            now += 1_000;
        }
        s.announce(&h, &connected[0], now);
        assert_eq!(s.due(now, &connected, usize::MAX), vec![
            FetchAction::RequestBlock {
                hash: h.clone(),
                peer: connected[0].clone()
            },
            FetchAction::BroadcastHasBlockRequest(h.clone()),
        ]);
    }

    #[test]
    fn announcements_do_not_ask_for_a_need_faster_than_the_base_interval() {
        let mut s = scheduler();
        let h = hash(1);
        let announcers = peers(15);
        s.need_block(&h, NeedOrigin::Cited, cited(5, true), 0);
        to_network(&mut s, &h, 0);
        assert!(!s.due(0, &announcers, usize::MAX).is_empty());
        let mut asked_again = Vec::new();
        for (i, peer) in announcers.iter().enumerate() {
            let now = 10 * (i as u64 + 1);
            s.announce(&h, peer, now);
            asked_again.extend(s.due(now, &announcers, usize::MAX));
        }
        assert_eq!(
            asked_again,
            vec![],
            "fifteen announcements inside one base interval asked for the block again"
        );
    }

    #[test]
    fn trusted_and_lowest_needs_take_the_window_first() {
        let mut s = scheduler();
        let total = MAX_OUTSTANDING_FETCHES as u32 + 10;
        for i in 0..total {
            let h = hash(i);
            s.need_block(
                &h,
                NeedOrigin::Cited,
                cited(i64::from(total - i), i % 2 == 0),
                0,
            );
        }
        s.due(0, &[], usize::MAX);
        for i in 0..total {
            s.local_miss(&hash(i), 0);
        }
        let requested: Vec<BlockHash> = s
            .due(0, &peers(4), usize::MAX)
            .into_iter()
            .filter_map(|action| match action {
                FetchAction::RequestBlock { hash, .. } => Some(hash),
                _ => None,
            })
            .collect();
        assert_eq!(requested.len(), MAX_OUTSTANDING_FETCHES);
        let mut expected: Vec<u32> = (0..total).collect();
        expected.sort_by_key(|i| (i % 2 != 0, total - i));
        let expected: Vec<BlockHash> = expected
            .into_iter()
            .take(MAX_OUTSTANDING_FETCHES)
            .map(hash)
            .collect();
        assert_eq!(requested, expected);
        assert_eq!(s.outstanding(0), MAX_OUTSTANDING_FETCHES);
    }

    #[test]
    fn uncited_needs_stay_within_their_bound_dropping_announcements_first() {
        let view = IngestView::new();
        let mut s = FetchScheduler::new(view.clone());
        let requested = hash(u32::MAX);
        s.need_block(&requested, NeedOrigin::Requested, NeedFacts::default(), 0);
        for i in 0..(MAX_UNCITED_NEEDS as u32 + 5) {
            s.announce(&hash(i), &peer("gossip"), 0);
        }
        assert_eq!(s.uncited_needs(), MAX_UNCITED_NEEDS);
        for i in 0..6 {
            assert!(!s.is_needed(&hash(i)) && !view.is_awaited(&hash(i)));
        }
        assert!(s.is_needed(&hash(6)) && view.is_awaited(&hash(6)));
        assert!(s.is_needed(&requested) && view.is_solicited(&requested));
    }

    #[test]
    fn the_view_mirrors_needs_and_their_solicitation() {
        let view = IngestView::new();
        let mut s = FetchScheduler::new(view.clone());
        let h = hash(1);
        s.announce(&h, &peer("p"), 0);
        assert!(view.is_awaited(&h) && !view.is_solicited(&h));
        s.need_block(&h, NeedOrigin::Cited, cited(3, true), 0);
        assert!(view.is_solicited(&h));
        s.uncite(&h);
        assert!(view.is_awaited(&h) && !view.is_solicited(&h));
        s.remove_block(&h);
        assert!(!view.is_awaited(&h));
    }

    #[test]
    fn a_cited_need_is_outside_the_uncited_bound() {
        let mut s = scheduler();
        let h = hash(1);
        s.announce(&h, &peer("gossip"), 0);
        s.need_block(&h, NeedOrigin::Cited, cited(3, true), 0);
        for i in 2..(MAX_UNCITED_NEEDS as u32 + 10) {
            s.announce(&hash(i), &peer("gossip"), 0);
        }
        assert!(s.is_needed(&h));
        assert_eq!(s.origin(&h), Some(NeedOrigin::Cited));
    }

    #[test]
    fn an_uncited_need_with_no_peer_is_dropped_and_one_with_a_peer_kept() {
        let mut s = scheduler();
        let quiet = hash(1);
        let announced = hash(2);
        s.need_block(&quiet, NeedOrigin::Cited, cited(3, true), 0);
        s.need_block(&announced, NeedOrigin::Cited, cited(3, true), 0);
        s.announce(&announced, &peer("p"), 0);
        s.uncite(&quiet);
        assert!(!s.is_needed(&quiet));
        s.uncite(&announced);
        assert_eq!(s.origin(&announced), Some(NeedOrigin::Announced));
    }

    #[test]
    fn a_failed_peer_is_skipped_and_the_next_asked_at_once() {
        let mut s = scheduler();
        let h = hash(1);
        let connected = peers(4);
        s.need_block(&h, NeedOrigin::Cited, cited(5, true), 0);
        to_network(&mut s, &h, 0);
        s.due(0, &connected, usize::MAX);
        s.peer_failed(&h, &connected[0], 1);
        assert_eq!(s.due(1, &connected, usize::MAX), vec![
            FetchAction::RequestBlock {
                hash: h.clone(),
                peer: connected[1].clone()
            }
        ]);
    }

    #[test]
    fn a_state_root_is_fetched_again_on_its_pacing_after_a_drop() {
        let mut s = scheduler();
        let root = Blake2b256Hash::new(b"root");
        s.need_root(&root, 0);
        assert_eq!(s.due(0, &[], 0), vec![FetchAction::FetchStateRoot(
            root.clone()
        )]);
        assert!(s.due(10_000, &[], 0).is_empty());
        s.root_dropped(&root);
        assert!(s.due(UNRESOLVED_REREQUEST_ANCHOR_MS - 1, &[], 0).is_empty());
        assert_eq!(s.due(UNRESOLVED_REREQUEST_ANCHOR_MS, &[], 0), vec![
            FetchAction::FetchStateRoot(root.clone())
        ]);
        s.remove_root(&root);
        assert_eq!(s.root_needs(), 0);
    }

    #[test]
    fn the_pusher_survives_until_the_need_is_removed() {
        let mut s = scheduler();
        let h = hash(1);
        s.need_block(&h, NeedOrigin::Cited, cited(5, true), 0);
        s.carry_pusher(&h, peer("pusher"));
        s.carry_pusher(&h, peer("later"));
        assert_eq!(s.remove_block(&h).unwrap().pushed_by, Some(peer("pusher")));
    }
}
