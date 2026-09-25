use std::collections::BTreeMap;

use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostSignature, Par};
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use prost::Message;
use thiserror::Error;

use super::{NativePhloRegionDemand, NativePhloRegionDemands};
use crate::rust::interpreter::accounting::authority::{cost_signature_to_sig, AuthorityError};
use crate::rust::interpreter::accounting::{Sig, SignatureChannel};
use crate::rust::interpreter::host_work::HostWorkBudget;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativePhloPurseLimits {
    pub bindings: usize,
    pub encoded_binding_bytes: usize,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum NativePhloPurseError {
    #[error(transparent)]
    Authority(#[from] AuthorityError),
    #[error("native purse binding limit exceeded")]
    BindingLimit,
    #[error("native purse binding byte limit exceeded")]
    BindingByteLimit,
    #[error("native signature channel size differs from its checked funding shape")]
    ChannelShapeMismatch,
}

#[derive(Debug, PartialEq, Eq)]
pub struct NativePhloPurse<'a> {
    original_authority: &'a CostSignature,
    authority: Sig,
    channel: Par,
    encoded_channel: Vec<u8>,
}

impl NativePhloPurse<'_> {
    pub fn original_authority(&self) -> &CostSignature { self.original_authority }
    pub fn authority(&self) -> &Sig { &self.authority }
    pub fn channel(&self) -> &Par { &self.channel }
    pub fn encoded_channel(&self) -> &[u8] { &self.encoded_channel }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativePhloLocatedDemand<'a> {
    demand: NativePhloRegionDemand<'a>,
    purse: &'a NativePhloPurse<'a>,
}

impl<'a> NativePhloLocatedDemand<'a> {
    pub fn demand(&self) -> NativePhloRegionDemand<'a> { self.demand }
    pub fn purse(&self) -> &'a NativePhloPurse<'a> { self.purse }
}

#[derive(Debug)]
pub struct NativePhloLocatedDemands<'a> {
    demands: &'a NativePhloRegionDemands<'a>,
    purses: BTreeMap<&'a [u8], NativePhloPurse<'a>>,
}

fn reserve(
    host_work: &HostWorkBudget,
    dimension: HostWorkDimension,
    amount: usize,
) -> Result<(), NativePhloPurseError> {
    host_work
        .reserve(
            dimension,
            HostWorkUnits::new(
                u64::try_from(amount).map_err(|_| AuthorityError::ArithmeticOverflow)?,
            ),
        )
        .map_err(AuthorityError::from)?;
    Ok(())
}

fn channel_bytes(
    signature: &CostSignature,
    host_work: &HostWorkBudget,
) -> Result<usize, NativePhloPurseError> {
    reserve(host_work, HostWorkDimension::StructuralItems, 1)?;
    let atom_bytes = SignatureChannel::from_sig(&Sig::Ground(Vec::new()))
        .par
        .encoded_len();
    let mut pending = vec![signature];
    let mut bytes = 0_usize;
    while let Some(signature) = pending.pop() {
        match signature.value.as_ref() {
            Some(Value::Unit(true)) => {}
            Some(Value::Ground(_) | Value::Quote(_) | Value::Name(_)) => {
                bytes = bytes
                    .checked_add(atom_bytes)
                    .ok_or(AuthorityError::ArithmeticOverflow)?;
            }
            Some(Value::Compound(compound)) => {
                reserve(
                    host_work,
                    HostWorkDimension::StructuralItems,
                    compound.elements.len(),
                )?;
                pending.extend(&compound.elements);
            }
            _ => return Err(AuthorityError::UnsupportedFundingSignature.into()),
        }
    }
    Ok(bytes)
}

impl NativePhloRegionDemands<'_> {
    pub fn locate_purses(
        &self,
        limits: NativePhloPurseLimits,
        host_work: &HostWorkBudget,
    ) -> Result<NativePhloLocatedDemands<'_>, NativePhloPurseError> {
        let mut purses: BTreeMap<&[u8], NativePhloPurse<'_>> = BTreeMap::new();
        let mut encoded_bytes = 0_usize;
        for row in self.measurements.observations.rows() {
            reserve(host_work, HostWorkDimension::VerificationOperations, 1)?;
            for region in &row.authority.regions {
                reserve(host_work, HostWorkDimension::VerificationOperations, 1)?;
                let original_authority = region
                    .signature
                    .as_ref()
                    .ok_or(AuthorityError::MissingSignature)?;
                let authority_bytes = original_authority.encoded_len();
                reserve(
                    host_work,
                    HostWorkDimension::VerificationBytes,
                    authority_bytes,
                )?;
                if let Some(prior) = purses.get(region.instance_id.as_slice()) {
                    if prior.original_authority != original_authority {
                        return Err(AuthorityError::RegionIdentityConflict.into());
                    }
                    continue;
                }
                if purses.len() >= limits.bindings {
                    return Err(NativePhloPurseError::BindingLimit);
                }
                let expected_channel_bytes = channel_bytes(original_authority, host_work)?;
                let binding_bytes = authority_bytes
                    .checked_add(expected_channel_bytes)
                    .ok_or(NativePhloPurseError::BindingByteLimit)?;
                encoded_bytes = encoded_bytes
                    .checked_add(binding_bytes)
                    .filter(|count| *count <= limits.encoded_binding_bytes)
                    .ok_or(NativePhloPurseError::BindingByteLimit)?;
                reserve(host_work, HostWorkDimension::StructuralBytes, binding_bytes)?;
                let authority = cost_signature_to_sig(original_authority)?;
                let channel = SignatureChannel::from_sig(&authority).par;
                if channel.encoded_len() != expected_channel_bytes {
                    return Err(NativePhloPurseError::ChannelShapeMismatch);
                }
                let encoded_channel = channel.encode_to_vec();
                purses.insert(region.instance_id.as_slice(), NativePhloPurse {
                    original_authority,
                    authority,
                    channel,
                    encoded_channel,
                });
            }
        }
        Ok(NativePhloLocatedDemands {
            demands: self,
            purses,
        })
    }
}

impl NativePhloLocatedDemands<'_> {
    pub fn binding_count(&self) -> usize { self.purses.len() }

    pub fn occurrences(&self) -> impl Iterator<Item = NativePhloLocatedDemand<'_>> {
        self.demands
            .occurrences()
            .map(|demand| NativePhloLocatedDemand {
                purse: self
                    .purses
                    .get(demand.region().instance_id.as_slice())
                    .expect("every immutable measured region has a checked purse binding"),
                demand,
            })
    }
}

#[cfg(test)]
mod tests;
