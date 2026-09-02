// See casper/src/test/scala/coop/rchain/casper/util/rholang/RuntimeManagerTest.scala

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use block_storage::rust::dag::block_dag_key_value_storage::InsertMode;
use casper::rust::errors::CasperError;
use casper::rust::rholang::replay_runtime::ReplayRuntimeOps;
use casper::rust::rholang::runtime::RuntimeOps;
use casper::rust::slashing_authorization::CanonicalSlashAuthority;
use casper::rust::util::construct_deploy;
use casper::rust::util::rholang::costacc::check_balance::CheckBalance;
use casper::rust::util::rholang::costacc::close_block_deploy::CloseBlockDeploy;
use casper::rust::util::rholang::costacc::redeem_deploy::{RedeemDeploy, RedemptionOutcome};
use casper::rust::util::rholang::costacc::slash_deploy::SlashDeploy;
use casper::rust::util::rholang::costacc::vault_cost_deploy::{
    ApplyCostDeploy, ProtocolBurnDeploy, ProtocolMintDeploy, VaultAllocation, VaultSettlement,
};
use casper::rust::util::rholang::costacc::vault_payer::{balance_query_source, vault_payer};
use casper::rust::util::rholang::replay_failure::ReplayFailure;
use casper::rust::util::rholang::runtime_manager::RuntimeManager;
use casper::rust::util::rholang::system_deploy::SystemDeployTrait;
use casper::rust::util::rholang::system_deploy_result::SystemDeployResult;
use casper::rust::util::rholang::system_deploy_user_error::SystemDeployUserError;
use casper::rust::util::rholang::{acceptance, supply, system_deploy_util};
use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use crypto::rust::private_key::PrivateKey;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::{Cosigned, Signed};
use models::rhoapi::{CostSignature, CostStack, ListParWithRandom, PCost, Par};
use models::rust::block::state_hash::StateHash;
use models::rust::bond_generation::BondGeneration;
use models::rust::casper::protocol::casper_message::{
    BlockMessage, Body, DeployData, Event, F1r3flyState, Header, ProcessedDeploy,
    ProcessedSystemDeploy,
};
use models::rust::utils::new_gstring_par;
use rholang::rust::interpreter::accounting::authority::{
    allocate_quantitative_events, authority_demand, sig_to_cost_signature, AuthorityByteEvent,
    AuthorityByteEventKind, ResourceMultiset,
};
use rholang::rust::interpreter::accounting::{self, Sig};
use rholang::rust::interpreter::compiler::compiler::Compiler;
use rholang::rust::interpreter::rho_runtime::RhoRuntime;
use rholang::rust::interpreter::rho_type::{Extractor, RhoBoolean, RhoNumber, RhoString};
use rholang::rust::interpreter::system_processes::BlockData;
use rholang::rust::interpreter::test_utils::par_builder_util::ParBuilderUtil;
use rholang::rust::interpreter::util::vault_address::VaultAddress;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::history_reader::HistoryReader;
use rspace_plus_plus::rspace::history::Either;

use crate::util::genesis_builder::{GenesisBuilder, GenesisContext};
use crate::util::rholang::resources::{self, with_runtime_manager};

enum SystemDeployReplayResult<A> {
    ReplaySucceeded {
        state_hash: StateHash,
        result: A,
    },
    ReplayFailed {
        system_deploy_error: SystemDeployUserError,
    },
}

fn protocol_v6_envelope(
    deploy: Signed<DeployData>,
    private_key: PrivateKey,
) -> Cosigned<DeployData> {
    let mut data = deploy.data;
    if data.shard_id.is_empty() {
        data.shard_id = "root".to_string();
    }
    Cosigned::create_single_envelope(data, deploy.sig_algorithm, private_key)
        .expect("protocol-v6 test envelope")
}

async fn system_vault_balance(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
    address: &VaultAddress,
) -> i64 {
    let (values, _) = runtime_manager
        .play_exploratory_deploy(balance_query_source(address), state_hash, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    RhoNumber::unapply(&values[0]).unwrap()
}

async fn pos_validator_bond(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
    validator: &crypto::rust::public_key::PublicKey,
) -> i64 {
    let source = format!(
        r#"
        new return, poSCh, bondsCh,
            rl(`rho:registry:lookup`)
        in {{
          rl!(`rho:system:pos`, *poSCh) |
          for (@(_, PoS) <- poSCh) {{
            @PoS!("getBonds", *bondsCh) |
            for (@bonds <- bondsCh) {{
              return!(bonds.getOrElse("{}".hexToBytes(), -1))
            }}
          }}
        }}"#,
        hex::encode(&validator.bytes)
    );
    let (values, _) = runtime_manager
        .play_exploratory_deploy(source, state_hash, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    RhoNumber::unapply(&values[0]).unwrap()
}

async fn pos_quarantined_stake(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
    validator: &crypto::rust::public_key::PublicKey,
) -> i64 {
    let source = format!(
        r#"
        new return, poSCh, quarantineCh,
            rl(`rho:registry:lookup`)
        in {{
          rl!(`rho:system:pos`, *poSCh) |
          for (@(_, PoS) <- poSCh) {{
            @PoS!("getQuarantinedStake", *quarantineCh) |
            for (@quarantine <- quarantineCh) {{
              match quarantine.getOrElse("{}".hexToBytes(), Nil) {{
                (bond, _, _, _, _, _) => {{ return!(bond) }}
                Nil => {{ return!(-1) }}
              }}
            }}
          }}
        }}"#,
        hex::encode(&validator.bytes)
    );
    let (values, _) = runtime_manager
        .play_exploratory_deploy(source, state_hash, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    RhoNumber::unapply(&values[0]).unwrap()
}

async fn pos_quarantined_reward(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
    validator: &crypto::rust::public_key::PublicKey,
) -> i64 {
    let source = format!(
        r#"
        new return, poSCh, quarantineCh,
            rl(`rho:registry:lookup`)
        in {{
          rl!(`rho:system:pos`, *poSCh) |
          for (@(_, PoS) <- poSCh) {{
            @PoS!("getQuarantinedStake", *quarantineCh) |
            for (@quarantine <- quarantineCh) {{
              match quarantine.getOrElse("{}".hexToBytes(), Nil) {{
                (_, reward, _, _, _, _) => {{ return!(reward) }}
                Nil => {{ return!(-1) }}
              }}
            }}
          }}
        }}"#,
        hex::encode(&validator.bytes)
    );
    let (values, _) = runtime_manager
        .play_exploratory_deploy(source, state_hash, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    RhoNumber::unapply(&values[0]).unwrap()
}

async fn pos_validator_reward(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
    validator: &crypto::rust::public_key::PublicKey,
) -> i64 {
    let source = format!(
        r#"
        new return, poSCh, rewardsCh,
            rl(`rho:registry:lookup`)
        in {{
          rl!(`rho:system:pos`, *poSCh) |
          for (@(_, PoS) <- poSCh) {{
            @PoS!("getRewards", *rewardsCh) |
            for (@rewards <- rewardsCh) {{
              return!(rewards.getOrElse("{}".hexToBytes(), 0))
            }}
          }}
        }}"#,
        hex::encode(&validator.bytes)
    );
    let (values, _) = runtime_manager
        .play_exploratory_deploy(source, state_hash, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    RhoNumber::unapply(&values[0]).unwrap()
}

async fn pos_validator_locked_stake(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
    validator: &crypto::rust::public_key::PublicKey,
) -> i64 {
    let source = format!(
        r#"
        new return, poSCh, bondsCh, withdrawersCh, quarantineCh,
            rl(`rho:registry:lookup`)
        in {{
          rl!(`rho:system:pos`, *poSCh) |
          for (@(_, PoS) <- poSCh) {{
            @PoS!("getBonds", *bondsCh) |
            @PoS!("getWithdrawers", *withdrawersCh) |
            @PoS!("getQuarantinedStake", *quarantineCh) |
            for (@bonds <- bondsCh & @withdrawers <- withdrawersCh & @quarantine <- quarantineCh) {{
              match quarantine.getOrElse("{}".hexToBytes(), Nil) {{
                (bond, _, _, _, _, _) => {{ return!(bond) }}
                Nil => {{
                  match withdrawers.getOrElse("{}".hexToBytes(), Nil) {{
                    (bond, _, _) => {{ return!(bond) }}
                    Nil => {{ return!(bonds.getOrElse("{}".hexToBytes(), -1)) }}
                  }}
                }}
              }}
            }}
          }}
        }}"#,
        hex::encode(&validator.bytes),
        hex::encode(&validator.bytes),
        hex::encode(&validator.bytes)
    );
    let (values, _) = runtime_manager
        .play_exploratory_deploy(source, state_hash, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    RhoNumber::unapply(&values[0]).unwrap()
}

async fn pos_validator_lifecycle(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
    validator: &crypto::rust::public_key::PublicKey,
) -> String {
    let source = format!(
        r#"
        new return, poSCh, bondsCh, pendingCh, withdrawersCh, quarantineCh,
            rl(`rho:registry:lookup`)
        in {{
          rl!(`rho:system:pos`, *poSCh) |
          for (@(_, PoS) <- poSCh) {{
            @PoS!("getBonds", *bondsCh) |
            @PoS!("getPendingWithdrawer", *pendingCh) |
            @PoS!("getWithdrawers", *withdrawersCh) |
            @PoS!("getQuarantinedStake", *quarantineCh) |
            for (@bonds <- bondsCh & @pending <- pendingCh &
                 @withdrawers <- withdrawersCh & @quarantine <- quarantineCh) {{
              if (quarantine.contains("{}".hexToBytes())) {{
                match quarantine.get("{}".hexToBytes()) {{
                  (_, _, _, origin, _, _) => {{ return!("Quarantined:" ++ origin) }}
                }}
              }} else if (pending.contains("{}".hexToBytes())) {{
                return!("PendingWithdraw")
              }} else if (withdrawers.contains("{}".hexToBytes())) {{
                return!("Withdrawing")
              }} else if (bonds.contains("{}".hexToBytes())) {{
                return!("Bonded")
              }} else {{
                return!("Absent")
              }}
            }}
          }}
        }}"#,
        hex::encode(&validator.bytes),
        hex::encode(&validator.bytes),
        hex::encode(&validator.bytes),
        hex::encode(&validator.bytes),
        hex::encode(&validator.bytes)
    );
    let (values, _) = runtime_manager
        .play_exploratory_deploy(source, state_hash, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    RhoString::unapply(&values[0]).unwrap()
}

async fn pos_validator_lifecycle_deadline(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
    validator: &crypto::rust::public_key::PublicKey,
) -> i64 {
    let source = format!(
        r#"
        new return, poSCh, pendingCh, withdrawersCh, quarantineCh,
            rl(`rho:registry:lookup`)
        in {{
          rl!(`rho:system:pos`, *poSCh) |
          for (@(_, PoS) <- poSCh) {{
            @PoS!("getPendingWithdrawer", *pendingCh) |
            @PoS!("getWithdrawers", *withdrawersCh) |
            @PoS!("getQuarantinedStake", *quarantineCh) |
            for (@pending <- pendingCh & @withdrawers <- withdrawersCh & @quarantine <- quarantineCh) {{
              if (quarantine.contains("{}".hexToBytes())) {{
                match quarantine.get("{}".hexToBytes()) {{
                  (_, _, _, _, deadline, _) => {{ return!(deadline) }}
                }}
              }} else if (pending.contains("{}".hexToBytes())) {{
                match pending.get("{}".hexToBytes()) {{
                  (deadline, _) => {{ return!(deadline) }}
                }}
              }} else if (withdrawers.contains("{}".hexToBytes())) {{
                match withdrawers.get("{}".hexToBytes()) {{
                  (_, deadline, _) => {{ return!(deadline) }}
                }}
              }} else {{
                return!(-1)
              }}
            }}
          }}
        }}"#,
        hex::encode(&validator.bytes),
        hex::encode(&validator.bytes),
        hex::encode(&validator.bytes),
        hex::encode(&validator.bytes),
        hex::encode(&validator.bytes),
        hex::encode(&validator.bytes)
    );
    let (values, _) = runtime_manager
        .play_exploratory_deploy(source, state_hash, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    RhoNumber::unapply(&values[0]).unwrap()
}

async fn pos_validator_lifecycle_generation(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
    validator: &crypto::rust::public_key::PublicKey,
) -> i64 {
    let source = format!(
        r#"
        new return, poSCh, generationsCh,
            rl(`rho:registry:lookup`)
        in {{
          rl!(`rho:system:pos`, *poSCh) |
          for (@(_, PoS) <- poSCh) {{
            @PoS!("getBondGenerations", *generationsCh) |
            for (@generations <- generationsCh) {{
              return!(generations.getOrElse("{}".hexToBytes(), -1))
            }}
          }}
        }}"#,
        hex::encode(&validator.bytes)
    );
    let (values, _) = runtime_manager
        .play_exploratory_deploy(source, state_hash, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    RhoNumber::unapply(&values[0]).unwrap()
}

async fn pos_validator_is_active(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
    validator: &crypto::rust::public_key::PublicKey,
) -> bool {
    let source = format!(
        r#"
        new return, poSCh, activeCh,
            rl(`rho:registry:lookup`)
        in {{
          rl!(`rho:system:pos`, *poSCh) |
          for (@(_, PoS) <- poSCh) {{
            @PoS!("getActiveValidators", *activeCh) |
            for (@active <- activeCh) {{
              return!(active.contains("{}".hexToBytes()))
            }}
          }}
        }}"#,
        hex::encode(&validator.bytes)
    );
    let (values, _) = runtime_manager
        .play_exploratory_deploy(source, state_hash, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    RhoBoolean::unapply(&values[0]).unwrap()
}

async fn pos_coop_vault_address(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
) -> VaultAddress {
    let source = r#"
        new return, poSCh, coopCh,
            rl(`rho:registry:lookup`)
        in {
          rl!(`rho:system:pos`, *poSCh) |
          for (@(_, PoS) <- poSCh) {
            @PoS!("getCoopVault", *coopCh) |
            for (@(_, coopAddress, _) <- coopCh) {
              return!(coopAddress)
            }
          }
        }"#;
    let (values, _) = runtime_manager
        .play_exploratory_deploy(source.to_string(), state_hash, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    VaultAddress::parse(&RhoString::unapply(&values[0]).unwrap()).unwrap()
}

async fn pos_stake_vault_address(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
) -> VaultAddress {
    let source = r#"
        new return, poSCh, vaultCh,
            rl(`rho:registry:lookup`)
        in {
          rl!(`rho:system:pos`, *poSCh) |
          for (@(_, PoS) <- poSCh) {
            @PoS!("getInitialPosVault", *vaultCh) |
            for (@(vaultAddress, _) <- vaultCh) {
              return!(vaultAddress)
            }
          }
        }"#;
    let (values, _) = runtime_manager
        .play_exploratory_deploy(source.to_string(), state_hash, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    VaultAddress::parse(&RhoString::unapply(&values[0]).unwrap()).unwrap()
}

async fn pos_epoch_length(runtime_manager: &RuntimeManager, state_hash: &StateHash) -> i64 {
    let source = r#"
        new return, poSCh, epochCh,
            rl(`rho:registry:lookup`)
        in {
          rl!(`rho:system:pos`, *poSCh) |
          for (@(_, PoS) <- poSCh) {
            @PoS!("getEpochLength", *epochCh) |
            for (@epochLength <- epochCh) {
              return!(epochLength)
            }
          }
        }"#;
    let (values, _) = runtime_manager
        .play_exploratory_deploy(source.to_string(), state_hash, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    RhoNumber::unapply(&values[0]).unwrap()
}

fn pos_vault_transfer_source(target: &VaultAddress, result_channel: &str) -> String {
    format!(
        r#"
        new transferResult, poSCh,
            rl(`rho:registry:lookup`),
            deployerId(`rho:system:deployerId`)
        in {{
          rl!(`rho:system:pos`, *poSCh) |
          for (@(_, PoS) <- poSCh) {{
            @PoS!("posVaultTransfer", "{}", 1, *deployerId, *transferResult) |
            for (@(accepted, _) <- transferResult) {{
              @"{}"!(accepted)
            }}
          }}
        }}"#,
        target.to_base58(),
        result_channel
    )
}

async fn stored_boolean(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
    channel: &str,
) -> bool {
    let source =
        format!(r#"new return in {{ for (@value <<- @"{channel}") {{ return!(value) }} }}"#);
    let (values, _) = runtime_manager
        .play_exploratory_deploy(source, state_hash, None)
        .await
        .unwrap();
    assert_eq!(values.len(), 1);
    RhoBoolean::unapply(&values[0]).unwrap()
}

fn successful_system_state(result: SystemDeployResult<()>) -> StateHash {
    match result {
        SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
        SystemDeployResult::PlayFailed { .. } => panic!("system deploy unexpectedly failed"),
    }
}

async fn protocol_mint_to_vault(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
    address: &VaultAddress,
    amount: i64,
    seed: u8,
) -> StateHash {
    let runtime = runtime_manager.spawn_runtime().await;
    let mut ops = RuntimeOps::new(runtime);
    successful_system_state(
        ops.play_system_deploy(
            state_hash,
            &mut ProtocolMintDeploy::new(
                address.to_base58(),
                amount,
                Blake2b512Random::create_from_bytes(&[seed]),
            )
            .unwrap(),
        )
        .await
        .unwrap(),
    )
}

async fn measured_authority_cost(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
    deploy: &crypto::rust::signatures::signed::Cosigned<DeployData>,
) -> (u64, u64, u64) {
    let runtime = runtime_manager.spawn_runtime().await;
    let mut ops = RuntimeOps::new(runtime);
    ops.runtime
        .reset(&Blake2b256Hash::from_bytes_prost(state_hash))
        .await
        .unwrap();
    let (processed, _) = ops.process_deploy_cosigned(deploy.clone()).await.unwrap();
    assert!(!processed.is_failed);
    let witness = processed.authority_cost_witness.as_ref().unwrap();
    let physical = witness
        .events
        .iter()
        .flat_map(|event| &event.debit)
        .try_fold(0_u64, |total, resource| total.checked_add(resource.amount))
        .unwrap();
    let byte_events = witness
        .byte_events
        .iter()
        .map(|event| AuthorityByteEvent {
            event_id: event.event_id.as_ref().try_into().unwrap(),
            kind: AuthorityByteEventKind::from_tag(u8::try_from(event.kind).unwrap()).unwrap(),
            authority: event.authority.clone().unwrap(),
            amount: event.amount,
        })
        .collect::<Vec<_>>();
    let upper_bound = byte_events
        .iter()
        .try_fold(0_u64, |total, event| {
            let region_count = authority_demand(&event.authority)?
                .0
                .values()
                .try_fold(0_u64, |sum, amount| sum.checked_add(*amount))
                .ok_or(
                    rholang::rust::interpreter::accounting::authority::AuthorityError::ArithmeticOverflow,
                )?;
            total
                .checked_add(
                    region_count
                        .checked_mul(event.amount)
                        .ok_or(
                            rholang::rust::interpreter::accounting::authority::AuthorityError::ArithmeticOverflow,
                        )?,
                )
                .ok_or(
                    rholang::rust::interpreter::accounting::authority::AuthorityError::ArithmeticOverflow,
                )
        })
        .unwrap();
    let payer = accounting::funding_sig(deploy).lane_hash();
    let byte_allocation = allocate_quantitative_events(
        &byte_events,
        &ResourceMultiset::singleton(payer, upper_bound),
    )
    .unwrap();
    assert!(byte_allocation.0.keys().all(|key| *key == payer));
    (physical, witness.byte_cost, byte_allocation.get(&payer))
}

fn exact_vault_funding(physical: u64, bytes: u64) -> i64 {
    i64::try_from(
        physical
            .checked_add(bytes)
            .and_then(|total| total.checked_add(1))
            .unwrap(),
    )
    .unwrap()
}

async fn play_close(
    runtime_manager: &RuntimeManager,
    state_hash: &StateHash,
    block_data: BlockData,
) -> StateHash {
    let runtime = runtime_manager.spawn_runtime().await;
    runtime.set_block_data(block_data.clone()).await;
    let mut ops = RuntimeOps::new(runtime);
    let mut close = CloseBlockDeploy::new(
        system_deploy_util::generate_close_deploy_random_seed_from_pk(
            block_data.sender,
            block_data.seq_num,
        ),
    );
    match ops
        .play_system_deploy(state_hash, &mut close)
        .await
        .unwrap()
    {
        SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
        SystemDeployResult::PlayFailed {
            processed_system_deploy,
        } => panic!("close failed: {processed_system_deploy:?}"),
    }
}

fn recorded_removal_pair_count(events: &[Event]) -> usize {
    events
        .windows(2)
        .filter(|pair| {
            matches!(
                (&pair[0], &pair[1]),
                (Event::Consume(consume), Event::Comm(comm)) if consume == &comm.consume
            )
        })
        .count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn system_vault_atomic_cost_application_is_conservative_and_rolls_back() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let address_a =
                VaultAddress::from_public_key(&genesis_context.genesis_vaults[0].1).unwrap();
            let address_b =
                VaultAddress::from_public_key(&genesis_context.genesis_vaults[1].1).unwrap();
            let fee_address =
                VaultAddress::from_unforgeable(&models::rhoapi::GPrivate { id: vec![0xD4; 32] });
            let genesis_state = genesis_block.body.state.post_state_hash.clone();
            let initial_a =
                system_vault_balance(&runtime_manager, &genesis_state, &address_a).await;
            let initial_b =
                system_vault_balance(&runtime_manager, &genesis_state, &address_b).await;
            assert_eq!(
                system_vault_balance(&runtime_manager, &genesis_state, &fee_address).await,
                0
            );

            let runtime = runtime_manager.spawn_runtime().await;
            runtime
                .set_block_data(BlockData {
                    time_stamp: 11,
                    block_number: 7,
                    sender: genesis_context.validator_pks()[0].clone(),
                    seq_num: 3,
                })
                .await;
            let mut runtime_ops = RuntimeOps::new(runtime);
            let settled_state = successful_system_state(
                runtime_ops
                    .play_system_deploy(
                        &genesis_state,
                        &mut ApplyCostDeploy::new(
                            [0x31; 32],
                            vec![
                                VaultAllocation::new(address_b.to_base58(), 200).unwrap(),
                                VaultAllocation::new(address_a.to_base58(), 100).unwrap(),
                            ],
                            vec![
                                VaultSettlement::new(address_b.to_base58(), 120, 20).unwrap(),
                                VaultSettlement::new(address_a.to_base58(), 60, 10).unwrap(),
                            ],
                            fee_address.to_base58(),
                            Blake2b512Random::create_from_bytes(&[0x31]),
                        )
                        .unwrap(),
                    )
                    .await
                    .unwrap(),
            );
            let settled_a =
                system_vault_balance(&runtime_manager, &settled_state, &address_a).await;
            let settled_b =
                system_vault_balance(&runtime_manager, &settled_state, &address_b).await;
            let settled_fee =
                system_vault_balance(&runtime_manager, &settled_state, &fee_address).await;
            assert_eq!(settled_a, initial_a - 70);
            assert_eq!(settled_b, initial_b - 140);
            assert_eq!(settled_fee, 30);
            assert_eq!(
                settled_a + settled_b + settled_fee,
                initial_a + initial_b - 180
            );

            let minted_state = successful_system_state(
                runtime_ops
                    .play_system_deploy(
                        &settled_state,
                        &mut ProtocolMintDeploy::new(
                            fee_address.to_base58(),
                            50,
                            Blake2b512Random::create_from_bytes(&[0x35]),
                        )
                        .unwrap(),
                    )
                    .await
                    .unwrap(),
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &minted_state, &fee_address).await,
                80
            );

            let (low_address, low_balance, high_address, high_balance) =
                if address_a.to_base58() < address_b.to_base58() {
                    (address_a.clone(), settled_a, address_b.clone(), settled_b)
                } else {
                    (address_b.clone(), settled_b, address_a.clone(), settled_a)
                };
            let failed_application = runtime_ops
                .play_system_deploy(
                    &minted_state,
                    &mut ApplyCostDeploy::new(
                        [0x41; 32],
                        vec![
                            VaultAllocation::new(low_address.to_base58(), 10).unwrap(),
                            VaultAllocation::new(high_address.to_base58(), high_balance + 1)
                                .unwrap(),
                        ],
                        vec![
                            VaultSettlement::new(low_address.to_base58(), 5, 0).unwrap(),
                            VaultSettlement::new(high_address.to_base58(), 1, 0).unwrap(),
                        ],
                        fee_address.to_base58(),
                        Blake2b512Random::create_from_bytes(&[0x41]),
                    )
                    .unwrap(),
                )
                .await
                .unwrap();
            assert!(matches!(
                failed_application,
                SystemDeployResult::PlayFailed { .. }
            ));
            let failed_application_state = runtime_ops
                .runtime
                .create_checkpoint()
                .await
                .root
                .to_bytes_prost();
            assert_eq!(
                system_vault_balance(&runtime_manager, &failed_application_state, &low_address)
                    .await,
                low_balance
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &failed_application_state, &high_address)
                    .await,
                high_balance
            );
        },
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pos_vault_human_control_uses_the_authenticated_genesis_deployer_and_replays() {
    use casper::rust::genesis::contracts::standard_deploys::{
        POS_GENERATOR_PK, POS_GENERATOR_PUB_KEY,
    };

    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let genesis_state = genesis_block.body.state.post_state_hash.clone();
            let pos_payer = VaultAddress::from_public_key(&POS_GENERATOR_PUB_KEY).unwrap();
            let target =
                VaultAddress::from_public_key(&genesis_context.genesis_vaults[1].1).unwrap();
            let funded_state =
                protocol_mint_to_vault(&runtime_manager, &genesis_state, &pos_payer, 1_000_000, 71)
                    .await;
            let initial_pos_balance = system_vault_balance(
                &runtime_manager,
                &funded_state,
                &pos_stake_vault_address(&runtime_manager, &funded_state).await,
            )
            .await;
            let initial_target_balance =
                system_vault_balance(&runtime_manager, &funded_state, &target).await;

            let unauthorized = construct_deploy::source_deploy(
                pos_vault_transfer_source(&target, "pos-vault-transfer-unauthorized"),
                71,
                None,
                None,
                Some(genesis_context.genesis_vaults[0].0.clone()),
                None,
                Some("root".to_string()),
            )
            .unwrap();
            let unauthorized_block_data = BlockData {
                time_stamp: unauthorized.data.time_stamp,
                block_number: 1,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 1,
            };
            let (unauthorized_state, unauthorized_processed, unauthorized_system) = runtime_manager
                .compute_state(
                    &funded_state,
                    vec![unauthorized],
                    Vec::new(),
                    unauthorized_block_data.clone(),
                    None,
                )
                .await
                .unwrap();
            assert!(!unauthorized_processed[0].is_failed);
            assert!(
                !stored_boolean(
                    &runtime_manager,
                    &unauthorized_state,
                    "pos-vault-transfer-unauthorized"
                )
                .await
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &unauthorized_state, &target).await,
                initial_target_balance
            );
            assert_eq!(
                runtime_manager
                    .replay_compute_state(
                        &funded_state,
                        unauthorized_processed,
                        unauthorized_system,
                        &unauthorized_block_data,
                        None,
                        false,
                    )
                    .await
                    .unwrap(),
                unauthorized_state
            );

            let pos_key = PrivateKey::from_bytes(
                &hex::decode(POS_GENERATOR_PK).expect("invalid PoS generator private key"),
            );
            let authorized = construct_deploy::source_deploy(
                pos_vault_transfer_source(&target, "pos-vault-transfer-authorized"),
                72,
                None,
                None,
                Some(pos_key),
                None,
                Some("root".to_string()),
            )
            .unwrap();
            let authorized_block_data = BlockData {
                time_stamp: authorized.data.time_stamp,
                block_number: 2,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 2,
            };
            let (authorized_state, authorized_processed, authorized_system) = runtime_manager
                .compute_state(
                    &unauthorized_state,
                    vec![authorized],
                    Vec::new(),
                    authorized_block_data.clone(),
                    None,
                )
                .await
                .unwrap();
            assert!(!authorized_processed[0].is_failed);
            assert!(
                stored_boolean(
                    &runtime_manager,
                    &authorized_state,
                    "pos-vault-transfer-authorized"
                )
                .await
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &authorized_state, &target).await,
                initial_target_balance + 1
            );
            let pos_vault = pos_stake_vault_address(&runtime_manager, &authorized_state).await;
            assert_eq!(
                system_vault_balance(&runtime_manager, &authorized_state, &pos_vault).await,
                initial_pos_balance - 1
            );
            assert_eq!(
                runtime_manager
                    .replay_compute_state(
                        &unauthorized_state,
                        authorized_processed,
                        authorized_system,
                        &authorized_block_data,
                        None,
                        false,
                    )
                    .await
                    .unwrap(),
                authorized_state
            );
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stack_settlement_preserves_balance_and_releases_the_tail() {
    with_runtime_manager(
        |runtime_manager, _genesis_context, genesis_block| async move {
            use models::rhoapi::cost_signature::Value;

            let head = CostSignature {
                value: Some(Value::Ground(b"stack-head".to_vec())),
            };
            let tail = CostSignature {
                value: Some(Value::Ground(b"stack-tail".to_vec())),
            };
            let head_channel = supply::supply_channel(&Sig::Ground(b"stack-head".to_vec()));
            let tail_channel = supply::supply_channel(&Sig::Ground(b"stack-tail".to_vec()));
            let final_cell = CostSignature {
                value: Some(Value::Ground(b"final-cell".to_vec())),
            };
            let final_channel = supply::supply_channel(&Sig::Ground(b"final-cell".to_vec()));
            let runtime = runtime_manager.spawn_runtime().await;
            let mut runtime_ops = RuntimeOps::new(runtime);
            runtime_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(
                    &genesis_block.body.state.post_state_hash,
                ))
                .await
                .unwrap();
            runtime_ops
                .runtime
                .reducer
                .space
                .produce(
                    final_channel.clone(),
                    ListParWithRandom {
                        pars: Vec::new(),
                        random_state: vec![9; 64],
                        cost_authority: None,
                        cost_stack: Some(CostStack {
                            cells: vec![final_cell.clone()],
                        }),
                    },
                    false,
                )
                .await
                .unwrap();
            runtime_ops
                .runtime
                .reducer
                .space
                .produce(
                    head_channel.clone(),
                    ListParWithRandom {
                        pars: Vec::new(),
                        random_state: vec![7; 64],
                        cost_authority: None,
                        cost_stack: Some(CostStack {
                            cells: vec![head.clone(), tail.clone()],
                        }),
                    },
                    false,
                )
                .await
                .unwrap();
            let pre_settlement = runtime_ops.runtime.create_checkpoint().await;

            let inventory = supply::decode_purse_inventory(
                &runtime_ops.get_data_datums(&head_channel).await,
                &head,
            )
            .unwrap();
            assert_eq!(inventory.balance, None);
            assert_eq!(inventory.stacks.len(), 1);
            let stack_id = inventory.stacks[0].instance_id;
            let final_inventory = supply::decode_purse_inventory(
                &runtime_ops.get_data_datums(&final_channel).await,
                &final_cell,
            )
            .unwrap();
            assert_eq!(final_inventory.stacks.len(), 1);
            let final_stack_id = final_inventory.stacks[0].instance_id;
            let stacks = inventory
                .stacks
                .iter()
                .chain(final_inventory.stacks.iter())
                .cloned()
                .collect::<Vec<_>>();
            supply::apply_stack_pops(
                &mut runtime_ops,
                &stacks,
                &std::collections::BTreeMap::from([(stack_id, 1), (final_stack_id, 1)]),
            )
            .await
            .unwrap();

            let settlement_log = runtime_ops.runtime.take_event_log().await;
            assert_eq!(settlement_log.len(), 5);
            assert_eq!(
                settlement_log
                    .iter()
                    .filter(|event| matches!(
                        event,
                        rspace_plus_plus::rspace::trace::event::Event::IoEvent(
                            rspace_plus_plus::rspace::trace::event::IOEvent::Consume(_)
                        )
                    ))
                    .count(),
                2
            );
            assert_eq!(
                settlement_log
                    .iter()
                    .filter(|event| matches!(
                        event,
                        rspace_plus_plus::rspace::trace::event::Event::Comm(_)
                    ))
                    .count(),
                2
            );
            assert_eq!(
                settlement_log
                    .iter()
                    .filter(|event| matches!(
                        event,
                        rspace_plus_plus::rspace::trace::event::Event::IoEvent(
                            rspace_plus_plus::rspace::trace::event::IOEvent::Produce(_)
                        )
                    ))
                    .count(),
                1
            );
            let post_settlement = runtime_ops.runtime.create_checkpoint().await;
            let history = runtime_manager.get_history_repo();
            let pre_reader = history
                .get_history_reader_struct(&pre_settlement.root)
                .unwrap();
            let post_reader = history
                .get_history_reader_struct(&post_settlement.root)
                .unwrap();
            let event_index = rspace_plus_plus::rspace::merger::event_log_index::EventLogIndex::new(
                settlement_log,
                |produce| {
                    pre_reader
                        .get_data(&produce.channel_hash)
                        .is_ok_and(|data| data.iter().any(|datum| datum.source == *produce))
                },
                |_| false,
                std::collections::BTreeMap::new(),
            );
            let state_change = rspace_plus_plus::rspace::merger::state_change::StateChange::new(
                pre_reader,
                post_reader,
                &event_index,
            )
            .unwrap();
            let head_hash =
                rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(&head_channel);
            let head_change = state_change.datums_changes.get(&head_hash).unwrap();
            assert_eq!(head_change.removed.len(), 1);
            let final_hash =
                rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(&final_channel);
            let final_change = state_change.datums_changes.get(&final_hash).unwrap();
            assert_eq!(final_change.removed.len(), 1);
            assert!(
                rspace_plus_plus::rspace::merger::merging_logic::are_conflicting(
                    &event_index,
                    &event_index
                )
            );

            assert!(runtime_ops.get_data_datums(&final_channel).await.is_empty());
            let tail_inventory = supply::decode_purse_inventory(
                &runtime_ops.get_data_datums(&tail_channel).await,
                &tail,
            )
            .unwrap();
            assert_eq!(tail_inventory.stacks.len(), 1);
            assert_eq!(tail_inventory.stacks[0].stack.cells, vec![tail]);
        },
    )
    .await
    .unwrap();
}

async fn compute_state(
    runtime_manager: &mut RuntimeManager,
    genesis_context: &GenesisContext,
    deploy: Signed<DeployData>,
    state_hash: &StateHash,
) -> (StateHash, ProcessedDeploy, Vec<ProcessedSystemDeploy>) {
    let time_stamp = deploy.data.time_stamp;
    let (new_state_hash, processed_deploys, processed_system_deploys) = runtime_manager
        .compute_state(
            state_hash,
            vec![deploy],
            Vec::new(), // No system deploys
            BlockData {
                time_stamp,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            },
            None,
        )
        .await
        .unwrap();

    let result = processed_deploys.into_iter().next().unwrap();
    (new_state_hash, result, processed_system_deploys)
}

async fn replay_compute_state(
    runtime_manager: &mut RuntimeManager,
    genesis_context: &GenesisContext,
    processed_deploy: ProcessedDeploy,
    processed_system_deploys: Vec<ProcessedSystemDeploy>,
    state_hash: &StateHash,
) -> Result<StateHash, CasperError> {
    let time_stamp = processed_deploy.deploy.data.time_stamp;
    runtime_manager
        .replay_compute_state(
            state_hash,
            vec![processed_deploy],
            processed_system_deploys,
            &BlockData {
                time_stamp,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            },
            None,
            false,
        )
        .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn comput_state_should_charge_for_deploys() {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let gen_post_state = genesis_block.body.state.post_state_hash;
            let source = r#"
            new rl(`rho:registry:lookup`), listOpsCh in {
                rl!(`rho:lang:listOps`, *listOpsCh) |
                for(x <- listOpsCh){
                    Nil
                }
            }
            "#;

            // TODO: Prohibit negative gas prices and gas limits in deploys. - OLD
            // TODO: Make minimum maximum yield for deploy parameter of node. - OLD
            let deploy = construct_deploy::source_deploy_now_full(
                source.to_string(),
                Some(100000),
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let (new_state_hash, processed_deploy, processed_system_deploys) = compute_state(
                &mut runtime_manager,
                &genesis_context,
                deploy,
                &gen_post_state,
            )
            .await;

            let caller = Sig::Ground(construct_deploy::DEFAULT_PUB.bytes.to_vec());
            let witness = processed_deploy
                .authority_cost_witness
                .as_ref()
                .expect("admitted user deploy must carry authority evidence");
            let witnessed_signatures = witness
                .events
                .iter()
                .flat_map(|event| {
                    event
                        .authority
                        .as_ref()
                        .expect("authority event must carry its regions")
                        .regions
                        .iter()
                })
                .map(|region| {
                    rholang::rust::interpreter::accounting::authority::cost_signature_to_sig(
                        region
                            .signature
                            .as_ref()
                            .expect("authority region must carry its signature"),
                    )
                    .unwrap()
                })
                .collect::<Vec<_>>();
            assert!(witnessed_signatures.contains(&caller));
            assert!(
                witnessed_signatures
                    .iter()
                    .all(|signature| signature == &Sig::Unit || signature == &caller),
                "blessed contracts must contribute only unit authority to later user interactions"
            );

            let replay_state_hash = replay_compute_state(
                &mut runtime_manager,
                &genesis_context,
                processed_deploy,
                processed_system_deploys,
                &gen_post_state,
            )
            .await
            .unwrap();

            assert_ne!(
                new_state_hash, gen_post_state,
                "terminal settlement must commit the realized cost and fee"
            );
            assert_eq!(
                replay_state_hash, new_state_hash,
                "replay must reproduce play's post-state exactly (no divergence)"
            );
        },
    )
    .await
    .unwrap()
}

async fn compare_successful_system_deploys<S: SystemDeployTrait, F>(
    runtime_manager: &mut RuntimeManager,
    genesis_context: &GenesisContext,
    start_state: &StateHash,
    play_system_deploy: &mut S,
    replay_system_deploy: &mut S,
    result_assertion: F,
) -> Result<StateHash, CasperError>
where
    F: Fn(&S::Result) -> bool,
    <S as SystemDeployTrait>::Result: PartialEq,
{
    let runtime = runtime_manager.spawn_runtime().await;
    {
        runtime
            .set_block_data(BlockData {
                time_stamp: 0,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            })
            .await;
    }

    let mut runtime_ops = RuntimeOps::new(runtime);
    let play_system_result = runtime_ops
        .play_system_deploy(start_state, play_system_deploy)
        .await?;

    match play_system_result {
        SystemDeployResult::PlaySucceeded {
            state_hash: final_play_state_hash,
            processed_system_deploy,
            mergeable_channels: _,
            result: play_result,
        } => {
            result_assertion(&play_result);

            let replay_runtime = runtime_manager.spawn_replay_runtime().await;
            {
                replay_runtime
                    .set_block_data(BlockData {
                        time_stamp: 0,
                        block_number: 0,
                        sender: genesis_context.validator_pks()[0].clone(),
                        seq_num: 0,
                    })
                    .await;
            }

            let replay_runtime_ops = ReplayRuntimeOps::new_from_runtime(replay_runtime);
            let replay_system_result = exec_replay_system_deploy(
                replay_runtime_ops,
                start_state,
                replay_system_deploy,
                &processed_system_deploy,
            )
            .await?;

            match replay_system_result {
                SystemDeployReplayResult::ReplaySucceeded {
                    state_hash: final_replay_state_hash,
                    result: replay_result,
                } => {
                    assert!(final_play_state_hash == final_replay_state_hash);
                    assert!(play_result == replay_result);
                    Ok(final_replay_state_hash)
                }

                SystemDeployReplayResult::ReplayFailed {
                    system_deploy_error,
                } => panic!(
                    "Unexpected user error during replay: {:?}",
                    system_deploy_error
                ),
            }
        }

        SystemDeployResult::PlayFailed {
            processed_system_deploy,
        } => panic!(
            "Unexpected system error during play: {:?}",
            processed_system_deploy
        ),
    }
}

async fn exec_replay_system_deploy<S: SystemDeployTrait>(
    mut replay_runtime_ops: ReplayRuntimeOps,
    state_hash: &StateHash,
    system_deploy: &mut S,
    processed_system_deploy: &ProcessedSystemDeploy,
) -> Result<SystemDeployReplayResult<S::Result>, CasperError> {
    let expected_failure = match processed_system_deploy {
        ProcessedSystemDeploy::Failed { error_msg, .. } => Some(error_msg.clone()),
        _ => None,
    };

    replay_runtime_ops
        .rig_system_deploy(processed_system_deploy)
        .await?;
    replay_runtime_ops
        .runtime_ops
        .runtime
        .reset(&Blake2b256Hash::from_bytes_prost(state_hash))
        .await?;

    let (value, eval_res) = replay_runtime_ops
        .replay_system_deploy_internal(system_deploy, &expected_failure)
        .await?;

    replay_runtime_ops
        .check_replay_data_with_fix(eval_res.errors.is_empty())
        .await?;

    match (value, eval_res) {
        (Either::Right(result), _) => {
            let checkpoint = replay_runtime_ops
                .runtime_ops
                .runtime
                .create_checkpoint()
                .await;

            Ok(SystemDeployReplayResult::ReplaySucceeded {
                state_hash: checkpoint.root.to_bytes_prost(),
                result,
            })
        }

        (Either::Left(error), _) => Ok(SystemDeployReplayResult::ReplayFailed {
            system_deploy_error: error,
        }),
    }
}

// D3 (DR-9, OD-2): `pre_charge_deploy_should_reduce_user_account_balance_by_correct_amount`
// and `refund_deploy_should_reject_refunds_above_recorded_precharge` are removed
// — the escrow PreChargeDeploy / RefundDeploy system deploys they exercised no
// longer exist. A deploy's cost is the per-COMM token count, settled once
// against Σ⟦s⟧ at block close (no per-deploy charge/refund round-trip).

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn close_block_should_make_epoch_change_and_reward_validator() {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let _ = compare_successful_system_deploys(
                &mut runtime_manager,
                &genesis_context,
                &genesis_block.body.state.post_state_hash,
                &mut CloseBlockDeploy::new(Blake2b512Random::create_from_bytes(&[0])),
                &mut CloseBlockDeploy::new(Blake2b512Random::create_from_bytes(&[0])),
                |_| true,
            )
            .await
            .unwrap();
        },
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn close_block_replay_should_fail_with_different_random_seed() {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let res = compare_successful_system_deploys(
                &mut runtime_manager,
                &genesis_context,
                &genesis_block.body.state.post_state_hash,
                &mut CloseBlockDeploy::new(Blake2b512Random::create_from_bytes(&[0])),
                &mut CloseBlockDeploy::new(Blake2b512Random::create_from_bytes(&[1])),
                |_| true,
            )
            .await;

            assert!(res.is_err());
        },
    )
    .await
    .unwrap();
}

/// Consensus-critical play/replay determinism test for epoch minting. At an
/// epoch boundary, `closeBlock` credits each eligible validator's canonical
/// SystemVault through the authenticated protocol-mint operation. Replaying the
/// same processed system deploy must produce the identical post-state root and
/// the exact expected vault credit.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn close_block_protocol_mint_is_play_replay_deterministic() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let sender = genesis_context.validator_pks()[0].clone();
            let sender_address = VaultAddress::from_public_key(&sender).unwrap();
            let initial_balance =
                system_vault_balance(&runtime_manager, &start_state, &sender_address).await;
            let block_data = BlockData {
                time_stamp: 0,
                block_number: 0,
                sender: sender.clone(),
                seq_num: 0,
            };

            // ---- PLAY ----
            let play_runtime = runtime_manager.spawn_runtime().await;
            play_runtime.set_block_data(block_data.clone()).await;
            let mut play_ops = RuntimeOps::new(play_runtime);

            let mut play_close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    sender.clone(),
                    block_data.seq_num,
                ),
            );
            let play_result = play_ops
                .play_system_deploy(&start_state, &mut play_close)
                .await
                .unwrap();

            let (final_play_state_hash, processed_system_deploy) = match play_result {
                SystemDeployResult::PlaySucceeded {
                    state_hash,
                    processed_system_deploy,
                    ..
                } => (state_hash, processed_system_deploy),
                SystemDeployResult::PlayFailed {
                    processed_system_deploy,
                } => panic!("close-block play failed: {:?}", processed_system_deploy),
            };

            let play_balance =
                system_vault_balance(&runtime_manager, &final_play_state_hash, &sender_address);
            let play_balance = play_balance.await;
            assert_eq!(
                play_balance,
                initial_balance + casper::rust::casper_conf::DEFAULT_EPOCH_PHLOGISTON
            );

            // ---- REPLAY (production path: replay_block_system_deploy) ----
            let replay_runtime = runtime_manager.spawn_replay_runtime().await;
            replay_runtime.set_block_data(block_data.clone()).await;
            let mut replay_ops = ReplayRuntimeOps::new_from_runtime(replay_runtime);
            replay_ops
                .runtime_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&start_state))
                .await
                .unwrap();

            replay_ops
                .replay_block_system_deploy(&block_data, &processed_system_deploy)
                .await
                .unwrap();

            let replay_checkpoint = replay_ops.runtime_ops.runtime.create_checkpoint().await;
            let final_replay_state_hash = replay_checkpoint.root.to_bytes_prost();

            // The consensus-critical assertion: byte-identical post-state
            // (including every Σ⟦v⟧ balance) between play and replay.
            assert_eq!(
                final_play_state_hash, final_replay_state_hash,
                "play and replay post-state hashes diverged on the Stage-B supply mint"
            );

            let replay_balance =
                system_vault_balance(&runtime_manager, &final_replay_state_hash, &sender_address)
                    .await;
            assert_eq!(
                play_balance, replay_balance,
                "Σ⟦v⟧ balance diverged between play and replay"
            );
        },
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn block_one_initial_draw_does_not_double_genesis_supply() {
    let mut parameters = GenesisBuilder::build_genesis_parameters_with_defaults(None, Some(3));
    parameters.2.proof_of_stake.epoch_length = 1;
    parameters.2.proof_of_stake.initial_phlogiston = 11;
    parameters.2.proof_of_stake.epoch_phlogiston = 7;
    let genesis_context = GenesisBuilder::new()
        .build_genesis_with_parameters(Some(parameters))
        .await
        .unwrap();
    let genesis_block = genesis_context.genesis_block.clone();
    let mut kvm = resources::mk_test_rnode_store_manager_from_genesis(&genesis_context);
    let (runtime_manager, _history) =
        resources::mk_runtime_manager_with_history_at(&mut *kvm).await;

    let start_state = genesis_block.body.state.post_state_hash.clone();
    let sender = genesis_context.validator_pks()[0].clone();
    let sender_address = VaultAddress::from_public_key(&sender).unwrap();
    let genesis_balance =
        system_vault_balance(&runtime_manager, &start_state, &sender_address).await;
    assert_eq!(genesis_balance, 11);

    let block_data = BlockData {
        time_stamp: 0,
        block_number: 1,
        sender: sender.clone(),
        seq_num: 0,
    };
    let play_runtime = runtime_manager.spawn_runtime().await;
    play_runtime.set_block_data(block_data.clone()).await;
    let mut play_ops = RuntimeOps::new(play_runtime);
    let mut close = CloseBlockDeploy::new(
        system_deploy_util::generate_close_deploy_random_seed_from_pk(sender, 0),
    );
    let (play_root, processed) = match play_ops
        .play_system_deploy(&start_state, &mut close)
        .await
        .unwrap()
    {
        SystemDeployResult::PlaySucceeded {
            state_hash,
            processed_system_deploy,
            ..
        } => (state_hash, processed_system_deploy),
        SystemDeployResult::PlayFailed {
            processed_system_deploy,
        } => panic!("block-one close failed: {processed_system_deploy:?}"),
    };
    assert_eq!(
        system_vault_balance(&runtime_manager, &play_root, &sender_address).await,
        genesis_balance + 7
    );
    let replay_runtime = runtime_manager.spawn_replay_runtime().await;
    replay_runtime.set_block_data(block_data.clone()).await;
    let mut replay_ops = ReplayRuntimeOps::new_from_runtime(replay_runtime);
    replay_ops
        .runtime_ops
        .runtime
        .reset(&Blake2b256Hash::from_bytes_prost(&start_state))
        .await
        .unwrap();
    replay_ops
        .replay_block_system_deploy(&block_data, &processed)
        .await
        .unwrap();
    let replay_root = replay_ops
        .runtime_ops
        .runtime
        .create_checkpoint()
        .await
        .root
        .to_bytes_prost();
    assert_eq!(replay_root, play_root);
}

/// Consensus-critical play/replay determinism test for slash quarantine. It
/// first gives the offender a nonzero canonical vault balance through epoch
/// minting, then verifies that slash play and replay quarantine the same exact
/// amount, halt future minting, and produce byte-identical state roots.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn slash_drains_validator_vault_is_play_replay_deterministic() {
    use rholang::rust::interpreter::rho_runtime::RhoRuntime as _;

    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            // The PROPOSER (slash issuer) is validator 0; the OFFENDER (the
            // validator whose invalid block is slashed) is validator 1.
            let proposer = genesis_context.validator_pks()[0].clone();
            let offender = genesis_context.validator_pks()[1].clone();

            // ── Step 1: fund Σ⟦offender⟧ with an epoch mint (non-vacuity) ──────
            let mint_block_data = BlockData {
                time_stamp: 0,
                block_number: 0, // 0 % epochLength == 0 ⇒ epoch boundary
                sender: proposer.clone(),
                seq_num: 0,
            };
            let mint_runtime = runtime_manager.spawn_runtime().await;
            mint_runtime.set_block_data(mint_block_data.clone()).await;
            let mut mint_ops = RuntimeOps::new(mint_runtime);
            let mut mint_close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    proposer.clone(),
                    mint_block_data.seq_num,
                ),
            );
            let mint_result = mint_ops
                .play_system_deploy(&start_state, &mut mint_close)
                .await
                .unwrap();
            let funded_state = match mint_result {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
                SystemDeployResult::PlayFailed { .. } => panic!("epoch-mint close failed"),
            };

            let offender_address = VaultAddress::from_public_key(&offender).unwrap();
            let pre_slash_balance =
                system_vault_balance(&runtime_manager, &funded_state, &offender_address).await;
            assert!(
                pre_slash_balance > 0,
                "non-vacuity: offender Σ⟦v⟧ must be positive before slash, got {}",
                pre_slash_balance
            );

            // ── Step 2: seed invalidBlocks (blockHash -> offender) ────────────
            let invalid_block_hash: prost::bytes::Bytes =
                prost::bytes::Bytes::from_static(b"slash-play-replay-invalid-block");
            let mut invalid_blocks: HashMap<prost::bytes::Bytes, prost::bytes::Bytes> =
                HashMap::new();
            invalid_blocks.insert(invalid_block_hash.clone(), offender.bytes.clone());

            let slash_block_data = BlockData {
                time_stamp: 0,
                block_number: 1,
                sender: proposer.clone(),
                seq_num: 1,
            };

            // ── Step 3: PLAY the slash ────────────────────────────────────────
            let play_runtime = runtime_manager.spawn_runtime().await;
            play_runtime.set_block_data(slash_block_data.clone()).await;
            play_runtime
                .set_invalid_blocks(invalid_blocks.clone())
                .await;
            let mut play_ops = RuntimeOps::new(play_runtime);
            let mut play_slash = SlashDeploy {
                invalid_block_hash: invalid_block_hash.clone(),
                equivocation_block_hash: None,
                pk: proposer.clone(),
                target_activation_epoch: 0,
                target_bond_generation: models::rust::bond_generation::BondGeneration::GENESIS,
                initial_rand: system_deploy_util::generate_slash_deploy_random_seed(
                    proposer.bytes.clone(),
                    slash_block_data.seq_num,
                    &invalid_block_hash,
                ),
            };
            let play_result = play_ops
                .play_system_deploy(&funded_state, &mut play_slash)
                .await
                .unwrap();
            let (final_play_state_hash, processed_slash) = match play_result {
                SystemDeployResult::PlaySucceeded {
                    state_hash,
                    processed_system_deploy,
                    ..
                } => (state_hash, processed_system_deploy),
                SystemDeployResult::PlayFailed {
                    processed_system_deploy,
                } => panic!("slash play failed: {:?}", processed_system_deploy),
            };

            let play_post_balance =
                system_vault_balance(&runtime_manager, &final_play_state_hash, &offender_address)
                    .await;
            assert_eq!(
                play_post_balance, 0,
                "slash must zero Σ⟦offender⟧ on play, got {}",
                play_post_balance
            );

            // ── Step 4: REPLAY the slash (production path) ────────────────────
            let replay_runtime = runtime_manager.spawn_replay_runtime().await;
            replay_runtime
                .set_block_data(slash_block_data.clone())
                .await;
            replay_runtime
                .set_invalid_blocks(invalid_blocks.clone())
                .await;
            let mut replay_ops = ReplayRuntimeOps::new_from_runtime(replay_runtime);
            replay_ops
                .runtime_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&funded_state))
                .await
                .unwrap();
            replay_ops
                .replay_block_system_deploy(&slash_block_data, &processed_slash)
                .await
                .unwrap();
            let replay_checkpoint = replay_ops.runtime_ops.runtime.create_checkpoint().await;
            let final_replay_state_hash = replay_checkpoint.root.to_bytes_prost();

            // The consensus-critical assertion: byte-identical post-state.
            assert_eq!(
                final_play_state_hash, final_replay_state_hash,
                "play and replay post-state hashes diverged on the Stage-C slash Σ⟦v⟧-zero"
            );

            let replay_post_balance = system_vault_balance(
                &runtime_manager,
                &final_replay_state_hash,
                &offender_address,
            )
            .await;
            assert_eq!(
                replay_post_balance, 0,
                "Σ⟦offender⟧ must be zero on replay too, got {}",
                replay_post_balance
            );
        },
    )
    .await
    .unwrap()
}

