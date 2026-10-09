use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{
    CostAuthority, CostRegion, CostSignature, CostSignatureCompound, GPrivate, GUnforgeable, Par,
};
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use models::rust::rholang::sorter::cost_accounting_sorter::{
    sort_signature, sort_signature_metered,
};
use models::rust::rholang::sorter::metered::SorterMeter;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use prost::Message;
use serde::{Deserialize, Serialize};
use shared::rust::clone_backing::{BackingError, BackingMeter};
use shared::rust::collection_backing::tree_backing;
use thiserror::Error;

use super::Sig;
use crate::rust::interpreter::host_work::HostWorkBudget;

mod fallback_metered;
mod monetary;
mod residue;
mod valuation;
pub use fallback_metered::{cost_region_metered, sig_to_cost_signature_metered};
pub use monetary::monetary_funding_signatures_with_host_work;
pub use residue::{
    is_system_seal, is_unit_cost_signature, resolve_system_residue, resolve_system_residue_metered,
    system_residue_authority, system_residue_region, ResidueContext,
};
pub use valuation::AuthorityResourceDemand;

const CERTIFICATE_DOMAIN: &[u8] = b"f1r3node:authority-funding-certificate:v9";
const WITNESS_DOMAIN: &[u8] = b"f1r3node:authority-cost-witness:v9";
pub const AUTHORITY_ACCOUNTING_PROTOCOL_VERSION: u32 = 9;
const REGION_DOMAIN: &[u8] = b"f1r3node:cost-accounted-rho:region:v1";
const REGION_OCCURRENCE_DOMAIN: &[u8] = b"f1r3node:cost-accounted-rho:region-occurrence:v1";
const STACK_TRANSFER_EVENT_DOMAIN: &[u8] = b"f1r3node:cost-accounted-rho:stack-transfer-event:v1";

pub fn stack_transfer_event_id(produce_hash: &[u8; 32], cell_index: u64) -> [u8; 32] {
    let mut bytes = Vec::with_capacity(
        STACK_TRANSFER_EVENT_DOMAIN.len() + produce_hash.len() + std::mem::size_of::<u64>(),
    );
    bytes.extend_from_slice(STACK_TRANSFER_EVENT_DOMAIN);
    bytes.extend_from_slice(produce_hash);
    bytes.extend_from_slice(&cell_index.to_le_bytes());
    Blake2b256::hash(bytes)
        .try_into()
        .expect("Blake2b-256 digest length")
}

pub fn stack_transfer_event_id_metered(
    produce_hash: &[u8; 32],
    cell_index: u64,
    backing: &dyn BackingMeter,
) -> Result<[u8; 32], AuthorityError> {
    reserve_stack_transfer_event_id(backing)?;
    let capacity = STACK_TRANSFER_EVENT_DOMAIN
        .len()
        .checked_add(produce_hash.len())
        .and_then(|len| len.checked_add(std::mem::size_of::<u64>()))
        .ok_or(AuthorityError::HostWorkRejected)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| AuthorityError::HostWorkRejected)?;
    bytes.extend_from_slice(STACK_TRANSFER_EVENT_DOMAIN);
    bytes.extend_from_slice(produce_hash);
    bytes.extend_from_slice(&cell_index.to_le_bytes());
    Blake2b256::hash(bytes)
        .try_into()
        .map_err(|_| AuthorityError::HostWorkRejected)
}

pub fn reserve_stack_transfer_event_id(backing: &dyn BackingMeter) -> Result<(), AuthorityError> {
    let meter = SorterMeter::new(backing);
    let capacity = STACK_TRANSFER_EVENT_DOMAIN
        .len()
        .checked_add(32)
        .and_then(|len| len.checked_add(std::mem::size_of::<u64>()))
        .ok_or(AuthorityError::HostWorkRejected)?;
    meter
        .reserve(1, capacity, 0)
        .map_err(authority_backing_error)?;
    meter
        .reserve(capacity, 0, capacity)
        .map_err(authority_backing_error)?;
    meter
        .reserve(
            capacity
                .checked_add(2)
                .ok_or(AuthorityError::HostWorkRejected)?,
            capacity
                .checked_add(32)
                .ok_or(AuthorityError::HostWorkRejected)?,
            32,
        )
        .map_err(authority_backing_error)
}

pub fn canonical_cost_signature(
    signature: &CostSignature,
) -> Result<CostSignature, AuthorityError> {
    let canonical = sort_signature(signature).term;
    validate_cost_signature(&canonical)?;
    if &canonical != signature {
        return Err(AuthorityError::NonCanonicalSignature);
    }
    Ok(canonical)
}

fn validate_cost_signature(signature: &CostSignature) -> Result<(), AuthorityError> {
    match signature.value.as_ref() {
        Some(CostSignatureValue::Ground(_)) | Some(CostSignatureValue::Unit(true)) => Ok(()),
        Some(CostSignatureValue::Unit(false)) => Err(AuthorityError::NonCanonicalSignature),
        Some(CostSignatureValue::BoundLevel(_)) => Err(AuthorityError::UnresolvedBoundLevel),
        Some(CostSignatureValue::Quote(par)) | Some(CostSignatureValue::Name(par)) => {
            if ParSortMatcher::sort_match(par).term == *par {
                Ok(())
            } else {
                Err(AuthorityError::NonCanonicalSignature)
            }
        }
        Some(CostSignatureValue::Compound(compound)) if compound.elements.len() >= 2 => compound
            .elements
            .iter()
            .try_for_each(validate_cost_signature),
        Some(CostSignatureValue::Compound(_)) => Err(AuthorityError::MalformedCompound),
        None => Err(AuthorityError::MissingSignature),
    }
}

pub fn canonical_cost_signature_metered(
    signature: &CostSignature,
    backing: &dyn BackingMeter,
) -> Result<CostSignature, AuthorityError> {
    let meter = SorterMeter::new(backing);
    let canonical = sort_signature_metered(signature, &meter)
        .map_err(authority_backing_error)?
        .term;
    validate_cost_signature_metered(&canonical, &meter)?;
    // Changed by D-O1 (DR-110): a block inspection prepays one traversal. The
    // comparison below reads `canonical`, and the surplus of the per-level
    // charge was the only payment for its release. The second inspection
    // prepays that release where the signature is born, on every path. Its
    // consumers then pay no release for the signature or its parts (Rule B).
    // meter.inspect(&canonical).map_err(authority_backing_error)?;
    for _ in 0..2 {
        meter
            .inspect_blocks(&canonical)
            .map_err(authority_backing_error)?;
    }
    // Changed by D-O1 (DR-94): block accounting charges inline bytes once per enclosing block.
    // meter.inspect(signature).map_err(authority_backing_error)?;
    meter
        .inspect_blocks(signature)
        .map_err(authority_backing_error)?;
    if &canonical != signature {
        return Err(AuthorityError::NonCanonicalSignature);
    }
    Ok(canonical)
}

fn validate_cost_signature_metered(
    signature: &CostSignature,
    meter: &SorterMeter<'_>,
) -> Result<(), AuthorityError> {
    let mut pending = Vec::new();
    meter
        .push(&mut pending, signature)
        .map_err(authority_backing_error)?;
    while let Some(signature) = pending.pop() {
        meter
            .reserve(1, std::mem::size_of::<CostSignature>(), 0)
            .map_err(authority_backing_error)?;
        match signature.value.as_ref() {
            Some(CostSignatureValue::Ground(_)) | Some(CostSignatureValue::Unit(true)) => {}
            Some(CostSignatureValue::Unit(false)) => {
                return Err(AuthorityError::NonCanonicalSignature)
            }
            Some(CostSignatureValue::BoundLevel(_)) => {
                return Err(AuthorityError::UnresolvedBoundLevel)
            }
            Some(CostSignatureValue::Quote(par)) | Some(CostSignatureValue::Name(par)) => {
                let sorted = ParSortMatcher::sort_match_metered(par, meter)
                    .map_err(authority_backing_error)?;
                // Changed by D-O1 (DR-94): block accounting charges inline bytes once per enclosing block.
                // meter.inspect(par).map_err(authority_backing_error)?;
                meter.inspect_blocks(par).map_err(authority_backing_error)?;
                // Changed by D-O1 (DR-110): the owned `sorted` is compared and
                // then dropped at the end of this arm. A block inspection
                // prepays one traversal, so the comparison and the release
                // each get one.
                // Was (D-O1, DR-94): kept legacy because the per-level charge
                // also paid that release.
                // meter
                //     .inspect(&sorted.term)
                //     .map_err(authority_backing_error)?;
                for _ in 0..2 {
                    meter
                        .inspect_blocks(&sorted.term)
                        .map_err(authority_backing_error)?;
                }
                if sorted.term != *par {
                    return Err(AuthorityError::NonCanonicalSignature);
                }
            }
            Some(CostSignatureValue::Compound(compound)) if compound.elements.len() >= 2 => {
                for element in compound.elements.iter().rev() {
                    meter
                        .push(&mut pending, element)
                        .map_err(authority_backing_error)?;
                }
            }
            Some(CostSignatureValue::Compound(_)) => return Err(AuthorityError::MalformedCompound),
            None => return Err(AuthorityError::MissingSignature),
        }
    }
    Ok(())
}

fn authority_backing_error(_: BackingError) -> AuthorityError { AuthorityError::HostWorkRejected }

pub fn cost_signature_to_sig(signature: &CostSignature) -> Result<Sig, AuthorityError> {
    let canonical = canonical_cost_signature(signature)?;
    match canonical.value {
        Some(CostSignatureValue::Ground(bytes)) => Ok(Sig::Ground(bytes)),
        Some(CostSignatureValue::Unit(true)) => Ok(Sig::Unit),
        Some(CostSignatureValue::Unit(false)) => Err(AuthorityError::NonCanonicalSignature),
        Some(CostSignatureValue::Quote(par)) => Ok(Sig::Quote(par.encode_to_vec())),
        Some(CostSignatureValue::Name(par)) => Ok(Sig::Ground(par.encode_to_vec())),
        Some(CostSignatureValue::Compound(compound)) => {
            let mut elements = compound
                .elements
                .iter()
                .map(cost_signature_to_sig)
                .collect::<Result<Vec<_>, _>>()?;
            if elements.len() < 2 {
                return Err(AuthorityError::MalformedCompound);
            }
            while elements.len() > 1 {
                let mut next = Vec::with_capacity(elements.len().div_ceil(2));
                let mut pairs = elements.into_iter();
                while let Some(left) = pairs.next() {
                    match pairs.next() {
                        Some(right) => next.push(Sig::And(Box::new(left), Box::new(right))),
                        None => next.push(left),
                    }
                }
                elements = next;
            }
            elements.pop().ok_or(AuthorityError::MalformedCompound)
        }
        Some(CostSignatureValue::BoundLevel(_)) => Err(AuthorityError::UnresolvedBoundLevel),
        None => Err(AuthorityError::MissingSignature),
    }
}

pub fn cost_signature_to_sig_metered(
    signature: &CostSignature,
    backing: &dyn BackingMeter,
) -> Result<Sig, AuthorityError> {
    let cleanup = |operations: usize, scanned: usize, bytes: usize| {
        backing.reserve(
            operations.checked_mul(2).ok_or(BackingError::Overflow)?,
            scanned,
            bytes,
        )
    };
    let canonical = canonical_cost_signature_metered(signature, &cleanup)?;
    let meter = SorterMeter::new(&cleanup);
    match canonical.value {
        Some(CostSignatureValue::Compound(compound)) => {
            if compound.elements.len() < 2 {
                return Err(AuthorityError::MalformedCompound);
            }
            let mut elements = meter
                .vec::<Sig>(compound.elements.len())
                .map_err(authority_backing_error)?;
            for element in compound.elements {
                elements.push(cost_atom_to_sig_metered(element, &meter)?);
            }
            while elements.len() > 1 {
                let mut next = meter
                    .vec::<Sig>(elements.len().div_ceil(2))
                    .map_err(authority_backing_error)?;
                let mut pairs = elements.into_iter();
                while let Some(left) = pairs.next() {
                    match pairs.next() {
                        Some(right) => {
                            meter
                                .reserve(
                                    4,
                                    2 * std::mem::size_of::<Sig>(),
                                    2 * std::mem::size_of::<Sig>(),
                                )
                                .map_err(authority_backing_error)?;
                            next.push(Sig::And(Box::new(left), Box::new(right)));
                        }
                        None => next.push(left),
                    }
                }
                elements = next;
            }
            elements.pop().ok_or(AuthorityError::MalformedCompound)
        }
        _ => cost_atom_to_sig_metered(canonical, &meter),
    }
}

fn cost_atom_to_sig_metered(
    signature: CostSignature,
    meter: &SorterMeter<'_>,
) -> Result<Sig, AuthorityError> {
    meter
        .reserve(1, std::mem::size_of::<CostSignature>(), 0)
        .map_err(authority_backing_error)?;
    match signature.value {
        Some(CostSignatureValue::Ground(bytes)) => Ok(Sig::Ground(bytes)),
        Some(CostSignatureValue::Unit(true)) => Ok(Sig::Unit),
        Some(CostSignatureValue::Quote(par)) => {
            // Changed by D-O1 (DR-110): the inspection prepays the length
            // computation, as at the sibling encode sites (DR-94). The nested
            // encode reservation prepays the encode, and the release of `par`,
            // a part of a canonical signature, was prepaid at its birth
            // (Rule B).
            // meter.inspect(&par).map_err(authority_backing_error)?;
            meter
                .inspect_blocks(&par)
                .map_err(authority_backing_error)?;
            let encoded_len = par.encoded_len();
            let mut bytes = meter
                .vec::<u8>(encoded_len)
                .map_err(authority_backing_error)?;
            meter
                .nested_encode(&par, encoded_len)
                .map_err(authority_backing_error)?;
            par.encode(&mut bytes)
                .map_err(|_| AuthorityError::HostWorkRejected)?;
            Ok(Sig::Quote(bytes))
        }
        Some(CostSignatureValue::Name(par)) => {
            // Changed by D-O1 (DR-110): as in the `Quote` arm above.
            // meter.inspect(&par).map_err(authority_backing_error)?;
            meter
                .inspect_blocks(&par)
                .map_err(authority_backing_error)?;
            let encoded_len = par.encoded_len();
            let mut bytes = meter
                .vec::<u8>(encoded_len)
                .map_err(authority_backing_error)?;
            meter
                .nested_encode(&par, encoded_len)
                .map_err(authority_backing_error)?;
            par.encode(&mut bytes)
                .map_err(|_| AuthorityError::HostWorkRejected)?;
            Ok(Sig::Ground(bytes))
        }
        Some(CostSignatureValue::Unit(false)) => Err(AuthorityError::NonCanonicalSignature),
        Some(CostSignatureValue::BoundLevel(_)) => Err(AuthorityError::UnresolvedBoundLevel),
        Some(CostSignatureValue::Compound(_)) => Err(AuthorityError::MalformedCompound),
        None => Err(AuthorityError::MissingSignature),
    }
}

pub fn sig_to_cost_signature(signature: &Sig) -> Result<CostSignature, AuthorityError> {
    match signature {
        Sig::Unit => Ok(CostSignature {
            value: Some(CostSignatureValue::Unit(true)),
        }),
        Sig::Ground(bytes) | Sig::Quote(bytes) => Ok(CostSignature {
            value: Some(CostSignatureValue::Ground(bytes.clone())),
        }),
        Sig::And(left, right) => compound_cost_signatures(
            &sig_to_cost_signature(left)?,
            &sig_to_cost_signature(right)?,
        ),
        _ => Err(AuthorityError::UnsupportedFundingSignature),
    }
}

pub fn compound_cost_signatures(
    left: &CostSignature,
    right: &CostSignature,
) -> Result<CostSignature, AuthorityError> {
    let signature = sort_signature(&CostSignature {
        value: Some(CostSignatureValue::Compound(CostSignatureCompound {
            elements: vec![
                canonical_cost_signature(left)?,
                canonical_cost_signature(right)?,
            ],
        })),
    })
    .term;
    validate_cost_signature(&signature)?;
    Ok(signature)
}

pub fn cost_region(
    signature: &CostSignature,
    entropy: &[u8],
    discriminator: u32,
) -> Result<CostRegion, AuthorityError> {
    let signature = canonical_cost_signature(signature)?;
    let signature_bytes = signature.encode_to_vec();
    let mut bytes = Vec::with_capacity(
        REGION_DOMAIN.len() + entropy.len() + signature_bytes.len() + std::mem::size_of::<u32>(),
    );
    bytes.extend_from_slice(REGION_DOMAIN);
    bytes.extend_from_slice(&(entropy.len() as u64).to_le_bytes());
    bytes.extend_from_slice(entropy);
    bytes.extend_from_slice(&discriminator.to_le_bytes());
    bytes.extend_from_slice(&(signature_bytes.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&signature_bytes);
    Ok(CostRegion {
        instance_id: Blake2b256::hash(bytes),
        signature: Some(signature),
    })
}

/// Added by D-F1 (DR-117): an authority that canonicalization produced. Its
/// regions have 32-byte identities in strictly ascending order, and each
/// signature is canonical, so a new canonicalization returns it unchanged.
/// Only this module constructs it, through canonicalization or through a
/// rekey of a witness. A holder therefore never validates its regions again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalAuthority(CostAuthority);

impl CanonicalAuthority {
    #[cfg(test)]
    pub(crate) fn as_authority(&self) -> &CostAuthority { &self.0 }

    pub(crate) fn into_authority(self) -> CostAuthority { self.0 }

    pub(crate) fn is_empty(&self) -> bool { self.0.regions.is_empty() }
}

// Changed by D-F1 (DR-117): the body moved, unchanged, into
// `canonical_regions`, which reads borrowed regions and returns the witness.
pub fn canonical_authority(authority: &CostAuthority) -> Result<CostAuthority, AuthorityError> {
    canonical_regions(&authority.regions).map(CanonicalAuthority::into_authority)
}

/// Added by D-F1 (DR-117): `canonical_authority` over borrowed regions, in
/// their order. The value and the first error are those of
/// `canonical_authority` of an authority with these regions.
pub(crate) fn canonical_regions<'a>(
    input: impl IntoIterator<Item = &'a CostRegion>,
) -> Result<CanonicalAuthority, AuthorityError> {
    let mut regions = BTreeMap::<Vec<u8>, CostSignature>::new();
    for region in input {
        if region.instance_id.len() != 32 {
            return Err(AuthorityError::InvalidRegionIdentity);
        }
        let signature = canonical_cost_signature(
            region
                .signature
                .as_ref()
                .ok_or(AuthorityError::MissingSignature)?,
        )?;
        match regions.get(&region.instance_id) {
            Some(existing) if existing != &signature => {
                return Err(AuthorityError::RegionIdentityConflict)
            }
            Some(_) => {}
            None => {
                regions.insert(region.instance_id.clone(), signature);
            }
        }
    }
    Ok(CanonicalAuthority(CostAuthority {
        regions: regions
            .into_iter()
            .map(|(instance_id, signature)| CostRegion {
                instance_id,
                signature: Some(signature),
            })
            .collect(),
    }))
}

// Changed by D-F1 (DR-117): the body moved, unchanged, into
// `canonical_regions_metered`, which reads borrowed regions and returns the
// witness. This entry point keeps its value, its errors and its charges.
pub fn canonical_authority_metered(
    authority: &CostAuthority,
    backing: &dyn BackingMeter,
) -> Result<CostAuthority, AuthorityError> {
    canonical_regions_metered(&authority.regions, backing).map(CanonicalAuthority::into_authority)
}

/// Added by D-F1 (DR-117): `canonical_authority_metered` over borrowed
/// regions, in their order, with the same value, errors and charges.
pub(crate) fn canonical_regions_metered<'a>(
    input: impl IntoIterator<Item = &'a CostRegion>,
    backing: &dyn BackingMeter,
) -> Result<CanonicalAuthority, AuthorityError> {
    let meter = SorterMeter::new(backing);
    let mut regions = BTreeMap::<Vec<u8>, CostSignature>::new();
    for region in input {
        meter
            .reserve(1, std::mem::size_of::<CostRegion>(), 0)
            .map_err(authority_backing_error)?;
        if region.instance_id.len() != 32 {
            return Err(AuthorityError::InvalidRegionIdentity);
        }
        let signature = canonical_cost_signature_metered(
            region
                .signature
                .as_ref()
                .ok_or(AuthorityError::MissingSignature)?,
            backing,
        )?;
        let comparisons = regions
            .len()
            .checked_add(1)
            .ok_or(AuthorityError::HostWorkRejected)?;
        let scanned = comparisons
            .checked_mul(32)
            .ok_or(AuthorityError::HostWorkRejected)?;
        meter
            .reserve(comparisons, scanned, 0)
            .map_err(authority_backing_error)?;
        match regions.get(&region.instance_id) {
            Some(existing) => {
                // Changed by D-O1 (DR-94): block accounting charges inline bytes once per enclosing block.
                // meter.inspect(existing).map_err(authority_backing_error)?;
                meter
                    .inspect_blocks(existing)
                    .map_err(authority_backing_error)?;
                // Changed by D-O1 (DR-110): the owned `signature` is canonical,
                // so its release was prepaid at its birth (Rule B). The block
                // inspection prepays the comparison.
                // Was (D-O1, DR-94): kept legacy because the per-level charge
                // also paid that release.
                // meter.inspect(&signature).map_err(authority_backing_error)?;
                meter
                    .inspect_blocks(&signature)
                    .map_err(authority_backing_error)?;
                if existing != &signature {
                    return Err(AuthorityError::RegionIdentityConflict);
                }
            }
            None => {
                reserve_authority_tree_insert::<Vec<u8>, CostSignature>(&meter, regions.len())?;
                // Changed by D-O1 (DR-94): block accounting charges inline bytes once per enclosing block.
                // let instance_id = meter
                //     .clone(&region.instance_id)
                //     .map_err(authority_backing_error)?;
                let instance_id = meter
                    .clone_blocks(&region.instance_id)
                    .map_err(authority_backing_error)?;
                regions.insert(instance_id, signature);
            }
        }
    }
    let mut sorted = meter
        .vec::<CostRegion>(regions.len())
        .map_err(authority_backing_error)?;
    for (instance_id, signature) in regions {
        sorted.push(CostRegion {
            instance_id,
            signature: Some(signature),
        });
    }
    Ok(CanonicalAuthority(CostAuthority { regions: sorted }))
}

fn reserve_authority_tree_insert<K, V>(
    meter: &SorterMeter<'_>,
    entries: usize,
) -> Result<(), AuthorityError> {
    let next_len = entries
        .checked_add(1)
        .ok_or(AuthorityError::HostWorkRejected)?;
    let (next_ops, next_bytes) =
        tree_backing::<K, V>(next_len).ok_or(AuthorityError::HostWorkRejected)?;
    let (prior_ops, prior_bytes) =
        tree_backing::<K, V>(entries).ok_or(AuthorityError::HostWorkRejected)?;
    let comparisons = next_len;
    let scanned = comparisons
        .checked_mul(32)
        .ok_or(AuthorityError::HostWorkRejected)?;
    meter
        .reserve(
            next_ops
                .checked_sub(prior_ops)
                .ok_or(AuthorityError::HostWorkRejected)?
                .checked_add(comparisons)
                .ok_or(AuthorityError::HostWorkRejected)?,
            scanned,
            next_bytes
                .checked_sub(prior_bytes)
                .ok_or(AuthorityError::HostWorkRejected)?,
        )
        .map_err(authority_backing_error)
}

pub fn merge_authorities_metered<'a, I>(
    authorities: I,
    backing: &dyn BackingMeter,
) -> Result<CostAuthority, AuthorityError>
where
    I: IntoIterator<Item = &'a CostAuthority>,
    I::IntoIter: Clone,
{
    // Changed by D-F1 (DR-117): the copy was only the input of the
    // canonicalization, which reads the borrowed regions in the same order.
    // Nothing read the copy afterwards. The copying body is kept, unchanged,
    // as the test oracle `merge_authorities_metered_legacy`.
    merge_canonical_metered(authorities, backing).map(CanonicalAuthority::into_authority)
}

/// Added by D-F1 (DR-117): the canonical merge of `authorities`, read in
/// place. It reserves one unit for each authority, as the copying merge did,
/// and canonicalizes the borrowed regions in participant order. The copying
/// merge passed the same regions in the same order to the same
/// canonicalization, so the value and the first error are the same.
pub(crate) fn merge_canonical_metered<'a, I>(
    authorities: I,
    backing: &dyn BackingMeter,
) -> Result<CanonicalAuthority, AuthorityError>
where
    I: IntoIterator<Item = &'a CostAuthority>,
    I::IntoIter: Clone,
{
    let authorities = authorities.into_iter();
    let meter = SorterMeter::new(backing);
    for _ in authorities.clone() {
        meter
            .reserve(1, std::mem::size_of::<CostAuthority>(), 0)
            .map_err(authority_backing_error)?;
    }
    canonical_regions_metered(
        authorities.flat_map(|authority| authority.regions.iter()),
        backing,
    )
}

/// D-F1 (DR-117): the copying merge that `merge_authorities_metered` ran
/// before, kept as the test oracle of the in-place merge.
#[cfg(test)]
pub(crate) fn merge_authorities_metered_legacy<'a, I>(
    authorities: I,
    backing: &dyn BackingMeter,
) -> Result<CostAuthority, AuthorityError>
where
    I: IntoIterator<Item = &'a CostAuthority>,
{
    let meter = SorterMeter::new(backing);
    let mut merged = CostAuthority::default();
    for authority in authorities {
        meter
            .reserve(1, std::mem::size_of::<CostAuthority>(), 0)
            .map_err(authority_backing_error)?;
        for region in &authority.regions {
            // Disabled by D-O1 (DR-94): nothing reads `region` before the copy
            // below, whose copy-and-cleanup reservation prepays its own reads
            // and the release of the copy.
            // meter.inspect(region).map_err(authority_backing_error)?;
            // Changed by D-O1 (DR-94): block accounting charges inline bytes once per enclosing block.
            // let copied = meter.clone(region).map_err(authority_backing_error)?;
            let copied = meter
                .clone_blocks(region)
                .map_err(authority_backing_error)?;
            meter
                .push(&mut merged.regions, copied)
                .map_err(authority_backing_error)?;
        }
    }
    canonical_authority_metered(&merged, backing)
}

pub fn merge_authorities<'a, I>(authorities: I) -> Result<CostAuthority, AuthorityError>
where I: IntoIterator<Item = &'a CostAuthority> {
    let mut merged = CostAuthority::default();
    for authority in authorities {
        merged.regions.extend(authority.regions.iter().cloned());
    }
    canonical_authority(&merged)
}

pub fn extend_authority(
    authority: &CostAuthority,
    region: CostRegion,
) -> Result<CostAuthority, AuthorityError> {
    let mut extended = authority.clone();
    extended.regions.push(region);
    canonical_authority(&extended)
}

pub fn authority_demand(
    authority: &CostAuthority,
) -> Result<ResourceMultiset<[u8; 32]>, AuthorityError> {
    let regions = authority_regions(authority)?;
    let mut demand = ResourceMultiset::default();
    for signature in regions.values() {
        let signature = cost_signature_to_sig(signature)?;
        if signature == Sig::Unit {
            continue;
        }
        demand = demand.checked_add(&ResourceMultiset::singleton(signature.lane_hash(), 1))?;
    }
    Ok(demand)
}

/// Added by D-F1 (DR-117): the demand of a witness. The regions are canonical
/// and in identity order, so the lanes come in the order and from the
/// signatures that `authority_demand` reads after its canonicalization.
pub(crate) fn authority_demand_from_canonical(
    witness: &CanonicalAuthority,
) -> Result<ResourceMultiset<[u8; 32]>, AuthorityError> {
    let mut demand = ResourceMultiset::default();
    for region in &witness.0.regions {
        let signature = cost_signature_to_sig(
            region
                .signature
                .as_ref()
                .ok_or(AuthorityError::MissingSignature)?,
        )?;
        if signature == Sig::Unit {
            continue;
        }
        demand = demand.checked_add(&ResourceMultiset::singleton(signature.lane_hash(), 1))?;
    }
    Ok(demand)
}

/// Added by D-F1 (DR-117): `authority_demand_from_canonical` with charges. It
/// reserves the read of each region as `authority_regions_metered` did, and
/// it charges each lane and increment as `authority_demand_metered` does.
/// The canonicalization and the identity map are not rebuilt.
pub(crate) fn authority_demand_from_canonical_metered(
    witness: &CanonicalAuthority,
    backing: &dyn BackingMeter,
) -> Result<ResourceMultiset<[u8; 32]>, AuthorityError> {
    let meter = SorterMeter::new(backing);
    let mut demand = ResourceMultiset::default();
    for region in &witness.0.regions {
        meter
            .reserve(1, std::mem::size_of::<CostRegion>(), 0)
            .map_err(authority_backing_error)?;
        let signature = region
            .signature
            .as_ref()
            .ok_or(AuthorityError::MissingSignature)?;
        if let Some(lane) = cost_signature_lane_metered(signature, &meter)? {
            demand.increment_metered(lane, 1, &meter)?;
        }
    }
    Ok(demand)
}

