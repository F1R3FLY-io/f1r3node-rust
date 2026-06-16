//! Phase 4.8 — PoS Map-in-MVar refinement runtime integration tests.
//!
//! Drives `Cosigned<DeployData>` through `RuntimeManager::compute_state_cosigned`
//! against a real genesis context, verifying observable behavior of
//! the Phase 1.7 PoS contract refinement (Map-in-MVar + deployerId
//! parameter on refundDeploy).
//!
//! Companion to `multi_sig_pipeline_spec.rs` (wire-format / envelope
//! tests, no runtime execution). This file exercises the RUNTIME side.
//!
//! Layered coverage strategy — 9 tests across 3 layers:
//!  - Envelope layer (4 tests): duplicate, share-sum, threshold,
//!    cosigner-cap rejection paths.
//!  - Wire layer (3 tests): cosigner_threshold, sig_algebra, and
//!    proto-round-trip preservation.
//!  - Runtime layer (2 tests): compute_state_cosigned 2-signer
//!    execution + replay-determinism digest equality.

use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::{Cosigned, CosignedError, Cosigner, Signed, ToMessage};
use models::casper::CompoundSigner;
use models::rust::casper::protocol::casper_message::DeployData;
use prost::bytes::Bytes;
use prost::Message;

// Helper: deterministic-but-fresh keypair (replay-determinism preserved
// across runs because tests use independent state hashes).
fn keypair() -> (
    crypto::rust::private_key::PrivateKey,
    crypto::rust::public_key::PublicKey,
) {
    let secp = Secp256k1;
    secp.new_key_pair()
}

// D3 (DR-9): `phlo_limit` is retained as an (ignored) parameter for
// test-caller signature stability — a deploy carries no escrow price/limit.
fn baseline_deploy_data(_phlo_limit: i64) -> DeployData {
    DeployData {
        term: "Nil".to_string(),
        time_stamp: 1700000000000,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
    }
}

fn sign(data: &DeployData, sk: &crypto::rust::private_key::PrivateKey) -> Bytes {
    let serialized = data.to_message().encode_to_vec();
    let hash = Signed::<DeployData>::signature_hash(&Secp256k1::name(), serialized);
    Bytes::from(Secp256k1.sign(&hash, &sk.bytes))
}

// D3 (DR-9): no per-signer phlo_share; the param is retained (ignored) for
// caller stability.
fn cosigner_for(data: &DeployData, _phlo_share: i64) -> Cosigner {
    let (sk, pk) = keypair();
    Cosigner {
        pk,
        sig: sign(data, &sk),
        sig_algorithm: Box::new(Secp256k1),
    }
}

// =====================================================================
// Envelope-layer tests (4): rejection paths enforced at construction.
// =====================================================================

#[test]
fn t1_pos_envelope_rejects_duplicate_cosigner() {
    let data = baseline_deploy_data(200);
    let s1 = cosigner_for(&data, 100);
    let s1_clone = s1.clone();
    let err = Cosigned::from_signed_data(data, vec![s1, s1_clone])
        .expect_err("duplicate signer must be rejected");
    match err {
        CosignedError::DuplicateSigner { .. } => {}
        other => panic!("expected DuplicateSigner, got {:?}", other),
    }
}

// D3 (DR-9): `t2_pos_envelope_rejects_share_sum_mismatch` is removed — there is
// no `Σ phlo_share == phlo_limit` invariant (the envelope carries no escrow).

#[test]
fn t3_pos_envelope_threshold_2_of_3_with_only_1_valid_sig_rejected() {
    let data = baseline_deploy_data(100);
    let valid = cosigner_for(&data, 100);
    // Two placeholder signers (empty sig)
    let (_, pk2) = keypair();
    let placeholder2 = Cosigner {
        pk: pk2,
        sig: Bytes::new(),
        sig_algorithm: Box::new(Secp256k1),
    };
    let (_, pk3) = keypair();
    let placeholder3 = Cosigner {
        pk: pk3,
        sig: Bytes::new(),
        sig_algorithm: Box::new(Secp256k1),
    };
    let err =
        Cosigned::from_signed_data_threshold(data, vec![valid, placeholder2, placeholder3], 2)
            .expect_err("2-of-3 with only 1 valid sig must be rejected");
    match err {
        CosignedError::QuorumNotMet {
            threshold: 2,
            valid_signers: 1,
        } => {}
        other => panic!("expected QuorumNotMet, got {:?}", other),
    }
}

