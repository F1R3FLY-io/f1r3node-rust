//! Phase 1 multi-signature deploy pipeline integration tests.
//!
//! Covers the wire-format-to-replay round-trip for both legacy single-signature
//! deploys (byte-identical back-compat) and multi-signature deploys (full
//! cosigner-data preservation through the checked `ProcessedDeploy` envelope.
//!
//! These tests exercise the codec/envelope/storage layers without requiring
//! a full node runtime — the runtime fan-out itself is covered by the
//! cost-accounting test suite, the §1.7 PoS contract refinement is covered
//! by the existing genesis tests + pre-charge / refund / compute-state tests
//! (3 verified back-compat tests), and the Phase 1.10 Rocq + TLA+ formal
//! mechanizations cover the algebraic correctness.

use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::{Signed, ToMessage};
use models::casper::{CompoundSigner, DeployDataProto};
use models::rhoapi::PCost;
use models::rust::casper::protocol::casper_message::{DeployData, ProcessedDeploy};
use models::rust::casper::protocol::legacy_checked_deploy::CheckedLegacyDeploy;
use prost::bytes::Bytes;
use prost::Message;

fn fresh_keypair() -> (crypto::rust::private_key::PrivateKey, PublicKey) {
    Secp256k1.new_key_pair()
}

fn baseline_deploy_data(phlo_limit: i64) -> DeployData {
    DeployData {
        term: "Nil".to_string(),
        time_stamp: 100,
        phlo_price: 1,
        phlo_limit,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
    }
}

fn sign_canonical_hash(data: &DeployData, sk: &crypto::rust::private_key::PrivateKey) -> Bytes {
    let serialized = data.to_message().encode_to_vec();
    let hash = Signed::<DeployData>::signature_hash(&Secp256k1::name(), serialized);
    Bytes::from(Secp256k1.sign(&hash, &sk.bytes))
}

fn build_multi_sig_proto(num_signers: usize) -> DeployDataProto {
    assert!(num_signers >= 2, "multi-sig requires at least 2 signers");
    let data = baseline_deploy_data((num_signers as i64) * 100);

    let (primary_sk, primary_pk) = fresh_keypair();
    let primary_sig = sign_canonical_hash(&data, &primary_sk);

    let mut cosigners = Vec::with_capacity(num_signers - 1);
    for _ in 0..(num_signers - 1) {
        let (sk, pk) = fresh_keypair();
        cosigners.push(CompoundSigner {
            // Changed by the merge of feature/cost-accounted-rho (clippy gate): the
            // value already has the field type, so `.into()` converts nothing.
            // pk: pk.bytes.clone().into(),
            pk: pk.bytes.clone(),
            sig: sign_canonical_hash(&data, &sk),
            sig_algorithm: Secp256k1::name(),
        });
    }

    DeployDataProto {
        // Changed by the merge of feature/cost-accounted-rho (clippy gate): the
        // value already has the field type, so `.into()` converts nothing.
        // deployer: primary_pk.bytes.clone().into(),
        deployer: primary_pk.bytes.clone(),
        term: data.term.clone(),
        timestamp: data.time_stamp,
        sig: primary_sig,
        sig_algorithm: Secp256k1::name(),
        phlo_price: data.phlo_price,
        phlo_limit: data.phlo_limit,
        valid_after_block_number: data.valid_after_block_number,
        shard_id: data.shard_id.clone(),
        language: String::new(),
        expiration_timestamp: 0,
        cosigners,
        cosigner_threshold: 0,
        sig_algebra: None,
        ..Default::default()
    }
}

fn build_single_sig_proto() -> DeployDataProto {
    let data = baseline_deploy_data(100);
    let (sk, pk) = fresh_keypair();
    let sig = sign_canonical_hash(&data, &sk);
    DeployDataProto {
        // Changed by the merge of feature/cost-accounted-rho (clippy gate): the
        // value already has the field type, so `.into()` converts nothing.
        // deployer: pk.bytes.clone().into(),
        deployer: pk.bytes.clone(),
        term: data.term.clone(),
        timestamp: data.time_stamp,
        sig,
        sig_algorithm: Secp256k1::name(),
        phlo_price: data.phlo_price,
        phlo_limit: data.phlo_limit,
        valid_after_block_number: data.valid_after_block_number,
        shard_id: data.shard_id.clone(),
        language: String::new(),
        expiration_timestamp: 0,
        cosigners: Vec::new(),
        cosigner_threshold: 0,
        sig_algebra: None,
        ..Default::default()
    }
}