pub fn authority_demand_metered(
    authority: &CostAuthority,
    backing: &dyn BackingMeter,
) -> Result<ResourceMultiset<[u8; 32]>, AuthorityError> {
    let regions = authority_regions_metered(authority, backing)?;
    let meter = SorterMeter::new(backing);
    let mut demand = ResourceMultiset::default();
    for signature in regions.values() {
        if let Some(lane) = cost_signature_lane_metered(signature, &meter)? {
            demand.increment_metered(lane, 1, &meter)?;
        }
    }
    Ok(demand)
}

fn cost_signature_lane_metered(
    signature: &CostSignature,
    meter: &SorterMeter<'_>,
) -> Result<Option<[u8; 32]>, AuthorityError> {
    let mut pending = Vec::new();
    meter
        .push(&mut pending, signature)
        .map_err(authority_backing_error)?;
    let mut channel = Par::default();
    while let Some(signature) = pending.pop() {
        meter
            .reserve(1, std::mem::size_of::<CostSignature>(), 0)
            .map_err(authority_backing_error)?;
        match signature.value.as_ref() {
            Some(CostSignatureValue::Unit(true)) => {}
            Some(CostSignatureValue::Ground(bytes)) => {
                append_signature_channel_atom_metered(bytes, &mut channel, meter)?;
            }
            Some(CostSignatureValue::Quote(par)) | Some(CostSignatureValue::Name(par)) => {
                // Changed by D-O1 (DR-94): block accounting charges inline bytes
                // once per enclosing block. `par` is borrowed, and the
                // inspection prepays only the length computation below.
                // meter.inspect(par).map_err(authority_backing_error)?;
                meter.inspect_blocks(par).map_err(authority_backing_error)?;
                let encoded_len = par.encoded_len();
                let mut bytes = meter
                    .vec::<u8>(encoded_len)
                    .map_err(authority_backing_error)?;
                meter
                    .nested_encode(par, encoded_len)
                    .map_err(authority_backing_error)?;
                par.encode(&mut bytes)
                    .map_err(|_| AuthorityError::HostWorkRejected)?;
                append_signature_channel_atom_metered(&bytes, &mut channel, meter)?;
            }
            Some(CostSignatureValue::Compound(compound)) if compound.elements.len() >= 2 => {
                for element in compound.elements.iter().rev() {
                    meter
                        .push(&mut pending, element)
                        .map_err(authority_backing_error)?;
                }
            }
            Some(CostSignatureValue::BoundLevel(_)) => {
                return Err(AuthorityError::UnresolvedBoundLevel);
            }
            Some(CostSignatureValue::Unit(false)) => {
                return Err(AuthorityError::NonCanonicalSignature);
            }
            Some(CostSignatureValue::Compound(_)) => {
                return Err(AuthorityError::MalformedCompound);
            }
            None => return Err(AuthorityError::MissingSignature),
        }
    }
    if channel.unforgeables.is_empty() {
        return Ok(None);
    }
    let channel = ParSortMatcher::sort_match_metered(&channel, meter)
        .map_err(authority_backing_error)?
        .term;
    // Changed by D-O1 (DR-110): a block inspection prepays one traversal. The
    // first prepays the length computation, as at the sibling encode sites
    // (DR-94). The second prepays the release of the sorted channel. The third
    // (added by D-E3) prepays the release of the shadowed unsorted channel,
    // which also lives until the return.
    // meter.inspect(&channel).map_err(authority_backing_error)?;
    for _ in 0..3 {
        meter
            .inspect_blocks(&channel)
            .map_err(authority_backing_error)?;
    }
    let encoded_len = channel.encoded_len();
    let mut encoded = meter
        .vec::<u8>(encoded_len)
        .map_err(authority_backing_error)?;
    meter
        .nested_encode(&channel, encoded_len)
        .map_err(authority_backing_error)?;
    channel
        .encode(&mut encoded)
        .map_err(|_| AuthorityError::HostWorkRejected)?;
    let scanned = super::SIGNATURE_LANE_DOMAIN
        .len()
        .checked_add(encoded.len())
        .ok_or(AuthorityError::HostWorkRejected)?;
    meter
        .reserve(1, scanned, 32)
        .map_err(authority_backing_error)?;
    let hash = Blake2b256::hash_parts([super::SIGNATURE_LANE_DOMAIN, encoded.as_slice()]);
    Ok(Some(
        hash.as_slice()
            .try_into()
            .map_err(|_| AuthorityError::HostWorkRejected)?,
    ))
}

fn append_signature_channel_atom_metered(
    bytes: &[u8],
    channel: &mut Par,
    meter: &SorterMeter<'_>,
) -> Result<(), AuthorityError> {
    meter.reserve(2, 32, 0).map_err(authority_backing_error)?;
    meter
        .reserve(1, bytes.len(), 32)
        .map_err(authority_backing_error)?;
    let id = Blake2b256::hash_parts([bytes]);
    meter
        .push(&mut channel.unforgeables, GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
        })
        .map_err(authority_backing_error)
}

pub fn funding_sig_channel_metered(
    signature: &Sig,
    backing: &dyn BackingMeter,
) -> Result<Par, AuthorityError> {
    let meter = SorterMeter::new(backing);
    let mut pending = Vec::new();
    meter
        .push(&mut pending, signature)
        .map_err(authority_backing_error)?;
    let mut channel = Par::default();
    while let Some(signature) = pending.pop() {
        meter
            .reserve(1, std::mem::size_of::<Sig>(), 0)
            .map_err(authority_backing_error)?;
        match signature {
            Sig::Unit => {}
            Sig::Ground(bytes) | Sig::Quote(bytes) => {
                append_signature_channel_atom_metered(bytes, &mut channel, &meter)?;
            }
            Sig::And(left, right) => {
                meter
                    .push(&mut pending, right.as_ref())
                    .map_err(authority_backing_error)?;
                meter
                    .push(&mut pending, left.as_ref())
                    .map_err(authority_backing_error)?;
            }
            _ => return Err(AuthorityError::UnsupportedFundingSignature),
        }
    }
    if channel.unforgeables.len() < 2 {
        return Ok(channel);
    }
    let sorted =
        ParSortMatcher::sort_match_metered(&channel, &meter).map_err(authority_backing_error)?;
    // Changed by D-O1 (DR-110): the unsorted `channel` is dropped at the
    // return, and the sorted term holds the same blocks. A block inspection
    // prepays that release. The caller reserves its own read of the returned
    // channel.
    // meter
    //     .inspect(&sorted.term)
    //     .map_err(authority_backing_error)?;
    meter
        .inspect_blocks(&sorted.term)
        .map_err(authority_backing_error)?;
    Ok(sorted.term)
}

pub fn authority_regions(
    authority: &CostAuthority,
) -> Result<BTreeMap<[u8; 32], CostSignature>, AuthorityError> {
    let authority = canonical_authority(authority)?;
    authority
        .regions
        .into_iter()
        .map(|region| {
            let instance_id = region
                .instance_id
                .as_slice()
                .try_into()
                .map_err(|_| AuthorityError::InvalidRegionIdentity)?;
            let signature = region.signature.ok_or(AuthorityError::MissingSignature)?;
            Ok((instance_id, signature))
        })
        .collect()
}

pub fn authority_regions_metered(
    authority: &CostAuthority,
    backing: &dyn BackingMeter,
) -> Result<BTreeMap<[u8; 32], CostSignature>, AuthorityError> {
    let canonical = canonical_authority_metered(authority, backing)?;
    let meter = SorterMeter::new(backing);
    let mut regions = BTreeMap::new();
    for region in canonical.regions {
        meter
            .reserve(1, std::mem::size_of::<CostRegion>(), 0)
            .map_err(authority_backing_error)?;
        let instance_id = region
            .instance_id
            .as_slice()
            .try_into()
            .map_err(|_| AuthorityError::InvalidRegionIdentity)?;
        let signature = region.signature.ok_or(AuthorityError::MissingSignature)?;
        reserve_authority_tree_insert::<[u8; 32], CostSignature>(&meter, regions.len())?;
        regions.insert(instance_id, signature);
    }
    Ok(regions)
}

fn signature_atoms(signature: &CostSignature) -> Result<Vec<CostSignature>, AuthorityError> {
    let signature = canonical_cost_signature(signature)?;
    match signature.value {
        Some(CostSignatureValue::Unit(true)) => Ok(Vec::new()),
        Some(CostSignatureValue::Compound(compound)) => {
            let mut atoms = Vec::new();
            for element in &compound.elements {
                atoms.extend(signature_atoms(element)?);
            }
            Ok(atoms)
        }
        Some(_) => Ok(vec![signature]),
        None => Err(AuthorityError::MissingSignature),
    }
}

fn signature_from_atoms(atoms: &[CostSignature]) -> Result<CostSignature, AuthorityError> {
    match atoms {
        [] => sig_to_cost_signature(&Sig::Unit),
        [single] => canonical_cost_signature(single),
        _ => {
            let signature = sort_signature(&CostSignature {
                value: Some(CostSignatureValue::Compound(CostSignatureCompound {
                    elements: atoms.to_vec(),
                })),
            })
            .term;
            validate_cost_signature(&signature)?;
            Ok(signature)
        }
    }
}

fn event_atoms(event: &AuthorityEvent<[u8; 32]>) -> Result<Vec<CostSignature>, AuthorityError> {
    event.verify_authority()?;
    let mut signatures = BTreeMap::<[u8; 32], CostSignature>::new();
    for signature in authority_regions(&event.authority)?.into_values() {
        let key = cost_signature_to_sig(&signature)?.lane_hash();
        match signatures.get(&key) {
            Some(existing) if existing != &signature => {
                return Err(AuthorityError::EventSignatureConflict);
            }
            Some(_) => {}
            None => {
                signatures.insert(key, signature);
            }
        }
    }
    let mut atoms = Vec::new();
    for (key, amount) in &event.debit.0 {
        let signature = signatures
            .get(key)
            .ok_or(AuthorityError::EventDebitMismatch)?;
        let signature_atoms = signature_atoms(signature)?;
        for _ in 0..*amount {
            atoms.extend(signature_atoms.iter().cloned());
        }
    }
    atoms.sort_by_key(|signature| {
        cost_signature_to_sig(signature)
            .expect("validated cost signature")
            .lane_hash()
    });
    Ok(atoms)
}

pub fn authority_funding_options(
    event: &AuthorityEvent<[u8; 32]>,
) -> Result<Vec<ResourceMultiset<[u8; 32]>>, AuthorityError> {
    let atoms = event_atoms(event)?;
    if atoms.is_empty() {
        return Ok(vec![ResourceMultiset::default()]);
    }
    let mut unique = BTreeMap::new();
    for allocation in [
        event.debit.clone(),
        valuation::demand_from_atoms(&atoms)?,
        ResourceMultiset::singleton(
            cost_signature_to_sig(&signature_from_atoms(&atoms)?)?.lane_hash(),
            1,
        ),
    ] {
        let mut canonical = Vec::new();
        allocation.write_canonical(&mut canonical);
        unique.insert(canonical, allocation);
    }
    let mut options: Vec<_> = unique.into_values().collect();
    options.sort_by(|left, right| {
        let left_cells: u128 = left.0.values().map(|amount| u128::from(*amount)).sum();
        let right_cells: u128 = right.0.values().map(|amount| u128::from(*amount)).sum();
        left_cells.cmp(&right_cells).then_with(|| {
            let mut left_bytes = Vec::new();
            let mut right_bytes = Vec::new();
            left.write_canonical(&mut left_bytes);
            right.write_canonical(&mut right_bytes);
            left_bytes.cmp(&right_bytes)
        })
    });
    Ok(options)
}

pub fn authority_funding_signatures(
    events: &[AuthorityEvent<[u8; 32]>],
) -> Result<BTreeMap<[u8; 32], CostSignature>, AuthorityError> {
    authority_funding_signatures_with_presentations(events, &[])
}

pub fn authority_funding_signatures_with_presentations(
    events: &[AuthorityEvent<[u8; 32]>],
    presentations: &[CostSignature],
) -> Result<BTreeMap<[u8; 32], CostSignature>, AuthorityError> {
    authority_funding_signatures_for_events(events, &[], presentations)
}

pub fn authority_funding_signatures_for_events(
    events: &[AuthorityEvent<[u8; 32]>],
    byte_events: &[AuthorityByteEvent],
    presentations: &[CostSignature],
) -> Result<BTreeMap<[u8; 32], CostSignature>, AuthorityError> {
    authority_funding_signatures_for_events_with_host_work(events, byte_events, presentations, None)
}

pub fn authority_funding_signatures_for_events_with_host_work(
    events: &[AuthorityEvent<[u8; 32]>],
    byte_events: &[AuthorityByteEvent],
    presentations: &[CostSignature],
    host_work: Option<&HostWorkBudget>,
) -> Result<BTreeMap<[u8; 32], CostSignature>, AuthorityError> {
    fn insert(
        signatures: &mut BTreeMap<[u8; 32], CostSignature>,
        signature: CostSignature,
    ) -> Result<(), AuthorityError> {
        let signature = canonical_cost_signature(&signature)?;
        let runtime_signature = cost_signature_to_sig(&signature)?;
        if runtime_signature == Sig::Unit {
            return Ok(());
        }
        let key = runtime_signature.lane_hash();
        match signatures.get(&key) {
            Some(existing) if existing != &signature => Err(AuthorityError::EventSignatureConflict),
            Some(_) => Ok(()),
            None => {
                signatures.insert(key, signature);
                Ok(())
            }
        }
    }

    reserve_authority_discovery(events, byte_events, presentations, host_work)?;
    let mut signatures = BTreeMap::new();
    for event in events {
        let atoms = event_atoms(event)?;
        for signature in authority_regions(&event.authority)?.into_values() {
            insert(&mut signatures, signature)?;
        }
        for atom in &atoms {
            insert(&mut signatures, atom.clone())?;
        }
        if !atoms.is_empty() {
            insert(&mut signatures, signature_from_atoms(&atoms)?)?;
        }
    }
    for event in byte_events {
        event.verify_authority()?;
        for signature in authority_regions(&event.authority)?.into_values() {
            insert(&mut signatures, signature)?;
        }
        let funding_event = event.funding_event()?;
        let atoms = event_atoms(&funding_event)?;
        for atom in &atoms {
            insert(&mut signatures, atom.clone())?;
        }
        if !atoms.is_empty() {
            insert(&mut signatures, signature_from_atoms(&atoms)?)?;
        }
    }
    for presentation in presentations {
        insert(&mut signatures, presentation.clone())?;
    }
    Ok(signatures)
}

fn reserve_authority_discovery(
    events: &[AuthorityEvent<[u8; 32]>],
    byte_events: &[AuthorityByteEvent],
    presentations: &[CostSignature],
    host_work: Option<&HostWorkBudget>,
) -> Result<(), AuthorityError> {
    let Some(host_work) = host_work else {
        return Ok(());
    };
    let mut observed_depth = 0_u64;
    for event in events {
        for region in &event.authority.regions {
            if let Some(signature) = &region.signature {
                reserve_authority_signature_tree(signature, host_work, &mut observed_depth)?;
            }
        }
    }
    for event in byte_events {
        for region in &event.authority.regions {
            if let Some(signature) = &region.signature {
                reserve_authority_signature_tree(signature, host_work, &mut observed_depth)?;
            }
        }
    }
    for signature in presentations {
        reserve_authority_signature_tree(signature, host_work, &mut observed_depth)?;
    }
    Ok(())
}

pub fn reserve_authority_signature_tree(
    signature: &CostSignature,
    host_work: &HostWorkBudget,
    observed_depth: &mut u64,
) -> Result<(), AuthorityError> {
    reserve_host_work(host_work, HostWorkDimension::AuthorityNodes, 1)?;
    reserve_authority_depth(host_work, observed_depth, 1)?;
    let mut pending = vec![(signature, 1_u64)];
    while let Some((signature, depth)) = pending.pop() {
        let Some(CostSignatureValue::Compound(compound)) = signature.value.as_ref() else {
            continue;
        };
        let child_depth = depth
            .checked_add(1)
            .ok_or(AuthorityError::ArithmeticOverflow)?;
        reserve_authority_depth(host_work, observed_depth, child_depth)?;
        let child_count = u64::try_from(compound.elements.len())
            .map_err(|_| AuthorityError::ArithmeticOverflow)?;
        reserve_host_work(host_work, HostWorkDimension::AuthorityNodes, child_count)?;
        pending.extend(
            compound
                .elements
                .iter()
                .rev()
                .map(|element| (element, child_depth)),
        );
    }
    Ok(())
}

fn reserve_authority_depth(
    host_work: &HostWorkBudget,
    observed_depth: &mut u64,
    depth: u64,
) -> Result<(), AuthorityError> {
    if depth <= *observed_depth {
        return Ok(());
    }
    reserve_host_work(
        host_work,
        HostWorkDimension::AuthorityDepth,
        depth - *observed_depth,
    )?;
    *observed_depth = depth;
    Ok(())
}

fn reserve_host_work(
    host_work: &HostWorkBudget,
    dimension: HostWorkDimension,
    units: u64,
) -> Result<(), AuthorityError> {
    host_work.reserve(dimension, HostWorkUnits::new(units))?;
    Ok(())
}

pub fn allocate_authority_events(
    events: &[AuthorityEvent<[u8; 32]>],
    available: &ResourceMultiset<[u8; 32]>,
) -> Result<ResourceMultiset<[u8; 32]>, AuthorityError> {
    allocate_authority_event_draws(events, available)?
        .into_iter()
        .try_fold(ResourceMultiset::default(), |total, draw| {
            total.checked_add(&draw.balances)
        })
}

pub fn allocate_quantitative_debit(
    funding_event: &AuthorityEvent<[u8; 32]>,
    amount: u64,
    available: &ResourceMultiset<[u8; 32]>,
) -> Result<ResourceMultiset<[u8; 32]>, AuthorityError> {
    if amount == 0 {
        return Ok(ResourceMultiset::default());
    }
    for option in authority_funding_options(funding_event)? {
        let scaled = option.0.into_iter().try_fold(
            ResourceMultiset::default(),
            |mut allocation, (key, units)| {
                let debit = units
                    .checked_mul(amount)
                    .ok_or(AuthorityError::ArithmeticOverflow)?;
                if debit > 0 {
                    allocation.0.insert(key, debit);
                }
                Ok::<_, AuthorityError>(allocation)
            },
        )?;
        if available.dominates(&scaled) {
            return Ok(scaled);
        }
    }
    Err(AuthorityError::InsufficientAuthority)
}

pub fn allocate_quantitative_events(
    events: &[AuthorityByteEvent],
    available: &ResourceMultiset<[u8; 32]>,
) -> Result<ResourceMultiset<[u8; 32]>, AuthorityError> {
    let mut events = events.iter().collect::<Vec<_>>();
    events.sort_by_key(|event| event.canonical_key());
    let options = events
        .iter()
        .map(|event| {
            event.verify_authority()?;
            authority_funding_options(&event.funding_event()?)?
                .into_iter()
                .map(|option| {
                    option.0.into_iter().try_fold(
                        ResourceMultiset::default(),
                        |mut allocation, (key, units)| {
                            let debit = units
                                .checked_mul(event.amount)
                                .ok_or(AuthorityError::ArithmeticOverflow)?;
                            if debit > 0 {
                                allocation.0.insert(key, debit);
                            }
                            Ok(allocation)
                        },
                    )
                })
                .collect::<Result<Vec<_>, AuthorityError>>()
        })
        .collect::<Result<Vec<_>, AuthorityError>>()?;
    let mut failed = BTreeSet::new();
    allocate_event_options(&options, available, &mut failed)
        .map(|(_, draws)| {
            draws
                .into_iter()
                .try_fold(ResourceMultiset::default(), |total, draw| {
                    total.checked_add(&draw)
                })
        })
        .transpose()?
        .ok_or(AuthorityError::InsufficientAuthority)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuthorityBalanceSettlement<K: Ord = [u8; 32]> {
    pub logical_debit: ResourceMultiset<K>,
    pub custody_debit: ResourceMultiset<K>,
}

pub fn allocate_authority_events_with_custody(
    events: &[AuthorityEvent<[u8; 32]>],
    available: &ResourceMultiset<[u8; 32]>,
    balance_custody: &BTreeMap<[u8; 32], [u8; 32]>,
) -> Result<AuthorityBalanceSettlement<[u8; 32]>, AuthorityError> {
    let options = events
        .iter()
        .map(authority_funding_options)
        .collect::<Result<Vec<_>, _>>()?;
    allocate_event_options_with_custody(&options, available, balance_custody)
}

pub fn allocate_quantitative_events_with_custody(
    events: &[AuthorityByteEvent],
    available: &ResourceMultiset<[u8; 32]>,
    balance_custody: &BTreeMap<[u8; 32], [u8; 32]>,
) -> Result<AuthorityBalanceSettlement<[u8; 32]>, AuthorityError> {
    let mut events = events.iter().collect::<Vec<_>>();
    events.sort_by_key(|event| event.canonical_key());
    let options = events
        .iter()
        .map(|event| {
            event.verify_authority()?;
            authority_funding_options(&event.funding_event()?)?
                .into_iter()
                .map(|option| {
                    option.0.into_iter().try_fold(
                        ResourceMultiset::default(),
                        |mut allocation, (key, units)| {
                            let debit = units
                                .checked_mul(event.amount)
                                .ok_or(AuthorityError::ArithmeticOverflow)?;
                            if debit > 0 {
                                allocation.0.insert(key, debit);
                            }
                            Ok(allocation)
                        },
                    )
                })
                .collect::<Result<Vec<_>, AuthorityError>>()
        })
        .collect::<Result<Vec<_>, AuthorityError>>()?;
    allocate_event_options_with_custody(&options, available, balance_custody)
}

pub fn allocate_authority_event_draws(
    events: &[AuthorityEvent<[u8; 32]>],
    available: &ResourceMultiset<[u8; 32]>,
) -> Result<Vec<AuthorityPhysicalEventDraw>, AuthorityError> {
    let options = events
        .iter()
        .map(authority_funding_options)
        .collect::<Result<Vec<_>, _>>()?;
    let mut failed = BTreeSet::new();
    allocate_event_options(&options, available, &mut failed)
        .map(|(_, draws)| {
            events
                .iter()
                .zip(draws)
                .map(|(event, balances)| AuthorityPhysicalEventDraw {
                    event_id: event.event_id,
                    balances,
                    stack_ids: Vec::new(),
                })
                .collect()
        })
        .ok_or(AuthorityError::InsufficientAuthority)
}

fn allocate_event_options(
    options: &[Vec<ResourceMultiset<[u8; 32]>>],
    available: &ResourceMultiset<[u8; 32]>,
    failed: &mut BTreeSet<(usize, Vec<([u8; 32], u64)>)>,
) -> Option<(ResourceMultiset<[u8; 32]>, Vec<ResourceMultiset<[u8; 32]>>)> {
    struct Frame {
        index: usize,
        next_option: usize,
        available: ResourceMultiset<[u8; 32]>,
    }

    let mut frames = vec![Frame {
        index: 0,
        next_option: 0,
        available: available.clone(),
    }];
    let mut draws = Vec::with_capacity(options.len());
    while let Some(frame) = frames.last_mut() {
        if frame.index == options.len() {
            return Some((frame.available.clone(), draws));
        }
        let state = (
            frame.index,
            frame
                .available
                .0
                .iter()
                .map(|(key, amount)| (*key, *amount))
                .collect(),
        );
        if failed.contains(&state) || frame.next_option == options[frame.index].len() {
            failed.insert(state);
            let had_parent = frame.index != 0;
            frames.pop();
            if had_parent {
                draws.pop();
            }
            continue;
        }
        let draw = options[frame.index][frame.next_option].clone();
        let next_index = frame.index + 1;
        frame.next_option += 1;
        if let Ok(remaining) = frame.available.checked_sub(&draw) {
            draws.push(draw);
            frames.push(Frame {
                index: next_index,
                next_option: 0,
                available: remaining,
            });
        }
    }
    None
}

fn allocate_event_options_with_custody(
    options: &[Vec<ResourceMultiset<[u8; 32]>>],
    available: &ResourceMultiset<[u8; 32]>,
    balance_custody: &BTreeMap<[u8; 32], [u8; 32]>,
) -> Result<AuthorityBalanceSettlement<[u8; 32]>, AuthorityError> {
    struct Frame {
        index: usize,
        next_option: usize,
        available: ResourceMultiset<[u8; 32]>,
    }

    if options.iter().flatten().any(|option| {
        option
            .0
            .keys()
            .any(|lane| !balance_custody.contains_key(lane))
    }) {
        return Err(AuthorityError::UnknownPhysicalCustody);
    }

    let mut frames = vec![Frame {
        index: 0,
        next_option: 0,
        available: available.clone(),
    }];
    let mut draws = Vec::with_capacity(options.len());
    let mut failed = BTreeSet::new();
    while let Some(frame) = frames.last_mut() {
        if frame.index == options.len() {
            let logical_debit = draws
                .iter()
                .try_fold(ResourceMultiset::default(), |total, draw| {
                    total.checked_add(draw)
                })?;
            let custody_debit = physicalize_balance_debit(&logical_debit, balance_custody)?;
            return Ok(AuthorityBalanceSettlement {
                logical_debit,
                custody_debit,
            });
        }
        let state = (
            frame.index,
            frame
                .available
                .0
                .iter()
                .map(|(key, amount)| (*key, *amount))
                .collect::<Vec<_>>(),
        );
        if failed.contains(&state) || frame.next_option == options[frame.index].len() {
            failed.insert(state);
            let had_parent = frame.index != 0;
            frames.pop();
            if had_parent {
                draws.pop();
            }
            continue;
        }
        let draw = options[frame.index][frame.next_option].clone();
        let custody_draw = physicalize_balance_debit(&draw, balance_custody)?;
        let next_index = frame.index + 1;
        frame.next_option += 1;
        if let Ok(remaining) = frame.available.checked_sub(&custody_draw) {
            draws.push(draw);
            frames.push(Frame {
                index: next_index,
                next_option: 0,
                available: remaining,
            });
        }
    }
    Err(AuthorityError::InsufficientAuthority)
}

pub fn instantiate_persistent_regions(
    authority: &CostAuthority,
    persistent_regions: &BTreeSet<[u8; 32]>,
    occurrence: [u8; 32],
) -> Result<CostAuthority, AuthorityError> {
    // Changed by D-F1 (DR-117): the body moved, unchanged, into
    // `instantiate_persistent_regions_canonical`, which returns the witness.
    instantiate_persistent_regions_canonical(authority, persistent_regions, occurrence)
        .map(CanonicalAuthority::into_authority)
}

/// Added by D-F1 (DR-117): `instantiate_persistent_regions` with its
/// canonical result as a witness.
pub(crate) fn instantiate_persistent_regions_canonical(
    authority: &CostAuthority,
    persistent_regions: &BTreeSet<[u8; 32]>,
    occurrence: [u8; 32],
) -> Result<CanonicalAuthority, AuthorityError> {
    let regions = authority_regions(authority)?;
    let instantiated = CostAuthority {
        regions: regions
            .into_iter()
            .map(|(instance_id, signature)| {
                let instance_id = if persistent_regions.contains(&instance_id) {
                    let mut bytes = Vec::with_capacity(
                        REGION_OCCURRENCE_DOMAIN.len() + instance_id.len() + occurrence.len(),
                    );
                    bytes.extend_from_slice(REGION_OCCURRENCE_DOMAIN);
                    bytes.extend_from_slice(&instance_id);
                    bytes.extend_from_slice(&occurrence);
                    Blake2b256::hash(bytes)
                } else {
                    instance_id.to_vec()
                };
                CostRegion {
                    instance_id,
                    signature: Some(signature),
                }
            })
            .collect(),
    };
    canonical_regions(&instantiated.regions)
}

pub fn instantiate_persistent_regions_metered(
    authority: &CostAuthority,
    persistent_regions: &BTreeSet<[u8; 32]>,
    occurrence: [u8; 32],
    backing: &dyn BackingMeter,
) -> Result<CostAuthority, AuthorityError> {
    let regions = authority_regions_metered(authority, backing)?;
    let meter = SorterMeter::new(backing);
    let mut instantiated = meter
        .vec::<CostRegion>(regions.len())
        .map_err(authority_backing_error)?;
    for (instance_id, signature) in regions {
        let comparisons = persistent_regions
            .len()
            .checked_add(1)
            .ok_or(AuthorityError::HostWorkRejected)?;
        let scanned = comparisons
            .checked_mul(32)
            .ok_or(AuthorityError::HostWorkRejected)?;
        meter
            .reserve(comparisons, scanned, 0)
            .map_err(authority_backing_error)?;
        let instance_id = if persistent_regions.contains(&instance_id) {
            let capacity = REGION_OCCURRENCE_DOMAIN
                .len()
                .checked_add(instance_id.len())
                .and_then(|len| len.checked_add(occurrence.len()))
                .ok_or(AuthorityError::HostWorkRejected)?;
            let mut bytes = meter.vec::<u8>(capacity).map_err(authority_backing_error)?;
            bytes.extend_from_slice(REGION_OCCURRENCE_DOMAIN);
            bytes.extend_from_slice(&instance_id);
            bytes.extend_from_slice(&occurrence);
            meter
                .reserve(1, capacity, 32)
                .map_err(authority_backing_error)?;
            Blake2b256::hash(bytes)
        } else {
            let mut bytes = meter
                .vec::<u8>(instance_id.len())
                .map_err(authority_backing_error)?;
            bytes.extend_from_slice(&instance_id);
            bytes
        };
        instantiated.push(CostRegion {
            instance_id,
            signature: Some(signature),
        });
    }
    canonical_authority_metered(
        &CostAuthority {
            regions: instantiated,
        },
        backing,
    )
}

/// Added by D-F1 (DR-117): the persistent-occurrence instantiation of a
/// witness. A persistent region takes a fresh identity from its identity and
/// the occurrence, and every other region keeps its identity. The witness is
/// canonical, so no signature is validated again. Only the new identity order
/// and the conflict check of a repeated new identity remain. The value and
/// the first error are those of `instantiate_persistent_regions_metered`,
/// which canonicalizes the witness again before and after the new identities.
pub(crate) fn rekey_metered(
    witness: CanonicalAuthority,
    persistent_regions: &BTreeSet<[u8; 32]>,
    occurrence: [u8; 32],
    backing: &dyn BackingMeter,
) -> Result<CanonicalAuthority, AuthorityError> {
    rekey_by(witness, backing, |instance_id, meter| {
        let key: [u8; 32] = instance_id
            .as_slice()
            .try_into()
            .map_err(|_| AuthorityError::InvalidRegionIdentity)?;
        let comparisons = persistent_regions
            .len()
            .checked_add(1)
            .ok_or(AuthorityError::HostWorkRejected)?;
        let scanned = comparisons
            .checked_mul(32)
            .ok_or(AuthorityError::HostWorkRejected)?;
        meter
            .reserve(comparisons, scanned, 0)
            .map_err(authority_backing_error)?;
        if !persistent_regions.contains(&key) {
            return Ok(instance_id);
        }
        let capacity = REGION_OCCURRENCE_DOMAIN
            .len()
            .checked_add(key.len())
            .and_then(|len| len.checked_add(occurrence.len()))
            .ok_or(AuthorityError::HostWorkRejected)?;
        let mut bytes = meter.vec::<u8>(capacity).map_err(authority_backing_error)?;
        bytes.extend_from_slice(REGION_OCCURRENCE_DOMAIN);
        bytes.extend_from_slice(&key);
        bytes.extend_from_slice(&occurrence);
        meter
            .reserve(1, capacity, 32)
            .map_err(authority_backing_error)?;
        Ok(Blake2b256::hash(bytes))
    })
}

/// Added by D-F1 (DR-117): the rekey of a witness with an arbitrary identity
/// map, so that a test can force a collision of new identities. The regions
/// are visited in identity order, as the canonicalization visits them.
fn rekey_by(
    witness: CanonicalAuthority,
    backing: &dyn BackingMeter,
    mut new_identity: impl FnMut(Vec<u8>, &SorterMeter<'_>) -> Result<Vec<u8>, AuthorityError>,
) -> Result<CanonicalAuthority, AuthorityError> {
    let meter = SorterMeter::new(backing);
    let mut regions = BTreeMap::<Vec<u8>, CostSignature>::new();
    for region in witness.0.regions {
        meter
            .reserve(1, std::mem::size_of::<CostRegion>(), 0)
            .map_err(authority_backing_error)?;
        let signature = region.signature.ok_or(AuthorityError::MissingSignature)?;
        let instance_id = new_identity(region.instance_id, &meter)?;
        let comparisons = regions
            .len()
            .checked_add(1)
            .ok_or(AuthorityError::HostWorkRejected)?;
        let scanned = comparisons
            .checked_mul(32)
            .ok_or(AuthorityError::HostWorkRejected)?;
        meter
            .reserve(comparisons, scanned, 0)
            .map_err(authority_backing_error)?;
        match regions.get(&instance_id) {
            Some(existing) => {
                meter
                    .inspect_blocks(existing)
                    .map_err(authority_backing_error)?;
                meter
                    .inspect_blocks(&signature)
                    .map_err(authority_backing_error)?;
                if existing != &signature {
                    return Err(AuthorityError::RegionIdentityConflict);
                }
            }
            None => {
                reserve_authority_tree_insert::<Vec<u8>, CostSignature>(&meter, regions.len())?;
                regions.insert(instance_id, signature);
            }
        }
    }
    let mut sorted = meter
        .vec::<CostRegion>(regions.len())
        .map_err(authority_backing_error)?;
    for (instance_id, signature) in regions {
        sorted.push(CostRegion {
            instance_id,
            signature: Some(signature),
        });
    }
    Ok(CanonicalAuthority(CostAuthority { regions: sorted }))
}

pub trait CanonicalAuthorityKey {
    fn write_canonical(&self, output: &mut Vec<u8>);
}

impl CanonicalAuthorityKey for [u8; 32] {
    fn write_canonical(&self, output: &mut Vec<u8>) { output.extend_from_slice(self); }
}

impl CanonicalAuthorityKey for Vec<u8> {
    fn write_canonical(&self, output: &mut Vec<u8>) {
        output.extend_from_slice(&(self.len() as u64).to_le_bytes());
        output.extend_from_slice(self);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "K: Ord + Serialize",
    deserialize = "K: Ord + Deserialize<'de>"
))]
pub struct ResourceMultiset<K>(pub BTreeMap<K, u64>);

impl<K> Default for ResourceMultiset<K> {
    fn default() -> Self { Self(BTreeMap::new()) }
}

impl<K: Ord + Clone> ResourceMultiset<K> {
    pub fn singleton(key: K, amount: u64) -> Self {
        let mut values = BTreeMap::new();
        if amount > 0 {
            values.insert(key, amount);
        }
        Self(values)
    }

    pub fn get(&self, key: &K) -> u64 { self.0.get(key).copied().unwrap_or(0) }

    pub fn dominates(&self, other: &Self) -> bool {
        other.0.iter().all(|(key, amount)| self.get(key) >= *amount)
    }

    pub fn checked_add(&self, other: &Self) -> Result<Self, AuthorityError> {
        let mut result = self.clone();
        for (key, amount) in &other.0 {
            let next = result
                .get(key)
                .checked_add(*amount)
                .ok_or(AuthorityError::ArithmeticOverflow)?;
            if next == 0 {
                result.0.remove(key);
            } else {
                result.0.insert(key.clone(), next);
            }
        }
        Ok(result)
    }

    pub fn checked_sub(&self, other: &Self) -> Result<Self, AuthorityError> {
        if !self.dominates(other) {
            return Err(AuthorityError::InsufficientAuthority);
        }
        let mut result = self.clone();
        for (key, amount) in &other.0 {
            let next = result.get(key) - *amount;
            if next == 0 {
                result.0.remove(key);
            } else {
                result.0.insert(key.clone(), next);
            }
        }
        Ok(result)
    }
}

impl ResourceMultiset<[u8; 32]> {
    fn increment_metered(
        &mut self,
        key: [u8; 32],
        amount: u64,
        meter: &SorterMeter<'_>,
    ) -> Result<(), AuthorityError> {
        let comparisons = self
            .0
            .len()
            .checked_add(1)
            .ok_or(AuthorityError::HostWorkRejected)?;
        let scanned = comparisons
            .checked_mul(32)
            .ok_or(AuthorityError::HostWorkRejected)?;
        meter
            .reserve(comparisons, scanned, 0)
            .map_err(authority_backing_error)?;
        match self.0.get_mut(&key) {
            Some(existing) => {
                *existing = existing
                    .checked_add(amount)
                    .ok_or(AuthorityError::ArithmeticOverflow)?;
            }
            None if amount > 0 => {
                reserve_authority_tree_insert::<[u8; 32], u64>(meter, self.0.len())?;
                self.0.insert(key, amount);
            }
            None => {}
        }
        Ok(())
    }
}

impl<K: CanonicalAuthorityKey + Ord> ResourceMultiset<K> {
    fn write_canonical(&self, output: &mut Vec<u8>) {
        output.extend_from_slice(&(self.0.len() as u64).to_le_bytes());
        for (key, amount) in &self.0 {
            key.write_canonical(output);
            output.extend_from_slice(&amount.to_le_bytes());
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "K: Ord + Serialize",
    deserialize = "K: Ord + Deserialize<'de>"
))]
pub struct AuthorityPhysicalInventory<K: Ord = [u8; 32]> {
    pub balances: ResourceMultiset<K>,
    #[serde(default)]
    pub balance_custody: BTreeMap<K, K>,
    pub stacks: BTreeMap<[u8; 32], Vec<CostSignature>>,
    #[serde(default)]
    pub born_stacks: BTreeMap<[u8; 32], [u8; 32]>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "K: Ord + Serialize",
    deserialize = "K: Ord + Deserialize<'de>"
))]
pub struct AuthorityPhysicalEventDraw<K: Ord = [u8; 32]> {
    pub event_id: [u8; 32],
    pub balances: ResourceMultiset<K>,
    pub stack_ids: Vec<[u8; 32]>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "K: Ord + Serialize",
    deserialize = "K: Ord + Deserialize<'de>"
))]
pub struct AuthorityPhysicalSettlement<K: Ord = [u8; 32]> {
    pub draws: Vec<AuthorityPhysicalEventDraw<K>>,
    pub balance_debit: ResourceMultiset<K>,
    pub custody_debit: ResourceMultiset<K>,
    pub stack_pops: BTreeMap<[u8; 32], u64>,
}

