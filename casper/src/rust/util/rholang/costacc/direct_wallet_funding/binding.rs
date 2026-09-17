use rholang::rust::interpreter::accounting::phlo_controls::PhloFundingTerms;
use rholang::rust::interpreter::accounting::phlo_execution::{
    CheckedPhloFundingFamily, CheckedSignedPhloFundingIntent, PhloFundingIntentView,
    SignedPhloConsentError, SignedPhloConsentLimits,
};

use super::*;

#[derive(Debug)]
pub struct CheckedDirectWalletFunding<'a, A = FundedDeploy> {
    snapshot: &'a DirectWalletSnapshot<'a, A>,
    consent: CheckedSignedPhloFundingIntent<'a, A>,
}

impl<'a, A> CheckedDirectWalletFunding<'a, A> {
    pub fn snapshot(&self) -> &'a DirectWalletSnapshot<'a, A> { self.snapshot }
    pub fn consent(&self) -> CheckedSignedPhloFundingIntent<'a, A> { self.consent }
}

#[derive(Debug, Error)]
pub enum DirectWalletBindingError {
    #[error("funding family sources differ from the authenticated wallet snapshot")]
    SourcesMismatch,
    #[error(transparent)]
    Consent(#[from] SignedPhloConsentError),
}

impl<A> DirectWalletSnapshot<'_, A> {
    fn check_family_sources(
        &self,
        family: &CheckedPhloFundingFamily<'_>,
    ) -> Result<(), DirectWalletBindingError> {
        if family.sources().len() != self.sources().len() {
            return Err(DirectWalletBindingError::SourcesMismatch);
        }
        let sources: BTreeMap<_, _> = self
            .sources()
            .iter()
            .map(|source| (source.source().custody, source.source()))
            .collect();
        for source in family.sources() {
            if sources.get(source.custody) != Some(source) {
                return Err(DirectWalletBindingError::SourcesMismatch);
            }
        }
        Ok(())
    }
}

macro_rules! impl_wallet_family_binding {
    ($envelope:ty, $check:ident) => {
        impl DirectWalletSnapshot<'_, $envelope> {
            pub fn bind_family<'a>(
                &'a self,
                view: &'a PhloFundingIntentView<'a>,
                family: &'a CheckedPhloFundingFamily<'a>,
                terms: PhloFundingTerms<'a>,
                limits: SignedPhloConsentLimits,
            ) -> Result<CheckedDirectWalletFunding<'a, $envelope>, DirectWalletBindingError> {
                self.check_family_sources(family)?;
                let consent =
                    view.$check(self.authorization().envelope(), family, terms, limits)?;
                Ok(CheckedDirectWalletFunding {
                    snapshot: self,
                    consent,
                })
            }
        }
    };
}

impl_wallet_family_binding!(FundedDeploy, check_signed_family);
impl_wallet_family_binding!(OfferedFundedDeploy, check_offered_signed_family);
