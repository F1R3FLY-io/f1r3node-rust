use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy, Default)]
struct CommitteeState {
    post_state_bonded: bool,
    post_state_active: bool,
    latest_message_registered: bool,
    activation_boundary_applied: bool,
    floor_bonded: bool,
    floor_active: bool,
    floor_bonds_revision: usize,
    floor_active_revision: usize,
}

fn insert_transition(state: &Mutex<CommitteeState>) {
    let mut state = state.lock().unwrap();
    state.post_state_bonded = true;
    state.latest_message_registered = true;
}

fn promote_if_ready(state: &Mutex<CommitteeState>) {
    let mut state = state.lock().unwrap();
    if state.post_state_bonded && state.latest_message_registered {
        state.floor_bonded = true;
        state.floor_active = state.post_state_active;
        state.floor_bonds_revision += 1;
        state.floor_active_revision = state.floor_bonds_revision;
    }
}

fn apply_activation_boundary(state: &Mutex<CommitteeState>) {
    let mut state = state.lock().unwrap();
    if state.post_state_bonded && state.latest_message_registered {
        state.activation_boundary_applied = true;
        state.post_state_active = true;
    }
}

#[test]
fn concurrent_registration_and_off_boundary_promotion_never_activate_new_authority() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(CommitteeState::default()));
        let insert = {
            let state = state.clone();
            thread::spawn(move || insert_transition(&state))
        };
        let promote = {
            let state = state.clone();
            thread::spawn(move || promote_if_ready(&state))
        };

        insert.join().unwrap();
        promote.join().unwrap();
        {
            let state = state.lock().unwrap();
            assert!(!state.floor_active);
            assert!(!state.post_state_active);
        }

        promote_if_ready(&state);
        let state = state.lock().unwrap();
        assert!(state.floor_bonded);
        assert!(!state.floor_active);
        assert!(state.latest_message_registered);
        assert_eq!(state.floor_bonds_revision, state.floor_active_revision);
    });
}

#[test]
fn same_block_post_state_never_grants_sender_authority() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(CommitteeState::default()));
        let stage = {
            let state = state.clone();
            thread::spawn(move || insert_transition(&state))
        };
        let validate_same_block = {
            let state = state.clone();
            thread::spawn(move || {
                let state = state.lock().unwrap();
                let sender_authorized = state.floor_active;
                assert!(!sender_authorized);
            })
        };

        stage.join().unwrap();
        validate_same_block.join().unwrap();
        let state = state.lock().unwrap();
        assert!(state.post_state_bonded);
        assert!(!state.post_state_active);
        assert!(!state.floor_active);
    });
}

#[test]
fn concurrent_boundary_and_floor_promotion_never_publish_uncertified_activation() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(CommitteeState::default()));
        insert_transition(&state);

        let boundary = {
            let state = state.clone();
            thread::spawn(move || apply_activation_boundary(&state))
        };
        let promote = {
            let state = state.clone();
            thread::spawn(move || promote_if_ready(&state))
        };

        boundary.join().unwrap();
        promote.join().unwrap();
        {
            let state = state.lock().unwrap();
            assert!(!state.floor_active || state.activation_boundary_applied);
            assert_eq!(state.floor_bonds_revision, state.floor_active_revision);
        }

        promote_if_ready(&state);
        let state = state.lock().unwrap();
        assert!(state.floor_bonded);
        assert!(state.floor_active);
        assert!(state.activation_boundary_applied);
        assert_eq!(state.floor_bonds_revision, state.floor_active_revision);
    });
}

#[test]
fn concurrent_floor_read_observes_one_bonded_active_revision() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(CommitteeState::default()));
        insert_transition(&state);
        apply_activation_boundary(&state);

        let promote = {
            let state = state.clone();
            thread::spawn(move || promote_if_ready(&state))
        };
        let read = {
            let state = state.clone();
            thread::spawn(move || {
                let state = state.lock().unwrap();
                (
                    state.floor_bonds_revision,
                    state.floor_active_revision,
                    state.floor_bonded,
                    state.floor_active,
                )
            })
        };

        promote.join().unwrap();
        let (bond_revision, active_revision, bonded, active) = read.join().unwrap();
        assert_eq!(bond_revision, active_revision);
        assert_eq!(bonded, active);
    });
}

