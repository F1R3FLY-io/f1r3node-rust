use thiserror::Error;

use super::phlo_schedule::{PhloScheduleError, PhloScheduleLimits, PhloScheduleV1};
use super::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireError, PhloWireLimits};

pub const PHLO_CONTROLS_V1_DOMAIN: &[u8] = b"f1r3node:phlo-controls:v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloControlsV1<'a> {
    pub limit: u64,
    pub price_ceiling: u64,
    pub required_owner_ceilings: Vec<u64>,
    pub permitted_schedules: Vec<PhloScheduleV1<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloControlsLimits {
    pub wire: PhloWireLimits,
    pub owners: usize,
    pub schedules: usize,
    pub total_classes: usize,
}

impl PhloControlsLimits {
    fn nested(self) -> PhloWireLimits {
        PhloWireLimits {
            total_bytes: self.wire.field_bytes,
            field_bytes: self.wire.field_bytes,
        }
    }

    pub fn schedule(self, remaining_classes: usize) -> PhloScheduleLimits {
        PhloScheduleLimits {
            wire: self.nested(),
            classes: remaining_classes.min(self.total_classes),
        }
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloControlsWireError {
    #[error("unsupported phlo controls format domain")]
    FormatDomain,
    #[error("phlo controls exceed the required-owner entry limit")]
    OwnerLimit,
    #[error("phlo controls exceed the permitted-schedule entry limit")]
    ScheduleLimit,
    #[error("phlo controls exceed the aggregate resource-class limit")]
    ClassLimit,
    #[error("phlo controls field has a noncanonical width")]
    FieldWidth,
    #[error(transparent)]
    Schedule(#[from] PhloScheduleError),
    #[error(transparent)]
    Wire(#[from] PhloWireError),
}

impl<'a> PhloControlsV1<'a> {
    pub fn encode(&self, limits: PhloControlsLimits) -> Result<Vec<u8>, PhloControlsWireError> {
        let owner_count = bounded_count(
            self.required_owner_ceilings.len(),
            limits.owners,
            PhloControlsWireError::OwnerLimit,
        )?;
        let schedule_count = bounded_count(
            self.permitted_schedules.len(),
            limits.schedules,
            PhloControlsWireError::ScheduleLimit,
        )?;
        let mut remaining_classes = limits.total_classes;
        for schedule in &self.permitted_schedules {
            remaining_classes = remaining_classes
                .checked_sub(schedule.classes.len())
                .ok_or(PhloControlsWireError::ClassLimit)?;
        }
        let mut owners = PhloWireEncoder::new(limits.nested());
        owners.bytes(&owner_count.to_be_bytes())?;
        for ceiling in &self.required_owner_ceilings {
            owners.bytes(&ceiling.to_be_bytes())?;
        }
        let mut schedules = PhloWireEncoder::new(limits.nested());
        schedules.bytes(&schedule_count.to_be_bytes())?;
        for schedule in &self.permitted_schedules {
            schedules.bytes(&schedule.encode(limits.schedule(schedule.classes.len()))?)?;
        }
        let mut output = PhloWireEncoder::new(limits.wire);
        for field in [
            PHLO_CONTROLS_V1_DOMAIN,
            &self.limit.to_be_bytes(),
            &self.price_ceiling.to_be_bytes(),
            owners.as_bytes(),
            schedules.as_bytes(),
        ] {
            output.bytes(field)?;
        }
        Ok(output.into_bytes())
    }

    pub fn decode(
        input: &'a [u8],
        limits: PhloControlsLimits,
    ) -> Result<Self, PhloControlsWireError> {
        let mut fields = PhloWireDecoder::new(input, limits.wire)?;
        if fields.bytes()? != PHLO_CONTROLS_V1_DOMAIN {
            return Err(PhloControlsWireError::FormatDomain);
        }
        let limit = u64::from_be_bytes(fixed_field(&mut fields)?);
        let price_ceiling = u64::from_be_bytes(fixed_field(&mut fields)?);
        let owner_bytes = fields.bytes()?;
        let schedule_bytes = fields.bytes()?;
        fields.finish()?;
        let mut owners = PhloWireDecoder::new(owner_bytes, limits.nested())?;
        let owner_count = u32::from_be_bytes(fixed_field(&mut owners)?);
        let owner_count =
            usize::try_from(owner_count).map_err(|_| PhloControlsWireError::OwnerLimit)?;
        bounded_count(
            owner_count,
            limits.owners,
            PhloControlsWireError::OwnerLimit,
        )?;
        let mut required_owner_ceilings = Vec::new();
        for _ in 0..owner_count {
            let ceiling = u64::from_be_bytes(fixed_field(&mut owners)?);
            required_owner_ceilings
                .try_reserve(1)
                .map_err(|_| PhloWireError::AllocationFailed)?;
            required_owner_ceilings.push(ceiling);
        }
        owners.finish()?;
        let mut schedules = PhloWireDecoder::new(schedule_bytes, limits.nested())?;
        let schedule_count = u32::from_be_bytes(fixed_field(&mut schedules)?);
        let schedule_count =
            usize::try_from(schedule_count).map_err(|_| PhloControlsWireError::ScheduleLimit)?;
        bounded_count(
            schedule_count,
            limits.schedules,
            PhloControlsWireError::ScheduleLimit,
        )?;
        let mut remaining_classes = limits.total_classes;
        let mut permitted_schedules = Vec::new();
        for _ in 0..schedule_count {
            let schedule =
                PhloScheduleV1::decode(schedules.bytes()?, limits.schedule(remaining_classes))?;
            remaining_classes = remaining_classes
                .checked_sub(schedule.classes.len())
                .ok_or(PhloControlsWireError::ClassLimit)?;
            permitted_schedules
                .try_reserve(1)
                .map_err(|_| PhloWireError::AllocationFailed)?;
            permitted_schedules.push(schedule);
        }
        schedules.finish()?;
        Ok(Self {
            limit,
            price_ceiling,
            required_owner_ceilings,
            permitted_schedules,
        })
    }
}

fn bounded_count(
    count: usize,
    maximum: usize,
    error: PhloControlsWireError,
) -> Result<u32, PhloControlsWireError> {
    if count > maximum {
        return Err(error);
    }
    u32::try_from(count).map_err(|_| error)
}

fn fixed_field<'a, const N: usize>(
    fields: &mut PhloWireDecoder<'a>,
) -> Result<[u8; N], PhloControlsWireError> {
    fields
        .bytes()?
        .try_into()
        .map_err(|_| PhloControlsWireError::FieldWidth)
}

#[cfg(test)]
mod tests;
