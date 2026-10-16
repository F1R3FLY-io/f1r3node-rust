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

#[cfg(test)]
mod tests;
