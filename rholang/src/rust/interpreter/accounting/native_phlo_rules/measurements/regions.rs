use models::rhoapi::CostRegion;
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use prost::Message;
use thiserror::Error;

use super::{NativePhloMeasurement, NativePhloMeasurements};
use crate::rust::interpreter::accounting::authority::{
    canonical_cost_signature, reserve_authority_signature_tree, AuthorityError,
};
use crate::rust::interpreter::host_work::HostWorkBudget;

mod purses;
pub use purses::{
    NativePhloLocatedDemand, NativePhloLocatedDemands, NativePhloPurse, NativePhloPurseError,
    NativePhloPurseLimits,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativePhloRegionLimits {
    pub regions: usize,
    pub encoded_authority_bytes: usize,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum NativePhloRegionError {
    #[error(transparent)]
    Authority(#[from] AuthorityError),
    #[error("native measurement region limit exceeded")]
    RegionLimit,
    #[error("native measurement authority byte limit exceeded")]
    AuthorityByteLimit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativePhloRegionDemand<'a> {
    measurement: NativePhloMeasurement<'a>,
    region: &'a CostRegion,
}

impl<'a> NativePhloRegionDemand<'a> {
    pub fn measurement(&self) -> NativePhloMeasurement<'a> { self.measurement }
    pub fn region(&self) -> &'a CostRegion { self.region }
}

#[derive(Debug)]
pub struct NativePhloRegionDemands<'a> {
    measurements: &'a NativePhloMeasurements<'a>,
}

impl NativePhloMeasurements<'_> {
    pub fn region_demands(
        &self,
        limits: NativePhloRegionLimits,
        host_work: &HostWorkBudget,
    ) -> Result<NativePhloRegionDemands<'_>, NativePhloRegionError> {
        let mut region_count = 0_usize;
        let mut byte_count = 0_usize;
        let mut depth = 0_u64;
        for row in self.observations.rows() {
            host_work
                .reserve(
                    HostWorkDimension::VerificationOperations,
                    HostWorkUnits::new(1),
                )
                .map_err(AuthorityError::from)?;
            region_count = region_count
                .checked_add(row.authority.regions.len())
                .filter(|count| *count <= limits.regions)
                .ok_or(NativePhloRegionError::RegionLimit)?;
            for region in &row.authority.regions {
                if region.instance_id.len() != 32 {
                    return Err(AuthorityError::InvalidRegionIdentity.into());
                }
                let signature = region
                    .signature
                    .as_ref()
                    .ok_or(AuthorityError::MissingSignature)?;
                reserve_authority_signature_tree(signature, host_work, &mut depth)?;
            }
            let encoded_bytes = row.authority.encoded_len();
            byte_count = byte_count
                .checked_add(encoded_bytes)
                .filter(|count| *count <= limits.encoded_authority_bytes)
                .ok_or(NativePhloRegionError::AuthorityByteLimit)?;
            host_work
                .reserve(
                    HostWorkDimension::StructuralBytes,
                    HostWorkUnits::new(
                        u64::try_from(encoded_bytes)
                            .map_err(|_| AuthorityError::ArithmeticOverflow)?,
                    ),
                )
                .map_err(AuthorityError::from)?;
            let mut previous: Option<&CostRegion> = None;
            for region in &row.authority.regions {
                if previous.is_some_and(|prior| prior.instance_id >= region.instance_id) {
                    return Err(AuthorityError::NonCanonicalAuthority.into());
                }
                canonical_cost_signature(
                    region
                        .signature
                        .as_ref()
                        .ok_or(AuthorityError::MissingSignature)?,
                )?;
                previous = Some(region);
            }
        }
        if self
            .occurrences()
            .any(|part| part.observation().authority.regions.is_empty())
        {
            return Err(AuthorityError::MissingAuthority.into());
        }
        Ok(NativePhloRegionDemands { measurements: self })
    }
}

impl NativePhloRegionDemands<'_> {
    pub fn occurrences(&self) -> impl Iterator<Item = NativePhloRegionDemand<'_>> {
        self.measurements.occurrences().flat_map(|measurement| {
            measurement
                .observation()
                .authority
                .regions
                .iter()
                .map(move |region| NativePhloRegionDemand {
                    measurement,
                    region,
                })
        })
    }
}

#[cfg(test)]
mod tests;
