use super::*;

fn offered(limit: i64, price: i64) -> OfferedFundedDeploy {
    OfferedFundedDeploy::new(body(), funding(10, 3, 30), limit, price, limits()).unwrap()
}

#[test]
fn offered_rev_ceiling_includes_the_separate_fee() {
    assert_eq!(offered(17, 7).rev_ceiling(11).unwrap(), 130);
    assert!(offered(i64::MAX, i64::MAX).rev_ceiling(u128::MAX).is_err());
}

#[test]
fn offered_processed_wire_preserves_signed_envelope_and_rejects_changed_cost() {
    use std::num::NonZeroUsize;

    use crate::rhoapi::PCost;
    use crate::rust::casper::protocol::casper_message::ProcessedUserDeploy;
    use crate::rust::casper::protocol::offered_processed_deploy::OfferedProcessedDeploy;
    use crate::rust::cost_protocol_limits::offered_funded_v6_limits;
    use crate::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeLimits};
    use crate::rust::native_cost_evidence::{
        NativeCostEvidenceV1, NativeCostFailureClass, NativeFundingCaseSource, NativeFundingCaseV1,
        NativeFundingObligation, NativePrepaidDeltaV1,
    };
    use crate::rust::native_wallet_receipt::{
        NativeWalletReceiptLimits, NativeWalletReceiptRow, NativeWalletReceiptV1,
    };
    use crate::rust::phlo_wire::PhloWireLimits;

    let envelope_limits = DeployEnvelopeLimits {
        payload: limits(),
        members: NonZeroUsize::new(4).unwrap(),
    };
    let evidence_limits = PhloWireLimits {
        total_bytes: 4096,
        field_bytes: 1024,
    };
    let envelope = DeployEnvelope::from_proto(
        OfferedFundedDeploy::to_proto(&signed_offer(10, 2)).unwrap(),
        envelope_limits,
    )
    .unwrap();
    let commitment: [u8; 32] = envelope.identity().as_bytes().try_into().unwrap();
    let wallet_receipt = NativeWalletReceiptV1 {
        rows: vec![NativeWalletReceiptRow {
            address: b"purse",
            resource_rev: 14,
            fee_rev: 1,
            post_balance: 100,
        }],
        resource_rev: 14,
        fee_rev: 1,
    }
    .encode(NativeWalletReceiptLimits {
        wire: evidence_limits,
        payers: 4,
    })
    .unwrap();
    let section_limits = offered_funded_v6_limits();
    let funding_case = NativeFundingCaseV1 {
        sources: vec![NativeFundingCaseSource {
            custody: b"purse",
            capacity: 100,
            exposure_limit: 100,
            debit_limit: 100,
            hold: 15,
            debit: 15,
            fee: 1,
            refund: 0,
        }],
        obligations: vec![NativeFundingObligation {
            key: b"cost",
            quantity: 1,
            amount: 15,
        }],
        eligible: vec![vec![true]],
        assignment: vec![vec![15]],
        resource_next_cursor: Some(0),
        fee_next_cursor: Some(0),
        resource_unrestricted: true,
        resource_restriction_witness: None,
        possible_fee_payers: vec![true],
    }
    .encode(section_limits.funding_case)
    .unwrap();
    let prepaid_delta = NativePrepaidDeltaV1 {
        draws: Vec::new(),
        births: Vec::new(),
        replacements: Vec::new(),
    }
    .encode(section_limits.prepaid_delta)
    .unwrap();
    let evidence_record = NativeCostEvidenceV1 {
        wallet_settlement_log_events: 0,
        envelope_commitment: commitment,
        genesis_policy_commitment: [1; 32],
        schedule_commitment: [2; 32],
        original_funding_root: [3; 32],
        settlement_runtime_root: [4; 32],
        post_state_root: [5; 32],
        phlo_used: 7,
        fresh_phlo: 7,
        retained_phlo: 0,
        phlo_limit: 10,
        phlo_price: 2,
        fee_rev: 1,
        failure_class: NativeCostFailureClass::Success,
        budget_recording: b"budget",
        operation_journal: b"journal",
        funding_case: &funding_case,
        prepaid_delta: &prepaid_delta,
        wallet_settlement: &wallet_receipt,
    };
    let evidence = evidence_record.encode(evidence_limits).unwrap();
    let processed = OfferedProcessedDeploy::new(
        envelope,
        PCost { cost: 7 },
        Vec::new(),
        false,
        evidence,
        evidence_limits,
    )
    .unwrap();
    let proto = processed.to_proto(evidence_limits).unwrap();
    assert_eq!(
        OfferedProcessedDeploy::from_proto(proto.clone(), envelope_limits, evidence_limits)
            .unwrap(),
        processed
    );
    let decoded = ProcessedUserDeploy::from_proto(proto.clone()).unwrap();
    assert!(decoded.as_offered().is_some());
    assert_eq!(decoded.identity_bytes(), processed.identity_bytes());
    assert_eq!(decoded.to_proto(), proto);
    let mut oversized = proto.clone();
    oversized.native_cost_evidence =
        Some(vec![0; offered_funded_v6_limits().evidence.total_bytes + 1].into());
    assert!(ProcessedUserDeploy::from_proto(oversized).is_err());
    let mut malformed_wallet = evidence_record.clone();
    malformed_wallet.wallet_settlement = b"malformed";
    let mut malformed = proto.clone();
    malformed.native_cost_evidence = Some(malformed_wallet.encode(evidence_limits).unwrap().into());
    assert!(ProcessedUserDeploy::from_proto(malformed).is_err());
    let mut malformed_case = evidence_record.clone();
    malformed_case.funding_case = b"malformed";
    let mut malformed = proto.clone();
    malformed.native_cost_evidence = Some(malformed_case.encode(evidence_limits).unwrap().into());
    assert!(ProcessedUserDeploy::from_proto(malformed).is_err());
    let mut malformed_delta = evidence_record.clone();
    malformed_delta.prepaid_delta = b"malformed";
    let mut malformed = proto.clone();
    malformed.native_cost_evidence = Some(malformed_delta.encode(evidence_limits).unwrap().into());
    assert!(ProcessedUserDeploy::from_proto(malformed).is_err());
    let mismatched_wallet = NativeWalletReceiptV1 {
        rows: vec![NativeWalletReceiptRow {
            address: b"purse",
            resource_rev: 13,
            fee_rev: 1,
            post_balance: 100,
        }],
        resource_rev: 13,
        fee_rev: 1,
    }
    .encode(NativeWalletReceiptLimits {
        wire: evidence_limits,
        payers: 4,
    })
    .unwrap();
    let mut mismatched_evidence = evidence_record;
    mismatched_evidence.wallet_settlement = &mismatched_wallet;
    let mut mismatched = proto.clone();
    mismatched.native_cost_evidence =
        Some(mismatched_evidence.encode(evidence_limits).unwrap().into());
    assert!(ProcessedUserDeploy::from_proto(mismatched).is_err());
    let mut changed = proto;
    changed.cost = Some(PCost { cost: 8 });
    assert!(OfferedProcessedDeploy::from_proto(changed, envelope_limits, evidence_limits).is_err());
}

