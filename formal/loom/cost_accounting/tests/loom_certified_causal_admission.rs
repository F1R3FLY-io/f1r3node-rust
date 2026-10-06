use loom::sync::{Arc, Mutex};
use loom::thread;

const BLOCK_COUNT: usize = 8;
const INCARNATION_COUNT: usize = 4;

type Context = [Option<u16>; INCARNATION_COUNT];

#[derive(Clone, Copy)]
struct Block {
    accepted: bool,
    predecessors: [usize; 2],
    predecessor_count: usize,
    delta: Option<(usize, u16)>,
    message: Option<(usize, usize)>,
}

fn block(id: usize) -> Block {
    match id {
        0 => Block {
            accepted: true,
            predecessors: [0, 0],
            predecessor_count: 0,
            delta: Some((1, 1)),
            message: None,
        },
        1 => Block {
            accepted: true,
            predecessors: [0, 0],
            predecessor_count: 0,
            delta: None,
            message: None,
        },
        2 => Block {
            accepted: true,
            predecessors: [0, 0],
            predecessor_count: 0,
            delta: Some((0, 7)),
            message: None,
        },
        3 => Block {
            accepted: false,
            predecessors: [4, 5],
            predecessor_count: 2,
            delta: Some((3, 1)),
            message: None,
        },
        4 => Block {
            accepted: true,
            predecessors: [6, 7],
            predecessor_count: 2,
            delta: None,
            message: Some((2, 1)),
        },
        5 => Block {
            accepted: true,
            predecessors: [0, 0],
            predecessor_count: 0,
            delta: None,
            message: Some((2, 1)),
        },
        6 => Block {
            accepted: true,
            predecessors: [0, 0],
            predecessor_count: 0,
            delta: None,
            message: Some((2, 2)),
        },
        7 => Block {
            accepted: true,
            predecessors: [0, 0],
            predecessor_count: 0,
            delta: None,
            message: Some((2, 2)),
        },
        _ => unreachable!(),
    }
}

fn insert_min(context: &mut Context, incarnation: usize, rank: u16) {
    context[incarnation] = Some(
        context[incarnation]
            .map(|current| current.min(rank))
            .unwrap_or(rank),
    );
}

fn derive_context() -> (Context, Context) {
    let mut pending = vec![2, 3];
    let mut visited = [false; BLOCK_COUNT];
    let mut inherited = [None; INCARNATION_COUNT];
    let mut groups = [[[false; BLOCK_COUNT]; 3]; INCARNATION_COUNT];

    while let Some(id) = pending.pop() {
        if visited[id] {
            continue;
        }
        visited[id] = true;
        let current = block(id);
        if current.accepted {
            if let Some((incarnation, rank)) = current.delta {
                insert_min(&mut inherited, incarnation, rank);
            }
        }
        if let Some((incarnation, sequence)) = current.message {
            groups[incarnation][sequence][id] = true;
        }
        pending.extend_from_slice(&current.predecessors[..current.predecessor_count]);
    }

    let mut effective = inherited;
    for (incarnation, sequences) in groups.iter().enumerate() {
        for (sequence, messages) in sequences.iter().enumerate() {
            let pair = messages
                .iter()
                .enumerate()
                .filter_map(|(id, present)| present.then_some(id))
                .take(2)
                .collect::<Vec<_>>();
            if pair.len() == 2 {
                let rank = (sequence * 64 + pair[0] * 8 + pair[1]) as u16;
                insert_min(&mut effective, incarnation, rank);
            }
        }
    }
    (inherited, effective)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Outcome {
    block: usize,
    ruleset: usize,
    context: Context,
    accepted: bool,
}

struct Replica {
    known: [bool; BLOCK_COUNT],
    ambient_tracker: bool,
    outcome: Option<Outcome>,
}

impl Replica {
    fn receive(&mut self, id: usize) {
        self.known[id] = true;
        self.reconcile();
    }

    fn reconcile(&mut self) {
        if self.outcome.is_some() || self.known.iter().any(|known| !known) {
            return;
        }
        let (inherited, effective) = derive_context();
        assert_eq!(inherited, [Some(7), None, None, None]);
        assert_eq!(effective[0], Some(7));
        assert_eq!(effective[1], None);
        assert_eq!(effective[2], Some(101));
        assert_eq!(effective[3], None);
        self.outcome = Some(Outcome {
            block: BLOCK_COUNT,
            ruleset: 7,
            context: effective,
            accepted: true,
        });
    }
}

#[test]
fn opposite_delivery_and_ambient_tracker_races_preserve_certified_context() {
    loom::model(|| {
        let replica = Arc::new(Mutex::new(Replica {
            known: [false; BLOCK_COUNT],
            ambient_tracker: false,
            outcome: None,
        }));
        let even = {
            let replica = replica.clone();
            thread::spawn(move || {
                for id in [0, 2, 4, 6] {
                    replica.lock().unwrap().receive(id);
                }
            })
        };
        let odd = {
            let replica = replica.clone();
            thread::spawn(move || {
                for id in [7, 5, 3, 1] {
                    replica.lock().unwrap().receive(id);
                }
            })
        };
        let ambient = {
            let replica = replica.clone();
            thread::spawn(move || {
                let mut replica = replica.lock().unwrap();
                replica.ambient_tracker = !replica.ambient_tracker;
            })
        };
        even.join().unwrap();
        odd.join().unwrap();
        ambient.join().unwrap();

        let mut replica = replica.lock().unwrap();
        replica.reconcile();
        let outcome = replica.outcome.expect("complete dependency closure");
        assert!(outcome.accepted);
        assert_eq!(outcome.ruleset, 7);
        assert_eq!(outcome.context, derive_context().1);
    });
}

fn insert_certified(slot: &Mutex<Option<Outcome>>, expected: Outcome, candidate: Outcome) -> bool {
    if candidate != expected {
        return false;
    }
    let mut slot = slot.lock().unwrap();
    match *slot {
        Some(existing) => existing == candidate,
        None => {
            *slot = Some(candidate);
            true
        }
    }
}

#[test]
fn tampered_outcome_cannot_win_a_concurrent_insert_race() {
    loom::model(|| {
        let context = derive_context().1;
        let expected = Outcome {
            block: BLOCK_COUNT,
            ruleset: 7,
            context,
            accepted: true,
        };
        let tampered = Outcome {
            ruleset: 8,
            ..expected
        };
        let slot = Arc::new(Mutex::new(None));
        let valid = {
            let slot = slot.clone();
            thread::spawn(move || insert_certified(&slot, expected, expected))
        };
        let invalid = {
            let slot = slot.clone();
            thread::spawn(move || insert_certified(&slot, expected, tampered))
        };

        assert!(valid.join().unwrap());
        assert!(!invalid.join().unwrap());
        assert_eq!(*slot.lock().unwrap(), Some(expected));
    });
}
