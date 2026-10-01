use std::num::NonZeroUsize;

use crypto::rust::signatures::signed::{Cosigned, CosignedError, Cosigner, ToMessage};
use models::rust::phlo_intent::{
    PhloFundingIntentError, PhloFundingIntentLimits, PhloFundingIntentV1,
};
use models::rust::phlo_wire::PhloWireError;
use models::rust::signed_phlo_deploy::{FundedDeploy, OfferedFundedDeploy};
use thiserror::Error;

use super::{
    check_phlo_family_wire_consent, CheckedPhloBoundFamilyConsent, CheckedPhloFundingFamily,
    DecodedPhloFamilyWireIntent, PhloConsentError, PhloConsentLimits,
};
use crate::rust::interpreter::accounting::phlo_controls::{
    PhloControlsBinding, PhloControlsView, PhloFundingTerms, PhloFundingTermsError, PhloOffer,
    PhloOfferError, PhloSchedulePolicy, PhloSchedulePolicyMismatch, SignedPhloControls,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloFundingIntentBinding<'a> {
    record: &'a PhloFundingIntentV1<'a>,
    controls: PhloControlsBinding<'a>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhloFundingIntentView<'a> {
    record: &'a PhloFundingIntentV1<'a>,
    controls: PhloControlsView<'a>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckedPhloFundingIntent<'a> {
    record: &'a PhloFundingIntentV1<'a>,
    bound: CheckedPhloBoundFamilyConsent<'a>,
}

#[derive(Clone, Copy, Debug)]
pub struct SignedPhloConsentLimits {
    pub members: NonZeroUsize,
    pub intent: PhloFundingIntentLimits,
    pub consent: PhloConsentLimits,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CheckedSignedPhloFundingIntent<'a, A = FundedDeploy> {
    envelope: &'a Cosigned<A>,
    intent: CheckedPhloFundingIntent<'a>,
}

impl<A> Copy for CheckedSignedPhloFundingIntent<'_, A> {}

impl<A> Clone for CheckedSignedPhloFundingIntent<'_, A> {
    fn clone(&self) -> Self { *self }
}

impl<'a, A> CheckedSignedPhloFundingIntent<'a, A> {
    pub fn envelope(self) -> &'a Cosigned<A> { self.envelope }
    pub fn intent(self) -> CheckedPhloFundingIntent<'a> { self.intent }
}

impl<'a, A: std::fmt::Debug + serde::Serialize + ToMessage> CheckedSignedPhloFundingIntent<'a, A> {
    pub fn verified_witnesses(self) -> impl Iterator<Item = &'a Cosigner> {
        self.envelope
            .signers()
            .iter()
            .filter(|signer| !signer.sig.is_empty())
    }
}

