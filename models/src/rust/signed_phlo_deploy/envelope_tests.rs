use std::num::NonZeroUsize;

use super::*;
use crate::rust::deploy_envelope::{
    DeployEnvelope, DeployEnvelopeFormat, DeployEnvelopeLimits, DeployEnvelopeRef,
    StoredDeployEnvelope,
};
use crate::rust::deploy_id::{DeployIdV6, DeployLookupId, LegacyDeploySignature};

fn envelope_limits() -> DeployEnvelopeLimits {
    DeployEnvelopeLimits {
        payload: limits(),
        members: NonZeroUsize::new(256).unwrap(),
    }
}

#[test]
fn normalizer_context_preserves_each_authenticated_envelope_identity() {
    use crate::rhoapi::g_unforgeable::UnfInstance;
    use crate::rust::normalizer_env::{
        normalizer_env_from_cosigned_deploy, normalizer_env_from_envelope,
    };

    for (format, proto) in variants() {
        let envelope = DeployEnvelope::from_proto(proto.clone(), envelope_limits()).unwrap();
        let env = normalizer_env_from_envelope(&envelope);
        let id = &env["rho:system:deployId"].unforgeables[0].unf_instance;
        assert!(
            matches!(id, Some(UnfInstance::GDeployIdBody(id)) if id.sig == envelope.identity().as_bytes())
        );
        assert_eq!(env["rho:system:deployId"], env["rho:rchain:deployId"]);
        match format {
            DeployEnvelopeFormat::Legacy | DeployEnvelopeFormat::BodyV61 => {
                assert_eq!(
                    env,
                    normalizer_env_from_cosigned_deploy(envelope.body_envelope().unwrap())
                );
            }
            DeployEnvelopeFormat::Funded | DeployEnvelopeFormat::OfferedFunded => {
                assert!(envelope.body_envelope().is_err());
                assert!(env.contains_key("rho:system:authorityId"));
                assert!(env.contains_key("rho:system:deployerId"));
            }
        }
        let roundtrip =
            DeployEnvelope::from_proto(envelope.to_proto().unwrap(), envelope_limits()).unwrap();
        assert_eq!(env, normalizer_env_from_envelope(&roundtrip));
        assert_eq!(envelope.to_proto().unwrap(), proto);
    }
}

