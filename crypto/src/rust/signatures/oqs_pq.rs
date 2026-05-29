//! Off-by-default post-quantum signature backends via the Open Quantum Safe
//! `oqs` crate (liboqs, built from C through cmake when the `vendored`
//! feature is active).
//!
//! This module realizes the cost-accounted rho-calculus paper's §4.5
//! "genericity over the signature group G" claim: the consensus and deploy
//! pipelines are agnostic to the concrete signature scheme behind the
//! [`SignaturesAlg`] trait object, so a NIST-standardized post-quantum scheme
//! plugs in with no change to the verification surface beyond name
//! registration.
//!
//! Three NIST/FIPS parameter sets are exposed, one zero-sized struct each:
//!   * [`MlDsa65`]          — FIPS 204 ML-DSA-65 (lattice, recommended default)
//!   * [`Falcon512`]        — FALCON-512 (lattice, compact, recommended default)
//!   * [`SlhDsaSha2_128s`]  — FIPS 205 SLH-DSA-SHA2-128s (hash-based,
//!                            operator-gated — large signatures, ~7.8 KiB)
//!
//! ## Consensus determinism
//!
//! Each scheme's [`SignaturesAlg::name`] returns a VERSION-PINNED canonical
//! string (`"oqs-ml-dsa-65/v1"`, `"oqs-falcon-512/v1"`,
//! `"oqs-slh-dsa-sha2-128s/v1"`). Two nodes that both link a conforming
//! liboqs agree byte-for-byte on the algorithm identifier carried in the
//! deploy/block envelope, on the domain-separated signing preimage, and on
//! the verification predicate. The version suffix lets a future,
//! wire-incompatible parameterization (e.g. a different domain-separation
//! scheme) coexist as `/v2` without ambiguity. See
//! [`assert_oqs_algorithms_available`] for the startup determinism guard.
//!
//! ## Error handling
//!
//! The [`SignaturesAlg`] trait is infallible at its boundary (`verify`
//! returns `bool`; `sign`/`to_public`/`new_key_pair` return values, not
//! `Result`). We therefore map every liboqs error or malformed input to the
//! trait's "failure" value — `false` for `verify`, an empty `Vec` for
//! `sign`, and an empty key for `to_public` — exactly as the `ed25519`,
//! `secp256k1`, and `schnorr-secp256k1` backends do.

use crate::rust::hash::blake2b256::Blake2b256;
use crate::rust::private_key::PrivateKey;
use crate::rust::public_key::PublicKey;

use oqs::sig::{Algorithm, Sig};

use super::signatures_alg::SignaturesAlg;

// ---------------------------------------------------------------------------
// Canonical, version-pinned algorithm names (consensus-critical identifiers).
// ---------------------------------------------------------------------------

/// Canonical wire name for FIPS 204 ML-DSA-65.
pub const OQS_ML_DSA_65_ALGORITHM_NAME: &str = "oqs-ml-dsa-65/v1";
/// Canonical wire name for FALCON-512.
pub const OQS_FALCON_512_ALGORITHM_NAME: &str = "oqs-falcon-512/v1";
/// Canonical wire name for FIPS 205 SLH-DSA-SHA2-128s.
pub const OQS_SLH_DSA_SHA2_128S_ALGORITHM_NAME: &str = "oqs-slh-dsa-sha2-128s/v1";

// ---------------------------------------------------------------------------
// Per-scheme domain-separation constants (mirror schnorr's SCHNORR_*_DOMAIN).
// The version suffix in the domain string is tied to the `/v1` name suffix.
// ---------------------------------------------------------------------------

/// Domain separator folded into the ML-DSA-65 signing preimage.
pub const OQS_ML_DSA_65_SIGNING_DOMAIN: &[u8] = b"f1r3node/oqs-ml-dsa-65/signing/v1";
/// Domain separator folded into the FALCON-512 signing preimage.
pub const OQS_FALCON_512_SIGNING_DOMAIN: &[u8] = b"f1r3node/oqs-falcon-512/signing/v1";
/// Domain separator folded into the SLH-DSA-SHA2-128s signing preimage.
pub const OQS_SLH_DSA_SHA2_128S_SIGNING_DOMAIN: &[u8] =
    b"f1r3node/oqs-slh-dsa-sha2-128s/signing/v1";

