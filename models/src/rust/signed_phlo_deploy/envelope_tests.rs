use std::num::NonZeroUsize;

use super::*;
use crate::rust::deploy_envelope::{
    DeployEnvelope, DeployEnvelopeFormat, DeployEnvelopeLimits, StoredDeployEnvelope,
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
