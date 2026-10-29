use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use block_storage::rust::pending_blocks::{PendingBlockLimits, PendingUsage};
use comm::rust::peer_node::PeerNode;
use models::rust::block_hash::BlockHash;
use proptest::prelude::*;
use prost::bytes::Bytes;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;

use super::dependency_tracker::{BodyAt, HeldMeta, Work};
use super::fetch_scheduler::FetchAction;
use super::ingest_state::*;
use super::ingest_view::IngestView;
use super::peer_reputation::tests::peer;
use super::peer_reputation::{Strike, StrikeOutcome, STRIKE_THRESHOLD};

fn hash(i: u32) -> BlockHash { Bytes::copy_from_slice(&i.to_be_bytes()) }

const LIMITS: PendingBlockLimits = PendingBlockLimits {
    max_blocks: 100,
    max_bytes: 100_000,
    max_per_sender: 40,
};

fn config() -> IngestConfig { IngestConfig::new(0, LIMITS) }

#[derive(Default)]
struct Dag(HashSet<BlockHash>);

impl Dependencies for Dag {
    fn in_dag(&self, hash: &BlockHash) -> bool { self.0.contains(hash) }

    fn is_satisfied(&self, hash: &BlockHash) -> bool { self.0.contains(hash) }
}

fn dag_with(hashes: &[u32]) -> Dag { Dag(hashes.iter().copied().map(hash).collect()) }

fn meta(i: u32, height: i64, deps: &[u32]) -> HeldMeta {
    HeldMeta {
        hash: hash(i),
        sender: Bytes::from_static(b"v0"),
        height,
        deps: deps.iter().copied().map(hash).collect(),
        body: BodyAt::Disk,
        size: 1_000,
        trusted: true,
        bonded_at_anchor: true,
        solicited: false,
        pushed_by: None,
    }
}

fn pending(
    mut m: HeldMeta,
    sender: &'static [u8],
    solicited: bool,
    pusher: Option<PeerNode>,
) -> HeldMeta {
    m.body = BodyAt::Pending;
    m.sender = Bytes::from_static(sender);
    m.solicited = solicited;
    m.pushed_by = if solicited { None } else { pusher };
    m
}

fn received(meta: HeldMeta) -> IngestInput { IngestInput::Received { meta, area: None } }

fn state() -> IngestState { IngestState::new(config(), IngestView::new()) }

fn fetches(effects: &[Effect]) -> Vec<FetchAction> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Fetch(action) => Some(action.clone()),
            _ => None,
        })
        .collect()
}

fn tick(s: &mut IngestState, now_ms: u64, connected: &[PeerNode], dag: &Dag) -> Vec<Effect> {
    s.handle(
        IngestInput::Tick {
            connected: connected.to_vec(),
            area: PendingUsage::default(),
        },
        now_ms,
        dag,
    )
}

// ── Transitions ─────────────────────────────────────────────────────────────

#[test]
fn a_block_with_missing_parents_waits_and_its_parents_become_solicited_needs() {
    let mut s = state();
    let dag = dag_with(&[0]);
    s.handle(received(meta(10, 5, &[0, 7, 8])), 0, &dag);
    assert!(s.is_held(&hash(10)) && s.view().is_held(&hash(10)));
    assert!(s.next_dispatch().is_none());
    for parent in [7, 8] {
        assert!(s.is_needed(&hash(parent)));
        assert!(s.view().is_solicited(&hash(parent)));
        assert!(s.view().is_awaited(&hash(parent)));
        assert!(s.view().has_waiters(&hash(parent)));
    }
    assert!(!s.is_needed(&hash(0)));
}

#[test]
fn a_block_is_dispatched_once_its_parents_are_inserted() {
    let mut s = state();
    let mut dag = dag_with(&[0]);
    s.handle(received(meta(10, 2, &[7])), 0, &dag);
    s.handle(received(meta(7, 1, &[0])), 0, &dag);
    assert!(!s.is_needed(&hash(7)));
    assert_eq!(s.next_dispatch(), Some((hash(7), Work::Validate)));
    dag.0.insert(hash(7));
    s.handle(IngestInput::Inserted { hash: hash(7) }, 0, &dag);
    assert!(!s.is_held(&hash(7)) && !s.view().has_waiters(&hash(7)));
    assert_eq!(s.next_dispatch(), Some((hash(10), Work::Validate)));
}

