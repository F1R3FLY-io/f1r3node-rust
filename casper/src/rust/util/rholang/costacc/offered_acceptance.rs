//! DR-102: role-local acceptance budgets for offered deploys.
//!
//! The replay budget of an offered deploy measures only
//! `certify_offered_draft`, which the producer and every validator run on the
//! same inputs, so every node reaches the same replay verdict. The work that
//! one role does after the certified replay to reach its own decision is
//! charged to an acceptance budget of that role: the producer's publication
//! comparison, and a validator's system continuation, mergeable result vector
//! and mergeable pre-state read.

use std::collections::HashMap;

use models::rust::block_hash::BlockHash;
use models::rust::cost_protocol_limits::offered_funded_v6_host_work_limits;
use models::rust::host_work::HostWorkLimits;
use models::rust::validator::Validator;
use rholang::rust::interpreter::host_work::HostWorkBudget;
use rholang::rust::interpreter::system_processes::BlockData;

use crate::rust::errors::CasperError;

/// DR-102: the limits of an acceptance budget. They are a protocol constant,
/// never node configuration, and they must stay at least the execution limits
/// in every dimension. A validator's acceptance work equals work that the
/// producer's execution budget already admitted, so with these limits an
/// honestly published block never exhausts a validator's acceptance budget.
pub fn offered_acceptance_host_work_limits() -> HostWorkLimits {
    offered_funded_v6_host_work_limits()
}

/// DR-102: the budget for the work that one role does after the certified
/// replay of an offered deploy. A producer creates one per candidate attempt,
/// a validator one per replay call.
pub(crate) struct OfferedAcceptanceBudget {
    meter: HostWorkBudget,
}

impl OfferedAcceptanceBudget {
    pub(crate) fn new() -> Self {
        Self {
            meter: HostWorkBudget::new(offered_acceptance_host_work_limits()),
        }
    }

    pub(crate) fn meter(&self) -> &HostWorkBudget { &self.meter }
}

/// DR-102: the canonical bytes that certify copies from the block context:
/// the sender key and every slashed block hash with its validator. The count
/// uses no `size_of`, so platform layout cannot change a consensus verdict.
pub(crate) fn offered_replay_context_copy_bytes(
    block_data: &BlockData,
    invalid_blocks: &HashMap<BlockHash, Validator>,
) -> Result<usize, CasperError> {
    invalid_blocks
        .iter()
        .try_fold(block_data.sender.bytes.len(), |bytes, (hash, validator)| {
            bytes.checked_add(hash.len())?.checked_add(validator.len())
        })
        .ok_or_else(|| {
            CasperError::RuntimeError("offered replay context copy overflows".to_string())
        })
}

#[cfg(any(test, feature = "test-utils"))]
pub use recorder::{OfferedBudgetRecorder, OfferedBudgetUsage, OfferedUsageKind};

#[cfg(any(test, feature = "test-utils"))]
mod recorder {
    use std::sync::{Arc, Mutex};

    use models::rust::host_work::{HostWorkDimension, HostWorkUsage, HostWorkUsages};

    /// The budget that a usage record measures.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum OfferedUsageKind {
        ProducerSelfReplay,
        ProducerAcceptance,
        ProducerMergeableExecution,
        ValidatorReplay,
        ValidatorAcceptance,
    }

    /// The final usage of one budget in one completed attempt.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct OfferedBudgetUsage {
        pub kind: OfferedUsageKind,
        pub deploy_id: Vec<u8>,
        pub pre_state_root: Vec<u8>,
        pub attempt: usize,
        pub usages: HostWorkUsages,
    }

    /// Test-only records of offered budget usage, shared by every clone of a
    /// runtime manager.
    #[derive(Clone, Default)]
    pub struct OfferedBudgetRecorder(Arc<Mutex<Vec<OfferedBudgetUsage>>>);

    impl OfferedBudgetRecorder {
        pub fn record(
            &self,
            kind: OfferedUsageKind,
            deploy_id: &[u8],
            pre_state_root: &[u8],
            usages: HostWorkUsages,
        ) {
            let mut records = self.0.lock().expect("offered usage records lock");
            let attempt = records
                .iter()
                .filter(|record| {
                    record.kind == kind
                        && record.deploy_id == deploy_id
                        && record.pre_state_root == pre_state_root
                })
                .count();
            records.push(OfferedBudgetUsage {
                kind,
                deploy_id: deploy_id.to_vec(),
                pre_state_root: pre_state_root.to_vec(),
                attempt,
                usages,
            });
        }

        pub fn records(&self) -> Vec<OfferedBudgetUsage> {
            self.0.lock().expect("offered usage records lock").clone()
        }
    }

    /// The usage that a budget added between two snapshots.
    pub fn usage_delta(before: HostWorkUsages, after: HostWorkUsages) -> HostWorkUsages {
        HostWorkUsages::new(HostWorkDimension::ALL.map(|dimension| {
            HostWorkUsage::new(
                after
                    .get(dimension)
                    .get()
                    .checked_sub(before.get(dimension).get())
                    .expect("a budget's usage never decreases"),
            )
        }))
    }
}

#[cfg(any(test, feature = "test-utils"))]
pub use recorder::usage_delta;

#[cfg(test)]
mod tests;
