// See casper/src/test/scala/coop/rchain/casper/genesis/contracts/StandardDeploysSpec.scala

use casper::rust::genesis::contracts::{fs_genesis, standard_deploys};
use crypto::rust::hash::blake2b256::Blake2b256;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use models::rust::utils::{new_etuple_par, new_gint_par};
use prost::Message;

#[test]
fn should_print_public_keys_used_for_signing_standard_blessed_contracts() {
    println!("Public keys used to sign standard (blessed) contracts");
    println!("=====================================================");

    for (idx, pub_key) in standard_deploys::system_public_keys().iter().enumerate() {
        println!("{}. {}", idx + 1, hex::encode(&pub_key.bytes));
    }
}

/// Fast parse/normalize check on the new VersionedRegistry.rho embedded
/// constant. Runs the same compile path the genesis loader uses so a typo
/// in the new resource fails here before the slower RhoSpec deploy test.
#[test]
fn versioned_registry_embedded_source_compiles() {
    // `standard_deploys::versioned_registry` internally calls
    // `embedded_source(..., embedded_rho::VERSIONED_REGISTRY)`, which
    // invokes `CompiledRholangSource::new` and panics on a parse/normalize
    // error. A clean return here is the check.
    let _ = standard_deploys::versioned_registry("root");
}

/// Byte-anchor the fs_generator deploy's signed-registry signature.
/// The secp256k1 signature derived from (FS_GENERATOR_PK,
/// FS_GENERATOR_TIMESTAMP, FS_NONCE) must equal a hardcoded value.
/// If a future refactor swaps the signer to a randomized-k
/// implementation (or FS_GENERATOR_PK / FS_GENERATOR_TIMESTAMP /
/// FS_NONCE drifts), this test fails LOCALLY instead of silently
/// splitting the network at runtime.
///
/// Regenerate the pin via `println!("{sig_hex}")` and update
/// fs_generator_expected_sig.txt — treat the change as a Genesis
/// hard-fork event.
#[test]
fn fs_generator_signature_matches_hardcoded_expected() {
    let sk =
        PrivateKey::from_bytes(&hex::decode(standard_deploys::FS_GENERATOR_PK).expect("valid hex"));
    let sig_hex =
        fs_genesis::fs_genesis_signature_hex(&sk, standard_deploys::FS_GENERATOR_TIMESTAMP);
    const EXPECTED_SIG_HEX: &str = include_str!("fs_generator_expected_sig.txt");
    let expected = EXPECTED_SIG_HEX.trim();
    assert_eq!(
        sig_hex.as_str(),
        expected,
        "signature drift detected — validators would diverge in production.  \
         Regenerate via `println!(\"{{sig_hex}}\")` and update \
         fs_generator_expected_sig.txt only when the signer semantics or \
         FS_GENERATOR_PK / _TIMESTAMP intentionally changed."
    );
}

/// Verify the derived signature actually validates against the
/// public key.  Defense-in-depth against the "syntactically valid
/// but cryptographically invalid" failure mode: the hardcoded-sig
/// pin above catches semantic drift (signer swap, PK change),
/// but a signer that silently produces bytes that happen to be
/// 142 hex chars without being a real signature would pass that
/// check and fail at registry verification time.  This test
/// cryptographically verifies the sig against FS_GENERATOR_PUB_KEY
/// over the exact to_sign preimage that fs_genesis_signature_hex
/// uses, catching that class of regression locally.
#[test]
fn fs_generator_signature_verifies_against_pubkey() {
    let sk =
        PrivateKey::from_bytes(&hex::decode(standard_deploys::FS_GENERATOR_PK).expect("valid hex"));
    let sig_hex =
        fs_genesis::fs_genesis_signature_hex(&sk, standard_deploys::FS_GENERATOR_TIMESTAMP);
    let sig = hex::decode(&sig_hex).expect("valid hex sig");
    let pk = &*standard_deploys::FS_GENERATOR_PUB_KEY;
    // Reconstruct the to_sign tuple exactly as fs_genesis_signature_hex
    // does: etuple(timestamp, GByteArray(pk.bytes), FS_NONCE).
    let to_sign: Par = new_etuple_par(vec![
        new_gint_par(standard_deploys::FS_GENERATOR_TIMESTAMP, Vec::new(), false),
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GByteArray(pk.bytes.to_vec())),
        }]),
        new_gint_par(fs_genesis::FS_NONCE, Vec::new(), false),
    ]);
    let sign_bytes = Blake2b256::hash(to_sign.encode_to_vec());
    let ok = Secp256k1.verify(&sign_bytes, &sig, &pk.bytes);
    assert!(
        ok,
        "fs_generator signature must verify against FS_GENERATOR_PUB_KEY.  \
         A failing verify indicates the signer produced bytes that are \
         syntactically valid hex but cryptographically invalid — the \
         composed-source `rs!(..., sig_hex, ...)` call would silently \
         reject at registry verification time (deploy succeeds without \
         publishing the Fs cap)."
    );
}