fn signed_offer(limit: i64, price: i64) -> Cosigned<OfferedFundedDeploy> {
    Cosigned::create_single_envelope(
        offered(limit, price),
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[1; 32]),
    )
    .unwrap()
}

#[test]
fn offered_envelope_accepts_canonical_v2_funding_without_changing_v1() {
    use crate::rust::phlo_intent::{
        PhloConversionCompositionV2, PhloFundingIntentV1, PhloFundingIntentV2,
        PhloFundingIntentV2Limits,
    };

    let original = funding(10, 3, 30);
    let base = PhloFundingIntentV1::decode(&original, limits().funding).unwrap();
    let v2_limits = PhloFundingIntentV2Limits {
        wire: limits().funding.wire,
        base: limits().funding,
        grant_uses: limits().funding.wire.total_bytes / 8,
        grant_id_bytes: limits().funding.wire.field_bytes,
        quote_evidence_bytes: limits().funding.wire.field_bytes,
    };
    let v2 = PhloFundingIntentV2 {
        base,
        grant_uses: Vec::new(),
        conversion: PhloConversionCompositionV2::NoConversion,
    }
    .encode(v2_limits)
    .unwrap();
    let envelope = Cosigned::create_single_envelope(
        OfferedFundedDeploy::new(body(), v2, 10, 2, limits()).unwrap(),
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[1; 32]),
    )
    .unwrap();
    let wire = OfferedFundedDeploy::to_proto(&envelope).unwrap();
    assert_eq!(
        OfferedFundedDeploy::from_proto(wire, limits()).unwrap(),
        envelope
    );
    assert_eq!(
        OfferedFundedDeploy::from_proto(
            OfferedFundedDeploy::to_proto(&signed_offer(10, 2)).unwrap(),
            limits(),
        )
        .unwrap(),
        signed_offer(10, 2),
    );
}

#[test]
fn offered_payload_matches_independent_framing_and_both_version_separations() {
    let data = offered(10, 2);
    let mut expected = vec![0, 3];
    for field in [
        b"f1r3node:offered-funded-deploy-intent:v1".as_slice(),
        &[0, 6, 0, 3],
        body().envelope_intent_v61().unwrap().as_slice(),
        data.funding_intent(),
        &10u64.to_be_bytes(),
        &2u64.to_be_bytes(),
    ] {
        expected.extend_from_slice(&(field.len() as u64).to_be_bytes());
        expected.extend_from_slice(field);
    }
    assert_eq!(data.envelope_intent_v61().unwrap(), expected);
    assert_ne!(expected, funded().envelope_intent_v61().unwrap());
    assert_ne!(expected, body().envelope_intent_v61().unwrap());
}

