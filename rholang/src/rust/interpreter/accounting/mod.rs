use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use costs::Cost;

use super::errors::InterpreterError;

pub mod comm_ledger;
pub mod cost_accounting;
pub mod costs;
pub mod delta_sigma;
pub mod has_cost;
pub mod noop;
pub mod rev_stack;
pub mod stops;
pub mod storage_rule;
pub mod system_charge;

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/accounting/package.scala
#[allow(non_camel_case_types)]
pub type _cost = CostManager;

#[derive(Clone)]
pub struct CostManager {
    state: Arc<Mutex<Cost>>,
    log: Arc<Mutex<VecDeque<Cost>>>,
    max_log_entries: usize,
    comms: comm_ledger::CommLedger,
    storage: Arc<Mutex<Option<storage_rule::LeftInSpace>>>,
    own_origin: Arc<Mutex<Option<Vec<u8>>>>,
    foreign: Arc<Mutex<(i64, i64)>>,
    stops: Arc<Mutex<Option<stops::ReplayStops>>>,
    system_charge: Arc<Mutex<Option<system_charge::SystemCharge>>>,
}

impl CostManager {
    fn resolve_max_log_entries() -> usize { 1024 }

    pub fn new(initial_value: Cost) -> Self {
        let max_log_entries = Self::resolve_max_log_entries();
        let initial_capacity = if max_log_entries == 0 {
            0
        } else if max_log_entries == usize::MAX {
            1024
        } else {
            max_log_entries.min(1024)
        };

        Self {
            state: Arc::new(Mutex::new(initial_value)),
            log: Arc::new(Mutex::new(VecDeque::with_capacity(initial_capacity))),
            max_log_entries,
            comms: comm_ledger::CommLedger::default(),
            storage: Arc::new(Mutex::new(None)),
            own_origin: Arc::new(Mutex::new(None)),
            foreign: Arc::new(Mutex::new((0, 0))),
            stops: Arc::new(Mutex::new(None)),
            system_charge: Arc::new(Mutex::new(None)),
        }
    }

    pub fn charge(&self, amount: Cost) -> Result<(), InterpreterError> {
        let mut current_cost = self
            .state
            .lock()
            .map_err(|_| InterpreterError::SetupError("Failed to lock cost state".to_string()))?;

        if current_cost.value < 0 {
            return Err(InterpreterError::OutOfPhlogistonsError);
        }

        let surface = rev_stack::seal::current().map(|record| comm_ledger::Surface::of(&record));
        let foreign = match (&surface, self.own_origin.lock().unwrap().as_ref()) {
            (Some(s), Some(own)) => &s.envelope != own,
            _ => false,
        };
        // Use saturating arithmetic to prevent overflow when credits (negative
        // charges) are applied to balances near i64::MAX (e.g., genesis deploys).
        self.comms.on_work(surface.clone(), amount.value);
        if let Some(stops) = self.replay_stops() {
            stops.on_charge(&surface, amount.value);
        }
        if foreign {
            let mut budget = self.foreign.lock().unwrap();
            budget.1 = budget.1.saturating_add(amount.value);
            if budget.1 > budget.0 {
                return Err(InterpreterError::OutOfPhlogistonsError);
            }
        } else {
            current_cost.value = current_cost.value.saturating_sub(amount.value);
        }
        if self.max_log_entries > 0 {
            let mut log = self.log.lock().unwrap();
            if log.len() >= self.max_log_entries {
                let _ = log.pop_front();
            }
            log.push_back(amount);
        }

        if current_cost.value < 0 {
            return Err(InterpreterError::OutOfPhlogistonsError);
        }

        Ok(())
    }

    pub fn get(&self) -> Cost {
        let current_cost = self.state.lock().expect("cost state lock");
        current_cost.clone()
    }

    pub fn set(&self, new_value: Cost) {
        let mut current_cost = self.state.lock().expect("cost state lock");
        self.foreign.lock().expect("foreign budget lock").1 = 0;
        *current_cost = new_value;
    }

    pub fn get_log(&self) -> Vec<Cost> { self.log.lock().unwrap().iter().cloned().collect() }

    pub fn clear_log(&self) { self.log.lock().unwrap().clear(); }

    pub fn record_comm(&self) { self.comms.on_comm(); }

    pub fn set_own_origin(&self, envelope: Option<Vec<u8>>, foreign_budget: i64) {
        *self.own_origin.lock().unwrap() = envelope;
        *self.foreign.lock().unwrap() = (foreign_budget, 0);
    }

    pub fn comm_ledger(&self) -> &comm_ledger::CommLedger { &self.comms }

    pub fn set_replay_stops(&self, stops: Option<stops::ReplayStops>) {
        *self.stops.lock().unwrap_or_else(|p| p.into_inner()) = stops;
    }

    pub fn replay_stops(&self) -> Option<stops::ReplayStops> {
        self.stops.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn set_system_charge(&self, charge: Option<system_charge::SystemCharge>) {
        *self.system_charge.lock().unwrap_or_else(|p| p.into_inner()) = charge;
    }

    pub fn system_charge(&self) -> Option<system_charge::SystemCharge> {
        self.system_charge.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn charge_storage_left_in_space(&self) {
        *self.storage.lock().expect("storage rule lock") =
            Some(storage_rule::LeftInSpace::default());
    }

    pub fn storage_left_in_space(
        &self,
    ) -> std::sync::MutexGuard<'_, Option<storage_rule::LeftInSpace>> {
        self.storage.lock().expect("storage rule lock")
    }
}

#[cfg(test)]
mod own_limit_tests {
    use super::costs::Cost;
    use super::rev_stack::seal::{within_now, Origin, SealRecord};
    use super::rev_stack::WalletOwner;
    use super::CostManager;
    use crate::rust::interpreter::errors::InterpreterError;

    fn record(envelope: u8) -> SealRecord {
        SealRecord::deployment(
            WalletOwner::Unforgeable(vec![envelope; 32]),
            Origin {
                envelope: vec![envelope],
                signers: Vec::new(),
                phlo_price: 1,
            },
        )
    }

    fn charge(cost: &CostManager, under: Option<SealRecord>, phlo: i64) -> Result<(), InterpreterError> {
        within_now(under, || cost.charge(Cost::create(phlo, "test".to_string())))
    }

    #[test]
    fn only_the_clients_own_work_uses_up_its_limit_and_foreign_work_has_its_own_bound() {
        let cost = CostManager::new(Cost::create(0, "init".to_string()));
        cost.set_own_origin(Some(vec![1]), 300);
        cost.set(Cost::create(100, "limit".to_string()));
        charge(&cost, Some(record(1)), 60).unwrap();
        charge(&cost, None, 30).unwrap();
        charge(&cost, Some(record(2)), 250).unwrap();
        assert_eq!(cost.get().value, 10, "foreign work does not use up the limit");
        assert!(matches!(
            charge(&cost, Some(record(2)), 60),
            Err(InterpreterError::OutOfPhlogistonsError)
        ));
        assert!(matches!(
            charge(&cost, Some(record(1)), 20),
            Err(InterpreterError::OutOfPhlogistonsError)
        ));

        let legacy = CostManager::new(Cost::create(0, "init".to_string()));
        legacy.set(Cost::create(100, "limit".to_string()));
        charge(&legacy, Some(record(2)), 60).unwrap();
        assert_eq!(legacy.get().value, 40, "with no own origin every charge counts");
    }
}
