use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::secp256k1_eth::Secp256k1Eth;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::{Cosigner, Signed};
use proptest::prelude::*;

use super::*;
use crate::rust::phlo_controls::{PhloControlsLimits, PhloControlsV1};
use crate::rust::phlo_schedule::{PhloResourceClassV1, PhloScheduleV1};
use crate::rust::phlo_source::{PhloSourceLimits, PhloSourcePolicyV1};

#[path = "offered_tests.rs"]
mod offered_tests;

#[path = "envelope_tests.rs"]
mod envelope_tests;

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
                schedules: 4,
                total_classes: 16,
            },
            sources: 256,
            resource_permissions: 1024,
            authority_nodes: 4096,
        },
    }
}

fn body() -> DeployData {
    DeployData {
        term: "Nil".to_string(),
        language: "rholang".to_string(),
        time_stamp: 1,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    }
}

fn funding(limit: u64, ceiling: u64, exposure: u128) -> Vec<u8> {
    let controls = PhloControlsV1 {
        limit,
        price_ceiling: ceiling,
        required_owner_ceilings: vec![ceiling],
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
    let schedule_commitment = controls.permitted_schedules[0]
        .digest(limits().funding.controls().schedule(1))
        .unwrap();
    PhloFundingIntentV1 {
        controls,
        schedule_commitment,
        total_exposure: exposure,
        sources: vec![PhloSourcePolicyV1::new(
            b"custody",
            100,
            100,
            true,
            vec![],
            PhloSourceLimits {
                wire: limits().funding.wire,
                resource_permissions: 0,
                authority_nodes: 0,
            },
        )
        .unwrap()],
    }
    .encode(limits().funding)
    .unwrap()
}

fn funded() -> FundedDeploy { FundedDeploy::new(body(), funding(10, 3, 30), limits()).unwrap() }

fn signed() -> Cosigned<FundedDeploy> {
    Cosigned::create_single_envelope(
        funded(),
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[1; 32]),
    )
    .unwrap()
}

#[test]
fn funded_payload_matches_independent_framing_and_is_disjoint_from_v61() {
    let data = funded();
    let mut oracle = vec![0, 2];
    for field in [
        b"f1r3node:funded-deploy-intent:v1".as_slice(),
        &[0, 6, 0, 2],
        body().envelope_intent_v61().unwrap().as_slice(),
        data.funding_intent(),
    ] {
        oracle.extend_from_slice(&(field.len() as u64).to_be_bytes());
        oracle.extend_from_slice(field);
    }
    assert_eq!(data.envelope_intent_v61().unwrap(), oracle);
    assert_eq!(&body().envelope_intent_v61().unwrap()[..2], &[0, 1]);
    assert_ne!(
        data.envelope_intent_v61().unwrap(),
        body().envelope_intent_v61().unwrap()
    );
}

#[test]
fn funded_wire_roundtrip_keeps_every_signed_byte() {
    let envelope = signed();
    let proto = FundedDeploy::to_proto(&envelope).unwrap();
    assert_eq!(
        hex::encode(&proto.deploy_id),
        "4bf7027b5d901be26d6ee27c452731c97edc1817817d54042f380c7bfa99103d"
    );
    assert_eq!(
        proto.authorization_v61.as_ref().unwrap().format_version,
        0x0006_0002
    );
    assert_eq!(
        proto.funding_intent.as_deref(),
        Some(envelope.data.funding_intent())
    );
    let bytes = proto.encode_to_vec();
    assert_eq!(
        Blake2b256::hash(bytes.clone()),
        hex::decode("8e5fe94cf45bd1556d96fbe29553a9986472f6f3f37cfb3a91fbc5fc7e99e179").unwrap()
    );
    assert_eq!(
        Blake2b256::hash(envelope.data.funding_intent().to_vec()),
        hex::decode("25753d4b0dfbecbc129ea1926fa8980a484a8bb9403cbf717ddfa366d3151ea8").unwrap()
    );
    let recovered =
        FundedDeploy::from_proto(DeployDataProto::decode(bytes.as_slice()).unwrap(), limits())
            .unwrap();
    assert_eq!(recovered, envelope);
    assert_eq!(
        FundedDeploy::to_proto(&recovered).unwrap().encode_to_vec(),
        bytes
    );
}

