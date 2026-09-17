use thiserror::Error;

use super::phlo_resource::{PhloResourceKeyError, PhloResourceKeyV1, PhloResourceLimits};
use super::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireError, PhloWireLimits};

pub const PHLO_SOURCE_V1_DOMAIN: &[u8] = b"f1r3node:phlo-source:v1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloSourcePolicyV1<'a> {
    custody: &'a [u8],
    hold_cap: u64,
    debit_cap: u64,
    fee_permitted: bool,
    resources: Vec<PhloResourceKeyV1<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloSourceLimits {
    pub wire: PhloWireLimits,
    pub resource_permissions: usize,
    pub authority_nodes: usize,
}

impl PhloSourceLimits {
    fn nested(self) -> PhloWireLimits {
        PhloWireLimits {
            total_bytes: self.wire.field_bytes,
            field_bytes: self.wire.field_bytes,
        }
    }

    fn resource(self, remaining: usize) -> PhloResourceLimits {
        PhloResourceLimits {
            wire: self.nested(),
            authority_nodes: remaining,
        }
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloSourceError {
    #[error("unsupported phlo source-policy format domain")]
    FormatDomain,
    #[error("phlo source policy requires a nonempty custody identity")]
    EmptyCustody,
    #[error("phlo source policy exceeds its permission-entry limit")]
    PermissionLimit,
    #[error("phlo source policy exceeds its aggregate authority-node limit")]
    NodeLimit,
    #[error("phlo source policy has a noncanonical field width")]
    FieldWidth,
    #[error("phlo source policy has a noncanonical fee permission")]
    FeePermission,
    #[error("phlo source permissions must have strictly increasing canonical key bytes")]
    NonCanonicalPermissions,
    #[error(transparent)]
    Resource(#[from] PhloResourceKeyError),
    #[error(transparent)]
    Wire(#[from] PhloWireError),
}

impl<'a> PhloSourcePolicyV1<'a> {
    pub fn new(
        custody: &'a [u8],
        hold_cap: u64,
        debit_cap: u64,
        fee_permitted: bool,
        resources: Vec<PhloResourceKeyV1<'a>>,
        limits: PhloSourceLimits,
    ) -> Result<Self, PhloSourceError> {
        check_header(custody, resources.len(), limits)?;
        let mut remaining_nodes = limits.authority_nodes;
        let mut remaining_bytes = limits
            .wire
            .field_bytes
            .checked_sub(12)
            .ok_or(PhloWireError::LimitExceeded)?;
        let mut records = Vec::new();
        for resource in resources {
            remaining_nodes = remaining_nodes
                .checked_sub(resource.authority.len())
                .ok_or(PhloSourceError::NodeLimit)?;
            let bytes = resource.encode(limits.resource(resource.authority.len()))?;
            remaining_bytes = bytes
                .len()
                .checked_add(8)
                .and_then(|size| remaining_bytes.checked_sub(size))
                .ok_or(PhloWireError::LimitExceeded)?;
            records
                .try_reserve(1)
                .map_err(|_| PhloWireError::AllocationFailed)?;
            records.push((bytes, resource));
        }
        records.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        records.dedup_by(|left, right| left.0 == right.0);
        let policy = Self {
            custody,
            hold_cap,
            debit_cap,
            fee_permitted,
            resources: records.into_iter().map(|(_, resource)| resource).collect(),
        };
        policy.encode(limits)?;
        Ok(policy)
    }

    pub fn custody(&self) -> &'a [u8] { self.custody }
    pub fn hold_cap(&self) -> u64 { self.hold_cap }
    pub fn debit_cap(&self) -> u64 { self.debit_cap }
    pub fn fee_permitted(&self) -> bool { self.fee_permitted }
    pub fn resources(&self) -> &[PhloResourceKeyV1<'a>] { &self.resources }

    pub fn encode(&self, limits: PhloSourceLimits) -> Result<Vec<u8>, PhloSourceError> {
        let count = check_header(self.custody, self.resources.len(), limits)?;
        let mut permissions = PhloWireEncoder::new(limits.nested());
        permissions.bytes(&count.to_be_bytes())?;
        let mut remaining_nodes = limits.authority_nodes;
        for resource in &self.resources {
            remaining_nodes = remaining_nodes
                .checked_sub(resource.authority.len())
                .ok_or(PhloSourceError::NodeLimit)?;
            permissions.bytes(&resource.encode(limits.resource(resource.authority.len()))?)?;
        }
        let mut output = PhloWireEncoder::new(limits.wire);
        for field in [
            PHLO_SOURCE_V1_DOMAIN,
            self.custody,
            &self.hold_cap.to_be_bytes(),
            &self.debit_cap.to_be_bytes(),
            &[u8::from(self.fee_permitted)],
            permissions.as_bytes(),
        ] {
            output.bytes(field)?;
        }
        Ok(output.into_bytes())
    }

    pub fn decode(input: &'a [u8], limits: PhloSourceLimits) -> Result<Self, PhloSourceError> {
        let mut fields = PhloWireDecoder::new(input, limits.wire)?;
        if fields.bytes()? != PHLO_SOURCE_V1_DOMAIN {
            return Err(PhloSourceError::FormatDomain);
        }
        let custody = fields.bytes()?;
        let hold_cap = u64::from_be_bytes(fixed_field(&mut fields)?);
        let debit_cap = u64::from_be_bytes(fixed_field(&mut fields)?);
        let fee_permitted = match fixed_field(&mut fields)? {
            [0] => false,
            [1] => true,
            _ => return Err(PhloSourceError::FeePermission),
        };
        let mut permissions = PhloWireDecoder::new(fields.bytes()?, limits.nested())?;
        fields.finish()?;
        let count = usize::try_from(u32::from_be_bytes(fixed_field(&mut permissions)?))
            .map_err(|_| PhloSourceError::PermissionLimit)?;
        check_header(custody, count, limits)?;
        let mut previous = None::<&[u8]>;
        let mut resources = Vec::new();
        let mut remaining_nodes = limits.authority_nodes;
        for _ in 0..count {
            let bytes = permissions.bytes()?;
            if previous.is_some_and(|prior| prior >= bytes) {
                return Err(PhloSourceError::NonCanonicalPermissions);
            }
            let resource = PhloResourceKeyV1::decode(bytes, limits.resource(remaining_nodes))?;
            remaining_nodes = remaining_nodes
                .checked_sub(resource.authority.len())
                .ok_or(PhloSourceError::NodeLimit)?;
            previous = Some(bytes);
            resources
                .try_reserve(1)
                .map_err(|_| PhloWireError::AllocationFailed)?;
            resources.push(resource);
        }
        permissions.finish()?;
        Ok(Self {
            custody,
            hold_cap,
            debit_cap,
            fee_permitted,
            resources,
        })
    }
}

fn check_header(
    custody: &[u8],
    count: usize,
    limits: PhloSourceLimits,
) -> Result<u32, PhloSourceError> {
    if custody.is_empty() {
        return Err(PhloSourceError::EmptyCustody);
    }
    if custody.len() > limits.wire.field_bytes {
        return Err(PhloWireError::LimitExceeded.into());
    }
    if count > limits.resource_permissions {
        return Err(PhloSourceError::PermissionLimit);
    }
    u32::try_from(count).map_err(|_| PhloSourceError::PermissionLimit)
}

fn fixed_field<const N: usize>(
    decoder: &mut PhloWireDecoder<'_>,
) -> Result<[u8; N], PhloSourceError> {
    decoder
        .bytes()?
        .try_into()
        .map_err(|_| PhloSourceError::FieldWidth)
}

#[cfg(test)]
mod tests;
