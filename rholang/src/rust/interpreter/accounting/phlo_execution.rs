use std::collections::BTreeMap;

use models::rust::host_work::{HostWorkDimension, HostWorkReservationError, HostWorkUnits};
use models::rust::phlo_resource::{PhloAuthorityNode as AuthorityNode, PhloResourceKeyV1};
use thiserror::Error;

use super::phlo_controls::CheckedPhloControls;
use super::Sig;
use crate::rust::interpreter::host_work::HostWorkBudget;

mod obligations;
mod funding;
mod family_policy;
pub use family_policy::{
    select_phlo_funding_family, verify_phlo_funding_family, PhloFamilyFundingError,
    PhloFamilyFundingInput, PhloFamilyFundingLimits, PhloFamilyFundingProposal,
    PhloFamilyFundingSelection, PhloFundingRequirement,
};
mod consent;
mod intent;
mod capture;
mod policy_capture;
mod native_policy;
mod outcome_match;
pub use capture::{
    CanonicalPhloFundingCapture, CapturedPhloSource, PhloCaptureError, PhloCaptureLimits,
};
pub use consent::{
    check_phlo_family_consent, check_phlo_family_wire_consent, CheckedPhloBoundFamilyConsent,
    CheckedPhloFamilyConsent, CheckedPhloFamilyWireConsent, DecodedPhloFamilyIntent,
    DecodedPhloFamilyWireIntent, PhloConsentError, PhloConsentLimits, PhloSourceConsent,
};
pub use funding::{
    check_phlo_funding_family, CheckedPhloFundingFamily, NativePhloAmountError,
    NativePhloSourceAmounts, PhloFundingCase, PhloFundingError, PhloFundingLimits,
    PhloFundingSource, PhloRefund,
};
pub use intent::{
    CheckedPhloFundingIntent, CheckedSignedPhloFundingIntent, PhloFundingIntentBinding,
    PhloFundingIntentCheckError, PhloFundingIntentView, SignedPhloConsentError,
    SignedPhloConsentLimits,
};
pub use native_policy::{
    CheckedNativeSignedPhloFamilyPolicy, NativePhloPolicyError, NativeScopedPhloFundingCapture,
};
pub use obligations::{
    project_phlo_obligations, CheckedPhloObligations, PhloObligationError, PhloObligationFunding,
    PhloObligationFundingError, PhloObligationFundingInput, PhloObligationFundingLimits,
    PhloObligationFundingProposal, PhloObligationKey,
};
pub use outcome_match::{PhloOutcomeMatchError, PhloOutcomeMatchLimits};
pub use policy_capture::{
    CheckedSignedPhloFamilyPolicy, PhloFamilyCursorSnapshot, PhloPolicyCaptureError,
    PhloScopedCursorSnapshot, ScopedPhloFundingCapture,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloResource<'a> {
    pub location: &'a [u8],
    pub class: usize,
    pub acquisition_terms: &'a [u8],
    pub authority: &'a Sig,
}

impl<'a> PhloResource<'a> {
    pub fn wire_key(
        self,
        limits: PhloExecutionLimits,
    ) -> Result<PhloResourceKeyV1<'a>, PhloExecutionError> {
        if limits.resource_entries == 0 {
            return Err(PhloExecutionError::TooManyResourceEntries);
        }
        let key = resource_key(self, &mut WorkBudget {
            remaining_nodes: limits.authority_nodes,
            remaining_bytes: limits.key_bytes,
        })?;
        key.into_wire()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloExecutionWitness<'a> {
    pub available: &'a [PhloResource<'a>],
    pub required: &'a [PhloResource<'a>],
    pub used: &'a [PhloResource<'a>],
    pub unused: &'a [PhloResource<'a>],
    pub fresh: &'a [PhloResource<'a>],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloResourceAmount<'a> {
    pub resource: PhloResource<'a>,
    pub quantity: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountedPhloExecutionWitness<'a> {
    pub available: &'a [PhloResourceAmount<'a>],
    pub required: &'a [PhloResourceAmount<'a>],
    pub used: &'a [PhloResourceAmount<'a>],
    pub unused: &'a [PhloResourceAmount<'a>],
    pub fresh: &'a [PhloResourceAmount<'a>],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhloExecutionResources<'a> {
    Occurrences(PhloExecutionWitness<'a>),
    Counted(CountedPhloExecutionWitness<'a>),
}

#[derive(Clone, Copy)]
struct ResourceEntries<'a> {
    occurrences: &'a [PhloResource<'a>],
    counted: &'a [PhloResourceAmount<'a>],
}

