use std::collections::{BTreeMap, BTreeSet};

use models::rust::phlo_source::{PhloSourceError, PhloSourceLimits, PhloSourcePolicyV1};
use models::rust::phlo_wire::PhloWireError;
use thiserror::Error;

use super::{
    resource_key, AuthorityNode, CheckedPhloFundingFamily, PhloExecutionError, PhloExecutionLimits,
    PhloObligationKey, PhloResource, ResourceKey, WorkBudget,
};
use crate::rust::interpreter::accounting::phlo_controls::{
    check_phlo_funding_terms, CheckedPhloFundingTerms, PhloFundingTerms, PhloFundingTermsError,
    SignedPhloControls,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloSourceConsent<'a> {
    pub custody: &'a [u8],
    pub hold_cap: u64,
    pub debit_cap: u64,
    pub fee_permitted: bool,
    pub resources: &'a [PhloResource<'a>],
}

impl<'a> PhloSourceConsent<'a> {
    pub fn wire_policy(
        self,
        limits: PhloExecutionLimits,
        wire_limits: PhloSourceLimits,
    ) -> Result<PhloSourcePolicyV1<'a>, PhloConsentError> {
        if self.resources.len() > limits.resource_entries
            || self.resources.len() > wire_limits.resource_permissions
        {
            return Err(PhloConsentError::TooManyPermissions);
        }
        let mut budget = WorkBudget {
            remaining_nodes: limits.authority_nodes,
            remaining_bytes: limits.key_bytes,
        };
        budget.bytes(self.custody.len())?;
        let mut resources = Vec::new();
        for resource in self.resources {
            let key = resource_key(*resource, &mut budget)?.into_wire()?;
            resources
                .try_reserve(1)
                .map_err(|_| PhloSourceError::Wire(PhloWireError::AllocationFailed))?;
            resources.push(key);
        }
        Ok(PhloSourcePolicyV1::new(
            self.custody,
            self.hold_cap,
            self.debit_cap,
            self.fee_permitted,
            resources,
            wire_limits,
        )?)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodedPhloFamilyIntent<'a> {
    pub controls: SignedPhloControls<'a>,
    pub total_exposure: u128,
    pub sources: &'a [PhloSourceConsent<'a>],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodedPhloFamilyWireIntent<'a> {
    pub controls: SignedPhloControls<'a>,
    pub total_exposure: u128,
    pub sources: &'a [PhloSourcePolicyV1<'a>],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckedPhloFamilyWireConsent<'a> {
    family: &'a CheckedPhloFundingFamily<'a>,
    intent: DecodedPhloFamilyWireIntent<'a>,
}

impl<'a> CheckedPhloFamilyWireConsent<'a> {
    pub fn family(self) -> &'a CheckedPhloFundingFamily<'a> { self.family }
    pub fn intent(self) -> DecodedPhloFamilyWireIntent<'a> { self.intent }

