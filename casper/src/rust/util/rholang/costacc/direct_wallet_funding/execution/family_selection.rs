use std::collections::BTreeMap;
use std::mem::size_of;
use std::num::NonZeroUsize;

use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::CostSignature;
use models::rust::host_work::{HostWorkDimension, HostWorkUnits};
use models::rust::phlo_resource::PhloAuthorityNode;
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use rholang::rust::interpreter::accounting::authority::{
    authority_funding_signatures_for_events_with_host_work, cost_signature_to_sig,
    AuthorityByteEvent, AuthorityEvent,
};
use rholang::rust::interpreter::accounting::monetary_allocation::MonetaryCursor;
use rholang::rust::interpreter::accounting::phlo_execution::{
    select_phlo_funding_family, CheckedPhloObligations, PhloFamilyFundingInput,
    PhloFamilyFundingSelection, PhloFundingRequirement, PhloFundingSource, PhloObligationKey,
};
use rholang::rust::interpreter::host_work::HostWorkBudget;

use super::producer::NativeOfferedFamilyLimits;
use super::settlement::invalid;
use crate::rust::errors::CasperError;
use crate::rust::util::rholang::costacc::direct_wallet_funding::DirectWalletPolicySnapshot;
use crate::rust::util::rholang::costacc::vault_payer::VaultPayer;

fn reserve(
    budget: &HostWorkBudget,
    dimension: HostWorkDimension,
    amount: usize,
) -> Result<(), CasperError> {
    budget
        .reserve(
            dimension,
            HostWorkUnits::new(
                u64::try_from(amount).map_err(|_| invalid("native funding work overflow"))?,
            ),
        )
        .map(|_| ())
        .map_err(|error| invalid(error.to_string()))
}

pub(crate) struct NativeRootedFamilySelection<'a> {
    pub sources: Vec<PhloFundingSource<'a>>,
    pub eligible: Vec<Vec<bool>>,
    pub selected: PhloFamilyFundingSelection,
    pub total_exposure_limit: u128,
}

fn verified_onchain_authority(
    payer: &VaultPayer,
    observed: &BTreeMap<[u8; 32], CostSignature>,
) -> Result<Vec<u8>, CasperError> {
    let Some(CostSignatureValue::Name(_)) = payer.signature.value.as_ref() else {
        return Err(invalid("on-chain purse lacks a private-name authority"));
    };
    let signature = cost_signature_to_sig(&payer.signature).map_err(invalid)?;
    if observed.get(&signature.lane_hash()) != Some(&payer.signature) {
        return Err(invalid(
            "on-chain purse has no independently observed private-name authority",
        ));
    }
    match signature {
        rholang::rust::interpreter::accounting::Sig::Ground(bytes) => Ok(bytes),
        _ => Err(invalid("on-chain private-name authority is not grounded")),
    }
}

