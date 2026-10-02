use casper::rust::api::block_api::BlockAPI;
use casper::rust::genesis::contracts::standard_deploys;
use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::util::rholang::costacc::vault_payer::{balance_query_source, vault_payer};
use crypto::rust::private_key::PrivateKey;
use crypto::rust::public_key::PublicKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::{Cosigned, Cosigner};
use models::rhoapi::cost_signature::Value;
use models::rhoapi::CostSignature;
use models::rust::cost_deploy_data::DeployData;
use models::rust::cost_protocol_limits::offered_funded_v6_limits;
use models::rust::phlo_controls::PhloControlsV1;
use models::rust::phlo_intent::{
    PhloConversionCompositionV2, PhloFundingIntentV1, PhloFundingIntentV2,
    PhloFundingIntentV2Limits,
};
use models::rust::phlo_schedule::{PhloGenesisPolicy, PhloScheduleV1};
use models::rust::phlo_source::{PhloSourceLimits, PhloSourcePolicyV1};
use models::rust::signed_phlo_deploy::OfferedFundedDeploy;
use prost::Message;
use rholang::rust::interpreter::accounting::authority::cost_signature_to_sig;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativePhloDimension,
};
use rholang::rust::interpreter::accounting::phlo_execution::{PhloExecutionLimits, PhloResource};
use rholang::rust::interpreter::accounting::{principal_ground_v61, SignatureChannel};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::rho_type::RhoNumber;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::GenesisBuilder;

const COST_TERM: &str = "{% new x in { x!(0) | for (@n <- x) { Nil } } %}[ payer ]";

fn schedule() -> PhloScheduleV1<'static> {
    let identities: [&[u8]; 4] = [b"compute", b"introduction", b"transfer", b"trace"];
    PhloScheduleV1 {
        protocol_version: 6,
        network: b"test",
        shard: b"root",
        settlement_asset: b"REV",
        settlement_unit: b"atomic-REV",
        decimal_scale: 8,
        classes: NativePhloDimension::ALL
            .into_iter()
            .zip(identities)
            .map(|(dimension, identity)| dimension.resource_class(identity, 1))
            .collect(),
        actual_price: 2,
        compatibility_rule: native_resource_compatibility_rule(),
    }
}