struct SlashAttemptPosition {
    target_bond_generation: BondGeneration,
    block_number: i64,
    seq_num: i32,
}

async fn play_slash_attempt(
    runtime_manager: &RuntimeManager,
    start_state: &StateHash,
    proposer: &crypto::rust::public_key::PublicKey,
    offender: &crypto::rust::public_key::PublicKey,
    invalid_block_hash: &prost::bytes::Bytes,
    position: SlashAttemptPosition,
) -> (SystemDeployResult<()>, StateHash) {
    use rholang::rust::interpreter::rho_runtime::RhoRuntime as _;
    let mut invalid_blocks: HashMap<prost::bytes::Bytes, prost::bytes::Bytes> = HashMap::new();
    invalid_blocks.insert(invalid_block_hash.clone(), offender.bytes.clone());
    let block_data = BlockData {
        time_stamp: 0,
        block_number: position.block_number,
        sender: proposer.clone(),
        seq_num: position.seq_num,
    };
    let runtime = runtime_manager.spawn_runtime().await;
    runtime.set_block_data(block_data.clone()).await;
    runtime.set_invalid_blocks(invalid_blocks).await;
    let mut ops = RuntimeOps::new(runtime);
    let mut slash = SlashDeploy {
        invalid_block_hash: invalid_block_hash.clone(),
        equivocation_block_hash: None,
        pk: proposer.clone(),
        target_activation_epoch: 0,
        target_bond_generation: position.target_bond_generation,
        initial_rand: system_deploy_util::generate_slash_deploy_random_seed(
            proposer.bytes.clone(),
            position.seq_num,
            invalid_block_hash,
        ),
    };
    let result = ops
        .play_system_deploy(start_state, &mut slash)
        .await
        .unwrap();
    let current_state = ops.runtime.create_checkpoint().await.root.to_bytes_prost();
    (result, current_state)
}

async fn play_one_slash(
    runtime_manager: &RuntimeManager,
    start_state: &StateHash,
    proposer: &crypto::rust::public_key::PublicKey,
    offender: &crypto::rust::public_key::PublicKey,
    invalid_block_hash: &prost::bytes::Bytes,
    block_number: i64,
    seq_num: i32,
) -> StateHash {
    match play_slash_attempt(
        runtime_manager,
        start_state,
        proposer,
        offender,
        invalid_block_hash,
        SlashAttemptPosition {
            target_bond_generation: BondGeneration::GENESIS,
            block_number,
            seq_num,
        },
    )
    .await
    .0
    {
        SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
        SystemDeployResult::PlayFailed {
            processed_system_deploy,
        } => {
            panic!("setup slash failed: {:?}", processed_system_deploy)
        }
    }
}

async fn request_validator_withdrawal(
    runtime_manager: &RuntimeManager,
    start_state: &StateHash,
    proposer: &crypto::rust::public_key::PublicKey,
    validator_secret: &PrivateKey,
    block_number: i64,
    seq_num: i32,
) -> StateHash {
    let source = r#"
        new return, poSCh, deployerId(`rho:system:deployerId`),
            rl(`rho:registry:lookup`)
        in {
          rl!(`rho:system:pos`, *poSCh) |
          for (@(_, PoS) <- poSCh) {
            @PoS!("withdraw", *deployerId, *return)
          }
        }
    "#
    .to_string();
    let deploy = construct_deploy::source_deploy(
        source,
        i64::from(seq_num),
        None,
        None,
        Some(validator_secret.clone()),
        None,
        Some("root".to_string()),
    )
    .unwrap();
    let block_data = BlockData {
        time_stamp: deploy.data.time_stamp,
        block_number,
        sender: proposer.clone(),
        seq_num,
    };
    let (state_hash, processed, _) = runtime_manager
        .compute_state(start_state, vec![deploy], Vec::new(), block_data, None)
        .await
        .unwrap();
    assert_eq!(processed.len(), 1);
    assert!(!processed[0].is_failed);
    state_hash
}

async fn submit_validator_bond(
    runtime_manager: &RuntimeManager,
    start_state: &StateHash,
    proposer: &crypto::rust::public_key::PublicKey,
    validator_secret: &PrivateKey,
    amount: i64,
    block_number: i64,
    seq_num: i32,
) -> StateHash {
    let source = format!(
        r#"
        new return, poSCh, deployerId(`rho:system:deployerId`),
            rl(`rho:registry:lookup`)
        in {{
          rl!(`rho:system:pos`, *poSCh) |
          for (@(_, PoS) <- poSCh) {{
            @PoS!("bond", *deployerId, {amount}, *return)
          }}
        }}
    "#
    );
    let deploy = construct_deploy::source_deploy(
        source,
        i64::from(seq_num),
        None,
        None,
        Some(validator_secret.clone()),
        None,
        Some("root".to_string()),
    )
    .unwrap();
    let block_data = BlockData {
        time_stamp: deploy.data.time_stamp,
        block_number,
        sender: proposer.clone(),
        seq_num,
    };
    let (state_hash, processed, _) = runtime_manager
        .compute_state(start_state, vec![deploy], Vec::new(), block_data, None)
        .await
        .unwrap();
    assert_eq!(processed.len(), 1);
    assert!(!processed[0].is_failed);
    state_hash
}

fn authorized_redeem(
    proposer: &crypto::rust::public_key::PublicKey,
    offender: &crypto::rust::public_key::PublicKey,
    target_bond_generation: BondGeneration,
    outcome: RedemptionOutcome,
    seq_num: i32,
) -> RedeemDeploy {
    use casper::rust::util::rholang::costacc::redeem_deploy::RedemptionAuthorization;

    let secp = Secp256k1;
    let keypairs = (0..3).map(|_| secp.new_key_pair()).collect::<Vec<_>>();
    let keyset = keypairs
        .iter()
        .map(|(_, public_key)| hex::encode(&public_key.bytes))
        .collect::<Vec<_>>();
    let mut redeem = RedeemDeploy::new(
        offender.bytes.to_vec(),
        target_bond_generation,
        outcome,
        keyset,
        2,
        proposer.bytes.clone(),
        seq_num,
    );
    let digest = redeem.auth_digest();
    redeem.authorizations = keypairs
        .iter()
        .take(2)
        .map(|(private_key, public_key)| RedemptionAuthorization {
            public_key: public_key.bytes.to_vec(),
            signature: secp.sign(&digest, &private_key.bytes),
        })
        .collect();
    assert!(redeem.verify_multisig_quorum());
    redeem
}

async fn play_redeem_attempt(
    runtime_manager: &RuntimeManager,
    start_state: &StateHash,
    proposer: &crypto::rust::public_key::PublicKey,
    mut redeem: RedeemDeploy,
    block_number: i64,
    seq_num: i32,
) -> (SystemDeployResult<()>, StateHash) {
    let block_data = BlockData {
        time_stamp: 0,
        block_number,
        sender: proposer.clone(),
        seq_num,
    };
    let runtime = runtime_manager.spawn_runtime().await;
    runtime.set_block_data(block_data).await;
    let mut ops = RuntimeOps::new(runtime);
    let result = ops
        .play_system_deploy(start_state, &mut redeem)
        .await
        .unwrap();
    let current_state = ops.runtime.create_checkpoint().await.root.to_bytes_prost();
    (result, current_state)
}

