use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use crypto::rust::signatures::signed::{Cosigned, CosignedError, ToMessage};
use models::rhoapi::cost_signature::Value;
use models::rhoapi::CostSignature;
use models::rust::phlo_intent::{
    PhloFundingIntentError, PhloFundingIntentLimits, PhloFundingIntentV1,
};
use models::rust::signed_phlo_deploy::{FundedDeploy, OfferedFundedDeploy};
use rholang::rust::interpreter::accounting::principal_ground_v61;
use thiserror::Error;

use super::vault_payer::{vault_payer, VaultPayer, VaultPayerError};

mod snapshot;
pub use snapshot::{DirectWalletSnapshot, DirectWalletSnapshotError, DirectWalletSource};

mod binding;
pub use binding::{CheckedDirectWalletFunding, DirectWalletBindingError};

mod policy_snapshot;
pub use policy_snapshot::{
    CheckedDirectWalletPolicy, DirectWalletPolicySnapshot, DirectWalletPolicySnapshotError,
};

mod settlement;
pub use settlement::{
    CheckedDirectWalletRequest, CheckedDirectWalletSettlement, DirectWalletSettlementError,
    PreparedDirectWalletSettlement,
};

mod execution;
pub use execution::{
    NativeAttemptSettlementInput, NativeAttemptSettlementLimits, NativeFundedAttempt,
    NativeFundedExecutionContext, NativeFundedReplayInput, NativeFundedReplayLimits,
};

#[derive(Clone, Copy, Debug)]
pub struct DirectWalletFundingLimits {
    pub members: NonZeroUsize,
    pub funding: PhloFundingIntentLimits,
}

#[derive(Debug)]
pub struct DirectWalletFunding<'a, A = FundedDeploy> {
    envelope: &'a Cosigned<A>,
    record: PhloFundingIntentV1<'a>,
    payers: BTreeMap<[u8; 32], VaultPayer>,
}

impl<'a, A> DirectWalletFunding<'a, A> {
    pub fn envelope(&self) -> &'a Cosigned<A> { self.envelope }
    pub fn record(&self) -> &PhloFundingIntentV1<'a> { &self.record }
    pub fn payers(&self) -> &BTreeMap<[u8; 32], VaultPayer> { &self.payers }
}

#[derive(Debug, Error)]
pub enum DirectWalletFundingError {
    #[error("direct-wallet funding exceeds the envelope member limit")]
    TooManyMembers,
    #[error("direct-wallet funding source {index} has a malformed custody identity")]
    MalformedCustody { index: usize },
    #[error("direct-wallet funding source {index} has no verified owner signature")]
    UnwitnessedCustody { index: usize },
    #[error("direct-wallet funding has conflicting custody projections")]
    ConflictingCustody,
    #[error(transparent)]
    Signature(#[from] CosignedError),
    #[error(transparent)]
    Record(#[from] PhloFundingIntentError),
    #[error(transparent)]
    Payer(#[from] VaultPayerError),
}

pub fn authorize_direct_wallet_funding(
    envelope: &Cosigned<FundedDeploy>,
    limits: DirectWalletFundingLimits,
) -> Result<DirectWalletFunding<'_>, DirectWalletFundingError> {
    authorize_wallets(envelope, envelope.data.funding_intent(), limits)
}

pub fn authorize_offered_direct_wallet_funding(
    envelope: &Cosigned<OfferedFundedDeploy>,
    limits: DirectWalletFundingLimits,
) -> Result<DirectWalletFunding<'_, OfferedFundedDeploy>, DirectWalletFundingError> {
    authorize_wallets(envelope, envelope.data.funding_intent(), limits)
}

fn authorize_wallets<'a, A: std::fmt::Debug + serde::Serialize + ToMessage + Clone>(
    envelope: &'a Cosigned<A>,
    funding: &'a [u8],
    limits: DirectWalletFundingLimits,
) -> Result<DirectWalletFunding<'a, A>, DirectWalletFundingError> {
    if envelope.signers().len() > limits.members.get() {
        return Err(DirectWalletFundingError::TooManyMembers);
    }
    let record = PhloFundingIntentV1::decode(funding, limits.funding)?;
    envelope.validate_envelope()?;
    let mut witnessed = BTreeMap::new();
    for signer in envelope.selected_signers_v61()? {
        let payer = vault_payer(&CostSignature {
            value: Some(Value::Ground(principal_ground_v61(&signer.pk.bytes))),
        })?;
        if let Some(previous) = witnessed.insert(payer.custody_key, payer.clone()) {
            if previous != payer {
                return Err(DirectWalletFundingError::ConflictingCustody);
            }
        }
    }
    let mut payers = BTreeMap::new();
    for (index, source) in record.sources.iter().enumerate() {
        let key: [u8; 32] = source
            .custody()
            .try_into()
            .map_err(|_| DirectWalletFundingError::MalformedCustody { index })?;
        let payer = witnessed
            .get(&key)
            .ok_or(DirectWalletFundingError::UnwitnessedCustody { index })?;
        payers.insert(key, payer.clone());
    }
    Ok(DirectWalletFunding {
        envelope,
        record,
        payers,
    })
}
