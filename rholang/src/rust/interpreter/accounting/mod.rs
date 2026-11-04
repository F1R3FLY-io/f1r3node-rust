use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use costs::Cost;

use super::errors::InterpreterError;

pub mod comm_ledger;
pub mod cost_accounting;
pub mod costs;
pub mod delta_sigma;
pub mod has_cost;
pub mod membrane;
pub mod noop;
pub mod rev_stack;
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

        if let Some(record) = rev_stack::seal::current() {
            if let Some(meter) = rev_stack::seal::meter() {
                if amount.value <= 0 && meter.gates(&record) {
                    drop(current_cost);
                    self.comms
                        .on_work(Some(comm_ledger::Surface::of(&record)), amount.value);
                    self.push_log(amount);
                    return Ok(());
                }
                match meter.admit_charge(&record, amount.value) {
                    membrane::Admission::Refused => return Err(InterpreterError::MembraneStopped),
                    membrane::Admission::Admitted => {
                        drop(current_cost);
                        self.comms
                            .on_work(Some(comm_ledger::Surface::of(&record)), amount.value);
                        self.push_log(amount);
                        return Ok(());
                    }
                    membrane::Admission::Ungated => {}
                }
            }
        }

        // Use saturating arithmetic to prevent overflow when credits (negative
        // charges) are applied to balances near i64::MAX (e.g., genesis deploys).
        current_cost.value = current_cost.value.saturating_sub(amount.value);
        self.comms.on_work(
            rev_stack::seal::current().map(|record| comm_ledger::Surface::of(&record)),
            amount.value,
        );
        self.push_log(amount);

        if current_cost.value < 0 {
            return Err(InterpreterError::OutOfPhlogistonsError);
        }

        Ok(())
    }

    pub fn charge_reserving(&self, amount: Cost, reserve: i64) -> Result<(), InterpreterError> {
        if let (Some(record), Some(meter)) = (rev_stack::seal::current(), rev_stack::seal::meter())
        {
            if amount.value > 0 && meter.gates(&record) {
                return match meter.admit_charge(&record, amount.value.saturating_add(reserve)) {
                    membrane::Admission::Refused => Err(InterpreterError::MembraneStopped),
                    _ => {
                        self.comms
                            .on_work(Some(comm_ledger::Surface::of(&record)), amount.value);
                        self.push_log(amount);
                        Ok(())
                    }
                };
            }
        }
        self.charge(amount)
    }

    fn push_log(&self, amount: Cost) {
        if self.max_log_entries > 0 {
            let mut log = self.log.lock().unwrap();
            if log.len() >= self.max_log_entries {
                let _ = log.pop_front();
            }
            log.push_back(amount);
        }
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
    fn ungated_work_uses_up_the_clients_limit_whatever_its_origin() {
        let cost = CostManager::new(Cost::create(0, "init".to_string()));
        cost.set(Cost::create(100, "limit".to_string()));
        charge(&cost, Some(record(1)), 60).unwrap();
        charge(&cost, Some(record(2)), 30).unwrap();
        assert_eq!(cost.get().value, 10);
        assert!(matches!(
            charge(&cost, Some(record(2)), 20),
            Err(InterpreterError::OutOfPhlogistonsError)
        ));
    }
}