#[test]
fn offered_scalars_retain_original_protobuf_tags_and_numeric_boundaries() {
    let proto = DeployDataProto {
        phlo_price: 2,
        phlo_limit: 10,
        ..Default::default()
    };
    assert_eq!(proto.encode_to_vec(), [0x38, 2, 0x40, 10]);
    for limit in [0, 1, i64::MAX] {
        for price in [0, 1, i64::MAX] {
            let envelope = signed_offer(limit, price);
            let proto = OfferedFundedDeploy::to_proto(&envelope).unwrap();
            assert_eq!(proto.phlo_limit, limit);
            assert_eq!(proto.phlo_price, price);
            let wire = proto.encode_to_vec();
            let decoded = OfferedFundedDeploy::from_proto(
                DeployDataProto::decode(wire.as_slice()).unwrap(),
                limits(),
            )
            .unwrap();
            assert_eq!(decoded, envelope);
        }
    }
    for invalid in [i64::MIN, -1] {
        assert!(
            OfferedFundedDeploy::new(body(), funding(10, 3, 30), invalid, 1, limits()).is_err()
        );
        assert!(
            OfferedFundedDeploy::new(body(), funding(10, 3, 30), 10, invalid, limits()).is_err()
        );
    }
}

#[test]
fn offered_authorization_rejects_each_scalar_bit_mutation() {
    let proto = OfferedFundedDeploy::to_proto(&signed_offer(10, 2)).unwrap();
    for bit in 0..64 {
        let mut changed = proto.clone();
        changed.phlo_limit ^= 1i64 << bit;
        assert!(OfferedFundedDeploy::from_proto(changed, limits()).is_err());
        let mut changed = proto.clone();
        changed.phlo_price ^= 1i64 << bit;
        assert!(OfferedFundedDeploy::from_proto(changed, limits()).is_err());
    }
}

#[test]
fn offered_authorization_rejects_body_funding_identity_and_mixed_authorization_changes() {
    let proto = OfferedFundedDeploy::to_proto(&signed_offer(10, 2)).unwrap();
    for index in 0..proto.funding_intent.as_ref().unwrap().len() {
        let mut changed = proto.clone();
        let mut funding = changed.funding_intent.take().unwrap().to_vec();
        funding[index] ^= 1;
        changed.funding_intent = Some(funding.into());
        assert!(
            OfferedFundedDeploy::from_proto(changed, limits()).is_err(),
            "funding byte {index}"
        );
    }
    let mut changed = proto.clone();
    changed.term.push(' ');
    assert!(OfferedFundedDeploy::from_proto(changed, limits()).is_err());
    let mut changed = proto.clone();
    changed.deploy_id = vec![0; 32].into();
    assert!(OfferedFundedDeploy::from_proto(changed, limits()).is_err());
    let mut changed = proto.clone();
    changed.sig = vec![1].into();
    assert!(OfferedFundedDeploy::from_proto(changed, limits()).is_err());
    for funding in [None, Some(Vec::new().into())] {
        let mut changed = proto.clone();
        changed.funding_intent = funding;
        assert!(OfferedFundedDeploy::from_proto(changed, limits()).is_err());
    }
}

#[test]
fn offered_formats_cannot_be_transplanted_or_downgraded() {
    let proto = OfferedFundedDeploy::to_proto(&signed_offer(0, 0)).unwrap();
    assert!(DeployData::decode(proto.encode_to_vec()).is_err());
    assert!(DeployData::from_proto(proto.clone()).is_err());
    assert!(DeployData::from_proto_cosigned_legacy(proto.clone()).is_err());
    assert!(DeployData::from_proto_cosigned(proto.clone()).is_err());
    assert!(FundedDeploy::from_proto(proto.clone(), limits()).is_err());
    for version in [0, 1, 0x0006_0001, 0x0006_0002, u32::MAX] {
        let mut changed = proto.clone();
        changed.authorization_v61.as_mut().unwrap().format_version = version;
        assert!(OfferedFundedDeploy::from_proto(changed.clone(), limits()).is_err());
        assert!(FundedDeploy::from_proto(changed.clone(), limits()).is_err());
        changed.funding_intent = None;
        assert!(DeployData::from_proto_cosigned(changed).is_err());
    }
    let mut old_funded = FundedDeploy::to_proto(&signed()).unwrap();
    old_funded
        .authorization_v61
        .as_mut()
        .unwrap()
        .format_version = OFFERED_FUNDED_DEPLOY_AUTHORIZATION_VERSION;
    assert!(OfferedFundedDeploy::from_proto(old_funded, limits()).is_err());
    let old_body = Cosigned::create_single_envelope(
        body(),
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[1; 32]),
    )
    .unwrap();
    let mut transplanted = DeployData::to_proto_cosigned(&old_body);
    transplanted
        .authorization_v61
        .as_mut()
        .unwrap()
        .format_version = OFFERED_FUNDED_DEPLOY_AUTHORIZATION_VERSION;
    transplanted.funding_intent = proto.funding_intent;
    assert!(OfferedFundedDeploy::from_proto(transplanted, limits()).is_err());
}