impl<K: Ord + Clone> AuthorityPhysicalInventory<K> {
    pub fn insert_balance_lane(
        &mut self,
        lane: K,
        custody: K,
        balance: u64,
    ) -> Result<(), AuthorityError> {
        if self
            .balance_custody
            .get(&lane)
            .is_some_and(|existing| existing != &custody)
        {
            return Err(AuthorityError::PhysicalCustodyMismatch);
        }
        let known = self
            .balance_custody
            .values()
            .any(|existing| existing == &custody);
        if known && self.balances.get(&custody) != balance {
            return Err(AuthorityError::PhysicalCustodyMismatch);
        }
        if !known && balance > 0 {
            self.balances.0.insert(custody.clone(), balance);
        }
        self.balance_custody.insert(lane, custody);
        Ok(())
    }

    pub fn physical_debit(
        &self,
        logical: &ResourceMultiset<K>,
    ) -> Result<ResourceMultiset<K>, AuthorityError> {
        physicalize_balance_debit(logical, &self.balance_custody)
    }

    pub fn logical_balance_view(&self) -> ResourceMultiset<K> {
        logical_balance_view(&self.balances, &self.balance_custody)
    }
}

pub fn physicalize_balance_debit<K: Ord + Clone>(
    logical: &ResourceMultiset<K>,
    balance_custody: &BTreeMap<K, K>,
) -> Result<ResourceMultiset<K>, AuthorityError> {
    let mut physical = ResourceMultiset::default();
    for (lane, amount) in &logical.0 {
        let custody = balance_custody
            .get(lane)
            .ok_or(AuthorityError::UnknownPhysicalCustody)?;
        let total = physical
            .get(custody)
            .checked_add(*amount)
            .ok_or(AuthorityError::ArithmeticOverflow)?;
        if total > 0 {
            physical.0.insert(custody.clone(), total);
        }
    }
    Ok(physical)
}

pub fn logical_balance_view<K: Ord + Clone>(
    physical: &ResourceMultiset<K>,
    balance_custody: &BTreeMap<K, K>,
) -> ResourceMultiset<K> {
    ResourceMultiset(
        balance_custody
            .iter()
            .filter_map(|(lane, custody)| {
                let balance = physical.get(custody);
                (balance > 0).then(|| (lane.clone(), balance))
            })
            .collect(),
    )
}

fn add_signature_atoms(
    target: &mut ResourceMultiset<[u8; 32]>,
    signatures: &mut BTreeMap<[u8; 32], CostSignature>,
    signature: &CostSignature,
    amount: u64,
) -> Result<(), AuthorityError> {
    for atom in signature_atoms(signature)? {
        let key = cost_signature_to_sig(&atom)?.lane_hash();
        match signatures.get(&key) {
            Some(existing) if existing != &atom => {
                return Err(AuthorityError::EventSignatureConflict);
            }
            Some(_) => {}
            None => {
                signatures.insert(key, atom);
            }
        }
        let next = target
            .get(&key)
            .checked_add(amount)
            .ok_or(AuthorityError::ArithmeticOverflow)?;
        target.0.insert(key, next);
    }
    Ok(())
}

pub fn verify_physical_settlement(
    events: &[AuthorityEvent<[u8; 32]>],
    signatures: &BTreeMap<[u8; 32], CostSignature>,
    inventory: &AuthorityPhysicalInventory<[u8; 32]>,
    draws: &[AuthorityPhysicalEventDraw<[u8; 32]>],
) -> Result<AuthorityPhysicalSettlement<[u8; 32]>, AuthorityError> {
    if events.len() != draws.len() {
        return Err(AuthorityError::SettlementPresentationMismatch);
    }
    let mut remaining = inventory.balances.clone();
    let mut stack_positions = BTreeMap::<[u8; 32], usize>::new();
    let mut balance_debit = ResourceMultiset::default();
    let mut custody_debit = ResourceMultiset::default();
    let mut stack_pops = BTreeMap::<[u8; 32], u64>::new();
    let event_positions = events
        .iter()
        .enumerate()
        .map(|(index, event)| (event.event_id, index))
        .collect::<BTreeMap<_, _>>();
    let mut born_available_after = BTreeMap::new();
    for (stack_id, produce_hash) in &inventory.born_stacks {
        let cells = inventory
            .stacks
            .get(stack_id)
            .ok_or(AuthorityError::UnknownStackResource)?;
        let mut available_after = None;
        for index in 0..cells.len() {
            let transfer = stack_transfer_event_id(produce_hash, index as u64);
            let position = *event_positions
                .get(&transfer)
                .ok_or(AuthorityError::SettlementPresentationMismatch)?;
            available_after =
                Some(available_after.map_or(position, |prior: usize| prior.max(position)));
        }
        born_available_after.insert(
            *stack_id,
            available_after.ok_or(AuthorityError::MissingSignature)?,
        );
    }

    for (event_index, (event, draw)) in events.iter().zip(draws).enumerate() {
        event.verify_authority()?;
        if event.event_id != draw.event_id {
            return Err(AuthorityError::SettlementPresentationMismatch);
        }
        if draw.stack_ids.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(AuthorityError::NonCanonicalStackDraw);
        }
        let physical_draw = inventory.physical_debit(&draw.balances)?;
        remaining = remaining.checked_sub(&physical_draw)?;
        balance_debit = balance_debit.checked_add(&draw.balances)?;
        custody_debit = custody_debit.checked_add(&physical_draw)?;

        let mut expected = ResourceMultiset::default();
        let mut expected_signatures = BTreeMap::new();
        for atom in event_atoms(event)? {
            add_signature_atoms(&mut expected, &mut expected_signatures, &atom, 1)?;
        }

        let mut presented = ResourceMultiset::default();
        let mut presented_signatures = BTreeMap::new();
        for (key, amount) in &draw.balances.0 {
            let signature = signatures
                .get(key)
                .ok_or(AuthorityError::UnknownPhysicalSignature)?;
            if cost_signature_to_sig(signature)?.lane_hash() != *key {
                return Err(AuthorityError::EventSignatureConflict);
            }
            add_signature_atoms(
                &mut presented,
                &mut presented_signatures,
                signature,
                *amount,
            )?;
        }
        for stack_id in &draw.stack_ids {
            if born_available_after
                .get(stack_id)
                .is_some_and(|available_after| event_index <= *available_after)
            {
                return Err(AuthorityError::PhysicalAuthorityMismatch);
            }
            let cells = inventory
                .stacks
                .get(stack_id)
                .ok_or(AuthorityError::UnknownStackResource)?;
            let position = stack_positions.entry(*stack_id).or_default();
            let signature = cells
                .get(*position)
                .ok_or(AuthorityError::ExhaustedStackResource)?;
            add_signature_atoms(&mut presented, &mut presented_signatures, signature, 1)?;
            *position += 1;
            *stack_pops.entry(*stack_id).or_default() += 1;
        }
        if presented != expected || presented_signatures != expected_signatures {
            return Err(AuthorityError::PhysicalAuthorityMismatch);
        }
    }

    Ok(AuthorityPhysicalSettlement {
        draws: draws.to_vec(),
        balance_debit,
        custody_debit,
        stack_pops,
    })
}

#[derive(Clone)]
enum PhysicalCandidate {
    Balance { lane: [u8; 32], custody: [u8; 32] },
    Stack([u8; 32]),
}

type PhysicalSearchState = (
    usize,
    Vec<([u8; 32], u64)>,
    Vec<([u8; 32], u64)>,
    Vec<([u8; 32], usize)>,
    Vec<[u8; 32]>,
);

#[derive(Clone)]
struct PhysicalSearchNode {
    event_index: usize,
    event_remaining: ResourceMultiset<[u8; 32]>,
    balances: ResourceMultiset<[u8; 32]>,
    stack_positions: BTreeMap<[u8; 32], usize>,
    event_balances: ResourceMultiset<[u8; 32]>,
    event_stacks: BTreeSet<[u8; 32]>,
    draws: Option<Arc<PhysicalDrawLink>>,
}

struct PhysicalDrawLink {
    previous: Option<Arc<PhysicalDrawLink>>,
    draw: AuthorityPhysicalEventDraw<[u8; 32]>,
}

enum PhysicalSearchWork {
    Search(PhysicalSearchNode),
    MarkFailed(PhysicalSearchState),
}

const SEARCH_KEY_BYTES: u64 = 32;
const SEARCH_COUNT_BYTES: u64 = 8;
const SEARCH_RESOURCE_ENTRY_BYTES: u64 = SEARCH_KEY_BYTES + SEARCH_COUNT_BYTES;
const SEARCH_CANDIDATE_BYTES: u64 = 2 + SEARCH_COUNT_BYTES + SEARCH_KEY_BYTES * 3;

fn checked_collection_bytes(length: usize, entry_bytes: u64) -> Result<u64, AuthorityError> {
    u64::try_from(length)
        .map_err(|_| AuthorityError::ArithmeticOverflow)?
        .checked_mul(entry_bytes)
        .and_then(|bytes| bytes.checked_add(SEARCH_COUNT_BYTES))
        .ok_or(AuthorityError::ArithmeticOverflow)
}

fn checked_search_state_bytes(
    event_remaining: usize,
    balances: usize,
    stack_positions: usize,
    event_balances: usize,
    event_stacks: usize,
) -> Result<u64, AuthorityError> {
    [
        SEARCH_COUNT_BYTES,
        checked_collection_bytes(event_remaining, SEARCH_RESOURCE_ENTRY_BYTES)?,
        checked_collection_bytes(balances, SEARCH_RESOURCE_ENTRY_BYTES)?,
        checked_collection_bytes(stack_positions, SEARCH_RESOURCE_ENTRY_BYTES)?,
        checked_collection_bytes(event_balances, SEARCH_RESOURCE_ENTRY_BYTES)?,
        checked_collection_bytes(event_stacks, SEARCH_KEY_BYTES)?,
        SEARCH_COUNT_BYTES,
    ]
    .into_iter()
    .try_fold(0_u64, |total, bytes| {
        total
            .checked_add(bytes)
            .ok_or(AuthorityError::ArithmeticOverflow)
    })
}

fn search_node_bytes(node: &PhysicalSearchNode) -> Result<u64, AuthorityError> {
    checked_search_state_bytes(
        node.event_remaining.0.len(),
        node.balances.0.len(),
        node.stack_positions.len(),
        node.event_balances.0.len(),
        node.event_stacks.len(),
    )
}

fn failed_state_bytes(node: &PhysicalSearchNode) -> Result<u64, AuthorityError> {
    [
        SEARCH_COUNT_BYTES,
        checked_collection_bytes(node.event_remaining.0.len(), SEARCH_RESOURCE_ENTRY_BYTES)?,
        checked_collection_bytes(node.balances.0.len(), SEARCH_RESOURCE_ENTRY_BYTES)?,
        checked_collection_bytes(node.stack_positions.len(), SEARCH_RESOURCE_ENTRY_BYTES)?,
        checked_collection_bytes(node.event_stacks.len(), SEARCH_KEY_BYTES)?,
    ]
    .into_iter()
    .try_fold(0_u64, |total, bytes| {
        total
            .checked_add(bytes)
            .ok_or(AuthorityError::ArithmeticOverflow)
    })
}

fn draw_bytes(balance_count: usize, stack_count: usize) -> Result<u64, AuthorityError> {
    [
        SEARCH_KEY_BYTES,
        checked_collection_bytes(balance_count, SEARCH_RESOURCE_ENTRY_BYTES)?,
        checked_collection_bytes(stack_count, SEARCH_KEY_BYTES)?,
    ]
    .into_iter()
    .try_fold(0_u64, |total, bytes| {
        total
            .checked_add(bytes)
            .ok_or(AuthorityError::ArithmeticOverflow)
    })
}

fn reserve_search_work(
    host_work: Option<&HostWorkBudget>,
    dimension: HostWorkDimension,
    units: u64,
) -> Result<(), AuthorityError> {
    if let Some(host_work) = host_work {
        reserve_host_work(host_work, dimension, units)?;
    }
    Ok(())
}

fn reserve_search_state_with<F>(
    host_work: Option<&HostWorkBudget>,
    measure: F,
) -> Result<(), AuthorityError>
where
    F: FnOnce() -> Result<u64, AuthorityError>,
{
    let Some(host_work) = host_work else {
        return Ok(());
    };
    reserve_host_work(host_work, HostWorkDimension::SearchStateBytes, measure()?)
}

#[allow(clippy::too_many_arguments)]
fn search_physical_settlement(
    events: &[AuthorityEvent<[u8; 32]>],
    expected: &[ResourceMultiset<[u8; 32]>],
    balance_atoms: &BTreeMap<[u8; 32], ResourceMultiset<[u8; 32]>>,
    balance_custody: &BTreeMap<[u8; 32], [u8; 32]>,
    stack_atoms: &BTreeMap<[u8; 32], Vec<ResourceMultiset<[u8; 32]>>>,
    event_index: usize,
    event_remaining: ResourceMultiset<[u8; 32]>,
    balances: ResourceMultiset<[u8; 32]>,
    stack_positions: BTreeMap<[u8; 32], usize>,
    event_balances: ResourceMultiset<[u8; 32]>,
    event_stacks: BTreeSet<[u8; 32]>,
    born_available_after: &BTreeMap<[u8; 32], usize>,
    failed: &mut BTreeSet<PhysicalSearchState>,
    host_work: Option<&HostWorkBudget>,
) -> Result<Option<Vec<AuthorityPhysicalEventDraw<[u8; 32]>>>, AuthorityError> {
    let initial = PhysicalSearchNode {
        event_index,
        event_remaining,
        balances,
        stack_positions,
        event_balances,
        event_stacks,
        draws: None,
    };
    reserve_search_state_with(host_work, || search_node_bytes(&initial))?;
    let mut work = vec![PhysicalSearchWork::Search(initial)];

    while let Some(next_work) = work.pop() {
        let mut node = match next_work {
            PhysicalSearchWork::Search(node) => node,
            PhysicalSearchWork::MarkFailed(state) => {
                failed.insert(state);
                continue;
            }
        };

        if node.event_index == events.len() {
            if !node.event_remaining.0.is_empty() {
                continue;
            }
            reserve_search_state_with(host_work, || {
                let mut result_bytes = SEARCH_COUNT_BYTES;
                let mut measured_link = node.draws.as_ref();
                while let Some(current) = measured_link {
                    result_bytes = result_bytes
                        .checked_add(draw_bytes(
                            current.draw.balances.0.len(),
                            current.draw.stack_ids.len(),
                        )?)
                        .ok_or(AuthorityError::ArithmeticOverflow)?;
                    measured_link = current.previous.as_ref();
                }
                Ok(result_bytes)
            })?;
            let mut draws = Vec::with_capacity(events.len());
            let mut link = node.draws;
            while let Some(current) = link {
                draws.push(current.draw.clone());
                link = current.previous.clone();
            }
            draws.reverse();
            return Ok(Some(draws));
        }

        if node.event_remaining.0.is_empty() {
            let next_index = node
                .event_index
                .checked_add(1)
                .ok_or(AuthorityError::ArithmeticOverflow)?;
            let next_expected = expected.get(next_index);
            reserve_search_state_with(host_work, || {
                checked_search_state_bytes(
                    next_expected.map_or(0, |value| value.0.len()),
                    node.balances.0.len(),
                    node.stack_positions.len(),
                    0,
                    0,
                )?
                .checked_add(draw_bytes(
                    node.event_balances.0.len(),
                    node.event_stacks.len(),
                )?)
                .ok_or(AuthorityError::ArithmeticOverflow)
            })?;
            let draw = AuthorityPhysicalEventDraw {
                event_id: events[node.event_index].event_id,
                balances: std::mem::take(&mut node.event_balances),
                stack_ids: std::mem::take(&mut node.event_stacks).into_iter().collect(),
            };
            node.event_index = next_index;
            node.event_remaining = next_expected.cloned().unwrap_or_default();
            node.draws = Some(Arc::new(PhysicalDrawLink {
                previous: node.draws,
                draw,
            }));
            work.push(PhysicalSearchWork::Search(node));
            continue;
        }

        reserve_search_state_with(host_work, || failed_state_bytes(&node))?;
        let state = (
            node.event_index,
            node.event_remaining
                .0
                .iter()
                .map(|(key, amount)| (*key, *amount))
                .collect(),
            node.balances
                .0
                .iter()
                .map(|(key, amount)| (*key, *amount))
                .collect(),
            node.stack_positions
                .iter()
                .map(|(key, position)| (*key, *position))
                .collect(),
            node.event_stacks.iter().copied().collect(),
        );
        if failed.contains(&state) {
            continue;
        }

        let Some(pivot) = node.event_remaining.0.keys().next().copied() else {
            continue;
        };
        let mut candidates =
            Vec::<(std::cmp::Reverse<u64>, u8, [u8; 32], PhysicalCandidate)>::new();
        for (lane, custody) in balance_custody {
            reserve_search_work(host_work, HostWorkDimension::SearchCandidates, 1)?;
            if node.balances.get(custody) == 0 {
                continue;
            }
            let Some(atoms) = balance_atoms.get(lane) else {
                return Ok(None);
            };
            if atoms.get(&pivot) > 0 && node.event_remaining.dominates(atoms) {
                reserve_search_work(
                    host_work,
                    HostWorkDimension::SearchStateBytes,
                    SEARCH_CANDIDATE_BYTES,
                )?;
                candidates.push((
                    std::cmp::Reverse(atoms.0.values().copied().sum()),
                    1,
                    *lane,
                    PhysicalCandidate::Balance {
                        lane: *lane,
                        custody: *custody,
                    },
                ));
            }
        }
        for (stack_id, cells) in stack_atoms {
            reserve_search_work(host_work, HostWorkDimension::SearchCandidates, 1)?;
            if node.event_stacks.contains(stack_id) {
                continue;
            }
            if born_available_after
                .get(stack_id)
                .is_some_and(|available_after| node.event_index <= *available_after)
            {
                continue;
            }
            let position = node
                .stack_positions
                .get(stack_id)
                .copied()
                .unwrap_or_default();
            let Some(atoms) = cells.get(position) else {
                continue;
            };
            if atoms.get(&pivot) > 0 && node.event_remaining.dominates(atoms) {
                reserve_search_work(
                    host_work,
                    HostWorkDimension::SearchStateBytes,
                    SEARCH_CANDIDATE_BYTES,
                )?;
                candidates.push((
                    std::cmp::Reverse(atoms.0.values().copied().sum()),
                    0,
                    *stack_id,
                    PhysicalCandidate::Stack(*stack_id),
                ));
            }
        }
        candidates.sort_by_key(|candidate| (candidate.0, candidate.1, candidate.2));
        work.push(PhysicalSearchWork::MarkFailed(state));

        for (_, _, _, candidate) in candidates.into_iter().rev() {
            let growth = match &candidate {
                PhysicalCandidate::Balance { .. } => SEARCH_RESOURCE_ENTRY_BYTES,
                PhysicalCandidate::Stack(_) => SEARCH_RESOURCE_ENTRY_BYTES
                    .checked_add(SEARCH_KEY_BYTES)
                    .ok_or(AuthorityError::ArithmeticOverflow)?,
            };
            reserve_search_state_with(host_work, || {
                search_node_bytes(&node)?
                    .checked_add(growth)
                    .ok_or(AuthorityError::ArithmeticOverflow)
            })?;
            let mut next = node.clone();
            let atoms = match candidate {
                PhysicalCandidate::Balance { lane, custody } => {
                    let Some(balances) = next
                        .balances
                        .checked_sub(&ResourceMultiset::singleton(custody, 1))
                        .ok()
                    else {
                        return Ok(None);
                    };
                    next.balances = balances;
                    let Some(event_balances) = next
                        .event_balances
                        .checked_add(&ResourceMultiset::singleton(lane, 1))
                        .ok()
                    else {
                        return Ok(None);
                    };
                    next.event_balances = event_balances;
                    let Some(atoms) = balance_atoms.get(&lane) else {
                        return Ok(None);
                    };
                    atoms.clone()
                }
                PhysicalCandidate::Stack(stack_id) => {
                    let position = next.stack_positions.entry(stack_id).or_default();
                    let Some(atoms) = stack_atoms
                        .get(&stack_id)
                        .and_then(|cells| cells.get(*position))
                    else {
                        return Ok(None);
                    };
                    let atoms = atoms.clone();
                    *position += 1;
                    next.event_stacks.insert(stack_id);
                    atoms
                }
            };
            let Some(event_remaining) = next.event_remaining.checked_sub(&atoms).ok() else {
                return Ok(None);
            };
            next.event_remaining = event_remaining;
            work.push(PhysicalSearchWork::Search(next));
        }
    }

    Ok(None)
}