pub(crate) fn select_rooted_native_family<'a>(
    snapshot: &DirectWalletPolicySnapshot<'a, OfferedFundedDeploy>,
    obligations: &CheckedPhloObligations<'_>,
    authority_events: &[AuthorityEvent<[u8; 32]>],
    authority_byte_events: &[AuthorityByteEvent],
    limits: NativeOfferedFamilyLimits,
    budget: &HostWorkBudget,
) -> Result<NativeRootedFamilySelection<'a>, CasperError> {
    let onchain = snapshot.wallets().authorization().onchain_payers();
    let observed = if onchain.is_empty() {
        None
    } else {
        if !snapshot
            .wallets()
            .authorization()
            .envelope()
            .data
            .body()
            .authority_presentations
            .is_empty()
        {
            return Err(invalid(
                "on-chain purse funding forbids caller authority presentations",
            ));
        }
        Some(
            authority_funding_signatures_for_events_with_host_work(
                authority_events,
                authority_byte_events,
                &[],
                Some(budget),
            )
            .map_err(invalid)?,
        )
    };
    let source_count = snapshot.wallets().sources().len();
    let columns = obligations.keys().len();
    let cells = source_count
        .checked_mul(columns)
        .ok_or_else(|| invalid("native funding eligibility size overflow"))?;
    reserve(
        budget,
        HostWorkDimension::SearchStateBytes,
        source_count
            .checked_mul(size_of::<PhloFundingSource<'_>>() + size_of::<Vec<bool>>())
            .and_then(|n| n.checked_add(cells))
            .ok_or_else(|| invalid("native funding eligibility size overflow"))?,
    )?;
    reserve(budget, HostWorkDimension::VerificationOperations, cells)?;
    let mut sources = Vec::new();
    let mut eligible = Vec::new();
    sources
        .try_reserve_exact(source_count)
        .map_err(|_| invalid("native funding source allocation failed"))?;
    eligible
        .try_reserve_exact(source_count)
        .map_err(|_| invalid("native funding eligibility allocation failed"))?;
    let signed_sources = &snapshot.wallets().authorization().record().sources;
    if signed_sources.len() != source_count {
        return Err(invalid(
            "signed funding source count differs from rooted snapshot",
        ));
    }
    for (physical, signed) in snapshot.wallets().sources().iter().zip(signed_sources) {
        let source = physical.source();
        if source.custody != signed.custody() {
            return Err(invalid(
                "signed funding source differs from rooted snapshot",
            ));
        }
        let custody: [u8; 32] = source
            .custody
            .try_into()
            .map_err(|_| invalid("invalid on-chain custody width"))?;
        let onchain_authority = if onchain.contains(&custody) {
            let payer = snapshot
                .wallets()
                .authorization()
                .payers()
                .get(&custody)
                .ok_or_else(|| invalid("on-chain purse payer is absent"))?;
            Some(verified_onchain_authority(
                payer,
                observed
                    .as_ref()
                    .ok_or_else(|| invalid("on-chain authority evidence is absent"))?,
            )?)
        } else {
            None
        };
        sources.push(source);
        let mut row = Vec::new();
        row.try_reserve_exact(columns)
            .map_err(|_| invalid("native funding eligibility row allocation failed"))?;
        for key in obligations.keys() {
            let permitted = match key {
                PhloObligationKey::Fee => signed.fee_permitted(),
                PhloObligationKey::Resource(resource)
                | PhloObligationKey::RetainedResource(resource) => {
                    let wire = resource
                        .wire_key(limits.measured.settlement.matching.execution)
                        .map_err(invalid)?;
                    let mut allowed = false;
                    for permission in signed.resources() {
                        reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
                        if permission == &wire
                            && onchain_authority.as_ref().is_none_or(|bytes| {
                                wire.authority == [PhloAuthorityNode::Ground(bytes.as_slice())]
                            })
                        {
                            allowed = true;
                        }
                    }
                    allowed
                }
            };
            row.push(permitted);
        }
        eligible.push(row);
    }
    let count = NonZeroUsize::new(source_count)
        .ok_or_else(|| invalid("native funding has no rooted sources"))?;
    let resource_cursor = snapshot
        .resource_cursor()
        .unwrap_or(MonetaryCursor::INITIAL)
        .position_index(count)
        .map_err(invalid)?;
    let fee_cursor = snapshot
        .fee_cursor()
        .unwrap_or(MonetaryCursor::INITIAL)
        .position_index(count)
        .map_err(invalid)?;
    let requirements = [PhloFundingRequirement {
        obligations,
        eligible: &eligible,
    }];
    let total_exposure_limit = snapshot.wallets().authorization().record().total_exposure;
    let selected = select_phlo_funding_family(
        PhloFamilyFundingInput {
            sources: &sources,
            outcomes: &requirements,
            total_exposure_limit,
            canonical_resource_cursor: resource_cursor,
            canonical_fee_cursor: fee_cursor,
        },
        limits.policy,
        budget,
    )
    .map_err(invalid)?
    .ok_or_else(|| invalid("measured funding case has no feasible signed assignment"))?;
    Ok(NativeRootedFamilySelection {
        sources,
        eligible,
        selected,
        total_exposure_limit,
    })
}

#[cfg(test)]
mod tests {
    use models::rhoapi::g_unforgeable::UnfInstance;
    use models::rhoapi::{CostAuthority, GPrivate, GUnforgeable, Par};
    use prost::Message;
    use rholang::rust::interpreter::accounting::authority::{
        authority_demand, canonical_authority, cost_region,
    };

    use super::*;
    use crate::rust::util::rholang::costacc::vault_payer::vault_payer;

    #[test]
    fn forged_ground_alias_does_not_authorize_private_purse() {
        let name = Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: vec![3; 32] })),
        }]);
        let actual = CostSignature {
            value: Some(CostSignatureValue::Name(name.clone())),
        };
        let payer = vault_payer(&actual).unwrap();
        let forged = CostSignature {
            value: Some(CostSignatureValue::Ground(name.encode_to_vec())),
        };
        for (signature, should_accept) in [(actual, true), (forged, false)] {
            let authority = canonical_authority(&CostAuthority {
                regions: vec![cost_region(&signature, b"private purse", 0).unwrap()],
            })
            .unwrap();
            let event = AuthorityEvent {
                event_id: [1; 32],
                debit: authority_demand(&authority).unwrap(),
                authority,
            };
            let observed =
                authority_funding_signatures_for_events_with_host_work(&[event], &[], &[], None)
                    .unwrap();
            assert_eq!(
                verified_onchain_authority(&payer, &observed).is_ok(),
                should_accept
            );
        }
    }
}