#[test]
fn existing_decoders_reject_added_unsigned_offer_scalars() {
    let old_body = Cosigned::create_single_envelope(
        body(),
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[1; 32]),
    )
    .unwrap();
    let legacy = Signed::create(
        body(),
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[1; 32]),
    )
    .unwrap();
    for original in [
        DeployData::to_proto_cosigned(&old_body),
        DeployData::to_proto(legacy),
    ] {
        for (limit, price) in [(1, 0), (0, 1), (-1, 0), (0, -1), (i64::MAX, i64::MAX)] {
            let mut proto = original.clone();
            proto.phlo_limit = limit;
            proto.phlo_price = price;
            assert!(DeployData::decode(proto.encode_to_vec()).is_err());
            assert!(DeployData::from_proto(proto.clone()).is_err());
            assert!(DeployData::from_proto_cosigned_legacy(proto.clone()).is_err());
            assert!(DeployData::from_proto_cosigned(proto).is_err());
            let mut funded = FundedDeploy::to_proto(&signed()).unwrap();
            funded.phlo_limit = limit;
            funded.phlo_price = price;
            assert!(FundedDeploy::from_proto(funded, limits()).is_err());
        }
    }
}

#[test]
fn offered_byte_limits_include_both_scalars_and_full_signing_payload() {
    let envelope = signed_offer(i64::MAX, i64::MAX);
    let proto = OfferedFundedDeploy::to_proto(&envelope).unwrap();
    let mut bounded = limits();
    bounded.deploy_bytes = proto.encoded_len();
    assert!(OfferedFundedDeploy::from_proto(proto.clone(), bounded).is_ok());
    bounded.deploy_bytes -= 1;
    assert!(OfferedFundedDeploy::from_proto(proto, bounded).is_err());
    bounded = limits();
    bounded.signing.total_bytes = envelope.data.envelope_intent_v61().unwrap().len();
    assert!(
        OfferedFundedDeploy::new(body(), funding(10, 3, 30), i64::MAX, i64::MAX, bounded).is_ok()
    );
    bounded.signing.total_bytes -= 1;
    assert!(
        OfferedFundedDeploy::new(body(), funding(10, 3, 30), i64::MAX, i64::MAX, bounded).is_err()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn full_width_offers_bind_identity_and_round_trip(limit in 0i64..=i64::MAX, price in 0i64..=i64::MAX) {
        let envelope = signed_offer(limit, price);
        let altered = signed_offer(limit, price ^ 1);
        prop_assert_ne!(envelope.envelope_commitment().unwrap(), altered.envelope_commitment().unwrap());
        let proto = OfferedFundedDeploy::to_proto(&envelope).unwrap();
        prop_assert_eq!(OfferedFundedDeploy::from_proto(proto, limits()).unwrap(), envelope);
    }
}

#[test]
fn offered_threshold_preserves_arbitrary_member_counts_and_exact_witness_selection() {
    for count in [1usize, 3, 17, 65] {
        let envelope = threshold_envelope(offered(10, 2), count);
        let proto = OfferedFundedDeploy::to_proto(&envelope).unwrap();
        assert_eq!(
            proto.authorization_v61.as_ref().unwrap().witnesses.len(),
            count.div_ceil(2)
        );
        assert_eq!(
            OfferedFundedDeploy::from_proto(proto, limits()).unwrap(),
            envelope
        );
    }
}

#[test]
fn offered_ethereum_signatures_bind_the_same_offer_fields() {
    let envelope = Cosigned::create_single_envelope(
        offered(10, 2),
        Box::new(Secp256k1Eth),
        PrivateKey::from_bytes(&[1; 32]),
    )
    .unwrap();
    let mut proto = OfferedFundedDeploy::to_proto(&envelope).unwrap();
    assert_eq!(
        OfferedFundedDeploy::from_proto(proto.clone(), limits()).unwrap(),
        envelope
    );
    proto.phlo_price = 1;
    assert!(OfferedFundedDeploy::from_proto(proto, limits()).is_err());
}
