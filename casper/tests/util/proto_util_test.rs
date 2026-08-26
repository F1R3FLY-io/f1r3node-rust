// See casper/src/test/scala/coop/rchain/casper/util/ProtoUtilTest.scala

use std::collections::HashSet;

use casper::rust::util::{construct_deploy, proto_util};
use crypto::rust::public_key::PublicKey;
use models::rust::block_implicits::{block_element_gen, block_hash_gen};
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::{
    Justification, ObjectiveEquivocationEvidence, ProcessedSystemDeploy, SystemDeployData,
};
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use prost::bytes::Bytes;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};

proptest! {
    #[test]
    fn dependencies_hashes_of_returns_exact_block_dependency_set(
        mut block in block_element_gen(None, None, None, None, None, None, None, None, None, None, None, None, None, None),
        slash_evidence in prop::collection::vec((block_hash_gen(), prop::option::of(block_hash_gen())), 0..5),
        header_evidence in prop::collection::vec(
            (block_hash_gen(), block_hash_gen())
                .prop_filter("objective evidence hashes must differ", |(first, second)| first != second),
            0..5,
        ),
    ) {
        block.body.system_deploys = slash_evidence
            .iter()
            .cloned()
            .map(|(invalid_block_hash, equivocation_block_hash)| ProcessedSystemDeploy::Succeeded {
                event_list: vec![],
                system_deploy: SystemDeployData::Slash {
                    invalid_block_hash,
                    equivocation_block_hash,
                    issuer_public_key: PublicKey::from_bytes(&[]),
                    target_activation_epoch: 0,
                    target_bond_generation: BondGeneration::GENESIS,
                },
                pre_state_hash: Bytes::new(),
                post_state_hash: Bytes::new(),
            })
            .chain(std::iter::once(ProcessedSystemDeploy::Succeeded {
                event_list: vec![],
                system_deploy: SystemDeployData::CloseBlockSystemDeployData,
                pre_state_hash: Bytes::new(),
                post_state_hash: Bytes::new(),
            }))
            .chain(std::iter::once(ProcessedSystemDeploy::Failed {
                event_list: vec![],
                error_msg: "failed".to_string(),
                pre_state_hash: Bytes::new(),
                post_state_hash: Bytes::new(),
            }))
            .collect();
        block.header.objective_equivocation_evidence_delta = header_evidence
            .into_iter()
            .enumerate()
            .map(|(sequence_number, (first, second))| {
                ObjectiveEquivocationEvidence::new(
                    Bytes::from(vec![9; models::rust::validator::LENGTH]),
                    BondGeneration::GENESIS,
                    sequence_number as i32,
                    first,
                    second,
                )
                .expect("valid generated objective evidence")
            })
            .collect();
        let result = proto_util::dependencies_hashes_of(&block);

        let justifications_hashes: Vec<Bytes> = block
            .justifications
            .iter()
            .map(|j| j.latest_block_hash.clone())
            .collect();

        let parents_hashes: Vec<Bytes> = block.header.parents_hash_list.clone();

        let header_evidence_hashes: Vec<Bytes> = block
            .header
            .objective_equivocation_evidence_delta
            .iter()
            .flat_map(|evidence| {
                [
                    evidence.first_block_hash.clone(),
                    evidence.second_block_hash.clone(),
                ]
            })
            .collect();

        for hash in &justifications_hashes {
            prop_assert!(result.contains(hash), "Missing justification hash");
        }

        for hash in &parents_hashes {
            prop_assert!(result.contains(hash), "Missing parent hash");
        }

        let result_set: HashSet<Bytes> = result.iter().cloned().collect();
        let expected: HashSet<Bytes> = justifications_hashes
            .into_iter()
            .chain(parents_hashes.into_iter())
            .chain(slash_evidence.into_iter().flat_map(|(first, second)| {
                std::iter::once(first).chain(second)
            }))
            .chain(header_evidence_hashes)
            .collect();

        prop_assert_eq!(result_set, expected);

        let mut permuted = block;
        permuted.header.parents_hash_list.reverse();
        permuted.justifications.reverse();
        permuted.body.system_deploys.reverse();
        permuted
            .header
            .objective_equivocation_evidence_delta
            .reverse();
        prop_assert_eq!(
            proto_util::dependencies_hashes_of(&permuted),
            result,
            "dependency projection changed under origin permutation",
        );
    }
}

