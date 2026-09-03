// See casper/src/test/scala/coop/rchain/casper/genesis/contracts/StandardDeploysSpec.scala

use casper::rust::genesis::contracts::standard_deploys;
use models::rust::block_metadata::CERTIFIED_ADMISSION_PROTOCOL_VERSION;
use models::rust::casper::protocol::casper_message::ProcessedDeploy;

use crate::util::genesis_builder::GenesisBuilder;

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

#[test]
fn system_vault_embedded_source_compiles() { let _ = standard_deploys::system_vault("root"); }

#[test]
fn proof_of_stake_embedded_template_compiles() {
    let parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
    let _ = standard_deploys::pos_generator(&parameters.2.proof_of_stake, "root");
}

#[test]
fn protocol_envelope_preserves_legacy_identity_before_v6() {
    let signed = standard_deploys::registry("root");
    let envelope = standard_deploys::protocol_envelope(
        signed.clone(),
        CERTIFIED_ADMISSION_PROTOCOL_VERSION - 1,
    )
    .unwrap();
    let processed = ProcessedDeploy::empty_from_cosigned(&envelope);

    assert!(!envelope.is_envelope_bound());
    assert!(processed.envelope_commitment.is_empty());
    assert_eq!(processed.deploy.pk, signed.pk);
    assert_eq!(processed.deploy.sig, signed.sig);
    assert_eq!(processed.deploy.data, signed.data);
}

#[test]
fn protocol_envelope_commits_and_round_trips_blessed_v6_identity() {
    let envelope = standard_deploys::protocol_envelope(
        standard_deploys::registry("root"),
        CERTIFIED_ADMISSION_PROTOCOL_VERSION,
    )
    .unwrap();
    let commitment = envelope.envelope_commitment().unwrap();
    let processed = ProcessedDeploy::empty_from_cosigned(&envelope);
    let replay = processed.to_cosigned().unwrap();

    assert!(envelope.is_envelope_bound());
    assert_eq!(commitment.len(), 32);
    assert_eq!(processed.envelope_commitment, commitment);
    assert_eq!(processed.cosigner_threshold, 1);
    assert!(replay.is_envelope_bound());
    assert_eq!(replay.envelope_commitment().unwrap(), commitment);
}

#[test]
fn protocol_envelope_is_deterministic_for_blessed_v6_deploys() {
    let first = standard_deploys::protocol_envelope(
        standard_deploys::registry("root"),
        CERTIFIED_ADMISSION_PROTOCOL_VERSION,
    )
    .unwrap();
    let second = standard_deploys::protocol_envelope(
        standard_deploys::registry("root"),
        CERTIFIED_ADMISSION_PROTOCOL_VERSION,
    )
    .unwrap();

    assert_eq!(
        first.envelope_commitment().unwrap(),
        second.envelope_commitment().unwrap()
    );
    assert_eq!(first.signers()[0].sig, second.signers()[0].sig);
}
