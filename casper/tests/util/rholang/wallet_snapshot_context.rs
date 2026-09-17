use casper::rust::util::rholang::tools::Tools;

fn shard() -> casper::rust::casper::CasperShardConf {
    casper::rust::casper::CasperShardConf {
        shard_name: "root".to_string(),
        ..casper::rust::casper::CasperShardConf::new()
    }
}
use crypto::rust::signatures::signed::{Cosigner, ToMessage};
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rust::deploy_envelope::{DeployEnvelope, DeployEnvelopeFormat, DeployEnvelopeLimits};
use models::rust::normalizer_env::normalizer_env_from_envelope;
use proptest::prelude::*;
use rholang::rust::interpreter::system_processes::{
    DeployAuthority, DeployData as SystemDeployData,
};

use super::*;

fn payload_limits(limits: DirectWalletFundingLimits) -> FundedDeployLimits {
    FundedDeployLimits {
        deploy_bytes: 2_097_152,
        signing: limits.funding.wire,
        funding: limits.funding,
    }
}

fn decode(
    proto: models::casper::DeployDataProto,
    limits: DirectWalletFundingLimits,
    members: usize,
) -> DeployEnvelope {
    DeployEnvelope::from_proto(proto, DeployEnvelopeLimits {
        payload: payload_limits(limits),
        members: NonZeroUsize::new(members).unwrap(),
    })
    .unwrap()
}

fn assert_context(deploy: &DeployEnvelope) {
    let original = deploy.to_proto().unwrap();
    let data = SystemDeployData::from_envelope(deploy);
    let env = normalizer_env_from_envelope(deploy);
    assert_eq!(data.timestamp, deploy.body().time_stamp);
    assert_eq!(data.deploy_id, deploy.identity().as_bytes());
    assert!(matches!(
        &env["rho:system:deployId"].unforgeables[0].unf_instance,
        Some(UnfInstance::GDeployIdBody(id)) if id.sig == data.deploy_id
    ));
    match data.authority {
        DeployAuthority::Legacy(public_key) => assert!(matches!(
            &env["rho:system:deployerId"].unforgeables[0].unf_instance,
            Some(UnfInstance::GDeployerIdBody(id)) if id.public_key == public_key.bytes
        )),
        DeployAuthority::Principal(principal) => assert!(matches!(
            &env["rho:system:deployerId"].unforgeables[0].unf_instance,
            Some(UnfInstance::GPrincipalIdBody(id)) if *id == principal
        )),
        DeployAuthority::Compound(authority) => {
            assert!(!env.contains_key("rho:system:deployerId"));
            assert!(matches!(
                &env["rho:system:authorityId"].unforgeables[0].unf_instance,
                Some(UnfInstance::GAuthorityIdBody(id)) if *id == authority
            ));
        }
    }
    let expected = if deploy.format() == DeployEnvelopeFormat::Legacy {
        Tools::unforgeable_name_rng(&deploy.primary().pk, deploy.body().time_stamp)
    } else {
        let mut seed = b"f1r3node:user-deploy-unforgeable:v6".to_vec();
        seed.extend_from_slice(deploy.identity().as_bytes());
        Blake2b512Random::create_from_bytes(&seed)
    };
    assert_eq!(Tools::user_envelope_rng(deploy), expected);
    assert_eq!(deploy.to_proto().unwrap(), original);
}

