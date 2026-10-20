use super::*;

impl<'a> CheckedPhloExecution<'a> {
    pub fn with_retained_acquisitions(
        mut self,
        retained: &'a [PhloResourceAmount<'a>],
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
                resource_key(entry.resource, &mut work)?;
            }
        }
        let mut counts = ResourceCounts::new();
        for entry in retained {
            if entry.quantity == 0 {
                return Err(PhloExecutionError::ZeroQuantity);
            }
            let key = resource_key(entry.resource, &mut work)?;
            let count = counts.entry(key).or_insert(0u64);
            *count = count
                .checked_add(entry.quantity)
                .ok_or(PhloExecutionError::ArithmeticOverflow)?;
        }
        let usage = weighted_usage(&counts, self.controls.schedule().weights)?;
        let value = usage
            .checked_mul(self.controls.schedule().actual_price)
            .ok_or(PhloExecutionError::ArithmeticOverflow)?;
        self.acquisition_charge
            .checked_add(value)
            .ok_or(PhloExecutionError::ArithmeticOverflow)?;
        self.retained_acquisitions = retained;
        self.retained_usage = usage;
        self.retained_acquisition_value = value;
        Ok(self)
    }
}