#[test]
fn funded_ethereum_scheme_roundtrip_uses_the_same_bound_intent() {
    let envelope = Cosigned::create_single_envelope(
        funded(),
        Box::new(Secp256k1Eth),
        PrivateKey::from_bytes(&[1; 32]),
    )
    .unwrap();
    let proto = FundedDeploy::to_proto(&envelope).unwrap();
    assert_eq!(FundedDeploy::from_proto(proto, limits()).unwrap(), envelope);
}

#[test]
fn signed_funding_intents_preserve_arbitrary_source_cohorts() {
    for count in [1usize, 2, 3, 17, 65, 129] {
        let base = funding(10, 3, 30);
        let mut intent = PhloFundingIntentV1::decode(&base, limits().funding).unwrap();
        let custody = (0..count)
            .map(|index| index.to_be_bytes())
            .collect::<Vec<_>>();
        intent.sources = custody
            .iter()
            .map(|identity| {
                PhloSourcePolicyV1::new(identity, 100, 100, true, vec![], PhloSourceLimits {
                    wire: limits().funding.wire,
                    resource_permissions: 0,
                    authority_nodes: 0,
                })
                .unwrap()
            })
            .collect();
        let encoded = intent.encode(limits().funding).unwrap();
        let data = FundedDeploy::new(body(), encoded.clone(), limits()).unwrap();
        let envelope = Cosigned::create_single_envelope(
            data,
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap();
        let recovered =
            FundedDeploy::from_proto(FundedDeploy::to_proto(&envelope).unwrap(), limits()).unwrap();
        assert_eq!(recovered.data.funding_intent(), encoded);
        let recovered =
            PhloFundingIntentV1::decode(recovered.data.funding_intent(), limits().funding).unwrap();
        assert_eq!(recovered.sources, intent.sources);
    }
}

#[test]
fn old_payload_signatures_cannot_authorize_an_added_funding_field() {
    let legacy = Signed::create(
        body(),
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[1; 32]),
    )
    .unwrap();
    let proto = DeployData::to_proto(legacy);
    assert!(DeployData::from_proto(proto.clone()).is_ok());
    for bytes in [Vec::new(), funding(10, 3, 30)] {
        let mut candidate = proto.clone();
        candidate.funding_intent = Some(bytes.into());
        assert!(DeployData::from_proto(candidate.clone()).is_err());
        assert!(DeployData::from_proto_cosigned_legacy(candidate.clone()).is_err());
        assert!(FundedDeploy::from_proto(candidate, limits()).is_err());
    }
}

#[test]
fn funded_fields_cannot_be_silently_dropped_by_any_existing_decoder() {
    let proto = FundedDeploy::to_proto(&signed()).unwrap();
    for funding in [proto.funding_intent.clone(), Some(Vec::new().into())] {
        let mut candidate = proto.clone();
        candidate.funding_intent = funding;
        assert!(DeployData::decode(candidate.encode_to_vec()).is_err());
        assert!(DeployData::from_proto(candidate.clone()).is_err());
        assert!(DeployData::from_proto_cosigned_legacy(candidate.clone()).is_err());
        assert!(DeployData::from_proto_cosigned(candidate).is_err());
    }
    let mut missing = proto.clone();
    missing.funding_intent = None;
    assert!(FundedDeploy::from_proto(missing.clone(), limits()).is_err());
    assert!(DeployData::decode(missing.encode_to_vec()).is_err());
    missing.authorization_v61.as_mut().unwrap().format_version = 0x0006_0001;
    assert!(DeployData::from_proto_cosigned(missing).is_err());
    let legacy = Cosigned::create_single_envelope(
        body(),
        Box::new(Secp256k1),
        PrivateKey::from_bytes(&[1; 32]),
    )
    .unwrap();
    let mut transplanted = DeployData::to_proto_cosigned(&legacy);
    transplanted.funding_intent = proto.funding_intent;
    transplanted
        .authorization_v61
        .as_mut()
        .unwrap()
        .format_version = 0x0006_0002;
    assert!(FundedDeploy::from_proto(transplanted, limits()).is_err());
}

#[test]
fn every_single_byte_funding_mutation_invalidates_authorization() {
    let proto = FundedDeploy::to_proto(&signed()).unwrap();
    let original = proto.funding_intent.as_ref().unwrap();
    for index in 0..original.len() {
        for bit in 0..8 {
            let mut bytes = original.to_vec();
            bytes[index] ^= 1 << bit;
            let mut candidate = proto.clone();
            candidate.funding_intent = Some(bytes.into());
            assert!(
                FundedDeploy::from_proto(candidate, limits()).is_err(),
                "byte {index}, bit {bit}"
            );
        }
    }
}

