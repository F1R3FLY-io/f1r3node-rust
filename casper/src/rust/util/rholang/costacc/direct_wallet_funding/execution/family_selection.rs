use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::CostSignature;
use models::rust::phlo_resource::PhloAuthorityNode;
use models::rust::phlo_source::PhloSourcePolicyV1;
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use rholang::rust::interpreter::accounting::authority::{
    authority_funding_signatures_for_events, cost_signature_to_sig, AuthorityByteEvent,
    AuthorityEvent,
};
// Changed by dedup (task 11415, D1): `rooted_cursors` names the cursor error.
// use rholang::rust::interpreter::accounting::monetary_allocation::MonetaryCursor;
use rholang::rust::interpreter::accounting::monetary_allocation::{
    MonetaryCursor, MonetaryCursorError,
};
use rholang::rust::interpreter::accounting::phlo_execution::{
    select_phlo_funding_family, CheckedPhloObligations, PhloFamilyFundingInput,
    PhloFamilyFundingSelection, PhloFundingRequirement, PhloFundingSource, PhloObligationKey,
};

use super::producer::NativeOfferedFamilyLimits;
use super::settlement::invalid;
use crate::rust::errors::CasperError;
// Changed by dedup (task 11415, D1): `rooted_source` reads one snapshot source.
// use crate::rust::util::rholang::costacc::direct_wallet_funding::DirectWalletPolicySnapshot;
use crate::rust::util::rholang::costacc::direct_wallet_funding::{
    DirectWalletPolicySnapshot, DirectWalletSource,
};
use crate::rust::util::rholang::costacc::vault_payer::VaultPayer;

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
    match &signature {
        rholang::rust::interpreter::accounting::Sig::Ground(bytes) => Ok(bytes.clone()),
        _ => Err(invalid("on-chain private-name authority is not grounded")),
    }
}

/// Added by G1-4b (DR-123): how a source shows the authority of the resources
/// that it funds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SourceAuthority<'b> {
    /// A source of the signed record. Its permissions alone decide.
    Signed,
    /// An on-chain purse whose private-name authority the run observed. A
    /// resource also needs that ground authority.
    Observed(&'b [u8]),
    /// An on-chain purse before execution. No run has observed its authority,
    /// so it funds the fee only (map gap fill G-5).
    Unobserved,
}

/// Added by G1-4b (DR-123): the permission test of
/// `select_rooted_native_family`, which the gate shares. A resource key needs
/// a signed permission equal to its wire key, and an on-chain purse also
/// needs its observed ground authority.
pub(super) fn source_permits(
    key: &PhloObligationKey<'_>,
    signed: &PhloSourcePolicyV1<'_>,
    authority: SourceAuthority<'_>,
    limits: NativeOfferedFamilyLimits,
) -> Result<bool, CasperError> {
    match key {
        PhloObligationKey::Fee => Ok(signed.fee_permitted()),
        PhloObligationKey::Resource(resource) | PhloObligationKey::RetainedResource(resource) => {
            let observed = match authority {
                SourceAuthority::Signed => None,
                SourceAuthority::Observed(bytes) => Some(bytes),
                SourceAuthority::Unobserved => return Ok(false),
            };
            let wire = resource
                .wire_key(limits.measured.settlement.matching.execution)
                .map_err(invalid)?;
            let mut allowed = false;
            for permission in signed.resources() {
                if permission == &wire
                    && observed
                        .is_none_or(|bytes| wire.authority == [PhloAuthorityNode::Ground(bytes)])
                {
                    allowed = true;
                }
            }
            Ok(allowed)
        }
    }
}

/// Added by dedup (task 11415, D1): why a step of the rooted funding setup
/// fails. `select_rooted_native_family` and the check before execution
/// (`gate.rs`) map each case to their own message.
#[derive(Debug)]
pub(super) enum RootedSetupError {
    /// A snapshot source differs from its signed source.
    SourceDiffers,
    /// A custody key is not 32 bytes.
    CustodyWidth,
    /// The snapshot has no source.
    NoSources,
    /// A canonical cursor does not index the sources.
    Cursor(MonetaryCursorError),
}

/// Added by dedup (task 11415, D1): the source step of the rooted funding
/// setup. It checks one snapshot source against its signed source and returns
/// the source with its custody key. `select_rooted_native_family` and
/// `GateFunding::from_snapshot` (`gate.rs`) call it once per source, each in
/// its own loop.
pub(super) fn rooted_source<'a>(
    physical: &DirectWalletSource<'a>,
    signed: &PhloSourcePolicyV1<'_>,
) -> Result<(PhloFundingSource<'a>, [u8; 32]), RootedSetupError> {
    let source = physical.source();
    if source.custody != signed.custody() {
        return Err(RootedSetupError::SourceDiffers);
    }
    let custody: [u8; 32] = source
        .custody
        .try_into()
        .map_err(|_| RootedSetupError::CustodyWidth)?;
    Ok((source, custody))
}