#[test]
fn a_cited_block_at_or_below_the_anchor_is_admitted_as_settled_history() {
    let mut s = IngestState::new(IngestConfig::new(50, LIMITS), IngestView::new());
    let dag = dag_with(&[0]);
    s.handle(received(meta(10, 41, &[9])), 0, &dag);
    s.handle(received(meta(9, 40, &[3])), 0, &dag);
    assert_eq!(s.next_dispatch(), Some((hash(9), Work::AdmitSettled)));
    s.handle(
        IngestInput::Outcome {
            hash: hash(9),
            outcome: WorkOutcome::NotSettledHistory,
        },
        0,
        &dag,
    );
    assert!(
        s.is_needed(&hash(3)),
        "a declined admission is classified normally"
    );
}

#[test]
fn an_announced_hash_is_awaited_but_not_solicited() {
    let mut s = state();
    let dag = dag_with(&[0]);
    s.handle(
        IngestInput::Announced {
            hash: hash(5),
            peer: peer("p"),
        },
        0,
        &dag,
    );
    assert!(s.view().is_awaited(&hash(5)));
    assert!(!s.view().is_solicited(&hash(5)));
    s.handle(
        IngestInput::Announced {
            hash: hash(0),
            peer: peer("p"),
        },
        0,
        &dag,
    );
    assert!(!s.is_needed(&hash(0)), "a block in the DAG is not needed");
}

#[test]
fn an_unsolicited_malformed_block_strikes_its_pusher_and_a_solicited_one_does_not() {
    let mut s = state();
    let dag = dag_with(&[0]);
    let refused = |solicited: bool| IngestInput::IntakeRefused {
        hash: hash(1),
        from: Some(peer("p")),
        unsolicited: !solicited,
        size: 10,
        sender: Bytes::from_static(b"v0"),
        reason: Refusal::Malformed,
        area: None,
    };
    let effects = s.handle(refused(false), 0, &dag);
    assert_eq!(effects, vec![Effect::Struck {
        peer: peer("p"),
        strike: Strike::Malformed,
        outcome: StrikeOutcome::Counted
    }]);
    assert!(s.handle(refused(true), 0, &dag).is_empty());
}

#[test]
fn the_sixteenth_strike_ignores_the_peer_in_the_view() {
    let mut s = state();
    let dag = dag_with(&[0]);
    for i in 0..STRIKE_THRESHOLD as u32 {
        s.handle(
            IngestInput::IntakeRefused {
                hash: hash(i),
                from: Some(peer("bad")),
                unsolicited: true,
                size: 10,
                sender: Bytes::from_static(b"v0"),
                reason: Refusal::Unauthorized,
                area: None,
            },
            0,
            &dag,
        );
    }
    assert!(s.view().is_ignored(&peer("bad"), 0));
    assert!(!s.view().is_ignored(&peer("good"), 0));
}

#[test]
fn a_rejected_block_drops_what_waits_on_it_and_strikes_an_unsolicited_pusher() {
    let mut s = state();
    let dag = dag_with(&[0]);
    s.handle(
        received(pending(
            meta(1, 1, &[0]),
            b"v1",
            false,
            Some(peer("pusher")),
        )),
        0,
        &dag,
    );
    s.handle(
        received(pending(meta(2, 2, &[1]), b"v1", false, Some(peer("other")))),
        0,
        &dag,
    );
    assert_eq!(s.next_dispatch(), Some((hash(1), Work::Validate)));
    let effects = s.handle(
        IngestInput::Outcome {
            hash: hash(1),
            outcome: WorkOutcome::Rejected {
                invalid_sender: true,
            },
        },
        0,
        &dag,
    );
    assert!(effects.contains(&Effect::Struck {
        peer: peer("pusher"),
        strike: Strike::InvalidSender,
        outcome: StrikeOutcome::Counted
    }));
    assert!(effects.contains(&Effect::DiscardPending(hash(1))));
    assert!(effects.contains(&Effect::DiscardPending(hash(2))));
    assert_eq!(s.held(), 0);
    assert_eq!(s.block_needs(), 0);
}