#[test]
fn funded_decoding_rejects_mixed_authorization_and_wrong_versions() {
    let proto = FundedDeploy::to_proto(&signed()).unwrap();
    for version in [0, 1, 0x0006_0001, 0x0006_0003, u32::MAX] {
        let mut candidate = proto.clone();
        candidate.authorization_v61.as_mut().unwrap().format_version = version;
        assert!(FundedDeploy::from_proto(candidate, limits()).is_err());
    }
    let mut candidate = proto.clone();
    candidate.sig = vec![1].into();
    assert!(FundedDeploy::from_proto(candidate, limits()).is_err());
    let mut candidate = proto.clone();
    candidate.term.push(' ');
    assert!(FundedDeploy::from_proto(candidate, limits()).is_err());
    let mut candidate = proto;
    candidate.deploy_id = vec![0; 32].into();
    assert!(FundedDeploy::from_proto(candidate, limits()).is_err());
    let legacy = Cosigned::from_single_signer(
        Signed::create(
            funded(),
            Box::new(Secp256k1),
            PrivateKey::from_bytes(&[1; 32]),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(FundedDeploy::to_proto(&legacy).is_err());
}

#[test]
fn wire_and_signing_bounds_reject_without_partial_results() {
    let envelope = signed();
    let proto = FundedDeploy::to_proto(&envelope).unwrap();
    let mut bounded = limits();
    bounded.deploy_bytes = proto.encoded_len();
    assert!(FundedDeploy::from_proto(proto.clone(), bounded).is_ok());
    bounded.deploy_bytes -= 1;
    assert!(FundedDeploy::from_proto(proto, bounded).is_err());
    let payload_len = envelope.data.envelope_intent_v61().unwrap().len();
    bounded = limits();
    bounded.signing.total_bytes = payload_len;
    assert!(FundedDeploy::new(body(), funding(10, 3, 30), bounded).is_ok());
    bounded.signing.total_bytes -= 1;
    assert!(FundedDeploy::new(body(), funding(10, 3, 30), bounded).is_err());
    let bytes = funding(10, 3, 30);
    for end in 0..bytes.len() {
        assert!(FundedDeploy::new(body(), bytes[..end].to_vec(), limits()).is_err());
    }
}

fn threshold_envelope<A: std::fmt::Debug + serde::Serialize + ToMessage>(
    data: A,
    count: usize,
) -> Cosigned<A> {
    let mut members = (1..=count)
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
        .collect::<Vec<_>>();
    members.sort_by(|a, b| a.0.pk.bytes.cmp(&b.0.pk.bytes));
    let mut signers = members
        .iter()
        .map(|(signer, _)| signer.clone())
        .collect::<Vec<_>>();
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
fn funded_threshold_supports_many_signers_with_exact_selected_witnesses() {
    for count in [1usize, 3, 17, 65] {
        let envelope = threshold_envelope(funded(), count);
        let selected = count.div_ceil(2);
        let proto = FundedDeploy::to_proto(&envelope).unwrap();
        assert_eq!(
            proto.authorization_v61.as_ref().unwrap().witnesses.len(),
            selected
        );
        assert_eq!(FundedDeploy::from_proto(proto, limits()).unwrap(), envelope);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn changed_controls_change_signed_payload_and_deploy_identity(
        limit in any::<u64>(), ceiling in any::<u64>(), exposure in any::<u128>(),
    ) {
        let first = FundedDeploy::new(body(), funding(limit, ceiling, exposure), limits()).unwrap();
        let second = FundedDeploy::new(body(), funding(limit, ceiling, exposure ^ 1), limits()).unwrap();
        prop_assert_ne!(first.envelope_intent_v61().unwrap(), second.envelope_intent_v61().unwrap());
        let first = Cosigned::create_single_envelope(first, Box::new(Secp256k1), PrivateKey::from_bytes(&[1; 32])).unwrap();
        let second = Cosigned::create_single_envelope(second, Box::new(Secp256k1), PrivateKey::from_bytes(&[1; 32])).unwrap();
        prop_assert_ne!(first.envelope_commitment().unwrap(), second.envelope_commitment().unwrap());
        let proto = FundedDeploy::to_proto(&first).unwrap();
        prop_assert_eq!(FundedDeploy::from_proto(proto, limits()).unwrap(), first);
    }
}