/// Added by dedup (task 11415, D1): the eligibility step of the rooted funding
/// setup. It appends one permission test for each obligation key, in key
/// order, to the row of one source. The authority mode of the source is the
/// parameter. After execution, `select_rooted_native_family` passes the
/// signed or the observed authority. Before execution, the check of `gate.rs`
/// (`eligibility`) passes the signed authority or no observed authority (map
/// gap fill G-5). The caller allocates the row. The step returns the first
/// error of `source_permits`.
pub(super) fn eligibility_row(
    row: &mut Vec<bool>,
    obligations: &CheckedPhloObligations<'_>,
    signed: &PhloSourcePolicyV1<'_>,
    authority: SourceAuthority<'_>,
    limits: NativeOfferedFamilyLimits,
) -> Result<(), CasperError> {
    for key in obligations.keys() {
        row.push(source_permits(key, signed, authority, limits)?);
    }
    Ok(())
}

/// Added by dedup (task 11415, D1): the cursor step of the rooted funding
/// setup. It returns the canonical resource cursor and then the canonical fee
/// cursor of the snapshot as source positions. An absent cursor is the
/// initial cursor.
pub(super) fn rooted_cursors(
    snapshot: &DirectWalletPolicySnapshot<'_, OfferedFundedDeploy>,
    source_count: usize,
) -> Result<(usize, usize), RootedSetupError> {
    let count = NonZeroUsize::new(source_count).ok_or(RootedSetupError::NoSources)?;
    let position = |cursor: Option<MonetaryCursor>| {
        cursor
            .unwrap_or(MonetaryCursor::INITIAL)
            .position_index(count)
            .map_err(RootedSetupError::Cursor)
    };
    let resource = position(snapshot.resource_cursor())?;
    let fee = position(snapshot.fee_cursor())?;
    Ok((resource, fee))
}

