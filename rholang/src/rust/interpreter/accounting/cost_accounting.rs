// See rholang/src/main/scala/coop/rchain/rholang/interpreter/accounting/CostAccounting.scala

use super::costs::Cost;
use super::{CostManager, _cost};

pub struct CostAccounting;

impl CostAccounting {
    fn empty() -> Cost {
        Cost {
            value: 0,
            operation: "init".into(),
        }
    }

    pub fn empty_cost() -> _cost { CostManager::new(Self::empty(), 1) }
}
