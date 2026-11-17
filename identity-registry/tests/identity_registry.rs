// Integration tests for the identity registry.
// Requires:
//   - A running F1R3FLY cluster on 127.0.0.1:40422 (gRPC internal), or on
//     the port in F1R3FLY_GRPC_PORT
//   - F1R3FLY_SIGNING_KEY env var set to a 32-byte hex secp256k1 key with
//     REV balance on that cluster. It signs the deploys only.
//   - The genesis identity registry admin set of docker/quorum.conf: 2 of
//     ADMIN_SKS (`make firefly`).
//
// Run with: cargo test --features integration_tests -- --nocapture

#![cfg(feature = "integration_tests")]

use std::env;
use std::sync::OnceLock;
use std::time::Duration;

use identity_registry::payload::{derive_name, register_payload, NAME_LEN, NONCE_LEN};
use identity_registry::reply::{fetch_reply, parse_tagged_reply};
use identity_registry::signing::{admin_sign, sign_payload, signature_der, uncompressed_pk_bytes};
use identity_registry::term::{new_reply_slot, register_term};
use identity_registry::{IdentityRegistryClient, IdentityRegistryError};
use rand::RngCore;
use secp256k1::{PublicKey, Secp256k1, SecretKey};

const NODE_HOST: &str = "127.0.0.1";
const DEFAULT_GRPC_PORT: u16 = 40422;
const REGISTRY_URI: &str = "rho:system:identityRegistry";

// --- Harness -----------------------------------------------------------------

/// F1R3FLY_GRPC_PORT overrides the port, for CI's remapped host ports.
fn grpc_port() -> u16 {
    env::var("F1R3FLY_GRPC_PORT")
        .map(|p| p.parse().expect("F1R3FLY_GRPC_PORT must be a port number"))
        .unwrap_or(DEFAULT_GRPC_PORT)
}

fn signing_key() -> &'static str {
    static SK: OnceLock<String> = OnceLock::new();
    SK.get_or_init(|| env::var("F1R3FLY_SIGNING_KEY").expect("F1R3FLY_SIGNING_KEY must be set"))
        .as_str()
}

/// 5s between deploys so each test op lands in its own block. Avoids the
/// lost-update / empty-reply pattern that shows up when the cluster batches
/// several pending deploys into one block.
const POST_DEPLOY_WAIT: Duration = Duration::from_secs(5);

/// Every test shares the genesis registry but operates on its own fresh
/// keypair/name so they do not interfere.
async fn test_client() -> IdentityRegistryClient {
    let mut client = IdentityRegistryClient::new(
        signing_key(),
        NODE_HOST,
        grpc_port(),
        Some(REGISTRY_URI.into()),
    )
    .expect("init client");
    client.post_deploy_wait = POST_DEPLOY_WAIT;
    client
}

fn fresh_keypair() -> (SecretKey, PublicKey) { Secp256k1::new().generate_keypair(&mut rand::rng()) }

fn fresh_nonce() -> [u8; NONCE_LEN] {
    let mut buf = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut buf);
    buf
}

/// The bootstrap, validator1 and validator2 dev keys of docker/.env.example.
/// docker/quorum.conf makes their public keys the admin set, quorum 2.
const ADMIN_SKS: [&str; 3] = [
    "5f668a7ee96d944a4494cc947e4005e172d7ab3461ee5538f1f2a45a835e9657",
    "357cdc4201a5650830e0bc5a03299a30038d9934ba4c7ab73ec164ad82471ff9",
    "2c02138097d019d263c1d5383fcaddb1ba6416a0f4e64e3a617fe3af45b7851d",
];

fn admin_sk(i: usize) -> SecretKey {
    let arr: [u8; 32] = hex::decode(ADMIN_SKS[i]).unwrap().try_into().unwrap();
    SecretKey::from_byte_array(arr).unwrap()
}

fn admin_pk(sk: &SecretKey) -> [u8; 65] {
    uncompressed_pk_bytes(&PublicKey::from_secret_key(&Secp256k1::new(), sk))
}

// --- REG-AC1 -----------------------------------------------------------------

/// A registration signed by a fresh keypair mints a name, and the name
/// resolves to the registering key and the height of the block that minted
/// it. The height is the deploy's, so it must be a real on-chain block
/// number rather than a value the payload could carry.
#[cfg_attr(not(feature = "integration_tests"), ignore)]
#[tokio::test]
async fn register_mints_name_resolving_to_key() {
    let client = test_client().await;
    let (sk, pk) = fresh_keypair();
    let nonce = fresh_nonce();

    let name = client.register(&sk, &nonce).await.expect("register");

    let resolved = client.resolve_current(&name).await.expect("resolve");
    assert_eq!(resolved.pk, uncompressed_pk_bytes(&pk));
    assert_eq!(resolved.seq, 0);
    assert!(
        resolved.block_height > 0,
        "expected a real block height for the minting block, got {}",
        resolved.block_height
    );
}

