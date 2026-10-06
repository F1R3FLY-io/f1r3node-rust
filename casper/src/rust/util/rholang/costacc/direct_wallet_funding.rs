use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;

use crypto::rust::signatures::signed::{Cosigned, CosignedError, ToMessage};
use models::rhoapi::cost_signature::Value;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{CostSignature, Par};
use models::rust::phlo_intent::{
    PhloConversionCompositionV2, PhloFundingIntentError, PhloFundingIntentLimits,
    PhloFundingIntentV1, PhloFundingIntentV2Error, PhloFundingIntentV2Limits,
    PhloFundingIntentVersioned,
};
use models::rust::phlo_resource::PhloAuthorityNode;
use models::rust::phlo_source::PhloSourcePolicyV1;
use models::rust::signed_phlo_deploy::{FundedDeploy, OfferedFundedDeploy};
use prost::Message;
use rholang::rust::interpreter::accounting::authority::cost_signature_to_sig;
use rholang::rust::interpreter::accounting::{principal_ground_v61, SignatureChannel};
use thiserror::Error;

use super::offered_grants::VerifiedOfferedGrantSources;
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
    NativeAttemptSettlementInput, NativeAttemptSettlementLimits, NativeCapturedSettlement,
    NativeFundedAttempt, NativeFundedExecutionContext, NativeFundedReplayInput,
    NativeFundedReplayLimits, NativeFundedSettlement, NativeGrantIssuanceInput,
    NativeGrantSettlementInput, NativeOfferedAttempt, NativeOfferedCandidateResult,
    NativeOfferedFamilyLimits, NativeOfferedPreparedResult, NativeOfferedProductionLimits,
    NativeScopedOfferedResult, PendingReplayedNativeSettlement, ReplayedNativeFundedUser,
};

#[derive(Clone, Copy, Debug)]
pub struct DirectWalletFundingLimits {
    pub members: NonZeroUsize,
    pub funding: PhloFundingIntentLimits,
}

#[derive(Debug)]
pub struct DirectWalletFunding<'a, A = FundedDeploy> {
    envelope: &'a Cosigned<A>,
    record: PhloFundingIntentVersioned<'a>,
    payers: BTreeMap<[u8; 32], VaultPayer>,
    onchain_payers: BTreeSet<[u8; 32]>,
    grant_root: Option<[u8; 32]>,
}

