//! DR-101: the seal of residue that a system body leaves behind.
//!
//! A continuation sealed by system authority alone (genesis, system deploys
//! and earlier residue) runs its body for the deployment that fires it. That
//! deployment pays the body's work, but the data and continuations the body
//! stores in shared state keep a system (Unit) seal bound to the paying
//! deployment. Inside that deployment the seal resolves to the payer's own
//! region, so its charges keep their lanes and COMM counts. Any later
//! deployment sees a Unit region, which has no demand, so no deployment pays
//! for another deployment's system residue.

use std::borrow::Cow;
use std::mem::size_of;

use super::*;

const SYSTEM_RESIDUE_DOMAIN: &[u8] = b"f1r3node:cost-accounted-rho:system-residue:v1";

fn unit_cost_signature() -> CostSignature {
    CostSignature {
        value: Some(CostSignatureValue::Unit(true)),
    }
}

pub fn is_unit_cost_signature(signature: &CostSignature) -> bool {
    matches!(signature.value, Some(CostSignatureValue::Unit(true)))
}

fn is_unit_region(region: &CostRegion) -> bool {
    region
        .signature
        .as_ref()
        .is_some_and(is_unit_cost_signature)
}

/// DR-101: a seal of system authority alone marks a system body.
pub fn is_system_seal(seal: &CostAuthority) -> bool {
    !seal.regions.is_empty() && seal.regions.iter().all(is_unit_region)
}

/// DR-101: the payer and deployment that resolve residue seals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResidueContext {
    payer: CostSignature,
    deploy_id: [u8; 32],
}

impl ResidueContext {
    pub fn new(payer: &Sig, deploy_id: [u8; 32]) -> Result<Self, AuthorityError> {
        Ok(Self {
            payer: sig_to_cost_signature(payer)?,
            deploy_id,
        })
    }

    /// The context of system work. It resolves no seal.
    pub fn system() -> Self {
        Self {
            payer: unit_cost_signature(),
            deploy_id: [0; 32],
        }
    }

    fn resolves(&self) -> bool { !is_unit_cost_signature(&self.payer) }
}

fn residue_preimage(deploy_id: &[u8; 32], payer_region: &[u8]) -> Vec<u8> {
    let mut bytes =
        Vec::with_capacity(SYSTEM_RESIDUE_DOMAIN.len() + deploy_id.len() + payer_region.len());
    bytes.extend_from_slice(SYSTEM_RESIDUE_DOMAIN);
    bytes.extend_from_slice(deploy_id);
    bytes.extend_from_slice(payer_region);
    bytes
}

fn residue_identity(
    deploy_id: &[u8; 32],
    payer_region: &CostRegion,
) -> Result<Vec<u8>, AuthorityError> {
    if payer_region.instance_id.len() != 32 {
        return Err(AuthorityError::InvalidRegionIdentity);
    }
    Ok(Blake2b256::hash(residue_preimage(
        deploy_id,
        &payer_region.instance_id,
    )))
}

/// DR-101: the stored seal of residue that `payer_region` pays for in
/// deployment `deploy_id`. A Unit payer region is already a system region, so
/// genesis and system deploys keep their seals.
pub fn system_residue_region(
    payer_region: &CostRegion,
    deploy_id: &[u8; 32],
) -> Result<CostRegion, AuthorityError> {
    if is_unit_region(payer_region) {
        return Ok(payer_region.clone());
    }
    payer_region
        .signature
        .as_ref()
        .ok_or(AuthorityError::MissingSignature)?;
    Ok(CostRegion {
        instance_id: residue_identity(deploy_id, payer_region)?,
        signature: Some(unit_cost_signature()),
    })
}

/// DR-101: the stored seal of residue whose introduction `introduction` pays.
pub fn system_residue_authority(
    introduction: &CostAuthority,
    deploy_id: &[u8; 32],
) -> Result<CostAuthority, AuthorityError> {
    let regions = introduction
        .regions
        .iter()
        .map(|region| system_residue_region(region, deploy_id))
        .collect::<Result<Vec<_>, _>>()?;
    canonical_authority(&CostAuthority { regions })
}

