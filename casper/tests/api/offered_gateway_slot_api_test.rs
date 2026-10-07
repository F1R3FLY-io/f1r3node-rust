use casper::rust::api::block_api::BlockAPI;
use casper::rust::genesis::contracts::vault::Vault;
use casper::rust::util::rholang::costacc::vault_payer::{balance_query_source, vault_payer};
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signed::Cosigned;
use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostSignature, Par};
use models::rust::casper::protocol::casper_message::BlockMessage;
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
use models::rust::utils::new_gstring_par;
use prost::Message;
use rholang::rust::interpreter::accounting::authority::cost_signature_to_sig;
use rholang::rust::interpreter::accounting::native_phlo_rules::{
    native_resource_compatibility_rule, NativePhloDimension,
};
use rholang::rust::interpreter::accounting::phlo_execution::{PhloExecutionLimits, PhloResource};
use rholang::rust::interpreter::accounting::{principal_ground_v61, SignatureChannel};
use rholang::rust::interpreter::rho_type::{RhoBoolean, RhoNumber, RhoString};
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::history::Either;

use crate::helper::test_node::TestNode;
use crate::util::genesis_builder::{GenesisBuilder, EXTRA_GENESIS_VAULT_KEY_PAIRS};

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

fn signed_direct_offer(
    term: String,
    timestamp: i64,
    valid_after: i64,
    owner_secret: crypto::rust::private_key::PrivateKey,
    owner_public: &crypto::rust::public_key::PublicKey,
) -> models::casper::DeployDataProto {
    let limits = offered_funded_v6_limits().envelope.payload;
    let signature = CostSignature {
        value: Some(Value::Ground(principal_ground_v61(&owner_public.bytes))),
    };
    let payer = vault_payer(&signature).unwrap();
    let selected = schedule();
    let terms = selected.encode(PhloGenesisPolicy::LIMITS).unwrap();
    let authority = cost_signature_to_sig(&signature).unwrap();
    let location = SignatureChannel::from_sig(&authority).par.encode_to_vec();
    let permissions = (0..selected.classes.len())
        .map(|class| {
            PhloResource {
                location: &location,
                class,
                acquisition_terms: &terms,
                authority: &authority,
            }
            .wire_key(PhloExecutionLimits {
                resource_entries: 1,
                authority_nodes: limits.funding.authority_nodes,
                key_bytes: limits.funding.wire.field_bytes,
            })
            .unwrap()
        })
        .collect();
    let source = PhloSourcePolicyV1::new(
        &payer.custody_key,
        5_000_000,
        5_000_000,
        true,
        permissions,
        PhloSourceLimits {
            wire: limits.funding.wire,
            resource_permissions: selected.classes.len(),
            authority_nodes: limits.funding.authority_nodes,
        },
    )
    .unwrap();
    let funding = PhloFundingIntentV2 {
        base: PhloFundingIntentV1 {
            controls: PhloControlsV1 {
                limit: 2_000_000,
                price_ceiling: 2,
                required_owner_ceilings: vec![2],
                permitted_schedules: vec![selected.clone()],
            },
            schedule_commitment: selected.digest(PhloGenesisPolicy::LIMITS).unwrap(),
            total_exposure: 5_000_000,
            sources: vec![source],
        },
        grant_uses: Vec::new(),
        conversion: PhloConversionCompositionV2::NoConversion,
    }
    .encode(PhloFundingIntentV2Limits {
        wire: limits.funding.wire,
        base: limits.funding,
        grant_uses: limits.funding.wire.total_bytes / 8,
        grant_id_bytes: limits.funding.wire.field_bytes,
        quote_evidence_bytes: limits.funding.wire.field_bytes,
    })
    .unwrap();
    let body = DeployData {
        term,
        language: "rholang".to_owned(),
        time_stamp: timestamp,
        valid_after_block_number: valid_after,
        shard_id: "root".to_owned(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let payload = OfferedFundedDeploy::new(body, funding, 2_000_000, 2, limits).unwrap();
    let signed =
        Cosigned::create_single_envelope(payload, Box::new(Secp256k1), owner_secret).unwrap();
    OfferedFundedDeploy::to_proto(&signed).unwrap()
}

async fn propagate_offer(
    nodes: &mut [TestNode],
    offer: models::casper::DeployDataProto,
    stage: &str,
) -> BlockMessage {
    BlockAPI::deploy_offered(&nodes[0].engine_cell, offer, &None, false, "root")
        .await
        .unwrap_or_else(|error| panic!("{stage} admission: {error}"));
    let block = nodes[0]
        .create_block_unsafe(&[])
        .await
        .unwrap_or_else(|error| panic!("{stage} proposal: {error}"));
    for node in nodes {
        assert!(matches!(
            node.process_block(block.clone()).await.unwrap(),
            Either::Right(_)
        ));
    }
    block
}

fn collect_private_signature(signature: &CostSignature, output: &mut Vec<CostSignature>) {
    match signature.value.as_ref() {
        Some(Value::Name(par)) if par.unforgeables.len() == 1 => {
            if vault_payer(signature).is_ok() && !output.contains(signature) {
                output.push(signature.clone());
            }
        }
        Some(Value::Compound(compound)) => {
            for part in &compound.elements {
                collect_private_signature(part, output);
            }
        }
        _ => {}
    }
}

fn collect_private_in_par(par: &Par, output: &mut Vec<CostSignature>) {
    for signed in &par.cost_signed_terms {
        if let Some(signature) = &signed.signature {
            collect_private_signature(signature, output);
        }
        if let Some(body) = &signed.body {
            collect_private_in_par(body, output);
        }
    }
    for new in &par.news {
        if let Some(body) = &new.p {
            collect_private_in_par(body, output);
        }
    }
    for receive in &par.receives {
        if let Some(body) = &receive.body {
            collect_private_in_par(body, output);
        }
        if let Some(condition) = &receive.condition {
            collect_private_in_par(condition, output);
        }
    }
    for conditional in &par.conditionals {
        for branch in [
            conditional.condition.as_ref(),
            conditional.if_true.as_ref(),
            conditional.if_false.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            collect_private_in_par(branch, output);
        }
    }
    for branch in &par.matches {
        if let Some(target) = &branch.target {
            collect_private_in_par(target, output);
        }
        for case in &branch.cases {
            if let Some(source) = &case.source {
                collect_private_in_par(source, output);
            }
        }
    }
    for bundle in &par.bundles {
        if let Some(body) = &bundle.body {
            collect_private_in_par(body, output);
        }
    }
}

async fn data(
    nodes: &[TestNode],
    root: &models::rust::block::state_hash::StateHash,
    name: &str,
) -> Vec<Par> {
    nodes[0]
        .runtime_manager
        .get_data(
            root.clone(),
            &new_gstring_par(name.to_owned(), Vec::new(), false),
        )
        .await
        .unwrap()
}

async fn balance(
    nodes: &[TestNode],
    root: &models::rust::block::state_hash::StateHash,
    address: &VaultAddress,
) -> u64 {
    let (values, _) = nodes[0]
        .runtime_manager
        .play_exploratory_deploy(balance_query_source(address), root, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    u64::try_from(RhoNumber::unapply(&values[0]).unwrap()).unwrap()
}

fn offered_gateway_call(
    gateway_secret: crypto::rust::private_key::PrivateKey,
    gateway_public: &crypto::rust::public_key::PublicKey,
    entry: CostSignature,
    slot: CostSignature,
) -> models::casper::DeployDataProto {
    let limits = offered_funded_v6_limits().envelope.payload;
    let gateway = CostSignature {
        value: Some(Value::Ground(principal_ground_v61(&gateway_public.bytes))),
    };
    let signatures = [gateway, entry, slot];
    let payers = signatures
        .iter()
        .map(|signature| vault_payer(signature).unwrap())
        .collect::<Vec<_>>();
    let authorities = signatures
        .iter()
        .map(|signature| cost_signature_to_sig(signature).unwrap())
        .collect::<Vec<_>>();
    let locations = authorities
        .iter()
        .map(|authority| SignatureChannel::from_sig(authority).par.encode_to_vec())
        .collect::<Vec<_>>();
    let selected = schedule();
    let terms = selected.encode(PhloGenesisPolicy::LIMITS).unwrap();
    let sources = payers
        .iter()
        .enumerate()
        .map(|(index, payer)| {
            let permissions = (0..selected.classes.len())
                .map(|class| {
                    PhloResource {
                        location: &locations[index],
                        class,
                        acquisition_terms: &terms,
                        authority: &authorities[index],
                    }
                    .wire_key(PhloExecutionLimits {
                        resource_entries: 1,
                        authority_nodes: limits.funding.authority_nodes,
                        key_bytes: limits.funding.wire.field_bytes,
                    })
                    .unwrap()
                })
                .collect();
            PhloSourcePolicyV1::new(
                &payer.custody_key,
                if index == 0 { 300_000 } else { 100_000 },
                if index == 0 { 300_000 } else { 100_000 },
                index == 0,
                permissions,
                PhloSourceLimits {
                    wire: limits.funding.wire,
                    resource_permissions: selected.classes.len(),
                    authority_nodes: limits.funding.authority_nodes,
                },
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let intent = PhloFundingIntentV2 {
        base: PhloFundingIntentV1 {
            controls: PhloControlsV1 {
                limit: 250_000,
                price_ceiling: 2,
                required_owner_ceilings: vec![2],
                permitted_schedules: vec![selected.clone()],
            },
            schedule_commitment: selected.digest(PhloGenesisPolicy::LIMITS).unwrap(),
            total_exposure: 900_000,
            sources,
        },
        grant_uses: Vec::new(),
        conversion: PhloConversionCompositionV2::NoConversion,
    }
    .encode(PhloFundingIntentV2Limits {
        wire: limits.funding.wire,
        base: limits.funding,
        grant_uses: limits.funding.wire.total_bytes / 8,
        grant_id_bytes: limits.funding.wire.field_bytes,
        quote_evidence_bytes: limits.funding.wire.field_bytes,
    })
    .unwrap();
    let body = DeployData {
        term: "new deployerId(`rho:system:deployerId`) in { @\"agent-trigger\"!(0, *deployerId) }"
            .to_owned(),
        language: "rholang".to_owned(),
        time_stamp: 4,
        valid_after_block_number: 3,
        shard_id: "root".to_owned(),
        expiration_timestamp: None,
        authority_presentations: Vec::new(),
    };
    let payload = OfferedFundedDeploy::new(body, intent, 250_000, 2, limits).unwrap();
    let signed =
        Cosigned::create_single_envelope(payload, Box::new(Secp256k1), gateway_secret).unwrap();
    OfferedFundedDeploy::to_proto(&signed).unwrap()
}

/// The three-node network with a funded gateway vault that the gateway tests
/// share.
async fn gateway_network() -> (crate::util::genesis_builder::GenesisContext, Vec<TestNode>) {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(3));
    parameters.2.version = 6;
    parameters.2.proof_of_stake.min_phlo_price = 1;
    let extra_gateway = EXTRA_GENESIS_VAULT_KEY_PAIRS[1].clone();
    parameters.2.vaults.push(Vault {
        vault_address: VaultAddress::from_public_key(&extra_gateway.1).unwrap(),
        initial_balance: 9_000_000,
    });
    parameters.1.push(extra_gateway);
    let policy = PhloGenesisPolicy::from_schedule(&schedule())
        .unwrap()
        .with_offered_funded_v6_active();
    let genesis = GenesisBuilder::new()
        .with_resource_policy(policy)
        .build_genesis_with_parameters(Some(parameters))
        .await
        .unwrap();
    let nodes = TestNode::create_network(genesis.clone(), 3, None, None, None, None)
        .await
        .unwrap();
    (genesis, nodes)
}

/// The installer's offer: an unsigned trigger that runs the gateway's signed
/// `entry -o slot` body and publishes the entry and slot vault addresses.
fn installer_offer(
    genesis: &crate::util::genesis_builder::GenesisContext,
) -> models::casper::DeployDataProto {
    let installer_secret = genesis.genesis_vaults[0].0.clone();
    let gateway_public = &genesis.genesis_vaults[3].1;
    let installer_source = r#"new entry, slot, entryAddressCh, slotAddressCh,
      VaultAddress(`rho:vault:address`), DeployerIdOps(`rho:system:deployerId:ops`) in {
      for (@request, deployerId <= @"agent-trigger") {
        new publicKeyCh in {
          DeployerIdOps!("pubKeyBytes", *deployerId, *publicKeyCh) |
          for (@publicKey <- publicKeyCh) {
            if (publicKey == "GATEWAY_PUBLIC_KEY".hexToBytes()) {
              {% for(@accepted <- entry) {
                new x in { x!(0) | for(@0 <- x) { @"agent-ran"!(true) } }
              } %}[entry -o slot] |
              entry!(request)
            }
          }
        }
      } |
      VaultAddress!("fromUnforgeable", *entry, *entryAddressCh) |
      for (@entryAddress <- entryAddressCh) { @"agent-entry-address"!!(entryAddress) } |
      VaultAddress!("fromUnforgeable", *slot, *slotAddressCh) |
      for (@slotAddress <- slotAddressCh) { @"agent-slot-address"!!(slotAddress) }
    }"#
    .replace("GATEWAY_PUBLIC_KEY", &hex::encode(&gateway_public.bytes));
    signed_direct_offer(
        installer_source,
        1,
        0,
        installer_secret,
        &genesis.genesis_vaults[0].1,
    )
}

/// The sponsor's offer that funds the entry and slot vaults with 100,000 each.
fn funding_offer(
    genesis: &crate::util::genesis_builder::GenesisContext,
    entry_address: &VaultAddress,
    slot_address: &VaultAddress,
) -> models::casper::DeployDataProto {
    let sponsor_secret = genesis.genesis_vaults[2].0.clone();
    let sponsor_address = VaultAddress::from_public_key(&genesis.genesis_vaults[2].1).unwrap();
    let funding_source = format!(
        r#"new rl(`rho:registry:lookup`), systemVaultCh, payerCh, authKeyCh,
          transferCh, deployerId(`rho:system:deployerId`) in {{
          rl!(`rho:vault:system`, *systemVaultCh) |
          for (@(_, systemVault) <- systemVaultCh) {{
            @systemVault!("find", "{}", *payerCh) |
            @systemVault!("deployerAuthKey", *deployerId, *authKeyCh) |
            for (@(true, payer) <- payerCh & key <- authKeyCh) {{
              @payer!("transferBatch", [("{}", 100000), ("{}", 100000)], *key, *transferCh) |
              for (@result <- transferCh) {{ @"agent-slot-funded"!(result) }}
            }}
          }}
        }}"#,
        sponsor_address.to_base58(),
        entry_address.to_base58(),
        slot_address.to_base58(),
    );
    signed_direct_offer(
        funding_source,
        2,
        1,
        sponsor_secret,
        &genesis.genesis_vaults[2].1,
    )
}

/// The entry and slot vault addresses that the installer published at `root`.
async fn installed_addresses(
    nodes: &[TestNode],
    root: &models::rust::block::state_hash::StateHash,
) -> (VaultAddress, VaultAddress) {
    let entry_address = VaultAddress::parse(
        &RhoString::unapply(&data(nodes, root, "agent-entry-address").await[0]).unwrap(),
    )
    .unwrap();
    let slot_address = VaultAddress::parse(
        &RhoString::unapply(&data(nodes, root, "agent-slot-address").await[0]).unwrap(),
    )
    .unwrap();
    (entry_address, slot_address)
}

/// DR-102: the installer block and the gateway funding block each charge the
/// same replay usage on the producer and on every validator, and each role
/// charges its own acceptance work to a separate budget. The funding block
/// needs the Phase D caps.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn offered_replay_usage_is_identical_across_roles() {
    let (genesis, mut nodes) = gateway_network().await;
    let installed = crate::helper::offered_replay_usage::propose_offer_with_identical_replay_usage(
        &mut nodes,
        installer_offer(&genesis),
        "installer",
    )
    .await;
    let (entry_address, slot_address) =
        installed_addresses(&nodes, &installed.body.state.post_state_hash).await;
    let funded = crate::helper::offered_replay_usage::propose_offer_with_identical_replay_usage(
        &mut nodes,
        funding_offer(&genesis, &entry_address, &slot_address),
        "funding",
    )
    .await;
    let funded_root = &funded.body.state.post_state_hash;
    assert_eq!(balance(&nodes, funded_root, &entry_address).await, 100_000);
    assert_eq!(balance(&nodes, funded_root, &slot_address).await, 100_000);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn offered_gateway_call_draws_funded_private_purses_across_validators() {
    let (genesis, mut nodes) = gateway_network().await;
    let installer_secret = genesis.genesis_vaults[0].0.clone();
    let gateway_secret = genesis.genesis_vaults[3].0.clone();
    let gateway_public = &genesis.genesis_vaults[3].1;
    let installed = propagate_offer(&mut nodes, installer_offer(&genesis), "installer").await;
    let installed_root = &installed.body.state.post_state_hash;
    let (entry_address, slot_address) = installed_addresses(&nodes, installed_root).await;
    let continuations = nodes[0]
        .runtime_manager
        .get_continuation(installed_root.clone(), vec![new_gstring_par(
            "agent-trigger".to_owned(),
            Vec::new(),
            false,
        )])
        .await
        .unwrap();
    let mut private = Vec::new();
    for (_, body) in &continuations {
        collect_private_in_par(body, &mut private);
    }
    let entry_signature = private
        .iter()
        .find(|signature| vault_payer(signature).unwrap().address == entry_address)
        .cloned()
        .expect("installed entry authority is rooted");
    let slot_signature = private
        .iter()
        .find(|signature| vault_payer(signature).unwrap().address == slot_address)
        .cloned()
        .expect("installed slot authority is rooted");
    let funding = funding_offer(&genesis, &entry_address, &slot_address);
    let funded = propagate_offer(&mut nodes, funding, "funding").await;
    let funded_root = &funded.body.state.post_state_hash;
    assert_eq!(balance(&nodes, funded_root, &entry_address).await, 100_000);
    assert_eq!(balance(&nodes, funded_root, &slot_address).await, 100_000);
    let unauthorized = signed_direct_offer(
        "new deployerId(`rho:system:deployerId`) in { @\"agent-trigger\"!(0, *deployerId) }"
            .to_owned(),
        3,
        2,
        installer_secret,
        &genesis.genesis_vaults[0].1,
    );
    let rejected = propagate_offer(&mut nodes, unauthorized, "unauthorized").await;
    let rejected_root = &rejected.body.state.post_state_hash;
    assert!(data(&nodes, rejected_root, "agent-ran").await.is_empty());
    assert_eq!(
        balance(&nodes, rejected_root, &entry_address).await,
        100_000
    );
    assert_eq!(balance(&nodes, rejected_root, &slot_address).await, 100_000);

    let offer = offered_gateway_call(
        gateway_secret,
        gateway_public,
        entry_signature,
        slot_signature,
    );
    let offer_id: block_storage::rust::dag::block_dag_key_value_storage::DeployId =
        offer.deploy_id.to_vec();
    BlockAPI::deploy_offered(&nodes[0].engine_cell, offer, &None, false, "root")
        .await
        .unwrap();
    let block = nodes[0].create_block_unsafe(&[]).await.unwrap();
    assert_eq!(block.body.deploys.len(), 1);
    assert_eq!(block.body.deploys[0].identity_bytes(), offer_id.as_slice());
    for node in &mut nodes {
        assert!(matches!(
            node.process_block(block.clone()).await.unwrap(),
            Either::Right(_)
        ));
    }
    let root = &block.body.state.post_state_hash;
    assert_eq!(
        RhoBoolean::unapply(&data(&nodes, root, "agent-ran").await[0]),
        Some(true)
    );
    let producer = BlockAPI::find_offered_settlement_receipt(&nodes[0].engine_cell, &offer_id)
        .await
        .unwrap()
        .unwrap();
    let validator = BlockAPI::find_offered_settlement_receipt(&nodes[1].engine_cell, &offer_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(producer, validator);
    assert_eq!(producer.purses.len(), 3);
    assert_eq!(
        producer
            .purses
            .iter()
            .map(|purse| purse.fee_rev)
            .sum::<u128>(),
        1
    );
    assert_eq!(
        producer
            .purses
            .iter()
            .map(|purse| purse.resource_rev)
            .sum::<u128>(),
        producer.rev_spent - producer.fee_rev
    );
    for address in [&entry_address, &slot_address] {
        let purse = producer
            .purses
            .iter()
            .find(|purse| purse.address == address.to_base58().as_bytes())
            .expect("funded private purse is settled");
        assert!(purse.resource_rev > 0);
        assert_eq!(purse.fee_rev, 0);
        assert!(purse.post_balance < 100_000);
        assert_eq!(balance(&nodes, root, address).await, purse.post_balance);
    }
    assert_eq!(producer.fee_rev, 1);
    assert!(producer.fresh_phlo <= producer.phlo_used);
    assert!(
        u128::from(producer.phlo_used) + u128::from(producer.retained_phlo)
            <= u128::from(producer.phlo_limit)
    );
    assert_eq!(
        producer.rev_spent,
        (u128::from(producer.fresh_phlo) + u128::from(producer.retained_phlo))
            * u128::from(producer.phlo_price)
            + producer.fee_rev
    );
    assert_eq!(
        producer.rev_ceiling,
        u128::from(producer.phlo_limit) * u128::from(producer.phlo_price) + producer.fee_rev
    );
    assert!(producer.rev_spent <= producer.rev_ceiling);
}