pub fn allocate_physical_settlement(
    events: &[AuthorityEvent<[u8; 32]>],
    signatures: &BTreeMap<[u8; 32], CostSignature>,
    inventory: &AuthorityPhysicalInventory<[u8; 32]>,
) -> Result<AuthorityPhysicalSettlement<[u8; 32]>, AuthorityError> {
    allocate_physical_settlement_with_host_work(events, signatures, inventory, None)
}

pub fn allocate_physical_settlement_with_host_work(
    events: &[AuthorityEvent<[u8; 32]>],
    signatures: &BTreeMap<[u8; 32], CostSignature>,
    inventory: &AuthorityPhysicalInventory<[u8; 32]>,
    host_work: Option<&HostWorkBudget>,
) -> Result<AuthorityPhysicalSettlement<[u8; 32]>, AuthorityError> {
    let mut atom_signatures = BTreeMap::new();
    let mut expected = Vec::with_capacity(events.len());
    for event in events {
        event.verify_authority()?;
        let mut atoms = ResourceMultiset::default();
        for atom in event_atoms(event)? {
            add_signature_atoms(&mut atoms, &mut atom_signatures, &atom, 1)?;
        }
        expected.push(atoms);
    }
    let mut balance_atoms = BTreeMap::new();
    for key in inventory.balance_custody.keys() {
        let signature = signatures
            .get(key)
            .ok_or(AuthorityError::UnknownPhysicalSignature)?;
        if cost_signature_to_sig(signature)?.lane_hash() != *key {
            return Err(AuthorityError::EventSignatureConflict);
        }
        let mut atoms = ResourceMultiset::default();
        add_signature_atoms(&mut atoms, &mut atom_signatures, signature, 1)?;
        balance_atoms.insert(*key, atoms);
    }
    let mut stack_atoms = BTreeMap::new();
    for (stack_id, cells) in &inventory.stacks {
        let mut prepared = Vec::with_capacity(cells.len());
        for cell in cells {
            let mut atoms = ResourceMultiset::default();
            add_signature_atoms(&mut atoms, &mut atom_signatures, cell, 1)?;
            prepared.push(atoms);
        }
        stack_atoms.insert(*stack_id, prepared);
    }
    let event_positions = events
        .iter()
        .enumerate()
        .map(|(index, event)| (event.event_id, index))
        .collect::<BTreeMap<_, _>>();
    let mut born_available_after = BTreeMap::new();
    for (stack_id, produce_hash) in &inventory.born_stacks {
        let cells = inventory
            .stacks
            .get(stack_id)
            .ok_or(AuthorityError::UnknownStackResource)?;
        let mut available_after = None;
        for index in 0..cells.len() {
            let transfer = stack_transfer_event_id(produce_hash, index as u64);
            let position = *event_positions
                .get(&transfer)
                .ok_or(AuthorityError::SettlementPresentationMismatch)?;
            available_after =
                Some(available_after.map_or(position, |prior: usize| prior.max(position)));
        }
        born_available_after.insert(
            *stack_id,
            available_after.ok_or(AuthorityError::MissingSignature)?,
        );
    }
    let draws = search_physical_settlement(
        events,
        &expected,
        &balance_atoms,
        &inventory.balance_custody,
        &stack_atoms,
        0,
        expected.first().cloned().unwrap_or_default(),
        inventory.balances.clone(),
        BTreeMap::new(),
        ResourceMultiset::default(),
        BTreeSet::new(),
        &born_available_after,
        &mut BTreeSet::new(),
        host_work,
    )?
    .ok_or(AuthorityError::InsufficientAuthority)?;
    verify_physical_settlement(events, signatures, inventory, &draws)
}

