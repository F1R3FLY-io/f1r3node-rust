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

#[derive(Default)]
struct Dag {
    blocks: HashSet<BlockHash>,
    area: PendingUsage,
    /// The bonds of every block in the DAG, the last finalized one included.
    bonds: HashSet<Bytes>,
    /// Pending bodies the area no longer holds; every other one it does.
    discarded: HashSet<BlockHash>,
}

impl StoreReads for Dag {
    fn in_dag(&self, hash: &BlockHash) -> bool { self.blocks.contains(hash) }

    fn is_satisfied(&self, hash: &BlockHash) -> bool { self.blocks.contains(hash) }

    fn pending_usage(&self) -> PendingUsage { self.area.clone() }

    fn holds_pending(&self, hash: &BlockHash) -> bool { !self.discarded.contains(hash) }

    fn bonded_in(&self, hash: &BlockHash) -> Option<HashSet<Bytes>> {
        self.blocks.contains(hash).then(|| self.bonds.clone())
    }

    fn bonded_in_last_finalized(&self) -> Option<HashSet<Bytes>> { Some(self.bonds.clone()) }
}

fn dag_with(hashes: &[u32]) -> Dag {
    Dag {
        blocks: hashes.iter().copied().map(hash).collect(),
        area: usage(0, HashMap::new()),
        bonds: HashSet::new(),
        discarded: HashSet::new(),
    }
}

fn usage(blocks: usize, by_sender: HashMap<Bytes, usize>) -> PendingUsage {
    PendingUsage {
        limits: LIMITS,
        blocks,
        bytes: 0,
        by_sender,
    }
}

fn meta(i: u32, height: i64, deps: &[u32]) -> HeldMeta {
    HeldMeta {
        hash: hash(i),
        sender: Bytes::from_static(b"v0"),
        height,
        deps: deps.iter().copied().map(hash).collect(),
        parents: deps.iter().copied().map(hash).collect(),
        justification_validators: Vec::new(),
        deferred: false,
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

fn received(meta: HeldMeta) -> IngestInput { IngestInput::Received(meta) }

/// Block `i` refused, declaring height 0: every held block is above it.
fn refused(
    i: u32,
    from: &str,
    solicited: bool,
    sender: &'static [u8],
    reason: Refusal,
) -> IngestInput {
    refused_at(i, 0, from, solicited, sender, reason)
}

fn refused_at(
    i: u32,
    height: i64,
    from: &str,
    solicited: bool,
    sender: &'static [u8],
    reason: Refusal,
) -> IngestInput {
    IngestInput::IntakeRefused(Refused {
        hash: hash(i),
        from: Some(peer(from)),
        unsolicited: !solicited,
        size: 10,
        sender: Bytes::from_static(sender),
        height,
        reason,
    })
}

fn reserve(i: u32, sender: &'static [u8]) -> Effect {
    Effect::Reserve {
        hash: hash(i),
        sender: Bytes::from_static(sender),
        size: 10,
    }
}

fn state() -> (IngestState, IngestView) {
    let view = IngestView::new();
    (IngestState::new(view.clone()), view)
}

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
        },
        now_ms,
        dag,
    )
}

// ── Transitions ─────────────────────────────────────────────────────────────

#[test]
fn a_block_with_missing_parents_waits_and_its_parents_become_solicited_needs() {
    let (mut s, view) = state();
    let dag = dag_with(&[0]);
    s.handle(received(meta(10, 5, &[0, 7, 8])), 0, &dag);
    assert!(s.is_held(&hash(10)) && view.is_held(&hash(10)));
    assert!(s.next_dispatch().is_none());
    for parent in [7, 8] {
        assert!(s.is_needed(&hash(parent)));
        assert!(view.is_solicited(&hash(parent)));
        assert!(view.is_awaited(&hash(parent)));
        assert!(view.is_cited(&hash(parent)));
    }
    assert!(!s.is_needed(&hash(0)));
}

#[test]
fn a_block_is_dispatched_once_its_parents_are_inserted() {
    let (mut s, _) = state();
    let mut dag = dag_with(&[0]);
    s.handle(received(meta(10, 2, &[7])), 0, &dag);
    s.handle(received(meta(7, 1, &[0])), 0, &dag);
    assert!(!s.is_needed(&hash(7)));
    assert_eq!(s.next_dispatch(), Some((hash(7), Work::Validate)));
    dag.blocks.insert(hash(7));
    s.handle(IngestInput::Inserted { hash: hash(7) }, 0, &dag);
    s.handle(
        IngestInput::Outcome {
            hash: hash(7),
            outcome: WorkOutcome::Settled,
        },
        0,
        &dag,
    );
    assert!(!s.is_held(&hash(7)));
    assert_eq!(s.next_dispatch(), Some((hash(10), Work::Validate)));
}

#[test]
fn a_block_received_twice_is_held_and_dispatched_once() {
    let (mut s, _) = state();
    let dag = dag_with(&[0]);
    s.handle(received(meta(1, 1, &[0])), 0, &dag);
    s.handle(received(meta(1, 1, &[0])), 0, &dag);
    assert_eq!(s.held(), 1);
    assert_eq!(s.next_dispatch(), Some((hash(1), Work::Validate)));
    s.handle(received(meta(1, 1, &[0])), 0, &dag);
    assert_eq!(
        s.next_dispatch(),
        None,
        "a block being validated is not dispatched again"
    );
}