fn funded_offer(
    keys: &[(PrivateKey, PublicKey)],
    source_count: usize,
    dense: bool,
) -> models::casper::DeployDataProto {
    let protocol = offered_funded_v6_limits();
    let mut payload_limits = protocol.envelope.payload;
    payload_limits.funding.sources = source_count;
    let selected = schedule();
    let terms = selected.encode(PhloGenesisPolicy::LIMITS).unwrap();
    let parsed = Compiler::source_to_adt(COST_TERM).unwrap();
    let payers = keys
        .iter()
        .take(source_count)
        .map(|(_, public)| {
            let signature = CostSignature {
                value: Some(Value::Ground(principal_ground_v61(&public.bytes))),
            };
            vault_payer(&signature).unwrap()
        })
        .collect::<Vec<_>>();
    let mut authorities =
        vec![
            cost_signature_to_sig(parsed.cost_signed_terms[0].signature.as_ref().unwrap()).unwrap(),
        ];
    authorities.extend(
        payers
            .iter()
            .map(|payer| cost_signature_to_sig(&payer.signature).unwrap()),
    );
    let locations = authorities
        .iter()
        .map(|authority| SignatureChannel::from_sig(authority).par.encode_to_vec())
        .collect::<Vec<_>>();
    let permissions = (0..source_count)
        .map(|index| {
            let authority_index = if dense || index < selected.classes.len() {
                0
            } else {
                index + 1
            };
            PhloResource {
                location: &locations[authority_index],
                class: index % selected.classes.len(),
                acquisition_terms: &terms,
                authority: &authorities[authority_index],
            }
            .wire_key(PhloExecutionLimits {
                resource_entries: 1,
                authority_nodes: payload_limits.funding.authority_nodes,
                key_bytes: payload_limits.funding.wire.field_bytes,
            })
            .unwrap()
        })
        .collect::<Vec<_>>();
    let sources = payers
        .iter()
        .enumerate()
        .map(|(index, payer)| {
            PhloSourcePolicyV1::new(
                &payer.custody_key,
                5_000_000,
                5_000_000,
                index == 0,
                vec![permissions[index].clone()],
                PhloSourceLimits {
                    wire: payload_limits.funding.wire,
                    resource_permissions: 1,
                    authority_nodes: payload_limits.funding.authority_nodes,
                },
            )
            .unwrap()
        })
        .collect();
    let funding = PhloFundingIntentV2 {
        base: PhloFundingIntentV1 {
            controls: PhloControlsV1 {
                limit: 2_000_000,
                price_ceiling: 2,
                required_owner_ceilings: vec![2],
                permitted_schedules: vec![selected.clone()],
            },
            schedule_commitment: selected.digest(PhloGenesisPolicy::LIMITS).unwrap(),
            total_exposure: 4_000_001,
            sources,
        },
        grant_uses: Vec::new(),
        conversion: PhloConversionCompositionV2::NoConversion,
    }
    .encode(PhloFundingIntentV2Limits {
        wire: payload_limits.funding.wire,
        base: payload_limits.funding,
        grant_uses: payload_limits.funding.wire.total_bytes / 8,
        grant_id_bytes: payload_limits.funding.wire.field_bytes,
        quote_evidence_bytes: payload_limits.funding.wire.field_bytes,
    })
    .unwrap();
    let body = DeployData {
        term: COST_TERM.to_owned(),
        language: "rholang".to_owned(),
        time_stamp: 1,
        valid_after_block_number: 0,
        shard_id: "root".to_owned(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let payload = OfferedFundedDeploy::new(body, funding, 2_000_000, 2, payload_limits).unwrap();
    let signer_keys = if source_count > protocol.envelope.members.get() {
        &keys[..1]
    } else {
        &keys[..source_count]
    };
    let mut signers = signer_keys
        .iter()
        .map(|(_, public)| Cosigner {
            pk: public.clone(),
            sig: prost::bytes::Bytes::from_static(&[1]),
            sig_algorithm: Box::new(Secp256k1),
        })
        .collect::<Vec<_>>();
    let hash = Cosigned::envelope_signing_hash(
        &payload,
        &signers,
        signers.len() as u32,
        &Secp256k1::name(),
    )
    .unwrap();
    for (signer, (private, _)) in signers.iter_mut().zip(signer_keys) {
        signer.sig = Secp256k1.sign(&hash, &private.bytes).into();
    }
    let signed = Cosigned::from_envelope_signed_data(payload, signers).unwrap();
    OfferedFundedDeploy::to_proto(&signed).unwrap()
}

async fn rooted_balance(
    node: &TestNode,
    root: &models::rust::block::state_hash::StateHash,
    address: &VaultAddress,
) -> u64 {
    let (values, _) = node
        .runtime_manager
        .play_exploratory_deploy(balance_query_source(address), root, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    u64::try_from(RhoNumber::unapply(&values[0]).unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn offered_funding_uses_all_64_sources_with_one_fee_across_validators() {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(3));
    parameters.2.version = 6;
    parameters.2.proof_of_stake.min_phlo_price = 1;
    let forbidden = standard_deploys::system_public_keys();
    let mut index = 10_000u32;
    while parameters.1.len() < 67 {
        let mut private_bytes = [0u8; 32];
        private_bytes[28..].copy_from_slice(&index.to_be_bytes());
        index += 1;
        let private = PrivateKey::from_bytes(&private_bytes);
        let public = Secp256k1.to_public(&private);
        if forbidden.iter().any(|key| **key == public) {
            continue;
        }
        parameters.2.vaults.push(Vault {
            vault_address: VaultAddress::from_public_key(&public).unwrap(),
            initial_balance: 9_000_000,
        });
        parameters.1.push((private, public));
    }
    assert_eq!(parameters.1.len(), 67);
    let policy = PhloGenesisPolicy::from_schedule(&schedule())
        .unwrap()
        .with_offered_funded_v6_active();
    let genesis = GenesisBuilder::new()
        .with_resource_policy(policy)
        .build_genesis_with_parameters(Some(parameters))
        .await
        .unwrap();
    let mut nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();
    let funded_keys = &genesis.genesis_vaults[3..];
    let valid = funded_offer(funded_keys, 64, false);
    let mut over_cap_keys = funded_keys.to_vec();
    let mut private_bytes = [0u8; 32];
    private_bytes[28..].copy_from_slice(&index.to_be_bytes());
    let private = PrivateKey::from_bytes(&private_bytes);
    over_cap_keys.push((private.clone(), Secp256k1.to_public(&private)));
    let over_cap = funded_offer(&over_cap_keys, 65, false);
    let rejected = BlockAPI::deploy_offered(&nodes[0].engine_cell, over_cap, &None, false, "root")
        .await
        .unwrap_err();
    assert!(rejected.to_string().contains("source limit"), "{rejected}");
    let deploy_id: block_storage::rust::dag::block_dag_key_value_storage::DeployId =
        valid.deploy_id.to_vec();
    BlockAPI::deploy_offered(&nodes[0].engine_cell, valid, &None, false, "root")
        .await
        .unwrap();
    nodes[0].allow_empty_blocks = true;
    let first = TestNode::propagate_block_at_index(&mut nodes, 0, &[])
        .await
        .unwrap();
    assert!(first.body.deploys.is_empty());
    let block = nodes[0].create_block_unsafe(&[]).await.unwrap();
    assert_eq!(block.body.deploys.len(), 1);
    assert_eq!(block.body.deploys[0].identity_bytes(), deploy_id.as_slice());
    for node in &mut nodes {
        assert!(matches!(
            node.process_block(block.clone()).await.unwrap(),
            Either::Right(_)
        ));
    }
    let producer = BlockAPI::find_offered_settlement_receipt(&nodes[0].engine_cell, &deploy_id)
        .await
        .unwrap()
        .unwrap();
    let validator = BlockAPI::find_offered_settlement_receipt(&nodes[1].engine_cell, &deploy_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(producer, validator);
    assert_eq!(producer.purses.len(), 64);
    assert!(
        producer
            .purses
            .iter()
            .filter(|row| row.resource_rev > 0)
            .count()
            >= 2
    );
    assert_eq!(
        producer.purses.iter().map(|row| row.fee_rev).sum::<u128>(),
        1
    );
    assert_eq!(producer.fee_rev, 1);
    assert_eq!(
        producer
            .purses
            .iter()
            .map(|row| row.resource_rev)
            .sum::<u128>(),
        u128::from(producer.fresh_phlo + producer.retained_phlo) * 2
    );
    assert_eq!(
        producer
            .purses
            .iter()
            .map(|row| row.resource_rev + row.fee_rev)
            .sum::<u128>(),
        producer.rev_spent
    );
    for (_, public) in funded_keys {
        let address = VaultAddress::from_public_key(public).unwrap();
        let row = producer
            .purses
            .iter()
            .find(|row| row.address == address.to_base58().as_bytes())
            .unwrap();
        assert_eq!(
            rooted_balance(&nodes[1], &block.body.state.post_state_hash, &address).await,
            row.post_balance
        );
    }

    let dense = funded_offer(funded_keys, 64, true);
    let dense_id = dense.deploy_id.to_vec();
    BlockAPI::deploy_offered(&nodes[0].engine_cell, dense, &None, false, "root")
        .await
        .unwrap();
    let barrier = TestNode::propagate_block_at_index(&mut nodes, 0, &[])
        .await
        .unwrap();
    assert!(barrier.body.deploys.is_empty());
    let next = nodes[0].create_block_unsafe(&[]).await.unwrap();
    assert!(next
        .body
        .deploys
        .iter()
        .all(|deploy| deploy.identity_bytes() != dense_id.as_slice()));
}
