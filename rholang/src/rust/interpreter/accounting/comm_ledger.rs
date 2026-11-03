use std::collections::{BTreeMap, BTreeSet};
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
    surfaces: BTreeMap<Option<Surface>, u64>,
    work: BTreeMap<Option<Surface>, i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Surface {
    pub owner: Vec<u8>,
    pub envelope: Vec<u8>,
    pub phlo_price: u64,
}

impl Surface {
    pub fn of(record: &super::rev_stack::seal::SealRecord) -> Self {
        Surface {
            owner: record.owner.canonical_bytes(),
            envelope: record.origin.envelope.clone(),
            phlo_price: record.origin.phlo_price,
        }
    }
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

    pub fn on_comm_between(&self, surfaces: BTreeSet<Option<Surface>>) {
        self.on_comm();
        let mut s = self.lock();
        for surface in surfaces {
            *s.surfaces.entry(surface).or_insert(0) += 1;
        }
    }

    pub fn take_surfaces(&self) -> BTreeMap<Option<Surface>, u64> {
        std::mem::take(&mut self.lock().surfaces)
    }

    pub fn on_work(&self, surface: Option<Surface>, phlo: i64) {
        let mut s = self.lock();
        let w = s.work.entry(surface).or_insert(0);
        *w = w.saturating_add(phlo);
    }

    pub fn take_work(&self) -> BTreeMap<Option<Surface>, u64> {
        std::mem::take(&mut self.lock().work)
            .into_iter()
            .filter_map(|(surface, phlo)| u64::try_from(phlo).ok().filter(|p| *p > 0).map(|p| (surface, p)))
            .collect()
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
        s.surfaces.clear();
        s.work.clear();
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

#[cfg(test)]
mod surface_tests {
    use super::*;

    fn surface(owner: u8) -> Option<Surface> {
        Some(Surface {
            owner: vec![owner],
            envelope: vec![9],
            phlo_price: 2,
        })
    }

    #[test]
    fn each_distinct_surface_of_a_comm_is_counted_once() {
        let ledger = CommLedger::default();
        ledger.on_comm_between(BTreeSet::from([surface(1), surface(2)]));
        ledger.on_comm_between(BTreeSet::from([surface(1)]));
        ledger.on_comm_between(BTreeSet::from([None, surface(2)]));
        assert_eq!(ledger.take().comms, 3);
        assert_eq!(
            ledger.take_surfaces(),
            BTreeMap::from([(None, 1), (surface(1), 2), (surface(2), 2)])
        );
        ledger.on_comm_between(BTreeSet::from([surface(1)]));
        ledger.reset();
        assert!(ledger.take_surfaces().is_empty());
    }
}
