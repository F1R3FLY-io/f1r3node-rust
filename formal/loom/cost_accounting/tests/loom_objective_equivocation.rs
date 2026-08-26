use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy)]
struct ReplicaState {
    blocks: [bool; 4],
    generations: [usize; 4],
    epochs: [usize; 4],
    sequences: [usize; 4],
    local_invalid: [bool; 4],
    authority_generation: usize,
    authority_epoch: usize,
    authority_bonded: bool,
    evidence: Option<(usize, usize)>,
}

impl Default for ReplicaState {
    fn default() -> Self {
        Self {
            blocks: [false; 4],
            generations: [0; 4],
            epochs: [0; 4],
            sequences: [0; 4],
            local_invalid: [false; 4],
            authority_generation: 0,
            authority_epoch: 0,
            authority_bonded: true,
            evidence: None,
        }
    }
}

fn evidence_at_authority(state: &ReplicaState) -> Option<(usize, usize)> {
    for left in 0..state.blocks.len() {
        for right in left + 1..state.blocks.len() {
            if state.blocks[left]
                && state.blocks[right]
                && state.generations[left] == state.authority_generation
                && state.generations[right] == state.authority_generation
                && state.epochs[left] == state.authority_epoch
                && state.epochs[right] == state.authority_epoch
                && state.sequences[left] == state.sequences[right]
            {
                return Some((left, right));
            }
        }
    }
    None
}

fn pair_authorized(state: &ReplicaState, pair: Option<(usize, usize)>) -> bool {
    pair.is_some_and(|(left, right)| {
        left != right
            && state.authority_bonded
            && state.generations[left] == state.authority_generation
            && state.generations[right] == state.authority_generation
            && state.epochs[left] == state.authority_epoch
            && state.epochs[right] == state.authority_epoch
            && state.sequences[left] == state.sequences[right]
    })
}

fn unary_authorized(state: &ReplicaState, block: usize) -> bool {
    state.blocks[block]
        && state.local_invalid[block]
        && state.authority_bonded
        && state.generations[block] == state.authority_generation
        && state.epochs[block] == state.authority_epoch
        && !(0..state.blocks.len()).any(|other| {
            other != block
                && state.blocks[other]
                && state.generations[other] == state.generations[block]
                && state.sequences[other] == state.sequences[block]
        })
}

fn insert(state: &Mutex<ReplicaState>, block: usize, invalid: bool) {
    let mut state = state.lock().unwrap();
    state.blocks[block] = true;
    state.local_invalid[block] = invalid;
    state.evidence = evidence_at_authority(&state);
}

fn slash_evidence(state: &Mutex<ReplicaState>) -> Option<(usize, usize)> {
    state.lock().unwrap().evidence
}

#[test]
fn parallel_sibling_insertion_discovers_one_canonical_pair() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(ReplicaState {
            ..ReplicaState::default()
        }));
        let first = {
            let state = state.clone();
            thread::spawn(move || insert(&state, 0, false))
        };
        let second = {
            let state = state.clone();
            thread::spawn(move || insert(&state, 1, false))
        };
        first.join().unwrap();
        second.join().unwrap();

        let state = state.lock().unwrap();
        assert_eq!(state.evidence, Some((0, 1)));
        assert!(pair_authorized(&state, state.evidence));
    });
}

#[test]
fn opposite_local_classifications_authorize_identical_evidence() {
    loom::model(|| {
        let forward = Arc::new(Mutex::new(ReplicaState {
            ..ReplicaState::default()
        }));
        let reverse = Arc::new(Mutex::new(ReplicaState {
            ..ReplicaState::default()
        }));
        let receive_forward = {
            let state = forward.clone();
            thread::spawn(move || {
                insert(&state, 0, false);
                insert(&state, 1, true);
            })
        };
        let receive_reverse = {
            let state = reverse.clone();
            thread::spawn(move || {
                insert(&state, 1, false);
                insert(&state, 0, true);
            })
        };
        receive_forward.join().unwrap();
        receive_reverse.join().unwrap();

        assert_eq!(slash_evidence(&forward), Some((0, 1)));
        assert_eq!(slash_evidence(&reverse), Some((0, 1)));
        assert_ne!(
            forward.lock().unwrap().local_invalid,
            reverse.lock().unwrap().local_invalid
        );
    });
}

#[test]
fn generation_grouping_precedes_pair_selection_and_vote_exclusion() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(ReplicaState {
            generations: [0, 1, 1, 1],
            epochs: [1, 0, 1, 1],
            sequences: [3, 4, 5, 5],
            authority_generation: 1,
            authority_epoch: 1,
            ..ReplicaState::default()
        }));
        let historical_stream = {
            let state = state.clone();
            thread::spawn(move || {
                insert(&state, 0, false);
                insert(&state, 2, false);
            })
        };
        let current_stream = {
            let state = state.clone();
            thread::spawn(move || {
                insert(&state, 1, false);
                insert(&state, 3, false);
            })
        };
        historical_stream.join().unwrap();
        current_stream.join().unwrap();

        let state = state.lock().unwrap();
        assert_eq!(state.evidence, Some((2, 3)));
        assert!(pair_authorized(&state, state.evidence));
    });
}

#[test]
fn proposer_and_receiver_share_epoch_generation_and_bond_authority() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(ReplicaState {
            generations: [0, 0, 0, 0],
            epochs: [0, 1, 1, 0],
            authority_epoch: 1,
            ..ReplicaState::default()
        }));
        let first = {
            let state = state.clone();
            thread::spawn(move || insert(&state, 1, false))
        };
        let second = {
            let state = state.clone();
            thread::spawn(move || insert(&state, 2, false))
        };
        first.join().unwrap();
        second.join().unwrap();

        let state = state.lock().unwrap();
        let proposer = pair_authorized(&state, state.evidence);
        let receiver = pair_authorized(&state, state.evidence);
        assert!(proposer);
        assert_eq!(proposer, receiver);
    });
}

#[test]
fn nonpositive_authority_cannot_authorize_an_objective_pair() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(ReplicaState {
            authority_bonded: false,
            ..ReplicaState::default()
        }));
        let first = {
            let state = state.clone();
            thread::spawn(move || insert(&state, 0, false))
        };
        let second = {
            let state = state.clone();
            thread::spawn(move || insert(&state, 1, false))
        };
        first.join().unwrap();
        second.join().unwrap();

        let state = state.lock().unwrap();
        assert_eq!(state.evidence, Some((0, 1)));
        assert!(!pair_authorized(&state, state.evidence));
    });
}

#[test]
fn structural_collision_suppresses_only_the_matching_unary_key() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(ReplicaState {
            epochs: [0, 1, 1, 0],
            sequences: [11, 11, 12, 0],
            authority_epoch: 1,
            ..ReplicaState::default()
        }));
        let equivocation_stream = {
            let state = state.clone();
            thread::spawn(move || {
                insert(&state, 0, false);
                insert(&state, 1, true);
            })
        };
        let independent_fault = {
            let state = state.clone();
            thread::spawn(move || insert(&state, 2, true))
        };
        equivocation_stream.join().unwrap();
        independent_fault.join().unwrap();

        let state = state.lock().unwrap();
        assert_eq!(state.evidence, None);
        assert!(!unary_authorized(&state, 1));
        assert!(unary_authorized(&state, 2));
    });
}
