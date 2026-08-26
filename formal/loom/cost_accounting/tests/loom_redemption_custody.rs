use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    Bonded,
    PendingWithdraw,
    Withdrawing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Bonded,
    PendingWithdraw,
    Withdrawing,
    Quarantined(Origin),
    Burned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Vindicated,
    Guilty { penalty: usize },
    Burned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Request {
    generation: usize,
    outcome: Outcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct State {
    phase: Phase,
    generation: usize,
    bond: usize,
    reward: usize,
    fuel: usize,
    cooperative_stake: usize,
    cooperative_fuel: usize,
    burned_stake: usize,
    burned_fuel: usize,
    receipt: Option<Request>,
    version: usize,
    lock_owner: Option<usize>,
    minting_halted: bool,
}

impl State {
    fn new(origin: Origin, generation: usize) -> Self {
        Self {
            phase: Phase::Quarantined(origin),
            generation,
            bond: 4,
            reward: 2,
            fuel: 3,
            cooperative_stake: 0,
            cooperative_fuel: 0,
            burned_stake: 0,
            burned_fuel: 0,
            receipt: None,
            version: 0,
            lock_owner: None,
            minting_halted: true,
        }
    }

    fn stake_total(self) -> usize {
        self.bond + self.reward + self.cooperative_stake + self.burned_stake
    }

    fn fuel_total(self) -> usize { self.fuel + self.cooperative_fuel + self.burned_fuel }
}

struct Ledger {
    state: Mutex<State>,
}

enum Begin {
    Transaction(Transaction),
    Idempotent,
    Rejected,
}

struct Transaction {
    ledger: Arc<Ledger>,
    worker: usize,
    request: Request,
    version: usize,
    origin: Origin,
    active: bool,
}

impl Ledger {
    fn new(origin: Origin, generation: usize) -> Self {
        Self {
            state: Mutex::new(State::new(origin, generation)),
        }
    }

    fn read(&self) -> State { *self.state.lock().unwrap() }

    fn begin(ledger: &Arc<Self>, worker: usize, request: Request) -> Begin {
        let mut state = ledger.state.lock().unwrap();
        if let Some(receipt) = state.receipt {
            return if receipt == request {
                Begin::Idempotent
            } else {
                Begin::Rejected
            };
        }
        let Phase::Quarantined(origin) = state.phase else {
            return Begin::Rejected;
        };
        if request.generation != state.generation
            || state.lock_owner.is_some()
            || matches!(request.outcome, Outcome::Guilty { penalty } if penalty >= state.bond)
        {
            return Begin::Rejected;
        }
        state.lock_owner = Some(worker);
        Begin::Transaction(Transaction {
            ledger: ledger.clone(),
            worker,
            request,
            version: state.version,
            origin,
            active: true,
        })
    }
}

impl Transaction {
    fn commit(mut self) -> bool {
        let mut state = self.ledger.state.lock().unwrap();
        if state.lock_owner != Some(self.worker)
            || state.version != self.version
            || state.generation != self.request.generation
            || state.receipt.is_some()
            || state.phase != Phase::Quarantined(self.origin)
        {
            return false;
        }
        match self.request.outcome {
            Outcome::Vindicated => {
                state.phase = match self.origin {
                    Origin::Bonded => Phase::Bonded,
                    Origin::PendingWithdraw => Phase::PendingWithdraw,
                    Origin::Withdrawing => Phase::Withdrawing,
                };
                state.minting_halted = false;
            }
            Outcome::Guilty { penalty } => {
                if penalty >= state.bond {
                    return false;
                }
                let fuel_penalty = penalty.min(state.fuel);
                state.bond -= penalty;
                state.cooperative_stake += penalty;
                state.fuel -= fuel_penalty;
                state.cooperative_fuel += fuel_penalty;
                state.phase = match self.origin {
                    Origin::Bonded => Phase::Bonded,
                    Origin::PendingWithdraw => Phase::PendingWithdraw,
                    Origin::Withdrawing => Phase::Withdrawing,
                };
                state.minting_halted = false;
            }
            Outcome::Burned => {
                state.burned_stake += state.bond + state.reward;
                state.bond = 0;
                state.reward = 0;
                state.burned_fuel += state.fuel;
                state.fuel = 0;
                state.phase = Phase::Burned;
                state.minting_halted = true;
            }
        }
        state.receipt = Some(self.request);
        state.version += 1;
        state.lock_owner = None;
        self.active = false;
        true
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut state = self.ledger.state.lock().unwrap();
        if state.lock_owner == Some(self.worker) && state.version == self.version {
            state.lock_owner = None;
        }
        self.active = false;
    }
}

fn resolve(ledger: &Arc<Ledger>, worker: usize, request: Request) -> bool {
    match Ledger::begin(ledger, worker, request) {
        Begin::Transaction(transaction) => transaction.commit(),
        Begin::Idempotent => true,
        Begin::Rejected => false,
    }
}

#[test]
fn same_generation_conflicts_commit_exactly_one_conserving_resolution() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::new(Origin::PendingWithdraw, 3));
        let initial = ledger.read();
        let vindication = Request {
            generation: 3,
            outcome: Outcome::Vindicated,
        };
        let guilty = Request {
            generation: 3,
            outcome: Outcome::Guilty { penalty: 2 },
        };
        let left = {
            let ledger = ledger.clone();
            thread::spawn(move || resolve(&ledger, 1, vindication))
        };
        let right = {
            let ledger = ledger.clone();
            thread::spawn(move || resolve(&ledger, 2, guilty))
        };
        let accepted = usize::from(left.join().unwrap()) + usize::from(right.join().unwrap());
        assert_eq!(accepted, 1);
        let state = ledger.read();
        assert!(state.receipt == Some(vindication) || state.receipt == Some(guilty));
        assert_eq!(state.lock_owner, None);
        assert_eq!(state.version, 1);
        assert_eq!(state.stake_total(), initial.stake_total());
        assert_eq!(state.fuel_total(), initial.fuel_total());
        match state.receipt.unwrap().outcome {
            Outcome::Vindicated => {
                assert_eq!(state.phase, Phase::PendingWithdraw);
                assert_eq!(state.bond, initial.bond);
                assert!(!state.minting_halted);
            }
            Outcome::Guilty { penalty } => {
                assert_eq!(state.phase, Phase::PendingWithdraw);
                assert_eq!(state.bond + penalty, initial.bond);
                assert_eq!(state.cooperative_stake, penalty);
                assert!(!state.minting_halted);
            }
            Outcome::Burned => unreachable!(),
        }
    });
}

