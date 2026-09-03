use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy)]
struct Facts {
    state_parent: u8,
    applied: u8,
    own: u8,
    rejected: u8,
    causal_sources: u8,
    committed_sources: u8,
}

impl Facts {
    fn validate(self) -> bool {
        self.applied & self.rejected == 0
            && self.applied & !self.causal_sources == 0
            && self.applied & !self.committed_sources == 0
    }

    fn active(self) -> Option<u8> {
        self.validate()
            .then_some(self.state_parent | self.applied | self.own)
    }
}

#[derive(Default)]
struct Published {
    facts: Option<Facts>,
    finality_read: Option<u8>,
}

fn evaluate(state: &Mutex<Published>) {
    let mut state = state.lock().unwrap();
    let Some(facts) = state.facts else {
        return;
    };
    if let Some(active) = facts.active() {
        state.finality_read = Some(active);
    }
}

#[test]
fn exact_facts_publish_atomically_before_finality_reads_them() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(Published::default()));
        let publish = {
            let state = state.clone();
            thread::spawn(move || {
                state.lock().unwrap().facts = Some(Facts {
                    state_parent: 0b0001,
                    applied: 0b0010,
                    own: 0b0100,
                    rejected: 0b1000,
                    causal_sources: 0b1010,
                    committed_sources: 0b1010,
                });
            })
        };
        let finalize = {
            let state = state.clone();
            thread::spawn(move || evaluate(&state))
        };

        publish.join().unwrap();
        finalize.join().unwrap();
        evaluate(&state);
        assert_eq!(state.lock().unwrap().finality_read, Some(0b0111));
    });
}

#[test]
fn rejected_header_parent_effect_never_enters_the_exact_state() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(Published::default()));
        let rejected = {
            let state = state.clone();
            thread::spawn(move || {
                state.lock().unwrap().facts = Some(Facts {
                    state_parent: 0b0001,
                    applied: 0,
                    own: 0b0010,
                    rejected: 0b0100,
                    causal_sources: 0b0100,
                    committed_sources: 0b0100,
                });
            })
        };
        let finalize = {
            let state = state.clone();
            thread::spawn(move || evaluate(&state))
        };

        rejected.join().unwrap();
        finalize.join().unwrap();
        evaluate(&state);
        let active = state.lock().unwrap().finality_read.unwrap();
        assert_eq!(active, 0b0011);
        assert_eq!(active & 0b0100, 0);
    });
}

#[test]
fn rejection_evidence_never_subtracts_an_inherited_state_parent_effect() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(Published::default()));
        let publish = {
            let state = state.clone();
            thread::spawn(move || {
                state.lock().unwrap().facts = Some(Facts {
                    state_parent: 0b0001,
                    applied: 0,
                    own: 0,
                    rejected: 0b0001,
                    causal_sources: 0b0001,
                    committed_sources: 0b0001,
                });
            })
        };
        let finalize = {
            let state = state.clone();
            thread::spawn(move || evaluate(&state))
        };

        publish.join().unwrap();
        finalize.join().unwrap();
        evaluate(&state);
        assert_eq!(state.lock().unwrap().finality_read, Some(0b0001));
    });
}

#[test]
fn invalid_applied_facts_cannot_authorize_finality() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(Published::default()));
        let publish = {
            let state = state.clone();
            thread::spawn(move || {
                state.lock().unwrap().facts = Some(Facts {
                    state_parent: 0b0001,
                    applied: 0b0010,
                    own: 0,
                    rejected: 0b0010,
                    causal_sources: 0,
                    committed_sources: 0,
                });
            })
        };
        let finalize = {
            let state = state.clone();
            thread::spawn(move || evaluate(&state))
        };

        publish.join().unwrap();
        finalize.join().unwrap();
        evaluate(&state);
        assert_eq!(state.lock().unwrap().finality_read, None);
    });
}

#[derive(Clone, Copy)]
enum Consumer {
    Certificate,
    Proposal,
    DurableAppend,
}

#[derive(Default)]
struct ConsumerState {
    facts: Option<Facts>,
    certificate: Option<u8>,
    proposal: Option<u8>,
    durable_append: Option<u8>,
}

