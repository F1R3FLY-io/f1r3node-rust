use std::collections::BTreeSet;

use loom::sync::{Arc, Mutex};
use loom::thread;

#[derive(Clone, Copy, Debug)]
struct Ledger {
    deposits: u64,
    balance: u64,
    charges: u64,
    committed_operations: u64,
    recorded_roots: u64,
    version: u64,
}

struct Shard {
    ledger: Mutex<Ledger>,
}

impl Shard {
    fn new(balance: u64) -> Self {
        Self {
            ledger: Mutex::new(Ledger {
                deposits: balance,
                balance,
                charges: 0,
                committed_operations: 0,
                recorded_roots: 0,
                version: 0,
            }),
        }
    }

    fn snapshot(&self) -> Ledger { *self.ledger.lock().unwrap() }

    fn top_up(&self, amount: u64) {
        let mut ledger = self.ledger.lock().unwrap();
        ledger.deposits += amount;
        ledger.balance += amount;
        ledger.version += 1;
    }

    fn charge_with_retry(&self, amount: u64) -> bool {
        loop {
            let observed = self.snapshot();
            if observed.balance < amount {
                return false;
            }
            thread::yield_now();
            let mut ledger = self.ledger.lock().unwrap();
            if ledger.version != observed.version {
                continue;
            }
            ledger.balance -= amount;
            ledger.charges += amount;
            ledger.committed_operations += 1;
            ledger.recorded_roots += 1;
            ledger.version += 1;
            return true;
        }
    }
}

struct WorkerPool {
    capacity: usize,
    owners: Mutex<BTreeSet<usize>>,
}

impl WorkerPool {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            owners: Mutex::new(BTreeSet::new()),
        }
    }

    fn try_acquire(&self, task: usize) -> bool {
        let mut owners = self.owners.lock().unwrap();
        if owners.len() < self.capacity {
            assert!(owners.insert(task));
            true
        } else {
            false
        }
    }

    fn release(&self, task: usize) {
        assert!(self.owners.lock().unwrap().remove(&task));
    }
}

fn assert_conserved(shard: &Shard) {
    let ledger = shard.snapshot();
    assert_eq!(ledger.deposits, ledger.balance + ledger.charges);
    assert_eq!(ledger.recorded_roots, ledger.committed_operations);
}

#[test]
fn concurrent_shard_actions_preserve_frames_and_conservation() {
    loom::model(|| {
        let left = Arc::new(Shard::new(10));
        let right = Arc::new(Shard::new(20));
        let pool = Arc::new(WorkerPool::new(2));

        let left_task = {
            let left = left.clone();
            let pool = pool.clone();
            thread::spawn(move || {
                assert!(pool.try_acquire(1));
                assert!(left.charge_with_retry(7));
                pool.release(1);
            })
        };
        let right_task = {
            let right = right.clone();
            let pool = pool.clone();
            thread::spawn(move || {
                assert!(pool.try_acquire(2));
                right.top_up(5);
                assert!(right.charge_with_retry(9));
                pool.release(2);
            })
        };

        left_task.join().unwrap();
        right_task.join().unwrap();
        assert_conserved(&left);
        assert_conserved(&right);
        let left = left.snapshot();
        let right = right.snapshot();
        assert_eq!((left.deposits, left.balance, left.charges), (10, 3, 7));
        assert_eq!((right.deposits, right.balance, right.charges), (25, 16, 9));
        assert!(pool.owners.lock().unwrap().is_empty());
    });
}

#[test]
fn concurrent_top_up_and_charge_retry_without_lost_update() {
    loom::model(|| {
        let shard = Arc::new(Shard::new(10));
        let top_up = {
            let shard = shard.clone();
            thread::spawn(move || shard.top_up(4))
        };
        let charge = {
            let shard = shard.clone();
            thread::spawn(move || assert!(shard.charge_with_retry(8)))
        };

        top_up.join().unwrap();
        charge.join().unwrap();
        assert_conserved(&shard);
        let ledger = shard.snapshot();
        assert_eq!(
            (ledger.deposits, ledger.balance, ledger.charges),
            (14, 6, 8)
        );
    });
}

