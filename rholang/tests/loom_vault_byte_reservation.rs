use loom::sync::atomic::{AtomicUsize, Ordering};
use loom::sync::{Arc, Mutex};
use loom::thread;

struct VaultReservation {
    unreserved: AtomicUsize,
    reserved: AtomicUsize,
    burned: AtomicUsize,
}

struct PersistentIntroduction {
    charged: Mutex<bool>,
}

impl PersistentIntroduction {
    fn charge_once(&self, vault: &VaultReservation, amount: usize) -> bool {
        let mut charged = self.charged.lock().unwrap();
        if *charged {
            return true;
        }
        if !vault.charge(amount) {
            return false;
        }
        *charged = true;
        true
    }
}

impl VaultReservation {
    fn reserve(&self, amount: usize) -> bool {
        let mut available = self.unreserved.load(Ordering::Acquire);
        loop {
            let Some(remaining) = available.checked_sub(amount) else {
                return false;
            };
            match self.unreserved.compare_exchange(
                available,
                remaining,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.reserved.fetch_add(amount, Ordering::AcqRel);
                    return true;
                }
                Err(actual) => available = actual,
            }
        }
    }

    fn charge(&self, amount: usize) -> bool {
        let mut reserved = self.reserved.load(Ordering::Acquire);
        loop {
            let Some(remaining) = reserved.checked_sub(amount) else {
                return false;
            };
            match self.reserved.compare_exchange(
                reserved,
                remaining,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.burned.fetch_add(amount, Ordering::AcqRel);
                    return true;
                }
                Err(actual) => reserved = actual,
            }
        }
    }

    fn top_up(&self, amount: usize) { self.unreserved.fetch_add(amount, Ordering::AcqRel); }
}

#[test]
fn concurrent_top_up_never_expands_an_existing_reservation() {
    loom::model(|| {
        let vault = Arc::new(VaultReservation {
            unreserved: AtomicUsize::new(100),
            reserved: AtomicUsize::new(0),
            burned: AtomicUsize::new(0),
        });
        assert!(vault.reserve(80));

        let process = {
            let vault = vault.clone();
            thread::spawn(move || {
                assert!(vault.charge(60));
                assert!(!vault.charge(30));
            })
        };
        let credit = {
            let vault = vault.clone();
            thread::spawn(move || vault.top_up(50))
        };

        process.join().unwrap();
        credit.join().unwrap();

        assert_eq!(vault.unreserved.load(Ordering::Acquire), 70);
        assert_eq!(vault.reserved.load(Ordering::Acquire), 20);
        assert_eq!(vault.burned.load(Ordering::Acquire), 60);
    });
}

#[test]
fn racing_byte_charges_cannot_overdraw_the_reserved_ceiling() {
    loom::model(|| {
        let vault = Arc::new(VaultReservation {
            unreserved: AtomicUsize::new(80),
            reserved: AtomicUsize::new(0),
            burned: AtomicUsize::new(0),
        });
        assert!(vault.reserve(80));

        let left = {
            let vault = vault.clone();
            thread::spawn(move || vault.charge(50))
        };
        let right = {
            let vault = vault.clone();
            thread::spawn(move || vault.charge(50))
        };

        let successes = usize::from(left.join().unwrap()) + usize::from(right.join().unwrap());
        assert_eq!(successes, 1);
        assert_eq!(vault.burned.load(Ordering::Acquire), 50);
        assert_eq!(vault.reserved.load(Ordering::Acquire), 30);
        assert_eq!(
            vault.unreserved.load(Ordering::Acquire)
                + vault.reserved.load(Ordering::Acquire)
                + vault.burned.load(Ordering::Acquire),
            80
        );
    });
}

#[test]
fn racing_persistent_retries_charge_the_stable_identity_once() {
    loom::model(|| {
        let vault = Arc::new(VaultReservation {
            unreserved: AtomicUsize::new(9),
            reserved: AtomicUsize::new(0),
            burned: AtomicUsize::new(0),
        });
        assert!(vault.reserve(9));
        let introduction = Arc::new(PersistentIntroduction {
            charged: Mutex::new(false),
        });

        let left = {
            let vault = vault.clone();
            let introduction = introduction.clone();
            thread::spawn(move || introduction.charge_once(&vault, 9))
        };
        let right = {
            let vault = vault.clone();
            let introduction = introduction.clone();
            thread::spawn(move || introduction.charge_once(&vault, 9))
        };

        assert!(left.join().unwrap());
        assert!(right.join().unwrap());
        assert_eq!(vault.burned.load(Ordering::Acquire), 9);
        assert_eq!(vault.reserved.load(Ordering::Acquire), 0);
    });
}

#[test]
fn rejected_persistent_charge_does_not_mark_the_identity_paid() {
    loom::model(|| {
        let vault = VaultReservation {
            unreserved: AtomicUsize::new(8),
            reserved: AtomicUsize::new(0),
            burned: AtomicUsize::new(0),
        };
        assert!(vault.reserve(8));
        let introduction = PersistentIntroduction {
            charged: Mutex::new(false),
        };

        assert!(!introduction.charge_once(&vault, 9));
        assert!(!introduction.charge_once(&vault, 9));
        assert!(!*introduction.charged.lock().unwrap());
        assert_eq!(vault.burned.load(Ordering::Acquire), 0);
        assert_eq!(vault.reserved.load(Ordering::Acquire), 8);
    });
}