async fn play_one_redeem(
    runtime_manager: &RuntimeManager,
    start_state: &StateHash,
    proposer: &crypto::rust::public_key::PublicKey,
    offender: &crypto::rust::public_key::PublicKey,
    outcome: RedemptionOutcome,
    block_number: i64,
    seq_num: i32,
) -> StateHash {
    let redeem = authorized_redeem(
        proposer,
        offender,
        BondGeneration::GENESIS,
        outcome,
        seq_num,
    );
    match play_redeem_attempt(
        runtime_manager,
        start_state,
        proposer,
        redeem,
        block_number,
        seq_num,
    )
    .await
    .0
    {
        SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
        SystemDeployResult::PlayFailed {
            processed_system_deploy,
        } => panic!("redemption failed: {processed_system_deploy:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn redemption_restores_exact_pending_and_withdrawing_lifecycle() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let proposer = genesis_context.validator_pks()[0].clone();
            let offender = genesis_context.validator_pks()[1].clone();
            let offender_secret = genesis_context.validator_key_pairs[1].0.clone();
            let stake_vault_address = pos_stake_vault_address(&runtime_manager, &start_state).await;
            let reward_seeded_state = protocol_mint_to_vault(
                &runtime_manager,
                &start_state,
                &stake_vault_address,
                160,
                10,
            )
            .await;
            let funded_state = play_close(&runtime_manager, &reward_seeded_state, BlockData {
                time_stamp: 0,
                block_number: 0,
                sender: proposer.clone(),
                seq_num: 10,
            })
            .await;
            let offender_address = VaultAddress::from_public_key(&offender).unwrap();
            let coop_address = pos_coop_vault_address(&runtime_manager, &funded_state).await;
            let pending_state = request_validator_withdrawal(
                &runtime_manager,
                &funded_state,
                &proposer,
                &offender_secret,
                1,
                11,
            )
            .await;
            assert_eq!(
                pos_validator_lifecycle(&runtime_manager, &pending_state, &offender).await,
                "PendingWithdraw"
            );
            let pending_deadline =
                pos_validator_lifecycle_deadline(&runtime_manager, &pending_state, &offender).await;
            let pending_generation =
                pos_validator_lifecycle_generation(&runtime_manager, &pending_state, &offender)
                    .await;
            let pending_active =
                pos_validator_is_active(&runtime_manager, &pending_state, &offender).await;
            let pending_fuel =
                system_vault_balance(&runtime_manager, &pending_state, &offender_address).await;
            let pending_reward =
                pos_validator_reward(&runtime_manager, &pending_state, &offender).await;
            assert!(pending_reward > 0);
            let pending_stake_vault =
                system_vault_balance(&runtime_manager, &pending_state, &stake_vault_address).await;
            let pending_evidence = prost::bytes::Bytes::from_static(b"pending-phase-evidence");
            let pending_slashed = play_one_slash(
                &runtime_manager,
                &pending_state,
                &proposer,
                &offender,
                &pending_evidence,
                2,
                12,
            )
            .await;
            assert_eq!(
                pos_validator_lifecycle(&runtime_manager, &pending_slashed, &offender).await,
                "Quarantined:PendingWithdraw"
            );
            assert_eq!(
                pos_validator_lifecycle_deadline(&runtime_manager, &pending_slashed, &offender,)
                    .await,
                pending_deadline
            );
            assert_eq!(
                pos_quarantined_reward(&runtime_manager, &pending_slashed, &offender).await,
                pending_reward
            );
            let next_generation = BondGeneration::new(1).unwrap();
            let (stale_pending_slash, stale_pending_slash_root) = play_slash_attempt(
                &runtime_manager,
                &pending_slashed,
                &proposer,
                &offender,
                &pending_evidence,
                SlashAttemptPosition {
                    target_bond_generation: next_generation,
                    block_number: 3,
                    seq_num: 13,
                },
            )
            .await;
            assert!(matches!(
                stale_pending_slash,
                SystemDeployResult::PlayFailed { .. }
            ));
            assert_eq!(stale_pending_slash_root, pending_slashed);
            let stale_pending_redeem = authorized_redeem(
                &proposer,
                &offender,
                next_generation,
                RedemptionOutcome::Vindicated,
                14,
            );
            let (stale_pending_redemption, stale_pending_redemption_root) = play_redeem_attempt(
                &runtime_manager,
                &pending_slashed,
                &proposer,
                stale_pending_redeem,
                4,
                14,
            )
            .await;
            assert!(matches!(
                stale_pending_redemption,
                SystemDeployResult::PlayFailed { .. }
            ));
            assert_eq!(stale_pending_redemption_root, pending_slashed);
            let pending_restored = play_one_redeem(
                &runtime_manager,
                &pending_slashed,
                &proposer,
                &offender,
                RedemptionOutcome::Vindicated,
                3,
                13,
            )
            .await;
            assert_eq!(
                pos_validator_lifecycle(&runtime_manager, &pending_restored, &offender).await,
                "PendingWithdraw"
            );
            assert_eq!(
                pos_validator_lifecycle_deadline(&runtime_manager, &pending_restored, &offender,)
                    .await,
                pending_deadline
            );
            assert_eq!(
                pos_validator_lifecycle_generation(&runtime_manager, &pending_restored, &offender,)
                    .await,
                pending_generation
            );
            assert_eq!(
                pos_validator_is_active(&runtime_manager, &pending_restored, &offender).await,
                pending_active
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &pending_restored, &offender_address).await,
                pending_fuel
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &pending_restored, &stake_vault_address,)
                    .await,
                pending_stake_vault
            );
            assert_eq!(
                pos_validator_reward(&runtime_manager, &pending_restored, &offender).await,
                pending_reward
            );

            let epoch_length = pos_epoch_length(&runtime_manager, &pending_state).await;
            let withdrawing_state = play_close(&runtime_manager, &pending_state, BlockData {
                time_stamp: 0,
                block_number: epoch_length,
                sender: proposer.clone(),
                seq_num: 14,
            })
            .await;
            assert_eq!(
                pos_validator_lifecycle(&runtime_manager, &withdrawing_state, &offender).await,
                "Withdrawing"
            );
            let withdrawing_deadline =
                pos_validator_lifecycle_deadline(&runtime_manager, &withdrawing_state, &offender)
                    .await;
            let withdrawing_generation =
                pos_validator_lifecycle_generation(&runtime_manager, &withdrawing_state, &offender)
                    .await;
            let withdrawing_active =
                pos_validator_is_active(&runtime_manager, &withdrawing_state, &offender).await;
            let withdrawing_fuel =
                system_vault_balance(&runtime_manager, &withdrawing_state, &offender_address).await;
            let withdrawing_stake =
                pos_validator_locked_stake(&runtime_manager, &withdrawing_state, &offender).await;
            let withdrawing_reward =
                pos_validator_reward(&runtime_manager, &withdrawing_state, &offender).await;
            let withdrawing_coop =
                system_vault_balance(&runtime_manager, &withdrawing_state, &coop_address).await;
            assert!(withdrawing_stake > 1);
            assert!(withdrawing_reward > 0);
            let withdrawing_stake_vault =
                system_vault_balance(&runtime_manager, &withdrawing_state, &stake_vault_address)
                    .await;
            let withdrawing_evidence =
                prost::bytes::Bytes::from_static(b"withdrawing-phase-evidence");
            let withdrawing_slashed = play_one_slash(
                &runtime_manager,
                &withdrawing_state,
                &proposer,
                &offender,
                &withdrawing_evidence,
                epoch_length + 1,
                15,
            )
            .await;
            assert_eq!(
                pos_validator_lifecycle(&runtime_manager, &withdrawing_slashed, &offender).await,
                "Quarantined:Withdrawing"
            );
            assert_eq!(
                pos_validator_lifecycle_deadline(
                    &runtime_manager,
                    &withdrawing_slashed,
                    &offender,
                )
                .await,
                withdrawing_deadline
            );
            assert_eq!(
                pos_quarantined_reward(&runtime_manager, &withdrawing_slashed, &offender).await,
                withdrawing_reward
            );
            let full_penalty_redeem = authorized_redeem(
                &proposer,
                &offender,
                BondGeneration::GENESIS,
                RedemptionOutcome::Guilty {
                    penalty: withdrawing_stake,
                },
                16,
            );
            let (full_penalty_result, full_penalty_root) = play_redeem_attempt(
                &runtime_manager,
                &withdrawing_slashed,
                &proposer,
                full_penalty_redeem,
                epoch_length + 2,
                16,
            )
            .await;
            assert!(matches!(
                full_penalty_result,
                SystemDeployResult::PlayFailed { .. }
            ));
            assert_eq!(full_penalty_root, withdrawing_slashed);
            assert_eq!(
                pos_quarantined_stake(&runtime_manager, &full_penalty_root, &offender).await,
                withdrawing_stake
            );
            assert_eq!(
                pos_quarantined_reward(&runtime_manager, &full_penalty_root, &offender).await,
                withdrawing_reward
            );
            let withdrawing_restored = play_one_redeem(
                &runtime_manager,
                &withdrawing_slashed,
                &proposer,
                &offender,
                RedemptionOutcome::Guilty { penalty: 1 },
                epoch_length + 2,
                17,
            )
            .await;
            assert_eq!(
                pos_validator_lifecycle(&runtime_manager, &withdrawing_restored, &offender).await,
                "Withdrawing"
            );
            assert_eq!(
                pos_validator_lifecycle_deadline(
                    &runtime_manager,
                    &withdrawing_restored,
                    &offender,
                )
                .await,
                withdrawing_deadline
            );
            assert_eq!(
                pos_validator_lifecycle_generation(
                    &runtime_manager,
                    &withdrawing_restored,
                    &offender,
                )
                .await,
                withdrawing_generation
            );
            assert_eq!(
                pos_validator_is_active(&runtime_manager, &withdrawing_restored, &offender).await,
                withdrawing_active
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &withdrawing_restored, &offender_address,)
                    .await,
                withdrawing_fuel - 1_i64.min(withdrawing_fuel)
            );
            assert_eq!(
                pos_validator_locked_stake(&runtime_manager, &withdrawing_restored, &offender)
                    .await,
                withdrawing_stake - 1
            );
            assert_eq!(
                pos_validator_reward(&runtime_manager, &withdrawing_restored, &offender).await,
                withdrawing_reward
            );
            assert_eq!(
                system_vault_balance(
                    &runtime_manager,
                    &withdrawing_restored,
                    &stake_vault_address,
                )
                .await,
                withdrawing_stake_vault - 1
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &withdrawing_restored, &coop_address).await,
                withdrawing_coop + 1 + 1_i64.min(withdrawing_fuel)
            );
        },
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completed_withdrawal_rebond_scopes_slash_and_redemption_to_generation() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let proposer = genesis_context.validator_pks()[0].clone();
            let offender = genesis_context.validator_pks()[1].clone();
            let offender_secret = genesis_context.validator_key_pairs[1].0.clone();
            let offender_address = VaultAddress::from_public_key(&offender).unwrap();
            let stake_vault_address = pos_stake_vault_address(&runtime_manager, &start_state).await;
            let reward_seeded_state = protocol_mint_to_vault(
                &runtime_manager,
                &start_state,
                &stake_vault_address,
                160,
                31,
            )
            .await;
            let funded_state = play_close(&runtime_manager, &reward_seeded_state, BlockData {
                time_stamp: 0,
                block_number: 0,
                sender: proposer.clone(),
                seq_num: 31,
            })
            .await;
            let original_bond =
                pos_validator_bond(&runtime_manager, &funded_state, &offender).await;
            let original_reward =
                pos_validator_reward(&runtime_manager, &funded_state, &offender).await;
            assert!(original_bond > 0);
            assert!(original_reward > 0);
            assert_eq!(
                pos_validator_lifecycle_generation(&runtime_manager, &funded_state, &offender)
                    .await,
                0
            );

            let pending_state = request_validator_withdrawal(
                &runtime_manager,
                &funded_state,
                &proposer,
                &offender_secret,
                1,
                32,
            )
            .await;
            let epoch_length = pos_epoch_length(&runtime_manager, &pending_state).await;
            let deadline =
                pos_validator_lifecycle_deadline(&runtime_manager, &pending_state, &offender).await;
            assert!(deadline > epoch_length);
            assert_eq!(deadline % epoch_length, 0);
            let withdrawing_state = play_close(&runtime_manager, &pending_state, BlockData {
                time_stamp: 0,
                block_number: epoch_length,
                sender: proposer.clone(),
                seq_num: 33,
            })
            .await;
            assert_eq!(
                pos_validator_lifecycle(&runtime_manager, &withdrawing_state, &offender).await,
                "Withdrawing"
            );
            assert_eq!(
                pos_validator_locked_stake(&runtime_manager, &withdrawing_state, &offender).await,
                original_bond
            );
            assert_eq!(
                pos_validator_reward(&runtime_manager, &withdrawing_state, &offender).await,
                original_reward
            );
            let wallet_before_payout =
                system_vault_balance(&runtime_manager, &withdrawing_state, &offender_address).await;
            let stake_vault_before_payout =
                system_vault_balance(&runtime_manager, &withdrawing_state, &stake_vault_address)
                    .await;
            let paid_state = play_close(&runtime_manager, &withdrawing_state, BlockData {
                time_stamp: 0,
                block_number: deadline,
                sender: proposer.clone(),
                seq_num: 34,
            })
            .await;
            assert_eq!(
                pos_validator_lifecycle(&runtime_manager, &paid_state, &offender).await,
                "Absent"
            );
            assert_eq!(
                pos_validator_lifecycle_generation(&runtime_manager, &paid_state, &offender).await,
                0
            );
            assert_eq!(
                pos_validator_locked_stake(&runtime_manager, &paid_state, &offender).await,
                -1
            );
            assert_eq!(
                pos_validator_reward(&runtime_manager, &paid_state, &offender).await,
                0
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &paid_state, &offender_address).await,
                wallet_before_payout + original_bond + original_reward
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &paid_state, &stake_vault_address).await,
                stake_vault_before_payout - original_bond - original_reward
            );

            let rebonded_state = submit_validator_bond(
                &runtime_manager,
                &paid_state,
                &proposer,
                &offender_secret,
                original_bond,
                deadline + 1,
                35,
            )
            .await;
            let generation_one = BondGeneration::new(1).unwrap();
            let (generation_zero_authority, generation_one_authority) = tokio::try_join!(
                CanonicalSlashAuthority::load(&runtime_manager, &funded_state),
                CanonicalSlashAuthority::load(&runtime_manager, &rebonded_state),
            )
            .unwrap();
            assert_eq!(generation_zero_authority.state_hash(), &funded_state);
            assert_eq!(generation_one_authority.state_hash(), &rebonded_state);
            assert_eq!(
                generation_zero_authority.generation(&offender.bytes),
                Some(BondGeneration::GENESIS)
            );
            assert_eq!(
                generation_one_authority.generation(&offender.bytes),
                Some(generation_one)
            );
            assert_eq!(
                generation_zero_authority.bond(&offender.bytes),
                original_bond
            );
            assert_eq!(
                generation_one_authority.bond(&offender.bytes),
                original_bond
            );
            assert_eq!(
                pos_validator_lifecycle(&runtime_manager, &rebonded_state, &offender).await,
                "Bonded"
            );
            assert_eq!(
                pos_validator_bond(&runtime_manager, &rebonded_state, &offender).await,
                original_bond
            );
            assert_eq!(
                pos_validator_lifecycle_generation(&runtime_manager, &rebonded_state, &offender)
                    .await,
                generation_one.get()
            );
            let activated_state = play_close(&runtime_manager, &rebonded_state, BlockData {
                time_stamp: 0,
                block_number: deadline + epoch_length,
                sender: proposer.clone(),
                seq_num: 36,
            })
            .await;
            assert!(pos_validator_is_active(&runtime_manager, &activated_state, &offender).await);
            let second_reward_seeded_state = protocol_mint_to_vault(
                &runtime_manager,
                &activated_state,
                &stake_vault_address,
                160,
                37,
            )
            .await;
            let generation_one_state =
                play_close(&runtime_manager, &second_reward_seeded_state, BlockData {
                    time_stamp: 0,
                    block_number: deadline + 2 * epoch_length,
                    sender: proposer.clone(),
                    seq_num: 37,
                })
                .await;
            let generation_one_fuel =
                system_vault_balance(&runtime_manager, &generation_one_state, &offender_address)
                    .await;
            let generation_one_reward =
                pos_validator_reward(&runtime_manager, &generation_one_state, &offender).await;
            let generation_one_stake_vault = system_vault_balance(
                &runtime_manager,
                &generation_one_state,
                &stake_vault_address,
            )
            .await;
            let coop_address =
                pos_coop_vault_address(&runtime_manager, &generation_one_state).await;
            let generation_one_coop =
                system_vault_balance(&runtime_manager, &generation_one_state, &coop_address).await;
            assert!(generation_one_fuel > 0);
            assert!(generation_one_reward > 0);

            let evidence = prost::bytes::Bytes::from_static(b"generation-one-evidence");
            let (stale_slash, stale_slash_root) = play_slash_attempt(
                &runtime_manager,
                &generation_one_state,
                &proposer,
                &offender,
                &evidence,
                SlashAttemptPosition {
                    target_bond_generation: BondGeneration::GENESIS,
                    block_number: deadline + 2 * epoch_length + 1,
                    seq_num: 38,
                },
            )
            .await;
            assert!(matches!(stale_slash, SystemDeployResult::PlayFailed { .. }));
            assert_eq!(stale_slash_root, generation_one_state);

            let (slash, slash_root) = play_slash_attempt(
                &runtime_manager,
                &generation_one_state,
                &proposer,
                &offender,
                &evidence,
                SlashAttemptPosition {
                    target_bond_generation: generation_one,
                    block_number: deadline + 2 * epoch_length + 1,
                    seq_num: 39,
                },
            )
            .await;
            let slashed_state = match slash {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
                SystemDeployResult::PlayFailed {
                    processed_system_deploy,
                } => panic!("generation-one slash failed: {processed_system_deploy:?}"),
            };
            assert_eq!(slash_root, slashed_state);
            assert_eq!(
                pos_quarantined_stake(&runtime_manager, &slashed_state, &offender).await,
                original_bond
            );
            assert_eq!(
                pos_quarantined_reward(&runtime_manager, &slashed_state, &offender).await,
                generation_one_reward
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &slashed_state, &offender_address).await,
                0
            );

            let (duplicate_slash, duplicate_slash_root) = play_slash_attempt(
                &runtime_manager,
                &slashed_state,
                &proposer,
                &offender,
                &evidence,
                SlashAttemptPosition {
                    target_bond_generation: generation_one,
                    block_number: deadline + 2 * epoch_length + 1,
                    seq_num: 39,
                },
            )
            .await;
            match duplicate_slash {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => {
                    assert_eq!(state_hash, duplicate_slash_root)
                }
                SystemDeployResult::PlayFailed {
                    processed_system_deploy,
                } => panic!("duplicate generation-one slash failed: {processed_system_deploy:?}"),
            }
            assert_eq!(
                pos_quarantined_stake(&runtime_manager, &duplicate_slash_root, &offender).await,
                original_bond
            );
            assert_eq!(
                pos_quarantined_reward(&runtime_manager, &duplicate_slash_root, &offender).await,
                generation_one_reward
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &duplicate_slash_root, &offender_address)
                    .await,
                0
            );
            assert_eq!(
                system_vault_balance(
                    &runtime_manager,
                    &duplicate_slash_root,
                    &stake_vault_address,
                )
                .await,
                generation_one_stake_vault
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &duplicate_slash_root, &coop_address).await,
                generation_one_coop
            );
            let conflicting_evidence =
                prost::bytes::Bytes::from_static(b"generation-one-conflicting-evidence");
            let (conflicting_slash, conflicting_slash_root) = play_slash_attempt(
                &runtime_manager,
                &slashed_state,
                &proposer,
                &offender,
                &conflicting_evidence,
                SlashAttemptPosition {
                    target_bond_generation: generation_one,
                    block_number: deadline + 2 * epoch_length + 2,
                    seq_num: 40,
                },
            )
            .await;
            match conflicting_slash {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => {
                    assert_eq!(state_hash, conflicting_slash_root)
                }
                SystemDeployResult::PlayFailed {
                    processed_system_deploy,
                } => panic!(
                    "second generation-one evidence failed idempotence: {processed_system_deploy:?}"
                ),
            }
            assert_eq!(
                pos_quarantined_stake(&runtime_manager, &conflicting_slash_root, &offender).await,
                original_bond
            );
            assert_eq!(
                pos_quarantined_reward(&runtime_manager, &conflicting_slash_root, &offender).await,
                generation_one_reward
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &conflicting_slash_root, &offender_address)
                    .await,
                0
            );
            assert_eq!(
                system_vault_balance(
                    &runtime_manager,
                    &conflicting_slash_root,
                    &stake_vault_address,
                )
                .await,
                generation_one_stake_vault
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &conflicting_slash_root, &coop_address)
                    .await,
                generation_one_coop
            );

            let stale_redeem = authorized_redeem(
                &proposer,
                &offender,
                BondGeneration::GENESIS,
                RedemptionOutcome::Vindicated,
                41,
            );
            let (stale_redemption, stale_redemption_root) = play_redeem_attempt(
                &runtime_manager,
                &slashed_state,
                &proposer,
                stale_redeem,
                deadline + 2 * epoch_length + 3,
                41,
            )
            .await;
            assert!(matches!(
                stale_redemption,
                SystemDeployResult::PlayFailed { .. }
            ));
            assert_eq!(stale_redemption_root, slashed_state);

            let vindication = authorized_redeem(
                &proposer,
                &offender,
                generation_one,
                RedemptionOutcome::Vindicated,
                42,
            );
            let (redemption, redemption_root) = play_redeem_attempt(
                &runtime_manager,
                &slashed_state,
                &proposer,
                vindication.clone(),
                deadline + 2 * epoch_length + 4,
                42,
            )
            .await;
            let restored_state = match redemption {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
                SystemDeployResult::PlayFailed {
                    processed_system_deploy,
                } => panic!("generation-one vindication failed: {processed_system_deploy:?}"),
            };
            assert_eq!(redemption_root, restored_state);
            assert_eq!(
                pos_validator_bond(&runtime_manager, &restored_state, &offender).await,
                original_bond
            );
            assert_eq!(
                pos_validator_reward(&runtime_manager, &restored_state, &offender).await,
                generation_one_reward
            );
            assert_eq!(
                pos_validator_lifecycle_generation(&runtime_manager, &restored_state, &offender)
                    .await,
                generation_one.get()
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &restored_state, &offender_address).await,
                generation_one_fuel
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &restored_state, &stake_vault_address).await,
                generation_one_stake_vault
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &restored_state, &coop_address).await,
                generation_one_coop
            );

            let (duplicate_redemption, duplicate_redemption_root) = play_redeem_attempt(
                &runtime_manager,
                &restored_state,
                &proposer,
                vindication,
                deadline + 2 * epoch_length + 4,
                42,
            )
            .await;
            assert!(matches!(
                duplicate_redemption,
                SystemDeployResult::PlayFailed { .. }
            ));
            assert_eq!(duplicate_redemption_root, restored_state);
            let conflicting_redemption = authorized_redeem(
                &proposer,
                &offender,
                generation_one,
                RedemptionOutcome::Burned,
                43,
            );
            let (conflicting_redemption, conflicting_redemption_root) = play_redeem_attempt(
                &runtime_manager,
                &restored_state,
                &proposer,
                conflicting_redemption,
                deadline + 2 * epoch_length + 5,
                43,
            )
            .await;
            assert!(matches!(
                conflicting_redemption,
                SystemDeployResult::PlayFailed { .. }
            ));
            assert_eq!(conflicting_redemption_root, restored_state);
        },
    )
    .await
    .unwrap()
}

/// CONSENSUS-CRITICAL Stage-C redemption end-to-end (DR-7/DR-12). Drives the real
/// `redeemSlashed` Rholang contract through `RedeemDeploy`:
///   (1) fund + slash an offender (reaching a quarantined, halted state whose
///       bond is removed from the live committee map);
///   (2) play a Vindicated redeem with a VALID PoS-multisig quorum — asserts the
///       deploy SUCCEEDS, the validator is restored to active, and un-halted;
///   (3) play a Vindicated redeem with an UNDER-QUORUM authorization — asserts the
///       deploy is REJECTED (no restore: the validator stays quarantined/halted).
/// The DR-12 multisig-quorum verification is the Rust platform obligation; the
/// keyset/quorum/authorizations ride on `RedeemDeploy` (replay-carried).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn redeem_outcomes_and_multisig_gate() {
    use casper::rust::util::rholang::costacc::redeem_deploy::RedemptionAuthorization;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use rholang::rust::interpreter::rho_runtime::RhoRuntime as _;

    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let proposer = genesis_context.validator_pks()[0].clone();
            let offender = genesis_context.validator_pks()[1].clone();
            let invalid_block_hash: prost::bytes::Bytes =
                prost::bytes::Bytes::from_static(b"redeem-e2e-invalid-block");

            // ── (1) fund Σ⟦offender⟧ then slash to quarantine the offender ────
            let mint_block_data = BlockData {
                time_stamp: 0,
                block_number: 0,
                sender: proposer.clone(),
                seq_num: 0,
            };
            let mint_runtime = runtime_manager.spawn_runtime().await;
            mint_runtime.set_block_data(mint_block_data.clone()).await;
            let mut mint_ops = RuntimeOps::new(mint_runtime);
            let mut mint_close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(proposer.clone(), 0),
            );
            let funded_state = match mint_ops
                .play_system_deploy(&start_state, &mut mint_close)
                .await
                .unwrap()
            {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
                SystemDeployResult::PlayFailed { .. } => panic!("epoch mint failed"),
            };
            let offender_address = VaultAddress::from_public_key(&offender).unwrap();
            let coop_address = pos_coop_vault_address(&runtime_manager, &funded_state).await;
            let stake_vault_address =
                pos_stake_vault_address(&runtime_manager, &funded_state).await;
            let original_fuel =
                system_vault_balance(&runtime_manager, &funded_state, &offender_address).await;
            let original_bond =
                pos_validator_bond(&runtime_manager, &funded_state, &offender).await;
            let original_coop_fuel =
                system_vault_balance(&runtime_manager, &funded_state, &coop_address).await;
            let original_stake_vault_balance =
                system_vault_balance(&runtime_manager, &funded_state, &stake_vault_address).await;
            assert!(original_fuel > 0);
            assert!(original_bond > 0);

            let slashed_state = play_one_slash(
                &runtime_manager,
                &funded_state,
                &proposer,
                &offender,
                &invalid_block_hash,
                1,
                1,
            )
            .await;
            assert_eq!(
                system_vault_balance(&runtime_manager, &slashed_state, &offender_address).await,
                0
            );
            assert_eq!(
                pos_validator_bond(&runtime_manager, &slashed_state, &offender).await,
                -1
            );
            assert_eq!(
                pos_quarantined_stake(&runtime_manager, &slashed_state, &offender).await,
                original_bond
            );

            // Build a custom 3-key multisig set (quorum 2) and its secrets. The
            // RedeemDeploy carries its own keyset/quorum (replay-stable); the
            // Rust DR-12 obligation verifies signatures over the redemption digest.
            let secp = Secp256k1;
            let keypairs: Vec<(Vec<u8>, Vec<u8>)> = (0..3)
                .map(|_| {
                    let (sk, pk) = secp.new_key_pair();
                    (sk.bytes.to_vec(), pk.bytes.to_vec())
                })
                .collect();
            let keyset: Vec<String> = keypairs.iter().map(|(_, pk)| hex::encode(pk)).collect();

            let make_redeem =
                |n_signers: usize, seq: i32, outcome: RedemptionOutcome| -> RedeemDeploy {
                    let mut d = RedeemDeploy::new(
                        offender.bytes.to_vec(),
                        models::rust::bond_generation::BondGeneration::GENESIS,
                        outcome,
                        keyset.clone(),
                        2,
                        proposer.bytes.clone(),
                        seq,
                    );
                    let digest = d.auth_digest();
                    d.authorizations = keypairs
                        .iter()
                        .take(n_signers)
                        .map(|(sk, pk)| RedemptionAuthorization {
                            public_key: pk.clone(),
                            signature: secp.sign(&digest, sk),
                        })
                        .collect();
                    d
                };

            let redeem_block_data = BlockData {
                time_stamp: 0,
                block_number: 2,
                sender: proposer.clone(),
                seq_num: 2,
            };

            // ── (3 first: under-quorum REJECTION on the quarantined state) ────
            // Only 1 of 2 required signers ⇒ verify_multisig_quorum is false ⇒
            // redeemSlashed rejects with NO state change.
            let under_runtime = runtime_manager.spawn_runtime().await;
            under_runtime
                .set_block_data(redeem_block_data.clone())
                .await;
            let mut under_ops = RuntimeOps::new(under_runtime);
            let mut under_redeem = make_redeem(1, 2, RedemptionOutcome::Vindicated);
            assert!(
                !under_redeem.verify_multisig_quorum(),
                "1-of-2 must be under quorum"
            );
            let under_result = under_ops
                .play_system_deploy(&slashed_state, &mut under_redeem)
                .await
                .unwrap();
            // The deploy itself does not error, but the contract returns
            // (false, ...) ⇒ play_system_deploy reports a system-deploy USER
            // failure (PlayFailed). Either way, the offender must STAY quarantined.
            let under_post_state = match under_result {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
                SystemDeployResult::PlayFailed { .. } => slashed_state.clone(),
            };
            // Assert the offender is STILL halted (not restored) on the under-quorum path.
            let under_runtime2 = runtime_manager.spawn_runtime().await;
            let mut under_ops2 = RuntimeOps::new(under_runtime2);
            assert!(
                pos_validator_is_halted(&mut under_ops2, &under_post_state, &offender).await,
                "under-quorum redemption must NOT restore: offender stays halted"
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &under_post_state, &offender_address).await,
                0
            );
            assert_eq!(
                pos_validator_bond(&runtime_manager, &under_post_state, &offender).await,
                -1
            );
            assert_eq!(
                pos_quarantined_stake(&runtime_manager, &under_post_state, &offender).await,
                original_bond
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &under_post_state, &coop_address).await,
                original_coop_fuel
            );

            // ── (2) valid quorum (2-of-2) Vindicated ⇒ restore + un-halt ──────
            let ok_runtime = runtime_manager.spawn_runtime().await;
            ok_runtime.set_block_data(redeem_block_data.clone()).await;
            let mut ok_ops = RuntimeOps::new(ok_runtime);
            let mut ok_redeem = make_redeem(2, 2, RedemptionOutcome::Vindicated);
            assert!(
                ok_redeem.verify_multisig_quorum(),
                "2-of-2 must meet quorum"
            );
            let ok_result = ok_ops
                .play_system_deploy(&slashed_state, &mut ok_redeem)
                .await
                .unwrap();
            let ok_post_state = match ok_result {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
                SystemDeployResult::PlayFailed {
                    processed_system_deploy,
                } => {
                    panic!(
                        "valid-quorum vindicated redeem failed: {:?}",
                        processed_system_deploy
                    )
                }
            };
            let ok_runtime2 = runtime_manager.spawn_runtime().await;
            let mut ok_ops2 = RuntimeOps::new(ok_runtime2);
            assert!(
                !pos_validator_is_halted(&mut ok_ops2, &ok_post_state, &offender).await,
                "valid-quorum vindicated redemption must un-halt the offender"
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &ok_post_state, &offender_address).await,
                original_fuel
            );
            assert_eq!(
                pos_validator_bond(&runtime_manager, &ok_post_state, &offender).await,
                original_bond
            );
            assert_eq!(
                pos_quarantined_stake(&runtime_manager, &ok_post_state, &offender).await,
                -1
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &ok_post_state, &coop_address).await,
                original_coop_fuel
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &ok_post_state, &stake_vault_address).await,
                original_stake_vault_balance
            );
            let epoch_length = pos_epoch_length(&runtime_manager, &ok_post_state).await;
            let same_epoch_state = play_close(&runtime_manager, &ok_post_state, BlockData {
                time_stamp: 0,
                block_number: 0,
                sender: proposer.clone(),
                seq_num: 9,
            })
            .await;
            assert_eq!(
                system_vault_balance(&runtime_manager, &same_epoch_state, &offender_address).await,
                original_fuel
            );
            let next_epoch_state = play_close(&runtime_manager, &same_epoch_state, BlockData {
                time_stamp: 0,
                block_number: epoch_length,
                sender: proposer.clone(),
                seq_num: 10,
            })
            .await;
            let next_epoch_fuel =
                system_vault_balance(&runtime_manager, &next_epoch_state, &offender_address).await;
            assert!(next_epoch_fuel > original_fuel);
            let repeated_epoch_state = play_close(&runtime_manager, &next_epoch_state, BlockData {
                time_stamp: 0,
                block_number: epoch_length,
                sender: proposer.clone(),
                seq_num: 11,
            })
            .await;
            assert_eq!(
                system_vault_balance(&runtime_manager, &repeated_epoch_state, &offender_address)
                    .await,
                next_epoch_fuel
            );

            let full_guilty_runtime = runtime_manager.spawn_runtime().await;
            full_guilty_runtime
                .set_block_data(redeem_block_data.clone())
                .await;
            let mut full_guilty_ops = RuntimeOps::new(full_guilty_runtime);
            let mut full_guilty_redeem = make_redeem(2, 3, RedemptionOutcome::Guilty {
                penalty: original_bond,
            });
            assert!(matches!(
                full_guilty_ops
                    .play_system_deploy(&slashed_state, &mut full_guilty_redeem)
                    .await
                    .unwrap(),
                SystemDeployResult::PlayFailed { .. }
            ));
            assert_eq!(
                full_guilty_ops
                    .runtime
                    .create_checkpoint()
                    .await
                    .root
                    .to_bytes_prost(),
                slashed_state
            );
            assert_eq!(
                pos_quarantined_stake(&runtime_manager, &slashed_state, &offender).await,
                original_bond
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &slashed_state, &offender_address).await,
                0
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &slashed_state, &coop_address).await,
                original_coop_fuel
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &slashed_state, &stake_vault_address).await,
                original_stake_vault_balance
            );

            let guilty_penalty = original_bond.saturating_sub(1).min(7);
            assert!(guilty_penalty > 0);
            let guilty_runtime = runtime_manager.spawn_runtime().await;
            guilty_runtime
                .set_block_data(redeem_block_data.clone())
                .await;
            let mut guilty_ops = RuntimeOps::new(guilty_runtime);
            let mut guilty_redeem = make_redeem(2, 3, RedemptionOutcome::Guilty {
                penalty: guilty_penalty,
            });
            let guilty_state = match guilty_ops
                .play_system_deploy(&slashed_state, &mut guilty_redeem)
                .await
                .unwrap()
            {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
                SystemDeployResult::PlayFailed {
                    processed_system_deploy,
                } => panic!("valid-quorum guilty redeem failed: {processed_system_deploy:?}"),
            };
            let stake_penalty = guilty_penalty;
            let fuel_penalty = stake_penalty.min(original_fuel);
            assert_eq!(
                system_vault_balance(&runtime_manager, &guilty_state, &offender_address).await,
                original_fuel - fuel_penalty
            );
            assert_eq!(
                pos_validator_bond(&runtime_manager, &guilty_state, &offender).await,
                original_bond - stake_penalty
            );
            assert_eq!(
                pos_quarantined_stake(&runtime_manager, &guilty_state, &offender).await,
                -1
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &guilty_state, &coop_address).await,
                original_coop_fuel + fuel_penalty + stake_penalty
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &guilty_state, &stake_vault_address).await,
                original_stake_vault_balance - stake_penalty
            );
            let guilty_check_runtime = runtime_manager.spawn_runtime().await;
            let mut guilty_check_ops = RuntimeOps::new(guilty_check_runtime);
            assert!(
                !pos_validator_is_halted(&mut guilty_check_ops, &guilty_state, &offender).await
            );

            let burned_runtime = runtime_manager.spawn_runtime().await;
            burned_runtime
                .set_block_data(redeem_block_data.clone())
                .await;
            let mut burned_ops = RuntimeOps::new(burned_runtime);
            let mut burned_redeem = make_redeem(2, 4, RedemptionOutcome::Burned);
            let burned_state = match burned_ops
                .play_system_deploy(&slashed_state, &mut burned_redeem)
                .await
                .unwrap()
            {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
                SystemDeployResult::PlayFailed {
                    processed_system_deploy,
                } => panic!("valid-quorum burned redeem failed: {processed_system_deploy:?}"),
            };
            assert_eq!(
                system_vault_balance(&runtime_manager, &burned_state, &offender_address).await,
                0
            );
            assert_eq!(
                pos_validator_bond(&runtime_manager, &burned_state, &offender).await,
                -1
            );
            assert_eq!(
                pos_quarantined_stake(&runtime_manager, &burned_state, &offender).await,
                -1
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &burned_state, &coop_address).await,
                original_coop_fuel
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &burned_state, &stake_vault_address).await,
                original_stake_vault_balance - original_bond
            );
            let burned_check_runtime = runtime_manager.spawn_runtime().await;
            let mut burned_check_ops = RuntimeOps::new(burned_check_runtime);
            assert!(pos_validator_is_halted(&mut burned_check_ops, &burned_state, &offender).await);
        },
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn redeem_outcomes_are_play_replay_deterministic() {
    use casper::rust::util::rholang::costacc::redeem_deploy::RedemptionAuthorization;
    use crypto::rust::signatures::secp256k1::Secp256k1;
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;
    use rholang::rust::interpreter::rho_runtime::RhoRuntime as _;

    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let proposer = genesis_context.validator_pks()[0].clone();
            let offender = genesis_context.validator_pks()[1].clone();
            let invalid_block_hash: prost::bytes::Bytes =
                prost::bytes::Bytes::from_static(b"redeem-replay-invalid-block");

            let mint_block_data = BlockData {
                time_stamp: 0,
                block_number: 0,
                sender: proposer.clone(),
                seq_num: 0,
            };
            let mint_runtime = runtime_manager.spawn_runtime().await;
            mint_runtime.set_block_data(mint_block_data.clone()).await;
            let mut mint_ops = RuntimeOps::new(mint_runtime);
            let mut mint_close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(proposer.clone(), 0),
            );
            let funded_state = match mint_ops
                .play_system_deploy(&start_state, &mut mint_close)
                .await
                .unwrap()
            {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
                SystemDeployResult::PlayFailed { .. } => panic!("epoch mint failed"),
            };
            let slashed_state = play_one_slash(
                &runtime_manager,
                &funded_state,
                &proposer,
                &offender,
                &invalid_block_hash,
                1,
                1,
            )
            .await;

            let secp = Secp256k1;
            let keypairs: Vec<(Vec<u8>, Vec<u8>)> = (0..3)
                .map(|_| {
                    let (sk, pk) = secp.new_key_pair();
                    (sk.bytes.to_vec(), pk.bytes.to_vec())
                })
                .collect();
            let keyset: Vec<String> = keypairs.iter().map(|(_, pk)| hex::encode(pk)).collect();
            let outcomes = [
                RedemptionOutcome::Vindicated,
                RedemptionOutcome::Guilty { penalty: 0 },
                RedemptionOutcome::Burned,
            ];
            for (index, outcome) in outcomes.into_iter().enumerate() {
                let seq_num = i32::try_from(index).unwrap() + 2;
                let label = format!("{outcome:?}");
                let mut redeem = RedeemDeploy::new(
                    offender.bytes.to_vec(),
                    models::rust::bond_generation::BondGeneration::GENESIS,
                    outcome.clone(),
                    keyset.clone(),
                    2,
                    proposer.bytes.clone(),
                    seq_num,
                );
                let digest = redeem.auth_digest();
                redeem.authorizations = keypairs
                    .iter()
                    .take(2)
                    .map(|(sk, pk)| RedemptionAuthorization {
                        public_key: pk.clone(),
                        signature: secp.sign(&digest, sk),
                    })
                    .collect();
                assert!(redeem.verify_multisig_quorum());

                let redeem_block_data = BlockData {
                    time_stamp: 0,
                    block_number: 2,
                    sender: proposer.clone(),
                    seq_num,
                };
                let play_runtime = runtime_manager.spawn_runtime().await;
                play_runtime.set_block_data(redeem_block_data.clone()).await;
                let mut play_ops = RuntimeOps::new(play_runtime);
                let (play_state, processed) = match play_ops
                    .play_system_deploy(&slashed_state, &mut redeem)
                    .await
                    .unwrap()
                {
                    SystemDeployResult::PlaySucceeded {
                        state_hash,
                        processed_system_deploy,
                        ..
                    } => (state_hash, processed_system_deploy),
                    SystemDeployResult::PlayFailed {
                        processed_system_deploy,
                    } => panic!("{label} redemption failed: {processed_system_deploy:?}"),
                };

                let replay_runtime = runtime_manager.spawn_replay_runtime().await;
                replay_runtime
                    .set_block_data(redeem_block_data.clone())
                    .await;
                let mut replay_ops = ReplayRuntimeOps::new_from_runtime(replay_runtime);
                replay_ops
                    .runtime_ops
                    .runtime
                    .reset(&Blake2b256Hash::from_bytes_prost(&slashed_state))
                    .await
                    .unwrap();
                replay_ops
                    .replay_block_system_deploy(&redeem_block_data, &processed)
                    .await
                    .unwrap();
                let replay_state = replay_ops
                    .runtime_ops
                    .runtime
                    .create_checkpoint()
                    .await
                    .root
                    .to_bytes_prost();
                assert_eq!(play_state, replay_state, "{label} play/replay mismatch");

                let check_runtime = runtime_manager.spawn_runtime().await;
                let mut check_ops = RuntimeOps::new(check_runtime);
                assert_eq!(
                    pos_validator_is_halted(&mut check_ops, &replay_state, &offender).await,
                    matches!(outcome, RedemptionOutcome::Burned)
                );
            }
        },
    )
    .await
    .unwrap()
}

