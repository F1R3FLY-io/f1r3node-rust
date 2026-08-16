use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crypto::rust::hash::blake2b256::Blake2b256;
use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::{CostAuthority, CostRegion, CostSignature, CostSignatureCompound};
use models::rust::rholang::sorter::cost_accounting_sorter::sort_signature;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use prost::Message;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::Sig;

const CERTIFICATE_DOMAIN: &[u8] = b"f1r3node:authority-funding-certificate:v7";
const WITNESS_DOMAIN: &[u8] = b"f1r3node:authority-cost-witness:v7";
pub const AUTHORITY_ACCOUNTING_PROTOCOL_VERSION: u32 = 7;
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

pub fn canonical_authority(authority: &CostAuthority) -> Result<CostAuthority, AuthorityError> {
    let mut regions = BTreeMap::<Vec<u8>, CostSignature>::new();
    for region in &authority.regions {
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
    Ok(CostAuthority {
        regions: regions
            .into_iter()
            .map(|(instance_id, signature)| CostRegion {
                instance_id,
                signature: Some(signature),
            })
            .collect(),
    })
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
        atoms
            .iter()
            .try_fold(ResourceMultiset::default(), |allocation, atom| {
                allocation.checked_add(&ResourceMultiset::singleton(
                    cost_signature_to_sig(atom)?.lane_hash(),
                    1,
                ))
            })?,
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
    for presentation in presentations {
        insert(&mut signatures, presentation.clone())?;
    }
    Ok(signatures)
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

pub fn allocate_authority_event_draws(
    events: &[AuthorityEvent<[u8; 32]>],
    available: &ResourceMultiset<[u8; 32]>,
) -> Result<Vec<AuthorityPhysicalEventDraw>, AuthorityError> {
    let options = events
        .iter()
        .map(authority_funding_options)
        .collect::<Result<Vec<_>, _>>()?;
    let mut failed = BTreeSet::new();
    allocate_event_options(&options, 0, available, &mut failed)
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
    index: usize,
    available: &ResourceMultiset<[u8; 32]>,
    failed: &mut BTreeSet<(usize, Vec<([u8; 32], u64)>)>,
) -> Option<(ResourceMultiset<[u8; 32]>, Vec<ResourceMultiset<[u8; 32]>>)> {
    if index == options.len() {
        return Some((available.clone(), Vec::new()));
    }
    let state = (
        index,
        available
            .0
            .iter()
            .map(|(key, amount)| (*key, *amount))
            .collect(),
    );
    if failed.contains(&state) {
        return None;
    }
    for draw in &options[index] {
        let Ok(remaining) = available.checked_sub(draw) else {
            continue;
        };
        if let Some((final_remaining, mut rest)) =
            allocate_event_options(options, index + 1, &remaining, failed)
        {
            rest.insert(0, draw.clone());
            return Some((final_remaining, rest));
        }
    }
    failed.insert(state);
    None
}

pub fn instantiate_persistent_regions(
    authority: &CostAuthority,
    persistent_regions: &BTreeSet<[u8; 32]>,
    occurrence: [u8; 32],
) -> Result<CostAuthority, AuthorityError> {
    let regions = authority_regions(authority)?;
    canonical_authority(&CostAuthority {
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
    })
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
    pub stack_pops: BTreeMap<[u8; 32], u64>,
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
        remaining = remaining.checked_sub(&draw.balances)?;
        balance_debit = balance_debit.checked_add(&draw.balances)?;

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
        stack_pops,
    })
}