fn threshold<A: std::fmt::Debug + serde::Serialize + ToMessage>(
    data: A,
    count: usize,
    selected: usize,
) -> Cosigned<A> {
    let mut members: Vec<_> = (1..=count)
        .map(|index| {
            let key = PrivateKey::from_bytes(&[u8::try_from(index).unwrap(); 32]);
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
    members.sort_by(|a, b| a.0.pk.bytes.cmp(&b.0.pk.bytes));
    let mut signers: Vec<_> = members.iter().map(|(signer, _)| signer.clone()).collect();
    let mut bitmap = vec![0_u8; count.div_ceil(8)];
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
fn envelope_runtime_context_preserves_all_formats_and_selected_authority() {
    let key = PrivateKey::from_bytes(&[1; 32]);
    let (funded, limits) = funded_snapshot_envelope(key.clone(), &shard());
    let body = funded.data.body().clone();
    let legacy = Signed::create(body.clone(), Box::new(Secp256k1), key.clone()).unwrap();
    let plain = Cosigned::create_single_envelope(body.clone(), Box::new(Secp256k1), key).unwrap();
    for proto in [
        DeployData::to_proto(legacy),
        DeployData::to_proto_cosigned(&plain),
    ] {
        let envelope = decode(proto, limits, 1);
        assert_context(&envelope);
        assert_eq!(
            Tools::user_envelope_rng(&envelope),
            Tools::user_deploy_rng(envelope.body_envelope().unwrap())
        );
        let prior = SystemDeployData::from_cosigned(envelope.body_envelope().unwrap());
        let current = SystemDeployData::from_envelope(&envelope);
        assert_eq!(prior.timestamp, current.timestamp);
        assert_eq!(prior.deploy_id, current.deploy_id);
    }
    for count in [1_usize, 2, 3, 9, 65] {
        for selected in [1, count.div_ceil(2), count] {
            let funded = threshold(funded.data.clone(), count, selected);
            let offered = threshold(
                OfferedFundedDeploy::new(
                    body.clone(),
                    funded.data.funding_intent().to_vec(),
                    8,
                    1,
                    payload_limits(limits),
                )
                .unwrap(),
                count,
                selected,
            );
            for proto in [
                FundedDeploy::to_proto(&funded).unwrap(),
                OfferedFundedDeploy::to_proto(&offered).unwrap(),
            ] {
                let envelope = decode(proto, limits, count);
                assert_context(&envelope);
                assert_eq!(
                    matches!(
                        SystemDeployData::from_envelope(&envelope).authority,
                        DeployAuthority::Principal(_)
                    ),
                    selected == 1
                );
                let restored = decode(envelope.to_proto().unwrap(), limits, count);
                assert_context(&restored);
                assert_eq!(
                    Tools::user_envelope_rng(&envelope),
                    Tools::user_envelope_rng(&restored)
                );
            }
        }
    }
}

#[test]
fn envelope_runtime_context_retains_noncanonical_legacy_primary() {
    let key = PrivateKey::from_bytes(&[1; 32]);
    let (funded, limits) = funded_snapshot_envelope(key, &shard());
    let body = funded.data.body().clone();
    let signers = (1..=3)
        .map(|index| {
            let signed = Signed::create(
                body.clone(),
                Box::new(Secp256k1),
                PrivateKey::from_bytes(&[index; 32]),
            )
            .unwrap();
            Cosigner {
                pk: signed.pk,
                sig: signed.sig,
                sig_algorithm: signed.sig_algorithm,
            }
        })
        .collect();
    let legacy = Cosigned::from_signed_data(body, signers).unwrap();
    let mut proto = DeployData::to_proto_cosigned(&legacy);
    let mut other = proto.cosigners.remove(1);
    std::mem::swap(&mut proto.deployer, &mut other.pk);
    std::mem::swap(&mut proto.sig, &mut other.sig);
    std::mem::swap(&mut proto.sig_algorithm, &mut other.sig_algorithm);
    proto.cosigners.push(other);
    let envelope = decode(proto.clone(), limits, 3);
    assert_ne!(envelope.primary().pk, envelope.signers()[0].pk);
    assert_context(&envelope);
    let context = SystemDeployData::from_envelope(&envelope);
    assert!(
        matches!(context.authority, DeployAuthority::Legacy(public_key) if public_key.bytes == proto.deployer)
    );
    assert_eq!(context.deploy_id, proto.sig);
    assert_ne!(
        Tools::user_envelope_rng(&envelope),
        Tools::unforgeable_name_rng(&envelope.signers()[0].pk, envelope.body().time_stamp)
    );
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::Direct(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/util/rholang/wallet_snapshot_context.proptest-regressions")
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn envelope_runtime_context_binds_offer_and_survives_wire_roundtrip(
        limit in 0_i64..=i64::MAX,
        price in 0_i64..=i64::MAX,
        timestamp in any::<i64>(),
    ) {
        let key = PrivateKey::from_bytes(&[1; 32]);
        let (funded, limits) = funded_snapshot_envelope(key.clone(), &shard());
        let mut body = funded.data.body().clone();
        body.time_stamp = timestamp;
        if timestamp < 0 {
            prop_assert_eq!(
                OfferedFundedDeploy::new(body, funded.data.funding_intent().to_vec(), limit, price, payload_limits(limits)),
                Err("protocol-v6 deploy timestamp must be nonnegative".to_string())
            );
            return Ok(());
        }
        let make = |price| {
            let offered = Cosigned::create_single_envelope(OfferedFundedDeploy::new(body.clone(), funded.data.funding_intent().to_vec(), limit, price, payload_limits(limits)).unwrap(), Box::new(Secp256k1), key.clone()).unwrap();
            decode(OfferedFundedDeploy::to_proto(&offered).unwrap(), limits, 1)
        };
        let first = make(price);
        let changed = make(price ^ 1);
        assert_context(&first);
        assert_context(&changed);
        let restored = decode(first.to_proto().unwrap(), limits, 1);
        prop_assert_eq!(SystemDeployData::from_envelope(&first).deploy_id, SystemDeployData::from_envelope(&restored).deploy_id);
        prop_assert_eq!(Tools::user_envelope_rng(&first), Tools::user_envelope_rng(&restored));
        prop_assert_ne!(Tools::user_envelope_rng(&first), Tools::user_envelope_rng(&changed));
    }
}