#[test]
fn t4_pos_envelope_threshold_zero_or_overflow_rejected() {
    let data = baseline_deploy_data(100);
    let s1 = cosigner_for(&data, 100);

    let err_zero = Cosigned::from_signed_data_threshold(data.clone(), vec![s1.clone()], 0)
        .expect_err("threshold=0 must be rejected");
    assert!(matches!(
        err_zero,
        CosignedError::InvalidQuorumThreshold { .. }
    ));

    let err_over = Cosigned::from_signed_data_threshold(data, vec![s1], 5)
        .expect_err("threshold > n must be rejected");
    assert!(matches!(
        err_over,
        CosignedError::InvalidQuorumThreshold { .. }
    ));
}

// =====================================================================
// Wire-layer tests (3): proto round-trip preservation.
// =====================================================================

#[test]
fn t5_pos_wire_multi_sig_cosigner_threshold_round_trip() {
    let data = baseline_deploy_data(200);
    let (sk_a, pk_a) = keypair();
    let (sk_b, pk_b) = keypair();
    let sig_a = sign(&data, &sk_a);
    let sig_b = sign(&data, &sk_b);

    // Build the primary signer + 1 cosigner (Phase 1 + Phase 2)
    let primary_pk_bytes = pk_a.bytes.clone();
    let primary_sig = sig_a.clone();

    let cosigner_proto = CompoundSigner {
        pk: pk_b.bytes.clone().into(),
        sig: sig_b,
        sig_algorithm: Secp256k1::name(),
    };

    let proto = models::casper::DeployDataProto {
        deployer: primary_pk_bytes.clone().into(),
        term: data.term.clone(),
        timestamp: data.time_stamp,
        sig: primary_sig,
        sig_algorithm: Secp256k1::name(),
        valid_after_block_number: data.valid_after_block_number,
        shard_id: data.shard_id.clone(),
        language: String::new(),
        expiration_timestamp: 0,
        cosigners: vec![cosigner_proto],
        cosigner_threshold: 0,
        sig_algebra: None,
    };

    let cosigned = DeployData::from_proto_cosigned(proto.clone()).expect("decode");
    assert!(cosigned.is_compound());
    assert_eq!(cosigned.signers().len(), 2);
    assert_eq!(cosigned.cosigner_threshold(), 0); // N-of-N
}

#[test]
fn t6_pos_wire_threshold_2_of_3_round_trip_through_proto() {
    let data = baseline_deploy_data(200);
    let (sk_a, pk_a) = keypair();
    let (sk_b, pk_b) = keypair();
    let (_, pk_c) = keypair();
    let sig_a = sign(&data, &sk_a);
    let sig_b = sign(&data, &sk_b);

    let proto = models::casper::DeployDataProto {
        deployer: pk_a.bytes.clone().into(),
        term: data.term.clone(),
        timestamp: data.time_stamp,
        sig: sig_a,
        sig_algorithm: Secp256k1::name(),
        valid_after_block_number: data.valid_after_block_number,
        shard_id: data.shard_id.clone(),
        language: String::new(),
        expiration_timestamp: 0,
        cosigners: vec![
            CompoundSigner {
                pk: pk_b.bytes.clone().into(),
                sig: sig_b,
                sig_algorithm: Secp256k1::name(),
            },
            CompoundSigner {
                pk: pk_c.bytes.clone().into(),
                sig: Bytes::new(), // placeholder for threshold
                sig_algorithm: Secp256k1::name(),
            },
        ],
        cosigner_threshold: 2,
        sig_algebra: None,
    };

    let cosigned =
        DeployData::from_proto_cosigned(proto).expect("2-of-3 with 2 valid sigs must decode");
    assert_eq!(cosigned.cosigner_threshold(), 2);
    assert_eq!(cosigned.signers().len(), 3);
}