impl<'a> ResourceEntries<'a> {
    fn len(self) -> usize { self.occurrences.len() + self.counted.len() }

    fn iter(self) -> impl Iterator<Item = PhloResourceAmount<'a>> {
        self.occurrences
            .iter()
            .map(|resource| PhloResourceAmount {
                resource: *resource,
                quantity: 1,
            })
            .chain(self.counted.iter().copied())
    }
}

impl<'a> PhloExecutionResources<'a> {
    pub fn occurrences(self) -> Option<PhloExecutionWitness<'a>> {
        match self {
            Self::Occurrences(witness) => Some(witness),
            Self::Counted(_) => None,
        }
    }

    fn parts(self) -> [ResourceEntries<'a>; 5] {
        match self {
            Self::Occurrences(w) => {
                [w.available, w.required, w.used, w.unused, w.fresh].map(|occurrences| {
                    ResourceEntries {
                        occurrences,
                        counted: &[],
                    }
                })
            }
            Self::Counted(w) => {
                [w.available, w.required, w.used, w.unused, w.fresh].map(|counted| {
                    ResourceEntries {
                        occurrences: &[],
                        counted,
                    }
                })
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloExecutionLimits {
    pub resource_entries: usize,
    pub authority_nodes: usize,
    pub key_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckedPhloExecution<'a> {
    controls: CheckedPhloControls<'a>,
    witness: PhloExecutionResources<'a>,
    usage: u64,
    prepaid_usage: u64,
    fresh_usage: u64,
    acquisition_charge: u64,
    limits: PhloExecutionLimits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhloFailure {
    User,
    Platform,
    Certificate,
    Unclassified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhloOutcome<'a> {
    AdmissionRejected,
    Accepted(&'a [PhloFailure]),
}

impl<'a> CheckedPhloExecution<'a> {
    pub fn controls(self) -> CheckedPhloControls<'a> { self.controls }

    pub fn witness(self) -> PhloExecutionResources<'a> { self.witness }

    pub fn usage(self) -> u64 { self.usage }

    pub fn prepaid_usage(self) -> u64 { self.prepaid_usage }

    pub fn fresh_usage(self) -> u64 { self.fresh_usage }

    pub fn retained_charge(self, outcome: PhloOutcome<'_>) -> u64 {
        match outcome {
            PhloOutcome::Accepted(failures)
                if failures.iter().all(|failure| *failure == PhloFailure::User) =>
            {
                self.acquisition_charge
            }
            _ => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloExecutionError {
    #[error(transparent)]
    HostWork(#[from] HostWorkReservationError),
    #[error("phlo resource allocation failed")]
    AllocationFailed,
    #[error("phlo resource quantity must be positive")]
    ZeroQuantity,
    #[error("phlo execution witness exceeds the resource-entry limit")]
    TooManyResourceEntries,
    #[error("phlo execution witness exceeds the authority-node limit")]
    TooManyAuthorityNodes,
    #[error("phlo execution witness exceeds the key-byte limit")]
    TooManyKeyBytes,
    #[error("phlo resource contains an unsupported funding authority")]
    UnsupportedFundingAuthority,
    #[error("phlo resource class is absent from the selected schedule")]
    UnknownResourceClass,
    #[error("weighted resource use exceeds the checked execution bound")]
    UsageExceeded,
    #[error("prepaid supply does not equal used and unused resource occurrences")]
    SupplyPartitionMismatch,
    #[error("required resources do not equal used and fresh resource occurrences")]
    DemandPartitionMismatch,
    #[error("fresh acquisition bypasses compatible unused prepaid resources")]
    UnusedCompatiblePrepaid,
    #[error("phlo resource arithmetic overflow")]
    ArithmeticOverflow,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ResourceKey<'a> {
    location: &'a [u8],
    class: usize,
    acquisition_terms: &'a [u8],
    authority: Vec<AuthorityNode<'a>>,
}

type ResourceCounts<'a> = BTreeMap<ResourceKey<'a>, u64>;

impl<'a> ResourceKey<'a> {
    fn into_wire(self) -> Result<PhloResourceKeyV1<'a>, PhloExecutionError> {
        Ok(PhloResourceKeyV1 {
            location: self.location,
            class: u32::try_from(self.class)
                .map_err(|_| PhloExecutionError::UnknownResourceClass)?,
            acquisition_terms: self.acquisition_terms,
            authority: self.authority,
        })
    }
}

struct WorkBudget {
    remaining_nodes: usize,
    remaining_bytes: usize,
}

impl WorkBudget {
    fn bytes(&mut self, amount: usize) -> Result<(), PhloExecutionError> {
        self.remaining_bytes = self
            .remaining_bytes
            .checked_sub(amount)
            .ok_or(PhloExecutionError::TooManyKeyBytes)?;
        Ok(())
    }
}

fn resource_key<'a>(
    resource: PhloResource<'a>,
    budget: &mut WorkBudget,
) -> Result<ResourceKey<'a>, PhloExecutionError> {
    resource_key_with_host_work(resource, budget, None)
}

fn reserve_key_work(
    budget: Option<&HostWorkBudget>,
    dimension: HostWorkDimension,
    amount: usize,
) -> Result<(), PhloExecutionError> {
    if let Some(budget) = budget {
        budget.reserve(
            dimension,
            HostWorkUnits::new(
                u64::try_from(amount).map_err(|_| PhloExecutionError::ArithmeticOverflow)?,
            ),
        )?;
    }
    Ok(())
}

fn push_key_entry<T>(
    output: &mut Vec<T>,
    reserved: &mut usize,
    value: T,
    host: Option<&HostWorkBudget>,
) -> Result<(), PhloExecutionError> {
    let length = output.len();
    if length == *reserved {
        let capacity = if *reserved == 0 {
            1
        } else {
            reserved
                .checked_mul(2)
                .ok_or(PhloExecutionError::ArithmeticOverflow)?
        };
        let bytes = capacity
            .checked_mul(std::mem::size_of::<T>())
            .ok_or(PhloExecutionError::ArithmeticOverflow)?;
        reserve_key_work(host, HostWorkDimension::SearchStateBytes, bytes)?;
        reserve_key_work(host, HostWorkDimension::VerificationOperations, length)?;
        output
            .try_reserve_exact(capacity - length)
            .map_err(|_| PhloExecutionError::AllocationFailed)?;
        *reserved = capacity;
    }
    reserve_key_work(host, HostWorkDimension::VerificationOperations, 1)?;
    output.push(value);
    Ok(())
}

fn resource_key_with_host_work<'a>(
    resource: PhloResource<'a>,
    budget: &mut WorkBudget,
    host: Option<&HostWorkBudget>,
) -> Result<ResourceKey<'a>, PhloExecutionError> {
    reserve_key_work(host, HostWorkDimension::VerificationOperations, 3)?;
    budget.bytes(resource.location.len())?;
    budget.bytes(resource.acquisition_terms.len())?;
    let mut pending = Vec::new();
    let mut pending_reserved = 0;
    push_key_entry(
        &mut pending,
        &mut pending_reserved,
        resource.authority,
        host,
    )?;
    let mut authority = Vec::new();
    let mut authority_reserved = 0;
    while let Some(current) = pending.pop() {
        reserve_key_work(host, HostWorkDimension::VerificationOperations, 1)?;
        budget.remaining_nodes = budget
            .remaining_nodes
            .checked_sub(1)
            .ok_or(PhloExecutionError::TooManyAuthorityNodes)?;
        match current {
            Sig::Unit => push_key_entry(
                &mut authority,
                &mut authority_reserved,
                AuthorityNode::Unit,
                host,
            )?,
            Sig::Ground(bytes) => {
                budget.bytes(bytes.len())?;
                push_key_entry(
                    &mut authority,
                    &mut authority_reserved,
                    AuthorityNode::Ground(bytes),
                    host,
                )?;
            }
            Sig::Quote(bytes) => {
                budget.bytes(bytes.len())?;
                push_key_entry(
                    &mut authority,
                    &mut authority_reserved,
                    AuthorityNode::Quote(bytes),
                    host,
                )?;
            }
            Sig::And(left, right) => {
                push_key_entry(
                    &mut authority,
                    &mut authority_reserved,
                    AuthorityNode::And,
                    host,
                )?;
                push_key_entry(&mut pending, &mut pending_reserved, right.as_ref(), host)?;
                push_key_entry(&mut pending, &mut pending_reserved, left.as_ref(), host)?;
            }
            _ => return Err(PhloExecutionError::UnsupportedFundingAuthority),
        }
    }
    Ok(ResourceKey {
        location: resource.location,
        class: resource.class,
        acquisition_terms: resource.acquisition_terms,
        authority,
    })
}

fn count_resources<'a>(
    resources: ResourceEntries<'a>,
    budget: &mut WorkBudget,
) -> Result<ResourceCounts<'a>, PhloExecutionError> {
    let mut counts = ResourceCounts::new();
    for amount in resources.iter() {
        if amount.quantity == 0 {
            return Err(PhloExecutionError::ZeroQuantity);
        }
        let count = counts
            .entry(resource_key(amount.resource, budget)?)
            .or_default();
        *count = count
            .checked_add(amount.quantity)
            .ok_or(PhloExecutionError::ArithmeticOverflow)?;
    }
    Ok(counts)
}

fn is_partition(
    whole: &ResourceCounts<'_>,
    left: &ResourceCounts<'_>,
    right: &ResourceCounts<'_>,
) -> bool {
    whole.iter().all(|(key, amount)| {
        u128::from(*amount)
            == u128::from(left.get(key).copied().unwrap_or(0))
                + u128::from(right.get(key).copied().unwrap_or(0))
    }) && left
        .keys()
        .chain(right.keys())
        .all(|key| whole.contains_key(key))
}

fn resource_weight(key: &ResourceKey<'_>, weights: &[u64]) -> Result<u64, PhloExecutionError> {
    let weight = weights
        .get(key.class)
        .ok_or(PhloExecutionError::UnknownResourceClass)?;
    let units = u64::try_from(
        key.authority
            .iter()
            .filter(|node| matches!(node, AuthorityNode::Ground(_) | AuthorityNode::Quote(_)))
            .count(),
    )
    .map_err(|_| PhloExecutionError::ArithmeticOverflow)?;
    weight
        .checked_mul(units)
        .ok_or(PhloExecutionError::ArithmeticOverflow)
}

fn weighted_usage(
    resources: &ResourceCounts<'_>,
    weights: &[u64],
) -> Result<u64, PhloExecutionError> {
    resources.iter().try_fold(0_u64, |total, (key, count)| {
        resource_weight(key, weights)?
            .checked_mul(*count)
            .and_then(|value| value.checked_add(total))
            .ok_or(PhloExecutionError::ArithmeticOverflow)
    })
}

pub fn check_phlo_execution<'a>(
    controls: CheckedPhloControls<'a>,
    witness: PhloExecutionWitness<'a>,
    limits: PhloExecutionLimits,
) -> Result<CheckedPhloExecution<'a>, PhloExecutionError> {
    check_execution_resources(
        controls,
        PhloExecutionResources::Occurrences(witness),
        limits,
    )
}

