use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::{Cosigned, Cosigner, ToMessage};
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::phlo_controls::{PhloControlsLimits, PhloControlsV1};
use models::rust::phlo_intent::{PhloFundingIntentLimits, PhloFundingIntentV1};
use models::rust::phlo_schedule::{PhloResourceClassV1, PhloScheduleV1};
use models::rust::phlo_wire::PhloWireLimits;
use models::rust::signed_phlo_deploy::{FundedDeploy, FundedDeployLimits, OfferedFundedDeploy};

use super::*;
use crate::rust::interpreter::accounting::phlo_controls::{
    PhloFundingTerms, PhloFundingTermsError,
};
use crate::rust::interpreter::accounting::phlo_execution::{
    PhloFundingIntentBinding, PhloFundingIntentCheckError, SignedPhloConsentError,
    SignedPhloConsentLimits,
};

fn threshold_envelope<A: std::fmt::Debug + serde::Serialize + ToMessage>(
    data: A,
    count: usize,
) -> Cosigned<A> {
    let mut members: Vec<_> = (1..=count)
        .map(|index| {
            let key = PrivateKey::from_bytes(&[index as u8; 32]);
            (
                Cosigner {
                    pk: Secp256k1.to_public(&key),
                    sig: Vec::new().into(),
                    sig_algorithm: Box::new(Secp256k1),
                },
                key,
            )
        })
        .collect();
    members.sort_by(|left, right| left.0.pk.bytes.cmp(&right.0.pk.bytes));
    let mut signers: Vec<_> = members.iter().map(|(signer, _)| signer.clone()).collect();
    let selected = count.div_ceil(2);
    let mut bitmap = vec![0; count.div_ceil(8)];
    for index in 0..selected {
        bitmap[index / 8] |= 1 << (index % 8);
    }
    let hash = Cosigned::envelope_signing_hash_for_presence(
        &data,
        &signers,
        selected as u32,
        &bitmap,
        "secp256k1",
    )
    .unwrap();
    for index in 0..selected {
        signers[index].sig = Secp256k1.sign(&hash, &members[index].1.bytes).into();
    }
    Cosigned::from_envelope_signed_data_threshold(data, signers, selected as u32).unwrap()
}