// --- REG-AC2 -----------------------------------------------------------------

/// A payload signed by a different key mints no name, and the refusal is
/// distinguishable from whatever the submission's own finalisation state is.
#[cfg_attr(not(feature = "integration_tests"), ignore)]
#[tokio::test]
async fn bad_signature_registers_nothing() {
    let client = test_client().await;
    let (_sk_a, pk_a) = fresh_keypair();
    let (sk_b, _pk_b) = fresh_keypair();
    let nonce = fresh_nonce();

    // Build register payload claiming pk_a but sign with sk_b.
    let pk_a_bytes = uncompressed_pk_bytes(&pk_a);
    let payload = register_payload(&pk_a_bytes, &nonce);
    let sig = sign_payload(&payload, &sk_b);
    let sig_der = signature_der(&sig);

    let slot = new_reply_slot();
    let uri = client.registry_uri.as_deref().unwrap();
    let term = register_term(uri, &pk_a_bytes, &nonce, &sig_der, &slot);
    let block_hash = client.deploy(&term).await.expect("deploy");
    let reply = fetch_reply(&client.node_host, client.grpc_port, &slot, &block_hash)
        .await
        .expect("fetch reply");
    let parsed = parse_tagged_reply(reply);
    match parsed {
        Err(IdentityRegistryError::Rejected(r)) => assert_eq!(r, "bad_signature"),
        other => panic!("expected Rejected(bad_signature), got {other:?}"),
    }

    // Verify the name was not minted.
    let expected_name = derive_name(&pk_a_bytes, &nonce);
    let resolve_result = client.resolve_current(&expected_name).await;
    assert!(
        matches!(resolve_result, Err(IdentityRegistryError::Rejected(ref r)) if r == "unknown_name"),
        "expected unknown_name, got {resolve_result:?}"
    );
}

// --- REG-AC3 -----------------------------------------------------------------

/// A second party resolves the first party's name to the keys linked to
/// it. An unregistered name resolves to nothing.
#[cfg_attr(not(feature = "integration_tests"), ignore)]
#[tokio::test]
async fn resolve_returns_keys_or_nothing() {
    let client = test_client().await;
    let (sk_a, pk_a) = fresh_keypair();
    let nonce = fresh_nonce();
    let name = client.register(&sk_a, &nonce).await.expect("register");

    // A "second party" resolve: same client API, different caller intent.
    let resolved = client.resolve_current(&name).await.expect("resolve");
    assert_eq!(resolved.pk, uncompressed_pk_bytes(&pk_a));

    let unknown = [0u8; NAME_LEN];
    let result = client.resolve_current(&unknown).await;
    assert!(
        matches!(result, Err(IdentityRegistryError::Rejected(ref r)) if r == "unknown_name"),
        "expected unknown_name, got {result:?}"
    );
}

// --- REG-AC4 -----------------------------------------------------------------

/// After a rotation signed by the current key, the new key is accepted for
/// speaking as the identity and the old one is rejected. The name is the
/// same before and after, and the old key is still readable as a previous
/// key of that name.
#[cfg_attr(not(feature = "integration_tests"), ignore)]
#[tokio::test]
async fn rotate_accepts_new_rejects_old_name_stable() {
    let client = test_client().await;
    let (sk1, pk1) = fresh_keypair();
    let nonce = fresh_nonce();
    let name = client.register(&sk1, &nonce).await.expect("register");

    let (sk2, pk2) = fresh_keypair();
    let new_seq = client.rotate(&sk1, &name, &sk2, 0).await.expect("rotate");
    assert_eq!(new_seq, 1);

    // The name is unchanged, and resolve now returns pk2.
    let resolved = client.resolve_current(&name).await.expect("resolve");
    assert_eq!(resolved.pk, uncompressed_pk_bytes(&pk2));
    assert_eq!(resolved.seq, 1);

    // The old key is rejected for further rotation.
    let (sk3, _pk3) = fresh_keypair();
    let old_key_rotation = client.rotate(&sk1, &name, &sk3, 1).await;
    assert!(
        matches!(old_key_rotation, Err(IdentityRegistryError::Rejected(ref r)) if r == "bad_signature"),
        "expected bad_signature when rotating with the superseded key, got {old_key_rotation:?}"
    );

    // The old key is still readable in history.
    let history = client.resolve_history(&name).await.expect("history");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].pk, uncompressed_pk_bytes(&pk1));
    assert_eq!(history[0].op, "register");
    assert_eq!(history[1].pk, uncompressed_pk_bytes(&pk2));
    assert_eq!(history[1].op, "rotate");
}

