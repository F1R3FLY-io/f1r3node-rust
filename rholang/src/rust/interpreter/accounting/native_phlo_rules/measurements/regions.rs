// Added by G3 (DR-135, S7): the client's demand leaves out owner regions.
use std::collections::BTreeSet;

use models::rhoapi::CostRegion;
use prost::Message;
use thiserror::Error;

use super::{NativePhloMeasurement, NativePhloMeasurements};
use crate::rust::interpreter::accounting::authority::{canonical_cost_signature, AuthorityError};

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

/// Added by G3 (DR-135, S7): whether the demand of the client keeps a
/// region. Owner funding keeps the client regions and leaves out the owner
/// regions, which their owners pay (R4, R10). It fails when the region is
/// malformed, as `OwnerFundingBinding::is_owner_region` does.
pub type NativePhloRegionFilter<'f> = &'f dyn Fn(&CostRegion) -> Result<bool, AuthorityError>;

#[derive(Debug)]
pub struct NativePhloRegionDemands<'a> {
    measurements: &'a NativePhloMeasurements<'a>,
    /// Added by G3 (DR-135, S7): the identities of the regions that the
    /// filter of `region_demands_filtered` left out. `occurrences` and
    /// `locate_purses` skip them. `indexed_occurrences`, which the meter
    /// reads, keeps every region.
    excluded: BTreeSet<[u8; 32]>,
}

impl NativePhloMeasurements<'_> {
    // Changed by G3 (DR-135, S7): `region_demands_filtered` with no filter,
    // so every existing caller keeps every region.
    // pub fn region_demands(
    //     &self,
    //     limits: NativePhloRegionLimits,
    // ) -> Result<NativePhloRegionDemands<'_>, NativePhloRegionError> {
    pub fn region_demands(
        &self,
        limits: NativePhloRegionLimits,
    ) -> Result<NativePhloRegionDemands<'_>, NativePhloRegionError> {
        self.region_demands_filtered(limits, None)
    }

    /// Added by G3 (DR-135, S7): `region_demands` with `filter`, which leaves
    /// a region out of the demand when it gives `false` (R10). The size and
    /// shape checks still count every region.
    pub fn region_demands_filtered(
        &self,
        limits: NativePhloRegionLimits,
        filter: Option<NativePhloRegionFilter<'_>>,
    ) -> Result<NativePhloRegionDemands<'_>, NativePhloRegionError> {
        let mut region_count = 0_usize;
        let mut byte_count = 0_usize;
        for row in self.observations.rows() {
            region_count = region_count
                .checked_add(row.authority.regions.len())
                .filter(|count| *count <= limits.regions)
                .ok_or(NativePhloRegionError::RegionLimit)?;
            for region in &row.authority.regions {
                if region.instance_id.len() != 32 {
                    return Err(AuthorityError::InvalidRegionIdentity.into());
                }
                if region.signature.is_none() {
                    return Err(AuthorityError::MissingSignature.into());
                }
            }
            let encoded_bytes = row.authority.encoded_len();
            byte_count = byte_count
                .checked_add(encoded_bytes)
                .filter(|count| *count <= limits.encoded_authority_bytes)
                .ok_or(NativePhloRegionError::AuthorityByteLimit)?;
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
        // Changed by G3 (DR-135, S7): the demand records the regions that the
        // filter leaves out.
        // Ok(NativePhloRegionDemands { measurements: self })
        let mut excluded = BTreeSet::new();
        if let Some(filter) = filter {
            for row in self.observations.rows() {
                for region in &row.authority.regions {
                    if !filter(region)? {
                        let identity: [u8; 32] = region
                            .instance_id
                            .as_slice()
                            .try_into()
                            .map_err(|_| AuthorityError::InvalidRegionIdentity)?;
                        excluded.insert(identity);
                    }
                }
            }
        }
        Ok(NativePhloRegionDemands {
            measurements: self,
            excluded,
        })
    }
}

impl NativePhloRegionDemands<'_> {
    /// Added by G3 (DR-135, S7): whether the filter left `region` out of the
    /// demand.
    pub(super) fn excludes(&self, region: &CostRegion) -> bool {
        <[u8; 32]>::try_from(region.instance_id.as_slice())
            .is_ok_and(|identity| self.excluded.contains(&identity))
    }

    // Changed by G3 (DR-135, S7): the occurrences skip the regions that the
    // filter left out.
    // pub fn occurrences(&self) -> impl Iterator<Item = NativePhloRegionDemand<'_>> {
    //     self.measurements.occurrences().flat_map(|measurement| {
    //         measurement
    //             .observation()
    //             .authority
    //             .regions
    //             .iter()
    //             .map(move |region| NativePhloRegionDemand {
    //                 measurement,
    //                 region,
    //             })
    //     })
    // }
    pub fn occurrences(&self) -> impl Iterator<Item = NativePhloRegionDemand<'_>> {
        self.measurements
            .occurrences()
            .flat_map(move |measurement| {
                measurement
                    .observation()
                    .authority
                    .regions
                    .iter()
                    .filter(move |region| !self.excludes(region))
                    .map(move |region| NativePhloRegionDemand {
                        measurement,
                        region,
                    })
            })
    }

    /// Added by G3 (DR-135): the occurrences of `occurrences`, each with the
    /// position of its region in the authority of its observation. The owner
    /// shares of a charge (R4) sum the usage of each region position.
    pub fn indexed_occurrences(&self) -> impl Iterator<Item = (usize, NativePhloRegionDemand<'_>)> {
        self.measurements.occurrences().flat_map(|measurement| {
            measurement
                .observation()
                .authority
                .regions
                .iter()
                .enumerate()
                .map(move |(index, region)| {
                    (index, NativePhloRegionDemand {
                        measurement,
                        region,
                    })
                })
        })
    }
}

#[cfg(test)]
mod tests;