#[test]
fn equivocation_slash_requires_both_evidence_blocks() {
    let first = Bytes::from_static(b"equivocation-a");
    let second = Bytes::from_static(b"equivocation-b");
    let mut block = block_element_gen(
        None, None, None, None, None, None, None, None, None, None, None, None, None, None,
    )
    .new_tree(&mut proptest::test_runner::TestRunner::default())
    .expect("block")
    .current();
    block.body.system_deploys = vec![ProcessedSystemDeploy::Succeeded {
        event_list: vec![],
        system_deploy: SystemDeployData::Slash {
            invalid_block_hash: first.clone(),
            equivocation_block_hash: Some(second.clone()),
            issuer_public_key: PublicKey::from_bytes(&[]),
            target_activation_epoch: 0,
            target_bond_generation: BondGeneration::GENESIS,
        },
        pre_state_hash: Bytes::new(),
        post_state_hash: Bytes::new(),
    }];

    assert_eq!(proto_util::slash_evidence_dependencies_of(&block), vec![
        proto_util::SlashEvidenceDependency::ObjectivePair { first, second }
    ]);
}

#[test]
fn every_structural_and_evidence_origin_projects_to_the_exact_dependency_set() {
    let hash = |byte| Bytes::from(vec![byte; models::rust::block_hash::LENGTH]);
    let parent = hash(1);
    let justification = hash(2);
    let unary = hash(3);
    let pair_first = hash(4);
    let pair_second = hash(5);
    let header_first = hash(6);
    let header_second = hash(7);
    let mut block = block_element_gen(
        None, None, None, None, None, None, None, None, None, None, None, None, None, None,
    )
    .new_tree(&mut proptest::test_runner::TestRunner::default())
    .expect("block")
    .current();
    block.header.parents_hash_list = vec![parent.clone()];
    block.justifications = vec![Justification {
        validator: Bytes::from(vec![8; models::rust::validator::LENGTH]),
        latest_block_hash: justification.clone(),
    }];
    block.header.objective_equivocation_evidence_delta = vec![ObjectiveEquivocationEvidence::new(
        Bytes::from(vec![9; models::rust::validator::LENGTH]),
        BondGeneration::GENESIS,
        1,
        header_first.clone(),
        header_second.clone(),
    )
    .expect("header evidence")];
    block.body.system_deploys = vec![
        ProcessedSystemDeploy::Succeeded {
            event_list: vec![],
            system_deploy: SystemDeployData::Slash {
                invalid_block_hash: unary.clone(),
                equivocation_block_hash: None,
                issuer_public_key: PublicKey::from_bytes(&[]),
                target_activation_epoch: 0,
                target_bond_generation: BondGeneration::GENESIS,
            },
            pre_state_hash: Bytes::new(),
            post_state_hash: Bytes::new(),
        },
        ProcessedSystemDeploy::Succeeded {
            event_list: vec![],
            system_deploy: SystemDeployData::Slash {
                invalid_block_hash: pair_first.clone(),
                equivocation_block_hash: Some(pair_second.clone()),
                issuer_public_key: PublicKey::from_bytes(&[]),
                target_activation_epoch: 0,
                target_bond_generation: BondGeneration::GENESIS,
            },
            pre_state_hash: Bytes::new(),
            post_state_hash: Bytes::new(),
        },
    ];

    assert_eq!(proto_util::slash_evidence_dependencies_of(&block), vec![
        proto_util::SlashEvidenceDependency::LegacyUnary(unary.clone()),
        proto_util::SlashEvidenceDependency::ObjectivePair {
            first: pair_first.clone(),
            second: pair_second.clone(),
        },
    ]);
    assert_eq!(
        proto_util::dependencies_hashes_of(&block)
            .into_iter()
            .collect::<HashSet<_>>(),
        HashSet::from([
            parent,
            justification,
            unary,
            pair_first,
            pair_second,
            header_first,
            header_second,
        ])
    );
}

