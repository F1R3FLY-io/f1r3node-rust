use std::cmp::Ordering;

use models::rust::host_work::HostWorkDimension;
use models::rust::phlo_obligation::PhloObligationKeyLimits;

use super::partition::{normalize_partition, PhloPartitionError};
use super::{
    push_key_entry, resource_key_with_host_work, CountedPhloExecutionWitness, PhloExecutionError,
    PhloExecutionLimits, PhloResourceAmount, ResourceEntries, WorkBudget,
};
use crate::rust::interpreter::accounting::monetary_allocation::{reserve_work, FundingSearchError};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Copy, Debug)]
pub struct PhloDischargeLimits {
    pub execution: PhloExecutionLimits,
    pub key: PhloObligationKeyLimits,
    pub aggregate_key_bytes: usize,
}

#[derive(Debug)]
pub struct PreparedPhloDischarge<'a> {
    available: &'a [PhloResourceAmount<'a>],
    required: &'a [PhloResourceAmount<'a>],
    used: Vec<PhloResourceAmount<'a>>,
    unused: Vec<PhloResourceAmount<'a>>,
    fresh: Vec<PhloResourceAmount<'a>>,
}

impl PreparedPhloDischarge<'_> {
    pub fn witness(&self) -> CountedPhloExecutionWitness<'_> {
        CountedPhloExecutionWitness {
            available: self.available,
            required: self.required,
            used: &self.used,
            unused: &self.unused,
            fresh: &self.fresh,
        }
    }
}

struct OutputBudget<'a> {
    remaining_entries: usize,
    keys: WorkBudget,
    host: &'a HostWorkBudget,
}

impl OutputBudget<'_> {
    fn emit<'a>(
        &mut self,
        target: &mut Vec<PhloResourceAmount<'a>>,
        reserved: &mut usize,
        mut entry: PhloResourceAmount<'a>,
        quantity: u64,
    ) -> Result<(), PhloPartitionError> {
        if quantity == 0 {
            return Ok(());
        }
        self.remaining_entries = self
            .remaining_entries
            .checked_sub(1)
            .ok_or(PhloExecutionError::TooManyResourceEntries)?;
        resource_key_with_host_work(entry.resource, &mut self.keys, Some(self.host))?;
        entry.quantity = quantity;
        push_key_entry(target, reserved, entry, Some(self.host))?;
        Ok(())
    }
}

pub fn prepare_counted_phlo_discharge<'a>(
    available: &'a [PhloResourceAmount<'a>],
    required: &'a [PhloResourceAmount<'a>],
    limits: PhloDischargeLimits,
    budget: &HostWorkBudget,
) -> Result<PreparedPhloDischarge<'a>, PhloPartitionError> {
    let remaining_entries = limits
        .execution
        .resource_entries
        .checked_sub(available.len())
        .and_then(|left| left.checked_sub(required.len()))
        .ok_or(PhloExecutionError::TooManyResourceEntries)?;
    let mut keys_work = WorkBudget {
        remaining_nodes: limits.execution.authority_nodes,
        remaining_bytes: limits.execution.key_bytes,
    };
    let mut remaining_bytes = limits.aggregate_key_bytes;
    let supply = normalize_partition(
        ResourceEntries {
            occurrences: &[],
            counted: available,
        },
        limits.key,
        &mut remaining_bytes,
        &mut keys_work,
        budget,
    )?;
    let demand = normalize_partition(
        ResourceEntries {
            occurrences: &[],
            counted: required,
        },
        limits.key,
        &mut remaining_bytes,
        &mut keys_work,
        budget,
    )?;
    let mut output = PreparedPhloDischarge {
        available,
        required,
        used: Vec::new(),
        unused: Vec::new(),
        fresh: Vec::new(),
    };
    let mut output_budget = OutputBudget {
        remaining_entries,
        keys: keys_work,
        host: budget,
    };
    let (mut used_reserved, mut unused_reserved, mut fresh_reserved) = (0, 0, 0);
    let (mut left, mut right) = (0, 0);
    while left < supply.order.len() || right < demand.order.len() {
        reserve_work(budget, HostWorkDimension::VerificationOperations, 1)?;
        let a = supply.order.get(left).copied();
        let r = demand.order.get(right).copied();
        let order = match (a, r) {
            (Some(a), Some(r)) => {
                let work = supply.keys[a]
                    .len()
                    .min(demand.keys[r].len())
                    .checked_add(1)
                    .ok_or(FundingSearchError::Overflow)?;
                reserve_work(budget, HostWorkDimension::VerificationOperations, work)?;
                supply.keys[a].cmp(&demand.keys[r])
            }
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => break,
        };
        match order {
            Ordering::Less => {
                let a = a.expect("supply entry precedes demand");
                output_budget.emit(
                    &mut output.unused,
                    &mut unused_reserved,
                    available[a],
                    supply.quantities[a],
                )?;
                left += 1;
            }
            Ordering::Greater => {
                let r = r.expect("demand entry precedes supply");
                output_budget.emit(
                    &mut output.fresh,
                    &mut fresh_reserved,
                    required[r],
                    demand.quantities[r],
                )?;
                right += 1;
            }
            Ordering::Equal => {
                let a = a.expect("matching supply entry");
                let r = r.expect("matching demand entry");
                let used = supply.quantities[a].min(demand.quantities[r]);
                output_budget.emit(&mut output.used, &mut used_reserved, available[a], used)?;
                output_budget.emit(
                    &mut output.unused,
                    &mut unused_reserved,
                    available[a],
                    supply.quantities[a] - used,
                )?;
                output_budget.emit(
                    &mut output.fresh,
                    &mut fresh_reserved,
                    required[r],
                    demand.quantities[r] - used,
                )?;
                left += 1;
                right += 1;
            }
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
