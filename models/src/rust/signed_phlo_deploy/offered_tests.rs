use super::*;

fn offered(limit: i64, price: i64) -> OfferedFundedDeploy {
    OfferedFundedDeploy::new(body(), funding(10, 3, 30), limit, price, limits()).unwrap()
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
