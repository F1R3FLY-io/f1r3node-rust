use thiserror::Error;

use super::{NativePhloDimension, NativePhloRuleError, NativePhloRules};
use crate::rust::interpreter::accounting::authority::AuthorityByteEventKind;
use crate::rust::interpreter::accounting::byte_receipts::{
    ByteMeasurementError, ByteObservation, ByteObservationSnapshot, CheckedByteMeasurements,
};

mod regions;
pub use regions::{
    NativePhloLocatedDemand, NativePhloLocatedDemands, NativePhloPurse, NativePhloPurseError,
    NativePhloPurseLimits, NativePhloRegionDemand, NativePhloRegionDemands, NativePhloRegionError,
    NativePhloRegionLimits,
};

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum NativePhloMeasurementError {
    #[error(transparent)]
    Observation(#[from] ByteMeasurementError),
    #[error(transparent)]
    Rule(#[from] NativePhloRuleError),
    #[error("native phlo interaction count overflow")]
    InteractionOverflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativePhloMeasurement<'a> {
    observation: &'a ByteObservation,
    dimension: NativePhloDimension,
    class: usize,
    quantity: u64,
}

impl<'a> NativePhloMeasurement<'a> {
    pub fn observation(&self) -> &'a ByteObservation { self.observation }
    pub fn dimension(&self) -> NativePhloDimension { self.dimension }
    pub fn class(&self) -> usize { self.class }
    pub fn quantity(&self) -> u64 { self.quantity }
}

#[derive(Debug)]
pub struct NativePhloMeasurements<'a> {
    rules: &'a NativePhloRules,
    observations: CheckedByteMeasurements<'a>,
    totals: [u64; 4],
}

impl NativePhloRules {
    pub fn measure<'a>(
        &'a self,
        snapshot: &'a ByteObservationSnapshot,
        maximum_entries: usize,
    ) -> Result<NativePhloMeasurements<'a>, NativePhloMeasurementError> {
        let observations = snapshot.checked_measurements(maximum_entries)?;
        let bytes = observations.totals();
        let comms = observations.rows().iter().try_fold(0_u64, |total, row| {
            total
                .checked_add(u64::from(row.kind == AuthorityByteEventKind::Comm))
                .ok_or(NativePhloMeasurementError::InteractionOverflow)
        })?;
        let totals = [
            comms,
            bytes.introduction_bytes,
            bytes.transfer_bytes,
            bytes.trace_bytes,
        ];
        for dimension in NativePhloDimension::ALL {
            if totals[dimension.index()] > 0 {
                self.class_for(dimension)?;
            }
        }
        Ok(NativePhloMeasurements {
            rules: self,
            observations,
            totals,
        })
    }
}

impl NativePhloMeasurements<'_> {
    pub fn total(&self, dimension: NativePhloDimension) -> u64 { self.totals[dimension.index()] }

    pub fn occurrences(&self) -> impl Iterator<Item = NativePhloMeasurement<'_>> {
        self.observations.rows().iter().flat_map(move |row| {
            let raw = row.measurement.expect("checked raw measurement");
            let quantities = [
                u64::from(row.kind == AuthorityByteEventKind::Comm),
                raw.introduction_bytes,
                raw.transfer_bytes,
                raw.trace_bytes,
            ];
            NativePhloDimension::ALL
                .into_iter()
                .zip(quantities)
                .filter(|(_, quantity)| *quantity > 0)
                .map(move |(dimension, quantity)| NativePhloMeasurement {
                    observation: row,
                    dimension,
                    class: self
                        .rules
                        .class_for(dimension)
                        .expect("positive dimension has a checked native class"),
                    quantity,
                })
        })
    }
}

#[cfg(test)]
mod tests;
