use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use costs::Cost;

use super::errors::InterpreterError;

pub mod comm_ledger;
pub mod cost_accounting;
pub mod costs;
pub mod has_cost;
pub mod noop;
pub mod rev_stack;
pub mod storage_rule;

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

        // Use saturating arithmetic to prevent overflow when credits (negative
        // charges) are applied to balances near i64::MAX (e.g., genesis deploys).
        current_cost.value = current_cost.value.saturating_sub(amount.value);
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
        *current_cost = new_value;
    }

    pub fn get_log(&self) -> Vec<Cost> { self.log.lock().unwrap().iter().cloned().collect() }

    pub fn clear_log(&self) { self.log.lock().unwrap().clear(); }

    pub fn record_comm(&self) { self.comms.on_comm(); }

    pub fn comm_ledger(&self) -> &comm_ledger::CommLedger { &self.comms }

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
