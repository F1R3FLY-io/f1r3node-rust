use models::rust::phlo_resource::{PhloAuthorityNode, PhloResourceKeyV1, PhloResourceLimits};
use models::rust::phlo_wire::{PhloWireDecoder, PhloWireEncoder, PhloWireLimits};

use super::{invalid, CasperError};
use crate::rust::util::rholang::costacc::genesis_resource_policy::AdoptedResourcePolicy;

const DOMAIN: &[u8] = b"f1r3node:native-prepaid-cell:v1";
const CONTRIBUTION_BYTES: usize = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativePrepaidOrigin {
    pub genesis_root: [u8; 32],
    pub pre_state_root: [u8; 32],
    pub deploy_id: [u8; 32],
    pub birth_source: [u8; 32],
    pub cell_index: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativePrepaidContribution {
    pub custody: [u8; 32],
    pub amount: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativePrepaidCellLimits {
    pub wire: PhloWireLimits,
    pub authority_nodes: usize,
    pub sources: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct NativePrepaidCell<'a> {
    origin: NativePrepaidOrigin,
    resource_bytes: &'a [u8],
    resource: PhloResourceKeyV1<'a>,
    contributions: Vec<NativePrepaidContribution>,
    acquisition_value: u64,
}

impl<'a> NativePrepaidCell<'a> {
    pub fn origin(&self) -> NativePrepaidOrigin { self.origin }

    pub fn resource_bytes(&self) -> &'a [u8] { self.resource_bytes }

    pub fn resource(&self) -> &PhloResourceKeyV1<'a> { &self.resource }

    pub fn contributions(&self) -> &[NativePrepaidContribution] { &self.contributions }

    pub fn acquisition_value(&self) -> u64 { self.acquisition_value }

    pub fn encode(
        policy: &AdoptedResourcePolicy,
        origin: NativePrepaidOrigin,
        resource_bytes: &[u8],
        contributions: &[NativePrepaidContribution],
        limits: NativePrepaidCellLimits,
    ) -> Result<Vec<u8>, CasperError> {
        let size = encoded_size(resource_bytes.len(), contributions.len(), limits)?;
        let (_, acquisition_value) = check_resource(policy, origin, resource_bytes, limits)?;
        check_contributions(contributions, acquisition_value)?;
        let mut out = PhloWireEncoder::with_capacity(limits.wire, size)
            .map_err(|e| invalid(&e.to_string()))?;
        for field in [
            DOMAIN,
            &origin.genesis_root,
            &origin.pre_state_root,
            &origin.deploy_id,
            &origin.birth_source,
        ] {
            out.bytes(field).map_err(|e| invalid(&e.to_string()))?;
        }
        out.u64(origin.cell_index)
            .map_err(|e| invalid(&e.to_string()))?;
        out.bytes(resource_bytes)
            .map_err(|e| invalid(&e.to_string()))?;
        out.u64(u64::try_from(contributions.len()).map_err(|_| invalid("source count overflow"))?)
            .map_err(|e| invalid(&e.to_string()))?;
        for contribution in contributions {
            out.bytes(&contribution.custody)
                .map_err(|e| invalid(&e.to_string()))?;
            out.u64(contribution.amount)
                .map_err(|e| invalid(&e.to_string()))?;
        }
        Ok(out.into_bytes())
    }

    pub fn decode(
        policy: &AdoptedResourcePolicy,
        bytes: &'a [u8],
        limits: NativePrepaidCellLimits,
    ) -> Result<Self, CasperError> {
        let mut input =
            PhloWireDecoder::new(bytes, limits.wire).map_err(|e| invalid(&e.to_string()))?;
        if input.bytes().map_err(|e| invalid(&e.to_string()))? != DOMAIN {
            return Err(invalid("unsupported native cell domain"));
        }
        let origin = NativePrepaidOrigin {
            genesis_root: fixed_identity(&mut input)?,
            pre_state_root: fixed_identity(&mut input)?,
            deploy_id: fixed_identity(&mut input)?,
            birth_source: fixed_identity(&mut input)?,
            cell_index: input.u64().map_err(|e| invalid(&e.to_string()))?,
        };
        let resource_bytes = input.bytes().map_err(|e| invalid(&e.to_string()))?;
        let count = usize::try_from(input.u64().map_err(|e| invalid(&e.to_string()))?)
            .map_err(|_| invalid("source count overflow"))?;
        encoded_size(resource_bytes.len(), count, limits)?;
        if count > input.remaining().len() / CONTRIBUTION_BYTES {
            return Err(invalid("source count exceeds the remaining cell record"));
        }
        let (resource, acquisition_value) = check_resource(policy, origin, resource_bytes, limits)?;
        let mut contributions = Vec::new();
        contributions
            .try_reserve_exact(count)
            .map_err(|_| invalid("contribution allocation failed"))?;
        for _ in 0..count {
            contributions.push(NativePrepaidContribution {
                custody: fixed_identity(&mut input)?,
                amount: input.u64().map_err(|e| invalid(&e.to_string()))?,
            });
        }
        input.finish().map_err(|e| invalid(&e.to_string()))?;
        check_contributions(&contributions, acquisition_value)?;
        Ok(Self {
            origin,
            resource_bytes,
            resource,
            contributions,
            acquisition_value,
        })
    }
}

pub(super) fn encoded_size(
    resource_bytes: usize,
    sources: usize,
    limits: NativePrepaidCellLimits,
) -> Result<usize, CasperError> {
    if sources > limits.sources {
        return Err(invalid("native cell source limit exceeded"));
    }
    if DOMAIN.len() > limits.wire.field_bytes
        || 32 > limits.wire.field_bytes
        || resource_bytes > limits.wire.field_bytes
    {
        return Err(invalid("native cell field limit exceeded"));
    }
    let size = sources
        .checked_mul(CONTRIBUTION_BYTES)
        .and_then(|n| n.checked_add(resource_bytes))
        .and_then(|n| n.checked_add(DOMAIN.len() + 8 + 4 * 40 + 8 + 8 + 8))
        .filter(|n| *n <= limits.wire.total_bytes)
        .ok_or_else(|| invalid("native cell total byte limit exceeded"))?;
    Ok(size)
}

fn fixed_identity(input: &mut PhloWireDecoder<'_>) -> Result<[u8; 32], CasperError> {
    input
        .bytes()
        .map_err(|e| invalid(&e.to_string()))?
        .try_into()
        .map_err(|_| invalid("native cell identity must contain 32 bytes"))
}

fn check_resource<'a>(
    policy: &AdoptedResourcePolicy,
    origin: NativePrepaidOrigin,
    bytes: &'a [u8],
    limits: NativePrepaidCellLimits,
) -> Result<(PhloResourceKeyV1<'a>, u64), CasperError> {
    if origin.genesis_root.as_slice() != policy.genesis().genesis_root().as_ref() {
        return Err(invalid("native cell belongs to another genesis"));
    }
    let resource = PhloResourceKeyV1::decode(bytes, PhloResourceLimits {
        wire: PhloWireLimits {
            total_bytes: limits.wire.field_bytes.min(limits.wire.total_bytes),
            field_bytes: limits.wire.field_bytes,
        },
        authority_nodes: limits.authority_nodes,
    })
    .map_err(|e| invalid(&e.to_string()))?;
    let terms = policy.check_acquisition_terms(resource.acquisition_terms)?;
    let class = usize::try_from(resource.class)
        .ok()
        .and_then(|index| terms.schedule().classes.get(index))
        .ok_or_else(|| invalid("native cell has an unknown resource class"))?;
    let leaves = u64::try_from(
        resource
            .authority
            .iter()
            .filter(|node| {
                matches!(
                    node,
                    PhloAuthorityNode::Ground(_) | PhloAuthorityNode::Quote(_)
                )
            })
            .count(),
    )
    .map_err(|_| invalid("authority occurrence count overflow"))?;
    let acquisition_value = class
        .weight
        .checked_mul(leaves)
        .and_then(|value| value.checked_mul(terms.schedule().actual_price))
        .ok_or_else(|| invalid("cell acquisition value exceeds the unsigned amount range"))?;
    Ok((resource, acquisition_value))
}

fn check_contributions(
    contributions: &[NativePrepaidContribution],
    acquisition_value: u64,
) -> Result<(), CasperError> {
    let mut previous = None;
    let mut total = 0u64;
    for contribution in contributions {
        if contribution.amount == 0
            || i64::try_from(contribution.amount).is_err()
            || previous.is_some_and(|key| key >= contribution.custody)
        {
            return Err(invalid(
                "native cell contributions must fit positive native amounts and be strictly custody-ordered",
            ));
        }
        total = total
            .checked_add(contribution.amount)
            .ok_or_else(|| invalid("cell contribution total overflow"))?;
        previous = Some(contribution.custody);
    }
    if total != acquisition_value {
        return Err(invalid(
            "cell contributions differ from the original acquisition value",
        ));
    }
    Ok(())
}