// --- REG-AC5 -----------------------------------------------------------------

/// Rotation leaves the number of names unchanged while the number of keys
/// linked to the rotating name increases. A rotation not signed by the
/// current key is rejected.
#[cfg_attr(not(feature = "integration_tests"), ignore)]
#[tokio::test]
async fn rotation_adds_key_not_identity() {
    let client = test_client().await;
    let (sk1, _pk1) = fresh_keypair();
    let nonce = fresh_nonce();
    let name = client.register(&sk1, &nonce).await.expect("register");

    let history_before = client.resolve_history(&name).await.expect("history before");
    assert_eq!(history_before.len(), 1);

    let (sk2, _pk2) = fresh_keypair();
    client.rotate(&sk1, &name, &sk2, 0).await.expect("rotate");

    let history_after = client.resolve_history(&name).await.expect("history after");
    // Same name throughout. Key count grew by exactly one.
    assert_eq!(history_after.len(), history_before.len() + 1);

    // A rotation not signed by the current key is rejected.
    let (sk_stranger, _) = fresh_keypair();
    let (sk3, _pk3) = fresh_keypair();
    let result = client.rotate(&sk_stranger, &name, &sk3, 1).await;
    assert!(
        matches!(result, Err(IdentityRegistryError::Rejected(ref r)) if r == "bad_signature"),
        "expected bad_signature when rotating with a stranger key, got {result:?}"
    );
}

// --- REG-AC6 -----------------------------------------------------------------

