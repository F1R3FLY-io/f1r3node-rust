use thiserror::Error;

use super::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireError, PhloWireLimits};

pub const PHLO_RESOURCE_V1_DOMAIN: &[u8] = b"f1r3node:phlo-resource:v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PhloAuthorityNode<'a> {
    Unit,
    Ground(&'a [u8]),
    Quote(&'a [u8]),
    And,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloResourceKeyV1<'a> {
    pub location: &'a [u8],
    pub class: u32,
    pub acquisition_terms: &'a [u8],
    pub authority: Vec<PhloAuthorityNode<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloResourceLimits {
    pub wire: PhloWireLimits,
    pub authority_nodes: usize,
}

impl PhloResourceLimits {
    fn nested(self) -> PhloWireLimits {
        PhloWireLimits {
            total_bytes: self.wire.field_bytes,
            field_bytes: self.wire.field_bytes,
        }
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloResourceKeyError {
    #[error("unsupported phlo resource-key format domain")]
    FormatDomain,
    #[error("phlo resource key exceeds the authority-node limit")]
    NodeLimit,
    #[error("phlo resource key has a noncanonical field width")]
    FieldWidth,
    #[error("phlo resource key contains an unsupported authority node")]
    UnsupportedNode,
    #[error("phlo resource key does not contain exactly one complete authority tree")]
    AuthorityShape,
    #[error(transparent)]
    Wire(#[from] PhloWireError),
}

impl<'a> PhloResourceKeyV1<'a> {
    pub fn encode(&self, limits: PhloResourceLimits) -> Result<Vec<u8>, PhloResourceKeyError> {
        let prepared = self.prepare_encoding(limits)?;
        let mut output = PhloWireEncoder::with_capacity(limits.wire, prepared.size)?;
        prepared.write_to(&mut output)?;
        Ok(output.into_bytes())
    }

    pub(super) fn prepare_encoding(
        &self,
        limits: PhloResourceLimits,
    ) -> Result<PhloResourceEncoding<'_, 'a>, PhloResourceKeyError> {
        let count = checked_count(self.authority.len(), limits)?;
        let mut nodes_size = checked_field_size(4, limits.nested())?;
        let mut slots = 1usize;
        for node in &self.authority {
            advance_shape(&mut slots, *node)?;
            let (_, payload) = node_fields(*node);
            let node_size = checked_field_size(1, limits.nested())?
                .checked_add(checked_field_size(payload.len(), limits.nested())?)
                .filter(|size| *size <= limits.nested().total_bytes)
                .ok_or(PhloWireError::LimitExceeded)?;
            nodes_size = nodes_size
                .checked_add(checked_field_size(node_size, limits.nested())?)
                .filter(|size| *size <= limits.nested().total_bytes)
                .ok_or(PhloWireError::LimitExceeded)?;
        }
        complete_shape(slots)?;
        let size = [
            PHLO_RESOURCE_V1_DOMAIN.len(),
            self.location.len(),
            4,
            self.acquisition_terms.len(),
            nodes_size,
        ]
        .into_iter()
        .try_fold(0usize, |size, field| {
            size.checked_add(checked_field_size(field, limits.wire)?)
                .filter(|size| *size <= limits.wire.total_bytes)
                .ok_or(PhloWireError::LimitExceeded)
        })?;
        Ok(PhloResourceEncoding {
            record: self,
            size,
            nodes_size,
            count,
        })
    }

    pub fn decode(
        input: &'a [u8],
        limits: PhloResourceLimits,
    ) -> Result<Self, PhloResourceKeyError> {
        let mut fields = PhloWireDecoder::new(input, limits.wire)?;
        if fields.bytes()? != PHLO_RESOURCE_V1_DOMAIN {
            return Err(PhloResourceKeyError::FormatDomain);
        }
        let location = fields.bytes()?;
        let class = u32::from_be_bytes(fixed_field(&mut fields)?);
        let acquisition_terms = fields.bytes()?;
        let mut nodes = PhloWireDecoder::new(fields.bytes()?, limits.nested())?;
        fields.finish()?;
        let count = usize::try_from(u32::from_be_bytes(fixed_field(&mut nodes)?))
            .map_err(|_| PhloResourceKeyError::NodeLimit)?;
        checked_count(count, limits)?;
        let mut authority = Vec::new();
        let mut slots = 1usize;
        for _ in 0..count {
            let mut fields = PhloWireDecoder::new(nodes.bytes()?, limits.nested())?;
            let [tag] = fixed_field(&mut fields)?;
            let payload = fields.bytes()?;
            fields.finish()?;
            let node = match (tag, payload.is_empty()) {
                (0, true) => PhloAuthorityNode::Unit,
                (1, _) => PhloAuthorityNode::Ground(payload),
                (2, _) => PhloAuthorityNode::Quote(payload),
                (3, true) => PhloAuthorityNode::And,
                (0 | 3, false) => return Err(PhloResourceKeyError::AuthorityShape),
                _ => return Err(PhloResourceKeyError::UnsupportedNode),
            };
            advance_shape(&mut slots, node)?;
            authority
                .try_reserve(1)
                .map_err(|_| PhloWireError::AllocationFailed)?;
            authority.push(node);
        }
        nodes.finish()?;
        complete_shape(slots)?;
        Ok(Self {
            location,
            class,
            acquisition_terms,
            authority,
        })
    }
}

pub(super) struct PhloResourceEncoding<'record, 'data> {
    record: &'record PhloResourceKeyV1<'data>,
    pub(super) size: usize,
    nodes_size: usize,
    count: u32,
}

impl PhloResourceEncoding<'_, '_> {
    pub(super) fn write_to(&self, output: &mut PhloWireEncoder) -> Result<(), PhloWireError> {
        for field in [
            PHLO_RESOURCE_V1_DOMAIN,
            self.record.location,
            &self.record.class.to_be_bytes(),
            self.record.acquisition_terms,
        ] {
            output.bytes(field)?;
        }
        output.u64(u64::try_from(self.nodes_size).map_err(|_| PhloWireError::LengthOutOfRange)?)?;
        output.bytes(&self.count.to_be_bytes())?;
        for node in &self.record.authority {
            let (tag, payload) = node_fields(*node);
            let size = payload
                .len()
                .checked_add(17)
                .ok_or(PhloWireError::LimitExceeded)?;
            output.u64(u64::try_from(size).map_err(|_| PhloWireError::LengthOutOfRange)?)?;
            output.bytes(&[tag])?;
            output.bytes(payload)?;
        }
        Ok(())
    }
}

pub(super) fn checked_field_size(
    payload: usize,
    limits: PhloWireLimits,
) -> Result<usize, PhloWireError> {
    if payload > limits.field_bytes {
        return Err(PhloWireError::LimitExceeded);
    }
    u64::try_from(payload).map_err(|_| PhloWireError::LengthOutOfRange)?;
    payload.checked_add(8).ok_or(PhloWireError::LimitExceeded)
}

fn node_fields(node: PhloAuthorityNode<'_>) -> (u8, &[u8]) {
    match node {
        PhloAuthorityNode::Unit => (0, &[]),
        PhloAuthorityNode::Ground(bytes) => (1, bytes),
        PhloAuthorityNode::Quote(bytes) => (2, bytes),
        PhloAuthorityNode::And => (3, &[]),
    }
}

fn checked_count(count: usize, limits: PhloResourceLimits) -> Result<u32, PhloResourceKeyError> {
    if count == 0 {
        return Err(PhloResourceKeyError::AuthorityShape);
    }
    if count > limits.authority_nodes {
        return Err(PhloResourceKeyError::NodeLimit);
    }
    u32::try_from(count).map_err(|_| PhloResourceKeyError::NodeLimit)
}

fn advance_shape(
    slots: &mut usize,
    node: PhloAuthorityNode<'_>,
) -> Result<(), PhloResourceKeyError> {
    *slots = slots
        .checked_sub(1)
        .and_then(|remaining| match node {
            PhloAuthorityNode::And => remaining.checked_add(2),
            _ => Some(remaining),
        })
        .ok_or(PhloResourceKeyError::AuthorityShape)?;
    Ok(())
}

fn complete_shape(slots: usize) -> Result<(), PhloResourceKeyError> {
    if slots == 0 {
        Ok(())
    } else {
        Err(PhloResourceKeyError::AuthorityShape)
    }
}

fn fixed_field<const N: usize>(
    decoder: &mut PhloWireDecoder<'_>,
) -> Result<[u8; N], PhloResourceKeyError> {
    decoder
        .bytes()?
        .try_into()
        .map_err(|_| PhloResourceKeyError::FieldWidth)
}

#[cfg(test)]
mod tests;
