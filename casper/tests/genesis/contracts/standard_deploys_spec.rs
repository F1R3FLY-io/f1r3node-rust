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
    // Assertion (added slice 19): the blessed-key count is a load-bearing
    // invariant — an accidental removal or duplication of a key would
    // silently shift genesis state.  Bump this on purpose when adding a
    // new blessed contract.
    assert_eq!(
        standard_deploys::system_public_keys().len(),
        14,
        "blessed-key count changed unexpectedly"
    );
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

/// Fast parse/normalize check on the slice-19 FsGenesis composed source.
/// This runs the same compile path as genesis: assembles the library
/// bodies, injects the deterministic signature, and normalizes.  A
/// syntax break in File.rho / Dir.rho / Stream.rho / Buffer.rho /
/// Stdin.rho / Stdout.rho / Fs.rho — or in the composition template
/// itself — fails here.
#[test]
fn fs_generator_embedded_source_compiles() {
    let _ = standard_deploys::fs_generator("root", &[], None);
}

/// The signature computation is deterministic (function of PK, timestamp,
/// NONCE only).  Calling twice must return identical hex — otherwise
/// validators would compute different genesis blocks.
#[test]
fn fs_generator_signature_is_deterministic() {
    let d1 = standard_deploys::fs_generator("root", &[], None);
    let d2 = standard_deploys::fs_generator("root", &[], None);
    assert_eq!(
        d1.data.term, d2.data.term,
        "FsGenesis source (including embedded signature) must be identical across calls"
    );
}

/// Cross-process signature determinism: the sig hex derived from
/// (FS_GENERATOR_PK, FS_GENERATOR_TIMESTAMP, FS_NONCE) must equal a
/// hardcoded value.  If a future refactor swaps the signer to a
/// randomized-k implementation, this test fails LOCALLY instead of
/// silently splitting the network at runtime.  Update the constant
/// on purpose only when the signer semantics intentionally change.
#[test]
fn fs_generator_signature_matches_hardcoded_expected() {
    let sk =
        PrivateKey::from_bytes(&hex::decode(standard_deploys::FS_GENERATOR_PK).expect("valid hex"));
    let sig_hex =
        fs_genesis::fs_genesis_signature_hex(&sk, standard_deploys::FS_GENERATOR_TIMESTAMP);
    // Sig is deterministic (RFC 6979); capture the current value once
    // and lock it in.  If this assertion trips, either (a) the signer
    // changed semantics or (b) FS_GENERATOR_PK / _TIMESTAMP moved.
    // Regenerate via `println!("{sig_hex}")` and update.
    const EXPECTED_SIG_HEX: &str = include_str!("fs_generator_expected_sig.txt");
    let expected = EXPECTED_SIG_HEX.trim();
    assert_eq!(
        sig_hex.as_str(),
        expected,
        "signature drift detected — validators would diverge in production"
    );
}

/// Verify the sig actually validates against the public key.  Guards
/// against the "syntactically valid but cryptographically invalid"
/// failure mode where the composed source publishes a garbage sig
/// that the registry silently rejects.
#[test]
fn fs_generator_signature_verifies_against_pubkey() {
    let sk =
        PrivateKey::from_bytes(&hex::decode(standard_deploys::FS_GENERATOR_PK).expect("valid hex"));
    let sig_hex =
        fs_genesis::fs_genesis_signature_hex(&sk, standard_deploys::FS_GENERATOR_TIMESTAMP);
    let sig = hex::decode(&sig_hex).expect("valid hex sig");
    let pk = &*standard_deploys::FS_GENERATOR_PUB_KEY;
    // Reconstruct the to_sign tuple exactly as fs_genesis_signature_hex does.
    let to_sign: Par = new_etuple_par(vec![
        new_gint_par(standard_deploys::FS_GENERATOR_TIMESTAMP, Vec::new(), false),
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GByteArray(pk.bytes.to_vec())),
        }]),
        new_gint_par(fs_genesis::FS_NONCE, Vec::new(), false),
    ]);
    let sign_bytes = Blake2b256::hash(to_sign.encode_to_vec());
    let ok = Secp256k1.verify(&sign_bytes, &sig, &pk.bytes);
    assert!(ok, "sig must verify against FS_GENERATOR_PUB_KEY");
}