#[test]
fn a_requested_block_rejected_for_an_invalid_sender_strikes_nobody() {
    let mut s = state();
    let dag = dag_with(&[0]);
    s.handle(IngestInput::Need { hash: hash(1) }, 0, &dag);
    s.handle(
        received(pending(meta(1, 1, &[0]), b"v1", true, None)),
        0,
        &dag,
    );
    s.next_dispatch();
    let effects = s.handle(
        IngestInput::Outcome {
            hash: hash(1),
            outcome: WorkOutcome::Rejected {
                invalid_sender: true,
            },
        },
        0,
        &dag,
    );
    assert!(!effects.iter().any(|e| matches!(e, Effect::Struck { .. })));
}

#[test]
fn an_evicted_block_keeps_its_pusher_through_redelivery() {
    let mut s = state();
    let dag = dag_with(&[0]);
    s.handle(
        received(pending(
            meta(1, 1, &[9]),
            b"v1",
            false,
            Some(peer("pusher")),
        )),
        0,
        &dag,
    );
    s.handle(received(meta(2, 2, &[1])), 0, &dag);
    let full = PendingUsage {
        blocks: LIMITS.max_blocks,
        bytes: 0,
        by_sender: HashMap::new(),
    };
    let effects = s.handle(
        IngestInput::Tick {
            connected: vec![],
            area: full,
        },
        0,
        &dag,
    );
    assert!(effects.contains(&Effect::DiscardPending(hash(1))));
    assert!(s.is_needed(&hash(1)) && s.view().is_solicited(&hash(1)));
    s.handle(
        received(pending(meta(1, 1, &[0]), b"v1", true, None)),
        0,
        &dag,
    );
    s.handle(IngestInput::Inserted { hash: hash(9) }, 0, &dag);
    assert_eq!(s.next_dispatch(), Some((hash(1), Work::Validate)));
    let effects = s.handle(
        IngestInput::Outcome {
            hash: hash(1),
            outcome: WorkOutcome::Rejected {
                invalid_sender: true,
            },
        },
        0,
        &dag,
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Struck { peer, .. } if *peer == peer_named("pusher"))),
        "the original pusher is struck: {effects:?}"
    );
}

fn peer_named(name: &str) -> PeerNode { peer(name) }

#[test]
fn a_block_awaiting_a_state_root_is_released_when_it_is_imported() {
    let mut s = state();
    let dag = dag_with(&[0]);
    let root = Blake2b256Hash::new(b"root");
    s.handle(received(meta(1, 1, &[0])), 0, &dag);
    s.next_dispatch();
    s.handle(
        IngestInput::Outcome {
            hash: hash(1),
            outcome: WorkOutcome::AwaitingState(root.clone()),
        },
        0,
        &dag,
    );
    assert_eq!(fetches(&tick(&mut s, 0, &[], &dag)), vec![
        FetchAction::FetchStateRoot(root.clone())
    ]);
    s.handle(
        IngestInput::StateFetch {
            root: root.clone(),
            result: StateFetchResult::Dropped(StateFetchDrop::Deadline),
        },
        0,
        &dag,
    );
    assert_eq!(fetches(&tick(&mut s, 60_000, &[], &dag)), vec![
        FetchAction::FetchStateRoot(root.clone())
    ]);
    s.handle(
        IngestInput::StateFetch {
            root,
            result: StateFetchResult::Imported,
        },
        0,
        &dag,
    );
    assert_eq!(s.next_dispatch(), Some((hash(1), Work::Validate)));
    assert_eq!(s.root_needs(), 0);
}

#[test]
fn an_exhausted_state_root_budget_drops_the_blocks_awaiting_it() {
    let mut s = state();
    let dag = dag_with(&[0]);
    let root = Blake2b256Hash::new(b"root");
    s.handle(received(meta(1, 1, &[0])), 0, &dag);
    s.handle(received(meta(2, 2, &[1])), 0, &dag);
    s.next_dispatch();
    s.handle(
        IngestInput::Outcome {
            hash: hash(1),
            outcome: WorkOutcome::AwaitingState(root.clone()),
        },
        0,
        &dag,
    );
    s.handle(
        IngestInput::StateFetch {
            root,
            result: StateFetchResult::Dropped(StateFetchDrop::BudgetExhausted),
        },
        0,
        &dag,
    );
    assert_eq!(s.held(), 0);
    assert_eq!(s.root_needs(), 0);
}

