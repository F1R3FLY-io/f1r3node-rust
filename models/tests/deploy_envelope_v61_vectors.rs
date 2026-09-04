use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::{Cosigned, Cosigner, ToMessage};
use models::casper::DeployDataProto;
use models::rust::casper::protocol::casper_message::DeployData;
use prost::Message;

fn field<'a>(value: &'a serde_json::Value, name: &str) -> &'a serde_json::Value {
    value.get(name).unwrap_or_else(|| panic!("missing {name}"))
}

fn string(value: &serde_json::Value, name: &str) -> String {
    field(value, name)
        .as_str()
        .unwrap_or_else(|| panic!("{name} is not a string"))
        .to_string()
}

fn integer(value: &serde_json::Value, name: &str) -> i64 {
    field(value, name)
        .as_i64()
        .unwrap_or_else(|| panic!("{name} is not an integer"))
}

#[test]
fn rust_consumes_the_canonical_v61_threshold_vector() {
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("../../test-vectors/deploy-envelope-v6.1.json"))
            .expect("v6.1 vectors");
    assert_eq!(integer(&vectors, "protocolVersion"), 6);
    assert_eq!(integer(&vectors, "authorizationFormatVersion"), 0x0006_0001);
    let vector = &field(&vectors, "positive")["threshold2Of3Selected0And2"];
    let data = DeployData {
        term: string(vector, "term"),
        language: string(vector, "language"),
        time_stamp: integer(vector, "timestamp"),
        valid_after_block_number: integer(vector, "validAfterBlockNumber"),
        shard_id: string(vector, "shardId"),
        expiration_timestamp: Some(integer(vector, "expirationTimestamp")),
        authority_presentations: Vec::new(),
    };
    assert_eq!(
        hex::encode(data.envelope_intent_v61().expect("canonical intent")),
        string(vector, "intentHex")
    );

    let algorithm = Secp256k1;
    let members = field(vector, "members").as_array().expect("members array");
    let signers = members
        .iter()
        .map(|member| {
            assert_eq!(integer(member, "schemeId"), 1);
            let private_key = PrivateKey::from_bytes(
                &hex::decode(string(member, "privateKeyHex")).expect("private key"),
            );
            let public_key = hex::decode(string(member, "publicKeyHex")).expect("public key");
            assert_eq!(algorithm.to_public(&private_key).bytes.as_ref(), public_key);
            let signer = Cosigner {
                pk: PublicKey::from_bytes(&public_key),
                sig: hex::decode(string(member, "signatureHex"))
                    .expect("signature")
                    .into(),
                sig_algorithm: Box::new(algorithm.clone()),
            };
            assert_eq!(
                hex::encode(signer.principal_bytes_v61().expect("principal")),
                string(member, "principalHex")
            );
            signer
        })
        .collect::<Vec<_>>();
    let threshold = integer(vector, "threshold") as u32;
    let bitmap = hex::decode(string(vector, "presenceBitmapHex")).expect("presence bitmap");
    assert_eq!(
        hex::encode(
            Cosigned::envelope_signing_hash_for_presence(
                &data,
                &signers,
                threshold,
                &bitmap,
                "secp256k1",
            )
            .expect("signing hash")
        ),
        string(vector, "signingHashHex")
    );

    let envelope = Cosigned::from_envelope_signed_data_threshold(data, signers, threshold)
        .expect("canonical envelope");
    assert_eq!(
        hex::encode(envelope.envelope_commitment().expect("deploy id")),
        string(vector, "deployIdHex")
    );
    let encoded = DeployData::to_proto_cosigned(&envelope).encode_to_vec();
    assert_eq!(hex::encode(&encoded), string(vector, "protobufHex"));
    let decoded = DeployDataProto::decode(encoded.as_slice()).expect("protobuf");
    let decoded = DeployData::from_proto_cosigned(decoded).expect("v6.1 wire envelope");
    assert_eq!(
        decoded.envelope_commitment().expect("decoded deploy id"),
        envelope.envelope_commitment().expect("source deploy id")
    );
}