    pub fn bind_funding_terms(
        self,
        terms: PhloFundingTerms<'a>,
    ) -> Result<CheckedPhloBoundFamilyConsent<'a>, PhloFundingTermsError> {
        let controls = self.family.cases()[0].obligations.execution().controls();
        let terms = check_phlo_funding_terms(controls, terms)?;
        Ok(CheckedPhloBoundFamilyConsent {
            consent: self,
            terms,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckedPhloBoundFamilyConsent<'a> {
    consent: CheckedPhloFamilyWireConsent<'a>,
    terms: CheckedPhloFundingTerms<'a>,
}

impl<'a> CheckedPhloBoundFamilyConsent<'a> {
    pub fn consent(self) -> CheckedPhloFamilyWireConsent<'a> { self.consent }
    pub fn terms(self) -> CheckedPhloFundingTerms<'a> { self.terms }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhloConsentLimits {
    pub sources: usize,
    pub permission_entries: usize,
    pub case_cells: usize,
    pub authority_nodes: usize,
    pub key_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckedPhloFamilyConsent<'a> {
    family: &'a CheckedPhloFundingFamily<'a>,
    intent: DecodedPhloFamilyIntent<'a>,
}

impl<'a> CheckedPhloFamilyConsent<'a> {
    pub fn family(self) -> &'a CheckedPhloFundingFamily<'a> { self.family }

    pub fn intent(self) -> DecodedPhloFamilyIntent<'a> { self.intent }
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloConsentError {
    #[error("phlo funding controls differ from the decoded intent")]
    DifferentControls,
    #[error("phlo funding exposure exceeds the decoded intent")]
    TotalExposureExceeded,
    #[error("phlo consent exceeds the source limit")]
    TooManySources,
    #[error("phlo consent exceeds the permission-entry limit")]
    TooManyPermissions,
    #[error("phlo consent check exceeds the case-cell limit")]
    TooManyCaseCells,
    #[error("phlo consent repeats a physical custody identity")]
    DuplicateCustody,
    #[error("phlo funding source has no decoded consent")]
    MissingSource,
    #[error("phlo funding source exposure exceeds its decoded hold cap")]
    HoldCapExceeded,
    #[error("phlo funding source debit cap exceeds its decoded debit cap")]
    DebitCapExceeded,
    #[error("phlo funding permission exceeds the decoded source consent")]
    PermissionExceeded,
    #[error(transparent)]
    Execution(#[from] PhloExecutionError),
    #[error(transparent)]
    SourceWire(#[from] PhloSourceError),
}

pub fn check_phlo_family_consent<'a>(
    family: &'a CheckedPhloFundingFamily<'a>,
    intent: DecodedPhloFamilyIntent<'a>,
    limits: PhloConsentLimits,
) -> Result<CheckedPhloFamilyConsent<'a>, PhloConsentError> {
    validate_phlo_family_consent(
        family,
        intent.controls,
        intent.total_exposure,
        intent.sources.iter().map(SourcePolicy::Native),
        limits,
    )?;
    Ok(CheckedPhloFamilyConsent { family, intent })
}

pub fn check_phlo_family_wire_consent<'a>(
    family: &'a CheckedPhloFundingFamily<'a>,
    intent: DecodedPhloFamilyWireIntent<'a>,
    limits: PhloConsentLimits,
) -> Result<CheckedPhloFamilyWireConsent<'a>, PhloConsentError> {
    validate_phlo_family_consent(
        family,
        intent.controls,
        intent.total_exposure,
        intent.sources.iter().map(SourcePolicy::Wire),
        limits,
    )?;
    Ok(CheckedPhloFamilyWireConsent { family, intent })
}

#[derive(Clone, Copy)]
enum SourcePolicy<'a> {
    Native(&'a PhloSourceConsent<'a>),
    Wire(&'a PhloSourcePolicyV1<'a>),
}

impl<'a> SourcePolicy<'a> {
    fn custody(self) -> &'a [u8] {
        match self {
            Self::Native(policy) => policy.custody,
            Self::Wire(policy) => policy.custody(),
        }
    }

    fn hold_cap(self) -> u64 {
        match self {
            Self::Native(policy) => policy.hold_cap,
            Self::Wire(policy) => policy.hold_cap(),
        }
    }

    fn debit_cap(self) -> u64 {
        match self {
            Self::Native(policy) => policy.debit_cap,
            Self::Wire(policy) => policy.debit_cap(),
        }
    }

    fn fee_permitted(self) -> bool {
        match self {
            Self::Native(policy) => policy.fee_permitted,
            Self::Wire(policy) => policy.fee_permitted(),
        }
    }

    fn resource_count(self) -> usize {
        match self {
            Self::Native(policy) => policy.resources.len(),
            Self::Wire(policy) => policy.resources().len(),
        }
    }

    fn resources(
        self,
        budget: &mut WorkBudget,
    ) -> Result<BTreeSet<ResourceKey<'a>>, PhloConsentError> {
        let mut resources = BTreeSet::new();
        match self {
            Self::Native(policy) => {
                for resource in policy.resources {
                    resources.insert(resource_key(*resource, budget)?);
                }
            }
            Self::Wire(policy) => {
                for resource in policy.resources() {
                    budget.bytes(resource.location.len())?;
                    budget.bytes(resource.acquisition_terms.len())?;
                    budget.remaining_nodes = budget
                        .remaining_nodes
                        .checked_sub(resource.authority.len())
                        .ok_or(PhloExecutionError::TooManyAuthorityNodes)?;
                    for node in &resource.authority {
                        if let AuthorityNode::Ground(bytes) | AuthorityNode::Quote(bytes) = node {
                            budget.bytes(bytes.len())?;
                        }
                    }
                    resources.insert(ResourceKey {
                        location: resource.location,
                        class: usize::try_from(resource.class)
                            .map_err(|_| PhloExecutionError::UnknownResourceClass)?,
                        acquisition_terms: resource.acquisition_terms,
                        authority: resource.authority.clone(),
                    });
                }
            }
        }
        Ok(resources)
    }
}