/// Length, in bytes, of the Blake2b256 prehash that all OQS backends sign and
/// verify. Mirrors the 32-byte prehash convention of the secp256k1/schnorr
/// backends so that [`crate::rust::signatures::signed::Signed::signature_hash`]
/// can feed every algorithm a uniform-width digest.
const OQS_PREHASH_LEN: usize = 32;

/// Build the domain-separated signing preimage for an OQS scheme:
/// `domain || be64(len(payload)) || payload`. Identical in shape to
/// [`crate::rust::signatures::schnorr_secp256k1::SchnorrSecp256k1::signing_preimage`].
fn oqs_signing_preimage(domain: &[u8], serialized_payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(domain.len() + 8 + serialized_payload.len());
    out.extend_from_slice(domain);
    out.extend_from_slice(&(serialized_payload.len() as u64).to_be_bytes());
    out.extend_from_slice(serialized_payload);
    out
}

/// Blake2b256 over the domain-separated preimage for an OQS scheme.
fn oqs_domain_separated_hash(domain: &[u8], serialized_payload: &[u8]) -> Vec<u8> {
    Blake2b256::hash(oqs_signing_preimage(domain, serialized_payload))
}

/// Construct a liboqs `Sig` handle for `algorithm`, after ensuring liboqs is
/// initialized. Returns `None` if the linked liboqs was built without the
/// requested algorithm (mirrors the trait's infallible failure mode).
///
/// `oqs::init()` is idempotent and thread-safe under the crate's `std`
/// feature, so calling it per-operation is correct and cheap; it guarantees
/// the deterministic-RNG / constant-time tables liboqs needs are set up
/// before any keygen/sign/verify call on any thread.
fn sig_handle(algorithm: Algorithm) -> Option<Sig> {
    oqs::init();
    Sig::new(algorithm).ok()
}

/// Shared `verify` implementation: parse the public key and signature as
/// liboqs byte refs, then run liboqs verification over the 32-byte prehash.
/// Any malformed length or liboqs error collapses to `false`.
fn oqs_verify(algorithm: Algorithm, scheme_name: &str, data: &[u8], signature: &[u8], pub_key: &[u8]) -> bool {
    if data.len() != OQS_PREHASH_LEN {
        tracing::warn!(
            "{}.verify: expected {}-byte prehash, got {} bytes",
            scheme_name,
            OQS_PREHASH_LEN,
            data.len()
        );
        return false;
    }
    let Some(sig) = sig_handle(algorithm) else {
        tracing::warn!("{}.verify: liboqs does not provide this algorithm", scheme_name);
        return false;
    };
    let Some(pk_ref) = sig.public_key_from_bytes(pub_key) else {
        return false;
    };
    let Some(sig_ref) = sig.signature_from_bytes(signature) else {
        return false;
    };
    sig.verify(data, sig_ref, pk_ref).is_ok()
}

/// Shared `sign` implementation: parse the secret key as a liboqs byte ref and
/// produce a detached signature over the 32-byte prehash. Any malformed length
/// or liboqs error collapses to an empty `Vec`.
fn oqs_sign(algorithm: Algorithm, scheme_name: &str, data: &[u8], sec: &[u8]) -> Vec<u8> {
    if data.len() != OQS_PREHASH_LEN {
        tracing::warn!(
            "{}.sign: expected {}-byte prehash, got {} bytes",
            scheme_name,
            OQS_PREHASH_LEN,
            data.len()
        );
        return Vec::new();
    }
    let Some(sig) = sig_handle(algorithm) else {
        tracing::warn!("{}.sign: liboqs does not provide this algorithm", scheme_name);
        return Vec::new();
    };
    let Some(sk_ref) = sig.secret_key_from_bytes(sec) else {
        tracing::warn!("{}.sign: malformed secret key", scheme_name);
        return Vec::new();
    };
    match sig.sign(data, sk_ref) {
        Ok(signature) => signature.into_vec(),
        Err(_) => Vec::new(),
    }
}

