use models::rust::phlo_controls::{PhloControlsLimits, PhloControlsV1, PhloControlsWireError};
use models::rust::phlo_wire::PhloWireError;

use super::{
    PhloSchedule, PhloScheduleBinding, PhloSchedulePolicy, PhloSchedulePolicyMismatch,
    SignedPhloControls,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloControlsBinding<'a> {
    descriptor: &'a PhloControlsV1<'a>,
    schedules: Vec<PhloScheduleBinding<'a>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloControlsView<'a> {
    descriptor: &'a PhloControlsV1<'a>,
    schedules: Vec<PhloSchedule<'a>>,
}

impl<'a> PhloControlsBinding<'a> {
    pub fn new(
        descriptor: &'a PhloControlsV1<'a>,
        limits: PhloControlsLimits,
    ) -> Result<Self, PhloControlsWireError> {
        descriptor.encode(limits)?;
        let mut schedules = Vec::new();
        schedules
            .try_reserve_exact(descriptor.permitted_schedules.len())
            .map_err(|_| PhloWireError::AllocationFailed)?;
        for schedule in &descriptor.permitted_schedules {
            schedules.push(PhloScheduleBinding::new(
                schedule,
                limits.schedule(schedule.classes.len()),
            )?);
        }
        Ok(Self {
            descriptor,
            schedules,
        })
    }

    pub fn descriptor(&self) -> &'a PhloControlsV1<'a> { self.descriptor }

    pub fn view(&self) -> Result<PhloControlsView<'_>, PhloWireError> {
        let mut schedules = Vec::new();
        schedules
            .try_reserve_exact(self.schedules.len())
            .map_err(|_| PhloWireError::AllocationFailed)?;
        schedules.extend(self.schedules.iter().map(PhloScheduleBinding::schedule));
        Ok(PhloControlsView {
            descriptor: self.descriptor,
            schedules,
        })
    }
}

impl PhloControlsView<'_> {
    pub fn check_schedule_policy(
        &self,
        commitment: &[u8; 32],
        required: PhloSchedulePolicy<'_>,
    ) -> Result<(), PhloSchedulePolicyMismatch> {
        let index = self
            .schedules
            .iter()
            .position(|schedule| &schedule.commitment == commitment)
            .ok_or(PhloSchedulePolicyMismatch)?;
        let descriptor = &self.descriptor.permitted_schedules[index];
        let policy = PhloSchedulePolicy {
            environment: self.schedules[index].environment,
            classes: &descriptor.classes,
            compatibility_rule: descriptor.compatibility_rule,
        };
        if policy != required {
            return Err(PhloSchedulePolicyMismatch);
        }
        Ok(())
    }

    pub fn terms(&self) -> SignedPhloControls<'_> {
        SignedPhloControls {
            limit: self.descriptor.limit,
            price_ceiling: self.descriptor.price_ceiling,
            required_owner_ceilings: &self.descriptor.required_owner_ceilings,
            permitted_schedules: &self.schedules,
        }
    }
}

#[cfg(test)]
mod tests;