pub fn apply_physical_settlement(
    inventory: &mut AuthorityPhysicalInventory<[u8; 32]>,
    settlement: &AuthorityPhysicalSettlement<[u8; 32]>,
) -> Result<(), AuthorityError> {
    inventory.balances = inventory.balances.checked_sub(&settlement.custody_debit)?;
    for (stack_id, pop_count) in &settlement.stack_pops {
        let cells = inventory
            .stacks
            .get_mut(stack_id)
            .ok_or(AuthorityError::UnknownStackResource)?;
        let pop_count =
            usize::try_from(*pop_count).map_err(|_| AuthorityError::ArithmeticOverflow)?;
        if pop_count > cells.len() {
            return Err(AuthorityError::ExhaustedStackResource);
        }
        cells.drain(..pop_count);
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnprovableDemand {
    RecursiveDequotation,
    DynamicAuthority,
    UnboundedControlFlow,
    UnsupportedSyntax,
}

impl UnprovableDemand {
    pub fn tag(&self) -> u8 {
        match self {
            Self::RecursiveDequotation => 0,
            Self::DynamicAuthority => 1,
            Self::UnboundedControlFlow => 2,
            Self::UnsupportedSyntax => 3,
        }
    }

    pub fn from_tag(tag: u8) -> Result<Self, AuthorityError> {
        match tag {
            0 => Ok(Self::RecursiveDequotation),
            1 => Ok(Self::DynamicAuthority),
            2 => Ok(Self::UnboundedControlFlow),
            3 => Ok(Self::UnsupportedSyntax),
            _ => Err(AuthorityError::InvalidUnprovableDemand),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DemandBound<K: Ord> {
    Exact(ResourceMultiset<K>),
    FiniteUpperBound {
        bound: ResourceMultiset<K>,
        proof: Vec<u8>,
    },
    Unprovable(UnprovableDemand),
}

impl<K: CanonicalAuthorityKey + Ord> DemandBound<K> {
    fn write_canonical(&self, output: &mut Vec<u8>) {
        match self {
            Self::Exact(bound) => {
                output.push(0);
                bound.write_canonical(output);
            }
            Self::FiniteUpperBound { bound, proof } => {
                output.push(1);
                bound.write_canonical(output);
                output.extend_from_slice(&(proof.len() as u64).to_le_bytes());
                output.extend_from_slice(proof);
            }
            Self::Unprovable(reason) => {
                output.push(2);
                output.push(reason.tag());
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FundingCertificate<K: Ord> {
    pub protocol_version: u32,
    pub program_hash: [u8; 32],
    pub pre_state_root: [u8; 32],
    pub reservation_id: [u8; 32],
    pub demand: DemandBound<K>,
    pub allocation: ResourceMultiset<K>,
    #[serde(default)]
    pub stack_reservations: BTreeMap<[u8; 32], u64>,
    #[serde(default)]
    pub fee_allocation: ResourceMultiset<K>,
    pub fee_plan: Option<super::monetary_allocation::MonetaryFeeEvidence>,
    #[serde(default)]
    pub fee_recipient: Vec<u8>,
    pub byte_cost_schedule_version: u32,
    pub byte_cost_schedule_digest: [u8; 32],
    pub byte_cost_bound: u64,
    pub byte_allocation: ResourceMultiset<K>,
}

impl<K: CanonicalAuthorityKey + Ord + Clone + Eq> FundingCertificate<K> {
    pub fn verify(
        &self,
        protocol_version: u32,
        program_hash: [u8; 32],
        pre_state_root: [u8; 32],
        available: &ResourceMultiset<K>,
    ) -> Result<(), AuthorityError> {
        self.verify_with(
            protocol_version,
            program_hash,
            pre_state_root,
            available,
            |_, _| false,
        )
    }

    pub fn verify_with<F>(
        &self,
        protocol_version: u32,
        program_hash: [u8; 32],
        pre_state_root: [u8; 32],
        available: &ResourceMultiset<K>,
        verify_finite_bound: F,
    ) -> Result<(), AuthorityError>
    where
        F: FnOnce(&ResourceMultiset<K>, &[u8]) -> bool,
    {
        self.verify_with_allocation(
            protocol_version,
            program_hash,
            pre_state_root,
            available,
            verify_finite_bound,
            |demand, allocation| demand == allocation,
        )
    }

    pub fn verify_with_allocation<F, A>(
        &self,
        protocol_version: u32,
        program_hash: [u8; 32],
        pre_state_root: [u8; 32],
        available: &ResourceMultiset<K>,
        verify_finite_bound: F,
        verify_allocation: A,
    ) -> Result<(), AuthorityError>
    where
        F: FnOnce(&ResourceMultiset<K>, &[u8]) -> bool,
        A: FnOnce(&ResourceMultiset<K>, &ResourceMultiset<K>) -> bool,
    {
        if self.protocol_version != protocol_version {
            return Err(AuthorityError::ProtocolVersionMismatch);
        }
        if self.program_hash != program_hash {
            return Err(AuthorityError::ProgramHashMismatch);
        }
        if self.pre_state_root != pre_state_root {
            return Err(AuthorityError::PreStateMismatch);
        }
        if self.byte_cost_schedule_version != super::byte_accounting::BYTE_COST_SCHEDULE_VERSION
            || self.byte_cost_schedule_digest != super::byte_accounting::byte_cost_schedule_digest()
        {
            return Err(AuthorityError::ProtocolVersionMismatch);
        }
        self.verify_stack_reservations()?;
        let reservation = match &self.demand {
            DemandBound::Exact(bound) => bound,
            DemandBound::FiniteUpperBound { bound, proof } if proof.is_empty() => {
                return Err(AuthorityError::MissingBoundProof);
            }
            DemandBound::FiniteUpperBound { bound, proof } if verify_finite_bound(bound, proof) => {
                bound
            }
            DemandBound::FiniteUpperBound { .. } => {
                return Err(AuthorityError::InvalidBoundProof);
            }
            DemandBound::Unprovable(_) => return Err(AuthorityError::UnprovableDemand),
        };
        if !verify_allocation(reservation, &self.allocation) {
            return Err(AuthorityError::AllocationMismatch);
        }
        let total_allocation = self
            .allocation
            .checked_add(&self.byte_allocation)?
            .checked_add(&self.fee_allocation)?;
        if !available.dominates(&total_allocation) {
            return Err(AuthorityError::InsufficientAuthority);
        }
        Ok(())
    }

    pub fn verify_with_custody<F, A>(
        &self,
        protocol_version: u32,
        program_hash: [u8; 32],
        pre_state_root: [u8; 32],
        available: &ResourceMultiset<K>,
        balance_custody: &BTreeMap<K, K>,
        verify_finite_bound: F,
        verify_allocation: A,
    ) -> Result<(), AuthorityError>
    where
        F: FnOnce(&ResourceMultiset<K>, &[u8]) -> bool,
        A: FnOnce(&ResourceMultiset<K>, &ResourceMultiset<K>) -> bool,
    {
        let logical_available = logical_balance_view(available, balance_custody);
        self.verify_with_allocation(
            protocol_version,
            program_hash,
            pre_state_root,
            &logical_available,
            verify_finite_bound,
            verify_allocation,
        )?;
        let total_allocation = self
            .allocation
            .checked_add(&self.byte_allocation)?
            .checked_add(&self.fee_allocation)?;
        let custody_allocation = physicalize_balance_debit(&total_allocation, balance_custody)?;
        if !available.dominates(&custody_allocation) {
            return Err(AuthorityError::InsufficientAuthority);
        }
        Ok(())
    }

    pub fn verify_stack_reservations(&self) -> Result<(), AuthorityError> {
        if self.stack_reservations.values().any(|count| *count == 0) {
            return Err(AuthorityError::SettlementPresentationMismatch);
        }
        Ok(())
    }

    pub fn certificate_id(&self) -> [u8; 32] {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(CERTIFICATE_DOMAIN);
        bytes.extend_from_slice(&self.protocol_version.to_le_bytes());
        bytes.extend_from_slice(&self.program_hash);
        bytes.extend_from_slice(&self.pre_state_root);
        bytes.extend_from_slice(&self.reservation_id);
        self.demand.write_canonical(&mut bytes);
        self.allocation.write_canonical(&mut bytes);
        bytes.extend_from_slice(&(self.stack_reservations.len() as u64).to_le_bytes());
        for (stack_id, count) in &self.stack_reservations {
            bytes.extend_from_slice(stack_id);
            bytes.extend_from_slice(&count.to_le_bytes());
        }
        self.fee_allocation.write_canonical(&mut bytes);
        match &self.fee_plan {
            Some(plan) => {
                bytes.push(1);
                plan.write_canonical(&mut bytes);
            }
            None => bytes.push(0),
        }
        bytes.extend_from_slice(&(self.fee_recipient.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&self.fee_recipient);
        bytes.extend_from_slice(&self.byte_cost_schedule_version.to_le_bytes());
        bytes.extend_from_slice(&self.byte_cost_schedule_digest);
        bytes.extend_from_slice(&self.byte_cost_bound.to_le_bytes());
        self.byte_allocation.write_canonical(&mut bytes);
        Blake2b256::hash(bytes)
            .try_into()
            .expect("Blake2b-256 digest length")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityEvent<K: Ord> {
    pub event_id: [u8; 32],
    pub authority: CostAuthority,
    pub debit: ResourceMultiset<K>,
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize
)]
pub enum AuthorityByteEventKind {
    ProduceIntroduction,
    ConsumeIntroduction,
    Comm,
}

impl AuthorityByteEventKind {
    pub fn tag(self) -> u8 {
        match self {
            Self::ProduceIntroduction => 0,
            Self::ConsumeIntroduction => 1,
            Self::Comm => 2,
        }
    }

    pub fn from_tag(tag: u8) -> Result<Self, AuthorityError> {
        match tag {
            0 => Ok(Self::ProduceIntroduction),
            1 => Ok(Self::ConsumeIntroduction),
            2 => Ok(Self::Comm),
            _ => Err(AuthorityError::InvalidByteEventKind),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityByteEvent {
    pub event_id: [u8; 32],
    pub kind: AuthorityByteEventKind,
    pub authority: CostAuthority,
    pub amount: u64,
}

impl AuthorityByteEvent {
    pub fn verify_authority(&self) -> Result<(), AuthorityError> {
        if self.amount == 0 {
            return Err(AuthorityError::InvalidByteEventAmount);
        }
        if canonical_authority(&self.authority)? != self.authority {
            return Err(AuthorityError::NonCanonicalAuthority);
        }
        Ok(())
    }

    fn funding_event(&self) -> Result<AuthorityEvent<[u8; 32]>, AuthorityError> {
        Ok(AuthorityEvent {
            event_id: self.event_id,
            authority: self.authority.clone(),
            debit: authority_demand(&self.authority)?,
        })
    }

    pub(super) fn canonical_key(&self) -> Vec<u8> {
        let mut key = Vec::new();
        key.extend_from_slice(&self.event_id);
        key.push(self.kind.tag());
        let encoded = self.authority.encode_to_vec();
        key.extend_from_slice(&(encoded.len() as u64).to_le_bytes());
        key.extend_from_slice(&encoded);
        key.extend_from_slice(&self.amount.to_le_bytes());
        key
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityStackBirth {
    pub produce_hash: [u8; 32],
    pub cells: Vec<CostSignature>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityBornStack {
    pub stack_id: [u8; 32],
    pub produce_hash: [u8; 32],
    pub cells: Vec<CostSignature>,
}

impl AuthorityEvent<[u8; 32]> {
    pub fn verify_authority(&self) -> Result<(), AuthorityError> {
        if canonical_authority(&self.authority)? != self.authority {
            return Err(AuthorityError::NonCanonicalAuthority);
        }
        let declared = authority_demand(&self.authority)?;
        if declared != self.debit {
            return Err(AuthorityError::EventDebitMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityCostWitness<K: Ord> {
    pub protocol_version: u32,
    pub certificate_id: [u8; 32],
    pub pre_state_root: [u8; 32],
    pub post_state_root: [u8; 32],
    pub events: Vec<AuthorityEvent<K>>,
    pub realized: ResourceMultiset<K>,
    pub settlement: ResourceMultiset<K>,
    #[serde(default)]
    pub physical_draws: Vec<AuthorityPhysicalEventDraw<K>>,
    #[serde(default)]
    pub born_stacks: Vec<AuthorityBornStack>,
    pub byte_cost_schedule_version: u32,
    pub byte_cost_schedule_digest: [u8; 32],
    pub byte_events: Vec<AuthorityByteEvent>,
    pub byte_cost: u64,
    pub byte_settlement: ResourceMultiset<K>,
}

impl<K: CanonicalAuthorityKey + Ord + Clone + Eq> AuthorityCostWitness<K> {
    pub fn verify_structure(&self) -> Result<(), AuthorityError> {
        let mut identities = BTreeSet::new();
        let mut realized = ResourceMultiset::default();
        for event in &self.events {
            if !identities.insert(event.event_id) {
                return Err(AuthorityError::NonCanonicalEventOrder);
            }
            realized = realized.checked_add(&event.debit)?;
        }
        if realized != self.realized {
            return Err(AuthorityError::RealizedCostMismatch);
        }
        let mut byte_identities = BTreeMap::new();
        let mut byte_cost = 0_u64;
        let mut previous_byte_key = None;
        for event in &self.byte_events {
            event.verify_authority()?;
            let key = event.canonical_key();
            if previous_byte_key
                .as_ref()
                .is_some_and(|previous| previous > &key)
            {
                return Err(AuthorityError::NonCanonicalEventOrder);
            }
            previous_byte_key = Some(key);
            match byte_identities.get(&event.event_id) {
                Some(existing)
                    if existing != &(event.kind, event.authority.clone(), event.amount) =>
                {
                    return Err(AuthorityError::EventIdentityConflict);
                }
                Some(_) => {}
                None => {
                    byte_identities.insert(
                        event.event_id,
                        (event.kind, event.authority.clone(), event.amount),
                    );
                }
            }
            byte_cost = byte_cost
                .checked_add(event.amount)
                .ok_or(AuthorityError::ArithmeticOverflow)?;
        }
        if byte_cost != self.byte_cost {
            return Err(AuthorityError::ByteCostMismatch);
        }
        if !self.physical_draws.is_empty()
            && (self.physical_draws.len() != self.events.len()
                || self
                    .events
                    .iter()
                    .zip(&self.physical_draws)
                    .any(|(event, draw)| {
                        event.event_id != draw.event_id
                            || draw.stack_ids.windows(2).any(|pair| pair[0] >= pair[1])
                    }))
        {
            return Err(AuthorityError::SettlementPresentationMismatch);
        }
        if self
            .born_stacks
            .windows(2)
            .any(|pair| pair[0].stack_id >= pair[1].stack_id)
        {
            return Err(AuthorityError::SettlementPresentationMismatch);
        }
        let event_ids = self
            .events
            .iter()
            .map(|event| event.event_id)
            .collect::<BTreeSet<_>>();
        for birth in &self.born_stacks {
            if birth.cells.is_empty() {
                return Err(AuthorityError::MissingSignature);
            }
            for (index, cell) in birth.cells.iter().enumerate() {
                let cell = canonical_cost_signature(cell)?;
                if cost_signature_to_sig(&cell)? == Sig::Unit {
                    return Err(AuthorityError::NonCanonicalSignature);
                }
                if !event_ids.contains(&stack_transfer_event_id(&birth.produce_hash, index as u64))
                {
                    return Err(AuthorityError::SettlementPresentationMismatch);
                }
            }
        }
        Ok(())
    }

    pub fn verify(&self, certificate: &FundingCertificate<K>) -> Result<(), AuthorityError> {
        self.verify_with_settlement(certificate, |_, realized, _| Ok(realized.clone()))
    }

    pub fn verify_with_settlement<F>(
        &self,
        certificate: &FundingCertificate<K>,
        settle: F,
    ) -> Result<(), AuthorityError>
    where
        F: FnOnce(
            &[AuthorityEvent<K>],
            &ResourceMultiset<K>,
            &ResourceMultiset<K>,
        ) -> Result<ResourceMultiset<K>, AuthorityError>,
    {
        if self.protocol_version != certificate.protocol_version {
            return Err(AuthorityError::ProtocolVersionMismatch);
        }
        if self.byte_cost_schedule_version != certificate.byte_cost_schedule_version
            || self.byte_cost_schedule_digest != certificate.byte_cost_schedule_digest
            || self.byte_cost > certificate.byte_cost_bound
            || !certificate.byte_allocation.dominates(&self.byte_settlement)
        {
            return Err(AuthorityError::SettlementMismatch);
        }
        if self.certificate_id != certificate.certificate_id() {
            return Err(AuthorityError::CertificateMismatch);
        }
        if self.pre_state_root != certificate.pre_state_root {
            return Err(AuthorityError::PreStateMismatch);
        }
        certificate.verify_stack_reservations()?;
        self.verify_structure()?;
        let demand = match &certificate.demand {
            DemandBound::Exact(bound) | DemandBound::FiniteUpperBound { bound, .. } => bound,
            DemandBound::Unprovable(_) => return Err(AuthorityError::UnprovableDemand),
        };
        if !demand.dominates(&self.realized) {
            return Err(AuthorityError::RealizedCostExceedsReservation);
        }
        let expected_settlement = settle(&self.events, &self.realized, &certificate.allocation)?;
        if expected_settlement != self.settlement {
            return Err(AuthorityError::SettlementMismatch);
        }
        if !certificate.allocation.dominates(&self.settlement) {
            return Err(AuthorityError::SettlementExceedsReservation);
        }
        let mut stack_pops = BTreeMap::<[u8; 32], u64>::new();
        for draw in &self.physical_draws {
            for stack_id in &draw.stack_ids {
                let count = stack_pops.entry(*stack_id).or_default();
                *count = count
                    .checked_add(1)
                    .ok_or(AuthorityError::ArithmeticOverflow)?;
            }
        }
        if stack_pops.iter().any(|(stack_id, count)| {
            let reserved = certificate
                .stack_reservations
                .get(stack_id)
                .copied()
                .unwrap_or_default();
            let born = self
                .born_stacks
                .iter()
                .find(|birth| birth.stack_id == *stack_id)
                .map(|birth| birth.cells.len() as u64)
                .unwrap_or_default();
            reserved
                .checked_add(born)
                .is_none_or(|available| available < *count)
        }) {
            return Err(AuthorityError::SettlementExceedsReservation);
        }
        Ok(())
    }

    pub fn refund(
        &self,
        certificate: &FundingCertificate<K>,
    ) -> Result<ResourceMultiset<K>, AuthorityError> {
        certificate
            .allocation
            .checked_sub(&self.settlement)?
            .checked_add(
                &certificate
                    .byte_allocation
                    .checked_sub(&self.byte_settlement)?,
            )
    }

    pub fn witness_id(&self) -> [u8; 32] {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(WITNESS_DOMAIN);
        bytes.extend_from_slice(&self.protocol_version.to_le_bytes());
        bytes.extend_from_slice(&self.certificate_id);
        bytes.extend_from_slice(&self.pre_state_root);
        bytes.extend_from_slice(&self.post_state_root);
        bytes.extend_from_slice(&(self.events.len() as u64).to_le_bytes());
        for event in &self.events {
            bytes.extend_from_slice(&event.event_id);
            let authority = canonical_authority(&event.authority)
                .expect("verified authority witness contains canonical authority");
            let encoded = authority.encode_to_vec();
            bytes.extend_from_slice(&(encoded.len() as u64).to_le_bytes());
            bytes.extend_from_slice(&encoded);
            event.debit.write_canonical(&mut bytes);
        }
        self.realized.write_canonical(&mut bytes);
        self.settlement.write_canonical(&mut bytes);
        bytes.extend_from_slice(&(self.physical_draws.len() as u64).to_le_bytes());
        for draw in &self.physical_draws {
            bytes.extend_from_slice(&draw.event_id);
            draw.balances.write_canonical(&mut bytes);
            bytes.extend_from_slice(&(draw.stack_ids.len() as u64).to_le_bytes());
            for stack_id in &draw.stack_ids {
                bytes.extend_from_slice(stack_id);
            }
        }
        bytes.extend_from_slice(&(self.born_stacks.len() as u64).to_le_bytes());
        for birth in &self.born_stacks {
            bytes.extend_from_slice(&birth.stack_id);
            bytes.extend_from_slice(&birth.produce_hash);
            bytes.extend_from_slice(&(birth.cells.len() as u64).to_le_bytes());
            for cell in &birth.cells {
                let encoded = canonical_cost_signature(cell)
                    .expect("verified authority witness contains canonical born stack")
                    .encode_to_vec();
                bytes.extend_from_slice(&(encoded.len() as u64).to_le_bytes());
                bytes.extend_from_slice(&encoded);
            }
        }
        bytes.extend_from_slice(&self.byte_cost_schedule_version.to_le_bytes());
        bytes.extend_from_slice(&self.byte_cost_schedule_digest);
        bytes.extend_from_slice(&(self.byte_events.len() as u64).to_le_bytes());
        for event in &self.byte_events {
            bytes.extend_from_slice(&event.canonical_key());
        }
        bytes.extend_from_slice(&self.byte_cost.to_le_bytes());
        self.byte_settlement.write_canonical(&mut bytes);
        Blake2b256::hash(bytes)
            .try_into()
            .expect("Blake2b-256 digest length")
    }
}

impl AuthorityCostWitness<[u8; 32]> {
    pub fn verify_event_authorities(&self) -> Result<(), AuthorityError> {
        self.events
            .iter()
            .try_for_each(AuthorityEvent::verify_authority)?;
        self.byte_events
            .iter()
            .try_for_each(AuthorityByteEvent::verify_authority)
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AuthorityError {
    #[error("host work limit rejected authority processing")]
    HostWorkRejected,
    #[error("cost authority is missing a signature")]
    MissingSignature,
    #[error("a billable communication has no cost-authority wrapper")]
    MissingAuthority,
    #[error("cost authority contains an unresolved bound signature")]
    UnresolvedBoundLevel,
    #[error("cost authority contains a malformed compound signature")]
    MalformedCompound,
    #[error("cost authority contains a non-canonical signature")]
    NonCanonicalSignature,
    #[error("cost authority regions are not in canonical order")]
    NonCanonicalAuthority,
    #[error("runtime funding signature cannot be represented by the cost-accounting grammar")]
    UnsupportedFundingSignature,
    #[error("cost authority region identity must be exactly 32 bytes")]
    InvalidRegionIdentity,
    #[error("cost authority region identity maps to conflicting signatures")]
    RegionIdentityConflict,
    #[error("one COMM identity maps to conflicting authority demands")]
    EventIdentityConflict,
    #[error("one authority lane maps to conflicting canonical signatures")]
    EventSignatureConflict,
    #[error("a COMM debit is not justified by its wrapper authority")]
    EventDebitMismatch,
    #[error("a byte-accounting event kind is not recognized")]
    InvalidByteEventKind,
    #[error("a byte-accounting event amount must be positive")]
    InvalidByteEventAmount,
    #[error("the byte-accounting event sum differs from the declared quantitative cost")]
    ByteCostMismatch,
    #[error("authority arithmetic overflow")]
    ArithmeticOverflow,
    #[error("insufficient authority")]
    InsufficientAuthority,
    #[error("demand has no finite proof")]
    UnprovableDemand,
    #[error("unprovable-demand reason is not recognized by this protocol version")]
    InvalidUnprovableDemand,
    #[error("finite demand bound is missing its proof")]
    MissingBoundProof,
    #[error("finite demand bound proof is invalid")]
    InvalidBoundProof,
    #[error("protocol version mismatch")]
    ProtocolVersionMismatch,
    #[error("program hash mismatch")]
    ProgramHashMismatch,
    #[error("pre-state root mismatch")]
    PreStateMismatch,
    #[error("certificate allocation does not satisfy its proven demand")]
    AllocationMismatch,
    #[error("cost witness references a different funding certificate")]
    CertificateMismatch,
    #[error("authority event identities are not unique")]
    NonCanonicalEventOrder,
    #[error("realized cost does not equal the event fold")]
    RealizedCostMismatch,
    #[error("realized cost exceeds the reserved authority")]
    RealizedCostExceedsReservation,
    #[error("physical settlement does not match the realized authority events")]
    SettlementMismatch,
    #[error("physical settlement exceeds the reserved purse cells")]
    SettlementExceedsReservation,
    #[error("physical settlement presentation does not correspond to its authority events")]
    SettlementPresentationMismatch,
    #[error("physical settlement contains a non-canonical stack draw")]
    NonCanonicalStackDraw,
    #[error("physical settlement references an unknown signature")]
    UnknownPhysicalSignature,
    #[error("physical settlement references an unknown custody identity")]
    UnknownPhysicalCustody,
    #[error("logical authority aliases disagree on their physical custody state")]
    PhysicalCustodyMismatch,
    #[error("physical settlement references an unknown stack resource")]
    UnknownStackResource,
    #[error("physical settlement attempts to pop an exhausted stack resource")]
    ExhaustedStackResource,
    #[error("physical settlement does not exactly realize the event authority")]
    PhysicalAuthorityMismatch,
}

impl From<models::rust::host_work::HostWorkReservationError> for AuthorityError {
    fn from(_: models::rust::host_work::HostWorkReservationError) -> Self { Self::HostWorkRejected }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::cost_signature::Value as CostSignatureValue;
    use models::rhoapi::g_unforgeable::UnfInstance;
    use models::rhoapi::{CostAuthority, CostRegion, CostSignature, GPrivate, GUnforgeable, Par};
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits, HostWorkUsage};
    use proptest::prelude::*;

    use super::*;

    fn funding_tree_with_atoms() -> impl Strategy<Value = (Sig, Vec<Vec<u8>>)> {
        (0_u8..5, any::<bool>())
            .prop_map(|(atom, quoted)| {
                if atom == 0 {
                    (Sig::Unit, Vec::new())
                } else {
                    let bytes = vec![atom];
                    let signature = if quoted {
                        Sig::Quote(bytes.clone())
                    } else {
                        Sig::Ground(bytes.clone())
                    };
                    (signature, vec![bytes])
                }
            })
            .prop_recursive(4, 64, 2, |inner| {
                (inner.clone(), inner).prop_map(|((left, mut atoms), (right, others))| {
                    atoms.extend(others);
                    (Sig::And(Box::new(left), Box::new(right)), atoms)
                })
            })
    }

    fn byte_schedule_version() -> u32 { super::super::byte_accounting::BYTE_COST_SCHEDULE_VERSION }

    fn byte_schedule_digest() -> [u8; 32] {
        super::super::byte_accounting::byte_cost_schedule_digest()
    }

    fn certificate(allocation: ResourceMultiset<[u8; 32]>) -> FundingCertificate<[u8; 32]> {
        FundingCertificate {
            protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
            program_hash: [1; 32],
            pre_state_root: [2; 32],
            reservation_id: [3; 32],
            demand: DemandBound::Exact(allocation.clone()),
            allocation,
            stack_reservations: BTreeMap::new(),
            fee_allocation: ResourceMultiset::default(),
            fee_plan: None,
            fee_recipient: Vec::new(),
            byte_cost_schedule_version: byte_schedule_version(),
            byte_cost_schedule_digest: byte_schedule_digest(),
            byte_cost_bound: 0,
            byte_allocation: ResourceMultiset::default(),
        }
    }

    #[test]
    fn funding_certificate_id_matches_python_client_golden_vector() {
        use super::super::monetary_allocation::{MonetaryFeeEvidence, MonetaryFeeFields};

        let mut certificate = FundingCertificate {
            protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
            program_hash: [b'm'; 32],
            pre_state_root: [b'p'; 32],
            reservation_id: [b'r'; 32],
            demand: DemandBound::Exact(ResourceMultiset::singleton([b's'; 32], 2)),
            allocation: ResourceMultiset::singleton([b's'; 32], 2),
            stack_reservations: BTreeMap::from([([b'k'; 32], 1)]),
            fee_allocation: ResourceMultiset::singleton([b'g'; 32], 1),
            fee_plan: None,
            fee_recipient: b"proposer".to_vec(),
            byte_cost_schedule_version: byte_schedule_version(),
            byte_cost_schedule_digest: byte_schedule_digest(),
            byte_cost_bound: 0,
            byte_allocation: ResourceMultiset::default(),
        };

        assert_eq!(
            hex::encode(certificate.certificate_id()),
            "093145bb99125f8918e7c93f711a6164c7f8cfc6d166a9f4657b1c8a19410205"
        );
        certificate.fee_plan = Some(
            MonetaryFeeEvidence::try_from(MonetaryFeeFields {
                policy_version: 1,
                policy_context: hex::decode(
                    "5d2e52ab4952964ed53953a58c6a84b4bde70f7416f0422376523a99d1f07e00",
                )
                .unwrap()
                .try_into()
                .unwrap(),
                scope: hex::decode(
                    "42c6a8c9435cf30c7e388e7f0e1c31c6910ec375a467536373638a272a364d01",
                )
                .unwrap()
                .try_into()
                .unwrap(),
                payer_custodies: vec![[b'g'; 32], [b'h'; 32]],
                obligation: 1,
                expected_revision: 0,
                expected_position: 0,
                next_revision: 1,
                next_position: 1,
            })
            .unwrap(),
        );
        assert_eq!(
            hex::encode(certificate.certificate_id()),
            "8baf872246032dda3ce22fa51ad22371420acd16256b6b593e68492e58c71257"
        );
    }

    fn ground(bytes: &[u8]) -> CostSignature {
        CostSignature {
            value: Some(CostSignatureValue::Ground(bytes.to_vec())),
        }
    }

    fn private_name(bytes: &[u8]) -> CostSignature {
        CostSignature {
            value: Some(CostSignatureValue::Name(Par::default().with_unforgeables(
                vec![GUnforgeable {
                    unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: bytes.to_vec() })),
                }],
            ))),
        }
    }

    fn event(signatures: &[CostSignature]) -> AuthorityEvent<[u8; 32]> {
        let authority = canonical_authority(&CostAuthority {
            regions: signatures
                .iter()
                .enumerate()
                .map(|(index, signature)| {
                    cost_region(signature, b"authority allocation test", index as u32).unwrap()
                })
                .collect(),
        })
        .unwrap();
        AuthorityEvent {
            event_id: [21; 32],
            debit: authority_demand(&authority).unwrap(),
            authority,
        }
    }

    fn byte_event(
        event_id: u8,
        kind: AuthorityByteEventKind,
        signatures: &[CostSignature],
        amount: u64,
    ) -> AuthorityByteEvent {
        let event = event(signatures);
        AuthorityByteEvent {
            event_id: [event_id; 32],
            kind,
            authority: event.authority,
            amount,
        }
    }

    #[test]
    fn authority_merge_is_canonical_deduplicating_and_conflict_rejecting() {
        let first = cost_region(&ground(b"a"), b"redex", 0).unwrap();
        let second = cost_region(&ground(b"b"), b"redex", 1).unwrap();
        let left = CostAuthority {
            regions: vec![second.clone(), first.clone(), first.clone()],
        };
        let right = CostAuthority {
            regions: vec![first.clone(), second.clone()],
        };
        let merged = merge_authorities([&left, &right]).unwrap();
        assert_eq!(merged.regions.len(), 2);
        assert!(merged.regions[0].instance_id < merged.regions[1].instance_id);

        let conflict = CostAuthority {
            regions: vec![CostRegion {
                instance_id: first.instance_id,
                signature: Some(ground(b"different")),
            }],
        };
        assert_eq!(
            merge_authorities([&merged, &conflict]),
            Err(AuthorityError::RegionIdentityConflict)
        );
    }

    #[test]
    fn metered_authority_canonicalization_matches_legacy_and_rejects_before_copy() {
        let first = cost_region(&ground(b"a"), b"metered authority", 0).unwrap();
        let second = cost_region(&private_name(b"b"), b"metered authority", 1).unwrap();
        let authority = CostAuthority {
            regions: vec![second.clone(), first.clone(), second.clone()],
        };
        let unlimited = |_: usize, _: usize, _: usize| Ok(());
        assert_eq!(
            canonical_authority_metered(&authority, &unlimited),
            canonical_authority(&authority)
        );
        assert_eq!(
            merge_authorities_metered([&authority], &unlimited),
            merge_authorities([&authority])
        );
        assert_eq!(
            authority_regions_metered(&authority, &unlimited),
            authority_regions(&authority)
        );
        let persistent = BTreeSet::from([second.instance_id.as_slice().try_into().unwrap()]);
        assert_eq!(
            instantiate_persistent_regions_metered(&authority, &persistent, [5; 32], &unlimited),
            instantiate_persistent_regions(&authority, &persistent, [5; 32])
        );

        let conflict = CostAuthority {
            regions: vec![first.clone(), CostRegion {
                instance_id: first.instance_id,
                signature: Some(ground(b"different")),
            }],
        };
        assert_eq!(
            canonical_authority_metered(&conflict, &unlimited),
            Err(AuthorityError::RegionIdentityConflict)
        );

        let large = CostAuthority {
            regions: vec![CostRegion {
                instance_id: vec![7; 32],
                signature: Some(ground(&vec![9; 4096])),
            }],
        };
        let limited = |_: usize, _: usize, backing: usize| {
            if backing > 1024 {
                Err(BackingError::Rejected)
            } else {
                Ok(())
            }
        };
        assert_eq!(
            canonical_authority_metered(&large, &limited),
            Err(AuthorityError::HostWorkRejected)
        );
        assert_eq!(
            merge_authorities_metered([&large], &limited),
            Err(AuthorityError::HostWorkRejected)
        );
    }

    #[test]
    fn metered_authority_demand_matches_lanes_and_rejects_short_budget() {
        use std::cell::Cell;

        let unit = CostSignature {
            value: Some(CostSignatureValue::Unit(true)),
        };
        let compound =
            signature_from_atoms(&[ground(b"a"), private_name(b"name"), ground(b"b")]).unwrap();
        let signatures = [
            ground(b"a"),
            ground(b"a"),
            private_name(b"name"),
            CostSignature {
                value: Some(CostSignatureValue::Quote(Par::default())),
            },
            compound,
            unit,
        ];
        let authority = CostAuthority {
            regions: signatures
                .iter()
                .enumerate()
                .map(|(index, signature)| {
                    cost_region(signature, b"metered demand", index as u32).unwrap()
                })
                .collect(),
        };
        let used = Cell::new([0usize; 3]);
        let full = |operations: usize, scanned: usize, backing: usize| {
            let mut next = used.get();
            for (total, amount) in next.iter_mut().zip([operations, scanned, backing]) {
                *total = total.checked_add(amount).unwrap();
            }
            used.set(next);
            Ok(())
        };
        assert_eq!(
            authority_demand_metered(&authority, &full),
            authority_demand(&authority)
        );
        let required = used.get();
        let meter = SorterMeter::new(&full);
        for region in &authority.regions {
            let signature = region.signature.as_ref().unwrap();
            assert_eq!(
                cost_signature_lane_metered(signature, &meter).unwrap(),
                match cost_signature_to_sig(signature).unwrap() {
                    Sig::Unit => None,
                    sig => Some(sig.lane_hash()),
                }
            );
        }
        assert!(required.iter().all(|value| *value > 0));
        for dimension in 0..3 {
            let mut limit = required;
            limit[dimension] -= 1;
            let spent = Cell::new([0usize; 3]);
            let short = |operations: usize, scanned: usize, backing: usize| {
                let mut next = spent.get();
                for (total, amount) in next.iter_mut().zip([operations, scanned, backing]) {
                    *total = total.checked_add(amount).ok_or(BackingError::Overflow)?;
                }
                if next.iter().zip(limit).any(|(value, max)| *value > max) {
                    return Err(BackingError::Rejected);
                }
                spent.set(next);
                Ok(())
            };
            assert_eq!(
                authority_demand_metered(&authority, &short),
                Err(AuthorityError::HostWorkRejected)
            );
        }
    }

    #[test]
    fn metered_funding_channel_matches_historical_reflection() {
        let signatures = [
            Sig::Unit,
            Sig::Ground(b"a".to_vec()),
            Sig::Quote(b"a".to_vec()),
            Sig::And(
                Box::new(Sig::And(
                    Box::new(Sig::Ground(b"b".to_vec())),
                    Box::new(Sig::Unit),
                )),
                Box::new(Sig::And(
                    Box::new(Sig::Quote(b"a".to_vec())),
                    Box::new(Sig::Ground(b"a".to_vec())),
                )),
            ),
        ];
        let full = |_: usize, _: usize, _: usize| Ok(());
        for signature in &signatures {
            assert_eq!(
                funding_sig_channel_metered(signature, &full),
                Ok(super::super::SignatureChannel::from_sig(signature).par)
            );
        }
        let reject_backing = |_: usize, _: usize, bytes: usize| {
            if bytes > 0 {
                Err(BackingError::Rejected)
            } else {
                Ok(())
            }
        };
        assert_eq!(
            funding_sig_channel_metered(&signatures[1], &reject_backing),
            Err(AuthorityError::HostWorkRejected)
        );
        assert_eq!(
            funding_sig_channel_metered(
                &Sig::Plus(Box::new(Sig::Unit), Box::new(Sig::Unit)),
                &full
            ),
            Err(AuthorityError::UnsupportedFundingSignature)
        );
    }

    #[test]
    fn metered_cost_signature_conversion_preserves_balanced_compound() {
        let signatures = [
            CostSignature {
                value: Some(CostSignatureValue::Unit(true)),
            },
            ground(b"a"),
            private_name(b"name"),
            CostSignature {
                value: Some(CostSignatureValue::Quote(Par::default())),
            },
            signature_from_atoms(&[
                ground(b"a"),
                ground(b"b"),
                ground(b"c"),
                ground(b"d"),
                ground(b"e"),
            ])
            .unwrap(),
        ];
        let full = |_: usize, _: usize, _: usize| Ok(());
        for signature in &signatures {
            assert_eq!(
                cost_signature_to_sig_metered(signature, &full),
                cost_signature_to_sig(signature)
            );
        }
        let reject_backing = |_: usize, _: usize, bytes: usize| {
            if bytes > 0 {
                Err(BackingError::Rejected)
            } else {
                Ok(())
            }
        };
        assert_eq!(
            cost_signature_to_sig_metered(&signatures[4], &reject_backing),
            Err(AuthorityError::HostWorkRejected)
        );
    }

    #[test]
    fn metered_stack_transfer_event_id_preserves_preimage_and_rejects_budget() {
        let produce_hash = [0x5a; 32];
        let full = |_: usize, _: usize, _: usize| Ok(());
        for cell_index in [0, 1, u64::MAX] {
            assert_eq!(
                stack_transfer_event_id_metered(&produce_hash, cell_index, &full),
                Ok(stack_transfer_event_id(&produce_hash, cell_index))
            );
        }
        let reject_backing = |_: usize, _: usize, bytes: usize| {
            if bytes > 0 {
                Err(BackingError::Rejected)
            } else {
                Ok(())
            }
        };
        assert_eq!(
            stack_transfer_event_id_metered(&produce_hash, 7, &reject_backing),
            Err(AuthorityError::HostWorkRejected)
        );
    }

    #[test]
    fn authority_discovery_counts_all_roots_and_maximum_depth() {
        let event = event(&[ground(b"a"), ground(b"b")]);
        let host_work = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(16)));

        authority_funding_signatures_for_events_with_host_work(
            std::slice::from_ref(&event),
            &[],
            &[],
            Some(&host_work),
        )
        .unwrap();

        assert_eq!(
            host_work.usage(HostWorkDimension::AuthorityNodes),
            HostWorkUsage::new(2)
        );
        assert_eq!(
            host_work.usage(HostWorkDimension::AuthorityDepth),
            HostWorkUsage::new(1)
        );
    }

    #[test]
    fn authority_depth_rejects_before_recursive_validation() {
        let nested = CostSignature {
            value: Some(CostSignatureValue::Compound(CostSignatureCompound {
                elements: vec![ground(b"a"), CostSignature {
                    value: Some(CostSignatureValue::Compound(CostSignatureCompound {
                        elements: vec![ground(b"b"), ground(b"c")],
                    })),
                }],
            })),
        };
        let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(16));
        limits.set(HostWorkDimension::AuthorityDepth, HostWorkLimit::new(2));
        let host_work = HostWorkBudget::new(limits);

        assert_eq!(
            authority_funding_signatures_for_events_with_host_work(
                &[],
                &[],
                std::slice::from_ref(&nested),
                Some(&host_work),
            ),
            Err(AuthorityError::HostWorkRejected)
        );
        assert_eq!(
            host_work.usage(HostWorkDimension::AuthorityDepth),
            HostWorkUsage::new(2)
        );
        assert!(host_work.is_rejected());
    }

    #[test]
    fn event_debit_must_exactly_match_declared_authority() {
        let event = event(&[ground(b"a"), ground(b"b")]);
        event.verify_authority().unwrap();

        let mut weakened = event.clone();
        weakened.debit.0.pop_last();
        assert_eq!(
            weakened.verify_authority(),
            Err(AuthorityError::EventDebitMismatch)
        );

        let mut amplified = event;
        let key = *amplified.debit.0.keys().next().unwrap();
        amplified.debit.0.insert(key, 2);
        assert_eq!(
            amplified.verify_authority(),
            Err(AuthorityError::EventDebitMismatch)
        );
    }

    #[test]
    fn serialized_private_name_presentation_cannot_create_event_authority() {
        let signer = ground(b"signed-event-authority");
        let exposed_name = private_name(&[7; 32]);
        let event = event(std::slice::from_ref(&signer));
        let signatures = authority_funding_signatures_with_presentations(
            std::slice::from_ref(&event),
            std::slice::from_ref(&exposed_name),
        )
        .unwrap();
        let exposed_key = cost_signature_to_sig(&exposed_name).unwrap().lane_hash();
        let inventory = AuthorityPhysicalInventory {
            balances: ResourceMultiset::singleton(exposed_key, u64::MAX),
            balance_custody: BTreeMap::from([(exposed_key, exposed_key)]),
            stacks: BTreeMap::new(),
            born_stacks: BTreeMap::new(),
        };

        assert_eq!(
            allocate_physical_settlement(std::slice::from_ref(&event), &signatures, &inventory,),
            Err(AuthorityError::InsufficientAuthority)
        );
        assert_eq!(
            allocate_quantitative_debit(&event, 1, &inventory.balances),
            Err(AuthorityError::InsufficientAuthority)
        );
    }

    proptest! {
        #[test]
        fn capability_evidence_cannot_become_payable_signature(
            (funding, mut expected_atoms) in funding_tree_with_atoms(),
            contexts in proptest::collection::vec((funding_tree_with_atoms(), any::<bool>()), 0..12),
        ) {
            prop_assert!(funding.is_funding_former());
            let encoded = sig_to_cost_signature(&funding).unwrap();
            let mut actual_atoms = signature_atoms(&encoded).unwrap().into_iter().map(|atom| {
                match atom.value {
                    Some(CostSignatureValue::Ground(bytes)) => bytes,
                    other => panic!("unexpected payable atom: {other:?}"),
                }
            }).collect::<Vec<_>>();
            actual_atoms.sort();
            expected_atoms.sort();
            prop_assert_eq!(actual_atoms, expected_atoms);

            let capabilities = [
                Sig::Plus(Box::new(funding.clone()), Box::new(Sig::Unit)),
                Sig::With(Box::new(funding.clone()), Box::new(funding.clone())),
                Sig::Bang(Box::new(funding.clone())),
                Sig::WhyNot(Box::new(funding.clone())),
                Sig::Lolly(Box::new(funding.clone()), Box::new(funding.clone())),
                Sig::Threshold { threshold: 1, members: vec![funding] },
            ];
            for mut signature in capabilities {
                for ((context, _), left) in &contexts {
                    signature = if *left {
                        Sig::And(Box::new(context.clone()), Box::new(signature))
                    } else {
                        Sig::And(Box::new(signature), Box::new(context.clone()))
                    };
                }
                prop_assert!(!signature.is_funding_former());
                prop_assert_eq!(sig_to_cost_signature(&signature),
                    Err(AuthorityError::UnsupportedFundingSignature));
            }
        }

        #[test]
        fn arbitrary_private_name_presentations_cannot_create_event_authority(
            private_id in any::<[u8; 32]>(),
            available in 1_u64..u64::MAX,
        ) {
            let signer = ground(b"signed-event-authority");
            let exposed_name = private_name(&private_id);
            let event = event(std::slice::from_ref(&signer));
            let signatures = authority_funding_signatures_with_presentations(
                std::slice::from_ref(&event),
                std::slice::from_ref(&exposed_name),
            )
            .unwrap();
            let exposed_key = cost_signature_to_sig(&exposed_name).unwrap().lane_hash();
            let inventory = AuthorityPhysicalInventory {
                balances: ResourceMultiset::singleton(exposed_key, available),
                balance_custody: BTreeMap::from([(exposed_key, exposed_key)]),
                stacks: BTreeMap::new(),
                born_stacks: BTreeMap::new(),
            };

            prop_assert_eq!(
                allocate_physical_settlement(
                    std::slice::from_ref(&event),
                    &signatures,
                    &inventory,
                ),
                Err(AuthorityError::InsufficientAuthority)
            );
            prop_assert_eq!(
                allocate_quantitative_debit(&event, 1, &inventory.balances),
                Err(AuthorityError::InsufficientAuthority)
            );
        }
    }

    #[test]
    fn bound_signatures_cannot_cross_the_runtime_authority_boundary() {
        let signature = CostSignature {
            value: Some(CostSignatureValue::BoundLevel(0)),
        };
        assert_eq!(
            cost_signature_to_sig(&signature),
            Err(AuthorityError::UnresolvedBoundLevel)
        );
    }

    #[test]
    fn one_region_is_one_cell_even_when_its_signature_is_compound() {
        let signature = compound_cost_signatures(&ground(b"a"), &ground(b"b")).unwrap();
        let region = cost_region(&signature, b"redex", 0).unwrap();
        let authority = CostAuthority {
            regions: vec![region],
        };
        let demand = authority_demand(&authority).unwrap();
        assert_eq!(demand.0.len(), 1);
        assert_eq!(
            demand.get(&cost_signature_to_sig(&signature).unwrap().lane_hash()),
            1
        );
    }

    #[test]
    fn quantitative_debit_uses_combined_pool_then_balanced_components() {
        let left = ground(b"quantitative-left");
        let right = ground(b"quantitative-right");
        let event = event(&[left.clone(), right.clone()]);
        let combined = compound_cost_signatures(&left, &right).unwrap();
        let combined_key = cost_signature_to_sig(&combined).unwrap().lane_hash();
        let left_key = cost_signature_to_sig(&left).unwrap().lane_hash();
        let right_key = cost_signature_to_sig(&right).unwrap().lane_hash();

        assert_eq!(
            allocate_quantitative_debit(&event, 7, &ResourceMultiset::singleton(combined_key, 7),)
                .unwrap(),
            ResourceMultiset::singleton(combined_key, 7)
        );

        let components = ResourceMultiset(BTreeMap::from([(left_key, 7), (right_key, 7)]));
        assert_eq!(
            allocate_quantitative_debit(&event, 7, &components).unwrap(),
            components
        );
        assert_eq!(
            allocate_quantitative_debit(&event, 7, &ResourceMultiset::singleton(left_key, 7),),
            Err(AuthorityError::InsufficientAuthority)
        );
    }

    #[test]
    fn quantitative_debit_rejects_scaled_overflow() {
        let signature = ground(b"quantitative-overflow");
        let key = cost_signature_to_sig(&signature).unwrap().lane_hash();
        let event = event(&[signature.clone(), signature]);

        assert_eq!(
            allocate_quantitative_debit(
                &event,
                u64::MAX,
                &ResourceMultiset::singleton(key, u64::MAX),
            ),
            Err(AuthorityError::ArithmeticOverflow)
        );
    }

    #[test]
    fn quantitative_events_debit_their_own_located_purses() {
        let outer = ground(b"outer-purse");
        let continuation = ground(b"continuation-purse");
        let outer_key = cost_signature_to_sig(&outer).unwrap().lane_hash();
        let continuation_key = cost_signature_to_sig(&continuation).unwrap().lane_hash();
        let events = vec![
            byte_event(
                1,
                AuthorityByteEventKind::Comm,
                std::slice::from_ref(&outer),
                5,
            ),
            byte_event(
                2,
                AuthorityByteEventKind::ProduceIntroduction,
                std::slice::from_ref(&continuation),
                7,
            ),
        ];
        let available = ResourceMultiset(BTreeMap::from([(outer_key, 5), (continuation_key, 7)]));

        assert_eq!(
            allocate_quantitative_events(&events, &available).unwrap(),
            available
        );
        assert_eq!(
            allocate_quantitative_events(&events, &ResourceMultiset::singleton(outer_key, 12),),
            Err(AuthorityError::InsufficientAuthority)
        );
    }

    #[test]
    fn quantitative_event_allocation_backtracks_across_compound_choices() {
        let left = ground(b"byte-left");
        let right = ground(b"byte-right");
        let combined = compound_cost_signatures(&left, &right).unwrap();
        let left_key = cost_signature_to_sig(&left).unwrap().lane_hash();
        let right_key = cost_signature_to_sig(&right).unwrap().lane_hash();
        let combined_key = cost_signature_to_sig(&combined).unwrap().lane_hash();
        let events = vec![
            byte_event(1, AuthorityByteEventKind::Comm, &[left, right], 1),
            byte_event(
                2,
                AuthorityByteEventKind::ConsumeIntroduction,
                &[combined],
                1,
            ),
        ];
        let available = ResourceMultiset(BTreeMap::from([
            (left_key, 1),
            (right_key, 1),
            (combined_key, 1),
        ]));

        assert_eq!(
            allocate_quantitative_events(&events, &available).unwrap(),
            available
        );
    }

    #[test]
    fn quantitative_event_allocation_is_permutation_invariant_and_stack_safe() {
        let signature = ground(b"byte-trace-payer");
        let key = cost_signature_to_sig(&signature).unwrap().lane_hash();
        let count = 4096_u64;
        let events = (0..count)
            .map(|index| {
                let mut event = byte_event(
                    0,
                    AuthorityByteEventKind::ProduceIntroduction,
                    std::slice::from_ref(&signature),
                    1,
                );
                event.event_id[..8].copy_from_slice(&index.to_le_bytes());
                event
            })
            .collect::<Vec<_>>();
        let mut reversed = events.clone();
        reversed.reverse();
        let available = ResourceMultiset::singleton(key, count);

        assert_eq!(
            allocate_quantitative_events(&events, &available).unwrap(),
            allocate_quantitative_events(&reversed, &available).unwrap()
        );
    }

    #[test]
    fn unit_authority_requires_no_resource_cell() {
        let signature = sig_to_cost_signature(&Sig::Unit).unwrap();
        let authority = CostAuthority {
            regions: vec![cost_region(&signature, b"unit authority", 0).unwrap()],
        };
        let event = AuthorityEvent {
            event_id: [22; 32],
            debit: authority_demand(&authority).unwrap(),
            authority,
        };

        assert!(event.debit.0.is_empty());
        event.verify_authority().unwrap();
        assert_eq!(authority_funding_options(&event).unwrap(), vec![
            ResourceMultiset::default()
        ]);
        assert!(authority_funding_signatures(&[event]).unwrap().is_empty());
    }

    #[test]
    fn split_and_combined_cells_fund_each_other_without_partial_consumption() {
        let a = ground(b"a");
        let b = ground(b"b");
        let compound = compound_cost_signatures(&a, &b).unwrap();
        let compound_key = cost_signature_to_sig(&compound).unwrap().lane_hash();
        let a_key = cost_signature_to_sig(&a).unwrap().lane_hash();
        let b_key = cost_signature_to_sig(&b).unwrap().lane_hash();

        let split_event = event(&[a.clone(), b.clone()]);
        let combined_only = ResourceMultiset::singleton(compound_key, 1);
        assert_eq!(
            allocate_authority_events(std::slice::from_ref(&split_event), &combined_only).unwrap(),
            combined_only
        );

        let compound_event = event(&[compound]);
        let split_only = ResourceMultiset(BTreeMap::from([(a_key, 1), (b_key, 1)]));
        assert_eq!(
            allocate_authority_events(std::slice::from_ref(&compound_event), &split_only).unwrap(),
            split_only
        );
    }

    #[test]
    fn physical_presentation_accepts_an_arbitrary_join_partition() {
        let a = ground(b"a");
        let b = ground(b"b");
        let c = ground(b"c");
        let d = ground(b"d");
        let event = event(&[a.clone(), b.clone(), c.clone(), d.clone()]);
        let ab = compound_cost_signatures(&a, &b).unwrap();
        let cd = compound_cost_signatures(&c, &d).unwrap();
        let ab_key = cost_signature_to_sig(&ab).unwrap().lane_hash();
        let cd_key = cost_signature_to_sig(&cd).unwrap().lane_hash();
        let balances = ResourceMultiset(BTreeMap::from([(ab_key, 1), (cd_key, 1)]));
        let signatures = BTreeMap::from([(ab_key, ab), (cd_key, cd)]);
        let inventory = AuthorityPhysicalInventory {
            balances: balances.clone(),
            balance_custody: balances.0.keys().map(|key| (*key, *key)).collect(),
            stacks: BTreeMap::new(),
            born_stacks: BTreeMap::new(),
        };
        let draws = vec![AuthorityPhysicalEventDraw {
            event_id: event.event_id,
            balances: balances.clone(),
            stack_ids: Vec::new(),
        }];

        let settlement = verify_physical_settlement(
            std::slice::from_ref(&event),
            &signatures,
            &inventory,
            &draws,
        )
        .unwrap();
        assert_eq!(settlement.balance_debit, balances);
        assert!(settlement.stack_pops.is_empty());
        assert_eq!(
            allocate_physical_settlement(std::slice::from_ref(&event), &signatures, &inventory,)
                .unwrap(),
            settlement
        );
    }

    #[test]
    fn physical_search_budget_rejects_before_candidate_expansion() {
        let signature = ground(b"bounded-search");
        let key = cost_signature_to_sig(&signature).unwrap().lane_hash();
        let authority_event = event(std::slice::from_ref(&signature));
        let signatures = BTreeMap::from([(key, signature)]);
        let inventory = AuthorityPhysicalInventory {
            balances: ResourceMultiset::singleton(key, 1),
            balance_custody: BTreeMap::from([(key, key)]),
            stacks: BTreeMap::new(),
            born_stacks: BTreeMap::new(),
        };
        let original = inventory.clone();
        let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(10_000));
        limits.set(HostWorkDimension::SearchCandidates, HostWorkLimit::new(0));
        let host_work = HostWorkBudget::new(limits);

        assert_eq!(
            allocate_physical_settlement_with_host_work(
                std::slice::from_ref(&authority_event),
                &signatures,
                &inventory,
                Some(&host_work),
            ),
            Err(AuthorityError::HostWorkRejected)
        );
        assert_eq!(inventory, original);
        assert_eq!(
            host_work.usage(HostWorkDimension::SearchCandidates),
            HostWorkUsage::ZERO
        );
        assert!(host_work.is_rejected());
    }

    #[test]
    fn bounded_physical_search_matches_unbounded_settlement() {
        let signature = ground(b"bounded-search-equivalence");
        let key = cost_signature_to_sig(&signature).unwrap().lane_hash();
        let authority_event = event(std::slice::from_ref(&signature));
        let signatures = BTreeMap::from([(key, signature)]);
        let inventory = AuthorityPhysicalInventory {
            balances: ResourceMultiset::singleton(key, 1),
            balance_custody: BTreeMap::from([(key, key)]),
            stacks: BTreeMap::new(),
            born_stacks: BTreeMap::new(),
        };
        let expected = allocate_physical_settlement(
            std::slice::from_ref(&authority_event),
            &signatures,
            &inventory,
        )
        .unwrap();
        let host_work = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(10_000)));

        let actual = allocate_physical_settlement_with_host_work(
            std::slice::from_ref(&authority_event),
            &signatures,
            &inventory,
            Some(&host_work),
        )
        .unwrap();

        assert_eq!(actual, expected);
        assert!(host_work.usage(HostWorkDimension::SearchCandidates).get() > 0);
        assert!(host_work.usage(HostWorkDimension::SearchStateBytes).get() > 0);
        assert!(!host_work.is_rejected());
    }

    #[test]
    fn explicit_region_cannot_spend_an_unrelated_default_balance() {
        let default = ground(b"default envelope payer");
        let explicit = ground(b"explicit region payer");
        let default_key = cost_signature_to_sig(&default).unwrap().lane_hash();
        let explicit_key = cost_signature_to_sig(&explicit).unwrap().lane_hash();
        let authority_event = event(std::slice::from_ref(&explicit));
        let stack_id = [31; 32];
        let signatures = BTreeMap::from([(default_key, default)]);
        let without_explicit_stack = AuthorityPhysicalInventory {
            balances: ResourceMultiset::singleton(default_key, 100),
            balance_custody: BTreeMap::from([(default_key, default_key)]),
            stacks: BTreeMap::new(),
            born_stacks: BTreeMap::new(),
        };

        assert_eq!(
            allocate_physical_settlement(
                std::slice::from_ref(&authority_event),
                &signatures,
                &without_explicit_stack,
            ),
            Err(AuthorityError::InsufficientAuthority)
        );

        let inventory = AuthorityPhysicalInventory {
            balances: ResourceMultiset::singleton(default_key, 100),
            balance_custody: BTreeMap::from([(default_key, default_key)]),
            stacks: BTreeMap::from([(stack_id, vec![explicit])]),
            born_stacks: BTreeMap::new(),
        };
        let settlement = allocate_physical_settlement(
            std::slice::from_ref(&authority_event),
            &signatures,
            &inventory,
        )
        .unwrap();

        assert!(settlement.balance_debit.0.is_empty());
        assert_eq!(settlement.stack_pops, BTreeMap::from([(stack_id, 1)]));
        assert_eq!(settlement.draws[0].stack_ids, vec![stack_id]);
        assert_eq!(authority_event.debit.get(&explicit_key), 1);
        assert_eq!(authority_event.debit.get(&default_key), 0);
    }

    #[test]
    fn physical_settlement_search_is_stack_safe_for_long_event_traces() {
        let signature = ground(b"stack-safe");
        let key = cost_signature_to_sig(&signature).unwrap().lane_hash();
        let event_count = 4096_u64;
        let events = (0..event_count)
            .map(|index| {
                let mut authority_event = event(std::slice::from_ref(&signature));
                authority_event.event_id[..8].copy_from_slice(&index.to_le_bytes());
                authority_event
            })
            .collect::<Vec<_>>();
        let inventory = AuthorityPhysicalInventory {
            balances: ResourceMultiset::singleton(key, event_count),
            balance_custody: BTreeMap::from([(key, key)]),
            stacks: BTreeMap::new(),
            born_stacks: BTreeMap::new(),
        };
        let settlement =
            allocate_physical_settlement(&events, &BTreeMap::from([(key, signature)]), &inventory)
                .unwrap();

        assert_eq!(settlement.draws.len(), event_count as usize);
        assert_eq!(
            settlement.balance_debit,
            ResourceMultiset::singleton(key, event_count)
        );
        assert!(settlement.stack_pops.is_empty());
    }

    #[test]
    fn aliased_logical_lanes_share_one_physical_balance() {
        let lane_a = [1; 32];
        let lane_b = [2; 32];
        let lane_c = [3; 32];
        let custody = [9; 32];
        let mut inventory = AuthorityPhysicalInventory::default();

        inventory.insert_balance_lane(lane_a, custody, 7).unwrap();
        inventory.insert_balance_lane(lane_b, custody, 7).unwrap();

        assert_eq!(inventory.balances, ResourceMultiset::singleton(custody, 7));
        assert_eq!(
            inventory.logical_balance_view(),
            ResourceMultiset(BTreeMap::from([(lane_a, 7), (lane_b, 7)]))
        );
        assert_eq!(
            inventory.insert_balance_lane(lane_c, custody, 8),
            Err(AuthorityError::PhysicalCustodyMismatch)
        );
        assert_eq!(
            inventory.insert_balance_lane(lane_a, [8; 32], 7),
            Err(AuthorityError::PhysicalCustodyMismatch)
        );
    }

    #[test]
    fn aliased_lanes_cannot_double_spend_one_physical_cell() {
        let signature_a = ground(b"alias-a");
        let signature_b = ground(b"alias-b");
        let lane_a = cost_signature_to_sig(&signature_a).unwrap().lane_hash();
        let lane_b = cost_signature_to_sig(&signature_b).unwrap().lane_hash();
        let custody = [9; 32];
        let mut event_a = event(std::slice::from_ref(&signature_a));
        let mut event_b = event(std::slice::from_ref(&signature_b));
        event_a.event_id = [1; 32];
        event_b.event_id = [2; 32];
        let available = ResourceMultiset::singleton(custody, 1);
        let aliases = BTreeMap::from([(lane_a, custody), (lane_b, custody)]);

        let one = allocate_authority_events_with_custody(
            std::slice::from_ref(&event_a),
            &available,
            &aliases,
        )
        .unwrap();
        assert_eq!(one.logical_debit, ResourceMultiset::singleton(lane_a, 1));
        assert_eq!(one.custody_debit, ResourceMultiset::singleton(custody, 1));
        assert_eq!(
            allocate_authority_events_with_custody(&[event_a, event_b], &available, &aliases),
            Err(AuthorityError::InsufficientAuthority)
        );
    }

    #[test]
    fn physical_settlement_rejects_alias_overdraw() {
        let signature_a = ground(b"settlement-alias-a");
        let signature_b = ground(b"settlement-alias-b");
        let lane_a = cost_signature_to_sig(&signature_a).unwrap().lane_hash();
        let lane_b = cost_signature_to_sig(&signature_b).unwrap().lane_hash();
        let custody = [7; 32];
        let mut event_a = event(std::slice::from_ref(&signature_a));
        let mut event_b = event(std::slice::from_ref(&signature_b));
        event_a.event_id = [1; 32];
        event_b.event_id = [2; 32];
        let inventory = AuthorityPhysicalInventory {
            balances: ResourceMultiset::singleton(custody, 1),
            balance_custody: BTreeMap::from([(lane_a, custody), (lane_b, custody)]),
            stacks: BTreeMap::new(),
            born_stacks: BTreeMap::new(),
        };
        let draws = [
            AuthorityPhysicalEventDraw {
                event_id: event_a.event_id,
                balances: ResourceMultiset::singleton(lane_a, 1),
                stack_ids: Vec::new(),
            },
            AuthorityPhysicalEventDraw {
                event_id: event_b.event_id,
                balances: ResourceMultiset::singleton(lane_b, 1),
                stack_ids: Vec::new(),
            },
        ];

        assert_eq!(
            verify_physical_settlement(
                &[event_a, event_b],
                &BTreeMap::from([(lane_a, signature_a), (lane_b, signature_b)]),
                &inventory,
                &draws,
            ),
            Err(AuthorityError::InsufficientAuthority)
        );
    }

    #[test]
    fn compute_byte_and_fee_allocations_share_physical_capacity() {
        let signature_a = ground(b"phase-alias-a");
        let signature_b = ground(b"phase-alias-b");
        let lane_a = cost_signature_to_sig(&signature_a).unwrap().lane_hash();
        let lane_b = cost_signature_to_sig(&signature_b).unwrap().lane_hash();
        let custody = [5; 32];
        let aliases = BTreeMap::from([(lane_a, custody), (lane_b, custody)]);
        let available = ResourceMultiset::singleton(custody, 1);
        let bytes = [
            byte_event(1, AuthorityByteEventKind::Comm, &[signature_a], 1),
            byte_event(2, AuthorityByteEventKind::Comm, &[signature_b], 1),
        ];

        assert_eq!(
            allocate_quantitative_events_with_custody(&bytes, &available, &aliases),
            Err(AuthorityError::InsufficientAuthority)
        );

        let mut certificate = certificate(ResourceMultiset::singleton(lane_a, 1));
        certificate.fee_allocation = ResourceMultiset::singleton(lane_b, 1);
        assert_eq!(
            certificate.verify_with_custody(
                AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
                certificate.program_hash,
                certificate.pre_state_root,
                &available,
                &aliases,
                |_, _| false,
                |demand, allocation| demand == allocation,
            ),
            Err(AuthorityError::InsufficientAuthority)
        );
    }

    proptest! {
        #[test]
        fn aliased_lane_allocation_never_exceeds_physical_capacity(
            demand_a in 0_usize..24,
            demand_b in 0_usize..24,
            capacity in 0_u64..48,
        ) {
            let signature_a = ground(b"property-alias-a");
            let signature_b = ground(b"property-alias-b");
            let lane_a = cost_signature_to_sig(&signature_a).unwrap().lane_hash();
            let lane_b = cost_signature_to_sig(&signature_b).unwrap().lane_hash();
            let custody = [4; 32];
            let mut events = Vec::with_capacity(demand_a + demand_b);
            for index in 0..demand_a {
                let mut authority_event = event(std::slice::from_ref(&signature_a));
                authority_event.event_id[..8].copy_from_slice(&(index as u64).to_le_bytes());
                authority_event.event_id[8] = 1;
                events.push(authority_event);
            }
            for index in 0..demand_b {
                let mut authority_event = event(std::slice::from_ref(&signature_b));
                authority_event.event_id[..8].copy_from_slice(&(index as u64).to_le_bytes());
                authority_event.event_id[8] = 2;
                events.push(authority_event);
            }
            let available = ResourceMultiset::singleton(custody, capacity);
            let aliases = BTreeMap::from([(lane_a, custody), (lane_b, custody)]);
            let result = allocate_authority_events_with_custody(&events, &available, &aliases);
            let demand = (demand_a + demand_b) as u64;

            prop_assert_eq!(result.is_ok(), demand <= capacity);
            if let Ok(settlement) = result {
                prop_assert_eq!(settlement.custody_debit.get(&custody), demand);
                prop_assert!(available.dominates(&settlement.custody_debit));
            }
        }
    }

    #[test]
    fn physical_presentation_rejects_weakening_a_compound_cell() {
        let a = ground(b"a");
        let b = ground(b"b");
        let event = event(std::slice::from_ref(&a));
        let ab = compound_cost_signatures(&a, &b).unwrap();
        let ab_key = cost_signature_to_sig(&ab).unwrap().lane_hash();
        let balances = ResourceMultiset::singleton(ab_key, 1);
        let inventory = AuthorityPhysicalInventory {
            balances: balances.clone(),
            balance_custody: BTreeMap::from([(ab_key, ab_key)]),
            stacks: BTreeMap::new(),
            born_stacks: BTreeMap::new(),
        };
        let draws = vec![AuthorityPhysicalEventDraw {
            event_id: event.event_id,
            balances,
            stack_ids: Vec::new(),
        }];

        assert_eq!(
            verify_physical_settlement(
                std::slice::from_ref(&event),
                &BTreeMap::from([(ab_key, ab)]),
                &inventory,
                &draws,
            ),
            Err(AuthorityError::PhysicalAuthorityMismatch)
        );
    }

    #[test]
    fn physical_presentation_pops_a_stack_in_event_order() {
        let a = ground(b"a");
        let b = ground(b"b");
        let mut first = event(std::slice::from_ref(&a));
        first.event_id = [1; 32];
        let mut second = event(std::slice::from_ref(&b));
        second.event_id = [2; 32];
        let stack_id = [9; 32];
        let inventory = AuthorityPhysicalInventory {
            balances: ResourceMultiset::default(),
            balance_custody: BTreeMap::new(),
            stacks: BTreeMap::from([(stack_id, vec![a, b])]),
            born_stacks: BTreeMap::new(),
        };
        let draws = vec![
            AuthorityPhysicalEventDraw {
                event_id: first.event_id,
                balances: ResourceMultiset::default(),
                stack_ids: vec![stack_id],
            },
            AuthorityPhysicalEventDraw {
                event_id: second.event_id,
                balances: ResourceMultiset::default(),
                stack_ids: vec![stack_id],
            },
        ];

        let settlement = verify_physical_settlement(
            &[first.clone(), second.clone()],
            &BTreeMap::new(),
            &inventory,
            &draws,
        )
        .unwrap();
        assert_eq!(settlement.stack_pops, BTreeMap::from([(stack_id, 2)]));
        assert_eq!(
            allocate_physical_settlement(
                &[first.clone(), second.clone()],
                &BTreeMap::new(),
                &inventory,
            )
            .unwrap(),
            settlement
        );
        let reverse_draws = vec![
            AuthorityPhysicalEventDraw {
                event_id: second.event_id,
                balances: ResourceMultiset::default(),
                stack_ids: vec![stack_id],
            },
            AuthorityPhysicalEventDraw {
                event_id: first.event_id,
                balances: ResourceMultiset::default(),
                stack_ids: vec![stack_id],
            },
        ];
        assert_eq!(
            verify_physical_settlement(
                &[second, first],
                &BTreeMap::new(),
                &inventory,
                &reverse_draws,
            ),
            Err(AuthorityError::PhysicalAuthorityMismatch)
        );
    }

    #[test]
    fn born_stack_cannot_fund_its_own_transfer_but_can_fund_a_later_event() {
        let a = ground(b"a");
        let key = cost_signature_to_sig(&a).unwrap().lane_hash();
        let produce_hash = [7; 32];
        let stack_id = [9; 32];
        let mut transfer = event(std::slice::from_ref(&a));
        transfer.event_id = stack_transfer_event_id(&produce_hash, 0);
        let mut use_event = event(std::slice::from_ref(&a));
        use_event.event_id = [8; 32];
        let signatures = BTreeMap::from([(key, a.clone())]);
        let unfunded = AuthorityPhysicalInventory {
            balances: ResourceMultiset::default(),
            balance_custody: BTreeMap::new(),
            stacks: BTreeMap::from([(stack_id, vec![a.clone()])]),
            born_stacks: BTreeMap::from([(stack_id, produce_hash)]),
        };

        assert_eq!(
            allocate_physical_settlement(std::slice::from_ref(&transfer), &signatures, &unfunded,),
            Err(AuthorityError::InsufficientAuthority)
        );

        let funded = AuthorityPhysicalInventory {
            balances: ResourceMultiset::singleton(key, 1),
            balance_custody: BTreeMap::from([(key, key)]),
            ..unfunded
        };
        assert_eq!(
            allocate_physical_settlement(
                &[use_event.clone(), transfer.clone()],
                &signatures,
                &funded,
            ),
            Err(AuthorityError::InsufficientAuthority)
        );
        let settlement =
            allocate_physical_settlement(&[transfer, use_event], &signatures, &funded).unwrap();
        assert_eq!(
            settlement.balance_debit,
            ResourceMultiset::singleton(key, 1)
        );
        assert_eq!(settlement.stack_pops, BTreeMap::from([(stack_id, 1)]));
    }

    #[test]
    fn certificate_and_witness_separate_semantic_demand_from_physical_settlement() {
        let a = ground(b"a");
        let b = ground(b"b");
        let event = event(&[a.clone(), b.clone()]);
        let compound = compound_cost_signatures(&a, &b).unwrap();
        let compound_key = cost_signature_to_sig(&compound).unwrap().lane_hash();
        let physical = ResourceMultiset::singleton(compound_key, 2);
        let demand = event.debit.clone();
        let certificate = FundingCertificate {
            protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
            program_hash: [1; 32],
            pre_state_root: [2; 32],
            reservation_id: [3; 32],
            demand: DemandBound::Exact(demand.clone()),
            allocation: physical.clone(),
            stack_reservations: BTreeMap::new(),
            fee_allocation: ResourceMultiset::default(),
            fee_plan: None,
            fee_recipient: Vec::new(),
            byte_cost_schedule_version: byte_schedule_version(),
            byte_cost_schedule_digest: byte_schedule_digest(),
            byte_cost_bound: 0,
            byte_allocation: ResourceMultiset::default(),
        };
        certificate
            .verify_with_allocation(
                AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
                [1; 32],
                [2; 32],
                &physical,
                |_, _| false,
                |bound, allocation| {
                    bound == &demand
                        && allocate_authority_events(std::slice::from_ref(&event), allocation)
                            .is_ok()
                },
            )
            .unwrap();
        let settlement = ResourceMultiset::singleton(compound_key, 1);
        let witness = AuthorityCostWitness {
            protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
            certificate_id: certificate.certificate_id(),
            pre_state_root: [2; 32],
            post_state_root: [4; 32],
            events: vec![event],
            realized: demand,
            settlement: settlement.clone(),
            byte_cost_schedule_version: byte_schedule_version(),
            byte_cost_schedule_digest: byte_schedule_digest(),
            byte_events: Vec::new(),
            byte_cost: 0,
            byte_settlement: ResourceMultiset::default(),
            physical_draws: Vec::new(),
            born_stacks: Vec::new(),
        };
        witness
            .verify_with_settlement(&certificate, |events, _, reserved| {
                allocate_authority_events(events, reserved)
            })
            .unwrap();
        assert_eq!(
            witness.refund(&certificate).unwrap(),
            ResourceMultiset::singleton(compound_key, 1)
        );
    }

    #[test]
    fn witness_refunds_the_unforced_reservation() {
        let key = [7; 32];
        let cert = certificate(ResourceMultiset::singleton(key, 3));
        let witness = AuthorityCostWitness {
            protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
            certificate_id: cert.certificate_id(),
            pre_state_root: [2; 32],
            post_state_root: [4; 32],
            events: vec![AuthorityEvent {
                event_id: [5; 32],
                authority: CostAuthority::default(),
                debit: ResourceMultiset::singleton(key, 1),
            }],
            realized: ResourceMultiset::singleton(key, 1),
            settlement: ResourceMultiset::singleton(key, 1),
            byte_cost_schedule_version: byte_schedule_version(),
            byte_cost_schedule_digest: byte_schedule_digest(),
            byte_events: Vec::new(),
            byte_cost: 0,
            byte_settlement: ResourceMultiset::default(),
            physical_draws: Vec::new(),
            born_stacks: Vec::new(),
        };

        witness.verify(&cert).unwrap();
        assert_eq!(
            cert.allocation
                .checked_sub(&witness.realized)
                .unwrap()
                .get(&key),
            2
        );
    }

    #[test]
    fn witness_refund_includes_unused_authority_and_byte_reservations() {
        let key = [7; 32];
        let mut cert = certificate(ResourceMultiset::singleton(key, 5));
        cert.byte_cost_bound = 7;
        cert.byte_allocation = ResourceMultiset::singleton(key, 7);
        let witness = AuthorityCostWitness {
            protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
            certificate_id: cert.certificate_id(),
            pre_state_root: [2; 32],
            post_state_root: [4; 32],
            events: vec![AuthorityEvent {
                event_id: [5; 32],
                authority: CostAuthority::default(),
                debit: ResourceMultiset::singleton(key, 1),
            }],
            realized: ResourceMultiset::singleton(key, 1),
            settlement: ResourceMultiset::singleton(key, 1),
            byte_cost_schedule_version: byte_schedule_version(),
            byte_cost_schedule_digest: byte_schedule_digest(),
            byte_events: vec![AuthorityByteEvent {
                event_id: [8; 32],
                kind: AuthorityByteEventKind::Comm,
                authority: CostAuthority::default(),
                amount: 3,
            }],
            byte_cost: 3,
            byte_settlement: ResourceMultiset::singleton(key, 3),
            physical_draws: Vec::new(),
            born_stacks: Vec::new(),
        };

        witness.verify(&cert).unwrap();
        assert_eq!(
            witness.refund(&cert).unwrap(),
            ResourceMultiset::singleton(key, 8)
        );
    }

    #[test]
    fn byte_event_witness_rejects_omission_reordering_conflicts_and_zero_amounts() {
        let base = AuthorityCostWitness::<[u8; 32]> {
            protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
            certificate_id: [1; 32],
            pre_state_root: [2; 32],
            post_state_root: [3; 32],
            events: Vec::new(),
            realized: ResourceMultiset::default(),
            settlement: ResourceMultiset::default(),
            physical_draws: Vec::new(),
            born_stacks: Vec::new(),
            byte_cost_schedule_version: byte_schedule_version(),
            byte_cost_schedule_digest: byte_schedule_digest(),
            byte_events: vec![
                AuthorityByteEvent {
                    event_id: [1; 32],
                    kind: AuthorityByteEventKind::ProduceIntroduction,
                    authority: CostAuthority::default(),
                    amount: 1,
                },
                AuthorityByteEvent {
                    event_id: [2; 32],
                    kind: AuthorityByteEventKind::Comm,
                    authority: CostAuthority::default(),
                    amount: 2,
                },
            ],
            byte_cost: 3,
            byte_settlement: ResourceMultiset::default(),
        };
        base.verify_structure().unwrap();

        let mut omitted = base.clone();
        omitted.byte_events.pop();
        assert_eq!(
            omitted.verify_structure(),
            Err(AuthorityError::ByteCostMismatch)
        );

        let mut reordered = base.clone();
        reordered.byte_events.reverse();
        assert_eq!(
            reordered.verify_structure(),
            Err(AuthorityError::NonCanonicalEventOrder)
        );

        let mut conflicting = base.clone();
        conflicting.byte_events = vec![
            AuthorityByteEvent {
                event_id: [1; 32],
                kind: AuthorityByteEventKind::ProduceIntroduction,
                authority: CostAuthority::default(),
                amount: 1,
            },
            AuthorityByteEvent {
                event_id: [1; 32],
                kind: AuthorityByteEventKind::ProduceIntroduction,
                authority: CostAuthority::default(),
                amount: 2,
            },
        ];
        assert_eq!(
            conflicting.verify_structure(),
            Err(AuthorityError::EventIdentityConflict)
        );

        let mut zero = base.clone();
        zero.byte_events[0].amount = 0;
        assert_eq!(
            zero.verify_structure(),
            Err(AuthorityError::InvalidByteEventAmount)
        );

        let mut changed = base.clone();
        changed.byte_events[0].kind = AuthorityByteEventKind::ConsumeIntroduction;
        assert_ne!(base.witness_id(), changed.witness_id());
    }

    #[test]
    fn finite_bound_requires_an_explicit_verifier_and_is_digest_committed() {
        let key = [8; 32];
        let allocation = ResourceMultiset::singleton(key, 3);
        let mut cert = certificate(allocation.clone());
        cert.demand = DemandBound::FiniteUpperBound {
            bound: allocation.clone(),
            proof: b"proof-a".to_vec(),
        };
        let first_id = cert.certificate_id();

        assert_eq!(
            cert.verify(
                AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
                [1; 32],
                [2; 32],
                &allocation,
            ),
            Err(AuthorityError::InvalidBoundProof)
        );
        cert.verify_with(
            AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
            [1; 32],
            [2; 32],
            &allocation,
            |bound, proof| bound == &allocation && proof == b"proof-a",
        )
        .unwrap();

        if let DemandBound::FiniteUpperBound { proof, .. } = &mut cert.demand {
            *proof = b"proof-b".to_vec();
        }
        assert_ne!(first_id, cert.certificate_id());
    }

    #[test]
    fn resource_multiset_covers_zero_overflow_subtraction_and_dominance_boundaries() {
        let key = [6; 32];
        assert_eq!(
            ResourceMultiset::singleton(key, 0),
            ResourceMultiset::default()
        );

        let zero_entry = ResourceMultiset(BTreeMap::from([(key, 0)]));
        assert_eq!(
            ResourceMultiset::default()
                .checked_add(&zero_entry)
                .unwrap(),
            ResourceMultiset::default()
        );

        let maximum = ResourceMultiset(BTreeMap::from([(key, u64::MAX)]));
        assert_eq!(
            maximum.checked_add(&ResourceMultiset::singleton(key, 1)),
            Err(AuthorityError::ArithmeticOverflow)
        );

        let three = ResourceMultiset::singleton(key, 3);
        assert_eq!(
            three.checked_sub(&ResourceMultiset::singleton(key, 4)),
            Err(AuthorityError::InsufficientAuthority)
        );
        assert_eq!(
            three
                .checked_sub(&ResourceMultiset::singleton(key, 1))
                .unwrap()
                .get(&key),
            2
        );
        assert_eq!(
            three
                .checked_sub(&ResourceMultiset::singleton(key, 3))
                .unwrap(),
            ResourceMultiset::default()
        );
    }

    #[test]
    fn certificate_verification_rejects_every_invalid_binding_and_authority_case() {
        let key = [10; 32];
        let allocation = ResourceMultiset::singleton(key, 3);
        let exact = certificate(allocation.clone());

        assert_eq!(
            exact.verify(
                AUTHORITY_ACCOUNTING_PROTOCOL_VERSION + 1,
                [1; 32],
                [2; 32],
                &allocation,
            ),
            Err(AuthorityError::ProtocolVersionMismatch)
        );
        assert_eq!(
            exact.verify(
                AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
                [9; 32],
                [2; 32],
                &allocation
            ),
            Err(AuthorityError::ProgramHashMismatch)
        );
        assert_eq!(
            exact.verify(
                AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
                [1; 32],
                [9; 32],
                &allocation
            ),
            Err(AuthorityError::PreStateMismatch)
        );

        let mut missing_proof = exact.clone();
        missing_proof.demand = DemandBound::FiniteUpperBound {
            bound: allocation.clone(),
            proof: Vec::new(),
        };
        assert_eq!(
            missing_proof.verify_with(
                AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
                [1; 32],
                [2; 32],
                &allocation,
                |_, _| true,
            ),
            Err(AuthorityError::MissingBoundProof)
        );

        let mut unprovable = exact.clone();
        unprovable.demand = DemandBound::Unprovable(UnprovableDemand::DynamicAuthority);
        assert_eq!(
            unprovable.verify(
                AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
                [1; 32],
                [2; 32],
                &allocation,
            ),
            Err(AuthorityError::UnprovableDemand)
        );

        let mut mismatched = exact.clone();
        mismatched.allocation = ResourceMultiset::singleton(key, 2);
        assert_eq!(
            mismatched.verify(
                AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
                [1; 32],
                [2; 32],
                &allocation,
            ),
            Err(AuthorityError::AllocationMismatch)
        );
        assert_eq!(
            exact.verify(
                AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
                [1; 32],
                [2; 32],
                &ResourceMultiset::singleton(key, 2),
            ),
            Err(AuthorityError::InsufficientAuthority)
        );
    }

    #[test]
    fn witness_verification_rejects_every_invalid_binding_fold_and_order_case() {
        let key = [11; 32];
        let cert = certificate(ResourceMultiset::singleton(key, 3));
        let valid_events = vec![
            AuthorityEvent {
                event_id: [1; 32],
                authority: CostAuthority::default(),
                debit: ResourceMultiset::singleton(key, 1),
            },
            AuthorityEvent {
                event_id: [2; 32],
                authority: CostAuthority::default(),
                debit: ResourceMultiset::singleton(key, 1),
            },
        ];
        let valid = AuthorityCostWitness {
            protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
            certificate_id: cert.certificate_id(),
            pre_state_root: cert.pre_state_root,
            post_state_root: [12; 32],
            events: valid_events.clone(),
            realized: ResourceMultiset::singleton(key, 2),
            settlement: ResourceMultiset::singleton(key, 2),
            byte_cost_schedule_version: byte_schedule_version(),
            byte_cost_schedule_digest: byte_schedule_digest(),
            byte_events: Vec::new(),
            byte_cost: 0,
            byte_settlement: ResourceMultiset::default(),
            physical_draws: Vec::new(),
            born_stacks: Vec::new(),
        };
        valid.verify(&cert).unwrap();

        let mut wrong_version = valid.clone();
        wrong_version.protocol_version += 1;
        assert_eq!(
            wrong_version.verify(&cert),
            Err(AuthorityError::ProtocolVersionMismatch)
        );

        let mut wrong_certificate = valid.clone();
        wrong_certificate.certificate_id = [13; 32];
        assert_eq!(
            wrong_certificate.verify(&cert),
            Err(AuthorityError::CertificateMismatch)
        );

        let mut wrong_pre_state = valid.clone();
        wrong_pre_state.pre_state_root = [14; 32];
        assert_eq!(
            wrong_pre_state.verify(&cert),
            Err(AuthorityError::PreStateMismatch)
        );

        let mut noncanonical = valid.clone();
        noncanonical.events[1].event_id = noncanonical.events[0].event_id;
        assert_eq!(
            noncanonical.verify(&cert),
            Err(AuthorityError::NonCanonicalEventOrder)
        );

        let mut causally_reversed = valid.clone();
        causally_reversed.events.reverse();
        causally_reversed.verify(&cert).unwrap();
        assert_ne!(causally_reversed.witness_id(), valid.witness_id());

        let mut wrong_realized = valid.clone();
        wrong_realized.realized = ResourceMultiset::singleton(key, 1);
        assert_eq!(
            wrong_realized.verify(&cert),
            Err(AuthorityError::RealizedCostMismatch)
        );

        let excessive_event = AuthorityCostWitness {
            events: vec![AuthorityEvent {
                event_id: [1; 32],
                authority: CostAuthority::default(),
                debit: ResourceMultiset::singleton(key, 4),
            }],
            realized: ResourceMultiset::singleton(key, 4),
            ..valid.clone()
        };
        assert_eq!(
            excessive_event.verify(&cert),
            Err(AuthorityError::RealizedCostExceedsReservation)
        );

        let overflowing_fold = AuthorityCostWitness {
            events: vec![
                AuthorityEvent {
                    event_id: [1; 32],
                    authority: CostAuthority::default(),
                    debit: ResourceMultiset::singleton(key, u64::MAX),
                },
                AuthorityEvent {
                    event_id: [2; 32],
                    authority: CostAuthority::default(),
                    debit: ResourceMultiset::singleton(key, 1),
                },
            ],
            realized: ResourceMultiset::default(),
            ..valid
        };
        assert_eq!(
            overflowing_fold.verify(&cert),
            Err(AuthorityError::ArithmeticOverflow)
        );
    }

    #[test]
    fn canonical_identifiers_commit_every_demand_reason_key_and_event_field() {
        let vector_key = vec![1, 2, 3];
        let allocation = ResourceMultiset::singleton(vector_key.clone(), 2);
        let base = FundingCertificate {
            protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
            program_hash: [1; 32],
            pre_state_root: [2; 32],
            reservation_id: [3; 32],
            demand: DemandBound::Exact(allocation.clone()),
            allocation: allocation.clone(),
            stack_reservations: BTreeMap::new(),
            fee_allocation: ResourceMultiset::default(),
            fee_plan: None,
            fee_recipient: Vec::new(),
            byte_cost_schedule_version: byte_schedule_version(),
            byte_cost_schedule_digest: byte_schedule_digest(),
            byte_cost_bound: 0,
            byte_allocation: ResourceMultiset::default(),
        };
        let base_id = base.certificate_id();
        for reason in [
            UnprovableDemand::RecursiveDequotation,
            UnprovableDemand::DynamicAuthority,
            UnprovableDemand::UnboundedControlFlow,
            UnprovableDemand::UnsupportedSyntax,
        ] {
            let mut changed = base.clone();
            changed.demand = DemandBound::Unprovable(reason);
            assert_ne!(base_id, changed.certificate_id());
        }
        let mut finite = base.clone();
        finite.demand = DemandBound::FiniteUpperBound {
            bound: allocation.clone(),
            proof: vec![4],
        };
        assert_ne!(base_id, finite.certificate_id());
        let mut stack_bound = base.clone();
        stack_bound.stack_reservations.insert([9; 32], 2);
        assert_ne!(base_id, stack_bound.certificate_id());
        let mut fee_recipient = base.clone();
        fee_recipient.fee_recipient = vec![10; 65];
        assert_ne!(base_id, fee_recipient.certificate_id());

        let witness = AuthorityCostWitness {
            protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
            certificate_id: base_id,
            pre_state_root: [2; 32],
            post_state_root: [5; 32],
            events: vec![AuthorityEvent {
                event_id: [6; 32],
                authority: CostAuthority::default(),
                debit: ResourceMultiset::singleton(vector_key, 1),
            }],
            realized: ResourceMultiset::singleton(vec![1, 2, 3], 1),
            settlement: ResourceMultiset::singleton(vec![1, 2, 3], 1),
            byte_cost_schedule_version: byte_schedule_version(),
            byte_cost_schedule_digest: byte_schedule_digest(),
            byte_events: Vec::new(),
            byte_cost: 0,
            byte_settlement: ResourceMultiset::default(),
            physical_draws: Vec::new(),
            born_stacks: Vec::new(),
        };
        let witness_id = witness.witness_id();
        let mut changed = witness;
        changed.post_state_root = [7; 32];
        assert_ne!(witness_id, changed.witness_id());
    }

    proptest! {
        #[test]
        fn complete_join_payable_atoms_preserve_all_surfaces(
            clauses in prop::collection::vec((
                prop::collection::vec(0_u8..5, 0..7),
                prop::collection::vec(0_u8..5, 0..7),
            ), 0..17),
            cuts in prop::collection::vec(any::<bool>(), 0..192),
        ) {
            let unit = sig_to_cost_signature(&Sig::Unit).unwrap();
            let surfaces = clauses.iter().flat_map(|(receiver, sender)| [receiver, sender])
                .map(|atoms| atoms.iter().fold(unit.clone(), |compound, atom| {
                    let next = if *atom == 0 { unit.clone() } else { ground(&[*atom]) };
                    compound_cost_signatures(&compound, &next).unwrap()
                })).collect::<Vec<_>>();
            let mut expected = clauses.iter().flat_map(|(receiver, sender)| receiver.iter().chain(sender))
                .filter(|atom| **atom != 0)
                .map(|atom| ground(&[*atom]).encode_to_vec())
                .collect::<Vec<_>>();
            expected.sort();
            let combined = surfaces.iter().fold(unit.clone(), |compound, surface| {
                compound_cost_signatures(&compound, surface).unwrap()
            });
            let mut combined_atoms = signature_atoms(&combined).unwrap().iter()
                .map(Message::encode_to_vec).collect::<Vec<_>>();
            combined_atoms.sort();
            prop_assert_eq!(&combined_atoms, &expected);
            let join = event(&surfaces);
            let mut event_atoms = super::event_atoms(&join).unwrap().iter()
                .map(Message::encode_to_vec).collect::<Vec<_>>();
            event_atoms.sort();
            prop_assert_eq!(&event_atoms, &expected);

            let mut balances = ResourceMultiset::default();
            let mut signatures = BTreeMap::new();
            for atom in clauses.iter().flat_map(|(receiver, sender)| receiver.iter().chain(sender))
                .filter(|atom| **atom != 0)
            {
                let signature = ground(&[*atom]);
                let key = cost_signature_to_sig(&signature).unwrap().lane_hash();
                *balances.0.entry(key).or_default() += 1;
                signatures.insert(key, signature);
            }
            let inventory = AuthorityPhysicalInventory {
                balances: balances.clone(),
                balance_custody: balances.0.keys().map(|key| (*key, *key)).collect(),
                stacks: BTreeMap::new(),
                born_stacks: BTreeMap::new(),
            };
            let draw = AuthorityPhysicalEventDraw {
                event_id: join.event_id,
                balances: balances.clone(),
                stack_ids: Vec::new(),
            };
            let settled = verify_physical_settlement(
                std::slice::from_ref(&join), &signatures, &inventory, std::slice::from_ref(&draw),
            ).unwrap();
            prop_assert_eq!(&settled.balance_debit, &balances);

            let mut groups = Vec::<CostSignature>::new();
            for (index, atom) in clauses.iter()
                .flat_map(|(receiver, sender)| receiver.iter().chain(sender))
                .filter(|atom| **atom != 0).enumerate()
            {
                let signature = ground(&[*atom]);
                if index == 0 || cuts.get(index - 1).copied().unwrap_or(false) {
                    groups.push(signature);
                } else {
                    let group = groups.last_mut().unwrap();
                    *group = compound_cost_signatures(group, &signature).unwrap();
                }
            }
            let mut grouped_balances = ResourceMultiset::default();
            let mut grouped_signatures = BTreeMap::new();
            for group in groups {
                let key = cost_signature_to_sig(&group).unwrap().lane_hash();
                *grouped_balances.0.entry(key).or_default() += 1;
                grouped_signatures.insert(key, group);
            }
            let grouped_inventory = AuthorityPhysicalInventory {
                balances: grouped_balances.clone(),
                balance_custody: grouped_balances.0.keys().map(|key| (*key, *key)).collect(),
                stacks: BTreeMap::new(),
                born_stacks: BTreeMap::new(),
            };
            let grouped_draw = AuthorityPhysicalEventDraw {
                event_id: join.event_id,
                balances: grouped_balances.clone(),
                stack_ids: Vec::new(),
            };
            let grouped = verify_physical_settlement(
                std::slice::from_ref(&join), &grouped_signatures, &grouped_inventory, &[grouped_draw],
            ).unwrap();
            prop_assert_eq!(grouped.balance_debit, grouped_balances);

            let reversed = event(&surfaces.into_iter().rev().collect::<Vec<_>>());
            let regrouped = verify_physical_settlement(
                &[reversed], &signatures, &inventory, std::slice::from_ref(&draw),
            ).unwrap();
            prop_assert_eq!(&regrouped.balance_debit, &balances);
            if let Some(key) = balances.0.keys().next().copied() {
                let mut weakened = draw.clone();
                weakened.balances = weakened.balances.checked_sub(&ResourceMultiset::singleton(key, 1)).unwrap();
                prop_assert_eq!(
                    verify_physical_settlement(&[join], &signatures, &inventory, &[weakened]),
                    Err(AuthorityError::PhysicalAuthorityMismatch),
                );
            }
        }

        #[test]
        fn born_stack_readiness_matches_all_creation_events(
            seeds in prop::collection::vec((1_u8..5, any::<u64>()), 1..13),
            insertion_seed in any::<usize>(),
            use_seed in any::<usize>(),
        ) {
            let cells = seeds.iter().map(|(atom, _)| ground(&[*atom])).collect::<Vec<_>>();
            let payer = ground(b"creation-payer");
            let payer_lane = cost_signature_to_sig(&payer).unwrap().lane_hash();
            let produce_hash = [199; 32];
            let stack_id = [198; 32];
            let inventory = AuthorityPhysicalInventory {
                balances: ResourceMultiset::singleton(payer_lane, cells.len() as u64),
                balance_custody: BTreeMap::from([(payer_lane, payer_lane)]),
                stacks: BTreeMap::from([(stack_id, cells.clone())]),
                born_stacks: BTreeMap::from([(stack_id, produce_hash)]),
            };
            let signatures = BTreeMap::from([(payer_lane, payer.clone())]);
            let mut creation_order = (0..cells.len()).collect::<Vec<_>>();
            creation_order.sort_by_key(|index| (seeds[*index].1, *index));
            let creations = creation_order.into_iter().map(|index| {
                let mut creation = event(std::slice::from_ref(&payer));
                creation.event_id = stack_transfer_event_id(&produce_hash, index as u64);
                let draw = AuthorityPhysicalEventDraw {
                    event_id: creation.event_id,
                    balances: ResourceMultiset::singleton(payer_lane, 1),
                    stack_ids: Vec::new(),
                };
                (creation, draw)
            }).collect::<Vec<_>>();
            let used = 1 + use_seed % cells.len();
            let uses = cells.iter().take(used).enumerate().map(|(index, signature)| {
                let mut use_event = event(std::slice::from_ref(signature));
                use_event.event_id = [197; 32];
                use_event.event_id[0] = index as u8;
                let draw = AuthorityPhysicalEventDraw {
                    event_id: use_event.event_id,
                    balances: ResourceMultiset::default(),
                    stack_ids: vec![stack_id],
                };
                (use_event, draw)
            }).collect::<Vec<_>>();
            let insertion = insertion_seed % (creations.len() + 1);
            let mut candidate = creations.clone();
            candidate.splice(insertion..insertion, uses.clone());
            let (events, draws): (Vec<_>, Vec<_>) = candidate.into_iter().unzip();
            let checked = verify_physical_settlement(&events, &signatures, &inventory, &draws);
            if insertion == cells.len() {
                let settlement = checked.unwrap();
                prop_assert_eq!(settlement.stack_pops, BTreeMap::from([(stack_id, used as u64)]));
                prop_assert_eq!(settlement.balance_debit, inventory.balances.clone());
            } else {
                prop_assert_eq!(checked, Err(AuthorityError::PhysicalAuthorityMismatch));
                let mut without_birth_guard = inventory.clone();
                without_birth_guard.born_stacks.clear();
                prop_assert!(verify_physical_settlement(&events, &signatures, &without_birth_guard, &draws).is_ok());
            }

            let mut complete = creations;
            complete.extend(uses);
            let (complete_events, complete_draws): (Vec<_>, Vec<_>) = complete.clone().into_iter().unzip();
            let settlement = verify_physical_settlement(&complete_events, &signatures, &inventory, &complete_draws).unwrap();
            prop_assert_eq!(settlement.stack_pops, BTreeMap::from([(stack_id, used as u64)]));
            prop_assert_eq!(settlement.balance_debit, inventory.balances.clone());
            complete.remove(0);
            let (missing_events, missing_draws): (Vec<_>, Vec<_>) = complete.into_iter().unzip();
            prop_assert_eq!(verify_physical_settlement(&missing_events, &signatures, &inventory, &missing_draws),
                Err(AuthorityError::SettlementPresentationMismatch));
            let mut empty = inventory;
            empty.stacks.insert(stack_id, Vec::new());
            prop_assert_eq!(verify_physical_settlement(&complete_events, &signatures, &empty, &complete_draws),
                Err(AuthorityError::MissingSignature));
            prop_assert_eq!(verify_physical_settlement(&[], &signatures, &empty, &[]),
                Err(AuthorityError::MissingSignature));
            empty.stacks.insert(stack_id, cells);
            prop_assert_eq!(verify_physical_settlement(&[], &signatures, &empty, &[]),
                Err(AuthorityError::SettlementPresentationMismatch));
            empty.stacks.remove(&stack_id);
            prop_assert_eq!(verify_physical_settlement(&[], &signatures, &empty, &[]),
                Err(AuthorityError::UnknownStackResource));
        }

        #[test]
        fn selected_stack_histories_preserve_exact_prefixes(
            seeds in prop::collection::vec(prop::collection::vec(1_u8..5, 1..8), 1..9),
            masks in prop::collection::vec(any::<u8>(), 1..33),
            split_seed in any::<usize>(),
        ) {
            let stacks = seeds.iter().enumerate().map(|(index, seed)| {
                let cells = (0..masks.len() + seed.len())
                    .map(|position| ground(&[seed[position % seed.len()]]))
                    .collect::<Vec<_>>();
                ([index as u8 + 1; 32], cells)
            }).chain(std::iter::once(([240; 32], vec![ground(b"untouched")]))).collect::<BTreeMap<_, _>>();
            let inventory = AuthorityPhysicalInventory {
                balances: ResourceMultiset::default(),
                balance_custody: BTreeMap::new(),
                stacks: stacks.clone(),
                born_stacks: BTreeMap::new(),
            };
            let mut counts = BTreeMap::<[u8; 32], u64>::new();
            let mut events = Vec::new();
            let mut draws = Vec::new();
            for (position, mask) in masks.iter().enumerate() {
                let mut ids = (0..seeds.len())
                    .filter(|index| mask & (1_u8 << index) != 0)
                    .map(|index| [index as u8 + 1; 32])
                    .collect::<Vec<_>>();
                if ids.is_empty() {
                    ids.push([position as u8 % seeds.len() as u8 + 1; 32]);
                }
                let signatures = ids.iter().map(|id| {
                    let used = counts.entry(*id).or_default();
                    let signature = stacks[id][*used as usize].clone();
                    *used += 1;
                    signature
                }).collect::<Vec<_>>();
                let mut selected_event = event(&signatures);
                selected_event.event_id = [position as u8 + 1; 32];
                draws.push(AuthorityPhysicalEventDraw {
                    event_id: selected_event.event_id,
                    balances: ResourceMultiset::default(),
                    stack_ids: ids,
                });
                events.push(selected_event);
            }

            let whole = verify_physical_settlement(&events, &BTreeMap::new(), &inventory, &draws).unwrap();
            prop_assert_eq!(&whole.stack_pops, &counts);
            prop_assert!(whole.balance_debit.0.is_empty());
            prop_assert!(whole.custody_debit.0.is_empty());
            prop_assert_eq!(&inventory.stacks, &stacks);
            prop_assert!(!whole.stack_pops.contains_key(&[240; 32]));

            let split = split_seed % (events.len() + 1);
            let prefix = verify_physical_settlement(&events[..split], &BTreeMap::new(), &inventory, &draws[..split]).unwrap();
            let residual_stacks = stacks.iter().map(|(id, cells)| {
                let used = prefix.stack_pops.get(id).copied().unwrap_or_default() as usize;
                (*id, cells[used..].to_vec())
            }).collect();
            let residual = AuthorityPhysicalInventory {
                balances: ResourceMultiset::default(),
                balance_custody: BTreeMap::new(),
                stacks: residual_stacks,
                born_stacks: BTreeMap::new(),
            };
            let suffix = verify_physical_settlement(&events[split..], &BTreeMap::new(), &residual, &draws[split..]).unwrap();
            for (id, expected) in &counts {
                prop_assert_eq!(
                    prefix.stack_pops.get(id).copied().unwrap_or_default()
                        + suffix.stack_pops.get(id).copied().unwrap_or_default(),
                    *expected,
                );
            }

            let first_id = draws[0].stack_ids[0];
            let mut exhausted = inventory.clone();
            exhausted.stacks.get_mut(&first_id).unwrap().truncate(counts[&first_id] as usize - 1);
            let exhausted_before = exhausted.stacks.clone();
            prop_assert_eq!(
                verify_physical_settlement(&events, &BTreeMap::new(), &exhausted, &draws),
                Err(AuthorityError::ExhaustedStackResource),
            );
            prop_assert_eq!(exhausted.stacks, exhausted_before);

            let mut duplicate = draws.clone();
            duplicate[0].stack_ids.insert(0, first_id);
            prop_assert_eq!(
                verify_physical_settlement(&events, &BTreeMap::new(), &inventory, &duplicate),
                Err(AuthorityError::NonCanonicalStackDraw),
            );
            let mut missing = inventory.clone();
            missing.stacks.remove(&first_id);
            prop_assert_eq!(
                verify_physical_settlement(&events, &BTreeMap::new(), &missing, &draws),
                Err(AuthorityError::UnknownStackResource),
            );
        }

        #[test]
        fn compound_and_split_stack_histories_preserve_each_occurrence(
            seeds in prop::collection::vec(prop::collection::vec(1_u8..5, 1..8), 1..33),
            rounds in 1_usize..9,
            split_seed in any::<usize>(),
            changed_seed in any::<usize>(),
        ) {
            let split_stacks = seeds.iter().enumerate().map(|(index, seed)| {
                let cells = (0..rounds + 2)
                    .map(|position| ground(&[seed[position % seed.len()]]))
                    .collect::<Vec<_>>();
                ([index as u8 + 1; 32], cells)
            }).collect::<BTreeMap<_, _>>();
            let grouped_cells = (0..rounds + 2).map(|position| {
                let mut signatures = split_stacks.values().map(|cells| cells[position].clone());
                let first = signatures.next().unwrap();
                signatures.fold(first, |left, right| compound_cost_signatures(&left, &right).unwrap())
            }).collect::<Vec<_>>();
            let group_id = [200; 32];
            let untouched_id = [240; 32];
            let untouched = vec![ground(b"untouched")];
            let split_inventory = AuthorityPhysicalInventory {
                balances: ResourceMultiset::default(),
                balance_custody: BTreeMap::new(),
                stacks: split_stacks.clone().into_iter()
                    .chain(std::iter::once((untouched_id, untouched.clone()))).collect(),
                born_stacks: BTreeMap::new(),
            };
            let grouped_inventory = AuthorityPhysicalInventory {
                stacks: BTreeMap::from([(group_id, grouped_cells), (untouched_id, untouched)]),
                ..split_inventory.clone()
            };
            let events = (0..rounds).map(|position| {
                let signatures = split_stacks.values().map(|cells| cells[position].clone()).collect::<Vec<_>>();
                let mut current = event(&signatures);
                current.event_id = [position as u8 + 1; 32];
                current
            }).collect::<Vec<_>>();
            let grouped_draws = events.iter().map(|current| AuthorityPhysicalEventDraw {
                event_id: current.event_id,
                balances: ResourceMultiset::default(),
                stack_ids: vec![group_id],
            }).collect::<Vec<_>>();
            let split_draws = events.iter().map(|current| AuthorityPhysicalEventDraw {
                stack_ids: split_stacks.keys().copied().collect(),
                event_id: current.event_id,
                balances: ResourceMultiset::default(),
            }).collect::<Vec<_>>();
            for (inventory, draws, selected) in [
                (&grouped_inventory, &grouped_draws, vec![group_id]),
                (&split_inventory, &split_draws, split_stacks.keys().copied().collect()),
            ] {
                let original = inventory.stacks.clone();
                let whole = verify_physical_settlement(&events, &BTreeMap::new(), inventory, draws).unwrap();
                let expected = selected.iter().map(|id| (*id, rounds as u64)).collect::<BTreeMap<_, _>>();
                prop_assert_eq!(&whole.stack_pops, &expected);
                prop_assert!(whole.balance_debit.0.is_empty());
                prop_assert!(whole.custody_debit.0.is_empty());
                prop_assert_eq!(&inventory.stacks, &original);

                let split = split_seed % (rounds + 1);
                let prefix = verify_physical_settlement(&events[..split], &BTreeMap::new(), inventory, &draws[..split]).unwrap();
                let residual = AuthorityPhysicalInventory {
                    stacks: inventory.stacks.iter().map(|(id, cells)| {
                        let used = prefix.stack_pops.get(id).copied().unwrap_or_default() as usize;
                        (*id, cells[used..].to_vec())
                    }).collect(),
                    ..inventory.clone()
                };
                let suffix = verify_physical_settlement(&events[split..], &BTreeMap::new(), &residual, &draws[split..]).unwrap();
                for id in &selected {
                    let first = prefix.stack_pops.get(id).copied().unwrap_or_default();
                    let last = suffix.stack_pops.get(id).copied().unwrap_or_default();
                    prop_assert_eq!(first + last, rounds as u64);
                    prop_assert_eq!(&residual.stacks[id][last as usize..], &original[id][rounds..]);
                }
                prop_assert_eq!(&residual.stacks[&untouched_id], &original[&untouched_id]);

                let mut changed = inventory.clone();
                let changed_id = selected[changed_seed % selected.len()];
                changed.stacks.get_mut(&changed_id).unwrap()[changed_seed % rounds] = ground(b"unfunded");
                prop_assert_eq!(
                    verify_physical_settlement(&events, &BTreeMap::new(), &changed, draws),
                    Err(AuthorityError::PhysicalAuthorityMismatch),
                );

                let mut duplicated = draws.clone();
                duplicated[0].stack_ids.insert(0, selected[0]);
                prop_assert_eq!(
                    verify_physical_settlement(&events, &BTreeMap::new(), inventory, &duplicated),
                    Err(AuthorityError::NonCanonicalStackDraw),
                );
            }
        }

        #[test]
        fn authority_discovery_units_match_arbitrary_tree_shape(leaf_count in 1_u64..17) {
            let mut signature = ground(&[0]);
            for index in 1..leaf_count {
                signature = CostSignature {
                    value: Some(CostSignatureValue::Compound(CostSignatureCompound {
                        elements: vec![signature, ground(&[index as u8])],
                    })),
                };
            }
            let host_work =
                HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000)));

            let result = authority_funding_signatures_for_events_with_host_work(
                &[],
                &[],
                std::slice::from_ref(&signature),
                Some(&host_work),
            );

            prop_assert!(!matches!(result, Err(AuthorityError::HostWorkRejected)));
            prop_assert_eq!(
                host_work.usage(HostWorkDimension::AuthorityNodes),
                HostWorkUsage::new(leaf_count * 2 - 1)
            );
            prop_assert_eq!(
                host_work.usage(HostWorkDimension::AuthorityDepth),
                HostWorkUsage::new(leaf_count)
            );
        }

        #[test]
        fn bounded_search_preserves_unbounded_results(
            event_count in 1_u64..33,
            slack in 0_u64..33,
        ) {
            let signature = ground(b"bounded-search-property");
            let key = cost_signature_to_sig(&signature).unwrap().lane_hash();
            let events = (0..event_count)
                .map(|index| {
                    let mut authority_event = event(std::slice::from_ref(&signature));
                    authority_event.event_id[..8].copy_from_slice(&index.to_le_bytes());
                    authority_event
                })
                .collect::<Vec<_>>();
            let signatures = BTreeMap::from([(key, signature)]);
            let inventory = AuthorityPhysicalInventory {
                balances: ResourceMultiset::singleton(key, event_count + slack),
                balance_custody: BTreeMap::from([(key, key)]),
                stacks: BTreeMap::new(),
                born_stacks: BTreeMap::new(),
            };
            let expected = allocate_physical_settlement(&events, &signatures, &inventory);
            let host_work = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(
                10_000_000,
            )));
            let actual = allocate_physical_settlement_with_host_work(
                &events,
                &signatures,
                &inventory,
                Some(&host_work),
            );

            prop_assert_eq!(actual, expected);
            prop_assert!(!host_work.is_rejected());
        }

        #[test]
        fn worklist_settlement_preserves_event_order_and_exact_debits(
            choices in prop::collection::vec(any::<bool>(), 1..257),
        ) {
            let left = ground(b"worklist-left");
            let right = ground(b"worklist-right");
            let left_key = cost_signature_to_sig(&left).unwrap().lane_hash();
            let right_key = cost_signature_to_sig(&right).unwrap().lane_hash();
            let mut balances = ResourceMultiset::default();
            let events = choices
                .iter()
                .enumerate()
                .map(|(index, choose_right)| {
                    let (signature, key) = if *choose_right {
                        (&right, right_key)
                    } else {
                        (&left, left_key)
                    };
                    balances = balances
                        .checked_add(&ResourceMultiset::singleton(key, 1))
                        .unwrap();
                    let mut authority_event = event(std::slice::from_ref(signature));
                    authority_event.event_id[..8]
                        .copy_from_slice(&(index as u64).to_le_bytes());
                    authority_event
                })
                .collect::<Vec<_>>();
            let inventory = AuthorityPhysicalInventory {
                balances: balances.clone(),
                balance_custody: balances.0.keys().map(|key| (*key, *key)).collect(),
                stacks: BTreeMap::new(),
                born_stacks: BTreeMap::new(),
            };
            let settlement = allocate_physical_settlement(
                &events,
                &BTreeMap::from([(left_key, left), (right_key, right)]),
                &inventory,
            )
            .unwrap();

            prop_assert_eq!(settlement.draws.len(), events.len());
            prop_assert_eq!(settlement.balance_debit, balances);
            prop_assert!(settlement
                .draws
                .iter()
                .zip(&events)
                .all(|(draw, authority_event)| draw.event_id == authority_event.event_id));
        }

        #[test]
        fn every_contiguous_partition_is_a_valid_physical_join(
            atom_count in 1_usize..9,
            cuts in prop::collection::vec(any::<bool>(), 0..8),
        ) {
            let atoms = (0..atom_count)
                .map(|index| ground(&[index as u8 + 1]))
                .collect::<Vec<_>>();
            let event = event(&atoms);
            let mut groups = Vec::<Vec<CostSignature>>::new();
            for (index, atom) in atoms.into_iter().enumerate() {
                if index == 0 || cuts.get(index - 1).copied().unwrap_or(false) {
                    groups.push(Vec::new());
                }
                groups.last_mut().unwrap().push(atom);
            }
            let mut balances = ResourceMultiset::default();
            let mut signatures = BTreeMap::new();
            for group in groups {
                let signature = signature_from_atoms(&group).unwrap();
                let key = cost_signature_to_sig(&signature).unwrap().lane_hash();
                balances = balances
                    .checked_add(&ResourceMultiset::singleton(key, 1))
                    .unwrap();
                signatures.insert(key, signature);
            }
            let inventory = AuthorityPhysicalInventory {
                balances: balances.clone(),
                balance_custody: balances.0.keys().map(|key| (*key, *key)).collect(),
                stacks: BTreeMap::new(),
                born_stacks: BTreeMap::new(),
            };
            let draws = vec![AuthorityPhysicalEventDraw {
                event_id: event.event_id,
                balances: balances.clone(),
                stack_ids: Vec::new(),
            }];

            prop_assert_eq!(
                verify_physical_settlement(
                    std::slice::from_ref(&event),
                    &signatures,
                    &inventory,
                    &draws,
                )
                .unwrap()
                .balance_debit,
                balances,
            );
            prop_assert!(
                allocate_physical_settlement(
                    std::slice::from_ref(&event),
                    &signatures,
                    &inventory,
                )
                .is_ok()
            );
        }

        #[test]
        fn authority_merge_is_permutation_invariant(
            a in prop::collection::vec(any::<u8>(), 0..64),
            b in prop::collection::vec(any::<u8>(), 0..64),
        ) {
            let first = cost_region(&ground(&a), b"first", 0).unwrap();
            let second = cost_region(&ground(&b), b"second", 0).unwrap();
            let left = CostAuthority { regions: vec![first.clone(), second.clone()] };
            let right = CostAuthority { regions: vec![second, first] };
            prop_assert_eq!(canonical_authority(&left), canonical_authority(&right));
        }

        #[test]
        fn addition_is_commutative(a in 0u64..1_000_000, b in 0u64..1_000_000) {
            let left = ResourceMultiset::singleton([1; 32], a);
            let right = ResourceMultiset::singleton([1; 32], b);
            prop_assert_eq!(left.checked_add(&right), right.checked_add(&left));
        }

        #[test]
        fn verified_realized_cost_never_exceeds_reservation(
            reserved in 0u64..10_000,
            realized in 0u64..10_000,
        ) {
            let key = [9; 32];
            let cert = certificate(ResourceMultiset::singleton(key, reserved));
            let witness = AuthorityCostWitness {
                protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
                certificate_id: cert.certificate_id(),
                pre_state_root: [2; 32],
                post_state_root: [4; 32],
                events: if realized == 0 { Vec::new() } else { vec![AuthorityEvent {
                    event_id: [5; 32],
                    authority: CostAuthority::default(),
                    debit: ResourceMultiset::singleton(key, realized),
                }]},
                realized: ResourceMultiset::singleton(key, realized),
                settlement: ResourceMultiset::singleton(key, realized),
                byte_cost_schedule_version: byte_schedule_version(),
                byte_cost_schedule_digest: byte_schedule_digest(),
                byte_events: Vec::new(),
                byte_cost: 0,
                byte_settlement: ResourceMultiset::default(),
                physical_draws: Vec::new(),
                born_stacks: Vec::new(),
            };
            prop_assert_eq!(witness.verify(&cert).is_ok(), realized <= reserved);
        }

        #[test]
        fn quantitative_compound_debit_is_balanced_and_exact(
            amount in 1u64..1_000_000,
            slack in 0u64..1_000_000,
        ) {
            let left = ground(b"quantitative-property-left");
            let right = ground(b"quantitative-property-right");
            let event = event(&[left.clone(), right.clone()]);
            let left_key = cost_signature_to_sig(&left).unwrap().lane_hash();
            let right_key = cost_signature_to_sig(&right).unwrap().lane_hash();
            let available = ResourceMultiset(BTreeMap::from([
                (left_key, amount.checked_add(slack).unwrap()),
                (right_key, amount.checked_add(slack).unwrap()),
            ]));

            let debit = allocate_quantitative_debit(&event, amount, &available).unwrap();
            prop_assert_eq!(debit.get(&left_key), amount);
            prop_assert_eq!(debit.get(&right_key), amount);
            prop_assert_eq!(debit.0.len(), 2);
            prop_assert!(available.dominates(&debit));
        }

        #[test]
        fn quantitative_event_permutations_preserve_local_conservation(
            outer_amount in 1u64..100_000,
            continuation_amount in 1u64..100_000,
            outer_slack in 0u64..100_000,
            continuation_slack in 0u64..100_000,
        ) {
            let outer = ground(b"property-outer");
            let continuation = ground(b"property-continuation");
            let outer_key = cost_signature_to_sig(&outer).unwrap().lane_hash();
            let continuation_key = cost_signature_to_sig(&continuation).unwrap().lane_hash();
            let events = vec![
                byte_event(
                    1,
                    AuthorityByteEventKind::Comm,
                    std::slice::from_ref(&outer),
                    outer_amount,
                ),
                byte_event(
                    2,
                    AuthorityByteEventKind::ConsumeIntroduction,
                    std::slice::from_ref(&continuation),
                    continuation_amount,
                ),
            ];
            let available = ResourceMultiset(BTreeMap::from([
                (outer_key, outer_amount.checked_add(outer_slack).unwrap()),
                (
                    continuation_key,
                    continuation_amount.checked_add(continuation_slack).unwrap(),
                ),
            ]));
            let mut reversed = events.clone();
            reversed.reverse();
            let debit = allocate_quantitative_events(&events, &available).unwrap();

            prop_assert_eq!(debit.clone(), allocate_quantitative_events(&reversed, &available).unwrap());
            prop_assert_eq!(debit.get(&outer_key), outer_amount);
            prop_assert_eq!(debit.get(&continuation_key), continuation_amount);
            prop_assert!(available.dominates(&debit));
        }
    }

    /// A Par nested `depth` levels deep through sends.
    fn send_chain(depth: usize) -> Par {
        let mut par = Par::default();
        for _ in 0..depth {
            par = Par {
                sends: vec![models::rhoapi::Send {
                    chan: Some(Par::default()),
                    data: vec![par],
                    ..Default::default()
                }],
                ..Default::default()
            };
        }
        par
    }

    fn quoted_chain(depth: usize) -> CostSignature {
        CostSignature {
            value: Some(CostSignatureValue::Quote(send_chain(depth))),
        }
    }

    fn scanned_by(charge: impl FnOnce(&dyn BackingMeter) -> Result<(), AuthorityError>) -> usize {
        let scanned = std::cell::Cell::new(0_usize);
        let meter = |_: usize, bytes: usize, _: usize| {
            scanned.set(scanned.get() + bytes);
            Ok(())
        };
        charge(&meter).expect("charge");
        scanned.get()
    }

    /// D-O6 (DR-93): the sites that encode a quoted Par reserve
    /// depth-weighted traversals, so their charge grows quadratically with
    /// the nesting depth; the results are unchanged.
    #[test]
    fn nested_encode_sites_charge_quadratically_in_depth() {
        let conversion = |depth: usize| {
            scanned_by(|meter| cost_signature_to_sig_metered(&quoted_chain(depth), meter).map(drop))
        };
        let lane = |depth: usize| {
            scanned_by(|meter| {
                cost_signature_lane_metered(&quoted_chain(depth), &SorterMeter::new(meter))
                    .map(drop)
            })
        };
        let region = |depth: usize| {
            scanned_by(|meter| {
                cost_region_metered(&quoted_chain(depth), b"identity", 0, meter).map(drop)
            })
        };
        let sites: [&dyn Fn(usize) -> usize; 3] = [&conversion, &lane, &region];
        for charge in sites {
            let (small, medium, large) = (charge(8), charge(16), charge(32));
            assert!(
                large - medium > 3 * (medium - small),
                "{small} {medium} {large}"
            );
        }
        for depth in [0, 3, 9] {
            let signature = quoted_chain(depth);
            assert_eq!(
                cost_signature_to_sig_metered(&signature, &|_, _, _| Ok(())).unwrap(),
                cost_signature_to_sig(&signature).unwrap()
            );
            assert_eq!(
                cost_region_metered(&signature, b"identity", 0, &|_, _, _| Ok(())).unwrap(),
                cost_region(&signature, b"identity", 0).unwrap()
            );
        }
    }
    /// The usage that `charge` reserves when every running total must stay
    /// within `limit`.
    fn run_within(
        limit: [usize; 3],
        charge: &dyn Fn(&dyn BackingMeter) -> Result<(), AuthorityError>,
    ) -> Result<[usize; 3], AuthorityError> {
        let used = std::cell::Cell::new([0_usize; 3]);
        let meter = |operations: usize, scanned: usize, backing: usize| {
            let [total_operations, total_scanned, total_backing] = used.get();
            let next = [
                total_operations + operations,
                total_scanned + scanned,
                total_backing + backing,
            ];
            if next.iter().zip(limit).any(|(total, bound)| *total > bound) {
                return Err(BackingError::Rejected);
            }
            used.set(next);
            Ok(())
        };
        charge(&meter)?;
        Ok(used.get())
    }

    fn assert_exact_credit(
        site: &str,
        charge: &dyn Fn(&dyn BackingMeter) -> Result<(), AuthorityError>,
    ) {
        let exact = run_within([usize::MAX; 3], charge).expect("unlimited");
        assert_eq!(run_within(exact, charge).expect("exact"), exact, "{site}");
        for dimension in 0..3 {
            if exact[dimension] > 0 {
                let mut smaller = exact;
                smaller[dimension] -= 1;
                assert!(
                    matches!(
                        run_within(smaller, charge),
                        Err(AuthorityError::HostWorkRejected)
                    ),
                    "{site}: dimension {dimension}"
                );
            }
        }
    }

    /// D-O1 (DR-94): the block-mode authority sites reserve their exact
    /// credit before their work: each accepts the credit that it uses and
    /// rejects one unit less in any dimension.
    #[test]
    fn block_mode_authority_sites_accept_exact_credit() {
        for depth in [0, 2, 5] {
            let signature = quoted_chain(depth);
            let authority = CostAuthority {
                regions: vec![
                    CostRegion {
                        instance_id: vec![1; 32],
                        signature: Some(signature.clone()),
                    },
                    CostRegion {
                        instance_id: vec![2; 32],
                        signature: Some(signature.clone()),
                    },
                ],
            };
            assert_exact_credit("conversion", &|meter| {
                cost_signature_to_sig_metered(&signature, meter).map(drop)
            });
            assert_exact_credit("region", &|meter| {
                cost_region_metered(&signature, b"identity", 0, meter).map(drop)
            });
            assert_exact_credit("demand", &|meter| {
                authority_demand_metered(&authority, meter).map(drop)
            });
            assert_exact_credit("merge", &|meter| {
                merge_authorities_metered([&authority, &authority], meter).map(drop)
            });
            // Added by D-F1 (DR-117): the witness sites.
            let witness = canonical_regions(&authority.regions).expect("canonical regions");
            let persistent = BTreeSet::from([[1; 32]]);
            assert_exact_credit("witness merge", &|meter| {
                merge_canonical_metered([&authority, &authority], meter).map(drop)
            });
            assert_exact_credit("rekey", &|meter| {
                rekey_metered(witness.clone(), &persistent, [3; 32], meter).map(drop)
            });
            assert_exact_credit("witness demand", &|meter| {
                authority_demand_from_canonical_metered(&witness, meter).map(drop)
            });
        }
    }

    /// D-F1 (DR-117): the authority pipeline of one COMM before the witness.
    /// It runs the copying merge, the instantiation (which canonicalizes
    /// before and after the new identities), the canonicalization of the
    /// observation, the empty-authority check and the demand, which
    /// canonicalizes again.
    fn legacy_comm_pipeline(
        participants: &[CostAuthority],
        persistent: &BTreeSet<[u8; 32]>,
        occurrence: [u8; 32],
        meter: &dyn BackingMeter,
    ) -> Result<(CostAuthority, ResourceMultiset<[u8; 32]>), AuthorityError> {
        let merged = merge_authorities_metered_legacy(participants, meter)?;
        let instantiated =
            instantiate_persistent_regions_metered(&merged, persistent, occurrence, meter)?;
        let observed = canonical_authority_metered(&instantiated, meter)?;
        if observed.regions.is_empty() {
            return Err(AuthorityError::MissingAuthority);
        }
        let demand = authority_demand_metered(&observed, meter)?;
        Ok((observed, demand))
    }

    /// D-F1 (DR-117): the same pipeline with the witness: the in-place merge,
    /// the rekey, the empty-authority check and the demand of the witness.
    fn witness_comm_pipeline(
        participants: &[CostAuthority],
        persistent: &BTreeSet<[u8; 32]>,
        occurrence: [u8; 32],
        meter: &dyn BackingMeter,
    ) -> Result<(CostAuthority, ResourceMultiset<[u8; 32]>), AuthorityError> {
        let merged = merge_canonical_metered(participants, meter)?;
        let rekeyed = rekey_metered(merged, persistent, occurrence, meter)?;
        if rekeyed.is_empty() {
            return Err(AuthorityError::MissingAuthority);
        }
        let demand = authority_demand_from_canonical_metered(&rekeyed, meter)?;
        Ok((rekeyed.into_authority(), demand))
    }

    /// D-F1 (DR-117): the charge that `run` reserves, whatever its result.
    fn charge_and_result<T>(
        run: impl FnOnce(&dyn BackingMeter) -> Result<T, AuthorityError>,
    ) -> ([usize; 3], Result<T, AuthorityError>) {
        let totals = std::cell::Cell::new([0_usize; 3]);
        let meter = |operations: usize, scanned: usize, backing: usize| {
            let [total_operations, total_scanned, total_backing] = totals.get();
            totals.set([
                total_operations + operations,
                total_scanned + scanned,
                total_backing + backing,
            ]);
            Ok(())
        };
        let result = run(&meter);
        (totals.get(), result)
    }

    /// D-F1 (DR-117): `run` under a limit in each dimension.
    fn result_within<T>(
        limit: [usize; 3],
        run: impl FnOnce(&dyn BackingMeter) -> Result<T, AuthorityError>,
    ) -> Result<T, AuthorityError> {
        let used = std::cell::Cell::new([0_usize; 3]);
        let meter = |operations: usize, scanned: usize, backing: usize| {
            let [total_operations, total_scanned, total_backing] = used.get();
            let next = [
                total_operations.saturating_add(operations),
                total_scanned.saturating_add(scanned),
                total_backing.saturating_add(backing),
            ];
            if next.iter().zip(limit).any(|(total, bound)| *total > bound) {
                return Err(BackingError::Rejected);
            }
            used.set(next);
            Ok(())
        };
        run(&meter)
    }

    /// D-F1 (DR-117): the signatures of the generated pipeline inputs, with
    /// canonical, non-canonical and missing signatures.
    fn pipeline_signature() -> impl Strategy<Value = Option<CostSignature>> {
        prop_oneof![
            4 => (0_u8..3).prop_map(|byte| Some(ground(&[byte]))),
            2 => (0_u8..3).prop_map(|byte| Some(private_name(&[byte]))),
            2 => (1_usize..4).prop_map(|depth| Some(quoted_chain(depth))),
            1 => Just(Some(CostSignature {
                value: Some(CostSignatureValue::Unit(true)),
            })),
            1 => Just(Some(
                compound_cost_signatures(&ground(&[1]), &ground(&[2])).expect("compound"),
            )),
            1 => Just(None),
            1 => Just(Some(CostSignature { value: None })),
            1 => Just(Some(CostSignature {
                value: Some(CostSignatureValue::Unit(false)),
            })),
            1 => Just(Some(CostSignature {
                value: Some(CostSignatureValue::BoundLevel(0)),
            })),
            1 => Just(Some(CostSignature {
                value: Some(CostSignatureValue::Compound(CostSignatureCompound {
                    elements: vec![ground(&[1])],
                })),
            })),
            1 => Just(Some(CostSignature {
                value: Some(CostSignatureValue::Compound(CostSignatureCompound {
                    elements: vec![ground(&[2]), ground(&[1])],
                })),
            })),
        ]
    }

    /// D-F1 (DR-117): participants with repeated identities (so equal and
    /// conflicting regions occur), short identities and the signatures above.
    fn pipeline_participants() -> impl Strategy<Value = Vec<CostAuthority>> {
        prop::collection::vec(
            prop::collection::vec((0_u8..6, 0_u8..10, pipeline_signature()), 0..4),
            0..4,
        )
        .prop_map(|participants| {
            participants
                .into_iter()
                .map(|regions| CostAuthority {
                    regions: regions
                        .into_iter()
                        .map(|(identity, shape, signature)| CostRegion {
                            instance_id: if shape == 0 {
                                vec![identity; 31]
                            } else {
                                vec![identity; 32]
                            },
                            signature,
                        })
                        .collect(),
                })
                .collect()
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// D-F1 (DR-117): the witness pipeline returns the value or the first
        /// error of the legacy pipeline, on valid, invalid and conflicting
        /// authorities. It reserves no more in any dimension. Under any limit,
        /// a legacy result that is not a host-work rejection is also the
        /// result of the witness pipeline.
        #[test]
        fn canonical_witness_pipeline_matches_legacy_pipeline(
            participants in pipeline_participants(),
            persistent_mask in any::<u8>(),
            occurrence in any::<[u8; 32]>(),
            limit in prop::array::uniform3(0_usize..24_000),
        ) {
            let persistent: BTreeSet<[u8; 32]> = (0_u8..6)
                .filter(|identity| persistent_mask & (1 << identity) != 0)
                .map(|identity| [identity; 32])
                .collect();
            let (legacy_charge, legacy) = charge_and_result(|meter| {
                legacy_comm_pipeline(&participants, &persistent, occurrence, meter)
            });
            let (witness_charge, witness) = charge_and_result(|meter| {
                witness_comm_pipeline(&participants, &persistent, occurrence, meter)
            });
            prop_assert_eq!(&witness, &legacy);
            for dimension in 0..3 {
                prop_assert!(
                    witness_charge[dimension] <= legacy_charge[dimension],
                    "dimension {}: witness {:?} legacy {:?}",
                    dimension,
                    witness_charge,
                    legacy_charge
                );
            }
            let legacy_limited = result_within(limit, |meter| {
                legacy_comm_pipeline(&participants, &persistent, occurrence, meter)
            });
            if !matches!(legacy_limited, Err(AuthorityError::HostWorkRejected)) {
                let witness_limited = result_within(limit, |meter| {
                    witness_comm_pipeline(&participants, &persistent, occurrence, meter)
                });
                prop_assert_eq!(witness_limited, legacy_limited);
            }
        }
    }

    /// D-F1 (DR-117): the in-place merge allocates exactly what the
    /// canonicalization of the concatenated regions allocates. The copying
    /// merge allocated the copies of the regions as well.
    #[test]
    fn merge_from_borrowed_regions_copies_no_intermediate_region() {
        let participants: Vec<CostAuthority> = (1_u8..4)
            .map(|identity| CostAuthority {
                regions: vec![CostRegion {
                    instance_id: vec![identity; 32],
                    signature: Some(quoted_chain(usize::from(identity))),
                }],
            })
            .collect();
        let flat = CostAuthority {
            regions: participants
                .iter()
                .flat_map(|authority| authority.regions.iter().cloned())
                .collect(),
        };
        let unlimited = |_: usize, _: usize, _: usize| Ok(());
        let (merged, merge_bytes) =
            crate::rust::interpreter::accounting::measured_allocations(|| {
                merge_canonical_metered(&participants, &unlimited)
            });
        let (canonical, canonical_bytes) =
            crate::rust::interpreter::accounting::measured_allocations(|| {
                canonical_authority_metered(&flat, &unlimited)
            });
        let canonical = canonical.expect("canonical");
        assert_eq!(merged.expect("merge").into_authority(), canonical);
        assert_eq!(merge_bytes, canonical_bytes);
        let (legacy, legacy_bytes) =
            crate::rust::interpreter::accounting::measured_allocations(|| {
                merge_authorities_metered_legacy(&participants, &unlimited)
            });
        assert_eq!(legacy.expect("legacy merge"), canonical);
        assert!(
            legacy_bytes > merge_bytes,
            "the copying merge allocated {legacy_bytes} bytes, the in-place merge {merge_bytes}"
        );
    }

    /// D-F1 (DR-117), negative control: the legacy pipeline validates the
    /// signature of a region at each of its five canonicalizations (the
    /// merge, both canonicalizations of the instantiation, the observation
    /// and the demand). The witness pipeline validates it once.
    #[test]
    fn legacy_pipeline_recanonicalized_each_stage() {
        let signature = quoted_chain(3);
        let pattern = inspection_log(&signature);
        let participants = vec![CostAuthority {
            regions: vec![CostRegion {
                instance_id: vec![1; 32],
                signature: Some(signature),
            }],
        }];
        let persistent = BTreeSet::new();
        let legacy = reservation_log(|meter| {
            legacy_comm_pipeline(&participants, &persistent, [7; 32], meter).map(drop)
        });
        let witness = reservation_log(|meter| {
            witness_comm_pipeline(&participants, &persistent, [7; 32], meter).map(drop)
        });
        assert_eq!(runs(&legacy, &pattern), vec![3, 3, 3, 3, 3]);
        assert_eq!(runs(&witness, &pattern), vec![3]);
    }

    /// D-F1 (DR-117): a rekey that maps two regions to one new identity keeps
    /// the conflict check. Equal signatures merge into one region, and
    /// different signatures conflict.
    #[test]
    fn rekey_keeps_the_conflict_check_of_new_identities() {
        let unlimited = |_: usize, _: usize, _: usize| Ok(());
        let region = |identity: u8, signature: CostSignature| CostRegion {
            instance_id: vec![identity; 32],
            signature: Some(signature),
        };
        let collide = |_: Vec<u8>, _: &SorterMeter<'_>| Ok(vec![9; 32]);
        let different = canonical_regions(&[region(1, ground(&[1])), region(2, ground(&[2]))])
            .expect("canonical");
        assert!(matches!(
            rekey_by(different, &unlimited, collide),
            Err(AuthorityError::RegionIdentityConflict)
        ));
        let equal = canonical_regions(&[region(1, ground(&[1])), region(2, ground(&[1]))])
            .expect("canonical");
        let merged = rekey_by(equal, &unlimited, collide).expect("equal signatures merge");
        assert_eq!(merged.as_authority().regions, vec![region(9, ground(&[1]))]);
    }

    /// D-E3 (DR-110): every reservation that `charge` makes, in order.
    fn reservation_log(
        charge: impl FnOnce(&dyn BackingMeter) -> Result<(), AuthorityError>,
    ) -> Vec<[usize; 3]> {
        let log = std::cell::RefCell::new(Vec::with_capacity(4_096));
        let meter = |operations: usize, scanned: usize, backing: usize| {
            log.borrow_mut().push([operations, scanned, backing]);
            Ok(())
        };
        charge(&meter).expect("charge");
        log.into_inner()
    }

    /// D-E3 (DR-110): every reservation of one block inspection of `value`.
    fn inspection_log<T: shared::rust::clone_backing::CloneBacking>(value: &T) -> Vec<[usize; 3]> {
        reservation_log(|meter| {
            shared::rust::clone_backing::inspect_blocks(value, meter)
                .map_err(authority_backing_error)
        })
    }

    /// D-E3 (DR-110): the lengths of the maximal runs of consecutive copies
    /// of `pattern` in `log`, in order.
    fn runs(log: &[[usize; 3]], pattern: &[[usize; 3]]) -> Vec<usize> {
        let mut lengths = Vec::new();
        let mut index = 0;
        while index + pattern.len() <= log.len() {
            if log[index..].starts_with(pattern) {
                let mut length = 0;
                while log[index..].starts_with(pattern) {
                    length += 1;
                    index += pattern.len();
                }
                lengths.push(length);
            } else {
                index += 1;
            }
        }
        lengths
    }

    fn total(log: &[[usize; 3]]) -> [usize; 3] {
        log.iter()
            .fold([0; 3], |[o, s, b], [operations, scanned, backing]| {
                [o + operations, s + scanned, b + backing]
            })
    }

    /// D-E3 (DR-110), Rule B: `canonical_cost_signature_metered` inspects the
    /// canonical signature twice (the comparison and its release) and the
    /// input once. For a canonical input the three inspections are equal
    /// runs. The comparison of a repeated region then inspects the canonical
    /// signature once more, because its release was prepaid at its birth.
    #[test]
    fn canonical_signature_inspected_twice_and_parts_once() {
        let signature = quoted_chain(3);
        let pattern = inspection_log(&signature);
        let log =
            reservation_log(|meter| canonical_cost_signature_metered(&signature, meter).map(drop));
        assert_eq!(runs(&log, &pattern), vec![3]);
        let region = CostRegion {
            instance_id: vec![1; 32],
            signature: Some(signature.clone()),
        };
        let authority = CostAuthority {
            regions: vec![region.clone(), region],
        };
        let log = reservation_log(|meter| canonical_authority_metered(&authority, meter).map(drop));
        assert_eq!(runs(&log, &pattern), vec![3, 3, 2]);
    }

    /// D-E3 (DR-110): the validation of a quoted part inspects the part once
    /// (DR-94) and the owned sorted part twice, for the comparison and for
    /// its release. For a canonical part the three inspections are equal
    /// runs.
    #[test]
    fn validated_quoted_part_charges_comparison_and_release() {
        let par = send_chain(3);
        let signature = CostSignature {
            value: Some(CostSignatureValue::Quote(par.clone())),
        };
        let log = reservation_log(|meter| {
            validate_cost_signature_metered(&signature, &SorterMeter::new(meter))
        });
        assert_eq!(runs(&log, &inspection_log(&par)), vec![3]);
    }

    /// D-E3 (DR-110): the conversion of a canonical quoted atom charges
    /// exactly its header, one block inspection of the quoted Par (the length
    /// computation), the key bytes and the nested encode (DR-93). The release
    /// of the part was prepaid at the birth of its canonical signature.
    #[test]
    fn quoted_atom_conversion_inspects_its_part_once() {
        let par = send_chain(4);
        let atom = CostSignature {
            value: Some(CostSignatureValue::Quote(par.clone())),
        };
        let log = reservation_log(|meter| {
            cost_atom_to_sig_metered(atom.clone(), &SorterMeter::new(meter)).map(drop)
        });
        let encoded_len = par.encoded_len();
        let encode = reservation_log(|meter| {
            shared::rust::clone_backing::reserve_nested_encode(&par, encoded_len, meter)
                .map_err(authority_backing_error)
        });
        let expected = total(
            &[
                vec![[1, std::mem::size_of::<CostSignature>(), 0]],
                inspection_log(&par),
                vec![[encoded_len, 0, encoded_len]],
                encode,
            ]
            .concat(),
        );
        assert_eq!(total(&log), expected);
    }

    /// D-E3 (DR-110): the lane of a signature inspects its sorted channel three
    /// times: the length computation, the release of the sorted channel and
    /// the release of the shadowed unsorted channel. The funding channel
    /// inspects its sorted channel once, for the release of the unsorted
    /// channel.
    #[test]
    fn lane_channel_inspected_three_times_and_funding_channel_once() {
        let ground = |byte: u8| CostSignature {
            value: Some(CostSignatureValue::Ground(vec![byte; 32])),
        };
        let signature = CostSignature {
            value: Some(CostSignatureValue::Compound(CostSignatureCompound {
                elements: vec![ground(9), ground(3)],
            })),
        };
        let unlimited = |_: usize, _: usize, _: usize| Ok(());
        let mut channel = Par::default();
        for byte in [9, 3] {
            append_signature_channel_atom_metered(
                &[byte; 32],
                &mut channel,
                &SorterMeter::new(&unlimited),
            )
            .expect("an unlimited append");
        }
        let sorted = ParSortMatcher::sort_match_metered(&channel, &SorterMeter::new(&unlimited))
            .expect("an unlimited sort")
            .term;
        let log = reservation_log(|meter| {
            cost_signature_lane_metered(&signature, &SorterMeter::new(meter)).map(drop)
        });
        assert_eq!(runs(&log, &inspection_log(&sorted)), vec![3]);
        let funding = Sig::And(
            Box::new(Sig::Ground(vec![9; 32])),
            Box::new(Sig::Ground(vec![3; 32])),
        );
        let log = reservation_log(|meter| funding_sig_channel_metered(&funding, meter).map(drop));
        assert_eq!(runs(&log, &inspection_log(&sorted)), vec![1]);
    }
}
