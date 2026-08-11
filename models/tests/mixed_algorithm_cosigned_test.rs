//! Phase 4.13 — mixed-algorithm cosigner integration tests.
//!
//! Verifies the `Cosigned<A>` envelope correctly handles a mix of
//! signature algorithms across cosigners (secp256k1, ed25519,
//! secp256k1_eth, and feature-gated Schnorr/FROST). Each
//! `Cosigner` carries its own `Box<dyn SignaturesAlg>` so the
//! envelope must dispatch verification per-signer.

use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::ed25519::Ed25519;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::secp256k1_eth::Secp256k1Eth;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::{Cosigned, CosignedError, Cosigner, Signed, ToMessage};
use models::rust::casper::protocol::casper_message::DeployData;
use prost::bytes::Bytes;
use prost::Message;

fn payload(time_stamp: i64) -> DeployData {
    DeployData {
        term: "Nil".to_string(),
        time_stamp,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
    }
}

fn sign_with(data: &DeployData, alg: &dyn SignaturesAlg, sk: &PrivateKey) -> Bytes {
    let serialized = data.to_message().encode_to_vec();
    let hash = Signed::<DeployData>::signature_hash(&alg.name(), serialized);
    Bytes::from(alg.sign(&hash, &sk.bytes))
}

fn mk_cosigner(data: &DeployData, alg: Box<dyn SignaturesAlg>) -> (Cosigner, PublicKey) {
    let (sk, pk) = alg.new_key_pair();
    let sig = sign_with(data, alg.as_ref(), &sk);
    let cosigner = Cosigner {
        pk: pk.clone(),
        sig,
        sig_algorithm: alg,
    };
    (cosigner, pk)
}

#[test]
fn cosigned_mixed_secp256k1_and_ed25519() {
    let data = payload(1700000000000);
    let (s_secp, _) = mk_cosigner(&data, Box::new(Secp256k1));
    let (s_ed, _) = mk_cosigner(&data, Box::new(Ed25519));
    let cosigned = Cosigned::from_signed_data(data, vec![s_secp, s_ed])
        .expect("mixed secp256k1 + ed25519 envelope must verify");
    assert_eq!(cosigned.signers().len(), 2);
    // Both algorithms preserved on the canonical signer list.
    let alg_names: Vec<_> = cosigned
        .signers()
        .iter()
        .map(|s| s.sig_algorithm.name())
        .collect();
    assert!(alg_names.contains(&Secp256k1::name()));
    assert!(alg_names.contains(&Ed25519.name()));
}

#[test]
fn cosigned_mixed_three_algorithms_secp_ed25519_secp_eth() {
    let data = payload(1700000000001);
    let (s1, _) = mk_cosigner(&data, Box::new(Secp256k1));
    let (s2, _) = mk_cosigner(&data, Box::new(Ed25519));
    let (s3, _) = mk_cosigner(&data, Box::new(Secp256k1Eth));
    let cosigned = Cosigned::from_signed_data(data, vec![s1, s2, s3])
        .expect("3-way mixed-algorithm envelope must verify");
    assert_eq!(cosigned.signers().len(), 3);
    let alg_names: std::collections::HashSet<_> = cosigned
        .signers()
        .iter()
        .map(|s| s.sig_algorithm.name())
        .collect();
    assert_eq!(alg_names.len(), 3);
    assert!(alg_names.contains(&Secp256k1::name()));
    assert!(alg_names.contains(&Ed25519.name()));
    assert!(alg_names.contains(&Secp256k1Eth::name()));
}

#[test]
fn cosigned_threshold_mixed_algorithms_quorum_satisfied() {
    let data = payload(1700000000002);
    let (s1, _) = mk_cosigner(&data, Box::new(Secp256k1));
    let (s2, _) = mk_cosigner(&data, Box::new(Ed25519));
    // Placeholder atom — counts toward the signer list but not toward
    // `valid_signers`.
    let (_, pk_c) = Secp256k1.new_key_pair();
    let placeholder = Cosigner {
        pk: pk_c,
        sig: Bytes::new(),
        sig_algorithm: Box::new(Secp256k1),
    };
    let cosigned = Cosigned::from_signed_data_threshold(data, vec![s1, s2, placeholder], 2)
        .expect("2-of-3 mixed-algorithm threshold must verify");
    assert_eq!(cosigned.signers().len(), 3);
    assert_eq!(cosigned.cosigner_threshold(), 2);
}

#[test]
fn cosigned_mixed_algorithm_tampered_signature_rejected() {
    let data = payload(1700000000003);
    let (s1, _) = mk_cosigner(&data, Box::new(Secp256k1));
    // s2 signed for a DIFFERENT payload — verification must fail.
    let other_data = payload(9999999999999);
    let (s2_for_other, _) = mk_cosigner(&other_data, Box::new(Ed25519));
    let err = Cosigned::from_signed_data(data, vec![s1, s2_for_other])
        .expect_err("tampered mixed-algorithm envelope must be rejected");
    match err {
        CosignedError::SignatureVerifyFailed { .. } => {}
        other => panic!("expected SignatureVerifyFailed, got {:?}", other),
    }
}
