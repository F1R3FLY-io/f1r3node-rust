use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;
use models::rust::phlo_schedule::{
    PhloResourceClassV1, PhloScheduleError, PhloScheduleLimits, PhloScheduleV1,
};
use thiserror::Error;

use super::{
    NativePhloDimension, NativePhloLocatedDemand, NativePhloLocatedDemands, NativePhloRuleError,
    NativePhloRules,
};
use crate::rust::interpreter::accounting::monetary_allocation::{reserve_work, FundingSearchError};
use crate::rust::interpreter::accounting::phlo_controls::{
    CheckedPhloControls, PhloSchedule, PhloScheduleBinding,
};
use crate::rust::interpreter::accounting::phlo_execution::{PhloResource, PhloResourceAmount};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Copy, Debug)]
pub struct NativePhloAcquisitionLimits {
    pub schedule: PhloScheduleLimits,
    pub entries: usize,
}

#[derive(Debug, Error)]
pub enum NativePhloAcquisitionError {
    #[error(transparent)]
    Schedule(#[from] PhloScheduleError),
    #[error(transparent)]
    Rule(#[from] NativePhloRuleError),
    #[error(transparent)]
    Work(#[from] FundingSearchError),
    #[error("native acquisition terms differ from the selected schedule")]
    TermsMismatch,
    #[error("native acquisition class differs from its measured dimension")]
    ClassMismatch,
    #[error("native acquisition demand entry limit exceeded")]
    EntryLimit,
    #[error("native acquisition controls differ from the selected schedule")]
    ControlsMismatch,
}

#[derive(Debug)]
pub struct NativePhloAcquisitionDemand<'a> {
    located: &'a NativePhloLocatedDemands<'a>,
    schedule: PhloSchedule<'a>,
    terms: &'a [u8],
    resources: Vec<PhloResourceAmount<'a>>,
}

impl NativePhloLocatedDemands<'_> {
    pub fn prepare_acquisition_demand<'a>(
        &'a self,
        binding: &'a PhloScheduleBinding<'_>,
        terms: &'a [u8],
        limits: NativePhloAcquisitionLimits,
        budget: &HostWorkBudget,
    ) -> Result<NativePhloAcquisitionDemand<'a>, NativePhloAcquisitionError> {
        let schedule_limits = PhloScheduleLimits {
            classes: limits.schedule.classes.min(NativePhloDimension::ALL.len()),
            ..limits.schedule
        };
        reserve_work(budget, HostWorkDimension::VerificationBytes, terms.len())?;
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            terms.len(),
        )?;
        reserve_work(
            budget,
            HostWorkDimension::SearchStateBytes,
            schedule_limits
                .classes
                .checked_mul(2 * size_of::<PhloResourceClassV1<'_>>() + 128)
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        let descriptor = PhloScheduleV1::decode(terms, schedule_limits)?;
        if &descriptor != binding.descriptor() {
            return Err(NativePhloAcquisitionError::TermsMismatch);
        }
        let rules = NativePhloRules::resolve(&descriptor)?;
        let mut entries = 0_usize;
        for located in self.occurrences() {
            reserve_work(budget, HostWorkDimension::VerificationOperations, 1)?;
            entries = entries
                .checked_add(1)
                .filter(|count| *count <= limits.entries)
                .ok_or(NativePhloAcquisitionError::EntryLimit)?;
            let measured = located.demand().measurement();
            if rules.class_for(measured.dimension())? != measured.class() {
                return Err(NativePhloAcquisitionError::ClassMismatch);
            }
        }
        reserve_work(
            budget,
            HostWorkDimension::SearchStateBytes,
            entries
                .checked_mul(size_of::<PhloResourceAmount<'_>>())
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        let mut resources = Vec::new();
        resources
            .try_reserve_exact(entries)
            .map_err(|_| FundingSearchError::AllocationFailed)?;
        for located in self.occurrences() {
            reserve_work(budget, HostWorkDimension::VerificationOperations, 1)?;
            let measured = located.demand().measurement();
            resources.push(PhloResourceAmount {
                resource: PhloResource {
                    location: located.purse().encoded_channel(),
                    class: measured.class(),
                    acquisition_terms: terms,
                    authority: located.purse().authority(),
                },
                quantity: measured.quantity(),
            });
        }
        Ok(NativePhloAcquisitionDemand {
            located: self,
            schedule: binding.schedule(),
            terms,
            resources,
        })
    }
}

impl NativePhloAcquisitionDemand<'_> {
    pub fn resources(&self) -> &[PhloResourceAmount<'_>] { &self.resources }
    pub fn schedule(&self) -> PhloSchedule<'_> { self.schedule }
    pub fn terms(&self) -> &[u8] { self.terms }

    pub fn occurrences(
        &self,
    ) -> impl Iterator<Item = (NativePhloLocatedDemand<'_>, PhloResourceAmount<'_>)> {
        self.located
            .occurrences()
            .zip(self.resources.iter().copied())
    }

    pub fn check_controls(
        &self,
        controls: CheckedPhloControls<'_>,
    ) -> Result<(), NativePhloAcquisitionError> {
        if controls.schedule() != self.schedule {
            return Err(NativePhloAcquisitionError::ControlsMismatch);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