/// CONSENSUS-CRITICAL Stage-C halt observation. Reads the PoS `mintingHalted`
/// Set[PublicKey] off `post_state` and returns whether `validator` is a member.
/// Drives the `getMintingHalted` peek contract (PoS.rhox, added for Stage-C
/// observability) through a registry-looked-up exploratory deploy, computing the
/// membership predicate INSIDE Rholang (`halted.contains(pk)`) so the captured
/// result is a single `GBool` — robust, no nested ESet decode. The exploratory
/// deploy resets to `post_state` internally (read-only; no mutation).
async fn pos_validator_is_halted(
    ops: &mut RuntimeOps,
    post_state: &StateHash,
    validator: &crypto::rust::public_key::PublicKey,
) -> bool {
    use models::rhoapi::expr::ExprInstance;

    // `return` is the FIRST `new` name, so it is the channel
    // `play_exploratory_deploy` captures. Look PoS up from the registry, peek
    // `getMintingHalted`, and send back the membership boolean for the offender.
    let term = format!(
        r#"
        new return, poSCh, haltedCh,
            rl(`rho:registry:lookup`)
        in {{
          rl!(`rho:system:pos`, *poSCh) |
          for (@(_, PoS) <- poSCh) {{
            @PoS!("getMintingHalted", *haltedCh) |
            for (@halted <- haltedCh) {{
              return!(halted.contains("{}".hexToBytes()))
            }}
          }}
        }}"#,
        hex::encode(&validator.bytes)
    );

    let (results, _cost) = ops
        .play_exploratory_deploy(term, post_state, None)
        .await
        .expect("getMintingHalted exploratory query must execute");

    // The captured return value is a single `GBool`: true iff the offender is
    // still in `mintingHalted` (halted), false iff un-halted (restored).
    results
        .iter()
        .flat_map(|p| p.exprs.iter())
        .find_map(|e| match e.expr_instance {
            Some(ExprInstance::GBool(b)) => Some(b),
            _ => None,
        })
        .expect("getMintingHalted membership query must return a boolean")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn state_bound_settlement_charges_the_realized_branch_and_replays_identically() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let payer_key = genesis_context.genesis_vaults[0].0.clone();
            let payer_address =
                VaultAddress::from_public_key(&genesis_context.genesis_vaults[0].1).unwrap();
            let proposer_address =
                VaultAddress::from_public_key(&genesis_context.validator_pks()[0]).unwrap();
            let initial_payer =
                system_vault_balance(&runtime_manager, &start_state, &payer_address).await;
            let initial_proposer =
                system_vault_balance(&runtime_manager, &start_state, &proposer_address).await;
            let deploy = construct_deploy::source_deploy(
                "if (true) { new x in { x!(0) | for(@0 <- x){ Nil } } } else { new x, y, z in { x!(0) | for(@0 <- x){ Nil } | y!(1) | for(@1 <- y){ Nil } | z!(2) | for(@2 <- z){ Nil } } }".to_string(),
                1,
                None,
                None,
                Some(payer_key),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let cosigned =
                crypto::rust::signatures::signed::Cosigned::from_single_signer(deploy).unwrap();

            let block_data = BlockData {
                time_stamp: 2,
                block_number: 2,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 2,
            };
            let admission = runtime_manager
                .certify_state_bound_admission(
                    &start_state,
                    vec![cosigned],
                    &block_data,
                    &HashMap::new(),
                )
                .await
                .unwrap();
            let admitted_realized = admission
                .outcome()
                .debits
                .values()
                .map(|debit| debit.amount)
                .sum::<i64>();
            assert!(admitted_realized > 0);
            assert_eq!(
                admission
                    .outcome()
                    .fee_debits
                    .values()
                    .map(|debit| debit.amount)
                    .sum::<i64>(),
                1
            );
            assert!(admission.outcome().stack_pops.is_empty());

            let close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    block_data.sender.clone(),
                    block_data.seq_num,
                ),
            );

            let (play_post, processed, processed_system, _) = runtime_manager
                .compute_state_with_bonds_cosigned_admitted(admission, vec![
                    casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(close),
                ])
                .await
                .unwrap();
            assert_eq!(processed.len(), 1);

            let replay_post = runtime_manager
                .replay_compute_state(
                    &start_state,
                    processed.clone(),
                    processed_system.clone(),
                    &block_data,
                    None,
                    false,
                )
                .await
                .unwrap();
            assert_eq!(play_post, replay_post);
            let certificate = processed[0]
                .authority_funding_certificate
                .as_ref()
                .unwrap();
            let witness = processed[0].authority_cost_witness.as_ref().unwrap();
            let authority_reserved = certificate
                .allocation
                .iter()
                .map(|resource| resource.amount)
                .sum::<u64>();
            let byte_reserved = certificate
                .byte_allocation
                .iter()
                .map(|resource| resource.amount)
                .sum::<u64>();
            let authority_realized = witness
                .settlement
                .iter()
                .map(|resource| resource.amount)
                .sum::<u64>();
            let byte_realized = witness
                .byte_settlement
                .iter()
                .map(|resource| resource.amount)
                .sum::<u64>();
            let fee = certificate
                .fee_allocation
                .iter()
                .map(|resource| resource.amount)
                .sum::<u64>();
            assert!(authority_reserved >= authority_realized);
            assert!(byte_reserved >= byte_realized);
            assert!(byte_realized >= witness.byte_cost);
            assert_eq!(byte_reserved, byte_realized);
            assert!(!witness.byte_events.is_empty());
            assert_eq!(
                witness
                    .byte_events
                    .iter()
                    .map(|event| event.amount)
                    .sum::<u64>(),
                witness.byte_cost
            );
            assert_eq!(
                processed[0].cost.cost,
                u64::try_from(witness.events.len()).unwrap() + witness.byte_cost
            );
            assert_eq!(
                u64::try_from(admitted_realized).unwrap(),
                authority_realized + byte_realized
            );
            assert_eq!(fee, 1);
            assert_eq!(certificate.fee_recipient, block_data.sender.bytes);
            assert_eq!(
                system_vault_balance(&runtime_manager, &play_post, &payer_address).await,
                initial_payer - i64::try_from(authority_realized + byte_realized + fee).unwrap()
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &play_post, &proposer_address).await,
                initial_proposer + i64::try_from(fee).unwrap()
                    - casper::rust::util::rholang::costacc::VALIDATOR_HANDLER_COST_PER_DEPLOY
            );

            let mut tampered = processed;
            let byte_event = &mut tampered[0]
                .authority_cost_witness
                .as_mut()
                .unwrap()
                .byte_events[0];
            byte_event.kind = if byte_event.kind == 0 { 2 } else { 0 };
            let error = runtime_manager
                .replay_compute_state(
                    &start_state,
                    tampered,
                    processed_system,
                    &block_data,
                    None,
                    false,
                )
                .await
                .expect_err("replay must reject a byte-event kind that differs from execution");
            assert!(
                error
                    .to_string()
                    .contains("replay quantitative byte trace differs from the committed witness"),
                "{error:?}"
            );
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn state_bound_execution_observes_the_authenticated_pre_reservation_vault_balance() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let payer_key = genesis_context.genesis_vaults[0].0.clone();
            let payer_address =
                VaultAddress::from_public_key(&genesis_context.genesis_vaults[0].1).unwrap();
            let initial_payer =
                system_vault_balance(&runtime_manager, &start_state, &payer_address).await;
            let source = format!(
                r#"
                new rl(`rho:registry:lookup`), systemVaultCh, vaultCh, balanceCh in {{
                  rl!(`rho:vault:system`, *systemVaultCh) |
                  for (@(_, systemVault) <- systemVaultCh) {{
                    @systemVault!("find", "{}", *vaultCh) |
                    for (@result <- vaultCh) {{
                      match result {{
                        (true, vault) => {{
                          @vault!("balance", *balanceCh) |
                          for (@balance <- balanceCh) {{
                            @"observed-pre-reservation-balance"!(balance)
                          }}
                        }}
                        _ => {{ @"observed-pre-reservation-balance"!(-1) }}
                      }}
                    }}
                  }}
                }}
                "#,
                payer_address.to_base58()
            );
            let deploy = construct_deploy::source_deploy(
                source,
                1,
                None,
                None,
                Some(payer_key),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let cosigned =
                crypto::rust::signatures::signed::Cosigned::from_single_signer(deploy).unwrap();
            let block_data = BlockData {
                time_stamp: 3,
                block_number: 2,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 2,
            };
            let admission = runtime_manager
                .certify_state_bound_admission(
                    &start_state,
                    vec![cosigned],
                    &block_data,
                    &HashMap::new(),
                )
                .await
                .unwrap();
            assert_eq!(admission.outcome().admitted.len(), 1);
            let close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    block_data.sender.clone(),
                    block_data.seq_num,
                ),
            );
            let (play_post, processed, processed_system, _) = runtime_manager
                .compute_state_with_bonds_cosigned_admitted(admission, vec![
                    casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(close),
                ])
                .await
                .unwrap();
            let observed = runtime_manager
                .get_data(
                    play_post.clone(),
                    &new_gstring_par(
                        "observed-pre-reservation-balance".to_string(),
                        Vec::new(),
                        false,
                    ),
                )
                .await
                .unwrap();
            assert_eq!(observed.len(), 1);
            assert_eq!(RhoNumber::unapply(&observed[0]), Some(initial_payer));

            let witness = processed[0].authority_cost_witness.as_ref().unwrap();
            let certificate = processed[0].authority_funding_certificate.as_ref().unwrap();
            let realized = witness
                .settlement
                .iter()
                .map(|resource| resource.amount)
                .sum::<u64>();
            let byte_realized = witness
                .byte_settlement
                .iter()
                .map(|resource| resource.amount)
                .sum::<u64>();
            let fee = certificate
                .fee_allocation
                .iter()
                .map(|resource| resource.amount)
                .sum::<u64>();
            assert_eq!(
                system_vault_balance(&runtime_manager, &play_post, &payer_address).await,
                initial_payer - i64::try_from(realized + byte_realized + fee).unwrap()
            );

            let replay_post = runtime_manager
                .replay_compute_state(
                    &start_state,
                    processed,
                    processed_system,
                    &block_data,
                    None,
                    false,
                )
                .await
                .unwrap();
            assert_eq!(play_post, replay_post);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn state_bound_reservation_failure_rolls_back_the_retained_user_execution() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let payer_key = genesis_context.genesis_vaults[0].0.clone();
            let payer_address =
                VaultAddress::from_public_key(&genesis_context.genesis_vaults[0].1).unwrap();
            let target_address =
                VaultAddress::from_public_key(&genesis_context.genesis_vaults[1].1).unwrap();
            let proposer_address =
                VaultAddress::from_public_key(&genesis_context.validator_pks()[0]).unwrap();
            let initial_payer =
                system_vault_balance(&runtime_manager, &start_state, &payer_address).await;
            let initial_target =
                system_vault_balance(&runtime_manager, &start_state, &target_address).await;
            let initial_proposer =
                system_vault_balance(&runtime_manager, &start_state, &proposer_address).await;
            let source = format!(
                r#"
                new rl(`rho:registry:lookup`), systemVaultCh,
                    payerCh, targetCh, authKeyCh, transferCh,
                    deployerId(`rho:system:deployerId`) in {{
                  rl!(`rho:vault:system`, *systemVaultCh) |
                  for (@(_, systemVault) <- systemVaultCh) {{
                    @systemVault!("find", "{}", *payerCh) |
                    @systemVault!("find", "{}", *targetCh) |
                    @systemVault!("deployerAuthKey", *deployerId, *authKeyCh) |
                    for (@(true, payer) <- payerCh & @(true, _) <- targetCh & key <- authKeyCh) {{
                      @payer!("transfer", "{}", {}, *key, *transferCh) |
                      for (@result <- transferCh) {{
                        match result {{
                          (true, _) => {{ @"rolled-back-cost-reservation"!(true) }}
                          _ => {{ @"rolled-back-cost-reservation"!(false) }}
                        }}
                      }}
                    }}
                  }}
                }}
                "#,
                payer_address.to_base58(),
                target_address.to_base58(),
                target_address.to_base58(),
                initial_payer
            );
            let deploy = construct_deploy::source_deploy(
                source,
                1,
                None,
                None,
                Some(payer_key),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let cosigned =
                crypto::rust::signatures::signed::Cosigned::from_single_signer(deploy).unwrap();
            let block_data = BlockData {
                time_stamp: 4,
                block_number: 2,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 2,
            };
            let admission = runtime_manager
                .certify_state_bound_admission(
                    &start_state,
                    vec![cosigned],
                    &block_data,
                    &HashMap::new(),
                )
                .await
                .unwrap();
            assert!(admission.outcome().admitted.is_empty());
            assert_eq!(admission.outcome().rejected.len(), 1);

            let close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    block_data.sender.clone(),
                    block_data.seq_num,
                ),
            );
            let (play_post, processed, processed_system, _) = runtime_manager
                .compute_state_with_bonds_cosigned_admitted(admission, vec![
                    casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(close),
                ])
                .await
                .unwrap();
            assert!(processed.is_empty());
            assert_eq!(
                system_vault_balance(&runtime_manager, &play_post, &payer_address).await,
                initial_payer
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &play_post, &target_address).await,
                initial_target
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &play_post, &proposer_address).await,
                initial_proposer
            );
            let markers = runtime_manager
                .get_data(
                    play_post.clone(),
                    &new_gstring_par(
                        "rolled-back-cost-reservation".to_string(),
                        Vec::new(),
                        false,
                    ),
                )
                .await
                .unwrap();
            assert!(markers.is_empty());

            let replay_post = runtime_manager
                .replay_compute_state(
                    &start_state,
                    processed,
                    processed_system,
                    &block_data,
                    None,
                    false,
                )
                .await
                .unwrap();
            assert_eq!(play_post, replay_post);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_deploy_stack_transfer_is_vault_backed_consumed_and_replayed() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let payer_key = genesis_context.genesis_vaults[0].0.clone();
            let payer_address =
                VaultAddress::from_public_key(&genesis_context.genesis_vaults[0].1).unwrap();
            let initial_payer =
                system_vault_balance(&runtime_manager, &start_state, &payer_address).await;
            let stack_signature =
                Compiler::source_to_adt("t :: ()").unwrap().cost_stacks[0].cells[0].clone();
            let stack_payer = vault_payer(&stack_signature).unwrap();
            let stack_funding = 100_000_i64;
            assert_eq!(
                system_vault_balance(&runtime_manager, &start_state, &stack_payer.address).await,
                0
            );
            let funding_source = format!(
                r#"
                new rl(`rho:registry:lookup`), systemVaultCh,
                    payerCh, targetCh, authKeyCh, transferCh,
                    deployerId(`rho:system:deployerId`) in {{
                  rl!(`rho:vault:system`, *systemVaultCh) |
                  for (@(_, systemVault) <- systemVaultCh) {{
                    @systemVault!("find", "{}", *payerCh) |
                    @systemVault!("findOrCreate", "{}", *targetCh) |
                    @systemVault!("deployerAuthKey", *deployerId, *authKeyCh) |
                    for (@(true, payer) <- payerCh & @(true, _) <- targetCh & key <- authKeyCh) {{
                      @payer!("transfer", "{}", {}, *key, *transferCh) |
                      for (@result <- transferCh) {{
                        @"same-deploy-stack-funded"!(result)
                      }}
                    }}
                  }}
                }}
                "#,
                payer_address.to_base58(),
                stack_payer.address.to_base58(),
                stack_payer.address.to_base58(),
                stack_funding,
            );
            let funding = construct_deploy::source_deploy(
                funding_source,
                1,
                None,
                None,
                Some(payer_key.clone()),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let funding =
                crypto::rust::signatures::signed::Cosigned::from_single_signer(funding).unwrap();
            let funding_block = BlockData {
                time_stamp: 1,
                block_number: 2,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 2,
            };
            let funding_admission = runtime_manager
                .certify_state_bound_admission(
                    &start_state,
                    vec![funding],
                    &funding_block,
                    &HashMap::new(),
                )
                .await
                .unwrap();
            assert_eq!(funding_admission.outcome().admitted.len(), 1);
            let funding_close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    funding_block.sender.clone(),
                    funding_block.seq_num,
                ),
            );
            let (funded_state, funding_processed, funding_system, _) = runtime_manager
                .compute_state_with_bonds_cosigned_admitted(funding_admission, vec![
                    casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                        funding_close,
                    ),
                ])
                .await
                .unwrap();
            assert_eq!(funding_processed.len(), 1);
            let funding_certificate = funding_processed[0]
                .authority_funding_certificate
                .as_ref()
                .unwrap();
            let funding_witness = funding_processed[0]
                .authority_cost_witness
                .as_ref()
                .unwrap();
            let funding_charge = funding_witness
                .settlement
                .iter()
                .chain(&funding_witness.byte_settlement)
                .map(|resource| resource.amount)
                .sum::<u64>()
                + funding_certificate
                    .fee_allocation
                    .iter()
                    .map(|resource| resource.amount)
                    .sum::<u64>();
            let replayed_funding = runtime_manager
                .replay_compute_state(
                    &start_state,
                    funding_processed,
                    funding_system,
                    &funding_block,
                    None,
                    false,
                )
                .await
                .unwrap();
            assert_eq!(funded_state, replayed_funding);
            assert_eq!(
                system_vault_balance(&runtime_manager, &funded_state, &stack_payer.address).await,
                stack_funding
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &funded_state, &payer_address).await,
                initial_payer - stack_funding - i64::try_from(funding_charge).unwrap()
            );
            let source = r#"t :: t :: () | {% for(_ <- @"x"){ Nil } %}[ t ] | @"x"!(0)"#;
            let deploy = construct_deploy::source_deploy(
                source.to_string(),
                2,
                None,
                None,
                Some(payer_key),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let cosigned =
                crypto::rust::signatures::signed::Cosigned::from_single_signer(deploy).unwrap();
            let block_data = BlockData {
                time_stamp: 2,
                block_number: 3,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 3,
            };
            let prepare_runtime = runtime_manager.spawn_runtime().await;
            let mut prepare_ops = RuntimeOps::new(prepare_runtime);
            prepare_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&funded_state))
                .await
                .unwrap();
            let reader = acceptance::RuntimeOpsSupplyReader {
                runtime_ops: &prepare_ops,
                pre_state_root: funded_state.as_ref().try_into().unwrap(),
            };
            let canonical = Compiler::source_to_adt(source).unwrap();
            let plan = rholang::rust::interpreter::accounting::delta_sigma::static_authority_plan(
                &canonical,
                &accounting::funding_sig(&cosigned),
            )
            .unwrap();
            assert_eq!(plan.external_reservation.0.len(), 1, "{plan:?}");
            assert_eq!(
                plan.external_reservation.0.values().sum::<u64>(),
                3,
                "{plan:?}"
            );
            let prepared = acceptance::prepare_authority_reservation(
                &cosigned,
                &reader,
                &block_data.sender.bytes,
            )
            .await
            .unwrap();
            assert_eq!(prepared.certificate.allocation.0.values().sum::<u64>(), 3);
            let admission = runtime_manager
                .certify_state_bound_admission(
                    &funded_state,
                    vec![cosigned],
                    &block_data,
                    &HashMap::new(),
                )
                .await
                .unwrap();
            let close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    block_data.sender.clone(),
                    block_data.seq_num,
                ),
            );
            let (play_post, processed, processed_system, _) = runtime_manager
                .compute_state_with_bonds_cosigned_admitted(admission, vec![
                    casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(close),
                ])
                .await
                .unwrap();
            assert_eq!(processed.len(), 1);
            assert!(!processed[0].is_failed);
            let certificate = processed[0].authority_funding_certificate.as_ref().unwrap();
            let witness = processed[0].authority_cost_witness.as_ref().unwrap();
            assert_eq!(witness.born_stacks.len(), 1);
            let born_id = witness.born_stacks[0].stack_id.clone();
            assert!(witness
                .physical_draws
                .iter()
                .any(|draw| draw.stack_ids.iter().any(|stack_id| stack_id == &born_id)));
            assert!(certificate
                .stack_reservations
                .iter()
                .all(|reservation| reservation.stack_id != born_id));
            let reserved = certificate
                .allocation
                .iter()
                .map(|resource| resource.amount)
                .sum::<u64>();
            let burned = witness
                .settlement
                .iter()
                .map(|resource| resource.amount)
                .sum::<u64>();
            let byte_burned = witness
                .byte_settlement
                .iter()
                .map(|resource| resource.amount)
                .sum::<u64>();
            let fee = certificate
                .fee_allocation
                .iter()
                .map(|resource| resource.amount)
                .sum::<u64>();
            assert_eq!(reserved, 3);
            assert_eq!(burned, 3);
            assert!(byte_burned > 0);
            assert_eq!(fee, 1);
            let stack_debit = witness
                .settlement
                .iter()
                .chain(&witness.byte_settlement)
                .filter(|resource| resource.key.as_ref() == stack_payer.lane_key.as_slice())
                .map(|resource| resource.amount)
                .sum::<u64>();
            let payer_lane =
                accounting::funding_sig_single(&genesis_context.genesis_vaults[0].1.bytes)
                    .lane_hash();
            let payer_debit = witness
                .settlement
                .iter()
                .chain(&witness.byte_settlement)
                .filter(|resource| resource.key.as_ref() == payer_lane.as_slice())
                .map(|resource| resource.amount)
                .sum::<u64>();
            assert!(stack_debit >= 3);
            assert_eq!(stack_debit + payer_debit, burned + byte_burned);
            assert_eq!(
                system_vault_balance(&runtime_manager, &play_post, &stack_payer.address).await,
                stack_funding - i64::try_from(stack_debit).unwrap()
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &play_post, &payer_address).await,
                initial_payer
                    - stack_funding
                    - i64::try_from(funding_charge + payer_debit + fee).unwrap()
            );

            let replay_post = runtime_manager
                .replay_compute_state(
                    &funded_state,
                    processed,
                    processed_system,
                    &block_data,
                    None,
                    false,
                )
                .await
                .unwrap();
            assert_eq!(play_post, replay_post);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn physical_rejection_rolls_back_before_later_state_bound_execution() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let rejected_key = PrivateKey::from_bytes(&[0x51; 32]);
            let rejected = construct_deploy::source_deploy(
                r#"new x in { x!(0) | for(@0 <- x){ @"rollback-proof"!(1) } }"#.to_string(),
                1,
                None,
                None,
                Some(rejected_key.clone()),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let accepted_key = genesis_context.genesis_vaults[0].0.clone();
            let accepted = construct_deploy::source_deploy(
                r#"new x in { x!(0) | for(@0 <- x){ @"accepted-proof"!(1) } }"#.to_string(),
                2,
                None,
                None,
                Some(accepted_key.clone()),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let rejected = crypto::rust::signatures::signed::Cosigned::create_single_envelope(
                rejected.data,
                Box::new(Secp256k1),
                rejected_key,
            )
            .unwrap();
            let accepted = crypto::rust::signatures::signed::Cosigned::create_single_envelope(
                accepted.data,
                Box::new(Secp256k1),
                accepted_key,
            )
            .unwrap();
            let rejected_funding = accounting::funding_sig(&rejected);
            let rejected_signature = sig_to_cost_signature(&rejected_funding).unwrap();
            let rejected_funding_channel = supply::supply_channel(&rejected_funding);

            let seed_runtime = runtime_manager.spawn_runtime().await;
            let mut seed_ops = RuntimeOps::new(seed_runtime);
            seed_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&start_state))
                .await
                .unwrap();
            seed_ops
                .runtime
                .reducer
                .space
                .produce(
                    rejected_funding_channel,
                    ListParWithRandom {
                        pars: Vec::new(),
                        random_state: vec![41; 64],
                        cost_authority: None,
                        cost_stack: Some(CostStack {
                            cells: vec![
                                rejected_signature.clone(),
                                rejected_signature.clone(),
                                rejected_signature,
                            ],
                        }),
                    },
                    false,
                )
                .await
                .unwrap();
            let seeded_state = seed_ops
                .runtime
                .create_checkpoint()
                .await
                .root
                .to_bytes_prost();
            let block_data = BlockData {
                time_stamp: 3,
                block_number: 2,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 2,
            };
            let accepted_sig = accepted.primary().sig.clone();
            let rejected_record =
                ProcessedDeploy::admission_rejected(&rejected, seeded_state.clone());
            let rejected_id = rejected_record
                .deploy_id_for_protocol(genesis_block.header.version)
                .unwrap();
            let admission = runtime_manager
                .certify_state_bound_admission(
                    &seeded_state,
                    vec![accepted.clone(), rejected],
                    &block_data,
                    &HashMap::new(),
                )
                .await
                .unwrap();
            assert_eq!(
                admission
                    .outcome()
                    .admitted
                    .iter()
                    .map(|deploy| deploy.primary().sig.clone())
                    .collect::<Vec<_>>(),
                vec![accepted_sig.clone()]
            );
            assert_eq!(admission.outcome().rejected, vec![rejected_id]);

            let close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    block_data.sender.clone(),
                    block_data.seq_num,
                ),
            );
            let (play_post, mut processed, processed_system, _) = runtime_manager
                .compute_state_with_bonds_cosigned_admitted(admission, vec![
                    casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(close),
                ])
                .await
                .unwrap();
            assert_eq!(processed.len(), 1);
            assert_eq!(processed[0].deploy.sig, accepted_sig);
            processed.push(rejected_record);

            let rollback_channel = ParBuilderUtil::mk_term(r#""rollback-proof""#).unwrap();
            let accepted_channel = ParBuilderUtil::mk_term(r#""accepted-proof""#).unwrap();
            assert!(runtime_manager
                .get_data(play_post.clone(), &rollback_channel)
                .await
                .unwrap()
                .is_empty());
            assert_eq!(
                runtime_manager
                    .get_data(play_post.clone(), &accepted_channel)
                    .await
                    .unwrap(),
                vec![ParBuilderUtil::mk_term("1").unwrap()]
            );

            let block = BlockMessage {
                block_hash: vec![31; 32].into(),
                header: Header {
                    parents_hash_list: vec![vec![30; 32].into()],
                    timestamp: block_data.time_stamp,
                    version: genesis_block.header.version,
                    extra_bytes: Vec::<u8>::new().into(),
                    sender_bond_generation: Some(
                        models::rust::bond_generation::BondGeneration::GENESIS,
                    ),
                    objective_equivocation_evidence_delta: Vec::new(),
                    finalized_floor: None,
                },
                body: Body {
                    state: F1r3flyState {
                        pre_state_hash: seeded_state.clone(),
                        post_state_hash: play_post.clone(),
                        bonds: Vec::new(),
                        bond_generations: Vec::new(),
                        active_validators: Vec::new(),
                        block_number: block_data.block_number,
                    },
                    deploys: processed,
                    rejected_deploys: Vec::new(),
                    rejected_state_effects: Vec::new(),
                    system_deploys: processed_system,
                    extra_bytes: Vec::<u8>::new().into(),
                    applied_from_scope: Vec::new(),
                    merge_base: Vec::<u8>::new().into(),
                },
                justifications: Vec::new(),
                sender: block_data.sender.bytes.clone(),
                seq_num: block_data.seq_num,
                sig: Vec::<u8>::new().into(),
                sig_algorithm: String::new(),
                shard_id: genesis_block.shard_id.clone(),
                extra_bytes: Vec::<u8>::new().into(),
                finalized_floor_certificate: None,
            };
            let replay_post = runtime_manager
                .replay_block_from_consensus_data(&seeded_state, &block, None)
                .await
                .unwrap();
            assert_eq!(play_post, replay_post);

            let mut forged = block;
            forged.body.deploys = vec![ProcessedDeploy::admission_rejected(
                &accepted,
                seeded_state.clone(),
            )];
            let forged_result = runtime_manager
                .replay_block_from_consensus_data(&seeded_state, &forged, None)
                .await;
            assert!(matches!(
                forged_result,
                Err(CasperError::ReplayFailure(
                    ReplayFailure::ReplayAdmissionMismatch { .. }
                ))
            ));
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn candidate_minted_stack_cannot_fund_its_own_state_bound_settlement() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let payer_key = PrivateKey::from_bytes(&[0x53; 32]);
            let payer_address =
                VaultAddress::from_public_key(&Secp256k1.to_public(&payer_key)).unwrap();
            let deploy = construct_deploy::source_deploy(
                r#"minted :: minted :: () | {% new x in { x!(0) | for(@0 <- x){ @"uncommitted"!(1) } } %}[ minted ]"#.to_string(),
                1,
                None,
                None,
                Some(payer_key),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let deploy =
                crypto::rust::signatures::signed::Cosigned::from_single_signer(deploy).unwrap();
            let deploy_sig = deploy.primary().sig.clone();
            let seeded_state =
                protocol_mint_to_vault(&runtime_manager, &start_state, &payer_address, 2, 43).await;

            let block_data = BlockData {
                time_stamp: 2,
                block_number: 2,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 2,
            };
            let admission = runtime_manager
                .certify_state_bound_admission(
                    &seeded_state,
                    vec![deploy],
                    &block_data,
                    &HashMap::new(),
                )
                .await
                .unwrap();
            assert!(admission.outcome().admitted.is_empty());
            assert_eq!(
                admission.outcome().rejected,
                vec![crate::legacy_deploy_id(&deploy_sig)]
            );

            let close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    block_data.sender.clone(),
                    block_data.seq_num,
                ),
            );
            let (play_post, processed, processed_system, _) = runtime_manager
                .compute_state_with_bonds_cosigned_admitted(admission, vec![
                    casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(close),
                ])
                .await
                .unwrap();
            assert!(processed.is_empty());

            let minted = Sig::Ground(
                rholang::rust::interpreter::compiler::normalizer::cost_accounting::sig::canon_ground(
                    "minted",
                ),
            );
            assert!(runtime_manager
                .get_data(play_post.clone(), &supply::supply_channel(&minted))
                .await
                .unwrap()
                .is_empty());
            assert!(runtime_manager
                .get_data(
                    play_post.clone(),
                    &ParBuilderUtil::mk_term(r#""uncommitted""#).unwrap(),
                )
                .await
                .unwrap()
                .is_empty());
            assert_eq!(
                system_vault_balance(&runtime_manager, &play_post, &payer_address).await,
                2
            );

            let replay_post = runtime_manager
                .replay_compute_state(
                    &seeded_state,
                    processed,
                    processed_system,
                    &block_data,
                    None,
                    false,
                )
                .await
                .unwrap();
            assert_eq!(play_post, replay_post);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exactly_funded_stack_transfer_commits_and_replays_conservatively() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            use models::rhoapi::cost_signature::Value;

            let start_state = genesis_block.body.state.post_state_hash.clone();
            let payer_key = PrivateKey::from_bytes(&[0x54; 32]);
            let payer_address =
                VaultAddress::from_public_key(&Secp256k1.to_public(&payer_key)).unwrap();
            let deploy = construct_deploy::source_deploy(
                r#"a :: b :: () | @"committed-stack-transfer"!(1)"#.to_string(),
                1,
                None,
                None,
                Some(payer_key),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let deploy =
                crypto::rust::signatures::signed::Cosigned::from_single_signer(deploy).unwrap();
            let (physical_cost, byte_cost, byte_debit) =
                measured_authority_cost(&runtime_manager, &start_state, &deploy).await;
            assert_eq!(physical_cost, 2);
            assert!(byte_cost > 0);
            let seeded_state = protocol_mint_to_vault(
                &runtime_manager,
                &start_state,
                &payer_address,
                exact_vault_funding(physical_cost, byte_debit),
                54,
            )
            .await;
            let block_data = BlockData {
                time_stamp: 3,
                block_number: 2,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 2,
            };
            let admission = runtime_manager
                .certify_state_bound_admission(
                    &seeded_state,
                    vec![deploy],
                    &block_data,
                    &HashMap::new(),
                )
                .await
                .unwrap();
            assert_eq!(admission.outcome().admitted.len(), 1);
            assert!(admission.outcome().stack_pops.is_empty());
            let close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    block_data.sender.clone(),
                    block_data.seq_num,
                ),
            );
            let (play_post, processed, processed_system, _) = runtime_manager
                .compute_state_with_bonds_cosigned_admitted(admission, vec![
                    casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(close),
                ])
                .await
                .unwrap();
            assert_eq!(processed.len(), 1);
            assert_eq!(
                processed[0]
                    .authority_cost_witness
                    .as_ref()
                    .unwrap()
                    .events
                    .len(),
                2
            );

            assert_eq!(
                system_vault_balance(&runtime_manager, &play_post, &payer_address).await,
                0
            );
            let a = CostSignature {
                value: Some(Value::Ground(
                    rholang::rust::interpreter::compiler::normalizer::cost_accounting::sig::canon_ground(
                        "a",
                    ),
                )),
            };
            let inventory = supply::decode_purse_inventory(
                &runtime_manager
                    .get_data_datums(
                        play_post.clone(),
                        &supply::supply_channel(&Sig::Ground(
                            rholang::rust::interpreter::compiler::normalizer::cost_accounting::sig::canon_ground(
                                "a",
                            ),
                        )),
                    )
                    .await
                    .unwrap(),
                &a,
            )
            .unwrap();
            assert_eq!(inventory.stacks.len(), 1);
            assert_eq!(inventory.stacks[0].stack.cells.len(), 2);
            assert_eq!(
                runtime_manager
                    .get_data(
                        play_post.clone(),
                        &new_gstring_par(
                            "committed-stack-transfer".to_string(),
                            Vec::new(),
                            false,
                        ),
                    )
                    .await
                    .unwrap()
                    .len(),
                1
            );

            let replay_post = runtime_manager
                .replay_compute_state(
                    &seeded_state,
                    processed,
                    processed_system,
                    &block_data,
                    None,
                    false,
                )
                .await
                .unwrap();
            assert_eq!(play_post, replay_post);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_equal_stack_literals_have_distinct_conserving_transfers() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            use models::rhoapi::cost_signature::Value;

            let start_state = genesis_block.body.state.post_state_hash.clone();
            let payer_key = PrivateKey::from_bytes(&[0x55; 32]);
            let payer_address =
                VaultAddress::from_public_key(&Secp256k1.to_public(&payer_key)).unwrap();
            let deploy = construct_deploy::source_deploy(
                r#"a :: b :: () | a :: b :: ()"#.to_string(),
                1,
                None,
                None,
                Some(payer_key),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let deploy =
                crypto::rust::signatures::signed::Cosigned::from_single_signer(deploy).unwrap();
            let (physical_cost, byte_cost, byte_debit) =
                measured_authority_cost(&runtime_manager, &start_state, &deploy).await;
            assert_eq!(physical_cost, 4);
            assert!(byte_cost > 0);
            let seeded_state = protocol_mint_to_vault(
                &runtime_manager,
                &start_state,
                &payer_address,
                exact_vault_funding(physical_cost, byte_debit),
                55,
            )
            .await;
            let block_data = BlockData {
                time_stamp: 4,
                block_number: 2,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 2,
            };
            let admission = runtime_manager
                .certify_state_bound_admission(
                    &seeded_state,
                    vec![deploy],
                    &block_data,
                    &HashMap::new(),
                )
                .await
                .unwrap();
            assert_eq!(admission.outcome().admitted.len(), 1);
            assert!(admission.outcome().stack_pops.is_empty());
            let close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    block_data.sender.clone(),
                    block_data.seq_num,
                ),
            );
            let (play_post, processed, processed_system, _) = runtime_manager
                .compute_state_with_bonds_cosigned_admitted(admission, vec![
                    casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(close),
                ])
                .await
                .unwrap();
            assert_eq!(processed.len(), 1);
            let events = &processed[0]
                .authority_cost_witness
                .as_ref()
                .unwrap()
                .events;
            assert_eq!(events.len(), 4);
            assert_eq!(
                events
                    .iter()
                    .map(|event| event.event_id.clone())
                    .collect::<std::collections::BTreeSet<_>>()
                    .len(),
                4
            );

            let query_runtime = runtime_manager.spawn_runtime().await;
            let mut query_ops = RuntimeOps::new(query_runtime);
            query_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&play_post))
                .await
                .unwrap();
            assert_eq!(
                system_vault_balance(&runtime_manager, &play_post, &payer_address).await,
                0
            );
            let a = CostSignature {
                value: Some(Value::Ground(
                    rholang::rust::interpreter::compiler::normalizer::cost_accounting::sig::canon_ground(
                        "a",
                    ),
                )),
            };
            let inventory = supply::decode_purse_inventory(
                &query_ops
                    .get_data_datums(&supply::supply_channel(&Sig::Ground(
                        rholang::rust::interpreter::compiler::normalizer::cost_accounting::sig::canon_ground(
                            "a",
                        ),
                    )))
                    .await,
                &a,
            )
            .unwrap();
            assert_eq!(inventory.stacks.len(), 2);
            assert!(inventory
                .stacks
                .iter()
                .all(|stack| stack.stack.cells.len() == 2));

            let replay_post = runtime_manager
                .replay_compute_state(
                    &seeded_state,
                    processed,
                    processed_system,
                    &block_data,
                    None,
                    false,
                )
                .await
                .unwrap();
            assert_eq!(play_post, replay_post);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn certified_execution_settles_split_surfaces_from_persistent_stacks() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            use models::rhoapi::cost_signature::Value;

            let start_state = genesis_block.body.state.post_state_hash.clone();
            let payer_key = PrivateKey::from_bytes(&[0x56; 32]);
            let payer_address =
                VaultAddress::from_public_key(&Secp256k1.to_public(&payer_key)).unwrap();
            let deploy = construct_deploy::source_deploy_now_full(
                "new x in { x!(0) | for(@0 <- x){ Nil } }".to_string(),
                Some(1),
                None,
                Some(payer_key),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let cosigned =
                crypto::rust::signatures::signed::Cosigned::from_single_signer(deploy).unwrap();
            let funding = accounting::funding_sig(&cosigned);
            let signature = sig_to_cost_signature(&funding).unwrap();
            let funding_channel = supply::supply_channel(&funding);
            let first_tail = CostSignature {
                value: Some(Value::Ground(b"first-tail".to_vec())),
            };
            let second_tail = CostSignature {
                value: Some(Value::Ground(b"second-tail".to_vec())),
            };
            let seed_runtime = runtime_manager.spawn_runtime().await;
            let mut seed_ops = RuntimeOps::new(seed_runtime);
            seed_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&start_state))
                .await
                .unwrap();
            for (tail, random_state) in [
                (first_tail.clone(), vec![31; 64]),
                (second_tail.clone(), vec![32; 64]),
            ] {
                seed_ops
                    .runtime
                    .reducer
                    .space
                    .produce(
                        funding_channel.clone(),
                        ListParWithRandom {
                            pars: Vec::new(),
                            random_state,
                            cost_authority: None,
                            cost_stack: Some(CostStack {
                                cells: vec![signature.clone(), tail],
                            }),
                        },
                        false,
                    )
                    .await
                    .unwrap();
            }
            let stacks_state = seed_ops
                .runtime
                .create_checkpoint()
                .await
                .root
                .to_bytes_prost();
            let (physical_cost, byte_cost, byte_debit) =
                measured_authority_cost(&runtime_manager, &stacks_state, &cosigned).await;
            assert_eq!(physical_cost, 2);
            assert!(byte_debit >= byte_cost);
            let seeded_state = protocol_mint_to_vault(
                &runtime_manager,
                &stacks_state,
                &payer_address,
                exact_vault_funding(0, byte_debit),
                33,
            )
            .await;
            assert_eq!(
                system_vault_balance(&runtime_manager, &seeded_state, &payer_address).await,
                exact_vault_funding(0, byte_debit)
            );

            let block_data = BlockData {
                time_stamp: 2,
                block_number: 2,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 2,
            };
            let admission = runtime_manager
                .certify_state_bound_admission(
                    &seeded_state,
                    vec![cosigned],
                    &block_data,
                    &HashMap::new(),
                )
                .await
                .unwrap();
            assert_eq!(admission.outcome().admitted.len(), 1);
            assert_eq!(
                admission
                    .outcome()
                    .debits
                    .values()
                    .map(|debit| debit.amount)
                    .sum::<i64>(),
                i64::try_from(byte_debit).unwrap()
            );
            assert_eq!(
                admission
                    .outcome()
                    .fee_debits
                    .values()
                    .map(|debit| debit.amount)
                    .sum::<i64>(),
                1
            );
            assert_eq!(admission.outcome().stack_pops.len(), 2);
            assert_eq!(
                admission
                    .outcome()
                    .stack_pops
                    .values()
                    .copied()
                    .sum::<u64>(),
                2
            );

            let close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    block_data.sender.clone(),
                    block_data.seq_num,
                ),
            );
            let (play_post, processed, processed_system, _) = runtime_manager
                .compute_state_with_bonds_cosigned_admitted(admission, vec![
                    casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(close),
                ])
                .await
                .unwrap();
            assert_eq!(processed_system.len(), 1);
            assert!(recorded_removal_pair_count(&processed[0].deploy_log) >= 2);
            let replay_post = runtime_manager
                .replay_compute_state(
                    &seeded_state,
                    processed,
                    processed_system,
                    &block_data,
                    None,
                    false,
                )
                .await
                .unwrap();
            assert_eq!(play_post, replay_post);

            let query_runtime = runtime_manager.spawn_runtime().await;
            let mut query_ops = RuntimeOps::new(query_runtime);
            query_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&play_post))
                .await
                .unwrap();
            assert_eq!(
                system_vault_balance(&runtime_manager, &play_post, &payer_address).await,
                0
            );
            for tail in [first_tail, second_tail] {
                let tail_sig = match tail.value.as_ref().unwrap() {
                    Value::Ground(bytes) => Sig::Ground(bytes.clone()),
                    _ => unreachable!(),
                };
                let inventory = supply::decode_purse_inventory(
                    &query_ops
                        .get_data_datums(&supply::supply_channel(&tail_sig))
                        .await,
                    &tail,
                )
                .unwrap();
                assert_eq!(inventory.stacks.len(), 1);
                assert_eq!(inventory.stacks[0].stack.cells, vec![tail]);
            }
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn candidate_created_stack_cannot_supply_same_deploy_lollipop_byte_capacity() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let signer = genesis_context.genesis_vaults[0].0.clone();
            let payer_address =
                VaultAddress::from_public_key(&genesis_context.genesis_vaults[0].1).unwrap();
            let initial_payer =
                system_vault_balance(&runtime_manager, &start_state, &payer_address).await;
            let deposit = construct_deploy::source_deploy(
                r#"new slot in { {% for(_ <- @"trigger"){ new x in { x!(0) | for(@0 <- x){ Nil } } } %}[ a -o slot ] | a :: () | slot :: slot :: () | @"slot-registry"!(*slot) }"#
                    .to_string(),
                1,
                None,
                None,
                Some(signer.clone()),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let deposit =
                crypto::rust::signatures::signed::Cosigned::from_single_signer(deposit).unwrap();
            let deposit_id = deposit.primary().sig.clone();

            let deposit_block = BlockData {
                time_stamp: 1,
                block_number: 2,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 2,
            };
            let prepare_runtime = runtime_manager.spawn_runtime().await;
            let mut prepare_ops = RuntimeOps::new(prepare_runtime);
            prepare_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&start_state))
                .await
                .unwrap();
            let prepare_reader = acceptance::RuntimeOpsSupplyReader {
                runtime_ops: &prepare_ops,
                pre_state_root: start_state.as_ref().try_into().unwrap(),
            };
            let prepared = acceptance::prepare_authority_reservation(
                &deposit,
                &prepare_reader,
                &deposit_block.sender.bytes,
            )
            .await
            .unwrap();
            assert_eq!(
                prepared
                    .certificate
                    .allocation
                    .0
                    .values()
                    .copied()
                    .sum::<u64>(),
                4
            );
            let deposit_admission = runtime_manager
                .certify_state_bound_admission(
                    &start_state,
                    vec![deposit],
                    &deposit_block,
                    &HashMap::new(),
                )
                .await
                .unwrap();
            assert!(deposit_admission.outcome().admitted.is_empty());
            assert_eq!(
                deposit_admission.outcome().rejected,
                vec![crate::legacy_deploy_id(&deposit_id)]
            );
            assert!(deposit_admission.outcome().stack_pops.is_empty());
            assert!(deposit_admission.outcome().purse_stacks.is_empty());
            assert_eq!(deposit_admission.pre_state(), &start_state);
            assert_eq!(
                system_vault_balance(&runtime_manager, &start_state, &payer_address).await,
                initial_payer
            );
            assert!(runtime_manager
                .get_data(
                    start_state,
                    &new_gstring_par("slot-registry".to_string(), Vec::new(), false),
                )
                .await
                .unwrap()
                .is_empty());
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wallet_funded_lollipop_slot_settles_across_deploys_and_replays() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let installer_key = genesis_context.genesis_vaults[0].0.clone();
            let sponsor_key = genesis_context.genesis_vaults[1].0.clone();
            let sponsor_address =
                VaultAddress::from_public_key(&genesis_context.genesis_vaults[1].1).unwrap();
            let gateway_key = genesis_context.genesis_vaults[2].0.clone();
            let gateway_public_key = hex::encode(&genesis_context.genesis_vaults[2].1.bytes);
            let purse_funding = 100_000_i64;
            let installer_source = r#"new entry, slot, entryAddressCh, slotAddressCh,
                VaultAddress(`rho:vault:address`),
                DeployerIdOps(`rho:system:deployerId:ops`) in {
              for (@request, deployerId <= @"agent-trigger") {
                new publicKeyCh in {
                  DeployerIdOps!("pubKeyBytes", *deployerId, *publicKeyCh) |
                  for (@publicKey <- publicKeyCh) {
                    if (publicKey == "GATEWAY_PUBLIC_KEY".hexToBytes()) {
                      {% for(@accepted <- entry){
                        new x in { x!(0) | for(@0 <- x){ @"agent-ran"!(true) } }
                      } %}[ entry -o slot ] |
                      entry!(request)
                    }
                  }
                }
              } |
              entry :: () |
              VaultAddress!("fromUnforgeable", *entry, *entryAddressCh) |
              for (@entryAddress <- entryAddressCh) {
                @"agent-entry-address"!!(entryAddress)
              } |
              VaultAddress!("fromUnforgeable", *slot, *slotAddressCh) |
              for (@slotAddress <- slotAddressCh) {
                @"agent-slot-address"!!(slotAddress)
              }
            }"#
            .replace("GATEWAY_PUBLIC_KEY", &gateway_public_key);
            let installer = construct_deploy::source_deploy(
                installer_source,
                1,
                None,
                None,
                Some(installer_key.clone()),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let installer =
                crypto::rust::signatures::signed::Cosigned::from_single_signer(installer).unwrap();
            let install_block = BlockData {
                time_stamp: 1,
                block_number: 2,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 2,
            };
            let install_admission = runtime_manager
                .certify_state_bound_admission(
                    &start_state,
                    vec![installer],
                    &install_block,
                    &HashMap::new(),
                )
                .await
                .unwrap();
            assert_eq!(install_admission.outcome().admitted.len(), 1);
            let install_close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    install_block.sender.clone(),
                    install_block.seq_num,
                ),
            );
            let (installed_state, installed, installed_system, _) = runtime_manager
                .compute_state_with_bonds_cosigned_admitted(install_admission, vec![
                    casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                        install_close,
                    ),
                ])
                .await
                .unwrap();
            let replayed_install = runtime_manager
                .replay_compute_state(
                    &start_state,
                    installed,
                    installed_system,
                    &install_block,
                    None,
                    false,
                )
                .await
                .unwrap();
            assert_eq!(installed_state, replayed_install);

            let published_entry_address = runtime_manager
                .get_data(
                    installed_state.clone(),
                    &new_gstring_par("agent-entry-address".to_string(), Vec::new(), false),
                )
                .await
                .unwrap();
            assert_eq!(published_entry_address.len(), 1);
            let entry_address =
                VaultAddress::parse(&RhoString::unapply(&published_entry_address[0]).unwrap())
                    .unwrap();
            let published_address = runtime_manager
                .get_data(
                    installed_state.clone(),
                    &new_gstring_par("agent-slot-address".to_string(), Vec::new(), false),
                )
                .await
                .unwrap();
            assert_eq!(published_address.len(), 1);
            let slot_address =
                VaultAddress::parse(&RhoString::unapply(&published_address[0]).unwrap()).unwrap();
            assert_eq!(
                system_vault_balance(&runtime_manager, &installed_state, &slot_address).await,
                0
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &installed_state, &entry_address).await,
                0
            );

            let initial_sponsor =
                system_vault_balance(&runtime_manager, &installed_state, &sponsor_address).await;
            let funding_source = format!(
                r#"
                new rl(`rho:registry:lookup`), systemVaultCh,
                    payerCh, entryCh, slotCh, authKeyCh,
                    entryTransferCh, slotTransferCh,
                    deployerId(`rho:system:deployerId`) in {{
                  rl!(`rho:vault:system`, *systemVaultCh) |
                  for (@(_, systemVault) <- systemVaultCh) {{
                    @systemVault!("find", "{}", *payerCh) |
                    @systemVault!("findOrCreate", "{}", *entryCh) |
                    @systemVault!("findOrCreate", "{}", *slotCh) |
                    @systemVault!("deployerAuthKey", *deployerId, *authKeyCh) |
                    for (@(true, payer) <- payerCh & @(true, _) <- entryCh &
                         @(true, _) <- slotCh & key <- authKeyCh) {{
                      @payer!("transfer", "{}", {}, *key, *entryTransferCh) |
                      for (@entryResult <- entryTransferCh) {{
                        @payer!("transfer", "{}", {}, *key, *slotTransferCh) |
                        for (@slotResult <- slotTransferCh) {{
                          @"agent-slot-funded"!(entryResult, slotResult)
                        }}
                      }}
                    }}
                  }}
                }}
                "#,
                sponsor_address.to_base58(),
                entry_address.to_base58(),
                slot_address.to_base58(),
                entry_address.to_base58(),
                purse_funding,
                slot_address.to_base58(),
                purse_funding,
            );
            let funding = construct_deploy::source_deploy(
                funding_source,
                2,
                None,
                None,
                Some(sponsor_key),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let funding =
                crypto::rust::signatures::signed::Cosigned::from_single_signer(funding).unwrap();
            let funding_block = BlockData {
                time_stamp: 2,
                block_number: 3,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 3,
            };
            let funding_admission = runtime_manager
                .certify_state_bound_admission(
                    &installed_state,
                    vec![funding],
                    &funding_block,
                    &HashMap::new(),
                )
                .await
                .unwrap();
            assert_eq!(funding_admission.outcome().admitted.len(), 1);
            let funding_close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    funding_block.sender.clone(),
                    funding_block.seq_num,
                ),
            );
            let (funded_state, funded, funded_system, _) = runtime_manager
                .compute_state_with_bonds_cosigned_admitted(funding_admission, vec![
                    casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                        funding_close,
                    ),
                ])
                .await
                .unwrap();
            assert_eq!(funded.len(), 1);
            let funding_witness = funded[0].authority_cost_witness.as_ref().unwrap();
            let funding_certificate = funded[0].authority_funding_certificate.as_ref().unwrap();
            let funding_cost = funding_witness
                .settlement
                .iter()
                .map(|resource| resource.amount)
                .sum::<u64>();
            let funding_byte_cost = funding_witness
                .byte_settlement
                .iter()
                .map(|resource| resource.amount)
                .sum::<u64>();
            let funding_fee = funding_certificate
                .fee_allocation
                .iter()
                .map(|resource| resource.amount)
                .sum::<u64>();
            let replayed_funding = runtime_manager
                .replay_compute_state(
                    &installed_state,
                    funded,
                    funded_system,
                    &funding_block,
                    None,
                    false,
                )
                .await
                .unwrap();
            assert_eq!(funded_state, replayed_funding);
            assert_eq!(
                system_vault_balance(&runtime_manager, &funded_state, &slot_address).await,
                purse_funding
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &funded_state, &entry_address).await,
                purse_funding
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &funded_state, &sponsor_address).await,
                initial_sponsor
                    - purse_funding * 2
                    - i64::try_from(funding_cost + funding_byte_cost + funding_fee).unwrap()
            );
            let funding_markers = runtime_manager
                .get_data(
                    funded_state.clone(),
                    &new_gstring_par("agent-slot-funded".to_string(), Vec::new(), false),
                )
                .await
                .unwrap();
            assert_eq!(funding_markers.len(), 2);

            let trigger_source = r#"new deployerId(`rho:system:deployerId`) in {
              @"agent-trigger"!(0, *deployerId)
            }"#
            .to_string();
            let unauthorized_trigger = construct_deploy::source_deploy(
                trigger_source.clone(),
                3,
                None,
                None,
                Some(installer_key),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let unauthorized_trigger =
                crypto::rust::signatures::signed::Cosigned::from_single_signer(
                    unauthorized_trigger,
                )
                .unwrap();
            let unauthorized_block = BlockData {
                time_stamp: 3,
                block_number: 4,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 4,
            };
            let unauthorized_admission = runtime_manager
                .certify_state_bound_admission(
                    &funded_state,
                    vec![unauthorized_trigger],
                    &unauthorized_block,
                    &HashMap::new(),
                )
                .await
                .unwrap();
            assert_eq!(unauthorized_admission.outcome().admitted.len(), 1);
            assert_eq!(
                unauthorized_admission
                    .outcome()
                    .stack_pops
                    .values()
                    .copied()
                    .sum::<u64>(),
                0
            );
            let unauthorized_close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    unauthorized_block.sender.clone(),
                    unauthorized_block.seq_num,
                ),
            );
            let (unauthorized_state, unauthorized_processed, unauthorized_system, _) =
                runtime_manager
                    .compute_state_with_bonds_cosigned_admitted(unauthorized_admission, vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            unauthorized_close,
                        ),
                    ])
                    .await
                    .unwrap();
            let replayed_unauthorized = runtime_manager
                .replay_compute_state(
                    &funded_state,
                    unauthorized_processed,
                    unauthorized_system,
                    &unauthorized_block,
                    None,
                    false,
                )
                .await
                .unwrap();
            assert_eq!(unauthorized_state, replayed_unauthorized);
            assert_eq!(
                system_vault_balance(&runtime_manager, &unauthorized_state, &slot_address).await,
                purse_funding
            );
            assert_eq!(
                system_vault_balance(&runtime_manager, &unauthorized_state, &entry_address).await,
                purse_funding
            );
            assert!(runtime_manager
                .get_data(
                    unauthorized_state.clone(),
                    &new_gstring_par("agent-ran".to_string(), Vec::new(), false),
                )
                .await
                .unwrap()
                .is_empty());

            let trigger = construct_deploy::source_deploy(
                trigger_source,
                4,
                None,
                None,
                Some(gateway_key),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let trigger =
                crypto::rust::signatures::signed::Cosigned::from_single_signer(trigger).unwrap();
            let trigger_block = BlockData {
                time_stamp: 4,
                block_number: 5,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 5,
            };
            let trigger_admission = runtime_manager
                .certify_state_bound_admission(
                    &unauthorized_state,
                    vec![trigger],
                    &trigger_block,
                    &HashMap::new(),
                )
                .await
                .unwrap();
            assert_eq!(trigger_admission.outcome().admitted.len(), 1);
            assert_eq!(trigger_admission.outcome().stack_pops.len(), 1);
            assert_eq!(
                trigger_admission
                    .outcome()
                    .stack_pops
                    .values()
                    .copied()
                    .sum::<u64>(),
                1
            );
            let trigger_close = CloseBlockDeploy::new(
                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                    trigger_block.sender.clone(),
                    trigger_block.seq_num,
                ),
            );
            let (triggered_state, triggered, triggered_system, _) = runtime_manager
                .compute_state_with_bonds_cosigned_admitted(trigger_admission, vec![
                    casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                        trigger_close,
                    ),
                ])
                .await
                .unwrap();
            let replayed_trigger = runtime_manager
                .replay_compute_state(
                    &unauthorized_state,
                    triggered,
                    triggered_system,
                    &trigger_block,
                    None,
                    false,
                )
                .await
                .unwrap();
            assert_eq!(triggered_state, replayed_trigger);
            let remaining_slot =
                system_vault_balance(&runtime_manager, &triggered_state, &slot_address).await;
            let remaining_entry =
                system_vault_balance(&runtime_manager, &triggered_state, &entry_address).await;
            assert!(remaining_slot < purse_funding);
            assert!(remaining_entry < purse_funding);
            assert!(remaining_slot > 0);
            assert!(remaining_entry > 0);
            let ran = runtime_manager
                .get_data(
                    triggered_state,
                    &new_gstring_par("agent-ran".to_string(), Vec::new(), false),
                )
                .await
                .unwrap();
            assert_eq!(ran.len(), 1);
            assert_eq!(RhoBoolean::unapply(&ran[0]), Some(true));
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn balance_deploy_should_compute_rev_balances() {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let user_pk = construct_deploy::DEFAULT_PUB.clone();
            let _ = compare_successful_system_deploys(
                &mut runtime_manager,
                &genesis_context,
                &genesis_block.body.state.post_state_hash,
                &mut CheckBalance {
                    pk: user_pk.clone(),
                    rand: Blake2b512Random::create_from_bytes(&[]),
                },
                &mut CheckBalance {
                    pk: user_pk.clone(),
                    rand: Blake2b512Random::create_from_bytes(&[]),
                },
                |result| *result == 9000000,
            )
            .await
            .unwrap();
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_state_should_capture_rholang_errors() {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let bad_rholang =
                r#" for(@x <- @"x" & @y <- @"y"){ @"xy"!(x + y) } | @"x"!(1) | @"y"!("hi") "#;
            let deploy = construct_deploy::source_deploy_now_full(
                bad_rholang.to_string(),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let result = compute_state(
                &mut runtime_manager,
                &genesis_context,
                deploy,
                &genesis_block.body.state.post_state_hash,
            )
            .await;

            assert!(result.1.is_failed);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_state_then_compute_bonds_should_be_replayable_after_all() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let gps = genesis_block.body.state.post_state_hash;

            let s0 = "@1!(1)";
            let s1 = "@2!(2)";
            let s2 = "for(@a <- @1){ @123!(5 * a) }";

            // Deploys must carry DISTINCT timestamps. Signing is deterministic
            // (RFC 6979), so two deploys with identical DeployData (same source,
            // deployer, phlo, and millisecond timestamp) produce the SAME
            // signature. Pre-charge/refund random seeds are derived from that
            // signature (system_deploy_util), so identical signatures alias the
            // unforgeable purse channels across blocks, over-filling the
            // single-value NonNegativeNumber cells. The Scala original avoided
            // this via a monotonic LogicalTime; here we allocate a distinct
            // timestamp per deploy across both blocks. s0 and s3 share the source
            // "@1!(1)" specifically, so distinct timestamps are load-bearing.
            let base_ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            let deploys0 = vec![s0, s1, s2]
                .into_iter()
                .enumerate()
                .map(|(i, s)| {
                    construct_deploy::source_deploy(
                        s.to_string(),
                        base_ts + i as i64,
                        Some(1000000),
                        None,
                        None,
                        None,
                        None,
                    )
                    .unwrap()
                })
                .collect::<Vec<_>>();

            let s3 = "@1!(1)";
            let s4 = "for(@a <- @2){ @456!(5 * a) }";

            let deploys1 = vec![s3, s4]
                .into_iter()
                .enumerate()
                .map(|(i, s)| {
                    construct_deploy::source_deploy(
                        s.to_string(),
                        base_ts + 3 + i as i64,
                        Some(1000000),
                        None,
                        None,
                        None,
                        None,
                    )
                    .unwrap()
                })
                .collect::<Vec<_>>();

            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            let (play_state_hash_0, processed_deploys_0, processed_sys_deploys_0) = runtime_manager
                .compute_state(
                    &gps,
                    deploys0,
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    genesis_context.validator_pks()[0].clone(),
                                    0,
                                ),
                            ),
                        ),
                    ],
                    BlockData {
                        time_stamp: time,
                        block_number: 0,
                        sender: genesis_context.validator_pks()[0].clone(),
                        seq_num: 0,
                    },
                    None,
                )
                .await
                .unwrap();

            let bonds0 = runtime_manager
                .compute_bonds(&play_state_hash_0)
                .await
                .unwrap();

            let replay_state_hash_0 = runtime_manager
                .replay_compute_state(
                    &gps,
                    processed_deploys_0,
                    processed_sys_deploys_0,
                    &BlockData {
                        time_stamp: time,
                        block_number: 0,
                        sender: genesis_context.validator_pks()[0].clone(),
                        seq_num: 0,
                    },
                    None,
                    false,
                )
                .await
                .unwrap();

            assert!(play_state_hash_0 == replay_state_hash_0);

            let bonds1 = runtime_manager
                .compute_bonds(&play_state_hash_0)
                .await
                .unwrap();

            assert!(bonds0 == bonds1);

            let (play_state_hash_1, processed_deploys_1, processed_sys_deploys_1) = runtime_manager
                .compute_state(
                    &play_state_hash_0,
                    deploys1,
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    genesis_context.validator_pks()[0].clone(),
                                    0,
                                ),
                            ),
                        ),
                    ],
                    BlockData {
                        time_stamp: time,
                        block_number: 0,
                        sender: genesis_context.validator_pks()[0].clone(),
                        seq_num: 0,
                    },
                    None,
                )
                .await
                .unwrap();

            let bonds2 = runtime_manager
                .compute_bonds(&play_state_hash_1)
                .await
                .unwrap();

            let replay_state_hash_1 = runtime_manager
                .replay_compute_state(
                    &play_state_hash_0,
                    processed_deploys_1,
                    processed_sys_deploys_1,
                    &BlockData {
                        time_stamp: time,
                        block_number: 0,
                        sender: genesis_context.validator_pks()[0].clone(),
                        seq_num: 0,
                    },
                    None,
                    false,
                )
                .await
                .unwrap();

            assert!(play_state_hash_1 == replay_state_hash_1);

            let bonds3 = runtime_manager
                .compute_bonds(&play_state_hash_1)
                .await
                .unwrap();

            assert!(bonds2 == bonds3);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn state_bound_parser_rejection_has_no_processed_cost_evidence() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let payer_address =
                VaultAddress::from_public_key(&genesis_context.genesis_vaults[0].1).unwrap();
            let initial_payer =
                system_vault_balance(&runtime_manager, &start_state, &payer_address).await;
            let deploy = construct_deploy::source_deploy_now_full(
                "for(".to_string(),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let deploy =
                crypto::rust::signatures::signed::Cosigned::from_single_signer(deploy).unwrap();
            let deploy_id = deploy.primary().sig.clone();
            let block_data = BlockData {
                time_stamp: deploy.data().time_stamp,
                block_number: 2,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 2,
            };
            let (evidence, rejected) = runtime_manager
                .state_bound_cost_evidence(&start_state, vec![deploy], block_data, HashMap::new())
                .await
                .unwrap();
            assert!(evidence.is_empty());
            assert_eq!(rejected, vec![crate::legacy_deploy_id(&deploy_id)]);
            assert_eq!(
                system_vault_balance(&runtime_manager, &start_state, &payer_address).await,
                initial_payer
            );
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_state_should_charge_comm_execution_and_quantitative_bytes() {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let correct_rholang =
                r#" for(@x <- @"x" & @y <- @"y"){ @"xy"!(x + y) } | @"x"!(1) | @"y"!(2) "#;
            let deploy = construct_deploy::source_deploy_now_full(
                correct_rholang.to_string(),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let result = compute_state(
                &mut runtime_manager,
                &genesis_context,
                deploy,
                &genesis_block.body.state.post_state_hash,
            )
            .await;
            let witness = result.1.authority_cost_witness.as_ref().unwrap();
            assert_eq!(witness.events.len(), 1);
            assert!(witness.byte_cost > 0);
            assert_eq!(
                result.1.cost.cost,
                u64::try_from(witness.events.len()).unwrap() + witness.byte_cost
            );
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn capture_result_should_return_the_value_at_the_specified_channel_after_a_rholang_computation(
) {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let deployo0 = construct_deploy::source_deploy_now_full(
                r#"
                        new rl(`rho:registry:lookup`), NonNegativeNumberCh in {
                        rl!(`rho:lang:nonNegativeNumber`, *NonNegativeNumberCh) |
                        for(@(_, NonNegativeNumber) <- NonNegativeNumberCh) {
                          @NonNegativeNumber!(37, "nn")
                        }
                      }
                "#
                .to_string(),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let result0 = compute_state(
                &mut runtime_manager,
                &genesis_context,
                deployo0,
                &genesis_block.body.state.post_state_hash,
            )
            .await;

            let hash = result0.0;
            let deployo1 = construct_deploy::source_deploy_now_full(
                r#"
                new return in { for(nn <- @"nn"){ nn!("value", *return) } }
                "#
                .to_string(),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let result1 = runtime_manager
                .capture_results(&hash, &deployo1)
                .await
                .unwrap();

            assert!(result1.len() == 1);
            assert!(result1[0] == ParBuilderUtil::mk_term("37").unwrap());
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn capture_result_should_handle_multiple_results_and_no_results_appropriately() {
    with_runtime_manager(|runtime_manager, _, _| async move {
        let n = 8;
        let returns = (1..=n)
            .map(|i| format!("return!({})", i))
            .collect::<Vec<_>>();
        let term = format!("new return in {{ {} }}", returns.join("|"));
        let term_no_res = format!("new x, return in {{ {} }}", returns.join("|"));
        let deploy =
            construct_deploy::source_deploy(term, 0, None, None, None, None, None).unwrap();
        let deploy_no_res =
            construct_deploy::source_deploy(term_no_res, 0, None, None, None, None, None).unwrap();

        let many_results = runtime_manager
            .capture_results(&RuntimeManager::empty_state_hash_fixed(), &deploy)
            .await
            .unwrap();

        let no_results = runtime_manager
            .capture_results(&RuntimeManager::empty_state_hash_fixed(), &deploy_no_res)
            .await
            .unwrap();

        assert!(no_results.is_empty());
        assert!(many_results.len() == n);
        assert!((1..=n)
            .all(|i| many_results.contains(&ParBuilderUtil::mk_term(&i.to_string()).unwrap())));
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn capture_result_should_throw_error_if_execution_fails() {
    with_runtime_manager(|runtime_manager, _, _| async move {
        let deploy = construct_deploy::source_deploy(
            "new return in { return.undefined() }".to_string(),
            0,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let result = runtime_manager
            .capture_results(&RuntimeManager::empty_state_hash_fixed(), &deploy)
            .await;

        assert!(result.is_err());
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn empty_state_hash_should_not_remember_previous_hot_store_state() {
    with_runtime_manager(
        |mut runtime_manager, genesis_context, genesis_block| async move {
            let deploy1 = construct_deploy::basic_deploy_data(0, None, None).unwrap();
            let deploy2 = construct_deploy::basic_deploy_data(0, None, None).unwrap();

            let hash1 = RuntimeManager::empty_state_hash_fixed();
            let _ = compute_state(
                &mut runtime_manager,
                &genesis_context,
                deploy1,
                &genesis_block.body.state.post_state_hash,
            )
            .await;

            let hash2 = RuntimeManager::empty_state_hash_fixed();
            let _ = compute_state(
                &mut runtime_manager,
                &genesis_context,
                deploy2,
                &genesis_block.body.state.post_state_hash,
            )
            .await;

            assert!(hash1 == hash2);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deployer_id_system_vault_query_should_replay_from_state_bound_checkpoint() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let deploy = construct_deploy::source_deploy(
                r#"
                  new deployerId(`rho:system:deployerId`),
                  rl(`rho:registry:lookup`),
                  revAddressOps(`rho:vault:address`),
                  revAddressCh,
                  revVaultCh in {
                  rl!(`rho:vault:system`, *revVaultCh) |
                  revAddressOps!("fromDeployerId", *deployerId, *revAddressCh) |
                  for(@userRevAddress <- revAddressCh & @(_, revVault) <- revVaultCh){
                    new userVaultCh in {
                    @revVault!("findOrCreate", userRevAddress, *userVaultCh) |
                    for(@(true, userVault) <- userVaultCh){
                    @userVault!("balance", "IGNORE")
                    }
                  }
                }
                }
                "#
                .to_string(),
                1,
                None,
                None,
                Some(genesis_context.genesis_vaults[0].0.clone()),
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();

            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis();

            let genesis_post_state = genesis_block.body.state.post_state_hash;
            let block_data = BlockData {
                time_stamp: time as i64,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            };

            let invalid_blocks = HashMap::new();
            let (play_post_state, processed_deploys, processed_system_deploys) = runtime_manager
                .compute_state(
                    &genesis_post_state,
                    vec![deploy],
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    block_data.sender.clone(),
                                    block_data.seq_num,
                                ),
                            ),
                        ),
                    ],
                    block_data.clone(),
                    Some(invalid_blocks.clone()),
                )
                .await
                .unwrap();

            let replay_compute_state_result = runtime_manager
                .replay_compute_state(
                    &genesis_post_state,
                    processed_deploys,
                    processed_system_deploys,
                    &block_data,
                    Some(invalid_blocks),
                    false,
                )
                .await
                .unwrap();

            assert!(play_post_state == replay_compute_state_result);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_state_should_charge_deploys_separately() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            fn deploy_cost(p: &[ProcessedDeploy]) -> u64 { p.iter().map(|d| d.cost.cost).sum() }

            let deploy0 = construct_deploy::source_deploy(
                r#"new w, z in { w!("World") | for(@x <- w) { z!("Got x") } } "#.to_string(),
                123,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let deploy1 = construct_deploy::source_deploy(
                r#"for(@x <- @"x" & @y <- @"y"){ @"xy"!(x + y) } | @"x"!(1) | @"y"!(10) "#
                    .to_string(),
                123,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis();

            let genesis_post_state = genesis_block.body.state.post_state_hash;
            let block_data = BlockData {
                time_stamp: time as i64,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            };

            let invalid_blocks = HashMap::new();
            let (_, first_deploy, _) = runtime_manager
                .compute_state(
                    &genesis_post_state,
                    vec![construct_deploy::source_deploy(
                        r#"new w, z in { w!("World") | for(@x <- w) { z!("Got x") } } "#
                            .to_string(),
                        123,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                    .unwrap()],
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    block_data.sender.clone(),
                                    block_data.seq_num,
                                ),
                            ),
                        ),
                    ],
                    block_data.clone(),
                    Some(invalid_blocks.clone()),
                )
                .await
                .unwrap();

            let (_, second_deploy, _) = runtime_manager
                .compute_state(
                    &genesis_post_state,
                    vec![construct_deploy::source_deploy(
                        r#"for(@x <- @"x" & @y <- @"y"){ @"xy"!(x + y) } | @"x"!(1) | @"y"!(10) "#
                            .to_string(),
                        123,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                    .unwrap()],
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    block_data.sender.clone(),
                                    block_data.seq_num,
                                ),
                            ),
                        ),
                    ],
                    block_data.clone(),
                    Some(invalid_blocks.clone()),
                )
                .await
                .unwrap();

            let (_, compound_deploy, _) = runtime_manager
                .compute_state(
                    &genesis_post_state,
                    vec![deploy0, deploy1],
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    block_data.sender.clone(),
                                    block_data.seq_num,
                                ),
                            ),
                        ),
                    ],
                    block_data.clone(),
                    Some(invalid_blocks.clone()),
                )
                .await
                .unwrap();

            assert!(first_deploy.len() == 1);
            assert!(second_deploy.len() == 1);
            assert!(compound_deploy.len() == 2);

            let first_deploy_cost = deploy_cost(&first_deploy);
            let second_deploy_cost = deploy_cost(&second_deploy);
            let compound_deploy_cost = deploy_cost(&compound_deploy);

            for processed in first_deploy
                .iter()
                .chain(&second_deploy)
                .chain(&compound_deploy)
            {
                let witness = processed.authority_cost_witness.as_ref().unwrap();
                assert!(witness.byte_cost > 0);
                assert_eq!(
                    processed.cost.cost,
                    u64::try_from(witness.events.len()).unwrap() + witness.byte_cost
                );
            }
            assert!(first_deploy_cost < compound_deploy_cost);
            assert!(second_deploy_cost < compound_deploy_cost);

            let matched_first = compound_deploy
                .iter()
                .find(|d| d.deploy == first_deploy[0].deploy)
                .cloned()
                .expect("Expected at least one matching deploy");
            assert_eq!(first_deploy_cost, deploy_cost(&[matched_first]));

            let matched_second = compound_deploy
                .iter()
                .find(|d| d.deploy == second_deploy[0].deploy)
                .cloned()
                .expect("Expected at least one matching deploy");
            assert_eq!(second_deploy_cost, deploy_cost(&[matched_second]));

            assert_eq!(first_deploy_cost + second_deploy_cost, compound_deploy_cost);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn system_settlement_use_case_does_not_change_user_runtime_cost() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            // Keep the user COMM on a deploy-local channel so this test isolates
            // fee-settlement system deploys from public-channel application effects.
            let source = "new x in { x!(0) | for(@0 <- x){ Nil } }";
            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            let gen_post_state = genesis_block.body.state.post_state_hash;
            let block_data = BlockData {
                time_stamp: time,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            };

            let deploy_without_settlement = construct_deploy::source_deploy(
                source.to_string(),
                123,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let deploy_with_settlement = construct_deploy::source_deploy(
                source.to_string(),
                123,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let (_, user_only, _) = runtime_manager
                .compute_state(
                    &gen_post_state,
                    vec![deploy_without_settlement],
                    Vec::new(),
                    block_data.clone(),
                    Some(HashMap::new()),
                )
                .await
                .unwrap();

            let (_, with_settlement, _) = runtime_manager
                .compute_state(
                    &gen_post_state,
                    vec![deploy_with_settlement],
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    block_data.sender.clone(),
                                    block_data.seq_num,
                                ),
                            ),
                        ),
                    ],
                    block_data,
                    Some(HashMap::new()),
                )
                .await
                .unwrap();

            assert_eq!(user_only.len(), 1);
            assert_eq!(with_settlement.len(), 1);
            assert_eq!(user_only[0].cost, with_settlement[0].cost);
            assert_eq!(user_only[0].is_failed, with_settlement[0].is_failed);
            // NOTE: We intentionally do not assert equality of
            // `deploy_log.len()` here. The PoS pre-charge + refund flow
            // engages persistent consumes (`<<-`/`<=`-style) whose
            // re-registration spawns parallel futures in
            // `reduce::continue_consume_process`. Under tokio's
            // multi-thread scheduling, those persistent consumes can
            // legitimately match an extra or one-fewer time per run,
            // shifting `deploy_log.len()` by ±1 across otherwise-
            // identical play passes. The cost and is_failed assertions
            // above already cover the "system settlement does not change
            // user runtime cost" claim this test exists to enforce.
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn compute_state_should_just_work() {
    with_runtime_manager(|mut runtime_manager, genesis_context, genesis_block| async move {
      let gen_post_state = genesis_block.body.state.post_state_hash;
      let source =  r#"
      new d1,d2,d3,d4,d5,d6,d7,d8,d9 in {
        contract d1(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d1!(depth - 1) | d1!(depth - 1) | d1!(depth - 1) | d1!(depth - 1) | d1!(depth - 1) | d1!(depth - 1) | d1!(depth - 1) | d1!(depth - 1) | d1!(depth - 1) | d1!(depth - 1)
          }
        } |
        contract d2(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d2!(depth - 1) | d2!(depth - 1) | d2!(depth - 1) | d2!(depth - 1) | d2!(depth - 1) | d2!(depth - 1) | d2!(depth - 1) | d2!(depth - 1) | d2!(depth - 1) | d2!(depth - 1)
          }
        } |
        contract d3(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d3!(depth - 1) | d3!(depth - 1) | d3!(depth - 1) | d3!(depth - 1) | d3!(depth - 1) | d3!(depth - 1) | d3!(depth - 1) | d3!(depth - 1) | d3!(depth - 1) | d3!(depth - 1)
          }
        } |
        contract d4(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d4!(depth - 1) | d4!(depth - 1) | d4!(depth - 1) | d4!(depth - 1) | d4!(depth - 1) | d4!(depth - 1) | d4!(depth - 1) | d4!(depth - 1) | d4!(depth - 1) | d4!(depth - 1)
          }
        } |
        contract d5(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d5!(depth - 1) | d5!(depth - 1) | d5!(depth - 1) | d5!(depth - 1) | d5!(depth - 1) | d5!(depth - 1) | d5!(depth - 1) | d5!(depth - 1) | d5!(depth - 1) | d5!(depth - 1)
          }
        } |
        contract d6(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d6!(depth - 1) | d6!(depth - 1) | d6!(depth - 1) | d6!(depth - 1) | d6!(depth - 1) | d6!(depth - 1) | d6!(depth - 1) | d6!(depth - 1) | d6!(depth - 1) | d6!(depth - 1)
          }
        } |
        contract d7(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d7!(depth - 1) | d7!(depth - 1) | d7!(depth - 1) | d7!(depth - 1) | d7!(depth - 1) | d7!(depth - 1) | d7!(depth - 1) | d7!(depth - 1) | d7!(depth - 1) | d7!(depth - 1)
          }
        } |
        contract d8(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d8!(depth - 1) | d8!(depth - 1) | d8!(depth - 1) | d8!(depth - 1) | d8!(depth - 1) | d8!(depth - 1) | d8!(depth - 1) | d8!(depth - 1) | d8!(depth - 1) | d8!(depth - 1)
          }
        } |
        contract d9(@depth) = {
          if (depth <= 0) {
            Nil
          } else {
            d9!(depth - 1) | d9!(depth - 1) | d9!(depth - 1) | d9!(depth - 1) | d9!(depth - 1) | d9!(depth - 1) | d9!(depth - 1) | d9!(depth - 1) | d9!(depth - 1) | d9!(depth - 1)
          }
        } |
        d1!(2) |
        d2!(2) |
        d3!(2) |
        d4!(2) |
        d5!(2) |
        d6!(2) |
        d7!(2) |
        d8!(2) |
        d9!(2)
      }
      "#.to_string();

      // Protocol 4 derives the hard ceiling from authenticated RevVault
      // custody and settles exact physical, byte, and fee debit. The legacy
      // phlo-limit argument is retained only by this test helper's interface;
      // it is not an escrow amount or a source of execution authority.
      let deploy = construct_deploy::source_deploy_now_full(source, Some(9_000_000), None, None, None, None).unwrap();
      let (play_state_hash1, processed_deploy, processed_system_deploys) = compute_state(&mut runtime_manager, &genesis_context, deploy, &gen_post_state).await;
      let replay_compute_state_result = replay_compute_state(&mut runtime_manager, &genesis_context, processed_deploy, processed_system_deploys, &gen_post_state).await.unwrap();
      assert!(play_state_hash1 == replay_compute_state_result);
      assert!(play_state_hash1 != gen_post_state);
    })
        .await
        .unwrap()
}

