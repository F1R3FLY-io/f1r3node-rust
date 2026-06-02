// See rholang/src/main/scala/coop/rchain/rholang/interpreter/accounting/CostAccounting.scala

use super::costs::Cost;
use super::RuntimeBudget;

pub struct CostAccounting;

impl CostAccounting {
    fn empty() -> Cost {
        Cost {
            value: 0,
            operation: "init".into(),
        }
    }

    pub fn empty_cost() -> RuntimeBudget {
        RuntimeBudget::new(Self::empty())
    }

    pub fn unmetered_cost() -> RuntimeBudget {
        RuntimeBudget::unmetered()
    }
}
