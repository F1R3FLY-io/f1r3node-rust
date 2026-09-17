use std::collections::BTreeSet;
use std::num::NonZeroUsize;

use casper::rust::util::rholang::costacc::direct_wallet_funding::{
    authorize_direct_wallet_funding, authorize_offered_direct_wallet_funding,
    DirectWalletFundingError, DirectWalletFundingLimits,
};
use casper::rust::util::rholang::costacc::vault_payer::vault_payer;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::secp256k1_eth::Secp256k1Eth;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::{Cosigned, Cosigner, ToMessage};
use models::rhoapi::cost_signature::Value;
use models::rhoapi::CostSignature;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::phlo_controls::{PhloControlsLimits, PhloControlsV1};
use models::rust::phlo_intent::{PhloFundingIntentLimits, PhloFundingIntentV1};
use models::rust::phlo_schedule::{PhloResourceClassV1, PhloScheduleV1};
use models::rust::phlo_source::{PhloSourceLimits, PhloSourcePolicyV1};
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::signed_phlo_deploy::{FundedDeploy, FundedDeployLimits, OfferedFundedDeploy};
use proptest::prelude::*;

#[path = "direct_wallet_funding/snapshot_tests.rs"]
mod snapshot_tests;

#[path = "direct_wallet_funding/offered_tests.rs"]
mod offered_tests;

fn limits() -> FundedDeployLimits {
    let wire = PhloWireLimits {
        total_bytes: 1_048_576,
        field_bytes: 524_288,
    };
    FundedDeployLimits {
        deploy_bytes: 2_097_152,
        signing: wire,
        funding: PhloFundingIntentLimits {
            wire,
            controls: PhloControlsLimits {
                wire,
                owners: 256,
                schedules: 1,
                total_classes: 1,
            },
            sources: 256,
            resource_permissions: 0,
            authority_nodes: 0,
        },
    }
}

fn authorization_limits() -> DirectWalletFundingLimits {
    DirectWalletFundingLimits {
        members: NonZeroUsize::new(64).unwrap(),
        funding: limits().funding,
    }
}

fn key(index: usize) -> PrivateKey { PrivateKey::from_bytes(&[(index + 1) as u8; 32]) }

fn custody(index: usize) -> Vec<u8> {
    let public_key = Secp256k1.to_public(&key(index));
    vault_payer(&CostSignature {
        value: Some(Value::Ground(public_key.bytes.to_vec())),
    })
    .unwrap()
    .custody_key
    .to_vec()
}

