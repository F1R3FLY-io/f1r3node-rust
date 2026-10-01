use std::mem::size_of;

use super::*;

impl<'a> CheckedPhloExecution<'a> {
    pub fn with_retained_acquisitions(
        mut self,
        retained: &'a [PhloResourceAmount<'a>],
        host: &HostWorkBudget,
    ) -> Result<Self, PhloExecutionError> {
        let parts = self.witness.parts();
        let mut remaining = self.limits.resource_entries;
        for count in parts.iter().map(|part| part.len()).chain([retained.len()]) {
            remaining = remaining
                .checked_sub(count)
                .ok_or(PhloExecutionError::TooManyResourceEntries)?;
        }
        let mut work = WorkBudget {
            remaining_nodes: self.limits.authority_nodes,
            remaining_bytes: self.limits.key_bytes,
        };
        for part in parts {
            for entry in part.iter() {
                resource_key_with_host_work(entry.resource, &mut work, Some(host))?;
            }
        }
        reserve_key_work(
            Some(host),
            HostWorkDimension::SearchStateBytes,
            retained
                .len()
                .checked_mul(size_of::<ResourceKey<'_>>() + size_of::<u64>())
                .ok_or(PhloExecutionError::ArithmeticOverflow)?,
        )?;
        let mut counts = ResourceCounts::new();
        for entry in retained {
            if entry.quantity == 0 {
                return Err(PhloExecutionError::ZeroQuantity);
            }
            let before = work.remaining_bytes;
            let key = resource_key_with_host_work(entry.resource, &mut work, Some(host))?;
            let comparison = (before - work.remaining_bytes)
                .checked_add(key.authority.len())
                .and_then(|n| n.checked_add(1))
                .and_then(|n| {
                    n.checked_mul(1 + retained.len().checked_ilog2().unwrap_or(0) as usize)
                })
                .ok_or(PhloExecutionError::ArithmeticOverflow)?;
            reserve_key_work(
                Some(host),
                HostWorkDimension::VerificationOperations,
                comparison,
            )?;
            let count = counts.entry(key).or_insert(0u64);
            *count = count
                .checked_add(entry.quantity)
                .ok_or(PhloExecutionError::ArithmeticOverflow)?;
        }
        let value = weighted_usage(&counts, self.controls.schedule().weights)?
            .checked_mul(self.controls.schedule().actual_price)
            .ok_or(PhloExecutionError::ArithmeticOverflow)?;
        self.acquisition_charge
            .checked_add(value)
            .ok_or(PhloExecutionError::ArithmeticOverflow)?;
        self.retained_acquisitions = retained;
        self.retained_acquisition_value = value;
        Ok(self)
    }
}