#[test]
fn a_cited_block_at_or_below_the_anchor_is_admitted_as_settled_history() {
    let (mut s, _) = state();
    let dag = dag_with(&[0]);
    s.handle(IngestInput::Attached { anchor_height: 50 }, 0, &dag);
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
fn a_block_below_the_anchor_that_the_node_requested_itself_is_offered_as_settled_history() {
    let (mut s, _) = state();
    let dag = dag_with(&[0]);
    s.handle(IngestInput::Attached { anchor_height: 50 }, 0, &dag);
    s.handle(IngestInput::Need { hash: hash(9) }, 0, &dag);
    s.handle(received(meta(9, 40, &[0])), 0, &dag);
    assert_eq!(s.next_dispatch(), Some((hash(9), Work::AdmitSettled)));
}

#[test]
fn a_held_block_below_the_anchor_that_the_node_then_requests_is_admitted_as_settled_history() {
    let (mut s, _) = state();
    let dag = dag_with(&[0]);
    s.handle(IngestInput::Attached { anchor_height: 50 }, 0, &dag);
    s.handle(received(meta(9, 40, &[3])), 0, &dag);
    assert!(
        s.is_needed(&hash(3)) && s.next_dispatch().is_none(),
        "staging: received unvouched, the block waits on its parent to be validated"
    );
    s.handle(IngestInput::Need { hash: hash(9) }, 0, &dag);
    assert_eq!(s.next_dispatch(), Some((hash(9), Work::AdmitSettled)));
    assert!(
        !s.is_needed(&hash(3)),
        "settled history waits on nothing, so its parent is no longer fetched"
    );
}

#[test]
fn a_held_block_below_the_anchor_that_a_bonded_block_then_cites_is_admitted_as_settled_history() {
    let (mut s, _) = state();
    let dag = dag_with(&[0]);
    s.handle(IngestInput::Attached { anchor_height: 50 }, 0, &dag);
    s.handle(received(meta(9, 40, &[3])), 0, &dag);
    s.handle(received(meta(10, 41, &[9])), 0, &dag);
    assert_eq!(s.next_dispatch(), Some((hash(9), Work::AdmitSettled)));
}

#[test]
fn a_held_block_above_the_anchor_stays_held_for_validation_when_the_node_requests_it() {
    let (mut s, _) = state();
    let dag = dag_with(&[0]);
    s.handle(IngestInput::Attached { anchor_height: 50 }, 0, &dag);
    s.handle(received(meta(9, 51, &[0])), 0, &dag);
    s.handle(IngestInput::Need { hash: hash(9) }, 0, &dag);
    assert_eq!(s.next_dispatch(), Some((hash(9), Work::Validate)));
}

#[test]
fn no_block_is_admitted_as_settled_history_before_casper_is_attached() {
    let (mut s, _) = state();
    let dag = dag_with(&[0]);
    s.handle(received(meta(10, 41, &[9])), 0, &dag);
    s.handle(received(meta(9, 40, &[0])), 0, &dag);
    assert_eq!(s.next_dispatch(), Some((hash(9), Work::Validate)));
}

#[test]
fn an_announced_hash_is_awaited_but_not_solicited() {
    let (mut s, view) = state();
    let dag = dag_with(&[0]);
    s.handle(
        IngestInput::Announced {
            hash: hash(5),
            peer: peer("p"),
        },
        0,
        &dag,
    );
    assert!(view.is_awaited(&hash(5)));
    assert!(!view.is_solicited(&hash(5)));
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
fn a_message_that_fails_the_integrity_check_strikes_its_sender_and_touches_no_need() {
    let (mut s, view) = state();
    let dag = dag_with(&[0]);
    s.handle(IngestInput::Need { hash: hash(1) }, 0, &dag);
    for strike in [Strike::Malformed, Strike::HashMismatch] {
        let effects = s.handle(
            IngestInput::Misbehaved {
                peer: peer("p"),
                strike,
            },
            0,
            &dag,
        );
        assert_eq!(effects, vec![Effect::Struck {
            peer: peer("p"),
            strike,
            outcome: StrikeOutcome::Counted
        }]);
    }
    assert!(view.is_awaited(&hash(1)));
}

#[test]
fn the_sixteenth_strike_ignores_the_peer_in_the_view() {
    let (mut s, view) = state();
    let dag = dag_with(&[0]);
    for i in 0..STRIKE_THRESHOLD as u32 {
        s.handle(
            refused(i, "bad", false, b"v0", Refusal::Unauthorized),
            0,
            &dag,
        );
    }
    assert!(view.is_ignored_at(&peer("bad"), 0));
    assert!(!view.is_ignored_at(&peer("good"), 0));
}

#[test]
fn a_rejected_block_drops_what_waits_on_it_and_strikes_an_unsolicited_pusher() {
    let (mut s, _) = state();
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
    let (mut s, _) = state();
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
    let (mut s, view) = state();
    let mut dag = dag_with(&[0]);
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
    dag.area = usage(LIMITS.max_blocks, HashMap::new());
    assert!(
        !tick(&mut s, 0, &[], &dag).contains(&Effect::DiscardPending(hash(1))),
        "a block another held block waits on is not a proactive victim"
    );
    let effects = s.handle(
        refused(
            50,
            "q",
            true,
            b"v2",
            Refusal::Pending(PendingRefusal::AreaFull),
        ),
        0,
        &dag,
    );
    dag.area = usage(0, HashMap::new());
    assert!(effects.contains(&Effect::DiscardPending(hash(1))));
    assert!(s.is_needed(&hash(1)) && view.is_solicited(&hash(1)));
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
            .any(|e| matches!(e, Effect::Struck { peer: struck, .. } if *struck == peer("pusher"))),
        "the original pusher is struck: {effects:?}"
    );
}

#[test]
fn a_block_awaiting_a_state_root_is_released_when_it_is_imported() {
    let (mut s, _) = state();
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
    let (mut s, _) = state();
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
    let (mut s, _) = state();
    let dag = dag_with(&[0]);
    s.handle(received(meta(1, 1, &[0])), 0, &dag);
    let given_up_ms = fail_until_given_up(&mut s, 1, 0, &dag);
    assert!(!s.is_held(&hash(1)));
    s.handle(received(meta(1, 1, &[0])), given_up_ms + 1_000, &dag);
    assert!(!s.is_held(&hash(1)), "quarantined");
    s.handle(
        received(meta(1, 1, &[0])),
        given_up_ms + super::dependency_tracker::VALIDATION_FAILURE_QUARANTINE_MS,
        &dag,
    );
    assert!(s.is_held(&hash(1)));
}

/// Dispatch block `i` and fail it until the owner gives up on it, waiting out
/// each retry delay. Returns the time of the last failure.
fn fail_until_given_up(s: &mut IngestState, i: u32, start_ms: u64, dag: &Dag) -> u64 {
    use super::dependency_tracker::{validation_retry_delay_ms, MAX_VALIDATION_FAILURES};
    let mut now_ms = start_ms;
    for failures in 1..=MAX_VALIDATION_FAILURES {
        assert_eq!(s.next_dispatch(), Some((hash(i), Work::Validate)));
        s.handle(
            IngestInput::Outcome {
                hash: hash(i),
                outcome: WorkOutcome::Failed,
            },
            now_ms,
            dag,
        );
        if failures < MAX_VALIDATION_FAILURES {
            now_ms += validation_retry_delay_ms(failures);
            tick(s, now_ms, &[], dag);
        }
    }
    now_ms
}

#[test]
fn a_need_with_a_waiter_is_asked_for_until_a_peer_serves_it() {
    let (mut s, _) = state();
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
    let (mut s, _) = state();
    let mut dag = dag_with(&[0]);
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
    dag.area = usage(LIMITS.max_blocks, HashMap::new());
    let discarded: HashSet<BlockHash> = tick(&mut s, 0, &[], &dag)
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
    let (mut s, _) = state();
    let mut dag = dag_with(&[0]);
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
    dag.area = usage(LIMITS.max_blocks, HashMap::new());
    let area_full = || {
        refused(
            50,
            "q",
            true,
            b"v2",
            Refusal::Pending(PendingRefusal::AreaFull),
        )
    };
    assert_eq!(s.handle(area_full(), 0, &dag), vec![
        reserve(50, b"v2"),
        Effect::DiscardPending(hash(2))
    ]);
    assert_eq!(s.handle(area_full(), 0, &dag), vec![
        Effect::DiscardPending(hash(1))
    ]);
}

#[test]
fn a_sender_at_its_quota_loses_only_its_own_unsolicited_blocks() {
    let (mut s, _) = state();
    let mut dag = dag_with(&[0]);
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
    dag.area = usage(
        3,
        [(Bytes::from_static(b"v1"), LIMITS.max_per_sender)].into(),
    );
    let effects = s.handle(
        refused(
            50,
            "q",
            true,
            b"v1",
            Refusal::Pending(PendingRefusal::SenderLimitReached),
        ),
        0,
        &dag,
    );
    assert_eq!(effects, vec![
        reserve(50, b"v1"),
        Effect::DiscardPending(hash(1))
    ]);
}

/// A full area holding block 5, requested at height 5 and waiting on block 4.
fn full_area_holding_a_waiting_child() -> (IngestState, Dag) {
    let (mut s, _) = state();
    let mut dag = dag_with(&[0]);
    s.handle(
        received(pending(meta(5, 5, &[4]), b"v1", true, None)),
        0,
        &dag,
    );
    dag.area = usage(LIMITS.max_blocks, HashMap::new());
    (s, dag)
}

fn area_full_at(block: u32, height: i64) -> IngestInput {
    refused_at(
        block,
        height,
        "q",
        true,
        b"v1",
        Refusal::Pending(PendingRefusal::AreaFull),
    )
}

#[test]
fn the_room_made_for_a_refused_block_is_reserved_before_it_is_freed() {
    let (mut s, dag) = full_area_holding_a_waiting_child();
    assert_eq!(s.handle(area_full_at(4, 4), 0, &dag), vec![
        reserve(4, b"v1"),
        Effect::DiscardPending(hash(5)),
    ]);
    assert!(s.is_needed(&hash(4)) && s.is_needed(&hash(5)));
}

#[test]
fn a_refused_block_takes_no_room_from_a_lower_requested_block() {
    let (mut s, dag) = full_area_holding_a_waiting_child();
    assert_eq!(s.handle(area_full_at(9, 9), 0, &dag), vec![]);
    assert!(s.is_held(&hash(5)));
    assert!(
        s.is_needed(&hash(9)),
        "the refused block is asked for again"
    );
}

#[test]
fn a_reservation_is_released_when_its_block_arrives() {
    let (mut s, dag) = full_area_holding_a_waiting_child();
    s.handle(area_full_at(4, 4), 0, &dag);
    let effects = s.handle(
        received(pending(meta(4, 4, &[0]), b"v1", true, None)),
        0,
        &dag,
    );
    assert!(effects.contains(&Effect::ReleaseReservation(hash(4))));
    assert!(s.is_held(&hash(4)));
}

#[test]
fn a_reservation_is_released_when_its_need_ends() {
    let (mut s, mut dag) = full_area_holding_a_waiting_child();
    s.handle(area_full_at(4, 4), 0, &dag);
    dag.blocks.insert(hash(4));
    let effects = s.handle(IngestInput::Inserted { hash: hash(4) }, 0, &dag);
    assert!(effects.contains(&Effect::ReleaseReservation(hash(4))));
    assert!(!s.is_needed(&hash(4)));
}

/// A requested pending block of sender `v1` that waits on `dependency`.
fn requested(block: u32, height: i64, dependency: u32) -> HeldMeta {
    pending(meta(block, height, &[dependency]), b"v1", true, None)
}

/// Fill sender `v1`'s quota with `blocks`.
fn fill_sender_quota(s: &mut IngestState, dag: &mut Dag, blocks: Vec<HeldMeta>) {
    assert_eq!(blocks.len(), LIMITS.max_per_sender);
    for block in blocks {
        s.handle(received(block), 0, dag);
    }
    dag.area = usage(
        LIMITS.max_per_sender,
        [(Bytes::from_static(b"v1"), LIMITS.max_per_sender)].into(),
    );
}

fn refused_at_quota(s: &mut IngestState, dag: &Dag, block: u32) -> Vec<Effect> {
    s.handle(
        refused(
            block,
            "q",
            true,
            b"v1",
            Refusal::Pending(PendingRefusal::SenderLimitReached),
        ),
        0,
        dag,
    )
}

#[test]
fn a_requested_parent_at_its_sender_quota_makes_room_among_its_own_requested_children() {
    let (mut s, _) = state();
    let mut dag = dag_with(&[0]);
    let quota = LIMITS.max_per_sender as u32;
    let children = (1..=quota).map(|child| requested(child, 2, 100)).collect();
    fill_sender_quota(&mut s, &mut dag, children);
    assert!(s.is_needed(&hash(100)));

    let effects = refused_at_quota(&mut s, &dag, 100);

    assert_eq!(effects.len(), 2, "one slot is missing: {effects:?}");
    assert_eq!(effects[0], reserve(100, b"v1"));
    assert!(
        matches!(
            &effects[1],
            Effect::DiscardPending(child) if (1..=quota).map(hash).any(|c| c == *child)
        ),
        "the parent's own children hold every slot of its sender: {effects:?}"
    );
    assert!(s.is_needed(&hash(100)));
}

#[test]
fn a_requested_block_at_its_sender_quota_takes_no_room_from_blocks_that_do_not_wait_on_it() {
    let (mut s, _) = state();
    let mut dag = dag_with(&[0]);
    let quota = LIMITS.max_per_sender as u32;
    let unrelated = (1..=quota).map(|block| requested(block, 2, 99)).collect();
    fill_sender_quota(&mut s, &mut dag, unrelated);

    assert_eq!(refused_at_quota(&mut s, &dag, 100), vec![]);
}

#[test]
fn a_requested_parent_displacing_its_only_waiter_keeps_both_needed() {
    let (mut s, _) = state();
    let mut dag = dag_with(&[0]);
    let quota = LIMITS.max_per_sender as u32;
    // The parent's only waiter is the only block it may displace; the rest of
    // the sender's quota does not wait on it.
    let mut blocks = vec![requested(1, 2, 100)];
    blocks.extend((2..=quota).map(|block| requested(block, 2, 99)));
    fill_sender_quota(&mut s, &mut dag, blocks);

    let effects = refused_at_quota(&mut s, &dag, 100);

    assert_eq!(effects, vec![
        reserve(100, b"v1"),
        Effect::DiscardPending(hash(1))
    ]);
    assert!(
        s.is_needed(&hash(100)),
        "the parent stays needed after its only waiter left"
    );
    assert!(
        s.is_needed(&hash(1)),
        "the displaced child is fetched again once its parent has moved on"
    );
}

#[test]
fn a_block_too_large_for_the_area_expires_its_need_and_its_waiters() {
    let (mut s, view) = state();
    let dag = dag_with(&[0]);
    s.handle(received(meta(10, 2, &[1])), 0, &dag);
    s.handle(
        refused(
            1,
            "q",
            true,
            b"v1",
            Refusal::Pending(PendingRefusal::TooLarge),
        ),
        0,
        &dag,
    );
    assert!(!s.is_needed(&hash(1)) && !s.is_held(&hash(10)));
    assert!(!view.is_awaited(&hash(1)) && !view.is_cited(&hash(1)));
}

fn network_fetches_for(effects: &[Effect], h: &BlockHash) -> usize {
    fetches(effects)
        .iter()
        .filter(|action| match action {
            FetchAction::RequestBlock { hash, .. } => hash == h,
            FetchAction::BroadcastHasBlockRequest(hash) => hash == h,
            _ => false,
        })
        .count()
}

/// Blocks recorded as equivocations: satisfied, not in the DAG.
struct Recorded {
    dag: Dag,
    recorded: HashSet<BlockHash>,
}

impl StoreReads for Recorded {
    fn in_dag(&self, hash: &BlockHash) -> bool { self.dag.in_dag(hash) }

    fn is_satisfied(&self, hash: &BlockHash) -> bool {
        self.dag.in_dag(hash) || self.recorded.contains(hash)
    }

    fn pending_usage(&self) -> PendingUsage { self.dag.pending_usage() }

    fn holds_pending(&self, hash: &BlockHash) -> bool { self.dag.holds_pending(hash) }

    fn bonded_in(&self, hash: &BlockHash) -> Option<HashSet<Bytes>> { self.dag.bonded_in(hash) }

    fn bonded_in_last_finalized(&self) -> Option<HashSet<Bytes>> {
        self.dag.bonded_in_last_finalized()
    }
}

#[test]
fn a_failed_validation_waits_before_its_retry() {
    use super::dependency_tracker::validation_retry_delay_ms;
    let (mut s, _) = state();
    let dag = dag_with(&[0]);
    s.handle(received(meta(1, 1, &[0])), 0, &dag);
    assert_eq!(s.next_dispatch(), Some((hash(1), Work::Validate)));
    s.handle(
        IngestInput::Outcome {
            hash: hash(1),
            outcome: WorkOutcome::Failed,
        },
        0,
        &dag,
    );
    let delay = validation_retry_delay_ms(1);
    tick(&mut s, delay - 1, &[], &dag);
    assert!(s.next_dispatch().is_none());
    tick(&mut s, delay, &[], &dag);
    assert_eq!(s.next_dispatch(), Some((hash(1), Work::Validate)));
}

#[test]
fn giving_up_on_a_failing_block_keeps_its_waiters_and_asks_for_it_after_the_quarantine() {
    use super::dependency_tracker::VALIDATION_FAILURE_QUARANTINE_MS;
    let (mut s, _) = state();
    let dag = dag_with(&[0]);
    let peers = [peer("a")];
    s.handle(received(meta(1, 1, &[0])), 0, &dag);
    s.handle(received(meta(2, 2, &[1])), 0, &dag);
    s.handle(received(meta(3, 3, &[2])), 0, &dag);
    let given_up_ms = fail_until_given_up(&mut s, 1, 0, &dag);
    assert!(!s.is_held(&hash(1)));
    assert!(s.is_held(&hash(2)) && s.is_held(&hash(3)));
    assert!(s.is_needed(&hash(1)));
    let until_ms = given_up_ms + VALIDATION_FAILURE_QUARANTINE_MS;
    let early = tick(&mut s, until_ms - 1, &peers, &dag);
    assert_eq!(network_fetches_for(&early, &hash(1)), 0);
    let due = tick(&mut s, until_ms, &peers, &dag);
    assert!(network_fetches_for(&due, &hash(1)) > 0);
}

#[test]
fn a_per_sender_pass_counts_blocks_the_global_pass_already_evicted() {
    let (mut s, _) = state();
    let mut dag = dag_with(&[0]);
    for i in 1..=50 {
        let mut m = meta(i, 1, &[99]);
        m.trusted = false;
        s.handle(received(pending(m, b"v1", false, Some(peer("p")))), 0, &dag);
    }
    dag.area = usage(90, [(Bytes::from_static(b"v1"), 50)].into());
    let discarded = tick(&mut s, 0, &[], &dag)
        .iter()
        .filter(|e| matches!(e, Effect::DiscardPending(_)))
        .count();
    assert_eq!(
        discarded, 14,
        "the global pass to 80 takes 10, leaving v1 at 40; its mark is 36, so 4 more"
    );
}

#[test]
fn a_quarantined_local_copy_moves_its_need_to_the_network_after_the_quarantine() {
    use super::dependency_tracker::VALIDATION_FAILURE_QUARANTINE_MS;
    let (mut s, _) = state();
    let dag = dag_with(&[0]);
    let peers = [peer("a")];
    s.handle(received(meta(1, 1, &[0])), 0, &dag);
    let given_up_ms = fail_until_given_up(&mut s, 1, 0, &dag);
    let now_ms = given_up_ms + 1_000;
    s.handle(received(meta(2, 2, &[1])), now_ms, &dag);
    assert_eq!(fetches(&tick(&mut s, now_ms, &peers, &dag)), vec![
        FetchAction::LookupLocal(hash(1))
    ]);
    s.handle(received(meta(1, 1, &[0])), now_ms, &dag);
    assert!(!s.is_held(&hash(1)), "still quarantined");
    let due = tick(
        &mut s,
        given_up_ms + VALIDATION_FAILURE_QUARANTINE_MS,
        &peers,
        &dag,
    );
    assert!(network_fetches_for(&due, &hash(1)) > 0);
}

#[test]
fn a_local_copy_refused_without_a_peer_moves_its_need_to_the_network() {
    let (mut s, _) = state();
    let dag = dag_with(&[0]);
    let peers = [peer("a")];
    s.handle(received(meta(10, 2, &[5])), 0, &dag);
    assert_eq!(fetches(&tick(&mut s, 0, &peers, &dag)), vec![
        FetchAction::LookupLocal(hash(5))
    ]);
    s.handle(
        IngestInput::IntakeRefused(Refused {
            hash: hash(5),
            from: None,
            unsolicited: false,
            size: 10,
            sender: Bytes::from_static(b"v0"),
            height: 1,
            reason: Refusal::NotOfInterest,
        }),
        0,
        &dag,
    );
    assert!(network_fetches_for(&tick(&mut s, 1, &peers, &dag), &hash(5)) > 0);
}

#[test]
fn a_validation_that_keeps_naming_a_dependency_in_the_dag_is_bounded() {
    use super::dependency_tracker::MAX_VALIDATION_FAILURES;
    let (mut s, _) = state();
    let dag = dag_with(&[0, 5]);
    s.handle(received(meta(1, 1, &[0])), 0, &dag);
    for _ in 0..MAX_VALIDATION_FAILURES {
        assert_eq!(s.next_dispatch(), Some((hash(1), Work::Validate)));
        s.handle(
            IngestInput::Outcome {
                hash: hash(1),
                outcome: WorkOutcome::AwaitingBlock(hash(5)),
            },
            0,
            &dag,
        );
    }
    assert!(!s.is_held(&hash(1)));
    assert!(s.next_dispatch().is_none());
}

#[test]
fn naming_a_recorded_block_leaves_that_block_with_its_worker() {
    let (mut s, _) = state();
    let stores = Recorded {
        dag: dag_with(&[0]),
        recorded: [hash(1)].into(),
    };
    s.handle(
        received(pending(meta(1, 1, &[0]), b"v1", false, Some(peer("p")))),
        0,
        &stores,
    );
    assert_eq!(s.next_dispatch(), Some((hash(1), Work::Validate)));
    s.handle(received(meta(2, 2, &[0])), 0, &stores);
    assert_eq!(s.next_dispatch(), Some((hash(2), Work::Validate)));
    let effects = s.handle(
        IngestInput::Outcome {
            hash: hash(2),
            outcome: WorkOutcome::AwaitingBlock(hash(1)),
        },
        0,
        &stores,
    );
    assert!(s.is_held(&hash(1)));
    assert!(!effects.contains(&Effect::DiscardPending(hash(1))));
    s.handle(received(meta(1, 1, &[0])), 0, &stores);
    s.handle(
        IngestInput::Outcome {
            hash: hash(1),
            outcome: WorkOutcome::AwaitingState(Blake2b256Hash::new(b"r")),
        },
        0,
        &stores,
    );
    assert!(s.is_held(&hash(1)));
}

#[test]
fn a_late_refusal_for_a_block_in_the_dag_leaves_no_need() {
    let (mut s, _) = state();
    let mut dag = dag_with(&[0]);
    s.handle(IngestInput::Need { hash: hash(1) }, 0, &dag);
    s.handle(
        received(pending(meta(1, 1, &[0]), b"v1", true, None)),
        0,
        &dag,
    );
    assert_eq!(s.next_dispatch(), Some((hash(1), Work::Validate)));
    dag.blocks.insert(hash(1));
    s.handle(IngestInput::Inserted { hash: hash(1) }, 0, &dag);
    s.handle(
        refused(
            1,
            "q",
            true,
            b"v1",
            Refusal::Pending(PendingRefusal::AreaFull),
        ),
        0,
        &dag,
    );
    assert!(!s.is_needed(&hash(1)));
}

#[test]
fn a_sender_flooding_the_disk_bound_cannot_evict_other_senders_blocks() {
    use super::dependency_tracker::MAX_DISK_ENTRIES;
    let (mut s, _) = state();
    let dag = dag_with(&[0]);
    for i in 0..MAX_DISK_ENTRIES as u32 {
        let mut flood = meta(1_000_000 + i, 1, &[0, 3_000_000 + i]);
        flood.sender = Bytes::from_static(b"byzantine");
        s.handle(received(flood), 0, &dag);
    }
    for (k, height) in [(10u32, 100i64), (11, 101), (12, 102)] {
        s.handle(received(meta(k, height, &[0])), 0, &dag);
        assert!(s.is_held(&hash(k)), "honest block {k} was evicted");
    }
    let dispatched: HashSet<BlockHash> = std::iter::from_fn(|| s.next_dispatch())
        .map(|(h, _)| h)
        .collect();
    assert_eq!(dispatched, [10, 11, 12].map(hash).into());
    assert_eq!(s.disk_entries(), MAX_DISK_ENTRIES);
}

#[test]
fn a_block_that_leaves_for_good_has_its_stored_body_deleted() {
    let (mut s, _) = state();
    let dag = dag_with(&[0]);
    s.handle(received(meta(1, 1, &[0])), 0, &dag);
    assert_eq!(s.next_dispatch(), Some((hash(1), Work::Validate)));
    let effects = s.handle(
        IngestInput::Outcome {
            hash: hash(1),
            outcome: WorkOutcome::Rejected {
                invalid_sender: false,
            },
        },
        0,
        &dag,
    );
    assert!(effects.contains(&Effect::DiscardStored(hash(1))));
}

#[test]
fn a_stored_body_that_a_held_block_still_waits_on_is_kept() {
    let (mut s, _) = state();
    let dag = dag_with(&[0]);
    s.handle(received(meta(1, 1, &[0])), 0, &dag);
    s.handle(received(meta(2, 2, &[1])), 0, &dag);
    let mut now_ms = 0;
    let mut effects = Vec::new();
    for failures in 1..=super::dependency_tracker::MAX_VALIDATION_FAILURES {
        assert_eq!(s.next_dispatch(), Some((hash(1), Work::Validate)));
        effects = s.handle(
            IngestInput::Outcome {
                hash: hash(1),
                outcome: WorkOutcome::Failed,
            },
            now_ms,
            &dag,
        );
        now_ms += super::dependency_tracker::validation_retry_delay_ms(failures);
        tick(&mut s, now_ms, &[], &dag);
    }
    assert!(!s.is_held(&hash(1)) && s.is_needed(&hash(1)));
    assert!(!effects.contains(&Effect::DiscardStored(hash(1))));
}

#[test]
fn a_report_for_a_pending_body_already_discarded_is_not_held() {
    let (mut s, _) = state();
    let mut dag = dag_with(&[0]);
    s.handle(received(meta(20, 3, &[1])), 0, &dag);
    let one = || received(pending(meta(1, 2, &[2]), b"v1", true, None));
    s.handle(one(), 0, &dag);
    dag.area = usage(LIMITS.max_blocks, HashMap::new());
    let effects = s.handle(
        refused(
            50,
            "q",
            true,
            b"v2",
            Refusal::Pending(PendingRefusal::AreaFull),
        ),
        0,
        &dag,
    );
    assert!(effects.contains(&Effect::DiscardPending(hash(1))));
    dag.discarded.insert(hash(1));
    s.handle(one(), 0, &dag);
    assert!(!s.is_held(&hash(1)));
    assert!(s.is_needed(&hash(1)));
}

#[test]
fn a_block_inserted_while_validated_releases_its_waiters_after_its_outcome() {
    let (mut s, _) = state();
    let mut dag = dag_with(&[0]);
    s.handle(received(meta(1, 1, &[0])), 0, &dag);
    s.handle(received(meta(2, 2, &[1])), 0, &dag);
    assert_eq!(s.next_dispatch(), Some((hash(1), Work::Validate)));
    dag.blocks.insert(hash(1));
    s.handle(IngestInput::Inserted { hash: hash(1) }, 0, &dag);
    assert!(
        s.next_dispatch().is_none(),
        "the child waits for the parent's outcome"
    );
    s.handle(
        IngestInput::Outcome {
            hash: hash(1),
            outcome: WorkOutcome::Settled,
        },
        0,
        &dag,
    );
    assert!(!s.is_held(&hash(1)));
    assert_eq!(s.next_dispatch(), Some((hash(2), Work::Validate)));
}

/// Per-block bonds and the last finalized block's bonds.
struct Bonds {
    dag: Dag,
    by_block: HashMap<BlockHash, HashSet<Bytes>>,
    last_finalized: HashSet<Bytes>,
}

impl StoreReads for Bonds {
    fn in_dag(&self, hash: &BlockHash) -> bool { self.dag.in_dag(hash) }

    fn is_satisfied(&self, hash: &BlockHash) -> bool { self.dag.is_satisfied(hash) }

    fn pending_usage(&self) -> PendingUsage { self.dag.pending_usage() }

    fn holds_pending(&self, hash: &BlockHash) -> bool { self.dag.holds_pending(hash) }

    fn bonded_in(&self, hash: &BlockHash) -> Option<HashSet<Bytes>> {
        self.dag
            .in_dag(hash)
            .then(|| self.by_block.get(hash).cloned().unwrap_or_default())
    }

    fn bonded_in_last_finalized(&self) -> Option<HashSet<Bytes>> {
        Some(self.last_finalized.clone())
    }
}

fn validator(i: u32) -> Bytes {
    Bytes::copy_from_slice(&[b"val-".as_slice(), &i.to_be_bytes()].concat())
}

/// Block `i` with main parent `main_parent` and one justification per named
/// validator, each pointing at block `first_justification + k`.
fn shaped(i: u32, main_parent: u32, validators: &[u32], first_justification: u32) -> HeldMeta {
    let justifications: Vec<u32> = (0..validators.len() as u32)
        .map(|k| first_justification + k)
        .collect();
    let mut deps: Vec<u32> = justifications.clone();
    deps.push(main_parent);
    let mut m = meta(i, 10, &deps);
    m.parents = vec![hash(main_parent)];
    m.justification_validators = validators.iter().copied().map(validator).collect();
    m
}

fn bonds_of(validators: &[u32]) -> HashSet<Bytes> {
    validators.iter().copied().map(validator).collect()
}

#[test]
fn a_block_naming_validators_bonded_in_the_last_finalized_block_cites_every_dependency_at_once() {
    let (mut s, _) = state();
    let stores = Bonds {
        dag: dag_with(&[0]),
        by_block: HashMap::new(),
        last_finalized: bonds_of(&[1, 2, 3]),
    };
    s.handle(received(shaped(100, 50, &[1, 2, 3], 60)), 0, &stores);
    assert_eq!(
        s.block_needs(),
        4,
        "the main parent and three justifications"
    );
}

#[test]
fn a_block_naming_validators_outside_the_bonds_cites_only_its_main_parent() {
    let (mut s, _) = state();
    let stores = Bonds {
        dag: dag_with(&[0]),
        by_block: HashMap::new(),
        last_finalized: bonds_of(&[1, 2, 3]),
    };
    let fabricated: Vec<u32> = (1_000..201_000).collect();
    s.handle(
        received(shaped(100, 50, &fabricated, 1_000_000)),
        0,
        &stores,
    );
    assert_eq!(s.block_needs(), 1);
    assert!(s.is_needed(&hash(50)));
}

#[test]
fn a_deferred_block_cites_the_rest_once_its_main_parent_shows_it_fits() {
    let (mut s, _) = state();
    let mut stores = Bonds {
        dag: dag_with(&[0]),
        by_block: [(hash(50), bonds_of(&[1, 2, 3, 4]))].into(),
        last_finalized: bonds_of(&[1, 2, 3]),
    };
    s.handle(received(shaped(100, 50, &[1, 2, 3, 4], 60)), 0, &stores);
    assert_eq!(
        s.block_needs(),
        1,
        "validator 4 is not bonded in the last finalized block"
    );
    stores.dag.blocks.insert(hash(50));
    s.handle(IngestInput::Inserted { hash: hash(50) }, 0, &stores);
    assert_eq!(s.block_needs(), 4);
    assert!(
        s.next_dispatch().is_none(),
        "the block waits on its justifications"
    );
}

#[test]
fn a_justification_a_deferred_block_names_is_cited_while_it_is_validated() {
    let (mut s, view) = state();
    let stores = Bonds {
        dag: dag_with(&[0]),
        by_block: HashMap::new(),
        last_finalized: bonds_of(&[1, 2, 3]),
    };
    s.handle(received(shaped(100, 50, &[1, 2, 3, 4], 60)), 0, &stores);
    s.handle(received(meta(60, 9, &[0])), 0, &stores);
    assert_eq!(s.next_dispatch(), Some((hash(60), Work::Validate)));
    assert!(
        view.is_cited(&hash(60)),
        "the deferred block waits only on its main parent but names 60"
    );
}

#[test]
fn a_block_whose_validators_do_not_match_its_main_parent_goes_to_validation() {
    let (mut s, _) = state();
    let stores = Bonds {
        dag: dag_with(&[0, 50]),
        by_block: [(hash(50), bonds_of(&[1, 2, 3]))].into(),
        last_finalized: bonds_of(&[1, 2, 3]),
    };
    s.handle(received(shaped(100, 50, &[1, 2, 2], 60)), 0, &stores);
    assert_eq!(s.block_needs(), 0);
    assert_eq!(s.next_dispatch(), Some((hash(100), Work::Validate)));
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

/// The node's stores as the simulation sees them: the DAG and the pending area.
struct SimStores<'a> {
    dag: &'a HashSet<BlockHash>,
    area: &'a BTreeMap<BlockHash, bool>,
}

impl StoreReads for SimStores<'_> {
    fn in_dag(&self, hash: &BlockHash) -> bool { self.dag.contains(hash) }

    fn is_satisfied(&self, hash: &BlockHash) -> bool { self.dag.contains(hash) }

    fn pending_usage(&self) -> PendingUsage {
        PendingUsage {
            limits: LIMITS,
            blocks: self.area.len(),
            bytes: self.area.len() * 1_000,
            by_sender: HashMap::new(),
        }
    }

    fn holds_pending(&self, hash: &BlockHash) -> bool { self.area.contains_key(hash) }

    fn bonded_in(&self, hash: &BlockHash) -> Option<HashSet<Bytes>> {
        self.dag.contains(hash).then(HashSet::new)
    }

    fn bonded_in_last_finalized(&self) -> Option<HashSet<Bytes>> { Some(HashSet::new()) }
}