#[test]
fn underfunded_foreign_shard_cannot_draw_from_funded_peer() {
    loom::model(|| {
        let funded = Arc::new(Shard::new(12));
        let underfunded = Arc::new(Shard::new(2));
        let funded_task = {
            let funded = funded.clone();
            thread::spawn(move || assert!(funded.charge_with_retry(5)))
        };
        let underfunded_task = {
            let underfunded = underfunded.clone();
            thread::spawn(move || assert!(!underfunded.charge_with_retry(5)))
        };

        funded_task.join().unwrap();
        underfunded_task.join().unwrap();
        assert_conserved(&funded);
        assert_conserved(&underfunded);
        let funded = funded.snapshot();
        let underfunded = underfunded.snapshot();
        assert_eq!((funded.balance, funded.charges), (7, 5));
        assert_eq!((underfunded.balance, underfunded.charges), (2, 0));
    });
}

#[test]
fn racing_charges_cannot_overdraw_a_shard() {
    loom::model(|| {
        let shard = Arc::new(Shard::new(10));
        let left = {
            let shard = shard.clone();
            thread::spawn(move || shard.charge_with_retry(7))
        };
        let right = {
            let shard = shard.clone();
            thread::spawn(move || shard.charge_with_retry(7))
        };

        let outcomes = [left.join().unwrap(), right.join().unwrap()];
        assert_eq!(outcomes.into_iter().filter(|charged| *charged).count(), 1);
        assert_conserved(&shard);
        let ledger = shard.snapshot();
        assert_eq!((ledger.balance, ledger.charges), (3, 7));
    });
}

#[test]
fn racing_worker_acquisitions_never_exceed_capacity() {
    loom::model(|| {
        let pool = Arc::new(WorkerPool::new(1));
        let left = {
            let pool = pool.clone();
            thread::spawn(move || pool.try_acquire(1))
        };
        let right = {
            let pool = pool.clone();
            thread::spawn(move || pool.try_acquire(2))
        };

        let outcomes = [left.join().unwrap(), right.join().unwrap()];
        assert_eq!(outcomes.into_iter().filter(|acquired| *acquired).count(), 1);
        let owner = *pool.owners.lock().unwrap().iter().next().unwrap();
        pool.release(owner);
        assert!(pool.owners.lock().unwrap().is_empty());
    });
}

#[test]
fn crashed_task_releases_its_worker_without_publishing_a_root_or_debit() {
    loom::model(|| {
        let restarted = Arc::new(Shard::new(10));
        let foreign = Arc::new(Shard::new(20));
        let pool = Arc::new(WorkerPool::new(2));

        let interrupted = {
            let pool = pool.clone();
            thread::spawn(move || {
                assert!(pool.try_acquire(1));
                thread::yield_now();
                pool.release(1);
            })
        };
        let foreign_commit = {
            let foreign = foreign.clone();
            let pool = pool.clone();
            thread::spawn(move || {
                assert!(pool.try_acquire(2));
                assert!(foreign.charge_with_retry(6));
                pool.release(2);
            })
        };

        interrupted.join().unwrap();
        foreign_commit.join().unwrap();
        assert_conserved(&restarted);
        assert_conserved(&foreign);
        let before_restart = restarted.snapshot();
        assert_eq!(before_restart.balance, 10);
        assert_eq!(before_restart.charges, 0);
        assert_eq!(before_restart.recorded_roots, 0);

        assert!(pool.try_acquire(1));
        assert!(restarted.charge_with_retry(4));
        pool.release(1);
        assert_conserved(&restarted);
        let after_restart = restarted.snapshot();
        assert_eq!(after_restart.balance, 6);
        assert_eq!(after_restart.charges, 4);
        assert_eq!(after_restart.recorded_roots, 1);
        let foreign = foreign.snapshot();
        assert_eq!(foreign.balance, 14);
        assert_eq!(foreign.charges, 6);
        assert_eq!(foreign.recorded_roots, 1);
        assert!(pool.owners.lock().unwrap().is_empty());
    });
}
