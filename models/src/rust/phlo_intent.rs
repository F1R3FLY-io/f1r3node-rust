use std::collections::BTreeSet;

use thiserror::Error;

use super::phlo_controls::{PhloControlsLimits, PhloControlsV1, PhloControlsWireError};
use super::phlo_source::{PhloSourceError, PhloSourceLimits, PhloSourcePolicyV1};
use super::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireError, PhloWireLimits};

pub const PHLO_FUNDING_INTENT_V1_DOMAIN: &[u8] = b"f1r3node:phlo-funding-intent:v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloFundingIntentV1<'a> {
    pub controls: PhloControlsV1<'a>,
    pub schedule_commitment: [u8; 32],
    pub total_exposure: u128,
    pub sources: Vec<PhloSourcePolicyV1<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloFundingIntentLimits {
    pub wire: PhloWireLimits,
    pub controls: PhloControlsLimits,
    pub sources: usize,
    pub resource_permissions: usize,
    pub authority_nodes: usize,
}

impl PhloFundingIntentLimits {
    fn nested(self) -> PhloWireLimits {
        PhloWireLimits {
            total_bytes: self.wire.field_bytes,
            field_bytes: self.wire.field_bytes,
        }
    }

    pub fn controls(self) -> PhloControlsLimits {
        PhloControlsLimits {
            wire: PhloWireLimits {
                total_bytes: self.controls.wire.total_bytes.min(self.wire.field_bytes),
                field_bytes: self.controls.wire.field_bytes.min(self.wire.field_bytes),
            },
            ..self.controls
        }
    }

    fn source(self) -> PhloSourceLimits {
        PhloSourceLimits {
            wire: self.nested(),
            resource_permissions: self.resource_permissions,
            authority_nodes: self.authority_nodes,
        }
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloFundingIntentError {
    #[error("unsupported phlo funding-intent format domain")]
    FormatDomain,
    #[error("phlo funding intent exceeds its source limit")]
    SourceLimit,
    #[error("phlo funding intent repeats a physical custody identity")]
    DuplicateCustody,
    #[error("phlo funding intent exceeds its aggregate resource-permission limit")]
    PermissionLimit,
    #[error("phlo funding intent exceeds its aggregate authority-node limit")]
    NodeLimit,
    #[error("phlo funding-intent field has a noncanonical width")]
    FieldWidth,
    #[error(transparent)]
    Controls(#[from] PhloControlsWireError),
    #[error(transparent)]
    Source(#[from] PhloSourceError),
    #[error(transparent)]
    Wire(#[from] PhloWireError),
}

impl<'a> PhloFundingIntentV1<'a> {
    pub fn encode(
        &self,
        limits: PhloFundingIntentLimits,
    ) -> Result<Vec<u8>, PhloFundingIntentError> {
        let count = check_count(self.sources.len(), limits.sources)?;
        let mut remaining = limits.source();
        let mut custody = BTreeSet::new();
        let mut sources = PhloWireEncoder::new(limits.nested());
        sources.bytes(&count.to_be_bytes())?;
        for source in &self.sources {
            if !custody.insert(source.custody()) {
                return Err(PhloFundingIntentError::DuplicateCustody);
            }
            let bytes = source.encode(remaining)?;
            consume_budget(&mut remaining, source)?;
            sources.bytes(&bytes)?;
        }
        let controls = self.controls.encode(limits.controls())?;
        let mut output = PhloWireEncoder::new(limits.wire);
        for field in [
            PHLO_FUNDING_INTENT_V1_DOMAIN,
            controls.as_slice(),
            &self.schedule_commitment,
            &self.total_exposure.to_be_bytes(),
            sources.as_bytes(),
        ] {
            output.bytes(field)?;
        }
        Ok(output.into_bytes())
    }

    pub fn decode(
        input: &'a [u8],
        limits: PhloFundingIntentLimits,
    ) -> Result<Self, PhloFundingIntentError> {
        let mut fields = PhloWireDecoder::new(input, limits.wire)?;
        if fields.bytes()? != PHLO_FUNDING_INTENT_V1_DOMAIN {
            return Err(PhloFundingIntentError::FormatDomain);
        }
        let control_bytes = fields.bytes()?;
        let schedule_commitment = fixed_field(&mut fields)?;
        let total_exposure = u128::from_be_bytes(fixed_field(&mut fields)?);
        let source_bytes = fields.bytes()?;
        fields.finish()?;
        let controls = PhloControlsV1::decode(control_bytes, limits.controls())?;
        let mut source_fields = PhloWireDecoder::new(source_bytes, limits.nested())?;
        let count = usize::try_from(u32::from_be_bytes(fixed_field(&mut source_fields)?))
            .map_err(|_| PhloFundingIntentError::SourceLimit)?;
        check_count(count, limits.sources)?;
        let mut remaining = limits.source();
        let mut custody = BTreeSet::new();
        let mut sources = Vec::new();
        for _ in 0..count {
            let source = PhloSourcePolicyV1::decode(source_fields.bytes()?, remaining)?;
            if !custody.insert(source.custody()) {
                return Err(PhloFundingIntentError::DuplicateCustody);
            }
            consume_budget(&mut remaining, &source)?;
            sources
                .try_reserve(1)
                .map_err(|_| PhloWireError::AllocationFailed)?;
            sources.push(source);
        }
        source_fields.finish()?;
        Ok(Self {
            controls,
            schedule_commitment,
            total_exposure,
            sources,
        })
    }
}

fn check_count(count: usize, maximum: usize) -> Result<u32, PhloFundingIntentError> {
    if count > maximum {
        return Err(PhloFundingIntentError::SourceLimit);
    }
    u32::try_from(count).map_err(|_| PhloFundingIntentError::SourceLimit)
}

fn consume_budget(
    remaining: &mut PhloSourceLimits,
    source: &PhloSourcePolicyV1<'_>,
) -> Result<(), PhloFundingIntentError> {
    remaining.resource_permissions = remaining
        .resource_permissions
        .checked_sub(source.resources().len())
        .ok_or(PhloFundingIntentError::PermissionLimit)?;
    for resource in source.resources() {
        remaining.authority_nodes = remaining
            .authority_nodes
            .checked_sub(resource.authority.len())
            .ok_or(PhloFundingIntentError::NodeLimit)?;
    }
    Ok(())
}

fn fixed_field<const N: usize>(
    fields: &mut PhloWireDecoder<'_>,
) -> Result<[u8; N], PhloFundingIntentError> {
    fields
        .bytes()?
        .try_into()
        .map_err(|_| PhloFundingIntentError::FieldWidth)
}

#[cfg(test)]
mod tests;