pub(crate) fn select_rooted_native_family<'a>(
    snapshot: &DirectWalletPolicySnapshot<'a, OfferedFundedDeploy>,
    obligations: &CheckedPhloObligations<'_>,
    authority_events: &[AuthorityEvent<[u8; 32]>],
    authority_byte_events: &[AuthorityByteEvent],
    limits: NativeOfferedFamilyLimits,
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
            authority_funding_signatures_for_events(authority_events, authority_byte_events, &[])
                .map_err(invalid)?,
        )
    };
    let source_count = snapshot.wallets().sources().len();
    let columns = obligations.keys().len();
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
    // Added by dedup (task 11415, D1): the messages of this selection for the
    // shared steps of the rooted funding setup.
    let setup_error = |error: RootedSetupError| match error {
        RootedSetupError::SourceDiffers => {
            invalid("signed funding source differs from rooted snapshot")
        }
        RootedSetupError::CustodyWidth => invalid("invalid on-chain custody width"),
        RootedSetupError::NoSources => invalid("native funding has no rooted sources"),
        RootedSetupError::Cursor(error) => invalid(error),
    };
    for (physical, signed) in snapshot.wallets().sources().iter().zip(signed_sources) {
        // Moved by dedup (task 11415) to `rooted_source` in
        // `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/family_selection.rs`.
        // let source = physical.source();
        // if source.custody != signed.custody() {
        //     return Err(invalid(
        //         "signed funding source differs from rooted snapshot",
        //     ));
        // }
        // let custody: [u8; 32] = source
        //     .custody
        //     .try_into()
        //     .map_err(|_| invalid("invalid on-chain custody width"))?;
        let (source, custody) = rooted_source(physical, signed).map_err(setup_error)?;
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
        // Changed by G1-4b (DR-123): the permission test moved to
        // `source_permits`, which the gate shares. The replaced lines follow.
        // for key in obligations.keys() {
        //     let permitted = match key {
        //         PhloObligationKey::Fee => signed.fee_permitted(),
        //         PhloObligationKey::Resource(resource)
        //         | PhloObligationKey::RetainedResource(resource) => {
        //             let wire = resource
        //                 .wire_key(limits.measured.settlement.matching.execution)
        //                 .map_err(invalid)?;
        //             let mut allowed = false;
        //             for permission in signed.resources() {
        //                 reserve(budget, HostWorkDimension::VerificationOperations, 1)?;
        //                 if permission == &wire
        //                     && onchain_authority.as_ref().is_none_or(|bytes| {
        //                         wire.authority == [PhloAuthorityNode::Ground(bytes.as_slice())]
        //                     })
        //                 {
        //                     allowed = true;
        //                 }
        //             }
        //             allowed
        //         }
        //     };
        //     row.push(permitted);
        // }
        let authority = match onchain_authority.as_deref() {
            Some(bytes) => SourceAuthority::Observed(bytes),
            None => SourceAuthority::Signed,
        };
        // Moved by dedup (task 11415) to `eligibility_row` in
        // `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/family_selection.rs`.
        // for key in obligations.keys() {
        //     row.push(source_permits(key, signed, authority, limits, budget)?);
        // }
        eligibility_row(&mut row, obligations, signed, authority, limits)?;
        eligible.push(row);
    }
    // Moved by dedup (task 11415) to `rooted_cursors` in
    // `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/family_selection.rs`.
    // let count = NonZeroUsize::new(source_count)
    //     .ok_or_else(|| invalid("native funding has no rooted sources"))?;
    // let resource_cursor = snapshot
    //     .resource_cursor()
    //     .unwrap_or(MonetaryCursor::INITIAL)
    //     .position_index(count)
    //     .map_err(invalid)?;
    // let fee_cursor = snapshot
    //     .fee_cursor()
    //     .unwrap_or(MonetaryCursor::INITIAL)
    //     .position_index(count)
    //     .map_err(invalid)?;
    let (resource_cursor, fee_cursor) =
        rooted_cursors(snapshot, source_count).map_err(setup_error)?;
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
            let observed = authority_funding_signatures_for_events(&[event], &[], &[]).unwrap();
            assert_eq!(
                verified_onchain_authority(&payer, &observed).is_ok(),
                should_accept
            );
        }
    }

    /// Added by G1-4b (DR-123): the permission test that
    /// `select_rooted_native_family` computed inline before the refactor,
    /// kept as the oracle of `source_permits`.
    fn inline_permission(
        key: &PhloObligationKey<'_>,
        signed: &PhloSourcePolicyV1<'_>,
        onchain_authority: Option<&Vec<u8>>,
        limits: NativeOfferedFamilyLimits,
    ) -> Result<bool, CasperError> {
        Ok(match key {
            PhloObligationKey::Fee => signed.fee_permitted(),
            PhloObligationKey::Resource(resource)
            | PhloObligationKey::RetainedResource(resource) => {
                let wire = resource
                    .wire_key(limits.measured.settlement.matching.execution)
                    .map_err(invalid)?;
                let mut allowed = false;
                for permission in signed.resources() {
                    if permission == &wire
                        && onchain_authority.is_none_or(|bytes| {
                            wire.authority == [PhloAuthorityNode::Ground(bytes.as_slice())]
                        })
                    {
                        allowed = true;
                    }
                }
                allowed
            }
        })
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

        /// Added by G1-4b (DR-123): `source_permits` decides every key as the
        /// inline test did, for signed sources and for on-chain purses with an
        /// observed authority. An on-chain purse without an observed authority
        /// funds the fee only.
        #[test]
        fn source_permits_decides_as_the_inline_test_did(
            permitted in proptest::collection::btree_set((0u8..3, 0usize..3), 0..5),
            asked in (0u8..3, 0usize..3),
            kind in 0u8..3,
            fee in proptest::prelude::any::<bool>(),
            observed in proptest::option::of(0u8..3),
        ) {
            use models::rust::phlo_source::PhloSourceLimits;
            use models::rust::phlo_wire::PhloWireLimits;
            use rholang::rust::interpreter::accounting::phlo_execution::{
                PhloResource, PhloSourceConsent,
            };
            use rholang::rust::interpreter::accounting::{Sig, SignatureChannel};

            use crate::rust::util::rholang::costacc::production_limits::offered_funded_v6_production_limits;

            let limits = offered_funded_v6_production_limits().family;
            let terms = b"selected terms".to_vec();
            let authorities = (0u8..3).map(|tag| Sig::Ground(vec![tag + 1; 4])).collect::<Vec<_>>();
            let locations = authorities
                .iter()
                .map(|authority| SignatureChannel::from_sig(authority).par.encode_to_vec())
                .collect::<Vec<_>>();
            let resource = |(tag, class): (u8, usize)| PhloResource {
                location: &locations[usize::from(tag)],
                class,
                acquisition_terms: &terms,
                authority: &authorities[usize::from(tag)],
            };
            let permissions = permitted.iter().copied().map(resource).collect::<Vec<_>>();
            let custody = [5u8; 32];
            let signed = PhloSourceConsent {
                custody: &custody,
                hold_cap: 10,
                debit_cap: 10,
                fee_permitted: fee,
                resources: &permissions,
            }
            .wire_policy(limits.measured.settlement.matching.execution, PhloSourceLimits {
                wire: PhloWireLimits {
                    total_bytes: 1_048_576,
                    field_bytes: 524_288,
                },
                resource_permissions: 16,
                authority_nodes: 65_536,
            })
            .expect("the source policy encodes");
            let key = match kind {
                0 => PhloObligationKey::Fee,
                1 => PhloObligationKey::Resource(resource(asked)),
                _ => PhloObligationKey::RetainedResource(resource(asked)),
            };
            let observed = observed.map(|tag| vec![tag + 1; 4]);
            let authority = match observed.as_ref() {
                Some(bytes) => SourceAuthority::Observed(bytes),
                None => SourceAuthority::Signed,
            };
            let decided = source_permits(&key, &signed, authority, limits)
                .expect("the refactored test decides");
            let expected = inline_permission(&key, &signed, observed.as_ref(), limits)
                .expect("the inline test decides");
            proptest::prop_assert_eq!(decided, expected);
            let before_execution = source_permits(&key, &signed, SourceAuthority::Unobserved, limits)
                .expect("the pre-execution test decides");
            proptest::prop_assert_eq!(before_execution, kind == 0 && fee);
        }
    }
}
