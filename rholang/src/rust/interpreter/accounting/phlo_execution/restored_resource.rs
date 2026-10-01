use std::mem::size_of;

use super::{
    reserve_key_work, AuthorityNode, HostWorkBudget, HostWorkDimension, PhloExecutionError,
    PhloExecutionLimits, PhloResource, PhloResourceKeyV1, Sig,
};

#[derive(Debug, PartialEq, Eq)]
pub struct RestoredPhloResource<'a> {
    location: &'a [u8],
    class: usize,
    acquisition_terms: &'a [u8],
    authority: Sig,
}

impl<'a> RestoredPhloResource<'a> {
    pub fn from_wire_key(
        key: &PhloResourceKeyV1<'a>,
        limits: PhloExecutionLimits,
        host: &HostWorkBudget,
    ) -> Result<Self, PhloExecutionError> {
        if limits.resource_entries == 0 {
            return Err(PhloExecutionError::TooManyResourceEntries);
        }
        if key.authority.len() > limits.authority_nodes {
            return Err(PhloExecutionError::TooManyAuthorityNodes);
        }
        reserve_key_work(
            Some(host),
            HostWorkDimension::VerificationOperations,
            key.authority.len(),
        )?;
        let mut bytes = key
            .location
            .len()
            .checked_add(key.acquisition_terms.len())
            .filter(|bytes| *bytes <= limits.key_bytes)
            .ok_or(PhloExecutionError::TooManyKeyBytes)?;
        let mut slots = 1_usize;
        for node in &key.authority {
            slots = slots
                .checked_sub(1)
                .and_then(|remaining| match node {
                    AuthorityNode::And => remaining.checked_add(2),
                    _ => Some(remaining),
                })
                .ok_or(PhloExecutionError::MalformedFundingAuthority)?;
            if let AuthorityNode::Ground(payload) | AuthorityNode::Quote(payload) = node {
                bytes = bytes
                    .checked_add(payload.len())
                    .filter(|bytes| *bytes <= limits.key_bytes)
                    .ok_or(PhloExecutionError::TooManyKeyBytes)?;
            }
        }
        if slots != 0 {
            return Err(PhloExecutionError::MalformedFundingAuthority);
        }
        let allocation = key
            .authority
            .len()
            .checked_mul(2 * size_of::<Sig>())
            .and_then(|nodes| nodes.checked_add(bytes))
            .ok_or(PhloExecutionError::ArithmeticOverflow)?;
        reserve_key_work(Some(host), HostWorkDimension::SearchStateBytes, allocation)?;
        reserve_key_work(Some(host), HostWorkDimension::VerificationBytes, bytes)?;
        let mut stack = Vec::new();
        stack
            .try_reserve_exact(key.authority.len())
            .map_err(|_| PhloExecutionError::AllocationFailed)?;
        for node in key.authority.iter().rev() {
            let authority = match node {
                AuthorityNode::Unit => Sig::Unit,
                AuthorityNode::Ground(payload) => Sig::Ground(payload.to_vec()),
                AuthorityNode::Quote(payload) => Sig::Quote(payload.to_vec()),
                AuthorityNode::And => {
                    let left = stack
                        .pop()
                        .ok_or(PhloExecutionError::MalformedFundingAuthority)?;
                    let right = stack
                        .pop()
                        .ok_or(PhloExecutionError::MalformedFundingAuthority)?;
                    Sig::And(Box::new(left), Box::new(right))
                }
            };
            stack.push(authority);
        }
        if stack.len() != 1 {
            return Err(PhloExecutionError::MalformedFundingAuthority);
        }
        Ok(Self {
            location: key.location,
            class: usize::try_from(key.class)
                .map_err(|_| PhloExecutionError::UnknownResourceClass)?,
            acquisition_terms: key.acquisition_terms,
            authority: stack
                .pop()
                .ok_or(PhloExecutionError::MalformedFundingAuthority)?,
        })
    }

    pub fn resource(&self) -> PhloResource<'_> {
        PhloResource {
            location: self.location,
            class: self.class,
            acquisition_terms: self.acquisition_terms,
            authority: &self.authority,
        }
    }
}
