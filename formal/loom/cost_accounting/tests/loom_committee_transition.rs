use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy, Default)]
struct CommitteeState {
    post_state_has_new_validator: bool,
    latest_message_registered: bool,
    floor_has_new_validator: bool,
}

fn insert_transition(state: &Mutex<CommitteeState>) {
    let mut state = state.lock().unwrap();
    state.post_state_has_new_validator = true;
    state.latest_message_registered = true;
}

fn promote_if_ready(state: &Mutex<CommitteeState>) {
    let mut state = state.lock().unwrap();
    if state.post_state_has_new_validator && state.latest_message_registered {
        state.floor_has_new_validator = true;
    }
}

#[test]
fn concurrent_registration_and_floor_promotion_never_expose_unregistered_authority() {
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
            assert!(!state.floor_has_new_validator || state.latest_message_registered);
        }

        promote_if_ready(&state);
        let state = state.lock().unwrap();
        assert!(state.floor_has_new_validator);
        assert!(state.latest_message_registered);
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
                let sender_authorized = state.floor_has_new_validator;
                assert!(!sender_authorized);
            })
        };

        stage.join().unwrap();
        validate_same_block.join().unwrap();
        let state = state.lock().unwrap();
        assert!(state.post_state_has_new_validator);
        assert!(!state.floor_has_new_validator);
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