#[test]
fn multi_sig_deploy_wire_codec_round_trip() {
    let original = build_multi_sig_proto(3);

    // Decode the wire shape into a Cosigned envelope. This exercises:
    //   - per-signer signature verification against canonical hash
    //   - canonical pk-ascending sort
    //   - no-duplicate-signer invariant
    //   D3 (DR-9): there is no Σ phlo_share == phlo_limit invariant.
    let cosigned = DeployData::from_proto_cosigned(original.clone())
        .expect("multi-sig wire deploy must decode");
    assert_eq!(cosigned.signers().len(), 3);
    assert!(cosigned.is_compound());

    // Round-trip: serialize back to proto, decode again. The cosigner list
    // must round-trip bit-identically.
    let re_proto = CheckedLegacyDeploy::from_processed_proto(original)
        .expect("checked historical wire")
        .to_proto();
    assert_eq!(re_proto.cosigners.len(), 2); // 3 signers total = primary + 2
    assert_eq!(
        re_proto.cosigners.len() + 1, // primary in fields 1/4/5
        cosigned.signers().len()
    );
    let re_cosigned = DeployData::from_proto_cosigned(re_proto)
        .expect("multi-sig deploy must round-trip through proto");
    assert_eq!(re_cosigned.signers().len(), cosigned.signers().len());
    for (a, b) in re_cosigned.signers().iter().zip(cosigned.signers().iter()) {
        assert_eq!(a.pk, b.pk);
        assert_eq!(a.sig, b.sig);
    }
}

#[test]
fn single_sig_deploy_wire_codec_back_compat() {
    let original = build_single_sig_proto();
    let cosigned = DeployData::from_proto_cosigned(original.clone())
        .expect("single-sig wire deploy must decode through cosigned path");
    assert_eq!(cosigned.signers().len(), 1);
    assert!(!cosigned.is_compound());

    // Round-trip through the checked historical envelope. For single-sig deploys,
    // cosigners must be empty — recovers the byte-identical legacy wire shape.
    let re_proto = CheckedLegacyDeploy::from_processed_proto(original.clone())
        .expect("checked historical wire")
        .to_proto();
    assert!(
        re_proto.cosigners.is_empty(),
        "single-sig round-trip must produce empty cosigners"
    );
    assert_eq!(re_proto.deployer, original.deployer);
    assert_eq!(re_proto.sig, original.sig);
}

#[test]
fn multi_sig_wire_rejects_tampered_cosigner_signature() {
    let mut tampered = build_multi_sig_proto(3);
    // Flip a byte in one cosigner's signature — the verification must fail.
    if let Some(sig_bytes) = tampered.cosigners.last_mut().map(|c| &mut c.sig) {
        let mut v: Vec<u8> = sig_bytes.to_vec();
        let last = v.last_mut().unwrap();
        *last ^= 0x01;
        *sig_bytes = Bytes::from(v);
    }
    let err = DeployData::from_proto_cosigned(tampered)
        .expect_err("tampered cosigner signature must be rejected");
    assert!(
        err.contains("failed signature verification")
            || err.contains("SignatureVerifyFailed")
            || err.contains("verification failed"),
        "expected signature verification rejection, got: {}",
        err
    );
}

// D3 (DR-9): `multi_sig_wire_rejects_share_sum_mismatch` is removed — there is
// no `Σ phlo_share == phlo_limit` invariant to violate.

#[test]
fn multi_sig_wire_rejects_duplicate_signer() {
    let mut dup = build_multi_sig_proto(3);
    // Replace the second cosigner's pk with the primary's — duplicate.
    let primary_pk = dup.deployer.clone();
    if let Some(first_cosigner) = dup.cosigners.first_mut() {
        first_cosigner.pk = primary_pk;
        // Note: signature is now invalid (signed with different key), so
        // we'd hit either DuplicateSigner OR SignatureVerifyFailed depending
        // on canonical-sort order. Either is acceptable rejection behavior.
    }
    let _err = DeployData::from_proto_cosigned(dup)
        .expect_err("duplicate signer or invalid sig must be rejected");
}

