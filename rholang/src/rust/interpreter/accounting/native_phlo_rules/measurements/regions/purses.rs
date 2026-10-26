use std::collections::BTreeMap;

use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostSignature, Par};
use prost::Message;
use thiserror::Error;

use super::{NativePhloRegionDemand, NativePhloRegionDemands};
use crate::rust::interpreter::accounting::authority::{cost_signature_to_sig, AuthorityError};
use crate::rust::interpreter::accounting::{Sig, SignatureChannel};

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
    /// The position of this purse in insertion order (C8, DR-87).
    ordinal: usize,
}

impl NativePhloPurse<'_> {
    pub(crate) fn ordinal(&self) -> usize { self.ordinal }
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

fn channel_bytes(signature: &CostSignature) -> Result<usize, NativePhloPurseError> {
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
    ) -> Result<NativePhloLocatedDemands<'_>, NativePhloPurseError> {
        let mut purses: BTreeMap<&[u8], NativePhloPurse<'_>> = BTreeMap::new();
        let mut encoded_bytes = 0_usize;
        for row in self.measurements.observations.rows() {
            for region in &row.authority.regions {
                let original_authority = region
                    .signature
                    .as_ref()
                    .ok_or(AuthorityError::MissingSignature)?;
                let authority_bytes = original_authority.encoded_len();
                if let Some(prior) = purses.get(region.instance_id.as_slice()) {
                    if prior.original_authority != original_authority {
                        return Err(AuthorityError::RegionIdentityConflict.into());
                    }
                    continue;
                }
                if purses.len() >= limits.bindings {
                    return Err(NativePhloPurseError::BindingLimit);
                }
                let expected_channel_bytes = channel_bytes(original_authority)?;
                let binding_bytes = authority_bytes
                    .checked_add(expected_channel_bytes)
                    .ok_or(NativePhloPurseError::BindingByteLimit)?;
                encoded_bytes = encoded_bytes
                    .checked_add(binding_bytes)
                    .filter(|count| *count <= limits.encoded_binding_bytes)
                    .ok_or(NativePhloPurseError::BindingByteLimit)?;
                let authority = cost_signature_to_sig(original_authority)?;
                let channel = SignatureChannel::from_sig(&authority).par;
                if channel.encoded_len() != expected_channel_bytes {
                    return Err(NativePhloPurseError::ChannelShapeMismatch);
                }
                let encoded_channel = channel.encode_to_vec();
                let ordinal = purses.len();
                purses.insert(region.instance_id.as_slice(), NativePhloPurse {
                    original_authority,
                    authority,
                    channel,
                    encoded_channel,
                    ordinal,
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

    /// The purse bindings in region instance-id order (C8, DR-87).
    pub(crate) fn purses(&self) -> impl Iterator<Item = &NativePhloPurse<'_>> {
        self.purses.values()
    }

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
