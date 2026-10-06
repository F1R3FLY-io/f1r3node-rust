// See casper/src/test/scala/coop/rchain/casper/genesis/contracts/StandardDeploysSpec.scala

use casper::rust::genesis::contracts::{fs_genesis, standard_deploys};
use crypto::rust::private_key::PrivateKey;

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