#[test]
fn signed_native_family_checks_maximum_holds_before_selecting_charged_or_zero_branches() {
    use models::rust::host_work::{HostWorkLimit, HostWorkLimits};
    use models::rust::phlo_obligation::PhloObligationKeyLimits;

    use crate::rust::interpreter::accounting::monetary_allocation::{
        FundingSearchLimits, MonetaryCursor,
    };
    use crate::rust::interpreter::accounting::phlo_execution::{
        NativePhloAmountError, NativePhloPolicyError, PhloCaptureLimits, PhloFamilyCursorSnapshot,
        PhloFamilyFundingLimits, PhloScopedCursorSnapshot,
    };
    use crate::rust::interpreter::host_work::HostWorkBudget;

    let cap = |value| NonZeroUsize::new(value).unwrap();
    let budget = || HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)));
    let work = PhloExecutionLimits {
        resource_entries: 4,
        authority_nodes: 1024,
        key_bytes: 1_048_576,
    };
    let wire = PhloWireLimits {
        total_bytes: 1_048_576,
        field_bytes: 524_288,
    };
    let controls_limit = PhloControlsLimits {
        wire,
        owners: 1,
        schedules: 1,
        total_classes: 1,
    };
    let intent_limits = PhloFundingIntentLimits {
        wire,
        controls: controls_limit,
        sources: 1,
        resource_permissions: 1,
        authority_nodes: 1024,
    };
    let source_limits = PhloSourceLimits {
        wire,
        resource_permissions: 1,
        authority_nodes: 1024,
    };
    let family_limits = PhloFundingLimits {
        sources: cap(1),
        cases: cap(2),
        obligations: cap(2),
        assignment_cells: 4,
        custody_bytes: 64,
    };
    let policy_limits = PhloFamilyFundingLimits {
        funding: family_limits,
        keys: PhloObligationKeyLimits {
            wire,
            authority_nodes: 1024,
        },
        aggregate_key_bytes: 1_048_576,
    };
    let capture_limits = PhloCaptureLimits {
        funding: FundingSearchLimits {
            source_cap: cap(1),
            obligation_cap: cap(2),
        },
        key: policy_limits.keys,
        aggregate_key_bytes: policy_limits.aggregate_key_bytes,
    };
    let authority = Sig::Ground(vec![1]);
    let resources = [resource(&authority)];
    for hold in [i64::MAX as u64, i64::MAX as u64 + 1] {
        let cost = hold - 1;
        let descriptor = PhloControlsV1 {
            limit: cost,
            price_ceiling: 1,
            required_owner_ceilings: vec![1],
            permitted_schedules: vec![PhloScheduleV1 {
                protocol_version: ENV.protocol_version,
                network: ENV.network,
                shard: ENV.shard,
                settlement_asset: ENV.asset,
                settlement_unit: ENV.unit,
                decimal_scale: ENV.decimal_scale,
                classes: vec![PhloResourceClassV1 {
                    identity: b"COMM",
                    measurement_unit: b"authority demand",
                    measurement_rule: [2; 32],
                    valuation_rule: [3; 32],
                    weight: cost,
                }],
                actual_price: 1,
                compatibility_rule: [4; 32],
            }],
        };
        let record = PhloFundingIntentV1 {
            schedule_commitment: descriptor.permitted_schedules[0]
                .digest(controls_limit.schedule(1))
                .unwrap(),
            controls: descriptor,
            total_exposure: u128::from(hold),
            sources: vec![policy(b"wallet", hold, true, &resources)
                .wire_policy(work, source_limits)
                .unwrap()],
        };
        let bytes = record.encode(intent_limits).unwrap();
        let decoded = PhloFundingIntentV1::decode(&bytes, intent_limits).unwrap();
        let binding = PhloFundingIntentBinding::new(&decoded, intent_limits).unwrap();
        let view = binding.view().unwrap();
        let selected = view.controls().permitted_schedules[0];
        let checked =
            check_phlo_controls(ENV, 0, u64::MAX, view.controls(), selected, cost).unwrap();
        let execution = check_phlo_execution(
            checked,
            PhloExecutionWitness {
                available: &[],
                required: &resources,
                used: &[],
                unused: &[],
                fresh: &resources,
            },
            work,
        )
        .unwrap();
        let charged =
            project_phlo_obligations(execution, PhloOutcome::Accepted(&[]), cap(2)).unwrap();
        let rejected = project_phlo_obligations(
            execution,
            PhloOutcome::Accepted(&[PhloFailure::Platform]),
            cap(2),
        )
        .unwrap();
        let sources = [PhloFundingSource {
            custody: b"wallet",
            capacity: u64::MAX,
            exposure_limit: hold,
            debit_limit: hold,
        }];
        let eligibility = [vec![true, true]];
        let allocation = [vec![1, cost]];
        let zero = [vec![0, 0]];
        let cases = [
            PhloFundingCase {
                obligations: &charged,
                eligible: &eligibility,
                assignment: &allocation,
            },
            PhloFundingCase {
                obligations: &rejected,
                eligible: &eligibility,
                assignment: &zero,
            },
        ];
        let family =
            check_phlo_funding_family(&sources, &cases, u128::from(hold), family_limits).unwrap();
        let body = DeployData {
            term: "Nil".to_string(),
            language: "rholang".to_string(),
            time_stamp: 1,
            valid_after_block_number: 0,
            shard_id: "shard".to_string(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        };
        let envelope = threshold_envelope(
            FundedDeploy::new(body, bytes.clone(), FundedDeployLimits {
                deploy_bytes: 2_097_152,
                signing: wire,
                funding: intent_limits,
            })
            .unwrap(),
            1,
        );
        let signed = view
            .check_signed_family(
                &envelope,
                &family,
                PhloFundingTerms {
                    required_owner_ceilings: &record.controls.required_owner_ceilings,
                    asset: ENV.asset,
                    schedule_commitment: selected.commitment,
                },
                SignedPhloConsentLimits {
                    members: cap(1),
                    intent: intent_limits,
                    consent: limits(),
                },
            )
            .unwrap();
        let custody: [&[u8]; 1] = [b"wallet"];
        let policy = signed
            .plan_funding_policy(
                PhloFamilyCursorSnapshot {
                    canonical_custodies: &custody,
                    resource: PhloScopedCursorSnapshot {
                        scope: [31; 32],
                        cursor: MonetaryCursor::INITIAL,
                    },
                    fee: PhloScopedCursorSnapshot {
                        scope: [42; 32],
                        cursor: MonetaryCursor::INITIAL,
                    },
                },
                policy_limits,
                &budget(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(policy.selection().source_holds(), &[hold]);
        if hold == i64::MAX as u64 {
            let native = policy.into_native(&budget()).unwrap();
            for branch in 0..2 {
                let capture = native
                    .capture_case(branch, capture_limits, &budget())
                    .unwrap();
                let amounts = &capture.amounts()[0];
                assert_eq!(amounts.hold(), i64::MAX);
                assert_eq!(amounts.custody(), b"wallet");
                assert_eq!(
                    amounts.hold(),
                    amounts.acquisition() + amounts.fee() + amounts.refund()
                );
                assert_eq!(
                    capture.scoped().capture().sources()[0].source().capacity,
                    u64::MAX
                );
                if branch == 0 {
                    assert_eq!(
                        (amounts.acquisition(), amounts.fee(), amounts.refund()),
                        (i64::MAX - 1, 1, 0)
                    );
                    assert!(capture.scoped().resource_transition().is_some());
                    assert!(capture.scoped().fee_transition().is_some());
                } else {
                    assert_eq!(
                        (amounts.acquisition(), amounts.fee(), amounts.refund()),
                        (0, 0, i64::MAX)
                    );
                    assert_eq!(capture.scoped().resource_transition(), None);
                    assert_eq!(capture.scoped().fee_transition(), None);
                }
            }
        } else {
            assert!(matches!(
                policy.into_native(&budget()),
                Err(NativePhloPolicyError::Amount(
                    NativePhloAmountError::OutOfRange
                ))
            ));
        }
    }
}

#[test]
fn decoded_controls_and_sources_preserve_complete_family_charges_and_original_refunds() {
    let work = PhloExecutionLimits {
        resource_entries: 1024,
        authority_nodes: 65_536,
        key_bytes: 1_048_576,
    };
    let wire_limits = PhloWireLimits {
        total_bytes: 1_048_576,
        field_bytes: 524_288,
    };
    let control_limits = PhloControlsLimits {
        wire: wire_limits,
        owners: 256,
        schedules: 2,
        total_classes: 2,
    };
    let source_limits = PhloSourceLimits {
        wire: wire_limits,
        resource_permissions: 1024,
        authority_nodes: 65_536,
    };
    let authority = Sig::Ground(vec![1]);
    let resources = [resource(&authority); 3];
    let intent_limits = PhloFundingIntentLimits {
        wire: wire_limits,
        controls: control_limits,
        sources: 256,
        resource_permissions: 1024,
        authority_nodes: 65_536,
    };
    for count in [1, 2, 3, 4, 64, 65, 129] {
        let identities: Vec<_> = (0..count)
            .map(|index| (index as u64).to_be_bytes())
            .collect();
        let descriptor = PhloControlsV1 {
            limit: 3,
            price_ceiling: 1,
            required_owner_ceilings: vec![1; count],
            permitted_schedules: vec![PhloScheduleV1 {
                protocol_version: ENV.protocol_version,
                network: ENV.network,
                shard: ENV.shard,
                settlement_asset: ENV.asset,
                settlement_unit: ENV.unit,
                decimal_scale: ENV.decimal_scale,
                classes: vec![PhloResourceClassV1 {
                    identity: b"COMM",
                    measurement_unit: b"authority demand",
                    measurement_rule: [2; 32],
                    valuation_rule: [3; 32],
                    weight: 1,
                }],
                actual_price: 1,
                compatibility_rule: [4; 32],
            }],
        };
        let record = PhloFundingIntentV1 {
            schedule_commitment: descriptor.permitted_schedules[0]
                .digest(control_limits.schedule(1))
                .unwrap(),
            controls: descriptor,
            total_exposure: (count as u128) * 4,
            sources: identities
                .iter()
                .map(|identity| {
                    policy(identity, 4, true, &resources)
                        .wire_policy(work, source_limits)
                        .unwrap()
                })
                .collect(),
        };
        let bytes = record.encode(intent_limits).unwrap();
        let decoded = PhloFundingIntentV1::decode(&bytes, intent_limits).unwrap();
        let binding = PhloFundingIntentBinding::new(&decoded, intent_limits).unwrap();
        let view = binding.view().unwrap();
        assert_eq!(view.record(), &record);
        let terms = view.controls();
        let selected = terms.permitted_schedules[0];
        let checked = check_phlo_controls(ENV, 0, i64::MAX as u64, terms, selected, 3).unwrap();
        let execution = check_phlo_execution(
            checked,
            PhloExecutionWitness {
                available: &[],
                required: &resources,
                used: &[],
                unused: &[],
                fresh: &resources,
            },
            work,
        )
        .unwrap();
        let accepted = project_phlo_obligations(
            execution,
            PhloOutcome::Accepted(&[]),
            NonZeroUsize::new(4).unwrap(),
        )
        .unwrap();
        let rejected = project_phlo_obligations(
            execution,
            PhloOutcome::Accepted(&[PhloFailure::Platform]),
            NonZeroUsize::new(4).unwrap(),
        )
        .unwrap();
        let sources: Vec<_> = identities
            .iter()
            .map(|identity| source(identity, 4))
            .collect();
        let eligibility = vec![vec![true; 2]; count];
        let mut allocation = vec![vec![0; 2]; count];
        for index in 0..4 {
            allocation[index % count][usize::from(index != 0)] += 1;
        }
        let zero = vec![vec![0; 2]; count];
        let cases = [
            PhloFundingCase {
                obligations: &accepted,
                eligible: &eligibility,
                assignment: &allocation,
            },
            PhloFundingCase {
                obligations: &rejected,
                eligible: &eligibility,
                assignment: &zero,
            },
        ];
        let family =
            check_phlo_funding_family(&sources, &cases, (count as u128) * 4, PhloFundingLimits {
                sources: NonZeroUsize::new(256).unwrap(),
                cases: NonZeroUsize::new(2).unwrap(),
                obligations: NonZeroUsize::new(4).unwrap(),
                assignment_cells: 65_536,
                custody_bytes: 65_536,
            })
            .unwrap();
        let intent = DecodedPhloFamilyWireIntent {
            controls: terms,
            total_exposure: (count as u128) * 4,
            sources: &decoded.sources,
        };
        let right = PhloFundingTerms {
            required_owner_ceilings: &record.controls.required_owner_ceilings,
            asset: ENV.asset,
            schedule_commitment: selected.commitment,
        };
        let deploy_limits = FundedDeployLimits {
            deploy_bytes: 2_097_152,
            signing: wire_limits,
            funding: intent_limits,
        };
        let body = DeployData {
            term: "Nil".to_string(),
            language: "rholang".to_string(),
            time_stamp: 1,
            valid_after_block_number: 0,
            shard_id: "shard".to_string(),
            expiration_timestamp: None,
            authority_presentations: Vec::new(),
        };
        let envelope = threshold_envelope(
            FundedDeploy::new(body.clone(), bytes.clone(), deploy_limits).unwrap(),
            count,
        );
        let signed_limits = SignedPhloConsentLimits {
            members: NonZeroUsize::new(256).unwrap(),
            intent: intent_limits,
            consent: limits(),
        };
        let offered_envelope = threshold_envelope(
            OfferedFundedDeploy::new(body.clone(), bytes.clone(), 3, 1, deploy_limits).unwrap(),
            count,
        );
        let offered = view
            .check_offered_signed_family(&offered_envelope, &family, right, signed_limits)
            .unwrap();
        assert!(std::ptr::eq(offered.envelope(), &offered_envelope));
        assert_eq!(offered.verified_witnesses().count(), count.div_ceil(2));
        for (limit, price, expected) in [
            (
                2,
                1,
                crate::rust::interpreter::accounting::phlo_controls::PhloOfferError::LimitMismatch,
            ),
            (
                4,
                1,
                crate::rust::interpreter::accounting::phlo_controls::PhloOfferError::LimitMismatch,
            ),
            (
                3,
                0,
                crate::rust::interpreter::accounting::phlo_controls::PhloOfferError::PriceMismatch,
            ),
            (
                3,
                2,
                crate::rust::interpreter::accounting::phlo_controls::PhloOfferError::PriceMismatch,
            ),
        ] {
            let data =
                OfferedFundedDeploy::new(body.clone(), bytes.clone(), limit, price, deploy_limits)
                    .unwrap();
            let mut tampered = offered_envelope.clone();
            tampered.data = data.clone();
            assert!(matches!(
                view.check_offered_signed_family(&tampered, &family, right, signed_limits),
                Err(SignedPhloConsentError::Signature(_))
            ));
            let resigned = threshold_envelope(data, count);
            assert!(
                matches!(view.check_offered_signed_family(&resigned, &family, right, signed_limits),
                Err(SignedPhloConsentError::Offer(error)) if error == expected)
            );
        }
        let mut bounded = signed_limits;
        bounded.members = NonZeroUsize::new(count.saturating_sub(1).max(1)).unwrap();
        if count > 1 {
            assert!(matches!(
                view.check_offered_signed_family(&offered_envelope, &family, right, bounded),
                Err(SignedPhloConsentError::TooManyMembers)
            ));
        }
        let authenticated = view
            .check_signed_family(&envelope, &family, right, signed_limits)
            .unwrap();
        assert!(std::ptr::eq(authenticated.envelope(), &envelope));
        assert_eq!(
            authenticated.verified_witnesses().count(),
            count.div_ceil(2)
        );
        assert!(authenticated
            .verified_witnesses()
            .all(|witness| !witness.sig.is_empty()));
        if count > 1 {
            let mut bounded = signed_limits;
            bounded.members = NonZeroUsize::new(count - 1).unwrap();
            assert!(matches!(
                view.check_signed_family(&envelope, &family, right, bounded),
                Err(SignedPhloConsentError::TooManyMembers)
            ));
        }
        let checked_intent = authenticated.intent();
        assert_eq!(checked_intent.record(), &decoded);
        if count == 2 {
            use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
            use models::rust::phlo_obligation::PhloObligationKeyLimits;

            use crate::rust::interpreter::accounting::monetary_allocation::{
                FundingSearchError, FundingSearchLimits, MonetaryCursor, MonetaryCursorError,
            };
            use crate::rust::interpreter::accounting::phlo_execution::{
                select_phlo_funding_family, NativePhloPolicyError, PhloCaptureLimits,
                PhloFamilyCursorSnapshot, PhloFamilyFundingInput, PhloFamilyFundingLimits,
                PhloFundingRequirement, PhloOutcomeMatchError, PhloOutcomeMatchLimits,
                PhloPolicyCaptureError, PhloScopedCursorSnapshot,
            };
            use crate::rust::interpreter::host_work::HostWorkBudget;

            let budget =
                || HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)));
            let policy_limits = PhloFamilyFundingLimits {
                funding: PhloFundingLimits {
                    sources: NonZeroUsize::new(256).unwrap(),
                    cases: NonZeroUsize::new(2).unwrap(),
                    obligations: NonZeroUsize::new(4).unwrap(),
                    assignment_cells: 65_536,
                    custody_bytes: 65_536,
                },
                keys: PhloObligationKeyLimits {
                    wire: wire_limits,
                    authority_nodes: 65_536,
                },
                aggregate_key_bytes: 1_048_576,
            };
            assert!(!checked_intent
                .verify_funding_policy(0, 0, policy_limits, &budget())
                .unwrap());
            let requirements: Vec<_> = cases
                .iter()
                .map(|case| PhloFundingRequirement {
                    obligations: case.obligations,
                    eligible: case.eligible,
                })
                .collect();
            let selected_family = select_phlo_funding_family(
                PhloFamilyFundingInput {
                    sources: &sources,
                    outcomes: &requirements,
                    total_exposure_limit: record.total_exposure,
                    canonical_resource_cursor: 0,
                    canonical_fee_cursor: 0,
                },
                policy_limits,
                &budget(),
            )
            .unwrap()
            .unwrap();
            assert_ne!(&allocation, &selected_family.assignments()[0]);
            let selected_cases: Vec<_> = cases
                .iter()
                .zip(selected_family.assignments())
                .map(|(case, assignment)| PhloFundingCase {
                    obligations: case.obligations,
                    eligible: case.eligible,
                    assignment,
                })
                .collect();
            let canonical_family = check_phlo_funding_family(
                &sources,
                &selected_cases,
                record.total_exposure,
                policy_limits.funding,
            )
            .unwrap();
            let canonical_signed = view
                .check_signed_family(&envelope, &canonical_family, right, signed_limits)
                .unwrap();
            let canonical_intent = canonical_signed.intent();
            assert!(canonical_intent
                .verify_funding_policy(0, 0, policy_limits, &budget())
                .unwrap());
            assert_eq!(canonical_intent.record(), checked_intent.record());
            let canonical_custodies: Vec<&[u8]> =
                identities.iter().map(|key| key.as_slice()).collect();
            let payer_count = NonZeroUsize::new(count).unwrap();
            let snapshots = PhloFamilyCursorSnapshot {
                canonical_custodies: &canonical_custodies,
                resource: PhloScopedCursorSnapshot {
                    scope: [31; 32],
                    cursor: MonetaryCursor::new(7, 0, payer_count).unwrap(),
                },
                fee: PhloScopedCursorSnapshot {
                    scope: [42; 32],
                    cursor: MonetaryCursor::new(19, 0, payer_count).unwrap(),
                },
            };
            let policy = canonical_signed
                .plan_funding_policy(snapshots, policy_limits, &budget())
                .unwrap()
                .unwrap();
            assert_eq!(policy.signed_intent().intent().record(), &decoded);
            assert_eq!(policy.snapshots().canonical_custodies, canonical_custodies);
            assert_eq!(policy.snapshots().resource.scope, snapshots.resource.scope);
            assert_eq!(
                policy.snapshots().resource.cursor,
                snapshots.resource.cursor
            );
            assert_eq!(policy.snapshots().fee.scope, snapshots.fee.scope);
            assert_eq!(policy.snapshots().fee.cursor, snapshots.fee.cursor);
            assert_eq!(
                policy.selection().assignments(),
                selected_family.assignments()
            );
            assert!(authenticated
                .plan_funding_policy(snapshots, policy_limits, &budget())
                .unwrap()
                .is_none());
            let capture_limits = PhloCaptureLimits {
                funding: FundingSearchLimits {
                    source_cap: payer_count,
                    obligation_cap: NonZeroUsize::new(4).unwrap(),
                },
                key: policy_limits.keys,
                aggregate_key_bytes: policy_limits.aggregate_key_bytes,
            };
            let accepted_capture = policy.capture_case(0, capture_limits, &budget()).unwrap();
            let offered_policy = view
                .check_offered_signed_family(
                    &offered_envelope,
                    &canonical_family,
                    right,
                    signed_limits,
                )
                .unwrap()
                .plan_funding_policy(snapshots, policy_limits, &budget())
                .unwrap()
                .unwrap()
                .into_native(&budget())
                .unwrap();
            assert_eq!(
                offered_policy.policy().selection().assignments(),
                policy.selection().assignments()
            );
            for branch in 0..canonical_family.cases().len() {
                let offered_capture = offered_policy
                    .capture_case(branch, capture_limits, &budget())
                    .unwrap();
                let original = policy
                    .capture_case(branch, capture_limits, &budget())
                    .unwrap();
                assert!(std::ptr::eq(
                    offered_capture.scoped().signed_intent().envelope(),
                    &offered_envelope
                ));
                assert_eq!(offered_capture.scoped().capture(), original.capture());
                assert_eq!(
                    offered_capture.scoped().resource_transition(),
                    original.resource_transition()
                );
                assert_eq!(
                    offered_capture.scoped().fee_transition(),
                    original.fee_transition()
                );
            }
            assert_eq!(accepted_capture.capture().branch(), 0);
            assert_eq!(
                accepted_capture
                    .capture()
                    .sources()
                    .iter()
                    .map(|source| source.source().custody)
                    .collect::<Vec<_>>(),
                canonical_custodies
            );
            assert_eq!(
                accepted_capture
                    .capture()
                    .sources()
                    .iter()
                    .map(|source| source.acquisition())
                    .collect::<Vec<_>>(),
                vec![2, 1]
            );
            assert_eq!(
                accepted_capture
                    .capture()
                    .sources()
                    .iter()
                    .map(|source| source.fee())
                    .collect::<Vec<_>>(),
                vec![1, 0]
            );
            assert_eq!(
                accepted_capture
                    .capture()
                    .sources()
                    .iter()
                    .map(|source| source.refund())
                    .collect::<Vec<_>>(),
                vec![0, 0]
            );
            for (transition, snapshot) in [
                (
                    accepted_capture.resource_transition().unwrap(),
                    snapshots.resource,
                ),
                (accepted_capture.fee_transition().unwrap(), snapshots.fee),
            ] {
                assert_eq!(transition.scope(), &snapshot.scope);
                assert_eq!(transition.expected(), snapshot.cursor);
                assert_eq!(transition.next().revision(), snapshot.cursor.revision() + 1);
                assert_eq!(transition.next().position(), 1);
                assert_eq!(
                    transition
                        .checked_successor(&snapshot.scope, snapshot.cursor, payer_count)
                        .unwrap(),
                    transition.next()
                );
                assert_eq!(
                    transition.checked_successor(&snapshot.scope, transition.next(), payer_count),
                    Err(MonetaryCursorError::StaleTransition)
                );
                assert_eq!(
                    transition.checked_successor(&[99; 32], snapshot.cursor, payer_count),
                    Err(MonetaryCursorError::ScopeMismatch)
                );
            }
            let rejected_capture = policy.capture_case(1, capture_limits, &budget()).unwrap();
            assert_eq!(rejected_capture.capture().branch(), 1);
            assert_eq!(rejected_capture.resource_transition(), None);
            assert_eq!(rejected_capture.fee_transition(), None);
            assert_eq!(
                rejected_capture
                    .capture()
                    .sources()
                    .iter()
                    .map(|source| source.debit())
                    .collect::<Vec<_>>(),
                vec![0, 0]
            );
            assert_eq!(
                rejected_capture
                    .capture()
                    .sources()
                    .iter()
                    .map(|source| source.refund())
                    .collect::<Vec<_>>(),
                vec![3, 1]
            );
            assert!(policy.capture_case(2, capture_limits, &budget()).is_err());
            let native_policy = policy.clone().into_native(&budget()).unwrap();
            let match_limits = PhloOutcomeMatchLimits {
                execution: work,
                key: policy_limits.keys,
                aggregate_key_bytes: policy_limits.aggregate_key_bytes,
                cases: NonZeroUsize::new(3).unwrap(),
            };
            let matched_success = native_policy
                .capture_matching_execution(
                    execution,
                    PhloOutcome::Accepted(&[]),
                    match_limits,
                    capture_limits,
                    &budget(),
                )
                .unwrap();
            let offered_success = offered_policy
                .capture_matching_execution(
                    execution,
                    PhloOutcome::Accepted(&[]),
                    match_limits,
                    capture_limits,
                    &budget(),
                )
                .unwrap();
            assert!(std::ptr::eq(
                offered_success.scoped().signed_intent().envelope(),
                &offered_envelope
            ));
            assert_eq!(offered_success.amounts(), matched_success.amounts());
            assert_eq!(matched_success.scoped(), &accepted_capture);
            let matched_platform = native_policy
                .capture_matching_execution(
                    execution,
                    PhloOutcome::Accepted(&[PhloFailure::Platform, PhloFailure::Platform]),
                    match_limits,
                    capture_limits,
                    &budget(),
                )
                .unwrap();
            assert_eq!(matched_platform.scoped(), &rejected_capture);
            assert!(matches!(
                native_policy.capture_matching_execution(
                    execution,
                    PhloOutcome::Accepted(&[PhloFailure::User]),
                    match_limits,
                    capture_limits,
                    &budget()
                ),
                Err(PhloOutcomeMatchError::NoMatchingOutcome)
            ));
            assert!(matches!(
                native_policy.capture_matching_execution(
                    execution,
                    PhloOutcome::AdmissionRejected,
                    match_limits,
                    capture_limits,
                    &budget()
                ),
                Err(PhloOutcomeMatchError::NoMatchingOutcome)
            ));
            let empty_execution = check_phlo_execution(
                checked,
                PhloExecutionWitness {
                    available: &[],
                    required: &[],
                    used: &[],
                    unused: &[],
                    fresh: &[],
                },
                work,
            )
            .unwrap();
            assert!(matches!(
                native_policy.capture_matching_execution(
                    empty_execution,
                    PhloOutcome::Accepted(&[]),
                    match_limits,
                    capture_limits,
                    &budget()
                ),
                Err(PhloOutcomeMatchError::NoMatchingOutcome)
            ));
            assert!(matches!(
                native_policy.capture_matching_execution(
                    execution,
                    PhloOutcome::Accepted(&[]),
                    PhloOutcomeMatchLimits {
                        cases: NonZeroUsize::new(1).unwrap(),
                        ..match_limits
                    },
                    capture_limits,
                    &budget()
                ),
                Err(PhloOutcomeMatchError::TooManyCases)
            ));
            let no_match_work = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
            assert!(native_policy
                .capture_matching_execution(
                    execution,
                    PhloOutcome::Accepted(&[]),
                    match_limits,
                    capture_limits,
                    &no_match_work
                )
                .is_err());
            let reversed_resources: Vec<_> = resources.iter().rev().copied().collect();
            let reordered_execution = check_phlo_execution(
                checked,
                PhloExecutionWitness {
                    available: &[],
                    required: &reversed_resources,
                    used: &[],
                    unused: &[],
                    fresh: &reversed_resources,
                },
                work,
            )
            .unwrap();
            let reordered_obligations = project_phlo_obligations(
                reordered_execution,
                PhloOutcome::Accepted(&[]),
                NonZeroUsize::new(4).unwrap(),
            )
            .unwrap();
            let alias_cases = [
                selected_cases[0],
                PhloFundingCase {
                    obligations: &reordered_obligations,
                    eligible: &eligibility,
                    assignment: selected_cases[0].assignment,
                },
                selected_cases[1],
            ];
            let alias_limits = PhloFamilyFundingLimits {
                funding: PhloFundingLimits {
                    cases: NonZeroUsize::new(3).unwrap(),
                    ..policy_limits.funding
                },
                ..policy_limits
            };
            let alias_family = check_phlo_funding_family(
                &sources,
                &alias_cases,
                record.total_exposure,
                alias_limits.funding,
            )
            .unwrap();
            let alias_policy = view
                .check_signed_family(&envelope, &alias_family, right, signed_limits)
                .unwrap()
                .plan_funding_policy(snapshots, alias_limits, &budget())
                .unwrap()
                .unwrap()
                .into_native(&budget())
                .unwrap();
            let alias_budget = budget();
            let alias_capture = alias_policy
                .capture_matching_execution(
                    reordered_execution,
                    PhloOutcome::Accepted(&[]),
                    match_limits,
                    capture_limits,
                    &alias_budget,
                )
                .unwrap();
            for dimension in HostWorkDimension::ALL {
                let usage = alias_budget.usage(dimension).get();
                if usage == 0 {
                    continue;
                }
                let mut exact = alias_budget.limits();
                exact.set(dimension, HostWorkLimit::new(usage));
                assert_eq!(
                    alias_policy
                        .capture_matching_execution(
                            reordered_execution,
                            PhloOutcome::Accepted(&[]),
                            match_limits,
                            capture_limits,
                            &HostWorkBudget::new(exact),
                        )
                        .unwrap(),
                    alias_capture,
                );
                exact.set(dimension, HostWorkLimit::new(usage - 1));
                let short = HostWorkBudget::new(exact);
                assert!(alias_policy
                    .capture_matching_execution(
                        reordered_execution,
                        PhloOutcome::Accepted(&[]),
                        match_limits,
                        capture_limits,
                        &short,
                    )
                    .is_err());
                assert!(short.is_rejected());
            }
            let canonical_capture = alias_capture.scoped().capture();
            let expected_capture = accepted_capture.capture();
            assert_eq!(canonical_capture.sources(), expected_capture.sources());
            assert_eq!(
                canonical_capture.obligation_keys(),
                expected_capture.obligation_keys()
            );
            assert_eq!(canonical_capture.amounts(), expected_capture.amounts());
            assert_eq!(canonical_capture.eligible(), expected_capture.eligible());
            assert_eq!(
                canonical_capture.assignment(),
                expected_capture.assignment()
            );
            assert_eq!(
                alias_capture.scoped().resource_transition(),
                accepted_capture.resource_transition()
            );
            assert_eq!(
                alias_capture.scoped().fee_transition(),
                accepted_capture.fee_transition()
            );
            let mut narrower_eligibility = eligibility.clone();
            narrower_eligibility[0][0] = false;
            let conflicting_cases = [
                PhloFundingCase {
                    obligations: &rejected,
                    eligible: &eligibility,
                    assignment: &zero,
                },
                PhloFundingCase {
                    obligations: &rejected,
                    eligible: &narrower_eligibility,
                    assignment: &zero,
                },
            ];
            let conflicting_family = check_phlo_funding_family(
                &sources,
                &conflicting_cases,
                record.total_exposure,
                policy_limits.funding,
            )
            .unwrap();
            let conflicting_policy = view
                .check_signed_family(&envelope, &conflicting_family, right, signed_limits)
                .unwrap()
                .plan_funding_policy(snapshots, policy_limits, &budget())
                .unwrap()
                .unwrap()
                .into_native(&budget())
                .unwrap();
            assert!(matches!(
                conflicting_policy.capture_matching_execution(
                    execution,
                    PhloOutcome::Accepted(&[PhloFailure::Platform]),
                    match_limits,
                    capture_limits,
                    &budget()
                ),
                Err(PhloOutcomeMatchError::ConflictingMatchingOutcomes)
            ));
            assert_eq!(native_policy.policy().snapshots(), snapshots);
            assert_eq!(
                native_policy.policy().selection().assignments(),
                policy.selection().assignments()
            );
            for branch in 0..2 {
                let scoped = policy
                    .capture_case(branch, capture_limits, &budget())
                    .unwrap();
                let native = native_policy
                    .capture_case(branch, capture_limits, &budget())
                    .unwrap();
                assert_eq!(native.scoped(), &scoped);
                assert_eq!(native.amounts().len(), canonical_custodies.len());
                for ((amount, captured), custody) in native
                    .amounts()
                    .iter()
                    .zip(scoped.capture().sources())
                    .zip(&canonical_custodies)
                {
                    assert_eq!(amount.custody(), *custody);
                    assert_eq!(amount.custody(), captured.source().custody);
                    assert_eq!(u64::try_from(amount.hold()).unwrap(), captured.hold());
                    assert_eq!(
                        u64::try_from(amount.acquisition()).unwrap(),
                        captured.acquisition()
                    );
                    assert_eq!(u64::try_from(amount.fee()).unwrap(), captured.fee());
                    assert_eq!(u64::try_from(amount.refund()).unwrap(), captured.refund());
                    assert_eq!(
                        amount.hold(),
                        amount.acquisition() + amount.fee() + amount.refund()
                    );
                }
            }
            assert!(matches!(
                native_policy.capture_case(2, capture_limits, &budget()),
                Err(NativePhloPolicyError::Policy(_))
            ));
            let zero_budget =
                || HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
            assert!(matches!(
                policy.clone().into_native(&zero_budget()),
                Err(NativePhloPolicyError::Search(_))
            ));
            assert!(native_policy
                .capture_case(0, capture_limits, &zero_budget())
                .is_err());
            for branch in 0..2 {
                let generic_budget = budget();
                policy
                    .capture_case(branch, capture_limits, &generic_budget)
                    .unwrap();
                let complete_budget = budget();
                let complete = native_policy
                    .capture_case(branch, capture_limits, &complete_budget)
                    .unwrap();
                for dimension in [
                    HostWorkDimension::VerificationOperations,
                    HostWorkDimension::SearchStateBytes,
                ] {
                    let generic_usage = generic_budget.usage(dimension).get();
                    let complete_usage = complete_budget.usage(dimension).get();
                    assert!(complete_usage > generic_usage);
                    let mut generic_limit = generic_budget.limits();
                    generic_limit.set(dimension, HostWorkLimit::new(generic_usage));
                    assert!(policy
                        .capture_case(branch, capture_limits, &HostWorkBudget::new(generic_limit))
                        .is_ok());
                    assert!(matches!(
                        native_policy.capture_case(
                            branch,
                            capture_limits,
                            &HostWorkBudget::new(generic_limit)
                        ),
                        Err(NativePhloPolicyError::Search(FundingSearchError::HostWork(
                            _
                        )))
                    ));
                    let mut exact_limit = generic_budget.limits();
                    exact_limit.set(dimension, HostWorkLimit::new(complete_usage));
                    assert_eq!(
                        native_policy
                            .capture_case(branch, capture_limits, &HostWorkBudget::new(exact_limit))
                            .unwrap(),
                        complete
                    );
                }
            }
            let reversed_sources: Vec<_> = sources.iter().rev().copied().collect();
            let reversed_eligibility: Vec<_> = eligibility.iter().rev().cloned().collect();
            let reversed_assignments: Vec<Vec<Vec<u64>>> = selected_family
                .assignments()
                .iter()
                .map(|assignment| assignment.iter().rev().cloned().collect())
                .collect();
            let reversed_cases: Vec<_> = selected_cases
                .iter()
                .zip(&reversed_assignments)
                .map(|(case, assignment)| PhloFundingCase {
                    obligations: case.obligations,
                    eligible: &reversed_eligibility,
                    assignment,
                })
                .collect();
            let reversed_family = check_phlo_funding_family(
                &reversed_sources,
                &reversed_cases,
                record.total_exposure,
                policy_limits.funding,
            )
            .unwrap();
            let reversed_policy = view
                .check_signed_family(&envelope, &reversed_family, right, signed_limits)
                .unwrap()
                .plan_funding_policy(snapshots, policy_limits, &budget())
                .unwrap()
                .unwrap()
                .into_native(&budget())
                .unwrap();
            for branch in 0..2 {
                let native = native_policy
                    .capture_case(branch, capture_limits, &budget())
                    .unwrap();
                let reversed = reversed_policy
                    .capture_case(branch, capture_limits, &budget())
                    .unwrap();
                assert_eq!(native.amounts(), reversed.amounts());
                assert_eq!(
                    native.scoped().resource_transition(),
                    reversed.scoped().resource_transition()
                );
                assert_eq!(
                    native.scoped().fee_transition(),
                    reversed.scoped().fee_transition()
                );
            }
            let unsorted = [canonical_custodies[1], canonical_custodies[0]];
            let duplicate = [canonical_custodies[0], canonical_custodies[0]];
            let wrong: [&[u8]; 2] = [b"wrong-a", b"wrong-b"];
            let empty: [&[u8]; 2] = [b"", canonical_custodies[1]];
            for custody_keys in [
                unsorted.as_slice(),
                duplicate.as_slice(),
                wrong.as_slice(),
                empty.as_slice(),
                &canonical_custodies[..1],
            ] {
                assert!(matches!(
                    canonical_signed.plan_funding_policy(
                        PhloFamilyCursorSnapshot {
                            canonical_custodies: custody_keys,
                            ..snapshots
                        },
                        policy_limits,
                        &budget()
                    ),
                    Err(PhloPolicyCaptureError::CursorCohortMismatch)
                ));
            }
            for exhaust_resource in [false, true] {
                let exhausted = MonetaryCursor::new(i64::MAX, 0, payer_count).unwrap();
                let exhausted_snapshots = if exhaust_resource {
                    PhloFamilyCursorSnapshot {
                        resource: PhloScopedCursorSnapshot {
                            cursor: exhausted,
                            ..snapshots.resource
                        },
                        ..snapshots
                    }
                } else {
                    PhloFamilyCursorSnapshot {
                        fee: PhloScopedCursorSnapshot {
                            cursor: exhausted,
                            ..snapshots.fee
                        },
                        ..snapshots
                    }
                };
                assert!(matches!(
                    canonical_signed.plan_funding_policy(
                        exhausted_snapshots,
                        policy_limits,
                        &budget()
                    ),
                    Err(PhloPolicyCaptureError::Cursor(
                        MonetaryCursorError::RevisionExhausted
                    ))
                ));
            }
            let zero_cases = [selected_cases[1]];
            let zero_family = check_phlo_funding_family(
                &sources,
                &zero_cases,
                record.total_exposure,
                policy_limits.funding,
            )
            .unwrap();
            let zero_signed = view
                .check_signed_family(&envelope, &zero_family, right, signed_limits)
                .unwrap();
            let exhausted = MonetaryCursor::new(i64::MAX, 0, payer_count).unwrap();
            let both_exhausted = PhloFamilyCursorSnapshot {
                resource: PhloScopedCursorSnapshot {
                    cursor: exhausted,
                    ..snapshots.resource
                },
                fee: PhloScopedCursorSnapshot {
                    cursor: exhausted,
                    ..snapshots.fee
                },
                ..snapshots
            };
            let zero_policy = zero_signed
                .plan_funding_policy(both_exhausted, policy_limits, &budget())
                .unwrap()
                .unwrap();
            let zero_capture = zero_policy
                .capture_case(0, capture_limits, &budget())
                .unwrap();
            assert_eq!(zero_capture.resource_transition(), None);
            assert_eq!(zero_capture.fee_transition(), None);
            assert!(zero_capture
                .capture()
                .sources()
                .iter()
                .all(|source| source.hold() == 0 && source.debit() == 0 && source.refund() == 0));
            let prepaid_execution = check_phlo_execution(
                checked,
                PhloExecutionWitness {
                    available: &resources,
                    required: &resources,
                    used: &resources,
                    unused: &[],
                    fresh: &[],
                },
                work,
            )
            .unwrap();
            let fee_only = project_phlo_obligations(
                prepaid_execution,
                PhloOutcome::Accepted(&[]),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
            let fee_only_edges = vec![vec![true]; count];
            let fee_only_assignment = [vec![1], vec![0]];
            let fee_only_cases = [PhloFundingCase {
                obligations: &fee_only,
                eligible: &fee_only_edges,
                assignment: &fee_only_assignment,
            }];
            let fee_only_family = check_phlo_funding_family(
                &sources,
                &fee_only_cases,
                record.total_exposure,
                policy_limits.funding,
            )
            .unwrap();
            let fee_only_signed = view
                .check_signed_family(&envelope, &fee_only_family, right, signed_limits)
                .unwrap();
            let fee_only_policy = fee_only_signed
                .plan_funding_policy(
                    PhloFamilyCursorSnapshot {
                        resource: both_exhausted.resource,
                        ..snapshots
                    },
                    policy_limits,
                    &budget(),
                )
                .unwrap()
                .unwrap();
            let fee_only_capture = fee_only_policy
                .capture_case(0, capture_limits, &budget())
                .unwrap();
            assert_eq!(fee_only_capture.resource_transition(), None);
            assert_eq!(
                fee_only_capture.fee_transition().unwrap().expected(),
                snapshots.fee.cursor
            );
            assert!(matches!(
                fee_only_signed.plan_funding_policy(
                    PhloFamilyCursorSnapshot {
                        fee: both_exhausted.fee,
                        ..snapshots
                    },
                    policy_limits,
                    &budget()
                ),
                Err(PhloPolicyCaptureError::Cursor(
                    MonetaryCursorError::RevisionExhausted
                ))
            ));
            let generated_snapshots = (
                proptest::array::uniform32(any::<u8>()),
                proptest::array::uniform32(any::<u8>()),
                0_i64..i64::MAX,
                0_i64..i64::MAX,
            );
            let mut runner = proptest::test_runner::TestRunner::new(
                proptest::test_runner::Config::with_cases(32),
            );
            runner
                .run(
                    &generated_snapshots,
                    |(resource_scope, fee_scope, resource_revision, fee_revision)| {
                        let generated = PhloFamilyCursorSnapshot {
                            resource: PhloScopedCursorSnapshot {
                                scope: resource_scope,
                                cursor: MonetaryCursor::new(resource_revision, 0, payer_count)
                                    .unwrap(),
                            },
                            fee: PhloScopedCursorSnapshot {
                                scope: fee_scope,
                                cursor: MonetaryCursor::new(fee_revision, 0, payer_count).unwrap(),
                            },
                            ..snapshots
                        };
                        let generated_policy = canonical_signed
                            .plan_funding_policy(generated, policy_limits, &budget())
                            .unwrap()
                            .unwrap();
                        let native_policy =
                            generated_policy.clone().into_native(&budget()).unwrap();
                        let accepted = generated_policy
                            .capture_case(0, capture_limits, &budget())
                            .unwrap();
                        for (transition, captured) in [
                            (accepted.resource_transition().unwrap(), generated.resource),
                            (accepted.fee_transition().unwrap(), generated.fee),
                        ] {
                            prop_assert_eq!(transition.scope(), &captured.scope);
                            prop_assert_eq!(transition.expected(), captured.cursor);
                            prop_assert_eq!(
                                transition.next().revision(),
                                captured.cursor.revision() + 1
                            );
                            prop_assert_eq!(transition.next().position(), 1);
                            let successor = transition
                                .checked_successor(&captured.scope, captured.cursor, payer_count)
                                .unwrap();
                            prop_assert_eq!(successor, transition.next());
                            prop_assert_eq!(
                                transition.checked_successor(
                                    &captured.scope,
                                    successor,
                                    payer_count
                                ),
                                Err(MonetaryCursorError::StaleTransition)
                            );
                        }
                        for branch in 0..2 {
                            let captured = generated_policy
                                .capture_case(branch, capture_limits, &budget())
                                .unwrap();
                            let native = native_policy
                                .capture_case(branch, capture_limits, &budget())
                                .unwrap();
                            prop_assert_eq!(native.scoped(), &captured);
                            prop_assert_eq!(
                                native.amounts().len(),
                                captured.capture().sources().len()
                            );
                            for (amount, source) in
                                native.amounts().iter().zip(captured.capture().sources())
                            {
                                prop_assert_eq!(amount.custody(), source.source().custody);
                                prop_assert_eq!(
                                    u64::try_from(amount.hold()).unwrap(),
                                    source.hold()
                                );
                                prop_assert_eq!(
                                    u64::try_from(amount.acquisition()).unwrap(),
                                    source.acquisition()
                                );
                                prop_assert_eq!(u64::try_from(amount.fee()).unwrap(), source.fee());
                                prop_assert_eq!(
                                    u64::try_from(amount.refund()).unwrap(),
                                    source.refund()
                                );
                                prop_assert_eq!(
                                    amount.hold(),
                                    amount.acquisition() + amount.fee() + amount.refund()
                                );
                            }
                            for source in captured.capture().sources() {
                                prop_assert_eq!(source.hold(), source.debit() + source.refund());
                                prop_assert_eq!(
                                    source.debit(),
                                    source.acquisition() + source.fee()
                                );
                            }
                            if branch == 1 {
                                prop_assert_eq!(captured.resource_transition(), None);
                                prop_assert_eq!(captured.fee_transition(), None);
                            }
                        }
                        Ok(())
                    },
                )
                .unwrap();
        }
        let mut changed = envelope.clone();
        let mut changed_body = body.clone();
        changed_body.term = "new x in { x!(0) }".to_string();
        changed.data = FundedDeploy::new(changed_body, bytes.clone(), deploy_limits).unwrap();
        assert!(matches!(
            view.check_signed_family(&changed, &family, right, signed_limits),
            Err(SignedPhloConsentError::Signature(_))
        ));
        let mut other_record = record.clone();
        other_record.total_exposure += 1;
        let other_bytes = other_record.encode(intent_limits).unwrap();
        let other_envelope = Cosigned::create_single_envelope(
            FundedDeploy::new(body, other_bytes, deploy_limits).unwrap(),
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap();
        assert!(matches!(
            view.check_signed_family(&other_envelope, &family, right, signed_limits),
            Err(SignedPhloConsentError::RecordMismatch)
        ));
        let mut bounded = signed_limits;
        bounded.intent.sources = count - 1;
        assert!(matches!(
            view.check_signed_family(&envelope, &family, right, bounded),
            Err(SignedPhloConsentError::Record(_))
        ));
        {
            use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
            use models::rust::phlo_obligation::PhloObligationKeyLimits;

            use crate::rust::interpreter::accounting::monetary_allocation::{
                FundingReservationError, FundingSearchError, FundingSearchLimits,
            };
            use crate::rust::interpreter::accounting::phlo_execution::{
                PhloCaptureError, PhloCaptureLimits, PhloObligationError,
            };
            use crate::rust::interpreter::host_work::HostWorkBudget;
            let sources: Vec<_> = sources
                .iter()
                .enumerate()
                .map(|(index, source)| PhloFundingSource {
                    capacity: 4 + index as u64,
                    exposure_limit: allocation[index].iter().sum::<u64>(),
                    debit_limit: allocation[index].iter().sum::<u64>(),
                    ..*source
                })
                .collect();
            let eligibility: Vec<_> = (0..count).map(|index| vec![index == 0, true]).collect();
            let short_execution = check_phlo_execution(
                checked,
                PhloExecutionWitness {
                    available: &[],
                    required: &resources[..1],
                    used: &[],
                    unused: &[],
                    fresh: &resources[..1],
                },
                work,
            )
            .unwrap();
            let rejected = project_phlo_obligations(
                short_execution,
                PhloOutcome::Accepted(&[PhloFailure::Platform]),
                NonZeroUsize::new(4).unwrap(),
            )
            .unwrap();
            let short_eligibility: Vec<_> =
                eligibility.iter().map(|row| row[..2].to_vec()).collect();
            let zero = vec![vec![0; 2]; count];
            let cases = [
                PhloFundingCase {
                    obligations: &accepted,
                    eligible: &eligibility,
                    assignment: &allocation,
                },
                PhloFundingCase {
                    obligations: &rejected,
                    eligible: &short_eligibility,
                    assignment: &zero,
                },
            ];
            let family = check_phlo_funding_family(
                &sources,
                &cases,
                (count as u128) * 4,
                PhloFundingLimits {
                    sources: NonZeroUsize::new(256).unwrap(),
                    cases: NonZeroUsize::new(2).unwrap(),
                    obligations: NonZeroUsize::new(4).unwrap(),
                    assignment_cells: 65_536,
                    custody_bytes: 65_536,
                },
            )
            .unwrap();
            let checked_intent = view
                .check_signed_family(&envelope, &family, right, signed_limits)
                .unwrap()
                .intent();
            let capture_limits = PhloCaptureLimits {
                funding: FundingSearchLimits {
                    source_cap: NonZeroUsize::new(256).unwrap(),
                    obligation_cap: NonZeroUsize::new(4).unwrap(),
                },
                key: PhloObligationKeyLimits {
                    wire: wire_limits,
                    authority_nodes: 65_536,
                },
                aggregate_key_bytes: 1_048_576,
            };
            let budget =
                || HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000)));
            assert_eq!(
                checked_intent.capture_case(2, capture_limits, &budget()),
                Err(PhloCaptureError::Branch(
                    FundingReservationError::UnknownBranch
                ))
            );
            let reverse_sources: Vec<_> = sources.iter().rev().copied().collect();
            let reverse_eligibility: Vec<_> = eligibility.iter().rev().cloned().collect();
            let reverse_short_eligibility: Vec<_> =
                short_eligibility.iter().rev().cloned().collect();
            let reverse_assignment: Vec<_> = allocation.iter().rev().cloned().collect();
            let reverse_cases = [
                PhloFundingCase {
                    obligations: &accepted,
                    eligible: &reverse_eligibility,
                    assignment: &reverse_assignment,
                },
                PhloFundingCase {
                    obligations: &rejected,
                    eligible: &reverse_short_eligibility,
                    assignment: &zero,
                },
            ];
            let reverse_family = check_phlo_funding_family(
                &reverse_sources,
                &reverse_cases,
                (count as u128) * 4,
                PhloFundingLimits {
                    sources: NonZeroUsize::new(256).unwrap(),
                    cases: NonZeroUsize::new(2).unwrap(),
                    obligations: NonZeroUsize::new(4).unwrap(),
                    assignment_cells: 65_536,
                    custody_bytes: 65_536,
                },
            )
            .unwrap();
            let reverse_intent = view.check_family(&reverse_family, right, limits()).unwrap();
            for branch in 0..2 {
                let captured = checked_intent
                    .capture_case(branch, capture_limits, &budget())
                    .unwrap();
                let reordered = reverse_intent
                    .capture_case(branch, capture_limits, &budget())
                    .unwrap();
                assert_eq!(captured.branch(), branch);
                assert_eq!(captured.amounts().len(), 2);
                assert!(std::ptr::eq(
                    captured.intent().bound().consent().family(),
                    &family
                ));
                assert_eq!(captured.intent().record(), &decoded);
                assert_eq!(captured.sources(), reordered.sources());
                assert_eq!(captured.obligation_keys(), reordered.obligation_keys());
                assert_eq!(captured.amounts(), reordered.amounts());
                assert_eq!(captured.eligible(), reordered.eligible());
                assert_eq!(captured.assignment(), reordered.assignment());
                let native = family.native_amounts(branch).unwrap();
                for (index, (source, amounts)) in captured.sources().iter().zip(native).enumerate()
                {
                    assert_eq!(source.source(), sources[index]);
                    assert_eq!(source.source().custody, amounts.custody());
                    assert_eq!(source.hold(), amounts.hold() as u64);
                    assert_eq!(
                        source.debit(),
                        (amounts.acquisition() + amounts.fee()) as u64
                    );
                    assert_eq!(source.fee(), amounts.fee() as u64);
                    assert_eq!(source.refund(), amounts.refund() as u64);
                    assert_eq!(
                        source.hold(),
                        source.acquisition() + source.fee() + source.refund()
                    );
                }
                if count <= 3 {
                    let measured = budget();
                    checked_intent
                        .capture_case(branch, capture_limits, &measured)
                        .unwrap();
                    for dimension in [
                        HostWorkDimension::SearchCandidates,
                        HostWorkDimension::SearchStateBytes,
                        HostWorkDimension::VerificationOperations,
                    ] {
                        let complete = measured.usage(dimension).get();
                        for prefix in [0, 1, complete / 2, complete - 1, complete] {
                            let mut bounds =
                                HostWorkLimits::uniform(HostWorkLimit::new(1_000_000_000));
                            bounds.set(dimension, HostWorkLimit::new(prefix));
                            let result = checked_intent.capture_case(
                                branch,
                                capture_limits,
                                &HostWorkBudget::new(bounds),
                            );
                            if prefix == complete {
                                assert_eq!(result.unwrap(), captured);
                            } else {
                                assert!(matches!(result,
                                Err(PhloCaptureError::Search(FundingSearchError::HostWork(_))) |
                                Err(PhloCaptureError::Obligation(PhloObligationError::Execution(PhloExecutionError::HostWork(_)))) |
                                Err(PhloCaptureError::Identity(crate::rust::interpreter::accounting::monetary_allocation::FundingIdentityError::Search(FundingSearchError::HostWork(_))))));
                            }
                        }
                    }
                }
            }
        }
        let bound = checked_intent.bound();
        let charged = bound.consent().family().native_amounts(0).unwrap();
        let released = bound.consent().family().native_amounts(1).unwrap();
        assert_eq!(
            charged
                .iter()
                .map(|amount| amount.acquisition())
                .sum::<i64>(),
            3
        );
        assert_eq!(charged.iter().map(|amount| amount.fee()).sum::<i64>(), 1);
        for index in 0..count {
            assert_eq!(charged[index].custody(), identities[index]);
            assert_eq!(released[index].custody(), identities[index]);
            assert_eq!(
                charged[index].hold(),
                charged[index].acquisition() + charged[index].fee() + charged[index].refund()
            );
            assert_eq!(released[index].acquisition(), 0);
            assert_eq!(released[index].fee(), 0);
            assert_eq!(released[index].refund(), charged[index].hold());
        }
        assert_eq!(
            check_phlo_family_wire_consent(
                &family,
                DecodedPhloFamilyWireIntent {
                    controls: SignedPhloControls { limit: 4, ..terms },
                    ..intent
                },
                limits()
            ),
            Err(PhloConsentError::DifferentControls)
        );
        let mut mutations = Vec::new();
        let mut changed = record.clone();
        changed.controls.limit += 1;
        mutations.push((
            changed,
            PhloFundingIntentCheckError::Consent(PhloConsentError::DifferentControls),
        ));
        let mut changed = record.clone();
        changed.total_exposure = 3;
        mutations.push((
            changed,
            PhloFundingIntentCheckError::Consent(PhloConsentError::TotalExposureExceeded),
        ));
        let mut changed = record.clone();
        changed.schedule_commitment[0] ^= 1;
        mutations.push((
            changed,
            PhloFundingIntentCheckError::SelectedScheduleMismatch,
        ));
        let mut changed = record.clone();
        changed.sources.pop();
        mutations.push((
            changed,
            PhloFundingIntentCheckError::Consent(PhloConsentError::MissingSource),
        ));
        let mut changed = record.clone();
        changed.controls.required_owner_ceilings[0] += 1;
        mutations.push((
            changed,
            PhloFundingIntentCheckError::Consent(PhloConsentError::DifferentControls),
        ));
        for (changed, expected) in mutations {
            let bytes = changed.encode(intent_limits).unwrap();
            let decoded = PhloFundingIntentV1::decode(&bytes, intent_limits).unwrap();
            let binding = PhloFundingIntentBinding::new(&decoded, intent_limits).unwrap();
            let changed_view = binding.view().unwrap();
            assert!(matches!(
                changed_view.check_signed_family(&envelope, &family, right, signed_limits),
                Err(SignedPhloConsentError::RecordMismatch)
            ));
            let signed_change = threshold_envelope(
                FundedDeploy::new(envelope.data.body().clone(), bytes.clone(), deploy_limits)
                    .unwrap(),
                count,
            );
            assert!(matches!(
                changed_view.check_signed_family(&signed_change, &family, right, signed_limits),
                Err(SignedPhloConsentError::Funding(error)) if error == expected
            ));
            assert_eq!(
                changed_view
                    .check_family(&family, right, limits())
                    .map(|_| ()),
                Err(expected)
            );
        }
        assert_eq!(
            view.check_family(
                &family,
                PhloFundingTerms {
                    asset: b"other asset",
                    ..right
                },
                limits()
            )
            .map(|_| ()),
            Err(PhloFundingIntentCheckError::FundingTerms(
                PhloFundingTermsError::AssetMismatch
            ))
        );
        assert_eq!(
            view.check_family(
                &family,
                PhloFundingTerms {
                    schedule_commitment: [7; 32],
                    ..right
                },
                limits()
            )
            .map(|_| ()),
            Err(PhloFundingIntentCheckError::FundingTerms(
                PhloFundingTermsError::ScheduleMismatch
            ))
        );
    }
}
