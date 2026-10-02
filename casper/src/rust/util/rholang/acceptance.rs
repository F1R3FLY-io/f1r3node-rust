use crypto::rust::signatures::signed::Cosigned;
use models::rust::casper::protocol::casper_message::Event;
use models::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use models::rust::phlo_intent::{
    PhloFundingIntentV2, PhloFundingIntentV2Limits, PhloFundingIntentVersioned,
};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use rholang::rust::interpreter::accounting::monetary_allocation::MonetaryCursor;
use rholang::rust::interpreter::host_work::HostWorkBudget;

use crate::rust::errors::CasperError;
use crate::rust::util::rholang::costacc::genesis_resource_policy::{
    AdoptedResourcePolicy, CompatibleAcquisitionTerms, OFFERED_PRODUCTION_READY,
};
use crate::rust::util::rholang::runtime_manager::CertifiedOfferedDraft;

const FIXED_FEE_REV: u128 = 1;

pub type MonetaryCursorRead<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<Option<MonetaryCursor>, CasperError>> + Send + 'a>,
>;

#[derive(Clone, Copy, Debug)]
pub struct OfferedCandidateLimits {
    pub funding: PhloFundingIntentV2Limits,
    pub max_phlo_limit: u64,
}

#[derive(Clone, Debug)]
pub struct PreparedOfferedCandidate<'a> {
    pub intent: PhloFundingIntentV2<'a>,
    selected_schedule_bytes: Vec<u8>,
    pub envelope_identity: [u8; 32],
    pub funding_root: [u8; 32],
    pub execution_root: [u8; 32],
    pub phlo_limit: u64,
    pub phlo_price: u64,
    pub fee_rev: u128,
    pub rev_ceiling: u128,
}

#[derive(Clone, Copy, Debug)]
pub struct CandidatePublicationPermit {
    envelope_identity: [u8; 32],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CandidateDraftSettlementPermit {
    envelope_identity: [u8; 32],
}

pub struct CandidateReplayPermit<'a> {
    processed: &'a OfferedProcessedDeploy,
    settlement_runtime_root: [u8; 32],
    user_events: usize,
    grant_events: usize,
}

impl CandidatePublicationPermit {
    pub fn envelope_identity(self) -> [u8; 32] { self.envelope_identity }
}

impl CandidateDraftSettlementPermit {
    pub fn envelope_identity(self) -> [u8; 32] { self.envelope_identity }
}

impl<'a> CandidateReplayPermit<'a> {
    pub(crate) fn authorize(
        processed: &'a OfferedProcessedDeploy,
        independently_reached_root: [u8; 32],
        user_events: usize,
        grant_events: usize,
    ) -> Result<Self, CasperError> {
        let evidence = processed
            .evidence(offered_funded_v6_limits().evidence)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        if evidence.settlement_runtime_root != independently_reached_root {
            return Err(invalid(
                "independent replay did not reach the committed settlement root",
            ));
        }
        let boundary = user_events
            .checked_add(grant_events)
            .ok_or_else(|| invalid("offered replay event boundary overflows"))?;
        if boundary > processed.deploy_log().len() {
            return Err(invalid(
                "offered replay event boundary exceeds committed log",
            ));
        }
        Ok(Self {
            processed,
            settlement_runtime_root: independently_reached_root,
            user_events,
            grant_events,
        })
    }

    pub fn envelope_identity(&self) -> &[u8] { self.processed.identity_bytes() }

    pub fn settlement_runtime_root(&self) -> [u8; 32] { self.settlement_runtime_root }

    pub(crate) fn committed_settlement_segments(
        &self,
    ) -> Result<(&[Event], &[Event]), CasperError> {
        let evidence = self
            .processed
            .evidence(offered_funded_v6_limits().evidence)
            .map_err(|error| invalid(&error.to_string()))?;
        let boundary = self
            .user_events
            .checked_add(self.grant_events)
            .ok_or_else(|| invalid("offered replay event boundary overflows"))?;
        let wallet_events = usize::try_from(evidence.wallet_settlement_log_events)
            .map_err(|_| invalid("offered wallet event count overflows"))?;
        let wallet_end = boundary
            .checked_add(wallet_events)
            .ok_or_else(|| invalid("offered wallet event boundary overflows"))?;
        let log = self.processed.deploy_log();
        let wallet = log
            .get(boundary..wallet_end)
            .ok_or_else(|| invalid("offered wallet event boundary exceeds committed log"))?;
        let receipts = log
            .get(wallet_end..)
            .ok_or_else(|| invalid("offered receipt event boundary exceeds committed log"))?;
        Ok((wallet, receipts))
    }