fn evaluate_consumer(state: &Mutex<ConsumerState>, consumer: Consumer) {
    let mut state = state.lock().unwrap();
    let Some(active) = state.facts.and_then(Facts::active) else {
        return;
    };
    match consumer {
        Consumer::Certificate => state.certificate = Some(active),
        Consumer::Proposal => state.proposal = Some(active),
        Consumer::DurableAppend => state.durable_append = Some(active),
    }
}

#[test]
fn certificate_proposal_and_durable_append_share_exact_facts() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(ConsumerState::default()));
        let publish = {
            let state = state.clone();
            thread::spawn(move || {
                state.lock().unwrap().facts = Some(Facts {
                    state_parent: 0b0001,
                    applied: 0b0010,
                    own: 0b0100,
                    rejected: 0b1000,
                    causal_sources: 0b1010,
                    committed_sources: 0b1010,
                });
            })
        };
        let consumers = {
            let state = state.clone();
            thread::spawn(move || {
                evaluate_consumer(&state, Consumer::Certificate);
                evaluate_consumer(&state, Consumer::Proposal);
                evaluate_consumer(&state, Consumer::DurableAppend);
            })
        };

        publish.join().unwrap();
        consumers.join().unwrap();
        evaluate_consumer(&state, Consumer::Certificate);
        evaluate_consumer(&state, Consumer::Proposal);
        evaluate_consumer(&state, Consumer::DurableAppend);
        let state = state.lock().unwrap();
        assert_eq!(state.certificate, Some(0b0111));
        assert_eq!(state.proposal, state.certificate);
        assert_eq!(state.durable_append, state.certificate);
    });
}

struct FloorSnapshot {
    known: u8,
    required: u8,
    decision: Option<bool>,
}

fn attempt_floor_selection(state: &Mutex<FloorSnapshot>, candidate: u8) {
    let mut state = state.lock().unwrap();
    if state.known == state.required {
        state.decision = Some(state.required & !candidate == 0);
    }
}

#[test]
fn incomplete_floor_snapshot_defers_before_exact_selection() {
    loom::model(|| {
        let state = Arc::new(Mutex::new(FloorSnapshot {
            known: 0,
            required: 0b0011,
            decision: None,
        }));
        let publish_left = {
            let state = state.clone();
            thread::spawn(move || state.lock().unwrap().known |= 0b0001)
        };
        let publish_right = {
            let state = state.clone();
            thread::spawn(move || state.lock().unwrap().known |= 0b0010)
        };
        let select_partial = {
            let state = state.clone();
            thread::spawn(move || attempt_floor_selection(&state, 0b0001))
        };

        publish_left.join().unwrap();
        publish_right.join().unwrap();
        select_partial.join().unwrap();
        attempt_floor_selection(&state, 0b0001);
        assert_eq!(state.lock().unwrap().decision, Some(false));
        attempt_floor_selection(&state, 0b0011);
        assert_eq!(state.lock().unwrap().decision, Some(true));
    });
}

struct SemanticCache {
    schema: u32,
    selection: u8,
}

fn read_selection(cache: &Mutex<SemanticCache>, exact: u8) -> u8 {
    let mut cache = cache.lock().unwrap();
    if cache.schema != 13 {
        cache.schema = 13;
        cache.selection = exact;
    }
    cache.selection
}

#[test]
fn stale_semantic_cache_never_supplies_floor_selection() {
    loom::model(|| {
        let cache = Arc::new(Mutex::new(SemanticCache {
            schema: 12,
            selection: 0b0001,
        }));
        let first = {
            let cache = cache.clone();
            thread::spawn(move || read_selection(&cache, 0b0011))
        };
        let second = {
            let cache = cache.clone();
            thread::spawn(move || read_selection(&cache, 0b0011))
        };

        assert_eq!(first.join().unwrap(), 0b0011);
        assert_eq!(second.join().unwrap(), 0b0011);
        let cache = cache.lock().unwrap();
        assert_eq!(cache.schema, 13);
        assert_eq!(cache.selection, 0b0011);
    });
}