#[test]
fn a_block_that_keeps_failing_is_dropped_and_quarantined() {
    let mut s = state();
    let dag = dag_with(&[0]);
    s.handle(received(meta(1, 1, &[0])), 0, &dag);
    for _ in 0..super::dependency_tracker::MAX_VALIDATION_FAILURES {
        assert!(s.next_dispatch().is_some());
        s.handle(
            IngestInput::Outcome {
                hash: hash(1),
                outcome: WorkOutcome::Failed,
            },
            0,
            &dag,
        );
    }
    assert!(!s.is_held(&hash(1)));
    s.handle(received(meta(1, 1, &[0])), 1_000, &dag);
    assert!(!s.is_held(&hash(1)), "quarantined");
    s.handle(
        received(meta(1, 1, &[0])),
        super::dependency_tracker::VALIDATION_FAILURE_QUARANTINE_MS,
        &dag,
    );
    assert!(s.is_held(&hash(1)));
}

#[test]
fn a_need_with_a_waiter_is_asked_for_until_a_peer_serves_it() {
    let mut s = state();
    let dag = dag_with(&[0]);
    let connected = vec![peer("a"), peer("b")];
    s.handle(received(meta(10, 2, &[1])), 0, &dag);
    tick(&mut s, 0, &connected, &dag);
    s.handle(IngestInput::LocalMiss { hash: hash(1) }, 0, &dag);
    let mut now = 0;
    let mut asked = 0;
    while now < 30 * 60 * 1000 {
        asked += fetches(&tick(&mut s, now, &connected, &dag)).len();
        now += 1_000;
    }
    assert!(s.is_needed(&hash(1)) && s.is_held(&hash(10)));
    assert!(asked > 400, "asked {asked} times in 30 minutes");
    s.handle(
        IngestInput::Announced {
            hash: hash(1),
            peer: connected[0].clone(),
        },
        now,
        &dag,
    );
    assert!(
        fetches(&tick(&mut s, now, &connected, &dag)).contains(&FetchAction::RequestBlock {
            hash: hash(1),
            peer: connected[0].clone()
        })
    );
}