#[test]
fn stale_generation_is_effect_free_during_current_resolution() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::new(Origin::Withdrawing, 4));
        let initial = ledger.read();
        let stale = {
            let ledger = ledger.clone();
            thread::spawn(move || {
                resolve(&ledger, 1, Request {
                    generation: 3,
                    outcome: Outcome::Burned,
                })
            })
        };
        let current = {
            let ledger = ledger.clone();
            thread::spawn(move || {
                resolve(&ledger, 2, Request {
                    generation: 4,
                    outcome: Outcome::Vindicated,
                })
            })
        };
        assert!(!stale.join().unwrap());
        assert!(current.join().unwrap());
        let state = ledger.read();
        assert_eq!(state.phase, Phase::Withdrawing);
        assert_eq!(state.generation, 4);
        assert!(!state.minting_halted);
        assert_eq!(state.stake_total(), initial.stake_total());
        assert_eq!(state.fuel_total(), initial.fuel_total());
        assert_eq!(state.version, 1);
    });
}

#[test]
fn full_guilty_confiscation_is_effect_free_during_valid_resolution() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::new(Origin::Bonded, 6));
        let initial = ledger.read();
        let invalid = {
            let ledger = ledger.clone();
            thread::spawn(move || {
                resolve(&ledger, 1, Request {
                    generation: 6,
                    outcome: Outcome::Guilty { penalty: 4 },
                })
            })
        };
        let valid = {
            let ledger = ledger.clone();
            thread::spawn(move || {
                resolve(&ledger, 2, Request {
                    generation: 6,
                    outcome: Outcome::Vindicated,
                })
            })
        };
        assert!(!invalid.join().unwrap());
        assert!(valid.join().unwrap());
        let state = ledger.read();
        assert_eq!(state.phase, Phase::Bonded);
        assert_eq!(state.bond, initial.bond);
        assert_eq!(state.reward, initial.reward);
        assert_eq!(state.cooperative_stake, 0);
        assert_eq!(state.receipt.unwrap().outcome, Outcome::Vindicated);
        assert_eq!(state.stake_total(), initial.stake_total());
        assert_eq!(state.fuel_total(), initial.fuel_total());
    });
}