/// Submitting the same registration twice mints one name.
#[cfg_attr(not(feature = "integration_tests"), ignore)]
#[tokio::test]
async fn double_registration_mints_one_name() {
    let client = test_client().await;
    let (sk, pk) = fresh_keypair();
    let nonce = fresh_nonce();

    let name1 = client.register(&sk, &nonce).await.expect("first register");
    let name2 = client.register(&sk, &nonce).await.expect("second register");
    assert_eq!(name1, name2);

    // Only one entry in history for that name.
    let history = client.resolve_history(&name1).await.expect("history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].op, "register");
    assert_eq!(history[0].pk, uncompressed_pk_bytes(&pk));
}

// --- REG-AC7 -----------------------------------------------------------------

// "Capability by possession, not by table." The behavioural check is that a
// party holding a key linked to a name can post in a reputation channel and
// refute a claim. Both require the Ledger milestone, so this AC is deferred
// to that milestone's test suite.
//
// What is testable here, and is covered by the other ACs: no operation in
// this contract grants or consults a stored permission, and the only
// gating is the signature check.
//
// --- REG-AC8 -----------------------------------------------------------------

/// No operation removes a name or a key, and a claim signed under a
/// superseded key stays readable and attributed to that key.
#[cfg_attr(not(feature = "integration_tests"), ignore)]
#[tokio::test]
async fn record_is_append_only() {
    let client = test_client().await;
    let (sk1, pk1) = fresh_keypair();
    let nonce = fresh_nonce();
    let name = client.register(&sk1, &nonce).await.expect("register");

    let (sk2, pk2) = fresh_keypair();
    client.rotate(&sk1, &name, &sk2, 0).await.expect("rotate 1");
    let (sk3, pk3) = fresh_keypair();
    client.rotate(&sk2, &name, &sk3, 1).await.expect("rotate 2");

    let history = client.resolve_history(&name).await.expect("history");
    assert_eq!(history.len(), 3);

    // All historical keys remain readable and in order, with the ops and
    // seqs that produced them. Each entry carries a non-zero block height,
    // and later entries record at a height no earlier than earlier ones.
    assert_eq!(history[0].pk, uncompressed_pk_bytes(&pk1));
    assert_eq!(history[0].op, "register");
    assert_eq!(history[0].seq, 0);
    assert_eq!(history[1].pk, uncompressed_pk_bytes(&pk2));
    assert_eq!(history[1].op, "rotate");
    assert_eq!(history[1].seq, 1);
    assert_eq!(history[2].pk, uncompressed_pk_bytes(&pk3));
    assert_eq!(history[2].op, "rotate");
    assert_eq!(history[2].seq, 2);
    assert!(history[0].block_height > 0);
    assert!(history[1].block_height >= history[0].block_height);
    assert!(history[2].block_height >= history[1].block_height);
}

// --- REG-AC9 -----------------------------------------------------------------

/// A quorum of genesis admins can relink an identity, also with offline
/// admin_sign signatures. History records the evidence and the signers. A
/// stranger's signature does not count toward the quorum, also under an
/// admin pk.
#[cfg_attr(not(feature = "integration_tests"), ignore)]
#[tokio::test]
async fn admin_relink_works_and_is_bounded() {
    let client = test_client().await;
    let (sk1, _pk1) = fresh_keypair();
    let nonce = fresh_nonce();
    let name = client.register(&sk1, &nonce).await.expect("register");

    // Two admin keys meet the 2-of-3 quorum (REG-D3).
    let admins = [admin_sk(0), admin_sk(1)];
    let (sk_new, pk_new) = fresh_keypair();
    let evidence = b"out_of_band_proof".to_vec();
    let new_seq = client
        .admin_relink(&admins, &name, &sk_new, 0, &evidence)
        .await
        .expect("admin relink");
    assert_eq!(new_seq, 1);

    let history = client.resolve_history(&name).await.expect("history");
    assert_eq!(history.len(), 2);
    assert_eq!(history[1].op, "admin_relink");
    assert_eq!(history[1].pk, uncompressed_pk_bytes(&pk_new));
    assert_eq!(history[1].admin_evidence.as_deref(), Some(&evidence[..]));
    let mut signers = history[1].admin_signers.clone().expect("admin signers");
    signers.sort();
    let mut expected: Vec<_> = admins.iter().map(admin_pk).collect();
    expected.sort();
    assert_eq!(signers, expected);

    // Offline path: each admin signs with admin_sign, and the deploy carries
    // only the (pk, sig) pairs.
    let (sk_newer, pk_newer) = fresh_keypair();
    let pairs: Vec<_> = admins
        .iter()
        .map(|sk| {
            admin_sign(
                sk,
                &name,
                &uncompressed_pk_bytes(&pk_newer),
                1,
                b"evidence2",
            )
        })
        .collect();
    let new_seq = client
        .admin_relink_with_sigs(&pairs, &name, &sk_newer, 1, b"evidence2")
        .await
        .expect("admin relink with offline signatures");
    assert_eq!(new_seq, 2);

    // One admin plus a stranger's key, and one admin plus a pair that claims
    // the other admin pk but carries a stranger's signature: both rejected.
    let (sk_stranger, _) = fresh_keypair();
    let (sk_newest, pk_newest) = fresh_keypair();
    let pk_newest_bytes = uncompressed_pk_bytes(&pk_newest);
    let admin_pair = admin_sign(&admins[0], &name, &pk_newest_bytes, 2, b"evidence3");
    let (_, stranger_sig) = admin_sign(&sk_stranger, &name, &pk_newest_bytes, 2, b"evidence3");
    for (label, result) in [
        (
            "admin and stranger key",
            client
                .admin_relink(
                    &[admins[0], sk_stranger],
                    &name,
                    &sk_newest,
                    2,
                    b"evidence3",
                )
                .await,
        ),
        (
            "admin and admin pk with stranger sig",
            client
                .admin_relink_with_sigs(
                    &[admin_pair, (admin_pk(&admins[1]), stranger_sig)],
                    &name,
                    &sk_newest,
                    2,
                    b"evidence3",
                )
                .await,
        ),
    ] {
        assert!(
            matches!(result, Err(IdentityRegistryError::Rejected(ref r)) if r == "bad_signature"),
            "expected bad_signature for {label}, got {result:?}"
        );
    }
}

/// Fewer than quorum distinct admin signatures relink nothing. A repeated
/// admin counts once.
#[cfg_attr(not(feature = "integration_tests"), ignore)]
#[tokio::test]
async fn admin_relink_below_quorum_is_rejected() {
    let client = test_client().await;
    let (sk1, pk1) = fresh_keypair();
    let nonce = fresh_nonce();
    let name = client.register(&sk1, &nonce).await.expect("register");

    let admin = admin_sk(0);
    let (sk_new, _) = fresh_keypair();
    for (label, admin_sks) in [
        ("one admin", vec![admin]),
        ("one admin twice", vec![admin, admin]),
    ] {
        let result = client
            .admin_relink(&admin_sks, &name, &sk_new, 0, b"evidence")
            .await;
        assert!(
            matches!(result, Err(IdentityRegistryError::Rejected(ref r)) if r == "bad_signature"),
            "expected bad_signature for {label}, got {result:?}"
        );
    }

    let resolved = client.resolve_current(&name).await.expect("resolve");
    assert_eq!(resolved.pk, uncompressed_pk_bytes(&pk1));
    assert_eq!(resolved.seq, 0);
}