#[test]
fn proactive_passes_evict_only_unsolicited_blocks() {
    let mut s = state();
    let dag = dag_with(&[0]);
    for i in 1..=5 {
        s.handle(
            received(pending(meta(i, 1, &[99]), b"v1", true, None)),
            0,
            &dag,
        );
    }
    for i in 6..=10 {
        s.handle(
            received(pending(meta(i, 1, &[99]), b"v1", false, Some(peer("p")))),
            0,
            &dag,
        );
    }
    let effects = s.handle(
        IngestInput::Tick {
            connected: vec![],
            area: PendingUsage {
                blocks: LIMITS.max_blocks,
                bytes: 0,
                by_sender: HashMap::new(),
            },
        },
        0,
        &dag,
    );
    let discarded: HashSet<BlockHash> = effects
        .iter()
        .filter_map(|e| match e {
            Effect::DiscardPending(h) => Some(h.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(discarded, (6..=10).map(hash).collect());
}

#[test]
fn a_refused_requested_block_evicts_unsolicited_before_requested_blocks() {
    let mut s = state();
    let dag = dag_with(&[0]);
    s.handle(
        received(pending(meta(1, 1, &[99]), b"v1", true, None)),
        0,
        &dag,
    );
    s.handle(
        received(pending(meta(2, 1, &[99]), b"v1", false, Some(peer("p")))),
        0,
        &dag,
    );
    let refused = |blocks: usize| IngestInput::IntakeRefused {
        hash: hash(50),
        from: Some(peer("q")),
        unsolicited: false,
        size: 10,
        sender: Bytes::from_static(b"v2"),
        reason: Refusal::Pending(PendingRefusal::AreaFull),
        area: Some(PendingUsage {
            blocks,
            bytes: 0,
            by_sender: HashMap::new(),
        }),
    };
    assert_eq!(s.handle(refused(LIMITS.max_blocks), 0, &dag), vec![
        Effect::DiscardPending(hash(2))
    ]);
    assert_eq!(s.handle(refused(LIMITS.max_blocks), 0, &dag), vec![
        Effect::DiscardPending(hash(1))
    ]);
}

#[test]
fn a_sender_at_its_quota_loses_only_its_own_unsolicited_blocks() {
    let mut s = state();
    let dag = dag_with(&[0]);
    s.handle(
        received(pending(meta(1, 1, &[99]), b"v1", false, Some(peer("p")))),
        0,
        &dag,
    );
    s.handle(
        received(pending(meta(2, 1, &[99]), b"v1", true, None)),
        0,
        &dag,
    );
    s.handle(
        received(pending(meta(3, 1, &[99]), b"v2", false, Some(peer("p")))),
        0,
        &dag,
    );
    let effects = s.handle(
        IngestInput::IntakeRefused {
            hash: hash(50),
            from: Some(peer("q")),
            unsolicited: false,
            size: 10,
            sender: Bytes::from_static(b"v1"),
            reason: Refusal::Pending(PendingRefusal::SenderLimitReached),
            area: Some(PendingUsage {
                blocks: 3,
                bytes: 0,
                by_sender: [(Bytes::from_static(b"v1"), LIMITS.max_per_sender)].into(),
            }),
        },
        0,
        &dag,
    );
    assert_eq!(effects, vec![Effect::DiscardPending(hash(1))]);
}

#[test]
fn a_block_too_large_for_the_area_expires_its_need_and_its_waiters() {
    let mut s = state();
    let dag = dag_with(&[0]);
    s.handle(received(meta(10, 2, &[1])), 0, &dag);
    s.handle(
        IngestInput::IntakeRefused {
            hash: hash(1),
            from: Some(peer("q")),
            unsolicited: false,
            size: LIMITS.max_bytes + 1,
            sender: Bytes::from_static(b"v1"),
            reason: Refusal::Pending(PendingRefusal::TooLarge),
            area: None,
        },
        0,
        &dag,
    );
    assert!(!s.is_needed(&hash(1)) && !s.is_held(&hash(10)));
}

// ── Simulation ──────────────────────────────────────────────────────────────

/// A DAG of blocks above genesis (hash 0): block `i` cites the listed parents.
#[derive(Debug, Clone)]
struct Chain {
    parents: Vec<Vec<u32>>,
}

impl Chain {
    fn blocks(&self) -> u32 { self.parents.len() as u32 }

    fn parents_of(&self, i: u32) -> &[u32] { &self.parents[i as usize - 1] }

    fn height(&self, i: u32) -> i64 {
        if i == 0 {
            return 0;
        }
        1 + self
            .parents_of(i)
            .iter()
            .map(|p| self.height(*p))
            .max()
            .expect("every block has a parent")
    }
}

fn chains() -> impl Strategy<Value = Chain> {
    (1usize..40).prop_flat_map(|n| {
        (1..=n)
            .map(|i| proptest::collection::btree_set(0..i as u32, 1..=3.min(i)))
            .collect::<Vec<_>>()
            .prop_map(|parents| Chain {
                parents: parents
                    .into_iter()
                    .map(|set: std::collections::BTreeSet<u32>| set.into_iter().collect())
                    .collect(),
            })
    })
}

#[derive(Debug, Clone)]
struct Scenario {
    chain: Chain,
    gossip_order: Vec<u32>,
    drop_every: u32,
    restart_after: Option<usize>,
}

fn scenarios() -> impl Strategy<Value = Scenario> {
    chains().prop_flat_map(|chain| {
        let n = chain.blocks();
        (
            Just(chain),
            proptest::collection::vec(1..=n, 0..(2 * n as usize)),
            2u32..6,
            proptest::option::of(0usize..400),
        )
            .prop_map(
                |(chain, gossip_order, drop_every, restart_after)| Scenario {
                    chain,
                    gossip_order,
                    drop_every,
                    restart_after,
                },
            )
    })
}

struct Sim<'a> {
    chain: &'a Chain,
    state: IngestState,
    dag: Dag,
    disk: HashSet<BlockHash>,
    area: BTreeMap<BlockHash, bool>,
    inbox: VecDeque<(BlockHash, Option<PeerNode>, bool)>,
    peers: Vec<PeerNode>,
    requests: u32,
    drop_every: u32,
    withheld_until: HashMap<BlockHash, u64>,
    now: u64,
}

const WORKERS: usize = 3;

impl<'a> Sim<'a> {
    fn new(chain: &'a Chain, drop_every: u32) -> Self {
        Self {
            chain,
            state: state(),
            dag: dag_with(&[0]),
            disk: HashSet::new(),
            area: BTreeMap::new(),
            inbox: VecDeque::new(),
            peers: (0..6).map(|i| peer(&format!("peer{i}"))).collect(),
            requests: 0,
            drop_every,
            withheld_until: HashMap::new(),
            now: 0,
        }
    }

    fn index(hash: &BlockHash) -> u32 {
        u32::from_be_bytes(hash[..].try_into().expect("four bytes"))
    }

    fn usage(&self) -> PendingUsage {
        PendingUsage {
            blocks: self.area.len(),
            bytes: self.area.len() * 1_000,
            by_sender: HashMap::new(),
        }
    }

    fn intake(&mut self, h: BlockHash, from: Option<PeerNode>, from_disk: bool) {
        if self.dag.in_dag(&h) || self.state.view().is_held(&h) {
            return;
        }
        let i = Self::index(&h);
        let deps = self.chain.parents_of(i).to_vec();
        let mut m = meta(i, self.chain.height(i), &deps);
        let authorized = self.dag.in_dag(&hash(deps[0]));
        m.body = if from_disk || authorized {
            BodyAt::Disk
        } else {
            BodyAt::Pending
        };
        m.solicited = self.state.view().is_solicited(&h);
        m.pushed_by = if m.solicited { None } else { from };
        let area = match m.body {
            BodyAt::Disk => {
                self.disk.insert(h.clone());
                None
            }
            BodyAt::Pending => {
                self.area.insert(h.clone(), m.solicited);
                Some(self.usage())
            }
        };
        let effects =
            self.state
                .handle(IngestInput::Received { meta: m, area }, self.now, &self.dag);
        self.apply(effects);
    }

    fn apply(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::DiscardPending(h) => {
                    self.area.remove(&h);
                }
                Effect::Fetch(FetchAction::LookupLocal(h)) => {
                    if self.disk.contains(&h) {
                        self.inbox.push_back((h, None, true));
                    } else {
                        let effects = self.state.handle(
                            IngestInput::LocalMiss { hash: h },
                            self.now,
                            &self.dag,
                        );
                        self.apply(effects);
                    }
                }
                Effect::Fetch(FetchAction::RequestBlock { hash: h, peer }) => {
                    self.requests += 1;
                    if !self.requests.is_multiple_of(self.drop_every) && self.served(&h) {
                        self.inbox.push_back((h, Some(peer), false));
                    }
                }
                Effect::Fetch(FetchAction::BroadcastHasBlockRequest(h)) => {
                    if self.served(&h) {
                        let effects = self.state.handle(
                            IngestInput::Announced {
                                hash: h,
                                peer: self.peers[1].clone(),
                            },
                            self.now,
                            &self.dag,
                        );
                        self.apply(effects);
                    }
                }
                Effect::Fetch(FetchAction::FetchStateRoot(_)) | Effect::Struck { .. } => {}
            }
        }
    }

    fn served(&self, h: &BlockHash) -> bool {
        self.withheld_until
            .get(h)
            .is_none_or(|until| self.now >= *until)
    }

    fn work(&mut self) {
        for _ in 0..WORKERS {
            let Some((h, work)) = self.state.next_dispatch() else {
                return;
            };
            assert_eq!(work, Work::Validate);
            let i = Self::index(&h);
            for parent in self.chain.parents_of(i) {
                assert!(
                    self.dag.in_dag(&hash(*parent)),
                    "block {i} was dispatched before its parent {parent} was in the DAG"
                );
            }
            self.dag.0.insert(h.clone());
            self.disk.insert(h.clone());
            self.area.remove(&h);
            for input in [
                IngestInput::Inserted { hash: h.clone() },
                IngestInput::Outcome {
                    hash: h,
                    outcome: WorkOutcome::Settled,
                },
            ] {
                let effects = self.state.handle(input, self.now, &self.dag);
                self.apply(effects);
            }
        }
    }

    fn restart(&mut self) {
        self.state = state();
        self.area.clear();
        self.inbox.clear();
        let tips: Vec<u32> = (1..=self.chain.blocks())
            .filter(|i| {
                !(1..=self.chain.blocks()).any(|child| self.chain.parents_of(child).contains(i))
            })
            .collect();
        for tip in tips {
            let effects = self.state.handle(
                IngestInput::Announced {
                    hash: hash(tip),
                    peer: self.peers[0].clone(),
                },
                self.now,
                &self.dag,
            );
            self.apply(effects);
        }
    }

    fn step(&mut self) {
        let effects = self.state.handle(
            IngestInput::Tick {
                connected: self.peers.clone(),
                area: self.usage(),
            },
            self.now,
            &self.dag,
        );
        self.apply(effects);
        while let Some((h, from, from_disk)) = self.inbox.pop_front() {
            self.intake(h, from, from_disk);
        }
        self.work();
        self.check_invariants();
        self.now += 250;
    }

    fn check_invariants(&self) {
        for i in 0..=self.chain.blocks() {
            let h = hash(i);
            assert_eq!(
                self.state.is_held(&h),
                self.state.view().is_held(&h),
                "view held for {i}"
            );
            assert_eq!(
                self.state.is_needed(&h),
                self.state.view().is_awaited(&h),
                "view needed for {i}"
            );
            assert!(
                !(self.state.is_held(&h) && self.dag.in_dag(&h)),
                "{i} is held and in the DAG"
            );
            let held_pending = self
                .state
                .held_meta(&h)
                .is_some_and(|m| m.body == BodyAt::Pending);
            assert_eq!(
                held_pending,
                self.area.contains_key(&h),
                "area entry and held entry for {i}"
            );
        }
    }

    fn converged(&self) -> bool { (1..=self.chain.blocks()).all(|i| self.dag.in_dag(&hash(i))) }
}

