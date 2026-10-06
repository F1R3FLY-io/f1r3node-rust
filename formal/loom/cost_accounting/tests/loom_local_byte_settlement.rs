use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Purse {
    unreserved: usize,
    reserved: usize,
    burned: usize,
}

struct Vault {
    purses: [Mutex<Purse>; 2],
}

struct VectorVault {
    purses: Mutex<[Purse; 2]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Registration {
    Inserted,
    Idempotent,
    Conflict,
}

struct IntroductionRegistry {
    payer: Mutex<Option<usize>>,
}

impl IntroductionRegistry {
    fn new() -> Self {
        Self {
            payer: Mutex::new(None),
        }
    }

    fn register(&self, payer: usize) -> Registration {
        let mut current = self.payer.lock().unwrap();
        match *current {
            Some(existing) if existing == payer => Registration::Idempotent,
            Some(_) => Registration::Conflict,
            None => {
                *current = Some(payer);
                Registration::Inserted
            }
        }
    }

    fn resolve_or_register(&self, fallback: usize) -> usize {
        let mut current = self.payer.lock().unwrap();
        match *current {
            Some(payer) => payer,
            None => {
                *current = Some(fallback);
                fallback
            }
        }
    }

    fn read(&self) -> Option<usize> { *self.payer.lock().unwrap() }
}

impl Vault {
    fn new(outer: usize, continuation: usize) -> Self {
        Self {
            purses: [
                Mutex::new(Purse {
                    unreserved: outer,
                    reserved: 0,
                    burned: 0,
                }),
                Mutex::new(Purse {
                    unreserved: continuation,
                    reserved: 0,
                    burned: 0,
                }),
            ],
        }
    }

    fn reserve(&self, index: usize, amount: usize) -> bool {
        let mut purse = self.purses[index].lock().unwrap();
        let Some(remaining) = purse.unreserved.checked_sub(amount) else {
            return false;
        };
        purse.unreserved = remaining;
        purse.reserved += amount;
        true
    }

    fn settle(&self, index: usize, amount: usize) -> bool {
        let mut purse = self.purses[index].lock().unwrap();
        let Some(remaining) = purse.reserved.checked_sub(amount) else {
            return false;
        };
        purse.reserved = remaining;
        purse.burned += amount;
        true
    }

    fn top_up(&self, index: usize, amount: usize) {
        self.purses[index].lock().unwrap().unreserved += amount;
    }

    fn read(&self, index: usize) -> Purse { *self.purses[index].lock().unwrap() }
}

impl VectorVault {
    fn new(outer: Purse, continuation: Purse) -> Self {
        Self {
            purses: Mutex::new([outer, continuation]),
        }
    }

    fn reserve(&self, draw: [usize; 2]) -> bool {
        let mut purses = self.purses.lock().unwrap();
        let Some(outer) = purses[0].unreserved.checked_sub(draw[0]) else {
            return false;
        };
        let Some(continuation) = purses[1].unreserved.checked_sub(draw[1]) else {
            return false;
        };
        purses[0].unreserved = outer;
        purses[1].unreserved = continuation;
        purses[0].reserved += draw[0];
        purses[1].reserved += draw[1];
        true
    }

    fn settle(&self, draw: [usize; 2]) -> bool {
        let mut purses = self.purses.lock().unwrap();
        let Some(outer) = purses[0].reserved.checked_sub(draw[0]) else {
            return false;
        };
        let Some(continuation) = purses[1].reserved.checked_sub(draw[1]) else {
            return false;
        };
        purses[0].reserved = outer;
        purses[1].reserved = continuation;
        purses[0].burned += draw[0];
        purses[1].burned += draw[1];
        true
    }