fn residue_index(seal: &CostAuthority, expected: &[u8]) -> Option<usize> {
    seal.regions
        .iter()
        .position(|region| is_unit_region(region) && region.instance_id == expected)
}

fn with_payer_region(
    seal: &CostAuthority,
    index: usize,
    payer_region: CostRegion,
) -> CostAuthority {
    let mut regions = seal.regions.clone();
    regions[index] = payer_region;
    CostAuthority { regions }
}

/// DR-101: the authority that a stored seal charges in the deployment of
/// `context`. A Unit region that this deployment derived from `entropy`
/// resolves to the payer's region. Every other region stays as stored.
pub fn resolve_system_residue<'a>(
    seal: &'a CostAuthority,
    entropy: &[u8],
    context: &ResidueContext,
) -> Result<Cow<'a, CostAuthority>, AuthorityError> {
    if !context.resolves() || !seal.regions.iter().any(is_unit_region) {
        return Ok(Cow::Borrowed(seal));
    }
    let payer_region = cost_region(&context.payer, entropy, 0)?;
    let expected = residue_identity(&context.deploy_id, &payer_region)?;
    match residue_index(seal, &expected) {
        None => Ok(Cow::Borrowed(seal)),
        Some(index) => {
            canonical_authority(&with_payer_region(seal, index, payer_region)).map(Cow::Owned)
        }
    }
}

/// DR-101: `resolve_system_residue`, reserving the region scans, both hashes
/// and the canonical form before the work runs.
pub fn resolve_system_residue_metered<'a>(
    seal: &'a CostAuthority,
    entropy: &[u8],
    context: &ResidueContext,
    backing: &dyn BackingMeter,
) -> Result<Cow<'a, CostAuthority>, AuthorityError> {
    if !context.resolves() {
        return Ok(Cow::Borrowed(seal));
    }
    backing
        .reserve(
            seal.regions
                .len()
                .checked_add(1)
                .ok_or(AuthorityError::HostWorkRejected)?,
            seal.regions
                .len()
                .checked_mul(size_of::<CostRegion>())
                .ok_or(AuthorityError::HostWorkRejected)?,
            0,
        )
        .map_err(authority_backing_error)?;
    if !seal.regions.iter().any(is_unit_region) {
        return Ok(Cow::Borrowed(seal));
    }
    let payer_region = cost_region_metered(&context.payer, entropy, 0, backing)?;
    let preimage_len = SYSTEM_RESIDUE_DOMAIN
        .len()
        .checked_add(context.deploy_id.len())
        .and_then(|length| length.checked_add(payer_region.instance_id.len()))
        .ok_or(AuthorityError::HostWorkRejected)?;
    backing
        .reserve(
            preimage_len
                .checked_add(1)
                .ok_or(AuthorityError::HostWorkRejected)?,
            preimage_len,
            preimage_len
                .checked_add(32)
                .ok_or(AuthorityError::HostWorkRejected)?,
        )
        .map_err(authority_backing_error)?;
    let expected = residue_identity(&context.deploy_id, &payer_region)?;
    backing
        .reserve(
            seal.regions.len(),
            seal.regions
                .len()
                .checked_mul(expected.len())
                .ok_or(AuthorityError::HostWorkRejected)?,
            0,
        )
        .map_err(authority_backing_error)?;
    let Some(index) = residue_index(seal, &expected) else {
        return Ok(Cow::Borrowed(seal));
    };
    let signature_bytes = seal
        .regions
        .iter()
        .chain(std::iter::once(&payer_region))
        .try_fold(0_usize, |total, region| {
            total.checked_add(region.signature.as_ref().map_or(0, Message::encoded_len))
        })
        .ok_or(AuthorityError::HostWorkRejected)?;
    backing
        .reserve(
            seal.regions.len(),
            0,
            seal.regions
                .len()
                .checked_mul(size_of::<CostRegion>() + 32)
                .and_then(|bytes| bytes.checked_add(signature_bytes))
                .ok_or(AuthorityError::HostWorkRejected)?,
        )
        .map_err(authority_backing_error)?;
    canonical_authority_metered(&with_payer_region(seal, index, payer_region), backing)
        .map(Cow::Owned)
}

#[cfg(test)]
mod tests;
