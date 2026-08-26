use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug)]
pub struct ActivityGate {
    state: AtomicUsize,
}

#[derive(Debug)]
pub struct ActivityGuard {
    gate: Arc<ActivityGate>,
}

impl ActivityGate {
    const RETIRING: usize = 1usize << (usize::BITS - 1);
    const ACTIVE_MASK: usize = Self::RETIRING - 1;

    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: AtomicUsize::new(0),
        })
    }

    pub fn try_enter(self: &Arc<Self>) -> Option<ActivityGuard> {
        let mut current = self.state.load(Ordering::Acquire);
        loop {
            if current & Self::RETIRING != 0 || current & Self::ACTIVE_MASK == Self::ACTIVE_MASK {
                return None;
            }
            match self.state.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(ActivityGuard { gate: self.clone() }),
                Err(observed) => current = observed,
            }
        }
    }

    pub fn try_retire_if(&self, additional_idle: impl FnOnce() -> bool) -> bool {
        if self
            .state
            .compare_exchange(0, Self::RETIRING, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        let idle = additional_idle();
        if !idle {
            self.state.store(0, Ordering::Release);
        }
        idle
    }

    pub fn is_accepting(&self) -> bool { self.state.load(Ordering::Acquire) & Self::RETIRING == 0 }

    pub fn active(&self) -> usize { self.state.load(Ordering::Acquire) & Self::ACTIVE_MASK }
}

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        let previous = self.gate.state.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous >= 1);
        debug_assert_eq!(previous & ActivityGate::RETIRING, 0);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::ActivityGate;

    #[test]
    fn active_work_prevents_retirement_and_reopens_admission() {
        let gate = ActivityGate::new();
        let guard = gate.try_enter().unwrap();
        assert!(!gate.try_retire_if(|| true));
        assert!(gate.is_accepting());
        assert_eq!(gate.active(), 1);
        drop(guard);
        assert!(gate.try_retire_if(|| true));
        assert!(!gate.is_accepting());
    }

    #[test]
    fn external_residency_prevents_retirement() {
        let gate = ActivityGate::new();
        let resident = AtomicBool::new(true);
        assert!(!gate.try_retire_if(|| !resident.load(Ordering::SeqCst)));
        assert!(gate.is_accepting());
        resident.store(false, Ordering::SeqCst);
        assert!(gate.try_retire_if(|| !resident.load(Ordering::SeqCst)));
    }

    #[test]
    fn retired_gate_rejects_new_work_without_leaking_activity() {
        let gate = ActivityGate::new();
        assert!(gate.try_retire_if(|| true));
        assert!(gate.try_enter().is_none());
        assert_eq!(gate.active(), 0);
    }
}