fn validate_phlo_family_consent<'a>(
    family: &'a CheckedPhloFundingFamily<'a>,
    controls: SignedPhloControls<'a>,
    total_exposure: u128,
    sources: impl ExactSizeIterator<Item = SourcePolicy<'a>> + Clone,
    limits: PhloConsentLimits,
) -> Result<(), PhloConsentError> {
    if sources.len() > limits.sources || family.sources().len() > limits.sources {
        return Err(PhloConsentError::TooManySources);
    }
    if family.cases()[0].obligations.execution().controls().terms() != controls {
        return Err(PhloConsentError::DifferentControls);
    }
    if family.total_exposure_limit() > total_exposure {
        return Err(PhloConsentError::TotalExposureExceeded);
    }
    let mut entries = limits.permission_entries;
    for source in sources.clone() {
        entries = entries
            .checked_sub(source.resource_count())
            .ok_or(PhloConsentError::TooManyPermissions)?;
    }
    let mut cells = limits.case_cells;
    for case in family.cases() {
        cells = family
            .sources()
            .len()
            .checked_mul(case.obligations.keys().len())
            .and_then(|count| cells.checked_sub(count))
            .ok_or(PhloConsentError::TooManyCaseCells)?;
    }
    let mut budget = WorkBudget {
        remaining_nodes: limits.authority_nodes,
        remaining_bytes: limits.key_bytes,
    };
    for source in sources.clone() {
        budget.bytes(source.custody().len())?;
    }
    for source in family.sources() {
        budget.bytes(source.custody.len())?;
    }
    let mut policies = BTreeMap::new();
    for consent in sources {
        let resources = consent.resources(&mut budget)?;
        if policies
            .insert(consent.custody(), (consent, resources))
            .is_some()
        {
            return Err(PhloConsentError::DuplicateCustody);
        }
    }
    let mut source_policies = Vec::with_capacity(family.sources().len());
    for source in family.sources() {
        let policy = policies
            .get(source.custody)
            .ok_or(PhloConsentError::MissingSource)?;
        if source.exposure_limit > policy.0.hold_cap() {
            return Err(PhloConsentError::HoldCapExceeded);
        }
        if source.debit_limit > policy.0.debit_cap() {
            return Err(PhloConsentError::DebitCapExceeded);
        }
        source_policies.push(policy);
    }
    for case in family.cases() {
        for (slot, key) in case.obligations.keys().iter().enumerate() {
            let resource = match key {
                PhloObligationKey::Fee => None,
                PhloObligationKey::Resource(resource)
                | PhloObligationKey::RetainedResource(resource) => {
                    Some(resource_key(*resource, &mut budget)?)
                }
            };
            for (source, (consent, resources)) in source_policies.iter().enumerate() {
                let permitted = resource
                    .as_ref()
                    .map_or(consent.fee_permitted(), |key| resources.contains(key));
                if case.eligible[source][slot] && !permitted {
                    return Err(PhloConsentError::PermissionExceeded);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod wire_tests;
