//! Phase 4.9 — `rho:system:capabilities` registry verification.
//!
//! Verifies the Phase 3 capability registry's:
//! 1. Genesis-bootstrap inclusion (the contract IS in the default
//!    blessed-terms deploy list and is signed with the
//!    `CAPABILITIES_REGISTRY_PK` constant).
//! 2. URI determinism — the registry URI is content-addressed from
//!    the pubkey hash, so the same private key yields the same URI
//!    across runs (replay determinism / genesis-hash stability).
//! 3. Phase 3 `Sig::Lolly` / `Sig::Bang` wire-format dispatch
//!    carrying `capability_handle` round-trips through the algebra
//!    walker, providing the data-plane handshake the in-contract
//!    `register`/`invoke`/`revoke`/`lookup` operations use.
//!
//! Companion to the in-runtime RhoSpec tests for the registry
//! contract body itself (those drive register/invoke/revoke/lookup
//! interactions against a live runtime; the present spec exercises
//! the Rust glue + wire format).

use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use rholang::rust::interpreter::registry::registry::Registry;

use casper::rust::genesis::contracts::standard_deploys;

#[test]
fn capabilities_registry_pubkey_resolves_to_deterministic_uri() {
    let sk = PrivateKey::from_bytes(
        &hex::decode(standard_deploys::CAPABILITIES_REGISTRY_PK)
            .expect("hex decode of CAPABILITIES_REGISTRY_PK"),
    );
    let secp = Secp256k1;
    let pk1 = secp.to_public(&sk);
    let pk2 = secp.to_public(&sk);
    assert_eq!(pk1, pk2, "Secp256k1::to_public must be a pure function");

    let hash1 = Blake2b256::hash(pk1.bytes.to_vec());
    let hash2 = Blake2b256::hash(pk2.bytes.to_vec());
    assert_eq!(hash1, hash2);

    let uri1 = Registry::build_uri(&hash1);
    let uri2 = Registry::build_uri(&hash2);
    assert_eq!(uri1, uri2, "URI derivation must be deterministic");
    assert!(uri1.starts_with("rho:id:"), "URI must use rho:id: prefix");
}

#[test]
fn capabilities_registry_included_in_default_system_public_keys() {
    // The genesis blessed-terms registration uses
    // `system_public_keys()`. The capabilities registry's pubkey
    // must be present so block-validation accepts deploys signed
    // by it.
    let secp = Secp256k1;
    let sk = PrivateKey::from_bytes(
        &hex::decode(standard_deploys::CAPABILITIES_REGISTRY_PK).unwrap(),
    );
    let expected_pk = secp.to_public(&sk);

    let system_pks = standard_deploys::system_public_keys();
    let found = system_pks.iter().any(|p| *p == &expected_pk);
    assert!(
        found,
        "CAPABILITIES_REGISTRY_PUB_KEY must appear in system_public_keys()"
    );
}

#[test]
fn capabilities_registry_deploy_constructable_with_test_shard_id() {
    // Construct the genesis-deploy artifact via the standard
    // deploy generator. The body of the deploy carries the
    // post-substitution Rholang source (with the per-shard
    // `$$capabilitiesRegistryPubKey$$` / `$$capabilitiesRegistrySig$$`
    // placeholders filled in). The signature on the OUTER deploy
    // (RegistrySigGen-derived) is verified at genesis-replay time.
    let shard_id = "phase4-test";
    let deploy = standard_deploys::capabilities_registry(shard_id);
    assert!(deploy.data.term.contains("CapabilitiesRegistry"));
    assert_eq!(deploy.data.shard_id, shard_id);
    assert_eq!(
        deploy.pk.bytes.to_vec(),
        Secp256k1
            .to_public(&PrivateKey::from_bytes(
                &hex::decode(standard_deploys::CAPABILITIES_REGISTRY_PK).unwrap(),
            ))
            .bytes
            .to_vec()
    );
    assert!(!deploy.sig.is_empty(), "deploy must be signed");
    // The deploy term must NOT contain the placeholder syntax —
    // template substitution must have happened.
    assert!(!deploy.data.term.contains("$$capabilitiesRegistryPubKey$$"));
    assert!(!deploy.data.term.contains("$$capabilitiesRegistrySig$$"));
}

#[test]
fn capabilities_registry_deploy_per_shard_keys_are_stable_within_a_shard() {
    // Two invocations with the same shard_id must produce
    // identical (pk, sig, term) — this is the replay-determinism
    // foundation for capability handles since handles content-
    // address against the deployer's pubkey.
    let shard_id = "phase4-stability";
    let d1 = standard_deploys::capabilities_registry(shard_id);
    let d2 = standard_deploys::capabilities_registry(shard_id);
    assert_eq!(d1.pk.bytes, d2.pk.bytes);
    assert_eq!(d1.sig, d2.sig);
    assert_eq!(d1.data.term, d2.data.term);
    assert_eq!(d1.data.time_stamp, d2.data.time_stamp);
}

#[test]
fn capabilities_registry_uri_independent_of_shard_id() {
    // The registry URI is derived from the pubkey hash, NOT from
    // the shard_id. Two shards using the same CAPABILITIES_REGISTRY_PK
    // produce the same URI, so capability handles registered on
    // one shard could in principle be looked up via the same URI
    // shape on another shard (cross-shard handle handshake — a
    // Phase 4+ extension).
    let d_root = standard_deploys::capabilities_registry("root");
    let d_other = standard_deploys::capabilities_registry("test-shard-2");
    assert_eq!(d_root.pk.bytes, d_other.pk.bytes);
    // The term DIFFERS only in the shard_id field, but URI lives
    // in the pk-derived registry slot.
    let hash = Blake2b256::hash(d_root.pk.bytes.to_vec());
    let uri = Registry::build_uri(&hash);
    let hash2 = Blake2b256::hash(d_other.pk.bytes.to_vec());
    let uri2 = Registry::build_uri(&hash2);
    assert_eq!(uri, uri2);
}