impl<'a, A> DirectWalletFunding<'a, A> {
    pub fn envelope(&self) -> &'a Cosigned<A> { self.envelope }
    pub fn record(&self) -> &PhloFundingIntentV1<'a> {
        match &self.record {
            PhloFundingIntentVersioned::V1(record) => record,
            PhloFundingIntentVersioned::V2(record) => &record.base,
        }
    }
    pub fn versioned_record(&self) -> &PhloFundingIntentVersioned<'a> { &self.record }
    pub fn payers(&self) -> &BTreeMap<[u8; 32], VaultPayer> { &self.payers }
    pub fn onchain_payers(&self) -> &BTreeSet<[u8; 32]> { &self.onchain_payers }
    pub fn grant_root(&self) -> Option<[u8; 32]> { self.grant_root }
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
    #[error("on-chain purse source has ambiguous private-name authority")]
    AmbiguousOnchainAuthority,
    #[error("on-chain purse source cannot pay the deployment fee")]
    OnchainFee,
    #[error("delegated funding proof belongs to another offered envelope")]
    GrantEnvelopeMismatch,
    #[error("delegated funding proof does not cover its signed source")]
    GrantSourceMismatch,
    #[error(
        "persistent funding grants and conversion terms require authenticated settlement support"
    )]
    UnsupportedOfferedTerms,
    #[error(transparent)]
    Signature(#[from] CosignedError),
    #[error(transparent)]
    Record(#[from] PhloFundingIntentError),
    #[error(transparent)]
    VersionedRecord(#[from] PhloFundingIntentV2Error),
    #[error(transparent)]
    Payer(#[from] VaultPayerError),
}

pub fn authorize_direct_wallet_funding(
    envelope: &Cosigned<FundedDeploy>,
    limits: DirectWalletFundingLimits,
) -> Result<DirectWalletFunding<'_>, DirectWalletFundingError> {
    let record = PhloFundingIntentVersioned::V1(PhloFundingIntentV1::decode(
        envelope.data.funding_intent(),
        limits.funding,
    )?);
    authorize_wallets(envelope, record, limits, None, false)
}

pub fn authorize_offered_direct_wallet_funding(
    envelope: &Cosigned<OfferedFundedDeploy>,
    limits: DirectWalletFundingLimits,
) -> Result<DirectWalletFunding<'_, OfferedFundedDeploy>, DirectWalletFundingError> {
    let base = limits.funding;
    let versioned = PhloFundingIntentV2Limits {
        wire: base.wire,
        base,
        grant_uses: base.wire.total_bytes / 8,
        grant_id_bytes: base.wire.field_bytes,
        quote_evidence_bytes: base.wire.field_bytes,
    };
    let record = PhloFundingIntentVersioned::decode(envelope.data.funding_intent(), versioned)?;
    if let PhloFundingIntentVersioned::V2(v2) = &record {
        if !v2.grant_uses.is_empty()
            || !matches!(v2.conversion, PhloConversionCompositionV2::NoConversion)
        {
            return Err(DirectWalletFundingError::UnsupportedOfferedTerms);
        }
    }
    let allow_onchain = matches!(record, PhloFundingIntentVersioned::V2(_));
    authorize_wallets(envelope, record, limits, None, allow_onchain)
}

pub fn authorize_offered_direct_wallet_funding_with_grants<'a>(
    envelope: &'a Cosigned<OfferedFundedDeploy>,
    limits: DirectWalletFundingLimits,
    verified: &VerifiedOfferedGrantSources,
) -> Result<DirectWalletFunding<'a, OfferedFundedDeploy>, DirectWalletFundingError> {
    let base = limits.funding;
    let versioned = PhloFundingIntentV2Limits {
        wire: base.wire,
        base,
        grant_uses: base.wire.total_bytes / 8,
        grant_id_bytes: base.wire.field_bytes,
        quote_evidence_bytes: base.wire.field_bytes,
    };
    let record = PhloFundingIntentVersioned::decode(envelope.data.funding_intent(), versioned)?;
    let PhloFundingIntentVersioned::V2(v2) = &record else {
        return Err(DirectWalletFundingError::GrantSourceMismatch);
    };
    if !matches!(v2.conversion, PhloConversionCompositionV2::NoConversion) {
        return Err(DirectWalletFundingError::UnsupportedOfferedTerms);
    }
    let identity = envelope.envelope_commitment()?;
    if identity.as_ref() != verified.envelope_identity() {
        return Err(DirectWalletFundingError::GrantEnvelopeMismatch);
    }
    if v2.grant_uses.len() != verified.source_indices().count() {
        return Err(DirectWalletFundingError::GrantSourceMismatch);
    }
    for grant in &v2.grant_uses {
        let Some(source) = v2.base.sources.get(grant.source_index as usize) else {
            return Err(DirectWalletFundingError::GrantSourceMismatch);
        };
        let Some(payer) = verified.payer_for_source(grant.source_index) else {
            return Err(DirectWalletFundingError::GrantSourceMismatch);
        };
        if payer.custody_key.as_slice() != source.custody() {
            return Err(DirectWalletFundingError::GrantSourceMismatch);
        }
    }
    authorize_wallets(envelope, record, limits, Some(verified), true)
}