async fn invalid_replay(source: String) -> Result<StateHash, CasperError> {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let deploy = construct_deploy::source_deploy_now_full(
                source,
                Some(10000),
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            let gen_post_state = genesis_block.body.state.post_state_hash;
            let block_data = BlockData {
                time_stamp: time,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            };

            let invalid_blocks = HashMap::new();

            let (_, processed_deploys, processed_system_deploys) = runtime_manager
                .compute_state(
                    &gen_post_state,
                    vec![deploy],
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    block_data.sender.clone(),
                                    block_data.seq_num,
                                ),
                            ),
                        ),
                    ],
                    block_data.clone(),
                    Some(invalid_blocks.clone()),
                )
                .await
                .unwrap();
            let processed_deploy = processed_deploys.into_iter().next().unwrap();
            let processed_deploy_cost = processed_deploy.cost.cost;

            let invalid_processed_deploy = ProcessedDeploy {
                cost: PCost {
                    cost: processed_deploy_cost - 1,
                },
                ..processed_deploy
            };

            let result = runtime_manager
                .replay_compute_state(
                    &gen_post_state,
                    vec![invalid_processed_deploy],
                    processed_system_deploys,
                    &block_data,
                    Some(invalid_blocks),
                    false,
                )
                .await;

            result
        },
    )
    .await?
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn matched_and_unmatched_deploys_keep_isolated_cost_traces() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let matched_source = "@0!(0) | for(@0 <- @0){ Nil }";
            let unmatched_source = "@1!(1)";
            let success = construct_deploy::source_deploy_now_full(
                matched_source.to_string(),
                Some(10000),
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let unmatched = construct_deploy::source_deploy_now_full(
                unmatched_source.to_string(),
                Some(1),
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;
            let gen_post_state = genesis_block.body.state.post_state_hash;
            let block_data = BlockData {
                time_stamp: time,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            };

            let (play_state, processed_deploys, processed_system_deploys) = runtime_manager
                .compute_state(
                    &gen_post_state,
                    vec![success, unmatched],
                    Vec::new(),
                    block_data.clone(),
                    None,
                )
                .await
                .unwrap();

            assert_eq!(processed_deploys.len(), 2);
            assert_eq!(
                processed_deploys
                    .iter()
                    .filter(|deploy| deploy.is_failed)
                    .count(),
                0,
                "matched and unmatched deployments both complete within their certified capacities"
            );
            let matched = processed_deploys
                .iter()
                .find(|deploy| deploy.deploy.data.term == matched_source)
                .expect("matched deployment must be present in the execution evidence");
            let unmatched = processed_deploys
                .iter()
                .find(|deploy| deploy.deploy.data.term == unmatched_source)
                .expect("unmatched deployment must be present in the execution evidence");
            let matched_witness = matched.authority_cost_witness.as_ref().unwrap();
            let unmatched_witness = unmatched.authority_cost_witness.as_ref().unwrap();
            let matched_physical_authority = matched_witness
                .settlement
                .iter()
                .map(|resource| resource.amount)
                .sum::<u64>();
            let unmatched_physical_authority = unmatched_witness
                .settlement
                .iter()
                .map(|resource| resource.amount)
                .sum::<u64>();
            assert_eq!(
                matched_witness.events.len(),
                1,
                "the complete send/receive match records one atomic COMM authority event"
            );
            assert_eq!(
                unmatched_witness.events.len(),
                0,
                "the unmatched send records no COMM authority event"
            );
            assert!(matched_physical_authority > 0);
            assert_eq!(unmatched_physical_authority, 0);
            assert!(matched_witness.byte_cost > unmatched_witness.byte_cost);
            assert!(unmatched_witness.byte_cost > 0);
            assert_eq!(
                matched.cost.cost,
                u64::try_from(matched_witness.events.len()).unwrap() + matched_witness.byte_cost
            );
            assert_eq!(
                unmatched.cost.cost,
                u64::try_from(unmatched_witness.events.len()).unwrap()
                    + unmatched_witness.byte_cost
            );

            let replay_state = runtime_manager
                .replay_compute_state(
                    &gen_post_state,
                    processed_deploys,
                    processed_system_deploys,
                    &block_data,
                    None,
                    false,
                )
                .await
                .unwrap();

            assert_eq!(play_state, replay_state);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejected_replay_post_state_witness_restores_block_pre_state() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash;
            let deploy = construct_deploy::source_deploy_now_full(
                "@0!(0) | for(@0 <- @0){ Nil }".to_string(),
                Some(10000),
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let block_data = BlockData {
                time_stamp: deploy.data.time_stamp,
                block_number: 1,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 1,
            };
            let (_, mut processed_deploys, processed_system_deploys) = runtime_manager
                .compute_state(
                    &start_state,
                    vec![deploy],
                    Vec::new(),
                    block_data.clone(),
                    None,
                )
                .await
                .unwrap();
            processed_deploys[0].post_state_hash = vec![0xa5; 32].into();

            let replay_runtime = runtime_manager.spawn_replay_runtime().await;
            let mut replay_ops = ReplayRuntimeOps::new_from_runtime(replay_runtime);
            let result = replay_ops
                .replay_compute_state(
                    &start_state,
                    processed_deploys,
                    processed_system_deploys,
                    &block_data,
                    None,
                    false,
                    Some(&runtime_manager),
                )
                .await;

            assert!(matches!(
                result,
                Err(CasperError::ReplayFailure(
                    ReplayFailure::EffectStateMismatch { .. }
                ))
            ));
            let checkpoint = replay_ops.runtime_ops.runtime.create_checkpoint().await;
            assert_eq!(checkpoint.root.to_bytes_prost(), start_state);
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejected_block_final_state_does_not_publish_mergeable_evidence() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let deploy = construct_deploy::source_deploy_now_full(
                "@0!(0) | for(@0 <- @0){ Nil }".to_string(),
                Some(10000),
                None,
                None,
                None,
                Some(genesis_block.shard_id.clone()),
            )
            .unwrap();
            let block_data = BlockData {
                time_stamp: deploy.data.time_stamp,
                block_number: 1,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 1,
            };
            let (valid_post_state, processed_deploys, processed_system_deploys) = runtime_manager
                .compute_state(
                    &start_state,
                    vec![deploy],
                    Vec::new(),
                    block_data.clone(),
                    None,
                )
                .await
                .unwrap();
            let valid_block = BlockMessage {
                block_hash: vec![0x71; 32].into(),
                header: Header {
                    parents_hash_list: vec![genesis_block.block_hash],
                    timestamp: block_data.time_stamp,
                    version: genesis_block.header.version,
                    extra_bytes: Vec::<u8>::new().into(),
                    sender_bond_generation: Some(
                        models::rust::bond_generation::BondGeneration::GENESIS,
                    ),
                    objective_equivocation_evidence_delta: Vec::new(),
                    finalized_floor: None,
                },
                body: Body {
                    state: F1r3flyState {
                        pre_state_hash: start_state.clone(),
                        post_state_hash: valid_post_state.clone(),
                        bonds: Vec::new(),
                        bond_generations: Vec::new(),
                        active_validators: Vec::new(),
                        block_number: block_data.block_number,
                    },
                    deploys: processed_deploys,
                    rejected_deploys: Vec::new(),
                    rejected_state_effects: Vec::new(),
                    system_deploys: processed_system_deploys,
                    extra_bytes: Vec::<u8>::new().into(),
                    applied_from_scope: Vec::new(),
                    merge_base: Vec::<u8>::new().into(),
                },
                justifications: Vec::new(),
                sender: block_data.sender.bytes.clone(),
                seq_num: block_data.seq_num,
                sig: Vec::<u8>::new().into(),
                sig_algorithm: String::new(),
                shard_id: genesis_block.shard_id,
                extra_bytes: Vec::<u8>::new().into(),
                finalized_floor_certificate: None,
            };

            assert!(runtime_manager
                .delete_mergeable_channels(&valid_block)
                .unwrap());
            assert!(!runtime_manager.has_mergeable_entry(&valid_block).unwrap());

            let mut forged_block = valid_block.clone();
            forged_block.block_hash = vec![0x72; 32].into();
            forged_block.body.state.post_state_hash = vec![0xb6; 32].into();
            let result = runtime_manager
                .replay_block_from_consensus_data(&start_state, &forged_block, None)
                .await;
            assert!(matches!(
                result,
                Err(CasperError::ReplayFailure(
                    ReplayFailure::EffectStateMismatch { ref boundary, .. }
                )) if boundary == "final-post-state"
            ));
            assert!(!runtime_manager.has_mergeable_entry(&valid_block).unwrap());
            assert!(!runtime_manager.has_mergeable_entry(&forged_block).unwrap());

            let replayed = runtime_manager
                .replay_block_from_consensus_data(&start_state, &valid_block, None)
                .await
                .unwrap();
            assert_eq!(replayed, valid_post_state);
            assert!(runtime_manager.has_mergeable_entry(&valid_block).unwrap());
        },
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replaycomputestate_should_catch_discrepancies_in_initial_and_replay_cost_when_no_errors_are_thrown(
) {
    let result = invalid_replay("@0!(0) | for(@0 <- @0){ Nil }".to_string()).await;
    match result {
        Err(CasperError::ReplayFailure(ReplayFailure::ReplayCostMismatch {
            initial_cost,
            replay_cost,
        })) => {
            // The test corrupts the recorded deploy cost by one token. Exact
            // totals belong to the reducer's source-token schedule, while the
            // replay contract here is that the mismatch is detected exactly.
            assert_eq!(initial_cost.checked_add(1), Some(replay_cost));
        }
        _ => panic!("Expected ReplayCostMismatch error"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replaycomputestate_should_not_catch_discrepancies_in_initial_and_replay_cost_when_user_errors_are_thrown(
) {
    let result = invalid_replay("@0!(0) | for(@x <- @0){ x.undefined() }".to_string()).await;
    match result {
        Err(CasperError::ReplayFailure(ReplayFailure::ReplayCostMismatch {
            initial_cost,
            replay_cost,
        })) => {
            // User execution errors are rollback-safe, but replay must still
            // reject a processed deploy whose charged token count was forged.
            assert_eq!(initial_cost.checked_add(1), Some(replay_cost));
        }
        _ => panic!("Expected ReplayCostMismatch error"),
    }
}

// This is additional test for sorting with joins and channels inside joins.
// - after reverted PR https://github.com/rchain/rchain/pull/2436
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn joins_should_be_replayed_correctly() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let term = r#"
            new a, b, c, d in {
              for (_ <- a & _ <- b) { Nil } |
              for (_ <- a & _ <- c) { Nil } |
              for (_ <- a & _ <- d) { Nil }
            }
            "#;

            let gen_post_state = genesis_block.body.state.post_state_hash;
            let deploy = construct_deploy::source_deploy_now_full(
                term.to_string(),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            let block_data = BlockData {
                time_stamp: time,
                block_number: 1,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 1,
            };

            let invalid_blocks = HashMap::new();
            let (state_hash, processed_deploys, processed_sys_deploys) = runtime_manager
                .compute_state(
                    &gen_post_state,
                    vec![deploy],
                    Vec::new(), // No system deploys
                    block_data.clone(),
                    Some(invalid_blocks.clone()),
                )
                .await
                .unwrap();

            let replay_state_hash = runtime_manager
                .replay_compute_state(
                    &gen_post_state,
                    processed_deploys,
                    processed_sys_deploys,
                    &block_data,
                    Some(invalid_blocks),
                    false,
                )
                .await
                .unwrap();

            assert_eq!(hex::encode(&state_hash), hex::encode(&replay_state_hash));
        },
    )
    .await
    .unwrap();
}

/// Reproduce ReplayCostMismatch with duplicate channel sends in bridge contracts.
///
/// Uses two independent RuntimeManagers sharing the same genesis RSpace scope.
/// The first plays the deploy (hot store populated from execution).
/// The second replays with a fresh hot store (loads from history).
/// This simulates the block creator vs replayer divergence.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn replay_on_independent_runtime_should_match_play_cost_for_bridge_contracts() {
    use crate::util::rholang::resources::{
        mk_runtime_manager_with_history_at, mk_test_rnode_store_manager_from_genesis,
    };

    crate::init_logger();
    let genesis_context = crate::util::rholang::resources::genesis_context()
        .await
        .unwrap();
    let genesis_block = genesis_context.genesis_block.clone();
    let genesis_post_state = genesis_block.body.state.post_state_hash.clone();

    let fixtures = ["bridge.rho", "bridge-v2.rho"];

    let mut failures = Vec::new();
    for fixture in fixtures {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/resources")
                .join(fixture),
        )
        .unwrap_or_else(|error| panic!("failed to read {fixture}: {error}"));

        for attempt in 0..10 {
            let mut kvm_play = mk_test_rnode_store_manager_from_genesis(&genesis_context);
            let (rm_play, _) = mk_runtime_manager_with_history_at(&mut *kvm_play).await;

            let deploy = construct_deploy::source_deploy_now_full(
                source.clone(),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let play_block_data = BlockData {
                time_stamp: deploy.data.time_stamp,
                block_number: 1,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 1,
            };

            let (play_post, play_deploys, play_sys_deploys) = rm_play
                .compute_state(
                    &genesis_post_state,
                    vec![deploy],
                    Vec::new(),
                    play_block_data.clone(),
                    None,
                )
                .await
                .unwrap();

            let play_cost = play_deploys[0].cost.cost;

            let mut kvm_replay = mk_test_rnode_store_manager_from_genesis(&genesis_context);
            let (rm_replay, _) = mk_runtime_manager_with_history_at(&mut *kvm_replay).await;

            let replay_result = rm_replay
                .replay_compute_state(
                    &genesis_post_state,
                    play_deploys,
                    play_sys_deploys,
                    &play_block_data,
                    None,
                    false,
                )
                .await;

            match replay_result {
                Ok(replay_post) if replay_post == play_post => {}
                Ok(replay_post) => failures.push(format!(
                    "{fixture} attempt {attempt}: play_cost={play_cost}, play_post={}, replay_post={}",
                    hex::encode(play_post),
                    hex::encode(replay_post)
                )),
                Err(CasperError::ReplayFailure(ref failure)) => {
                    failures.push(format!(
                        "{fixture} attempt {attempt}: play_cost={play_cost}, {failure:?}"
                    ));
                }
                Err(error) => {
                    failures.push(format!("{fixture} attempt {attempt}: {error:?}"));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "play/replay divergence in {}/20 attempts:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn independent_validator_materializes_intermediate_roots_before_reading_purses() {
    use crate::util::rholang::resources::{
        generate_scope_id, mk_runtime_manager_with_history_at,
        mk_test_rnode_store_manager_from_genesis, mk_test_rnode_store_manager_shared,
    };

    crate::init_logger();
    let genesis_context = crate::util::rholang::resources::genesis_context()
        .await
        .unwrap();
    let genesis_post_state = genesis_context
        .genesis_block
        .body
        .state
        .post_state_hash
        .clone();
    let first = construct_deploy::source_deploy_now_full(
        "@0!(0) | for(@0 <- @0){ Nil }".to_string(),
        Some(10000),
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let second = construct_deploy::source_deploy_now_full(
        "@1!(1)".to_string(),
        Some(10000),
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let block_data = BlockData {
        time_stamp: first.data.time_stamp.max(second.data.time_stamp),
        block_number: 1,
        sender: genesis_context.validator_pks()[0].clone(),
        seq_num: 1,
    };

    let mut producer_store = mk_test_rnode_store_manager_from_genesis(&genesis_context);
    let (producer, _) = mk_runtime_manager_with_history_at(&mut *producer_store).await;
    let (expected_post, processed, system_deploys) = producer
        .compute_state(
            &genesis_post_state,
            vec![first, second],
            Vec::new(),
            block_data.clone(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(processed.len(), 2);
    assert_eq!(processed[0].pre_state_hash, genesis_post_state);
    assert_eq!(processed[1].pre_state_hash, processed[0].post_state_hash);

    let intermediate = Blake2b256Hash::from_bytes_prost(&processed[1].pre_state_hash);
    let mut validator_store = mk_test_rnode_store_manager_shared(generate_scope_id());
    let (validator, _) = mk_runtime_manager_with_history_at(&mut *validator_store).await;
    let genesis_pre_state = genesis_context
        .genesis_block
        .body
        .state
        .pre_state_hash
        .clone();
    let replayed_genesis = validator
        .replay_block_from_consensus_data(&genesis_pre_state, &genesis_context.genesis_block, None)
        .await
        .unwrap();
    assert_eq!(replayed_genesis, genesis_post_state);
    let genesis_root = Blake2b256Hash::from_bytes_prost(&genesis_post_state);
    assert!(validator.has_root(&genesis_root).unwrap());
    assert!(!validator.has_root(&intermediate).unwrap());

    let actual_post = validator
        .replay_compute_state(
            &genesis_post_state,
            processed,
            system_deploys,
            &block_data,
            None,
            false,
        )
        .await
        .unwrap();

    assert_eq!(actual_post, expected_post);
    assert!(validator.has_root(&intermediate).unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cross_deploy_bridge_full_admin_flow() {
    use crate::util::rholang::resources::{
        mk_runtime_manager_with_history_at, mk_test_rnode_store_manager_from_genesis,
    };

    crate::init_logger();
    let genesis_context = crate::util::rholang::resources::genesis_context()
        .await
        .unwrap();
    let genesis_post_state = genesis_context
        .genesis_block
        .body
        .state
        .post_state_hash
        .clone();

    let bridge_rho = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources/bridge.rho"),
    )
    .expect("Failed to read bridge.rho");

    let mut kvm = mk_test_rnode_store_manager_from_genesis(&genesis_context);
    let (rm, _) = mk_runtime_manager_with_history_at(&mut *kvm).await;

    let uri_regex = regex::Regex::new(r"rho:id:[a-zA-Z0-9]+").unwrap();

    let make_deploy_id_par = |sig: &[u8]| -> models::rhoapi::Par {
        models::rhoapi::Par {
            unforgeables: vec![models::rhoapi::GUnforgeable {
                unf_instance: Some(models::rhoapi::g_unforgeable::UnfInstance::GDeployIdBody(
                    models::rhoapi::GDeployId { sig: sig.to_vec() },
                )),
            }],
            ..Default::default()
        }
    };

    let mut block_number = 0u64;
    let mut current_state = genesis_post_state.clone();

    // Step 1: Deploy bridge.rho
    tracing::info!("Step 1: Deploying bridge.rho");
    block_number += 1;
    let deploy1 =
        construct_deploy::source_deploy_now_full(bridge_rho, None, None, None, None, None).unwrap();

    let (post_state_1, pd1_vec, _) = rm
        .compute_state(
            &current_state,
            vec![deploy1],
            Vec::new(),
            BlockData {
                time_stamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64,
                block_number: block_number as i64,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: block_number as i32,
            },
            None,
        )
        .await
        .unwrap();

    let pd1 = &pd1_vec[0];
    assert!(
        !pd1.is_failed,
        "Step 1: bridge deploy failed: {:?}",
        pd1.system_deploy_error
    );
    tracing::info!(
        "Step 1: cost={}, events={}",
        pd1.cost.cost,
        pd1.deploy_log.len()
    );

    let deploy1_data = rm
        .get_data(
            post_state_1.clone(),
            &make_deploy_id_par(&pd1_vec[0].deploy.sig),
        )
        .await
        .unwrap();
    assert!(
        !deploy1_data.is_empty(),
        "Step 1: bridge deploy wrote no data to deployId"
    );

    let data_str = format!("{:?}", deploy1_data);
    let uris: Vec<String> = uri_regex
        .find_iter(&data_str)
        .map(|m| m.as_str().to_string())
        .collect();
    let mut unique_uris: Vec<String> = Vec::new();
    for uri in &uris {
        if !unique_uris.contains(uri) {
            unique_uris.push(uri.clone());
        }
    }
    assert!(
        unique_uris.len() >= 2,
        "Expected at least 2 URIs, got: {:?}",
        unique_uris
    );
    let query_uri = unique_uris[0].clone();
    let admin_uri = unique_uris.last().unwrap().clone();
    tracing::info!("  queryUri: {}, adminUri: {}", query_uri, admin_uri);
    current_state = post_state_1;

    // Steps 2-7: getNonce + admin calls
    let steps: Vec<(&str, String)> = vec![
        (
            "getNonce",
            format!(
                r#"
new deployId(`rho:system:deployId`),
    lookup(`rho:registry:lookup`),
    queryCh, ret
in {{
  lookup!(`{}`, *queryCh) |
  for (query <- queryCh) {{
    query!("getNonce", Nil, *ret) |
    for (@result <- ret) {{ deployId!(result) }}
  }}
}}
"#,
                query_uri
            ),
        ),
        (
            "setVerifier",
            format!(
                r#"
new deployId(`rho:system:deployId`), deployerId(`rho:system:deployerId`),
    lookup(`rho:registry:lookup`), VaultAddress(`rho:vault:address`),
    adminBridgeCh, callerAddrCh, ret
in {{
  lookup!(`{}`, *adminBridgeCh) |
  VaultAddress!("fromDeployerId", *deployerId, *callerAddrCh) |
  for (adminBridge <- adminBridgeCh; @callerAddr <- callerAddrCh) {{
    adminBridge!("setVerifier", callerAddr, "verifier_v2", *ret) |
    for (@result <- ret) {{ deployId!(result) }}
  }}
}}
"#,
                admin_uri
            ),
        ),
        (
            "setRelayer",
            format!(
                r#"
new deployId(`rho:system:deployId`), deployerId(`rho:system:deployerId`),
    lookup(`rho:registry:lookup`), VaultAddress(`rho:vault:address`),
    adminBridgeCh, callerAddrCh, ret
in {{
  lookup!(`{}`, *adminBridgeCh) |
  VaultAddress!("fromDeployerId", *deployerId, *callerAddrCh) |
  for (adminBridge <- adminBridgeCh; @callerAddr <- callerAddrCh) {{
    adminBridge!("setRelayer", callerAddr, "relayer_addr_1", *ret) |
    for (@result <- ret) {{ deployId!(result) }}
  }}
}}
"#,
                admin_uri
            ),
        ),
        (
            "setRequiredSignatures",
            format!(
                r#"
new deployId(`rho:system:deployId`), deployerId(`rho:system:deployerId`),
    lookup(`rho:registry:lookup`), VaultAddress(`rho:vault:address`),
    adminBridgeCh, callerAddrCh, ret
in {{
  lookup!(`{}`, *adminBridgeCh) |
  VaultAddress!("fromDeployerId", *deployerId, *callerAddrCh) |
  for (adminBridge <- adminBridgeCh; @callerAddr <- callerAddrCh) {{
    adminBridge!("setRequiredSignatures", callerAddr, 2, *ret) |
    for (@result <- ret) {{ deployId!(result) }}
  }}
}}
"#,
                admin_uri
            ),
        ),
        (
            "addOracle",
            format!(
                r#"
new deployId(`rho:system:deployId`), deployerId(`rho:system:deployerId`),
    lookup(`rho:registry:lookup`), VaultAddress(`rho:vault:address`),
    adminBridgeCh, callerAddrCh, ret
in {{
  lookup!(`{}`, *adminBridgeCh) |
  VaultAddress!("fromDeployerId", *deployerId, *callerAddrCh) |
  for (adminBridge <- adminBridgeCh; @callerAddr <- callerAddrCh) {{
    adminBridge!("addOracle", callerAddr, "oracle-4", *ret) |
    for (@result <- ret) {{ deployId!(result) }}
  }}
}}
"#,
                admin_uri
            ),
        ),
        (
            "removeOracle",
            format!(
                r#"
new deployId(`rho:system:deployId`), deployerId(`rho:system:deployerId`),
    lookup(`rho:registry:lookup`), VaultAddress(`rho:vault:address`),
    adminBridgeCh, callerAddrCh, ret
in {{
  lookup!(`{}`, *adminBridgeCh) |
  VaultAddress!("fromDeployerId", *deployerId, *callerAddrCh) |
  for (adminBridge <- adminBridgeCh; @callerAddr <- callerAddrCh) {{
    adminBridge!("removeOracle", callerAddr, "oracle-4", *ret) |
    for (@result <- ret) {{ deployId!(result) }}
  }}
}}
"#,
                admin_uri
            ),
        ),
    ];

    let mut failures = Vec::new();
    for (name, code) in &steps {
        block_number += 1;
        tracing::info!("{}", name);

        let deploy =
            construct_deploy::source_deploy_now_full(code.clone(), None, None, None, None, None)
                .unwrap();

        let (post_state_n, pdn_vec, _) = rm
            .compute_state(
                &current_state,
                vec![deploy],
                Vec::new(),
                BlockData {
                    time_stamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as i64,
                    block_number: block_number as i64,
                    sender: genesis_context.validator_pks()[0].clone(),
                    seq_num: block_number as i32,
                },
                None,
            )
            .await
            .unwrap();

        let pdn = &pdn_vec[0];
        assert!(
            !pdn.is_failed,
            "{}: deploy failed: {:?}",
            name, pdn.system_deploy_error
        );
        assert!(
            pdn.cost.cost > 0,
            "{}: successful contract interaction recorded zero consensus cost",
            name
        );
        let deploy_data = rm
            .get_data(
                post_state_n.clone(),
                &make_deploy_id_par(&pdn_vec[0].deploy.sig),
            )
            .await
            .unwrap();
        let has_data = !deploy_data.is_empty();
        tracing::info!(
            "  {}: cost={}, events={}, deployId_data={}",
            name,
            pdn.cost.cost,
            pdn.deploy_log.len(),
            has_data
        );

        if !has_data {
            failures.push(format!(
                "{} returned no data. cost={}, events={}",
                name,
                pdn.cost.cost,
                pdn.deploy_log.len()
            ));
        }
        current_state = post_state_n;
    }

    assert!(
        failures.is_empty(),
        "Bridge admin API failures:\n{}",
        failures.join("\n")
    );
}

/// Tests that bridge registry entries survive multi-parent DAG merge.
///
/// Deploys bridge.rho on block A (from genesis), creates empty block B (from
/// genesis, sibling branch), merges [A, B] via compute_parents_post_state,
/// then queries getNonce from the merged state.
///
/// Reproduces: system-integration docs/TODO.md "Contract query deploy returns
/// empty deployId after finalization (intermittent)"
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bridge_query_survives_multi_parent_merge() {
    use std::collections::HashMap;

    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
    use casper::rust::genesis::genesis::Genesis;
    use casper::rust::util::proto_util;
    use casper::rust::util::rholang::interpreter_util::{
        compute_deploys_checkpoint_cosigned, compute_parents_post_state,
    };
    use dashmap::DashSet;
    use models::rust::block_hash::BlockHash;
    use models::rust::block_implicits;
    use rholang::rust::interpreter::external_services::ExternalServices;

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mergeable_store_from_dyn,
        mk_test_rnode_store_manager_from_genesis,
    };

    crate::init_logger();
    let genesis_context = crate::util::rholang::resources::genesis_context()
        .await
        .unwrap();
    let genesis_block = genesis_context.genesis_block.clone();
    let genesis_hash = genesis_block.block_hash.clone();
    let genesis_state = proto_util::post_state_hash(&genesis_block);
    let genesis_bonds = genesis_block.body.state.bonds.clone();
    let validator: prost::bytes::Bytes = genesis_context.validator_pks()[0].bytes.clone();
    let shard_name = genesis_block.shard_id.clone();

    // Create all stores from the same KVM (shared genesis scope)
    let mut kvm = mk_test_rnode_store_manager_from_genesis(&genesis_context);

    let rspace_store = kvm.r_space_stores().await.expect("rspace stores");
    let mergeable_store = mergeable_store_from_dyn(&mut *kvm)
        .await
        .expect("mergeable store");
    let (rm, _) = RuntimeManager::create_with_history(
        rspace_store,
        mergeable_store,
        std::sync::Arc::new(Genesis::default_mergeable_tags()),
        ExternalServices::noop(),
    );

    let mut block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");

    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(
            &genesis_block,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::ApprovedGenesis,
        )
        .expect("dag genesis");

    let now_millis = || -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    };

    let mk_snapshot = |lfb: &BlockHash| -> CasperSnapshot {
        let mut snapshot = CasperSnapshot::new(
            dag_storage
                .get_representation()
                .expect("dag representation"),
        );
        snapshot.last_finalized_block = lfb.clone();
        let mut max_seq_nums: HashMap<prost::bytes::Bytes, u64> = HashMap::new();
        max_seq_nums.insert(validator.clone(), 0);
        snapshot.max_seq_nums = max_seq_nums;
        let mut shard_conf = CasperShardConf::new();
        shard_conf.shard_name = shard_name.clone();
        shard_conf.max_parent_depth = 0;
        let mut bonds_map = HashMap::new();
        bonds_map.insert(validator.clone(), 100);
        snapshot.on_chain_state = OnChainCasperState {
            shard_conf,
            bonds_map,
            bond_generations: HashMap::from([(
                validator.clone(),
                models::rust::bond_generation::BondGeneration::GENESIS,
            )]),
            active_validators: vec![validator.clone()],
        };
        snapshot.deploys_in_scope = std::sync::Arc::new(DashSet::new());
        snapshot
    };

    let make_deploy_id_par = |sig: &[u8]| -> models::rhoapi::Par {
        models::rhoapi::Par {
            unforgeables: vec![models::rhoapi::GUnforgeable {
                unf_instance: Some(models::rhoapi::g_unforgeable::UnfInstance::GDeployIdBody(
                    models::rhoapi::GDeployId { sig: sig.to_vec() },
                )),
            }],
            ..Default::default()
        }
    };

    let bridge_rho = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources/bridge.rho"),
    )
    .expect("Failed to read bridge.rho");

    // --- Block A: bridge deploy from genesis ---
    let bridge_deploy = protocol_v6_envelope(
        construct_deploy::source_deploy_now_full(bridge_rho, None, None, None, None, None).unwrap(),
        construct_deploy::DEFAULT_SEC.clone(),
    );

    let block_a_raw = block_implicits::get_random_block(
        Some(1),
        Some(1),
        Some(genesis_state.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(genesis_block.header.version),
        Some(now_millis()),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty_from_cosigned(&bridge_deploy)]),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );

    let parents_a = vec![genesis_block.clone()];
    let snapshot_a = mk_snapshot(&genesis_hash);
    let (_, post_state_a, pd_a, _, sys_pd_a, bonds_a) = compute_deploys_checkpoint_cosigned(
        &mut block_store,
        parents_a,
        vec![bridge_deploy],
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &snapshot_a,
        &rm,
        BlockData::from_block(&block_a_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute block A");

    assert!(
        !pd_a[0].is_failed,
        "Bridge deploy failed: {:?}",
        pd_a[0].system_deploy_error
    );

    let mut block_a = block_a_raw;
    block_a.body.state.post_state_hash = post_state_a.clone();
    block_a.body.deploys = pd_a.clone();
    block_a.body.system_deploys = sys_pd_a;
    block_a.body.state.bonds = bonds_a;
    block_store.put_block_message(&block_a).expect("store A");
    dag_storage
        .insert(
            &block_a,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .expect("dag A");

    // Verify bridge wrote data and extract queryUri
    let bridge_data = rm
        .get_data(
            post_state_a.clone(),
            &make_deploy_id_par(pd_a[0].deploy_id()),
        )
        .await
        .unwrap();
    assert!(
        !bridge_data.is_empty(),
        "Bridge deploy wrote no data to deployId"
    );

    let uri_regex = regex::Regex::new(r"rho:id:[a-zA-Z0-9]+").unwrap();
    let data_str = format!("{:?}", bridge_data);
    let uris: Vec<String> = uri_regex
        .find_iter(&data_str)
        .map(|m| m.as_str().to_string())
        .collect();
    let mut unique_uris: Vec<String> = Vec::new();
    for uri in &uris {
        if !unique_uris.contains(uri) {
            unique_uris.push(uri.clone());
        }
    }
    assert!(
        unique_uris.len() >= 2,
        "Expected at least 2 URIs, got: {:?}",
        unique_uris
    );
    let query_uri = unique_uris[0].clone();

    // --- Block B: empty block from genesis (sibling branch) ---
    let block_b_raw = block_implicits::get_random_block(
        Some(1),
        Some(2),
        Some(genesis_state.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(genesis_block.header.version),
        Some(now_millis()),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );

    let parents_b = vec![genesis_block.clone()];
    let snapshot_b = mk_snapshot(&genesis_hash);
    let (_, post_state_b, pd_b, _, sys_pd_b, bonds_b) = compute_deploys_checkpoint_cosigned(
        &mut block_store,
        parents_b,
        Vec::new(),
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &snapshot_b,
        &rm,
        BlockData::from_block(&block_b_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute block B");

    let mut block_b = block_b_raw;
    block_b.body.state.post_state_hash = post_state_b.clone();
    block_b.body.deploys = pd_b;
    block_b.body.system_deploys = sys_pd_b;
    block_b.body.state.bonds = bonds_b;
    block_store.put_block_message(&block_b).expect("store B");
    dag_storage
        .insert(
            &block_b,
            block_storage::rust::dag::block_dag_key_value_storage::InsertMode::Normal,
        )
        .expect("dag B");

    // --- Merge [A, B] ---
    let parents = vec![block_a.clone(), block_b.clone()];
    let snapshot_merge = mk_snapshot(&genesis_hash);
    let latest_messages: std::collections::BTreeMap<_, _> = snapshot_merge
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let merged = compute_parents_post_state(
        &block_store,
        parents,
        &snapshot_merge,
        &rm,
        &latest_messages,
        None,
        None,
        None,
        None,
    )
    .await
    .expect("merge parents");
    let merged_state = merged.state;
    let rejected = merged.rejected_user;

    assert!(
        rejected.is_empty(),
        "Merge rejected deploys: {:?}",
        rejected
    );
    // --- Query getNonce from merged state ---
    let get_nonce_rho = format!(
        r#"
new deployId(`rho:system:deployId`),
    lookup(`rho:registry:lookup`),
    queryCh, ret
in {{
  lookup!(`{}`, *queryCh) |
  for (query <- queryCh) {{
    query!("getNonce", Nil, *ret) |
    for (@result <- ret) {{ deployId!(result) }}
  }}
}}
"#,
        query_uri
    );

    let query_deploy = protocol_v6_envelope(
        construct_deploy::source_deploy_now_full(get_nonce_rho, None, None, None, None, None)
            .unwrap(),
        construct_deploy::DEFAULT_SEC.clone(),
    );

    let query_block_raw = block_implicits::get_random_block(
        Some(2),
        Some(3),
        Some(merged_state.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(genesis_block.header.version),
        Some(now_millis()),
        Some(vec![block_a.block_hash.clone(), block_b.block_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty_from_cosigned(&query_deploy)]),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );

    let parents_q = vec![block_a.clone(), block_b.clone()];
    let snapshot_q = mk_snapshot(&genesis_hash);
    let (_, post_state_q, pd_q, _, _, _) = compute_deploys_checkpoint_cosigned(
        &mut block_store,
        parents_q,
        vec![query_deploy],
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &snapshot_q,
        &rm,
        BlockData::from_block(&query_block_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute query block");

    assert!(
        !pd_q[0].is_failed,
        "Query deploy failed: {:?}",
        pd_q[0].system_deploy_error
    );

    let query_data = rm
        .get_data(post_state_q, &make_deploy_id_par(pd_q[0].deploy_id()))
        .await
        .unwrap();

    assert!(
        !query_data.is_empty(),
        "Bridge query returned empty deployId after multi-parent merge. \
         The merge did not preserve the bridge's registry entries when \
         combining a bridge branch with an empty sibling branch."
    );
}

/// Two independent contracts both call insertArbitrary, inserting DISTINCT
/// registry leaves from sibling branches. They genuinely RACE at the raw level —
/// both read-modify-write the SAME shared TreeHashMap internal-node produce
/// channels (`racesForSameIOEvent: produceRaces=2`) — yet because they touch
/// DIFFERENT leaves the merge keeps BOTH: the keep-one / §3c discriminator
/// classifies the shared-internal-node produces as mergeable, not a genuine
/// single-value-cell conflict. The correct outcome is therefore `rejected == 0`
/// with both contracts' data present in the merged state (verified below).
///
/// Historical note: this was `#[ignore]`d under an earlier premise that one insert
/// "must be rejected"; the current keep-one design correctly merges these distinct
/// leaves, so that premise no longer holds and the test is re-enabled. The genuine
/// raw race is asserted (below) so `rejected == 0` is a real keep-one result, not a
/// vacuous merge of disjoint branches.
///
/// The raw race is PROBABILISTIC per deploy pair: Registry.rho's TreeHashMap
/// setter locks (consume+produce) only the deepest node that already exists on
/// the key's hash path (everything above is peeked), so two fresh random URIs
/// share a consumed produce only when their paths collide at/below their
/// divergence from the genesis trie (measured ~5/6 per pair). The test therefore
/// searches a bounded number of fresh deploy pairs for one that genuinely races
/// and asserts no-conflict on THAT pair.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_registry_inserts_should_not_conflict() {
    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
    use casper::rust::genesis::genesis::Genesis;
    use casper::rust::util::proto_util;
    use casper::rust::util::rholang::interpreter_util::{
        compute_deploys_checkpoint_legacy_signer, compute_parents_post_state,
    };
    use dashmap::DashSet;
    use models::rust::block_hash::BlockHash;
    use models::rust::block_implicits;
    use rholang::rust::interpreter::external_services::ExternalServices;

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mergeable_store_from_dyn,
        mk_test_rnode_store_manager_from_genesis,
    };

    crate::init_logger();
    let genesis_context = crate::util::rholang::resources::genesis_context()
        .await
        .unwrap();
    let genesis_block = genesis_context.genesis_block.clone();
    let genesis_hash = genesis_block.block_hash.clone();
    let genesis_state = proto_util::post_state_hash(&genesis_block);
    let genesis_bonds = genesis_block.body.state.bonds.clone();
    let validator: prost::bytes::Bytes = genesis_context.validator_pks()[0].bytes.clone();
    let shard_name = genesis_block.shard_id.clone();

    let mut kvm = mk_test_rnode_store_manager_from_genesis(&genesis_context);
    let rspace_store = kvm.r_space_stores().await.expect("rspace stores");
    let mergeable_store = mergeable_store_from_dyn(&mut *kvm)
        .await
        .expect("mergeable store");
    let (rm, _) = RuntimeManager::create_with_history(
        rspace_store,
        mergeable_store,
        std::sync::Arc::new(Genesis::default_mergeable_tags()),
        ExternalServices::noop(),
    );

    let mut block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");

    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(&genesis_block, InsertMode::ApprovedGenesis)
        .expect("dag genesis");

    let now_millis = || -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    };

    let mk_snapshot = |lfb: &BlockHash| -> CasperSnapshot {
        let mut snapshot = CasperSnapshot::new(
            dag_storage
                .get_representation()
                .expect("dag representation"),
        );
        snapshot.last_finalized_block = lfb.clone();
        let mut max_seq_nums: HashMap<prost::bytes::Bytes, u64> = HashMap::new();
        max_seq_nums.insert(validator.clone(), 0);
        snapshot.max_seq_nums = max_seq_nums;
        let mut shard_conf = CasperShardConf::new();
        shard_conf.shard_name = shard_name.clone();
        shard_conf.max_parent_depth = 0;
        let mut bonds_map = HashMap::new();
        bonds_map.insert(validator.clone(), 100);
        snapshot.on_chain_state = OnChainCasperState {
            shard_conf,
            bonds_map,
            bond_generations: HashMap::from([(
                validator.clone(),
                models::rust::bond_generation::BondGeneration::GENESIS,
            )]),
            active_validators: vec![validator.clone()],
        };
        snapshot.deploys_in_scope = std::sync::Arc::new(DashSet::new());
        snapshot
    };

    // Both blocks deploy bridge-v2.rho — a complex contract with vault operations,
    // registry inserts, and many shared channel interactions.
    // Use different genesis validator keys so both deployers have funded vaults.
    let bridge_rho = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources/bridge.rho"),
    )
    .expect("Failed to read bridge.rho");

    // Use DEFAULT_SEC / DEFAULT_SEC2 — these have funded vaults (9M balance) in genesis.
    // Validator keys have 0 balance and can't deploy.
    let key_a = construct_deploy::DEFAULT_SEC.clone();
    let key_b = construct_deploy::DEFAULT_SEC2.clone();

    // The cross-deploy race is probabilistic (see the doc comment): search a
    // bounded number of fresh deploy pairs for one that genuinely races, then
    // run the no-conflict assertions on THAT pair. With measured per-pair
    // no-race odds of ~1/6, ten misses (~2e-8) can only mean the genesis-trie
    // shape or the event-log classification changed — fail loudly below.
    const MAX_RACE_SEARCH_ATTEMPTS: usize = 10;
    let base_ts = now_millis();
    let mut found = None;
    for attempt in 0..MAX_RACE_SEARCH_ATTEMPTS {
        // Fresh timestamps ⇒ fresh RFC6979 signatures ⇒ fresh insertArbitrary
        // URIs (and a fresh vault address) for every attempt.
        let attempt_ts = base_ts + (2 * attempt) as i64;
        // --- Block A: bridge deploy from genesis (funded deployer A) ---
        let deploy_a = construct_deploy::source_deploy(
            bridge_rho.clone(),
            attempt_ts,
            None,
            None,
            Some(key_a.clone()),
            None,
            None,
        )
        .unwrap();

        let block_a_raw = block_implicits::get_random_block(
            Some(1),
            Some(1),
            Some(genesis_state.clone()),
            Some(StateHash::default()),
            Some(validator.clone()),
            Some(genesis_block.header.version),
            Some(now_millis()),
            Some(vec![genesis_hash.clone()]),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy_a)]),
            Some(Vec::new()),
            Some(genesis_bonds.clone()),
            Some(shard_name.clone()),
            None,
        );

        let parents_a = vec![genesis_block.clone()];
        let deploys_a = proto_util::deploys(&block_a_raw)
            .into_iter()
            .map(|d| d.deploy)
            .collect();
        let snapshot_a = mk_snapshot(&genesis_hash);
        let (_, post_state_a, pd_a, _, sys_pd_a, bonds_a) =
            compute_deploys_checkpoint_legacy_signer(
                &mut block_store,
                parents_a,
                deploys_a,
                Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
                &snapshot_a,
                &rm,
                BlockData::from_block(&block_a_raw),
                HashMap::new(),
                None,
            )
            .await
            .expect("compute block A");

        assert!(
            !pd_a[0].is_failed,
            "Contract A deploy failed: {:?}",
            pd_a[0].system_deploy_error
        );
        tracing::info!(
            "Block A: cost={}, events={}",
            pd_a[0].cost.cost,
            pd_a[0].deploy_log.len()
        );

        let mut block_a = block_a_raw;
        block_a.body.state.post_state_hash = post_state_a.clone();
        block_a.body.deploys = pd_a.clone();
        block_a.body.system_deploys = sys_pd_a;
        block_a.body.state.bonds = bonds_a;

        // --- Block B: second bridge deploy from genesis (sibling branch, funded deployer B) ---
        let deploy_b = construct_deploy::source_deploy(
            bridge_rho.clone(),
            attempt_ts + 1,
            None,
            None,
            Some(key_b.clone()),
            None,
            None,
        )
        .unwrap();

        let block_b_raw = block_implicits::get_random_block(
            Some(1),
            Some(2),
            Some(genesis_state.clone()),
            Some(StateHash::default()),
            Some(validator.clone()),
            Some(genesis_block.header.version),
            Some(now_millis()),
            Some(vec![genesis_hash.clone()]),
            Some(Vec::new()),
            Some(vec![ProcessedDeploy::empty(deploy_b)]),
            Some(Vec::new()),
            Some(genesis_bonds.clone()),
            Some(shard_name.clone()),
            None,
        );

        let parents_b = vec![genesis_block.clone()];
        let deploys_b = proto_util::deploys(&block_b_raw)
            .into_iter()
            .map(|d| d.deploy)
            .collect();
        let snapshot_b = mk_snapshot(&genesis_hash);
        let (_, post_state_b, pd_b, _, sys_pd_b, bonds_b) =
            compute_deploys_checkpoint_legacy_signer(
                &mut block_store,
                parents_b,
                deploys_b,
                Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
                &snapshot_b,
                &rm,
                BlockData::from_block(&block_b_raw),
                HashMap::new(),
                None,
            )
            .await
            .expect("compute block B");

        assert!(
            !pd_b[0].is_failed,
            "Contract B deploy failed: {:?}",
            pd_b[0].system_deploy_error
        );
        tracing::info!(
            "Block B: cost={}, events={}",
            pd_b[0].cost.cost,
            pd_b[0].deploy_log.len()
        );

        let mut block_b = block_b_raw;
        block_b.body.state.post_state_hash = post_state_b.clone();
        block_b.body.deploys = pd_b.clone();
        block_b.body.system_deploys = sys_pd_b;
        block_b.body.state.bonds = bonds_b;

        // Analyze conflict between the two deploys' event logs BEFORE merge
        {
            use casper::rust::merging::block_index::create_event_log_index;
            use rspace_plus_plus::rspace::merger::merging_logic::{conflict_reason, conflicts};

            let history_repo = rm.get_history_repo();
            let genesis_hash_b256 =
                rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash::from_bytes_prost(
                    &genesis_state,
                );

            let eli_a = create_event_log_index(
                &pd_a[0].deploy_log,
                history_repo.clone(),
                &genesis_hash_b256,
                std::collections::BTreeMap::new(),
            );
            let eli_b = create_event_log_index(
                &pd_b[0].deploy_log,
                history_repo.clone(),
                &genesis_hash_b256,
                std::collections::BTreeMap::new(),
            );

            let reason = conflict_reason(&eli_a, &eli_b);
            let conflict_channels = conflicts(&eli_a, &eli_b);
            tracing::info!(
                "Conflict analysis: reason={:?}, conflicting_channels={}",
                reason,
                conflict_channels.0.len(),
            );
            for ch in &conflict_channels.0 {
                tracing::info!("  conflicting channel: {}", hex::encode(&ch.0[..8]));
            }

            // Find which produces are racing
            let shared_produces: std::collections::HashSet<_> = eli_a
                .produces_consumed
                .0
                .intersection(&eli_b.produces_consumed.0)
                .cloned()
                .collect();
            let mergeable_produces: std::collections::HashSet<_> = eli_a
                .produces_mergeable
                .0
                .intersection(&eli_b.produces_mergeable.0)
                .cloned()
                .collect();
            let racing_produces: Vec<_> = shared_produces
                .difference(&mergeable_produces)
                .filter(|p| !p.persistent)
                .collect();
            tracing::info!("Racing produces: {}", racing_produces.len());
            // Non-vacuity: the two inserts must GENUINELY race on shared internal-node
            // produce channels, else `rejected == 0` below would be a trivial merge of
            // disjoint branches rather than a real keep-one/exemption result. The race
            // is probabilistic per URI set (Registry.rho's TreeHashMap locks only the
            // deepest pre-existing node on each key's hash path; measured ~5/6 per
            // pair), so a non-racing pair is REGENERATED with fresh timestamps rather
            // than failed.
            if racing_produces.is_empty() {
                tracing::warn!(
                    attempt,
                    "no shared trie-node race for this URI set (all registry-insert \
                     pairs + the vault pair diverged above the genesis trie); \
                     regenerating the deploy pair with fresh timestamps"
                );
                continue;
            }
            // Collect racing channel hashes for COMM tracing
            let racing_channels: std::collections::HashSet<_> = racing_produces
                .iter()
                .map(|p| p.channel_hash.clone())
                .collect();

            // Search deploy A's event log for COMMs involving racing channels
            tracing::info!(
                "Searching deploy A events ({} total) for racing channels...",
                pd_a[0].deploy_log.len()
            );
            for (idx, event) in pd_a[0].deploy_log.iter().enumerate() {
                use models::rust::casper::protocol::casper_message::Event as CasperEvent;
                match event {
                    CasperEvent::Comm(comm) => {
                        let consume_channels: Vec<String> = comm
                            .consume
                            .channels_hashes
                            .iter()
                            .map(|h| hex::encode(&h[..std::cmp::min(8, h.len())]))
                            .collect();
                        let produce_channels: Vec<String> = comm
                            .produces
                            .iter()
                            .map(|p| {
                                hex::encode(
                                    &p.channels_hash[..std::cmp::min(8, p.channels_hash.len())],
                                )
                            })
                            .collect();
                        // Check if any racing channel is in this COMM's produces
                        for p in &comm.produces {
                            let ch = rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash::from_bytes_prost(&p.channels_hash);
                            if racing_channels.contains(&ch) {
                                tracing::info!(
                                    "  A event[{}] COMM: consume_channels={:?}, produce_channels={:?}, peeks={:?}, persistent_consume={}",
                                    idx,
                                    consume_channels,
                                    produce_channels,
                                    comm.peeks,
                                    comm.consume.persistent,
                                );
                            }
                        }
                    }
                    CasperEvent::Produce(p) => {
                        let ch = rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash::from_bytes_prost(&p.channels_hash);
                        if racing_channels.contains(&ch) {
                            tracing::info!(
                                "  A event[{}] IOProduce: channel={}, persistent={}, output_len={}",
                                idx,
                                hex::encode(
                                    &p.channels_hash[..std::cmp::min(8, p.channels_hash.len())]
                                ),
                                p.persistent,
                                p.output_value.len(),
                            );
                        }
                    }
                    CasperEvent::Consume(c) => {
                        for h in &c.channels_hashes {
                            let ch = rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash::from_bytes_prost(h);
                            if racing_channels.contains(&ch) {
                                tracing::info!(
                                    "  A event[{}] IOConsume: channels={:?}, persistent={}",
                                    idx,
                                    c.channels_hashes
                                        .iter()
                                        .map(|h| hex::encode(&h[..std::cmp::min(8, h.len())]))
                                        .collect::<Vec<_>>(),
                                    c.persistent,
                                );
                            }
                        }
                    }
                }
            }

            for p in &racing_produces {
                // Decode the output_value to see what data is being raced for
                let output_str: Vec<String> = p
                    .output_value
                    .iter()
                    .map(|v| {
                        format!(
                            "raw({} bytes, first8={})",
                            v.len(),
                            hex::encode(&v[..std::cmp::min(8, v.len())])
                        )
                    })
                    .collect();
                tracing::info!(
                    "  racing produce: channel={}, hash={}, persistent={}, output={:?}",
                    hex::encode(&p.channel_hash.0[..8]),
                    hex::encode(&p.hash.0[..8]),
                    p.persistent,
                    output_str,
                );
            }
        }
        found = Some((block_a, block_b, pd_a, pd_b));
        break;
    }
    let (block_a, block_b, pd_a, pd_b) = found.expect(
        "no genuinely-racing deploy pair in 10 attempts: per-attempt no-race odds \
         are ~1/6, so 10 misses (~2e-8) means the genesis trie shape or the \
         event-log classification changed — investigate; do not loosen this assert",
    );

    // Store ONLY the winning racing pair; failed search attempts must not
    // pollute the DAG that the merge snapshot below reads.
    block_store.put_block_message(&block_a).expect("store A");
    dag_storage
        .insert(&block_a, InsertMode::Normal)
        .expect("dag A");
    block_store.put_block_message(&block_b).expect("store B");
    dag_storage
        .insert(&block_b, InsertMode::Normal)
        .expect("dag B");

    // --- Merge [A, B] ---
    let parents = vec![block_a.clone(), block_b.clone()];
    let snapshot_merge = mk_snapshot(&genesis_hash);
    let latest_messages: std::collections::BTreeMap<_, _> = snapshot_merge
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let merged = compute_parents_post_state(
        &block_store,
        parents,
        &snapshot_merge,
        &rm,
        &latest_messages,
        None,
        None,
        None,
        None,
    )
    .await
    .expect("merge parents");
    let merged_state = merged.state;
    let rejected = merged.rejected_user;

    tracing::info!(
        "Merge result: rejected={}, merged_state={}",
        rejected.len(),
        hex::encode(&merged_state[..8]),
    );

    if !rejected.is_empty() {
        let rejected_sigs: Vec<String> = rejected
            .iter()
            .map(|d| hex::encode(&d.deploy_id()[..std::cmp::min(8, d.deploy_id().len())]))
            .collect();
        tracing::warn!(
            "CONFLICT DETECTED: {} deploys rejected: {:?}",
            rejected.len(),
            rejected_sigs,
        );

        // Identify which deploy was rejected
        let a_sig = hex::encode(&pd_a[0].deploy.sig[..8]);
        let b_sig = hex::encode(&pd_b[0].deploy.sig[..8]);
        let a_rejected = rejected_sigs.contains(&a_sig);
        let b_rejected = rejected_sigs.contains(&b_sig);
        tracing::warn!(
            "  Contract A ({}): {}",
            a_sig,
            if a_rejected { "REJECTED" } else { "kept" },
        );
        tracing::warn!(
            "  Contract B ({}): {}",
            b_sig,
            if b_rejected { "REJECTED" } else { "kept" },
        );
    }

    // Key assertion: keep-one correctly keeps BOTH inserts (0 rejected). They
    // raw-race on the shared TreeHashMap internal-node produce channels (asserted
    // non-empty above), but write DISTINCT leaves, so the §3c discriminator
    // classifies those shared-node produces as mergeable rather than a genuine
    // single-value-cell conflict. A rejection here would be a real regression: a
    // genuinely-mergeable race mis-rejected.
    assert!(
        rejected.is_empty(),
        "concurrent insertArbitrary to DISTINCT registry leaves must merge with 0 \
         rejected (they share TreeHashMap internal nodes, but the produces there are \
         mergeable); got {} rejected — keep-one wrongly rejected a mergeable race.",
        rejected.len(),
    );

    // Verify both URIs accessible from merged state
    let make_deploy_id_par = |sig: &[u8]| -> models::rhoapi::Par {
        models::rhoapi::Par {
            unforgeables: vec![models::rhoapi::GUnforgeable {
                unf_instance: Some(models::rhoapi::g_unforgeable::UnfInstance::GDeployIdBody(
                    models::rhoapi::GDeployId { sig: sig.to_vec() },
                )),
            }],
            ..Default::default()
        }
    };

    let data_a = rm
        .get_data(
            merged_state.clone(),
            &make_deploy_id_par(&pd_a[0].deploy.sig),
        )
        .await
        .unwrap();
    let data_b = rm
        .get_data(
            merged_state.clone(),
            &make_deploy_id_par(&pd_b[0].deploy.sig),
        )
        .await
        .unwrap();
    tracing::info!("Contract A data in merged state: {} pars", data_a.len());
    tracing::info!("Contract B data in merged state: {} pars", data_b.len());

    assert!(
        !data_a.is_empty(),
        "Contract A data missing from merged state"
    );
    assert!(
        !data_b.is_empty(),
        "Contract B data missing from merged state"
    );
}

/// Verifies that exploratory deploy can query user-deployed contracts
/// through the registry. The `contract` keyword is reserved in Rholang,
/// so variable names in the query must not use it.
///
/// Also verifies that play_exploratory_deploy propagates errors (previously
/// errors were silently swallowed, returning empty results).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn exploratory_deploy_async_contract_query() {
    use crypto::rust::signatures::signatures_alg::SignaturesAlg;

    with_runtime_manager(
        |runtime_manager, _genesis_context, genesis_block| async move {
            let genesis_state = genesis_block.body.state.post_state_hash.clone();

            // Deploy a contract with a persistent state channel + persistent consume
            let contract_rho = r#"
new return, stateCh, queryCh,
    insertArbitrary(`rho:registry:insertArbitrary`)
in {
  stateCh!(42) |
  contract queryCh(@method, ret) = {
    for (@v <- stateCh) {
      stateCh!(v) |
      ret!(v)
    }
  } |
  new uriCh in {
    insertArbitrary!(bundle+{*queryCh}, *uriCh) |
    for (@uri <- uriCh) {
      return!(uri)
    }
  }
}
"#;

            // Use a unique key to avoid GPrivate collision with exploratory deploy's DEFAULT_SEC
            let (contract_key, _) = crypto::rust::signatures::secp256k1::Secp256k1.new_key_pair();
            let deploy = construct_deploy::source_deploy(
                contract_rho.to_string(),
                0,
                Some(500_000_000),
                None,
                Some(contract_key),
                None,
                None,
            )
            .unwrap();

            // Deploy and read URI via capture_results
            let uri_pars = runtime_manager
                .capture_results(&genesis_state, &deploy)
                .await
                .expect("deploy contract");
            assert!(!uri_pars.is_empty(), "Contract deploy returned no URI");

            let uri_str = format!("{:?}", uri_pars[0]);
            let uri_regex = regex::Regex::new(r"rho:id:[a-zA-Z0-9]+").unwrap();
            let uri = uri_regex
                .find(&uri_str)
                .expect("No rho:id URI found")
                .as_str()
                .to_string();

            // Checkpoint via a fresh runtime so exploratory deploy can see the state
            let runtime = runtime_manager.spawn_runtime().await;
            let mut runtime_ops = RuntimeOps::new(runtime);
            runtime_ops
                .runtime
                .reset(&Blake2b256Hash::from_bytes_prost(&genesis_state))
                .await
                .expect("reset");
            let eval_result = runtime_ops.evaluate(&deploy).await.expect("evaluate");
            assert!(
                eval_result.errors.is_empty(),
                "Deploy errors: {:?}",
                eval_result.errors
            );
            let checkpoint = runtime_ops.runtime.create_checkpoint().await;
            let post_state: StateHash = checkpoint.root.to_bytes_prost();
            tracing::info!(
                "Contract at {}, post_state={}",
                uri,
                hex::encode(&post_state[..8])
            );

            // Query with correct variable names (NOT using reserved word 'contract')
            let query_term = format!(
                r#"new ret, lookup(`rho:registry:lookup`), ch in {{
                lookup!(`{}`, *ch) |
                for (c <- ch) {{
                    c!("get", *ret)
                }}
            }}"#,
                uri
            );
            let (query_result, _) = runtime_manager
                .play_exploratory_deploy(query_term, &post_state, None)
                .await
                .expect("query exploratory deploy");
            tracing::info!("Query with correct var name: {} pars", query_result.len());
            assert_eq!(
                query_result.len(),
                1,
                "Query should return 1 par (the value 42)"
            );

            // Verify play_exploratory_deploy propagates parse errors (not swallows them)
            let bad_term = format!(
                r#"new ret, lookup(`rho:registry:lookup`), ch in {{
                lookup!(`{}`, *ch) |
                for (contract <- ch) {{
                    contract!("get", *ret)
                }}
            }}"#,
                uri
            );
            let bad_result = runtime_manager
                .play_exploratory_deploy(bad_term, &post_state, None)
                .await;
            assert!(
                bad_result.is_err(),
                "Using reserved word 'contract' as var name should return Err, not empty Ok"
            );
        },
    )
    .await
    .unwrap();
}

/// Reproduces the replay determinism issue seen with tokio::spawn.
/// Deploys a contract with parallel composition, plays it, then replays it.
/// If tokio::spawn introduces non-deterministic evaluation order, the replay
/// cost will differ from the play cost, causing ReplayCostMismatch.
///
/// Run: cargo test -p casper --test mod --release parallel_replay_determinism -- --nocapture
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_replay_determinism() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let gps = genesis_block.body.state.post_state_hash;

            // Registry lookup — system process with internal parallel composition
            let parallel_contract = r#"
                new rl(`rho:registry:lookup`), ch in {
                    rl!(`rho:vault:system`, *ch)
                }
            "#;

            let deploy = construct_deploy::source_deploy_now_full(
                parallel_contract.to_string(),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            let block_data = BlockData {
                time_stamp: time,
                block_number: 0,
                sender: genesis_context.validator_pks()[0].clone(),
                seq_num: 0,
            };

            // Play the deploy with CloseBlockDeploy system deploy
            let (play_state, processed_deploys, processed_sys_deploys) = runtime_manager
                .compute_state(
                    &gps,
                    vec![deploy],
                    vec![
                        casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum::Close(
                            CloseBlockDeploy::new(
                                system_deploy_util::generate_close_deploy_random_seed_from_pk(
                                    block_data.sender.clone(),
                                    block_data.seq_num,
                                ),
                            ),
                        ),
                    ],
                    block_data.clone(),
                    None,
                )
                .await
                .unwrap();

            let play_cost = processed_deploys[0].cost.cost;
            let play_failed = processed_deploys[0].is_failed;
            let play_event_count = processed_deploys[0].deploy_log.len();
            let sys_deploy_count = processed_sys_deploys.len();

            // Hash the event log for comparison
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            for ev in &processed_deploys[0].deploy_log {
                format!("{:?}", ev).hash(&mut hasher);
            }
            let event_log_hash = hasher.finish();

            println!("Play: cost={}, failed={}, events={}, sys_deploys={}, event_hash={:016x}, state={:?}",
                play_cost, play_failed, play_event_count, sys_deploy_count, event_log_hash, &play_state[..8]);

            // Replay the same deploy — must produce identical state and cost
            let replay_state = runtime_manager
                .replay_compute_state(
                    &gps,
                    processed_deploys,
                    processed_sys_deploys,
                    &block_data,
                    None,
                    false,
                )
                .await;

            match replay_state {
                Ok(state) => {
                    println!("Replay succeeded, state match: {}", state == play_state);
                    assert_eq!(state, play_state, "Play and replay produced different state hashes");
                }
                Err(CasperError::ReplayFailure(ReplayFailure::ReplayCostMismatch {
                    initial_cost,
                    replay_cost,
                })) => {
                    panic!(
                        "REPLAY DETERMINISM FAILURE: play cost={} but replay cost={}. \
                         This indicates non-deterministic evaluation order in parallel composition.",
                        initial_cost, replay_cost
                    );
                }
                Err(e) => {
                    panic!("Replay failed: {:?}", e);
                }
            }
        },
    )
    .await
    .unwrap();
}

/// Regression guard for the rejection-expansion behavior in `DagMerger::merge`.
///
/// DAG shape:
///
///        genesis (LCA)
///         /     \
///        BA      BB       bridge(key_A), bridge(key_B) — conflict on shared system channels
///        |       |
///        BC      BD       trivial writes by the same deployer as the ancestor
///
/// `compute_parents_post_state([BC, BD])` drives a merge whose scope is
/// `{BA, BB, BC, BD}`. One of BA/BB is rejected by conflict resolution.
/// Without rejection expansion, the descendant of the rejected block retains
/// pre-computed diffs against a pre-state that no longer materializes — the
/// merged post-state ends up with the descendant's writes present but the
/// ancestor's writes absent, which is internally inconsistent.
///
/// The expansion in DagMerger rejects the descendant's chains as well, so the
/// assertion below — "no ancestor-rejected-but-descendant-surviving" — holds.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stale_diff_application_corrupts_merged_state() {
    use std::collections::HashSet;

    use block_storage::rust::key_value_block_store::KeyValueBlockStore;
    use casper::rust::casper::{CasperShardConf, CasperSnapshot, OnChainCasperState};
    use casper::rust::genesis::genesis::Genesis;
    use casper::rust::util::proto_util;
    use casper::rust::util::rholang::interpreter_util::{
        compute_deploys_checkpoint_cosigned, compute_parents_post_state,
    };
    use dashmap::DashSet;
    use models::rust::block_hash::BlockHash;
    use models::rust::block_implicits;
    use rholang::rust::interpreter::external_services::ExternalServices;

    use crate::util::rholang::resources::{
        block_dag_storage_from_dyn, mergeable_store_from_dyn,
        mk_test_rnode_store_manager_from_genesis,
    };

    crate::init_logger();
    let genesis_context = crate::util::rholang::resources::genesis_context()
        .await
        .unwrap();
    let genesis_block = genesis_context.genesis_block.clone();
    let genesis_hash = genesis_block.block_hash.clone();
    let genesis_state = proto_util::post_state_hash(&genesis_block);
    let genesis_bonds = genesis_block.body.state.bonds.clone();
    let validator: prost::bytes::Bytes = genesis_context.validator_pks()[0].bytes.clone();
    let shard_name = genesis_block.shard_id.clone();

    let mut kvm = mk_test_rnode_store_manager_from_genesis(&genesis_context);
    let rspace_store = kvm.r_space_stores().await.expect("rspace stores");
    let mergeable_store = mergeable_store_from_dyn(&mut *kvm)
        .await
        .expect("mergeable store");
    let (rm, _) = RuntimeManager::create_with_history(
        rspace_store,
        mergeable_store,
        std::sync::Arc::new(Genesis::default_mergeable_tags()),
        ExternalServices::noop(),
    );

    let mut block_store = KeyValueBlockStore::create_from_kvm(&mut *kvm)
        .await
        .expect("block store");
    let dag_storage = block_dag_storage_from_dyn(&mut *kvm)
        .await
        .expect("dag storage");

    block_store
        .put_block_message(&genesis_block)
        .expect("store genesis");
    dag_storage
        .insert(&genesis_block, InsertMode::ApprovedGenesis)
        .expect("dag genesis");

    let now_millis = || -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    };

    let mk_snapshot = |lfb: &BlockHash| -> CasperSnapshot {
        let mut snapshot = CasperSnapshot::new(
            dag_storage
                .get_representation()
                .expect("dag representation"),
        );
        snapshot.last_finalized_block = lfb.clone();
        let mut max_seq_nums: HashMap<prost::bytes::Bytes, u64> = HashMap::new();
        max_seq_nums.insert(validator.clone(), 0);
        snapshot.max_seq_nums = max_seq_nums;
        let mut shard_conf = CasperShardConf::new();
        shard_conf.shard_name = shard_name.clone();
        shard_conf.max_parent_depth = 0;
        let mut bonds_map = HashMap::new();
        bonds_map.insert(validator.clone(), 100);
        snapshot.on_chain_state = OnChainCasperState {
            shard_conf,
            bonds_map,
            bond_generations: HashMap::from([(
                validator.clone(),
                models::rust::bond_generation::BondGeneration::GENESIS,
            )]),
            active_validators: vec![validator.clone()],
        };
        snapshot.deploys_in_scope = std::sync::Arc::new(DashSet::new());
        snapshot
    };

    let make_deploy_id_par = |sig: &[u8]| -> models::rhoapi::Par {
        models::rhoapi::Par {
            unforgeables: vec![models::rhoapi::GUnforgeable {
                unf_instance: Some(models::rhoapi::g_unforgeable::UnfInstance::GDeployIdBody(
                    models::rhoapi::GDeployId { sig: sig.to_vec() },
                )),
            }],
            ..Default::default()
        }
    };

    let bridge_rho = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/resources/bridge.rho"),
    )
    .expect("Failed to read bridge.rho");

    let key_a = construct_deploy::DEFAULT_SEC.clone();
    let key_b = construct_deploy::DEFAULT_SEC2.clone();

    let trivial_rho = r#"