#[derive(Debug, Error)]
pub enum SignedPhloConsentError {
    #[error("signed funding envelope exceeds the member limit")]
    TooManyMembers,
    #[error("funding record differs from the signed envelope")]
    RecordMismatch,
    #[error(transparent)]
    Record(#[from] PhloFundingIntentError),
    #[error(transparent)]
    Signature(#[from] CosignedError),
    #[error(transparent)]
    Funding(#[from] PhloFundingIntentCheckError),
    #[error(transparent)]
    Offer(#[from] PhloOfferError),
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum PhloFundingIntentCheckError {
    #[error("execution schedule differs from the funding intent's selected schedule")]
    SelectedScheduleMismatch,
    #[error(transparent)]
    Consent(#[from] PhloConsentError),
    #[error(transparent)]
    FundingTerms(#[from] PhloFundingTermsError),
}

impl<'a> PhloFundingIntentBinding<'a> {
    pub fn new(
        record: &'a PhloFundingIntentV1<'a>,
        limits: PhloFundingIntentLimits,
    ) -> Result<Self, PhloFundingIntentError> {
        record.encode(limits)?;
        let controls = PhloControlsBinding::new(&record.controls, limits.controls())?;
        Ok(Self { record, controls })
    }

    pub fn view(&self) -> Result<PhloFundingIntentView<'_>, PhloWireError> {
        Ok(PhloFundingIntentView {
            record: self.record,
            controls: self.controls.view()?,
        })
    }
}

impl PhloFundingIntentView<'_> {
    pub fn check_schedule_policy(
        &self,
        required: PhloSchedulePolicy<'_>,
    ) -> Result<(), PhloSchedulePolicyMismatch> {
        self.controls
            .check_schedule_policy(&self.record.schedule_commitment, required)
    }

    pub fn record(&self) -> &PhloFundingIntentV1<'_> { self.record }
    pub fn controls(&self) -> SignedPhloControls<'_> { self.controls.terms() }

    pub fn check_signed_family<'a>(
        &'a self,
        envelope: &'a Cosigned<FundedDeploy>,
        family: &'a CheckedPhloFundingFamily<'a>,
        terms: PhloFundingTerms<'a>,
        limits: SignedPhloConsentLimits,
    ) -> Result<CheckedSignedPhloFundingIntent<'a>, SignedPhloConsentError> {
        if envelope.signers().len() > limits.members.get() {
            return Err(SignedPhloConsentError::TooManyMembers);
        }
        if self.record.encode(limits.intent)? != envelope.data.funding_intent() {
            return Err(SignedPhloConsentError::RecordMismatch);
        }
        envelope.validate_envelope()?;
        let intent = self.check_family(family, terms, limits.consent)?;
        Ok(CheckedSignedPhloFundingIntent { envelope, intent })
    }

    pub fn check_offered_signed_family<'a>(
        &'a self,
        envelope: &'a Cosigned<OfferedFundedDeploy>,
        family: &'a CheckedPhloFundingFamily<'a>,
        terms: PhloFundingTerms<'a>,
        limits: SignedPhloConsentLimits,
    ) -> Result<CheckedSignedPhloFundingIntent<'a, OfferedFundedDeploy>, SignedPhloConsentError>
    {
        if envelope.signers().len() > limits.members.get() {
            return Err(SignedPhloConsentError::TooManyMembers);
        }
        if self.record.encode(limits.intent)? != envelope.data.funding_intent() {
            return Err(SignedPhloConsentError::RecordMismatch);
        }
        envelope.validate_envelope()?;
        let intent = self.check_family(family, terms, limits.consent)?;
        let offer = PhloOffer {
            limit: envelope.data.phlo_limit(),
            price: envelope.data.phlo_price(),
        };
        for case in family.cases() {
            case.obligations.execution().controls().bind_offer(offer)?;
        }
        Ok(CheckedSignedPhloFundingIntent { envelope, intent })
    }

    pub fn check_family<'a>(
        &'a self,
        family: &'a CheckedPhloFundingFamily<'a>,
        terms: PhloFundingTerms<'a>,
        limits: PhloConsentLimits,
    ) -> Result<CheckedPhloFundingIntent<'a>, PhloFundingIntentCheckError> {
        let intent = DecodedPhloFamilyWireIntent {
            controls: self.controls(),
            total_exposure: self.record.total_exposure,
            sources: &self.record.sources,
        };
        let consent = check_phlo_family_wire_consent(family, intent, limits)?;
        if family.cases()[0]
            .obligations
            .execution()
            .controls()
            .schedule()
            .commitment
            != self.record.schedule_commitment
        {
            return Err(PhloFundingIntentCheckError::SelectedScheduleMismatch);
        }
        let bound = consent.bind_funding_terms(terms)?;
        Ok(CheckedPhloFundingIntent {
            record: self.record,
            bound,
        })
    }
}

impl<'a> CheckedPhloFundingIntent<'a> {
    pub fn record(self) -> &'a PhloFundingIntentV1<'a> { self.record }
    pub fn bound(self) -> CheckedPhloBoundFamilyConsent<'a> { self.bound }
}
