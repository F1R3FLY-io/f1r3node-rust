use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

const HANDLER_COST: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Quarantine {
    generation: usize,
    amount: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Resolution {
    generation: usize,
    penalty: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VaultState {
    general: usize,
    fuel: usize,
    quarantine: Option<Quarantine>,
    resolution: Option<Resolution>,
}

struct VaultCell {
    state: Mutex<VaultState>,
}

impl VaultCell {
    fn new(general: usize, fuel: usize) -> Self {
        Self {
            state: Mutex::new(VaultState {
                general,
                fuel,
                quarantine: None,
                resolution: None,
            }),
        }
    }

    fn snapshot(&self) -> VaultState { *self.state.lock().unwrap() }

    fn top_up(&self, amount: usize) -> bool {
        let mut state = self.state.lock().unwrap();
        if amount == 0 || state.general < amount || state.quarantine.is_some() {
            return false;
        }
        state.general -= amount;
        state.fuel += amount;
        true
    }

    fn charge_handler(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.quarantine.is_some() || state.fuel < HANDLER_COST {
            return false;
        }
        state.fuel -= HANDLER_COST;
        true
    }

    fn bond(&self, amount: usize) -> bool {
        let mut state = self.state.lock().unwrap();
        if amount == 0 || state.general < amount {
            return false;
        }
        state.general -= amount;
        true
    }

    fn credit_fee(&self, amount: usize) { self.state.lock().unwrap().general += amount; }

    fn quarantine(&self, generation: usize) -> bool {
        let mut state = self.state.lock().unwrap();
        if let Some(existing) = state.quarantine {
            return existing.generation == generation;
        }
        if state
            .resolution
            .is_some_and(|resolution| generation <= resolution.generation)
        {
            return false;
        }
        let amount = state.fuel;
        state.fuel = 0;
        state.quarantine = Some(Quarantine { generation, amount });
        true
    }

    fn resolve(&self, generation: usize, penalty: usize) -> bool {
        let mut state = self.state.lock().unwrap();
        if let Some(resolution) = state.resolution {
            return resolution
                == (Resolution {
                    generation,
                    penalty,
                });
        }
        let Some(quarantine) = state.quarantine else {
            return false;
        };
        if quarantine.generation != generation {
            return false;
        }
        let penalty = penalty.min(quarantine.amount);
        state.fuel += quarantine.amount - penalty;
        state.quarantine = None;
        state.resolution = Some(Resolution {
            generation,
            penalty,
        });
        true
    }
}

fn reserve(fuel: &AtomicUsize) -> bool {
    let mut available = fuel.load(Ordering::Acquire);
    loop {
        if available < HANDLER_COST {
            return false;
        }
        match fuel.compare_exchange_weak(
            available,
            available - HANDLER_COST,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return true,
            Err(current) => available = current,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FuelSnapshot {
    root: usize,
    validator: usize,
    fuel: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SettlementState {
    root: usize,
    validator: usize,
    fuel: usize,
    burned: usize,
}

struct SettlementCell {
    state: Mutex<SettlementState>,
}

impl SettlementCell {
    fn new(root: usize, validator: usize, fuel: usize) -> Self {
        Self {
            state: Mutex::new(SettlementState {
                root,
                validator,
                fuel,
                burned: 0,
            }),
        }
    }

    fn snapshot(&self) -> FuelSnapshot {
        let state = self.state.lock().unwrap();
        FuelSnapshot {
            root: state.root,
            validator: state.validator,
            fuel: state.fuel,
        }
    }

    fn settle(&self, snapshot: FuelSnapshot, amount: usize) -> bool {
        let mut state = self.state.lock().unwrap();
        if snapshot.root != state.root
            || snapshot.validator != state.validator
            || snapshot.fuel != state.fuel
            || amount > state.fuel
        {
            return false;
        }
        state.fuel -= amount;
        state.burned += amount;
        state.root += 1;
        true
    }

    fn state(&self) -> SettlementState { *self.state.lock().unwrap() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReplaySessionState {
    cursor: usize,
    fuel: usize,
    burned: usize,
}

struct ReplaySessionCell {
    state: Mutex<ReplaySessionState>,
}

impl ReplaySessionCell {
    fn new(snapshot: FuelSnapshot) -> Self {
        Self {
            state: Mutex::new(ReplaySessionState {
                cursor: 0,
                fuel: snapshot.fuel,
                burned: 0,
            }),
        }
    }

    fn replay_next(&self, expected_cursor: usize, amount: usize) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.cursor != expected_cursor || state.fuel < amount {
            return false;
        }
        state.cursor += 1;
        state.fuel -= amount;
        state.burned += amount;
        true
    }

    fn state(&self) -> ReplaySessionState { *self.state.lock().unwrap() }
}

#[test]
fn top_up_and_handler_charge_are_serializable() {
    loom::model(|| {
        let vault = Arc::new(VaultCell::new(10, 3));
        let top_up = {
            let vault = Arc::clone(&vault);
            thread::spawn(move || vault.top_up(4))
        };
        let handler = {
            let vault = Arc::clone(&vault);
            thread::spawn(move || vault.charge_handler())
        };
        assert!(top_up.join().unwrap());
        assert!(handler.join().unwrap());
        assert_eq!(vault.snapshot(), VaultState {
            general: 6,
            fuel: 4,
            quarantine: None,
            resolution: None,
        });
    });
}

#[test]
fn low_fuel_race_never_uses_uncommitted_top_up() {
    loom::model(|| {
        let vault = Arc::new(VaultCell::new(10, 0));
        let top_up = {
            let vault = Arc::clone(&vault);
            thread::spawn(move || vault.top_up(4))
        };
        let handler = {
            let vault = Arc::clone(&vault);
            thread::spawn(move || vault.charge_handler())
        };
        assert!(top_up.join().unwrap());
        let charged = handler.join().unwrap();
        let state = vault.snapshot();
        assert_eq!(state.general, 6);
        assert_eq!(state.fuel, if charged { 1 } else { 4 });
        assert_eq!(
            state.general + state.fuel + usize::from(charged) * HANDLER_COST,
            10
        );
    });
}

#[test]
fn top_up_and_quarantine_preserve_all_custody() {
    loom::model(|| {
        let vault = Arc::new(VaultCell::new(10, 3));
        let top_up = {
            let vault = Arc::clone(&vault);
            thread::spawn(move || vault.top_up(4))
        };
        let quarantine = {
            let vault = Arc::clone(&vault);
            thread::spawn(move || vault.quarantine(1))
        };
        let topped_up = top_up.join().unwrap();
        assert!(quarantine.join().unwrap());
        let state = vault.snapshot();
        assert_eq!(state.fuel, 0);
        let quarantined = state.quarantine.unwrap();
        assert_eq!(quarantined.generation, 1);
        assert_eq!(
            (state.general, quarantined.amount),
            if topped_up { (6, 7) } else { (10, 3) }
        );
        assert_eq!(state.general + quarantined.amount, 13);
    });
}

#[test]
fn resolution_and_top_up_are_serializable() {
    loom::model(|| {
        let vault = Arc::new(VaultCell::new(10, 3));
        assert!(vault.quarantine(1));
        let resolve = {
            let vault = Arc::clone(&vault);
            thread::spawn(move || vault.resolve(1, 0))
        };
        let top_up = {
            let vault = Arc::clone(&vault);
            thread::spawn(move || vault.top_up(4))
        };
        assert!(resolve.join().unwrap());
        let topped_up = top_up.join().unwrap();
        let state = vault.snapshot();
        assert_eq!(
            (state.general, state.fuel),
            if topped_up { (6, 7) } else { (10, 3) }
        );
        assert_eq!(state.general + state.fuel, 13);
        assert_eq!(
            state.resolution,
            Some(Resolution {
                generation: 1,
                penalty: 0
            })
        );
    });
}

#[test]
fn sibling_reservations_cannot_overdraw_validator_fuel() {
    loom::model(|| {
        let fuel = Arc::new(AtomicUsize::new(6));
        let first = {
            let fuel = Arc::clone(&fuel);
            thread::spawn(move || reserve(&fuel))
        };
        let second = {
            let fuel = Arc::clone(&fuel);
            thread::spawn(move || reserve(&fuel))
        };
        let third = {
            let fuel = Arc::clone(&fuel);
            thread::spawn(move || reserve(&fuel))
        };
        let accepted = usize::from(first.join().unwrap())
            + usize::from(second.join().unwrap())
            + usize::from(third.join().unwrap());
        assert_eq!(accepted, 2);
        assert_eq!(fuel.load(Ordering::Acquire), 0);
    });
}

#[test]
fn disjoint_validators_charge_without_shared_state() {
    loom::model(|| {
        let first = Arc::new(VaultCell::new(8, 3));
        let second = Arc::new(VaultCell::new(13, 6));
        let first_charge = {
            let first = Arc::clone(&first);
            thread::spawn(move || first.charge_handler())
        };
        let second_charge = {
            let second = Arc::clone(&second);
            thread::spawn(move || second.charge_handler())
        };
        assert!(first_charge.join().unwrap());
        assert!(second_charge.join().unwrap());
        assert_eq!((first.snapshot().general, first.snapshot().fuel), (8, 0));
        assert_eq!((second.snapshot().general, second.snapshot().fuel), (13, 3));
    });
}

#[test]
fn bond_fee_and_handler_use_their_exact_roles() {
    loom::model(|| {
        let vault = Arc::new(VaultCell::new(10, 6));
        let bond = {
            let vault = Arc::clone(&vault);
            thread::spawn(move || vault.bond(4))
        };
        let settle = {
            let vault = Arc::clone(&vault);
            thread::spawn(move || {
                vault.credit_fee(1);
                vault.charge_handler()
            })
        };
        assert!(bond.join().unwrap());
        assert!(settle.join().unwrap());
        assert_eq!((vault.snapshot().general, vault.snapshot().fuel), (7, 3));
    });
}

#[test]
fn stale_and_duplicate_resolutions_do_not_change_custody() {
    loom::model(|| {
        let vault = VaultCell::new(10, 7);
        assert!(vault.quarantine(2));
        assert!(!vault.resolve(1, 3));
        assert_eq!(vault.snapshot().quarantine.unwrap().amount, 7);
        assert!(vault.resolve(2, 3));
        let resolved = vault.snapshot();
        assert_eq!((resolved.general, resolved.fuel), (10, 4));
        assert!(vault.resolve(2, 3));
        assert_eq!(vault.snapshot(), resolved);
        assert!(!vault.resolve(2, 2));
        assert_eq!(vault.snapshot(), resolved);
    });
}

#[test]
fn stale_snapshot_cannot_commit_after_concurrent_settlement() {
    loom::model(|| {
        let settlement = Arc::new(SettlementCell::new(11, 7, 3));
        let snapshot = settlement.snapshot();
        let first = {
            let settlement = Arc::clone(&settlement);
            thread::spawn(move || settlement.settle(snapshot, HANDLER_COST))
        };
        let second = {
            let settlement = Arc::clone(&settlement);
            thread::spawn(move || settlement.settle(snapshot, HANDLER_COST))
        };
        let commits = usize::from(first.join().unwrap()) + usize::from(second.join().unwrap());
        assert_eq!(commits, 1);
        assert_eq!(settlement.state(), SettlementState {
            root: 12,
            validator: 7,
            fuel: 0,
            burned: 3,
        });
    });
}

#[test]
fn concurrent_replay_sessions_do_not_share_certificate_cursors() {
    loom::model(|| {
        let snapshot = FuelSnapshot {
            root: 11,
            validator: 7,
            fuel: 6,
        };
        let first = Arc::new(ReplaySessionCell::new(snapshot));
        let second = Arc::new(ReplaySessionCell::new(snapshot));
        let first_run = {
            let first = Arc::clone(&first);
            thread::spawn(move || {
                assert!(first.replay_next(0, HANDLER_COST));
                assert!(first.replay_next(1, HANDLER_COST));
            })
        };
        let second_run = {
            let second = Arc::clone(&second);
            thread::spawn(move || {
                assert!(second.replay_next(0, HANDLER_COST));
                assert!(second.replay_next(1, HANDLER_COST));
            })
        };
        first_run.join().unwrap();
        second_run.join().unwrap();
        let expected = ReplaySessionState {
            cursor: 2,
            fuel: 0,
            burned: 6,
        };
        assert_eq!(first.state(), expected);
        assert_eq!(second.state(), expected);
    });
}

#[test]
fn disjoint_validator_snapshots_settle_without_cross_effects() {
    loom::model(|| {
        let first = Arc::new(SettlementCell::new(11, 7, 3));
        let second = Arc::new(SettlementCell::new(19, 8, 6));
        let first_snapshot = first.snapshot();
        let second_snapshot = second.snapshot();
        let first_settlement = {
            let first = Arc::clone(&first);
            thread::spawn(move || first.settle(first_snapshot, HANDLER_COST))
        };
        let second_settlement = {
            let second = Arc::clone(&second);
            thread::spawn(move || second.settle(second_snapshot, HANDLER_COST))
        };
        assert!(first_settlement.join().unwrap());
        assert!(second_settlement.join().unwrap());
        assert_eq!(first.state(), SettlementState {
            root: 12,
            validator: 7,
            fuel: 0,
            burned: 3,
        });
        assert_eq!(second.state(), SettlementState {
            root: 20,
            validator: 8,
            fuel: 3,
            burned: 3,
        });
    });
}