#[test]
fn processed_deploy_legacy_uplift_is_checked() {
    let data = baseline_deploy_data(100);
    let (sk, _pk) = fresh_keypair();
    let signed = Signed::<DeployData>::create(data, Box::new(Secp256k1), sk).expect("sign");
    let mut pd = ProcessedDeploy::empty(signed.clone()).expect("checked legacy uplift");
    pd.cost = PCost { cost: 10 };
    assert_eq!(pd.signers().len(), 1);
    assert_eq!(pd.primary().pk, signed.pk);
    assert_eq!(pd.primary().sig, signed.sig);
}

#[test]
fn processed_deploy_multi_sig_authority_is_checked() {
    let original = build_multi_sig_proto(3);
    let cosigned_decoded =
        DeployData::from_proto_cosigned(original.clone()).expect("decode original");

    let checked = CheckedLegacyDeploy::from_processed_proto(original).unwrap();
    let mut pd = ProcessedDeploy::from_checked_envelope(checked);
    pd.cost = PCost { cost: 50 };
    assert_eq!(pd.signers().len(), 3);
    // Canonical sort preserved (primary at index 0 of the reconstructed
    // envelope may differ from `cosigned_decoded.primary()` after re-sort
    // because Cosigned::from_signed_data re-canonicalizes; both envelopes
    // must agree on the SET of signer pks).
    let mut expected_pks: Vec<Bytes> = cosigned_decoded
        .signers()
        .iter()
        .map(|c| c.pk.bytes.clone())
        .collect();
    expected_pks.sort();
    let mut got_pks: Vec<Bytes> = pd.signers().iter().map(|c| c.pk.bytes.clone()).collect();
    got_pks.sort();
    assert_eq!(got_pks, expected_pks);
}

#[test]
fn processed_deploy_proto_round_trip_preserves_cosigners() {
    // ProcessedDeploy with multi-sig cosigners survives the proto serializer.
    let original_proto = build_multi_sig_proto(4);
    let checked = CheckedLegacyDeploy::from_processed_proto(original_proto.clone()).unwrap();
    let mut pd_before = ProcessedDeploy::from_checked_envelope(checked);
    pd_before.cost = PCost { cost: 75 };
    let pd_proto = pd_before.clone().to_proto();
    // Cosigners should be in the inner DeployDataProto (D3: no primary_phlo_share).
    let inner_deploy = pd_proto.deploy.as_ref().expect("proto deploy field");
    assert_eq!(inner_deploy.cosigners.len(), 3);

    let pd_after = ProcessedDeploy::from_proto(pd_proto).expect("from_proto decode");
    assert_eq!(pd_after.signers().len(), pd_before.signers().len());
    assert_eq!(pd_after.threshold(), pd_before.threshold());
    assert_eq!(pd_after.checked_envelope().to_proto(), original_proto);
}

#[test]
fn legacy_single_sig_processed_deploy_proto_round_trip_unchanged() {
    let original_proto = build_single_sig_proto();
    let signed = DeployData::from_proto(original_proto.clone()).expect("legacy single-sig decode");
    let mut pd_before = ProcessedDeploy::empty(signed).expect("checked single signature");
    pd_before.cost = PCost { cost: 25 };
    let pd_proto = pd_before.clone().to_proto();
    let inner_deploy = pd_proto.deploy.as_ref().expect("proto deploy field");
    // Legacy single-sig: cosigners empty (D3: no primary_phlo_share).
    assert!(inner_deploy.cosigners.is_empty());

    let pd_after = ProcessedDeploy::from_proto(pd_proto).expect("from_proto decode");
    assert_eq!(pd_after.signers().len(), 1);
    assert_eq!(pd_after.primary().pk, pd_before.primary().pk);
    assert_eq!(pd_after.identity_bytes(), pd_before.identity_bytes());
}

// =====================================================================
// Phase 4.11 — sig_algebra dispatch tests (extends the wire pipeline
// with explicit `DeployDataProto.sig_algebra` routing through
// `DeployData::from_proto_cosigned_with_sig_algebra`).
// =====================================================================

use models::casper::{sig_compound, SigAtom, SigCompound, SigPair, SigPlus, SigThreshold};