/// Shared `to_public` implementation. liboqs derives public keys only at
/// keypair-generation time (the secret key does not carry a recoverable public
/// component in a stable API), so a raw secret-key-to-public derivation is not
/// available; we therefore return an empty key, mirroring how the other
/// backends degrade on an input they cannot honor. Callers obtain matched key
/// pairs through [`SignaturesAlg::new_key_pair`].
fn oqs_to_public(_algorithm: Algorithm, _sec: &PrivateKey) -> PublicKey {
    PublicKey::from_bytes(&[])
}

/// Shared `new_key_pair` implementation. Returns empty keys if liboqs lacks the
/// algorithm or keygen fails (infallible-trait failure mode).
fn oqs_new_key_pair(algorithm: Algorithm, scheme_name: &str) -> (PrivateKey, PublicKey) {
    let Some(sig) = sig_handle(algorithm) else {
        tracing::warn!(
            "{}.new_key_pair: liboqs does not provide this algorithm",
            scheme_name
        );
        return (PrivateKey::from_bytes(&[]), PublicKey::from_bytes(&[]));
    };
    match sig.keypair() {
        Ok((pk, sk)) => (
            PrivateKey::from_bytes(sk.as_ref()),
            PublicKey::from_bytes(pk.as_ref()),
        ),
        Err(_) => (PrivateKey::from_bytes(&[]), PublicKey::from_bytes(&[])),
    }
}

