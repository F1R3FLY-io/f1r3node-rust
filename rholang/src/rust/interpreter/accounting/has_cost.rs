// See rholang/src/main/scala/coop/rchain/rholang/interpreter/accounting/HasCost.scala

use super::RuntimeBudget;

pub trait HasCost {
    fn cost(&self) -> &RuntimeBudget;
}
