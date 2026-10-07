use std::collections::BTreeSet;

use loom::sync::{Arc, Mutex};
use loom::thread;

struct SettlementInventory {
    available: Mutex<BTreeSet<u8>>,
    recorded: Mutex<BTreeSet<u8>>,
}

impl SettlementInventory {
    fn new(instances: impl IntoIterator<Item = u8>) -> Self {
        Self {
            available: Mutex::new(instances.into_iter().collect()),
            recorded: Mutex::new(BTreeSet::new()),
        }
    }

    fn remove(&self, instance: u8) -> bool {
        let removed = self.available.lock().unwrap().remove(&instance);
        if removed {
            assert!(self.recorded.lock().unwrap().insert(instance));
        }
        removed
    }
}

#[test]
fn the_same_linear_instance_is_removed_and_recorded_once() {
    loom::model(|| {
        let inventory = Arc::new(SettlementInventory::new([7]));
        let left = {
            let inventory = inventory.clone();
            thread::spawn(move || inventory.remove(7))
        };
        let right = {
            let inventory = inventory.clone();
            thread::spawn(move || inventory.remove(7))
        };

        let successes = usize::from(left.join().unwrap()) + usize::from(right.join().unwrap());
        assert_eq!(successes, 1);
        assert!(inventory.available.lock().unwrap().is_empty());
        assert_eq!(*inventory.recorded.lock().unwrap(), BTreeSet::from([7]));
    });
}

#[test]
fn distinct_linear_instances_are_removed_and_recorded_independently() {
    loom::model(|| {
        let inventory = Arc::new(SettlementInventory::new([7, 9]));
        let left = {
            let inventory = inventory.clone();
            thread::spawn(move || inventory.remove(7))
        };
        let right = {
            let inventory = inventory.clone();
            thread::spawn(move || inventory.remove(9))
        };

        assert!(left.join().unwrap());
        assert!(right.join().unwrap());
        assert!(inventory.available.lock().unwrap().is_empty());
        assert_eq!(*inventory.recorded.lock().unwrap(), BTreeSet::from([7, 9]));
    });
}