fn data(requested: &[Vec<u8>]) -> FundedDeploy {
    let controls = PhloControlsV1 {
        limit: 10,
        price_ceiling: 3,
        required_owner_ceilings: vec![3],
        permitted_schedules: vec![PhloScheduleV1 {
            protocol_version: 6,
            network: b"test",
            shard: b"root",
            settlement_asset: b"REV",
            settlement_unit: b"smallest REV unit",
            decimal_scale: 8,
            classes: vec![PhloResourceClassV1 {
                identity: b"compute",
                measurement_unit: b"phlo",
                measurement_rule: [1; 32],
                valuation_rule: [2; 32],
                weight: 1,
            }],
            actual_price: 1,
            compatibility_rule: [3; 32],
        }],
    };
    let intent = PhloFundingIntentV1 {
        schedule_commitment: controls.permitted_schedules[0]
            .digest(limits().funding.controls().schedule(1))
            .unwrap(),
        controls,
        total_exposure: 30,
        sources: requested
            .iter()
            .map(|identity| {
                PhloSourcePolicyV1::new(identity, 100, 100, true, vec![], PhloSourceLimits {
                    wire: limits().funding.wire,
                    resource_permissions: 0,
                    authority_nodes: 0,
                })
                .unwrap()
            })
            .collect(),
    };
    FundedDeploy::new(
        DeployData {
            term: "Nil".to_string(),
            language: "rholang".to_string(),
            time_stamp: 1,
            valid_after_block_number: 0,
            shard_id: "root".to_string(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        },
        intent.encode(limits().funding).unwrap(),
        limits(),
    )
    .unwrap()
}

fn envelope(selected: &[bool], requested: &[Vec<u8>]) -> Cosigned<FundedDeploy> {
    sign_data(selected, data(requested))
}

fn offered_envelope(
    selected: &[bool],
    requested: &[Vec<u8>],
    limit: i64,
    price: i64,
) -> Cosigned<OfferedFundedDeploy> {
    let original = data(requested);
    sign_data(
        selected,
        OfferedFundedDeploy::new(
            original.body().clone(),
            original.funding_intent().to_vec(),
            limit,
            price,
            limits(),
        )
        .unwrap(),
    )
}

fn sign_data<A: std::fmt::Debug + serde::Serialize + ToMessage>(
    selected: &[bool],
    data: A,
) -> Cosigned<A> {
    let mut members: Vec<_> = selected
        .iter()
        .enumerate()
        .map(|(index, selected)| {
            (
                Cosigner {
                    pk: Secp256k1.to_public(&key(index)),
                    sig: Vec::new().into(),
                    sig_algorithm: Box::new(Secp256k1),
                },
                index,
                *selected,
            )
        })
        .collect();
    members.sort_by(|left, right| left.0.pk.bytes.cmp(&right.0.pk.bytes));
    let mut signers: Vec<_> = members.iter().map(|member| member.0.clone()).collect();
    let mut bitmap = vec![0; selected.len().div_ceil(8)];
    for (index, member) in members.iter().enumerate() {
        if member.2 {
            bitmap[index / 8] |= 1 << (index % 8);
        }
    }
    let threshold = selected.iter().filter(|present| **present).count() as u32;
    let hash = Cosigned::envelope_signing_hash_for_presence(
        &data,
        &signers,
        threshold,
        &bitmap,
        "secp256k1",
    )
    .unwrap();
    for (index, member) in members.iter().enumerate() {
        if member.2 {
            signers[index].sig = Secp256k1.sign(&hash, &key(member.1).bytes).into();
        }
    }
    Cosigned::from_envelope_signed_data_threshold(data, signers, threshold).unwrap()
}

#[test]
fn every_configured_direct_wallet_arity_uses_native_custody() {
    for count in 1..=64 {
        let requested: Vec<_> = (0..count).map(custody).collect();
        let signed = envelope(&vec![true; count], &requested);
        let checked = authorize_direct_wallet_funding(&signed, authorization_limits()).unwrap();
        assert!(std::ptr::eq(checked.envelope(), &signed));
        assert_eq!(checked.record().sources.len(), count);
        assert_eq!(checked.payers().len(), count);
        for requested in &requested {
            let key: [u8; 32] = requested.as_slice().try_into().unwrap();
            assert_eq!(checked.payers()[&key].custody_key, key);
        }
    }
}

#[test]
fn unsigned_threshold_members_and_unrelated_wallets_cannot_fund() {
    for index in [1, 3] {
        let signed = envelope(&[true, false, true], &[custody(index)]);
        assert!(matches!(
            authorize_direct_wallet_funding(&signed, authorization_limits()),
            Err(DirectWalletFundingError::UnwitnessedCustody { index: 0 })
        ));
    }
    let signed = envelope(&[true, false, true], &[custody(0), custody(2)]);
    assert_eq!(
        authorize_direct_wallet_funding(&signed, authorization_limits())
            .unwrap()
            .payers()
            .len(),
        2
    );
}

#[test]
fn source_and_member_caps_apply_before_any_authorization_result() {
    let signed = envelope(&[true, false, false], &[custody(0)]);
    let mut bounded = authorization_limits();
    bounded.members = NonZeroUsize::new(2).unwrap();
    assert!(matches!(
        authorize_direct_wallet_funding(&signed, bounded),
        Err(DirectWalletFundingError::TooManyMembers)
    ));
    bounded = authorization_limits();
    bounded.funding.sources = 0;
    assert!(matches!(
        authorize_direct_wallet_funding(&signed, bounded),
        Err(DirectWalletFundingError::Record(_))
    ));
    let signed = envelope(&[true; 65], &[custody(0)]);
    assert!(matches!(
        authorize_direct_wallet_funding(&signed, authorization_limits()),
        Err(DirectWalletFundingError::TooManyMembers)
    ));
    bounded = authorization_limits();
    bounded.members = NonZeroUsize::new(65).unwrap();
    assert!(authorize_direct_wallet_funding(&signed, bounded).is_ok());
}

#[test]
fn malformed_custody_and_stale_body_signatures_reject() {
    for length in [1, 31, 33, 64] {
        let signed = envelope(&[true], &[vec![0; length]]);
        assert!(matches!(
            authorize_direct_wallet_funding(&signed, authorization_limits()),
            Err(DirectWalletFundingError::MalformedCustody { index: 0 })
        ));
    }
    let mut signed = envelope(&[true], &[custody(0)]);
    let mut body = signed.data.body().clone();
    body.time_stamp += 1;
    signed.data = FundedDeploy::new(body, signed.data.funding_intent().to_vec(), limits()).unwrap();
    assert!(matches!(
        authorize_direct_wallet_funding(&signed, authorization_limits()),
        Err(DirectWalletFundingError::Signature(_))
    ));
}

#[test]
fn ethereum_signatures_resolve_the_same_native_wallet() {
    let requested = [custody(0)];
    let signed =
        Cosigned::create_single_envelope(data(&requested), Box::new(Secp256k1Eth), key(0)).unwrap();
    let checked = authorize_direct_wallet_funding(&signed, authorization_limits()).unwrap();
    assert_eq!(checked.payers().len(), 1);
    assert_eq!(
        checked
            .payers()
            .values()
            .next()
            .unwrap()
            .custody_key
            .to_vec(),
        requested[0]
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn authorization_equals_requested_subset_of_selected_native_wallets(
        count in 1usize..=12,
        presence in any::<u16>(),
        requested in prop::collection::btree_set(0usize..=12, 0..=13),
    ) {
        let selected: Vec<_> = (0..count).map(|index| index == 0 || presence & (1 << index) != 0).collect();
        let indices: BTreeSet<_> = requested.into_iter().map(|index| index % (count + 1)).collect();
        let expected = indices.iter().all(|index| *index < count && selected[*index]);
        let mut sources: Vec<_> = indices.iter().copied().map(custody).collect();
        let signed = envelope(&selected, &sources);
        let first = authorize_direct_wallet_funding(&signed, authorization_limits());
        prop_assert_eq!(first.is_ok(), expected);
        sources.reverse();
        let reordered = envelope(&selected, &sources);
        let second = authorize_direct_wallet_funding(&reordered, authorization_limits());
        prop_assert_eq!(second.is_ok(), expected);
        if let (Ok(first), Ok(second)) = (first, second) {
            prop_assert_eq!(first.payers(), second.payers());
            prop_assert_eq!(first.payers().len(), indices.len());
        }
    }
}

#[tokio::test]
async fn direct_wallet_authority_reaches_signed_family_and_canonical_capture() {
    for count in [1, 3, 64] {
        check_bound_wallet_funding(vec![100; count]).await;
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn signed_snapshot_binding_preserves_generated_rows(
        balances in prop::collection::vec(2_i64..100_000, 1..=12),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        runtime.block_on(check_bound_wallet_funding(balances));
    }
}

async fn check_bound_wallet_funding(balances: Vec<i64>) {
    use casper::rust::util::rholang::costacc::direct_wallet_funding::DirectWalletBindingError;
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
    use models::rust::phlo_obligation::PhloObligationKeyLimits;
    use rholang::rust::interpreter::accounting::monetary_allocation::FundingSearchLimits;
    use rholang::rust::interpreter::accounting::phlo_controls::{
        check_phlo_controls, PhloFundingTerms,
    };
    use rholang::rust::interpreter::accounting::phlo_execution::{
        check_phlo_execution, check_phlo_funding_family, project_phlo_obligations,
        PhloCaptureLimits, PhloConsentLimits, PhloExecutionLimits, PhloExecutionWitness,
        PhloFundingCase, PhloFundingIntentBinding, PhloFundingLimits, PhloFundingSource,
        PhloOutcome, SignedPhloConsentError, SignedPhloConsentLimits,
    };
    use rholang::rust::interpreter::host_work::HostWorkBudget;

    let cap = |value| NonZeroUsize::new(value).unwrap();
    let count = balances.len();
    let requested: Vec<_> = (0..count).map(custody).collect();
    let signed = envelope(&vec![true; count], &requested);
    let authorized = authorize_direct_wallet_funding(&signed, authorization_limits()).unwrap();
    let reader = snapshot_tests::Reader::new(balances.into_iter().map(Some).collect());
    let snapshot = authorized
        .read_snapshot(&reader, [1; 32], cap(2))
        .await
        .unwrap();
    let offered_signed = offered_envelope(&vec![true; count], &requested, 10, 1);
    let offered_authorized =
        authorize_offered_direct_wallet_funding(&offered_signed, authorization_limits()).unwrap();
    let offered_snapshot = offered_authorized
        .read_snapshot(&reader, [1; 32], cap(2))
        .await
        .unwrap();
    assert_eq!(offered_snapshot.sources(), snapshot.sources());
    let authorized = snapshot.authorization();
    let binding = PhloFundingIntentBinding::new(authorized.record(), limits().funding).unwrap();
    let view = binding.view().unwrap();
    let terms = view.controls();
    let schedule = terms.permitted_schedules[0];
    let controls =
        check_phlo_controls(schedule.environment, 1, i64::MAX as u64, terms, schedule, 0).unwrap();
    let execution = check_phlo_execution(
        controls,
        PhloExecutionWitness {
            available: &[],
            required: &[],
            used: &[],
            unused: &[],
            fresh: &[],
        },
        PhloExecutionLimits {
            resource_entries: 1,
            authority_nodes: 1,
            key_bytes: 1024,
        },
    )
    .unwrap();
    let obligations =
        project_phlo_obligations(execution, PhloOutcome::Accepted(&[]), cap(1)).unwrap();
    let sources: Vec<_> = snapshot
        .sources()
        .iter()
        .map(|source| source.source())
        .collect();
    let eligible = vec![vec![true]; count];
    let mut assignment = vec![vec![0]; count];
    assignment[0][0] = 1;
    let cases = [PhloFundingCase {
        obligations: &obligations,
        eligible: &eligible,
        assignment: &assignment,
    }];
    let family_limits = PhloFundingLimits {
        sources: cap(64),
        cases: cap(1),
        obligations: cap(1),
        assignment_cells: 1024,
        custody_bytes: 4096,
    };
    let family = check_phlo_funding_family(&sources, &cases, 30, family_limits).unwrap();
    let funding_terms = PhloFundingTerms {
        required_owner_ceilings: &authorized.record().controls.required_owner_ceilings,
        asset: schedule.environment.asset,
        schedule_commitment: schedule.commitment,
    };
    let consent_limits = SignedPhloConsentLimits {
        members: cap(64),
        intent: limits().funding,
        consent: PhloConsentLimits {
            sources: 64,
            permission_entries: 64,
            case_cells: 1024,
            authority_nodes: 1024,
            key_bytes: 4096,
        },
    };
    let checked = snapshot
        .bind_family(&view, &family, funding_terms, consent_limits)
        .unwrap();
    assert!(std::ptr::eq(checked.snapshot(), &snapshot));
    let offered_bound = offered_snapshot
        .bind_family(&view, &family, funding_terms, consent_limits)
        .unwrap();
    assert!(std::ptr::eq(
        offered_bound.consent().envelope(),
        &offered_signed
    ));
    assert!(std::ptr::eq(offered_bound.snapshot(), &offered_snapshot));
    let captured = checked
        .consent()
        .intent()
        .capture_case(
            0,
            PhloCaptureLimits {
                funding: FundingSearchLimits {
                    source_cap: cap(64),
                    obligation_cap: cap(1),
                },
                key: PhloObligationKeyLimits {
                    wire: limits().funding.wire,
                    authority_nodes: 1,
                },
                aggregate_key_bytes: 4096,
            },
            &HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000))),
        )
        .unwrap();
    assert_eq!(captured.amounts(), &[1]);
    assert_eq!(
        captured
            .sources()
            .iter()
            .map(|source| source.debit())
            .sum::<u64>(),
        1
    );
    for source in captured.sources() {
        let custody: [u8; 32] = source.source().custody.try_into().unwrap();
        assert!(authorized.payers().contains_key(&custody));
        assert_eq!(source.acquisition(), 0);
        assert_eq!(source.hold(), source.debit() + source.refund());
    }
    for index in 0..count {
        for field in 0..5 {
            let mut changed = sources.clone();
            match field {
                0 => changed[index].capacity += 1,
                1 => changed[index].capacity -= 1,
                2 => changed[index].exposure_limit -= 1,
                3 => changed[index].debit_limit -= 1,
                _ => changed[index].custody = &[0x88; 32],
            }
            let family = check_phlo_funding_family(&changed, &cases, 30, family_limits).unwrap();
            assert!(matches!(
                snapshot.bind_family(&view, &family, funding_terms, consent_limits),
                Err(DirectWalletBindingError::SourcesMismatch)
            ));
            assert!(matches!(
                offered_snapshot.bind_family(&view, &family, funding_terms, consent_limits),
                Err(DirectWalletBindingError::SourcesMismatch)
            ));
        }
    }
    let reordered: Vec<_> = sources.iter().copied().rev().collect();
    let family = check_phlo_funding_family(&reordered, &cases, 30, family_limits).unwrap();
    snapshot
        .bind_family(&view, &family, funding_terms, consent_limits)
        .unwrap();
    if count > 1 {
        let smaller_cases = [PhloFundingCase {
            obligations: &obligations,
            eligible: &eligible[..count - 1],
            assignment: &assignment[..count - 1],
        }];
        let smaller =
            check_phlo_funding_family(&sources[..count - 1], &smaller_cases, 30, family_limits)
                .unwrap();
        assert!(matches!(
            snapshot.bind_family(&view, &smaller, funding_terms, consent_limits),
            Err(DirectWalletBindingError::SourcesMismatch)
        ));
    }
    let mut extra_sources = sources.clone();
    extra_sources.push(PhloFundingSource {
        custody: &[0x88; 32],
        ..sources[0]
    });
    let mut extra_assignment = assignment.clone();
    extra_assignment.push(vec![0]);
    let extra_eligible = vec![vec![true]; count + 1];
    let extra_cases = [PhloFundingCase {
        obligations: &obligations,
        eligible: &extra_eligible,
        assignment: &extra_assignment,
    }];
    let extra = check_phlo_funding_family(&extra_sources, &extra_cases, 30, PhloFundingLimits {
        sources: cap(count + 1),
        ..family_limits
    })
    .unwrap();
    assert!(matches!(
        snapshot.bind_family(&view, &extra, funding_terms, consent_limits),
        Err(DirectWalletBindingError::SourcesMismatch)
    ));
    let mut different_record = authorized.record().clone();
    different_record.total_exposure += 1;
    let different_binding =
        PhloFundingIntentBinding::new(&different_record, limits().funding).unwrap();
    let different_view = different_binding.view().unwrap();
    assert!(matches!(
        snapshot.bind_family(&different_view, &family, funding_terms, consent_limits),
        Err(DirectWalletBindingError::Consent(
            SignedPhloConsentError::RecordMismatch
        ))
    ));
}