new deployId(`rho:system:deployId`) in {
  deployId!("descendant-tag")
}
"#
    .to_string();

    // ── Block A: bridge deployed by key_a, parent = genesis ──
    let deploy_a = protocol_v6_envelope(
        construct_deploy::source_deploy_now_full(
            bridge_rho.clone(),
            None,
            None,
            Some(key_a.clone()),
            None,
            None,
        )
        .unwrap(),
        key_a.clone(),
    );
    let block_a_raw = block_implicits::get_random_block(
        Some(1),
        Some(1),
        Some(genesis_state.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(genesis_block.header.version),
        Some(now_millis()),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty_from_cosigned(&deploy_a)]),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );
    let (_, post_state_a, pd_a, _, sys_pd_a, bonds_a) = compute_deploys_checkpoint_cosigned(
        &mut block_store,
        vec![genesis_block.clone()],
        vec![deploy_a],
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &mk_snapshot(&genesis_hash),
        &rm,
        BlockData::from_block(&block_a_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute block A");
    assert!(
        !pd_a[0].is_failed,
        "Bridge A failed: {:?}",
        pd_a[0].system_deploy_error
    );
    let mut block_a = block_a_raw;
    block_a.body.state.post_state_hash = post_state_a.clone();
    block_a.body.deploys = pd_a.clone();
    block_a.body.system_deploys = sys_pd_a;
    block_a.body.state.bonds = bonds_a;
    block_store.put_block_message(&block_a).expect("store A");
    dag_storage
        .insert(&block_a, InsertMode::Normal)
        .expect("dag A");

    // ── Block B: bridge deployed by key_b, parent = genesis (sibling of A) ──
    let deploy_b = protocol_v6_envelope(
        construct_deploy::source_deploy_now_full(
            bridge_rho,
            None,
            None,
            Some(key_b.clone()),
            None,
            None,
        )
        .unwrap(),
        key_b.clone(),
    );
    let block_b_raw = block_implicits::get_random_block(
        Some(1),
        Some(2),
        Some(genesis_state.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(genesis_block.header.version),
        Some(now_millis()),
        Some(vec![genesis_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty_from_cosigned(&deploy_b)]),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );
    let (_, post_state_b, pd_b, _, sys_pd_b, bonds_b) = compute_deploys_checkpoint_cosigned(
        &mut block_store,
        vec![genesis_block.clone()],
        vec![deploy_b],
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &mk_snapshot(&genesis_hash),
        &rm,
        BlockData::from_block(&block_b_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute block B");
    assert!(
        !pd_b[0].is_failed,
        "Bridge B failed: {:?}",
        pd_b[0].system_deploy_error
    );
    let mut block_b = block_b_raw;
    block_b.body.state.post_state_hash = post_state_b.clone();
    block_b.body.deploys = pd_b.clone();
    block_b.body.system_deploys = sys_pd_b;
    block_b.body.state.bonds = bonds_b;
    block_store.put_block_message(&block_b).expect("store B");
    dag_storage
        .insert(&block_b, InsertMode::Normal)
        .expect("dag B");

    // ── Block C: trivial deploy by key_a, parent = A ──
    let deploy_c = protocol_v6_envelope(
        construct_deploy::source_deploy_now_full(
            trivial_rho.clone(),
            None,
            None,
            Some(key_a.clone()),
            None,
            None,
        )
        .unwrap(),
        key_a,
    );
    let block_c_raw = block_implicits::get_random_block(
        Some(2),
        Some(3),
        Some(post_state_a.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(genesis_block.header.version),
        Some(now_millis()),
        Some(vec![block_a.block_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty_from_cosigned(&deploy_c)]),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );
    let (_, post_state_c, pd_c, _, sys_pd_c, bonds_c) = compute_deploys_checkpoint_cosigned(
        &mut block_store,
        vec![block_a.clone()],
        vec![deploy_c],
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &mk_snapshot(&genesis_hash),
        &rm,
        BlockData::from_block(&block_c_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute block C");
    assert!(
        !pd_c[0].is_failed,
        "Trivial C failed: {:?}",
        pd_c[0].system_deploy_error
    );
    let mut block_c = block_c_raw;
    block_c.body.state.post_state_hash = post_state_c.clone();
    block_c.body.deploys = pd_c.clone();
    block_c.body.system_deploys = sys_pd_c;
    block_c.body.state.bonds = bonds_c;
    block_store.put_block_message(&block_c).expect("store C");
    dag_storage
        .insert(&block_c, InsertMode::Normal)
        .expect("dag C");

    // ── Block D: trivial deploy by key_b, parent = B ──
    let deploy_d = protocol_v6_envelope(
        construct_deploy::source_deploy_now_full(
            trivial_rho,
            None,
            None,
            Some(key_b.clone()),
            None,
            None,
        )
        .unwrap(),
        key_b,
    );
    let block_d_raw = block_implicits::get_random_block(
        Some(2),
        Some(4),
        Some(post_state_b.clone()),
        Some(StateHash::default()),
        Some(validator.clone()),
        Some(genesis_block.header.version),
        Some(now_millis()),
        Some(vec![block_b.block_hash.clone()]),
        Some(Vec::new()),
        Some(vec![ProcessedDeploy::empty_from_cosigned(&deploy_d)]),
        Some(Vec::new()),
        Some(genesis_bonds.clone()),
        Some(shard_name.clone()),
        None,
    );
    let (_, post_state_d, pd_d, _, sys_pd_d, bonds_d) = compute_deploys_checkpoint_cosigned(
        &mut block_store,
        vec![block_b.clone()],
        vec![deploy_d],
        Vec::<casper::rust::util::rholang::system_deploy_enum::SystemDeployEnum>::new(),
        &mk_snapshot(&genesis_hash),
        &rm,
        BlockData::from_block(&block_d_raw),
        HashMap::new(),
        None,
    )
    .await
    .expect("compute block D");
    assert!(
        !pd_d[0].is_failed,
        "Trivial D failed: {:?}",
        pd_d[0].system_deploy_error
    );
    let mut block_d = block_d_raw;
    block_d.body.state.post_state_hash = post_state_d.clone();
    block_d.body.deploys = pd_d.clone();
    block_d.body.system_deploys = sys_pd_d;
    block_d.body.state.bonds = bonds_d;
    block_store.put_block_message(&block_d).expect("store D");
    dag_storage
        .insert(&block_d, InsertMode::Normal)
        .expect("dag D");

    // ── Merge [C, D] — simulates what a validator would compute when proposing
    //    a multi-parent block with parents [BC, BD]. LCA is genesis.
    let snapshot_cd = mk_snapshot(&genesis_hash);
    let latest_messages: std::collections::BTreeMap<_, _> = snapshot_cd
        .justifications
        .iter()
        .map(|j| (j.validator.clone(), j.latest_block_hash.clone()))
        .collect();
    let merged = compute_parents_post_state(
        &block_store,
        vec![block_c.clone(), block_d.clone()],
        &snapshot_cd,
        &rm,
        &latest_messages,
        None,
        None,
        None,
        None,
    )
    .await
    .expect("merge [C, D]");
    let merged_state = merged.state;
    let rejected = merged.rejected_user;

    let rejected_set: HashSet<prost::bytes::Bytes> = rejected
        .iter()
        .map(|item| prost::bytes::Bytes::copy_from_slice(item.deploy_id()))
        .collect();
    let ba_rejected = rejected_set.contains(&pd_a[0].deploy.sig);
    let bb_rejected = rejected_set.contains(&pd_b[0].deploy.sig);
    let bc_rejected = rejected_set.contains(&pd_c[0].deploy.sig);
    let bd_rejected = rejected_set.contains(&pd_d[0].deploy.sig);

    tracing::info!("──────── Rejection outcome ────────");
    tracing::info!(
        "BA (bridge, key_A)                 rejected: {}",
        ba_rejected
    );
    tracing::info!(
        "BB (bridge, key_B)                 rejected: {}",
        bb_rejected
    );
    tracing::info!(
        "BC (trivial, key_A, child of BA)   rejected: {}",
        bc_rejected
    );
    tracing::info!(
        "BD (trivial, key_B, child of BB)   rejected: {}",
        bd_rejected
    );
    tracing::info!("Total rejected: {} deploys", rejected.len());

    let ba_data = rm
        .get_data(
            merged_state.clone(),
            &make_deploy_id_par(&pd_a[0].deploy.sig),
        )
        .await
        .unwrap();
    let bb_data = rm
        .get_data(
            merged_state.clone(),
            &make_deploy_id_par(&pd_b[0].deploy.sig),
        )
        .await
        .unwrap();
    let bc_data = rm
        .get_data(
            merged_state.clone(),
            &make_deploy_id_par(&pd_c[0].deploy.sig),
        )
        .await
        .unwrap();
    let bd_data = rm
        .get_data(
            merged_state.clone(),
            &make_deploy_id_par(&pd_d[0].deploy.sig),
        )
        .await
        .unwrap();

    tracing::info!("──────── State presence in merged post-state ────────");
    tracing::info!("BA bridge data  pars: {}", ba_data.len());
    tracing::info!("BB bridge data  pars: {}", bb_data.len());
    tracing::info!("BC trivial data pars: {}", bc_data.len());
    tracing::info!("BD trivial data pars: {}", bd_data.len());

    let bc_orphaned = ba_rejected && !bc_rejected && ba_data.is_empty() && !bc_data.is_empty();
    let bd_orphaned = bb_rejected && !bd_rejected && bb_data.is_empty() && !bd_data.is_empty();

    assert!(
        !bc_orphaned && !bd_orphaned,
        "STALE-DIFF BUG REPRODUCED: descendant of rejected block has state present \
         in merged post-state while its ancestor's state is absent. \
         bc_orphaned={} (ba_rejected={}, bc_rejected={}, ba_empty={}, bc_present={}); \
         bd_orphaned={} (bb_rejected={}, bd_rejected={}, bb_empty={}, bd_present={}).",
        bc_orphaned,
        ba_rejected,
        bc_rejected,
        ba_data.is_empty(),
        !bc_data.is_empty(),
        bd_orphaned,
        bb_rejected,
        bd_rejected,
        bb_data.is_empty(),
        !bd_data.is_empty(),
    );
}

// =====================================================================
// Cost-Accounted Rho — authenticated protocol mint into canonical SystemVault
// custody.
//
// `mintPhlogiston(@validatorPk, @amount, @sysAuthToken, return)` is gated by
// `sysAuthTokenOps!("check", ...)`: true iff `sysAuthToken` is a
// `GSysAuthToken`, which is constructible ONLY by Rust system deploys via
// `mk_sys_auth_token` (system_deploy.rs). On a valid token the contract mints
// `amount` into the vault derived from `validatorPk`, then `return!(true)`; on
// an invalid token it changes no balance and returns an authorization error.
// =====================================================================

/// Minimal `SystemDeployTrait` that drives `PoS!("mintPhlogiston", ...)` with
/// a REAL `GSysAuthToken` (supplied by the inherited `mk_sys_auth_token`). The
/// validator pubkey bytes are injected via a dedicated fixed channel binding
/// (`sys:casper:mintValidatorPk`) and forwarded as the `@validatorPk` argument.
/// On success the contract returns the bare boolean `true`, so the deploy's
/// `Output` is `RhoBoolean`. This harness directly exercises the same
/// authenticated PoS mint entry point used by epoch processing.
struct MintPhlogistonDeploy {
    validator_pk: crypto::rust::public_key::PublicKey,
    amount: i64,
    rand: Blake2b512Random,
}

impl SystemDeployTrait for MintPhlogistonDeploy {
    type Output = RhoBoolean;
    type Result = bool;

    fn source() -> &'static str {
        r#"
          new rl(`rho:registry:lookup`),
          poSCh,
          mintValidatorPk(`sys:casper:mintValidatorPk`),
          mintAmount(`sys:casper:mintAmount`),
          sysAuthToken(`sys:casper:authToken`),
          return(`sys:casper:return`)
          in {
            rl!(`rho:system:pos`, *poSCh) |
            for(@(_, PoS) <- poSCh) {
              @PoS!("mintPhlogiston", *mintValidatorPk, *mintAmount, *sysAuthToken, *return)
            }
        }"#
    }

    fn process_result(
        value: <Self::Output as Extractor>::RustType,
    ) -> Either<SystemDeployUserError, Self::Result> {
        Either::Right(value)
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

    fn rand(&self) -> Blake2b512Random { self.rand.clone() }

    fn env(&mut self) -> HashMap<String, Par> {
        let mut env = HashMap::new();

        env.insert(
            "sys:casper:mintValidatorPk".to_string(),
            models::rust::utils::new_gbytearray_par(
                self.validator_pk.bytes.to_vec(),
                Vec::new(),
                false,
            ),
        );
        env.insert(
            "sys:casper:mintAmount".to_string(),
            models::rust::utils::new_gint_par(self.amount, Vec::new(), false),
        );

        let (sys_key, sys_value) = self.mk_sys_auth_token();
        env.insert(sys_key, sys_value);

        let (ret_key, ret_value) = self.mk_return_channel();
        env.insert(ret_key, ret_value);

        env
    }

    fn return_channel(&mut self) -> Result<Par, CasperError> {
        match self.env().get("sys:casper:return") {
            Some(par) => Ok(par.clone()),
            None => Err(CasperError::RuntimeError(
                "Return channel not found. This is a compile time error.".to_string(),
            )),
        }
    }
}

struct MutateThenRejectSystemDeploy {
    rand: Blake2b512Random,
}

impl SystemDeployTrait for MutateThenRejectSystemDeploy {
    type Output = RhoBoolean;
    type Result = ();

    fn source() -> &'static str {
        r#"
          new return(`sys:casper:return`) in {
            @"system-deploy-rollback-probe"!(1) |
            return!(false)
          }
        "#
    }

    fn process_result(
        value: <Self::Output as Extractor>::RustType,
    ) -> Either<SystemDeployUserError, Self::Result> {
        if value {
            Either::Right(())
        } else {
            Either::Left(SystemDeployUserError::new(
                "intentional system deploy rejection".to_string(),
            ))
        }
    }

    fn as_any(&self) -> &dyn std::any::Any { self }

    fn rand(&self) -> Blake2b512Random { self.rand.clone() }

    fn env(&mut self) -> HashMap<String, Par> {
        let mut env = HashMap::new();
        let (return_key, return_value) = self.mk_return_channel();
        env.insert(return_key, return_value);
        env
    }

    fn return_channel(&mut self) -> Result<Par, CasperError> {
        self.env()
            .get("sys:casper:return")
            .cloned()
            .ok_or_else(|| CasperError::RuntimeError("return channel not found".to_string()))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejected_system_deploy_restores_pre_state_root() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let runtime = runtime_manager.spawn_runtime().await;
            runtime
                .set_block_data(BlockData {
                    time_stamp: 0,
                    block_number: 1,
                    sender: genesis_context.validator_pks()[0].clone(),
                    seq_num: 1,
                })
                .await;
            let mut ops = RuntimeOps::new(runtime);
            let result = ops
                .play_system_deploy(&start_state, &mut MutateThenRejectSystemDeploy {
                    rand: Blake2b512Random::create_from_bytes(&[0xE1]),
                })
                .await
                .unwrap();
            assert!(matches!(result, SystemDeployResult::PlayFailed { .. }));
            let current_root = ops.runtime.create_checkpoint().await.root.to_bytes_prost();
            assert_eq!(current_root, start_state);
        },
    )
    .await
    .unwrap()
}

/// Accept path: a system deploy holding a real `GSysAuthToken` credits exactly
/// the requested amount to the validator's canonical SystemVault.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mintphlogiston_accepts_valid_sys_auth_token_and_credits_canonical_vault() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let validator_pk = genesis_context.validator_pks()[0].clone();
            let validator_address = VaultAddress::from_public_key(&validator_pk).unwrap();
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let balance_before =
                system_vault_balance(&runtime_manager, &start_state, &validator_address).await;

            let runtime = runtime_manager.spawn_runtime().await;
            runtime
                .set_block_data(BlockData {
                    time_stamp: 0,
                    block_number: 0,
                    sender: genesis_context.validator_pks()[0].clone(),
                    seq_num: 0,
                })
                .await;
            let mut runtime_ops = RuntimeOps::new(runtime);

            let result = runtime_ops
                .play_system_deploy(&start_state, &mut MintPhlogistonDeploy {
                    validator_pk: validator_pk.clone(),
                    amount: 1_000,
                    rand: Blake2b512Random::create_from_bytes(&[0xA1]),
                })
                .await
                .expect("mintPhlogiston system deploy must play");

            let state_hash = match result {
                SystemDeployResult::PlaySucceeded {
                    state_hash, result, ..
                } => {
                    assert!(result, "an authorized mint must return true");
                    state_hash
                }
                other => panic!(
                    "authorized mintPhlogiston must succeed as a system deploy; got {:?}",
                    std::mem::discriminant(&other)
                ),
            };
            assert_eq!(
                system_vault_balance(&runtime_manager, &state_hash, &validator_address).await,
                balance_before + 1_000
            );
        },
    )
    .await
    .unwrap()
}

/// REJECT path: an exploratory (user) deploy cannot bind the `sys:casper:*`
/// fixed channels, so it cannot fabricate a `GSysAuthToken`. Passing any
/// non-token value to `mintPhlogiston` drives the authorization check to
/// false; the contract returns `(false, "unauthorized mint")` and changes no
/// vault balance. `play_exploratory_deploy` captures the data sent on the
/// FIRST private name created in the term (our `return` channel).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mintphlogiston_rejects_forged_or_absent_sys_auth_token() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let validator_pk_hex = hex::encode(&genesis_context.validator_pks()[0].bytes);

            // `return` is the FIRST `new` name, so it is the channel
            // `play_exploratory_deploy` captures. `forgedToken` is a fresh
            // unforgeable name — NOT a GSysAuthToken — standing in for any
            // value a non-system caller could supply.
            let term = format!(
                r#"
                new return, poSCh, forgedToken,
                    rl(`rho:registry:lookup`)
                in {{
                  rl!(`rho:system:pos`, *poSCh) |
                  for (@(_, PoS) <- poSCh) {{
                    @PoS!("mintPhlogiston", "{}".hexToBytes(), 1000, *forgedToken, *return)
                  }}
                }}"#,
                validator_pk_hex
            );

            let (results, _cost) = runtime_manager
                .play_exploratory_deploy(term, &genesis_block.body.state.post_state_hash, None)
                .await
                .expect("exploratory mintPhlogiston term must execute");

            // The captured return value must be the rejection tuple
            // (false, "unauthorized mint") — never a success.
            assert!(
                !results.is_empty(),
                "the rejection result must be sent on the return channel"
            );
            let printed = format!("{:?}", results);
            assert!(
                printed.contains("unauthorized mint"),
                "an unauthorized mint must return (false, \"unauthorized mint\"); got: {}",
                printed
            );
        },
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn protocol_burn_debits_canonical_vault_and_rejects_overburn() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let validator = genesis_context.validator_pks()[0].clone();
            let address = VaultAddress::from_public_key(&validator).unwrap();
            let start_state = genesis_block.body.state.post_state_hash.clone();
            let initial = system_vault_balance(&runtime_manager, &start_state, &address).await;
            assert!(initial > 17);

            let runtime = runtime_manager.spawn_runtime().await;
            runtime
                .set_block_data(BlockData {
                    time_stamp: 0,
                    block_number: 0,
                    sender: validator.clone(),
                    seq_num: 20,
                })
                .await;
            let mut ops = RuntimeOps::new(runtime);
            let mut burn = ProtocolBurnDeploy::new(
                address.to_base58(),
                17,
                Blake2b512Random::create_from_bytes(&[0xB1]),
            )
            .unwrap();
            let burned_state = match ops
                .play_system_deploy(&start_state, &mut burn)
                .await
                .unwrap()
            {
                SystemDeployResult::PlaySucceeded { state_hash, .. } => state_hash,
                SystemDeployResult::PlayFailed {
                    processed_system_deploy,
                } => panic!("authorized protocol burn failed: {processed_system_deploy:?}"),
            };
            assert_eq!(
                system_vault_balance(&runtime_manager, &burned_state, &address).await,
                initial - 17
            );

            let overburn_runtime = runtime_manager.spawn_runtime().await;
            overburn_runtime
                .set_block_data(BlockData {
                    time_stamp: 0,
                    block_number: 0,
                    sender: validator,
                    seq_num: 21,
                })
                .await;
            let mut overburn_ops = RuntimeOps::new(overburn_runtime);
            let mut overburn = ProtocolBurnDeploy::new(
                address.to_base58(),
                initial - 16,
                Blake2b512Random::create_from_bytes(&[0xB2]),
            )
            .unwrap();
            assert!(matches!(
                overburn_ops
                    .play_system_deploy(&burned_state, &mut overburn)
                    .await
                    .unwrap(),
                SystemDeployResult::PlayFailed { .. }
            ));
            assert_eq!(
                system_vault_balance(&runtime_manager, &burned_state, &address).await,
                initial - 17
            );
        },
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn protocol_burn_rejects_forged_system_authority() {
    with_runtime_manager(
        |runtime_manager, genesis_context, genesis_block| async move {
            let address =
                VaultAddress::from_public_key(&genesis_context.validator_pks()[0]).unwrap();
            let term = format!(
                r#"
                new return, systemVaultCh, forgedToken,
                    rl(`rho:registry:lookup`)
                in {{
                  rl!(`rho:vault:system`, *systemVaultCh) |
                  for (@(_, SystemVault) <- systemVaultCh) {{
                    @SystemVault!("protocolBurn", "{}", 1, *forgedToken, *return)
                  }}
                }}"#,
                address.to_base58()
            );
            let (results, _) = runtime_manager
                .play_exploratory_deploy(term, &genesis_block.body.state.post_state_hash, None)
                .await
                .unwrap();
            assert!(!results.is_empty());
            assert!(format!("{results:?}").contains("Unauthorized or invalid protocol burn"));
        },
    )
    .await
    .unwrap()
}
