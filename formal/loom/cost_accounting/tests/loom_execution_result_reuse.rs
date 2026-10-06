use loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use loom::sync::Arc;
use loom::thread;

struct PersistentRewriteAccounting {
    installed: AtomicBool,
    installation_charges: AtomicUsize,
    firing_charges: AtomicUsize,
}

impl PersistentRewriteAccounting {
    fn new() -> Self {
        Self {
            installed: AtomicBool::new(false),
            installation_charges: AtomicUsize::new(0),
            firing_charges: AtomicUsize::new(0),
        }
    }

    fn install(&self) {
        if self
            .installed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.installation_charges.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn fire(&self) -> bool {
        if !self.installed.load(Ordering::Acquire) {
            return false;
        }
        self.firing_charges.fetch_add(1, Ordering::AcqRel);
        true
    }
}

#[test]
fn concurrent_reinstallation_charges_once_and_each_completed_firing_charges() {
    loom::model(|| {
        let accounting = Arc::new(PersistentRewriteAccounting::new());
        let first_install = {
            let accounting = accounting.clone();
            thread::spawn(move || accounting.install())
        };
        let second_install = {
            let accounting = accounting.clone();
            thread::spawn(move || accounting.install())
        };

        first_install.join().unwrap();
        second_install.join().unwrap();
        assert_eq!(accounting.installation_charges.load(Ordering::Acquire), 1);

        let first_fire = {
            let accounting = accounting.clone();
            thread::spawn(move || assert!(accounting.fire()))
        };
        let second_fire = {
            let accounting = accounting.clone();
            thread::spawn(move || assert!(accounting.fire()))
        };

        first_fire.join().unwrap();
        second_fire.join().unwrap();
        assert_eq!(accounting.firing_charges.load(Ordering::Acquire), 2);
    });
}

#[test]
fn persistent_firing_cannot_charge_before_installation() {
    loom::model(|| {
        let accounting = PersistentRewriteAccounting::new();
        assert!(!accounting.fire());
        assert_eq!(accounting.firing_charges.load(Ordering::Acquire), 0);
        accounting.install();
        assert!(accounting.fire());
        assert_eq!(accounting.installation_charges.load(Ordering::Acquire), 1);
        assert_eq!(accounting.firing_charges.load(Ordering::Acquire), 1);
    });
}
