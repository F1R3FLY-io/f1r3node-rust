use std::collections::{BTreeMap, BTreeSet};

use crypto::rust::public_key::PublicKey;
use models::rust::block_hash::BlockHashSerde;
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::{
    FinalizationCertificate, ObjectiveEquivocationEvidence,
};
use models::rust::validator::ValidatorSerde;
use proptest::prelude::*;
use prost::bytes::Bytes;

use super::*;

fn hash(value: u8) -> BlockHash { Bytes::from(vec![value; 32]) }

fn certificate() -> FinalizationCertificate {
    FinalizationCertificate {
        schema_version: FinalizationCertificate::SCHEMA_VERSION,
        protocol_version: 6,
        shard_id: "root".into(),
        genesis_hash: BlockHashSerde(hash(20)),
        predecessor_floor_hash: BlockHashSerde(hash(9)),
        predecessor_certificate_digest: BlockHashSerde(hash(21)),
        predecessor_certificate_block_hash: BlockHashSerde(hash(10)),
        target_floor_hash: BlockHashSerde(hash(11)),
        target_post_state_hash: BlockHashSerde(hash(22)),
        target_block_number: 1,
        fault_tolerance_numerator: 1,
        fault_tolerance_denominator: 1,
        exact_latest_messages: BTreeMap::from([
            (ValidatorSerde(hash(1)), BlockHashSerde(hash(8))),
            (ValidatorSerde(hash(2)), BlockHashSerde(hash(0))),
        ]),
        authority_context_digest: BlockHashSerde(hash(23)),
        supporting_manifest_digest: BlockHashSerde(hash(24)),
        finalized_manifest_digest: BlockHashSerde(hash(25)),
        supporting_block_count: 0,
        finalized_block_count: 0,
    }
}

fn slash(first: u8, second: Option<u8>) -> ProcessedSystemDeploy {
    ProcessedSystemDeploy::Succeeded {
        event_list: Vec::new(),
        system_deploy: SystemDeployData::Slash {
            invalid_block_hash: hash(first),
            equivocation_block_hash: second.map(hash),
            issuer_public_key: PublicKey::new(hash(90)),
            target_activation_epoch: 0,
            target_bond_generation: BondGeneration::GENESIS,
        },
        pre_state_hash: hash(91),
        post_state_hash: hash(92),
    }
}

fn fixture() -> BlockMessage {
    let mut block = models::rust::block_implicits::get_random_block_default();
    block.header.parents_hash_list = vec![hash(0), hash(1)];
    block.justifications = vec![Justification {
        validator: hash(90),
        latest_block_hash: hash(2),
    }];
    block.body.system_deploys = vec![
        slash(3, None),
        slash(4, Some(5)),
        ProcessedSystemDeploy::Failed {
            event_list: Vec::new(),
            error_msg: "fixture".into(),
            pre_state_hash: hash(93),
            post_state_hash: hash(94),
        },
        ProcessedSystemDeploy::Succeeded {
            event_list: Vec::new(),
            system_deploy: SystemDeployData::CloseBlockSystemDeployData,
            pre_state_hash: hash(95),
            post_state_hash: hash(96),
        },
    ];
    block.header.objective_equivocation_evidence_delta = vec![ObjectiveEquivocationEvidence {
        validator: hash(90),
        bond_generation: BondGeneration::GENESIS,
        sequence_number: 0,
        first_block_hash: hash(6),
        second_block_hash: hash(7),
    }];
    block.finalized_floor_certificate = Some(certificate());
    block
}

#[test]
fn empty_dependencies_require_no_reads_without_a_certificate() {
    let mut block = fixture();
    block.header.parents_hash_list.clear();
    block.justifications.clear();
    block.body.system_deploys.clear();
    block.header.objective_equivocation_evidence_delta.clear();
    block.finalized_floor_certificate = None;
    assert_eq!(dependency_hashes_iter(&block).count(), 0);
    assert!(
        dependencies_have_admitted_metadata::<()>(&block, |_| panic!("unexpected read")).unwrap()
    );
}

#[test]
fn repeated_dependencies_preserve_missing_observations_and_later_errors() {
    let mut block = fixture();
    block.header.parents_hash_list = vec![hash(1); 3];
    block.justifications.clear();
    block.body.system_deploys.clear();
    block.header.objective_equivocation_evidence_delta.clear();
    block.finalized_floor_certificate = None;
    for (results, expected) in [
        ([Ok(false), Ok(true), Ok(true)], Ok(false)),
        ([Ok(true), Ok(false), Ok(true)], Ok(false)),
        ([Ok(false), Ok(true), Err("later read")], Err("later read")),
        ([Ok(true); 3], Ok(true)),
    ] {
        let mut results = results.into_iter();
        assert_eq!(
            dependencies_have_admitted_metadata(&block, |key| {
                assert_eq!(key, &hash(1));
                results.next().unwrap()
            }),
            expected
        );
        assert!(results.next().is_none());
    }
}