struct Sim<'a> {
    chain: &'a Chain,
    state: IngestState,
    view: IngestView,
    dag: HashSet<BlockHash>,
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
        let (state, view) = state();
        Self {
            chain,
            state,
            view,
            dag: [hash(0)].into(),
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

    fn handle(&mut self, input: IngestInput) {
        let stores = SimStores {
            dag: &self.dag,
            area: &self.area,
        };
        let effects = self.state.handle(input, self.now, &stores);
        self.apply(effects);
    }

    fn intake(&mut self, h: BlockHash, from: Option<PeerNode>, from_disk: bool) {
        if self.dag.contains(&h) || self.view.is_held(&h) {
            return;
        }
        let i = Self::index(&h);
        let deps = self.chain.parents_of(i).to_vec();
        let mut m = meta(i, self.chain.height(i), &deps);
        let authorized = self.dag.contains(&hash(deps[0]));
        m.body = if from_disk || authorized {
            BodyAt::Disk
        } else {
            BodyAt::Pending
        };
        m.solicited = self.view.is_solicited(&h);
        m.pushed_by = if m.solicited { None } else { from };
        match m.body {
            BodyAt::Disk => {
                self.disk.insert(h.clone());
            }
            BodyAt::Pending => {
                self.area.insert(h.clone(), m.solicited);
            }
        }
        self.handle(IngestInput::Received(m));
    }

    fn apply(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::DiscardPending(h) => {
                    self.area.remove(&h);
                }
                Effect::DiscardStored(h) => {
                    if !self.dag.contains(&h) {
                        self.disk.remove(&h);
                    }
                }
                Effect::Fetch(FetchAction::LookupLocal(h)) => {
                    if self.disk.contains(&h) {
                        self.inbox.push_back((h, None, true));
                    } else {
                        self.handle(IngestInput::LocalMiss { hash: h });
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
                        let peer = self.peers[1].clone();
                        self.handle(IngestInput::Announced { hash: h, peer });
                    }
                }
                // Intake here never refuses a block for room.
                Effect::Reserve { .. }
                | Effect::ReleaseReservation(_)
                | Effect::Fetch(FetchAction::FetchStateRoot(_))
                | Effect::Struck { .. } => {}
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
                    self.dag.contains(&hash(*parent)),
                    "block {i} was dispatched before its parent {parent} was in the DAG"
                );
            }
            self.dag.insert(h.clone());
            self.disk.insert(h.clone());
            self.area.remove(&h);
            self.handle(IngestInput::Inserted { hash: h.clone() });
            self.handle(IngestInput::Outcome {
                hash: h,
                outcome: WorkOutcome::Settled,
            });
        }
    }

    fn restart(&mut self) {
        let (state, view) = state();
        self.state = state;
        self.view = view;
        self.area.clear();
        self.inbox.clear();
        let tips: Vec<u32> = (1..=self.chain.blocks())
            .filter(|i| {
                !(1..=self.chain.blocks()).any(|child| self.chain.parents_of(child).contains(i))
            })
            .collect();
        for tip in tips {
            let peer = self.peers[0].clone();
            self.handle(IngestInput::Announced {
                hash: hash(tip),
                peer,
            });
        }
    }

    fn step(&mut self) {
        let connected = self.peers.clone();
        self.handle(IngestInput::Tick { connected });
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
                self.view.is_held(&h),
                "view held for {i}"
            );
            assert_eq!(
                self.state.is_needed(&h),
                self.view.is_awaited(&h),
                "view needed for {i}"
            );
            assert!(
                !(self.state.is_held(&h) && self.dag.contains(&h)),
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

    fn converged(&self) -> bool { (1..=self.chain.blocks()).all(|i| self.dag.contains(&hash(i))) }
}

fn all_tips_announced(sim: &mut Sim) {
    let n = sim.chain.blocks();
    for i in 1..=n {
        if !(1..=n).any(|child| sim.chain.parents_of(child).contains(&i)) {
            let peer = sim.peers[2].clone();
            sim.handle(IngestInput::Announced {
                hash: hash(i),
                peer,
            });
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