pub fn check_counted_phlo_execution<'a>(
    controls: CheckedPhloControls<'a>,
    witness: CountedPhloExecutionWitness<'a>,
    limits: PhloExecutionLimits,
) -> Result<CheckedPhloExecution<'a>, PhloExecutionError> {
    check_execution_resources(controls, PhloExecutionResources::Counted(witness), limits)
}

fn check_execution_resources<'a>(
    controls: CheckedPhloControls<'a>,
    witness: PhloExecutionResources<'a>,
    limits: PhloExecutionLimits,
) -> Result<CheckedPhloExecution<'a>, PhloExecutionError> {
    let parts = witness.parts();
    let mut remaining = limits.resource_entries;
    for part in parts {
        remaining = remaining
            .checked_sub(part.len())
            .ok_or(PhloExecutionError::TooManyResourceEntries)?;
    }
    let mut budget = WorkBudget {
        remaining_nodes: limits.authority_nodes,
        remaining_bytes: limits.key_bytes,
    };
    let available = count_resources(parts[0], &mut budget)?;
    let required = count_resources(parts[1], &mut budget)?;
    let used = count_resources(parts[2], &mut budget)?;
    let unused = count_resources(parts[3], &mut budget)?;
    let fresh = count_resources(parts[4], &mut budget)?;
    let usage = weighted_usage(&required, controls.schedule().weights)?;
    if usage > controls.resource_bound() {
        return Err(PhloExecutionError::UsageExceeded);
    }
    if !is_partition(&available, &used, &unused) {
        return Err(PhloExecutionError::SupplyPartitionMismatch);
    }
    if !is_partition(&required, &used, &fresh) {
        return Err(PhloExecutionError::DemandPartitionMismatch);
    }
    if unused.keys().any(|key| fresh.contains_key(key)) {
        return Err(PhloExecutionError::UnusedCompatiblePrepaid);
    }
    let prepaid_usage = weighted_usage(&used, controls.schedule().weights)?;
    let fresh_usage = weighted_usage(&fresh, controls.schedule().weights)?;
    let acquisition_charge = fresh_usage
        .checked_mul(controls.schedule().actual_price)
        .and_then(|charge| charge.checked_add(1))
        .ok_or(PhloExecutionError::ArithmeticOverflow)?;
    Ok(CheckedPhloExecution {
        controls,
        witness,
        usage,
        prepaid_usage,
        fresh_usage,
        acquisition_charge,
        limits,
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod wire_tests;