#[test]
fn funded_normalizer_context_separates_signers_from_policy_members() {
    use crate::rhoapi::expr::ExprInstance;
    use crate::rust::normalizer_env::normalizer_env_from_envelope;

    for count in [1, 2, 3, 8, 9, 33, 65] {
        let signed = threshold_envelope(
            OfferedFundedDeploy::new(body(), funding(10, 3, 30), 10, 2, limits()).unwrap(),
            count,
        );
        let envelope = DeployEnvelope::from_proto(
            OfferedFundedDeploy::to_proto(&signed).unwrap(),
            envelope_limits(),
        )
        .unwrap();
        let env = normalizer_env_from_envelope(&envelope);
        for (name, expected) in [
            ("rho:system:signers", count.div_ceil(2)),
            ("rho:system:policyMembers", count),
        ] {
            match &env[name].exprs[0].expr_instance {
                Some(ExprInstance::EListBody(list)) => assert_eq!(list.ps.len(), expected),
                other => panic!("expected principal descriptor list, got {other:?}"),
            }
        }
        assert_eq!(
            env.contains_key("rho:system:deployerId"),
            count.div_ceil(2) == 1
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn changed_funding_terms_change_deploy_context_not_signer_authority(
        limit in 0_i64..=i64::MAX,
        price in 0_i64..=i64::MAX,
        change_limit in any::<bool>(),
    ) {
        use crate::rust::normalizer_env::normalizer_env_from_envelope;
        let make = |limit, price| {
            let signed = Cosigned::create_single_envelope(
                OfferedFundedDeploy::new(body(), funding(10, 3, 30), limit, price, limits()).unwrap(),
                Box::new(Secp256k1),
                PrivateKey::from_bytes(&[1; 32]),
            ).unwrap();
            DeployEnvelope::from_proto(OfferedFundedDeploy::to_proto(&signed).unwrap(), envelope_limits()).unwrap()
        };
        let first = make(limit, price);
        let second = if change_limit { make(limit ^ 1, price) } else { make(limit, price ^ 1) };
        let mut first_env = normalizer_env_from_envelope(&first);
        let mut second_env = normalizer_env_from_envelope(&second);
        prop_assert_ne!(first_env.remove("rho:system:deployId"), second_env.remove("rho:system:deployId"));
        prop_assert_ne!(first_env.remove("rho:rchain:deployId"), second_env.remove("rho:rchain:deployId"));
        prop_assert_eq!(first_env, second_env);
    }
}

fn variants() -> Vec<(DeployEnvelopeFormat, DeployDataProto)> {
    let key = PrivateKey::from_bytes(&[1; 32]);
    let legacy = Signed::create(body(), Box::new(Secp256k1), key.clone()).unwrap();
    let plain = Cosigned::create_single_envelope(body(), Box::new(Secp256k1), key.clone()).unwrap();
    let offered = Cosigned::create_single_envelope(
        OfferedFundedDeploy::new(body(), funding(10, 3, 30), 10, 1, limits()).unwrap(),
        Box::new(Secp256k1),
        key,
    )
    .unwrap();
    vec![
        (DeployEnvelopeFormat::Legacy, DeployData::to_proto(legacy)),
        (
            DeployEnvelopeFormat::BodyV61,
            DeployData::to_proto_cosigned(&plain),
        ),
        (
            DeployEnvelopeFormat::Funded,
            FundedDeploy::to_proto(&signed()).unwrap(),
        ),
        (
            DeployEnvelopeFormat::OfferedFunded,
            OfferedFundedDeploy::to_proto(&offered).unwrap(),
        ),
    ]
}

fn block_proto_with_envelopes(deploys: Vec<DeployDataProto>) -> crate::casper::BlockMessageProto {
    use crate::casper::{
        BlockMessageProto, BodyProto, HeaderProto, ProcessedDeployProto, RChainStateProto,
    };

    BlockMessageProto {
        header: Some(HeaderProto {
            version: 6,
            ..Default::default()
        }),
        body: Some(BodyProto {
            state: Some(RChainStateProto::default()),
            deploys: deploys
                .into_iter()
                .map(|deploy| ProcessedDeployProto {
                    deploy: Some(deploy),
                    cost: Some(crate::rhoapi::PCost { cost: 7 }),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn approval_containers_propagate_explicit_envelope_limits() {
    use crate::casper::{
        ApprovedBlockCandidateProto, ApprovedBlockProto, BlockApprovalProto, UnapprovedBlockProto,
    };
    use crate::rust::casper::protocol::casper_message::{
        ApprovedBlock, ApprovedBlockCandidate, BlockApproval, UnapprovedBlock,
    };

    for (_, deploy) in variants().into_iter().skip(2) {
        let block = block_proto_with_envelopes(vec![deploy.clone()]);
        let candidate = ApprovedBlockCandidateProto {
            block: Some(block),
            required_sigs: 2,
        };
        let approved = ApprovedBlockProto {
            candidate: Some(candidate.clone()),
            ..Default::default()
        };
        let approval = BlockApprovalProto {
            candidate: Some(candidate.clone()),
            sig: Some(Default::default()),
        };
        let unapproved = UnapprovedBlockProto {
            candidate: Some(candidate.clone()),
            timestamp: 17,
            duration: 19,
        };
        let limits = envelope_limits();
        assert!(ApprovedBlockCandidate::from_proto(candidate.clone()).is_err());
        assert!(ApprovedBlock::from_proto(approved.clone()).is_err());
        assert!(BlockApproval::from_proto(approval.clone()).is_err());
        assert!(UnapprovedBlock::from_proto(unapproved.clone()).is_err());
        assert_eq!(
            ApprovedBlockCandidate::from_proto_with_limits(candidate.clone(), limits)
                .unwrap()
                .to_proto(),
            candidate
        );
        assert_eq!(
            ApprovedBlock::from_proto_with_limits(approved.clone(), limits)
                .unwrap()
                .to_proto(),
            approved
        );
        assert_eq!(
            BlockApproval::from_proto_with_limits(approval.clone(), limits)
                .unwrap()
                .to_proto(),
            approval
        );
        assert_eq!(
            UnapprovedBlock::from_proto_with_limits(unapproved.clone(), limits)
                .unwrap()
                .to_proto(),
            unapproved
        );
        let mut too_small = limits;
        too_small.payload.deploy_bytes = deploy.encoded_len() - 1;
        assert!(ApprovedBlockCandidate::from_proto_with_limits(candidate, too_small).is_err());
        assert!(ApprovedBlock::from_proto_with_limits(approved, too_small).is_err());
        assert!(BlockApproval::from_proto_with_limits(approval, too_small).is_err());
        assert!(UnapprovedBlock::from_proto_with_limits(unapproved, too_small).is_err());
    }
}

#[test]
fn nested_decoding_enforces_full_policy_membership_limit() {
    use crate::rust::casper::protocol::casper_message::{BlockMessage, Body};

    let envelope = absent_first(body());
    let wire = block_proto_with_envelopes(vec![DeployData::to_proto_cosigned(&envelope)]);
    let mut exact = envelope_limits();
    exact.members = NonZeroUsize::new(envelope.signers().len()).unwrap();
    assert!(BlockMessage::from_proto_with_limits(wire.clone(), exact).is_ok());
    let mut too_small = exact;
    too_small.members = NonZeroUsize::new(envelope.signers().len() - 1).unwrap();
    assert!(BlockMessage::from_proto_with_limits(wire.clone(), too_small).is_err());
    assert!(Body::from_proto_with_limits(wire.body.unwrap(), too_small).is_err());
}

#[test]
fn processed_records_retain_all_formats_and_require_explicit_funded_limits() {
    use crate::rust::casper::protocol::casper_message::ProcessedDeploy;
    for (format, proto) in variants() {
        let checked = DeployEnvelope::from_proto(proto.clone(), envelope_limits()).unwrap();
        let mut processed = ProcessedDeploy::from_envelope(checked.clone());
        processed.cost.cost = 7;
        processed.is_failed = true;
        processed.system_deploy_error = Some("body failed".to_string());
        processed.pre_state_hash = vec![3; 32].into();
        processed.post_state_hash = vec![4; 32].into();
        let wire = processed.clone().to_proto();
        assert_eq!(wire.deploy.as_ref().unwrap(), &proto);
        assert_eq!(
            ProcessedDeploy::from_proto_with_limits(wire.clone(), envelope_limits()).unwrap(),
            processed
        );
        assert_eq!(processed.envelope(), &checked);
        let body_only = matches!(
            format,
            DeployEnvelopeFormat::Legacy | DeployEnvelopeFormat::BodyV61
        );
        assert_eq!(ProcessedDeploy::from_proto(wire.clone()).is_ok(), body_only);
        assert_eq!(processed.to_cosigned().is_ok(), body_only);
        let mut too_small = envelope_limits();
        too_small.payload.deploy_bytes = proto.encoded_len() - 1;
        assert!(ProcessedDeploy::from_proto_with_limits(wire.clone(), too_small).is_err());
        let mut corrupt = wire;
        corrupt.deploy.as_mut().unwrap().term.push_str(" | Nil");
        assert!(ProcessedDeploy::from_proto_with_limits(corrupt, envelope_limits()).is_err());
    }
}

#[test]
fn processed_constructors_reject_mutated_signed_inputs() {
    use crate::rust::casper::protocol::casper_message::ProcessedDeploy;
    let mut signed = Signed::create(
        body(),
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[1; 32]),
    )
    .unwrap();
    signed.data.term.push_str(" | Nil");
    assert!(ProcessedDeploy::empty(signed).is_err());
    let mut envelope = absent_first(body());
    envelope.data.term.push_str(" | Nil");
    assert!(ProcessedDeploy::empty_from_cosigned(&envelope).is_err());
}

#[test]
fn processed_decoding_rejects_bound_authorization_substitution() {
    use crate::rust::casper::protocol::casper_message::ProcessedDeploy;

    for (format, original) in variants().into_iter().skip(1) {
        let checked = DeployEnvelope::from_proto(original, envelope_limits()).unwrap();
        let wire = ProcessedDeploy::from_envelope(checked).to_proto();
        for axis in 0..8 {
            let mut changed = wire.clone();
            let deploy = changed.deploy.as_mut().unwrap();
            if axis == 0 {
                let mut identity = deploy.deploy_id.to_vec();
                identity[0] ^= 1;
                deploy.deploy_id = identity.into();
            } else {
                let authorization = deploy.authorization_v61.as_mut().unwrap();
                match axis {
                    1 => authorization.format_version = u32::MAX,
                    2 => {
                        let mut presence = authorization.presence_bitmap.to_vec();
                        presence[0] ^= 1;
                        authorization.presence_bitmap = presence.into();
                    }
                    3 => {
                        let mut signature = authorization.witnesses[0].signature.to_vec();
                        signature[0] ^= 1;
                        authorization.witnesses[0].signature = signature.into();
                    }
                    4 => authorization.witnesses[0].member_index = u32::MAX,
                    5 => authorization.policy = None,
                    6 => authorization
                        .witnesses
                        .push(authorization.witnesses[0].clone()),
                    7 => authorization.witnesses.clear(),
                    _ => unreachable!(),
                }
            }
            assert!(
                ProcessedDeploy::from_proto_with_limits(changed, envelope_limits()).is_err(),
                "{format:?} accepted authorization mutation {axis}"
            );
        }
    }
}

#[test]
fn processed_identity_requires_its_protocol_domain() {
    use crate::rust::block_metadata::CERTIFIED_ADMISSION_PROTOCOL_VERSION;
    use crate::rust::casper::protocol::casper_message::ProcessedDeploy;

    for (format, wire) in variants() {
        let envelope = DeployEnvelope::from_proto(wire, envelope_limits()).unwrap();
        let processed = ProcessedDeploy::from_envelope(envelope.clone());
        let legacy = format == DeployEnvelopeFormat::Legacy;
        let pre_v6 = processed.deploy_id_for_protocol(CERTIFIED_ADMISSION_PROTOCOL_VERSION - 1);
        let v6 = processed.deploy_id_for_protocol(CERTIFIED_ADMISSION_PROTOCOL_VERSION);
        assert_eq!(pre_v6.is_ok(), legacy);
        assert_eq!(v6.is_ok(), !legacy);
        let identity = if legacy { pre_v6 } else { v6 }.unwrap();
        assert_eq!(&identity, envelope.identity());
    }
}

#[test]
fn historical_processed_additional_algorithm_alias_keeps_wire_spelling() {
    use crypto::rust::signatures::secp256k1_eth::Secp256k1Eth;

    use crate::rust::casper::protocol::casper_message::ProcessedDeploy;
    let signers = (1..=2)
        .map(|i| {
            let signed = Signed::create(
                body(),
                Box::new(Secp256k1Eth),
                PrivateKey::from_bytes(&[i; 32]),
            )
            .unwrap();
            Cosigner {
                pk: signed.pk,
                sig: signed.sig,
                sig_algorithm: signed.sig_algorithm,
            }
        })
        .collect();
    let envelope = Cosigned::from_signed_data(body(), signers).unwrap();
    let mut proto = ProcessedDeploy::empty_from_cosigned(&envelope)
        .unwrap()
        .to_proto();
    proto.deploy.as_mut().unwrap().cosigners[0].sig_algorithm =
        Secp256k1Eth::LEGACY_NAME.to_string();
    let restored = ProcessedDeploy::from_proto(proto.clone()).unwrap();
    assert_eq!(
        restored.signers()[1].sig_algorithm.name(),
        Secp256k1Eth::NAME
    );
    assert_eq!(restored.to_proto(), proto);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn nested_decoding_preserves_order_and_rejects_any_invalid_record(
        formats in proptest::collection::vec(0usize..4, 0..13),
        selected in any::<usize>(),
    ) {
        use crate::rust::casper::protocol::casper_message::{BlockMessage, Body};

        let templates = variants();
        let deploys: Vec<_> = formats.iter().map(|index| templates[*index].1.clone()).collect();
        let original = block_proto_with_envelopes(deploys);
        let decoded = BlockMessage::from_proto_with_limits(original.clone(), envelope_limits()).unwrap();
        prop_assert_eq!(decoded.body.deploys.len(), formats.len());
        prop_assert_eq!(decoded.to_proto(), original.clone());
        let body = original.body.clone().unwrap();
        prop_assert_eq!(Body::from_proto_with_limits(body.clone(), envelope_limits()).unwrap().to_proto(), body);
        if !formats.is_empty() {
            let index = selected % formats.len();
            let mut changed = original.clone();
            changed.body.as_mut().unwrap().deploys[index].deploy.as_mut().unwrap().term.push_str(" | Nil");
            prop_assert!(BlockMessage::from_proto_with_limits(changed.clone(), envelope_limits()).is_err());
            prop_assert!(Body::from_proto_with_limits(changed.body.unwrap(), envelope_limits()).is_err());
            let mut limited = envelope_limits();
            limited.payload.deploy_bytes = original.body.as_ref().unwrap().deploys[index].deploy.as_ref().unwrap().encoded_len() - 1;
            prop_assert!(BlockMessage::from_proto_with_limits(original, limited).is_err());
        }
    }

    #[test]
    fn processed_receipt_updates_preserve_signed_authority(
        variant in 0usize..4,
        updates in proptest::collection::vec((any::<u64>(), any::<bool>(), any::<[u8; 32]>()), 0..16),
    ) {
        use crate::rust::casper::protocol::casper_message::ProcessedDeploy;
        let (_, proto) = variants().swap_remove(variant);
        let envelope = DeployEnvelope::from_proto(proto, envelope_limits()).unwrap();
        let mut processed = ProcessedDeploy::from_envelope(envelope.clone());
        for (cost, failed, root) in updates {
            processed.cost.cost = cost;
            processed.is_failed = failed;
            processed.post_state_hash = root.to_vec().into();
            prop_assert_eq!(processed.envelope(), &envelope);
            prop_assert_eq!(processed.typed_deploy_id(), envelope.identity());
            let restored = ProcessedDeploy::from_proto_with_limits(processed.clone().to_proto(), envelope_limits()).unwrap();
            prop_assert_eq!(&restored, &processed);
            processed = restored;
        }
    }

    #[test]
    fn processed_disjoint_receipt_updates_commute(
        first in (any::<u64>(), any::<bool>(), any::<[u8; 32]>()),
        second in (any::<u64>(), any::<bool>(), any::<[u8; 32]>()),
    ) {
        use crate::rust::casper::protocol::casper_message::ProcessedDeploy;

        let original: Vec<_> = variants().into_iter().take(2)
            .map(|(_, proto)| ProcessedDeploy::from_envelope(
                DeployEnvelope::from_proto(proto, envelope_limits()).unwrap()
            ))
            .collect();
        prop_assert_ne!(original[0].typed_deploy_id(), original[1].typed_deploy_id());
        let update = |records: &mut Vec<ProcessedDeploy>, index: usize, (cost, failed, root): (u64, bool, [u8; 32])| {
            records[index].cost.cost = cost;
            records[index].is_failed = failed;
            records[index].post_state_hash = root.to_vec().into();
        };
        let mut forward = original.clone();
        update(&mut forward, 0, first);
        update(&mut forward, 1, second);
        let mut reverse = original.clone();
        update(&mut reverse, 1, second);
        update(&mut reverse, 0, first);
        prop_assert_eq!(&forward, &reverse);
        for (before, after) in original.iter().zip(&forward) {
            prop_assert_eq!(before.envelope(), after.envelope());
            let restored = ProcessedDeploy::from_proto_with_limits(after.clone().to_proto(), envelope_limits()).unwrap();
            prop_assert_eq!(after, &restored);
        }
    }
}

#[test]
fn body_adapter_preserves_signed_payloads_and_rejects_funded_formats() {
    for (format, proto) in variants() {
        let checked = DeployEnvelope::from_proto(proto.clone(), envelope_limits()).unwrap();
        if matches!(
            format,
            DeployEnvelopeFormat::Legacy | DeployEnvelopeFormat::BodyV61
        ) {
            let body = checked.clone().into_body_envelope().unwrap();
            assert_eq!(checked.body_envelope().unwrap(), &body);
            let restored = DeployEnvelope::from_body_envelope(body.clone()).unwrap();
            assert_eq!(restored, checked);
            assert_eq!(restored.to_proto().unwrap(), proto);
            let mut tampered = body;
            tampered.data.term.push_str(" | Nil");
            assert!(DeployEnvelope::from_body_envelope(tampered).is_err());
        } else {
            assert!(checked.body_envelope().is_err());
            assert!(checked.clone().into_body_envelope().is_err());
            assert_eq!(checked.to_proto().unwrap(), proto);
        }
    }
}

#[test]
fn checked_envelope_and_stored_record_preserve_all_formats_without_activation() {
    for (format, proto) in variants() {
        let original = proto.encode_to_vec();
        let checked = DeployEnvelope::decode(&original, envelope_limits()).unwrap();
        assert_eq!(checked.format(), format);
        assert_eq!(checked.body(), &body());
        assert_eq!(checked.to_proto().unwrap(), proto);
        assert_eq!(checked.to_proto().unwrap().encode_to_vec(), original);
        assert!(checked.require_format(&[]).is_err());
        assert!(checked.require_format(&[format]).is_ok());
        for other in [
            DeployEnvelopeFormat::Legacy,
            DeployEnvelopeFormat::BodyV61,
            DeployEnvelopeFormat::Funded,
            DeployEnvelopeFormat::OfferedFunded,
        ] {
            assert_eq!(checked.require_format(&[other]).is_ok(), format == other);
        }
        let stored = StoredDeployEnvelope::new(&checked).unwrap();
        let serialized = bincode::serialize(&stored).unwrap();
        let restored: StoredDeployEnvelope = bincode::deserialize(&serialized).unwrap();
        assert_eq!(
            restored
                .decode(checked.identity(), envelope_limits())
                .unwrap(),
            checked
        );
        let wrong = DeployLookupId::V6(DeployIdV6::try_from([7; 32].as_slice()).unwrap());
        assert!(restored.decode(&wrong, envelope_limits()).is_err());
        if format != DeployEnvelopeFormat::Legacy {
            let wrong_kind = DeployLookupId::Legacy(LegacyDeploySignature::new(
                checked.identity().as_bytes().to_vec(),
            ));
            assert!(restored.decode(&wrong_kind, envelope_limits()).is_err());
        }
        let mut too_small = envelope_limits();
        too_small.payload.deploy_bytes = original.len() - 1;
        assert!(DeployEnvelope::decode(&original, too_small).is_err());
        assert!(restored.decode(checked.identity(), too_small).is_err());
    }
}

#[test]
fn legacy_wire_primary_survives_canonical_signer_reordering() {
    let signers = (1..=3)
        .map(|i| {
            let signed = Signed::create(
                body(),
                Box::new(Secp256k1),
                PrivateKey::from_bytes(&[i; 32]),
            )
            .unwrap();
            Cosigner {
                pk: signed.pk,
                sig: signed.sig,
                sig_algorithm: signed.sig_algorithm,
            }
        })
        .collect();
    let legacy = Cosigned::from_signed_data(body(), signers).unwrap();
    let mut proto = DeployData::to_proto_cosigned(&legacy);
    let mut other = proto.cosigners.remove(1);
    std::mem::swap(&mut proto.deployer, &mut other.pk);
    std::mem::swap(&mut proto.sig, &mut other.sig);
    std::mem::swap(&mut proto.sig_algorithm, &mut other.sig_algorithm);
    proto.cosigners.insert(0, other);
    let checked = DeployEnvelope::from_proto(proto.clone(), envelope_limits()).unwrap();
    assert_eq!(checked.primary().sig, proto.sig);
    assert_ne!(checked.primary().sig, checked.signers()[0].sig);
    let env = crate::rust::normalizer_env::normalizer_env_from_envelope(&checked);
    assert!(matches!(
        &env["rho:system:deployId"].unforgeables[0].unf_instance,
        Some(crate::rhoapi::g_unforgeable::UnfInstance::GDeployIdBody(id)) if id.sig == proto.sig.as_ref()
    ));
    assert!(matches!(
        &env["rho:system:deployerId"].unforgeables[0].unf_instance,
        Some(crate::rhoapi::g_unforgeable::UnfInstance::GDeployerIdBody(id)) if id.public_key == proto.deployer
    ));
    assert!(checked.body_envelope().is_err());
    assert!(checked.clone().into_body_envelope().is_err());
    assert_eq!(checked.identity().as_bytes(), proto.sig.as_ref());
    let stored = StoredDeployEnvelope::new(&checked).unwrap();
    assert_eq!(
        stored
            .decode(checked.identity(), envelope_limits())
            .unwrap(),
        checked
    );
    assert_eq!(checked.to_proto().unwrap(), proto);
}

#[test]
fn legacy_wire_cosigner_order_survives_checked_round_trip() {
    let signers = (1..=3)
        .map(|i| {
            let signed = Signed::create(
                body(),
                Box::new(Secp256k1),
                PrivateKey::from_bytes(&[i; 32]),
            )
            .unwrap();
            Cosigner {
                pk: signed.pk,
                sig: signed.sig,
                sig_algorithm: signed.sig_algorithm,
            }
        })
        .collect();
    let legacy = Cosigned::from_signed_data(body(), signers).unwrap();
    let mut proto = DeployData::to_proto_cosigned(&legacy);
    proto.cosigners.reverse();
    let historical =
        crate::rust::casper::protocol::casper_message::ProcessedDeploy::empty_from_cosigned(
            &legacy,
        )
        .unwrap();
    let mut processed_proto = historical.to_proto();
    processed_proto.deploy = Some(proto.clone());
    assert_eq!(
        crate::rust::casper::protocol::casper_message::ProcessedDeploy::from_proto(
            processed_proto.clone()
        )
        .unwrap()
        .to_proto(),
        processed_proto,
    );
    let checked = DeployEnvelope::from_proto(proto.clone(), envelope_limits()).unwrap();
    assert_eq!(checked.to_proto().unwrap(), proto);
}

fn legacy_proto_for_order(order: &[usize]) -> DeployDataProto {
    let signers = (1..=order.len())
        .map(|i| {
            let key_byte = u8::try_from(i).unwrap();
            let signed = Signed::create(
                body(),
                Box::new(Secp256k1),
                PrivateKey::from_bytes(&[key_byte; 32]),
            )
            .unwrap();
            Cosigner {
                pk: signed.pk,
                sig: signed.sig,
                sig_algorithm: signed.sig_algorithm,
            }
        })
        .collect();
    let envelope = Cosigned::from_signed_data(body(), signers).unwrap();
    let mut proto = DeployData::to_proto_cosigned(&envelope);
    let primary = &envelope.signers()[order[0]];
    proto.deployer = primary.pk.bytes.clone().into();
    proto.sig = primary.sig.clone();
    proto.sig_algorithm = primary.sig_algorithm.name();
    proto.cosigners = order
        .iter()
        .skip(1)
        .map(|index| &envelope.signers()[*index])
        .map(|signer| crate::casper::CompoundSigner {
            pk: signer.pk.bytes.clone().into(),
            sig: signer.sig.clone(),
            sig_algorithm: signer.sig_algorithm.name(),
        })
        .collect();
    proto
}

#[test]
fn all_three_member_legacy_orders_preserve_wire_and_identity() {
    for order in [[0, 1, 2], [0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [
        2, 1, 0,
    ]] {
        let proto = legacy_proto_for_order(&order);
        let envelope = DeployEnvelope::from_proto(proto.clone(), envelope_limits()).unwrap();
        assert_eq!(
            envelope.to_proto().unwrap().encode_to_vec(),
            proto.encode_to_vec()
        );
        assert_eq!(envelope.identity().as_bytes(), proto.sig.as_ref());
        assert_eq!(envelope.primary().pk.bytes[..], proto.deployer[..]);
        assert_eq!(envelope.body_envelope().is_ok(), order == [0, 1, 2]);
        let stored = StoredDeployEnvelope::new(&envelope).unwrap();
        assert_eq!(
            stored
                .decode(envelope.identity(), envelope_limits())
                .unwrap(),
            envelope
        );
    }
}

#[test]
fn legacy_algebra_uses_verified_members_not_flat_hints() {
    use crate::casper::sig_compound::Connective;
    let mut proto = legacy_proto_for_order(&[1, 0]);
    let other = proto.cosigners.remove(0);
    let atom = |pk, sig, sig_algorithm| crate::casper::SigCompound {
        connective: Some(Connective::Atom(crate::casper::SigAtom {
            pk,
            sig,
            sig_algorithm,
            ..Default::default()
        })),
    };
    proto.sig_algebra = Some(crate::casper::SigCompound {
        connective: Some(Connective::Tensor(Box::new(crate::casper::SigPair {
            left: Some(Box::new(atom(
                proto.deployer.clone(),
                proto.sig.clone(),
                proto.sig_algorithm.clone(),
            ))),
            right: Some(Box::new(atom(other.pk, other.sig, other.sig_algorithm))),
        }))),
    });
    let verified = DeployData::from_proto_cosigned_legacy(proto.clone()).unwrap();
    assert_eq!(verified.signers().len(), 2);
    let checked = DeployEnvelope::from_proto(proto.clone(), envelope_limits()).unwrap();
    assert_eq!(checked.signers(), verified.signers());
    assert_eq!(checked.identity().as_bytes(), proto.sig.as_ref());
    let stored = StoredDeployEnvelope::new(&checked).unwrap();
    assert_eq!(
        stored
            .decode(checked.identity(), envelope_limits())
            .unwrap(),
        checked
    );
}

#[test]
fn historical_processed_flat_authority_does_not_switch_to_algebra() {
    use crate::casper::sig_compound::Connective;
    use crate::rust::casper::protocol::casper_message::ProcessedDeploy;
    let mut flat = legacy_proto_for_order(&[0, 2, 1]);
    flat.cosigner_threshold = 2;
    let envelope = DeployData::from_proto_cosigned_legacy(flat.clone()).unwrap();
    let mut processed = ProcessedDeploy::empty_from_cosigned(&envelope)
        .unwrap()
        .to_proto();
    processed.deploy = Some(flat.clone());
    let reference = ProcessedDeploy::from_proto(processed.clone())
        .unwrap()
        .to_proto();
    let invalid_algebra = crate::casper::SigCompound {
        connective: Some(Connective::Plus(Box::default())),
    };
    let atom = |pk, sig, sig_algorithm| crate::casper::SigCompound {
        connective: Some(Connective::Atom(crate::casper::SigAtom {
            pk,
            sig,
            sig_algorithm,
            ..Default::default()
        })),
    };
    let different_threshold = crate::casper::SigCompound {
        connective: Some(Connective::Threshold(crate::casper::SigThreshold {
            threshold: 1,
            members: std::iter::once(atom(
                flat.deployer.clone(),
                flat.sig.clone(),
                flat.sig_algorithm.clone(),
            ))
            .chain(flat.cosigners.iter().map(|signer| {
                atom(
                    signer.pk.clone(),
                    signer.sig.clone(),
                    signer.sig_algorithm.clone(),
                )
            }))
            .collect(),
        })),
    };
    for (algebra, ingress_threshold) in [(invalid_algebra, None), (different_threshold, Some(1))] {
        let mut candidate = flat.clone();
        candidate.sig_algebra = Some(algebra);
        let ingress = DeployData::from_proto_cosigned_legacy(candidate.clone());
        assert_eq!(
            ingress.ok().map(|envelope| envelope.cosigner_threshold()),
            ingress_threshold
        );
        let mut candidate_processed = processed.clone();
        candidate_processed.deploy = Some(candidate);
        let restored = ProcessedDeploy::from_proto(candidate_processed).unwrap();
        assert_eq!(restored.to_cosigned().unwrap().cosigner_threshold(), 2);
        assert_eq!(restored.to_proto(), reference);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn generated_legacy_orders_preserve_every_wire_member(
        ranks in proptest::collection::vec(any::<u64>(), 1..33),
    ) {
        let mut order: Vec<_> = (0..ranks.len()).collect();
        order.sort_by_key(|index| (ranks[*index], *index));
        let proto = legacy_proto_for_order(&order);
        let envelope = DeployEnvelope::from_proto(proto.clone(), envelope_limits()).unwrap();
        prop_assert_eq!(envelope.to_proto().unwrap().encode_to_vec(), proto.encode_to_vec());
        prop_assert_eq!(envelope.identity().as_bytes(), proto.sig.as_ref());
        prop_assert_eq!(envelope.signers().len(), ranks.len());
        prop_assert_eq!(envelope.body_envelope().is_ok(), order.iter().copied().eq(0..ranks.len()));
        let stored = StoredDeployEnvelope::new(&envelope).unwrap();
        prop_assert_eq!(stored.decode(envelope.identity(), envelope_limits()).unwrap(), envelope);
        if !proto.cosigners.is_empty() {
            let mut duplicate = proto;
            duplicate.cosigners[0].pk = duplicate.deployer.clone();
            duplicate.cosigners[0].sig = duplicate.sig.clone();
            duplicate.cosigners[0].sig_algorithm = duplicate.sig_algorithm.clone();
            prop_assert!(DeployEnvelope::from_proto(duplicate, envelope_limits()).is_err());
        }
    }
}

fn absent_first<A: std::fmt::Debug + serde::Serialize + ToMessage>(data: A) -> Cosigned<A> {
    let mut members: Vec<_> = (1..=3)
        .map(|i| {
            let key = PrivateKey::from_bytes(&[i; 32]);
            (
                Cosigner {
                    pk: Secp256k1.to_public(&key),
                    sig: Default::default(),
                    sig_algorithm: Box::new(Secp256k1) as Box<dyn SignaturesAlg>,
                },
                key,
            )
        })
        .collect();
    members.sort_by(|a, b| a.0.pk.bytes.cmp(&b.0.pk.bytes));
    let mut signers: Vec<_> = members.iter().map(|(signer, _)| signer.clone()).collect();
    let hash = Cosigned::envelope_signing_hash_for_presence(&data, &signers, 2, &[6], "secp256k1")
        .unwrap();
    for i in 1..3 {
        signers[i].sig = Secp256k1.sign(&hash, &members[i].1.bytes).into();
    }
    Cosigned::from_envelope_signed_data_threshold(data, signers, 2).unwrap()
}

#[test]
fn threshold_records_keep_absent_first_member_and_original_signing_payload() {
    let funded = absent_first(funded());
    let offered = absent_first(
        OfferedFundedDeploy::new(body(), funding(10, 3, 30), 10, 1, limits()).unwrap(),
    );
    for proto in [
        DeployData::to_proto_cosigned(&absent_first(body())),
        FundedDeploy::to_proto(&funded).unwrap(),
        OfferedFundedDeploy::to_proto(&offered).unwrap(),
    ] {
        let checked = DeployEnvelope::from_proto(proto.clone(), envelope_limits()).unwrap();
        assert_eq!(checked.threshold(), 2);
        assert_eq!(checked.signers().len(), 3);
        assert!(checked.signers()[0].sig.is_empty());
        assert!(!checked.primary().sig.is_empty());
        assert_eq!(checked.primary(), &checked.signers()[1]);
        let record = StoredDeployEnvelope::new(&checked).unwrap();
        let recovered = record
            .decode(checked.identity(), envelope_limits())
            .unwrap();
        assert_eq!(recovered.to_proto().unwrap(), proto);
        match recovered.view() {
            DeployEnvelopeRef::Funded(actual) => assert_eq!(actual, &funded),
            DeployEnvelopeRef::OfferedFunded(actual) => assert_eq!(actual, &offered),
            DeployEnvelopeRef::BodyV61(actual) => actual.validate_envelope().unwrap(),
            DeployEnvelopeRef::Legacy(_) => panic!("envelope changed to legacy authorization"),
        }
        let mut small = envelope_limits();
        small.members = NonZeroUsize::new(2).unwrap();
        assert!(record.decode(checked.identity(), small).is_err());
    }
}

fn stored(schema: u32, format: Option<u32>, wire: Vec<u8>) -> StoredDeployEnvelope {
    bincode::deserialize(&bincode::serialize(&(schema, format, wire)).unwrap()).unwrap()
}

#[test]
fn records_reject_schema_format_identity_and_canonical_wire_substitution() {
    for (format, proto) in variants() {
        let wire = proto.encode_to_vec();
        let checked = DeployEnvelope::from_proto(proto.clone(), envelope_limits()).unwrap();
        for schema in [0, 2, u32::MAX] {
            assert!(stored(schema, format.authorization_version(), wire.clone())
                .decode(checked.identity(), envelope_limits())
                .is_err());
        }
        for version in [
            None,
            Some(0),
            Some(0x60001),
            Some(0x60002),
            Some(0x60003),
            Some(u32::MAX),
        ] {
            if version != format.authorization_version() {
                assert!(stored(1, version, wire.clone())
                    .decode(checked.identity(), envelope_limits())
                    .is_err());
            }
        }
        let mut unknown_field = wire.clone();
        unknown_field.extend_from_slice(&[0xa0, 0x06, 0x01]);
        assert!(DeployEnvelope::decode(&unknown_field, envelope_limits()).is_ok());
        assert!(stored(1, format.authorization_version(), unknown_field)
            .decode(checked.identity(), envelope_limits())
            .is_err());
        let mut changed = proto;
        changed.term.push(' ');
        assert!(
            stored(1, format.authorization_version(), changed.encode_to_vec())
                .decode(checked.identity(), envelope_limits())
                .is_err()
        );
        assert!(stored(1, format.authorization_version(), vec![255])
            .decode(checked.identity(), envelope_limits())
            .is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn exact_format_dispatch_has_no_unknown_version_fallback(version in any::<u32>()) {
        let mut proto = OfferedFundedDeploy::to_proto(&absent_first(
            OfferedFundedDeploy::new(body(),funding(10,3,30),10,1,limits()).unwrap())).unwrap();
        proto.authorization_v61.as_mut().unwrap().format_version = version;
        prop_assert_eq!(DeployEnvelope::from_proto(proto,envelope_limits()).is_ok(), version == 0x60003);
        prop_assert_eq!(DeployEnvelopeFormat::from_authorization_version(Some(version)).is_ok(), matches!(version,0x60001..=0x60003));
    }
}
