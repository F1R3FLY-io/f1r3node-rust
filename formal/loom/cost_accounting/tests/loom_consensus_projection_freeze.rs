use loom::sync::atomic::{AtomicBool, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy)]
struct DagConsensusView {
    floor: usize,
    latest: usize,
}

#[derive(Clone, Copy)]
struct ParentFrontierView {
    version: usize,
    exact_parent_count: usize,
}

#[derive(Clone, Copy)]
struct TargetConsensusView {
    lfb_height: usize,
    lfb_revision: usize,
    target_status: TargetStatus,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TargetStatus {
    Pending,
    Finalized,
    Failed,
    Expired,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TargetWaitOutcome {
    Waiting,
    Succeeded,
    TerminalError,
    HistoryCorruption,
    TimedOut,
}

#[derive(Clone, Copy)]
struct TargetWaitView {
    baseline_known: bool,
    observed_height: usize,
    observed_revision: usize,
    progress_renewals: usize,
    now: usize,
    last_progress_at: usize,
    stall_timeout: usize,
    absolute_deadline: usize,
    completed_at: Option<usize>,
    outcome: TargetWaitOutcome,
}

fn observe_target(consensus: TargetConsensusView, wait: &mut TargetWaitView) {
    if wait.outcome != TargetWaitOutcome::Waiting {
        return;
    }
    if wait.now >= wait.absolute_deadline
        || wait.now.saturating_sub(wait.last_progress_at) >= wait.stall_timeout
    {
        wait.outcome = TargetWaitOutcome::TimedOut;
        wait.completed_at = Some(wait.now);
        return;
    }
    match consensus.target_status {
        TargetStatus::Finalized => {
            wait.outcome = TargetWaitOutcome::Succeeded;
            wait.completed_at = Some(wait.now);
            return;
        }
        TargetStatus::Failed | TargetStatus::Expired => {
            wait.outcome = TargetWaitOutcome::TerminalError;
            wait.completed_at = Some(wait.now);
            return;
        }
        TargetStatus::Pending => {}
    }
    if !wait.baseline_known {
        wait.baseline_known = true;
        wait.observed_height = consensus.lfb_height;
        wait.observed_revision = consensus.lfb_revision;
    } else if consensus.lfb_height < wait.observed_height
        || (consensus.lfb_height == wait.observed_height
            && consensus.lfb_revision != wait.observed_revision)
    {
        wait.outcome = TargetWaitOutcome::HistoryCorruption;
        wait.completed_at = Some(wait.now);
    } else if consensus.lfb_height > wait.observed_height {
        wait.observed_height = consensus.lfb_height;
        wait.observed_revision = consensus.lfb_revision;
        wait.progress_renewals += 1;
        wait.last_progress_at = wait.now;
    }
}

#[derive(Clone, Copy)]
enum ParentFrontierDecision {
    Signed {
        version: usize,
        parent_count: usize,
    },
    Deferred {
        version: usize,
        required_parents: usize,
    },
}

fn evaluate_parent_frontier(snapshot: ParentFrontierView, cap: usize) -> ParentFrontierDecision {
    if snapshot.exact_parent_count <= cap {
        ParentFrontierDecision::Signed {
            version: snapshot.version,
            parent_count: snapshot.exact_parent_count,
        }
    } else {
        ParentFrontierDecision::Deferred {
            version: snapshot.version,
            required_parents: snapshot.exact_parent_count,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct DurableHead {
    revision: usize,
    floor: usize,
    certificate: usize,
}

#[derive(Clone, Copy)]
struct DagProjection {
    revision: usize,
    floor: usize,
    latest: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CertifiedSnapshot {
    revision: usize,
    floor: usize,
    certificate: usize,
    latest: usize,
}

fn capture_certified_snapshot(
    head: &Mutex<DurableHead>,
    dag: &Mutex<DagProjection>,
) -> Option<CertifiedSnapshot> {
    let before = *head.lock().unwrap();
    let captured_dag = *dag.lock().unwrap();
    let after = *head.lock().unwrap();
    if before != after
        || captured_dag.revision != before.revision
        || captured_dag.floor != before.floor
    {
        return None;
    }
    Some(CertifiedSnapshot {
        revision: before.revision,
        floor: before.floor,
        certificate: before.certificate,
        latest: captured_dag.latest,
    })
}

fn assert_coherent_certified_snapshot(snapshot: CertifiedSnapshot) {
    assert!(
        snapshot
            == CertifiedSnapshot {
                revision: 0,
                floor: 0,
                certificate: 0,
                latest: 10,
            }
            || snapshot
                == CertifiedSnapshot {
                    revision: 1,
                    floor: 1,
                    certificate: 1,
                    latest: 11,
                }
    );
}

#[test]
fn proposer_capture_is_coherent_during_parallel_finalization() {
    loom::model(|| {
        let head = Arc::new(Mutex::new(DurableHead {
            revision: 0,
            floor: 0,
            certificate: 0,
        }));
        let dag = Arc::new(Mutex::new(DagProjection {
            revision: 0,
            floor: 0,
            latest: 10,
        }));

        let writer_head = Arc::clone(&head);
        let writer_dag = Arc::clone(&dag);
        let writer = thread::spawn(move || {
            *writer_head.lock().unwrap() = DurableHead {
                revision: 1,
                floor: 1,
                certificate: 1,
            };
            thread::yield_now();
            *writer_dag.lock().unwrap() = DagProjection {
                revision: 1,
                floor: 1,
                latest: 11,
            };
        });

        let proposer_head = Arc::clone(&head);
        let proposer_dag = Arc::clone(&dag);
        let proposer = thread::spawn(move || {
            if let Some(snapshot) = capture_certified_snapshot(&proposer_head, &proposer_dag) {
                assert_coherent_certified_snapshot(snapshot);
            }
        });

        writer.join().unwrap();
        proposer.join().unwrap();
        assert_coherent_certified_snapshot(
            capture_certified_snapshot(&head, &dag).expect("settled projection"),
        );
    });
}

#[test]
#[should_panic]
fn single_head_read_allows_a_torn_floor_certificate_snapshot() {
    loom::model(|| {
        let head = Arc::new(Mutex::new(DurableHead {
            revision: 0,
            floor: 0,
            certificate: 0,
        }));
        let dag = Arc::new(Mutex::new(DagProjection {
            revision: 0,
            floor: 0,
            latest: 10,
        }));
        let writer_head = Arc::clone(&head);
        let writer_dag = Arc::clone(&dag);
        let writer = thread::spawn(move || {
            *writer_head.lock().unwrap() = DurableHead {
                revision: 1,
                floor: 1,
                certificate: 1,
            };
            thread::yield_now();
            *writer_dag.lock().unwrap() = DagProjection {
                revision: 1,
                floor: 1,
                latest: 11,
            };
        });
        let reader_head = Arc::clone(&head);
        let reader_dag = Arc::clone(&dag);
        let reader = thread::spawn(move || {
            let captured_head = *reader_head.lock().unwrap();
            let captured_dag = *reader_dag.lock().unwrap();
            assert_eq!(captured_head.revision, captured_dag.revision);
            assert_eq!(captured_head.floor, captured_dag.floor);
        });
        writer.join().unwrap();
        reader.join().unwrap();
    });
}

fn cached_chain_candidate_gate(chain_verified: bool, parent_floor: usize, floor: usize) -> bool {
    parent_floor <= floor && chain_verified
}

#[test]
fn shared_certificate_cache_does_not_share_candidate_parent_admission() {
    loom::model(|| {
        let verified = Arc::new(AtomicBool::new(true));
        let compatible_cache = Arc::clone(&verified);
        let incompatible_cache = Arc::clone(&verified);
        let compatible = thread::spawn(move || {
            cached_chain_candidate_gate(compatible_cache.load(Ordering::Acquire), 1, 1)
        });
        let incompatible = thread::spawn(move || {
            cached_chain_candidate_gate(incompatible_cache.load(Ordering::Acquire), 2, 1)
        });
        assert!(compatible.join().unwrap());
        assert!(!incompatible.join().unwrap());
    });
}

#[test]
fn floor_and_latest_are_captured_from_one_atomic_dag_view() {
    loom::model(|| {
        let dag = Arc::new(Mutex::new(DagConsensusView {
            floor: 0,
            latest: 1,
        }));
        let writer_dag = Arc::clone(&dag);
        let reader_dag = Arc::clone(&dag);

        let writer = thread::spawn(move || {
            let mut view = writer_dag.lock().unwrap();
            view.floor = 2;
            view.latest = 3;
        });
        let reader = thread::spawn(move || {
            let captured = *reader_dag.lock().unwrap();
            assert!(
                (captured.floor == 0 && captured.latest == 1)
                    || (captured.floor == 2 && captured.latest == 3)
            );
        });

        writer.join().unwrap();
        reader.join().unwrap();
    });
}

#[test]
fn captured_floor_is_an_evidence_root_even_when_latest_is_stale() {
    loom::model(|| {
        let dag = Arc::new(Mutex::new(DagConsensusView {
            floor: 2,
            latest: 1,
        }));
        let writer_dag = Arc::clone(&dag);
        let reader_dag = Arc::clone(&dag);

        let writer = thread::spawn(move || {
            let mut view = writer_dag.lock().unwrap();
            view.latest = 3;
        });
        let reader = thread::spawn(move || {
            let captured = *reader_dag.lock().unwrap();
            let evidence_roots = [captured.floor, captured.latest];
            assert!(evidence_roots.contains(&captured.floor));
            if captured.latest == 1 {
                assert_eq!(evidence_roots, [2, 1]);
            }
        });

        writer.join().unwrap();
        reader.join().unwrap();
    });
}

#[test]
fn parallel_capacity_decisions_use_one_exact_frozen_frontier() {
    loom::model(|| {
        let cap = 101;
        let frontier = Arc::new(Mutex::new(ParentFrontierView {
            version: 0,
            exact_parent_count: 4,
        }));
        let decisions = Arc::new(Mutex::new(Vec::new()));

        let writer_frontier = Arc::clone(&frontier);
        let writer = thread::spawn(move || {
            let mut view = writer_frontier.lock().unwrap();
            view.version = 1;
            view.exact_parent_count = 102;
        });

        let mut readers = Vec::new();
        for _ in 0..2 {
            let reader_frontier = Arc::clone(&frontier);
            let reader_decisions = Arc::clone(&decisions);
            readers.push(thread::spawn(move || {
                let snapshot = *reader_frontier.lock().unwrap();
                let decision = evaluate_parent_frontier(snapshot, cap);
                reader_decisions.lock().unwrap().push(decision);
            }));
        }

        writer.join().unwrap();
        for reader in readers {
            reader.join().unwrap();
        }

        let decisions = decisions.lock().unwrap();
        assert_eq!(decisions.len(), 2);
        for decision in decisions.iter() {
            match decision {
                ParentFrontierDecision::Signed {
                    version,
                    parent_count,
                } => {
                    assert_eq!((*version, *parent_count), (0, 4));
                    assert!(*parent_count <= cap);
                }
                ParentFrontierDecision::Deferred {
                    version,
                    required_parents,
                } => {
                    assert_eq!((*version, *required_parents), (1, 102));
                    assert!(*required_parents > cap);
                }
            }
        }
    });
}

#[test]
fn concurrent_floor_progress_cannot_masquerade_as_target_terminality() {
    loom::model(|| {
        let consensus = Arc::new(Mutex::new(TargetConsensusView {
            lfb_height: 0,
            lfb_revision: 0,
            target_status: TargetStatus::Pending,
        }));
        let wait = Arc::new(Mutex::new(TargetWaitView {
            baseline_known: false,
            observed_height: 0,
            observed_revision: 0,
            progress_renewals: 0,
            now: 0,
            last_progress_at: 0,
            stall_timeout: 2,
            absolute_deadline: 3,
            completed_at: None,
            outcome: TargetWaitOutcome::Waiting,
        }));

        observe_target(*consensus.lock().unwrap(), &mut wait.lock().unwrap());

        let progress_consensus = Arc::clone(&consensus);
        let progress = thread::spawn(move || {
            let mut view = progress_consensus.lock().unwrap();
            view.lfb_height = 1;
        });

        let revision_consensus = Arc::clone(&consensus);
        let revision = thread::spawn(move || {
            let mut view = revision_consensus.lock().unwrap();
            view.lfb_revision = 1;
        });

        let observer_consensus = Arc::clone(&consensus);
        let observer_wait = Arc::clone(&wait);
        let observer = thread::spawn(move || {
            let snapshot = *observer_consensus.lock().unwrap();
            observe_target(snapshot, &mut observer_wait.lock().unwrap());
        });

        progress.join().unwrap();
        revision.join().unwrap();
        observer.join().unwrap();

        let snapshot = *consensus.lock().unwrap();
        observe_target(snapshot, &mut wait.lock().unwrap());
        let observed = *wait.lock().unwrap();
        assert!(observed.outcome != TargetWaitOutcome::Succeeded);
        if observed.outcome == TargetWaitOutcome::Waiting {
            assert_eq!(observed.observed_height, 1);
            assert_eq!(observed.progress_renewals, 1);
        }
        assert_eq!(observed.absolute_deadline, 3);
    });
}

#[test]
fn exact_target_publication_is_the_only_concurrent_success_path() {
    loom::model(|| {
        let consensus = Arc::new(Mutex::new(TargetConsensusView {
            lfb_height: 0,
            lfb_revision: 0,
            target_status: TargetStatus::Pending,
        }));
        let wait = Arc::new(Mutex::new(TargetWaitView {
            baseline_known: false,
            observed_height: 0,
            observed_revision: 0,
            progress_renewals: 0,
            now: 0,
            last_progress_at: 0,
            stall_timeout: 2,
            absolute_deadline: 3,
            completed_at: None,
            outcome: TargetWaitOutcome::Waiting,
        }));

        let publisher_consensus = Arc::clone(&consensus);
        let publisher = thread::spawn(move || {
            let mut view = publisher_consensus.lock().unwrap();
            view.target_status = TargetStatus::Finalized;
        });

        let observer_consensus = Arc::clone(&consensus);
        let observer_wait = Arc::clone(&wait);
        let observer = thread::spawn(move || {
            let snapshot = *observer_consensus.lock().unwrap();
            observe_target(snapshot, &mut observer_wait.lock().unwrap());
        });

        publisher.join().unwrap();
        observer.join().unwrap();

        let final_snapshot = *consensus.lock().unwrap();
        assert!(final_snapshot.target_status == TargetStatus::Finalized);
        observe_target(final_snapshot, &mut wait.lock().unwrap());
        assert!(wait.lock().unwrap().outcome == TargetWaitOutcome::Succeeded);
    });
}

#[test]
fn finalized_history_revision_and_regression_fail_loudly() {
    loom::model(|| {
        for anomalous in [
            TargetConsensusView {
                lfb_height: 6,
                lfb_revision: 11,
                target_status: TargetStatus::Pending,
            },
            TargetConsensusView {
                lfb_height: 5,
                lfb_revision: 9,
                target_status: TargetStatus::Pending,
            },
        ] {
            let mut wait = TargetWaitView {
                baseline_known: true,
                observed_height: 6,
                observed_revision: 10,
                progress_renewals: 0,
                now: 0,
                last_progress_at: 0,
                stall_timeout: 2,
                absolute_deadline: 3,
                completed_at: None,
                outcome: TargetWaitOutcome::Waiting,
            };
            observe_target(anomalous, &mut wait);
            assert!(wait.outcome == TargetWaitOutcome::HistoryCorruption);
            assert_eq!(wait.progress_renewals, 0);
        }
    });
}

#[test]
fn first_observation_is_only_a_baseline_and_terminal_errors_do_not_succeed() {
    loom::model(|| {
        let mut wait = TargetWaitView {
            baseline_known: false,
            observed_height: 0,
            observed_revision: 0,
            progress_renewals: 0,
            now: 0,
            last_progress_at: 0,
            stall_timeout: 2,
            absolute_deadline: 3,
            completed_at: None,
            outcome: TargetWaitOutcome::Waiting,
        };
        observe_target(
            TargetConsensusView {
                lfb_height: 6,
                lfb_revision: 10,
                target_status: TargetStatus::Pending,
            },
            &mut wait,
        );
        assert!(wait.outcome == TargetWaitOutcome::Waiting);
        assert_eq!(wait.progress_renewals, 0);

        for status in [TargetStatus::Failed, TargetStatus::Expired] {
            let mut terminal_wait = wait;
            observe_target(
                TargetConsensusView {
                    lfb_height: 6,
                    lfb_revision: 10,
                    target_status: status,
                },
                &mut terminal_wait,
            );
            assert!(terminal_wait.outcome == TargetWaitOutcome::TerminalError);
        }
    });
}

#[test]
fn terminal_status_and_deadline_interleavings_never_accept_a_late_response() {
    loom::model(|| {
        let consensus = Arc::new(Mutex::new(TargetConsensusView {
            lfb_height: 0,
            lfb_revision: 0,
            target_status: TargetStatus::Pending,
        }));
        let wait = Arc::new(Mutex::new(TargetWaitView {
            baseline_known: false,
            observed_height: 0,
            observed_revision: 0,
            progress_renewals: 0,
            now: 0,
            last_progress_at: 2,
            stall_timeout: 2,
            absolute_deadline: 3,
            completed_at: None,
            outcome: TargetWaitOutcome::Waiting,
        }));

        let publisher_consensus = Arc::clone(&consensus);
        let publisher = thread::spawn(move || {
            publisher_consensus.lock().unwrap().target_status = TargetStatus::Finalized;
        });

        let deadline_wait = Arc::clone(&wait);
        let deadline = thread::spawn(move || {
            deadline_wait.lock().unwrap().now = 3;
        });

        let observer_consensus = Arc::clone(&consensus);
        let observer_wait = Arc::clone(&wait);
        let observer = thread::spawn(move || {
            let snapshot = *observer_consensus.lock().unwrap();
            observe_target(snapshot, &mut observer_wait.lock().unwrap());
        });

        publisher.join().unwrap();
        deadline.join().unwrap();
        observer.join().unwrap();

        let final_snapshot = *consensus.lock().unwrap();
        observe_target(final_snapshot, &mut wait.lock().unwrap());
        let observed = *wait.lock().unwrap();
        match observed.outcome {
            TargetWaitOutcome::Succeeded => {
                assert!(observed.completed_at.unwrap() < observed.absolute_deadline);
            }
            TargetWaitOutcome::TimedOut => {
                assert!(observed.completed_at.unwrap() >= observed.absolute_deadline);
            }
            _ => panic!("terminal publication must either win before the deadline or time out"),
        }
    });
}

#[test]
fn every_terminal_status_at_the_deadline_times_out() {
    loom::model(|| {
        for status in [
            TargetStatus::Finalized,
            TargetStatus::Failed,
            TargetStatus::Expired,
        ] {
            let mut wait = TargetWaitView {
                baseline_known: true,
                observed_height: 6,
                observed_revision: 10,
                progress_renewals: 1,
                now: 3,
                last_progress_at: 2,
                stall_timeout: 2,
                absolute_deadline: 3,
                completed_at: None,
                outcome: TargetWaitOutcome::Waiting,
            };
            observe_target(
                TargetConsensusView {
                    lfb_height: 6,
                    lfb_revision: 10,
                    target_status: status,
                },
                &mut wait,
            );
            assert!(wait.outcome == TargetWaitOutcome::TimedOut);
            assert_eq!(wait.completed_at, Some(3));
        }
    });
}

#[derive(Clone, Copy)]
struct StaleSiblingLifecycle {
    floor_b: bool,
    stale_a_causal: bool,
    settlement_published: bool,
    exact_tombstone_a: bool,
    settlement_seen: u8,
    tombstone_seen: u8,
    buffer_seen: u8,
    recovery_owner: Option<usize>,
    effects: u8,
}

const EFFECT_A: u8 = 1;
const EFFECT_B: u8 = 2;
const EFFECT_FRESH: u8 = 4;

fn assert_stale_sibling_lifecycle(state: StaleSiblingLifecycle, recovery_leader: usize) {
    if state.floor_b {
        assert_ne!(state.effects & EFFECT_B, 0);
    }
    if state.floor_b && !state.settlement_published {
        assert!(state.stale_a_causal);
    }
    assert_eq!(state.settlement_seen & !state.tombstone_seen, 0);
    assert_eq!(state.settlement_seen & !state.buffer_seen, 0);
    if let Some(owner) = state.recovery_owner {
        let owner_bit = 1u8 << owner;
        assert_eq!(owner, recovery_leader);
        assert_ne!(state.settlement_seen & owner_bit, 0);
        assert_ne!(state.tombstone_seen & owner_bit, 0);
        assert_ne!(state.buffer_seen & owner_bit, 0);
        assert!(state.exact_tombstone_a);
        assert_eq!(
            state.effects & (EFFECT_A | EFFECT_B | EFFECT_FRESH),
            EFFECT_A | EFFECT_B | EFFECT_FRESH
        );
    }
}

fn advance_stale_sibling_floor(state: &mut StaleSiblingLifecycle) {
    state.floor_b = true;
    state.effects |= EFFECT_B;
}

fn publish_stale_sibling_settlement(state: &mut StaleSiblingLifecycle) {
    if state.floor_b {
        state.settlement_published = true;
        state.exact_tombstone_a = true;
        state.stale_a_causal = false;
    }
}

fn observe_stale_sibling_settlement(state: &mut StaleSiblingLifecycle, validator: usize) {
    if state.settlement_published {
        let validator_bit = 1u8 << validator;
        state.settlement_seen |= validator_bit;
        state.tombstone_seen |= validator_bit;
        state.buffer_seen |= validator_bit;
    }
}

fn try_stale_sibling_recovery(
    state: &mut StaleSiblingLifecycle,
    validator: usize,
    recovery_leader: usize,
) {
    let validator_bit = 1u8 << validator;
    if state.recovery_owner.is_none()
        && validator == recovery_leader
        && state.exact_tombstone_a
        && state.settlement_seen & validator_bit != 0
        && state.tombstone_seen & validator_bit != 0
        && state.buffer_seen & validator_bit != 0
    {
        state.recovery_owner = Some(validator);
        state.effects |= EFFECT_A | EFFECT_FRESH;
    }
}

#[derive(Clone, Copy)]
enum StaleSiblingAction {
    AdvanceFloor,
    PublishSettlement,
    ObserveLeader,
    ObserveNonleader,
}

fn apply_stale_sibling_action(
    state: &mut StaleSiblingLifecycle,
    action: StaleSiblingAction,
    recovery_leader: usize,
) {
    match action {
        StaleSiblingAction::AdvanceFloor => advance_stale_sibling_floor(state),
        StaleSiblingAction::PublishSettlement => publish_stale_sibling_settlement(state),
        StaleSiblingAction::ObserveLeader => {
            observe_stale_sibling_settlement(state, recovery_leader);
            try_stale_sibling_recovery(state, recovery_leader, recovery_leader);
        }
        StaleSiblingAction::ObserveNonleader => {
            observe_stale_sibling_settlement(state, 1);
            try_stale_sibling_recovery(state, 1, recovery_leader);
        }
    }
}

fn visit_stale_sibling_permutations(
    state: StaleSiblingLifecycle,
    remaining: Vec<StaleSiblingAction>,
    recovery_leader: usize,
) {
    if remaining.is_empty() {
        assert_stale_sibling_lifecycle(state, recovery_leader);
        return;
    }
    for index in 0..remaining.len() {
        let mut next_remaining = remaining.clone();
        let action = next_remaining.remove(index);
        let mut next_state = state;
        apply_stale_sibling_action(&mut next_state, action, recovery_leader);
        assert_stale_sibling_lifecycle(next_state, recovery_leader);
        visit_stale_sibling_permutations(next_state, next_remaining, recovery_leader);
    }
}

fn initial_stale_sibling_lifecycle() -> StaleSiblingLifecycle {
    StaleSiblingLifecycle {
        floor_b: false,
        stale_a_causal: true,
        settlement_published: false,
        exact_tombstone_a: false,
        settlement_seen: 0,
        tombstone_seen: 0,
        buffer_seen: 0,
        recovery_owner: None,
        effects: 0,
    }
}

#[test]
fn every_stale_sibling_action_order_preserves_recovery_authority() {
    visit_stale_sibling_permutations(
        initial_stale_sibling_lifecycle(),
        vec![
            StaleSiblingAction::AdvanceFloor,
            StaleSiblingAction::PublishSettlement,
            StaleSiblingAction::ObserveLeader,
            StaleSiblingAction::ObserveNonleader,
        ],
        0,
    );
}

#[test]
fn parallel_stale_sibling_settlement_authorizes_exactly_one_recovery() {
    loom::model(|| {
        let recovery_leader = 0usize;
        let state = Arc::new(Mutex::new(initial_stale_sibling_lifecycle()));

        let publisher_state = Arc::clone(&state);
        let publisher = thread::spawn(move || {
            {
                let mut state = publisher_state.lock().unwrap();
                advance_stale_sibling_floor(&mut state);
                assert_stale_sibling_lifecycle(*state, recovery_leader);
            }
            thread::yield_now();
            let mut state = publisher_state.lock().unwrap();
            publish_stale_sibling_settlement(&mut state);
            assert_stale_sibling_lifecycle(*state, recovery_leader);
        });

        let observer_state = Arc::clone(&state);
        let observer = thread::spawn(move || {
            {
                let mut state = observer_state.lock().unwrap();
                observe_stale_sibling_settlement(&mut state, 1);
                try_stale_sibling_recovery(&mut state, 1, recovery_leader);
                assert_stale_sibling_lifecycle(*state, recovery_leader);
            }
            thread::yield_now();
            let mut state = observer_state.lock().unwrap();
            observe_stale_sibling_settlement(&mut state, recovery_leader);
            try_stale_sibling_recovery(&mut state, recovery_leader, recovery_leader);
            assert_stale_sibling_lifecycle(*state, recovery_leader);
        });

        publisher.join().unwrap();
        observer.join().unwrap();

        let mut state = state.lock().unwrap();
        advance_stale_sibling_floor(&mut state);
        publish_stale_sibling_settlement(&mut state);
        observe_stale_sibling_settlement(&mut state, 0);
        observe_stale_sibling_settlement(&mut state, 1);
        try_stale_sibling_recovery(&mut state, 1, recovery_leader);
        try_stale_sibling_recovery(&mut state, 0, recovery_leader);
        assert_stale_sibling_lifecycle(*state, recovery_leader);
        assert_eq!(state.recovery_owner, Some(recovery_leader));
        assert_eq!(state.effects, EFFECT_A | EFFECT_B | EFFECT_FRESH);
    });
}