#[test]
fn exact_retries_are_idempotent_and_conflicting_retries_are_effect_free() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::new(Origin::Bonded, 2));
        let request = Request {
            generation: 2,
            outcome: Outcome::Guilty { penalty: 1 },
        };
        assert!(resolve(&ledger, 0, request));
        let committed = ledger.read();
        let left = {
            let ledger = ledger.clone();
            thread::spawn(move || resolve(&ledger, 1, request))
        };
        let right = {
            let ledger = ledger.clone();
            thread::spawn(move || {
                resolve(&ledger, 2, Request {
                    generation: 2,
                    outcome: Outcome::Burned,
                })
            })
        };
        assert!(left.join().unwrap());
        assert!(!right.join().unwrap());
        assert_eq!(ledger.read(), committed);
    });
}

#[test]
fn aborted_staging_releases_only_its_lock_and_publishes_nothing() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::new(Origin::Withdrawing, 5));
        let initial = ledger.read();
        let request = Request {
            generation: 5,
            outcome: Outcome::Burned,
        };
        let aborted = Ledger::begin(&ledger, 1, request);
        let Begin::Transaction(aborted) = aborted else {
            unreachable!();
        };
        let contender = {
            let ledger = ledger.clone();
            thread::spawn(move || resolve(&ledger, 2, request))
        };
        drop(aborted);
        let _ = contender.join().unwrap();
        let after_race = ledger.read();
        assert_eq!(after_race.lock_owner, None);
        assert_eq!(after_race.stake_total(), initial.stake_total());
        assert_eq!(after_race.fuel_total(), initial.fuel_total());
        if after_race.receipt.is_none() {
            assert_eq!(after_race, initial);
            assert!(resolve(&ledger, 3, request));
        }
        let committed = ledger.read();
        assert_eq!(committed.receipt, Some(request));
        assert_eq!(committed.phase, Phase::Burned);
        assert!(committed.minting_halted);
        assert_eq!(committed.version, 1);
        assert_eq!(committed.lock_owner, None);
        assert_eq!(committed.stake_total(), initial.stake_total());
        assert_eq!(committed.fuel_total(), initial.fuel_total());
    });
}

#[test]
fn distinct_validator_resolutions_have_no_shared_lock() {
    loom::model(|| {
        let left = Arc::new(Ledger::new(Origin::Bonded, 1));
        let right = Arc::new(Ledger::new(Origin::Withdrawing, 8));
        let left_initial = left.read();
        let right_initial = right.read();
        let left_worker = {
            let ledger = left.clone();
            thread::spawn(move || {
                resolve(&ledger, 1, Request {
                    generation: 1,
                    outcome: Outcome::Guilty { penalty: 1 },
                })
            })
        };
        let right_worker = {
            let ledger = right.clone();
            thread::spawn(move || {
                resolve(&ledger, 2, Request {
                    generation: 8,
                    outcome: Outcome::Burned,
                })
            })
        };
        assert!(left_worker.join().unwrap());
        assert!(right_worker.join().unwrap());
        let left_state = left.read();
        let right_state = right.read();
        assert_eq!(left_state.version, 1);
        assert_eq!(right_state.version, 1);
        assert_eq!(left_state.stake_total(), left_initial.stake_total());
        assert_eq!(left_state.fuel_total(), left_initial.fuel_total());
        assert_eq!(right_state.stake_total(), right_initial.stake_total());
        assert_eq!(right_state.fuel_total(), right_initial.fuel_total());
    });
}
