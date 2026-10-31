use std::num::NonZeroU64;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommTally {
    pub comms: u64,
    pub batches: Vec<u64>,
}

#[derive(Debug, Default)]
struct LedgerState {
    boxcar: Option<NonZeroU64>,
    pending: u64,
    tally: CommTally,
}

#[derive(Clone, Debug, Default)]
pub struct CommLedger {
    state: Arc<Mutex<LedgerState>>,
}

impl CommLedger {
    pub fn set_boxcar(&self, boxcar: Option<NonZeroU64>) {
        let mut s = self.lock();
        s.boxcar = boxcar;
    }

    pub fn on_comm(&self) {
        let mut s = self.lock();
        s.tally.comms = s.tally.comms.saturating_add(1);
        s.pending += 1;
        if s.boxcar.is_some_and(|b| s.pending >= b.get()) {
            let full = s.pending;
            s.tally.batches.push(full);
            s.pending = 0;
        }
    }

    pub fn take(&self) -> CommTally {
        let mut s = self.lock();
        if s.pending > 0 {
            let rest = s.pending;
            s.tally.batches.push(rest);
            s.pending = 0;
        }
        std::mem::take(&mut s.tally)
    }

    pub fn reset(&self) {
        let mut s = self.lock();
        s.pending = 0;
        s.tally = CommTally::default();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LedgerState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ledger_counts_comms_and_batches_them_by_box_car() {
        let ledger = CommLedger::default();
        for _ in 0..7 {
            ledger.on_comm();
        }
        assert_eq!(
            ledger.take(),
            CommTally {
                comms: 7,
                batches: vec![7]
            }
        );
        assert_eq!(ledger.take(), CommTally::default());

        ledger.set_boxcar(NonZeroU64::new(3));
        for _ in 0..7 {
            ledger.on_comm();
        }
        assert_eq!(
            ledger.take(),
            CommTally {
                comms: 7,
                batches: vec![3, 3, 1]
            }
        );

        ledger.on_comm();
        ledger.reset();
        assert_eq!(ledger.take(), CommTally::default());
    }
}