fn all_tips_announced(sim: &mut Sim) {
    let n = sim.chain.blocks();
    for i in 1..=n {
        if !(1..=n).any(|child| sim.chain.parents_of(child).contains(&i)) {
            let effects = sim.state.handle(
                IngestInput::Announced {
                    hash: hash(i),
                    peer: sim.peers[2].clone(),
                },
                sim.now,
                &sim.dag,
            );
            sim.apply(effects);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 200, ..ProptestConfig::default() })]

    #[test]
    fn every_block_reachable_from_the_tips_ends_in_the_dag(scenario in scenarios()) {
        let mut sim = Sim::new(&scenario.chain, scenario.drop_every);
        for i in &scenario.gossip_order {
            sim.inbox.push_back((hash(*i), Some(sim.peers[3].clone()), false));
        }
        all_tips_announced(&mut sim);
        let mut steps = 0;
        while !sim.converged() {
            if scenario.restart_after == Some(steps) {
                sim.restart();
            }
            sim.step();
            steps += 1;
            prop_assert!(steps < 20_000, "no convergence after {} steps", steps);
        }
        prop_assert_eq!(sim.state.held(), 0);
    }

    #[test]
    fn a_long_withheld_ancestor_does_not_lose_its_waiting_chain(chain in chains()) {
        let mut sim = Sim::new(&chain, u32::MAX);
        let thirty_minutes = 30 * 60 * 1000;
        sim.withheld_until.insert(hash(1), thirty_minutes);
        for i in 2..=chain.blocks() {
            sim.inbox.push_back((hash(i), Some(sim.peers[3].clone()), false));
        }
        all_tips_announced(&mut sim);
        while sim.now < thirty_minutes + 60_000 && !sim.converged() {
            sim.step();
        }
        prop_assert!(sim.converged(), "not converged a minute after the ancestor was served");
    }
}
