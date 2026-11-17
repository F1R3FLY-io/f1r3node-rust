// Canonical byte builders mirroring the layouts enforced by
// casper/src/main/resources/IdentityRegistry.rhox. Any change here must be matched in the
// contract and vice versa.

use blake2::{Blake2b, Digest};
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use prost::Message;
use typenum::U32;

/// Op tag that prefixes a register payload.
pub const TAG_REGISTER: u8 = 0x01;
/// Op tag that prefixes a rotate payload.
pub const TAG_ROTATE: u8 = 0x02;
/// Op tag that prefixes an admin_relink payload.
pub const TAG_ADMIN_RELINK: u8 = 0x03;

/// Length of a secp256k1 uncompressed public key.
pub const PK_LEN: usize = 65;

/// Length of the client-supplied nonce used at registration.
pub const NONCE_LEN: usize = 32;

/// Length of a derived identity name (blake2b256 output).
pub const NAME_LEN: usize = 32;

/// Builds the canonical register payload: `0x01 || pk || nonce`.
pub fn register_payload(pk: &[u8; PK_LEN], nonce: &[u8; NONCE_LEN]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(1 + PK_LEN + NONCE_LEN);
    buf.push(TAG_REGISTER);
    buf.extend_from_slice(pk);
    buf.extend_from_slice(nonce);
    buf
}

/// Builds the canonical rotate payload:
/// `0x02 || name || new_pk || seq_as_par_bytes`.
/// `seq_as_par_bytes` is the Protobuf encoding of `Par { exprs: [GInt(seq)] }`,
/// matching what `Int.toByteArray()` produces on-chain.
pub fn rotate_payload(name: &[u8; NAME_LEN], new_pk: &[u8; PK_LEN], seq: u64) -> Vec<u8> {
    let seq_bytes = int_par_bytes(seq as i64);
    let mut buf = Vec::with_capacity(1 + NAME_LEN + PK_LEN + seq_bytes.len());
    buf.push(TAG_ROTATE);
    buf.extend_from_slice(name);
    buf.extend_from_slice(new_pk);
    buf.extend_from_slice(&seq_bytes);
    buf
}

/// Builds the canonical admin_relink payload:
/// `0x03 || name || new_pk || seq_as_par_bytes || admin_evidence`.
pub fn admin_relink_payload(
    name: &[u8; NAME_LEN],
    new_pk: &[u8; PK_LEN],
    seq: u64,
    admin_evidence: &[u8],
) -> Vec<u8> {
    let seq_bytes = int_par_bytes(seq as i64);
    let mut buf =
        Vec::with_capacity(1 + NAME_LEN + PK_LEN + seq_bytes.len() + admin_evidence.len());
    buf.push(TAG_ADMIN_RELINK);
    buf.extend_from_slice(name);
    buf.extend_from_slice(new_pk);
    buf.extend_from_slice(&seq_bytes);
    buf.extend_from_slice(admin_evidence);
    buf
}

/// Serialises a Rholang `Int` to the bytes that `Int.toByteArray()` produces
/// on-chain. The Rholang method serialises the full `Par` wrapping the Int in
/// canonical Protobuf wire format, so prost's `encode_to_vec` on the matching
/// `Par { exprs: [GInt(n)] }` produces identical bytes.
fn int_par_bytes(n: i64) -> Vec<u8> {
    let par = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(n)),
        }],
        ..Default::default()
    };
    par.encode_to_vec()
}

/// Blake2b-256 digest over `data`.
pub fn blake2b256(data: &[u8]) -> [u8; NAME_LEN] {
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(data);
    let out = hasher.finalize();
    let mut arr = [0u8; NAME_LEN];
    arr.copy_from_slice(&out);
    arr
}

/// Convenience: derive the identity name from `(pk, nonce)`. Equivalent to
/// `blake2b256(register_payload(pk, nonce))`.
pub fn derive_name(pk: &[u8; PK_LEN], nonce: &[u8; NONCE_LEN]) -> [u8; NAME_LEN] {
    blake2b256(&register_payload(pk, nonce))
}