#[test]
fn capabilities_registry_lolly_capability_handle_round_trip_via_sig_compound() {
    use models::casper::{sig_compound, SigAtom, SigCompound, SigLolly};
    use prost::Message;

    // Build a SigCompound carrying a Lolly with a non-empty
    // capability_handle. Encode → decode → verify the handle is
    // preserved (the data plane the contract uses to address
    // pre-registered capabilities).
    let handle = vec![0xCA, 0xFE, 0xBA, 0xBE, 0xDE, 0xAD, 0xBE, 0xEF];
    let from_atom = SigAtom {
        pk: vec![0x01; 33].into(),
        sig: vec![0x10; 64].into(),
        sig_algorithm: "secp256k1".to_string(),
        atom_kind: models::casper::AtomKind::Ground as i32,
    };
    let to_atom = SigAtom {
        pk: vec![0x02; 33].into(),
        sig: vec![0x20; 64].into(),
        sig_algorithm: "secp256k1".to_string(),
        atom_kind: models::casper::AtomKind::Ground as i32,
    };
    let original = SigCompound {
        connective: Some(sig_compound::Connective::Lolly(Box::new(SigLolly {
            from: Some(Box::new(SigCompound {
                connective: Some(sig_compound::Connective::Atom(from_atom)),
            })),
            to: Some(Box::new(SigCompound {
                connective: Some(sig_compound::Connective::Atom(to_atom)),
            })),
            capability_handle: handle.clone().into(),
        }))),
    };

    let encoded = original.encode_to_vec();
    let decoded = SigCompound::decode(encoded.as_slice()).expect("decode");
    match &decoded.connective {
        Some(sig_compound::Connective::Lolly(lolly)) => {
            assert_eq!(lolly.capability_handle.to_vec(), handle);
        }
        other => panic!("expected Lolly, got {:?}", other),
    }
}

#[test]
fn capabilities_registry_bang_uses_bound_round_trip_via_sig_compound() {
    use models::casper::{sig_compound, SigAtom, SigBang, SigCompound};
    use prost::Message;

    // Bang with uses_bound = 5 (bounded replication) and a
    // capability_handle. Verifies both fields survive proto
    // round-trip — they're the contract's "is this capability
    // already registered, and what's its remaining usage count"
    // handshake.
    let atom = SigAtom {
        pk: vec![0x03; 33].into(),
        sig: vec![0x30; 64].into(),
        sig_algorithm: "secp256k1".to_string(),
        atom_kind: models::casper::AtomKind::Ground as i32,
    };
    let handle = vec![0xDE, 0xAD, 0xBE, 0xEF];
    let original = SigCompound {
        connective: Some(sig_compound::Connective::Bang(Box::new(SigBang {
            inner: Some(Box::new(SigCompound {
                connective: Some(sig_compound::Connective::Atom(atom)),
            })),
            uses_bound: 5,
            capability_handle: handle.clone().into(),
        }))),
    };
    let encoded = original.encode_to_vec();
    let decoded = SigCompound::decode(encoded.as_slice()).expect("decode");
    match &decoded.connective {
        Some(sig_compound::Connective::Bang(bang)) => {
            assert_eq!(bang.uses_bound, 5);
            assert_eq!(bang.capability_handle.to_vec(), handle);
        }
        other => panic!("expected Bang, got {:?}", other),
    }
}

#[test]
fn capabilities_registry_unbounded_bang_uses_bound_zero_round_trip() {
    use models::casper::{sig_compound, SigAtom, SigBang, SigCompound};
    use prost::Message;

    let atom = SigAtom {
        pk: vec![0x04; 33].into(),
        sig: vec![0x40; 64].into(),
        sig_algorithm: "secp256k1".to_string(),
        atom_kind: models::casper::AtomKind::Ground as i32,
    };
    let original = SigCompound {
        connective: Some(sig_compound::Connective::Bang(Box::new(SigBang {
            inner: Some(Box::new(SigCompound {
                connective: Some(sig_compound::Connective::Atom(atom)),
            })),
            uses_bound: 0, // LL-canonical unbounded
            capability_handle: vec![].into(),
        }))),
    };
    let encoded = original.encode_to_vec();
    let decoded = SigCompound::decode(encoded.as_slice()).expect("decode");
    match &decoded.connective {
        Some(sig_compound::Connective::Bang(bang)) => {
            assert_eq!(bang.uses_bound, 0);
            assert!(bang.capability_handle.is_empty());
        }
        other => panic!("expected Bang, got {:?}", other),
    }
}

#[test]
fn capabilities_registry_rhox_template_contains_all_four_rpcs() {
    // The bundled contract source must implement all four RPC
    // methods declared in the Phase 3 capability registry design.
    let template = casper::rust::genesis::contracts::embedded_rho::CAPABILITIES_REGISTRY;
    assert!(template.contains("\"register\""), "must declare register RPC");
    assert!(template.contains("\"invoke\""), "must declare invoke RPC");
    assert!(template.contains("\"revoke\""), "must declare revoke RPC");
    assert!(template.contains("\"lookup\""), "must declare lookup RPC");
    assert!(
        template.contains("CapabilitiesRegistry"),
        "must use canonical contract name"
    );
}