#[test]
fn every_dependency_origin_is_required_and_non_dependencies_are_excluded() {
    let block = fixture();
    let borrowed: BTreeSet<_> = dependency_hashes_iter(&block).cloned().collect();
    let expected: BTreeSet<_> = (0..=11).map(hash).collect();
    assert_eq!(borrowed, expected);
    assert_eq!(
        borrowed,
        dependencies_hashes_of(&block).into_iter().collect()
    );
    for missing in &expected {
        assert!(
            !dependencies_have_admitted_metadata(&block, |key| Ok::<_, ()>(key != missing))
                .unwrap()
        );
        assert_eq!(
            dependencies_have_admitted_metadata(&block, |key| {
                if key == missing {
                    Err("metadata error")
                } else {
                    Ok(true)
                }
            }),
            Err("metadata error")
        );
    }
}

#[test]
fn missing_parent_cannot_hide_a_later_certificate_read_error() {
    let block = fixture();
    let mut visited = Vec::new();
    let result = dependencies_have_admitted_metadata(&block, |key| {
        visited.push(key.clone());
        if key == &hash(11) {
            Err("certificate metadata")
        } else {
            Ok(key != &hash(0))
        }
    });
    assert_eq!(result, Err("certificate metadata"));
    assert_eq!(visited.len(), 12);
}

#[test]
fn all_zero_sentinels_are_excluded_only_from_certificate_references() {
    let mut block = fixture();
    block.header.parents_hash_list = vec![hash(0)];
    block.justifications.clear();
    block.body.system_deploys = vec![slash(0, Some(0))];
    block.header.objective_equivocation_evidence_delta.clear();
    let certificate = block.finalized_floor_certificate.as_mut().unwrap();
    certificate.predecessor_floor_hash = BlockHashSerde(hash(0));
    certificate.predecessor_certificate_block_hash = BlockHashSerde(hash(0));
    certificate.target_floor_hash = BlockHashSerde(hash(0));
    certificate.exact_latest_messages.clear();
    assert_eq!(
        dependency_hashes_iter(&block).cloned().collect::<Vec<_>>(),
        vec![hash(0); 3]
    );
}

proptest! {
    #[test]
    fn borrowed_dependencies_equal_the_canonical_dependency_set(
        operations in prop::collection::vec((0_u8..9, 0_u8..16, 0_u8..16), 0..128),
        missing in prop::collection::btree_set(0_u8..16, 0..16),
        faulty in prop::collection::btree_set(0_u8..16, 0..16),
    ) {
        let mut block = fixture();
        block.header.parents_hash_list.clear();
        block.justifications.clear();
        block.body.system_deploys.clear();
        block.header.objective_equivocation_evidence_delta.clear();
        let cert = block.finalized_floor_certificate.as_mut().unwrap();
        cert.exact_latest_messages.clear();
        cert.predecessor_floor_hash = BlockHashSerde(hash(0));
        cert.predecessor_certificate_block_hash = BlockHashSerde(hash(0));
        cert.target_floor_hash = BlockHashSerde(hash(0));
        for (source, first, second) in operations {
            match source {
                0 => block.header.parents_hash_list.push(hash(first)),
                1 => block.justifications.push(Justification { validator: hash(second), latest_block_hash: hash(first) }),
                2 => block.body.system_deploys.push(slash(first, None)),
                3 => block.body.system_deploys.push(slash(first, Some(second))),
                4 => block.header.objective_equivocation_evidence_delta.push(ObjectiveEquivocationEvidence {
                    validator: hash(90), bond_generation: BondGeneration::GENESIS, sequence_number: 0,
                    first_block_hash: hash(first), second_block_hash: hash(second),
                }),
                5 => { cert.exact_latest_messages.insert(ValidatorSerde(hash(second)), BlockHashSerde(hash(first))); },
                6 => cert.predecessor_floor_hash = BlockHashSerde(hash(first)),
                7 => cert.predecessor_certificate_block_hash = BlockHashSerde(hash(first)),
                _ => cert.target_floor_hash = BlockHashSerde(hash(first)),
            }
        }
        let canonical: BTreeSet<_> = dependencies_hashes_of(&block).into_iter().collect();
        let borrowed: BTreeSet<_> = dependency_hashes_iter(&block).cloned().collect();
        prop_assert_eq!(&borrowed, &canonical);
        let has_error = canonical.iter().any(|key| faulty.contains(&key[0]));
        let all_present = canonical.iter().all(|key| !missing.contains(&key[0]));
        let mut visits = 0;
        let result = dependencies_have_admitted_metadata(&block, |key| {
            visits += 1;
            if faulty.contains(&key[0]) { Err(()) } else { Ok(!missing.contains(&key[0])) }
        });
        prop_assert_eq!(result, if has_error { Err(()) } else { Ok(all_present) });
        if !has_error { prop_assert_eq!(visits, dependency_hashes_iter(&block).count()); }
    }
}