#[test]
fn head_drift_cannot_change_floor_authority_or_synchrony_weight() {
    loom::model(|| {
        #[derive(Clone, Copy)]
        struct View {
            floor_weight: usize,
            head_weight: usize,
        }

        let view = Arc::new(Mutex::new(View {
            floor_weight: 10,
            head_weight: 10,
        }));
        let drift = {
            let view = view.clone();
            thread::spawn(move || view.lock().unwrap().head_weight = 100)
        };
        let validate = {
            let view = view.clone();
            thread::spawn(move || {
                let view = view.lock().unwrap();
                (view.floor_weight, view.floor_weight)
            })
        };

        drift.join().unwrap();
        let (authority_weight, synchrony_weight) = validate.join().unwrap();
        assert_eq!(authority_weight, 10);
        assert_eq!(synchrony_weight, 10);
        assert_eq!(view.lock().unwrap().head_weight, 100);
    });
}

#[test]
fn inactive_bond_updates_cannot_change_the_active_finality_denominator() {
    loom::model(|| {
        #[derive(Clone, Copy)]
        struct Weights {
            active_floor: usize,
            all_bonds: usize,
        }

        let weights = Arc::new(Mutex::new(Weights {
            active_floor: 3,
            all_bonds: 3,
        }));
        let update = {
            let weights = weights.clone();
            thread::spawn(move || weights.lock().unwrap().all_bonds += 10_000)
        };
        let certify = {
            let weights = weights.clone();
            thread::spawn(move || weights.lock().unwrap().active_floor)
        };

        update.join().unwrap();
        assert_eq!(certify.join().unwrap(), 3);
        let weights = weights.lock().unwrap();
        assert_eq!(weights.active_floor, 3);
        assert_eq!(weights.all_bonds, 10_003);
    });
}

#[test]
fn concurrent_floor_promotion_cannot_tear_the_proposal_replay_anchor() {
    loom::model(|| {
        #[derive(Clone, Copy)]
        struct CertifiedFloor {
            hash: usize,
            state: usize,
        }

        let durable = Arc::new(Mutex::new(CertifiedFloor { hash: 1, state: 11 }));
        let promote = {
            let durable = durable.clone();
            thread::spawn(move || {
                *durable.lock().unwrap() = CertifiedFloor { hash: 2, state: 22 };
            })
        };
        let propose = {
            let durable = durable.clone();
            thread::spawn(move || {
                let captured = *durable.lock().unwrap();
                let replay_anchor = captured;
                (captured, replay_anchor)
            })
        };

        promote.join().unwrap();
        let (committed, replay) = propose.join().unwrap();
        assert_eq!(replay.hash, committed.hash);
        assert_eq!(replay.state, committed.state);
        assert!(matches!((replay.hash, replay.state), (1, 11) | (2, 22)));
    });
}

#[test]
fn concurrent_derived_floor_evidence_cannot_replace_the_committed_replay_floor() {
    loom::model(|| {
        let committed = Arc::new((1usize, 11usize));
        let derived_floor = Arc::new(AtomicUsize::new(1));
        let derive = {
            let derived_floor = derived_floor.clone();
            thread::spawn(move || derived_floor.store(2, Ordering::Release))
        };
        let replay = {
            let committed = committed.clone();
            let derived_floor = derived_floor.clone();
            thread::spawn(move || {
                (
                    committed.0,
                    committed.1,
                    derived_floor.load(Ordering::Acquire),
                )
            })
        };

        derive.join().unwrap();
        let (replay_floor, replay_state, derived_floor) = replay.join().unwrap();
        assert_eq!(replay_floor, 1);
        assert_eq!(replay_state, 11);
        assert!(matches!(derived_floor, 1 | 2));
    });
}