#[derive(Clone)]
enum PhysicalCandidate {
    Balance([u8; 32]),
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

#[allow(clippy::too_many_arguments)]
fn search_physical_settlement(
    events: &[AuthorityEvent<[u8; 32]>],
    expected: &[ResourceMultiset<[u8; 32]>],
    balance_atoms: &BTreeMap<[u8; 32], ResourceMultiset<[u8; 32]>>,
    stack_atoms: &BTreeMap<[u8; 32], Vec<ResourceMultiset<[u8; 32]>>>,
    event_index: usize,
    event_remaining: ResourceMultiset<[u8; 32]>,
    balances: ResourceMultiset<[u8; 32]>,
    stack_positions: BTreeMap<[u8; 32], usize>,
    event_balances: ResourceMultiset<[u8; 32]>,
    event_stacks: BTreeSet<[u8; 32]>,
    born_available_after: &BTreeMap<[u8; 32], usize>,
    failed: &mut BTreeSet<PhysicalSearchState>,
) -> Option<Vec<AuthorityPhysicalEventDraw<[u8; 32]>>> {
    let mut work = vec![PhysicalSearchWork::Search(PhysicalSearchNode {
        event_index,
        event_remaining,
        balances,
        stack_positions,
        event_balances,
        event_stacks,
        draws: None,
    })];

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
            let mut draws = Vec::with_capacity(events.len());
            let mut link = node.draws;
            while let Some(current) = link {
                draws.push(current.draw.clone());
                link = current.previous.clone();
            }
            draws.reverse();
            return Some(draws);
        }

        if node.event_remaining.0.is_empty() {
            let draw = AuthorityPhysicalEventDraw {
                event_id: events[node.event_index].event_id,
                balances: std::mem::take(&mut node.event_balances),
                stack_ids: std::mem::take(&mut node.event_stacks).into_iter().collect(),
            };
            node.event_index += 1;
            node.event_remaining = expected.get(node.event_index).cloned().unwrap_or_default();
            node.draws = Some(Arc::new(PhysicalDrawLink {
                previous: node.draws,
                draw,
            }));
            work.push(PhysicalSearchWork::Search(node));
            continue;
        }

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

        let pivot = *node.event_remaining.0.keys().next()?;
        let mut candidates =
            Vec::<(std::cmp::Reverse<u64>, u8, [u8; 32], PhysicalCandidate)>::new();
        for (key, amount) in &node.balances.0 {
            if *amount == 0 {
                continue;
            }
            let atoms = balance_atoms.get(key)?;
            if atoms.get(&pivot) > 0 && node.event_remaining.dominates(atoms) {
                candidates.push((
                    std::cmp::Reverse(atoms.0.values().copied().sum()),
                    1,
                    *key,
                    PhysicalCandidate::Balance(*key),
                ));
            }
        }
        for (stack_id, cells) in stack_atoms {
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
            let mut next = node.clone();
            let atoms = match candidate {
                PhysicalCandidate::Balance(key) => {
                    next.balances = next
                        .balances
                        .checked_sub(&ResourceMultiset::singleton(key, 1))
                        .ok()?;
                    next.event_balances = next
                        .event_balances
                        .checked_add(&ResourceMultiset::singleton(key, 1))
                        .ok()?;
                    balance_atoms.get(&key)?.clone()
                }
                PhysicalCandidate::Stack(stack_id) => {
                    let position = next.stack_positions.entry(stack_id).or_default();
                    let atoms = stack_atoms.get(&stack_id)?.get(*position)?.clone();
                    *position += 1;
                    next.event_stacks.insert(stack_id);
                    atoms
                }
            };
            next.event_remaining = next.event_remaining.checked_sub(&atoms).ok()?;
            work.push(PhysicalSearchWork::Search(next));
        }
    }

    None
}

pub fn allocate_physical_settlement(
    events: &[AuthorityEvent<[u8; 32]>],
    signatures: &BTreeMap<[u8; 32], CostSignature>,
    inventory: &AuthorityPhysicalInventory<[u8; 32]>,
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
    for key in inventory.balances.0.keys() {
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
        &stack_atoms,
        0,
        expected.first().cloned().unwrap_or_default(),
        inventory.balances.clone(),
        BTreeMap::new(),
        ResourceMultiset::default(),
        BTreeSet::new(),
        &born_available_after,
        &mut BTreeSet::new(),
    )
    .ok_or(AuthorityError::InsufficientAuthority)?;
    verify_physical_settlement(events, signatures, inventory, &draws)
}