    pub fn verify_funding_case(&self, recomputed: &[u8]) -> Result<(), CasperError> {
        let evidence = self
            .processed
            .evidence(offered_funded_v6_limits().evidence)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        if recomputed != evidence.funding_case {
            return Err(invalid(
                "independent funding case differs from committed evidence",
            ));
        }
        Ok(())
    }

    pub fn verify_prepaid_delta(&self, recomputed: &[u8]) -> Result<(), CasperError> {
        let evidence = self
            .processed
            .evidence(offered_funded_v6_limits().evidence)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        if recomputed != evidence.prepaid_delta {
            return Err(invalid(
                "independent prepaid delta differs from committed evidence",
            ));
        }
        Ok(())
    }

    pub fn verify_wallet_prefix(
        &self,
        wallet_receipt: &[u8],
        resource_rev: u128,
        fee_rev: u128,
    ) -> Result<(), CasperError> {
        let evidence = self
            .processed
            .evidence(offered_funded_v6_limits().evidence)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let expected_resource = evidence
            .resource_rev()
            .map_err(|error| invalid(&error.to_string()))?;
        if wallet_receipt != evidence.wallet_settlement {
            return Err(invalid(
                "independent wallet receipt differs from committed cost evidence",
            ));
        }
        if resource_rev != expected_resource {
            return Err(invalid(
                "independent resource REV differs from committed cost evidence",
            ));
        }
        if fee_rev != evidence.fee_rev {
            return Err(invalid(
                "independent fee REV differs from committed cost evidence",
            ));
        }
        Ok(())
    }