fn private_name_payer(
    source: &PhloSourcePolicyV1<'_>,
) -> Result<Option<VaultPayer>, DirectWalletFundingError> {
    let mut selected: Option<VaultPayer> = None;
    for permission in source.resources() {
        let [PhloAuthorityNode::Ground(bytes)] = permission.authority.as_slice() else {
            continue;
        };
        let Ok(par) = Par::decode(*bytes) else {
            continue;
        };
        if par.encode_to_vec().as_slice() != *bytes
            || !par.sends.is_empty()
            || !par.receives.is_empty()
            || !par.news.is_empty()
            || !par.exprs.is_empty()
            || !par.matches.is_empty()
            || par.unforgeables.len() != 1
            || !par.bundles.is_empty()
            || !par.connectives.is_empty()
            || !par.conditionals.is_empty()
            || !par.locally_free.is_empty()
            || par.connective_used
            || !par.cost_signed_terms.is_empty()
            || !par.cost_stacks.is_empty()
            || !matches!(
                par.unforgeables[0].unf_instance.as_ref(),
                Some(UnfInstance::GPrivateBody(private)) if private.id.len() == 32
            )
        {
            continue;
        }
        let payer = vault_payer(&CostSignature {
            value: Some(Value::Name(par)),
        })?;
        if payer.custody_key.as_slice() != source.custody() {
            continue;
        }
        match &selected {
            Some(previous) if previous != &payer => {
                return Err(DirectWalletFundingError::AmbiguousOnchainAuthority);
            }
            Some(_) => {}
            None => selected = Some(payer),
        }
    }
    if let Some(payer) = &selected {
        if source.fee_permitted() {
            return Err(DirectWalletFundingError::OnchainFee);
        }
        let signature = cost_signature_to_sig(&payer.signature)
            .map_err(|_| DirectWalletFundingError::AmbiguousOnchainAuthority)?;
        let rholang::rust::interpreter::accounting::Sig::Ground(bytes) = &signature else {
            return Err(DirectWalletFundingError::AmbiguousOnchainAuthority);
        };
        let location = SignatureChannel::from_sig(&signature).par.encode_to_vec();
        if source.resources().iter().any(|permission| {
            permission.location != location
                || permission.authority != [PhloAuthorityNode::Ground(bytes.as_slice())]
        }) {
            return Err(DirectWalletFundingError::AmbiguousOnchainAuthority);
        }
    }
    Ok(selected)
}

fn authorize_wallets<'a, A: std::fmt::Debug + serde::Serialize + ToMessage + Clone>(
    envelope: &'a Cosigned<A>,
    record: PhloFundingIntentVersioned<'a>,
    limits: DirectWalletFundingLimits,
    grants: Option<&VerifiedOfferedGrantSources>,
    allow_onchain: bool,
) -> Result<DirectWalletFunding<'a, A>, DirectWalletFundingError> {
    if envelope.signers().len() > limits.members.get() {
        return Err(DirectWalletFundingError::TooManyMembers);
    }
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
    let mut onchain_payers = BTreeSet::new();
    let sources = match &record {
        PhloFundingIntentVersioned::V1(record) => &record.sources,
        PhloFundingIntentVersioned::V2(record) => &record.base.sources,
    };
    for (index, source) in sources.iter().enumerate() {
        let key: [u8; 32] = source
            .custody()
            .try_into()
            .map_err(|_| DirectWalletFundingError::MalformedCustody { index })?;
        let grant = grants.and_then(|grants| grants.payer_for_source(index as u32));
        if grant.is_some_and(|payer| payer.custody_key != key) {
            return Err(DirectWalletFundingError::GrantSourceMismatch);
        }
        let payer = if let Some(payer) = witnessed.get(&key).or(grant) {
            payer.clone()
        } else if allow_onchain {
            let payer = private_name_payer(source)?
                .ok_or(DirectWalletFundingError::UnwitnessedCustody { index })?;
            onchain_payers.insert(key);
            payer
        } else {
            return Err(DirectWalletFundingError::UnwitnessedCustody { index });
        };
        payers.insert(key, payer);
    }
    Ok(DirectWalletFunding {
        envelope,
        record,
        payers,
        onchain_payers,
        grant_root: grants.map(VerifiedOfferedGrantSources::root),
    })
}

#[cfg(test)]
mod tests;
