use thiserror::Error;

use super::phlo_resource::{
    checked_field_size, PhloResourceEncoding, PhloResourceKeyError, PhloResourceKeyV1,
    PhloResourceLimits,
};
use super::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireError, PhloWireLimits};

pub const PHLO_OBLIGATION_V1_DOMAIN: &[u8] = b"f1r3node:phlo-obligation:v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PhloObligationKeyV1<'a> {
    Fee,
    Resource(PhloResourceKeyV1<'a>),
    RetainedResource(PhloResourceKeyV1<'a>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloObligationKeyLimits {
    pub wire: PhloWireLimits,
    pub authority_nodes: usize,
}

impl PhloObligationKeyLimits {
    fn resource(self, available: usize) -> PhloResourceLimits {
        PhloResourceLimits {
            wire: PhloWireLimits {
                total_bytes: available.min(self.wire.field_bytes),
                field_bytes: self.wire.field_bytes,
            },
            authority_nodes: self.authority_nodes,
        }
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloObligationKeyError {
    #[error("unsupported phlo obligation-key format domain")]
    FormatDomain,
    #[error("phlo obligation key has an invalid kind or fee payload")]
    InvalidKind,
    #[error(transparent)]
    Resource(#[from] PhloResourceKeyError),
    #[error(transparent)]
    Wire(#[from] PhloWireError),
}

impl<'a> PhloObligationKeyV1<'a> {
    pub fn encode(
        &self,
        limits: PhloObligationKeyLimits,
    ) -> Result<Vec<u8>, PhloObligationKeyError> {
        self.prepare_encoding(limits)?.encode()
    }

    pub fn prepare_encoding(
        &self,
        limits: PhloObligationKeyLimits,
    ) -> Result<PhloObligationEncoding<'_, 'a>, PhloObligationKeyError> {
        let header = checked_field_size(PHLO_OBLIGATION_V1_DOMAIN.len(), limits.wire)?
            .checked_add(checked_field_size(1, limits.wire)?)
            .ok_or(PhloWireError::LimitExceeded)?;
        let available = limits
            .wire
            .total_bytes
            .checked_sub(header)
            .and_then(|bytes| bytes.checked_sub(8))
            .ok_or(PhloWireError::LimitExceeded)?;
        let resource = match self {
            Self::Fee => None,
            Self::Resource(resource) | Self::RetainedResource(resource) => {
                Some(resource.prepare_encoding(limits.resource(available))?)
            }
        };
        let size = header
            .checked_add(checked_field_size(
                resource.as_ref().map_or(0, |resource| resource.size),
                limits.wire,
            )?)
            .filter(|size| *size <= limits.wire.total_bytes)
            .ok_or(PhloWireError::LimitExceeded)?;
        Ok(PhloObligationEncoding {
            kind: match self {
                Self::Fee => 0,
                Self::Resource(_) => 1,
                Self::RetainedResource(_) => 2,
            },
            resource,
            size,
            limits: limits.wire,
        })
    }

    pub fn decode(
        input: &'a [u8],
        limits: PhloObligationKeyLimits,
    ) -> Result<Self, PhloObligationKeyError> {
        let mut fields = PhloWireDecoder::new(input, limits.wire)?;
        if fields.bytes()? != PHLO_OBLIGATION_V1_DOMAIN {
            return Err(PhloObligationKeyError::FormatDomain);
        }
        let kind = fields.bytes()?;
        let payload = fields.bytes()?;
        fields.finish()?;
        match (kind, payload.is_empty()) {
            ([0], true) => Ok(Self::Fee),
            ([1], _) => Ok(Self::Resource(PhloResourceKeyV1::decode(
                payload,
                limits.resource(payload.len()),
            )?)),
            ([2], _) => Ok(Self::RetainedResource(PhloResourceKeyV1::decode(
                payload,
                limits.resource(payload.len()),
            )?)),
            _ => Err(PhloObligationKeyError::InvalidKind),
        }
    }
}

pub struct PhloObligationEncoding<'record, 'data> {
    kind: u8,
    resource: Option<PhloResourceEncoding<'record, 'data>>,
    size: usize,
    limits: PhloWireLimits,
}

impl PhloObligationEncoding<'_, '_> {
    pub fn encoded_len(&self) -> usize { self.size }

    pub fn encode(&self) -> Result<Vec<u8>, PhloObligationKeyError> {
        let mut output = PhloWireEncoder::with_capacity(self.limits, self.size)?;
        output.bytes(PHLO_OBLIGATION_V1_DOMAIN)?;
        if let Some(resource) = &self.resource {
            output.bytes(&[self.kind])?;
            output
                .u64(u64::try_from(resource.size).map_err(|_| PhloWireError::LengthOutOfRange)?)?;
            resource.write_to(&mut output)?;
        } else {
            output.bytes(&[0])?;
            output.bytes(&[])?;
        }
        Ok(output.into_bytes())
    }
}

#[cfg(test)]
mod tests;