#[test]
fn canonical_vector_catalog_covers_every_normative_rejection_class() {
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("../../test-vectors/deploy-envelope-v6.1.json"))
            .expect("v6.1 vectors");
    let actual = field(&vectors, "negative")
        .as_array()
        .expect("negative vectors")
        .iter()
        .map(|case| string(case, "id"))
        .collect::<std::collections::BTreeSet<_>>();
    let expected = [
        "bitmap-selects-too-few",
        "bitmap-unused-high-bit",
        "deploy-id-bit-flip",
        "duplicate-ground-across-schemes",
        "inactive-signature-scheme",
        "legacy-field-mixed-with-v6",
        "noncanonical-high-s-signature",
        "noncanonical-policy-order",
        "witness-index-disagrees-with-bitmap",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(actual, expected);
}

#[test]
fn rust_rejects_every_canonical_v61_negative_vector() {
    use models::casper::authorization_policy_v61::Policy;

    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("../../test-vectors/deploy-envelope-v6.1.json"))
            .expect("v6.1 vectors");
    let positive = &field(&vectors, "positive")["threshold2Of3Selected0And2"];
    let encoded = hex::decode(string(positive, "protobufHex")).expect("protobuf hex");
    let baseline = DeployDataProto::decode(encoded.as_slice()).expect("protobuf");
    for case in field(&vectors, "negative")
        .as_array()
        .expect("negative vectors")
    {
        let id = string(case, "id");
        let expected_error = string(case, "expectedError");
        let mut proto = baseline.clone();
        let authorization = proto
            .authorization_v61
            .as_mut()
            .expect("v6.1 authorization");
        let policy = authorization
            .policy
            .as_mut()
            .and_then(|policy| policy.policy.as_mut())
            .expect("v6.1 policy");
        let members = match policy {
            Policy::Threshold(policy) => &mut policy.members,
            Policy::AllOf(_) => panic!("threshold vector policy"),
        };
        match id.as_str() {
            "deploy-id-bit-flip" => {
                let mut deploy_id = proto.deploy_id.to_vec();
                deploy_id[0] ^= 1;
                proto.deploy_id = deploy_id.into();
            }
            "bitmap-selects-too-few" => {
                authorization.presence_bitmap = vec![1].into();
                authorization.witnesses.truncate(1);
            }
            "bitmap-unused-high-bit" => authorization.presence_bitmap = vec![0x85].into(),
            "witness-index-disagrees-with-bitmap" => {
                authorization.witnesses[1].member_index = 1;
            }
            "noncanonical-policy-order" => members.swap(0, 1),
            "duplicate-ground-across-schemes" => {
                let first = members[0].clone();
                let third = members[2].clone();
                let first_signature = authorization.witnesses[0].signature.clone();
                let third_signature = authorization.witnesses[1].signature.clone();
                members.clear();
                members.push(first.clone());
                members.push(third);
                members.push(models::casper::PrincipalV61 {
                    scheme: 2,
                    public_key: first.public_key,
                });
                authorization.presence_bitmap = vec![3].into();
                authorization.witnesses = vec![
                    models::casper::SignatureWitnessV61 {
                        member_index: 0,
                        signature: first_signature,
                    },
                    models::casper::SignatureWitnessV61 {
                        member_index: 1,
                        signature: third_signature,
                    },
                ];
            }
            "inactive-signature-scheme" => members[1].scheme = 5,
            "legacy-field-mixed-with-v6" => proto.sig_algorithm = "secp256k1".to_string(),
            "noncanonical-high-s-signature" => {
                authorization.witnesses[0].signature = hex::decode(
                    "30450220486fda74ae514bfe06034dee498fc34f700980826ce2c62b0cde382107021fde022100b3457ddf03b023ebcf95a7fc8d1d46d20e026d0b6a5d0c2249c3592d52c8fc36",
                )
                .expect("high-S signature")
                .into();
            }
            other => panic!("unhandled negative vector {other}"),
        }
        let error = DeployData::from_proto_cosigned(proto).expect_err("negative vector");
        assert!(
            error.contains(&expected_error),
            "{id}: expected {expected_error:?}, got {error:?}"
        );
    }
}