pub fn apply_physical_settlement(
    inventory: &mut AuthorityPhysicalInventory<[u8; 32]>,
    settlement: &AuthorityPhysicalSettlement<[u8; 32]>,
) -> Result<(), AuthorityError> {
    inventory.balances = inventory.balances.checked_sub(&settlement.balance_debit)?;
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
    #[serde(default)]
    pub fee_recipient: Vec<u8>,
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
        if !available.dominates(&self.allocation) {
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
        bytes.extend_from_slice(&(self.fee_recipient.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&self.fee_recipient);
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
        certificate.allocation.checked_sub(&self.settlement)
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
        Blake2b256::hash(bytes)
            .try_into()
            .expect("Blake2b-256 digest length")
    }
}

impl AuthorityCostWitness<[u8; 32]> {
    pub fn verify_event_authorities(&self) -> Result<(), AuthorityError> {
        self.events
            .iter()
            .try_for_each(AuthorityEvent::verify_authority)
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum AuthorityError {
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
    #[error("physical settlement references an unknown stack resource")]
    UnknownStackResource,
    #[error("physical settlement attempts to pop an exhausted stack resource")]
    ExhaustedStackResource,
    #[error("physical settlement does not exactly realize the event authority")]
    PhysicalAuthorityMismatch,
}

#[cfg(test)]
mod tests {
    use models::rhoapi::cost_signature::Value as CostSignatureValue;
    use models::rhoapi::{CostAuthority, CostRegion, CostSignature};
    use proptest::prelude::*;

    use super::*;

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
            fee_recipient: Vec::new(),
        }
    }

    #[test]
    fn funding_certificate_id_matches_python_client_golden_vector() {
        let certificate = FundingCertificate {
            protocol_version: AUTHORITY_ACCOUNTING_PROTOCOL_VERSION,
            program_hash: [b'm'; 32],
            pre_state_root: [b'p'; 32],
            reservation_id: [b'r'; 32],
            demand: DemandBound::Exact(ResourceMultiset::singleton([b's'; 32], 2)),
            allocation: ResourceMultiset::singleton([b's'; 32], 2),
            stack_reservations: BTreeMap::from([([b'k'; 32], 1)]),
            fee_allocation: ResourceMultiset::singleton([b'g'; 32], 1),
            fee_recipient: b"proposer".to_vec(),
        };

        assert_eq!(
            hex::encode(certificate.certificate_id()),
            "88ecd40f4d389fa44d6f5464bf4e704cc67f1a487894d1242bd303e0a6d02096"
        );
    }

    fn ground(bytes: &[u8]) -> CostSignature {
        CostSignature {
            value: Some(CostSignatureValue::Ground(bytes.to_vec())),
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
    fn physical_presentation_rejects_weakening_a_compound_cell() {
        let a = ground(b"a");
        let b = ground(b"b");
        let event = event(std::slice::from_ref(&a));
        let ab = compound_cost_signatures(&a, &b).unwrap();
        let ab_key = cost_signature_to_sig(&ab).unwrap().lane_hash();
        let balances = ResourceMultiset::singleton(ab_key, 1);
        let inventory = AuthorityPhysicalInventory {
            balances: balances.clone(),
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
            stacks: BTreeMap::from([(stack_id, vec![a.clone()])]),
            born_stacks: BTreeMap::from([(stack_id, produce_hash)]),
        };

        assert_eq!(
            allocate_physical_settlement(std::slice::from_ref(&transfer), &signatures, &unfunded,),
            Err(AuthorityError::InsufficientAuthority)
        );

        let funded = AuthorityPhysicalInventory {
            balances: ResourceMultiset::singleton(key, 1),
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
            fee_recipient: Vec::new(),
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
            fee_recipient: Vec::new(),
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
                physical_draws: Vec::new(),
                born_stacks: Vec::new(),
            };
            prop_assert_eq!(witness.verify(&cert).is_ok(), realized <= reserved);
        }
    }
}
