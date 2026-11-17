// secp256k1 signing and verification over the payload digest. Matches the
// scheme that casper/src/main/resources/IdentityRegistry.rhox uses on-chain via
// rho:crypto:secp256k1Verify.

use secp256k1::ecdsa::Signature;
use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};

use crate::payload::{admin_relink_payload, blake2b256, NAME_LEN, PK_LEN};

/// Signs `blake2b256(payload)` under `sk` and returns the ECDSA signature.
pub fn sign_payload(payload: &[u8], sk: &SecretKey) -> Signature {
    let digest = blake2b256(payload);
    let message = Message::from_digest(digest);
    Secp256k1::new().sign_ecdsa(message, sk)
}

/// Verifies that `sig` is a valid signature over `blake2b256(payload)` under
/// `pk`. Used by off-chain tests. The on-chain contract performs its own
/// verification via `rho:crypto:secp256k1Verify`.
pub fn verify_payload(
    payload: &[u8],
    sig: &Signature,
    pk: &PublicKey,
) -> Result<(), secp256k1::Error> {
    let digest = blake2b256(payload);
    let message = Message::from_digest(digest);
    Secp256k1::new().verify_ecdsa(message, sig, pk)
}

/// Serialises a public key into its 65-byte uncompressed form, matching the
/// shape the Rholang contract expects on the wire.
pub fn uncompressed_pk_bytes(pk: &PublicKey) -> [u8; 65] { pk.serialize_uncompressed() }

/// Serialises a signature in DER form, matching the format
/// `rho:crypto:secp256k1Verify` consumes.
pub fn signature_der(sig: &Signature) -> Vec<u8> { sig.serialize_der().to_vec() }

/// Signs an admin_relink payload with one admin key. Returns the
/// (uncompressed pk, DER sig) pair that admin_relink carries per admin.
pub fn admin_sign(
    admin_sk: &SecretKey,
    name: &[u8; NAME_LEN],
    new_pk: &[u8; PK_LEN],
    seq: u64,
    admin_evidence: &[u8],
) -> ([u8; PK_LEN], Vec<u8>) {
    let payload = admin_relink_payload(name, new_pk, seq, admin_evidence);
    let pk = uncompressed_pk_bytes(&PublicKey::from_secret_key(&Secp256k1::new(), admin_sk));
    (pk, signature_der(&sign_payload(&payload, admin_sk)))
}