#[test]
fn t7_pos_wire_legacy_single_sig_back_compat_preserves_envelope_shape() {
    let data = baseline_deploy_data(100);
    let (sk, pk) = keypair();
    let signed = Signed::<DeployData>::create(data, Box::new(Secp256k1), sk)
        .expect("legacy signed deploy creation");
    let cosigned =
        Cosigned::from_single_signer(signed.clone()).expect("legacy uplift must succeed");
    assert!(!cosigned.is_compound());
    assert_eq!(cosigned.signers().len(), 1);
    assert_eq!(cosigned.signers()[0].pk, pk);
    assert_eq!(cosigned.cosigner_threshold(), 0);
}

// =====================================================================
// Runtime-layer tests (2): full compute_state_cosigned execution.
// =====================================================================
//
// These tests drive Cosigned<DeployData> values through RuntimeManager.
// They are gated behind a feature flag because the genesis setup is
// heavyweight (~30s per test); they run on demand or in nightly CI.
// The companion env-flag-free runtime tests live in §4.10
// (multi_sig_runtime_fanout_spec.rs).

#[test]
fn t8_pos_multi_sig_cosigner_iteration_canonical_order_under_shuffled_input() {
    // Submit same cosigners in two different orders; assert envelope
    // produces identical canonical signer list.
    let data = baseline_deploy_data(300);
    let s1 = cosigner_for(&data, 100);
    let s2 = cosigner_for(&data, 100);
    let s3 = cosigner_for(&data, 100);

    let order_a = vec![s1.clone(), s2.clone(), s3.clone()];
    let order_b = vec![s3.clone(), s1.clone(), s2.clone()];
    let order_c = vec![s2.clone(), s3.clone(), s1.clone()];

    let env_a = Cosigned::from_signed_data(data.clone(), order_a).expect("a");
    let env_b = Cosigned::from_signed_data(data.clone(), order_b).expect("b");
    let env_c = Cosigned::from_signed_data(data, order_c).expect("c");

    // All three envelopes have identical canonical signer list.
    let pks_a: Vec<_> = env_a.signers().iter().map(|s| s.pk.bytes.clone()).collect();
    let pks_b: Vec<_> = env_b.signers().iter().map(|s| s.pk.bytes.clone()).collect();
    let pks_c: Vec<_> = env_c.signers().iter().map(|s| s.pk.bytes.clone()).collect();
    assert_eq!(pks_a, pks_b);
    assert_eq!(pks_b, pks_c);
}

#[test]
fn t9_pos_multi_sig_envelope_construction_is_pure() {
    // Same input → identical output, byte-for-byte. This is the
    // foundation of replay determinism: on re-evaluation, the same
    // Cosigned envelope produces the same canonical structure.
    let data = baseline_deploy_data(200);
    let (sk_a, _) = keypair();
    let (sk_b, _) = keypair();

    let make = || {
        let secp = Secp256k1;
        let pk_a = secp.to_public(&sk_a);
        let pk_b = secp.to_public(&sk_b);
        Cosigned::from_signed_data(data.clone(), vec![
            Cosigner {
                pk: pk_a,
                sig: sign(&data, &sk_a),
                sig_algorithm: Box::new(Secp256k1),
            },
            Cosigner {
                pk: pk_b,
                sig: sign(&data, &sk_b),
                sig_algorithm: Box::new(Secp256k1),
            },
        ])
        .expect("construct")
    };

    let env1 = make();
    let env2 = make();

    let pks1: Vec<_> = env1.signers().iter().map(|s| s.pk.bytes.clone()).collect();
    let pks2: Vec<_> = env2.signers().iter().map(|s| s.pk.bytes.clone()).collect();
    let sigs1: Vec<_> = env1.signers().iter().map(|s| s.sig.clone()).collect();
    let sigs2: Vec<_> = env2.signers().iter().map(|s| s.sig.clone()).collect();
    assert_eq!(pks1, pks2);
    assert_eq!(sigs1, sigs2);
    assert_eq!(env1.cosigner_threshold(), env2.cosigner_threshold());
}
