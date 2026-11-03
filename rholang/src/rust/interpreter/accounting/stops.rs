use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use super::comm_ledger::Surface;

pub type Membrane = Option<Surface>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub membrane: Membrane,
    pub step: Vec<u8>,
    pub drawn: u64,
    pub cost: u64,
}

#[derive(Default)]
struct Observed {
    gross: BTreeMap<Membrane, u64>,
    found: BTreeMap<Vec<u8>, u64>,
    refused: u64,
}

#[derive(Clone, Default)]
pub struct ReplayStops {
    stopped: Arc<BTreeSet<Membrane>>,
    steps: Arc<BTreeSet<Vec<u8>>>,
    logged: Arc<BTreeSet<Vec<u8>>>,
    observed: Arc<Mutex<Observed>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopOutcome {
    Run,
    Refuse,
}

impl ReplayStops {
    pub fn new(refusals: &[Refusal], logged_io: impl IntoIterator<Item = Vec<u8>>) -> Self {
        ReplayStops {
            stopped: Arc::new(refusals.iter().map(|r| r.membrane.clone()).collect()),
            steps: Arc::new(refusals.iter().map(|r| r.step.clone()).collect()),
            logged: Arc::new(logged_io.into_iter().collect()),
            observed: Arc::new(Mutex::new(Observed::default())),
        }
    }

    pub fn is_stopped(&self, membrane: &Membrane) -> bool { self.stopped.contains(membrane) }

    pub fn on_charge(&self, membrane: &Membrane, amount: i64) {
        if amount <= 0 || !self.is_stopped(membrane) {
            return;
        }
        let mut observed = self.lock();
        let gross = observed.gross.entry(membrane.clone()).or_insert(0);
        *gross = gross.saturating_add(amount as u64);
    }

    pub fn io(&self, membrane: &Membrane, step: &[u8], cost: i64) -> StopOutcome {
        if !self.is_stopped(membrane) {
            return StopOutcome::Run;
        }
        if self.steps.contains(step) {
            let mut observed = self.lock();
            observed.found.insert(step.to_vec(), cost.max(0) as u64);
            observed.refused += 1;
            return StopOutcome::Refuse;
        }
        if self.logged.contains(step) {
            return StopOutcome::Run;
        }
        self.lock().refused += 1;
        StopOutcome::Refuse
    }

    pub fn found_step(&self, step: &[u8], cost: u64) {
        if self.steps.contains(step) {
            self.lock().found.insert(step.to_vec(), cost);
        }
    }

    pub fn gross(&self, membrane: &Membrane) -> u64 {
        self.lock().gross.get(membrane).copied().unwrap_or(0)
    }

    pub fn found(&self, step: &[u8]) -> Option<u64> { self.lock().found.get(step).copied() }

    pub fn refused(&self) -> u64 { self.lock().refused }

    fn lock(&self) -> std::sync::MutexGuard<'_, Observed> {
        self.observed
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StopCheck {
    EarlyStop,
    ForgedDrawn,
    UnpayableAdmission,
    MissingStep,
    ForgedCost,
}

pub fn check_stop(
    refusal: &Refusal,
    available: u64,
    replayed_work: u64,
    found_cost: Option<u64>,
) -> Result<(), StopCheck> {
    let cost = found_cost.ok_or(StopCheck::MissingStep)?;
    if cost != refusal.cost {
        return Err(StopCheck::ForgedCost);
    }
    if refusal.drawn > available {
        return Err(StopCheck::UnpayableAdmission);
    }
    if refusal.drawn > replayed_work {
        return Err(StopCheck::ForgedDrawn);
    }
    if available - refusal.drawn >= refusal.cost {
        return Err(StopCheck::EarlyStop);
    }
    Ok(())
}

pub fn check_unstopped(available: u64, work: u64) -> Result<(), StopCheck> {
    if work > available {
        return Err(StopCheck::UnpayableAdmission);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn membrane(owner: u8) -> Membrane {
        Some(Surface {
            owner: vec![owner],
            envelope: vec![9],
            phlo_price: 1,
        })
    }

    fn refusal(drawn: u64, cost: u64) -> Refusal {
        Refusal {
            membrane: membrane(1),
            step: vec![7],
            drawn,
            cost,
        }
    }

    #[test]
    fn an_honest_stop_passes() {
        assert_eq!(check_stop(&refusal(95, 10), 100, 120, Some(10)), Ok(()));
    }

    #[test]
    fn a_stop_at_a_payable_step_is_refused() {
        assert_eq!(
            check_stop(&refusal(90, 10), 100, 120, Some(10)),
            Err(StopCheck::EarlyStop)
        );
    }

    #[test]
    fn a_drawn_amount_above_the_replayed_work_is_refused() {
        assert_eq!(
            check_stop(&refusal(95, 10), 100, 60, Some(10)),
            Err(StopCheck::ForgedDrawn)
        );
    }

    #[test]
    fn a_drawn_amount_below_the_stop_point_is_refused() {
        assert_eq!(
            check_stop(&refusal(40, 10), 100, 120, Some(10)),
            Err(StopCheck::EarlyStop)
        );
    }

    #[test]
    fn an_admission_past_the_balance_is_refused() {
        assert_eq!(
            check_stop(&refusal(101, 10), 100, 200, Some(10)),
            Err(StopCheck::UnpayableAdmission)
        );
        assert_eq!(check_unstopped(100, 101), Err(StopCheck::UnpayableAdmission));
        assert_eq!(check_unstopped(100, 100), Ok(()));
    }

    #[test]
    fn a_refused_step_that_replay_does_not_reach_is_refused() {
        assert_eq!(
            check_stop(&refusal(95, 10), 100, 120, None),
            Err(StopCheck::MissingStep)
        );
    }

    #[test]
    fn a_forged_step_cost_is_refused() {
        assert_eq!(
            check_stop(&refusal(95, 50), 100, 120, Some(10)),
            Err(StopCheck::ForgedCost)
        );
    }

    #[test]
    fn under_a_stopped_membrane_only_logged_io_runs() {
        let stops = ReplayStops::new(&[refusal(95, 10)], vec![vec![1], vec![2]]);
        assert_eq!(stops.io(&membrane(1), &[1], 3), StopOutcome::Run);
        assert_eq!(stops.io(&membrane(1), &[3], 3), StopOutcome::Refuse);
        assert_eq!(stops.io(&membrane(1), &[7], 10), StopOutcome::Refuse);
        assert_eq!(stops.found(&[7]), Some(10));
        assert_eq!(stops.io(&membrane(2), &[3], 3), StopOutcome::Run);
        assert_eq!(stops.refused(), 2);
    }

    #[test]
    fn only_positive_charges_under_a_stopped_membrane_count_as_gross_work() {
        let stops = ReplayStops::new(&[refusal(95, 10)], Vec::<Vec<u8>>::new());
        stops.on_charge(&membrane(1), 40);
        stops.on_charge(&membrane(1), -15);
        stops.on_charge(&membrane(1), 25);
        stops.on_charge(&membrane(2), 50);
        assert_eq!(stops.gross(&membrane(1)), 65);
        assert_eq!(stops.gross(&membrane(2)), 0);
    }
}