#[test]
fn slash_evidence_dependency_is_deduplicated_with_parent_and_justification() {
    let evidence_hash = Bytes::from_static(b"evidence");
    let slash = ProcessedSystemDeploy::Succeeded {
        event_list: vec![],
        system_deploy: SystemDeployData::Slash {
            invalid_block_hash: evidence_hash.clone(),
            equivocation_block_hash: None,
            issuer_public_key: PublicKey::from_bytes(&[]),
            target_activation_epoch: 0,
            target_bond_generation: BondGeneration::GENESIS,
        },
        pre_state_hash: Bytes::new(),
        post_state_hash: Bytes::new(),
    };
    let block = block_element_gen(
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(vec![evidence_hash.clone()]),
        Some(vec![
            models::rust::casper::protocol::casper_message::Justification {
                validator: Bytes::from_static(b"validator"),
                latest_block_hash: evidence_hash.clone(),
            },
        ]),
        None,
        Some(vec![slash]),
        None,
        None,
        None,
    )
    .new_tree(&mut proptest::test_runner::TestRunner::default())
    .expect("block")
    .current();

    assert_eq!(proto_util::dependencies_hashes_of(&block), vec![
        evidence_hash
    ]);
}

struct TestContext {
    genesis: GenesisContext,
}

impl TestContext {
    async fn new() -> Self {
        let mut genesis_builder = GenesisBuilder::new();
        let genesis_parameters_tuple =
            GenesisBuilder::build_genesis_parameters_with_defaults(None, None);
        let genesis_context = genesis_builder
            .build_genesis_with_parameters(Some(genesis_parameters_tuple))
            .await
            .expect("Failed to build genesis context");

        Self {
            genesis: genesis_context,
        }
    }
}

#[tokio::test]
async fn unseen_block_hashes_should_return_empty_for_a_single_block_dag() {
    let ctx = TestContext::new().await;
    let mut node = TestNode::standalone(ctx.genesis.clone()).await.unwrap();

    let shard_id = ctx.genesis.genesis_block.shard_id.clone();

    let deploy = construct_deploy::basic_deploy_data(0, None, Some(shard_id)).unwrap();
    let signed_block = node.add_block_from_deploys(&[deploy]).await.unwrap();

    let dag = node
        .block_dag_storage
        .get_representation()
        .expect("dag representation");

    let unseen_block_hashes = proto_util::unseen_block_hashes(
        &dag,
        &signed_block.justifications,
        Some(&signed_block.block_hash),
    )
    .unwrap();

    assert!(
        unseen_block_hashes.is_empty(),
        "Expected empty set but got {:?}",
        unseen_block_hashes
    );
}

#[tokio::test]
async fn unseen_block_hashes_should_return_all_but_the_first_block_when_passed_the_first_block_in_a_chain(
) {
    let ctx = TestContext::new().await;
    let mut node = TestNode::standalone(ctx.genesis.clone()).await.unwrap();

    let shard_id = ctx.genesis.genesis_block.shard_id.clone();

    let deploy0 = construct_deploy::basic_deploy_data(0, None, Some(shard_id.clone())).unwrap();
    let block0 = node.add_block_from_deploys(&[deploy0]).await.unwrap();

    let deploy1 = construct_deploy::basic_deploy_data(1, None, Some(shard_id)).unwrap();
    let block1 = node.add_block_from_deploys(&[deploy1]).await.unwrap();

    let dag = node
        .block_dag_storage
        .get_representation()
        .expect("dag representation");

    let unseen_block_hashes =
        proto_util::unseen_block_hashes(&dag, &block0.justifications, Some(&block0.block_hash))
            .unwrap();

    let expected: HashSet<Bytes> = vec![block1.block_hash.clone()].into_iter().collect();
    assert_eq!(
        unseen_block_hashes, expected,
        "Expected {:?} but got {:?}",
        expected, unseen_block_hashes
    );
}