/// Shared `sig_length` implementation. For fixed-length schemes (ML-DSA,
/// SLH-DSA) this is exact; for FALCON the value is liboqs' MAXIMUM signature
/// length and is therefore ADVISORY — FALCON signatures are variable-length
/// and a given signature may be shorter. Returns 0 if liboqs lacks the
/// algorithm.
fn oqs_sig_length(algorithm: Algorithm) -> usize {
    sig_handle(algorithm)
        .map(|s| s.length_signature())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// FIPS 204 ML-DSA-65 (recommended default).
// ---------------------------------------------------------------------------

/// FIPS 204 ML-DSA-65 signature backend. Recommended post-quantum default
/// alongside [`Falcon512`].
#[derive(Clone, Debug, PartialEq)]
pub struct MlDsa65;

impl MlDsa65 {
    pub const ALGORITHM: Algorithm = Algorithm::MlDsa65;

    pub fn name() -> String { OQS_ML_DSA_65_ALGORITHM_NAME.to_string() }

    pub fn signing_preimage(serialized_payload: &[u8]) -> Vec<u8> {
        oqs_signing_preimage(OQS_ML_DSA_65_SIGNING_DOMAIN, serialized_payload)
    }

    pub fn domain_separated_hash(serialized_payload: &[u8]) -> Vec<u8> {
        oqs_domain_separated_hash(OQS_ML_DSA_65_SIGNING_DOMAIN, serialized_payload)
    }
}

impl SignaturesAlg for MlDsa65 {
    fn verify(&self, data: &[u8], signature: &[u8], pub_key: &[u8]) -> bool {
        oqs_verify(Self::ALGORITHM, OQS_ML_DSA_65_ALGORITHM_NAME, data, signature, pub_key)
    }

    fn sign(&self, data: &[u8], sec: &[u8]) -> Vec<u8> {
        oqs_sign(Self::ALGORITHM, OQS_ML_DSA_65_ALGORITHM_NAME, data, sec)
    }

    fn to_public(&self, sec: &PrivateKey) -> PublicKey { oqs_to_public(Self::ALGORITHM, sec) }

    fn new_key_pair(&self) -> (PrivateKey, PublicKey) {
        oqs_new_key_pair(Self::ALGORITHM, OQS_ML_DSA_65_ALGORITHM_NAME)
    }

    fn name(&self) -> String { Self::name() }

    fn sig_length(&self) -> usize { oqs_sig_length(Self::ALGORITHM) }

    fn eq(&self, other: &dyn SignaturesAlg) -> bool { self.name() == other.name() }

    fn box_clone(&self) -> Box<dyn SignaturesAlg> { Box::new(self.clone()) }
}

// ---------------------------------------------------------------------------
// FALCON-512 (recommended default; compact, variable-length signatures).
// ---------------------------------------------------------------------------

/// FALCON-512 signature backend. Recommended post-quantum default alongside
/// [`MlDsa65`]. Note FALCON signatures are variable-length; [`Self::sig_length`]
/// is advisory (the liboqs maximum).
#[derive(Clone, Debug, PartialEq)]
pub struct Falcon512;

impl Falcon512 {
    pub const ALGORITHM: Algorithm = Algorithm::Falcon512;

    pub fn name() -> String { OQS_FALCON_512_ALGORITHM_NAME.to_string() }

    pub fn signing_preimage(serialized_payload: &[u8]) -> Vec<u8> {
        oqs_signing_preimage(OQS_FALCON_512_SIGNING_DOMAIN, serialized_payload)
    }

    pub fn domain_separated_hash(serialized_payload: &[u8]) -> Vec<u8> {
        oqs_domain_separated_hash(OQS_FALCON_512_SIGNING_DOMAIN, serialized_payload)
    }
}

impl SignaturesAlg for Falcon512 {
    fn verify(&self, data: &[u8], signature: &[u8], pub_key: &[u8]) -> bool {
        oqs_verify(Self::ALGORITHM, OQS_FALCON_512_ALGORITHM_NAME, data, signature, pub_key)
    }

    fn sign(&self, data: &[u8], sec: &[u8]) -> Vec<u8> {
        oqs_sign(Self::ALGORITHM, OQS_FALCON_512_ALGORITHM_NAME, data, sec)
    }

    fn to_public(&self, sec: &PrivateKey) -> PublicKey { oqs_to_public(Self::ALGORITHM, sec) }

    fn new_key_pair(&self) -> (PrivateKey, PublicKey) {
        oqs_new_key_pair(Self::ALGORITHM, OQS_FALCON_512_ALGORITHM_NAME)
    }

    fn name(&self) -> String { Self::name() }

    /// Advisory: liboqs MAXIMUM FALCON-512 signature length. FALCON signatures
    /// are variable-length, so an actual signature may be shorter.
    fn sig_length(&self) -> usize { oqs_sig_length(Self::ALGORITHM) }

    fn eq(&self, other: &dyn SignaturesAlg) -> bool { self.name() == other.name() }

    fn box_clone(&self) -> Box<dyn SignaturesAlg> { Box::new(self.clone()) }
}

// ---------------------------------------------------------------------------
// FIPS 205 SLH-DSA-SHA2-128s (operator-gated; large signatures).
// In liboqs 0.11 this parameter set is exposed as the SPHINCS+-SHA2-128s-simple
// algorithm, which is the pre-standardization name for FIPS 205 SLH-DSA-SHA2-128s.
// ---------------------------------------------------------------------------

/// FIPS 205 SLH-DSA-SHA2-128s signature backend. Hash-based and conservative,
/// but with large (~7.8 KiB) signatures, so it is OPERATOR-GATED rather than a
/// default. In liboqs 0.11 the underlying algorithm enum variant is
/// `SphincsSha2128sSimple` (the pre-FIPS-205 SPHINCS+ name for the same
/// parameter set).
#[derive(Clone, Debug, PartialEq)]
pub struct SlhDsaSha2_128s;

impl SlhDsaSha2_128s {
    pub const ALGORITHM: Algorithm = Algorithm::SphincsSha2128sSimple;

    pub fn name() -> String { OQS_SLH_DSA_SHA2_128S_ALGORITHM_NAME.to_string() }

    pub fn signing_preimage(serialized_payload: &[u8]) -> Vec<u8> {
        oqs_signing_preimage(OQS_SLH_DSA_SHA2_128S_SIGNING_DOMAIN, serialized_payload)
    }

    pub fn domain_separated_hash(serialized_payload: &[u8]) -> Vec<u8> {
        oqs_domain_separated_hash(OQS_SLH_DSA_SHA2_128S_SIGNING_DOMAIN, serialized_payload)
    }
}

impl SignaturesAlg for SlhDsaSha2_128s {
    fn verify(&self, data: &[u8], signature: &[u8], pub_key: &[u8]) -> bool {
        oqs_verify(
            Self::ALGORITHM,
            OQS_SLH_DSA_SHA2_128S_ALGORITHM_NAME,
            data,
            signature,
            pub_key,
        )
    }

    fn sign(&self, data: &[u8], sec: &[u8]) -> Vec<u8> {
        oqs_sign(Self::ALGORITHM, OQS_SLH_DSA_SHA2_128S_ALGORITHM_NAME, data, sec)
    }

    fn to_public(&self, sec: &PrivateKey) -> PublicKey { oqs_to_public(Self::ALGORITHM, sec) }

    fn new_key_pair(&self) -> (PrivateKey, PublicKey) {
        oqs_new_key_pair(Self::ALGORITHM, OQS_SLH_DSA_SHA2_128S_ALGORITHM_NAME)
    }

    fn name(&self) -> String { Self::name() }

    fn sig_length(&self) -> usize { oqs_sig_length(Self::ALGORITHM) }

    fn eq(&self, other: &dyn SignaturesAlg) -> bool { self.name() == other.name() }

    fn box_clone(&self) -> Box<dyn SignaturesAlg> { Box::new(self.clone()) }
}

// ---------------------------------------------------------------------------
// Registry surface + startup determinism guard.
// ---------------------------------------------------------------------------

/// All OQS canonical algorithm names registered by this module, paired with
/// the liboqs `Algorithm` each one resolves to. Used by the registry-parity
/// test and by [`assert_oqs_algorithms_available`].
pub const OQS_REGISTERED_ALGORITHMS: [(&str, Algorithm); 3] = [
    (OQS_ML_DSA_65_ALGORITHM_NAME, Algorithm::MlDsa65),
    (OQS_FALCON_512_ALGORITHM_NAME, Algorithm::Falcon512),
    (
        OQS_SLH_DSA_SHA2_128S_ALGORITHM_NAME,
        Algorithm::SphincsSha2128sSimple,
    ),
];

/// `true` iff `name` is one of this module's registered OQS algorithm names.
/// Replicated by the separate block-signature registry in
/// `casper::rust::validate`; the crypto-side registry-parity test asserts the
/// two never drift.
pub fn is_registered_oqs_name(name: &str) -> bool {
    OQS_REGISTERED_ALGORITHMS
        .iter()
        .any(|(registered, _)| *registered == name)
}

/// Consensus-determinism startup guard. Asserts that the liboqs the node has
/// linked actually provides every OQS algorithm this build registers. If a
/// node links a liboqs compiled without (say) FALCON-512, it would otherwise
/// silently treat every FALCON-512 signature as invalid — a consensus fork
/// hazard. Call this once at startup (after enabling the feature) so the node
/// fails fast and loudly instead.
///
/// Returns `Ok(())` when all registered algorithms are available, or
/// `Err(message)` naming the first missing algorithm.
pub fn assert_oqs_algorithms_available() -> Result<(), String> {
    oqs::init();
    for (name, algorithm) in OQS_REGISTERED_ALGORITHMS.iter() {
        if Sig::new(*algorithm).is_err() {
            return Err(format!(
                "linked liboqs does not provide algorithm {} (canonical name {}); \
                 refusing to start to avoid a consensus-determinism fork",
                algorithm, name
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schemes() -> Vec<(Box<dyn SignaturesAlg>, &'static str)> {
        vec![
            (Box::new(MlDsa65), OQS_ML_DSA_65_ALGORITHM_NAME),
            (Box::new(Falcon512), OQS_FALCON_512_ALGORITHM_NAME),
            (Box::new(SlhDsaSha2_128s), OQS_SLH_DSA_SHA2_128S_ALGORITHM_NAME),
        ]
    }

    #[test]
    fn startup_guard_reports_all_algorithms_available() {
        assert_oqs_algorithms_available()
            .expect("liboqs must provide all registered OQS algorithms");
    }

    #[test]
    fn names_are_version_pinned() {
        assert_eq!(MlDsa65::name(), "oqs-ml-dsa-65/v1");
        assert_eq!(Falcon512::name(), "oqs-falcon-512/v1");
        assert_eq!(SlhDsaSha2_128s::name(), "oqs-slh-dsa-sha2-128s/v1");
    }

    #[test]
    fn keygen_sign_verify_roundtrip_each_scheme() {
        for (alg, name) in schemes() {
            let (sk, pk) = alg.new_key_pair();
            assert!(!sk.bytes.is_empty(), "{name}: secret key must be non-empty");
            assert!(!pk.bytes.is_empty(), "{name}: public key must be non-empty");
            let msg = Blake2b256::hash(b"oqs roundtrip message".to_vec());
            let sig = alg.sign(&msg, &sk.bytes);
            assert!(!sig.is_empty(), "{name}: signature must be non-empty");
            assert!(
                alg.verify(&msg, &sig, &pk.bytes),
                "{name}: fresh signature must verify"
            );
        }
    }

    #[test]
    fn verify_fails_on_corrupted_signature() {
        for (alg, name) in schemes() {
            let (sk, pk) = alg.new_key_pair();
            let msg = Blake2b256::hash(b"corrupt-sig message".to_vec());
            let mut sig = alg.sign(&msg, &sk.bytes);
            assert!(!sig.is_empty());
            // Flip a bit in the middle of the signature body.
            let mid = sig.len() / 2;
            sig[mid] ^= 0x01;
            assert!(
                !alg.verify(&msg, &sig, &pk.bytes),
                "{name}: corrupted signature must not verify"
            );
        }
    }

    #[test]
    fn verify_fails_on_corrupted_message() {
        for (alg, name) in schemes() {
            let (sk, pk) = alg.new_key_pair();
            let msg = Blake2b256::hash(b"original message".to_vec());
            let sig = alg.sign(&msg, &sk.bytes);
            let other = Blake2b256::hash(b"tampered message".to_vec());
            assert!(
                !alg.verify(&other, &sig, &pk.bytes),
                "{name}: signature must not verify against a different prehash"
            );
        }
    }

    #[test]
    fn verify_fails_on_wrong_pubkey() {
        for (alg, name) in schemes() {
            let (sk, _pk) = alg.new_key_pair();
            let (_sk2, wrong_pk) = alg.new_key_pair();
            let msg = Blake2b256::hash(b"wrong-pubkey message".to_vec());
            let sig = alg.sign(&msg, &sk.bytes);
            assert!(
                !alg.verify(&msg, &sig, &wrong_pk.bytes),
                "{name}: signature must not verify under a different public key"
            );
        }
    }

    #[test]
    fn verify_rejects_non_32_byte_prehash() {
        for (alg, name) in schemes() {
            let (sk, pk) = alg.new_key_pair();
            // Sign a proper 32-byte prehash...
            let msg = Blake2b256::hash(b"length-guard".to_vec());
            let sig = alg.sign(&msg, &sk.bytes);
            // ...then present a malformed 31-byte "prehash" to verify.
            let short = vec![0u8; 31];
            assert!(
                !alg.verify(&short, &sig, &pk.bytes),
                "{name}: verify must reject a non-32-byte prehash"
            );
            // And sign must refuse a malformed prehash (empty result).
            assert!(
                alg.sign(&short, &sk.bytes).is_empty(),
                "{name}: sign must reject a non-32-byte prehash"
            );
        }
    }

    #[test]
    fn verify_rejects_malformed_key_and_signature_lengths() {
        for (alg, name) in schemes() {
            let msg = [0u8; 32];
            let bad_sig = vec![0u8; 7];
            let bad_pk = vec![0u8; 5];
            assert!(
                !alg.verify(&msg, &bad_sig, &bad_pk),
                "{name}: malformed key/sig lengths must not verify"
            );
        }
    }

    #[test]
    fn domain_separation_changes_the_prehash_per_scheme() {
        let payload = b"identical serialized payload";
        let h_mldsa = MlDsa65::domain_separated_hash(payload);
        let h_falcon = Falcon512::domain_separated_hash(payload);
        let h_slh = SlhDsaSha2_128s::domain_separated_hash(payload);
        let h_plain = Blake2b256::hash(payload.to_vec());

        // Each scheme's domain-separated hash differs from the plain hash and
        // from every other scheme's hash, even over identical payload bytes.
        assert_ne!(h_mldsa, h_plain);
        assert_ne!(h_falcon, h_plain);
        assert_ne!(h_slh, h_plain);
        assert_ne!(h_mldsa, h_falcon);
        assert_ne!(h_mldsa, h_slh);
        assert_ne!(h_falcon, h_slh);
    }

    #[test]
    fn domain_separated_signature_does_not_verify_under_plain_hash() {
        // A signature produced over the ML-DSA domain-separated hash must not
        // verify when the verifier hands liboqs the plain (non-separated) hash.
        let alg = MlDsa65;
        let (sk, pk) = alg.new_key_pair();
        let payload = b"domain separation matters";
        let separated = MlDsa65::domain_separated_hash(payload);
        let plain = Blake2b256::hash(payload.to_vec());
        let sig = alg.sign(&separated, &sk.bytes);
        assert!(alg.verify(&separated, &sig, &pk.bytes));
        assert!(!alg.verify(&plain, &sig, &pk.bytes));
    }

    #[test]
    fn sig_length_is_reported_for_each_scheme() {
        // ML-DSA-65 and SLH-DSA-SHA2-128s are fixed length; FALCON-512 is the
        // advisory maximum. All must be positive when liboqs provides them.
        assert!(MlDsa65.sig_length() > 0);
        assert!(Falcon512.sig_length() > 0);
        assert!(SlhDsaSha2_128s.sig_length() > 0);
    }

    #[test]
    fn actual_signature_length_within_advisory_bound() {
        // The produced signature length must never exceed the advertised
        // (advisory) sig_length(); for FALCON it may be strictly less.
        for (alg, name) in schemes() {
            let (sk, _pk) = alg.new_key_pair();
            let msg = Blake2b256::hash(b"length-bound".to_vec());
            let sig = alg.sign(&msg, &sk.bytes);
            assert!(
                sig.len() <= alg.sig_length(),
                "{name}: actual sig length {} exceeded advisory bound {}",
                sig.len(),
                alg.sig_length()
            );
        }
    }

    #[test]
    fn registered_algorithm_table_matches_names() {
        assert!(is_registered_oqs_name(OQS_ML_DSA_65_ALGORITHM_NAME));
        assert!(is_registered_oqs_name(OQS_FALCON_512_ALGORITHM_NAME));
        assert!(is_registered_oqs_name(OQS_SLH_DSA_SHA2_128S_ALGORITHM_NAME));
        assert!(!is_registered_oqs_name("secp256k1"));
        assert!(!is_registered_oqs_name("oqs-ml-dsa-65")); // missing /v1 suffix
    }

    /// Known-answer style determinism check for ML-DSA-65: a deterministic
    /// (seeded-by-construction) keypair is not exposed by liboqs' safe API, so
    /// instead we assert the structural invariants that a KAT would also
    /// enforce — the public/secret key and signature lengths reported by
    /// liboqs equal the FIPS 204 ML-DSA-65 standard sizes, and a fresh
    /// signature verifies. This pins the parameter set: a wrong liboqs build
    /// (different parameter set behind the same name) would fail these sizes.
    #[test]
    fn ml_dsa_65_standard_sizes() {
        oqs::init();
        let sig = Sig::new(Algorithm::MlDsa65).expect("ML-DSA-65 available");
        // FIPS 204 ML-DSA-65 sizes.
        assert_eq!(sig.length_public_key(), 1952, "ML-DSA-65 public key size");
        assert_eq!(sig.length_secret_key(), 4032, "ML-DSA-65 secret key size");
        assert_eq!(sig.length_signature(), 3309, "ML-DSA-65 signature size");
    }

    /// Parameter-set pin for FALCON-512: liboqs' reported key sizes are fixed
    /// even though the signature is variable-length. A wrong liboqs build would
    /// fail these sizes. (Signature length is the variable maximum, asserted
    /// only as an upper bound elsewhere.)
    #[test]
    fn falcon_512_standard_key_sizes() {
        oqs::init();
        let sig = Sig::new(Algorithm::Falcon512).expect("FALCON-512 available");
        assert_eq!(sig.length_public_key(), 897, "FALCON-512 public key size");
        assert_eq!(sig.length_secret_key(), 1281, "FALCON-512 secret key size");
        // Maximum (advisory) signature length for FALCON-512 in liboqs.
        assert_eq!(sig.length_signature(), 752, "FALCON-512 max signature size");
    }
}