    fn read(&self) -> [Purse; 2] { *self.purses.lock().unwrap() }
}

#[test]
fn lollipop_purses_settle_only_their_local_byte_events() {
    loom::model(|| {
        let vault = Arc::new(Vault::new(5, 7));
        assert!(vault.reserve(0, 5));
        assert!(vault.reserve(1, 7));

        let outer = {
            let vault = vault.clone();
            thread::spawn(move || vault.settle(0, 5))
        };
        let continuation = {
            let vault = vault.clone();
            thread::spawn(move || vault.settle(1, 7))
        };

        assert!(outer.join().unwrap());
        assert!(continuation.join().unwrap());
        assert_eq!(vault.read(0), Purse {
            unreserved: 0,
            reserved: 0,
            burned: 5
        });
        assert_eq!(vault.read(1), Purse {
            unreserved: 0,
            reserved: 0,
            burned: 7
        });
    });
}

#[test]
fn concurrent_top_up_cannot_rescue_or_expand_an_inflight_local_reservation() {
    loom::model(|| {
        let vault = Arc::new(Vault::new(5, 7));
        assert!(vault.reserve(0, 5));
        assert!(vault.reserve(1, 7));

        let rejected = {
            let vault = vault.clone();
            thread::spawn(move || vault.settle(0, 6))
        };
        let credit = {
            let vault = vault.clone();
            thread::spawn(move || vault.top_up(0, 100))
        };
        let continuation = {
            let vault = vault.clone();
            thread::spawn(move || vault.settle(1, 7))
        };

        assert!(!rejected.join().unwrap());
        credit.join().unwrap();
        assert!(continuation.join().unwrap());
        assert_eq!(vault.read(0), Purse {
            unreserved: 100,
            reserved: 5,
            burned: 0
        });
        assert_eq!(vault.read(1), Purse {
            unreserved: 0,
            reserved: 0,
            burned: 7
        });
    });
}

#[test]
fn racing_local_settlements_preserve_per_purse_conservation() {
    loom::model(|| {
        let vault = Arc::new(Vault::new(10, 10));
        assert!(vault.reserve(0, 10));
        assert!(vault.reserve(1, 10));

        let left = {
            let vault = vault.clone();
            thread::spawn(move || vault.settle(0, 8))
        };
        let wrong_purse = {
            let vault = vault.clone();
            thread::spawn(move || vault.settle(1, 12))
        };

        assert!(left.join().unwrap());
        assert!(!wrong_purse.join().unwrap());
        let outer = vault.read(0);
        let continuation = vault.read(1);
        assert_eq!(outer.unreserved + outer.reserved + outer.burned, 10);
        assert_eq!(
            continuation.unreserved + continuation.reserved + continuation.burned,
            10
        );
        assert_eq!(outer.burned, 8);
        assert_eq!(continuation.burned, 0);
    });
}

#[test]
fn compound_reservation_is_atomic_across_every_component_purse() {
    loom::model(|| {
        let vault = Arc::new(VectorVault::new(
            Purse {
                unreserved: 5,
                reserved: 0,
                burned: 0,
            },
            Purse {
                unreserved: 5,
                reserved: 0,
                burned: 0,
            },
        ));
        let compound = {
            let vault = vault.clone();
            thread::spawn(move || vault.reserve([5, 5]))
        };
        let outer = {
            let vault = vault.clone();
            thread::spawn(move || vault.reserve([5, 0]))
        };

        let compound = compound.join().unwrap();
        let outer = outer.join().unwrap();
        assert_ne!(compound, outer);
        let purses = vault.read();
        if compound {
            assert_eq!(purses[0].reserved, 5);
            assert_eq!(purses[1].reserved, 5);
        } else {
            assert_eq!(purses[0].reserved, 5);
            assert_eq!(purses[1].unreserved, 5);
            assert_eq!(purses[1].reserved, 0);
        }
    });
}

#[test]
fn compound_settlement_cannot_commit_a_partial_component_debit() {
    loom::model(|| {
        let vault = Arc::new(VectorVault::new(
            Purse {
                unreserved: 0,
                reserved: 5,
                burned: 0,
            },
            Purse {
                unreserved: 0,
                reserved: 4,
                burned: 0,
            },
        ));
        let invalid_compound = {
            let vault = vault.clone();
            thread::spawn(move || vault.settle([5, 5]))
        };
        let valid_outer = {
            let vault = vault.clone();
            thread::spawn(move || vault.settle([5, 0]))
        };

        assert!(!invalid_compound.join().unwrap());
        assert!(valid_outer.join().unwrap());
        assert_eq!(vault.read(), [
            Purse {
                unreserved: 0,
                reserved: 0,
                burned: 5,
            },
            Purse {
                unreserved: 0,
                reserved: 4,
                burned: 0,
            },
        ]);
    });
}

#[test]
fn concurrent_introduction_registration_cannot_overwrite_or_mix_payers() {
    loom::model(|| {
        let registry = Arc::new(IntroductionRegistry::new());
        let left = {
            let registry = registry.clone();
            thread::spawn(move || registry.register(1))
        };
        let right = {
            let registry = registry.clone();
            thread::spawn(move || registry.register(2))
        };

        let left = left.join().unwrap();
        let right = right.join().unwrap();
        assert!(matches!(
            (left, right),
            (Registration::Inserted, Registration::Conflict)
                | (Registration::Conflict, Registration::Inserted)
        ));
        assert!(matches!(registry.read(), Some(1) | Some(2)));
    });
}

#[test]
fn concurrent_same_payer_registration_is_idempotent() {
    loom::model(|| {
        let registry = Arc::new(IntroductionRegistry::new());
        let left = {
            let registry = registry.clone();
            thread::spawn(move || registry.register(7))
        };
        let right = {
            let registry = registry.clone();
            thread::spawn(move || registry.register(7))
        };

        let left = left.join().unwrap();
        let right = right.join().unwrap();
        assert!(matches!(
            (left, right),
            (Registration::Inserted, Registration::Idempotent)
                | (Registration::Idempotent, Registration::Inserted)
        ));
        assert_eq!(registry.read(), Some(7));
    });
}

#[test]
fn fallback_resolution_and_explicit_registration_have_one_linearized_payer() {
    loom::model(|| {
        let registry = Arc::new(IntroductionRegistry::new());
        let fallback = {
            let registry = registry.clone();
            thread::spawn(move || registry.resolve_or_register(1))
        };
        let explicit = {
            let registry = registry.clone();
            thread::spawn(move || registry.register(2))
        };

        let fallback = fallback.join().unwrap();
        let explicit = explicit.join().unwrap();
        let committed = registry.read().unwrap();
        assert_eq!(fallback, committed);
        assert!(matches!(
            (committed, explicit),
            (1, Registration::Conflict) | (2, Registration::Inserted)
        ));
    });
}