// D3 (DR-9): `phlo_share` retained as an (ignored) param for caller stability;
// a sig atom carries no escrow share.
fn make_signed_atom(data: &DeployData, _phlo_share: i64) -> SigAtom {
    let (sk, pk) = fresh_keypair();
    SigAtom {
        // Changed by the merge of feature/cost-accounted-rho (clippy gate): the
        // value already has the field type, so `.into()` converts nothing.
        // pk: pk.bytes.clone().into(),
        pk: pk.bytes.clone(),
        sig: sign_canonical_hash(data, &sk),
        sig_algorithm: Secp256k1::name(),
        atom_kind: models::casper::AtomKind::Ground as i32,
    }
}

fn make_compound_from_atom(atom: SigAtom) -> SigCompound {
    SigCompound {
        connective: Some(sig_compound::Connective::Atom(atom)),
    }
}

#[test]
fn sig_algebra_overrides_flat_cosigners_routes_via_algebra_dispatch() {
    // Build a proto with BOTH `cosigners` AND `sig_algebra` populated.
    // The Phase 3 dispatch in `from_proto_cosigned` MUST ignore the flat
    // `cosigners[]` field and route through the algebra walk.
    let data = baseline_deploy_data(200);
    let atom_a = make_signed_atom(&data, 100);
    let atom_b = make_signed_atom(&data, 100);
    let algebra = SigCompound {
        connective: Some(sig_compound::Connective::Tensor(Box::new(SigPair {
            left: Some(Box::new(make_compound_from_atom(atom_a))),
            right: Some(Box::new(make_compound_from_atom(atom_b))),
        }))),
    };

    // Garbage value in the flat cosigners[] that would fail validation
    // if the dispatch routed through the flat path:
    let (sk_dummy, pk_dummy) = fresh_keypair();
    let bogus_cosigner = CompoundSigner {
        // Changed by the merge of feature/cost-accounted-rho (clippy gate): the
        // value already has the field type, so `.into()` converts nothing.
        // pk: pk_dummy.bytes.into(),
        pk: pk_dummy.bytes,
        sig: sign_canonical_hash(&baseline_deploy_data(999), &sk_dummy), // wrong-deploy sig
        sig_algorithm: Secp256k1::name(),
    };

    let (primary_sk, primary_pk) = fresh_keypair();
    let proto = DeployDataProto {
        // Changed by the merge of feature/cost-accounted-rho (clippy gate): the
        // value already has the field type, so `.into()` converts nothing.
        // deployer: primary_pk.bytes.clone().into(),
        deployer: primary_pk.bytes.clone(),
        term: data.term.clone(),
        timestamp: data.time_stamp,
        sig: sign_canonical_hash(&data, &primary_sk),
        sig_algorithm: Secp256k1::name(),
        phlo_price: data.phlo_price,
        phlo_limit: data.phlo_limit,
        valid_after_block_number: data.valid_after_block_number,
        shard_id: data.shard_id.clone(),
        language: String::new(),
        expiration_timestamp: 0,
        cosigners: vec![bogus_cosigner],
        cosigner_threshold: 0,
        sig_algebra: Some(algebra),
        ..Default::default()
    };

    // The flat-cosigners path would fail (bogus sig), but the sig_algebra path
    // validates the two real atoms and succeeds.
    let cosigned = DeployData::from_proto_cosigned(proto)
        .expect("sig_algebra dispatch must succeed despite bogus flat cosigners");
    assert_eq!(cosigned.signers().len(), 2);
}

#[test]
fn sig_algebra_tensor_3_atoms_processed_deploy_round_trip() {
    let data = baseline_deploy_data(300);
    let atom_a = make_signed_atom(&data, 100);
    let atom_b = make_signed_atom(&data, 100);
    let atom_c = make_signed_atom(&data, 100);
    // Build And(a, And(b, c)) — right-associated tensor tree.
    let algebra = SigCompound {
        connective: Some(sig_compound::Connective::Tensor(Box::new(SigPair {
            left: Some(Box::new(make_compound_from_atom(atom_a))),
            right: Some(Box::new(SigCompound {
                connective: Some(sig_compound::Connective::Tensor(Box::new(SigPair {
                    left: Some(Box::new(make_compound_from_atom(atom_b))),
                    right: Some(Box::new(make_compound_from_atom(atom_c))),
                }))),
            })),
        }))),
    };

    let cosigned = DeployData::from_proto_cosigned_with_sig_algebra(data, &algebra)
        .expect("Tensor with 3 atoms must verify");
    assert_eq!(cosigned.signers().len(), 3);
}