/// Composed source shape assertions.  A silent omission (e.g. `fs_body`
/// accidentally sourced from `embedded_rho::STDOUT` twice) would still
/// parse; these substring checks catch that.
#[test]
fn fs_generator_composed_source_contains_expected_shape() {
    let d = standard_deploys::fs_generator("root", &[], None);
    let term = &d.data.term;
    // Footer: mint fs and publish.
    assert!(
        term.contains("for (@fs <- Fs!?(0, 1, 2, {}))"),
        "footer mint pattern missing"
    );
    assert!(
        term.contains(".hexToBytes()"),
        "hex-bytes helper reference missing from publication call"
    );
    assert!(
        term.contains("rho:registry:insertSigned:secp256k1"),
        "insertSigned URN missing"
    );
    // Per-library body markers (pick a unique method or comment name
    // per file so accidental duplicate-inclusion fails).
    for marker in [
        "agent File",   // File.rho
        "agent Dir",    // Dir.rho
        "agent Stream", // Stream.rho
        "agent Buffer", // Buffer.rho
        "agent Stdin",  // Stdin.rho
        "agent Stdout", // Stdout.rho
        "agent Fs",     // Fs.rho
    ] {
        assert!(
            term.contains(marker),
            "composed source missing library marker: {marker}"
        );
    }
    // ST-27-4 review fix: negative assertions — the composed source
    // must NOT contain slice-17's cache identifiers.  A regression
    // that re-added the Fs cache would slip past the positive shape
    // checks above; these lock in the slice-27 revert.
    for forbidden in ["fsCacheP", "cacheAndOpenFile", "cacheAndOpenDir"] {
        assert!(
            !term.contains(forbidden),
            "composed source unexpectedly contains slice-17 cache name: {forbidden} \
             (slice 27 reverted the Fs cache — regression?)"
        );
    }
}

/// The publication URI derived from FS_GENERATOR_PK must be a well-
/// formed `rho:id:...` URI AND must not collide with any other blessed
/// contract's URI.
#[test]
fn fs_generator_uri_is_well_formed_and_unique() {
    let uri = fs_genesis::fs_genesis_uri(&standard_deploys::FS_GENERATOR_PUB_KEY);
    assert!(
        uri.starts_with("rho:id:"),
        "fs_genesis_uri must yield a rho:id: URI, got {uri}"
    );
    // Cross-check against all other blessed pubkeys.
    for pk in standard_deploys::system_public_keys() {
        // Skip our own key.
        if pk.bytes == standard_deploys::FS_GENERATOR_PUB_KEY.bytes {
            continue;
        }
        let other = fs_genesis::fs_genesis_uri(pk);
        assert_ne!(
            uri, other,
            "FS_GENERATOR_PUB_KEY URI collides with another blessed key"
        );
    }
}

/// Deploy sequence ordering: `fs_generator` must appear AFTER Registry
/// (it uses `rho:registry:insertSigned:secp256k1`) and BEFORE any deploy
/// that would depend on the published Fs cap.  Also asserts total deploy
/// count so an accidental removal or duplication trips the test.
#[test]
fn fs_generator_appears_in_deploy_sequence_after_registry() {
    use casper::rust::genesis::contracts::proof_of_stake::ProofOfStake;
    use casper::rust::genesis::contracts::validator::Validator as GenesisValidator;
    use casper::rust::genesis::genesis::Genesis;
    use crypto::rust::public_key::PublicKey;

    // Minimal PoS with one validator — required because pos_generator
    // asserts `!validators.is_empty()`.  Value doesn't matter for
    // ordering purposes.
    let stub_pk = PublicKey {
        bytes: vec![0x04u8; 65].into(),
    };
    let pos = ProofOfStake {
        minimum_bond: 1,
        maximum_bond: 1_000_000,
        validators: vec![GenesisValidator {
            pk: stub_pk,
            stake: 100,
        }],
        epoch_length: 100,
        quarantine_length: 100,
        number_of_active_validators: 1,
        fault_tolerance_threshold_ppm: 0,
        pos_multi_sig_public_keys: vec![],
        pos_multi_sig_quorum: 1,
    };
    let vaults = vec![];
    let deploys =
        Genesis::default_blessed_terms(&pos, &vaults, 0, "root", "F1R3fly", "F1R", 8, &[], None);
    // The FsGenesis deploy is the only one whose term binds
    // `rho:io:fs:native:1.0.0/open`.
    let fs_pos = deploys
        .iter()
        .position(|d| d.data.term.contains("rho:io:fs:native:1.0.0/open"))
        .expect("fs_generator deploy not found in sequence");
    // The Registry.rho deploy defines the `rho:registry:insertSigned`
    // URN itself; other deploys (Stack, ListOps, fs) merely USE it.
    let reg_pos = deploys
        .iter()
        .position(|d| {
            d.data.term.contains("rho:registry:lookup") && !d.data.term.contains("rho:io:fs:native")
        })
        .expect("registry deploy not found");
    assert!(
        fs_pos > reg_pos,
        "fs_generator (idx {fs_pos}) must come after registry (idx {reg_pos})"
    );
    // Count invariant: with empty vault set the sequence is exactly:
    // registry, versioned_registry, list_ops, either, non_negative_number,
    // make_mint, auth_key, system_vault, multi_sig_system_vault, stack,
    // token_metadata, fs_generator, pos_generator = 13.  Bump on purpose.
    assert_eq!(
        deploys.len(),
        13,
        "unexpected deploy count with empty vaults; someone changed the sequence"
    );
}