    pub fn verify_complete(
        &self,
        final_root: &[u8],
        wallet_receipt: &[u8],
        resource_rev: u128,
        fee_rev: u128,
    ) -> Result<(), CasperError> {
        let evidence = self
            .processed
            .evidence(offered_funded_v6_limits().evidence)
            .map_err(|error| CasperError::RuntimeError(error.to_string()))?;
        let expected_resource = evidence
            .resource_rev()
            .map_err(|error| invalid(&error.to_string()))?;
        if final_root != evidence.post_state_root {
            return Err(invalid(
                "independent final root differs from committed cost evidence",
            ));
        }
        if wallet_receipt != evidence.wallet_settlement {
            return Err(invalid(
                "independent wallet receipt differs from committed cost evidence",
            ));
        }
        if resource_rev != expected_resource || fee_rev != evidence.fee_rev {
            return Err(invalid(
                "independent REV charge differs from committed cost evidence",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateBoundObservation {
    pub envelope_identity: [u8; 32],
    pub funding_root: [u8; 32],
    pub execution_root: [u8; 32],
    pub settlement_root: [u8; 32],
    pub final_root: [u8; 32],
    pub phlo_used: u64,
    pub fresh_phlo: u64,
    pub retained_phlo: u64,
    pub resource_rev: u128,
    pub fee_rev: u128,
    pub operation_evidence_hash: [u8; 32],
    pub funding_evidence_hash: [u8; 32],
    pub failure_class: u8,
}

fn invalid(reason: &str) -> CasperError { CasperError::RuntimeError(reason.to_owned()) }

fn check_offer_terms(
    intent: &PhloFundingIntentV2<'_>,
    selected_schedule: &PhloScheduleV1<'_>,
    genesis_minimum: u64,
    signed_limit: u64,
    signed_price: u64,
    max_phlo_limit: u64,
) -> Result<u128, CasperError> {
    if intent.base.sources.is_empty() {
        return Err(invalid(
            "offered funding requires at least one signed source",
        ));
    }
    if signed_limit > max_phlo_limit || signed_limit > intent.base.controls.limit {
        return Err(invalid(
            "offered phloLimit exceeds its signed or configured bound",
        ));
    }
    if signed_price != selected_schedule.actual_price || signed_price < genesis_minimum {
        return Err(invalid(
            "offered phloPrice differs from the adopted schedule or minimum",
        ));
    }
    if signed_price > intent.base.controls.price_ceiling
        || intent
            .base
            .controls
            .required_owner_ceilings
            .iter()
            .any(|ceiling| signed_price > *ceiling)
    {
        return Err(invalid("offered phloPrice exceeds signed owner consent"));
    }
    let selected_commitment = selected_schedule
        .digest(PhloGenesisPolicy::LIMITS)
        .map_err(|error| invalid(&error.to_string()))?;
    if intent.base.schedule_commitment != selected_commitment
        || !intent
            .base
            .controls
            .permitted_schedules
            .contains(selected_schedule)
    {
        return Err(invalid(
            "offered schedule is not the signed adopted schedule",
        ));
    }
    let ceiling = u128::from(signed_limit)
        .checked_mul(u128::from(signed_price))
        .and_then(|amount| amount.checked_add(FIXED_FEE_REV))
        .ok_or_else(|| invalid("offered REV ceiling overflows"))?;
    if ceiling > intent.base.total_exposure {
        return Err(invalid("offered REV ceiling exceeds signed total exposure"));
    }
    Ok(ceiling)
}

fn select_signed_schedule<'intent, 'terms>(
    intent: &'intent PhloFundingIntentV2<'terms>,
    signed_price: u64,
) -> Result<(&'intent PhloScheduleV1<'terms>, Vec<u8>), CasperError> {
    let mut selected = None;
    for schedule in &intent.base.controls.permitted_schedules {
        if schedule.actual_price == signed_price
            && schedule
                .digest(PhloGenesisPolicy::LIMITS)
                .map_err(|error| invalid(&error.to_string()))?
                == intent.base.schedule_commitment
            && selected.replace(schedule).is_some()
        {
            return Err(invalid(
                "offered intent selects more than one concrete schedule",
            ));
        }
    }
    let selected =
        selected.ok_or_else(|| invalid("offered intent has no matching signed schedule"))?;
    let bytes = selected
        .encode(PhloGenesisPolicy::LIMITS)
        .map_err(|error| invalid(&error.to_string()))?;
    Ok((selected, bytes))
}

pub fn prepare_offered_candidate<'a>(
    envelope: &'a Cosigned<OfferedFundedDeploy>,
    adopted: &AdoptedResourcePolicy,
    funding_root: [u8; 32],
    execution_root: [u8; 32],
    limits: OfferedCandidateLimits,
) -> Result<PreparedOfferedCandidate<'a>, CasperError> {
    envelope
        .validate_envelope()
        .map_err(|error| invalid(&error.to_string()))?;
    let versioned =
        PhloFundingIntentVersioned::decode(envelope.data.funding_intent(), limits.funding)
            .map_err(|error| invalid(&error.to_string()))?;
    let PhloFundingIntentVersioned::V2(intent) = versioned else {
        return Err(invalid(
            "production offered funding requires explicit V2 composition terms",
        ));
    };
    let signed_limit = u64::try_from(envelope.data.phlo_limit())
        .map_err(|_| invalid("offered phloLimit is negative"))?;
    let signed_price = u64::try_from(envelope.data.phlo_price())
        .map_err(|_| invalid("offered phloPrice is negative"))?;
    let (selected, selected_schedule_bytes) = select_signed_schedule(&intent, signed_price)?;
    adopted.check_acquisition_terms(&selected_schedule_bytes)?;
    let rev_ceiling = check_offer_terms(
        &intent,
        selected,
        adopted.genesis().minimum_price(),
        signed_limit,
        signed_price,
        limits.max_phlo_limit,
    )?;
    let envelope_identity = envelope
        .envelope_commitment()
        .map_err(|error| invalid(&error.to_string()))?
        .as_ref()
        .try_into()
        .map_err(|_| invalid("offered envelope identity is not 32 bytes"))?;
    Ok(PreparedOfferedCandidate {
        intent,
        selected_schedule_bytes,
        envelope_identity,
        funding_root,
        execution_root,
        phlo_limit: signed_limit,
        phlo_price: signed_price,
        fee_rev: FIXED_FEE_REV,
        rev_ceiling,
    })
}

pub fn compare_state_bound_observations(
    expected_identity: [u8; 32],
    expected_funding_root: [u8; 32],
    expected_execution_root: [u8; 32],
    phlo_limit: u64,
    phlo_price: u64,
    observed: &StateBoundObservation,
    replayed: &StateBoundObservation,
) -> Result<(), CasperError> {
    if observed != replayed {
        return Err(invalid(
            "independent replay differs from the complete private observation",
        ));
    }
    if observed.envelope_identity != expected_identity
        || observed.funding_root != expected_funding_root
        || observed.execution_root != expected_execution_root
    {
        return Err(invalid(
            "candidate differs from its signed envelope or captured funding/execution roots",
        ));
    }
    if observed.fresh_phlo > observed.phlo_used
        || observed
            .phlo_used
            .checked_add(observed.retained_phlo)
            .filter(|used| *used <= phlo_limit)
            .is_none()
    {
        return Err(invalid("candidate exceeds signed phloLimit"));
    }
    let resource_rev = u128::from(observed.fresh_phlo)
        .checked_add(u128::from(observed.retained_phlo))
        .and_then(|used| used.checked_mul(u128::from(phlo_price)))
        .ok_or_else(|| invalid("candidate resource REV charge overflows"))?;
    let total_rev = resource_rev
        .checked_add(FIXED_FEE_REV)
        .ok_or_else(|| invalid("candidate total REV charge overflows"))?;
    if observed.resource_rev != resource_rev || observed.fee_rev != FIXED_FEE_REV {
        return Err(invalid(
            "candidate REV charge differs from signed price and separate fee",
        ));
    }
    let ceiling = u128::from(phlo_limit)
        .checked_mul(u128::from(phlo_price))
        .and_then(|amount| amount.checked_add(FIXED_FEE_REV))
        .ok_or_else(|| invalid("candidate REV ceiling overflows"))?;
    if total_rev > ceiling {
        return Err(invalid("candidate REV charge exceeds signed ceiling"));
    }
    Ok(())
}

impl PreparedOfferedCandidate<'_> {
    pub fn selected_terms<'policy, 'terms>(
        &'terms self,
        adopted: &'policy AdoptedResourcePolicy,
    ) -> Result<CompatibleAcquisitionTerms<'policy, 'terms>, CasperError> {
        adopted.check_acquisition_terms(&self.selected_schedule_bytes)
    }

    pub fn authorize_publication(&self) -> Result<CandidatePublicationPermit, CasperError> {
        Err(invalid(
            "offered candidate publication requires authenticated proposal inputs, deterministic execution scope, side-effect-free external services, and verified rollback",
        ))
    }

    pub(crate) fn authorize_publication_after_replay(
        &self,
        processed: &OfferedProcessedDeploy,
        candidate_root: [u8; 32],
        certificate: &CertifiedOfferedDraft,
        budget: &HostWorkBudget,
    ) -> Result<CandidatePublicationPermit, CasperError> {
        let protocol = offered_funded_v6_limits();
        let compare_bound = protocol
            .evidence
            .total_bytes
            .checked_add(protocol.deploy_log_bytes)
            .and_then(|bytes| bytes.checked_add(protocol.envelope.payload.deploy_bytes))
            .and_then(|bytes| bytes.checked_add(protocol.envelope.payload.signing.total_bytes))
            .and_then(|bytes| bytes.checked_add(protocol.envelope.payload.funding.wire.total_bytes))
            .ok_or_else(|| invalid("offered candidate comparison bound overflows"))?;
        let units = HostWorkUnits::new(
            u64::try_from(compare_bound)
                .map_err(|_| invalid("offered candidate comparison bound overflows"))?,
        );
        for dimension in [
            HostWorkDimension::VerificationBytes,
            HostWorkDimension::VerificationOperations,
        ] {
            budget
                .reserve(dimension, units)
                .map_err(|error| invalid(&error.to_string()))?;
        }
        let evidence = processed
            .evidence(offered_funded_v6_limits().evidence)
            .map_err(|error| invalid(&error.to_string()))?;
        if !certificate.matches_candidate(processed)
            || processed.identity_bytes() != self.envelope_identity
            || evidence.envelope_commitment != self.envelope_identity
            || certificate.envelope_identity() != self.envelope_identity
            || evidence.original_funding_root != self.funding_root
            || evidence.phlo_limit != self.phlo_limit
            || evidence.phlo_price != self.phlo_price
            || evidence.fee_rev != self.fee_rev
            || evidence.post_state_root != candidate_root
            || certificate.settled_root() != candidate_root
        {
            return Err(invalid(
                "offered draft and independent replay differ from signed preflight",
            ));
        }
        if !OFFERED_PRODUCTION_READY {
            return Err(invalid(
                "offered publication awaits complete independent replay activation",
            ));
        }
        Ok(CandidatePublicationPermit {
            envelope_identity: self.envelope_identity,
        })
    }

    pub(crate) fn authorize_private_draft(
        &self,
        observed_identity: [u8; 32],
        complete_measurements: bool,
        observed_phlo_usage: Option<u64>,
    ) -> Result<CandidateDraftSettlementPermit, CasperError> {
        if observed_identity != self.envelope_identity
            || !complete_measurements
            || observed_phlo_usage.is_none()
        {
            return Err(invalid(
                "private offered draft lacks the authenticated complete native cut",
            ));
        }
        Ok(CandidateDraftSettlementPermit {
            envelope_identity: self.envelope_identity,
        })
    }

    pub fn compare_private_replay(
        &self,
        observed: &StateBoundObservation,
        replayed: &StateBoundObservation,
    ) -> Result<(), CasperError> {
        compare_state_bound_observations(
            self.envelope_identity,
            self.funding_root,
            self.execution_root,
            self.phlo_limit,
            self.phlo_price,
            observed,
            replayed,
        )
    }
}

#[cfg(test)]
mod tests;
