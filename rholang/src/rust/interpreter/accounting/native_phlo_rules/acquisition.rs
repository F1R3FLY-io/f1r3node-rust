use std::mem::size_of;

use models::rust::host_work::HostWorkDimension;
use models::rust::phlo_obligation::PhloObligationKeyLimits;
use models::rust::phlo_schedule::{
    PhloResourceClassV1, PhloScheduleError, PhloScheduleLimits, PhloScheduleV1,
};
use thiserror::Error;

use super::{
    NativePhloDimension, NativePhloLocatedDemand, NativePhloLocatedDemands, NativePhloPurse,
    NativePhloRuleError, NativePhloRules,
};
use crate::rust::interpreter::accounting::monetary_allocation::{
    canonical_funding_key_order, filled_vec, reserve_work, FundingSearchError,
};
use crate::rust::interpreter::accounting::phlo_controls::{
    CheckedPhloControls, PhloSchedule, PhloScheduleBinding,
};
use crate::rust::interpreter::accounting::phlo_execution::{
    PhloObligationError, PhloResource, PhloResourceAmount,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Copy, Debug)]
pub struct NativePhloAcquisitionLimits {
    pub schedule: PhloScheduleLimits,
    /// The limit on distinct demand keys: a purse identity and a class
    /// (C8, DR-87).
    pub entries: usize,
    /// The limits of the obligation-key encoding that identifies a purse
    /// (C8, DR-87).
    pub key: PhloObligationKeyLimits,
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
    #[error(transparent)]
    Key(#[from] PhloObligationError),
    #[error("native acquisition demand has a zero quantity")]
    ZeroQuantity,
    #[error("native acquisition demand quantity overflows")]
    QuantityOverflow,
}

#[derive(Debug)]
pub struct NativePhloAcquisitionDemand<'a> {
    located: &'a NativePhloLocatedDemands<'a>,
    schedule: PhloSchedule<'a>,
    terms: &'a [u8],
    /// One entry for each distinct (purse identity, class), sorted by the
    /// identity encoding and the class, with the summed quantity of its
    /// occurrences (C8, DR-87).
    resources: Vec<PhloResourceAmount<'a>>,
    /// The entry of each occurrence, in occurrence order (C8, DR-87).
    occurrence_entries: Vec<usize>,
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
        // Changed by C8 (DR-87): the demand holds one entry for each distinct
        // (purse identity, class) instead of one for each located
        // occurrence, and the entry limit applies to distinct keys. The
        // replaced lines follow.
        // let mut entries = 0_usize;
        // for located in self.occurrences() {
        //     reserve_work(budget, HostWorkDimension::VerificationOperations, 1)?;
        //     entries = entries
        //         .checked_add(1)
        //         .filter(|count| *count <= limits.entries)
        //         .ok_or(NativePhloAcquisitionError::EntryLimit)?;
        //     let measured = located.demand().measurement();
        //     if rules.class_for(measured.dimension())? != measured.class() {
        //         return Err(NativePhloAcquisitionError::ClassMismatch);
        //     }
        // }
        // reserve_work(
        //     budget,
        //     HostWorkDimension::SearchStateBytes,
        //     entries
        //         .checked_mul(size_of::<PhloResourceAmount<'_>>())
        //         .ok_or(FundingSearchError::Overflow)?,
        // )?;
        // let mut resources = Vec::new();
        // resources
        //     .try_reserve_exact(entries)
        //     .map_err(|_| FundingSearchError::AllocationFailed)?;
        // for located in self.occurrences() {
        //     reserve_work(budget, HostWorkDimension::VerificationOperations, 1)?;
        //     let measured = located.demand().measurement();
        //     resources.push(PhloResourceAmount {
        //         resource: PhloResource {
        //             location: located.purse().encoded_channel(),
        //             class: measured.class(),
        //             acquisition_terms: terms,
        //             authority: located.purse().authority(),
        //         },
        //         quantity: measured.quantity(),
        //     });
        // }
        // Ok(NativePhloAcquisitionDemand {
        //     located: self,
        //     schedule: binding.schedule(),
        //     terms,
        //     resources,
        // })
        let classes = NativePhloDimension::ALL.len();
        let purse_count = self.binding_count();
        reserve_work(
            budget,
            HostWorkDimension::SearchStateBytes,
            purse_count
                .checked_mul(size_of::<usize>())
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        let mut rank_of = filled_vec(purse_count, usize::MAX)?;
        let mut occurrences = 0_usize;
        for located in self.occurrences() {
            reserve_work(budget, HostWorkDimension::VerificationOperations, 1)?;
            occurrences = occurrences
                .checked_add(1)
                .ok_or(FundingSearchError::Overflow)?;
            let measured = located.demand().measurement();
            if rules.class_for(measured.dimension())? != measured.class() {
                return Err(NativePhloAcquisitionError::ClassMismatch);
            }
            if measured.quantity() == 0 {
                return Err(NativePhloAcquisitionError::ZeroQuantity);
            }
            rank_of[located.purse().ordinal()] = 0;
        }
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            purse_count,
        )?;
        reserve_work(
            budget,
            HostWorkDimension::SearchStateBytes,
            purse_count
                .checked_mul(
                    size_of::<Vec<u8>>()
                        + size_of::<&[u8]>()
                        + 2 * size_of::<&NativePhloPurse<'_>>(),
                )
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        let mut identities = Vec::new();
        identities
            .try_reserve_exact(purse_count)
            .map_err(|_| FundingSearchError::AllocationFailed)?;
        let mut owners: Vec<&NativePhloPurse<'_>> = Vec::new();
        owners
            .try_reserve_exact(purse_count)
            .map_err(|_| FundingSearchError::AllocationFailed)?;
        for purse in self.purses() {
            if rank_of[purse.ordinal()] == usize::MAX {
                continue;
            }
            identities.push(
                PhloResource {
                    location: purse.encoded_channel(),
                    class: 0,
                    acquisition_terms: &[],
                    authority: purse.authority(),
                }
                .encoded_obligation_key(limits.key, budget)?,
            );
            owners.push(purse);
        }
        let mut references = Vec::new();
        references
            .try_reserve_exact(identities.len())
            .map_err(|_| FundingSearchError::AllocationFailed)?;
        references.extend(identities.iter().map(Vec::as_slice));
        let order = canonical_funding_key_order(&references, budget)?;
        let mut representatives: Vec<&NativePhloPurse<'_>> = Vec::new();
        representatives
            .try_reserve_exact(owners.len())
            .map_err(|_| FundingSearchError::AllocationFailed)?;
        let mut previous: Option<&[u8]> = None;
        for index in order {
            let identity = references[index];
            let fresh = match previous {
                Some(prior) => {
                    reserve_work(
                        budget,
                        HostWorkDimension::VerificationOperations,
                        prior
                            .len()
                            .min(identity.len())
                            .checked_add(1)
                            .ok_or(FundingSearchError::Overflow)?,
                    )?;
                    prior != identity
                }
                None => true,
            };
            if fresh {
                representatives.push(owners[index]);
            }
            rank_of[owners[index].ordinal()] = representatives.len() - 1;
            previous = Some(identity);
        }
        let slots = representatives
            .len()
            .checked_mul(classes)
            .ok_or(FundingSearchError::Overflow)?;
        reserve_work(
            budget,
            HostWorkDimension::SearchStateBytes,
            slots
                .checked_mul(size_of::<u64>() + size_of::<usize>())
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        reserve_work(budget, HostWorkDimension::VerificationOperations, slots)?;
        let mut slot_quantity = filled_vec(slots, 0_u64)?;
        let mut slot_entry = filled_vec(slots, usize::MAX)?;
        reserve_work(
            budget,
            HostWorkDimension::SearchStateBytes,
            occurrences
                .checked_mul(size_of::<usize>())
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        let mut occurrence_entries = Vec::new();
        occurrence_entries
            .try_reserve_exact(occurrences)
            .map_err(|_| FundingSearchError::AllocationFailed)?;
        let mut entries = 0_usize;
        for located in self.occurrences() {
            reserve_work(budget, HostWorkDimension::VerificationOperations, 1)?;
            let measured = located.demand().measurement();
            let slot = rank_of[located.purse().ordinal()]
                .checked_mul(classes)
                .and_then(|base| base.checked_add(measured.dimension().index()))
                .ok_or(FundingSearchError::Overflow)?;
            if slot_entry[slot] == usize::MAX {
                entries = entries
                    .checked_add(1)
                    .filter(|count| *count <= limits.entries)
                    .ok_or(NativePhloAcquisitionError::EntryLimit)?;
                slot_entry[slot] = 0;
            }
            slot_quantity[slot] = slot_quantity[slot]
                .checked_add(measured.quantity())
                .ok_or(NativePhloAcquisitionError::QuantityOverflow)?;
            occurrence_entries.push(slot);
        }
        reserve_work(
            budget,
            HostWorkDimension::SearchStateBytes,
            entries
                .checked_mul(size_of::<PhloResourceAmount<'_>>())
                .ok_or(FundingSearchError::Overflow)?,
        )?;
        reserve_work(budget, HostWorkDimension::VerificationOperations, slots)?;
        let mut resources = Vec::new();
        resources
            .try_reserve_exact(entries)
            .map_err(|_| FundingSearchError::AllocationFailed)?;
        for (slot, entry) in slot_entry.iter_mut().enumerate() {
            if *entry == usize::MAX {
                continue;
            }
            *entry = resources.len();
            let purse = representatives[slot / classes];
            resources.push(PhloResourceAmount {
                resource: PhloResource {
                    location: purse.encoded_channel(),
                    class: rules.class_for(NativePhloDimension::ALL[slot % classes])?,
                    acquisition_terms: terms,
                    authority: purse.authority(),
                },
                quantity: slot_quantity[slot],
            });
        }
        reserve_work(
            budget,
            HostWorkDimension::VerificationOperations,
            occurrences,
        )?;
        for entry in &mut occurrence_entries {
            *entry = slot_entry[*entry];
        }
        Ok(NativePhloAcquisitionDemand {
            located: self,
            schedule: binding.schedule(),
            terms,
            resources,
            occurrence_entries,
        })
    }
}

impl NativePhloAcquisitionDemand<'_> {
    pub fn resources(&self) -> &[PhloResourceAmount<'_>] { &self.resources }
    pub fn schedule(&self) -> PhloSchedule<'_> { self.schedule }
    pub fn terms(&self) -> &[u8] { self.terms }

    // Changed by C8 (DR-87): the entries are aggregated, so each occurrence
    // rebuilds its own amount from its purse and measurement.
    // pub fn occurrences(
    //     &self,
    // ) -> impl Iterator<Item = (NativePhloLocatedDemand<'_>, PhloResourceAmount<'_>)> {
    //     self.located
    //         .occurrences()
    //         .zip(self.resources.iter().copied())
    // }
    pub fn occurrences(
        &self,
    ) -> impl Iterator<Item = (NativePhloLocatedDemand<'_>, PhloResourceAmount<'_>)> {
        self.located.occurrences().map(move |located| {
            let measured = located.demand().measurement();
            (located, PhloResourceAmount {
                resource: PhloResource {
                    location: located.purse().encoded_channel(),
                    class: measured.class(),
                    acquisition_terms: self.terms,
                    authority: located.purse().authority(),
                },
                quantity: measured.quantity(),
            })
        })
    }

    /// The number of located occurrences (C8, DR-87).
    pub fn occurrence_count(&self) -> usize { self.occurrence_entries.len() }

    /// The entry of the occurrence at `position` (C8, DR-87).
    pub fn occurrence_entry(&self, position: usize) -> Option<usize> {
        self.occurrence_entries.get(position).copied()
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
