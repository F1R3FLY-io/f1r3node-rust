use loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use loom::sync::{mpsc, Arc, Mutex};
use loom::thread;

struct Budget {
    byte_capacity: usize,
    item_capacity: usize,
    bytes: AtomicUsize,
    items: AtomicUsize,
}

struct Reservation {
    budget: Arc<Budget>,
    bytes: usize,
}

impl Budget {
    fn try_reserve(budget: &Arc<Self>, bytes: usize) -> Option<Reservation> {
        let previous_items = budget.items.fetch_add(1, Ordering::SeqCst);
        if previous_items >= budget.item_capacity {
            budget.items.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        let mut current = budget.bytes.load(Ordering::SeqCst);
        loop {
            let Some(next) = current
                .checked_add(bytes)
                .filter(|next| *next <= budget.byte_capacity)
            else {
                budget.items.fetch_sub(1, Ordering::SeqCst);
                return None;
            };
            match budget.bytes.compare_exchange_weak(
                current,
                next,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    return Some(Reservation {
                        budget: budget.clone(),
                        bytes,
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        assert!(self.budget.bytes.fetch_sub(self.bytes, Ordering::SeqCst) >= self.bytes);
        assert!(self.budget.items.fetch_sub(1, Ordering::SeqCst) >= 1);
    }
}

struct Gate {
    state: AtomicUsize,
}

struct DecoderGate {
    active: AtomicUsize,
    bytes: AtomicUsize,
    handler_limit: usize,
    max_message_bytes: usize,
}

struct DecoderPermit {
    gate: Arc<DecoderGate>,
    bytes: usize,
}

impl DecoderGate {
    fn try_enter(gate: &Arc<Self>, bytes: usize) -> Option<DecoderPermit> {
        assert!(bytes <= gate.max_message_bytes);
        let mut active = gate.active.load(Ordering::Acquire);
        loop {
            if active >= gate.handler_limit {
                return None;
            }
            match gate.active.compare_exchange_weak(
                active,
                active + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(observed) => active = observed,
            }
        }
        let previous = gate.bytes.fetch_add(bytes, Ordering::AcqRel);
        assert!(previous + bytes <= gate.handler_limit * gate.max_message_bytes);
        Some(DecoderPermit {
            gate: gate.clone(),
            bytes,
        })
    }
}

impl Drop for DecoderPermit {
    fn drop(&mut self) {
        assert!(self.gate.bytes.fetch_sub(self.bytes, Ordering::AcqRel) >= self.bytes);
        assert!(self.gate.active.fetch_sub(1, Ordering::AcqRel) >= 1);
    }
}

impl Gate {
    const RETIRING: usize = 1usize << (usize::BITS - 1);
    const ACTIVE_MASK: usize = Self::RETIRING - 1;

    fn enter(&self) -> bool {
        let mut current = self.state.load(Ordering::Acquire);
        loop {
            if current & Self::RETIRING != 0 {
                return false;
            }
            match self.state.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(observed) => current = observed,
            }
        }
    }

    fn leave(&self) { self.state.fetch_sub(1, Ordering::AcqRel); }

    fn try_retire(&self) -> bool {
        self.state
            .compare_exchange(0, Self::RETIRING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

#[test]
fn byte_and_item_reservations_are_linearizable_and_release_exactly() {
    loom::model(|| {
        let budget = Arc::new(Budget {
            byte_capacity: 3,
            item_capacity: 2,
            bytes: AtomicUsize::new(0),
            items: AtomicUsize::new(0),
        });
        let handles = [2, 2].map(|bytes| {
            let budget = budget.clone();
            thread::spawn(move || {
                if let Some(reservation) = Budget::try_reserve(&budget, bytes) {
                    assert!(budget.bytes.load(Ordering::SeqCst) <= budget.byte_capacity);
                    assert!(budget.items.load(Ordering::SeqCst) <= budget.item_capacity);
                    thread::yield_now();
                    drop(reservation);
                }
            })
        });
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(budget.bytes.load(Ordering::SeqCst), 0);
        assert_eq!(budget.items.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn queue_retirement_cannot_abort_acknowledged_or_handling_work() {
    loom::model(|| {
        let gate = Arc::new(Gate {
            state: AtomicUsize::new(0),
        });
        let resident = Arc::new(AtomicUsize::new(0));
        let acknowledged = Arc::new(AtomicBool::new(false));
        let completed = Arc::new(AtomicBool::new(false));
        let retired = Arc::new(AtomicBool::new(false));

        let sender = {
            let gate = gate.clone();
            let resident = resident.clone();
            let acknowledged = acknowledged.clone();
            let completed = completed.clone();
            let retired = retired.clone();
            thread::spawn(move || {
                if gate.enter() {
                    resident.fetch_add(1, Ordering::SeqCst);
                    assert!(!retired.load(Ordering::SeqCst));
                    acknowledged.store(true, Ordering::SeqCst);
                    thread::yield_now();
                    assert!(!retired.load(Ordering::SeqCst));
                    completed.store(true, Ordering::SeqCst);
                    resident.fetch_sub(1, Ordering::SeqCst);
                    gate.leave();
                }
            })
        };
        let cleanup = {
            let gate = gate.clone();
            let retired = retired.clone();
            thread::spawn(move || {
                if gate.try_retire() {
                    retired.store(true, Ordering::SeqCst);
                }
            })
        };
        sender.join().unwrap();
        cleanup.join().unwrap();
        if acknowledged.load(Ordering::SeqCst) {
            assert!(completed.load(Ordering::SeqCst));
        }
        assert_eq!(resident.load(Ordering::SeqCst), 0);
        assert_eq!(gate.state.load(Ordering::SeqCst) & Gate::ACTIVE_MASK, 0);
    });
}

#[test]
fn initialization_guard_prevents_orphaned_peer_slots() {
    loom::model(|| {
        struct Slot {
            present: bool,
            in_progress: usize,
        }

        let slot = Arc::new(Mutex::new(Slot {
            present: false,
            in_progress: 0,
        }));
        let orphaned = Arc::new(AtomicBool::new(false));
        let initializer = {
            let slot = slot.clone();
            let orphaned = orphaned.clone();
            thread::spawn(move || {
                {
                    let mut state = slot.lock().unwrap();
                    state.present = true;
                    state.in_progress += 1;
                }
                thread::yield_now();
                {
                    let state = slot.lock().unwrap();
                    if !state.present {
                        orphaned.store(true, Ordering::SeqCst);
                    }
                }
                slot.lock().unwrap().in_progress -= 1;
            })
        };
        let cleanup = {
            let slot = slot.clone();
            thread::spawn(move || {
                let mut state = slot.lock().unwrap();
                if state.present && state.in_progress == 0 {
                    state.present = false;
                }
            })
        };
        initializer.join().unwrap();
        cleanup.join().unwrap();
        assert!(!orphaned.load(Ordering::SeqCst));
    });
}

#[test]
fn fanout_shares_one_payload_reservation_until_the_last_owner_releases() {
    loom::model(|| {
        let budget = Arc::new(Budget {
            byte_capacity: 4,
            item_capacity: 1,
            bytes: AtomicUsize::new(0),
            items: AtomicUsize::new(0),
        });
        let payload = Arc::new(Budget::try_reserve(&budget, 4).unwrap());
        let first = {
            let payload = payload.clone();
            thread::spawn(move || drop(payload))
        };
        let second = {
            let payload = payload.clone();
            thread::spawn(move || drop(payload))
        };
        first.join().unwrap();
        second.join().unwrap();
        assert_eq!(budget.bytes.load(Ordering::SeqCst), 4);
        assert_eq!(budget.items.load(Ordering::SeqCst), 1);
        drop(payload);
        assert_eq!(budget.bytes.load(Ordering::SeqCst), 0);
        assert_eq!(budget.items.load(Ordering::SeqCst), 0);
    });
}

#[test]
fn parallel_validation_uses_each_requests_immutable_context() {
    loom::model(|| {
        let decisions = Arc::new(Mutex::new(Vec::new()));
        let handles = [true, false].map(|header_matches_expected| {
            let decisions = decisions.clone();
            thread::spawn(move || {
                thread::yield_now();
                decisions.lock().unwrap().push(header_matches_expected);
            })
        });
        for handle in handles {
            handle.join().unwrap();
        }
        let mut observed = decisions.lock().unwrap().clone();
        observed.sort();
        assert_eq!(observed, vec![false, true]);
    });
}

#[test]
fn transport_success_is_observable_only_after_remote_acknowledgement() {
    loom::model(|| {
        let (delivery_tx, delivery_rx) = mpsc::channel();
        let (completion_tx, completion_rx) = mpsc::channel();
        let remotely_acknowledged = Arc::new(AtomicBool::new(false));
        let worker_acknowledged = remotely_acknowledged.clone();
        let worker = thread::spawn(move || {
            delivery_rx.recv().unwrap();
            worker_acknowledged.store(true, Ordering::Release);
            completion_tx.send(()).unwrap();
        });
        delivery_tx.send(()).unwrap();
        completion_rx.recv().unwrap();
        assert!(remotely_acknowledged.load(Ordering::Acquire));
        worker.join().unwrap();
    });
}

#[test]
fn ingress_item_capacity_accepts_the_supported_pre_settings_burst() {
    loom::model(|| {
        let budget = Arc::new(Budget {
            byte_capacity: 2,
            item_capacity: 2,
            bytes: AtomicUsize::new(0),
            items: AtomicUsize::new(0),
        });
        let (reservation_tx, reservation_rx) = mpsc::channel();
        let handles = [(), ()].map(|()| {
            let budget = budget.clone();
            let reservation_tx = reservation_tx.clone();
            thread::spawn(move || {
                reservation_tx
                    .send(Budget::try_reserve(&budget, 1).unwrap())
                    .unwrap();
            })
        });
        drop(reservation_tx);
        let first = reservation_rx.recv().unwrap();
        let second = reservation_rx.recv().unwrap();
        assert_eq!(budget.items.load(Ordering::Acquire), 2);
        assert_eq!(budget.bytes.load(Ordering::Acquire), 2);
        drop(first);
        drop(second);
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(budget.items.load(Ordering::Acquire), 0);
        assert_eq!(budget.bytes.load(Ordering::Acquire), 0);
    });
}

#[test]
fn service_parallelism_bounds_pre_reservation_decoder_bytes() {
    loom::model(|| {
        let gate = Arc::new(DecoderGate {
            active: AtomicUsize::new(0),
            bytes: AtomicUsize::new(0),
            handler_limit: 1,
            max_message_bytes: 2,
        });
        let handles = [2, 2].map(|bytes| {
            let gate = gate.clone();
            thread::spawn(move || {
                if let Some(permit) = DecoderGate::try_enter(&gate, bytes) {
                    thread::yield_now();
                    assert!(gate.bytes.load(Ordering::Acquire) <= 2);
                    drop(permit);
                }
            })
        });
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(gate.bytes.load(Ordering::Acquire), 0);
        assert_eq!(gate.active.load(Ordering::Acquire), 0);
    });
}