#[test]
fn sig_algebra_threshold_2_of_3_processed_deploy_round_trip() {
    let data = baseline_deploy_data(200);
    let atom_a = make_signed_atom(&data, 100);
    let atom_b = make_signed_atom(&data, 100);
    // Third atom presented but UNSIGNED (placeholder) — quorum 2/3
    // tolerates one absent.
    let (_, pk_c) = fresh_keypair();
    let placeholder = SigAtom {
        // Changed by the merge of feature/cost-accounted-rho (clippy gate): the
        // value already has the field type, so `.into()` converts nothing.
        // pk: pk_c.bytes.clone().into(),
        pk: pk_c.bytes.clone(),
        sig: Bytes::new(),
        sig_algorithm: Secp256k1::name(),
        atom_kind: models::casper::AtomKind::Ground as i32,
    };
    let algebra = SigCompound {
        connective: Some(sig_compound::Connective::Threshold(SigThreshold {
            threshold: 2,
            members: vec![
                make_compound_from_atom(atom_a),
                make_compound_from_atom(atom_b),
                make_compound_from_atom(placeholder),
            ],
        })),
    };
    let cosigned = DeployData::from_proto_cosigned_with_sig_algebra(data, &algebra)
        .expect("Threshold 2-of-3 with 2 valid sigs must verify");
    assert_eq!(cosigned.signers().len(), 3);
    assert_eq!(cosigned.cosigner_threshold(), 2);
}

/// F-A funding/capability separation — INGRESS REJECT (c),
/// `docs/theory/cost-accounting-impl/f-a-funding-vs-capability-separation.md`
/// §3/§6: `Plus` (⊕) is a value/capability connective, NOT a funding-signature
/// former (§App-A `g|#P|s∘s`). The deploy-decode path now rejects it at the wire
/// boundary BEFORE the `chosen_branch` range check, so an out-of-range
/// `chosen_branch` surfaces the connective-rejection rather than the branch
/// error (this test previously asserted the `chosen_branch` message).
#[test]
fn sig_algebra_plus_rejected_at_ingress_before_chosen_branch_check() {
    let data = baseline_deploy_data(100);
    let atom_a = make_signed_atom(&data, 100);
    let atom_b = make_signed_atom(&data, 100);
    let algebra = SigCompound {
        connective: Some(sig_compound::Connective::Plus(Box::new(SigPlus {
            left: Some(Box::new(make_compound_from_atom(atom_a))),
            right: Some(Box::new(make_compound_from_atom(atom_b))),
            chosen_branch: 99, // invalid — but the ⊕ connective is rejected first
        }))),
    };
    let err = DeployData::from_proto_cosigned_with_sig_algebra(data, &algebra)
        .expect_err("Plus (⊕) is rejected at ingress before chosen_branch validation");
    assert!(
        err.contains("Plus") && err.contains("not a funding-signature former"),
        "F-A ingress reject must fire before the chosen_branch check: {}",
        err
    );
}

#[test]
fn sig_algebra_unknown_signature_algorithm_rejected() {
    let data = baseline_deploy_data(100);
    // Atom with a sig_algorithm string that doesn't resolve via
    // SignaturesAlgFactory::apply.
    let (sk, pk) = fresh_keypair();
    let atom_bad = SigAtom {
        // Changed by the merge of feature/cost-accounted-rho (clippy gate): the
        // value already has the field type, so `.into()` converts nothing.
        // pk: pk.bytes.into(),
        pk: pk.bytes,
        sig: sign_canonical_hash(&data, &sk),
        sig_algorithm: "nonexistent_alg_v9999".to_string(),
        atom_kind: models::casper::AtomKind::Ground as i32,
    };
    let algebra = SigCompound {
        connective: Some(sig_compound::Connective::Atom(atom_bad)),
    };
    let err = DeployData::from_proto_cosigned_with_sig_algebra(data, &algebra)
        .expect_err("unknown sig_algorithm must be rejected");
    assert!(
        err.contains("Unknown signature algorithm") || err.contains("nonexistent_alg_v9999"),
        "error must reference the unknown algorithm: {}",
        err
    );
}
